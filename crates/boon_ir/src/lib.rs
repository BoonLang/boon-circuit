use boon_parser::{
    AstCallArg, AstExpr, AstExprKind, AstRecordField, AstStatement, AstStatementKind,
    BytesSizeSyntax, ParsedProgram, ParserItem as AstItem, ProgramKind,
};
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
    pub kind: ProgramKind,
    pub executable: ExecutableProgram,
    pub storage: StorageCatalog,
    pub expression_count: usize,
    pub expressions: Vec<AstExpr>,
    pub expression_coverage: ExpressionCoverage,
    #[serde(default)]
    pub distributed_references: DistributedReferences,
    pub semantic_index: SemanticIndex,
    pub graph_node_count: usize,
    pub nodes: Vec<IrNode>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub immediate_dependencies: Vec<ImmediateDependency>,
    pub dependencies: Vec<DependencyEdge>,
    pub possible_causes: Vec<PossibleCause>,
    pub update_branches: Vec<UpdateBranch>,
    pub list_operations: Vec<ListOperation>,
    pub list_projections: Vec<ListProjection>,
    pub functions: Vec<FunctionDefinition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_owners: Vec<StaticOwnerDef>,
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
    pub pure_calls: Vec<DistributedPureCall>,
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
pub struct DistributedPureCall {
    pub expr_id: ExprId,
    pub canonical_function: String,
    pub producer_role: boon_typecheck::ProgramRole,
    pub result_type: boon_typecheck::Type,
    pub arguments: Vec<DistributedPureCallArgument>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedPureCallArgument {
    pub name: String,
    pub expr_id: ExprId,
    pub argument_type: boon_typecheck::Type,
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
    NodeId,
    ScopeId,
    SourceId,
    StateId,
    ListId,
    FieldId,
    ViewBindingId,
    SourceUnitId,
    FunctionId,
    StaticOwnerId,
    StorageBindingId,
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
    pub unknown_initial_value_count: usize,
    pub unknown_list_initializer_count: usize,
    pub unknown_list_initial_value_count: usize,
    pub unknown_update_expression_count: usize,
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
            unknown_initial_value_count: 0,
            unknown_list_initializer_count: 0,
            unknown_list_initial_value_count: 0,
            unknown_update_expression_count: 0,
            unknown_list_predicate_count: 0,
            unknown_derived_value_count: 0,
            unknown_labels: Vec::new(),
            ignored_unknown_labels: Vec::new(),
        }
    }

    pub fn unknown_total(&self) -> usize {
        self.unknown_ast_expression_count
            + self.unknown_initial_value_count
            + self.unknown_list_initializer_count
            + self.unknown_list_initial_value_count
            + self.unknown_update_expression_count
            + self.unknown_list_predicate_count
            + self.unknown_derived_value_count
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IrNode {
    pub id: NodeId,
    pub name: String,
    pub kind: IrNodeKind,
    pub indexed: bool,
    pub expr_id: Option<ExprId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum IrNodeKind {
    SourceRead,
    PureCall,
    When,
    While,
    Then,
    Latest,
    Hold,
    ListAppend,
    ListRemove,
    ListMap,
    ListRetain,
    Aggregate,
    RenderLowering,
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
pub struct HostEffectCallArgument {
    pub name: String,
    pub value_expr_id: ExprId,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_lookup_field: Option<String>,
}

impl SourcePayloadSchema {
    pub fn row_lookup_field_name(&self) -> Option<&str> {
        self.row_lookup_field.as_deref()
    }
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
    fn from_name(name: &str) -> Self {
        match name {
            "address" => Self::Address,
            "bytes" => Self::Bytes,
            "key" => Self::Key,
            "text" => Self::Text,
            _ => Self::Named(name.to_owned()),
        }
    }

    fn name(&self) -> &str {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_state_id: Option<ExecutableStateId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_owner: Option<StaticOwnerId>,
    pub statement_id: usize,
    pub scope_id: Option<ScopeId>,
    pub hold_name: String,
    pub initial_value: InitialValue,
    pub initial_expr_id: Option<ExprId>,
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
    Chunk {
        size: Option<usize>,
        item_field: String,
        label_field: String,
    },
    TextPrefix {
        field: String,
        prefix: String,
        limit: Option<usize>,
        normalization: ListTextNormalization,
    },
    IndexedQuery {
        fields: Vec<ListQueryIndexField>,
        selection: ListQuerySelection,
        residual: Option<ListQueryResidual>,
        limit: Option<usize>,
        cursor: Option<String>,
        unique: bool,
        order: ListQueryOrder,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ListTextNormalization {
    Exact,
    TrimLowercase,
    Tokens,
    Unknown { value: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListQueryIndexField {
    pub path: Vec<String>,
    pub normalization: ListTextNormalization,
    pub multi_value: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ListQueryOrder {
    Ascending,
    Descending,
    Unknown { value: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ListQuerySelection {
    Exact {
        key: String,
    },
    TextPrefix {
        leading: Option<String>,
        prefix: String,
    },
    Range {
        lower: Option<String>,
        lower_inclusive: bool,
        upper: Option<String>,
        upper_inclusive: bool,
    },
    Union {
        keys: String,
    },
    Intersection {
        keys: String,
    },
    Unknown {
        value: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ListQueryResidual {
    FieldEqual {
        path: Vec<String>,
        value: String,
    },
    TextContains {
        path: Vec<String>,
        needle: String,
    },
    NumberRange {
        path: Vec<String>,
        minimum: Option<String>,
        maximum: Option<String>,
    },
    Wgs84Radius {
        latitude_path: Vec<String>,
        longitude_path: Vec<String>,
        center_latitude: String,
        center_longitude: String,
        radius_meters: String,
    },
    Unknown {
        value: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListInitialRecord {
    pub fields: Vec<ListRowInitialField>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListRowInitialField {
    pub name: String,
    pub value: InitialValue,
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
    pub statement: AstStatement,
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
    pub storage_binding_id: StorageBindingId,
    pub line: usize,
    pub typed_contract_known: bool,
    pub statement: AstStatement,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DerivedValueKind {
    SourceEventTransform,
    ListView,
    Aggregate,
    Pure,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub id: FunctionId,
    pub name: String,
    pub args: Vec<String>,
    pub statement: AstStatement,
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableExpression {
    pub id: ExecutableExprId,
    pub checked_expr_id: boon_typecheck::CheckedExprId,
    pub flow_type: boon_typecheck::FlowType,
    pub effect: boon_typecheck::CheckedEffectSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<StaticOwnerId>,
    pub kind: ExecutableExpressionKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutableRecordField {
    pub name: String,
    pub value: ExecutableExprId,
    pub spread: bool,
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
    pub pattern: Vec<String>,
    pub output: ExecutableExprId,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        storage_binding: Option<StorageBindingId>,
        path: String,
        projection: Vec<String>,
    },
    ExternalRead {
        canonical_path: String,
    },
    Drain {
        target: boon_typecheck::DeclId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        storage_binding: Option<StorageBindingId>,
        path: String,
        projection: Vec<String>,
    },
    Text(String),
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
        pattern: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<ExecutableExprId>,
    },
    Object(Vec<ExecutableRecordField>),
    Record(Vec<ExecutableRecordField>),
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
    Every,
    Any,
    Find,
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

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct StorageCatalog {
    pub bindings: Vec<StorageBinding>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StorageBinding {
    pub id: StorageBindingId,
    pub declaration: boon_typecheck::DeclId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_owner: Option<StaticOwnerId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owner_ancestry: Vec<StaticOwnerId>,
    pub flow_type: boon_typecheck::FlowType,
    pub producer: ExecutableExprId,
    pub diagnostic_path: String,
    pub kind: StorageBindingKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StorageBindingKind {
    Value {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        field: Option<FieldId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        list: Option<ListId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        row_scope: Option<ScopeId>,
    },
    Source {
        executable: ExecutableSourceId,
        runtime: SourceId,
    },
    State {
        executable: ExecutableStateId,
        runtime: StateId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContextualMaterialization {
    pub id: usize,
    pub operation: ContextualOperationKind,
    pub source: ExecutableExprId,
    pub body: ExecutableExprId,
    pub result_kind: MaterializationResultKind,
    pub row_local: MaterializationLocalId,
    pub owner: StaticOwnerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_list_id: Option<ListId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_scope_id: Option<ScopeId>,
    pub item_type: boon_typecheck::Type,
    pub result_type: boon_typecheck::Type,
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
pub struct ImmediateDependency {
    pub dependent: String,
    pub dependency: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PossibleCause {
    pub target: String,
    pub sources: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateBranch {
    pub target: String,
    pub source: String,
    pub expression: UpdateExpression,
    pub guard: Option<UpdateGuard>,
    pub indexed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum UpdateGuard {
    ValueOneOf { input: String, values: Vec<String> },
    ListIsNotEmpty { input: String, expected: bool },
    ValuesEqual { left: String, right: String },
    ValuesNotEqual { left: String, right: String },
    All { guards: Vec<UpdateGuard> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateMatchArm {
    pub pattern: String,
    pub output: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UpdateValueMatchArm {
    pub pattern: String,
    pub output: UpdateValueExpression,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum UpdateValueExpression {
    Const {
        value: String,
    },
    ReadPath {
        path: String,
    },
    MatchConst {
        input: String,
        arms: Vec<UpdateValueMatchArm>,
    },
    MatchTextIsEmptyConst {
        input: String,
        arms: Vec<UpdateValueMatchArm>,
    },
    NumberInfix {
        left: String,
        op: String,
        right: String,
    },
    MatchInfixConst {
        left: String,
        op: String,
        right: String,
        arms: Vec<UpdateValueMatchArm>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BytesScalarArg {
    Static(u64),
    Path(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum UpdateExpression {
    SourcePayload {
        path: String,
    },
    Const {
        value: String,
    },
    NumberInfix {
        left: String,
        op: String,
        right: String,
    },
    ProjectTime {
        pointer_x: String,
        pointer_width: String,
        viewport_start: String,
        viewport_end: String,
        fallback: String,
    },
    PreviousValue {
        path: String,
    },
    ReadPath {
        path: String,
    },
    TextTrimOrPrevious {
        path: String,
        previous: String,
    },
    PrefixPayloadConcat {
        prefix: String,
        payload_path: String,
        separator: String,
    },
    PrefixRootConcat {
        prefix: String,
        path: String,
        separator: String,
    },
    BoolNot {
        path: String,
    },
    BytesLength {
        path: String,
    },
    BytesIsEmpty {
        path: String,
    },
    BytesGet {
        path: String,
        index: u64,
    },
    ListGet {
        path: String,
        index: u64,
    },
    BytesSet {
        path: String,
        index: u64,
        value: u8,
    },
    BytesSlice {
        path: String,
        offset: BytesScalarArg,
        byte_count: BytesScalarArg,
    },
    BytesTake {
        path: String,
        byte_count: BytesScalarArg,
    },
    BytesDrop {
        path: String,
        byte_count: BytesScalarArg,
    },
    BytesZeros {
        byte_count: u64,
    },
    BytesToHex {
        path: String,
    },
    BytesFromHex {
        path: String,
    },
    BytesToBase64 {
        path: String,
    },
    BytesFromBase64 {
        path: String,
    },
    BytesReadUnsigned {
        path: String,
        offset: u64,
        byte_count: u64,
        endian: String,
    },
    BytesReadSigned {
        path: String,
        offset: u64,
        byte_count: u64,
        endian: String,
    },
    BytesWriteUnsigned {
        path: String,
        offset: u64,
        byte_count: u64,
        endian: String,
        value: i64,
    },
    BytesWriteSigned {
        path: String,
        offset: u64,
        byte_count: u64,
        endian: String,
        value: i64,
    },
    HostEffect {
        operation: String,
        call_expr_id: ExprId,
        arguments: Vec<HostEffectCallArgument>,
    },
    BytesFind {
        haystack: String,
        needle: String,
    },
    BytesStartsWith {
        path: String,
        prefix: String,
    },
    BytesEndsWith {
        path: String,
        suffix: String,
    },
    TextToBytes {
        path: String,
        encoding: String,
    },
    TextToNumber {
        path: String,
    },
    BytesToText {
        path: String,
        encoding: String,
    },
    BytesConcat {
        left: String,
        right: String,
    },
    BytesEqual {
        left: String,
        right: String,
    },
    MatchConst {
        input: String,
        arms: Vec<UpdateMatchArm>,
    },
    MatchValueConst {
        input: String,
        arms: Vec<UpdateValueMatchArm>,
    },
    MatchTextIsEmptyConst {
        input: String,
        arms: Vec<UpdateValueMatchArm>,
    },
    MatchInfixConst {
        left: UpdateValueExpression,
        op: String,
        right: UpdateValueExpression,
        arms: Vec<UpdateValueMatchArm>,
    },
    Unknown {
        summary: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListOperation {
    pub list_id: ListId,
    pub list: String,
    pub kind: ListOperationKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UnboundListOperation {
    list: String,
    kind: ListOperationKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ListOperationKind {
    Append {
        trigger: String,
        fields: Vec<ListAppendField>,
    },
    Remove {
        source: String,
        predicate: ListPredicate,
    },
    Retain {
        target: String,
        predicate: ListPredicate,
    },
    Count {
        target: String,
        predicate: ListPredicate,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListAppendField {
    pub name: String,
    pub value: ListAppendFieldValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ListAppendFieldValue {
    Source { path: String },
    Const { value: String },
    TypedConst { value: InitialValue },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ListPredicate {
    AlwaysTrue,
    RowFieldBool { path: String },
    RowFieldBoolNot { path: String },
    SelectedFilterVisibility { selector: String, row_field: String },
    Unknown { summary: String },
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
    pub source_id: Option<SourceId>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ViewBindingTarget {
    Storage {
        binding: StorageBindingId,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        projection: Vec<String>,
    },
    MaterializationLocal {
        owner: StaticOwnerId,
        local: MaterializationLocalId,
        projection: Vec<String>,
    },
    ExternalExpression {
        expression: ExecutableExprId,
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
    lower_with_typecheck(program, external_types, true)
}

pub fn lower_runtime_with_external_types(
    program: &ParsedProgram,
    external_types: &boon_typecheck::ExternalTypeEnvironment,
) -> Result<ErasedProgram, String> {
    lower_with_typecheck(program, external_types, false)
}

fn lower_with_typecheck(
    program: &ParsedProgram,
    external_types: &boon_typecheck::ExternalTypeEnvironment,
    include_type_hints: bool,
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
    validate_checked_program_for_lowering(&checked_program)?;
    let out_net = out_net::OutNet::build(&checked_program);
    if out_net.has_errors() {
        return Err(out_net
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "));
    }
    let mut distributed_references = distributed_references(&checked_program, external_types)?;
    let nodes_started = Instant::now();
    let nodes = source_driven_nodes(program);
    let nodes_ms = lower_elapsed_ms(nodes_started);
    trace_phase("source_driven_nodes", nodes_ms);
    let fields_started = Instant::now();
    let fields = typed_field_defs(program);
    bind_distributed_reference_aliases(&fields, &mut distributed_references.value_references);
    let fields_ms = lower_elapsed_ms(fields_started);
    trace_phase("typed_field_defs", fields_ms);
    let direct_sources_started = Instant::now();
    let mut direct_sources = direct_source_refs_by_path(&fields, program, &checked_program)?;
    add_distributed_event_source_refs(
        &fields,
        &distributed_references.value_references,
        &mut direct_sources,
    );
    let direct_sources_ms = lower_elapsed_ms(direct_sources_started);
    trace_phase("direct_source_refs", direct_sources_ms);
    let row_scopes_started = Instant::now();
    let mut row_scopes = row_scopes(program);
    let row_scopes_ms = lower_elapsed_ms(row_scopes_started);
    trace_phase("row_scopes", row_scopes_ms);
    let sources_started = Instant::now();
    let mut sources = program
        .source_ports
        .iter()
        .enumerate()
        .map(|(id, source)| SourcePort {
            id: SourceId(id),
            binding_path: source.binding_path.clone(),
            executable_source_id: None,
            static_owner: None,
            source_expr_id: source.expr_id.map(ExprId),
            source_line: source.line,
            scoped: source.scoped,
            scope_id: scope_id_for_path(&row_scopes, &source.path),
            interval_ms: source.interval_ms,
            payload_schema: source_payload_schema(
                program,
                &fields,
                &direct_sources,
                &typecheck_report,
                &source.path,
            ),
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
                row_lookup_field: None,
            },
        });
    }
    let sources_ms = lower_elapsed_ms(sources_started);
    trace_phase("sources", sources_ms);
    let state_cells_started = Instant::now();
    let mut state_cells = program
        .state_cells
        .iter()
        .enumerate()
        .map(|(id, cell)| {
            let field = fields.iter().find(|field| field.path == cell.path);
            let mut expression_ids = cell
                .expr_id
                .into_iter()
                .map(ExprId)
                .chain(
                    field
                        .into_iter()
                        .flat_map(|field| field.ast_exprs.iter())
                        .map(|expr| ExprId(expr.id)),
                )
                .collect::<Vec<_>>();
            expression_ids.sort_unstable();
            expression_ids.dedup();
            StateCell {
                id: StateId(id),
                path: cell.path.clone(),
                semantic_path: None,
                executable_state_id: None,
                static_owner: None,
                statement_id: field.map_or(usize::MAX, |field| field.statement.id),
                scope_id: scope_id_for_path(&row_scopes, &cell.path),
                hold_name: cell.hold_name.clone(),
                initial_value: field
                    .map(|field| field_initial_value(field, &row_scopes, &fields))
                    .unwrap_or_else(|| InitialValue::Unknown {
                        summary: "missing initial value".to_owned(),
                    }),
                initial_expr_id: field
                    .and_then(field_initial_expr)
                    .map(|expr| ExprId(expr.id)),
                expression_ids,
                indexed: cell.indexed,
                source_line: cell.line,
            }
        })
        .collect::<Vec<_>>();
    let state_cells_ms = lower_elapsed_ms(state_cells_started);
    trace_phase("state_cells", state_cells_ms);
    let mut immediate_dependencies =
        immediate_field_dependencies(&fields, &state_cells, &typecheck_report);
    let verify_cycles_started = Instant::now();
    verify_combinational_field_cycles(program, &fields, &state_cells)?;
    let verify_cycles_ms = lower_elapsed_ms(verify_cycles_started);
    trace_phase("verify_combinational_field_cycles", verify_cycles_ms);
    let lists_started = Instant::now();
    let mut lists = program
        .list_memories
        .iter()
        .filter(|list| !is_output_registry_value_path(&list.name))
        .enumerate()
        .map(|(id, list)| ListMemory {
            id: ListId(id),
            name: list.name.clone(),
            row_scope_id: scope_id_for_list(&row_scopes, &list.name),
            hidden_key_type: hidden_key_type(&list.name),
            has_generation: true,
            graph_clones_per_item: 0,
            capacity: list.capacity,
            initializer: list_initializer(program, list),
        })
        .collect::<Vec<_>>();
    let lists_ms = lower_elapsed_ms(lists_started);
    trace_phase("lists", lists_ms);
    if nodes
        .iter()
        .any(|node| matches!(node.kind, IrNodeKind::ListMap) && !node.indexed)
    {
        return Err("List/map node must be indexed".to_owned());
    }
    let dependencies_started = Instant::now();
    let mut candidate_sources = CandidateSourceIndex::new(&fields, &direct_sources, &state_cells);
    let mut dependencies = dependency_edges(program, &state_cells, &mut candidate_sources);
    let dependencies_ms = lower_elapsed_ms(dependencies_started);
    trace_phase("dependency_edges", dependencies_ms);
    let possible_causes_started = Instant::now();
    let mut possible_causes = possible_causes(&state_cells, &mut candidate_sources);
    let possible_causes_ms = lower_elapsed_ms(possible_causes_started);
    trace_phase("possible_causes", possible_causes_ms);
    let update_branches_started = Instant::now();
    let resolved_constants = ResolvedConstantLookup::new(&typecheck_report);
    let mut update_branches = update_branches(
        program,
        &state_cells,
        &fields,
        &direct_sources,
        &mut candidate_sources,
        &resolved_constants,
    );
    verify_host_effect_calls_scheduled(program, &update_branches)?;
    let update_branches_ms = lower_elapsed_ms(update_branches_started);
    trace_phase("update_branches", update_branches_ms);
    let list_operations_started = Instant::now();
    let unbound_list_operations = unbound_list_operations(program);
    let list_projections_started = Instant::now();
    let list_projections = list_projections(program);
    let list_projections_ms = lower_elapsed_ms(list_projections_started);
    trace_phase("list_projections", list_projections_ms);
    let functions_started = Instant::now();
    let functions = function_definitions(program);
    let functions_ms = lower_elapsed_ms(functions_started);
    trace_phase("function_definitions", functions_ms);
    let (mut materializations, materialization_expressions) =
        contextual_materializations(&checked_program, &out_net.graph)?;
    let mut executable = contextual_expansion::derive_executable_program(
        &checked_program,
        &out_net.graph,
        &materializations,
        &distributed_references,
        materialization_expressions,
    )
    .map_err(|error| error.to_string())?;
    let derived_list_storage =
        materialize_typed_derived_list_storage(&executable, &mut row_scopes, &mut lists)?;
    let mut list_operations = bind_list_operations(unbound_list_operations, &lists)?;
    let list_operations_ms = lower_elapsed_ms(list_operations_started);
    trace_phase("list_operations", list_operations_ms);
    let materialization_target_lists =
        materialization_target_lists(&executable, &materializations, &derived_list_storage)?;
    let mut resource_aliases = bind_executable_state_resources(
        &executable,
        &materialization_target_lists,
        &lists,
        &mut state_cells,
    )?;
    let source_aliases = bind_executable_source_resources(
        &checked_program,
        &executable,
        &materialization_target_lists,
        &lists,
        program.source_ports.len(),
        &mut sources,
    )?;
    merge_resource_aliases(&mut resource_aliases, source_aliases)?;
    canonicalize_update_branches(&mut update_branches, &resource_aliases);
    canonicalize_runtime_resource_metadata(
        &mut immediate_dependencies,
        &mut dependencies,
        &mut possible_causes,
        &mut list_operations,
        &mut state_cells,
        &resource_aliases,
    );
    bind_contextual_materialization_storage(
        &executable,
        &derived_list_storage,
        &row_scopes,
        &lists,
        &sources,
        &state_cells,
        &mut materializations,
    )?;
    let derived_values_started = Instant::now();
    let mut derived_values = derived_values(
        program,
        &executable,
        &row_scopes,
        &derived_list_storage,
        &fields,
        &state_cells,
        &sources,
        &materializations,
        &distributed_references.value_references,
    )?;
    for value in &mut derived_values {
        value.path = canonical_resource_path(&value.path, &resource_aliases);
        for source in &mut value.sources {
            *source = canonical_resource_path(source, &resource_aliases);
        }
    }
    let derived_values_ms = lower_elapsed_ms(derived_values_started);
    trace_phase("derived_values", derived_values_ms);
    let semantic_fields = semantic_field_entries(
        &fields,
        &row_scopes,
        &state_cells,
        &lists,
        &derived_list_storage,
    );
    let storage = build_storage_catalog(
        &executable,
        &out_net.graph.static_owners,
        &sources,
        &state_cells,
        &lists,
        &derived_list_storage,
        &derived_values,
        &semantic_fields,
    )?;
    bind_executable_storage_reads(&mut executable, &storage, &out_net.graph.static_owners)?;
    let output_values_started = Instant::now();
    let output_values = output_root_values(program, &typecheck_report, &executable, &storage)?;
    let output_values_ms = lower_elapsed_ms(output_values_started);
    trace_phase("output_values", output_values_ms);
    let view_bindings_started = Instant::now();
    let view_bindings = view_bindings(
        &executable,
        &storage,
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
        program,
        &nodes,
        &state_cells,
        &lists,
        &derived_values,
        &update_branches,
        &list_operations,
        &distributed_references,
    );
    let expression_coverage_ms = lower_elapsed_ms(expression_coverage_started);
    trace_phase("expression_coverage", expression_coverage_ms);
    let semantic_index_started = Instant::now();
    let semantic_index = semantic_index(
        program,
        &row_scopes,
        &sources,
        &lists,
        &functions,
        &view_bindings,
        &typecheck_report,
        semantic_fields,
    );
    let semantic_index_ms = lower_elapsed_ms(semantic_index_started);
    trace_phase("semantic_index", semantic_index_ms);
    let semantic_migration_started = Instant::now();
    let (semantic_memory, migration_edges) = lower_semantic_memory_and_migrations(
        program,
        &fields,
        &row_scopes,
        &state_cells,
        &lists,
        &derived_list_storage,
        &typecheck_report,
    )?;
    let semantic_migration_ms = lower_elapsed_ms(semantic_migration_started);
    trace_phase("semantic_memory_and_migrations", semantic_migration_ms);
    let typed = ErasedProgram {
        kind: program.kind,
        executable,
        storage,
        expression_count: program.expressions.len(),
        expressions: program.expressions.clone(),
        expression_coverage,
        distributed_references,
        semantic_index,
        graph_node_count: nodes.len(),
        nodes,
        row_scopes,
        sources,
        host_ports: host_port_declarations(&typecheck_report),
        output_values,
        dependencies,
        possible_causes,
        update_branches,
        list_operations,
        list_projections,
        functions,
        static_owners: out_net.graph.static_owners.clone(),
        materializations,
        view_bindings,
        expression_types: typecheck_report.expr_type_table,
        function_types: typecheck_report.function_type_table,
        named_value_types: typecheck_report.named_value_type_table,
        derived_values,
        immediate_dependencies,
        state_cells,
        lists,
        semantic_memory,
        migration_edges,
        hidden_identity_verified: true,
        static_schedule_verified: true,
    };
    let verify_static_started = Instant::now();
    verify_storage_catalog(&typed)?;
    verify_static_schedule(&typed)?;
    let verify_static_ms = lower_elapsed_ms(verify_static_started);
    trace_phase("verify_static_schedule", verify_static_ms);
    let verify_hidden_started = Instant::now();
    verify_hidden_identity(&typed)?;
    let verify_hidden_ms = lower_elapsed_ms(verify_hidden_started);
    trace_phase("verify_hidden_identity", verify_hidden_ms);
    Ok(typed)
}

fn build_storage_catalog(
    executable: &ExecutableProgram,
    static_owners: &[StaticOwnerDef],
    sources: &[SourcePort],
    states: &[StateCell],
    lists: &[ListMemory],
    list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    derived_values: &[DerivedValue],
    semantic_fields: &[SemanticFieldEntry],
) -> Result<StorageCatalog, String> {
    let mut bindings = Vec::new();
    for statement in &executable.statements {
        let (Some(declaration), Some(flow_type), Some(producer)) = (
            statement.declaration,
            statement.flow_type.clone(),
            statement.value,
        ) else {
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
            list_storage
                .get(&statement.id)
                .map(|storage| storage.list_id)
                .ok_or_else(|| {
                    format!(
                        "typed list declaration {} (`{diagnostic_path}`) has no allocated ListId",
                        declaration.0
                    )
                })?
                .into()
        } else {
            None
        };
        let derived_value = derived_values
            .iter()
            .find(|value| value.executable_statement_id == statement.id);
        let field = if list.is_none()
            && let Some(derived_value) = derived_value
        {
            let candidates = semantic_fields
                .iter()
                .filter(|field| {
                    field.statement_id == statement.id.0 && field.scope_id == derived_value.scope_id
                })
                .collect::<Vec<_>>();
            let [field] = candidates.as_slice() else {
                return Err(format!(
                    "derived storage declaration {} (`{diagnostic_path}`) statement {} scope {:?} has {} semantic fields",
                    declaration.0,
                    statement.id,
                    derived_value.scope_id,
                    candidates.len()
                ));
            };
            Some(field.id)
        } else {
            None
        };
        let row_scope = list.and_then(|list_id| {
            lists
                .get(list_id.as_usize())
                .filter(|list| list.id == list_id)
                .and_then(|list| list.row_scope_id)
        });
        bindings.push(StorageBinding {
            id: StorageBindingId(bindings.len()),
            declaration,
            static_owner: expression.owner,
            owner_ancestry: static_owner_ancestry(expression.owner, static_owners)?,
            flow_type,
            producer,
            diagnostic_path,
            kind: StorageBindingKind::Value {
                field,
                list,
                row_scope,
            },
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
        bindings.push(StorageBinding {
            id: StorageBindingId(bindings.len()),
            declaration: source.declaration,
            static_owner: source.owner,
            owner_ancestry: static_owner_ancestry(source.owner, static_owners)?,
            flow_type: expression.flow_type.clone(),
            producer: source.expression,
            diagnostic_path: runtime.path.clone(),
            kind: StorageBindingKind::Source {
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
        bindings.push(StorageBinding {
            id: StorageBindingId(bindings.len()),
            declaration: state.declaration,
            static_owner: state.owner,
            owner_ancestry: static_owner_ancestry(state.owner, static_owners)?,
            flow_type: expression.flow_type.clone(),
            producer: state.expression,
            diagnostic_path: runtime.path.clone(),
            kind: StorageBindingKind::State {
                executable: state.id,
                runtime: runtime.id,
            },
        });
    }
    let mut identities = BTreeSet::new();
    for binding in &bindings {
        let kind = match binding.kind {
            StorageBindingKind::Value { .. } => 0_u8,
            StorageBindingKind::Source { .. } => 1,
            StorageBindingKind::State { .. } => 2,
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
    Ok(StorageCatalog { bindings })
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

fn storage_binding_for_read(
    storage: &StorageCatalog,
    static_owners: &[StaticOwnerDef],
    declaration: boon_typecheck::DeclId,
    owner: Option<StaticOwnerId>,
) -> Result<StorageBindingId, String> {
    let mut lexical_owners = static_owner_ancestry(owner, static_owners)?;
    lexical_owners.reverse();
    let lexical_owners = lexical_owners
        .into_iter()
        .map(Some)
        .chain(std::iter::once(None));
    for lexical_owner in lexical_owners {
        let candidates = storage
            .bindings
            .iter()
            .filter(|binding| {
                binding.declaration == declaration && binding.static_owner == lexical_owner
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            continue;
        }
        let preferred_kind = candidates
            .iter()
            .map(|binding| match binding.kind {
                StorageBindingKind::Value { .. } => 0_u8,
                StorageBindingKind::State { .. } | StorageBindingKind::Source { .. } => 1,
            })
            .min()
            .expect("non-empty candidates");
        let preferred = candidates
            .into_iter()
            .filter(|binding| {
                (match binding.kind {
                    StorageBindingKind::Value { .. } => 0_u8,
                    StorageBindingKind::State { .. } | StorageBindingKind::Source { .. } => 1,
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

fn bind_executable_storage_reads(
    executable: &mut ExecutableProgram,
    storage: &StorageCatalog,
    static_owners: &[StaticOwnerDef],
) -> Result<(), String> {
    let expression_owners = executable
        .expressions
        .iter()
        .map(|expression| (expression.id, expression.owner))
        .collect::<BTreeMap<_, _>>();
    for expression in &mut executable.expressions {
        let target = match &expression.kind {
            ExecutableExpressionKind::CanonicalRead { target, .. }
            | ExecutableExpressionKind::Drain { target, .. } => Some(*target),
            _ => None,
        };
        let Some(target) = target else {
            continue;
        };
        let binding = storage_binding_for_read(storage, static_owners, target, expression.owner)
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
                format!(
                    "{error}; executable read {} from checked expression {} ({:?}) sees declaration producers {:?}, sources {:?}, states {:?}",
                    expression.id,
                    expression.checked_expr_id.0,
                    expression.kind,
                    producers,
                    sources,
                    states
                )
            })?;
        match &mut expression.kind {
            ExecutableExpressionKind::CanonicalRead {
                storage_binding, ..
            }
            | ExecutableExpressionKind::Drain {
                storage_binding, ..
            } => *storage_binding = Some(binding),
            _ => unreachable!("target was read from this expression kind"),
        }
    }
    Ok(())
}

fn verify_storage_catalog(program: &ErasedProgram) -> Result<(), String> {
    for (index, binding) in program.storage.bindings.iter().enumerate() {
        if binding.id != StorageBindingId(index) {
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
                .static_owners
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
        match binding.kind {
            StorageBindingKind::Value {
                field,
                list,
                row_scope,
            } => {
                if let Some(field) = field
                    && !program
                        .semantic_index
                        .fields
                        .iter()
                        .any(|value| value.id == field)
                {
                    return Err(format!(
                        "storage binding {} references missing semantic FieldId {field}",
                        binding.id
                    ));
                }
                if let Some(list) = list {
                    let memory = program
                        .lists
                        .get(list.as_usize())
                        .filter(|memory| memory.id == list)
                        .ok_or_else(|| {
                            format!(
                                "storage binding {} references missing ListId {list}",
                                binding.id
                            )
                        })?;
                    if memory.row_scope_id != row_scope {
                        return Err(format!(
                            "storage binding {} row scope differs from ListId {list}",
                            binding.id
                        ));
                    }
                } else if row_scope.is_some() {
                    return Err(format!(
                        "storage binding {} has a row scope without a ListId",
                        binding.id
                    ));
                }
            }
            StorageBindingKind::Source {
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
            StorageBindingKind::State {
                executable,
                runtime,
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
    for statement in &program.executable.statements {
        let Some(declaration) = statement.declaration else {
            continue;
        };
        let Some(flow_type) = &statement.flow_type else {
            return Err(format!(
                "executable declaration {} statement {} has no final checked type",
                declaration.0, statement.id
            ));
        };
        if !matches!(&flow_type.ty, boon_typecheck::Type::List(_)) {
            continue;
        }
        let matches = program
            .storage
            .bindings
            .iter()
            .filter(|binding| {
                binding.declaration == declaration
                    && matches!(
                        binding.kind,
                        StorageBindingKind::Value { list: Some(_), .. }
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
) -> Result<DistributedReferences, String> {
    let mut references = DistributedReferences::default();
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
                references.value_references.push(DistributedValueReference {
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
                if !signature.pure {
                    return Err(format!(
                        "typecheck accepted impure external function `{function}`"
                    ));
                }
                ensure_distributed_flow_is_closed(
                    &expr.flow_type,
                    &format!("qualified external call `{function}` result"),
                )?;
                ensure_distributed_flow_is_closed(
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
                    ensure_distributed_type_is_closed(
                        &declared_argument.ty,
                        &format!("external function `{function}` argument `{name}`"),
                    )?;
                    let actual = program
                        .expressions
                        .get(value.0 as usize)
                        .map(|expression| &expression.flow_type)
                        .ok_or_else(|| {
                            format!(
                                "qualified external call `{function}` argument `{name}` references missing expression {}",
                                value.0
                            )
                        })?;
                    ensure_distributed_flow_is_closed(
                        actual,
                        &format!("qualified external call `{function}` argument `{name}`"),
                    )?;
                    arguments.push(DistributedPureCallArgument {
                        name: name.clone(),
                        expr_id: ExprId(value.0 as usize),
                        argument_type: declared_argument.ty.clone(),
                    });
                }
                references.pure_calls.push(DistributedPureCall {
                    expr_id: ExprId(expr.id.0 as usize),
                    canonical_function: function.clone(),
                    producer_role,
                    result_type: signature.result.ty.clone(),
                    arguments,
                });
            }
            _ => {}
        }
    }
    Ok(references)
}

fn distributed_function_role(function: &str) -> Option<boon_typecheck::ProgramRole> {
    function
        .split_once('/')
        .and_then(|(namespace, _)| distributed_role(namespace))
}

fn distributed_role(namespace: &str) -> Option<boon_typecheck::ProgramRole> {
    match namespace {
        "Client" => Some(boon_typecheck::ProgramRole::Client),
        "Session" => Some(boon_typecheck::ProgramRole::Session),
        "Server" => Some(boon_typecheck::ProgramRole::Server),
        _ => None,
    }
}

fn ensure_distributed_flow_is_closed(
    flow_type: &boon_typecheck::FlowType,
    context: &str,
) -> Result<(), String> {
    if flow_type.mode != boon_typecheck::FlowMode::Continuous {
        return Err(format!("{context} is not continuous"));
    }
    ensure_distributed_type_is_closed(&flow_type.ty, context)
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
    program: &ParsedProgram,
    row_scopes: &[RowScope],
    sources: &[SourcePort],
    lists: &[ListMemory],
    functions: &[FunctionDefinition],
    view_bindings: &[ViewBinding],
    typecheck_report: &boon_typecheck::TypeCheckReport,
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
    let source_units = program
        .files
        .iter()
        .enumerate()
        .map(|(id, file)| SemanticSourceUnit {
            id: SourceUnitId(id),
            path: file.path.clone(),
            module: file.module.clone(),
            start_line: file.start_line,
            line_count: file.source.lines().count().max(1),
        })
        .collect::<Vec<_>>();
    let output_roots = semantic_output_roots(program, typecheck_report);
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
    let functions = functions
        .iter()
        .enumerate()
        .map(|(id, function)| SemanticFunctionEntry {
            id: FunctionId(id),
            name: function.name.clone(),
            args: function.args.clone(),
            statement_id: function.statement.id,
            line: function.statement.line,
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
            source_id: binding.source_id,
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
        program,
        &output_roots,
        &sources,
        &lists,
        &row_scopes,
        &functions,
        &semantic_fields,
        &view_bindings,
    );
    let readiness = semantic_index_readiness(
        &sources,
        row_scopes.len(),
        lists.len(),
        program,
        typecheck_report,
    );
    SemanticIndex {
        version: 1,
        computed_from: "parser_ast_ir_typecheck_tables".to_owned(),
        parser_policy_phase: "syntax_parse_then_semantic_index_policy_checks".to_owned(),
        reuse_key: semantic_index_reuse_key(program, &readiness),
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
            parser_reused_by_ir: true,
            typecheck_reused_by_ir: true,
            runtime_reports_reuse_index: true,
            shared_tables: vec![
                "ParsedProgram.source_ports".to_owned(),
                "ParsedProgram.list_memories".to_owned(),
                "ParsedProgram.row_scope_functions".to_owned(),
                "TypeCheckReport.source_payload_shape_table".to_owned(),
                "TypeCheckReport.render_slot_table".to_owned(),
                "ErasedProgram.semantic_index.output_roots".to_owned(),
                "ErasedProgram.view_bindings".to_owned(),
            ],
        },
    }
}

fn semantic_field_entries(
    fields: &[FieldDef],
    row_scopes: &[RowScope],
    state_cells: &[StateCell],
    lists: &[ListMemory],
    derived_list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
) -> Vec<SemanticFieldEntry> {
    let mut semantic_fields = fields
        .iter()
        .enumerate()
        .map(|(id, field)| SemanticFieldEntry {
            id: FieldId(id),
            path: field.path.clone(),
            local_name: field.local_name.clone(),
            parent_path: field.parent_path.clone(),
            scope_id: scope_id_for_path(row_scopes, &field.path),
            statement_id: field.statement.id,
            line: field.statement.line,
            kind: semantic_field_kind(field, state_cells, lists),
        })
        .collect::<Vec<_>>();
    for storage in derived_list_storage.values() {
        let path = &storage.path;
        let source = fields.iter().find(|field| field.path == *path);
        for local_name in &storage.item_fields {
            if semantic_fields.iter().any(|field| {
                field.scope_id == Some(storage.row_scope_id) && field.local_name == *local_name
            }) {
                continue;
            }
            semantic_fields.push(SemanticFieldEntry {
                id: FieldId(semantic_fields.len()),
                path: format!("{path}.{local_name}"),
                local_name: local_name.clone(),
                parent_path: path.clone(),
                scope_id: Some(storage.row_scope_id),
                statement_id: source.map_or(usize::MAX, |field| field.statement.id),
                line: source.map_or(0, |field| field.statement.line),
                kind: "materialized_field".to_owned(),
            });
        }
    }
    semantic_fields
}

fn semantic_output_roots(
    program: &ParsedProgram,
    typecheck_report: &boon_typecheck::TypeCheckReport,
) -> Vec<SemanticOutputRootEntry> {
    output_root_declarations(program, typecheck_report)
        .into_iter()
        .map(|declaration| SemanticOutputRootEntry {
            root: declaration.root,
            contract: declaration.contract,
            demand: SemanticOutputDemandPolicy::HostDemanded,
            data_type: declaration.data_type,
            statement_id: declaration.statement.id,
            line: declaration.statement.line,
            typed_contract_known: declaration.typed_contract_known,
        })
        .collect()
}

fn output_root_values(
    program: &ParsedProgram,
    typecheck_report: &boon_typecheck::TypeCheckReport,
    executable: &ExecutableProgram,
    storage: &StorageCatalog,
) -> Result<Vec<OutputRootValue>, String> {
    output_root_declarations(program, typecheck_report)
        .into_iter()
        .map(|declaration| {
            let executable_statement_id = ExecutableStatementId(declaration.statement.id);
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
            let bindings = storage
                .bindings
                .iter()
                .filter(|binding| {
                    binding.declaration == declaration_id
                        && binding.producer == value_expression_id
                        && matches!(binding.kind, StorageBindingKind::Value { .. })
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
            Ok(OutputRootValue {
                root: declaration.root,
                value_path: declaration.value_path,
                contract: declaration.contract,
                demand: SemanticOutputDemandPolicy::HostDemanded,
                data_type: declaration.data_type,
                statement_id: declaration.statement.id,
                executable_statement_id,
                value_expression_id,
                storage_binding_id: binding.id,
                line: declaration.statement.line,
                typed_contract_known: declaration.typed_contract_known,
                statement: declaration.statement.clone(),
            })
        })
        .collect()
}

struct OutputRootDeclaration<'a> {
    root: String,
    value_path: String,
    contract: SemanticOutputContractKind,
    data_type: Option<SemanticDataType>,
    typed_contract_known: bool,
    statement: &'a AstStatement,
}

fn output_root_declarations<'a>(
    program: &'a ParsedProgram,
    typecheck_report: &boon_typecheck::TypeCheckReport,
) -> Vec<OutputRootDeclaration<'a>> {
    let mut declarations = Vec::new();
    for statement in &program.ast.statements {
        let AstStatementKind::Field { name } = &statement.kind else {
            continue;
        };
        let visual_kind = match name.as_str() {
            "document" => Some(SemanticRetainedVisualKind::Document),
            "scene" => Some(SemanticRetainedVisualKind::Scene),
            _ => None,
        };
        if let Some(kind) = visual_kind {
            declarations.push(OutputRootDeclaration {
                root: name.clone(),
                value_path: name.clone(),
                contract: SemanticOutputContractKind::RetainedVisual { kind },
                data_type: None,
                typed_contract_known: retained_visual_contract_known(
                    kind,
                    statement,
                    program,
                    typecheck_report,
                ),
                statement,
            });
            continue;
        }
        if name != "outputs" {
            continue;
        }
        for output in &statement.children {
            let name = match &output.kind {
                AstStatementKind::Field { name }
                | AstStatementKind::List {
                    field: Some(name), ..
                } => name,
                _ => continue,
            };
            let data_type = typecheck_report
                .output_root_types
                .iter()
                .find(|entry| entry.statement_id == output.id && entry.name == *name)
                .map(|entry| semantic_data_type(&entry.ty));
            declarations.push(OutputRootDeclaration {
                root: name.clone(),
                value_path: format!("outputs.{name}"),
                contract: SemanticOutputContractKind::HostValue,
                typed_contract_known: data_type.as_ref().is_some_and(semantic_data_type_is_closed),
                data_type,
                statement: output,
            });
        }
    }
    declarations.sort_by(|left, right| left.root.cmp(&right.root));
    declarations
}

fn retained_visual_contract_known(
    kind: SemanticRetainedVisualKind,
    statement: &AstStatement,
    program: &ParsedProgram,
    typecheck_report: &boon_typecheck::TypeCheckReport,
) -> bool {
    if typecheck_report.has_errors() {
        return false;
    }
    match kind {
        SemanticRetainedVisualKind::Document => {
            statement_contains_constructor(statement, program, "Document/")
                || statement_contains_constructor(statement, program, "Element/")
        }
        SemanticRetainedVisualKind::Scene => {
            statement_contains_constructor(statement, program, "Scene/")
        }
    }
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

fn statement_contains_constructor(
    statement: &AstStatement,
    program: &ParsedProgram,
    prefix: &str,
) -> bool {
    collect_statement_ast_exprs(statement, program)
        .iter()
        .any(|expr| match &expr.kind {
            AstExprKind::Call { function, .. } => function.starts_with(prefix),
            AstExprKind::Pipe { op, .. } => op.starts_with(prefix),
            _ => false,
        })
        || statement
            .children
            .iter()
            .any(|child| statement_contains_constructor(child, program, prefix))
}

#[allow(clippy::too_many_arguments)]
fn semantic_symbols(
    program: &ParsedProgram,
    output_roots: &[SemanticOutputRootEntry],
    sources: &[SemanticSourceEntry],
    lists: &[SemanticListEntry],
    row_scopes: &[SemanticRowScopeEntry],
    functions: &[SemanticFunctionEntry],
    fields: &[SemanticFieldEntry],
    view_bindings: &[SemanticViewBindingEntry],
) -> Vec<SemanticSymbolEntry> {
    let mut table = SemanticSymbolTable::default();
    for file in &program.files {
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
    for operator in &program.operators {
        table.intern("operator_name", operator);
    }
    for expr in &program.expressions {
        match &expr.kind {
            AstExprKind::Enum(tag) | AstExprKind::Tag(tag) => {
                table.intern("tag", tag);
            }
            AstExprKind::TaggedObject { tag, fields } => {
                table.intern("tag", tag);
                for field in fields {
                    table.intern("document_attr", &field.name);
                }
            }
            AstExprKind::Object(fields) | AstExprKind::Record(fields) => {
                for field in fields {
                    table.intern("document_attr", &field.name);
                    table.intern("style_attr", &field.name);
                }
            }
            AstExprKind::Call { function, args, .. } => {
                table.intern("operator_name", function);
                for arg in args {
                    if let Some(name) = arg.named_name() {
                        table.intern("document_attr", name);
                    }
                }
            }
            AstExprKind::Pipe { op, args, .. } => {
                table.intern("operator_name", op);
                for arg in args {
                    if let Some(name) = arg.named_name() {
                        table.intern("document_attr", name);
                    }
                }
            }
            _ => {}
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

fn semantic_field_kind(
    field: &FieldDef,
    state_cells: &[StateCell],
    lists: &[ListMemory],
) -> String {
    if state_cells.iter().any(|cell| cell.path == field.path) {
        "state_cell".to_owned()
    } else if lists
        .iter()
        .any(|list| field.path == list.name || field.path.ends_with(&format!(".{}", list.name)))
    {
        "list_memory".to_owned()
    } else {
        "derived_value".to_owned()
    }
}

fn semantic_index_readiness(
    sources: &[SemanticSourceEntry],
    row_scope_count: usize,
    list_count: usize,
    program: &ParsedProgram,
    typecheck_report: &boon_typecheck::TypeCheckReport,
) -> SemanticIndexReadiness {
    let source_payload_fallbacks = sources
        .iter()
        .filter(|source| !source.payload_schema_known)
        .map(|source| format!("{} has no source payload shape entry", source.path))
        .collect::<Vec<_>>();
    let row_scope_fallbacks = if list_count > 0 && row_scope_count == 0 {
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
    let row_scope_ambiguity_fallbacks = row_scope_ambiguity_reasons(program);
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
            known_count: row_scope_count,
            fallback_count: row_scope_fallbacks.len(),
            fallback_reasons: row_scope_fallbacks,
        },
        row_scope_ambiguity: SemanticKnowledgeStatus {
            known_count: row_scope_count,
            fallback_count: row_scope_ambiguity_fallbacks.len(),
            fallback_reasons: row_scope_ambiguity_fallbacks,
        },
        selectors: SemanticKnowledgeStatus {
            known_count: program.list_memories.len(),
            fallback_count: selector_fallbacks.len(),
            fallback_reasons: selector_fallbacks.clone(),
        },
        selector_index_ambiguity: SemanticKnowledgeStatus {
            known_count: program.list_memories.len(),
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
    typecheck_report: &boon_typecheck::TypeCheckReport,
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

fn row_scope_ambiguity_reasons(program: &ParsedProgram) -> Vec<String> {
    let mut seen = BTreeMap::<&str, &str>::new();
    let mut reasons = Vec::new();
    for scope in &program.row_scope_functions {
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

fn selector_fallback_reasons(program: &ParsedProgram) -> Vec<String> {
    program
        .expressions
        .iter()
        .filter_map(|expr| match &expr.kind {
            AstExprKind::Unknown(tokens) if tokens.iter().any(|token| token.contains("List/")) => {
                Some(format!(
                    "list selector expression at line {} was parsed as unknown",
                    expr.line
                ))
            }
            _ => None,
        })
        .collect()
}

fn semantic_index_reuse_key(program: &ParsedProgram, readiness: &SemanticIndexReadiness) -> String {
    format!(
        "semantic-index-v1:{}:{}:{}:{}:{}:{}",
        program.path,
        program.files.len(),
        program.source_ports.len(),
        program.list_memories.len(),
        program.row_scope_functions.len(),
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
    if program.graph_node_count != program.nodes.len() {
        return Err(format!(
            "graph_node_count {} does not match {} scheduled nodes",
            program.graph_node_count,
            program.nodes.len()
        ));
    }
    for (index, node) in program.nodes.iter().enumerate() {
        if node.id.as_usize() != index {
            return Err(format!(
                "scheduled node `{}` has id {}, expected {index}",
                node.name, node.id
            ));
        }
        if node
            .expr_id
            .is_some_and(|expr_id| expr_id.as_usize() >= program.expression_count)
        {
            return Err(format!(
                "scheduled node `{}` references missing ExprId {:?}",
                node.name, node.expr_id
            ));
        }
        if matches!(
            node.kind,
            IrNodeKind::ListAppend
                | IrNodeKind::ListRemove
                | IrNodeKind::ListMap
                | IrNodeKind::ListRetain
                | IrNodeKind::Aggregate
                | IrNodeKind::RenderLowering
        ) && !node.indexed
        {
            return Err(format!(
                "scheduled collection node `{}` is not indexed/keyed",
                node.name
            ));
        }
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
    for (index, value) in program.derived_values.iter().enumerate() {
        if value.id.as_usize() != index {
            return Err(format!(
                "derived value `{}` has FieldId {}, expected {index}",
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
            .storage
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
        let StorageBindingKind::Value {
            list: Some(list),
            row_scope: Some(row_scope),
            ..
        } = binding.kind
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
        match binding.kind {
            ViewBindingKind::Source => {
                let Some(source_id) = binding.source_id else {
                    return Err(format!(
                        "view source binding `{}.{}` has no SourceId",
                        binding.node_kind, binding.attr
                    ));
                };
                if source_id.as_usize() >= program.sources.len()
                    || program.sources[source_id.as_usize()].path != binding.path
                {
                    return Err(format!(
                        "view source binding `{}.{}` does not match SourceId {:?}",
                        binding.node_kind, binding.attr, binding.source_id
                    ));
                }
            }
            ViewBindingKind::Data | ViewBindingKind::Target => {
                if binding.source_id.is_some() {
                    return Err(format!(
                        "view data binding `{}.{}` unexpectedly has SourceId {:?}",
                        binding.node_kind, binding.attr, binding.source_id
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
    for edge in &program.immediate_dependencies {
        require_known_symbol(
            "immediate dependency dependent",
            &edge.dependent,
            &known_symbols,
        )?;
        require_known_symbol(
            "immediate dependency dependency",
            &edge.dependency,
            &known_symbols,
        )?;
    }
    for cause in &program.possible_causes {
        require_known_symbol("cause target", &cause.target, &known_symbols)?;
        for source in &cause.sources {
            require_known_symbol("cause source", source, &known_symbols)?;
        }
    }
    let effect_result_states = program
        .update_branches
        .iter()
        .filter_map(|branch| {
            matches!(&branch.expression, UpdateExpression::HostEffect { .. })
                .then_some(branch.target.as_str())
        })
        .collect::<BTreeSet<_>>();
    for branch in &program.update_branches {
        if !state_paths.contains(branch.target.as_str()) {
            return Err(format!(
                "update branch target `{}` is not a scheduled state cell",
                branch.target
            ));
        }
        if !source_paths.contains(branch.source.as_str())
            && !state_paths.contains(branch.source.as_str())
        {
            return Err(format!(
                "update branch trigger `{}` is neither a declared source nor a state cell",
                branch.source
            ));
        }
        if matches!(&branch.expression, UpdateExpression::HostEffect { .. })
            && state_paths.contains(branch.source.as_str())
            && !effect_result_states.contains(branch.source.as_str())
        {
            return Err(format!(
                "host effect update trigger `{}` is not a typed host-effect result state",
                branch.source
            ));
        }
        verify_scheduled_update_expression(
            &branch.expression,
            &branch.target,
            &branch.source,
            &known_symbols,
        )
        .map_err(|error| {
            format!(
                "update branch `{}` from `{}` failed static schedule: {error}; expression={:?}",
                branch.target, branch.source, branch.expression
            )
        })?;
    }
    for operation in &program.list_operations {
        let Some(list) = program.lists.get(operation.list_id.as_usize()) else {
            return Err(format!(
                "list operation references missing ListId {} for `{}`",
                operation.list_id, operation.list
            ));
        };
        if list.id != operation.list_id || list.name != operation.list {
            return Err(format!(
                "list operation ListId {} resolves to `{}`, not `{}`",
                operation.list_id, list.name, operation.list
            ));
        }
        verify_scheduled_list_operation(&operation.kind, &source_paths, &known_symbols)?;
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
            .is_some_and(|owner| owner.as_usize() >= program.static_owners.len())
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
        if materialization.owner.as_usize() >= program.static_owners.len() {
            return Err(format!(
                "contextual materialization {} references missing static owner {}",
                materialization.id, materialization.owner
            ));
        }
        for (label, root) in [
            ("source", materialization.source),
            ("body", materialization.body),
        ] {
            if root.as_usize() >= expressions.len() {
                return Err(format!(
                    "contextual materialization {} {label} references missing expression {}",
                    materialization.id, root
                ));
            }
        }
        let mut ancestor_locals = BTreeSet::new();
        let mut ancestor = program.static_owners[materialization.owner.as_usize()].parent;
        while let Some(owner) = ancestor {
            let local = local_by_owner.get(&owner).copied().ok_or_else(|| {
                format!(
                    "contextual materialization {} has ancestor owner {} without a row local",
                    materialization.id, owner
                )
            })?;
            ancestor_locals.insert((owner, local));
            ancestor = program.static_owners[owner.as_usize()].parent;
        }
        verify_materialization_locals(
            expressions,
            materialization.source,
            &ancestor_locals,
            materialization.id,
        )?;
        let mut body_locals = ancestor_locals;
        body_locals.insert((materialization.owner, materialization.row_local));
        verify_materialization_locals(
            expressions,
            materialization.body,
            &body_locals,
            materialization.id,
        )?;
    }
    Ok(())
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
        | ExecutableExpressionKind::ExternalRead { .. }
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
        ExecutableExpressionKind::TaggedObject { fields, .. }
        | ExecutableExpressionKind::Object(fields)
        | ExecutableExpressionKind::Record(fields) => {
            fields.iter().map(|field| field.value).collect()
        }
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
        + program.distributed_references.pure_calls.len();
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
        .nodes
        .iter()
        .filter_map(|node| node.expr_id)
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

    for call in &program.distributed_references.pure_calls {
        if !reference_expr_ids.insert(call.expr_id) {
            return Err(format!(
                "distributed expression {} is represented more than once",
                call.expr_id
            ));
        }
        require_scheduled_distributed_expr(call.expr_id, &scheduled_expr_ids)?;
        if distributed_function_role(&call.canonical_function) != Some(call.producer_role) {
            return Err(format!(
                "distributed call `{}` does not match producer role {:?}",
                call.canonical_function, call.producer_role
            ));
        }
        verify_distributed_metadata_type(
            program,
            call.expr_id,
            boon_typecheck::FlowMode::Continuous,
            &call.result_type,
            &format!("distributed call `{}` result", call.canonical_function),
        )?;
        let mut names = BTreeSet::new();
        for argument in &call.arguments {
            if !names.insert(argument.name.as_str()) {
                return Err(format!(
                    "distributed call `{}` repeats argument `{}`",
                    call.canonical_function, argument.name
                ));
            }
            require_scheduled_distributed_expr(argument.expr_id, &scheduled_expr_ids)?;
            let context = format!(
                "distributed call `{}` argument `{}`",
                call.canonical_function, argument.name
            );
            ensure_distributed_type_is_closed(&argument.argument_type, &context)?;
            let checked =
                distributed_expr_type(&program.expression_types, argument.expr_id.as_usize())?;
            ensure_distributed_flow_is_closed(checked, &context)?;
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

fn row_scopes(program: &ParsedProgram) -> Vec<RowScope> {
    let mut scopes = Vec::new();
    for scope in &program.row_scope_functions {
        if scopes.iter().any(|existing: &RowScope| {
            existing.list == scope.list && existing.row_scope == scope.row_scope
        }) {
            continue;
        }
        scopes.push(RowScope {
            id: ScopeId(scopes.len()),
            list: scope.list.clone(),
            function: scope.function.clone(),
            row_scope: scope.row_scope.clone(),
        });
    }
    scopes
}

fn scope_id_for_path(row_scopes: &[RowScope], path: &str) -> Option<ScopeId> {
    path.split('.').find_map(|segment| {
        row_scopes
            .iter()
            .find(|scope| scope.row_scope == segment)
            .map(|scope| scope.id)
    })
}

fn scope_id_for_list(row_scopes: &[RowScope], list: &str) -> Option<ScopeId> {
    row_scopes
        .iter()
        .find(|scope| scope.list == list)
        .map(|scope| scope.id)
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

fn typed_derived_list_targets(
    executable: &ExecutableProgram,
) -> Result<Vec<TypedDerivedListTarget>, String> {
    let mut targets = Vec::new();
    let mut seen = BTreeSet::new();
    for statement in &executable.statements {
        let Some((name, path)) = executable_statement_name_path(&statement.kind) else {
            continue;
        };
        let Some(value) = statement.value else {
            continue;
        };
        let Some(declaration) = statement.declaration else {
            return Err(format!(
                "typed list-valued statement {} has no checked declaration",
                statement.id
            ));
        };
        let expression = executable
            .expressions
            .get(value.as_usize())
            .ok_or_else(|| {
                format!("typed field `{path}` references missing executable expression {value}")
            })?;
        let boon_typecheck::Type::List(item_type) = &expression.flow_type.ty else {
            continue;
        };
        if !seen.insert(declaration) {
            return Err(format!(
                "checked declaration {} has more than one executable list storage target",
                declaration.0
            ));
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
            item_fields: typed_item_field_names(item_type),
        });
    }
    Ok(targets)
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
    executable: &ExecutableProgram,
    row_scopes: &mut Vec<RowScope>,
    lists: &mut Vec<ListMemory>,
) -> Result<BTreeMap<ExecutableStatementId, DerivedListStorageIds>, String> {
    let targets = typed_derived_list_targets(executable)?;
    let mut target_paths_by_local = BTreeMap::<String, Vec<String>>::new();
    for target in &targets {
        target_paths_by_local
            .entry(target.local_name.clone())
            .or_default()
            .push(target.path.clone());
    }
    for paths in target_paths_by_local.values_mut() {
        paths.sort();
        paths.dedup();
    }
    for list in lists.iter_mut() {
        if targets.iter().any(|target| target.path == list.name) {
            continue;
        }
        let Some(paths) = target_paths_by_local.get(&list.name) else {
            continue;
        };
        let [path] = paths.as_slice() else {
            return Err(format!(
                "parsed list storage `{}` ambiguously matches checked paths {}",
                list.name,
                paths.join(", ")
            ));
        };
        list.name.clone_from(path);
    }
    for scope in row_scopes.iter_mut() {
        if targets.iter().any(|target| target.path == scope.list) {
            continue;
        }
        let Some(paths) = target_paths_by_local.get(&scope.list) else {
            continue;
        };
        let [path] = paths.as_slice() else {
            return Err(format!(
                "row scope `{}` ambiguously refers to typed derived lists {}",
                scope.row_scope,
                paths.join(", ")
            ));
        };
        scope.list.clone_from(path);
    }
    let target_paths = targets
        .iter()
        .map(|target| target.path.as_str())
        .collect::<BTreeSet<_>>();
    lists.retain(|list| target_paths.contains(list.name.as_str()));
    for (id, list) in lists.iter_mut().enumerate() {
        list.id = ListId(id);
        list.row_scope_id = scope_id_for_list(row_scopes, &list.name);
    }

    let mut storage = BTreeMap::new();
    for target in targets {
        let existing = lists.iter().position(|list| list.name == target.path);
        let list_id = existing.map_or(ListId(lists.len()), ListId);
        let matching_scopes = row_scopes
            .iter()
            .filter(|scope| scope.list == target.path)
            .map(|scope| scope.id)
            .collect::<Vec<_>>();
        let row_scope_id = match matching_scopes.as_slice() {
            [scope] => *scope,
            [] => {
                let scope = ScopeId(row_scopes.len());
                row_scopes.push(RowScope {
                    id: scope,
                    list: target.path.clone(),
                    function: "typed_list".to_owned(),
                    row_scope: format!("list_{}_row", list_id.as_usize()),
                });
                scope
            }
            _ => {
                return Err(format!(
                    "typed derived list target `{}` has ambiguous row scopes {:?}",
                    target.path, matching_scopes
                ));
            }
        };
        if let Some(index) = existing {
            lists[index].id = list_id;
            lists[index].row_scope_id = Some(row_scope_id);
            if lists[index].capacity.is_none() {
                lists[index].capacity = target.capacity;
            }
        } else {
            lists.push(ListMemory {
                id: list_id,
                name: target.path.clone(),
                row_scope_id: Some(row_scope_id),
                hidden_key_type: hidden_key_type(&target.path),
                has_generation: true,
                graph_clones_per_item: 0,
                capacity: target.capacity,
                initializer: ListInitializer::Empty,
            });
        }
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

fn insert_resource_alias(
    aliases: &mut BTreeMap<String, String>,
    from: &str,
    to: &str,
) -> Result<(), String> {
    if let Some(previous) = aliases.insert(from.to_owned(), to.to_owned())
        && previous != to
    {
        return Err(format!(
            "runtime resource alias `{from}` resolves to both `{previous}` and `{to}`"
        ));
    }
    Ok(())
}

fn merge_resource_aliases(
    aliases: &mut BTreeMap<String, String>,
    additions: BTreeMap<String, String>,
) -> Result<(), String> {
    for (from, to) in additions {
        insert_resource_alias(aliases, &from, &to)?;
    }
    Ok(())
}

fn canonical_resource_path(path: &str, aliases: &BTreeMap<String, String>) -> String {
    if let Some(canonical) = aliases.get(path) {
        return canonical.clone();
    }
    aliases
        .iter()
        .filter_map(|(alias, canonical)| {
            path.strip_prefix(alias)
                .filter(|suffix| suffix.starts_with('.'))
                .map(|suffix| (alias.len(), format!("{canonical}{suffix}")))
        })
        .max_by_key(|(length, _)| *length)
        .map_or_else(|| path.to_owned(), |(_, canonical)| canonical)
}

fn canonicalize_update_branches(branches: &mut [UpdateBranch], aliases: &BTreeMap<String, String>) {
    for branch in branches {
        branch.target = canonical_resource_path(&branch.target, aliases);
        branch.source = canonical_resource_path(&branch.source, aliases);
        if let Some(guard) = &mut branch.guard {
            canonicalize_update_guard(guard, aliases);
        }
        canonicalize_update_expression(&mut branch.expression, aliases);
    }
}

fn canonicalize_runtime_resource_metadata(
    immediate_dependencies: &mut [ImmediateDependency],
    dependencies: &mut [DependencyEdge],
    possible_causes: &mut [PossibleCause],
    list_operations: &mut [ListOperation],
    state_cells: &mut [StateCell],
    aliases: &BTreeMap<String, String>,
) {
    for dependency in immediate_dependencies {
        dependency.dependent = canonical_resource_path(&dependency.dependent, aliases);
        dependency.dependency = canonical_resource_path(&dependency.dependency, aliases);
    }
    for dependency in dependencies {
        dependency.from = canonical_resource_path(&dependency.from, aliases);
        dependency.to = canonical_resource_path(&dependency.to, aliases);
    }
    for cause in possible_causes {
        cause.target = canonical_resource_path(&cause.target, aliases);
        for source in &mut cause.sources {
            *source = canonical_resource_path(source, aliases);
        }
        cause.sources.sort();
        cause.sources.dedup();
    }
    for operation in list_operations {
        operation.list = canonical_resource_path(&operation.list, aliases);
        match &mut operation.kind {
            ListOperationKind::Append { trigger, fields } => {
                *trigger = canonical_resource_path(trigger, aliases);
                for field in fields {
                    match &mut field.value {
                        ListAppendFieldValue::Source { path } => {
                            *path = canonical_resource_path(path, aliases);
                        }
                        ListAppendFieldValue::TypedConst { value } => {
                            canonicalize_initial_value(value, aliases);
                        }
                        ListAppendFieldValue::Const { .. } => {}
                    }
                }
            }
            ListOperationKind::Remove { source, predicate } => {
                *source = canonical_resource_path(source, aliases);
                canonicalize_list_predicate(predicate, aliases);
            }
            ListOperationKind::Retain { target, predicate }
            | ListOperationKind::Count { target, predicate } => {
                *target = canonical_resource_path(target, aliases);
                canonicalize_list_predicate(predicate, aliases);
            }
        }
    }
    for state in state_cells {
        canonicalize_initial_value(&mut state.initial_value, aliases);
    }
}

fn canonicalize_initial_value(value: &mut InitialValue, aliases: &BTreeMap<String, String>) {
    match value {
        InitialValue::RootInitialField { path } | InitialValue::RowInitialField { path } => {
            *path = canonical_resource_path(path, aliases);
        }
        InitialValue::Text { .. }
        | InitialValue::Number { .. }
        | InitialValue::Bool { .. }
        | InitialValue::Bytes { .. }
        | InitialValue::Enum { .. }
        | InitialValue::Data { .. }
        | InitialValue::Unknown { .. } => {}
    }
}

fn canonicalize_list_predicate(predicate: &mut ListPredicate, aliases: &BTreeMap<String, String>) {
    match predicate {
        ListPredicate::RowFieldBool { path } | ListPredicate::RowFieldBoolNot { path } => {
            *path = canonical_resource_path(path, aliases);
        }
        ListPredicate::SelectedFilterVisibility {
            selector,
            row_field,
        } => {
            *selector = canonical_resource_path(selector, aliases);
            *row_field = canonical_resource_path(row_field, aliases);
        }
        ListPredicate::AlwaysTrue | ListPredicate::Unknown { .. } => {}
    }
}

fn canonicalize_update_guard(guard: &mut UpdateGuard, aliases: &BTreeMap<String, String>) {
    match guard {
        UpdateGuard::ValueOneOf { input, .. } | UpdateGuard::ListIsNotEmpty { input, .. } => {
            *input = canonical_resource_path(input, aliases);
        }
        UpdateGuard::ValuesEqual { left, right } | UpdateGuard::ValuesNotEqual { left, right } => {
            *left = canonical_resource_path(left, aliases);
            *right = canonical_resource_path(right, aliases);
        }
        UpdateGuard::All { guards } => {
            for guard in guards {
                canonicalize_update_guard(guard, aliases);
            }
        }
    }
}

fn canonicalize_update_value_expression(
    expression: &mut UpdateValueExpression,
    aliases: &BTreeMap<String, String>,
) {
    match expression {
        UpdateValueExpression::Const { .. } => {}
        UpdateValueExpression::ReadPath { path } => {
            *path = canonical_resource_path(path, aliases);
        }
        UpdateValueExpression::MatchConst { input, arms }
        | UpdateValueExpression::MatchTextIsEmptyConst { input, arms } => {
            *input = canonical_resource_path(input, aliases);
            for arm in arms {
                canonicalize_update_value_expression(&mut arm.output, aliases);
            }
        }
        UpdateValueExpression::NumberInfix { left, right, .. }
        | UpdateValueExpression::MatchInfixConst { left, right, .. } => {
            *left = canonical_resource_path(left, aliases);
            *right = canonical_resource_path(right, aliases);
            if let UpdateValueExpression::MatchInfixConst { arms, .. } = expression {
                for arm in arms {
                    canonicalize_update_value_expression(&mut arm.output, aliases);
                }
            }
        }
    }
}

fn canonicalize_bytes_scalar_arg(
    argument: &mut BytesScalarArg,
    aliases: &BTreeMap<String, String>,
) {
    if let BytesScalarArg::Path(path) = argument {
        *path = canonical_resource_path(path, aliases);
    }
}

fn canonicalize_update_expression(
    expression: &mut UpdateExpression,
    aliases: &BTreeMap<String, String>,
) {
    match expression {
        UpdateExpression::SourcePayload { path }
        | UpdateExpression::PreviousValue { path }
        | UpdateExpression::ReadPath { path }
        | UpdateExpression::BoolNot { path }
        | UpdateExpression::BytesLength { path }
        | UpdateExpression::BytesIsEmpty { path }
        | UpdateExpression::BytesGet { path, .. }
        | UpdateExpression::ListGet { path, .. }
        | UpdateExpression::BytesSet { path, .. }
        | UpdateExpression::BytesToHex { path }
        | UpdateExpression::BytesFromHex { path }
        | UpdateExpression::BytesToBase64 { path }
        | UpdateExpression::BytesFromBase64 { path }
        | UpdateExpression::BytesReadUnsigned { path, .. }
        | UpdateExpression::BytesReadSigned { path, .. }
        | UpdateExpression::BytesWriteUnsigned { path, .. }
        | UpdateExpression::BytesWriteSigned { path, .. }
        | UpdateExpression::TextToBytes { path, .. }
        | UpdateExpression::TextToNumber { path }
        | UpdateExpression::BytesToText { path, .. } => {
            *path = canonical_resource_path(path, aliases);
        }
        UpdateExpression::NumberInfix { left, right, .. }
        | UpdateExpression::BytesConcat { left, right }
        | UpdateExpression::BytesEqual { left, right } => {
            *left = canonical_resource_path(left, aliases);
            *right = canonical_resource_path(right, aliases);
        }
        UpdateExpression::ProjectTime {
            pointer_x,
            pointer_width,
            viewport_start,
            viewport_end,
            fallback,
        } => {
            for path in [
                pointer_x,
                pointer_width,
                viewport_start,
                viewport_end,
                fallback,
            ] {
                *path = canonical_resource_path(path, aliases);
            }
        }
        UpdateExpression::TextTrimOrPrevious { path, previous } => {
            *path = canonical_resource_path(path, aliases);
            *previous = canonical_resource_path(previous, aliases);
        }
        UpdateExpression::PrefixPayloadConcat { payload_path, .. } => {
            *payload_path = canonical_resource_path(payload_path, aliases);
        }
        UpdateExpression::PrefixRootConcat { path, .. } => {
            *path = canonical_resource_path(path, aliases);
        }
        UpdateExpression::BytesSlice {
            path,
            offset,
            byte_count,
        } => {
            *path = canonical_resource_path(path, aliases);
            canonicalize_bytes_scalar_arg(offset, aliases);
            canonicalize_bytes_scalar_arg(byte_count, aliases);
        }
        UpdateExpression::BytesTake { path, byte_count }
        | UpdateExpression::BytesDrop { path, byte_count } => {
            *path = canonical_resource_path(path, aliases);
            canonicalize_bytes_scalar_arg(byte_count, aliases);
        }
        UpdateExpression::BytesFind { haystack, needle } => {
            *haystack = canonical_resource_path(haystack, aliases);
            *needle = canonical_resource_path(needle, aliases);
        }
        UpdateExpression::BytesStartsWith { path, prefix } => {
            *path = canonical_resource_path(path, aliases);
            *prefix = canonical_resource_path(prefix, aliases);
        }
        UpdateExpression::BytesEndsWith { path, suffix } => {
            *path = canonical_resource_path(path, aliases);
            *suffix = canonical_resource_path(suffix, aliases);
        }
        UpdateExpression::MatchConst { input, .. }
        | UpdateExpression::MatchValueConst { input, .. }
        | UpdateExpression::MatchTextIsEmptyConst { input, .. } => {
            *input = canonical_resource_path(input, aliases);
            if let UpdateExpression::MatchValueConst { arms, .. }
            | UpdateExpression::MatchTextIsEmptyConst { arms, .. } = expression
            {
                for arm in arms {
                    canonicalize_update_value_expression(&mut arm.output, aliases);
                }
            }
        }
        UpdateExpression::MatchInfixConst {
            left, right, arms, ..
        } => {
            canonicalize_update_value_expression(left, aliases);
            canonicalize_update_value_expression(right, aliases);
            for arm in arms {
                canonicalize_update_value_expression(&mut arm.output, aliases);
            }
        }
        UpdateExpression::Const { .. }
        | UpdateExpression::BytesZeros { .. }
        | UpdateExpression::HostEffect { .. }
        | UpdateExpression::Unknown { .. } => {}
    }
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

fn state_metadata_matches_checked_expression(
    state: &StateCell,
    checked_expression: ExprId,
) -> bool {
    state.expression_ids.contains(&checked_expression)
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
) -> Result<BTreeMap<String, String>, String> {
    fn merge_metadata(
        source: ExecutableSourceId,
        records: &[&SourcePort],
    ) -> Result<(Option<u64>, SourcePayloadSchema), String> {
        let mut interval_ms = None;
        let mut fields = BTreeSet::new();
        let mut typed_fields = BTreeMap::new();
        let mut row_lookup_field = None;
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
            if let Some(candidate) = &record.payload_schema.row_lookup_field {
                if let Some(existing) = &row_lookup_field
                    && existing != candidate
                {
                    return Err(format!(
                        "executable source {source} has conflicting row lookup fields `{existing}` and `{candidate}`"
                    ));
                }
                row_lookup_field = Some(candidate.clone());
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
                row_lookup_field,
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
    let mut aliases = BTreeMap::new();
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
                    && target.is_none_or(|(_, list)| source.scope_id == list.row_scope_id)
            })
            .collect::<Vec<_>>();
        if matches.is_empty() {
            return Err(format!(
                "executable source {} (`{}` owner {:?}) has no parser metadata record; available {:?}",
                executable_source.id,
                executable_source.binding_path,
                executable_source.owner,
                parser_sources
                    .iter()
                    .map(|source| (&source.path, &source.binding_path, source.scope_id))
                    .collect::<Vec<_>>()
            ));
        }
        let (interval_ms, payload_schema) =
            merge_metadata(executable_source.id, matches.as_slice())?;
        let canonical_path = target.map_or_else(
            || executable_source.binding_path.clone(),
            |(path, _)| format!("{path}.{}", executable_source.binding_path),
        );
        for metadata in matches {
            insert_resource_alias(&mut aliases, &metadata.path, &canonical_path)?;
            insert_resource_alias(&mut aliases, &metadata.binding_path, &canonical_path)?;
        }
        let scope_id = target.and_then(|(_, list)| list.row_scope_id);
        bound.push(SourcePort {
            id: SourceId(bound.len()),
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

fn bind_executable_state_resources(
    executable: &ExecutableProgram,
    materialization_targets: &BTreeMap<StaticOwnerId, ListId>,
    lists: &[ListMemory],
    states: &mut [StateCell],
) -> Result<BTreeMap<String, String>, String> {
    let mut aliases = BTreeMap::new();
    for executable_state in &executable.states {
        let executable_expression = executable
            .expressions
            .get(executable_state.expression.as_usize())
            .filter(|expression| expression.id == executable_state.expression)
            .ok_or_else(|| {
                format!(
                    "executable state {} has no executable expression",
                    executable_state.id
                )
            })?;
        let checked_expression = ExprId(executable_expression.checked_expr_id.0 as usize);
        let target = executable_state
            .owner
            .and_then(|owner| materialization_targets.get(&owner))
            .and_then(|list| lists.get(list.as_usize()))
            .map(|list| (list.name.as_str(), list));
        let available = states
            .iter()
            .map(|state| (state.path.clone(), state.scope_id))
            .collect::<Vec<_>>();
        let mut matches = states
            .iter_mut()
            .filter(|state| {
                state_metadata_matches_checked_expression(state, checked_expression)
                    && target.is_none_or(|(_, list)| state.scope_id == list.row_scope_id)
            })
            .collect::<Vec<_>>();
        let [state] = matches.as_mut_slice() else {
            return Err(format!(
                "executable state {} (`{}` owner {:?}) matched {} runtime state cells; available {:?}",
                executable_state.id,
                executable_state.binding_path,
                executable_state.owner,
                matches.len(),
                available
            ));
        };
        if let Some(existing) = state.executable_state_id
            && existing != executable_state.id
        {
            return Err(format!(
                "runtime state `{}` aliases executable states {} and {}",
                state.path, existing, executable_state.id
            ));
        }
        let semantic_path = target.map_or_else(
            || {
                is_canonical_resource_path(&executable_state.binding_path)
                    .then(|| executable_state.binding_path.clone())
                    .unwrap_or_else(|| state.path.clone())
            },
            |(target, _)| format!("{target}.{}", executable_state.binding_path),
        );
        insert_resource_alias(&mut aliases, &state.path, &semantic_path)?;
        state.path.clone_from(&semantic_path);
        state.semantic_path = Some(semantic_path);
        state.executable_state_id = Some(executable_state.id);
        state.static_owner = executable_state.owner;
    }
    Ok(aliases)
}

fn materialization_target_lists(
    executable: &ExecutableProgram,
    materializations: &[ContextualMaterialization],
    list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
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
                let owner = materializations
                    .get(materialization)
                    .filter(|candidate| candidate.id == materialization)
                    .ok_or_else(|| {
                        format!(
                            "state resource ownership reaches missing materialization {materialization}"
                        )
                    })?
                    .owner;
                if let Some(existing) = owner_targets.insert(owner, storage.list_id)
                    && existing != storage.list_id
                {
                    return Err(format!(
                        "static owner {owner} ambiguously materializes ListId {existing} and {}",
                        storage.list_id
                    ));
                }
            }
            pending.extend(executable_expression_children(&expression.kind));
        }
    }
    Ok(owner_targets)
}

fn source_payload_schema(
    program: &ParsedProgram,
    fields: &[FieldDef],
    direct_sources: &BTreeMap<String, Vec<String>>,
    typecheck_report: &boon_typecheck::TypeCheckReport,
    source: &str,
) -> SourcePayloadSchema {
    let variants = source_ref_variants(source);
    let mut payload_fields = BTreeSet::new();
    for field in fields {
        if !direct_sources_for_field(direct_sources, field)
            .any(|direct_source| direct_source == source)
        {
            continue;
        }
        for variant in &variants {
            payload_fields.extend(field.referenced_payload_fields(variant));
        }
    }
    let row_lookup_field = source_row_lookup_field(program, fields, source);
    if row_lookup_field.is_some() {
        payload_fields.insert(SourcePayloadField::Address);
    }
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
    payload_fields.extend(typed_payload_fields.keys().cloned());
    SourcePayloadSchema {
        fields: payload_fields.iter().cloned().collect(),
        typed_fields: payload_fields
            .iter()
            .cloned()
            .map(|field| SourcePayloadDescriptor {
                data_type: typed_payload_fields
                    .get(&field)
                    .cloned()
                    .unwrap_or_else(|| source_payload_data_type(&field)),
                field,
            })
            .collect(),
        row_lookup_field,
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

fn host_port_declarations(report: &boon_typecheck::TypeCheckReport) -> Vec<HostPortDeclaration> {
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

fn source_row_lookup_field(
    program: &ParsedProgram,
    fields: &[FieldDef],
    source: &str,
) -> Option<String> {
    let source_scope = source.split('.').next()?;
    let scope = program
        .row_scope_functions
        .iter()
        .find(|scope| scope.row_scope == source_scope);
    if let Some(scope) = scope
        && let Some(explicit_address) = fields.iter().find_map(|field| {
            field
                .path
                .strip_prefix(&format!("{}.", scope.row_scope))
                .filter(|lookup| *lookup == "address")
                .or_else(|| {
                    field
                        .path
                        .rsplit_once(&format!(".{}.", scope.row_scope))
                        .and_then(|(_, lookup)| (lookup == "address").then_some(lookup))
                })
                .map(str::to_owned)
        })
    {
        return Some(explicit_address);
    }
    let mut candidates = Vec::new();
    for field in fields {
        let Some(branch) = field.source_branch(source) else {
            continue;
        };
        let Some(SimpleThenUpdateValue::Path(path)) = branch.then_simple_update_value() else {
            continue;
        };
        if let Some(scope) = scope {
            let canonical =
                canonical_scalar_update_path_for_source(field, &field.path, &path, fields, source);
            if let Some(lookup) = canonical
                .strip_prefix(&format!("{}.", scope.row_scope))
                .filter(|lookup| !lookup.contains('.'))
            {
                candidates.push(lookup.to_owned());
            }
        } else if store_list_source_tail(source, program).is_some()
            && let Some((row_alias, lookup)) = path.split_once('.')
            && row_alias != "store"
            && !lookup.contains('.')
        {
            candidates.push(lookup.to_owned());
        }
    }
    select_source_row_lookup_field(source, candidates)
}

fn select_source_row_lookup_field(source: &str, candidates: Vec<String>) -> Option<String> {
    candidates
        .into_iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            let score = source_row_lookup_field_score(source, &candidate);
            (score > 0).then_some((index, candidate, score))
        })
        .max_by_key(|(index, _, score)| (*score, std::cmp::Reverse(*index)))
        .map(|(_, candidate, _)| candidate)
}

fn source_row_lookup_field_score(source: &str, candidate: &str) -> i32 {
    let terms = source_row_lookup_intent_terms(source);
    let mut score = 0;
    if matches!(candidate, "id" | "key" | "unique_id") {
        score += 50;
    }
    if candidate.ends_with("_id") || candidate.ends_with("_key") {
        score += 45;
    }
    if candidate == "file" {
        score += 25;
    }
    for term in terms {
        if candidate == term {
            score += 120;
        }
        if candidate == format!("{term}_key") || candidate == format!("{term}_id") {
            score += 130;
        }
        if candidate.starts_with(&format!("{term}_")) {
            score += 80;
        }
        if candidate.contains(&term) {
            score += 50;
        }
    }
    score
}

fn source_row_lookup_intent_terms(source: &str) -> Vec<String> {
    let mut terms = BTreeSet::new();
    for segment in source.split('.') {
        for part in segment.split('_') {
            if matches!(part, "select" | "row" | "rows" | "element" | "elements") {
                continue;
            }
            if part.is_empty() {
                continue;
            }
            terms.insert(part.to_owned());
        }
    }
    terms.into_iter().collect()
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
    storage: &StorageCatalog,
    list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    output_values: &[OutputRootValue],
    row_scopes: &[RowScope],
    sources: &[SourcePort],
    states: &[StateCell],
    materializations: &[ContextualMaterialization],
) -> Result<Vec<ViewBinding>, String> {
    let mut collector = ExecutableViewBindingCollector::new(
        executable,
        Some(storage),
        list_storage,
        row_scopes,
        sources,
        states,
        materializations,
    );
    collector.collect_output_roots(output_values)?;
    let mut bindings = collector.bindings;
    normalize_view_binding_ids(&mut bindings);
    Ok(bindings)
}

fn bind_contextual_materialization_storage(
    executable: &ExecutableProgram,
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
            None,
            list_storage,
            row_scopes,
            sources,
            states,
            materializations,
        );
        materializations
            .iter()
            .map(|materialization| {
                let scope =
                    resolver.local_scope(materialization.owner, materialization.row_local)?;
                let list = scope
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
            binding.source_id.map(SourceId::as_usize),
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
enum ExecutableViewRead {
    Canonical {
        declaration: Option<boon_typecheck::DeclId>,
        owner: Option<StaticOwnerId>,
        path: String,
        projection: Vec<String>,
        storage_binding: Option<StorageBindingId>,
        expression: ExecutableExprId,
    },
    Local {
        owner: StaticOwnerId,
        local: MaterializationLocalId,
        projection: Vec<String>,
    },
}

struct ExecutableViewBindingCollector<'a> {
    executable: &'a ExecutableProgram,
    storage: Option<&'a StorageCatalog>,
    row_scopes: &'a [RowScope],
    sources: &'a [SourcePort],
    materializations: &'a [ContextualMaterialization],
    statement_values: BTreeMap<boon_typecheck::DeclId, ExecutableExprId>,
    list_scopes_by_declaration: BTreeMap<boon_typecheck::DeclId, ScopeId>,
    materializations_by_local: BTreeMap<(StaticOwnerId, MaterializationLocalId), usize>,
    states_by_declaration: BTreeMap<(boon_typecheck::DeclId, Option<StaticOwnerId>), StateId>,
    host_effect_states: BTreeSet<StateId>,
    local_scope_cache: BTreeMap<(StaticOwnerId, MaterializationLocalId), Option<ScopeId>>,
    local_scope_visiting: BTreeSet<(StaticOwnerId, MaterializationLocalId)>,
    render_visited: BTreeSet<ExecutableExprId>,
    bindings: Vec<ViewBinding>,
}

impl<'a> ExecutableViewBindingCollector<'a> {
    fn new(
        executable: &'a ExecutableProgram,
        storage: Option<&'a StorageCatalog>,
        list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
        row_scopes: &'a [RowScope],
        sources: &'a [SourcePort],
        states: &[StateCell],
        materializations: &'a [ContextualMaterialization],
    ) -> Self {
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
            .filter_map(|state| {
                let executable_state = state
                    .executable_state_id
                    .and_then(|id| executable.states.get(id.as_usize()))?;
                Some(((executable_state.declaration, state.static_owner), state.id))
            })
            .collect();
        let mut collector = Self {
            executable,
            storage,
            row_scopes,
            sources,
            materializations,
            statement_values,
            list_scopes_by_declaration,
            materializations_by_local,
            states_by_declaration,
            host_effect_states: BTreeSet::new(),
            local_scope_cache: BTreeMap::new(),
            local_scope_visiting: BTreeSet::new(),
            render_visited: BTreeSet::new(),
            bindings: Vec::new(),
        };
        collector.host_effect_states = states
            .iter()
            .filter_map(|state| {
                let executable_state_id = state.executable_state_id?;
                let executable_state = collector
                    .executable
                    .states
                    .get(executable_state_id.as_usize())
                    .filter(|definition| definition.id == executable_state_id)?;
                collector
                    .expression_invokes_host(executable_state.expression, &mut BTreeSet::new())
                    .then_some(state.id)
            })
            .collect();
        collector
    }

    fn expression_invokes_host(
        &self,
        id: ExecutableExprId,
        visited: &mut BTreeSet<ExecutableExprId>,
    ) -> bool {
        if !visited.insert(id) {
            return false;
        }
        let Ok(expression) = self.expression(id) else {
            return false;
        };
        if expression.effect.invokes_host {
            return true;
        }
        let children = match &expression.kind {
            ExecutableExpressionKind::CanonicalRead { target, .. } => self
                .statement_values
                .get(target)
                .copied()
                .filter(|value| *value != id)
                .into_iter()
                .collect(),
            ExecutableExpressionKind::Call { arguments, .. } => {
                arguments.iter().map(|argument| argument.value).collect()
            }
            ExecutableExpressionKind::Materialize { materialization } => self
                .materializations
                .get(*materialization)
                .map(|materialization| vec![materialization.body])
                .unwrap_or_default(),
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
            ExecutableExpressionKind::Object(fields)
            | ExecutableExpressionKind::Record(fields)
            | ExecutableExpressionKind::TaggedObject { fields, .. } => {
                fields.iter().map(|field| field.value).collect()
            }
            ExecutableExpressionKind::List { items, .. }
            | ExecutableExpressionKind::Bytes { items, .. } => items.clone(),
            ExecutableExpressionKind::ExternalRead { .. }
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
        children
            .into_iter()
            .any(|child| self.expression_invokes_host(child, visited))
    }

    fn event_causes_for_expression(
        &mut self,
        root: ExecutableExprId,
    ) -> Result<Vec<EventCause>, String> {
        let mut causes = BTreeSet::new();
        self.collect_event_causes(
            root,
            &mut causes,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
        )?;
        Ok(causes.into_iter().collect())
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
        let mut arms = BTreeMap::new();
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
        Ok((arms.into_values().collect(), default_roots))
    }

    fn collect_trigger_owned_arms(
        &mut self,
        id: ExecutableExprId,
        visited: &mut BTreeSet<ExecutableExprId>,
        arms: &mut BTreeMap<(EventCause, ExecutableExprId, ExecutableExprId), TriggerOwnedArm>,
    ) -> Result<(), String> {
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
                    self.collect_trigger_owned_arms(arm.output, visited, arms)?;
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
                    self.collect_trigger_owned_arms(output, visited, arms)?;
                }
            }
            ExecutableExpressionKind::Hold { updates, .. }
            | ExecutableExpressionKind::Latest { branches: updates } => {
                for update in updates {
                    self.collect_trigger_owned_arms(update, visited, arms)?;
                }
            }
            ExecutableExpressionKind::Call { arguments, .. } => {
                for argument in arguments {
                    self.collect_trigger_owned_arms(argument.value, visited, arms)?;
                }
            }
            ExecutableExpressionKind::Materialize { materialization } => {
                let body = self.materialization(materialization)?.body;
                self.collect_trigger_owned_arms(body, visited, arms)?;
            }
            ExecutableExpressionKind::Draining { input }
            | ExecutableExpressionKind::Project { input, .. } => {
                self.collect_trigger_owned_arms(input, visited, arms)?;
            }
            ExecutableExpressionKind::Infix { left, right, .. } => {
                self.collect_trigger_owned_arms(left, visited, arms)?;
                self.collect_trigger_owned_arms(right, visited, arms)?;
            }
            ExecutableExpressionKind::MatchArm { output, .. } => {
                if let Some(output) = output {
                    self.collect_trigger_owned_arms(output, visited, arms)?;
                }
            }
            ExecutableExpressionKind::Object(fields)
            | ExecutableExpressionKind::Record(fields)
            | ExecutableExpressionKind::TaggedObject { fields, .. } => {
                for field in fields {
                    self.collect_trigger_owned_arms(field.value, visited, arms)?;
                }
            }
            ExecutableExpressionKind::List { items, .. }
            | ExecutableExpressionKind::Bytes { items, .. } => {
                for item in items {
                    self.collect_trigger_owned_arms(item, visited, arms)?;
                }
            }
            ExecutableExpressionKind::CanonicalRead { .. }
            | ExecutableExpressionKind::ExternalRead { .. }
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
        arms: &mut BTreeMap<(EventCause, ExecutableExprId, ExecutableExprId), TriggerOwnedArm>,
    ) -> Result<(), String> {
        let gate_expression = self.expression(gate)?;
        arms.entry((cause, gate, output))
            .or_insert_with(|| TriggerOwnedArm {
                cause,
                gate_checked_expr_id: gate_expression.checked_expr_id,
                gate_expression_id: gate,
                owner: gate_expression.owner,
                output_expression_id: output,
            });
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
        if let Some(read) = self.direct_view_read_unscoped(id)? {
            let candidates = self.source_candidates(&read)?;
            if !candidates.is_empty() {
                causes.extend(
                    candidates
                        .into_iter()
                        .map(|(_, source_id, _)| EventCause::Source(source_id)),
                );
                return Ok(());
            }
            if let ExecutableViewRead::Canonical {
                declaration,
                owner,
                path: _,
                ..
            } = read
            {
                if let Some(state_id) = declaration.and_then(|declaration| {
                    self.states_by_declaration
                        .get(&(declaration, owner))
                        .or_else(|| self.states_by_declaration.get(&(declaration, None)))
                        .copied()
                }) {
                    if self.host_effect_states.contains(&state_id) {
                        causes.insert(EventCause::State(state_id));
                    }
                    return Ok(());
                }
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
            ExecutableExpressionKind::List { items, .. }
            | ExecutableExpressionKind::Bytes { items, .. } => items,
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
        let Some(read) = self.direct_view_read(id)? else {
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
                target: ViewBindingTarget::Storage {
                    binding: self.storage_binding_for_runtime_source(*source_id)?,
                    projection: Vec::new(),
                },
                kind: ViewBindingKind::Source,
                scope_id: *scope_id,
                source_id: Some(*source_id),
            });
        }
        Ok(candidates.len())
    }

    fn source_candidates(
        &mut self,
        read: &ExecutableViewRead,
    ) -> Result<Vec<(String, SourceId, Option<ScopeId>)>, String> {
        let mut exact = Vec::new();
        let mut grouped = Vec::new();
        match read {
            ExecutableViewRead::Canonical {
                declaration,
                owner,
                path,
                projection,
                ..
            } => {
                if let Some(declaration) = declaration {
                    let exact_sources = self
                        .sources
                        .iter()
                        .filter(|source| {
                            source
                                .executable_source_id
                                .and_then(|id| self.executable.sources.get(id.as_usize()))
                                .is_some_and(|definition| {
                                    definition.declaration == *declaration
                                        && (source.static_owner == *owner
                                            || source.static_owner.is_none())
                                })
                        })
                        .map(|source| (source.path.clone(), source.id, source.scope_id))
                        .collect::<Vec<_>>();
                    if !exact_sources.is_empty() {
                        return Ok(exact_sources);
                    }
                    return Ok(Vec::new());
                }
                let path = canonical_read_path(path, projection);
                let prefix = format!("{path}.");
                let mut projected = Vec::new();
                for source in self.sources {
                    let candidate = (source.path.clone(), source.id, source.scope_id);
                    if source.path == path {
                        exact.push(candidate);
                    } else if path
                        .strip_prefix(&source.path)
                        .is_some_and(|suffix| suffix.starts_with('.'))
                    {
                        projected.push(candidate);
                    } else if source.path.starts_with(&prefix) {
                        grouped.push(candidate);
                    }
                }
                if !projected.is_empty() {
                    return Ok(projected);
                }
            }
            ExecutableViewRead::Local {
                owner,
                local: _,
                projection,
            } => {
                let projection = projection.join(".");
                let prefix = format!("{projection}.");
                let mut pending = vec![*owner];
                let mut visited = BTreeSet::new();
                while let Some(candidate_owner) = pending.pop() {
                    if !visited.insert(candidate_owner) {
                        continue;
                    }
                    let mut owner_exact = Vec::new();
                    let mut owner_grouped = Vec::new();
                    for source in self
                        .sources
                        .iter()
                        .filter(|source| source.static_owner == Some(candidate_owner))
                    {
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
        let mut reads = BTreeSet::new();
        self.collect_view_reads(id, &mut reads, &mut BTreeSet::new())?;
        for read in reads {
            let Some((path, scope_id, target)) = self.view_read_binding(&read)? else {
                continue;
            };
            self.bindings.push(ViewBinding {
                id: ViewBindingId(self.bindings.len()),
                node_kind: node_kind.to_owned(),
                attr: attr.to_owned(),
                path,
                target,
                kind,
                scope_id,
                source_id: None,
            });
        }
        Ok(())
    }

    fn collect_view_reads(
        &self,
        id: ExecutableExprId,
        reads: &mut BTreeSet<ExecutableViewRead>,
        visited: &mut BTreeSet<ExecutableExprId>,
    ) -> Result<(), String> {
        if !visited.insert(id) {
            return Ok(());
        }
        if let Some(read) = self.direct_view_read_unscoped(id)? {
            reads.insert(read);
            return Ok(());
        }
        let expression = self.expression(id)?;
        if matches!(
            expression.kind,
            ExecutableExpressionKind::Materialize { .. }
        ) {
            return Ok(());
        }
        for child in executable_expression_children(&expression.kind) {
            self.collect_view_reads(child, reads, visited)?;
        }
        Ok(())
    }

    fn direct_view_read(&self, id: ExecutableExprId) -> Result<Option<ExecutableViewRead>, String> {
        self.direct_view_read_unscoped(id)
    }

    fn direct_view_read_unscoped(
        &self,
        id: ExecutableExprId,
    ) -> Result<Option<ExecutableViewRead>, String> {
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
            return Ok(Some(ExecutableViewRead::Canonical {
                declaration: Some(source.declaration),
                owner: source.owner,
                path: source.binding_path.clone(),
                projection: Vec::new(),
                storage_binding: self.storage_binding_for_source(source.id)?,
                expression: id,
            }));
        }
        match &expression.kind {
            ExecutableExpressionKind::CanonicalRead {
                target,
                storage_binding,
                path,
                projection,
            } => Ok(Some(ExecutableViewRead::Canonical {
                declaration: Some(*target),
                owner: expression.owner,
                path: path.clone(),
                projection: projection.clone(),
                storage_binding: *storage_binding,
                expression: id,
            })),
            ExecutableExpressionKind::ExternalRead { canonical_path } => {
                Ok(Some(ExecutableViewRead::Canonical {
                    declaration: None,
                    owner: expression.owner,
                    path: canonical_path.clone(),
                    projection: Vec::new(),
                    storage_binding: None,
                    expression: id,
                }))
            }
            ExecutableExpressionKind::Drain {
                target,
                storage_binding,
                path,
                projection,
            } => Ok(Some(ExecutableViewRead::Canonical {
                declaration: Some(*target),
                owner: expression.owner,
                path: path.clone(),
                projection: projection.clone(),
                storage_binding: *storage_binding,
                expression: id,
            })),
            ExecutableExpressionKind::MaterializationLocal {
                owner,
                local,
                projection,
            } => Ok(Some(ExecutableViewRead::Local {
                owner: *owner,
                local: *local,
                projection: projection.clone(),
            })),
            ExecutableExpressionKind::Project { input, fields } => {
                let Some(mut read) = self.direct_view_read_unscoped(*input)? else {
                    return Ok(None);
                };
                match &mut read {
                    ExecutableViewRead::Canonical { projection, .. } => {
                        if !fields.is_empty() {
                            projection.extend(fields.iter().cloned());
                        }
                    }
                    ExecutableViewRead::Local { projection, .. } => {
                        projection.extend(fields.iter().cloned());
                    }
                }
                Ok(Some(read))
            }
            _ => Ok(None),
        }
    }

    fn storage_binding_for_source(
        &self,
        source: ExecutableSourceId,
    ) -> Result<Option<StorageBindingId>, String> {
        let Some(storage) = self.storage else {
            return Ok(None);
        };
        let matches = storage
            .bindings
            .iter()
            .filter(|binding| {
                matches!(
                    binding.kind,
                    StorageBindingKind::Source { executable, .. } if executable == source
                )
            })
            .collect::<Vec<_>>();
        let [binding] = matches.as_slice() else {
            return Err(format!(
                "executable source {source} has {} exact storage bindings",
                matches.len()
            ));
        };
        Ok(Some(binding.id))
    }

    fn storage_binding_for_runtime_source(
        &self,
        source: SourceId,
    ) -> Result<StorageBindingId, String> {
        let runtime = self
            .sources
            .get(source.as_usize())
            .filter(|candidate| candidate.id == source)
            .ok_or_else(|| format!("view binding references missing runtime source {source}"))?;
        let executable = runtime.executable_source_id.ok_or_else(|| {
            format!("runtime source {source} has no exact executable source identity")
        })?;
        self.storage_binding_for_source(executable)?.ok_or_else(|| {
            "view source binding resolution requires the exact storage catalog".to_owned()
        })
    }

    fn view_read_binding(
        &mut self,
        read: &ExecutableViewRead,
    ) -> Result<Option<(String, Option<ScopeId>, ViewBindingTarget)>, String> {
        match read {
            ExecutableViewRead::Canonical {
                declaration,
                path,
                projection,
                storage_binding,
                expression,
                ..
            } => {
                let diagnostic_path = canonical_read_path(path, projection);
                let scope = declaration
                    .and_then(|declaration| {
                        self.list_scopes_by_declaration.get(&declaration).copied()
                    })
                    .or_else(|| scope_id_for_path(self.row_scopes, &diagnostic_path));
                let target = match storage_binding {
                    Some(binding) => ViewBindingTarget::Storage {
                        binding: *binding,
                        projection: projection.clone(),
                    },
                    None if declaration.is_none() => ViewBindingTarget::ExternalExpression {
                        expression: *expression,
                    },
                    None => {
                        return Err(format!(
                            "canonical view read `{diagnostic_path}` has no exact storage binding"
                        ));
                    }
                };
                Ok(Some((diagnostic_path, scope, target)))
            }
            ExecutableViewRead::Local {
                owner,
                local,
                projection,
            } => {
                let Some(scope_id) = self.local_scope(*owner, *local)? else {
                    return Ok(None);
                };
                if projection.is_empty() {
                    return Ok(None);
                }
                let row_scope = self
                    .row_scopes
                    .get(scope_id.as_usize())
                    .ok_or_else(|| format!("view local references missing ScopeId {scope_id}"))?;
                Ok(Some((
                    format!("{}.{}", row_scope.list, projection.join(".")),
                    Some(scope_id),
                    ViewBindingTarget::MaterializationLocal {
                        owner: *owner,
                        local: *local,
                        projection: projection.clone(),
                    },
                )))
            }
        }
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
            ExecutableExpressionKind::CanonicalRead { target, .. } => {
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
                if projection.first().map(String::as_str) == Some("items")
                    && let Some(scope) = scope
                {
                    return self.chunk_items_storage_scope_for_scope(scope);
                }
                Ok(scope)
            }
            ExecutableExpressionKind::Project { input, fields } => {
                if fields.first().map(String::as_str) == Some("items")
                    && let Some(scope) = self.chunk_items_storage_scope(input)?
                {
                    return Ok(Some(scope));
                }
                self.storage_scope_for_expression(input, visited_expressions, visited_paths)
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

fn verify_scheduled_update_expression(
    value: &UpdateExpression,
    target: &str,
    source: &str,
    known_symbols: &BTreeSet<&str>,
) -> Result<(), String> {
    match value {
        UpdateExpression::SourcePayload { .. } | UpdateExpression::Const { .. } => Ok(()),
        UpdateExpression::NumberInfix { left, op, right } => {
            require_supported_numeric_update_op(op, "number infix")?;
            if !is_number_literal(left) {
                require_known_symbol("number infix left", left, known_symbols)?;
            }
            if !is_number_literal(right) {
                require_known_symbol("number infix right", right, known_symbols)?;
            }
            Ok(())
        }
        UpdateExpression::ProjectTime {
            pointer_x,
            pointer_width,
            viewport_start,
            viewport_end,
            fallback,
        } => {
            for (context, path) in [
                ("project time pointer_x", pointer_x),
                ("project time pointer_width", pointer_width),
                ("project time viewport_start", viewport_start),
                ("project time viewport_end", viewport_end),
                ("project time fallback", fallback),
            ] {
                if !is_number_literal(path) && !source_payload_input_matches(path, source) {
                    require_known_symbol(context, path, known_symbols)?;
                }
            }
            Ok(())
        }
        UpdateExpression::MatchInfixConst {
            left,
            op,
            right,
            arms,
        } => {
            require_supported_numeric_update_op(op, "match number infix")?;
            verify_update_value_expression(left, known_symbols, "match infix left")?;
            verify_update_value_expression(right, known_symbols, "match infix right")?;
            for arm in arms {
                verify_update_value_expression(
                    &arm.output,
                    known_symbols,
                    "match number infix arm",
                )?;
            }
            Ok(())
        }
        UpdateExpression::PreviousValue { path } | UpdateExpression::ReadPath { path } => {
            if source_payload_input_matches(path, source) {
                Ok(())
            } else {
                require_known_symbol("update expression path", path, known_symbols)
            }
        }
        UpdateExpression::BoolNot { path } => {
            require_known_symbol("update expression path", path, known_symbols)
        }
        UpdateExpression::TextToNumber { path } => {
            if source_payload_input_matches(path, source) {
                Ok(())
            } else {
                require_known_symbol("text-to-number input", path, known_symbols)
            }
        }
        UpdateExpression::BytesLength { path }
        | UpdateExpression::BytesIsEmpty { path }
        | UpdateExpression::BytesGet { path, .. }
        | UpdateExpression::BytesSet { path, .. }
        | UpdateExpression::BytesToHex { path }
        | UpdateExpression::BytesFromHex { path }
        | UpdateExpression::BytesToBase64 { path }
        | UpdateExpression::BytesFromBase64 { path }
        | UpdateExpression::BytesReadUnsigned { path, .. }
        | UpdateExpression::BytesReadSigned { path, .. }
        | UpdateExpression::BytesWriteUnsigned { path, .. }
        | UpdateExpression::BytesWriteSigned { path, .. }
        | UpdateExpression::TextToBytes { path, .. }
        | UpdateExpression::BytesToText { path, .. } => {
            require_known_symbol("bytes update path", path, known_symbols)
        }
        UpdateExpression::ListGet { path, .. } => {
            if source_payload_input_matches(path, source) {
                Ok(())
            } else {
                require_known_symbol("list get path", path, known_symbols)
            }
        }
        UpdateExpression::BytesSlice {
            path,
            offset,
            byte_count,
        } => {
            require_known_symbol("bytes update path", path, known_symbols)?;
            require_known_bytes_scalar_arg("bytes slice offset", offset, known_symbols)?;
            require_known_bytes_scalar_arg("bytes slice byte_count", byte_count, known_symbols)
        }
        UpdateExpression::BytesTake { path, byte_count }
        | UpdateExpression::BytesDrop { path, byte_count } => {
            require_known_symbol("bytes update path", path, known_symbols)?;
            require_known_bytes_scalar_arg("bytes count", byte_count, known_symbols)
        }
        UpdateExpression::BytesZeros { .. } => Ok(()),
        UpdateExpression::BytesConcat { left, right } => {
            require_known_symbol("bytes concat left path", left, known_symbols)?;
            require_known_symbol("bytes concat right path", right, known_symbols)
        }
        UpdateExpression::BytesEqual { left, right } => {
            require_known_symbol("bytes equality left path", left, known_symbols)?;
            require_known_symbol("bytes equality right path", right, known_symbols)
        }
        UpdateExpression::BytesFind { haystack, needle } => {
            require_known_symbol("bytes find haystack path", haystack, known_symbols)?;
            require_known_symbol("bytes find needle path", needle, known_symbols)
        }
        UpdateExpression::BytesStartsWith { path, prefix } => {
            require_known_symbol("bytes starts_with path", path, known_symbols)?;
            require_known_symbol("bytes starts_with prefix path", prefix, known_symbols)
        }
        UpdateExpression::BytesEndsWith { path, suffix } => {
            require_known_symbol("bytes ends_with path", path, known_symbols)?;
            require_known_symbol("bytes ends_with suffix path", suffix, known_symbols)
        }
        UpdateExpression::TextTrimOrPrevious { path, previous } => {
            if path != "text" && path != "key" {
                require_known_symbol("trim source", path, known_symbols)?;
            }
            require_known_symbol("trim previous", previous, known_symbols)
        }
        UpdateExpression::PrefixPayloadConcat {
            prefix: _,
            payload_path,
            separator: _,
        } => {
            if source_payload_input_matches(payload_path, source) {
                Ok(())
            } else {
                require_known_symbol("concat payload", payload_path, known_symbols)
            }
        }
        UpdateExpression::PrefixRootConcat {
            prefix: _,
            path,
            separator: _,
        } => require_known_symbol("concat path", path, known_symbols),
        UpdateExpression::MatchConst { input, .. } => {
            if source_payload_input_matches(input, source) {
                Ok(())
            } else {
                require_known_symbol("match input", input, known_symbols)
            }
        }
        UpdateExpression::MatchValueConst { input, arms }
        | UpdateExpression::MatchTextIsEmptyConst { input, arms } => {
            if !source_payload_input_matches(input, source) {
                require_known_symbol("match value input", input, known_symbols)?;
            }
            for arm in arms {
                verify_update_value_expression(&arm.output, known_symbols, "match value arm")?;
            }
            Ok(())
        }
        UpdateExpression::HostEffect {
            operation,
            arguments,
            ..
        } => {
            if !boon_typecheck::is_typed_host_effect(operation) {
                return Err(format!(
                    "static schedule contains unknown typed host effect `{operation}`"
                ));
            }
            if arguments.iter().any(|argument| argument.name.is_empty()) {
                return Err(format!(
                    "typed host effect `{operation}` contains an unnamed argument"
                ));
            }
            Ok(())
        }
        UpdateExpression::Unknown { summary } => Err(format!(
            "static schedule contains unsupported update expression for `{target}` from `{source}`: `{summary}`"
        )),
    }
}

fn require_known_bytes_scalar_arg(
    context: &str,
    arg: &BytesScalarArg,
    known_symbols: &BTreeSet<&str>,
) -> Result<(), String> {
    match arg {
        BytesScalarArg::Static(_) => Ok(()),
        BytesScalarArg::Path(path) => require_known_symbol(context, path, known_symbols),
    }
}

fn verify_update_value_expression(
    value: &UpdateValueExpression,
    known_symbols: &BTreeSet<&str>,
    context: &str,
) -> Result<(), String> {
    match value {
        UpdateValueExpression::Const { .. } => Ok(()),
        UpdateValueExpression::ReadPath { path } => {
            require_known_symbol(&format!("{context} path"), path, known_symbols)
        }
        UpdateValueExpression::MatchConst { input, arms } => {
            require_known_symbol(&format!("{context} match input"), input, known_symbols)?;
            for arm in arms {
                verify_update_value_expression(
                    &arm.output,
                    known_symbols,
                    "nested match const arm",
                )?;
            }
            Ok(())
        }
        UpdateValueExpression::MatchTextIsEmptyConst { input, arms } => {
            require_known_symbol(
                &format!("{context} text-is-empty input"),
                input,
                known_symbols,
            )?;
            for arm in arms {
                verify_update_value_expression(
                    &arm.output,
                    known_symbols,
                    "nested text-is-empty arm",
                )?;
            }
            Ok(())
        }
        UpdateValueExpression::NumberInfix { left, op, right } => {
            require_supported_numeric_update_op(op, &format!("{context} number infix"))?;
            if !is_number_literal(left) {
                require_known_symbol(&format!("{context} number infix left"), left, known_symbols)?;
            }
            if !is_number_literal(right) {
                require_known_symbol(
                    &format!("{context} number infix right"),
                    right,
                    known_symbols,
                )?;
            }
            Ok(())
        }
        UpdateValueExpression::MatchInfixConst {
            left,
            op,
            right,
            arms,
        } => {
            require_supported_numeric_update_op(op, &format!("{context} match number infix"))?;
            if !is_number_literal(left) {
                require_known_symbol(
                    &format!("{context} match number infix left"),
                    left,
                    known_symbols,
                )?;
            }
            if !is_number_literal(right) {
                require_known_symbol(
                    &format!("{context} match number infix right"),
                    right,
                    known_symbols,
                )?;
            }
            for arm in arms {
                verify_update_value_expression(
                    &arm.output,
                    known_symbols,
                    "nested match number infix arm",
                )?;
            }
            Ok(())
        }
    }
}

fn require_supported_numeric_update_op(op: &str, context: &str) -> Result<(), String> {
    matches!(op, "+" | "-" | ">" | ">=" | "<" | "<=" | "==" | "!=")
        .then_some(())
        .ok_or_else(|| format!("{context} uses unsupported numeric operator `{op}`"))
}

fn source_payload_input_matches(input: &str, source: &str) -> bool {
    source_payload_field_from_path(input, &source_ref_variants(source)).is_some()
}

fn verify_scheduled_list_operation(
    value: &ListOperationKind,
    source_paths: &BTreeSet<&str>,
    known_symbols: &BTreeSet<&str>,
) -> Result<(), String> {
    match value {
        ListOperationKind::Append { trigger, fields } => {
            require_known_symbol("append trigger", trigger, known_symbols)?;
            for field in fields {
                if let ListAppendFieldValue::Source { path } = &field.value {
                    require_known_symbol("append field source", path, known_symbols)?;
                }
            }
            Ok(())
        }
        ListOperationKind::Remove { source, predicate } => {
            if !source_paths.contains(source.as_str()) {
                return Err(format!(
                    "remove source `{source}` is not a declared source port"
                ));
            }
            verify_scheduled_list_predicate(predicate, known_symbols)
        }
        ListOperationKind::Retain { target, predicate }
        | ListOperationKind::Count { target, predicate } => {
            require_known_symbol("list operation target", target, known_symbols)?;
            verify_scheduled_list_predicate(predicate, known_symbols)
        }
    }
}

fn verify_scheduled_list_predicate(
    value: &ListPredicate,
    known_symbols: &BTreeSet<&str>,
) -> Result<(), String> {
    match value {
        ListPredicate::AlwaysTrue => Ok(()),
        ListPredicate::RowFieldBool { path } | ListPredicate::RowFieldBoolNot { path } => {
            if is_row_local_field_path(path) {
                return Ok(());
            }
            require_known_symbol("list predicate field", path, known_symbols)
        }
        ListPredicate::SelectedFilterVisibility {
            selector,
            row_field,
        } => {
            require_known_symbol("list predicate selector", selector, known_symbols)?;
            if is_row_local_field_path(row_field) {
                return Ok(());
            }
            require_known_symbol("list predicate row field", row_field, known_symbols)
        }
        ListPredicate::Unknown { summary } => Err(format!(
            "static schedule contains unsupported list predicate `{summary}`"
        )),
    }
}

fn is_row_local_field_path(path: &str) -> bool {
    let Some((row, field)) = path.split_once('.') else {
        return false;
    };
    !field.is_empty() && value_starts_lowercase_identifier(row)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FieldCycleVisit {
    Pending,
    Visiting,
    Complete,
}

fn field_symbol_dependency_graph(
    fields: &[FieldDef],
    excluded_paths: &BTreeSet<&str>,
) -> (Vec<bool>, Vec<Vec<usize>>) {
    let excluded_field = fields
        .iter()
        .map(|field| excluded_paths.contains(field.path.as_str()))
        .collect::<Vec<_>>();
    let fields_by_path = fields
        .iter()
        .enumerate()
        .map(|(index, field)| (field.path.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let mut fields_by_local_name = BTreeMap::<&str, Vec<usize>>::new();
    for (index, field) in fields.iter().enumerate() {
        fields_by_local_name
            .entry(field.local_name.as_str())
            .or_default()
            .push(index);
    }
    let mut dependency_edges = vec![Vec::<usize>::new(); fields.len()];
    for (field_index, field) in fields.iter().enumerate() {
        if excluded_field[field_index] {
            continue;
        }
        let mut dependencies = BTreeSet::new();
        for expr in &field.ast_exprs {
            let raw = match &expr.kind {
                AstExprKind::Identifier(value) => value.clone(),
                AstExprKind::Path(parts) => parts.join("."),
                _ => continue,
            };
            if let Some(dependency_index) =
                scoped_field_reference_candidates(&field.parent_path, &raw)
                    .into_iter()
                    .find_map(|candidate| longest_field_path_prefix(&fields_by_path, &candidate))
                && dependency_index != field_index
                && !excluded_field[dependency_index]
            {
                dependencies.insert(dependency_index);
            }
            if !raw.contains('.')
                && let Some(candidates) = fields_by_local_name.get(raw.as_str())
            {
                for &dependency_index in candidates {
                    if dependency_index != field_index
                        && !excluded_field[dependency_index]
                        && expression_references_field(field, expr, &fields[dependency_index])
                    {
                        dependencies.insert(dependency_index);
                    }
                }
            }
        }
        dependency_edges[field_index].extend(dependencies);
    }
    (excluded_field, dependency_edges)
}

fn immediate_field_dependencies(
    fields: &[FieldDef],
    state_cells: &[StateCell],
    typecheck_report: &boon_typecheck::TypeCheckReport,
) -> Vec<ImmediateDependency> {
    let state_paths = state_cells
        .iter()
        .map(|state| state.path.as_str())
        .collect::<BTreeSet<_>>();
    let flow_modes = typecheck_report
        .named_value_type_table
        .entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry.flow_type.mode))
        .collect::<BTreeMap<_, _>>();
    let (_, dependencies) = field_symbol_dependency_graph(fields, &BTreeSet::new());
    let mut result = Vec::new();
    for (field_index, dependency_indexes) in dependencies.into_iter().enumerate() {
        let field = &fields[field_index];
        if state_paths.contains(field.path.as_str())
            || flow_modes.get(field.path.as_str()) != Some(&boon_typecheck::FlowMode::Continuous)
        {
            continue;
        }
        result.extend(
            dependency_indexes
                .into_iter()
                .map(|dependency| ImmediateDependency {
                    dependent: field.path.clone(),
                    dependency: fields[dependency].path.clone(),
                }),
        );
    }
    result
}

fn scoped_field_reference_candidates(parent_path: &str, path: &str) -> Vec<String> {
    let mut candidates = vec![path.to_owned()];
    let mut scope = Some(parent_path);
    while let Some(parent) = scope.filter(|parent| !parent.is_empty()) {
        candidates.push(format!("{parent}.{path}"));
        scope = parent.rsplit_once('.').map(|(ancestor, _)| ancestor);
    }
    candidates
}

fn longest_field_path_prefix(fields_by_path: &BTreeMap<&str, usize>, path: &str) -> Option<usize> {
    let mut candidate = path;
    loop {
        if let Some(index) = fields_by_path.get(candidate) {
            return Some(*index);
        }
        let (parent, _) = candidate.rsplit_once('.')?;
        candidate = parent;
    }
}

fn verify_combinational_field_cycles(
    program: &ParsedProgram,
    fields: &[FieldDef],
    state_cells: &[StateCell],
) -> Result<(), String> {
    let memory_paths = state_cells
        .iter()
        .map(|cell| cell.path.as_str())
        .chain(
            fields
                .iter()
                .filter(|field| {
                    (field_is_list_memory_path(field, program) || field.has_operator("List/append"))
                        && !is_output_registry_value_path(&field.path)
                })
                .map(|field| field.path.as_str()),
        )
        .collect::<BTreeSet<_>>();
    let (memory_field, dependency_edges) = field_symbol_dependency_graph(fields, &memory_paths);

    let mut visits = vec![FieldCycleVisit::Pending; fields.len()];
    for (field_index, is_memory_field) in memory_field.iter().enumerate() {
        if *is_memory_field {
            continue;
        }
        let mut visiting = Vec::new();
        verify_combinational_field_cycles_from(
            field_index,
            fields,
            &dependency_edges,
            &mut visits,
            &mut visiting,
        )?;
    }
    Ok(())
}

fn verify_combinational_field_cycles_from(
    field_index: usize,
    fields: &[FieldDef],
    dependency_edges: &[Vec<usize>],
    visits: &mut [FieldCycleVisit],
    visiting: &mut Vec<usize>,
) -> Result<(), String> {
    match visits[field_index] {
        FieldCycleVisit::Complete => return Ok(()),
        FieldCycleVisit::Visiting => {
            let position = visiting
                .iter()
                .position(|candidate| *candidate == field_index)
                .unwrap_or(0);
            let mut cycle = visiting[position..]
                .iter()
                .map(|index| fields[*index].path.as_str())
                .collect::<Vec<_>>();
            cycle.push(fields[field_index].path.as_str());
            return Err(format!(
                "combinational dependency cycle through pure/WHILE expressions must be broken by HOLD or another authoritative memory boundary: {}",
                cycle.join(" -> ")
            ));
        }
        FieldCycleVisit::Pending => {}
    }
    visits[field_index] = FieldCycleVisit::Visiting;
    visiting.push(field_index);
    for &dependency_index in &dependency_edges[field_index] {
        verify_combinational_field_cycles_from(
            dependency_index,
            fields,
            dependency_edges,
            visits,
            visiting,
        )?;
    }
    visiting.pop();
    visits[field_index] = FieldCycleVisit::Complete;
    Ok(())
}

fn verify_identity_clean_identifiers(program: &ErasedProgram) -> Result<(), String> {
    for node in &program.nodes {
        reject_hidden_identity_identifier("node", &node.name)?;
    }
    for source in &program.sources {
        reject_hidden_identity_identifier("source port", &source.path)?;
    }
    for cell in &program.state_cells {
        reject_hidden_identity_identifier("state cell", &cell.path)?;
        reject_hidden_identity_identifier("hold name", &cell.hold_name)?;
        reject_initial_value_identity(&cell.initial_value)?;
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
    for branch in &program.update_branches {
        reject_hidden_identity_identifier("update target", &branch.target)?;
        reject_hidden_identity_identifier("update source", &branch.source)?;
        reject_update_expression_identity(&branch.expression)?;
    }
    for operation in &program.list_operations {
        reject_hidden_identity_identifier("list operation", &operation.list)?;
        reject_list_operation_identity(&operation.kind)?;
    }
    for projection in &program.list_projections {
        reject_hidden_identity_identifier("list projection target", &projection.target)?;
        reject_hidden_identity_identifier("list projection list", &projection.list)?;
        match &projection.kind {
            ListProjectionKind::Chunk {
                item_field,
                label_field,
                ..
            } => {
                reject_hidden_identity_identifier("list chunk item field", item_field)?;
                reject_hidden_identity_identifier("list chunk label field", label_field)?;
            }
            ListProjectionKind::TextPrefix { field, prefix, .. } => {
                reject_hidden_identity_identifier("list query field", field)?;
                reject_hidden_identity_identifier("list query prefix", prefix)?;
            }
            ListProjectionKind::IndexedQuery {
                fields,
                selection,
                residual,
                cursor,
                ..
            } => {
                for field in fields {
                    reject_hidden_identity_identifier(
                        "indexed query field",
                        &field.path.join("."),
                    )?;
                }
                for value in list_query_selection_paths(selection) {
                    reject_hidden_identity_identifier("indexed query selection", value)?;
                }
                if let Some(residual) = residual {
                    for value in list_query_residual_paths(residual) {
                        reject_hidden_identity_identifier("indexed query residual", value)?;
                    }
                }
                if let Some(cursor) = cursor {
                    reject_hidden_identity_identifier("indexed query cursor", cursor)?;
                }
            }
        }
    }
    Ok(())
}

fn list_query_selection_paths(selection: &ListQuerySelection) -> Vec<&str> {
    match selection {
        ListQuerySelection::Exact { key } => vec![key],
        ListQuerySelection::TextPrefix { leading, prefix } => leading
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(prefix.as_str()))
            .collect(),
        ListQuerySelection::Range { lower, upper, .. } => lower
            .iter()
            .chain(upper.iter())
            .map(String::as_str)
            .collect(),
        ListQuerySelection::Union { keys } | ListQuerySelection::Intersection { keys } => {
            vec![keys]
        }
        ListQuerySelection::Unknown { .. } => Vec::new(),
    }
}

fn list_query_residual_paths(residual: &ListQueryResidual) -> Vec<&str> {
    match residual {
        ListQueryResidual::FieldEqual { value, .. } => vec![value],
        ListQueryResidual::TextContains { needle, .. } => vec![needle],
        ListQueryResidual::NumberRange {
            minimum, maximum, ..
        } => minimum
            .iter()
            .chain(maximum.iter())
            .map(String::as_str)
            .collect(),
        ListQueryResidual::Wgs84Radius {
            center_latitude,
            center_longitude,
            radius_meters,
            ..
        } => vec![center_latitude, center_longitude, radius_meters],
        ListQueryResidual::Unknown { .. } => Vec::new(),
    }
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

fn reject_update_expression_identity(value: &UpdateExpression) -> Result<(), String> {
    match value {
        UpdateExpression::SourcePayload { path } => {
            reject_hidden_identity_identifier("source payload", path)
        }
        UpdateExpression::PreviousValue { path }
        | UpdateExpression::ReadPath { path }
        | UpdateExpression::BoolNot { path }
        | UpdateExpression::TextToNumber { path }
        | UpdateExpression::BytesLength { path }
        | UpdateExpression::BytesIsEmpty { path }
        | UpdateExpression::BytesGet { path, .. }
        | UpdateExpression::BytesSet { path, .. }
        | UpdateExpression::BytesToHex { path }
        | UpdateExpression::BytesFromHex { path }
        | UpdateExpression::BytesToBase64 { path }
        | UpdateExpression::BytesFromBase64 { path }
        | UpdateExpression::BytesReadUnsigned { path, .. }
        | UpdateExpression::BytesReadSigned { path, .. }
        | UpdateExpression::BytesWriteUnsigned { path, .. }
        | UpdateExpression::BytesWriteSigned { path, .. }
        | UpdateExpression::TextToBytes { path, .. }
        | UpdateExpression::BytesToText { path, .. } => {
            reject_hidden_identity_identifier("update expression path", path)
        }
        UpdateExpression::ListGet { path, .. } => {
            reject_hidden_identity_identifier("list get path", path)
        }
        UpdateExpression::BytesSlice {
            path,
            offset,
            byte_count,
        } => {
            reject_hidden_identity_identifier("update expression path", path)?;
            reject_bytes_scalar_arg_identity("bytes slice offset", offset)?;
            reject_bytes_scalar_arg_identity("bytes slice byte_count", byte_count)
        }
        UpdateExpression::BytesTake { path, byte_count }
        | UpdateExpression::BytesDrop { path, byte_count } => {
            reject_hidden_identity_identifier("update expression path", path)?;
            reject_bytes_scalar_arg_identity("bytes count", byte_count)
        }
        UpdateExpression::BytesZeros { .. } => Ok(()),
        UpdateExpression::BytesConcat { left, right } => {
            reject_hidden_identity_identifier("bytes concat left path", left)?;
            reject_hidden_identity_identifier("bytes concat right path", right)
        }
        UpdateExpression::BytesEqual { left, right } => {
            reject_hidden_identity_identifier("bytes equality left path", left)?;
            reject_hidden_identity_identifier("bytes equality right path", right)
        }
        UpdateExpression::BytesFind { haystack, needle } => {
            reject_hidden_identity_identifier("bytes find haystack path", haystack)?;
            reject_hidden_identity_identifier("bytes find needle path", needle)
        }
        UpdateExpression::BytesStartsWith { path, prefix } => {
            reject_hidden_identity_identifier("bytes starts_with path", path)?;
            reject_hidden_identity_identifier("bytes starts_with prefix path", prefix)
        }
        UpdateExpression::BytesEndsWith { path, suffix } => {
            reject_hidden_identity_identifier("bytes ends_with path", path)?;
            reject_hidden_identity_identifier("bytes ends_with suffix path", suffix)
        }
        UpdateExpression::TextTrimOrPrevious { path, previous } => {
            reject_hidden_identity_identifier("trim source", path)?;
            reject_hidden_identity_identifier("trim previous", previous)
        }
        UpdateExpression::PrefixPayloadConcat {
            prefix,
            payload_path,
            separator,
        } => {
            reject_hidden_identity_identifier("concat prefix", prefix)?;
            reject_hidden_identity_identifier("concat payload", payload_path)?;
            reject_hidden_identity_identifier("concat separator", separator)
        }
        UpdateExpression::PrefixRootConcat {
            prefix,
            path,
            separator,
        } => {
            reject_hidden_identity_identifier("concat prefix", prefix)?;
            reject_hidden_identity_identifier("concat path", path)?;
            reject_hidden_identity_identifier("concat separator", separator)
        }
        UpdateExpression::NumberInfix { left, right, .. } => {
            reject_hidden_identity_identifier("number infix left", left)?;
            reject_hidden_identity_identifier("number infix right", right)
        }
        UpdateExpression::ProjectTime {
            pointer_x,
            pointer_width,
            viewport_start,
            viewport_end,
            fallback,
        } => {
            reject_hidden_identity_identifier("project time pointer_x", pointer_x)?;
            reject_hidden_identity_identifier("project time pointer_width", pointer_width)?;
            reject_hidden_identity_identifier("project time viewport_start", viewport_start)?;
            reject_hidden_identity_identifier("project time viewport_end", viewport_end)?;
            reject_hidden_identity_identifier("project time fallback", fallback)
        }
        UpdateExpression::MatchInfixConst {
            left, right, arms, ..
        } => {
            reject_update_value_expression_identity(left)?;
            reject_update_value_expression_identity(right)?;
            for arm in arms {
                reject_hidden_identity_identifier("match pattern", &arm.pattern)?;
                reject_update_value_expression_identity(&arm.output)?;
            }
            Ok(())
        }
        UpdateExpression::MatchConst { input, arms } => {
            reject_hidden_identity_identifier("match input", input)?;
            for arm in arms {
                reject_hidden_identity_identifier("match pattern", &arm.pattern)?;
                reject_hidden_identity_identifier("match output", &arm.output)?;
            }
            Ok(())
        }
        UpdateExpression::MatchValueConst { input, arms }
        | UpdateExpression::MatchTextIsEmptyConst { input, arms } => {
            reject_hidden_identity_identifier("match value input", input)?;
            for arm in arms {
                reject_hidden_identity_identifier("match pattern", &arm.pattern)?;
                reject_update_value_expression_identity(&arm.output)?;
            }
            Ok(())
        }
        UpdateExpression::HostEffect {
            operation,
            arguments,
            ..
        } => {
            reject_hidden_identity_identifier("host effect operation", operation)?;
            for argument in arguments {
                reject_hidden_identity_identifier("host effect argument", &argument.name)?;
            }
            Ok(())
        }
        UpdateExpression::Unknown { summary } => {
            reject_hidden_identity_identifier("unknown update expression", summary)
        }
        UpdateExpression::Const { value } => {
            reject_hidden_identity_identifier("const value", value)
        }
    }
}

fn reject_bytes_scalar_arg_identity(context: &str, arg: &BytesScalarArg) -> Result<(), String> {
    match arg {
        BytesScalarArg::Static(_) => Ok(()),
        BytesScalarArg::Path(path) => reject_hidden_identity_identifier(context, path),
    }
}

fn reject_update_value_expression_identity(value: &UpdateValueExpression) -> Result<(), String> {
    match value {
        UpdateValueExpression::Const { value } => {
            reject_hidden_identity_identifier("match output const", value)
        }
        UpdateValueExpression::ReadPath { path } => {
            reject_hidden_identity_identifier("match output path", path)
        }
        UpdateValueExpression::MatchConst { input, arms } => {
            reject_hidden_identity_identifier("match output match input", input)?;
            for arm in arms {
                reject_hidden_identity_identifier("match pattern", &arm.pattern)?;
                reject_update_value_expression_identity(&arm.output)?;
            }
            Ok(())
        }
        UpdateValueExpression::MatchTextIsEmptyConst { input, arms } => {
            reject_hidden_identity_identifier("match output text-is-empty input", input)?;
            for arm in arms {
                reject_hidden_identity_identifier("match pattern", &arm.pattern)?;
                reject_update_value_expression_identity(&arm.output)?;
            }
            Ok(())
        }
        UpdateValueExpression::NumberInfix { left, right, .. } => {
            reject_hidden_identity_identifier("match output number infix left", left)?;
            reject_hidden_identity_identifier("match output number infix right", right)
        }
        UpdateValueExpression::MatchInfixConst {
            left, right, arms, ..
        } => {
            reject_hidden_identity_identifier("match output match number infix left", left)?;
            reject_hidden_identity_identifier("match output match number infix right", right)?;
            for arm in arms {
                reject_hidden_identity_identifier("match pattern", &arm.pattern)?;
                reject_update_value_expression_identity(&arm.output)?;
            }
            Ok(())
        }
    }
}

fn reject_list_operation_identity(value: &ListOperationKind) -> Result<(), String> {
    match value {
        ListOperationKind::Append { trigger, fields } => {
            reject_hidden_identity_identifier("append trigger", trigger)?;
            for field in fields {
                reject_hidden_identity_identifier("append field", &field.name)?;
                match &field.value {
                    ListAppendFieldValue::Source { path } => {
                        reject_hidden_identity_identifier("append field source", path)?;
                    }
                    ListAppendFieldValue::Const { value } => {
                        reject_hidden_identity_identifier("append field const", value)?;
                    }
                    ListAppendFieldValue::TypedConst { value } => {
                        reject_initial_value_identity(value)?;
                    }
                }
            }
            Ok(())
        }
        ListOperationKind::Remove { source, predicate } => {
            reject_hidden_identity_identifier("remove source", source)?;
            reject_list_predicate_identity(predicate)
        }
        ListOperationKind::Retain { target, predicate }
        | ListOperationKind::Count { target, predicate } => {
            reject_hidden_identity_identifier("list operation target", target)?;
            reject_list_predicate_identity(predicate)
        }
    }
}

fn reject_list_predicate_identity(value: &ListPredicate) -> Result<(), String> {
    match value {
        ListPredicate::RowFieldBool { path } | ListPredicate::RowFieldBoolNot { path } => {
            reject_hidden_identity_identifier("list predicate field", path)
        }
        ListPredicate::SelectedFilterVisibility {
            selector,
            row_field,
        } => {
            reject_hidden_identity_identifier("list predicate selector", selector)?;
            reject_hidden_identity_identifier("list predicate row field", row_field)
        }
        ListPredicate::Unknown { summary } => {
            reject_hidden_identity_identifier("unknown list predicate", summary)
        }
        ListPredicate::AlwaysTrue => Ok(()),
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
    program: &ParsedProgram,
    nodes: &[IrNode],
    state_cells: &[StateCell],
    lists: &[ListMemory],
    derived_values: &[DerivedValue],
    update_branches: &[UpdateBranch],
    list_operations: &[ListOperation],
    distributed_references: &DistributedReferences,
) -> ExpressionCoverage {
    let mut coverage = ExpressionCoverage {
        ast_expression_count: program.expressions.len(),
        distributed_reference_expression_count: distributed_references.value_references.len()
            + distributed_references.pure_calls.len(),
        ..ExpressionCoverage::empty()
    };
    let scheduled_expr_ids = nodes
        .iter()
        .filter_map(|node| node.expr_id)
        .map(ExprId::as_usize)
        .collect::<BTreeSet<_>>();
    for expr in &program.expressions {
        if let AstExprKind::Unknown(tokens) = &expr.kind {
            if scheduled_expr_ids.contains(&expr.id) {
                coverage.unknown_ast_expression_count += 1;
                coverage.unknown_labels.push(format!(
                    "scheduled ast expression line {}: {}",
                    expr.line,
                    if tokens.is_empty() {
                        "<empty>".to_owned()
                    } else {
                        tokens.join(" ")
                    }
                ));
            } else {
                coverage.ignored_unknown_ast_expression_count += 1;
                coverage.ignored_unknown_labels.push(format!(
                    "ignored ast expression line {}: {}",
                    expr.line,
                    if tokens.is_empty() {
                        "<empty>".to_owned()
                    } else {
                        tokens.join(" ")
                    }
                ));
            }
        }
    }
    for cell in state_cells {
        if let InitialValue::Unknown { summary } = &cell.initial_value {
            coverage.unknown_initial_value_count += 1;
            coverage
                .unknown_labels
                .push(format!("initial value {}: {summary}", cell.path));
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
    for branch in update_branches {
        if let UpdateExpression::Unknown { summary } = &branch.expression {
            coverage.unknown_update_expression_count += 1;
            coverage.unknown_labels.push(format!(
                "update branch {} from {}: {summary}",
                branch.target, branch.source
            ));
        }
    }
    for operation in list_operations {
        for summary in unknown_predicate_summaries(&operation.kind) {
            coverage.unknown_list_predicate_count += 1;
            coverage
                .unknown_labels
                .push(format!("list operation {}: {summary}", operation.list));
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

fn unknown_predicate_summaries(kind: &ListOperationKind) -> Vec<&str> {
    match kind {
        ListOperationKind::Remove { predicate, .. }
        | ListOperationKind::Retain { predicate, .. }
        | ListOperationKind::Count { predicate, .. } => match predicate {
            ListPredicate::Unknown { summary } => vec![summary.as_str()],
            ListPredicate::AlwaysTrue
            | ListPredicate::RowFieldBool { .. }
            | ListPredicate::RowFieldBoolNot { .. }
            | ListPredicate::SelectedFilterVisibility { .. } => Vec::new(),
        },
        ListOperationKind::Append { .. } => Vec::new(),
    }
}

fn source_driven_nodes(program: &ParsedProgram) -> Vec<IrNode> {
    let mut nodes = program
        .expressions
        .iter()
        .filter_map(expression_node)
        .enumerate()
        .map(|(id, mut node)| {
            node.id = NodeId(id);
            node
        })
        .collect::<Vec<_>>();
    for list in &program.list_memories {
        push_generated(
            &mut nodes,
            &format!("render_{}_template", sanitize_node_name(&list.name)),
            IrNodeKind::RenderLowering,
            true,
        );
    }
    nodes
}

fn expression_node(expr: &AstExpr) -> Option<IrNode> {
    let kind = expression_ir_node_kind(expr)?;
    Some(IrNode {
        id: NodeId(0),
        name: format!(
            "expr_{}_{}",
            expr.id,
            sanitize_node_name(&ast_expr_label(expr))
        ),
        indexed: expression_is_indexed(expr, &kind),
        kind,
        expr_id: Some(ExprId(expr.id)),
    })
}

fn expression_ir_node_kind(expr: &AstExpr) -> Option<IrNodeKind> {
    match &expr.kind {
        AstExprKind::Source => Some(IrNodeKind::SourceRead),
        AstExprKind::Hold { .. } => Some(IrNodeKind::Hold),
        AstExprKind::ListLiteral { .. } => Some(IrNodeKind::ListMap),
        AstExprKind::Latest => Some(IrNodeKind::Latest),
        AstExprKind::When { .. } => Some(IrNodeKind::When),
        AstExprKind::Then { .. } => Some(IrNodeKind::Then),
        AstExprKind::Pipe { op, .. } => expression_operator_node_kind(std::slice::from_ref(op)),
        AstExprKind::Call { function, .. } => {
            expression_operator_node_kind(std::slice::from_ref(function))
                .or(Some(IrNodeKind::PureCall))
        }
        AstExprKind::Drain { .. } | AstExprKind::Draining { .. } => None,
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::BytesLiteral { .. }
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::TaggedObject { .. }
        | AstExprKind::Infix { .. }
        | AstExprKind::Block { .. }
        | AstExprKind::Record(_)
        | AstExprKind::Object(_) => Some(IrNodeKind::PureCall),
        AstExprKind::MatchArm { .. } | AstExprKind::Delimiter | AstExprKind::Unknown(_) => None,
    }
}

fn expression_operator_node_kind(operators: &[String]) -> Option<IrNodeKind> {
    if operators.iter().any(|operator| operator == "List/append") {
        Some(IrNodeKind::ListAppend)
    } else if operators.iter().any(|operator| operator == "List/remove") {
        Some(IrNodeKind::ListRemove)
    } else if operators.iter().any(|operator| operator == "List/map") {
        Some(IrNodeKind::ListMap)
    } else if operators.iter().any(|operator| operator == "List/retain") {
        Some(IrNodeKind::ListRetain)
    } else if operators
        .iter()
        .any(|operator| operator == "List/count" || operator == "List/every")
    {
        Some(IrNodeKind::Aggregate)
    } else if operators.iter().any(|operator| operator == "LATEST") {
        Some(IrNodeKind::Latest)
    } else if operators.iter().any(|operator| operator == "WHILE") {
        Some(IrNodeKind::While)
    } else if operators.iter().any(|operator| operator == "THEN") {
        Some(IrNodeKind::Then)
    } else if operators.iter().any(|operator| operator == "WHEN") {
        Some(IrNodeKind::When)
    } else if operators
        .iter()
        .any(|operator| operator.starts_with("Text/") || operator.starts_with("Bool/"))
    {
        Some(IrNodeKind::PureCall)
    } else {
        None
    }
}

fn expression_is_indexed(_expr: &AstExpr, kind: &IrNodeKind) -> bool {
    matches!(
        kind,
        IrNodeKind::ListAppend
            | IrNodeKind::ListRemove
            | IrNodeKind::ListMap
            | IrNodeKind::ListRetain
            | IrNodeKind::Aggregate
            | IrNodeKind::RenderLowering
    )
}

fn ast_expr_label(expr: &AstExpr) -> String {
    match &expr.kind {
        AstExprKind::Identifier(name)
        | AstExprKind::Number(name)
        | AstExprKind::Enum(name)
        | AstExprKind::Tag(name) => format!("{:?}", name),
        AstExprKind::Unknown(tokens) => tokens.join("_"),
        AstExprKind::Delimiter => "delimiter".to_owned(),
        AstExprKind::Path(parts) => boon_parser::canonical_value_path(parts),
        AstExprKind::Drain { .. } => "drain".to_owned(),
        AstExprKind::Draining { .. } => "draining".to_owned(),
        AstExprKind::StringLiteral(_) => "string_literal".to_owned(),
        AstExprKind::TextLiteral(_) => "text_literal".to_owned(),
        AstExprKind::ByteLiteral { value, .. } => format!("byte_{value}"),
        AstExprKind::BytesLiteral { .. } => "bytes".to_owned(),
        AstExprKind::Bool(value) => format!("bool_{value}"),
        AstExprKind::Source => "source".to_owned(),
        AstExprKind::Call { function, .. } => function.clone(),
        AstExprKind::Pipe { op, .. } => op.clone(),
        AstExprKind::Hold { name, .. } => format!("hold_{name}"),
        AstExprKind::Latest => "latest".to_owned(),
        AstExprKind::When { .. } => "when".to_owned(),
        AstExprKind::Then { .. } => "then".to_owned(),
        AstExprKind::Infix { op, .. } => format!("infix_{op}"),
        AstExprKind::MatchArm { .. } => "match_arm".to_owned(),
        AstExprKind::Block { .. } => "block".to_owned(),
        AstExprKind::Record(_) | AstExprKind::Object(_) => "object".to_owned(),
        AstExprKind::TaggedObject { tag, .. } => format!("tagged_object_{tag}"),
        AstExprKind::ListLiteral { .. } => "list".to_owned(),
    }
}

fn push_generated(nodes: &mut Vec<IrNode>, name: &str, kind: IrNodeKind, indexed: bool) {
    nodes.push(IrNode {
        id: NodeId(nodes.len()),
        name: name.to_owned(),
        kind,
        indexed,
        expr_id: None,
    });
}

fn dependency_edges(
    program: &ParsedProgram,
    cells: &[StateCell],
    candidate_sources: &mut CandidateSourceIndex<'_>,
) -> Vec<DependencyEdge> {
    let mut edges = Vec::new();
    for cell in cells {
        for source in candidate_sources.candidate_sources(&cell.path) {
            edges.push(DependencyEdge {
                indexed: cell.indexed || path_has_parsed_row_scope(program, &source),
                from: source,
                to: cell.path.clone(),
            });
        }
    }
    edges
}

fn possible_causes(
    cells: &[StateCell],
    candidate_sources: &mut CandidateSourceIndex<'_>,
) -> Vec<PossibleCause> {
    cells
        .iter()
        .map(|cell| PossibleCause {
            target: cell.path.clone(),
            sources: candidate_sources.candidate_sources(&cell.path),
        })
        .collect()
}

fn update_branches(
    program: &ParsedProgram,
    cells: &[StateCell],
    fields: &[FieldDef],
    direct_sources: &BTreeMap<String, Vec<String>>,
    candidate_sources: &mut CandidateSourceIndex<'_>,
    resolved_constants: &ResolvedConstantLookup<'_>,
) -> Vec<UpdateBranch> {
    cells
        .iter()
        .flat_map(|cell| {
            let Some(field) = fields.iter().find(|field| field.path == cell.path) else {
                return Vec::new();
            };
            let mut branches = direct_sources_for_field(direct_sources, field)
                .cloned()
                .map(|source| {
                    let branch = field.source_branch(&source).unwrap_or_default();
                    let expression = update_expression_for_routed_branch(
                        program,
                        &cell.path,
                        field,
                        fields,
                        &source,
                        &source_ref_variants(&source),
                        branch.clone(),
                        resolved_constants,
                    );
                    let guard =
                        update_guard_for_routed_branch(field, &source, &branch).or_else(|| {
                            matches!(&expression, UpdateExpression::Const { value } if value.is_empty())
                                .then(|| {
                                    then_empty_dependency_guard(field, fields, &source, &branch)
                                        .or_else(|| update_guard_for_field_source(field, &source))
                                })
                                .flatten()
                        });
                    UpdateBranch {
                        expression,
                        guard,
                        indexed: cell.indexed,
                        target: cell.path.clone(),
                        source,
                    }
                })
                .collect::<Vec<_>>();
            branches.extend(derived_dependency_update_branches(
                program,
                fields,
                field,
                cell,
                &branches,
                candidate_sources,
                resolved_constants,
            ));
            branches.extend(derived_then_empty_update_branches(
                fields,
                field,
                cell,
                direct_sources,
            ));
            branches
        })
        .collect()
}

fn verify_host_effect_calls_scheduled(
    program: &ParsedProgram,
    update_branches: &[UpdateBranch],
) -> Result<(), String> {
    let scheduled = update_branches
        .iter()
        .filter_map(|branch| match &branch.expression {
            UpdateExpression::HostEffect { call_expr_id, .. } => Some(call_expr_id.as_usize()),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    for expr in &program.expressions {
        let AstExprKind::Call { function, .. } = &expr.kind else {
            continue;
        };
        if boon_typecheck::is_typed_host_effect(function) && !scheduled.contains(&expr.id) {
            return Err(format!(
                "typed host effect `{function}` on line {} is not a dependency-triggered HOLD update",
                expr.line
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn derived_dependency_update_branches(
    program: &ParsedProgram,
    fields: &[FieldDef],
    field: &FieldDef,
    cell: &StateCell,
    existing_branches: &[UpdateBranch],
    candidate_sources: &mut CandidateSourceIndex<'_>,
    resolved_constants: &ResolvedConstantLookup<'_>,
) -> Vec<UpdateBranch> {
    let mut branches = Vec::new();
    for dependency_path in candidate_sources.dependency_paths(&field.path) {
        let Some(dependency) = fields
            .iter()
            .find(|dependency| dependency.path == dependency_path)
        else {
            continue;
        };
        if !field_dependency_is_event_cause(field, dependency) {
            continue;
        }
        for source in candidate_sources.event_sources_for_dependency(&dependency.path) {
            if cell.indexed && candidate_sources.is_effect_result_state(&source) {
                continue;
            }
            if existing_branches
                .iter()
                .chain(branches.iter())
                .any(|branch: &UpdateBranch| branch.source == source)
            {
                continue;
            }
            let Some((expression, guard)) = update_expression_for_derived_dependency_source(
                program,
                &cell.path,
                field,
                fields,
                dependency,
                &source,
                resolved_constants,
            ) else {
                continue;
            };
            branches.push(UpdateBranch {
                expression,
                guard,
                indexed: cell.indexed,
                target: cell.path.clone(),
                source,
            });
        }
    }
    branches
}

fn derived_then_empty_update_branches(
    fields: &[FieldDef],
    field: &FieldDef,
    cell: &StateCell,
    direct_sources: &BTreeMap<String, Vec<String>>,
) -> Vec<UpdateBranch> {
    let mut branches = Vec::new();
    for dependency in fields.iter().filter(|dependency| {
        dependency.parent_path == field.parent_path
            && dependency.path != field.path
            && field.mentions_identifier_expr(&dependency.local_name)
            && field.has_then_from_local_with_empty_output(&dependency.local_name)
    }) {
        for source in direct_sources_for_field(direct_sources, dependency).cloned() {
            if branches
                .iter()
                .any(|branch: &UpdateBranch| branch.source == source)
            {
                continue;
            }
            branches.push(UpdateBranch {
                expression: UpdateExpression::Const {
                    value: String::new(),
                },
                guard: dependency
                    .source_branch(&source)
                    .and_then(|branch| update_guard_for_routed_branch(dependency, &source, &branch))
                    .or_else(|| update_guard_for_field_source(dependency, &source)),
                indexed: cell.indexed,
                target: cell.path.clone(),
                source,
            });
        }
    }
    branches
}

fn unbound_list_operations(program: &ParsedProgram) -> Vec<UnboundListOperation> {
    let fields = typed_field_defs(program);
    let mut operations = Vec::new();
    for field in &fields {
        let list_name = field
            .path
            .rsplit_once('.')
            .map_or(field.path.as_str(), |(_, local)| local);
        if !program
            .list_memories
            .iter()
            .any(|list| list.name == list_name)
        {
            continue;
        }
        for append_expr in list_append_exprs(field) {
            let Some(trigger) = list_append_trigger(field, append_expr) else {
                continue;
            };
            let fields = list_append_fields(field, program, &fields, append_expr);
            operations.push(UnboundListOperation {
                list: field.path.clone(),
                kind: ListOperationKind::Append { trigger, fields },
            });
        }
        for source in direct_source_refs(field, program) {
            let branch = field.source_branch(&source).unwrap_or_default();
            if branch.has_token("List/remove")
                || field.has_token("List/remove")
                || (field.has_operator("List/retain") && branch.has_token("False"))
            {
                let canonical_row_scope = row_scope_for_list(program, list_name);
                let row_scope = ast_call_argument(field, "List/retain")
                    .or_else(|| canonical_row_scope.map(str::to_owned));
                operations.push(UnboundListOperation {
                    list: field.path.clone(),
                    kind: ListOperationKind::Remove {
                        predicate: list_remove_predicate(
                            field,
                            &source,
                            &branch,
                            row_scope.as_deref(),
                            canonical_row_scope,
                        ),
                        source,
                    },
                });
            }
        }
    }
    for field in &fields {
        if field.has_operator("List/count") {
            let Some(list) = count_or_retain_source_list(field, program) else {
                continue;
            };
            let row_scope = row_scope_for_list(program, &list)
                .map(str::to_owned)
                .or_else(|| ast_call_argument(field, "List/count"));
            let canonical_row_scope = row_scope_for_list(program, &list);
            operations.push(UnboundListOperation {
                list,
                kind: ListOperationKind::Count {
                    target: field.path.clone(),
                    predicate: list_retain_predicate(
                        field,
                        row_scope.as_deref(),
                        canonical_row_scope,
                    ),
                },
            });
        } else if field.has_operator("List/retain") {
            let Some(list) = count_or_retain_source_list(field, program) else {
                continue;
            };
            let canonical_row_scope = row_scope_for_list(program, &list);
            let row_scope = ast_call_argument(field, "List/retain")
                .or_else(|| canonical_row_scope.map(str::to_owned));
            let retain_predicate =
                list_retain_predicate(field, row_scope.as_deref(), canonical_row_scope);
            if field_is_derived_list_memory_view(field, program)
                && matches!(retain_predicate, ListPredicate::Unknown { .. })
            {
                continue;
            }
            for source in
                retain_remove_sources(field, program, row_scope.as_deref(), canonical_row_scope)
            {
                let branch = field.source_branch(&source).unwrap_or_default();
                operations.push(UnboundListOperation {
                    list: list.clone(),
                    kind: ListOperationKind::Remove {
                        predicate: list_retain_remove_predicate(
                            field,
                            &source,
                            &branch,
                            row_scope.as_deref(),
                            canonical_row_scope,
                        ),
                        source,
                    },
                });
            }
            operations.push(UnboundListOperation {
                list,
                kind: ListOperationKind::Retain {
                    target: field.path.clone(),
                    predicate: retain_predicate,
                },
            });
        }
    }
    operations
}

fn bind_list_operations(
    operations: Vec<UnboundListOperation>,
    lists: &[ListMemory],
) -> Result<Vec<ListOperation>, String> {
    let lists_by_path = lists
        .iter()
        .map(|list| (list.name.as_str(), list.id))
        .collect::<BTreeMap<_, _>>();
    operations
        .into_iter()
        .map(|operation| {
            let list_id = lists_by_path
                .get(operation.list.as_str())
                .copied()
                .ok_or_else(|| {
                    format!(
                        "list operation references unknown canonical list `{}`",
                        operation.list
                    )
                })?;
            Ok(ListOperation {
                list_id,
                list: operation.list,
                kind: operation.kind,
            })
        })
        .collect()
}

fn list_projections(program: &ParsedProgram) -> Vec<ListProjection> {
    typed_field_defs(program)
        .into_iter()
        .filter_map(|field| {
            if field.has_operator("List/query") {
                let field_paths = ast_named_call_argument(&field, "List/query", "fields")
                    .map(|value| parse_query_csv(&value))
                    .unwrap_or_default();
                let normalizations = ast_named_call_argument(&field, "List/query", "normalization")
                    .map(|value| parse_query_csv(&value))
                    .unwrap_or_else(|| vec!["Exact".to_owned()]);
                let multi_value = ast_named_call_argument(&field, "List/query", "multi_value")
                    .map(|value| parse_query_csv(&value))
                    .unwrap_or_default()
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                let query_fields = field_paths
                    .into_iter()
                    .enumerate()
                    .map(|(index, path)| ListQueryIndexField {
                        multi_value: multi_value.contains(&path),
                        path: parse_query_path(&path),
                        normalization: parse_query_normalization(
                            normalizations
                                .get(index)
                                .or_else(|| (normalizations.len() == 1).then(|| &normalizations[0]))
                                .map(String::as_str)
                                .unwrap_or("Unknown"),
                        ),
                    })
                    .collect::<Vec<_>>();
                let dynamic = |name: &str| {
                    ast_named_call_argument(&field, "List/query", name)
                        .map(|value| canonical_local_path(&value, &field.parent_path))
                };
                let selection_name =
                    ast_named_call_argument(&field, "List/query", "select").unwrap_or_default();
                let selection = match selection_name.as_str() {
                    "Exact" => dynamic("key")
                        .map(|key| ListQuerySelection::Exact { key })
                        .unwrap_or_else(|| ListQuerySelection::Unknown {
                            value: "Exact.key".to_owned(),
                        }),
                    "Prefix" => dynamic("prefix")
                        .map(|prefix| ListQuerySelection::TextPrefix {
                            leading: dynamic("leading"),
                            prefix,
                        })
                        .unwrap_or_else(|| ListQuerySelection::Unknown {
                            value: "Prefix.prefix".to_owned(),
                        }),
                    "Range" => ListQuerySelection::Range {
                        lower: dynamic("lower"),
                        lower_inclusive: static_query_bool(&field, "lower_inclusive", true),
                        upper: dynamic("upper"),
                        upper_inclusive: static_query_bool(&field, "upper_inclusive", true),
                    },
                    "Union" => dynamic("keys")
                        .map(|keys| ListQuerySelection::Union { keys })
                        .unwrap_or_else(|| ListQuerySelection::Unknown {
                            value: "Union.keys".to_owned(),
                        }),
                    "Intersection" => dynamic("keys")
                        .map(|keys| ListQuerySelection::Intersection { keys })
                        .unwrap_or_else(|| ListQuerySelection::Unknown {
                            value: "Intersection.keys".to_owned(),
                        }),
                    _ => ListQuerySelection::Unknown {
                        value: selection_name,
                    },
                };
                let residual_name = ast_named_call_argument(&field, "List/query", "residual")
                    .unwrap_or_else(|| "None".to_owned());
                let residual_path = |name: &str| {
                    ast_named_call_argument(&field, "List/query", name)
                        .map(|value| parse_query_path(&value))
                        .unwrap_or_default()
                };
                let residual = match residual_name.as_str() {
                    "None" => None,
                    "FieldEqual" => Some(
                        dynamic("residual_value")
                            .map(|value| ListQueryResidual::FieldEqual {
                                path: residual_path("residual_field"),
                                value,
                            })
                            .unwrap_or_else(|| ListQueryResidual::Unknown {
                                value: "FieldEqual.residual_value".to_owned(),
                            }),
                    ),
                    "TextContains" => Some(
                        dynamic("needle")
                            .map(|needle| ListQueryResidual::TextContains {
                                path: residual_path("residual_field"),
                                needle,
                            })
                            .unwrap_or_else(|| ListQueryResidual::Unknown {
                                value: "TextContains.needle".to_owned(),
                            }),
                    ),
                    "NumberRange" => Some(ListQueryResidual::NumberRange {
                        path: residual_path("residual_field"),
                        minimum: dynamic("minimum"),
                        maximum: dynamic("maximum"),
                    }),
                    "Wgs84Radius" => Some(
                        match (
                            dynamic("center_latitude"),
                            dynamic("center_longitude"),
                            dynamic("radius_meters"),
                        ) {
                            (
                                Some(center_latitude),
                                Some(center_longitude),
                                Some(radius_meters),
                            ) => ListQueryResidual::Wgs84Radius {
                                latitude_path: residual_path("latitude_field"),
                                longitude_path: residual_path("longitude_field"),
                                center_latitude,
                                center_longitude,
                                radius_meters,
                            },
                            _ => ListQueryResidual::Unknown {
                                value: "Wgs84Radius.bounds".to_owned(),
                            },
                        },
                    ),
                    _ => Some(ListQueryResidual::Unknown {
                        value: residual_name,
                    }),
                };
                return Some(ListProjection {
                    target: field.path.clone(),
                    list: ast_list_projection_argument(program, &field, "List/query")?,
                    kind: ListProjectionKind::IndexedQuery {
                        fields: query_fields,
                        selection,
                        residual,
                        limit: ast_named_call_argument(&field, "List/query", "limit")
                            .and_then(|value| value.parse::<usize>().ok()),
                        cursor: dynamic("cursor"),
                        unique: static_query_bool(&field, "unique", false),
                        order: match ast_named_call_argument(&field, "List/query", "order")
                            .as_deref()
                            .unwrap_or("Ascending")
                        {
                            "Ascending" => ListQueryOrder::Ascending,
                            "Descending" => ListQueryOrder::Descending,
                            value => ListQueryOrder::Unknown {
                                value: value.to_owned(),
                            },
                        },
                    },
                });
            }
            if field.has_operator("List/query_prefix") {
                let normalization =
                    ast_named_call_argument(&field, "List/query_prefix", "normalization")
                        .unwrap_or_else(|| "Exact".to_owned());
                return Some(ListProjection {
                    target: field.path.clone(),
                    list: ast_list_projection_argument(program, &field, "List/query_prefix")?,
                    kind: ListProjectionKind::TextPrefix {
                        field: ast_named_call_argument(&field, "List/query_prefix", "field")?,
                        prefix: canonical_local_path(
                            &ast_named_call_argument(&field, "List/query_prefix", "prefix")?,
                            &field.parent_path,
                        ),
                        limit: ast_named_call_argument(&field, "List/query_prefix", "limit")
                            .and_then(|value| value.parse::<usize>().ok()),
                        normalization: match normalization.as_str() {
                            "Exact" => ListTextNormalization::Exact,
                            "TrimLowercase" => ListTextNormalization::TrimLowercase,
                            "Tokens" => ListTextNormalization::Tokens,
                            _ => ListTextNormalization::Unknown {
                                value: normalization,
                            },
                        },
                    },
                });
            }
            if field.has_operator("List/chunk") {
                return Some(ListProjection {
                    target: field.path.clone(),
                    list: ast_list_projection_argument(program, &field, "List/chunk")?,
                    kind: ListProjectionKind::Chunk {
                        size: ast_named_call_argument(&field, "List/chunk", "size")
                            .and_then(|value| value.parse::<usize>().ok()),
                        item_field: ast_named_call_argument(&field, "List/chunk", "items")
                            .unwrap_or_else(|| "items".to_owned()),
                        label_field: ast_named_call_argument(&field, "List/chunk", "label")
                            .unwrap_or_else(|| "index".to_owned()),
                    },
                });
            }
            None
        })
        .collect()
}

fn parse_query_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_query_path(value: &str) -> Vec<String> {
    value
        .split('.')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_query_normalization(value: &str) -> ListTextNormalization {
    match value {
        "Exact" => ListTextNormalization::Exact,
        "TrimLowercase" => ListTextNormalization::TrimLowercase,
        "Tokens" => ListTextNormalization::Tokens,
        value => ListTextNormalization::Unknown {
            value: value.to_owned(),
        },
    }
}

fn static_query_bool(field: &FieldDef, name: &str, default: bool) -> bool {
    ast_named_call_argument(field, "List/query", name).map_or(default, |value| value == "True")
}

fn ast_list_projection_argument(
    program: &ParsedProgram,
    field: &FieldDef,
    function: &str,
) -> Option<String> {
    let raw = ast_named_call_argument(field, function, "list").or_else(|| {
        field.ast_exprs.iter().find_map(|expression| {
            let AstExprKind::Pipe { input, op, .. } = &expression.kind else {
                return None;
            };
            (op == function)
                .then(|| ast_argument_value(field, *input))
                .flatten()
        })
    })?;
    Some(
        resolve_list_memory_argument(program, &raw, &field.parent_path)
            .unwrap_or_else(|| canonical_local_path(&raw, &field.parent_path)),
    )
}

fn resolve_list_memory_argument(
    program: &ParsedProgram,
    raw: &str,
    parent_path: &str,
) -> Option<String> {
    let canonical = canonical_local_path(raw, parent_path);
    for candidate in [raw, canonical.as_str()] {
        if program
            .list_memories
            .iter()
            .any(|list| list.name == candidate)
        {
            return Some(candidate.to_owned());
        }
    }
    let local = raw.rsplit_once('.').map(|(_, local)| local).unwrap_or(raw);
    let prefix = format!("{local}_list_");
    let mut matches = program
        .list_memories
        .iter()
        .filter(|list| list.name.starts_with(&prefix))
        .map(|list| list.name.clone())
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    (matches.len() == 1).then(|| matches.remove(0))
}

fn derived_values(
    program: &ParsedProgram,
    executable: &ExecutableProgram,
    row_scopes: &[RowScope],
    derived_list_storage: &BTreeMap<ExecutableStatementId, DerivedListStorageIds>,
    fields: &[FieldDef],
    state_cells: &[StateCell],
    sources: &[SourcePort],
    materializations: &[ContextualMaterialization],
    distributed_value_references: &[DistributedValueReference],
) -> Result<Vec<DerivedValue>, String> {
    let mut event_source_collector = ExecutableViewBindingCollector::new(
        executable,
        None,
        derived_list_storage,
        row_scopes,
        sources,
        state_cells,
        materializations,
    );
    let executable_field_statements = executable
        .statements
        .iter()
        .filter_map(|statement| match (&statement.kind, statement.value) {
            (ExecutableStatementKind::Field { path, .. }, Some(_))
            | (
                ExecutableStatementKind::List {
                    path: Some(path), ..
                },
                Some(_),
            ) => Some((path.as_str(), statement.id)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let mut typed_fields = fields.to_vec();
    let semantic_items = program.ast.semantic_parser_items().collect::<Vec<_>>();
    for target in typed_derived_list_targets(executable)? {
        if typed_fields.iter().any(|field| field.path == target.path) {
            continue;
        }
        let Some(producer) = executable
            .expressions
            .get(target.producer.as_usize())
            .filter(|expression| expression.id == target.producer)
        else {
            return Err(format!(
                "typed list field `{}` references missing producer {}",
                target.path, target.producer
            ));
        };
        if matches!(producer.kind, ExecutableExpressionKind::List { .. }) {
            continue;
        }
        let statement = find_statement_by_id(&program.ast.statements, target.statement.0 as usize)
            .ok_or_else(|| {
                format!(
                    "typed list field `{}` references missing AST statement {}",
                    target.path, target.statement
                )
            })?;
        let (parent_path, local_name) = target.path.rsplit_once('.').map_or_else(
            || (String::new(), target.path.clone()),
            |(parent, local)| (parent.to_owned(), local.to_owned()),
        );
        typed_fields.push(FieldDef {
            path: target.path,
            local_name,
            parent_path,
            statement: statement.clone(),
            ast_items: collect_statement_ast_items(statement, &semantic_items),
            ast_exprs: collect_statement_ast_exprs(statement, program),
        });
    }
    let candidate_fields = typed_fields
        .iter()
        .filter(|field| {
            let has_executable_statement =
                executable_field_statements.contains_key(field.path.as_str());
            let indexed_field = path_has_parsed_row_scope(program, &field.path);
            let typed_list_view = executable_field_statements
                .get(field.path.as_str())
                .is_some_and(|statement| {
                    derived_list_storage.contains_key(statement)
                        && executable.statements.iter().any(|candidate| {
                            candidate.id == *statement
                                && matches!(candidate.kind, ExecutableStatementKind::Field { .. })
                        })
                });
            let list_memory_path = field_is_list_memory_path(field, program)
                && !is_output_registry_value_path(&field.path);
            has_executable_statement
                && !state_cells
                    .iter()
                    .any(|cell| cell.statement_id == field.statement.id)
                && !program
                    .source_ports
                    .iter()
                    .any(|source| source.path == field.path)
                && !distributed_value_references.iter().any(|reference| {
                    matches!(
                        reference.flow_mode,
                        boon_typecheck::FlowMode::TickPresent
                            | boon_typecheck::FlowMode::PresentOrAbsent
                    ) && reference.local_alias_paths.contains(&field.path)
                })
                && (typed_list_view || indexed_field || !list_memory_path)
        })
        .collect::<Vec<_>>();
    let mut values = Vec::with_capacity(candidate_fields.len());
    for (id, field) in candidate_fields.into_iter().enumerate() {
        let executable_statement_id = executable_field_statements[field.path.as_str()];
        let executable_statement = executable
            .statements
            .iter()
            .find(|statement| statement.id == executable_statement_id)
            .ok_or_else(|| {
                format!(
                    "derived value `{}` references missing executable statement {}",
                    field.path, executable_statement_id
                )
            })?;
        let checked_result_is_list = executable_statement
            .flow_type
            .as_ref()
            .is_some_and(|flow_type| matches!(flow_type.ty, boon_typecheck::Type::List(_)));
        let structural_group = field_is_structural_group(field);
        let (trigger_arms, default_roots) = if structural_group {
            (Vec::new(), Vec::new())
        } else {
            event_source_collector.trigger_owned_arms_for_statement(executable_statement_id)?
        };
        let event_causes = trigger_arms
            .iter()
            .map(|arm| arm.cause)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let mut transform_sources = event_causes
            .iter()
            .map(|cause| match cause {
                EventCause::Source(source_id) => sources
                    .get(source_id.as_usize())
                    .filter(|source| source.id == *source_id)
                    .map(|source| source.path.clone())
                    .ok_or_else(|| format!("event cause references missing SourceId {source_id}")),
                EventCause::State(state_id) => state_cells
                    .get(state_id.as_usize())
                    .filter(|state| state.id == *state_id)
                    .map(|state| state.path.clone())
                    .ok_or_else(|| format!("event cause references missing StateId {state_id}")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        transform_sources.sort();
        transform_sources.dedup();
        let materialized_storage = derived_list_storage.get(&executable_statement_id).cloned();
        let list_memory_view = materialized_storage.is_some();
        let indexed = path_has_parsed_row_scope(program, &field.path);
        let scope_id = scope_id_for_path(row_scopes, &field.path);
        let kind = if list_memory_view {
            DerivedValueKind::ListView
        } else if structural_group {
            DerivedValueKind::Pure
        } else if !trigger_arms.is_empty() {
            DerivedValueKind::SourceEventTransform
        } else {
            match derived_value_kind(field, &[]) {
                // A parser operator spelling cannot establish keyed list
                // storage. Only the checked executable list type above can.
                DerivedValueKind::ListView if checked_result_is_list => {
                    return Err(format!(
                        "checked list value `{}` has no keyed materialized storage",
                        field.path
                    ));
                }
                DerivedValueKind::ListView => DerivedValueKind::Pure,
                // Event ownership comes only from exact executable arms.
                DerivedValueKind::SourceEventTransform => DerivedValueKind::Pure,
                kind => kind,
            }
        };
        let (causes, sources, trigger_arms, default_roots) =
            if kind == DerivedValueKind::SourceEventTransform {
                (event_causes, transform_sources, trigger_arms, default_roots)
            } else {
                (Vec::new(), Vec::new(), Vec::new(), Vec::new())
            };
        values.push(DerivedValue {
            id: FieldId(id),
            executable_statement_id,
            indexed,
            scope_id,
            startup_recompute: derived_value_startup_recompute(&kind),
            kind,
            materialized_list_id: materialized_storage.as_ref().map(|storage| storage.list_id),
            materialized_row_scope_id: materialized_storage
                .as_ref()
                .map(|storage| storage.row_scope_id),
            causes,
            trigger_arms,
            default_roots,
            path: field.path.clone(),
            sources,
            statement: field.statement.clone(),
        });
    }
    Ok(values)
}

fn field_is_structural_group(field: &FieldDef) -> bool {
    field.statement.expr.is_none()
        && !field.statement.children.is_empty()
        && field.statement.children.iter().all(|child| {
            matches!(
                child.kind,
                AstStatementKind::Field { .. } | AstStatementKind::Source { .. }
            )
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

fn field_is_list_memory_path(field: &FieldDef, program: &ParsedProgram) -> bool {
    program
        .list_memories
        .iter()
        .any(|list| field.path.ends_with(&format!(".{}", list.name)) || field.path == list.name)
}

fn field_is_derived_list_memory_view(field: &FieldDef, program: &ParsedProgram) -> bool {
    if !field_is_list_memory_path(field, program) || field.has_operator("List/append") {
        return false;
    }
    let Some(list) = field_list_memory(field, program) else {
        return false;
    };
    match list_initializer(program, list) {
        ListInitializer::RecordLiteral { rows } => list_initializer_has_dynamic_fields(&rows),
        ListInitializer::Range { .. } => false,
        ListInitializer::Empty => field.has_any_operator(&DERIVED_LIST_VIEW_OPERATORS),
        ListInitializer::Unknown { .. } => {
            field.has_any_operator(&DERIVED_LIST_VIEW_OPERATORS)
                || field_references_list_memory(field, program)
        }
    }
}

const DERIVED_LIST_VIEW_OPERATORS: [&str; 8] = [
    "List/map",
    "List/filter",
    "List/retain",
    "List/query",
    "List/query_prefix",
    "List/move_field_first",
    "List/move_field_last",
    "WHEN",
];

fn list_initializer_has_dynamic_fields(rows: &[ListInitialRecord]) -> bool {
    rows.iter().any(|row| {
        row.fields.iter().any(|field| {
            matches!(
                field.value,
                InitialValue::Unknown { .. } | InitialValue::RootInitialField { .. }
            )
        })
    })
}

fn field_references_list_memory(field: &FieldDef, program: &ParsedProgram) -> bool {
    program
        .list_memories
        .iter()
        .any(|list| list.name != field.local_name && field.mentions_identifier_expr(&list.name))
}

fn field_list_memory<'a>(
    field: &FieldDef,
    program: &'a ParsedProgram,
) -> Option<&'a boon_parser::ParsedListMemory> {
    let local = field
        .path
        .rsplit_once('.')
        .map(|(_, local)| local)
        .unwrap_or(&field.path);
    program.list_memories.iter().find(|list| list.name == local)
}

fn function_definitions(program: &ParsedProgram) -> Vec<FunctionDefinition> {
    let mut functions = Vec::new();
    collect_function_definitions(&program.ast.statements, &mut functions);
    functions
}

fn collect_function_definitions(
    statements: &[AstStatement],
    functions: &mut Vec<FunctionDefinition>,
) {
    for statement in statements {
        if let AstStatementKind::Function { name, parameters } = &statement.kind {
            functions.push(FunctionDefinition {
                id: FunctionId(functions.len()),
                name: name.clone(),
                args: parameters
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .collect(),
                statement: statement.clone(),
            });
        }
        collect_function_definitions(&statement.children, functions);
    }
}

fn derived_value_kind(field: &FieldDef, sources: &[String]) -> DerivedValueKind {
    if field.has_operator("List/count") || field.has_operator("List/every") {
        DerivedValueKind::Aggregate
    } else if field.has_operator("List/latest")
        || field_terminal_pipeline_operator(field).is_some_and(list_scalar_reducer_operator)
    {
        if !sources.is_empty() || field.has_then_expr() {
            DerivedValueKind::SourceEventTransform
        } else {
            DerivedValueKind::Pure
        }
    } else if field.has_any_operator(&[
        "List/retain",
        "List/filter",
        "List/map",
        "List/chunk",
        "List/find",
        "List/query",
        "List/query_prefix",
        "List/move_field_first",
        "List/move_field_last",
    ]) {
        DerivedValueKind::ListView
    } else if !sources.is_empty() || field.has_then_expr() {
        DerivedValueKind::SourceEventTransform
    } else if field.ast_items.is_empty() {
        DerivedValueKind::Unknown
    } else {
        DerivedValueKind::Pure
    }
}

fn field_terminal_pipeline_operator(field: &FieldDef) -> Option<&str> {
    let expr_id = field
        .statement
        .children
        .iter()
        .rev()
        .find_map(top_level_pipeline_statement_expr_id)
        .or(field.statement.expr)?;
    field
        .ast_exprs
        .iter()
        .find(|expr| expr.id == expr_id)
        .and_then(expr_operator)
}

fn top_level_pipeline_statement_expr_id(statement: &AstStatement) -> Option<usize> {
    if let Some(expr_id) = statement
        .children
        .iter()
        .rev()
        .find_map(top_level_pipeline_statement_expr_id)
    {
        return Some(expr_id);
    }
    match statement.kind {
        AstStatementKind::Expression
        | AstStatementKind::Hold { .. }
        | AstStatementKind::List { field: None, .. } => statement.expr,
        _ => None,
    }
}

fn expr_operator(expr: &AstExpr) -> Option<&str> {
    match &expr.kind {
        AstExprKind::Pipe { op, .. } => Some(op.as_str()),
        AstExprKind::Call { function, .. } => Some(function.as_str()),
        _ => None,
    }
}

fn list_scalar_reducer_operator(operator: &str) -> bool {
    matches!(operator, "Text/join" | "List/count" | "List/sum")
}

fn field_initial_value(
    field: &FieldDef,
    row_scopes: &[RowScope],
    fields: &[FieldDef],
) -> InitialValue {
    let initial_expr = field_initial_expr(field);
    let Some(expr) = initial_expr else {
        return InitialValue::Unknown {
            summary: "missing initial value".to_owned(),
        };
    };
    let current_row_scope = row_scopes
        .iter()
        .find(|scope| field.path.starts_with(&format!("{}.", scope.row_scope)))
        .map(|scope| scope.row_scope.as_str());
    let value = ast_initial_value(expr, &field.ast_exprs, row_scopes, current_row_scope);
    let InitialValue::RootInitialField { path } = value else {
        return value;
    };
    let Some(current_row_scope) = current_row_scope else {
        return InitialValue::RootInitialField { path };
    };
    let Some(canonical) = canonical_current_row_member_path(field, &path, fields) else {
        return InitialValue::RootInitialField { path };
    };
    if canonical.starts_with(&format!("{current_row_scope}.")) {
        InitialValue::RowInitialField { path: canonical }
    } else {
        InitialValue::RootInitialField { path }
    }
}

fn field_initial_expr(field: &FieldDef) -> Option<&AstExpr> {
    if let Some(initial) = field.ast_exprs.iter().find_map(|expr| match expr.kind {
        AstExprKind::Hold { initial, .. } => Some(initial),
        AstExprKind::Pipe { input, ref op, .. } if op == "HOLD" => Some(input),
        _ => None,
    }) {
        let candidate = field.ast_exprs.iter().find(|expr| expr.id == initial)?;
        if !matches!(candidate.kind, AstExprKind::Delimiter) {
            return Some(candidate);
        }
        field.ast_exprs.iter().rev().find(|expr| {
            expr.id < initial
                && !matches!(
                    expr.kind,
                    AstExprKind::Delimiter | AstExprKind::Latest | AstExprKind::Hold { .. }
                )
                && !ast_expr_is_block_marker(expr)
        })
    } else {
        field.ast_exprs.iter().find(|expr| {
            !matches!(expr.kind, AstExprKind::Latest) && !ast_expr_is_block_marker(expr)
        })
    }
}

fn ast_expr_is_block_marker(expr: &AstExpr) -> bool {
    matches!(&expr.kind, AstExprKind::Identifier(value) if value == "BLOCK")
        || matches!(&expr.kind, AstExprKind::Unknown(tokens) if tokens.first().map(String::as_str) == Some("BLOCK") && tokens.last().map(String::as_str) == Some("{"))
}

fn ast_initial_value(
    expr: &AstExpr,
    expressions: &[AstExpr],
    row_scopes: &[RowScope],
    current_row_scope: Option<&str>,
) -> InitialValue {
    match &expr.kind {
        AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => InitialValue::Text {
            value: value.clone(),
        },
        AstExprKind::Number(value) => InitialValue::Number {
            value: value.clone(),
        },
        AstExprKind::ByteLiteral { value, .. } => InitialValue::Bytes {
            bytes: vec![*value],
            fixed_len: Some(1),
        },
        AstExprKind::BytesLiteral { size, items } => initial_bytes_value(size, items, expressions)
            .unwrap_or_else(|| InitialValue::Unknown {
                summary: ast_expr_label(expr),
            }),
        AstExprKind::Bool(value) => InitialValue::Bool { value: *value },
        AstExprKind::Enum(value) | AstExprKind::Tag(value) if value == "Text/empty" => {
            InitialValue::Text {
                value: String::new(),
            }
        }
        AstExprKind::Enum(value) | AstExprKind::Tag(value) => InitialValue::Enum {
            value: value.clone(),
        },
        AstExprKind::Path(parts) if parts.as_slice() == ["Text/empty"] => InitialValue::Text {
            value: String::new(),
        },
        AstExprKind::Path(parts)
            if parts
                .first()
                .is_some_and(|root| row_scopes.iter().any(|scope| scope.row_scope == *root)) =>
        {
            InitialValue::RowInitialField {
                path: parts[1..].join("."),
            }
        }
        AstExprKind::Path(parts)
            if parts.len() == 1 && value_starts_uppercase_identifier(&parts[0]) =>
        {
            InitialValue::Enum {
                value: parts[0].clone(),
            }
        }
        AstExprKind::Path(parts) => InitialValue::RootInitialField {
            path: parts.join("."),
        },
        AstExprKind::Identifier(value)
            if current_row_scope.is_some() && !value_starts_uppercase_identifier(value) =>
        {
            InitialValue::RowInitialField {
                path: value.clone(),
            }
        }
        AstExprKind::Identifier(value) if value_starts_uppercase_identifier(value) => {
            InitialValue::Enum {
                value: value.clone(),
            }
        }
        AstExprKind::Identifier(value) => InitialValue::RootInitialField {
            path: value.clone(),
        },
        _ => InitialValue::Unknown {
            summary: ast_expr_label(expr),
        },
    }
}

fn initial_bytes_value(
    size: &BytesSizeSyntax,
    items: &[usize],
    expressions: &[AstExpr],
) -> Option<InitialValue> {
    let mut bytes = Vec::new();
    for item in items {
        let item_expr = expressions.iter().find(|expr| expr.id == *item)?;
        push_initial_bytes_expr(item_expr, expressions, &mut bytes)?;
    }
    if let BytesSizeSyntax::Fixed(expected) = size {
        if items.is_empty() && *expected > 0 {
            bytes.resize(*expected, 0);
        } else if bytes.len() != *expected {
            return None;
        }
    }
    let fixed_len = match size {
        BytesSizeSyntax::Dynamic => None,
        BytesSizeSyntax::Infer => Some(bytes.len()),
        BytesSizeSyntax::Fixed(expected) => Some(*expected),
    };
    Some(InitialValue::Bytes { bytes, fixed_len })
}

fn push_initial_bytes_expr(
    expr: &AstExpr,
    expressions: &[AstExpr],
    bytes: &mut Vec<u8>,
) -> Option<()> {
    match &expr.kind {
        AstExprKind::ByteLiteral { value, .. } => {
            bytes.push(*value);
            Some(())
        }
        AstExprKind::BytesLiteral { size, items } => {
            let InitialValue::Bytes { bytes: nested, .. } =
                initial_bytes_value(size, items, expressions)?
            else {
                return None;
            };
            bytes.extend(nested);
            Some(())
        }
        _ => None,
    }
}

fn list_initializer(
    program: &ParsedProgram,
    list: &boon_parser::ParsedListMemory,
) -> ListInitializer {
    let Some(items) = (list.line > 0)
        .then(|| {
            list_body_items(program, &list.name)
                .or_else(|| list_body_items_by_line(program, list.line))
        })
        .flatten()
    else {
        if list.line == 0 {
            return ListInitializer::Empty;
        }
        return ListInitializer::Unknown {
            summary: "list body not found".to_owned(),
        };
    };
    if items.iter().any(|item| item_has_symbol(item, "List/range")) {
        return ListInitializer::Range {
            from: extract_i64_arg_from_items(&items, "from").unwrap_or(0),
            to: extract_i64_arg_from_items(&items, "to").unwrap_or(0),
        };
    }
    if let Some(rows) = structured_list_record_rows(program, list)
        && !rows.is_empty()
    {
        return ListInitializer::RecordLiteral { rows };
    }
    let rows = list_record_literal_rows(&items);
    if !rows.is_empty() {
        return ListInitializer::RecordLiteral { rows };
    }
    if items.iter().any(|item| item_has_symbol(item, "LIST")) {
        ListInitializer::Empty
    } else {
        ListInitializer::Unknown {
            summary: items.first().map(item_summary).unwrap_or_default(),
        }
    }
}

fn structured_list_record_rows(
    program: &ParsedProgram,
    list: &boon_parser::ParsedListMemory,
) -> Option<Vec<ListInitialRecord>> {
    let statement = find_list_statement(&program.ast.statements, list)?;
    let rows = statement
        .children
        .iter()
        .filter_map(|row| {
            if matches!(row.kind, AstStatementKind::Block) {
                structured_list_record_row(program, row)
            } else {
                row.expr
                    .and_then(|expr_id| {
                        static_initial_data_expr(
                            program,
                            expr_id,
                            &BTreeMap::new(),
                            &mut Vec::new(),
                        )
                    })
                    .and_then(initial_record_from_data)
            }
        })
        .collect::<Vec<_>>();
    Some(rows)
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

fn static_initial_data_expr(
    program: &ParsedProgram,
    expr_id: usize,
    env: &BTreeMap<String, boon_data::Value>,
    call_stack: &mut Vec<String>,
) -> Option<boon_data::Value> {
    let expr = program.expressions.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => {
            Some(boon_data::Value::Text(value.clone()))
        }
        AstExprKind::Number(value) => value
            .parse::<boon_data::FiniteReal>()
            .ok()
            .map(boon_data::Value::Number),
        AstExprKind::ByteLiteral { value, .. } => boon_data::Value::integer(i64::from(*value)).ok(),
        AstExprKind::BytesLiteral { size, items } => {
            let InitialValue::Bytes { bytes, .. } =
                initial_bytes_value(size, items, &program.expressions)?
            else {
                return None;
            };
            Some(boon_data::Value::Bytes(bytes.into()))
        }
        AstExprKind::Bool(value) => Some(boon_data::Value::Bool(*value)),
        AstExprKind::Enum(tag) | AstExprKind::Tag(tag) => Some(boon_data::Value::Variant {
            tag: tag.clone(),
            fields: BTreeMap::new(),
        }),
        AstExprKind::Identifier(name) => env.get(name).cloned(),
        AstExprKind::Path(parts) if parts.len() == 1 => env.get(&parts[0]).cloned(),
        AstExprKind::Record(fields) | AstExprKind::Object(fields) => fields
            .iter()
            .filter(|field| !field.spread)
            .map(|field| {
                Some((
                    field.name.clone(),
                    static_initial_data_expr(program, field.value, env, call_stack)?,
                ))
            })
            .collect::<Option<BTreeMap<_, _>>>()
            .map(boon_data::Value::Record),
        AstExprKind::TaggedObject { tag, fields } => fields
            .iter()
            .filter(|field| !field.spread)
            .map(|field| {
                Some((
                    field.name.clone(),
                    static_initial_data_expr(program, field.value, env, call_stack)?,
                ))
            })
            .collect::<Option<BTreeMap<_, _>>>()
            .map(|fields| boon_data::Value::Variant {
                tag: tag.clone(),
                fields,
            }),
        AstExprKind::ListLiteral { items, .. } => items
            .iter()
            .map(|item| static_initial_data_expr(program, *item, env, call_stack))
            .collect::<Option<Vec<_>>>()
            .map(boon_data::Value::List),
        AstExprKind::Call { function, .. } if function == "Text/empty" => {
            Some(boon_data::Value::Text(String::new()))
        }
        AstExprKind::Call { function, args, .. } => {
            static_initial_function_call(program, expr.id, function, args, env, call_stack)
        }
        _ => None,
    }
}

fn static_initial_function_call(
    program: &ParsedProgram,
    call_expr_id: usize,
    function: &str,
    inline_args: &[boon_parser::AstCallArg],
    outer_env: &BTreeMap<String, boon_data::Value>,
    call_stack: &mut Vec<String>,
) -> Option<boon_data::Value> {
    if call_stack.len() >= 64 || call_stack.iter().any(|active| active == function) {
        return None;
    }
    let definition = find_function_statement(&program.ast.statements, function)?;
    let AstStatementKind::Function { parameters, .. } = &definition.kind else {
        return None;
    };
    let mut argument_exprs = BTreeMap::<String, usize>::new();
    for arg in inline_args {
        argument_exprs.insert(arg.named_name()?.to_owned(), arg.value);
    }
    if inline_args.is_empty() {
        let statement = find_statement_by_expr(&program.ast.statements, call_expr_id)?;
        for child in &statement.children {
            match &child.kind {
                AstStatementKind::Field { name } => {
                    argument_exprs.insert(name.clone(), child.expr?);
                }
                _ => {}
            }
        }
    }
    let mut env = BTreeMap::new();
    for parameter in parameters
        .iter()
        .filter(|parameter| parameter.kind == boon_parser::AstParameterKind::Value)
    {
        let expr_id = *argument_exprs.get(&parameter.name)?;
        env.insert(
            parameter.name.clone(),
            static_initial_data_expr(program, expr_id, outer_env, call_stack)?,
        );
    }
    call_stack.push(function.to_owned());
    let result = static_initial_function_body(program, definition, &env, call_stack);
    call_stack.pop();
    result
}

fn static_initial_function_body(
    program: &ParsedProgram,
    definition: &AstStatement,
    env: &BTreeMap<String, boon_data::Value>,
    call_stack: &mut Vec<String>,
) -> Option<boon_data::Value> {
    if let Some(block) = definition.children.iter().find(|child| {
        matches!(child.kind, AstStatementKind::Block)
            && child
                .children
                .iter()
                .any(|field| matches!(field.kind, AstStatementKind::Field { .. }))
    }) {
        let fields = block
            .children
            .iter()
            .filter_map(|field| {
                let AstStatementKind::Field { name } = &field.kind else {
                    return None;
                };
                Some((
                    name.clone(),
                    static_initial_data_expr(program, field.expr?, env, call_stack)?,
                ))
            })
            .collect::<BTreeMap<_, _>>();
        if !fields.is_empty() {
            return Some(boon_data::Value::Record(fields));
        }
    }
    let result_expr = function_result_expr_id(definition, &program.expressions)?;
    static_initial_data_expr(program, result_expr, env, call_stack)
}

fn find_function_statement<'a>(
    statements: &'a [AstStatement],
    function: &str,
) -> Option<&'a AstStatement> {
    statements.iter().find_map(|statement| {
        let matches = matches!(
            &statement.kind,
            AstStatementKind::Function { name, .. }
                if name == function
                    || function.rsplit_once('/').is_some_and(|(_, suffix)| suffix == name)
        );
        matches
            .then_some(statement)
            .or_else(|| find_function_statement(&statement.children, function))
    })
}

fn find_statement_by_expr(statements: &[AstStatement], expr_id: usize) -> Option<&AstStatement> {
    statements.iter().find_map(|statement| {
        (statement.expr == Some(expr_id))
            .then_some(statement)
            .or_else(|| find_statement_by_expr(&statement.children, expr_id))
    })
}

fn find_statement_by_id(statements: &[AstStatement], statement_id: usize) -> Option<&AstStatement> {
    statements.iter().find_map(|statement| {
        (statement.id == statement_id)
            .then_some(statement)
            .or_else(|| find_statement_by_id(&statement.children, statement_id))
    })
}

fn function_result_expr_id(statement: &AstStatement, expressions: &[AstExpr]) -> Option<usize> {
    statement
        .expr
        .filter(|expr_id| {
            expressions.iter().any(|expr| {
                expr.id == *expr_id
                    && matches!(
                        expr.kind,
                        AstExprKind::Record(_)
                            | AstExprKind::Object(_)
                            | AstExprKind::TaggedObject { .. }
                            | AstExprKind::Call { .. }
                            | AstExprKind::ListLiteral { .. }
                    )
            })
        })
        .or_else(|| {
            statement
                .children
                .iter()
                .rev()
                .find_map(|child| function_result_expr_id(child, expressions))
        })
}

fn find_list_statement<'a>(
    statements: &'a [AstStatement],
    list: &boon_parser::ParsedListMemory,
) -> Option<&'a AstStatement> {
    statements.iter().find_map(|statement| {
        let matches_list = statement.line == list.line
            && matches!(
                &statement.kind,
                AstStatementKind::List {
                    field: Some(field),
                    ..
                } if field == &list.name
            );
        matches_list
            .then_some(statement)
            .or_else(|| find_list_statement(&statement.children, list))
    })
}

fn structured_list_record_row(
    program: &ParsedProgram,
    statement: &AstStatement,
) -> Option<ListInitialRecord> {
    let fields = statement
        .children
        .iter()
        .filter_map(|field| {
            let AstStatementKind::Field { name } = &field.kind else {
                return None;
            };
            let expr = field
                .expr
                .and_then(|expr_id| program.expressions.iter().find(|expr| expr.id == expr_id))?;
            Some(ListRowInitialField {
                name: name.clone(),
                value: ast_initial_value(expr, &program.expressions, &[], None),
            })
        })
        .collect::<Vec<_>>();
    (!fields.is_empty()).then_some(ListInitialRecord { fields })
}

fn list_body_items_by_line(program: &ParsedProgram, line: usize) -> Option<Vec<AstItem>> {
    let items = program.ast.semantic_parser_items().collect::<Vec<_>>();
    items
        .iter()
        .position(|item| item.line == line)
        .map(|item_index| collect_field_ast_items(&items, item_index, items[item_index].indent))
}

fn list_body_items(program: &ParsedProgram, list_name: &str) -> Option<Vec<AstItem>> {
    let items = program.ast.semantic_parser_items().collect::<Vec<_>>();
    for (item_index, item) in items.iter().enumerate() {
        if item.field.as_deref() == Some(list_name) {
            return Some(collect_field_ast_items(&items, item_index, item.indent));
        }
    }
    None
}

fn list_record_literal_rows(items: &[AstItem]) -> Vec<ListInitialRecord> {
    let mut rows = Vec::new();
    let mut in_literal = false;
    for item in items {
        if item_has_symbol(item, "LIST") {
            in_literal = true;
            continue;
        }
        if item_has_symbol(item, "|>")
            && item
                .symbols
                .iter()
                .any(|lexeme| symbol_is_list_operator(lexeme))
        {
            break;
        }
        if !in_literal {
            continue;
        }
        if let Some(record) = list_record_literal_item(item) {
            rows.push(record);
        }
    }
    rows
}

fn list_record_literal_item(item: &AstItem) -> Option<ListInitialRecord> {
    if item.symbols.first().map(String::as_str) != Some("[")
        || item.symbols.last().map(String::as_str) != Some("]")
    {
        return None;
    }
    let mut fields = Vec::new();
    for part in split_top_level(&item.symbols[1..item.symbols.len() - 1], ",") {
        if part.len() < 3 || part.get(1).map(String::as_str) != Some(":") {
            continue;
        }
        let name = part[0].as_str();
        if !is_name(name) {
            continue;
        }
        fields.push(ListRowInitialField {
            name: name.to_owned(),
            value: literal_initial_value(&part[2..]),
        });
    }
    (!fields.is_empty()).then_some(ListInitialRecord { fields })
}

fn literal_initial_value(tokens: &[String]) -> InitialValue {
    if let Some(value) = string_literal_value(tokens) {
        return InitialValue::Text { value };
    }
    if let Some(value) = text_literal_value(tokens) {
        return InitialValue::Text { value };
    }
    if let Some(value) = signed_integer_literal_value(tokens) {
        return InitialValue::Number {
            value: value.to_string(),
        };
    }
    if let Some(value) = byte_literal_value(tokens) {
        return InitialValue::Bytes {
            bytes: vec![value],
            fixed_len: Some(1),
        };
    }
    if let Some(value) = bytes_literal_value(tokens) {
        return value;
    }
    let value = tokens_to_path(tokens);
    match value.as_str() {
        "True" => InitialValue::Bool { value: true },
        "False" => InitialValue::Bool { value: false },
        value if is_number_literal(value) => InitialValue::Number {
            value: value.to_owned(),
        },
        value if value_starts_uppercase_identifier(value) => InitialValue::Enum {
            value: value.to_owned(),
        },
        value if value_is_root_initial_field_ref(value) => InitialValue::RootInitialField {
            path: value.to_owned(),
        },
        value => InitialValue::Unknown {
            summary: value.to_owned(),
        },
    }
}

fn value_is_root_initial_field_ref(value: &str) -> bool {
    !value.is_empty() && !value_starts_uppercase_identifier(value) && value.split('.').all(is_name)
}

fn signed_integer_literal_value(tokens: &[String]) -> Option<i64> {
    match tokens {
        [value] => value.parse::<i64>().ok(),
        [sign, value] if sign == "-" && value.chars().all(|ch| ch.is_ascii_digit()) => {
            value.parse::<i64>().ok().and_then(i64::checked_neg)
        }
        [sign, value] if sign == "+" && value.chars().all(|ch| ch.is_ascii_digit()) => {
            value.parse::<i64>().ok()
        }
        _ => None,
    }
}

fn is_number_literal(value: &str) -> bool {
    value.parse::<f64>().is_ok_and(f64::is_finite)
}

fn byte_literal_value(tokens: &[String]) -> Option<u8> {
    match tokens {
        [base, suffix] => parse_byte_literal_token_parts(base, suffix),
        [combined] => {
            let split = combined.find('u')?;
            let (base, suffix) = combined.split_at(split);
            parse_byte_literal_token_parts(base, suffix)
        }
        _ => None,
    }
}

fn parse_byte_literal_token_parts(base: &str, suffix: &str) -> Option<u8> {
    let radix = match base {
        "2" => 2,
        "8" => 8,
        "10" => 10,
        "16" => 16,
        _ => return None,
    };
    let digits = suffix.strip_prefix('u')?;
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_digit(radix)) {
        return None;
    }
    let value = u16::from_str_radix(digits, radix).ok()?;
    (value <= u8::MAX as u16).then_some(value as u8)
}

fn bytes_literal_value(tokens: &[String]) -> Option<InitialValue> {
    if tokens.first().map(String::as_str) != Some("BYTES") {
        return None;
    }
    let (fixed_len, body_open) = match tokens.get(1).map(String::as_str) {
        Some("{") => (None, 1),
        Some("[") => {
            let size_close = matching_close_token(tokens, 1)?;
            let fixed_len = match &tokens[2..size_close] {
                [value] if value == "__" => Some(None),
                [value] => Some(Some(value.parse::<usize>().ok()?)),
                _ => return None,
            }?;
            if tokens.get(size_close + 1).map(String::as_str) != Some("{") {
                return None;
            }
            (fixed_len, size_close + 1)
        }
        _ => return None,
    };
    let body_close = matching_close_token(tokens, body_open)?;
    if body_close + 1 != tokens.len() {
        return None;
    }
    let mut bytes = Vec::new();
    if body_close > body_open + 1 {
        for item in split_top_level(&tokens[body_open + 1..body_close], ",") {
            if item.is_empty() {
                continue;
            }
            if let Some(byte) = byte_literal_value(&item) {
                bytes.push(byte);
            } else if let Some(InitialValue::Bytes { bytes: nested, .. }) =
                bytes_literal_value(&item)
            {
                bytes.extend(nested);
            } else {
                return None;
            }
        }
    }
    if let Some(expected) = fixed_len {
        if bytes.is_empty() && expected > 0 {
            bytes.resize(expected, 0);
        } else if bytes.len() != expected {
            return None;
        }
    }
    Some(InitialValue::Bytes { bytes, fixed_len })
}

fn matching_close_token(tokens: &[String], open: usize) -> Option<usize> {
    let (open_token, close_token) = match tokens.get(open).map(String::as_str)? {
        "[" => ("[", "]"),
        "{" => ("{", "}"),
        "(" => ("(", ")"),
        _ => return None,
    };
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token == open_token {
            depth += 1;
        } else if token == close_token {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn string_literal_value(tokens: &[String]) -> Option<String> {
    let token = tokens.first()?;
    if tokens.len() != 1 || !token.starts_with('"') || !token.ends_with('"') {
        return None;
    }
    Some(token[1..token.len().saturating_sub(1)].replace("\\\"", "\""))
}

fn text_literal_value(tokens: &[String]) -> Option<String> {
    if tokens.first().map(String::as_str) != Some("TEXT")
        || tokens.get(1).map(String::as_str) != Some("{")
    {
        return None;
    }
    let close = tokens.iter().rposition(|token| token == "}")?;
    Some(join_text_literal_tokens(&tokens[2..close]))
}

fn join_text_literal_tokens(tokens: &[String]) -> String {
    let mut output = String::new();
    let mut previous = "";
    for token in tokens {
        if output.is_empty() {
            output.push_str(token);
        } else if text_literal_needs_space(previous, token) {
            output.push(' ');
            output.push_str(token);
        } else {
            output.push_str(token);
        }
        previous = token;
    }
    output
}

fn text_literal_needs_space(previous: &str, current: &str) -> bool {
    if matches!(
        current,
        "[" | "(" | "{" | "]" | ")" | "}" | "," | "." | ":" | ";" | "%"
    ) {
        return false;
    }
    if matches!(previous, "[" | "(" | "{" | "." | ":" | "#" | "/" | "%") {
        return false;
    }
    if previous.chars().all(|ch| ch.is_ascii_digit())
        && current
            .chars()
            .next()
            .is_some_and(|ch| matches!(ch, 'x' | 'X'))
    {
        return false;
    }
    true
}

fn extract_i64_arg_from_items(items: &[AstItem], name: &str) -> Option<i64> {
    items.iter().find_map(|item| {
        item.symbols.windows(3).find_map(|window| {
            (window[0] == name && window[1] == ":")
                .then(|| window[2].parse().ok())
                .flatten()
        })
    })
}

fn ast_call_argument(field: &FieldDef, function: &str) -> Option<String> {
    ast_call_arguments(field, function).into_iter().next()
}

fn ast_call_arguments(field: &FieldDef, function: &str) -> Vec<String> {
    let inline = field
        .ast_exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            AstExprKind::Call {
                function: call_function,
                args,
                ..
            } if call_function == function => Some(args.as_slice()),
            AstExprKind::Pipe { op, args, .. } if op == function => Some(args.as_slice()),
            _ => None,
        })
        .into_iter()
        .flatten()
        .filter(|arg| arg.is_bare_binding())
        .map(|arg| arg.name.clone())
        .collect::<Vec<_>>();
    if !inline.is_empty() {
        return inline;
    }
    ast_multiline_call_statement(field, function)
        .into_iter()
        .flat_map(|statement| &statement.children)
        .filter(|child| matches!(child.kind, AstStatementKind::Expression))
        .filter_map(|child| child.expr)
        .filter_map(|expr_id| ast_argument_value(field, expr_id))
        .collect()
}

fn ast_named_call_argument(field: &FieldDef, function: &str, name: &str) -> Option<String> {
    let inline = field
        .ast_exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            AstExprKind::Call {
                function: call_function,
                args,
                ..
            } if call_function == function => Some(args.as_slice()),
            AstExprKind::Pipe { op, args, .. } if op == function => Some(args.as_slice()),
            _ => None,
        })?
        .iter()
        .find(|arg| arg.named_name() == Some(name))
        .and_then(|arg| ast_argument_value(field, arg.value));
    inline.or_else(|| {
        ast_multiline_call_statement(field, function)?
            .children
            .iter()
            .find(|child| {
                matches!(&child.kind, AstStatementKind::Field { name: field } if field == name)
            })
            .and_then(|child| child.expr)
            .and_then(|expr_id| ast_argument_value(field, expr_id))
    })
}

fn ast_multiline_call_statement<'a>(
    field: &'a FieldDef,
    function: &str,
) -> Option<&'a AstStatement> {
    fn find<'a>(
        statement: &'a AstStatement,
        expressions: &[AstExpr],
        function: &str,
    ) -> Option<&'a AstStatement> {
        if statement.expr.is_some_and(|expr_id| {
            expressions.iter().find(|expr| expr.id == expr_id).is_some_and(|expr| {
                matches!(&expr.kind, AstExprKind::Call { function: called, .. } if called == function)
                    || matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == function)
            })
        }) {
            return Some(statement);
        }
        statement
            .children
            .iter()
            .find_map(|child| find(child, expressions, function))
    }

    find(&field.statement, &field.ast_exprs, function)
}

fn ast_argument_value(field: &FieldDef, expr_id: usize) -> Option<String> {
    ast_argument_value_in_exprs(&field.ast_exprs, expr_id)
}

fn scalar_number_operand(field: &FieldDef, expr_id: usize, target: &str) -> Option<String> {
    let value = ast_argument_value(field, expr_id)?;
    if is_number_literal(&value) {
        return Some(value);
    }
    let target_field = target
        .rsplit_once('.')
        .map(|(_, field)| field)
        .unwrap_or(target);
    let canonical = if value == field.local_name || value == target_field {
        target.to_owned()
    } else {
        canonical_local_path(&value, &field.parent_path)
    };
    Some(canonical)
}

fn ast_argument_value_in_exprs(exprs: &[AstExpr], expr_id: usize) -> Option<String> {
    let expr = exprs.iter().find(|expr| expr.id == expr_id)?;
    Some(match &expr.kind {
        AstExprKind::Identifier(value)
        | AstExprKind::Enum(value)
        | AstExprKind::Tag(value)
        | AstExprKind::Number(value) => value.clone(),
        AstExprKind::ByteLiteral { value, .. } => value.to_string(),
        AstExprKind::Path(parts) => boon_parser::canonical_value_path(parts),
        AstExprKind::Bool(true) => "True".to_owned(),
        AstExprKind::Bool(false) => "False".to_owned(),
        AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => value.clone(),
        AstExprKind::Unknown(tokens) => tokens_to_path(tokens),
        AstExprKind::Delimiter => String::new(),
        AstExprKind::Source
        | AstExprKind::Drain { .. }
        | AstExprKind::Draining { .. }
        | AstExprKind::Call { .. }
        | AstExprKind::Pipe { .. }
        | AstExprKind::Hold { .. }
        | AstExprKind::Latest
        | AstExprKind::When { .. }
        | AstExprKind::Then { .. }
        | AstExprKind::Infix { .. }
        | AstExprKind::MatchArm { .. }
        | AstExprKind::Block { .. }
        | AstExprKind::Record(_)
        | AstExprKind::Object(_)
        | AstExprKind::TaggedObject { .. }
        | AstExprKind::BytesLiteral { .. }
        | AstExprKind::ListLiteral { .. } => ast_expr_label(expr),
    })
}

struct ResolvedConstantLookup<'a> {
    by_expr_id: BTreeMap<usize, &'a boon_typecheck::ResolvedConstantValue>,
}

impl<'a> ResolvedConstantLookup<'a> {
    fn new(report: &'a boon_typecheck::TypeCheckReport) -> Self {
        Self {
            by_expr_id: report
                .resolved_constant_table
                .entries
                .iter()
                .map(|entry| (entry.expr_id, &entry.value))
                .collect(),
        }
    }

    fn unsigned_integer(&self, expr_id: usize) -> Option<u64> {
        match self.by_expr_id.get(&expr_id).copied()? {
            boon_typecheck::ResolvedConstantValue::UnsignedInteger { value } => Some(*value),
            _ => None,
        }
    }

    fn signed_integer(&self, expr_id: usize) -> Option<i64> {
        match self.by_expr_id.get(&expr_id).copied()? {
            boon_typecheck::ResolvedConstantValue::SignedInteger { value } => Some(*value),
            boon_typecheck::ResolvedConstantValue::UnsignedInteger { value } => {
                i64::try_from(*value).ok()
            }
            _ => None,
        }
    }

    fn symbol(&self, expr_id: usize) -> Option<&str> {
        match self.by_expr_id.get(&expr_id).copied()? {
            boon_typecheck::ResolvedConstantValue::Symbol { value } => Some(value.as_str()),
            _ => None,
        }
    }
}

fn bytes_arg_expr_id<'a>(
    args: &'a [AstCallArg],
    names: &[&str],
    _positional_index: usize,
) -> Option<&'a AstCallArg> {
    args.iter()
        .find(|arg| arg.named_name().is_some_and(|name| names.contains(&name)))
}

fn bytes_get_input_arg_in_exprs(exprs: &[AstExpr], args: &[AstCallArg]) -> Option<String> {
    args.iter()
        .find(|arg| arg.named_name() == Some("input"))
        .or_else(|| args.iter().find(|arg| arg.is_bare_binding()))
        .and_then(|arg| ast_argument_value_in_exprs(exprs, arg.value))
}

fn bytes_get_index_arg_in_exprs(
    resolved_constants: &ResolvedConstantLookup<'_>,
    args: &[AstCallArg],
    piped: bool,
) -> Option<u64> {
    let positional_index = if piped { 0 } else { 1 };
    bytes_arg_expr_id(args, &["index"], positional_index)
        .and_then(|arg| resolved_constants.unsigned_integer(arg.value))
}

fn bytes_set_input_arg_in_exprs(exprs: &[AstExpr], args: &[AstCallArg]) -> Option<String> {
    bytes_get_input_arg_in_exprs(exprs, args)
}

fn bytes_set_index_arg_in_exprs(
    resolved_constants: &ResolvedConstantLookup<'_>,
    args: &[AstCallArg],
    piped: bool,
) -> Option<u64> {
    bytes_get_index_arg_in_exprs(resolved_constants, args, piped)
}

fn bytes_set_value_arg_in_exprs(exprs: &[AstExpr], args: &[AstCallArg], piped: bool) -> Option<u8> {
    let positional_index = if piped { 1 } else { 2 };
    let arg = bytes_arg_expr_id(args, &["value"], positional_index)?;
    let expr = exprs.iter().find(|expr| expr.id == arg.value)?;
    let AstExprKind::BytesLiteral { size, items } = &expr.kind else {
        return None;
    };
    let InitialValue::Bytes { bytes, fixed_len } = initial_bytes_value(size, items, exprs)? else {
        return None;
    };
    (fixed_len == Some(1) && bytes.len() == 1).then(|| bytes[0])
}

fn bytes_u64_arg_in_exprs(
    resolved_constants: &ResolvedConstantLookup<'_>,
    args: &[AstCallArg],
    names: &[&str],
    positional_index: usize,
) -> Option<u64> {
    bytes_arg_expr_id(args, names, positional_index)
        .and_then(|arg| resolved_constants.unsigned_integer(arg.value))
}

fn bytes_scalar_arg_in_exprs(
    exprs: &[AstExpr],
    resolved_constants: &ResolvedConstantLookup<'_>,
    args: &[AstCallArg],
    names: &[&str],
    positional_index: usize,
) -> Option<BytesScalarArg> {
    let arg = bytes_arg_expr_id(args, names, positional_index)?;
    if let Some(value) = resolved_constants.unsigned_integer(arg.value) {
        return Some(BytesScalarArg::Static(value));
    }
    ast_argument_value_in_exprs(exprs, arg.value).map(BytesScalarArg::Path)
}

fn bytes_slice_input_arg_in_exprs(exprs: &[AstExpr], args: &[AstCallArg]) -> Option<String> {
    bytes_get_input_arg_in_exprs(exprs, args)
}

fn bytes_slice_offset_arg_in_exprs(
    resolved_constants: &ResolvedConstantLookup<'_>,
    args: &[AstCallArg],
    piped: bool,
) -> Option<u64> {
    let positional_index = if piped { 0 } else { 1 };
    bytes_u64_arg_in_exprs(
        resolved_constants,
        args,
        &["offset", "start"],
        positional_index,
    )
}

fn bytes_slice_byte_count_arg_in_exprs(
    exprs: &[AstExpr],
    resolved_constants: &ResolvedConstantLookup<'_>,
    args: &[AstCallArg],
    piped: bool,
) -> Option<BytesScalarArg> {
    let positional_index = if piped { 1 } else { 2 };
    bytes_scalar_arg_in_exprs(
        exprs,
        resolved_constants,
        args,
        &["byte_count", "length", "count"],
        positional_index,
    )
}

fn bytes_count_arg_in_exprs(
    exprs: &[AstExpr],
    resolved_constants: &ResolvedConstantLookup<'_>,
    args: &[AstCallArg],
    piped: bool,
) -> Option<BytesScalarArg> {
    let positional_index = if piped { 0 } else { 1 };
    bytes_scalar_arg_in_exprs(
        exprs,
        resolved_constants,
        args,
        &["byte_count", "count", "length"],
        positional_index,
    )
}

fn bytes_zeros_byte_count_arg_in_exprs(
    resolved_constants: &ResolvedConstantLookup<'_>,
    args: &[AstCallArg],
) -> Option<u64> {
    bytes_u64_arg_in_exprs(
        resolved_constants,
        args,
        &["byte_count", "count", "length"],
        0,
    )
}

fn bytes_i64_arg_in_exprs(
    resolved_constants: &ResolvedConstantLookup<'_>,
    args: &[AstCallArg],
    names: &[&str],
    positional_index: usize,
) -> Option<i64> {
    bytes_arg_expr_id(args, names, positional_index)
        .and_then(|arg| resolved_constants.signed_integer(arg.value))
}

fn bytes_numeric_offset_arg_in_exprs(
    resolved_constants: &ResolvedConstantLookup<'_>,
    args: &[AstCallArg],
    piped: bool,
) -> Option<u64> {
    let positional_index = if piped { 0 } else { 1 };
    bytes_u64_arg_in_exprs(
        resolved_constants,
        args,
        &["offset", "start"],
        positional_index,
    )
}

fn bytes_numeric_byte_count_arg_in_exprs(
    resolved_constants: &ResolvedConstantLookup<'_>,
    args: &[AstCallArg],
    piped: bool,
) -> Option<u64> {
    let positional_index = if piped { 1 } else { 2 };
    bytes_u64_arg_in_exprs(
        resolved_constants,
        args,
        &["byte_count", "count"],
        positional_index,
    )
}

fn bytes_numeric_endian_arg_in_exprs(
    resolved_constants: &ResolvedConstantLookup<'_>,
    args: &[AstCallArg],
    piped: bool,
) -> Option<String> {
    let positional_index = if piped { 2 } else { 3 };
    let value = bytes_arg_expr_id(args, &["endian"], positional_index)
        .and_then(|arg| resolved_constants.symbol(arg.value))?;
    matches!(value, "Little" | "Big").then(|| value.to_owned())
}

fn bytes_numeric_value_arg_in_exprs(
    resolved_constants: &ResolvedConstantLookup<'_>,
    args: &[AstCallArg],
    piped: bool,
) -> Option<i64> {
    let positional_index = if piped { 3 } else { 4 };
    bytes_i64_arg_in_exprs(resolved_constants, args, &["value"], positional_index)
}

fn bytes_equal_left_arg_in_exprs(exprs: &[AstExpr], args: &[AstCallArg]) -> Option<String> {
    args.iter()
        .find(|arg| arg.named_name() == Some("input"))
        .or_else(|| args.iter().find(|arg| arg.named_name() == Some("left")))
        .or_else(|| args.iter().find(|arg| arg.is_bare_binding()))
        .and_then(|arg| ast_argument_value_in_exprs(exprs, arg.value))
}

fn bytes_equal_right_arg_in_exprs(
    exprs: &[AstExpr],
    args: &[AstCallArg],
    piped: bool,
) -> Option<String> {
    let positional_index = if piped { 0 } else { 1 };
    args.iter()
        .find(|arg| arg.named_name() == Some("with"))
        .or_else(|| args.iter().find(|arg| arg.named_name() == Some("right")))
        .or_else(|| args.iter().find(|arg| arg.named_name() == Some("other")))
        .or_else(|| {
            args.iter()
                .filter(|arg| arg.is_bare_binding())
                .nth(positional_index)
        })
        .and_then(|arg| ast_argument_value_in_exprs(exprs, arg.value))
}

fn bytes_concat_left_arg_in_exprs(exprs: &[AstExpr], args: &[AstCallArg]) -> Option<String> {
    bytes_equal_left_arg_in_exprs(exprs, args)
}

fn bytes_concat_right_arg_in_exprs(
    exprs: &[AstExpr],
    args: &[AstCallArg],
    piped: bool,
) -> Option<String> {
    bytes_equal_right_arg_in_exprs(exprs, args, piped)
}

fn bytes_search_haystack_arg_in_exprs(exprs: &[AstExpr], args: &[AstCallArg]) -> Option<String> {
    args.iter()
        .find(|arg| arg.named_name() == Some("input"))
        .or_else(|| args.iter().find(|arg| arg.named_name() == Some("haystack")))
        .or_else(|| args.iter().find(|arg| arg.is_bare_binding()))
        .and_then(|arg| ast_argument_value_in_exprs(exprs, arg.value))
}

fn bytes_search_second_arg_in_exprs(
    exprs: &[AstExpr],
    args: &[AstCallArg],
    piped: bool,
    names: &[&str],
) -> Option<String> {
    let _ = piped;
    args.iter()
        .find(|arg| arg.named_name().is_some_and(|name| names.contains(&name)))
        .and_then(|arg| ast_argument_value_in_exprs(exprs, arg.value))
}

fn text_to_bytes_input_arg_in_exprs(exprs: &[AstExpr], args: &[AstCallArg]) -> Option<String> {
    args.iter()
        .find(|arg| arg.named_name() == Some("input"))
        .or_else(|| args.iter().find(|arg| arg.named_name() == Some("text")))
        .or_else(|| args.iter().find(|arg| arg.is_bare_binding()))
        .and_then(|arg| ast_argument_value_in_exprs(exprs, arg.value))
}

fn bytes_to_text_input_arg_in_exprs(exprs: &[AstExpr], args: &[AstCallArg]) -> Option<String> {
    args.iter()
        .find(|arg| arg.named_name() == Some("input"))
        .or_else(|| args.iter().find(|arg| arg.named_name() == Some("bytes")))
        .or_else(|| args.iter().find(|arg| arg.is_bare_binding()))
        .and_then(|arg| ast_argument_value_in_exprs(exprs, arg.value))
}

fn bytes_text_input_arg_in_exprs(exprs: &[AstExpr], args: &[AstCallArg]) -> Option<String> {
    args.iter()
        .find(|arg| arg.named_name() == Some("input"))
        .or_else(|| args.iter().find(|arg| arg.named_name() == Some("text")))
        .or_else(|| args.iter().find(|arg| arg.is_bare_binding()))
        .and_then(|arg| ast_argument_value_in_exprs(exprs, arg.value))
}

fn bytes_input_arg_in_exprs(exprs: &[AstExpr], args: &[AstCallArg]) -> Option<String> {
    args.iter()
        .find(|arg| arg.named_name() == Some("input"))
        .or_else(|| args.iter().find(|arg| arg.named_name() == Some("bytes")))
        .or_else(|| args.iter().find(|arg| arg.is_bare_binding()))
        .and_then(|arg| ast_argument_value_in_exprs(exprs, arg.value))
}

fn bytes_encoding_arg_in_exprs(
    resolved_constants: &ResolvedConstantLookup<'_>,
    args: &[AstCallArg],
    piped: bool,
) -> Option<String> {
    let positional_index = if piped { 0 } else { 1 };
    bytes_arg_expr_id(args, &["encoding"], positional_index)
        .and_then(|arg| resolved_constants.symbol(arg.value))
        .map(str::to_owned)
}

fn ast_simple_update_value_in_exprs(exprs: &[AstExpr], expr_id: usize) -> Option<String> {
    let expr = exprs.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::Identifier(value)
        | AstExprKind::Enum(value)
        | AstExprKind::Tag(value)
        | AstExprKind::Number(value)
        | AstExprKind::StringLiteral(value)
        | AstExprKind::TextLiteral(value) => Some(value.clone()),
        AstExprKind::ByteLiteral { value, .. } => Some(value.to_string()),
        AstExprKind::Bool(true) => Some("True".to_owned()),
        AstExprKind::Bool(false) => Some("False".to_owned()),
        AstExprKind::Path(parts) if !parts.is_empty() => Some(parts.join(".")),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SimpleThenUpdateValue {
    Const(String),
    Path(String),
}

fn ast_simple_then_update_value_in_exprs(
    exprs: &[AstExpr],
    expr_id: usize,
) -> Option<SimpleThenUpdateValue> {
    let expr = exprs.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::Identifier(value) => Some(SimpleThenUpdateValue::Path(value.clone())),
        AstExprKind::Path(parts) if !parts.is_empty() => {
            Some(SimpleThenUpdateValue::Path(parts.join(".")))
        }
        AstExprKind::Enum(value)
        | AstExprKind::Tag(value)
        | AstExprKind::Number(value)
        | AstExprKind::StringLiteral(value)
        | AstExprKind::TextLiteral(value) => Some(SimpleThenUpdateValue::Const(value.clone())),
        AstExprKind::ByteLiteral { value, .. } => {
            Some(SimpleThenUpdateValue::Const(value.to_string()))
        }
        AstExprKind::Bool(true) => Some(SimpleThenUpdateValue::Const("True".to_owned())),
        AstExprKind::Bool(false) => Some(SimpleThenUpdateValue::Const("False".to_owned())),
        _ => None,
    }
}

fn list_append_trigger(field: &FieldDef, append_expr: &AstExpr) -> Option<String> {
    let AstExprKind::Pipe { args, .. } = &append_expr.kind else {
        return None;
    };
    let item_arg = args.iter().find(|arg| arg.named_name() == Some("item"))?;
    let value = field
        .ast_exprs
        .iter()
        .find(|expr| expr.id == item_arg.value)?;
    let trigger = match &value.kind {
        AstExprKind::Then { input, .. } => ast_argument_value(field, *input)?,
        AstExprKind::Pipe { input, .. } => ast_argument_value(field, *input)?,
        _ => ast_argument_value(field, item_arg.value)?,
    };
    (!trigger.is_empty()).then(|| canonical_local_path(&trigger, &field.parent_path))
}

fn list_append_fields(
    field: &FieldDef,
    program: &ParsedProgram,
    fields: &[FieldDef],
    append_expr: &AstExpr,
) -> Vec<ListAppendField> {
    let literal_fields = list_append_item_record_fields(field, append_expr)
        .map(|fields| {
            fields
                .iter()
                .filter_map(|record_field| {
                    let value = list_append_record_field_value(field, record_field.value)?;
                    (!record_field.name.is_empty()).then(|| ListAppendField {
                        name: record_field.name.clone(),
                        value,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !literal_fields.is_empty() {
        return literal_fields;
    }
    let statement_fields = list_append_item_statement_fields(field, append_expr);
    if !statement_fields.is_empty() {
        return statement_fields;
    }
    let referenced_fields = list_append_referenced_record_fields(field, fields, append_expr);
    if !referenced_fields.is_empty() {
        return referenced_fields;
    }
    list_append_function_constructor_fields(field, program, fields, append_expr)
}

fn list_append_referenced_record_fields(
    field: &FieldDef,
    fields: &[FieldDef],
    append_expr: &AstExpr,
) -> Vec<ListAppendField> {
    let Some(item_expr) = list_append_item_expr(field, append_expr) else {
        return Vec::new();
    };
    let Some(path) = ast_argument_value(field, item_expr.id) else {
        return Vec::new();
    };
    let path = canonical_local_path(&path, &field.parent_path);
    fields
        .iter()
        .find(|candidate| candidate.path == path)
        .and_then(|candidate| list_append_statement_fields(candidate, &candidate.statement))
        .unwrap_or_default()
}

fn list_append_item_statement_fields(
    field: &FieldDef,
    append_expr: &AstExpr,
) -> Vec<ListAppendField> {
    statement_containing_expr(&field.statement, append_expr.id)
        .or_else(|| statement_containing_span(&field.statement, append_expr.start, append_expr.end))
        .and_then(|statement| list_append_statement_fields(field, statement))
        .unwrap_or_default()
}

fn list_append_statement_fields(
    field: &FieldDef,
    statement: &AstStatement,
) -> Option<Vec<ListAppendField>> {
    let fields = statement
        .children
        .iter()
        .filter_map(|child| {
            let AstStatementKind::Field { name } = &child.kind else {
                return None;
            };
            let value = list_append_record_field_value(field, child.expr?)?;
            Some(ListAppendField {
                name: name.clone(),
                value,
            })
        })
        .collect::<Vec<_>>();
    if !fields.is_empty() {
        return Some(fields);
    }
    statement
        .children
        .iter()
        .find_map(|child| list_append_statement_fields(field, child))
}

fn list_append_item_record_fields<'a>(
    field: &'a FieldDef,
    append_expr: &AstExpr,
) -> Option<&'a [AstRecordField]> {
    let item_expr = list_append_item_expr(field, append_expr)?;
    append_item_record_fields_from_expr(field, item_expr.id).or_else(|| {
        let statement =
            statement_containing_expr(&field.statement, append_expr.id).or_else(|| {
                statement_containing_span(&field.statement, append_expr.start, append_expr.end)
            })?;
        append_item_record_fields_from_statement(field, statement)
    })
}

fn statement_containing_span(
    statement: &AstStatement,
    start: usize,
    end: usize,
) -> Option<&AstStatement> {
    if start < statement.start || end > statement.end {
        return None;
    }
    statement
        .children
        .iter()
        .find_map(|child| statement_containing_span(child, start, end))
        .or(Some(statement))
}

fn append_item_record_fields_from_statement<'a>(
    field: &'a FieldDef,
    statement: &AstStatement,
) -> Option<&'a [AstRecordField]> {
    statement
        .expr
        .and_then(|expr| append_item_record_fields_from_expr(field, expr))
        .or_else(|| {
            statement
                .children
                .iter()
                .find_map(|child| append_item_record_fields_from_statement(field, child))
        })
}

fn append_item_record_fields_from_expr(
    field: &FieldDef,
    expr_id: usize,
) -> Option<&[AstRecordField]> {
    let expr = field.ast_exprs.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::Record(_) | AstExprKind::Object(_) => record_fields_from_expr(expr),
        AstExprKind::Then {
            output: Some(output),
            ..
        } => append_item_record_fields_from_expr(field, *output),
        AstExprKind::Pipe { args, .. } | AstExprKind::Call { args, .. } => args
            .iter()
            .find_map(|arg| append_item_record_fields_from_expr(field, arg.value)),
        AstExprKind::Hold { initial, .. }
        | AstExprKind::When { input: initial, .. }
        | AstExprKind::Draining { input: initial } => {
            append_item_record_fields_from_expr(field, *initial)
        }
        AstExprKind::Infix { left, right, .. } => append_item_record_fields_from_expr(field, *left)
            .or_else(|| append_item_record_fields_from_expr(field, *right)),
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => append_item_record_fields_from_expr(field, *output),
        _ => None,
    }
}

fn record_fields_from_expr(expr: &AstExpr) -> Option<&[AstRecordField]> {
    match &expr.kind {
        AstExprKind::Record(fields) | AstExprKind::Object(fields) => Some(fields.as_slice()),
        _ => None,
    }
}

fn list_append_function_constructor_fields(
    field: &FieldDef,
    program: &ParsedProgram,
    fields: &[FieldDef],
    append_expr: &AstExpr,
) -> Vec<ListAppendField> {
    let Some((function, arg_sources)) =
        list_append_item_constructor_args(field, program, append_expr)
    else {
        return Vec::new();
    };
    let Some(row_scope) = row_scope_for_append_constructor(program, &function) else {
        return Vec::new();
    };
    let prefix = format!("{row_scope}.");
    let row_scopes = row_scopes(program);
    fields
        .iter()
        .filter(|candidate| candidate.path.starts_with(&prefix))
        .filter_map(|candidate| {
            let InitialValue::RowInitialField { path } =
                field_initial_value(candidate, &row_scopes, fields)
            else {
                return None;
            };
            let source = arg_sources.get(&path)?;
            Some(ListAppendField {
                name: candidate
                    .path
                    .strip_prefix(&prefix)
                    .unwrap_or(candidate.local_name.as_str())
                    .to_owned(),
                value: ListAppendFieldValue::Source {
                    path: source.clone(),
                },
            })
        })
        .collect()
}

fn list_append_record_field_value(
    field: &FieldDef,
    expr_id: usize,
) -> Option<ListAppendFieldValue> {
    let expr = field.ast_exprs.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::StringLiteral(value)
        | AstExprKind::TextLiteral(value)
        | AstExprKind::Number(value)
        | AstExprKind::Enum(value)
        | AstExprKind::Tag(value) => Some(ListAppendFieldValue::Const {
            value: value.clone(),
        }),
        AstExprKind::Bool(true) => Some(ListAppendFieldValue::Const {
            value: "True".to_owned(),
        }),
        AstExprKind::Bool(false) => Some(ListAppendFieldValue::Const {
            value: "False".to_owned(),
        }),
        AstExprKind::ByteLiteral { .. } | AstExprKind::BytesLiteral { .. } => {
            Some(ListAppendFieldValue::TypedConst {
                value: ast_initial_value(expr, &field.ast_exprs, &[], None),
            })
        }
        AstExprKind::Identifier(value) => Some(ListAppendFieldValue::Source {
            path: canonical_local_path(value, &field.parent_path),
        }),
        AstExprKind::Path(parts) if !parts.is_empty() => Some(ListAppendFieldValue::Source {
            path: canonical_local_path(&parts.join("."), &field.parent_path),
        }),
        _ => {
            let source = ast_argument_value(field, expr_id)?;
            (!source.is_empty()).then(|| ListAppendFieldValue::Source {
                path: canonical_local_path(&source, &field.parent_path),
            })
        }
    }
}

fn list_append_item_constructor_args(
    field: &FieldDef,
    program: &ParsedProgram,
    append_expr: &AstExpr,
) -> Option<(String, BTreeMap<String, String>)> {
    let item_expr = list_append_item_expr(field, append_expr)?;
    match &item_expr.kind {
        AstExprKind::Pipe {
            input, op, args, ..
        } => Some((
            op.clone(),
            constructor_arg_sources(
                field,
                program,
                op,
                Some(*input),
                args,
                field.parent_path.as_str(),
            )?,
        )),
        AstExprKind::Call { function, args, .. } => Some((
            function.clone(),
            constructor_arg_sources(
                field,
                program,
                function,
                None,
                args,
                field.parent_path.as_str(),
            )?,
        )),
        _ => None,
    }
}

fn constructor_arg_sources(
    field: &FieldDef,
    program: &ParsedProgram,
    function: &str,
    piped_input: Option<usize>,
    args: &[AstCallArg],
    parent_path: &str,
) -> Option<BTreeMap<String, String>> {
    let function_args = function_arg_names(program, function)?;
    let mut sources = BTreeMap::new();
    if let Some(input) = piped_input {
        let arg_name = function_args.first()?;
        let source = ast_argument_value(field, input)?;
        sources.insert(arg_name.clone(), canonical_local_path(&source, parent_path));
    }
    for arg in args {
        let arg_name = arg.named_name()?.to_owned();
        let source = ast_argument_value(field, arg.value)?;
        sources.insert(arg_name, canonical_local_path(&source, parent_path));
    }
    Some(sources)
}

fn function_arg_names(program: &ParsedProgram, function: &str) -> Option<Vec<String>> {
    function_definitions(program)
        .into_iter()
        .find(|definition| definition.name == function)
        .map(|definition| definition.args)
}

fn row_scope_for_append_constructor<'a>(
    program: &'a ParsedProgram,
    function: &str,
) -> Option<&'a str> {
    program
        .row_scope_functions
        .iter()
        .find(|scope| {
            scope.function == function
                || scope
                    .function
                    .strip_prefix("__source_row_scope_")
                    .is_some_and(|source_function| source_function == function)
        })
        .map(|scope| scope.row_scope.as_str())
}

fn list_append_item_expr<'a>(field: &'a FieldDef, append_expr: &AstExpr) -> Option<&'a AstExpr> {
    let AstExprKind::Pipe { args, .. } = &append_expr.kind else {
        return None;
    };
    let item_arg = args.iter().find(|arg| arg.named_name() == Some("item"))?;
    field
        .ast_exprs
        .iter()
        .find(|expr| expr.id == item_arg.value)
}

fn list_append_exprs(field: &FieldDef) -> impl Iterator<Item = &AstExpr> {
    field.ast_exprs.iter().filter(|expr| {
        matches!(
            &expr.kind,
            AstExprKind::Pipe { op, .. } if op == "List/append"
        )
    })
}

fn retain_remove_sources(
    field: &FieldDef,
    program: &ParsedProgram,
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> Vec<String> {
    let mut sources = direct_source_refs(field, program)
        .into_iter()
        .filter(|source| {
            let scoped = program
                .source_ports
                .iter()
                .find(|port| port.path == *source)
                .is_some_and(|port| port.scoped);
            scoped
                || retain_source_predicate(field, source, row_scope, canonical_row_scope).is_some()
        })
        .collect::<Vec<_>>();
    for source in &program.source_ports {
        if retain_source_predicate(field, &source.path, row_scope, canonical_row_scope).is_some() {
            push_unique(&mut sources, source.path.clone());
        }
    }
    if !field.has_token("False") {
        return sources;
    }
    for source in &program.source_ports {
        if source.scoped
            && source
                .path
                .split('.')
                .any(|segment| segment.contains("remove") || segment.contains("delete"))
            && source
                .path
                .split('.')
                .any(|segment| field.has_token(segment))
        {
            push_unique(&mut sources, source.path.clone());
        }
    }
    sources
}

fn list_retain_remove_predicate(
    field: &FieldDef,
    source: &str,
    branch: &RoutedBranch,
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> ListPredicate {
    if !source.starts_with("store.")
        && source
            .split('.')
            .any(|segment| segment.contains("remove") || segment.contains("delete"))
    {
        return ListPredicate::AlwaysTrue;
    }
    if branch.has_token("False") {
        return ListPredicate::AlwaysTrue;
    }
    if let Some(retain_predicate) =
        retain_source_predicate(field, source, row_scope, canonical_row_scope)
        && let Some(remove_predicate) = invert_retain_predicate(retain_predicate)
    {
        return remove_predicate;
    }
    list_remove_predicate(field, source, branch, row_scope, canonical_row_scope)
}

fn retain_source_predicate(
    field: &FieldDef,
    source: &str,
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> Option<ListPredicate> {
    list_remove_predicate_from_then_output(field, source, row_scope, canonical_row_scope).or_else(
        || {
            let branch = field.source_branch(source)?;
            let path = row_field_path_in_exprs(branch.ast_exprs(), row_scope, canonical_row_scope)?;
            let bool_not_path = branch.bool_not_path().and_then(|path| {
                canonical_row_field_path_from_raw(&path, row_scope, canonical_row_scope)
            });
            if bool_not_path.as_deref() == Some(path.as_str()) {
                Some(ListPredicate::RowFieldBoolNot { path })
            } else {
                Some(ListPredicate::RowFieldBool { path })
            }
        },
    )
}

fn invert_retain_predicate(predicate: ListPredicate) -> Option<ListPredicate> {
    match predicate {
        ListPredicate::RowFieldBool { path } => Some(ListPredicate::RowFieldBoolNot { path }),
        ListPredicate::RowFieldBoolNot { path } => Some(ListPredicate::RowFieldBool { path }),
        ListPredicate::AlwaysTrue
        | ListPredicate::SelectedFilterVisibility { .. }
        | ListPredicate::Unknown { .. } => None,
    }
}

fn list_remove_predicate(
    field: &FieldDef,
    source: &str,
    branch: &RoutedBranch,
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> ListPredicate {
    if source
        .split('.')
        .any(|segment| segment.contains("remove") || segment.contains("delete"))
    {
        return ListPredicate::AlwaysTrue;
    }
    if let Some(predicate) =
        list_remove_predicate_from_then_output(field, source, row_scope, canonical_row_scope)
    {
        return predicate;
    }
    if branch.has_bool_expr(true) {
        return ListPredicate::AlwaysTrue;
    }
    if let Some(path) = row_field_path_in_exprs(branch.ast_exprs(), row_scope, canonical_row_scope)
        && branch
            .bool_not_path()
            .and_then(|bool_not_path| {
                canonical_row_field_path_from_raw(&bool_not_path, row_scope, canonical_row_scope)
            })
            .as_deref()
            == Some(path.as_str())
    {
        return ListPredicate::RowFieldBoolNot { path };
    }
    if let Some(path) = row_field_path_in_exprs(branch.ast_exprs(), row_scope, canonical_row_scope)
    {
        return ListPredicate::RowFieldBool { path };
    }
    ListPredicate::Unknown {
        summary: branch.summary(),
    }
}

fn list_remove_predicate_from_then_output(
    field: &FieldDef,
    source: &str,
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> Option<ListPredicate> {
    field.ast_exprs.iter().find_map(|expr| {
        let AstExprKind::Then {
            input,
            output: Some(output),
        } = expr.kind
        else {
            return None;
        };
        let input_path = ast_argument_value(field, input)?;
        let matches_source = source_ref_variants(source).iter().any(|variant| {
            path_matches_source_variant(&input_path, variant)
                || path_matches_source_variant(
                    &canonical_local_path(&input_path, &field.parent_path),
                    variant,
                )
        });
        if !matches_source {
            return None;
        }
        list_predicate_from_expr(field, output, row_scope, canonical_row_scope)
    })
}

fn path_matches_source_variant(path: &str, variant: &str) -> bool {
    path == variant
        || path
            .strip_prefix(variant)
            .is_some_and(|rest| rest.starts_with('.'))
}

fn list_predicate_from_expr(
    field: &FieldDef,
    expr_id: usize,
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> Option<ListPredicate> {
    let expr = field.ast_exprs.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::Bool(true) => Some(ListPredicate::AlwaysTrue),
        AstExprKind::Latest => {
            latest_default_list_predicate(field, expr_id, row_scope, canonical_row_scope)
        }
        AstExprKind::When { .. } => {
            selected_filter_predicate_from_when_expr(field, expr_id, row_scope, canonical_row_scope)
        }
        AstExprKind::Pipe { input, op, .. } if op == "Bool/not" => {
            row_field_path_from_expr(field, *input, row_scope, canonical_row_scope)
                .map(|path| ListPredicate::RowFieldBoolNot { path })
        }
        _ => row_field_path_from_expr(field, expr_id, row_scope, canonical_row_scope)
            .map(|path| ListPredicate::RowFieldBool { path }),
    }
}

fn latest_default_list_predicate(
    field: &FieldDef,
    latest_expr_id: usize,
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> Option<ListPredicate> {
    let statement = statement_containing_expr(&field.statement, latest_expr_id)?;
    statement
        .children
        .iter()
        .find_map(|child| child.expr)
        .and_then(|expr_id| {
            list_predicate_from_expr(field, expr_id, row_scope, canonical_row_scope)
        })
}

fn statement_containing_expr(statement: &AstStatement, expr_id: usize) -> Option<&AstStatement> {
    if statement.expr == Some(expr_id) {
        return Some(statement);
    }
    statement
        .children
        .iter()
        .find_map(|child| statement_containing_expr(child, expr_id))
}

fn statement_containing_expr_graph<'a>(
    statement: &'a AstStatement,
    expr_id: usize,
    expressions: &[AstExpr],
) -> Option<&'a AstStatement> {
    if statement.expr == Some(expr_id) {
        return Some(statement);
    }
    if let Some(nested) = statement
        .children
        .iter()
        .find_map(|child| statement_containing_expr_graph(child, expr_id, expressions))
    {
        return Some(nested);
    }
    statement
        .expr
        .is_some_and(|root| expr_contains_expr_id_in_exprs(expressions, root, expr_id))
        .then_some(statement)
}

fn row_field_path_from_expr(
    field: &FieldDef,
    expr_id: usize,
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> Option<String> {
    let row_scope = row_scope?;
    let expr = field.ast_exprs.iter().find(|expr| expr.id == expr_id)?;
    let AstExprKind::Path(parts) = &expr.kind else {
        return None;
    };
    row_field_path_from_parts(parts, row_scope, canonical_row_scope)
}

fn list_retain_predicate(
    field: &FieldDef,
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> ListPredicate {
    if let Some(predicate) =
        list_retain_predicate_from_ast_arg(field, row_scope, canonical_row_scope)
    {
        return predicate;
    }
    if let Some(predicate) = field.ast_exprs.iter().find_map(|expr| match expr.kind {
        AstExprKind::When { .. } => {
            selected_filter_predicate_from_when_expr(field, expr.id, row_scope, canonical_row_scope)
        }
        _ => None,
    }) {
        return predicate;
    }
    if let Some(path) = bool_not_path_in_exprs(&field.ast_exprs) {
        let path = canonical_row_field_path_from_raw(&path, row_scope, canonical_row_scope)
            .unwrap_or(path);
        return ListPredicate::RowFieldBoolNot { path };
    }
    if let Some(path) = row_field_path_in_exprs(&field.ast_exprs, row_scope, canonical_row_scope) {
        return ListPredicate::RowFieldBool { path };
    }
    ListPredicate::Unknown {
        summary: field
            .ast_items
            .first()
            .map(item_summary)
            .unwrap_or_default(),
    }
}

fn list_retain_predicate_from_ast_arg(
    field: &FieldDef,
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> Option<ListPredicate> {
    let retain = field.ast_exprs.iter().find(|expr| {
        matches!(
            &expr.kind,
            AstExprKind::Pipe { op, .. } if op == "List/retain"
        )
    })?;
    let AstExprKind::Pipe { args, .. } = &retain.kind else {
        return None;
    };
    let predicate_arg = args
        .iter()
        .find(|arg| arg.named_name() == Some("if"))
        .or_else(|| args.get(1))?;
    list_predicate_from_expr(field, predicate_arg.value, row_scope, canonical_row_scope)
}

fn count_or_retain_source_list(field: &FieldDef, program: &ParsedProgram) -> Option<String> {
    let field_local_name = field
        .path
        .rsplit_once('.')
        .map_or(field.path.as_str(), |(_, local)| local);
    if program
        .list_memories
        .iter()
        .any(|list| list.name == field_local_name)
    {
        return Some(field.path.clone());
    }
    let count_or_retain = field.ast_exprs.iter().find(|expr| {
        matches!(
            &expr.kind,
            AstExprKind::Pipe { op, .. }
                if op == "List/count" || op == "List/retain" || op == "List/every"
        )
    })?;
    let source = source_list_from_expr(field, count_or_retain.id)?;
    let canonical = canonical_local_path(&source, &field.parent_path);
    let local_name = canonical
        .rsplit_once('.')
        .map_or(canonical.as_str(), |(_, local)| local);
    program
        .list_memories
        .iter()
        .any(|list| list.name == local_name)
        .then_some(canonical)
}

fn source_list_from_expr(field: &FieldDef, expr_id: usize) -> Option<String> {
    let expr = field.ast_exprs.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::Identifier(name) if is_name(name) => Some(name.clone()),
        AstExprKind::Path(parts) if parts.len() == 1 => parts.first().cloned(),
        AstExprKind::Pipe { input, .. } => source_list_from_expr(field, *input)
            .or_else(|| previous_source_list_expr(field, *input)),
        _ => None,
    }
}

fn previous_source_list_expr(field: &FieldDef, before_id: usize) -> Option<String> {
    field
        .ast_exprs
        .iter()
        .filter(|candidate| candidate.id < before_id)
        .rev()
        .find_map(|candidate| match &candidate.kind {
            AstExprKind::Identifier(name) if is_name(name) => Some(name.clone()),
            AstExprKind::Path(parts) if parts.len() == 1 => parts.first().cloned(),
            AstExprKind::Pipe { .. } => source_list_from_expr(field, candidate.id),
            _ => None,
        })
}

fn row_scope_for_list<'a>(program: &'a ParsedProgram, list_name: &str) -> Option<&'a str> {
    program
        .row_scope_functions
        .iter()
        .find(|scope| scope.list == list_name)
        .map(|scope| scope.row_scope.as_str())
}

fn row_field_path_in_exprs(
    exprs: &[AstExpr],
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> Option<String> {
    let row_scope = row_scope?;
    exprs.iter().find_map(|expr| match &expr.kind {
        AstExprKind::Path(parts) => {
            row_field_path_from_parts(parts, row_scope, canonical_row_scope)
        }
        _ => None,
    })
}

fn canonical_row_field_path_from_raw(
    raw: &str,
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> Option<String> {
    let row_scope = row_scope?;
    let parts = raw.split('.').map(str::to_owned).collect::<Vec<_>>();
    row_field_path_from_parts(&parts, row_scope, canonical_row_scope).or_else(|| {
        let canonical = canonical_row_scope?;
        raw.strip_prefix(canonical)
            .is_some_and(|rest| rest.starts_with('.'))
            .then(|| raw.to_owned())
    })
}

fn selected_filter_predicate_from_when_expr(
    field: &FieldDef,
    when_expr_id: usize,
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> Option<ListPredicate> {
    let expr = field_expr(field, when_expr_id)?;
    let AstExprKind::When { input, .. } = expr.kind else {
        return None;
    };
    let selector = ast_argument_value(field, input)?;
    if selector.is_empty() {
        return None;
    }
    let row_field =
        selected_filter_row_field_for_when(field, when_expr_id, row_scope, canonical_row_scope)?;
    Some(ListPredicate::SelectedFilterVisibility {
        selector: canonical_local_path(&selector, &field.parent_path),
        row_field,
    })
}

fn selected_filter_row_field_for_when(
    field: &FieldDef,
    when_expr_id: usize,
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> Option<String> {
    let mut outputs = Vec::new();
    collect_match_arm_outputs_for_when(field, &field.statement, when_expr_id, &mut outputs);
    if outputs.is_empty() {
        outputs = match_arm_outputs_after_when_expr(field, when_expr_id);
    }
    let mut row_field = None;
    for output in outputs {
        let Some(candidate) =
            selected_filter_row_field_from_expr(field, output, row_scope, canonical_row_scope)
        else {
            continue;
        };
        if row_field
            .as_deref()
            .is_some_and(|existing| existing != candidate)
        {
            return None;
        }
        row_field = Some(candidate);
    }
    row_field
}

fn collect_match_arm_outputs_for_when(
    field: &FieldDef,
    statement: &AstStatement,
    when_expr_id: usize,
    outputs: &mut Vec<usize>,
) -> bool {
    let statement_contains_when = statement.expr.is_some_and(|expr_id| {
        expr_id == when_expr_id || expr_contains_expr_id(field, expr_id, when_expr_id)
    });
    if statement_contains_when {
        for child in &statement.children {
            if let Some(output) = match_arm_output(field, child) {
                outputs.push(output);
            }
        }
        if !outputs.is_empty() {
            return true;
        }
    }
    for child in &statement.children {
        if collect_match_arm_outputs_for_when(field, child, when_expr_id, outputs) {
            return true;
        }
    }
    false
}

fn match_arm_output(field: &FieldDef, statement: &AstStatement) -> Option<usize> {
    let expr_id = statement.expr?;
    let expr = field_expr(field, expr_id)?;
    let AstExprKind::MatchArm {
        output: Some(output),
        ..
    } = expr.kind
    else {
        return None;
    };
    Some(output)
}

fn match_arm_outputs_after_when_expr(field: &FieldDef, when_expr_id: usize) -> Vec<usize> {
    let Some(when_expr) = field_expr(field, when_expr_id) else {
        return Vec::new();
    };
    let end_line = field
        .ast_exprs
        .iter()
        .filter(|expr| {
            expr.line > when_expr.line
                && matches!(
                    expr.kind,
                    AstExprKind::When { .. } | AstExprKind::Then { .. }
                )
        })
        .map(|expr| expr.line)
        .min()
        .unwrap_or(usize::MAX);
    field
        .ast_exprs
        .iter()
        .filter(|expr| expr.line > when_expr.line && expr.line < end_line)
        .filter_map(|expr| match &expr.kind {
            AstExprKind::MatchArm {
                output: Some(output),
                ..
            } => Some(*output),
            _ => None,
        })
        .collect()
}

fn selected_filter_row_field_from_expr(
    field: &FieldDef,
    expr_id: usize,
    row_scope: Option<&str>,
    canonical_row_scope: Option<&str>,
) -> Option<String> {
    let row_scope = row_scope?;
    let expr = field_expr(field, expr_id)?;
    match &expr.kind {
        AstExprKind::Pipe { input, op, .. } if op == "Bool/not" => {
            row_field_path_from_expr(field, *input, Some(row_scope), canonical_row_scope)
        }
        AstExprKind::Path(parts) => {
            row_field_path_from_parts(parts, row_scope, canonical_row_scope)
        }
        AstExprKind::Bool(_) => None,
        _ => None,
    }
}

fn row_field_path_from_parts(
    parts: &[String],
    row_scope: &str,
    canonical_row_scope: Option<&str>,
) -> Option<String> {
    let output_scope = canonical_row_scope.unwrap_or(row_scope);
    parts.windows(2).find_map(|window| {
        (window[0] == row_scope && is_name(&window[1]))
            .then(|| format!("{output_scope}.{}", window[1]))
    })
}

fn split_top_level(tokens: &[String], separator: &str) -> Vec<Vec<String>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();
    let mut depth = 0i32;
    for token in tokens {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            _ => {}
        }
        if token == separator && depth == 0 {
            groups.push(std::mem::take(&mut current));
        } else {
            current.push(token.clone());
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

fn tokens_to_path(tokens: &[String]) -> String {
    tokens
        .iter()
        .filter(|token| !matches!(token.as_str(), "{" | "}" | "[" | "]"))
        .fold(String::new(), |mut output, token| {
            if token == "."
                || output.ends_with('.')
                || output.is_empty()
                || matches!(token.as_str(), ":" | "(" | ")")
                || output.ends_with('(')
                || output.ends_with(':')
            {
                output.push_str(token);
            } else {
                output.push(' ');
                output.push_str(token);
            }
            output
        })
        .trim()
        .to_owned()
}

fn dotted_path_parts(path: &str) -> Vec<&str> {
    path.split('.').filter(|part| !part.is_empty()).collect()
}

fn path_parts_match_source_ref(candidate: &[String], expected: &[&str]) -> bool {
    let candidate = candidate
        .iter()
        .filter(|part| part.as_str() != "PASSED")
        .map(String::as_str)
        .collect::<Vec<_>>();
    if source_ref_parts_match(&candidate, expected) {
        return true;
    }
    let candidate = candidate
        .into_iter()
        .filter(|part| *part != "events")
        .collect::<Vec<_>>();
    source_ref_parts_match(&candidate, expected)
}

fn source_ref_parts_match(candidate: &[&str], expected: &[&str]) -> bool {
    if candidate
        .iter()
        .take(expected.len())
        .copied()
        .eq(expected.iter().copied())
    {
        return true;
    }
    expected.len() > 1
        && candidate
            .windows(expected.len())
            .any(|window| window.iter().copied().eq(expected.iter().copied()))
}

fn item_summary(item: &AstItem) -> String {
    tokens_to_path(&item.symbols)
}

fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn item_has_symbol(item: &AstItem, symbol: &str) -> bool {
    item.symbols.iter().any(|candidate| candidate == symbol)
}

fn item_starts_with_symbol(item: &AstItem, symbol: &str) -> bool {
    item.symbols
        .first()
        .is_some_and(|candidate| candidate == symbol)
}

fn item_symbols_start_with_path(item: &AstItem, expected: &[&str]) -> bool {
    if expected.is_empty() {
        return false;
    }
    let joined = expected.join(".");
    if item
        .symbols
        .first()
        .is_some_and(|candidate| candidate == &joined)
    {
        return true;
    }
    let mut index = 0usize;
    for (part_index, part) in expected.iter().enumerate() {
        if item.symbols.get(index).map(String::as_str) != Some(*part) {
            return false;
        }
        index += 1;
        if part_index + 1 < expected.len() {
            if item.symbols.get(index).map(String::as_str) != Some(".") {
                return false;
            }
            index += 1;
        }
    }
    true
}

fn symbol_is_list_operator(symbol: &str) -> bool {
    matches!(
        symbol,
        "List/map"
            | "List/filter"
            | "List/range"
            | "List/chunk"
            | "List/find"
            | "List/query"
            | "List/query_prefix"
            | "List/move_field_first"
            | "List/move_field_last"
            | "List/get"
            | "List/append"
            | "List/remove"
            | "List/retain"
            | "List/count"
            | "List/sum"
            | "List/every"
    )
}

fn canonical_local_path(path: &str, parent_path: &str) -> String {
    if path.contains('.') || path.contains('/') || parent_path.is_empty() {
        path.to_owned()
    } else {
        format!("{parent_path}.{path}")
    }
}

fn update_expression_for_derived_dependency_source(
    program: &ParsedProgram,
    target: &str,
    field: &FieldDef,
    fields: &[FieldDef],
    dependency: &FieldDef,
    source: &str,
    resolved_constants: &ResolvedConstantLookup<'_>,
) -> Option<(UpdateExpression, Option<UpdateGuard>)> {
    let branch = field
        .source_trigger_branch(&dependency.path)
        .or_else(|| field.source_trigger_branch(&dependency.local_name))?;
    let variants = source_ref_variants(source);
    if dependency.has_operator("List/latest")
        && branch_is_direct_dependency_passthrough(field, fields, dependency, &branch)
        && let Some(dependency_branch) = dependency.source_branch(source)
    {
        let expression = update_expression_for_routed_branch(
            program,
            target,
            dependency,
            fields,
            source,
            &variants,
            dependency_branch.clone(),
            resolved_constants,
        );
        let guard = update_guard_for_routed_branch(dependency, source, &dependency_branch)
            .or_else(|| update_guard_for_routed_branch(field, source, &branch));
        return Some((expression, guard));
    }
    let expression = update_expression_for_routed_branch(
        program,
        target,
        field,
        fields,
        &dependency.path,
        &variants,
        branch.clone(),
        resolved_constants,
    );
    let guard = update_guard_for_routed_branch(field, source, &branch).or_else(|| {
        branch_is_direct_dependency_passthrough(field, fields, dependency, &branch)
            .then(|| {
                dependency
                    .source_branch(source)
                    .and_then(|dependency_branch| {
                        update_guard_for_routed_branch(dependency, source, &dependency_branch)
                    })
                    .or_else(|| update_guard_for_field_source(dependency, source))
            })
            .flatten()
    });
    Some((expression, guard))
}

fn branch_is_direct_dependency_passthrough(
    field: &FieldDef,
    fields: &[FieldDef],
    dependency: &FieldDef,
    branch: &RoutedBranch,
) -> bool {
    let Some(SimpleThenUpdateValue::Path(path)) = branch.simple_update_value() else {
        return false;
    };
    canonical_scalar_update_path_for_source(field, &field.path, &path, fields, &dependency.path)
        == dependency.path
}

fn update_guard_for_routed_branch(
    field: &FieldDef,
    source: &str,
    branch: &RoutedBranch,
) -> Option<UpdateGuard> {
    let effect_calls = branch
        .ast_exprs
        .iter()
        .filter_map(|expr| match &expr.kind {
            AstExprKind::Call { function, .. }
                if boon_typecheck::is_typed_host_effect(function) =>
            {
                Some(expr.id)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if let [effect_call] = effect_calls.as_slice()
        && let Some(guard) = update_guard_for_effect_call(field, source, *effect_call)
    {
        return Some(guard);
    }

    let variants = source_ref_variants(source);
    for expr in &branch.ast_exprs {
        let AstExprKind::When { input, .. } = expr.kind else {
            continue;
        };
        let input_path = ast_argument_value(field, input);
        let values = non_skip_literal_match_patterns_after_when(branch, expr.line);
        if values.is_empty() {
            continue;
        }
        let Some(input) = input_path else {
            continue;
        };
        let source_related = variants.iter().any(|variant| {
            input == *variant
                || input
                    .strip_prefix(variant)
                    .is_some_and(|suffix| suffix.starts_with('.'))
        }) || source_payload_field_from_path(&input, &variants).is_some();
        if !source_related {
            continue;
        }
        let input = canonical_update_guard_input(field, source, &input, &variants);
        return Some(UpdateGuard::ValueOneOf { input, values });
    }
    None
}

fn update_guard_for_effect_call(
    field: &FieldDef,
    source: &str,
    effect_call_expr_id: usize,
) -> Option<UpdateGuard> {
    let mut guards = Vec::new();
    collect_effect_call_guard_path(
        field,
        source,
        &field.statement,
        effect_call_expr_id,
        &mut guards,
    );
    match guards.len() {
        0 => None,
        1 => guards.pop(),
        _ => Some(UpdateGuard::All { guards }),
    }
}

fn collect_effect_call_guard_path(
    field: &FieldDef,
    source: &str,
    statement: &AstStatement,
    effect_call_expr_id: usize,
    guards: &mut Vec<UpdateGuard>,
) -> bool {
    if !statement_subtree_contains_expr(statement, effect_call_expr_id, &field.ast_exprs) {
        return false;
    }

    if let Some(when_expr) = statement
        .expr
        .and_then(|root| first_when_expr_in_graph(field, root))
    {
        let matching_arm = statement.children.iter().find(|child| {
            child
                .expr
                .and_then(|expr_id| field_expr(field, expr_id))
                .is_some_and(|expr| matches!(expr.kind, AstExprKind::MatchArm { .. }))
                && statement_subtree_contains_expr(child, effect_call_expr_id, &field.ast_exprs)
        });
        if let Some(arm) = matching_arm {
            if let Some(guard) = update_guard_for_when_arm(field, source, when_expr, arm) {
                guards.push(guard);
            }
            collect_effect_call_guard_path(field, source, arm, effect_call_expr_id, guards);
            return true;
        }
    }

    if let Some(child) = statement
        .children
        .iter()
        .find(|child| statement_subtree_contains_expr(child, effect_call_expr_id, &field.ast_exprs))
    {
        collect_effect_call_guard_path(field, source, child, effect_call_expr_id, guards);
    }
    true
}

fn first_when_expr_in_graph(field: &FieldDef, root: usize) -> Option<usize> {
    let mut expr_ids = vec![root];
    collect_expr_ids_recursive(root, &field.ast_exprs, &mut expr_ids);
    expr_ids.into_iter().find(|expr_id| {
        field_expr(field, *expr_id).is_some_and(|expr| when_input_expr_id(expr).is_some())
    })
}

fn when_input_expr_id(expr: &AstExpr) -> Option<usize> {
    match &expr.kind {
        AstExprKind::When { input, .. } => Some(*input),
        AstExprKind::Pipe { input, op, .. } if matches!(op.as_str(), "WHEN" | "WHILE") => {
            Some(*input)
        }
        _ => None,
    }
}

fn update_guard_for_when_arm(
    field: &FieldDef,
    source: &str,
    when_expr_id: usize,
    arm: &AstStatement,
) -> Option<UpdateGuard> {
    let input = when_input_expr_id(field_expr(field, when_expr_id)?)?;
    let AstExprKind::MatchArm { pattern, .. } = &field_expr(field, arm.expr?)?.kind else {
        return None;
    };
    let value = match_const_pattern_label(pattern)?;
    if value == "__" || value_starts_lowercase_identifier(&value) {
        return None;
    }
    let variants = source_ref_variants(source);
    if let AstExprKind::Pipe {
        input: list_input,
        op,
        ..
    } = &field_expr(field, input)?.kind
        && op == "List/is_not_empty"
        && matches!(value.as_str(), "True" | "False")
    {
        let raw_input = ast_argument_value(field, *list_input)?;
        return Some(UpdateGuard::ListIsNotEmpty {
            input: canonical_update_guard_input(field, source, &raw_input, &variants),
            expected: value == "True",
        });
    }
    if let AstExprKind::Infix { left, op, right } = &field_expr(field, input)?.kind {
        let equality_required = match (op.as_str(), value.as_str()) {
            ("==", "True") | ("!=", "False") => true,
            ("!=", "True") | ("==", "False") => false,
            _ => return None,
        };
        let left = canonical_update_guard_input(
            field,
            source,
            &ast_argument_value(field, *left)?,
            &variants,
        );
        let right = canonical_update_guard_input(
            field,
            source,
            &ast_argument_value(field, *right)?,
            &variants,
        );
        return Some(if equality_required {
            UpdateGuard::ValuesEqual { left, right }
        } else {
            UpdateGuard::ValuesNotEqual { left, right }
        });
    }
    let raw_input = ast_argument_value(field, input)?;
    Some(UpdateGuard::ValueOneOf {
        input: canonical_update_guard_input(field, source, &raw_input, &variants),
        values: vec![value],
    })
}

fn canonical_update_guard_input(
    field: &FieldDef,
    source: &str,
    input: &str,
    source_variants: &[String],
) -> String {
    for variant in source_variants {
        if input == variant {
            return source.to_owned();
        }
        if let Some(suffix) = input.strip_prefix(variant)
            && suffix.starts_with('.')
        {
            return format!("{source}{suffix}");
        }
    }
    canonical_local_path(input, &field.parent_path)
}

fn update_guard_for_field_source(field: &FieldDef, source: &str) -> Option<UpdateGuard> {
    let branch = RoutedBranch {
        items: field.ast_items.clone(),
        ast_exprs: field.ast_exprs.clone(),
    };
    update_guard_for_routed_branch(field, source, &branch)
}

fn then_empty_dependency_guard(
    field: &FieldDef,
    fields: &[FieldDef],
    source: &str,
    _branch: &RoutedBranch,
) -> Option<UpdateGuard> {
    field.ast_exprs.iter().find_map(|expr| {
        let AstExprKind::Then {
            input,
            output: Some(output),
        } = expr.kind
        else {
            return None;
        };
        let output_expr = field
            .ast_exprs
            .iter()
            .find(|candidate| candidate.id == output)?;
        if ast_initial_value(output_expr, &field.ast_exprs, &[], None)
            != (InitialValue::Text {
                value: String::new(),
            })
        {
            return None;
        }
        let input = ast_argument_value(field, input)?;
        let dependency = fields.iter().find(|candidate| {
            candidate.parent_path == field.parent_path
                && (candidate.local_name == input || candidate.path == input)
        })?;
        dependency
            .source_branch(source)
            .and_then(|dependency_branch| {
                update_guard_for_routed_branch(dependency, source, &dependency_branch)
            })
            .or_else(|| update_guard_for_field_source(dependency, source))
    })
}

fn non_skip_literal_match_patterns_after_when(
    branch: &RoutedBranch,
    when_line: usize,
) -> Vec<String> {
    let end_line = branch
        .ast_exprs
        .iter()
        .filter(|expr| {
            expr.line > when_line
                && matches!(
                    expr.kind,
                    AstExprKind::When { .. } | AstExprKind::Then { .. }
                )
        })
        .map(|expr| expr.line)
        .min()
        .unwrap_or(usize::MAX);
    let has_non_skip_catch_all = branch
        .ast_exprs
        .iter()
        .filter(|expr| expr.line >= when_line && expr.line < end_line)
        .any(|expr| {
            let AstExprKind::MatchArm { pattern, output } = &expr.kind else {
                return false;
            };
            let Some(pattern) = match_const_pattern_label(pattern) else {
                return false;
            };
            let catch_all = pattern == "__" || value_starts_lowercase_identifier(&pattern);
            let skips = output.is_some_and(|output| {
                ast_simple_update_value_in_exprs(&branch.ast_exprs, output)
                    == Some("SKIP".to_owned())
            });
            catch_all && !skips
        });
    if has_non_skip_catch_all {
        return Vec::new();
    }
    let mut values = branch
        .ast_exprs
        .iter()
        .filter(|expr| expr.line >= when_line && expr.line < end_line)
        .filter_map(|expr| {
            let AstExprKind::MatchArm { pattern, output } = &expr.kind else {
                return None;
            };
            let pattern = match_const_pattern_label(pattern)?;
            if pattern == "__" || value_starts_lowercase_identifier(&pattern) {
                return None;
            }
            if let Some(output) = output
                && ast_simple_update_value_in_exprs(&branch.ast_exprs, *output)
                    == Some("SKIP".to_owned())
            {
                return None;
            }
            Some(pattern)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if values.is_empty() {
        values = branch
            .items
            .iter()
            .filter(|item| {
                item.line >= when_line
                    && item.line < end_line
                    && item.operators.iter().any(|operator| operator == "WHEN")
            })
            .flat_map(|item| non_skip_literal_match_patterns_from_symbols(&item.symbols))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
    }
    values.sort();
    values
}

fn non_skip_literal_match_patterns_from_symbols(symbols: &[String]) -> Vec<String> {
    let mut values = Vec::new();
    for (index, symbol) in symbols.iter().enumerate() {
        if symbol != "=>" || index == 0 {
            continue;
        }
        let Some(pattern) = symbols
            .get(index - 1)
            .and_then(|value| match_const_pattern_label(std::slice::from_ref(value)))
        else {
            continue;
        };
        if pattern == "__" || value_starts_lowercase_identifier(&pattern) {
            continue;
        }
        if symbols.get(index + 1).is_some_and(|value| value == "SKIP") {
            continue;
        }
        values.push(pattern);
    }
    values
}

#[allow(clippy::too_many_arguments)]
fn update_expression_for_routed_branch(
    program: &ParsedProgram,
    target: &str,
    field: &FieldDef,
    fields: &[FieldDef],
    branch_source: &str,
    variants: &[String],
    branch: RoutedBranch,
    resolved_constants: &ResolvedConstantLookup<'_>,
) -> UpdateExpression {
    if branch.has_token("=>") && branch.has_token("False") && !branch.has_token("True") {
        return UpdateExpression::Const {
            value: "False".to_owned(),
        };
    }
    if let Some(value) = branch_value_after_match(&branch, "Escape")
        && value_starts_lowercase_identifier(value)
    {
        return UpdateExpression::PreviousValue {
            path: value.to_owned(),
        };
    }
    if let Some(path) = branch.bool_not_path().filter(|path| !path.is_empty()) {
        return UpdateExpression::BoolNot { path };
    }
    if branch.has_token("Bool/not") {
        return UpdateExpression::BoolNot {
            path: target.to_owned(),
        };
    }
    if bool_toggle_when_matches_source(field, branch_source) {
        return UpdateExpression::BoolNot {
            path: target.to_owned(),
        };
    }
    if let Some(expression) = text_trim_or_previous_update(program, target, &branch) {
        return expression;
    }
    if let Some(expression) = branch.then_number_infix_expression(field, target) {
        return expression;
    }
    if let Some(expression) = branch.then_project_time_expression(field, target) {
        return expression;
    }
    if let Some(expression) = branch.then_bytes_length_expression(field, target, fields) {
        return expression;
    }
    if let Some(expression) = branch.then_bytes_is_empty_expression(field, target, fields) {
        return expression;
    }
    if let Some(expression) =
        branch.then_bytes_get_expression(field, target, fields, resolved_constants)
    {
        return expression;
    }
    if let Some(expression) =
        branch.then_list_get_expression(field, target, fields, branch_source, resolved_constants)
    {
        return expression;
    }
    if let Some(expression) =
        branch.then_bytes_set_expression(field, target, fields, resolved_constants)
    {
        return expression;
    }
    if let Some(expression) =
        branch.then_bytes_slice_expression(field, target, fields, resolved_constants)
    {
        return expression;
    }
    if let Some(expression) =
        branch.then_bytes_take_expression(field, target, fields, resolved_constants)
    {
        return expression;
    }
    if let Some(expression) =
        branch.then_bytes_drop_expression(field, target, fields, resolved_constants)
    {
        return expression;
    }
    if let Some(expression) = branch.then_bytes_zeros_expression(field, resolved_constants) {
        return expression;
    }
    if let Some(expression) = branch.then_bytes_to_hex_expression(field, target, fields) {
        return expression;
    }
    if let Some(expression) = branch.then_bytes_from_hex_expression(field, target, fields) {
        return expression;
    }
    if let Some(expression) = branch.then_bytes_to_base64_expression(field, target, fields) {
        return expression;
    }
    if let Some(expression) = branch.then_bytes_from_base64_expression(field, target, fields) {
        return expression;
    }
    if let Some(expression) =
        branch.then_bytes_read_unsigned_expression(field, target, fields, resolved_constants)
    {
        return expression;
    }
    if let Some(expression) =
        branch.then_bytes_read_signed_expression(field, target, fields, resolved_constants)
    {
        return expression;
    }
    if let Some(expression) =
        branch.then_bytes_write_unsigned_expression(field, target, fields, resolved_constants)
    {
        return expression;
    }
    if let Some(expression) =
        branch.then_bytes_write_signed_expression(field, target, fields, resolved_constants)
    {
        return expression;
    }
    if let Some(expression) = branch.host_effect_expression(field) {
        return expression;
    }
    if let Some(expression) = branch.then_text_to_number_expression(field, target, fields) {
        return expression;
    }
    if let Some(expression) =
        branch.then_text_to_bytes_expression(field, target, fields, resolved_constants)
    {
        return expression;
    }
    if let Some(expression) =
        branch.then_bytes_to_text_expression(field, target, fields, resolved_constants)
    {
        return expression;
    }
    if let Some(expression) = branch.then_bytes_concat_expression(field, target, fields) {
        return expression;
    }
    if let Some(expression) = branch.then_bytes_equal_expression(field, target, fields) {
        return expression;
    }
    if let Some(expression) = branch.then_bytes_find_expression(field, target, fields) {
        return expression;
    }
    if let Some(expression) = branch.then_bytes_starts_with_expression(field, target, fields) {
        return expression;
    }
    if let Some(expression) = branch.then_bytes_ends_with_expression(field, target, fields) {
        return expression;
    }
    if let Some(expression) =
        prefix_payload_concat_update_expression_from_items(&branch.items, variants)
    {
        return expression;
    }
    if let Some(expression) =
        guarded_then_function_match_update_expression(program, field, target, fields, branch_source)
    {
        return expression;
    }
    if let Some(expression) =
        then_function_match_update_expression(program, field, target, fields, branch_source)
    {
        return expression;
    }
    if let Some(expression) = branch_text_is_empty_match_value_update_expression(
        field,
        target,
        fields,
        branch_source,
        &branch,
    ) {
        return expression;
    }
    if let Some(expression) = branch.then_prefix_payload_concat_expression(variants) {
        return expression;
    }
    if let Some(expression) = branch.then_prefix_root_concat_expression(field, target, fields) {
        return expression;
    }
    if let Some(expression) =
        match_const_update_expression(field, target, fields, branch_source, &branch)
    {
        return expression;
    }
    if let Some(value) = branch.then_simple_update_value() {
        return match value {
            SimpleThenUpdateValue::Const(value) => UpdateExpression::Const { value },
            SimpleThenUpdateValue::Path(path) => UpdateExpression::ReadPath {
                path: canonical_scalar_update_path_for_source(
                    field,
                    target,
                    &path,
                    fields,
                    branch_source,
                ),
            },
        };
    }
    if let Some(payload_field) = variants
        .iter()
        .find_map(|variant| field.first_referenced_payload_field(variant))
    {
        return UpdateExpression::SourcePayload {
            path: payload_field,
        };
    }
    if let Some(value) = branch.simple_update_value() {
        return match value {
            SimpleThenUpdateValue::Const(value) => UpdateExpression::Const { value },
            SimpleThenUpdateValue::Path(path) => UpdateExpression::ReadPath {
                path: canonical_scalar_update_path_for_source(
                    field,
                    target,
                    &path,
                    fields,
                    branch_source,
                ),
            },
        };
    }
    if !branch.is_empty() {
        return UpdateExpression::Unknown {
            summary: branch.summary(),
        };
    }
    UpdateExpression::Unknown {
        summary: "source reaches target through derived local field".to_owned(),
    }
}

fn branch_text_is_empty_match_value_update_expression(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    source: &str,
    branch: &RoutedBranch,
) -> Option<UpdateExpression> {
    let (raw_input, text_is_empty_line) = branch_text_is_empty_input(branch)?;
    let when_expr_id = branch
        .ast_exprs
        .iter()
        .find(|expr| {
            expr.line >= text_is_empty_line && matches!(expr.kind, AstExprKind::When { .. })
        })
        .map(|expr| expr.id)?;
    let input = canonical_scalar_update_path_for_source(field, target, &raw_input, fields, source);
    let mut arms = match_value_arms_for_when(field, target, fields, when_expr_id, Some(source));
    if arms.is_empty() {
        arms = branch_inline_match_value_arms(
            field,
            target,
            fields,
            source,
            branch,
            text_is_empty_line,
        );
    }
    (!arms.is_empty()).then_some(UpdateExpression::MatchTextIsEmptyConst { input, arms })
}

fn branch_text_is_empty_input(branch: &RoutedBranch) -> Option<(String, usize)> {
    for (index, item) in branch.items.iter().enumerate() {
        if !item
            .operators
            .iter()
            .any(|operator| operator == "Text/is_empty")
            && !item_has_symbol(item, "Text/is_empty")
        {
            continue;
        }
        let input = branch.items[..index]
            .iter()
            .rev()
            .find_map(text_is_empty_item_input_path)?;
        return Some((input, item.line));
    }
    None
}

fn text_is_empty_item_input_path(item: &AstItem) -> Option<String> {
    if item.symbols.iter().any(|symbol| {
        matches!(
            symbol.as_str(),
            "|>" | "THEN" | "WHEN" | "=>" | "," | "Text/is_empty"
        )
    }) {
        return None;
    }
    let value = item_summary(item);
    (!value.is_empty()
        && value != "SKIP"
        && value != "True"
        && value != "False"
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.'))
    .then_some(value)
}

fn branch_inline_match_value_arms(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    source: &str,
    branch: &RoutedBranch,
    after_line: usize,
) -> Vec<UpdateValueMatchArm> {
    branch
        .items
        .iter()
        .filter(|item| {
            item.line >= after_line && item.operators.iter().any(|operator| operator == "WHEN")
        })
        .flat_map(|item| {
            inline_match_value_arms_from_symbols(field, target, fields, source, &item.symbols)
        })
        .collect()
}

fn inline_match_value_arms_from_symbols(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    source: &str,
    symbols: &[String],
) -> Vec<UpdateValueMatchArm> {
    let mut arms = Vec::new();
    for (index, symbol) in symbols.iter().enumerate() {
        if symbol != "=>" || index == 0 {
            continue;
        }
        let Some(pattern) = symbols
            .get(index - 1)
            .and_then(|value| match_const_pattern_label(std::slice::from_ref(value)))
        else {
            continue;
        };
        let Some(output) = symbols
            .get(index + 1)
            .filter(|value| !matches!(value.as_str(), "," | "}" | "{"))
        else {
            continue;
        };
        let output = inline_update_value_expression(field, target, fields, source, output);
        arms.push(UpdateValueMatchArm { pattern, output });
    }
    arms
}

fn inline_update_value_expression(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    source: &str,
    value: &str,
) -> UpdateValueExpression {
    let path = canonical_scalar_update_path_for_source(field, target, value, fields, source);
    if path == target || fields.iter().any(|candidate| candidate.path == path) {
        UpdateValueExpression::ReadPath { path }
    } else {
        UpdateValueExpression::Const {
            value: value.to_owned(),
        }
    }
}

fn then_function_match_update_expression(
    program: &ParsedProgram,
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    source: &str,
) -> Option<UpdateExpression> {
    field.ast_exprs.iter().find_map(|expr| {
        let AstExprKind::Then { input, output } = expr.kind else {
            return None;
        };
        if !then_input_matches_source(field, input, source) {
            return None;
        }
        let output = output.or_else(|| following_direct_then_call_expr_id(field, expr.line))?;
        let output = field
            .ast_exprs
            .iter()
            .find(|candidate| candidate.id == output)?;
        let AstExprKind::Call { function, args, .. } = &output.kind else {
            return None;
        };
        function_match_const_update_expression(program, field, target, fields, function, args)
    })
}

fn following_direct_then_call_expr_id(field: &FieldDef, line: usize) -> Option<usize> {
    field
        .ast_exprs
        .iter()
        .filter(|candidate| candidate.line > line)
        .find_map(|candidate| match candidate.kind {
            AstExprKind::Call { .. } => Some(Some(candidate.id)),
            AstExprKind::When { .. } | AstExprKind::MatchArm { .. } | AstExprKind::Then { .. } => {
                Some(None)
            }
            _ => None,
        })
        .flatten()
}

fn guarded_then_function_match_update_expression(
    program: &ParsedProgram,
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    source: &str,
) -> Option<UpdateExpression> {
    field.ast_exprs.iter().find_map(|expr| {
        let AstExprKind::Then { input, output } = expr.kind else {
            return None;
        };
        if !then_input_matches_source(field, input, source) {
            return None;
        }
        let output = output.or_else(|| following_when_expr_id(field, expr))?;
        guarded_function_match_update_expression_from_expr(
            program, field, target, fields, output, source,
        )
    })
}

fn guarded_function_match_update_expression_from_expr(
    program: &ParsedProgram,
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    expr_id: usize,
    source: &str,
) -> Option<UpdateExpression> {
    let expr = field_expr(field, expr_id)?;
    let AstExprKind::When { input, .. } = expr.kind else {
        return None;
    };
    let arms =
        guarded_match_value_arms_after_when_expr(program, field, target, fields, expr.id, source);
    if arms.is_empty() {
        return None;
    }
    if let Some(input_expr) = field_expr(field, input)
        && let AstExprKind::Infix { left, op, right } = &input_expr.kind
    {
        return Some(UpdateExpression::MatchInfixConst {
            left: update_value_expression_from_expr(field, target, fields, *left, Some(source))?,
            op: op.clone(),
            right: update_value_expression_from_expr(field, target, fields, *right, Some(source))?,
            arms,
        });
    }
    if let Some(raw_input) = text_is_empty_input_path(field, input) {
        let input =
            canonical_scalar_update_path_for_source(field, target, &raw_input, fields, source);
        return Some(UpdateExpression::MatchTextIsEmptyConst { input, arms });
    }
    let raw_input = ast_argument_value(field, input)?;
    let input = canonical_scalar_update_path_for_source(field, target, &raw_input, fields, source);
    Some(UpdateExpression::MatchValueConst { input, arms })
}

fn guarded_match_value_arms_after_when_expr(
    program: &ParsedProgram,
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    when_expr_id: usize,
    source: &str,
) -> Vec<UpdateValueMatchArm> {
    let mut arms = Vec::new();
    collect_guarded_match_value_arms_for_when(
        program,
        field,
        target,
        fields,
        &field.statement,
        when_expr_id,
        source,
        &mut arms,
    );
    if !arms.is_empty() {
        return arms;
    }
    let Some(when_expr) = field_expr(field, when_expr_id) else {
        return Vec::new();
    };
    let end_line = field
        .ast_exprs
        .iter()
        .filter(|expr| {
            expr.line > when_expr.line
                && matches!(
                    expr.kind,
                    AstExprKind::When { .. } | AstExprKind::Then { .. }
                )
        })
        .map(|expr| expr.line)
        .min()
        .unwrap_or(usize::MAX);
    field
        .ast_exprs
        .iter()
        .filter(|expr| expr.line > when_expr.line && expr.line < end_line)
        .filter_map(|expr| {
            guarded_match_value_arm_expr(program, field, target, fields, expr, source)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn collect_guarded_match_value_arms_for_when(
    program: &ParsedProgram,
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    statement: &AstStatement,
    when_expr_id: usize,
    source: &str,
    arms: &mut Vec<UpdateValueMatchArm>,
) -> bool {
    let statement_contains_when = statement.expr.is_some_and(|expr_id| {
        expr_id == when_expr_id || expr_contains_expr_id(field, expr_id, when_expr_id)
    });
    if statement_contains_when {
        for child in &statement.children {
            let Some(expr_id) = child.expr else {
                continue;
            };
            let Some(expr) = field_expr(field, expr_id) else {
                continue;
            };
            if let Some(arm) =
                guarded_match_value_arm_expr(program, field, target, fields, expr, source)
            {
                arms.push(arm);
            }
        }
        if !arms.is_empty() {
            return true;
        }
    }
    for child in &statement.children {
        if collect_guarded_match_value_arms_for_when(
            program,
            field,
            target,
            fields,
            child,
            when_expr_id,
            source,
            arms,
        ) {
            return true;
        }
    }
    false
}

fn guarded_match_value_arm_expr(
    program: &ParsedProgram,
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    expr: &AstExpr,
    source: &str,
) -> Option<UpdateValueMatchArm> {
    let AstExprKind::MatchArm {
        pattern,
        output: Some(output),
    } = &expr.kind
    else {
        return None;
    };
    let output =
        guarded_update_value_expression_from_expr(program, field, target, fields, *output, source)?;
    let pattern = match_const_pattern_label(pattern)?;
    (!pattern.is_empty()).then_some(UpdateValueMatchArm { pattern, output })
}

fn guarded_update_value_expression_from_expr(
    program: &ParsedProgram,
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    expr_id: usize,
    source: &str,
) -> Option<UpdateValueExpression> {
    let expr = field_expr(field, expr_id)?;
    if let AstExprKind::Call { function, args, .. } = &expr.kind {
        return function_match_const_update_value_expression(
            program, field, target, fields, function, args,
        );
    }
    update_value_expression_from_expr(field, target, fields, expr_id, Some(source))
}

fn function_match_const_update_value_expression(
    program: &ParsedProgram,
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    function: &str,
    args: &[AstCallArg],
) -> Option<UpdateValueExpression> {
    let UpdateExpression::MatchConst { input, arms } =
        function_match_const_update_expression(program, field, target, fields, function, args)?
    else {
        return None;
    };
    Some(UpdateValueExpression::MatchConst {
        input,
        arms: arms
            .into_iter()
            .map(|arm| UpdateValueMatchArm {
                pattern: arm.pattern,
                output: UpdateValueExpression::Const { value: arm.output },
            })
            .collect(),
    })
}

fn function_match_const_update_expression(
    program: &ParsedProgram,
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    function: &str,
    args: &[AstCallArg],
) -> Option<UpdateExpression> {
    let function = function_definition_for_call(program, function)?;
    let function_expr_ids = statement_expr_ids_recursive(&function.statement, &program.expressions);
    let function_exprs = function_expr_ids
        .iter()
        .filter_map(|expr_id| program.expressions.iter().find(|expr| expr.id == *expr_id))
        .cloned()
        .collect::<Vec<_>>();
    function_exprs.iter().find_map(|expr| {
        let AstExprKind::When { input, .. } = expr.kind else {
            return None;
        };
        let input_name = ast_argument_value_in_exprs(&function_exprs, input)?;
        let call_input = function_call_arg_update_path(
            field,
            target,
            fields,
            &field.ast_exprs,
            args,
            &function.args,
            &input_name,
        )?;
        let arms =
            match_const_arms_for_statement_exprs(&function.statement, &function_exprs, expr.id);
        (!arms.is_empty()).then_some(UpdateExpression::MatchConst {
            input: call_input,
            arms,
        })
    })
}

fn function_definition_for_call(
    program: &ParsedProgram,
    function: &str,
) -> Option<FunctionDefinition> {
    let definitions = function_definitions(program);
    definitions
        .iter()
        .find(|definition| definition.name == function)
        .cloned()
        .or_else(|| {
            let suffix = function.rsplit_once('/').map(|(_, name)| name)?;
            definitions
                .iter()
                .find(|definition| definition.name == suffix)
                .cloned()
        })
}

fn statement_expr_ids_recursive(statement: &AstStatement, expressions: &[AstExpr]) -> Vec<usize> {
    let mut ids = Vec::new();
    collect_statement_expr_ids_recursive(statement, expressions, &mut ids);
    ids
}

fn collect_statement_expr_ids_recursive(
    statement: &AstStatement,
    expressions: &[AstExpr],
    ids: &mut Vec<usize>,
) {
    if let Some(expr_id) = statement.expr {
        ids.push(expr_id);
        collect_expr_ids_recursive(expr_id, expressions, ids);
    }
    for child in &statement.children {
        collect_statement_expr_ids_recursive(child, expressions, ids);
    }
}

fn collect_expr_ids_recursive(expr_id: usize, expressions: &[AstExpr], ids: &mut Vec<usize>) {
    let Some(expr) = expressions.iter().find(|expr| expr.id == expr_id) else {
        return;
    };
    let push_child = |child_id: usize, ids: &mut Vec<usize>| {
        if !ids.contains(&child_id) {
            ids.push(child_id);
        }
        collect_expr_ids_recursive(child_id, expressions, ids);
    };
    match &expr.kind {
        AstExprKind::Call { args, .. } => {
            for arg in args {
                push_child(arg.value, ids);
            }
        }
        AstExprKind::Pipe { input, args, .. } => {
            push_child(*input, ids);
            for arg in args {
                push_child(arg.value, ids);
            }
        }
        AstExprKind::Hold { initial, .. }
        | AstExprKind::When { input: initial, .. }
        | AstExprKind::Draining { input: initial } => {
            push_child(*initial, ids);
        }
        AstExprKind::Then {
            input,
            output: Some(output),
        } => {
            push_child(*input, ids);
            push_child(*output, ids);
        }
        AstExprKind::Then {
            input,
            output: None,
        } => push_child(*input, ids),
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => push_child(*output, ids),
        AstExprKind::Block { bindings, result } => {
            for binding in bindings {
                push_child(binding.value, ids);
            }
            if let Some(result) = result {
                push_child(*result, ids);
            }
        }
        AstExprKind::Infix { left, right, .. } => {
            push_child(*left, ids);
            push_child(*right, ids);
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => {
            for record_field in fields {
                push_child(record_field.value, ids);
            }
        }
        AstExprKind::ListLiteral { items, .. } | AstExprKind::BytesLiteral { items, .. } => {
            for item in items {
                push_child(*item, ids);
            }
        }
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::Drain { .. }
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::MatchArm { output: None, .. }
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => {}
    }
}

fn function_call_arg_update_path(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    call_exprs: &[AstExpr],
    args: &[AstCallArg],
    formals: &[String],
    input_name: &str,
) -> Option<String> {
    let named_arg = args
        .iter()
        .find(|arg| arg.named_name() == Some(input_name))
        .and_then(|arg| ast_argument_value_in_exprs(call_exprs, arg.value));
    let positional_arg = formals
        .iter()
        .position(|formal| formal == input_name)
        .and_then(|index| {
            args.iter()
                .filter(|arg| arg.is_bare_binding())
                .nth(index)
                .and_then(|arg| ast_argument_value_in_exprs(call_exprs, arg.value))
        });
    named_arg
        .or(positional_arg)
        .map(|path| canonical_scalar_update_path_with_fields(field, target, &path, fields))
}

fn match_const_update_expression(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    source: &str,
    branch: &RoutedBranch,
) -> Option<UpdateExpression> {
    field
        .ast_exprs
        .iter()
        .find_map(|expr| {
            let AstExprKind::When { input, .. } = expr.kind else {
                return None;
            };
            expr_matches_source(field, input, source)
                .then(|| {
                    match_const_update_expression_from_expr(
                        field,
                        target,
                        fields,
                        expr.id,
                        Some(source),
                    )
                })
                .flatten()
        })
        .or_else(|| {
            branch.ast_exprs.iter().find_map(|expr| {
                let AstExprKind::When { input, .. } = expr.kind else {
                    return None;
                };
                expr_matches_source(field, input, source)
                    .then(|| {
                        match_const_update_expression_from_expr(
                            field,
                            target,
                            fields,
                            expr.id,
                            Some(source),
                        )
                    })
                    .flatten()
            })
        })
        .or_else(|| {
            field.ast_exprs.iter().find_map(|expr| {
                let AstExprKind::Then { input, .. } = expr.kind else {
                    return None;
                };
                (then_input_matches_source(field, input, source)
                    || branch
                        .ast_exprs
                        .iter()
                        .any(|branch_expr| branch_expr.id == expr.id))
                .then(|| {
                    match_const_update_expression_from_then_expr(
                        field,
                        target,
                        fields,
                        expr.id,
                        Some(source),
                    )
                })
                .flatten()
            })
        })
        .or_else(|| following_match_const_update_expression(field, target, fields, source, branch))
        .or_else(|| inline_match_value_update_expression(field, target, fields, source, branch))
}

fn inline_match_value_update_expression(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    source: &str,
    branch: &RoutedBranch,
) -> Option<UpdateExpression> {
    let when = branch
        .ast_exprs
        .iter()
        .find(|expr| matches!(expr.kind, AstExprKind::When { .. }))?;
    let AstExprKind::When { input, .. } = when.kind else {
        return None;
    };
    let arms = branch_inline_match_value_arms(field, target, fields, source, branch, when.line);
    if arms.is_empty() {
        return None;
    }
    if let Some(input_expr) = field_expr(field, input)
        && let AstExprKind::Infix { left, op, right } = &input_expr.kind
    {
        return Some(UpdateExpression::MatchInfixConst {
            left: update_value_expression_from_expr(field, target, fields, *left, Some(source))?,
            op: op.clone(),
            right: update_value_expression_from_expr(field, target, fields, *right, Some(source))?,
            arms,
        });
    }
    if let Some(raw_input) = text_is_empty_input_path(field, input) {
        let input =
            canonical_scalar_update_path_for_source(field, target, &raw_input, fields, source);
        return Some(UpdateExpression::MatchTextIsEmptyConst { input, arms });
    }
    let raw_input = ast_argument_value(field, input)?;
    let input = canonical_scalar_update_path_for_source(field, target, &raw_input, fields, source);
    Some(UpdateExpression::MatchValueConst { input, arms })
}

fn then_input_matches_source(field: &FieldDef, expr_id: usize, source: &str) -> bool {
    expr_matches_source(field, expr_id, source)
}

fn expr_matches_source(field: &FieldDef, expr_id: usize, source: &str) -> bool {
    let Some(input_path) = ast_argument_value(field, expr_id) else {
        return false;
    };
    let input_parts = input_path
        .split('.')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let canonical_path = canonical_local_path(&input_path, &field.parent_path);
    let canonical_parts = canonical_path
        .split('.')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    source_ref_variants(source).iter().any(|variant| {
        let expected = dotted_path_parts(variant);
        path_parts_match_source_ref(&input_parts, &expected)
            || path_parts_match_source_ref(&canonical_parts, &expected)
    })
}

fn bool_toggle_when_matches_source(field: &FieldDef, source: &str) -> bool {
    field.ast_exprs.iter().any(|expr| {
        let AstExprKind::Pipe { op, args, .. } = &expr.kind else {
            return false;
        };
        op == "Bool/toggle"
            && args
                .iter()
                .find(|arg| arg.named_name() == Some("when"))
                .is_some_and(|arg| expr_matches_source(field, arg.value, source))
    })
}

fn match_const_update_expression_from_then_expr(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    expr_id: usize,
    source: Option<&str>,
) -> Option<UpdateExpression> {
    let expr = field_expr(field, expr_id)?;
    let AstExprKind::Then { output, .. } = expr.kind else {
        return None;
    };
    let output = output.or_else(|| following_when_expr_id(field, expr))?;
    match_const_update_expression_from_expr(field, target, fields, output, source)
}

fn following_match_const_update_expression(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    source: &str,
    branch: &RoutedBranch,
) -> Option<UpdateExpression> {
    branch.ast_exprs.iter().find_map(|expr| {
        if !expr_matches_source(field, expr.id, source) {
            return None;
        }
        let then = branch.ast_exprs.iter().find(|candidate| {
            candidate.line > expr.line && matches!(candidate.kind, AstExprKind::Then { .. })
        })?;
        match_const_update_expression_from_then_expr(field, target, fields, then.id, Some(source))
    })
}

fn following_when_expr_id(field: &FieldDef, parent: &AstExpr) -> Option<usize> {
    nested_when_expr_id(&field.statement, parent.id, &field.ast_exprs).or_else(|| {
        field
            .ast_exprs
            .iter()
            .find(|candidate| {
                candidate.id != parent.id
                    && candidate.start >= parent.start
                    && candidate.end <= parent.end
                    && matches!(candidate.kind, AstExprKind::When { .. })
            })
            .map(|expr| expr.id)
    })
}

fn nested_when_expr_id(
    statement: &AstStatement,
    parent_expr_id: usize,
    exprs: &[AstExpr],
) -> Option<usize> {
    if statement.expr == Some(parent_expr_id) {
        return statement
            .children
            .iter()
            .find_map(|child| first_when_expr_id(child, exprs));
    }
    statement
        .children
        .iter()
        .find_map(|child| nested_when_expr_id(child, parent_expr_id, exprs))
}

fn first_when_expr_id(statement: &AstStatement, exprs: &[AstExpr]) -> Option<usize> {
    if let Some(expr_id) = statement.expr
        && exprs
            .iter()
            .find(|expr| expr.id == expr_id)
            .is_some_and(|expr| matches!(expr.kind, AstExprKind::When { .. }))
    {
        return Some(expr_id);
    }
    statement
        .children
        .iter()
        .find_map(|child| first_when_expr_id(child, exprs))
}

fn match_const_update_expression_from_expr(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    expr_id: usize,
    source: Option<&str>,
) -> Option<UpdateExpression> {
    let expr = field_expr(field, expr_id)?;
    match &expr.kind {
        AstExprKind::When { input, .. } => {
            if let Some(expression) = match_infix_const_update_expression_from_input(
                field, target, fields, *input, expr.id, source,
            ) {
                return Some(expression);
            }
            if let Some(expression) = match_text_is_empty_const_update_expression_from_input(
                field, target, fields, *input, expr.id, source,
            ) {
                return Some(expression);
            }
            let raw_input = ast_argument_value(field, *input)?;
            let input = source.map_or_else(
                || canonical_scalar_update_path_with_fields(field, target, &raw_input, fields),
                |source| {
                    canonical_scalar_update_path_for_source(
                        field, target, &raw_input, fields, source,
                    )
                },
            );
            let arms = match_const_arms_for_when(field, expr.id);
            if arms.is_empty() {
                return None;
            }
            let value_arms = match_value_arms_for_when(field, target, fields, expr.id, source);
            if match_value_arms_need_structured_update(&arms, &value_arms) {
                Some(UpdateExpression::MatchValueConst {
                    input,
                    arms: value_arms,
                })
            } else {
                Some(UpdateExpression::MatchConst { input, arms })
            }
        }
        AstExprKind::Then {
            output: Some(output),
            ..
        } => match_const_update_expression_from_expr(field, target, fields, *output, source),
        _ => None,
    }
}

fn match_text_is_empty_const_update_expression_from_input(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    input: usize,
    when_expr_id: usize,
    source: Option<&str>,
) -> Option<UpdateExpression> {
    let raw_input = text_is_empty_input_path(field, input)?;
    let input = source.map_or_else(
        || canonical_scalar_update_path_with_fields(field, target, &raw_input, fields),
        |source| canonical_scalar_update_path_for_source(field, target, &raw_input, fields, source),
    );
    let arms = match_value_arms_for_when(field, target, fields, when_expr_id, source);
    (!arms.is_empty()).then_some(UpdateExpression::MatchTextIsEmptyConst { input, arms })
}

fn match_infix_const_update_expression_from_input(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    input: usize,
    when_expr_id: usize,
    source: Option<&str>,
) -> Option<UpdateExpression> {
    let input = field_expr(field, input)?;
    let AstExprKind::Infix { left, op, right } = &input.kind else {
        return None;
    };
    let left = update_value_expression_from_expr(field, target, fields, *left, source)?;
    let right = update_value_expression_from_expr(field, target, fields, *right, source)?;
    let arms = match_value_arms_for_when(field, target, fields, when_expr_id, source);
    (!arms.is_empty()).then_some(UpdateExpression::MatchInfixConst {
        left,
        op: op.clone(),
        right,
        arms,
    })
}

fn text_is_empty_input_path(field: &FieldDef, expr_id: usize) -> Option<String> {
    let expr = field_expr(field, expr_id)?;
    match &expr.kind {
        AstExprKind::Pipe { input, op, .. } if op == "Text/is_empty" => {
            ast_argument_value(field, *input)
        }
        AstExprKind::Call { function, args, .. } if function == "Text/is_empty" => args
            .iter()
            .find(|arg| arg.is_bare_binding())
            .and_then(|arg| ast_argument_value(field, arg.value)),
        _ => None,
    }
}

fn match_value_arms_for_when(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    when_expr_id: usize,
    source: Option<&str>,
) -> Vec<UpdateValueMatchArm> {
    let mut arms = Vec::new();
    collect_match_value_arms_for_when(
        field,
        target,
        fields,
        &field.statement,
        when_expr_id,
        &mut arms,
        source,
    );
    if arms.is_empty() {
        match_value_arms_after_when_expr(field, target, fields, when_expr_id, source)
    } else {
        arms
    }
}

fn match_value_arms_need_structured_update(
    const_arms: &[UpdateMatchArm],
    value_arms: &[UpdateValueMatchArm],
) -> bool {
    if value_arms.len() != const_arms.len() {
        return !value_arms.is_empty();
    }
    value_arms
        .iter()
        .zip(const_arms)
        .any(|(value_arm, const_arm)| {
            value_arm.pattern != const_arm.pattern
                || !matches!(
                    &value_arm.output,
                    UpdateValueExpression::Const { value } if value == &const_arm.output
                )
        })
}

fn collect_match_value_arms_for_when(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    statement: &AstStatement,
    when_expr_id: usize,
    arms: &mut Vec<UpdateValueMatchArm>,
    source: Option<&str>,
) -> bool {
    let statement_contains_when = statement.expr.is_some_and(|expr_id| {
        expr_id == when_expr_id || expr_contains_expr_id(field, expr_id, when_expr_id)
    });
    if statement_contains_when {
        for child in &statement.children {
            if let Some(arm) = match_value_arm(field, target, fields, child, source) {
                arms.push(arm);
            }
        }
        if !arms.is_empty() {
            return true;
        }
    }
    for child in &statement.children {
        if collect_match_value_arms_for_when(
            field,
            target,
            fields,
            child,
            when_expr_id,
            arms,
            source,
        ) {
            return true;
        }
    }
    false
}

fn match_value_arms_after_when_expr(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    when_expr_id: usize,
    source: Option<&str>,
) -> Vec<UpdateValueMatchArm> {
    let Some(when_expr) = field_expr(field, when_expr_id) else {
        return Vec::new();
    };
    let end_line = field
        .ast_exprs
        .iter()
        .filter(|expr| {
            expr.line > when_expr.line
                && matches!(
                    expr.kind,
                    AstExprKind::When { .. } | AstExprKind::Then { .. }
                )
        })
        .map(|expr| expr.line)
        .min()
        .unwrap_or(usize::MAX);
    field
        .ast_exprs
        .iter()
        .filter(|expr| expr.line > when_expr.line && expr.line < end_line)
        .filter_map(|expr| match_value_arm_expr(field, target, fields, expr, source))
        .collect()
}

fn match_const_arms_for_when(field: &FieldDef, when_expr_id: usize) -> Vec<UpdateMatchArm> {
    let mut arms = Vec::new();
    collect_match_const_arms_for_when(field, &field.statement, when_expr_id, &mut arms);
    if arms.is_empty() {
        match_const_arms_after_when_expr(field, when_expr_id)
    } else {
        arms
    }
}

fn match_const_arms_for_statement_exprs(
    statement: &AstStatement,
    exprs: &[AstExpr],
    when_expr_id: usize,
) -> Vec<UpdateMatchArm> {
    let mut arms = Vec::new();
    collect_match_const_arms_for_statement_exprs(statement, exprs, when_expr_id, &mut arms);
    if arms.is_empty() {
        match_const_arms_after_when_expr_in_exprs(exprs, when_expr_id)
    } else {
        arms
    }
}

fn collect_match_const_arms_for_statement_exprs(
    statement: &AstStatement,
    exprs: &[AstExpr],
    when_expr_id: usize,
    arms: &mut Vec<UpdateMatchArm>,
) -> bool {
    let statement_contains_when = statement.expr.is_some_and(|expr_id| {
        expr_id == when_expr_id || expr_contains_expr_id_in_exprs(exprs, expr_id, when_expr_id)
    });
    if statement_contains_when {
        for child in &statement.children {
            if let Some(arm) = match_const_arm_in_exprs(exprs, child) {
                arms.push(arm);
            }
        }
        if !arms.is_empty() {
            return true;
        }
    }
    for child in &statement.children {
        if collect_match_const_arms_for_statement_exprs(child, exprs, when_expr_id, arms) {
            return true;
        }
    }
    false
}

fn match_const_arms_after_when_expr_in_exprs(
    exprs: &[AstExpr],
    when_expr_id: usize,
) -> Vec<UpdateMatchArm> {
    let Some(when_expr) = exprs.iter().find(|expr| expr.id == when_expr_id) else {
        return Vec::new();
    };
    let end_line = exprs
        .iter()
        .filter(|expr| {
            expr.line > when_expr.line
                && matches!(
                    expr.kind,
                    AstExprKind::When { .. } | AstExprKind::Then { .. }
                )
        })
        .map(|expr| expr.line)
        .min()
        .unwrap_or(usize::MAX);
    exprs
        .iter()
        .filter(|expr| expr.line > when_expr.line && expr.line < end_line)
        .filter_map(|expr| match_const_arm_expr_in_exprs(exprs, expr))
        .collect()
}

fn match_const_arms_after_when_expr(field: &FieldDef, when_expr_id: usize) -> Vec<UpdateMatchArm> {
    let Some(when_expr) = field_expr(field, when_expr_id) else {
        return Vec::new();
    };
    let end_line = field
        .ast_exprs
        .iter()
        .filter(|expr| {
            expr.line > when_expr.line
                && matches!(
                    expr.kind,
                    AstExprKind::When { .. } | AstExprKind::Then { .. }
                )
        })
        .map(|expr| expr.line)
        .min()
        .unwrap_or(usize::MAX);
    field
        .ast_exprs
        .iter()
        .filter(|expr| expr.line > when_expr.line && expr.line < end_line)
        .filter_map(|expr| match_const_arm_expr(field, expr))
        .collect()
}

fn collect_match_const_arms_for_when(
    field: &FieldDef,
    statement: &AstStatement,
    when_expr_id: usize,
    arms: &mut Vec<UpdateMatchArm>,
) -> bool {
    let statement_contains_when = statement.expr.is_some_and(|expr_id| {
        expr_id == when_expr_id || expr_contains_expr_id(field, expr_id, when_expr_id)
    });
    if statement_contains_when {
        for child in &statement.children {
            if let Some(arm) = match_const_arm(field, child) {
                arms.push(arm);
            }
        }
        if !arms.is_empty() {
            return true;
        }
    }
    for child in &statement.children {
        if collect_match_const_arms_for_when(field, child, when_expr_id, arms) {
            return true;
        }
    }
    false
}

fn match_const_arm(field: &FieldDef, statement: &AstStatement) -> Option<UpdateMatchArm> {
    let expr_id = statement.expr?;
    let expr = field_expr(field, expr_id)?;
    match_const_arm_expr(field, expr)
}

fn match_const_arm_in_exprs(exprs: &[AstExpr], statement: &AstStatement) -> Option<UpdateMatchArm> {
    let expr_id = statement.expr?;
    let expr = exprs.iter().find(|expr| expr.id == expr_id)?;
    match_const_arm_expr_in_exprs(exprs, expr)
}

fn match_const_arm_expr(field: &FieldDef, expr: &AstExpr) -> Option<UpdateMatchArm> {
    match_const_arm_expr_in_exprs(&field.ast_exprs, expr)
}

fn match_value_arm(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    statement: &AstStatement,
    source: Option<&str>,
) -> Option<UpdateValueMatchArm> {
    let expr_id = statement.expr?;
    let expr = field_expr(field, expr_id)?;
    match_value_arm_expr(field, target, fields, expr, source)
}

fn match_value_arm_expr(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    expr: &AstExpr,
    source: Option<&str>,
) -> Option<UpdateValueMatchArm> {
    let AstExprKind::MatchArm {
        pattern,
        output: Some(output),
    } = &expr.kind
    else {
        return None;
    };
    let output = update_value_expression_from_expr(field, target, fields, *output, source)?;
    let pattern = match_const_pattern_label(pattern)?;
    (!pattern.is_empty()).then_some(UpdateValueMatchArm { pattern, output })
}

fn update_value_expression_from_expr(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    expr_id: usize,
    source: Option<&str>,
) -> Option<UpdateValueExpression> {
    let expr = field_expr(field, expr_id)?;
    if matches!(expr.kind, AstExprKind::Identifier(_) | AstExprKind::Path(_)) {
        let raw = ast_argument_value(field, expr_id)?;
        let path = source.map_or_else(
            || canonical_scalar_update_path_with_fields(field, target, &raw, fields),
            |source| canonical_scalar_update_path_for_source(field, target, &raw, fields, source),
        );
        if path == target
            || fields
                .iter()
                .any(|candidate| symbol_is_rooted_in(&path, &candidate.path))
            || source.is_some_and(|source| source_payload_input_matches(&path, source))
        {
            return Some(UpdateValueExpression::ReadPath { path });
        }
    }
    if let Some(value) = ast_simple_update_value_in_exprs(&field.ast_exprs, expr_id) {
        return Some(UpdateValueExpression::Const { value });
    }
    if let AstExprKind::When { input, .. } = expr.kind {
        if let Some(expression) =
            update_value_match_infix_from_input(field, target, fields, input, expr.id, source)
        {
            return Some(expression);
        }
        if let Some(raw_input) = text_is_empty_input_path(field, input) {
            let input = source.map_or_else(
                || canonical_scalar_update_path_with_fields(field, target, &raw_input, fields),
                |source| {
                    canonical_scalar_update_path_for_source(
                        field, target, &raw_input, fields, source,
                    )
                },
            );
            let arms = match_value_arms_for_when(field, target, fields, expr.id, source);
            if !arms.is_empty() {
                return Some(UpdateValueExpression::MatchTextIsEmptyConst { input, arms });
            }
        }
        let raw_input = ast_argument_value(field, input)?;
        let input = source.map_or_else(
            || canonical_scalar_update_path_with_fields(field, target, &raw_input, fields),
            |source| {
                canonical_scalar_update_path_for_source(field, target, &raw_input, fields, source)
            },
        );
        let arms = match_value_arms_for_when(field, target, fields, expr.id, source);
        if !arms.is_empty() {
            return Some(UpdateValueExpression::MatchConst { input, arms });
        }
    }
    let AstExprKind::Infix { left, op, right } = &expr.kind else {
        return None;
    };
    let left = scalar_number_operand(field, *left, target)?;
    let right = scalar_number_operand(field, *right, target)?;
    Some(UpdateValueExpression::NumberInfix {
        left,
        op: op.clone(),
        right,
    })
}

fn update_value_match_infix_from_input(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    input: usize,
    when_expr_id: usize,
    source: Option<&str>,
) -> Option<UpdateValueExpression> {
    let input = field_expr(field, input)?;
    let AstExprKind::Infix { left, op, right } = &input.kind else {
        return None;
    };
    let left = scalar_number_operand(field, *left, target)?;
    let right = scalar_number_operand(field, *right, target)?;
    let arms = match_value_arms_for_when(field, target, fields, when_expr_id, source);
    (!arms.is_empty()).then_some(UpdateValueExpression::MatchInfixConst {
        left,
        op: op.clone(),
        right,
        arms,
    })
}

fn match_const_arm_expr_in_exprs(exprs: &[AstExpr], expr: &AstExpr) -> Option<UpdateMatchArm> {
    let AstExprKind::MatchArm {
        pattern,
        output: Some(output),
    } = &expr.kind
    else {
        return None;
    };
    let output = ast_simple_update_value_in_exprs(exprs, *output)?;
    let pattern = match_const_pattern_label(pattern)?;
    (!pattern.is_empty()).then_some(UpdateMatchArm { pattern, output })
}

fn match_const_pattern_label(pattern: &[String]) -> Option<String> {
    if pattern.is_empty() {
        None
    } else if let Some(value) = text_literal_value(pattern) {
        Some(value)
    } else if pattern.len() == 1 {
        Some(pattern[0].clone())
    } else {
        Some(pattern.join("."))
    }
}

fn expr_contains_expr_id_in_exprs(exprs: &[AstExpr], root: usize, needle: usize) -> bool {
    expr_contains_expr_id_in_exprs_seen(exprs, root, needle, &mut BTreeSet::new())
}

fn expr_contains_expr_id_in_exprs_seen(
    exprs: &[AstExpr],
    root: usize,
    needle: usize,
    seen: &mut BTreeSet<usize>,
) -> bool {
    if root == needle {
        return true;
    }
    if !seen.insert(root) {
        return false;
    }
    let Some(expr) = exprs.iter().find(|expr| expr.id == root) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::Call { args, .. } => args
            .iter()
            .any(|arg| expr_contains_expr_id_in_exprs_seen(exprs, arg.value, needle, seen)),
        AstExprKind::Pipe { input, args, .. } => {
            expr_contains_expr_id_in_exprs_seen(exprs, *input, needle, seen)
                || args
                    .iter()
                    .any(|arg| expr_contains_expr_id_in_exprs_seen(exprs, arg.value, needle, seen))
        }
        AstExprKind::Hold { initial, .. }
        | AstExprKind::When { input: initial, .. }
        | AstExprKind::Draining { input: initial } => {
            expr_contains_expr_id_in_exprs_seen(exprs, *initial, needle, seen)
        }
        AstExprKind::Then {
            input,
            output: Some(output),
        } => {
            expr_contains_expr_id_in_exprs_seen(exprs, *input, needle, seen)
                || expr_contains_expr_id_in_exprs_seen(exprs, *output, needle, seen)
        }
        AstExprKind::Then {
            input,
            output: None,
        } => expr_contains_expr_id_in_exprs_seen(exprs, *input, needle, seen),
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => expr_contains_expr_id_in_exprs_seen(exprs, *output, needle, seen),
        AstExprKind::Block { bindings, result } => {
            bindings.iter().any(|binding| {
                expr_contains_expr_id_in_exprs_seen(exprs, binding.value, needle, seen)
            }) || result.is_some_and(|result| {
                expr_contains_expr_id_in_exprs_seen(exprs, result, needle, seen)
            })
        }
        AstExprKind::Infix { left, right, .. } => {
            expr_contains_expr_id_in_exprs_seen(exprs, *left, needle, seen)
                || expr_contains_expr_id_in_exprs_seen(exprs, *right, needle, seen)
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => fields.iter().any(|record_field| {
            expr_contains_expr_id_in_exprs_seen(exprs, record_field.value, needle, seen)
        }),
        AstExprKind::ListLiteral { items, .. } | AstExprKind::BytesLiteral { items, .. } => items
            .iter()
            .any(|item| expr_contains_expr_id_in_exprs_seen(exprs, *item, needle, seen)),
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::Drain { .. }
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::MatchArm { output: None, .. }
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => false,
    }
}

fn expr_contains_expr_id(field: &FieldDef, root: usize, needle: usize) -> bool {
    expr_contains_expr_id_seen(field, root, needle, &mut BTreeSet::new())
}

fn expr_contains_expr_id_seen(
    field: &FieldDef,
    root: usize,
    needle: usize,
    seen: &mut BTreeSet<usize>,
) -> bool {
    if root == needle {
        return true;
    }
    if !seen.insert(root) {
        return false;
    }
    let Some(expr) = field_expr(field, root) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::Call { args, .. } => args
            .iter()
            .any(|arg| expr_contains_expr_id_seen(field, arg.value, needle, seen)),
        AstExprKind::Pipe { input, args, .. } => {
            expr_contains_expr_id_seen(field, *input, needle, seen)
                || args
                    .iter()
                    .any(|arg| expr_contains_expr_id_seen(field, arg.value, needle, seen))
        }
        AstExprKind::Hold { initial, .. }
        | AstExprKind::When { input: initial, .. }
        | AstExprKind::Draining { input: initial } => {
            expr_contains_expr_id_seen(field, *initial, needle, seen)
        }
        AstExprKind::Then {
            input,
            output: Some(output),
        } => {
            expr_contains_expr_id_seen(field, *input, needle, seen)
                || expr_contains_expr_id_seen(field, *output, needle, seen)
        }
        AstExprKind::Then {
            input,
            output: None,
        } => expr_contains_expr_id_seen(field, *input, needle, seen),
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => expr_contains_expr_id_seen(field, *output, needle, seen),
        AstExprKind::Block { bindings, result } => {
            bindings
                .iter()
                .any(|binding| expr_contains_expr_id_seen(field, binding.value, needle, seen))
                || result
                    .is_some_and(|result| expr_contains_expr_id_seen(field, result, needle, seen))
        }
        AstExprKind::Infix { left, right, .. } => {
            expr_contains_expr_id_seen(field, *left, needle, seen)
                || expr_contains_expr_id_seen(field, *right, needle, seen)
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => fields.iter().any(|record_field| {
            expr_contains_expr_id_seen(field, record_field.value, needle, seen)
        }),
        AstExprKind::ListLiteral { items, .. } | AstExprKind::BytesLiteral { items, .. } => items
            .iter()
            .any(|item| expr_contains_expr_id_seen(field, *item, needle, seen)),
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::Drain { .. }
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::MatchArm { output: None, .. }
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => false,
    }
}

fn canonical_scalar_update_path_with_fields(
    field: &FieldDef,
    target: &str,
    value: &str,
    fields: &[FieldDef],
) -> String {
    let target_field = target
        .rsplit_once('.')
        .map(|(_, field)| field)
        .unwrap_or(target);
    if value == field.local_name
        || value == target_field
        || field_hold_name(field).as_deref() == Some(value)
    {
        target.to_owned()
    } else if let Some(path) = canonical_current_row_member_path(field, value, fields) {
        path
    } else if !value.contains('.') {
        let child_path = format!("{}.{}", field.path, value);
        if fields.iter().any(|candidate| candidate.path == child_path) {
            child_path
        } else if !field.parent_path.is_empty() {
            let sibling_path = canonical_local_path(value, &field.parent_path);
            if fields
                .iter()
                .any(|candidate| candidate.path == sibling_path)
            {
                sibling_path
            } else if fields.iter().any(|candidate| candidate.path == value) {
                value.to_owned()
            } else {
                sibling_path
            }
        } else if fields.iter().any(|candidate| candidate.path == value) {
            value.to_owned()
        } else {
            canonical_local_path(value, &field.parent_path)
        }
    } else {
        canonical_local_path(value, &field.parent_path)
    }
}

fn canonical_current_row_member_path(
    field: &FieldDef,
    value: &str,
    fields: &[FieldDef],
) -> Option<String> {
    if fields.iter().any(|candidate| candidate.path == value) {
        return Some(value.to_owned());
    }
    let (_, tail) = value.split_once('.')?;
    let mut parent = field.parent_path.as_str();
    loop {
        let candidate = format!("{parent}.{tail}");
        if fields.iter().any(|field| field.path == candidate) {
            return Some(candidate);
        }
        let Some((ancestor, _)) = parent.rsplit_once('.') else {
            return None;
        };
        parent = ancestor;
    }
}

fn canonical_scalar_update_path_for_source(
    field: &FieldDef,
    target: &str,
    value: &str,
    fields: &[FieldDef],
    source: &str,
) -> String {
    if let Some(member_path) = canonical_source_member_path(source, value) {
        return member_path;
    }
    let canonical = canonical_scalar_update_path_with_fields(field, target, value, fields);
    if fields.iter().any(|candidate| candidate.path == canonical) {
        return canonical;
    }
    let Some((source_scope, _)) = source.split_once('.') else {
        return canonical;
    };
    let Some((_, value_tail)) = value.split_once('.') else {
        return canonical;
    };
    let source_scoped = format!("{source_scope}.{value_tail}");
    if fields
        .iter()
        .any(|candidate| candidate.path == source_scoped)
    {
        source_scoped
    } else {
        canonical
    }
}

fn canonical_source_member_path(source: &str, value: &str) -> Option<String> {
    let source_alias = std::iter::successors(Some(source), |candidate| {
        candidate.split_once('.').map(|(_, suffix)| suffix)
    })
    .find(|candidate| {
        value == *candidate
            || value
                .strip_prefix(*candidate)
                .is_some_and(|suffix| suffix.starts_with('.'))
    })?;
    let suffix = value.strip_prefix(source_alias)?;
    Some(format!("{source}{suffix}"))
}

fn canonical_bytes_scalar_arg(
    field: &FieldDef,
    target: &str,
    arg: BytesScalarArg,
    fields: &[FieldDef],
) -> BytesScalarArg {
    match arg {
        BytesScalarArg::Static(value) => BytesScalarArg::Static(value),
        BytesScalarArg::Path(path) => BytesScalarArg::Path(
            canonical_scalar_update_path_with_fields(field, target, &path, fields),
        ),
    }
}

fn field_hold_name(field: &FieldDef) -> Option<String> {
    match &field.statement.kind {
        AstStatementKind::Hold { name, .. } => name.clone(),
        _ => None,
    }
}

fn field_expr(field: &FieldDef, expr_id: usize) -> Option<&AstExpr> {
    field.ast_exprs.iter().find(|expr| expr.id == expr_id)
}

fn text_trim_or_previous_update(
    program: &ParsedProgram,
    target: &str,
    branch: &RoutedBranch,
) -> Option<UpdateExpression> {
    if !path_has_parsed_row_scope(program, target) || !branch.has_operator("Text/trim") {
        return None;
    }
    let mut previous = branch_value_after_match(branch, "TEXT")?;
    let mut path = branch.text_trim_input_path()?;
    if !value_starts_lowercase_identifier(&path) || !value_starts_lowercase_identifier(previous) {
        return None;
    }
    let target_field = target.rsplit_once('.').map(|(_, field)| field)?;
    if previous != target_field
        && !branch
            .items
            .iter()
            .any(|item| item.field.as_deref() == Some(previous))
    {
        previous = target_field;
    }
    if path.as_str() != "text"
        && !branch
            .items
            .iter()
            .any(|item| item.field.as_deref() == Some(path.as_str()))
        && branch.references_path_tail("text")
    {
        path = "text".to_owned();
    }
    Some(UpdateExpression::TextTrimOrPrevious {
        path,
        previous: previous.to_owned(),
    })
}

fn branch_value_after_match<'a>(branch: &'a RoutedBranch, label: &str) -> Option<&'a str> {
    branch.items.iter().find_map(|item| {
        let label_index = item.symbols.iter().position(|lexeme| lexeme == label)?;
        let arrow_index = item.symbols[label_index..]
            .iter()
            .position(|lexeme| lexeme == "=>")
            .map(|offset| label_index + offset)?;
        item.symbols[arrow_index + 1..]
            .iter()
            .find(|lexeme| is_name(lexeme))
            .map(String::as_str)
    })
}

fn value_starts_lowercase_identifier(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_lowercase() || ch == '_')
}

fn value_starts_uppercase_identifier(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn path_has_parsed_row_scope(program: &ParsedProgram, path: &str) -> bool {
    path.split('.').any(|segment| {
        program
            .row_scope_functions
            .iter()
            .any(|scope| scope.row_scope == segment)
    })
}

fn bool_not_path_in_exprs(exprs: &[AstExpr]) -> Option<String> {
    exprs
        .iter()
        .find_map(|expr| bool_not_path_from_expr(exprs, expr.id))
}

fn bool_not_path_from_expr(exprs: &[AstExpr], expr_id: usize) -> Option<String> {
    let expr = exprs.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::Pipe { input, op, .. } if op == "Bool/not" => {
            ast_argument_value_in_exprs(exprs, *input)
        }
        AstExprKind::Then {
            output: Some(output),
            ..
        } => bool_not_path_from_expr(exprs, *output),
        _ => None,
    }
}

struct CandidateSourceIndex<'a> {
    fields: &'a [FieldDef],
    direct_sources: &'a BTreeMap<String, Vec<String>>,
    effect_result_states: BTreeSet<String>,
    fields_by_path: BTreeMap<&'a str, usize>,
    dependencies_by_field: Vec<Vec<usize>>,
    cache: BTreeMap<(String, bool), Vec<String>>,
}

impl<'a> CandidateSourceIndex<'a> {
    fn new(
        fields: &'a [FieldDef],
        direct_sources: &'a BTreeMap<String, Vec<String>>,
        state_cells: &[StateCell],
    ) -> CandidateSourceIndex<'a> {
        let empty_exclusions = BTreeSet::new();
        let (_, dependencies_by_field) = field_symbol_dependency_graph(fields, &empty_exclusions);
        let fields_by_path = fields
            .iter()
            .enumerate()
            .map(|(index, field)| (field.path.as_str(), index))
            .collect();
        let effect_result_states = state_cells
            .iter()
            .filter_map(|state| {
                fields
                    .iter()
                    .find(|field| field.path == state.path)
                    .filter(|field| {
                        field.ast_exprs.iter().any(|expr| match &expr.kind {
                            AstExprKind::Call { function, .. } => {
                                boon_typecheck::is_typed_host_effect(function)
                            }
                            _ => false,
                        })
                    })
                    .map(|_| state.path.clone())
            })
            .collect();
        CandidateSourceIndex {
            fields,
            direct_sources,
            effect_result_states,
            fields_by_path,
            dependencies_by_field,
            cache: BTreeMap::new(),
        }
    }

    fn candidate_sources(&mut self, target: &str) -> Vec<String> {
        let cache_key = (target.to_owned(), false);
        if let Some(cached) = self.cache.get(&cache_key) {
            return cached.clone();
        }
        let Some(&field_index) = self.fields_by_path.get(target) else {
            self.cache.insert(cache_key, Vec::new());
            return Vec::new();
        };
        let mut visiting = Vec::new();
        self.candidate_sources_for_index(field_index, false, &mut visiting)
    }

    fn dependency_paths(&self, target: &str) -> Vec<String> {
        let Some(&field_index) = self.fields_by_path.get(target) else {
            return Vec::new();
        };
        self.dependencies_by_field[field_index]
            .iter()
            .map(|dependency| self.fields[*dependency].path.clone())
            .collect()
    }

    fn event_sources_for_dependency(&mut self, dependency: &str) -> Vec<String> {
        if self.effect_result_states.contains(dependency) {
            return vec![dependency.to_owned()];
        }
        self.candidate_sources(dependency)
    }

    fn is_effect_result_state(&self, path: &str) -> bool {
        self.effect_result_states.contains(path)
    }

    fn candidate_sources_for_index(
        &mut self,
        field_index: usize,
        as_dependency: bool,
        visiting: &mut Vec<usize>,
    ) -> Vec<String> {
        let path = self.fields[field_index].path.clone();
        if as_dependency && self.effect_result_states.contains(&path) {
            return vec![path];
        }
        if visiting.contains(&field_index) {
            return Vec::new();
        }
        let cache_key = (path.clone(), as_dependency);
        if let Some(cached) = self.cache.get(&cache_key) {
            return cached.clone();
        }
        visiting.push(field_index);
        let field = &self.fields[field_index];
        let mut candidates = direct_sources_for_field(self.direct_sources, field)
            .cloned()
            .collect::<Vec<_>>();
        for dependency_index in self.dependencies_by_field[field_index].clone() {
            if !field_dependency_is_event_cause(field, &self.fields[dependency_index]) {
                continue;
            }
            for source in self.candidate_sources_for_index(dependency_index, true, visiting) {
                push_unique(&mut candidates, source);
            }
        }
        visiting.pop();
        self.cache.insert(cache_key, candidates.clone());
        candidates
    }
}

fn field_dependency_is_event_cause(field: &FieldDef, dependency: &FieldDef) -> bool {
    let references = field
        .ast_exprs
        .iter()
        .filter(|expr| expression_references_field(field, expr, dependency))
        .map(|expr| expr.id)
        .filter(|reference| !reference_is_list_map_collection(field, *reference))
        .collect::<Vec<_>>();
    if references.is_empty() {
        return false;
    }
    let then_inputs = field
        .ast_exprs
        .iter()
        .filter_map(|expr| match expr.kind {
            AstExprKind::Then { input, .. } => Some(input),
            _ => None,
        })
        .collect::<Vec<_>>();
    if then_inputs.is_empty() {
        return true;
    }
    if references.iter().any(|reference| {
        then_inputs
            .iter()
            .any(|input| expr_contains_expr_id_in_exprs(&field.ast_exprs, *input, *reference))
    }) {
        return true;
    }
    let sampled_outputs = field
        .ast_exprs
        .iter()
        .filter_map(|expr| match expr.kind {
            AstExprKind::Then {
                output: Some(output),
                ..
            }
            | AstExprKind::MatchArm {
                output: Some(output),
                ..
            } => Some(output),
            _ => None,
        })
        .collect::<Vec<_>>();
    references.into_iter().any(|reference| {
        !sampled_outputs
            .iter()
            .any(|output| expr_contains_expr_id_in_exprs(&field.ast_exprs, *output, reference))
    })
}

fn reference_is_list_map_collection(field: &FieldDef, reference: usize) -> bool {
    field.ast_exprs.iter().any(|expr| match &expr.kind {
        AstExprKind::Pipe { input, op, .. } if op == "List/map" => {
            expr_contains_expr_id_in_exprs(&field.ast_exprs, *input, reference)
                || list_map_pipeline_prefix_contains_reference(field, expr.id, reference)
        }
        AstExprKind::Call { function, args, .. } if function == "List/map" => {
            args.first().is_some_and(|input| {
                expr_contains_expr_id_in_exprs(&field.ast_exprs, input.value, reference)
            })
        }
        _ => false,
    })
}

fn list_map_pipeline_prefix_contains_reference(
    field: &FieldDef,
    map_expr_id: usize,
    reference: usize,
) -> bool {
    let statements = std::slice::from_ref(&field.statement);
    let mut cursor = map_expr_id;
    let mut visited = BTreeSet::new();
    while visited.insert(cursor) {
        let Some(previous) = previous_pipeline_expression_id(statements, cursor, &field.ast_exprs)
        else {
            return false;
        };
        if expr_contains_expr_id_in_exprs(&field.ast_exprs, previous, reference) {
            return true;
        }
        cursor = previous;
    }
    false
}

fn previous_pipeline_expression_id(
    statements: &[AstStatement],
    marker_expr_id: usize,
    expressions: &[AstExpr],
) -> Option<usize> {
    for statement in statements {
        if let Some(expr_ids) = statement_pipeline_expression_ids(statement, expressions)
            && let Some(position) = expr_ids
                .iter()
                .position(|expr_id| *expr_id == marker_expr_id)
            && position > 0
        {
            return expr_ids.get(position - 1).copied();
        }
        if statement.expr == Some(marker_expr_id) {
            return None;
        }
        if let Some(found) =
            previous_pipeline_expression_id(&statement.children, marker_expr_id, expressions)
        {
            return Some(found);
        }
    }
    None
}

fn statement_pipeline_expression_ids(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Option<Vec<usize>> {
    let mut expr_ids = statement.expr.into_iter().collect::<Vec<_>>();
    collect_pipeline_continuation_expr_ids(statement, expressions, &mut expr_ids);
    (expr_ids.len() > 1
        && !expression_is_pipeline_continuation(expr_ids[0], expressions)
        && expr_ids
            .iter()
            .skip(1)
            .all(|expr_id| expression_is_pipeline_continuation(*expr_id, expressions)))
    .then_some(expr_ids)
}

fn collect_pipeline_continuation_expr_ids(
    statement: &AstStatement,
    expressions: &[AstExpr],
    expr_ids: &mut Vec<usize>,
) {
    for child in statement.children.iter().filter(|child| {
        matches!(child.kind, AstStatementKind::Expression)
            && child
                .expr
                .is_some_and(|expr_id| expression_is_pipeline_continuation(expr_id, expressions))
    }) {
        if let Some(expr_id) = child.expr {
            expr_ids.push(expr_id);
        }
        collect_pipeline_continuation_expr_ids(child, expressions, expr_ids);
    }
}

fn expression_is_pipeline_continuation(expr_id: usize, expressions: &[AstExpr]) -> bool {
    let input = match expressions
        .iter()
        .find(|expr| expr.id == expr_id)
        .map(|expr| &expr.kind)
    {
        Some(AstExprKind::Pipe { input, .. })
        | Some(AstExprKind::Then { input, .. })
        | Some(AstExprKind::When { input, .. })
        | Some(AstExprKind::Draining { input })
        | Some(AstExprKind::Hold { initial: input, .. }) => *input,
        _ => return false,
    };
    expression_chain_starts_with_pipeline_placeholder(input, expressions)
}

fn expression_chain_starts_with_pipeline_placeholder(
    expr_id: usize,
    expressions: &[AstExpr],
) -> bool {
    let Some(expr) = expressions.iter().find(|expr| expr.id == expr_id) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::Delimiter => true,
        AstExprKind::Unknown(tokens) => !tokens
            .iter()
            .any(|token| token.trim_start().starts_with('"')),
        AstExprKind::Pipe { input, .. }
        | AstExprKind::Then { input, .. }
        | AstExprKind::When { input, .. }
        | AstExprKind::Draining { input }
        | AstExprKind::Hold { initial: input, .. } => {
            expression_chain_starts_with_pipeline_placeholder(*input, expressions)
        }
        _ => false,
    }
}

fn expression_references_field(field: &FieldDef, expr: &AstExpr, dependency: &FieldDef) -> bool {
    let raw = match &expr.kind {
        AstExprKind::Identifier(value) => {
            if field.expression_has_match_binding(expr.id, value) {
                return false;
            }
            value.as_str()
        }
        AstExprKind::Path(parts) => {
            return expression_path_references_field(field, expr.id, parts, dependency);
        }
        _ => return false,
    };
    raw == dependency.path
        || raw == dependency.local_name
        || canonical_local_path(raw, &field.parent_path) == dependency.path
}

fn expression_path_references_field(
    field: &FieldDef,
    expr_id: usize,
    parts: &[String],
    dependency: &FieldDef,
) -> bool {
    if parts.is_empty() {
        return false;
    }
    if field.expression_has_match_binding(expr_id, &parts[0]) {
        return false;
    }
    let raw = parts.join(".");
    path_is_or_is_within(&raw, &dependency.path)
        || (parts.len() == 1 && parts[0] == dependency.local_name)
        || scoped_field_reference_candidates(&field.parent_path, &raw)
            .iter()
            .any(|candidate| path_is_or_is_within(candidate, &dependency.path))
}

fn path_is_or_is_within(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

#[derive(Clone, Debug)]
struct FieldDef {
    path: String,
    local_name: String,
    parent_path: String,
    statement: AstStatement,
    ast_items: Vec<AstItem>,
    ast_exprs: Vec<AstExpr>,
}

#[derive(Clone, Debug, Default)]
struct RoutedBranch {
    items: Vec<AstItem>,
    ast_exprs: Vec<AstExpr>,
}

impl RoutedBranch {
    fn ast_exprs(&self) -> &[AstExpr] {
        &self.ast_exprs
    }

    fn summary(&self) -> String {
        self.items
            .iter()
            .map(item_summary)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn has_token(&self, token: &str) -> bool {
        self.items.iter().any(|item| item_has_symbol(item, token))
    }

    fn has_operator(&self, operator: &str) -> bool {
        self.ast_exprs.iter().any(|expr| match &expr.kind {
            AstExprKind::Pipe { op, .. } => op == operator,
            AstExprKind::Call { function, .. } => function == operator,
            _ => false,
        })
    }

    fn has_bool_expr(&self, value: bool) -> bool {
        self.ast_exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                AstExprKind::Bool(candidate) if candidate == value
            )
        })
    }

    fn references_path_tail(&self, path_tail: &str) -> bool {
        self.ast_exprs.iter().any(|expr| match &expr.kind {
            AstExprKind::Path(parts) => parts.last().map(String::as_str) == Some(path_tail),
            _ => false,
        })
    }

    fn then_simple_update_value(&self) -> Option<SimpleThenUpdateValue> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then { output, .. } = expr.kind else {
                return None;
            };
            if let Some(output) = output {
                return ast_simple_then_update_value_in_exprs(&self.ast_exprs, output);
            }
            self.ast_exprs
                .iter()
                .filter(|candidate| candidate.line > expr.line)
                .find_map(|candidate| {
                    ast_simple_then_update_value_in_exprs(&self.ast_exprs, candidate.id)
                })
        })
    }

    fn simple_update_value(&self) -> Option<SimpleThenUpdateValue> {
        if self.ast_exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                AstExprKind::Then { .. } | AstExprKind::When { .. }
            )
        }) {
            return None;
        }
        self.ast_exprs
            .iter()
            .find_map(|expr| ast_simple_then_update_value_in_exprs(&self.ast_exprs, expr.id))
    }

    fn then_number_infix_expression(
        &self,
        field: &FieldDef,
        target: &str,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then { output, .. } = expr.kind else {
                return None;
            };
            let output = output.or_else(|| following_direct_then_call_expr_id(field, expr.line))?;
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let AstExprKind::Infix { left, op, right } = &output.kind else {
                return None;
            };
            if op != "+" && op != "-" {
                return None;
            }
            let left = scalar_number_operand(field, *left, target)?;
            let right = scalar_number_operand(field, *right, target)?;
            Some(UpdateExpression::NumberInfix {
                left,
                op: op.clone(),
                right,
            })
        })
    }

    fn then_project_time_expression(
        &self,
        field: &FieldDef,
        target: &str,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let AstExprKind::Call { function, args, .. } = &output.kind else {
                return None;
            };
            if function != "Number/project_time" {
                return None;
            }
            let arg = |name: &str| {
                args.iter()
                    .find(|arg| arg.named_name() == Some(name))
                    .and_then(|arg| scalar_number_operand(field, arg.value, target))
            };
            Some(UpdateExpression::ProjectTime {
                pointer_x: arg("pointer_x")?,
                pointer_width: arg("pointer_width")?,
                viewport_start: arg("viewport_start")?,
                viewport_end: arg("viewport_end")?,
                fallback: arg("fallback")?,
            })
        })
    }

    fn then_bytes_length_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let raw_path = match &output.kind {
                AstExprKind::Pipe { input, op, .. } if op == "Bytes/length" => {
                    ast_argument_value_in_exprs(&self.ast_exprs, *input)?
                }
                AstExprKind::Call { function, args, .. } if function == "Bytes/length" => {
                    ast_argument_value_in_exprs(&self.ast_exprs, args.first()?.value)?
                }
                _ => return None,
            };
            Some(UpdateExpression::BytesLength {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
            })
        })
    }

    fn then_bytes_is_empty_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let raw_path = match &output.kind {
                AstExprKind::Pipe { input, op, .. } if op == "Bytes/is_empty" => {
                    ast_argument_value_in_exprs(&self.ast_exprs, *input)?
                }
                AstExprKind::Call { function, args, .. } if function == "Bytes/is_empty" => {
                    ast_argument_value_in_exprs(&self.ast_exprs, args.first()?.value)?
                }
                _ => return None,
            };
            Some(UpdateExpression::BytesIsEmpty {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
            })
        })
    }

    fn then_bytes_get_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
        resolved_constants: &ResolvedConstantLookup<'_>,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = field
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_path, index) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/get" => (
                    ast_argument_value_in_exprs(&field.ast_exprs, *input)?,
                    bytes_get_index_arg_in_exprs(resolved_constants, args, true)?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/get" => (
                    bytes_get_input_arg_in_exprs(&field.ast_exprs, args)?,
                    bytes_get_index_arg_in_exprs(resolved_constants, args, false)?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesGet {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
                index,
            })
        })
    }

    fn then_list_get_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
        branch_source: &str,
        resolved_constants: &ResolvedConstantLookup<'_>,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = field
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_path, index) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "List/get" => (
                    ast_argument_value_in_exprs(&field.ast_exprs, *input)?,
                    bytes_get_index_arg_in_exprs(resolved_constants, args, true)?,
                ),
                AstExprKind::Call { function, args, .. } if function == "List/get" => (
                    bytes_get_input_arg_in_exprs(&field.ast_exprs, args)?,
                    bytes_get_index_arg_in_exprs(resolved_constants, args, false)?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::ListGet {
                path: canonical_scalar_update_path_for_source(
                    field,
                    target,
                    &raw_path,
                    fields,
                    branch_source,
                ),
                index,
            })
        })
    }

    fn then_bytes_set_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
        resolved_constants: &ResolvedConstantLookup<'_>,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = field
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_path, index, value) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/set" => (
                    ast_argument_value_in_exprs(&field.ast_exprs, *input)?,
                    bytes_set_index_arg_in_exprs(resolved_constants, args, true)?,
                    bytes_set_value_arg_in_exprs(&field.ast_exprs, args, true)?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/set" => (
                    bytes_set_input_arg_in_exprs(&field.ast_exprs, args)?,
                    bytes_set_index_arg_in_exprs(resolved_constants, args, false)?,
                    bytes_set_value_arg_in_exprs(&field.ast_exprs, args, false)?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesSet {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
                index,
                value,
            })
        })
    }

    fn then_bytes_slice_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
        resolved_constants: &ResolvedConstantLookup<'_>,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = field
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_path, offset, byte_count) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/slice" => (
                    ast_argument_value_in_exprs(&field.ast_exprs, *input)?,
                    bytes_slice_offset_arg_in_exprs(resolved_constants, args, true)?,
                    bytes_slice_byte_count_arg_in_exprs(
                        &field.ast_exprs,
                        resolved_constants,
                        args,
                        true,
                    )?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/slice" => (
                    bytes_slice_input_arg_in_exprs(&field.ast_exprs, args)?,
                    bytes_slice_offset_arg_in_exprs(resolved_constants, args, false)?,
                    bytes_slice_byte_count_arg_in_exprs(
                        &field.ast_exprs,
                        resolved_constants,
                        args,
                        false,
                    )?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesSlice {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
                offset: BytesScalarArg::Static(offset),
                byte_count: canonical_bytes_scalar_arg(field, target, byte_count, fields),
            })
        })
    }

    fn then_bytes_take_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
        resolved_constants: &ResolvedConstantLookup<'_>,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = field
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_path, byte_count) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/take" => (
                    ast_argument_value_in_exprs(&field.ast_exprs, *input)?,
                    bytes_count_arg_in_exprs(&field.ast_exprs, resolved_constants, args, true)?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/take" => (
                    bytes_slice_input_arg_in_exprs(&field.ast_exprs, args)?,
                    bytes_count_arg_in_exprs(&field.ast_exprs, resolved_constants, args, false)?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesTake {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
                byte_count: canonical_bytes_scalar_arg(field, target, byte_count, fields),
            })
        })
    }

    fn then_bytes_drop_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
        resolved_constants: &ResolvedConstantLookup<'_>,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = field
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_path, byte_count) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/drop" => (
                    ast_argument_value_in_exprs(&field.ast_exprs, *input)?,
                    bytes_count_arg_in_exprs(&field.ast_exprs, resolved_constants, args, true)?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/drop" => (
                    bytes_slice_input_arg_in_exprs(&field.ast_exprs, args)?,
                    bytes_count_arg_in_exprs(&field.ast_exprs, resolved_constants, args, false)?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesDrop {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
                byte_count: canonical_bytes_scalar_arg(field, target, byte_count, fields),
            })
        })
    }

    fn then_bytes_zeros_expression(
        &self,
        field: &FieldDef,
        resolved_constants: &ResolvedConstantLookup<'_>,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = field
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let byte_count = match &output.kind {
                AstExprKind::Call { function, args, .. } if function == "Bytes/zeros" => {
                    bytes_zeros_byte_count_arg_in_exprs(resolved_constants, args)?
                }
                _ => return None,
            };
            Some(UpdateExpression::BytesZeros { byte_count })
        })
    }

    fn then_bytes_to_hex_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let raw_path = match &output.kind {
                AstExprKind::Pipe { input, op, .. } if op == "Bytes/to_hex" => {
                    ast_argument_value_in_exprs(&self.ast_exprs, *input)?
                }
                AstExprKind::Call { function, args, .. } if function == "Bytes/to_hex" => {
                    bytes_input_arg_in_exprs(&self.ast_exprs, args)?
                }
                _ => return None,
            };
            Some(UpdateExpression::BytesToHex {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
            })
        })
    }

    fn then_bytes_from_hex_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let raw_path = match &output.kind {
                AstExprKind::Pipe { input, op, .. } if op == "Bytes/from_hex" => {
                    ast_argument_value_in_exprs(&self.ast_exprs, *input)?
                }
                AstExprKind::Call { function, args, .. } if function == "Bytes/from_hex" => {
                    bytes_text_input_arg_in_exprs(&self.ast_exprs, args)?
                }
                _ => return None,
            };
            Some(UpdateExpression::BytesFromHex {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
            })
        })
    }

    fn then_bytes_to_base64_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let raw_path = match &output.kind {
                AstExprKind::Pipe { input, op, .. } if op == "Bytes/to_base64" => {
                    ast_argument_value_in_exprs(&self.ast_exprs, *input)?
                }
                AstExprKind::Call { function, args, .. } if function == "Bytes/to_base64" => {
                    bytes_input_arg_in_exprs(&self.ast_exprs, args)?
                }
                _ => return None,
            };
            Some(UpdateExpression::BytesToBase64 {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
            })
        })
    }

    fn then_bytes_from_base64_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let raw_path = match &output.kind {
                AstExprKind::Pipe { input, op, .. } if op == "Bytes/from_base64" => {
                    ast_argument_value_in_exprs(&self.ast_exprs, *input)?
                }
                AstExprKind::Call { function, args, .. } if function == "Bytes/from_base64" => {
                    bytes_text_input_arg_in_exprs(&self.ast_exprs, args)?
                }
                _ => return None,
            };
            Some(UpdateExpression::BytesFromBase64 {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
            })
        })
    }

    fn then_bytes_read_unsigned_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
        resolved_constants: &ResolvedConstantLookup<'_>,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = field
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_path, offset, byte_count, endian) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/read_unsigned" => (
                    ast_argument_value_in_exprs(&field.ast_exprs, *input)?,
                    bytes_numeric_offset_arg_in_exprs(resolved_constants, args, true)?,
                    bytes_numeric_byte_count_arg_in_exprs(resolved_constants, args, true)?,
                    bytes_numeric_endian_arg_in_exprs(resolved_constants, args, true)?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/read_unsigned" => (
                    bytes_input_arg_in_exprs(&field.ast_exprs, args)?,
                    bytes_numeric_offset_arg_in_exprs(resolved_constants, args, false)?,
                    bytes_numeric_byte_count_arg_in_exprs(resolved_constants, args, false)?,
                    bytes_numeric_endian_arg_in_exprs(resolved_constants, args, false)?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesReadUnsigned {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
                offset,
                byte_count,
                endian,
            })
        })
    }

    fn then_bytes_read_signed_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
        resolved_constants: &ResolvedConstantLookup<'_>,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = field
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_path, offset, byte_count, endian) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/read_signed" => (
                    ast_argument_value_in_exprs(&field.ast_exprs, *input)?,
                    bytes_numeric_offset_arg_in_exprs(resolved_constants, args, true)?,
                    bytes_numeric_byte_count_arg_in_exprs(resolved_constants, args, true)?,
                    bytes_numeric_endian_arg_in_exprs(resolved_constants, args, true)?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/read_signed" => (
                    bytes_input_arg_in_exprs(&field.ast_exprs, args)?,
                    bytes_numeric_offset_arg_in_exprs(resolved_constants, args, false)?,
                    bytes_numeric_byte_count_arg_in_exprs(resolved_constants, args, false)?,
                    bytes_numeric_endian_arg_in_exprs(resolved_constants, args, false)?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesReadSigned {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
                offset,
                byte_count,
                endian,
            })
        })
    }

    fn then_bytes_write_unsigned_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
        resolved_constants: &ResolvedConstantLookup<'_>,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = field
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_path, offset, byte_count, endian, value) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/write_unsigned" => (
                    ast_argument_value_in_exprs(&field.ast_exprs, *input)?,
                    bytes_numeric_offset_arg_in_exprs(resolved_constants, args, true)?,
                    bytes_numeric_byte_count_arg_in_exprs(resolved_constants, args, true)?,
                    bytes_numeric_endian_arg_in_exprs(resolved_constants, args, true)?,
                    bytes_numeric_value_arg_in_exprs(resolved_constants, args, true)?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/write_unsigned" => (
                    bytes_input_arg_in_exprs(&field.ast_exprs, args)?,
                    bytes_numeric_offset_arg_in_exprs(resolved_constants, args, false)?,
                    bytes_numeric_byte_count_arg_in_exprs(resolved_constants, args, false)?,
                    bytes_numeric_endian_arg_in_exprs(resolved_constants, args, false)?,
                    bytes_numeric_value_arg_in_exprs(resolved_constants, args, false)?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesWriteUnsigned {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
                offset,
                byte_count,
                endian,
                value,
            })
        })
    }

    fn then_bytes_write_signed_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
        resolved_constants: &ResolvedConstantLookup<'_>,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = field
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_path, offset, byte_count, endian, value) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/write_signed" => (
                    ast_argument_value_in_exprs(&field.ast_exprs, *input)?,
                    bytes_numeric_offset_arg_in_exprs(resolved_constants, args, true)?,
                    bytes_numeric_byte_count_arg_in_exprs(resolved_constants, args, true)?,
                    bytes_numeric_endian_arg_in_exprs(resolved_constants, args, true)?,
                    bytes_numeric_value_arg_in_exprs(resolved_constants, args, true)?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/write_signed" => (
                    bytes_input_arg_in_exprs(&field.ast_exprs, args)?,
                    bytes_numeric_offset_arg_in_exprs(resolved_constants, args, false)?,
                    bytes_numeric_byte_count_arg_in_exprs(resolved_constants, args, false)?,
                    bytes_numeric_endian_arg_in_exprs(resolved_constants, args, false)?,
                    bytes_numeric_value_arg_in_exprs(resolved_constants, args, false)?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesWriteSigned {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
                offset,
                byte_count,
                endian,
                value,
            })
        })
    }

    fn host_effect_expression(&self, field: &FieldDef) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|output| {
            let AstExprKind::Call { function, args, .. } = &output.kind else {
                return None;
            };
            if !boon_typecheck::is_typed_host_effect(function) {
                return None;
            }
            let arguments = if args.is_empty() {
                statement_containing_expr_graph(&field.statement, output.id, &field.ast_exprs)
                    .map(|statement| {
                        statement
                            .children
                            .iter()
                            .filter_map(|argument| {
                                let name = match &argument.kind {
                                    AstStatementKind::Field { name }
                                    | AstStatementKind::List {
                                        field: Some(name), ..
                                    } => name,
                                    _ => return None,
                                };
                                Some(HostEffectCallArgument {
                                    name: name.clone(),
                                    value_expr_id: ExprId(argument.expr?),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                args.iter()
                    .map(|argument| {
                        Some(HostEffectCallArgument {
                            name: argument.named_name()?.to_owned(),
                            value_expr_id: ExprId(argument.value),
                        })
                    })
                    .collect::<Option<Vec<_>>>()?
            };
            Some(UpdateExpression::HostEffect {
                operation: function.clone(),
                call_expr_id: ExprId(output.id),
                arguments,
            })
        })
    }

    fn then_text_to_number_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let raw_path = match &output.kind {
                AstExprKind::Pipe { input, op, .. } if op == "Text/to_number" => {
                    ast_argument_value_in_exprs(&self.ast_exprs, *input)?
                }
                AstExprKind::Call { function, args, .. } if function == "Text/to_number" => {
                    text_to_bytes_input_arg_in_exprs(&self.ast_exprs, args)?
                }
                _ => return None,
            };
            Some(UpdateExpression::TextToNumber {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
            })
        })
    }

    fn then_text_to_bytes_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
        resolved_constants: &ResolvedConstantLookup<'_>,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_path, encoding) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Text/to_bytes" => (
                    ast_argument_value_in_exprs(&self.ast_exprs, *input)?,
                    bytes_encoding_arg_in_exprs(resolved_constants, args, true)?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Text/to_bytes" => (
                    text_to_bytes_input_arg_in_exprs(&self.ast_exprs, args)?,
                    bytes_encoding_arg_in_exprs(resolved_constants, args, false)?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::TextToBytes {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
                encoding,
            })
        })
    }

    fn then_bytes_to_text_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
        resolved_constants: &ResolvedConstantLookup<'_>,
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_path, encoding) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/to_text" => (
                    ast_argument_value_in_exprs(&self.ast_exprs, *input)?,
                    bytes_encoding_arg_in_exprs(resolved_constants, args, true)?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/to_text" => (
                    bytes_to_text_input_arg_in_exprs(&self.ast_exprs, args)?,
                    bytes_encoding_arg_in_exprs(resolved_constants, args, false)?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesToText {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
                encoding,
            })
        })
    }

    fn then_bytes_concat_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_left, raw_right) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/concat" => (
                    ast_argument_value_in_exprs(&self.ast_exprs, *input)?,
                    bytes_concat_right_arg_in_exprs(&self.ast_exprs, args, true)?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/concat" => (
                    bytes_concat_left_arg_in_exprs(&self.ast_exprs, args)?,
                    bytes_concat_right_arg_in_exprs(&self.ast_exprs, args, false)?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesConcat {
                left: canonical_scalar_update_path_with_fields(field, target, &raw_left, fields),
                right: canonical_scalar_update_path_with_fields(field, target, &raw_right, fields),
            })
        })
    }

    fn then_bytes_equal_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_left, raw_right) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/equal" => (
                    ast_argument_value_in_exprs(&self.ast_exprs, *input)?,
                    bytes_equal_right_arg_in_exprs(&self.ast_exprs, args, true)?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/equal" => (
                    bytes_equal_left_arg_in_exprs(&self.ast_exprs, args)?,
                    bytes_equal_right_arg_in_exprs(&self.ast_exprs, args, false)?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesEqual {
                left: canonical_scalar_update_path_with_fields(field, target, &raw_left, fields),
                right: canonical_scalar_update_path_with_fields(field, target, &raw_right, fields),
            })
        })
    }

    fn then_bytes_find_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_haystack, raw_needle) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/find" => (
                    ast_argument_value_in_exprs(&self.ast_exprs, *input)?,
                    bytes_search_second_arg_in_exprs(
                        &self.ast_exprs,
                        args,
                        true,
                        &["needle", "with"],
                    )?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/find" => (
                    bytes_search_haystack_arg_in_exprs(&self.ast_exprs, args)?,
                    bytes_search_second_arg_in_exprs(
                        &self.ast_exprs,
                        args,
                        false,
                        &["needle", "with"],
                    )?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesFind {
                haystack: canonical_scalar_update_path_with_fields(
                    field,
                    target,
                    &raw_haystack,
                    fields,
                ),
                needle: canonical_scalar_update_path_with_fields(
                    field,
                    target,
                    &raw_needle,
                    fields,
                ),
            })
        })
    }

    fn then_bytes_starts_with_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_path, raw_prefix) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/starts_with" => (
                    ast_argument_value_in_exprs(&self.ast_exprs, *input)?,
                    bytes_search_second_arg_in_exprs(
                        &self.ast_exprs,
                        args,
                        true,
                        &["prefix", "with"],
                    )?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/starts_with" => (
                    bytes_search_haystack_arg_in_exprs(&self.ast_exprs, args)?,
                    bytes_search_second_arg_in_exprs(
                        &self.ast_exprs,
                        args,
                        false,
                        &["prefix", "with"],
                    )?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesStartsWith {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
                prefix: canonical_scalar_update_path_with_fields(
                    field,
                    target,
                    &raw_prefix,
                    fields,
                ),
            })
        })
    }

    fn then_bytes_ends_with_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return None;
            };
            let output = self
                .ast_exprs
                .iter()
                .find(|candidate| candidate.id == output)?;
            let (raw_path, raw_suffix) = match &output.kind {
                AstExprKind::Pipe {
                    input, op, args, ..
                } if op == "Bytes/ends_with" => (
                    ast_argument_value_in_exprs(&self.ast_exprs, *input)?,
                    bytes_search_second_arg_in_exprs(
                        &self.ast_exprs,
                        args,
                        true,
                        &["suffix", "with"],
                    )?,
                ),
                AstExprKind::Call { function, args, .. } if function == "Bytes/ends_with" => (
                    bytes_search_haystack_arg_in_exprs(&self.ast_exprs, args)?,
                    bytes_search_second_arg_in_exprs(
                        &self.ast_exprs,
                        args,
                        false,
                        &["suffix", "with"],
                    )?,
                ),
                _ => return None,
            };
            Some(UpdateExpression::BytesEndsWith {
                path: canonical_scalar_update_path_with_fields(field, target, &raw_path, fields),
                suffix: canonical_scalar_update_path_with_fields(
                    field,
                    target,
                    &raw_suffix,
                    fields,
                ),
            })
        })
    }

    fn text_trim_input_path(&self) -> Option<String> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Pipe { input, op, .. } = &expr.kind else {
                return None;
            };
            (op == "Text/trim").then(|| ast_argument_value_in_exprs(&self.ast_exprs, *input))?
        })
    }

    fn bool_not_path(&self) -> Option<String> {
        bool_not_path_in_exprs(&self.ast_exprs)
    }

    fn then_prefix_payload_concat_expression(
        &self,
        source_variants: &[String],
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then { output, .. } = expr.kind else {
                return None;
            };
            output
                .and_then(|output| {
                    prefix_payload_concat_update_expression(
                        &self.ast_exprs,
                        output,
                        source_variants,
                    )
                    .or_else(|| {
                        prefix_payload_concat_update_expression_using_input(
                            &self.ast_exprs,
                            output,
                            source_variants,
                        )
                    })
                })
                .or_else(|| {
                    prefix_payload_concat_update_expression_after_line(
                        &self.ast_exprs,
                        expr.line,
                        source_variants,
                    )
                })
                .or_else(|| {
                    prefix_payload_concat_update_expression_from_items(&self.items, source_variants)
                })
        })
    }

    fn then_prefix_root_concat_expression(
        &self,
        field: &FieldDef,
        target: &str,
        fields: &[FieldDef],
    ) -> Option<UpdateExpression> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then { output, .. } = expr.kind else {
                return None;
            };
            output
                .and_then(|output| {
                    prefix_root_concat_update_expression(
                        &self.ast_exprs,
                        output,
                        field,
                        target,
                        fields,
                    )
                    .or_else(|| {
                        prefix_root_concat_update_expression_using_input(
                            &self.ast_exprs,
                            output,
                            field,
                            target,
                            fields,
                        )
                    })
                })
                .or_else(|| {
                    prefix_root_concat_update_expression_after_line(
                        &self.ast_exprs,
                        expr.line,
                        field,
                        target,
                        fields,
                    )
                })
        })
    }

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

fn prefix_root_concat_update_expression_using_input(
    exprs: &[AstExpr],
    input: usize,
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
) -> Option<UpdateExpression> {
    exprs.iter().find_map(|expr| {
        let AstExprKind::Pipe {
            input: pipe_input, ..
        } = &expr.kind
        else {
            return None;
        };
        (*pipe_input == input)
            .then(|| prefix_root_concat_update_expression(exprs, expr.id, field, target, fields))
            .flatten()
    })
}

fn prefix_root_concat_update_expression_after_line(
    exprs: &[AstExpr],
    line: usize,
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
) -> Option<UpdateExpression> {
    let end_line = exprs
        .iter()
        .filter(|expr| {
            expr.line > line
                && matches!(
                    expr.kind,
                    AstExprKind::When { .. } | AstExprKind::Then { .. }
                )
        })
        .map(|expr| expr.line)
        .min()
        .unwrap_or(usize::MAX);
    exprs
        .iter()
        .filter(|expr| expr.line > line && expr.line < end_line)
        .find_map(|expr| {
            prefix_root_concat_update_expression(exprs, expr.id, field, target, fields)
        })
}

fn prefix_root_concat_update_expression(
    exprs: &[AstExpr],
    output: usize,
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
) -> Option<UpdateExpression> {
    let expr = exprs.iter().find(|expr| expr.id == output)?;
    let AstExprKind::Pipe {
        op, input, args, ..
    } = &expr.kind
    else {
        return None;
    };
    if op != "Text/concat" {
        return None;
    }
    let SimpleThenUpdateValue::Const(prefix) =
        ast_simple_then_update_value_in_exprs(exprs, *input)?
    else {
        return None;
    };
    let raw_path = args
        .iter()
        .find(|arg| arg.named_name() == Some("with"))
        .or_else(|| args.iter().find(|arg| arg.is_bare_binding()))
        .and_then(|arg| ast_argument_value_in_exprs(exprs, arg.value))?;
    let path = canonical_scalar_update_path_with_fields(field, target, &raw_path, fields);
    let separator = args
        .iter()
        .find(|arg| arg.named_name() == Some("separator"))
        .and_then(|arg| ast_simple_then_update_value_in_exprs(exprs, arg.value))
        .and_then(|value| match value {
            SimpleThenUpdateValue::Const(value) => Some(value),
            SimpleThenUpdateValue::Path(_) => None,
        })
        .unwrap_or_default();
    Some(UpdateExpression::PrefixRootConcat {
        prefix,
        path,
        separator,
    })
}

fn prefix_payload_concat_update_expression_using_input(
    exprs: &[AstExpr],
    input: usize,
    source_variants: &[String],
) -> Option<UpdateExpression> {
    exprs.iter().find_map(|expr| {
        let AstExprKind::Pipe {
            input: pipe_input, ..
        } = &expr.kind
        else {
            return None;
        };
        (*pipe_input == input)
            .then(|| prefix_payload_concat_update_expression(exprs, expr.id, source_variants))
            .flatten()
    })
}

fn prefix_payload_concat_update_expression_from_items(
    items: &[AstItem],
    source_variants: &[String],
) -> Option<UpdateExpression> {
    items.iter().find_map(|item| {
        let summary = item_summary(item);
        let concat_marker = " |> Text/concat";
        let prefix_start = summary
            .find("|> THEN TEXT ")
            .map(|start| start + "|> THEN TEXT ".len())
            .or_else(|| summary.strip_prefix("TEXT ").map(|_| "TEXT ".len()))?;
        let prefix_end = summary[prefix_start..].find(concat_marker)? + prefix_start;
        let prefix = summary[prefix_start..prefix_end].trim().to_owned();
        if prefix.is_empty() {
            return None;
        }
        let with_marker = "with:";
        let with_start = summary.find(with_marker)? + with_marker.len();
        let payload_tail = &summary[with_start..];
        let payload_path = payload_tail
            .split([',', ')'])
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        source_payload_field_from_path(&payload_path, source_variants)?;
        let separator = summary
            .split_once("separator:")
            .map(|(_, tail)| tail.trim())
            .and_then(|tail| {
                tail.strip_prefix('"')
                    .and_then(|quoted| quoted.find('"').map(|end| quoted[..end].to_owned()))
            })
            .unwrap_or_default();
        Some(UpdateExpression::PrefixPayloadConcat {
            prefix,
            payload_path,
            separator,
        })
    })
}

fn prefix_payload_concat_update_expression_after_line(
    exprs: &[AstExpr],
    line: usize,
    source_variants: &[String],
) -> Option<UpdateExpression> {
    let end_line = exprs
        .iter()
        .filter(|expr| {
            expr.line > line
                && matches!(
                    expr.kind,
                    AstExprKind::When { .. } | AstExprKind::Then { .. }
                )
        })
        .map(|expr| expr.line)
        .min()
        .unwrap_or(usize::MAX);
    exprs
        .iter()
        .filter(|expr| expr.line > line && expr.line < end_line)
        .find_map(|expr| prefix_payload_concat_update_expression(exprs, expr.id, source_variants))
}

fn prefix_payload_concat_update_expression(
    exprs: &[AstExpr],
    output: usize,
    source_variants: &[String],
) -> Option<UpdateExpression> {
    let expr = exprs.iter().find(|expr| expr.id == output)?;
    let AstExprKind::Pipe {
        op, input, args, ..
    } = &expr.kind
    else {
        return None;
    };
    if op != "Text/concat" {
        return None;
    }
    let SimpleThenUpdateValue::Const(prefix) =
        ast_simple_then_update_value_in_exprs(exprs, *input)?
    else {
        return None;
    };
    let payload_path = args
        .iter()
        .find(|arg| arg.named_name() == Some("with"))
        .or_else(|| args.iter().find(|arg| arg.is_bare_binding()))
        .and_then(|arg| ast_argument_value_in_exprs(exprs, arg.value))?;
    source_payload_field_from_path(&payload_path, source_variants)?;
    let separator = args
        .iter()
        .find(|arg| arg.named_name() == Some("separator"))
        .and_then(|arg| ast_simple_then_update_value_in_exprs(exprs, arg.value))
        .and_then(|value| match value {
            SimpleThenUpdateValue::Const(value) => Some(value),
            SimpleThenUpdateValue::Path(_) => None,
        })
        .unwrap_or_default();
    Some(UpdateExpression::PrefixPayloadConcat {
        prefix,
        payload_path,
        separator,
    })
}

fn source_payload_field_from_path(path: &str, source_variants: &[String]) -> Option<String> {
    source_variants.iter().find_map(|variant| {
        let suffix = source_payload_suffix_from_variant(path, variant)?;
        Some(match suffix {
            "change.text" | "event.change.text" | "events.change.text" => "text".to_owned(),
            "change.bytes" | "event.change.bytes" | "events.change.bytes" => "bytes".to_owned(),
            "key_down.key" | "event.key_down.key" | "events.key_down.key" => "key".to_owned(),
            "event.address" | "events.address" => "address".to_owned(),
            _ if !suffix.contains('.') => suffix.to_owned(),
            _ if suffix.starts_with("event.") => {
                source_payload_field_from_event_suffix(&suffix["event.".len()..])?
            }
            _ if suffix.starts_with("events.") => {
                source_payload_field_from_event_suffix(&suffix["events.".len()..])?
            }
            _ => return None,
        })
    })
}

fn source_payload_field_from_event_suffix(suffix: &str) -> Option<String> {
    if !suffix.contains('.') {
        return Some(suffix.to_owned());
    }
    let mut parts = suffix.split('.');
    let _event_name = parts.next()?;
    let payload_field = parts.next()?;
    parts.next().is_none().then(|| payload_field.to_owned())
}

fn source_payload_suffix_from_variant<'a>(path: &'a str, variant: &str) -> Option<&'a str> {
    if let Some(suffix) = path
        .strip_prefix(variant)
        .and_then(|suffix| suffix.strip_prefix('.'))
    {
        return Some(suffix);
    }
    let (base, event) = variant.rsplit_once('.')?;
    for event_prefix in [
        format!("{base}.event.{event}"),
        format!("{base}.events.{event}"),
    ] {
        if let Some(suffix) = path
            .strip_prefix(&event_prefix)
            .and_then(|suffix| suffix.strip_prefix('.'))
        {
            return Some(suffix);
        }
    }
    None
}

impl FieldDef {
    fn expression_has_match_binding(&self, expr_id: usize, name: &str) -> bool {
        statement_match_binding_for_expr(&self.statement, expr_id, name, &self.ast_exprs)
    }

    fn has_token(&self, token: &str) -> bool {
        self.ast_items
            .iter()
            .any(|item| item_has_symbol(item, token))
    }

    fn has_operator(&self, operator: &str) -> bool {
        self.ast_exprs.iter().any(|expr| match &expr.kind {
            AstExprKind::Pipe { op, .. } => op == operator,
            AstExprKind::Call { function, .. } => function == operator,
            _ => false,
        })
    }

    fn has_any_operator(&self, operators: &[&str]) -> bool {
        operators.iter().any(|operator| self.has_operator(operator))
    }

    fn has_then_expr(&self) -> bool {
        self.ast_exprs
            .iter()
            .any(|expr| matches!(expr.kind, AstExprKind::Then { .. }))
    }

    fn mentions_identifier_expr(&self, identifier: &str) -> bool {
        self.ast_exprs.iter().any(|expr| match &expr.kind {
            AstExprKind::Identifier(value) => value == identifier,
            AstExprKind::Path(parts) => parts.iter().any(|part| part == identifier),
            _ => false,
        })
    }

    fn has_then_from_local_with_empty_output(&self, local_name: &str) -> bool {
        self.ast_exprs.iter().any(|expr| {
            let AstExprKind::Then {
                input,
                output: Some(output),
            } = expr.kind
            else {
                return false;
            };
            ast_argument_value(self, input).as_deref() == Some(local_name)
                && self
                    .ast_exprs
                    .iter()
                    .find(|candidate| candidate.id == output)
                    .is_some_and(|output| {
                        ast_initial_value(output, &self.ast_exprs, &[], None)
                            == InitialValue::Text {
                                value: String::new(),
                            }
                    })
        })
    }

    fn references_source_variant(&self, source_variant: &str) -> bool {
        let path_parts = dotted_path_parts(source_variant);
        self.ast_exprs.iter().any(|expr| match &expr.kind {
            AstExprKind::Path(parts) => path_parts_match_source_ref(parts, &path_parts),
            AstExprKind::Identifier(value) if path_parts.len() == 1 => value == path_parts[0],
            _ => false,
        })
    }

    fn first_referenced_payload_field(&self, source_variant: &str) -> Option<String> {
        self.referenced_payload_fields(source_variant)
            .into_iter()
            .next()
            .map(|field| match field {
                SourcePayloadField::Address => "address".to_owned(),
                SourcePayloadField::Bytes => "bytes".to_owned(),
                SourcePayloadField::Key => "key".to_owned(),
                SourcePayloadField::Named(name) => name,
                SourcePayloadField::Text => "text".to_owned(),
            })
    }

    fn referenced_payload_fields(&self, source_variant: &str) -> BTreeSet<SourcePayloadField> {
        let variants = vec![source_variant.to_owned()];
        self.ast_exprs
            .iter()
            .filter_map(|expr| match &expr.kind {
                AstExprKind::Path(parts) => {
                    source_payload_field_from_path(&parts.join("."), &variants)
                }
                _ => None,
            })
            .filter(|name| name != "__" && name != "SKIP")
            .map(|name| SourcePayloadField::from_name(&name))
            .collect()
    }

    fn source_branch(&self, source: &str) -> Option<RoutedBranch> {
        source_ref_variants(source)
            .iter()
            .find_map(|variant| self.source_branch_variant(variant))
    }

    fn source_trigger_branch(&self, source: &str) -> Option<RoutedBranch> {
        source_ref_variants(source)
            .iter()
            .find_map(|variant| self.source_trigger_branch_variant(variant))
    }

    fn source_branch_variant(&self, source_variant: &str) -> Option<RoutedBranch> {
        let source_parts = dotted_path_parts(source_variant);
        let start_line = self.ast_exprs.iter().find_map(|expr| match &expr.kind {
            AstExprKind::Path(parts) if path_parts_match_source_ref(parts, &source_parts) => {
                Some(expr.line)
            }
            AstExprKind::Identifier(value)
                if source_parts.len() == 1 && value == source_parts[0] =>
            {
                Some(expr.line)
            }
            _ => None,
        })?;
        let start = self
            .ast_items
            .iter()
            .position(|item| item.line == start_line)?;
        let start_indent = self.ast_items[start].indent;
        let mut depth = 0i32;
        let mut items = Vec::new();
        for (offset, item) in self.ast_items.iter().skip(start).enumerate() {
            let same_indent_pipe_continuation =
                item.indent == start_indent && item_starts_with_symbol(item, "|>");
            if offset > 0 && item.indent <= start_indent && !same_indent_pipe_continuation {
                break;
            }
            items.push(item.clone());
            let scope_delta = item
                .symbols
                .iter()
                .map(|lexeme| match lexeme.as_str() {
                    "{" => 1,
                    "}" => -1,
                    _ => 0,
                })
                .sum::<i32>();
            depth += scope_delta;
            let has_indented_continuation =
                self.ast_items.get(start + offset + 1).is_some_and(|next| {
                    next.indent > start_indent
                        || (next.indent == start_indent && item_starts_with_symbol(next, "|>"))
                });
            if offset == 0 && depth == 0 && scope_delta == 0 && !has_indented_continuation {
                break;
            }
            if depth <= 0 && item_has_symbol(item, "}") {
                break;
            }
        }
        let lines = items.iter().map(|item| item.line).collect::<Vec<_>>();
        let ast_exprs = self
            .ast_exprs
            .iter()
            .filter(|expr| lines.contains(&expr.line))
            .cloned()
            .collect();
        Some(RoutedBranch { items, ast_exprs })
    }

    fn source_trigger_branch_variant(&self, source_variant: &str) -> Option<RoutedBranch> {
        let source_parts = dotted_path_parts(source_variant);
        let start_line = self.ast_exprs.iter().find_map(|expr| {
            let line_starts_with_source = self
                .ast_items
                .iter()
                .find(|item| {
                    item.line == expr.line
                        && item.hold.is_none()
                        && item_symbols_start_with_path(item, &source_parts)
                })
                .is_some();
            if !line_starts_with_source {
                return None;
            }
            match &expr.kind {
                AstExprKind::Path(parts) if path_parts_match_source_ref(parts, &source_parts) => {
                    Some(expr.line)
                }
                AstExprKind::Identifier(value)
                    if source_parts.len() == 1 && value == source_parts[0] =>
                {
                    Some(expr.line)
                }
                _ => None,
            }
        })?;
        self.branch_from_line(start_line)
    }

    fn branch_from_line(&self, start_line: usize) -> Option<RoutedBranch> {
        let start = self
            .ast_items
            .iter()
            .position(|item| item.line == start_line)?;
        let start_indent = self.ast_items[start].indent;
        let mut depth = 0i32;
        let mut items = Vec::new();
        for (offset, item) in self.ast_items.iter().skip(start).enumerate() {
            let same_indent_pipe_continuation =
                item.indent == start_indent && item_starts_with_symbol(item, "|>");
            if offset > 0 && item.indent <= start_indent && !same_indent_pipe_continuation {
                break;
            }
            items.push(item.clone());
            let scope_delta = item
                .symbols
                .iter()
                .map(|lexeme| match lexeme.as_str() {
                    "{" => 1,
                    "}" => -1,
                    _ => 0,
                })
                .sum::<i32>();
            depth += scope_delta;
            let has_indented_continuation =
                self.ast_items.get(start + offset + 1).is_some_and(|next| {
                    next.indent > start_indent
                        || (next.indent == start_indent && item_starts_with_symbol(next, "|>"))
                });
            if offset == 0 && depth == 0 && scope_delta == 0 && !has_indented_continuation {
                break;
            }
            if depth <= 0 && item_has_symbol(item, "}") {
                break;
            }
        }
        let lines = items.iter().map(|item| item.line).collect::<Vec<_>>();
        let ast_exprs = self
            .ast_exprs
            .iter()
            .filter(|expr| lines.contains(&expr.line))
            .cloned()
            .collect();
        Some(RoutedBranch { items, ast_exprs })
    }
}

fn statement_match_binding_for_expr(
    statement: &AstStatement,
    expr_id: usize,
    name: &str,
    expressions: &[AstExpr],
) -> bool {
    let binds_name = statement
        .expr
        .and_then(|statement_expr_id| expressions.iter().find(|expr| expr.id == statement_expr_id))
        .and_then(|expr| match &expr.kind {
            AstExprKind::MatchArm { pattern, .. } => match_arm_binding_name(pattern),
            _ => None,
        })
        .is_some_and(|binding| binding == name);
    if binds_name && statement_subtree_contains_expr(statement, expr_id, expressions) {
        return true;
    }
    statement
        .children
        .iter()
        .any(|child| statement_match_binding_for_expr(child, expr_id, name, expressions))
}

fn statement_subtree_contains_expr(
    statement: &AstStatement,
    expr_id: usize,
    expressions: &[AstExpr],
) -> bool {
    statement.expr.is_some_and(|root| {
        root == expr_id || expr_contains_expr_id_in_exprs(expressions, root, expr_id)
    }) || statement
        .children
        .iter()
        .any(|child| statement_subtree_contains_expr(child, expr_id, expressions))
}

fn match_arm_binding_name(pattern: &[String]) -> Option<&str> {
    let [name] = pattern else {
        return None;
    };
    (name != "_" && name != "__" && value_starts_lowercase_identifier(name))
        .then_some(name.as_str())
}

fn direct_source_refs_by_path(
    fields: &[FieldDef],
    program: &ParsedProgram,
    checked: &boon_typecheck::CheckedProgram,
) -> Result<BTreeMap<String, Vec<String>>, String> {
    fn statement_declaration(
        statement: &boon_typecheck::CheckedStatement,
    ) -> Option<boon_typecheck::DeclId> {
        match statement.kind {
            boon_typecheck::CheckedStatementKind::Function { declaration }
            | boon_typecheck::CheckedStatementKind::Field { declaration } => Some(declaration),
            boon_typecheck::CheckedStatementKind::Source { declaration, .. }
            | boon_typecheck::CheckedStatementKind::Hold { declaration, .. }
            | boon_typecheck::CheckedStatementKind::List { declaration, .. } => declaration,
            boon_typecheck::CheckedStatementKind::Block
            | boon_typecheck::CheckedStatementKind::Spread
            | boon_typecheck::CheckedStatementKind::Expression => None,
        }
    }

    let mut source_paths = BTreeMap::<boon_typecheck::DeclId, BTreeSet<String>>::new();
    for source in &program.source_ports {
        let Some(expression_id) = source.expr_id else {
            continue;
        };
        let expression = checked
            .expressions
            .get(expression_id)
            .filter(|expression| expression.id.0 as usize == expression_id)
            .ok_or_else(|| {
                format!(
                    "source metadata `{}` references missing checked expression {expression_id}",
                    source.path
                )
            })?;
        let declaration = expression.declaration.ok_or_else(|| {
            format!(
                "source metadata `{}` has no checked declaration ownership",
                source.path
            )
        })?;
        source_paths
            .entry(declaration)
            .or_default()
            .insert(source.path.clone());
    }

    let mut result = BTreeMap::new();
    for field in fields {
        let statement_id = boon_typecheck::CheckedStatementId(field.statement.id as u32);
        let declaration = checked
            .statements
            .iter()
            .find(|statement| statement.id == statement_id)
            .and_then(statement_declaration)
            .ok_or_else(|| {
                format!(
                    "semantic field `{}` has no checked declaration ownership",
                    field.path
                )
            })?;
        let sources = checked
            .expressions
            .iter()
            .filter(|expression| expression.declaration == Some(declaration))
            .filter_map(|expression| match &expression.kind {
                boon_typecheck::CheckedExpressionKind::Read { target, .. } => Some(*target),
                _ => None,
            })
            .flat_map(|target| source_paths.get(&target).into_iter().flatten().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        result.insert(field.path.clone(), sources);
    }
    Ok(result)
}

fn add_distributed_event_source_refs(
    fields: &[FieldDef],
    references: &[DistributedValueReference],
    direct_sources: &mut BTreeMap<String, Vec<String>>,
) {
    for reference in references.iter().filter(|reference| {
        matches!(
            reference.flow_mode,
            boon_typecheck::FlowMode::TickPresent | boon_typecheck::FlowMode::PresentOrAbsent
        )
    }) {
        let source_path = distributed_event_source_path(&reference.canonical_path);
        for field in fields
            .iter()
            .filter(|field| reference.local_alias_paths.contains(&field.path))
        {
            let sources = direct_sources.entry(field.path.clone()).or_default();
            push_unique(sources, source_path.clone());
        }
    }
}

fn bind_distributed_reference_aliases(
    fields: &[FieldDef],
    references: &mut [DistributedValueReference],
) {
    for reference in references {
        reference.local_alias_paths = fields
            .iter()
            .filter(|field| field.statement.expr == Some(reference.expr_id.as_usize()))
            .map(|field| field.path.clone())
            .collect();
    }
}

fn direct_sources_for_field<'a>(
    direct_sources: &'a BTreeMap<String, Vec<String>>,
    field: &FieldDef,
) -> impl Iterator<Item = &'a String> {
    direct_sources
        .get(&field.path)
        .into_iter()
        .flat_map(|sources| sources.iter())
}

fn direct_source_refs(field: &FieldDef, program: &ParsedProgram) -> Vec<String> {
    let mut sources = Vec::new();
    for source in &program.source_ports {
        if source_ref_variants_for_program(&source.path, program)
            .iter()
            .any(|variant| field.references_source_variant(variant))
        {
            push_unique(&mut sources, source.path.clone());
        }
    }
    sources
}

fn source_ref_variants(path: &str) -> Vec<String> {
    let mut variants = vec![path.to_owned()];
    if let Some((_, suffix)) = path.split_once('.') {
        variants.push(suffix.to_owned());
        variants.push(format!("item.{suffix}"));
    }
    if let Some(tail) = store_list_source_tail_without_program(path) {
        variants.push(tail);
    }
    variants
}

fn source_ref_variants_for_program(path: &str, program: &ParsedProgram) -> Vec<String> {
    let mut variants = vec![path.to_owned()];
    let Some((_, suffix)) = path.split_once('.') else {
        return variants;
    };
    if source_suffix_is_unique(suffix, program) {
        variants.push(suffix.to_owned());
        variants.push(format!("item.{suffix}"));
    }
    if let Some(list_item_tail) = unique_list_item_source_tail(path, program) {
        variants.push(list_item_tail);
    }
    variants
}

fn source_suffix_is_unique(suffix: &str, program: &ParsedProgram) -> bool {
    let suffix = format!(".{suffix}");
    program
        .source_ports
        .iter()
        .filter(|source| source.path.ends_with(&suffix))
        .take(2)
        .count()
        == 1
}

fn unique_list_item_source_tail(path: &str, program: &ParsedProgram) -> Option<String> {
    let tail = store_list_source_tail(path, program)?;
    let tail_suffix = format!(".{tail}");
    let unique = program
        .source_ports
        .iter()
        .filter(|source| source.path.ends_with(&tail_suffix))
        .take(2)
        .count()
        == 1;
    unique.then_some(tail)
}

fn store_list_source_tail(path: &str, program: &ParsedProgram) -> Option<String> {
    let parts = dotted_path_parts(path);
    let ["store", list, tail @ ..] = parts.as_slice() else {
        return None;
    };
    if tail.is_empty()
        || !program
            .list_memories
            .iter()
            .any(|memory| memory.name == *list)
    {
        return None;
    }
    Some(tail.join("."))
}

fn store_list_source_tail_without_program(path: &str) -> Option<String> {
    let parts = dotted_path_parts(path);
    let ["store", _list, tail @ ..] = parts.as_slice() else {
        return None;
    };
    (!tail.is_empty()).then(|| tail.join("."))
}

fn typed_field_defs(program: &ParsedProgram) -> Vec<FieldDef> {
    let mut fields = Vec::new();
    let items = program.ast.semantic_parser_items().collect::<Vec<_>>();
    let function_bodies = field_function_body_index(&program.ast.statements);
    gather_field_defs_from_statements(
        &program.ast.statements,
        &mut Vec::new(),
        program,
        &items,
        &function_bodies,
        &mut Vec::new(),
        &mut fields,
    );
    fields
}

fn field_function_body_index(statements: &[AstStatement]) -> BTreeMap<&str, &[AstStatement]> {
    let mut functions = BTreeMap::new();
    collect_field_function_body_index(statements, &mut functions);
    functions
}

fn collect_field_function_body_index<'a>(
    statements: &'a [AstStatement],
    functions: &mut BTreeMap<&'a str, &'a [AstStatement]>,
) {
    for statement in statements {
        if let AstStatementKind::Function { name, .. } = &statement.kind {
            functions.insert(name.as_str(), statement.children.as_slice());
        }
        collect_field_function_body_index(&statement.children, functions);
    }
}

fn gather_field_defs_from_statements(
    statements: &[AstStatement],
    scope: &mut Vec<String>,
    program: &ParsedProgram,
    items: &[&AstItem],
    function_bodies: &BTreeMap<&str, &[AstStatement]>,
    function_stack: &mut Vec<String>,
    fields: &mut Vec<FieldDef>,
) {
    for statement in statements {
        gather_field_defs_from_called_functions(
            statement,
            scope,
            program,
            items,
            function_bodies,
            function_stack,
            fields,
        );
        match &statement.kind {
            AstStatementKind::Function { name, .. } => {
                let function_row_scopes = function_row_scopes(name, program)
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                for row_scope in function_row_scopes {
                    scope.push(row_scope);
                    function_stack.push(name.clone());
                    if function_body_defines_record_fields(
                        &statement.children,
                        &program.ast.expressions,
                        &program.ast.lines,
                    ) {
                        gather_field_defs_from_statements(
                            &statement.children,
                            scope,
                            program,
                            items,
                            function_bodies,
                            function_stack,
                            fields,
                        );
                    } else {
                        gather_field_defs_from_called_functions_in_statements(
                            &statement.children,
                            scope,
                            program,
                            items,
                            function_bodies,
                            function_stack,
                            fields,
                        );
                    }
                    function_stack.pop();
                    scope.pop();
                }
            }
            AstStatementKind::Field { name } => {
                if should_record_field_statement(name, scope, program) {
                    push_field_def(statement, name, scope, program, items, fields);
                }
                if !statement.children.is_empty() {
                    scope.push(name.clone());
                    gather_field_defs_from_statements(
                        &statement.children,
                        scope,
                        program,
                        items,
                        function_bodies,
                        function_stack,
                        fields,
                    );
                    scope.pop();
                }
            }
            AstStatementKind::Hold {
                field: Some(name), ..
            } => {
                if should_record_field_statement(name, scope, program) {
                    push_field_def(statement, name, scope, program, items, fields);
                }
                gather_field_defs_from_statements(
                    &statement.children,
                    scope,
                    program,
                    items,
                    function_bodies,
                    function_stack,
                    fields,
                );
            }
            AstStatementKind::List {
                field: Some(name), ..
            } => {
                if should_record_field_statement(name, scope, program) {
                    push_field_def(statement, name, scope, program, items, fields);
                }
            }
            AstStatementKind::Expression
                if statement.expr.is_some_and(|expr_id| {
                    program.ast.expressions.get(expr_id).is_some_and(|expr| {
                        matches!(
                            expr.kind,
                            AstExprKind::Call { .. } | AstExprKind::Pipe { .. }
                        )
                    })
                }) =>
            {
                // Multiline call children are arguments owned by this expression,
                // not independently addressable semantic fields.
            }
            AstStatementKind::Block
            | AstStatementKind::Spread
            | AstStatementKind::Expression
            | AstStatementKind::Hold { .. }
            | AstStatementKind::List { field: None, .. }
            | AstStatementKind::Source { .. } => {
                gather_field_defs_from_statements(
                    &statement.children,
                    scope,
                    program,
                    items,
                    function_bodies,
                    function_stack,
                    fields,
                );
            }
        }
    }
}

fn gather_field_defs_from_called_functions(
    statement: &AstStatement,
    scope: &mut Vec<String>,
    program: &ParsedProgram,
    items: &[&AstItem],
    function_bodies: &BTreeMap<&str, &[AstStatement]>,
    function_stack: &mut Vec<String>,
    fields: &mut Vec<FieldDef>,
) {
    if !scope.iter().any(|name| {
        program
            .row_scope_functions
            .iter()
            .any(|scope| scope.row_scope == *name)
    }) {
        return;
    }
    let Some(expr_id) = statement.expr else {
        return;
    };
    let mut calls = Vec::new();
    collect_field_called_functions(expr_id, &program.ast.expressions, &mut calls);
    for function in calls {
        gather_field_defs_from_helper_function(
            &function,
            scope,
            program,
            items,
            function_bodies,
            function_stack,
            fields,
        );
    }
}

fn gather_field_defs_from_helper_function(
    function: &str,
    scope: &mut Vec<String>,
    program: &ParsedProgram,
    items: &[&AstItem],
    function_bodies: &BTreeMap<&str, &[AstStatement]>,
    function_stack: &mut Vec<String>,
    fields: &mut Vec<FieldDef>,
) {
    if function_stack.iter().any(|entry| entry == function) {
        return;
    }
    if function_has_row_scope(function, program) {
        return;
    }
    let Some(children) = function_bodies.get(function) else {
        return;
    };
    function_stack.push(function.to_owned());
    if function_body_defines_record_fields(children, &program.ast.expressions, &program.ast.lines) {
        gather_field_defs_from_statements(
            children,
            scope,
            program,
            items,
            function_bodies,
            function_stack,
            fields,
        );
    } else {
        gather_field_defs_from_called_functions_in_statements(
            children,
            scope,
            program,
            items,
            function_bodies,
            function_stack,
            fields,
        );
    }
    function_stack.pop();
}

fn gather_field_defs_from_called_functions_in_statements(
    statements: &[AstStatement],
    scope: &mut Vec<String>,
    program: &ParsedProgram,
    items: &[&AstItem],
    function_bodies: &BTreeMap<&str, &[AstStatement]>,
    function_stack: &mut Vec<String>,
    fields: &mut Vec<FieldDef>,
) {
    for statement in statements {
        if let Some(expr_id) = statement.expr {
            let mut calls = Vec::new();
            collect_field_called_functions(expr_id, &program.ast.expressions, &mut calls);
            for function in calls {
                gather_field_defs_from_helper_function(
                    &function,
                    scope,
                    program,
                    items,
                    function_bodies,
                    function_stack,
                    fields,
                );
            }
        }
        gather_field_defs_from_called_functions_in_statements(
            &statement.children,
            scope,
            program,
            items,
            function_bodies,
            function_stack,
            fields,
        );
    }
}

fn function_body_defines_record_fields(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    lines: &[boon_parser::ParserLine],
) -> bool {
    statements.iter().any(|statement| {
        if statement
            .expr
            .and_then(|expr_id| expressions.get(expr_id))
            .is_some_and(expr_is_render_constructor)
        {
            return false;
        }
        if statement_is_record_field(statement) {
            return true;
        }
        if statement_is_record_constructor_block(statement, lines)
            && statement.children.iter().any(statement_is_record_field)
        {
            return true;
        }
        if matches!(statement.kind, AstStatementKind::Expression)
            && statement.children.iter().any(statement_is_record_field)
        {
            return true;
        }
        statement
            .expr
            .and_then(|expr_id| expressions.get(expr_id))
            .is_some_and(|expr| {
                matches!(
                    expr.kind,
                    AstExprKind::Object(_)
                        | AstExprKind::Record(_)
                        | AstExprKind::TaggedObject { .. }
                )
            })
    })
}

fn expr_is_render_constructor(expr: &AstExpr) -> bool {
    match &expr.kind {
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. } => {
            boon_typecheck::is_registered_render_constructor(function)
        }
        _ => false,
    }
}

fn statement_is_record_constructor_block(
    statement: &AstStatement,
    lines: &[boon_parser::ParserLine],
) -> bool {
    matches!(statement.kind, AstStatementKind::Block)
        && lines
            .iter()
            .find(|line| line.line == statement.line)
            .is_some_and(|line| line.symbols.iter().any(|symbol| symbol == "["))
}

fn statement_is_record_field(statement: &AstStatement) -> bool {
    matches!(
        statement.kind,
        AstStatementKind::Spread
            | AstStatementKind::Field { .. }
            | AstStatementKind::Source { .. }
            | AstStatementKind::Hold { field: Some(_), .. }
            | AstStatementKind::List { field: Some(_), .. }
    )
}

fn collect_field_called_functions(
    expr_id: usize,
    expressions: &[AstExpr],
    calls: &mut Vec<String>,
) {
    let Some(expr) = expressions.get(expr_id) else {
        return;
    };
    match &expr.kind {
        AstExprKind::Call { function, args, .. } => {
            calls.push(function.clone());
            for arg in args {
                collect_field_called_functions(arg.value, expressions, calls);
            }
        }
        AstExprKind::Pipe {
            input, op, args, ..
        } => {
            if !op.starts_with("Field/") {
                collect_field_called_functions(*input, expressions, calls);
            }
            calls.push(op.clone());
            for arg in args {
                collect_field_called_functions(arg.value, expressions, calls);
            }
        }
        AstExprKind::Hold { initial, .. }
        | AstExprKind::When { input: initial, .. }
        | AstExprKind::Draining { input: initial } => {
            collect_field_called_functions(*initial, expressions, calls);
        }
        AstExprKind::Then { input, output } => {
            collect_field_called_functions(*input, expressions, calls);
            if let Some(output) = output {
                collect_field_called_functions(*output, expressions, calls);
            }
        }
        AstExprKind::Infix { left, right, .. } => {
            collect_field_called_functions(*left, expressions, calls);
            collect_field_called_functions(*right, expressions, calls);
        }
        AstExprKind::MatchArm { output, .. } => {
            if let Some(output) = output {
                collect_field_called_functions(*output, expressions, calls);
            }
        }
        AstExprKind::Block { bindings, result } => {
            for binding in bindings {
                collect_field_called_functions(binding.value, expressions, calls);
            }
            if let Some(result) = result {
                collect_field_called_functions(*result, expressions, calls);
            }
        }
        AstExprKind::Record(fields) | AstExprKind::Object(fields) => {
            for field in fields {
                collect_field_called_functions(field.value, expressions, calls);
            }
        }
        AstExprKind::TaggedObject { fields, .. } => {
            for field in fields {
                collect_field_called_functions(field.value, expressions, calls);
            }
        }
        AstExprKind::ListLiteral { items, .. } => {
            for item in items {
                collect_field_called_functions(*item, expressions, calls);
            }
        }
        AstExprKind::BytesLiteral { items, .. } => {
            for item in items {
                collect_field_called_functions(*item, expressions, calls);
            }
        }
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::Drain { .. }
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => {}
    }
}

fn push_field_def(
    statement: &AstStatement,
    name: &str,
    scope: &[String],
    program: &ParsedProgram,
    items: &[&AstItem],
    fields: &mut Vec<FieldDef>,
) {
    let parent_path = scope.join(".");
    let path = if parent_path.is_empty() {
        name.to_owned()
    } else {
        format!("{parent_path}.{name}")
    };
    if fields.iter().any(|field| field.path == path) {
        return;
    }
    fields.push(FieldDef {
        path,
        local_name: name.to_owned(),
        parent_path,
        statement: statement.clone(),
        ast_items: collect_statement_ast_items(statement, items),
        ast_exprs: collect_statement_ast_exprs(statement, program),
    });
}

fn collect_statement_ast_exprs(statement: &AstStatement, program: &ParsedProgram) -> Vec<AstExpr> {
    let mut expr_ids = Vec::new();
    collect_statement_expr_ids(statement, program, &mut Vec::new(), &mut expr_ids);
    let mut lines = Vec::new();
    collect_statement_lines(statement, &mut lines);
    for expr in &program.ast.expressions {
        if lines.contains(&expr.line) && !expr_ids.contains(&expr.id) {
            collect_expr_tree(expr.id, program, &mut Vec::new(), &mut expr_ids);
        }
    }
    expr_ids
        .into_iter()
        .filter_map(|id| program.ast.expressions.get(id).cloned())
        .collect()
}

fn collect_statement_expr_ids(
    statement: &AstStatement,
    program: &ParsedProgram,
    seen: &mut Vec<usize>,
    exprs: &mut Vec<usize>,
) {
    if let Some(expr) = statement.expr {
        collect_expr_tree(expr, program, seen, exprs);
    }
    for child in &statement.children {
        collect_statement_expr_ids(child, program, seen, exprs);
    }
}

fn collect_expr_tree(
    id: usize,
    program: &ParsedProgram,
    seen: &mut Vec<usize>,
    exprs: &mut Vec<usize>,
) {
    if seen.contains(&id) {
        return;
    }
    seen.push(id);
    exprs.push(id);
    let Some(expr) = program.ast.expressions.get(id) else {
        return;
    };
    match &expr.kind {
        AstExprKind::Call { args, .. } => {
            for arg in args {
                collect_expr_tree(arg.value, program, seen, exprs);
            }
        }
        AstExprKind::Pipe { input, args, .. } => {
            collect_expr_tree(*input, program, seen, exprs);
            for arg in args {
                collect_expr_tree(arg.value, program, seen, exprs);
            }
        }
        AstExprKind::Hold { initial, .. }
        | AstExprKind::When { input: initial, .. }
        | AstExprKind::Draining { input: initial } => {
            collect_expr_tree(*initial, program, seen, exprs);
        }
        AstExprKind::Then { input, output } => {
            collect_expr_tree(*input, program, seen, exprs);
            if let Some(output) = output {
                collect_expr_tree(*output, program, seen, exprs);
            }
        }
        AstExprKind::Infix { left, right, .. } => {
            collect_expr_tree(*left, program, seen, exprs);
            collect_expr_tree(*right, program, seen, exprs);
        }
        AstExprKind::MatchArm { output, .. } => {
            if let Some(output) = output {
                collect_expr_tree(*output, program, seen, exprs);
            }
        }
        AstExprKind::Block { bindings, result } => {
            for binding in bindings {
                collect_expr_tree(binding.value, program, seen, exprs);
            }
            if let Some(result) = result {
                collect_expr_tree(*result, program, seen, exprs);
            }
        }
        AstExprKind::Record(fields) | AstExprKind::Object(fields) => {
            for field in fields {
                collect_expr_tree(field.value, program, seen, exprs);
            }
        }
        AstExprKind::TaggedObject { fields, .. } => {
            for field in fields {
                collect_expr_tree(field.value, program, seen, exprs);
            }
        }
        AstExprKind::ListLiteral { items, .. } | AstExprKind::BytesLiteral { items, .. } => {
            for item in items {
                collect_expr_tree(*item, program, seen, exprs);
            }
        }
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::Drain { .. }
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => {}
    }
}

fn should_record_field_statement(
    local_name: &str,
    scope: &[String],
    program: &ParsedProgram,
) -> bool {
    let candidate_path = if scope.is_empty() {
        local_name.to_owned()
    } else {
        format!("{}.{}", scope.join("."), local_name)
    };
    let top_level_data_scope = scope.first().is_some_and(|root| {
        !matches!(root.as_str(), "store" | "document" | "scene" | "host_ports")
    });
    local_name != "sources"
        && local_name != "host_ports"
        && !scope.iter().any(|name| name == "sources")
        && !scope.iter().any(|name| name == "host_ports")
        && scope.first().is_none_or(|name| name != "effects")
        && (program
            .state_cells
            .iter()
            .any(|cell| cell.path == candidate_path)
            || top_level_data_scope
            || scope.iter().any(|name| {
                name == "store"
                    || program
                        .row_scope_functions
                        .iter()
                        .any(|scope| scope.row_scope == *name)
            }))
}

fn collect_statement_ast_items(statement: &AstStatement, items: &[&AstItem]) -> Vec<AstItem> {
    let mut lines = Vec::new();
    collect_statement_lines(statement, &mut lines);
    items
        .iter()
        .filter(|item| lines.iter().any(|line| line == &item.line))
        .map(|item| (*item).clone())
        .collect()
}

fn collect_statement_lines(statement: &AstStatement, lines: &mut Vec<usize>) {
    lines.push(statement.line);
    for child in &statement.children {
        collect_statement_lines(child, lines);
    }
}

fn collect_field_ast_items(items: &[&AstItem], start: usize, indent: usize) -> Vec<AstItem> {
    let mut body = Vec::new();
    for item in &items[start..] {
        let current_indent = item.indent;
        if current_indent <= indent && !body.is_empty() && item.line != items[start].line {
            break;
        }
        body.push((*item).clone());
    }
    body
}

fn function_row_scopes<'a>(
    name: &str,
    program: &'a ParsedProgram,
) -> impl Iterator<Item = &'a str> {
    program
        .row_scope_functions
        .iter()
        .filter(move |scope| {
            scope.function == name
                || scope
                    .function
                    .strip_prefix("__source_row_scope_")
                    .is_some_and(|source_function| source_function == name)
        })
        .map(|scope| scope.row_scope.as_str())
}

fn function_has_row_scope(name: &str, program: &ParsedProgram) -> bool {
    program.row_scope_functions.iter().any(|scope| {
        scope.function == name
            || scope
                .function
                .strip_prefix("__source_row_scope_")
                .is_some_and(|source_function| source_function == name)
    })
}

fn push_unique(output: &mut Vec<String>, value: String) {
    if !output.contains(&value) {
        output.push(value);
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

fn sanitize_node_name(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .chars()
        .take(48)
        .collect()
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
            .storage
            .bindings
            .iter()
            .find(|binding| binding.diagnostic_path == "defaults")
            .expect("literal list storage binding");
        let StorageBindingKind::Value {
            field: None,
            list: Some(list),
            row_scope: Some(row_scope),
        } = binding.kind
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
            .storage
            .bindings
            .iter()
            .find(|binding| binding.diagnostic_path == "mapped")
            .expect("computed list storage binding");
        assert!(matches!(
            binding.kind,
            StorageBindingKind::Value {
                field: None,
                list: Some(binding_list),
                row_scope: Some(binding_scope),
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
