use crate::{
    OwnerExpressionInput, OwnerExternalExpressionInput, OwnerSyntaxInput,
    stable_check_owner_key_fingerprint_v1,
};
use boon_checked::{OwnerExpressionId, OwnerExpressionRef, OwnerStatementChild, OwnerStatementId};
use boon_compilation_db::{
    DenseProjectionGraphBuilder, ProjectionGraphDigestDomains, ProjectionGraphStats, ProjectionId,
};
use boon_syntax::{
    AstCallArgKind, AstDrainPath, AstExprKind, AstMatchPattern, AstStatementKind, AstTextSegment,
    BytesSizeSyntax, StableCheckOwnerKey, StableExpressionKey,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

const OWNER_CONSTRAINT_SEED_DOMAIN_V1: &[u8] = b"boon.owner-constraint-seed.v1\0";
const OWNER_CONSTRAINT_TOPOLOGY_DOMAIN_V1: &[u8] = b"boon.owner-constraint-topology.v1\0";
const OWNER_RESOLVED_CONSTRAINT_SUMMARY_DOMAIN_V1: &[u8] =
    b"boon.owner-resolved-constraint-summary.v1\0";
const OWNER_RESOLVED_CONSTRAINT_TOPOLOGY_DOMAIN_V1: &[u8] =
    b"boon.owner-resolved-constraint-topology.v1\0";
const OWNER_INTERFACE_COMPONENT_DOMAIN_V1: &[u8] = b"boon.owner-interface-component.v1\0";
const OWNER_INTERFACE_SCC_DOMAIN_V1: &[u8] = b"boon.owner-interface-scc.v1\0";
const OWNER_INTERFACE_TOPOLOGY_DOMAIN_V1: &[u8] = b"boon.owner-interface-topology.v1\0";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerConstraintSeedError {
    message: String,
}

impl OwnerConstraintSeedError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OwnerConstraintSeedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for OwnerConstraintSeedError {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerDeclarationKind {
    Function,
    Field,
    Source,
    Hold,
    List,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerParameterKind {
    Value,
    Out,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OwnerParameterConstraint {
    pub name: String,
    pub kind: OwnerParameterKind,
    pub ordinal: u32,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OwnerDeclarationConstraint {
    pub statement: u32,
    pub public: bool,
    pub kind: OwnerDeclarationKind,
    pub names: Box<[String]>,
    pub parameters: Box<[OwnerParameterConstraint]>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerReferenceKind {
    Value,
    Callable,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OwnerSymbolReference {
    pub expression: StableExpressionKey,
    pub kind: OwnerReferenceKind,
    pub parts: Box<[String]>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerPatternConstraint {
    Wildcard,
    Number,
    Text,
    Tag { name: String, fields: Box<[String]> },
    Binding { name: String },
    Invalid,
    Bits { width: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerCollectionKind {
    List,
    Bytes,
    Set,
    Map,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerConstraintNodeKind {
    Text,
    TextTemplate,
    Number,
    Byte,
    Bits {
        width: u32,
    },
    Tag {
        name: String,
    },
    Source,
    Reference {
        parts: Box<[String]>,
    },
    Drain {
        parts: Box<[String]>,
    },
    Record {
        tag: Option<String>,
    },
    Flush,
    Call {
        function: String,
    },
    Pipe {
        operation: String,
    },
    Draining,
    Hold {
        name: String,
    },
    Latest,
    When,
    Then,
    Infix {
        operation: String,
    },
    MatchArm {
        pattern: OwnerPatternConstraint,
    },
    Block,
    Collection {
        collection: OwnerCollectionKind,
        fixed_size_or_capacity: Option<u32>,
    },
    Arrow {
        pattern: OwnerPatternConstraint,
    },
    MapEntry,
    Delimiter,
    Unknown {
        tokens: Box<[String]>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum OwnerConstraintEdgeRole {
    TextDynamic,
    RecordField {
        name: String,
        spread: bool,
    },
    FlushPayload,
    CallArgument {
        ordinal: u32,
        kind: OwnerArgumentKind,
        name: String,
    },
    CallPass {
        final_clause: bool,
    },
    PipeInput,
    PipeArgument {
        ordinal: u32,
        kind: OwnerArgumentKind,
        name: String,
    },
    PipePass {
        final_clause: bool,
    },
    PipeArm,
    DrainingInput,
    HoldInitial,
    LatestBranch,
    WhenInput,
    WhenArm,
    ThenInput,
    ThenOutput,
    InfixLeft,
    InfixRight,
    MatchOutput,
    BlockBinding {
        name: String,
    },
    BlockResult,
    CollectionItem,
    ArrowLeft,
    ArrowOutput,
    MapKey,
    MapValue,
    MapEntry,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerArgumentKind {
    BareBinding,
    Named,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerConstraintEdge {
    pub role: OwnerConstraintEdgeRole,
    pub expression: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerExpressionConstraint {
    pub expression: StableExpressionKey,
    pub kind: OwnerConstraintNodeKind,
    pub inputs: Box<[OwnerConstraintEdge]>,
}

/// Static FLUSH-control edges for one local expression.
///
/// `value_inputs` contribute their ordinary value type (the direct FLUSH
/// payload). `escape_inputs` contribute the child's FLUSH channel. References
/// use the same local-plus-external u32 namespace as ordinary expression
/// constraints; callable-result FLUSH edges are added after symbol resolution.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerFlushConstraint {
    pub value_inputs: Box<[u32]>,
    pub escape_inputs: Box<[u32]>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct OwnerEffectConstraintSeed {
    pub declares_source: bool,
    pub declares_state: bool,
    pub declares_list: bool,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OwnerResultStaticNumber {
    pub expression: StableExpressionKey,
    pub literal: String,
}

/// Interface-relevant constraints for one owner.
///
/// Source positions and body-only literal payloads are deliberately absent.
/// The sole payload exception is `result_static_numbers`: exact numeric leaves
/// needed to determine a public function's static Bits result. Other literal
/// edits re-execute this projection and backdate when the interface surface is
/// identical.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerConstraintSeed {
    pub owner: StableCheckOwnerKey,
    pub declarations: Box<[OwnerDeclarationConstraint]>,
    pub statement_values: Box<[(u32, u32)]>,
    pub expressions: Box<[OwnerExpressionConstraint]>,
    /// Per-expression FLUSH-control graph. Declaration/callable boundaries
    /// union the parallel escape channel into the ordinary public flow.
    pub expression_flush_plans: Box<[OwnerFlushConstraint]>,
    pub references: Box<[OwnerSymbolReference]>,
    pub external_expressions: Box<[OwnerExternalExpressionInput]>,
    pub result_static_numbers: Box<[OwnerResultStaticNumber]>,
    pub effect_seed: OwnerEffectConstraintSeed,
    fingerprint_v1: [u8; 32],
    topology_fingerprint_v1: [u8; 32],
}

impl OwnerConstraintSeed {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub const fn topology_fingerprint_v1(&self) -> [u8; 32] {
        self.topology_fingerprint_v1
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerConstraintDependencyKind {
    ChildValue,
    ValueRead,
    CallResult,
    CallEffect,
    ActualToFormal,
    PipeInputToFormal,
    FreshOutFromFormal,
    ForwardOut,
    PassedContext,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OwnerConstraintDependency {
    /// The interface request that consumes `dependency`.
    pub request: StableCheckOwnerKey,
    pub dependency: StableCheckOwnerKey,
    pub kind: OwnerConstraintDependencyKind,
    pub expression: StableExpressionKey,
    pub parameter_ordinal: Option<u32>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OwnerInterfaceTopologyEdge {
    pub request: StableCheckOwnerKey,
    pub dependency: StableCheckOwnerKey,
    pub kind: OwnerConstraintDependencyKind,
    pub parameter_ordinal: Option<u32>,
}

impl From<&OwnerConstraintDependency> for OwnerInterfaceTopologyEdge {
    fn from(edge: &OwnerConstraintDependency) -> Self {
        Self {
            request: edge.request.clone(),
            dependency: edge.dependency.clone(),
            kind: edge.kind,
            parameter_ordinal: edge.parameter_ordinal,
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ResolvedOwnerSymbolReference {
    pub reference: OwnerSymbolReference,
    pub owner: StableCheckOwnerKey,
    /// Path suffix selected below the stable owner declaration.
    ///
    /// Project symbol lookup resolves the longest declaration prefix and
    /// retains the remaining fields here.  Treating an object-field read as
    /// the complete owner value would give owner-local inference a wider and
    /// sometimes entirely different type.
    pub projection: Box<[String]>,
    pub parameters: Box<[OwnerParameterConstraint]>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct AmbiguousOwnerSymbolCandidate {
    pub owner: StableCheckOwnerKey,
    pub parameters: Box<[OwnerParameterConstraint]>,
}

/// Exact project/authoritative resolution state for one owner reference.
///
/// Absence and ambiguity are deliberately retained instead of being collapsed
/// into a missing dependency edge. Later body diagnostics can therefore
/// distinguish an unknown name from multiple equally ranked project targets,
/// while authoritative callables remain explicit inputs to ABI currentness.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "resolution", rename_all = "snake_case")]
pub enum OwnerSymbolResolution {
    Resolved {
        reference: OwnerSymbolReference,
        owner: StableCheckOwnerKey,
        projection: Box<[String]>,
        parameters: Box<[OwnerParameterConstraint]>,
    },
    Authoritative {
        reference: OwnerSymbolReference,
    },
    Unresolved {
        reference: OwnerSymbolReference,
    },
    Ambiguous {
        reference: OwnerSymbolReference,
        candidates: Box<[AmbiguousOwnerSymbolCandidate]>,
    },
}

impl OwnerSymbolResolution {
    pub fn reference(&self) -> &OwnerSymbolReference {
        match self {
            Self::Resolved { reference, .. }
            | Self::Authoritative { reference }
            | Self::Unresolved { reference }
            | Self::Ambiguous { reference, .. } => reference,
        }
    }

    fn resolved(&self) -> Option<ResolvedOwnerSymbolReference> {
        match self {
            Self::Resolved {
                reference,
                owner,
                projection,
                parameters,
            } => Some(ResolvedOwnerSymbolReference {
                reference: reference.clone(),
                owner: owner.clone(),
                projection: projection.clone(),
                parameters: parameters.clone(),
            }),
            Self::Authoritative { .. } | Self::Unresolved { .. } | Self::Ambiguous { .. } => None,
        }
    }
}

impl From<ResolvedOwnerSymbolReference> for OwnerSymbolResolution {
    fn from(resolved: ResolvedOwnerSymbolReference) -> Self {
        Self::Resolved {
            reference: resolved.reference,
            owner: resolved.owner,
            projection: resolved.projection,
            parameters: resolved.parameters,
        }
    }
}
/// Symbol-resolved owner dependency and constraint identity.
///
/// The large flat constraint nodes remain shared in [`OwnerConstraintSeed`].
/// This summary owns only stable dependency keys and fixed fingerprints, so
/// SCC topology can be rebuilt without copying syntax-shaped constraint rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerConstraintSummary {
    pub owner: StableCheckOwnerKey,
    pub seed_fingerprint_v1: [u8; 32],
    /// Exact symbol targets used by the SCC solver and later body checker.
    ///
    /// Keeping this mapping beside the dependency rows prevents those stages
    /// from repeating project name lookup or guessing a target from an edge
    /// whose direction was inverted for `ActualToFormal`/`PassedContext`.
    pub resolved_references: Box<[ResolvedOwnerSymbolReference]>,
    /// One lossless state for every reference in the seed, including
    /// authoritative, unresolved, and ambiguous references that create no
    /// project-interface dependency edge.
    pub symbol_resolutions: Box<[OwnerSymbolResolution]>,
    pub dependencies: Box<[OwnerConstraintDependency]>,
    fingerprint_v1: [u8; 32],
    topology_fingerprint_v1: [u8; 32],
}

impl OwnerConstraintSummary {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub const fn topology_fingerprint_v1(&self) -> [u8; 32] {
        self.topology_fingerprint_v1
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OwnerInterfaceSccKey {
    pub members: Box<[StableCheckOwnerKey]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerInterfaceScc {
    pub key: OwnerInterfaceSccKey,
    pub dependencies: Box<[OwnerInterfaceSccKey]>,
    pub edges: Box<[OwnerInterfaceTopologyEdge]>,
    fingerprint_v1: [u8; 32],
}

impl OwnerInterfaceScc {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerInterfaceTopology {
    pub sccs: Box<[OwnerInterfaceScc]>,
    pub stats: ProjectionGraphStats,
    fingerprint_v1: [u8; 32],
}

impl OwnerInterfaceTopology {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub fn scc_for_owner(&self, owner: &StableCheckOwnerKey) -> Option<&OwnerInterfaceScc> {
        self.sccs
            .iter()
            .find(|scc| scc.key.members.binary_search(owner).is_ok())
    }
}

pub fn build_owner_interface_topology<'a>(
    summaries: impl IntoIterator<Item = &'a OwnerConstraintSummary>,
) -> Result<OwnerInterfaceTopology, OwnerConstraintSeedError> {
    let mut summaries = summaries.into_iter().collect::<Vec<_>>();
    summaries.sort_by(|left, right| left.owner.cmp(&right.owner));
    if summaries
        .windows(2)
        .any(|pair| pair[0].owner == pair[1].owner)
    {
        return Err(OwnerConstraintSeedError::new(
            "owner interface topology contains a duplicate owner",
        ));
    }

    let mut builder = DenseProjectionGraphBuilder::new();
    let mut projection_by_owner = BTreeMap::<StableCheckOwnerKey, ProjectionId>::new();
    let mut owner_by_projection = BTreeMap::<ProjectionId, StableCheckOwnerKey>::new();
    for summary in &summaries {
        let projection = builder
            .register(
                stable_check_owner_key_fingerprint_v1(&summary.owner),
                summary.topology_fingerprint_v1(),
            )
            .map_err(|error| {
                OwnerConstraintSeedError::new(format!(
                    "cannot register owner interface topology: {error}"
                ))
            })?;
        projection_by_owner.insert(summary.owner.clone(), projection);
        owner_by_projection.insert(projection, summary.owner.clone());
    }

    let mut edges = summaries
        .iter()
        .flat_map(|summary| summary.dependencies.iter().cloned())
        .collect::<Vec<_>>();
    edges.sort();
    edges.dedup();
    let topology_edges = edges
        .iter()
        .map(OwnerInterfaceTopologyEdge::from)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    for edge in &edges {
        let request = projection_by_owner
            .get(&edge.request)
            .copied()
            .ok_or_else(|| {
                OwnerConstraintSeedError::new(format!(
                    "owner interface edge request {:?} is not registered",
                    edge.request
                ))
            })?;
        let dependency = projection_by_owner
            .get(&edge.dependency)
            .copied()
            .ok_or_else(|| {
                OwnerConstraintSeedError::new(format!(
                    "owner interface edge dependency {:?} is not registered",
                    edge.dependency
                ))
            })?;
        builder
            .add_dependency(request, dependency)
            .map_err(|error| {
                OwnerConstraintSeedError::new(format!(
                    "cannot add owner interface dependency: {error}"
                ))
            })?;
    }

    let graph = builder
        .seal(ProjectionGraphDigestDomains {
            component: OWNER_INTERFACE_COMPONENT_DOMAIN_V1,
        })
        .map_err(|error| {
            OwnerConstraintSeedError::new(format!("cannot seal owner interface topology: {error}"))
        })?;
    let mut component_keys = Vec::with_capacity(graph.component_count());
    let mut component_by_owner = BTreeMap::<StableCheckOwnerKey, usize>::new();
    for component in 0..graph.component_count() {
        let mut members = graph
            .component_members_by_ordinal(component)
            .ok_or_else(|| {
                OwnerConstraintSeedError::new(format!(
                    "owner interface component {component} has no member range"
                ))
            })?
            .map(|projection| {
                owner_by_projection
                    .get(&projection)
                    .cloned()
                    .ok_or_else(|| {
                        OwnerConstraintSeedError::new(
                            "owner interface component contains an unknown projection",
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        members.sort();
        if members.is_empty() {
            return Err(OwnerConstraintSeedError::new(format!(
                "owner interface component {component} is empty"
            )));
        }
        for member in &members {
            component_by_owner.insert(member.clone(), component);
        }
        component_keys.push(OwnerInterfaceSccKey {
            members: members.into_boxed_slice(),
        });
    }

    let mut sccs = Vec::with_capacity(component_keys.len());
    for (component, key) in component_keys.iter().cloned().enumerate() {
        let members = key.members.iter().cloned().collect::<BTreeSet<_>>();
        let component_edges = topology_edges
            .iter()
            .filter(|edge| members.contains(&edge.request))
            .cloned()
            .collect::<Vec<_>>();
        let dependency_components = component_edges
            .iter()
            .map(|edge| component_by_owner[&edge.dependency])
            .filter(|dependency| *dependency != component)
            .collect::<BTreeSet<_>>();
        if dependency_components
            .iter()
            .any(|dependency| *dependency >= component)
        {
            return Err(OwnerConstraintSeedError::new(format!(
                "owner interface component {component} is not dependency-first"
            )));
        }
        let dependencies = dependency_components
            .iter()
            .map(|dependency| component_keys[*dependency].clone())
            .collect::<Vec<_>>();
        let member_fingerprints = summaries
            .iter()
            .filter(|summary| members.contains(&summary.owner))
            .map(|summary| {
                (
                    stable_check_owner_key_fingerprint_v1(&summary.owner),
                    summary.topology_fingerprint_v1(),
                )
            })
            .collect::<Vec<_>>();
        let fingerprint_v1 = fingerprint(
            OWNER_INTERFACE_SCC_DOMAIN_V1,
            &(&key, &dependencies, &component_edges, &member_fingerprints),
        )?;
        sccs.push(OwnerInterfaceScc {
            key,
            dependencies: dependencies.into_boxed_slice(),
            edges: component_edges.into_boxed_slice(),
            fingerprint_v1,
        });
    }
    let stats = graph.stats();
    let stat_values = (
        stats.nodes,
        stats.edges,
        stats.components,
        stats.cyclic_components,
        stats.maximum_component_nodes,
        stats.component_edges,
    );
    let scc_fingerprints = sccs
        .iter()
        .map(OwnerInterfaceScc::fingerprint_v1)
        .collect::<Vec<_>>();
    let fingerprint_v1 = fingerprint(
        OWNER_INTERFACE_TOPOLOGY_DOMAIN_V1,
        &(&stat_values, &scc_fingerprints),
    )?;
    Ok(OwnerInterfaceTopology {
        sccs: sccs.into_boxed_slice(),
        stats,
        fingerprint_v1,
    })
}

fn interface_edge(
    request: &StableCheckOwnerKey,
    dependency: &StableCheckOwnerKey,
    kind: OwnerConstraintDependencyKind,
    expression: &StableExpressionKey,
    parameter_ordinal: Option<u32>,
) -> OwnerConstraintDependency {
    OwnerConstraintDependency {
        request: request.clone(),
        dependency: dependency.clone(),
        kind,
        expression: expression.clone(),
        parameter_ordinal,
    }
}

fn append_callable_interface_dependencies(
    seed: &OwnerConstraintSeed,
    resolved: ResolvedOwnerSymbolReference,
    dependencies: &mut BTreeSet<OwnerConstraintDependency>,
) -> Result<(), OwnerConstraintSeedError> {
    let ResolvedOwnerSymbolReference {
        reference,
        owner: callable,
        projection: _,
        parameters,
    } = resolved;
    let expression = seed
        .expressions
        .iter()
        .find(|expression| expression.expression == reference.expression)
        .ok_or_else(|| {
            OwnerConstraintSeedError::new(format!(
                "owner {:?} callable reference {:?} has no constraint expression",
                seed.owner, reference.expression
            ))
        })?;
    if !matches!(
        expression.kind,
        OwnerConstraintNodeKind::Call { .. } | OwnerConstraintNodeKind::Pipe { .. }
    ) {
        return Err(OwnerConstraintSeedError::new(format!(
            "owner {:?} callable reference {:?} does not name a call expression",
            seed.owner, reference.expression
        )));
    }

    dependencies.insert(interface_edge(
        &seed.owner,
        &callable,
        OwnerConstraintDependencyKind::CallResult,
        &reference.expression,
        None,
    ));
    dependencies.insert(interface_edge(
        &seed.owner,
        &callable,
        OwnerConstraintDependencyKind::CallEffect,
        &reference.expression,
        None,
    ));

    let value_parameter = parameters
        .iter()
        .filter(|parameter| parameter.kind == OwnerParameterKind::Value)
        .min_by_key(|parameter| parameter.ordinal);
    for input in &expression.inputs {
        match &input.role {
            OwnerConstraintEdgeRole::PipeInput => {
                if let Some(parameter) = value_parameter {
                    dependencies.insert(interface_edge(
                        &callable,
                        &seed.owner,
                        OwnerConstraintDependencyKind::PipeInputToFormal,
                        &reference.expression,
                        Some(parameter.ordinal),
                    ));
                }
            }
            OwnerConstraintEdgeRole::CallArgument { kind, name, .. }
            | OwnerConstraintEdgeRole::PipeArgument { kind, name, .. } => {
                let Some(parameter) = parameters.iter().find(|parameter| parameter.name == *name)
                else {
                    continue;
                };
                match (parameter.kind, kind) {
                    (OwnerParameterKind::Value, OwnerArgumentKind::Named) => {
                        dependencies.insert(interface_edge(
                            &callable,
                            &seed.owner,
                            OwnerConstraintDependencyKind::ActualToFormal,
                            &reference.expression,
                            Some(parameter.ordinal),
                        ));
                    }
                    (OwnerParameterKind::Out, OwnerArgumentKind::BareBinding) => {
                        dependencies.insert(interface_edge(
                            &seed.owner,
                            &callable,
                            OwnerConstraintDependencyKind::FreshOutFromFormal,
                            &reference.expression,
                            Some(parameter.ordinal),
                        ));
                    }
                    (OwnerParameterKind::Out, OwnerArgumentKind::Named) => {
                        dependencies.insert(interface_edge(
                            &callable,
                            &seed.owner,
                            OwnerConstraintDependencyKind::ForwardOut,
                            &reference.expression,
                            Some(parameter.ordinal),
                        ));
                        dependencies.insert(interface_edge(
                            &seed.owner,
                            &callable,
                            OwnerConstraintDependencyKind::ForwardOut,
                            &reference.expression,
                            Some(parameter.ordinal),
                        ));
                    }
                    (OwnerParameterKind::Value, OwnerArgumentKind::BareBinding) => {}
                }
            }
            OwnerConstraintEdgeRole::CallPass { .. } | OwnerConstraintEdgeRole::PipePass { .. } => {
                dependencies.insert(interface_edge(
                    &callable,
                    &seed.owner,
                    OwnerConstraintDependencyKind::PassedContext,
                    &reference.expression,
                    None,
                ));
            }
            OwnerConstraintEdgeRole::TextDynamic
            | OwnerConstraintEdgeRole::RecordField { .. }
            | OwnerConstraintEdgeRole::FlushPayload
            | OwnerConstraintEdgeRole::PipeArm
            | OwnerConstraintEdgeRole::DrainingInput
            | OwnerConstraintEdgeRole::HoldInitial
            | OwnerConstraintEdgeRole::LatestBranch
            | OwnerConstraintEdgeRole::WhenInput
            | OwnerConstraintEdgeRole::WhenArm
            | OwnerConstraintEdgeRole::ThenInput
            | OwnerConstraintEdgeRole::ThenOutput
            | OwnerConstraintEdgeRole::InfixLeft
            | OwnerConstraintEdgeRole::InfixRight
            | OwnerConstraintEdgeRole::MatchOutput
            | OwnerConstraintEdgeRole::BlockBinding { .. }
            | OwnerConstraintEdgeRole::BlockResult
            | OwnerConstraintEdgeRole::CollectionItem
            | OwnerConstraintEdgeRole::ArrowLeft
            | OwnerConstraintEdgeRole::ArrowOutput
            | OwnerConstraintEdgeRole::MapKey
            | OwnerConstraintEdgeRole::MapValue
            | OwnerConstraintEdgeRole::MapEntry => {}
        }
    }
    Ok(())
}

pub fn resolve_owner_constraint_seed(
    seed: &OwnerConstraintSeed,
    referenced_dependencies: impl IntoIterator<Item = ResolvedOwnerSymbolReference>,
) -> Result<OwnerConstraintSummary, OwnerConstraintSeedError> {
    let mut resolutions = referenced_dependencies
        .into_iter()
        .map(OwnerSymbolResolution::from)
        .map(|resolution| (resolution.reference().clone(), resolution))
        .collect::<BTreeMap<_, _>>();
    for reference in &seed.references {
        resolutions.entry(reference.clone()).or_insert_with(|| {
            if reference.kind == OwnerReferenceKind::Callable
                && crate::owner_interface::is_authoritative_callable_name(
                    &reference.parts.join("/"),
                )
            {
                OwnerSymbolResolution::Authoritative {
                    reference: reference.clone(),
                }
            } else {
                OwnerSymbolResolution::Unresolved {
                    reference: reference.clone(),
                }
            }
        });
    }
    resolve_owner_constraint_seed_with_resolutions(seed, resolutions.into_values())
}

pub fn resolve_owner_constraint_seed_with_resolutions(
    seed: &OwnerConstraintSeed,
    resolutions: impl IntoIterator<Item = OwnerSymbolResolution>,
) -> Result<OwnerConstraintSummary, OwnerConstraintSeedError> {
    let mut dependencies = BTreeSet::new();
    let mut resolved_references = BTreeSet::new();
    for external in &seed.external_expressions {
        if external.owner == seed.owner {
            continue;
        }
        dependencies.insert(OwnerConstraintDependency {
            request: seed.owner.clone(),
            dependency: external.owner.clone(),
            kind: OwnerConstraintDependencyKind::ChildValue,
            expression: external.expression.clone(),
            parameter_ordinal: None,
        });
    }
    let mut symbol_resolutions = BTreeMap::new();
    for resolution in resolutions {
        let reference = resolution.reference().clone();
        if !seed.references.contains(&reference) {
            return Err(OwnerConstraintSeedError::new(format!(
                "owner constraint resolution references an expression absent from {:?}",
                seed.owner
            )));
        }
        if symbol_resolutions
            .insert(reference.clone(), resolution.clone())
            .is_some()
        {
            return Err(OwnerConstraintSeedError::new(format!(
                "owner constraint resolution contains duplicate state for {reference:?}"
            )));
        }
        let Some(resolved) = resolution.resolved() else {
            continue;
        };
        resolved_references.insert(resolved.clone());
        match resolved.reference.kind {
            OwnerReferenceKind::Value => {
                dependencies.insert(OwnerConstraintDependency {
                    request: seed.owner.clone(),
                    dependency: resolved.owner,
                    kind: OwnerConstraintDependencyKind::ValueRead,
                    expression: resolved.reference.expression,
                    parameter_ordinal: None,
                });
            }
            OwnerReferenceKind::Callable => {
                append_callable_interface_dependencies(seed, resolved, &mut dependencies)?;
            }
        }
    }
    if symbol_resolutions.keys().collect::<BTreeSet<_>>()
        != seed.references.iter().collect::<BTreeSet<_>>()
    {
        return Err(OwnerConstraintSeedError::new(format!(
            "owner constraint resolutions do not cover every reference in {:?}",
            seed.owner
        )));
    }
    let resolved_references = resolved_references.into_iter().collect::<Vec<_>>();
    let symbol_resolutions = symbol_resolutions.into_values().collect::<Vec<_>>();
    let dependencies = dependencies.into_iter().collect::<Vec<_>>();
    let topology_dependencies = dependencies
        .iter()
        .map(OwnerInterfaceTopologyEdge::from)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let topology_fingerprint_v1 = fingerprint(
        OWNER_RESOLVED_CONSTRAINT_TOPOLOGY_DOMAIN_V1,
        &(
            stable_check_owner_key_fingerprint_v1(&seed.owner),
            seed.topology_fingerprint_v1(),
            &topology_dependencies,
        ),
    )?;
    let fingerprint_v1 = fingerprint(
        OWNER_RESOLVED_CONSTRAINT_SUMMARY_DOMAIN_V1,
        &(
            stable_check_owner_key_fingerprint_v1(&seed.owner),
            seed.fingerprint_v1(),
            &resolved_references,
            &symbol_resolutions,
            &dependencies,
        ),
    )?;
    Ok(OwnerConstraintSummary {
        owner: seed.owner.clone(),
        seed_fingerprint_v1: seed.fingerprint_v1(),
        resolved_references: resolved_references.into_boxed_slice(),
        symbol_resolutions: symbol_resolutions.into_boxed_slice(),
        dependencies: dependencies.into_boxed_slice(),
        fingerprint_v1,
        topology_fingerprint_v1,
    })
}

fn checked_u32(value: usize, context: &str) -> Result<u32, OwnerConstraintSeedError> {
    u32::try_from(value).map_err(|_| {
        OwnerConstraintSeedError::new(format!("{context} exceeds the owner-local u32 bound"))
    })
}

fn fingerprint<T: Serialize>(
    domain: &[u8],
    value: &T,
) -> Result<[u8; 32], OwnerConstraintSeedError> {
    boon_contract::canonical_serde_hash_v1(domain, value).map_err(|error| {
        OwnerConstraintSeedError::new(format!("cannot fingerprint owner constraints: {error}"))
    })
}

fn parameter_kind(kind: boon_syntax::AstParameterKind) -> OwnerParameterKind {
    match kind {
        boon_syntax::AstParameterKind::Value => OwnerParameterKind::Value,
        boon_syntax::AstParameterKind::Out => OwnerParameterKind::Out,
    }
}

fn declaration(
    statement: u32,
    public: bool,
    kind: &AstStatementKind,
) -> Result<Option<OwnerDeclarationConstraint>, OwnerConstraintSeedError> {
    let (kind, names, parameters) = match kind {
        AstStatementKind::Function { name, parameters } => (
            OwnerDeclarationKind::Function,
            vec![name.clone()],
            parameters
                .iter()
                .map(|parameter| {
                    Ok(OwnerParameterConstraint {
                        name: parameter.name.clone(),
                        kind: parameter_kind(parameter.kind),
                        ordinal: checked_u32(parameter.ordinal, "function parameter ordinal")?,
                    })
                })
                .collect::<Result<Vec<_>, OwnerConstraintSeedError>>()?,
        ),
        AstStatementKind::Field { name } => {
            (OwnerDeclarationKind::Field, vec![name.clone()], Vec::new())
        }
        AstStatementKind::Source { field, event } => (
            OwnerDeclarationKind::Source,
            field.iter().chain(event).cloned().collect(),
            Vec::new(),
        ),
        AstStatementKind::Hold { field, name } => (
            OwnerDeclarationKind::Hold,
            field.iter().chain(name).cloned().collect(),
            Vec::new(),
        ),
        AstStatementKind::List { field, .. } => (
            OwnerDeclarationKind::List,
            field.iter().cloned().collect(),
            Vec::new(),
        ),
        AstStatementKind::Block | AstStatementKind::Spread | AstStatementKind::Expression => {
            return Ok(None);
        }
    };
    Ok(Some(OwnerDeclarationConstraint {
        statement,
        public,
        kind,
        names: names.into_boxed_slice(),
        parameters: parameters.into_boxed_slice(),
    }))
}

fn pattern(pattern: &AstMatchPattern) -> OwnerPatternConstraint {
    match pattern {
        AstMatchPattern::Wildcard => OwnerPatternConstraint::Wildcard,
        AstMatchPattern::Number { .. } => OwnerPatternConstraint::Number,
        AstMatchPattern::Text { .. } => OwnerPatternConstraint::Text,
        AstMatchPattern::Tag { name, fields } => OwnerPatternConstraint::Tag {
            name: name.clone(),
            fields: fields.clone().into_boxed_slice(),
        },
        AstMatchPattern::Binding { name } => OwnerPatternConstraint::Binding { name: name.clone() },
        AstMatchPattern::Invalid { .. } => OwnerPatternConstraint::Invalid,
        AstMatchPattern::Bits { width, .. } => OwnerPatternConstraint::Bits { width: *width },
    }
}

fn argument_kind(kind: AstCallArgKind) -> OwnerArgumentKind {
    match kind {
        AstCallArgKind::BareBinding => OwnerArgumentKind::BareBinding,
        AstCallArgKind::Named => OwnerArgumentKind::Named,
    }
}

fn expression_ref(value: usize) -> Result<u32, OwnerConstraintSeedError> {
    checked_u32(value, "owner expression reference")
}

fn push_edge(
    inputs: &mut Vec<OwnerConstraintEdge>,
    role: OwnerConstraintEdgeRole,
    expression: usize,
) -> Result<(), OwnerConstraintSeedError> {
    inputs.push(OwnerConstraintEdge {
        role,
        expression: expression_ref(expression)?,
    });
    Ok(())
}

fn drain_parts(path: &AstDrainPath) -> Vec<String> {
    match path {
        AstDrainPath::Binding { name } => vec![name.clone()],
        AstDrainPath::Field { binding, fields } => std::iter::once(binding.clone())
            .chain(fields.iter().cloned())
            .collect(),
        AstDrainPath::Passed { fields } => std::iter::once("PASSED".to_owned())
            .chain(fields.iter().cloned())
            .collect(),
    }
}

fn bytes_size(size: &BytesSizeSyntax) -> Result<Option<u32>, OwnerConstraintSeedError> {
    match size {
        BytesSizeSyntax::Dynamic | BytesSizeSyntax::Infer => Ok(None),
        BytesSizeSyntax::Fixed(size) => checked_u32(*size, "fixed BYTES size").map(Some),
    }
}

fn project_expression(
    expression: &crate::OwnerExpressionInput,
    references: &mut BTreeSet<OwnerSymbolReference>,
) -> Result<OwnerExpressionConstraint, OwnerConstraintSeedError> {
    let mut inputs = Vec::new();
    let kind = match &expression.kind {
        AstExprKind::StringLiteral(_) | AstExprKind::TextLiteral(_) => {
            OwnerConstraintNodeKind::Text
        }
        AstExprKind::TextTemplate { segments } => {
            for segment in segments {
                if let AstTextSegment::Dynamic { value } = segment {
                    push_edge(&mut inputs, OwnerConstraintEdgeRole::TextDynamic, *value)?;
                }
            }
            OwnerConstraintNodeKind::TextTemplate
        }
        AstExprKind::Number(_) => OwnerConstraintNodeKind::Number,
        AstExprKind::ByteLiteral { .. } => OwnerConstraintNodeKind::Byte,
        AstExprKind::BitsLiteral { width, .. } => OwnerConstraintNodeKind::Bits { width: *width },
        AstExprKind::Tag(name) => OwnerConstraintNodeKind::Tag { name: name.clone() },
        AstExprKind::Source => OwnerConstraintNodeKind::Source,
        AstExprKind::Identifier(name) => {
            let parts = vec![name.clone()].into_boxed_slice();
            references.insert(OwnerSymbolReference {
                expression: expression.stable_key.clone(),
                kind: OwnerReferenceKind::Value,
                parts: parts.clone(),
            });
            OwnerConstraintNodeKind::Reference { parts }
        }
        AstExprKind::Path(path) => {
            let parts = path.clone().into_boxed_slice();
            references.insert(OwnerSymbolReference {
                expression: expression.stable_key.clone(),
                kind: OwnerReferenceKind::Value,
                parts: parts.clone(),
            });
            OwnerConstraintNodeKind::Reference { parts }
        }
        AstExprKind::Drain { path } => OwnerConstraintNodeKind::Drain {
            parts: drain_parts(path).into_boxed_slice(),
        },
        AstExprKind::TaggedObject { tag, fields } => {
            for field in fields {
                push_edge(
                    &mut inputs,
                    OwnerConstraintEdgeRole::RecordField {
                        name: field.name.clone(),
                        spread: field.spread,
                    },
                    field.value,
                )?;
            }
            OwnerConstraintNodeKind::Record {
                tag: Some(tag.clone()),
            }
        }
        AstExprKind::Object(fields) => {
            for field in fields {
                push_edge(
                    &mut inputs,
                    OwnerConstraintEdgeRole::RecordField {
                        name: field.name.clone(),
                        spread: field.spread,
                    },
                    field.value,
                )?;
            }
            OwnerConstraintNodeKind::Record { tag: None }
        }
        AstExprKind::Flush { payload } => {
            if let Some(payload) = payload {
                push_edge(&mut inputs, OwnerConstraintEdgeRole::FlushPayload, *payload)?;
            }
            OwnerConstraintNodeKind::Flush
        }
        AstExprKind::Call {
            function,
            args,
            pass,
        } => {
            references.insert(OwnerSymbolReference {
                expression: expression.stable_key.clone(),
                kind: OwnerReferenceKind::Callable,
                parts: function.split('/').map(str::to_owned).collect(),
            });
            for (ordinal, argument) in args.iter().enumerate() {
                push_edge(
                    &mut inputs,
                    OwnerConstraintEdgeRole::CallArgument {
                        ordinal: checked_u32(ordinal, "call argument ordinal")?,
                        kind: argument_kind(argument.kind),
                        name: argument.name.clone(),
                    },
                    argument.value,
                )?;
            }
            if let Some(pass) = pass {
                push_edge(
                    &mut inputs,
                    OwnerConstraintEdgeRole::CallPass {
                        final_clause: pass.final_clause,
                    },
                    pass.value,
                )?;
            }
            OwnerConstraintNodeKind::Call {
                function: function.clone(),
            }
        }
        AstExprKind::Pipe {
            input,
            op,
            args,
            pass,
            arms,
        } => {
            references.insert(OwnerSymbolReference {
                expression: expression.stable_key.clone(),
                kind: OwnerReferenceKind::Callable,
                parts: op.split('/').map(str::to_owned).collect(),
            });
            push_edge(&mut inputs, OwnerConstraintEdgeRole::PipeInput, *input)?;
            for (ordinal, argument) in args.iter().enumerate() {
                push_edge(
                    &mut inputs,
                    OwnerConstraintEdgeRole::PipeArgument {
                        ordinal: checked_u32(ordinal, "pipe argument ordinal")?,
                        kind: argument_kind(argument.kind),
                        name: argument.name.clone(),
                    },
                    argument.value,
                )?;
            }
            if let Some(pass) = pass {
                push_edge(
                    &mut inputs,
                    OwnerConstraintEdgeRole::PipePass {
                        final_clause: pass.final_clause,
                    },
                    pass.value,
                )?;
            }
            for arm in arms {
                push_edge(&mut inputs, OwnerConstraintEdgeRole::PipeArm, *arm)?;
            }
            OwnerConstraintNodeKind::Pipe {
                operation: op.clone(),
            }
        }
        AstExprKind::Draining { input } => {
            push_edge(&mut inputs, OwnerConstraintEdgeRole::DrainingInput, *input)?;
            OwnerConstraintNodeKind::Draining
        }
        AstExprKind::Hold { initial, name } => {
            push_edge(&mut inputs, OwnerConstraintEdgeRole::HoldInitial, *initial)?;
            OwnerConstraintNodeKind::Hold { name: name.clone() }
        }
        AstExprKind::Latest { branches } => {
            for branch in branches {
                push_edge(&mut inputs, OwnerConstraintEdgeRole::LatestBranch, *branch)?;
            }
            OwnerConstraintNodeKind::Latest
        }
        AstExprKind::When { input, arms } => {
            push_edge(&mut inputs, OwnerConstraintEdgeRole::WhenInput, *input)?;
            for arm in arms {
                push_edge(&mut inputs, OwnerConstraintEdgeRole::WhenArm, *arm)?;
            }
            OwnerConstraintNodeKind::When
        }
        AstExprKind::Then { input, output } => {
            push_edge(&mut inputs, OwnerConstraintEdgeRole::ThenInput, *input)?;
            if let Some(output) = output {
                push_edge(&mut inputs, OwnerConstraintEdgeRole::ThenOutput, *output)?;
            }
            OwnerConstraintNodeKind::Then
        }
        AstExprKind::Infix { left, op, right } => {
            push_edge(&mut inputs, OwnerConstraintEdgeRole::InfixLeft, *left)?;
            push_edge(&mut inputs, OwnerConstraintEdgeRole::InfixRight, *right)?;
            OwnerConstraintNodeKind::Infix {
                operation: op.clone(),
            }
        }
        AstExprKind::MatchArm {
            pattern: value,
            output,
        } => {
            if let Some(output) = output {
                push_edge(&mut inputs, OwnerConstraintEdgeRole::MatchOutput, *output)?;
            }
            OwnerConstraintNodeKind::MatchArm {
                pattern: pattern(value),
            }
        }
        AstExprKind::Block { bindings, result } => {
            for binding in bindings {
                push_edge(
                    &mut inputs,
                    OwnerConstraintEdgeRole::BlockBinding {
                        name: binding.name.clone(),
                    },
                    binding.value,
                )?;
            }
            if let Some(result) = result {
                push_edge(&mut inputs, OwnerConstraintEdgeRole::BlockResult, *result)?;
            }
            OwnerConstraintNodeKind::Block
        }
        AstExprKind::ListLiteral { capacity, items } => {
            for item in items {
                push_edge(&mut inputs, OwnerConstraintEdgeRole::CollectionItem, *item)?;
            }
            OwnerConstraintNodeKind::Collection {
                collection: OwnerCollectionKind::List,
                fixed_size_or_capacity: capacity
                    .map(|capacity| checked_u32(capacity, "LIST capacity"))
                    .transpose()?,
            }
        }
        AstExprKind::BytesLiteral { size, items } => {
            for item in items {
                push_edge(&mut inputs, OwnerConstraintEdgeRole::CollectionItem, *item)?;
            }
            OwnerConstraintNodeKind::Collection {
                collection: OwnerCollectionKind::Bytes,
                fixed_size_or_capacity: bytes_size(size)?,
            }
        }
        AstExprKind::SetLiteral { items } => {
            for item in items {
                push_edge(&mut inputs, OwnerConstraintEdgeRole::CollectionItem, *item)?;
            }
            OwnerConstraintNodeKind::Collection {
                collection: OwnerCollectionKind::Set,
                fixed_size_or_capacity: None,
            }
        }
        AstExprKind::Arrow {
            left,
            pattern: value,
            output,
        } => {
            push_edge(&mut inputs, OwnerConstraintEdgeRole::ArrowLeft, *left)?;
            if let Some(output) = output {
                push_edge(&mut inputs, OwnerConstraintEdgeRole::ArrowOutput, *output)?;
            }
            OwnerConstraintNodeKind::Arrow {
                pattern: pattern(value),
            }
        }
        AstExprKind::MapEntry { key, value } => {
            push_edge(&mut inputs, OwnerConstraintEdgeRole::MapKey, *key)?;
            push_edge(&mut inputs, OwnerConstraintEdgeRole::MapValue, *value)?;
            OwnerConstraintNodeKind::MapEntry
        }
        AstExprKind::MapLiteral { entries } => {
            for entry in entries {
                push_edge(&mut inputs, OwnerConstraintEdgeRole::MapEntry, *entry)?;
            }
            OwnerConstraintNodeKind::Collection {
                collection: OwnerCollectionKind::Map,
                fixed_size_or_capacity: None,
            }
        }
        AstExprKind::Delimiter => OwnerConstraintNodeKind::Delimiter,
        AstExprKind::Unknown(tokens) => OwnerConstraintNodeKind::Unknown {
            tokens: tokens.clone().into_boxed_slice(),
        },
    };
    Ok(OwnerExpressionConstraint {
        expression: expression.stable_key.clone(),
        kind,
        inputs: inputs.into_boxed_slice(),
    })
}

fn static_bits_width_argument(function: &str) -> Option<&'static str> {
    match function {
        "Bits/slice" => Some("count"),
        "Bits/zero_extend" | "Bits/sign_extend" | "Bits/truncate" | "Number/to_bits"
        | "Bytes/to_bits" => Some("width"),
        _ => None,
    }
}

fn collect_static_number_leaves(
    reference: u32,
    input: &OwnerSyntaxInput,
    expressions: &[OwnerExpressionConstraint],
    numbers: &mut BTreeMap<StableExpressionKey, String>,
    active: &mut BTreeSet<u32>,
) {
    let Some(expression) = expressions.get(reference as usize) else {
        return;
    };
    if !active.insert(reference) {
        return;
    }
    if let Some(OwnerExpressionInput {
        kind: AstExprKind::Number(literal),
        ..
    }) = input.expressions.get(reference as usize)
    {
        numbers.insert(expression.expression.clone(), literal.clone());
    }
    for child in &expression.inputs {
        collect_static_number_leaves(child.expression, input, expressions, numbers, active);
    }
    active.remove(&reference);
}

fn project_result_static_numbers(
    input: &OwnerSyntaxInput,
    declarations: &[OwnerDeclarationConstraint],
    statement_values: &[(u32, u32)],
    expressions: &[OwnerExpressionConstraint],
) -> Box<[OwnerResultStaticNumber]> {
    let Some(public) = declarations.iter().find(|declaration| {
        declaration.public && declaration.kind == OwnerDeclarationKind::Function
    }) else {
        return Box::new([]);
    };
    let Some(root) = statement_values
        .iter()
        .find(|(statement, _)| *statement == public.statement)
        .or_else(|| statement_values.last())
        .map(|(_, expression)| *expression)
    else {
        return Box::new([]);
    };
    let mut reachable = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(reference) = pending.pop() {
        let Some(expression) = expressions.get(reference as usize) else {
            continue;
        };
        if !reachable.insert(reference) {
            continue;
        }
        pending.extend(expression.inputs.iter().map(|input| input.expression));
    }

    let mut numbers = BTreeMap::new();
    for reference in reachable {
        let expression = &expressions[reference as usize];
        let function = match &expression.kind {
            OwnerConstraintNodeKind::Call { function }
            | OwnerConstraintNodeKind::Pipe {
                operation: function,
            } => function,
            _ => continue,
        };
        let Some(width_name) = static_bits_width_argument(function) else {
            continue;
        };
        for width in expression.inputs.iter().filter(|input| {
            matches!(
                &input.role,
                OwnerConstraintEdgeRole::CallArgument {
                    kind: OwnerArgumentKind::Named,
                    name,
                    ..
                } | OwnerConstraintEdgeRole::PipeArgument {
                    kind: OwnerArgumentKind::Named,
                    name,
                    ..
                } if name == width_name
            )
        }) {
            collect_static_number_leaves(
                width.expression,
                input,
                expressions,
                &mut numbers,
                &mut BTreeSet::new(),
            );
        }
    }
    numbers
        .into_iter()
        .map(|(expression, literal)| OwnerResultStaticNumber {
            expression,
            literal,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn flush_reference_index(input: &OwnerSyntaxInput, reference: &OwnerExpressionRef) -> Option<u32> {
    match reference {
        OwnerExpressionRef::Local { expression } => Some(expression.0),
        OwnerExpressionRef::Child { owner, expression } => {
            let external = input.external_expressions.iter().position(|candidate| {
                &candidate.owner == owner && &candidate.expression == expression
            })?;
            u32::try_from(input.expressions.len().checked_add(external)?).ok()
        }
    }
}

fn flush_index_reference(input: &OwnerSyntaxInput, reference: usize) -> Option<OwnerExpressionRef> {
    if reference < input.expressions.len() {
        return Some(OwnerExpressionRef::Local {
            expression: OwnerExpressionId(u32::try_from(reference).ok()?),
        });
    }
    let external = input.external_expression(reference)?;
    Some(OwnerExpressionRef::Child {
        owner: external.owner.clone(),
        expression: external.expression.clone(),
    })
}

fn child_owner_flush_values(
    input: &OwnerSyntaxInput,
    owner: &StableCheckOwnerKey,
) -> Vec<OwnerExpressionRef> {
    input
        .external_expressions
        .iter()
        .filter(|external| &external.owner == owner)
        .map(|external| OwnerExpressionRef::Child {
            owner: external.owner.clone(),
            expression: external.expression.clone(),
        })
        .collect()
}

fn flush_child_update_values(
    input: &OwnerSyntaxInput,
    graph: &crate::OwnerSyntaxGraph,
    child: &OwnerStatementChild,
) -> Vec<OwnerExpressionRef> {
    match child {
        OwnerStatementChild::Local { statement } => {
            flush_statement_update_values(input, graph, *statement)
        }
        OwnerStatementChild::Owner { owner } => child_owner_flush_values(input, owner),
    }
}

fn flush_statement_update_values(
    input: &OwnerSyntaxInput,
    graph: &crate::OwnerSyntaxGraph,
    statement: OwnerStatementId,
) -> Vec<OwnerExpressionRef> {
    let Some(statement) = graph.statement(statement) else {
        return Vec::new();
    };
    if let Some(value) = &statement.canonical_value {
        return vec![value.clone()];
    }
    statement
        .children
        .iter()
        .flat_map(|child| flush_child_update_values(input, graph, child))
        .collect()
}

fn hold_flush_update_expressions(
    input: &OwnerSyntaxInput,
    graph: &crate::OwnerSyntaxGraph,
    expression: OwnerExpressionId,
) -> Vec<OwnerExpressionRef> {
    let Some(statement) = input
        .statements
        .iter()
        .find(|statement| statement.expression == Some(expression.0))
        .map(|statement| OwnerStatementId(statement.id))
    else {
        return Vec::new();
    };
    let Some(statement) = graph.statement(statement) else {
        return Vec::new();
    };
    let mut updates = Vec::new();
    for child in &statement.children {
        let OwnerStatementChild::Local { statement: child } = child else {
            updates.extend(flush_child_update_values(input, graph, child));
            continue;
        };
        let Some(child_node) = graph.statement(*child) else {
            continue;
        };
        let continuation = input
            .statements
            .get(child.0 as usize)
            .and_then(|statement| statement.expression)
            .and_then(|expression| input.expressions.get(expression as usize))
            .is_some_and(|expression| expression.linked_input.is_some());
        let update_start = updates.len();
        let child_is_latest = child_node
            .direct_value
            .as_ref()
            .and_then(|value| match value {
                OwnerExpressionRef::Local { expression } => {
                    input.expressions.get(expression.0 as usize)
                }
                OwnerExpressionRef::Child { .. } => None,
            })
            .is_some_and(|expression| matches!(expression.kind, AstExprKind::Latest { .. }));
        if child_is_latest {
            updates.extend(
                child_node
                    .children
                    .iter()
                    .flat_map(|grandchild| flush_child_update_values(input, graph, grandchild)),
            );
        } else {
            updates.extend(flush_statement_update_values(input, graph, *child));
        }
        if continuation && update_start > 0 && updates.len() > update_start {
            updates.remove(update_start - 1);
        }
    }
    updates
}

fn project_expression_flush_plans(
    input: &OwnerSyntaxInput,
    graph: &crate::OwnerSyntaxGraph,
) -> Box<[OwnerFlushConstraint]> {
    input
        .expressions
        .iter()
        .enumerate()
        .map(|(index, expression)| {
            let mut value_inputs = Vec::new();
            let escape_references = match &expression.kind {
                AstExprKind::Flush {
                    payload: Some(payload),
                } => {
                    if let Ok(payload) = u32::try_from(*payload) {
                        value_inputs.push(payload);
                    }
                    flush_index_reference(input, *payload)
                        .into_iter()
                        .collect::<Vec<_>>()
                }
                AstExprKind::Flush { payload: None }
                | AstExprKind::Block { .. }
                | AstExprKind::Object(_)
                | AstExprKind::TaggedObject { .. } => Vec::new(),
                AstExprKind::Hold { .. } => hold_flush_update_expressions(
                    input,
                    graph,
                    OwnerExpressionId(u32::try_from(index).expect("owner expression id is u32")),
                ),
                _ => graph
                    .expression_inputs(OwnerExpressionId(
                        u32::try_from(index).expect("owner expression id is u32"),
                    ))
                    .unwrap_or_default()
                    .to_vec(),
            };
            let escape_inputs = escape_references
                .iter()
                .filter_map(|reference| flush_reference_index(input, reference))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            OwnerFlushConstraint {
                value_inputs: value_inputs.into_boxed_slice(),
                escape_inputs: escape_inputs.into_boxed_slice(),
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

pub fn project_owner_constraint_seed(
    input: &OwnerSyntaxInput,
) -> Result<OwnerConstraintSeed, OwnerConstraintSeedError> {
    let mut declarations = Vec::new();
    let public_statement = matches!(input.owner, StableCheckOwnerKey::Item(_)).then_some(0);
    let mut effect_seed = OwnerEffectConstraintSeed::default();
    for statement in &input.statements {
        if let Some(declaration) = declaration(
            statement.id,
            public_statement == Some(statement.id),
            &statement.kind,
        )? {
            match declaration.kind {
                OwnerDeclarationKind::Source => effect_seed.declares_source = true,
                OwnerDeclarationKind::Hold => effect_seed.declares_state = true,
                OwnerDeclarationKind::List => effect_seed.declares_list = true,
                OwnerDeclarationKind::Function | OwnerDeclarationKind::Field => {}
            }
            declarations.push(declaration);
        }
    }

    let syntax_graph = crate::OwnerSyntaxGraph::build(input).map_err(|error| {
        OwnerConstraintSeedError::new(format!(
            "cannot derive canonical statement values for {:?}: {error}",
            input.owner
        ))
    })?;
    let statement_values = syntax_graph
        .statements()
        .iter()
        .filter_map(|statement| {
            let expression = match statement.canonical_value.as_ref()? {
                boon_checked::OwnerExpressionRef::Local { expression } => expression.0,
                boon_checked::OwnerExpressionRef::Child { owner, expression } => {
                    let external = input.external_expressions.iter().position(|candidate| {
                        &candidate.owner == owner && &candidate.expression == expression
                    })?;
                    u32::try_from(input.expressions.len().checked_add(external)?).ok()?
                }
            };
            Some((statement.id.0, expression))
        })
        .collect::<Vec<_>>();
    let mut references = BTreeSet::new();
    let expressions = input
        .expressions
        .iter()
        .map(|expression| project_expression(expression, &mut references))
        .collect::<Result<Vec<_>, _>>()?;
    let expression_flush_plans = project_expression_flush_plans(input, &syntax_graph);
    // Preserve the compact namespace order: expression edges address external
    // values after the local-expression range. `OwnerSyntaxInput` already
    // interns each referenced syntax expression exactly once.
    let external_expressions = input.external_expressions.to_vec();
    let references = references.into_iter().collect::<Vec<_>>();
    let result_static_numbers =
        project_result_static_numbers(input, &declarations, &statement_values, &expressions);
    let topology_references = references
        .iter()
        .map(|reference| (reference.kind, reference.parts.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let external_owners = external_expressions
        .iter()
        .map(|external| external.owner.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let topology_fingerprint_v1 = fingerprint(
        OWNER_CONSTRAINT_TOPOLOGY_DOMAIN_V1,
        &(
            stable_check_owner_key_fingerprint_v1(&input.owner),
            &declarations,
            &topology_references,
            &external_owners,
        ),
    )?;
    let fingerprint_v1 = fingerprint(
        OWNER_CONSTRAINT_SEED_DOMAIN_V1,
        &(
            stable_check_owner_key_fingerprint_v1(&input.owner),
            &declarations,
            &statement_values,
            &expressions,
            &expression_flush_plans,
            &references,
            &external_expressions,
            &result_static_numbers,
            effect_seed,
        ),
    )?;
    Ok(OwnerConstraintSeed {
        owner: input.owner.clone(),
        declarations: declarations.into_boxed_slice(),
        statement_values: statement_values.into_boxed_slice(),
        expressions: expressions.into_boxed_slice(),
        expression_flush_plans,
        references: references.into_boxed_slice(),
        external_expressions: external_expressions.into_boxed_slice(),
        result_static_numbers,
        effect_seed,
        fingerprint_v1,
        topology_fingerprint_v1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_owner_syntax_input;
    use boon_parser::{UnitSyntaxSnapshot, parse_project_source_unit, project_unit_link_keys};

    fn link(source: &str) -> UnitSyntaxSnapshot {
        let parsed = parse_project_source_unit("app/RUN.bn", source).unwrap();
        let key = project_unit_link_keys(
            "app/RUN.bn",
            [(
                parsed.source_unit_id.clone(),
                parsed.declared_functions.clone(),
            )],
        )
        .unwrap()
        .remove(&parsed.source_unit_id)
        .unwrap();
        parsed.into_unit_syntax_snapshot(key).unwrap()
    }

    fn owner_named(unit: &UnitSyntaxSnapshot, name: &str) -> StableCheckOwnerKey {
        unit.stable_check_owner_keys()
            .find(|owner| {
                matches!(owner, StableCheckOwnerKey::Item(owner) if owner.item_route.segments().last().is_some_and(|segment| segment.names == [name]))
            })
            .unwrap()
    }

    fn summary(unit: &UnitSyntaxSnapshot, owner: &StableCheckOwnerKey) -> OwnerConstraintSeed {
        let syntax = project_owner_syntax_input(unit.owner_view_for_key(owner).unwrap()).unwrap();
        project_owner_constraint_seed(&syntax).unwrap()
    }

    #[test]
    fn literal_payload_edits_backdate_interface_constraints() {
        let before = link("value: TEXT { before }\n");
        let after = link("value: TEXT { after with more bytes }\n");
        let before_owner = owner_named(&before, "value");
        let after_owner = owner_named(&after, "value");
        assert_eq!(before_owner, after_owner);
        assert_eq!(
            summary(&before, &before_owner),
            summary(&after, &after_owner)
        );
    }

    #[test]
    fn only_result_relevant_static_bits_numbers_enter_the_interface_seed() {
        let width_four = link(concat!(
            "FUNCTION take(bits) {\n",
            "    ignored: 11\n",
            "    bits |> Bits/slice(from: 1, count: 2 + 2)\n",
            "}\n",
        ));
        let width_five = link(concat!(
            "FUNCTION take(bits) {\n",
            "    ignored: 99\n",
            "    bits |> Bits/slice(from: 7, count: 2 + 3)\n",
            "}\n",
        ));
        let owner = owner_named(&width_four, "take");
        let changed_owner = owner_named(&width_five, "take");
        assert_eq!(owner, changed_owner);
        let width_four = summary(&width_four, &owner);
        let width_five = summary(&width_five, &changed_owner);

        let mut four_literals = width_four
            .result_static_numbers
            .iter()
            .map(|number| number.literal.as_str())
            .collect::<Vec<_>>();
        four_literals.sort_unstable();
        let mut five_literals = width_five
            .result_static_numbers
            .iter()
            .map(|number| number.literal.as_str())
            .collect::<Vec<_>>();
        five_literals.sort_unstable();
        assert_eq!(four_literals, ["2", "2"]);
        assert_eq!(five_literals, ["2", "3"]);
        assert_ne!(width_four.fingerprint_v1(), width_five.fingerprint_v1());
    }

    #[test]
    fn dependency_reference_edits_change_topology_fingerprint() {
        let before = link("left: 1\nright: left\n");
        let after = link("left: 1\nright: missing\n");
        let before_owner = owner_named(&before, "right");
        let after_owner = owner_named(&after, "right");
        let before = summary(&before, &before_owner);
        let after = summary(&after, &after_owner);
        assert_ne!(
            before.topology_fingerprint_v1(),
            after.topology_fingerprint_v1()
        );
        assert_eq!(before.references[0].parts.as_ref(), ["left"]);
        assert_eq!(after.references[0].parts.as_ref(), ["missing"]);
    }

    #[test]
    fn body_constraint_changes_preserve_dependency_topology() {
        let before = link("FUNCTION identity(input) {\n    input\n}\n");
        let after = link("FUNCTION identity(input) {\n    input + 0\n}\n");
        let before_owner = owner_named(&before, "identity");
        let after_owner = owner_named(&after, "identity");
        let before = summary(&before, &before_owner);
        let after = summary(&after, &after_owner);
        assert_ne!(before.fingerprint_v1(), after.fingerprint_v1());
        assert_eq!(
            before.topology_fingerprint_v1(),
            after.topology_fingerprint_v1()
        );
    }

    #[test]
    fn resolved_calls_publish_both_interface_flow_directions() {
        let unit = link(
            "FUNCTION remap(input, item: OUT) {\n    item: input\n}\ncaller: remap(\n    input: 1\n    item\n    PASS: [theme: theme]\n)\n",
        );
        let callable = owner_named(&unit, "remap");
        let caller = owner_named(&unit, "caller");
        let callable_seed = summary(&unit, &callable);
        let caller_seed = summary(&unit, &caller);
        let reference = caller_seed
            .references
            .iter()
            .find(|reference| reference.kind == OwnerReferenceKind::Callable)
            .cloned()
            .unwrap();
        let parameters = callable_seed
            .declarations
            .iter()
            .find(|declaration| declaration.public)
            .unwrap()
            .parameters
            .clone();
        let resolved = resolve_owner_constraint_seed(
            &caller_seed,
            [ResolvedOwnerSymbolReference {
                reference,
                owner: callable.clone(),
                projection: Box::new([]),
                parameters,
            }],
        )
        .unwrap();

        let edges = resolved
            .dependencies
            .iter()
            .map(|edge| {
                (
                    edge.request.clone(),
                    edge.dependency.clone(),
                    edge.kind,
                    edge.parameter_ordinal,
                )
            })
            .collect::<BTreeSet<_>>();
        assert!(edges.contains(&(
            caller.clone(),
            callable.clone(),
            OwnerConstraintDependencyKind::CallResult,
            None,
        )));
        assert!(edges.contains(&(
            caller.clone(),
            callable.clone(),
            OwnerConstraintDependencyKind::CallEffect,
            None,
        )));
        assert!(edges.contains(&(
            callable.clone(),
            caller.clone(),
            OwnerConstraintDependencyKind::ActualToFormal,
            Some(0),
        )));
        assert!(edges.contains(&(
            caller.clone(),
            callable.clone(),
            OwnerConstraintDependencyKind::FreshOutFromFormal,
            Some(1),
        )));
        assert!(edges.contains(&(
            callable,
            caller,
            OwnerConstraintDependencyKind::PassedContext,
            None,
        )));

        let callable_summary = resolve_owner_constraint_seed(&callable_seed, []).unwrap();
        let topology = build_owner_interface_topology([&callable_summary, &resolved]).unwrap();
        assert_eq!(topology.stats.nodes, 2);
        assert_eq!(topology.stats.components, 1);
        assert_eq!(topology.stats.cyclic_components, 1);
        assert_eq!(topology.sccs.len(), 1);
        assert_eq!(topology.sccs[0].key.members.len(), 2);
        assert!(topology.sccs[0].dependencies.is_empty());
    }

    #[test]
    fn exact_symbol_resolution_preserves_ambiguity_and_rejects_missing_states() {
        let unit = link("left: 1\nright: 2\nvalue: mystery()\n");
        let value = owner_named(&unit, "value");
        let left = owner_named(&unit, "left");
        let right = owner_named(&unit, "right");
        let seed = summary(&unit, &value);
        let reference = seed.references.first().cloned().unwrap();

        assert!(resolve_owner_constraint_seed_with_resolutions(&seed, []).is_err());
        let resolved = resolve_owner_constraint_seed_with_resolutions(
            &seed,
            [OwnerSymbolResolution::Ambiguous {
                reference: reference.clone(),
                candidates: vec![
                    AmbiguousOwnerSymbolCandidate {
                        owner: left,
                        parameters: Box::new([]),
                    },
                    AmbiguousOwnerSymbolCandidate {
                        owner: right,
                        parameters: Box::new([]),
                    },
                ]
                .into_boxed_slice(),
            }],
        )
        .unwrap();

        assert!(resolved.resolved_references.is_empty());
        assert!(resolved.dependencies.is_empty());
        assert!(matches!(
            resolved.symbol_resolutions.as_ref(),
            [OwnerSymbolResolution::Ambiguous {
                reference: retained,
                candidates,
            }] if retained == &reference && candidates.len() == 2
        ));
    }

    #[test]
    fn compatibility_resolution_marks_default_builtins_authoritative() {
        let unit = link("value: Number/to_text(value: 1)\n");
        let value = owner_named(&unit, "value");
        let seed = summary(&unit, &value);
        let resolved = resolve_owner_constraint_seed(&seed, []).unwrap();
        assert!(matches!(
            resolved.symbol_resolutions.as_ref(),
            [OwnerSymbolResolution::Authoritative { .. }]
        ));
    }
}
