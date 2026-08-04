#![forbid(unsafe_code)]

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;

pub type RequestFingerprint = [u8; 32];

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Revision(pub u64);

impl Revision {
    pub fn next(self) -> Result<Self, CompilationDbError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or_else(|| CompilationDbError::new("compilation database revision overflow"))
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
    pub dependencies_verified_at: Revision,
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
            dependencies_verified_at: revision,
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
        dependencies_verified_at: Revision,
        input_fingerprint: RequestFingerprint,
        result_fingerprint: RequestFingerprint,
    ) -> Result<bool, CompilationDbError> {
        if revision <= self.verified_at {
            return Err(CompilationDbError::new(format!(
                "compilation request publication revision {} is not newer than verified revision {}",
                revision.0, self.verified_at.0,
            )));
        }
        if dependencies_verified_at != revision {
            return Err(CompilationDbError::new(format!(
                "compilation request dependencies are verified at revision {}, expected {}",
                dependencies_verified_at.0, revision.0,
            )));
        }
        let changed = self.result_fingerprint != result_fingerprint;
        if changed {
            self.changed_at = revision;
        }
        self.verified_at = revision;
        self.dependencies_verified_at = dependencies_verified_at;
        self.input_fingerprint = input_fingerprint;
        self.result_fingerprint = result_fingerprint;
        Ok(changed)
    }
}

/// Work performed by a typed request evaluator across all revisions.
///
/// `reused` means a request was verified green without executing. `backdated`
/// means it executed after conservative invalidation but published the same
/// result fingerprint, so its previous `changed_at` revision was retained.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RequestEvaluationStats {
    pub executed: usize,
    pub reused: usize,
    pub backdated: usize,
    pub changed: usize,
    pub removed: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestDisposition {
    Executed,
    Reused,
}

/// Borrowed typed result of one evaluator request.
pub struct RequestEvaluation<'a, V> {
    value: &'a V,
    disposition: RequestDisposition,
}

impl<'a, V> RequestEvaluation<'a, V> {
    pub const fn value(&self) -> &'a V {
        self.value
    }

    pub const fn disposition(&self) -> RequestDisposition {
        self.disposition
    }

    pub const fn was_reused(&self) -> bool {
        matches!(self.disposition, RequestDisposition::Reused)
    }
}

struct TypedRequestSlot<K, V> {
    memo: RequestMemo,
    dependencies: Vec<K>,
    value: V,
}

/// Revisioned evaluator for one language-owned typed key/result family.
///
/// The generic instantiation remains owned by the language component; this
/// crate never erases values through `Any` or serializes them into a second
/// semantic authority. Callers evaluate dependencies first and pass their
/// exact typed keys. The evaluator verifies those dependencies at the current
/// revision, records the span, reuses green values, backdates equal results,
/// and exposes exact reverse cones for invalidation and evidence.
pub struct TypedRequestEvaluator<K, V> {
    revision: Revision,
    generation: u64,
    slots: BTreeMap<K, TypedRequestSlot<K, V>>,
    stats: RequestEvaluationStats,
}

