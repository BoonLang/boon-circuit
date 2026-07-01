use boon_ir::{
    TypedProgram, debug_tables, lower_profiled, lower_runtime_profiled, verify_hidden_identity,
    verify_static_schedule,
};
pub use boon_parser::{
    AstCallArg, AstExpr, AstExprKind, AstRecordField, AstStatement, AstStatementKind,
    BytesSizeSyntax, DocumentAst, ParsedProgram, parse_project, parse_source,
};

pub use boon_plan::{MachinePlan, PlanError, TargetProfile};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

mod legacy_backend;

pub type CompilerResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Debug, PartialEq)]
pub struct CompilerSourceUnit {
    pub path: String,
    pub source: String,
}

#[derive(Clone, Debug)]
pub struct CompiledMachinePlanFromSource {
    pub parsed: ParsedProgram,
    pub ir: TypedProgram,
    pub plan: MachinePlan,
    pub load_pipeline_profile: JsonValue,
}

#[derive(Clone, Debug)]
pub struct CompiledRuntimeIrFromSource {
    pub parsed: ParsedProgram,
    pub ir: TypedProgram,
    pub runtime_program: CompilerRuntimeProgram,
    pub load_pipeline_profile: JsonValue,
}

#[derive(Clone, Debug)]
pub struct CompiledFullIrFromSource {
    pub parsed: ParsedProgram,
    pub ir: TypedProgram,
    pub runtime_program: CompilerRuntimeProgram,
    pub load_pipeline_profile: JsonValue,
}

