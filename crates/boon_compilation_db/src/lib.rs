#![forbid(unsafe_code)]

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

pub type RequestFingerprint = [u8; 32];

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Revision(pub u64);

impl Revision {
    pub fn next(self) -> Result<Self, RequestGraphError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| RequestGraphError::new("compilation database revision overflow"))
    }
}

/// Revision metadata for one stable owner/projection request.
///
/// Values remain owned by the language component. This memo records only the
/// fingerprints and currentness needed by both proof sealing and later warm
/// backdating, so the database does not become a second semantic authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestMemo {
    pub changed_at: Revision,
    pub verified_at: Revision,
    pub input_fingerprint: RequestFingerprint,
    pub result_fingerprint: RequestFingerprint,
}

impl RequestMemo {
    pub const fn new(
        revision: Revision,
        input_fingerprint: RequestFingerprint,
        result_fingerprint: RequestFingerprint,
    ) -> Self {
        Self {
            changed_at: revision,
            verified_at: revision,
            input_fingerprint,
            result_fingerprint,
        }
    }

    /// Publishes a recomputed request at `revision`.
    ///
    /// An unchanged result is backdated by retaining `changed_at`; callers can
    /// therefore keep downstream requests green even when an input was
    /// conservatively invalidated and re-executed.
    pub fn publish(
        &mut self,
        revision: Revision,
        input_fingerprint: RequestFingerprint,
        result_fingerprint: RequestFingerprint,
    ) -> bool {
        let changed = self.result_fingerprint != result_fingerprint;
        if changed {
            self.changed_at = revision;
        }
        self.verified_at = revision;
        self.input_fingerprint = input_fingerprint;
        self.result_fingerprint = result_fingerprint;
        changed
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RequestGraphDigestDomains<'a> {
    pub component: &'a [u8],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RequestGraphStats {
    pub nodes: usize,
    pub edges: usize,
    pub components: usize,
    pub cyclic_components: usize,
    pub maximum_component_nodes: usize,
    pub component_edges: usize,
}

#[derive(Clone, Copy, Debug)]
struct PendingRequestNode {
    identity_digest: RequestFingerprint,
    local_digest: RequestFingerprint,
}

/// Cold and warm request graph builder at stable owner/projection granularity.
///
/// Rows inside a request are folded by the language-owned table builder before
/// insertion. This graph deliberately does not expose per-expression queries.
pub struct RequestGraphBuilder<K> {
    nodes: BTreeMap<K, PendingRequestNode>,
    dependencies: Vec<(K, K)>,
}

impl<K> Default for RequestGraphBuilder<K> {
    fn default() -> Self {
        Self {
            nodes: BTreeMap::new(),
            dependencies: Vec::new(),
        }
    }
}

impl<K: Clone + Ord> RequestGraphBuilder<K> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(
        &mut self,
        key: K,
        identity_digest: RequestFingerprint,
        local_digest: RequestFingerprint,
    ) -> Result<(), RequestGraphError> {
        if self
            .nodes
            .insert(
                key,
                PendingRequestNode {
                    identity_digest,
                    local_digest,
                },
            )
            .is_some()
        {
            return Err(RequestGraphError::new(
                "compilation request graph contains a duplicate key",
            ));
        }
        Ok(())
    }

    pub fn add_dependency(&mut self, request: K, dependency: K) {
        self.dependencies.push((request, dependency));
    }

