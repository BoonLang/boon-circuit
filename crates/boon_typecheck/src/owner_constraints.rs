use crate::{
    OwnerContainingScopeInput, OwnerExpressionInput, OwnerExternalExpressionInput,
    OwnerSyntaxInput, stable_check_owner_key_fingerprint_v1,
};
use boon_checked::{
    OwnerDeclarationStableKey, OwnerExpressionId, OwnerExpressionRef, OwnerExpressionScopeRole,
    OwnerLexicalDeclarationCapability, OwnerLexicalTargetRef, OwnerScopeStableKey,
    OwnerStableScopeRef, OwnerStatementChild, OwnerStatementId, OwnerStatementScopeRole,
};
use boon_compilation_db::{
    DenseProjectionGraphBuilder, ProjectionGraphDigestDomains, ProjectionGraphStats, ProjectionId,
};
use boon_syntax::{
    AstBlockBindingDeclaration, AstCallArgKind, AstDrainPath, AstExprKind, AstMatchPattern,
    AstStatementKind, AstTextSegment, BytesSizeSyntax, StableCheckOwnerKey, StableExpressionKey,
    StableStatementKey, StableStatementKind, UnitItemKind,
};
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::sync::Arc;

const OWNER_CONSTRAINT_SEED_DOMAIN_V4: &[u8] = b"boon.owner-constraint-seed.v4\0";
const OWNER_CONSTRAINT_TOPOLOGY_DOMAIN_V2: &[u8] = b"boon.owner-constraint-topology.v2\0";
const OWNER_DECLARATION_SURFACE_DOMAIN_V1: &[u8] = b"boon.owner-declaration-surface.v1\0";
const OWNER_LEXICAL_PLAN_DOMAIN_V3: &[u8] = b"boon.owner-lexical-plan.v3\0";
const OWNER_LEXICAL_READS_DOMAIN_V3: &[u8] = b"boon.owner-lexical-reads.v3\0";
const OWNER_LEXICAL_BOUNDARY_BINDINGS_DOMAIN_V1: &[u8] =
    b"boon.owner-lexical-boundary-bindings.v1\0";
const OWNER_LEXICAL_CONTAINMENT_DOMAIN_V2: &[u8] = b"boon.owner-lexical-containment.v2\0";
const OWNER_SIGNATURE_REGION_INDEX_DOMAIN_V2: &[u8] = b"boon.owner-signature-region-index.v2\0";
const OWNER_RESOLVED_CONSTRAINT_SUMMARY_DOMAIN_V2: &[u8] =
    b"boon.owner-resolved-constraint-summary.v2\0";
const OWNER_RESOLVED_CONSTRAINT_TOPOLOGY_DOMAIN_V2: &[u8] =
    b"boon.owner-resolved-constraint-topology.v2\0";
const OWNER_INTERFACE_COMPONENT_DOMAIN_V2: &[u8] = b"boon.owner-interface-component.v2\0";
const OWNER_INTERFACE_SCC_DOMAIN_V2: &[u8] = b"boon.owner-interface-scc.v2\0";
const OWNER_INTERFACE_TOPOLOGY_DOMAIN_V2: &[u8] = b"boon.owner-interface-topology.v2\0";

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

/// Body-independent declaration data needed by project symbol publication.
///
/// Keeping this projection separate from [`OwnerConstraintSeed`] prevents the
/// project symbol layer from depending on every expression in every owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerDeclarationSurface {
    owner: StableCheckOwnerKey,
    public: Option<OwnerDeclarationConstraint>,
    fingerprint_v1: [u8; 32],
}

impl OwnerDeclarationSurface {
    pub const fn owner(&self) -> &StableCheckOwnerKey {
        &self.owner
    }