impl<K, V> TypedRequestEvaluator<K, V>
where
    K: Clone + Ord,
{
    pub fn new(revision: Revision) -> Self {
        Self {
            revision,
            generation: 0,
            slots: BTreeMap::new(),
            stats: RequestEvaluationStats::default(),
        }
    }

    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn stats(&self) -> RequestEvaluationStats {
        self.stats
    }

    pub fn request_count(&self) -> usize {
        self.slots.len()
    }

    /// Advance to a newer source revision. Values remain retained but cease to
    /// be current until their request is explicitly verified or executed.
    pub fn advance_to(&mut self, revision: Revision) -> Result<(), CompilationDbError> {
        if revision <= self.revision {
            return Err(CompilationDbError::new(format!(
                "compilation evaluator revision {} is not newer than {}",
                revision.0, self.revision.0,
            )));
        }
        self.revision = revision;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| CompilationDbError::new("compilation evaluator generation overflow"))?;
        Ok(())
    }

    /// Return a value only when it has been verified for the current revision.
    pub fn current_value(&self, key: &K) -> Option<&V> {
        let slot = self.slots.get(key)?;
        (slot.memo.verified_at == self.revision).then_some(&slot.value)
    }

    pub fn memo(&self, key: &K) -> Option<&RequestMemo> {
        self.slots.get(key).map(|slot| &slot.memo)
    }

    pub fn dependencies(&self, key: &K) -> Option<&[K]> {
        self.slots.get(key).map(|slot| slot.dependencies.as_slice())
    }

    /// Remove requests outside a language-owned live key set. Any surviving
    /// request whose dependency span changes will execute on its next demand.
    pub fn retain_requests(&mut self, mut retain: impl FnMut(&K) -> bool) {
        let before = self.slots.len();
        self.slots.retain(|key, _| retain(key));
        self.stats.removed = self
            .stats
            .removed
            .saturating_add(before.saturating_sub(self.slots.len()));
    }

    /// Compute the exact retained reverse dependency cone, including roots.
    pub fn reverse_cone(&self, roots: impl IntoIterator<Item = K>) -> BTreeSet<K> {
        let mut reverse = BTreeMap::<&K, Vec<&K>>::new();
        for (request, slot) in &self.slots {
            for dependency in &slot.dependencies {
                reverse.entry(dependency).or_default().push(request);
            }
        }
        let mut cone = BTreeSet::new();
        let mut pending = roots.into_iter().collect::<VecDeque<_>>();
        while let Some(key) = pending.pop_front() {
            if !cone.insert(key.clone()) {
                continue;
            }
            if let Some(dependents) = reverse.get(&key) {
                pending.extend(dependents.iter().map(|dependent| (*dependent).clone()));
            }
        }
        cone
    }

    /// Verify or execute one typed request.
    ///
    /// Every supplied dependency must already be current. Dependency order is
    /// canonicalized, so evaluation scheduling cannot perturb fingerprints or
    /// currentness. A request may be demanded repeatedly in one revision only
    /// with the same input and dependency span.
    pub fn evaluate<E>(
        &mut self,
        key: K,
        input_fingerprint: RequestFingerprint,
        dependencies: impl IntoIterator<Item = K>,
        execute: impl FnOnce() -> Result<(V, RequestFingerprint), E>,
    ) -> Result<RequestEvaluation<'_, V>, E>
    where
        E: From<CompilationDbError>,
    {
        let mut dependencies = dependencies.into_iter().collect::<Vec<_>>();
        dependencies.sort();
        dependencies.dedup();
        if dependencies.binary_search(&key).is_ok() {
            return Err(CompilationDbError::new(
                "compilation evaluator request directly depends on itself",
            )
            .into());
        }
        for dependency in &dependencies {
            let Some(slot) = self.slots.get(dependency) else {
                return Err(CompilationDbError::new(
                    "compilation evaluator dependency has no published value",
                )
                .into());
            };
            if slot.memo.verified_at != self.revision {
                return Err(CompilationDbError::new(format!(
                    "compilation evaluator dependency is verified at revision {}, expected {}",
                    slot.memo.verified_at.0, self.revision.0,
                ))
                .into());
            }
        }

        if let Some(slot) = self.slots.get(&key) {
            if slot.memo.verified_at == self.revision {
                if slot.memo.input_fingerprint != input_fingerprint
                    || slot.dependencies != dependencies
                {
                    return Err(CompilationDbError::new(
                        "compilation evaluator request changed after publication in one revision",
                    )
                    .into());
                }
                self.stats.reused = self.stats.reused.saturating_add(1);
                let value = &self.slots.get(&key).expect("request remains present").value;
                return Ok(RequestEvaluation {
                    value,
                    disposition: RequestDisposition::Reused,
                });
            }

            let dependencies_are_green = dependencies.iter().all(|dependency| {
                self.slots
                    .get(dependency)
                    .is_some_and(|dependency| dependency.memo.changed_at <= slot.memo.verified_at)
            });
            if slot.memo.input_fingerprint == input_fingerprint
                && slot.dependencies == dependencies
                && dependencies_are_green
            {
                let slot = self.slots.get_mut(&key).expect("request remains present");
                slot.memo.verified_at = self.revision;
                slot.memo.dependencies_verified_at = self.revision;
                self.stats.reused = self.stats.reused.saturating_add(1);
                return Ok(RequestEvaluation {
                    value: &slot.value,
                    disposition: RequestDisposition::Reused,
                });
            }
        }

        let generation = self.generation;
        let (value, result_fingerprint) = execute()?;
        if generation != self.generation {
            return Err(CompilationDbError::new(
                "compilation evaluator rejected a superseded generation publication",
            )
            .into());
        }
        self.stats.executed = self.stats.executed.saturating_add(1);

        if let Some(slot) = self.slots.get_mut(&key) {
            let changed = slot
                .memo
                .publish(
                    self.revision,
                    self.revision,
                    input_fingerprint,
                    result_fingerprint,
                )
                .map_err(E::from)?;
            slot.dependencies = dependencies;
            if changed {
                slot.value = value;
                self.stats.changed = self.stats.changed.saturating_add(1);
            } else {
                self.stats.backdated = self.stats.backdated.saturating_add(1);
            }
        } else {
            self.slots.insert(
                key.clone(),
                TypedRequestSlot {
                    memo: RequestMemo::new(self.revision, input_fingerprint, result_fingerprint),
                    dependencies,
                    value,
                },
            );
            self.stats.changed = self.stats.changed.saturating_add(1);
        }
        let value = &self
            .slots
            .get(&key)
            .expect("published request is present")
            .value;
        Ok(RequestEvaluation {
            value,
            disposition: RequestDisposition::Executed,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ProjectionGraphDigestDomains<'a> {
    pub component: &'a [u8],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProjectionGraphStats {
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
    local_digest: Option<RequestFingerprint>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProjectionId(u32);

impl ProjectionId {
    pub fn from_usize(value: usize) -> Option<Self> {
        u32::try_from(value).ok().map(Self)
    }

    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

/// Cold and warm request graph builder at stable owner/projection granularity.
///
/// Rows inside a request are folded by the language-owned table builder before
/// insertion. Stable semantic keys are interned and collision-checked by the
/// language-owned image before this boundary. This graph therefore carries
/// only dense IDs and fixed fingerprints; it never clones or re-indexes rich
/// language keys.
pub struct DenseProjectionGraphBuilder {
    nodes: Vec<PendingRequestNode>,
    dependencies: Vec<(ProjectionId, ProjectionId)>,
}

impl Default for DenseProjectionGraphBuilder {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            dependencies: Vec::new(),
        }
    }
}

impl DenseProjectionGraphBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        identity_digest: RequestFingerprint,
        local_digest: RequestFingerprint,
    ) -> Result<ProjectionId, CompilationDbError> {
        let id = self.register_pending(identity_digest)?;
        self.set_local_digest(id, local_digest)?;
        Ok(id)
    }

    /// Registers one stable request before its result receipt is available.
    ///
    /// Some language-owned builders need the dense identity to resolve exact
    /// dependency edges while they are still folding rows. Sealing fails closed
    /// unless every pending request receives exactly one local digest.
    pub fn register_pending(
        &mut self,
        identity_digest: RequestFingerprint,
    ) -> Result<ProjectionId, CompilationDbError> {
        let ordinal = u32::try_from(self.nodes.len()).map_err(|_| {
            CompilationDbError::new("compilation projection graph exceeds u32 identities")
        })?;
        let id = ProjectionId(ordinal);
        self.nodes.push(PendingRequestNode {
            identity_digest,
            local_digest: None,
        });
        Ok(id)
    }

    pub fn set_local_digest(
        &mut self,
        id: ProjectionId,
        local_digest: RequestFingerprint,
    ) -> Result<(), CompilationDbError> {
        let node = self.nodes.get_mut(id.as_usize()).ok_or_else(|| {
            CompilationDbError::new("compilation request receipt has an unregistered identity")
        })?;
        if node.local_digest.replace(local_digest).is_some() {
            return Err(CompilationDbError::new(
                "compilation request receipt is published more than once",
            ));
        }
        Ok(())
    }

    pub fn local_digest(&self, id: ProjectionId) -> Option<RequestFingerprint> {
        self.nodes.get(id.as_usize())?.local_digest
    }

    pub fn add_dependency(
        &mut self,
        request: ProjectionId,
        dependency: ProjectionId,
    ) -> Result<(), CompilationDbError> {
        if request.as_usize() >= self.nodes.len() {
            return Err(CompilationDbError::new(
                "compilation projection edge has an unregistered source",
            ));
        }
        if dependency.as_usize() >= self.nodes.len() {
            return Err(CompilationDbError::new(
                "compilation projection edge has an unregistered target",
            ));
        }
        self.dependencies.push((request, dependency));
        Ok(())
    }

    pub fn seal(
        self,
        domains: ProjectionGraphDigestDomains<'_>,
    ) -> Result<SealedDenseProjectionGraph, CompilationDbError> {
        if domains.component.is_empty() {
            return Err(CompilationDbError::new(
                "compilation request graph component digest domain is empty",
            ));
        }

        let mut registered = self.nodes.into_iter().enumerate().collect::<Vec<_>>();
        registered.sort_unstable_by_key(|(_, node)| node.identity_digest);
        if registered
            .windows(2)
            .any(|pair| pair[0].1.identity_digest == pair[1].1.identity_digest)
        {
            return Err(CompilationDbError::new(
                "compilation request graph contains duplicate stable identity fingerprints",
            ));
        }
        let mut canonical_by_registered = vec![0usize; registered.len()];
        let mut registered_by_canonical = Vec::with_capacity(registered.len());
        let mut identity_digests = Vec::with_capacity(registered.len());
        let mut local_digests = Vec::with_capacity(registered.len());
        for (canonical, (registered_ordinal, node)) in registered.into_iter().enumerate() {
            canonical_by_registered[registered_ordinal] = canonical;
            registered_by_canonical.push(registered_ordinal);
            identity_digests.push(node.identity_digest);
            local_digests.push(node.local_digest.ok_or_else(|| {
                CompilationDbError::new(
                    "compilation request graph contains an unsealed pending request",
                )
            })?);
        }

        let mut edges = vec![Vec::new(); identity_digests.len()];
        for (request, dependency) in self.dependencies {
            let request = canonical_by_registered[request.as_usize()];
            let dependency = canonical_by_registered[dependency.as_usize()];
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
                    CompilationDbError::new("compilation request graph produced an empty component")
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut component_digests = vec![None; components.len()];
        let mut dependencies = Vec::new();
        let mut component_edge_count = 0usize;
        for component in 0..components.len() {
            let members = components.members(component).ok_or_else(|| {
                CompilationDbError::new(format!(
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
                    CompilationDbError::new("compilation request component edge count overflow")
                })?;
            if dependencies
                .iter()
                .any(|dependency| *dependency >= component)
            {
                return Err(CompilationDbError::new(format!(
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
                    CompilationDbError::new(
                        "compilation request component was hashed before its dependency",
                    )
                })?);
            }
            component_digests[component] = Some(hasher.finalize().into());
        }

        let edge_offsets = adjacency_offsets(&edges)?;
        let edge_arena = edges.into_iter().flatten().collect::<Vec<_>>();
        let reverse_edges = reverse_adjacency(identity_digests.len(), &edge_offsets, &edge_arena)?;
        let stats = ProjectionGraphStats {
            nodes: identity_digests.len(),
            edges: edge_count,
            components: components.len(),
            cyclic_components: components
                .iter()
                .filter(|members| members.len() > 1)
                .count(),
            maximum_component_nodes: components.iter().map(<[usize]>::len).max().unwrap_or(0),
            component_edges: component_edge_count,
        };

        Ok(SealedDenseProjectionGraph {
            canonical_by_registered,
            registered_by_canonical,
            identity_digests,
            local_digests,
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
                        CompilationDbError::new(
                            "compilation request component has no sealed digest",
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            component_representatives: representatives,
            stats,
        })
    }

    /// Seals the exact cold request graph and publishes revision metadata for
    /// later warm red/green evaluation. The same graph is therefore proof
    /// authority and currentness authority rather than a disposable digest
    /// helper followed by a second incremental database.
    pub fn seal_snapshot(
        self,
        domains: ProjectionGraphDigestDomains<'_>,
        revision: Revision,
    ) -> Result<SealedRequestGraphSnapshot, CompilationDbError> {
        let graph = self.seal(domains)?;
        SealedRequestGraphSnapshot::new(graph, revision)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SealedDenseProjectionGraph {
    canonical_by_registered: Vec<usize>,
    registered_by_canonical: Vec<usize>,
    identity_digests: Vec<RequestFingerprint>,
    local_digests: Vec<RequestFingerprint>,
    edge_offsets: Vec<usize>,
    edge_arena: Vec<usize>,
    reverse_edge_offsets: Vec<usize>,
    reverse_edge_arena: Vec<usize>,
    component_by_node: Vec<usize>,
    component_member_offsets: Vec<usize>,
    component_members: Vec<usize>,
    component_digests: Vec<RequestFingerprint>,
    component_representatives: Vec<usize>,
    stats: ProjectionGraphStats,
}

impl SealedDenseProjectionGraph {
    pub const fn stats(&self) -> ProjectionGraphStats {
        self.stats
    }

    pub fn dependencies(
        &self,
        id: ProjectionId,
    ) -> Option<impl Iterator<Item = ProjectionId> + '_> {
        let ordinal = *self.canonical_by_registered.get(id.as_usize())?;
        let start = self.edge_offsets[ordinal];
        let end = self.edge_offsets[ordinal + 1];
        Some(
            self.edge_arena[start..end]
                .iter()
                .map(|target| ProjectionId(self.registered_by_canonical[*target] as u32)),
        )
    }

    pub fn id_by_identity_digest(
        &self,
        identity_digest: RequestFingerprint,
    ) -> Option<ProjectionId> {
        let canonical = self.identity_digests.binary_search(&identity_digest).ok()?;
        let registered = *self.registered_by_canonical.get(canonical)?;
        u32::try_from(registered).ok().map(ProjectionId)
    }

    pub fn identity_digest(&self, id: ProjectionId) -> Option<RequestFingerprint> {
        let canonical = *self.canonical_by_registered.get(id.as_usize())?;
        self.identity_digests.get(canonical).copied()
    }

    pub fn local_digest(&self, id: ProjectionId) -> Option<RequestFingerprint> {
        let canonical = *self.canonical_by_registered.get(id.as_usize())?;
        self.local_digests.get(canonical).copied()
    }

    pub fn reverse_dependents(
        &self,
        id: ProjectionId,
    ) -> Option<impl Iterator<Item = ProjectionId> + '_> {
        let ordinal = *self.canonical_by_registered.get(id.as_usize())?;
        let start = self.reverse_edge_offsets[ordinal];
        let end = self.reverse_edge_offsets[ordinal + 1];
        Some(
            self.reverse_edge_arena[start..end]
                .iter()
                .map(|source| ProjectionId(self.registered_by_canonical[*source] as u32)),
        )
    }

    pub fn implementation_digest(
        &self,
        id: ProjectionId,
        domain: &[u8],
    ) -> Result<RequestFingerprint, CompilationDbError> {
        if domain.is_empty() {
            return Err(CompilationDbError::new(
                "compilation request implementation digest domain is empty",
            ));
        }
        let node = *self
            .canonical_by_registered
            .get(id.as_usize())
            .ok_or_else(|| {
                CompilationDbError::new("compilation request implementation key is absent")
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

    pub fn component_members(
        &self,
        id: ProjectionId,
    ) -> Option<impl Iterator<Item = ProjectionId> + '_> {
        let node = *self.canonical_by_registered.get(id.as_usize())?;
        let component = self.component_by_node[node];
        let start = self.component_member_offsets[component];
        let end = self.component_member_offsets[component + 1];
        Some(
            self.component_members[start..end]
                .iter()
                .map(|member| ProjectionId(self.registered_by_canonical[*member] as u32)),
        )
    }
}

/// Immutable request topology plus the revision-zero memo state produced by a
/// cold compiler request. Language components retain their typed values; this
/// snapshot retains only stable identities, exact dependency cones, SCC proof
/// receipts, and red/green publication metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SealedRequestGraphSnapshot {
    revision: Revision,
    graph: SealedDenseProjectionGraph,
    memos: Vec<RequestMemo>,
}

impl SealedRequestGraphSnapshot {
    fn new(
        graph: SealedDenseProjectionGraph,
        revision: Revision,
    ) -> Result<Self, CompilationDbError> {
        let mut memos = Vec::with_capacity(graph.canonical_by_registered.len());
        for registered in 0..graph.canonical_by_registered.len() {
            let registered = u32::try_from(registered).map_err(|_| {
                CompilationDbError::new("compilation request snapshot exceeds u32 identities")
            })?;
            let id = ProjectionId(registered);
            let input_fingerprint = graph.local_digest(id).ok_or_else(|| {
                CompilationDbError::new("compilation request snapshot has no local receipt")
            })?;
            memos.push(RequestMemo::new(
                revision,
                input_fingerprint,
                input_fingerprint,
            ));
        }
        Ok(Self {
            revision,
            graph,
            memos,
        })
    }

    pub const fn revision(&self) -> Revision {
        self.revision
    }

    pub const fn graph(&self) -> &SealedDenseProjectionGraph {
        &self.graph
    }

    pub fn memo(&self, id: ProjectionId) -> Option<&RequestMemo> {
        self.memos.get(id.as_usize())
    }

    pub fn id_by_identity_digest(
        &self,
        identity_digest: RequestFingerprint,
    ) -> Option<ProjectionId> {
        self.graph.id_by_identity_digest(identity_digest)
    }

    pub fn request_count(&self) -> usize {
        self.memos.len()
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
    ) -> Result<(), CompilationDbError> {
        let start = self.members.len();
        loop {
            let member = active.pop().ok_or_else(|| {
                CompilationDbError::new("compilation request component stack underflow")
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
) -> Result<(Vec<usize>, Components), CompilationDbError> {
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
            CompilationDbError::new("compilation request discovery index overflow")
        })?;
        active.push(start);
        pending.push((start, 0usize));

        while !pending.is_empty() {
            let frame = pending.len() - 1;
            let (node, next_edge) = pending[frame];
            let targets = edges.get(node).ok_or_else(|| {
                CompilationDbError::new(format!(
                    "compilation request graph references missing node {node}"
                ))
            })?;
            if next_edge < targets.len() {
                pending[frame].1 += 1;
                let target = targets[next_edge];
                if target >= node_count {
                    return Err(CompilationDbError::new(format!(
                        "compilation request graph references missing target {target}"
                    )));
                }
                if discovery_index[target] == usize::MAX {
                    discovery_index[target] = next_discovery_index;
                    low_link[target] = next_discovery_index;
                    next_discovery_index =
                        next_discovery_index.checked_add(1).ok_or_else(|| {
                            CompilationDbError::new("compilation request discovery index overflow")
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
        return Err(CompilationDbError::new(
            "compilation request component stack is not empty after traversal",
        ));
    }
    Ok((component_by_node, components))
}

fn adjacency_offsets(edges: &[Vec<usize>]) -> Result<Vec<usize>, CompilationDbError> {
    let mut offsets = Vec::with_capacity(edges.len().saturating_add(1));
    offsets.push(0usize);
    for targets in edges {
        let next = offsets
            .last()
            .copied()
            .expect("request adjacency has an initial offset")
            .checked_add(targets.len())
            .ok_or_else(|| CompilationDbError::new("request adjacency offset overflow"))?;
        offsets.push(next);
    }
    Ok(offsets)
}

fn reverse_adjacency(
    node_count: usize,
    offsets: &[usize],
    edges: &[usize],
) -> Result<(Vec<usize>, Vec<usize>), CompilationDbError> {
    let mut counts = vec![0usize; node_count];
    for target in edges.iter().copied() {
        let count = counts.get_mut(target).ok_or_else(|| {
            CompilationDbError::new("request reverse adjacency target is out of range")
        })?;
        *count = count
            .checked_add(1)
            .ok_or_else(|| CompilationDbError::new("request reverse degree overflow"))?;
    }
    let mut reverse_offsets = Vec::with_capacity(node_count.saturating_add(1));
    reverse_offsets.push(0usize);
    for count in &counts {
        let next = reverse_offsets
            .last()
            .copied()
            .expect("request reverse adjacency has an initial offset")
            .checked_add(*count)
            .ok_or_else(|| CompilationDbError::new("request reverse offset overflow"))?;
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

fn update_count(
    hasher: &mut Sha256,
    value: usize,
    context: &str,
) -> Result<(), CompilationDbError> {
    let value = u64::try_from(value).map_err(|_| {
        CompilationDbError::new(format!(
            "compilation request {context} exceeds its u64 encoding"
        ))
    })?;
    hasher.update(value.to_be_bytes());
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilationDbError {
    message: String,
}

impl CompilationDbError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CompilationDbError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CompilationDbError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const COMPONENT_DOMAIN: &[u8] = b"test.request-component.v1\0";
    const IMPLEMENTATION_DOMAIN: &[u8] = b"test.request-implementation.v1\0";

    fn digest(value: u8) -> RequestFingerprint {
        [value; 32]
    }

    struct TestGraph {
        sealed: SealedDenseProjectionGraph,
        ids: BTreeMap<u8, ProjectionId>,
        keys_by_id: Vec<u8>,
    }

    impl TestGraph {
        fn implementation_digest(&self, key: u8) -> RequestFingerprint {
            self.sealed
                .implementation_digest(self.ids[&key], IMPLEMENTATION_DOMAIN)
                .unwrap()
        }

        fn keys(&self, ids: impl Iterator<Item = ProjectionId>) -> Vec<u8> {
            ids.map(|id| self.keys_by_id[id.as_usize()]).collect()
        }
    }

    fn graph(nodes: &[(u8, u8)], edges: &[(u8, u8)]) -> TestGraph {
        let mut builder = DenseProjectionGraphBuilder::new();
        let mut ids = BTreeMap::new();
        let mut keys_by_id = Vec::new();
        for (key, value) in nodes {
            let id = builder.register(digest(*key), digest(*value)).unwrap();
            assert_eq!(id.as_usize(), keys_by_id.len());
            keys_by_id.push(*key);
            ids.insert(*key, id);
        }
        for (source, target) in edges {
            builder.add_dependency(ids[source], ids[target]).unwrap();
        }
        let sealed = builder
            .seal(ProjectionGraphDigestDomains {
                component: COMPONENT_DOMAIN,
            })
            .unwrap();
        TestGraph {
            sealed,
            ids,
            keys_by_id,
        }
    }

    #[test]
    fn cold_snapshot_retains_exact_request_memos_and_identity_lookup() {
        let mut builder = DenseProjectionGraphBuilder::new();
        let root = builder.register(digest(1), digest(11)).unwrap();
        let child = builder.register_pending(digest(2)).unwrap();
        builder.add_dependency(root, child).unwrap();
        builder.set_local_digest(child, digest(22)).unwrap();
        let snapshot = builder
            .seal_snapshot(
                ProjectionGraphDigestDomains {
                    component: COMPONENT_DOMAIN,
                },
                Revision(0),
            )
            .unwrap();

        assert_eq!(snapshot.revision(), Revision(0));
        assert_eq!(snapshot.request_count(), 2);
        assert_eq!(snapshot.id_by_identity_digest(digest(1)), Some(root));
        assert_eq!(snapshot.id_by_identity_digest(digest(2)), Some(child));
        assert_eq!(snapshot.memo(root).unwrap().result_fingerprint, digest(11));
        assert_eq!(snapshot.memo(child).unwrap().result_fingerprint, digest(22));
        assert_eq!(
            snapshot
                .graph()
                .dependencies(root)
                .unwrap()
                .collect::<Vec<_>>(),
            vec![child]
        );
    }

    #[test]
    fn pending_request_must_publish_exactly_one_receipt_before_seal() {
        let mut missing = DenseProjectionGraphBuilder::new();
        missing.register_pending(digest(1)).unwrap();
        assert!(
            missing
                .seal(ProjectionGraphDigestDomains {
                    component: COMPONENT_DOMAIN,
                })
                .is_err()
        );

        let mut duplicate = DenseProjectionGraphBuilder::new();
        let id = duplicate.register_pending(digest(1)).unwrap();
        duplicate.set_local_digest(id, digest(2)).unwrap();
        assert!(duplicate.set_local_digest(id, digest(3)).is_err());
    }

    #[test]
    fn dependency_mutation_propagates_without_touching_unrelated_request() {
        let before = graph(&[(1, 11), (2, 22), (3, 33)], &[(1, 2)]);
        let after = graph(&[(1, 11), (2, 44), (3, 33)], &[(1, 2)]);
        assert_ne!(
            before.implementation_digest(1),
            after.implementation_digest(1)
        );
        assert_ne!(
            before.implementation_digest(2),
            after.implementation_digest(2)
        );
        assert_eq!(
            before.implementation_digest(3),
            after.implementation_digest(3)
        );
    }

    #[test]
    fn cycles_are_one_component_and_reverse_edges_are_exact() {
        let graph = graph(&[(1, 11), (2, 22), (3, 33)], &[(1, 2), (2, 1), (3, 1)]);
        assert_eq!(graph.sealed.stats().components, 2);
        assert_eq!(graph.sealed.stats().cyclic_components, 1);
        assert_eq!(
            graph.keys(graph.sealed.component_members(graph.ids[&1]).unwrap()),
            vec![1, 2]
        );
        assert_eq!(
            graph.keys(graph.sealed.reverse_dependents(graph.ids[&1]).unwrap()),
            vec![2, 3]
        );
    }

    #[test]
    fn unchanged_results_are_backdated() {
        let mut memo = RequestMemo::new(Revision(1), digest(1), digest(2));
        assert!(
            !memo
                .publish(Revision(2), Revision(2), digest(3), digest(2))
                .unwrap()
        );
        assert_eq!(memo.changed_at, Revision(1));
        assert_eq!(memo.verified_at, Revision(2));
        assert!(
            memo.publish(Revision(3), Revision(3), digest(4), digest(5))
                .unwrap()
        );
        assert_eq!(memo.changed_at, Revision(3));
        assert!(
            memo.publish(Revision(3), Revision(3), digest(6), digest(7))
                .is_err()
        );
        assert!(
            memo.publish(Revision(4), Revision(3), digest(6), digest(7))
                .is_err()
        );
    }

    #[test]
    fn typed_evaluator_reuses_green_dependencies_and_backdates_equal_results() {
        let mut evaluator = TypedRequestEvaluator::<&'static str, u32>::new(Revision(0));
        let parsed = evaluator
            .evaluate("parse", digest(1), std::iter::empty(), || {
                Ok::<_, CompilationDbError>((10, digest(10)))
            })
            .unwrap();
        assert_eq!(*parsed.value(), 10);
        assert_eq!(parsed.disposition(), RequestDisposition::Executed);
        let linked = evaluator
            .evaluate("link", digest(20), ["parse"], || {
                Ok::<_, CompilationDbError>((20, digest(20)))
            })
            .unwrap();
        assert_eq!(*linked.value(), 20);

        evaluator.advance_to(Revision(1)).unwrap();
        let parsed = evaluator
            .evaluate::<CompilationDbError>("parse", digest(1), std::iter::empty(), || {
                panic!("green parse request executed")
            })
            .unwrap();
        assert!(parsed.was_reused());
        let linked = evaluator
            .evaluate::<CompilationDbError>("link", digest(20), ["parse"], || {
                panic!("green link request executed")
            })
            .unwrap();
        assert!(linked.was_reused());

        evaluator.advance_to(Revision(2)).unwrap();
        evaluator
            .evaluate("parse", digest(2), std::iter::empty(), || {
                Ok::<_, CompilationDbError>((999, digest(10)))
            })
            .unwrap();
        assert_eq!(evaluator.memo(&"parse").unwrap().changed_at, Revision(0));
        let linked = evaluator
            .evaluate::<CompilationDbError>("link", digest(20), ["parse"], || {
                panic!("backdated dependency dirtied its dependent")
            })
            .unwrap();
        assert!(linked.was_reused());
        assert_eq!(*linked.value(), 20);

        let stats = evaluator.stats();
        assert_eq!(stats.executed, 3);
        assert_eq!(stats.reused, 3);
        assert_eq!(stats.backdated, 1);
        assert_eq!(stats.changed, 2);
    }

    #[test]
    fn typed_evaluator_exposes_exact_reverse_cones_and_current_values() {
        let mut evaluator = TypedRequestEvaluator::<u8, u8>::new(Revision(0));
        for (key, dependencies) in [(1, vec![]), (2, vec![1]), (3, vec![2]), (4, vec![])] {
            evaluator
                .evaluate(key, digest(key), dependencies, || {
                    Ok::<_, CompilationDbError>((key, digest(key + 10)))
                })
                .unwrap();
        }
        assert_eq!(
            evaluator.reverse_cone([1]).into_iter().collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(evaluator.current_value(&4), Some(&4));

        evaluator.advance_to(Revision(1)).unwrap();
        assert_eq!(evaluator.current_value(&4), None);
        evaluator.retain_requests(|key| *key != 2 && *key != 3);
        assert_eq!(
            evaluator.reverse_cone([1]).into_iter().collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(evaluator.stats().removed, 2);
    }

    #[test]
    fn typed_evaluator_rejects_unverified_and_self_dependencies() {
        let mut evaluator = TypedRequestEvaluator::<u8, u8>::new(Revision(0));
        let error = evaluator
            .evaluate(1, digest(1), [1], || {
                Ok::<_, CompilationDbError>((1, digest(11)))
            })
            .err()
            .expect("self dependency fails closed");
        assert!(error.to_string().contains("directly depends on itself"));

        evaluator
            .evaluate(1, digest(1), std::iter::empty(), || {
                Ok::<_, CompilationDbError>((1, digest(11)))
            })
            .unwrap();
        evaluator.advance_to(Revision(1)).unwrap();
        let error = evaluator
            .evaluate(2, digest(2), [1], || {
                Ok::<_, CompilationDbError>((2, digest(12)))
            })
            .err()
            .expect("stale dependency fails closed");
        assert!(error.to_string().contains("verified at revision 0"));
    }

    #[test]
    fn missing_and_duplicate_nodes_fail_closed() {
        let mut duplicate = DenseProjectionGraphBuilder::new();
        duplicate.register(digest(1), digest(2)).unwrap();
        duplicate.register(digest(1), digest(2)).unwrap();
        assert!(
            duplicate
                .seal(ProjectionGraphDigestDomains {
                    component: COMPONENT_DOMAIN,
                })
                .is_err()
        );

        let mut missing = DenseProjectionGraphBuilder::new();
        let id = missing.register(digest(1), digest(2)).unwrap();
        assert!(missing.add_dependency(id, ProjectionId(1)).is_err());
    }

    #[test]
    fn registration_order_does_not_change_projection_digests() {
        let forward = graph(&[(1, 11), (2, 22), (3, 33)], &[(1, 2), (2, 3)]);
        let reverse = graph(&[(3, 33), (2, 22), (1, 11)], &[(2, 3), (1, 2)]);
        for key in [1, 2, 3] {
            assert_eq!(
                forward.implementation_digest(key),
                reverse.implementation_digest(key),
            );
        }
    }
}
