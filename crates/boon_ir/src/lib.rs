use boon_parser::ParsedProgram;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

mod contextual_expansion;
mod out_net;
mod semantic_migration;

pub use semantic_migration::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasedProgram {
    pub executable: ExecutableProgram,
    pub scope_index: ErasedScopeIndex,
    pub expression_count: usize,
    pub expression_coverage: ExpressionCoverage,
    #[serde(default)]
    pub distributed_references: DistributedReferences,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub producer_function_instances: Vec<ProducerFunctionInstance>,
    pub semantic_index: SemanticIndex,
    pub graph_node_count: usize,
    pub row_scopes: Vec<RowScope>,
    pub sources: Vec<SourcePort>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub host_ports: Vec<HostPortDeclaration>,
    pub state_cells: Vec<StateCell>,
    pub lists: Vec<ListMemory>,
    #[serde(default)]
    pub semantic_memory: Vec<SemanticMemory>,
    #[serde(default)]
    pub migration_edges: Vec<MigrationEdge>,
    pub output_values: Vec<OutputRootValue>,
    pub derived_values: Vec<DerivedValue>,
    pub dependencies: Vec<DependencyEdge>,
    pub possible_causes: Vec<PossibleCause>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_update_arms: Vec<StateUpdateArm>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_mutations: Vec<ListMutation>,
    pub list_projections: Vec<ListProjection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materializations: Vec<ContextualMaterialization>,
    pub view_bindings: Vec<ViewBinding>,
    pub expression_types: boon_typecheck::ExprTypeTable,
    pub function_types: boon_typecheck::FunctionTypeTable,
    pub named_value_types: boon_typecheck::NamedValueTypeTable,
    pub hidden_identity_verified: bool,
    pub static_schedule_verified: bool,
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
    pub mode: ProducerFunctionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_source: Option<SourceId>,
    pub arguments: Vec<ProducerFunctionArgument>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerFunctionMode {
    Current,
    Invocation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProducerFunctionArgument {
    pub name: String,
    pub parameter: ExecutableParameterId,
    pub flow_type: boon_typecheck::FlowType,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_expressions: Vec<ExecutableExprId>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProducerFunctionLoweringRequest {
    pub identity: [u8; 32],
    pub local_function: String,
    pub mode: ProducerFunctionMode,
}

#[derive(Clone, Debug)]
struct PendingDistributedReferences {
    value_references: Vec<DistributedValueReference>,
    calls: Vec<PendingDistributedCall>,
}

#[derive(Clone, Debug)]
struct PendingDistributedCall {
    checked_expression: boon_typecheck::CheckedExprId,
    canonical_function: String,
    producer_role: boon_typecheck::ProgramRole,
    result: boon_typecheck::FlowType,
    effect: boon_typecheck::CheckedEffectSummary,
    arguments: Vec<PendingDistributedCallArgument>,
}

#[derive(Clone, Debug)]
struct PendingDistributedCallArgument {
    name: String,
    flow_type: boon_typecheck::FlowType,
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
    ExecutableStatementId,
    ExecutableSourceId,
    ExecutableStateId,
    ScopeId,
    SourceId,
    StateId,
    ListId,
    FieldId,
    ViewBindingId,
    SourceUnitId,
    FunctionId,
    StaticOwnerId,
    ErasedBindingId,
    ErasedReadId,
    DiagnosticSpanId,
    SemanticSymbolId,
    SemanticMemoryId,
);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ExecutableParameterId {
    pub function: FunctionId,
    pub ordinal: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticIndex {
    pub version: u32,
    pub computed_from: String,
    pub parser_policy_phase: String,
    pub reuse_key: String,
    pub output_roots: Vec<SemanticOutputRootEntry>,
    pub source_units: Vec<SemanticSourceUnit>,
    pub sources: Vec<SemanticSourceEntry>,
    pub lists: Vec<SemanticListEntry>,
    pub row_scopes: Vec<SemanticRowScopeEntry>,
    pub functions: Vec<SemanticFunctionEntry>,
    pub fields: Vec<SemanticFieldEntry>,
    pub view_bindings: Vec<SemanticViewBindingEntry>,
    pub diagnostic_spans: Vec<SemanticDiagnosticSpan>,
    pub symbols: Vec<SemanticSymbolEntry>,
    pub readiness: SemanticIndexReadiness,
    pub reuse: SemanticIndexReuse,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticOutputRootEntry {
    pub root: String,
    pub contract: SemanticOutputContractKind,
    pub demand: SemanticOutputDemandPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_type: Option<SemanticDataType>,
    pub statement_id: usize,
    pub line: usize,
    pub typed_contract_known: bool,
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
pub struct SemanticSourceEntry {
    pub id: SourceId,
    pub path: String,
    pub scoped: bool,
    pub scope_id: Option<ScopeId>,
    pub payload_schema_known: bool,
    pub payload_field_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticListEntry {
    pub id: ListId,
    pub name: String,
    pub row_scope_id: Option<ScopeId>,
    pub capacity: Option<usize>,
    pub initializer_known: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticRowScopeEntry {
    pub id: ScopeId,
    pub list: String,
    pub function: String,
    pub row_scope: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticFunctionEntry {
    pub id: FunctionId,
    pub name: String,
    pub args: Vec<String>,
    pub statement_id: usize,
    pub line: usize,
    pub type_known: bool,
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
pub struct SemanticViewBindingEntry {
    pub id: ViewBindingId,
    pub node_kind: String,
    pub attr: String,
    pub path: String,
    pub kind: ViewBindingKind,
    pub scope_id: Option<ScopeId>,
    pub source_id: Option<SourceId>,
    pub render_contract_known: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticDiagnosticSpan {
    pub id: DiagnosticSpanId,
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub severity: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticSymbolEntry {
    pub id: SemanticSymbolId,
    pub category: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticIndexReadiness {
    pub source_payload_schemas: SemanticKnowledgeStatus,
    pub source_completions: SemanticKnowledgeStatus,
    pub route_critical_unknowns: SemanticKnowledgeStatus,
    pub row_scopes: SemanticKnowledgeStatus,
    pub row_scope_ambiguity: SemanticKnowledgeStatus,
    pub selectors: SemanticKnowledgeStatus,
    pub selector_index_ambiguity: SemanticKnowledgeStatus,
    pub render_contracts: SemanticKnowledgeStatus,
    pub bridge_page_descriptors: SemanticKnowledgeStatus,
    pub dynamic_fallback_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticKnowledgeStatus {
    pub known_count: usize,
    pub fallback_count: usize,
    pub fallback_reasons: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticIndexReuse {
    pub parser_reused_by_ir: bool,
    pub typecheck_reused_by_ir: bool,
    pub runtime_reports_reuse_index: bool,
    pub shared_tables: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExpressionCoverage {
    pub computed_from: String,
    pub ast_expression_count: usize,
    #[serde(default)]
    pub distributed_reference_expression_count: usize,
    pub unknown_ast_expression_count: usize,
    pub ignored_unknown_ast_expression_count: usize,
    pub unknown_list_initializer_count: usize,
    pub unknown_list_initial_value_count: usize,
    pub unknown_list_predicate_count: usize,
    pub unknown_derived_value_count: usize,
    pub unknown_labels: Vec<String>,
    pub ignored_unknown_labels: Vec<String>,
}

impl ExpressionCoverage {
    pub fn empty() -> Self {
        Self {
            computed_from: "parser_ast_and_typed_ir".to_owned(),
            ast_expression_count: 0,
            distributed_reference_expression_count: 0,
            unknown_ast_expression_count: 0,
            ignored_unknown_ast_expression_count: 0,
            unknown_list_initializer_count: 0,
            unknown_list_initial_value_count: 0,
            unknown_list_predicate_count: 0,
            unknown_derived_value_count: 0,
            unknown_labels: Vec::new(),
            ignored_unknown_labels: Vec::new(),
        }
    }

    pub fn unknown_total(&self) -> usize {
        self.unknown_ast_expression_count
            + self.unknown_list_initializer_count
            + self.unknown_list_initial_value_count
            + self.unknown_list_predicate_count
            + self.unknown_derived_value_count
    }
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
pub struct RowScope {
    pub id: ScopeId,
    pub list: String,
    pub function: String,
    pub row_scope: String,
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
    pub statement_id: usize,
    pub scope_id: Option<ScopeId>,
    pub hold_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expression_ids: Vec<ExprId>,
    pub indexed: bool,
    pub source_line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum InitialValue {
    Text {
        value: String,
    },
    Number {
        value: String,
    },
    Bool {
        value: bool,
    },
    Bytes {
        bytes: Vec<u8>,
        fixed_len: Option<usize>,
    },
    Enum {
        value: String,
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
    pub path: String,
    pub kind: DerivedValueKind,
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
    /// Exact semantic resource path after contextual call expansion. This is
    /// diagnostic/addressing metadata, never runtime ownership identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_binding_path: Option<String>,
    pub kind: ExecutableExpressionKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableRecordField {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration: Option<boon_typecheck::DeclId>,
    pub name: String,
    pub value: ExecutableExprId,
    pub spread: bool,
    #[serde(default)]
    pub resource_only: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableBlockBinding {
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
    Number(String),
    BytesByte(u8),
    Bool(bool),
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
    Record(Vec<ExecutableRecordField>),
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
    },
    FunctionParameter {
        parameter: ExecutableParameterId,
        projection: Vec<String>,
    },
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
    pub declaration: boon_typecheck::DeclId,
    pub expression: ExecutableExprId,
    pub binding_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableStateDef {
    pub id: ExecutableStateId,
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
    pub name: String,
    pub parameters: Vec<ExecutableFunctionParameter>,
    pub result_type: boon_typecheck::FlowType,
    pub root: ExecutableExprId,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasedLocalMemberForwarding {
    pub owner: StaticOwnerId,
    pub local: MaterializationLocalId,
    pub path: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErasedLocalCapture {
    pub source_owner: StaticOwnerId,
    pub source_local: MaterializationLocalId,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projection: Vec<String>,
    pub field: FieldId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ErasedLocalMemberTarget {
    Field(FieldId),
    Source(SourceId),
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
        declaration: boon_typecheck::DeclId,
        value: ExecutableExprId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    ExternalValue {
        reference: usize,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextualMaterialization {
    pub id: usize,
    pub operation: ContextualOperationKind,
    pub source: ExecutableExprId,
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
pub struct StaticOwnerDef {
    pub id: StaticOwnerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<StaticOwnerId>,
    pub child_ordinal: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub indexed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PossibleCause {
    pub target: String,
    pub sources: Vec<String>,
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

pub fn lower(program: &ParsedProgram) -> Result<ErasedProgram, String> {
    lower_with_external_types(program, &boon_typecheck::ExternalTypeEnvironment::default())
}

pub fn lower_runtime(program: &ParsedProgram) -> Result<ErasedProgram, String> {
    lower_runtime_with_external_types(program, &boon_typecheck::ExternalTypeEnvironment::default())
}

pub fn lower_with_external_types(
    program: &ParsedProgram,
    external_types: &boon_typecheck::ExternalTypeEnvironment,
) -> Result<ErasedProgram, String> {
    lower_with_typecheck(program, external_types, true, &[])
}

pub fn lower_runtime_with_external_types(
    program: &ParsedProgram,
    external_types: &boon_typecheck::ExternalTypeEnvironment,
) -> Result<ErasedProgram, String> {
    lower_with_typecheck(program, external_types, false, &[])
}

pub fn lower_runtime_with_external_types_and_producer_functions(
    program: &ParsedProgram,
    external_types: &boon_typecheck::ExternalTypeEnvironment,
    requests: &[ProducerFunctionLoweringRequest],
) -> Result<ErasedProgram, String> {
    lower_with_typecheck(program, external_types, false, requests)
}

fn producer_identity_text(identity: [u8; 32]) -> String {
    identity.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn next_synthetic_checked_id(next: &mut u32, kind: &str) -> Result<u32, String> {
    let id = *next;
    *next = next
        .checked_add(1)
        .ok_or_else(|| format!("checked {kind} IDs are exhausted"))?;
    Ok(id)
}

fn checked_scope_is_within(
    program: &boon_typecheck::CheckedProgram,
    mut scope: boon_typecheck::LexicalScopeId,
    ancestor: boon_typecheck::LexicalScopeId,
) -> bool {
    let mut visited = BTreeSet::new();
    while visited.insert(scope) {
        if scope == ancestor {
            return true;
        }
        let Some(parent) = program
            .scopes
            .iter()
            .find(|candidate| candidate.id == scope)
            .and_then(|scope| scope.parent)
        else {
            break;
        };
        scope = parent;
    }
    false
}

fn checked_function_requires_pass(
    program: &boon_typecheck::CheckedProgram,
    callable: &boon_typecheck::CheckedCallableSignature,
) -> bool {
    !callable.contexts.is_empty()
        || program.expressions.iter().any(|expression| {
            checked_scope_is_within(program, expression.scope_id, callable.scope_id)
                && matches!(
                    expression.kind,
                    boon_typecheck::CheckedExpressionKind::Passed { .. }
                )
        })
}

fn elaborate_producer_function_roots(
    program: &mut boon_typecheck::CheckedProgram,
    requests: &[ProducerFunctionLoweringRequest],
) -> Result<Vec<out_net::ProducerRoot>, String> {
    let mut requests = requests.to_vec();
    requests.sort();
    requests.dedup();
    for request in &requests {
        if request.identity.iter().all(|byte| *byte == 0) {
            return Err("producer function lowering request identity must be nonzero".to_owned());
        }
    }
    for pair in requests.windows(2) {
        if pair[0].identity == pair[1].identity {
            return Err(format!(
                "producer function identity {} is requested for both `{}` and `{}`",
                producer_identity_text(pair[0].identity),
                pair[0].local_function,
                pair[1].local_function,
            ));
        }
    }

    let mut resolved = Vec::with_capacity(requests.len());
    for request in requests {
        let matches = program
            .callables
            .iter()
            .filter(|callable| {
                callable.kind == boon_typecheck::CheckedCallableKind::User
                    && callable.name == request.local_function
            })
            .cloned()
            .collect::<Vec<_>>();
        let callable = match matches.as_slice() {
            [callable] => callable.clone(),
            [] => {
                return Err(format!(
                    "producer function `{}` does not resolve to a user function",
                    request.local_function
                ));
            }
            _ => {
                return Err(format!(
                    "producer function `{}` resolves ambiguously to {} user functions",
                    request.local_function,
                    matches.len()
                ));
            }
        };
        if callable.result.mode != boon_typecheck::FlowMode::Continuous {
            return Err(format!(
                "producer function `{}` result must be continuous, found {:?}",
                request.local_function, callable.result.mode
            ));
        }
        let out_parameters = callable
            .parameters
            .iter()
            .filter(|parameter| parameter.kind != boon_typecheck::CheckedParameterKind::Value)
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>();
        if !out_parameters.is_empty() {
            return Err(format!(
                "producer function `{}` has unsupported OUT parameter(s): {}",
                request.local_function,
                out_parameters.join(", ")
            ));
        }
        if checked_function_requires_pass(program, &callable) {
            return Err(format!(
                "producer function `{}` has an unsupported PASS-in-signature requirement",
                request.local_function
            ));
        }
        if callable.result_expression.is_none() {
            return Err(format!(
                "producer function `{}` has no checked result expression",
                request.local_function
            ));
        }
        let span = program
            .declarations
            .iter()
            .find(|declaration| declaration.id == callable.decl_id)
            .map(|declaration| declaration.span)
            .unwrap_or_default();
        resolved.push((request, callable, span));
    }

    let mut next_declaration = program
        .declarations
        .iter()
        .map(|declaration| declaration.id.0)
        .max()
        .map_or(0, |id| id.saturating_add(1));
    let mut next_expression = program
        .expressions
        .iter()
        .map(|expression| expression.id.0)
        .max()
        .map_or(0, |id| id.saturating_add(1));
    let mut next_call = program
        .calls
        .iter()
        .map(|call| call.id.0)
        .max()
        .map_or(0, |id| id.saturating_add(1));
    let mut next_statement = program
        .statements
        .iter()
        .map(|statement| statement.id.0)
        .max()
        .map_or(0, |id| id.saturating_add(1));
    let mut roots = Vec::with_capacity(resolved.len());

    for (function_ordinal, (request, callable, span)) in resolved.into_iter().enumerate() {
        let function = FunctionId(function_ordinal);
        let identity_text = producer_identity_text(request.identity);
        let result_path = format!("@producer/{identity_text}/result");
        let invocation = request.mode == ProducerFunctionMode::Invocation;
        let invocation_flow = boon_typecheck::FlowType {
            mode: boon_typecheck::FlowMode::PresentOrAbsent,
            ty: callable.result.ty.clone(),
        };
        let invocation_source = if invocation {
            let declaration = boon_typecheck::DeclId(next_synthetic_checked_id(
                &mut next_declaration,
                "declaration",
            )?);
            let expression = boon_typecheck::CheckedExprId(next_synthetic_checked_id(
                &mut next_expression,
                "expression",
            )?);
            let source_flow = boon_typecheck::FlowType {
                mode: boon_typecheck::FlowMode::PresentOrAbsent,
                ty: boon_typecheck::Type::Unknown,
            };
            program
                .declarations
                .push(boon_typecheck::CheckedDeclaration {
                    id: declaration,
                    scope_id: program.root_scope,
                    name: format!("__producer_{identity_text}_invoke"),
                    kind: boon_typecheck::CheckedDeclarationKind::Source,
                    flow_type: source_flow.clone(),
                    value: Some(expression),
                    body_scope: None,
                    span,
                });
            program.expressions.push(boon_typecheck::CheckedExpression {
                id: expression,
                scope_id: program.root_scope,
                declaration: Some(declaration),
                flow_type: source_flow,
                effect: boon_typecheck::CheckedEffectSummary {
                    emits_source: true,
                    ..boon_typecheck::CheckedEffectSummary::default()
                },
                kind: boon_typecheck::CheckedExpressionKind::Source,
                span,
            });
            Some(expression)
        } else {
            None
        };
        let result_declaration = boon_typecheck::DeclId(next_synthetic_checked_id(
            &mut next_declaration,
            "declaration",
        )?);
        let mut parameters = callable.parameters.clone();
        parameters.sort_by_key(|parameter| parameter.ordinal);
        let mut root_parameters = Vec::with_capacity(parameters.len());
        let mut entries = Vec::with_capacity(parameters.len());
        for parameter in parameters {
            let checked_expression = boon_typecheck::CheckedExprId(next_synthetic_checked_id(
                &mut next_expression,
                "expression",
            )?);
            let executable_parameter = ExecutableParameterId {
                function,
                ordinal: parameter.ordinal,
            };
            program.expressions.push(boon_typecheck::CheckedExpression {
                id: checked_expression,
                scope_id: program.root_scope,
                declaration: None,
                flow_type: parameter.flow_type.clone(),
                effect: boon_typecheck::CheckedEffectSummary::default(),
                kind: boon_typecheck::CheckedExpressionKind::Read {
                    target: parameter.decl_id,
                    projection: Vec::new(),
                    source: None,
                },
                span,
            });
            entries.push(boon_typecheck::CheckedCallEntry::Input {
                formal: parameter.decl_id,
                name: parameter.name.clone(),
                value: checked_expression,
                from_pipe: false,
                evaluation_scope: parameter.evaluation_scope,
            });
            root_parameters.push(out_net::ProducerRootParameter {
                checked_expression,
                parameter: executable_parameter,
                name: parameter.name,
                flow_type: parameter.flow_type,
            });
        }

        let call =
            boon_typecheck::CheckedCallId(next_synthetic_checked_id(&mut next_call, "call")?);
        let call_expression = boon_typecheck::CheckedExprId(next_synthetic_checked_id(
            &mut next_expression,
            "expression",
        )?);
        let result_expression = if invocation {
            boon_typecheck::CheckedExprId(next_synthetic_checked_id(
                &mut next_expression,
                "expression",
            )?)
        } else {
            call_expression
        };
        let result_statement = boon_typecheck::CheckedStatementId(next_synthetic_checked_id(
            &mut next_statement,
            "statement",
        )?);
        program
            .declarations
            .push(boon_typecheck::CheckedDeclaration {
                id: result_declaration,
                scope_id: program.root_scope,
                name: format!("__producer_{identity_text}_result"),
                kind: boon_typecheck::CheckedDeclarationKind::Field,
                flow_type: if invocation {
                    invocation_flow.clone()
                } else {
                    callable.result.clone()
                },
                value: Some(result_expression),
                body_scope: None,
                span,
            });
        program.calls.push(boon_typecheck::CheckedCall {
            id: call,
            expression: call_expression,
            callable: callable.decl_id,
            owner_callable: None,
            function: callable.name.clone(),
            entries,
            contexts: Vec::new(),
            pass: None,
            type_substitutions: Vec::new(),
            result: callable.result.clone(),
            role: callable.role,
            span,
        });
        program.expressions.push(boon_typecheck::CheckedExpression {
            id: call_expression,
            scope_id: program.root_scope,
            declaration: (!invocation).then_some(result_declaration),
            flow_type: callable.result.clone(),
            effect: callable.effect,
            kind: boon_typecheck::CheckedExpressionKind::Call { call },
            span,
        });
        if let Some(source_expression) = invocation_source {
            program.expressions.push(boon_typecheck::CheckedExpression {
                id: result_expression,
                scope_id: program.root_scope,
                declaration: Some(result_declaration),
                flow_type: invocation_flow.clone(),
                effect: boon_typecheck::CheckedEffectSummary {
                    emits_source: true,
                    ..callable.effect
                },
                kind: boon_typecheck::CheckedExpressionKind::Then {
                    input: source_expression,
                    output: Some(call_expression),
                },
                span,
            });
        }
        program.statements.push(boon_typecheck::CheckedStatement {
            id: result_statement,
            scope_id: program.root_scope,
            kind: boon_typecheck::CheckedStatementKind::Field {
                declaration: result_declaration,
            },
            value: Some(result_expression),
            value_use: boon_typecheck::CheckedValueUse::RuntimeValue,
            children: Vec::new(),
            span,
        });
        roots.push(out_net::ProducerRoot {
            identity: request.identity,
            mode: request.mode,
            call,
            function,
            function_name: callable.name,
            result_statement,
            result_declaration,
            result_path,
            result_type: if invocation {
                invocation_flow
            } else {
                callable.result
            },
            invocation_source_expression: invocation_source,
            parameters: root_parameters,
        });
    }
    Ok(roots)
}

fn lower_with_typecheck(
    program: &ParsedProgram,
    external_types: &boon_typecheck::ExternalTypeEnvironment,
    include_type_hints: bool,
    producer_requests: &[ProducerFunctionLoweringRequest],
) -> Result<ErasedProgram, String> {
    let trace_lower = std::env::var_os("BOON_IR_LOWER_TRACE").is_some();
    let trace_phase = |phase: &str, elapsed_ms: f64| {
        if trace_lower {
            eprintln!("boon_ir lower {phase}: {elapsed_ms:.3}ms");
        }
    };
    let typecheck_started = Instant::now();
    if trace_lower {
        eprintln!("boon_ir lower typecheck:start");
    }
    let check_output = if include_type_hints {
        boon_typecheck::check_program_profiled_with_external_types(program, external_types)
    } else {
        boon_typecheck::check_runtime_program_profiled_with_external_types(program, external_types)
    }
    .0;
    let typecheck_report = check_output.report;
    let typecheck_ms = lower_elapsed_ms(typecheck_started);
    trace_phase("typecheck", typecheck_ms);
    if typecheck_report.has_errors() {
        let mut failures = typecheck_report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == boon_typecheck::DiagnosticSeverity::Error)
            .map(|diagnostic| {
                let location = program
                    .files
                    .iter()
                    .filter(|file| file.start_line <= diagnostic.line)
                    .max_by_key(|file| file.start_line)
                    .map_or_else(
                        || format!("line {}", diagnostic.line),
                        |file| {
                            format!(
                                "{}:{}",
                                file.path,
                                diagnostic
                                    .line
                                    .saturating_sub(file.start_line)
                                    .saturating_add(1)
                            )
                        },
                    );
                format!("{location}: {}", diagnostic.message)
            })
            .collect::<Vec<_>>();
        failures.extend(
            typecheck_report
                .render_slot_table
                .slots
                .iter()
                .flat_map(|slot| {
                    slot.diagnostics
                        .iter()
                        .filter(|diagnostic| {
                            diagnostic.severity == boon_typecheck::DiagnosticSeverity::Error
                        })
                        .map(|diagnostic| {
                            format!(
                                "render slot `{}` at line {}: {}",
                                slot.slot_name, diagnostic.line, diagnostic.message
                            )
                        })
                }),
        );
        let messages = failures.join("; ");
        return Err(format!(
            "typecheck failed with {} error diagnostic(s): {messages}",
            failures.len(),
        ));
    }
    let checked_program = check_output
        .program
        .ok_or_else(|| "typecheck produced no CheckedProgram for valid source".to_owned())?;
    lower_checked(checked_program, producer_requests)
}

pub fn lower_checked(
    mut checked_program: boon_typecheck::CheckedProgram,
    producer_requests: &[ProducerFunctionLoweringRequest],
) -> Result<ErasedProgram, String> {
    let trace_lower = std::env::var_os("BOON_IR_LOWER_TRACE").is_some();
    let trace_phase = |phase: &str, elapsed_ms: f64| {
        if trace_lower {
            eprintln!("boon_ir lower {phase}: {elapsed_ms:.3}ms");
        }
    };
    validate_checked_program_for_lowering(&checked_program)?;
    let source_expression_count = checked_program
        .lowering_metadata
        .original_source_expression_count;
    let producer_roots =
        elaborate_producer_function_roots(&mut checked_program, producer_requests)?;
    let out_net = out_net::OutNet::build_with_producer_roots(&checked_program, producer_roots);
    if out_net.has_errors() {
        return Err(out_net
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "));
    }
    let pending_distributed_references =
        distributed_references(&checked_program, &checked_program.external_types)?;
    let mut distributed_references = DistributedReferences {
        value_references: pending_distributed_references.value_references.clone(),
        calls: Vec::new(),
    };
    // Row scopes and list storage are structural products of checked
    // contextual materializations. Parser-discovered row-scope metadata was a
    // second ownership model and was discarded by
    // `materialize_typed_derived_list_storage` later in this pass.
    let mut row_scopes = Vec::new();
    let sources_started = Instant::now();
    let mut sources = checked_program
        .sources
        .iter()
        .enumerate()
        .map(|(id, source)| SourcePort {
            id: SourceId(id),
            binding_path: source.binding_path.clone(),
            executable_source_id: None,
            static_owner: None,
            source_expr_id: source
                .expression
                .map(|expression| ExprId(expression.0 as usize)),
            source_line: source.line,
            scoped: source.scoped,
            scope_id: None,
            interval_ms: source.interval_ms,
            payload_schema: source_payload_schema(&checked_program.lowering_metadata, &source.path),
            path: source.path.clone(),
        })
        .collect::<Vec<_>>();
    let mut source_paths = sources
        .iter()
        .map(|source| source.path.clone())
        .collect::<BTreeSet<_>>();
    for reference in distributed_references
        .value_references
        .iter()
        .filter(|reference| {
            matches!(
                reference.flow_mode,
                boon_typecheck::FlowMode::TickPresent | boon_typecheck::FlowMode::PresentOrAbsent
            )
        })
    {
        let path = distributed_event_source_path(&reference.canonical_path);
        if !source_paths.insert(path.clone()) {
            continue;
        }
        sources.push(SourcePort {
            id: SourceId(sources.len()),
            binding_path: path.clone(),
            executable_source_id: None,
            static_owner: None,
            source_expr_id: None,
            source_line: 0,
            path,
            scoped: false,
            scope_id: None,
            interval_ms: None,
            payload_schema: SourcePayloadSchema {
                fields: Vec::new(),
                typed_fields: Vec::new(),
            },
        });
    }
    let sources_ms = lower_elapsed_ms(sources_started);
    trace_phase("sources", sources_ms);
    // Executable expansion is the sole authority for state and list resource
    // allocation. The old parser-derived records were overwritten below and
    // could disagree with contextual expansion before being discarded.
    let mut lists = Vec::new();
    let contextual_materializations_started = Instant::now();
    let (mut materializations, materialization_expressions) =
        contextual_materializations(&checked_program, &out_net.graph)?;
    trace_phase(
        "contextual_materializations",
        lower_elapsed_ms(contextual_materializations_started),
    );
    let executable_started = Instant::now();
    let mut executable = contextual_expansion::derive_executable_program(
        &checked_program,
        &out_net.graph,
        &materializations,
        materialization_expressions,
    )
    .map_err(|error| error.to_string())?;
    trace_phase(
        "derive_executable_program",
        lower_elapsed_ms(executable_started),
    );
    distributed_references.calls = concrete_distributed_calls(
        &executable,
        &materializations,
        &pending_distributed_references.calls,
    )?;
    bind_distributed_reference_aliases(
        &checked_program,
        &executable,
        &mut distributed_references.value_references,
    )?;
    let derived_list_storage_started = Instant::now();
    let derived_list_storage = materialize_typed_derived_list_storage(
        &checked_program,
        &executable,
        &materializations,
        &mut row_scopes,
        &mut lists,
    )?;
    trace_phase(
        "materialize_typed_derived_list_storage",
        lower_elapsed_ms(derived_list_storage_started),
    );
    let materialization_target_lists = materialization_target_lists(
        &executable,
        &materializations,
        &derived_list_storage,
        &out_net.graph.static_owners,
    )?;
    bind_contextual_materialization_targets(
        &materialization_target_lists,
        &lists,
        &mut materializations,
    )?;
    let list_projections_started = Instant::now();
    let list_projections = executable_list_projections(
        &executable,
        &materializations,
        &derived_list_storage,
        &lists,
    )?;
    trace_phase(
        "list_projections",
        lower_elapsed_ms(list_projections_started),
    );
    let (mut state_cells, mut resource_aliases) = bind_executable_state_resources(
        &checked_program,
        &executable,
        &materialization_target_lists,
        &lists,
    )?;
    let source_aliases = bind_executable_source_resources(
        &checked_program,
        &executable,
        &materialization_target_lists,
        &lists,
        checked_program.sources.len(),
        &mut sources,
    )?;
    merge_resource_aliases(&mut resource_aliases, source_aliases)?;
    resource_aliases.bind_owner_parents(&out_net.graph.static_owners)?;
    bind_contextual_materialization_storage(
        &executable,
        &out_net.graph.static_owners,
        &derived_list_storage,
        &row_scopes,
        &lists,
        &sources,
        &state_cells,
        &mut materializations,
    )?;
    let list_mutations_started = Instant::now();
    let list_mutations = exact_list_mutations(
        &executable,
        &out_net.graph.static_owners,
        &derived_list_storage,
        &row_scopes,
        &sources,
        &state_cells,
        &materializations,
    )?;
    trace_phase("list_mutations", lower_elapsed_ms(list_mutations_started));
    let producer_result_owners = producer_result_owners(&out_net.graph)?;
    bind_producer_invocation_source_owners(
        &out_net.graph,
        &executable,
        &producer_result_owners,
        &mut sources,
    )?;
    let mut erased_fields = build_erased_fields(
        &checked_program,
        &executable,
        &lists,
        &derived_list_storage,
        &materializations,
        &list_mutations,
        &producer_result_owners,
    )?;
    let producer_function_instances = concrete_producer_function_instances(
        &out_net.graph,
        &executable,
        &erased_fields,
        &sources,
    )?;
    bind_indexed_state_fields(
        &checked_program,
        &executable,
        &lists,
        &state_cells,
        &mut erased_fields,
    )?;
    let derived_values_started = Instant::now();
    let mut derived_values = derived_values(
        &checked_program,
        &executable,
        &out_net.graph.static_owners,
        &row_scopes,
        &derived_list_storage,
        &erased_fields,
        &state_cells,
        &sources,
        &materializations,
        &producer_function_instances,
        &distributed_references.value_references,
    )?;
    derived_values.extend(producer_derived_values(
        &executable,
        &out_net.graph.static_owners,
        &row_scopes,
        &derived_list_storage,
        &state_cells,
        &sources,
        &materializations,
        &producer_function_instances,
    )?);
    let state_update_arms = state_update_arms(
        &executable,
        &out_net.graph.static_owners,
        &row_scopes,
        &derived_list_storage,
        &sources,
        &state_cells,
        &materializations,
    )?;
    let dependencies_started = Instant::now();
    let mut dependencies = exact_dependency_edges(&state_update_arms, &sources, &state_cells)?;
    let dependencies_ms = lower_elapsed_ms(dependencies_started);
    trace_phase("dependency_edges", dependencies_ms);
    let possible_causes_started = Instant::now();
    let mut possible_causes = exact_possible_causes(&state_update_arms, &sources, &state_cells)?;
    let possible_causes_ms = lower_elapsed_ms(possible_causes_started);
    trace_phase("possible_causes", possible_causes_ms);
    verify_executable_host_effect_calls_scheduled(&executable, &state_update_arms)?;
    canonicalize_runtime_resource_metadata(
        &mut dependencies,
        &mut possible_causes,
        &mut state_cells,
        &sources,
        &resource_aliases,
    )?;
    let derived_values_ms = lower_elapsed_ms(derived_values_started);
    trace_phase("derived_values", derived_values_ms);
    bind_derived_field_ids(&mut derived_values, &erased_fields)?;
    let semantic_fields = semantic_field_entries(&erased_fields, &derived_values, &state_cells);
    let mut scope_index = build_erased_scope_index(
        &executable,
        &out_net.graph.static_owners,
        &materializations,
        &sources,
        &state_cells,
        &lists,
        &derived_list_storage,
        std::mem::take(&mut erased_fields),
    )?;
    mark_forwarded_executable_resource_fields(&mut executable, &scope_index)?;
    bind_detached_state_captures(
        &executable,
        &materializations,
        &sources,
        &lists,
        &state_update_arms,
        &mut scope_index,
    )?;
    let read_roots = executable_read_root_bindings(
        &checked_program,
        &executable,
        &materializations,
        &scope_index,
        &out_net.graph.static_owners,
    )?;
    scope_index.reads = build_erased_read_bindings(
        &executable,
        &out_net.graph.static_owners,
        &materializations,
        &sources,
        &scope_index,
        &distributed_references,
        &read_roots,
    )?;
    scope_index.row_values = build_erased_row_values(&executable, &materializations, &scope_index)?;
    scope_index.dependencies = build_erased_dependency_uses(
        &executable,
        &materializations,
        &distributed_references,
        &scope_index,
    )?;
    bind_distributed_call_invocation_arms(
        &executable,
        &out_net.graph.static_owners,
        &scope_index,
        &derived_list_storage,
        &row_scopes,
        &sources,
        &state_cells,
        &materializations,
        &mut distributed_references.calls,
    )?;
    let output_values_started = Instant::now();
    let output_values = output_root_values(
        &checked_program,
        &checked_program.lowering_metadata,
        &executable,
        &scope_index,
    )?;
    let output_values_ms = lower_elapsed_ms(output_values_started);
    trace_phase("output_values", output_values_ms);
    let view_bindings_started = Instant::now();
    let view_bindings = view_bindings(
        &executable,
        &out_net.graph.static_owners,
        &scope_index,
        &derived_list_storage,
        &output_values,
        &row_scopes,
        &sources,
        &state_cells,
        &materializations,
    )?;
    let view_bindings_ms = lower_elapsed_ms(view_bindings_started);
    trace_phase("view_bindings", view_bindings_ms);
    let expression_coverage_started = Instant::now();
    let expression_coverage = expression_coverage(
        &checked_program,
        source_expression_count,
        &executable,
        &lists,
        &derived_values,
        &distributed_references,
    );
    let expression_coverage_ms = lower_elapsed_ms(expression_coverage_started);
    trace_phase("expression_coverage", expression_coverage_ms);
    let semantic_index_started = Instant::now();
    let semantic_index = semantic_index(
        &checked_program,
        &row_scopes,
        &sources,
        &lists,
        &view_bindings,
        &output_values,
        &checked_program.lowering_metadata,
        semantic_fields,
    );
    let semantic_index_ms = lower_elapsed_ms(semantic_index_started);
    trace_phase("semantic_index", semantic_index_ms);
    let semantic_migration_started = Instant::now();
    let (semantic_memory, migration_edges) = lower_semantic_memory_and_migrations(
        &checked_program,
        &executable,
        &state_cells,
        &lists,
        &materializations,
        &derived_list_storage,
        &scope_index,
    )?;
    let semantic_migration_ms = lower_elapsed_ms(semantic_migration_started);
    trace_phase("semantic_memory_and_migrations", semantic_migration_ms);
    let graph_node_count = executable.expressions.len();
    let typed = ErasedProgram {
        executable,
        scope_index,
        expression_count: source_expression_count,
        expression_coverage,
        distributed_references,
        producer_function_instances,
        semantic_index,
        graph_node_count,
        row_scopes,
        sources,
        host_ports: host_port_declarations(&checked_program.lowering_metadata),
        output_values,
        dependencies,
        possible_causes,
        state_update_arms,
        list_mutations,
        list_projections,
        materializations,
        view_bindings,
        expression_types: checked_program.lowering_metadata.expr_type_table.clone(),
        function_types: checked_program
            .lowering_metadata
            .function_type_table
            .clone(),
        named_value_types: checked_program
            .lowering_metadata
            .named_value_type_table
            .clone(),
        derived_values,
        state_cells,
        lists,
        semantic_memory,
        migration_edges,
        hidden_identity_verified: true,
        static_schedule_verified: true,
    };
    let verify_static_started = Instant::now();
    verify_erased_scope_index(&typed)?;
    verify_static_schedule(&typed)?;
    let verify_static_ms = lower_elapsed_ms(verify_static_started);
    trace_phase("verify_static_schedule", verify_static_ms);
    let verify_hidden_started = Instant::now();
    verify_hidden_identity(&typed)?;
    let verify_hidden_ms = lower_elapsed_ms(verify_hidden_started);
    trace_phase("verify_hidden_identity", verify_hidden_ms);
    Ok(typed)
}

fn producer_result_owners(
    out_net: &out_net::OutNet,
) -> Result<BTreeMap<ExecutableStatementId, StaticOwnerId>, String> {
    out_net
        .producer_roots()
        .iter()
        .map(|root| {
            let call = out_net
                .producer_root_for_identity(root.identity)
                .ok_or_else(|| {
                    format!(
                        "producer function identity {} has no concrete root call frame",
                        producer_identity_text(root.identity)
                    )
                })?;
            let instance = out_net
                .call_instances
                .get(call.as_usize())
                .filter(|instance| instance.id == call && instance.parent.is_none())
                .ok_or_else(|| {
                    format!(
                        "producer function identity {} root frame is not static",
                        producer_identity_text(root.identity)
                    )
                })?;
            let owner = instance.owner.ok_or_else(|| {
                format!(
                    "producer function identity {} root frame has no static owner",
                    producer_identity_text(root.identity)
                )
            })?;
            Ok((
                ExecutableStatementId(root.result_statement.0 as usize),
                owner,
            ))
        })
        .collect()
}

fn producer_invocation_source_id(
    producer: &out_net::ProducerRoot,
    executable: &ExecutableProgram,
    sources: &[SourcePort],
) -> Result<Option<SourceId>, String> {
    let Some(checked_expression) = producer.invocation_source_expression else {
        return Ok(None);
    };
    let expression = executable
        .expressions
        .iter()
        .find(|expression| {
            expression.checked_expr_id == checked_expression
                && matches!(expression.kind, ExecutableExpressionKind::Source { .. })
        })
        .ok_or_else(|| {
            format!(
                "producer function identity {} has no executable invocation SOURCE",
                producer_identity_text(producer.identity)
            )
        })?;
    let executable_source = executable
        .sources
        .iter()
        .find(|source| source.expression == expression.id)
        .ok_or_else(|| {
            format!(
                "producer function identity {} invocation expression has no source definition",
                producer_identity_text(producer.identity)
            )
        })?;
    sources
        .iter()
        .find(|source| source.executable_source_id == Some(executable_source.id))
        .map(|source| Some(source.id))
        .ok_or_else(|| {
            format!(
                "producer function identity {} invocation SOURCE has no runtime source",
                producer_identity_text(producer.identity)
            )
        })
}

fn bind_producer_invocation_source_owners(
    out_net: &out_net::OutNet,
    executable: &ExecutableProgram,
    result_owners: &BTreeMap<ExecutableStatementId, StaticOwnerId>,
    sources: &mut [SourcePort],
) -> Result<(), String> {
    for producer in out_net.producer_roots() {
        let Some(source_id) = producer_invocation_source_id(producer, executable, sources)? else {
            continue;
        };
        let statement = ExecutableStatementId(producer.result_statement.0 as usize);
        let owner = result_owners.get(&statement).copied().ok_or_else(|| {
            format!(
                "producer function identity {} invocation SOURCE has no call-site owner",
                producer_identity_text(producer.identity)
            )
        })?;
        let source = sources
            .get_mut(source_id.as_usize())
            .filter(|source| source.id == source_id)
            .ok_or_else(|| "producer invocation source ID is not canonical".to_owned())?;
        source.static_owner = Some(owner);
    }
    Ok(())
}

fn concrete_producer_function_instances(
    out_net: &out_net::OutNet,
    executable: &ExecutableProgram,
    fields: &[ErasedFieldDef],
    sources: &[SourcePort],
) -> Result<Vec<ProducerFunctionInstance>, String> {
    let result_owners = producer_result_owners(out_net)?;
    let mut instances = Vec::with_capacity(out_net.producer_roots().len());
    for producer in out_net.producer_roots() {
        let statement_id = ExecutableStatementId(producer.result_statement.0 as usize);
        let statement = executable
            .statements
            .iter()
            .find(|statement| statement.id == statement_id)
            .ok_or_else(|| {
                format!(
                    "producer function identity {} has no executable result statement {}",
                    producer_identity_text(producer.identity),
                    statement_id
                )
            })?;
        let root = statement.value.ok_or_else(|| {
            format!(
                "producer function identity {} result statement {} has no value",
                producer_identity_text(producer.identity),
                statement_id
            )
        })?;
        let owner = result_owners[&statement_id];
        let result_fields = fields
            .iter()
            .filter(|field| {
                field.statement == Some(statement_id)
                    && field.declaration == Some(producer.result_declaration)
                    && field.diagnostic_path == producer.result_path
                    && field.static_owner == Some(owner)
            })
            .collect::<Vec<_>>();
        let result_field = match result_fields.as_slice() {
            [field] => field.id,
            _ => {
                return Err(format!(
                    "producer function identity {} result has {} exact erased fields",
                    producer_identity_text(producer.identity),
                    result_fields.len()
                ));
            }
        };
        let function = executable
            .functions
            .iter()
            .find(|function| function.id == producer.function)
            .ok_or_else(|| {
                format!(
                    "producer function identity {} has no concrete function {}",
                    producer_identity_text(producer.identity),
                    producer.function
                )
            })?;
        if function.root != root || function.name != producer.function_name {
            return Err(format!(
                "producer function identity {} concrete function metadata differs from its root",
                producer_identity_text(producer.identity)
            ));
        }
        let arguments = producer
            .parameters
            .iter()
            .map(|parameter| {
                let mut input_expressions = executable
                    .expressions
                    .iter()
                    .filter_map(|expression| match expression.kind {
                        ExecutableExpressionKind::FunctionParameter {
                            parameter: candidate,
                            ..
                        } if candidate == parameter.parameter => Some(expression.id),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                input_expressions.sort_unstable();
                input_expressions.dedup();
                ProducerFunctionArgument {
                    name: parameter.name.clone(),
                    parameter: parameter.parameter,
                    flow_type: parameter.flow_type.clone(),
                    input_expressions,
                }
            })
            .collect();
        instances.push(ProducerFunctionInstance {
            identity: producer.identity,
            owner,
            function: producer.function,
            function_name: producer.function_name.clone(),
            result_field,
            result_path: producer.result_path.clone(),
            root,
            mode: producer.mode,
            invocation_source: producer_invocation_source_id(producer, executable, sources)?,
            arguments,
        });
    }
    instances.sort_by_key(|instance| instance.identity);
    Ok(instances)
}

fn build_erased_fields(
    checked: &boon_typecheck::CheckedProgram,
    executable: &ExecutableProgram,
    lists: &[ListMemory],
    list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    materializations: &[ContextualMaterialization],
    list_mutations: &[ListMutation],
    producer_result_owners: &BTreeMap<ExecutableStatementId, StaticOwnerId>,
) -> Result<Vec<ErasedFieldDef>, String> {
    let owner_rows = materializations
        .iter()
        .map(|materialization| {
            let source_row = paired_row_binding(
                materialization.source_list_id,
                materialization.source_scope_id,
                "source",
                materialization.owner,
            )?;
            let target_row = paired_row_binding(
                materialization.target_list_id,
                materialization.target_scope_id,
                "target",
                materialization.owner,
            )?;
            Ok((materialization.owner, target_row.or(source_row)))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let statement_parents = executable_statement_parents(executable);
    let direct_storage_statements = direct_erased_storage_statements(executable);
    let mut fields = Vec::new();
    let mut statement_fields = BTreeMap::new();
    for statement in &executable.statements {
        if !direct_storage_statements.contains(&statement.id) {
            continue;
        }
        let (name, diagnostic_path) = match &statement.kind {
            ExecutableStatementKind::Field { name, path } => (name.clone(), path.clone()),
            ExecutableStatementKind::List {
                name: Some(name),
                path: Some(path),
                ..
            } => (name.clone(), path.clone()),
            ExecutableStatementKind::Source {
                name: Some(name),
                path: Some(path),
                ..
            }
            | ExecutableStatementKind::Hold {
                name: Some(name),
                path: Some(path),
                ..
            } => (name.clone(), path.clone()),
            _ => continue,
        };
        let (Some(declaration), Some(producer), Some(flow_type)) = (
            statement.declaration,
            statement.value,
            statement.flow_type.clone(),
        ) else {
            continue;
        };
        let expression = executable
            .expressions
            .get(producer.as_usize())
            .filter(|expression| expression.id == producer)
            .ok_or_else(|| {
                format!(
                    "field declaration {} statement {} references missing producer {producer}",
                    declaration.0, statement.id
                )
            })?;
        let id = FieldId(fields.len());
        let static_owner = producer_result_owners
            .get(&statement.id)
            .copied()
            .or(expression.owner);
        statement_fields.insert(statement.id, id);
        fields.push(ErasedFieldDef {
            id,
            role: ErasedFieldRole::Value,
            declaration: Some(declaration),
            static_owner,
            parent: None,
            row: static_owner.and_then(|owner| owner_rows.get(&owner).copied().flatten()),
            name,
            diagnostic_path,
            statement: Some(statement.id),
            producer: Some(producer),
            resource_only: false,
            flow_type,
        });
    }
    for field in &mut fields {
        field.parent = field
            .statement
            .and_then(|statement| statement_parents.get(&statement))
            .and_then(|parent| statement_fields.get(parent))
            .copied();
    }

    for (statement, storage) in list_storage {
        let row = ErasedRowBinding {
            list: storage.list_id,
            scope: storage.row_scope_id,
        };
        let list = lists
            .get(storage.list_id.as_usize())
            .filter(|list| list.id == storage.list_id)
            .ok_or_else(|| {
                format!(
                    "typed list statement {statement} references missing ListId {}",
                    storage.list_id
                )
            })?;
        if list.row_scope_id != Some(storage.row_scope_id) {
            return Err(format!(
                "typed list statement {statement} row scope {} differs from ListId {}",
                storage.row_scope_id, storage.list_id
            ));
        }
        let owners = materializations
            .iter()
            .filter(|materialization| materialization.target_list_id == Some(storage.list_id))
            .map(|materialization| materialization.owner)
            .collect::<BTreeSet<_>>();
        let owner = match owners.into_iter().collect::<Vec<_>>().as_slice() {
            [] => None,
            [owner] => Some(*owner),
            _ => None,
        };
        let parent = statement_fields.get(statement).copied();
        let item_shape = match &storage.item_type {
            boon_typecheck::Type::Object(shape) => Some(shape),
            _ => None,
        };
        for name in &storage.item_fields {
            if fields
                .iter()
                .any(|field| field.row == Some(row) && field.name == *name)
            {
                continue;
            }
            let candidates = exact_record_field_candidates(
                executable,
                materializations,
                storage.list_id,
                *statement,
                name,
            )?;
            let (declaration, producer, field_owner, resource_only) = match candidates.as_slice() {
                [] => (None, None, owner, false),
                [(declaration, producer, field_owner, resource_only)] => (
                    Some(*declaration),
                    Some(*producer),
                    field_owner.or(owner),
                    *resource_only,
                ),
                _ => (None, None, owner, false),
            };
            if executable.sources.iter().any(|source| {
                Some(source.declaration) == declaration
                    && Some(source.expression) == producer
                    && source.owner == field_owner
            }) {
                continue;
            }
            let flow_type = boon_typecheck::FlowType {
                mode: boon_typecheck::FlowMode::Continuous,
                ty: item_shape
                    .and_then(|shape| shape.fields.get(name))
                    .cloned()
                    .unwrap_or(boon_typecheck::Type::Unknown),
            };
            fields.push(ErasedFieldDef {
                id: FieldId(fields.len()),
                role: ErasedFieldRole::Value,
                declaration,
                static_owner: field_owner,
                parent,
                row: Some(row),
                name: name.clone(),
                diagnostic_path: format!("{}.{}", list.name, name),
                statement: Some(*statement),
                producer,
                resource_only,
                flow_type,
            });
        }
    }

    let indexed_state_fields = fields
        .iter()
        .filter(|field| field.row.is_some())
        .filter(|field| {
            executable.states.iter().any(|state| {
                field.declaration == Some(state.declaration) && field.static_owner == state.owner
            })
        })
        .map(|field| field.id)
        .collect::<BTreeSet<_>>();

    for (statement, storage) in list_storage {
        let row = ErasedRowBinding {
            list: storage.list_id,
            scope: storage.row_scope_id,
        };
        let list = lists
            .get(storage.list_id.as_usize())
            .filter(|list| list.id == storage.list_id)
            .ok_or_else(|| {
                format!(
                    "typed list statement {statement} references missing ListId {}",
                    storage.list_id
                )
            })?;
        let mut authority_types = BTreeMap::new();
        let mut direct_constructor_fields = BTreeSet::new();
        for item in list_mutations
            .iter()
            .filter(|mutation| mutation.list_id == storage.list_id)
            .filter_map(|mutation| match &mutation.kind {
                ListMutationKind::Append { item, .. } => Some(*item),
                ListMutationKind::Remove { .. } => None,
            })
        {
            for (name, ty) in exact_list_item_field_types(executable, item)?
                .into_iter()
                .filter(|(_, ty)| distributed_type_is_closed(ty))
            {
                direct_constructor_fields.insert(name.clone());
                merge_authority_field_type(&mut authority_types, &name, ty)?;
            }
        }
        for materialization in materializations.iter().filter(|materialization| {
            materialization.operation == ContextualOperationKind::Map
                && materialization.target_list_id == Some(storage.list_id)
                && (materialization
                    .source_list_id
                    .is_none_or(|source| source == storage.list_id)
                    || contextual_materialization_transfers_list_authority(
                        executable,
                        materialization,
                    ))
        }) {
            if let boon_typecheck::Type::Object(shape) = &materialization.item_type {
                for (name, ty) in shape.fields.iter().filter(|(name, ty)| {
                    distributed_type_is_closed(ty)
                        && !row_field_contains_source(executable, &fields, row, name)
                }) {
                    direct_constructor_fields.insert(name.clone());
                    merge_authority_field_type(&mut authority_types, name, ty.clone())?;
                }
            }
        }
        let derives_distinct_map_rows = matches!(
            list.initializer,
            ListInitializer::Empty | ListInitializer::Unknown { .. }
        ) && materializations.iter().any(|materialization| {
            materialization.operation == ContextualOperationKind::Map
                && materialization.target_list_id == Some(storage.list_id)
        });
        if derives_distinct_map_rows && let boon_typecheck::Type::Object(shape) = &storage.item_type
        {
            for (name, ty) in shape.fields.iter().filter(|(name, ty)| {
                distributed_type_is_closed(ty)
                    && !row_field_contains_source(executable, &fields, row, name)
            }) {
                merge_authority_field_type(&mut authority_types, name, ty.clone())?;
            }
        }
        match &list.initializer {
            ListInitializer::RecordLiteral { rows } => {
                for field in rows.iter().flat_map(|row| &row.fields) {
                    direct_constructor_fields.insert(field.name.clone());
                    merge_authority_field_type(
                        &mut authority_types,
                        &field.name,
                        initial_value_checked_type(&field.value),
                    )?;
                }
            }
            ListInitializer::Range { .. } => {
                for name in ["index", "value"] {
                    direct_constructor_fields.insert(name.to_owned());
                    merge_authority_field_type(
                        &mut authority_types,
                        name,
                        boon_typecheck::Type::Number,
                    )?;
                }
            }
            ListInitializer::Empty | ListInitializer::Unknown { .. } => {}
        }
        let parent = statement_fields.get(statement).copied();
        for (name, ty) in authority_types {
            if fields.iter().any(|field| {
                field.row == Some(row)
                    && field.name == name
                    && indexed_state_fields.contains(&field.id)
            }) && !direct_constructor_fields.contains(&name)
            {
                continue;
            }
            let shared = fields
                .iter()
                .enumerate()
                .filter(|(_, field)| {
                    field.row == Some(row)
                        && field.name == name
                        && field.role == ErasedFieldRole::Value
                        && field.static_owner.is_none()
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if let [index] = shared.as_slice() {
                let field = &mut fields[*index];
                merge_value_authority_field_type(
                    &field.diagnostic_path,
                    &mut field.flow_type.ty,
                    ty,
                )?;
                field.role = ErasedFieldRole::ValueAuthority;
                continue;
            }
            fields.push(ErasedFieldDef {
                id: FieldId(fields.len()),
                role: ErasedFieldRole::ListAuthority,
                declaration: None,
                static_owner: None,
                parent,
                row: Some(row),
                name: name.clone(),
                diagnostic_path: format!("@authority/{}/{name}", storage.list_id.as_usize()),
                statement: Some(*statement),
                producer: None,
                resource_only: false,
                flow_type: boon_typecheck::FlowType {
                    mode: boon_typecheck::FlowMode::Continuous,
                    ty,
                },
            });
        }
    }

    let mut parent_index = 0;
    while parent_index < fields.len() {
        let parent = fields[parent_index].clone();
        parent_index += 1;
        let Some(producer) = parent.producer else {
            continue;
        };
        let expression = executable
            .expressions
            .get(producer.as_usize())
            .filter(|expression| expression.id == producer)
            .ok_or_else(|| {
                format!(
                    "erased field {} references missing record producer {producer}",
                    parent.id
                )
            })?;
        let record_fields = match &expression.kind {
            ExecutableExpressionKind::Object(record_fields)
            | ExecutableExpressionKind::Record(record_fields)
            | ExecutableExpressionKind::TaggedObject {
                fields: record_fields,
                ..
            } => record_fields,
            _ => continue,
        };
        for record_field in record_fields.iter().filter(|field| !field.spread) {
            if fields.iter().any(|field| {
                field.parent == Some(parent.id)
                    && field.name == record_field.name
                    && field.producer == Some(record_field.value)
            }) {
                continue;
            }
            let value = executable
                .expressions
                .get(record_field.value.as_usize())
                .filter(|expression| expression.id == record_field.value)
                .ok_or_else(|| {
                    format!(
                        "erased record field `{}.{}` references missing producer {}",
                        parent.diagnostic_path, record_field.name, record_field.value
                    )
                })?;
            fields.push(ErasedFieldDef {
                id: FieldId(fields.len()),
                role: ErasedFieldRole::Value,
                declaration: record_field.declaration,
                static_owner: value.owner.or(parent.static_owner),
                parent: Some(parent.id),
                row: None,
                name: record_field.name.clone(),
                diagnostic_path: format!("{}.{}", parent.diagnostic_path, record_field.name),
                statement: None,
                producer: Some(record_field.value),
                resource_only: record_field.resource_only,
                flow_type: value.flow_type.clone(),
            });
        }
    }

    for field in &fields {
        if let Some(declaration) = field.declaration
            && !checked
                .declarations
                .iter()
                .any(|candidate| candidate.id == declaration)
        {
            return Err(format!(
                "erased FieldId {} references missing checked declaration {}",
                field.id, declaration.0
            ));
        }
    }
    Ok(fields)
}

fn contextual_materialization_transfers_list_authority(
    executable: &ExecutableProgram,
    materialization: &ContextualMaterialization,
) -> bool {
    executable
        .expressions
        .get(materialization.source.as_usize())
        .is_some_and(|expression| {
            expression.id == materialization.source
                && matches!(expression.kind, ExecutableExpressionKind::Drain { .. })
        })
}

fn bind_indexed_state_fields(
    checked: &boon_typecheck::CheckedProgram,
    executable: &ExecutableProgram,
    lists: &[ListMemory],
    states: &[StateCell],
    fields: &mut Vec<ErasedFieldDef>,
) -> Result<(), String> {
    for state in states
        .iter()
        .filter(|state| state.published && state.scope_id.is_some())
    {
        let executable_state = state
            .executable_state_id
            .and_then(|state| executable.states.get(state.as_usize()))
            .filter(|candidate| Some(candidate.id) == state.executable_state_id)
            .ok_or_else(|| format!("indexed StateId {} has no exact executable state", state.id))?;
        let expression = executable
            .expressions
            .get(executable_state.expression.as_usize())
            .filter(|expression| expression.id == executable_state.expression)
            .ok_or_else(|| {
                format!(
                    "indexed executable state {} has no producer {}",
                    executable_state.id, executable_state.expression
                )
            })?;
        let declaration = checked
            .declarations
            .iter()
            .find(|declaration| declaration.id == executable_state.declaration)
            .ok_or_else(|| {
                format!(
                    "indexed executable state {} references missing declaration {}",
                    executable_state.id, executable_state.declaration.0
                )
            })?;
        let row_scope = state
            .scope_id
            .expect("filtered indexed state has row scope");
        let matching_lists = lists
            .iter()
            .filter(|list| list.row_scope_id == Some(row_scope))
            .collect::<Vec<_>>();
        let [list] = matching_lists.as_slice() else {
            return Err(format!(
                "indexed executable state {} scope {} belongs to {} lists",
                executable_state.id,
                row_scope,
                matching_lists.len()
            ));
        };
        let row = ErasedRowBinding {
            list: list.id,
            scope: row_scope,
        };
        let field_name = state
            .semantic_path
            .as_deref()
            .and_then(|path| path.rsplit('.').next())
            .filter(|name| !name.is_empty())
            .unwrap_or(&declaration.name);
        if fields.iter().any(|field| {
            field.row == Some(row)
                && field.declaration == Some(executable_state.declaration)
                && field.static_owner == executable_state.owner
                && field.producer == Some(executable_state.expression)
        }) {
            continue;
        }
        let placeholders = fields
            .iter()
            .enumerate()
            .filter(|(_, field)| {
                field.role == ErasedFieldRole::Value
                    && field.row == Some(row)
                    && field.name == field_name
                    && field.declaration.is_none()
                    && field.producer.is_none()
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let diagnostic_path = state
            .semantic_path
            .clone()
            .unwrap_or_else(|| format!("{}.{}", list.name, declaration.name));
        match placeholders.as_slice() {
            [index] => {
                let field = &mut fields[*index];
                field.declaration = Some(executable_state.declaration);
                field.static_owner = executable_state.owner;
                field.producer = Some(executable_state.expression);
                field.resource_only = false;
                field.flow_type = expression.flow_type.clone();
                field.diagnostic_path = diagnostic_path;
            }
            [] => fields.push(ErasedFieldDef {
                id: FieldId(fields.len()),
                role: ErasedFieldRole::Value,
                declaration: Some(executable_state.declaration),
                static_owner: executable_state.owner,
                parent: None,
                row: Some(row),
                name: field_name.to_owned(),
                diagnostic_path,
                statement: None,
                producer: Some(executable_state.expression),
                resource_only: false,
                flow_type: expression.flow_type.clone(),
            }),
            _ => {
                return Err(format!(
                    "indexed executable state {} (`{}`) has {} unowned row-field placeholders",
                    executable_state.id,
                    diagnostic_path,
                    placeholders.len()
                ));
            }
        }
    }
    Ok(())
}

fn executable_statement_parents(
    executable: &ExecutableProgram,
) -> BTreeMap<ExecutableStatementId, ExecutableStatementId> {
    executable
        .statements
        .iter()
        .flat_map(|parent| parent.children.iter().map(move |child| (*child, parent.id)))
        .collect()
}

fn direct_erased_storage_statements(
    executable: &ExecutableProgram,
) -> BTreeSet<ExecutableStatementId> {
    let parents = executable_statement_parents(executable);
    executable
        .statements
        .iter()
        .filter(|statement| {
            let Some(parent) = parents.get(&statement.id) else {
                return true;
            };
            executable
                .statements
                .iter()
                .find(|candidate| candidate.id == *parent)
                .is_some_and(|parent| {
                    parent.declaration.is_some()
                        && matches!(parent.kind, ExecutableStatementKind::Field { .. })
                })
        })
        .map(|statement| statement.id)
        .collect()
}

fn row_field_contains_source(
    executable: &ExecutableProgram,
    fields: &[ErasedFieldDef],
    row: ErasedRowBinding,
    name: &str,
) -> bool {
    fields
        .iter()
        .filter(|field| field.row == Some(row) && field.name == name && field.role.is_value())
        .filter_map(|field| field.producer)
        .any(|producer| executable_expression_contains_source(executable, producer))
}

fn executable_expression_contains_source(
    executable: &ExecutableProgram,
    root: ExecutableExprId,
) -> bool {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression) = pending.pop() {
        if !visited.insert(expression) {
            continue;
        }
        let Some(expression) = executable
            .expressions
            .get(expression.as_usize())
            .filter(|candidate| candidate.id == expression)
        else {
            continue;
        };
        if matches!(&expression.kind, ExecutableExpressionKind::Source { .. }) {
            return true;
        }
        pending.extend(executable_expression_children(&expression.kind));
    }
    false
}

fn exact_record_field_candidates(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    list: ListId,
    statement: ExecutableStatementId,
    name: &str,
) -> Result<
    Vec<(
        boon_typecheck::DeclId,
        ExecutableExprId,
        Option<StaticOwnerId>,
        bool,
    )>,
    String,
> {
    let mut roots = materializations
        .iter()
        .filter(|materialization| materialization.target_list_id == Some(list))
        .map(|materialization| materialization.body)
        .collect::<Vec<_>>();
    if let Some(root) = executable
        .statements
        .iter()
        .find(|candidate| candidate.id == statement)
        .and_then(|candidate| candidate.value)
    {
        roots.push(root);
    }
    let mut pending = roots;
    let mut visited = BTreeSet::new();
    let mut candidates = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        let expression = executable
            .expressions
            .get(expression_id.as_usize())
            .filter(|expression| expression.id == expression_id)
            .ok_or_else(|| format!("field discovery reaches missing expression {expression_id}"))?;
        match &expression.kind {
            ExecutableExpressionKind::Object(fields)
            | ExecutableExpressionKind::Record(fields)
            | ExecutableExpressionKind::TaggedObject { fields, .. } => {
                for field in fields {
                    if !field.spread
                        && field.name == name
                        && let Some(declaration) = field.declaration
                    {
                        candidates.insert((
                            declaration,
                            field.value,
                            expression.owner,
                            field.resource_only,
                        ));
                    }
                }
            }
            _ => {}
        }
        pending.extend(executable_expression_children(&expression.kind));
    }
    Ok(candidates.into_iter().collect())
}

fn preceding_map_owner(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    mut expression: ExecutableExprId,
) -> Result<Option<StaticOwnerId>, String> {
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(expression) {
            return Err(format!(
                "contextual source expression {expression} contains a materialization cycle"
            ));
        }
        let value = executable
            .expressions
            .get(expression.as_usize())
            .filter(|candidate| candidate.id == expression)
            .ok_or_else(|| {
                format!("contextual source references missing expression {expression}")
            })?;
        match value.kind {
            ExecutableExpressionKind::Materialize { materialization } => {
                let materialization = materializations
                    .get(materialization)
                    .filter(|candidate| candidate.id == materialization)
                    .ok_or_else(|| {
                        format!(
                            "contextual source expression {expression} references missing materialization {materialization}"
                        )
                    })?;
                if materialization.operation == ContextualOperationKind::Map {
                    return Ok(Some(materialization.owner));
                }
                expression = materialization.source;
            }
            ExecutableExpressionKind::Draining { input }
            | ExecutableExpressionKind::Project { input, .. } => expression = input,
            ExecutableExpressionKind::Block { result, .. } => expression = result,
            _ => return Ok(None),
        }
    }
}

fn merge_authority_field_type(
    fields: &mut BTreeMap<String, boon_typecheck::Type>,
    name: &str,
    candidate: boon_typecheck::Type,
) -> Result<(), String> {
    match fields.entry(name.to_owned()) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(candidate);
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            let merged = compatible_storage_type(entry.get(), &candidate).ok_or_else(|| {
                format!(
                    "list authority field `{name}` has incompatible checked types {:?} and {candidate:?}",
                    entry.get()
                )
            })?;
            entry.insert(merged);
        }
    }
    Ok(())
}

fn merge_value_authority_field_type(
    path: &str,
    value: &mut boon_typecheck::Type,
    authority: boon_typecheck::Type,
) -> Result<(), String> {
    let merged = compatible_storage_type(value, &authority).ok_or_else(|| {
        format!(
            "list field `{path}` has incompatible value and authority types {value:?} and {authority:?}"
        )
    })?;
    *value = merged;
    Ok(())
}

fn compatible_storage_type(
    left: &boon_typecheck::Type,
    right: &boon_typecheck::Type,
) -> Option<boon_typecheck::Type> {
    if *left == boon_typecheck::Type::Unknown {
        return Some(right.clone());
    }
    if *right == boon_typecheck::Type::Unknown || left == right {
        return Some(left.clone());
    }
    match (left, right) {
        (boon_typecheck::Type::VariantSet(existing), boon_typecheck::Type::VariantSet(extra)) => {
            let mut variants = existing.clone();
            variants.extend(extra.iter().cloned());
            variants.sort_by_key(authority_variant_sort_key);
            variants.dedup();
            Some(boon_typecheck::Type::VariantSet(variants))
        }
        (boon_typecheck::Type::Bytes(_), boon_typecheck::Type::Bytes(_)) => Some(
            boon_typecheck::Type::Bytes(boon_typecheck::BytesType::Dynamic),
        ),
        _ => None,
    }
}

fn authority_variant_sort_key(variant: &boon_typecheck::Variant) -> String {
    match variant {
        boon_typecheck::Variant::Tag(tag) => format!("0:{tag}"),
        boon_typecheck::Variant::Tagged { tag, fields } => {
            format!("1:{tag}:{}", fields.fields.len())
        }
    }
}

fn initial_value_checked_type(value: &InitialValue) -> boon_typecheck::Type {
    match value {
        InitialValue::Text { .. } => boon_typecheck::Type::Text,
        InitialValue::Number { .. } => boon_typecheck::Type::Number,
        InitialValue::Bool { value } => {
            boon_typecheck::Type::VariantSet(vec![boon_typecheck::Variant::Tag(
                if *value { "True" } else { "False" }.to_owned(),
            )])
        }
        InitialValue::Bytes { fixed_len, .. } => boon_typecheck::Type::Bytes(
            fixed_len.map_or(boon_typecheck::BytesType::Dynamic, |length| {
                boon_typecheck::BytesType::Fixed(length)
            }),
        ),
        InitialValue::Enum { value } => {
            boon_typecheck::Type::VariantSet(vec![boon_typecheck::Variant::Tag(value.clone())])
        }
        InitialValue::Data { .. }
        | InitialValue::RootInitialField { .. }
        | InitialValue::RowInitialField { .. }
        | InitialValue::Unknown { .. } => boon_typecheck::Type::Unknown,
    }
}

fn static_owner_descends_from(
    mut candidate: StaticOwnerId,
    ancestor: StaticOwnerId,
    owners: &[StaticOwnerDef],
) -> bool {
    loop {
        if candidate == ancestor {
            return true;
        }
        let Some(parent) = owners
            .get(candidate.as_usize())
            .filter(|owner| owner.id == candidate)
            .and_then(|owner| owner.parent)
        else {
            return false;
        };
        candidate = parent;
    }
}

fn erased_local_members(
    executable: &ExecutableProgram,
    static_owners: &[StaticOwnerDef],
    materializations: &[ContextualMaterialization],
    fields: &[ErasedFieldDef],
    sources: &[SourcePort],
    states: &[StateCell],
    lists: &[ListMemory],
    materialization: &ContextualMaterialization,
    row: Option<ErasedRowBinding>,
) -> Result<Vec<ErasedLocalMember>, String> {
    let Some(row) = row else {
        return Ok(Vec::new());
    };
    let preceding_owner =
        preceding_map_owner(executable, materializations, materialization.source)?;
    let list = lists
        .iter()
        .find(|list| list.id == row.list)
        .ok_or_else(|| {
            format!(
                "contextual owner {} local {} references missing ListId {}",
                materialization.owner, materialization.row_local.0, row.list
            )
        })?;
    let relative_path = |path: &str| -> Result<Vec<String>, String> {
        path.strip_prefix(&list.name)
            .and_then(|suffix| suffix.strip_prefix('.'))
            .filter(|suffix| !suffix.is_empty())
            .map(|suffix| suffix.split('.').map(str::to_owned).collect())
            .ok_or_else(|| {
                format!(
                    "row-scoped resource `{path}` is not structurally owned by list `{}`",
                    list.name
                )
            })
    };
    let mut members = BTreeMap::<Vec<String>, ErasedLocalMemberTarget>::new();
    for source in sources
        .iter()
        .filter(|source| source.scope_id == Some(row.scope))
    {
        let path = relative_path(&source.path)?;
        if let Some(previous) =
            members.insert(path.clone(), ErasedLocalMemberTarget::Source(source.id))
        {
            return Err(format!(
                "contextual owner {} local {} path `{}` resolves to both {previous:?} and source {}",
                materialization.owner,
                materialization.row_local.0,
                path.join("."),
                source.id
            ));
        }
    }
    for state in states
        .iter()
        .filter(|state| state.scope_id == Some(row.scope) && state.published)
    {
        let semantic_path = state.semantic_path.as_deref().ok_or_else(|| {
            format!(
                "published row-scoped state {} has no semantic path",
                state.id
            )
        })?;
        let path = relative_path(semantic_path)?;
        if let Some(previous) =
            members.insert(path.clone(), ErasedLocalMemberTarget::State(state.id))
        {
            return Err(format!(
                "contextual owner {} local {} path `{}` resolves to both {previous:?} and state {}",
                materialization.owner,
                materialization.row_local.0,
                path.join("."),
                state.id
            ));
        }
    }
    let mut field_names = typed_item_field_names(&materialization.item_type);
    if field_names.is_empty() {
        field_names = fields
            .iter()
            .filter(|field| field.row == Some(row))
            .map(|field| field.name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
    }
    for name in field_names {
        let path = vec![name.clone()];
        if members.contains_key(&path) {
            continue;
        }
        let candidates = fields
            .iter()
            .filter(|field| field.row == Some(row) && field.name == name)
            .collect::<Vec<_>>();
        let preferred = if let Some(owner) = preceding_owner {
            candidates
                .iter()
                .copied()
                .filter(|field| {
                    field.role.is_value()
                        && field.static_owner.is_some_and(|candidate| {
                            static_owner_descends_from(candidate, owner, static_owners)
                        })
                })
                .collect::<Vec<_>>()
        } else {
            let authority = candidates
                .iter()
                .copied()
                .filter(|field| field.role.is_authority())
                .collect::<Vec<_>>();
            if authority.is_empty() {
                let ownerless = candidates
                    .iter()
                    .copied()
                    .filter(|field| field.role.is_value() && field.static_owner.is_none())
                    .collect::<Vec<_>>();
                if ownerless.is_empty() {
                    candidates
                        .iter()
                        .copied()
                        .filter(|field| field.role.is_value())
                        .collect::<Vec<_>>()
                } else {
                    ownerless
                }
            } else {
                authority
            }
        };
        let [field] = preferred.as_slice() else {
            let source = executable
                .expressions
                .get(materialization.source.as_usize())
                .filter(|expression| expression.id == materialization.source);
            return Err(format!(
                "contextual owner {} local {} field `{name}` resolves to {} preferred fields among {:?}; operation={:?}, source={}, checked_source={:?}, resource={:?}, source_kind={:?}, list=`{}` (line {}), preceding_owner={preceding_owner:?}, item_type={:?}",
                materialization.owner,
                materialization.row_local.0,
                preferred.len(),
                candidates
                    .iter()
                    .map(|field| (field.id, field.role, field.static_owner))
                    .collect::<Vec<_>>(),
                materialization.operation,
                materialization.source,
                source.map(|expression| expression.checked_expr_id.0),
                source.and_then(|expression| expression.resource_binding_path.as_deref()),
                source.map(|expression| &expression.kind),
                list.name,
                list.source_line,
                materialization.item_type,
            ));
        };
        members.insert(path, ErasedLocalMemberTarget::Field(field.id));
    }
    Ok(members
        .into_iter()
        .map(|(path, target)| ErasedLocalMember {
            path,
            target,
            forwarded_from: None,
        })
        .collect())
}

fn materialization_local_projection(
    executable: &ExecutableProgram,
    expression: ExecutableExprId,
) -> Result<Option<(StaticOwnerId, MaterializationLocalId, Vec<String>)>, String> {
    fn resolve(
        executable: &ExecutableProgram,
        expression: ExecutableExprId,
        visiting: &mut BTreeSet<ExecutableExprId>,
    ) -> Result<Option<(StaticOwnerId, MaterializationLocalId, Vec<String>)>, String> {
        if !visiting.insert(expression) {
            return Err(format!(
                "materialization-local projection contains a cycle at expression {expression}"
            ));
        }
        let value = executable
            .expressions
            .get(expression.as_usize())
            .filter(|candidate| candidate.id == expression)
            .ok_or_else(|| {
                format!("materialization-local projection references missing {expression}")
            })?;
        let result = match &value.kind {
            ExecutableExpressionKind::MaterializationLocal {
                owner,
                local,
                projection,
            } => Some((*owner, *local, projection.clone())),
            ExecutableExpressionKind::Project { input, fields } => {
                resolve(executable, *input, visiting)?.map(|(owner, local, mut projection)| {
                    projection.extend(fields.iter().cloned());
                    (owner, local, projection)
                })
            }
            ExecutableExpressionKind::Block { result, .. } => {
                resolve(executable, *result, visiting)?
            }
            _ => None,
        };
        visiting.remove(&expression);
        Ok(result)
    }

    resolve(executable, expression, &mut BTreeSet::new())
}

fn executable_expression_is_source_group(
    executable: &ExecutableProgram,
    expression: ExecutableExprId,
) -> Result<bool, String> {
    fn check(
        executable: &ExecutableProgram,
        expression: ExecutableExprId,
        visiting: &mut BTreeSet<ExecutableExprId>,
    ) -> Result<bool, String> {
        if !visiting.insert(expression) {
            return Err(format!(
                "source-group expression contains a cycle at {expression}"
            ));
        }
        let value = executable
            .expressions
            .get(expression.as_usize())
            .filter(|candidate| candidate.id == expression)
            .ok_or_else(|| format!("source-group check references missing {expression}"))?;
        let result = match &value.kind {
            ExecutableExpressionKind::Source { .. } => true,
            ExecutableExpressionKind::Object(fields)
            | ExecutableExpressionKind::Record(fields)
            | ExecutableExpressionKind::TaggedObject { fields, .. } => {
                if fields.is_empty() {
                    false
                } else {
                    let mut all_sources = true;
                    for field in fields {
                        if !check(executable, field.value, visiting)? {
                            all_sources = false;
                            break;
                        }
                    }
                    all_sources
                }
            }
            ExecutableExpressionKind::Block { result, .. } => check(executable, *result, visiting)?,
            _ => false,
        };
        visiting.remove(&expression);
        Ok(result)
    }

    check(executable, expression, &mut BTreeSet::new())
}

fn resource_members_for_projection<'a>(
    local: &'a ErasedLocalDef,
    projection: &[String],
    fields: &[ErasedFieldDef],
) -> Option<Vec<&'a ErasedLocalMember>> {
    let exact_sources = local
        .members
        .iter()
        .filter(|member| {
            member.path == projection && matches!(member.target, ErasedLocalMemberTarget::Source(_))
        })
        .collect::<Vec<_>>();
    if !exact_sources.is_empty() {
        return Some(exact_sources);
    }

    let covering_fields = local
        .members
        .iter()
        .filter_map(|member| match member.target {
            ErasedLocalMemberTarget::Field(field) if projection.starts_with(&member.path) => {
                Some((member.path.len(), field))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if let Some((_, field)) = covering_fields.iter().max_by_key(|(len, _)| *len) {
        let resource_only = fields
            .get(field.as_usize())
            .filter(|candidate| candidate.id == *field)
            .is_some_and(|field| field.resource_only);
        if !resource_only {
            return None;
        }
    }

    let resources = local
        .members
        .iter()
        .filter(|member| {
            member.path.starts_with(projection)
                && matches!(member.target, ErasedLocalMemberTarget::Source(_))
        })
        .collect::<Vec<_>>();
    (!resources.is_empty()).then_some(resources)
}

fn resource_field_local_projection(
    executable: &ExecutableProgram,
    field: &ErasedFieldDef,
    fields: &[ErasedFieldDef],
    materializations: &[ContextualMaterialization],
    locals: &[ErasedLocalDef],
) -> Result<Option<(StaticOwnerId, MaterializationLocalId, Vec<String>)>, String> {
    if let Some(producer) = field.producer
        && let Some(projection) = materialization_local_projection(executable, producer)?
    {
        return Ok(Some(projection));
    }
    let mut candidates = BTreeSet::new();
    if field.role.is_authority() {
        candidates.extend(
            fields
                .iter()
                .filter(|candidate| {
                    candidate.id != field.id
                        && candidate.row == field.row
                        && candidate.name == field.name
                        && candidate.role.is_value()
                        && candidate.resource_only
                })
                .filter_map(|candidate| candidate.producer)
                .map(|producer| materialization_local_projection(executable, producer))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten(),
        );
    }
    if let Some(row) = field.row {
        for materialization in materializations
            .iter()
            .filter(|materialization| materialization.target_list_id == Some(row.list))
        {
            let Some(local) = locals.iter().find(|local| {
                local.owner == materialization.owner && local.local == materialization.row_local
            }) else {
                continue;
            };
            let projection = vec![field.name.clone()];
            if resource_members_for_projection(local, &projection, fields).is_some() {
                candidates.insert((local.owner, local.local, projection));
            }
        }
    }
    let candidates = candidates.into_iter().collect::<Vec<_>>();
    match candidates.as_slice() {
        [] => Ok(None),
        [candidate] => Ok(Some(candidate.clone())),
        _ => {
            let signatures = candidates
                .iter()
                .map(|(owner, local, projection)| {
                    let local = locals
                        .iter()
                        .find(|candidate| candidate.owner == *owner && candidate.local == *local)?;
                    let members = resource_members_for_projection(local, projection, fields)?;
                    let mut signature = members
                        .into_iter()
                        .filter_map(|member| {
                            let ErasedLocalMemberTarget::Source(source) = member.target else {
                                return None;
                            };
                            let suffix = member.path.strip_prefix(projection.as_slice())?;
                            Some((suffix.to_vec(), source))
                        })
                        .collect::<Vec<_>>();
                    signature
                        .sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
                    Some(signature)
                })
                .collect::<Option<Vec<_>>>();
            if signatures
                .as_ref()
                .is_some_and(|signatures| signatures.windows(2).all(|pair| pair[0] == pair[1]))
            {
                return Ok(candidates.first().cloned());
            }
            let details = candidates
                .iter()
                .map(|(owner, local, projection)| {
                    let operation = materializations
                        .iter()
                        .find(|materialization| {
                            materialization.owner == *owner && materialization.row_local == *local
                        })
                        .map(|materialization| materialization.operation);
                    let members = locals
                        .iter()
                        .find(|candidate| candidate.owner == *owner && candidate.local == *local)
                        .and_then(|candidate| {
                            resource_members_for_projection(candidate, projection, fields)
                        })
                        .map(|members| {
                            members
                                .into_iter()
                                .map(|member| {
                                    (
                                        member.path.clone(),
                                        member.target,
                                        member.forwarded_from.clone(),
                                    )
                                })
                                .collect::<Vec<_>>()
                        });
                    (owner, local, projection, operation, members)
                })
                .collect::<Vec<_>>();
            Err(format!(
                "resource FieldId {} has {} forwarding producers: {:?}",
                field.id,
                candidates.len(),
                details
            ))
        }
    }
}

fn propagate_forwarded_local_resources(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    fields: &mut [ErasedFieldDef],
    locals: &mut [ErasedLocalDef],
) -> Result<(), String> {
    let iteration_limit = fields.len().saturating_add(locals.len()).saturating_add(1);
    for _ in 0..iteration_limit {
        let locals_snapshot = locals.to_vec();
        let fields_snapshot = fields.to_vec();
        let mut changed = false;

        let mut newly_resource_only = Vec::new();
        for field in fields_snapshot.iter().filter(|field| !field.resource_only) {
            if field.role.is_authority() {
                let value_fields = fields_snapshot
                    .iter()
                    .filter(|candidate| {
                        candidate.id != field.id
                            && candidate.row == field.row
                            && candidate.name == field.name
                            && candidate.role.is_value()
                    })
                    .collect::<Vec<_>>();
                if !value_fields.is_empty()
                    && value_fields.iter().all(|candidate| candidate.resource_only)
                {
                    newly_resource_only.push(field.id);
                    continue;
                }
            }
            if let Some(row) = field.row
                && materializations
                    .iter()
                    .filter(|materialization| materialization.target_list_id == Some(row.list))
                    .filter_map(|materialization| {
                        locals_snapshot.iter().find(|local| {
                            local.owner == materialization.owner
                                && local.local == materialization.row_local
                        })
                    })
                    .any(|local| {
                        resource_members_for_projection(
                            local,
                            std::slice::from_ref(&field.name),
                            &fields_snapshot,
                        )
                        .is_some()
                    })
            {
                newly_resource_only.push(field.id);
                continue;
            }
            let Some(producer) = field.producer else {
                continue;
            };
            if executable_expression_is_source_group(executable, producer)? {
                newly_resource_only.push(field.id);
                continue;
            }
            let Some((owner, local, projection)) =
                materialization_local_projection(executable, producer)?
            else {
                continue;
            };
            let source = locals_snapshot
                .iter()
                .find(|candidate| candidate.owner == owner && candidate.local == local)
                .ok_or_else(|| {
                    format!(
                        "field {} forwards missing materialization local {}:{}",
                        field.id, owner, local.0
                    )
                })?;
            if resource_members_for_projection(source, &projection, &fields_snapshot).is_some() {
                newly_resource_only.push(field.id);
            }
        }
        for field in newly_resource_only {
            let target = fields
                .get_mut(field.as_usize())
                .filter(|candidate| candidate.id == field)
                .ok_or_else(|| format!("missing forwarded resource FieldId {field}"))?;
            target.resource_only = true;
            changed = true;
        }

        let fields_snapshot = fields.to_vec();
        let locals_snapshot = locals.to_vec();
        for local in locals.iter_mut() {
            let mut members = local
                .members
                .iter()
                .cloned()
                .map(|member| (member.path.clone(), member))
                .collect::<BTreeMap<_, _>>();
            let field_members = local
                .members
                .iter()
                .filter_map(|member| match member.target {
                    ErasedLocalMemberTarget::Field(field) => Some((member.clone(), field)),
                    ErasedLocalMemberTarget::Source(_) | ErasedLocalMemberTarget::State(_) => None,
                })
                .collect::<Vec<_>>();
            for (member, field_id) in field_members {
                let field = fields_snapshot
                    .get(field_id.as_usize())
                    .filter(|candidate| candidate.id == field_id)
                    .ok_or_else(|| {
                        format!(
                            "owner {} local {} member `{}` references missing FieldId {field_id}",
                            local.owner,
                            local.local.0,
                            member.path.join(".")
                        )
                    })?;
                if !field.resource_only {
                    continue;
                }

                if members.remove(&member.path).is_some() {
                    changed = true;
                }

                if let Some((source_owner, source_local, projection)) =
                    resource_field_local_projection(
                        executable,
                        field,
                        &fields_snapshot,
                        materializations,
                        &locals_snapshot,
                    )?
                {
                    let source = locals_snapshot
                        .iter()
                        .find(|candidate| {
                            candidate.owner == source_owner && candidate.local == source_local
                        })
                        .ok_or_else(|| {
                            format!(
                                "resource field {} forwards missing materialization local {}:{}",
                                field.id, source_owner, source_local.0
                            )
                        })?;
                    let resources =
                        resource_members_for_projection(source, &projection, &fields_snapshot)
                            .ok_or_else(|| {
                                format!(
                                    "resource field {} producer {} has no source members at `{}`",
                                    field.id,
                                    field.producer.map_or_else(
                                        || "authority".to_owned(),
                                        |id| id.to_string()
                                    ),
                                    projection.join(".")
                                )
                            })?;
                    for resource in resources {
                        let suffix = resource
                            .path
                            .strip_prefix(projection.as_slice())
                            .ok_or_else(|| {
                                format!(
                                    "resource member `{}` is outside forwarded projection `{}`",
                                    resource.path.join("."),
                                    projection.join(".")
                                )
                            })?;
                        let mut path = member.path.clone();
                        path.extend_from_slice(suffix);
                        let forwarded = ErasedLocalMember {
                            path: path.clone(),
                            target: resource.target,
                            forwarded_from: Some(ErasedLocalMemberForwarding {
                                owner: source_owner,
                                local: source_local,
                                path: resource.path.clone(),
                            }),
                        };
                        match members.entry(path.clone()) {
                            std::collections::btree_map::Entry::Vacant(entry) => {
                                entry.insert(forwarded);
                            }
                            std::collections::btree_map::Entry::Occupied(entry)
                                if entry.get().target == forwarded.target
                                    || (matches!(
                                        entry.get().target,
                                        ErasedLocalMemberTarget::Source(_)
                                    ) && entry.get().forwarded_from.is_none()) => {}
                            std::collections::btree_map::Entry::Occupied(entry) => {
                                return Err(format!(
                                    "owner {} local {} forwarded resource `{}` conflicts with {:?}",
                                    local.owner,
                                    local.local.0,
                                    path.join("."),
                                    entry.get().target
                                ));
                            }
                        }
                    }
                }
            }
            local.members = members.into_values().collect();
        }

        if !changed {
            for local in locals.iter() {
                for member in &local.members {
                    if let ErasedLocalMemberTarget::Field(field) = member.target
                        && fields
                            .get(field.as_usize())
                            .filter(|candidate| candidate.id == field)
                            .is_some_and(|field| field.resource_only)
                    {
                        return Err(format!(
                            "owner {} local {} retains resource-only FieldId {} at `{}`",
                            local.owner,
                            local.local.0,
                            field,
                            member.path.join(".")
                        ));
                    }
                }
            }
            return Ok(());
        }
    }
    Err("forwarded materialization resources did not reach a fixed point".to_owned())
}

fn mark_forwarded_executable_resource_fields(
    executable: &mut ExecutableProgram,
    scope_index: &ErasedScopeIndex,
) -> Result<(), String> {
    let mut forwarded = Vec::new();
    for (expression_index, expression) in executable.expressions.iter().enumerate() {
        let fields = match &expression.kind {
            ExecutableExpressionKind::Object(fields)
            | ExecutableExpressionKind::Record(fields)
            | ExecutableExpressionKind::TaggedObject { fields, .. } => fields,
            _ => continue,
        };
        for (field_index, field) in fields.iter().enumerate() {
            if field.resource_only {
                continue;
            }
            if executable_expression_is_source_group(executable, field.value)? {
                forwarded.push((expression_index, field_index));
                continue;
            }
            let Some((owner, local, projection)) =
                materialization_local_projection(executable, field.value)?
            else {
                continue;
            };
            let source = scope_index
                .locals
                .iter()
                .find(|candidate| candidate.owner == owner && candidate.local == local)
                .ok_or_else(|| {
                    format!(
                        "record field `{}` forwards missing materialization local {}:{}",
                        field.name, owner, local.0
                    )
                })?;
            if resource_members_for_projection(source, &projection, &scope_index.fields).is_some() {
                forwarded.push((expression_index, field_index));
            }
        }
    }
    for (expression_index, field_index) in forwarded {
        let expression = executable
            .expressions
            .get_mut(expression_index)
            .ok_or_else(|| format!("missing executable expression index {expression_index}"))?;
        let fields = match &mut expression.kind {
            ExecutableExpressionKind::Object(fields)
            | ExecutableExpressionKind::Record(fields)
            | ExecutableExpressionKind::TaggedObject { fields, .. } => fields,
            _ => {
                return Err(format!(
                    "executable expression {} stopped being a record",
                    expression.id
                ));
            }
        };
        let field = fields.get_mut(field_index).ok_or_else(|| {
            format!(
                "executable expression {} is missing record field index {field_index}",
                expression.id
            )
        })?;
        field.resource_only = true;
    }
    Ok(())
}

fn bind_derived_field_ids(
    derived_values: &mut [DerivedValue],
    fields: &[ErasedFieldDef],
) -> Result<(), String> {
    for value in derived_values {
        let matches = fields
            .iter()
            .filter(|field| field.statement == Some(value.executable_statement_id))
            .filter(|field| field.diagnostic_path == value.path)
            .collect::<Vec<_>>();
        let field = match matches.as_slice() {
            [field] => *field,
            [] => {
                let by_statement = fields
                    .iter()
                    .filter(|field| field.statement == Some(value.executable_statement_id))
                    .collect::<Vec<_>>();
                let [field] = by_statement.as_slice() else {
                    return Err(format!(
                        "derived value `{}` statement {} has {} exact erased fields",
                        value.path,
                        value.executable_statement_id,
                        by_statement.len()
                    ));
                };
                *field
            }
            _ => {
                return Err(format!(
                    "derived value `{}` statement {} has {} exact erased fields",
                    value.path,
                    value.executable_statement_id,
                    matches.len()
                ));
            }
        };
        value.id = field.id;
        value.scope_id = field.row.map(|row| row.scope);
        value.indexed = field.row.is_some();
    }
    Ok(())
}

fn build_erased_scope_index(
    executable: &ExecutableProgram,
    static_owners: &[StaticOwnerDef],
    materializations: &[ContextualMaterialization],
    sources: &[SourcePort],
    states: &[StateCell],
    lists: &[ListMemory],
    list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    fields: Vec<ErasedFieldDef>,
) -> Result<ErasedScopeIndex, String> {
    let mut bindings = Vec::new();
    let direct_storage_statements = direct_erased_storage_statements(executable);
    for statement in &executable.statements {
        if !direct_storage_statements.contains(&statement.id) {
            continue;
        }
        let (Some(declaration), Some(producer)) = (statement.declaration, statement.value) else {
            continue;
        };
        let diagnostic_path = match &statement.kind {
            ExecutableStatementKind::Field { path, .. }
            | ExecutableStatementKind::List {
                path: Some(path), ..
            }
            | ExecutableStatementKind::Source {
                path: Some(path), ..
            } => path.clone(),
            _ => continue,
        };
        let expression = executable
            .expressions
            .get(producer.as_usize())
            .filter(|expression| expression.id == producer)
            .ok_or_else(|| {
                format!(
                    "storage declaration {} references missing producer {producer}",
                    declaration.0
                )
            })?;
        let flow_type = expression.flow_type.clone();
        let owns_direct_resource = executable.sources.iter().any(|source| {
            source.declaration == declaration
                && source.owner == expression.owner
                && source.expression == producer
        }) || executable.states.iter().any(|state| {
            state.declaration == declaration
                && state.owner == expression.owner
                && state.expression == producer
        });
        if owns_direct_resource {
            continue;
        }
        let list = if matches!(&flow_type.ty, boon_typecheck::Type::List(_)) {
            if let Some(storage) = list_storage.get(&statement.id) {
                Some(storage.list_id)
            } else if let Some(target) = direct_list_alias_target(executable, statement) {
                let candidates = executable
                    .statements
                    .iter()
                    .filter(|candidate| candidate.declaration == Some(target))
                    .filter_map(|candidate| list_storage.get(&candidate.id))
                    .map(|storage| storage.list_id)
                    .collect::<BTreeSet<_>>();
                let candidates = candidates.into_iter().collect::<Vec<_>>();
                let [list] = candidates.as_slice() else {
                    return Err(format!(
                        "list alias declaration {} (`{diagnostic_path}`) target {} has {} keyed storage bindings",
                        declaration.0,
                        target.0,
                        candidates.len()
                    ));
                };
                Some(*list)
            } else {
                None
            }
        } else {
            None
        };
        let field = if list.is_none() {
            let candidates = fields
                .iter()
                .filter(|field| field.statement == Some(statement.id))
                .filter(|field| field.declaration == Some(declaration))
                .filter(|field| field.static_owner == expression.owner)
                .collect::<Vec<_>>();
            let [field] = candidates.as_slice() else {
                let statement_fields = fields
                    .iter()
                    .filter(|field| field.statement == Some(statement.id))
                    .map(|field| {
                        (
                            field.id,
                            field.declaration,
                            field.static_owner,
                            field.diagnostic_path.as_str(),
                        )
                    })
                    .collect::<Vec<_>>();
                return Err(format!(
                    "value declaration {} (`{diagnostic_path}`) statement {} owner {:?} has {} exact fields; statement fields={statement_fields:?}",
                    declaration.0,
                    statement.id,
                    expression.owner,
                    candidates.len()
                ));
            };
            Some(field.id)
        } else {
            None
        };
        let row = list
            .map(|list_id| erased_row_binding_for_list(lists, list_id))
            .transpose()?;
        bindings.push(ErasedBinding {
            id: ErasedBindingId(bindings.len()),
            declaration,
            static_owner: expression.owner,
            owner_ancestry: static_owner_ancestry(expression.owner, static_owners)?,
            flow_type,
            producer,
            diagnostic_path,
            target: ErasedBindingTarget::Value { field, row },
        });
    }
    for source in &executable.sources {
        let runtime = sources
            .iter()
            .find(|candidate| candidate.executable_source_id == Some(source.id))
            .ok_or_else(|| format!("executable source {} has no allocated SourceId", source.id))?;
        let expression = executable
            .expressions
            .get(source.expression.as_usize())
            .filter(|expression| expression.id == source.expression)
            .ok_or_else(|| format!("executable source {} has no producer", source.id))?;
        bindings.push(ErasedBinding {
            id: ErasedBindingId(bindings.len()),
            declaration: source.declaration,
            static_owner: source.owner,
            owner_ancestry: static_owner_ancestry(source.owner, static_owners)?,
            flow_type: expression.flow_type.clone(),
            producer: source.expression,
            diagnostic_path: runtime.path.clone(),
            target: ErasedBindingTarget::Source {
                executable: source.id,
                runtime: runtime.id,
            },
        });
    }
    for state in &executable.states {
        let runtime = states
            .iter()
            .find(|candidate| candidate.executable_state_id == Some(state.id))
            .ok_or_else(|| format!("executable state {} has no allocated StateId", state.id))?;
        let expression = executable
            .expressions
            .get(state.expression.as_usize())
            .filter(|expression| expression.id == state.expression)
            .ok_or_else(|| format!("executable state {} has no producer", state.id))?;
        let (field, row) = if let Some(row_scope) = runtime.scope_id {
            let lists = lists
                .iter()
                .filter(|list| list.row_scope_id == Some(row_scope))
                .collect::<Vec<_>>();
            let [list] = lists.as_slice() else {
                return Err(format!(
                    "indexed executable state {} (`{}`) scope {} belongs to {} lists",
                    state.id,
                    runtime.semantic_path.as_deref().unwrap_or(&runtime.path),
                    row_scope,
                    lists.len()
                ));
            };
            let field = if runtime.published {
                let candidates = fields
                    .iter()
                    .filter(|field| {
                        field.declaration == Some(state.declaration)
                            && field.static_owner == state.owner
                            && field.row.map(|row| row.scope) == Some(row_scope)
                            && field.producer == Some(state.expression)
                    })
                    .collect::<Vec<_>>();
                let [field] = candidates.as_slice() else {
                    let declaration_fields = fields
                        .iter()
                        .filter(|field| field.declaration == Some(state.declaration))
                        .map(|field| {
                            (
                                field.id,
                                field.role,
                                field.static_owner,
                                field.row,
                                field.name.as_str(),
                                field.diagnostic_path.as_str(),
                                field.statement,
                                field.producer,
                            )
                        })
                        .collect::<Vec<_>>();
                    let runtime_states = states
                        .iter()
                        .filter(|candidate| candidate.executable_state_id == Some(state.id))
                        .map(|candidate| {
                            (
                                candidate.id,
                                candidate.scope_id,
                                candidate.published,
                                candidate.semantic_path.as_deref(),
                            )
                        })
                        .collect::<Vec<_>>();
                    return Err(format!(
                        "indexed executable state {} (`{}`) declaration {} owner {:?} scope {} has {} exact fields; declaration fields={declaration_fields:?}; runtime states={runtime_states:?}",
                        state.id,
                        runtime.semantic_path.as_deref().unwrap_or(&runtime.path),
                        state.declaration.0,
                        state.owner,
                        row_scope,
                        candidates.len()
                    ));
                };
                Some(field.id)
            } else {
                None
            };
            (
                field,
                Some(ErasedRowBinding {
                    list: list.id,
                    scope: row_scope,
                }),
            )
        } else {
            let candidates = fields
                .iter()
                .filter(|field| {
                    field.declaration == Some(state.declaration)
                        && field.static_owner == state.owner
                        && field.row.is_none()
                })
                .collect::<Vec<_>>();
            let field = if runtime.published {
                match candidates.as_slice() {
                    [] => None,
                    [field] => Some(field.id),
                    _ => {
                        return Err(format!(
                            "root executable state {} (`{}`) has {} exact fields",
                            state.id,
                            runtime.semantic_path.as_deref().unwrap_or(&runtime.path),
                            candidates.len()
                        ));
                    }
                }
            } else {
                None
            };
            (field, None)
        };
        bindings.push(ErasedBinding {
            id: ErasedBindingId(bindings.len()),
            declaration: state.declaration,
            static_owner: state.owner,
            owner_ancestry: static_owner_ancestry(state.owner, static_owners)?,
            flow_type: expression.flow_type.clone(),
            producer: state.expression,
            diagnostic_path: runtime.path.clone(),
            target: ErasedBindingTarget::State {
                executable: state.id,
                runtime: runtime.id,
                published: runtime.published,
                field,
                row,
            },
        });
    }
    let mut identities = BTreeSet::new();
    for binding in &bindings {
        let kind = match binding.target {
            ErasedBindingTarget::Value { .. } => 0_u8,
            ErasedBindingTarget::Source { .. } => 1,
            ErasedBindingTarget::State { .. } => 2,
        };
        if !identities.insert((
            binding.declaration,
            binding.static_owner,
            binding.producer,
            kind,
        )) {
            return Err(format!(
                "declaration {} owner {:?} producer {} has duplicate storage identity",
                binding.declaration.0, binding.static_owner, binding.producer
            ));
        }
    }
    let erased_sources = sources
        .iter()
        .map(|source| {
            let origin = match source.executable_source_id {
                Some(executable) => {
                    let candidates = bindings
                        .iter()
                        .filter_map(|binding| match binding.target {
                            ErasedBindingTarget::Source {
                                executable: binding_executable,
                                runtime,
                            } if binding_executable == executable && runtime == source.id => {
                                Some(binding.id)
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    let [binding] = candidates.as_slice() else {
                        return Err(format!(
                            "source `{}` has {} exact executable storage bindings",
                            source.path,
                            candidates.len()
                        ));
                    };
                    ErasedSourceOrigin::Executable {
                        executable,
                        binding: *binding,
                    }
                }
                None if source.path.starts_with("@distributed/") => {
                    if source.static_owner.is_some() || source.scope_id.is_some() || source.scoped {
                        return Err(format!(
                            "distributed ingress source `{}` must be structurally root owned",
                            source.path
                        ));
                    }
                    ErasedSourceOrigin::DistributedImport
                }
                None => {
                    return Err(format!(
                        "source `{}` has neither executable nor distributed ingress origin",
                        source.path
                    ));
                }
            };
            Ok(ErasedSourceDef {
                source: source.id,
                static_owner: source.static_owner,
                owner_ancestry: static_owner_ancestry(source.static_owner, static_owners)?,
                origin,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let owners = static_owners
        .iter()
        .map(|owner| {
            let materialization = materializations
                .iter()
                .find(|materialization| materialization.owner == owner.id);
            let source_row = materialization
                .map(|materialization| {
                    paired_row_binding(
                        materialization.source_list_id,
                        materialization.source_scope_id,
                        "source",
                        owner.id,
                    )
                })
                .transpose()?
                .flatten();
            let target_row = materialization
                .map(|materialization| {
                    paired_row_binding(
                        materialization.target_list_id,
                        materialization.target_scope_id,
                        "target",
                        owner.id,
                    )
                })
                .transpose()?
                .flatten();
            Ok(ErasedOwnerDef {
                id: owner.id,
                parent: owner.parent,
                child_ordinal: owner.child_ordinal,
                source_row,
                target_row,
                authority_row: target_row.or(source_row),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut fields = fields;
    let mut locals = materializations
        .iter()
        .map(|materialization| {
            let row = paired_row_binding(
                materialization.source_list_id,
                materialization.source_scope_id,
                "source",
                materialization.owner,
            )?;
            Ok(ErasedLocalDef {
                owner: materialization.owner,
                local: materialization.row_local,
                row,
                source: materialization.source,
                item_type: materialization.item_type.clone(),
                members: erased_local_members(
                    executable,
                    static_owners,
                    materializations,
                    &fields,
                    sources,
                    states,
                    lists,
                    materialization,
                    row,
                )?,
                captures: Vec::new(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    propagate_forwarded_local_resources(executable, materializations, &mut fields, &mut locals)?;
    Ok(ErasedScopeIndex {
        owners,
        locals,
        fields,
        bindings,
        sources: erased_sources,
        reads: Vec::new(),
        row_values: Vec::new(),
        dependencies: Vec::new(),
    })
}

type DetachedStateCaptureKey = (
    StaticOwnerId,
    MaterializationLocalId,
    StaticOwnerId,
    MaterializationLocalId,
    Vec<String>,
    ErasedRowBinding,
);

fn bind_detached_state_captures(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    sources: &[SourcePort],
    lists: &[ListMemory],
    state_update_arms: &[StateUpdateArm],
    scope_index: &mut ErasedScopeIndex,
) -> Result<(), String> {
    let mut requests = BTreeMap::<DetachedStateCaptureKey, boon_typecheck::Type>::new();
    for binding in &scope_index.bindings {
        let ErasedBindingTarget::State {
            executable: executable_state,
            runtime,
            row: Some(state_row),
            ..
        } = binding.target
        else {
            continue;
        };
        let state = executable
            .states
            .get(executable_state.as_usize())
            .filter(|state| state.id == executable_state)
            .ok_or_else(|| {
                format!(
                    "indexed state binding {} references missing executable state {}",
                    binding.id, executable_state
                )
            })?;
        let state_owner = binding.static_owner.ok_or_else(|| {
            format!(
                "indexed state {} (`{}`) has no static owner",
                executable_state, binding.diagnostic_path
            )
        })?;
        let target_materialization = nearest_state_row_materialization(
            state_owner,
            state_row,
            materializations,
            scope_index,
        )?;
        collect_detached_state_capture_requests(
            executable,
            materializations,
            scope_index,
            state.initial,
            state_row,
            target_materialization,
            None,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
            &mut requests,
        )?;
        for arm in state_update_arms.iter().filter(|arm| arm.state == runtime) {
            let event_list = event_cause_row_list(arm.cause, sources, lists)?;
            collect_detached_state_capture_requests(
                executable,
                materializations,
                scope_index,
                arm.output_expression_id,
                state_row,
                target_materialization,
                event_list,
                &mut BTreeSet::new(),
                &mut BTreeSet::new(),
                &mut requests,
            )?;
        }
    }

    for ((target_owner, target_local, source_owner, source_local, projection, target_row), ty) in
        requests
    {
        let target_materialization = materializations.iter().find(|materialization| {
            materialization.owner == target_owner && materialization.row_local == target_local
        });
        let reads_transferred_authority = target_materialization.is_some_and(|materialization| {
            contextual_materialization_transfers_list_authority(executable, materialization)
                && projection.first().is_some_and(|name| {
                    scope_index.fields.iter().any(|field| {
                        field.row == Some(target_row)
                            && field.name == *name
                            && field.role.is_authority()
                    })
                })
        });
        if reads_transferred_authority {
            continue;
        }
        let target = scope_index
            .locals
            .iter()
            .position(|local| local.owner == target_owner && local.local == target_local)
            .ok_or_else(|| {
                format!(
                    "detached capture target owner {} local {} is missing",
                    target_owner, target_local.0
                )
            })?;
        let ordinal = scope_index.locals[target].captures.len();
        let name = format!(
            "@capture/{}/{}/{}",
            target_owner.as_usize(),
            target_local.0,
            ordinal
        );
        if scope_index
            .fields
            .iter()
            .any(|field| field.row == Some(target_row) && field.name == name)
        {
            return Err(format!(
                "detached capture `{name}` collides with existing row storage"
            ));
        }
        let field = FieldId(scope_index.fields.len());
        scope_index.fields.push(ErasedFieldDef {
            id: field,
            role: ErasedFieldRole::Capture,
            declaration: None,
            static_owner: None,
            parent: None,
            row: Some(target_row),
            name: name.clone(),
            diagnostic_path: format!(
                "{name}/from/{}/{}{}",
                source_owner.as_usize(),
                source_local.0,
                projection
                    .iter()
                    .map(|field| format!("/{field}"))
                    .collect::<String>()
            ),
            statement: None,
            producer: None,
            resource_only: false,
            flow_type: boon_typecheck::FlowType {
                mode: boon_typecheck::FlowMode::Continuous,
                ty,
            },
        });
        scope_index.locals[target]
            .captures
            .push(ErasedLocalCapture {
                source_owner,
                source_local,
                projection,
                field,
            });
    }
    Ok(())
}

fn nearest_state_row_materialization(
    mut owner: StaticOwnerId,
    row: ErasedRowBinding,
    materializations: &[ContextualMaterialization],
    scope_index: &ErasedScopeIndex,
) -> Result<(StaticOwnerId, MaterializationLocalId), String> {
    loop {
        if let Some(materialization) = materializations.iter().find(|materialization| {
            materialization.owner == owner
                && materialization.target_list_id == Some(row.list)
                && materialization.target_scope_id == Some(row.scope)
        }) {
            if materialization.operation != ContextualOperationKind::Map {
                return Err(format!(
                    "indexed state row {}/{} is owned by non-map contextual operation {:?}",
                    row.list, row.scope, materialization.operation
                ));
            }
            return Ok((materialization.owner, materialization.row_local));
        }
        let definition = scope_index
            .owners
            .get(owner.as_usize())
            .filter(|definition| definition.id == owner)
            .ok_or_else(|| format!("indexed state references missing owner {owner}"))?;
        let Some(parent) = definition.parent else {
            return Err(format!(
                "indexed state row {}/{} has no owning contextual map",
                row.list, row.scope
            ));
        };
        owner = parent;
    }
}

fn event_cause_row_list(
    cause: EventCause,
    sources: &[SourcePort],
    lists: &[ListMemory],
) -> Result<Option<ListId>, String> {
    let EventCause::Source(source) = cause else {
        return Ok(None);
    };
    let source = sources
        .get(source.as_usize())
        .filter(|candidate| candidate.id == source)
        .ok_or_else(|| format!("state update references missing source {source}"))?;
    let Some(scope) = source.scope_id else {
        return Ok(None);
    };
    let candidates = lists
        .iter()
        .filter(|list| list.row_scope_id == Some(scope))
        .map(|list| list.id)
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [list] => Ok(Some(*list)),
        _ => Err(format!(
            "scoped source `{}` scope {} resolves to {} row lists",
            source.path,
            scope,
            candidates.len()
        )),
    }
}

fn contextual_list_reaches(
    materializations: &[ContextualMaterialization],
    source: ListId,
    target: ListId,
) -> bool {
    if source == target {
        return true;
    }
    let mut pending = vec![source];
    let mut visited = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !visited.insert(current) {
            continue;
        }
        for materialization in materializations.iter().filter(|materialization| {
            matches!(
                materialization.operation,
                ContextualOperationKind::Map
                    | ContextualOperationKind::Filter
                    | ContextualOperationKind::Retain
            ) && materialization.source_list_id == Some(current)
        }) {
            let Some(next) = materialization.target_list_id else {
                continue;
            };
            if next == target {
                return true;
            }
            pending.push(next);
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn collect_detached_state_capture_requests(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    scope_index: &ErasedScopeIndex,
    expression_id: ExecutableExprId,
    state_row: ErasedRowBinding,
    target: (StaticOwnerId, MaterializationLocalId),
    event_list: Option<ListId>,
    active_owners: &mut BTreeSet<StaticOwnerId>,
    visited: &mut BTreeSet<(ExecutableExprId, Vec<StaticOwnerId>)>,
    requests: &mut BTreeMap<DetachedStateCaptureKey, boon_typecheck::Type>,
) -> Result<(), String> {
    let active_key = active_owners.iter().copied().collect::<Vec<_>>();
    if !visited.insert((expression_id, active_key)) {
        return Ok(());
    }
    let expression = executable
        .expressions
        .get(expression_id.as_usize())
        .filter(|expression| expression.id == expression_id)
        .ok_or_else(|| format!("detached capture references missing expression {expression_id}"))?;
    match &expression.kind {
        ExecutableExpressionKind::MaterializationLocal {
            owner,
            local,
            projection,
        } if !active_owners.contains(owner) => {
            let source = scope_index
                .locals
                .iter()
                .find(|candidate| candidate.owner == *owner && candidate.local == *local)
                .ok_or_else(|| {
                    format!(
                        "detached capture expression {} references missing owner {} local {}",
                        expression_id, owner, local.0
                    )
                })?;
            if source.row == Some(state_row) {
                return Ok(());
            }
            if source.row.is_some_and(|row| {
                event_list.is_some_and(|event_list| {
                    contextual_list_reaches(materializations, event_list, row.list)
                })
            }) {
                return Ok(());
            }
            if !scope_index.owner_descends_from(target.0, *owner)? {
                return Err(format!(
                    "state row {}/{} cannot capture owner {} local {} from unrelated target owner {}",
                    state_row.list, state_row.scope, owner, local.0, target.0
                ));
            }
            let key = (
                target.0,
                target.1,
                *owner,
                *local,
                projection.clone(),
                state_row,
            );
            if let Some(previous) = requests.get(&key)
                && previous != &expression.flow_type.ty
            {
                return Err(format!(
                    "detached owner {} local {} projection `{}` has incompatible capture types {previous:?} and {:?}",
                    owner,
                    local.0,
                    projection.join("."),
                    expression.flow_type.ty
                ));
            }
            requests.insert(key, expression.flow_type.ty.clone());
        }
        ExecutableExpressionKind::Materialize { materialization } => {
            let materialization = materializations
                .get(*materialization)
                .filter(|candidate| candidate.id == *materialization)
                .ok_or_else(|| {
                    format!(
                        "detached capture expression {} references missing materialization {}",
                        expression_id, materialization
                    )
                })?;
            collect_detached_state_capture_requests(
                executable,
                materializations,
                scope_index,
                materialization.source,
                state_row,
                target,
                event_list,
                active_owners,
                visited,
                requests,
            )?;
            if let Some(direction) = materialization.direction {
                collect_detached_state_capture_requests(
                    executable,
                    materializations,
                    scope_index,
                    direction,
                    state_row,
                    target,
                    event_list,
                    active_owners,
                    visited,
                    requests,
                )?;
            }
            for key in &materialization.inherited_order {
                collect_detached_state_capture_requests(
                    executable,
                    materializations,
                    scope_index,
                    key.direction,
                    state_row,
                    target,
                    event_list,
                    active_owners,
                    visited,
                    requests,
                )?;
            }
            active_owners.insert(materialization.owner);
            let mut body_result = collect_detached_state_capture_requests(
                executable,
                materializations,
                scope_index,
                materialization.body,
                state_row,
                target,
                event_list,
                active_owners,
                visited,
                requests,
            );
            for key in &materialization.inherited_order {
                if body_result.is_ok() {
                    body_result = collect_detached_state_capture_requests(
                        executable,
                        materializations,
                        scope_index,
                        key.body,
                        state_row,
                        target,
                        event_list,
                        active_owners,
                        visited,
                        requests,
                    );
                }
            }
            active_owners.remove(&materialization.owner);
            body_result?;
        }
        _ => {
            for child in executable_expression_children(&expression.kind) {
                collect_detached_state_capture_requests(
                    executable,
                    materializations,
                    scope_index,
                    child,
                    state_row,
                    target,
                    event_list,
                    active_owners,
                    visited,
                    requests,
                )?;
            }
        }
    }
    Ok(())
}

fn paired_row_binding(
    list: Option<ListId>,
    scope: Option<ScopeId>,
    role: &str,
    owner: StaticOwnerId,
) -> Result<Option<ErasedRowBinding>, String> {
    match (list, scope) {
        (None, None) => Ok(None),
        (Some(list), Some(scope)) => Ok(Some(ErasedRowBinding { list, scope })),
        _ => Err(format!(
            "contextual owner {owner} has an incomplete {role} row binding: list={list:?}, scope={scope:?}"
        )),
    }
}

fn erased_row_binding_for_list(
    lists: &[ListMemory],
    list: ListId,
) -> Result<ErasedRowBinding, String> {
    let memory = lists
        .get(list.as_usize())
        .filter(|memory| memory.id == list)
        .ok_or_else(|| format!("missing ListId {list}"))?;
    let scope = memory
        .row_scope_id
        .ok_or_else(|| format!("ListId {list} has no exact row scope"))?;
    Ok(ErasedRowBinding { list, scope })
}

fn static_owner_ancestry(
    owner: Option<StaticOwnerId>,
    static_owners: &[StaticOwnerDef],
) -> Result<Vec<StaticOwnerId>, String> {
    let mut result = Vec::new();
    let mut current = owner;
    let mut visiting = BTreeSet::new();
    while let Some(owner) = current {
        if !visiting.insert(owner) {
            return Err(format!("static owner ancestry contains a cycle at {owner}"));
        }
        let definition = static_owners
            .get(owner.as_usize())
            .filter(|definition| definition.id == owner)
            .ok_or_else(|| format!("storage binding references missing static owner {owner}"))?;
        result.push(owner);
        current = definition.parent;
    }
    result.reverse();
    Ok(result)
}

fn erased_binding_for_read(
    scope_index: &ErasedScopeIndex,
    static_owners: &[StaticOwnerDef],
    declaration: boon_typecheck::DeclId,
    owner: Option<StaticOwnerId>,
) -> Result<ErasedBindingId, String> {
    let mut lexical_owners = static_owner_ancestry(owner, static_owners)?;
    lexical_owners.reverse();
    let lexical_owners = lexical_owners
        .into_iter()
        .map(Some)
        .chain(std::iter::once(None));
    for lexical_owner in lexical_owners {
        let candidates = scope_index
            .bindings
            .iter()
            .filter(|binding| {
                binding.declaration == declaration && binding.static_owner == lexical_owner
            })
            .filter(|binding| {
                !matches!(
                    binding.target,
                    ErasedBindingTarget::State {
                        published: false,
                        ..
                    }
                )
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            continue;
        }
        let preferred_kind = candidates
            .iter()
            .map(|binding| match binding.target {
                ErasedBindingTarget::Value { .. } => 0_u8,
                ErasedBindingTarget::State { .. } | ErasedBindingTarget::Source { .. } => 1,
            })
            .min()
            .expect("non-empty candidates");
        let preferred = candidates
            .into_iter()
            .filter(|binding| {
                (match binding.target {
                    ErasedBindingTarget::Value { .. } => 0_u8,
                    ErasedBindingTarget::State { .. } | ErasedBindingTarget::Source { .. } => 1,
                }) == preferred_kind
            })
            .collect::<Vec<_>>();
        let [binding] = preferred.as_slice() else {
            return Err(format!(
                "declaration {} owner {:?} resolves to multiple exact storage bindings {:?}",
                declaration.0, lexical_owner, preferred
            ));
        };
        return Ok(binding.id);
    }
    Err(format!(
        "declaration {} read from owner {:?} has no lexical storage binding",
        declaration.0, owner
    ))
}

fn executable_read_root_bindings(
    checked: &boon_typecheck::CheckedProgram,
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    scope_index: &ErasedScopeIndex,
    static_owners: &[StaticOwnerDef],
) -> Result<BTreeMap<ExecutableExprId, ErasedBindingId>, String> {
    let reachable = reachable_executable_expression_ids(executable, materializations)?;
    let expression_owners = executable
        .expressions
        .iter()
        .map(|expression| (expression.id, expression.owner))
        .collect::<BTreeMap<_, _>>();
    let mut bindings = BTreeMap::new();
    for expression in executable
        .expressions
        .iter()
        .filter(|expression| reachable.contains(&expression.id))
    {
        let target = match &expression.kind {
            ExecutableExpressionKind::CanonicalRead { target, .. }
            | ExecutableExpressionKind::Drain { target, .. } => Some(*target),
            _ => None,
        };
        let Some(target) = target else {
            continue;
        };
        let binding = erased_binding_for_read(scope_index, static_owners, target, expression.owner)
            .map_err(|error| {
                let producers = executable
                    .statements
                    .iter()
                    .filter(|statement| statement.declaration == Some(target))
                    .map(|statement| {
                        (
                            statement.id,
                            statement.value,
                            statement.kind.clone(),
                            statement.flow_type.clone(),
                            statement.value.and_then(|value| {
                                expression_owners.get(&value).copied().flatten()
                            }),
                        )
                    })
                    .collect::<Vec<_>>();
                let sources = executable
                    .sources
                    .iter()
                    .filter(|source| source.declaration == target)
                    .map(|source| (source.id, source.expression, source.owner))
                    .collect::<Vec<_>>();
                let states = executable
                    .states
                    .iter()
                    .filter(|state| state.declaration == target)
                    .map(|state| (state.id, state.expression, state.owner))
                    .collect::<Vec<_>>();
                let equivalent_reads = executable
                    .expressions
                    .iter()
                    .filter(|candidate| {
                        candidate.checked_expr_id == expression.checked_expr_id
                            && matches!(
                                candidate.kind,
                                ExecutableExpressionKind::CanonicalRead { .. }
                                    | ExecutableExpressionKind::Drain { .. }
                            )
                    })
                    .map(|candidate| {
                        (
                            candidate.id,
                            candidate.owner,
                            static_owner_ancestry(candidate.owner, static_owners),
                        )
                    })
                    .collect::<Vec<_>>();
                let checked_declaration = checked
                    .declarations
                    .iter()
                    .find(|declaration| declaration.id == target)
                    .map(|declaration| {
                        (
                            declaration.name.as_str(),
                            declaration.kind,
                            declaration.scope_id,
                            declaration.span,
                        )
                    });
                let checked_read = checked
                    .expressions
                    .iter()
                    .find(|candidate| candidate.id == expression.checked_expr_id)
                    .map(|candidate| (candidate.scope_id, candidate.span));
                format!(
                    "{error}; executable read {} from checked expression {} ({:?}, checked {:?}) targets checked declaration {:?}, and sees equivalent reads {:?}, declaration producers {:?}, sources {:?}, states {:?}",
                    expression.id,
                    expression.checked_expr_id.0,
                    expression.kind,
                    checked_read,
                    checked_declaration,
                    equivalent_reads,
                    producers,
                    sources,
                    states
                )
            })?;
        if bindings.insert(expression.id, binding).is_some() {
            return Err(format!(
                "executable read {} has duplicate root bindings",
                expression.id
            ));
        }
    }
    Ok(bindings)
}

fn erased_binding_read_target(
    executable: &ExecutableProgram,
    sources: &[SourcePort],
    scope_index: &ErasedScopeIndex,
    binding: ErasedBindingId,
    projection: &[String],
    read_flow_type: &boon_typecheck::FlowType,
) -> Result<ErasedReadTarget, String> {
    let mut binding = binding;
    let mut consumed = 0;
    while let Some((projected, projected_count)) =
        exact_projected_binding(scope_index, binding, &projection[consumed..])?
    {
        binding = projected;
        consumed += projected_count;
    }
    let remaining = &projection[consumed..];
    let storage = scope_index
        .bindings
        .get(binding.as_usize())
        .filter(|candidate| candidate.id == binding)
        .ok_or_else(|| format!("missing erased binding {binding}"))?;
    match storage.target {
        ErasedBindingTarget::Source { .. } if storage.flow_type == *read_flow_type => {
            Ok(ErasedReadTarget::Binding {
                binding,
                projection: Vec::new(),
            })
        }
        ErasedBindingTarget::Source { runtime, .. } if !remaining.is_empty() => {
            erased_source_payload_read(sources, binding, runtime, remaining).map_err(|error| {
                format!(
                    "{error}; read binding {binding} (`{}`) consumed {consumed} of projection `{}`",
                    storage.diagnostic_path,
                    projection.join(".")
                )
            })
        }
        ErasedBindingTarget::State { .. } if storage.flow_type == *read_flow_type => {
            Ok(ErasedReadTarget::Binding {
                binding,
                projection: Vec::new(),
            })
        }
        ErasedBindingTarget::State { runtime, .. } if !remaining.is_empty() => {
            Ok(ErasedReadTarget::StateProjection {
                binding,
                state: runtime,
                fields: remaining.to_vec(),
            })
        }
        ErasedBindingTarget::Value { field, row: None }
            if field.is_none()
                || matches!(
                    executable
                        .expressions
                        .get(storage.producer.as_usize())
                        .map(|expression| &expression.kind),
                    Some(
                        ExecutableExpressionKind::Object(_)
                            | ExecutableExpressionKind::Record(_)
                            | ExecutableExpressionKind::TaggedObject { .. }
                    )
                ) =>
        {
            let (expression, remaining) =
                exact_executable_record_projection(executable, storage.producer, remaining)?;
            let resources = scope_index
                .bindings
                .iter()
                .filter(|candidate| candidate.producer == expression)
                .filter(|candidate| {
                    matches!(
                        candidate.target,
                        ErasedBindingTarget::Source { .. } | ErasedBindingTarget::State { .. }
                    )
                })
                .collect::<Vec<_>>();
            match resources.as_slice() {
                [] => Ok(ErasedReadTarget::Expression {
                    expression,
                    projection: remaining.to_vec(),
                }),
                [resource] => match resource.target {
                    ErasedBindingTarget::Source { .. }
                        if resource.flow_type == *read_flow_type =>
                    {
                        Ok(ErasedReadTarget::Binding {
                            binding: resource.id,
                            projection: Vec::new(),
                        })
                    }
                    ErasedBindingTarget::Source { runtime, .. } if !remaining.is_empty() => {
                        erased_source_payload_read(
                            sources,
                            resource.id,
                            runtime,
                            remaining,
                        )
                        .map_err(|error| {
                            format!(
                                "{error}; expression {expression} projected from binding {binding} (`{}`)",
                                storage.diagnostic_path
                            )
                        })
                    }
                    ErasedBindingTarget::State { runtime, .. } if !remaining.is_empty() => {
                        Ok(ErasedReadTarget::StateProjection {
                            binding: resource.id,
                            state: runtime,
                            fields: remaining.to_vec(),
                        })
                    }
                    ErasedBindingTarget::Source { .. }
                    | ErasedBindingTarget::State { .. } => Ok(ErasedReadTarget::Binding {
                        binding: resource.id,
                        projection: Vec::new(),
                    }),
                    ErasedBindingTarget::Value { .. } => unreachable!("filtered resource target"),
                },
                _ => Err(format!(
                    "executable expression {expression} has {} exact state/source bindings",
                    resources.len()
                )),
            }
        }
        _ => Ok(ErasedReadTarget::Binding {
            binding,
            projection: remaining.to_vec(),
        }),
    }
}

fn exact_projected_binding(
    scope_index: &ErasedScopeIndex,
    binding: ErasedBindingId,
    projection: &[String],
) -> Result<Option<(ErasedBindingId, usize)>, String> {
    let root = scope_index
        .bindings
        .get(binding.as_usize())
        .filter(|candidate| candidate.id == binding)
        .ok_or_else(|| format!("missing erased binding {binding}"))?;
    let ErasedBindingTarget::Value {
        field: Some(mut field),
        ..
    } = root.target
    else {
        return Ok(None);
    };
    let mut deepest = None;
    for (index, name) in projection.iter().enumerate() {
        let fields = scope_index
            .fields
            .iter()
            .filter(|candidate| candidate.parent == Some(field) && candidate.name == *name)
            .collect::<Vec<_>>();
        let child = match fields.as_slice() {
            [] => return Ok(None),
            [child] => *child,
            _ => {
                return Err(format!(
                    "erased field {field} projection `{name}` resolves to {} child fields",
                    fields.len()
                ));
            }
        };
        field = child.id;
        let exact = scope_index
            .bindings
            .iter()
            .filter(|candidate| {
                child.producer == Some(candidate.producer)
                    && candidate.static_owner == child.static_owner
            })
            .collect::<Vec<_>>();
        let bindings = if exact.is_empty() {
            let Some(declaration) = child.declaration else {
                continue;
            };
            scope_index
                .bindings
                .iter()
                .filter(|candidate| {
                    candidate.declaration == declaration
                        && candidate.static_owner == child.static_owner
                })
                .collect::<Vec<_>>()
        } else {
            exact
        };
        match bindings.as_slice() {
            [] => {}
            [projected] if projected.id != binding => {
                deepest = Some((projected.id, index + 1));
            }
            [..] if bindings.iter().all(|projected| projected.id == binding) => {}
            _ => {}
        }
    }
    Ok(deepest)
}

fn erased_source_payload_read(
    sources: &[SourcePort],
    binding: ErasedBindingId,
    source: SourceId,
    projection: &[String],
) -> Result<ErasedReadTarget, String> {
    let source_port = sources
        .get(source.as_usize())
        .filter(|candidate| candidate.id == source)
        .ok_or_else(|| format!("missing SourceId {source}"))?;
    let (field_name, projection) = projection.split_first().ok_or_else(|| {
        format!(
            "source `{}` has an empty payload projection",
            source_port.path
        )
    })?;
    let fields = source_port
        .payload_schema
        .fields
        .iter()
        .filter(|field| field.name() == field_name)
        .collect::<Vec<_>>();
    let [field] = fields.as_slice() else {
        return Err(format!(
            "source `{}` payload projection `{field_name}` resolves to {} fields",
            source_port.path,
            fields.len()
        ));
    };
    Ok(ErasedReadTarget::SourcePayload {
        binding,
        source,
        field: (*field).clone(),
        projection: projection.to_vec(),
    })
}

fn erased_exact_source_read_target(
    executable: &ExecutableProgram,
    static_owners: &[StaticOwnerDef],
    sources: &[SourcePort],
    scope_index: &ErasedScopeIndex,
    owner: Option<StaticOwnerId>,
    source_read: &boon_typecheck::CheckedSourceRead,
) -> Result<ErasedReadTarget, String> {
    let mut lexical_owners = static_owner_ancestry(owner, static_owners)?;
    lexical_owners.reverse();
    for lexical_owner in lexical_owners
        .into_iter()
        .map(Some)
        .chain(std::iter::once(None))
    {
        let candidates = scope_index
            .bindings
            .iter()
            .filter(|binding| binding.static_owner == lexical_owner)
            .filter_map(|binding| {
                let ErasedBindingTarget::Source {
                    executable: source,
                    runtime,
                } = binding.target
                else {
                    return None;
                };
                let definition = executable
                    .sources
                    .get(source.as_usize())
                    .filter(|definition| definition.id == source)?;
                let producer = executable
                    .expressions
                    .get(definition.expression.as_usize())
                    .filter(|producer| producer.id == definition.expression)?;
                (producer.checked_expr_id == source_read.expression)
                    .then_some((binding.id, runtime))
            })
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [] => continue,
            [(binding, _)] if source_read.payload_projection.is_empty() => {
                return Ok(ErasedReadTarget::Binding {
                    binding: *binding,
                    projection: Vec::new(),
                });
            }
            [(binding, runtime)] => {
                return erased_source_payload_read(
                    sources,
                    *binding,
                    *runtime,
                    &source_read.payload_projection,
                );
            }
            _ => {
                return Err(format!(
                    "checked SOURCE expression {} owner {:?} resolves to {} exact source bindings",
                    source_read.expression.0,
                    lexical_owner,
                    candidates.len()
                ));
            }
        }
    }
    Err(format!(
        "checked SOURCE expression {} read from owner {:?} has no lexical source binding",
        source_read.expression.0, owner
    ))
}

fn exact_executable_record_projection<'a>(
    executable: &ExecutableProgram,
    mut expression: ExecutableExprId,
    projection: &'a [String],
) -> Result<(ExecutableExprId, &'a [String]), String> {
    let mut remaining = projection;
    while let Some((field_name, rest)) = remaining.split_first() {
        let value = executable
            .expressions
            .get(expression.as_usize())
            .filter(|candidate| candidate.id == expression)
            .ok_or_else(|| format!("read projection reaches missing expression {expression}"))?;
        let fields = match &value.kind {
            ExecutableExpressionKind::Object(fields)
            | ExecutableExpressionKind::Record(fields)
            | ExecutableExpressionKind::TaggedObject { fields, .. } => fields,
            _ => break,
        };
        let matches = fields
            .iter()
            .filter(|field| !field.spread && field.name == *field_name)
            .collect::<Vec<_>>();
        let field = match matches.as_slice() {
            [] => break,
            [field] => *field,
            _ => {
                return Err(format!(
                    "expression {expression} projection `{field_name}` resolves to {} fields",
                    matches.len()
                ));
            }
        };
        expression = field.value;
        remaining = rest;
    }
    Ok((expression, remaining))
}

fn build_erased_row_values(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    scope_index: &ErasedScopeIndex,
) -> Result<Vec<ErasedRowValue>, String> {
    let expression_ids = reachable_executable_expression_ids(executable, materializations)?;
    let mut resolver = ErasedRowValueResolver {
        executable,
        materializations,
        scope_index,
        row_memo: BTreeMap::new(),
        list_memo: BTreeMap::new(),
        visiting_rows: BTreeSet::new(),
        visiting_lists: BTreeSet::new(),
    };
    let mut values = Vec::new();
    for expression in expression_ids {
        values.extend(
            resolver
                .row_values(expression)?
                .into_iter()
                .map(|(projection, row)| ErasedRowValue {
                    expression,
                    projection,
                    row,
                }),
        );
    }
    Ok(values)
}

type ErasedRowValueMap = BTreeMap<Vec<String>, ErasedRowBinding>;

struct ErasedRowValueResolver<'a> {
    executable: &'a ExecutableProgram,
    materializations: &'a [ContextualMaterialization],
    scope_index: &'a ErasedScopeIndex,
    row_memo: BTreeMap<ExecutableExprId, ErasedRowValueMap>,
    list_memo: BTreeMap<ExecutableExprId, Option<ErasedRowBinding>>,
    visiting_rows: BTreeSet<ExecutableExprId>,
    visiting_lists: BTreeSet<ExecutableExprId>,
}

impl ErasedRowValueResolver<'_> {
    fn expression(&self, id: ExecutableExprId) -> Result<&ExecutableExpression, String> {
        self.executable
            .expressions
            .get(id.as_usize())
            .filter(|expression| expression.id == id)
            .ok_or_else(|| format!("row provenance references missing expression {id}"))
    }

    fn row_values(&mut self, id: ExecutableExprId) -> Result<ErasedRowValueMap, String> {
        if let Some(values) = self.row_memo.get(&id) {
            return Ok(values.clone());
        }
        if !self.visiting_rows.insert(id) {
            return Ok(BTreeMap::new());
        }
        let kind = self.expression(id)?.kind.clone();
        let values = self.row_values_for_kind(id, kind);
        self.visiting_rows.remove(&id);
        let values = values?;
        self.row_memo.insert(id, values.clone());
        Ok(values)
    }

    fn row_values_for_kind(
        &mut self,
        expression: ExecutableExprId,
        kind: ExecutableExpressionKind,
    ) -> Result<ErasedRowValueMap, String> {
        match kind {
            ExecutableExpressionKind::CanonicalRead { .. }
            | ExecutableExpressionKind::LocalRead { .. }
            | ExecutableExpressionKind::ExternalRead { .. }
            | ExecutableExpressionKind::Drain { .. }
            | ExecutableExpressionKind::MaterializationLocal { .. }
            | ExecutableExpressionKind::FunctionParameter { .. } => {
                self.row_values_for_read(expression)
            }
            ExecutableExpressionKind::Materialize { materialization } => {
                let materialization = self
                    .materializations
                    .get(materialization)
                    .filter(|candidate| candidate.id == materialization)
                    .ok_or_else(|| {
                        format!(
                            "row provenance expression {expression} references missing materialization {materialization}"
                        )
                    })?;
                if materialization.operation != ContextualOperationKind::Find {
                    return Ok(BTreeMap::new());
                }
                let Some(row) = paired_row_binding(
                    materialization.source_list_id,
                    materialization.source_scope_id,
                    "source",
                    materialization.owner,
                )?
                else {
                    return Ok(BTreeMap::new());
                };
                Ok(BTreeMap::from([(vec!["value".to_owned()], row)]))
            }
            ExecutableExpressionKind::Call {
                name, arguments, ..
            } if matches!(name.as_str(), "List/get" | "List/latest") => {
                let source = arguments
                    .iter()
                    .find(|argument| argument.name == "list")
                    .or_else(|| arguments.iter().find(|argument| argument.ordinal == 0))
                    .ok_or_else(|| {
                        format!("{name} expression {expression} has no typed list argument")
                    })?;
                Ok(self
                    .list_row(source.value)?
                    .map(|row| BTreeMap::from([(Vec::new(), row)]))
                    .unwrap_or_default())
            }
            ExecutableExpressionKind::Object(fields)
            | ExecutableExpressionKind::Record(fields)
            | ExecutableExpressionKind::TaggedObject { fields, .. } => {
                let mut values = BTreeMap::new();
                for field in fields.into_iter().filter(|field| !field.spread) {
                    let nested = self
                        .row_values(field.value)?
                        .into_iter()
                        .map(|(mut path, row)| {
                            path.insert(0, field.name.clone());
                            (path, row)
                        })
                        .collect();
                    merge_erased_row_values(
                        &mut values,
                        nested,
                        &format!("expression {expression} field `{}`", field.name),
                    )?;
                }
                Ok(values)
            }
            ExecutableExpressionKind::Block { result, .. } => self.row_values(result),
            ExecutableExpressionKind::Project { input, fields } => {
                Ok(project_erased_row_values(self.row_values(input)?, &fields))
            }
            ExecutableExpressionKind::When { arms, .. } => {
                self.merge_row_value_branches(expression, arms.into_iter().map(|arm| arm.output))
            }
            ExecutableExpressionKind::Latest { branches } => {
                self.merge_row_value_branches(expression, branches.into_iter())
            }
            ExecutableExpressionKind::Hold {
                initial, updates, ..
            } => self.merge_row_value_branches(expression, std::iter::once(initial).chain(updates)),
            ExecutableExpressionKind::Then {
                output: Some(output),
                ..
            }
            | ExecutableExpressionKind::MatchArm {
                output: Some(output),
                ..
            } => self.row_values(output),
            ExecutableExpressionKind::Draining { input } => self.row_values(input),
            _ => Ok(BTreeMap::new()),
        }
    }

    fn merge_row_value_branches(
        &mut self,
        expression: ExecutableExprId,
        branches: impl Iterator<Item = ExecutableExprId>,
    ) -> Result<ErasedRowValueMap, String> {
        let mut values = BTreeMap::new();
        for branch in branches {
            let branch_values = self.row_values(branch)?;
            merge_erased_row_values(
                &mut values,
                branch_values,
                &format!("expression {expression} branch {branch}"),
            )?;
        }
        Ok(values)
    }

    fn row_values_for_read(
        &mut self,
        expression: ExecutableExprId,
    ) -> Result<ErasedRowValueMap, String> {
        let Some(read) = self
            .scope_index
            .reads
            .iter()
            .find(|read| read.expression == expression)
        else {
            return Ok(BTreeMap::new());
        };
        match &read.target {
            ErasedReadTarget::Binding {
                binding,
                projection,
            } => {
                let binding = self.binding(*binding)?;
                Ok(project_erased_row_values(
                    self.row_values(binding.producer)?,
                    projection,
                ))
            }
            ErasedReadTarget::Expression {
                expression,
                projection,
            } => Ok(project_erased_row_values(
                self.row_values(*expression)?,
                projection,
            )),
            ErasedReadTarget::Local {
                value, projection, ..
            } => Ok(project_erased_row_values(
                self.row_values(*value)?,
                projection,
            )),
            ErasedReadTarget::MaterializationLocal {
                owner,
                local,
                projection,
            } if projection.is_empty() => {
                let rows = self
                    .scope_index
                    .locals
                    .iter()
                    .filter(|candidate| candidate.owner == *owner && candidate.local == *local)
                    .filter_map(|candidate| candidate.row)
                    .collect::<BTreeSet<_>>();
                let rows = rows.into_iter().collect::<Vec<_>>();
                match rows.as_slice() {
                    [row] => Ok(BTreeMap::from([(Vec::new(), *row)])),
                    [] => Ok(BTreeMap::new()),
                    rows => Err(format!(
                        "materialization local {owner}:{} has multiple exact row owners {rows:?}",
                        local.0
                    )),
                }
            }
            ErasedReadTarget::SourcePayload { .. }
            | ErasedReadTarget::StateProjection { .. }
            | ErasedReadTarget::ExternalValue { .. }
            | ErasedReadTarget::MaterializationLocal { .. }
            | ErasedReadTarget::FunctionParameter { .. } => Ok(BTreeMap::new()),
        }
    }

    fn list_row(
        &mut self,
        expression: ExecutableExprId,
    ) -> Result<Option<ErasedRowBinding>, String> {
        if let Some(row) = self.list_memo.get(&expression) {
            return Ok(*row);
        }
        if !self.visiting_lists.insert(expression) {
            return Ok(None);
        }
        let kind = self.expression(expression)?.kind.clone();
        let mut rows =
            self.scope_index
                .bindings
                .iter()
                .filter(|binding| binding.producer == expression)
                .filter_map(|binding| match binding.target {
                    ErasedBindingTarget::Value { row, .. }
                    | ErasedBindingTarget::State { row, .. } => row,
                    ErasedBindingTarget::Source { .. } => None,
                })
                .collect::<BTreeSet<_>>();
        match kind {
            ExecutableExpressionKind::CanonicalRead { .. }
            | ExecutableExpressionKind::LocalRead { .. }
            | ExecutableExpressionKind::Drain { .. } => {
                let Some(read) = self
                    .scope_index
                    .reads
                    .iter()
                    .find(|read| read.expression == expression)
                else {
                    self.visiting_lists.remove(&expression);
                    self.list_memo.insert(expression, None);
                    return Ok(None);
                };
                match &read.target {
                    ErasedReadTarget::Binding {
                        binding,
                        projection,
                    } if projection.is_empty() => match self.binding(*binding)?.target {
                        ErasedBindingTarget::Value { row: Some(row), .. }
                        | ErasedBindingTarget::State { row: Some(row), .. } => {
                            rows.insert(row);
                        }
                        _ => {}
                    },
                    ErasedReadTarget::Expression {
                        expression: target,
                        projection,
                    }
                    | ErasedReadTarget::Local {
                        value: target,
                        projection,
                        ..
                    } if projection.is_empty() => {
                        if let Some(row) = self.list_row(*target)? {
                            rows.insert(row);
                        }
                    }
                    _ => {}
                }
            }
            ExecutableExpressionKind::Materialize { materialization } => {
                let materialization = self
                    .materializations
                    .get(materialization)
                    .filter(|candidate| candidate.id == materialization)
                    .ok_or_else(|| {
                        format!(
                            "list provenance expression {expression} references missing materialization {materialization}"
                        )
                    })?;
                if let Some(row) = paired_row_binding(
                    materialization.target_list_id,
                    materialization.target_scope_id,
                    "target",
                    materialization.owner,
                )? {
                    rows.insert(row);
                } else if matches!(
                    materialization.operation,
                    ContextualOperationKind::Filter
                        | ContextualOperationKind::Retain
                        | ContextualOperationKind::Remove
                ) && let Some(row) = paired_row_binding(
                    materialization.source_list_id,
                    materialization.source_scope_id,
                    "source",
                    materialization.owner,
                )? {
                    rows.insert(row);
                }
            }
            ExecutableExpressionKind::Block { result, .. } => {
                if let Some(row) = self.list_row(result)? {
                    rows.insert(row);
                }
            }
            ExecutableExpressionKind::Project { input, fields } if fields.is_empty() => {
                if let Some(row) = self.list_row(input)? {
                    rows.insert(row);
                }
            }
            ExecutableExpressionKind::When { arms, .. } => {
                for arm in arms {
                    if let Some(row) = self.list_row(arm.output)? {
                        rows.insert(row);
                    }
                }
            }
            ExecutableExpressionKind::Latest { branches } => {
                for branch in branches {
                    if let Some(row) = self.list_row(branch)? {
                        rows.insert(row);
                    }
                }
            }
            ExecutableExpressionKind::Then {
                output: Some(output),
                ..
            }
            | ExecutableExpressionKind::MatchArm {
                output: Some(output),
                ..
            } => {
                if let Some(row) = self.list_row(output)? {
                    rows.insert(row);
                }
            }
            ExecutableExpressionKind::Draining { input } => {
                if let Some(row) = self.list_row(input)? {
                    rows.insert(row);
                }
            }
            ExecutableExpressionKind::Call {
                name, arguments, ..
            } if name == "List/append" => {
                if let Some(source) = arguments
                    .iter()
                    .find(|argument| argument.name == "list")
                    .or_else(|| arguments.iter().find(|argument| argument.ordinal == 0))
                    && let Some(row) = self.list_row(source.value)?
                {
                    rows.insert(row);
                }
            }
            _ => {}
        }
        self.visiting_lists.remove(&expression);
        let rows = rows.into_iter().collect::<Vec<_>>();
        let row = match rows.as_slice() {
            [] => None,
            [row] => Some(*row),
            rows => {
                return Err(format!(
                    "list expression {expression} has multiple exact row owners {rows:?}"
                ));
            }
        };
        self.list_memo.insert(expression, row);
        Ok(row)
    }

    fn binding(&self, id: ErasedBindingId) -> Result<&ErasedBinding, String> {
        self.scope_index
            .bindings
            .get(id.as_usize())
            .filter(|binding| binding.id == id)
            .ok_or_else(|| format!("row provenance references missing binding {id}"))
    }
}

fn project_erased_row_values(
    values: ErasedRowValueMap,
    projection: &[String],
) -> ErasedRowValueMap {
    if projection.is_empty() {
        return values;
    }
    values
        .into_iter()
        .filter_map(|(path, row)| {
            path.strip_prefix(projection)
                .map(|remaining| (remaining.to_vec(), row))
        })
        .collect()
}

fn merge_erased_row_values(
    target: &mut ErasedRowValueMap,
    source: ErasedRowValueMap,
    context: &str,
) -> Result<(), String> {
    for (projection, row) in source {
        if let Some(existing) = target.insert(projection.clone(), row)
            && existing != row
        {
            return Err(format!(
                "{context} row projection `{}` has conflicting owners {existing:?} and {row:?}",
                projection.join(".")
            ));
        }
    }
    Ok(())
}

fn build_erased_read_bindings(
    executable: &ExecutableProgram,
    static_owners: &[StaticOwnerDef],
    materializations: &[ContextualMaterialization],
    sources: &[SourcePort],
    scope_index: &ErasedScopeIndex,
    distributed: &DistributedReferences,
    read_roots: &BTreeMap<ExecutableExprId, ErasedBindingId>,
) -> Result<Vec<ErasedReadBinding>, String> {
    let roots = executable_expression_roots(executable);
    let reachable = reachable_executable_expression_ids(executable, materializations)?;
    let mut targets = BTreeMap::<ExecutableExprId, ErasedReadTarget>::new();
    for expression in executable
        .expressions
        .iter()
        .filter(|expression| reachable.contains(&expression.id))
    {
        let target = match &expression.kind {
            ExecutableExpressionKind::CanonicalRead {
                projection, source, ..
            } => {
                if let Some(source) = source {
                    Some(erased_exact_source_read_target(
                        executable,
                        static_owners,
                        sources,
                        scope_index,
                        expression.owner,
                        source,
                    )?)
                } else {
                    let binding = read_roots.get(&expression.id).copied().ok_or_else(|| {
                        format!(
                            "executable read {} has no exact root binding",
                            expression.id
                        )
                    })?;
                    Some(erased_binding_read_target(
                        executable,
                        sources,
                        scope_index,
                        binding,
                        projection,
                        &expression.flow_type,
                    )?)
                }
            }
            ExecutableExpressionKind::Drain { projection, .. } => {
                let binding = read_roots.get(&expression.id).copied().ok_or_else(|| {
                    format!(
                        "executable read {} has no exact root binding",
                        expression.id
                    )
                })?;
                Some(erased_binding_read_target(
                    executable,
                    sources,
                    scope_index,
                    binding,
                    projection,
                    &expression.flow_type,
                )?)
            }
            ExecutableExpressionKind::ExternalRead { canonical_path } => {
                let matches = distributed
                    .value_references
                    .iter()
                    .enumerate()
                    .filter(|(_, reference)| reference.canonical_path == *canonical_path)
                    .filter(|(_, reference)| {
                        reference.expr_id.as_usize() == expression.checked_expr_id.0 as usize
                    })
                    .map(|(index, _)| index)
                    .collect::<Vec<_>>();
                let [reference] = matches.as_slice() else {
                    return Err(format!(
                        "external read {} (`{canonical_path}`) checked expression {} has {} exact distributed references",
                        expression.id,
                        expression.checked_expr_id.0,
                        matches.len()
                    ));
                };
                Some(ErasedReadTarget::ExternalValue {
                    reference: *reference,
                })
            }
            ExecutableExpressionKind::MaterializationLocal {
                owner,
                local,
                projection,
            } => {
                if !scope_index
                    .locals
                    .iter()
                    .any(|candidate| candidate.owner == *owner && candidate.local == *local)
                {
                    return Err(format!(
                        "materialization local read {} references missing owner {} local {}",
                        expression.id, owner, local.0
                    ));
                }
                Some(ErasedReadTarget::MaterializationLocal {
                    owner: *owner,
                    local: *local,
                    projection: projection.clone(),
                })
            }
            ExecutableExpressionKind::FunctionParameter {
                parameter,
                projection,
            } => Some(ErasedReadTarget::FunctionParameter {
                parameter: *parameter,
                projection: projection.clone(),
            }),
            _ => None,
        };
        if let Some(target) = target {
            targets.insert(expression.id, target);
        }
    }

    for root in roots {
        collect_local_read_targets(
            executable,
            root,
            &BTreeMap::new(),
            &mut BTreeSet::new(),
            &mut targets,
        )?;
    }
    for materialization in materializations.iter().filter(|materialization| {
        reachable.contains(&materialization.source)
            || reachable.contains(&materialization.body)
            || materialization
                .direction
                .is_some_and(|direction| reachable.contains(&direction))
            || materialization
                .inherited_order
                .iter()
                .any(|key| reachable.contains(&key.body) || reachable.contains(&key.direction))
    }) {
        for root in materialization.expression_roots() {
            collect_local_read_targets(
                executable,
                root,
                &BTreeMap::new(),
                &mut BTreeSet::new(),
                &mut targets,
            )?;
        }
    }
    for expression in executable
        .expressions
        .iter()
        .filter(|expression| reachable.contains(&expression.id))
    {
        if matches!(expression.kind, ExecutableExpressionKind::LocalRead { .. })
            && !targets.contains_key(&expression.id)
        {
            return Err(format!(
                "local read {} is not reachable from an exact erased BLOCK",
                expression.id
            ));
        }
    }
    Ok(targets
        .into_iter()
        .enumerate()
        .map(|(index, (expression, target))| ErasedReadBinding {
            id: ErasedReadId(index),
            expression,
            target,
        })
        .collect())
}

fn executable_expression_roots(executable: &ExecutableProgram) -> Vec<ExecutableExprId> {
    let child_statements = executable
        .statements
        .iter()
        .flat_map(|statement| statement.children.iter().copied())
        .collect::<BTreeSet<_>>();
    let mut roots = executable
        .statements
        .iter()
        .filter(|statement| !child_statements.contains(&statement.id))
        .filter_map(|statement| statement.value)
        .chain(executable.roots.iter().map(|root| root.expression))
        .chain(executable.functions.iter().map(|function| function.root))
        .chain(executable.sources.iter().map(|source| source.expression))
        .chain(executable.states.iter().map(|state| state.expression))
        .collect::<Vec<_>>();
    roots.sort_unstable();
    roots.dedup();
    roots
}

fn runtime_executable_expression_roots(executable: &ExecutableProgram) -> Vec<ExecutableExprId> {
    let child_statements = executable
        .statements
        .iter()
        .flat_map(|statement| statement.children.iter().copied())
        .collect::<BTreeSet<_>>();
    let mut roots = executable
        .statements
        .iter()
        .filter(|statement| !child_statements.contains(&statement.id))
        .filter_map(|statement| statement.value)
        .chain(executable.roots.iter().map(|root| root.expression))
        .chain(executable.sources.iter().map(|source| source.expression))
        .chain(executable.states.iter().map(|state| state.expression))
        .collect::<Vec<_>>();
    roots.sort_unstable();
    roots.dedup();
    roots
}

pub(crate) fn reachable_executable_expression_ids(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
) -> Result<BTreeSet<ExecutableExprId>, String> {
    reachable_executable_expression_ids_from_roots(
        executable,
        materializations,
        executable_expression_roots(executable),
    )
}

fn reachable_runtime_expression_ids(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
) -> Result<BTreeSet<ExecutableExprId>, String> {
    reachable_executable_expression_ids_from_roots(
        executable,
        materializations,
        runtime_executable_expression_roots(executable),
    )
}

fn reachable_executable_expression_ids_from_roots(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    mut pending: Vec<ExecutableExprId>,
) -> Result<BTreeSet<ExecutableExprId>, String> {
    let mut reachable = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !reachable.insert(expression_id) {
            continue;
        }
        let expression = executable
            .expressions
            .get(expression_id.as_usize())
            .filter(|candidate| candidate.id == expression_id)
            .ok_or_else(|| {
                format!("executable reachability reaches missing expression {expression_id}")
            })?;
        if let ExecutableExpressionKind::Materialize { materialization } = expression.kind {
            let materialization = materializations
                .get(materialization)
                .filter(|candidate| candidate.id == materialization)
                .ok_or_else(|| {
                    format!(
                        "executable reachability reaches missing materialization {materialization}"
                    )
                })?;
            pending.extend(materialization.expression_roots());
        }
        pending.extend(executable_expression_children(&expression.kind));
    }
    Ok(reachable)
}

fn build_erased_dependency_uses(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    distributed: &DistributedReferences,
    scope_index: &ErasedScopeIndex,
) -> Result<Vec<ErasedDependencyUse>, String> {
    let reachable = reachable_executable_expression_ids(executable, materializations)?;
    let mut subjects = BTreeSet::<(ExecutableExprId, ErasedDependencyTarget)>::new();
    for read in &scope_index.reads {
        if !matches!(read.target, ErasedReadTarget::ExternalValue { .. }) {
            continue;
        }
        subjects.insert((
            read.expression,
            ErasedDependencyTarget::ExternalRead { read: read.id },
        ));
    }
    for (reference, call) in distributed.calls.iter().enumerate() {
        let expression = executable
            .expressions
            .get(call.expression.as_usize())
            .filter(|expression| expression.id == call.expression)
            .ok_or_else(|| {
                format!(
                    "distributed call `{}` references missing executable expression {}",
                    call.canonical_function, call.expression
                )
            })?;
        if !reachable.contains(&expression.id)
            || !matches!(
                &expression.kind,
                ExecutableExpressionKind::Call {
                    callable_kind: ExecutableCallableKind::External,
                    name,
                    ..
                } if name == &call.canonical_function
            )
        {
            return Err(format!(
                "distributed call `{}` does not reference a reachable matching external call",
                call.canonical_function
            ));
        }
        subjects.insert((
            expression.id,
            ErasedDependencyTarget::ExternalCall { reference },
        ));
    }
    let mut dependencies = Vec::new();
    for (expression, target) in subjects {
        if let Some(dependency) = erased_dependency_use_for_expression(
            executable,
            materializations,
            scope_index,
            expression,
            target,
        )? {
            dependencies.push(dependency);
        }
    }
    dependencies.sort_by_key(|dependency| {
        (
            dependency.dependent,
            dependency.expression,
            dependency.target.clone(),
        )
    });
    Ok(dependencies)
}

fn erased_dependency_use_for_expression(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    scope_index: &ErasedScopeIndex,
    expression: ExecutableExprId,
    target: ErasedDependencyTarget,
) -> Result<Option<ErasedDependencyUse>, String> {
    let mut candidates = scope_index
        .bindings
        .iter()
        .filter_map(|binding| {
            executable_expression_distance(
                executable,
                materializations,
                binding.producer,
                expression,
            )
            .map(|distance| (binding, distance))
        })
        .collect::<Vec<_>>();
    let priority = |binding: &ErasedBinding| match binding.target {
        ErasedBindingTarget::State { .. } => 0_u8,
        ErasedBindingTarget::Source { .. } => 1,
        ErasedBindingTarget::Value { .. } => 2,
    };
    candidates.sort_by(|(left, left_distance), (right, right_distance)| {
        left_distance
            .cmp(right_distance)
            .then_with(|| priority(left).cmp(&priority(right)))
            .then_with(|| right.owner_ancestry.len().cmp(&left.owner_ancestry.len()))
            .then_with(|| left.id.cmp(&right.id))
    });
    let Some((dependent, distance)) = candidates.first().copied() else {
        return Ok(None);
    };
    let equally_specific = candidates
        .iter()
        .take_while(|(candidate, candidate_distance)| {
            *candidate_distance == distance
                && priority(candidate) == priority(dependent)
                && candidate.owner_ancestry.len() == dependent.owner_ancestry.len()
        })
        .map(|(candidate, _)| candidate.id)
        .collect::<Vec<_>>();
    if equally_specific.len() != 1 {
        return Err(format!(
            "external dependency expression {expression} has {} equally specific erased owners {equally_specific:?}",
            equally_specific.len()
        ));
    }
    let timing = match dependent.target {
        ErasedBindingTarget::State { runtime, .. } => ErasedDependencyTiming::After {
            boundaries: vec![ErasedTemporalBoundary::State(runtime)],
        },
        ErasedBindingTarget::Source { runtime, .. } => ErasedDependencyTiming::After {
            boundaries: vec![ErasedTemporalBoundary::Source(runtime)],
        },
        ErasedBindingTarget::Value { .. } => ErasedDependencyTiming::Immediate,
    };
    Ok(Some(ErasedDependencyUse {
        dependent: dependent.id,
        expression,
        target,
        timing,
    }))
}

fn executable_expression_distance(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    root: ExecutableExprId,
    target: ExecutableExprId,
) -> Option<usize> {
    let mut pending = vec![(root, 0_usize)];
    let mut best = BTreeMap::<ExecutableExprId, usize>::new();
    while let Some((expression_id, distance)) = pending.pop() {
        if best
            .get(&expression_id)
            .is_some_and(|known| *known <= distance)
        {
            continue;
        }
        best.insert(expression_id, distance);
        if expression_id == target {
            continue;
        }
        let expression = executable
            .expressions
            .get(expression_id.as_usize())
            .filter(|candidate| candidate.id == expression_id)?;
        let next_distance = distance.saturating_add(1);
        if let ExecutableExpressionKind::Materialize { materialization } = expression.kind {
            let materialization = materializations
                .get(materialization)
                .filter(|candidate| candidate.id == materialization)?;
            pending.extend(
                materialization
                    .expression_roots()
                    .into_iter()
                    .map(|root| (root, next_distance)),
            );
        }
        pending.extend(
            executable_expression_children(&expression.kind)
                .into_iter()
                .map(|child| (child, next_distance)),
        );
    }
    best.get(&target).copied()
}

fn collect_local_read_targets(
    executable: &ExecutableProgram,
    expression_id: ExecutableExprId,
    locals: &BTreeMap<boon_typecheck::DeclId, ExecutableExprId>,
    active: &mut BTreeSet<ExecutableExprId>,
    targets: &mut BTreeMap<ExecutableExprId, ErasedReadTarget>,
) -> Result<(), String> {
    if !active.insert(expression_id) {
        return Ok(());
    }
    let expression = executable
        .expressions
        .get(expression_id.as_usize())
        .filter(|expression| expression.id == expression_id)
        .ok_or_else(|| {
            format!("read binding traversal reaches missing expression {expression_id}")
        })?;
    match &expression.kind {
        ExecutableExpressionKind::LocalRead {
            declaration,
            projection,
        } => {
            let Some(value) = locals.get(declaration).copied() else {
                active.remove(&expression_id);
                return Ok(());
            };
            let target = ErasedReadTarget::Local {
                declaration: *declaration,
                value,
                projection: projection.clone(),
            };
            if let Some(previous) = targets.insert(expression.id, target.clone())
                && previous != target
            {
                return Err(format!(
                    "local read {} resolves to incompatible erased bindings",
                    expression.id
                ));
            }
        }
        ExecutableExpressionKind::Block { bindings, result } => {
            let mut nested = locals.clone();
            for binding in bindings {
                nested.insert(binding.declaration, binding.value);
            }
            for binding in bindings {
                collect_local_read_targets(executable, binding.value, &nested, active, targets)?;
            }
            collect_local_read_targets(executable, *result, &nested, active, targets)?;
        }
        _ => {
            for child in executable_expression_children(&expression.kind) {
                collect_local_read_targets(executable, child, locals, active, targets)?;
            }
        }
    }
    active.remove(&expression_id);
    Ok(())
}

fn verify_erased_scope_index(program: &ErasedProgram) -> Result<(), String> {
    for (index, owner) in program.scope_index.owners.iter().enumerate() {
        if owner.id != StaticOwnerId(index) {
            return Err(format!(
                "erased owner at index {index} has non-dense ID {}",
                owner.id
            ));
        }
        if owner.authority_row != owner.target_row.or(owner.source_row) {
            return Err(format!(
                "erased owner {} has inconsistent authority row",
                owner.id
            ));
        }
    }
    for (index, field) in program.scope_index.fields.iter().enumerate() {
        if field.id != FieldId(index) {
            return Err(format!(
                "erased field at index {index} has non-dense ID {}",
                field.id
            ));
        }
        if let Some(row) = field.row {
            verify_erased_row(program, row, &format!("FieldId {}", field.id))?;
        }
        if field.role == ErasedFieldRole::ListAuthority
            && (field.row.is_none() || field.declaration.is_some() || field.producer.is_some())
        {
            return Err(format!(
                "list authority FieldId {} must be hidden row storage without a declaration or producer",
                field.id
            ));
        }
        if field.role == ErasedFieldRole::Capture
            && (field.row.is_none()
                || field.declaration.is_some()
                || field.producer.is_some()
                || field.static_owner.is_some())
        {
            return Err(format!(
                "capture FieldId {} must be hidden row storage without a declaration, producer, or semantic owner",
                field.id
            ));
        }
    }
    for local in &program.scope_index.locals {
        if let Some(row) = local.row {
            verify_erased_row(
                program,
                row,
                &format!("owner {} local {}", local.owner, local.local.0),
            )?;
        }
        if program
            .executable
            .expressions
            .get(local.source.as_usize())
            .is_none_or(|expression| expression.id != local.source)
        {
            return Err(format!(
                "owner {} local {} references missing source {}",
                local.owner, local.local.0, local.source
            ));
        }
        let target_row = program
            .scope_index
            .owners
            .get(local.owner.as_usize())
            .filter(|owner| owner.id == local.owner)
            .and_then(|owner| owner.target_row);
        let mut capture_identities = BTreeSet::new();
        for capture in &local.captures {
            if !capture_identities.insert((
                capture.source_owner,
                capture.source_local,
                capture.projection.clone(),
            )) {
                return Err(format!(
                    "owner {} local {} has a duplicate detached capture",
                    local.owner, local.local.0
                ));
            }
            if !program.scope_index.locals.iter().any(|source| {
                source.owner == capture.source_owner && source.local == capture.source_local
            }) {
                return Err(format!(
                    "owner {} local {} capture references missing source owner {} local {}",
                    local.owner, local.local.0, capture.source_owner, capture.source_local.0
                ));
            }
            if !program
                .scope_index
                .owner_descends_from(local.owner, capture.source_owner)?
            {
                return Err(format!(
                    "owner {} local {} capture source owner {} is not an ancestor",
                    local.owner, local.local.0, capture.source_owner
                ));
            }
            let field = program
                .scope_index
                .fields
                .get(capture.field.as_usize())
                .filter(|field| field.id == capture.field)
                .ok_or_else(|| {
                    format!(
                        "owner {} local {} capture references missing FieldId {}",
                        local.owner, local.local.0, capture.field
                    )
                })?;
            if field.role != ErasedFieldRole::Capture || field.row != target_row {
                return Err(format!(
                    "owner {} local {} capture FieldId {} is not hidden storage on its target row",
                    local.owner, local.local.0, capture.field
                ));
            }
        }
        let list_name = local
            .row
            .and_then(|row| program.lists.get(row.list.as_usize()))
            .filter(|list| local.row.is_some_and(|row| list.id == row.list))
            .map(|list| list.name.as_str());
        let relative_path = |path: &str| {
            list_name
                .and_then(|list| path.strip_prefix(list))
                .and_then(|suffix| suffix.strip_prefix('.'))
                .map(|suffix| suffix.split('.').map(str::to_owned).collect::<Vec<_>>())
        };
        let mut paths = BTreeSet::new();
        for member in &local.members {
            if member.path.is_empty()
                || member.path.iter().any(String::is_empty)
                || !paths.insert(member.path.clone())
            {
                return Err(format!(
                    "owner {} local {} contains an empty or duplicate member path `{}`",
                    local.owner,
                    local.local.0,
                    member.path.join(".")
                ));
            }
            match member.target {
                ErasedLocalMemberTarget::Field(field) => {
                    if member.forwarded_from.is_some() {
                        return Err(format!(
                            "owner {} local {} scalar member `{}` has resource forwarding metadata",
                            local.owner,
                            local.local.0,
                            member.path.join(".")
                        ));
                    }
                    let field = program
                        .scope_index
                        .fields
                        .get(field.as_usize())
                        .filter(|candidate| candidate.id == field)
                        .ok_or_else(|| {
                            format!(
                                "owner {} local {} member `{}` references missing FieldId {field}",
                                local.owner,
                                local.local.0,
                                member.path.join(".")
                            )
                        })?;
                    if field.row != local.row
                        || member.path.len() != 1
                        || member.path[0] != field.name
                    {
                        return Err(format!(
                            "owner {} local {} member `{}` is inconsistent with FieldId {}",
                            local.owner,
                            local.local.0,
                            member.path.join("."),
                            field.id
                        ));
                    }
                }
                ErasedLocalMemberTarget::Source(source) => {
                    let source = program
                        .sources
                        .get(source.as_usize())
                        .filter(|candidate| candidate.id == source)
                        .ok_or_else(|| {
                            format!(
                                "owner {} local {} member `{}` references missing SourceId {source}",
                                local.owner, local.local.0, member.path.join(".")
                            )
                        })?;
                    if let Some(forwarding) = member.forwarded_from.as_ref() {
                        let upstream = program
                            .scope_index
                            .locals
                            .iter()
                            .find(|candidate| {
                                candidate.owner == forwarding.owner
                                    && candidate.local == forwarding.local
                            })
                            .ok_or_else(|| {
                                format!(
                                    "owner {} local {} member `{}` forwards from missing local {}:{}",
                                    local.owner,
                                    local.local.0,
                                    member.path.join("."),
                                    forwarding.owner,
                                    forwarding.local.0
                                )
                            })?;
                        let upstream_members = upstream
                            .members
                            .iter()
                            .filter(|candidate| {
                                candidate.path == forwarding.path
                                    && candidate.target
                                        == ErasedLocalMemberTarget::Source(source.id)
                            })
                            .collect::<Vec<_>>();
                        if upstream_members.len() != 1 {
                            return Err(format!(
                                "owner {} local {} member `{}` forwards source {} from {} exact upstream members",
                                local.owner,
                                local.local.0,
                                member.path.join("."),
                                source.id,
                                upstream_members.len()
                            ));
                        }
                        let Some(root_name) = member.path.first() else {
                            unreachable!("empty member paths were rejected above");
                        };
                        let mut forwarding_paths = BTreeSet::new();
                        for field in program.scope_index.fields.iter().filter(|field| {
                            field.row == local.row
                                && field.name == *root_name
                                && field.resource_only
                        }) {
                            let Some((owner, source_local, projection)) =
                                resource_field_local_projection(
                                    &program.executable,
                                    field,
                                    &program.scope_index.fields,
                                    &program.materializations,
                                    &program.scope_index.locals,
                                )?
                            else {
                                continue;
                            };
                            if owner != forwarding.owner || source_local != forwarding.local {
                                continue;
                            }
                            let Some(suffix) = forwarding.path.strip_prefix(projection.as_slice())
                            else {
                                continue;
                            };
                            let mut expected = vec![field.name.clone()];
                            expected.extend_from_slice(suffix);
                            if expected == member.path {
                                forwarding_paths.insert((owner, source_local, projection));
                            }
                        }
                        if forwarding_paths.len() != 1 {
                            return Err(format!(
                                "owner {} local {} member `{}` has {} exact resource forwarding paths",
                                local.owner,
                                local.local.0,
                                member.path.join("."),
                                forwarding_paths.len()
                            ));
                        }
                    } else if source.scope_id != local.row.map(|row| row.scope)
                        || relative_path(&source.path).is_none_or(|path| path != member.path)
                    {
                        return Err(format!(
                            "owner {} local {} member `{}` is inconsistent with source `{}`",
                            local.owner,
                            local.local.0,
                            member.path.join("."),
                            source.path
                        ));
                    }
                }
                ErasedLocalMemberTarget::State(state) => {
                    if member.forwarded_from.is_some() {
                        return Err(format!(
                            "owner {} local {} state member `{}` has unsupported forwarding metadata",
                            local.owner,
                            local.local.0,
                            member.path.join(".")
                        ));
                    }
                    let state = program
                        .state_cells
                        .get(state.as_usize())
                        .filter(|candidate| candidate.id == state)
                        .ok_or_else(|| {
                            format!(
                                "owner {} local {} member `{}` references missing StateId {state}",
                                local.owner,
                                local.local.0,
                                member.path.join(".")
                            )
                        })?;
                    if state.scope_id != local.row.map(|row| row.scope)
                        || relative_path(&state.path).is_none_or(|path| path != member.path)
                    {
                        return Err(format!(
                            "owner {} local {} member `{}` is inconsistent with state `{}`",
                            local.owner,
                            local.local.0,
                            member.path.join("."),
                            state.path
                        ));
                    }
                }
            }
        }
    }
    for (index, binding) in program.scope_index.bindings.iter().enumerate() {
        if binding.id != ErasedBindingId(index) {
            return Err(format!(
                "storage binding at index {index} has non-dense ID {}",
                binding.id
            ));
        }
        if binding.owner_ancestry.last().copied() != binding.static_owner {
            return Err(format!(
                "storage binding {} has owner {:?} but ancestry {:?}",
                binding.id, binding.static_owner, binding.owner_ancestry
            ));
        }
        for (depth, owner) in binding.owner_ancestry.iter().copied().enumerate() {
            let definition = program
                .scope_index
                .owners
                .get(owner.as_usize())
                .filter(|definition| definition.id == owner)
                .ok_or_else(|| {
                    format!(
                        "storage binding {} references missing owner {owner}",
                        binding.id
                    )
                })?;
            let expected_parent = depth
                .checked_sub(1)
                .map(|parent| binding.owner_ancestry[parent]);
            if definition.parent != expected_parent {
                return Err(format!(
                    "storage binding {} owner ancestry is not structural at {owner}",
                    binding.id
                ));
            }
        }
        let expression = program
            .executable
            .expressions
            .get(binding.producer.as_usize())
            .filter(|expression| expression.id == binding.producer)
            .ok_or_else(|| {
                format!(
                    "storage binding {} references missing producer {}",
                    binding.id, binding.producer
                )
            })?;
        if expression.flow_type != binding.flow_type {
            return Err(format!(
                "storage binding {} declaration {} (`{}`) type {:?} differs from producer {} checked {} owner {:?} kind {:?} type {:?}",
                binding.id,
                binding.declaration.0,
                binding.diagnostic_path,
                binding.flow_type,
                binding.producer,
                expression.checked_expr_id.0,
                expression.owner,
                expression.kind,
                expression.flow_type,
            ));
        }
        match binding.target {
            ErasedBindingTarget::Value { field, row } => {
                if let Some(field) = field
                    && !program
                        .scope_index
                        .fields
                        .iter()
                        .any(|value| value.id == field)
                {
                    return Err(format!(
                        "erased binding {} references missing FieldId {field}",
                        binding.id
                    ));
                }
                if let Some(row) = row {
                    verify_erased_row(program, row, &format!("binding {}", binding.id))?;
                }
            }
            ErasedBindingTarget::Source {
                executable,
                runtime,
            } => {
                if !program.executable.sources.iter().any(|source| {
                    source.id == executable && source.declaration == binding.declaration
                }) || !program.sources.iter().any(|source| {
                    source.id == runtime && source.executable_source_id == Some(executable)
                }) {
                    return Err(format!(
                        "storage binding {} has an invalid source allocation",
                        binding.id
                    ));
                }
            }
            ErasedBindingTarget::State {
                executable,
                runtime,
                ..
            } => {
                if !program
                    .executable
                    .states
                    .iter()
                    .any(|state| state.id == executable && state.declaration == binding.declaration)
                    || !program.state_cells.iter().any(|state| {
                        state.id == runtime && state.executable_state_id == Some(executable)
                    })
                {
                    return Err(format!(
                        "storage binding {} has an invalid state allocation",
                        binding.id
                    ));
                }
            }
        }
    }
    for (index, source) in program.scope_index.sources.iter().enumerate() {
        let expected_id = SourceId(index);
        if source.source != expected_id {
            return Err(format!(
                "erased source at index {index} has non-dense SourceId {}",
                source.source
            ));
        }
        let runtime = program
            .sources
            .get(index)
            .filter(|candidate| candidate.id == source.source)
            .ok_or_else(|| format!("erased source {} has no runtime source", source.source))?;
        if source.static_owner != runtime.static_owner
            || source.owner_ancestry.last().copied() != source.static_owner
        {
            return Err(format!(
                "erased source {} has inconsistent structural ownership",
                source.source
            ));
        }
        for (depth, owner) in source.owner_ancestry.iter().copied().enumerate() {
            let definition = program
                .scope_index
                .owners
                .get(owner.as_usize())
                .filter(|definition| definition.id == owner)
                .ok_or_else(|| {
                    format!(
                        "erased source {} references missing owner {owner}",
                        source.source
                    )
                })?;
            let expected_parent = depth
                .checked_sub(1)
                .map(|parent| source.owner_ancestry[parent]);
            if definition.parent != expected_parent {
                return Err(format!(
                    "erased source {} owner ancestry is not structural at {owner}",
                    source.source
                ));
            }
        }
        match source.origin {
            ErasedSourceOrigin::Executable {
                executable,
                binding,
            } => {
                let valid_binding = program
                    .scope_index
                    .bindings
                    .get(binding.as_usize())
                    .is_some_and(|candidate| {
                        candidate.id == binding
                            && matches!(
                                candidate.target,
                                ErasedBindingTarget::Source {
                                    executable: candidate_executable,
                                    runtime: candidate_runtime,
                                } if candidate_executable == executable
                                    && candidate_runtime == source.source
                            )
                    });
                if runtime.executable_source_id != Some(executable) || !valid_binding {
                    return Err(format!(
                        "erased source {} has an invalid executable origin",
                        source.source
                    ));
                }
            }
            ErasedSourceOrigin::DistributedImport => {
                if runtime.executable_source_id.is_some()
                    || !runtime.path.starts_with("@distributed/")
                    || source.static_owner.is_some()
                {
                    return Err(format!(
                        "erased source {} has an invalid distributed ingress origin",
                        source.source
                    ));
                }
            }
        }
    }
    let direct_storage_statements = direct_erased_storage_statements(&program.executable);
    for statement in &program.executable.statements {
        if !direct_storage_statements.contains(&statement.id) {
            continue;
        }
        let Some(declaration) = statement.declaration else {
            continue;
        };
        let Some(flow_type) = &statement.flow_type else {
            return Err(format!(
                "executable declaration {} statement {} has no final checked type",
                declaration.0, statement.id
            ));
        };
        if flow_type.mode != boon_typecheck::FlowMode::Continuous
            || !matches!(
                &flow_type.ty,
                boon_typecheck::Type::List(item)
                    if matches!(item.as_ref(), boon_typecheck::Type::Object(_))
            )
        {
            continue;
        }
        let matches = program
            .scope_index
            .bindings
            .iter()
            .filter(|binding| {
                binding.declaration == declaration
                    && matches!(
                        binding.target,
                        ErasedBindingTarget::Value { row: Some(_), .. }
                    )
            })
            .count();
        if matches != 1 {
            return Err(format!(
                "typed list declaration {} must have one ListId storage binding, found {matches}",
                declaration.0
            ));
        }
    }
    let mut read_expressions = BTreeSet::new();
    for (index, read) in program.scope_index.reads.iter().enumerate() {
        if read.id != ErasedReadId(index) {
            return Err(format!(
                "erased read at index {index} has non-dense ID {}",
                read.id
            ));
        }
        let expression = program
            .executable
            .expressions
            .get(read.expression.as_usize())
            .filter(|expression| expression.id == read.expression)
            .ok_or_else(|| {
                format!(
                    "erased read {} references missing expression {}",
                    read.id, read.expression
                )
            })?;
        if !matches!(
            expression.kind,
            ExecutableExpressionKind::CanonicalRead { .. }
                | ExecutableExpressionKind::LocalRead { .. }
                | ExecutableExpressionKind::ExternalRead { .. }
                | ExecutableExpressionKind::Drain { .. }
                | ExecutableExpressionKind::MaterializationLocal { .. }
                | ExecutableExpressionKind::FunctionParameter { .. }
        ) {
            return Err(format!(
                "erased read {} targets non-read expression {}",
                read.id, read.expression
            ));
        }
        if !read_expressions.insert(read.expression) {
            return Err(format!(
                "executable expression {} has multiple erased read targets",
                read.expression
            ));
        }
        match &read.target {
            ErasedReadTarget::Binding {
                binding,
                projection,
            } => {
                let binding = program
                    .scope_index
                    .bindings
                    .get(binding.as_usize())
                    .filter(|candidate| candidate.id == *binding)
                    .ok_or_else(|| {
                        format!("erased read {} references missing {binding}", read.id)
                    })?;
                if !projection.is_empty()
                    && matches!(
                        binding.target,
                        ErasedBindingTarget::Source { .. } | ErasedBindingTarget::State { .. }
                    )
                {
                    return Err(format!(
                        "erased read {} leaves a projection on a source/state binding",
                        read.id
                    ));
                }
            }
            ErasedReadTarget::SourcePayload {
                binding,
                source,
                field,
                ..
            } => {
                if !program.scope_index.bindings.iter().any(|candidate| {
                    candidate.id == *binding
                        && matches!(
                            candidate.target,
                            ErasedBindingTarget::Source { runtime, .. } if runtime == *source
                        )
                }) {
                    return Err(format!(
                        "erased read {} has mismatched source binding {binding}",
                        read.id
                    ));
                }
                let source = program
                    .sources
                    .get(source.as_usize())
                    .filter(|candidate| candidate.id == *source)
                    .ok_or_else(|| {
                        format!(
                            "erased read {} references missing SourceId {source}",
                            read.id
                        )
                    })?;
                if !source.payload_schema.fields.contains(field) {
                    return Err(format!(
                        "erased read {} references absent payload field {field:?} on `{}`",
                        read.id, source.path
                    ));
                }
            }
            ErasedReadTarget::StateProjection {
                binding,
                state,
                fields,
            } => {
                if fields.is_empty() {
                    return Err(format!(
                        "erased read {} has an empty state projection",
                        read.id
                    ));
                }
                if !program
                    .state_cells
                    .iter()
                    .any(|candidate| candidate.id == *state)
                {
                    return Err(format!(
                        "erased read {} references missing StateId {state}",
                        read.id
                    ));
                }
                if !program.scope_index.bindings.iter().any(|candidate| {
                    candidate.id == *binding
                        && matches!(
                            candidate.target,
                            ErasedBindingTarget::State { runtime, .. } if runtime == *state
                        )
                }) {
                    return Err(format!(
                        "erased read {} has mismatched state binding {binding}",
                        read.id
                    ));
                }
            }
            ErasedReadTarget::Expression {
                expression: target, ..
            } => {
                if *target == read.expression {
                    return Err(format!(
                        "erased read {} recursively targets itself",
                        read.id
                    ));
                }
                if !program
                    .executable
                    .expressions
                    .iter()
                    .any(|candidate| candidate.id == *target)
                {
                    return Err(format!(
                        "erased read {} references missing expression {target}",
                        read.id
                    ));
                }
            }
            ErasedReadTarget::Local { value, .. } => {
                if !program
                    .executable
                    .expressions
                    .iter()
                    .any(|candidate| candidate.id == *value)
                {
                    return Err(format!(
                        "erased read {} references missing local expression {value}",
                        read.id
                    ));
                }
            }
            ErasedReadTarget::ExternalValue { reference } => {
                if program
                    .distributed_references
                    .value_references
                    .get(*reference)
                    .is_none()
                {
                    return Err(format!(
                        "erased read {} references missing external value {reference}",
                        read.id
                    ));
                }
            }
            ErasedReadTarget::MaterializationLocal { owner, local, .. } => {
                if !program
                    .scope_index
                    .locals
                    .iter()
                    .any(|candidate| candidate.owner == *owner && candidate.local == *local)
                {
                    return Err(format!(
                        "erased read {} references missing owner {owner} local {}",
                        read.id, local.0
                    ));
                }
            }
            ErasedReadTarget::FunctionParameter { parameter, .. } => {
                if !program.executable.functions.iter().any(|function| {
                    function.id == parameter.function
                        && function
                            .parameters
                            .iter()
                            .any(|candidate| candidate.id == *parameter)
                }) {
                    return Err(format!(
                        "erased read {} references missing function parameter {:?}",
                        read.id, parameter
                    ));
                }
            }
        }
    }
    let mut row_value_keys = BTreeSet::new();
    for row_value in &program.scope_index.row_values {
        if program
            .executable
            .expressions
            .get(row_value.expression.as_usize())
            .is_none_or(|expression| expression.id != row_value.expression)
        {
            return Err(format!(
                "erased row value references missing expression {}",
                row_value.expression
            ));
        }
        if row_value.projection.iter().any(String::is_empty)
            || !row_value_keys.insert((row_value.expression, row_value.projection.clone()))
        {
            return Err(format!(
                "erased row value expression {} has an empty or duplicate projection `{}`",
                row_value.expression,
                row_value.projection.join(".")
            ));
        }
        verify_erased_row(
            program,
            row_value.row,
            &format!(
                "row value {} projection `{}`",
                row_value.expression,
                row_value.projection.join(".")
            ),
        )?;
    }
    for dependency in &program.scope_index.dependencies {
        let dependent = program
            .scope_index
            .bindings
            .get(dependency.dependent.as_usize())
            .filter(|binding| binding.id == dependency.dependent)
            .ok_or_else(|| {
                format!(
                    "erased dependency references missing binding {}",
                    dependency.dependent
                )
            })?;
        let expression = program
            .executable
            .expressions
            .get(dependency.expression.as_usize())
            .filter(|expression| expression.id == dependency.expression)
            .ok_or_else(|| {
                format!(
                    "erased dependency references missing expression {}",
                    dependency.expression
                )
            })?;
        match dependency.target {
            ErasedDependencyTarget::ExternalRead { read: read_id } => {
                let read = program
                    .scope_index
                    .reads
                    .get(read_id.as_usize())
                    .filter(|read| read.id == read_id && read.expression == dependency.expression)
                    .ok_or_else(|| {
                        format!(
                            "erased dependency expression {} references a missing read",
                            dependency.expression
                        )
                    })?;
                if !matches!(read.target, ErasedReadTarget::ExternalValue { .. }) {
                    return Err(format!(
                        "erased dependency expression {} references a non-external read",
                        dependency.expression
                    ));
                }
            }
            ErasedDependencyTarget::ExternalCall { reference } => {
                let call = program
                    .distributed_references
                    .calls
                    .get(reference)
                    .ok_or_else(|| {
                        format!(
                            "erased dependency expression {} references missing external call {reference}",
                            dependency.expression
                        )
                    })?;
                if !matches!(
                    &expression.kind,
                    ExecutableExpressionKind::Call {
                        callable_kind: ExecutableCallableKind::External,
                        name,
                        ..
                    } if name == &call.canonical_function
                ) {
                    return Err(format!(
                        "erased dependency expression {} does not match external call {reference}",
                        dependency.expression
                    ));
                }
            }
        }
        match (&dependency.timing, &dependent.target) {
            (ErasedDependencyTiming::Immediate, ErasedBindingTarget::Value { .. }) => {}
            (
                ErasedDependencyTiming::After { boundaries },
                ErasedBindingTarget::State { runtime, .. },
            ) if boundaries.as_slice() == [ErasedTemporalBoundary::State(*runtime)] => {}
            (
                ErasedDependencyTiming::After { boundaries },
                ErasedBindingTarget::Source { runtime, .. },
            ) if boundaries.as_slice() == [ErasedTemporalBoundary::Source(*runtime)] => {}
            _ => {
                return Err(format!(
                    "erased dependency expression {} has timing inconsistent with binding {}",
                    dependency.expression, dependency.dependent
                ));
            }
        }
    }
    Ok(())
}

fn verify_erased_row(
    program: &ErasedProgram,
    row: ErasedRowBinding,
    context: &str,
) -> Result<(), String> {
    let list = program
        .lists
        .get(row.list.as_usize())
        .filter(|list| list.id == row.list)
        .ok_or_else(|| format!("{context} references missing ListId {}", row.list))?;
    if list.row_scope_id != Some(row.scope) {
        return Err(format!(
            "{context} row scope {} differs from ListId {} scope {:?}",
            row.scope, row.list, list.row_scope_id
        ));
    }
    Ok(())
}

fn validate_checked_program_for_lowering(
    program: &boon_typecheck::CheckedProgram,
) -> Result<(), String> {
    let scopes = program
        .scopes
        .iter()
        .map(|scope| scope.id)
        .collect::<BTreeSet<_>>();
    if !scopes.contains(&program.root_scope) {
        return Err("CheckedProgram root scope is missing".to_owned());
    }
    let declarations = program
        .declarations
        .iter()
        .map(|declaration| declaration.id)
        .collect::<BTreeSet<_>>();
    let expressions = program
        .expressions
        .iter()
        .map(|expression| expression.id)
        .collect::<BTreeSet<_>>();
    let statements = program
        .statements
        .iter()
        .map(|statement| statement.id)
        .collect::<BTreeSet<_>>();
    let calls = program
        .calls
        .iter()
        .map(|call| call.id)
        .collect::<BTreeSet<_>>();
    for source in &program.sources {
        let expression = source.expression.ok_or_else(|| {
            format!(
                "checked source `{}` has no exact expression ownership",
                source.path
            )
        })?;
        let declaration = source.declaration.ok_or_else(|| {
            format!(
                "checked source `{}` has no exact declaration ownership",
                source.path
            )
        })?;
        let checked_expression = program
            .expressions
            .iter()
            .find(|candidate| candidate.id == expression)
            .ok_or_else(|| {
                format!(
                    "checked source `{}` references missing expression {}",
                    source.path, expression.0
                )
            })?;
        if checked_expression.declaration != Some(declaration)
            || !declarations.contains(&declaration)
        {
            return Err(format!(
                "checked source `{}` expression {} and declaration {} disagree",
                source.path, expression.0, declaration.0
            ));
        }
    }
    for scope in &program.scopes {
        if scope.parent.is_some_and(|parent| !scopes.contains(&parent)) {
            return Err(format!("checked scope {} has a missing parent", scope.id.0));
        }
        if scope
            .owner
            .is_some_and(|owner| !declarations.contains(&owner))
        {
            return Err(format!("checked scope {} has a missing owner", scope.id.0));
        }
    }
    for declaration in &program.declarations {
        if !scopes.contains(&declaration.scope_id) {
            return Err(format!(
                "checked declaration {} has a missing scope",
                declaration.id.0
            ));
        }
        if declaration
            .body_scope
            .is_some_and(|scope| !scopes.contains(&scope))
        {
            return Err(format!(
                "checked declaration {} has a missing body scope",
                declaration.id.0
            ));
        }
        if declaration
            .value
            .is_some_and(|expression| !expressions.contains(&expression))
        {
            return Err(format!(
                "checked declaration {} has a missing value expression",
                declaration.id.0
            ));
        }
    }
    for statement in &program.statements {
        if !scopes.contains(&statement.scope_id) {
            return Err(format!(
                "checked statement {} has a missing scope",
                statement.id.0
            ));
        }
        if statement
            .value
            .is_some_and(|expression| !expressions.contains(&expression))
            || statement
                .children
                .iter()
                .any(|child| !statements.contains(child))
        {
            return Err(format!(
                "checked statement {} references a missing child",
                statement.id.0
            ));
        }
        let declaration = match statement.kind {
            boon_typecheck::CheckedStatementKind::Function { declaration }
            | boon_typecheck::CheckedStatementKind::Field { declaration } => Some(declaration),
            boon_typecheck::CheckedStatementKind::Source { declaration, .. }
            | boon_typecheck::CheckedStatementKind::Hold { declaration, .. }
            | boon_typecheck::CheckedStatementKind::List { declaration, .. } => declaration,
            boon_typecheck::CheckedStatementKind::Block
            | boon_typecheck::CheckedStatementKind::Spread
            | boon_typecheck::CheckedStatementKind::Expression => None,
        };
        if declaration.is_some_and(|declaration| !declarations.contains(&declaration)) {
            return Err(format!(
                "checked statement {} references a missing declaration",
                statement.id.0
            ));
        }
    }
    let callables = program
        .callables
        .iter()
        .map(|callable| callable.decl_id)
        .collect::<BTreeSet<_>>();
    for call in &program.calls {
        if !expressions.contains(&call.expression) {
            return Err(format!(
                "checked call {} has a missing expression",
                call.id.0
            ));
        }
        if !callables.contains(&call.callable) {
            return Err(format!(
                "checked call {} references missing callable {}",
                call.expression.0, call.callable.0
            ));
        }
        if call.entries.iter().any(|entry| match entry {
            boon_typecheck::CheckedCallEntry::Input { formal, .. }
            | boon_typecheck::CheckedCallEntry::FreshOut { formal, .. }
            | boon_typecheck::CheckedCallEntry::ForwardOut { formal, .. } => {
                !program.callables.iter().any(|callable| {
                    callable.decl_id == call.callable
                        && callable
                            .parameters
                            .iter()
                            .any(|parameter| parameter.decl_id == *formal)
                })
            }
        }) {
            return Err(format!(
                "checked call {} contains a formal from another callable",
                call.expression.0
            ));
        }
        for entry in &call.entries {
            match entry {
                boon_typecheck::CheckedCallEntry::Input {
                    value,
                    evaluation_scope,
                    ..
                } => {
                    if !expressions.contains(value) {
                        return Err(format!(
                            "checked call {} references a missing input expression",
                            call.id.0
                        ));
                    }
                    if let boon_typecheck::CheckedEvaluationScope::Output { formal } =
                        evaluation_scope
                        && !call.entries.iter().any(|candidate| match candidate {
                            boon_typecheck::CheckedCallEntry::FreshOut {
                                formal: output_formal,
                                ..
                            }
                            | boon_typecheck::CheckedCallEntry::ForwardOut {
                                formal: output_formal,
                                ..
                            } => output_formal == formal,
                            boon_typecheck::CheckedCallEntry::Input { .. } => false,
                        })
                    {
                        return Err(format!(
                            "checked call {} evaluates an input under an unbound OUT formal",
                            call.id.0
                        ));
                    }
                }
                boon_typecheck::CheckedCallEntry::FreshOut {
                    output, scope_id, ..
                } => {
                    if !declarations.contains(output) || !scopes.contains(scope_id) {
                        return Err(format!(
                            "checked call {} has an incomplete fresh OUT binding",
                            call.id.0
                        ));
                    }
                }
                boon_typecheck::CheckedCallEntry::ForwardOut { target, .. } => {
                    if !declarations.contains(target) {
                        return Err(format!(
                            "checked call {} forwards to a missing OUT declaration",
                            call.id.0
                        ));
                    }
                }
            }
        }
        if call.pass.is_some_and(|pass| !expressions.contains(&pass)) {
            return Err(format!(
                "checked call {} has a missing PASS expression",
                call.id.0
            ));
        }
    }
    for expression in &program.expressions {
        if !scopes.contains(&expression.scope_id) {
            return Err(format!(
                "checked expression {} has a missing scope",
                expression.id.0
            ));
        }
        match &expression.kind {
            boon_typecheck::CheckedExpressionKind::Read { target, .. }
            | boon_typecheck::CheckedExpressionKind::Drain { target, .. }
                if !declarations.contains(target) =>
            {
                return Err(format!(
                    "checked expression {} reads a missing declaration",
                    expression.id.0
                ));
            }
            boon_typecheck::CheckedExpressionKind::Call { call } if !calls.contains(call) => {
                return Err(format!(
                    "checked expression {} references a missing call",
                    expression.id.0
                ));
            }
            _ => {}
        }
    }
    for occurrence in &program.occurrences {
        if !declarations.contains(&occurrence.target) {
            return Err("semantic occurrence references a missing declaration".to_owned());
        }
    }
    Ok(())
}

fn distributed_references(
    program: &boon_typecheck::CheckedProgram,
    external_types: &boon_typecheck::ExternalTypeEnvironment,
) -> Result<PendingDistributedReferences, String> {
    let mut value_references = Vec::new();
    let mut calls = Vec::new();
    for expr in &program.expressions {
        match &expr.kind {
            boon_typecheck::CheckedExpressionKind::ExternalRead { canonical_path } => {
                let Some(producer_role) = distributed_function_role(canonical_path) else {
                    continue;
                };
                let declared = external_types.values.get(canonical_path).ok_or_else(|| {
                    format!(
                        "typecheck accepted qualified external value `{canonical_path}` without an external type"
                    )
                })?;
                ensure_distributed_value_flow_is_closed(
                    &expr.flow_type,
                    &format!("qualified external value `{canonical_path}`"),
                )?;
                ensure_distributed_value_flow_is_closed(
                    declared,
                    &format!("external value declaration `{canonical_path}`"),
                )?;
                if expr.flow_type.mode != declared.mode {
                    return Err(format!(
                        "qualified external value `{canonical_path}` flow does not match its declaration"
                    ));
                }
                value_references.push(DistributedValueReference {
                    expr_id: ExprId(expr.id.0 as usize),
                    canonical_path: canonical_path.clone(),
                    local_alias_paths: Vec::new(),
                    producer_role,
                    flow_mode: declared.mode,
                    value_type: declared.ty.clone(),
                });
            }
            boon_typecheck::CheckedExpressionKind::Call { call } => {
                let checked_call = program
                    .calls
                    .iter()
                    .find(|candidate| candidate.id == *call)
                    .ok_or_else(|| {
                        format!(
                            "checked expression {} references missing call {}",
                            expr.id.0, call.0
                        )
                    })?;
                let function = &checked_call.function;
                let Some(producer_role) = distributed_function_role(function) else {
                    continue;
                };
                let signature = external_types.functions.get(function).ok_or_else(|| {
                    format!(
                        "typecheck accepted qualified external function `{function}` without an external signature"
                    )
                })?;
                ensure_distributed_value_flow_is_closed(
                    &expr.flow_type,
                    &format!("qualified external call `{function}` result"),
                )?;
                ensure_distributed_value_flow_is_closed(
                    &signature.result,
                    &format!("external function declaration `{function}` result"),
                )?;

                let mut arguments = Vec::with_capacity(checked_call.entries.len());
                for entry in &checked_call.entries {
                    let boon_typecheck::CheckedCallEntry::Input { name, value, .. } = entry else {
                        return Err(format!(
                            "typecheck accepted an OUT binding in qualified external call `{function}`"
                        ));
                    };
                    let declared_argument = signature
                        .args
                        .iter()
                        .find(|candidate| candidate.name == *name)
                        .ok_or_else(|| {
                            format!(
                                "typecheck accepted unknown argument `{name}` in qualified external call `{function}`"
                            )
                        })?;
                    ensure_distributed_value_flow_is_closed(
                        &declared_argument.flow_type,
                        &format!("external function `{function}` argument `{name}`"),
                    )?;
                    program.expressions.get(value.0 as usize).ok_or_else(|| {
                        format!(
                            "qualified external call `{function}` argument `{name}` references missing expression {}",
                            value.0
                        )
                    })?;
                    arguments.push(PendingDistributedCallArgument {
                        name: name.clone(),
                        flow_type: declared_argument.flow_type.clone(),
                    });
                }
                calls.push(PendingDistributedCall {
                    checked_expression: expr.id,
                    canonical_function: function.clone(),
                    producer_role,
                    result: expr.flow_type.clone(),
                    effect: signature.effect,
                    arguments,
                });
            }
            _ => {}
        }
    }
    Ok(PendingDistributedReferences {
        value_references,
        calls,
    })
}

fn concrete_distributed_calls(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    pending: &[PendingDistributedCall],
) -> Result<Vec<DistributedCall>, String> {
    let reachable = reachable_runtime_expression_ids(executable, materializations)?;
    let mut calls = Vec::new();
    for expression in &executable.expressions {
        if !reachable.contains(&expression.id) {
            continue;
        }
        let ExecutableExpressionKind::Call {
            callable_kind: ExecutableCallableKind::External,
            name,
            arguments,
            ..
        } = &expression.kind
        else {
            continue;
        };
        let matches = pending
            .iter()
            .filter(|call| {
                call.checked_expression == expression.checked_expr_id
                    && call.canonical_function == *name
            })
            .collect::<Vec<_>>();
        let call = match matches.as_slice() {
            [call] => *call,
            [] => {
                return Err(format!(
                    "expanded external call {} (`{name}`) has no checked distributed contract",
                    expression.id
                ));
            }
            _ => {
                return Err(format!(
                    "expanded external call {} (`{name}`) has multiple checked distributed contracts",
                    expression.id
                ));
            }
        };
        ensure_distributed_value_flow_is_closed(
            &expression.flow_type,
            &format!("expanded distributed call `{name}` result"),
        )?;
        if expression.flow_type != call.result {
            return Err(format!(
                "expanded distributed call `{name}` result flow differs from its checked contract"
            ));
        }
        let mut concrete_arguments = Vec::with_capacity(arguments.len());
        let mut names = BTreeSet::new();
        for argument in arguments {
            if !names.insert(argument.name.as_str()) {
                return Err(format!(
                    "expanded distributed call `{name}` repeats argument `{}`",
                    argument.name
                ));
            }
            let declared = call
                .arguments
                .iter()
                .find(|candidate| candidate.name == argument.name)
                .ok_or_else(|| {
                    format!(
                        "expanded distributed call `{name}` has unknown argument `{}`",
                        argument.name
                    )
                })?;
            let value = executable
                .expressions
                .get(argument.value.as_usize())
                .filter(|candidate| candidate.id == argument.value)
                .ok_or_else(|| {
                    format!(
                        "expanded distributed call `{name}` argument `{}` references missing expression {}",
                        argument.name, argument.value
                    )
                })?;
            ensure_distributed_value_flow_is_closed(
                &value.flow_type,
                &format!(
                    "expanded distributed call `{name}` argument `{}`",
                    argument.name
                ),
            )?;
            if value.flow_type.ty != declared.flow_type.ty {
                return Err(format!(
                    "expanded distributed call `{name}` argument `{}` type differs from its checked contract",
                    argument.name
                ));
            }
            concrete_arguments.push(DistributedCallArgument {
                name: argument.name.clone(),
                value: argument.value,
                flow_type: value.flow_type.clone(),
            });
        }
        if concrete_arguments.len() != call.arguments.len() {
            return Err(format!(
                "expanded distributed call `{name}` has {} arguments, expected {}",
                concrete_arguments.len(),
                call.arguments.len()
            ));
        }
        calls.push(DistributedCall {
            expression: expression.id,
            owner: expression.owner,
            canonical_function: call.canonical_function.clone(),
            producer_role: call.producer_role,
            result: expression.flow_type.clone(),
            effect: call.effect,
            arguments: concrete_arguments,
            invocation_arms: Vec::new(),
        });
    }
    calls.sort_by_key(|call| call.expression);
    Ok(calls)
}

fn distributed_function_role(function: &str) -> Option<boon_typecheck::ProgramRole> {
    function
        .split_once('/')
        .and_then(|(namespace, _)| distributed_role(namespace))
}

fn distributed_role(namespace: &str) -> Option<boon_typecheck::ProgramRole> {
    Some(match boon_parser::program_role_root(namespace)? {
        boon_parser::ProgramRoleRoot::Client => boon_typecheck::ProgramRole::Client,
        boon_parser::ProgramRoleRoot::Session => boon_typecheck::ProgramRole::Session,
        boon_parser::ProgramRoleRoot::Server => boon_typecheck::ProgramRole::Server,
    })
}

fn ensure_distributed_value_flow_is_closed(
    flow_type: &boon_typecheck::FlowType,
    context: &str,
) -> Result<(), String> {
    if flow_type.mode == boon_typecheck::FlowMode::Absent {
        return Err(format!("{context} is always absent"));
    }
    ensure_distributed_type_is_closed(&flow_type.ty, context)
}

fn ensure_distributed_type_is_closed(
    data_type: &boon_typecheck::Type,
    context: &str,
) -> Result<(), String> {
    if distributed_type_is_closed(data_type) {
        Ok(())
    } else {
        Err(format!(
            "{context} does not have a closed value type: {data_type:?}"
        ))
    }
}

fn distributed_type_is_closed(data_type: &boon_typecheck::Type) -> bool {
    match data_type {
        boon_typecheck::Type::Text
        | boon_typecheck::Type::Number
        | boon_typecheck::Type::Bytes(_) => true,
        boon_typecheck::Type::Object(shape) => {
            !shape.open && shape.fields.values().all(distributed_type_is_closed)
        }
        boon_typecheck::Type::List(item) => distributed_type_is_closed(item),
        boon_typecheck::Type::VariantSet(variants) => {
            variants.iter().all(|variant| match variant {
                boon_typecheck::Variant::Tag(_) => true,
                boon_typecheck::Variant::Tagged { fields, .. } => {
                    !fields.open && fields.fields.values().all(distributed_type_is_closed)
                }
            })
        }
        boon_typecheck::Type::Skip
        | boon_typecheck::Type::RenderContract
        | boon_typecheck::Type::Function { .. }
        | boon_typecheck::Type::UnresolvedShape { .. }
        | boon_typecheck::Type::Var(_)
        | boon_typecheck::Type::Unknown => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn semantic_index(
    checked_program: &boon_typecheck::CheckedProgram,
    row_scopes: &[RowScope],
    sources: &[SourcePort],
    lists: &[ListMemory],
    view_bindings: &[ViewBinding],
    output_values: &[OutputRootValue],
    typecheck_report: &boon_typecheck::CheckedProgramLoweringMetadata,
    semantic_fields: Vec<SemanticFieldEntry>,
) -> SemanticIndex {
    let payload_shape_by_source = typecheck_report
        .source_payload_shape_table
        .iter()
        .map(|entry| (entry.source_path.as_str(), entry.fields.len()))
        .collect::<BTreeMap<_, _>>();
    let function_types = typecheck_report
        .function_type_table
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<BTreeSet<_>>();
    let source_units = typecheck_report
        .source_units
        .iter()
        .enumerate()
        .map(|(id, file)| SemanticSourceUnit {
            id: SourceUnitId(id),
            path: file.path.clone(),
            module: file.module.clone(),
            start_line: file.start_line,
            line_count: file.line_count,
        })
        .collect::<Vec<_>>();
    let output_roots = output_values
        .iter()
        .map(|output| SemanticOutputRootEntry {
            root: output.root.clone(),
            contract: output.contract,
            demand: output.demand,
            data_type: output.data_type.clone(),
            statement_id: output.statement_id,
            line: output.line,
            typed_contract_known: output.typed_contract_known,
        })
        .collect::<Vec<_>>();
    let sources = sources
        .iter()
        .map(|source| {
            let payload_field_count = payload_shape_by_source
                .get(source.path.as_str())
                .copied()
                .unwrap_or(source.payload_schema.fields.len());
            SemanticSourceEntry {
                id: source.id,
                path: source.path.clone(),
                scoped: source.scoped,
                scope_id: source.scope_id,
                payload_schema_known: payload_shape_by_source.contains_key(source.path.as_str()),
                payload_field_count,
            }
        })
        .collect::<Vec<_>>();
    let lists = lists
        .iter()
        .map(|list| SemanticListEntry {
            id: list.id,
            name: list.name.clone(),
            row_scope_id: list.row_scope_id,
            capacity: list.capacity,
            initializer_known: !matches!(list.initializer, ListInitializer::Unknown { .. }),
        })
        .collect::<Vec<_>>();
    let row_scopes = row_scopes
        .iter()
        .map(|scope| SemanticRowScopeEntry {
            id: scope.id,
            list: scope.list.clone(),
            function: scope.function.clone(),
            row_scope: scope.row_scope.clone(),
        })
        .collect::<Vec<_>>();
    let functions = checked_program
        .callables
        .iter()
        .filter(|function| function.kind == boon_typecheck::CheckedCallableKind::User)
        .enumerate()
        .map(|(id, function)| SemanticFunctionEntry {
            id: FunctionId(id),
            name: function.name.clone(),
            args: function
                .parameters
                .iter()
                .map(|parameter| parameter.name.clone())
                .collect(),
            statement_id: function
                .body
                .map_or(usize::MAX, |statement| statement.0 as usize),
            line: checked_program
                .declarations
                .iter()
                .find(|declaration| declaration.id == function.decl_id)
                .map_or(0, |declaration| declaration.span.line),
            type_known: function_types.contains(function.name.as_str()),
        })
        .collect::<Vec<_>>();
    let view_bindings = view_bindings
        .iter()
        .map(|binding| SemanticViewBindingEntry {
            id: binding.id,
            node_kind: binding.node_kind.clone(),
            attr: binding.attr.clone(),
            path: binding.path.clone(),
            kind: binding.kind,
            scope_id: binding.scope_id,
            source_id: match binding.target {
                ViewBindingTarget::Source { source } => Some(source),
                ViewBindingTarget::Read { .. } => None,
            },
            render_contract_known: true,
        })
        .collect::<Vec<_>>();
    let diagnostic_spans = typecheck_report
        .diagnostics
        .iter()
        .enumerate()
        .map(|(id, diagnostic)| SemanticDiagnosticSpan {
            id: DiagnosticSpanId(id),
            line: diagnostic.line,
            start: diagnostic.start,
            end: diagnostic.end,
            severity: format!("{:?}", diagnostic.severity).to_ascii_lowercase(),
            message: diagnostic.message.clone(),
        })
        .collect::<Vec<_>>();
    let symbols = semantic_symbols(
        checked_program,
        &source_units,
        &output_roots,
        &sources,
        &lists,
        &row_scopes,
        &functions,
        &semantic_fields,
        &view_bindings,
    );
    let readiness = semantic_index_readiness(
        checked_program,
        &sources,
        &row_scopes,
        &lists,
        typecheck_report,
    );
    SemanticIndex {
        version: 1,
        computed_from: "checked_program_erased_ir".to_owned(),
        parser_policy_phase: "checked_semantics_only".to_owned(),
        reuse_key: semantic_index_reuse_key(
            &source_units,
            &sources,
            &lists,
            &row_scopes,
            &readiness,
        ),
        output_roots,
        source_units,
        sources,
        lists,
        row_scopes,
        functions,
        fields: semantic_fields,
        view_bindings,
        diagnostic_spans,
        symbols,
        readiness,
        reuse: SemanticIndexReuse {
            parser_reused_by_ir: false,
            typecheck_reused_by_ir: true,
            runtime_reports_reuse_index: true,
            shared_tables: vec![
                "CheckedProgram.sources".to_owned(),
                "CheckedProgram.lowering_metadata.source_payload_shape_table".to_owned(),
                "CheckedProgram.lowering_metadata.render_slot_table".to_owned(),
                "ErasedProgram.semantic_index.output_roots".to_owned(),
                "ErasedProgram.view_bindings".to_owned(),
            ],
        },
    }
}

fn semantic_field_entries(
    fields: &[ErasedFieldDef],
    derived_values: &[DerivedValue],
    state_cells: &[StateCell],
) -> Vec<SemanticFieldEntry> {
    fields
        .iter()
        .filter(|field| field.role.is_value())
        .map(|field| SemanticFieldEntry {
            id: field.id,
            path: field.diagnostic_path.clone(),
            local_name: field.name.clone(),
            parent_path: field
                .parent
                .and_then(|parent| fields.get(parent.as_usize()))
                .map_or_else(String::new, |parent| parent.diagnostic_path.clone()),
            scope_id: field.row.map(|row| row.scope),
            statement_id: field.statement.map_or(usize::MAX, |statement| statement.0),
            line: field
                .statement
                .and_then(|statement| {
                    state_cells
                        .iter()
                        .find(|state| state.statement_id == statement.as_usize())
                })
                .map_or(0, |state| state.source_line),
            kind: if state_cells.iter().any(|state| {
                field
                    .statement
                    .is_some_and(|statement| state.statement_id == statement.as_usize())
            }) {
                "state".to_owned()
            } else if let Some(derived) =
                derived_values.iter().find(|derived| derived.id == field.id)
            {
                match derived.kind {
                    DerivedValueKind::SourceEventTransform => "source_event_transform",
                    DerivedValueKind::ListView => "list_view",
                    DerivedValueKind::Aggregate => "aggregate",
                    DerivedValueKind::Pure => "pure",
                    DerivedValueKind::Unknown => "unknown",
                }
                .to_owned()
            } else if field.row.is_some() {
                "materialized_field".to_owned()
            } else {
                "field".to_owned()
            },
        })
        .collect()
}

fn output_root_values(
    checked_program: &boon_typecheck::CheckedProgram,
    typecheck_report: &boon_typecheck::CheckedProgramLoweringMetadata,
    executable: &ExecutableProgram,
    scope_index: &ErasedScopeIndex,
) -> Result<Vec<OutputRootValue>, String> {
    output_root_declarations(checked_program, typecheck_report)
        .into_iter()
        .map(|declaration| {
            let executable_statement_id = ExecutableStatementId(declaration.statement_id);
            let executable_statement = executable
                .statements
                .iter()
                .find(|statement| statement.id == executable_statement_id)
                .ok_or_else(|| {
                    format!(
                        "output root `{}` has no exact executable statement {}",
                        declaration.value_path, executable_statement_id
                    )
                })?;
            let declaration_id = executable_statement.declaration.ok_or_else(|| {
                format!(
                    "output root `{}` executable statement {} has no checked declaration",
                    declaration.value_path, executable_statement_id
                )
            })?;
            let value_expression_id = executable_statement.value.ok_or_else(|| {
                format!(
                    "output root `{}` executable statement {} has no value",
                    declaration.value_path, executable_statement_id
                )
            })?;
            let bindings = scope_index
                .bindings
                .iter()
                .filter(|binding| {
                    binding.declaration == declaration_id
                        && binding.producer == value_expression_id
                        && matches!(binding.target, ErasedBindingTarget::Value { .. })
                })
                .collect::<Vec<_>>();
            let [binding] = bindings.as_slice() else {
                return Err(format!(
                    "output root `{}` declaration {} producer {} has {} exact value storage bindings",
                    declaration.value_path,
                    declaration_id.0,
                    value_expression_id,
                    bindings.len()
                ));
            };
            let typed_contract_known = match declaration.contract {
                SemanticOutputContractKind::RetainedVisual {
                    kind: SemanticRetainedVisualKind::Document,
                } => executable_expression_contains_call_prefix(
                    executable,
                    value_expression_id,
                    &["Document/", "Element/"],
                ),
                SemanticOutputContractKind::RetainedVisual {
                    kind: SemanticRetainedVisualKind::Scene,
                } => executable_expression_contains_call_prefix(
                    executable,
                    value_expression_id,
                    &["Scene/"],
                ),
                SemanticOutputContractKind::HostValue => declaration
                    .data_type
                    .as_ref()
                    .is_some_and(semantic_data_type_is_closed),
            };
            Ok(OutputRootValue {
                root: declaration.root,
                value_path: declaration.value_path,
                contract: declaration.contract,
                demand: SemanticOutputDemandPolicy::HostDemanded,
                data_type: declaration.data_type,
                statement_id: declaration.statement_id,
                executable_statement_id,
                value_expression_id,
                binding_id: binding.id,
                line: declaration.line,
                typed_contract_known,
            })
        })
        .collect()
}

struct OutputRootDeclaration {
    root: String,
    value_path: String,
    contract: SemanticOutputContractKind,
    data_type: Option<SemanticDataType>,
    statement_id: usize,
    line: usize,
}

fn output_root_declarations(
    program: &boon_typecheck::CheckedProgram,
    typecheck_report: &boon_typecheck::CheckedProgramLoweringMetadata,
) -> Vec<OutputRootDeclaration> {
    let mut declarations = Vec::new();
    for statement in program
        .statements
        .iter()
        .filter(|statement| statement.scope_id == program.root_scope)
    {
        let boon_typecheck::CheckedStatementKind::Field { declaration } = statement.kind else {
            continue;
        };
        let Some(name) = program
            .declarations
            .iter()
            .find(|candidate| candidate.id == declaration)
            .map(|declaration| declaration.name.as_str())
        else {
            continue;
        };
        let visual_kind = match name {
            "document" => Some(SemanticRetainedVisualKind::Document),
            "scene" => Some(SemanticRetainedVisualKind::Scene),
            _ => None,
        };
        if let Some(kind) = visual_kind {
            declarations.push(OutputRootDeclaration {
                root: name.to_owned(),
                value_path: name.to_owned(),
                contract: SemanticOutputContractKind::RetainedVisual { kind },
                data_type: None,
                statement_id: statement.id.0 as usize,
                line: statement.span.line,
            });
            continue;
        }
        if name != "outputs" {
            continue;
        }
        for output_id in &statement.children {
            let Some(output) = program
                .statements
                .get(output_id.0 as usize)
                .filter(|output| output.id == *output_id)
            else {
                continue;
            };
            let output_declaration = match output.kind {
                boon_typecheck::CheckedStatementKind::Field { declaration } => Some(declaration),
                boon_typecheck::CheckedStatementKind::List { declaration, .. } => declaration,
                _ => None,
            };
            let Some(name) = output_declaration.and_then(|declaration| {
                program
                    .declarations
                    .iter()
                    .find(|candidate| candidate.id == declaration)
                    .map(|declaration| declaration.name.as_str())
            }) else {
                continue;
            };
            let data_type = typecheck_report
                .output_root_types
                .iter()
                .find(|entry| entry.statement_id == output.id.0 as usize && entry.name == name)
                .map(|entry| semantic_data_type(&entry.ty));
            declarations.push(OutputRootDeclaration {
                root: name.to_owned(),
                value_path: format!("outputs.{name}"),
                contract: SemanticOutputContractKind::HostValue,
                data_type,
                statement_id: output.id.0 as usize,
                line: output.span.line,
            });
        }
    }
    declarations.sort_by(|left, right| left.root.cmp(&right.root));
    declarations
}

fn semantic_data_type_is_closed(data_type: &SemanticDataType) -> bool {
    match data_type {
        SemanticDataType::Unknown { .. } => false,
        SemanticDataType::Record { fields, open } => {
            !open
                && fields
                    .iter()
                    .all(|field| semantic_data_type_is_closed(&field.data_type))
        }
        SemanticDataType::Variant { variants } => variants.iter().all(|variant| {
            !variant.open
                && variant
                    .fields
                    .iter()
                    .all(|field| semantic_data_type_is_closed(&field.data_type))
        }),
        SemanticDataType::List { item } => semantic_data_type_is_closed(item),
        SemanticDataType::Null
        | SemanticDataType::Bool
        | SemanticDataType::Number
        | SemanticDataType::Text
        | SemanticDataType::Bytes { .. } => true,
    }
}

fn executable_expression_contains_call_prefix(
    program: &ExecutableProgram,
    root: ExecutableExprId,
    prefixes: &[&str],
) -> bool {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        let Some(expression) = program
            .expressions
            .get(expression_id.as_usize())
            .filter(|expression| expression.id == expression_id)
        else {
            continue;
        };
        if matches!(
            &expression.kind,
            ExecutableExpressionKind::Call { name, .. }
                if prefixes.iter().any(|prefix| name.starts_with(prefix))
        ) {
            return true;
        }
        pending.extend(executable_expression_children(&expression.kind));
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn semantic_symbols(
    program: &boon_typecheck::CheckedProgram,
    source_units: &[SemanticSourceUnit],
    output_roots: &[SemanticOutputRootEntry],
    sources: &[SemanticSourceEntry],
    lists: &[SemanticListEntry],
    row_scopes: &[SemanticRowScopeEntry],
    functions: &[SemanticFunctionEntry],
    fields: &[SemanticFieldEntry],
    view_bindings: &[SemanticViewBindingEntry],
) -> Vec<SemanticSymbolEntry> {
    let mut table = SemanticSymbolTable::default();
    for file in source_units {
        table.intern("source_unit_path", &file.path);
        if let Some(module) = &file.module {
            table.intern("module_path", module);
        }
    }
    for root in output_roots {
        table.intern("output_root", &root.root);
        table.intern("output_kind", root.contract.as_str());
    }
    for source in sources {
        table.intern("source_label", &source.path);
        for part in source.path.split('.') {
            table.intern("source_label_segment", part);
        }
    }
    for list in lists {
        table.intern("list_name", &list.name);
    }
    for scope in row_scopes {
        table.intern("row_scope", &scope.row_scope);
        table.intern("row_scope_function", &scope.function);
    }
    for function in functions {
        table.intern("function_name", &function.name);
        for arg in &function.args {
            table.intern("function_arg", arg);
        }
    }
    for field in fields {
        table.intern("field_path", &field.path);
        table.intern("field_name", &field.local_name);
    }
    for expr in &program.expressions {
        match &expr.kind {
            boon_typecheck::CheckedExpressionKind::Tag { name } => {
                table.intern("tag", name);
            }
            boon_typecheck::CheckedExpressionKind::TaggedObject { tag, fields } => {
                table.intern("tag", tag);
                for field in fields {
                    table.intern("document_attr", &field.name);
                }
            }
            boon_typecheck::CheckedExpressionKind::Object { fields }
            | boon_typecheck::CheckedExpressionKind::Record { fields } => {
                for field in fields {
                    table.intern("document_attr", &field.name);
                    table.intern("style_attr", &field.name);
                }
            }
            _ => {}
        }
    }
    for call in &program.calls {
        table.intern("operator_name", &call.function);
        for entry in &call.entries {
            let name = match entry {
                boon_typecheck::CheckedCallEntry::Input { name, .. }
                | boon_typecheck::CheckedCallEntry::FreshOut { name, .. }
                | boon_typecheck::CheckedCallEntry::ForwardOut { name, .. } => name,
            };
            table.intern("document_attr", name);
        }
    }
    for binding in view_bindings {
        table.intern("document_attr", &binding.attr);
        table.intern("view_node_kind", &binding.node_kind);
    }
    table.into_entries()
}

#[derive(Default)]
struct SemanticSymbolTable {
    ids_by_category: BTreeMap<String, BTreeMap<String, SemanticSymbolId>>,
    entries: Vec<SemanticSymbolEntry>,
}

impl SemanticSymbolTable {
    fn intern(&mut self, category: &str, text: &str) -> SemanticSymbolId {
        if let Some(id) = self
            .ids_by_category
            .get(category)
            .and_then(|symbols| symbols.get(text))
            .copied()
        {
            return id;
        }
        let id = SemanticSymbolId(self.entries.len());
        if let Some(symbols) = self.ids_by_category.get_mut(category) {
            symbols.insert(text.to_owned(), id);
        } else {
            self.ids_by_category
                .insert(category.to_owned(), BTreeMap::from([(text.to_owned(), id)]));
        }
        self.entries.push(SemanticSymbolEntry {
            id,
            category: category.to_owned(),
            text: text.to_owned(),
        });
        id
    }

    fn into_entries(self) -> Vec<SemanticSymbolEntry> {
        self.entries
    }
}

fn semantic_index_readiness(
    program: &boon_typecheck::CheckedProgram,
    sources: &[SemanticSourceEntry],
    row_scopes: &[SemanticRowScopeEntry],
    lists: &[SemanticListEntry],
    typecheck_report: &boon_typecheck::CheckedProgramLoweringMetadata,
) -> SemanticIndexReadiness {
    let source_payload_fallbacks = sources
        .iter()
        .filter(|source| !source.payload_schema_known)
        .map(|source| format!("{} has no source payload shape entry", source.path))
        .collect::<Vec<_>>();
    let row_scope_fallbacks = if !lists.is_empty() && row_scopes.is_empty() {
        vec!["lists exist but no row scope function was discovered".to_owned()]
    } else {
        Vec::new()
    };
    let selector_fallbacks = selector_fallback_reasons(program);
    let render_fallbacks = typecheck_report
        .render_slot_table
        .slots
        .iter()
        .filter(|slot| !slot.diagnostics.is_empty())
        .map(|slot| {
            format!(
                "render slot `{}` at statement {} has {} diagnostic(s)",
                slot.slot_name,
                slot.slot_statement_id,
                slot.diagnostics.len()
            )
        })
        .collect::<Vec<_>>();
    let route_critical_fallbacks = route_critical_unknown_reasons(typecheck_report);
    let row_scope_ambiguity_fallbacks = row_scope_ambiguity_reasons(row_scopes);
    SemanticIndexReadiness {
        source_payload_schemas: SemanticKnowledgeStatus {
            known_count: sources.len().saturating_sub(source_payload_fallbacks.len()),
            fallback_count: source_payload_fallbacks.len(),
            fallback_reasons: source_payload_fallbacks,
        },
        source_completions: SemanticKnowledgeStatus {
            known_count: sources.len(),
            fallback_count: 0,
            fallback_reasons: Vec::new(),
        },
        route_critical_unknowns: SemanticKnowledgeStatus {
            known_count: typecheck_report.checked_expression_count,
            fallback_count: route_critical_fallbacks.len(),
            fallback_reasons: route_critical_fallbacks,
        },
        row_scopes: SemanticKnowledgeStatus {
            known_count: row_scopes.len(),
            fallback_count: row_scope_fallbacks.len(),
            fallback_reasons: row_scope_fallbacks,
        },
        row_scope_ambiguity: SemanticKnowledgeStatus {
            known_count: row_scopes.len(),
            fallback_count: row_scope_ambiguity_fallbacks.len(),
            fallback_reasons: row_scope_ambiguity_fallbacks,
        },
        selectors: SemanticKnowledgeStatus {
            known_count: lists.len(),
            fallback_count: selector_fallbacks.len(),
            fallback_reasons: selector_fallbacks.clone(),
        },
        selector_index_ambiguity: SemanticKnowledgeStatus {
            known_count: lists.len(),
            fallback_count: selector_fallbacks.len(),
            fallback_reasons: selector_fallbacks.clone(),
        },
        render_contracts: SemanticKnowledgeStatus {
            known_count: typecheck_report
                .render_slot_table
                .slots
                .len()
                .saturating_sub(render_fallbacks.len()),
            fallback_count: render_fallbacks.len(),
            fallback_reasons: render_fallbacks,
        },
        bridge_page_descriptors: SemanticKnowledgeStatus {
            known_count: 0,
            fallback_count: 0,
            fallback_reasons: Vec::new(),
        },
        dynamic_fallback_count: typecheck_report.dynamic_fallback_count,
    }
}

fn route_critical_unknown_reasons(
    typecheck_report: &boon_typecheck::CheckedProgramLoweringMetadata,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if typecheck_report.dynamic_fallback_count > 0 {
        reasons.push(format!(
            "typecheck dynamic_fallback_count={} in route-critical report; inspect expr_type_table and diagnostics for expression spans",
            typecheck_report.dynamic_fallback_count
        ));
    }
    for slot in &typecheck_report.render_slot_table.slots {
        if !slot.diagnostics.is_empty() {
            reasons.push(format!(
                "render slot `{}` statement={} value_expr={:?} has {} fallback diagnostic(s)",
                slot.slot_name,
                slot.slot_statement_id,
                slot.value_expr_id,
                slot.diagnostics.len()
            ));
        }
    }
    reasons
}

fn row_scope_ambiguity_reasons(row_scopes: &[SemanticRowScopeEntry]) -> Vec<String> {
    let mut seen = BTreeMap::<&str, &str>::new();
    let mut reasons = Vec::new();
    for scope in row_scopes {
        if let Some(existing_list) = seen.insert(scope.row_scope.as_str(), scope.list.as_str())
            && existing_list != scope.list
        {
            reasons.push(format!(
                "row scope `{}` is shared by lists `{}` and `{}`",
                scope.row_scope, existing_list, scope.list
            ));
        }
    }
    reasons
}

fn selector_fallback_reasons(program: &boon_typecheck::CheckedProgram) -> Vec<String> {
    program
        .expressions
        .iter()
        .filter_map(|expr| match &expr.kind {
            boon_typecheck::CheckedExpressionKind::Invalid { tokens }
                if tokens.iter().any(|token| token.contains("List/")) =>
            {
                Some(format!(
                    "list selector expression at line {} is invalid after typecheck",
                    expr.span.line
                ))
            }
            _ => None,
        })
        .collect()
}

fn semantic_index_reuse_key(
    source_units: &[SemanticSourceUnit],
    sources: &[SemanticSourceEntry],
    lists: &[SemanticListEntry],
    row_scopes: &[SemanticRowScopeEntry],
    readiness: &SemanticIndexReadiness,
) -> String {
    let source_identity = source_units
        .iter()
        .map(|source| source.path.as_str())
        .collect::<Vec<_>>()
        .join("|");
    format!(
        "semantic-index-v1:{}:{}:{}:{}:{}:{}",
        source_identity,
        source_units.len(),
        sources.len(),
        lists.len(),
        row_scopes.len(),
        readiness.dynamic_fallback_count
    )
}

fn lower_elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

pub fn verify_hidden_identity(program: &ErasedProgram) -> Result<(), String> {
    if !program.hidden_identity_verified {
        return Err("hidden identity verification did not run".to_owned());
    }
    if program.lists.iter().any(|list| !list.has_generation) {
        return Err("all list memories must carry generation guards".to_owned());
    }
    verify_identity_clean_identifiers(program)?;
    Ok(())
}

pub fn verify_static_schedule(program: &ErasedProgram) -> Result<(), String> {
    if !program.static_schedule_verified {
        return Err("static schedule verification did not run".to_owned());
    }
    if program.graph_node_count != program.executable.expressions.len() {
        return Err(format!(
            "graph_node_count {} does not match {} canonical executable expressions",
            program.graph_node_count,
            program.executable.expressions.len()
        ));
    }
    verify_distributed_reference_schedule(program)?;
    verify_executable_schedule(program)?;

    let source_paths = unique_strings(
        "source port",
        program.sources.iter().map(|source| source.path.as_str()),
    )?;
    for (index, source) in program.sources.iter().enumerate() {
        if source.id.as_usize() != index {
            return Err(format!(
                "source port `{}` has SourceId {}, expected {index}",
                source.path, source.id
            ));
        }
        if source.scoped && source.scope_id.is_none() {
            return Err(format!(
                "scoped source port `{}` has no typed ScopeId",
                source.path
            ));
        }
    }
    let state_paths = unique_strings(
        "state cell",
        program.state_cells.iter().map(|cell| cell.path.as_str()),
    )?;
    for (index, cell) in program.state_cells.iter().enumerate() {
        if cell.id.as_usize() != index {
            return Err(format!(
                "state cell `{}` has StateId {}, expected {index}",
                cell.path, cell.id
            ));
        }
    }
    let list_names = unique_strings("list", program.lists.iter().map(|list| list.name.as_str()))?;
    let row_scope_names = unique_strings(
        "row scope",
        program
            .row_scopes
            .iter()
            .map(|scope| scope.row_scope.as_str()),
    )?;
    for (index, scope) in program.row_scopes.iter().enumerate() {
        if scope.id.as_usize() != index {
            return Err(format!(
                "row scope `{}` has ScopeId {}, expected {index}",
                scope.row_scope, scope.id
            ));
        }
        if !list_names.contains(scope.list.as_str()) {
            return Err(format!(
                "row scope `{}` references unknown list `{}`",
                scope.row_scope, scope.list
            ));
        }
        if scope.function.trim().is_empty() {
            return Err(format!(
                "row scope `{}` has empty function",
                scope.row_scope
            ));
        }
    }
    for (index, list) in program.lists.iter().enumerate() {
        if list.id.as_usize() != index {
            return Err(format!(
                "list memory `{}` has ListId {}, expected {index}",
                list.name, list.id
            ));
        }
        if list.row_scope_id.is_some_and(|scope_id| {
            scope_id.as_usize() >= program.row_scopes.len()
                || program.row_scopes[scope_id.as_usize()].list != list.name
        }) {
            return Err(format!(
                "list memory `{}` has invalid row ScopeId {:?}",
                list.name, list.row_scope_id
            ));
        }
    }
    let derived_paths = unique_strings(
        "derived value",
        program
            .derived_values
            .iter()
            .map(|value| value.path.as_str()),
    )?;
    let mut derived_field_ids = BTreeSet::new();
    for value in &program.derived_values {
        if !derived_field_ids.insert(value.id) {
            return Err(format!(
                "derived value `{}` reuses FieldId {}",
                value.path, value.id
            ));
        }
        if !program
            .scope_index
            .fields
            .iter()
            .any(|field| field.id == value.id)
        {
            return Err(format!(
                "derived value `{}` references missing FieldId {}",
                value.path, value.id
            ));
        }
        if value.kind == DerivedValueKind::ListView {
            let list_id = value.materialized_list_id.ok_or_else(|| {
                format!(
                    "typed list view `{}` has no materialized ListId",
                    value.path
                )
            })?;
            let row_scope_id = value.materialized_row_scope_id.ok_or_else(|| {
                format!(
                    "typed list view `{}` has no materialized row ScopeId",
                    value.path
                )
            })?;
            let list = program.lists.get(list_id.as_usize()).ok_or_else(|| {
                format!(
                    "typed list view `{}` references missing materialized ListId {}",
                    value.path, list_id
                )
            })?;
            if list.id != list_id
                || list.name != value.path
                || list.row_scope_id != Some(row_scope_id)
            {
                return Err(format!(
                    "typed list view `{}` storage metadata does not match ListId {} and ScopeId {}",
                    value.path, list_id, row_scope_id
                ));
            }
            let row_scope = program
                .row_scopes
                .get(row_scope_id.as_usize())
                .ok_or_else(|| {
                    format!(
                        "typed list view `{}` references missing materialized row ScopeId {}",
                        value.path, row_scope_id
                    )
                })?;
            if row_scope.id != row_scope_id || row_scope.list != list.name {
                return Err(format!(
                    "typed list view `{}` row ScopeId {} does not own ListId {}",
                    value.path, row_scope_id, list_id
                ));
            }
        }
    }
    for target in typed_derived_list_targets(&program.executable)? {
        let binding = program
            .scope_index
            .bindings
            .iter()
            .find(|binding| {
                binding.declaration == target.declaration && binding.producer == target.producer
            })
            .ok_or_else(|| {
                format!(
                    "typed list declaration `{}` has no exact storage binding",
                    target.path
                )
            })?;
        let ErasedBindingTarget::Value {
            row:
                Some(ErasedRowBinding {
                    list,
                    scope: row_scope,
                }),
            ..
        } = binding.target
        else {
            return Err(format!(
                "typed list declaration `{}` did not receive keyed ListId/ScopeId storage",
                target.path
            ));
        };
        let statement = program
            .executable
            .statements
            .iter()
            .find(|statement| statement.id == target.statement)
            .ok_or_else(|| {
                format!(
                    "typed list declaration `{}` references missing statement {}",
                    target.path, target.statement
                )
            })?;
        if matches!(statement.kind, ExecutableStatementKind::Field { .. }) {
            let value = program
                .derived_values
                .iter()
                .find(|value| value.executable_statement_id == target.statement);
            let Some(value) = value else {
                let producer = program
                    .executable
                    .expressions
                    .get(target.producer.as_usize())
                    .filter(|expression| expression.id == target.producer)
                    .ok_or_else(|| {
                        format!(
                            "typed list field `{}` references missing producer {}",
                            target.path, target.producer
                        )
                    })?;
                if matches!(producer.kind, ExecutableExpressionKind::List { .. }) {
                    continue;
                }
                return Err(format!(
                    "computed typed list field `{}` (statement {}, declaration {}, producer {}) has no derived storage value; available {:?}",
                    target.path,
                    target.statement,
                    target.declaration.0,
                    target.producer,
                    program
                        .derived_values
                        .iter()
                        .map(|value| (&value.path, value.executable_statement_id))
                        .collect::<Vec<_>>()
                ));
            };
            if value.kind != DerivedValueKind::ListView
                || value.materialized_list_id != Some(list)
                || value.materialized_row_scope_id != Some(row_scope)
            {
                return Err(format!(
                    "typed derived list field `{}` does not target its exact keyed storage",
                    target.path
                ));
            }
        }
    }
    for (index, binding) in program.view_bindings.iter().enumerate() {
        if binding.id.as_usize() != index {
            return Err(format!(
                "view binding `{}.{}` has ViewBindingId {}, expected {index}",
                binding.node_kind, binding.attr, binding.id
            ));
        }
        if let Some(scope_id) = binding.scope_id
            && scope_id.as_usize() >= program.row_scopes.len()
        {
            return Err(format!(
                "view binding `{}.{}` references missing ScopeId {}",
                binding.node_kind,
                binding.attr,
                scope_id.as_usize()
            ));
        }
        match &binding.target {
            ViewBindingTarget::Read { read, .. } => {
                if !program
                    .scope_index
                    .reads
                    .get(read.as_usize())
                    .is_some_and(|candidate| candidate.id == *read)
                {
                    return Err(format!(
                        "view binding `{}.{}` references missing erased read {read}",
                        binding.node_kind, binding.attr
                    ));
                }
            }
            ViewBindingTarget::Source { source } => {
                if !program
                    .sources
                    .get(source.as_usize())
                    .is_some_and(|candidate| candidate.id == *source)
                {
                    return Err(format!(
                        "view binding `{}.{}` references missing source {source}",
                        binding.node_kind, binding.attr
                    ));
                }
            }
        }
        match binding.kind {
            ViewBindingKind::Source => {
                let ViewBindingTarget::Source { source: source_id } = binding.target else {
                    return Err(format!(
                        "view source binding `{}.{}` has no exact source target",
                        binding.node_kind, binding.attr
                    ));
                };
                if source_id.as_usize() >= program.sources.len()
                    || program.sources[source_id.as_usize()].path != binding.path
                {
                    return Err(format!(
                        "view source binding `{}.{}` does not match source {source_id}",
                        binding.node_kind, binding.attr
                    ));
                }
            }
            ViewBindingKind::Data | ViewBindingKind::Target => {
                if matches!(binding.target, ViewBindingTarget::Source { .. }) {
                    return Err(format!(
                        "view data binding `{}.{}` unexpectedly targets a source",
                        binding.node_kind, binding.attr
                    ));
                }
            }
        }
    }
    verify_scope_refs(
        "source",
        program.sources.iter().filter_map(|source| source.scope_id),
        program,
    )?;
    verify_scope_refs(
        "state cell",
        program.state_cells.iter().filter_map(|cell| cell.scope_id),
        program,
    )?;
    verify_scope_refs(
        "derived value",
        program
            .derived_values
            .iter()
            .filter_map(|value| value.scope_id),
        program,
    )?;
    for cell in &program.state_cells {
        if cell.indexed
            && cell.scope_id.is_none()
            && row_scope_names
                .iter()
                .any(|scope| cell.path.split('.').any(|segment| segment == *scope))
        {
            return Err(format!(
                "indexed state cell `{}` did not resolve to a typed ScopeId",
                cell.path
            ));
        }
    }
    let store_list_names = program
        .lists
        .iter()
        .map(|list| format!("store.{}", list.name))
        .collect::<Vec<_>>();
    let source_payload_paths = program
        .sources
        .iter()
        .flat_map(|source| {
            source.payload_schema.fields.iter().flat_map(move |field| {
                let field = field.name();
                [
                    format!("{}.{}", source.path, field),
                    source
                        .path
                        .strip_prefix("store.")
                        .map(|path| format!("{path}.{field}"))
                        .unwrap_or_else(|| format!("{}.{}", source.path, field)),
                ]
            })
        })
        .collect::<Vec<_>>();
    let materialization_local_symbols = program
        .executable
        .expressions
        .iter()
        .filter_map(|expression| match &expression.kind {
            ExecutableExpressionKind::MaterializationLocal { projection, .. }
                if !projection.is_empty() =>
            {
                Some(format!("@local.{}", projection.join(".")))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let known_symbols = source_paths
        .iter()
        .chain(state_paths.iter())
        .chain(list_names.iter())
        .chain(derived_paths.iter())
        .copied()
        .chain(store_list_names.iter().map(String::as_str))
        .chain(source_payload_paths.iter().map(String::as_str))
        .chain(materialization_local_symbols.iter().map(String::as_str))
        .chain(
            program
                .semantic_index
                .fields
                .iter()
                .map(|field| field.path.as_str()),
        )
        .chain(
            program
                .distributed_references
                .value_references
                .iter()
                .map(|reference| reference.canonical_path.as_str()),
        )
        .collect::<BTreeSet<_>>();
    let list_projection_symbols = list_projection_view_symbols(program);
    for binding in &program.view_bindings {
        if !matches!(binding.kind, ViewBindingKind::Source)
            && binding.scope_id.is_none()
            && !symbol_known(&binding.path, &known_symbols)
            && !list_projection_symbols.contains(binding.path.as_str())
            && !view_projection_symbol_known(&binding.path)
        {
            require_known_symbol("view binding path", &binding.path, &known_symbols)?;
        }
    }

    for edge in &program.dependencies {
        require_known_symbol("dependency source", &edge.from, &known_symbols)?;
        require_known_symbol("dependency target", &edge.to, &known_symbols)?;
    }
    for cause in &program.possible_causes {
        require_known_symbol("cause target", &cause.target, &known_symbols)?;
        for source in &cause.sources {
            require_known_symbol("cause source", source, &known_symbols)?;
        }
    }
    for arm in &program.state_update_arms {
        let state = program
            .state_cells
            .get(arm.state.as_usize())
            .filter(|state| state.id == arm.state)
            .ok_or_else(|| {
                format!(
                    "state update arm references missing target StateId {}",
                    arm.state
                )
            })?;
        let cause = event_cause_path_owned(arm.cause, &program.sources, &program.state_cells)?;
        let gate = program
            .executable
            .expressions
            .get(arm.gate_expression_id.as_usize())
            .filter(|expression| {
                expression.id == arm.gate_expression_id
                    && expression.checked_expr_id == arm.gate_checked_expr_id
                    && expression.owner == arm.owner
            })
            .ok_or_else(|| {
                format!(
                    "state update `{}` from `{cause}` has stale gate {}",
                    state.path, arm.gate_expression_id
                )
            })?;
        program
            .executable
            .expressions
            .get(arm.output_expression_id.as_usize())
            .filter(|expression| expression.id == arm.output_expression_id)
            .ok_or_else(|| {
                format!(
                    "state update `{}` from `{cause}` gate {} has missing output {}",
                    state.path, gate.id, arm.output_expression_id
                )
            })?;
    }
    for mutation in &program.list_mutations {
        let Some(list) = program.lists.get(mutation.list_id.as_usize()) else {
            return Err(format!(
                "list mutation references missing ListId {}",
                mutation.list_id
            ));
        };
        if list.id != mutation.list_id {
            return Err(format!(
                "list mutation ListId {} resolves to inconsistent list `{}`",
                mutation.list_id, list.name
            ));
        }
        let cause = event_cause_path_owned(mutation.cause, &program.sources, &program.state_cells)?;
        let verify_expression = |id: ExecutableExprId, role: &str| {
            program
                .executable
                .expressions
                .get(id.as_usize())
                .filter(|expression| expression.id == id)
                .ok_or_else(|| {
                    format!(
                        "list mutation {} from `{cause}` references missing {role} expression {id}",
                        mutation.list_id
                    )
                })
        };
        match mutation.kind {
            ListMutationKind::Append { gate, item } => {
                verify_expression(gate, "gate")?;
                verify_expression(item, "item")?;
            }
            ListMutationKind::Remove {
                gate,
                owner,
                row_local,
                predicate,
                ..
            } => {
                verify_expression(gate, "gate")?;
                verify_expression(predicate, "predicate")?;
                if !program.materializations.iter().any(|materialization| {
                    materialization.owner == owner
                        && materialization.row_local == row_local
                        && (materialization.source_list_id == Some(mutation.list_id)
                            || materialization.target_list_id == Some(mutation.list_id))
                }) {
                    return Err(format!(
                        "list mutation {} from `{cause}` has no exact contextual owner {} local {}; candidates={:?}",
                        mutation.list_id,
                        owner,
                        row_local.0,
                        program
                            .materializations
                            .iter()
                            .filter(|materialization| {
                                materialization.owner == owner
                                    && materialization.row_local == row_local
                            })
                            .map(|materialization| (
                                materialization.id,
                                materialization.operation,
                                materialization.source_list_id,
                                materialization.target_list_id,
                            ))
                            .collect::<Vec<_>>()
                    ));
                }
            }
        }
    }
    Ok(())
}

fn verify_executable_schedule(program: &ErasedProgram) -> Result<(), String> {
    let expressions = &program.executable.expressions;
    for (index, expression) in expressions.iter().enumerate() {
        if expression.id.as_usize() != index {
            return Err(format!(
                "executable expression {} has id {}, expected {index}",
                expression.id, expression.id
            ));
        }
        if expression
            .owner
            .is_some_and(|owner| owner.as_usize() >= program.scope_index.owners.len())
        {
            return Err(format!(
                "executable expression {} references missing static owner {:?}",
                expression.id, expression.owner
            ));
        }
        for child in executable_expression_children(&expression.kind) {
            if child.as_usize() >= index {
                return Err(format!(
                    "executable expression {} has non-topological child {}",
                    expression.id, child
                ));
            }
        }
        if let ExecutableExpressionKind::Materialize { materialization } = expression.kind
            && materialization >= program.materializations.len()
        {
            return Err(format!(
                "executable expression {} references missing materialization {}",
                expression.id, materialization
            ));
        }
        if let ExecutableExpressionKind::Call { arguments, .. } = &expression.kind {
            let mut previous = None;
            for argument in arguments {
                if previous.is_some_and(|ordinal| ordinal >= argument.ordinal) {
                    return Err(format!(
                        "executable call {} has unordered or duplicate formal ordinal {}",
                        expression.id, argument.ordinal
                    ));
                }
                previous = Some(argument.ordinal);
            }
        }
    }

    let statement_ids = program
        .executable
        .statements
        .iter()
        .map(|statement| statement.id)
        .collect::<BTreeSet<_>>();
    for statement in &program.executable.statements {
        if statement
            .value
            .is_some_and(|value| value.as_usize() >= expressions.len())
        {
            return Err(format!(
                "executable statement {} references missing expression {:?}",
                statement.id, statement.value
            ));
        }
        if let Some(child) = statement
            .children
            .iter()
            .find(|child| !statement_ids.contains(child))
        {
            return Err(format!(
                "executable statement {} references missing child {}",
                statement.id, child
            ));
        }
    }

    let local_by_owner = program
        .materializations
        .iter()
        .map(|materialization| (materialization.owner, materialization.row_local))
        .collect::<BTreeMap<_, _>>();
    for (index, materialization) in program.materializations.iter().enumerate() {
        if materialization.id != index {
            return Err(format!(
                "contextual materialization {} is stored at index {index}",
                materialization.id
            ));
        }
        if materialization.owner.as_usize() >= program.scope_index.owners.len() {
            return Err(format!(
                "contextual materialization {} references missing static owner {}",
                materialization.id, materialization.owner
            ));
        }
        for (label, root) in [
            ("source", Some(materialization.source)),
            ("body", Some(materialization.body)),
            ("direction", materialization.direction),
        ]
        .into_iter()
        .filter_map(|(label, root)| root.map(|root| (label, root)))
        {
            if root.as_usize() >= expressions.len() {
                return Err(format!(
                    "contextual materialization {} {label} references missing expression {}",
                    materialization.id, root
                ));
            }
        }
        for (key_index, key) in materialization.inherited_order.iter().enumerate() {
            if !matches!(
                key.operation,
                ContextualOperationKind::SortBy | ContextualOperationKind::ThenBy
            ) {
                return Err(format!(
                    "contextual materialization {} inherited order key {key_index} has non-order operation {:?}",
                    materialization.id, key.operation
                ));
            }
            if key_index == 0 && key.operation != ContextualOperationKind::SortBy {
                return Err(format!(
                    "contextual materialization {} inherited order chain does not start with List/sort_by",
                    materialization.id
                ));
            }
            for (label, root) in [("body", key.body), ("direction", key.direction)] {
                if root.as_usize() >= expressions.len() {
                    return Err(format!(
                        "contextual materialization {} inherited order key {key_index} {label} references missing expression {}",
                        materialization.id, root
                    ));
                }
            }
        }
        if !materialization.inherited_order.is_empty()
            && materialization.operation != ContextualOperationKind::ThenBy
        {
            return Err(format!(
                "contextual materialization {} carries inherited order keys for non-then_by operation {:?}",
                materialization.id, materialization.operation
            ));
        }
        let mut ancestor_locals = BTreeSet::new();
        let mut ancestor = program.scope_index.owners[materialization.owner.as_usize()].parent;
        while let Some(owner) = ancestor {
            if let Some(local) = local_by_owner.get(&owner).copied() {
                ancestor_locals.insert((owner, local));
            }
            ancestor = program.scope_index.owners[owner.as_usize()].parent;
        }
        verify_materialization_locals(
            expressions,
            materialization.source,
            &ancestor_locals,
            materialization.id,
        )?;
        if let Some(direction) = materialization.direction {
            verify_materialization_locals(
                expressions,
                direction,
                &ancestor_locals,
                materialization.id,
            )?;
        }
        for key in &materialization.inherited_order {
            verify_materialization_locals(
                expressions,
                key.direction,
                &ancestor_locals,
                materialization.id,
            )?;
        }
        let mut body_locals = ancestor_locals;
        body_locals.insert((materialization.owner, materialization.row_local));
        verify_materialization_locals(
            expressions,
            materialization.body,
            &body_locals,
            materialization.id,
        )?;
        for key in &materialization.inherited_order {
            verify_materialization_locals(expressions, key.body, &body_locals, materialization.id)?;
        }
    }
    verify_runtime_executable_types(program)?;
    Ok(())
}

fn verify_runtime_executable_types(program: &ErasedProgram) -> Result<(), String> {
    let mut pending = Vec::new();
    pending.extend(
        program
            .executable
            .statements
            .iter()
            .filter_map(|statement| statement.value),
    );
    pending.extend(program.executable.roots.iter().map(|root| root.expression));
    pending.extend(
        program
            .executable
            .sources
            .iter()
            .map(|source| source.expression),
    );
    pending.extend(
        program
            .executable
            .states
            .iter()
            .map(|state| state.expression),
    );
    pending.extend(
        program
            .materializations
            .iter()
            .flat_map(ContextualMaterialization::expression_roots),
    );
    pending.extend(
        program
            .state_update_arms
            .iter()
            .flat_map(|arm| [arm.gate_expression_id, arm.output_expression_id]),
    );
    pending.extend(program.derived_values.iter().flat_map(|derived| {
        derived
            .trigger_arms
            .iter()
            .flat_map(|arm| [arm.gate_expression_id, arm.output_expression_id])
            .chain(derived.default_roots.iter().copied())
    }));
    pending.extend(
        program
            .output_values
            .iter()
            .map(|output| output.value_expression_id),
    );
    pending.extend(program.view_bindings.iter().filter_map(|binding| {
        match &binding.target {
            ViewBindingTarget::Read { read, .. } => program
                .scope_index
                .reads
                .get(read.as_usize())
                .filter(|candidate| candidate.id == *read)
                .map(|read| read.expression),
            _ => None,
        }
    }));

    for materialization in &program.materializations {
        for (label, ty) in [
            ("item", &materialization.item_type),
            ("result", &materialization.result_type),
        ] {
            if runtime_type_contains_var(ty) {
                return Err(format!(
                    "contextual materialization {} has unresolved runtime {label} type {ty:?}",
                    materialization.id
                ));
            }
        }
    }

    let mut visited = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        let expression = program
            .executable
            .expressions
            .get(expression_id.as_usize())
            .ok_or_else(|| {
                format!(
                    "runtime type verification reaches missing executable expression {expression_id}"
                )
            })?;
        if runtime_type_contains_var(&expression.flow_type.ty) {
            return Err(format!(
                "runtime executable expression {expression_id} has unresolved type {:?}",
                expression.flow_type.ty
            ));
        }
        pending.extend(executable_expression_children(&expression.kind));
    }
    Ok(())
}

fn runtime_type_contains_var(ty: &boon_typecheck::Type) -> bool {
    match ty {
        boon_typecheck::Type::Var(_) => true,
        boon_typecheck::Type::List(item) => runtime_type_contains_var(item),
        boon_typecheck::Type::Function { args, result } => {
            args.iter().any(runtime_type_contains_var) || runtime_type_contains_var(&result.ty)
        }
        boon_typecheck::Type::Object(shape) => shape.fields.values().any(runtime_type_contains_var),
        boon_typecheck::Type::VariantSet(variants) => {
            variants.iter().any(|variant| match variant {
                boon_typecheck::Variant::Tag(_) => false,
                boon_typecheck::Variant::Tagged { fields, .. } => {
                    fields.fields.values().any(runtime_type_contains_var)
                }
            })
        }
        boon_typecheck::Type::Text
        | boon_typecheck::Type::Number
        | boon_typecheck::Type::Bytes(_)
        | boon_typecheck::Type::Skip
        | boon_typecheck::Type::RenderContract
        | boon_typecheck::Type::UnresolvedShape { .. }
        | boon_typecheck::Type::Unknown => false,
    }
}

fn verify_materialization_locals(
    expressions: &[ExecutableExpression],
    root: ExecutableExprId,
    allowed: &BTreeSet<(StaticOwnerId, MaterializationLocalId)>,
    materialization: usize,
) -> Result<(), String> {
    let mut stack = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression) = stack.pop() {
        if !visited.insert(expression) {
            continue;
        }
        let node = expressions.get(expression.as_usize()).ok_or_else(|| {
            format!(
                "contextual materialization {materialization} reaches missing expression {expression}"
            )
        })?;
        if let ExecutableExpressionKind::MaterializationLocal { owner, local, .. } = node.kind
            && !allowed.contains(&(owner, local))
        {
            return Err(format!(
                "contextual materialization {materialization} reads owner {} local {:?}, allowed {:?}",
                owner, local, allowed
            ));
        }
        stack.extend(executable_expression_children(&node.kind));
    }
    Ok(())
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
        | ExecutableExpressionKind::BytesByte(_)
        | ExecutableExpressionKind::Bool(_)
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
        | ExecutableExpressionKind::Object(fields)
        | ExecutableExpressionKind::Record(fields) => {
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
        ExecutableExpressionKind::Draining { input }
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
        ExecutableExpressionKind::MatchArm { output, .. } => output.iter().copied().collect(),
        ExecutableExpressionKind::List { items, .. }
        | ExecutableExpressionKind::Bytes { items, .. } => items.clone(),
    }
}

fn verify_distributed_reference_schedule(program: &ErasedProgram) -> Result<(), String> {
    let expected_count = program.distributed_references.value_references.len()
        + program.distributed_references.calls.len();
    if program
        .expression_coverage
        .distributed_reference_expression_count
        != expected_count
    {
        return Err(format!(
            "distributed expression coverage reports {}, expected {expected_count}",
            program
                .expression_coverage
                .distributed_reference_expression_count
        ));
    }

    let scheduled_expr_ids = program
        .executable
        .expressions
        .iter()
        .map(|expression| ExprId(expression.checked_expr_id.0 as usize))
        .collect::<BTreeSet<_>>();
    let mut reference_expr_ids = BTreeSet::new();
    for reference in &program.distributed_references.value_references {
        if !reference_expr_ids.insert(reference.expr_id) {
            return Err(format!(
                "distributed expression {} is represented more than once",
                reference.expr_id
            ));
        }
        require_scheduled_distributed_expr(reference.expr_id, &scheduled_expr_ids)?;
        if distributed_function_role(&reference.canonical_path) != Some(reference.producer_role) {
            return Err(format!(
                "distributed value `{}` does not match producer role {:?}",
                reference.canonical_path, reference.producer_role
            ));
        }
        verify_distributed_metadata_type(
            program,
            reference.expr_id,
            reference.flow_mode,
            &reference.value_type,
            &format!("distributed value `{}`", reference.canonical_path),
        )?;
    }

    let mut call_expressions = BTreeSet::new();
    for call in &program.distributed_references.calls {
        if !call_expressions.insert(call.expression) {
            return Err(format!(
                "distributed expression {} is represented more than once",
                call.expression
            ));
        }
        let expression = program
            .executable
            .expressions
            .get(call.expression.as_usize())
            .filter(|candidate| candidate.id == call.expression)
            .ok_or_else(|| {
                format!(
                    "distributed call `{}` references missing executable expression {}",
                    call.canonical_function, call.expression
                )
            })?;
        if expression.owner != call.owner {
            return Err(format!(
                "distributed call `{}` executable owner does not match its concrete metadata",
                call.canonical_function
            ));
        }
        if distributed_function_role(&call.canonical_function) != Some(call.producer_role) {
            return Err(format!(
                "distributed call `{}` does not match producer role {:?}",
                call.canonical_function, call.producer_role
            ));
        }
        let result_context = format!("distributed call `{}` result", call.canonical_function);
        ensure_distributed_value_flow_is_closed(&expression.flow_type, &result_context)?;
        ensure_distributed_value_flow_is_closed(&call.result, &result_context)?;
        if expression.flow_type != call.result {
            return Err(format!(
                "{result_context} executable type does not match its boundary type"
            ));
        }
        let mut names = BTreeSet::new();
        for argument in &call.arguments {
            if !names.insert(argument.name.as_str()) {
                return Err(format!(
                    "distributed call `{}` repeats argument `{}`",
                    call.canonical_function, argument.name
                ));
            }
            let context = format!(
                "distributed call `{}` argument `{}`",
                call.canonical_function, argument.name
            );
            ensure_distributed_value_flow_is_closed(&argument.flow_type, &context)?;
            let value = program
                .executable
                .expressions
                .get(argument.value.as_usize())
                .filter(|candidate| candidate.id == argument.value)
                .ok_or_else(|| {
                    format!(
                        "{context} references missing executable expression {}",
                        argument.value
                    )
                })?;
            ensure_distributed_value_flow_is_closed(&value.flow_type, &context)?;
            if value.flow_type != argument.flow_type {
                return Err(format!(
                    "{context} executable type does not match its boundary type"
                ));
            }
        }
    }
    Ok(())
}

fn distributed_expr_type(
    expression_types: &boon_typecheck::ExprTypeTable,
    expr_id: usize,
) -> Result<&boon_typecheck::FlowType, String> {
    expression_types
        .entries
        .iter()
        .find(|entry| entry.expr_id == expr_id)
        .map(|entry| &entry.flow_type)
        .ok_or_else(|| format!("distributed expression {expr_id} has no checked type"))
}

fn require_scheduled_distributed_expr(
    expr_id: ExprId,
    scheduled_expr_ids: &BTreeSet<ExprId>,
) -> Result<(), String> {
    if scheduled_expr_ids.contains(&expr_id) {
        Ok(())
    } else {
        Err(format!(
            "distributed expression {expr_id} is not in the static schedule"
        ))
    }
}

fn verify_distributed_metadata_type(
    program: &ErasedProgram,
    expr_id: ExprId,
    flow_mode: boon_typecheck::FlowMode,
    metadata_type: &boon_typecheck::Type,
    context: &str,
) -> Result<(), String> {
    ensure_distributed_type_is_closed(metadata_type, context)?;
    let checked = distributed_expr_type(&program.expression_types, expr_id.as_usize())?;
    if checked.mode != flow_mode {
        return Err(format!("{context} flow mode does not match its metadata"));
    }
    if &checked.ty != metadata_type {
        return Err(format!(
            "{context} metadata type does not match its checked expression type"
        ));
    }
    Ok(())
}

fn unique_strings<'a>(
    label: &str,
    values: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeSet<&'a str>, String> {
    let mut set = BTreeSet::new();
    for value in values {
        if value.trim().is_empty() {
            return Err(format!("{label} has empty path"));
        }
        if !set.insert(value) {
            return Err(format!("duplicate {label} `{value}`"));
        }
    }
    Ok(set)
}

fn verify_scope_refs(
    label: &str,
    refs: impl IntoIterator<Item = ScopeId>,
    program: &ErasedProgram,
) -> Result<(), String> {
    for scope_id in refs {
        if scope_id.as_usize() >= program.row_scopes.len() {
            return Err(format!(
                "{label} references missing ScopeId {}",
                scope_id.as_usize()
            ));
        }
    }
    Ok(())
}

fn scope_id_for_path(row_scopes: &[RowScope], path: &str) -> Option<ScopeId> {
    path.split('.').find_map(|segment| {
        row_scopes
            .iter()
            .find(|scope| scope.row_scope == segment)
            .map(|scope| scope.id)
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DerivedListStorageIds {
    path: String,
    list_id: ListId,
    row_scope_id: ScopeId,
    item_type: boon_typecheck::Type,
    item_fields: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TypedDerivedListTarget {
    statement: ExecutableStatementId,
    declaration: boon_typecheck::DeclId,
    producer: ExecutableExprId,
    path: String,
    local_name: String,
    capacity: Option<usize>,
    item_type: boon_typecheck::Type,
    item_fields: Vec<String>,
}

fn executable_statement_name_path(kind: &ExecutableStatementKind) -> Option<(&str, &str)> {
    match kind {
        ExecutableStatementKind::Field { name, path } => Some((name, path)),
        ExecutableStatementKind::List {
            name: Some(name),
            path: Some(path),
            ..
        } => Some((name, path)),
        _ => None,
    }
}

fn direct_list_alias_target(
    executable: &ExecutableProgram,
    statement: &ExecutableStatement,
) -> Option<boon_typecheck::DeclId> {
    let value = statement.value?;
    let expression = executable
        .expressions
        .get(value.as_usize())
        .filter(|expression| expression.id == value)?;
    if !matches!(&expression.flow_type.ty, boon_typecheck::Type::List(_)) {
        return None;
    }
    match &expression.kind {
        ExecutableExpressionKind::CanonicalRead {
            target, projection, ..
        } if projection.is_empty() => Some(*target),
        _ => None,
    }
}

fn typed_derived_list_targets(
    executable: &ExecutableProgram,
) -> Result<Vec<TypedDerivedListTarget>, String> {
    let mut targets = Vec::new();
    let mut seen = BTreeSet::new();
    let direct_storage_statements = direct_erased_storage_statements(executable);
    for statement in &executable.statements {
        if !direct_storage_statements.contains(&statement.id) {
            continue;
        }
        let Some((name, path)) = executable_statement_name_path(&statement.kind) else {
            continue;
        };
        let Some(value) = statement.value else {
            continue;
        };
        let expression = executable
            .expressions
            .get(value.as_usize())
            .ok_or_else(|| {
                format!("typed field `{path}` references missing executable expression {value}")
            })?;
        if direct_list_alias_target(executable, statement).is_some() {
            continue;
        }
        if expression.flow_type.mode == boon_typecheck::FlowMode::Absent {
            continue;
        }
        let boon_typecheck::Type::List(item_type) = &expression.flow_type.ty else {
            continue;
        };
        if !matches!(item_type.as_ref(), boon_typecheck::Type::Object(_)) {
            continue;
        }
        let Some(declaration) = statement.declaration else {
            return Err(format!(
                "typed list-valued statement {} has no checked declaration",
                statement.id
            ));
        };
        if !seen.insert(declaration) {
            return Err(format!(
                "checked declaration {} has more than one executable list storage target",
                declaration.0
            ));
        }
        let mut item_fields = typed_item_field_names(item_type);
        for field in executable_list_item_field_names(executable, value) {
            if !item_fields.contains(&field) {
                item_fields.push(field);
            }
        }
        targets.push(TypedDerivedListTarget {
            statement: statement.id,
            declaration,
            producer: value,
            path: path.to_owned(),
            local_name: name.to_owned(),
            capacity: match &statement.kind {
                ExecutableStatementKind::List { capacity, .. } => *capacity,
                _ => match expression.kind {
                    ExecutableExpressionKind::List { capacity, .. } => capacity,
                    _ => None,
                },
            },
            item_type: (**item_type).clone(),
            item_fields,
        });
    }
    Ok(targets)
}

fn executable_list_item_field_names(
    executable: &ExecutableProgram,
    root: ExecutableExprId,
) -> Vec<String> {
    let mut fields = Vec::new();
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        let Some(expression) = executable
            .expressions
            .get(expression_id.as_usize())
            .filter(|expression| expression.id == expression_id)
        else {
            continue;
        };
        match &expression.kind {
            ExecutableExpressionKind::Object(record_fields)
            | ExecutableExpressionKind::Record(record_fields)
            | ExecutableExpressionKind::TaggedObject {
                fields: record_fields,
                ..
            } => {
                for field in record_fields {
                    if !field.spread && !fields.contains(&field.name) {
                        fields.push(field.name.clone());
                    }
                }
            }
            ExecutableExpressionKind::Then { output, .. }
            | ExecutableExpressionKind::MatchArm { output, .. } => {
                pending.extend(output.iter().copied());
            }
            ExecutableExpressionKind::When { arms, .. } => {
                pending.extend(arms.iter().map(|arm| arm.output));
            }
            ExecutableExpressionKind::Latest { branches } => {
                pending.extend(branches.iter().copied());
            }
            ExecutableExpressionKind::Hold {
                initial, updates, ..
            } => {
                pending.push(*initial);
                pending.extend(updates.iter().copied());
            }
            ExecutableExpressionKind::List { items, .. } => {
                pending.extend(items.iter().copied());
            }
            ExecutableExpressionKind::Draining { input }
            | ExecutableExpressionKind::Project { input, .. } => {
                pending.push(*input);
            }
            ExecutableExpressionKind::Block { bindings, result } => {
                pending.extend(bindings.iter().map(|binding| binding.value));
                pending.push(*result);
            }
            ExecutableExpressionKind::Materialize { .. } => {}
            ExecutableExpressionKind::TextTemplate { .. } => {}
            ExecutableExpressionKind::CanonicalRead { .. }
            | ExecutableExpressionKind::LocalRead { .. }
            | ExecutableExpressionKind::ExternalRead { .. }
            | ExecutableExpressionKind::ElementState { .. }
            | ExecutableExpressionKind::Drain { .. }
            | ExecutableExpressionKind::Text(_)
            | ExecutableExpressionKind::Number(_)
            | ExecutableExpressionKind::BytesByte(_)
            | ExecutableExpressionKind::Bool(_)
            | ExecutableExpressionKind::Tag(_)
            | ExecutableExpressionKind::Source { .. }
            | ExecutableExpressionKind::Call { .. }
            | ExecutableExpressionKind::Infix { .. }
            | ExecutableExpressionKind::Bytes { .. }
            | ExecutableExpressionKind::Delimiter
            | ExecutableExpressionKind::MaterializationLocal { .. }
            | ExecutableExpressionKind::FunctionParameter { .. } => {}
        }
    }
    fields
}

fn exact_list_item_field_types(
    executable: &ExecutableProgram,
    root: ExecutableExprId,
) -> Result<BTreeMap<String, boon_typecheck::Type>, String> {
    let statement_values = executable
        .statements
        .iter()
        .filter_map(|statement| Some((statement.declaration?, statement.value?)))
        .collect::<BTreeMap<_, _>>();
    let local_values = executable
        .expressions
        .iter()
        .flat_map(|expression| match &expression.kind {
            ExecutableExpressionKind::Block { bindings, .. } => bindings.as_slice(),
            _ => &[],
        })
        .map(|binding| (binding.declaration, binding.value))
        .collect::<BTreeMap<_, _>>();
    let mut fields = BTreeMap::new();
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        let expression = executable
            .expressions
            .get(expression_id.as_usize())
            .filter(|expression| expression.id == expression_id)
            .ok_or_else(|| {
                format!("list item schema references missing expression {expression_id}")
            })?;
        match &expression.kind {
            ExecutableExpressionKind::Object(record_fields)
            | ExecutableExpressionKind::Record(record_fields)
            | ExecutableExpressionKind::TaggedObject {
                fields: record_fields,
                ..
            } => {
                for field in record_fields {
                    if field.spread {
                        pending.push(field.value);
                        continue;
                    }
                    let value = executable
                        .expressions
                        .get(field.value.as_usize())
                        .filter(|value| value.id == field.value)
                        .ok_or_else(|| {
                            format!(
                                "list item field `{}` references missing expression {}",
                                field.name, field.value
                            )
                        })?;
                    merge_authority_field_type(
                        &mut fields,
                        &field.name,
                        value.flow_type.ty.clone(),
                    )?;
                }
            }
            ExecutableExpressionKind::Then { output, .. }
            | ExecutableExpressionKind::MatchArm { output, .. } => {
                pending.extend(output.iter().copied());
            }
            ExecutableExpressionKind::When { arms, .. } => {
                pending.extend(arms.iter().map(|arm| arm.output));
            }
            ExecutableExpressionKind::Latest { branches } => {
                pending.extend(branches.iter().copied());
            }
            ExecutableExpressionKind::Hold {
                initial, updates, ..
            } => {
                pending.push(*initial);
                pending.extend(updates.iter().copied());
            }
            ExecutableExpressionKind::List { items, .. } => {
                pending.extend(items.iter().copied());
            }
            ExecutableExpressionKind::Draining { input } => pending.push(*input),
            ExecutableExpressionKind::Project { input, .. } => {
                if let boon_typecheck::Type::Object(shape) = &expression.flow_type.ty {
                    for (name, ty) in &shape.fields {
                        merge_authority_field_type(&mut fields, name, ty.clone())?;
                    }
                } else {
                    pending.push(*input);
                }
            }
            ExecutableExpressionKind::Block { result, .. } => pending.push(*result),
            ExecutableExpressionKind::CanonicalRead {
                target, projection, ..
            } => {
                if projection.is_empty() {
                    if let Some(value) = statement_values.get(target) {
                        pending.push(*value);
                    }
                } else if let boon_typecheck::Type::Object(shape) = &expression.flow_type.ty {
                    for (name, ty) in &shape.fields {
                        merge_authority_field_type(&mut fields, name, ty.clone())?;
                    }
                }
            }
            ExecutableExpressionKind::LocalRead {
                declaration,
                projection,
            } => {
                if projection.is_empty() {
                    if let Some(value) = local_values.get(declaration) {
                        pending.push(*value);
                    }
                } else if let boon_typecheck::Type::Object(shape) = &expression.flow_type.ty {
                    for (name, ty) in &shape.fields {
                        merge_authority_field_type(&mut fields, name, ty.clone())?;
                    }
                }
            }
            ExecutableExpressionKind::Call { .. } => {
                if let boon_typecheck::Type::Object(shape) = &expression.flow_type.ty {
                    for (name, ty) in &shape.fields {
                        merge_authority_field_type(&mut fields, name, ty.clone())?;
                    }
                }
            }
            ExecutableExpressionKind::Materialize { .. }
            | ExecutableExpressionKind::TextTemplate { .. }
            | ExecutableExpressionKind::ExternalRead { .. }
            | ExecutableExpressionKind::ElementState { .. }
            | ExecutableExpressionKind::Drain { .. }
            | ExecutableExpressionKind::Text(_)
            | ExecutableExpressionKind::Number(_)
            | ExecutableExpressionKind::BytesByte(_)
            | ExecutableExpressionKind::Bool(_)
            | ExecutableExpressionKind::Tag(_)
            | ExecutableExpressionKind::Source { .. }
            | ExecutableExpressionKind::Infix { .. }
            | ExecutableExpressionKind::Bytes { .. }
            | ExecutableExpressionKind::Delimiter
            | ExecutableExpressionKind::MaterializationLocal { .. }
            | ExecutableExpressionKind::FunctionParameter { .. } => {}
        }
    }
    Ok(fields)
}

fn typed_item_field_names(item_type: &boon_typecheck::Type) -> Vec<String> {
    let boon_typecheck::Type::Object(shape) = item_type else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    shape
        .field_order
        .iter()
        .chain(shape.fields.keys())
        .filter(|field| seen.insert((*field).clone()))
        .cloned()
        .collect()
}

fn materialize_typed_derived_list_storage(
    checked: &boon_typecheck::CheckedProgram,
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    row_scopes: &mut Vec<RowScope>,
    lists: &mut Vec<ListMemory>,
) -> Result<BTreeMap<ExecutableStatementId, DerivedListStorageIds>, String> {
    let targets = typed_derived_list_targets(executable)?;
    lists.clear();
    row_scopes.clear();

    let mut storage = BTreeMap::new();
    for target in targets {
        let checked_statement = checked
            .statements
            .iter()
            .find(|statement| statement.id.0 as usize == target.statement.as_usize());
        let source_line = checked_statement.map_or(0, |statement| statement.span.line);
        let list_id = ListId(lists.len());
        let row_scope_id = ScopeId(row_scopes.len());
        row_scopes.push(RowScope {
            id: row_scope_id,
            list: target.path.clone(),
            function: "checked_list".to_owned(),
            row_scope: format!("list_{}_row", list_id.as_usize()),
        });
        let list = ListMemory {
            id: list_id,
            name: target.path.clone(),
            source_line,
            row_scope_id: Some(row_scope_id),
            hidden_key_type: hidden_key_type(&target.path),
            has_generation: true,
            graph_clones_per_item: 0,
            capacity: target.capacity,
            initializer: executable_list_initializer(
                executable,
                materializations,
                target.producer,
                &target.path,
            )?,
        };
        lists.push(list);
        if storage
            .insert(
                target.statement,
                DerivedListStorageIds {
                    path: target.path.clone(),
                    list_id,
                    row_scope_id,
                    item_type: target.item_type,
                    item_fields: target.item_fields,
                },
            )
            .is_some()
        {
            return Err(format!(
                "typed list declaration {} (`{}` producer {}) was allocated more than once",
                target.declaration.0, target.path, target.producer
            ));
        }
    }
    Ok(storage)
}

fn inline_list_authority_root(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    root: ExecutableExprId,
) -> Option<ExecutableExprId> {
    fn walk(
        executable: &ExecutableProgram,
        materializations: &[ContextualMaterialization],
        expression_id: ExecutableExprId,
        visited: &mut BTreeSet<ExecutableExprId>,
    ) -> Option<ExecutableExprId> {
        if !visited.insert(expression_id) {
            return None;
        }
        let expression = executable
            .expressions
            .get(expression_id.as_usize())
            .filter(|expression| expression.id == expression_id)?;
        match &expression.kind {
            ExecutableExpressionKind::List { .. } => Some(expression_id),
            ExecutableExpressionKind::Call { name, .. } if name == "List/range" => {
                Some(expression_id)
            }
            ExecutableExpressionKind::Call { arguments, .. }
                if matches!(expression.flow_type.ty, boon_typecheck::Type::List(_)) =>
            {
                let list_inputs = arguments
                    .iter()
                    .filter(|argument| {
                        executable
                            .expressions
                            .get(argument.value.as_usize())
                            .filter(|candidate| candidate.id == argument.value)
                            .is_some_and(|candidate| {
                                matches!(candidate.flow_type.ty, boon_typecheck::Type::List(_))
                            })
                    })
                    .map(|argument| argument.value)
                    .collect::<Vec<_>>();
                let [input] = list_inputs.as_slice() else {
                    return None;
                };
                walk(executable, materializations, *input, visited)
            }
            ExecutableExpressionKind::Materialize { materialization } => materializations
                .get(*materialization)
                .filter(|candidate| candidate.id == *materialization)
                .and_then(|materialization| {
                    walk(
                        executable,
                        materializations,
                        materialization.source,
                        visited,
                    )
                }),
            ExecutableExpressionKind::Draining { input }
            | ExecutableExpressionKind::Project { input, .. } => {
                walk(executable, materializations, *input, visited)
            }
            ExecutableExpressionKind::Block { result, .. } => {
                walk(executable, materializations, *result, visited)
            }
            ExecutableExpressionKind::Then { output, .. }
            | ExecutableExpressionKind::MatchArm { output, .. } => {
                output.and_then(|output| walk(executable, materializations, output, visited))
            }
            _ => None,
        }
    }

    walk(executable, materializations, root, &mut BTreeSet::new())
}

fn executable_list_initializer(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    producer: ExecutableExprId,
    path: &str,
) -> Result<ListInitializer, String> {
    let Some(root) = inline_list_authority_root(executable, materializations, producer) else {
        return Ok(ListInitializer::Empty);
    };
    let expression = executable
        .expressions
        .get(root.as_usize())
        .filter(|expression| expression.id == root)
        .ok_or_else(|| format!("list `{path}` authority root {root} is missing"))?;
    match &expression.kind {
        ExecutableExpressionKind::List { items, .. } => {
            if items.is_empty() {
                return Ok(ListInitializer::Empty);
            }
            let mut rows = Vec::with_capacity(items.len());
            for item in items {
                rows.push(
                    executable_initial_record(executable, *item).map_err(|error| {
                        format!("list `{path}` authority item {item} is invalid: {error}")
                    })?,
                );
            }
            Ok(ListInitializer::RecordLiteral { rows })
        }
        ExecutableExpressionKind::Call {
            name, arguments, ..
        } if name == "List/range" => {
            let bound = |name: &str| -> Result<i64, String> {
                let argument = arguments
                    .iter()
                    .find(|argument| argument.name == name)
                    .ok_or_else(|| format!("List/range authority has no `{name}` argument"))?;
                let value = executable_static_data(
                    executable,
                    argument.value,
                    &BTreeMap::new(),
                    &mut BTreeSet::new(),
                )?;
                let boon_data::Value::Number(value) = value else {
                    return Err(format!("List/range `{name}` is not a Number"));
                };
                value.to_i64_exact().map_err(|error| {
                    format!("List/range `{name}` is not an exact integer: {error}")
                })
            };
            Ok(ListInitializer::Range {
                from: bound("from")?,
                to: bound("to")?,
            })
        }
        other => Err(format!(
            "list `{path}` authority root {root} has unsupported executable shape {other:?}"
        )),
    }
}

fn executable_initial_record(
    executable: &ExecutableProgram,
    expression_id: ExecutableExprId,
) -> Result<ListInitialRecord, String> {
    let expression = executable
        .expressions
        .get(expression_id.as_usize())
        .filter(|expression| expression.id == expression_id)
        .ok_or_else(|| format!("missing executable expression {expression_id}"))?;
    let fields = match &expression.kind {
        ExecutableExpressionKind::Object(fields) | ExecutableExpressionKind::Record(fields) => {
            fields
        }
        _ => {
            let value = executable_static_data(
                executable,
                expression_id,
                &BTreeMap::new(),
                &mut BTreeSet::new(),
            )?;
            return initial_record_from_data(value)
                .ok_or_else(|| format!("expression {expression_id} is not a record"));
        }
    };

    let mut result = Vec::new();
    for field in fields {
        if field.spread {
            let value = executable_static_data(
                executable,
                field.value,
                &BTreeMap::new(),
                &mut BTreeSet::new(),
            )?;
            let boon_data::Value::Record(fields) = value else {
                return Err(format!(
                    "spread field `{}` is not a static record",
                    field.name
                ));
            };
            result.extend(fields.into_iter().map(|(name, value)| ListRowInitialField {
                name,
                value: initial_value_from_data(value),
                expression: None,
            }));
            continue;
        }
        result.push(ListRowInitialField {
            name: field.name.clone(),
            value: executable_initial_value(executable, field.value)?,
            expression: Some(field.value),
        });
    }
    Ok(ListInitialRecord { fields: result })
}

fn executable_initial_value(
    executable: &ExecutableProgram,
    expression_id: ExecutableExprId,
) -> Result<InitialValue, String> {
    if let Some(path) = executable_root_initial_path(executable, expression_id) {
        return Ok(InitialValue::RootInitialField { path });
    }
    Ok(executable_static_data(
        executable,
        expression_id,
        &BTreeMap::new(),
        &mut BTreeSet::new(),
    )
    .map(initial_value_from_data)
    .unwrap_or_else(|_| InitialValue::Unknown {
        summary: format!("executable expression {expression_id}"),
    }))
}

fn executable_root_initial_path(
    executable: &ExecutableProgram,
    expression_id: ExecutableExprId,
) -> Option<String> {
    let expression = executable
        .expressions
        .get(expression_id.as_usize())
        .filter(|expression| expression.id == expression_id)?;
    match &expression.kind {
        ExecutableExpressionKind::CanonicalRead {
            path, projection, ..
        } => Some(
            std::iter::once(path.as_str())
                .chain(projection.iter().map(String::as_str))
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("."),
        ),
        ExecutableExpressionKind::Project { input, fields } => {
            let base = executable_root_initial_path(executable, *input)?;
            Some(
                std::iter::once(base.as_str())
                    .chain(fields.iter().map(String::as_str))
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("."),
            )
        }
        ExecutableExpressionKind::Draining { input } => {
            executable_root_initial_path(executable, *input)
        }
        ExecutableExpressionKind::MatchArm {
            output: Some(output),
            ..
        }
        | ExecutableExpressionKind::Then {
            output: Some(output),
            ..
        } => executable_root_initial_path(executable, *output),
        _ => None,
    }
}

fn executable_static_data(
    executable: &ExecutableProgram,
    expression_id: ExecutableExprId,
    locals: &BTreeMap<boon_typecheck::DeclId, boon_data::Value>,
    visiting: &mut BTreeSet<ExecutableExprId>,
) -> Result<boon_data::Value, String> {
    if !visiting.insert(expression_id) {
        return Err(format!("static expression cycle at {expression_id}"));
    }
    let result = (|| {
        let expression = executable
            .expressions
            .get(expression_id.as_usize())
            .filter(|expression| expression.id == expression_id)
            .ok_or_else(|| format!("missing executable expression {expression_id}"))?;
        match &expression.kind {
            ExecutableExpressionKind::Text(value) => Ok(boon_data::Value::Text(value.clone())),
            ExecutableExpressionKind::Number(value) => value
                .parse::<boon_data::FiniteReal>()
                .map(boon_data::Value::Number)
                .map_err(|error| format!("invalid finite Number `{value}`: {error}")),
            ExecutableExpressionKind::BytesByte(value) => Ok(boon_data::Value::Bytes(
                boon_data::Bytes::copy_from_slice(&[*value]),
            )),
            ExecutableExpressionKind::Bool(value) => Ok(boon_data::Value::Bool(*value)),
            ExecutableExpressionKind::Tag(tag) => Ok(boon_data::Value::Variant {
                tag: tag.clone(),
                fields: BTreeMap::new(),
            }),
            ExecutableExpressionKind::TextTemplate { segments } => {
                let mut text = String::new();
                for segment in segments {
                    match segment {
                        ExecutableTextSegment::Static { value } => text.push_str(value),
                        ExecutableTextSegment::Dynamic { value } => {
                            let boon_data::Value::Text(value) =
                                executable_static_data(executable, *value, locals, visiting)?
                            else {
                                return Err(format!(
                                    "text template expression {value} is not static Text"
                                ));
                            };
                            text.push_str(&value);
                        }
                    }
                }
                Ok(boon_data::Value::Text(text))
            }
            ExecutableExpressionKind::Object(fields) | ExecutableExpressionKind::Record(fields) => {
                let fields = executable_static_record(executable, fields, locals, visiting)?;
                Ok(boon_data::Value::Record(fields))
            }
            ExecutableExpressionKind::TaggedObject { tag, fields } => {
                let fields = executable_static_record(executable, fields, locals, visiting)?;
                Ok(boon_data::Value::Variant {
                    tag: tag.clone(),
                    fields,
                })
            }
            ExecutableExpressionKind::List { items, .. } => items
                .iter()
                .map(|item| executable_static_data(executable, *item, locals, visiting))
                .collect::<Result<Vec<_>, _>>()
                .map(boon_data::Value::List),
            ExecutableExpressionKind::Bytes { fixed_size, items } => {
                let mut bytes = Vec::new();
                for item in items {
                    let boon_data::Value::Bytes(value) =
                        executable_static_data(executable, *item, locals, visiting)?
                    else {
                        return Err(format!("BYTES item {item} is not static BYTES"));
                    };
                    bytes.extend_from_slice(&value);
                }
                if let Some(expected) = fixed_size {
                    if items.is_empty() {
                        bytes.resize(*expected, 0);
                    } else if *expected != bytes.len() {
                        return Err(format!(
                            "BYTES literal has {} bytes, expected {expected}",
                            bytes.len()
                        ));
                    }
                }
                Ok(boon_data::Value::Bytes(bytes.into()))
            }
            ExecutableExpressionKind::Block { bindings, result } => {
                let mut scoped = locals.clone();
                for binding in bindings {
                    let value =
                        executable_static_data(executable, binding.value, &scoped, visiting)?;
                    scoped.insert(binding.declaration, value);
                }
                executable_static_data(executable, *result, &scoped, visiting)
            }
            ExecutableExpressionKind::LocalRead {
                declaration,
                projection,
            } => {
                let value = locals.get(declaration).cloned().ok_or_else(|| {
                    format!("local declaration {} has no static binding", declaration.0)
                })?;
                executable_static_projection(value, projection)
            }
            ExecutableExpressionKind::CanonicalRead {
                target, projection, ..
            } => {
                let value = executable
                    .statements
                    .iter()
                    .find(|statement| statement.declaration == Some(*target))
                    .and_then(|statement| statement.value)
                    .ok_or_else(|| {
                        format!("declaration {} has no static executable value", target.0)
                    })?;
                let value = executable_static_data(executable, value, locals, visiting)?;
                executable_static_projection(value, projection)
            }
            ExecutableExpressionKind::Project { input, fields } => {
                let value = executable_static_data(executable, *input, locals, visiting)?;
                executable_static_projection(value, fields)
            }
            ExecutableExpressionKind::MatchArm {
                output: Some(output),
                ..
            }
            | ExecutableExpressionKind::Then {
                output: Some(output),
                ..
            } => executable_static_data(executable, *output, locals, visiting),
            other => Err(format!(
                "expression {expression_id} has non-static executable shape {other:?}"
            )),
        }
    })();
    visiting.remove(&expression_id);
    result
}

fn executable_static_record(
    executable: &ExecutableProgram,
    fields: &[ExecutableRecordField],
    locals: &BTreeMap<boon_typecheck::DeclId, boon_data::Value>,
    visiting: &mut BTreeSet<ExecutableExprId>,
) -> Result<BTreeMap<String, boon_data::Value>, String> {
    let mut result = BTreeMap::new();
    for field in fields {
        let value = executable_static_data(executable, field.value, locals, visiting)?;
        if field.spread {
            let boon_data::Value::Record(fields) = value else {
                return Err(format!(
                    "spread field `{}` is not a static record",
                    field.name
                ));
            };
            result.extend(fields);
        } else {
            result.insert(field.name.clone(), value);
        }
    }
    Ok(result)
}

fn executable_static_projection(
    mut value: boon_data::Value,
    projection: &[String],
) -> Result<boon_data::Value, String> {
    for field in projection {
        value = match value {
            boon_data::Value::Record(mut fields)
            | boon_data::Value::Variant { mut fields, .. }
            | boon_data::Value::Error { mut fields, .. } => fields
                .remove(field)
                .ok_or_else(|| format!("static value has no field `{field}`"))?,
            _ => {
                return Err(format!(
                    "cannot project `{field}` from non-record static value"
                ));
            }
        };
    }
    Ok(value)
}

#[derive(Default)]
struct RuntimeResourceAliases {
    owned: BTreeMap<(Option<StaticOwnerId>, String), RuntimeResourceAliasTarget>,
    owner_parents: BTreeMap<StaticOwnerId, Option<StaticOwnerId>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeResourceAliasTarget {
    Source(SourceId),
    State(StateId),
}

impl RuntimeResourceAliases {
    fn bind_owner_parents(&mut self, owners: &[StaticOwnerDef]) -> Result<(), String> {
        for owner in owners {
            if let Some(previous) = self.owner_parents.insert(owner.id, owner.parent)
                && previous != owner.parent
            {
                return Err(format!(
                    "static owner {} has conflicting parents {previous:?} and {:?}",
                    owner.id, owner.parent
                ));
            }
        }
        Ok(())
    }

    fn owner_chain(
        &self,
        owner: Option<StaticOwnerId>,
    ) -> Result<Vec<Option<StaticOwnerId>>, String> {
        let mut chain = Vec::new();
        let mut current = owner;
        let mut visiting = BTreeSet::new();
        while let Some(owner) = current {
            if !visiting.insert(owner) {
                return Err(format!("static owner ancestry contains a cycle at {owner}"));
            }
            chain.push(Some(owner));
            current = self.owner_parents.get(&owner).copied().ok_or_else(|| {
                format!("runtime resource alias references missing static owner {owner}")
            })?;
        }
        chain.push(None);
        Ok(chain)
    }
}

fn insert_resource_alias(
    aliases: &mut RuntimeResourceAliases,
    owner: Option<StaticOwnerId>,
    from: &str,
    target: RuntimeResourceAliasTarget,
) -> Result<(), String> {
    let key = (owner, from.to_owned());
    if let Some(previous) = aliases.owned.insert(key, target)
        && previous != target
    {
        return Err(format!(
            "runtime resource alias `{from}` for owner {owner:?} resolves to both {previous:?} and {target:?}"
        ));
    }
    Ok(())
}

fn merge_resource_aliases(
    aliases: &mut RuntimeResourceAliases,
    additions: RuntimeResourceAliases,
) -> Result<(), String> {
    for ((owner, from), target) in additions.owned {
        insert_resource_alias(aliases, owner, &from, target)?;
    }
    Ok(())
}

fn runtime_resource_alias_target_path<'a>(
    target: RuntimeResourceAliasTarget,
    sources: &'a [SourcePort],
    state_paths: &'a [String],
) -> Result<&'a str, String> {
    match target {
        RuntimeResourceAliasTarget::Source(source) => sources
            .get(source.as_usize())
            .filter(|candidate| candidate.id == source)
            .map(|source| source.path.as_str())
            .ok_or_else(|| format!("runtime resource alias references missing SourceId {source}")),
        RuntimeResourceAliasTarget::State(state) => state_paths
            .get(state.as_usize())
            .map(String::as_str)
            .ok_or_else(|| format!("runtime resource alias references missing StateId {state}")),
    }
}

fn canonical_resource_path(
    path: &str,
    owner: Option<StaticOwnerId>,
    aliases: &RuntimeResourceAliases,
    sources: &[SourcePort],
    state_paths: &[String],
) -> Result<String, String> {
    for owner in aliases.owner_chain(owner)? {
        if let Some(target) = aliases.owned.get(&(owner, path.to_owned())) {
            return Ok(
                runtime_resource_alias_target_path(*target, sources, state_paths)?.to_owned(),
            );
        }
        if let Some((_, target, suffix)) = aliases
            .owned
            .iter()
            .filter(|((alias_owner, _), _)| *alias_owner == owner)
            .filter_map(|((_, alias), target)| {
                path.strip_prefix(alias)
                    .filter(|suffix| suffix.starts_with('.'))
                    .map(|suffix| (alias.len(), *target, suffix))
            })
            .max_by_key(|(length, _, _)| *length)
        {
            let canonical = runtime_resource_alias_target_path(target, sources, state_paths)?;
            return Ok(format!("{canonical}{suffix}"));
        }
    }
    Ok(path.to_owned())
}

fn canonicalize_runtime_resource_metadata(
    dependencies: &mut [DependencyEdge],
    possible_causes: &mut [PossibleCause],
    state_cells: &mut [StateCell],
    sources: &[SourcePort],
    aliases: &RuntimeResourceAliases,
) -> Result<(), String> {
    let state_paths = state_cells
        .iter()
        .map(|state| state.path.clone())
        .collect::<Vec<_>>();
    let state_owners = state_cells
        .iter()
        .map(|state| (state.path.clone(), state.static_owner))
        .collect::<BTreeMap<_, _>>();
    for dependency in dependencies {
        let owner = state_owners.get(&dependency.to).copied().flatten();
        dependency.from =
            canonical_resource_path(&dependency.from, owner, aliases, sources, &state_paths)?;
        dependency.to =
            canonical_resource_path(&dependency.to, owner, aliases, sources, &state_paths)?;
    }
    for cause in possible_causes {
        let owner = state_owners.get(&cause.target).copied().flatten();
        cause.target =
            canonical_resource_path(&cause.target, owner, aliases, sources, &state_paths)?;
        for source in &mut cause.sources {
            *source = canonical_resource_path(source, owner, aliases, sources, &state_paths)?;
        }
        cause.sources.sort();
        cause.sources.dedup();
    }
    Ok(())
}

fn source_metadata_matches_checked_expression(
    source: &SourcePort,
    checked_expression: ExprId,
    checked_span: boon_typecheck::CheckedSpan,
) -> bool {
    source.source_expr_id.map_or_else(
        || source.source_line == checked_span.line,
        |expression| expression == checked_expression,
    )
}

fn is_canonical_resource_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('.').all(|segment| {
            let mut chars = segment.chars();
            chars
                .next()
                .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
                && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
        })
}

fn bind_executable_source_resources(
    checked: &boon_typecheck::CheckedProgram,
    executable: &ExecutableProgram,
    materialization_targets: &BTreeMap<StaticOwnerId, ListId>,
    lists: &[ListMemory],
    parser_source_count: usize,
    sources: &mut Vec<SourcePort>,
) -> Result<RuntimeResourceAliases, String> {
    fn merge_metadata(
        source: ExecutableSourceId,
        records: &[&SourcePort],
    ) -> Result<(Option<u64>, SourcePayloadSchema), String> {
        let mut interval_ms = None;
        let mut fields = BTreeSet::new();
        let mut typed_fields = BTreeMap::new();
        for record in records {
            if let Some(interval) = record.interval_ms {
                if let Some(existing) = interval_ms
                    && existing != interval
                {
                    return Err(format!(
                        "executable source {source} has conflicting interval metadata {existing} and {interval}"
                    ));
                }
                interval_ms = Some(interval);
            }
            fields.extend(record.payload_schema.fields.iter().cloned());
            for descriptor in &record.payload_schema.typed_fields {
                if let Some(existing) =
                    typed_fields.insert(descriptor.field.clone(), descriptor.data_type.clone())
                    && existing != descriptor.data_type
                {
                    return Err(format!(
                        "executable source {source} has conflicting payload types for `{}`",
                        descriptor.field.name()
                    ));
                }
            }
        }
        fields.extend(typed_fields.keys().cloned());
        Ok((
            interval_ms,
            SourcePayloadSchema {
                fields: fields.into_iter().collect(),
                typed_fields: typed_fields
                    .into_iter()
                    .map(|(field, data_type)| SourcePayloadDescriptor { field, data_type })
                    .collect(),
            },
        ))
    }

    let parser_sources = sources
        .get(..parser_source_count)
        .ok_or_else(|| {
            format!(
                "parser source count {parser_source_count} exceeds {} runtime source records",
                sources.len()
            )
        })?
        .to_vec();
    let distributed_sources = sources
        .get(parser_source_count..)
        .unwrap_or_default()
        .to_vec();
    let mut bound = Vec::with_capacity(executable.sources.len() + distributed_sources.len());
    let mut aliases = RuntimeResourceAliases::default();
    for executable_source in &executable.sources {
        let executable_expression = executable
            .expressions
            .get(executable_source.expression.as_usize())
            .filter(|expression| expression.id == executable_source.expression)
            .ok_or_else(|| {
                format!(
                    "executable source {} has no executable expression",
                    executable_source.id
                )
            })?;
        let checked_expression = ExprId(executable_expression.checked_expr_id.0 as usize);
        let checked_span = checked
            .expressions
            .get(executable_expression.checked_expr_id.0 as usize)
            .filter(|expression| expression.id == executable_expression.checked_expr_id)
            .map(|expression| expression.span)
            .ok_or_else(|| {
                format!(
                    "executable source {} has no checked expression provenance",
                    executable_source.id
                )
            })?;
        let target = executable_source
            .owner
            .and_then(|owner| materialization_targets.get(&owner))
            .and_then(|list| lists.get(list.as_usize()))
            .map(|list| (list.name.as_str(), list));
        let matches = parser_sources
            .iter()
            .filter(|source| {
                source_metadata_matches_checked_expression(source, checked_expression, checked_span)
            })
            .collect::<Vec<_>>();
        let payload_projections = executable
            .expressions
            .iter()
            .filter_map(|expression| match &expression.kind {
                ExecutableExpressionKind::CanonicalRead {
                    target,
                    path,
                    projection,
                    ..
                } if *target == executable_source.declaration && !projection.is_empty() => {
                    let payload = executable_source_payload_projection(
                        &executable_source.binding_path,
                        path,
                        projection,
                    );
                    (!payload.is_empty()).then_some(payload)
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        if matches.is_empty() && !payload_projections.is_empty() {
            return Err(format!(
                "executable source {} (`{}` owner {:?}) has projected payload reads {:?} but no typed payload metadata; available parser metadata {:?}",
                executable_source.id,
                executable_source.binding_path,
                executable_source.owner,
                payload_projections,
                parser_sources
                    .iter()
                    .map(|source| (&source.path, &source.binding_path, source.scope_id))
                    .collect::<Vec<_>>()
            ));
        }
        let (interval_ms, payload_schema) =
            merge_metadata(executable_source.id, matches.as_slice())?;
        let expanded_path = executable_expression.resource_binding_path.as_deref();
        let canonical_path = match (target, expanded_path) {
            (Some((target, _)), Some(path))
                if path == target || path.starts_with(&format!("{target}.")) =>
            {
                path.to_owned()
            }
            (Some((target, _)), _) => format!("{target}.{}", executable_source.binding_path),
            (None, Some(path)) => path.to_owned(),
            (None, None) => executable_source.binding_path.clone(),
        };
        let source_id = SourceId(bound.len());
        for metadata in matches {
            insert_resource_alias(
                &mut aliases,
                executable_source.owner,
                &metadata.path,
                RuntimeResourceAliasTarget::Source(source_id),
            )?;
            insert_resource_alias(
                &mut aliases,
                executable_source.owner,
                &metadata.binding_path,
                RuntimeResourceAliasTarget::Source(source_id),
            )?;
        }
        let scope_id = target.and_then(|(_, list)| list.row_scope_id);
        bound.push(SourcePort {
            id: source_id,
            path: canonical_path.clone(),
            binding_path: canonical_path,
            executable_source_id: Some(executable_source.id),
            static_owner: executable_source.owner,
            source_expr_id: Some(checked_expression),
            source_line: checked_span.line,
            scoped: scope_id.is_some(),
            scope_id,
            interval_ms,
            payload_schema,
        });
    }
    for mut source in distributed_sources {
        source.id = SourceId(bound.len());
        bound.push(source);
    }
    *sources = bound;
    Ok(aliases)
}

fn executable_source_payload_projection(
    binding_path: &str,
    declaration_path: &str,
    projection: &[String],
) -> Vec<String> {
    if binding_path == declaration_path {
        return projection.to_vec();
    }
    let Some(relative_source) = binding_path
        .strip_prefix(declaration_path)
        .and_then(|suffix| suffix.strip_prefix('.'))
    else {
        return projection.to_vec();
    };
    let structural = relative_source
        .split('.')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    projection
        .strip_prefix(structural.as_slice())
        .unwrap_or(projection)
        .to_vec()
}

fn bind_distributed_reference_aliases(
    checked: &boon_typecheck::CheckedProgram,
    executable: &ExecutableProgram,
    references: &mut [DistributedValueReference],
) -> Result<(), String> {
    for reference in references {
        let checked_id = boon_typecheck::CheckedExprId(reference.expr_id.as_usize() as u32);
        let expression = checked
            .expressions
            .get(checked_id.0 as usize)
            .filter(|expression| expression.id == checked_id)
            .ok_or_else(|| {
                format!(
                    "distributed value `{}` references missing checked expression {}",
                    reference.canonical_path, checked_id.0
                )
            })?;
        if !matches!(
            &expression.kind,
            boon_typecheck::CheckedExpressionKind::ExternalRead { canonical_path }
                if canonical_path == &reference.canonical_path
        ) {
            return Err(format!(
                "distributed value `{}` expression {} is not its exact checked external read",
                reference.canonical_path, checked_id.0
            ));
        }
        let executable_reads = executable
            .expressions
            .iter()
            .filter(|candidate| candidate.checked_expr_id == checked_id)
            .filter(|candidate| {
                matches!(
                    &candidate.kind,
                    ExecutableExpressionKind::ExternalRead { canonical_path }
                        if canonical_path == &reference.canonical_path
                )
            })
            .map(|candidate| candidate.id)
            .collect::<BTreeSet<_>>();
        reference.local_alias_paths = executable
            .statements
            .iter()
            .filter(|statement| {
                statement
                    .value
                    .is_some_and(|value| executable_reads.contains(&value))
            })
            .filter_map(|statement| match &statement.kind {
                ExecutableStatementKind::Field { path, .. }
                | ExecutableStatementKind::List {
                    path: Some(path), ..
                }
                | ExecutableStatementKind::Source {
                    path: Some(path), ..
                }
                | ExecutableStatementKind::Hold {
                    path: Some(path), ..
                } => Some(path.clone()),
                ExecutableStatementKind::List { path: None, .. }
                | ExecutableStatementKind::Source { path: None, .. }
                | ExecutableStatementKind::Hold { path: None, .. }
                | ExecutableStatementKind::Block
                | ExecutableStatementKind::Spread
                | ExecutableStatementKind::Expression => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
    }
    Ok(())
}

fn bind_executable_state_resources(
    checked: &boon_typecheck::CheckedProgram,
    executable: &ExecutableProgram,
    materialization_targets: &BTreeMap<StaticOwnerId, ListId>,
    lists: &[ListMemory],
) -> Result<(Vec<StateCell>, RuntimeResourceAliases), String> {
    let mut aliases = RuntimeResourceAliases::default();
    let mut states = Vec::with_capacity(executable.states.len());
    let mut published = BTreeSet::new();
    for executable_state in &executable.states {
        let executable_expression = executable
            .expressions
            .get(executable_state.expression.as_usize())
            .filter(|expression| expression.id == executable_state.expression)
            .ok_or_else(|| {
                format!(
                    "executable state {} has no executable expression {}",
                    executable_state.id, executable_state.expression
                )
            })?;
        let declaration = checked
            .declarations
            .iter()
            .find(|declaration| declaration.id == executable_state.declaration)
            .ok_or_else(|| {
                format!(
                    "executable state {} references missing declaration {}",
                    executable_state.id, executable_state.declaration.0
                )
            })?;
        let statement = executable_state_statement(checked, executable, executable_state)?;
        let declared_path = executable_state_declared_path(statement.executable, executable_state)?;
        let hold_name =
            executable_state_hold_name(executable, executable_state, &statement, declaration)?;
        executable
            .expressions
            .get(executable_state.initial.as_usize())
            .filter(|expression| expression.id == executable_state.initial)
            .ok_or_else(|| {
                format!(
                    "executable state {} has no executable initial expression {}",
                    executable_state.id, executable_state.initial
                )
            })?;
        let expression_ids =
            executable_checked_expression_ids(executable, executable_state.expression)?;
        let is_published = !executable.states.iter().any(|candidate| {
            candidate.id != executable_state.id
                && candidate.declaration == executable_state.declaration
                && candidate.owner == executable_state.owner
                && executable_expression_reaches(
                    executable,
                    candidate.expression,
                    executable_state.expression,
                )
        });
        let target = executable_state
            .owner
            .and_then(|owner| materialization_targets.get(&owner))
            .and_then(|list| lists.get(list.as_usize()))
            .map(|list| (list.name.as_str(), list));
        if is_published && !published.insert((executable_state.declaration, executable_state.owner))
        {
            return Err(format!(
                "declaration {} owner {:?} publishes more than one executable state",
                executable_state.declaration.0, executable_state.owner
            ));
        }
        let semantic_path = is_published.then(|| {
            match (
                target,
                executable_expression.resource_binding_path.as_deref(),
            ) {
                (Some((target, _)), Some(path))
                    if path == target || path.starts_with(&format!("{target}.")) =>
                {
                    path.to_owned()
                }
                (Some((target, _)), _) => {
                    format!("{target}.{}", executable_state.binding_path)
                }
                (None, Some(path)) => path.to_owned(),
                (None, None) => is_canonical_resource_path(&executable_state.binding_path)
                    .then(|| executable_state.binding_path.clone())
                    .unwrap_or_else(|| declared_path.clone()),
            }
        });
        let state_id = StateId(states.len());
        if semantic_path.is_some() {
            insert_resource_alias(
                &mut aliases,
                executable_state.owner,
                &declared_path,
                RuntimeResourceAliasTarget::State(state_id),
            )?;
        }
        let scope_id = target.and_then(|(_, list)| list.row_scope_id);
        let path = if is_published {
            semantic_path.clone().ok_or_else(|| {
                format!(
                    "published executable state {} has no canonical semantic path",
                    executable_state.id
                )
            })?
        } else {
            format!("$state.s{}", executable_state.id.0)
        };
        states.push(StateCell {
            id: state_id,
            path,
            published: is_published,
            semantic_path,
            executable_state_id: Some(executable_state.id),
            static_owner: executable_state.owner,
            statement_id: statement.id,
            scope_id,
            hold_name: if is_published {
                hold_name
            } else {
                format!("{}#internal", executable_state.binding_path)
            },
            expression_ids,
            indexed: scope_id.is_some(),
            source_line: declaration.span.line,
        });
    }
    if states.len() != executable.states.len()
        || states.iter().enumerate().any(|(index, state)| {
            state.id != StateId(index)
                || state.executable_state_id != Some(ExecutableStateId(index))
        })
    {
        return Err("executable states and runtime state cells are not bijective".to_owned());
    }
    Ok((states, aliases))
}

struct ExecutableStateStatement<'a> {
    id: usize,
    checked: &'a boon_typecheck::CheckedStatement,
    executable: Option<&'a ExecutableStatement>,
}

fn executable_state_statement<'a>(
    checked: &'a boon_typecheck::CheckedProgram,
    executable: &'a ExecutableProgram,
    state: &ExecutableStateDef,
) -> Result<ExecutableStateStatement<'a>, String> {
    let checked_candidates = checked
        .statements
        .iter()
        .filter(|statement| {
            checked_statement_declaration(&statement.kind) == Some(state.declaration)
        })
        .collect::<Vec<_>>();
    let [checked_statement] = checked_candidates.as_slice() else {
        return Err(format!(
            "executable state {} declaration {} has {} checked statements",
            state.id,
            state.declaration.0,
            checked_candidates.len()
        ));
    };
    let executable_candidates = executable
        .statements
        .iter()
        .filter(|statement| statement.declaration == Some(state.declaration))
        .collect::<Vec<_>>();
    let executable_statement = match executable_candidates.as_slice() {
        [] => None,
        [statement] => Some(*statement),
        _ => {
            return Err(format!(
                "executable state {} declaration {} has {} executable statements",
                state.id,
                state.declaration.0,
                executable_candidates.len()
            ));
        }
    };
    if let Some(executable_statement) = executable_statement
        && executable_statement.id.as_usize() != checked_statement.id.0 as usize
    {
        return Err(format!(
            "executable state {} declaration {} statement identity differs between checked {} and executable {}",
            state.id, state.declaration.0, checked_statement.id.0, executable_statement.id
        ));
    }
    Ok(ExecutableStateStatement {
        id: checked_statement.id.0 as usize,
        checked: checked_statement,
        executable: executable_statement,
    })
}

fn checked_statement_declaration(
    kind: &boon_typecheck::CheckedStatementKind,
) -> Option<boon_typecheck::DeclId> {
    match kind {
        boon_typecheck::CheckedStatementKind::Function { declaration }
        | boon_typecheck::CheckedStatementKind::Field { declaration } => Some(*declaration),
        boon_typecheck::CheckedStatementKind::Source { declaration, .. }
        | boon_typecheck::CheckedStatementKind::Hold { declaration, .. }
        | boon_typecheck::CheckedStatementKind::List { declaration, .. } => *declaration,
        boon_typecheck::CheckedStatementKind::Block
        | boon_typecheck::CheckedStatementKind::Spread
        | boon_typecheck::CheckedStatementKind::Expression => None,
    }
}

fn executable_state_declared_path(
    statement: Option<&ExecutableStatement>,
    state: &ExecutableStateDef,
) -> Result<String, String> {
    let path = statement
        .and_then(|statement| match &statement.kind {
            ExecutableStatementKind::Field { path, .. } => Some(path.as_str()),
            ExecutableStatementKind::Hold {
                path: Some(path), ..
            }
            | ExecutableStatementKind::List {
                path: Some(path), ..
            }
            | ExecutableStatementKind::Source {
                path: Some(path), ..
            } => Some(path.as_str()),
            ExecutableStatementKind::Hold { path: None, .. }
            | ExecutableStatementKind::List { path: None, .. }
            | ExecutableStatementKind::Source { path: None, .. }
            | ExecutableStatementKind::Block
            | ExecutableStatementKind::Spread
            | ExecutableStatementKind::Expression => None,
        })
        .filter(|path| !path.is_empty())
        .or_else(|| {
            is_canonical_resource_path(&state.binding_path).then_some(state.binding_path.as_str())
        })
        .ok_or_else(|| {
            format!(
                "executable state {} declaration {} has no canonical statement path",
                state.id, state.declaration.0
            )
        })?;
    Ok(path.to_owned())
}

fn executable_state_hold_name(
    executable: &ExecutableProgram,
    state: &ExecutableStateDef,
    statement: &ExecutableStateStatement<'_>,
    declaration: &boon_typecheck::CheckedDeclaration,
) -> Result<String, String> {
    let expression = executable
        .expressions
        .get(state.expression.as_usize())
        .filter(|expression| expression.id == state.expression)
        .ok_or_else(|| format!("executable state {} has no state expression", state.id))?;
    let name = match &expression.kind {
        ExecutableExpressionKind::Hold { name, .. } if !name.is_empty() => Some(name.as_str()),
        _ => statement
            .executable
            .and_then(|statement| match &statement.kind {
                ExecutableStatementKind::Hold {
                    hold_name: Some(name),
                    ..
                } if !name.is_empty() => Some(name.as_str()),
                _ => None,
            })
            .or_else(|| match &statement.checked.kind {
                boon_typecheck::CheckedStatementKind::Hold {
                    name: Some(name), ..
                } if !name.is_empty() => Some(name.as_str()),
                _ => None,
            })
            .or_else(|| (!declaration.name.is_empty()).then_some(declaration.name.as_str())),
    }
    .ok_or_else(|| format!("executable state {} has no canonical HOLD name", state.id))?;
    Ok(name.to_owned())
}

fn executable_checked_expression_ids(
    executable: &ExecutableProgram,
    root: ExecutableExprId,
) -> Result<Vec<ExprId>, String> {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    let mut checked = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        let expression = executable
            .expressions
            .get(expression_id.as_usize())
            .filter(|expression| expression.id == expression_id)
            .ok_or_else(|| {
                format!("state expression graph references missing expression {expression_id}")
            })?;
        checked.insert(ExprId(expression.checked_expr_id.0 as usize));
        pending.extend(executable_expression_children(&expression.kind));
    }
    Ok(checked.into_iter().collect())
}

fn executable_expression_reaches(
    executable: &ExecutableProgram,
    root: ExecutableExprId,
    target: ExecutableExprId,
) -> bool {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        if expression_id == target {
            return true;
        }
        let Some(expression) = executable.expressions.get(expression_id.as_usize()) else {
            continue;
        };
        pending.extend(executable_expression_children(&expression.kind));
    }
    false
}

fn materialization_target_lists(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    static_owners: &[StaticOwnerDef],
) -> Result<BTreeMap<StaticOwnerId, ListId>, String> {
    let mut owner_targets = BTreeMap::<StaticOwnerId, ListId>::new();
    for statement in &executable.statements {
        let Some(storage) = list_storage.get(&statement.id) else {
            continue;
        };
        let Some(root) = statement.value else {
            continue;
        };
        let expression = executable.expressions.get(root.as_usize()).ok_or_else(|| {
            format!(
                "list storage statement {} references missing expression {root}",
                statement.id
            )
        })?;
        if !matches!(&expression.flow_type.ty, boon_typecheck::Type::List(_)) {
            continue;
        }
        let mut pending = vec![root];
        let mut visited = BTreeSet::new();
        while let Some(expression_id) = pending.pop() {
            if !visited.insert(expression_id) {
                continue;
            }
            let expression = executable
                .expressions
                .get(expression_id.as_usize())
                .ok_or_else(|| {
                    format!(
                        "state resource ownership reaches missing executable expression {expression_id}"
                    )
                })?;
            if let ExecutableExpressionKind::Materialize { materialization } = expression.kind {
                let materialization = materializations
                    .get(materialization)
                    .filter(|candidate| candidate.id == materialization)
                    .ok_or_else(|| {
                        format!(
                            "state resource ownership reaches missing materialization {materialization}"
                        )
                    })?;
                if let Some(existing) = owner_targets.insert(materialization.owner, storage.list_id)
                    && existing != storage.list_id
                {
                    return Err(format!(
                        "static owner {} ambiguously materializes ListId {existing} and {}",
                        materialization.owner, storage.list_id
                    ));
                }
                // A filtering/removal wrapper preserves ownership of rows produced by
                // its source chain. Body materializations are intentionally excluded:
                // they create child collections with independent ownership.
                pending.push(materialization.source);
                continue;
            }
            pending.extend(executable_expression_children(&expression.kind));
        }
    }
    let materialization_owners = materializations
        .iter()
        .map(|materialization| materialization.owner)
        .collect::<BTreeSet<_>>();
    for owner in static_owners {
        if owner_targets.contains_key(&owner.id) {
            continue;
        }
        if materialization_owners.contains(&owner.id) {
            continue;
        }
        let Some(parent) = owner.parent else {
            continue;
        };
        if let Some(inherited) = owner_targets.get(&parent).copied() {
            owner_targets.insert(owner.id, inherited);
        }
    }
    Ok(owner_targets)
}

fn bind_contextual_materialization_targets(
    targets: &BTreeMap<StaticOwnerId, ListId>,
    lists: &[ListMemory],
    materializations: &mut [ContextualMaterialization],
) -> Result<(), String> {
    for materialization in materializations {
        let Some(list_id) = targets.get(&materialization.owner).copied() else {
            continue;
        };
        let list = lists
            .get(list_id.as_usize())
            .filter(|list| list.id == list_id)
            .ok_or_else(|| {
                format!(
                    "contextual owner {} targets missing ListId {list_id}",
                    materialization.owner
                )
            })?;
        materialization.target_list_id = Some(list_id);
        materialization.target_scope_id = list.row_scope_id;
    }
    Ok(())
}

fn source_payload_schema(
    typecheck_report: &boon_typecheck::CheckedProgramLoweringMetadata,
    source: &str,
) -> SourcePayloadSchema {
    let typed_payload_fields = typecheck_report
        .source_payload_shape_table
        .iter()
        .find(|entry| entry.source_path == source)
        .into_iter()
        .flat_map(|entry| &entry.fields)
        .filter_map(|field| {
            let data_type = semantic_data_type(&field.ty);
            (!matches!(data_type, SemanticDataType::Unknown { .. }))
                .then(|| (SourcePayloadField::from_name(&field.name), data_type))
        })
        .collect::<BTreeMap<_, _>>();
    let payload_fields = typed_payload_fields.keys().cloned().collect::<Vec<_>>();
    SourcePayloadSchema {
        fields: payload_fields.clone(),
        typed_fields: payload_fields
            .into_iter()
            .map(|field| SourcePayloadDescriptor {
                data_type: typed_payload_fields
                    .get(&field)
                    .cloned()
                    .unwrap_or_else(|| source_payload_data_type(&field)),
                field,
            })
            .collect(),
    }
}

fn source_payload_data_type(field: &SourcePayloadField) -> SemanticDataType {
    match field {
        SourcePayloadField::Bytes => SemanticDataType::Bytes { fixed_len: None },
        SourcePayloadField::Named(name) if name == "press" => SemanticDataType::Bool,
        SourcePayloadField::Address | SourcePayloadField::Key | SourcePayloadField::Text => {
            SemanticDataType::Text
        }
        SourcePayloadField::Named(_) => SemanticDataType::Text,
    }
}

fn host_port_declarations(
    report: &boon_typecheck::CheckedProgramLoweringMetadata,
) -> Vec<HostPortDeclaration> {
    let mut declarations = Vec::with_capacity(
        usize::from(report.host_port_table.http.is_some())
            + usize::from(report.host_port_table.websocket.is_some()),
    );
    if let Some(http) = &report.host_port_table.http {
        declarations.push(HostPortDeclaration::HttpServer {
            line: http.line,
            request_source: http.request_source.clone(),
            disconnect_source: http.disconnect_source.clone(),
            response_output: http.response_output.clone(),
        });
    }
    if let Some(websocket) = &report.host_port_table.websocket {
        declarations.push(HostPortDeclaration::WebSocketServer {
            line: websocket.line,
            open_source: websocket.open_source.clone(),
            message_source: websocket.message_source.clone(),
            close_source: websocket.close_source.clone(),
            error_source: websocket.error_source.clone(),
            actions_output: websocket.actions_output.clone(),
        });
    }
    declarations
}

fn contextual_materializations(
    checked: &boon_typecheck::CheckedProgram,
    out_graph: &out_net::OutNet,
) -> Result<(Vec<ContextualMaterialization>, Vec<ExecutableExpression>), String> {
    contextual_expansion::derive_contextual_materializations(checked, out_graph)
        .map_err(|error| error.to_string())
}

fn view_bindings(
    executable: &ExecutableProgram,
    static_owners: &[StaticOwnerDef],
    storage: &ErasedScopeIndex,
    list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    output_values: &[OutputRootValue],
    row_scopes: &[RowScope],
    sources: &[SourcePort],
    states: &[StateCell],
    materializations: &[ContextualMaterialization],
) -> Result<Vec<ViewBinding>, String> {
    let mut collector = ExecutableViewBindingCollector::new(
        executable,
        static_owners,
        Some(storage),
        list_storage,
        row_scopes,
        sources,
        states,
        materializations,
    )?;
    collector.collect_output_roots(output_values)?;
    let mut bindings = collector.bindings;
    normalize_view_binding_ids(&mut bindings);
    Ok(bindings)
}

#[allow(clippy::too_many_arguments)]
fn bind_distributed_call_invocation_arms(
    executable: &ExecutableProgram,
    static_owners: &[StaticOwnerDef],
    storage: &ErasedScopeIndex,
    list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    row_scopes: &[RowScope],
    sources: &[SourcePort],
    states: &[StateCell],
    materializations: &[ContextualMaterialization],
    calls: &mut [DistributedCall],
) -> Result<(), String> {
    let mut collector = ExecutableViewBindingCollector::new(
        executable,
        static_owners,
        Some(storage),
        list_storage,
        row_scopes,
        sources,
        states,
        materializations,
    )?;
    for (reference, call) in calls.iter_mut().enumerate() {
        let current_capable = call.result.mode == boon_typecheck::FlowMode::Continuous
            && call
                .arguments
                .iter()
                .all(|argument| argument.flow_type.mode == boon_typecheck::FlowMode::Continuous)
            && !call.effect.emits_source
            && !call.effect.invokes_host;
        let dependent_roots = storage
            .dependencies
            .iter()
            .filter(|dependency| {
                matches!(
                    dependency.target,
                    ErasedDependencyTarget::ExternalCall { reference: candidate }
                        if candidate == reference
                )
            })
            .filter_map(|dependency| {
                storage
                    .bindings
                    .get(dependency.dependent.as_usize())
                    .filter(|binding| binding.id == dependency.dependent)
                    .map(|binding| binding.producer)
            })
            .collect::<BTreeSet<_>>();
        let mut arms = Vec::new();
        for root in dependent_roots {
            let root_arms = if current_capable {
                collector.trigger_owned_arms_before_expression(root, call.expression)?
            } else {
                collector.trigger_owned_arms_for_expression(root)?
            };
            for arm in root_arms {
                if executable_expression_distance(
                    executable,
                    materializations,
                    arm.output_expression_id,
                    call.expression,
                )
                .is_some()
                    && !arms.contains(&arm)
                {
                    arms.push(arm);
                }
            }
        }
        arms.sort_by_key(|arm| {
            (
                arm.cause,
                arm.gate_expression_id,
                arm.output_expression_id,
                arm.owner,
            )
        });
        call.invocation_arms = arms;
        let requires_invocation = !current_capable;
        if requires_invocation && call.invocation_arms.is_empty() {
            return Err(format!(
                "distributed call `{}` is stateful, effectful, or event-valued but has no exact SOURCE or state trigger",
                call.canonical_function
            ));
        }
    }
    Ok(())
}

fn bind_contextual_materialization_storage(
    executable: &ExecutableProgram,
    static_owners: &[StaticOwnerDef],
    list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    row_scopes: &[RowScope],
    lists: &[ListMemory],
    sources: &[SourcePort],
    states: &[StateCell],
    materializations: &mut [ContextualMaterialization],
) -> Result<(), String> {
    let storage = {
        let mut resolver = ExecutableViewBindingCollector::new(
            executable,
            static_owners,
            None,
            list_storage,
            row_scopes,
            sources,
            states,
            materializations,
        )?;
        materializations
            .iter()
            .map(|materialization| {
                let resolved_scope =
                    resolver.local_scope(materialization.owner, materialization.row_local)?;
                let mut scope = resolved_scope;
                let mut list = scope
                    .map(|scope| {
                        let mut matches =
                            lists.iter().filter(|list| list.row_scope_id == Some(scope));
                        let first = matches.next().map(|list| list.id);
                        if matches.next().is_some() {
                            return Err(format!(
                                "contextual owner {} source scope {} belongs to multiple lists",
                                materialization.owner, scope
                            ));
                        }
                        Ok(first)
                    })
                    .transpose()?
                    .flatten();
                if list.is_none()
                    && inline_list_authority_root(
                        executable,
                        materializations,
                        materialization.source,
                    )
                    .is_some()
                {
                    list = materialization.target_list_id;
                    scope = materialization.target_scope_id;
                }
                if list.is_none()
                    && matches!(
                        materialization.operation,
                        ContextualOperationKind::Filter
                            | ContextualOperationKind::Retain
                            | ContextualOperationKind::Remove
                            | ContextualOperationKind::SortBy
                            | ContextualOperationKind::ThenBy
                    )
                    && !matches!(
                        executable
                            .expressions
                            .get(materialization.source.as_usize())
                            .map(|expression| &expression.kind),
                        Some(ExecutableExpressionKind::Materialize { .. })
                    )
                {
                    list = materialization.target_list_id;
                    scope = materialization.target_scope_id;
                }
                Ok((list, scope))
            })
            .collect::<Result<Vec<_>, String>>()?
    };
    for (materialization, (list, scope)) in materializations.iter_mut().zip(storage) {
        materialization.source_list_id = list;
        materialization.source_scope_id = scope;
    }
    Ok(())
}

fn normalize_view_binding_ids(bindings: &mut Vec<ViewBinding>) {
    let mut seen = BTreeSet::new();
    bindings.retain(|binding| {
        seen.insert((
            binding.node_kind.clone(),
            binding.attr.clone(),
            binding.path.clone(),
            binding.target.clone(),
            view_binding_kind_key(binding.kind),
            binding.scope_id.map(ScopeId::as_usize),
        ))
    });
    for (index, binding) in bindings.iter_mut().enumerate() {
        binding.id = ViewBindingId(index);
    }
}

fn view_binding_kind_key(kind: ViewBindingKind) -> u8 {
    match kind {
        ViewBindingKind::Data => 0,
        ViewBindingKind::Source => 1,
        ViewBindingKind::Target => 2,
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ExecutableViewRead {
    read: ErasedReadId,
    additional_projection: Vec<String>,
    diagnostic_path: String,
    scope_id: Option<ScopeId>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ExecutableEventReference {
    Canonical {
        declaration: Option<boon_typecheck::DeclId>,
        owner: Option<StaticOwnerId>,
        path: String,
        projection: Vec<String>,
        expression: ExecutableExprId,
        source_expression: Option<boon_typecheck::CheckedExprId>,
    },
    Local {
        owner: StaticOwnerId,
        local: MaterializationLocalId,
        projection: Vec<String>,
    },
}

fn executable_local_read_values(
    executable: &ExecutableProgram,
) -> Result<BTreeMap<ExecutableExprId, ExecutableExprId>, String> {
    let mut targets = BTreeMap::new();
    for expression in executable
        .expressions
        .iter()
        .filter(|expression| matches!(expression.kind, ExecutableExpressionKind::Block { .. }))
    {
        collect_local_read_targets(
            executable,
            expression.id,
            &BTreeMap::new(),
            &mut BTreeSet::new(),
            &mut targets,
        )?;
    }
    Ok(targets
        .into_iter()
        .filter_map(|(read, target)| match target {
            ErasedReadTarget::Local { value, .. } => Some((read, value)),
            _ => None,
        })
        .collect())
}

struct ExecutableViewBindingCollector<'a> {
    executable: &'a ExecutableProgram,
    static_owners: &'a [StaticOwnerDef],
    storage: Option<&'a ErasedScopeIndex>,
    row_scopes: &'a [RowScope],
    sources: &'a [SourcePort],
    materializations: &'a [ContextualMaterialization],
    statement_values: BTreeMap<boon_typecheck::DeclId, ExecutableExprId>,
    local_values_by_read: BTreeMap<ExecutableExprId, ExecutableExprId>,
    list_scopes_by_declaration: BTreeMap<boon_typecheck::DeclId, ScopeId>,
    list_scopes_by_path: BTreeMap<String, ScopeId>,
    materializations_by_local: BTreeMap<(StaticOwnerId, MaterializationLocalId), usize>,
    reads_by_expression: Vec<Option<ErasedReadId>>,
    states_by_declaration: BTreeMap<(boon_typecheck::DeclId, Option<StaticOwnerId>), StateId>,
    local_scope_cache: BTreeMap<(StaticOwnerId, MaterializationLocalId), Option<ScopeId>>,
    local_scope_visiting: BTreeSet<(StaticOwnerId, MaterializationLocalId)>,
    event_causes_cache: BTreeMap<ExecutableExprId, Vec<EventCause>>,
    source_candidates_cache:
        BTreeMap<ExecutableEventReference, Vec<(String, SourceId, Option<ScopeId>)>>,
    view_reads_cache: BTreeMap<ExecutableExprId, Vec<ExecutableViewRead>>,
    render_visited: BTreeSet<ExecutableExprId>,
    bindings: Vec<ViewBinding>,
}

impl<'a> ExecutableViewBindingCollector<'a> {
    fn new(
        executable: &'a ExecutableProgram,
        static_owners: &'a [StaticOwnerDef],
        storage: Option<&'a ErasedScopeIndex>,
        list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
        row_scopes: &'a [RowScope],
        sources: &'a [SourcePort],
        states: &[StateCell],
        materializations: &'a [ContextualMaterialization],
    ) -> Result<Self, String> {
        let statement_values = executable
            .statements
            .iter()
            .filter_map(|statement| Some((statement.declaration?, statement.value?)))
            .collect();
        let list_scopes_by_declaration = list_storage
            .iter()
            .filter_map(|(statement, storage)| {
                executable
                    .statements
                    .iter()
                    .find(|candidate| candidate.id == *statement)
                    .and_then(|statement| statement.declaration)
                    .map(|declaration| (declaration, storage.row_scope_id))
            })
            .collect();
        let mut list_scopes_by_path = BTreeMap::new();
        for storage in list_storage.values() {
            if let Some(previous) = list_scopes_by_path
                .insert(storage.path.clone(), storage.row_scope_id)
                && previous != storage.row_scope_id
            {
                return Err(format!(
                    "typed list path `{}` resolves to both row scopes {previous} and {}",
                    storage.path, storage.row_scope_id
                ));
            }
        }
        let materializations_by_local = materializations
            .iter()
            .map(|materialization| {
                (
                    (materialization.owner, materialization.row_local),
                    materialization.id,
                )
            })
            .collect();
        let states_by_declaration = states
            .iter()
            .filter(|state| state.published)
            .filter_map(|state| {
                let executable_state = state
                    .executable_state_id
                    .and_then(|id| executable.states.get(id.as_usize()))?;
                Some(((executable_state.declaration, state.static_owner), state.id))
            })
            .collect();
        let mut reads_by_expression = vec![None; executable.expressions.len()];
        if let Some(storage) = storage {
            for read in &storage.reads {
                if let Some(slot) = reads_by_expression.get_mut(read.expression.as_usize()) {
                    *slot = Some(read.id);
                }
            }
        }
        Ok(Self {
            executable,
            static_owners,
            storage,
            row_scopes,
            sources,
            materializations,
            statement_values,
            local_values_by_read: executable_local_read_values(executable)?,
            list_scopes_by_declaration,
            list_scopes_by_path,
            materializations_by_local,
            reads_by_expression,
            states_by_declaration,
            local_scope_cache: BTreeMap::new(),
            local_scope_visiting: BTreeSet::new(),
            event_causes_cache: BTreeMap::new(),
            source_candidates_cache: BTreeMap::new(),
            view_reads_cache: BTreeMap::new(),
            render_visited: BTreeSet::new(),
            bindings: Vec::new(),
        })
    }

    fn event_causes_for_expression(
        &mut self,
        root: ExecutableExprId,
    ) -> Result<Vec<EventCause>, String> {
        if let Some(cached) = self.event_causes_cache.get(&root) {
            return Ok(cached.clone());
        }
        let mut causes = BTreeSet::new();
        self.collect_event_causes(
            root,
            &mut causes,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
        )?;
        let causes = causes.into_iter().collect::<Vec<_>>();
        self.event_causes_cache.insert(root, causes.clone());
        Ok(causes)
    }

    fn trigger_owned_arms_for_statement(
        &mut self,
        statement: ExecutableStatementId,
    ) -> Result<(Vec<TriggerOwnedArm>, Vec<ExecutableExprId>), String> {
        let Some(root) = self
            .executable
            .statements
            .iter()
            .find(|candidate| candidate.id == statement)
            .and_then(|statement| statement.value)
        else {
            return Ok((Vec::new(), Vec::new()));
        };
        let root_expression = self.expression(root)?;
        let root_owns_event = matches!(
            root_expression.flow_type.mode,
            boon_typecheck::FlowMode::TickPresent | boon_typecheck::FlowMode::PresentOrAbsent
        ) || matches!(
            &root_expression.kind,
            ExecutableExpressionKind::Latest { .. } | ExecutableExpressionKind::Hold { .. }
        ) || matches!(
            &root_expression.kind,
            ExecutableExpressionKind::Call { name, .. } if name == "List/latest"
        );
        if !root_owns_event {
            return Ok((Vec::new(), Vec::new()));
        }
        let mut arms = Vec::new();
        self.collect_trigger_owned_arms(root, &mut BTreeSet::new(), &mut arms)?;
        if arms.is_empty() {
            for cause in self.event_causes_for_expression(root)? {
                self.insert_trigger_owned_arm(cause, root, root, &mut arms)?;
            }
        }
        let default_roots = match &self.expression(root)?.kind {
            ExecutableExpressionKind::Latest { branches } => {
                let mut defaults = Vec::new();
                for branch in branches.clone() {
                    if self.event_causes_for_expression(branch)?.is_empty() {
                        defaults.push(branch);
                    }
                }
                defaults
            }
            ExecutableExpressionKind::Hold { initial, .. } => vec![*initial],
            _ => Vec::new(),
        };
        Ok((arms, default_roots))
    }

    fn trigger_owned_arms_for_expression(
        &mut self,
        root: ExecutableExprId,
    ) -> Result<Vec<TriggerOwnedArm>, String> {
        let mut arms = Vec::new();
        self.collect_trigger_owned_arms(root, &mut BTreeSet::new(), &mut arms)?;
        if arms.is_empty() {
            for cause in self.event_causes_for_expression(root)? {
                self.insert_trigger_owned_arm(cause, root, root, &mut arms)?;
            }
        }
        Ok(arms)
    }

    fn trigger_owned_arms_before_expression(
        &mut self,
        root: ExecutableExprId,
        terminal: ExecutableExprId,
    ) -> Result<Vec<TriggerOwnedArm>, String> {
        let mut arms = Vec::new();
        self.collect_trigger_owned_arms_until(
            root,
            Some(terminal),
            &mut BTreeSet::new(),
            &mut arms,
        )?;
        Ok(arms)
    }

    fn collect_trigger_owned_arms(
        &mut self,
        id: ExecutableExprId,
        visited: &mut BTreeSet<ExecutableExprId>,
        arms: &mut Vec<TriggerOwnedArm>,
    ) -> Result<(), String> {
        self.collect_trigger_owned_arms_until(id, None, visited, arms)
    }

    fn collect_trigger_owned_arms_until(
        &mut self,
        id: ExecutableExprId,
        terminal: Option<ExecutableExprId>,
        visited: &mut BTreeSet<ExecutableExprId>,
        arms: &mut Vec<TriggerOwnedArm>,
    ) -> Result<(), String> {
        if terminal == Some(id) {
            return Ok(());
        }
        if !visited.insert(id) {
            return Ok(());
        }
        let expression = self.expression(id)?.clone();
        match expression.kind {
            ExecutableExpressionKind::When {
                input,
                arms: select_arms,
            } => {
                let causes = self.event_causes_for_expression(input)?;
                if !causes.is_empty() {
                    for cause in causes {
                        self.insert_trigger_owned_arm(cause, input, id, arms)?;
                    }
                    return Ok(());
                }
                for arm in select_arms {
                    self.collect_trigger_owned_arms_until(arm.output, terminal, visited, arms)?;
                }
            }
            ExecutableExpressionKind::Then { input, output } => {
                let causes = self.event_causes_for_expression(input)?;
                if !causes.is_empty() {
                    let output = output.unwrap_or(input);
                    for cause in causes {
                        self.insert_trigger_owned_arm(cause, input, output, arms)?;
                    }
                    return Ok(());
                }
                if let Some(output) = output {
                    self.collect_trigger_owned_arms_until(output, terminal, visited, arms)?;
                }
            }
            ExecutableExpressionKind::Hold { updates, .. }
            | ExecutableExpressionKind::Latest { branches: updates } => {
                for update in updates {
                    let mut update_arms = Vec::new();
                    self.collect_trigger_owned_arms_until(
                        update,
                        terminal,
                        &mut BTreeSet::new(),
                        &mut update_arms,
                    )?;
                    if update_arms.is_empty() {
                        for cause in self.event_causes_for_expression(update)? {
                            self.insert_trigger_owned_arm(cause, update, update, arms)?;
                        }
                    } else {
                        for arm in update_arms {
                            self.insert_trigger_owned_arm(
                                arm.cause,
                                arm.gate_expression_id,
                                arm.output_expression_id,
                                arms,
                            )?;
                        }
                    }
                }
            }
            ExecutableExpressionKind::Call {
                name, arguments, ..
            } => {
                let preserves_trigger_output = name == "List/latest";
                for argument in arguments {
                    let mut argument_arms = Vec::new();
                    self.collect_trigger_owned_arms_until(
                        argument.value,
                        terminal,
                        &mut BTreeSet::new(),
                        &mut argument_arms,
                    )?;
                    if argument_arms.is_empty()
                        && self.expression(argument.value).is_ok_and(|expression| {
                            matches!(
                                expression.flow_type.mode,
                                boon_typecheck::FlowMode::TickPresent
                                    | boon_typecheck::FlowMode::PresentOrAbsent
                            )
                        })
                    {
                        for cause in self.event_causes_for_expression(argument.value)? {
                            self.insert_trigger_owned_arm(
                                cause,
                                argument.value,
                                if preserves_trigger_output {
                                    argument.value
                                } else {
                                    id
                                },
                                arms,
                            )?;
                        }
                    } else {
                        for arm in argument_arms {
                            self.insert_trigger_owned_arm(
                                arm.cause,
                                arm.gate_expression_id,
                                if preserves_trigger_output {
                                    arm.output_expression_id
                                } else {
                                    id
                                },
                                arms,
                            )?;
                        }
                    }
                }
            }
            ExecutableExpressionKind::Materialize { materialization } => {
                let body = self.materialization(materialization)?.body;
                self.collect_trigger_owned_arms_until(body, terminal, visited, arms)?;
            }
            ExecutableExpressionKind::Draining { input }
            | ExecutableExpressionKind::Project { input, .. } => {
                self.collect_trigger_owned_arms_until(input, terminal, visited, arms)?;
            }
            ExecutableExpressionKind::Infix { left, right, .. } => {
                self.collect_trigger_owned_arms_until(left, terminal, visited, arms)?;
                self.collect_trigger_owned_arms_until(right, terminal, visited, arms)?;
            }
            ExecutableExpressionKind::MatchArm { output, .. } => {
                if let Some(output) = output {
                    self.collect_trigger_owned_arms_until(output, terminal, visited, arms)?;
                }
            }
            ExecutableExpressionKind::Object(fields)
            | ExecutableExpressionKind::Record(fields)
            | ExecutableExpressionKind::TaggedObject { fields, .. } => {
                for field in fields {
                    self.collect_trigger_owned_arms_until(field.value, terminal, visited, arms)?;
                }
            }
            ExecutableExpressionKind::Block { bindings, result } => {
                for binding in bindings {
                    self.collect_trigger_owned_arms_until(binding.value, terminal, visited, arms)?;
                }
                self.collect_trigger_owned_arms_until(result, terminal, visited, arms)?;
            }
            ExecutableExpressionKind::List { items, .. }
            | ExecutableExpressionKind::Bytes { items, .. } => {
                for item in items {
                    self.collect_trigger_owned_arms_until(item, terminal, visited, arms)?;
                }
            }
            ExecutableExpressionKind::TextTemplate { segments } => {
                for value in segments.into_iter().filter_map(|segment| match segment {
                    ExecutableTextSegment::Static { .. } => None,
                    ExecutableTextSegment::Dynamic { value } => Some(value),
                }) {
                    self.collect_trigger_owned_arms_until(value, terminal, visited, arms)?;
                }
            }
            ExecutableExpressionKind::CanonicalRead {
                target, projection, ..
            } => {
                if let Some(cause) = self
                    .direct_event_reference(id)?
                    .as_ref()
                    .and_then(|read| self.state_event_cause(read))
                {
                    self.insert_trigger_owned_arm(cause, id, id, arms)?;
                    return Ok(());
                }
                if let Some(producer) = self.statement_values.get(&target).copied()
                    && producer != id
                {
                    let mut producer_arms = Vec::new();
                    self.collect_trigger_owned_arms_until(
                        producer,
                        terminal,
                        &mut BTreeSet::new(),
                        &mut producer_arms,
                    )?;
                    if producer_arms.is_empty() {
                        for cause in self.event_causes_for_expression(producer)? {
                            self.insert_trigger_owned_arm(
                                cause,
                                producer,
                                if projection.is_empty() { producer } else { id },
                                arms,
                            )?;
                        }
                    } else {
                        for arm in producer_arms {
                            self.insert_trigger_owned_arm(
                                arm.cause,
                                arm.gate_expression_id,
                                if projection.is_empty() {
                                    arm.output_expression_id
                                } else {
                                    id
                                },
                                arms,
                            )?;
                        }
                    }
                }
            }
            ExecutableExpressionKind::LocalRead { projection, .. } => {
                if let Some(producer) = self.local_values_by_read.get(&id).copied()
                    && producer != id
                {
                    let mut producer_arms = Vec::new();
                    self.collect_trigger_owned_arms_until(
                        producer,
                        terminal,
                        &mut BTreeSet::new(),
                        &mut producer_arms,
                    )?;
                    if producer_arms.is_empty() {
                        for cause in self.event_causes_for_expression(producer)? {
                            self.insert_trigger_owned_arm(
                                cause,
                                producer,
                                if projection.is_empty() { producer } else { id },
                                arms,
                            )?;
                        }
                    } else {
                        for arm in producer_arms {
                            self.insert_trigger_owned_arm(
                                arm.cause,
                                arm.gate_expression_id,
                                if projection.is_empty() {
                                    arm.output_expression_id
                                } else {
                                    id
                                },
                                arms,
                            )?;
                        }
                    }
                }
            }
            ExecutableExpressionKind::ExternalRead { .. }
            | ExecutableExpressionKind::ElementState { .. }
            | ExecutableExpressionKind::Drain { .. }
            | ExecutableExpressionKind::Source { .. }
            | ExecutableExpressionKind::MaterializationLocal { .. }
            | ExecutableExpressionKind::FunctionParameter { .. }
            | ExecutableExpressionKind::Text(_)
            | ExecutableExpressionKind::Number(_)
            | ExecutableExpressionKind::BytesByte(_)
            | ExecutableExpressionKind::Bool(_)
            | ExecutableExpressionKind::Tag(_)
            | ExecutableExpressionKind::Delimiter => {}
        }
        Ok(())
    }

    fn insert_trigger_owned_arm(
        &self,
        cause: EventCause,
        gate: ExecutableExprId,
        output: ExecutableExprId,
        arms: &mut Vec<TriggerOwnedArm>,
    ) -> Result<(), String> {
        let gate_expression = self.expression(gate)?;
        if !arms.iter().any(|arm| {
            arm.cause == cause
                && arm.gate_expression_id == gate
                && arm.output_expression_id == output
        }) {
            arms.push(TriggerOwnedArm {
                cause,
                gate_checked_expr_id: gate_expression.checked_expr_id,
                gate_expression_id: gate,
                owner: gate_expression.owner,
                output_expression_id: output,
            });
        }
        Ok(())
    }

    fn collect_event_causes(
        &mut self,
        id: ExecutableExprId,
        causes: &mut BTreeSet<EventCause>,
        visited_expressions: &mut BTreeSet<ExecutableExprId>,
        visited_paths: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        if !visited_expressions.insert(id) {
            return Ok(());
        }
        if let Some(producer) = self.local_values_by_read.get(&id).copied()
            && producer != id
        {
            self.collect_event_causes(
                producer,
                causes,
                visited_expressions,
                visited_paths,
            )?;
            return Ok(());
        }
        if let Some(read) = self.direct_event_reference(id)? {
            let candidates = self.source_candidates(&read)?;
            if !candidates.is_empty() {
                causes.extend(
                    candidates
                        .into_iter()
                        .map(|(_, source_id, _)| EventCause::Source(source_id)),
                );
                return Ok(());
            }
            if let Some(cause) = self.state_event_cause(&read) {
                causes.insert(cause);
                return Ok(());
            }
            if let ExecutableEventReference::Canonical {
                declaration,
                owner: _,
                path: _,
                ..
            } = read
            {
                if let Some(declaration) = declaration
                    && visited_paths.insert(format!("decl:{}", declaration.0))
                    && let Some(value) = self.statement_values.get(&declaration).copied()
                    && value != id
                {
                    self.collect_event_causes(value, causes, visited_expressions, visited_paths)?;
                }
                return Ok(());
            }
        }

        let expression = self.expression(id)?.clone();
        let children = match expression.kind {
            ExecutableExpressionKind::Then { input, .. }
            | ExecutableExpressionKind::When { input, .. }
            | ExecutableExpressionKind::Draining { input }
            | ExecutableExpressionKind::Project { input, .. } => vec![input],
            ExecutableExpressionKind::Materialize { materialization } => {
                vec![self.materialization(materialization)?.body]
            }
            ExecutableExpressionKind::Hold { updates, .. }
            | ExecutableExpressionKind::Latest { branches: updates } => updates,
            ExecutableExpressionKind::Call { arguments, .. } => arguments
                .into_iter()
                .map(|argument| argument.value)
                .collect(),
            ExecutableExpressionKind::Infix { left, right, .. } => vec![left, right],
            ExecutableExpressionKind::MatchArm { output, .. } => output.into_iter().collect(),
            ExecutableExpressionKind::Object(fields)
            | ExecutableExpressionKind::Record(fields)
            | ExecutableExpressionKind::TaggedObject { fields, .. } => {
                fields.into_iter().map(|field| field.value).collect()
            }
            ExecutableExpressionKind::Block { bindings, result } => bindings
                .into_iter()
                .map(|binding| binding.value)
                .chain(std::iter::once(result))
                .collect(),
            ExecutableExpressionKind::List { items, .. }
            | ExecutableExpressionKind::Bytes { items, .. } => items,
            ExecutableExpressionKind::TextTemplate { segments } => segments
                .into_iter()
                .filter_map(|segment| match segment {
                    ExecutableTextSegment::Static { .. } => None,
                    ExecutableTextSegment::Dynamic { value } => Some(value),
                })
                .collect(),
            ExecutableExpressionKind::ExternalRead { canonical_path } => {
                let source_path = distributed_event_source_path(&canonical_path);
                if let Some(source) = self
                    .sources
                    .iter()
                    .find(|source| source.path == source_path)
                {
                    causes.insert(EventCause::Source(source.id));
                }
                Vec::new()
            }
            ExecutableExpressionKind::CanonicalRead { .. }
            | ExecutableExpressionKind::LocalRead { .. }
            | ExecutableExpressionKind::ElementState { .. }
            | ExecutableExpressionKind::Drain { .. }
            | ExecutableExpressionKind::Source { .. }
            | ExecutableExpressionKind::MaterializationLocal { .. }
            | ExecutableExpressionKind::FunctionParameter { .. }
            | ExecutableExpressionKind::Text(_)
            | ExecutableExpressionKind::Number(_)
            | ExecutableExpressionKind::BytesByte(_)
            | ExecutableExpressionKind::Bool(_)
            | ExecutableExpressionKind::Tag(_)
            | ExecutableExpressionKind::Delimiter => Vec::new(),
        };
        for child in children {
            self.collect_event_causes(child, causes, visited_expressions, visited_paths)?;
        }
        Ok(())
    }

    fn state_event_cause(&self, read: &ExecutableEventReference) -> Option<EventCause> {
        let ExecutableEventReference::Canonical {
            declaration: Some(declaration),
            owner,
            ..
        } = read
        else {
            return None;
        };
        self.states_by_declaration
            .get(&(*declaration, *owner))
            .or_else(|| self.states_by_declaration.get(&(*declaration, None)))
            .copied()
            .map(EventCause::State)
    }

    fn collect_output_roots(&mut self, outputs: &[OutputRootValue]) -> Result<(), String> {
        let mut roots = BTreeSet::new();
        for output in outputs {
            if !matches!(
                output.contract,
                SemanticOutputContractKind::RetainedVisual { .. }
            ) {
                continue;
            }
            roots.insert(output.value_expression_id);
        }
        for statement in &self.executable.statements {
            let Some(value) = statement.value else {
                continue;
            };
            if self
                .expression(value)
                .is_ok_and(|expression| retained_visual_type(&expression.flow_type.ty))
            {
                roots.insert(value);
            }
        }
        for root in roots {
            self.collect_render(root)?;
        }
        Ok(())
    }

    fn expression(&self, id: ExecutableExprId) -> Result<&ExecutableExpression, String> {
        self.executable
            .expressions
            .get(id.as_usize())
            .filter(|expression| expression.id == id)
            .ok_or_else(|| format!("view binding reaches missing executable expression {id}"))
    }

    fn materialization(&self, id: usize) -> Result<&ContextualMaterialization, String> {
        self.materializations
            .get(id)
            .filter(|materialization| materialization.id == id)
            .ok_or_else(|| format!("view binding reaches missing materialization {id}"))
    }

    fn collect_render(&mut self, id: ExecutableExprId) -> Result<(), String> {
        if !self.render_visited.insert(id) {
            return Ok(());
        }
        let expression = self.expression(id)?.clone();
        match expression.kind {
            ExecutableExpressionKind::Call {
                name, arguments, ..
            } => {
                if let Some(node_kind) = executable_view_node_kind(&name) {
                    self.collect_element_call(&name, &node_kind, &arguments)?;
                }
                for argument in arguments {
                    if render_argument_can_contain_nodes(&name, &argument.name) {
                        self.collect_render(argument.value)?;
                    }
                }
            }
            ExecutableExpressionKind::Materialize { materialization } => {
                let body = self.materialization(materialization)?.body;
                self.collect_render(body)?;
            }
            ExecutableExpressionKind::CanonicalRead { target, .. } => {
                if let Some(value) = self.statement_values.get(&target).copied()
                    && value != id
                {
                    self.collect_render(value)?;
                }
            }
            ExecutableExpressionKind::ExternalRead { .. } => {}
            ExecutableExpressionKind::ElementState { .. } => {}
            ExecutableExpressionKind::Object(fields) | ExecutableExpressionKind::Record(fields) => {
                self.collect_record_render(&fields)?;
            }
            ExecutableExpressionKind::TaggedObject { fields, .. } => {
                self.collect_record_render(&fields)?;
            }
            ExecutableExpressionKind::List { items, .. } => {
                for item in items {
                    self.collect_render(item)?;
                }
            }
            ExecutableExpressionKind::Block { bindings, result } => {
                for binding in bindings {
                    self.collect_render(binding.value)?;
                }
                self.collect_render(result)?;
            }
            ExecutableExpressionKind::Project { input, .. }
            | ExecutableExpressionKind::Draining { input } => self.collect_render(input)?,
            ExecutableExpressionKind::Hold {
                initial, updates, ..
            } => {
                self.collect_render(initial)?;
                for update in updates {
                    self.collect_render(update)?;
                }
            }
            ExecutableExpressionKind::Latest { branches } => {
                for branch in branches {
                    self.collect_render(branch)?;
                }
            }
            ExecutableExpressionKind::When { input, arms } => {
                self.collect_render(input)?;
                for arm in arms {
                    self.collect_render(arm.output)?;
                }
            }
            ExecutableExpressionKind::Then { input, output } => {
                self.collect_render(input)?;
                if let Some(output) = output {
                    self.collect_render(output)?;
                }
            }
            ExecutableExpressionKind::Infix { left, right, .. } => {
                self.collect_render(left)?;
                self.collect_render(right)?;
            }
            ExecutableExpressionKind::MatchArm { output, .. } => {
                if let Some(output) = output {
                    self.collect_render(output)?;
                }
            }
            ExecutableExpressionKind::Drain { .. }
            | ExecutableExpressionKind::LocalRead { .. }
            | ExecutableExpressionKind::TextTemplate { .. }
            | ExecutableExpressionKind::Text(_)
            | ExecutableExpressionKind::Number(_)
            | ExecutableExpressionKind::BytesByte(_)
            | ExecutableExpressionKind::Bool(_)
            | ExecutableExpressionKind::Tag(_)
            | ExecutableExpressionKind::Source { .. }
            | ExecutableExpressionKind::Bytes { .. }
            | ExecutableExpressionKind::Delimiter
            | ExecutableExpressionKind::MaterializationLocal { .. }
            | ExecutableExpressionKind::FunctionParameter { .. } => {}
        }
        Ok(())
    }

    fn collect_element_call(
        &mut self,
        constructor: &str,
        node_kind: &str,
        arguments: &[ExecutableCallArgument],
    ) -> Result<(), String> {
        if let Some(element) = call_argument(arguments, "element") {
            let mut event_values = Vec::new();
            self.collect_named_field_values(
                element,
                "events",
                &mut event_values,
                &mut BTreeSet::new(),
            )?;
            if !event_values.is_empty() {
                let mut source_count = 0;
                for events in &event_values {
                    source_count +=
                        self.collect_event_tree(node_kind, *events, None, &mut BTreeSet::new())?;
                }
                if source_count == 0 {
                    return Err(format!(
                        "Element constructor `{constructor}` has `element.events` with no concrete SOURCE leaves"
                    ));
                }
            }

            let mut targets = Vec::new();
            self.collect_named_field_values(element, "target", &mut targets, &mut BTreeSet::new())?;
            for target in targets {
                self.collect_data_bindings(node_kind, "target", target, ViewBindingKind::Target)?;
            }
        }

        if let Some(style) = call_argument(arguments, "style") {
            self.collect_style_bindings(node_kind, style, &mut BTreeSet::new())?;
        }
        for argument in arguments {
            if argument.name != "element"
                && argument.name != "style"
                && attr_can_bind_data(&argument.name)
            {
                self.collect_data_bindings(
                    node_kind,
                    &argument.name,
                    argument.value,
                    if argument.name == "target" {
                        ViewBindingKind::Target
                    } else {
                        ViewBindingKind::Data
                    },
                )?;
            }
        }
        Ok(())
    }

    fn collect_record_render(&mut self, fields: &[ExecutableRecordField]) -> Result<(), String> {
        let node_kind = fields
            .iter()
            .find(|field| !field.spread && field.name == "kind")
            .and_then(|field| self.static_tag(field.value));
        if let Some(node_kind) = node_kind
            && !matches!(node_kind.as_str(), "Document" | "Scene")
        {
            let mut event_count = 0;
            let mut has_events = false;
            for field in fields {
                if field.spread {
                    continue;
                }
                if field.name == "events" {
                    has_events = true;
                    event_count += self.collect_event_tree(
                        &node_kind,
                        field.value,
                        None,
                        &mut BTreeSet::new(),
                    )?;
                } else if field.name != "kind" {
                    let direct_sources =
                        self.push_source_bindings(&node_kind, Some(&field.name), field.value)?;
                    if direct_sources == 0 && attr_can_bind_data(&field.name) {
                        self.collect_data_bindings(
                            &node_kind,
                            &field.name,
                            field.value,
                            if field.name == "target" {
                                ViewBindingKind::Target
                            } else {
                                ViewBindingKind::Data
                            },
                        )?;
                    }
                }
            }
            if has_events && event_count == 0 {
                return Err(format!(
                    "retained `{node_kind}` element has `events` with no concrete SOURCE leaves"
                ));
            }
        }
        for field in fields {
            if field.spread || render_record_field_can_contain_nodes(&field.name) {
                self.collect_render(field.value)?;
            }
        }
        Ok(())
    }

    fn collect_named_field_values(
        &self,
        id: ExecutableExprId,
        target: &str,
        values: &mut Vec<ExecutableExprId>,
        visited: &mut BTreeSet<ExecutableExprId>,
    ) -> Result<(), String> {
        if !visited.insert(id) {
            return Ok(());
        }
        let expression = self.expression(id)?;
        match &expression.kind {
            ExecutableExpressionKind::Object(fields)
            | ExecutableExpressionKind::Record(fields)
            | ExecutableExpressionKind::TaggedObject { fields, .. } => {
                for field in fields {
                    if !field.spread && field.name == target {
                        values.push(field.value);
                    } else if field.spread {
                        self.collect_named_field_values(field.value, target, values, visited)?;
                    }
                }
            }
            ExecutableExpressionKind::List { items, .. } => {
                for item in items {
                    self.collect_named_field_values(*item, target, values, visited)?;
                }
            }
            ExecutableExpressionKind::CanonicalRead {
                target: declaration,
                ..
            } => {
                if let Some(value) = self.statement_values.get(declaration).copied()
                    && value != id
                {
                    self.collect_named_field_values(value, target, values, visited)?;
                }
            }
            ExecutableExpressionKind::Project { input, .. }
            | ExecutableExpressionKind::Draining { input } => {
                self.collect_named_field_values(*input, target, values, visited)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn collect_event_tree(
        &mut self,
        node_kind: &str,
        id: ExecutableExprId,
        attr: Option<&str>,
        visited: &mut BTreeSet<ExecutableExprId>,
    ) -> Result<usize, String> {
        if !visited.insert(id) {
            return Ok(0);
        }
        let direct = self.push_source_bindings(node_kind, attr, id)?;
        if direct > 0 {
            return Ok(direct);
        }
        let expression = self.expression(id)?.clone();
        let mut count = 0;
        match expression.kind {
            ExecutableExpressionKind::Object(fields)
            | ExecutableExpressionKind::Record(fields)
            | ExecutableExpressionKind::TaggedObject { fields, .. } => {
                for field in fields {
                    count += self.collect_event_tree(
                        node_kind,
                        field.value,
                        (!field.spread).then_some(field.name.as_str()).or(attr),
                        visited,
                    )?;
                }
            }
            ExecutableExpressionKind::List { items, .. } => {
                for item in items {
                    count += self.collect_event_tree(node_kind, item, attr, visited)?;
                }
            }
            ExecutableExpressionKind::CanonicalRead { target, .. } => {
                if let Some(value) = self.statement_values.get(&target).copied()
                    && value != id
                {
                    count += self.collect_event_tree(node_kind, value, attr, visited)?;
                }
            }
            kind => {
                for child in executable_expression_children(&kind) {
                    count += self.collect_event_tree(node_kind, child, attr, visited)?;
                }
            }
        }
        Ok(count)
    }

    fn push_source_bindings(
        &mut self,
        node_kind: &str,
        attr: Option<&str>,
        id: ExecutableExprId,
    ) -> Result<usize, String> {
        let Some(read) = self.direct_event_reference(id)? else {
            return Ok(0);
        };
        let candidates = self.source_candidates(&read)?;
        let multiple = candidates.len() > 1;
        for (path, source_id, scope_id) in &candidates {
            let source_attr = if multiple || attr.is_none() {
                path.rsplit('.').next().unwrap_or("event")
            } else {
                attr.expect("checked above")
            };
            self.bindings.push(ViewBinding {
                id: ViewBindingId(self.bindings.len()),
                node_kind: node_kind.to_owned(),
                attr: canonical_event_attr(source_attr).to_owned(),
                path: path.clone(),
                target: ViewBindingTarget::Source { source: *source_id },
                kind: ViewBindingKind::Source,
                scope_id: *scope_id,
            });
        }
        Ok(candidates.len())
    }

    fn source_candidates(
        &mut self,
        read: &ExecutableEventReference,
    ) -> Result<Vec<(String, SourceId, Option<ScopeId>)>, String> {
        if let Some(cached) = self.source_candidates_cache.get(read) {
            return Ok(cached.clone());
        }
        let candidates = self.resolve_source_candidates(read)?;
        self.source_candidates_cache
            .insert(read.clone(), candidates.clone());
        Ok(candidates)
    }

    fn resolve_source_candidates(
        &mut self,
        read: &ExecutableEventReference,
    ) -> Result<Vec<(String, SourceId, Option<ScopeId>)>, String> {
        let mut exact = Vec::new();
        let mut grouped = Vec::new();
        match read {
            ExecutableEventReference::Canonical {
                declaration,
                owner,
                path,
                projection,
                expression,
                source_expression,
            } => {
                if let Some(source_id) = self.exact_source_id_for_expression(*expression)? {
                    let source = self
                        .sources
                        .get(source_id.as_usize())
                        .filter(|source| source.id == source_id)
                        .ok_or_else(|| {
                            format!("event reference points to missing source {source_id}")
                        })?;
                    return Ok(vec![(source.path.clone(), source.id, source.scope_id)]);
                }
                if let Some(declaration) = declaration {
                    let owner_sources = self
                        .sources
                        .iter()
                        .filter(|source| {
                            source
                                .executable_source_id
                                .and_then(|id| self.executable.sources.get(id.as_usize()))
                                .is_some_and(|definition| {
                                    definition.declaration == *declaration
                                        && source_expression.as_ref().is_none_or(
                                            |source_expression| {
                                                self.executable
                                                    .expressions
                                                    .get(definition.expression.as_usize())
                                                    .is_some_and(|expression| {
                                                        expression.id == definition.expression
                                                            && expression.checked_expr_id
                                                                == *source_expression
                                                    })
                                            },
                                        )
                                })
                        })
                        .collect::<Vec<_>>();
                    if !owner_sources.is_empty() {
                        for lexical_owner in [*owner, None] {
                            let candidates = owner_sources
                                .iter()
                                .filter(|source| source.static_owner == lexical_owner)
                                .map(|source| (source.path.clone(), source.id, source.scope_id))
                                .collect::<Vec<_>>();
                            match candidates.as_slice() {
                                [] => continue,
                                [_] => return Ok(candidates),
                                _ => {
                                    return Err(format!(
                                        "SOURCE declaration {} owner {:?} resolves to {} executable sources",
                                        declaration.0,
                                        lexical_owner,
                                        candidates.len()
                                    ));
                                }
                            }
                        }
                    }
                    if let Some(source_expression) = source_expression {
                        return Err(format!(
                            "checked SOURCE expression {} declaration {} read `{}` owner {:?} has no executable source",
                            source_expression.0,
                            declaration.0,
                            canonical_read_path(path, projection),
                            owner
                        ));
                    }
                }
                let path = canonical_read_path(path, projection);
                let distributed_path = distributed_event_source_path(&path);
                let paths = [path, distributed_path];
                let mut projected = Vec::new();
                for source in self.sources {
                    let candidate = (source.path.clone(), source.id, source.scope_id);
                    if paths.iter().any(|path| source.path == *path) {
                        exact.push(candidate);
                    } else if paths.iter().any(|path| {
                        path.strip_prefix(&source.path)
                            .is_some_and(|suffix| suffix.starts_with('.'))
                    }) {
                        projected.push(candidate);
                    } else if paths
                        .iter()
                        .any(|path| source.path.starts_with(&format!("{path}.")))
                    {
                        grouped.push(candidate);
                    }
                }
                if !projected.is_empty() {
                    return Ok(projected);
                }
            }
            ExecutableEventReference::Local {
                owner,
                local,
                projection,
                ..
            } => {
                let projection = projection.join(".");
                let prefix = format!("{projection}.");
                self.local_scope(*owner, *local)?;
                let mut pending = vec![*owner];
                let mut visited = BTreeSet::new();
                while let Some(candidate_owner) = pending.pop() {
                    if !visited.insert(candidate_owner) {
                        continue;
                    }
                    let mut owner_exact = Vec::new();
                    let mut owner_grouped = Vec::new();
                    for source in self.sources.iter().filter(|source| {
                        source.static_owner.is_some_and(|source_owner| {
                            self.owner_is_descendant_or_same(source_owner, candidate_owner)
                        })
                    }) {
                        let binding = source
                            .executable_source_id
                            .and_then(|id| self.executable.sources.get(id.as_usize()))
                            .filter(|definition| {
                                definition.id == source.executable_source_id.expect("checked above")
                            })
                            .map(|definition| definition.binding_path.as_str())
                            .ok_or_else(|| {
                                format!(
                                    "source {} has no exact executable source definition",
                                    source.id
                                )
                            })?;
                        let candidate = (source.path.clone(), source.id, source.scope_id);
                        if binding == projection {
                            owner_exact.push(candidate);
                        } else if projection
                            .strip_prefix(binding)
                            .is_some_and(|suffix| suffix.starts_with('.'))
                        {
                            owner_exact.push(candidate);
                        } else if projection.is_empty() || binding.starts_with(&prefix) {
                            owner_grouped.push(candidate);
                        }
                    }
                    if !owner_exact.is_empty() {
                        return Ok(owner_exact);
                    }
                    if !owner_grouped.is_empty() {
                        return Ok(owner_grouped);
                    }
                    pending.extend(self.materialization_predecessor_owners(candidate_owner)?);
                }
            }
        }
        if exact.is_empty() {
            Ok(grouped)
        } else {
            Ok(exact)
        }
    }

    fn owner_is_descendant_or_same(
        &self,
        mut candidate: StaticOwnerId,
        ancestor: StaticOwnerId,
    ) -> bool {
        loop {
            if candidate == ancestor {
                return true;
            }
            let Some(parent) = self
                .static_owners
                .get(candidate.as_usize())
                .filter(|owner| owner.id == candidate)
                .and_then(|owner| owner.parent)
            else {
                return false;
            };
            candidate = parent;
        }
    }

    fn exact_source_id_for_expression(
        &self,
        expression: ExecutableExprId,
    ) -> Result<Option<SourceId>, String> {
        let definitions = self
            .executable
            .sources
            .iter()
            .filter(|source| source.expression == expression)
            .collect::<Vec<_>>();
        if definitions.len() > 1 {
            return Err(format!(
                "executable expression {expression} owns {} source definitions",
                definitions.len()
            ));
        }
        if let Some(definition) = definitions.first() {
            let sources = self
                .sources
                .iter()
                .filter(|source| source.executable_source_id == Some(definition.id))
                .collect::<Vec<_>>();
            return match sources.as_slice() {
                [source] => Ok(Some(source.id)),
                _ => Err(format!(
                    "executable source {} has {} runtime sources",
                    definition.id,
                    sources.len()
                )),
            };
        }

        let Some(storage) = self.storage else {
            return Ok(None);
        };
        let Some(read_id) = self
            .reads_by_expression
            .get(expression.as_usize())
            .copied()
            .flatten()
        else {
            return Ok(None);
        };
        let read = storage
            .reads
            .get(read_id.as_usize())
            .filter(|read| read.id == read_id && read.expression == expression)
            .ok_or_else(|| {
                format!(
                    "event expression {expression} references inconsistent erased read {read_id}"
                )
            })?;
        match read.target {
            ErasedReadTarget::Binding { binding, .. } => {
                let binding = storage
                    .bindings
                    .get(binding.as_usize())
                    .filter(|candidate| candidate.id == binding)
                    .ok_or_else(|| {
                        format!("event erased read {read_id} references missing {binding}")
                    })?;
                match binding.target {
                    ErasedBindingTarget::Source { runtime, .. } => Ok(Some(runtime)),
                    ErasedBindingTarget::Value { .. } | ErasedBindingTarget::State { .. } => {
                        Ok(None)
                    }
                }
            }
            ErasedReadTarget::SourcePayload { source, .. } => Ok(Some(source)),
            ErasedReadTarget::StateProjection { .. }
            | ErasedReadTarget::Expression { .. }
            | ErasedReadTarget::Local { .. }
            | ErasedReadTarget::ExternalValue { .. }
            | ErasedReadTarget::MaterializationLocal { .. }
            | ErasedReadTarget::FunctionParameter { .. } => Ok(None),
        }
    }

    fn materialization_predecessor_owners(
        &self,
        owner: StaticOwnerId,
    ) -> Result<Vec<StaticOwnerId>, String> {
        let materialization = self
            .materializations
            .iter()
            .find(|materialization| materialization.owner == owner)
            .ok_or_else(|| format!("static owner {owner} has no contextual materialization"))?;
        let mut owners = BTreeSet::new();
        self.collect_expression_owners(
            materialization.source,
            &mut owners,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
        )?;
        owners.remove(&owner);
        Ok(owners.into_iter().collect())
    }

    fn collect_expression_owners(
        &self,
        id: ExecutableExprId,
        owners: &mut BTreeSet<StaticOwnerId>,
        visited_expressions: &mut BTreeSet<ExecutableExprId>,
        visited_paths: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        if !visited_expressions.insert(id) {
            return Ok(());
        }
        let expression = self.expression(id)?;
        match &expression.kind {
            ExecutableExpressionKind::Materialize { materialization } => {
                owners.insert(self.materialization(*materialization)?.owner);
            }
            ExecutableExpressionKind::MaterializationLocal { owner, .. } => {
                owners.insert(*owner);
            }
            ExecutableExpressionKind::CanonicalRead { target, .. } => {
                if visited_paths.insert(format!("decl:{}", target.0))
                    && let Some(value) = self.statement_values.get(target).copied()
                {
                    self.collect_expression_owners(
                        value,
                        owners,
                        visited_expressions,
                        visited_paths,
                    )?;
                }
            }
            kind => {
                for child in executable_expression_children(kind) {
                    self.collect_expression_owners(
                        child,
                        owners,
                        visited_expressions,
                        visited_paths,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn collect_style_bindings(
        &mut self,
        node_kind: &str,
        id: ExecutableExprId,
        visited: &mut BTreeSet<ExecutableExprId>,
    ) -> Result<(), String> {
        if !visited.insert(id) {
            return Ok(());
        }
        let expression = self.expression(id)?.clone();
        match expression.kind {
            ExecutableExpressionKind::Object(fields)
            | ExecutableExpressionKind::Record(fields)
            | ExecutableExpressionKind::TaggedObject { fields, .. } => {
                for field in fields {
                    if field.spread {
                        self.collect_style_bindings(node_kind, field.value, visited)?;
                    } else {
                        self.collect_data_bindings(
                            node_kind,
                            &field.name,
                            field.value,
                            ViewBindingKind::Data,
                        )?;
                        self.collect_style_bindings(node_kind, field.value, visited)?;
                    }
                }
            }
            ExecutableExpressionKind::List { items, .. } => {
                for item in items {
                    self.collect_style_bindings(node_kind, item, visited)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn collect_data_bindings(
        &mut self,
        node_kind: &str,
        attr: &str,
        id: ExecutableExprId,
        kind: ViewBindingKind,
    ) -> Result<(), String> {
        for read in self.view_reads_for_expression(id, &mut BTreeSet::new())? {
            let (path, scope_id, target) = self.view_read_binding(&read)?;
            self.bindings.push(ViewBinding {
                id: ViewBindingId(self.bindings.len()),
                node_kind: node_kind.to_owned(),
                attr: attr.to_owned(),
                path,
                target,
                kind,
                scope_id,
            });
        }
        Ok(())
    }

    fn view_reads_for_expression(
        &mut self,
        id: ExecutableExprId,
        visiting: &mut BTreeSet<ExecutableExprId>,
    ) -> Result<Vec<ExecutableViewRead>, String> {
        if let Some(cached) = self.view_reads_cache.get(&id) {
            return Ok(cached.clone());
        }
        if !visiting.insert(id) {
            return Ok(Vec::new());
        }
        let mut reads = BTreeSet::new();
        if let Some(read) = self.direct_view_read(id)? {
            reads.insert(read);
        } else {
            let expression = self.expression(id)?.clone();
            if !matches!(
                expression.kind,
                ExecutableExpressionKind::Materialize { .. }
            ) {
                for child in executable_expression_children(&expression.kind) {
                    reads.extend(self.view_reads_for_expression(child, visiting)?);
                }
            }
        }
        visiting.remove(&id);
        let reads = reads.into_iter().collect::<Vec<_>>();
        self.view_reads_cache.insert(id, reads.clone());
        Ok(reads)
    }

    fn direct_view_read(
        &mut self,
        id: ExecutableExprId,
    ) -> Result<Option<ExecutableViewRead>, String> {
        let expression = self.expression(id)?.clone();
        let is_drain = matches!(&expression.kind, ExecutableExpressionKind::Drain { .. });
        match expression.kind {
            ExecutableExpressionKind::CanonicalRead {
                target,
                path,
                projection,
                ..
            }
            | ExecutableExpressionKind::Drain {
                target,
                path,
                projection,
            } => {
                let read = self.exact_view_read(id)?;
                if matches!(
                    self.erased_read(read)?.target,
                    ErasedReadTarget::Expression { .. }
                ) && is_drain
                {
                    return Err(format!(
                        "DRAIN view read {id} targets a transient expression"
                    ));
                }
                let diagnostic_path = canonical_read_path(&path, &projection);
                let scope_id = self
                    .list_scopes_by_declaration
                    .get(&target)
                    .copied()
                    .or_else(|| scope_id_for_path(self.row_scopes, &diagnostic_path));
                Ok(Some(ExecutableViewRead {
                    read,
                    additional_projection: Vec::new(),
                    diagnostic_path,
                    scope_id,
                }))
            }
            ExecutableExpressionKind::ExternalRead { canonical_path } => {
                Ok(Some(ExecutableViewRead {
                    read: self.exact_view_read(id)?,
                    additional_projection: Vec::new(),
                    diagnostic_path: canonical_path,
                    scope_id: None,
                }))
            }
            ExecutableExpressionKind::MaterializationLocal {
                owner,
                local,
                projection,
            } => {
                if projection.is_empty() {
                    return Ok(None);
                }
                let Some(scope_id) = self.local_scope(owner, local)? else {
                    return Ok(None);
                };
                let row_scope = self
                    .row_scopes
                    .get(scope_id.as_usize())
                    .ok_or_else(|| format!("view local references missing ScopeId {scope_id}"))?;
                Ok(Some(ExecutableViewRead {
                    read: self.exact_view_read(id)?,
                    additional_projection: Vec::new(),
                    diagnostic_path: format!("{}.{}", row_scope.list, projection.join(".")),
                    scope_id: Some(scope_id),
                }))
            }
            ExecutableExpressionKind::Project { input, fields } => {
                let Some(mut read) = self.direct_view_read(input)? else {
                    return Ok(None);
                };
                read.additional_projection.extend(fields.iter().cloned());
                if !fields.is_empty() {
                    read.diagnostic_path.push('.');
                    read.diagnostic_path.push_str(&fields.join("."));
                }
                Ok(Some(read))
            }
            _ => Ok(None),
        }
    }

    fn exact_view_read(&self, expression: ExecutableExprId) -> Result<ErasedReadId, String> {
        let read = self
            .reads_by_expression
            .get(expression.as_usize())
            .copied()
            .flatten()
            .ok_or_else(|| format!("view expression {expression} has no exact erased read"))?;
        self.erased_read(read)?;
        Ok(read)
    }

    fn erased_read(&self, read: ErasedReadId) -> Result<&ErasedReadBinding, String> {
        self.storage
            .and_then(|storage| storage.reads.get(read.as_usize()))
            .filter(|candidate| candidate.id == read)
            .ok_or_else(|| format!("view references missing erased read {read}"))
    }

    fn direct_event_reference(
        &self,
        id: ExecutableExprId,
    ) -> Result<Option<ExecutableEventReference>, String> {
        let expression = self.expression(id)?;
        let mut source_definitions = self
            .executable
            .sources
            .iter()
            .filter(|source| source.expression == id);
        if let Some(source) = source_definitions.next() {
            if source_definitions.next().is_some() {
                return Err(format!(
                    "executable expression {id} owns more than one source definition"
                ));
            }
            return Ok(Some(ExecutableEventReference::Canonical {
                declaration: Some(source.declaration),
                owner: source.owner,
                path: source.binding_path.clone(),
                projection: Vec::new(),
                expression: id,
                source_expression: Some(expression.checked_expr_id),
            }));
        }
        match &expression.kind {
            ExecutableExpressionKind::CanonicalRead {
                target,
                path,
                projection,
                source,
            } => Ok(Some(ExecutableEventReference::Canonical {
                declaration: Some(*target),
                owner: expression.owner,
                path: path.clone(),
                projection: projection.clone(),
                expression: id,
                source_expression: source.as_ref().map(|source| source.expression),
            })),
            ExecutableExpressionKind::Drain {
                target,
                path,
                projection,
            } => Ok(Some(ExecutableEventReference::Canonical {
                declaration: Some(*target),
                owner: expression.owner,
                path: path.clone(),
                projection: projection.clone(),
                expression: id,
                source_expression: None,
            })),
            ExecutableExpressionKind::ExternalRead { canonical_path } => {
                Ok(Some(ExecutableEventReference::Canonical {
                    declaration: None,
                    owner: expression.owner,
                    path: canonical_path.clone(),
                    projection: Vec::new(),
                    expression: id,
                    source_expression: None,
                }))
            }
            ExecutableExpressionKind::MaterializationLocal {
                owner,
                local,
                projection,
            } => Ok(Some(ExecutableEventReference::Local {
                owner: *owner,
                local: *local,
                projection: projection.clone(),
            })),
            ExecutableExpressionKind::Project { input, fields } => {
                let Some(mut read) = self.direct_event_reference(*input)? else {
                    return Ok(None);
                };
                match &mut read {
                    ExecutableEventReference::Canonical { projection, .. }
                    | ExecutableEventReference::Local { projection, .. } => {
                        projection.extend(fields.iter().cloned());
                    }
                }
                Ok(Some(read))
            }
            _ => Ok(None),
        }
    }

    fn view_read_binding(
        &self,
        read: &ExecutableViewRead,
    ) -> Result<(String, Option<ScopeId>, ViewBindingTarget), String> {
        self.erased_read(read.read)?;
        Ok((
            read.diagnostic_path.clone(),
            read.scope_id,
            ViewBindingTarget::Read {
                read: read.read,
                additional_projection: read.additional_projection.clone(),
            },
        ))
    }

    fn local_scope(
        &mut self,
        owner: StaticOwnerId,
        local: MaterializationLocalId,
    ) -> Result<Option<ScopeId>, String> {
        let key = (owner, local);
        if let Some(cached) = self.local_scope_cache.get(&key).copied() {
            return Ok(cached);
        }
        if !self.local_scope_visiting.insert(key) {
            return Err(format!(
                "contextual view owner {owner} local {:?} forms a storage-scope cycle",
                local
            ));
        }
        let result = if let Some(materialization) = self
            .materializations_by_local
            .get(&key)
            .copied()
            .map(|id| self.materialization(id))
            .transpose()?
        {
            self.storage_scope_for_expression(
                materialization.source,
                &mut BTreeSet::new(),
                &mut BTreeSet::new(),
            )?
        } else {
            return Err(format!(
                "contextual view owner {owner} local {:?} has no typed materialization",
                local
            ));
        };
        self.local_scope_visiting.remove(&key);
        self.local_scope_cache.insert(key, result);
        Ok(result)
    }

    fn storage_scope_for_expression(
        &mut self,
        id: ExecutableExprId,
        visited_expressions: &mut BTreeSet<ExecutableExprId>,
        visited_paths: &mut BTreeSet<String>,
    ) -> Result<Option<ScopeId>, String> {
        if !visited_expressions.insert(id) {
            return Ok(None);
        }
        let expression = self.expression(id)?.clone();
        match expression.kind {
            ExecutableExpressionKind::CanonicalRead {
                target,
                path,
                projection,
                ..
            }
            | ExecutableExpressionKind::Drain {
                target,
                path,
                projection,
            } => {
                let projected_path = canonical_read_path(&path, &projection);
                if let Some(scope) = self.list_scopes_by_path.get(&projected_path).copied() {
                    return Ok(Some(scope));
                }
                if let Some(scope) = self.list_scopes_by_declaration.get(&target).copied() {
                    return Ok(Some(scope));
                }
                if visited_paths.insert(format!("decl:{}", target.0))
                    && let Some(value) = self.statement_values.get(&target).copied()
                    && value != id
                {
                    return self.storage_scope_for_expression(
                        value,
                        visited_expressions,
                        visited_paths,
                    );
                }
                Ok(None)
            }
            ExecutableExpressionKind::Materialize { materialization } => {
                let source = self.materialization(materialization)?.source;
                self.storage_scope_for_expression(source, visited_expressions, visited_paths)
            }
            ExecutableExpressionKind::MaterializationLocal {
                owner,
                local,
                projection,
            } => {
                let scope = self.local_scope(owner, local)?;
                match projection.first().map(String::as_str) {
                    None => Ok(scope),
                    Some("items") => match scope {
                        Some(scope) => self.chunk_items_storage_scope_for_scope(scope),
                        None => Ok(None),
                    },
                    Some(_) => Ok(None),
                }
            }
            ExecutableExpressionKind::Project { input, fields } => {
                match fields.first().map(String::as_str) {
                    None => {
                        self.storage_scope_for_expression(input, visited_expressions, visited_paths)
                    }
                    Some("items") => Ok(self.chunk_items_storage_scope(input)?),
                    Some(_) => Ok(None),
                }
            }
            ExecutableExpressionKind::Draining { input } => {
                self.storage_scope_for_expression(input, visited_expressions, visited_paths)
            }
            ExecutableExpressionKind::Call { arguments, .. } => self.unique_child_storage_scope(
                arguments.into_iter().map(|argument| argument.value),
                visited_expressions,
                visited_paths,
            ),
            ExecutableExpressionKind::Then { input, output } => self.unique_child_storage_scope(
                std::iter::once(input).chain(output),
                visited_expressions,
                visited_paths,
            ),
            ExecutableExpressionKind::Latest { branches } => {
                self.unique_child_storage_scope(branches, visited_expressions, visited_paths)
            }
            _ => Ok(None),
        }
    }

    fn chunk_items_storage_scope(
        &mut self,
        projected_row: ExecutableExprId,
    ) -> Result<Option<ScopeId>, String> {
        let Some(chunk_scope) = self.storage_scope_for_expression(
            projected_row,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
        )?
        else {
            return Ok(None);
        };
        self.chunk_items_storage_scope_for_scope(chunk_scope)
    }

    fn chunk_items_storage_scope_for_scope(
        &mut self,
        chunk_scope: ScopeId,
    ) -> Result<Option<ScopeId>, String> {
        let declarations = self
            .list_scopes_by_declaration
            .iter()
            .filter_map(|(declaration, scope)| (*scope == chunk_scope).then_some(*declaration))
            .collect::<Vec<_>>();
        let [declaration] = declarations.as_slice() else {
            return if declarations.is_empty() {
                Ok(None)
            } else {
                Err(format!(
                    "row scope {chunk_scope} belongs to multiple typed list declarations {declarations:?}"
                ))
            };
        };
        let Some(producer) = self.statement_values.get(declaration).copied() else {
            return Ok(None);
        };
        let expression = self.expression(producer)?.clone();
        let ExecutableExpressionKind::Call {
            name, arguments, ..
        } = expression.kind
        else {
            return Ok(None);
        };
        if name != "List/chunk" {
            return Ok(None);
        }
        let Some(source) = call_argument(&arguments, "list") else {
            return Err(format!(
                "typed List/chunk producer {producer} has no canonical `list` argument"
            ));
        };
        self.storage_scope_for_expression(source, &mut BTreeSet::new(), &mut BTreeSet::new())
    }

    fn unique_child_storage_scope(
        &mut self,
        children: impl IntoIterator<Item = ExecutableExprId>,
        visited_expressions: &mut BTreeSet<ExecutableExprId>,
        visited_paths: &mut BTreeSet<String>,
    ) -> Result<Option<ScopeId>, String> {
        let mut scopes = BTreeSet::new();
        for child in children {
            if let Some(scope) =
                self.storage_scope_for_expression(child, visited_expressions, visited_paths)?
            {
                scopes.insert(scope);
            }
        }
        if scopes.len() > 1 {
            return Err(format!(
                "contextual view source reaches ambiguous storage scopes {scopes:?}"
            ));
        }
        Ok(scopes.pop_first())
    }

    fn static_tag(&self, id: ExecutableExprId) -> Option<String> {
        match &self.expression(id).ok()?.kind {
            ExecutableExpressionKind::Tag(value) | ExecutableExpressionKind::Text(value) => {
                Some(value.clone())
            }
            _ => None,
        }
    }
}

fn canonical_read_path(path: &str, projection: &[String]) -> String {
    if projection.is_empty() {
        path.to_owned()
    } else {
        format!("{path}.{}", projection.join("."))
    }
}

fn retained_visual_type(ty: &boon_typecheck::Type) -> bool {
    let boon_typecheck::Type::Object(shape) = ty else {
        return false;
    };
    let Some(boon_typecheck::Type::VariantSet(variants)) = shape.fields.get("kind") else {
        return false;
    };
    variants.iter().any(|variant| {
        matches!(
            variant,
            boon_typecheck::Variant::Tag(tag) if matches!(tag.as_str(), "Document" | "Scene")
        )
    })
}

fn call_argument(arguments: &[ExecutableCallArgument], name: &str) -> Option<ExecutableExprId> {
    arguments
        .iter()
        .find(|argument| argument.name == name)
        .map(|argument| argument.value)
}

fn executable_view_node_kind(function: &str) -> Option<String> {
    let constructor = function
        .strip_prefix("Scene/Element/")
        .or_else(|| function.strip_prefix("Element/"))?;
    Some(
        match constructor {
            "text_input" => "Input",
            "checkbox" => "Checkbox",
            "button" => "Button",
            "label" | "text" | "paragraph" | "link" => "Text",
            "stripe" => "Stripe",
            other => other,
        }
        .to_owned(),
    )
}

fn render_argument_can_contain_nodes(function: &str, argument: &str) -> bool {
    matches!(function, "Document/new" | "Scene/new")
        || matches!(
            argument,
            "root" | "child" | "children" | "items" | "contents" | "label" | "icon"
        )
}

fn render_record_field_can_contain_nodes(field: &str) -> bool {
    matches!(
        field,
        "root" | "child" | "children" | "items" | "contents" | "label" | "icon"
    )
}

fn canonical_event_attr(attr: &str) -> &str {
    if attr == "key_down" { "submit" } else { attr }
}

#[cfg(test)]
fn normalized_view_source_path(path: &str) -> String {
    path.split('.')
        .filter(|part| *part != "PASSED")
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
fn view_source_path_candidates(path: &str) -> Vec<String> {
    let normalized = normalized_view_source_path(path);
    let without_event_groups = normalized
        .split('.')
        .filter(|part| *part != "events")
        .collect::<Vec<_>>()
        .join(".");
    if normalized == without_event_groups {
        vec![normalized]
    } else {
        vec![normalized, without_event_groups]
    }
}

#[cfg(test)]
fn canonical_view_source_path<'a>(
    source_paths: &'a [(&'a str, SourceId)],
    value: &str,
) -> Option<(&'a str, SourceId)> {
    view_source_path_candidates(value)
        .into_iter()
        .find_map(|candidate| canonical_view_source_path_candidate(source_paths, &candidate))
}

#[cfg(test)]
fn canonical_view_source_path_candidate<'a>(
    source_paths: &'a [(&'a str, SourceId)],
    value: &str,
) -> Option<(&'a str, SourceId)> {
    if let Some((path, source_id)) = source_paths
        .iter()
        .find(|(source_path, _)| *source_path == value)
    {
        return Some((*path, *source_id));
    }
    let suffix = format!(".{}", value.split_once('.')?.1);
    let mut matches = source_paths
        .iter()
        .filter(|(source_path, _)| source_path.ends_with(&suffix));
    if let Some(first) = matches.next()
        && matches.next().is_none()
    {
        return Some((first.0, first.1));
    }

    let source_suffix = value.find(".sources.").map(|offset| &value[offset..])?;
    let mut matches = source_paths
        .iter()
        .filter(|(source_path, _)| source_path.ends_with(source_suffix));
    let first = matches.next()?;
    matches.next().is_none().then_some((first.0, first.1))
}

fn attr_can_bind_data(attr: &str) -> bool {
    matches!(
        attr,
        "text"
            | "label"
            | "value"
            | "display_value"
            | "edit_value"
            | "placeholder"
            | "checked"
            | "visible"
            | "selected"
            | "target"
            | "key"
            | "address"
            | "width"
            | "height"
            | "size"
            | "box_size"
            | "min_width"
            | "max_width"
            | "min_height"
            | "max_height"
            | "padding"
            | "padding_left"
            | "padding_right"
            | "padding_top"
            | "padding_bottom"
            | "gap"
            | "center"
            | "align_x"
            | "overlay_children"
            | "materialized"
            | "focus"
    )
}

fn require_known_symbol(
    context: &str,
    value: &str,
    known_symbols: &BTreeSet<&str>,
) -> Result<(), String> {
    if symbol_known(value, known_symbols) {
        Ok(())
    } else {
        Err(format!(
            "{context} `{value}` is not in the static schedule symbol table"
        ))
    }
}

fn symbol_known(value: &str, known_symbols: &BTreeSet<&str>) -> bool {
    known_symbols.contains(value)
        || known_symbols.iter().any(|known| {
            known.strip_prefix("@local.").map_or_else(
                || symbol_is_rooted_in(value, known),
                |projection| {
                    value
                        .split_once('.')
                        .is_some_and(|(_, suffix)| suffix == projection)
                },
            )
        })
}

fn symbol_is_rooted_in(value: &str, known: &str) -> bool {
    let mut candidate = known;
    loop {
        if value == candidate
            || value
                .strip_prefix(candidate)
                .is_some_and(|suffix| suffix.starts_with('.'))
        {
            return true;
        }
        let Some((_, suffix)) = candidate.split_once('.') else {
            return false;
        };
        candidate = suffix;
    }
}

fn view_projection_symbol_known(value: &str) -> bool {
    matches!(
        value,
        "column.label"
            | "column.index"
            | "sheet_row.row_number"
            | "focused_input.active"
            | "focused_input.address"
            | "focused_input.display_value"
            | "focused_input.edit_value"
            | "focused_input.value"
            | "focused_input.formula"
            | "focused_input.change_source"
            | "focused_input.submit_source"
            | "focused_input.cancel_source"
            | "focused_input.escape_source"
            | "focused_input.blur_source"
    )
}

fn list_projection_view_symbols(program: &ErasedProgram) -> BTreeSet<String> {
    program
        .list_projections
        .iter()
        .map(|projection| projection.target.clone())
        .collect()
}

fn verify_identity_clean_identifiers(program: &ErasedProgram) -> Result<(), String> {
    for source in &program.sources {
        reject_hidden_identity_identifier("source port", &source.path)?;
    }
    for cell in &program.state_cells {
        reject_hidden_identity_identifier("state cell", &cell.path)?;
        reject_hidden_identity_identifier("hold name", &cell.hold_name)?;
    }
    for list in &program.lists {
        reject_hidden_identity_identifier("list", &list.name)?;
        reject_list_initializer_identity(&list.initializer)?;
    }
    for value in &program.derived_values {
        reject_hidden_identity_identifier("derived value", &value.path)?;
        for source in &value.sources {
            reject_hidden_identity_identifier("derived value source", source)?;
        }
    }
    for edge in &program.dependencies {
        reject_hidden_identity_identifier("dependency source", &edge.from)?;
        reject_hidden_identity_identifier("dependency target", &edge.to)?;
    }
    for cause in &program.possible_causes {
        reject_hidden_identity_identifier("cause target", &cause.target)?;
        for source in &cause.sources {
            reject_hidden_identity_identifier("cause source", source)?;
        }
    }
    for projection in &program.list_projections {
        reject_hidden_identity_identifier("list projection target", &projection.target)?;
        reject_hidden_identity_identifier("list projection list", &projection.list)?;
        debug_assert!(matches!(&projection.kind, ListProjectionKind::Chunk { .. }));
    }
    Ok(())
}

fn reject_initial_value_identity(value: &InitialValue) -> Result<(), String> {
    match value {
        InitialValue::RootInitialField { path } => {
            reject_hidden_identity_identifier("root initial field", path)
        }
        InitialValue::RowInitialField { path } => {
            reject_hidden_identity_identifier("row initial field", path)
        }
        InitialValue::Enum { value } => reject_hidden_identity_identifier("enum value", value),
        InitialValue::Unknown { summary } => {
            reject_hidden_identity_identifier("unknown initializer", summary)
        }
        InitialValue::Text { .. }
        | InitialValue::Number { .. }
        | InitialValue::Bool { .. }
        | InitialValue::Bytes { .. }
        | InitialValue::Data { .. } => Ok(()),
    }
}

fn reject_list_initializer_identity(value: &ListInitializer) -> Result<(), String> {
    match value {
        ListInitializer::RecordLiteral { rows } => {
            for row in rows {
                for field in &row.fields {
                    reject_hidden_identity_identifier("list initial field", &field.name)?;
                    reject_initial_value_identity(&field.value)?;
                }
            }
            Ok(())
        }
        ListInitializer::Unknown { summary } => {
            reject_hidden_identity_identifier("unknown list initializer", summary)
        }
        ListInitializer::Range { .. } => Ok(()),
        ListInitializer::Empty => Ok(()),
    }
}

fn reject_hidden_identity_identifier(context: &str, value: &str) -> Result<(), String> {
    if let Some(token) = hidden_identity_token(value) {
        Err(format!(
            "IR exposes hidden runtime identity token `{token}` in {context} `{value}`"
        ))
    } else {
        Ok(())
    }
}

fn hidden_identity_token(value: &str) -> Option<&'static str> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("$boon") {
        return Some("$boon");
    }
    let tokens = lower
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .filter(|token| !token.is_empty());
    const FORBIDDEN: &[&str] = &[
        "runtime_key",
        "item_key",
        "row_key",
        "hidden_key",
        "hidden_keys",
        "hidden_generation",
        "target_key",
        "target_generation",
        "source_id",
        "bind_epoch",
        "listkey",
        "slot",
    ];
    tokens.into_iter().find_map(|token| {
        FORBIDDEN
            .iter()
            .copied()
            .find(|forbidden| token == *forbidden)
    })
}

fn expression_coverage(
    program: &boon_typecheck::CheckedProgram,
    source_expression_count: usize,
    executable: &ExecutableProgram,
    lists: &[ListMemory],
    derived_values: &[DerivedValue],
    distributed_references: &DistributedReferences,
) -> ExpressionCoverage {
    let mut coverage = ExpressionCoverage {
        ast_expression_count: source_expression_count,
        distributed_reference_expression_count: distributed_references.value_references.len()
            + distributed_references.calls.len(),
        ..ExpressionCoverage::empty()
    };
    let scheduled_expr_ids = executable
        .expressions
        .iter()
        .map(|expression| expression.checked_expr_id.0 as usize)
        .collect::<BTreeSet<_>>();
    for expr in program
        .expressions
        .iter()
        .filter(|expression| (expression.id.0 as usize) < source_expression_count)
    {
        if let boon_typecheck::CheckedExpressionKind::Invalid { tokens } = &expr.kind {
            if scheduled_expr_ids.contains(&(expr.id.0 as usize)) {
                coverage.unknown_ast_expression_count += 1;
                coverage.unknown_labels.push(format!(
                    "scheduled checked expression line {}: {}",
                    expr.span.line,
                    if tokens.is_empty() {
                        "<empty>".to_owned()
                    } else {
                        tokens.join(" ")
                    }
                ));
            } else {
                coverage.ignored_unknown_ast_expression_count += 1;
                coverage.ignored_unknown_labels.push(format!(
                    "ignored checked expression line {}: {}",
                    expr.span.line,
                    if tokens.is_empty() {
                        "<empty>".to_owned()
                    } else {
                        tokens.join(" ")
                    }
                ));
            }
        }
    }
    for list in lists {
        match &list.initializer {
            ListInitializer::Unknown { summary } => {
                coverage.unknown_list_initializer_count += 1;
                coverage
                    .unknown_labels
                    .push(format!("list initializer {}: {summary}", list.name));
            }
            ListInitializer::RecordLiteral { rows } => {
                for row in rows {
                    for field in &row.fields {
                        if let InitialValue::Unknown { summary } = &field.value {
                            coverage.unknown_list_initial_value_count += 1;
                            coverage.unknown_labels.push(format!(
                                "list initial {}.{}: {summary}",
                                list.name, field.name
                            ));
                        }
                    }
                }
            }
            ListInitializer::Range { .. } | ListInitializer::Empty => {}
        }
    }
    for value in derived_values {
        if matches!(value.kind, DerivedValueKind::Unknown) {
            coverage.unknown_derived_value_count += 1;
            coverage
                .unknown_labels
                .push(format!("derived value {}: unknown", value.path));
        }
    }
    coverage
}

fn exact_dependency_edges(
    arms: &[StateUpdateArm],
    sources: &[SourcePort],
    states: &[StateCell],
) -> Result<Vec<DependencyEdge>, String> {
    let mut edges = BTreeSet::new();
    for arm in arms {
        let target = states
            .get(arm.state.as_usize())
            .filter(|state| state.id == arm.state)
            .ok_or_else(|| {
                format!(
                    "state update arm references missing target StateId {}",
                    arm.state
                )
            })?;
        let (from, source_indexed) = match arm.cause {
            EventCause::Source(source) => {
                let source = sources
                    .get(source.as_usize())
                    .filter(|candidate| candidate.id == source)
                    .ok_or_else(|| {
                        format!("state update arm references missing SourceId {source}")
                    })?;
                (source.path.clone(), source.scoped)
            }
            EventCause::State(state) => {
                let state = states
                    .get(state.as_usize())
                    .filter(|candidate| candidate.id == state)
                    .ok_or_else(|| {
                        format!("state update arm references missing StateId {state}")
                    })?;
                (state.path.clone(), state.indexed)
            }
        };
        edges.insert((from, target.path.clone(), target.indexed || source_indexed));
    }
    Ok(edges
        .into_iter()
        .map(|(from, to, indexed)| DependencyEdge { from, to, indexed })
        .collect())
}

fn exact_possible_causes(
    arms: &[StateUpdateArm],
    sources: &[SourcePort],
    states: &[StateCell],
) -> Result<Vec<PossibleCause>, String> {
    let mut causes = states
        .iter()
        .map(|state| (state.id, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for arm in arms {
        let source = event_cause_path_owned(arm.cause, sources, states)?;
        let target = causes.get_mut(&arm.state).ok_or_else(|| {
            format!(
                "state update arm references missing target StateId {}",
                arm.state
            )
        })?;
        target.insert(source);
    }
    Ok(states
        .iter()
        .map(|state| PossibleCause {
            target: state.path.clone(),
            sources: causes
                .remove(&state.id)
                .unwrap_or_default()
                .into_iter()
                .collect(),
        })
        .collect())
}

fn event_cause_path_owned(
    cause: EventCause,
    sources: &[SourcePort],
    states: &[StateCell],
) -> Result<String, String> {
    match cause {
        EventCause::Source(source_id) => sources
            .get(source_id.as_usize())
            .filter(|source| source.id == source_id)
            .map(|source| source.path.clone())
            .ok_or_else(|| format!("state update arm references missing SourceId {source_id}")),
        EventCause::State(state_id) => states
            .get(state_id.as_usize())
            .filter(|state| state.id == state_id)
            .map(|state| state.path.clone())
            .ok_or_else(|| format!("state update arm references missing StateId {state_id}")),
    }
}

fn event_cause_static_owner(
    cause: EventCause,
    sources: &[SourcePort],
    states: &[StateCell],
) -> Result<Option<StaticOwnerId>, String> {
    match cause {
        EventCause::Source(source_id) => sources
            .get(source_id.as_usize())
            .filter(|source| source.id == source_id)
            .map(|source| source.static_owner)
            .ok_or_else(|| format!("event cause references missing SourceId {source_id}")),
        EventCause::State(state_id) => states
            .get(state_id.as_usize())
            .filter(|state| state.id == state_id)
            .map(|state| state.static_owner)
            .ok_or_else(|| format!("event cause references missing StateId {state_id}")),
    }
}

fn verify_executable_host_effect_calls_scheduled(
    executable: &ExecutableProgram,
    state_update_arms: &[StateUpdateArm],
) -> Result<(), String> {
    let mut calls = BTreeMap::<
        (
            boon_typecheck::CheckedExprId,
            Option<StaticOwnerId>,
            String,
        ),
        Vec<ExecutableExprId>,
    >::new();
    for expression in &executable.expressions {
        let ExecutableExpressionKind::Call { name, .. } = &expression.kind else {
            continue;
        };
        if boon_typecheck::is_typed_host_effect(name) {
            calls
                .entry((expression.checked_expr_id, expression.owner, name.clone()))
                .or_default()
                .push(expression.id);
        }
    }
    for ((checked, owner, name), expressions) in calls {
        if !state_update_arms.iter().any(|arm| {
            arm.owner == owner
                && expressions.iter().any(|expression| {
                    executable_expression_reaches(
                        executable,
                        arm.output_expression_id,
                        *expression,
                    )
                })
        }) {
            return Err(format!(
                "typed host effect `{name}` at checked expression {} owner {owner:?} has no exact state update arm; concrete expressions={expressions:?}",
                checked.0,
            ));
        }
    }
    Ok(())
}

fn exact_list_mutations(
    executable: &ExecutableProgram,
    static_owners: &[StaticOwnerDef],
    list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    row_scopes: &[RowScope],
    sources: &[SourcePort],
    states: &[StateCell],
    materializations: &[ContextualMaterialization],
) -> Result<Vec<ListMutation>, String> {
    let mut collector = ExecutableViewBindingCollector::new(
        executable,
        static_owners,
        None,
        list_storage,
        row_scopes,
        sources,
        states,
        materializations,
    )?;
    let mut mutations = Vec::new();
    for (statement_id, storage) in list_storage {
        let root = executable
            .statements
            .iter()
            .find(|statement| statement.id == *statement_id)
            .and_then(|statement| statement.value)
            .ok_or_else(|| {
                format!("typed list statement {statement_id} has no exact executable value")
            })?;
        collect_exact_list_mutations(
            executable,
            materializations,
            &mut collector,
            storage.list_id,
            root,
            &mut BTreeSet::new(),
            &mut mutations,
        )?;
    }
    for (ordinal, mutation) in mutations.iter_mut().enumerate() {
        mutation.ordinal = ordinal.try_into().map_err(|_| {
            "typed list mutation count exceeds the canonical u32 schedule range".to_owned()
        })?;
        mutation.owner = event_cause_static_owner(mutation.cause, sources, states)?;
    }
    Ok(mutations)
}

fn collect_exact_list_mutations(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    collector: &mut ExecutableViewBindingCollector<'_>,
    list_id: ListId,
    root: ExecutableExprId,
    visited: &mut BTreeSet<ExecutableExprId>,
    mutations: &mut Vec<ListMutation>,
) -> Result<(), String> {
    if !visited.insert(root) {
        return Ok(());
    }
    let expression = executable
        .expressions
        .get(root.as_usize())
        .filter(|expression| expression.id == root)
        .ok_or_else(|| format!("list mutation pipeline references missing expression {root}"))?;
    match &expression.kind {
        ExecutableExpressionKind::Materialize { materialization } => {
            let materialization = materializations.get(*materialization).ok_or_else(|| {
                format!(
                    "list mutation pipeline references missing materialization {materialization}"
                )
            })?;
            collect_exact_list_mutations(
                executable,
                materializations,
                collector,
                list_id,
                materialization.source,
                visited,
                mutations,
            )?;
            let remove_when = match materialization.operation {
                ContextualOperationKind::Remove => Some(true),
                ContextualOperationKind::Retain => Some(false),
                ContextualOperationKind::Map
                | ContextualOperationKind::Filter
                | ContextualOperationKind::Every
                | ContextualOperationKind::Any
                | ContextualOperationKind::Find
                | ContextualOperationKind::SortBy
                | ContextualOperationKind::ThenBy => None,
            };
            if let Some(remove_when) = remove_when {
                for arm in collector.trigger_owned_arms_for_expression(materialization.body)? {
                    mutations.push(ListMutation {
                        list_id,
                        site: root,
                        ordinal: 0,
                        cause: arm.cause,
                        owner: arm.owner,
                        kind: ListMutationKind::Remove {
                            gate: arm.gate_expression_id,
                            owner: materialization.owner,
                            row_local: materialization.row_local,
                            predicate: arm.output_expression_id,
                            remove_when,
                        },
                    });
                }
            }
        }
        ExecutableExpressionKind::Call {
            name, arguments, ..
        } if name == "List/append" => {
            let list = arguments
                .iter()
                .find(|argument| argument.name == "list")
                .map(|argument| argument.value)
                .ok_or_else(|| {
                    format!("List/append expression {root} has no typed `list` input")
                })?;
            let item = arguments
                .iter()
                .find(|argument| argument.name == "item")
                .map(|argument| argument.value)
                .ok_or_else(|| {
                    format!("List/append expression {root} has no typed `item` input")
                })?;
            collect_exact_list_mutations(
                executable,
                materializations,
                collector,
                list_id,
                list,
                visited,
                mutations,
            )?;
            for arm in collector.trigger_owned_arms_for_expression(item)? {
                mutations.push(ListMutation {
                    list_id,
                    site: root,
                    ordinal: 0,
                    cause: arm.cause,
                    owner: arm.owner,
                    kind: ListMutationKind::Append {
                        gate: arm.gate_expression_id,
                        item: arm.output_expression_id,
                    },
                });
            }
        }
        ExecutableExpressionKind::Call { arguments, .. } => {
            if let Some(list) = arguments
                .iter()
                .find(|argument| argument.name == "list")
                .map(|argument| argument.value)
            {
                collect_exact_list_mutations(
                    executable,
                    materializations,
                    collector,
                    list_id,
                    list,
                    visited,
                    mutations,
                )?;
            }
        }
        ExecutableExpressionKind::Draining { input }
        | ExecutableExpressionKind::Project { input, .. } => {
            collect_exact_list_mutations(
                executable,
                materializations,
                collector,
                list_id,
                *input,
                visited,
                mutations,
            )?;
        }
        ExecutableExpressionKind::Block { bindings, result } => {
            for value in bindings
                .iter()
                .map(|binding| binding.value)
                .chain([*result])
            {
                collect_exact_list_mutations(
                    executable,
                    materializations,
                    collector,
                    list_id,
                    value,
                    visited,
                    mutations,
                )?;
            }
        }
        ExecutableExpressionKind::When { arms, .. } => {
            for arm in arms {
                collect_exact_list_mutations(
                    executable,
                    materializations,
                    collector,
                    list_id,
                    arm.output,
                    visited,
                    mutations,
                )?;
            }
        }
        ExecutableExpressionKind::Latest { branches } => {
            for branch in branches {
                collect_exact_list_mutations(
                    executable,
                    materializations,
                    collector,
                    list_id,
                    *branch,
                    visited,
                    mutations,
                )?;
            }
        }
        ExecutableExpressionKind::CanonicalRead { .. }
        | ExecutableExpressionKind::LocalRead { .. }
        | ExecutableExpressionKind::ExternalRead { .. }
        | ExecutableExpressionKind::ElementState { .. }
        | ExecutableExpressionKind::Drain { .. }
        | ExecutableExpressionKind::Text(_)
        | ExecutableExpressionKind::TextTemplate { .. }
        | ExecutableExpressionKind::Number(_)
        | ExecutableExpressionKind::BytesByte(_)
        | ExecutableExpressionKind::Bool(_)
        | ExecutableExpressionKind::Tag(_)
        | ExecutableExpressionKind::TaggedObject { .. }
        | ExecutableExpressionKind::Source { .. }
        | ExecutableExpressionKind::Hold { .. }
        | ExecutableExpressionKind::Then { .. }
        | ExecutableExpressionKind::Infix { .. }
        | ExecutableExpressionKind::MatchArm { .. }
        | ExecutableExpressionKind::Object(_)
        | ExecutableExpressionKind::Record(_)
        | ExecutableExpressionKind::List { .. }
        | ExecutableExpressionKind::Bytes { .. }
        | ExecutableExpressionKind::Delimiter
        | ExecutableExpressionKind::MaterializationLocal { .. }
        | ExecutableExpressionKind::FunctionParameter { .. } => {}
    }
    Ok(())
}

fn executable_list_projections(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    lists: &[ListMemory],
) -> Result<Vec<ListProjection>, String> {
    let lists_by_declaration = list_storage
        .iter()
        .filter_map(|(statement, storage)| {
            executable
                .statements
                .iter()
                .find(|candidate| candidate.id == *statement)
                .and_then(|statement| statement.declaration)
                .map(|declaration| (declaration, storage.list_id))
        })
        .collect::<BTreeMap<_, _>>();
    let mut projections = Vec::new();
    let mut targets = BTreeSet::new();
    for (statement_id, target) in list_storage {
        let statement = executable
            .statements
            .iter()
            .find(|candidate| candidate.id == *statement_id)
            .ok_or_else(|| {
                format!(
                    "list storage '{}' references missing executable statement {}",
                    target.path, statement_id
                )
            })?;
        let Some(root) = statement.value else {
            continue;
        };
        let Some(chunk) = terminal_chunk_expression(executable, root, &mut BTreeSet::new())? else {
            continue;
        };
        let expression = executable
            .expressions
            .get(chunk.as_usize())
            .filter(|candidate| candidate.id == chunk)
            .ok_or_else(|| format!("chunk expression {chunk} is missing"))?;
        let ExecutableExpressionKind::Call {
            name, arguments, ..
        } = &expression.kind
        else {
            return Err(format!(
                "terminal chunk expression {chunk} is not an executable call"
            ));
        };
        if name != "List/chunk" {
            return Err(format!(
                "terminal chunk expression {chunk} resolved to unexpected callable '{name}'"
            ));
        }
        let list_arguments = arguments
            .iter()
            .filter(|argument| argument.name == "list")
            .collect::<Vec<_>>();
        let [list_argument] = list_arguments.as_slice() else {
            return Err(format!(
                "List/chunk expression {chunk} must have exactly one checked 'list' argument"
            ));
        };
        let source_id = executable_list_id(
            executable,
            materializations,
            &lists_by_declaration,
            list_argument.value,
            &mut BTreeSet::new(),
        )?
        .ok_or_else(|| {
            format!("List/chunk expression {chunk} has no exact executable list provenance")
        })?;
        let source = lists
            .get(source_id.as_usize())
            .filter(|candidate| candidate.id == source_id)
            .ok_or_else(|| {
                format!(
                    "List/chunk expression {chunk} references missing source ListId {source_id}"
                )
            })?;
        let size_arguments = arguments
            .iter()
            .filter(|argument| argument.name == "size")
            .collect::<Vec<_>>();
        let [size_argument] = size_arguments.as_slice() else {
            return Err(format!(
                "List/chunk expression {chunk} must have exactly one checked 'size' argument"
            ));
        };
        let size = match executable_static_data(
            executable,
            size_argument.value,
            &BTreeMap::new(),
            &mut BTreeSet::new(),
        ) {
            Ok(boon_data::Value::Number(value)) => value.to_usize_exact().ok(),
            Ok(_) | Err(_) => None,
        };
        if !targets.insert(target.path.clone()) {
            return Err(format!(
                "list projection target '{}' was lowered more than once",
                target.path
            ));
        }
        projections.push(ListProjection {
            target: target.path.clone(),
            list: source.name.clone(),
            kind: ListProjectionKind::Chunk { size },
        });
    }
    Ok(projections)
}

fn terminal_chunk_expression(
    executable: &ExecutableProgram,
    expression: ExecutableExprId,
    visiting: &mut BTreeSet<ExecutableExprId>,
) -> Result<Option<ExecutableExprId>, String> {
    if !visiting.insert(expression) {
        return Err(format!(
            "list projection expression {expression} contains an executable cycle"
        ));
    }
    let candidate = executable
        .expressions
        .get(expression.as_usize())
        .filter(|candidate| candidate.id == expression)
        .ok_or_else(|| format!("list projection expression {expression} is missing"))?;
    let result = match &candidate.kind {
        ExecutableExpressionKind::Call { name, .. } if name == "List/chunk" => Some(expression),
        ExecutableExpressionKind::Block { result, .. } => {
            terminal_chunk_expression(executable, *result, visiting)?
        }
        ExecutableExpressionKind::Project { input, .. }
        | ExecutableExpressionKind::Draining { input } => {
            terminal_chunk_expression(executable, *input, visiting)?
        }
        ExecutableExpressionKind::Then {
            output: Some(output),
            ..
        }
        | ExecutableExpressionKind::MatchArm {
            output: Some(output),
            ..
        } => terminal_chunk_expression(executable, *output, visiting)?,
        ExecutableExpressionKind::When { arms, .. } => {
            terminal_chunk_branches(executable, arms.iter().map(|arm| arm.output), visiting)?
        }
        ExecutableExpressionKind::Latest { branches } => {
            terminal_chunk_branches(executable, branches.iter().copied(), visiting)?
        }
        _ => None,
    };
    visiting.remove(&expression);
    Ok(result)
}

fn terminal_chunk_branches(
    executable: &ExecutableProgram,
    branches: impl Iterator<Item = ExecutableExprId>,
    visiting: &mut BTreeSet<ExecutableExprId>,
) -> Result<Option<ExecutableExprId>, String> {
    let chunks = branches
        .map(|branch| terminal_chunk_expression(executable, branch, visiting))
        .collect::<Result<BTreeSet<_>, _>>()?;
    match chunks.len() {
        0 => Ok(None),
        1 => Ok(chunks.into_iter().next().flatten()),
        _ => {
            Err("conditional list projection has inconsistent terminal chunk operations".to_owned())
        }
    }
}

fn executable_list_id(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    lists_by_declaration: &BTreeMap<boon_typecheck::DeclId, ListId>,
    expression: ExecutableExprId,
    visiting: &mut BTreeSet<ExecutableExprId>,
) -> Result<Option<ListId>, String> {
    if !visiting.insert(expression) {
        return Err(format!(
            "list provenance expression {expression} contains an executable cycle"
        ));
    }
    let candidate = executable
        .expressions
        .get(expression.as_usize())
        .filter(|candidate| candidate.id == expression)
        .ok_or_else(|| format!("list provenance expression {expression} is missing"))?;
    let result = match &candidate.kind {
        ExecutableExpressionKind::CanonicalRead {
            target, projection, ..
        }
        | ExecutableExpressionKind::LocalRead {
            declaration: target,
            projection,
        } if projection.is_empty() => lists_by_declaration.get(target).copied(),
        ExecutableExpressionKind::Materialize { materialization } => materializations
            .get(*materialization)
            .filter(|candidate| candidate.id == *materialization)
            .and_then(|materialization| {
                materialization
                    .target_list_id
                    .or(materialization.source_list_id)
            }),
        ExecutableExpressionKind::Block { result, .. } => executable_list_id(
            executable,
            materializations,
            lists_by_declaration,
            *result,
            visiting,
        )?,
        ExecutableExpressionKind::Project { input, fields } if fields.is_empty() => {
            executable_list_id(
                executable,
                materializations,
                lists_by_declaration,
                *input,
                visiting,
            )?
        }
        ExecutableExpressionKind::Draining { input } => executable_list_id(
            executable,
            materializations,
            lists_by_declaration,
            *input,
            visiting,
        )?,
        ExecutableExpressionKind::Then {
            output: Some(output),
            ..
        }
        | ExecutableExpressionKind::MatchArm {
            output: Some(output),
            ..
        } => executable_list_id(
            executable,
            materializations,
            lists_by_declaration,
            *output,
            visiting,
        )?,
        ExecutableExpressionKind::Call { arguments, .. }
            if matches!(candidate.flow_type.ty, boon_typecheck::Type::List(_)) =>
        {
            let list_inputs = arguments
                .iter()
                .filter(|argument| {
                    executable
                        .expressions
                        .get(argument.value.as_usize())
                        .filter(|input| input.id == argument.value)
                        .is_some_and(|input| {
                            matches!(input.flow_type.ty, boon_typecheck::Type::List(_))
                        })
                })
                .map(|argument| argument.value)
                .collect::<Vec<_>>();
            let [input] = list_inputs.as_slice() else {
                visiting.remove(&expression);
                return Ok(None);
            };
            executable_list_id(
                executable,
                materializations,
                lists_by_declaration,
                *input,
                visiting,
            )?
        }
        _ => None,
    };
    visiting.remove(&expression);
    Ok(result)
}

fn state_update_arms(
    executable: &ExecutableProgram,
    static_owners: &[StaticOwnerDef],
    row_scopes: &[RowScope],
    derived_list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    sources: &[SourcePort],
    state_cells: &[StateCell],
    materializations: &[ContextualMaterialization],
) -> Result<Vec<StateUpdateArm>, String> {
    let mut collector = ExecutableViewBindingCollector::new(
        executable,
        static_owners,
        None,
        derived_list_storage,
        row_scopes,
        sources,
        state_cells,
        materializations,
    )?;
    let mut result = Vec::new();
    for state in state_cells {
        let Some(executable_state_id) = state.executable_state_id else {
            continue;
        };
        let executable_state = executable
            .states
            .get(executable_state_id.as_usize())
            .filter(|candidate| candidate.id == executable_state_id)
            .ok_or_else(|| {
                format!(
                    "state `{}` references missing executable state {}",
                    state.path, executable_state_id
                )
            })?;
        for arm in collector.trigger_owned_arms_for_expression(executable_state.expression)? {
            result.push(StateUpdateArm {
                state: state.id,
                cause: arm.cause,
                gate_checked_expr_id: arm.gate_checked_expr_id,
                gate_expression_id: arm.gate_expression_id,
                owner: arm.owner,
                output_expression_id: arm.output_expression_id,
            });
        }
    }
    result.sort_by_key(|arm| {
        (
            arm.state,
            arm.cause,
            arm.gate_expression_id,
            arm.output_expression_id,
        )
    });
    result.dedup();
    Ok(result)
}

fn derived_values(
    checked: &boon_typecheck::CheckedProgram,
    executable: &ExecutableProgram,
    static_owners: &[StaticOwnerDef],
    row_scopes: &[RowScope],
    derived_list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    erased_fields: &[ErasedFieldDef],
    state_cells: &[StateCell],
    sources: &[SourcePort],
    materializations: &[ContextualMaterialization],
    producer_function_instances: &[ProducerFunctionInstance],
    distributed_value_references: &[DistributedValueReference],
) -> Result<Vec<DerivedValue>, String> {
    let mut event_source_collector = ExecutableViewBindingCollector::new(
        executable,
        static_owners,
        None,
        derived_list_storage,
        row_scopes,
        sources,
        state_cells,
        materializations,
    )?;
    let output_roots = output_root_declarations(checked, &checked.lowering_metadata);
    let retained_output_root_statements = output_roots
        .iter()
        .filter(|output| {
            matches!(
                output.contract,
                SemanticOutputContractKind::RetainedVisual { .. }
            )
        })
        .map(|output| ExecutableStatementId(output.statement_id))
        .collect::<BTreeSet<_>>();
    let host_output_root_statements = output_roots
        .iter()
        .filter(|output| matches!(output.contract, SemanticOutputContractKind::HostValue))
        .map(|output| ExecutableStatementId(output.statement_id))
        .collect::<BTreeSet<_>>();
    let hold_body_statements = executable_hold_body_statement_ids(executable);
    let candidates = executable
        .statements
        .iter()
        .filter_map(|statement| {
            if hold_body_statements.contains(&statement.id) {
                return None;
            }
            if retained_output_root_statements.contains(&statement.id) {
                return None;
            }
            // Structural field groups define ownership and schema. Checked
            // elaboration resolves their uses to concrete child producers, so
            // compiling the parent as a scalar value would duplicate
            // authority and send list mutation pipelines through the scalar
            // backend.
            if executable_statement_is_structural_group(executable, statement)
                && !host_output_root_statements.contains(&statement.id)
            {
                return None;
            }
            let (_, path) = executable_statement_name_path(&statement.kind)?;
            let value = statement.value?;
            let field = erased_fields.iter().find(|field| {
                field.statement == Some(statement.id)
                    && field.diagnostic_path == path
                    && field.role.is_value()
                    && field.producer.is_some()
            })?;
            // A list expression is not necessarily keyed list authority.
            // Closed scalar lists remain ordinary demand-current values and
            // need a root field computation. Only declarations with exact
            // materialized row storage are represented by `ListId` instead.
            if (derived_list_storage.contains_key(&statement.id)
                && matches!(
                    executable
                        .expressions
                        .get(value.as_usize())
                        .map(|value| &value.kind),
                    Some(ExecutableExpressionKind::List { .. })
                ))
                || direct_list_alias_target(executable, statement).is_some()
                || producer_function_instances
                    .iter()
                    .any(|instance| instance.result_path == path && instance.root == value)
                || field.static_owner.is_some_and(|owner| {
                    producer_function_instances.iter().any(|instance| {
                        static_owner_descends_from(owner, instance.owner, static_owners)
                    })
                })
                || state_cells.iter().any(|state| {
                    state.statement_id == statement.id.as_usize()
                        && state
                            .executable_state_id
                            .and_then(|state| executable.states.get(state.as_usize()))
                            .is_some_and(|state| state.expression == value)
                })
                || checked.sources.iter().any(|source| source.path == path)
                || distributed_value_references.iter().any(|reference| {
                    matches!(
                        reference.flow_mode,
                        boon_typecheck::FlowMode::TickPresent
                            | boon_typecheck::FlowMode::PresentOrAbsent
                    ) && reference
                        .local_alias_paths
                        .iter()
                        .any(|alias| alias == path)
                })
            {
                return None;
            }
            Some((statement, field))
        })
        .collect::<Vec<_>>();

    let mut values = Vec::with_capacity(candidates.len());
    for (statement, field) in candidates {
        let structural_group = executable_statement_is_structural_group(executable, statement);
        let materialized_storage = derived_list_storage.get(&statement.id).cloned();
        if materialized_storage.is_none()
            && !structural_group
            && statement.flow_type.as_ref().is_some_and(|flow_type| {
                flow_type.mode == boon_typecheck::FlowMode::Continuous
                    && matches!(
                        &flow_type.ty,
                        boon_typecheck::Type::List(item)
                            if matches!(item.as_ref(), boon_typecheck::Type::Object(_))
                    )
            })
        {
            return Err(format!(
                "checked keyed list value `{}` has no materialized storage",
                field.diagnostic_path
            ));
        }
        let (trigger_arms, default_roots) = if structural_group || materialized_storage.is_some() {
            (Vec::new(), Vec::new())
        } else {
            event_source_collector.trigger_owned_arms_for_statement(statement.id)?
        };
        let causes = trigger_arms
            .iter()
            .map(|arm| arm.cause)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut source_paths = causes
            .iter()
            .map(|cause| event_cause_path_owned(*cause, sources, state_cells))
            .collect::<Result<Vec<_>, _>>()?;
        source_paths.sort();
        source_paths.dedup();
        let kind = if materialized_storage.is_some() {
            DerivedValueKind::ListView
        } else if structural_group {
            DerivedValueKind::Pure
        } else if !trigger_arms.is_empty() {
            DerivedValueKind::SourceEventTransform
        } else if statement.value.is_some_and(|root| {
            executable_expression_contains_exact_call(
                executable,
                root,
                &[
                    "List/count",
                    "List/every",
                    "List/sum",
                    "List/page",
                    "Text/join",
                ],
            )
        }) {
            DerivedValueKind::Aggregate
        } else {
            DerivedValueKind::Pure
        };
        values.push(DerivedValue {
            id: FieldId(values.len()),
            executable_statement_id: statement.id,
            path: field.diagnostic_path.clone(),
            kind: kind.clone(),
            materialized_list_id: materialized_storage.as_ref().map(|storage| storage.list_id),
            materialized_row_scope_id: materialized_storage
                .as_ref()
                .map(|storage| storage.row_scope_id),
            causes,
            trigger_arms,
            default_roots,
            sources: source_paths,
            indexed: field.row.is_some(),
            scope_id: field.row.map(|row| row.scope),
            startup_recompute: derived_value_startup_recompute(&kind),
        });
    }
    Ok(values)
}

fn executable_hold_body_statement_ids(
    executable: &ExecutableProgram,
) -> BTreeSet<ExecutableStatementId> {
    let statements = executable
        .statements
        .iter()
        .map(|statement| (statement.id, statement))
        .collect::<BTreeMap<_, _>>();
    let mut pending = executable
        .statements
        .iter()
        .filter(|statement| matches!(statement.kind, ExecutableStatementKind::Hold { .. }))
        .flat_map(|statement| statement.children.iter().copied())
        .collect::<Vec<_>>();
    let mut result = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !result.insert(id) {
            continue;
        }
        if let Some(statement) = statements.get(&id) {
            pending.extend(statement.children.iter().copied());
        }
    }
    result
}

fn executable_expression_contains_exact_call(
    program: &ExecutableProgram,
    root: ExecutableExprId,
    names: &[&str],
) -> bool {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(expression_id) = pending.pop() {
        if !visited.insert(expression_id) {
            continue;
        }
        let Some(expression) = program
            .expressions
            .get(expression_id.as_usize())
            .filter(|expression| expression.id == expression_id)
        else {
            continue;
        };
        if matches!(
            &expression.kind,
            ExecutableExpressionKind::Call { name, .. }
                if names.iter().any(|candidate| name == candidate)
        ) {
            return true;
        }
        pending.extend(executable_expression_children(&expression.kind));
    }
    false
}
#[allow(clippy::too_many_arguments)]
fn producer_derived_values(
    executable: &ExecutableProgram,
    static_owners: &[StaticOwnerDef],
    row_scopes: &[RowScope],
    derived_list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    state_cells: &[StateCell],
    sources: &[SourcePort],
    materializations: &[ContextualMaterialization],
    instances: &[ProducerFunctionInstance],
) -> Result<Vec<DerivedValue>, String> {
    let mut event_source_collector = ExecutableViewBindingCollector::new(
        executable,
        static_owners,
        None,
        derived_list_storage,
        row_scopes,
        sources,
        state_cells,
        materializations,
    )?;
    let mut values = Vec::with_capacity(instances.len());
    for instance in instances {
        let field = executable
            .statements
            .iter()
            .find(|statement| {
                statement.value == Some(instance.root)
                    && matches!(
                        &statement.kind,
                        ExecutableStatementKind::Field { path, .. }
                            if path == &instance.result_path
                    )
            })
            .ok_or_else(|| {
                format!(
                    "producer function identity {} has no ordinary executable result field",
                    producer_identity_text(instance.identity)
                )
            })?;
        let materialized_storage = derived_list_storage.get(&field.id).cloned();
        let structural_group = executable_statement_is_structural_group(executable, field);
        let (trigger_arms, default_roots) = if structural_group || materialized_storage.is_some() {
            (Vec::new(), Vec::new())
        } else {
            event_source_collector.trigger_owned_arms_for_statement(field.id)?
        };
        let causes = trigger_arms
            .iter()
            .map(|arm| arm.cause)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut source_paths = causes
            .iter()
            .map(|cause| match cause {
                EventCause::Source(source) => sources
                    .get(source.as_usize())
                    .filter(|candidate| candidate.id == *source)
                    .map(|source| source.path.clone())
                    .ok_or_else(|| format!("event cause references missing SourceId {source}")),
                EventCause::State(state) => state_cells
                    .get(state.as_usize())
                    .filter(|candidate| candidate.id == *state)
                    .map(|state| state.path.clone())
                    .ok_or_else(|| format!("event cause references missing StateId {state}")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        source_paths.sort();
        source_paths.dedup();
        let kind = if materialized_storage.is_some() {
            DerivedValueKind::ListView
        } else if !trigger_arms.is_empty() {
            DerivedValueKind::SourceEventTransform
        } else {
            DerivedValueKind::Pure
        };
        values.push(DerivedValue {
            id: instance.result_field,
            executable_statement_id: field.id,
            path: instance.result_path.clone(),
            kind: kind.clone(),
            materialized_list_id: materialized_storage.as_ref().map(|storage| storage.list_id),
            materialized_row_scope_id: materialized_storage
                .as_ref()
                .map(|storage| storage.row_scope_id),
            causes,
            trigger_arms,
            default_roots,
            sources: source_paths,
            indexed: false,
            scope_id: None,
            startup_recompute: derived_value_startup_recompute(&kind),
        });
    }
    Ok(values)
}

fn executable_statement_is_structural_group(
    executable: &ExecutableProgram,
    statement: &ExecutableStatement,
) -> bool {
    matches!(statement.kind, ExecutableStatementKind::Field { .. })
        && statement.value.is_some_and(|value| {
            executable
                .expressions
                .get(value.as_usize())
                .filter(|expression| expression.id == value)
                .is_some_and(|expression| {
                    matches!(
                        expression.kind,
                        ExecutableExpressionKind::Object(_)
                            | ExecutableExpressionKind::Record(_)
                            | ExecutableExpressionKind::TaggedObject { .. }
                    )
                })
        })
        && !statement.children.is_empty()
        && statement.children.iter().all(|child| {
            executable
                .statements
                .iter()
                .find(|candidate| candidate.id == *child)
                .is_some_and(|child| {
                    matches!(
                        child.kind,
                        ExecutableStatementKind::Field { .. }
                            | ExecutableStatementKind::Source { .. }
                            | ExecutableStatementKind::Hold { .. }
                            | ExecutableStatementKind::List { .. }
                            | ExecutableStatementKind::Spread
                    )
                })
        })
}

fn is_output_registry_value_path(path: &str) -> bool {
    path.strip_prefix("outputs.")
        .is_some_and(|name| !name.is_empty() && !name.contains('.'))
}

fn derived_value_startup_recompute(kind: &DerivedValueKind) -> bool {
    match kind {
        DerivedValueKind::SourceEventTransform => true,
        DerivedValueKind::Pure => false,
        DerivedValueKind::ListView | DerivedValueKind::Aggregate | DerivedValueKind::Unknown => {
            false
        }
    }
}

fn initial_record_from_data(value: boon_data::Value) -> Option<ListInitialRecord> {
    let boon_data::Value::Record(fields) = value else {
        return None;
    };
    Some(ListInitialRecord {
        fields: fields
            .into_iter()
            .map(|(name, value)| ListRowInitialField {
                name,
                value: initial_value_from_data(value),
                expression: None,
            })
            .collect(),
    })
}

fn initial_value_from_data(value: boon_data::Value) -> InitialValue {
    match value {
        boon_data::Value::Bool(value) => InitialValue::Bool { value },
        boon_data::Value::Number(value) => InitialValue::Number {
            value: value.to_string(),
        },
        boon_data::Value::Text(value) => InitialValue::Text { value },
        boon_data::Value::Bytes(bytes) => InitialValue::Bytes {
            fixed_len: Some(bytes.len()),
            bytes: bytes.to_vec(),
        },
        boon_data::Value::Variant { tag, fields } if fields.is_empty() => {
            InitialValue::Enum { value: tag }
        }
        value => InitialValue::Data { value },
    }
}

fn hidden_key_type(name: &str) -> String {
    let singular = name
        .strip_suffix("ies")
        .map(|prefix| format!("{prefix}y"))
        .or_else(|| name.strip_suffix('s').map(ToOwned::to_owned))
        .unwrap_or_else(|| name.to_owned());
    let mut output = String::new();
    let mut uppercase_next = true;
    for ch in singular.chars() {
        if ch == '_' || ch == '-' {
            uppercase_next = true;
            continue;
        }
        if uppercase_next {
            output.push(ch.to_ascii_uppercase());
            uppercase_next = false;
        } else {
            output.push(ch);
        }
    }
    output.push_str("Key");
    output
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod typed_derived_list_storage_tests {
    use super::*;

    fn storage_for(ir: &ErasedProgram, path: &str) -> (ListId, ScopeId) {
        let derived = ir
            .derived_values
            .iter()
            .find(|value| value.path == path)
            .unwrap_or_else(|| panic!("missing derived value `{path}`"));
        assert_eq!(derived.kind, DerivedValueKind::ListView);
        let list_id = derived
            .materialized_list_id
            .unwrap_or_else(|| panic!("missing materialized ListId for `{path}`"));
        let row_scope_id = derived
            .materialized_row_scope_id
            .unwrap_or_else(|| panic!("missing materialized row ScopeId for `{path}`"));
        let list = &ir.lists[list_id.as_usize()];
        assert_eq!(list.name, path);
        assert_eq!(list.row_scope_id, Some(row_scope_id));
        assert!(list.has_generation);
        assert_eq!(ir.row_scopes[row_scope_id.as_usize()].list, path);
        (list_id, row_scope_id)
    }

    #[test]
    fn literal_list_field_uses_direct_keyed_storage_without_a_derived_recompute() {
        let parsed = boon_parser::parse_source(
            "typed-literal-list.bn",
            r#"
defaults:
    LIST {
        [name: TEXT { one }]
        [name: TEXT { two }]
    }
"#,
        )
        .unwrap();
        let ir = lower(&parsed).expect("typed literal list must lower");
        assert!(
            ir.derived_values
                .iter()
                .all(|value| value.path != "defaults"),
            "a literal initializer is storage, not a recomputed list view"
        );
        let binding = ir
            .scope_index
            .bindings
            .iter()
            .find(|binding| binding.diagnostic_path == "defaults")
            .expect("literal list storage binding");
        let ErasedBindingTarget::Value {
            field: None,
            row:
                Some(ErasedRowBinding {
                    list,
                    scope: row_scope,
                }),
        } = binding.target
        else {
            panic!("literal list must own exact keyed storage: {binding:?}");
        };
        assert_eq!(ir.lists[list.as_usize()].row_scope_id, Some(row_scope));
        assert!(matches!(
            ir.lists[list.as_usize()].initializer,
            ListInitializer::RecordLiteral { .. }
        ));
    }

    #[test]
    fn contextual_map_reuses_constructor_authority_and_preserves_row_identity() {
        let parsed = boon_parser::parse_source(
            "typed-literal-map.bn",
            r#"
mapped:
    LIST {
        [value: 1]
        [value: 2]
    }
    |> List/map(item, new: [value: item.value + 1])
"#,
        )
        .unwrap();
        let ir = lower(&parsed).expect("typed literal map must lower");
        let (list, _) = storage_for(&ir, "mapped");
        assert!(matches!(
            ir.lists[list.as_usize()].initializer,
            ListInitializer::RecordLiteral { ref rows } if rows.len() == 2
        ));
        let materialization = ir
            .materializations
            .iter()
            .find(|materialization| materialization.target_list_id == Some(list))
            .expect("mapped output materialization");
        assert_eq!(materialization.source_list_id, Some(list));
        assert_eq!(
            materialization.source_scope_id,
            materialization.target_scope_id
        );
        let local = ir
            .scope_index
            .locals
            .iter()
            .find(|local| {
                local.owner == materialization.owner && local.local == materialization.row_local
            })
            .expect("mapped local");
        assert_eq!(
            local.row,
            Some(ErasedRowBinding {
                list,
                scope: materialization.target_scope_id.expect("target scope"),
            })
        );
    }

    #[test]
    fn scalar_list_literals_remain_values_without_keyed_row_storage() {
        let parsed = boon_parser::parse_source(
            "typed-scalar-list.bn",
            r#"
selected: TEXT { alpha }
selected_ids: LIST { selected }
"#,
        )
        .unwrap();
        let ir = lower(&parsed).expect("scalar list must lower as an ordinary value");
        let binding = ir
            .scope_index
            .bindings
            .iter()
            .find(|binding| binding.diagnostic_path == "selected_ids")
            .expect("scalar list binding");
        assert!(matches!(
            binding.target,
            ErasedBindingTarget::Value {
                field: Some(_),
                row: None,
            }
        ));
        assert!(ir.lists.iter().all(|list| list.name != "selected_ids"));
        assert!(ir.derived_values.iter().any(|value| {
            value.path == "selected_ids"
                && value.materialized_list_id.is_none()
                && value.kind == DerivedValueKind::Pure
        }));
    }

    #[test]
    fn function_local_record_lists_are_values_not_global_storage() {
        let parsed = boon_parser::parse_source(
            "function-local-record-list.bn",
            r#"
store: component()

FUNCTION component() {
    [
        title: TEXT { component }
        rows: LIST {
            [name: TEXT { one }]
            [name: TEXT { two }]
        }
    ]
}
"#,
        )
        .unwrap();
        let ir = lower(&parsed).expect("function-local lists must lower through call ownership");
        assert!(
            ir.lists.is_empty(),
            "function-template record lists must not allocate global ListId storage: {:?}",
            ir.lists
        );
        assert!(ir.executable.statements.iter().any(|statement| {
            matches!(
                &statement.kind,
                ExecutableStatementKind::Field { path, .. } if path == "store"
            )
        }));
    }

    #[test]
    fn list_constructor_authority_is_explicit_before_backend_lowering() {
        let parsed = boon_parser::parse_source(
            "typed-list-authority.bn",
            r#"
store: [
    add: SOURCE
    candidate:
        add |> THEN {
            entries
            |> List/any(item, if: item.id == add.text)
            |> WHEN {
                True => SKIP
                False => [
                    id: add.text
                ]
            }
        }
    entries:
        LIST {}
        |> List/append(item: candidate)
        |> List/map(item, new: entry_view(entry: item))
]

FUNCTION entry_view(entry) {
    [
        id: entry.id
    ]
}
"#,
        )
        .unwrap();
        let ir = lower(&parsed).expect("list authority must lower structurally");
        let list = ir
            .lists
            .iter()
            .find(|list| list.name == "store.entries")
            .expect("entries list");
        let fields = ir
            .scope_index
            .fields
            .iter()
            .filter(|field| field.row.map(|row| row.list) == Some(list.id))
            .collect::<Vec<_>>();
        let authority = fields
            .iter()
            .copied()
            .find(|field| field.name == "id" && field.role == ErasedFieldRole::ListAuthority)
            .unwrap_or_else(|| panic!("constructor authority field among {fields:#?}"));
        let value = fields
            .iter()
            .copied()
            .find(|field| field.name == "id" && field.role == ErasedFieldRole::Value)
            .expect("mapped value field");
        assert_ne!(authority.id, value.id);
        assert!(authority.declaration.is_none());
        assert!(authority.producer.is_none());
        assert!(authority.diagnostic_path.starts_with("@authority/"));
        assert!(
            ir.semantic_index
                .fields
                .iter()
                .all(|field| field.id != authority.id),
            "authority storage is not a user-visible semantic field"
        );

        let any = ir
            .materializations
            .iter()
            .find(|materialization| materialization.operation == ContextualOperationKind::Any)
            .expect("List/any materialization");
        let local = ir
            .scope_index
            .locals
            .iter()
            .find(|local| local.owner == any.owner && local.local == any.row_local)
            .expect("List/any local");
        assert_eq!(
            local
                .members
                .iter()
                .find(|member| member.path == ["id"])
                .map(|member| member.target),
            Some(ErasedLocalMemberTarget::Field(authority.id))
        );
    }

    #[test]
    fn top_level_computed_list_gets_a_typed_derived_storage_value() {
        let parsed = boon_parser::parse_source(
            "typed-top-level-computed-list.bn",
            r#"
rows:
    LIST {
        [value: 1]
        [value: 2]
    }
mapped:
    rows
    |> List/map(item, new: [value: item.value + 1])
"#,
        )
        .unwrap();
        let ir = lower(&parsed).expect("top-level computed list must lower");
        let (list, row_scope) = storage_for(&ir, "mapped");
        let binding = ir
            .scope_index
            .bindings
            .iter()
            .find(|binding| binding.diagnostic_path == "mapped")
            .expect("computed list storage binding");
        assert!(matches!(
            binding.target,
            ErasedBindingTarget::Value {
                field: None,
                row: Some(ErasedRowBinding {
                    list: binding_list,
                    scope: binding_scope,
                }),
            } if binding_list == list && binding_scope == row_scope
        ));
    }

    #[test]
    fn direct_and_wrapped_map_filter_fields_get_distinct_keyed_storage() {
        let source = r#"
FUNCTION map_rows(list, entry: OUT, new) {
    list
    |> List/map(
        item: entry
        new: new
    )
}

FUNCTION filter_rows(list, entry: OUT, predicate) {
    list
    |> List/filter(
        item: entry
        if: predicate
    )
}

store: [
    rows: LIST {
        [value: 1]
        [value: 2]
    }
    direct_mapped:
        rows
        |> List/map(item, new: [value: item.value + 1])
    wrapped_mapped:
        rows
        |> map_rows(entry, new: [value: entry.value + 1])
    direct_filtered:
        rows
        |> List/filter(item, if: item.value > 0)
    wrapped_filtered:
        rows
        |> filter_rows(entry, predicate: entry.value > 0)
]
"#;
        let parsed = boon_parser::parse_source("typed-derived-map-filter.bn", source).unwrap();
        let ir = lower(&parsed).expect("typed direct and wrapped views must lower");
        let paths = [
            "store.direct_mapped",
            "store.wrapped_mapped",
            "store.direct_filtered",
            "store.wrapped_filtered",
        ];
        let storage = paths
            .iter()
            .map(|path| storage_for(&ir, path))
            .collect::<Vec<_>>();

        assert_eq!(
            storage
                .iter()
                .map(|(list, _)| *list)
                .collect::<BTreeSet<_>>()
                .len(),
            paths.len()
        );
        assert_eq!(
            storage
                .iter()
                .map(|(_, scope)| *scope)
                .collect::<BTreeSet<_>>()
                .len(),
            paths.len()
        );
        assert!(ir.lists.iter().any(|list| list.name == "store.rows"));
        assert!(ir.lists.iter().all(|list| list.name != "rows"));
        assert!(ir.lists.iter().all(|list| {
            !matches!(
                list.name.as_str(),
                "direct_mapped" | "wrapped_mapped" | "direct_filtered" | "wrapped_filtered"
            )
        }));
        assert!(
            ir.derived_values
                .iter()
                .filter(|value| value.kind == DerivedValueKind::ListView)
                .all(|value| value.materialized_list_id.is_some()
                    && value.materialized_row_scope_id.is_some())
        );
    }

    #[test]
    fn direct_and_wrapped_chunk_fields_get_deterministic_storage_without_parser_aliases() {
        let source = r#"
FUNCTION chunk_rows(list, size) {
    list |> List/chunk(size: size)
}

store: [
    rows: LIST {
        [value: 1]
        [value: 2]
        [value: 3]
    }
    direct_chunks: rows |> List/chunk(size: 2)
    wrapped_chunks: rows |> chunk_rows(size: 2)
]
"#;
        let parsed = boon_parser::parse_source("typed-derived-chunks.bn", source).unwrap();
        assert!(
            !parsed
                .list_memories
                .iter()
                .any(|list| list.name == "wrapped_chunks"),
            "the test must exercise a wrapper that parser list heuristics do not publish"
        );
        let first = lower(&parsed).expect("typed chunk views must lower");
        let second = lower(&parsed).expect("repeated lowering must be deterministic");
        let first_ids = [
            storage_for(&first, "store.direct_chunks"),
            storage_for(&first, "store.wrapped_chunks"),
        ];
        let second_ids = [
            storage_for(&second, "store.direct_chunks"),
            storage_for(&second, "store.wrapped_chunks"),
        ];

        assert_eq!(first_ids, second_ids);
        assert_ne!(first_ids[0].0, first_ids[1].0);
        assert_ne!(first_ids[0].1, first_ids[1].1);
        assert!(
            first
                .lists
                .iter()
                .all(|list| !matches!(list.name.as_str(), "direct_chunks" | "wrapped_chunks"))
        );
    }

    #[test]
    fn conditional_list_view_gets_keyed_storage_without_parser_aliases() {
        let source = r#"
store: [
    rows: LIST {
        [id: TEXT { a }]
        [id: TEXT { b }]
    }
    selected:
        True |> WHEN {
            True => rows |> List/filter(item, if: item.id == TEXT { a })
            False => rows
        }
    mapped:
        selected
        |> List/map(item, new: [label: item.id])
]
"#;
        let parsed = boon_parser::parse_source("typed-derived-conditional.bn", source).unwrap();
        let ir = lower(&parsed).expect("typed conditional list view must lower");
        let selected = storage_for(&ir, "store.selected");
        let mapped = storage_for(&ir, "store.mapped");

        assert_ne!(selected, mapped);
        assert!(ir.lists.iter().any(|list| list.name == "store.rows"));
        assert!(
            ir.lists
                .iter()
                .all(|list| !matches!(list.name.as_str(), "rows" | "selected" | "mapped"))
        );
    }
}