    pub const fn public(&self) -> Option<&OwnerDeclarationConstraint> {
        self.public.as_ref()
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerLexicalAccess {
    Read,
    Drain,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerLexicalDeclarationTarget {
    Parameter {
        ordinal: u32,
    },
    Statement {
        statement: u32,
    },
    RecordField {
        object: u32,
        ordinal: u32,
        name: String,
    },
    PatternBinding {
        arm: u32,
        name: String,
    },
    Imported {
        target: OwnerLexicalTargetRef,
    },
    Passed,
    Ambiguous {
        name: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerLexicalReadPlan {
    pub target: OwnerLexicalDeclarationTarget,
    /// Exact authored scope that declares `target`.
    ///
    /// Signature-backed dynamic regions use this to distinguish a nested
    /// authored shadow (which wins) from a declaration at or above the call
    /// boundary (which the dynamic binding shadows). `PASSED` has no authored
    /// declaration scope.
    pub declaration_scope: Option<u32>,
    pub projection: Box<[String]>,
    pub access: OwnerLexicalAccess,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerLexicalScopePlan {
    pub parent: Option<u32>,
    pub origin: OwnerLexicalScopeOrigin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OwnerLexicalScopeOrigin {
    Root,
    StatementBody { statement: u32 },
    PatternArm { expression: u32 },
    Record { expression: u32 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerLexicalRecordFieldPlan {
    pub object: u32,
    pub ordinal: u32,
    pub name: String,
    pub scope: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerLexicalBoundaryBindingPlan {
    pub name: String,
    pub target: OwnerLexicalTargetRef,
    pub declaration_scope: Option<OwnerStableScopeRef>,
}

/// Immutable visible-binding set shared by child boundaries that occupy the
/// same authored scope. The content digest lets enclosing artifacts commit the
/// set once instead of serializing a wide record's identical environment for
/// every child boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerLexicalBoundaryBindings {
    rows: Arc<[OwnerLexicalBoundaryBindingPlan]>,
    fingerprint_v1: [u8; 32],
}

impl OwnerLexicalBoundaryBindings {
    pub(crate) fn try_new(
        bindings: Vec<OwnerLexicalBoundaryBindingPlan>,
    ) -> Result<Self, boon_contract::CanonicalEncodingError> {
        let fingerprint_v1 = boon_contract::canonical_serde_hash_v1(
            OWNER_LEXICAL_BOUNDARY_BINDINGS_DOMAIN_V1,
            &bindings,
        )?;
        Ok(Self {
            rows: Arc::from(bindings),
            fingerprint_v1,
        })
    }

    pub fn iter(&self) -> std::slice::Iter<'_, OwnerLexicalBoundaryBindingPlan> {
        self.rows.iter()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn get(&self, name: &str) -> Option<&OwnerLexicalBoundaryBindingPlan> {
        self.rows
            .binary_search_by(|binding| binding.name.as_str().cmp(name))
            .ok()
            .and_then(|index| self.rows.get(index))
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    #[cfg(test)]
    pub(crate) fn ptr_eq(left: &Self, right: &Self) -> bool {
        Arc::ptr_eq(&left.rows, &right.rows)
    }
}

impl Default for OwnerLexicalBoundaryBindings {
    fn default() -> Self {
        Self::try_new(Vec::new()).expect("empty lexical bindings are canonically serializable")
    }
}

impl Serialize for OwnerLexicalBoundaryBindings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.rows.as_ref().serialize(serializer)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerLexicalChildBoundaryPlan {
    pub owner: StableCheckOwnerKey,
    pub parent_statement: Option<StableStatementKey>,
    pub child_index: u32,
    pub boundary_expression: Option<StableExpressionKey>,
    pub result_expression: Option<StableExpressionKey>,
    pub result_placement: crate::OwnerChildResultPlacementInput,
    pub scope: Option<OwnerStableScopeRef>,
    pub bindings: OwnerLexicalBoundaryBindings,
}

impl OwnerLexicalChildBoundaryPlan {
    pub const fn inherits_lexical_environment(&self) -> bool {
        self.parent_statement.is_some() && self.result_expression.is_some()
    }
}

/// Stable containment and authored lexical environment for one syntax owner.
///
/// Top-level siblings remain independent. A child inherits only across an
/// ordinary value boundary; a nested FUNCTION has no result expression and
/// deliberately resets lexical inheritance until closure semantics are
/// specified.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OwnerLexicalContainmentPlan {
    pub owner: StableCheckOwnerKey,
    pub containing_scope: OwnerContainingScopeInput,
    pub children: Box<[OwnerLexicalChildBoundaryPlan]>,
    fingerprint_v1: [u8; 32],
}

impl OwnerLexicalContainmentPlan {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

/// Compact structural authority needed by signature-backed lexical planning.
///
/// This index is Arc-shared by the base lexical plan and constraint seed. It
/// carries no syntax payload, types, symbol resolutions, or checked rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerSignatureRegionIndex {
    scopes: Arc<[OwnerLexicalScopePlan]>,
    stable_scopes: Arc<[Option<OwnerStableScopeRef>]>,
    expression_scopes: Arc<[u32]>,
    stable_targets: BTreeMap<OwnerLexicalDeclarationTarget, OwnerLexicalTargetRef>,
    containment: Arc<OwnerLexicalContainmentPlan>,
    fingerprint_v1: [u8; 32],
}

impl OwnerSignatureRegionIndex {
    pub fn scopes(&self) -> &[OwnerLexicalScopePlan] {
        &self.scopes
    }

    pub fn stable_scopes(&self) -> &[Option<OwnerStableScopeRef>] {
        &self.stable_scopes
    }

    pub fn expression_scopes(&self) -> &[u32] {
        &self.expression_scopes
    }

    pub fn stable_target(
        &self,
        target: &OwnerLexicalDeclarationTarget,
    ) -> Option<&OwnerLexicalTargetRef> {
        self.stable_targets.get(target)
    }

    pub(crate) fn stable_targets(
        &self,
    ) -> &BTreeMap<OwnerLexicalDeclarationTarget, OwnerLexicalTargetRef> {
        &self.stable_targets
    }

    pub fn containment(&self) -> &Arc<OwnerLexicalContainmentPlan> {
        &self.containment
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }
}

/// Syntax-owned lexical authority for one owner.
///
/// This base plan predeclares authored declarations for their complete scope
/// and publishes one shared projection for parameters, BLOCK/record
/// declarations, pattern bindings, PASSED reads, and external candidates.
/// Fresh OUT
/// and call-context declarations are intentionally a later signature-backed
/// overlay; this pass never guesses them from spelling alone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerLexicalPlan {
    owner: StableCheckOwnerKey,
    syntax_fingerprint_v1: [u8; 32],
    graph: crate::OwnerSyntaxGraph,
    statement_values: Box<[(u32, u32)]>,
    signature_regions: Arc<OwnerSignatureRegionIndex>,
    reads: Arc<[Option<OwnerLexicalReadPlan>]>,
    record_scopes: Box<[(u32, u32)]>,
    record_fields: Box<[OwnerLexicalRecordFieldPlan]>,
    external_candidates: Box<[OwnerSymbolReference]>,
    reads_fingerprint_v1: [u8; 32],
    fingerprint_v1: [u8; 32],
}

impl OwnerLexicalPlan {
    pub const fn owner(&self) -> &StableCheckOwnerKey {
        &self.owner
    }

    pub fn statement_values(&self) -> &[(u32, u32)] {
        &self.statement_values
    }

    pub fn scopes(&self) -> &[OwnerLexicalScopePlan] {
        self.signature_regions.scopes()
    }

    pub fn expression_scopes(&self) -> &[u32] {
        self.signature_regions.expression_scopes()
    }

    pub fn signature_regions(&self) -> &Arc<OwnerSignatureRegionIndex> {
        &self.signature_regions
    }

    pub fn reads(&self) -> &[Option<OwnerLexicalReadPlan>] {
        &self.reads
    }

    pub fn record_scopes(&self) -> &[(u32, u32)] {
        &self.record_scopes
    }

    pub fn record_fields(&self) -> &[OwnerLexicalRecordFieldPlan] {
        &self.record_fields
    }

    pub fn external_candidates(&self) -> &[OwnerSymbolReference] {
        &self.external_candidates
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub const fn reads_fingerprint_v1(&self) -> [u8; 32] {
        self.reads_fingerprint_v1
    }

    pub(crate) fn graph(&self) -> &crate::OwnerSyntaxGraph {
        &self.graph
    }

    pub(crate) fn matches_input(&self, input: &OwnerSyntaxInput) -> bool {
        self.owner == input.owner && self.syntax_fingerprint_v1 == input.fingerprint_v1()
    }
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
    MatchSelector,
    MatchBinding {
        name: String,
    },
    /// A read of the exact selector inside one pattern arm. The projection is
    /// relative to that selector and is inferred against an arm-local payload,
    /// not against the callable's public parameter contract.
    MatchNarrowedSelector {
        projection: Box<[String]>,
    },
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

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OwnerSourcePayloadQuery {
    pub expression: StableExpressionKey,
    pub canonical_path: String,
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
    lexical_reads_fingerprint_v1: [u8; 32],
    lexical_reads: Arc<[Option<OwnerLexicalReadPlan>]>,
    signature_regions: Arc<OwnerSignatureRegionIndex>,
    pub declarations: Box<[OwnerDeclarationConstraint]>,
    pub statement_values: Box<[(u32, u32)]>,
    pub expressions: Box<[OwnerExpressionConstraint]>,
    /// Per-expression FLUSH-control graph. Declaration/callable boundaries
    /// union the parallel escape channel into the ordinary public flow.
    pub expression_flush_plans: Box<[OwnerFlushConstraint]>,
    pub references: Box<[OwnerSymbolReference]>,
    pub external_expressions: Box<[OwnerExternalExpressionInput]>,
    /// Stable source declarations whose payload contracts affect this owner.
    /// The path is derived from the body-insensitive owner route plus record
    /// projections, never from dense statement or expression ids.
    pub source_payload_queries: Box<[OwnerSourcePayloadQuery]>,
    pub result_static_numbers: Box<[OwnerResultStaticNumber]>,
    pub effect_seed: OwnerEffectConstraintSeed,
    fingerprint_v1: [u8; 32],
    topology_fingerprint_v1: [u8; 32],
}

impl OwnerConstraintSeed {
    pub const fn lexical_reads_fingerprint_v1(&self) -> [u8; 32] {
        self.lexical_reads_fingerprint_v1
    }

    pub fn lexical_reads(&self) -> &[Option<OwnerLexicalReadPlan>] {
        &self.lexical_reads
    }

    pub fn signature_regions(&self) -> &Arc<OwnerSignatureRegionIndex> {
        &self.signature_regions
    }

    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub const fn topology_fingerprint_v1(&self) -> [u8; 32] {
        self.topology_fingerprint_v1
    }

    /// Canonical authoritative callable names queried by this owner.
    ///
    /// The explicit sorted set is the request key surface for exact ABI lookup
    /// dependencies; missing names remain queries rather than disappearing.
    pub fn callable_abi_names(&self) -> Box<[String]> {
        self.references
            .iter()
            .filter(|reference| reference.kind == OwnerReferenceKind::Callable)
            .map(|reference| reference.parts.join("/"))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    pub fn source_payload_abi_paths(&self) -> Box<[String]> {
        self.source_payload_queries
            .iter()
            .map(|query| query.canonical_path.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    pub fn parameter_requirement_keys(&self) -> Box<[crate::OwnerParameterRequirementKey]> {
        self.declarations
            .iter()
            .find(|declaration| {
                declaration.public && declaration.kind == OwnerDeclarationKind::Function
            })
            .into_iter()
            .flat_map(|declaration| &declaration.parameters)
            .map(|parameter| {
                crate::OwnerParameterRequirementKey::new(self.owner.clone(), parameter.ordinal)
            })
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    pub fn parameter_requirement_names(&self, ordinal: u32) -> Option<(&str, &str)> {
        let declaration = self.declarations.iter().find(|declaration| {
            declaration.public && declaration.kind == OwnerDeclarationKind::Function
        })?;
        let function = declaration.names.first()?;
        let parameter = declaration
            .parameters
            .iter()
            .find(|parameter| parameter.ordinal == ordinal)?;
        Some((function, &parameter.name))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerConstraintDependencyKind {
    ChildValue,
    /// A readable public declaration imported from an already-frozen owner
    /// interface. Unlike private lexical captures, this ordinary one-way
    /// dependency does not require the provider and consumer to share an SCC.
    PublicLexicalCapture,
    LexicalCapture,
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
    /// The value namespace had no declaration, but the exact same spelling
    /// names a callable. Boon does not expose callables as first-class values,
    /// so consumers must diagnose and lower this reference fail-closed instead
    /// of treating it as an unresolved external value.
    CallableAsValue {
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
            | Self::CallableAsValue { reference }
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
            Self::Authoritative { .. }
            | Self::Unresolved { .. }
            | Self::CallableAsValue { .. }
            | Self::Ambiguous { .. } => None,
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
    pub signature_lexical_fingerprint_v1: Option<[u8; 32]>,
    pub lexical_captures: Box<[OwnerLexicalTargetRef]>,
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

    pub fn matches_signature_plan(&self, plan: &crate::OwnerSignatureLexicalPlan) -> bool {
        self.owner == *plan.owner()
            && self.signature_lexical_fingerprint_v1 == Some(plan.fingerprint_v1())
            && self.lexical_captures.as_ref() == plan.imported_captures()
    }

    pub fn symbol_resolution_for_parts(
        &self,
        expression: &StableExpressionKey,
        kind: OwnerReferenceKind,
        parts: &[String],
    ) -> Option<&OwnerSymbolResolution> {
        self.symbol_resolutions
            .binary_search_by(|resolution| {
                let reference = resolution.reference();
                reference
                    .expression
                    .cmp(expression)
                    .then_with(|| reference.kind.cmp(&kind))
                    .then_with(|| reference.parts.as_ref().cmp(parts))
            })
            .ok()
            .and_then(|index| self.symbol_resolutions.get(index))
    }

    pub fn resolved_reference_for_parts(
        &self,
        expression: &StableExpressionKey,
        kind: OwnerReferenceKind,
        parts: &[String],
    ) -> Option<&ResolvedOwnerSymbolReference> {
        self.resolved_references
            .binary_search_by(|resolved| {
                resolved
                    .reference
                    .expression
                    .cmp(expression)
                    .then_with(|| resolved.reference.kind.cmp(&kind))
                    .then_with(|| resolved.reference.parts.as_ref().cmp(parts))
            })
            .ok()
            .and_then(|index| self.resolved_references.get(index))
    }

    pub fn matches_effective_references(&self, references: &[OwnerSymbolReference]) -> bool {
        self.symbol_resolutions.len() == references.len()
            && self
                .symbol_resolutions
                .iter()
                .map(OwnerSymbolResolution::reference)
                .eq(references)
    }

    /// Exact authoritative ABI lookup names retained by symbol resolution.
    /// Project-resolved and project-ambiguous callables deliberately do not
    /// enter this surface; unresolved names remain explicit missing lookups.
    pub fn authoritative_abi_names(&self) -> Box<[String]> {
        self.symbol_resolutions
            .iter()
            .filter(|resolution| {
                resolution.reference().kind == OwnerReferenceKind::Callable
                    && matches!(
                        resolution,
                        OwnerSymbolResolution::Authoritative { .. }
                            | OwnerSymbolResolution::Unresolved { .. }
                    )
            })
            .map(|resolution| resolution.reference().parts.join("/"))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    /// Exact role-qualified external value paths consumed by this owner.
    /// Missing and forbidden values remain explicit ABI lookups so later
    /// policy or provider changes cannot leave a cached owner falsely green.
    pub fn authoritative_value_abi_paths(&self) -> Box<[String]> {
        self.symbol_resolutions
            .iter()
            .filter(|resolution| {
                resolution.reference().kind == OwnerReferenceKind::Value
                    && matches!(resolution, OwnerSymbolResolution::Authoritative { .. })
            })
            .map(|resolution| boon_syntax::canonical_value_path(&resolution.reference().parts))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .into_boxed_slice()
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
    scc_by_owner: BTreeMap<StableCheckOwnerKey, usize>,
    fingerprint_v1: [u8; 32],
}

impl OwnerInterfaceTopology {
    pub const fn fingerprint_v1(&self) -> [u8; 32] {
        self.fingerprint_v1
    }

    pub fn scc_for_owner(&self, owner: &StableCheckOwnerKey) -> Option<&OwnerInterfaceScc> {
        self.sccs.get(*self.scc_by_owner.get(owner)?)
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

    // `OwnerConstraintSummary` retains both semantic interface dependencies
    // and reverse actual/PASS currentness edges. Only dependencies consumed by
    // the summary's own public interface participate in the solve graph. The
    // reverse edges remain in the summary for invalidation and checked-body
    // currentness, but must not collapse an ordinary one-way call graph into a
    // project-wide interface SCC.
    let mut edges = summaries
        .iter()
        .flat_map(|summary| {
            summary
                .dependencies
                .iter()
                .filter(|edge| edge.request == summary.owner)
                .cloned()
        })
        .collect::<Vec<_>>();
    edges.sort();
    edges.dedup();
    let topology_edges = edges
        .iter()
        .map(OwnerInterfaceTopologyEdge::from)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut graph_edges = edges.clone();
    graph_edges.extend(
        edges
            .iter()
            .filter(|edge| edge.kind == OwnerConstraintDependencyKind::LexicalCapture)
            .map(|edge| OwnerConstraintDependency {
                request: edge.dependency.clone(),
                dependency: edge.request.clone(),
                kind: OwnerConstraintDependencyKind::LexicalCapture,
                expression: edge.expression.clone(),
                parameter_ordinal: edge.parameter_ordinal,
            }),
    );
    graph_edges.sort();
    graph_edges.dedup();
    for edge in &graph_edges {
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
            component: OWNER_INTERFACE_COMPONENT_DOMAIN_V2,
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

    let mut topology_edges_by_component = std::iter::repeat_with(Vec::new)
        .take(component_keys.len())
        .collect::<Vec<_>>();
    for edge in topology_edges {
        let component = component_by_owner[&edge.request];
        topology_edges_by_component[component].push(edge);
    }

    let mut sccs = Vec::with_capacity(component_keys.len());
    for (component, key) in component_keys.iter().cloned().enumerate() {
        let component_edges = std::mem::take(&mut topology_edges_by_component[component]);
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
        let member_fingerprints = key
            .members
            .iter()
            .map(|member| {
                let summary = summaries
                    .binary_search_by(|summary| summary.owner.cmp(member))
                    .ok()
                    .map(|index| summaries[index])
                    .expect("component member has a registered owner summary");
                (
                    stable_check_owner_key_fingerprint_v1(&summary.owner),
                    summary.topology_fingerprint_v1(),
                )
            })
            .collect::<Vec<_>>();
        let fingerprint_v1 = fingerprint(
            OWNER_INTERFACE_SCC_DOMAIN_V2,
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
        OWNER_INTERFACE_TOPOLOGY_DOMAIN_V2,
        &(&stat_values, &scc_fingerprints),
    )?;
    Ok(OwnerInterfaceTopology {
        sccs: sccs.into_boxed_slice(),
        stats,
        scc_by_owner: component_by_owner,
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
            | OwnerConstraintEdgeRole::MatchSelector
            | OwnerConstraintEdgeRole::MatchBinding { .. }
            | OwnerConstraintEdgeRole::MatchNarrowedSelector { .. }
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

fn append_planned_callable_interface_dependencies(
    seed: &OwnerConstraintSeed,
    resolved: ResolvedOwnerSymbolReference,
    signature_plan: &crate::OwnerSignatureLexicalPlan,
    expression_index: usize,
    dependencies: &mut BTreeSet<OwnerConstraintDependency>,
) -> Result<(), OwnerConstraintSeedError> {
    let ResolvedOwnerSymbolReference {
        reference,
        owner: callable,
        projection: _,
        parameters,
    } = resolved;
    let call = signature_plan.call(expression_index).ok_or_else(|| {
        OwnerConstraintSeedError::new(format!(
            "owner {:?} exact signature plan omits callable reference {:?}",
            seed.owner, reference.expression
        ))
    })?;
    if call.stable_expression != reference.expression
        || !matches!(
            &call.target,
            crate::OwnerSignatureCallTarget::Owner { owner } if owner == &callable
        )
    {
        return Err(OwnerConstraintSeedError::new(format!(
            "owner {:?} exact signature target diverges for {:?}",
            seed.owner, reference.expression
        )));
    }

    // Keep the callee interface available for result/error recovery even when
    // the call shape is invalid. Reverse parameter edges are semantic and are
    // published only from a valid exact match.
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
    if !call.valid {
        return Ok(());
    }

    for input in &call.matched_inputs {
        let parameter = parameters
            .binary_search_by_key(&input.formal_ordinal, |parameter| parameter.ordinal)
            .ok()
            .and_then(|index| parameters.get(index))
            .ok_or_else(|| {
                OwnerConstraintSeedError::new(format!(
                    "owner {:?} exact signature input names missing formal {}",
                    seed.owner, input.formal_ordinal
                ))
            })?;
        if parameter.name != input.formal_name || parameter.kind != input.formal_kind {
            return Err(OwnerConstraintSeedError::new(format!(
                "owner {:?} exact signature input diverges from formal {}",
                seed.owner, input.formal_ordinal
            )));
        }
        match (input.formal_kind, input.argument_kind) {
            (OwnerParameterKind::Value, OwnerArgumentKind::Named) => {
                dependencies.insert(interface_edge(
                    &callable,
                    &seed.owner,
                    if input.from_pipe {
                        OwnerConstraintDependencyKind::PipeInputToFormal
                    } else {
                        OwnerConstraintDependencyKind::ActualToFormal
                    },
                    &reference.expression,
                    Some(input.formal_ordinal),
                ));
            }
            (OwnerParameterKind::Out, OwnerArgumentKind::BareBinding) => {
                dependencies.insert(interface_edge(
                    &seed.owner,
                    &callable,
                    OwnerConstraintDependencyKind::FreshOutFromFormal,
                    &reference.expression,
                    Some(input.formal_ordinal),
                ));
            }
            (OwnerParameterKind::Out, OwnerArgumentKind::Named) => {
                dependencies.insert(interface_edge(
                    &callable,
                    &seed.owner,
                    OwnerConstraintDependencyKind::ForwardOut,
                    &reference.expression,
                    Some(input.formal_ordinal),
                ));
                dependencies.insert(interface_edge(
                    &seed.owner,
                    &callable,
                    OwnerConstraintDependencyKind::ForwardOut,
                    &reference.expression,
                    Some(input.formal_ordinal),
                ));
            }
            (OwnerParameterKind::Value, OwnerArgumentKind::BareBinding) => {
                return Err(OwnerConstraintSeedError::new(format!(
                    "owner {:?} valid exact signature retained a bare VALUE input",
                    seed.owner
                )));
            }
        }
    }
    if call.explicit_pass.is_some() {
        dependencies.insert(interface_edge(
            &callable,
            &seed.owner,
            OwnerConstraintDependencyKind::PassedContext,
            &reference.expression,
            None,
        ));
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
            if (reference.kind == OwnerReferenceKind::Callable
                && crate::owner_interface::is_authoritative_callable_name(
                    &reference.parts.join("/"),
                ))
                || (reference.kind == OwnerReferenceKind::Value
                    && reference
                        .parts
                        .first()
                        .is_some_and(|part| boon_syntax::is_program_role_root(part)))
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
    resolve_owner_constraint_seed_with_effective_resolutions(seed, &seed.references, resolutions)
}

pub fn resolve_owner_constraint_seed_with_effective_resolutions(
    seed: &OwnerConstraintSeed,
    effective_references: &[OwnerSymbolReference],
    resolutions: impl IntoIterator<Item = OwnerSymbolResolution>,
) -> Result<OwnerConstraintSummary, OwnerConstraintSeedError> {
    resolve_owner_constraint_seed_with_effective_resolutions_impl(
        seed,
        effective_references,
        resolutions,
        None,
    )
}

pub fn resolve_owner_constraint_seed_with_signature_plan(
    seed: &OwnerConstraintSeed,
    signature_plan: &crate::OwnerSignatureLexicalPlan,
    resolutions: impl IntoIterator<Item = OwnerSymbolResolution>,
) -> Result<OwnerConstraintSummary, OwnerConstraintSeedError> {
    if !signature_plan.matches_seed(seed) {
        return Err(OwnerConstraintSeedError::new(format!(
            "owner constraint signature plan is stale for {:?}",
            seed.owner
        )));
    }
    resolve_owner_constraint_seed_with_effective_resolutions_impl(
        seed,
        signature_plan.external_candidates(),
        resolutions,
        Some(signature_plan),
    )
}

fn resolve_owner_constraint_seed_with_effective_resolutions_impl(
    seed: &OwnerConstraintSeed,
    effective_references: &[OwnerSymbolReference],
    resolutions: impl IntoIterator<Item = OwnerSymbolResolution>,
    signature_plan: Option<&crate::OwnerSignatureLexicalPlan>,
) -> Result<OwnerConstraintSummary, OwnerConstraintSeedError> {
    let expected_references = effective_references.iter().collect::<BTreeSet<_>>();
    if expected_references.len() != effective_references.len()
        || effective_references
            .iter()
            .any(|reference| seed.references.binary_search(reference).is_err())
        || seed
            .references
            .iter()
            .filter(|reference| reference.kind == OwnerReferenceKind::Callable)
            .any(|reference| !expected_references.contains(reference))
    {
        return Err(OwnerConstraintSeedError::new(format!(
            "owner constraint effective references are not a unique callable-complete subset of {:?}",
            seed.owner
        )));
    }
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
    let signature_lexical_fingerprint_v1 =
        signature_plan.map(crate::OwnerSignatureLexicalPlan::fingerprint_v1);
    let lexical_captures = signature_plan
        .map(|plan| plan.imported_captures().to_vec())
        .unwrap_or_default();
    if let Some(plan) = signature_plan {
        for capture_sites in plan.imported_capture_sites() {
            let capture = &capture_sites.target;
            let dependency = match capture {
                OwnerLexicalTargetRef::Declaration { owner, .. }
                | OwnerLexicalTargetRef::ContextFormal { owner } => owner,
                OwnerLexicalTargetRef::Ambiguous { .. } => {
                    return Err(OwnerConstraintSeedError::new(
                        "ambiguous lexical target cannot become an interface capture",
                    ));
                }
            };
            if dependency == &seed.owner {
                return Err(OwnerConstraintSeedError::new(
                    "owner lexical capture unexpectedly targets its consumer",
                ));
            }
            if capture_sites.sites.is_empty() {
                return Err(OwnerConstraintSeedError::new(
                    "owner lexical capture has no exact importing site",
                ));
            }
            let kind = if matches!(
                capture,
                OwnerLexicalTargetRef::Declaration {
                    declaration: OwnerDeclarationStableKey::Public,
                    ..
                }
            ) {
                OwnerConstraintDependencyKind::PublicLexicalCapture
            } else {
                OwnerConstraintDependencyKind::LexicalCapture
            };
            for expression in &capture_sites.sites {
                dependencies.insert(OwnerConstraintDependency {
                    request: seed.owner.clone(),
                    dependency: dependency.clone(),
                    kind,
                    expression: expression.clone(),
                    parameter_ordinal: None,
                });
            }
        }
    }
    let mut symbol_resolutions = BTreeMap::new();
    let expression_indices = seed
        .expressions
        .iter()
        .enumerate()
        .map(|(index, expression)| (expression.expression.clone(), index))
        .collect::<BTreeMap<_, _>>();
    if expression_indices.len() != seed.expressions.len() {
        return Err(OwnerConstraintSeedError::new(format!(
            "owner constraint seed {:?} repeats a stable expression key",
            seed.owner
        )));
    }
    for resolution in resolutions {
        let reference = resolution.reference().clone();
        if !expected_references.contains(&reference) {
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
                if let Some(signature_plan) = signature_plan {
                    let expression_index = expression_indices
                        .get(&resolved.reference.expression)
                        .copied()
                        .ok_or_else(|| {
                            OwnerConstraintSeedError::new(format!(
                                "owner {:?} callable reference {:?} has no constraint expression",
                                seed.owner, resolved.reference.expression
                            ))
                        })?;
                    append_planned_callable_interface_dependencies(
                        seed,
                        resolved,
                        signature_plan,
                        expression_index,
                        &mut dependencies,
                    )?;
                } else {
                    append_callable_interface_dependencies(seed, resolved, &mut dependencies)?;
                }
            }
        }
    }
    if symbol_resolutions.keys().collect::<BTreeSet<_>>() != expected_references {
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
        OWNER_RESOLVED_CONSTRAINT_TOPOLOGY_DOMAIN_V2,
        &(
            stable_check_owner_key_fingerprint_v1(&seed.owner),
            seed.topology_fingerprint_v1(),
            &topology_dependencies,
        ),
    )?;
    let fingerprint_v1 = fingerprint(
        OWNER_RESOLVED_CONSTRAINT_SUMMARY_DOMAIN_V2,
        &(
            stable_check_owner_key_fingerprint_v1(&seed.owner),
            seed.fingerprint_v1(),
            &resolved_references,
            &symbol_resolutions,
            signature_lexical_fingerprint_v1,
            &lexical_captures,
            &dependencies,
        ),
    )?;
    Ok(OwnerConstraintSummary {
        owner: seed.owner.clone(),
        seed_fingerprint_v1: seed.fingerprint_v1(),
        resolved_references: resolved_references.into_boxed_slice(),
        symbol_resolutions: symbol_resolutions.into_boxed_slice(),
        signature_lexical_fingerprint_v1,
        lexical_captures: lexical_captures.into_boxed_slice(),
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

pub fn project_owner_declaration_surface(
    input: &OwnerSyntaxInput,
) -> Result<OwnerDeclarationSurface, OwnerConstraintSeedError> {
    let public = matches!(input.owner, StableCheckOwnerKey::Item(_))
        .then(|| input.statements.first())
        .flatten()
        .map(|statement| declaration(statement.id, true, &statement.kind))
        .transpose()?
        .flatten();
    let fingerprint_v1 = fingerprint(
        OWNER_DECLARATION_SURFACE_DOMAIN_V1,
        &(stable_check_owner_key_fingerprint_v1(&input.owner), &public),
    )?;
    Ok(OwnerDeclarationSurface {
        owner: input.owner.clone(),
        public,
        fingerprint_v1,
    })
}

fn lexical_statement_name(kind: &AstStatementKind) -> Option<&str> {
    match kind {
        AstStatementKind::Field { name } | AstStatementKind::Function { name, .. } => Some(name),
        AstStatementKind::Source { field, .. }
        | AstStatementKind::Hold { field, .. }
        | AstStatementKind::List { field, .. } => field.as_deref(),
        AstStatementKind::Block | AstStatementKind::Spread | AstStatementKind::Expression => None,
    }
}

fn lexical_pattern_names(pattern: &AstMatchPattern) -> Vec<String> {
    match pattern {
        AstMatchPattern::Tag { fields, .. } => fields.clone(),
        AstMatchPattern::Binding { name } => vec![name.clone()],
        AstMatchPattern::Wildcard
        | AstMatchPattern::Number { .. }
        | AstMatchPattern::Text { .. }
        | AstMatchPattern::Invalid { .. }
        | AstMatchPattern::Bits { .. } => Vec::new(),
    }
}

fn lexical_statement_body_container<'a>(
    syntax: &'a OwnerSyntaxInput,
    statement: &crate::OwnerStatementInput,
) -> Option<&'a crate::OwnerExpressionInput> {
    fn is_container(expression: &crate::OwnerExpressionInput) -> bool {
        matches!(
            expression.kind,
            AstExprKind::Block { .. }
                | AstExprKind::Object(_)
                | AstExprKind::ListLiteral { .. }
                | AstExprKind::MapLiteral { .. }
                | AstExprKind::SetLiteral { .. }
        )
    }
    let expression = syntax.expressions.get(statement.expression? as usize)?;
    if is_container(expression) {
        return Some(expression);
    }
    let output = match &expression.kind {
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        }
        | AstExprKind::Then {
            output: Some(output),
            ..
        } => *output,
        _ => return None,
    };
    syntax
        .expressions
        .get(output)
        .filter(|output| is_container(output))
}

fn assign_lexical_expression_region(
    graph: &crate::OwnerSyntaxGraph,
    root: u32,
    scope: u32,
    override_existing: bool,
    expression_scopes: &mut [u32],
    assigned: &mut [bool],
) {
    let mut pending = vec![OwnerExpressionId(root)];
    while let Some(expression) = pending.pop() {
        let index = expression.0 as usize;
        if index >= expression_scopes.len() || (assigned[index] && !override_existing) {
            continue;
        }
        assigned[index] = true;
        expression_scopes[index] = scope;
        pending.extend(
            graph
                .expression_inputs(expression)
                .unwrap_or_default()
                .iter()
                .filter_map(|input| match input {
                    OwnerExpressionRef::Local { expression } => Some(*expression),
                    OwnerExpressionRef::Child { .. } => None,
                }),
        );
    }
}

fn lexical_record_fields(
    expression: &crate::OwnerExpressionInput,
) -> Option<&[boon_syntax::AstRecordField]> {
    match &expression.kind {
        AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } => Some(fields),
        _ => None,
    }
}

fn record_field_child_public_target(
    input: &OwnerSyntaxInput,
    value: usize,
) -> Result<Option<OwnerLexicalTargetRef>, OwnerConstraintSeedError> {
    if value < input.expressions.len() {
        return Ok(None);
    }
    let external = input.external_expression(value).ok_or_else(|| {
        OwnerConstraintSeedError::new(
            "owner lexical record field references a missing external expression",
        )
    })?;
    let mut matches = input.child_owners.iter().filter(|child| {
        if child.result_expression.is_none()
            || !(child.owner == external.owner
                || crate::owner_syntax::is_descendant_owner(&child.owner, &external.owner))
        {
            return false;
        }
        match &child.result_placement {
            crate::OwnerChildResultPlacementInput::Valueless => false,
            crate::OwnerChildResultPlacementInput::StatementLane => child
                .result_expression
                .as_ref()
                .is_some_and(|result| result == &external.expression),
            crate::OwnerChildResultPlacementInput::ExpressionEdge { edge } => {
                (edge.child_owner == external.owner && edge.child_expression == external.expression)
                    || child
                        .result_expression
                        .as_ref()
                        .is_some_and(|result| result == &external.expression)
                    || child
                        .boundary_expression
                        .as_ref()
                        .is_some_and(|boundary| boundary == &external.expression)
            }
        }
    });
    let child = matches.next().ok_or_else(|| {
        OwnerConstraintSeedError::new(
            "owner lexical record field has no exact child declaration provider",
        )
    })?;
    if matches.next().is_some() {
        return Err(OwnerConstraintSeedError::new(
            "owner lexical record field has multiple child declaration providers",
        ));
    }
    Ok(Some(OwnerLexicalTargetRef::Declaration {
        owner: child.owner.clone(),
        declaration: OwnerDeclarationStableKey::Public,
        capability: OwnerLexicalDeclarationCapability::Value,
    }))
}

fn assign_lexical_scope_regions(
    graph: &crate::OwnerSyntaxGraph,
    expression: OwnerExpressionId,
    inherited_scope: u32,
    boundaries: &BTreeMap<u32, u32>,
    scopes: &mut [OwnerLexicalScopePlan],
    expression_scopes: &mut [u32],
    assigned: &mut [Option<u32>],
    child_scopes: &mut BTreeMap<(StableCheckOwnerKey, StableExpressionKey), u32>,
    active: &mut BTreeSet<OwnerExpressionId>,
) -> Result<(), OwnerConstraintSeedError> {
    let index = expression.0 as usize;
    if index >= expression_scopes.len() {
        return Ok(());
    }
    let scope = boundaries
        .get(&expression.0)
        .copied()
        .unwrap_or(inherited_scope);
    if scope != inherited_scope {
        let planned = scopes.get_mut(scope as usize).ok_or_else(|| {
            OwnerConstraintSeedError::new("owner lexical boundary references a missing scope")
        })?;
        match planned.parent {
            None => planned.parent = Some(inherited_scope),
            Some(parent) if parent == inherited_scope => {}
            Some(parent) => {
                return Err(OwnerConstraintSeedError::new(format!(
                    "owner lexical scope {scope} has conflicting parents {parent} and {inherited_scope}"
                )));
            }
        }
    }
    match assigned[index] {
        Some(previous) if previous == scope => return Ok(()),
        Some(previous) => {
            return Err(OwnerConstraintSeedError::new(format!(
                "owner expression {} belongs to conflicting lexical scopes {previous} and {scope}",
                expression.0
            )));
        }
        None => assigned[index] = Some(scope),
    }
    if !active.insert(expression) {
        return Err(OwnerConstraintSeedError::new(
            "owner lexical expression graph contains a cycle",
        ));
    }
    expression_scopes[index] = scope;
    for child in graph.expression_inputs(expression).unwrap_or_default() {
        match child {
            OwnerExpressionRef::Local { expression: child } => {
                assign_lexical_scope_regions(
                    graph,
                    *child,
                    scope,
                    boundaries,
                    scopes,
                    expression_scopes,
                    assigned,
                    child_scopes,
                    active,
                )?;
            }
            OwnerExpressionRef::Child { owner, expression } => {
                let key = (owner.clone(), expression.clone());
                match child_scopes.entry(key) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(scope);
                    }
                    std::collections::btree_map::Entry::Occupied(entry)
                        if *entry.get() == scope => {}
                    std::collections::btree_map::Entry::Occupied(entry) => {
                        return Err(OwnerConstraintSeedError::new(format!(
                            "owner child expression belongs to conflicting lexical scopes {} and {scope}",
                            entry.get()
                        )));
                    }
                }
            }
        }
    }
    active.remove(&expression);
    Ok(())
}

fn insert_lexical_declaration(
    declarations: &mut [BTreeMap<String, OwnerLexicalDeclarationTarget>],
    scope: u32,
    name: String,
    target: OwnerLexicalDeclarationTarget,
) -> Result<(), OwnerConstraintSeedError> {
    let scope = declarations.get_mut(scope as usize).ok_or_else(|| {
        OwnerConstraintSeedError::new("owner lexical declaration references a missing scope")
    })?;
    match scope.entry(name.clone()) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(target);
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            entry.insert(OwnerLexicalDeclarationTarget::Ambiguous { name });
        }
    }
    Ok(())
}

fn lexical_reference(
    expression: &crate::OwnerExpressionInput,
) -> Option<(OwnerReferenceKind, Box<[String]>)> {
    match &expression.kind {
        AstExprKind::Identifier(name) => Some((
            OwnerReferenceKind::Value,
            vec![name.clone()].into_boxed_slice(),
        )),
        AstExprKind::Path(parts) => {
            Some((OwnerReferenceKind::Value, parts.clone().into_boxed_slice()))
        }
        AstExprKind::Drain { path } => Some((
            OwnerReferenceKind::Value,
            drain_parts(path).into_boxed_slice(),
        )),
        AstExprKind::Call { function, .. } => Some((
            OwnerReferenceKind::Callable,
            function
                .split('/')
                .map(str::to_owned)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )),
        AstExprKind::Pipe { op, .. } if op != "WHILE" => Some((
            OwnerReferenceKind::Callable,
            op.split('/')
                .map(str::to_owned)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )),
        _ => None,
    }
}

fn lexical_access(expression: &crate::OwnerExpressionInput) -> Option<OwnerLexicalAccess> {
    match expression.kind {
        AstExprKind::Identifier(_) | AstExprKind::Path(_) => Some(OwnerLexicalAccess::Read),
        AstExprKind::Drain { .. } => Some(OwnerLexicalAccess::Drain),
        _ => None,
    }
}

fn lexical_read_parts(expression: &crate::OwnerExpressionInput) -> Option<Box<[String]>> {
    match &expression.kind {
        AstExprKind::Identifier(name) => Some(vec![name.clone()].into_boxed_slice()),
        AstExprKind::Path(parts) => Some(parts.clone().into_boxed_slice()),
        AstExprKind::Drain { path } => Some(drain_parts(path).into_boxed_slice()),
        _ => None,
    }
}

fn resolve_lexical_read_target(
    root: &str,
    origin_scope: u32,
    scopes: &[OwnerLexicalScopePlan],
    declarations: &[BTreeMap<String, OwnerLexicalDeclarationTarget>],
) -> Option<(OwnerLexicalDeclarationTarget, Option<u32>)> {
    if root == "PASSED" {
        return Some((OwnerLexicalDeclarationTarget::Passed, None));
    }
    let mut scope = origin_scope;
    loop {
        if let Some(target) = declarations[scope as usize].get(root) {
            return Some((target.clone(), Some(scope)));
        }
        scope = scopes[scope as usize].parent?;
    }
}

fn containing_stable_scope(input: &OwnerSyntaxInput) -> Option<OwnerStableScopeRef> {
    match &input.containing_scope {
        OwnerContainingScopeInput::ProjectRoot => None,
        OwnerContainingScopeInput::OwnerStatement { owner, statement } => {
            Some(OwnerStableScopeRef {
                owner: owner.clone(),
                scope: OwnerScopeStableKey::Statement {
                    statement: statement.clone(),
                    role: OwnerStatementScopeRole::Body,
                },
            })
        }
    }
}

fn stable_scope_projection(
    input: &OwnerSyntaxInput,
    scopes: &[OwnerLexicalScopePlan],
) -> Result<Vec<Option<OwnerStableScopeRef>>, OwnerConstraintSeedError> {
    scopes
        .iter()
        .map(|scope| {
            let scope = match scope.origin {
                OwnerLexicalScopeOrigin::Root => return Ok(containing_stable_scope(input)),
                OwnerLexicalScopeOrigin::StatementBody { statement } => {
                    let statement = input.statements.get(statement as usize).ok_or_else(|| {
                        OwnerConstraintSeedError::new(
                            "owner lexical scope references a missing statement",
                        )
                    })?;
                    OwnerScopeStableKey::Statement {
                        statement: statement.stable_key.clone(),
                        role: OwnerStatementScopeRole::Body,
                    }
                }
                OwnerLexicalScopeOrigin::PatternArm { expression } => {
                    let expression =
                        input.expressions.get(expression as usize).ok_or_else(|| {
                            OwnerConstraintSeedError::new(
                                "owner lexical scope references a missing pattern expression",
                            )
                        })?;
                    OwnerScopeStableKey::Expression {
                        expression: expression.stable_key.clone(),
                        role: OwnerExpressionScopeRole::MatchArm,
                    }
                }
                OwnerLexicalScopeOrigin::Record { expression } => {
                    let expression =
                        input.expressions.get(expression as usize).ok_or_else(|| {
                            OwnerConstraintSeedError::new(
                                "owner lexical scope references a missing record expression",
                            )
                        })?;
                    OwnerScopeStableKey::Expression {
                        expression: expression.stable_key.clone(),
                        role: OwnerExpressionScopeRole::Record,
                    }
                }
            };
            Ok(Some(OwnerStableScopeRef {
                owner: input.owner.clone(),
                scope,
            }))
        })
        .collect()
}

fn stable_lexical_target(
    input: &OwnerSyntaxInput,
    target: &OwnerLexicalDeclarationTarget,
) -> Result<Option<OwnerLexicalTargetRef>, OwnerConstraintSeedError> {
    let declaration = match target {
        OwnerLexicalDeclarationTarget::Parameter { ordinal } => {
            let Some(statement) = input.statements.first() else {
                return Err(OwnerConstraintSeedError::new(
                    "owner lexical parameter has no function statement",
                ));
            };
            let AstStatementKind::Function { parameters, .. } = &statement.kind else {
                return Err(OwnerConstraintSeedError::new(
                    "owner lexical parameter belongs to a non-function owner",
                ));
            };
            let parameter = parameters
                .iter()
                .find(|parameter| parameter.ordinal as u32 == *ordinal)
                .ok_or_else(|| {
                    OwnerConstraintSeedError::new("owner lexical parameter ordinal is not declared")
                })?;
            let capability = match parameter.kind {
                boon_syntax::AstParameterKind::Value => OwnerLexicalDeclarationCapability::Value,
                boon_syntax::AstParameterKind::Out => OwnerLexicalDeclarationCapability::Out {
                    evaluation_scope: OwnerStableScopeRef {
                        owner: input.owner.clone(),
                        scope: OwnerScopeStableKey::Statement {
                            statement: statement.stable_key.clone(),
                            role: OwnerStatementScopeRole::RepeatedOutput {
                                parameter_ordinal: *ordinal,
                            },
                        },
                    },
                },
            };
            OwnerLexicalTargetRef::Declaration {
                owner: input.owner.clone(),
                declaration: OwnerDeclarationStableKey::Parameter { ordinal: *ordinal },
                capability,
            }
        }
        OwnerLexicalDeclarationTarget::Statement { statement } => {
            let statement_input = input.statements.get(*statement as usize).ok_or_else(|| {
                OwnerConstraintSeedError::new(
                    "owner lexical declaration references a missing statement",
                )
            })?;
            let declaration =
                if matches!(input.owner, StableCheckOwnerKey::Item(_)) && *statement == 0 {
                    OwnerDeclarationStableKey::Public
                } else {
                    OwnerDeclarationStableKey::Statement {
                        statement: statement_input.stable_key.clone(),
                    }
                };
            OwnerLexicalTargetRef::Declaration {
                owner: input.owner.clone(),
                declaration,
                capability: if matches!(statement_input.kind, AstStatementKind::Function { .. }) {
                    OwnerLexicalDeclarationCapability::CallableOnly
                } else {
                    OwnerLexicalDeclarationCapability::Value
                },
            }
        }
        OwnerLexicalDeclarationTarget::RecordField {
            object,
            ordinal,
            name,
        } => {
            let expression = input.expressions.get(*object as usize).ok_or_else(|| {
                OwnerConstraintSeedError::new(
                    "owner lexical record declaration references a missing expression",
                )
            })?;
            OwnerLexicalTargetRef::Declaration {
                owner: input.owner.clone(),
                declaration: OwnerDeclarationStableKey::RecordField {
                    object: expression.stable_key.clone(),
                    ordinal: *ordinal,
                    name: name.clone(),
                },
                capability: OwnerLexicalDeclarationCapability::Value,
            }
        }
        OwnerLexicalDeclarationTarget::PatternBinding { arm, name } => {
            let expression = input.expressions.get(*arm as usize).ok_or_else(|| {
                OwnerConstraintSeedError::new(
                    "owner lexical pattern declaration references a missing expression",
                )
            })?;
            let AstExprKind::MatchArm { pattern, .. } = &expression.kind else {
                return Err(OwnerConstraintSeedError::new(
                    "owner lexical pattern declaration belongs to a non-arm expression",
                ));
            };
            let ordinal = lexical_pattern_names(pattern)
                .iter()
                .position(|candidate| candidate == name)
                .ok_or_else(|| {
                    OwnerConstraintSeedError::new(
                        "owner lexical pattern declaration name is not present in its arm",
                    )
                })?;
            OwnerLexicalTargetRef::Declaration {
                owner: input.owner.clone(),
                declaration: OwnerDeclarationStableKey::PatternBinding {
                    selector: expression.stable_key.clone(),
                    ordinal: checked_u32(ordinal, "owner pattern binding ordinal")?,
                    name: name.clone(),
                },
                capability: OwnerLexicalDeclarationCapability::Value,
            }
        }
        OwnerLexicalDeclarationTarget::Imported { target } => target.clone(),
        OwnerLexicalDeclarationTarget::Passed => {
            if input.statements.first().is_some_and(|statement| {
                matches!(statement.kind, AstStatementKind::Function { .. })
            }) {
                OwnerLexicalTargetRef::ContextFormal {
                    owner: input.owner.clone(),
                }
            } else {
                return Ok(None);
            }
        }
        OwnerLexicalDeclarationTarget::Ambiguous { name } => OwnerLexicalTargetRef::Ambiguous {
            owner: input.owner.clone(),
            name: name.clone(),
        },
    };
    Ok(Some(declaration))
}

fn project_owner_lexical_containment(
    input: &OwnerSyntaxInput,
    scopes: &[OwnerLexicalScopePlan],
    statement_scopes: &[u32],
    statement_body_scopes: &[Option<u32>],
    declarations: &[BTreeMap<String, OwnerLexicalDeclarationTarget>],
    stable_scopes: &[Option<OwnerStableScopeRef>],
    stable_targets: &BTreeMap<OwnerLexicalDeclarationTarget, OwnerLexicalTargetRef>,
    child_scopes: &BTreeMap<(StableCheckOwnerKey, StableExpressionKey), u32>,
) -> Result<OwnerLexicalContainmentPlan, OwnerConstraintSeedError> {
    let passed = stable_targets
        .get(&OwnerLexicalDeclarationTarget::Passed)
        .cloned();
    let mut external_expression_counts = BTreeMap::<StableExpressionKey, usize>::new();
    for external in &input.external_expressions {
        *external_expression_counts
            .entry(external.expression.clone())
            .or_default() += 1;
    }
    let mut bindings_by_scope = BTreeMap::<u32, OwnerLexicalBoundaryBindings>::new();
    let mut children = Vec::with_capacity(input.child_owners.len());
    for child in &input.child_owners {
        let parent_statement = child
            .parent
            .map(|parent| {
                input
                    .statements
                    .get(parent as usize)
                    .map(|statement| statement.stable_key.clone())
                    .ok_or_else(|| {
                        OwnerConstraintSeedError::new(
                            "owner lexical child boundary references a missing parent statement",
                        )
                    })
            })
            .transpose()?;
        let fallback_scope = child.parent.map(|parent| {
            statement_body_scopes[parent as usize].unwrap_or(statement_scopes[parent as usize])
        });
        let scope_id = match &child.result_placement {
            crate::OwnerChildResultPlacementInput::Valueless => None,
            crate::OwnerChildResultPlacementInput::StatementLane => fallback_scope,
            crate::OwnerChildResultPlacementInput::ExpressionEdge { edge } => {
                Some(*child_scopes.get(&(edge.child_owner.clone(), edge.child_expression.clone())).ok_or_else(|| {
                    OwnerConstraintSeedError::new(
                        "owner child result expression edge was not observed by the lexical scope walk",
                    )
                })?)
            }
        };
        let scope = scope_id
            .and_then(|scope| stable_scopes.get(scope as usize))
            .cloned()
            .flatten();
        let inherits = parent_statement.is_some() && child.result_expression.is_some();
        if inherits && scope.is_none() {
            return Err(OwnerConstraintSeedError::new(
                "inheriting owner child boundary has no stable containing scope",
            ));
        }
        if let Some(result_expression) = &child.result_expression {
            let matches = external_expression_counts
                .get(result_expression)
                .copied()
                .unwrap_or_default();
            if matches != 1 {
                return Err(OwnerConstraintSeedError::new(
                    "owner lexical child boundary has no unique external result expression",
                ));
            }
        }

        let bindings = if inherits {
            let scope_id = scope_id.ok_or_else(|| {
                OwnerConstraintSeedError::new(
                    "inheriting owner child boundary lost its dense containing scope",
                )
            })?;
            if let Some(bindings) = bindings_by_scope.get(&scope_id) {
                bindings.clone()
            } else {
                let mut visible = BTreeMap::new();
                let mut current = scope_id;
                loop {
                    for (name, target) in declarations.get(current as usize).ok_or_else(|| {
                        OwnerConstraintSeedError::new(
                            "owner lexical child boundary references a missing declaration scope",
                        )
                    })? {
                        if visible.contains_key(name) {
                            continue;
                        }
                        let target = stable_targets.get(target).cloned().ok_or_else(|| {
                            OwnerConstraintSeedError::new(
                                "owner lexical child boundary declaration has no stable target",
                            )
                        })?;
                        visible.insert(
                            name.clone(),
                            (
                                target,
                                stable_scopes.get(current as usize).cloned().flatten(),
                            ),
                        );
                    }
                    let Some(parent) = scopes
                        .get(current as usize)
                        .ok_or_else(|| {
                            OwnerConstraintSeedError::new(
                                "owner lexical child boundary references a missing scope",
                            )
                        })?
                        .parent
                    else {
                        break;
                    };
                    current = parent;
                }
                if let Some(passed) = &passed {
                    // PASSED is a language-reserved binding and therefore wins
                    // even if an authored declaration reused the spelling.
                    visible.insert("PASSED".to_owned(), (passed.clone(), None));
                }
                let bindings = OwnerLexicalBoundaryBindings::try_new(
                    visible
                        .into_iter()
                        .map(|(name, (target, declaration_scope))| {
                            OwnerLexicalBoundaryBindingPlan {
                                name,
                                target,
                                declaration_scope,
                            }
                        })
                        .collect(),
                )
                .map_err(|error| {
                    OwnerConstraintSeedError::new(format!(
                        "cannot fingerprint owner lexical boundary bindings: {error}"
                    ))
                })?;
                bindings_by_scope.insert(scope_id, bindings.clone());
                bindings
            }
        } else {
            OwnerLexicalBoundaryBindings::default()
        };
        children.push(OwnerLexicalChildBoundaryPlan {
            owner: child.owner.clone(),
            parent_statement,
            child_index: child.child_index,
            boundary_expression: child.boundary_expression.clone(),
            result_expression: child.result_expression.clone(),
            result_placement: child.result_placement.clone(),
            scope,
            bindings,
        });
    }
    let child_fingerprint_rows = children
        .iter()
        .map(|child| {
            (
                &child.owner,
                &child.parent_statement,
                child.child_index,
                &child.boundary_expression,
                &child.result_expression,
                &child.result_placement,
                &child.scope,
                child.bindings.fingerprint_v1(),
            )
        })
        .collect::<Vec<_>>();
    let fingerprint_v1 = fingerprint(
        OWNER_LEXICAL_CONTAINMENT_DOMAIN_V2,
        &(
            &input.owner,
            &input.containing_scope,
            &child_fingerprint_rows,
        ),
    )?;
    Ok(OwnerLexicalContainmentPlan {
        owner: input.owner.clone(),
        containing_scope: input.containing_scope.clone(),
        children: children.into_boxed_slice(),
        fingerprint_v1,
    })
}

pub fn project_owner_lexical_plan(
    input: &OwnerSyntaxInput,
) -> Result<OwnerLexicalPlan, OwnerConstraintSeedError> {
    let graph = crate::OwnerSyntaxGraph::build(input).map_err(|error| {
        OwnerConstraintSeedError::new(format!(
            "cannot derive lexical plan for {:?}: {error}",
            input.owner
        ))
    })?;
    let mut expression_by_key = BTreeMap::new();
    for (index, expression) in input.expressions.iter().enumerate() {
        if expression_by_key
            .insert(expression.stable_key.clone(), index)
            .is_some()
        {
            return Err(OwnerConstraintSeedError::new(
                "owner lexical input repeats a stable expression key",
            ));
        }
    }
    let mut external_by_key = BTreeMap::new();
    for (index, external) in input.external_expressions.iter().enumerate() {
        if external_by_key
            .insert((external.owner.clone(), external.expression.clone()), index)
            .is_some()
        {
            return Err(OwnerConstraintSeedError::new(
                "owner lexical input repeats an external expression key",
            ));
        }
    }
    let mut statement_by_expression = BTreeMap::new();
    for (index, statement) in input.statements.iter().enumerate() {
        let Some(expression) = statement.expression else {
            continue;
        };
        if statement_by_expression.insert(expression, index).is_some() {
            return Err(OwnerConstraintSeedError::new(
                "owner lexical input repeats a direct statement expression",
            ));
        }
    }
    let mut statement_values = Vec::new();
    for statement in graph.statements() {
        let Some(value) = statement.canonical_value.as_ref() else {
            continue;
        };
        let expression = match value {
            OwnerExpressionRef::Local { expression } => expression.0,
            OwnerExpressionRef::Child { owner, expression } => {
                let external = external_by_key
                    .get(&(owner.clone(), expression.clone()))
                    .copied()
                    .ok_or_else(|| {
                        OwnerConstraintSeedError::new(
                            "owner lexical statement value has no external expression row",
                        )
                    })?;
                checked_u32(
                    input
                        .expressions
                        .len()
                        .checked_add(external)
                        .ok_or_else(|| {
                            OwnerConstraintSeedError::new(
                                "owner lexical statement value index overflowed",
                            )
                        })?,
                    "owner lexical external statement value",
                )?
            }
        };
        statement_values.push((statement.id.0, expression));
    }

    let mut scopes = vec![OwnerLexicalScopePlan {
        parent: None,
        origin: OwnerLexicalScopeOrigin::Root,
    }];
    let mut statement_scopes = vec![0u32; input.statements.len()];
    let mut statement_body_scopes = vec![None; input.statements.len()];
    for (index, statement) in input.statements.iter().enumerate() {
        let parent_scope = statement.parent.map_or(0, |parent| {
            statement_body_scopes[parent as usize].unwrap_or(statement_scopes[parent as usize])
        });
        statement_scopes[index] = parent_scope;
        let node = graph
            .statement(OwnerStatementId(statement.id))
            .ok_or_else(|| OwnerConstraintSeedError::new("owner lexical graph lost a statement"))?;
        if matches!(statement.kind, AstStatementKind::Function { .. }) || !node.children.is_empty()
        {
            let scope = checked_u32(scopes.len(), "owner lexical scope")?;
            scopes.push(OwnerLexicalScopePlan {
                parent: Some(parent_scope),
                origin: OwnerLexicalScopeOrigin::StatementBody {
                    statement: statement.id,
                },
            });
            statement_body_scopes[index] = Some(scope);
        }
    }

    let mut expression_scopes = vec![0u32; input.expressions.len()];
    let mut assigned = vec![false; input.expressions.len()];
    for (index, statement) in input.statements.iter().enumerate() {
        if let Some(expression) = statement.expression {
            assign_lexical_expression_region(
                &graph,
                expression,
                statement_scopes[index],
                false,
                &mut expression_scopes,
                &mut assigned,
            );
        }
        if let Some(container) = lexical_statement_body_container(input, statement)
            && let Some(scope) = statement_body_scopes[index]
            && let Some(expression) = expression_by_key.get(&container.stable_key).copied()
        {
            assign_lexical_expression_region(
                &graph,
                checked_u32(expression, "owner lexical body container")?,
                scope,
                true,
                &mut expression_scopes,
                &mut assigned,
            );
        }
    }

    let mut pattern_scopes = BTreeMap::new();
    for (arm, expression) in input.expressions.iter().enumerate() {
        let AstExprKind::MatchArm { .. } = &expression.kind else {
            continue;
        };
        let scope = checked_u32(scopes.len(), "owner pattern scope")?;
        // Expression ids follow parser allocation order, so a nested inline
        // arm may precede its enclosing arm. Leave the parent open here and
        // let the structural boundary walk below attach it to the actual
        // enclosing scope.
        scopes.push(OwnerLexicalScopePlan {
            parent: None,
            origin: OwnerLexicalScopeOrigin::PatternArm {
                expression: checked_u32(arm, "owner pattern expression")?,
            },
        });
        pattern_scopes.insert(checked_u32(arm, "owner pattern expression")?, scope);
        let statement = statement_by_expression.get(&(arm as u32)).copied();
        let body_scope = statement.and_then(|statement| statement_body_scopes[statement]);
        if let Some(statement) = statement {
            statement_scopes[statement] = scope;
        }
        if let Some(body_scope) = body_scope {
            scopes[body_scope as usize].parent = Some(scope);
        }
    }

    let mut expression_boundaries = BTreeMap::new();
    let mut statement_record_scopes = BTreeMap::new();
    for (index, statement) in input.statements.iter().enumerate() {
        let Some(scope) = statement_body_scopes[index] else {
            continue;
        };
        let Some(container) = lexical_statement_body_container(input, statement) else {
            continue;
        };
        let Some(expression) = expression_by_key.get(&container.stable_key).copied() else {
            continue;
        };
        let expression = checked_u32(expression, "owner lexical body container")?;
        expression_boundaries.insert(expression, scope);
        if matches!(container.kind, AstExprKind::Object(_)) {
            match statement_record_scopes.entry(expression) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(scope);
                }
                std::collections::btree_map::Entry::Occupied(entry) if *entry.get() == scope => {}
                std::collections::btree_map::Entry::Occupied(_) => {
                    return Err(OwnerConstraintSeedError::new(
                        "owner lexical record container belongs to conflicting statement scopes",
                    ));
                }
            }
        }
    }
    for (arm, scope) in &pattern_scopes {
        expression_boundaries.insert(*arm, *scope);
    }

    let mut record_scopes = Vec::new();
    for (index, expression) in input.expressions.iter().enumerate() {
        let expression_id = checked_u32(index, "owner lexical record expression")?;
        if lexical_record_fields(expression).is_none()
            || statement_record_scopes.contains_key(&expression_id)
        {
            continue;
        }
        let scope = checked_u32(scopes.len(), "owner lexical record scope")?;
        scopes.push(OwnerLexicalScopePlan {
            parent: None,
            origin: OwnerLexicalScopeOrigin::Record {
                expression: expression_id,
            },
        });
        expression_boundaries.insert(expression_id, scope);
        record_scopes.push((expression_id, scope));
    }

    let expression_children = (0..input.expressions.len())
        .flat_map(|index| {
            graph
                .expression_inputs(OwnerExpressionId(index as u32))
                .unwrap_or_default()
        })
        .filter_map(|input| match input {
            OwnerExpressionRef::Local { expression } => Some(expression.0),
            OwnerExpressionRef::Child { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    let mut scoped = vec![None; input.expressions.len()];
    let mut child_scopes = BTreeMap::new();
    for index in 0..input.expressions.len() {
        let expression = OwnerExpressionId(checked_u32(index, "owner lexical expression")?);
        if expression_children.contains(&expression.0) {
            continue;
        }
        let inherited_scope = expression_scopes[index];
        assign_lexical_scope_regions(
            &graph,
            expression,
            inherited_scope,
            &expression_boundaries,
            &mut scopes,
            &mut expression_scopes,
            &mut scoped,
            &mut child_scopes,
            &mut BTreeSet::new(),
        )?;
    }
    for index in 0..input.expressions.len() {
        if scoped[index].is_some() {
            continue;
        }
        let expression = OwnerExpressionId(checked_u32(index, "owner lexical expression")?);
        let inherited_scope = expression_scopes[index];
        assign_lexical_scope_regions(
            &graph,
            expression,
            inherited_scope,
            &expression_boundaries,
            &mut scopes,
            &mut expression_scopes,
            &mut scoped,
            &mut child_scopes,
            &mut BTreeSet::new(),
        )?;
    }

    let mut declarations = vec![BTreeMap::new(); scopes.len()];
    for (index, statement) in input.statements.iter().enumerate() {
        let Some(name) = lexical_statement_name(&statement.kind) else {
            continue;
        };
        insert_lexical_declaration(
            &mut declarations,
            statement_scopes[index],
            name.to_owned(),
            OwnerLexicalDeclarationTarget::Statement {
                statement: statement.id,
            },
        )?;
    }
    if let Some(statement) = input.statements.first()
        && let AstStatementKind::Function { parameters, .. } = &statement.kind
        && let Some(scope) = statement_body_scopes.first().copied().flatten()
    {
        for parameter in parameters {
            insert_lexical_declaration(
                &mut declarations,
                scope,
                parameter.name.clone(),
                OwnerLexicalDeclarationTarget::Parameter {
                    ordinal: checked_u32(parameter.ordinal, "owner parameter ordinal")?,
                },
            )?;
        }
    }
    for (arm, scope) in &pattern_scopes {
        let AstExprKind::MatchArm { pattern, .. } = &input.expressions[*arm as usize].kind else {
            continue;
        };
        for name in lexical_pattern_names(pattern) {
            insert_lexical_declaration(
                &mut declarations,
                *scope,
                name.clone(),
                OwnerLexicalDeclarationTarget::PatternBinding { arm: *arm, name },
            )?;
        }
    }
    let mut record_fields = Vec::new();
    for (object, scope) in &record_scopes {
        let fields = lexical_record_fields(&input.expressions[*object as usize])
            .expect("record scopes belong to record expressions");
        for (ordinal, field) in fields.iter().enumerate() {
            if field.spread {
                continue;
            }
            let ordinal = checked_u32(ordinal, "owner lexical record field ordinal")?;
            record_fields.push(OwnerLexicalRecordFieldPlan {
                object: *object,
                ordinal,
                name: field.name.clone(),
                scope: *scope,
            });
            insert_lexical_declaration(
                &mut declarations,
                *scope,
                field.name.clone(),
                OwnerLexicalDeclarationTarget::RecordField {
                    object: *object,
                    ordinal,
                    name: field.name.clone(),
                },
            )?;
        }
    }

    // A statement-bodied record can shard each field into an independently
    // checked child owner. The child Public declaration is the field's stable
    // lexical identity; routing sibling reads through a parent-owned synthetic
    // RecordField would couple every field through the whole record interface
    // and collapse wide records into one inference SCC.
    for (object, scope) in &statement_record_scopes {
        let fields = lexical_record_fields(&input.expressions[*object as usize])
            .expect("statement record scopes belong to record expressions");
        for (ordinal, field) in fields.iter().enumerate() {
            if field.spread
                || declarations.get(*scope as usize).is_some_and(|scope| {
                    matches!(
                        scope.get(&field.name),
                        Some(
                            OwnerLexicalDeclarationTarget::Statement { .. }
                                | OwnerLexicalDeclarationTarget::Ambiguous { .. }
                        )
                    )
                })
            {
                continue;
            }
            if let Some(target) = record_field_child_public_target(input, field.value)? {
                insert_lexical_declaration(
                    &mut declarations,
                    *scope,
                    field.name.clone(),
                    OwnerLexicalDeclarationTarget::Imported { target },
                )?;
            } else {
                let ordinal = checked_u32(ordinal, "owner lexical record field ordinal")?;
                record_fields.push(OwnerLexicalRecordFieldPlan {
                    object: *object,
                    ordinal,
                    name: field.name.clone(),
                    scope: *scope,
                });
                insert_lexical_declaration(
                    &mut declarations,
                    *scope,
                    field.name.clone(),
                    OwnerLexicalDeclarationTarget::RecordField {
                        object: *object,
                        ordinal,
                        name: field.name.clone(),
                    },
                )?;
            }
        }
    }

    // BLOCK declarations can be independently checked child owners. They are
    // still whole-scope lexical declarations in the containing BLOCK; only
    // their body and checked rows are sharded. Normalize the child row to its
    // canonical stable public declaration before any read or boundary plan is
    // projected.
    for (expression_index, expression) in input.expressions.iter().enumerate() {
        let AstExprKind::Block { bindings, .. } = &expression.kind else {
            continue;
        };
        let scope = expression_scopes[expression_index];
        for binding in bindings {
            let AstBlockBindingDeclaration::Child { child } = binding.declaration else {
                continue;
            };
            let child = input.child_owners.get(child).ok_or_else(|| {
                OwnerConstraintSeedError::new(
                    "owner lexical BLOCK declaration references a missing child row",
                )
            })?;
            let target = OwnerLexicalTargetRef::Declaration {
                owner: child.owner.clone(),
                declaration: OwnerDeclarationStableKey::Public,
                capability: OwnerLexicalDeclarationCapability::Value,
            };
            insert_lexical_declaration(
                &mut declarations,
                scope,
                binding.name.clone(),
                OwnerLexicalDeclarationTarget::Imported { target },
            )?;
        }
    }
    // Nested FUNCTION owners have no statement-lane value and therefore no
    // AstBlockBinding value edge. Their callable-only name still belongs to
    // the containing whole scope, while the function body resets inheritance.
    for child in &input.child_owners {
        if child.result_expression.is_some() {
            continue;
        }
        let Some(parent) = child.parent else {
            continue;
        };
        let StableCheckOwnerKey::Item(owner) = &child.owner else {
            continue;
        };
        let Some(segment) = owner.item_route.segments().last() else {
            continue;
        };
        if segment.kind != UnitItemKind::Function {
            continue;
        }
        let scope =
            statement_body_scopes[parent as usize].unwrap_or(statement_scopes[parent as usize]);
        for name in &segment.names {
            insert_lexical_declaration(
                &mut declarations,
                scope,
                name.clone(),
                OwnerLexicalDeclarationTarget::Imported {
                    target: OwnerLexicalTargetRef::Declaration {
                        owner: child.owner.clone(),
                        declaration: OwnerDeclarationStableKey::Public,
                        capability: OwnerLexicalDeclarationCapability::CallableOnly,
                    },
                },
            )?;
        }
    }

    let stable_scopes = stable_scope_projection(input, &scopes)?;
    let mut stable_targets = BTreeMap::new();
    let passed_target = OwnerLexicalDeclarationTarget::Passed;
    for target in declarations
        .iter()
        .flat_map(|scope| scope.values())
        .chain(std::iter::once(&passed_target))
    {
        let Some(stable) = stable_lexical_target(input, target)? else {
            continue;
        };
        match stable_targets.entry(target.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(stable);
            }
            std::collections::btree_map::Entry::Occupied(entry) if entry.get() == &stable => {}
            std::collections::btree_map::Entry::Occupied(_) => {
                return Err(OwnerConstraintSeedError::new(
                    "owner lexical declaration has conflicting stable targets",
                ));
            }
        }
    }
    let containment = Arc::new(project_owner_lexical_containment(
        input,
        &scopes,
        &statement_scopes,
        &statement_body_scopes,
        &declarations,
        &stable_scopes,
        &stable_targets,
        &child_scopes,
    )?);

    let mut reads = vec![None; input.expressions.len()];
    let mut external_candidates = BTreeSet::new();
    for (index, expression) in input.expressions.iter().enumerate() {
        let Some((kind, parts)) = lexical_reference(expression) else {
            if let (Some(access), Some(parts)) =
                (lexical_access(expression), lexical_read_parts(expression))
                && let Some((root, projection)) = parts.split_first()
            {
                if let Some((target, declaration_scope)) = resolve_lexical_read_target(
                    root,
                    expression_scopes[index],
                    &scopes,
                    &declarations,
                ) {
                    reads[index] = Some(OwnerLexicalReadPlan {
                        target,
                        declaration_scope,
                        projection: projection.to_vec().into_boxed_slice(),
                        access,
                    });
                }
            }
            continue;
        };
        let reference = OwnerSymbolReference {
            expression: expression.stable_key.clone(),
            kind,
            parts: parts.clone(),
        };
        if kind == OwnerReferenceKind::Callable {
            external_candidates.insert(reference);
            continue;
        }
        let Some((root, projection)) = parts.split_first() else {
            external_candidates.insert(reference);
            continue;
        };
        if let Some((target, declaration_scope)) =
            resolve_lexical_read_target(root, expression_scopes[index], &scopes, &declarations)
        {
            reads[index] = Some(OwnerLexicalReadPlan {
                target,
                declaration_scope,
                projection: projection.to_vec().into_boxed_slice(),
                access: lexical_access(expression).unwrap_or(OwnerLexicalAccess::Read),
            });
        } else {
            external_candidates.insert(reference);
        }
    }
    let external_candidates = external_candidates.into_iter().collect::<Vec<_>>();
    let syntax_fingerprint_v1 = input.fingerprint_v1();
    let reads_fingerprint_v1 = fingerprint(
        OWNER_LEXICAL_READS_DOMAIN_V3,
        &(stable_check_owner_key_fingerprint_v1(&input.owner), &reads),
    )?;
    let signature_region_fingerprint_v1 = fingerprint(
        OWNER_SIGNATURE_REGION_INDEX_DOMAIN_V2,
        &(
            stable_check_owner_key_fingerprint_v1(&input.owner),
            &scopes,
            &stable_scopes,
            &expression_scopes,
            &stable_targets,
            containment.fingerprint_v1(),
        ),
    )?;
    let signature_regions = Arc::new(OwnerSignatureRegionIndex {
        scopes: Arc::from(scopes),
        stable_scopes: Arc::from(stable_scopes),
        expression_scopes: Arc::from(expression_scopes),
        stable_targets,
        containment,
        fingerprint_v1: signature_region_fingerprint_v1,
    });
    let fingerprint_v1 = fingerprint(
        OWNER_LEXICAL_PLAN_DOMAIN_V3,
        &(
            stable_check_owner_key_fingerprint_v1(&input.owner),
            syntax_fingerprint_v1,
            &statement_values,
            signature_region_fingerprint_v1,
            &reads,
            &record_scopes,
            &record_fields,
            &external_candidates,
        ),
    )?;
    let reads = Arc::<[Option<OwnerLexicalReadPlan>]>::from(reads);
    Ok(OwnerLexicalPlan {
        owner: input.owner.clone(),
        syntax_fingerprint_v1,
        graph,
        statement_values: statement_values.into_boxed_slice(),
        signature_regions,
        reads,
        record_scopes: record_scopes.into_boxed_slice(),
        record_fields: record_fields.into_boxed_slice(),
        external_candidates: external_candidates.into_boxed_slice(),
        reads_fingerprint_v1,
        fingerprint_v1,
    })
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
            OwnerConstraintNodeKind::Reference { parts }
        }
        AstExprKind::Path(path) => {
            let parts = path.clone().into_boxed_slice();
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
            if op == "WHILE" {
                push_edge(&mut inputs, OwnerConstraintEdgeRole::WhenInput, *input)?;
                for arm in arms {
                    push_edge(&mut inputs, OwnerConstraintEdgeRole::WhenArm, *arm)?;
                }
                return Ok(OwnerExpressionConstraint {
                    expression: expression.stable_key.clone(),
                    kind: OwnerConstraintNodeKind::When,
                    inputs: inputs.into_boxed_slice(),
                });
            }
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
            if let Some(selector) = expression.pattern_selector {
                push_edge(
                    &mut inputs,
                    OwnerConstraintEdgeRole::MatchSelector,
                    selector as usize,
                )?;
            }
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

fn pattern_arms_by_scope(
    scopes: &[OwnerLexicalScopePlan],
) -> Result<Vec<Box<[u32]>>, OwnerConstraintSeedError> {
    let mut resolved = vec![None::<Box<[u32]>>; scopes.len()];
    let mut state = vec![0u8; scopes.len()];
    for start in 0..scopes.len() {
        if resolved[start].is_some() {
            continue;
        }
        let mut path = Vec::new();
        let mut current = start;
        let inherited = loop {
            if let Some(arms) = resolved[current].as_ref() {
                break arms.to_vec();
            }
            if state[current] == 1 {
                return Err(OwnerConstraintSeedError::new(
                    "owner lexical scopes contain a parent cycle",
                ));
            }
            state[current] = 1;
            path.push(current);
            let Some(parent) = scopes[current].parent else {
                break Vec::new();
            };
            current = usize::try_from(parent)
                .ok()
                .filter(|parent| *parent < scopes.len())
                .ok_or_else(|| {
                    OwnerConstraintSeedError::new("owner lexical scope references a missing parent")
                })?;
        };
        let mut arms = inherited;
        for scope in path.into_iter().rev() {
            if let OwnerLexicalScopeOrigin::PatternArm { expression } = scopes[scope].origin {
                arms.push(expression);
            }
            resolved[scope] = Some(arms.clone().into_boxed_slice());
            state[scope] = 2;
        }
    }
    Ok(resolved
        .into_iter()
        .map(|arms| arms.unwrap_or_default())
        .collect())
}

fn narrowed_selector_read_matches_lexical_plan(
    lexical_plan: &OwnerLexicalPlan,
    selector: u32,
    projection: &[String],
    read: u32,
) -> bool {
    let selector = lexical_plan
        .reads()
        .get(selector as usize)
        .and_then(Option::as_ref);
    let read = lexical_plan
        .reads()
        .get(read as usize)
        .and_then(Option::as_ref);
    match selector {
        Some(selector)
            if !matches!(
                selector.target,
                OwnerLexicalDeclarationTarget::Ambiguous { .. }
            ) =>
        {
            read.is_some_and(|read| {
                read.target == selector.target
                    && read.projection.starts_with(&selector.projection)
                    && read.projection.len() == selector.projection.len() + projection.len()
                    && read.projection[selector.projection.len()..] == *projection
            })
        }
        // Base-plan `None` means a genuinely external or signature-overlay
        // candidate. Keep the legacy spelling relation only when the output
        // read is also unplanned; any static local target wins instead.
        None => read.is_none(),
        Some(_) => false,
    }
}

fn attach_pattern_binding_constraints(
    expressions: &mut [OwnerExpressionConstraint],
    lexical_plan: &OwnerLexicalPlan,
) -> Result<(), OwnerConstraintSeedError> {
    let mut bindings = BTreeMap::<u32, BTreeSet<(String, u32)>>::new();
    for (read, plan) in lexical_plan.reads().iter().enumerate() {
        let Some(OwnerLexicalReadPlan {
            target: OwnerLexicalDeclarationTarget::PatternBinding { arm, name },
            ..
        }) = plan
        else {
            continue;
        };
        bindings.entry(*arm).or_default().insert((
            name.clone(),
            checked_u32(read, "owner pattern binding read")?,
        ));
    }

    let mut selector_by_arm = BTreeMap::<u32, (u32, Box<[String]>)>::new();
    for (arm, expression) in expressions.iter().enumerate() {
        let OwnerConstraintNodeKind::MatchArm { pattern } = &expression.kind else {
            continue;
        };
        if !expression
            .inputs
            .iter()
            .any(|input| matches!(input.role, OwnerConstraintEdgeRole::MatchSelector))
        {
            continue;
        }
        if !matches!(
            pattern,
            OwnerPatternConstraint::Wildcard
                | OwnerPatternConstraint::Binding { .. }
                | OwnerPatternConstraint::Invalid
        ) && let Some(selector) = expression.inputs.iter().find_map(|input| {
            matches!(input.role, OwnerConstraintEdgeRole::MatchSelector).then_some(input.expression)
        }) && let Some(
            OwnerConstraintNodeKind::Reference { parts } | OwnerConstraintNodeKind::Drain { parts },
        ) = expressions
            .get(selector as usize)
            .map(|expression| &expression.kind)
        {
            selector_by_arm.insert(
                checked_u32(arm, "owner narrowed selector arm")?,
                (selector, parts.clone()),
            );
        }
    }

    let arms_by_scope = pattern_arms_by_scope(lexical_plan.scopes())?;
    let mut narrowed_selectors = BTreeMap::<u32, BTreeSet<(Vec<String>, u32)>>::new();
    for (read, expression) in expressions.iter().enumerate() {
        let (OwnerConstraintNodeKind::Reference { parts }
        | OwnerConstraintNodeKind::Drain { parts }) = &expression.kind
        else {
            continue;
        };
        let Some(scope) = lexical_plan.expression_scopes().get(read).copied() else {
            continue;
        };
        let Some(arms) = arms_by_scope.get(scope as usize) else {
            continue;
        };
        for arm in arms {
            let Some((selector, selector_parts)) = selector_by_arm.get(arm) else {
                continue;
            };
            if !parts.starts_with(selector_parts) {
                continue;
            }
            let projection = parts[selector_parts.len()..].to_vec();
            let read = checked_u32(read, "owner narrowed selector read")?;
            if narrowed_selector_read_matches_lexical_plan(
                lexical_plan,
                *selector,
                &projection,
                read,
            ) {
                narrowed_selectors
                    .entry(*arm)
                    .or_default()
                    .insert((projection, read));
            }
        }
    }

    for (arm, reads) in bindings {
        for (name, read) in reads {
            expressions[arm as usize].inputs = expressions[arm as usize]
                .inputs
                .iter()
                .cloned()
                .chain([OwnerConstraintEdge {
                    role: OwnerConstraintEdgeRole::MatchBinding { name },
                    expression: read,
                }])
                .collect::<Vec<_>>()
                .into_boxed_slice();
        }
    }
    for (arm, reads) in narrowed_selectors {
        expressions[arm as usize].inputs = expressions[arm as usize]
            .inputs
            .iter()
            .cloned()
            .chain(
                reads
                    .into_iter()
                    .map(|(projection, expression)| OwnerConstraintEdge {
                        role: OwnerConstraintEdgeRole::MatchNarrowedSelector {
                            projection: projection.into_boxed_slice(),
                        },
                        expression,
                    }),
            )
            .collect::<Vec<_>>()
            .into_boxed_slice();
    }
    Ok(())
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

fn statement_source_path_prefix(statement: &StableStatementKey) -> Vec<String> {
    let mut prefix = statement
        .route
        .owner
        .iter()
        .flat_map(|owner| owner.segments())
        .filter_map(|segment| {
            let name = segment.names.first()?;
            Some(match segment.kind {
                UnitItemKind::Function => format!("FUNCTION:{name}"),
                UnitItemKind::Field
                | UnitItemKind::Source
                | UnitItemKind::Hold
                | UnitItemKind::List => name.clone(),
            })
        })
        .collect::<Vec<_>>();
    prefix.extend(
        statement
            .route
            .statement_route
            .iter()
            .filter_map(|segment| {
                let name = segment.names.first()?;
                Some(match segment.kind {
                    StableStatementKind::Function => format!("FUNCTION:{name}"),
                    StableStatementKind::Field
                    | StableStatementKind::Source
                    | StableStatementKind::Hold
                    | StableStatementKind::List => name.clone(),
                    StableStatementKind::Block
                    | StableStatementKind::Spread
                    | StableStatementKind::Expression => return None,
                })
            }),
    );
    prefix
}

fn project_source_payload_queries(
    input: &OwnerSyntaxInput,
    statement_values: &[(u32, u32)],
    expressions: &[OwnerExpressionConstraint],
) -> Result<Box<[OwnerSourcePayloadQuery]>, OwnerConstraintSeedError> {
    fn visit(
        reference: u32,
        expressions: &[OwnerExpressionConstraint],
        prefix: &[String],
        projection: &mut Vec<String>,
        active: &mut BTreeSet<u32>,
        queries: &mut BTreeMap<StableExpressionKey, String>,
    ) -> Result<(), OwnerConstraintSeedError> {
        let Some(expression) = expressions.get(reference as usize) else {
            return Ok(());
        };
        if !active.insert(reference) {
            return Ok(());
        }
        // Literal SOURCE consumes its inferred host payload shape. Source-
        // emitting calls such as Timer/interval consume their type, mode, and
        // effect from the callable ABI; their source-route metadata belongs to
        // checked-owner construction rather than this inference ABI.
        if expression.kind == OwnerConstraintNodeKind::Source {
            let canonical_path = prefix
                .iter()
                .chain(projection.iter())
                .cloned()
                .collect::<Vec<_>>()
                .join(".");
            if !canonical_path.is_empty() {
                match queries.entry(expression.expression.clone()) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(canonical_path);
                    }
                    std::collections::btree_map::Entry::Occupied(entry)
                        if entry.get() == &canonical_path => {}
                    std::collections::btree_map::Entry::Occupied(entry) => {
                        return Err(OwnerConstraintSeedError::new(format!(
                            "source expression {:?} has conflicting stable paths `{}` and `{canonical_path}`",
                            expression.expression,
                            entry.get()
                        )));
                    }
                }
            }
        }
        for input in &expression.inputs {
            let projection_len = projection.len();
            if let OwnerConstraintEdgeRole::RecordField {
                name,
                spread: false,
            } = &input.role
            {
                projection.push(name.clone());
            }
            visit(
                input.expression,
                expressions,
                prefix,
                projection,
                active,
                queries,
            )?;
            projection.truncate(projection_len);
        }
        active.remove(&reference);
        Ok(())
    }

    let mut queries = BTreeMap::new();
    for (statement, expression) in statement_values {
        let statement = input
            .statements
            .get(*statement as usize)
            .ok_or_else(|| OwnerConstraintSeedError::new("source query statement is missing"))?;
        let prefix = statement_source_path_prefix(&statement.stable_key);
        visit(
            *expression,
            expressions,
            &prefix,
            &mut Vec::new(),
            &mut BTreeSet::new(),
            &mut queries,
        )?;
    }
    Ok(queries
        .into_iter()
        .map(|(expression, canonical_path)| OwnerSourcePayloadQuery {
            expression,
            canonical_path,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice())
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

pub fn project_owner_constraint_seed_with_lexical_plan(
    input: &OwnerSyntaxInput,
    lexical_plan: &OwnerLexicalPlan,
) -> Result<OwnerConstraintSeed, OwnerConstraintSeedError> {
    if lexical_plan.owner != input.owner
        || lexical_plan.syntax_fingerprint_v1 != input.fingerprint_v1()
    {
        return Err(OwnerConstraintSeedError::new(
            "owner constraint seed received a stale lexical plan",
        ));
    }
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

    let statement_values = lexical_plan.statement_values.to_vec();
    let mut expressions = input
        .expressions
        .iter()
        .map(project_expression)
        .collect::<Result<Vec<_>, _>>()?;
    attach_pattern_binding_constraints(&mut expressions, lexical_plan)?;
    let expression_flush_plans = project_expression_flush_plans(input, lexical_plan.graph());
    let source_payload_queries =
        project_source_payload_queries(input, &statement_values, &expressions)?;
    // Preserve the compact namespace order: expression edges address external
    // values after the local-expression range. `OwnerSyntaxInput` already
    // interns each referenced syntax expression exactly once.
    let external_expressions = input.external_expressions.to_vec();
    // The shared lexical projection is the only authority for whether a
    // syntax reference is local or external.
    let references = lexical_plan.external_candidates.to_vec();
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
        OWNER_CONSTRAINT_TOPOLOGY_DOMAIN_V2,
        &(
            stable_check_owner_key_fingerprint_v1(&input.owner),
            &declarations,
            &topology_references,
            &external_owners,
        ),
    )?;
    let fingerprint_v1 = fingerprint(
        OWNER_CONSTRAINT_SEED_DOMAIN_V4,
        &(
            stable_check_owner_key_fingerprint_v1(&input.owner),
            lexical_plan.reads_fingerprint_v1(),
            lexical_plan.signature_regions.fingerprint_v1(),
            &declarations,
            &statement_values,
            &expressions,
            &expression_flush_plans,
            &references,
            &external_expressions,
            &source_payload_queries,
            &result_static_numbers,
            effect_seed,
        ),
    )?;
    Ok(OwnerConstraintSeed {
        owner: input.owner.clone(),
        lexical_reads_fingerprint_v1: lexical_plan.reads_fingerprint_v1(),
        lexical_reads: Arc::clone(&lexical_plan.reads),
        signature_regions: Arc::clone(&lexical_plan.signature_regions),
        declarations: declarations.into_boxed_slice(),
        statement_values: statement_values.into_boxed_slice(),
        expressions: expressions.into_boxed_slice(),
        expression_flush_plans,
        references: references.into_boxed_slice(),
        external_expressions: external_expressions.into_boxed_slice(),
        source_payload_queries,
        result_static_numbers,
        effect_seed,
        fingerprint_v1,
        topology_fingerprint_v1,
    })
}

pub fn project_owner_constraint_seed(
    input: &OwnerSyntaxInput,
) -> Result<OwnerConstraintSeed, OwnerConstraintSeedError> {
    let lexical_plan = project_owner_lexical_plan(input)?;
    project_owner_constraint_seed_with_lexical_plan(input, &lexical_plan)
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
    fn declaration_surface_is_body_independent() {
        let before = link("FUNCTION identity(input) {\n    input\n}\n");
        let after = link("FUNCTION identity(input) {\n    input + 0\n}\n");
        let owner = owner_named(&before, "identity");
        let changed_owner = owner_named(&after, "identity");
        let before =
            project_owner_syntax_input(before.owner_view_for_key(&owner).unwrap()).unwrap();
        let after =
            project_owner_syntax_input(after.owner_view_for_key(&changed_owner).unwrap()).unwrap();
        let before = project_owner_declaration_surface(&before).unwrap();
        let after = project_owner_declaration_surface(&after).unwrap();

        assert_eq!(before, after);
        assert_eq!(before.public().unwrap().parameters[0].name, "input");
    }

    #[test]
    fn lexical_plan_predeclares_block_bindings_for_the_whole_scope() {
        let unit = link(concat!(
            "FUNCTION calculate(input) {\n",
            "    BLOCK {\n",
            "        result: doubled + input\n",
            "        doubled: input * 2\n",
            "        result\n",
            "    }\n",
            "}\n",
        ));
        let owner = owner_named(&unit, "calculate");
        let syntax = project_owner_syntax_input(unit.owner_view_for_key(&owner).unwrap()).unwrap();
        let plan = project_owner_lexical_plan(&syntax).unwrap();

        assert!(
            plan.external_candidates()
                .iter()
                .all(|reference| reference.kind != OwnerReferenceKind::Value)
        );
        let mut saw_forward = false;
        let mut saw_parameter = false;
        for (expression, read) in syntax.expressions.iter().zip(plan.reads()) {
            let Some(read) = read else { continue };
            let root = match &expression.kind {
                AstExprKind::Identifier(name) => Some(name.as_str()),
                AstExprKind::Path(parts) => parts.first().map(String::as_str),
                _ => None,
            };
            match (root, &read.target) {
                (Some("doubled"), OwnerLexicalDeclarationTarget::Statement { .. }) => {
                    saw_forward = true;
                }
                (Some("input"), OwnerLexicalDeclarationTarget::Parameter { ordinal: 0 }) => {
                    saw_parameter = true;
                }
                _ => {}
            }
        }
        assert!(saw_forward, "forward BLOCK read must bind locally");
        assert!(saw_parameter, "parameter reads must bind locally");
    }

    #[test]
    fn wide_block_children_share_one_authored_binding_environment() {
        let unit = link(concat!(
            "container: BLOCK {\n",
            "    first: 1\n",
            "    second: 2\n",
            "    third: 3\n",
            "    third\n",
            "}\n",
        ));
        let owner = owner_named(&unit, "container");
        let syntax = project_owner_syntax_input(unit.owner_view_for_key(&owner).unwrap()).unwrap();
        let plan = project_owner_lexical_plan(&syntax).unwrap();
        let children = plan
            .signature_regions()
            .containment()
            .children
            .iter()
            .filter(|child| child.inherits_lexical_environment())
            .collect::<Vec<_>>();

        assert!(
            children.len() >= 3,
            "wide BLOCK must retain its child owners"
        );
        assert!(children[0].bindings.len() >= 3);
        assert!(children.iter().skip(1).all(|child| {
            OwnerLexicalBoundaryBindings::ptr_eq(&children[0].bindings, &child.bindings)
        }));

        let copied =
            OwnerLexicalBoundaryBindings::try_new(children[0].bindings.iter().cloned().collect())
                .unwrap();
        assert_eq!(
            copied.fingerprint_v1(),
            children[0].bindings.fingerprint_v1(),
            "equal binding environments must have one content identity",
        );
        let mut changed_rows = children[0].bindings.iter().cloned().collect::<Vec<_>>();
        changed_rows[0].name.push_str("_changed");
        let changed = OwnerLexicalBoundaryBindings::try_new(changed_rows).unwrap();
        assert_ne!(
            changed.fingerprint_v1(),
            children[0].bindings.fingerprint_v1(),
            "binding content changes must invalidate the shared environment identity",
        );
    }

    #[test]
    fn lexical_plan_binds_function_values_in_their_declaring_scope() {
        let unit = link(concat!("FUNCTION repeat(input) {\n", "    repeat\n", "}\n",));
        let owner = owner_named(&unit, "repeat");
        let syntax = project_owner_syntax_input(unit.owner_view_for_key(&owner).unwrap()).unwrap();
        let plan = project_owner_lexical_plan(&syntax).unwrap();

        assert!(
            syntax
                .expressions
                .iter()
                .zip(plan.reads())
                .any(|(expression, read)| {
                    matches!(&expression.kind, AstExprKind::Identifier(name) if name == "repeat")
                        && matches!(
                            read.as_ref().map(|read| &read.target),
                            Some(OwnerLexicalDeclarationTarget::Statement { statement: 0 })
                        )
                })
        );
        assert!(
            plan.external_candidates()
                .iter()
                .all(|reference| reference.parts.as_ref() != ["repeat"])
        );
    }

    #[test]
    fn lexical_plan_preserves_record_self_reference_and_spread_non_binding() {
        let unit = link(concat!(
            "FUNCTION merge(base) {\n",
            "    [...base, item: item]\n",
            "}\n",
        ));
        let owner = owner_named(&unit, "merge");
        let syntax = project_owner_syntax_input(unit.owner_view_for_key(&owner).unwrap()).unwrap();
        let plan = project_owner_lexical_plan(&syntax).unwrap();

        let mut saw_spread_parameter = false;
        let mut saw_self_reference = false;
        for (expression, read) in syntax.expressions.iter().zip(plan.reads()) {
            let Some(read) = read else { continue };
            let root = match &expression.kind {
                AstExprKind::Identifier(name) => Some(name.as_str()),
                AstExprKind::Path(parts) => parts.first().map(String::as_str),
                _ => None,
            };
            match (root, &read.target) {
                (Some("base"), OwnerLexicalDeclarationTarget::Parameter { ordinal: 0 }) => {
                    saw_spread_parameter = true;
                }
                (Some("item"), OwnerLexicalDeclarationTarget::RecordField { .. }) => {
                    saw_self_reference = true;
                }
                _ => {}
            }
        }
        assert!(saw_spread_parameter, "record spread must read its input");
        assert!(
            saw_self_reference,
            "record field must shadow itself: syntax={:#?}, scopes={:#?}, reads={:#?}",
            syntax,
            plan.expression_scopes(),
            plan.reads()
        );
        assert!(
            plan.external_candidates()
                .iter()
                .all(|reference| reference.parts.as_ref() != ["base"]
                    && reference.parts.as_ref() != ["item"])
        );
    }

    #[test]
    fn lexical_plan_limits_pattern_bindings_to_the_arm_output() {
        let unit = link(concat!(
            "FUNCTION unwrap(value) {\n",
            "    value |> WHEN { Found[item] => item }\n",
            "}\n",
        ));
        let owner = owner_named(&unit, "unwrap");
        let syntax = project_owner_syntax_input(unit.owner_view_for_key(&owner).unwrap()).unwrap();
        let plan = project_owner_lexical_plan(&syntax).unwrap();

        assert!(
            syntax
                .expressions
                .iter()
                .zip(plan.reads())
                .any(|(expression, read)| {
                    matches!(&expression.kind, AstExprKind::Identifier(name) if name == "item")
                        && matches!(
                            read.as_ref().map(|read| &read.target),
                            Some(OwnerLexicalDeclarationTarget::PatternBinding { name, .. })
                                if name == "item"
                        )
                })
        );
        assert!(
            plan.external_candidates()
                .iter()
                .all(|reference| reference.parts.as_ref() != ["item"])
        );
    }

    #[test]
    fn pattern_binding_constraints_follow_exact_lexical_targets() {
        for source in [
            concat!(
                "FUNCTION record_shadow(value) {\n",
                "    value |> WHEN { Found[item] => [copy: item, item: 1] }\n",
                "}\n",
            ),
            concat!(
                "FUNCTION block_shadow(value) {\n",
                "    value |> WHEN {\n",
                "        Found[x] => BLOCK {\n",
                "            y: x\n",
                "            x: 1\n",
                "            y\n",
                "        }\n",
                "    }\n",
                "}\n",
            ),
        ] {
            let unit = link(source);
            let owner = unit
                .stable_check_owner_keys()
                .find(|owner| matches!(owner, StableCheckOwnerKey::Item(_)))
                .unwrap();
            let syntax =
                project_owner_syntax_input(unit.owner_view_for_key(&owner).unwrap()).unwrap();
            let plan = project_owner_lexical_plan(&syntax).unwrap();
            let seed = project_owner_constraint_seed_with_lexical_plan(&syntax, &plan).unwrap();

            let shadowed_reads = plan
                .reads()
                .iter()
                .enumerate()
                .filter_map(|(read, plan)| {
                    matches!(
                        plan.as_ref().map(|plan| &plan.target),
                        Some(OwnerLexicalDeclarationTarget::Statement { .. })
                            | Some(OwnerLexicalDeclarationTarget::RecordField { .. })
                    )
                    .then_some(read as u32)
                })
                .collect::<BTreeSet<_>>();
            assert!(
                !shadowed_reads.is_empty(),
                "source must exercise local shadowing"
            );

            for (arm, expression) in seed.expressions.iter().enumerate() {
                for input in &expression.inputs {
                    let OwnerConstraintEdgeRole::MatchBinding { name } = &input.role else {
                        continue;
                    };
                    assert!(!shadowed_reads.contains(&input.expression));
                    assert!(matches!(
                        plan.reads()[input.expression as usize]
                            .as_ref()
                            .map(|plan| &plan.target),
                        Some(OwnerLexicalDeclarationTarget::PatternBinding {
                            arm: target_arm,
                            name: target_name,
                        }) if *target_arm as usize == arm && target_name == name
                    ));
                }
            }
        }
    }

    #[test]
    fn narrowed_selector_constraints_do_not_override_exact_local_targets() {
        for source in [
            concat!(
                "FUNCTION record_shadow(value) {\n",
                "    value |> WHEN { Found[item] => [copy: value, value: 1] }\n",
                "}\n",
            ),
            concat!(
                "FUNCTION block_shadow(value) {\n",
                "    value |> WHEN {\n",
                "        Found[item] => BLOCK {\n",
                "            copy: value\n",
                "            value: 1\n",
                "            copy\n",
                "        }\n",
                "    }\n",
                "}\n",
            ),
        ] {
            let unit = link(source);
            let owner = unit
                .stable_check_owner_keys()
                .find(|owner| matches!(owner, StableCheckOwnerKey::Item(_)))
                .unwrap();
            let syntax =
                project_owner_syntax_input(unit.owner_view_for_key(&owner).unwrap()).unwrap();
            let plan = project_owner_lexical_plan(&syntax).unwrap();
            let seed = project_owner_constraint_seed_with_lexical_plan(&syntax, &plan).unwrap();
            let shadowed_reads = plan
                .reads()
                .iter()
                .enumerate()
                .filter_map(|(read, plan)| {
                    matches!(
                        plan.as_ref().map(|plan| &plan.target),
                        Some(OwnerLexicalDeclarationTarget::Statement { .. })
                            | Some(OwnerLexicalDeclarationTarget::RecordField { .. })
                    )
                    .then_some(read as u32)
                })
                .collect::<BTreeSet<_>>();
            assert!(
                !shadowed_reads.is_empty(),
                "source must exercise local shadowing"
            );

            let narrowed_reads = seed
                .expressions
                .iter()
                .flat_map(|expression| &expression.inputs)
                .filter_map(|input| {
                    matches!(
                        input.role,
                        OwnerConstraintEdgeRole::MatchNarrowedSelector { .. }
                    )
                    .then_some(input.expression)
                })
                .collect::<BTreeSet<_>>();
            assert!(
                shadowed_reads.is_disjoint(&narrowed_reads),
                "whole-scope locals must not become selector-narrowing reads"
            );
        }
    }

    #[test]
    fn lexical_plan_parents_nested_inline_pattern_scopes_structurally() {
        let unit = link(concat!(
            "FUNCTION nested(value) {\n",
            "    value |> WHEN { Outer[a] => a |> WHEN { Inner[b] => b } }\n",
            "}\n",
        ));
        let owner = owner_named(&unit, "nested");
        let syntax = project_owner_syntax_input(unit.owner_view_for_key(&owner).unwrap()).unwrap();
        let plan = project_owner_lexical_plan(&syntax).unwrap();

        let pattern_scope = |tag: &str| {
            syntax
                .expressions
                .iter()
                .enumerate()
                .find_map(|(index, expression)| match &expression.kind {
                    AstExprKind::MatchArm {
                        pattern: AstMatchPattern::Tag { name, .. },
                        ..
                    } if name == tag => Some(plan.expression_scopes()[index]),
                    _ => None,
                })
                .unwrap()
        };
        let outer = pattern_scope("Outer");
        let inner = pattern_scope("Inner");
        assert_ne!(outer, inner);
        assert_eq!(plan.scopes()[inner as usize].parent, Some(outer));
        assert!(
            plan.external_candidates()
                .iter()
                .all(|reference| reference.kind != OwnerReferenceKind::Value)
        );
        assert!(plan.reads().iter().flatten().any(|read| {
            matches!(
                &read.target,
                OwnerLexicalDeclarationTarget::PatternBinding { name, .. } if name == "a"
            )
        }));
        assert!(plan.reads().iter().flatten().any(|read| {
            matches!(
                &read.target,
                OwnerLexicalDeclarationTarget::PatternBinding { name, .. } if name == "b"
            )
        }));
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
    fn while_is_a_control_constraint_without_a_callable_reference() {
        let unit = link(concat!(
            "value: True |> WHILE {\n",
            "    True => 1\n",
            "    False => 2\n",
            "}\n",
        ));
        let owner = owner_named(&unit, "value");
        let seed = summary(&unit, &owner);

        assert!(seed.references.iter().all(|reference| {
            reference.kind != OwnerReferenceKind::Callable || reference.parts.as_ref() != ["WHILE"]
        }));
        let while_constraint = seed
            .expressions
            .iter()
            .find(|expression| {
                matches!(expression.kind, OwnerConstraintNodeKind::When)
                    && expression
                        .inputs
                        .iter()
                        .any(|input| matches!(input.role, OwnerConstraintEdgeRole::WhenInput))
            })
            .expect("WHILE control constraint");
        assert_eq!(
            while_constraint
                .inputs
                .iter()
                .filter(|input| matches!(input.role, OwnerConstraintEdgeRole::WhenArm))
                .count(),
            2
        );
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
            callable.clone(),
            caller.clone(),
            OwnerConstraintDependencyKind::PassedContext,
            None,
        )));

        let callable_summary = resolve_owner_constraint_seed(&callable_seed, []).unwrap();
        let topology = build_owner_interface_topology([&callable_summary, &resolved]).unwrap();
        assert_eq!(topology.stats.nodes, 2);
        assert_eq!(topology.stats.components, 2);
        assert_eq!(topology.stats.cyclic_components, 0);
        let callable_scc = topology.scc_for_owner(&callable).unwrap();
        let caller_scc = topology.scc_for_owner(&caller).unwrap();
        assert!(callable_scc.dependencies.is_empty());
        assert_eq!(caller_scc.dependencies.as_ref(), [callable_scc.key.clone()]);
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

    #[test]
    fn source_payload_queries_use_stable_owner_routes_and_record_projections() {
        let unit = link("controls: [events: [press: SOURCE]]\n");
        let controls = unit
            .stable_check_owner_keys()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().iter().any(|segment| {
                            segment.names.iter().any(|name| name == "controls")
                        })
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "controls owner missing from {:#?}",
                    unit.stable_check_owner_keys().collect::<Vec<_>>()
                )
            });
        let seed = summary(&unit, &controls);

        assert_eq!(seed.source_payload_queries.len(), 1);
        assert_eq!(
            seed.source_payload_queries[0].canonical_path,
            "controls.events.press"
        );
        assert_eq!(
            seed.source_payload_abi_paths().as_ref(),
            ["controls.events.press"]
        );
    }

    #[test]
    fn interval_source_calls_do_not_create_payload_inference_queries() {
        let unit = link("clock: [tick: Duration[milliseconds: 16] |> Timer/interval()]\n");
        let clock = unit
            .stable_check_owner_keys()
            .find(|owner| {
                matches!(
                    owner,
                    StableCheckOwnerKey::Item(owner)
                        if owner.item_route.segments().iter().any(|segment| {
                            segment.names.iter().any(|name| name == "clock")
                        })
                )
            })
            .unwrap();
        let seed = summary(&unit, &clock);

        assert!(seed.source_payload_queries.is_empty());
        assert!(matches!(
            seed.expressions
                .iter()
                .find(|expression| matches!(
                    &expression.kind,
                    OwnerConstraintNodeKind::Pipe { operation }
                        if operation == "Timer/interval"
                ))
                .map(|expression| &expression.kind),
            Some(OwnerConstraintNodeKind::Pipe { operation })
                if operation == "Timer/interval"
        ));
    }
}