    pub fn seal(
        self,
        domains: RequestGraphDigestDomains<'_>,
    ) -> Result<SealedRequestGraph<K>, RequestGraphError> {
        if domains.component.is_empty() {
            return Err(RequestGraphError::new(
                "compilation request graph component digest domain is empty",
            ));
        }

        let mut keys = Vec::with_capacity(self.nodes.len());
        let mut identity_digests = Vec::with_capacity(self.nodes.len());
        let mut local_digests = Vec::with_capacity(self.nodes.len());
        let mut ordinals = BTreeMap::new();
        for (key, node) in self.nodes {
            let ordinal = keys.len();
            ordinals.insert(key.clone(), ordinal);
            keys.push(key);
            identity_digests.push(node.identity_digest);
            local_digests.push(node.local_digest);
        }

        let mut edges = vec![Vec::new(); keys.len()];
        for (request, dependency) in self.dependencies {
            let request = ordinals.get(&request).copied().ok_or_else(|| {
                RequestGraphError::new("compilation request graph edge has an unregistered source")
            })?;
            let dependency = ordinals.get(&dependency).copied().ok_or_else(|| {
                RequestGraphError::new("compilation request graph edge has an unregistered target")
            })?;
            edges[request].push(dependency);
        }
        for targets in &mut edges {
            targets.sort_unstable();
            targets.dedup();
        }
        let edge_count = edges.iter().map(Vec::len).sum::<usize>();
        let (component_by_node, components) = strongly_connected_components(&edges)?;
        let representatives = components
            .iter()
            .map(|members| {
                members.first().copied().ok_or_else(|| {
                    RequestGraphError::new("compilation request graph produced an empty component")
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut component_digests = vec![None; components.len()];
        let mut dependencies = Vec::new();
        let mut component_edge_count = 0usize;
        for component in 0..components.len() {
            let members = components.members(component).ok_or_else(|| {
                RequestGraphError::new(format!(
                    "compilation request graph has no component {component}"
                ))
            })?;
            dependencies.clear();
            for member in members.iter().copied() {
                for target in edges[member].iter().copied() {
                    let target_component = component_by_node[target];
                    if target_component != component {
                        dependencies.push(target_component);
                    }
                }
            }
            dependencies.sort_unstable_by_key(|target| representatives[*target]);
            dependencies.dedup();
            component_edge_count = component_edge_count
                .checked_add(dependencies.len())
                .ok_or_else(|| {
                    RequestGraphError::new("compilation request component edge count overflow")
                })?;
            if dependencies
                .iter()
                .any(|dependency| *dependency >= component)
            {
                return Err(RequestGraphError::new(format!(
                    "compilation request Tarjan order is not dependency-first for component {component}"
                )));
            }

            let mut hasher = Sha256::new();
            hasher.update(domains.component);
            update_count(&mut hasher, members.len(), "component member count")?;
            for member in members.iter().copied() {
                hasher.update(identity_digests[member]);
                hasher.update(local_digests[member]);
            }
            update_count(
                &mut hasher,
                dependencies.len(),
                "component dependency count",
            )?;
            for dependency in dependencies.iter().copied() {
                hasher.update(identity_digests[representatives[dependency]]);
                hasher.update(component_digests[dependency].ok_or_else(|| {
                    RequestGraphError::new(
                        "compilation request component was hashed before its dependency",
                    )
                })?);
            }
            component_digests[component] = Some(hasher.finalize().into());
        }

        let edge_offsets = adjacency_offsets(&edges)?;
        let edge_arena = edges.into_iter().flatten().collect::<Vec<_>>();
        let reverse_edges = reverse_adjacency(keys.len(), &edge_offsets, &edge_arena)?;
        let stats = RequestGraphStats {
            nodes: keys.len(),
            edges: edge_count,
            components: components.len(),
            cyclic_components: components
                .iter()
                .filter(|members| members.len() > 1)
                .count(),
            maximum_component_nodes: components.iter().map(<[usize]>::len).max().unwrap_or(0),
            component_edges: component_edge_count,
        };

        Ok(SealedRequestGraph {
            keys,
            ordinals,
            identity_digests,
            edge_offsets,
            edge_arena,
            reverse_edge_offsets: reverse_edges.0,
            reverse_edge_arena: reverse_edges.1,
            component_by_node,
            component_member_offsets: components.member_offsets,
            component_members: components.members,
            component_digests: component_digests
                .into_iter()
                .map(|digest| {
                    digest.ok_or_else(|| {
                        RequestGraphError::new("compilation request component has no sealed digest")
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            component_representatives: representatives,
            stats,
        })
    }
}

pub struct SealedRequestGraph<K> {
    keys: Vec<K>,
    ordinals: BTreeMap<K, usize>,
    identity_digests: Vec<RequestFingerprint>,
    edge_offsets: Vec<usize>,
    edge_arena: Vec<usize>,
    reverse_edge_offsets: Vec<usize>,
    reverse_edge_arena: Vec<usize>,
    component_by_node: Vec<usize>,
    component_member_offsets: Vec<usize>,
    component_members: Vec<usize>,
    component_digests: Vec<RequestFingerprint>,
    component_representatives: Vec<usize>,
    stats: RequestGraphStats,
}

impl<K: Ord> SealedRequestGraph<K> {
    pub const fn stats(&self) -> RequestGraphStats {
        self.stats
    }

    pub fn dependencies(&self, key: &K) -> Option<impl Iterator<Item = &K>> {
        let ordinal = *self.ordinals.get(key)?;
        let start = self.edge_offsets[ordinal];
        let end = self.edge_offsets[ordinal + 1];
        Some(
            self.edge_arena[start..end]
                .iter()
                .map(|target| &self.keys[*target]),
        )
    }

    pub fn reverse_dependents(&self, key: &K) -> Option<impl Iterator<Item = &K>> {
        let ordinal = *self.ordinals.get(key)?;
        let start = self.reverse_edge_offsets[ordinal];
        let end = self.reverse_edge_offsets[ordinal + 1];
        Some(
            self.reverse_edge_arena[start..end]
                .iter()
                .map(|source| &self.keys[*source]),
        )
    }

    pub fn implementation_digest(
        &self,
        key: &K,
        domain: &[u8],
    ) -> Result<RequestFingerprint, RequestGraphError> {
        if domain.is_empty() {
            return Err(RequestGraphError::new(
                "compilation request implementation digest domain is empty",
            ));
        }
        let node = *self.ordinals.get(key).ok_or_else(|| {
            RequestGraphError::new("compilation request implementation key is absent")
        })?;
        let component = self.component_by_node[node];
        let representative = self.component_representatives[component];
        let mut hasher = Sha256::new();
        hasher.update(domain);
        hasher.update(self.identity_digests[node]);
        hasher.update(self.identity_digests[representative]);
        hasher.update(self.component_digests[component]);
        Ok(hasher.finalize().into())
    }

    pub fn component_members(&self, key: &K) -> Option<impl Iterator<Item = &K>> {
        let node = *self.ordinals.get(key)?;
        let component = self.component_by_node[node];
        let start = self.component_member_offsets[component];
        let end = self.component_member_offsets[component + 1];
        Some(
            self.component_members[start..end]
                .iter()
                .map(|member| &self.keys[*member]),
        )
    }
}

struct Components {
    member_offsets: Vec<usize>,
    members: Vec<usize>,
}

impl Components {
    fn with_node_capacity(node_count: usize) -> Self {
        let mut member_offsets = Vec::with_capacity(node_count.saturating_add(1));
        member_offsets.push(0);
        Self {
            member_offsets,
            members: Vec::with_capacity(node_count),
        }
    }

    fn len(&self) -> usize {
        self.member_offsets.len() - 1
    }

    fn members(&self, component: usize) -> Option<&[usize]> {
        let start = *self.member_offsets.get(component)?;
        let end = *self.member_offsets.get(component + 1)?;
        self.members.get(start..end)
    }

    fn iter(&self) -> impl Iterator<Item = &[usize]> {
        self.member_offsets
            .windows(2)
            .map(|range| &self.members[range[0]..range[1]])
    }

    fn push_from_active_until(
        &mut self,
        active: &mut Vec<usize>,
        root: usize,
    ) -> Result<(), RequestGraphError> {
        let start = self.members.len();
        loop {
            let member = active.pop().ok_or_else(|| {
                RequestGraphError::new("compilation request component stack underflow")
            })?;
            self.members.push(member);
            if member == root {
                break;
            }
        }
        self.members[start..].sort_unstable();
        self.member_offsets.push(self.members.len());
        Ok(())
    }
}

fn strongly_connected_components(
    edges: &[Vec<usize>],
) -> Result<(Vec<usize>, Components), RequestGraphError> {
    let node_count = edges.len();
    let mut discovery_index = vec![usize::MAX; node_count];
    let mut low_link = vec![usize::MAX; node_count];
    let mut component_by_node = vec![usize::MAX; node_count];
    let mut components = Components::with_node_capacity(node_count);
    let mut active = Vec::new();
    let mut pending = Vec::new();
    let mut next_discovery_index = 0usize;

    for start in 0..node_count {
        if discovery_index[start] != usize::MAX {
            continue;
        }
        discovery_index[start] = next_discovery_index;
        low_link[start] = next_discovery_index;
        next_discovery_index = next_discovery_index.checked_add(1).ok_or_else(|| {
            RequestGraphError::new("compilation request discovery index overflow")
        })?;
        active.push(start);
        pending.push((start, 0usize));

        while !pending.is_empty() {
            let frame = pending.len() - 1;
            let (node, next_edge) = pending[frame];
            let targets = edges.get(node).ok_or_else(|| {
                RequestGraphError::new(format!(
                    "compilation request graph references missing node {node}"
                ))
            })?;
            if next_edge < targets.len() {
                pending[frame].1 += 1;
                let target = targets[next_edge];
                if target >= node_count {
                    return Err(RequestGraphError::new(format!(
                        "compilation request graph references missing target {target}"
                    )));
                }
                if discovery_index[target] == usize::MAX {
                    discovery_index[target] = next_discovery_index;
                    low_link[target] = next_discovery_index;
                    next_discovery_index =
                        next_discovery_index.checked_add(1).ok_or_else(|| {
                            RequestGraphError::new("compilation request discovery index overflow")
                        })?;
                    active.push(target);
                    pending.push((target, 0usize));
                } else if component_by_node[target] == usize::MAX {
                    low_link[node] = low_link[node].min(discovery_index[target]);
                }
                continue;
            }

            pending.pop();
            if let Some((parent, _)) = pending.last().copied() {
                low_link[parent] = low_link[parent].min(low_link[node]);
            }
            if low_link[node] != discovery_index[node] {
                continue;
            }
            let component = components.len();
            components.push_from_active_until(&mut active, node)?;
            for member in components
                .members(component)
                .expect("fresh compilation request component has members")
            {
                component_by_node[*member] = component;
            }
        }
    }
    if !active.is_empty() {
        return Err(RequestGraphError::new(
            "compilation request component stack is not empty after traversal",
        ));
    }
    Ok((component_by_node, components))
}

fn adjacency_offsets(edges: &[Vec<usize>]) -> Result<Vec<usize>, RequestGraphError> {
    let mut offsets = Vec::with_capacity(edges.len().saturating_add(1));
    offsets.push(0usize);
    for targets in edges {
        let next = offsets
            .last()
            .copied()
            .expect("request adjacency has an initial offset")
            .checked_add(targets.len())
            .ok_or_else(|| RequestGraphError::new("request adjacency offset overflow"))?;
        offsets.push(next);
    }
    Ok(offsets)
}

fn reverse_adjacency(
    node_count: usize,
    offsets: &[usize],
    edges: &[usize],
) -> Result<(Vec<usize>, Vec<usize>), RequestGraphError> {
    let mut counts = vec![0usize; node_count];
    for target in edges.iter().copied() {
        let count = counts.get_mut(target).ok_or_else(|| {
            RequestGraphError::new("request reverse adjacency target is out of range")
        })?;
        *count = count
            .checked_add(1)
            .ok_or_else(|| RequestGraphError::new("request reverse degree overflow"))?;
    }
    let mut reverse_offsets = Vec::with_capacity(node_count.saturating_add(1));
    reverse_offsets.push(0usize);
    for count in &counts {
        let next = reverse_offsets
            .last()
            .copied()
            .expect("request reverse adjacency has an initial offset")
            .checked_add(*count)
            .ok_or_else(|| RequestGraphError::new("request reverse offset overflow"))?;
        reverse_offsets.push(next);
    }
    for (node, count) in counts.iter_mut().enumerate() {
        *count = reverse_offsets[node];
    }
    let mut reverse_edges = vec![0usize; edges.len()];
    for source in 0..node_count {
        let start = offsets[source];
        let end = offsets[source + 1];
        for target in edges[start..end].iter().copied() {
            let slot = counts[target];
            reverse_edges[slot] = source;
            counts[target] += 1;
        }
    }
    Ok((reverse_offsets, reverse_edges))
}

fn update_count(hasher: &mut Sha256, value: usize, context: &str) -> Result<(), RequestGraphError> {
    let value = u64::try_from(value).map_err(|_| {
        RequestGraphError::new(format!(
            "compilation request {context} exceeds its u64 encoding"
        ))
    })?;
    hasher.update(value.to_be_bytes());
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestGraphError {
    message: String,
}

impl RequestGraphError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for RequestGraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for RequestGraphError {}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPONENT_DOMAIN: &[u8] = b"test.request-component.v1\0";
    const IMPLEMENTATION_DOMAIN: &[u8] = b"test.request-implementation.v1\0";

    fn digest(value: u8) -> RequestFingerprint {
        [value; 32]
    }

    fn graph(nodes: &[(u8, u8)], edges: &[(u8, u8)]) -> SealedRequestGraph<u8> {
        let mut builder = RequestGraphBuilder::new();
        for (key, value) in nodes {
            builder.insert(*key, digest(*key), digest(*value)).unwrap();
        }
        for (source, target) in edges {
            builder.add_dependency(*source, *target);
        }
        builder
            .seal(RequestGraphDigestDomains {
                component: COMPONENT_DOMAIN,
            })
            .unwrap()
    }

    #[test]
    fn dependency_mutation_propagates_without_touching_unrelated_request() {
        let before = graph(&[(1, 11), (2, 22), (3, 33)], &[(1, 2)]);
        let after = graph(&[(1, 11), (2, 44), (3, 33)], &[(1, 2)]);
        assert_ne!(
            before
                .implementation_digest(&1, IMPLEMENTATION_DOMAIN)
                .unwrap(),
            after
                .implementation_digest(&1, IMPLEMENTATION_DOMAIN)
                .unwrap()
        );
        assert_ne!(
            before
                .implementation_digest(&2, IMPLEMENTATION_DOMAIN)
                .unwrap(),
            after
                .implementation_digest(&2, IMPLEMENTATION_DOMAIN)
                .unwrap()
        );
        assert_eq!(
            before
                .implementation_digest(&3, IMPLEMENTATION_DOMAIN)
                .unwrap(),
            after
                .implementation_digest(&3, IMPLEMENTATION_DOMAIN)
                .unwrap()
        );
    }

    #[test]
    fn cycles_are_one_component_and_reverse_edges_are_exact() {
        let graph = graph(&[(1, 11), (2, 22), (3, 33)], &[(1, 2), (2, 1), (3, 1)]);
        assert_eq!(graph.stats().components, 2);
        assert_eq!(graph.stats().cyclic_components, 1);
        assert_eq!(
            graph
                .component_members(&1)
                .unwrap()
                .copied()
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            graph
                .reverse_dependents(&1)
                .unwrap()
                .copied()
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
    }

    #[test]
    fn unchanged_results_are_backdated() {
        let mut memo = RequestMemo::new(Revision(1), digest(1), digest(2));
        assert!(!memo.publish(Revision(2), digest(3), digest(2)));
        assert_eq!(memo.changed_at, Revision(1));
        assert_eq!(memo.verified_at, Revision(2));
        assert!(memo.publish(Revision(3), digest(4), digest(5)));
        assert_eq!(memo.changed_at, Revision(3));
    }

    #[test]
    fn missing_and_duplicate_nodes_fail_closed() {
        let mut duplicate = RequestGraphBuilder::new();
        duplicate.insert(1u8, digest(1), digest(2)).unwrap();
        assert!(duplicate.insert(1u8, digest(1), digest(2)).is_err());

        let mut missing = RequestGraphBuilder::new();
        missing.insert(1u8, digest(1), digest(2)).unwrap();
        missing.add_dependency(1, 2);
        assert!(
            missing
                .seal(RequestGraphDigestDomains {
                    component: COMPONENT_DOMAIN,
                })
                .is_err()
        );
    }
}
