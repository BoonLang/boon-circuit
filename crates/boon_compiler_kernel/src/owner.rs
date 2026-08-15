use crate::{
    ComponentArtifact, ComponentProgram, ComponentProgramBuilder, KernelCollectionOperationKind,
    KernelPattern, KernelRecordEntry, KernelSelectArm, KernelSolveError, KernelSolveWork,
    KernelSummaryCallInput, KernelSummaryNode, KernelSummaryProgram, KernelSummaryProjectionStep,
    KernelSummaryRecordEntry, KernelSummarySelectArm, KernelSummaryValueId, OutputId, PublishMode,
    TypeTerm, TypeTermId, TypeVariableId, VariantTerm,
    alpha_normalize_callable_interface_and_diagnostics, alpha_normalize_definition,
    build_snapshot_receipts, definition_basis_fingerprint,
    definition_basis_fingerprint_with_buffer, solve_component,
};
use boon_checked::{
    BytesType, CheckedListKeyPolicy, CheckedStateKind, FlowMode, FlowType, ObjectShape, Type,
    Variant, type_is_recursively_closed,
};
use boon_data::{Bits, ExactNumber, ExactNumberParseReason, ExactRoundingRule};
use boon_effect_schema::{
    BarrierSpec, DeliveryCardinalitySpec, ReplaySpec, ResultPolicySpec, ValueType, host_effect_spec,
};
use boon_syntax::{AstExpr, AstExprKind, AstMatchPattern, StableExpressionKey, StableStatementKey};
use serde::Serialize;
use serde::ser::SerializeStruct;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct KernelExpressionId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct KernelStatementId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct KernelDeclarationId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct KernelSourceId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct KernelStateId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct KernelListId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct KernelOwnerId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelInheritedFormal {
    pub target_ordinal: u32,
    pub caller_ordinal: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelCollectionKind {
    List,
    Bytes,
    Set,
    Map,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelRenderConstructorKind {
    Fixed(Box<str>),
    StripeDirection,
}

/// A source-level pure ABI call compiled away before the work queue runs.
///
/// These variants describe result and requirement equations, not runtime
/// implementations. The residual program therefore contains no function-name
/// dispatch or generic ABI edge search.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelPureBuiltinKind {
    TextConstant,
    TextTransform,
    TextSlice,
    TextLength,
    TextConcat,
    TextPredicate,
    TextToNumber,
    NumberToText,
    NumberMath,
    NumberRound,
    NumberProjection,
    Boolean,
    /// A pure record constructor whose result fields are its named inputs.
    /// This covers ABI constructors such as the Light family without baking
    /// library-specific record layouts into the type engine.
    RecordConstructor,
    ListLength,
    ListPredicate,
    ListFilter,
    ListMap,
    ListFind,
    ListLatest,
    ListAppend,
    ListSort,
    ListChunk,
    TextJoin,
    FieldColor,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelOwnerNodeKind {
    /// A closed ABI value supplied at the kernel boundary (for example a
    /// SOURCE payload contract). It is imported once into the type DAG.
    Known(Type),
    /// A source occurrence with its closed payload ABI. This solves exactly
    /// like a known value while remaining explicit in the checked artifact.
    Source(Type),
    Absent,
    Text,
    TextTemplate,
    Number,
    Byte,
    Bits(u32),
    Tag(Box<str>),
    Record {
        tag: Option<Box<str>>,
    },
    Block,
    Collection {
        kind: KernelCollectionKind,
        capacity: Option<usize>,
    },
    MapEntry,
    /// One detached occurrence read from an invocation-local formal.
    FormalRead {
        formal: u32,
        fields: Box<[Box<str>]>,
    },
    /// A detached read from the invocation's inherited `PASSED` provider.
    /// Context is a capture-forward channel: unlike an ordinary value formal,
    /// consumer requirements never reshape the captured/public provider.
    ContextRead {
        formal: u32,
        fields: Box<[Box<str>]>,
    },
    /// A same-owner lexical alias. Unlike a cross-owner read, its occurrence
    /// remains an equality participant and can carry requirements back to the
    /// BLOCK binding that declared it.
    LexicalRead {
        fields: Box<[Box<str>]>,
    },
    /// A detached occurrence read from another owner's public provider.
    /// The exact provider is carried by the node's single `ReadProvider` edge.
    ValueRead {
        fields: Box<[Box<str>]>,
        /// A local selector expression whose tag match proves this nested
        /// projection belongs to the selected variant. Its mode, rather than
        /// an aggregate projection through every retained branch, owns the
        /// occurrence mode inside that match arm.
        mode_narrowing: Option<KernelExpressionId>,
    },
    /// A detached occurrence projected from an owner-local derived authority,
    /// such as a match payload or contextual collection binding.
    DerivedRead {
        fields: Box<[Box<str>]>,
    },
    /// A detached payload read owned by one authored match pattern. Unlike a
    /// generic object projection, this preserves the enclosing tag while an
    /// open formal is shaped and narrows a closed variant provider to the
    /// selected arm before projecting its payload.
    PatternRead {
        pattern: KernelPattern,
        fields: Box<[Box<str>]>,
    },
    /// One contextual collection callback binding, projected directionally
    /// from the input collection without coalescing producer and consumer.
    CollectionItemRead,
    /// One private compile-time output port created by a bare `OUT` call
    /// entry. The call frame, not this occurrence, supplies its producer.
    FreshOut,
    /// A user-call occurrence. Acyclic targets are composed into this
    /// component with a fresh formal frame during compilation.
    UserCall {
        target: KernelOwnerId,
        inherited_formal: Option<KernelInheritedFormal>,
    },
    RenderConstructor {
        kind: KernelRenderConstructorKind,
    },
    PureBuiltin {
        kind: KernelPureBuiltinKind,
    },
    /// A host call whose type and execution policies come from the stable
    /// lower-level effect-schema registry. The operation name is the ABI key;
    /// no legacy callable graph is consulted.
    HostEffect {
        operation: Box<str>,
    },
    Latest,
    When,
    Then,
    Infix {
        operation: Box<str>,
    },
    Draining,
    Hold,
    MatchArm {
        pattern: KernelPattern,
    },
    Arrow,
    Delimiter,
    Unknown,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelOwnerEdgeRole {
    RecordField {
        name: Box<str>,
        spread: bool,
    },
    /// A dynamic interpolation consumed while producing a `TextTemplate`.
    ///
    /// The interpolation does not constrain the template's result type, but
    /// it is still an authored dependency and therefore remains in the dense
    /// owner graph for reachability, currentness, and artifact provenance.
    TextDynamic,
    BlockResult,
    CollectionItem,
    MapEntry,
    MapKey,
    MapValue,
    ReadProvider,
    CallArgument {
        ordinal: u32,
    },
    /// An `OUT` call edge aliases a producer capability rather than reading
    /// the argument expression as an ordinary value occurrence.
    CallOutArgument {
        ordinal: u32,
    },
    AbiArgument {
        name: Box<str>,
    },
    LatestBranch,
    WhenInput,
    WhenArm,
    ThenInput,
    ThenOutput,
    InfixLeft,
    InfixRight,
    DrainingInput,
    HoldInitial,
    HoldUpdate,
    MatchOutput,
    ArrowOutput,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelOwnerInputEdge {
    pub role: KernelOwnerEdgeRole,
    pub expression: KernelExpressionId,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelOwnerNode {
    pub kind: KernelOwnerNodeKind,
    pub inputs: Box<[KernelOwnerInputEdge]>,
    pub mode: FlowMode,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelOwnerProgramInput {
    pub nodes: Box<[KernelOwnerNode]>,
    /// Invocation-local parameter/context slots consumed by `FormalRead`.
    pub formal_count: u32,
    /// Compact namespace suffix referenced after the local node range.
    pub external_expressions: Box<[KernelExternalExpression]>,
    pub result: KernelExpressionId,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelParameterKind {
    Value,
    Out,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Serialize)]
pub enum KernelParameterEvaluationScope {
    #[default]
    Parent,
    Output {
        parameter_ordinal: u32,
    },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelStatementParameter {
    pub name: Box<str>,
    pub kind: KernelParameterKind,
    pub ordinal: u32,
    pub evaluation_scope: KernelParameterEvaluationScope,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelStatementKind {
    Function {
        name: Box<str>,
        parameters: Box<[KernelStatementParameter]>,
    },
    Field {
        name: Box<str>,
    },
    Source {
        field: Option<Box<str>>,
        event: Option<Box<str>>,
    },
    Hold {
        field: Option<Box<str>>,
        name: Option<Box<str>>,
    },
    List {
        field: Option<Box<str>>,
        capacity: Option<usize>,
    },
    Block,
    Spread,
    Expression,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelStatementInput {
    pub id: KernelStatementId,
    pub kind: KernelStatementKind,
    pub value: Option<KernelExpressionId>,
    pub children: Box<[KernelStatementChildReference]>,
}

/// Stable-within-definition structural origin of one declaration row.
///
/// The dense declaration ID is intentionally revision-local. The origin is
/// expressed only through other definition-local IDs so a linker can relocate
/// it without retaining parser arenas, byte offsets, or legacy checker IDs.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum KernelDeclarationOrigin {
    Statement {
        statement: KernelStatementId,
    },
    Parameter {
        statement: KernelStatementId,
        ordinal: u32,
    },
    RecordField {
        object: KernelExpressionId,
        ordinal: u32,
    },
    PatternBinding {
        arm: KernelExpressionId,
        ordinal: u32,
    },
    CallbackBinding {
        call: KernelExpressionId,
        ordinal: u32,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum KernelDeclarationKind {
    Function,
    ValueParameter,
    OutParameter,
    Field,
    Source,
    Hold,
    List,
    PatternBinding,
    FreshOut,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelDeclarationInput {
    pub id: KernelDeclarationId,
    pub origin: KernelDeclarationOrigin,
    pub name: Box<str>,
    pub kind: KernelDeclarationKind,
    pub value: Option<KernelExpressionId>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelDeclarationReference {
    Local(KernelDeclarationId),
    /// The unique public declaration exported by another dense definition.
    OwnerPublic(KernelOwnerId),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelStatementReference {
    Local(KernelStatementId),
    /// The public/root statement exported by another dense definition.
    OwnerPublic(KernelOwnerId),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelLexicalBindingTargetInput {
    Declaration(KernelDeclarationReference),
    ContextFormal {
        ordinal: u32,
    },
    /// A definition-local or external value authority without an authored
    /// declaration (for example a contextual callback item).
    Value {
        provider: KernelExpressionId,
    },
    RuntimeContext,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelLexicalAccess {
    Read,
    Drain,
}

/// One occurrence-to-authority lexical equation.
///
/// The solved expression row still owns its type equation. This row owns the
/// declaration identity and authored projection so later state/resource and
/// diagnostics construction never has to rediscover lexical resolution.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelLexicalBindingInput {
    pub expression: KernelExpressionId,
    pub target: KernelLexicalBindingTargetInput,
    pub projection: Box<[Box<str>]>,
    pub access: KernelLexicalAccess,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelSemanticPath {
    pub anchor: KernelDeclarationReference,
    pub projection: Box<[Box<str>]>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelSourceInput {
    pub id: KernelSourceId,
    pub declaration: KernelDeclarationReference,
    pub statement: KernelStatementReference,
    pub expression: KernelExpressionId,
    pub projection: Box<[Box<str>]>,
    pub interval_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelStateInput {
    pub id: KernelStateId,
    pub binding_declaration: KernelDeclarationReference,
    pub declaration: KernelDeclarationReference,
    pub statement: KernelStatementReference,
    pub expression: KernelExpressionId,
    pub initial: KernelExpressionId,
    pub projection: Box<[Box<str>]>,
    pub kind: CheckedStateKind,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelListInput {
    pub id: KernelListId,
    pub declaration: KernelDeclarationReference,
    pub statement: KernelStatementReference,
    pub producer: KernelExpressionId,
    pub projection: Box<[Box<str>]>,
    pub capacity: Option<usize>,
    pub key_policy: CheckedListKeyPolicy,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelStatementChildReference {
    Local(KernelStatementId),
    Owner(KernelOwnerId),
}

/// Stable parser-owned identities for every dense expression and statement in
/// one definition.
///
/// These are linker relocations, not type-solver inputs. Keeping them beside
/// the normalized definition facts lets a checked-image or semantic linker
/// resolve exact source rows in one dense pass without retaining parser arena
/// IDs or rediscovering owner structure after solving.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq, Serialize)]
pub struct KernelDefinitionRelocations {
    pub expressions: Box<[KernelExpressionRelocation]>,
    pub statements: Box<[StableStatementKey]>,
}

/// Exact source/linker identity of one dense expression row.
///
/// A structural owner whose public value is composed exclusively from child
/// owners has no authored parser expression for that aggregate. Such a row is
/// an explicit synthetic definition result; it must never be assigned a fake
/// source key or accidentally presented as authored syntax.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum KernelExpressionRelocation {
    Authored(StableExpressionKey),
    SyntheticDefinitionResult,
}

impl KernelDefinitionRelocations {
    pub fn is_empty(&self) -> bool {
        self.expressions.is_empty() && self.statements.is_empty()
    }

    pub fn is_complete_for(&self, expression_count: usize, statement_count: usize) -> bool {
        self.expressions.len() == expression_count && self.statements.len() == statement_count
    }
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq, Serialize)]
pub struct KernelDefinitionFactsInput {
    pub relocations: KernelDefinitionRelocations,
    pub statements: Box<[KernelStatementInput]>,
    pub declarations: Box<[KernelDeclarationInput]>,
    pub lexical_bindings: Box<[KernelLexicalBindingInput]>,
    pub sources: Box<[KernelSourceInput]>,
    pub states: Box<[KernelStateInput]>,
    pub lists: Box<[KernelListInput]>,
    /// Typed failures projected directly from immutable syntax and resolved
    /// call contracts. These facts are definition-local and require neither
    /// type-solve replay nor checked-row materialization.
    pub diagnostics: Box<[KernelDiagnosticInput]>,
    /// Exact values whose solved types are needed only to evaluate compiler
    /// diagnostics. They are projected without constructing expression rows.
    pub diagnostic_values: Box<[KernelExpressionId]>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelExternalExpression {
    pub owner: KernelOwnerId,
    pub target: KernelExternalTarget,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelExternalTarget {
    Expression(KernelExpressionId),
    Result,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelProjectProgramInput {
    pub owners: Box<[KernelOwnerProgramInput]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelOwnerBuildError {
    message: String,
}

impl KernelOwnerBuildError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for KernelOwnerBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for KernelOwnerBuildError {}

#[derive(Debug)]
pub struct KernelOwnerProgram {
    component: ComponentProgram,
    result_output: OutputId,
    formal_outputs: Box<[OutputId]>,
    formal_modes: Box<[FlowMode]>,
    expression_outputs: Box<[OutputId]>,
    expression_modes: Box<[FlowMode]>,
    expression_artifacts: Box<[PendingKernelExpressionArtifact]>,
    relocations: KernelDefinitionRelocations,
    statements: Box<[KernelStatementArtifact]>,
    declarations: Box<[KernelDeclarationArtifact]>,
    lexical_bindings: Box<[KernelLexicalBindingArtifact]>,
    resources: PendingKernelResources,
    calls: Box<[PendingKernelCallArtifact]>,
    effects: Box<[KernelHostEffectArtifact]>,
    diagnostics: Box<[KernelDiagnosticArtifact]>,
    basis_fingerprint_v4: [u8; 32],
}

#[derive(Debug)]
pub struct KernelProjectProgram {
    component: ComponentProgram,
    owners: Box<[KernelProjectOwnerOutputs]>,
    compile_work: KernelCompileWork,
}

/// Quiescent project graph before any optional checked product is published.
///
/// Keeping this boundary explicit lets one session solve type equations once,
/// answer diagnostics from public interfaces, and materialize sparse or full
/// checked artifacts only when a later demand requires them.
#[derive(Clone, Debug)]
pub struct KernelSolvedProject {
    artifact: ComponentArtifact,
    owners: Box<[KernelProjectOwnerOutputs]>,
    public_results: Box<[FlowType]>,
    public_formals: Box<[Box<[FlowType]>]>,
    call_facts: Box<[Box<[SolvedKernelCallFacts]>]>,
    diagnostics: Box<[Box<[KernelDiagnosticArtifact]>]>,
}

pub const KERNEL_RESIDUAL_MODULE_RANKING_LEN: usize = 16;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelResidualModuleWork {
    pub owner: u32,
    pub operations: u32,
    pub frames: u32,
    pub linked_operations: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelCompileWork {
    pub definition_modules: u64,
    pub principal_expressions: u64,
    pub residual_type_modules: u64,
    pub residual_module_operations: u64,
    pub residual_module_terms: u64,
    pub residual_frames: u64,
    pub linked_operations: u64,
    pub scheduled_work_items: u64,
    pub acyclic_residual_frames: u64,
    pub dominant_module_owner: u64,
    pub dominant_module_operations: u64,
    pub dominant_module_frames: u64,
    pub dominant_module_linked_operations: u64,
    pub residual_module_ranking: [KernelResidualModuleWork; KERNEL_RESIDUAL_MODULE_RANKING_LEN],
    pub linked_terms: u64,
    pub acyclic_initial_operations: u64,
    pub compiled_call_sites: u64,
    pub invocation_frames: u64,
    pub reused_invocation_frames: u64,
    pub direct_result_summaries: u64,
    pub summary_definition_nodes: u64,
    pub summary_constant_folded_nodes: u64,
    pub summary_selector_fused_records: u64,
    pub summary_deduplicated_nodes: u64,
    pub summary_pruned_nodes: u64,
    pub summary_pruned_inputs: u64,
    pub summary_invoke_nodes: u64,
    pub principal_result_reuses: u64,
    pub principal_expression_reuses: u64,
    pub pruned_invocation_expressions: u64,
    pub specialization_plans: u64,
    pub reused_specialization_plans: u64,
    pub max_call_depth: u64,
}

#[derive(Clone, Debug)]
struct KernelProjectOwnerOutputs {
    result: OutputId,
    formals: Box<[OutputId]>,
    formal_modes: Box<[FlowMode]>,
    expressions: Box<[OutputId]>,
    expression_modes: Box<[FlowMode]>,
    expression_artifacts: Box<[PendingKernelExpressionArtifact]>,
    relocations: KernelDefinitionRelocations,
    statements: Box<[KernelStatementArtifact]>,
    declarations: Box<[KernelDeclarationArtifact]>,
    lexical_bindings: Box<[KernelLexicalBindingArtifact]>,
    resources: PendingKernelResources,
    calls: Box<[PendingKernelCallArtifact]>,
    effects: Box<[KernelHostEffectArtifact]>,
    diagnostics: Box<[KernelDiagnosticArtifact]>,
    diagnostic_values: Box<[KernelValueReference]>,
    /// Formals whose public type is an aggregate of branch-local projection
    /// requirements. That aggregate is useful to the solver, but is not a
    /// sound direct assignability contract for call diagnostics.
    syntax_discriminated_formals: Box<[u32]>,
    basis_fingerprint_v4: [u8; 32],
}

#[derive(Clone, Debug)]
struct PendingKernelExpressionArtifact {
    id: KernelExpressionId,
    kind: KernelOwnerNodeKind,
    inputs: Box<[KernelExpressionInputArtifact]>,
}

#[derive(Clone, Debug)]
struct PendingKernelCallArtifact {
    expression: KernelExpressionId,
    target: KernelCallTarget,
    inputs: Box<[KernelCallInputArtifact]>,
}

#[derive(Clone, Debug)]
struct SolvedKernelCallFacts {
    type_substitutions: Box<[KernelCallTypeSubstitution]>,
}

#[derive(Clone, Debug, Default)]
struct PendingKernelResources {
    sources: Box<[PendingKernelSourceArtifact]>,
    states: Box<[PendingKernelStateArtifact]>,
    lists: Box<[PendingKernelListArtifact]>,
}

#[derive(Clone, Debug)]
struct PendingKernelSourceArtifact {
    id: KernelSourceId,
    declaration: KernelDeclarationReference,
    statement: KernelStatementReference,
    expression: KernelExpressionId,
    path: KernelSemanticPath,
    interval_ms: Option<u64>,
}

#[derive(Clone, Debug)]
struct PendingKernelStateArtifact {
    id: KernelStateId,
    binding_declaration: KernelDeclarationReference,
    declaration: KernelDeclarationReference,
    statement: KernelStatementReference,
    expression: KernelExpressionId,
    initial: KernelValueReference,
    path: KernelSemanticPath,
    kind: CheckedStateKind,
}

#[derive(Clone, Debug)]
struct PendingKernelListArtifact {
    id: KernelListId,
    declaration: KernelDeclarationReference,
    statement: KernelStatementReference,
    producer: KernelExpressionId,
    path: KernelSemanticPath,
    capacity: Option<usize>,
    key_policy: CheckedListKeyPolicy,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelCallTarget {
    User {
        target: KernelOwnerId,
        inherited_formal: Option<KernelInheritedFormal>,
    },
    RenderConstructor {
        kind: KernelRenderConstructorKind,
    },
    PureBuiltin {
        kind: KernelPureBuiltinKind,
    },
    HostEffect {
        operation: Box<str>,
    },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelCallInputRole {
    Formal { ordinal: u32 },
    Abi { name: Box<str> },
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelCallInputArtifact {
    pub role: KernelCallInputRole,
    pub value: KernelValueReference,
}

/// A call input names either an expression in the call's definition or an
/// explicit expression/result authority in another dense definition. Keeping
/// the namespaces distinct prevents linked external providers from masquerading
/// as out-of-range local expression IDs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelValueReference {
    Local(KernelExpressionId),
    External(KernelExternalExpression),
}

pub type KernelCallValueReference = KernelValueReference;

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelExpressionInputArtifact {
    pub role: KernelOwnerEdgeRole,
    pub value: KernelValueReference,
}

/// One solved expression row in a definition artifact. The compact authored
/// kind and typed input edges survive solving, so downstream stages consume
/// immutable definition facts instead of reconstructing source graphs.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelExpressionArtifact {
    pub id: KernelExpressionId,
    pub kind: KernelOwnerNodeKind,
    pub inputs: Box<[KernelExpressionInputArtifact]>,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelStatementArtifact {
    pub id: KernelStatementId,
    pub kind: KernelStatementKind,
    pub value: Option<KernelValueReference>,
    pub children: Box<[KernelStatementChildReference]>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelDeclarationArtifact {
    pub id: KernelDeclarationId,
    pub origin: KernelDeclarationOrigin,
    pub name: Box<str>,
    pub kind: KernelDeclarationKind,
    pub value: Option<KernelValueReference>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelLexicalBindingTarget {
    Declaration(KernelDeclarationReference),
    ContextFormal { ordinal: u32 },
    Value { provider: KernelValueReference },
    RuntimeContext,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelLexicalBindingArtifact {
    pub expression: KernelExpressionId,
    pub target: KernelLexicalBindingTarget,
    pub projection: Box<[Box<str>]>,
    pub access: KernelLexicalAccess,
}

/// One source-authored call occurrence with its compact input edges and solved
/// result. Downstream consumers no longer need to rediscover call structure by
/// walking the owner expression graph.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelCallArtifact {
    pub expression: KernelExpressionId,
    pub target: KernelCallTarget,
    pub inputs: Box<[KernelCallInputArtifact]>,
    /// Target-definition-local substitutions, independent of the solver's
    /// revision-local variable namespace.
    pub type_substitutions: Box<[KernelCallTypeSubstitution]>,
    pub result: FlowType,
}

/// Stable ordinal of a schematic type variable in one callable interface.
///
/// Ordinals are assigned by walking formals in declaration order followed by
/// the result. They therefore remain meaningful across fresh solver arenas and
/// never expose global checked-program `TypeVar` numbering.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct KernelTypeParameterId(pub u32);

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelCallTypeSubstitution {
    pub variable: KernelTypeParameterId,
    pub value: Type,
}

/// Stable severity of a kernel-owned diagnostic fact.
///
/// Presentation layers relocate this fact to source coordinates. Keeping the
/// severity in the kernel makes diagnostics demand a complete compiler
/// product without importing parser or legacy typechecker DTOs.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum KernelDiagnosticSeverity {
    Error,
    Warning,
}

/// Dense authored location of a diagnostic before source relocation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum KernelDiagnosticSite {
    Expression {
        expression: KernelExpressionId,
    },
    CallArgument {
        call: KernelExpressionId,
        source: KernelCallArgumentSource,
    },
    CallPass {
        call: KernelExpressionId,
        pipe: bool,
    },
    CallInput {
        call: KernelExpressionId,
        target: KernelOwnerId,
        formal_ordinal: u32,
    },
}

/// Structural reason one actual type does not satisfy a callable formal.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum KernelTypeMismatch {
    MissingField(Box<str>),
    IncompatibleField(Box<str>),
    Type,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum KernelCallArgumentSource {
    PipeInput,
    CallArgument { ordinal: u32 },
    PipeArgument { ordinal: u32 },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelCallArgumentKind {
    Named,
    BareBinding,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelCallableKind {
    User,
    Builtin,
    External,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelCallShapeArgument {
    pub source: KernelCallArgumentSource,
    pub kind: KernelCallArgumentKind,
    pub name: Box<str>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelCallShapeParameter {
    pub ordinal: u32,
    pub kind: KernelParameterKind,
    pub name: Box<str>,
    pub optional: bool,
    pub evaluation_scope: KernelParameterEvaluationScope,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelCallShapeInput {
    pub expression: KernelExpressionId,
    pub function: Box<str>,
    pub pipe: bool,
    pub arguments: Box<[KernelCallShapeArgument]>,
    pub pass: bool,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelCallShapeResolution {
    Callable {
        kind: KernelCallableKind,
        parameters: Box<[KernelCallShapeParameter]>,
        context_ordinal: Option<u32>,
        caller_context_ordinal: Option<u32>,
    },
    Ambiguous {
        candidate_count: u32,
    },
    Unresolved,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelMatchedCallInput {
    pub source: KernelCallArgumentSource,
    pub formal_ordinal: u32,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelCallShapeProjection {
    pub matched_inputs: Box<[KernelMatchedCallInput]>,
    pub explicit_context_ordinal: Option<u32>,
    pub inherited_formal: Option<KernelInheritedFormal>,
    pub diagnostics: Box<[KernelDiagnosticInput]>,
    pub valid: bool,
}

/// Stable reason for rejecting one exact Number token. The source text and
/// one-based failure position are retained separately in the typed payload so
/// presentation never needs to parse the literal again.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum KernelNumberLiteralErrorReason {
    Empty,
    Whitespace,
    LeadingPlus,
    InvalidDigit,
    InvalidSyntax,
    InvalidExponent,
    ZeroDenominator,
    InvalidRadix,
    ResourceLimit,
}

impl From<ExactNumberParseReason> for KernelNumberLiteralErrorReason {
    fn from(reason: ExactNumberParseReason) -> Self {
        match reason {
            ExactNumberParseReason::Empty => Self::Empty,
            ExactNumberParseReason::Whitespace => Self::Whitespace,
            ExactNumberParseReason::LeadingPlus => Self::LeadingPlus,
            ExactNumberParseReason::InvalidDigit => Self::InvalidDigit,
            ExactNumberParseReason::InvalidSyntax => Self::InvalidSyntax,
            ExactNumberParseReason::InvalidExponent => Self::InvalidExponent,
            ExactNumberParseReason::ZeroDenominator => Self::ZeroDenominator,
            ExactNumberParseReason::InvalidRadix => Self::InvalidRadix,
            ExactNumberParseReason::ResourceLimit => Self::ResourceLimit,
        }
    }
}

/// Typed diagnostic payload. Human-facing codes, parameter names, spans, and
/// wording are deliberately supplied by the compiler facade from stable
/// owner/expression identities instead of being embedded in the hot kernel.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub enum KernelDiagnosticKind {
    InvalidExpression {
        tokens: Box<[Box<str>]>,
    },
    InvalidPattern,
    InvalidNumberLiteral {
        literal: Box<str>,
        reason: KernelNumberLiteralErrorReason,
        position: u32,
        detail: Box<str>,
    },
    InvalidBitsLiteral {
        width: u32,
        radix: u32,
        digits: Box<str>,
        /// `boon_data::BitsError` does not yet expose a stable reason enum.
        /// Retain its deterministic semantic detail while the surrounding
        /// diagnostic remains structurally typed.
        detail: Box<str>,
    },
    ByteLiteralOutsideBytes,
    DuplicateRecordField {
        name: Box<str>,
    },
    MissingPassedContext,
    UnresolvedValue {
        name: Box<str>,
    },
    CallableUsedAsValue {
        function: Box<str>,
    },
    AmbiguousValue {
        name: Box<str>,
        candidate_count: u32,
    },
    UnresolvedCallable {
        function: Box<str>,
    },
    AmbiguousCallable {
        function: Box<str>,
        candidate_count: u32,
    },
    PipeWithoutValueInput {
        function: Box<str>,
    },
    UnexpectedCallEntry {
        function: Box<str>,
        name: Box<str>,
    },
    MisorderedCallEntry {
        function: Box<str>,
        position: u32,
        expected_name: Box<str>,
        actual_name: Box<str>,
    },
    MissingCallEntry {
        function: Box<str>,
        name: Box<str>,
    },
    BareOrdinaryInput {
        name: Box<str>,
    },
    PassOnAuthoritativeCallable {
        function: Box<str>,
        callable_kind: KernelCallableKind,
    },
    MissingPassContext {
        function: Box<str>,
        root_call: bool,
    },
    CallInputType {
        actual: Type,
        expected: Type,
        mismatch: KernelTypeMismatch,
    },
}

/// Definition-local typed diagnostic before its dense owner is assigned.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelDiagnosticInput {
    pub severity: KernelDiagnosticSeverity,
    pub site: KernelDiagnosticSite,
    pub kind: KernelDiagnosticKind,
}

/// One immutable diagnostic emitted from the solved type graph.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelDiagnosticArtifact {
    pub owner: KernelOwnerId,
    pub severity: KernelDiagnosticSeverity,
    pub site: KernelDiagnosticSite,
    pub kind: KernelDiagnosticKind,
}

/// Project the source-expression diagnostic family from the stable syntax
/// model into dense definition-local facts.
///
/// This is intentionally owned by the kernel even while the differential
/// bridge supplies the syntax view. It consumes only `boon_syntax` rows, not
/// parser implementation state or legacy checker DTOs.
pub fn project_kernel_source_expression_diagnostics<'a>(
    expressions: impl IntoIterator<Item = (KernelExpressionId, &'a AstExpr)>,
) -> Result<Box<[KernelDiagnosticInput]>, KernelOwnerBuildError> {
    let expressions = expressions.into_iter().collect::<Vec<_>>();
    let mut dense_ids = vec![false; expressions.len()];
    let mut syntax_ids = HashSet::with_capacity(expressions.len());
    for (dense, expression) in &expressions {
        let Some(seen) = dense_ids.get_mut(dense.0 as usize) else {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel source diagnostic expression {} is outside dense range 0..{}",
                dense.0,
                expressions.len()
            )));
        };
        if *seen {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel source diagnostics repeat dense expression {}",
                dense.0
            )));
        }
        *seen = true;
        if !syntax_ids.insert(expression.id) {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel source diagnostics repeat syntax expression {}",
                expression.id
            )));
        }
    }
    let byte_items = expressions
        .iter()
        .filter_map(|(_, expression)| match &expression.kind {
            AstExprKind::BytesLiteral { items, .. } => Some(items.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect::<HashSet<_>>();
    let dense_by_syntax = expressions
        .iter()
        .map(|(dense, expression)| (expression.id, *dense))
        .collect::<HashMap<_, _>>();
    let mut diagnostics = Vec::new();
    for (expression_id, expression) in expressions {
        let mut push = |kind| {
            diagnostics.push(KernelDiagnosticInput {
                severity: KernelDiagnosticSeverity::Error,
                site: KernelDiagnosticSite::Expression {
                    expression: expression_id,
                },
                kind,
            });
        };
        match &expression.kind {
            AstExprKind::Unknown(tokens) => push(KernelDiagnosticKind::InvalidExpression {
                tokens: tokens
                    .iter()
                    .cloned()
                    .map(String::into_boxed_str)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            }),
            AstExprKind::MatchArm {
                pattern: AstMatchPattern::Invalid { .. },
                ..
            }
            | AstExprKind::Arrow {
                pattern: AstMatchPattern::Invalid { .. },
                ..
            } => push(KernelDiagnosticKind::InvalidPattern),
            _ => {}
        }

        let number = match &expression.kind {
            AstExprKind::Number(literal) => Some(literal.as_str()),
            AstExprKind::MatchArm {
                pattern: AstMatchPattern::Number { value },
                ..
            }
            | AstExprKind::Arrow {
                pattern: AstMatchPattern::Number { value },
                ..
            } => Some(value.as_str()),
            _ => None,
        };
        if let Some(literal) = number
            && let Err(error) = ExactNumber::parse_strict(literal, None)
        {
            push(KernelDiagnosticKind::InvalidNumberLiteral {
                literal: literal.into(),
                reason: error.reason().into(),
                position: u32::try_from(error.position()).unwrap_or(u32::MAX),
                detail: error.to_string().into_boxed_str(),
            });
        }

        let bits = match &expression.kind {
            AstExprKind::BitsLiteral {
                width,
                radix,
                digits,
            } => Some((*width, *radix, digits.as_str())),
            AstExprKind::MatchArm {
                pattern:
                    AstMatchPattern::Bits {
                        width,
                        radix,
                        digits,
                    },
                ..
            }
            | AstExprKind::Arrow {
                pattern:
                    AstMatchPattern::Bits {
                        width,
                        radix,
                        digits,
                    },
                ..
            } => Some((*width, *radix, digits.as_str())),
            _ => None,
        };
        if let Some((width, radix, digits)) = bits
            && let Err(error) = Bits::parse_encoded(width, radix, digits)
        {
            push(KernelDiagnosticKind::InvalidBitsLiteral {
                width,
                radix,
                digits: digits.into(),
                detail: error.to_string().into_boxed_str(),
            });
        }

        if matches!(expression.kind, AstExprKind::ByteLiteral { .. })
            && !byte_items.contains(&expression.id)
        {
            push(KernelDiagnosticKind::ByteLiteralOutsideBytes);
        }

        if let AstExprKind::Object(fields) | AstExprKind::TaggedObject { fields, .. } =
            &expression.kind
        {
            let mut names = HashSet::new();
            for field in fields.iter().filter(|field| !field.spread) {
                if names.insert(field.name.as_str()) {
                    continue;
                }
                let value = dense_by_syntax.get(&field.value).copied().ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel duplicate record field `{}` references missing expression {}",
                        field.name, field.value
                    ))
                })?;
                diagnostics.push(KernelDiagnosticInput {
                    severity: KernelDiagnosticSeverity::Error,
                    site: KernelDiagnosticSite::Expression { expression: value },
                    kind: KernelDiagnosticKind::DuplicateRecordField {
                        name: field.name.clone().into_boxed_str(),
                    },
                });
            }
        }
    }
    diagnostics.sort_unstable_by(|left, right| left.site.cmp(&right.site));
    Ok(diagnostics.into_boxed_slice())
}

/// Match one normalized call surface against one resolved callable contract.
///
/// This is the permanent lexical call-shape authority. It deliberately does
/// not inspect types or legacy owner plans: valid inputs are returned as dense
/// formal/source pairs, while resolution and arity failures become typed facts
/// and leave the caller free to publish an `Unknown` result node.
pub fn project_kernel_call_shape(
    input: &KernelCallShapeInput,
    resolution: &KernelCallShapeResolution,
) -> Result<KernelCallShapeProjection, KernelOwnerBuildError> {
    let argument_site = |source| KernelDiagnosticSite::CallArgument {
        call: input.expression,
        source,
    };
    let expression_site = || KernelDiagnosticSite::Expression {
        expression: input.expression,
    };
    let mut diagnostics = Vec::new();
    let mut push = |site, kind| {
        diagnostics.push(KernelDiagnosticInput {
            severity: KernelDiagnosticSeverity::Error,
            site,
            kind,
        });
    };
    for (ordinal, argument) in input.arguments.iter().enumerate() {
        let ordinal = u32::try_from(ordinal).map_err(|_| {
            KernelOwnerBuildError::new(format!(
                "kernel call `{}` argument count exceeds the u32 namespace",
                input.function
            ))
        })?;
        let expected = if input.pipe {
            KernelCallArgumentSource::PipeArgument { ordinal }
        } else {
            KernelCallArgumentSource::CallArgument { ordinal }
        };
        if argument.source != expected {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel call `{}` argument {ordinal} has source {:?}, expected {expected:?}",
                input.function, argument.source
            )));
        }
    }

    let (callable_kind, parameters, context_ordinal, caller_context_ordinal) = match resolution {
        KernelCallShapeResolution::Unresolved => {
            push(
                expression_site(),
                KernelDiagnosticKind::UnresolvedCallable {
                    function: input.function.clone(),
                },
            );
            return Ok(KernelCallShapeProjection {
                matched_inputs: Box::new([]),
                explicit_context_ordinal: None,
                inherited_formal: None,
                diagnostics: diagnostics.into_boxed_slice(),
                valid: false,
            });
        }
        KernelCallShapeResolution::Ambiguous { candidate_count } => {
            if *candidate_count < 2 {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel ambiguous call `{}` has fewer than two candidates",
                    input.function
                )));
            }
            push(
                expression_site(),
                KernelDiagnosticKind::AmbiguousCallable {
                    function: input.function.clone(),
                    candidate_count: *candidate_count,
                },
            );
            return Ok(KernelCallShapeProjection {
                matched_inputs: Box::new([]),
                explicit_context_ordinal: None,
                inherited_formal: None,
                diagnostics: diagnostics.into_boxed_slice(),
                valid: false,
            });
        }
        KernelCallShapeResolution::Callable {
            kind,
            parameters,
            context_ordinal,
            caller_context_ordinal,
        } => (*kind, parameters, *context_ordinal, *caller_context_ordinal),
    };

    let mut parameters = parameters.iter().collect::<Vec<_>>();
    parameters.sort_unstable_by_key(|parameter| parameter.ordinal);
    let mut parameter_ordinals = HashSet::with_capacity(parameters.len());
    let mut parameter_names = HashSet::with_capacity(parameters.len());
    for parameter in &parameters {
        if !parameter_ordinals.insert(parameter.ordinal)
            || !parameter_names.insert(parameter.name.as_ref())
        {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel callable `{}` has duplicate formal identity `{}`/{}",
                input.function, parameter.name, parameter.ordinal
            )));
        }
    }
    for parameter in &parameters {
        let KernelParameterEvaluationScope::Output { parameter_ordinal } =
            parameter.evaluation_scope
        else {
            continue;
        };
        if !parameters.iter().any(|candidate| {
            candidate.ordinal == parameter_ordinal && candidate.kind == KernelParameterKind::Out
        }) {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel callable `{}` parameter `{}` references missing OUT evaluation scope {}",
                input.function, parameter.name, parameter_ordinal
            )));
        }
    }
    if context_ordinal.is_some_and(|context| parameter_ordinals.contains(&context)) {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel callable `{}` context overlaps an ordinary formal",
            input.function
        )));
    }

    let piped_parameter = input.pipe.then(|| {
        parameters
            .iter()
            .copied()
            .filter(|parameter| parameter.kind == KernelParameterKind::Value)
            .min_by_key(|parameter| parameter.ordinal)
    });
    if matches!(piped_parameter, Some(None)) {
        push(
            expression_site(),
            KernelDiagnosticKind::PipeWithoutValueInput {
                function: input.function.clone(),
            },
        );
    }
    let piped_parameter = piped_parameter.flatten();
    let mut matched_inputs = Vec::new();
    if let Some(parameter) = piped_parameter {
        matched_inputs.push(KernelMatchedCallInput {
            source: KernelCallArgumentSource::PipeInput,
            formal_ordinal: parameter.ordinal,
        });
    }
    let expected = parameters
        .iter()
        .copied()
        .filter(|parameter| piped_parameter.is_none_or(|piped| parameter.ordinal != piped.ordinal))
        .collect::<Vec<_>>();
    let mut expected_index = 0usize;
    for (call_index, argument) in input.arguments.iter().enumerate() {
        while let Some(parameter) = expected.get(expected_index).copied()
            && parameter.name != argument.name
            && parameter.optional
        {
            expected_index += 1;
        }
        let Some(parameter) = expected.get(expected_index).copied() else {
            push(
                argument_site(argument.source),
                KernelDiagnosticKind::UnexpectedCallEntry {
                    function: input.function.clone(),
                    name: argument.name.clone(),
                },
            );
            continue;
        };
        if parameter.name != argument.name {
            let position = u32::try_from(call_index + 1).map_err(|_| {
                KernelOwnerBuildError::new(format!(
                    "kernel call `{}` argument count exceeds the u32 namespace",
                    input.function
                ))
            })?;
            push(
                argument_site(argument.source),
                KernelDiagnosticKind::MisorderedCallEntry {
                    function: input.function.clone(),
                    position,
                    expected_name: parameter.name.clone(),
                    actual_name: argument.name.clone(),
                },
            );
            expected_index += 1;
            continue;
        }
        expected_index += 1;
        if parameter.kind == KernelParameterKind::Value
            && argument.kind == KernelCallArgumentKind::BareBinding
        {
            push(
                argument_site(argument.source),
                KernelDiagnosticKind::BareOrdinaryInput {
                    name: argument.name.clone(),
                },
            );
        }
        matched_inputs.push(KernelMatchedCallInput {
            source: argument.source,
            formal_ordinal: parameter.ordinal,
        });
    }
    for parameter in expected.iter().skip(expected_index) {
        if !parameter.optional {
            push(
                expression_site(),
                KernelDiagnosticKind::MissingCallEntry {
                    function: input.function.clone(),
                    name: parameter.name.clone(),
                },
            );
        }
    }

    if callable_kind != KernelCallableKind::User && input.pass {
        push(
            KernelDiagnosticSite::CallPass {
                call: input.expression,
                pipe: input.pipe,
            },
            KernelDiagnosticKind::PassOnAuthoritativeCallable {
                function: input.function.clone(),
                callable_kind,
            },
        );
    }
    let mut explicit_context_ordinal = None;
    let mut inherited_formal = None;
    if callable_kind == KernelCallableKind::User
        && let Some(target_ordinal) = context_ordinal
    {
        if input.pass {
            explicit_context_ordinal = Some(target_ordinal);
        } else if let Some(caller_ordinal) = caller_context_ordinal {
            inherited_formal = Some(KernelInheritedFormal {
                target_ordinal,
                caller_ordinal,
            });
        } else {
            push(
                expression_site(),
                KernelDiagnosticKind::MissingPassContext {
                    function: input.function.clone(),
                    root_call: true,
                },
            );
        }
    }
    matched_inputs.sort_unstable_by_key(|input| input.formal_ordinal);
    let valid = diagnostics.is_empty();
    Ok(KernelCallShapeProjection {
        matched_inputs: matched_inputs.into_boxed_slice(),
        explicit_context_ordinal,
        inherited_formal,
        diagnostics: diagnostics.into_boxed_slice(),
        valid,
    })
}

/// One source-authored host-effect occurrence in a definition artifact.
///
/// Policies are copied from the stable ABI registry so downstream stages do
/// not rediscover effects by walking expressions or dispatching on call names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelHostEffectArtifact {
    pub expression: KernelExpressionId,
    pub operation: Box<str>,
    pub replay: ReplaySpec,
    pub barrier: BarrierSpec,
    pub result_policy: ResultPolicySpec,
    pub delivery: DeliveryCardinalitySpec,
}

#[derive(Serialize)]
enum KernelDeliveryCardinalityFingerprint<'a> {
    Single,
    Stream {
        initial_credits: u32,
        max_in_flight: u32,
        credit_result_tags: &'a [&'static str],
        terminal_result_tags: &'a [&'static str],
    },
}

impl Serialize for KernelHostEffectArtifact {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let replay = match self.replay {
            ReplaySpec::ReadOnly => "read_only",
            ReplaySpec::ProcessScoped => "process_scoped",
            ReplaySpec::IdempotentBytesKey => "idempotent_bytes_key",
            ReplaySpec::NonReplayable => "non_replayable",
        };
        let barrier = match self.barrier {
            BarrierSpec::None => "none",
            BarrierSpec::Before => "before",
            BarrierSpec::BeforeAndAfter => "before_and_after",
        };
        let result_policy = match self.result_policy {
            ResultPolicySpec::ReturnValue => "return_value",
            ResultPolicySpec::Acknowledgement => "acknowledgement",
            ResultPolicySpec::Discarded => "discarded",
        };
        let delivery = match &self.delivery {
            DeliveryCardinalitySpec::Single => KernelDeliveryCardinalityFingerprint::Single,
            DeliveryCardinalitySpec::Stream {
                initial_credits,
                max_in_flight,
                credit_result_tags,
                terminal_result_tags,
            } => KernelDeliveryCardinalityFingerprint::Stream {
                initial_credits: *initial_credits,
                max_in_flight: *max_in_flight,
                credit_result_tags,
                terminal_result_tags,
            },
        };
        let mut state = serializer.serialize_struct("KernelHostEffectArtifact", 6)?;
        state.serialize_field("expression", &self.expression)?;
        state.serialize_field("operation", &self.operation)?;
        state.serialize_field("replay", replay)?;
        state.serialize_field("barrier", barrier)?;
        state.serialize_field("result_policy", result_policy)?;
        state.serialize_field("delivery", &delivery)?;
        state.end()
    }
}

impl Hash for KernelHostEffectArtifact {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.expression.hash(state);
        self.operation.hash(state);
        match self.replay {
            ReplaySpec::ReadOnly => 0_u8,
            ReplaySpec::ProcessScoped => 1,
            ReplaySpec::IdempotentBytesKey => 2,
            ReplaySpec::NonReplayable => 3,
        }
        .hash(state);
        match self.barrier {
            BarrierSpec::None => 0_u8,
            BarrierSpec::Before => 1,
            BarrierSpec::BeforeAndAfter => 2,
        }
        .hash(state);
        match self.result_policy {
            ResultPolicySpec::ReturnValue => 0_u8,
            ResultPolicySpec::Acknowledgement => 1,
            ResultPolicySpec::Discarded => 2,
        }
        .hash(state);
        match &self.delivery {
            DeliveryCardinalitySpec::Single => 0_u8.hash(state),
            DeliveryCardinalitySpec::Stream {
                initial_credits,
                max_in_flight,
                credit_result_tags,
                terminal_result_tags,
            } => {
                1_u8.hash(state);
                initial_credits.hash(state);
                max_in_flight.hash(state);
                credit_result_tags.hash(state);
                terminal_result_tags.hash(state);
            }
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelSourceArtifact {
    pub id: KernelSourceId,
    pub declaration: KernelDeclarationReference,
    pub statement: KernelStatementReference,
    pub expression: KernelExpressionId,
    pub path: KernelSemanticPath,
    pub interval_ms: Option<u64>,
    pub payload_type: Type,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelStateArtifact {
    pub id: KernelStateId,
    pub binding_declaration: KernelDeclarationReference,
    pub declaration: KernelDeclarationReference,
    pub statement: KernelStatementReference,
    pub expression: KernelExpressionId,
    pub initial: KernelValueReference,
    pub path: KernelSemanticPath,
    pub kind: CheckedStateKind,
    pub flow_type: FlowType,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct KernelListArtifact {
    pub id: KernelListId,
    pub declaration: KernelDeclarationReference,
    pub statement: KernelStatementReference,
    pub producer: KernelExpressionId,
    pub path: KernelSemanticPath,
    pub item_type: Type,
    pub capacity: Option<usize>,
    pub key_policy: CheckedListKeyPolicy,
}

/// Immutable checked result surface for one definition.
///
/// This is deliberately free of solver cells, operation IDs, and work
/// counters. Later checked rows (calls, effects, state, lists, diagnostics)
/// extend this single artifact instead of creating parallel owner products.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct DefinitionArtifact {
    pub result: FlowType,
    /// Principal callable formal surfaces in dense declaration order. This is
    /// empty for non-callable definitions.
    pub formals: Box<[FlowType]>,
    /// Exact stable source identities retained for direct checked/semantic
    /// linking. Dense IDs remain definition-local and revision-local.
    pub relocations: KernelDefinitionRelocations,
    pub expressions: Box<[KernelExpressionArtifact]>,
    pub statements: Box<[KernelStatementArtifact]>,
    pub declarations: Box<[KernelDeclarationArtifact]>,
    pub lexical_bindings: Box<[KernelLexicalBindingArtifact]>,
    pub calls: Box<[KernelCallArtifact]>,
    pub effects: Box<[KernelHostEffectArtifact]>,
    pub sources: Box<[KernelSourceArtifact]>,
    pub states: Box<[KernelStateArtifact]>,
    pub lists: Box<[KernelListArtifact]>,
    pub diagnostics: Box<[KernelDiagnosticArtifact]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelDefinitionSnapshot {
    pub definition: DefinitionArtifact,
    pub dependencies: crate::KernelDefinitionDependencyGraph,
    pub currentness: crate::KernelDefinitionCurrentnessReceipt,
    pub work: KernelSolveWork,
}

/// Immutable checked snapshot produced by one complete dense solve.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelCheckedSnapshot {
    pub definitions: Box<[DefinitionArtifact]>,
    pub diagnostic_values: Box<[KernelDiagnosticValueArtifact]>,
    pub dependencies: crate::KernelDefinitionDependencyGraph,
    pub currentness: Box<[crate::KernelDefinitionCurrentnessReceipt]>,
    pub work: KernelSolveWork,
}

/// The complete public type surface needed by diagnostics and link planning.
///
/// This deliberately contains no expression, statement, resource, dependency,
/// or currentness rows. A diagnostics-only request must be able to stop here
/// without paying to construct and fingerprint a checked image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelInterfaceSnapshot {
    pub public_results: Box<[FlowType]>,
    pub callable_formals: Box<[Box<[FlowType]>]>,
    /// Fully typed diagnostics computed directly from the quiescent graph.
    /// No checked definition rows are materialized for this product.
    pub diagnostics: Box<[KernelDiagnosticArtifact]>,
    /// Sparse solved values explicitly demanded by diagnostic contracts.
    pub diagnostic_values: Box<[KernelDiagnosticValueArtifact]>,
    pub work: KernelSolveWork,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelDiagnosticValueArtifact {
    pub owner: KernelOwnerId,
    pub ordinal: u32,
    pub value: KernelValueReference,
    pub ty: Type,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelDemandedDefinitionArtifact {
    pub owner: KernelOwnerId,
    pub definition: DefinitionArtifact,
}

/// Sparse checked artifact product for explicit definition demand.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelDemandedDefinitionSnapshot {
    pub definitions: Box<[KernelDemandedDefinitionArtifact]>,
    pub work: KernelSolveWork,
}

/// Returns whether the permanent kernel can resolve this operation entirely
/// from the lower-level host-effect ABI registry.
pub fn is_kernel_host_effect(operation: &str) -> bool {
    host_effect_spec(operation).is_some_and(|spec| {
        spec.result_policy == ResultPolicySpec::ReturnValue && spec.schema.is_some()
    })
}

/// Returns whether the stable host-effect registry owns `operation`, even
/// when its compact result policy has not migrated into this kernel yet.
pub fn is_registered_kernel_host_effect(operation: &str) -> bool {
    host_effect_spec(operation).is_some()
}

impl KernelOwnerProgram {
    pub fn solve(self) -> Result<KernelDefinitionSnapshot, KernelSolveError> {
        let basis_fingerprint_v4 = self.basis_fingerprint_v4;
        let artifact = solve_component(self.component)?;
        let mut result = artifact
            .output(self.result_output)
            .expect("owner result output belongs to its component")
            .flow_type
            .clone();
        let expression_flows = self
            .expression_outputs
            .iter()
            .zip(self.expression_modes.iter().copied())
            .map(|(output, mode)| {
                let mut flow = artifact
                    .output(*output)
                    .expect("owner expression output belongs to its component")
                    .flow_type
                    .clone();
                flow.mode = mode;
                flow
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let result_index = self
            .expression_outputs
            .iter()
            .position(|output| *output == self.result_output)
            .expect("owner result belongs to its expression outputs");
        result.mode = self.expression_modes[result_index];
        let formal_flows = self
            .formal_outputs
            .iter()
            .zip(self.formal_modes.iter().copied())
            .map(|(output, mode)| {
                let mut flow = artifact
                    .output(*output)
                    .expect("owner formal output belongs to its component")
                    .flow_type
                    .clone();
                flow.mode = mode;
                flow
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let calls = materialize_call_artifacts(
            KernelOwnerId(0),
            self.calls,
            &expression_flows,
            std::slice::from_ref(&formal_flows),
            std::slice::from_ref(&result),
        );
        let (sources, states, lists) =
            materialize_resource_artifacts(self.resources, &expression_flows, None);
        let expressions =
            materialize_expression_artifacts(self.expression_artifacts, expression_flows);
        let mut definition = DefinitionArtifact {
            result,
            formals: formal_flows,
            relocations: self.relocations,
            expressions,
            statements: self.statements,
            declarations: self.declarations,
            lexical_bindings: self.lexical_bindings,
            calls,
            effects: self.effects,
            sources,
            states,
            lists,
            diagnostics: self.diagnostics,
        };
        let (dependencies, currentness) = build_snapshot_receipts(
            std::slice::from_mut(&mut definition),
            &[basis_fingerprint_v4],
        )?;
        let [currentness] = currentness.as_ref() else {
            unreachable!("one standalone kernel definition produces one receipt")
        };
        Ok(KernelDefinitionSnapshot {
            definition,
            dependencies,
            currentness: *currentness,
            work: artifact.work,
        })
    }

    pub fn component(&self) -> &ComponentProgram {
        &self.component
    }
}

impl KernelProjectProgram {
    pub fn solve(self) -> Result<KernelCheckedSnapshot, KernelSolveError> {
        self.solve_graph()?.into_checked_snapshot()
    }

    /// Solve the complete project type graph but publish only its public
    /// interfaces. This is the diagnostics boundary: it performs no checked
    /// expression/resource projection, alpha-normalization of definition
    /// artifacts, dependency indexing, or currentness hashing.
    pub fn solve_interfaces(self) -> Result<KernelInterfaceSnapshot, KernelSolveError> {
        Ok(self.solve_graph()?.interface_snapshot())
    }

    /// Materialize only the explicitly demanded definition artifacts.
    ///
    /// Public results for every definition are still solved because call and
    /// cross-owner equations share one component. Undemanded expression,
    /// statement, resource, dependency, and receipt tables are never built.
    pub fn solve_definitions(
        self,
        demanded: &[KernelOwnerId],
    ) -> Result<KernelDemandedDefinitionSnapshot, KernelSolveError> {
        let artifact = solve_component(self.component)?;
        let public_results = project_public_results(&self.owners, &artifact);
        let public_formals = project_public_formals(&self.owners, &artifact);
        let (call_facts, diagnostics) = project_call_facts_and_diagnostics(
            &self.owners,
            &artifact,
            &public_results,
            &public_formals,
        );
        KernelSolvedProject {
            artifact,
            owners: self.owners,
            public_results,
            public_formals,
            call_facts,
            diagnostics,
        }
        .into_demanded_definitions(demanded)
    }

    /// Solve all type equations once without publishing any optional checked
    /// product. `KernelSession` retains this quiescent graph across multiple
    /// demands in the same revision.
    pub fn solve_graph(self) -> Result<KernelSolvedProject, KernelSolveError> {
        let artifact = solve_component(self.component)?;
        let public_results = project_public_results(&self.owners, &artifact);
        let public_formals = project_public_formals(&self.owners, &artifact);
        let (call_facts, diagnostics) = project_call_facts_and_diagnostics(
            &self.owners,
            &artifact,
            &public_results,
            &public_formals,
        );
        Ok(KernelSolvedProject {
            artifact,
            owners: self.owners,
            public_results,
            public_formals,
            call_facts,
            diagnostics,
        })
    }

    pub fn component(&self) -> &ComponentProgram {
        &self.component
    }

    pub const fn compile_work(&self) -> KernelCompileWork {
        self.compile_work
    }
}

impl KernelSolvedProject {
    pub fn interface_snapshot(&self) -> KernelInterfaceSnapshot {
        let mut public_results = Vec::with_capacity(self.public_results.len());
        let mut callable_formals = Vec::with_capacity(self.public_formals.len());
        let mut diagnostics = Vec::new();
        let mut diagnostic_values = Vec::new();
        for (owner, (formals, result)) in self
            .public_formals
            .iter()
            .zip(&self.public_results)
            .enumerate()
        {
            let owner_value_types = self.owners[owner]
                .diagnostic_values
                .iter()
                .map(|value| {
                    project_call_value_type(
                        owner,
                        *value,
                        &self.owners,
                        &self.artifact,
                        &self.public_results,
                    )
                    .expect("validated diagnostic value has a solved provider")
                })
                .collect::<Vec<_>>();
            let (formals, result, owner_diagnostics, owner_value_types) =
                alpha_normalize_callable_interface_and_diagnostics(
                    formals,
                    result,
                    &self.diagnostics[owner],
                    &owner_value_types,
                );
            callable_formals.push(formals);
            public_results.push(result);
            diagnostics.extend(owner_diagnostics);
            diagnostic_values.extend(
                self.owners[owner]
                    .diagnostic_values
                    .iter()
                    .copied()
                    .zip(owner_value_types)
                    .enumerate()
                    .map(|(ordinal, (value, ty))| KernelDiagnosticValueArtifact {
                        owner: KernelOwnerId(
                            u32::try_from(owner)
                                .expect("kernel diagnostic owner count exceeds u32"),
                        ),
                        ordinal: u32::try_from(ordinal)
                            .expect("kernel diagnostic value count exceeds u32"),
                        value,
                        ty,
                    }),
            );
        }
        KernelInterfaceSnapshot {
            public_results: public_results.into_boxed_slice(),
            callable_formals: callable_formals.into_boxed_slice(),
            diagnostics: diagnostics.into_boxed_slice(),
            diagnostic_values: diagnostic_values.into_boxed_slice(),
            work: self.artifact.work,
        }
    }

    pub fn checked_snapshot(&self) -> Result<KernelCheckedSnapshot, KernelSolveError> {
        self.clone().into_checked_snapshot()
    }

    pub fn into_checked_snapshot(self) -> Result<KernelCheckedSnapshot, KernelSolveError> {
        let diagnostic_values = self.interface_snapshot().diagnostic_values;
        let basis_fingerprints = self
            .owners
            .iter()
            .map(|owner| owner.basis_fingerprint_v4)
            .collect::<Vec<_>>();
        let mut definitions = self
            .owners
            .into_vec()
            .into_iter()
            .enumerate()
            .zip(self.call_facts.into_vec())
            .zip(self.diagnostics.into_vec())
            .map(|(((owner_index, owner), call_facts), diagnostics)| {
                materialize_project_definition(
                    owner_index,
                    owner,
                    &self.artifact,
                    &self.public_results,
                    &self.public_formals,
                    call_facts,
                    diagnostics,
                )
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let (dependencies, currentness) =
            build_snapshot_receipts(&mut definitions, &basis_fingerprints)?;
        Ok(KernelCheckedSnapshot {
            definitions,
            diagnostic_values,
            dependencies,
            currentness,
            work: self.artifact.work,
        })
    }

    pub fn demanded_definitions(
        &self,
        demanded: &[KernelOwnerId],
    ) -> Result<KernelDemandedDefinitionSnapshot, KernelSolveError> {
        self.clone().into_demanded_definitions(demanded)
    }

    pub fn into_demanded_definitions(
        self,
        demanded: &[KernelOwnerId],
    ) -> Result<KernelDemandedDefinitionSnapshot, KernelSolveError> {
        let mut demanded = demanded.to_vec();
        demanded.sort_unstable();
        demanded.dedup();
        if let Some(owner) = demanded
            .iter()
            .find(|owner| owner.0 as usize >= self.owners.len())
        {
            return Err(KernelSolveError::new(format!(
                "kernel definition demand references missing owner {}",
                owner.0
            )));
        }
        let mut demanded_iter = demanded.into_iter().peekable();
        let mut definitions = Vec::with_capacity(demanded_iter.len());
        for (((owner_index, owner), call_facts), diagnostics) in self
            .owners
            .into_vec()
            .into_iter()
            .enumerate()
            .zip(self.call_facts.into_vec())
            .zip(self.diagnostics.into_vec())
        {
            let dense_owner = KernelOwnerId(
                u32::try_from(owner_index)
                    .expect("kernel definition count exceeds the dense u32 namespace"),
            );
            if demanded_iter.peek().copied() != Some(dense_owner) {
                continue;
            }
            demanded_iter.next();
            let mut definition = materialize_project_definition(
                owner_index,
                owner,
                &self.artifact,
                &self.public_results,
                &self.public_formals,
                call_facts,
                diagnostics,
            );
            alpha_normalize_definition(&mut definition);
            definitions.push(KernelDemandedDefinitionArtifact {
                owner: dense_owner,
                definition,
            });
        }
        debug_assert!(demanded_iter.next().is_none());
        Ok(KernelDemandedDefinitionSnapshot {
            definitions: definitions.into_boxed_slice(),
            work: self.artifact.work,
        })
    }
}

fn project_public_results(
    owners: &[KernelProjectOwnerOutputs],
    artifact: &ComponentArtifact,
) -> Box<[FlowType]> {
    owners
        .iter()
        .map(|owner| {
            let mut result = artifact
                .output(owner.result)
                .expect("project owner result belongs to its component")
                .flow_type
                .clone();
            let result_index = owner
                .expressions
                .iter()
                .position(|output| *output == owner.result)
                .expect("owner result belongs to its expression outputs");
            result.mode = owner.expression_modes[result_index];
            result
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn project_public_formals(
    owners: &[KernelProjectOwnerOutputs],
    artifact: &ComponentArtifact,
) -> Box<[Box<[FlowType]>]> {
    owners
        .iter()
        .map(|owner| {
            owner
                .formals
                .iter()
                .zip(owner.formal_modes.iter().copied())
                .map(|(output, mode)| {
                    let mut flow = artifact
                        .output(*output)
                        .expect("project owner formal belongs to its component")
                        .flow_type
                        .clone();
                    flow.mode = mode;
                    flow
                })
                .collect::<Vec<_>>()
                .into_boxed_slice()
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

/// Project reusable call facts and user-facing type failures directly from the
/// quiescent graph.
///
/// Call substitutions are derived exactly once and retained for later checked
/// artifact materialization. This deliberately consumes only solved output
/// cells, compact call edges, and public callable interfaces. It does not
/// construct checked expression, statement, resource, dependency, or
/// currentness rows, which is the central cost invariant of
/// `CheckDemand::Diagnostics`.
fn project_call_facts_and_diagnostics(
    owners: &[KernelProjectOwnerOutputs],
    artifact: &ComponentArtifact,
    public_results: &[FlowType],
    public_formals: &[Box<[FlowType]>],
) -> (
    Box<[Box<[SolvedKernelCallFacts]>]>,
    Box<[Box<[KernelDiagnosticArtifact]>]>,
) {
    let mut project_call_facts = Vec::with_capacity(owners.len());
    let mut project_diagnostics = Vec::with_capacity(owners.len());
    for (owner_index, owner) in owners.iter().enumerate() {
        let owner_id = KernelOwnerId(
            u32::try_from(owner_index)
                .expect("kernel diagnostic owner count exceeds the dense u32 namespace"),
        );
        let mut owner_call_facts = Vec::with_capacity(owner.calls.len());
        let mut diagnostics = owner.diagnostics.to_vec();
        for call in owner.calls.iter() {
            let substitutions = if let KernelCallTarget::User { target, .. } = call.target {
                let target_formals = public_formals
                    .get(target.0 as usize)
                    .expect("validated kernel call target has public formals");
                let target_result = public_results
                    .get(target.0 as usize)
                    .expect("validated kernel call target has a public result");

                let mut actuals = call
                    .inputs
                    .iter()
                    .filter_map(|input| {
                        let KernelCallInputRole::Formal { ordinal } = input.role else {
                            return None;
                        };
                        project_call_value_type(
                            owner_index,
                            input.value,
                            owners,
                            artifact,
                            public_results,
                        )
                        .map(|actual| (ordinal, actual))
                    })
                    .collect::<Vec<_>>();
                if let KernelCallTarget::User {
                    inherited_formal: Some(inherited),
                    ..
                } = call.target
                    && let Some(actual) = public_formals
                        .get(owner_index)
                        .and_then(|formals| formals.get(inherited.caller_ordinal as usize))
                {
                    actuals.push((inherited.target_ordinal, actual.ty.clone()));
                }
                let substitutions =
                    derive_kernel_call_type_substitutions(target_formals, target_result, &actuals);
                // Inherited context has no authored call-input site. Its
                // requirements are propagated through the separate formal
                // requirement channel and are intentionally not diagnosed as
                // an explicit argument error here.
                for input in call.inputs.iter() {
                    let KernelCallInputRole::Formal { ordinal } = input.role else {
                        continue;
                    };
                    if owners.get(target.0 as usize).is_some_and(|target| {
                        target
                            .syntax_discriminated_formals
                            .binary_search(&ordinal)
                            .is_ok()
                    }) {
                        // The target's aggregate formal combines projection
                        // requirements from mutually exclusive syntax arms.
                        // It is a solver surface, not a direct object-shape
                        // contract for this occurrence.
                        continue;
                    }
                    let Some(actual) = project_call_value_type(
                        owner_index,
                        input.value,
                        owners,
                        artifact,
                        public_results,
                    ) else {
                        continue;
                    };
                    let Some(expected) = target_formals.get(ordinal as usize) else {
                        continue;
                    };
                    let expected = instantiate_kernel_call_type(
                        &expected.ty,
                        target_formals,
                        target_result,
                        &substitutions,
                    );
                    if kernel_type_is_assignable_to(&actual, &expected) {
                        continue;
                    }
                    diagnostics.push(KernelDiagnosticArtifact {
                        owner: owner_id,
                        severity: KernelDiagnosticSeverity::Error,
                        site: KernelDiagnosticSite::CallInput {
                            call: call.expression,
                            target,
                            formal_ordinal: ordinal,
                        },
                        kind: KernelDiagnosticKind::CallInputType {
                            mismatch: kernel_type_mismatch(&actual, &expected),
                            actual,
                            expected,
                        },
                    });
                }
                substitutions
            } else {
                Box::new([])
            };
            owner_call_facts.push(SolvedKernelCallFacts {
                type_substitutions: substitutions,
            });
        }
        diagnostics.sort_unstable_by(|left, right| left.site.cmp(&right.site));
        project_call_facts.push(owner_call_facts.into_boxed_slice());
        project_diagnostics.push(diagnostics.into_boxed_slice());
    }
    (
        project_call_facts.into_boxed_slice(),
        project_diagnostics.into_boxed_slice(),
    )
}

fn project_call_value_type(
    caller: usize,
    value: KernelValueReference,
    owners: &[KernelProjectOwnerOutputs],
    artifact: &ComponentArtifact,
    public_results: &[FlowType],
) -> Option<Type> {
    match value {
        KernelValueReference::Local(expression) => owners
            .get(caller)?
            .expressions
            .get(expression.0 as usize)
            .and_then(|output| artifact.output(*output))
            .map(|output| output.flow_type.ty.clone()),
        KernelValueReference::External(KernelExternalExpression {
            owner,
            target: KernelExternalTarget::Result,
        }) => public_results
            .get(owner.0 as usize)
            .map(|result| result.ty.clone()),
        KernelValueReference::External(KernelExternalExpression {
            owner,
            target: KernelExternalTarget::Expression(expression),
        }) => owners
            .get(owner.0 as usize)?
            .expressions
            .get(expression.0 as usize)
            .and_then(|output| artifact.output(*output))
            .map(|output| output.flow_type.ty.clone()),
    }
}

fn instantiate_kernel_call_type(
    ty: &Type,
    target_formals: &[FlowType],
    target_result: &FlowType,
    substitutions: &[KernelCallTypeSubstitution],
) -> Type {
    let mut parameter_ids = BTreeMap::new();
    for formal in target_formals {
        collect_callable_type_parameters(&formal.ty, &mut parameter_ids);
    }
    collect_callable_type_parameters(&target_result.ty, &mut parameter_ids);
    let substitutions = substitutions
        .iter()
        .map(|substitution| (substitution.variable, &substitution.value))
        .collect::<BTreeMap<_, _>>();
    substitute_kernel_call_type(ty, &parameter_ids, &substitutions)
}

fn substitute_kernel_call_type(
    ty: &Type,
    parameter_ids: &BTreeMap<boon_checked::TypeVar, KernelTypeParameterId>,
    substitutions: &BTreeMap<KernelTypeParameterId, &Type>,
) -> Type {
    match ty {
        Type::Var(variable) => parameter_ids
            .get(variable)
            .and_then(|parameter| substitutions.get(parameter).copied())
            .cloned()
            .unwrap_or_else(|| ty.clone()),
        Type::Object(shape) => Type::object(ObjectShape {
            fields: shape
                .fields
                .iter()
                .map(|(name, field)| {
                    (
                        name.clone(),
                        substitute_kernel_call_type(field, parameter_ids, substitutions),
                    )
                })
                .collect(),
            field_order: shape.field_order.clone(),
            open: shape.open,
        }),
        Type::List(item) => Type::List(Type::shared(substitute_kernel_call_type(
            item,
            parameter_ids,
            substitutions,
        ))),
        Type::Set(item) => Type::Set(Type::shared(substitute_kernel_call_type(
            item,
            parameter_ids,
            substitutions,
        ))),
        Type::Map { key, value } => Type::Map {
            key: Box::new(substitute_kernel_call_type(
                key,
                parameter_ids,
                substitutions,
            )),
            value: Box::new(substitute_kernel_call_type(
                value,
                parameter_ids,
                substitutions,
            )),
        },
        Type::Function { args, result } => Type::Function {
            args: args
                .iter()
                .map(|argument| substitute_kernel_call_type(argument, parameter_ids, substitutions))
                .collect(),
            result: Box::new(FlowType {
                mode: result.mode,
                ty: substitute_kernel_call_type(&result.ty, parameter_ids, substitutions),
            }),
        },
        Type::VariantSet(variants) => Type::VariantSet(
            variants
                .iter()
                .map(|variant| match variant {
                    Variant::Tag(tag) => Variant::Tag(tag.clone()),
                    Variant::Tagged { tag, fields } => Variant::tagged(
                        tag.clone(),
                        ObjectShape {
                            fields: fields
                                .fields
                                .iter()
                                .map(|(name, field)| {
                                    (
                                        name.clone(),
                                        substitute_kernel_call_type(
                                            field,
                                            parameter_ids,
                                            substitutions,
                                        ),
                                    )
                                })
                                .collect(),
                            field_order: fields.field_order.clone(),
                            open: fields.open,
                        },
                    ),
                })
                .collect(),
        ),
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|member| substitute_kernel_call_type(member, parameter_ids, substitutions))
                .collect(),
        ),
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => ty.clone(),
    }
}

fn kernel_type_is_assignable_to(actual: &Type, expected: &Type) -> bool {
    if actual == expected {
        return true;
    }
    if call_type_is_placeholder(actual) || call_type_is_placeholder(expected) {
        return true;
    }
    match (actual, expected) {
        // RenderContract is an internal, API-provided boundary rather than a
        // language-level tag set. Until the concrete API contract is present
        // in the callable interface, diagnostics must not invent special tag
        // knowledge (including `NoElement`).
        (Type::RenderContract, _) | (_, Type::RenderContract) => true,
        (Type::Union(actual), _) if actual.is_empty() => false,
        (_, Type::Union(expected)) if expected.is_empty() => false,
        (Type::Union(actual), Type::Union(expected)) => actual.iter().all(|actual| {
            expected
                .iter()
                .any(|expected| kernel_type_is_assignable_to(actual, expected))
        }),
        (Type::Union(actual), expected) => actual
            .iter()
            .all(|actual| kernel_type_is_assignable_to(actual, expected)),
        (actual, Type::Union(expected)) => expected
            .iter()
            .any(|expected| kernel_type_is_assignable_to(actual, expected)),
        (Type::Text, Type::Text) | (Type::Number, Type::Number) | (Type::Absent, Type::Absent) => {
            true
        }
        (Type::Bytes(actual), Type::Bytes(expected)) => match (actual, expected) {
            (_, BytesType::Dynamic) => true,
            (BytesType::Fixed(actual), BytesType::Fixed(expected)) => actual == expected,
            (BytesType::Dynamic, BytesType::Fixed(_)) => false,
        },
        (Type::Bits { width: actual }, Type::Bits { width: expected }) => actual == expected,
        (Type::List(actual), Type::List(expected)) | (Type::Set(actual), Type::Set(expected)) => {
            kernel_type_is_assignable_to(actual, expected)
        }
        (
            Type::Map {
                key: actual_key,
                value: actual_value,
            },
            Type::Map {
                key: expected_key,
                value: expected_value,
            },
        ) => {
            kernel_type_is_assignable_to(actual_key, expected_key)
                && kernel_type_is_assignable_to(actual_value, expected_value)
        }
        (Type::Object(actual), Type::Object(expected)) => {
            expected.fields.iter().all(|(name, expected_field)| {
                actual.fields.get(name).is_some_and(|actual_field| {
                    kernel_type_is_assignable_to(actual_field, expected_field)
                }) || actual.open
            })
        }
        (Type::VariantSet(actual), Type::Object(expected)) => actual.iter().all(|variant| {
            let fields = match variant {
                Variant::Tag(_) => return expected.fields.is_empty(),
                Variant::Tagged { fields, .. } => fields,
            };
            kernel_type_is_assignable_to(
                &Type::Object(fields.clone()),
                &Type::Object(expected.clone()),
            )
        }),
        (Type::VariantSet(actual), Type::VariantSet(expected)) => actual.iter().all(|actual| {
            expected
                .iter()
                .any(|expected| kernel_variant_is_assignable_to(actual, expected))
        }),
        (
            Type::Function {
                args: actual_args,
                result: actual_result,
            },
            Type::Function {
                args: expected_args,
                result: expected_result,
            },
        ) => {
            actual_args.len() == expected_args.len()
                && actual_result.mode == expected_result.mode
                && expected_args
                    .iter()
                    .zip(actual_args)
                    .all(|(expected, actual)| kernel_type_is_assignable_to(expected, actual))
                && kernel_type_is_assignable_to(&actual_result.ty, &expected_result.ty)
        }
        _ => false,
    }
}

fn kernel_variant_is_assignable_to(actual: &Variant, expected: &Variant) -> bool {
    match (actual, expected) {
        (Variant::Tag(actual), Variant::Tag(expected)) => actual == expected,
        (
            Variant::Tagged {
                tag: actual_tag,
                fields: actual_fields,
            },
            Variant::Tagged {
                tag: expected_tag,
                fields: expected_fields,
            },
        ) => {
            actual_tag == expected_tag
                && kernel_type_is_assignable_to(
                    &Type::Object(actual_fields.clone()),
                    &Type::Object(expected_fields.clone()),
                )
        }
        _ => false,
    }
}

fn kernel_type_mismatch(actual: &Type, expected: &Type) -> KernelTypeMismatch {
    if let Some(field) = kernel_missing_field_name(actual, expected) {
        KernelTypeMismatch::MissingField(field.into_boxed_str())
    } else if let Some(field) = kernel_incompatible_field_name(actual, expected) {
        KernelTypeMismatch::IncompatibleField(field.into_boxed_str())
    } else {
        KernelTypeMismatch::Type
    }
}

fn kernel_missing_field_name(actual: &Type, expected: &Type) -> Option<String> {
    let (Type::Object(actual), Type::Object(expected)) = (actual, expected) else {
        return None;
    };
    expected.fields.iter().find_map(|(name, expected_field)| {
        let Some(actual_field) = actual.fields.get(name) else {
            return (!actual.open).then(|| name.clone());
        };
        kernel_missing_field_name(actual_field, expected_field)
            .map(|nested| format!("{name}.{nested}"))
    })
}

fn kernel_incompatible_field_name(actual: &Type, expected: &Type) -> Option<String> {
    let (Type::Object(actual), Type::Object(expected)) = (actual, expected) else {
        return None;
    };
    expected.fields.iter().find_map(|(name, expected_field)| {
        let actual_field = actual.fields.get(name)?;
        if let Some(nested) = kernel_incompatible_field_name(actual_field, expected_field) {
            return Some(format!("{name}.{nested}"));
        }
        (!kernel_type_is_assignable_to(actual_field, expected_field)).then(|| name.clone())
    })
}

fn materialize_project_definition(
    owner_index: usize,
    owner: KernelProjectOwnerOutputs,
    artifact: &ComponentArtifact,
    public_results: &[FlowType],
    public_formals: &[Box<[FlowType]>],
    call_facts: Box<[SolvedKernelCallFacts]>,
    diagnostics: Box<[KernelDiagnosticArtifact]>,
) -> DefinitionArtifact {
    let result = public_results[owner_index].clone();
    let expression_flows = owner
        .expressions
        .iter()
        .zip(owner.expression_modes.iter().copied())
        .map(|(output, mode)| {
            let mut flow = artifact
                .output(*output)
                .expect("project owner expression belongs to its component")
                .flow_type
                .clone();
            flow.mode = mode;
            flow
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let calls = materialize_project_call_artifacts(owner.calls, call_facts, &expression_flows);
    let (sources, states, lists) =
        materialize_resource_artifacts(owner.resources, &expression_flows, Some(public_results));
    let expressions =
        materialize_expression_artifacts(owner.expression_artifacts, expression_flows);
    DefinitionArtifact {
        result,
        formals: public_formals[owner_index].clone(),
        relocations: owner.relocations,
        expressions,
        statements: owner.statements,
        declarations: owner.declarations,
        lexical_bindings: owner.lexical_bindings,
        calls,
        effects: owner.effects,
        sources,
        states,
        lists,
        diagnostics,
    }
}

pub fn compile_owner_program(
    input: &KernelOwnerProgramInput,
) -> Result<KernelOwnerProgram, KernelOwnerBuildError> {
    compile_owner_program_with_definition_facts(input, &KernelDefinitionFactsInput::default())
}

fn validate_definition_relocations(
    input: &KernelOwnerProgramInput,
    facts: &KernelDefinitionFactsInput,
    definition: Option<usize>,
) -> Result<(), KernelOwnerBuildError> {
    let relocations = &facts.relocations;
    if relocations.is_empty() {
        return Ok(());
    }
    let label = definition
        .map(|definition| format!("definition {definition}"))
        .unwrap_or_else(|| "standalone definition".to_owned());
    if !relocations.is_complete_for(input.nodes.len(), facts.statements.len()) {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel {label} has {} expression and {} statement relocations for {} expressions and {} statements",
            relocations.expressions.len(),
            relocations.statements.len(),
            input.nodes.len(),
            facts.statements.len(),
        )));
    }
    if relocations
        .expressions
        .iter()
        .collect::<BTreeSet<_>>()
        .len()
        != relocations.expressions.len()
    {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel {label} repeats a stable expression relocation"
        )));
    }
    if relocations.statements.iter().collect::<BTreeSet<_>>().len() != relocations.statements.len()
    {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel {label} repeats a stable statement relocation"
        )));
    }
    Ok(())
}

pub fn compile_owner_program_with_definition_facts(
    input: &KernelOwnerProgramInput,
    facts: &KernelDefinitionFactsInput,
) -> Result<KernelOwnerProgram, KernelOwnerBuildError> {
    validate_definition_relocations(input, facts, None)?;
    let basis_fingerprint_v4 = definition_basis_fingerprint(input, facts)?;
    if !input.external_expressions.is_empty() {
        return Err(KernelOwnerBuildError::new(
            "standalone owner program cannot import external expressions",
        ));
    }
    if facts.statements.iter().any(|statement| {
        statement
            .children
            .iter()
            .any(|child| matches!(child, KernelStatementChildReference::Owner(_)))
    }) {
        return Err(KernelOwnerBuildError::new(
            "standalone owner statements cannot reference child owners",
        ));
    }
    if facts.lexical_bindings.iter().any(|binding| {
        matches!(
            binding.target,
            KernelLexicalBindingTargetInput::Declaration(KernelDeclarationReference::OwnerPublic(
                _
            ))
        )
    }) {
        return Err(KernelOwnerBuildError::new(
            "standalone owner lexical bindings cannot reference another owner",
        ));
    }
    if facts.sources.iter().any(|source| {
        matches!(
            source.declaration,
            KernelDeclarationReference::OwnerPublic(_)
        ) || matches!(source.statement, KernelStatementReference::OwnerPublic(_))
    }) || facts.states.iter().any(|state| {
        matches!(
            state.declaration,
            KernelDeclarationReference::OwnerPublic(_)
        ) || matches!(
            state.binding_declaration,
            KernelDeclarationReference::OwnerPublic(_)
        ) || matches!(state.statement, KernelStatementReference::OwnerPublic(_))
    }) || facts.lists.iter().any(|list| {
        matches!(list.declaration, KernelDeclarationReference::OwnerPublic(_))
            || matches!(list.statement, KernelStatementReference::OwnerPublic(_))
    }) {
        return Err(KernelOwnerBuildError::new(
            "standalone owner resources cannot reference another owner",
        ));
    }
    let result = checked_expression_index(input.result, input.nodes.len(), "owner result")?;
    let mut builder = ComponentProgramBuilder::new();
    let mut mode_builder = ModeProgramBuilder::default();
    let formal_static_variants = vec![None; input.formal_count as usize];
    let formal_dependent_expressions = [owner_expressions_depend_on_formals(input)];
    let formal_dependent_results = [formal_dependent_expressions[0][result]];
    let principals = vec![allocate_owner_instance(
        &mut builder,
        &mut mode_builder,
        input,
        &formal_static_variants,
    )];
    let principal = &principals[0];
    let context = OwnerCompileContext {
        initial_state_surface: false,
        owner: KernelOwnerId(0),
        input,
        expressions: &principal.expressions,
        formals: &principal.formals,
        formal_requirements: &principal.formal_requirements,
        expression_modes: &principal.expression_modes,
        formal_modes: &principal.formal_modes,
        formal_mode_sources: &principal.formal_mode_sources,
        static_variants: &principal.static_variants,
        formal_static_variants: &principal.formal_static_variants,
        project: None,
        principals: &principals,
        formal_dependent_results: &formal_dependent_results,
        formal_dependent_expressions: &formal_dependent_expressions,
        external_variables: None,
        syntax_selected_calls: None,
        direct_summaries: &[],
    };
    let specialization = OwnerSpecialization {
        static_variants: principal.static_variants.clone(),
        reachable: (0..input.nodes.len())
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        syntax_selected_calls: vec![false; input.nodes.len()].into_boxed_slice(),
        invocation_dependencies: formal_dependent_expressions[0].clone(),
        transparent_type_providers: vec![None; input.nodes.len()].into_boxed_slice(),
    };
    let module = compile_residual_type_module(
        KernelOwnerId(0),
        input,
        None,
        &principals,
        &formal_dependent_results,
        &formal_dependent_expressions,
        &formal_static_variants,
        &specialization,
        None,
        false,
    )?;
    if let Some(call) = module.calls.first() {
        return Err(KernelOwnerBuildError::new(format!(
            "standalone owner node {call} cannot contain a user call"
        )));
    }
    append_residual_type_frame(&mut builder, &module, principal, &[])?;
    for (index, node) in input.nodes.iter().enumerate() {
        let equation = node_mode_equation(&mut mode_builder, &context, index, node)?;
        mode_builder.set(principal.expression_modes[index], equation);
    }

    let expression_outputs = principal
        .expressions
        .iter()
        .zip(input.nodes.iter())
        .map(|(variable, node)| builder.add_output(*variable, node.mode))
        .collect::<Vec<_>>();
    let formal_outputs = principal
        .formal_requirements
        .iter()
        .map(|variable| builder.add_output(*variable, FlowMode::Continuous))
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let modes = mode_builder.solve();
    let expression_modes = principal
        .expression_modes
        .iter()
        .map(|mode| modes[mode.0 as usize])
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let formal_modes = principal
        .formal_modes
        .iter()
        .map(|mode| modes[mode.0 as usize])
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let result_output = expression_outputs[result];
    let statements = collect_statement_artifacts(input, facts)?;
    let declarations = collect_declaration_artifacts(input, facts)?;
    let lexical_bindings = collect_lexical_binding_artifacts(input, facts)?;
    let resources = collect_resource_artifacts(input, facts)?;
    Ok(KernelOwnerProgram {
        component: builder.finish(),
        result_output,
        formal_outputs,
        formal_modes,
        expression_outputs: expression_outputs.into_boxed_slice(),
        expression_modes,
        expression_artifacts: collect_expression_artifacts(input)?,
        relocations: facts.relocations.clone(),
        statements,
        declarations,
        lexical_bindings,
        resources,
        calls: collect_call_artifacts(input)?,
        effects: collect_host_effect_artifacts(input)?,
        diagnostics: collect_definition_diagnostic_artifacts(KernelOwnerId(0), input, facts)?,
        basis_fingerprint_v4,
    })
}

fn materialize_expression_artifacts(
    pending: Box<[PendingKernelExpressionArtifact]>,
    flows: Box<[FlowType]>,
) -> Box<[KernelExpressionArtifact]> {
    assert_eq!(
        pending.len(),
        flows.len(),
        "every solved expression must retain one compact artifact row"
    );
    pending
        .into_vec()
        .into_iter()
        .zip(flows)
        .map(|(expression, flow_type)| KernelExpressionArtifact {
            id: expression.id,
            kind: expression.kind,
            inputs: expression.inputs,
            flow_type,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn collect_expression_artifacts(
    input: &KernelOwnerProgramInput,
) -> Result<Box<[PendingKernelExpressionArtifact]>, KernelOwnerBuildError> {
    input
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            Ok(PendingKernelExpressionArtifact {
                id: KernelExpressionId(
                    u32::try_from(index).expect("kernel owner expression count exceeds u32"),
                ),
                kind: node.kind.clone(),
                inputs: node
                    .inputs
                    .iter()
                    .map(|edge| {
                        Ok(KernelExpressionInputArtifact {
                            role: edge.role.clone(),
                            value: kernel_value_reference(input, edge.expression, index)?,
                        })
                    })
                    .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?
                    .into_boxed_slice(),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

fn kernel_value_reference(
    input: &KernelOwnerProgramInput,
    expression: KernelExpressionId,
    consumer: usize,
) -> Result<KernelValueReference, KernelOwnerBuildError> {
    let reference = expression.0 as usize;
    if reference < input.nodes.len() {
        return Ok(KernelValueReference::Local(expression));
    }
    input
        .external_expressions
        .get(reference - input.nodes.len())
        .copied()
        .map(KernelValueReference::External)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "kernel expression {consumer} input references expression {reference} outside the local and external namespaces"
            ))
        })
}

fn collect_statement_artifacts(
    owner: &KernelOwnerProgramInput,
    facts: &KernelDefinitionFactsInput,
) -> Result<Box<[KernelStatementArtifact]>, KernelOwnerBuildError> {
    let statement_count = facts.statements.len();
    let mut claimed_local_children = BTreeSet::new();
    for (index, statement) in facts.statements.iter().enumerate() {
        if statement.id.0 as usize != index {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel statement rows must use dense IDs: row {index} has ID {}",
                statement.id.0
            )));
        }
        for child in &statement.children {
            let KernelStatementChildReference::Local(child) = child else {
                continue;
            };
            if child.0 as usize >= statement_count {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel statement {index} references missing child statement {}",
                    child.0
                )));
            }
            if child.0 as usize <= index {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel statement {index} has non-forward child statement {}",
                    child.0
                )));
            }
            if !claimed_local_children.insert(*child) {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel statement {} has more than one local parent",
                    child.0
                )));
            }
        }
    }
    facts
        .statements
        .iter()
        .map(|statement| {
            Ok(KernelStatementArtifact {
                id: statement.id,
                kind: statement.kind.clone(),
                value: statement
                    .value
                    .map(|value| kernel_value_reference(owner, value, statement.id.0 as usize))
                    .transpose()?,
                children: statement.children.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

fn collect_declaration_artifacts(
    owner: &KernelOwnerProgramInput,
    facts: &KernelDefinitionFactsInput,
) -> Result<Box<[KernelDeclarationArtifact]>, KernelOwnerBuildError> {
    let mut origins = BTreeSet::new();
    facts
        .declarations
        .iter()
        .enumerate()
        .map(|(index, declaration)| {
            if declaration.id.0 as usize != index {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel declaration rows must use dense IDs: row {index} has ID {}",
                    declaration.id.0
                )));
            }
            if !origins.insert(declaration.origin.clone()) {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel declaration {index} repeats structural origin {:?}",
                    declaration.origin
                )));
            }
            validate_declaration_origin(owner, facts, declaration)?;
            Ok(KernelDeclarationArtifact {
                id: declaration.id,
                origin: declaration.origin.clone(),
                name: declaration.name.clone(),
                kind: declaration.kind,
                value: declaration
                    .value
                    .map(|value| kernel_value_reference(owner, value, index))
                    .transpose()?,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

fn validate_declaration_origin(
    owner: &KernelOwnerProgramInput,
    facts: &KernelDefinitionFactsInput,
    declaration: &KernelDeclarationInput,
) -> Result<(), KernelOwnerBuildError> {
    let invalid = || {
        KernelOwnerBuildError::new(format!(
            "kernel declaration {} kind {:?} has incompatible origin {:?}",
            declaration.id.0, declaration.kind, declaration.origin
        ))
    };
    match (&declaration.origin, declaration.kind) {
        (
            KernelDeclarationOrigin::Statement { statement },
            KernelDeclarationKind::Function
            | KernelDeclarationKind::Field
            | KernelDeclarationKind::Source
            | KernelDeclarationKind::Hold
            | KernelDeclarationKind::List,
        ) => {
            let statement = facts.statements.get(statement.0 as usize).ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel declaration {} references missing statement {}",
                    declaration.id.0, statement.0
                ))
            })?;
            let matches = match (&statement.kind, declaration.kind) {
                (KernelStatementKind::Function { name, .. }, KernelDeclarationKind::Function)
                | (KernelStatementKind::Field { name }, KernelDeclarationKind::Field) => {
                    name == &declaration.name
                }
                (
                    KernelStatementKind::Source {
                        field: Some(name), ..
                    },
                    KernelDeclarationKind::Source,
                )
                | (
                    KernelStatementKind::Hold {
                        field: Some(name), ..
                    },
                    KernelDeclarationKind::Hold,
                )
                | (
                    KernelStatementKind::List {
                        field: Some(name), ..
                    },
                    KernelDeclarationKind::List,
                ) => name == &declaration.name,
                // An authored fieldless HOLD alias can be the private state
                // declaration for its lexical region. The syntax projection
                // decides whether this statement owns that authority; the
                // kernel validates the exact alias/statement identity here.
                (
                    KernelStatementKind::Hold {
                        field: None,
                        name: Some(name),
                    },
                    KernelDeclarationKind::Hold,
                ) => name == &declaration.name,
                _ => false,
            };
            if !matches {
                return Err(invalid());
            }
        }
        (
            KernelDeclarationOrigin::Parameter { statement, ordinal },
            KernelDeclarationKind::ValueParameter | KernelDeclarationKind::OutParameter,
        ) => {
            let Some(KernelStatementInput {
                kind: KernelStatementKind::Function { parameters, .. },
                ..
            }) = facts.statements.get(statement.0 as usize)
            else {
                return Err(invalid());
            };
            let parameter = parameters
                .iter()
                .find(|parameter| parameter.ordinal == *ordinal)
                .ok_or_else(invalid)?;
            let expected_kind = match parameter.kind {
                KernelParameterKind::Value => KernelDeclarationKind::ValueParameter,
                KernelParameterKind::Out => KernelDeclarationKind::OutParameter,
            };
            if expected_kind != declaration.kind || parameter.name != declaration.name {
                return Err(invalid());
            }
        }
        (
            KernelDeclarationOrigin::RecordField { object, ordinal },
            KernelDeclarationKind::Field,
        ) => {
            let expression = owner.nodes.get(object.0 as usize).ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel declaration {} references missing record expression {}",
                    declaration.id.0, object.0
                ))
            })?;
            if !matches!(expression.kind, KernelOwnerNodeKind::Record { .. }) {
                return Err(invalid());
            }
            let field = expression
                .inputs
                .get(*ordinal as usize)
                .ok_or_else(invalid)?;
            if !matches!(
                &field.role,
                KernelOwnerEdgeRole::RecordField { name, spread: false }
                    if name == &declaration.name
            ) {
                return Err(invalid());
            }
        }
        (
            KernelDeclarationOrigin::PatternBinding { arm, ordinal },
            KernelDeclarationKind::PatternBinding,
        ) => {
            let expression = owner.nodes.get(arm.0 as usize).ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel declaration {} references missing match arm {}",
                    declaration.id.0, arm.0
                ))
            })?;
            let KernelOwnerNodeKind::MatchArm { pattern } = &expression.kind else {
                return Err(invalid());
            };
            let names = match pattern {
                KernelPattern::Binding { name } => std::slice::from_ref(name),
                KernelPattern::Tag { fields, .. } => fields,
                KernelPattern::Wildcard
                | KernelPattern::Number
                | KernelPattern::Text
                | KernelPattern::Bits { .. }
                | KernelPattern::Invalid => &[],
            };
            if names.get(*ordinal as usize).map(Box::as_ref) != Some(declaration.name.as_ref()) {
                return Err(invalid());
            }
        }
        (
            KernelDeclarationOrigin::CallbackBinding { call, .. },
            KernelDeclarationKind::FreshOut,
        ) => {
            let expression = owner.nodes.get(call.0 as usize).ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel declaration {} references missing callback call {}",
                    declaration.id.0, call.0
                ))
            })?;
            if !matches!(
                expression.kind,
                KernelOwnerNodeKind::PureBuiltin { .. } | KernelOwnerNodeKind::UserCall { .. }
            ) {
                return Err(invalid());
            }
        }
        _ => return Err(invalid()),
    }
    Ok(())
}

fn collect_lexical_binding_artifacts(
    owner: &KernelOwnerProgramInput,
    facts: &KernelDefinitionFactsInput,
) -> Result<Box<[KernelLexicalBindingArtifact]>, KernelOwnerBuildError> {
    let mut expressions = BTreeSet::new();
    facts
        .lexical_bindings
        .iter()
        .map(|binding| {
            let expression = checked_expression_index(
                binding.expression,
                owner.nodes.len(),
                "kernel lexical binding expression",
            )?;
            if !expressions.insert(binding.expression) {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel expression {expression} has more than one lexical binding row"
                )));
            }
            let target = match binding.target {
                KernelLexicalBindingTargetInput::Declaration(reference) => {
                    if let KernelDeclarationReference::Local(declaration) = reference
                        && declaration.0 as usize >= facts.declarations.len()
                    {
                        return Err(KernelOwnerBuildError::new(format!(
                            "kernel expression {expression} references missing declaration {}",
                            declaration.0
                        )));
                    }
                    KernelLexicalBindingTarget::Declaration(reference)
                }
                KernelLexicalBindingTargetInput::ContextFormal { ordinal } => {
                    if ordinal >= owner.formal_count {
                        return Err(KernelOwnerBuildError::new(format!(
                            "kernel expression {expression} references missing context formal {ordinal}"
                        )));
                    }
                    KernelLexicalBindingTarget::ContextFormal { ordinal }
                }
                KernelLexicalBindingTargetInput::Value { provider } => {
                    KernelLexicalBindingTarget::Value {
                        provider: kernel_value_reference(owner, provider, expression)?,
                    }
                }
                KernelLexicalBindingTargetInput::RuntimeContext => {
                    KernelLexicalBindingTarget::RuntimeContext
                }
            };
            Ok(KernelLexicalBindingArtifact {
                expression: binding.expression,
                target,
                projection: binding.projection.clone(),
                access: binding.access,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

fn validate_resource_declaration(
    facts: &KernelDefinitionFactsInput,
    declaration: KernelDeclarationReference,
    context: &str,
) -> Result<(), KernelOwnerBuildError> {
    if let KernelDeclarationReference::Local(declaration) = declaration
        && declaration.0 as usize >= facts.declarations.len()
    {
        return Err(KernelOwnerBuildError::new(format!(
            "{context} references missing declaration {}",
            declaration.0
        )));
    }
    Ok(())
}

fn validate_resource_statement(
    facts: &KernelDefinitionFactsInput,
    statement: KernelStatementReference,
    context: &str,
) -> Result<(), KernelOwnerBuildError> {
    let KernelStatementReference::Local(statement) = statement else {
        return Ok(());
    };
    if statement.0 as usize >= facts.statements.len() {
        return Err(KernelOwnerBuildError::new(format!(
            "{context} references missing statement {}",
            statement.0
        )));
    }
    Ok(())
}

fn collect_resource_artifacts(
    owner: &KernelOwnerProgramInput,
    facts: &KernelDefinitionFactsInput,
) -> Result<PendingKernelResources, KernelOwnerBuildError> {
    let mut resource_expressions = BTreeSet::new();
    let sources = facts
        .sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            if source.id.0 as usize != index {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel SOURCE rows must use dense IDs: row {index} has ID {}",
                    source.id.0
                )));
            }
            validate_resource_declaration(facts, source.declaration, "kernel SOURCE row")?;
            validate_resource_statement(facts, source.statement, "kernel SOURCE row")?;
            let expression = checked_expression_index(
                source.expression,
                owner.nodes.len(),
                "kernel SOURCE expression",
            )?;
            if !matches!(owner.nodes[expression].kind, KernelOwnerNodeKind::Source(_)) {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel SOURCE row {index} expression {expression} is not a literal SOURCE"
                )));
            }
            if !resource_expressions.insert((0u8, source.expression)) {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel SOURCE expression {expression} is published more than once"
                )));
            }
            Ok(PendingKernelSourceArtifact {
                id: source.id,
                declaration: source.declaration,
                statement: source.statement,
                expression: source.expression,
                path: KernelSemanticPath {
                    anchor: source.declaration,
                    projection: source.projection.clone(),
                },
                interval_ms: source.interval_ms,
            })
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();

    let states = facts
        .states
        .iter()
        .enumerate()
        .map(|(index, state)| {
            if state.id.0 as usize != index {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel state rows must use dense IDs: row {index} has ID {}",
                    state.id.0
                )));
            }
            validate_resource_declaration(
                facts,
                state.binding_declaration,
                "kernel state binding",
            )?;
            validate_resource_declaration(facts, state.declaration, "kernel state row")?;
            validate_resource_statement(facts, state.statement, "kernel state row")?;
            let expression = checked_expression_index(
                state.expression,
                owner.nodes.len(),
                "kernel state expression",
            )?;
            if !matches!(
                (&owner.nodes[expression].kind, state.kind),
                (KernelOwnerNodeKind::Hold, CheckedStateKind::Hold)
            ) {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel state row {index} kind {:?} is incompatible with expression {expression} kind {:?}",
                    state.kind, owner.nodes[expression].kind
                )));
            }
            if !resource_expressions.insert((1u8, state.expression)) {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel state expression {expression} is published more than once"
                )));
            }
            let initial = kernel_value_reference(owner, state.initial, expression)?;
            Ok(PendingKernelStateArtifact {
                id: state.id,
                binding_declaration: state.binding_declaration,
                declaration: state.declaration,
                statement: state.statement,
                expression: state.expression,
                initial,
                path: KernelSemanticPath {
                    anchor: state.declaration,
                    projection: state.projection.clone(),
                },
                kind: state.kind,
            })
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();

    let lists = facts
        .lists
        .iter()
        .enumerate()
        .map(|(index, list)| {
            if list.id.0 as usize != index {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel LIST rows must use dense IDs: row {index} has ID {}",
                    list.id.0
                )));
            }
            validate_resource_declaration(facts, list.declaration, "kernel LIST row")?;
            validate_resource_statement(facts, list.statement, "kernel LIST row")?;
            let producer =
                checked_expression_index(list.producer, owner.nodes.len(), "kernel LIST producer")?;
            let KernelOwnerNodeKind::Collection {
                kind: KernelCollectionKind::List,
                capacity,
            } = &owner.nodes[producer].kind
            else {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel LIST row {index} producer {producer} is not a list literal"
                )));
            };
            if capacity != &list.capacity {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel LIST row {index} capacity {:?} differs from producer capacity {:?}",
                    list.capacity, capacity
                )));
            }
            if !resource_expressions.insert((2u8, list.producer)) {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel LIST producer {producer} is published more than once"
                )));
            }
            Ok(PendingKernelListArtifact {
                id: list.id,
                declaration: list.declaration,
                statement: list.statement,
                producer: list.producer,
                path: KernelSemanticPath {
                    anchor: list.declaration,
                    projection: list.projection.clone(),
                },
                capacity: list.capacity,
                key_policy: list.key_policy,
            })
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_boxed_slice();

    Ok(PendingKernelResources {
        sources,
        states,
        lists,
    })
}

fn materialize_resource_artifacts(
    pending: PendingKernelResources,
    expressions: &[FlowType],
    public_results: Option<&[FlowType]>,
) -> (
    Box<[KernelSourceArtifact]>,
    Box<[KernelStateArtifact]>,
    Box<[KernelListArtifact]>,
) {
    let sources = pending
        .sources
        .into_vec()
        .into_iter()
        .map(|source| KernelSourceArtifact {
            id: source.id,
            declaration: source.declaration,
            statement: source.statement,
            expression: source.expression,
            path: source.path,
            interval_ms: source.interval_ms,
            payload_type: expressions[source.expression.0 as usize].ty.clone(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let states = pending
        .states
        .into_vec()
        .into_iter()
        .map(|state| KernelStateArtifact {
            id: state.id,
            binding_declaration: state.binding_declaration,
            declaration: state.declaration,
            statement: state.statement,
            expression: state.expression,
            initial: state.initial,
            path: state.path,
            kind: state.kind,
            flow_type: expressions[state.expression.0 as usize].clone(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let lists = pending
        .lists
        .into_vec()
        .into_iter()
        .map(|list| {
            let producer = &expressions[list.producer.0 as usize];
            let authority = match list.declaration {
                KernelDeclarationReference::OwnerPublic(owner) => public_results
                    .and_then(|results| results.get(owner.0 as usize))
                    .filter(|result| matches!(result.ty, Type::List(_)))
                    .unwrap_or(producer),
                KernelDeclarationReference::Local(_) => producer,
            };
            let Type::List(item_type) = &authority.ty else {
                unreachable!("a solved LIST literal must retain a List type")
            };
            KernelListArtifact {
                id: list.id,
                declaration: list.declaration,
                statement: list.statement,
                producer: list.producer,
                path: list.path,
                item_type: item_type.as_ref().clone(),
                capacity: list.capacity,
                key_policy: list.key_policy,
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    (sources, states, lists)
}

fn materialize_call_artifacts(
    owner: KernelOwnerId,
    pending: Box<[PendingKernelCallArtifact]>,
    expressions: &[FlowType],
    public_formals: &[Box<[FlowType]>],
    public_results: &[FlowType],
) -> Box<[KernelCallArtifact]> {
    pending
        .into_vec()
        .into_iter()
        .map(|call| {
            let type_substitutions = materialize_user_call_type_substitutions(
                owner,
                &call,
                expressions,
                public_formals,
                public_results,
            );
            KernelCallArtifact {
                expression: call.expression,
                target: call.target,
                inputs: call.inputs,
                type_substitutions,
                result: expressions[call.expression.0 as usize].clone(),
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn materialize_project_call_artifacts(
    pending: Box<[PendingKernelCallArtifact]>,
    facts: Box<[SolvedKernelCallFacts]>,
    expressions: &[FlowType],
) -> Box<[KernelCallArtifact]> {
    assert_eq!(
        pending.len(),
        facts.len(),
        "solved kernel call facts must align with compact call rows"
    );
    pending
        .into_vec()
        .into_iter()
        .zip(facts)
        .map(|(call, facts)| KernelCallArtifact {
            expression: call.expression,
            target: call.target,
            inputs: call.inputs,
            type_substitutions: facts.type_substitutions,
            result: expressions[call.expression.0 as usize].clone(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn materialize_user_call_type_substitutions(
    owner: KernelOwnerId,
    call: &PendingKernelCallArtifact,
    expressions: &[FlowType],
    public_formals: &[Box<[FlowType]>],
    public_results: &[FlowType],
) -> Box<[KernelCallTypeSubstitution]> {
    let KernelCallTarget::User {
        target,
        inherited_formal,
    } = call.target
    else {
        return Box::new([]);
    };
    let Some(target_formals) = public_formals.get(target.0 as usize) else {
        return Box::new([]);
    };
    let Some(target_result) = public_results.get(target.0 as usize) else {
        return Box::new([]);
    };

    let mut actuals =
        Vec::with_capacity(call.inputs.len() + usize::from(inherited_formal.is_some()));
    for input in call.inputs.iter() {
        let KernelCallInputRole::Formal { ordinal } = input.role else {
            continue;
        };
        let Some(actual) = call_value_flow(input.value, expressions, public_results) else {
            continue;
        };
        actuals.push((ordinal, actual.ty.clone()));
    }
    if let Some(inherited) = inherited_formal
        && let Some(actual) = public_formals
            .get(owner.0 as usize)
            .and_then(|formals| formals.get(inherited.caller_ordinal as usize))
    {
        actuals.push((inherited.target_ordinal, actual.ty.clone()));
    }

    derive_kernel_call_type_substitutions(target_formals, target_result, &actuals)
}

/// Derive the canonical substitution environment for one callable
/// occurrence. The callable and actual type-variable namespaces must be
/// disjoint; production solver arenas guarantee that, while differential
/// adapters alpha-isolate legacy definition-local schemes before calling.
pub fn derive_kernel_call_type_substitutions(
    target_formals: &[FlowType],
    target_result: &FlowType,
    actuals: &[(u32, Type)],
) -> Box<[KernelCallTypeSubstitution]> {
    let mut parameter_ids = BTreeMap::new();
    for formal in target_formals {
        collect_callable_type_parameters(&formal.ty, &mut parameter_ids);
    }
    collect_callable_type_parameters(&target_result.ty, &mut parameter_ids);

    let mut substitutions = BTreeMap::new();
    for (ordinal, actual) in actuals {
        let Some(pattern) = target_formals.get(*ordinal as usize) else {
            continue;
        };
        match_call_type_pattern(&pattern.ty, actual, &mut substitutions);
    }

    let mut substitutions = substitutions
        .into_iter()
        .filter_map(|(variable, value)| {
            parameter_ids
                .get(&variable)
                .copied()
                .map(|variable| KernelCallTypeSubstitution { variable, value })
        })
        .collect::<Vec<_>>();
    substitutions.sort_unstable_by_key(|substitution| substitution.variable);
    substitutions.into_boxed_slice()
}

fn call_value_flow<'a>(
    value: KernelValueReference,
    expressions: &'a [FlowType],
    public_results: &'a [FlowType],
) -> Option<&'a FlowType> {
    match value {
        KernelValueReference::Local(expression) => expressions.get(expression.0 as usize),
        KernelValueReference::External(KernelExternalExpression {
            owner,
            target: KernelExternalTarget::Result,
        }) => public_results.get(owner.0 as usize),
        KernelValueReference::External(KernelExternalExpression {
            target: KernelExternalTarget::Expression(_),
            ..
        }) => None,
    }
}

fn collect_callable_type_parameters(
    ty: &Type,
    parameters: &mut BTreeMap<boon_checked::TypeVar, KernelTypeParameterId>,
) {
    match ty {
        Type::Var(variable) => {
            let next = KernelTypeParameterId(
                u32::try_from(parameters.len())
                    .expect("kernel callable type-parameter count exceeds u32"),
            );
            parameters.entry(*variable).or_insert(next);
        }
        Type::Object(shape) => {
            for field in shape.ordered_fields().into_iter().map(|(_, field)| field) {
                collect_callable_type_parameters(field, parameters);
            }
        }
        Type::List(item) | Type::Set(item) => {
            collect_callable_type_parameters(item, parameters);
        }
        Type::Map { key, value } => {
            collect_callable_type_parameters(key, parameters);
            collect_callable_type_parameters(value, parameters);
        }
        Type::Function { args, result } => {
            for argument in args {
                collect_callable_type_parameters(argument, parameters);
            }
            collect_callable_type_parameters(&result.ty, parameters);
        }
        Type::VariantSet(variants) => {
            for variant in variants {
                if let Variant::Tagged { fields, .. } = variant {
                    for field in fields.ordered_fields().into_iter().map(|(_, field)| field) {
                        collect_callable_type_parameters(field, parameters);
                    }
                }
            }
        }
        Type::Union(members) => {
            for member in members {
                collect_callable_type_parameters(member, parameters);
            }
        }
        Type::Text
        | Type::Number
        | Type::Bytes(_)
        | Type::Bits { .. }
        | Type::Absent
        | Type::RenderContract
        | Type::UnresolvedShape { .. }
        | Type::Unknown => {}
    }
}

fn call_type_is_placeholder(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Unknown | Type::Var(_) | Type::UnresolvedShape { .. }
    ) || matches!(ty, Type::Object(shape) if shape.open && shape.fields.is_empty())
}

fn match_call_type_pattern(
    pattern: &Type,
    actual: &Type,
    substitutions: &mut BTreeMap<boon_checked::TypeVar, Type>,
) {
    match (pattern, actual) {
        (Type::Var(_), Type::Unknown | Type::UnresolvedShape { .. } | Type::Absent) => {}
        (Type::Var(variable), actual) => {
            if substitutions
                .get(variable)
                .is_none_or(call_type_is_placeholder)
            {
                substitutions.insert(*variable, actual.clone());
            }
        }
        (Type::List(pattern), Type::List(actual)) | (Type::Set(pattern), Type::Set(actual)) => {
            match_call_type_pattern(pattern, actual, substitutions);
        }
        (
            Type::Map {
                key: pattern_key,
                value: pattern_value,
            },
            Type::Map {
                key: actual_key,
                value: actual_value,
            },
        ) => {
            match_call_type_pattern(pattern_key, actual_key, substitutions);
            match_call_type_pattern(pattern_value, actual_value, substitutions);
        }
        (
            Type::Function {
                args: pattern_args,
                result: pattern_result,
            },
            Type::Function {
                args: actual_args,
                result: actual_result,
            },
        ) if pattern_args.len() == actual_args.len() => {
            for (pattern, actual) in pattern_args.iter().zip(actual_args) {
                match_call_type_pattern(pattern, actual, substitutions);
            }
            match_call_type_pattern(&pattern_result.ty, &actual_result.ty, substitutions);
        }
        (Type::Object(pattern), Type::Object(actual)) => {
            for (name, pattern) in &pattern.fields {
                if let Some(actual) = actual.fields.get(name) {
                    match_call_type_pattern(pattern, actual, substitutions);
                }
            }
        }
        (Type::VariantSet(pattern), Type::VariantSet(actual)) => {
            for pattern in pattern {
                let Variant::Tagged {
                    tag: pattern_tag,
                    fields: pattern_fields,
                } = pattern
                else {
                    continue;
                };
                let Some(Variant::Tagged {
                    fields: actual_fields,
                    ..
                }) = actual.iter().find(
                    |variant| matches!(variant, Variant::Tagged { tag, .. } if tag == pattern_tag),
                )
                else {
                    continue;
                };
                for (name, pattern) in &pattern_fields.fields {
                    if let Some(actual) = actual_fields.fields.get(name) {
                        match_call_type_pattern(pattern, actual, substitutions);
                    }
                }
            }
        }
        (Type::Union(pattern), Type::Union(actual)) => {
            for actual in actual {
                if let Some(pattern) = pattern
                    .iter()
                    .find(|pattern| call_type_pattern_accepts(pattern, actual))
                {
                    match_call_type_pattern(pattern, actual, substitutions);
                }
            }
        }
        (Type::Union(pattern), actual) => {
            if let Some(pattern) = pattern
                .iter()
                .find(|pattern| call_type_pattern_accepts(pattern, actual))
            {
                match_call_type_pattern(pattern, actual, substitutions);
            }
        }
        (pattern, Type::Union(actual)) => {
            for actual in actual
                .iter()
                .filter(|actual| call_type_pattern_accepts(pattern, actual))
            {
                match_call_type_pattern(pattern, actual, substitutions);
            }
        }
        _ => {}
    }
}

fn call_type_pattern_accepts(pattern: &Type, actual: &Type) -> bool {
    match (pattern, actual) {
        (Type::Var(_), Type::Unknown | Type::UnresolvedShape { .. } | Type::Absent) => false,
        (Type::Var(_), _) => true,
        (Type::List(pattern), Type::List(actual)) | (Type::Set(pattern), Type::Set(actual)) => {
            call_type_pattern_accepts(pattern, actual)
        }
        (Type::Object(pattern), Type::Object(actual)) => pattern.fields.iter().all(|(name, ty)| {
            actual
                .fields
                .get(name)
                .is_some_and(|actual| call_type_pattern_accepts(ty, actual))
        }),
        (Type::Union(pattern), actual) => pattern
            .iter()
            .any(|pattern| call_type_pattern_accepts(pattern, actual)),
        (pattern, Type::Union(actual)) => actual
            .iter()
            .all(|actual| call_type_pattern_accepts(pattern, actual)),
        _ => pattern == actual || boon_checked::resolved_type_is_assignable_to(actual, pattern),
    }
}

fn collect_call_artifacts(
    input: &KernelOwnerProgramInput,
) -> Result<Box<[PendingKernelCallArtifact]>, KernelOwnerBuildError> {
    input
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(expression, node)| {
            let (target, uses_abi_inputs) = match &node.kind {
                KernelOwnerNodeKind::UserCall {
                    target,
                    inherited_formal,
                } => (
                    KernelCallTarget::User {
                        target: *target,
                        inherited_formal: *inherited_formal,
                    },
                    false,
                ),
                KernelOwnerNodeKind::RenderConstructor { kind } => (
                    KernelCallTarget::RenderConstructor { kind: kind.clone() },
                    true,
                ),
                KernelOwnerNodeKind::PureBuiltin { kind } => {
                    (KernelCallTarget::PureBuiltin { kind: *kind }, true)
                }
                KernelOwnerNodeKind::HostEffect { operation } => (
                    KernelCallTarget::HostEffect {
                        operation: operation.clone(),
                    },
                    true,
                ),
                _ => return None,
            };
            Some((|| {
                let mut inputs = Vec::with_capacity(node.inputs.len());
                for edge in &node.inputs {
                    let role = match &edge.role {
                        KernelOwnerEdgeRole::CallArgument { ordinal }
                        | KernelOwnerEdgeRole::CallOutArgument { ordinal }
                            if !uses_abi_inputs =>
                        {
                            KernelCallInputRole::Formal { ordinal: *ordinal }
                        }
                        KernelOwnerEdgeRole::AbiArgument { name } if uses_abi_inputs => {
                            KernelCallInputRole::Abi { name: name.clone() }
                        }
                        role => {
                            return Err(KernelOwnerBuildError::new(format!(
                                "kernel call node {expression} has non-call input role {role:?}"
                            )));
                        }
                    };
                    let value = kernel_value_reference(input, edge.expression, expression)?;
                    inputs.push(KernelCallInputArtifact { role, value });
                }
                Ok(PendingKernelCallArtifact {
                    expression: KernelExpressionId(
                        u32::try_from(expression)
                            .expect("kernel owner expression count exceeds u32"),
                    ),
                    target,
                    inputs: inputs.into_boxed_slice(),
                })
            })())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

fn collect_definition_diagnostic_artifacts(
    owner: KernelOwnerId,
    input: &KernelOwnerProgramInput,
    facts: &KernelDefinitionFactsInput,
) -> Result<Box<[KernelDiagnosticArtifact]>, KernelOwnerBuildError> {
    facts
        .diagnostics
        .iter()
        .map(|diagnostic| {
            match diagnostic.site {
                KernelDiagnosticSite::Expression { expression } => {
                    checked_expression_index(
                        expression,
                        input.nodes.len(),
                        "source diagnostic expression",
                    )?;
                }
                KernelDiagnosticSite::CallArgument { call, .. }
                | KernelDiagnosticSite::CallPass { call, .. } => {
                    checked_expression_index(
                        call,
                        input.nodes.len(),
                        "call diagnostic expression",
                    )?;
                }
                KernelDiagnosticSite::CallInput { .. } => {
                    return Err(KernelOwnerBuildError::new(
                        "definition diagnostic inputs cannot precompute solved call-input failures",
                    ));
                }
            }
            Ok(KernelDiagnosticArtifact {
                owner,
                severity: diagnostic.severity,
                site: diagnostic.site.clone(),
                kind: diagnostic.kind.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

fn collect_host_effect_artifacts(
    input: &KernelOwnerProgramInput,
) -> Result<Box<[KernelHostEffectArtifact]>, KernelOwnerBuildError> {
    input
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(expression, node)| {
            let KernelOwnerNodeKind::HostEffect { operation } = &node.kind else {
                return None;
            };
            Some((|| {
                let spec = host_effect_spec(operation).ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel host-effect node {expression} names unknown operation `{operation}`"
                    ))
                })?;
                if spec.result_policy != ResultPolicySpec::ReturnValue || spec.schema.is_none() {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel host-effect node {expression} operation `{operation}` has no return-value schema"
                    )));
                }
                Ok(KernelHostEffectArtifact {
                    expression: KernelExpressionId(
                        u32::try_from(expression)
                            .expect("kernel owner expression count exceeds u32"),
                    ),
                    operation: spec.operation.into(),
                    replay: spec.replay,
                    barrier: spec.barrier,
                    result_policy: spec.result_policy,
                    delivery: spec.delivery,
                })
            })())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

fn expression_formal_dependencies(
    owner: &KernelOwnerProgramInput,
    root: KernelExpressionId,
) -> BTreeSet<u32> {
    let mut dependencies = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut pending = vec![root.0 as usize];
    while let Some(expression) = pending.pop() {
        if expression >= owner.nodes.len() || !visited.insert(expression) {
            continue;
        }
        let node = &owner.nodes[expression];
        match &node.kind {
            KernelOwnerNodeKind::FormalRead { formal, .. }
            | KernelOwnerNodeKind::ContextRead { formal, .. } => {
                dependencies.insert(*formal);
            }
            _ => {}
        }
        pending.extend(node.inputs.iter().filter_map(|input| {
            let input = input.expression.0 as usize;
            (input < owner.nodes.len()).then_some(input)
        }));
    }
    dependencies
}

fn expression_contains_nested_formal_read(
    owner: &KernelOwnerProgramInput,
    root: KernelExpressionId,
    formal: u32,
) -> bool {
    let mut visited = BTreeSet::new();
    let mut pending = vec![root.0 as usize];
    while let Some(expression) = pending.pop() {
        if expression >= owner.nodes.len() || !visited.insert(expression) {
            continue;
        }
        let node = &owner.nodes[expression];
        if matches!(
            &node.kind,
            KernelOwnerNodeKind::FormalRead {
                formal: candidate,
                fields,
            } | KernelOwnerNodeKind::ContextRead {
                formal: candidate,
                fields,
            } if *candidate == formal && !fields.is_empty()
        ) {
            return true;
        }
        pending.extend(node.inputs.iter().filter_map(|input| {
            let input = input.expression.0 as usize;
            (input < owner.nodes.len()).then_some(input)
        }));
    }
    false
}

/// Find formal contracts whose field requirements are guarded by syntax
/// selection, then propagate that fact through direct user-call arguments.
///
/// A `WHEN` such as `value |> WHEN { A => value.a; B => value.b }` shapes the
/// principal formal as an object containing both `a` and `b`. Requiring every
/// member of an actual `A[a] | B[b]` value to contain both fields would turn a
/// valid branch-discriminated call into a false diagnostic. Wrapper functions
/// inherit the same property when they forward one of their formals into that
/// parameter, so this is a small fixed point over the packed call graph.
fn project_syntax_discriminated_formals(input: &KernelProjectProgramInput) -> Vec<Box<[u32]>> {
    let mut formals = vec![BTreeSet::<u32>::new(); input.owners.len()];
    for (owner_index, owner) in input.owners.iter().enumerate() {
        for node in owner
            .nodes
            .iter()
            .filter(|node| matches!(node.kind, KernelOwnerNodeKind::When))
        {
            for selector in node
                .inputs
                .iter()
                .filter(|input| matches!(input.role, KernelOwnerEdgeRole::WhenInput))
            {
                for formal in expression_formal_dependencies(owner, selector.expression) {
                    let has_branch_projection = node
                        .inputs
                        .iter()
                        .filter(|input| matches!(input.role, KernelOwnerEdgeRole::WhenArm))
                        .filter_map(|arm| owner.nodes.get(arm.expression.0 as usize))
                        .flat_map(|arm| {
                            arm.inputs.iter().filter(|input| {
                                matches!(input.role, KernelOwnerEdgeRole::MatchOutput)
                            })
                        })
                        .any(|output| {
                            expression_contains_nested_formal_read(owner, output.expression, formal)
                        });
                    if has_branch_projection {
                        formals[owner_index].insert(formal);
                    }
                }
            }
        }
    }

    loop {
        let mut changed = false;
        for (owner_index, owner) in input.owners.iter().enumerate() {
            for node in &owner.nodes {
                let KernelOwnerNodeKind::UserCall {
                    target,
                    inherited_formal,
                } = &node.kind
                else {
                    continue;
                };
                let Some(target_formals) = formals.get(target.0 as usize) else {
                    continue;
                };
                let inherited_is_discriminated = inherited_formal
                    .filter(|inherited| target_formals.contains(&inherited.target_ordinal));
                let discriminated_inputs = node
                    .inputs
                    .iter()
                    .filter_map(|input| {
                        let KernelOwnerEdgeRole::CallArgument { ordinal } = input.role else {
                            return None;
                        };
                        target_formals
                            .contains(&ordinal)
                            .then_some(input.expression)
                    })
                    .collect::<Vec<_>>();
                let mut propagated = discriminated_inputs
                    .into_iter()
                    .flat_map(|expression| expression_formal_dependencies(owner, expression))
                    .collect::<BTreeSet<_>>();
                if let Some(inherited) = inherited_is_discriminated {
                    propagated.insert(inherited.caller_ordinal);
                }
                let before = formals[owner_index].len();
                formals[owner_index].extend(propagated);
                changed |= formals[owner_index].len() != before;
            }
        }
        if !changed {
            break;
        }
    }

    formals
        .into_iter()
        .map(|formals| formals.into_iter().collect::<Vec<_>>().into_boxed_slice())
        .collect()
}

pub fn compile_project_program(
    input: &KernelProjectProgramInput,
) -> Result<KernelProjectProgram, KernelOwnerBuildError> {
    let facts = (0..input.owners.len())
        .map(|_| KernelDefinitionFactsInput::default())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    compile_project_program_with_definition_facts(input, &facts)
}

pub fn compile_project_program_with_definition_facts(
    input: &KernelProjectProgramInput,
    facts: &[KernelDefinitionFactsInput],
) -> Result<KernelProjectProgram, KernelOwnerBuildError> {
    if facts.len() != input.owners.len() {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel project has {} owners but {} definition-fact tables",
            input.owners.len(),
            facts.len()
        )));
    }
    for (definition, facts) in facts.iter().enumerate() {
        validate_definition_relocations(&input.owners[definition], facts, Some(definition))?;
        for statement in &facts.statements {
            for child in &statement.children {
                if let KernelStatementChildReference::Owner(owner) = child
                    && owner.0 as usize >= input.owners.len()
                {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel definition {definition} statement {} references missing child owner {}",
                        statement.id.0, owner.0
                    )));
                }
            }
        }
        for binding in &facts.lexical_bindings {
            if let KernelLexicalBindingTargetInput::Declaration(
                KernelDeclarationReference::OwnerPublic(owner),
            ) = binding.target
                && owner.0 as usize >= input.owners.len()
            {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel definition {definition} lexical binding {} references missing owner {}",
                    binding.expression.0, owner.0
                )));
            }
        }
        let validate_resource_owner = |reference: KernelDeclarationReference, context: &str| {
            if let KernelDeclarationReference::OwnerPublic(owner) = reference
                && owner.0 as usize >= input.owners.len()
            {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel definition {definition} {context} references missing owner {}",
                    owner.0
                )));
            }
            Ok(())
        };
        let validate_resource_statement_owner =
            |reference: KernelStatementReference, context: &str| {
                if let KernelStatementReference::OwnerPublic(owner) = reference
                    && owner.0 as usize >= input.owners.len()
                {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel definition {definition} {context} references missing owner {}",
                        owner.0
                    )));
                }
                Ok(())
            };
        for source in &facts.sources {
            validate_resource_owner(source.declaration, "SOURCE row")?;
            validate_resource_statement_owner(source.statement, "SOURCE statement")?;
        }
        for state in &facts.states {
            validate_resource_owner(state.declaration, "state row")?;
            validate_resource_owner(state.binding_declaration, "state binding")?;
            validate_resource_statement_owner(state.statement, "state statement")?;
        }
        for list in &facts.lists {
            validate_resource_owner(list.declaration, "LIST row")?;
            validate_resource_statement_owner(list.statement, "LIST statement")?;
        }
    }
    let mut builder = ComponentProgramBuilder::new();
    let mut mode_builder = ModeProgramBuilder::default();
    let mut invocations = HashMap::new();
    let mut specializations = HashMap::new();
    let mut residual_modules = HashMap::new();
    let mut compile_work = KernelCompileWork {
        definition_modules: input.owners.len() as u64,
        principal_expressions: input
            .owners
            .iter()
            .map(|owner| owner.nodes.len() as u64)
            .sum(),
        ..KernelCompileWork::default()
    };
    let formal_dependent_expressions = input
        .owners
        .iter()
        .map(owner_expressions_depend_on_formals)
        .collect::<Vec<_>>();
    let formal_dependent_results = input
        .owners
        .iter()
        .zip(&formal_dependent_expressions)
        .map(|(owner, dependencies)| {
            let result = owner.result.0 as usize;
            dependencies.get(result).copied().unwrap_or(true)
        })
        .collect::<Vec<_>>();
    let principals = input
        .owners
        .iter()
        .map(|owner| {
            allocate_owner_instance(
                &mut builder,
                &mut mode_builder,
                owner,
                &vec![None; owner.formal_count as usize],
            )
        })
        .collect::<Vec<_>>();
    for (owner_index, owner) in input.owners.iter().enumerate() {
        let owner_id = KernelOwnerId(
            u32::try_from(owner_index).expect("kernel project owner count exceeds u32"),
        );
        validate_owner_input(owner_id, owner, &principals)?;
    }
    let syntax_discriminated_formals = project_syntax_discriminated_formals(input);
    let direct_summaries = compile_direct_result_summaries(&mut builder, input);
    for summary in direct_summaries.iter().flatten() {
        compile_work.summary_definition_nodes = compile_work
            .summary_definition_nodes
            .saturating_add(summary.program.nodes.len() as u64);
        compile_work.summary_constant_folded_nodes = compile_work
            .summary_constant_folded_nodes
            .saturating_add(summary.constant_folded_nodes);
        compile_work.summary_selector_fused_records = compile_work
            .summary_selector_fused_records
            .saturating_add(summary.selector_fused_records);
        compile_work.summary_deduplicated_nodes = compile_work
            .summary_deduplicated_nodes
            .saturating_add(summary.deduplicated_nodes);
        compile_work.summary_pruned_nodes = compile_work
            .summary_pruned_nodes
            .saturating_add(summary.pruned_nodes);
        compile_work.summary_pruned_inputs = compile_work
            .summary_pruned_inputs
            .saturating_add(summary.pruned_inputs);
        compile_work.summary_invoke_nodes = compile_work.summary_invoke_nodes.saturating_add(
            summary
                .program
                .nodes
                .iter()
                .filter(|node| matches!(node, KernelSummaryNode::Invoke { .. }))
                .count() as u64,
        );
    }
    for (owner_index, owner) in input.owners.iter().enumerate() {
        let owner_id = KernelOwnerId(
            u32::try_from(owner_index).expect("kernel project owner count exceeds u32"),
        );
        let instance = &principals[owner_index];
        let context = OwnerCompileContext {
            initial_state_surface: false,
            owner: owner_id,
            input: owner,
            expressions: &instance.expressions,
            formals: &instance.formals,
            formal_requirements: &instance.formal_requirements,
            expression_modes: &instance.expression_modes,
            formal_modes: &instance.formal_modes,
            formal_mode_sources: &instance.formal_mode_sources,
            static_variants: &instance.static_variants,
            formal_static_variants: &instance.formal_static_variants,
            project: Some(input),
            principals: &principals,
            formal_dependent_results: &formal_dependent_results,
            formal_dependent_expressions: &formal_dependent_expressions,
            external_variables: None,
            syntax_selected_calls: None,
            direct_summaries: &direct_summaries,
        };
        let mut stack = vec![owner_id];
        let specialization = OwnerSpecialization {
            static_variants: instance.static_variants.clone(),
            reachable: (0..owner.nodes.len())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            syntax_selected_calls: vec![false; owner.nodes.len()].into_boxed_slice(),
            invocation_dependencies: formal_dependent_expressions[owner_index].clone(),
            transparent_type_providers: vec![None; owner.nodes.len()].into_boxed_slice(),
        };
        let module = compile_residual_type_module(
            owner_id,
            owner,
            Some(input),
            &principals,
            &formal_dependent_results,
            &formal_dependent_expressions,
            &instance.formal_static_variants,
            &specialization,
            None,
            false,
        )?;
        compile_work.residual_type_modules = compile_work.residual_type_modules.saturating_add(1);
        compile_work.residual_module_operations = compile_work
            .residual_module_operations
            .saturating_add(module.component.operation_count() as u64);
        compile_work.residual_module_terms = compile_work
            .residual_module_terms
            .saturating_add(module.component.terms().len() as u64);
        let external_variables = principal_external_variables(owner, input, &principals)?;
        append_residual_type_frame(&mut builder, &module, instance, &external_variables)?;
        compile_work.residual_frames = compile_work.residual_frames.saturating_add(1);
        for (index, node) in owner.nodes.iter().enumerate() {
            if matches!(node.kind, KernelOwnerNodeKind::UserCall { .. }) {
                compile_node(
                    &mut builder,
                    &mut mode_builder,
                    &mut invocations,
                    &mut specializations,
                    &mut residual_modules,
                    &mut compile_work,
                    &context,
                    &mut stack,
                    index,
                    node,
                    instance.expressions[index],
                    instance.expression_modes[index],
                    true,
                )?;
            } else {
                let equation = node_mode_equation(&mut mode_builder, &context, index, node)?;
                mode_builder.set(instance.expression_modes[index], equation);
            }
        }
    }
    let modes = mode_builder.solve();
    let mut basis_fingerprint_scratch = Vec::new();
    let owners = input
        .owners
        .iter()
        .enumerate()
        .map(|(owner_index, owner)| {
            let owner_id = KernelOwnerId(
                u32::try_from(owner_index)
                    .expect("kernel project owner count exceeds the dense u32 namespace"),
            );
            let result = checked_expression_index(
                owner.result,
                owner.nodes.len(),
                &format!("project owner {owner_index} result"),
            )?;
            let expressions = principals[owner_index]
                .expressions
                .iter()
                .zip(owner.nodes.iter())
                .map(|(variable, node)| builder.add_output(*variable, node.mode))
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let statements = collect_statement_artifacts(owner, &facts[owner_index])?;
            let resources = collect_resource_artifacts(owner, &facts[owner_index])?;
            Ok(KernelProjectOwnerOutputs {
                result: expressions[result],
                formals: principals[owner_index]
                    .formal_requirements
                    .iter()
                    .map(|variable| builder.add_output(*variable, FlowMode::Continuous))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
                formal_modes: principals[owner_index]
                    .formal_modes
                    .iter()
                    .map(|mode| modes[mode.0 as usize])
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
                expressions,
                expression_modes: principals[owner_index]
                    .expression_modes
                    .iter()
                    .map(|mode| modes[mode.0 as usize])
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
                expression_artifacts: collect_expression_artifacts(owner)?,
                relocations: facts[owner_index].relocations.clone(),
                statements,
                declarations: collect_declaration_artifacts(owner, &facts[owner_index])?,
                lexical_bindings: collect_lexical_binding_artifacts(owner, &facts[owner_index])?,
                resources,
                calls: collect_call_artifacts(owner)?,
                effects: collect_host_effect_artifacts(owner)?,
                diagnostics: collect_definition_diagnostic_artifacts(
                    owner_id,
                    owner,
                    &facts[owner_index],
                )?,
                diagnostic_values: facts[owner_index]
                    .diagnostic_values
                    .iter()
                    .copied()
                    .map(|expression| {
                        kernel_value_reference(owner, expression, expression.0 as usize)
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice(),
                syntax_discriminated_formals: syntax_discriminated_formals[owner_index].clone(),
                basis_fingerprint_v4: definition_basis_fingerprint_with_buffer(
                    owner,
                    &facts[owner_index],
                    &mut basis_fingerprint_scratch,
                )?,
            })
        })
        .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?;
    let component = builder.finish();
    let mut frame_counts = HashMap::<*const ComponentProgram, u64>::new();
    for frame in component.residual_frames.iter() {
        *frame_counts.entry(Arc::as_ptr(&frame.module)).or_default() += 1;
        if frame.module.acyclic_initial_operation_count() == frame.module.operation_count() as u64 {
            compile_work.acyclic_residual_frames =
                compile_work.acyclic_residual_frames.saturating_add(1);
        }
    }
    for (key, module) in &residual_modules {
        let operations = module.component.operation_count() as u64;
        let frames = frame_counts
            .get(&Arc::as_ptr(&module.component))
            .copied()
            .unwrap_or_default();
        let linked = operations.saturating_mul(frames);
        if linked > compile_work.dominant_module_linked_operations {
            compile_work.dominant_module_owner = key.target.0 as u64;
            compile_work.dominant_module_operations = operations;
            compile_work.dominant_module_frames = frames;
            compile_work.dominant_module_linked_operations = linked;
        }
        let candidate = KernelResidualModuleWork {
            owner: key.target.0,
            operations: u32::try_from(operations)
                .expect("kernel residual module operation count exceeds u32"),
            frames: u32::try_from(frames).expect("kernel residual module frame count exceeds u32"),
            linked_operations: linked,
        };
        if let Some(position) = compile_work
            .residual_module_ranking
            .iter()
            .position(|current| candidate.linked_operations > current.linked_operations)
        {
            for index in (position + 1..KERNEL_RESIDUAL_MODULE_RANKING_LEN).rev() {
                compile_work.residual_module_ranking[index] =
                    compile_work.residual_module_ranking[index - 1];
            }
            compile_work.residual_module_ranking[position] = candidate;
        }
    }
    compile_work.linked_operations = component.operation_count() as u64;
    compile_work.scheduled_work_items = component.scheduled_work_item_count() as u64;
    compile_work.linked_terms = component.terms().len() as u64;
    compile_work.acyclic_initial_operations = component.acyclic_initial_operation_count();
    Ok(KernelProjectProgram {
        component,
        owners: owners.into_boxed_slice(),
        compile_work,
    })
}

#[derive(Clone, Debug)]
struct OwnerInstance {
    expressions: Vec<TypeVariableId>,
    formals: Vec<TypeVariableId>,
    formal_requirements: Vec<TypeVariableId>,
    expression_modes: Arc<[ModeVariableId]>,
    formal_modes: Vec<ModeVariableId>,
    formal_mode_sources: Arc<[ModeSource]>,
    static_variants: Vec<Option<StaticVariantSet>>,
    formal_static_variants: Vec<Option<StaticVariantSet>>,
}

/// One structural flow-mode authority.
///
/// Types already retain their record/list shape while crossing owner calls.
/// Flow modes need the same provenance: a continuous record may contain a
/// `PresentOrAbsent` SOURCE field, and projecting that field must not collapse
/// to the record's root mode. Expression sources retain the compact residual
/// frame needed to follow record fields, collection items, and call formals;
/// opaque roots are used only where no structural syntax exists.
#[derive(Clone, Debug)]
enum ModeSource {
    Root(ModeVariableId),
    Expression {
        owner: KernelOwnerId,
        expression: usize,
        expression_modes: Arc<[ModeVariableId]>,
        formal_sources: Arc<[ModeSource]>,
    },
}

impl ModeSource {
    fn root_mode(&self) -> ModeVariableId {
        match self {
            Self::Root(mode) => *mode,
            Self::Expression {
                expression,
                expression_modes,
                ..
            } => expression_modes[*expression],
        }
    }
}

type StaticVariantSet = BTreeSet<Box<str>>;

#[derive(Clone, Debug)]
struct CallActual {
    /// Provider surface used to evaluate the callee occurrence.
    variable: TypeVariableId,
    /// Caller-owned surface that accepts definition-local requirements.
    ///
    /// Explicit arguments use the expression occurrence for both roles. An
    /// inherited formal deliberately separates them: its provider is the
    /// caller formal, while requirements must land on the caller's private
    /// formal-requirement surface. Keeping these channels distinct prevents
    /// inherited PASSED calls from silently detaching transitive constraints.
    requirement: TypeVariableId,
    /// Whether this occurrence reads a private output capability and therefore
    /// has a separate caller-owned requirement channel. The provider remains
    /// directional and unchanged; only `requirement` accepts constraints from
    /// the callee definition.
    requirement_backflow: bool,
    mode: ModeVariableId,
    mode_source: ModeSource,
    static_variants: Option<StaticVariantSet>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct InvocationKey {
    target: KernelOwnerId,
    actuals: Box<[(TypeVariableId, ModeVariableId)]>,
    static_variants: Box<[Option<StaticVariantSet>]>,
    initial_state_surface: bool,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct SpecializationKey {
    target: KernelOwnerId,
    static_variants: Box<[Option<StaticVariantSet>]>,
    initial_state_surface: bool,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct OwnerSpecialization {
    static_variants: Vec<Option<StaticVariantSet>>,
    reachable: Box<[usize]>,
    syntax_selected_calls: Box<[bool]>,
    invocation_dependencies: Box<[bool]>,
    transparent_type_providers: Box<[Option<usize>]>,
}

#[derive(Debug)]
struct ResidualTypeModule {
    component: Arc<ComponentProgram>,
    local: OwnerInstance,
    external_variables: Box<[TypeVariableId]>,
    calls: Box<[usize]>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct ModeVariableId(u32);

#[derive(Clone, Debug)]
enum ModeEquation {
    Fixed(FlowMode),
    Copy(ModeVariableId),
    Eventful(ModeVariableId),
    Latest(Box<[ModeVariableId]>),
    Call {
        result: FlowMode,
        inputs: Box<[ModeVariableId]>,
    },
}

#[derive(Clone, Debug)]
struct ModeVariable {
    fallback: FlowMode,
    equation: Option<ModeEquation>,
}

#[derive(Default)]
struct ModeProgramBuilder {
    variables: Vec<ModeVariable>,
}

impl ModeProgramBuilder {
    fn new_variable(&mut self, fallback: FlowMode) -> ModeVariableId {
        let id = ModeVariableId(
            u32::try_from(self.variables.len()).expect("kernel mode variable count exceeds u32"),
        );
        self.variables.push(ModeVariable {
            fallback,
            equation: None,
        });
        id
    }

    fn set(&mut self, output: ModeVariableId, equation: ModeEquation) {
        let slot = &mut self.variables[output.0 as usize].equation;
        assert!(slot.is_none(), "kernel mode variable has multiple writers");
        *slot = Some(equation);
    }

    fn solve(self) -> Box<[FlowMode]> {
        let mut reverse = vec![Vec::<usize>::new(); self.variables.len()];
        for (output, variable) in self.variables.iter().enumerate() {
            let inputs: &[ModeVariableId] = match variable.equation.as_ref() {
                Some(ModeEquation::Copy(input)) => std::slice::from_ref(input),
                Some(ModeEquation::Eventful(input)) => std::slice::from_ref(input),
                Some(ModeEquation::Latest(inputs)) => inputs,
                Some(ModeEquation::Call { inputs, .. }) => inputs,
                Some(ModeEquation::Fixed(_)) | None => &[],
            };
            for input in inputs {
                reverse[input.0 as usize].push(output);
            }
        }
        let mut modes = vec![None; self.variables.len()];
        let mut pending = (0..self.variables.len()).collect::<std::collections::VecDeque<_>>();
        let mut queued = vec![true; self.variables.len()];
        while let Some(output) = pending.pop_front() {
            queued[output] = false;
            let next = match self.variables[output].equation.as_ref() {
                Some(ModeEquation::Fixed(mode)) => Some(*mode),
                Some(ModeEquation::Copy(input)) => modes[input.0 as usize],
                Some(ModeEquation::Eventful(input)) => modes[input.0 as usize].filter(|mode| {
                    matches!(mode, FlowMode::TickPresent | FlowMode::PresentOrAbsent)
                }),
                Some(ModeEquation::Latest(inputs)) => {
                    latest_mode(inputs.iter().filter_map(|input| modes[input.0 as usize]))
                }
                Some(ModeEquation::Call { result, inputs }) => inputs
                    .iter()
                    .filter_map(|input| modes[input.0 as usize])
                    .fold(Some(*result), merge_call_mode),
                None => None,
            };
            if modes[output] == next {
                continue;
            }
            modes[output] = next;
            for consumer in &reverse[output] {
                if !queued[*consumer] {
                    queued[*consumer] = true;
                    pending.push_back(*consumer);
                }
            }
        }
        self.variables
            .into_iter()
            .enumerate()
            .map(|(index, variable)| modes[index].unwrap_or(variable.fallback))
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }
}

fn latest_mode(modes: impl IntoIterator<Item = FlowMode>) -> Option<FlowMode> {
    let mut saw_mode = false;
    let mut saw_continuous = false;
    let mut saw_present = false;
    for mode in modes {
        saw_mode = true;
        match mode {
            FlowMode::Continuous => saw_continuous = true,
            FlowMode::TickPresent | FlowMode::PresentOrAbsent => saw_present = true,
            FlowMode::Absent => {}
        }
    }
    saw_mode.then_some(if saw_continuous {
        FlowMode::Continuous
    } else if saw_present {
        FlowMode::PresentOrAbsent
    } else {
        FlowMode::Absent
    })
}

fn merge_call_mode(left: Option<FlowMode>, right: FlowMode) -> Option<FlowMode> {
    match (left, right) {
        (None, mode) => Some(mode),
        (Some(FlowMode::Absent), _) | (_, FlowMode::Absent) => Some(FlowMode::Absent),
        (Some(FlowMode::PresentOrAbsent), _) | (_, FlowMode::PresentOrAbsent) => {
            Some(FlowMode::PresentOrAbsent)
        }
        (Some(FlowMode::TickPresent), _) | (_, FlowMode::TickPresent) => {
            Some(FlowMode::TickPresent)
        }
        (Some(FlowMode::Continuous), FlowMode::Continuous) => Some(FlowMode::Continuous),
    }
}

fn allocate_owner_instance(
    builder: &mut ComponentProgramBuilder,
    mode_builder: &mut ModeProgramBuilder,
    owner: &KernelOwnerProgramInput,
    formal_static_variants: &[Option<StaticVariantSet>],
) -> OwnerInstance {
    let static_variants = infer_static_variants(owner, formal_static_variants);
    allocate_owner_instance_with_static_variants(
        builder,
        mode_builder,
        owner,
        formal_static_variants,
        static_variants,
        None,
    )
}

fn allocate_owner_instance_with_static_variants(
    builder: &mut ComponentProgramBuilder,
    mode_builder: &mut ModeProgramBuilder,
    owner: &KernelOwnerProgramInput,
    formal_static_variants: &[Option<StaticVariantSet>],
    static_variants: Vec<Option<StaticVariantSet>>,
    transparent_type_providers: Option<&[Option<usize>]>,
) -> OwnerInstance {
    let mut expressions = Vec::with_capacity(owner.nodes.len());
    for (index, node) in owner.nodes.iter().enumerate() {
        if let Some(provider) = transparent_type_providers.and_then(|providers| providers[index]) {
            expressions.push(
                *expressions
                    .get(provider)
                    .expect("transparent type provider precedes its match arm"),
            );
        } else if matches!(
            node.kind,
            KernelOwnerNodeKind::FormalRead { .. }
                | KernelOwnerNodeKind::ContextRead { .. }
                | KernelOwnerNodeKind::LexicalRead { .. }
                | KernelOwnerNodeKind::PatternRead { .. }
        ) {
            expressions.push(builder.new_variable());
        } else {
            expressions.push(builder.new_authoritative_provider());
        }
    }
    assert_eq!(formal_static_variants.len(), owner.formal_count as usize);
    let expression_modes = owner
        .nodes
        .iter()
        .map(|node| mode_builder.new_variable(node.mode))
        .collect::<Vec<_>>()
        .into();
    let formal_modes = (0..owner.formal_count)
        .map(|_| mode_builder.new_variable(FlowMode::Continuous))
        .collect::<Vec<_>>();
    let formal_mode_sources = formal_modes
        .iter()
        .copied()
        .map(ModeSource::Root)
        .collect::<Vec<_>>()
        .into();
    OwnerInstance {
        expressions,
        formals: (0..owner.formal_count)
            .map(|_| builder.new_contextual_hole())
            .collect(),
        formal_requirements: (0..owner.formal_count)
            .map(|_| builder.new_contextual_hole())
            .collect(),
        expression_modes,
        formal_modes,
        formal_mode_sources,
        static_variants,
        formal_static_variants: formal_static_variants.to_vec(),
    }
}

fn allocate_invocation_owner_instance(
    builder: &mut ComponentProgramBuilder,
    mode_builder: &mut ModeProgramBuilder,
    owner: &KernelOwnerProgramInput,
    principal: &OwnerInstance,
    actuals: &[CallActual],
    formal_static_variants: &[Option<StaticVariantSet>],
    static_variants: Vec<Option<StaticVariantSet>>,
    expression_dependencies: &[bool],
    reachable: &[usize],
    transparent_type_providers: &[Option<usize>],
) -> (OwnerInstance, u64) {
    assert_eq!(actuals.len(), owner.formal_count as usize);
    assert_eq!(expression_dependencies.len(), owner.nodes.len());
    let mut reachable_expressions = vec![false; owner.nodes.len()];
    for index in reachable {
        reachable_expressions[*index] = true;
    }
    let pruned_expressions = reachable_expressions
        .iter()
        .zip(expression_dependencies)
        .filter(|(reachable, dependent)| !**reachable && **dependent)
        .count() as u64;
    let mut expressions = Vec::with_capacity(owner.nodes.len());
    for (index, node) in owner.nodes.iter().enumerate() {
        if !expression_dependencies[index] || !reachable_expressions[index] {
            expressions.push(principal.expressions[index]);
        } else if let Some(provider) = transparent_type_providers[index] {
            expressions.push(
                *expressions
                    .get(provider)
                    .expect("transparent type provider precedes its match arm"),
            );
        } else if matches!(
            node.kind,
            KernelOwnerNodeKind::FormalRead { .. }
                | KernelOwnerNodeKind::ContextRead { .. }
                | KernelOwnerNodeKind::LexicalRead { .. }
                | KernelOwnerNodeKind::PatternRead { .. }
        ) {
            expressions.push(builder.new_variable());
        } else {
            expressions.push(builder.new_authoritative_provider());
        }
    }
    let expression_modes = owner
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            if expression_dependencies[index] && reachable_expressions[index] {
                mode_builder.new_variable(node.mode)
            } else {
                principal.expression_modes[index]
            }
        })
        .collect::<Vec<_>>()
        .into();
    (
        OwnerInstance {
            expressions,
            formals: actuals.iter().map(|actual| actual.variable).collect(),
            formal_requirements: (0..owner.formal_count)
                .map(|_| builder.new_contextual_hole())
                .collect(),
            expression_modes,
            formal_modes: actuals.iter().map(|actual| actual.mode).collect(),
            formal_mode_sources: actuals
                .iter()
                .map(|actual| actual.mode_source.clone())
                .collect::<Vec<_>>()
                .into(),
            static_variants,
            formal_static_variants: formal_static_variants.to_vec(),
        },
        pruned_expressions,
    )
}

struct OwnerCompileContext<'a> {
    initial_state_surface: bool,
    owner: KernelOwnerId,
    input: &'a KernelOwnerProgramInput,
    expressions: &'a [TypeVariableId],
    formals: &'a [TypeVariableId],
    formal_requirements: &'a [TypeVariableId],
    expression_modes: &'a Arc<[ModeVariableId]>,
    formal_modes: &'a [ModeVariableId],
    formal_mode_sources: &'a Arc<[ModeSource]>,
    static_variants: &'a [Option<StaticVariantSet>],
    formal_static_variants: &'a [Option<StaticVariantSet>],
    project: Option<&'a KernelProjectProgramInput>,
    principals: &'a [OwnerInstance],
    formal_dependent_results: &'a [bool],
    formal_dependent_expressions: &'a [Box<[bool]>],
    external_variables: Option<&'a [TypeVariableId]>,
    syntax_selected_calls: Option<&'a [bool]>,
    direct_summaries: &'a [Option<Arc<CompiledDirectSummary>>],
}

fn validate_owner_input(
    owner_id: KernelOwnerId,
    owner: &KernelOwnerProgramInput,
    principals: &[OwnerInstance],
) -> Result<(), KernelOwnerBuildError> {
    checked_expression_index(owner.result, owner.nodes.len(), "project owner result")?;
    for (index, external) in owner.external_expressions.iter().enumerate() {
        let Some(target) = principals.get(external.owner.0 as usize) else {
            return Err(KernelOwnerBuildError::new(format!(
                "project owner {} external expression {index} targets missing owner {}",
                owner_id.0, external.owner.0
            )));
        };
        if let KernelExternalTarget::Expression(expression) = external.target {
            checked_expression_index(
                expression,
                target.expressions.len(),
                &format!("project owner {} external expression {index}", owner_id.0),
            )?;
        }
    }
    Ok(())
}

fn edge_static_variants(
    context: &OwnerCompileContext<'_>,
    edge: &KernelOwnerInputEdge,
) -> Option<StaticVariantSet> {
    let expression = edge.expression.0 as usize;
    if expression < context.input.nodes.len() {
        return context.static_variants[expression].clone();
    }
    let external = context
        .input
        .external_expressions
        .get(expression.checked_sub(context.input.nodes.len())?)?;
    let owner = context.principals.get(external.owner.0 as usize)?;
    let expression = match external.target {
        KernelExternalTarget::Expression(expression) => expression.0 as usize,
        KernelExternalTarget::Result => {
            context
                .project?
                .owners
                .get(external.owner.0 as usize)?
                .result
                .0 as usize
        }
    };
    owner.static_variants.get(expression)?.clone()
}

fn infer_static_variants(
    owner: &KernelOwnerProgramInput,
    formal_static_variants: &[Option<StaticVariantSet>],
) -> Vec<Option<StaticVariantSet>> {
    let mut variants = vec![None; owner.nodes.len()];
    for (index, node) in owner.nodes.iter().enumerate() {
        variants[index] = match &node.kind {
            KernelOwnerNodeKind::Known(Type::VariantSet(values))
            | KernelOwnerNodeKind::Source(Type::VariantSet(values)) => values
                .iter()
                .map(|variant| match variant {
                    Variant::Tag(tag) | Variant::Tagged { tag, .. } => {
                        Some(tag.clone().into_boxed_str())
                    }
                })
                .collect::<Option<StaticVariantSet>>(),
            KernelOwnerNodeKind::Tag(tag) => Some(BTreeSet::from([tag.clone()])),
            KernelOwnerNodeKind::Record { tag: Some(tag) } => Some(BTreeSet::from([tag.clone()])),
            KernelOwnerNodeKind::FormalRead { formal, fields }
            | KernelOwnerNodeKind::ContextRead { formal, fields }
                if fields.is_empty() =>
            {
                formal_static_variants
                    .get(*formal as usize)
                    .cloned()
                    .flatten()
            }
            KernelOwnerNodeKind::Infix { operation } if infix_returns_bool(operation) => {
                Some(BTreeSet::from(["False".into(), "True".into()]))
            }
            KernelOwnerNodeKind::PureBuiltin {
                kind:
                    KernelPureBuiltinKind::TextPredicate
                    | KernelPureBuiltinKind::ListPredicate
                    | KernelPureBuiltinKind::Boolean,
            } => Some(BTreeSet::from(["False".into(), "True".into()])),
            _ => None,
        };
    }
    for _ in 0..=owner.nodes.len() {
        let mut changed = false;
        for (index, node) in owner.nodes.iter().enumerate() {
            let next = match &node.kind {
                KernelOwnerNodeKind::Block => {
                    merge_static_edge_variants(owner, node, &variants, |role| {
                        matches!(role, KernelOwnerEdgeRole::BlockResult)
                    })
                }
                KernelOwnerNodeKind::LexicalRead { fields }
                | KernelOwnerNodeKind::ValueRead { fields, .. }
                | KernelOwnerNodeKind::DerivedRead { fields }
                    if fields.is_empty() =>
                {
                    merge_static_edge_variants(owner, node, &variants, |role| {
                        matches!(role, KernelOwnerEdgeRole::ReadProvider)
                    })
                }
                KernelOwnerNodeKind::Latest => {
                    merge_static_edge_variants(owner, node, &variants, |role| {
                        matches!(role, KernelOwnerEdgeRole::LatestBranch)
                    })
                }
                KernelOwnerNodeKind::When => {
                    let arms = possible_when_arm_expressions(owner, index, &variants);
                    merge_static_expression_variants(&arms, &variants)
                }
                KernelOwnerNodeKind::Then => {
                    let has_output = node
                        .inputs
                        .iter()
                        .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::ThenOutput));
                    merge_static_edge_variants(owner, node, &variants, |role| {
                        matches!(role, KernelOwnerEdgeRole::ThenOutput)
                            || (!has_output && matches!(role, KernelOwnerEdgeRole::ThenInput))
                    })
                }
                KernelOwnerNodeKind::Draining => {
                    merge_static_edge_variants(owner, node, &variants, |role| {
                        matches!(role, KernelOwnerEdgeRole::DrainingInput)
                    })
                }
                KernelOwnerNodeKind::MatchArm { .. } => {
                    merge_static_edge_variants(owner, node, &variants, |role| {
                        matches!(role, KernelOwnerEdgeRole::MatchOutput)
                    })
                }
                KernelOwnerNodeKind::Arrow => {
                    merge_static_edge_variants(owner, node, &variants, |role| {
                        matches!(role, KernelOwnerEdgeRole::ArrowOutput)
                    })
                }
                _ => variants[index].clone(),
            };
            if variants[index] != next {
                variants[index] = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    variants
}

fn merge_static_edge_variants(
    owner: &KernelOwnerProgramInput,
    node: &KernelOwnerNode,
    variants: &[Option<StaticVariantSet>],
    selected: impl Fn(&KernelOwnerEdgeRole) -> bool,
) -> Option<StaticVariantSet> {
    let expressions = node
        .inputs
        .iter()
        .filter(|edge| selected(&edge.role))
        .map(|edge| edge.expression.0 as usize)
        .collect::<Vec<_>>();
    if expressions
        .iter()
        .any(|expression| *expression >= owner.nodes.len())
    {
        return None;
    }
    merge_static_expression_variants(&expressions, variants)
}

fn merge_static_expression_variants(
    expressions: &[usize],
    variants: &[Option<StaticVariantSet>],
) -> Option<StaticVariantSet> {
    if expressions.is_empty() {
        return None;
    }
    let mut merged = BTreeSet::new();
    for expression in expressions {
        merged.extend(variants.get(*expression)?.as_ref()?.iter().cloned());
    }
    Some(merged)
}

fn possible_when_arm_expressions(
    owner: &KernelOwnerProgramInput,
    when: usize,
    variants: &[Option<StaticVariantSet>],
) -> Vec<usize> {
    let node = &owner.nodes[when];
    let arms = node
        .inputs
        .iter()
        .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenArm))
        .map(|edge| edge.expression.0 as usize)
        .filter(|arm| *arm < owner.nodes.len())
        .collect::<Vec<_>>();
    let selector = node
        .inputs
        .iter()
        .find(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenInput))
        .map(|edge| edge.expression.0 as usize)
        .filter(|selector| *selector < owner.nodes.len())
        .and_then(|selector| variants.get(selector))
        .and_then(Option::as_ref);
    let Some(selector) = selector else {
        return arms;
    };
    let mut selected = BTreeSet::new();
    for tag in selector {
        if let Some(arm) = arms.iter().copied().find(|arm| {
            matches!(
                &owner.nodes[*arm].kind,
                KernelOwnerNodeKind::MatchArm { pattern }
                    if static_pattern_accepts_tag(pattern, tag)
            )
        }) {
            selected.insert(arm);
        }
    }
    selected.into_iter().collect()
}

fn static_pattern_accepts_tag(pattern: &KernelPattern, tag: &str) -> bool {
    match pattern {
        KernelPattern::Wildcard | KernelPattern::Binding { .. } => true,
        KernelPattern::Tag { name, .. } => name.as_ref() == tag,
        KernelPattern::Number
        | KernelPattern::Text
        | KernelPattern::Bits { .. }
        | KernelPattern::Invalid => false,
    }
}

fn reachable_owner_nodes(
    owner: &KernelOwnerProgramInput,
    result: usize,
    variants: &[Option<StaticVariantSet>],
) -> BTreeSet<usize> {
    let mut reachable = BTreeSet::new();
    let mut pending = vec![result];
    while let Some(index) = pending.pop() {
        if !reachable.insert(index) {
            continue;
        }
        let node = &owner.nodes[index];
        if matches!(node.kind, KernelOwnerNodeKind::When) {
            pending.extend(
                node.inputs
                    .iter()
                    .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenInput))
                    .map(|edge| edge.expression.0 as usize)
                    .filter(|input| *input < owner.nodes.len()),
            );
            pending.extend(possible_when_arm_expressions(owner, index, variants));
        } else {
            pending.extend(
                node.inputs
                    .iter()
                    .map(|edge| edge.expression.0 as usize)
                    .filter(|input| *input < owner.nodes.len()),
            );
        }
    }
    reachable
}

/// Mark calls whose result is constructed under a formal-derived `WHEN` arm.
///
/// This is occurrence provenance, not ordinary reachability. A definition
/// retains every state update in its public result. An invocation evaluates a
/// branch as a construction site, so stateful callees below that constructor
/// expose their initializer surface; the separately owned state artifact keeps
/// the complete update domain. This does not require a parallel static-type
/// evaluator merely to rediscover a tag nested inside another call's record.
fn syntax_selected_call_nodes(
    owner: &KernelOwnerProgramInput,
    variants: &[Option<StaticVariantSet>],
    formal_dependencies: &[bool],
) -> Box<[bool]> {
    let mut selected_calls = vec![false; owner.nodes.len()];
    for (when, node) in owner.nodes.iter().enumerate() {
        if !matches!(node.kind, KernelOwnerNodeKind::When) {
            continue;
        }
        let Some(selector) = node
            .inputs
            .iter()
            .find(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenInput))
            .map(|edge| edge.expression.0 as usize)
            .filter(|selector| *selector < owner.nodes.len())
        else {
            continue;
        };
        if !formal_dependencies.get(selector).copied().unwrap_or(false) {
            continue;
        }
        let arms = possible_when_arm_expressions(owner, when, variants);
        for arm in arms {
            for expression in reachable_owner_nodes(owner, arm, variants) {
                if matches!(
                    owner.nodes[expression].kind,
                    KernelOwnerNodeKind::UserCall { .. }
                ) {
                    selected_calls[expression] = true;
                }
            }
        }
    }
    selected_calls.into_boxed_slice()
}

fn invocation_expression_dependencies(
    owner: &KernelOwnerProgramInput,
    formal_dependencies: &[bool],
    syntax_selected_calls: &[bool],
) -> Box<[bool]> {
    let mut dependent = formal_dependencies
        .iter()
        .zip(syntax_selected_calls)
        .map(|(formal, selected)| *formal || *selected)
        .collect::<Vec<_>>();
    loop {
        let mut changed = false;
        for (index, node) in owner.nodes.iter().enumerate() {
            if dependent[index] {
                continue;
            }
            if node.inputs.iter().any(|edge| {
                dependent
                    .get(edge.expression.0 as usize)
                    .copied()
                    .unwrap_or(false)
            }) {
                dependent[index] = true;
                changed = true;
            }
        }
        if !changed {
            return dependent.into_boxed_slice();
        }
    }
}

/// Type-only aliases that need no invocation-local publication.
///
/// Principal frames still materialize every authored expression for checked
/// artifacts. Specialized invocation frames may point a transparent match arm
/// directly at its sole output, because the enclosing SELECT owns detachment
/// and publication. Field-backed delimiter arms remain real record producers.
fn transparent_type_providers(owner: &KernelOwnerProgramInput) -> Box<[Option<usize>]> {
    let mut has_use = vec![false; owner.nodes.len()];
    let mut select_only = vec![true; owner.nodes.len()];
    for node in &owner.nodes {
        for edge in &node.inputs {
            let expression = edge.expression.0 as usize;
            if expression >= owner.nodes.len() {
                continue;
            }
            has_use[expression] = true;
            select_only[expression] &= matches!(edge.role, KernelOwnerEdgeRole::WhenArm);
        }
    }
    owner
        .nodes
        .iter()
        .enumerate()
        .map(|(expression, node)| {
            let KernelOwnerNodeKind::MatchArm { .. } = node.kind else {
                return None;
            };
            let mut outputs = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::MatchOutput));
            let output = outputs.next()?;
            let provider = output.expression.0 as usize;
            (outputs.next().is_none()
                && node.inputs.len() == 1
                && provider < expression
                && has_use[expression]
                && select_only[expression]
                && owner.result.0 as usize != expression)
                .then_some(provider)
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

/// Return whether each expression requires an invocation-local cell.
///
/// This intentionally walks the unspecialized local graph rather than the
/// statically sliced occurrence graph. In addition to formal-dependent values,
/// every HOLD is occurrence-owned even when its initializer and updates are
/// syntactically formal-independent. A principal result may be reused only
/// when every invocation computes the same non-stateful value and imposes no
/// formal requirement. User-call inherited formals are implicit inputs, so
/// they are treated as dependencies even though they do not have an ordinary
/// edge.
fn owner_expressions_depend_on_formals(owner: &KernelOwnerProgramInput) -> Box<[bool]> {
    let mut dependent = owner
        .nodes
        .iter()
        .map(|node| {
            matches!(
                node.kind,
                KernelOwnerNodeKind::FormalRead { .. }
                    | KernelOwnerNodeKind::ContextRead { .. }
                    | KernelOwnerNodeKind::FreshOut
                    | KernelOwnerNodeKind::Hold
                    | KernelOwnerNodeKind::UserCall {
                        inherited_formal: Some(_),
                        ..
                    }
            )
        })
        .collect::<Vec<_>>();
    loop {
        let mut changed = false;
        for (index, node) in owner.nodes.iter().enumerate() {
            if dependent[index] {
                continue;
            }
            if node.inputs.iter().any(|edge| {
                dependent
                    .get(edge.expression.0 as usize)
                    .copied()
                    .unwrap_or(false)
            }) {
                dependent[index] = true;
                changed = true;
            }
        }
        if !changed {
            return dependent.into_boxed_slice();
        }
    }
}

fn compile_residual_type_module(
    owner_id: KernelOwnerId,
    owner: &KernelOwnerProgramInput,
    project: Option<&KernelProjectProgramInput>,
    principals: &[OwnerInstance],
    formal_dependent_results: &[bool],
    formal_dependent_expressions: &[Box<[bool]>],
    formal_static_variants: &[Option<StaticVariantSet>],
    specialization: &OwnerSpecialization,
    invocation_dependencies: Option<&[bool]>,
    initial_state_surface: bool,
) -> Result<Arc<ResidualTypeModule>, KernelOwnerBuildError> {
    let mut builder = ComponentProgramBuilder::new();
    let mut mode_builder = ModeProgramBuilder::default();
    let residual_transparent_type_providers = invocation_dependencies.map(|dependencies| {
        specialization
            .transparent_type_providers
            .iter()
            .copied()
            .enumerate()
            .map(|(expression, provider)| {
                (dependencies[expression]
                    && specialization.reachable.binary_search(&expression).is_ok())
                .then_some(provider)
                .flatten()
            })
            .collect::<Vec<_>>()
    });
    let local = allocate_owner_instance_with_static_variants(
        &mut builder,
        &mut mode_builder,
        owner,
        formal_static_variants,
        specialization.static_variants.clone(),
        residual_transparent_type_providers.as_deref(),
    );
    let external_variables = owner
        .external_expressions
        .iter()
        .map(|_| builder.new_contextual_hole())
        .collect::<Vec<_>>();
    let context = OwnerCompileContext {
        initial_state_surface,
        owner: owner_id,
        input: owner,
        expressions: &local.expressions,
        formals: &local.formals,
        formal_requirements: &local.formal_requirements,
        expression_modes: &local.expression_modes,
        formal_modes: &local.formal_modes,
        formal_mode_sources: &local.formal_mode_sources,
        static_variants: &local.static_variants,
        formal_static_variants: &local.formal_static_variants,
        project,
        principals,
        formal_dependent_results,
        formal_dependent_expressions,
        external_variables: Some(&external_variables),
        syntax_selected_calls: Some(&specialization.syntax_selected_calls),
        direct_summaries: &[],
    };
    let mut invocations = HashMap::new();
    let mut specializations = HashMap::new();
    let mut residual_modules = HashMap::new();
    let mut compile_work = KernelCompileWork::default();
    let mut stack = vec![owner_id];
    let mut calls = Vec::new();
    for index in specialization.reachable.iter().copied() {
        if invocation_dependencies.is_some_and(|dependencies| !dependencies[index]) {
            continue;
        }
        if residual_transparent_type_providers
            .as_ref()
            .is_some_and(|providers| providers[index].is_some())
        {
            continue;
        }
        let node = owner.nodes.get(index).ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "residual module owner {} references missing node {index}",
                owner_id.0
            ))
        })?;
        if matches!(node.kind, KernelOwnerNodeKind::UserCall { .. }) {
            calls.push(index);
            continue;
        }
        compile_node(
            &mut builder,
            &mut mode_builder,
            &mut invocations,
            &mut specializations,
            &mut residual_modules,
            &mut compile_work,
            &context,
            &mut stack,
            index,
            node,
            local.expressions[index],
            local.expression_modes[index],
            false,
        )?;
    }
    Ok(Arc::new(ResidualTypeModule {
        component: Arc::new(builder.finish()),
        local,
        external_variables: external_variables.into_boxed_slice(),
        calls: calls.into_boxed_slice(),
    }))
}

fn append_residual_type_frame(
    builder: &mut ComponentProgramBuilder,
    module: &ResidualTypeModule,
    instance: &OwnerInstance,
    external_variables: &[TypeVariableId],
) -> Result<u32, KernelOwnerBuildError> {
    if external_variables.len() != module.external_variables.len() {
        return Err(KernelOwnerBuildError::new(format!(
            "residual frame supplies {} external variables for {} module imports",
            external_variables.len(),
            module.external_variables.len()
        )));
    }
    let mut variables = vec![None; module.component.variable_count()];
    let mut map = |local: TypeVariableId,
                   global: TypeVariableId,
                   role: &str|
     -> Result<(), KernelOwnerBuildError> {
        let slot = variables.get_mut(local.0 as usize).ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "residual module {role} variable {} is outside its frame",
                local.0
            ))
        })?;
        if slot
            .replace(global)
            .is_some_and(|previous| previous != global)
        {
            return Err(KernelOwnerBuildError::new(format!(
                "residual module {role} variable {} has conflicting frame mappings",
                local.0
            )));
        }
        Ok(())
    };
    for (local, global) in module.local.expressions.iter().zip(&instance.expressions) {
        map(*local, *global, "expression")?;
    }
    for (local, global) in module.local.formals.iter().zip(&instance.formals) {
        map(*local, *global, "formal")?;
    }
    for (local, global) in module
        .local
        .formal_requirements
        .iter()
        .zip(&instance.formal_requirements)
    {
        map(*local, *global, "formal requirement")?;
    }
    for (local, global) in module.external_variables.iter().zip(external_variables) {
        map(*local, *global, "external")?;
    }
    for (index, spec) in module
        .component
        .variable_specs()
        .iter()
        .copied()
        .enumerate()
    {
        if variables[index].is_none() {
            variables[index] = Some(builder.new_variable_with(spec));
        }
    }
    let variables = variables
        .into_iter()
        .map(|variable| variable.expect("every residual frame variable is mapped"))
        .collect::<Vec<_>>();
    Ok(builder.add_residual_frame(Arc::clone(&module.component), variables))
}

fn principal_external_variables(
    owner: &KernelOwnerProgramInput,
    project: &KernelProjectProgramInput,
    principals: &[OwnerInstance],
) -> Result<Vec<TypeVariableId>, KernelOwnerBuildError> {
    owner
        .external_expressions
        .iter()
        .map(|external| {
            let target_instance = principals.get(external.owner.0 as usize).ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "residual module imports missing owner {}",
                    external.owner.0
                ))
            })?;
            let target_owner = project
                .owners
                .get(external.owner.0 as usize)
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "residual module imports missing owner input {}",
                        external.owner.0
                    ))
                })?;
            let expression = match external.target {
                KernelExternalTarget::Expression(expression) => checked_expression_index(
                    expression,
                    target_owner.nodes.len(),
                    "residual external expression",
                )?,
                KernelExternalTarget::Result => checked_expression_index(
                    target_owner.result,
                    target_owner.nodes.len(),
                    "residual external result",
                )?,
            };
            target_instance
                .expressions
                .get(expression)
                .copied()
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "residual module imports missing owner {} expression {expression}",
                        external.owner.0
                    ))
                })
        })
        .collect()
}

fn instantiate_owner(
    builder: &mut ComponentProgramBuilder,
    mode_builder: &mut ModeProgramBuilder,
    invocations: &mut HashMap<InvocationKey, OwnerInstance>,
    specializations: &mut HashMap<SpecializationKey, OwnerSpecialization>,
    residual_modules: &mut HashMap<SpecializationKey, Arc<ResidualTypeModule>>,
    compile_work: &mut KernelCompileWork,
    project: &KernelProjectProgramInput,
    principals: &[OwnerInstance],
    formal_dependent_results: &[bool],
    formal_dependent_expressions: &[Box<[bool]>],
    direct_summaries: &[Option<Arc<CompiledDirectSummary>>],
    target: KernelOwnerId,
    actuals: &[CallActual],
    initial_state_surface: bool,
    stack: &mut Vec<KernelOwnerId>,
) -> Result<OwnerInstance, KernelOwnerBuildError> {
    compile_work.max_call_depth = compile_work.max_call_depth.max(stack.len() as u64);
    let owner = project.owners.get(target.0 as usize).ok_or_else(|| {
        KernelOwnerBuildError::new(format!("user call targets missing owner {}", target.0))
    })?;
    if actuals.len() != owner.formal_count as usize {
        return Err(KernelOwnerBuildError::new(format!(
            "user call to owner {} supplies {} actuals for {} formals",
            target.0,
            actuals.len(),
            owner.formal_count
        )));
    }
    if stack.contains(&target) {
        return principals.get(target.0 as usize).cloned().ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "recursive user call targets missing principal owner {}",
                target.0
            ))
        });
    }

    let formal_static_variants = actuals
        .iter()
        .map(|actual| actual.static_variants.clone())
        .collect::<Vec<_>>();
    let specialization_key = SpecializationKey {
        target,
        static_variants: formal_static_variants.clone().into_boxed_slice(),
        initial_state_surface,
    };
    let specialization = if let Some(specialization) = specializations.get(&specialization_key) {
        compile_work.reused_specialization_plans =
            compile_work.reused_specialization_plans.saturating_add(1);
        specialization.clone()
    } else {
        let static_variants = infer_static_variants(owner, &formal_static_variants);
        let result =
            checked_expression_index(owner.result, owner.nodes.len(), "specialized owner result")?;
        let syntax_selected_calls = syntax_selected_call_nodes(
            owner,
            &static_variants,
            &formal_dependent_expressions[target.0 as usize],
        );
        let specialization = OwnerSpecialization {
            reachable: reachable_owner_nodes(owner, result, &static_variants)
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            invocation_dependencies: invocation_expression_dependencies(
                owner,
                &formal_dependent_expressions[target.0 as usize],
                &syntax_selected_calls,
            ),
            syntax_selected_calls,
            static_variants,
            transparent_type_providers: transparent_type_providers(owner),
        };
        specializations.insert(specialization_key.clone(), specialization.clone());
        compile_work.specialization_plans = compile_work.specialization_plans.saturating_add(1);
        specialization
    };
    let key = InvocationKey {
        target,
        actuals: actuals
            .iter()
            .map(|actual| (actual.variable, actual.mode))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        static_variants: formal_static_variants.clone().into_boxed_slice(),
        initial_state_surface,
    };
    let owns_state = owner
        .nodes
        .iter()
        .any(|node| matches!(node.kind, KernelOwnerNodeKind::Hold));
    if !owns_state {
        if let Some(instance) = invocations.get(&key) {
            compile_work.reused_invocation_frames =
                compile_work.reused_invocation_frames.saturating_add(1);
            return Ok(instance.clone());
        }
    }
    compile_work.invocation_frames = compile_work.invocation_frames.saturating_add(1);
    // Invocation frames borrow the caller's provider/mode roots directly.
    // The detached FormalRead occurrences and private requirement roots below
    // still isolate consumers; the redundant actual -> fresh-formal publish
    // layer only obscured identical applications from composition.
    let expression_dependencies = &specialization.invocation_dependencies;
    let principal = &principals[target.0 as usize];
    let (instance, pruned_expressions) = allocate_invocation_owner_instance(
        builder,
        mode_builder,
        owner,
        principal,
        actuals,
        &formal_static_variants,
        specialization.static_variants.clone(),
        expression_dependencies,
        &specialization.reachable,
        &specialization.transparent_type_providers,
    );
    compile_work.pruned_invocation_expressions = compile_work
        .pruned_invocation_expressions
        .saturating_add(pruned_expressions);
    compile_work.principal_expression_reuses =
        compile_work.principal_expression_reuses.saturating_add(
            specialization
                .reachable
                .iter()
                .filter(|index| !expression_dependencies[**index])
                .count() as u64,
        );
    let module = if let Some(module) = residual_modules.get(&specialization_key) {
        Arc::clone(module)
    } else {
        let module = compile_residual_type_module(
            target,
            owner,
            Some(project),
            principals,
            formal_dependent_results,
            formal_dependent_expressions,
            &formal_static_variants,
            &specialization,
            Some(expression_dependencies),
            initial_state_surface,
        )?;
        compile_work.residual_type_modules = compile_work.residual_type_modules.saturating_add(1);
        compile_work.residual_module_operations = compile_work
            .residual_module_operations
            .saturating_add(module.component.operation_count() as u64);
        compile_work.residual_module_terms = compile_work
            .residual_module_terms
            .saturating_add(module.component.terms().len() as u64);
        residual_modules.insert(specialization_key, Arc::clone(&module));
        module
    };
    let external_variables = principal_external_variables(owner, project, principals)?;
    stack.push(target);
    let result = (|| {
        let context = OwnerCompileContext {
            initial_state_surface,
            owner: target,
            input: owner,
            expressions: &instance.expressions,
            formals: &instance.formals,
            formal_requirements: &instance.formal_requirements,
            expression_modes: &instance.expression_modes,
            formal_modes: &instance.formal_modes,
            formal_mode_sources: &instance.formal_mode_sources,
            static_variants: &instance.static_variants,
            formal_static_variants: &instance.formal_static_variants,
            project: Some(project),
            principals,
            formal_dependent_results,
            formal_dependent_expressions,
            external_variables: None,
            syntax_selected_calls: Some(&specialization.syntax_selected_calls),
            direct_summaries,
        };
        append_residual_type_frame(builder, &module, &instance, &external_variables)?;
        compile_work.residual_frames = compile_work.residual_frames.saturating_add(1);
        for index in specialization.reachable.iter().copied() {
            if !expression_dependencies[index] {
                continue;
            }
            let node = &owner.nodes[index];
            if matches!(node.kind, KernelOwnerNodeKind::UserCall { .. }) {
                compile_node(
                    builder,
                    mode_builder,
                    invocations,
                    specializations,
                    residual_modules,
                    compile_work,
                    &context,
                    stack,
                    index,
                    node,
                    instance.expressions[index],
                    instance.expression_modes[index],
                    true,
                )?;
            } else {
                let equation = node_mode_equation(mode_builder, &context, index, node)?;
                mode_builder.set(instance.expression_modes[index], equation);
            }
        }
        Ok::<(), KernelOwnerBuildError>(())
    })();
    let popped = stack.pop();
    debug_assert_eq!(popped, Some(target));
    result?;
    for (actual, requirement) in actuals.iter().zip(&instance.formal_requirements) {
        // A closed or directionally derived actual is provider authority, not
        // a writable formal scaffold. Callee requirements remain useful for
        // open caller formals, but must never widen a concrete occurrence.
        if builder.is_authoritative(actual.variable) && !actual.requirement_backflow {
            continue;
        }
        let actual = builder.variable_term(actual.requirement);
        let requirement = builder.variable_term(*requirement);
        builder.add_unify(actual, requirement);
    }
    if !owns_state {
        invocations.insert(key, instance.clone());
    }
    Ok(instance)
}

fn direct_result_summary_supported(
    project: &KernelProjectProgramInput,
    owner_id: KernelOwnerId,
    expression: usize,
    active: &mut BTreeSet<(KernelOwnerId, usize)>,
) -> bool {
    let Some(owner) = project.owners.get(owner_id.0 as usize) else {
        return false;
    };
    let Some(node) = owner.nodes.get(expression) else {
        return false;
    };
    if !active.insert((owner_id, expression)) {
        return false;
    }
    let child = |edge: &KernelOwnerInputEdge, active: &mut BTreeSet<(KernelOwnerId, usize)>| {
        direct_result_summary_supported(project, owner_id, edge.expression.0 as usize, active)
    };
    let supported = match &node.kind {
        KernelOwnerNodeKind::Known(ty) | KernelOwnerNodeKind::Source(ty) => {
            type_is_recursively_closed(ty)
        }
        KernelOwnerNodeKind::Absent
        | KernelOwnerNodeKind::Text
        | KernelOwnerNodeKind::Number
        | KernelOwnerNodeKind::Byte
        | KernelOwnerNodeKind::Bits(_)
        | KernelOwnerNodeKind::Tag(_)
        | KernelOwnerNodeKind::FormalRead { .. }
        | KernelOwnerNodeKind::ContextRead { .. } => node.inputs.is_empty(),
        KernelOwnerNodeKind::TextTemplate => node.inputs.iter().all(|edge| {
            matches!(edge.role, KernelOwnerEdgeRole::TextDynamic) && child(edge, active)
        }),
        KernelOwnerNodeKind::Record { .. } => node.inputs.iter().all(|edge| {
            matches!(edge.role, KernelOwnerEdgeRole::RecordField { .. }) && child(edge, active)
        }),
        KernelOwnerNodeKind::Collection { kind, .. } => match kind {
            KernelCollectionKind::List
            | KernelCollectionKind::Bytes
            | KernelCollectionKind::Set => node.inputs.iter().all(|edge| {
                matches!(edge.role, KernelOwnerEdgeRole::CollectionItem) && child(edge, active)
            }),
            KernelCollectionKind::Map => node.inputs.iter().all(|edge| {
                matches!(edge.role, KernelOwnerEdgeRole::MapEntry) && child(edge, active)
            }),
        },
        KernelOwnerNodeKind::MapEntry => node.inputs.iter().all(|edge| {
            matches!(
                edge.role,
                KernelOwnerEdgeRole::MapKey | KernelOwnerEdgeRole::MapValue
            ) && child(edge, active)
        }),
        KernelOwnerNodeKind::Block => {
            let results = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::BlockResult))
                .collect::<Vec<_>>();
            matches!(results.as_slice(), [result] if child(result, active))
        }
        KernelOwnerNodeKind::Draining => {
            let inputs = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::DrainingInput))
                .collect::<Vec<_>>();
            matches!(inputs.as_slice(), [input] if child(input, active))
        }
        KernelOwnerNodeKind::LexicalRead { .. }
        | KernelOwnerNodeKind::ValueRead { .. }
        | KernelOwnerNodeKind::DerivedRead { .. } => {
            let providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider))
                .collect::<Vec<_>>();
            matches!(providers.as_slice(), [provider] if {
                let reference = provider.expression.0 as usize;
                if reference < owner.nodes.len() {
                    direct_result_summary_supported(project, owner_id, reference, active)
                } else {
                    owner
                        .external_expressions
                        .get(reference - owner.nodes.len())
                        .is_some()
                }
            })
        }
        // A pattern projection owns tag-sensitive formal shaping and cannot
        // be represented by the generic field-projection summary bytecode.
        KernelOwnerNodeKind::PatternRead { .. } => false,
        KernelOwnerNodeKind::UserCall { target, .. } => {
            let Some(target_owner) = project.owners.get(target.0 as usize) else {
                active.remove(&(owner_id, expression));
                return false;
            };
            node.inputs.iter().all(|edge| {
                matches!(edge.role, KernelOwnerEdgeRole::CallArgument { .. }) && child(edge, active)
            }) && direct_result_summary_supported(
                project,
                *target,
                target_owner.result.0 as usize,
                active,
            )
        }
        KernelOwnerNodeKind::RenderConstructor { .. } => node.inputs.iter().all(|edge| {
            matches!(edge.role, KernelOwnerEdgeRole::AbiArgument { .. }) && child(edge, active)
        }),
        KernelOwnerNodeKind::PureBuiltin { kind }
            if direct_summary_fixed_builtin_supported(*kind) =>
        {
            node.inputs.iter().all(|edge| {
                matches!(edge.role, KernelOwnerEdgeRole::AbiArgument { .. }) && child(edge, active)
            })
        }
        KernelOwnerNodeKind::When => {
            let selectors = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenInput))
                .collect::<Vec<_>>();
            matches!(selectors.as_slice(), [selector] if child(selector, active))
                && node.inputs.iter().all(|edge| match edge.role {
                    KernelOwnerEdgeRole::WhenInput => true,
                    KernelOwnerEdgeRole::WhenArm => child(edge, active),
                    _ => false,
                })
        }
        KernelOwnerNodeKind::MatchArm { .. } => {
            let outputs = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::MatchOutput))
                .collect::<Vec<_>>();
            if !outputs.is_empty() {
                matches!(outputs.as_slice(), [output] if node.inputs.len() == 1 && child(output, active))
            } else if node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::RecordField { .. }))
            {
                node.inputs.iter().all(|edge| {
                    matches!(edge.role, KernelOwnerEdgeRole::RecordField { .. })
                        && child(edge, active)
                })
            } else {
                node.inputs.is_empty()
            }
        }
        KernelOwnerNodeKind::Then => {
            let has_output = node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::ThenOutput));
            let selected = node
                .inputs
                .iter()
                .filter(|edge| {
                    matches!(edge.role, KernelOwnerEdgeRole::ThenOutput)
                        || (!has_output && matches!(edge.role, KernelOwnerEdgeRole::ThenInput))
                })
                .collect::<Vec<_>>();
            matches!(selected.as_slice(), [value] if child(value, active))
                && node.inputs.iter().all(|edge| {
                    matches!(
                        edge.role,
                        KernelOwnerEdgeRole::ThenInput | KernelOwnerEdgeRole::ThenOutput
                    ) && child(edge, active)
                })
        }
        KernelOwnerNodeKind::Infix { .. } => {
            let left = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::InfixLeft))
                .collect::<Vec<_>>();
            let right = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::InfixRight))
                .collect::<Vec<_>>();
            matches!(left.as_slice(), [left] if child(left, active))
                && matches!(right.as_slice(), [right] if child(right, active))
                && node.inputs.len() == 2
        }
        KernelOwnerNodeKind::Arrow => {
            let outputs = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ArrowOutput))
                .collect::<Vec<_>>();
            matches!(outputs.as_slice(), [output] if node.inputs.len() == 1 && child(output, active))
        }
        KernelOwnerNodeKind::Delimiter
        | KernelOwnerNodeKind::Unknown
        | KernelOwnerNodeKind::FreshOut => node.inputs.is_empty(),
        _ => false,
    };
    active.remove(&(owner_id, expression));
    supported
}

fn direct_summary_fixed_builtin_supported(kind: KernelPureBuiltinKind) -> bool {
    matches!(
        kind,
        KernelPureBuiltinKind::TextConstant
            | KernelPureBuiltinKind::TextTransform
            | KernelPureBuiltinKind::TextSlice
            | KernelPureBuiltinKind::TextLength
            | KernelPureBuiltinKind::TextConcat
            | KernelPureBuiltinKind::TextPredicate
            | KernelPureBuiltinKind::TextToNumber
            | KernelPureBuiltinKind::NumberToText
            | KernelPureBuiltinKind::NumberMath
            | KernelPureBuiltinKind::NumberRound
            | KernelPureBuiltinKind::NumberProjection
            | KernelPureBuiltinKind::Boolean
            | KernelPureBuiltinKind::RecordConstructor
            | KernelPureBuiltinKind::TextJoin
            | KernelPureBuiltinKind::FieldColor
    )
}

/// Tiny summary definitions cost less to inline than to enter through a
/// nested scratch frame. Larger definitions remain shared so call-heavy
/// projects do not duplicate their bytecode at every occurrence.
const SHARED_SUMMARY_MIN_NODES: usize = 128;

#[derive(Clone, Debug)]
enum DirectSummaryInput {
    FormalProjection {
        formal: u32,
        fields: Box<[crate::NameId]>,
    },
    External {
        owner: KernelOwnerId,
        expression: usize,
    },
}

#[derive(Clone, Copy, Debug)]
enum DirectSummaryMode {
    Fixed {
        owner: KernelOwnerId,
        expression: usize,
    },
    Input(u32),
}

#[derive(Clone, Debug)]
struct CompiledDirectSummary {
    program: Arc<KernelSummaryProgram>,
    inputs: Box<[DirectSummaryInput]>,
    result_mode: DirectSummaryMode,
    formal_count: usize,
    constant_folded_nodes: u64,
    selector_fused_records: u64,
    deduplicated_nodes: u64,
    pruned_nodes: u64,
    pruned_inputs: u64,
    shared_bytecode: bool,
}

#[derive(Clone, Debug)]
enum PlannedSummaryActual {
    Formal(u32),
    Value(PlannedSummaryValue),
}

#[derive(Clone, Copy, Debug)]
struct PlannedSummaryValue {
    value: KernelSummaryValueId,
    mode: DirectSummaryMode,
    formal_projection_input: Option<u32>,
}

struct DirectSummaryPlanCompiler<'a> {
    builder: &'a mut ComponentProgramBuilder,
    project: &'a KernelProjectProgramInput,
    summaries: &'a [Option<Arc<CompiledDirectSummary>>],
    nodes: Vec<KernelSummaryNode>,
    inputs: Vec<DirectSummaryInput>,
    formal_projection_inputs: HashMap<(u32, Box<[crate::NameId]>), (u32, KernelSummaryValueId)>,
}

impl DirectSummaryPlanCompiler<'_> {
    fn fold_constant_nodes(&mut self) -> (u64, u64) {
        fold_constant_summary_nodes(self.builder, &mut self.nodes)
    }

    fn compact_result(&mut self, result: &mut PlannedSummaryValue) -> (u64, u64) {
        compact_summary_result(&mut self.nodes, &mut self.inputs, result)
    }

    fn deduplicate_nodes(&mut self, result: &mut PlannedSummaryValue) -> u64 {
        deduplicate_summary_nodes(&mut self.nodes, result)
    }

    fn push_node(&mut self, node: KernelSummaryNode) -> KernelSummaryValueId {
        let id = KernelSummaryValueId(
            u32::try_from(self.nodes.len()).expect("kernel summary value count exceeds u32"),
        );
        self.nodes.push(node);
        id
    }

    fn push_formal_projection(
        &mut self,
        formal: u32,
        fields: Box<[crate::NameId]>,
    ) -> PlannedSummaryValue {
        let key = (formal, fields.clone());
        if let Some((input, value)) = self.formal_projection_inputs.get(&key).copied() {
            return PlannedSummaryValue {
                value,
                mode: DirectSummaryMode::Input(input),
                formal_projection_input: Some(input),
            };
        }
        let input =
            u32::try_from(self.inputs.len()).expect("kernel summary input count exceeds u32");
        self.inputs
            .push(DirectSummaryInput::FormalProjection { formal, fields });
        let value = self.push_node(KernelSummaryNode::Input(input));
        self.formal_projection_inputs.insert(key, (input, value));
        PlannedSummaryValue {
            value,
            mode: DirectSummaryMode::Input(input),
            formal_projection_input: Some(input),
        }
    }

    fn project_value(
        &mut self,
        value: PlannedSummaryValue,
        fields: &[Box<str>],
        mode: DirectSummaryMode,
    ) -> Option<PlannedSummaryValue> {
        if fields.is_empty() {
            return Some(value);
        }
        let fields = fields
            .iter()
            .map(|field| self.builder.terms_mut().intern_name(field))
            .collect::<Vec<_>>();
        if let Some(input) = value.formal_projection_input {
            let DirectSummaryInput::FormalProjection {
                formal,
                fields: prefix,
            } = self.inputs.get(input as usize)?.clone()
            else {
                return None;
            };
            let mut projection = prefix.into_vec();
            projection.extend(fields);
            return Some(self.push_formal_projection(formal, projection.into_boxed_slice()));
        }
        Some(PlannedSummaryValue {
            value: self.push_node(KernelSummaryNode::Projection {
                provider: value.value,
                fields: fields.into_boxed_slice(),
            }),
            mode,
            formal_projection_input: None,
        })
    }

    fn project_interned_formal_value(
        &mut self,
        value: PlannedSummaryValue,
        fields: &[crate::NameId],
    ) -> Option<PlannedSummaryValue> {
        if fields.is_empty() {
            return Some(value);
        }
        let input = value.formal_projection_input?;
        let DirectSummaryInput::FormalProjection {
            formal,
            fields: prefix,
        } = self.inputs.get(input as usize)?.clone()
        else {
            return None;
        };
        let mut projection = prefix.into_vec();
        projection.extend_from_slice(fields);
        Some(self.push_formal_projection(formal, projection.into_boxed_slice()))
    }

    fn compile_shared_invoke(
        &mut self,
        summary: &CompiledDirectSummary,
        actuals: &[PlannedSummaryActual],
    ) -> Option<PlannedSummaryValue> {
        if actuals.len() != summary.formal_count {
            return None;
        }
        let mut values = Vec::with_capacity(summary.inputs.len());
        let mut modes = Vec::with_capacity(summary.inputs.len());
        for input in summary.inputs.iter() {
            let value = match input {
                DirectSummaryInput::FormalProjection { formal, fields } => {
                    match actuals.get(*formal as usize)? {
                        PlannedSummaryActual::Formal(formal) => {
                            self.push_formal_projection(*formal, fields.clone())
                        }
                        PlannedSummaryActual::Value(value) => {
                            self.project_interned_formal_value(*value, fields)?
                        }
                    }
                }
                DirectSummaryInput::External { owner, expression } => {
                    let input = u32::try_from(self.inputs.len())
                        .expect("kernel summary input count exceeds u32");
                    self.inputs.push(DirectSummaryInput::External {
                        owner: *owner,
                        expression: *expression,
                    });
                    PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Input(input)),
                        mode: DirectSummaryMode::Fixed {
                            owner: *owner,
                            expression: *expression,
                        },
                        formal_projection_input: None,
                    }
                }
            };
            values.push(value.value);
            modes.push(value.mode);
        }
        let mode = match summary.result_mode {
            DirectSummaryMode::Fixed { owner, expression } => {
                DirectSummaryMode::Fixed { owner, expression }
            }
            DirectSummaryMode::Input(input) => *modes.get(input as usize)?,
        };
        Some(PlannedSummaryValue {
            value: self.push_node(KernelSummaryNode::Invoke {
                program: Arc::clone(&summary.program),
                inputs: values.into_boxed_slice(),
            }),
            mode,
            formal_projection_input: None,
        })
    }

    fn compile_expression(
        &mut self,
        owner_id: KernelOwnerId,
        expression: usize,
        actuals: &[PlannedSummaryActual],
        active: &mut BTreeSet<(KernelOwnerId, usize)>,
    ) -> Option<PlannedSummaryValue> {
        if !active.insert((owner_id, expression)) {
            return None;
        }
        let result = (|| {
            let owner = self.project.owners.get(owner_id.0 as usize)?;
            let node = owner.nodes.get(expression)?;
            let fixed_mode = DirectSummaryMode::Fixed {
                owner: owner_id,
                expression,
            };
            let term_value = |compiler: &mut Self, term| PlannedSummaryValue {
                value: compiler.push_node(KernelSummaryNode::Term(term)),
                mode: fixed_mode,
                formal_projection_input: None,
            };
            match &node.kind {
                KernelOwnerNodeKind::Known(ty) | KernelOwnerNodeKind::Source(ty)
                    if type_is_recursively_closed(ty) =>
                {
                    let value = self.builder.terms_mut().import_checked_type(ty, &mut |_| {
                        unreachable!("compiled direct-summary ABI type is closed")
                    });
                    Some(term_value(self, value))
                }
                KernelOwnerNodeKind::Absent => {
                    let value = self.builder.terms().absent();
                    Some(term_value(self, value))
                }
                KernelOwnerNodeKind::Text => {
                    let value = self.builder.terms().text();
                    Some(term_value(self, value))
                }
                KernelOwnerNodeKind::TextTemplate => {
                    let mut dependencies = Vec::with_capacity(node.inputs.len());
                    for edge in &node.inputs {
                        if !matches!(edge.role, KernelOwnerEdgeRole::TextDynamic) {
                            return None;
                        }
                        dependencies.push(
                            self.compile_expression(
                                owner_id,
                                edge.expression.0 as usize,
                                actuals,
                                active,
                            )?
                            .value,
                        );
                    }
                    let text = self.builder.terms().text();
                    let result = self.push_node(KernelSummaryNode::Term(text));
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Sequence {
                            inputs: dependencies.into_boxed_slice(),
                            result,
                        }),
                        mode: fixed_mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::Number => {
                    let value = self.builder.terms().number();
                    Some(term_value(self, value))
                }
                KernelOwnerNodeKind::Byte => {
                    let value = self.builder.terms_mut().bytes(crate::BytesTerm::Fixed(1));
                    Some(term_value(self, value))
                }
                KernelOwnerNodeKind::Bits(width) => {
                    let value = self.builder.terms_mut().bits(*width);
                    Some(term_value(self, value))
                }
                KernelOwnerNodeKind::Tag(tag) => {
                    let tag = self.builder.terms_mut().variant_tag(tag);
                    let value = self.builder.terms_mut().variant_set([tag]);
                    Some(term_value(self, value))
                }
                KernelOwnerNodeKind::FormalRead { formal, fields }
                | KernelOwnerNodeKind::ContextRead { formal, fields } => {
                    let actual = actuals.get(*formal as usize)?;
                    match actual {
                        PlannedSummaryActual::Formal(formal) => {
                            let fields = fields
                                .iter()
                                .map(|field| self.builder.terms_mut().intern_name(field))
                                .collect::<Vec<_>>()
                                .into_boxed_slice();
                            Some(self.push_formal_projection(*formal, fields))
                        }
                        PlannedSummaryActual::Value(value) => {
                            self.project_value(*value, fields, fixed_mode)
                        }
                    }
                }
                KernelOwnerNodeKind::Record { tag } => {
                    let mut entries = Vec::with_capacity(node.inputs.len());
                    for edge in &node.inputs {
                        let KernelOwnerEdgeRole::RecordField { name, spread } = &edge.role else {
                            return None;
                        };
                        let value = self.compile_expression(
                            owner_id,
                            edge.expression.0 as usize,
                            actuals,
                            active,
                        )?;
                        if *spread {
                            entries.push(KernelSummaryRecordEntry::Spread { value: value.value });
                        } else {
                            let name = self.builder.terms_mut().intern_name(name);
                            entries.push(KernelSummaryRecordEntry::Field {
                                name,
                                value: value.value,
                            });
                        }
                    }
                    let tag = tag
                        .as_ref()
                        .map(|tag| self.builder.terms_mut().intern_name(tag));
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Record {
                            tag,
                            entries: entries.into_boxed_slice(),
                        }),
                        mode: fixed_mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::Collection { kind, .. } => match kind {
                    KernelCollectionKind::List | KernelCollectionKind::Set => {
                        let mut inputs = Vec::with_capacity(node.inputs.len());
                        for edge in &node.inputs {
                            if !matches!(edge.role, KernelOwnerEdgeRole::CollectionItem) {
                                return None;
                            }
                            inputs.push(
                                self.compile_expression(
                                    owner_id,
                                    edge.expression.0 as usize,
                                    actuals,
                                    active,
                                )?
                                .value,
                            );
                        }
                        let kind = match kind {
                            KernelCollectionKind::List => KernelCollectionOperationKind::List,
                            KernelCollectionKind::Set => KernelCollectionOperationKind::Set,
                            _ => unreachable!(),
                        };
                        Some(PlannedSummaryValue {
                            value: self.push_node(KernelSummaryNode::Collection {
                                kind,
                                inputs: inputs.into_boxed_slice(),
                                values: Box::new([]),
                            }),
                            mode: fixed_mode,
                            formal_projection_input: None,
                        })
                    }
                    KernelCollectionKind::Bytes => {
                        let mut dependencies = Vec::with_capacity(node.inputs.len());
                        for edge in &node.inputs {
                            if !matches!(edge.role, KernelOwnerEdgeRole::CollectionItem) {
                                return None;
                            }
                            dependencies.push(
                                self.compile_expression(
                                    owner_id,
                                    edge.expression.0 as usize,
                                    actuals,
                                    active,
                                )?
                                .value,
                            );
                        }
                        let bytes = self
                            .builder
                            .terms_mut()
                            .bytes(crate::BytesTerm::Fixed(node.inputs.len()));
                        let result = self.push_node(KernelSummaryNode::Term(bytes));
                        Some(PlannedSummaryValue {
                            value: self.push_node(KernelSummaryNode::Sequence {
                                inputs: dependencies.into_boxed_slice(),
                                result,
                            }),
                            mode: fixed_mode,
                            formal_projection_input: None,
                        })
                    }
                    KernelCollectionKind::Map => {
                        let mut keys = Vec::new();
                        let mut values = Vec::new();
                        for edge in &node.inputs {
                            if !matches!(edge.role, KernelOwnerEdgeRole::MapEntry) {
                                return None;
                            }
                            let entry = owner.nodes.get(edge.expression.0 as usize)?;
                            if !matches!(entry.kind, KernelOwnerNodeKind::MapEntry) {
                                return None;
                            }
                            for entry_edge in &entry.inputs {
                                let value = self.compile_expression(
                                    owner_id,
                                    entry_edge.expression.0 as usize,
                                    actuals,
                                    active,
                                )?;
                                match entry_edge.role {
                                    KernelOwnerEdgeRole::MapKey => keys.push(value.value),
                                    KernelOwnerEdgeRole::MapValue => values.push(value.value),
                                    _ => return None,
                                }
                            }
                        }
                        Some(PlannedSummaryValue {
                            value: self.push_node(KernelSummaryNode::Collection {
                                kind: KernelCollectionOperationKind::Map,
                                inputs: keys.into_boxed_slice(),
                                values: values.into_boxed_slice(),
                            }),
                            mode: fixed_mode,
                            formal_projection_input: None,
                        })
                    }
                },
                KernelOwnerNodeKind::MapEntry => {
                    let mut dependencies = Vec::with_capacity(node.inputs.len());
                    for edge in &node.inputs {
                        if !matches!(
                            edge.role,
                            KernelOwnerEdgeRole::MapKey | KernelOwnerEdgeRole::MapValue
                        ) {
                            return None;
                        }
                        dependencies.push(
                            self.compile_expression(
                                owner_id,
                                edge.expression.0 as usize,
                                actuals,
                                active,
                            )?
                            .value,
                        );
                    }
                    let absent = self.builder.terms().absent();
                    let result = self.push_node(KernelSummaryNode::Term(absent));
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Sequence {
                            inputs: dependencies.into_boxed_slice(),
                            result,
                        }),
                        mode: fixed_mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::Block => {
                    let results = node
                        .inputs
                        .iter()
                        .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::BlockResult))
                        .collect::<Vec<_>>();
                    let [result] = results.as_slice() else {
                        return None;
                    };
                    self.compile_expression(owner_id, result.expression.0 as usize, actuals, active)
                }
                KernelOwnerNodeKind::Draining => {
                    let inputs = node
                        .inputs
                        .iter()
                        .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::DrainingInput))
                        .collect::<Vec<_>>();
                    let [input] = inputs.as_slice() else {
                        return None;
                    };
                    self.compile_expression(owner_id, input.expression.0 as usize, actuals, active)
                }
                KernelOwnerNodeKind::LexicalRead { fields }
                | KernelOwnerNodeKind::ValueRead { fields, .. }
                | KernelOwnerNodeKind::DerivedRead { fields } => {
                    let providers = node
                        .inputs
                        .iter()
                        .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider))
                        .collect::<Vec<_>>();
                    let [provider] = providers.as_slice() else {
                        return None;
                    };
                    let reference = provider.expression.0 as usize;
                    let provider = if reference < owner.nodes.len() {
                        self.compile_expression(owner_id, reference, actuals, active)?
                    } else {
                        let external = owner
                            .external_expressions
                            .get(reference - owner.nodes.len())?;
                        let target_owner = self.project.owners.get(external.owner.0 as usize)?;
                        let expression = match external.target {
                            KernelExternalTarget::Expression(expression) => expression.0 as usize,
                            KernelExternalTarget::Result => target_owner.result.0 as usize,
                        };
                        let input = u32::try_from(self.inputs.len())
                            .expect("kernel summary input count exceeds u32");
                        self.inputs.push(DirectSummaryInput::External {
                            owner: external.owner,
                            expression,
                        });
                        PlannedSummaryValue {
                            value: self.push_node(KernelSummaryNode::Input(input)),
                            mode: DirectSummaryMode::Fixed {
                                owner: external.owner,
                                expression,
                            },
                            formal_projection_input: None,
                        }
                    };
                    self.project_value(provider, fields, fixed_mode)
                }
                KernelOwnerNodeKind::PatternRead { .. } => None,
                KernelOwnerNodeKind::UserCall {
                    target,
                    inherited_formal,
                } => {
                    let target_owner = self.project.owners.get(target.0 as usize)?;
                    let mut target_actuals = vec![None; target_owner.formal_count as usize];
                    for edge in &node.inputs {
                        let KernelOwnerEdgeRole::CallArgument { ordinal } = edge.role else {
                            return None;
                        };
                        let value = self.compile_expression(
                            owner_id,
                            edge.expression.0 as usize,
                            actuals,
                            active,
                        )?;
                        let slot = target_actuals.get_mut(ordinal as usize)?;
                        if slot.replace(PlannedSummaryActual::Value(value)).is_some() {
                            return None;
                        }
                    }
                    if let Some(inherited) = inherited_formal {
                        let actual = actuals.get(inherited.caller_ordinal as usize)?.clone();
                        let slot = target_actuals.get_mut(inherited.target_ordinal as usize)?;
                        if slot.replace(actual).is_some() {
                            return None;
                        }
                    }
                    let target_actuals = target_actuals.into_iter().collect::<Option<Vec<_>>>()?;
                    let shared = self
                        .summaries
                        .get(target.0 as usize)
                        .and_then(Clone::clone)
                        .filter(|summary| summary.shared_bytecode);
                    if let Some(shared) = shared
                        && let Some(result) =
                            self.compile_shared_invoke(shared.as_ref(), &target_actuals)
                    {
                        return Some(result);
                    }
                    self.compile_expression(
                        *target,
                        target_owner.result.0 as usize,
                        &target_actuals,
                        active,
                    )
                }
                KernelOwnerNodeKind::RenderConstructor { kind } => {
                    let mut entries = Vec::with_capacity(node.inputs.len() + 1);
                    let mut direction = None;
                    for edge in &node.inputs {
                        let KernelOwnerEdgeRole::AbiArgument { name } = &edge.role else {
                            return None;
                        };
                        let mut value = self.compile_expression(
                            owner_id,
                            edge.expression.0 as usize,
                            actuals,
                            active,
                        )?;
                        if let Some(expected) =
                            render_argument_requirement(self.builder, name.as_ref())
                        {
                            value.value = self.push_node(KernelSummaryNode::Constrain {
                                value: value.value,
                                expected,
                            });
                        }
                        if name.as_ref() == "direction" {
                            direction = Some(value.value);
                        }
                        let name = self.builder.terms_mut().intern_name(name);
                        entries.push(KernelSummaryRecordEntry::Field {
                            name,
                            value: value.value,
                        });
                    }
                    let kind = match kind {
                        KernelRenderConstructorKind::Fixed(tag) => {
                            let tag = self.builder.terms_mut().variant_tag(tag);
                            let term = self.builder.terms_mut().variant_set([tag]);
                            self.push_node(KernelSummaryNode::Term(term))
                        }
                        KernelRenderConstructorKind::StripeDirection => {
                            let row_tag = self.builder.terms_mut().variant_tag("Row");
                            let row = self.builder.terms_mut().variant_set([row_tag]);
                            let stack_tag = self.builder.terms_mut().variant_tag("Stack");
                            let stack = self.builder.terms_mut().variant_set([stack_tag]);
                            let fallback = self.builder.terms_mut().union([row, stack]);
                            let fallback_value = self.push_node(KernelSummaryNode::Term(fallback));
                            if let Some(direction) = direction {
                                let row_value = self.push_node(KernelSummaryNode::Term(row));
                                let stack_value = self.push_node(KernelSummaryNode::Term(stack));
                                self.push_node(KernelSummaryNode::Select {
                                    selector: direction,
                                    arms: vec![
                                        KernelSummarySelectArm {
                                            pattern: KernelPattern::Tag {
                                                name: "Row".into(),
                                                fields: Box::new([]),
                                            },
                                            output: row_value,
                                        },
                                        KernelSummarySelectArm {
                                            pattern: KernelPattern::Tag {
                                                name: "Column".into(),
                                                fields: Box::new([]),
                                            },
                                            output: stack_value,
                                        },
                                        KernelSummarySelectArm {
                                            pattern: KernelPattern::Wildcard,
                                            output: fallback_value,
                                        },
                                    ]
                                    .into_boxed_slice(),
                                })
                            } else {
                                fallback_value
                            }
                        }
                    };
                    let kind_name = self.builder.terms_mut().intern_name("kind");
                    entries.push(KernelSummaryRecordEntry::Field {
                        name: kind_name,
                        value: kind,
                    });
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Record {
                            tag: None,
                            entries: entries.into_boxed_slice(),
                        }),
                        mode: fixed_mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::PureBuiltin { kind }
                    if direct_summary_fixed_builtin_supported(*kind) =>
                {
                    let mut dependencies = Vec::with_capacity(node.inputs.len());
                    let mut record_entries = Vec::with_capacity(node.inputs.len());
                    let mut names = BTreeSet::new();
                    for edge in &node.inputs {
                        let KernelOwnerEdgeRole::AbiArgument { name } = &edge.role else {
                            return None;
                        };
                        if !names.insert(name.as_ref()) {
                            return None;
                        }
                        let mut value = self.compile_expression(
                            owner_id,
                            edge.expression.0 as usize,
                            actuals,
                            active,
                        )?;
                        if let Some(expected) =
                            pure_builtin_argument_requirement(self.builder, *kind, name.as_ref())
                        {
                            value.value = self.push_node(KernelSummaryNode::Constrain {
                                value: value.value,
                                expected,
                            });
                        }
                        if matches!(kind, KernelPureBuiltinKind::RecordConstructor) {
                            record_entries.push(KernelSummaryRecordEntry::Field {
                                name: self.builder.terms_mut().intern_name(name),
                                value: value.value,
                            });
                        }
                        dependencies.push(value.value);
                    }
                    let result = match kind {
                        KernelPureBuiltinKind::TextConstant
                        | KernelPureBuiltinKind::TextTransform
                        | KernelPureBuiltinKind::TextSlice
                        | KernelPureBuiltinKind::TextConcat
                        | KernelPureBuiltinKind::NumberToText
                        | KernelPureBuiltinKind::TextJoin
                        | KernelPureBuiltinKind::FieldColor => self.builder.terms().text(),
                        KernelPureBuiltinKind::NumberMath
                        | KernelPureBuiltinKind::NumberRound
                        | KernelPureBuiltinKind::NumberProjection
                        | KernelPureBuiltinKind::TextLength => self.builder.terms().number(),
                        KernelPureBuiltinKind::TextPredicate | KernelPureBuiltinKind::Boolean => {
                            boolean_type(self.builder)
                        }
                        KernelPureBuiltinKind::TextToNumber => parsed_number_type(self.builder),
                        KernelPureBuiltinKind::RecordConstructor => {
                            let value = self.push_node(KernelSummaryNode::Record {
                                tag: None,
                                entries: record_entries.into_boxed_slice(),
                            });
                            return Some(PlannedSummaryValue {
                                value,
                                mode: fixed_mode,
                                formal_projection_input: None,
                            });
                        }
                        _ => return None,
                    };
                    let result = self.push_node(KernelSummaryNode::Term(result));
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Sequence {
                            inputs: dependencies.into_boxed_slice(),
                            result,
                        }),
                        mode: fixed_mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::When => {
                    let mut selector = None;
                    let mut arms = Vec::new();
                    for edge in &node.inputs {
                        match edge.role {
                            KernelOwnerEdgeRole::WhenInput => {
                                if selector.is_some() {
                                    return None;
                                }
                                selector = Some(self.compile_expression(
                                    owner_id,
                                    edge.expression.0 as usize,
                                    actuals,
                                    active,
                                )?);
                            }
                            KernelOwnerEdgeRole::WhenArm => {
                                let arm = owner.nodes.get(edge.expression.0 as usize)?;
                                let KernelOwnerNodeKind::MatchArm { pattern } = &arm.kind else {
                                    return None;
                                };
                                let output = self.compile_expression(
                                    owner_id,
                                    edge.expression.0 as usize,
                                    actuals,
                                    active,
                                )?;
                                arms.push(KernelSummarySelectArm {
                                    pattern: pattern.clone(),
                                    output: output.value,
                                });
                            }
                            _ => return None,
                        }
                    }
                    let selector = selector?;
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Select {
                            selector: selector.value,
                            arms: arms.into_boxed_slice(),
                        }),
                        mode: selector.mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::MatchArm { .. } => {
                    let outputs = node
                        .inputs
                        .iter()
                        .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::MatchOutput))
                        .collect::<Vec<_>>();
                    if let [output] = outputs.as_slice() {
                        if node.inputs.len() != 1 {
                            return None;
                        }
                        self.compile_expression(
                            owner_id,
                            output.expression.0 as usize,
                            actuals,
                            active,
                        )
                    } else if outputs.is_empty()
                        && node.inputs.iter().all(|edge| {
                            matches!(edge.role, KernelOwnerEdgeRole::RecordField { .. })
                        })
                        && !node.inputs.is_empty()
                    {
                        let mut entries = Vec::with_capacity(node.inputs.len());
                        for edge in &node.inputs {
                            let KernelOwnerEdgeRole::RecordField { name, spread } = &edge.role
                            else {
                                return None;
                            };
                            let value = self.compile_expression(
                                owner_id,
                                edge.expression.0 as usize,
                                actuals,
                                active,
                            )?;
                            if *spread {
                                entries
                                    .push(KernelSummaryRecordEntry::Spread { value: value.value });
                            } else {
                                let name = self.builder.terms_mut().intern_name(name);
                                entries.push(KernelSummaryRecordEntry::Field {
                                    name,
                                    value: value.value,
                                });
                            }
                        }
                        Some(PlannedSummaryValue {
                            value: self.push_node(KernelSummaryNode::Record {
                                tag: None,
                                entries: entries.into_boxed_slice(),
                            }),
                            mode: fixed_mode,
                            formal_projection_input: None,
                        })
                    } else if outputs.is_empty() && node.inputs.is_empty() {
                        let absent = self.builder.terms().absent();
                        Some(term_value(self, absent))
                    } else {
                        None
                    }
                }
                KernelOwnerNodeKind::Then => {
                    let has_output = node
                        .inputs
                        .iter()
                        .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::ThenOutput));
                    let mut dependencies = Vec::with_capacity(node.inputs.len());
                    let mut selected = None;
                    for edge in &node.inputs {
                        if !matches!(
                            edge.role,
                            KernelOwnerEdgeRole::ThenInput | KernelOwnerEdgeRole::ThenOutput
                        ) {
                            return None;
                        }
                        let value = self.compile_expression(
                            owner_id,
                            edge.expression.0 as usize,
                            actuals,
                            active,
                        )?;
                        if matches!(edge.role, KernelOwnerEdgeRole::ThenOutput)
                            || (!has_output && matches!(edge.role, KernelOwnerEdgeRole::ThenInput))
                        {
                            if selected.replace(value.value).is_some() {
                                return None;
                            }
                        }
                        dependencies.push(value.value);
                    }
                    let selected = selected?;
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Sequence {
                            inputs: dependencies.into_boxed_slice(),
                            result: selected,
                        }),
                        mode: fixed_mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::Infix { operation } => {
                    let mut left = None;
                    let mut right = None;
                    for edge in &node.inputs {
                        let slot = match edge.role {
                            KernelOwnerEdgeRole::InfixLeft => &mut left,
                            KernelOwnerEdgeRole::InfixRight => &mut right,
                            _ => return None,
                        };
                        if slot
                            .replace(self.compile_expression(
                                owner_id,
                                edge.expression.0 as usize,
                                actuals,
                                active,
                            )?)
                            .is_some()
                        {
                            return None;
                        }
                    }
                    let (Some(mut left), Some(mut right)) = (left, right) else {
                        return None;
                    };
                    if infix_requires_number_operands(operation) {
                        let number = self.builder.terms().number();
                        left.value = self.push_node(KernelSummaryNode::Constrain {
                            value: left.value,
                            expected: number,
                        });
                        right.value = self.push_node(KernelSummaryNode::Constrain {
                            value: right.value,
                            expected: number,
                        });
                    }
                    let result = if infix_returns_bool(operation) {
                        boolean_type(self.builder)
                    } else {
                        self.builder.terms().number()
                    };
                    let result = self.push_node(KernelSummaryNode::Term(result));
                    Some(PlannedSummaryValue {
                        value: self.push_node(KernelSummaryNode::Sequence {
                            inputs: vec![left.value, right.value].into_boxed_slice(),
                            result,
                        }),
                        mode: fixed_mode,
                        formal_projection_input: None,
                    })
                }
                KernelOwnerNodeKind::Arrow => {
                    let outputs = node
                        .inputs
                        .iter()
                        .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ArrowOutput))
                        .collect::<Vec<_>>();
                    let [output] = outputs.as_slice() else {
                        return None;
                    };
                    if node.inputs.len() != 1 {
                        return None;
                    }
                    self.compile_expression(owner_id, output.expression.0 as usize, actuals, active)
                }
                KernelOwnerNodeKind::Delimiter | KernelOwnerNodeKind::Unknown
                    if node.inputs.is_empty() =>
                {
                    let unknown = self.builder.terms().unknown();
                    Some(term_value(self, unknown))
                }
                _ => None,
            }
        })();
        active.remove(&(owner_id, expression));
        result
    }
}

/// Replace definition-constant summary subgraphs with one interned type term.
///
/// The summary evaluator is deliberately lazy across SELECT arms, so this pass
/// never evaluates an input, a requirement, or a nested invocation. It folds
/// only closed term algebra whose value cannot vary by call occurrence. Large
/// library constructors consequently own their constant record/collection
/// shapes once instead of rebuilding and re-interning them on every call.
fn fold_constant_summary_nodes(
    builder: &mut ComponentProgramBuilder,
    nodes: &mut Vec<KernelSummaryNode>,
) -> (u64, u64) {
    let mut constants = vec![None; nodes.len()];
    let mut folded = 0_u64;
    let mut fused_records = 0_u64;
    let mut index = 0;
    while index < nodes.len() {
        let node = nodes[index].clone();
        let constant = match &node {
            KernelSummaryNode::Input(_)
            | KernelSummaryNode::Constrain { .. }
            | KernelSummaryNode::Invoke { .. } => None,
            KernelSummaryNode::Term(term) => {
                (!builder.terms().has_variable(*term)).then_some(*term)
            }
            KernelSummaryNode::Projection { provider, fields } => {
                constant_summary_projection(builder, *provider, fields, &constants)
            }
            KernelSummaryNode::Sequence {
                inputs: dependencies,
                result,
            } => dependencies
                .iter()
                .all(|dependency| summary_constant(&constants, *dependency).is_some())
                .then(|| summary_constant(&constants, *result))
                .flatten(),
            KernelSummaryNode::Collection {
                kind,
                inputs,
                values,
            } => constant_summary_collection(builder, *kind, inputs, values, &constants),
            KernelSummaryNode::Select { selector, arms } => {
                constant_summary_select(builder, *selector, arms, &constants)
            }
            KernelSummaryNode::Record { tag, entries } => {
                constant_summary_record(builder, *tag, entries, &constants)
            }
        };
        constants[index] = constant;
        if let Some(constant) = constant {
            if !matches!(node, KernelSummaryNode::Term(term) if term == constant) {
                nodes[index] = KernelSummaryNode::Term(constant);
                folded = folded.saturating_add(1);
            }
        } else if let KernelSummaryNode::Record { tag, entries } = &node
            && let Some((selector, variants)) =
                fuse_constant_summary_record_selectors(builder, *tag, entries, nodes, &constants)
        {
            let arms = variants
                .into_iter()
                .map(|(pattern, term)| {
                    let output = KernelSummaryValueId(
                        u32::try_from(nodes.len()).expect("kernel summary value count exceeds u32"),
                    );
                    nodes.push(KernelSummaryNode::Term(term));
                    constants.push(Some(term));
                    KernelSummarySelectArm { pattern, output }
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            nodes[index] = KernelSummaryNode::Select { selector, arms };
            fused_records = fused_records.saturating_add(1);
        }
        index += 1;
    }
    (folded, fused_records)
}

/// Fuse a record whose dynamic fields all branch on the same selector into
/// one selector over complete closed record terms. Structural widening of the
/// resulting records is equivalent to the original field-wise joins, while a
/// singleton selector now performs one decision instead of one per field.
fn fuse_constant_summary_record_selectors(
    builder: &mut ComponentProgramBuilder,
    tag: Option<crate::NameId>,
    entries: &[KernelSummaryRecordEntry],
    nodes: &[KernelSummaryNode],
    constants: &[Option<TypeTermId>],
) -> Option<(KernelSummaryValueId, Vec<(KernelPattern, TypeTermId)>)> {
    let mut selector = None;
    let mut patterns = None::<Vec<KernelPattern>>;
    let mut has_dynamic_field = false;
    for entry in entries {
        let KernelSummaryRecordEntry::Field { value, .. } = entry else {
            return None;
        };
        if summary_constant(constants, *value).is_some() {
            continue;
        }
        let KernelSummaryNode::Select {
            selector: field_selector,
            arms,
        } = nodes.get(value.0 as usize)?
        else {
            return None;
        };
        if arms.is_empty()
            || arms.iter().any(|arm| {
                summary_constant(constants, arm.output)
                    .is_none_or(|term| matches!(builder.terms().term(term), TypeTerm::Absent))
            })
        {
            return None;
        }
        if selector
            .replace(*field_selector)
            .is_some_and(|previous| previous != *field_selector)
        {
            return None;
        }
        let field_patterns = arms
            .iter()
            .map(|arm| arm.pattern.clone())
            .collect::<Vec<_>>();
        if patterns
            .as_ref()
            .is_some_and(|patterns| *patterns != field_patterns)
        {
            return None;
        }
        patterns.get_or_insert(field_patterns);
        has_dynamic_field = true;
    }
    if !has_dynamic_field {
        return None;
    }
    let selector = selector?;
    let patterns = patterns?;
    let mut variants = Vec::with_capacity(patterns.len());
    for (ordinal, pattern) in patterns.into_iter().enumerate() {
        let mut fields = Vec::with_capacity(entries.len());
        for entry in entries {
            let KernelSummaryRecordEntry::Field { name, value } = entry else {
                unreachable!("record-selector fusion rejected spreads above")
            };
            let value = if let Some(value) = summary_constant(constants, *value) {
                value
            } else {
                let KernelSummaryNode::Select { arms, .. } = &nodes[value.0 as usize] else {
                    unreachable!("record-selector fusion validated every dynamic field")
                };
                summary_constant(constants, arms[ordinal].output)
                    .expect("record-selector fusion validated closed arm outputs")
            };
            insert_constant_summary_field(&mut fields, *name, value);
        }
        let record = intern_constant_summary_record(builder, tag, fields);
        variants.push((pattern, record));
    }
    Some((selector, variants))
}

/// Hash-cons pure nodes inside one immutable definition summary.
///
/// This pass canonicalizes the summary DAG after constant folding, but it does
/// not merge nodes that own requirement or evaluation-order effects. In
/// particular, CONSTRAIN, SEQUENCE, and INVOKE remain distinct even when their
/// printed operands match. Pure parents are eligible only when their complete
/// local dependency graph is also pure. The result is one definition-owned
/// computation for repeated type algebra without copying bytecode into callers.
fn deduplicate_summary_nodes(
    nodes: &mut Vec<KernelSummaryNode>,
    result: &mut PlannedSummaryValue,
) -> u64 {
    let old_nodes = std::mem::take(nodes);
    let old_node_count = old_nodes.len();
    let mut relocations = vec![None; old_node_count];
    let mut pure = vec![false; old_node_count];
    let mut active = vec![false; old_node_count];
    let mut interner = HashMap::<u64, Vec<KernelSummaryValueId>>::new();
    let mut canonical = Vec::with_capacity(old_node_count);
    for index in 0..old_node_count {
        canonicalize_summary_node(
            index,
            &old_nodes,
            &mut relocations,
            &mut pure,
            &mut active,
            &mut interner,
            &mut canonical,
        );
    }
    result.value = relocations[result.value.0 as usize]
        .expect("kernel summary result receives a canonical value");
    *nodes = canonical;
    u64::try_from(old_node_count - nodes.len()).expect("summary CSE delta exceeds u64")
}

#[allow(clippy::too_many_arguments)]
fn canonicalize_summary_node(
    index: usize,
    old_nodes: &[KernelSummaryNode],
    relocations: &mut [Option<KernelSummaryValueId>],
    purities: &mut [bool],
    active: &mut [bool],
    interner: &mut HashMap<u64, Vec<KernelSummaryValueId>>,
    canonical: &mut Vec<KernelSummaryNode>,
) -> (KernelSummaryValueId, bool) {
    if let Some(value) = relocations
        .get(index)
        .copied()
        .expect("kernel summary CSE references an existing node")
    {
        return (value, purities[index]);
    }
    if std::mem::replace(&mut active[index], true) {
        panic!("kernel summary CSE found a value cycle through {index}");
    }
    let mut node = old_nodes[index].clone();
    let is_pure = match &mut node {
        KernelSummaryNode::Input(_) | KernelSummaryNode::Term(_) => true,
        KernelSummaryNode::Projection { provider, .. } => {
            let (value, pure) = canonicalize_summary_value(
                *provider,
                old_nodes,
                relocations,
                purities,
                active,
                interner,
                canonical,
            );
            *provider = value;
            pure
        }
        KernelSummaryNode::Constrain { value, .. } => {
            *value = canonicalize_summary_value(
                *value,
                old_nodes,
                relocations,
                purities,
                active,
                interner,
                canonical,
            )
            .0;
            false
        }
        KernelSummaryNode::Sequence {
            inputs: dependencies,
            result,
        } => {
            for dependency in dependencies {
                *dependency = canonicalize_summary_value(
                    *dependency,
                    old_nodes,
                    relocations,
                    purities,
                    active,
                    interner,
                    canonical,
                )
                .0;
            }
            *result = canonicalize_summary_value(
                *result,
                old_nodes,
                relocations,
                purities,
                active,
                interner,
                canonical,
            )
            .0;
            false
        }
        KernelSummaryNode::Collection { inputs, values, .. } => {
            let mut pure = true;
            for value in inputs.iter_mut().chain(values.iter_mut()) {
                let (canonical_value, value_is_pure) = canonicalize_summary_value(
                    *value,
                    old_nodes,
                    relocations,
                    purities,
                    active,
                    interner,
                    canonical,
                );
                *value = canonical_value;
                pure &= value_is_pure;
            }
            pure
        }
        KernelSummaryNode::Invoke {
            inputs: arguments, ..
        } => {
            for argument in arguments {
                *argument = canonicalize_summary_value(
                    *argument,
                    old_nodes,
                    relocations,
                    purities,
                    active,
                    interner,
                    canonical,
                )
                .0;
            }
            false
        }
        KernelSummaryNode::Select { selector, arms } => {
            let (canonical_selector, mut pure) = canonicalize_summary_value(
                *selector,
                old_nodes,
                relocations,
                purities,
                active,
                interner,
                canonical,
            );
            *selector = canonical_selector;
            for arm in arms {
                let (output, output_is_pure) = canonicalize_summary_value(
                    arm.output,
                    old_nodes,
                    relocations,
                    purities,
                    active,
                    interner,
                    canonical,
                );
                arm.output = output;
                pure &= output_is_pure;
            }
            pure
        }
        KernelSummaryNode::Record { entries, .. } => {
            let mut pure = true;
            for entry in entries {
                let value = match entry {
                    KernelSummaryRecordEntry::Field { value, .. }
                    | KernelSummaryRecordEntry::Spread { value } => value,
                };
                let (canonical_value, value_is_pure) = canonicalize_summary_value(
                    *value,
                    old_nodes,
                    relocations,
                    purities,
                    active,
                    interner,
                    canonical,
                );
                *value = canonical_value;
                pure &= value_is_pure;
            }
            pure
        }
    };
    let hash = is_pure.then(|| pure_summary_node_hash(&node));
    let value = match hash {
        Some(hash) => {
            let existing = interner.get(&hash).and_then(|bucket| {
                bucket
                    .iter()
                    .copied()
                    .find(|candidate| canonical[candidate.0 as usize] == node)
            });
            if let Some(existing) = existing {
                existing
            } else {
                let value = KernelSummaryValueId(
                    u32::try_from(canonical.len()).expect("kernel summary value count exceeds u32"),
                );
                canonical.push(node);
                interner.entry(hash).or_default().push(value);
                value
            }
        }
        None => {
            let value = KernelSummaryValueId(
                u32::try_from(canonical.len()).expect("kernel summary value count exceeds u32"),
            );
            canonical.push(node);
            value
        }
    };
    active[index] = false;
    relocations[index] = Some(value);
    purities[index] = is_pure;
    (value, is_pure)
}

#[allow(clippy::too_many_arguments)]
fn canonicalize_summary_value(
    value: KernelSummaryValueId,
    old_nodes: &[KernelSummaryNode],
    relocations: &mut [Option<KernelSummaryValueId>],
    purities: &mut [bool],
    active: &mut [bool],
    interner: &mut HashMap<u64, Vec<KernelSummaryValueId>>,
    canonical: &mut Vec<KernelSummaryNode>,
) -> (KernelSummaryValueId, bool) {
    canonicalize_summary_node(
        value.0 as usize,
        old_nodes,
        relocations,
        purities,
        active,
        interner,
        canonical,
    )
}

fn pure_summary_node_hash(node: &KernelSummaryNode) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    match node {
        KernelSummaryNode::Input(input) => {
            0_u8.hash(&mut hasher);
            input.hash(&mut hasher);
        }
        KernelSummaryNode::Term(term) => {
            1_u8.hash(&mut hasher);
            term.hash(&mut hasher);
        }
        KernelSummaryNode::Projection { provider, fields } => {
            2_u8.hash(&mut hasher);
            provider.hash(&mut hasher);
            fields.hash(&mut hasher);
        }
        KernelSummaryNode::Collection {
            kind,
            inputs,
            values,
        } => {
            3_u8.hash(&mut hasher);
            let kind = match kind {
                KernelCollectionOperationKind::List => 0,
                KernelCollectionOperationKind::Set => 1,
                KernelCollectionOperationKind::Map => 2,
            };
            kind.hash(&mut hasher);
            inputs.hash(&mut hasher);
            values.hash(&mut hasher);
        }
        KernelSummaryNode::Select { selector, arms } => {
            4_u8.hash(&mut hasher);
            selector.hash(&mut hasher);
            for arm in arms {
                arm.pattern.hash(&mut hasher);
                arm.output.hash(&mut hasher);
            }
        }
        KernelSummaryNode::Record { tag, entries } => {
            5_u8.hash(&mut hasher);
            tag.hash(&mut hasher);
            for entry in entries {
                match entry {
                    KernelSummaryRecordEntry::Field { name, value } => {
                        0_u8.hash(&mut hasher);
                        name.hash(&mut hasher);
                        value.hash(&mut hasher);
                    }
                    KernelSummaryRecordEntry::Spread { value } => {
                        1_u8.hash(&mut hasher);
                        value.hash(&mut hasher);
                    }
                }
            }
        }
        KernelSummaryNode::Constrain { .. }
        | KernelSummaryNode::Sequence { .. }
        | KernelSummaryNode::Invoke { .. } => {
            unreachable!("effect-owning summary nodes are never hash-consed")
        }
    }
    hasher.finish()
}

/// Retain only bytecode and occurrence inputs reachable from one summary
/// result. Normalization deliberately leaves node IDs stable while it runs;
/// this final compaction performs one dense relocation after every rewrite is
/// complete. The caller's pre-normalization sharing decision is kept
/// separately, so a large definition cannot become per-call inline code merely
/// because its normalized program is compact.
fn compact_summary_result(
    nodes: &mut Vec<KernelSummaryNode>,
    inputs: &mut Vec<DirectSummaryInput>,
    result: &mut PlannedSummaryValue,
) -> (u64, u64) {
    let old_node_count = nodes.len();
    let old_input_count = inputs.len();
    let mut reachable = vec![false; old_node_count];
    let mut used_inputs = vec![false; old_input_count];
    let mut pending = vec![result.value];
    while let Some(value) = pending.pop() {
        let index = value.0 as usize;
        let Some(seen) = reachable.get_mut(index) else {
            panic!(
                "kernel summary compaction references missing value {}",
                value.0
            )
        };
        if *seen {
            continue;
        }
        *seen = true;
        match &nodes[index] {
            KernelSummaryNode::Input(input) => {
                *used_inputs
                    .get_mut(*input as usize)
                    .expect("kernel summary input belongs to its compact program") = true;
            }
            KernelSummaryNode::Term(_) => {}
            KernelSummaryNode::Projection { provider, .. } => pending.push(*provider),
            KernelSummaryNode::Constrain { value, .. } => pending.push(*value),
            KernelSummaryNode::Sequence {
                inputs: dependencies,
                result,
            } => {
                pending.extend(dependencies.iter().copied());
                pending.push(*result);
            }
            KernelSummaryNode::Collection {
                inputs: items,
                values,
                ..
            } => pending.extend(items.iter().chain(values.iter()).copied()),
            KernelSummaryNode::Invoke {
                inputs: arguments, ..
            } => pending.extend(arguments.iter().copied()),
            KernelSummaryNode::Select { selector, arms } => {
                pending.push(*selector);
                pending.extend(arms.iter().map(|arm| arm.output));
            }
            KernelSummaryNode::Record { entries, .. } => {
                pending.extend(entries.iter().map(|entry| match entry {
                    KernelSummaryRecordEntry::Field { value, .. }
                    | KernelSummaryRecordEntry::Spread { value } => *value,
                }));
            }
        }
    }
    for input in [
        match result.mode {
            DirectSummaryMode::Input(input) => Some(input),
            DirectSummaryMode::Fixed { .. } => None,
        },
        result.formal_projection_input,
    ]
    .into_iter()
    .flatten()
    {
        *used_inputs
            .get_mut(input as usize)
            .expect("kernel summary result mode references a compact input") = true;
    }

    let mut value_relocations = vec![None; old_node_count];
    let mut next_value = 0_u32;
    for (index, reachable) in reachable.iter().copied().enumerate() {
        if reachable {
            value_relocations[index] = Some(KernelSummaryValueId(next_value));
            next_value = next_value
                .checked_add(1)
                .expect("kernel summary value count exceeds u32");
        }
    }
    let mut input_relocations = vec![None; old_input_count];
    let mut next_input = 0_u32;
    for (index, used) in used_inputs.iter().copied().enumerate() {
        if used {
            input_relocations[index] = Some(next_input);
            next_input = next_input
                .checked_add(1)
                .expect("kernel summary input count exceeds u32");
        }
    }

    let old_nodes = std::mem::take(nodes);
    nodes.extend(
        old_nodes
            .into_iter()
            .enumerate()
            .filter_map(|(index, mut node)| {
                reachable[index].then(|| {
                    relocate_summary_node(&mut node, &value_relocations, &input_relocations);
                    node
                })
            }),
    );
    let old_inputs = std::mem::take(inputs);
    inputs.extend(
        old_inputs
            .into_iter()
            .enumerate()
            .filter_map(|(index, input)| used_inputs[index].then_some(input)),
    );
    result.value = relocated_summary_value(result.value, &value_relocations);
    result.mode = match result.mode {
        DirectSummaryMode::Fixed { owner, expression } => {
            DirectSummaryMode::Fixed { owner, expression }
        }
        DirectSummaryMode::Input(input) => DirectSummaryMode::Input(
            input_relocations[input as usize]
                .expect("kernel summary result mode input remains reachable"),
        ),
    };
    result.formal_projection_input = result.formal_projection_input.map(|input| {
        input_relocations[input as usize]
            .expect("kernel summary result projection input remains reachable")
    });
    (
        u64::try_from(old_node_count - nodes.len()).expect("summary node delta exceeds u64"),
        u64::try_from(old_input_count - inputs.len()).expect("summary input delta exceeds u64"),
    )
}

fn relocate_summary_node(
    node: &mut KernelSummaryNode,
    values: &[Option<KernelSummaryValueId>],
    inputs: &[Option<u32>],
) {
    match node {
        KernelSummaryNode::Input(input) => {
            *input = inputs[*input as usize].expect("reachable summary input has a relocation");
        }
        KernelSummaryNode::Term(_) => {}
        KernelSummaryNode::Projection { provider, .. } => {
            *provider = relocated_summary_value(*provider, values);
        }
        KernelSummaryNode::Constrain { value, .. } => {
            *value = relocated_summary_value(*value, values);
        }
        KernelSummaryNode::Sequence {
            inputs: dependencies,
            result,
        } => {
            for dependency in dependencies {
                *dependency = relocated_summary_value(*dependency, values);
            }
            *result = relocated_summary_value(*result, values);
        }
        KernelSummaryNode::Collection {
            inputs: items,
            values: map_values,
            ..
        } => {
            for value in items.iter_mut().chain(map_values.iter_mut()) {
                *value = relocated_summary_value(*value, values);
            }
        }
        KernelSummaryNode::Invoke {
            inputs: arguments, ..
        } => {
            for argument in arguments {
                *argument = relocated_summary_value(*argument, values);
            }
        }
        KernelSummaryNode::Select { selector, arms } => {
            *selector = relocated_summary_value(*selector, values);
            for arm in arms {
                arm.output = relocated_summary_value(arm.output, values);
            }
        }
        KernelSummaryNode::Record { entries, .. } => {
            for entry in entries {
                let value = match entry {
                    KernelSummaryRecordEntry::Field { value, .. }
                    | KernelSummaryRecordEntry::Spread { value } => value,
                };
                *value = relocated_summary_value(*value, values);
            }
        }
    }
}

fn relocated_summary_value(
    value: KernelSummaryValueId,
    relocations: &[Option<KernelSummaryValueId>],
) -> KernelSummaryValueId {
    relocations[value.0 as usize].expect("reachable summary value has a relocation")
}

fn summary_constant(
    constants: &[Option<TypeTermId>],
    value: KernelSummaryValueId,
) -> Option<TypeTermId> {
    constants.get(value.0 as usize).copied().flatten()
}

fn constant_summary_projection(
    builder: &mut ComponentProgramBuilder,
    provider: KernelSummaryValueId,
    fields: &[crate::NameId],
    constants: &[Option<TypeTermId>],
) -> Option<TypeTermId> {
    let mut provider = summary_constant(constants, provider)?;
    for field in fields {
        provider = constant_summary_project_field(builder, provider, *field).unwrap_or_else(|| {
            let field = builder.terms().name(*field).to_owned();
            builder.terms_mut().unresolved_shape(format!(
                "authoritative summary value omits projection `{field}`"
            ))
        });
    }
    Some(provider)
}

fn constant_summary_project_field(
    builder: &mut ComponentProgramBuilder,
    provider: TypeTermId,
    field: crate::NameId,
) -> Option<TypeTermId> {
    match builder.terms().term(provider).clone() {
        TypeTerm::Object { fields, .. } => fields
            .iter()
            .find(|candidate| candidate.name == field)
            .map(|candidate| candidate.ty),
        TypeTerm::Union(members) => {
            let projected = members
                .iter()
                .filter_map(|member| constant_summary_project_field(builder, *member, field))
                .collect::<Vec<_>>();
            (!projected.is_empty()).then(|| builder.terms_mut().union(projected))
        }
        TypeTerm::VariantSet(variants) => {
            let projected = variants
                .iter()
                .filter_map(|variant| match variant {
                    VariantTerm::Tagged { fields, .. } => {
                        constant_summary_project_field(builder, *fields, field)
                    }
                    VariantTerm::Tag(_) => None,
                })
                .collect::<Vec<_>>();
            (!projected.is_empty()).then(|| builder.terms_mut().union(projected))
        }
        _ => None,
    }
}

fn constant_summary_collection(
    builder: &mut ComponentProgramBuilder,
    kind: KernelCollectionOperationKind,
    inputs: &[KernelSummaryValueId],
    values: &[KernelSummaryValueId],
    constants: &[Option<TypeTermId>],
) -> Option<TypeTermId> {
    let inputs = inputs
        .iter()
        .map(|value| summary_constant(constants, *value))
        .collect::<Option<Vec<_>>>()?;
    let values = values
        .iter()
        .map(|value| summary_constant(constants, *value))
        .collect::<Option<Vec<_>>>()?;
    let item = if inputs.is_empty() {
        match kind {
            KernelCollectionOperationKind::List => builder.terms().open_object(),
            KernelCollectionOperationKind::Set | KernelCollectionOperationKind::Map => {
                builder.terms().unknown()
            }
        }
    } else {
        constant_summary_structural_widen(builder, &inputs)
    };
    Some(match kind {
        KernelCollectionOperationKind::List => builder.terms_mut().list(item),
        KernelCollectionOperationKind::Set => builder.terms_mut().set(item),
        KernelCollectionOperationKind::Map => {
            let value = if values.is_empty() {
                builder.terms().unknown()
            } else {
                constant_summary_structural_widen(builder, &values)
            };
            builder.terms_mut().map(item, value)
        }
    })
}

fn constant_summary_structural_widen(
    builder: &mut ComponentProgramBuilder,
    values: &[TypeTermId],
) -> TypeTermId {
    values
        .iter()
        .copied()
        .fold(None, |current, value| {
            Some(match current {
                None => value,
                Some(current) => builder.terms_mut().structural_widen(current, value),
            })
        })
        .unwrap_or_else(|| builder.terms().absent())
}

fn constant_summary_select(
    builder: &mut ComponentProgramBuilder,
    selector: KernelSummaryValueId,
    arms: &[KernelSummarySelectArm],
    constants: &[Option<TypeTermId>],
) -> Option<TypeTermId> {
    let selector = summary_constant(constants, selector)?;
    let singleton = matches!(
        builder.terms().term(selector),
        TypeTerm::VariantSet(variants) if variants.len() == 1
    );
    let mut candidates = Vec::new();
    for arm in arms {
        if singleton && !constant_summary_pattern_accepts(builder, selector, &arm.pattern) {
            continue;
        }
        let candidate = summary_constant(constants, arm.output)?;
        if !matches!(builder.terms().term(candidate), TypeTerm::Absent) {
            candidates.push(candidate);
            if singleton {
                break;
            }
        }
    }
    Some(constant_summary_structural_widen(builder, &candidates))
}

fn constant_summary_pattern_accepts(
    builder: &ComponentProgramBuilder,
    selector: TypeTermId,
    pattern: &KernelPattern,
) -> bool {
    match pattern {
        KernelPattern::Wildcard | KernelPattern::Binding { .. } => true,
        KernelPattern::Number => matches!(builder.terms().term(selector), TypeTerm::Number),
        KernelPattern::Text => matches!(builder.terms().term(selector), TypeTerm::Text),
        KernelPattern::Bits { width } => {
            matches!(builder.terms().term(selector), TypeTerm::Bits(actual) if actual == width)
        }
        KernelPattern::Tag { name, .. } => {
            matches!(builder.terms().term(selector), TypeTerm::VariantSet(variants) if variants.iter().any(|variant| builder.terms().name(variant.tag()) == name.as_ref()))
        }
        KernelPattern::Invalid => false,
    }
}

fn constant_summary_record(
    builder: &mut ComponentProgramBuilder,
    tag: Option<crate::NameId>,
    entries: &[KernelSummaryRecordEntry],
    constants: &[Option<TypeTermId>],
) -> Option<TypeTermId> {
    let mut fields = Vec::<(crate::NameId, TypeTermId)>::new();
    for entry in entries {
        match entry {
            KernelSummaryRecordEntry::Field { name, value } => {
                let value = summary_constant(constants, *value)?;
                insert_constant_summary_field(&mut fields, *name, value);
            }
            KernelSummaryRecordEntry::Spread { value } => {
                let value = summary_constant(constants, *value)?;
                if !merge_constant_summary_spread(builder, value, &mut fields) {
                    return None;
                }
            }
        }
    }
    Some(intern_constant_summary_record(builder, tag, fields))
}

fn intern_constant_summary_record(
    builder: &mut ComponentProgramBuilder,
    tag: Option<crate::NameId>,
    fields: Vec<(crate::NameId, TypeTermId)>,
) -> TypeTermId {
    let object = builder.terms_mut().object(fields, false);
    if let Some(tag) = tag {
        let tag = builder.terms().name(tag).to_owned();
        let variant = builder.terms_mut().tagged_variant(tag, object);
        builder.terms_mut().variant_set([variant])
    } else {
        object
    }
}

fn merge_constant_summary_spread(
    builder: &mut ComponentProgramBuilder,
    spread: TypeTermId,
    fields: &mut Vec<(crate::NameId, TypeTermId)>,
) -> bool {
    match builder.terms().term(spread).clone() {
        TypeTerm::Object {
            fields: spread_fields,
            ..
        } => {
            for field in spread_fields {
                insert_constant_summary_field(fields, field.name, field.ty);
            }
            true
        }
        TypeTerm::Union(members) => members
            .iter()
            .all(|member| merge_constant_summary_spread(builder, *member, fields)),
        TypeTerm::VariantSet(variants)
            if variants
                .iter()
                .any(|variant| builder.terms().name(variant.tag()) == "UNPLUGGED") =>
        {
            true
        }
        TypeTerm::Unknown | TypeTerm::UnresolvedShape(_) => true,
        _ => false,
    }
}

fn insert_constant_summary_field(
    fields: &mut Vec<(crate::NameId, TypeTermId)>,
    name: crate::NameId,
    value: TypeTermId,
) {
    if let Some((_, current)) = fields.iter_mut().find(|(candidate, _)| *candidate == name) {
        *current = value;
    } else {
        fields.push((name, value));
    }
}

fn compile_direct_result_summaries(
    builder: &mut ComponentProgramBuilder,
    project: &KernelProjectProgramInput,
) -> Vec<Option<Arc<CompiledDirectSummary>>> {
    let targets = project
        .owners
        .iter()
        .flat_map(|owner| owner.nodes.iter())
        .filter_map(|node| match node.kind {
            KernelOwnerNodeKind::UserCall { target, .. } => Some(target),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let supported = targets
        .into_iter()
        .filter(|target| {
            let Some(owner) = project.owners.get(target.0 as usize) else {
                return false;
            };
            direct_result_summary_supported(
                project,
                *target,
                owner.result.0 as usize,
                &mut BTreeSet::new(),
            )
        })
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(supported.len());
    let mut states = vec![0_u8; project.owners.len()];
    for target in supported.iter().copied() {
        append_direct_summary_order(project, target, &supported, &mut states, &mut order);
    }
    let mut summaries = vec![None; project.owners.len()];
    for target in order {
        let Some(owner) = project.owners.get(target.0 as usize) else {
            continue;
        };
        let result = owner.result.0 as usize;
        let actuals = (0..owner.formal_count)
            .map(PlannedSummaryActual::Formal)
            .collect::<Vec<_>>();
        let mut compiler = DirectSummaryPlanCompiler {
            builder,
            project,
            summaries: &summaries,
            nodes: Vec::new(),
            inputs: Vec::new(),
            formal_projection_inputs: HashMap::new(),
        };
        let Some(mut result) =
            compiler.compile_expression(target, result, &actuals, &mut BTreeSet::new())
        else {
            continue;
        };
        let shared_bytecode = compiler.nodes.len() >= SHARED_SUMMARY_MIN_NODES;
        let (constant_folded_nodes, selector_fused_records) = compiler.fold_constant_nodes();
        let deduplicated_nodes = compiler.deduplicate_nodes(&mut result);
        let (pruned_nodes, pruned_inputs) = compiler.compact_result(&mut result);
        summaries[target.0 as usize] = Some(Arc::new(CompiledDirectSummary {
            program: Arc::new(KernelSummaryProgram {
                definition: target.0,
                nodes: compiler.nodes.into_boxed_slice(),
                result: result.value,
            }),
            inputs: compiler.inputs.into_boxed_slice(),
            result_mode: result.mode,
            formal_count: owner.formal_count as usize,
            constant_folded_nodes,
            selector_fused_records,
            deduplicated_nodes,
            pruned_nodes,
            pruned_inputs,
            shared_bytecode,
        }));
    }
    summaries
}

fn append_direct_summary_order(
    project: &KernelProjectProgramInput,
    target: KernelOwnerId,
    supported: &BTreeSet<KernelOwnerId>,
    states: &mut [u8],
    order: &mut Vec<KernelOwnerId>,
) {
    let index = target.0 as usize;
    match states.get(index).copied() {
        Some(2) => return,
        Some(1) | None => return,
        Some(_) => {}
    }
    states[index] = 1;
    if let Some(owner) = project.owners.get(index) {
        let mut dependencies = BTreeSet::new();
        collect_direct_summary_call_targets(
            owner,
            owner.result.0 as usize,
            &mut BTreeSet::new(),
            &mut dependencies,
        );
        for dependency in dependencies {
            if supported.contains(&dependency) {
                append_direct_summary_order(project, dependency, supported, states, order);
            }
        }
    }
    states[index] = 2;
    order.push(target);
}

fn collect_direct_summary_call_targets(
    owner: &KernelOwnerProgramInput,
    expression: usize,
    active: &mut BTreeSet<usize>,
    targets: &mut BTreeSet<KernelOwnerId>,
) {
    if !active.insert(expression) {
        return;
    }
    let Some(node) = owner.nodes.get(expression) else {
        active.remove(&expression);
        return;
    };
    if let KernelOwnerNodeKind::UserCall { target, .. } = node.kind {
        targets.insert(target);
    }
    for edge in node.inputs.iter() {
        let dependency = edge.expression.0 as usize;
        if dependency < owner.nodes.len() {
            collect_direct_summary_call_targets(owner, dependency, active, targets);
        }
    }
    active.remove(&expression);
}

fn emit_compiled_direct_summary(
    builder: &mut ComponentProgramBuilder,
    mode_builder: &mut ModeProgramBuilder,
    context: &OwnerCompileContext<'_>,
    source_node: usize,
    output: TypeVariableId,
    output_mode: ModeVariableId,
    actuals: &[CallActual],
    summary: &CompiledDirectSummary,
) -> Result<(), KernelOwnerBuildError> {
    if actuals.len() != summary.formal_count {
        return Err(KernelOwnerBuildError::new(format!(
            "compiled direct summary receives {} actuals for {} formals",
            actuals.len(),
            summary.formal_count
        )));
    }
    let mut call_inputs = Vec::with_capacity(summary.inputs.len());
    let mut input_modes = Vec::with_capacity(summary.inputs.len());
    for input in summary.inputs.iter() {
        match input {
            DirectSummaryInput::FormalProjection { formal, fields } => {
                let actual = actuals.get(*formal as usize).ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "compiled direct summary reads missing formal {formal}"
                    ))
                })?;
                let mut steps = Vec::with_capacity(fields.len().max(1));
                if fields.is_empty() {
                    steps.push(KernelSummaryProjectionStep {
                        field: None,
                        consumer: builder.new_variable(),
                    });
                } else {
                    for field in fields.iter().copied() {
                        steps.push(KernelSummaryProjectionStep {
                            field: Some(field),
                            consumer: builder.new_variable(),
                        });
                    }
                }
                if !builder.is_authoritative(actual.variable) || actual.requirement_backflow {
                    // Summary inputs are detached projections so a concrete
                    // call-site provider can never be specialized by the
                    // callee. An open caller formal is different: the
                    // callee's definition-local requirement must flow back to
                    // that formal's private requirement surface. Recreate the
                    // same root/path equation used by a full invocation frame
                    // without allocating that frame.
                    let consumer = steps
                        .last()
                        .expect("a summary projection always has one step")
                        .consumer;
                    let requirement_root = builder.new_contextual_hole();
                    let requirement = requirement_projection(builder, requirement_root, fields);
                    let consumer = builder.variable_term(consumer);
                    let requirement = builder.variable_term(requirement);
                    builder.add_unify(consumer, requirement);
                    let actual = builder.variable_term(actual.requirement);
                    let requirement_root = builder.variable_term(requirement_root);
                    builder.add_unify(actual, requirement_root);
                }
                call_inputs.push(KernelSummaryCallInput::Projection {
                    provider: actual.variable,
                    steps: steps.into_boxed_slice(),
                });
                input_modes.push(projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &actual.mode_source,
                    &fields
                        .iter()
                        .map(|field| builder.terms().name(*field).into())
                        .collect::<Vec<Box<str>>>(),
                    &mut BTreeSet::new(),
                )?);
            }
            DirectSummaryInput::External { owner, expression } => {
                let instance = context.principals.get(owner.0 as usize).ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "compiled direct summary imports missing owner {}",
                        owner.0
                    ))
                })?;
                let variable = instance.expressions.get(*expression).copied().ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "compiled direct summary imports missing owner {} expression {expression}",
                        owner.0
                    ))
                })?;
                call_inputs.push(KernelSummaryCallInput::Term(
                    builder.variable_term(variable),
                ));
                input_modes.push(instance.expression_modes[*expression]);
            }
        }
    }
    builder.add_summary_call(output, Arc::clone(&summary.program), call_inputs);
    let mode = match summary.result_mode {
        DirectSummaryMode::Fixed { owner, expression } => context
            .principals
            .get(owner.0 as usize)
            .and_then(|instance| instance.expression_modes.get(expression))
            .copied()
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "compiled direct summary mode references missing owner {} expression {expression}",
                    owner.0
                ))
            })?,
        DirectSummaryMode::Input(input) => input_modes
            .get(input as usize)
            .copied()
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "compiled direct summary mode references missing input {input}"
                ))
            })?,
    };
    mode_builder.set(output_mode, ModeEquation::Copy(mode));
    Ok(())
}

fn compile_node(
    builder: &mut ComponentProgramBuilder,
    mode_builder: &mut ModeProgramBuilder,
    invocations: &mut HashMap<InvocationKey, OwnerInstance>,
    specializations: &mut HashMap<SpecializationKey, OwnerSpecialization>,
    residual_modules: &mut HashMap<SpecializationKey, Arc<ResidualTypeModule>>,
    compile_work: &mut KernelCompileWork,
    context: &OwnerCompileContext<'_>,
    stack: &mut Vec<KernelOwnerId>,
    index: usize,
    node: &KernelOwnerNode,
    output: TypeVariableId,
    output_mode: ModeVariableId,
    compile_mode: bool,
) -> Result<(), KernelOwnerBuildError> {
    match &node.kind {
        KernelOwnerNodeKind::Known(ty) | KernelOwnerNodeKind::Source(ty) => {
            if !type_is_recursively_closed(ty) {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} imports a non-closed ABI type {ty:?}"
                )));
            }
            let provider = builder
                .terms_mut()
                .import_checked_type(ty, &mut |_| unreachable!("closed ABI type has no variable"));
            builder.add_publish(output, [provider], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Absent => {
            let absent = builder.terms().absent();
            builder.add_publish(output, [absent], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Text | KernelOwnerNodeKind::TextTemplate => {
            let text = builder.terms().text();
            builder.add_publish(output, [text], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Number => {
            let number = builder.terms().number();
            builder.add_publish(output, [number], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Byte => {
            let byte = builder.terms_mut().bytes(crate::BytesTerm::Fixed(1));
            builder.add_publish(output, [byte], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Bits(width) => {
            let bits = builder.terms_mut().bits(*width);
            builder.add_publish(output, [bits], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Tag(tag) => {
            let tag = builder.terms_mut().variant_tag(tag);
            let variants = builder.terms_mut().variant_set([tag]);
            builder.add_publish(output, [variants], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Record { tag } => {
            compile_record(builder, context, index, node, output, tag.as_deref())?;
        }
        KernelOwnerNodeKind::Block => {
            let mut results = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::BlockResult));
            let result = results.next().ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} BLOCK has no result edge"
                ))
            })?;
            if results.next().is_some() {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} BLOCK has multiple result edges"
                )));
            }
            let provider = edge_variable(context, index, result)?;
            builder.add_projection_into(provider, [], output);
        }
        KernelOwnerNodeKind::Collection { kind, .. } => match kind {
            KernelCollectionKind::List | KernelCollectionKind::Set => {
                let items = selected_edge_terms(builder, context, index, node, |role| {
                    matches!(role, KernelOwnerEdgeRole::CollectionItem)
                })?;
                let kind = match kind {
                    KernelCollectionKind::List => KernelCollectionOperationKind::List,
                    KernelCollectionKind::Set => KernelCollectionOperationKind::Set,
                    _ => unreachable!(),
                };
                builder.add_collection(output, kind, items, []);
            }
            KernelCollectionKind::Bytes => {
                let size = node
                    .inputs
                    .iter()
                    .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::CollectionItem))
                    .count();
                let bytes = builder.terms_mut().bytes(crate::BytesTerm::Fixed(size));
                builder.add_publish(output, [bytes], PublishMode::Replace);
            }
            KernelCollectionKind::Map => {
                let mut keys = Vec::new();
                let mut values = Vec::new();
                for edge in &node.inputs {
                    if !matches!(edge.role, KernelOwnerEdgeRole::MapEntry) {
                        continue;
                    }
                    let entry_index = checked_expression_index(
                        edge.expression,
                        context.input.nodes.len(),
                        "map entry",
                    )?;
                    let entry = &context.input.nodes[entry_index];
                    if !matches!(entry.kind, KernelOwnerNodeKind::MapEntry) {
                        return Err(KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} map edge targets non-entry node {entry_index}"
                        )));
                    }
                    keys.extend(selected_edge_terms(
                        builder,
                        context,
                        entry_index,
                        entry,
                        |role| matches!(role, KernelOwnerEdgeRole::MapKey),
                    )?);
                    values.extend(selected_edge_terms(
                        builder,
                        context,
                        entry_index,
                        entry,
                        |role| matches!(role, KernelOwnerEdgeRole::MapValue),
                    )?);
                }
                builder.add_collection(output, KernelCollectionOperationKind::Map, keys, values);
            }
        },
        KernelOwnerNodeKind::MapEntry => {
            // The enclosing MAP consumes its exact key/value edges. The entry
            // itself remains an internal delimiter rather than a value type.
            let absent = builder.terms().absent();
            builder.add_publish(output, [absent], PublishMode::Replace);
        }
        KernelOwnerNodeKind::FormalRead { formal, fields } => {
            if !node.inputs.is_empty() {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} formal read has explicit inputs"
                )));
            }
            let provider = context
                .formals
                .get(*formal as usize)
                .copied()
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} reads missing formal {formal}"
                    ))
                })?;
            let path = fields
                .iter()
                .map(|field| builder.terms_mut().intern_name(field))
                .collect::<Vec<_>>();
            builder.add_projection_into(provider, path.iter().copied(), output);
            let requirement = context
                .formal_requirements
                .get(*formal as usize)
                .copied()
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} reads missing formal requirement {formal}"
                    ))
                })?;
            let requirement = requirement_projection(builder, requirement, &path);
            let requirement = builder.variable_term(requirement);
            let output_term = builder.variable_term(output);
            builder.add_unify(output_term, requirement);
        }
        KernelOwnerNodeKind::ContextRead { formal, fields } => {
            if !node.inputs.is_empty() {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} context read has explicit inputs"
                )));
            }
            let provider = context
                .formals
                .get(*formal as usize)
                .copied()
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} reads missing context formal {formal}"
                    ))
                })?;
            let path = fields
                .iter()
                .map(|field| builder.terms_mut().intern_name(field))
                .collect::<Vec<_>>();
            builder.add_projection_into(provider, path.iter().copied(), output);
            // PASSED is provider-only across the call boundary, but its
            // definition still owns a private structural requirement. Keep
            // that requirement connected to the detached occurrence so
            // diagnostics can compare an explicit PASS value against the
            // callable contract without ever reshaping the captured provider.
            let requirement = context
                .formal_requirements
                .get(*formal as usize)
                .copied()
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} reads missing context formal requirement {formal}"
                    ))
                })?;
            let requirement = requirement_projection(builder, requirement, &path);
            let requirement = builder.variable_term(requirement);
            let output_term = builder.variable_term(output);
            builder.add_unify(output_term, requirement);
        }
        KernelOwnerNodeKind::LexicalRead { fields } => {
            let mut providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider));
            let provider = providers.next().ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} lexical read has no provider"
                ))
            })?;
            if providers.next().is_some() || node.inputs.len() != 1 {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} lexical read must have exactly one provider"
                )));
            }
            let provider = edge_variable(context, index, provider)?;
            let path = fields
                .iter()
                .map(|field| builder.terms_mut().intern_name(field))
                .collect::<Vec<_>>();
            if path.is_empty() {
                builder.add_alias(provider, output);
            } else {
                builder.add_projection_into(provider, path, output);
            }
        }
        KernelOwnerNodeKind::ValueRead { fields, .. }
        | KernelOwnerNodeKind::DerivedRead { fields } => {
            let mut providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider));
            let provider = providers.next().ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} value read has no provider"
                ))
            })?;
            if providers.next().is_some() || node.inputs.len() != 1 {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} value read must have exactly one provider"
                )));
            }
            let provider = edge_variable(context, index, provider)?;
            let path = fields
                .iter()
                .map(|field| builder.terms_mut().intern_name(field))
                .collect::<Vec<_>>();
            builder.add_projection_into(provider, path, output);
        }
        KernelOwnerNodeKind::PatternRead { pattern, fields } => {
            let mut providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider));
            let provider = providers.next().ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} pattern read has no provider"
                ))
            })?;
            if providers.next().is_some() || node.inputs.len() != 1 {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} pattern read must have exactly one provider"
                )));
            }
            let provider = edge_variable(context, index, provider)?;
            let fields = fields
                .iter()
                .map(|field| builder.terms_mut().intern_name(field))
                .collect::<Vec<_>>();
            builder.add_pattern_projection_into(provider, pattern.clone(), fields, output);
        }
        KernelOwnerNodeKind::CollectionItemRead => {
            let mut providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider));
            let provider = providers.next().ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} collection item read has no provider"
                ))
            })?;
            if providers.next().is_some() || node.inputs.len() != 1 {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} collection item read must have exactly one provider"
                )));
            }
            let provider = edge_variable(context, index, provider)?;
            builder.add_collection_item_projection(provider, output);
        }
        KernelOwnerNodeKind::FreshOut => {
            if !node.inputs.is_empty() {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} fresh OUT has explicit inputs"
                )));
            }
            // The matching call frame writes constraints into this private
            // variable. Publishing Unknown here would erase that authority.
        }
        KernelOwnerNodeKind::UserCall {
            target,
            inherited_formal,
        } => {
            compile_work.compiled_call_sites = compile_work.compiled_call_sites.saturating_add(1);
            let project = context.project.ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "standalone owner node {index} cannot call owner {}",
                    target.0
                ))
            })?;
            let target_owner = project.owners.get(target.0 as usize).ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} calls missing owner {}",
                    target.0
                ))
            })?;
            let mut actuals = vec![None; target_owner.formal_count as usize];
            for edge in &node.inputs {
                let (ordinal, variable) = match edge.role {
                    KernelOwnerEdgeRole::CallArgument { ordinal } => {
                        (ordinal, edge_variable(context, index, edge)?)
                    }
                    KernelOwnerEdgeRole::CallOutArgument { ordinal } => {
                        (ordinal, edge_output_variable(context, index, edge)?)
                    }
                    _ => {
                        return Err(KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} user call has non-argument edge {:?}",
                            edge.role
                        )));
                    }
                };
                let requirement_backflow =
                    matches!(edge.role, KernelOwnerEdgeRole::CallOutArgument { .. })
                        || edge_reads_output_capability(context, index, edge)?;
                // A callback/output occurrence has two independent surfaces:
                // collection data flows forward through `variable`, while the
                // called definition's shape requirements flow backward into a
                // private hole. Coalescing the channels would make a use such
                // as `row.kind` rewrite the checked callback occurrence from a
                // generic value into `{ kind: _ }` and could specialize the
                // original collection producer.
                let requirement = if requirement_backflow {
                    builder.new_contextual_hole()
                } else {
                    variable
                };
                if requirement_backflow {
                    bind_output_capability_requirement(builder, context, edge, requirement)?;
                }
                let slot = actuals.get_mut(ordinal as usize).ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} supplies out-of-range argument {ordinal}"
                    ))
                })?;
                if slot.is_some() {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} repeats argument {ordinal}"
                    )));
                }
                *slot = Some(CallActual {
                    variable,
                    requirement,
                    requirement_backflow,
                    mode: edge_mode_variable(context, index, edge)?,
                    mode_source: mode_source_for_edge(
                        context,
                        context.owner,
                        context.expression_modes,
                        context.formal_mode_sources,
                        edge,
                    )?,
                    static_variants: edge_static_variants(context, edge),
                });
            }
            if let Some(inherited) = inherited_formal {
                let actual = context
                    .formals
                    .get(inherited.caller_ordinal as usize)
                    .copied()
                    .ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} inherits missing caller formal {}",
                            inherited.caller_ordinal
                        ))
                    })?;
                let actual_requirement = context
                    .formal_requirements
                    .get(inherited.caller_ordinal as usize)
                    .copied()
                    .ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} inherits missing caller formal requirement {}",
                            inherited.caller_ordinal
                        ))
                    })?;
                let actual_mode = context
                    .formal_modes
                    .get(inherited.caller_ordinal as usize)
                    .copied()
                    .ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} inherits missing caller mode {}",
                            inherited.caller_ordinal
                        ))
                    })?;
                let actual_mode_source = context
                    .formal_mode_sources
                    .get(inherited.caller_ordinal as usize)
                    .cloned()
                    .ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} inherits missing caller mode source {}",
                            inherited.caller_ordinal
                        ))
                    })?;
                let slot = actuals
                    .get_mut(inherited.target_ordinal as usize)
                    .ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} inherits out-of-range target formal {}",
                            inherited.target_ordinal
                        ))
                    })?;
                if slot
                    .replace(CallActual {
                        variable: actual,
                        requirement: actual_requirement,
                        requirement_backflow: false,
                        mode: actual_mode,
                        mode_source: actual_mode_source,
                        static_variants: context
                            .formal_static_variants
                            .get(inherited.caller_ordinal as usize)
                            .cloned()
                            .flatten(),
                    })
                    .is_some()
                {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} supplies and inherits target formal {}",
                        inherited.target_ordinal
                    )));
                }
            }
            let actuals = actuals
                .into_iter()
                .enumerate()
                .map(|(ordinal, actual)| {
                    actual.ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} omits argument {ordinal}"
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let result = checked_expression_index(
                target_owner.result,
                target_owner.nodes.len(),
                "user-call result",
            )?;
            // A formal-independent definition is already represented by its
            // principal frame. Reusing that frame is exact: no actual can
            // alter the result, select a different branch, or receive a
            // requirement from the result cone. This is the first flag-day
            // step away from cloning a complete callee residual for every
            // call occurrence.
            if !context.formal_dependent_results[target.0 as usize] {
                compile_work.principal_result_reuses =
                    compile_work.principal_result_reuses.saturating_add(1);
                let provider = builder
                    .variable_term(context.principals[target.0 as usize].expressions[result]);
                builder.add_publish(output, [provider], PublishMode::Replace);
                mode_builder.set(
                    output_mode,
                    ModeEquation::Copy(
                        context.principals[target.0 as usize].expression_modes[result],
                    ),
                );
                return Ok(());
            }
            if let KernelOwnerNodeKind::FormalRead { formal, fields }
            | KernelOwnerNodeKind::ContextRead { formal, fields } =
                &target_owner.nodes[result].kind
            {
                if !target_owner.nodes[result].inputs.is_empty() {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner {} direct-result formal read has explicit inputs",
                        target.0
                    )));
                }
                let actual = actuals.get(*formal as usize).ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner {} direct-result summary reads missing formal {formal}",
                        target.0
                    ))
                })?;
                let path = fields
                    .iter()
                    .map(|field| builder.terms_mut().intern_name(field))
                    .collect::<Vec<_>>();
                builder.add_projection_into(actual.variable, path.iter().copied(), output);
                if matches!(
                    &target_owner.nodes[result].kind,
                    KernelOwnerNodeKind::FormalRead { .. }
                ) && (!builder.is_authoritative(actual.variable) || actual.requirement_backflow)
                {
                    let requirement_root = builder.new_contextual_hole();
                    let requirement = requirement_projection(builder, requirement_root, &path);
                    let output_term = builder.variable_term(output);
                    let requirement = builder.variable_term(requirement);
                    builder.add_unify(output_term, requirement);
                    let actual_term = builder.variable_term(actual.requirement);
                    let requirement_root = builder.variable_term(requirement_root);
                    builder.add_unify(actual_term, requirement_root);
                }
                let projected_mode = projected_mode_variable(
                    mode_builder,
                    context,
                    index,
                    &actual.mode_source,
                    fields,
                    &mut BTreeSet::new(),
                )?;
                mode_builder.set(output_mode, ModeEquation::Copy(projected_mode));
                compile_work.direct_result_summaries =
                    compile_work.direct_result_summaries.saturating_add(1);
                return Ok(());
            }
            if let Some(Some(summary)) = context.direct_summaries.get(target.0 as usize) {
                emit_compiled_direct_summary(
                    builder,
                    mode_builder,
                    context,
                    index,
                    output,
                    output_mode,
                    &actuals,
                    summary,
                )?;
                compile_work.direct_result_summaries =
                    compile_work.direct_result_summaries.saturating_add(1);
                return Ok(());
            }
            let instance = instantiate_owner(
                builder,
                mode_builder,
                invocations,
                specializations,
                residual_modules,
                compile_work,
                project,
                context.principals,
                context.formal_dependent_results,
                context.formal_dependent_expressions,
                context.direct_summaries,
                *target,
                &actuals,
                context.initial_state_surface
                    || context
                        .syntax_selected_calls
                        .and_then(|calls| calls.get(index))
                        .copied()
                        .unwrap_or(false),
                stack,
            )?;
            let provider = builder.variable_term(instance.expressions[result]);
            builder.add_publish(output, [provider], PublishMode::Replace);
            mode_builder.set(
                output_mode,
                ModeEquation::Copy(instance.expression_modes[result]),
            );
        }
        KernelOwnerNodeKind::RenderConstructor { kind } => {
            let mut fields = Vec::with_capacity(node.inputs.len() + 1);
            let mut direction = None;
            for edge in &node.inputs {
                let KernelOwnerEdgeRole::AbiArgument { name } = &edge.role else {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} render constructor has invalid edge {:?}",
                        edge.role
                    )));
                };
                let value = edge_variable(context, index, edge)?;
                if name.as_ref() == "direction" {
                    direction = Some(value);
                }
                let value_term = builder.variable_term(value);
                constrain_render_argument(builder, name, value_term);
                let name = builder.terms_mut().intern_name(name);
                fields.push((name, value_term));
            }
            let kind = match kind {
                KernelRenderConstructorKind::Fixed(tag) => {
                    let tag = builder.terms_mut().variant_tag(tag);
                    builder.terms_mut().variant_set([tag])
                }
                KernelRenderConstructorKind::StripeDirection => {
                    let kind = builder.new_authoritative_provider();
                    let row_name = builder.terms_mut().variant_tag("Row");
                    let row = builder.terms_mut().variant_set([row_name]);
                    let stack_name = builder.terms_mut().variant_tag("Stack");
                    let stack = builder.terms_mut().variant_set([stack_name]);
                    let fallback = builder.terms_mut().union([row, stack]);
                    match direction {
                        Some(direction) => {
                            builder.add_select(
                                kind,
                                direction,
                                [
                                    KernelSelectArm {
                                        pattern: KernelPattern::Tag {
                                            name: "Row".into(),
                                            fields: Box::new([]),
                                        },
                                        output: row,
                                    },
                                    KernelSelectArm {
                                        pattern: KernelPattern::Tag {
                                            name: "Column".into(),
                                            fields: Box::new([]),
                                        },
                                        output: stack,
                                    },
                                    KernelSelectArm {
                                        pattern: KernelPattern::Wildcard,
                                        output: fallback,
                                    },
                                ],
                            );
                        }
                        None => {
                            builder.add_publish(kind, [fallback], PublishMode::Replace);
                        }
                    }
                    builder.variable_term(kind)
                }
            };
            let kind_name = builder.terms_mut().intern_name("kind");
            fields.push((kind_name, kind));
            let result = builder.terms_mut().object(fields, false);
            builder.add_publish(output, [result], PublishMode::Replace);
        }
        KernelOwnerNodeKind::PureBuiltin { kind } => {
            let mut arguments = BTreeMap::new();
            let mut argument_edges = BTreeMap::new();
            for edge in &node.inputs {
                let KernelOwnerEdgeRole::AbiArgument { name } = &edge.role else {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} pure builtin has invalid edge {:?}",
                        edge.role
                    )));
                };
                let value = edge_variable(context, index, edge)?;
                if arguments.insert(name.as_ref(), value).is_some() {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} pure builtin repeats argument `{name}`"
                    )));
                }
                argument_edges.insert(name.as_ref(), edge);
                let value = builder.variable_term(value);
                constrain_pure_builtin_argument(builder, *kind, name, value);
            }
            let argument = |name: &str| {
                arguments.get(name).copied().ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner {} node {index} pure builtin omits argument `{name}`",
                        context.owner.0
                    ))
                })
            };
            let list_argument = || {
                arguments
                    .get("$pipe")
                    .or_else(|| arguments.get("list"))
                    .copied()
                    .ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner {} node {index} list builtin omits its `$pipe`/`list` input",
                            context.owner.0
                        ))
                    })
            };
            if matches!(
                kind,
                KernelPureBuiltinKind::ListPredicate
                    | KernelPureBuiltinKind::ListFilter
                    | KernelPureBuiltinKind::ListMap
                    | KernelPureBuiltinKind::ListFind
                    | KernelPureBuiltinKind::ListSort
            ) && let Some(item_edge) = argument_edges.get("item").copied()
            {
                // Contextual collection calls drive the OUT port regardless
                // of whether the caller created it fresh or forwarded an
                // enclosing OUT. This single equation replaces the old
                // special-case projection attached only to a bare identifier.
                let item = edge_output_variable(context, index, item_edge)?;
                builder.add_collection_item_projection(list_argument()?, item);
            }
            if matches!(kind, KernelPureBuiltinKind::ListAppend) {
                // Append preserves the existing list item authority and
                // widens it directionally with the new item. Publishing only
                // `List<item>` loses every variant contributed by the input
                // collection; equality-unifying the two would instead
                // backflow the widened result into both producers.
                let existing_item = builder.new_authoritative_provider();
                builder.add_collection_item_projection(list_argument()?, existing_item);
                let existing_item = builder.variable_term(existing_item);
                let appended_item = builder.variable_term(argument("item")?);
                builder.add_collection(
                    output,
                    KernelCollectionOperationKind::List,
                    [existing_item, appended_item],
                    [],
                );
                return Ok(());
            }
            let result = match kind {
                KernelPureBuiltinKind::TextConstant
                | KernelPureBuiltinKind::TextTransform
                | KernelPureBuiltinKind::TextSlice
                | KernelPureBuiltinKind::TextConcat
                | KernelPureBuiltinKind::NumberToText
                | KernelPureBuiltinKind::TextJoin
                | KernelPureBuiltinKind::FieldColor => builder.terms().text(),
                KernelPureBuiltinKind::NumberMath
                | KernelPureBuiltinKind::NumberRound
                | KernelPureBuiltinKind::NumberProjection
                | KernelPureBuiltinKind::TextLength
                | KernelPureBuiltinKind::ListLength => builder.terms().number(),
                KernelPureBuiltinKind::TextPredicate
                | KernelPureBuiltinKind::ListPredicate
                | KernelPureBuiltinKind::Boolean => boolean_type(builder),
                KernelPureBuiltinKind::TextToNumber => parsed_number_type(builder),
                KernelPureBuiltinKind::RecordConstructor => {
                    let fields = node
                        .inputs
                        .iter()
                        .map(|edge| {
                            let KernelOwnerEdgeRole::AbiArgument { name } = &edge.role else {
                                unreachable!("pure builtin argument roles were validated above")
                            };
                            let value = arguments[name.as_ref()];
                            let name = builder.terms_mut().intern_name(name);
                            (name, builder.variable_term(value))
                        })
                        .collect::<Vec<_>>();
                    builder.terms_mut().object(fields, false)
                }
                KernelPureBuiltinKind::ListFilter | KernelPureBuiltinKind::ListSort => {
                    builder.variable_term(list_argument()?)
                }
                KernelPureBuiltinKind::ListMap => {
                    let item = builder.variable_term(argument("new")?);
                    builder.terms_mut().list(item)
                }
                KernelPureBuiltinKind::ListFind => {
                    let item = builder.variable_term(argument("item")?);
                    let value = builder.terms_mut().intern_name("value");
                    let fields = builder.terms_mut().object([(value, item)], false);
                    let found = builder.terms_mut().tagged_variant("Found", fields);
                    let not_found = builder.terms_mut().variant_tag("NotFound");
                    builder
                        .terms_mut()
                        .variant_set_preserving_order([found, not_found])
                }
                KernelPureBuiltinKind::ListLatest => {
                    let item = builder.new_authoritative_provider();
                    builder.add_collection_item_projection(list_argument()?, item);
                    builder.variable_term(item)
                }
                KernelPureBuiltinKind::ListAppend => unreachable!("handled above"),
                KernelPureBuiltinKind::ListChunk => {
                    let item = builder.new_authoritative_provider();
                    builder.add_collection_item_projection(list_argument()?, item);
                    let item = builder.variable_term(item);
                    let label_name = builder.terms_mut().intern_name("label");
                    let items_name = builder.terms_mut().intern_name("items");
                    let label = builder.terms().text();
                    let items = builder.terms_mut().list(item);
                    let chunk = builder
                        .terms_mut()
                        .object([(label_name, label), (items_name, items)], false);
                    builder.terms_mut().list(chunk)
                }
            };
            builder.add_publish(output, [result], PublishMode::Replace);
        }
        KernelOwnerNodeKind::HostEffect { operation } => {
            compile_host_effect(builder, context, index, node, output, operation)?;
        }
        KernelOwnerNodeKind::Latest => {
            let branches = selected_edge_terms(builder, context, index, node, |role| {
                matches!(role, KernelOwnerEdgeRole::LatestBranch)
            })?;
            if branches.is_empty() {
                // An empty LATEST carries no value-shape evidence. `Absent`
                // remains a runtime value/type used by explicit absent
                // providers, while the empty selector is a contextual hole.
                // HOLD widening consequently ignores it without claiming that
                // the LATEST expression itself has the Absent value type.
                let unknown = builder.terms().unknown();
                builder.add_publish(output, [unknown], PublishMode::Replace);
            } else {
                builder.add_publish(output, branches, PublishMode::Union);
            }
        }
        KernelOwnerNodeKind::When => {
            let mut selector = None;
            let mut arms = Vec::new();
            let possible_arms = possible_when_arm_references(context, index, node)?;
            for edge in &node.inputs {
                match edge.role {
                    KernelOwnerEdgeRole::WhenInput => {
                        if selector
                            .replace(edge_variable(context, index, edge)?)
                            .is_some()
                        {
                            return Err(KernelOwnerBuildError::new(format!(
                                "kernel owner node {index} WHEN repeats its selector"
                            )));
                        }
                    }
                    KernelOwnerEdgeRole::WhenArm => {
                        if !possible_arms.contains(&(edge.expression.0 as usize)) {
                            continue;
                        }
                        let arm = referenced_node(context, index, edge)?;
                        let KernelOwnerNodeKind::MatchArm { pattern } = &arm.kind else {
                            return Err(KernelOwnerBuildError::new(format!(
                                "kernel owner node {index} WHEN targets a non-arm expression"
                            )));
                        };
                        let arm = edge_variable(context, index, edge)?;
                        arms.push(KernelSelectArm {
                            pattern: pattern.clone(),
                            output: builder.variable_term(arm),
                        });
                    }
                    _ => {
                        return Err(KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} WHEN has invalid edge {:?}",
                            edge.role
                        )));
                    }
                }
            }
            let selector = selector.ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} WHEN has no selector"
                ))
            })?;
            // A closed match domain constrains a generic selector to the
            // union of its authored patterns. A wildcard or binding arm is
            // deliberately open: it accepts values outside the listed tags,
            // so publishing the explicit arms as a closed VariantSet would
            // incorrectly reject values handled by the fallback branch.
            if !arms.iter().any(|arm| {
                matches!(
                    arm.pattern,
                    KernelPattern::Wildcard | KernelPattern::Binding { .. }
                )
            }) {
                let selector_requirement = builder.variable_term(selector);
                for arm in &arms {
                    if let Some(requirement) = pattern_requirement_term(builder, &arm.pattern) {
                        builder.add_unify(selector_requirement, requirement);
                    }
                }
            }
            builder.add_select(output, selector, arms);
        }
        KernelOwnerNodeKind::Then => {
            let has_output = node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::ThenOutput));
            publish_selected_edges(
                builder,
                context,
                index,
                node,
                output,
                |role| {
                    matches!(role, KernelOwnerEdgeRole::ThenOutput)
                        || (!has_output && matches!(role, KernelOwnerEdgeRole::ThenInput))
                },
                PublishMode::Union,
            )?;
        }
        KernelOwnerNodeKind::Infix { operation } => {
            let mut left = None;
            let mut right = None;
            for edge in &node.inputs {
                let slot = match edge.role {
                    KernelOwnerEdgeRole::InfixLeft => &mut left,
                    KernelOwnerEdgeRole::InfixRight => &mut right,
                    _ => {
                        return Err(KernelOwnerBuildError::new(format!(
                            "kernel owner node {index} infix `{operation}` has invalid edge {:?}",
                            edge.role
                        )));
                    }
                };
                if slot.replace(edge_variable(context, index, edge)?).is_some() {
                    return Err(KernelOwnerBuildError::new(format!(
                        "kernel owner node {index} infix `{operation}` repeats an operand"
                    )));
                }
            }
            let (Some(left), Some(right)) = (left, right) else {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} infix `{operation}` requires two operands"
                )));
            };
            if infix_requires_number_operands(operation) {
                let number = builder.terms().number();
                let left = builder.variable_term(left);
                builder.add_unify(left, number);
                let right = builder.variable_term(right);
                builder.add_unify(right, number);
            }
            let result = if infix_returns_bool(operation) {
                let false_tag = builder.terms_mut().variant_tag("False");
                let true_tag = builder.terms_mut().variant_tag("True");
                builder.terms_mut().variant_set([false_tag, true_tag])
            } else {
                builder.terms().number()
            };
            builder.add_publish(output, [result], PublishMode::Replace);
        }
        KernelOwnerNodeKind::Draining => {
            publish_selected_edges(
                builder,
                context,
                index,
                node,
                output,
                |role| matches!(role, KernelOwnerEdgeRole::DrainingInput),
                PublishMode::Replace,
            )?;
        }
        KernelOwnerNodeKind::Hold => {
            if context.initial_state_surface {
                publish_selected_edges(
                    builder,
                    context,
                    index,
                    node,
                    output,
                    |role| matches!(role, KernelOwnerEdgeRole::HoldInitial),
                    PublishMode::Replace,
                )?;
            } else {
                // A normal definition or direct call denotes the complete
                // state domain. Only a proven syntax-selected construction
                // occurrence uses the initializer-only surface above.
                publish_selected_edges(
                    builder,
                    context,
                    index,
                    node,
                    output,
                    |role| {
                        matches!(
                            role,
                            KernelOwnerEdgeRole::HoldInitial | KernelOwnerEdgeRole::HoldUpdate
                        )
                    },
                    PublishMode::StructuralWiden,
                )?;
            }
        }
        KernelOwnerNodeKind::MatchArm { .. } => {
            if node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::MatchOutput))
            {
                publish_selected_edges(
                    builder,
                    context,
                    index,
                    node,
                    output,
                    |role| matches!(role, KernelOwnerEdgeRole::MatchOutput),
                    PublishMode::Replace,
                )?;
            } else if node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::RecordField { .. }))
            {
                compile_record(builder, context, index, node, output, None)?;
            } else {
                let absent = builder.terms().absent();
                builder.add_publish(output, [absent], PublishMode::Replace);
            }
        }
        KernelOwnerNodeKind::Arrow => {
            publish_selected_edges(
                builder,
                context,
                index,
                node,
                output,
                |role| matches!(role, KernelOwnerEdgeRole::ArrowOutput),
                PublishMode::Replace,
            )?;
        }
        KernelOwnerNodeKind::Delimiter | KernelOwnerNodeKind::Unknown => {
            let unknown = builder.terms().unknown();
            builder.add_publish(output, [unknown], PublishMode::Replace);
        }
    }
    if compile_mode && !matches!(node.kind, KernelOwnerNodeKind::UserCall { .. }) {
        let equation = node_mode_equation(mode_builder, context, index, node)?;
        mode_builder.set(output_mode, equation);
    }
    Ok(())
}

fn compile_record(
    builder: &mut ComponentProgramBuilder,
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    node: &KernelOwnerNode,
    output: TypeVariableId,
    tag: Option<&str>,
) -> Result<(), KernelOwnerBuildError> {
    let mut entries = Vec::new();
    for edge in &node.inputs {
        let KernelOwnerEdgeRole::RecordField { name, spread } = &edge.role else {
            continue;
        };
        let value = edge_variable(context, node_index, edge)?;
        let value = builder.variable_term(value);
        if *spread {
            entries.push(KernelRecordEntry::Spread { value });
        } else {
            let name = builder.terms_mut().intern_name(name);
            entries.push(KernelRecordEntry::Field { name, value });
        }
    }
    let tag = tag.map(|tag| builder.terms_mut().intern_name(tag));
    builder.add_record(output, tag, entries);
    Ok(())
}

fn publish_selected_edges(
    builder: &mut ComponentProgramBuilder,
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    node: &KernelOwnerNode,
    output: TypeVariableId,
    selected: impl Fn(&KernelOwnerEdgeRole) -> bool,
    mode: PublishMode,
) -> Result<(), KernelOwnerBuildError> {
    let terms = selected_edge_terms(builder, context, node_index, node, selected)?;
    if terms.is_empty() && mode == PublishMode::Replace {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel owner node {node_index} has no selected provider"
        )));
    }
    builder.add_publish(output, terms, mode);
    Ok(())
}

/// Build the invocation-private requirement surface for one formal read.
///
/// Provider projections remain directional and detached. Requirements instead
/// form ordinary equations, so a use such as `width + 1` constrains the fresh
/// call frame without specializing the callable's principal frame or another
/// invocation.
fn requirement_projection(
    builder: &mut ComponentProgramBuilder,
    root: TypeVariableId,
    fields: &[crate::NameId],
) -> TypeVariableId {
    let mut provider = root;
    for field in fields {
        let consumer = builder.new_variable();
        let consumer_term = builder.variable_term(consumer);
        let scaffold = builder.terms_mut().object([(*field, consumer_term)], true);
        let provider_term = builder.variable_term(provider);
        builder.add_unify(provider_term, scaffold);
        provider = consumer;
    }
    provider
}

fn pattern_requirement_term(
    builder: &mut ComponentProgramBuilder,
    pattern: &KernelPattern,
) -> Option<TypeTermId> {
    Some(match pattern {
        KernelPattern::Wildcard | KernelPattern::Binding { .. } | KernelPattern::Invalid => {
            return None;
        }
        KernelPattern::Number => builder.terms().number(),
        KernelPattern::Text => builder.terms().text(),
        KernelPattern::Bits { width } => builder.terms_mut().bits(*width),
        KernelPattern::Tag { name, fields } => {
            if fields.is_empty() {
                let tag = builder.terms_mut().variant_tag(name);
                builder.terms_mut().variant_set([tag])
            } else {
                let fields = fields
                    .iter()
                    .map(|field| {
                        let name = builder.terms_mut().intern_name(field);
                        let value = builder.new_contextual_hole();
                        (name, builder.variable_term(value))
                    })
                    .collect::<Vec<_>>();
                let fields = builder.terms_mut().object(fields, true);
                let tag = builder.terms_mut().tagged_variant(name, fields);
                builder.terms_mut().variant_set([tag])
            }
        }
    })
}

fn selected_edge_terms(
    builder: &mut ComponentProgramBuilder,
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    node: &KernelOwnerNode,
    selected: impl Fn(&KernelOwnerEdgeRole) -> bool,
) -> Result<Vec<crate::TypeTermId>, KernelOwnerBuildError> {
    node.inputs
        .iter()
        .filter(|edge| selected(&edge.role))
        .map(|edge| {
            let variable = edge_variable(context, node_index, edge)?;
            Ok(builder.variable_term(variable))
        })
        .collect()
}

fn node_mode_equation(
    mode_builder: &mut ModeProgramBuilder,
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    node: &KernelOwnerNode,
) -> Result<ModeEquation, KernelOwnerBuildError> {
    let copy = |selected: fn(&KernelOwnerEdgeRole) -> bool| {
        let mut inputs = node.inputs.iter().filter(|edge| selected(&edge.role));
        let input = inputs.next().ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "kernel owner node {node_index} has no provider for its flow mode"
            ))
        })?;
        if inputs.next().is_some() {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel owner node {node_index} has multiple providers for one flow mode"
            )));
        }
        edge_mode_variable(context, node_index, input).map(ModeEquation::Copy)
    };
    match &node.kind {
        KernelOwnerNodeKind::FormalRead { formal, fields }
        | KernelOwnerNodeKind::ContextRead { formal, fields } => {
            let source = context
                .formal_mode_sources
                .get(*formal as usize)
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner node {node_index} reads missing formal mode source {formal}"
                    ))
                })?;
            projected_mode_variable(
                mode_builder,
                context,
                node_index,
                source,
                fields,
                &mut BTreeSet::new(),
            )
            .map(ModeEquation::Copy)
        }
        KernelOwnerNodeKind::ValueRead {
            fields,
            mode_narrowing,
        } => {
            if let Some(selector) = mode_narrowing {
                let selector = context
                    .expression_modes
                    .get(selector.0 as usize)
                    .copied()
                    .ok_or_else(|| {
                        KernelOwnerBuildError::new(format!(
                            "kernel owner node {node_index} has an out-of-range mode-narrowing selector {}",
                            selector.0
                        ))
                    })?;
                return Ok(ModeEquation::Copy(selector));
            }
            // A whole-value read consumes the provider declaration's public
            // mode. A nested read instead projects the authored field's mode
            // through the finalized public result expression. The latter is
            // required for event fields inside an otherwise continuous
            // record: publishing the record as an owner result must not erase
            // the field's PresentOrAbsent surface.
            let mut providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider));
            let provider = providers.next().ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {node_index} value read has no mode provider"
                ))
            })?;
            if providers.next().is_some() {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {node_index} value read has multiple mode providers"
                )));
            }
            if fields.is_empty() {
                edge_mode_variable(context, node_index, provider).map(ModeEquation::Copy)
            } else {
                projected_edge_mode_variable(mode_builder, context, node_index, provider, fields)
                    .map(ModeEquation::Copy)
            }
        }
        KernelOwnerNodeKind::PatternRead { .. } => {
            let providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider))
                .count();
            if providers != 1 {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {node_index} pattern read has {providers} mode providers instead of one"
                )));
            }
            // A pattern binding is a stable value inside the selected arm.
            // The selector's event mode activates the arm/WHEN expression;
            // copying that mode onto the bound payload would incorrectly turn
            // ordinary uses of `value => value` into event occurrences.
            Ok(ModeEquation::Fixed(node.mode))
        }
        KernelOwnerNodeKind::LexicalRead { fields }
        | KernelOwnerNodeKind::DerivedRead { fields } => {
            let mut providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider));
            let provider = providers.next().ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {node_index} value read has no mode provider"
                ))
            })?;
            if providers.next().is_some() {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {node_index} value read has multiple mode providers"
                )));
            }
            if fields.is_empty() {
                return edge_mode_variable(context, node_index, provider).map(ModeEquation::Copy);
            }
            projected_edge_mode_variable(mode_builder, context, node_index, provider, fields)
                .map(ModeEquation::Copy)
        }
        KernelOwnerNodeKind::CollectionItemRead => {
            copy(|role| matches!(role, KernelOwnerEdgeRole::ReadProvider))
        }
        KernelOwnerNodeKind::FreshOut => {
            let mut active = BTreeSet::new();
            match output_binding_collection_item_mode_variable(
                mode_builder,
                context,
                node_index,
                &[],
                &mut active,
            )? {
                Some(provider) => Ok(ModeEquation::Copy(provider)),
                None => Ok(ModeEquation::Fixed(node.mode)),
            }
        }
        KernelOwnerNodeKind::Block => copy(|role| matches!(role, KernelOwnerEdgeRole::BlockResult)),
        KernelOwnerNodeKind::Latest => node
            .inputs
            .iter()
            .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::LatestBranch))
            .map(|edge| edge_mode_variable(context, node_index, edge))
            .collect::<Result<Vec<_>, _>>()
            .map(|inputs| ModeEquation::Latest(inputs.into_boxed_slice())),
        KernelOwnerNodeKind::When => copy(|role| matches!(role, KernelOwnerEdgeRole::WhenInput)),
        KernelOwnerNodeKind::Draining => {
            copy(|role| matches!(role, KernelOwnerEdgeRole::DrainingInput))
        }
        KernelOwnerNodeKind::MatchArm { .. } => {
            if node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::MatchOutput))
            {
                copy(|role| matches!(role, KernelOwnerEdgeRole::MatchOutput))
            } else if node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::RecordField { .. }))
            {
                Ok(ModeEquation::Fixed(node.mode))
            } else {
                Ok(ModeEquation::Fixed(FlowMode::Absent))
            }
        }
        KernelOwnerNodeKind::Arrow => {
            if node
                .inputs
                .iter()
                .any(|edge| matches!(edge.role, KernelOwnerEdgeRole::ArrowOutput))
            {
                copy(|role| matches!(role, KernelOwnerEdgeRole::ArrowOutput))
            } else {
                Ok(ModeEquation::Fixed(FlowMode::Absent))
            }
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListMap,
        } => copy(
            |role| matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if name.as_ref() == "new"),
        ),
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListLatest,
        } => copy(
            |role| matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if matches!(name.as_ref(), "$pipe" | "list")),
        ),
        KernelOwnerNodeKind::UserCall { .. } => Err(KernelOwnerBuildError::new(format!(
            "kernel owner node {node_index} user-call mode must come from its invocation"
        ))),
        KernelOwnerNodeKind::Known(_)
        | KernelOwnerNodeKind::Source(_)
        | KernelOwnerNodeKind::Absent
        | KernelOwnerNodeKind::Text
        | KernelOwnerNodeKind::TextTemplate
        | KernelOwnerNodeKind::Number
        | KernelOwnerNodeKind::Byte
        | KernelOwnerNodeKind::Bits(_)
        | KernelOwnerNodeKind::Tag(_)
        | KernelOwnerNodeKind::Record { .. }
        | KernelOwnerNodeKind::Collection { .. }
        | KernelOwnerNodeKind::MapEntry
        | KernelOwnerNodeKind::RenderConstructor { .. }
        | KernelOwnerNodeKind::PureBuiltin { .. }
        | KernelOwnerNodeKind::HostEffect { .. }
        | KernelOwnerNodeKind::Then
        | KernelOwnerNodeKind::Infix { .. }
        | KernelOwnerNodeKind::Hold
        | KernelOwnerNodeKind::Delimiter
        | KernelOwnerNodeKind::Unknown => Ok(ModeEquation::Fixed(node.mode)),
    }
}

fn possible_when_arm_references(
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    node: &KernelOwnerNode,
) -> Result<BTreeSet<usize>, KernelOwnerBuildError> {
    let arm_edges = node
        .inputs
        .iter()
        .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenArm))
        .collect::<Vec<_>>();
    let selector = node
        .inputs
        .iter()
        .find(|edge| matches!(edge.role, KernelOwnerEdgeRole::WhenInput))
        .and_then(|edge| edge_static_variants(context, edge));
    let Some(selector) = selector else {
        return Ok(arm_edges
            .into_iter()
            .map(|edge| edge.expression.0 as usize)
            .collect());
    };
    let mut selected = BTreeSet::new();
    for tag in selector {
        for edge in &arm_edges {
            let arm = referenced_node(context, node_index, edge)?;
            let KernelOwnerNodeKind::MatchArm { pattern } = &arm.kind else {
                return Err(KernelOwnerBuildError::new(format!(
                    "kernel owner node {node_index} WHEN targets a non-arm expression"
                )));
            };
            if static_pattern_accepts_tag(pattern, &tag) {
                selected.insert(edge.expression.0 as usize);
                break;
            }
        }
    }
    Ok(selected)
}

fn infix_returns_bool(operation: &str) -> bool {
    matches!(operation, "==" | "!=" | ">" | "<" | ">=" | "<=")
}

fn boolean_type(builder: &mut ComponentProgramBuilder) -> crate::TypeTermId {
    let false_tag = builder.terms_mut().variant_tag("False");
    let true_tag = builder.terms_mut().variant_tag("True");
    builder.terms_mut().variant_set([false_tag, true_tag])
}

fn parsed_number_type(builder: &mut ComponentProgramBuilder) -> crate::TypeTermId {
    let value = builder.terms_mut().intern_name("value");
    let number = builder.terms().number();
    let parsed_fields = builder.terms_mut().object([(value, number)], false);
    let parsed = builder.terms_mut().tagged_variant("Parsed", parsed_fields);

    let reason = builder.terms_mut().intern_name("reason");
    let position = builder.terms_mut().intern_name("position");
    let text = builder.terms().text();
    let number = builder.terms().number();
    let invalid_fields = builder
        .terms_mut()
        .object([(reason, text), (position, number)], false);
    let invalid = builder
        .terms_mut()
        .tagged_variant("InvalidNumber", invalid_fields);
    builder
        .terms_mut()
        .variant_set_preserving_order([parsed, invalid])
}

fn rounding_rule_type(builder: &mut ComponentProgramBuilder) -> crate::TypeTermId {
    let variants = ExactRoundingRule::ALL
        .into_iter()
        .map(|rule| builder.terms_mut().variant_tag(rule.as_tag()))
        .collect::<Vec<_>>();
    builder.terms_mut().variant_set(variants)
}

fn compile_host_effect(
    builder: &mut ComponentProgramBuilder,
    context: &OwnerCompileContext<'_>,
    index: usize,
    node: &KernelOwnerNode,
    output: TypeVariableId,
    operation: &str,
) -> Result<(), KernelOwnerBuildError> {
    let spec = host_effect_spec(operation).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {index} names unknown host effect `{operation}`"
        ))
    })?;
    if spec.result_policy != ResultPolicySpec::ReturnValue {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel owner node {index} host effect `{operation}` has no return value"
        )));
    }
    let schema = spec.schema.ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {index} host effect `{operation}` has no type schema"
        ))
    })?;
    let ValueType::Record {
        fields,
        open: false,
    } = &schema.intent
    else {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel owner node {index} host effect `{operation}` requires a closed record intent"
        )));
    };

    let pipe_field = fields.first().map(|field| field.name);
    let mut arguments = BTreeMap::<&str, TypeVariableId>::new();
    for edge in &node.inputs {
        let KernelOwnerEdgeRole::AbiArgument { name } = &edge.role else {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel owner node {index} host effect has invalid edge {:?}",
                edge.role
            )));
        };
        let name = if name.as_ref() == "$pipe" {
            pipe_field.ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {index} cannot pipe into argument-free host effect `{operation}`"
                ))
            })?
        } else {
            name
        };
        if !fields.iter().any(|field| field.name == name) {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel owner node {index} host effect `{operation}` has no argument `{name}`"
            )));
        }
        let value = edge_variable(context, index, edge)?;
        if arguments.insert(name, value).is_some() {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel owner node {index} host effect `{operation}` repeats argument `{name}`"
            )));
        }
    }
    for field in fields {
        let Some(actual) = arguments.get(field.name).copied() else {
            if schema
                .intent_defaults
                .iter()
                .any(|default| default.field_name == field.name)
            {
                continue;
            }
            return Err(KernelOwnerBuildError::new(format!(
                "kernel owner node {index} host effect `{operation}` omits required argument `{}`",
                field.name
            )));
        };
        let expected = effect_schema_type_to_checked(&field.value_type);
        let expected = builder
            .terms_mut()
            .import_checked_type(&expected, &mut |_| {
                unreachable!("host ABI types are closed")
            });
        let actual = builder.variable_term(actual);
        builder.add_unify(actual, expected);
    }

    let result = effect_schema_type_to_checked(&schema.result);
    let result = builder
        .terms_mut()
        .import_checked_type(&result, &mut |_| unreachable!("host ABI types are closed"));
    builder.add_publish(output, [result], PublishMode::Replace);
    Ok(())
}

fn effect_schema_type_to_checked(value_type: &ValueType) -> Type {
    match value_type {
        ValueType::Number => Type::Number,
        ValueType::Text => Type::Text,
        ValueType::Bytes { fixed_len } => {
            Type::Bytes(fixed_len.map_or(BytesType::Dynamic, |length| {
                BytesType::Fixed(
                    usize::try_from(length).expect("host ABI fixed byte length fits usize"),
                )
            }))
        }
        ValueType::List { item } => Type::List(Type::shared(effect_schema_type_to_checked(item))),
        ValueType::Record { fields, open } => Type::object(ObjectShape::from_ordered_fields(
            fields.iter().map(|field| {
                (
                    field.name.to_owned(),
                    effect_schema_type_to_checked(&field.value_type),
                )
            }),
            *open,
        )),
        ValueType::Variant { variants } => Type::VariantSet(
            variants
                .iter()
                .map(|variant| {
                    if variant.fields.is_empty() {
                        Variant::Tag(variant.tag.to_owned())
                    } else {
                        Variant::Tagged {
                            tag: variant.tag.to_owned(),
                            fields: ObjectShape::from_ordered_fields(
                                variant.fields.iter().map(|field| {
                                    (
                                        field.name.to_owned(),
                                        effect_schema_type_to_checked(&field.value_type),
                                    )
                                }),
                                false,
                            ),
                        }
                    }
                })
                .collect(),
        ),
    }
}

fn constrain_pure_builtin_argument(
    builder: &mut ComponentProgramBuilder,
    kind: KernelPureBuiltinKind,
    name: &str,
    value: TypeTermId,
) {
    if let Some(expected) = pure_builtin_argument_requirement(builder, kind, name) {
        builder.add_unify(value, expected);
    }
}

fn pure_builtin_argument_requirement(
    builder: &mut ComponentProgramBuilder,
    kind: KernelPureBuiltinKind,
    name: &str,
) -> Option<TypeTermId> {
    match kind {
        KernelPureBuiltinKind::TextConstant => None,
        KernelPureBuiltinKind::TextTransform
        | KernelPureBuiltinKind::TextLength
        | KernelPureBuiltinKind::TextPredicate => Some(builder.terms().text()),
        KernelPureBuiltinKind::TextToNumber if matches!(name, "$pipe" | "input" | "text") => {
            Some(builder.terms().text())
        }
        KernelPureBuiltinKind::TextToNumber if name == "radix" => Some(builder.terms().number()),
        KernelPureBuiltinKind::TextToNumber => None,
        KernelPureBuiltinKind::TextSlice if matches!(name, "$pipe" | "input") => {
            Some(builder.terms().text())
        }
        KernelPureBuiltinKind::TextSlice => Some(builder.terms().number()),
        // Text/concat accepts the runtime's text-formattable scalar family.
        // That is a validation contract, not an equality constraint.
        KernelPureBuiltinKind::TextConcat | KernelPureBuiltinKind::FieldColor => None,
        KernelPureBuiltinKind::RecordConstructor
            if matches!(
                name,
                "azimuth" | "altitude" | "spread" | "intensity" | "radius" | "softness"
            ) =>
        {
            Some(builder.terms().number())
        }
        KernelPureBuiltinKind::RecordConstructor => None,
        KernelPureBuiltinKind::TextJoin if name == "$pipe" => {
            let item = builder.terms().text();
            Some(builder.terms_mut().list(item))
        }
        KernelPureBuiltinKind::TextJoin => Some(builder.terms().text()),
        KernelPureBuiltinKind::NumberToText if name == "prefix" => Some(boolean_type(builder)),
        KernelPureBuiltinKind::NumberToText | KernelPureBuiltinKind::NumberMath => {
            Some(builder.terms().number())
        }
        KernelPureBuiltinKind::NumberRound if name == "using" => Some(rounding_rule_type(builder)),
        KernelPureBuiltinKind::NumberRound => Some(builder.terms().number()),
        KernelPureBuiltinKind::NumberProjection if name == "zoom" => None,
        KernelPureBuiltinKind::NumberProjection => Some(builder.terms().number()),
        KernelPureBuiltinKind::Boolean if matches!(name, "$pipe" | "value" | "left" | "right") => {
            Some(boolean_type(builder))
        }
        KernelPureBuiltinKind::Boolean => None,
        KernelPureBuiltinKind::ListLength
        | KernelPureBuiltinKind::ListPredicate
        | KernelPureBuiltinKind::ListFilter
        | KernelPureBuiltinKind::ListMap
        | KernelPureBuiltinKind::ListFind
        | KernelPureBuiltinKind::ListLatest
        | KernelPureBuiltinKind::ListAppend
        | KernelPureBuiltinKind::ListSort
        | KernelPureBuiltinKind::ListChunk
            if matches!(name, "$pipe" | "list") =>
        {
            let item = builder.new_variable();
            let item = builder.variable_term(item);
            Some(builder.terms_mut().list(item))
        }
        KernelPureBuiltinKind::ListFilter | KernelPureBuiltinKind::ListFind if name == "if" => {
            Some(boolean_type(builder))
        }
        KernelPureBuiltinKind::ListChunk if name == "size" => Some(builder.terms().number()),
        KernelPureBuiltinKind::ListLength
        | KernelPureBuiltinKind::ListPredicate
        | KernelPureBuiltinKind::ListFilter
        | KernelPureBuiltinKind::ListMap
        | KernelPureBuiltinKind::ListFind
        | KernelPureBuiltinKind::ListLatest
        | KernelPureBuiltinKind::ListAppend
        | KernelPureBuiltinKind::ListSort
        | KernelPureBuiltinKind::ListChunk => None,
    }
}

fn constrain_render_argument(builder: &mut ComponentProgramBuilder, name: &str, value: TypeTermId) {
    if let Some(expected) = render_argument_requirement(builder, name) {
        builder.add_unify(value, expected);
    }
}

fn render_argument_requirement(
    builder: &mut ComponentProgramBuilder,
    name: &str,
) -> Option<TypeTermId> {
    match name {
        "text"
        | "label"
        | "target"
        | "input_id"
        | "source"
        | "artifact_id"
        | "bootstrap_source"
        | "bootstrap_artifact_id" => Some(builder.terms().text()),
        "gap" | "revision" => Some(builder.terms().number()),
        "visible" | "selected" | "checked" | "focus" => {
            let false_tag = builder.terms_mut().variant_tag("False");
            let true_tag = builder.terms_mut().variant_tag("True");
            Some(builder.terms_mut().variant_set([false_tag, true_tag]))
        }
        "root" | "child" => Some(builder.terms().render_contract()),
        "items" | "contents" => {
            let item = builder.terms().render_contract();
            Some(builder.terms_mut().list(item))
        }
        "element" | "style" | "placeholder" | "lights" | "geometry" | "activate_focus" => {
            Some(builder.terms().open_object())
        }
        // Stripe direction is a Row/Column tag even though the legacy ABI
        // models the validation slot as an open object.
        "direction" => None,
        _ => None,
    }
}

fn infix_requires_number_operands(operation: &str) -> bool {
    matches!(
        operation,
        "+" | "-" | "*" | "/" | "%" | ">" | "<" | ">=" | "<="
    )
}

type ActiveModeProjection = BTreeSet<(KernelOwnerId, usize, usize, bool, bool)>;

fn owner_mode_input<'a>(
    context: &'a OwnerCompileContext<'a>,
    owner: KernelOwnerId,
) -> Result<&'a KernelOwnerProgramInput, KernelOwnerBuildError> {
    if owner == context.owner {
        return Ok(context.input);
    }
    context
        .project
        .and_then(|project| project.owners.get(owner.0 as usize))
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "owner {} mode projection references missing owner {}",
                context.owner.0, owner.0
            ))
        })
}

fn mode_source_for_edge(
    context: &OwnerCompileContext<'_>,
    owner: KernelOwnerId,
    expression_modes: &Arc<[ModeVariableId]>,
    formal_sources: &Arc<[ModeSource]>,
    edge: &KernelOwnerInputEdge,
) -> Result<ModeSource, KernelOwnerBuildError> {
    let input = owner_mode_input(context, owner)?;
    let reference = edge.expression.0 as usize;
    if reference < input.nodes.len() {
        return Ok(ModeSource::Expression {
            owner,
            expression: reference,
            expression_modes: Arc::clone(expression_modes),
            formal_sources: Arc::clone(formal_sources),
        });
    }
    let external_index = reference - input.nodes.len();
    let external = input.external_expressions.get(external_index).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "owner {} mode projection references external expression {external_index} outside 0..{}",
            owner.0,
            input.external_expressions.len()
        ))
    })?;
    let project = context.project.ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "owner {} mode projection references an external owner outside a project",
            owner.0
        ))
    })?;
    let target = project
        .owners
        .get(external.owner.0 as usize)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "owner {} mode projection references missing owner {}",
                owner.0, external.owner.0
            ))
        })?;
    let target_instance = context
        .principals
        .get(external.owner.0 as usize)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "owner {} mode projection references missing principal owner {}",
                owner.0, external.owner.0
            ))
        })?;
    let expression = match external.target {
        KernelExternalTarget::Expression(expression) => {
            checked_expression_index(expression, target.nodes.len(), "external mode projection")?
        }
        KernelExternalTarget::Result => checked_expression_index(
            target.result,
            target.nodes.len(),
            "external result mode projection",
        )?,
    };
    Ok(ModeSource::Expression {
        owner: external.owner,
        expression,
        expression_modes: Arc::clone(&target_instance.expression_modes),
        formal_sources: Arc::clone(&target_instance.formal_mode_sources),
    })
}

fn selected_mode_edge<'a>(
    node: &'a KernelOwnerNode,
    selected: impl Fn(&KernelOwnerEdgeRole) -> bool,
) -> Option<&'a KernelOwnerInputEdge> {
    node.inputs.iter().find(|edge| selected(&edge.role))
}

fn merge_projected_modes(
    mode_builder: &mut ModeProgramBuilder,
    mut inputs: Vec<ModeVariableId>,
    fallback: ModeVariableId,
) -> ModeVariableId {
    inputs.sort_unstable();
    inputs.dedup();
    match inputs.as_slice() {
        [] => fallback,
        [input] => *input,
        _ => {
            let output = mode_builder.new_variable(FlowMode::Continuous);
            mode_builder.set(output, ModeEquation::Latest(inputs.into_boxed_slice()));
            output
        }
    }
}

fn merge_contextual_call_modes(
    mode_builder: &mut ModeProgramBuilder,
    mut inputs: Vec<ModeVariableId>,
    result: FlowMode,
    fallback: ModeVariableId,
) -> ModeVariableId {
    inputs.sort_unstable();
    inputs.dedup();
    if inputs.is_empty() {
        return fallback;
    }
    let output = mode_builder.new_variable(result);
    mode_builder.set(
        output,
        ModeEquation::Call {
            result,
            inputs: inputs.into_boxed_slice(),
        },
    );
    output
}

fn eventful_projected_mode(
    mode_builder: &mut ModeProgramBuilder,
    input: ModeVariableId,
) -> ModeVariableId {
    let output = mode_builder.new_variable(FlowMode::Continuous);
    mode_builder.set(output, ModeEquation::Eventful(input));
    output
}

fn user_call_result_mode_source(
    context: &OwnerCompileContext<'_>,
    source_node: usize,
    owner: KernelOwnerId,
    expression_modes: &Arc<[ModeVariableId]>,
    formal_sources: &Arc<[ModeSource]>,
    node: &KernelOwnerNode,
    target: KernelOwnerId,
    inherited_formal: Option<KernelInheritedFormal>,
) -> Result<ModeSource, KernelOwnerBuildError> {
    let project = context.project.ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {source_node} projects through a user call outside a project"
        ))
    })?;
    let target_input = project.owners.get(target.0 as usize).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {source_node} projects through missing owner {}",
            target.0
        ))
    })?;
    let target_instance = context.principals.get(target.0 as usize).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {source_node} projects through missing principal owner {}",
            target.0
        ))
    })?;
    let mut actuals = vec![None; target_input.formal_count as usize];
    for edge in &node.inputs {
        let KernelOwnerEdgeRole::CallArgument { ordinal } = edge.role else {
            continue;
        };
        let actual = mode_source_for_edge(context, owner, expression_modes, formal_sources, edge)?;
        let slot = actuals.get_mut(ordinal as usize).ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "kernel owner node {source_node} projects out-of-range argument {ordinal}"
            ))
        })?;
        if slot.replace(actual).is_some() {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel owner node {source_node} projects repeated argument {ordinal}"
            )));
        }
    }
    if let Some(inherited) = inherited_formal {
        let actual = formal_sources
            .get(inherited.caller_ordinal as usize)
            .cloned()
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {source_node} projects missing inherited formal {}",
                    inherited.caller_ordinal
                ))
            })?;
        let slot = actuals
            .get_mut(inherited.target_ordinal as usize)
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {source_node} projects out-of-range inherited formal {}",
                    inherited.target_ordinal
                ))
            })?;
        if slot.replace(actual).is_some() {
            return Err(KernelOwnerBuildError::new(format!(
                "kernel owner node {source_node} projects repeated inherited formal {}",
                inherited.target_ordinal
            )));
        }
    }
    let actuals = actuals
        .into_iter()
        .enumerate()
        .map(|(ordinal, actual)| {
            actual.ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {source_node} projects omitted argument {ordinal}"
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let result = checked_expression_index(
        target_input.result,
        target_input.nodes.len(),
        "projected user-call result",
    )?;
    Ok(ModeSource::Expression {
        owner: target,
        expression: result,
        expression_modes: Arc::clone(&target_instance.expression_modes),
        formal_sources: actuals.into(),
    })
}

fn projected_edge_mode_variable(
    mode_builder: &mut ModeProgramBuilder,
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    edge: &KernelOwnerInputEdge,
    fields: &[Box<str>],
) -> Result<ModeVariableId, KernelOwnerBuildError> {
    let source = mode_source_for_edge(
        context,
        context.owner,
        context.expression_modes,
        context.formal_mode_sources,
        edge,
    )?;
    projected_mode_variable(
        mode_builder,
        context,
        node_index,
        &source,
        fields,
        &mut BTreeSet::new(),
    )
}

fn projected_mode_variable(
    mode_builder: &mut ModeProgramBuilder,
    context: &OwnerCompileContext<'_>,
    source_node: usize,
    source: &ModeSource,
    fields: &[Box<str>],
    active: &mut ActiveModeProjection,
) -> Result<ModeVariableId, KernelOwnerBuildError> {
    let ModeSource::Expression {
        owner,
        expression,
        expression_modes,
        formal_sources,
    } = source
    else {
        return Ok(source.root_mode());
    };
    let input = owner_mode_input(context, *owner)?;
    let node = input.nodes.get(*expression).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {source_node} projects missing owner {} expression {expression}",
            owner.0
        ))
    })?;
    let mode = source.root_mode();
    let unproven_branch = active.iter().any(|key| key.4)
        || matches!(
            node.kind,
            KernelOwnerNodeKind::Latest | KernelOwnerNodeKind::When
        );
    let active_key = (*owner, *expression, fields.len(), false, unproven_branch);
    if !active.insert(active_key) {
        return Ok(mode);
    }
    let follow = |edge: &KernelOwnerInputEdge| {
        mode_source_for_edge(context, *owner, expression_modes, formal_sources, edge)
    };
    let result = match &node.kind {
        KernelOwnerNodeKind::FormalRead {
            formal,
            fields: provider_fields,
        }
        | KernelOwnerNodeKind::ContextRead {
            formal,
            fields: provider_fields,
        } => {
            let mut combined = provider_fields.to_vec();
            combined.extend_from_slice(fields);
            let provider = formal_sources.get(*formal as usize).ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "kernel owner node {source_node} projects missing formal {formal}"
                ))
            })?;
            projected_mode_variable(
                mode_builder,
                context,
                source_node,
                provider,
                &combined,
                active,
            )?
        }
        KernelOwnerNodeKind::FreshOut => output_binding_collection_item_mode_variable(
            mode_builder,
            context,
            *expression,
            fields,
            active,
        )?
        .unwrap_or(mode),
        KernelOwnerNodeKind::LexicalRead {
            fields: provider_fields,
        }
        | KernelOwnerNodeKind::ValueRead {
            fields: provider_fields,
            ..
        }
        | KernelOwnerNodeKind::DerivedRead {
            fields: provider_fields,
        }
        | KernelOwnerNodeKind::PatternRead {
            fields: provider_fields,
            ..
        } => {
            let provider = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::ReadProvider)
            });
            if let Some(provider) = provider {
                let mut combined = provider_fields.to_vec();
                combined.extend_from_slice(fields);
                let provider = follow(provider)?;
                if combined.is_empty() {
                    provider.root_mode()
                } else {
                    projected_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        &combined,
                        active,
                    )?
                }
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::UserCall {
            target,
            inherited_formal,
        } => {
            let result = user_call_result_mode_source(
                context,
                source_node,
                *owner,
                expression_modes,
                formal_sources,
                node,
                *target,
                *inherited_formal,
            )?;
            projected_mode_variable(mode_builder, context, source_node, &result, fields, active)?
        }
        KernelOwnerNodeKind::Block => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::BlockResult)
            }) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::Draining => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::DrainingInput)
            }) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::MatchArm { .. } => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::MatchOutput)
            }) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::Arrow => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::ArrowOutput)
            }) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::CollectionItemRead => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::ReadProvider)
            }) {
                let provider = follow(edge)?;
                if fields.is_empty() {
                    projected_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )?
                } else {
                    projected_collection_item_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )?
                }
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListMap,
        } if fields.is_empty() => {
            if let Some(edge) = selected_mode_edge(
                node,
                |role| matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if name.as_ref() == "$pipe"),
            ) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                let inputs = node
                    .inputs
                    .iter()
                    .map(|edge| {
                        let provider = follow(edge)?;
                        projected_mode_variable(
                            mode_builder,
                            context,
                            source_node,
                            &provider,
                            fields,
                            active,
                        )
                    })
                    .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?;
                merge_contextual_call_modes(mode_builder, inputs, node.mode, mode)
            }
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListMap,
        } => {
            if let Some(edge) = selected_mode_edge(
                node,
                |role| matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if name.as_ref() == "new"),
            ) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListLatest,
        } if fields.is_empty() => {
            let inputs = node
                .inputs
                .iter()
                .map(|edge| {
                    let provider = follow(edge)?;
                    projected_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )
                })
                .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?;
            merge_contextual_call_modes(mode_builder, inputs, node.mode, mode)
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListLatest,
        } => {
            if let Some(edge) = selected_mode_edge(
                node,
                |role| matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if matches!(name.as_ref(), "$pipe" | "list")),
            ) {
                let provider = follow(edge)?;
                if fields.is_empty() {
                    projected_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )?
                } else {
                    projected_collection_item_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )?
                }
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::When if fields.is_empty() => {
            if let Some(edge) =
                selected_mode_edge(node, |role| matches!(role, KernelOwnerEdgeRole::WhenInput))
            {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::Latest | KernelOwnerNodeKind::When => {
            let selected = |role: &KernelOwnerEdgeRole| match &node.kind {
                KernelOwnerNodeKind::Latest => {
                    matches!(role, KernelOwnerEdgeRole::LatestBranch)
                }
                KernelOwnerNodeKind::When => matches!(role, KernelOwnerEdgeRole::WhenArm),
                _ => false,
            };
            let projected = node
                .inputs
                .iter()
                .filter(|edge| selected(&edge.role))
                .map(|edge| {
                    let provider = follow(edge)?;
                    projected_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )
                })
                .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?;
            merge_projected_modes(mode_builder, projected, mode)
        }
        KernelOwnerNodeKind::PureBuiltin { .. }
        | KernelOwnerNodeKind::RenderConstructor { .. }
        | KernelOwnerNodeKind::HostEffect { .. }
            if fields.is_empty() =>
        {
            let inputs = node
                .inputs
                .iter()
                .map(|edge| {
                    let provider = follow(edge)?;
                    projected_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )
                })
                .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?;
            merge_contextual_call_modes(mode_builder, inputs, node.mode, mode)
        }
        KernelOwnerNodeKind::Record { .. } if !fields.is_empty() => {
            let field = &fields[0];
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(
                    role,
                    KernelOwnerEdgeRole::RecordField { name, spread: false }
                        if name.as_ref() == field.as_ref()
                )
            }) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    &fields[1..],
                    active,
                )?
            } else {
                let projected_spreads = node
                    .inputs
                    .iter()
                    .filter(|edge| {
                        matches!(
                            edge.role,
                            KernelOwnerEdgeRole::RecordField { spread: true, .. }
                        )
                    })
                    .map(|edge| {
                        let provider = follow(edge)?;
                        projected_mode_variable(
                            mode_builder,
                            context,
                            source_node,
                            &provider,
                            fields,
                            active,
                        )
                    })
                    .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?;
                if projected_spreads.is_empty() && unproven_branch {
                    // A closed branch which does not define this field is not
                    // a continuous provider for the projection. Keep it
                    // unresolved here so a real eventful provider in another
                    // WHEN/LATEST branch determines the merged occurrence.
                    eventful_projected_mode(mode_builder, mode)
                } else if projected_spreads.is_empty() {
                    mode
                } else {
                    merge_projected_modes(mode_builder, projected_spreads, mode)
                }
            }
        }
        _ if !fields.is_empty() => eventful_projected_mode(mode_builder, mode),
        _ => mode,
    };
    active.remove(&active_key);
    Ok(result)
}

fn projected_collection_item_mode_variable(
    mode_builder: &mut ModeProgramBuilder,
    context: &OwnerCompileContext<'_>,
    source_node: usize,
    source: &ModeSource,
    fields: &[Box<str>],
    active: &mut ActiveModeProjection,
) -> Result<ModeVariableId, KernelOwnerBuildError> {
    let ModeSource::Expression {
        owner,
        expression,
        expression_modes,
        formal_sources,
    } = source
    else {
        return Ok(source.root_mode());
    };
    let input = owner_mode_input(context, *owner)?;
    let node = input.nodes.get(*expression).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {source_node} projects an item from missing owner {} expression {expression}",
            owner.0
        ))
    })?;
    let mode = source.root_mode();
    let active_key = (*owner, *expression, fields.len(), true, true);
    if !active.insert(active_key) {
        return Ok(mode);
    }
    let follow = |edge: &KernelOwnerInputEdge| {
        mode_source_for_edge(context, *owner, expression_modes, formal_sources, edge)
    };
    let result = match &node.kind {
        KernelOwnerNodeKind::Collection {
            kind: KernelCollectionKind::List | KernelCollectionKind::Set,
            ..
        } => {
            let projected = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::CollectionItem))
                .map(|edge| {
                    let provider = follow(edge)?;
                    projected_mode_variable(
                        mode_builder,
                        context,
                        source_node,
                        &provider,
                        fields,
                        active,
                    )
                })
                .collect::<Result<Vec<_>, KernelOwnerBuildError>>()?;
            merge_projected_modes(mode_builder, projected, mode)
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListMap,
        } => {
            if let Some(edge) = selected_mode_edge(
                node,
                |role| matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if name.as_ref() == "new"),
            ) {
                let provider = follow(edge)?;
                projected_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListFilter | KernelPureBuiltinKind::ListSort,
        } => {
            if let Some(edge) = selected_mode_edge(
                node,
                |role| matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if matches!(name.as_ref(), "$pipe" | "list")),
            ) {
                let provider = follow(edge)?;
                projected_collection_item_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::LexicalRead {
            fields: provider_fields,
        }
        | KernelOwnerNodeKind::ValueRead {
            fields: provider_fields,
            ..
        }
        | KernelOwnerNodeKind::DerivedRead {
            fields: provider_fields,
        }
        | KernelOwnerNodeKind::PatternRead {
            fields: provider_fields,
            ..
        } if provider_fields.is_empty() => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::ReadProvider)
            }) {
                let provider = follow(edge)?;
                projected_collection_item_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::Block => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::BlockResult)
            }) {
                let provider = follow(edge)?;
                projected_collection_item_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::Draining => {
            if let Some(edge) = selected_mode_edge(node, |role| {
                matches!(role, KernelOwnerEdgeRole::DrainingInput)
            }) {
                let provider = follow(edge)?;
                projected_collection_item_mode_variable(
                    mode_builder,
                    context,
                    source_node,
                    &provider,
                    fields,
                    active,
                )?
            } else {
                mode
            }
        }
        KernelOwnerNodeKind::UserCall {
            target,
            inherited_formal,
        } => {
            let provider = user_call_result_mode_source(
                context,
                source_node,
                *owner,
                expression_modes,
                formal_sources,
                node,
                *target,
                *inherited_formal,
            )?;
            projected_collection_item_mode_variable(
                mode_builder,
                context,
                source_node,
                &provider,
                fields,
                active,
            )?
        }
        _ => mode,
    };
    active.remove(&active_key);
    Ok(result)
}

fn output_binding_collection_item_mode_variable(
    mode_builder: &mut ModeProgramBuilder,
    context: &OwnerCompileContext<'_>,
    source_node: usize,
    fields: &[Box<str>],
    active: &mut ActiveModeProjection,
) -> Result<Option<ModeVariableId>, KernelOwnerBuildError> {
    let Some((consumer_index, list)) = output_binding_collection_driver(context, source_node)?
    else {
        return Ok(None);
    };
    let provider = mode_source_for_edge(
        context,
        context.owner,
        context.expression_modes,
        context.formal_mode_sources,
        list,
    )?;
    projected_collection_item_mode_variable(
        mode_builder,
        context,
        consumer_index,
        &provider,
        fields,
        active,
    )
    .map(Some)
}

fn output_binding_collection_driver<'a>(
    context: &'a OwnerCompileContext<'_>,
    source_node: usize,
) -> Result<Option<(usize, &'a KernelOwnerInputEdge)>, KernelOwnerBuildError> {
    let mut consumers = context
        .input
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            matches!(
                node.kind,
                KernelOwnerNodeKind::PureBuiltin {
                    kind: KernelPureBuiltinKind::ListPredicate
                        | KernelPureBuiltinKind::ListFilter
                        | KernelPureBuiltinKind::ListMap
                        | KernelPureBuiltinKind::ListFind
                        | KernelPureBuiltinKind::ListSort
                }
            ) && node.inputs.iter().any(|edge| {
                matches!(&edge.role, KernelOwnerEdgeRole::AbiArgument { name } if name.as_ref() == "item")
                    && edge.expression.0 as usize == source_node
            })
        });
    let Some((consumer_index, consumer)) = consumers.next() else {
        return Ok(None);
    };
    if consumers.next().is_some() {
        return Err(KernelOwnerBuildError::new(format!(
            "kernel owner node {source_node} is driven by multiple contextual collection calls"
        )));
    }
    let list = selected_mode_edge(consumer, |role| {
        matches!(role, KernelOwnerEdgeRole::AbiArgument { name } if matches!(name.as_ref(), "$pipe" | "list"))
    })
    .ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "kernel owner node {consumer_index} drives OUT {source_node} without a list input"
        ))
    })?;
    Ok(Some((consumer_index, list)))
}

/// Route a callback consumer requirement back through its collection input.
///
/// The requirement channel stops at owner/public providers. A definition-local
/// formal is different: its principal shape is inferred from every use, so a
/// callback requirement on `item.field` must constrain the corresponding
/// `list[].field` formal surface. Filter/sort preserve that item identity and
/// are traversed transparently. The callback occurrence itself remains a
/// detached directional read and is never unified with this surface.
fn bind_output_capability_requirement(
    builder: &mut ComponentProgramBuilder,
    context: &OwnerCompileContext<'_>,
    edge: &KernelOwnerInputEdge,
    requirement: TypeVariableId,
) -> Result<(), KernelOwnerBuildError> {
    let Some((output, fields)) =
        output_capability_read_path(context, edge.expression.0 as usize, &mut BTreeSet::new())?
    else {
        return Ok(());
    };
    let Some((_, list)) = output_binding_collection_driver(context, output)? else {
        return Ok(());
    };
    let item_requirement = if fields.is_empty() {
        requirement
    } else {
        let root = builder.new_contextual_hole();
        let fields = fields
            .iter()
            .map(|field| builder.terms_mut().intern_name(field))
            .collect::<Vec<_>>();
        let projected = requirement_projection(builder, root, &fields);
        let projected = builder.variable_term(projected);
        let requirement = builder.variable_term(requirement);
        builder.add_unify(projected, requirement);
        root
    };
    let Some(list_requirement) = expression_requirement_variable(
        builder,
        context,
        list.expression.0 as usize,
        &mut BTreeSet::new(),
    )?
    else {
        return Ok(());
    };
    let item_requirement = builder.variable_term(item_requirement);
    let list_shape = builder.terms_mut().list(item_requirement);
    let list_requirement = builder.variable_term(list_requirement);
    builder.add_unify(list_requirement, list_shape);
    Ok(())
}

fn output_capability_read_path(
    context: &OwnerCompileContext<'_>,
    expression: usize,
    active: &mut BTreeSet<usize>,
) -> Result<Option<(usize, Vec<Box<str>>)>, KernelOwnerBuildError> {
    if expression >= context.input.nodes.len() || !active.insert(expression) {
        return Ok(None);
    }
    let node = &context.input.nodes[expression];
    let result = match &node.kind {
        KernelOwnerNodeKind::FreshOut => Some((expression, Vec::new())),
        KernelOwnerNodeKind::LexicalRead { fields }
        | KernelOwnerNodeKind::ValueRead { fields, .. }
        | KernelOwnerNodeKind::DerivedRead { fields } => {
            let providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider))
                .collect::<Vec<_>>();
            let [provider] = providers.as_slice() else {
                active.remove(&expression);
                return Ok(None);
            };
            output_capability_read_path(context, provider.expression.0 as usize, active)?.map(
                |(output, mut path)| {
                    path.extend(fields.iter().cloned());
                    (output, path)
                },
            )
        }
        _ => None,
    };
    active.remove(&expression);
    Ok(result)
}

fn expression_requirement_variable(
    builder: &mut ComponentProgramBuilder,
    context: &OwnerCompileContext<'_>,
    expression: usize,
    active: &mut BTreeSet<usize>,
) -> Result<Option<TypeVariableId>, KernelOwnerBuildError> {
    if expression >= context.input.nodes.len() || !active.insert(expression) {
        return Ok(None);
    }
    let node = &context.input.nodes[expression];
    let result = match &node.kind {
        KernelOwnerNodeKind::FormalRead { formal, fields } => {
            let root = context
                .formal_requirements
                .get(*formal as usize)
                .copied()
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "kernel owner node {expression} reads missing formal requirement {formal}"
                    ))
                })?;
            let fields = fields
                .iter()
                .map(|field| builder.terms_mut().intern_name(field))
                .collect::<Vec<_>>();
            Some(requirement_projection(builder, root, &fields))
        }
        KernelOwnerNodeKind::LexicalRead { fields }
        | KernelOwnerNodeKind::DerivedRead { fields } => {
            let providers = node
                .inputs
                .iter()
                .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider))
                .collect::<Vec<_>>();
            let [provider] = providers.as_slice() else {
                active.remove(&expression);
                return Ok(None);
            };
            let Some(root) = expression_requirement_variable(
                builder,
                context,
                provider.expression.0 as usize,
                active,
            )?
            else {
                active.remove(&expression);
                return Ok(None);
            };
            let fields = fields
                .iter()
                .map(|field| builder.terms_mut().intern_name(field))
                .collect::<Vec<_>>();
            Some(requirement_projection(builder, root, &fields))
        }
        KernelOwnerNodeKind::Block => {
            let provider = node
                .inputs
                .iter()
                .find(|edge| matches!(edge.role, KernelOwnerEdgeRole::BlockResult));
            match provider {
                Some(provider) => expression_requirement_variable(
                    builder,
                    context,
                    provider.expression.0 as usize,
                    active,
                )?,
                None => None,
            }
        }
        KernelOwnerNodeKind::PureBuiltin {
            kind: KernelPureBuiltinKind::ListFilter | KernelPureBuiltinKind::ListSort,
        } => {
            let provider = node.inputs.iter().find(|edge| {
                matches!(&edge.role, KernelOwnerEdgeRole::AbiArgument { name } if matches!(name.as_ref(), "$pipe" | "list"))
            });
            match provider {
                Some(provider) => expression_requirement_variable(
                    builder,
                    context,
                    provider.expression.0 as usize,
                    active,
                )?,
                None => None,
            }
        }
        // Cross-owner/public values are provider-only boundaries. Requirements
        // must not specialize their producer or leak into another owner.
        _ => None,
    };
    active.remove(&expression);
    Ok(result)
}

fn edge_variable(
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    edge: &KernelOwnerInputEdge,
) -> Result<TypeVariableId, KernelOwnerBuildError> {
    let reference = edge.expression.0 as usize;
    if reference < context.input.nodes.len() {
        return Ok(context.expressions[reference]);
    }
    let external_index = reference - context.input.nodes.len();
    if let Some(external_variables) = context.external_variables {
        return external_variables
            .get(external_index)
            .copied()
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "input of owner {} node {node_index} references external frame variable {external_index} outside 0..{}",
                    context.owner.0,
                    external_variables.len()
                ))
            });
    }
    let external = context
        .input
        .external_expressions
        .get(external_index)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "input of owner {} node {node_index} references external expression {external_index} outside 0..{}",
                context.owner.0,
                context.input.external_expressions.len()
            ))
        })?;
    let target_owner = context
        .principals
        .get(external.owner.0 as usize)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "input of owner {} node {node_index} references missing owner {}",
                context.owner.0, external.owner.0
            ))
        })?;
    match external.target {
        KernelExternalTarget::Expression(expression) => target_owner
            .expressions
            .get(expression.0 as usize)
            .copied()
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "input of owner {} node {node_index} references missing owner {} expression {}",
                    context.owner.0, external.owner.0, expression.0
                ))
            }),
        KernelExternalTarget::Result => {
            let project = context.project.ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "input of owner {} node {node_index} references an owner result outside a project",
                    context.owner.0
                ))
            })?;
            let target_input = project
                .owners
                .get(external.owner.0 as usize)
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "input of owner {} node {node_index} references missing owner {} result",
                        context.owner.0, external.owner.0
                    ))
                })?;
            let result = checked_expression_index(
                target_input.result,
                target_input.nodes.len(),
                "external owner result",
            )?;
            Ok(target_owner.expressions[result])
        }
    }
}

/// Resolve the producer capability named by an `OUT` actual.
///
/// Ordinary reads are detached occurrences and therefore cannot be used as
/// the destination of a call-frame equation. A bare `OUT` creates a private
/// variable, while forwarding an enclosing `OUT` aliases that formal's frame
/// variable directly. Keeping this distinction explicit prevents the call and
/// the formal-read projection from becoming competing directional writers.
fn edge_output_variable(
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    edge: &KernelOwnerInputEdge,
) -> Result<TypeVariableId, KernelOwnerBuildError> {
    let reference = edge.expression.0 as usize;
    let node = context.input.nodes.get(reference).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "OUT input of owner {} node {node_index} must reference a local output binding",
            context.owner.0
        ))
    })?;
    match &node.kind {
        KernelOwnerNodeKind::FreshOut => context
            .expressions
            .get(reference)
            .copied()
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "OUT input of owner {} node {node_index} references missing fresh output {reference}",
                    context.owner.0
                ))
            }),
        KernelOwnerNodeKind::FormalRead { formal, fields } if fields.is_empty() => context
            .formals
            .get(*formal as usize)
            .copied()
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "OUT input of owner {} node {node_index} forwards missing formal {formal}",
                    context.owner.0
                ))
            }),
        _ => Err(KernelOwnerBuildError::new(format!(
            "OUT input of owner {} node {node_index} must be a bare fresh output or enclosing OUT formal",
            context.owner.0
        ))),
    }
}

fn edge_reads_output_capability(
    context: &OwnerCompileContext<'_>,
    _node_index: usize,
    edge: &KernelOwnerInputEdge,
) -> Result<bool, KernelOwnerBuildError> {
    fn visit(
        context: &OwnerCompileContext<'_>,
        expression: usize,
        active: &mut BTreeSet<usize>,
    ) -> Result<bool, KernelOwnerBuildError> {
        if expression >= context.input.nodes.len() || !active.insert(expression) {
            return Ok(false);
        }
        let node = &context.input.nodes[expression];
        let result = match &node.kind {
            KernelOwnerNodeKind::FreshOut | KernelOwnerNodeKind::CollectionItemRead => true,
            KernelOwnerNodeKind::LexicalRead { .. }
            | KernelOwnerNodeKind::ValueRead { .. }
            | KernelOwnerNodeKind::DerivedRead { .. } => {
                let providers = node
                    .inputs
                    .iter()
                    .filter(|edge| matches!(edge.role, KernelOwnerEdgeRole::ReadProvider))
                    .collect::<Vec<_>>();
                let [provider] = providers.as_slice() else {
                    active.remove(&expression);
                    return Ok(false);
                };
                visit(context, provider.expression.0 as usize, active)?
            }
            _ => false,
        };
        active.remove(&expression);
        Ok(result)
    }

    visit(context, edge.expression.0 as usize, &mut BTreeSet::new())
}

fn referenced_node<'a>(
    context: &'a OwnerCompileContext<'a>,
    node_index: usize,
    edge: &KernelOwnerInputEdge,
) -> Result<&'a KernelOwnerNode, KernelOwnerBuildError> {
    let reference = edge.expression.0 as usize;
    if reference < context.input.nodes.len() {
        return context.input.nodes.get(reference).ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "owner {} node {node_index} references missing local node {reference}",
                context.owner.0
            ))
        });
    }
    let external_index = reference - context.input.nodes.len();
    let external = context
        .input
        .external_expressions
        .get(external_index)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "owner {} node {node_index} references missing external node {external_index}",
                context.owner.0
            ))
        })?;
    let project = context.project.ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "owner {} node {node_index} references an external node outside a project",
            context.owner.0
        ))
    })?;
    let target = project
        .owners
        .get(external.owner.0 as usize)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "owner {} node {node_index} references missing owner {}",
                context.owner.0, external.owner.0
            ))
        })?;
    let expression = match external.target {
        KernelExternalTarget::Expression(expression) => {
            checked_expression_index(expression, target.nodes.len(), "external node reference")?
        }
        KernelExternalTarget::Result => checked_expression_index(
            target.result,
            target.nodes.len(),
            "external result node reference",
        )?,
    };
    target.nodes.get(expression).ok_or_else(|| {
        KernelOwnerBuildError::new(format!(
            "owner {} node {node_index} references missing owner {} node {expression}",
            context.owner.0, external.owner.0
        ))
    })
}

fn edge_mode_variable(
    context: &OwnerCompileContext<'_>,
    node_index: usize,
    edge: &KernelOwnerInputEdge,
) -> Result<ModeVariableId, KernelOwnerBuildError> {
    let reference = edge.expression.0 as usize;
    if reference < context.input.nodes.len() {
        return Ok(context.expression_modes[reference]);
    }
    let external_index = reference - context.input.nodes.len();
    let external = context
        .input
        .external_expressions
        .get(external_index)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "mode input of owner {} node {node_index} references external expression {external_index} outside 0..{}",
                context.owner.0,
                context.input.external_expressions.len()
            ))
        })?;
    let target_owner = context
        .principals
        .get(external.owner.0 as usize)
        .ok_or_else(|| {
            KernelOwnerBuildError::new(format!(
                "mode input of owner {} node {node_index} references missing owner {}",
                context.owner.0, external.owner.0
            ))
        })?;
    match external.target {
        KernelExternalTarget::Expression(expression) => target_owner
            .expression_modes
            .get(expression.0 as usize)
            .copied()
            .ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "mode input of owner {} node {node_index} references missing owner {} expression {}",
                    context.owner.0, external.owner.0, expression.0
                ))
            }),
        KernelExternalTarget::Result => {
            let project = context.project.ok_or_else(|| {
                KernelOwnerBuildError::new(format!(
                    "mode input of owner {} node {node_index} references an owner result outside a project",
                    context.owner.0
                ))
            })?;
            let target_input = project
                .owners
                .get(external.owner.0 as usize)
                .ok_or_else(|| {
                    KernelOwnerBuildError::new(format!(
                        "mode input of owner {} node {node_index} references missing owner {} result",
                        context.owner.0, external.owner.0
                    ))
                })?;
            let result = checked_expression_index(
                target_input.result,
                target_input.nodes.len(),
                "external owner result mode",
            )?;
            Ok(target_owner.expression_modes[result])
        }
    }
}

fn checked_expression_index(
    expression: KernelExpressionId,
    len: usize,
    context: &str,
) -> Result<usize, KernelOwnerBuildError> {
    let index = expression.0 as usize;
    if index >= len {
        return Err(KernelOwnerBuildError::new(format!(
            "{context} references expression {} outside 0..{len}",
            expression.0
        )));
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_checked::{ObjectShape, Type, Variant};

    fn edge(role: KernelOwnerEdgeRole, expression: u32) -> KernelOwnerInputEdge {
        KernelOwnerInputEdge {
            role,
            expression: KernelExpressionId(expression),
        }
    }

    #[test]
    fn callback_requirements_shape_a_local_formal_item_without_coalescing_channels() {
        let mut builder = ComponentProgramBuilder::new();
        let formal = builder.new_contextual_hole();
        let formal_requirement = builder.new_contextual_hole();
        let segments = builder.terms_mut().intern_name("segments");
        let state = builder.terms_mut().intern_name("state");
        let click_time = builder.terms_mut().intern_name("click_time");

        let list = builder.new_variable();
        builder.add_projection_into(formal, [segments], list);
        let list_requirement =
            requirement_projection(&mut builder, formal_requirement, &[segments]);
        let list_term = builder.variable_term(list);
        let list_requirement_term = builder.variable_term(list_requirement);
        builder.add_unify(list_term, list_requirement_term);

        let item = builder.new_authoritative_provider();
        builder.add_collection_item_projection(list, item);
        let callback_requirement = builder.new_contextual_hole();
        let callback_requirement_term = builder.variable_term(callback_requirement);
        let callback_list_requirement = builder.terms_mut().list(callback_requirement_term);
        builder.add_unify(list_requirement_term, callback_list_requirement);

        for field in [state, click_time] {
            let requirement = builder.new_contextual_hole();
            let _ = requirement_projection(&mut builder, requirement, &[field]);
            let callback_requirement_term = builder.variable_term(callback_requirement);
            let requirement_term = builder.variable_term(requirement);
            builder.add_unify(callback_requirement_term, requirement_term);
        }
        let requirement_output = builder.add_output(callback_requirement, FlowMode::Continuous);
        let output = builder.add_output(item, FlowMode::Continuous);

        let artifact = solve_component(builder.finish()).unwrap();
        let Type::Object(requirement_shape) =
            &artifact.output(requirement_output).unwrap().flow_type.ty
        else {
            panic!("callback requirement must produce an open object")
        };
        assert_eq!(
            requirement_shape
                .fields
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["click_time".to_owned(), "state".to_owned()])
        );
        let Type::Object(shape) = &artifact.output(output).unwrap().flow_type.ty else {
            panic!("callback item requirement must produce an open object")
        };
        assert!(shape.open);
        assert_eq!(
            shape.fields.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from(["click_time".to_owned(), "state".to_owned()])
        );
    }

    fn syntax_expression(id: usize, kind: AstExprKind) -> AstExpr {
        AstExpr {
            id,
            line: id + 1,
            start: id * 10,
            end: id * 10 + 1,
            linked_input: None,
            kind,
        }
    }

    #[test]
    fn source_expression_diagnostics_are_typed_and_published_without_checked_rows() {
        let syntax = vec![
            syntax_expression(10, AstExprKind::Unknown(vec!["?".to_owned()])),
            syntax_expression(
                11,
                AstExprKind::MatchArm {
                    pattern: AstMatchPattern::Invalid {
                        message: "bad".to_owned(),
                    },
                    output: None,
                },
            ),
            syntax_expression(12, AstExprKind::Number("1/0".to_owned())),
            syntax_expression(
                13,
                AstExprKind::BitsLiteral {
                    width: 4,
                    radix: 2,
                    digits: "11111".to_owned(),
                },
            ),
            syntax_expression(
                14,
                AstExprKind::ByteLiteral {
                    radix: 16,
                    digits: "FF".to_owned(),
                    value: 255,
                },
            ),
            syntax_expression(
                15,
                AstExprKind::ByteLiteral {
                    radix: 16,
                    digits: "00".to_owned(),
                    value: 0,
                },
            ),
            syntax_expression(
                16,
                AstExprKind::BytesLiteral {
                    size: boon_syntax::BytesSizeSyntax::Fixed(1),
                    items: vec![15],
                },
            ),
        ];
        let diagnostics = project_kernel_source_expression_diagnostics(
            syntax
                .iter()
                .enumerate()
                .map(|(dense, expression)| (KernelExpressionId(dense as u32), expression)),
        )
        .expect("source diagnostic projection");
        assert_eq!(diagnostics.len(), 5);
        assert!(matches!(
            diagnostics[0].kind,
            KernelDiagnosticKind::InvalidExpression { .. }
        ));
        assert!(matches!(
            diagnostics[1].kind,
            KernelDiagnosticKind::InvalidPattern
        ));
        assert!(matches!(
            diagnostics[2].kind,
            KernelDiagnosticKind::InvalidNumberLiteral {
                reason: KernelNumberLiteralErrorReason::ZeroDenominator,
                ..
            }
        ));
        assert!(matches!(
            diagnostics[3].kind,
            KernelDiagnosticKind::InvalidBitsLiteral { .. }
        ));
        assert!(matches!(
            diagnostics[4].kind,
            KernelDiagnosticKind::ByteLiteralOutsideBytes
        ));

        let program = KernelProjectProgramInput {
            owners: vec![KernelOwnerProgramInput {
                nodes: (0..syntax.len())
                    .map(|_| KernelOwnerNode {
                        kind: KernelOwnerNodeKind::Unknown,
                        inputs: Box::new([]),
                        mode: FlowMode::Continuous,
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
                formal_count: 0,
                external_expressions: Box::new([]),
                result: KernelExpressionId(6),
            }]
            .into_boxed_slice(),
        };
        let facts = vec![KernelDefinitionFactsInput {
            diagnostics,
            ..KernelDefinitionFactsInput::default()
        }]
        .into_boxed_slice();
        let solved = compile_project_program_with_definition_facts(&program, &facts)
            .expect("source diagnostic program compiles")
            .solve_graph()
            .expect("source diagnostic program solves");
        let interfaces = solved.interface_snapshot();
        assert_eq!(interfaces.diagnostics.len(), 5);
        let checked = solved
            .checked_snapshot()
            .expect("source diagnostics seal into checked image");
        assert_eq!(checked.definitions[0].diagnostics, interfaces.diagnostics);
        assert!(checked.definitions[0].statements.is_empty());
    }

    #[test]
    fn call_shape_failures_are_typed_without_constructing_a_call_node() {
        let parameters = vec![
            KernelCallShapeParameter {
                ordinal: 0,
                kind: KernelParameterKind::Value,
                name: "first".into(),
                optional: false,
                evaluation_scope: KernelParameterEvaluationScope::Parent,
            },
            KernelCallShapeParameter {
                ordinal: 1,
                kind: KernelParameterKind::Value,
                name: "second".into(),
                optional: false,
                evaluation_scope: KernelParameterEvaluationScope::Parent,
            },
        ]
        .into_boxed_slice();
        let resolution = KernelCallShapeResolution::Callable {
            kind: KernelCallableKind::User,
            parameters: parameters.clone(),
            context_ordinal: None,
            caller_context_ordinal: None,
        };
        let missing = project_kernel_call_shape(
            &KernelCallShapeInput {
                expression: KernelExpressionId(4),
                function: "needs".into(),
                pipe: false,
                arguments: vec![KernelCallShapeArgument {
                    source: KernelCallArgumentSource::CallArgument { ordinal: 0 },
                    kind: KernelCallArgumentKind::Named,
                    name: "first".into(),
                }]
                .into_boxed_slice(),
                pass: false,
            },
            &resolution,
        )
        .expect("missing call entry is a diagnostic, not a build failure");
        assert!(!missing.valid);
        assert!(matches!(
            missing.diagnostics.as_ref(),
            [KernelDiagnosticInput {
                site: KernelDiagnosticSite::Expression {
                    expression: KernelExpressionId(4)
                },
                kind: KernelDiagnosticKind::MissingCallEntry { function, name },
                ..
            }] if function.as_ref() == "needs" && name.as_ref() == "second"
        ));

        let pipe = project_kernel_call_shape(
            &KernelCallShapeInput {
                expression: KernelExpressionId(8),
                function: "needs".into(),
                pipe: true,
                arguments: vec![KernelCallShapeArgument {
                    source: KernelCallArgumentSource::PipeArgument { ordinal: 0 },
                    kind: KernelCallArgumentKind::Named,
                    name: "second".into(),
                }]
                .into_boxed_slice(),
                pass: false,
            },
            &resolution,
        )
        .expect("piped call shape");
        assert!(pipe.valid);
        assert_eq!(
            pipe.matched_inputs.as_ref(),
            [
                KernelMatchedCallInput {
                    source: KernelCallArgumentSource::PipeInput,
                    formal_ordinal: 0,
                },
                KernelMatchedCallInput {
                    source: KernelCallArgumentSource::PipeArgument { ordinal: 0 },
                    formal_ordinal: 1,
                },
            ]
        );

        let unresolved = project_kernel_call_shape(
            &KernelCallShapeInput {
                expression: KernelExpressionId(9),
                function: "mystery".into(),
                pipe: false,
                arguments: Box::new([]),
                pass: false,
            },
            &KernelCallShapeResolution::Unresolved,
        )
        .expect("unresolved call is a typed fact");
        assert!(!unresolved.valid);
        assert!(matches!(
            unresolved.diagnostics.as_ref(),
            [KernelDiagnosticInput {
                kind: KernelDiagnosticKind::UnresolvedCallable { function },
                ..
            }] if function.as_ref() == "mystery"
        ));

        let authoritative = project_kernel_call_shape(
            &KernelCallShapeInput {
                expression: KernelExpressionId(10),
                function: "builtin".into(),
                pipe: true,
                arguments: Box::new([]),
                pass: true,
            },
            &KernelCallShapeResolution::Callable {
                kind: KernelCallableKind::Builtin,
                parameters: vec![KernelCallShapeParameter {
                    ordinal: 0,
                    kind: KernelParameterKind::Out,
                    name: "output".into(),
                    optional: false,
                    evaluation_scope: KernelParameterEvaluationScope::Parent,
                }]
                .into_boxed_slice(),
                context_ordinal: None,
                caller_context_ordinal: None,
            },
        )
        .expect("authoritative call failures are typed facts");
        assert!(!authoritative.valid);
        assert!(matches!(
            authoritative.diagnostics.as_ref(),
            [
                KernelDiagnosticInput {
                    site: KernelDiagnosticSite::Expression {
                        expression: KernelExpressionId(10)
                    },
                    kind: KernelDiagnosticKind::PipeWithoutValueInput { function },
                    ..
                },
                KernelDiagnosticInput {
                    kind: KernelDiagnosticKind::MissingCallEntry { name, .. },
                    ..
                },
                KernelDiagnosticInput {
                    site: KernelDiagnosticSite::CallPass {
                        call: KernelExpressionId(10),
                        pipe: true,
                    },
                    kind: KernelDiagnosticKind::PassOnAuthoritativeCallable {
                        function: pass_function,
                        callable_kind: KernelCallableKind::Builtin,
                    },
                    ..
                },
            ] if function.as_ref() == "builtin"
                && pass_function.as_ref() == "builtin"
                && name.as_ref() == "output"
        ));
    }

    #[test]
    fn definition_statement_rows_keep_dense_values_and_child_topology() {
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Known(Type::Number),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Known(Type::Text),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let facts = KernelDefinitionFactsInput {
            statements: vec![
                KernelStatementInput {
                    id: KernelStatementId(0),
                    kind: KernelStatementKind::Block,
                    value: Some(KernelExpressionId(1)),
                    children: vec![KernelStatementChildReference::Local(KernelStatementId(1))]
                        .into_boxed_slice(),
                },
                KernelStatementInput {
                    id: KernelStatementId(1),
                    kind: KernelStatementKind::Expression,
                    value: Some(KernelExpressionId(0)),
                    children: Box::new([]),
                },
            ]
            .into_boxed_slice(),
            ..KernelDefinitionFactsInput::default()
        };

        let artifact = compile_owner_program_with_definition_facts(&input, &facts)
            .unwrap()
            .solve()
            .unwrap();

        assert_eq!(artifact.definition.statements.len(), 2);
        assert_eq!(artifact.definition.statements[0].id, KernelStatementId(0));
        assert_eq!(
            artifact.definition.statements[0].value,
            Some(KernelValueReference::Local(KernelExpressionId(1)))
        );
        assert_eq!(
            artifact.definition.statements[0].children.as_ref(),
            [KernelStatementChildReference::Local(KernelStatementId(1))]
        );
        assert_eq!(artifact.definition.statements[1].id, KernelStatementId(1));

        let mut invalid = facts.clone();
        invalid.statements[1].id = KernelStatementId(4);
        let error = compile_owner_program_with_definition_facts(&input, &invalid)
            .err()
            .expect("non-dense statement IDs must fail closed");
        assert!(error.to_string().contains("dense IDs"));
    }

    #[test]
    fn definition_relocations_are_exact_dense_linker_authority() {
        let source_unit_id = boon_syntax::SourceUnitId::from_path("relocations.bn").unwrap();
        let expression = |digest| StableExpressionKey {
            source_unit_id: source_unit_id.clone(),
            route_digest_v1: [digest; 32],
        };
        let statement = StableStatementKey {
            source_unit_id: source_unit_id.clone(),
            route: boon_syntax::StableStatementRoute {
                owner: None,
                statement_route: Vec::new(),
            },
        };
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let facts = KernelDefinitionFactsInput {
            relocations: KernelDefinitionRelocations {
                expressions: vec![
                    KernelExpressionRelocation::Authored(expression(1)),
                    KernelExpressionRelocation::Authored(expression(2)),
                ]
                .into_boxed_slice(),
                statements: vec![statement].into_boxed_slice(),
            },
            statements: vec![KernelStatementInput {
                id: KernelStatementId(0),
                kind: KernelStatementKind::Expression,
                value: Some(KernelExpressionId(1)),
                children: Box::new([]),
            }]
            .into_boxed_slice(),
            ..KernelDefinitionFactsInput::default()
        };

        let solved = compile_owner_program_with_definition_facts(&input, &facts)
            .unwrap()
            .solve()
            .unwrap();
        assert_eq!(solved.definition.relocations, facts.relocations);

        let mut moved = facts.clone();
        moved.relocations.expressions[1] = KernelExpressionRelocation::Authored(expression(3));
        let moved = compile_owner_program_with_definition_facts(&input, &moved)
            .unwrap()
            .solve()
            .unwrap();
        assert_eq!(solved.definition.result, moved.definition.result);
        assert_eq!(
            solved.currentness.public_result_fingerprint_v1,
            moved.currentness.public_result_fingerprint_v1
        );
        assert_ne!(
            solved.currentness.basis_fingerprint_v4,
            moved.currentness.basis_fingerprint_v4
        );
        assert_ne!(
            solved.currentness.artifact_fingerprint_v6,
            moved.currentness.artifact_fingerprint_v6
        );
        assert_ne!(
            solved.currentness.fingerprint_v6,
            moved.currentness.fingerprint_v6
        );

        let mut missing = facts.clone();
        missing.relocations.expressions =
            vec![KernelExpressionRelocation::Authored(expression(1))].into_boxed_slice();
        let error = compile_owner_program_with_definition_facts(&input, &missing)
            .err()
            .expect("incomplete relocations must fail closed");
        assert!(error.to_string().contains("for 2 expressions"));

        let mut duplicate = facts.clone();
        duplicate.relocations.expressions = vec![
            KernelExpressionRelocation::Authored(expression(1)),
            KernelExpressionRelocation::Authored(expression(1)),
        ]
        .into_boxed_slice();
        let error = compile_owner_program_with_definition_facts(&input, &duplicate)
            .err()
            .expect("duplicate relocations must fail closed");
        assert!(error.to_string().contains("repeats a stable expression"));
    }

    #[test]
    fn definition_declarations_and_lexical_bindings_are_validated_and_relocated() {
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Known(Type::Number),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "value".into(),
                            spread: false,
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::LexicalRead {
                        fields: Box::new([]),
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 0)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let facts = KernelDefinitionFactsInput {
            relocations: KernelDefinitionRelocations::default(),
            statements: vec![KernelStatementInput {
                id: KernelStatementId(0),
                kind: KernelStatementKind::Field {
                    name: "root".into(),
                },
                value: Some(KernelExpressionId(1)),
                children: Box::new([]),
            }]
            .into_boxed_slice(),
            declarations: vec![
                KernelDeclarationInput {
                    id: KernelDeclarationId(0),
                    origin: KernelDeclarationOrigin::Statement {
                        statement: KernelStatementId(0),
                    },
                    name: "root".into(),
                    kind: KernelDeclarationKind::Field,
                    value: Some(KernelExpressionId(1)),
                },
                KernelDeclarationInput {
                    id: KernelDeclarationId(1),
                    origin: KernelDeclarationOrigin::RecordField {
                        object: KernelExpressionId(1),
                        ordinal: 0,
                    },
                    name: "value".into(),
                    kind: KernelDeclarationKind::Field,
                    value: Some(KernelExpressionId(0)),
                },
            ]
            .into_boxed_slice(),
            lexical_bindings: vec![KernelLexicalBindingInput {
                expression: KernelExpressionId(2),
                target: KernelLexicalBindingTargetInput::Declaration(
                    KernelDeclarationReference::Local(KernelDeclarationId(1)),
                ),
                projection: Box::new([]),
                access: KernelLexicalAccess::Drain,
            }]
            .into_boxed_slice(),
            sources: Box::new([]),
            states: Box::new([]),
            lists: Box::new([]),
            diagnostics: Box::new([]),
            diagnostic_values: Box::new([]),
        };

        let artifact = compile_owner_program_with_definition_facts(&input, &facts)
            .unwrap()
            .solve()
            .unwrap();

        assert_eq!(artifact.definition.declarations.len(), 2);
        assert_eq!(
            artifact.definition.declarations[1].value,
            Some(KernelValueReference::Local(KernelExpressionId(0)))
        );
        assert_eq!(
            artifact.definition.lexical_bindings.as_ref(),
            [KernelLexicalBindingArtifact {
                expression: KernelExpressionId(2),
                target: KernelLexicalBindingTarget::Declaration(KernelDeclarationReference::Local(
                    KernelDeclarationId(1)
                )),
                projection: Box::new([]),
                access: KernelLexicalAccess::Drain,
            }]
        );

        let mut invalid_origin = facts.clone();
        invalid_origin.declarations[1].name = "missing".into();
        let error = compile_owner_program_with_definition_facts(&input, &invalid_origin)
            .err()
            .expect("a record declaration must name its exact structural field");
        assert!(error.to_string().contains("incompatible origin"));

        let mut missing_target = facts.clone();
        missing_target.lexical_bindings[0].target = KernelLexicalBindingTargetInput::Declaration(
            KernelDeclarationReference::Local(KernelDeclarationId(7)),
        );
        let error = compile_owner_program_with_definition_facts(&input, &missing_target)
            .err()
            .expect("a lexical row must not target a missing declaration");
        assert!(error.to_string().contains("missing declaration 7"));
    }

    #[test]
    fn definition_resources_materialize_from_one_solved_expression_table() {
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Source(Type::Number),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Off".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("On".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Hold,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::HoldInitial, 1),
                        edge(KernelOwnerEdgeRole::HoldUpdate, 2),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Collection {
                        kind: KernelCollectionKind::List,
                        capacity: Some(4),
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CollectionItem, 4)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "pulse".into(),
                                spread: false,
                            },
                            0,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "state".into(),
                                spread: false,
                            },
                            3,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "rows".into(),
                                spread: false,
                            },
                            5,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(6),
        };
        let statements = vec![
            KernelStatementInput {
                id: KernelStatementId(0),
                kind: KernelStatementKind::Field {
                    name: "root".into(),
                },
                value: Some(KernelExpressionId(6)),
                children: vec![
                    KernelStatementChildReference::Local(KernelStatementId(1)),
                    KernelStatementChildReference::Local(KernelStatementId(2)),
                    KernelStatementChildReference::Local(KernelStatementId(3)),
                ]
                .into_boxed_slice(),
            },
            KernelStatementInput {
                id: KernelStatementId(1),
                kind: KernelStatementKind::Source {
                    field: Some("pulse".into()),
                    event: None,
                },
                value: Some(KernelExpressionId(0)),
                children: Box::new([]),
            },
            KernelStatementInput {
                id: KernelStatementId(2),
                kind: KernelStatementKind::Hold {
                    field: Some("state".into()),
                    name: Some("state".into()),
                },
                value: Some(KernelExpressionId(3)),
                children: Box::new([]),
            },
            KernelStatementInput {
                id: KernelStatementId(3),
                kind: KernelStatementKind::List {
                    field: Some("rows".into()),
                    capacity: Some(4),
                },
                value: Some(KernelExpressionId(5)),
                children: Box::new([]),
            },
        ]
        .into_boxed_slice();
        let declarations = [
            ("root", KernelDeclarationKind::Field, 6u32),
            ("pulse", KernelDeclarationKind::Source, 0),
            ("state", KernelDeclarationKind::Hold, 3),
            ("rows", KernelDeclarationKind::List, 5),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (name, kind, value))| KernelDeclarationInput {
            id: KernelDeclarationId(u32::try_from(index).unwrap()),
            origin: KernelDeclarationOrigin::Statement {
                statement: KernelStatementId(u32::try_from(index).unwrap()),
            },
            name: name.into(),
            kind,
            value: Some(KernelExpressionId(value)),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
        let facts = KernelDefinitionFactsInput {
            relocations: KernelDefinitionRelocations::default(),
            statements,
            declarations,
            lexical_bindings: Box::new([]),
            sources: vec![KernelSourceInput {
                id: KernelSourceId(0),
                declaration: KernelDeclarationReference::Local(KernelDeclarationId(1)),
                statement: KernelStatementReference::Local(KernelStatementId(1)),
                expression: KernelExpressionId(0),
                projection: Box::new([]),
                interval_ms: None,
            }]
            .into_boxed_slice(),
            states: vec![KernelStateInput {
                id: KernelStateId(0),
                binding_declaration: KernelDeclarationReference::Local(KernelDeclarationId(2)),
                declaration: KernelDeclarationReference::Local(KernelDeclarationId(2)),
                statement: KernelStatementReference::Local(KernelStatementId(2)),
                expression: KernelExpressionId(3),
                initial: KernelExpressionId(1),
                projection: Box::new([]),
                kind: CheckedStateKind::Hold,
            }]
            .into_boxed_slice(),
            lists: vec![KernelListInput {
                id: KernelListId(0),
                declaration: KernelDeclarationReference::Local(KernelDeclarationId(3)),
                statement: KernelStatementReference::Local(KernelStatementId(3)),
                producer: KernelExpressionId(5),
                projection: Box::new([]),
                capacity: Some(4),
                key_policy: CheckedListKeyPolicy::GeneratedOccurrenceU64 {
                    has_generation: true,
                },
            }]
            .into_boxed_slice(),
            diagnostics: Box::new([]),
            diagnostic_values: Box::new([]),
        };

        let artifact = compile_owner_program_with_definition_facts(&input, &facts)
            .unwrap()
            .solve()
            .unwrap()
            .definition;

        let [source] = artifact.sources.as_ref() else {
            panic!("one SOURCE artifact must be materialized")
        };
        assert_eq!(source.payload_type, Type::Number);
        assert_eq!(source.expression, KernelExpressionId(0));
        let [state] = artifact.states.as_ref() else {
            panic!("one HOLD state artifact must be materialized")
        };
        assert_eq!(
            state.initial,
            KernelValueReference::Local(KernelExpressionId(1))
        );
        assert_eq!(
            state.flow_type.ty,
            Type::VariantSet(
                vec![
                    Variant::Tag("Off".to_owned()),
                    Variant::Tag("On".to_owned())
                ]
                .into()
            )
        );
        let [list] = artifact.lists.as_ref() else {
            panic!("one persistent LIST artifact must be materialized")
        };
        assert_eq!(list.item_type, Type::Number);
        assert_eq!(list.capacity, Some(4));
        assert_eq!(list.path.anchor, list.declaration);

        let mut invalid = facts;
        invalid.sources[0].expression = KernelExpressionId(4);
        let error = compile_owner_program_with_definition_facts(&input, &invalid)
            .err()
            .expect("a SOURCE artifact must name a SOURCE node");
        assert!(error.to_string().contains("is not a literal SOURCE"));
    }

    #[test]
    fn user_call_modes_follow_fixed_builtin_inputs_contextually() {
        let input = KernelProjectProgramInput {
            owners: vec![
                KernelOwnerProgramInput {
                    nodes: vec![KernelOwnerNode {
                        kind: KernelOwnerNodeKind::FormalRead {
                            formal: 0,
                            fields: Box::new([]),
                        },
                        inputs: Box::new([]),
                        mode: FlowMode::Continuous,
                    }]
                    .into_boxed_slice(),
                    formal_count: 1,
                    external_expressions: Box::new([]),
                    result: KernelExpressionId(0),
                },
                KernelOwnerProgramInput {
                    nodes: vec![
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::Known(Type::Text),
                            inputs: Box::new([]),
                            mode: FlowMode::PresentOrAbsent,
                        },
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::PureBuiltin {
                                kind: KernelPureBuiltinKind::TextTransform,
                            },
                            inputs: vec![edge(
                                KernelOwnerEdgeRole::AbiArgument {
                                    name: "$pipe".into(),
                                },
                                0,
                            )]
                            .into_boxed_slice(),
                            mode: FlowMode::Continuous,
                        },
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::UserCall {
                                target: KernelOwnerId(0),
                                inherited_formal: None,
                            },
                            inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 1)]
                                .into_boxed_slice(),
                            mode: FlowMode::Continuous,
                        },
                    ]
                    .into_boxed_slice(),
                    formal_count: 0,
                    external_expressions: Box::new([]),
                    result: KernelExpressionId(2),
                },
            ]
            .into_boxed_slice(),
        };

        let artifact = compile_project_program(&input).unwrap().solve().unwrap();

        assert_eq!(
            artifact.definitions[1].expressions[1].flow_type.mode,
            FlowMode::Continuous,
            "the builtin's ordinary checked surface remains fixed"
        );
        assert_eq!(
            artifact.definitions[1].result.mode,
            FlowMode::PresentOrAbsent,
            "the user-call frame follows the builtin's eventful actual"
        );
    }

    #[test]
    fn nested_public_result_reads_preserve_the_field_mode() {
        let input = KernelProjectProgramInput {
            owners: vec![
                KernelOwnerProgramInput {
                    nodes: vec![
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::Known(Type::Text),
                            inputs: Box::new([]),
                            mode: FlowMode::PresentOrAbsent,
                        },
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::Number,
                            inputs: Box::new([]),
                            mode: FlowMode::Continuous,
                        },
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::Record { tag: None },
                            inputs: vec![
                                edge(
                                    KernelOwnerEdgeRole::RecordField {
                                        name: "event".into(),
                                        spread: false,
                                    },
                                    0,
                                ),
                                edge(
                                    KernelOwnerEdgeRole::RecordField {
                                        name: "stable".into(),
                                        spread: false,
                                    },
                                    1,
                                ),
                            ]
                            .into_boxed_slice(),
                            mode: FlowMode::Continuous,
                        },
                    ]
                    .into_boxed_slice(),
                    formal_count: 0,
                    external_expressions: Box::new([]),
                    result: KernelExpressionId(2),
                },
                KernelOwnerProgramInput {
                    nodes: vec![KernelOwnerNode {
                        kind: KernelOwnerNodeKind::ValueRead {
                            fields: vec!["event".into()].into_boxed_slice(),
                            mode_narrowing: None,
                        },
                        inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 1)].into_boxed_slice(),
                        mode: FlowMode::Continuous,
                    }]
                    .into_boxed_slice(),
                    formal_count: 0,
                    external_expressions: vec![KernelExternalExpression {
                        owner: KernelOwnerId(0),
                        target: KernelExternalTarget::Result,
                    }]
                    .into_boxed_slice(),
                    result: KernelExpressionId(0),
                },
            ]
            .into_boxed_slice(),
        };

        let artifact = compile_project_program(&input).unwrap().solve().unwrap();

        assert_eq!(
            artifact.definitions[0].result.mode,
            FlowMode::Continuous,
            "the public record remains continuous",
        );
        assert_eq!(
            artifact.definitions[1].result.mode,
            FlowMode::PresentOrAbsent,
            "the nested read follows the event field through the public result",
        );
    }

    #[test]
    fn list_append_widens_existing_and_appended_items_directionally() {
        let completed = |tag: &str| {
            Type::object(ObjectShape::from_ordered_fields(
                [(
                    "completed".to_owned(),
                    Type::VariantSet(vec![Variant::Tag(tag.to_owned())].into()),
                )],
                false,
            ))
        };
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Known(Type::List(Type::shared(completed("True")))),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Known(completed("False")),
                    inputs: Box::new([]),
                    mode: FlowMode::PresentOrAbsent,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::PureBuiltin {
                        kind: KernelPureBuiltinKind::ListAppend,
                    },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::AbiArgument {
                                name: "$pipe".into(),
                            },
                            0,
                        ),
                        edge(
                            KernelOwnerEdgeRole::AbiArgument {
                                name: "item".into(),
                            },
                            1,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };

        let artifact = compile_owner_program(&input).unwrap().solve().unwrap();

        assert_eq!(
            artifact.definition.result.ty,
            Type::List(Type::shared(Type::object(
                ObjectShape::from_ordered_fields(
                    [(
                        "completed".to_owned(),
                        Type::VariantSet(
                            vec![
                                Variant::Tag("False".to_owned()),
                                Variant::Tag("True".to_owned()),
                            ]
                            .into(),
                        ),
                    )],
                    false,
                ),
            ))),
        );
        assert_eq!(
            artifact.definition.expressions[0].flow_type.ty,
            Type::List(Type::shared(completed("True"))),
            "the widened append result must not backflow into the input list",
        );
        assert_eq!(
            artifact.definition.expressions[1].flow_type.ty,
            completed("False"),
            "the widened append result must not backflow into the appended item",
        );
    }

    #[test]
    fn projected_mode_ignores_unproven_continuous_branches() {
        let mut builder = ModeProgramBuilder::default();
        let continuous = builder.new_variable(FlowMode::Continuous);
        let present = builder.new_variable(FlowMode::PresentOrAbsent);
        builder.set(continuous, ModeEquation::Fixed(FlowMode::Continuous));
        builder.set(present, ModeEquation::Fixed(FlowMode::PresentOrAbsent));
        let continuous_projection = eventful_projected_mode(&mut builder, continuous);
        let present_projection = eventful_projected_mode(&mut builder, present);
        let merged = builder.new_variable(FlowMode::Continuous);
        builder.set(
            merged,
            ModeEquation::Latest(
                vec![continuous_projection, present_projection].into_boxed_slice(),
            ),
        );

        let modes = builder.solve();

        assert_eq!(modes[merged.0 as usize], FlowMode::PresentOrAbsent);
    }

    #[test]
    fn missing_record_field_does_not_mask_an_eventful_projection() {
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Known(Type::Number),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "other".into(),
                            spread: false,
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Known(Type::Text),
                    inputs: Box::new([]),
                    mode: FlowMode::PresentOrAbsent,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "wanted".into(),
                            spread: false,
                        },
                        2,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Latest,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::LatestBranch, 1),
                        edge(KernelOwnerEdgeRole::LatestBranch, 3),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::DerivedRead {
                        fields: vec!["wanted".into()].into_boxed_slice(),
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 4)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::DerivedRead {
                        fields: vec!["wanted".into()].into_boxed_slice(),
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 1)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(5),
        };

        let artifact = compile_owner_program(&input).unwrap().solve().unwrap();

        assert_eq!(
            artifact.definition.result.mode,
            FlowMode::PresentOrAbsent,
            "a closed branch without the projected field contributes no continuous provider",
        );
        assert_eq!(
            artifact.definition.expressions[6].flow_type.mode,
            FlowMode::Continuous,
            "a direct missing-field occurrence retains its declared/root mode",
        );
    }

    #[test]
    fn tag_matched_value_read_uses_the_selector_mode() {
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Initial".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Known(Type::Text),
                    inputs: Box::new([]),
                    mode: FlowMode::PresentOrAbsent,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record {
                        tag: Some("Ready".into()),
                    },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "wanted".into(),
                            spread: false,
                        },
                        1,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Latest,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::LatestBranch, 0),
                        edge(KernelOwnerEdgeRole::LatestBranch, 2),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::ValueRead {
                        fields: Box::new([]),
                        mode_narrowing: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 3)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::ValueRead {
                        fields: vec!["wanted".into()].into_boxed_slice(),
                        mode_narrowing: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 3)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::ValueRead {
                        fields: vec!["wanted".into()].into_boxed_slice(),
                        mode_narrowing: Some(KernelExpressionId(4)),
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 3)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Tag {
                            name: "Ready".into(),
                            fields: Box::new([]),
                        },
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 6)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Wildcard,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 8)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::When,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::WhenInput, 4),
                        edge(KernelOwnerEdgeRole::WhenArm, 7),
                        edge(KernelOwnerEdgeRole::WhenArm, 9),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(10),
        };

        let artifact = compile_owner_program(&input).unwrap().solve().unwrap();

        assert_eq!(
            artifact.definition.expressions[5].flow_type.mode,
            FlowMode::PresentOrAbsent,
            "an unguarded projection retains the eventful update surface",
        );
        assert_eq!(
            artifact.definition.expressions[6].flow_type.mode,
            FlowMode::Continuous,
            "the matching tag makes the retained selector mode authoritative",
        );
    }

    #[test]
    fn invocation_match_arms_alias_their_outputs_without_a_publish_cell() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Tag {
                            name: "True".into(),
                            fields: Box::new([]),
                        },
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 1)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::When,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::WhenInput, 0),
                        edge(KernelOwnerEdgeRole::WhenArm, 2),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(3),
        };
        let wrapper = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("True".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(1),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, wrapper, caller].into_boxed_slice(),
        })
        .unwrap();
        assert_eq!(program.compile_work().invocation_frames, 0);
        assert!(program.component().operations.iter().any(|operation| {
            matches!(
                operation.as_ref(),
                crate::KernelOperation::SummaryCall { program, .. }
                    if program.nodes.iter().any(|node| matches!(node, KernelSummaryNode::Select { .. }))
                        && program.nodes.iter().all(|node| !matches!(node, KernelSummaryNode::Invoke { .. }))
            )
        }), "the tiny selector callee should remain inline below the sharing threshold");
        assert_eq!(
            program.solve().unwrap().definitions[2].result.ty,
            Type::Number
        );
    }

    #[test]
    fn owner_program_compiles_a_widened_record_list() {
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Header".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "kind".into(),
                            spread: false,
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Empty".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "kind".into(),
                            spread: false,
                        },
                        2,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Collection {
                        kind: KernelCollectionKind::List,
                        capacity: None,
                    },
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::CollectionItem, 1),
                        edge(KernelOwnerEdgeRole::CollectionItem, 3),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(4),
        };

        let artifact = compile_owner_program(&input).unwrap().solve().unwrap();
        let Type::List(item) = artifact.definition.result.ty else {
            panic!("owner result must be a list")
        };
        let Type::Object(item) = item.as_ref() else {
            panic!("list item must be a record")
        };
        assert_eq!(
            item.fields["kind"],
            Type::VariantSet(
                vec![
                    Variant::Tag("Empty".to_owned()),
                    Variant::Tag("Header".to_owned())
                ]
                .into()
            )
        );
        assert_eq!(
            artifact.definition.expressions[1].flow_type.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [(
                    "kind".to_owned(),
                    Type::VariantSet(vec![Variant::Tag("Header".to_owned())].into()),
                )],
                false,
            )),
            "directional collection widening must not backflow into producers"
        );
    }

    #[test]
    fn empty_latest_is_an_unknown_shape_and_not_an_absent_hold_update() {
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Latest,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Hold,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::HoldInitial, 0),
                        edge(KernelOwnerEdgeRole::HoldUpdate, 1),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };

        let artifact = compile_owner_program(&input).unwrap().solve().unwrap();

        assert_eq!(
            artifact.definition.expressions[1].flow_type.ty,
            Type::Unknown
        );
        assert_eq!(
            artifact.definition.expressions[1].flow_type.mode,
            FlowMode::Continuous
        );
        assert_eq!(artifact.definition.result.ty, Type::Number);
    }

    #[test]
    fn host_effect_call_and_policy_are_published_once_in_the_definition_artifact() {
        let input = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::HostEffect {
                    operation: "Clock/wall".into(),
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };

        let artifact = compile_owner_program(&input).unwrap().solve().unwrap();
        let [call] = artifact.definition.calls.as_ref() else {
            panic!("host effect must publish one call artifact")
        };
        let [effect] = artifact.definition.effects.as_ref() else {
            panic!("host effect must publish one policy artifact")
        };
        let spec = host_effect_spec("Clock/wall").expect("wall-clock effect ABI");

        assert_eq!(call.expression, KernelExpressionId(0));
        assert_eq!(
            call.target,
            KernelCallTarget::HostEffect {
                operation: "Clock/wall".into(),
            }
        );
        assert!(call.inputs.is_empty());
        assert_eq!(call.result, artifact.definition.expressions[0].flow_type);
        assert_eq!(effect.expression, KernelExpressionId(0));
        assert_eq!(effect.operation.as_ref(), spec.operation);
        assert_eq!(effect.replay, spec.replay);
        assert_eq!(effect.barrier, spec.barrier);
        assert_eq!(effect.result_policy, spec.result_policy);
        assert_eq!(effect.delivery, spec.delivery);
    }

    #[test]
    fn empty_collections_use_language_neutral_item_authorities() {
        let solve = |kind| {
            compile_owner_program(&KernelOwnerProgramInput {
                nodes: vec![KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Collection {
                        kind,
                        capacity: None,
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                }]
                .into_boxed_slice(),
                formal_count: 0,
                external_expressions: Box::new([]),
                result: KernelExpressionId(0),
            })
            .unwrap()
            .solve()
            .unwrap()
            .definition
            .result
            .ty
        };

        assert_eq!(
            solve(KernelCollectionKind::List),
            Type::List(Type::shared(Type::object(ObjectShape::new(
                std::collections::BTreeMap::new(),
                true,
            ))))
        );
        assert_eq!(
            solve(KernelCollectionKind::Set),
            Type::Set(Type::shared(Type::Unknown))
        );
        assert_eq!(
            solve(KernelCollectionKind::Map),
            Type::Map {
                key: Box::new(Type::Unknown),
                value: Box::new(Type::Unknown),
            }
        );
    }

    #[test]
    fn stripe_constructor_compiles_direction_to_one_exact_render_kind() {
        let solve = |direction: &str| {
            compile_owner_program(&KernelOwnerProgramInput {
                nodes: vec![
                    KernelOwnerNode {
                        kind: KernelOwnerNodeKind::Tag(direction.into()),
                        inputs: Box::new([]),
                        mode: FlowMode::Continuous,
                    },
                    KernelOwnerNode {
                        kind: KernelOwnerNodeKind::RenderConstructor {
                            kind: KernelRenderConstructorKind::StripeDirection,
                        },
                        inputs: vec![edge(
                            KernelOwnerEdgeRole::AbiArgument {
                                name: "direction".into(),
                            },
                            0,
                        )]
                        .into_boxed_slice(),
                        mode: FlowMode::Continuous,
                    },
                ]
                .into_boxed_slice(),
                formal_count: 0,
                external_expressions: Box::new([]),
                result: KernelExpressionId(1),
            })
            .unwrap()
            .solve()
            .unwrap()
            .definition
            .result
            .ty
        };
        let expected = |direction: &str, kind: &str| {
            Type::object(ObjectShape::from_ordered_fields(
                [
                    (
                        "direction".to_owned(),
                        Type::VariantSet(vec![Variant::Tag(direction.to_owned())].into()),
                    ),
                    (
                        "kind".to_owned(),
                        Type::VariantSet(vec![Variant::Tag(kind.to_owned())].into()),
                    ),
                ],
                false,
            ))
        };
        assert_eq!(solve("Row"), expected("Row", "Row"));
        assert_eq!(solve("Column"), expected("Column", "Stack"));
    }

    #[test]
    fn record_spreads_overlay_fields_in_authored_order() {
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "family".into(),
                                spread: false,
                            },
                            0,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "size".into(),
                                spread: false,
                            },
                            1,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "base".into(),
                                spread: true,
                            },
                            2,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "family".into(),
                                spread: false,
                            },
                            1,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "color".into(),
                                spread: false,
                            },
                            3,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(4),
        };
        let result = compile_owner_program(&input)
            .unwrap()
            .solve()
            .unwrap()
            .definition
            .result;
        assert_eq!(
            result.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("family".to_owned(), Type::Number),
                    ("size".to_owned(), Type::Number),
                    ("color".to_owned(), Type::Text),
                ],
                false,
            ))
        );
    }

    #[test]
    fn callable_formals_publish_definition_local_builtin_requirements() {
        let input = KernelProjectProgramInput {
            owners: vec![KernelOwnerProgramInput {
                nodes: vec![
                    KernelOwnerNode {
                        kind: KernelOwnerNodeKind::FormalRead {
                            formal: 0,
                            fields: Box::new([]),
                        },
                        inputs: Box::new([]),
                        mode: FlowMode::Continuous,
                    },
                    KernelOwnerNode {
                        kind: KernelOwnerNodeKind::PureBuiltin {
                            kind: KernelPureBuiltinKind::TextLength,
                        },
                        inputs: vec![edge(
                            KernelOwnerEdgeRole::AbiArgument {
                                name: "$pipe".into(),
                            },
                            0,
                        )]
                        .into_boxed_slice(),
                        mode: FlowMode::Continuous,
                    },
                ]
                .into_boxed_slice(),
                formal_count: 1,
                external_expressions: Box::new([]),
                result: KernelExpressionId(1),
            }]
            .into_boxed_slice(),
        };

        let artifact = compile_project_program(&input).unwrap().solve().unwrap();
        assert_eq!(
            artifact.definitions[0].formals.as_ref(),
            [FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Text,
            }]
        );
        assert_eq!(
            artifact.definitions[0].result,
            FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Number,
            }
        );
    }

    #[test]
    fn direct_result_summaries_forward_callee_requirements_to_open_caller_formals() {
        let requiring_callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::PureBuiltin {
                        kind: KernelPureBuiltinKind::TextLength,
                    },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::AbiArgument {
                            name: "$pipe".into(),
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let wrapper = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![requiring_callee, wrapper].into_boxed_slice(),
        })
        .unwrap();
        assert!(program.compile_work().direct_result_summaries > 0);
        let artifact = program.solve().unwrap();
        let text_formal = FlowType {
            mode: FlowMode::Continuous,
            ty: Type::Text,
        };
        assert_eq!(
            artifact.definitions[0].formals.as_ref(),
            [text_formal.clone()]
        );
        assert_eq!(artifact.definitions[1].formals.as_ref(), [text_formal]);
        assert_eq!(
            artifact.definitions[1].result,
            FlowType {
                mode: FlowMode::Continuous,
                ty: Type::Number,
            }
        );
    }

    #[test]
    fn diagnostics_are_projected_from_solved_call_providers_without_definition_rows() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::PureBuiltin {
                        kind: KernelPureBuiltinKind::TextLength,
                    },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::AbiArgument {
                            name: "$pipe".into(),
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };

        let solved = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap()
        .solve_graph()
        .unwrap();
        let interfaces = solved.interface_snapshot();
        let [diagnostic] = interfaces.diagnostics.as_ref() else {
            panic!(
                "diagnostics-only projection must emit one typed call failure: {:#?}",
                interfaces.diagnostics
            )
        };
        assert_eq!(diagnostic.owner, KernelOwnerId(1));
        assert_eq!(diagnostic.severity, KernelDiagnosticSeverity::Error);
        assert_eq!(
            diagnostic.site,
            KernelDiagnosticSite::CallInput {
                call: KernelExpressionId(1),
                target: KernelOwnerId(0),
                formal_ordinal: 0,
            }
        );
        assert_eq!(
            diagnostic.kind,
            KernelDiagnosticKind::CallInputType {
                actual: Type::Number,
                expected: Type::Text,
                mismatch: KernelTypeMismatch::Type,
            }
        );

        let checked = solved.checked_snapshot().unwrap();
        assert!(checked.definitions[0].diagnostics.is_empty());
        assert_eq!(
            checked.definitions[1].diagnostics.as_ref(),
            [diagnostic.clone()]
        );
    }

    #[test]
    fn syntax_discriminated_formals_do_not_become_conjunctive_call_contracts() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: vec!["a".into()].into_boxed_slice(),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::PureBuiltin {
                        kind: KernelPureBuiltinKind::TextLength,
                    },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::AbiArgument {
                            name: "$pipe".into(),
                        },
                        1,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Tag {
                            name: "A".into(),
                            fields: Box::new([]),
                        },
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 2)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: vec!["b".into()].into_boxed_slice(),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::PureBuiltin {
                        kind: KernelPureBuiltinKind::NumberToText,
                    },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::AbiArgument {
                            name: "$pipe".into(),
                        },
                        4,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Tag {
                            name: "B".into(),
                            fields: Box::new([]),
                        },
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 5)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::When,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::WhenInput, 0),
                        edge(KernelOwnerEdgeRole::WhenArm, 3),
                        edge(KernelOwnerEdgeRole::WhenArm, 6),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(7),
        };
        let wrapper = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: vec!["value".into()].into_boxed_slice(),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let caller = |target, wrap| {
            let mut nodes = vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record {
                        tag: Some("A".into()),
                    },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "a".into(),
                            spread: false,
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record {
                        tag: Some("B".into()),
                    },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "b".into(),
                            spread: false,
                        },
                        2,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Latest,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::LatestBranch, 1),
                        edge(KernelOwnerEdgeRole::LatestBranch, 3),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ];
            let argument = if wrap {
                nodes.push(KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "value".into(),
                            spread: false,
                        },
                        4,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                });
                5
            } else {
                4
            };
            nodes.push(KernelOwnerNode {
                kind: KernelOwnerNodeKind::UserCall {
                    target: KernelOwnerId(target),
                    inherited_formal: None,
                },
                inputs: vec![edge(
                    KernelOwnerEdgeRole::CallArgument { ordinal: 0 },
                    argument,
                )]
                .into_boxed_slice(),
                mode: FlowMode::Continuous,
            });
            KernelOwnerProgramInput {
                result: KernelExpressionId(
                    u32::try_from(nodes.len() - 1).expect("test node count fits u32"),
                ),
                nodes: nodes.into_boxed_slice(),
                formal_count: 0,
                external_expressions: Box::new([]),
            }
        };

        let program = KernelProjectProgramInput {
            owners: vec![callee, wrapper, caller(0, false), caller(1, true)].into_boxed_slice(),
        };
        let discriminated = project_syntax_discriminated_formals(&program);
        assert_eq!(discriminated[0].as_ref(), [0]);
        assert_eq!(
            discriminated[1].as_ref(),
            [0],
            "a wrapper formal must inherit the callee's branch-dependent contract"
        );
        assert!(discriminated[2].is_empty());
        assert!(discriminated[3].is_empty());

        let interfaces = compile_project_program(&program)
            .unwrap()
            .solve_interfaces()
            .unwrap();
        assert!(
            interfaces.diagnostics.is_empty(),
            "mutually exclusive field requirements must remain conditional through direct and wrapper calls: {:#?}",
            interfaces.diagnostics
        );
    }

    #[test]
    fn diagnostics_project_only_explicit_value_demands_without_expression_rows() {
        let owner = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::Number,
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };
        let facts = KernelDefinitionFactsInput {
            diagnostic_values: vec![KernelExpressionId(0)].into_boxed_slice(),
            ..KernelDefinitionFactsInput::default()
        };
        let solved = compile_project_program_with_definition_facts(
            &KernelProjectProgramInput {
                owners: vec![owner].into_boxed_slice(),
            },
            &[facts],
        )
        .unwrap()
        .solve_graph()
        .unwrap();

        let interfaces = solved.interface_snapshot();
        let [value] = interfaces.diagnostic_values.as_ref() else {
            panic!("one sparse diagnostic value must be projected")
        };
        assert_eq!(value.owner, KernelOwnerId(0));
        assert_eq!(value.ordinal, 0);
        assert_eq!(
            value.value,
            KernelValueReference::Local(KernelExpressionId(0))
        );
        assert_eq!(value.ty, Type::Number);
        assert!(interfaces.diagnostics.is_empty());

        let checked = solved.checked_snapshot().unwrap();
        assert_eq!(checked.diagnostic_values.as_ref(), [value.clone()]);
        assert_eq!(checked.definitions.len(), 1);
    }

    #[test]
    fn generic_calls_instantiate_formals_before_diagnostic_comparison() {
        let identity = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::FormalRead {
                    formal: 0,
                    fields: Box::new([]),
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };

        let interfaces = compile_project_program(&KernelProjectProgramInput {
            owners: vec![identity, caller].into_boxed_slice(),
        })
        .unwrap()
        .solve_interfaces()
        .unwrap();
        assert!(
            interfaces.diagnostics.is_empty(),
            "a concrete generic occurrence satisfies its instantiated formal: {:#?}",
            interfaces.diagnostics
        );
    }

    #[test]
    fn wildcard_when_keeps_the_callable_selector_open() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Tag {
                            name: "Known".into(),
                            fields: Box::new([]),
                        },
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 1)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Wildcard,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 3)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::When,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::WhenInput, 0),
                        edge(KernelOwnerEdgeRole::WhenArm, 2),
                        edge(KernelOwnerEdgeRole::WhenArm, 4),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(5),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Other".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };

        let artifact = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap()
        .solve()
        .unwrap();
        assert!(matches!(
            artifact.definitions[0].formals[0].ty,
            Type::Var(_)
        ));
        assert_eq!(artifact.definitions[1].result.ty, Type::Text);
        assert!(artifact.definitions[1].diagnostics.is_empty());
    }

    #[test]
    fn closed_when_preserves_bare_tags_and_projects_tagged_payloads() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::PatternRead {
                        pattern: KernelPattern::Tag {
                            name: "Material".into(),
                            fields: vec!["of".into()].into_boxed_slice(),
                        },
                        fields: vec!["of".into()].into_boxed_slice(),
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 0)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Tag {
                            name: "Material".into(),
                            fields: vec!["of".into()].into_boxed_slice(),
                        },
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 1)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Tag {
                            name: "Lights".into(),
                            fields: Box::new([]),
                        },
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 3)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::When,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::WhenInput, 0),
                        edge(KernelOwnerEdgeRole::WhenArm, 2),
                        edge(KernelOwnerEdgeRole::WhenArm, 4),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(5),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record {
                        tag: Some("Material".into()),
                    },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "of".into(),
                            spread: false,
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 1)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Lights".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 3)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "material".into(),
                                spread: false,
                            },
                            2,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "lights".into(),
                                spread: false,
                            },
                            4,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(5),
        };

        let artifact = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap()
        .solve()
        .unwrap();
        let Type::VariantSet(formal) = &artifact.definitions[0].formals[0].ty else {
            panic!("closed WHEN formal must be a variant set")
        };
        assert!(
            formal
                .iter()
                .any(|variant| matches!(variant, Variant::Tag(tag) if tag == "Lights"))
        );
        assert!(formal.iter().any(|variant| matches!(variant, Variant::Tagged { tag, fields } if tag == "Material" && fields.fields.contains_key("of"))));
        let Type::Object(result) = &artifact.definitions[1].result.ty else {
            panic!("caller result must be an object")
        };
        assert_eq!(result.fields["material"], Type::Text);
        assert_eq!(result.fields["lights"], Type::Number);
        assert!(artifact.definitions[1].diagnostics.is_empty());
    }

    #[test]
    fn pattern_bindings_are_continuous_values_inside_event_arms() {
        let input = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Source(Type::Text),
                    inputs: Box::new([]),
                    mode: FlowMode::PresentOrAbsent,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::PatternRead {
                        pattern: KernelPattern::Binding {
                            name: "value".into(),
                        },
                        fields: Box::new([]),
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 0)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };

        let artifact = compile_owner_program(&input).unwrap().solve().unwrap();
        assert_eq!(artifact.definition.result.ty, Type::Text);
        assert_eq!(artifact.definition.result.mode, FlowMode::Continuous);
    }

    #[test]
    fn call_diagnostics_retain_the_exact_missing_structural_field() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: vec!["required".into()].into_boxed_slice(),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::PureBuiltin {
                        kind: KernelPureBuiltinKind::TextLength,
                    },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::AbiArgument {
                            name: "$pipe".into(),
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "other".into(),
                            spread: false,
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 1)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };

        let interfaces = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap()
        .solve_interfaces()
        .unwrap();
        let [diagnostic] = interfaces.diagnostics.as_ref() else {
            panic!("missing-field call must emit one diagnostic")
        };
        assert!(matches!(
            &diagnostic.kind,
            KernelDiagnosticKind::CallInputType {
                mismatch: KernelTypeMismatch::MissingField(field),
                ..
            } if field.as_ref() == "required"
        ));
    }

    #[test]
    fn acyclic_user_calls_compose_fresh_formal_frames_into_one_component() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::FormalRead {
                    formal: 0,
                    fields: Box::new([]),
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 1)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "number".into(),
                                spread: false,
                            },
                            2,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "text".into(),
                                spread: false,
                            },
                            3,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(4),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap();
        assert_eq!(program.compile_work().direct_result_summaries, 2);
        assert_eq!(
            program.compile_work().invocation_frames,
            0,
            "direct formal-result summaries must not allocate callee frames",
        );
        let artifact = program.solve().unwrap();
        assert_eq!(
            artifact.definitions[1].result.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("number".to_owned(), Type::Number),
                    ("text".to_owned(), Type::Text),
                ],
                false,
            )),
            "repeated calls must not share or specialize one formal frame"
        );
        assert_eq!(
            artifact.definitions[1].expressions[2].flow_type.ty,
            Type::Number
        );
        assert_eq!(
            artifact.definitions[1].expressions[3].flow_type.ty,
            Type::Text
        );
        let [number_call, text_call] = artifact.definitions[1].calls.as_ref() else {
            panic!("caller definition must publish both call occurrences")
        };
        assert_eq!(number_call.expression, KernelExpressionId(2));
        assert_eq!(text_call.expression, KernelExpressionId(3));
        assert_eq!(
            number_call.target,
            KernelCallTarget::User {
                target: KernelOwnerId(0),
                inherited_formal: None,
            }
        );
        assert_eq!(
            number_call.inputs.as_ref(),
            [KernelCallInputArtifact {
                role: KernelCallInputRole::Formal { ordinal: 0 },
                value: KernelCallValueReference::Local(KernelExpressionId(0)),
            }]
        );
        assert_eq!(number_call.result.ty, Type::Number);
        assert_eq!(text_call.result.ty, Type::Text);
        assert_eq!(
            number_call.type_substitutions.as_ref(),
            [KernelCallTypeSubstitution {
                variable: KernelTypeParameterId(0),
                value: Type::Number,
            }]
        );
        assert_eq!(
            text_call.type_substitutions.as_ref(),
            [KernelCallTypeSubstitution {
                variable: KernelTypeParameterId(0),
                value: Type::Text,
            }]
        );
        assert_eq!(artifact.definitions[0].formals.len(), 1);
        assert!(matches!(
            artifact.definitions[0].formals[0].ty,
            Type::Var(_)
        ));
        let number_expression = &artifact.definitions[1].expressions[2];
        assert_eq!(number_expression.id, KernelExpressionId(2));
        assert_eq!(
            number_expression.kind,
            KernelOwnerNodeKind::UserCall {
                target: KernelOwnerId(0),
                inherited_formal: None,
            }
        );
        assert_eq!(
            number_expression.inputs.as_ref(),
            [KernelExpressionInputArtifact {
                role: KernelOwnerEdgeRole::CallArgument { ordinal: 0 },
                value: KernelValueReference::Local(KernelExpressionId(0)),
            }]
        );
        assert_eq!(number_expression.flow_type, number_call.result);
        assert!(artifact.definitions[0].calls.is_empty());
    }

    #[test]
    fn structural_result_summary_inlines_trivial_nested_bytecode() {
        let identity = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::FormalRead {
                    formal: 0,
                    fields: Box::new([]),
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };
        let wrapper = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "value".into(),
                            spread: false,
                        },
                        1,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(1),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(1),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 1)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "number".into(),
                                spread: false,
                            },
                            2,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "text".into(),
                                spread: false,
                            },
                            3,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(4),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![identity, wrapper, caller].into_boxed_slice(),
        })
        .unwrap();
        assert_eq!(program.compile_work().invocation_frames, 0);
        assert_eq!(
            program.compile_work().direct_result_summaries,
            3,
            "the wrapper principal plus both caller occurrences use summaries",
        );
        let summary_programs = program
            .component()
            .operations
            .iter()
            .filter_map(|operation| match operation.as_ref() {
                crate::KernelOperation::SummaryCall { program, .. } => Some(program),
                _ => None,
            })
            .collect::<Vec<_>>();
        let [number_summary, text_summary] = summary_programs.as_slice() else {
            panic!("two caller occurrences must use the compiled wrapper summary")
        };
        assert!(
            Arc::ptr_eq(number_summary, text_summary),
            "compatible calls must share one immutable result-summary program",
        );
        assert!(
            number_summary
                .nodes
                .iter()
                .all(|node| !matches!(node, KernelSummaryNode::Invoke { .. })),
            "a one-node identity summary must stay inline",
        );
        let artifact = program.solve().unwrap();
        let Type::Object(result) = &artifact.definitions[2].result.ty else {
            panic!("caller result must be a record")
        };
        let Type::Object(number) = &result.fields["number"] else {
            panic!("number result must be a record")
        };
        let Type::Object(text) = &result.fields["text"] else {
            panic!("text result must be a record")
        };
        assert_eq!(number.fields["value"], Type::Number);
        assert_eq!(text.fields["value"], Type::Text);
    }

    #[test]
    fn structural_result_summary_shares_identical_formal_projection_inputs() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "left".into(),
                                spread: false,
                            },
                            0,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "right".into(),
                                spread: false,
                            },
                            1,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap();
        let calls = program
            .component()
            .operations
            .iter()
            .filter_map(|operation| match operation.as_ref() {
                crate::KernelOperation::SummaryCall {
                    program, inputs, ..
                } if program.definition == 0 => Some((program, inputs)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(!calls.is_empty(), "the callee must use its direct summary");
        for (summary, inputs) in calls {
            assert_eq!(
                summary
                    .nodes
                    .iter()
                    .filter(|node| matches!(node, KernelSummaryNode::Input(_)))
                    .count(),
                1,
                "one formal path is one immutable summary value",
            );
            assert_eq!(
                inputs.len(),
                1,
                "one formal path allocates one occurrence-local projection equation",
            );
        }

        let artifact = program.solve().unwrap();
        let Type::Object(result) = &artifact.definitions[1].result.ty else {
            panic!("caller result must be a record")
        };
        assert_eq!(result.fields["left"], Type::Number);
        assert_eq!(result.fields["right"], Type::Number);
    }

    #[test]
    fn structural_result_summary_invokes_large_shared_nested_bytecode() {
        let field_count = SHARED_SUMMARY_MIN_NODES - 1;
        let mut callee_nodes = (0..field_count)
            .map(|_| KernelOwnerNode {
                kind: KernelOwnerNodeKind::Number,
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            })
            .collect::<Vec<_>>();
        callee_nodes.push(KernelOwnerNode {
            kind: KernelOwnerNodeKind::Record { tag: None },
            inputs: (0..field_count)
                .map(|index| {
                    edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: format!("value_{index}").into(),
                            spread: false,
                        },
                        u32::try_from(index).expect("test field index exceeds u32"),
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            mode: FlowMode::Continuous,
        });
        let callee = KernelOwnerProgramInput {
            nodes: callee_nodes.into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(field_count as u32),
        };
        let wrapper = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::UserCall {
                    target: KernelOwnerId(0),
                    inherited_formal: None,
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::UserCall {
                    target: KernelOwnerId(1),
                    inherited_formal: None,
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, wrapper, caller].into_boxed_slice(),
        })
        .unwrap();
        assert!(program.compile_work().summary_invoke_nodes >= 1);
        assert!(
            program.compile_work().summary_constant_folded_nodes >= 1,
            "the definition-owned constant record must fold before any call occurrence"
        );
        assert!(
            program.compile_work().summary_pruned_nodes >= 1,
            "folded summary children must not survive as dead bytecode"
        );

        let artifact = program.solve().unwrap();
        let Type::Object(result) = &artifact.definitions[2].result.ty else {
            panic!("caller result must be the shared record")
        };
        assert_eq!(result.fields.len(), field_count);
        assert!(result.fields.values().all(|field| *field == Type::Number));
    }

    #[test]
    fn definition_constant_summary_records_fold_to_one_type_term() {
        let mut builder = ComponentProgramBuilder::new();
        let number = builder.terms().number();
        let value = builder.terms_mut().intern_name("value");
        let mut nodes = vec![
            KernelSummaryNode::Term(number),
            KernelSummaryNode::Record {
                tag: None,
                entries: vec![KernelSummaryRecordEntry::Field {
                    name: value,
                    value: KernelSummaryValueId(0),
                }]
                .into_boxed_slice(),
            },
        ];

        assert_eq!(
            fold_constant_summary_nodes(&mut builder, &mut nodes),
            (1, 0)
        );
        let KernelSummaryNode::Term(record) = nodes[1] else {
            panic!("closed summary record was not partial-evaluated")
        };
        assert_eq!(
            builder.terms().export_checked_type(record),
            Type::object(ObjectShape::from_ordered_fields(
                [("value".to_owned(), Type::Number)],
                false,
            ))
        );
    }

    #[test]
    fn definition_summary_cse_shares_pure_algebra_but_not_requirements() {
        let mut builder = ComponentProgramBuilder::new();
        let number = builder.terms().number();
        let value_name = builder.terms_mut().intern_name("value");
        let mut nodes = vec![
            KernelSummaryNode::Input(0),
            KernelSummaryNode::Input(0),
            KernelSummaryNode::Term(number),
            KernelSummaryNode::Term(number),
            KernelSummaryNode::Record {
                tag: None,
                entries: vec![KernelSummaryRecordEntry::Field {
                    name: value_name,
                    value: KernelSummaryValueId(2),
                }]
                .into_boxed_slice(),
            },
            KernelSummaryNode::Record {
                tag: None,
                entries: vec![KernelSummaryRecordEntry::Field {
                    name: value_name,
                    value: KernelSummaryValueId(3),
                }]
                .into_boxed_slice(),
            },
            KernelSummaryNode::Constrain {
                value: KernelSummaryValueId(4),
                expected: number,
            },
            KernelSummaryNode::Constrain {
                value: KernelSummaryValueId(5),
                expected: number,
            },
            KernelSummaryNode::Sequence {
                inputs: vec![KernelSummaryValueId(6), KernelSummaryValueId(7)].into_boxed_slice(),
                result: KernelSummaryValueId(5),
            },
        ];
        let mut result = PlannedSummaryValue {
            value: KernelSummaryValueId(8),
            mode: DirectSummaryMode::Fixed {
                owner: KernelOwnerId(0),
                expression: 0,
            },
            formal_projection_input: None,
        };

        assert_eq!(deduplicate_summary_nodes(&mut nodes, &mut result), 3);
        assert_eq!(result.value, KernelSummaryValueId(5));
        assert_eq!(
            nodes
                .iter()
                .filter(|node| matches!(node, KernelSummaryNode::Constrain { .. }))
                .count(),
            2,
            "requirement publications must retain distinct authored nodes"
        );
        let KernelSummaryNode::Sequence {
            inputs,
            result: sequence_result,
        } = &nodes[result.value.0 as usize]
        else {
            panic!("summary result must remain the ordered requirement sequence")
        };
        assert_eq!(inputs.len(), 2);
        assert_eq!(*sequence_result, KernelSummaryValueId(2));
    }

    #[test]
    fn sibling_record_field_selectors_fuse_into_one_definition_decision() {
        let mut builder = ComponentProgramBuilder::new();
        let selector = builder.new_authoritative_provider();
        let output = builder.new_authoritative_provider();
        let number = builder.terms().number();
        let text = builder.terms().text();
        let value_name = builder.terms_mut().intern_name("value");
        let fixed_name = builder.terms_mut().intern_name("fixed");
        let mut nodes = vec![
            KernelSummaryNode::Input(0),
            KernelSummaryNode::Term(number),
            KernelSummaryNode::Term(text),
            KernelSummaryNode::Select {
                selector: KernelSummaryValueId(0),
                arms: vec![
                    KernelSummarySelectArm {
                        pattern: KernelPattern::Tag {
                            name: "Dark".into(),
                            fields: Box::new([]),
                        },
                        output: KernelSummaryValueId(1),
                    },
                    KernelSummarySelectArm {
                        pattern: KernelPattern::Tag {
                            name: "Light".into(),
                            fields: Box::new([]),
                        },
                        output: KernelSummaryValueId(2),
                    },
                ]
                .into_boxed_slice(),
            },
            KernelSummaryNode::Record {
                tag: None,
                entries: vec![
                    KernelSummaryRecordEntry::Field {
                        name: value_name,
                        value: KernelSummaryValueId(3),
                    },
                    KernelSummaryRecordEntry::Field {
                        name: fixed_name,
                        value: KernelSummaryValueId(1),
                    },
                ]
                .into_boxed_slice(),
            },
        ];

        assert_eq!(
            fold_constant_summary_nodes(&mut builder, &mut nodes),
            (0, 1)
        );
        assert!(matches!(
            &nodes[4],
            KernelSummaryNode::Select { arms, .. }
                if arms.len() == 2
                    && arms.iter().all(|arm| matches!(
                        &nodes[arm.output.0 as usize],
                        KernelSummaryNode::Term(_)
                    ))
        ));

        let dark = builder.terms_mut().variant_tag("Dark");
        let dark = builder.terms_mut().variant_set([dark]);
        builder.add_publish(selector, [dark], PublishMode::Replace);
        let selector_term = builder.variable_term(selector);
        builder.add_summary_call(
            output,
            Arc::new(KernelSummaryProgram {
                definition: 0,
                nodes: nodes.into_boxed_slice(),
                result: KernelSummaryValueId(4),
            }),
            [selector_term],
        );
        let result = builder.add_output(output, FlowMode::Continuous);
        let artifact = solve_component(builder.finish()).expect("fused summary solves");
        assert_eq!(
            artifact.output(result).unwrap().flow_type.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [
                    ("value".to_owned(), Type::Number),
                    ("fixed".to_owned(), Type::Number),
                ],
                false,
            ))
        );
    }

    #[test]
    fn structural_result_summary_composes_nested_formal_projections() {
        let projector = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::FormalRead {
                    formal: 0,
                    fields: vec!["value".into()].into_boxed_slice(),
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(0),
        };
        let wrapper = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "value".into(),
                            spread: false,
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(1),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 1)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![projector, wrapper, caller].into_boxed_slice(),
        })
        .unwrap();
        assert_eq!(
            program.compile_work().invocation_frames,
            0,
            "a nested projection of a forwarded formal must stay in summary bytecode",
        );
        let summary_inputs = program
            .component()
            .operations
            .iter()
            .find_map(|operation| match operation.as_ref() {
                crate::KernelOperation::SummaryCall { inputs, .. }
                    if inputs.iter().any(|input| {
                        matches!(
                            input,
                            KernelSummaryCallInput::Projection { steps, .. }
                                if steps.iter().any(|step| step.field.is_some())
                        )
                    }) =>
                {
                    Some(inputs)
                }
                _ => None,
            })
            .expect("the wrapper call must use a composed formal projection input");
        assert_eq!(
            summary_inputs.len(),
            1,
            "the composed projection must replace the now-dead whole-formal input"
        );
        assert!(
            program.compile_work().summary_pruned_inputs >= 1,
            "summary compaction must report removing the redundant formal input"
        );
        assert_eq!(
            program.solve().unwrap().definitions[2].result.ty,
            Type::Number
        );
    }

    #[test]
    fn structural_result_summary_projects_a_computed_value() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "value".into(),
                            spread: false,
                        },
                        0,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::DerivedRead {
                        fields: vec!["value".into()].into_boxed_slice(),
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 1)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap();
        assert_eq!(program.compile_work().invocation_frames, 0);
        assert!(program.component().operations.iter().any(|operation| {
            matches!(
                operation.as_ref(),
                crate::KernelOperation::SummaryCall { program, .. }
                    if program.nodes.iter().any(|node| {
                        matches!(node, KernelSummaryNode::Projection { .. })
                    })
            )
        }));
        assert_eq!(
            program.solve().unwrap().definitions[1].result.ty,
            Type::Number
        );
    }

    #[test]
    fn formal_independent_calls_share_the_compiled_principal_residual() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(1),
        };
        let mut caller_nodes = vec![KernelOwnerNode {
            kind: KernelOwnerNodeKind::Text,
            inputs: Box::new([]),
            mode: FlowMode::Continuous,
        }];
        let mut items = Vec::new();
        for _ in 0..32 {
            let call = u32::try_from(caller_nodes.len()).unwrap();
            caller_nodes.push(KernelOwnerNode {
                kind: KernelOwnerNodeKind::UserCall {
                    target: KernelOwnerId(0),
                    inherited_formal: None,
                },
                inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                    .into_boxed_slice(),
                mode: FlowMode::Continuous,
            });
            items.push(edge(KernelOwnerEdgeRole::CollectionItem, call));
        }
        let result = u32::try_from(caller_nodes.len()).unwrap();
        caller_nodes.push(KernelOwnerNode {
            kind: KernelOwnerNodeKind::Collection {
                kind: KernelCollectionKind::List,
                capacity: None,
            },
            inputs: items.into_boxed_slice(),
            mode: FlowMode::Continuous,
        });
        let caller = KernelOwnerProgramInput {
            nodes: caller_nodes.into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(result),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap();
        assert!(
            program.component().operation_count() < 80,
            "formal-independent calls must not clone their callee: {} operations",
            program.component().operation_count()
        );
        let artifact = program.solve().unwrap();
        assert_eq!(
            artifact.definitions[1].result.ty,
            Type::List(Type::shared(Type::Number))
        );
    }

    #[test]
    fn formal_independent_subexpressions_share_principal_cells_across_calls() {
        let callee = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "value".into(),
                                spread: false,
                            },
                            0,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "constant".into(),
                                spread: false,
                            },
                            1,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Number,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 1)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "number".into(),
                                spread: false,
                            },
                            2,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "text".into(),
                                spread: false,
                            },
                            3,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(4),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap();
        assert_eq!(program.compile_work().direct_result_summaries, 2);
        assert_eq!(program.compile_work().invocation_frames, 0);
        assert!(
            program.component().scheduled_work_item_count() < program.component().operation_count(),
            "acyclic residual instructions must execute through compact frame work items: {} scheduled for {} instructions",
            program.component().scheduled_work_item_count(),
            program.component().operation_count(),
        );
        let artifact = program.solve().unwrap();
        let Type::Object(result) = &artifact.definitions[1].result.ty else {
            panic!("caller result must be a record")
        };
        let Type::Object(number) = &result.fields["number"] else {
            panic!("number call result must be a record")
        };
        let Type::Object(text) = &result.fields["text"] else {
            panic!("text call result must be a record")
        };
        assert_eq!(number.fields["value"], Type::Number);
        assert_eq!(text.fields["value"], Type::Text);
        assert_eq!(number.fields["constant"], Type::Number);
        assert_eq!(text.fields["constant"], Type::Number);
    }

    #[test]
    fn static_call_selectors_slice_unreachable_residual_arms() {
        let mut callee_nodes = vec![KernelOwnerNode {
            kind: KernelOwnerNodeKind::FormalRead {
                formal: 0,
                fields: Box::new([]),
            },
            inputs: Box::new([]),
            mode: FlowMode::Continuous,
        }];
        let mut arms = Vec::new();
        for ordinal in 0..8 {
            let output = u32::try_from(callee_nodes.len()).unwrap();
            callee_nodes.push(KernelOwnerNode {
                kind: KernelOwnerNodeKind::FormalRead {
                    formal: 1,
                    fields: Box::new([]),
                },
                inputs: Box::new([]),
                mode: FlowMode::Continuous,
            });
            let arm = u32::try_from(callee_nodes.len()).unwrap();
            callee_nodes.push(KernelOwnerNode {
                kind: KernelOwnerNodeKind::MatchArm {
                    pattern: KernelPattern::Tag {
                        name: format!("Arm{ordinal}").into_boxed_str(),
                        fields: Box::new([]),
                    },
                },
                inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, output)].into_boxed_slice(),
                mode: FlowMode::Continuous,
            });
            arms.push(edge(KernelOwnerEdgeRole::WhenArm, arm));
        }
        let result = u32::try_from(callee_nodes.len()).unwrap();
        callee_nodes.push(KernelOwnerNode {
            kind: KernelOwnerNodeKind::When,
            inputs: std::iter::once(edge(KernelOwnerEdgeRole::WhenInput, 0))
                .chain(arms)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            mode: FlowMode::Continuous,
        });
        let callee = KernelOwnerProgramInput {
            nodes: callee_nodes.into_boxed_slice(),
            formal_count: 2,
            external_expressions: Box::new([]),
            result: KernelExpressionId(result),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Arm3".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Text,
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0),
                        edge(KernelOwnerEdgeRole::CallArgument { ordinal: 1 }, 1),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![callee, caller].into_boxed_slice(),
        })
        .unwrap();
        assert!(
            program.component().operation_count() < 40,
            "one static call must not clone all eight residual arms: {} operations",
            program.component().operation_count()
        );
        assert_eq!(
            program.compile_work().invocation_frames,
            0,
            "the definition-owned SELECT summary replaces the complete occurrence frame",
        );
        assert!(program.component().operations.iter().any(|operation| {
            matches!(
                operation.as_ref(),
                crate::KernelOperation::SummaryCall { program, .. }
                    if program.nodes.iter().any(|node| matches!(node, KernelSummaryNode::Select { .. }))
            )
        }));
        let artifact = program.solve().unwrap();
        assert_eq!(artifact.definitions[1].result.ty, Type::Text);
    }

    #[test]
    fn project_program_propagates_a_child_owner_expression_without_reconstruction() {
        let input = KernelProjectProgramInput {
            owners: vec![
                KernelOwnerProgramInput {
                    nodes: vec![KernelOwnerNode {
                        kind: KernelOwnerNodeKind::Tag("Ready".into()),
                        inputs: Box::new([]),
                        mode: FlowMode::Continuous,
                    }]
                    .into_boxed_slice(),
                    formal_count: 0,
                    external_expressions: Box::new([]),
                    result: KernelExpressionId(0),
                },
                KernelOwnerProgramInput {
                    nodes: vec![KernelOwnerNode {
                        kind: KernelOwnerNodeKind::Record { tag: None },
                        inputs: vec![edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "status".into(),
                                spread: false,
                            },
                            1,
                        )]
                        .into_boxed_slice(),
                        mode: FlowMode::Continuous,
                    }]
                    .into_boxed_slice(),
                    formal_count: 0,
                    external_expressions: vec![KernelExternalExpression {
                        owner: KernelOwnerId(0),
                        target: KernelExternalTarget::Result,
                    }]
                    .into_boxed_slice(),
                    result: KernelExpressionId(0),
                },
            ]
            .into_boxed_slice(),
        };

        let artifact = compile_project_program(&input).unwrap().solve().unwrap();
        assert_eq!(artifact.definitions.len(), 2);
        assert_eq!(
            artifact.definitions[1].result.ty,
            Type::object(ObjectShape::from_ordered_fields(
                [(
                    "status".to_owned(),
                    Type::VariantSet(vec![Variant::Tag("Ready".to_owned())].into()),
                )],
                false,
            ))
        );
        assert!(artifact.work.activations < 16);
    }

    #[test]
    fn stateful_calls_use_fresh_occurrences_without_losing_the_state_domain() {
        let stateful = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Closed".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Open".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Hold,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::HoldInitial, 0),
                        edge(KernelOwnerEdgeRole::HoldUpdate, 1),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "state".into(),
                            spread: false,
                        },
                        2,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(3),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "first".into(),
                                spread: false,
                            },
                            0,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "second".into(),
                                spread: false,
                            },
                            1,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(2),
        };

        let program = compile_project_program(&KernelProjectProgramInput {
            owners: vec![stateful, caller].into_boxed_slice(),
        })
        .unwrap();
        assert_eq!(program.compile_work().invocation_frames, 2);
        assert_eq!(program.compile_work().reused_invocation_frames, 0);
        let artifact = program.solve().unwrap();
        let definition = &artifact.definitions[0].result.ty;
        let first_call = &artifact.definitions[1].expressions[0].flow_type.ty;
        let second_call = &artifact.definitions[1].expressions[1].flow_type.ty;
        let state_type = |ty: &Type| {
            let Type::Object(shape) = ty else {
                panic!("stateful call result is not an object: {ty:?}");
            };
            shape
                .fields
                .get("state")
                .cloned()
                .expect("stateful call result has a state field")
        };
        let definition_state = state_type(definition);
        let first_state = state_type(first_call);
        let second_state = state_type(second_call);
        assert_eq!(
            definition_state,
            Type::VariantSet(
                vec![
                    Variant::Tag("Closed".to_owned()),
                    Variant::Tag("Open".to_owned()),
                ]
                .into(),
            )
        );
        assert_eq!(first_state, definition_state);
        assert_eq!(second_state, first_state);
    }

    #[test]
    fn singleton_syntax_selection_exposes_only_the_nested_state_initializer() {
        let stateful = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Closed".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Open".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Hold,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::HoldInitial, 0),
                        edge(KernelOwnerEdgeRole::HoldUpdate, 1),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![edge(
                        KernelOwnerEdgeRole::RecordField {
                            name: "state".into(),
                            spread: false,
                        },
                        2,
                    )]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(3),
        };
        let wrapper = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::FormalRead {
                        formal: 0,
                        fields: Box::new([]),
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Tag {
                            name: "UseState".into(),
                            fields: Box::new([]),
                        },
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 1)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Fallback".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Wildcard,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 3)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::When,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::WhenInput, 0),
                        edge(KernelOwnerEdgeRole::WhenArm, 2),
                        edge(KernelOwnerEdgeRole::WhenArm, 4),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 1,
            external_expressions: Box::new([]),
            result: KernelExpressionId(5),
        };
        let caller = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("UseState".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(1),
                        inherited_formal: None,
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                        .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::UserCall {
                        target: KernelOwnerId(0),
                        inherited_formal: None,
                    },
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Record { tag: None },
                    inputs: vec![
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "selected".into(),
                                spread: false,
                            },
                            1,
                        ),
                        edge(
                            KernelOwnerEdgeRole::RecordField {
                                name: "direct".into(),
                                spread: false,
                            },
                            2,
                        ),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: Box::new([]),
            result: KernelExpressionId(3),
        };

        let wrapper_dependencies = owner_expressions_depend_on_formals(&wrapper);
        let wrapper_variants =
            infer_static_variants(&wrapper, &[Some(BTreeSet::from(["UseState".into()]))]);
        assert!(
            syntax_selected_call_nodes(&wrapper, &wrapper_variants, &wrapper_dependencies)[1],
            "the nested stateful call must retain syntax-selection provenance",
        );

        let artifact = compile_project_program(&KernelProjectProgramInput {
            owners: vec![stateful, wrapper, caller].into_boxed_slice(),
        })
        .unwrap()
        .solve()
        .unwrap();
        let state_type = |ty: &Type| {
            let Type::Object(shape) = ty else {
                panic!("stateful result is not an object: {ty:?}");
            };
            shape.fields.get("state").cloned().expect("state field")
        };
        let full = Type::VariantSet(
            vec![
                Variant::Tag("Closed".to_owned()),
                Variant::Tag("Open".to_owned()),
            ]
            .into(),
        );
        assert_eq!(state_type(&artifact.definitions[0].result.ty), full);
        assert_eq!(
            state_type(&artifact.definitions[1].expressions[1].flow_type.ty),
            full
        );
        assert_eq!(
            state_type(&artifact.definitions[2].expressions[1].flow_type.ty),
            Type::VariantSet(vec![Variant::Tag("Closed".to_owned())].into()),
        );
        assert_eq!(
            state_type(&artifact.definitions[2].expressions[2].flow_type.ty),
            full
        );
    }

    #[test]
    fn project_result_alias_replays_after_a_cyclic_child_reaches_quiescence() {
        let child = KernelOwnerProgramInput {
            nodes: vec![
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Initial".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Tag("Updated".into()),
                    inputs: Box::new([]),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::MatchArm {
                        pattern: KernelPattern::Tag {
                            name: "Initial".into(),
                            fields: Box::new([]),
                        },
                    },
                    inputs: vec![edge(KernelOwnerEdgeRole::MatchOutput, 1)].into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::When,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::WhenInput, 5),
                        edge(KernelOwnerEdgeRole::WhenArm, 2),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
                KernelOwnerNode {
                    kind: KernelOwnerNodeKind::Hold,
                    inputs: vec![
                        edge(KernelOwnerEdgeRole::HoldInitial, 0),
                        edge(KernelOwnerEdgeRole::HoldUpdate, 3),
                    ]
                    .into_boxed_slice(),
                    mode: FlowMode::Continuous,
                },
            ]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: vec![KernelExternalExpression {
                owner: KernelOwnerId(0),
                target: KernelExternalTarget::Result,
            }]
            .into_boxed_slice(),
            result: KernelExpressionId(4),
        };
        let parent = KernelOwnerProgramInput {
            nodes: vec![KernelOwnerNode {
                kind: KernelOwnerNodeKind::Block,
                inputs: vec![edge(KernelOwnerEdgeRole::BlockResult, 1)].into_boxed_slice(),
                mode: FlowMode::Continuous,
            }]
            .into_boxed_slice(),
            formal_count: 0,
            external_expressions: vec![KernelExternalExpression {
                owner: KernelOwnerId(1),
                target: KernelExternalTarget::Result,
            }]
            .into_boxed_slice(),
            result: KernelExpressionId(0),
        };

        let artifact = compile_project_program(&KernelProjectProgramInput {
            owners: vec![parent, child].into_boxed_slice(),
        })
        .unwrap()
        .solve()
        .unwrap();
        let expected = Type::VariantSet(
            vec![
                Variant::Tag("Initial".to_owned()),
                Variant::Tag("Updated".to_owned()),
            ]
            .into(),
        );
        assert_eq!(artifact.definitions[1].result.ty, expected);
        assert_eq!(
            artifact.definitions[0].result.ty, expected,
            "a cross-owner result alias must observe the child's final epoch"
        );
    }

    #[test]
    fn public_and_exact_reads_project_the_nested_actual_mode() {
        let input = KernelProjectProgramInput {
            owners: vec![
                KernelOwnerProgramInput {
                    nodes: vec![KernelOwnerNode {
                        kind: KernelOwnerNodeKind::FormalRead {
                            formal: 0,
                            fields: Box::new([]),
                        },
                        inputs: Box::new([]),
                        mode: FlowMode::Continuous,
                    }]
                    .into_boxed_slice(),
                    formal_count: 1,
                    external_expressions: Box::new([]),
                    result: KernelExpressionId(0),
                },
                KernelOwnerProgramInput {
                    nodes: vec![
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::Known(Type::Text),
                            inputs: Box::new([]),
                            mode: FlowMode::PresentOrAbsent,
                        },
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::Record { tag: None },
                            inputs: vec![edge(
                                KernelOwnerEdgeRole::RecordField {
                                    name: "event".into(),
                                    spread: false,
                                },
                                0,
                            )]
                            .into_boxed_slice(),
                            mode: FlowMode::Continuous,
                        },
                    ]
                    .into_boxed_slice(),
                    formal_count: 0,
                    external_expressions: Box::new([]),
                    result: KernelExpressionId(1),
                },
                KernelOwnerProgramInput {
                    nodes: vec![
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::ValueRead {
                                fields: vec!["event".into()].into_boxed_slice(),
                                mode_narrowing: None,
                            },
                            inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 2)]
                                .into_boxed_slice(),
                            mode: FlowMode::Continuous,
                        },
                        KernelOwnerNode {
                            kind: KernelOwnerNodeKind::UserCall {
                                target: KernelOwnerId(0),
                                inherited_formal: None,
                            },
                            inputs: vec![edge(KernelOwnerEdgeRole::CallArgument { ordinal: 0 }, 0)]
                                .into_boxed_slice(),
                            mode: FlowMode::Continuous,
                        },
                    ]
                    .into_boxed_slice(),
                    formal_count: 0,
                    external_expressions: vec![KernelExternalExpression {
                        owner: KernelOwnerId(1),
                        target: KernelExternalTarget::Result,
                    }]
                    .into_boxed_slice(),
                    result: KernelExpressionId(1),
                },
                KernelOwnerProgramInput {
                    nodes: vec![KernelOwnerNode {
                        kind: KernelOwnerNodeKind::ValueRead {
                            fields: vec!["event".into()].into_boxed_slice(),
                            mode_narrowing: None,
                        },
                        inputs: vec![edge(KernelOwnerEdgeRole::ReadProvider, 1)].into_boxed_slice(),
                        mode: FlowMode::Continuous,
                    }]
                    .into_boxed_slice(),
                    formal_count: 0,
                    external_expressions: vec![KernelExternalExpression {
                        owner: KernelOwnerId(1),
                        target: KernelExternalTarget::Expression(KernelExpressionId(1)),
                    }]
                    .into_boxed_slice(),
                    result: KernelExpressionId(0),
                },
            ]
            .into_boxed_slice(),
        };

        let artifact = compile_project_program(&input).unwrap().solve().unwrap();
        assert_eq!(artifact.definitions[1].result.mode, FlowMode::Continuous);
        assert_eq!(
            artifact.definitions[2].expressions[0].flow_type.ty,
            Type::Text
        );
        assert_eq!(
            artifact.definitions[2].expressions[0].flow_type.mode,
            FlowMode::PresentOrAbsent,
            "a public-result read preserves the nested event field mode"
        );
        assert_eq!(
            artifact.definitions[2].result.mode,
            FlowMode::PresentOrAbsent,
            "contextual call inference follows the eventful projected actual"
        );
        assert_eq!(
            artifact.definitions[3].result.mode,
            FlowMode::PresentOrAbsent,
            "an exact external expression boundary retains structural mode projection"
        );
    }
}
