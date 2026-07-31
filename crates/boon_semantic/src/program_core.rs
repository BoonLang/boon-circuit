pub use crate::{StaticOwnerDef, StaticOwnerId};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Canonical target-neutral executable core produced by semantic elaboration.
///
/// The core is inspectable and serializable, but compiler backends accept it
/// only after `boon_verify` binds it into the opaque `boon_ir::ErasedProgram`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CanonicalProgramCoreV1 {
    pub executable: ExecutableProgram,
    pub scope_index: ErasedScopeIndex,
    pub expression_count: usize,
    #[serde(default)]
    pub distributed_references: DistributedReferences,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub producer_function_instances: Vec<ProducerFunctionInstance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub debug_source_units: Vec<SemanticSourceUnit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub debug_fields: Vec<SemanticFieldEntry>,
    pub graph_node_count: usize,
    pub sources: Vec<SourcePort>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_ports: Vec<HostPortDeclaration>,
    pub state_cells: Vec<StateCell>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activations: Vec<ActivationSite>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pulse_batches: Vec<PulseBatch>,
    pub lists: Vec<ListMemory>,
    #[serde(default)]
    pub semantic_memory: Vec<SemanticMemory>,
    #[serde(default)]
    pub migration_edges: Vec<MigrationEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transient_collections: Vec<TransientCollection>,
    pub output_values: Vec<OutputRootValue>,
    pub derived_values: Vec<DerivedValue>,
    pub dependencies: Vec<DependencyEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_update_arms: Vec<StateUpdateArm>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_effect_schedules: Vec<HostEffectSchedule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_mutations: Vec<ListMutation>,
    pub list_projections: Vec<ListProjection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materializations: Vec<ContextualMaterialization>,
    pub view_bindings: Vec<ViewBinding>,
    pub expression_types: boon_typecheck::ExprTypeTable,
    pub function_types: boon_typecheck::FunctionTypeTable,
    pub named_value_types: boon_typecheck::NamedValueTypeTable,
}
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransientCollectionKind {
    List,
    Map,
    Set,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TransientMapEntry {
    pub key: ExecutableExprId,
    pub value: ExecutableExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransientCollectionStep {
    ListAppend {
        expression: ExecutableExprId,
        item: ExecutableExprId,
    },
    MapUpsert {
        expression: ExecutableExprId,
        key: ExecutableExprId,
        value: ExecutableExprId,
    },
    MapRemove {
        expression: ExecutableExprId,
        key: ExecutableExprId,
    },
    SetAdd {
        expression: ExecutableExprId,
        item: ExecutableExprId,
    },
    SetRemove {
        expression: ExecutableExprId,
        item: ExecutableExprId,
    },
}

impl TransientCollectionStep {
    pub const fn expression(&self) -> ExecutableExprId {
        match self {
            Self::ListAppend { expression, .. }
            | Self::MapUpsert { expression, .. }
            | Self::MapRemove { expression, .. }
            | Self::SetAdd { expression, .. }
            | Self::SetRemove { expression, .. } => *expression,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransientCollectionResult {
    ListGet {
        expression: ExecutableExprId,
        position: ExecutableExprId,
    },
    ListLength {
        expression: ExecutableExprId,
    },
    ListIsNotEmpty {
        expression: ExecutableExprId,
    },
    MapGet {
        expression: ExecutableExprId,
        key: ExecutableExprId,
    },
    SetContains {
        expression: ExecutableExprId,
        item: ExecutableExprId,
    },
}

impl TransientCollectionResult {
    pub const fn expression(&self) -> ExecutableExprId {
        match self {
            Self::ListGet { expression, .. }
            | Self::ListLength { expression }
            | Self::ListIsNotEmpty { expression }
            | Self::MapGet { expression, .. }
            | Self::SetContains { expression, .. } => *expression,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TransientCollection {
    pub kind: TransientCollectionKind,
    pub constructor: ExecutableExprId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_capacity: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_items: Vec<ExecutableExprId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub map_entries: Vec<TransientMapEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set_items: Vec<ExecutableExprId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<TransientCollectionStep>,
    pub result: TransientCollectionResult,
    pub authority_flow: Vec<ExecutableExprId>,
    pub operation_work_budget: u64,
    pub storage_growth_budget: usize,
    pub snapshot_copy_budget: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedReferences {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub value_references: Vec<DistributedValueReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<DistributedCall>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedValueReference {
    pub expr_id: ExprId,
    pub canonical_path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_alias_paths: Vec<String>,
    pub producer_role: boon_typecheck::ProgramRole,
    pub flow_mode: boon_typecheck::FlowMode,
    pub value_type: boon_typecheck::Type,
}

pub fn distributed_event_source_path(canonical_path: &str) -> String {
    format!("@distributed/{canonical_path}")
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedCall {
    pub expression: ExecutableExprId,
    pub owner: Option<StaticOwnerId>,
    pub occurrence_path: String,
    pub canonical_function: String,
    pub producer_role: boon_typecheck::ProgramRole,
    pub result: boon_typecheck::FlowType,
    pub effect: boon_typecheck::CheckedEffectSummary,
    pub arguments: Vec<DistributedCallArgument>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invocation_arms: Vec<TriggerOwnedArm>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedCallArgument {
    pub name: String,
    pub value: ExecutableExprId,
    pub flow_type: boon_typecheck::FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProducerFunctionInstance {
    pub identity: [u8; 32],
    pub owner: StaticOwnerId,
    pub function: FunctionId,
    pub function_name: String,
    pub result_field: FieldId,
    pub result_path: String,
    pub root: ExecutableExprId,
    pub mode: crate::ProducerMaterializationMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_source: Option<SourceId>,
    pub arguments: Vec<ProducerFunctionArgument>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProducerFunctionArgument {
    pub name: String,
    pub parameter: ExecutableParameterId,
    pub flow_type: boon_typecheck::FlowType,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_expressions: Vec<ExecutableExprId>,
}

macro_rules! typed_usize_ids {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
            #[serde(transparent)]
            pub struct $name(pub usize);

            impl $name {
                pub fn as_usize(self) -> usize {
                    self.0
                }
            }

            impl fmt::Display for $name {
                fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    self.0.fmt(formatter)
                }
            }
        )+
    };
}

typed_usize_ids!(
    ExprId,
    ExecutableExprId,
    ExecutableLocalBindingId,
    ExecutableStatementId,
    ExecutableSourceId,
    ExecutableStateId,
    ScopeId,
    SourceId,
    StateId,
    ActivationId,
    PulseBatchId,
    ListId,
    FieldId,
    ViewBindingId,
    SourceUnitId,
    FunctionId,
    ErasedBindingId,
    ErasedReadId,
    SemanticMemoryId,
);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ExecutableParameterId {
    pub function: FunctionId,
    pub ordinal: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOutputContractKind {
    RetainedVisual { kind: SemanticRetainedVisualKind },
    HostValue,
}

impl SemanticOutputContractKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RetainedVisual {
                kind: SemanticRetainedVisualKind::Document,
            } => "retained_visual_document",
            Self::RetainedVisual {
                kind: SemanticRetainedVisualKind::Scene,
            } => "retained_visual_scene",
            Self::HostValue => "host_value",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRetainedVisualKind {
    Document,
    Scene,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOutputDemandPolicy {
    HostDemanded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticSourceUnit {
    pub id: SourceUnitId,
    pub path: String,
    pub module: Option<String>,
    pub start_line: usize,
    pub line_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticFieldEntry {
    pub id: FieldId,
    pub path: String,
    pub local_name: String,
    pub parent_path: String,
    pub scope_id: Option<ScopeId>,
    pub statement_id: usize,
    pub line: usize,
    pub kind: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourcePort {
    pub id: SourceId,
    pub path: String,
    pub binding_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_source_id: Option<ExecutableSourceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_owner: Option<StaticOwnerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_expr_id: Option<ExprId>,
    pub source_line: usize,
    pub scoped: bool,
    pub scope_id: Option<ScopeId>,
    pub interval_ms: Option<u64>,
    pub payload_schema: SourcePayloadSchema,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostPortDeclaration {
    HttpServer {
        line: usize,
        request_source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        disconnect_source: Option<String>,
        response_output: String,
    },
    WebSocketServer {
        line: usize,
        open_source: String,
        message_source: String,
        close_source: String,
        error_source: String,
        actions_output: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourcePayloadSchema {
    pub fields: Vec<SourcePayloadField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub typed_fields: Vec<SourcePayloadDescriptor>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourcePayloadDescriptor {
    pub field: SourcePayloadField,
    pub data_type: SemanticDataType,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum SourcePayloadField {
    Address,
    Bytes,
    Key,
    Named(String),
    Text,
}

impl SourcePayloadField {
    pub fn from_name(name: &str) -> Self {
        match name {
            "address" => Self::Address,
            "bytes" => Self::Bytes,
            "key" => Self::Key,
            "text" => Self::Text,
            _ => Self::Named(name.to_owned()),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Address => "address",
            Self::Bytes => "bytes",
            Self::Key => "key",
            Self::Named(name) => name.as_str(),
            Self::Text => "text",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListMemory {
    pub id: ListId,
    pub name: String,
    #[serde(default)]
    pub source_line: usize,
    pub row_scope_id: Option<ScopeId>,
    pub hidden_key_type: String,
    pub has_generation: bool,
    pub graph_clones_per_item: usize,
    pub capacity: Option<usize>,
    pub initializer: ListInitializer,
    /// Exact scalar storage destinations for named fields entering this list's
    /// row constructor. This includes semantic forwarding such as an
    /// initializer field `id` that constructs the stored field `key`.
    /// Resource-only row facades never enter this table.
    pub initializer_inputs: Vec<ListInitializerInput>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListInitializerInput {
    pub name: String,
    pub field: FieldId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StateCell {
    pub id: StateId,
    pub path: String,
    pub published: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_state_id: Option<ExecutableStateId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_owner: Option<StaticOwnerId>,
    pub lifetime: StateCellLifetimeV1,
    pub statement_id: usize,
    pub scope_id: Option<ScopeId>,
    pub hold_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expression_ids: Vec<ExprId>,
    pub indexed: bool,
    pub source_line: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StateCellLifetimeV1 {
    Persistent,
    ActivationLocal { then_expression: ExecutableExprId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum InitialValue {
    Text {
        value: String,
    },
    Number {
        value: boon_data::ExactNumber,
    },
    Bytes {
        bytes: Vec<u8>,
        fixed_len: Option<usize>,
    },
    Tag {
        name: String,
    },
    Data {
        value: boon_data::Value,
    },
    RootInitialField {
        path: String,
    },
    RowInitialField {
        path: String,
    },
    Unknown {
        summary: String,
    },
    /// The enclosing list field carries the exact executable expression that
    /// constructs a nested collection authority in the occurrence scope.
    ExpressionAuthority,
    /// Private row-source routing metadata; never a scalar initializer.
    ResourceOnly,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ListInitializer {
    RecordLiteral { rows: Vec<ListInitialRecord> },
    Range { from: i64, to: i64 },
    Empty,
    Unknown { summary: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListProjection {
    pub target: String,
    pub list: String,
    pub kind: ListProjectionKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ListProjectionKind {
    Chunk { size: Option<usize> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListInitialRecord {
    pub fields: Vec<ListRowInitialField>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListRowInitialField {
    pub name: String,
    pub value: InitialValue,
    /// Exact checked executable value for this field. Static fields introduced
    /// by a closed record spread may omit it because `value` is already the
    /// complete, lossless constant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<ExecutableExprId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DerivedValue {
    pub id: FieldId,
    pub executable_statement_id: ExecutableStatementId,
    /// Exact executable producer selected by semantic storage elaboration.
    ///
    /// A field statement can also retain a speculative, context-free checked
    /// value for diagnostics. Nested structural values must execute this
    /// producer instead of re-reading the statement's fallback value.
    pub producer: ExecutableExprId,
    pub path: String,
    pub kind: DerivedValueKind,
    /// Exact runtime state whose whole current value backs this derived field.
    ///
    /// This identity is emitted by semantic provenance. Backends must not
    /// recover it from paths or from a context-free fallback HOLD expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_backing: Option<StateId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialized_list_id: Option<ListId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialized_row_scope_id: Option<ScopeId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub causes: Vec<EventCause>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trigger_arms: Vec<TriggerOwnedArm>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_roots: Vec<ExecutableExprId>,
    pub sources: Vec<String>,
    pub indexed: bool,
    pub scope_id: Option<ScopeId>,
    pub startup_recompute: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum EventCause {
    Source(SourceId),
    State(StateId),
    Pulse(PulseBatchId),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TriggerOwnedArm {
    pub cause: EventCause,
    pub gate_checked_expr_id: boon_typecheck::CheckedExprId,
    pub gate_expression_id: ExecutableExprId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    pub output_expression_id: ExecutableExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StateUpdateArm {
    pub state: StateId,
    pub cause: EventCause,
    pub gate_checked_expr_id: boon_typecheck::CheckedExprId,
    pub gate_expression_id: ExecutableExprId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    pub output_expression_id: ExecutableExprId,
}

/// Exact verified owner for one typed host-effect occurrence.
///
/// Retained effects target ordinary state-update arms. Direct asynchronous
/// expressions target one compiler-owned transient derived-value lane instead;
/// that lane is runtime-only and never becomes HOLD or persistence authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HostEffectSchedule {
    pub id: usize,
    pub expression: ExecutableExprId,
    pub checked_expression: boon_typecheck::CheckedExprId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    pub operation: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_update_arms: Vec<StateUpdateArm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transient_result: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OutputRootValue {
    pub root: String,
    pub value_path: String,
    pub contract: SemanticOutputContractKind,
    pub demand: SemanticOutputDemandPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_type: Option<SemanticDataType>,
    pub statement_id: usize,
    pub executable_statement_id: ExecutableStatementId,
    pub value_expression_id: ExecutableExprId,
    pub binding_id: ErasedBindingId,
    pub line: usize,
    pub typed_contract_known: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DerivedValueKind {
    SourceEventTransform,
    ListView,
    Aggregate,
    Pure,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterializationResultKind {
    RuntimeValue,
    RenderSlot,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MaterializationLocalId(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ExecutableCallContextId {
    pub call_instance: usize,
    pub ordinal: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableExpression {
    pub id: ExecutableExprId,
    pub checked_expr_id: boon_typecheck::CheckedExprId,
    pub flow_type: boon_typecheck::FlowType,
    pub effect: boon_typecheck::CheckedEffectSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    #[serde(
        default,
        skip_serializing_if = "ExecutableValueProvenance::is_runtime_only"
    )]
    pub provenance: ExecutableValueProvenance,
    /// Exact semantic resource path after contextual call expansion. This is
    /// diagnostic/addressing metadata, never runtime ownership identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_binding_path: Option<String>,
    pub kind: ExecutableExpressionKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableValueProvenance {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<ExecutableValueMember>,
}

impl Default for ExecutableValueProvenance {
    fn default() -> Self {
        Self {
            members: vec![ExecutableValueMember {
                path: Vec::new(),
                origin: ExecutableValueOrigin::Runtime,
            }],
        }
    }
}

impl ExecutableValueProvenance {
    fn is_runtime_only(&self) -> bool {
        self.members.as_slice()
            == [ExecutableValueMember {
                path: Vec::new(),
                origin: ExecutableValueOrigin::Runtime,
            }]
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ExecutableValueMember {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    pub origin: ExecutableValueOrigin,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutableValueOrigin {
    Runtime,
    Source {
        source: boon_typecheck::CheckedSourceId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<StaticOwnerId>,
    },
    ProducerSource {
        function: FunctionId,
        identity: [u8; 32],
        owner: StaticOwnerId,
    },
    State {
        state: boon_typecheck::CheckedStateId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner: Option<StaticOwnerId>,
    },
    MaterializationLocal {
        owner: StaticOwnerId,
        local: MaterializationLocalId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActivationSite {
    pub id: ActivationId,
    pub then_expression: ExecutableExprId,
    pub input_expression: ExecutableExprId,
    pub output_expression: ExecutableExprId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_owner: Option<StaticOwnerId>,
    pub states: Vec<StateId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PulseSchedule {
    StageArbitrateCommitPublishBeforeNext,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PulseFlushPolicy {
    DiscardCurrentStopRemainingKeepPriorCommits,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PulseStart {
    Startup,
    Triggered { arms: Vec<TriggerOwnedArm> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PulseFusionEligibility {
    VerifiedActivationLocalRecurrence {
        activation: ActivationId,
        state: StateId,
        state_update_arm_index: usize,
        proof: PulseFusionProof,
    },
    Ineligible {
        diagnostics: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PulseFusionProof {
    FrozenRuntimeTargetGuardedFullTraceEmptySideLanes,
    FrozenRuntimeTargetGuardedFullTracePreservedListMutations,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PulseBatch {
    pub id: PulseBatchId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enclosing_activation: Option<ActivationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<StateId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_expression: Option<ExecutableExprId>,
    pub call_expression: ExecutableExprId,
    pub count_expression: ExecutableExprId,
    pub start: PulseStart,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_expression: Option<ExecutableExprId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_output: Option<ExecutableExprId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trigger_arms: Vec<TriggerOwnedArm>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_update_arms: Vec<StateUpdateArm>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_mutations: Vec<ListMutation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derived_value_indices: Vec<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_effects: Vec<PulseHostEffect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flush_roots: Vec<ExecutableExprId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emission_routes: Vec<PulseEmissionRoute>,
    pub schedule: PulseSchedule,
    pub flush_policy: PulseFlushPolicy,
    pub fusion: PulseFusionEligibility,
    pub semantic_slice_digest: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PulseHostEffect {
    pub expression: ExecutableExprId,
    pub operation: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PulseEmissionRoute {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer: Option<ExecutableExprId>,
    pub filter: PulseEmissionFilter,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PulseEmissionFilter {
    Passthrough,
    Skip {
        expression: ExecutableExprId,
        count_expression: ExecutableExprId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableRecordField {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<boon_typecheck::DeclId>,
    pub name: String,
    pub value: ExecutableExprId,
    pub spread: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableBlockBinding {
    pub id: ExecutableLocalBindingId,
    pub declaration: boon_typecheck::DeclId,
    pub value: ExecutableExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutableTextSegment {
    Static { value: String },
    Dynamic { value: ExecutableExprId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableCallArgument {
    pub ordinal: usize,
    pub name: String,
    pub value: ExecutableExprId,
    pub from_pipe: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableSelectArm {
    pub pattern: boon_typecheck::CheckedMatchPattern,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<ExecutablePatternBinding>,
    pub output: ExecutableExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutablePatternBinding {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutableCallableKind {
    Builtin,
    External,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutableExpressionKind {
    CanonicalRead {
        target: boon_typecheck::DeclId,
        path: String,
        projection: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<boon_typecheck::CheckedSourceRead>,
    },
    LocalRead {
        binding: ExecutableLocalBindingId,
        declaration: boon_typecheck::DeclId,
        projection: Vec<String>,
    },
    ExternalRead {
        canonical_path: String,
    },
    ElementState {
        context: ExecutableCallContextId,
        projection: Vec<String>,
    },
    Drain {
        target: boon_typecheck::DeclId,
        path: String,
        projection: Vec<String>,
    },
    Text(String),
    TextTemplate {
        segments: Vec<ExecutableTextSegment>,
    },
    Number(boon_data::ExactNumber),
    BytesByte(u8),
    /// Private flow absence. It cannot be materialized as public data.
    Absent,
    /// Private fail-fast control; the carrier is consumed by
    /// `FlushBoundary` and never enters the public value algebra.
    Flush {
        payload: ExecutableExprId,
    },
    FlushBoundary {
        input: ExecutableExprId,
    },
    Tag(String),
    TaggedObject {
        tag: String,
        fields: Vec<ExecutableRecordField>,
    },
    Source {
        binding_path: String,
    },
    Call {
        callable_kind: ExecutableCallableKind,
        name: String,
        intrinsic: Option<boon_typecheck::CheckedIntrinsicV1>,
        instance: usize,
        arguments: Vec<ExecutableCallArgument>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        contexts: Vec<ExecutableCallContextId>,
    },
    Materialize {
        materialization: usize,
    },
    Draining {
        input: ExecutableExprId,
    },
    Hold {
        initial: ExecutableExprId,
        name: String,
        binding_path: String,
        updates: Vec<ExecutableExprId>,
    },
    Latest {
        branches: Vec<ExecutableExprId>,
    },
    When {
        input: ExecutableExprId,
        arms: Vec<ExecutableSelectArm>,
    },
    Then {
        input: ExecutableExprId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<ExecutableExprId>,
    },
    Infix {
        left: ExecutableExprId,
        op: String,
        right: ExecutableExprId,
    },
    MatchArm {
        pattern: boon_typecheck::CheckedMatchPattern,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<ExecutableExprId>,
    },
    Object(Vec<ExecutableRecordField>),
    Block {
        bindings: Vec<ExecutableBlockBinding>,
        result: ExecutableExprId,
    },
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capacity: Option<usize>,
        items: Vec<ExecutableExprId>,
    },
    Bytes {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fixed_size: Option<usize>,
        items: Vec<ExecutableExprId>,
    },
    Delimiter,
    Project {
        input: ExecutableExprId,
        fields: Vec<String>,
    },
    MaterializationLocal {
        owner: StaticOwnerId,
        local: MaterializationLocalId,
        projection: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        constructor_projection: Vec<String>,
    },
    FunctionParameter {
        parameter: ExecutableParameterId,
        projection: Vec<String>,
    },
    MapEntry {
        key: ExecutableExprId,
        value: ExecutableExprId,
    },
    Map {
        entries: Vec<ExecutableExprId>,
    },
    Set {
        items: Vec<ExecutableExprId>,
    },
    Bits(boon_data::Bits),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextualOperationKind {
    Map,
    Filter,
    Retain,
    Remove,
    Every,
    Any,
    Find,
    SortBy,
    ThenBy,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableProgram {
    pub expressions: Vec<ExecutableExpression>,
    pub statements: Vec<ExecutableStatement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<ExecutableSourceDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<ExecutableStateDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<ExecutableRoot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<ExecutableFunction>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableSourceDef {
    pub id: ExecutableSourceId,
    pub origin: ExecutableSourceOrigin,
    pub declaration: boon_typecheck::DeclId,
    pub expression: ExecutableExprId,
    pub binding_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutableSourceOrigin {
    Checked {
        source: boon_typecheck::CheckedSourceId,
    },
    ProducerInvocation {
        function: FunctionId,
        identity: [u8; 32],
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableStateDef {
    pub id: ExecutableStateId,
    pub checked_state: boon_typecheck::CheckedStateId,
    pub declaration: boon_typecheck::DeclId,
    pub expression: ExecutableExprId,
    pub initial: ExecutableExprId,
    pub binding_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableRoot {
    pub checked_expr_id: boon_typecheck::CheckedExprId,
    pub expression: ExecutableExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableFunctionParameter {
    pub id: ExecutableParameterId,
    pub name: String,
    pub flow_type: boon_typecheck::FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableFunction {
    pub id: FunctionId,
    pub identity: [u8; 32],
    pub name: String,
    pub parameters: Vec<ExecutableFunctionParameter>,
    pub result_type: boon_typecheck::FlowType,
    pub root: ExecutableExprId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_source: Option<ExecutableExprId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableStatement {
    pub id: ExecutableStatementId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<boon_typecheck::DeclId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_type: Option<boon_typecheck::FlowType>,
    pub kind: ExecutableStatementKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<ExecutableExprId>,
    pub value_use: MaterializationResultKind,
    pub children: Vec<ExecutableStatementId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutableStatementKind {
    Field {
        name: String,
        path: String,
    },
    Source {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        event: Option<String>,
    },
    Hold {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hold_name: Option<String>,
    },
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        capacity: Option<usize>,
    },
    Block,
    Spread,
    Expression,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ErasedRowBinding {
    pub list: ListId,
    pub scope: ScopeId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasedOwnerDef {
    pub id: StaticOwnerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<StaticOwnerId>,
    pub child_ordinal: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_row: Option<ErasedRowBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_row: Option<ErasedRowBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_row: Option<ErasedRowBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasedLocalDef {
    pub owner: StaticOwnerId,
    pub local: MaterializationLocalId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<ErasedRowBinding>,
    pub source: ExecutableExprId,
    pub item_type: boon_typecheck::Type,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<ErasedLocalMember>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub captures: Vec<ErasedLocalCapture>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasedLocalMember {
    pub path: Vec<String>,
    pub target: ErasedLocalMemberTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forwarded_from: Option<ErasedLocalMemberForwarding>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErasedLocalMemberForwarding {
    Local {
        owner: StaticOwnerId,
        local: MaterializationLocalId,
        path: Vec<String>,
    },
    Row {
        row: ErasedRowBinding,
        path: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasedLocalCapture {
    pub source_owner: StaticOwnerId,
    pub source_local: MaterializationLocalId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
    pub field: FieldId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ErasedLocalMemberTarget {
    Field(FieldId),
    Sources(Vec<SourceId>),
    State(StateId),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErasedFieldRole {
    Value,
    ListAuthority,
    ValueAuthority,
    Capture,
}

impl ErasedFieldRole {
    pub const fn is_value(self) -> bool {
        matches!(self, Self::Value | Self::ValueAuthority)
    }

    pub const fn is_authority(self) -> bool {
        matches!(self, Self::ListAuthority | Self::ValueAuthority)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasedFieldDef {
    pub id: FieldId,
    pub role: ErasedFieldRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<boon_typecheck::DeclId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_owner: Option<StaticOwnerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<FieldId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<ErasedRowBinding>,
    /// Exact structural path within `row`. Empty for row roots and fields
    /// without row storage. Diagnostic paths are never storage identity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_path: Vec<String>,
    pub name: String,
    pub diagnostic_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement: Option<ExecutableStatementId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<ExecutableExprId>,
    #[serde(default)]
    pub resource_only: bool,
    pub flow_type: boon_typecheck::FlowType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasedReadBinding {
    pub id: ErasedReadId,
    pub expression: ExecutableExprId,
    pub target: ErasedReadTarget,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasedSourceDef {
    pub source: SourceId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_owner: Option<StaticOwnerId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owner_ancestry: Vec<StaticOwnerId>,
    pub origin: ErasedSourceOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErasedSourceOrigin {
    Executable {
        executable: ExecutableSourceId,
        binding: ErasedBindingId,
    },
    DistributedImport,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasedDependencyUse {
    pub dependent: ErasedBindingId,
    pub expression: ExecutableExprId,
    pub target: ErasedDependencyTarget,
    pub timing: ErasedDependencyTiming,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErasedDependencyTarget {
    ExternalRead { read: ErasedReadId },
    ExternalCall { reference: usize },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErasedDependencyTiming {
    Immediate,
    After {
        boundaries: Vec<ErasedTemporalBoundary>,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ErasedTemporalBoundary {
    Source(SourceId),
    State(StateId),
    Pulse(PulseBatchId),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErasedReadTarget {
    Binding {
        binding: ErasedBindingId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    SourcePayload {
        binding: ErasedBindingId,
        source: SourceId,
        field: SourcePayloadField,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    StateProjection {
        binding: ErasedBindingId,
        state: StateId,
        fields: Vec<String>,
    },
    Expression {
        expression: ExecutableExprId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    Local {
        binding: ExecutableLocalBindingId,
        declaration: boon_typecheck::DeclId,
        value: ExecutableExprId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    ExternalValue {
        reference: usize,
    },
    ElementState {
        context: ExecutableCallContextId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    MaterializationLocal {
        owner: StaticOwnerId,
        local: MaterializationLocalId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    FunctionParameter {
        parameter: ExecutableParameterId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasedScopeIndex {
    pub owners: Vec<ErasedOwnerDef>,
    pub locals: Vec<ErasedLocalDef>,
    pub fields: Vec<ErasedFieldDef>,
    pub bindings: Vec<ErasedBinding>,
    pub sources: Vec<ErasedSourceDef>,
    pub reads: Vec<ErasedReadBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_values: Vec<ErasedRowValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_source_projections: Vec<ErasedRowSourceProjection>,
    pub dependencies: Vec<ErasedDependencyUse>,
}

impl ErasedScopeIndex {
    pub fn owner_descends_from(
        &self,
        candidate: StaticOwnerId,
        ancestor: StaticOwnerId,
    ) -> Result<bool, String> {
        let mut next = Some(candidate);
        let mut remaining = self.owners.len().saturating_add(1);
        while let Some(owner) = next {
            if owner == ancestor {
                return Ok(true);
            }
            if remaining == 0 {
                return Err("erased static owner ancestry contains a cycle".to_owned());
            }
            remaining -= 1;
            next = self
                .owners
                .get(owner.as_usize())
                .filter(|definition| definition.id == owner)
                .ok_or_else(|| format!("missing erased static owner {owner}"))?
                .parent;
        }
        Ok(false)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasedRowValue {
    pub expression: ExecutableExprId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
    pub row: ErasedRowBinding,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ErasedRowSourceProjection {
    pub row: ErasedRowBinding,
    pub path: Vec<String>,
    pub source: SourceId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasedBinding {
    pub id: ErasedBindingId,
    pub declaration: boon_typecheck::DeclId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_owner: Option<StaticOwnerId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owner_ancestry: Vec<StaticOwnerId>,
    pub flow_type: boon_typecheck::FlowType,
    pub producer: ExecutableExprId,
    pub diagnostic_path: String,
    pub target: ErasedBindingTarget,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErasedBindingTarget {
    Value {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        field: Option<FieldId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        row: Option<ErasedRowBinding>,
    },
    Source {
        executable: ExecutableSourceId,
        runtime: SourceId,
    },
    State {
        executable: ExecutableStateId,
        runtime: StateId,
        published: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        field: Option<FieldId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        row: Option<ErasedRowBinding>,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextualRowPredecessor {
    Value,
    Stored { row: ErasedRowBinding },
    Materialized { materialization: usize },
    Provenance { materialization: usize },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextualMaterialization {
    pub id: usize,
    pub operation: ContextualOperationKind,
    pub source: ExecutableExprId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_row_predecessors: Vec<ContextualRowPredecessor>,
    pub body: ExecutableExprId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<ExecutableExprId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inherited_order: Vec<ContextualOrderKey>,
    pub result_kind: MaterializationResultKind,
    pub row_local: MaterializationLocalId,
    pub owner: StaticOwnerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_list_id: Option<ListId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_scope_id: Option<ScopeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_list_id: Option<ListId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_scope_id: Option<ScopeId>,
    pub item_type: boon_typecheck::Type,
    pub result_type: boon_typecheck::Type,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextualOrderKey {
    pub operation: ContextualOperationKind,
    pub body: ExecutableExprId,
    pub direction: ExecutableExprId,
}

impl ContextualMaterialization {
    pub fn expression_roots(&self) -> Vec<ExecutableExprId> {
        let mut roots = Vec::with_capacity(3 + self.inherited_order.len() * 2);
        roots.push(self.source);
        roots.push(self.body);
        roots.extend(self.direction);
        for key in &self.inherited_order {
            roots.push(key.body);
            roots.push(key.direction);
        }
        roots
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub indexed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListMutation {
    pub list_id: ListId,
    pub site: ExecutableExprId,
    pub ordinal: u32,
    pub cause: EventCause,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    pub kind: ListMutationKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ListMutationKind {
    Append {
        gate: ExecutableExprId,
        item: ExecutableExprId,
    },
    Remove {
        gate: ExecutableExprId,
        owner: StaticOwnerId,
        row_local: MaterializationLocalId,
        predicate: ExecutableExprId,
        remove_when: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ViewBinding {
    pub id: ViewBindingId,
    /// Exact executable retained-view constructor that owns this binding.
    /// Backends must not rediscover the node from diagnostic kind/path text.
    pub node_expression: ExecutableExprId,
    /// Exact executable constructor argument whose dependency produced this
    /// binding. Backends use it when the dependency is not itself the complete
    /// bound value (for example, a read inside BLOCK, WHEN, or List/find).
    pub argument_expression: ExecutableExprId,
    pub node_kind: String,
    pub attr: String,
    pub path: String,
    pub target: ViewBindingTarget,
    pub kind: ViewBindingKind,
    pub scope_id: Option<ScopeId>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewBindingTarget {
    Read {
        read: ErasedReadId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        additional_projection: Vec<String>,
    },
    Source {
        source: SourceId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ViewBindingKind {
    Data,
    Source,
    Target,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticMemoryKind {
    RootScalar,
    IndexedField,
    ListOwner,
    Map,
    Set,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SemanticMemoryIdentity {
    pub canonical_module: String,
    pub owner_path: String,
    pub semantic_path: String,
    pub kind: SemanticMemoryKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticMemory {
    pub id: SemanticMemoryId,
    pub identity: SemanticMemoryIdentity,
    pub data_type: SemanticDataType,
    pub leaves: Vec<SemanticMemoryLeaf>,
    pub status: SemanticMemoryStatus,
    pub runtime_backing: SemanticMemoryRuntimeBacking,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub structural_owner_rows: Vec<ErasedRowBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticMemoryLeaf {
    pub semantic_path: String,
    pub data_type: SemanticDataType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticMemoryStatus {
    Active,
    Draining { marker_expr_id: ExprId },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticMemoryRuntimeBacking {
    RootState {
        state_id: StateId,
        field_id: Option<FieldId>,
    },
    IndexedState {
        state_id: StateId,
        field_id: Option<FieldId>,
        scope_id: ScopeId,
        list_id: Option<ListId>,
    },
    List {
        list_id: ListId,
        row_scope_id: Option<ScopeId>,
    },
    Collection {
        expression: ExecutableExprId,
        owner: Option<StaticOwnerId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticDataType {
    Number,
    Text,
    Bytes {
        fixed_len: Option<usize>,
    },
    Variant {
        variants: Vec<SemanticVariantType>,
    },
    Record {
        fields: Vec<SemanticTypeField>,
        open: bool,
    },
    List {
        item: Box<SemanticDataType>,
    },
    Map {
        key: Box<SemanticDataType>,
        value: Box<SemanticDataType>,
    },
    Set {
        item: Box<SemanticDataType>,
    },
    Unknown {
        reason: String,
    },
    Union {
        members: Vec<SemanticDataType>,
    },
    Bits {
        width: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticTypeField {
    pub name: String,
    pub data_type: SemanticDataType,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticVariantType {
    pub tag: String,
    pub fields: Vec<SemanticTypeField>,
    pub open: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationEdge {
    pub source_leaves: Vec<MigrationSourceLeaf>,
    pub destination: MigrationDestination,
    pub transfer_kind: MigrationTransferKind,
    pub transform: MigrationTransform,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationSourceLeaf {
    pub memory_id: SemanticMemoryId,
    pub semantic_path: String,
    pub data_type: SemanticDataType,
    pub drain_expr_id: ExprId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationDestination {
    pub memory_id: SemanticMemoryId,
    pub semantic_path: String,
    pub data_type: SemanticDataType,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationTransferKind {
    Scalar,
    List,
    IndexedField,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MigrationTransform {
    Identity,
    PureExpression {
        expression_root: ExprId,
        pipeline: Vec<ExprId>,
    },
}

pub fn executable_expression_children(kind: &ExecutableExpressionKind) -> Vec<ExecutableExprId> {
    match kind {
        ExecutableExpressionKind::CanonicalRead { .. }
        | ExecutableExpressionKind::LocalRead { .. }
        | ExecutableExpressionKind::ExternalRead { .. }
        | ExecutableExpressionKind::ElementState { .. }
        | ExecutableExpressionKind::Drain { .. }
        | ExecutableExpressionKind::Text(_)
        | ExecutableExpressionKind::Number(_)
        | ExecutableExpressionKind::Bits(_)
        | ExecutableExpressionKind::BytesByte(_)
        | ExecutableExpressionKind::Absent
        | ExecutableExpressionKind::Tag(_)
        | ExecutableExpressionKind::Source { .. }
        | ExecutableExpressionKind::Materialize { .. }
        | ExecutableExpressionKind::Delimiter
        | ExecutableExpressionKind::MaterializationLocal { .. }
        | ExecutableExpressionKind::FunctionParameter { .. } => Vec::new(),
        ExecutableExpressionKind::TextTemplate { segments } => segments
            .iter()
            .filter_map(|segment| match segment {
                ExecutableTextSegment::Static { .. } => None,
                ExecutableTextSegment::Dynamic { value } => Some(*value),
            })
            .collect(),
        ExecutableExpressionKind::TaggedObject { fields, .. }
        | ExecutableExpressionKind::Object(fields) => {
            fields.iter().map(|field| field.value).collect()
        }
        ExecutableExpressionKind::Block { bindings, result } => bindings
            .iter()
            .map(|binding| binding.value)
            .chain(std::iter::once(*result))
            .collect(),
        ExecutableExpressionKind::Call { arguments, .. } => {
            arguments.iter().map(|argument| argument.value).collect()
        }
        ExecutableExpressionKind::Flush { payload: input }
        | ExecutableExpressionKind::FlushBoundary { input }
        | ExecutableExpressionKind::Draining { input }
        | ExecutableExpressionKind::Project { input, .. } => vec![*input],
        ExecutableExpressionKind::Hold {
            initial, updates, ..
        } => std::iter::once(*initial)
            .chain(updates.iter().copied())
            .collect(),
        ExecutableExpressionKind::Latest { branches } => branches.clone(),
        ExecutableExpressionKind::When { input, arms } => std::iter::once(*input)
            .chain(arms.iter().map(|arm| arm.output))
            .collect(),
        ExecutableExpressionKind::Then { input, output } => std::iter::once(*input)
            .chain(output.iter().copied())
            .collect(),
        ExecutableExpressionKind::Infix { left, right, .. } => vec![*left, *right],
        ExecutableExpressionKind::MapEntry { key, value } => vec![*key, *value],
        ExecutableExpressionKind::MatchArm { output, .. } => output.iter().copied().collect(),
        ExecutableExpressionKind::List { items, .. }
        | ExecutableExpressionKind::Bytes { items, .. }
        | ExecutableExpressionKind::Map { entries: items }
        | ExecutableExpressionKind::Set { items } => items.clone(),
    }
}