#[derive(Clone, Debug)]
pub struct CompilerRuntimeProgram {
    pub symbols: CompilerRuntimeSymbols,
    pub unsupported_diagnostics: CompilerUnsupportedRuntimeDiagnostics,
    pub storage_root_slots: Vec<CompilerStorageRootSlot>,
    pub storage_indexed_row_initial_resets: Vec<CompilerStorageIndexedRowInitialReset>,
    pub storage_list_slots: Vec<CompilerStorageListSlot>,
    pub storage_row_templates: Vec<CompilerStorageRowTemplate>,
    pub storage_initial_rows: Vec<CompilerStorageInitialRows>,
    pub storage_indexed_derived_fields: Vec<CompilerStorageIndexedDerivedFields>,
    pub scalar_equations: CompilerScalarEquationPlan,
    pub derived_equations: CompilerDerivedEquationPlan,
    pub generic_derived_plan: CompilerGenericDerivedPlan,
    pub list_operations: Vec<CompilerListOperation>,
    pub list_projections: Vec<CompilerListProjection>,
    pub root_state_paths: Vec<String>,
    pub list_summary_fields: Vec<CompilerListSummaryFields>,
    pub dynamic_list_view_lists: BTreeSet<String>,
    pub observed_root_paths: BTreeSet<String>,
    pub projection_storage: CompilerDocumentProjectionStorageResolutions,
    pub document_render_slots: CompilerDocumentRenderSlots,
    pub field_slot_collision_diagnostics: Vec<CompilerFieldSlotCollisionDiagnostic>,
    pub source_route_root_targets: BTreeSet<String>,
    pub source_route_sources: Vec<CompilerSourceRouteSource>,
    pub source_route_bool_facts: CompilerSourceRouteBoolFacts,
    pub source_route_router_targets: Vec<CompilerSourceRouteRouterRoute>,
    pub source_route_root_text_transform_targets: Vec<CompilerSourceRouteRootTextTransform>,
    pub static_analysis: CompilerStaticProgramAnalysis,
    pub list_source_bindings: Vec<CompilerListSourceBindingSlot>,
    pub source_payload_counts: CompilerSourcePayloadCounts,
    pub storage_layout_counts: CompilerTypedStorageLayoutCounts,
    pub inventory_counts: CompilerTypedProgramInventoryCounts,
    pub program_metadata: CompilerTypedProgramReportMetadata,
    pub typecheck_metadata: CompilerTypecheckReportMetadata,
    pub runtime_profile_metadata: CompilerRuntimeProfileMetadata,
    pub ir_debug_tables: JsonValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledSourceReportContext {
    pub source_hash: String,
    pub source_units: Vec<CompilerSourceUnit>,
    pub source_files: Vec<String>,
    pub program_hash: String,
    pub program_kind: String,
    pub program_file_count: usize,
    pub graph_node_count: usize,
    pub load_pipeline_profile: JsonValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompilerTypecheckReportMetadata {
    pub typecheck_report_hash: String,
    pub render_slot_table_hash: String,
    pub typed_render_metadata_used: bool,
    pub unresolved_type_variable_count: usize,
    pub render_slot_count: usize,
    pub render_slot_failure_count: usize,
    pub report: JsonValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompilerTypedProgramReportMetadata {
    pub expression_count: usize,
    pub expression_coverage: JsonValue,
    pub expression_coverage_unknown_total: usize,
    pub graph_node_count: usize,
    pub semantic_index: JsonValue,
    pub hidden_identity_verified: bool,
    pub static_schedule_verified: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CompilerTypedProgramInventoryCounts {
    pub schedule_node_count: usize,
    pub source_port_count: usize,
    pub state_initializer_count: usize,
    pub list_initializer_count: usize,
    pub derived_value_count: usize,
    pub update_branch_count: usize,
    pub list_operation_count: usize,
    pub list_projection_count: usize,
    pub view_binding_count: usize,
}

#[derive(Clone, Debug)]
pub struct CompilerStaticProgramAnalysis {
    pub typecheck_report: boon_typecheck::TypeCheckReport,
    pub view_bindings: Vec<CompilerViewBinding>,
    pub row_scopes: Vec<CompilerRowScope>,
    pub source_paths: Vec<String>,
    pub source_row_lookup_fields: BTreeMap<String, String>,
    pub source_address_lookup_fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerViewBinding {
    pub node_kind: String,
    pub attr: String,
    pub path: String,
    pub kind: CompilerViewBindingKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerViewBindingKind {
    Data,
    Source,
    Target,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerRowScope {
    pub list: String,
    pub row_scope: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerListProjection {
    pub target: String,
    pub list: String,
    pub columns: usize,
    pub rows: usize,
    pub kind: CompilerListProjectionKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerListProjectionKind {
    Chunk {
        item_field: String,
        label_field: String,
    },
    Find {
        field: String,
        value: String,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CompilerSourcePayloadCounts {
    pub schema_count: usize,
    pub field_count: usize,
    pub text_field_count: usize,
    pub key_field_count: usize,
    pub address_field_count: usize,
    pub bytes_field_count: usize,
    pub pointer_field_count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompilerUnsupportedRuntimeDiagnostics {
    pub unsupported_state_initializer: Option<CompilerUnsupportedStateInitializer>,
    pub unsupported_list_initializer: Option<CompilerUnsupportedListInitializer>,
    pub graph_clone_list: Option<CompilerGraphCloneList>,
    pub unsupported_update_branch_count: usize,
    pub unsupported_update_branch: Option<CompilerUnsupportedUpdateBranch>,
    pub unsupported_list_operation_count: usize,
    pub unsupported_list_operation: Option<CompilerUnsupportedListOperation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerUnsupportedStateInitializer {
    pub path: String,
    pub summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerUnsupportedListInitializer {
    pub list: String,
    pub summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerGraphCloneList {
    pub list: String,
    pub graph_clones_per_item: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerUnsupportedUpdateBranch {
    pub target: String,
    pub source: String,
    pub summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerUnsupportedListOperation {
    pub list: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerListSourceBindingSlot {
    pub list: String,
    pub row_scope: String,
    pub source_paths: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerListSummaryFields {
    pub list: String,
    pub row_scope: String,
    pub fields: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerListOperation {
    pub list: String,
    pub kind: CompilerListOperationKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerListOperationKind {
    Append {
        trigger: String,
        fields: Vec<CompilerListAppendField>,
    },
    Remove {
        source: String,
        predicate: CompilerListPredicate,
    },
    Retain {
        target: String,
        predicate: CompilerListPredicate,
    },
    Count {
        target: String,
        predicate: CompilerListPredicate,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerListAppendField {
    pub name: String,
    pub value: CompilerListAppendFieldValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerListAppendFieldValue {
    Source { path: String },
    Const { value: String },
    TypedConst { value: CompilerInitialValue },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerListPredicate {
    AlwaysTrue,
    FieldBool { path: String },
    FieldBoolNot { path: String },
    SelectorVisibility { selector: String, row_field: String },
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerInitialValue {
    Text(String),
    Number(i64),
    Byte(u8),
    Bool(bool),
    Bytes(Vec<u8>),
    Enum(String),
    RootInitialField { path: String },
    RowInitialField { path: String },
    Unknown { summary: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerStorageRootSlot {
    pub path: String,
    pub initializer: CompilerInitialValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerStorageIndexedRowInitialReset {
    pub list: String,
    pub target_field: String,
    pub source_field: String,
    pub original_source_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerStorageListSlot {
    pub id: usize,
    pub name: String,
    pub capacity: Option<usize>,
    pub row_scope: String,
    pub synthetic_list_view_storage: bool,
    pub initializer_kind: CompilerStorageListInitializerKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerStorageListInitializerKind {
    RecordLiteral,
    Range { from: i64, to: i64 },
    Empty,
    DeferredDynamicListView,
    SyntheticListViewStorage,
    Unsupported { summary: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerStorageRowTemplate {
    pub list: String,
    pub row_scope: String,
    pub fields: Vec<CompilerStorageRowFieldTemplate>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerStorageRowFieldTemplate {
    pub field_name: String,
    pub initial_value: CompilerInitialValue,
    pub missing_row_initial_value: Option<CompilerFieldValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerStorageInitialRows {
    pub list: String,
    pub rows: Vec<CompilerStorageInitialRow>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerStorageInitialRow {
    pub fields: Vec<CompilerStorageInitialField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerStorageInitialField {
    pub name: String,
    pub value: CompilerInitialValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerStorageIndexedDerivedFields {
    pub list: String,
    pub row_scope: String,
    pub fields: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerDocumentProjectionStorageResolutions {
    pub resolutions: BTreeMap<String, String>,
    pub unresolved_paths: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompilerDocumentRenderSlots {
    pub render_slot_table_hash: String,
    pub render_slot_count: usize,
    pub render_slot_failure_count: usize,
    pub full_document_typecheck_coverage: bool,
    pub list_map_binding_count_render_slot_materialization: usize,
    pub slots: Vec<CompilerDocumentRenderSlot>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompilerDocumentRenderSlot {
    pub slot_statement_id: usize,
    pub slot_name: String,
    pub expected_contract: String,
    pub value_expr_id: Option<usize>,
    pub actual_type: JsonValue,
    pub diagnostic_count: usize,
    pub optional_list_map_binding_id: Option<usize>,
    pub item_scope_id: Option<usize>,
    pub template_function: Option<String>,
    pub template_arg_count: usize,
    pub materialization_policy: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerRuntimeSymbols {
    pub paths: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerFieldSlotCollisionDiagnostic {
    pub field_id: usize,
    pub labels: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerScalarEquationPlan {
    pub branches: Vec<CompilerScalarUpdateBranch>,
    pub source_paths: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerScalarUpdateBranch {
    pub target: String,
    pub source: String,
    pub expression: CompilerScalarUpdateExpression,
    pub guard: Option<CompilerUpdateGuard>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CompilerUpdateGuard {
    SourcePayloadOneOf {
        field: CompilerSourcePayloadField,
        values: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompilerUpdateMatchArm {
    pub pattern: String,
    pub output: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompilerUpdateValueMatchArm {
    pub pattern: String,
    pub output: CompilerUpdateValueExpression,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CompilerUpdateValueExpression {
    Const {
        value: String,
    },
    ReadPath {
        path: String,
    },
    MatchConst {
        input: String,
        arms: Vec<CompilerUpdateValueMatchArm>,
    },
    NumberInfix {
        left: String,
        op: String,
        right: String,
    },
    MatchNumberInfixConst {
        left: String,
        op: String,
        right: String,
        arms: Vec<CompilerUpdateValueMatchArm>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CompilerBytesScalarArg {
    Static(u64),
    Path(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerScalarUpdateExpression {
    SourceText,
    SourceKey,
    SourceAddress,
    SourcePayload(String),
    Const(String),
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
    MatchNumberInfixConst {
        left: String,
        op: String,
        right: String,
        arms: Vec<CompilerUpdateValueMatchArm>,
    },
    ListFindValue {
        list: String,
        field: String,
        expected: Box<CompilerUpdateValueExpression>,
        target: String,
        fallback: Option<Box<CompilerUpdateValueExpression>>,
    },
    PreviousValue(String),
    ReadPath(String),
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
    BoolNot(String),
    BytesLength(String),
    BytesIsEmpty(String),
    BytesGet {
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
        offset: CompilerBytesScalarArg,
        byte_count: CompilerBytesScalarArg,
    },
    BytesTake {
        path: String,
        byte_count: CompilerBytesScalarArg,
    },
    BytesDrop {
        path: String,
        byte_count: CompilerBytesScalarArg,
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
    FileReadBytes {
        path: String,
    },
    TextToBytes {
        path: String,
        encoding: String,
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
    MatchConst {
        input: String,
        arms: Vec<CompilerUpdateMatchArm>,
    },
    MatchValueConst {
        input: String,
        arms: Vec<CompilerUpdateValueMatchArm>,
    },
    MatchTextIsEmptyConst {
        input: String,
        arms: Vec<CompilerUpdateValueMatchArm>,
    },
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerSourceRouteSource {
    pub path: String,
    pub source_id: usize,
    pub payload_fields: Vec<CompilerSourcePayloadField>,
    pub row_lookup_field: Option<String>,
    pub address_lookup_field: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CompilerSourcePayloadField {
    Address,
    Bytes,
    Key,
    Named(String),
    Text,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerSourceRouteBoolFacts {
    pub scalar_targets: BTreeSet<String>,
    pub read_paths: Vec<CompilerSourceRouteBoolReadPath>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerSourceRouteBoolReadPath {
    pub target: String,
    pub path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerSourceRouteRouterRoute {
    pub source: String,
    pub target: String,
    pub path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerSourceRouteRootTextTransform {
    pub source: String,
    pub target: String,
    pub value: CompilerFieldValue,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompilerDerivedEquationPlan {
    pub text_transforms: Vec<CompilerDerivedTextTransform>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerDerivedTextTransform {
    pub target: String,
    pub source: String,
    pub expression: CompilerDerivedTextExpression,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerDerivedTextExpression {
    Const {
        value: String,
    },
    MatchConst {
        input: String,
        arms: Vec<CompilerUpdateMatchArm>,
    },
    EnterKeyPayloadTextTrimNonEmpty,
    EnterKeyRootTextTrimNonEmpty {
        path: String,
    },
    SourceRootText {
        path: String,
    },
    ListFindValue {
        list: String,
        field: String,
        expected: Box<CompilerUpdateValueExpression>,
        target: String,
        fallback: Option<Box<CompilerUpdateValueExpression>>,
    },
    PrefixRootConcat {
        prefix: String,
        path: String,
        separator: String,
    },
    PrefixPayloadConcat {
        prefix: String,
        payload_path: String,
        separator: String,
    },
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerFieldValue {
    Text(String),
    Bool(bool),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CompilerTypedStorageLayoutCounts {
    pub root_text_slot_count: usize,
    pub root_bool_slot_count: usize,
    pub root_enum_slot_count: usize,
    pub list_memory_count: usize,
    pub list_row_template_field_count: usize,
    pub list_row_text_slot_count: usize,
    pub list_row_bool_slot_count: usize,
    pub list_row_enum_slot_count: usize,
    pub list_hidden_key_slot_count: usize,
    pub list_hidden_generation_slot_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerRuntimeProfileMetadata {
    pub all_lists_bounded: bool,
    pub all_bytes_bounded: bool,
    pub lists: Vec<CompilerRuntimeListCapacity>,
    pub bytes: Vec<CompilerRuntimeBytesCapacity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerRuntimeListCapacity {
    pub name: String,
    pub declared_capacity: Option<usize>,
    pub effective_capacity: Option<usize>,
    pub capacity_source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerRuntimeBytesCapacity {
    pub name: String,
    pub scope: String,
    pub fixed_len: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerGenericDerivedPlan {
    pub expressions: Vec<AstExpr>,
    pub functions: Vec<CompilerGenericDerivedFunction>,
    pub output_roots: Vec<CompilerGenericDerivedOutputRoot>,
    pub root_fields: Vec<CompilerGenericDerivedRootField>,
    pub observed_root_paths: BTreeSet<String>,
    pub indexed_fields: Vec<CompilerGenericDerivedIndexedField>,
    pub runtime_plan: CompilerRuntimeGenericDerivedPlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerGenericDerivedFunction {
    pub name: String,
    pub args: Vec<String>,
    pub statement: AstStatement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerGenericDerivedOutputRoot {
    pub root: String,
    pub output_kind: String,
    pub typed_contract_known: bool,
    pub generic_output_port: bool,
    pub statement: AstStatement,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CompilerDerivedValueKind {
    SourceEventTransform,
    ListView,
    Aggregate,
    Pure,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerGenericDerivedRootField {
    pub path: String,
    pub kind: CompilerDerivedValueKind,
    pub has_sources: bool,
    pub statement: AstStatement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerGenericDerivedIndexedField {
    pub list: String,
    pub row_scope: String,
    pub field: String,
    pub kind: CompilerDerivedValueKind,
    pub startup_recompute: bool,
    pub statement: AstStatement,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompilerRuntimeGenericDerivedPlan {
    pub functions: Vec<CompilerRuntimeGenericFunction>,
    pub output_roots: Vec<CompilerRuntimeGenericOutputRoot>,
    pub root_fields: Vec<CompilerRuntimeGenericRootField>,
    pub indexed_fields: Vec<CompilerRuntimeGenericIndexedField>,
    pub unsupported_reasons: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerRuntimeGenericFunction {
    pub name: String,
    pub args: Vec<String>,
    pub statement: CompilerRuntimeGenericStatement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerRuntimeGenericOutputRoot {
    pub root: String,
    pub output_kind: String,
    pub typed_contract_known: bool,
    pub generic_output_port: bool,
    pub statement: Option<CompilerRuntimeGenericStatement>,
    pub unsupported_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerRuntimeGenericRootField {
    pub path: String,
    pub kind: CompilerDerivedValueKind,
    pub has_sources: bool,
    pub statement: Option<CompilerRuntimeGenericStatement>,
    pub unsupported_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerRuntimeGenericIndexedField {
    pub list: String,
    pub row_scope: String,
    pub field: String,
    pub kind: CompilerDerivedValueKind,
    pub startup_recompute: bool,
    pub statement: Option<CompilerRuntimeGenericStatement>,
    pub unsupported_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerRuntimeGenericStatement {
    Empty,
    Expr(CompilerRuntimeGenericExpr),
    Binding {
        name: String,
        value: Box<CompilerRuntimeGenericStatement>,
    },
    ExprWithChildren {
        expr: CompilerRuntimeGenericExpr,
        children: Vec<CompilerRuntimeGenericStatement>,
    },
    Block(Vec<CompilerRuntimeGenericStatement>),
    List(Vec<CompilerRuntimeGenericStatement>),
    Record(Vec<CompilerRuntimeGenericRecordField>),
    Latest(Vec<CompilerRuntimeGenericStatement>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerRuntimeGenericRecordField {
    pub name: String,
    pub value: CompilerRuntimeGenericStatement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerRuntimeGenericExpr {
    Identifier(String),
    Path(Vec<String>),
    Text(String),
    Number(i64),
    NaN,
    Bool(bool),
    Enum(String),
    TaggedObject {
        tag: String,
        fields: Vec<CompilerRuntimeGenericRecordExprField>,
    },
    Call {
        function: String,
        args: Vec<CompilerRuntimeGenericArg>,
    },
    Pipe {
        input: Box<CompilerRuntimeGenericExpr>,
        op: String,
        args: Vec<CompilerRuntimeGenericArg>,
    },
    Infix {
        left: Box<CompilerRuntimeGenericExpr>,
        op: String,
        right: Box<CompilerRuntimeGenericExpr>,
    },
    Record(Vec<CompilerRuntimeGenericRecordExprField>),
    List(Vec<CompilerRuntimeGenericExpr>),
    Bytes {
        size: BytesSizeSyntax,
        items: Vec<CompilerRuntimeGenericExpr>,
    },
    Then {
        input: Box<CompilerRuntimeGenericExpr>,
        output: Option<Box<CompilerRuntimeGenericExpr>>,
    },
    When {
        input: Box<CompilerRuntimeGenericExpr>,
    },
    MatchArm {
        pattern: Vec<String>,
        output: Option<Box<CompilerRuntimeGenericExpr>>,
    },
    Delimiter,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerRuntimeGenericRecordExprField {
    pub name: String,
    pub value: CompilerRuntimeGenericExpr,
    pub spread: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerRuntimeGenericArg {
    pub name: Option<String>,
    pub value: CompilerRuntimeGenericExpr,
}

impl CompilerStaticProgramAnalysis {
    pub fn from_ir_parts(
        ir: &TypedProgram,
        source_paths: Vec<String>,
        source_address_lookup_fields: BTreeMap<String, String>,
    ) -> Self {
        let source_row_lookup_fields = ir
            .sources
            .iter()
            .filter_map(|source| {
                source
                    .payload_schema
                    .row_lookup_field
                    .as_ref()
                    .or(source.payload_schema.address_lookup_field.as_ref())
                    .map(|field| (source.path.clone(), field.clone()))
            })
            .collect();
        Self {
            typecheck_report: ir.typecheck_report.clone(),
            view_bindings: ir
                .view_bindings
                .iter()
                .map(|binding| CompilerViewBinding {
                    node_kind: binding.node_kind.clone(),
                    attr: binding.attr.clone(),
                    path: binding.path.clone(),
                    kind: match binding.kind {
                        boon_ir::ViewBindingKind::Data => CompilerViewBindingKind::Data,
                        boon_ir::ViewBindingKind::Source => CompilerViewBindingKind::Source,
                        boon_ir::ViewBindingKind::Target => CompilerViewBindingKind::Target,
                    },
                })
                .collect(),
            row_scopes: ir
                .row_scopes
                .iter()
                .map(|scope| CompilerRowScope {
                    list: scope.list.clone(),
                    row_scope: scope.row_scope.clone(),
                })
                .collect(),
            source_paths,
            source_row_lookup_fields,
            source_address_lookup_fields,
        }
    }
}

pub fn compiler_runtime_symbols_from_ir(ir: &TypedProgram) -> CompilerRuntimeSymbols {
    let mut paths = CompilerRuntimeSymbolPaths::default();
    for source in &ir.sources {
        paths.intern(&source.path);
    }
    for cell in &ir.state_cells {
        paths.intern(&cell.path);
    }
    for list in &ir.lists {
        paths.intern(&list.name);
    }
    for value in &ir.derived_values {
        paths.intern(&value.path);
        for source in &value.sources {
            paths.intern(source);
        }
    }
    for branch in &ir.update_branches {
        paths.intern(&branch.target);
        paths.intern(&branch.source);
        match &branch.expression {
            boon_ir::UpdateExpression::SourcePayload { path }
            | boon_ir::UpdateExpression::Const { value: path }
            | boon_ir::UpdateExpression::PreviousValue { path }
            | boon_ir::UpdateExpression::ReadPath { path }
            | boon_ir::UpdateExpression::BoolNot { path }
            | boon_ir::UpdateExpression::BytesLength { path }
            | boon_ir::UpdateExpression::BytesIsEmpty { path }
            | boon_ir::UpdateExpression::BytesGet { path, .. }
            | boon_ir::UpdateExpression::BytesSet { path, .. }
            | boon_ir::UpdateExpression::BytesSlice { path, .. }
            | boon_ir::UpdateExpression::BytesTake { path, .. }
            | boon_ir::UpdateExpression::BytesDrop { path, .. }
            | boon_ir::UpdateExpression::BytesToHex { path }
            | boon_ir::UpdateExpression::BytesFromHex { path }
            | boon_ir::UpdateExpression::BytesToBase64 { path }
            | boon_ir::UpdateExpression::BytesFromBase64 { path }
            | boon_ir::UpdateExpression::BytesReadUnsigned { path, .. }
            | boon_ir::UpdateExpression::BytesReadSigned { path, .. }
            | boon_ir::UpdateExpression::BytesWriteUnsigned { path, .. }
            | boon_ir::UpdateExpression::BytesWriteSigned { path, .. }
            | boon_ir::UpdateExpression::TextToBytes { path, .. }
            | boon_ir::UpdateExpression::BytesToText { path, .. } => {
                paths.intern(path);
            }
            boon_ir::UpdateExpression::FileReadBytes { path } => {
                compiler_intern_file_bytes_path(&mut paths, path);
            }
            boon_ir::UpdateExpression::FileWriteBytes { bytes_path, path } => {
                paths.intern(bytes_path);
                compiler_intern_file_bytes_path(&mut paths, path);
            }
            boon_ir::UpdateExpression::BytesZeros { .. } => {}
            boon_ir::UpdateExpression::BytesConcat { left, right }
            | boon_ir::UpdateExpression::BytesEqual { left, right } => {
                paths.intern(left);
                paths.intern(right);
            }
            boon_ir::UpdateExpression::BytesFind { haystack, needle } => {
                paths.intern(haystack);
                paths.intern(needle);
            }
            boon_ir::UpdateExpression::BytesStartsWith { path, prefix } => {
                paths.intern(path);
                paths.intern(prefix);
            }
            boon_ir::UpdateExpression::BytesEndsWith { path, suffix } => {
                paths.intern(path);
                paths.intern(suffix);
            }
            boon_ir::UpdateExpression::TextTrimOrPrevious { path, previous } => {
                paths.intern(path);
                paths.intern(previous);
            }
            boon_ir::UpdateExpression::PrefixPayloadConcat {
                prefix,
                payload_path,
                separator,
            } => {
                paths.intern(prefix);
                paths.intern(payload_path);
                paths.intern(separator);
            }
            boon_ir::UpdateExpression::PrefixRootConcat {
                prefix,
                path,
                separator,
            } => {
                paths.intern(prefix);
                paths.intern(path);
                paths.intern(separator);
            }
            boon_ir::UpdateExpression::NumberInfix { left, right, .. } => {
                paths.intern(left);
                paths.intern(right);
            }
            boon_ir::UpdateExpression::ProjectTime {
                pointer_x,
                pointer_width,
                viewport_start,
                viewport_end,
                fallback,
            } => {
                paths.intern(pointer_x);
                paths.intern(pointer_width);
                paths.intern(viewport_start);
                paths.intern(viewport_end);
                paths.intern(fallback);
            }
            boon_ir::UpdateExpression::MatchNumberInfixConst {
                left, right, arms, ..
            } => {
                paths.intern(left);
                paths.intern(right);
                for arm in arms {
                    paths.intern(&arm.pattern);
                    compiler_intern_update_value_expression_symbols(&mut paths, &arm.output);
                }
            }
            boon_ir::UpdateExpression::MatchConst { input, arms } => {
                paths.intern(input);
                for arm in arms {
                    paths.intern(&arm.pattern);
                    paths.intern(&arm.output);
                }
            }
            boon_ir::UpdateExpression::MatchValueConst { input, arms }
            | boon_ir::UpdateExpression::MatchTextIsEmptyConst { input, arms } => {
                paths.intern(input);
                for arm in arms {
                    paths.intern(&arm.pattern);
                    compiler_intern_update_value_expression_symbols(&mut paths, &arm.output);
                }
            }
            boon_ir::UpdateExpression::ListFindValue {
                list,
                field,
                expected,
                target,
                fallback,
            } => {
                paths.intern(list);
                paths.intern(field);
                compiler_intern_update_value_expression_symbols(&mut paths, expected);
                paths.intern(target);
                if let Some(fallback) = fallback {
                    compiler_intern_update_value_expression_symbols(&mut paths, fallback);
                }
            }
            boon_ir::UpdateExpression::Unknown { summary } => {
                paths.intern(summary);
            }
        }
    }
    for operation in &ir.list_operations {
        paths.intern(&operation.list);
        match &operation.kind {
            boon_ir::ListOperationKind::Append { trigger, fields } => {
                paths.intern(trigger);
                for field in fields {
                    paths.intern(&field.name);
                    match &field.value {
                        boon_ir::ListAppendFieldValue::Source { path } => {
                            paths.intern(path);
                        }
                        boon_ir::ListAppendFieldValue::Const { value } => {
                            paths.intern(value);
                        }
                        boon_ir::ListAppendFieldValue::TypedConst { .. } => {}
                    }
                }
            }
            boon_ir::ListOperationKind::Remove { source, predicate } => {
                paths.intern(source);
                compiler_intern_list_predicate(&mut paths, predicate);
            }
            boon_ir::ListOperationKind::Retain { target, predicate }
            | boon_ir::ListOperationKind::Count { target, predicate } => {
                paths.intern(target);
                compiler_intern_list_predicate(&mut paths, predicate);
            }
        }
    }
    for projection in &ir.list_projections {
        paths.intern(&projection.target);
        paths.intern(&projection.list);
        if let boon_ir::ListProjectionKind::Find { field, value } = &projection.kind {
            paths.intern(field);
            paths.intern(value);
        }
    }
    CompilerRuntimeSymbols { paths: paths.paths }
}

pub fn compiler_scalar_equation_plan_from_ir(ir: &TypedProgram) -> CompilerScalarEquationPlan {
    let source_paths = ir
        .sources
        .iter()
        .map(|source| source.path.clone())
        .collect::<Vec<_>>();
    let branches = ir
        .update_branches
        .iter()
        .map(|branch| CompilerScalarUpdateBranch {
            target: branch.target.clone(),
            source: branch.source.clone(),
            guard: branch.guard.as_ref().map(compiler_update_guard),
            expression: compiler_scalar_update_expression(&branch.expression, &branch.source),
        })
        .collect();
    CompilerScalarEquationPlan {
        branches,
        source_paths,
    }
}

fn compiler_scalar_update_expression(
    expression: &boon_ir::UpdateExpression,
    source: &str,
) -> CompilerScalarUpdateExpression {
    match expression {
        boon_ir::UpdateExpression::SourcePayload { path } if path == "text" => {
            CompilerScalarUpdateExpression::SourceText
        }
        boon_ir::UpdateExpression::SourcePayload { path } if path == "key" => {
            CompilerScalarUpdateExpression::SourceKey
        }
        boon_ir::UpdateExpression::SourcePayload { path } if path == "address" => {
            CompilerScalarUpdateExpression::SourceAddress
        }
        boon_ir::UpdateExpression::SourcePayload { path } => {
            CompilerScalarUpdateExpression::SourcePayload(path.clone())
        }
        boon_ir::UpdateExpression::Const { value } => {
            CompilerScalarUpdateExpression::Const(value.clone())
        }
        boon_ir::UpdateExpression::NumberInfix { left, op, right } => {
            CompilerScalarUpdateExpression::NumberInfix {
                left: left.clone(),
                op: op.clone(),
                right: right.clone(),
            }
        }
        boon_ir::UpdateExpression::ProjectTime {
            pointer_x,
            pointer_width,
            viewport_start,
            viewport_end,
            fallback,
        } => CompilerScalarUpdateExpression::ProjectTime {
            pointer_x: pointer_x.clone(),
            pointer_width: pointer_width.clone(),
            viewport_start: viewport_start.clone(),
            viewport_end: viewport_end.clone(),
            fallback: fallback.clone(),
        },
        boon_ir::UpdateExpression::MatchNumberInfixConst {
            left,
            op,
            right,
            arms,
        } => CompilerScalarUpdateExpression::MatchNumberInfixConst {
            left: left.clone(),
            op: op.clone(),
            right: right.clone(),
            arms: compiler_update_value_match_arms(arms),
        },
        boon_ir::UpdateExpression::ListFindValue {
            list,
            field,
            expected,
            target,
            fallback,
        } => CompilerScalarUpdateExpression::ListFindValue {
            list: list.clone(),
            field: field.clone(),
            expected: Box::new(compiler_update_value_expression(expected)),
            target: target.clone(),
            fallback: fallback
                .as_deref()
                .map(compiler_update_value_expression)
                .map(Box::new),
        },
        boon_ir::UpdateExpression::PreviousValue { path } => {
            compiler_scalar_payload_or_path_expression(path, source, true)
        }
        boon_ir::UpdateExpression::ReadPath { path } => {
            compiler_scalar_payload_or_path_expression(path, source, false)
        }
        boon_ir::UpdateExpression::TextTrimOrPrevious { path, previous } => {
            CompilerScalarUpdateExpression::TextTrimOrPrevious {
                path: path.clone(),
                previous: previous.clone(),
            }
        }
        boon_ir::UpdateExpression::PrefixPayloadConcat {
            prefix,
            payload_path,
            separator,
        } => CompilerScalarUpdateExpression::PrefixPayloadConcat {
            prefix: prefix.clone(),
            payload_path: payload_path.clone(),
            separator: separator.clone(),
        },
        boon_ir::UpdateExpression::PrefixRootConcat {
            prefix,
            path,
            separator,
        } => CompilerScalarUpdateExpression::PrefixRootConcat {
            prefix: prefix.clone(),
            path: path.clone(),
            separator: separator.clone(),
        },
        boon_ir::UpdateExpression::BoolNot { path } => {
            CompilerScalarUpdateExpression::BoolNot(path.clone())
        }
        boon_ir::UpdateExpression::BytesLength { path } => {
            CompilerScalarUpdateExpression::BytesLength(path.clone())
        }
        boon_ir::UpdateExpression::BytesIsEmpty { path } => {
            CompilerScalarUpdateExpression::BytesIsEmpty(path.clone())
        }
        boon_ir::UpdateExpression::BytesGet { path, index } => {
            CompilerScalarUpdateExpression::BytesGet {
                path: path.clone(),
                index: *index,
            }
        }
        boon_ir::UpdateExpression::BytesSet { path, index, value } => {
            CompilerScalarUpdateExpression::BytesSet {
                path: path.clone(),
                index: *index,
                value: *value,
            }
        }
        boon_ir::UpdateExpression::BytesSlice {
            path,
            offset,
            byte_count,
        } => CompilerScalarUpdateExpression::BytesSlice {
            path: path.clone(),
            offset: compiler_bytes_scalar_arg(offset),
            byte_count: compiler_bytes_scalar_arg(byte_count),
        },
        boon_ir::UpdateExpression::BytesTake { path, byte_count } => {
            CompilerScalarUpdateExpression::BytesTake {
                path: path.clone(),
                byte_count: compiler_bytes_scalar_arg(byte_count),
            }
        }
        boon_ir::UpdateExpression::BytesDrop { path, byte_count } => {
            CompilerScalarUpdateExpression::BytesDrop {
                path: path.clone(),
                byte_count: compiler_bytes_scalar_arg(byte_count),
            }
        }
        boon_ir::UpdateExpression::BytesZeros { byte_count } => {
            CompilerScalarUpdateExpression::BytesZeros {
                byte_count: *byte_count,
            }
        }
        boon_ir::UpdateExpression::BytesToHex { path } => {
            CompilerScalarUpdateExpression::BytesToHex { path: path.clone() }
        }
        boon_ir::UpdateExpression::BytesFromHex { path } => {
            CompilerScalarUpdateExpression::BytesFromHex { path: path.clone() }
        }
        boon_ir::UpdateExpression::BytesToBase64 { path } => {
            CompilerScalarUpdateExpression::BytesToBase64 { path: path.clone() }
        }
        boon_ir::UpdateExpression::BytesFromBase64 { path } => {
            CompilerScalarUpdateExpression::BytesFromBase64 { path: path.clone() }
        }
        boon_ir::UpdateExpression::BytesReadUnsigned {
            path,
            offset,
            byte_count,
            endian,
        } => CompilerScalarUpdateExpression::BytesReadUnsigned {
            path: path.clone(),
            offset: *offset,
            byte_count: *byte_count,
            endian: endian.clone(),
        },
        boon_ir::UpdateExpression::BytesReadSigned {
            path,
            offset,
            byte_count,
            endian,
        } => CompilerScalarUpdateExpression::BytesReadSigned {
            path: path.clone(),
            offset: *offset,
            byte_count: *byte_count,
            endian: endian.clone(),
        },
        boon_ir::UpdateExpression::BytesWriteUnsigned {
            path,
            offset,
            byte_count,
            endian,
            value,
        } => CompilerScalarUpdateExpression::BytesWriteUnsigned {
            path: path.clone(),
            offset: *offset,
            byte_count: *byte_count,
            endian: endian.clone(),
            value: *value,
        },
        boon_ir::UpdateExpression::BytesWriteSigned {
            path,
            offset,
            byte_count,
            endian,
            value,
        } => CompilerScalarUpdateExpression::BytesWriteSigned {
            path: path.clone(),
            offset: *offset,
            byte_count: *byte_count,
            endian: endian.clone(),
            value: *value,
        },
        boon_ir::UpdateExpression::FileReadBytes { path } => {
            CompilerScalarUpdateExpression::FileReadBytes {
                path: match path {
                    boon_ir::FileBytesPath::StaticText(path)
                    | boon_ir::FileBytesPath::StatePath(path) => path.clone(),
                },
            }
        }
        boon_ir::UpdateExpression::TextToBytes { path, encoding } => {
            CompilerScalarUpdateExpression::TextToBytes {
                path: path.clone(),
                encoding: encoding.clone(),
            }
        }
        boon_ir::UpdateExpression::BytesToText { path, encoding } => {
            CompilerScalarUpdateExpression::BytesToText {
                path: path.clone(),
                encoding: encoding.clone(),
            }
        }
        boon_ir::UpdateExpression::BytesConcat { left, right } => {
            CompilerScalarUpdateExpression::BytesConcat {
                left: left.clone(),
                right: right.clone(),
            }
        }
        boon_ir::UpdateExpression::BytesEqual { left, right } => {
            CompilerScalarUpdateExpression::BytesEqual {
                left: left.clone(),
                right: right.clone(),
            }
        }
        boon_ir::UpdateExpression::BytesFind { haystack, needle } => {
            CompilerScalarUpdateExpression::BytesFind {
                haystack: haystack.clone(),
                needle: needle.clone(),
            }
        }
        boon_ir::UpdateExpression::BytesStartsWith { path, prefix } => {
            CompilerScalarUpdateExpression::BytesStartsWith {
                path: path.clone(),
                prefix: prefix.clone(),
            }
        }
        boon_ir::UpdateExpression::BytesEndsWith { path, suffix } => {
            CompilerScalarUpdateExpression::BytesEndsWith {
                path: path.clone(),
                suffix: suffix.clone(),
            }
        }
        boon_ir::UpdateExpression::MatchConst { input, arms } => {
            CompilerScalarUpdateExpression::MatchConst {
                input: input.clone(),
                arms: compiler_update_match_arms(arms),
            }
        }
        boon_ir::UpdateExpression::MatchValueConst { input, arms } => {
            CompilerScalarUpdateExpression::MatchValueConst {
                input: input.clone(),
                arms: compiler_update_value_match_arms(arms),
            }
        }
        boon_ir::UpdateExpression::MatchTextIsEmptyConst { input, arms } => {
            CompilerScalarUpdateExpression::MatchTextIsEmptyConst {
                input: input.clone(),
                arms: compiler_update_value_match_arms(arms),
            }
        }
        _ => CompilerScalarUpdateExpression::Unsupported,
    }
}

fn compiler_scalar_payload_or_path_expression(
    path: &str,
    source: &str,
    previous: bool,
) -> CompilerScalarUpdateExpression {
    match compiler_source_payload_field_from_input(path, source).as_deref() {
        Some("text") => CompilerScalarUpdateExpression::SourceText,
        Some("key") => CompilerScalarUpdateExpression::SourceKey,
        Some("address") => CompilerScalarUpdateExpression::SourceAddress,
        Some(field) => CompilerScalarUpdateExpression::SourcePayload(field.to_owned()),
        None if previous => CompilerScalarUpdateExpression::PreviousValue(path.to_owned()),
        None => CompilerScalarUpdateExpression::ReadPath(path.to_owned()),
    }
}

#[derive(Default)]
struct CompilerRuntimeSymbolPaths {
    paths: Vec<String>,
    seen: BTreeSet<String>,
}

impl CompilerRuntimeSymbolPaths {
    fn intern(&mut self, path: &str) {
        if self.seen.insert(path.to_owned()) {
            self.paths.push(path.to_owned());
        }
    }
}

fn compiler_intern_file_bytes_path(
    paths: &mut CompilerRuntimeSymbolPaths,
    path: &boon_ir::FileBytesPath,
) {
    match path {
        boon_ir::FileBytesPath::StaticText(path) | boon_ir::FileBytesPath::StatePath(path) => {
            paths.intern(path);
        }
    }
}

fn compiler_intern_list_predicate(
    paths: &mut CompilerRuntimeSymbolPaths,
    predicate: &boon_ir::ListPredicate,
) {
    match predicate {
        boon_ir::ListPredicate::RowFieldBool { path }
        | boon_ir::ListPredicate::RowFieldBoolNot { path } => {
            paths.intern(path);
        }
        boon_ir::ListPredicate::SelectedFilterVisibility {
            selector,
            row_field,
        } => {
            paths.intern(selector);
            paths.intern(row_field);
        }
        boon_ir::ListPredicate::AlwaysTrue | boon_ir::ListPredicate::Unknown { .. } => {}
    }
}

fn compiler_intern_update_value_expression_symbols(
    paths: &mut CompilerRuntimeSymbolPaths,
    value: &boon_ir::UpdateValueExpression,
) {
    match value {
        boon_ir::UpdateValueExpression::Const { value } => {
            paths.intern(value);
        }
        boon_ir::UpdateValueExpression::ReadPath { path } => {
            paths.intern(path);
        }
        boon_ir::UpdateValueExpression::MatchConst { input, arms } => {
            paths.intern(input);
            for arm in arms {
                paths.intern(&arm.pattern);
                compiler_intern_update_value_expression_symbols(paths, &arm.output);
            }
        }
        boon_ir::UpdateValueExpression::NumberInfix { left, right, .. } => {
            paths.intern(left);
            paths.intern(right);
        }
        boon_ir::UpdateValueExpression::MatchNumberInfixConst {
            left, right, arms, ..
        } => {
            paths.intern(left);
            paths.intern(right);
            for arm in arms {
                paths.intern(&arm.pattern);
                compiler_intern_update_value_expression_symbols(paths, &arm.output);
            }
        }
    }
}

pub fn compiler_field_slot_collision_diagnostics_from_ir(
    ir: &TypedProgram,
) -> Vec<CompilerFieldSlotCollisionDiagnostic> {
    let mut names = Vec::new();
    for cell in &ir.state_cells {
        names.push(compiler_row_field_name(&cell.path).to_owned());
    }
    for value in &ir.derived_values {
        names.push(compiler_row_field_name(&value.path).to_owned());
    }
    for list in &ir.lists {
        match &list.initializer {
            boon_ir::ListInitializer::RecordLiteral { rows } => {
                for row in rows {
                    for field in &row.fields {
                        names.push(compiler_row_field_name(&field.name).to_owned());
                    }
                }
            }
            boon_ir::ListInitializer::Range { .. } => {
                names.push("index".to_owned());
                names.push("value".to_owned());
            }
            boon_ir::ListInitializer::Empty | boon_ir::ListInitializer::Unknown { .. } => {}
        }
    }
    for operation in &ir.list_operations {
        if let boon_ir::ListOperationKind::Append { fields, .. } = &operation.kind {
            for field in fields {
                names.push(compiler_row_field_name(&field.name).to_owned());
            }
        }
    }
    compiler_field_slot_collision_diagnostics_from_names(names)
}

fn compiler_field_slot_collision_diagnostics_from_names(
    names: impl IntoIterator<Item = String>,
) -> Vec<CompilerFieldSlotCollisionDiagnostic> {
    let mut by_id = BTreeMap::<usize, BTreeSet<String>>::new();
    for name in names {
        by_id
            .entry(compiler_stable_runtime_field_id(&name))
            .or_default()
            .insert(name);
    }
    by_id
        .into_iter()
        .filter_map(|(field_id, labels)| {
            (labels.len() > 1).then(|| CompilerFieldSlotCollisionDiagnostic {
                field_id,
                labels: labels.into_iter().collect(),
            })
        })
        .collect()
}

fn compiler_stable_runtime_field_id(name: &str) -> usize {
    const OFFSET: usize = 10_000;
    let mut hash = 0xcbf29ce484222325u64;
    for byte in name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    OFFSET + (hash as usize & 0x000f_ffff)
}

pub fn compiler_typed_storage_layout_counts_from_ir(
    ir: &TypedProgram,
) -> CompilerTypedStorageLayoutCounts {
    let mut counts = CompilerTypedStorageLayoutCounts {
        list_memory_count: ir.lists.len(),
        list_hidden_key_slot_count: ir.lists.len(),
        list_hidden_generation_slot_count: ir.lists.len(),
        ..CompilerTypedStorageLayoutCounts::default()
    };
    for cell in &ir.state_cells {
        match (cell.indexed, &cell.initial_value) {
            (false, boon_ir::InitialValue::Bool { .. }) => counts.root_bool_slot_count += 1,
            (false, boon_ir::InitialValue::Enum { .. }) => counts.root_enum_slot_count += 1,
            (
                false,
                boon_ir::InitialValue::Text { .. }
                | boon_ir::InitialValue::Number { .. }
                | boon_ir::InitialValue::Byte { .. }
                | boon_ir::InitialValue::Bytes { .. }
                | boon_ir::InitialValue::RootInitialField { .. }
                | boon_ir::InitialValue::RowInitialField { .. },
            ) => {
                counts.root_text_slot_count += 1;
            }
            (false, boon_ir::InitialValue::Unknown { .. }) => {}
            (true, boon_ir::InitialValue::Bool { .. }) => {
                counts.list_row_template_field_count += 1;
                counts.list_row_bool_slot_count += 1;
            }
            (true, boon_ir::InitialValue::Enum { .. }) => {
                counts.list_row_template_field_count += 1;
                counts.list_row_enum_slot_count += 1;
            }
            (
                true,
                boon_ir::InitialValue::Text { .. }
                | boon_ir::InitialValue::Number { .. }
                | boon_ir::InitialValue::Byte { .. }
                | boon_ir::InitialValue::Bytes { .. }
                | boon_ir::InitialValue::RootInitialField { .. }
                | boon_ir::InitialValue::RowInitialField { .. },
            ) => {
                counts.list_row_template_field_count += 1;
                counts.list_row_text_slot_count += 1;
            }
            (true, boon_ir::InitialValue::Unknown { .. }) => {}
        }
    }
    counts.list_row_text_slot_count += ir
        .derived_values
        .iter()
        .filter(|value| value.indexed)
        .count();
    counts
}

pub fn compiler_list_source_bindings_from_ir(
    ir: &TypedProgram,
) -> Vec<CompilerListSourceBindingSlot> {
    let mut list_slots = Vec::new();
    for list in &ir.lists {
        let Some(scope_id) = list.row_scope_id else {
            continue;
        };
        let Some(row_scope) = ir.row_scopes.iter().find(|scope| scope.id == scope_id) else {
            continue;
        };
        let row_scope = row_scope.row_scope.clone();
        let prefix = format!("{row_scope}.");
        let source_paths = ir
            .sources
            .iter()
            .filter(|source| source.scoped && source.path.starts_with(&prefix))
            .map(|source| source.path.clone())
            .collect::<Vec<_>>();
        if !source_paths.is_empty() {
            list_slots.push(CompilerListSourceBindingSlot {
                list: list.name.clone(),
                row_scope,
                source_paths,
            });
        }
    }
    list_slots
}

pub fn compiler_root_state_paths_from_ir(ir: &TypedProgram) -> Vec<String> {
    let mut root_state_paths = ir
        .state_cells
        .iter()
        .filter(|cell| !cell.indexed)
        .map(|cell| cell.path.clone())
        .collect::<Vec<_>>();
    root_state_paths.extend(
        ir.derived_values
            .iter()
            .filter(|value| {
                !value.indexed
                    && value.scope_id.is_none()
                    && (value.kind == boon_ir::DerivedValueKind::SourceEventTransform
                        || (value.kind == boon_ir::DerivedValueKind::Pure
                            && compiler_statement_contains_latest(
                                &value.statement,
                                &ir.expressions,
                            )))
            })
            .map(|value| value.path.clone()),
    );
    root_state_paths.sort();
    root_state_paths.dedup();
    root_state_paths
}

pub fn compiler_list_operations_from_ir(ir: &TypedProgram) -> Vec<CompilerListOperation> {
    ir.list_operations
        .iter()
        .map(|operation| {
            let list = operation.list.clone();
            let kind = match &operation.kind {
                boon_ir::ListOperationKind::Append { trigger, fields } => {
                    CompilerListOperationKind::Append {
                        trigger: trigger.clone(),
                        fields: fields
                            .iter()
                            .map(|field| CompilerListAppendField {
                                name: field.name.clone(),
                                value: compiler_list_append_field_value(&field.value),
                            })
                            .collect(),
                    }
                }
                boon_ir::ListOperationKind::Remove { source, predicate } => {
                    CompilerListOperationKind::Remove {
                        source: source.clone(),
                        predicate: compiler_list_predicate(predicate),
                    }
                }
                boon_ir::ListOperationKind::Retain { target, predicate } => {
                    CompilerListOperationKind::Retain {
                        target: target.clone(),
                        predicate: compiler_list_predicate(predicate),
                    }
                }
                boon_ir::ListOperationKind::Count { target, predicate } => {
                    CompilerListOperationKind::Count {
                        target: target.clone(),
                        predicate: compiler_list_predicate(predicate),
                    }
                }
            };
            CompilerListOperation { list, kind }
        })
        .collect()
}

pub fn compiler_storage_root_slots_from_ir(ir: &TypedProgram) -> Vec<CompilerStorageRootSlot> {
    ir.state_cells
        .iter()
        .filter(|cell| !cell.indexed)
        .map(|cell| CompilerStorageRootSlot {
            path: cell.path.clone(),
            initializer: compiler_initial_value(&cell.initial_value),
        })
        .collect()
}

pub fn compiler_storage_indexed_row_initial_resets_from_ir(
    ir: &TypedProgram,
) -> Vec<CompilerStorageIndexedRowInitialReset> {
    ir.state_cells
        .iter()
        .filter(|cell| cell.indexed)
        .filter_map(|cell| {
            let boon_ir::InitialValue::RowInitialField { path } = &cell.initial_value else {
                return None;
            };
            let scope_id = cell.scope_id?;
            let row_scope = ir.row_scopes.iter().find(|scope| scope.id == scope_id)?;
            let target_field = cell
                .path
                .strip_prefix(&format!("{}.", row_scope.row_scope))?
                .to_owned();
            let source_field = if path == &target_field {
                compiler_base_row_field_name(&target_field)
            } else {
                path.clone()
            };
            Some(CompilerStorageIndexedRowInitialReset {
                list: row_scope.list.clone(),
                target_field,
                source_field,
                original_source_path: path.clone(),
            })
        })
        .collect()
}

pub fn compiler_storage_list_slots_from_ir(ir: &TypedProgram) -> Vec<CompilerStorageListSlot> {
    let mut slots = Vec::new();
    let mut list_names = BTreeSet::new();
    for list in &ir.lists {
        let row_scope = compiler_row_scope_name_for_ir_list(ir, list);
        let initializer_kind = match &list.initializer {
            boon_ir::ListInitializer::RecordLiteral { rows }
                if compiler_ir_list_has_derived_list_view(ir, &list.name)
                    && compiler_list_initial_records_have_dynamic_fields(rows) =>
            {
                CompilerStorageListInitializerKind::DeferredDynamicListView
            }
            boon_ir::ListInitializer::RecordLiteral { .. } => {
                CompilerStorageListInitializerKind::RecordLiteral
            }
            boon_ir::ListInitializer::Range { from, to } => {
                CompilerStorageListInitializerKind::Range {
                    from: *from,
                    to: *to,
                }
            }
            boon_ir::ListInitializer::Empty => CompilerStorageListInitializerKind::Empty,
            boon_ir::ListInitializer::Unknown { summary } => {
                if compiler_ir_list_has_derived_list_view(ir, &list.name) {
                    CompilerStorageListInitializerKind::DeferredDynamicListView
                } else {
                    CompilerStorageListInitializerKind::Unsupported {
                        summary: summary.clone(),
                    }
                }
            }
        };
        list_names.insert(list.name.clone());
        slots.push(CompilerStorageListSlot {
            id: list.id.0,
            name: list.name.clone(),
            capacity: list.capacity,
            row_scope,
            synthetic_list_view_storage: false,
            initializer_kind,
        });
    }

    let mut synthetic_list_id = ir
        .lists
        .iter()
        .map(|list| list.id.0)
        .max()
        .map_or(0, |id| id.saturating_add(1));
    for value in ir.derived_values.iter().filter(|value| {
        !value.indexed
            && value.scope_id.is_none()
            && matches!(value.kind, boon_ir::DerivedValueKind::ListView)
    }) {
        let name = compiler_derived_root_list_storage_name(&value.path);
        if list_names.contains(&name) {
            continue;
        }
        list_names.insert(name.clone());
        slots.push(CompilerStorageListSlot {
            id: synthetic_list_id,
            name,
            capacity: None,
            row_scope: String::new(),
            synthetic_list_view_storage: true,
            initializer_kind: CompilerStorageListInitializerKind::SyntheticListViewStorage,
        });
        synthetic_list_id = synthetic_list_id.saturating_add(1);
    }
    slots
}

pub fn compiler_storage_row_templates_from_ir(
    ir: &TypedProgram,
) -> Vec<CompilerStorageRowTemplate> {
    ir.lists
        .iter()
        .map(|list| {
            let row_scope = compiler_row_scope_name_for_ir_list(ir, list);
            let prefix = format!("{row_scope}.");
            let fields = ir
                .state_cells
                .iter()
                .filter(|cell| cell.indexed && cell.path.starts_with(&prefix))
                .filter_map(|cell| {
                    let field_name = cell.path.strip_prefix(&prefix)?.to_owned();
                    Some(CompilerStorageRowFieldTemplate {
                        field_name,
                        initial_value: compiler_initial_value(&cell.initial_value),
                        missing_row_initial_value: compiler_missing_row_initial_value(cell, ir),
                    })
                })
                .collect();
            CompilerStorageRowTemplate {
                list: list.name.clone(),
                row_scope,
                fields,
            }
        })
        .collect()
}

pub fn compiler_storage_initial_rows_from_ir(ir: &TypedProgram) -> Vec<CompilerStorageInitialRows> {
    ir.lists
        .iter()
        .map(|list| {
            let rows = match &list.initializer {
                boon_ir::ListInitializer::RecordLiteral { rows } => rows
                    .iter()
                    .map(|row| CompilerStorageInitialRow {
                        fields: row
                            .fields
                            .iter()
                            .map(|field| CompilerStorageInitialField {
                                name: field.name.clone(),
                                value: compiler_initial_value(&field.value),
                            })
                            .collect(),
                    })
                    .collect(),
                boon_ir::ListInitializer::Range { .. }
                | boon_ir::ListInitializer::Empty
                | boon_ir::ListInitializer::Unknown { .. } => Vec::new(),
            };
            CompilerStorageInitialRows {
                list: list.name.clone(),
                rows,
            }
        })
        .collect()
}

pub fn compiler_storage_indexed_derived_fields_from_ir(
    ir: &TypedProgram,
) -> Vec<CompilerStorageIndexedDerivedFields> {
    ir.lists
        .iter()
        .map(|list| {
            let row_scope = compiler_row_scope_name_for_ir_list(ir, list);
            let prefix = format!("{row_scope}.");
            let fields = ir
                .derived_values
                .iter()
                .filter(|value| value.indexed)
                .filter_map(|value| value.path.strip_prefix(&prefix).map(str::to_owned))
                .collect();
            CompilerStorageIndexedDerivedFields {
                list: list.name.clone(),
                row_scope,
                fields,
            }
        })
        .collect()
}

pub fn compiler_source_route_root_targets_from_ir(ir: &TypedProgram) -> BTreeSet<String> {
    ir.state_cells
        .iter()
        .filter(|cell| !cell.indexed)
        .map(|cell| cell.path.clone())
        .collect()
}

pub fn compiler_source_route_sources_from_ir(ir: &TypedProgram) -> Vec<CompilerSourceRouteSource> {
    ir.sources
        .iter()
        .map(|source| CompilerSourceRouteSource {
            path: source.path.clone(),
            source_id: source.id.0,
            payload_fields: source
                .payload_schema
                .fields
                .iter()
                .map(compiler_source_payload_field)
                .collect(),
            row_lookup_field: source
                .payload_schema
                .row_lookup_field
                .clone()
                .or_else(|| source.payload_schema.address_lookup_field.clone()),
            address_lookup_field: source.payload_schema.address_lookup_field.clone(),
        })
        .collect()
}

pub fn compiler_source_route_bool_facts_from_ir(ir: &TypedProgram) -> CompilerSourceRouteBoolFacts {
    let mut scalar_targets = BTreeSet::new();
    let mut read_paths = Vec::new();
    for branch in &ir.update_branches {
        if compiler_ir_scalar_target_is_bool(ir, &branch.target) {
            scalar_targets.insert(branch.target.clone());
        }
        if let boon_ir::UpdateExpression::ReadPath { path } = &branch.expression {
            if compiler_ir_read_path_is_bool_for_target(ir, &branch.target, path) {
                read_paths.push(CompilerSourceRouteBoolReadPath {
                    target: branch.target.clone(),
                    path: path.clone(),
                });
            }
        }
    }
    CompilerSourceRouteBoolFacts {
        scalar_targets,
        read_paths,
    }
}

pub fn compiler_source_route_router_targets_from_ir(
    ir: &TypedProgram,
) -> Vec<CompilerSourceRouteRouterRoute> {
    let mut targets = Vec::new();
    for value in &ir.derived_values {
        if value.indexed || value.kind != boon_ir::DerivedValueKind::SourceEventTransform {
            continue;
        }
        let exprs = compiler_statement_ast_exprs(&value.statement, &ir.expressions);
        if !compiler_statement_calls_router_go_to(&exprs) {
            continue;
        }
        for source in &value.sources {
            let Some(path) = compiler_source_then_text_value(&exprs, source) else {
                continue;
            };
            targets.push(CompilerSourceRouteRouterRoute {
                source: source.clone(),
                target: value.path.clone(),
                path,
            });
        }
    }
    targets
}

pub fn compiler_source_route_root_text_transform_targets_from_ir(
    ir: &TypedProgram,
) -> Vec<CompilerSourceRouteRootTextTransform> {
    let mut targets = Vec::new();
    let append_triggers = ir
        .list_operations
        .iter()
        .filter_map(|operation| match &operation.kind {
            boon_ir::ListOperationKind::Append { trigger, .. } => Some(trigger.as_str()),
            boon_ir::ListOperationKind::Remove { .. }
            | boon_ir::ListOperationKind::Retain { .. }
            | boon_ir::ListOperationKind::Count { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    let statically_scheduled_root_updates = ir
        .update_branches
        .iter()
        .filter(|branch| !branch.indexed)
        .map(|branch| (branch.target.as_str(), branch.source.as_str()))
        .collect::<BTreeSet<_>>();

    for value in &ir.derived_values {
        if value.indexed || value.kind != boon_ir::DerivedValueKind::SourceEventTransform {
            continue;
        }
        if append_triggers.contains(value.path.as_str()) {
            continue;
        }
        let exprs = compiler_statement_ast_exprs(&value.statement, &ir.expressions);
        if compiler_statement_calls_router_go_to(&exprs) {
            continue;
        }
        for source in &value.sources {
            if compiler_source_is_scoped(ir, source) {
                continue;
            }
            if statically_scheduled_root_updates.contains(&(value.path.as_str(), source.as_str())) {
                continue;
            }
            if compiler_source_payload_when_match_const_expression(&exprs, source) {
                continue;
            }
            if compiler_source_then_inline_match_const_expression(&value.path, &exprs, source) {
                continue;
            }
            if compiler_source_then_prefix_payload_concat_expression(&exprs, source) {
                continue;
            }
            if compiler_source_then_prefix_root_concat_expression(&value.path, &exprs, source) {
                continue;
            }
            let Some(output) = compiler_source_then_field_value(&exprs, source) else {
                continue;
            };
            targets.push(CompilerSourceRouteRootTextTransform {
                source: source.clone(),
                target: value.path.clone(),
                value: output,
            });
        }
    }
    targets
}

pub fn compiler_derived_equation_plan_from_ir(ir: &TypedProgram) -> CompilerDerivedEquationPlan {
    let append_triggers = ir
        .list_operations
        .iter()
        .filter_map(|operation| match &operation.kind {
            boon_ir::ListOperationKind::Append { trigger, .. } => Some(trigger.as_str()),
            boon_ir::ListOperationKind::Remove { .. }
            | boon_ir::ListOperationKind::Retain { .. }
            | boon_ir::ListOperationKind::Count { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    let append_source_paths = ir
        .list_operations
        .iter()
        .filter_map(|operation| match &operation.kind {
            boon_ir::ListOperationKind::Append { trigger, fields } => Some(
                std::iter::once(trigger.as_str())
                    .chain(fields.iter().filter_map(|field| match &field.value {
                        boon_ir::ListAppendFieldValue::Source { path } => Some(path.as_str()),
                        boon_ir::ListAppendFieldValue::Const { .. }
                        | boon_ir::ListAppendFieldValue::TypedConst { .. } => None,
                    }))
                    .collect::<Vec<_>>(),
            ),
            boon_ir::ListOperationKind::Remove { .. }
            | boon_ir::ListOperationKind::Retain { .. }
            | boon_ir::ListOperationKind::Count { .. } => None,
        })
        .flatten()
        .collect::<BTreeSet<_>>();
    let text_transforms = ir
        .derived_values
        .iter()
        .filter(|value| {
            value.kind == boon_ir::DerivedValueKind::SourceEventTransform
                && (!value.indexed || append_triggers.contains(value.path.as_str()))
        })
        .filter_map(|value| {
            let transforms = value
                .sources
                .iter()
                .filter_map(|source| {
                    let exprs = compiler_statement_ast_exprs(&value.statement, &ir.expressions);
                    let expression = compiler_source_event_transform_text_expression(
                        value,
                        source,
                        &ir.expressions,
                        &ir.functions,
                    );
                    if !value.indexed
                        && !append_source_paths.contains(value.path.as_str())
                        && !compiler_source_is_scoped(ir, source)
                        && matches!(expression, CompilerDerivedTextExpression::Const { .. })
                        && compiler_source_then_field_value(&exprs, source).is_some()
                    {
                        return None;
                    }
                    if !matches!(expression, CompilerDerivedTextExpression::Unsupported) {
                        Some(CompilerDerivedTextTransform {
                            target: value.path.clone(),
                            source: source.clone(),
                            expression,
                        })
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            (!transforms.is_empty()).then_some(transforms)
        })
        .flatten()
        .collect();
    CompilerDerivedEquationPlan { text_transforms }
}

fn compiler_source_event_transform_text_expression(
    value: &boon_ir::DerivedValue,
    source: &str,
    expressions: &[AstExpr],
    functions: &[boon_ir::FunctionDefinition],
) -> CompilerDerivedTextExpression {
    let exprs = compiler_statement_ast_exprs(&value.statement, expressions);
    let Some(path) = compiler_text_trim_input_path_from_exprs(&exprs) else {
        if let Some(expression) =
            compiler_source_payload_when_match_const_text_expression(&exprs, source)
        {
            return expression;
        }
        if let Some(expression) = compiler_source_then_match_const_text_expression(
            &value.path,
            &exprs,
            source,
            expressions,
            functions,
        ) {
            return expression;
        }
        if let Some(expression) =
            compiler_source_then_inline_match_const_text_expression(&value.path, &exprs, source)
        {
            return expression;
        }
        if let Some(expression) =
            compiler_source_then_prefix_payload_concat_text_expression(&exprs, source)
        {
            return expression;
        }
        if let Some(expression) =
            compiler_source_then_prefix_root_concat_text_expression(&value.path, &exprs, source)
        {
            return expression;
        }
        if let Some(expression) =
            compiler_source_then_list_find_value_text_expression(&value.path, &exprs, source)
        {
            return expression;
        }
        if let Some(value) = compiler_source_then_const_text_value(&exprs, source) {
            return CompilerDerivedTextExpression::Const { value };
        }
        let Some(path) = compiler_source_then_text_value(&exprs, source)
            .or_else(|| compiler_source_direct_text_value(&exprs, source))
        else {
            return CompilerDerivedTextExpression::Unsupported;
        };
        return CompilerDerivedTextExpression::SourceRootText {
            path: compiler_canonical_transform_text_path(&value.path, &path),
        };
    };
    if path == "text"
        || path.ends_with(".text")
        || value
            .sources
            .iter()
            .any(|source| path == format!("{source}.text"))
    {
        return CompilerDerivedTextExpression::EnterKeyPayloadTextTrimNonEmpty;
    }
    CompilerDerivedTextExpression::EnterKeyRootTextTrimNonEmpty {
        path: compiler_canonical_transform_text_path(&value.path, &path),
    }
}

fn compiler_source_payload_field(
    field: &boon_ir::SourcePayloadField,
) -> CompilerSourcePayloadField {
    match field {
        boon_ir::SourcePayloadField::Address => CompilerSourcePayloadField::Address,
        boon_ir::SourcePayloadField::Bytes => CompilerSourcePayloadField::Bytes,
        boon_ir::SourcePayloadField::Key => CompilerSourcePayloadField::Key,
        boon_ir::SourcePayloadField::Named(name) => CompilerSourcePayloadField::Named(name.clone()),
        boon_ir::SourcePayloadField::Text => CompilerSourcePayloadField::Text,
    }
}

fn compiler_update_guard(guard: &boon_ir::UpdateGuard) -> CompilerUpdateGuard {
    match guard {
        boon_ir::UpdateGuard::SourcePayloadOneOf { field, values } => {
            CompilerUpdateGuard::SourcePayloadOneOf {
                field: compiler_source_payload_field(field),
                values: values.clone(),
            }
        }
    }
}

fn compiler_update_match_arm(arm: &boon_ir::UpdateMatchArm) -> CompilerUpdateMatchArm {
    CompilerUpdateMatchArm {
        pattern: arm.pattern.clone(),
        output: arm.output.clone(),
    }
}

fn compiler_update_match_arms(arms: &[boon_ir::UpdateMatchArm]) -> Vec<CompilerUpdateMatchArm> {
    arms.iter().map(compiler_update_match_arm).collect()
}

fn compiler_update_value_match_arm(
    arm: &boon_ir::UpdateValueMatchArm,
) -> CompilerUpdateValueMatchArm {
    CompilerUpdateValueMatchArm {
        pattern: arm.pattern.clone(),
        output: compiler_update_value_expression(&arm.output),
    }
}

fn compiler_update_value_match_arms(
    arms: &[boon_ir::UpdateValueMatchArm],
) -> Vec<CompilerUpdateValueMatchArm> {
    arms.iter().map(compiler_update_value_match_arm).collect()
}

fn compiler_update_value_expression(
    value: &boon_ir::UpdateValueExpression,
) -> CompilerUpdateValueExpression {
    match value {
        boon_ir::UpdateValueExpression::Const { value } => CompilerUpdateValueExpression::Const {
            value: value.clone(),
        },
        boon_ir::UpdateValueExpression::ReadPath { path } => {
            CompilerUpdateValueExpression::ReadPath { path: path.clone() }
        }
        boon_ir::UpdateValueExpression::MatchConst { input, arms } => {
            CompilerUpdateValueExpression::MatchConst {
                input: input.clone(),
                arms: compiler_update_value_match_arms(arms),
            }
        }
        boon_ir::UpdateValueExpression::NumberInfix { left, op, right } => {
            CompilerUpdateValueExpression::NumberInfix {
                left: left.clone(),
                op: op.clone(),
                right: right.clone(),
            }
        }
        boon_ir::UpdateValueExpression::MatchNumberInfixConst {
            left,
            op,
            right,
            arms,
        } => CompilerUpdateValueExpression::MatchNumberInfixConst {
            left: left.clone(),
            op: op.clone(),
            right: right.clone(),
            arms: compiler_update_value_match_arms(arms),
        },
    }
}

fn compiler_bytes_scalar_arg(arg: &boon_ir::BytesScalarArg) -> CompilerBytesScalarArg {
    match arg {
        boon_ir::BytesScalarArg::Static(value) => CompilerBytesScalarArg::Static(*value),
        boon_ir::BytesScalarArg::Path(path) => CompilerBytesScalarArg::Path(path.clone()),
    }
}

fn compiler_list_append_field_value(
    value: &boon_ir::ListAppendFieldValue,
) -> CompilerListAppendFieldValue {
    match value {
        boon_ir::ListAppendFieldValue::Source { path } => {
            CompilerListAppendFieldValue::Source { path: path.clone() }
        }
        boon_ir::ListAppendFieldValue::Const { value } => CompilerListAppendFieldValue::Const {
            value: value.clone(),
        },
        boon_ir::ListAppendFieldValue::TypedConst { value } => {
            CompilerListAppendFieldValue::TypedConst {
                value: compiler_initial_value(value),
            }
        }
    }
}

fn compiler_list_predicate(predicate: &boon_ir::ListPredicate) -> CompilerListPredicate {
    match predicate {
        boon_ir::ListPredicate::AlwaysTrue => CompilerListPredicate::AlwaysTrue,
        boon_ir::ListPredicate::RowFieldBool { path } => {
            CompilerListPredicate::FieldBool { path: path.clone() }
        }
        boon_ir::ListPredicate::RowFieldBoolNot { path } => {
            CompilerListPredicate::FieldBoolNot { path: path.clone() }
        }
        boon_ir::ListPredicate::SelectedFilterVisibility {
            selector,
            row_field,
        } => CompilerListPredicate::SelectorVisibility {
            selector: selector.clone(),
            row_field: row_field.clone(),
        },
        boon_ir::ListPredicate::Unknown { .. } => CompilerListPredicate::Unsupported,
    }
}

fn compiler_initial_value(value: &boon_ir::InitialValue) -> CompilerInitialValue {
    match value {
        boon_ir::InitialValue::Text { value } => CompilerInitialValue::Text(value.clone()),
        boon_ir::InitialValue::Number { value } => CompilerInitialValue::Number(*value),
        boon_ir::InitialValue::Byte { value } => CompilerInitialValue::Byte(*value),
        boon_ir::InitialValue::Bool { value } => CompilerInitialValue::Bool(*value),
        boon_ir::InitialValue::Bytes { bytes, .. } => CompilerInitialValue::Bytes(bytes.clone()),
        boon_ir::InitialValue::Enum { value } => CompilerInitialValue::Enum(value.clone()),
        boon_ir::InitialValue::RootInitialField { path } => {
            CompilerInitialValue::RootInitialField { path: path.clone() }
        }
        boon_ir::InitialValue::RowInitialField { path } => {
            CompilerInitialValue::RowInitialField { path: path.clone() }
        }
        boon_ir::InitialValue::Unknown { summary } => CompilerInitialValue::Unknown {
            summary: summary.clone(),
        },
    }
}

fn compiler_missing_row_initial_value(
    cell: &boon_ir::StateCell,
    ir: &TypedProgram,
) -> Option<CompilerFieldValue> {
    let boon_ir::InitialValue::RowInitialField { .. } = &cell.initial_value else {
        return None;
    };
    if ir.update_branches.iter().any(|branch| {
        branch.target == cell.path
            && matches!(branch.expression, boon_ir::UpdateExpression::BoolNot { .. })
    }) {
        Some(CompilerFieldValue::Bool(false))
    } else {
        Some(CompilerFieldValue::Text(String::new()))
    }
}

fn compiler_ir_read_path_is_bool_for_target(ir: &TypedProgram, target: &str, path: &str) -> bool {
    if ir.state_cells.iter().any(|cell| {
        cell.path == path && matches!(cell.initial_value, boon_ir::InitialValue::Bool { .. })
    }) {
        return true;
    }
    let field = compiler_row_field_name(path);
    let path_scope = path.split_once('.').map(|(scope, _)| scope);
    let target_scope = target.split_once('.').map(|(scope, _)| scope);
    let list = path_scope
        .and_then(|scope| ir.row_scopes.iter().find(|row| row.row_scope == scope))
        .or_else(|| {
            target_scope.and_then(|scope| ir.row_scopes.iter().find(|row| row.row_scope == scope))
        })
        .map(|scope| scope.list.as_str());
    match list {
        Some(list) => compiler_ir_list_literal_field_is_bool(ir, list, field),
        None => false,
    }
}

fn compiler_ir_scalar_target_is_bool(ir: &TypedProgram, target: &str) -> bool {
    let Some(cell) = ir.state_cells.iter().find(|cell| cell.path == target) else {
        return compiler_ir_row_literal_field_for_path_is_bool(ir, target);
    };
    match &cell.initial_value {
        boon_ir::InitialValue::Bool { .. } => true,
        boon_ir::InitialValue::RowInitialField { path } => {
            let Some(scope_id) = cell.scope_id else {
                return false;
            };
            let Some(scope) = ir.row_scopes.get(scope_id.as_usize()) else {
                return false;
            };
            compiler_ir_list_literal_field_is_bool(ir, &scope.list, compiler_row_field_name(path))
        }
        boon_ir::InitialValue::RootInitialField { path } => ir.state_cells.iter().any(|root| {
            root.path == *path && matches!(root.initial_value, boon_ir::InitialValue::Bool { .. })
        }),
        boon_ir::InitialValue::Text { .. }
        | boon_ir::InitialValue::Number { .. }
        | boon_ir::InitialValue::Byte { .. }
        | boon_ir::InitialValue::Bytes { .. }
        | boon_ir::InitialValue::Enum { .. }
        | boon_ir::InitialValue::Unknown { .. } => false,
    }
}

fn compiler_ir_row_literal_field_for_path_is_bool(ir: &TypedProgram, path: &str) -> bool {
    let Some((scope, field)) = path.split_once('.') else {
        return false;
    };
    let Some(scope) = ir
        .row_scopes
        .iter()
        .find(|candidate| candidate.row_scope == scope)
    else {
        return false;
    };
    compiler_ir_list_literal_field_is_bool(ir, &scope.list, compiler_row_field_name(field))
}

fn compiler_ir_list_literal_field_is_bool(ir: &TypedProgram, list: &str, field_name: &str) -> bool {
    let Some(list) = ir.lists.iter().find(|candidate| candidate.name == list) else {
        return false;
    };
    let boon_ir::ListInitializer::RecordLiteral { rows } = &list.initializer else {
        return false;
    };
    let mut matched = false;
    for row in rows {
        for field in &row.fields {
            if field.name != field_name {
                continue;
            }
            matched = true;
            if !matches!(field.value, boon_ir::InitialValue::Bool { .. }) {
                return false;
            }
        }
    }
    matched
}

fn compiler_statement_ast_exprs(statement: &AstStatement, expressions: &[AstExpr]) -> Vec<AstExpr> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    compiler_collect_statement_expr_ids(statement, expressions, &mut seen, &mut ids);
    for expr in expressions {
        if compiler_statement_tree_contains_expr(statement, expr) {
            seen.insert(expr.id);
        }
    }
    expressions
        .iter()
        .filter(|expr| seen.contains(&expr.id))
        .cloned()
        .collect()
}

fn compiler_statement_tree_contains_expr(statement: &AstStatement, expr: &AstExpr) -> bool {
    (expr.start >= statement.start && expr.end <= statement.end)
        || statement
            .children
            .iter()
            .any(|child| compiler_statement_tree_contains_expr(child, expr))
}

fn compiler_collect_statement_expr_ids(
    statement: &AstStatement,
    expressions: &[AstExpr],
    seen: &mut BTreeSet<usize>,
    ids: &mut Vec<usize>,
) {
    if let Some(expr) = statement.expr {
        compiler_collect_expr_tree(expr, expressions, seen, ids);
    }
    for child in &statement.children {
        compiler_collect_statement_expr_ids(child, expressions, seen, ids);
    }
}

fn compiler_collect_expr_tree(
    id: usize,
    expressions: &[AstExpr],
    seen: &mut BTreeSet<usize>,
    ids: &mut Vec<usize>,
) {
    if !seen.insert(id) {
        return;
    }
    ids.push(id);
    let Some(expr) = expressions.iter().find(|expr| expr.id == id) else {
        return;
    };
    match &expr.kind {
        AstExprKind::Call { args, .. } => {
            for arg in args {
                compiler_collect_expr_tree(arg.value, expressions, seen, ids);
            }
        }
        AstExprKind::Pipe { input, args, .. } => {
            compiler_collect_expr_tree(*input, expressions, seen, ids);
            for arg in args {
                compiler_collect_expr_tree(arg.value, expressions, seen, ids);
            }
        }
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial } => {
            compiler_collect_expr_tree(*initial, expressions, seen, ids);
        }
        AstExprKind::Then { input, output } => {
            compiler_collect_expr_tree(*input, expressions, seen, ids);
            if let Some(output) = output {
                compiler_collect_expr_tree(*output, expressions, seen, ids);
            }
        }
        AstExprKind::Infix { left, right, .. } => {
            compiler_collect_expr_tree(*left, expressions, seen, ids);
            compiler_collect_expr_tree(*right, expressions, seen, ids);
        }
        AstExprKind::MatchArm { output, .. } => {
            if let Some(output) = output {
                compiler_collect_expr_tree(*output, expressions, seen, ids);
            }
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => {
            for field in fields {
                compiler_collect_expr_tree(field.value, expressions, seen, ids);
            }
        }
        AstExprKind::ListLiteral { items, .. } => {
            for item in items {
                compiler_collect_expr_tree(*item, expressions, seen, ids);
            }
        }
        AstExprKind::BytesLiteral { items, .. } => {
            for item in items {
                compiler_collect_expr_tree(*item, expressions, seen, ids);
            }
        }
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
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

fn compiler_statement_calls_router_go_to(exprs: &[AstExpr]) -> bool {
    exprs.iter().any(|expr| match &expr.kind {
        AstExprKind::Pipe { op, .. } => op == "Router/go_to",
        AstExprKind::Call { function, .. } => function == "Router/go_to",
        _ => false,
    })
}

fn compiler_source_then_text_value(exprs: &[AstExpr], source: &str) -> Option<String> {
    exprs.iter().find_map(|expr| {
        let AstExprKind::Then { input, output } = expr.kind else {
            return None;
        };
        let input_matches = compiler_source_then_input_matches(exprs, input, expr.line, source);
        if !input_matches {
            return None;
        }
        match output {
            Some(output) => compiler_ast_argument_value_in_exprs(exprs, output)
                .or_else(|| compiler_simple_value_in_expr_subtree(exprs, output)),
            None => compiler_simple_value_after_line(exprs, expr.line),
        }
    })
}

fn compiler_source_then_field_value(exprs: &[AstExpr], source: &str) -> Option<CompilerFieldValue> {
    exprs.iter().find_map(|expr| {
        let AstExprKind::Then { input, output } = expr.kind else {
            return None;
        };
        let input_matches = compiler_source_then_input_matches(exprs, input, expr.line, source);
        if !input_matches {
            return None;
        }
        if let Some(output) = output {
            return compiler_ast_simple_field_value_in_exprs(exprs, output)
                .or_else(|| compiler_simple_field_value_in_expr_subtree(exprs, output));
        }
        compiler_simple_field_value_after_line(exprs, expr.line)
    })
}

fn compiler_source_then_input_matches(
    exprs: &[AstExpr],
    input: usize,
    line: usize,
    source: &str,
) -> bool {
    match compiler_ast_argument_value_in_exprs(exprs, input) {
        Some(input_path) if !input_path.is_empty() => {
            compiler_source_event_path_matches(&input_path, source)
        }
        None => compiler_source_path_before_line_matches(exprs, line, source),
        Some(_) => compiler_source_path_before_line_matches(exprs, line, source),
    }
}

fn compiler_source_path_before_line_matches(exprs: &[AstExpr], line: usize, source: &str) -> bool {
    exprs
        .iter()
        .filter(|expr| expr.line < line)
        .rev()
        .find_map(|expr| match &expr.kind {
            AstExprKind::Path(parts) => Some(parts.join(".")),
            _ => None,
        })
        .is_some_and(|path| compiler_source_event_path_matches(&path, source))
}

fn compiler_simple_value_after_line(exprs: &[AstExpr], line: usize) -> Option<String> {
    exprs
        .iter()
        .filter(|expr| expr.line > line)
        .find_map(|expr| compiler_ast_simple_text_value_in_exprs(exprs, expr.id))
}

fn compiler_simple_value_in_expr_subtree(exprs: &[AstExpr], expr_id: usize) -> Option<String> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    compiler_collect_expr_tree(expr_id, exprs, &mut seen, &mut ids);
    ids.into_iter()
        .find_map(|id| compiler_ast_simple_text_value_in_exprs(exprs, id))
}

fn compiler_simple_field_value_after_line(
    exprs: &[AstExpr],
    line: usize,
) -> Option<CompilerFieldValue> {
    exprs
        .iter()
        .filter(|expr| expr.line > line)
        .find_map(|expr| compiler_ast_simple_field_value_in_exprs(exprs, expr.id))
}

fn compiler_simple_field_value_in_expr_subtree(
    exprs: &[AstExpr],
    expr_id: usize,
) -> Option<CompilerFieldValue> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    compiler_collect_expr_tree(expr_id, exprs, &mut seen, &mut ids);
    ids.into_iter()
        .find_map(|id| compiler_ast_simple_field_value_in_exprs(exprs, id))
}

fn compiler_ast_argument_value_in_exprs(exprs: &[AstExpr], expr_id: usize) -> Option<String> {
    let expr = exprs.iter().find(|expr| expr.id == expr_id)?;
    Some(match &expr.kind {
        AstExprKind::Identifier(value)
        | AstExprKind::Enum(value)
        | AstExprKind::Tag(value)
        | AstExprKind::Number(value) => value.clone(),
        AstExprKind::ByteLiteral { value, .. } => value.to_string(),
        AstExprKind::Path(parts) => parts.join("."),
        AstExprKind::Bool(true) => "True".to_owned(),
        AstExprKind::Bool(false) => "False".to_owned(),
        AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => value.clone(),
        AstExprKind::Unknown(tokens) => tokens.join("."),
        AstExprKind::Delimiter => String::new(),
        AstExprKind::Source
        | AstExprKind::Call { .. }
        | AstExprKind::Pipe { .. }
        | AstExprKind::Hold { .. }
        | AstExprKind::Latest
        | AstExprKind::When { .. }
        | AstExprKind::Then { .. }
        | AstExprKind::Infix { .. }
        | AstExprKind::MatchArm { .. }
        | AstExprKind::Record(_)
        | AstExprKind::Object(_)
        | AstExprKind::TaggedObject { .. }
        | AstExprKind::BytesLiteral { .. }
        | AstExprKind::ListLiteral { .. } => return None,
    })
}

fn compiler_ast_simple_text_value_in_exprs(exprs: &[AstExpr], expr_id: usize) -> Option<String> {
    let expr = exprs.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::Identifier(value)
        | AstExprKind::Enum(value)
        | AstExprKind::Tag(value)
        | AstExprKind::Number(value)
        | AstExprKind::StringLiteral(value)
        | AstExprKind::TextLiteral(value) => Some(value.clone()),
        AstExprKind::Path(parts) if !parts.is_empty() => Some(parts.join(".")),
        _ => None,
    }
}

fn compiler_ast_const_text_value_in_exprs(exprs: &[AstExpr], expr_id: usize) -> Option<String> {
    let expr = exprs.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::Enum(value)
        | AstExprKind::Tag(value)
        | AstExprKind::Number(value)
        | AstExprKind::StringLiteral(value)
        | AstExprKind::TextLiteral(value) => Some(value.clone()),
        AstExprKind::Bool(true) => Some("True".to_owned()),
        AstExprKind::Bool(false) => Some("False".to_owned()),
        _ => None,
    }
}

fn compiler_ast_simple_field_value_in_exprs(
    exprs: &[AstExpr],
    expr_id: usize,
) -> Option<CompilerFieldValue> {
    let expr = exprs.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::Bool(value) => Some(CompilerFieldValue::Bool(*value)),
        AstExprKind::Enum(value)
        | AstExprKind::Tag(value)
        | AstExprKind::Number(value)
        | AstExprKind::StringLiteral(value)
        | AstExprKind::TextLiteral(value) => Some(CompilerFieldValue::Text(value.clone())),
        _ => None,
    }
}

fn compiler_text_trim_input_path_from_exprs(exprs: &[AstExpr]) -> Option<String> {
    exprs.iter().find_map(|expr| {
        let AstExprKind::Pipe { input, op, .. } = &expr.kind else {
            return None;
        };
        (op == "Text/trim").then(|| compiler_ast_argument_value_in_exprs(exprs, *input))?
    })
}

fn compiler_source_payload_when_match_const_text_expression(
    exprs: &[AstExpr],
    source: &str,
) -> Option<CompilerDerivedTextExpression> {
    exprs.iter().find_map(|expr| {
        let AstExprKind::When { input } = expr.kind else {
            return None;
        };
        let input = compiler_ast_argument_value_in_exprs(exprs, input)
            .filter(|input| !input.is_empty())
            .or_else(|| compiler_source_payload_path_in_expr_subtree(exprs, input, source))
            .or_else(|| compiler_source_payload_path_before_line(exprs, expr.line, source))?;
        compiler_source_payload_field_from_input(&input, source)?;
        let arms = compiler_match_const_arms_after_when_values(exprs, expr.line);
        (!arms.is_empty()).then_some(CompilerDerivedTextExpression::MatchConst { input, arms })
    })
}

fn compiler_source_payload_when_match_const_expression(exprs: &[AstExpr], source: &str) -> bool {
    exprs.iter().any(|expr| {
        let AstExprKind::When { input } = expr.kind else {
            return false;
        };
        let Some(input) = compiler_ast_argument_value_in_exprs(exprs, input)
            .filter(|input| !input.is_empty())
            .or_else(|| compiler_source_payload_path_in_expr_subtree(exprs, input, source))
            .or_else(|| compiler_source_payload_path_before_line(exprs, expr.line, source))
        else {
            return false;
        };
        compiler_source_payload_field_from_input(&input, source).is_some()
            && compiler_match_const_arms_after_when(exprs, expr.line)
    })
}

fn compiler_source_then_match_const_text_expression(
    target: &str,
    exprs: &[AstExpr],
    source: &str,
    all_expressions: &[AstExpr],
    functions: &[boon_ir::FunctionDefinition],
) -> Option<CompilerDerivedTextExpression> {
    exprs.iter().find_map(|expr| {
        let AstExprKind::Then { input, output } = expr.kind else {
            return None;
        };
        if !compiler_source_then_input_matches(exprs, input, expr.line, source) {
            return None;
        }
        let output = output?;
        let output_expr = exprs.iter().find(|candidate| candidate.id == output)?;
        let AstExprKind::Call { function, args } = &output_expr.kind else {
            return None;
        };
        let function = compiler_function_definition_for_call(functions, function)?;
        let function_exprs = compiler_statement_ast_exprs(&function.statement, all_expressions);
        let when = function_exprs
            .iter()
            .find(|expr| matches!(expr.kind, AstExprKind::When { .. }))?;
        let AstExprKind::When { input } = when.kind else {
            return None;
        };
        let input_name = compiler_ast_argument_value_in_exprs(&function_exprs, input)?;
        let input =
            compiler_function_call_arg_path(target, exprs, args, &function.args, &input_name)?;
        let arms = compiler_match_const_arms_after_when_values(&function_exprs, when.line);
        (!arms.is_empty()).then_some(CompilerDerivedTextExpression::MatchConst { input, arms })
    })
}

fn compiler_function_definition_for_call<'a>(
    functions: &'a [boon_ir::FunctionDefinition],
    function: &str,
) -> Option<&'a boon_ir::FunctionDefinition> {
    functions
        .iter()
        .find(|definition| definition.name == function)
        .or_else(|| {
            let suffix = function.rsplit_once('/').map(|(_, name)| name)?;
            functions
                .iter()
                .find(|definition| definition.name == suffix)
        })
}

fn compiler_function_call_arg_path(
    target: &str,
    exprs: &[AstExpr],
    args: &[AstCallArg],
    formals: &[String],
    input_name: &str,
) -> Option<String> {
    let named = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some(input_name))
        .and_then(|arg| compiler_ast_argument_value_in_exprs(exprs, arg.value));
    let positional = formals
        .iter()
        .position(|formal| formal == input_name)
        .and_then(|index| {
            args.iter()
                .filter(|arg| arg.name.is_none())
                .nth(index)
                .and_then(|arg| compiler_ast_argument_value_in_exprs(exprs, arg.value))
        });
    named
        .or(positional)
        .map(|path| compiler_canonical_transform_text_path(target, &path))
}

fn compiler_source_then_inline_match_const_text_expression(
    target: &str,
    exprs: &[AstExpr],
    source: &str,
) -> Option<CompilerDerivedTextExpression> {
    exprs.iter().find_map(|expr| {
        let AstExprKind::Then { input, output } = expr.kind else {
            return None;
        };
        if !compiler_source_then_input_matches(exprs, input, expr.line, source) {
            return None;
        }
        let output = output?;
        let when = exprs.iter().find(|candidate| candidate.id == output)?;
        let AstExprKind::When { input } = when.kind else {
            return None;
        };
        let input = compiler_ast_argument_value_in_exprs(exprs, input)
            .map(|path| compiler_canonical_transform_text_path(target, &path))?;
        let arms = compiler_match_const_arms_after_when_values(exprs, when.line);
        (!arms.is_empty()).then_some(CompilerDerivedTextExpression::MatchConst { input, arms })
    })
}

fn compiler_source_then_inline_match_const_expression(
    target: &str,
    exprs: &[AstExpr],
    source: &str,
) -> bool {
    exprs.iter().any(|expr| {
        let AstExprKind::Then { input, output } = expr.kind else {
            return false;
        };
        if !compiler_source_then_input_matches(exprs, input, expr.line, source) {
            return false;
        }
        let Some(output) = output else {
            return false;
        };
        let Some(when) = exprs.iter().find(|candidate| candidate.id == output) else {
            return false;
        };
        let AstExprKind::When { input } = when.kind else {
            return false;
        };
        compiler_ast_argument_value_in_exprs(exprs, input)
            .map(|path| compiler_canonical_transform_text_path(target, &path))
            .is_some()
            && compiler_match_const_arms_after_when(exprs, when.line)
    })
}

fn compiler_source_then_prefix_payload_concat_text_expression(
    exprs: &[AstExpr],
    source: &str,
) -> Option<CompilerDerivedTextExpression> {
    exprs.iter().find_map(|expr| {
        let AstExprKind::Then { input, output } = expr.kind else {
            return None;
        };
        if !compiler_source_then_input_matches(exprs, input, expr.line, source) {
            return None;
        }
        output
            .and_then(|output| {
                compiler_prefix_payload_concat_text_expression(exprs, output, source)
            })
            .or_else(|| {
                compiler_prefix_payload_concat_text_expression_after_line(exprs, expr.line, source)
            })
    })
}

fn compiler_source_then_prefix_payload_concat_expression(exprs: &[AstExpr], source: &str) -> bool {
    exprs.iter().any(|expr| {
        let AstExprKind::Then { input, output } = expr.kind else {
            return false;
        };
        let input_matches = compiler_source_then_input_matches(exprs, input, expr.line, source);
        if !input_matches {
            return false;
        }
        output
            .is_some_and(|output| compiler_prefix_payload_concat_expression(exprs, output, source))
            || compiler_prefix_payload_concat_expression_after_line(exprs, expr.line, source)
    })
}

fn compiler_source_then_prefix_root_concat_text_expression(
    target: &str,
    exprs: &[AstExpr],
    source: &str,
) -> Option<CompilerDerivedTextExpression> {
    exprs.iter().find_map(|expr| {
        let AstExprKind::Then { input, output } = expr.kind else {
            return None;
        };
        if !compiler_source_then_input_matches(exprs, input, expr.line, source) {
            return None;
        }
        output
            .and_then(|output| compiler_prefix_root_concat_text_expression(target, exprs, output))
            .or_else(|| {
                compiler_prefix_root_concat_text_expression_after_line(target, exprs, expr.line)
            })
    })
}

fn compiler_source_then_prefix_root_concat_expression(
    target: &str,
    exprs: &[AstExpr],
    source: &str,
) -> bool {
    exprs.iter().any(|expr| {
        let AstExprKind::Then { input, output } = expr.kind else {
            return false;
        };
        if !compiler_source_then_input_matches(exprs, input, expr.line, source) {
            return false;
        }
        output.is_some_and(|output| compiler_prefix_root_concat_expression(target, exprs, output))
            || compiler_prefix_root_concat_expression_after_line(target, exprs, expr.line)
    })
}

fn compiler_source_direct_text_value(exprs: &[AstExpr], source: &str) -> Option<String> {
    exprs.iter().find_map(|expr| {
        let path = compiler_ast_argument_value_in_exprs(exprs, expr.id)?;
        compiler_source_payload_field_from_input(&path, source)?;
        Some(path)
    })
}

fn compiler_source_payload_path_in_expr_subtree(
    exprs: &[AstExpr],
    expr_id: usize,
    source: &str,
) -> Option<String> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    compiler_collect_expr_tree(expr_id, exprs, &mut seen, &mut ids);
    ids.into_iter().find_map(|id| {
        let path = compiler_ast_argument_value_in_exprs(exprs, id)?;
        compiler_source_payload_field_from_input(&path, source)?;
        Some(path)
    })
}

fn compiler_source_payload_path_before_line(
    exprs: &[AstExpr],
    line: usize,
    source: &str,
) -> Option<String> {
    exprs
        .iter()
        .filter(|expr| expr.line < line)
        .rev()
        .find_map(|expr| {
            let path = compiler_ast_argument_value_in_exprs(exprs, expr.id)?;
            compiler_source_payload_field_from_input(&path, source)?;
            Some(path)
        })
}

fn compiler_prefix_payload_concat_text_expression_after_line(
    exprs: &[AstExpr],
    line: usize,
    source: &str,
) -> Option<CompilerDerivedTextExpression> {
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
        .find_map(|expr| compiler_prefix_payload_concat_text_expression(exprs, expr.id, source))
}

fn compiler_prefix_payload_concat_expression_after_line(
    exprs: &[AstExpr],
    line: usize,
    source: &str,
) -> bool {
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
        .any(|expr| compiler_prefix_payload_concat_expression(exprs, expr.id, source))
}

fn compiler_prefix_payload_concat_text_expression(
    exprs: &[AstExpr],
    output: usize,
    source: &str,
) -> Option<CompilerDerivedTextExpression> {
    let expr = exprs.iter().find(|expr| expr.id == output)?;
    let AstExprKind::Pipe { op, input, args } = &expr.kind else {
        return None;
    };
    if op != "Text/concat" {
        return None;
    }
    let prefix = compiler_ast_const_text_value_in_exprs(exprs, *input)?;
    let payload_path = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some("with"))
        .or_else(|| args.iter().find(|arg| arg.name.is_none()))
        .and_then(|arg| compiler_ast_argument_value_in_exprs(exprs, arg.value))?;
    compiler_source_payload_field_from_input(&payload_path, source)?;
    let separator = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some("separator"))
        .and_then(|arg| compiler_ast_const_text_value_in_exprs(exprs, arg.value))
        .unwrap_or_default();
    Some(CompilerDerivedTextExpression::PrefixPayloadConcat {
        prefix,
        payload_path,
        separator,
    })
}

fn compiler_prefix_payload_concat_expression(
    exprs: &[AstExpr],
    output: usize,
    source: &str,
) -> bool {
    let Some(expr) = exprs.iter().find(|expr| expr.id == output) else {
        return false;
    };
    let AstExprKind::Pipe { op, input, args } = &expr.kind else {
        return false;
    };
    if op != "Text/concat" || compiler_ast_const_text_value_in_exprs(exprs, *input).is_none() {
        return false;
    }
    args.iter()
        .find(|arg| arg.name.as_deref() == Some("with"))
        .or_else(|| args.iter().find(|arg| arg.name.is_none()))
        .and_then(|arg| compiler_ast_argument_value_in_exprs(exprs, arg.value))
        .is_some_and(|payload_path| {
            compiler_source_payload_field_from_input(&payload_path, source).is_some()
        })
}

fn compiler_prefix_root_concat_text_expression_after_line(
    target: &str,
    exprs: &[AstExpr],
    line: usize,
) -> Option<CompilerDerivedTextExpression> {
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
        .filter(|expr| expr.line >= line && expr.line < end_line)
        .find_map(|expr| compiler_prefix_root_concat_text_expression(target, exprs, expr.id))
}

fn compiler_prefix_root_concat_expression_after_line(
    target: &str,
    exprs: &[AstExpr],
    line: usize,
) -> bool {
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
        .filter(|expr| expr.line >= line && expr.line < end_line)
        .any(|expr| compiler_prefix_root_concat_expression(target, exprs, expr.id))
}

fn compiler_prefix_root_concat_text_expression(
    target: &str,
    exprs: &[AstExpr],
    output: usize,
) -> Option<CompilerDerivedTextExpression> {
    let expr = exprs.iter().find(|expr| expr.id == output)?;
    let AstExprKind::Pipe { op, input, args } = &expr.kind else {
        return None;
    };
    if op != "Text/concat" {
        return None;
    }
    let prefix = compiler_ast_const_text_value_in_exprs(exprs, *input)?;
    let path = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some("with"))
        .or_else(|| args.iter().find(|arg| arg.name.is_none()))
        .and_then(|arg| compiler_ast_argument_value_in_exprs(exprs, arg.value))
        .map(|path| compiler_canonical_transform_text_path(target, &path))?;
    let separator = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some("separator"))
        .and_then(|arg| compiler_ast_const_text_value_in_exprs(exprs, arg.value))
        .unwrap_or_default();
    Some(CompilerDerivedTextExpression::PrefixRootConcat {
        prefix,
        path,
        separator,
    })
}

fn compiler_prefix_root_concat_expression(target: &str, exprs: &[AstExpr], output: usize) -> bool {
    let Some(expr) = exprs.iter().find(|expr| expr.id == output) else {
        return false;
    };
    let AstExprKind::Pipe { op, input, args } = &expr.kind else {
        return false;
    };
    if op != "Text/concat" || compiler_ast_const_text_value_in_exprs(exprs, *input).is_none() {
        return false;
    }
    args.iter()
        .find(|arg| arg.name.as_deref() == Some("with"))
        .or_else(|| args.iter().find(|arg| arg.name.is_none()))
        .and_then(|arg| compiler_ast_argument_value_in_exprs(exprs, arg.value))
        .map(|path| compiler_canonical_transform_text_path(target, &path))
        .is_some()
}

fn compiler_source_then_list_find_value_text_expression(
    target: &str,
    exprs: &[AstExpr],
    source: &str,
) -> Option<CompilerDerivedTextExpression> {
    exprs.iter().find_map(|expr| {
        let AstExprKind::Then { input, output } = expr.kind else {
            return None;
        };
        if !compiler_source_then_input_matches(exprs, input, expr.line, source) {
            return None;
        }
        let (list_input, args) = match output {
            Some(output) => compiler_list_find_value_call_in_expr_subtree(exprs, output),
            None => compiler_list_find_value_call_after_then(exprs, expr),
        }?;
        let list = compiler_ast_argument_value_in_exprs(exprs, list_input)
            .map(|path| compiler_canonical_transform_text_path(target, &path))?;
        let field = compiler_named_arg_value(exprs, args, "field")?;
        let expected = args
            .iter()
            .find(|arg| arg.name.as_deref() == Some("value"))
            .and_then(|arg| compiler_update_value_expression_from_expr(exprs, arg.value))?;
        let value_target = compiler_named_arg_value(exprs, args, "target")?;
        let fallback = args
            .iter()
            .find(|arg| arg.name.as_deref() == Some("fallback"))
            .and_then(|arg| compiler_update_value_expression_from_expr(exprs, arg.value))
            .map(Box::new);
        Some(CompilerDerivedTextExpression::ListFindValue {
            list,
            field,
            expected: Box::new(expected),
            target: value_target,
            fallback,
        })
    })
}

fn compiler_named_arg_value(exprs: &[AstExpr], args: &[AstCallArg], name: &str) -> Option<String> {
    args.iter()
        .find(|arg| arg.name.as_deref() == Some(name))
        .and_then(|arg| compiler_ast_argument_value_in_exprs(exprs, arg.value))
}

fn compiler_list_find_value_call_in_expr_subtree(
    exprs: &[AstExpr],
    expr_id: usize,
) -> Option<(usize, &[AstCallArg])> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    compiler_collect_expr_tree(expr_id, exprs, &mut seen, &mut ids);
    ids.into_iter()
        .filter_map(|id| exprs.iter().find(|expr| expr.id == id))
        .find_map(compiler_list_find_value_call_parts)
}

fn compiler_list_find_value_call_after_then<'a>(
    exprs: &'a [AstExpr],
    then_expr: &AstExpr,
) -> Option<(usize, &'a [AstCallArg])> {
    exprs
        .iter()
        .filter(|expr| expr.start > then_expr.end)
        .take_while(|expr| {
            !matches!(
                expr.kind,
                AstExprKind::Then { .. } | AstExprKind::MatchArm { .. }
            )
        })
        .find_map(compiler_list_find_value_call_parts)
}

fn compiler_list_find_value_call_parts(expr: &AstExpr) -> Option<(usize, &[AstCallArg])> {
    match &expr.kind {
        AstExprKind::Call { function, args } if function == "List/find_value" => args
            .iter()
            .find(|arg| arg.name.is_none())
            .map(|arg| (arg.value, args.as_slice())),
        AstExprKind::Pipe { input, op, args } if op == "List/find_value" => {
            Some((*input, args.as_slice()))
        }
        _ => None,
    }
}

fn compiler_update_value_expression_from_expr(
    exprs: &[AstExpr],
    expr_id: usize,
) -> Option<CompilerUpdateValueExpression> {
    let expr = exprs.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::Path(parts) if !parts.is_empty() => {
            Some(CompilerUpdateValueExpression::ReadPath {
                path: parts.join("."),
            })
        }
        AstExprKind::Identifier(value) => Some(CompilerUpdateValueExpression::ReadPath {
            path: value.clone(),
        }),
        AstExprKind::Bool(value) => Some(CompilerUpdateValueExpression::Const {
            value: if *value { "True" } else { "False" }.to_owned(),
        }),
        AstExprKind::Enum(value)
        | AstExprKind::Tag(value)
        | AstExprKind::Number(value)
        | AstExprKind::StringLiteral(value)
        | AstExprKind::TextLiteral(value) => Some(CompilerUpdateValueExpression::Const {
            value: value.clone(),
        }),
        _ => None,
    }
}

fn compiler_source_then_const_text_value(exprs: &[AstExpr], source: &str) -> Option<String> {
    exprs.iter().find_map(|expr| {
        let AstExprKind::Then { input, output } = expr.kind else {
            return None;
        };
        if !compiler_source_then_input_matches(exprs, input, expr.line, source) {
            return None;
        }
        match output {
            Some(output) => compiler_ast_const_text_value_in_exprs(exprs, output)
                .or_else(|| compiler_const_text_value_in_expr_subtree(exprs, output)),
            None => compiler_const_text_value_after_line(exprs, expr.line),
        }
    })
}

fn compiler_const_text_value_in_expr_subtree(exprs: &[AstExpr], expr_id: usize) -> Option<String> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    compiler_collect_expr_tree(expr_id, exprs, &mut seen, &mut ids);
    ids.into_iter()
        .find_map(|id| compiler_ast_const_text_value_in_exprs(exprs, id))
}

fn compiler_const_text_value_after_line(exprs: &[AstExpr], line: usize) -> Option<String> {
    exprs
        .iter()
        .filter(|expr| expr.line > line)
        .find_map(|expr| compiler_ast_const_text_value_in_exprs(exprs, expr.id))
}

fn compiler_match_const_arms_after_when_values(
    exprs: &[AstExpr],
    when_line: usize,
) -> Vec<CompilerUpdateMatchArm> {
    let end_line = exprs
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
    exprs
        .iter()
        .filter(|expr| expr.line >= when_line && expr.line < end_line)
        .filter_map(|expr| {
            let AstExprKind::MatchArm {
                pattern,
                output: Some(output),
            } = &expr.kind
            else {
                return None;
            };
            let pattern = compiler_match_const_pattern_label(pattern)?;
            let output = compiler_ast_simple_text_value_in_exprs(exprs, *output)?;
            Some(CompilerUpdateMatchArm { pattern, output })
        })
        .collect()
}

fn compiler_match_const_pattern_label(pattern: &[String]) -> Option<String> {
    if pattern == ["__"] {
        Some("__".to_owned())
    } else if pattern.first().map(String::as_str) == Some("TEXT")
        && pattern.get(1).map(String::as_str) == Some("{")
        && pattern.last().map(String::as_str) == Some("}")
    {
        Some(pattern[2..pattern.len() - 1].join(" "))
    } else if pattern.len() == 1 {
        Some(pattern[0].clone())
    } else {
        Some(pattern.join("."))
    }
}

fn compiler_match_const_arms_after_when(exprs: &[AstExpr], when_line: usize) -> bool {
    let end_line = exprs
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
    exprs
        .iter()
        .filter(|expr| expr.line >= when_line && expr.line < end_line)
        .any(|expr| {
            matches!(
                expr.kind,
                AstExprKind::MatchArm {
                    output: Some(_),
                    ..
                }
            )
        })
}

fn compiler_canonical_transform_text_path(owner_path: &str, path: &str) -> String {
    if path.contains('.') {
        return path.to_owned();
    }
    owner_path
        .rsplit_once('.')
        .map(|(parent, _)| format!("{parent}.{path}"))
        .unwrap_or_else(|| path.to_owned())
}

fn compiler_source_payload_field_from_input(input: &str, source: &str) -> Option<String> {
    compiler_source_event_ref_variants(source)
        .into_iter()
        .find_map(|variant| {
            let suffix = compiler_source_payload_suffix_from_variant(input, &variant)?;
            Some(match suffix {
                "change.text" | "event.change.text" | "events.change.text" => "text".to_owned(),
                "change.bytes" | "event.change.bytes" | "events.change.bytes" => "bytes".to_owned(),
                "key_down.key" | "event.key_down.key" | "events.key_down.key" => "key".to_owned(),
                "event.address" | "events.address" => "address".to_owned(),
                _ if !suffix.contains('.') => suffix.to_owned(),
                _ if suffix.starts_with("event.") && !suffix["event.".len()..].contains('.') => {
                    suffix["event.".len()..].to_owned()
                }
                _ if suffix.starts_with("events.") && !suffix["events.".len()..].contains('.') => {
                    suffix["events.".len()..].to_owned()
                }
                _ => return None,
            })
        })
}

fn compiler_source_payload_suffix_from_variant<'a>(
    input: &'a str,
    variant: &str,
) -> Option<&'a str> {
    if let Some(suffix) = compiler_source_suffix_after_variant(input, variant) {
        return Some(suffix);
    }
    let (base, event) = variant.rsplit_once('.')?;
    for event_prefix in [
        format!("{base}.event.{event}"),
        format!("{base}.events.{event}"),
    ] {
        if let Some(suffix) = compiler_source_suffix_after_variant(input, &event_prefix) {
            return Some(suffix);
        }
    }
    None
}

fn compiler_source_is_scoped(ir: &TypedProgram, source: &str) -> bool {
    ir.sources
        .iter()
        .find(|candidate| candidate.path == source)
        .is_some_and(|candidate| candidate.scoped)
}

fn compiler_source_event_path_matches(path: &str, source: &str) -> bool {
    compiler_source_event_ref_variants(source)
        .iter()
        .any(|variant| compiler_source_suffix_after_variant(path, variant).is_some())
        || source
            .strip_prefix("store.")
            .and_then(|suffix| suffix.split_once('.').map(|(_, tail)| tail))
            .filter(|tail| !tail.is_empty())
            .is_some_and(|tail| compiler_source_suffix_after_variant(path, tail).is_some())
}

fn compiler_source_event_ref_variants(source: &str) -> Vec<String> {
    let mut variants = vec![source.to_owned()];
    if let Some((_, suffix)) = source.split_once('.') {
        variants.push(suffix.to_owned());
        variants.push(format!("item.{suffix}"));
    }
    variants
}

fn compiler_source_suffix_after_variant<'a>(input: &'a str, variant: &str) -> Option<&'a str> {
    if input == variant {
        return Some("");
    }
    if let Some(suffix) = input
        .strip_prefix(variant)
        .and_then(|suffix| suffix.strip_prefix('.'))
    {
        return Some(suffix);
    }
    let dotted_variant = format!(".{variant}");
    let start = input.find(&dotted_variant)?;
    let suffix = &input[start + dotted_variant.len()..];
    if suffix.is_empty() {
        return Some("");
    }
    suffix.strip_prefix('.')
}

fn compiler_statement_contains_latest(statement: &AstStatement, expressions: &[AstExpr]) -> bool {
    statement
        .expr
        .and_then(|expr_id| expressions.get(expr_id))
        .is_some_and(|expr| matches!(expr.kind, AstExprKind::Latest))
        || statement
            .children
            .iter()
            .any(|child| compiler_statement_contains_latest(child, expressions))
}

pub fn compiler_dynamic_list_view_lists_from_ir(ir: &TypedProgram) -> BTreeSet<String> {
    ir.lists
        .iter()
        .filter(|list| compiler_ir_list_has_derived_list_view(ir, &list.name))
        .map(|list| list.name.clone())
        .collect()
}

pub fn compiler_observed_root_paths_from_ir(ir: &TypedProgram) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for binding in &ir.view_bindings {
        if !matches!(
            binding.kind,
            boon_ir::ViewBindingKind::Data
                | boon_ir::ViewBindingKind::Target
                | boon_ir::ViewBindingKind::Source
        ) {
            continue;
        }
        for variant in compiler_root_path_observation_variants(&binding.path) {
            paths.insert(variant);
        }
    }
    paths
}

fn compiler_root_path_observation_variants(path: &str) -> Vec<String> {
    let mut variants = BTreeSet::from([path.to_owned()]);
    if let Some(passed) = path.strip_prefix("PASSED.") {
        variants.extend(compiler_root_path_observation_variants(passed));
    }
    if let Some(local) = path.strip_prefix("store.") {
        variants.insert(local.to_owned());
    } else if !path.starts_with('@') && !path.contains(':') {
        variants.insert(format!("store.{path}"));
    }
    variants.into_iter().collect()
}

pub fn compiler_document_projection_storage_resolutions_from_ir(
    ir: &TypedProgram,
) -> CompilerDocumentProjectionStorageResolutions {
    let list_projections = compiler_list_projections_from_ir(ir);
    let storage_list_slots = compiler_storage_list_slots_from_ir(ir);
    let storage_list_names = storage_list_slots
        .iter()
        .map(|slot| slot.name.clone())
        .collect::<BTreeSet<_>>();
    let mut resolutions = BTreeMap::new();
    let mut unresolved_paths = BTreeSet::new();
    for projection in &list_projections {
        if resolutions.contains_key(&projection.list) {
            continue;
        }
        if let Some(list) =
            compiler_document_storage_list_name_for_path(&projection.list, &storage_list_names)
        {
            resolutions.insert(projection.list.clone(), list);
            continue;
        }
        if let Some((_, list)) = compiler_document_direct_root_list_ref_for_path(
            ir,
            &projection.list,
            &storage_list_names,
        ) {
            resolutions.insert(projection.list.clone(), list);
            continue;
        }
        unresolved_paths.insert(projection.list.clone());
    }
    CompilerDocumentProjectionStorageResolutions {
        resolutions,
        unresolved_paths,
    }
}

fn compiler_document_storage_list_name_for_path(
    path: &str,
    storage_list_names: &BTreeSet<String>,
) -> Option<String> {
    if storage_list_names.contains(path) {
        return Some(path.to_owned());
    }
    if let Some(stripped) = path.strip_prefix("store.")
        && storage_list_names.contains(stripped)
    {
        return Some(stripped.to_owned());
    }
    let local = compiler_row_field_name(path);
    storage_list_names.contains(local).then(|| local.to_owned())
}

fn compiler_document_direct_root_list_ref_for_path(
    ir: &TypedProgram,
    path: &str,
    storage_list_names: &BTreeSet<String>,
) -> Option<(String, String)> {
    let value = ir.derived_values.iter().find(|value| {
        !value.indexed && compiler_root_state_path_matches_runtime_path(&value.path, path)
    })?;
    compiler_document_direct_root_list_ref_for_statement(ir, &value.statement, storage_list_names)
}

fn compiler_document_direct_root_list_ref_for_statement(
    ir: &TypedProgram,
    statement: &AstStatement,
    storage_list_names: &BTreeSet<String>,
) -> Option<(String, String)> {
    if !statement.children.is_empty() {
        return None;
    }
    let expr = ir.expressions.get(statement.expr?)?;
    let referenced = match &expr.kind {
        AstExprKind::Identifier(name) => name.clone(),
        AstExprKind::Path(parts) if !parts.is_empty() => parts.join("."),
        _ => return None,
    };
    let plan = ir.derived_values.iter().find(|value| {
        !value.indexed
            && value.scope_id.is_none()
            && matches!(value.kind, boon_ir::DerivedValueKind::ListView)
            && compiler_root_state_path_matches_runtime_path(&value.path, &referenced)
    })?;
    let list = compiler_document_storage_list_name_for_path(&plan.path, storage_list_names)?;
    Some((plan.path.clone(), list))
}

fn compiler_root_state_path_matches_runtime_path(root_path: &str, path: &str) -> bool {
    root_path == path || root_path.strip_prefix("store.") == Some(path)
}

pub fn compiler_document_render_slots_from_ir(ir: &TypedProgram) -> CompilerDocumentRenderSlots {
    let slots = ir
        .typecheck_report
        .render_slot_table
        .slots
        .iter()
        .map(|slot| CompilerDocumentRenderSlot {
            slot_statement_id: slot.slot_statement_id,
            slot_name: slot.slot_name.clone(),
            expected_contract: slot.expected_contract.clone(),
            value_expr_id: slot.value_expr_id,
            actual_type: serde_json::to_value(&slot.actual_type)
                .unwrap_or_else(|_| json!({"serialization_error": "actual_type"})),
            diagnostic_count: slot.diagnostics.len(),
            optional_list_map_binding_id: slot.optional_list_map_binding_id,
            item_scope_id: slot.item_scope_id,
            template_function: slot.template_function.clone(),
            template_arg_count: slot.template_args.len(),
            materialization_policy: format!("{:?}", slot.materialization_policy),
        })
        .collect();
    CompilerDocumentRenderSlots {
        render_slot_table_hash: compiler_render_slot_table_hash(ir),
        render_slot_count: ir.typecheck_report.render_slot_count,
        render_slot_failure_count: ir.typecheck_report.render_slot_failure_count,
        full_document_typecheck_coverage: ir.typecheck_report.full_document_typecheck_coverage,
        list_map_binding_count_render_slot_materialization: ir
            .typecheck_report
            .list_map_binding_count_render_slot_materialization,
        slots,
    }
}

fn compiler_render_slot_table_hash(ir: &TypedProgram) -> String {
    serde_json::to_vec(&ir.typecheck_report.render_slot_table)
        .map(|bytes| {
            let mut hasher = Sha256::new();
            hasher.update(bytes);
            format!("{:x}", hasher.finalize())
        })
        .unwrap_or_else(|_| "render-slot-table-hash-error".to_owned())
}

pub fn compiler_typecheck_report_metadata_from_ir(
    ir: &TypedProgram,
) -> CompilerTypecheckReportMetadata {
    CompilerTypecheckReportMetadata {
        typecheck_report_hash: serde_json::to_vec(&ir.typecheck_report)
            .map(|bytes| sha256_bytes(&bytes))
            .unwrap_or_else(|_| "unserializable-typecheck-report".to_owned()),
        render_slot_table_hash: compiler_render_slot_table_hash(ir),
        typed_render_metadata_used: ir.typecheck_report.render_slot_count > 0,
        unresolved_type_variable_count: ir.typecheck_report.unresolved_type_variable_count,
        render_slot_count: ir.typecheck_report.render_slot_count,
        render_slot_failure_count: ir.typecheck_report.render_slot_failure_count,
        report: serde_json::to_value(&ir.typecheck_report).unwrap_or_else(|_| {
            json!({
                "status": "unserializable",
                "reason": "typecheck report could not be serialized"
            })
        }),
    }
}

pub fn compiler_typed_program_report_metadata_from_ir(
    ir: &TypedProgram,
) -> CompilerTypedProgramReportMetadata {
    CompilerTypedProgramReportMetadata {
        expression_count: ir.expression_count,
        expression_coverage: serde_json::to_value(&ir.expression_coverage).unwrap_or_else(|_| {
            json!({
                "status": "unserializable",
                "reason": "expression coverage could not be serialized"
            })
        }),
        expression_coverage_unknown_total: ir.expression_coverage.unknown_total(),
        graph_node_count: ir.graph_node_count,
        semantic_index: ir.semantic_index.report(),
        hidden_identity_verified: ir.hidden_identity_verified,
        static_schedule_verified: ir.static_schedule_verified,
    }
}

pub fn compiler_typed_program_inventory_counts_from_ir(
    ir: &TypedProgram,
) -> CompilerTypedProgramInventoryCounts {
    CompilerTypedProgramInventoryCounts {
        schedule_node_count: ir.nodes.len(),
        source_port_count: ir.sources.len(),
        state_initializer_count: ir.state_cells.len(),
        list_initializer_count: ir.lists.len(),
        derived_value_count: ir.derived_values.len(),
        update_branch_count: ir.update_branches.len(),
        list_operation_count: ir.list_operations.len(),
        list_projection_count: ir.list_projections.len(),
        view_binding_count: ir.view_bindings.len(),
    }
}

pub fn compiler_list_summary_fields_from_ir(ir: &TypedProgram) -> Vec<CompilerListSummaryFields> {
    let mut summaries = ir
        .lists
        .iter()
        .map(|list| {
            let row_scope = compiler_row_scope_name_for_ir_list(ir, list);
            let prefix = format!("{row_scope}.");
            let mut fields = ir
                .state_cells
                .iter()
                .filter(|cell| cell.indexed && cell.path.starts_with(&prefix))
                .filter_map(|cell| cell.path.strip_prefix(&prefix).map(str::to_owned))
                .collect::<Vec<_>>();
            for value in &ir.derived_values {
                if value.indexed && value.path.starts_with(&prefix) {
                    if let Some(field) = value.path.strip_prefix(&prefix) {
                        fields.push(field.to_owned());
                    }
                }
            }
            if let boon_ir::ListInitializer::RecordLiteral { rows } = &list.initializer {
                for row in rows {
                    for field in &row.fields {
                        fields.push(field.name.clone());
                    }
                }
            }
            for operation in &ir.list_operations {
                let boon_ir::ListOperationKind::Append {
                    fields: append_fields,
                    ..
                } = &operation.kind
                else {
                    continue;
                };
                if operation.list == list.name {
                    fields.extend(append_fields.iter().map(|field| field.name.clone()));
                }
            }
            fields.sort();
            fields.dedup();
            CompilerListSummaryFields {
                list: list.name.clone(),
                row_scope,
                fields,
            }
        })
        .collect::<Vec<_>>();
    for value in ir.derived_values.iter().filter(|value| {
        !value.indexed
            && value.scope_id.is_none()
            && matches!(value.kind, boon_ir::DerivedValueKind::ListView)
    }) {
        let list = compiler_derived_root_list_storage_name(&value.path);
        if summaries.iter().any(|summary| summary.list == list) {
            continue;
        }
        summaries.push(CompilerListSummaryFields {
            row_scope: list.clone(),
            list,
            fields: Vec::new(),
        });
    }
    summaries
}

fn compiler_ir_list_has_derived_list_view(ir: &TypedProgram, list_name: &str) -> bool {
    let store_path = format!("store.{list_name}");
    ir.derived_values.iter().any(|value| {
        matches!(value.kind, boon_ir::DerivedValueKind::ListView)
            && (value.path == list_name
                || value.path == store_path
                || compiler_row_field_name(&value.path) == list_name)
    })
}

fn compiler_list_initial_records_have_dynamic_fields(rows: &[boon_ir::ListInitialRecord]) -> bool {
    rows.iter().any(|row| {
        row.fields
            .iter()
            .any(|field| matches!(field.value, boon_ir::InitialValue::RowInitialField { .. }))
    })
}

fn compiler_derived_root_list_storage_name(path: &str) -> String {
    path.strip_prefix("store.")
        .map(str::to_owned)
        .unwrap_or_else(|| compiler_row_field_name(path).to_owned())
}

fn compiler_row_scope_name_for_ir_list(ir: &TypedProgram, list: &boon_ir::ListMemory) -> String {
    list.row_scope_id
        .and_then(|scope_id| ir.row_scopes.get(scope_id.as_usize()))
        .map(|scope| scope.row_scope.clone())
        .unwrap_or_else(|| compiler_row_scope_name(&list.name))
}

fn compiler_row_scope_name(list_name: &str) -> String {
    list_name
        .strip_suffix("ies")
        .map(|prefix| format!("{prefix}y"))
        .or_else(|| list_name.strip_suffix('s').map(str::to_owned))
        .unwrap_or_else(|| format!("{list_name}_item"))
}

fn compiler_row_field_name(path: &str) -> &str {
    path.rsplit_once('.')
        .map(|(_, field)| field)
        .unwrap_or(path)
}

fn compiler_base_row_field_name(field: &str) -> String {
    format!("__base_{}", field.replace('.', "_"))
}

pub fn compiler_source_payload_counts_from_ir(ir: &TypedProgram) -> CompilerSourcePayloadCounts {
    let mut counts = CompilerSourcePayloadCounts {
        schema_count: ir.sources.len(),
        ..CompilerSourcePayloadCounts::default()
    };
    for source in &ir.sources {
        for field in &source.payload_schema.fields {
            counts.field_count += 1;
            match field {
                boon_ir::SourcePayloadField::Text => counts.text_field_count += 1,
                boon_ir::SourcePayloadField::Key => counts.key_field_count += 1,
                boon_ir::SourcePayloadField::Address => counts.address_field_count += 1,
                boon_ir::SourcePayloadField::Bytes => counts.bytes_field_count += 1,
                boon_ir::SourcePayloadField::Named(_) => counts.pointer_field_count += 1,
            }
        }
    }
    counts
}

pub fn compiler_unsupported_runtime_diagnostics_from_ir(
    ir: &TypedProgram,
) -> CompilerUnsupportedRuntimeDiagnostics {
    let unsupported_state_initializer = ir.state_cells.iter().find_map(|cell| {
        let boon_ir::InitialValue::Unknown { summary } = &cell.initial_value else {
            return None;
        };
        Some(CompilerUnsupportedStateInitializer {
            path: cell.path.clone(),
            summary: summary.clone(),
        })
    });
    let unsupported_list_initializer = ir.lists.iter().find_map(|list| {
        let boon_ir::ListInitializer::Unknown { summary } = &list.initializer else {
            return None;
        };
        (!compiler_ir_list_has_derived_list_view(ir, &list.name)).then(|| {
            CompilerUnsupportedListInitializer {
                list: list.name.clone(),
                summary: summary.clone(),
            }
        })
    });
    let graph_clone_list = ir.lists.iter().find_map(|list| {
        (list.graph_clones_per_item != 0).then(|| CompilerGraphCloneList {
            list: list.name.clone(),
            graph_clones_per_item: list.graph_clones_per_item,
        })
    });
    let unsupported_update_branch_count = ir
        .update_branches
        .iter()
        .filter(|branch| matches!(branch.expression, boon_ir::UpdateExpression::Unknown { .. }))
        .count();
    let unsupported_update_branch = ir.update_branches.iter().find_map(|branch| {
        let boon_ir::UpdateExpression::Unknown { summary } = &branch.expression else {
            return None;
        };
        Some(CompilerUnsupportedUpdateBranch {
            target: branch.target.clone(),
            source: branch.source.clone(),
            summary: summary.clone(),
        })
    });
    let unsupported_list_operation_count = ir
        .list_operations
        .iter()
        .filter(|operation| compiler_list_operation_has_unknown_predicate(operation))
        .count();
    let unsupported_list_operation = ir
        .list_operations
        .iter()
        .find(|operation| compiler_list_operation_has_unknown_predicate(operation))
        .map(|operation| CompilerUnsupportedListOperation {
            list: operation.list.clone(),
        });
    CompilerUnsupportedRuntimeDiagnostics {
        unsupported_state_initializer,
        unsupported_list_initializer,
        graph_clone_list,
        unsupported_update_branch_count,
        unsupported_update_branch,
        unsupported_list_operation_count,
        unsupported_list_operation,
    }
}

fn compiler_list_operation_has_unknown_predicate(operation: &boon_ir::ListOperation) -> bool {
    match &operation.kind {
        boon_ir::ListOperationKind::Append { .. } => false,
        boon_ir::ListOperationKind::Remove { predicate, .. }
        | boon_ir::ListOperationKind::Retain { predicate, .. }
        | boon_ir::ListOperationKind::Count { predicate, .. } => {
            matches!(predicate, boon_ir::ListPredicate::Unknown { .. })
        }
    }
}

pub fn compiler_runtime_profile_metadata_from_ir(
    ir: &TypedProgram,
) -> CompilerRuntimeProfileMetadata {
    let lists = ir
        .lists
        .iter()
        .map(|list| CompilerRuntimeListCapacity {
            name: list.name.clone(),
            declared_capacity: list.capacity,
            effective_capacity: compiler_list_effective_capacity(list),
            capacity_source: compiler_list_capacity_source(list).to_owned(),
        })
        .collect::<Vec<_>>();
    let bytes = ir
        .state_cells
        .iter()
        .filter_map(|cell| match &cell.initial_value {
            boon_ir::InitialValue::Bytes { fixed_len, .. } => Some(CompilerRuntimeBytesCapacity {
                name: cell.path.clone(),
                scope: if cell.indexed { "indexed" } else { "root" }.to_owned(),
                fixed_len: *fixed_len,
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    CompilerRuntimeProfileMetadata {
        all_lists_bounded: lists.iter().all(|list| list.effective_capacity.is_some()),
        all_bytes_bounded: bytes.iter().all(|bytes| bytes.fixed_len.is_some()),
        lists,
        bytes,
    }
}

fn compiler_list_effective_capacity(list: &boon_ir::ListMemory) -> Option<usize> {
    list.capacity.or_else(|| match list.initializer {
        boon_ir::ListInitializer::Range { from, to } if from <= to => {
            usize::try_from(to.saturating_sub(from).saturating_add(1)).ok()
        }
        boon_ir::ListInitializer::Range { .. } => Some(0),
        _ => None,
    })
}

fn compiler_list_capacity_source(list: &boon_ir::ListMemory) -> &'static str {
    if list.capacity.is_some() {
        "list_capacity_syntax"
    } else if matches!(list.initializer, boon_ir::ListInitializer::Range { .. }) {
        "range_initializer"
    } else {
        "dynamic_list"
    }
}

fn compiler_derived_value_kind(kind: &boon_ir::DerivedValueKind) -> CompilerDerivedValueKind {
    match kind {
        boon_ir::DerivedValueKind::SourceEventTransform => {
            CompilerDerivedValueKind::SourceEventTransform
        }
        boon_ir::DerivedValueKind::ListView => CompilerDerivedValueKind::ListView,
        boon_ir::DerivedValueKind::Aggregate => CompilerDerivedValueKind::Aggregate,
        boon_ir::DerivedValueKind::Pure => CompilerDerivedValueKind::Pure,
        boon_ir::DerivedValueKind::Unknown => CompilerDerivedValueKind::Unknown,
    }
}

fn compiler_runtime_generic_derived_plan_from_parts(
    expressions: &[AstExpr],
    functions: &[CompilerGenericDerivedFunction],
    output_roots: &[CompilerGenericDerivedOutputRoot],
    root_fields: &[CompilerGenericDerivedRootField],
    indexed_fields: &[CompilerGenericDerivedIndexedField],
) -> CompilerRuntimeGenericDerivedPlan {
    let mut plan = CompilerRuntimeGenericDerivedPlan::default();
    let function_names = functions
        .iter()
        .map(|function| function.name.clone())
        .collect::<BTreeSet<_>>();
    let mut compiled_functions = BTreeMap::new();
    let mut unsupported_functions = BTreeMap::new();
    for function in functions {
        match CompilerRuntimeGenericFunction::from_ast(function, expressions, &function_names) {
            Ok(compiled) => {
                compiled_functions.insert(function.name.clone(), compiled);
            }
            Err(reason) => {
                unsupported_functions.insert(function.name.clone(), reason);
            }
        }
    }
    for output in output_roots {
        let compiled = CompilerRuntimeGenericStatement::from_ast(
            &output.statement,
            expressions,
            &function_names,
        );
        let (statement, unsupported_reason) = match compiled {
            Ok(statement) => (Some(statement), None),
            Err(reason) => {
                *plan.unsupported_reasons.entry(reason.clone()).or_default() += 1;
                (None, Some(reason))
            }
        };
        plan.output_roots.push(CompilerRuntimeGenericOutputRoot {
            root: output.root.clone(),
            output_kind: output.output_kind.clone(),
            typed_contract_known: output.typed_contract_known,
            generic_output_port: output.generic_output_port,
            statement,
            unsupported_reason,
        });
    }
    for field in root_fields {
        let compiled = CompilerRuntimeGenericStatement::from_ast(
            &field.statement,
            expressions,
            &function_names,
        );
        let (statement, unsupported_reason) = match compiled {
            Ok(statement) => (Some(statement), None),
            Err(reason) => {
                *plan.unsupported_reasons.entry(reason.clone()).or_default() += 1;
                (None, Some(reason))
            }
        };
        plan.root_fields.push(CompilerRuntimeGenericRootField {
            path: field.path.clone(),
            kind: field.kind.clone(),
            has_sources: field.has_sources,
            statement,
            unsupported_reason,
        });
    }
    for field in indexed_fields {
        let compiled = CompilerRuntimeGenericStatement::from_ast(
            &field.statement,
            expressions,
            &function_names,
        );
        let (statement, unsupported_reason) = match compiled {
            Ok(statement) => (Some(statement), None),
            Err(reason) => {
                *plan.unsupported_reasons.entry(reason.clone()).or_default() += 1;
                (None, Some(reason))
            }
        };
        plan.indexed_fields
            .push(CompilerRuntimeGenericIndexedField {
                list: field.list.clone(),
                row_scope: field.row_scope.clone(),
                field: field.field.clone(),
                kind: field.kind.clone(),
                startup_recompute: field.startup_recompute,
                statement,
                unsupported_reason,
            });
    }

    let reachable_functions = compiler_runtime_reachable_function_names(
        &plan,
        &compiled_functions,
        &unsupported_functions,
    );
    for function in &reachable_functions.missing {
        let reason = unsupported_functions
            .get(function)
            .map(|reason| format!("function:{function}:{reason}"))
            .unwrap_or_else(|| format!("function:{function}:missing"));
        *plan.unsupported_reasons.entry(reason).or_default() += 1;
    }
    for field in &mut plan.root_fields {
        if let Some(statement) = field.statement.as_ref()
            && let Some(function) = compiler_runtime_statement_missing_function(
                statement,
                &compiled_functions,
                &unsupported_functions,
            )
        {
            let reason = unsupported_functions
                .get(&function)
                .map(|reason| format!("function:{function}:{reason}"))
                .unwrap_or_else(|| format!("function:{function}:missing"));
            field.statement = None;
            field.unsupported_reason = Some(reason.clone());
            *plan.unsupported_reasons.entry(reason).or_default() += 1;
        }
    }
    for output in &mut plan.output_roots {
        if let Some(statement) = output.statement.as_ref()
            && let Some(function) = compiler_runtime_statement_missing_function(
                statement,
                &compiled_functions,
                &unsupported_functions,
            )
        {
            let reason = unsupported_functions
                .get(&function)
                .map(|reason| format!("function:{function}:{reason}"))
                .unwrap_or_else(|| format!("function:{function}:missing"));
            output.statement = None;
            output.unsupported_reason = Some(reason.clone());
            *plan.unsupported_reasons.entry(reason).or_default() += 1;
        }
    }
    for field in &mut plan.indexed_fields {
        if let Some(statement) = field.statement.as_ref()
            && let Some(function) = compiler_runtime_statement_missing_function(
                statement,
                &compiled_functions,
                &unsupported_functions,
            )
        {
            let reason = unsupported_functions
                .get(&function)
                .map(|reason| format!("function:{function}:{reason}"))
                .unwrap_or_else(|| format!("function:{function}:missing"));
            field.statement = None;
            field.unsupported_reason = Some(reason.clone());
            *plan.unsupported_reasons.entry(reason).or_default() += 1;
        }
    }
    plan.functions = compiled_functions
        .into_iter()
        .filter(|(name, _)| reachable_functions.supported.contains(name))
        .map(|(_, function)| function)
        .collect();
    plan
}

#[derive(Default)]
struct CompilerReachableRuntimeFunctions {
    supported: BTreeSet<String>,
    missing: BTreeSet<String>,
}

fn compiler_runtime_reachable_function_names(
    plan: &CompilerRuntimeGenericDerivedPlan,
    functions: &BTreeMap<String, CompilerRuntimeGenericFunction>,
    unsupported_functions: &BTreeMap<String, String>,
) -> CompilerReachableRuntimeFunctions {
    let mut pending = Vec::new();
    for statement in plan
        .output_roots
        .iter()
        .filter_map(|output| output.statement.as_ref())
        .chain(
            plan.root_fields
                .iter()
                .filter_map(|field| field.statement.as_ref()),
        )
        .chain(
            plan.indexed_fields
                .iter()
                .filter_map(|field| field.statement.as_ref()),
        )
    {
        statement.collect_user_function_calls(functions, unsupported_functions, &mut pending);
    }

    let mut supported = BTreeSet::new();
    let mut missing = BTreeSet::new();
    while let Some(function) = pending.pop() {
        if supported.contains(&function) || missing.contains(&function) {
            continue;
        }
        if let Some(definition) = functions.get(&function) {
            supported.insert(function.clone());
            definition.statement.collect_user_function_calls(
                functions,
                unsupported_functions,
                &mut pending,
            );
        } else {
            missing.insert(function);
        }
    }
    CompilerReachableRuntimeFunctions { supported, missing }
}

impl CompilerRuntimeGenericFunction {
    fn from_ast(
        function: &CompilerGenericDerivedFunction,
        expressions: &[AstExpr],
        functions: &BTreeSet<String>,
    ) -> Result<Self, String> {
        Ok(Self {
            name: function.name.clone(),
            args: function.args.clone(),
            statement: CompilerRuntimeGenericStatement::from_ast(
                &function.statement,
                expressions,
                functions,
            )?,
        })
    }
}

impl CompilerRuntimeGenericStatement {
    fn from_ast(
        statement: &AstStatement,
        expressions: &[AstExpr],
        functions: &BTreeSet<String>,
    ) -> Result<Self, String> {
        if statement.children.is_empty() {
            let value = statement
                .expr
                .map(|expr| {
                    CompilerRuntimeGenericExpr::from_ast(expr, expressions, functions)
                        .map(Self::Expr)
                })
                .unwrap_or(Ok(Self::Empty))?;
            if let AstStatementKind::Field { name } = &statement.kind {
                return Ok(Self::Binding {
                    name: name.clone(),
                    value: Box::new(value),
                });
            }
            return Ok(value);
        }
        if statement.expr.is_none()
            && statement.children.len() == 1
            && matches!(statement.children[0].kind, AstStatementKind::Expression)
        {
            return Self::from_ast(&statement.children[0], expressions, functions);
        }
        if statement.expr.is_some_and(|expr| {
            expressions
                .get(expr)
                .is_some_and(|expr| matches!(expr.kind, AstExprKind::Latest))
        }) {
            return statement
                .children
                .iter()
                .map(|child| Self::from_ast(child, expressions, functions))
                .collect::<Result<Vec<_>, _>>()
                .map(Self::Latest);
        }
        if statement.expr.is_none()
            && matches!(statement.kind, AstStatementKind::List { field: None, .. })
            && compiler_runtime_record_statement_children(&statement.children)
        {
            return CompilerRuntimeGenericRecordField::from_ast_children(
                &statement.children,
                expressions,
                functions,
            )
            .map(Self::Record);
        }
        if let AstStatementKind::List { field, .. } = &statement.kind {
            let value = statement
                .children
                .iter()
                .map(|child| Self::from_ast(child, expressions, functions))
                .collect::<Result<Vec<_>, _>>()
                .map(Self::List)?;
            if let Some(name) = field {
                return Ok(Self::Binding {
                    name: name.clone(),
                    value: Box::new(value),
                });
            }
            return Ok(value);
        }
        if statement.expr.is_none()
            && compiler_runtime_record_statement_children(&statement.children)
        {
            return CompilerRuntimeGenericRecordField::from_ast_children(
                &statement.children,
                expressions,
                functions,
            )
            .map(Self::Record);
        }
        if statement.expr.is_none()
            && statement.children.len() == 1
            && matches!(statement.children[0].kind, AstStatementKind::Block)
            && compiler_runtime_record_statement_children(&statement.children[0].children)
        {
            return CompilerRuntimeGenericRecordField::from_ast_children(
                &statement.children[0].children,
                expressions,
                functions,
            )
            .map(Self::Record);
        }
        if let Some(expr) = statement.expr {
            return CompilerRuntimeGenericExpr::from_ast(expr, expressions, functions).and_then(
                |expr| {
                    let value = if statement.children.is_empty() {
                        Self::Expr(expr)
                    } else {
                        statement
                            .children
                            .iter()
                            .map(|child| Self::from_ast(child, expressions, functions))
                            .collect::<Result<Vec<_>, _>>()
                            .map(|children| Self::ExprWithChildren { expr, children })?
                    };
                    if let AstStatementKind::Field { name } = &statement.kind {
                        Ok(Self::Binding {
                            name: name.clone(),
                            value: Box::new(value),
                        })
                    } else {
                        Ok(value)
                    }
                },
            );
        }
        let value = statement
            .children
            .iter()
            .map(|child| Self::from_ast(child, expressions, functions))
            .collect::<Result<Vec<_>, _>>()
            .map(Self::Block)?;
        if let AstStatementKind::Field { name } = &statement.kind {
            Ok(Self::Binding {
                name: name.clone(),
                value: Box::new(value),
            })
        } else {
            Ok(value)
        }
    }

    fn collect_user_function_calls(
        &self,
        functions: &BTreeMap<String, CompilerRuntimeGenericFunction>,
        unsupported_functions: &BTreeMap<String, String>,
        output: &mut Vec<String>,
    ) {
        match self {
            Self::Empty => {}
            Self::Expr(expr) => {
                expr.collect_user_function_calls(functions, unsupported_functions, output);
            }
            Self::Binding { value, .. } => {
                value.collect_user_function_calls(functions, unsupported_functions, output);
            }
            Self::ExprWithChildren { expr, children } => {
                expr.collect_user_function_calls(functions, unsupported_functions, output);
                for child in children {
                    child.collect_user_function_calls(functions, unsupported_functions, output);
                }
            }
            Self::Block(statements) | Self::List(statements) | Self::Latest(statements) => {
                for statement in statements {
                    statement.collect_user_function_calls(functions, unsupported_functions, output);
                }
            }
            Self::Record(fields) => {
                for field in fields {
                    field.value.collect_user_function_calls(
                        functions,
                        unsupported_functions,
                        output,
                    );
                }
            }
        }
    }
}

impl CompilerRuntimeGenericRecordField {
    fn from_ast_children(
        children: &[AstStatement],
        expressions: &[AstExpr],
        functions: &BTreeSet<String>,
    ) -> Result<Vec<Self>, String> {
        children
            .iter()
            .map(|child| {
                let name = compiler_runtime_record_statement_child_name(child)
                    .ok_or_else(|| "record_child_name".to_owned())?
                    .to_owned();
                CompilerRuntimeGenericStatement::from_ast(child, expressions, functions)
                    .map(|value| Self { name, value })
            })
            .collect()
    }
}

impl CompilerRuntimeGenericExpr {
    fn from_ast(
        expr_id: usize,
        expressions: &[AstExpr],
        functions: &BTreeSet<String>,
    ) -> Result<Self, String> {
        let expr = expressions
            .get(expr_id)
            .ok_or_else(|| "missing_expr".to_owned())?;
        match &expr.kind {
            AstExprKind::Identifier(name) => Ok(Self::Identifier(name.clone())),
            AstExprKind::Path(parts) => Ok(Self::Path(parts.clone())),
            AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => {
                Ok(Self::Text(value.clone()))
            }
            AstExprKind::Number(value) => {
                Ok(value.parse::<i64>().map(Self::Number).unwrap_or(Self::NaN))
            }
            AstExprKind::ByteLiteral { value, .. } => Ok(Self::Number(i64::from(*value))),
            AstExprKind::Bool(value) => Ok(Self::Bool(*value)),
            AstExprKind::Enum(value) | AstExprKind::Tag(value) => Ok(Self::Enum(value.clone())),
            AstExprKind::TaggedObject { tag, fields } => Ok(Self::TaggedObject {
                tag: tag.clone(),
                fields: CompilerRuntimeGenericRecordExprField::from_ast_fields(
                    fields,
                    expressions,
                    functions,
                )?,
            }),
            AstExprKind::Call { function, args } => {
                compiler_runtime_generic_call_supported(function, functions)?;
                Ok(Self::Call {
                    function: function.clone(),
                    args: CompilerRuntimeGenericArg::from_ast_args(args, expressions, functions)?,
                })
            }
            AstExprKind::Pipe { input, op, args } => {
                compiler_runtime_generic_call_supported(op, functions)?;
                Ok(Self::Pipe {
                    input: Box::new(Self::from_ast(*input, expressions, functions)?),
                    op: op.clone(),
                    args: CompilerRuntimeGenericArg::from_ast_args(args, expressions, functions)?,
                })
            }
            AstExprKind::Infix { left, op, right } => Ok(Self::Infix {
                left: Box::new(Self::from_ast(*left, expressions, functions)?),
                op: op.clone(),
                right: Box::new(Self::from_ast(*right, expressions, functions)?),
            }),
            AstExprKind::Record(fields) | AstExprKind::Object(fields) => Ok(Self::Record(
                CompilerRuntimeGenericRecordExprField::from_ast_fields(
                    fields,
                    expressions,
                    functions,
                )?,
            )),
            AstExprKind::ListLiteral { items, .. } => items
                .iter()
                .map(|item| Self::from_ast(*item, expressions, functions))
                .collect::<Result<Vec<_>, _>>()
                .map(Self::List),
            AstExprKind::BytesLiteral { size, items } => Ok(Self::Bytes {
                size: size.clone(),
                items: items
                    .iter()
                    .map(|item| Self::from_ast(*item, expressions, functions))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            AstExprKind::Then { input, output } => Ok(Self::Then {
                input: Box::new(Self::from_ast(*input, expressions, functions)?),
                output: output
                    .map(|output| Self::from_ast(output, expressions, functions).map(Box::new))
                    .transpose()?,
            }),
            AstExprKind::When { input } => Ok(Self::When {
                input: Box::new(Self::from_ast(*input, expressions, functions)?),
            }),
            AstExprKind::MatchArm { pattern, output } => Ok(Self::MatchArm {
                pattern: pattern.clone(),
                output: output
                    .map(|output| Self::from_ast(output, expressions, functions).map(Box::new))
                    .transpose()?,
            }),
            AstExprKind::Delimiter => Ok(Self::Delimiter),
            AstExprKind::Source
            | AstExprKind::Hold { .. }
            | AstExprKind::Latest
            | AstExprKind::Unknown(_) => {
                Err(compiler_runtime_generic_expr_kind_label(&expr.kind).to_owned())
            }
        }
    }

    fn collect_user_function_calls(
        &self,
        functions: &BTreeMap<String, CompilerRuntimeGenericFunction>,
        unsupported_functions: &BTreeMap<String, String>,
        output: &mut Vec<String>,
    ) {
        match self {
            Self::Call { function, args } => {
                collect_compiler_runtime_user_function_call(
                    function,
                    functions,
                    unsupported_functions,
                    output,
                );
                for arg in args {
                    arg.value
                        .collect_user_function_calls(functions, unsupported_functions, output);
                }
            }
            Self::Pipe { input, op, args } => {
                input.collect_user_function_calls(functions, unsupported_functions, output);
                collect_compiler_runtime_user_function_call(
                    op,
                    functions,
                    unsupported_functions,
                    output,
                );
                for arg in args {
                    arg.value
                        .collect_user_function_calls(functions, unsupported_functions, output);
                }
            }
            Self::Infix { left, right, .. } => {
                left.collect_user_function_calls(functions, unsupported_functions, output);
                right.collect_user_function_calls(functions, unsupported_functions, output);
            }
            Self::TaggedObject { fields, .. } | Self::Record(fields) => {
                for field in fields {
                    field.value.collect_user_function_calls(
                        functions,
                        unsupported_functions,
                        output,
                    );
                }
            }
            Self::List(items) | Self::Bytes { items, .. } => {
                for item in items {
                    item.collect_user_function_calls(functions, unsupported_functions, output);
                }
            }
            Self::Then {
                input,
                output: then_output,
            } => {
                input.collect_user_function_calls(functions, unsupported_functions, output);
                if let Some(output_expr) = then_output {
                    output_expr.collect_user_function_calls(
                        functions,
                        unsupported_functions,
                        output,
                    );
                }
            }
            Self::When { input } => {
                input.collect_user_function_calls(functions, unsupported_functions, output);
            }
            Self::MatchArm {
                output: Some(output_expr),
                ..
            } => {
                output_expr.collect_user_function_calls(functions, unsupported_functions, output);
            }
            Self::Identifier(_)
            | Self::Path(_)
            | Self::Text(_)
            | Self::Number(_)
            | Self::NaN
            | Self::Bool(_)
            | Self::Enum(_)
            | Self::MatchArm { output: None, .. }
            | Self::Delimiter => {}
        }
    }
}

impl CompilerRuntimeGenericRecordExprField {
    fn from_ast_fields(
        fields: &[AstRecordField],
        expressions: &[AstExpr],
        functions: &BTreeSet<String>,
    ) -> Result<Vec<Self>, String> {
        fields
            .iter()
            .map(|field| {
                Ok(Self {
                    name: field.name.clone(),
                    value: CompilerRuntimeGenericExpr::from_ast(
                        field.value,
                        expressions,
                        functions,
                    )?,
                    spread: field.spread,
                })
            })
            .collect()
    }
}

impl CompilerRuntimeGenericArg {
    fn from_ast_args(
        args: &[AstCallArg],
        expressions: &[AstExpr],
        functions: &BTreeSet<String>,
    ) -> Result<Vec<Self>, String> {
        args.iter()
            .map(|arg| {
                Ok(Self {
                    name: arg.name.clone(),
                    value: CompilerRuntimeGenericExpr::from_ast(arg.value, expressions, functions)?,
                })
            })
            .collect()
    }
}

fn collect_compiler_runtime_user_function_call(
    function: &str,
    functions: &BTreeMap<String, CompilerRuntimeGenericFunction>,
    unsupported_functions: &BTreeMap<String, String>,
    output: &mut Vec<String>,
) {
    if let Some(name) = resolve_compiler_runtime_generic_function_name(function, functions.keys()) {
        output.push(name);
        return;
    }
    if let Some(name) =
        resolve_compiler_runtime_generic_function_name(function, unsupported_functions.keys())
    {
        output.push(name);
    }
}

fn compiler_runtime_statement_missing_function(
    statement: &CompilerRuntimeGenericStatement,
    functions: &BTreeMap<String, CompilerRuntimeGenericFunction>,
    unsupported_functions: &BTreeMap<String, String>,
) -> Option<String> {
    let mut pending = Vec::new();
    statement.collect_user_function_calls(functions, unsupported_functions, &mut pending);
    let mut seen = BTreeSet::new();
    while let Some(function) = pending.pop() {
        if !seen.insert(function.clone()) {
            continue;
        }
        if unsupported_functions.contains_key(&function) {
            return Some(function);
        }
        let Some(definition) = functions.get(&function) else {
            return Some(function);
        };
        definition.statement.collect_user_function_calls(
            functions,
            unsupported_functions,
            &mut pending,
        );
    }
    None
}

fn resolve_compiler_runtime_generic_function_name<'a, I>(function: &str, names: I) -> Option<String>
where
    I: IntoIterator<Item = &'a String>,
{
    let names = names.into_iter().collect::<Vec<_>>();
    if names.iter().any(|name| name.as_str() == function) {
        return Some(function.to_owned());
    }
    let suffix = function.rsplit_once('/').map(|(_, suffix)| suffix)?;
    names
        .into_iter()
        .find(|name| name.as_str() == suffix)
        .cloned()
}

fn compiler_runtime_generic_call_supported(
    function: &str,
    functions: &BTreeSet<String>,
) -> Result<(), String> {
    if function.strip_prefix("Field/").is_some()
        || compiler_is_generic_render_constructor(function)
        || compiler_is_generic_world_constructor(function)
        || compiler_is_generic_solid_constructor(function)
        || compiler_is_light_constructor(function)
        || functions.contains(function)
        || function
            .rsplit_once('/')
            .is_some_and(|(_, suffix)| functions.contains(suffix))
    {
        return Ok(());
    }
    match function {
        "SOURCE"
        | "Text/empty"
        | "Text/space"
        | "Text/concat"
        | "Text/trim"
        | "Text/to_number"
        | "Text/starts_with"
        | "Text/substring"
        | "Text/length"
        | "Text/find"
        | "Text/is_empty"
        | "Text/is_not_empty"
        | "Text/to_bytes"
        | "Bytes/length"
        | "Bytes/is_empty"
        | "Bytes/get"
        | "Bytes/set"
        | "Bytes/slice"
        | "Bytes/take"
        | "Bytes/drop"
        | "Bytes/concat"
        | "Bytes/equal"
        | "Bytes/find"
        | "Bytes/starts_with"
        | "Bytes/ends_with"
        | "Bytes/zeros"
        | "Bytes/to_text"
        | "Bytes/to_hex"
        | "Bytes/from_hex"
        | "Bytes/to_base64"
        | "Bytes/from_base64"
        | "Bytes/read_unsigned"
        | "Bytes/read_signed"
        | "Bytes/write_unsigned"
        | "Bytes/write_signed"
        | "Number/min"
        | "Number/max"
        | "Number/interpolate"
        | "Number/project_time"
        | "Bool/not"
        | "Bool/and"
        | "WHEN"
        | "WHILE"
        | "List/range"
        | "List/find"
        | "List/find_value"
        | "List/chunk"
        | "List/get"
        | "List/map"
        | "List/sum"
        | "List/retain"
        | "List/every"
        | "List/any"
        | "Error/new"
        | "Error/text"
        | "Router/route"
        | "Router/go_to" => Ok(()),
        _ => Err(format!("call:{function}")),
    }
}

fn compiler_is_generic_render_constructor(function: &str) -> bool {
    matches!(
        function,
        "Document/new"
            | "Element/container"
            | "Element/stripe"
            | "Element/text"
            | "Element/label"
            | "Element/paragraph"
            | "Element/link"
            | "Element/button"
            | "Element/checkbox"
            | "Element/text_input"
            | "Scene/new"
            | "Scene/Element/stripe"
            | "Scene/Element/block"
            | "Scene/Element/text"
            | "Scene/Element/text_input"
            | "Scene/Element/checkbox"
            | "Scene/Element/label"
            | "Scene/Element/button"
            | "Scene/Element/paragraph"
            | "Scene/Element/link"
    )
}

fn compiler_is_generic_world_constructor(function: &str) -> bool {
    matches!(
        function,
        "World/new"
            | "World/camera"
            | "World/perspective_camera"
            | "World/light"
            | "World/point_light"
            | "World/material"
            | "World/transform"
            | "World/primitive"
            | "World/indexed_mesh"
            | "World/model"
            | "World/group"
            | "Camera/perspective"
            | "Light/directional"
            | "World/instance"
    )
}

fn compiler_is_generic_solid_constructor(function: &str) -> bool {
    matches!(
        function,
        "Assembly/new"
            | "Part/new"
            | "Part/instance"
            | "Solid/box"
            | "Solid/rounded_box"
            | "Solid/sphere"
            | "Solid/cylinder"
            | "Solid/cone"
            | "Solid/torus"
            | "Solid/extrude"
            | "Solid/revolve"
            | "Solid/loft"
            | "Solid/shell"
            | "Solid/union"
            | "Solid/difference"
            | "Solid/translate"
    )
}

fn compiler_is_light_constructor(function: &str) -> bool {
    matches!(
        function,
        "Light/directional" | "Light/ambient" | "Light/spot"
    )
}

fn compiler_runtime_generic_expr_kind_label(kind: &AstExprKind) -> &'static str {
    match kind {
        AstExprKind::Source => "source_expr",
        AstExprKind::Hold { .. } => "hold_expr",
        AstExprKind::Latest => "latest_expr",
        AstExprKind::When { .. } => "when_expr",
        AstExprKind::MatchArm { .. } => "match_arm_expr",
        AstExprKind::Unknown(_) => "unknown_expr",
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::TaggedObject { .. }
        | AstExprKind::Call { .. }
        | AstExprKind::Pipe { .. }
        | AstExprKind::Then { .. }
        | AstExprKind::Infix { .. }
        | AstExprKind::Object(_)
        | AstExprKind::Record(_)
        | AstExprKind::ListLiteral { .. }
        | AstExprKind::BytesLiteral { .. }
        | AstExprKind::Delimiter => "supported",
    }
}

fn compiler_runtime_record_statement_children(children: &[AstStatement]) -> bool {
    !children.is_empty()
        && children
            .iter()
            .all(|child| compiler_runtime_record_statement_child_name(child).is_some())
}

fn compiler_runtime_record_statement_child_name(statement: &AstStatement) -> Option<&str> {
    match &statement.kind {
        AstStatementKind::Field { name }
        | AstStatementKind::List {
            field: Some(name), ..
        }
        | AstStatementKind::Source {
            field: Some(name), ..
        }
        | AstStatementKind::Hold {
            field: Some(name), ..
        } => Some(name.as_str()),
        AstStatementKind::Function { .. }
        | AstStatementKind::Source { field: None, .. }
        | AstStatementKind::Hold { field: None, .. }
        | AstStatementKind::List { field: None, .. }
        | AstStatementKind::Block
        | AstStatementKind::Expression => None,
    }
}

pub fn compiler_generic_derived_plan_from_ir(ir: &TypedProgram) -> CompilerGenericDerivedPlan {
    let expressions = ir.expressions.clone();
    let functions = ir
        .functions
        .iter()
        .map(|function| CompilerGenericDerivedFunction {
            name: function.name.clone(),
            args: function.args.clone(),
            statement: function.statement.clone(),
        })
        .collect::<Vec<_>>();
    let output_roots = ir
        .output_values
        .iter()
        .map(|output| CompilerGenericDerivedOutputRoot {
            root: output.root.clone(),
            output_kind: output.output_kind.clone(),
            typed_contract_known: output.typed_contract_known,
            generic_output_port: output.generic_output_port,
            statement: output.statement.clone(),
        })
        .collect::<Vec<_>>();
    let root_fields = ir
        .derived_values
        .iter()
        .filter(|value| {
            !value.indexed
                && value.scope_id.is_none()
                && matches!(
                    value.kind,
                    boon_ir::DerivedValueKind::Pure
                        | boon_ir::DerivedValueKind::SourceEventTransform
                        | boon_ir::DerivedValueKind::ListView
                )
        })
        .map(|value| CompilerGenericDerivedRootField {
            path: value.path.clone(),
            kind: compiler_derived_value_kind(&value.kind),
            has_sources: !value.sources.is_empty(),
            statement: value.statement.clone(),
        })
        .collect::<Vec<_>>();
    let indexed_fields = ir
        .derived_values
        .iter()
        .filter(|value| {
            (value.indexed || value.scope_id.is_some())
                && matches!(
                    value.kind,
                    boon_ir::DerivedValueKind::Pure
                        | boon_ir::DerivedValueKind::SourceEventTransform
                )
        })
        .filter_map(|value| {
            let (row_scope, field) = value.path.split_once('.')?;
            let list = ir
                .row_scopes
                .iter()
                .find(|scope| scope.row_scope == row_scope)
                .map(|scope| scope.list.clone())?;
            Some(CompilerGenericDerivedIndexedField {
                list,
                row_scope: row_scope.to_owned(),
                field: field.to_owned(),
                kind: compiler_derived_value_kind(&value.kind),
                startup_recompute: value.startup_recompute,
                statement: value.statement.clone(),
            })
        })
        .collect::<Vec<_>>();
    let runtime_plan = compiler_runtime_generic_derived_plan_from_parts(
        &expressions,
        &functions,
        &output_roots,
        &root_fields,
        &indexed_fields,
    );
    CompilerGenericDerivedPlan {
        expressions,
        functions,
        output_roots,
        root_fields,
        observed_root_paths: compiler_observed_root_paths_from_ir(ir),
        indexed_fields,
        runtime_plan,
    }
}

pub fn compiler_list_projections_from_ir(ir: &TypedProgram) -> Vec<CompilerListProjection> {
    ir.list_projections
        .iter()
        .filter_map(|projection| {
            let (columns, rows) = match &projection.kind {
                boon_ir::ListProjectionKind::Chunk { size, .. } => {
                    let columns = (*size)?;
                    (columns, 0)
                }
                boon_ir::ListProjectionKind::Find { .. } => (0, 0),
            };
            Some(CompilerListProjection {
                target: projection.target.clone(),
                list: projection.list.clone(),
                columns,
                rows,
                kind: match &projection.kind {
                    boon_ir::ListProjectionKind::Chunk {
                        item_field,
                        label_field,
                        ..
                    } => CompilerListProjectionKind::Chunk {
                        item_field: item_field.clone(),
                        label_field: label_field.clone(),
                    },
                    boon_ir::ListProjectionKind::Find { field, value } => {
                        CompilerListProjectionKind::Find {
                            field: field.clone(),
                            value: value.clone(),
                        }
                    }
                },
            })
        })
        .collect()
}

impl CompiledMachinePlanFromSource {
    pub fn report_context(&self) -> CompiledSourceReportContext {
        compiled_source_report_context(
            &self.parsed,
            self.ir.graph_node_count,
            self.load_pipeline_profile.clone(),
        )
    }
}

impl CompiledRuntimeIrFromSource {
    pub fn report_context(&self) -> CompiledSourceReportContext {
        compiled_source_report_context(
            &self.parsed,
            self.ir.graph_node_count,
            self.load_pipeline_profile.clone(),
        )
    }
}

impl CompiledFullIrFromSource {
    pub fn report_context(&self) -> CompiledSourceReportContext {
        compiled_source_report_context(
            &self.parsed,
            self.ir.graph_node_count,
            self.load_pipeline_profile.clone(),
        )
    }
}

pub fn compile_typed_program(
    program: &TypedProgram,
    target_profile: TargetProfile,
) -> Result<MachinePlan, PlanError> {
    legacy_backend::compile_typed_program(program, target_profile)
}

pub fn compiler_parsed_document(parsed: &ParsedProgram) -> Option<DocumentAst> {
    boon_parser::parsed_document(parsed)
}

pub fn compiler_ir_debug_tables_from_ir(ir: &TypedProgram) -> JsonValue {
    debug_tables(ir)
}

pub fn compiler_ir_debug_report_from_path(source_path: &Path) -> CompilerResult<JsonValue> {
    let compiled = compile_source_path_to_full_ir(source_path)?;
    let ir = compiled.ir;
    Ok(json!({
        "status": "pass",
        "measurement_mode": "diagnostic",
        "program_kind": "generic",
        "expression_count": ir.expression_count,
        "graph_node_count": ir.graph_node_count,
        "semantic_index": ir.semantic_index.report(),
        "hidden_identity_verified": ir.hidden_identity_verified,
        "static_schedule_verified": ir.static_schedule_verified,
        "nodes": ir.nodes,
        "debug_tables": compiler_ir_debug_tables_from_ir(&ir),
    }))
}

pub fn compiler_runtime_program_from_ir(ir: &TypedProgram) -> CompilerRuntimeProgram {
    CompilerRuntimeProgram {
        symbols: compiler_runtime_symbols_from_ir(ir),
        unsupported_diagnostics: compiler_unsupported_runtime_diagnostics_from_ir(ir),
        storage_root_slots: compiler_storage_root_slots_from_ir(ir),
        storage_indexed_row_initial_resets: compiler_storage_indexed_row_initial_resets_from_ir(ir),
        storage_list_slots: compiler_storage_list_slots_from_ir(ir),
        storage_row_templates: compiler_storage_row_templates_from_ir(ir),
        storage_initial_rows: compiler_storage_initial_rows_from_ir(ir),
        storage_indexed_derived_fields: compiler_storage_indexed_derived_fields_from_ir(ir),
        scalar_equations: compiler_scalar_equation_plan_from_ir(ir),
        derived_equations: compiler_derived_equation_plan_from_ir(ir),
        generic_derived_plan: compiler_generic_derived_plan_from_ir(ir),
        list_operations: compiler_list_operations_from_ir(ir),
        list_projections: compiler_list_projections_from_ir(ir),
        root_state_paths: compiler_root_state_paths_from_ir(ir),
        list_summary_fields: compiler_list_summary_fields_from_ir(ir),
        dynamic_list_view_lists: compiler_dynamic_list_view_lists_from_ir(ir),
        observed_root_paths: compiler_observed_root_paths_from_ir(ir),
        projection_storage: compiler_document_projection_storage_resolutions_from_ir(ir),
        document_render_slots: compiler_document_render_slots_from_ir(ir),
        field_slot_collision_diagnostics: compiler_field_slot_collision_diagnostics_from_ir(ir),
        source_route_root_targets: compiler_source_route_root_targets_from_ir(ir),
        source_route_sources: compiler_source_route_sources_from_ir(ir),
        source_route_bool_facts: compiler_source_route_bool_facts_from_ir(ir),
        source_route_router_targets: compiler_source_route_router_targets_from_ir(ir),
        source_route_root_text_transform_targets:
            compiler_source_route_root_text_transform_targets_from_ir(ir),
        static_analysis: CompilerStaticProgramAnalysis::from_ir_parts(
            ir,
            Vec::new(),
            BTreeMap::new(),
        ),
        list_source_bindings: compiler_list_source_bindings_from_ir(ir),
        source_payload_counts: compiler_source_payload_counts_from_ir(ir),
        storage_layout_counts: compiler_typed_storage_layout_counts_from_ir(ir),
        inventory_counts: compiler_typed_program_inventory_counts_from_ir(ir),
        program_metadata: compiler_typed_program_report_metadata_from_ir(ir),
        typecheck_metadata: compiler_typecheck_report_metadata_from_ir(ir),
        runtime_profile_metadata: compiler_runtime_profile_metadata_from_ir(ir),
        ir_debug_tables: compiler_ir_debug_tables_from_ir(ir),
    }
}

pub fn compile_source_path_to_machine_plan(
    source_path: &Path,
    target_profile: TargetProfile,
) -> CompilerResult<CompiledMachinePlanFromSource> {
    let total_started = Instant::now();
    let parse_started = Instant::now();
    let parsed = parse_source_path_or_manifest_project(source_path)?;
    let parse_ms = elapsed_ms(parse_started);
    let lower_started = Instant::now();
    let (ir, lower_profile) = lower_profiled(&parsed)?;
    let lower_ms = elapsed_ms(lower_started);
    let verify_started = Instant::now();
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    let verify_ms = elapsed_ms(verify_started);
    let compile_started = Instant::now();
    let plan = compile_typed_program(&ir, target_profile)?;
    let compile_ms = elapsed_ms(compile_started);
    let load_pipeline_profile = json!({
        "owner": "boon_compiler",
        "cache_hit": false,
        "source_unit_count": parsed.files.len(),
        "parse_ms": parse_ms,
        "lower_ms": lower_ms,
        "lower_profile": lower_profile,
        "verify_ms": verify_ms,
        "compile_ms": compile_ms,
        "expression_count": ir.expression_count,
        "graph_node_count": ir.graph_node_count,
        "total_ms": elapsed_ms(total_started)
    });
    Ok(CompiledMachinePlanFromSource {
        parsed,
        ir,
        plan,
        load_pipeline_profile,
    })
}

pub fn compile_source_path_to_runtime_ir(
    source_path: &Path,
) -> CompilerResult<CompiledRuntimeIrFromSource> {
    let total_started = Instant::now();
    let parse_started = Instant::now();
    let parsed = parse_source_path_or_manifest_project(source_path)?;
    let parse_ms = elapsed_ms(parse_started);
    compile_parsed_to_runtime_ir(parsed, parse_ms, total_started)
}

pub fn compile_source_path_to_full_ir(
    source_path: &Path,
) -> CompilerResult<CompiledFullIrFromSource> {
    let total_started = Instant::now();
    let parse_started = Instant::now();
    let parsed = parse_source_path_or_manifest_project(source_path)?;
    let parse_ms = elapsed_ms(parse_started);
    compile_parsed_to_full_ir(parsed, parse_ms, total_started)
}

pub fn compile_source_text_to_runtime_ir(
    source_label: &str,
    source_text: &str,
) -> CompilerResult<CompiledRuntimeIrFromSource> {
    let total_started = Instant::now();
    let parse_started = Instant::now();
    let parsed = parse_source(source_label.to_owned(), source_text.to_owned())?;
    let parse_ms = elapsed_ms(parse_started);
    compile_parsed_to_runtime_ir(parsed, parse_ms, total_started)
}

pub fn compile_source_text_to_full_ir(
    source_label: &str,
    source_text: &str,
) -> CompilerResult<CompiledFullIrFromSource> {
    let total_started = Instant::now();
    let parse_started = Instant::now();
    let parsed = parse_source(source_label.to_owned(), source_text.to_owned())?;
    let parse_ms = elapsed_ms(parse_started);
    compile_parsed_to_full_ir(parsed, parse_ms, total_started)
}

pub fn compile_source_units_to_runtime_ir(
    source_label: &str,
    units: &[CompilerSourceUnit],
) -> CompilerResult<CompiledRuntimeIrFromSource> {
    let total_started = Instant::now();
    let parse_started = Instant::now();
    let parsed = parse_source_units(source_label, units)?;
    let parse_ms = elapsed_ms(parse_started);
    compile_parsed_to_runtime_ir(parsed, parse_ms, total_started)
}

pub fn compile_source_units_to_full_ir(
    source_label: &str,
    units: &[CompilerSourceUnit],
) -> CompilerResult<CompiledFullIrFromSource> {
    let total_started = Instant::now();
    let parse_started = Instant::now();
    let parsed = parse_source_units(source_label, units)?;
    let parse_ms = elapsed_ms(parse_started);
    compile_parsed_to_full_ir(parsed, parse_ms, total_started)
}

pub fn compile_parsed_program_to_runtime_ir(
    parsed: ParsedProgram,
) -> CompilerResult<CompiledRuntimeIrFromSource> {
    compile_parsed_to_runtime_ir(parsed, 0.0, Instant::now())
}

pub fn compile_parsed_program_to_full_ir(
    parsed: ParsedProgram,
) -> CompilerResult<CompiledFullIrFromSource> {
    compile_parsed_to_full_ir(parsed, 0.0, Instant::now())
}

fn compile_parsed_to_runtime_ir(
    parsed: ParsedProgram,
    parse_ms: f64,
    total_started: Instant,
) -> CompilerResult<CompiledRuntimeIrFromSource> {
    let lower_started = Instant::now();
    let (ir, lower_profile) = lower_runtime_profiled(&parsed)?;
    let lower_ms = elapsed_ms(lower_started);
    let verify_started = Instant::now();
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    let verify_ms = elapsed_ms(verify_started);
    let runtime_program = compiler_runtime_program_from_ir(&ir);
    let load_pipeline_profile = json!({
        "owner": "boon_compiler",
        "surface": "runtime-ir",
        "cache_hit": false,
        "source_unit_count": parsed.files.len(),
        "parse_ms": parse_ms,
        "lower_ms": lower_ms,
        "lower_profile": lower_profile,
        "verify_ms": verify_ms,
        "expression_count": ir.expression_count,
        "graph_node_count": ir.graph_node_count,
        "total_ms": elapsed_ms(total_started)
    });
    Ok(CompiledRuntimeIrFromSource {
        parsed,
        ir,
        runtime_program,
        load_pipeline_profile,
    })
}

fn compile_parsed_to_full_ir(
    parsed: ParsedProgram,
    parse_ms: f64,
    total_started: Instant,
) -> CompilerResult<CompiledFullIrFromSource> {
    let lower_started = Instant::now();
    let (ir, lower_profile) = lower_profiled(&parsed)?;
    let lower_ms = elapsed_ms(lower_started);
    let verify_started = Instant::now();
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    let verify_ms = elapsed_ms(verify_started);
    let runtime_program = compiler_runtime_program_from_ir(&ir);
    let load_pipeline_profile = json!({
        "owner": "boon_compiler",
        "surface": "full-ir",
        "cache_hit": false,
        "source_unit_count": parsed.files.len(),
        "parse_ms": parse_ms,
        "lower_ms": lower_ms,
        "lower_profile": lower_profile,
        "verify_ms": verify_ms,
        "expression_count": ir.expression_count,
        "graph_node_count": ir.graph_node_count,
        "total_ms": elapsed_ms(total_started)
    });
    Ok(CompiledFullIrFromSource {
        parsed,
        ir,
        runtime_program,
        load_pipeline_profile,
    })
}

fn parse_source_units(
    source_label: &str,
    units: &[CompilerSourceUnit],
) -> CompilerResult<ParsedProgram> {
    Ok(if let [unit] = units {
        parse_source(unit.path.clone(), unit.source.clone())?
    } else {
        parse_project(
            source_label.to_owned(),
            units
                .iter()
                .map(|unit| (unit.path.clone(), unit.source.clone())),
        )?
    })
}

fn compiled_source_report_context(
    parsed: &ParsedProgram,
    graph_node_count: usize,
    load_pipeline_profile: JsonValue,
) -> CompiledSourceReportContext {
    let source_hash = parsed_program_hash(parsed);
    CompiledSourceReportContext {
        source_hash: source_hash.clone(),
        source_units: parsed
            .files
            .iter()
            .map(|file| CompilerSourceUnit {
                path: file.path.clone(),
                source: file.source.clone(),
            })
            .collect(),
        source_files: parsed.files.iter().map(|file| file.path.clone()).collect(),
        program_hash: source_hash,
        program_kind: parsed.kind.as_str().to_owned(),
        program_file_count: parsed.files.len(),
        graph_node_count,
        load_pipeline_profile,
    }
}

fn parsed_program_hash(parsed: &ParsedProgram) -> String {
    if parsed.files.len() == 1 {
        return sha256_bytes(parsed.files[0].source.as_bytes());
    }
    let mut bytes = Vec::new();
    for file in &parsed.files {
        bytes.extend_from_slice(file.path.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(file.source.as_bytes());
        bytes.push(0xff);
    }
    sha256_bytes(&bytes)
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn compiler_source_units_for_path(path: &Path) -> CompilerResult<Vec<CompilerSourceUnit>> {
    compiler_source_units_for_files(compiler_source_files_for_path(path)?)
}

pub fn compiler_source_units_for_manifest_source(
    source: &str,
    source_files: &[String],
) -> CompilerResult<Vec<CompilerSourceUnit>> {
    compiler_source_units_for_files(compiler_source_files_for_manifest_source(
        source,
        source_files,
    ))
}

pub fn compiler_source_files_for_path(path: &Path) -> CompilerResult<Vec<PathBuf>> {
    source_files_for_path(path)
}

pub fn compiler_source_files_for_manifest_source(
    source: &str,
    source_files: &[String],
) -> Vec<PathBuf> {
    source_files_for_manifest_source(source, source_files)
}

pub fn compiler_source_text_for_path(path: &Path) -> CompilerResult<String> {
    let source_path = resolve_repo_file(path);
    let entries = example_manifest_entries().unwrap_or_default();
    for entry in entries {
        let entry_source = resolve_repo_file(&entry.source);
        if paths_match(&entry_source, &source_path) {
            return Ok(fs::read_to_string(entry_source)?);
        }
    }
    Ok(fs::read_to_string(source_path)?)
}

pub fn compiler_source_text_for_manifest_source(source: &str) -> CompilerResult<String> {
    Ok(fs::read_to_string(resolve_repo_file(source))?)
}

pub fn parse_scenario_file<T>(path: &Path) -> CompilerResult<T>
where
    T: DeserializeOwned,
{
    let text = fs::read_to_string(resolve_repo_file(path))?;
    Ok(toml::from_str(&text)?)
}

fn compiler_source_units_for_files(files: Vec<PathBuf>) -> CompilerResult<Vec<CompilerSourceUnit>> {
    files
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(&path)?;
            Ok(CompilerSourceUnit {
                path: path.display().to_string(),
                source,
            })
        })
        .collect()
}

fn parse_source_path_or_manifest_project(source_path: &Path) -> CompilerResult<ParsedProgram> {
    let units = compiler_source_units_for_path(source_path)?;
    if units.len() <= 1 {
        let source = units
            .first()
            .map(|unit| unit.source.clone())
            .unwrap_or_default();
        return Ok(parse_source(source_path.display().to_string(), source)?);
    }
    Ok(parse_project(
        source_path.display().to_string(),
        units.into_iter().map(|unit| (unit.path, unit.source)),
    )?)
}

#[derive(Clone, Debug, Deserialize)]
struct ExampleManifest {
    #[serde(default)]
    example: Vec<ExampleManifestEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct ExampleManifestEntry {
    id: String,
    label: String,
    source: String,
    #[serde(default)]
    source_files: Vec<String>,
    scenario: String,
    budget: String,
}

fn source_files_for_path(source_path: &Path) -> CompilerResult<Vec<PathBuf>> {
    let source_path = resolve_repo_file(source_path);
    let entries = example_manifest_entries().unwrap_or_default();
    for entry in entries {
        let entry_source = resolve_repo_file(&entry.source);
        if paths_match(&entry_source, &source_path) {
            return Ok(source_files_for_entry(&entry));
        }
    }
    Ok(vec![source_path])
}

fn example_manifest_entries() -> CompilerResult<Vec<ExampleManifestEntry>> {
    let path = resolve_repo_file("examples/manifest.toml");
    let manifest_text = fs::read_to_string(&path)?;
    let manifest: ExampleManifest = toml::from_str(&manifest_text)?;
    validate_example_manifest(&path, &manifest)?;
    Ok(manifest.example)
}

fn validate_example_manifest(path: &Path, manifest: &ExampleManifest) -> CompilerResult<()> {
    if manifest.example.is_empty() {
        return Err(format!("example manifest `{}` has no entries", path.display()).into());
    }
    let mut ids = BTreeSet::new();
    for entry in &manifest.example {
        if entry.id.trim().is_empty() {
            return Err(format!("example manifest `{}` has an empty id", path.display()).into());
        }
        if !ids.insert(entry.id.clone()) {
            return Err(format!(
                "example manifest `{}` has duplicate id `{}`",
                path.display(),
                entry.id
            )
            .into());
        }
        for value in [&entry.label, &entry.source, &entry.scenario, &entry.budget] {
            if value.trim().is_empty() {
                return Err(format!(
                    "example manifest `{}` entry `{}` has an empty required field",
                    path.display(),
                    entry.id
                )
                .into());
            }
        }
    }
    Ok(())
}

fn source_files_for_entry(entry: &ExampleManifestEntry) -> Vec<PathBuf> {
    source_files_for_manifest_source(&entry.source, &entry.source_files)
}

fn source_files_for_manifest_source(source: &str, source_files: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if source_files.is_empty() {
        files.push(resolve_repo_file(source));
    } else {
        for relative in source_files {
            files.push(resolve_repo_file(relative));
        }
        let source_path = resolve_repo_file(source);
        if !files.iter().any(|path| paths_match(path, &source_path)) {
            files.push(source_path);
        }
    }
    files
}

fn paths_match(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .canonicalize()
            .ok()
            .zip(right.canonicalize().ok())
            .is_some_and(|(left, right)| left == right)
}

fn resolve_repo_file(relative: impl AsRef<Path>) -> PathBuf {
    let relative = relative.as_ref();
    if relative.exists() {
        return relative.to_path_buf();
    }
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            let candidate = ancestor.join(relative);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    relative.to_path_buf()
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_facade_produces_stable_counter_plan() {
        let source = include_str!("../../../examples/counter.bn");
        let parsed =
            boon_parser::parse_source("examples/counter.bn".to_owned(), source.to_owned()).unwrap();
        let ir = boon_ir::lower(&parsed).unwrap();

        let facade_plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
        let repeated_plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

        assert_eq!(
            boon_plan::plan_sha256(&facade_plan).unwrap(),
            boon_plan::plan_sha256(&repeated_plan).unwrap()
        );
    }

    #[test]
    fn compiler_facade_loads_source_path_to_machine_plan() {
        let compiled = compile_source_path_to_machine_plan(
            Path::new("../../examples/bytes_length_plan_ops.bn"),
            TargetProfile::SoftwareDefault,
        )
        .unwrap();

        assert_eq!(compiled.parsed.files.len(), 1);
        assert_eq!(
            compiled.plan.capability_summary.cpu_plan_executor_complete,
            true
        );
        assert_eq!(compiled.load_pipeline_profile["owner"], "boon_compiler");
    }

    #[test]
    fn compiler_facade_owns_compiled_source_report_context() {
        let compiled = compile_source_path_to_machine_plan(
            Path::new("../../examples/bytes_length_plan_ops.bn"),
            TargetProfile::SoftwareDefault,
        )
        .unwrap();
        let context = compiled.report_context();

        assert_eq!(context.program_kind, "generic");
        assert_eq!(context.program_file_count, 1);
        assert_eq!(context.source_files.len(), 1);
        assert_eq!(context.source_units.len(), 1);
        assert_eq!(context.source_units[0].path, context.source_files[0]);
        assert!(context.source_units[0].source.contains("Bytes/length"));
        assert_eq!(context.program_hash, context.source_hash);
        assert_eq!(context.graph_node_count, compiled.ir.graph_node_count);
        assert_eq!(context.load_pipeline_profile["owner"], "boon_compiler");
    }

    #[test]
    fn compiler_facade_owns_manifest_source_units_for_multifile_examples() {
        let units = compiler_source_units_for_path(Path::new("../../examples/cells.bn")).unwrap();

        assert!(units.len() > 1);
        assert!(
            units
                .iter()
                .any(|unit| unit.path.ends_with("examples/cells/model.bn"))
        );
        assert!(
            units
                .iter()
                .any(|unit| unit.path.ends_with("examples/cells.bn"))
        );

        let source = compiler_source_text_for_path(Path::new("../../examples/cells.bn")).unwrap();
        assert!(source.contains("cells_app()"));
    }

    #[test]
    fn compiler_facade_owns_manifest_source_units_from_entry_fields() {
        let source_files = vec![
            "examples/cells/defaults.bn".to_owned(),
            "examples/cells/formula.bn".to_owned(),
            "examples/cells/cell.bn".to_owned(),
            "examples/cells/model.bn".to_owned(),
            "examples/cells/columns.bn".to_owned(),
            "examples/cells/store.bn".to_owned(),
            "examples/cells/view.bn".to_owned(),
            "examples/cells.bn".to_owned(),
        ];
        let units =
            compiler_source_units_for_manifest_source("examples/cells.bn", &source_files).unwrap();

        assert_eq!(units.len(), source_files.len());
        assert_eq!(
            compiler_source_text_for_manifest_source("examples/cells.bn")
                .unwrap()
                .contains("cells_app()"),
            true
        );
    }

    #[test]
    fn compiler_facade_loads_runtime_ir_from_source_units() {
        let units = vec![CompilerSourceUnit {
            path: "examples/counter.bn".to_owned(),
            source: include_str!("../../../examples/counter.bn").to_owned(),
        }];
        let compiled = compile_source_units_to_runtime_ir("examples/counter.bn", &units).unwrap();

        assert_eq!(compiled.parsed.files.len(), 1);
        assert!(compiled.ir.expression_count > 0);
        assert_eq!(compiled.load_pipeline_profile["owner"], "boon_compiler");
        assert_eq!(compiled.load_pipeline_profile["surface"], "runtime-ir");
    }

    #[test]
    fn compiler_facade_loads_full_ir_from_source_units() {
        let units = vec![CompilerSourceUnit {
            path: "examples/counter.bn".to_owned(),
            source: include_str!("../../../examples/counter.bn").to_owned(),
        }];
        let compiled = compile_source_units_to_full_ir("examples/counter.bn", &units).unwrap();

        assert_eq!(compiled.parsed.files.len(), 1);
        assert!(compiled.ir.expression_count > 0);
        assert_eq!(compiled.load_pipeline_profile["owner"], "boon_compiler");
        assert_eq!(compiled.load_pipeline_profile["surface"], "full-ir");
    }

    #[test]
    fn compiler_facade_owns_scenario_file_decode() {
        #[derive(Debug, Deserialize)]
        struct ScenarioLite {
            name: String,
            step: Vec<ScenarioStepLite>,
        }

        #[derive(Debug, Deserialize)]
        struct ScenarioStepLite {
            id: String,
        }

        let scenario: ScenarioLite =
            parse_scenario_file(Path::new("../../examples/counter.scn")).unwrap();

        assert_eq!(scenario.name, "generic");
        assert!(
            scenario
                .step
                .iter()
                .any(|step| step.id == "press-increment")
        );
    }

    #[test]
    fn compiler_facade_lowers_parsed_program_to_runtime_ir() {
        let source = include_str!("../../../examples/counter.bn");
        let parsed =
            boon_parser::parse_source("examples/counter.bn".to_owned(), source.to_owned()).unwrap();
        let compiled = compile_parsed_program_to_runtime_ir(parsed).unwrap();

        assert!(compiled.ir.expression_count > 0);
        assert_eq!(compiled.load_pipeline_profile["owner"], "boon_compiler");
        assert_eq!(compiled.load_pipeline_profile["surface"], "runtime-ir");
        assert_eq!(compiled.load_pipeline_profile["parse_ms"], 0.0);
    }
}
