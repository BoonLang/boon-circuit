#![forbid(unsafe_code)]

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;

pub type RequestFingerprint = [u8; 32];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct RequestInputFingerprint(pub RequestFingerprint);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct RequestOutputFingerprint(pub RequestFingerprint);

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
    pub demanded: usize,
    pub executed: usize,
    pub reused: usize,
    pub backdated: usize,
    pub changed: usize,
    pub failed: usize,
    pub canceled: usize,
    pub superseded: usize,
    pub removed: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequestStart {
    Execute(RequestEvaluationTicket),
    Reused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestAbortReason {
    Failed,
    Canceled,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestNodeId(u32);

impl RequestNodeId {
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RequestNodeIdentity {
    pub family: &'static str,
    pub key_fingerprint: RequestFingerprint,
}

impl fmt::Display for RequestNodeIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:", self.family)?;
        for byte in &self.key_fingerprint[..6] {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RequestNodeState {
    Vacant,
    Published,
    Computing {
        revision: Revision,
        generation: u64,
        stack_index: usize,
    },
    Failed {
        revision: Revision,
    },
    Removed,
}

struct RequestNode {
    identity: RequestNodeIdentity,
    state: RequestNodeState,
    memo: Option<RequestMemo>,
    dependencies: Vec<RequestNodeId>,
    reverse_dependents: BTreeSet<RequestNodeId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RequestEvaluationTicket {
    node: RequestNodeId,
    revision: Revision,
    generation: u64,
    stack_index: usize,
    input_fingerprint: RequestInputFingerprint,
    dependencies: Vec<RequestNodeId>,
    had_publication: bool,
}

/// Shared currentness and evaluation-edge graph for language-owned typed
/// request tables.
///
/// Values never enter this graph. Each [`TypedRequestTable`] owns exactly one
/// request family's Rust keys and values; this graph owns revisions,
/// automatically captured cross-family edges, reverse cones, cycle paths,
/// backdating, and generation-checked publication.
pub struct RequestEvaluatorGraph {
    revision: Revision,
    generation: u64,
    identities: BTreeMap<RequestNodeIdentity, RequestNodeId>,
    nodes: Vec<RequestNode>,
    active: Vec<RequestNodeId>,
    stats: RequestEvaluationStats,
    revision_stats: RequestEvaluationStats,
}

impl RequestEvaluatorGraph {
    pub fn new(revision: Revision) -> Self {
        Self {
            revision,
            generation: 0,
            identities: BTreeMap::new(),
            nodes: Vec::new(),
            active: Vec::new(),
            stats: RequestEvaluationStats::default(),
            revision_stats: RequestEvaluationStats::default(),
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

    pub const fn revision_stats(&self) -> RequestEvaluationStats {
        self.revision_stats
    }

    pub fn request_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|node| node.state != RequestNodeState::Removed)
            .count()
    }

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
        for node in self.active.drain(..) {
            let slot = self
                .nodes
                .get_mut(node.as_usize())
                .expect("active request node exists");
            slot.state = if slot.memo.is_some() {
                RequestNodeState::Published
            } else {
                RequestNodeState::Vacant
            };
            self.stats.superseded = self.stats.superseded.saturating_add(1);
        }
        self.revision_stats = RequestEvaluationStats::default();
        Ok(())
    }

    pub fn identity(&self, node: RequestNodeId) -> Option<&RequestNodeIdentity> {
        self.nodes.get(node.as_usize()).map(|node| &node.identity)
    }

    pub fn memo(&self, node: RequestNodeId) -> Option<&RequestMemo> {
        self.nodes.get(node.as_usize())?.memo.as_ref()
    }

    pub fn dependencies(&self, node: RequestNodeId) -> Option<&[RequestNodeId]> {
        self.nodes
            .get(node.as_usize())
            .map(|node| node.dependencies.as_slice())
    }

    fn is_current(&self, node: RequestNodeId) -> bool {
        self.nodes.get(node.as_usize()).is_some_and(|node| {
            node.state == RequestNodeState::Published
                && node
                    .memo
                    .as_ref()
                    .is_some_and(|memo| memo.verified_at == self.revision)
        })
    }

    pub fn reverse_cone(
        &self,
        roots: impl IntoIterator<Item = RequestNodeId>,
    ) -> BTreeSet<RequestNodeId> {
        let mut cone = BTreeSet::new();
        let mut pending = roots.into_iter().collect::<VecDeque<_>>();
        while let Some(node) = pending.pop_front() {
            if !cone.insert(node) {
                continue;
            }
            if let Some(node) = self.nodes.get(node.as_usize()) {
                pending.extend(node.reverse_dependents.iter().copied());
            }
        }
        cone
    }

    fn register(
        &mut self,
        identity: RequestNodeIdentity,
    ) -> Result<RequestNodeId, CompilationDbError> {
        if let Some(node) = self.identities.get(&identity).copied() {
            if self.nodes[node.as_usize()].state == RequestNodeState::Removed {
                self.nodes[node.as_usize()].state = RequestNodeState::Vacant;
            }
            return Ok(node);
        }
        let node =
            RequestNodeId(u32::try_from(self.nodes.len()).map_err(|_| {
                CompilationDbError::new("compilation request graph exceeds u32 nodes")
            })?);
        self.identities.insert(identity.clone(), node);
        self.nodes.push(RequestNode {
            identity,
            state: RequestNodeState::Vacant,
            memo: None,
            dependencies: Vec::new(),
            reverse_dependents: BTreeSet::new(),
        });
        Ok(node)
    }

    fn cycle_error(&self, stack_index: usize, repeated: RequestNodeId) -> CompilationDbError {
        let mut path = self.active[stack_index..]
            .iter()
            .filter_map(|node| self.identity(*node))
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if let Some(identity) = self.identity(repeated) {
            path.push(identity.to_string());
        }
        CompilationDbError::new(format!("compilation request cycle: {}", path.join(" -> ")))
    }

    fn begin(
        &mut self,
        node: RequestNodeId,
        input_fingerprint: RequestInputFingerprint,
    ) -> Result<RequestStart, CompilationDbError> {
        self.stats.demanded = self.stats.demanded.saturating_add(1);
        self.revision_stats.demanded = self.revision_stats.demanded.saturating_add(1);
        let state = self
            .nodes
            .get(node.as_usize())
            .ok_or_else(|| CompilationDbError::new("compilation request node is missing"))?
            .state;
        if let RequestNodeState::Computing { stack_index, .. } = state {
            return Err(self.cycle_error(stack_index, node));
        }
        if state == RequestNodeState::Removed {
            return Err(CompilationDbError::new(
                "removed compilation request was not re-registered",
            ));
        }

        if let Some(memo) = self.nodes[node.as_usize()]
            .memo
            .as_ref()
            .filter(|memo| memo.verified_at == self.revision)
        {
            if memo.input_fingerprint != input_fingerprint.0 {
                return Err(CompilationDbError::new(
                    "compilation request input changed after publication in one revision",
                ));
            }
            self.stats.reused = self.stats.reused.saturating_add(1);
            self.revision_stats.reused = self.revision_stats.reused.saturating_add(1);
            return Ok(RequestStart::Reused);
        }

        let green = self.nodes[node.as_usize()]
            .memo
            .as_ref()
            .is_some_and(|memo| {
                memo.input_fingerprint == input_fingerprint.0
                    && self.nodes[node.as_usize()]
                        .dependencies
                        .iter()
                        .all(|dependency| {
                            self.nodes
                                .get(dependency.as_usize())
                                .and_then(|dependency| dependency.memo.as_ref())
                                .is_some_and(|dependency| {
                                    dependency.verified_at == self.revision
                                        && dependency.changed_at <= memo.verified_at
                                })
                        })
            });
        if green {
            let memo = self.nodes[node.as_usize()]
                .memo
                .as_mut()
                .expect("green request has a memo");
            memo.verified_at = self.revision;
            memo.dependencies_verified_at = self.revision;
            self.nodes[node.as_usize()].state = RequestNodeState::Published;
            self.stats.reused = self.stats.reused.saturating_add(1);
            self.revision_stats.reused = self.revision_stats.reused.saturating_add(1);
            return Ok(RequestStart::Reused);
        }

        let stack_index = self.active.len();
        self.active.push(node);
        self.nodes[node.as_usize()].state = RequestNodeState::Computing {
            revision: self.revision,
            generation: self.generation,
            stack_index,
        };
        self.stats.executed = self.stats.executed.saturating_add(1);
        self.revision_stats.executed = self.revision_stats.executed.saturating_add(1);
        Ok(RequestStart::Execute(RequestEvaluationTicket {
            node,
            revision: self.revision,
            generation: self.generation,
            stack_index,
            input_fingerprint,
            dependencies: Vec::new(),
            had_publication: self.nodes[node.as_usize()].memo.is_some(),
        }))
    }
}

impl RequestEvaluatorGraph {
    fn validate_ticket(&self, ticket: &RequestEvaluationTicket) -> Result<(), CompilationDbError> {
        if ticket.revision != self.revision || ticket.generation != self.generation {
            return Err(CompilationDbError::new(
                "compilation evaluator rejected a superseded generation ticket",
            ));
        }
        if self.active.get(ticket.stack_index) != Some(&ticket.node)
            || self.active.len() != ticket.stack_index + 1
        {
            return Err(CompilationDbError::new(
                "compilation request tickets must publish or abort in stack order",
            ));
        }
        match self
            .nodes
            .get(ticket.node.as_usize())
            .map(|node| node.state)
        {
            Some(RequestNodeState::Computing {
                revision,
                generation,
                stack_index,
            }) if revision == ticket.revision
                && generation == ticket.generation
                && stack_index == ticket.stack_index =>
            {
                Ok(())
            }
            _ => Err(CompilationDbError::new(
                "compilation request ticket is not the active computation",
            )),
        }
    }

    fn require(
        &self,
        ticket: &mut RequestEvaluationTicket,
        dependency: RequestNodeId,
    ) -> Result<(), CompilationDbError> {
        self.validate_ticket(ticket)?;
        if dependency == ticket.node {
            return Err(self.cycle_error(ticket.stack_index, dependency));
        }
        let dependency_node = self.nodes.get(dependency.as_usize()).ok_or_else(|| {
            CompilationDbError::new("compilation evaluator dependency node is missing")
        })?;
        if let RequestNodeState::Computing { stack_index, .. } = dependency_node.state {
            return Err(self.cycle_error(stack_index, dependency));
        }
        if !self.is_current(dependency) {
            let verified_at = dependency_node.memo.as_ref().map(|memo| memo.verified_at.0);
            return Err(CompilationDbError::new(match verified_at {
                Some(revision) => format!(
                    "compilation evaluator dependency is verified at revision {revision}, expected {}",
                    self.revision.0
                ),
                None => "compilation evaluator dependency has no published value".to_owned(),
            }));
        }
        ticket.dependencies.push(dependency);
        Ok(())
    }

    fn replace_dependencies(&mut self, node: RequestNodeId, dependencies: Vec<RequestNodeId>) {
        let old_dependencies = std::mem::take(&mut self.nodes[node.as_usize()].dependencies);
        for dependency in old_dependencies {
            if let Some(dependency) = self.nodes.get_mut(dependency.as_usize()) {
                dependency.reverse_dependents.remove(&node);
            }
        }
        for dependency in &dependencies {
            self.nodes[dependency.as_usize()]
                .reverse_dependents
                .insert(node);
        }
        self.nodes[node.as_usize()].dependencies = dependencies;
    }

    fn publish(
        &mut self,
        mut ticket: RequestEvaluationTicket,
        result_fingerprint: RequestOutputFingerprint,
    ) -> Result<bool, CompilationDbError> {
        self.validate_ticket(&ticket)?;
        ticket.dependencies.sort_unstable();
        ticket.dependencies.dedup();
        if ticket.dependencies.binary_search(&ticket.node).is_ok() {
            return Err(CompilationDbError::new(
                "compilation evaluator request directly depends on itself",
            ));
        }
        for dependency in &ticket.dependencies {
            if !self.is_current(*dependency) {
                return Err(CompilationDbError::new(
                    "compilation request dependency became stale before publication",
                ));
            }
        }

        let changed = if let Some(memo) = self.nodes[ticket.node.as_usize()].memo.as_mut() {
            memo.publish(
                self.revision,
                self.revision,
                ticket.input_fingerprint.0,
                result_fingerprint.0,
            )?
        } else {
            self.nodes[ticket.node.as_usize()].memo = Some(RequestMemo::new(
                self.revision,
                ticket.input_fingerprint.0,
                result_fingerprint.0,
            ));
            true
        };
        self.replace_dependencies(ticket.node, ticket.dependencies);
        self.nodes[ticket.node.as_usize()].state = RequestNodeState::Published;
        self.active.pop();
        if changed {
            self.stats.changed = self.stats.changed.saturating_add(1);
            self.revision_stats.changed = self.revision_stats.changed.saturating_add(1);
        } else {
            self.stats.backdated = self.stats.backdated.saturating_add(1);
            self.revision_stats.backdated = self.revision_stats.backdated.saturating_add(1);
        }
        Ok(changed)
    }

    fn abort(
        &mut self,
        ticket: RequestEvaluationTicket,
        reason: RequestAbortReason,
    ) -> Result<(), CompilationDbError> {
        self.validate_ticket(&ticket)?;
        self.active.pop();
        self.nodes[ticket.node.as_usize()].state = match reason {
            RequestAbortReason::Failed => RequestNodeState::Failed {
                revision: self.revision,
            },
            RequestAbortReason::Canceled if ticket.had_publication => RequestNodeState::Published,
            RequestAbortReason::Canceled => RequestNodeState::Vacant,
        };
        match reason {
            RequestAbortReason::Failed => {
                self.stats.failed = self.stats.failed.saturating_add(1);
                self.revision_stats.failed = self.revision_stats.failed.saturating_add(1);
            }
            RequestAbortReason::Canceled => {
                self.stats.canceled = self.stats.canceled.saturating_add(1);
                self.revision_stats.canceled = self.revision_stats.canceled.saturating_add(1);
            }
        }
        Ok(())
    }

    fn remove_nodes(&mut self, nodes: &[RequestNodeId]) -> Result<(), CompilationDbError> {
        if !self.active.is_empty() {
            return Err(CompilationDbError::new(
                "cannot remove compilation requests while evaluation is active",
            ));
        }
        for node in nodes {
            let dependencies = std::mem::take(&mut self.nodes[node.as_usize()].dependencies);
            for dependency in dependencies {
                self.nodes[dependency.as_usize()]
                    .reverse_dependents
                    .remove(node);
            }
            let slot = &mut self.nodes[node.as_usize()];
            if slot.state != RequestNodeState::Removed {
                slot.state = RequestNodeState::Removed;
                slot.memo = None;
                self.stats.removed = self.stats.removed.saturating_add(1);
                self.revision_stats.removed = self.revision_stats.removed.saturating_add(1);
            }
        }
        Ok(())
    }
}

pub trait RequestFamily {
    type Key: Clone + fmt::Debug + Ord;
    type Value;

    const NAME: &'static str;

    fn key_fingerprint(key: &Self::Key) -> RequestFingerprint;

    fn output_fingerprint(
        value: &Self::Value,
    ) -> Result<RequestOutputFingerprint, CompilationDbError>;
}

struct TypedRequestCell<V> {
    node: RequestNodeId,
    value: Option<V>,
}

/// Language-owned values for one statically typed request family.
///
/// A parse key can only publish a parse value because the family associates
/// both types. Cross-family dependencies are captured by [`require`](Self::require)
/// into the shared [`RequestEvaluatorGraph`]; no `Any`, sum-value enum, or
/// caller-declared dependency list is involved.
pub struct TypedRequestTable<F: RequestFamily> {
    cells: BTreeMap<F::Key, TypedRequestCell<F::Value>>,
    keys_by_node: BTreeMap<RequestNodeId, F::Key>,
}

impl<F: RequestFamily> Default for TypedRequestTable<F> {
    fn default() -> Self {
        Self {
            cells: BTreeMap::new(),
            keys_by_node: BTreeMap::new(),
        }
    }
}

impl<F: RequestFamily> TypedRequestTable<F> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_count(&self) -> usize {
        self.cells.len()
    }

    fn ensure_node(
        &mut self,
        graph: &mut RequestEvaluatorGraph,
        key: &F::Key,
    ) -> Result<RequestNodeId, CompilationDbError> {
        if let Some(cell) = self.cells.get(key) {
            return Ok(cell.node);
        }
        let node = graph.register(RequestNodeIdentity {
            family: F::NAME,
            key_fingerprint: F::key_fingerprint(key),
        })?;
        if let Some(previous) = self.keys_by_node.get(&node)
            && previous != key
        {
            return Err(CompilationDbError::new(format!(
                "request family {} key fingerprint collision between {previous:?} and {key:?}",
                F::NAME
            )));
        }
        self.keys_by_node.insert(node, key.clone());
        self.cells
            .insert(key.clone(), TypedRequestCell { node, value: None });
        Ok(node)
    }

    pub fn begin(
        &mut self,
        graph: &mut RequestEvaluatorGraph,
        key: F::Key,
        input_fingerprint: RequestInputFingerprint,
    ) -> Result<RequestStart, CompilationDbError> {
        let node = self.ensure_node(graph, &key)?;
        graph.begin(node, input_fingerprint)
    }

    pub fn require<'a>(
        &'a self,
        graph: &RequestEvaluatorGraph,
        ticket: &mut RequestEvaluationTicket,
        key: &F::Key,
    ) -> Result<&'a F::Value, CompilationDbError> {
        let cell = self.cells.get(key).ok_or_else(|| {
            CompilationDbError::new(format!(
                "request family {} dependency key is absent",
                F::NAME
            ))
        })?;
        graph.require(ticket, cell.node)?;
        cell.value.as_ref().ok_or_else(|| {
            CompilationDbError::new(format!(
                "request family {} dependency has no typed value",
                F::NAME
            ))
        })
    }

    pub fn publish(
        &mut self,
        graph: &mut RequestEvaluatorGraph,
        ticket: RequestEvaluationTicket,
        value: F::Value,
    ) -> Result<(), CompilationDbError> {
        let key = self
            .keys_by_node
            .get(&ticket.node)
            .cloned()
            .ok_or_else(|| {
                CompilationDbError::new(format!(
                    "request family {} ticket has no typed key",
                    F::NAME
                ))
            })?;
        let result_fingerprint = match F::output_fingerprint(&value) {
            Ok(result_fingerprint) => result_fingerprint,
            Err(error) => {
                graph.abort(ticket, RequestAbortReason::Failed)?;
                return Err(error);
            }
        };
        let changed = graph.publish(ticket, result_fingerprint)?;
        let cell = self
            .cells
            .get_mut(&key)
            .expect("ticket typed key remains present");
        if changed || cell.value.is_none() {
            cell.value = Some(value);
        }
        Ok(())
    }

    pub fn abort(
        &mut self,
        graph: &mut RequestEvaluatorGraph,
        ticket: RequestEvaluationTicket,
        reason: RequestAbortReason,
    ) -> Result<(), CompilationDbError> {
        graph.abort(ticket, reason)
    }

    pub fn current_value<'a>(
        &'a self,
        graph: &RequestEvaluatorGraph,
        key: &F::Key,
    ) -> Result<Option<&'a F::Value>, CompilationDbError> {
        if !graph.active.is_empty() {
            return Err(CompilationDbError::new(format!(
                "request family {} cannot read a current value during evaluation; use require",
                F::NAME
            )));
        }
        let Some(cell) = self.cells.get(key) else {
            return Ok(None);
        };
        Ok(graph
            .is_current(cell.node)
            .then(|| cell.value.as_ref())
            .flatten())
    }

    pub fn memo<'a>(
        &self,
        graph: &'a RequestEvaluatorGraph,
        key: &F::Key,
    ) -> Option<&'a RequestMemo> {
        graph.memo(self.cells.get(key)?.node)
    }

    pub fn node_id(&self, key: &F::Key) -> Option<RequestNodeId> {
        self.cells.get(key).map(|cell| cell.node)
    }

    pub fn retain(
        &mut self,
        graph: &mut RequestEvaluatorGraph,
        mut retain: impl FnMut(&F::Key) -> bool,
    ) -> Result<(), CompilationDbError> {
        let removed = self
            .cells
            .iter()
            .filter_map(|(key, cell)| (!retain(key)).then_some((key.clone(), cell.node)))
            .collect::<Vec<_>>();
        let nodes = removed.iter().map(|(_, node)| *node).collect::<Vec<_>>();
        graph.remove_nodes(&nodes)?;
        for (key, node) in removed {
            self.cells.remove(&key);
            self.keys_by_node.remove(&node);
        }
        Ok(())
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

    struct NumberRequest;

    impl RequestFamily for NumberRequest {
        type Key = u8;
        type Value = u32;

        const NAME: &'static str = "test.number";

        fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
            digest(*key)
        }

        fn output_fingerprint(
            value: &Self::Value,
        ) -> Result<RequestOutputFingerprint, CompilationDbError> {
            let mut fingerprint = [0; 32];
            fingerprint[..4].copy_from_slice(&value.to_le_bytes());
            Ok(RequestOutputFingerprint(fingerprint))
        }
    }

    struct TextRequest;

    impl RequestFamily for TextRequest {
        type Key = u8;
        type Value = String;

        const NAME: &'static str = "test.text";

        fn key_fingerprint(key: &Self::Key) -> RequestFingerprint {
            digest(*key)
        }

        fn output_fingerprint(
            value: &Self::Value,
        ) -> Result<RequestOutputFingerprint, CompilationDbError> {
            let mut hasher = Sha256::new();
            hasher.update(b"test.text.output.v1\0");
            hasher.update(value.as_bytes());
            Ok(RequestOutputFingerprint(hasher.finalize().into()))
        }
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
    fn request_tables_capture_cross_family_dependencies_and_backdate() {
        let mut graph = RequestEvaluatorGraph::new(Revision(0));
        let mut numbers = TypedRequestTable::<NumberRequest>::new();
        let mut texts = TypedRequestTable::<TextRequest>::new();

        let RequestStart::Execute(number_ticket) = numbers
            .begin(&mut graph, 1, RequestInputFingerprint(digest(1)))
            .unwrap()
        else {
            panic!("new number request executes");
        };
        numbers.publish(&mut graph, number_ticket, 10).unwrap();

        let RequestStart::Execute(mut text_ticket) = texts
            .begin(&mut graph, 2, RequestInputFingerprint(digest(2)))
            .unwrap()
        else {
            panic!("new text request executes");
        };
        let omitted_edge = numbers
            .current_value(&graph, &1)
            .expect_err("raw value reads during evaluation must fail closed");
        assert!(omitted_edge.to_string().contains("use require"));
        let number = numbers
            .require(&graph, &mut text_ticket, &1)
            .copied()
            .unwrap();
        texts
            .publish(&mut graph, text_ticket, format!("value={number}"))
            .unwrap();

        let number_node = numbers.node_id(&1).unwrap();
        let text_node = texts.node_id(&2).unwrap();
        assert_eq!(
            graph.dependencies(text_node),
            Some([number_node].as_slice())
        );
        assert_eq!(
            graph.reverse_cone([number_node]),
            BTreeSet::from([number_node, text_node])
        );

        graph.advance_to(Revision(1)).unwrap();
        assert!(matches!(
            numbers
                .begin(&mut graph, 1, RequestInputFingerprint(digest(1)))
                .unwrap(),
            RequestStart::Reused
        ));
        assert!(matches!(
            texts
                .begin(&mut graph, 2, RequestInputFingerprint(digest(2)))
                .unwrap(),
            RequestStart::Reused
        ));

        graph.advance_to(Revision(2)).unwrap();
        let RequestStart::Execute(number_ticket) = numbers
            .begin(&mut graph, 1, RequestInputFingerprint(digest(3)))
            .unwrap()
        else {
            panic!("changed number input executes");
        };
        numbers.publish(&mut graph, number_ticket, 10).unwrap();
        assert_eq!(graph.memo(number_node).unwrap().changed_at, Revision(0));
        assert!(matches!(
            texts
                .begin(&mut graph, 2, RequestInputFingerprint(digest(2)))
                .unwrap(),
            RequestStart::Reused
        ));

        let stats = graph.stats();
        assert_eq!(stats.demanded, 6);
        assert_eq!(stats.executed, 3);
        assert_eq!(stats.reused, 3);
        assert_eq!(stats.backdated, 1);
        assert_eq!(stats.changed, 2);
    }

    #[test]
    fn request_graph_replaces_edges_and_keeps_removed_dependencies_stale() {
        let mut graph = RequestEvaluatorGraph::new(Revision(0));
        let mut requests = TypedRequestTable::<NumberRequest>::new();
        for key in [1, 3] {
            let RequestStart::Execute(ticket) = requests
                .begin(&mut graph, key, RequestInputFingerprint(digest(key)))
                .unwrap()
            else {
                panic!("new leaf executes");
            };
            requests
                .publish(&mut graph, ticket, u32::from(key))
                .unwrap();
        }
        let RequestStart::Execute(mut ticket) = requests
            .begin(&mut graph, 2, RequestInputFingerprint(digest(2)))
            .unwrap()
        else {
            panic!("new dependent executes");
        };
        requests.require(&graph, &mut ticket, &1).unwrap();
        requests.publish(&mut graph, ticket, 2).unwrap();

        let first = requests.node_id(&1).unwrap();
        let dependent = requests.node_id(&2).unwrap();
        let replacement = requests.node_id(&3).unwrap();
        graph.advance_to(Revision(1)).unwrap();
        for key in [1, 3] {
            assert!(matches!(
                requests
                    .begin(&mut graph, key, RequestInputFingerprint(digest(key)))
                    .unwrap(),
                RequestStart::Reused
            ));
        }
        let RequestStart::Execute(mut ticket) = requests
            .begin(&mut graph, 2, RequestInputFingerprint(digest(4)))
            .unwrap()
        else {
            panic!("changed dependent executes");
        };
        requests.require(&graph, &mut ticket, &3).unwrap();
        requests.publish(&mut graph, ticket, 2).unwrap();
        assert_eq!(graph.reverse_cone([first]), BTreeSet::from([first]));
        assert_eq!(
            graph.reverse_cone([replacement]),
            BTreeSet::from([dependent, replacement])
        );

        graph.advance_to(Revision(2)).unwrap();
        requests.retain(&mut graph, |key| *key != 3).unwrap();
        assert_eq!(requests.current_value(&graph, &2).unwrap(), None);
        assert_eq!(
            graph.reverse_cone([replacement]),
            BTreeSet::from([dependent, replacement])
        );
    }

    #[test]
    fn request_graph_reports_indirect_cycles_and_rejects_stale_tickets() {
        let mut graph = RequestEvaluatorGraph::new(Revision(0));
        let mut requests = TypedRequestTable::<NumberRequest>::new();
        let RequestStart::Execute(first) = requests
            .begin(&mut graph, 1, RequestInputFingerprint(digest(1)))
            .unwrap()
        else {
            panic!("first request executes");
        };
        let RequestStart::Execute(second) = requests
            .begin(&mut graph, 2, RequestInputFingerprint(digest(2)))
            .unwrap()
        else {
            panic!("second request executes");
        };
        let RequestStart::Execute(third) = requests
            .begin(&mut graph, 3, RequestInputFingerprint(digest(3)))
            .unwrap()
        else {
            panic!("third request executes");
        };
        let cycle = requests
            .begin(&mut graph, 1, RequestInputFingerprint(digest(1)))
            .unwrap_err();
        assert!(cycle.to_string().contains("compilation request cycle"));
        assert!(cycle.to_string().contains("test.number"));
        requests
            .abort(&mut graph, third, RequestAbortReason::Canceled)
            .unwrap();
        requests
            .abort(&mut graph, second, RequestAbortReason::Canceled)
            .unwrap();
        requests
            .abort(&mut graph, first, RequestAbortReason::Canceled)
            .unwrap();

        let RequestStart::Execute(stale) = requests
            .begin(&mut graph, 4, RequestInputFingerprint(digest(4)))
            .unwrap()
        else {
            panic!("new request executes");
        };
        graph.advance_to(Revision(1)).unwrap();
        let error = requests.publish(&mut graph, stale, 4).unwrap_err();
        assert!(error.to_string().contains("superseded generation"));
        assert_eq!(graph.stats().superseded, 1);
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
