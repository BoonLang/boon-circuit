use boon_parser::{
    AstCallArg, AstExpr, AstExprKind, AstRecordField, AstStatement, AstStatementKind,
    ParsedProgram, ParserItem as AstItem, ProgramKind,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypedProgram {
    pub kind: ProgramKind,
    pub expression_count: usize,
    pub expressions: Vec<AstExpr>,
    pub expression_coverage: ExpressionCoverage,
    pub semantic_index: SemanticIndex,
    pub graph_node_count: usize,
    pub nodes: Vec<IrNode>,
    pub row_scopes: Vec<RowScope>,
    pub sources: Vec<SourcePort>,
    pub state_cells: Vec<StateCell>,
    pub lists: Vec<ListMemory>,
    pub derived_values: Vec<DerivedValue>,
    pub dependencies: Vec<DependencyEdge>,
    pub possible_causes: Vec<PossibleCause>,
    pub update_branches: Vec<UpdateBranch>,
    pub list_operations: Vec<ListOperation>,
    pub list_projections: Vec<ListProjection>,
    pub functions: Vec<FunctionDefinition>,
    pub view_bindings: Vec<ViewBinding>,
    pub typecheck_report: boon_typecheck::TypeCheckReport,
    pub hidden_identity_verified: bool,
    pub static_schedule_verified: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExprId(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScopeId(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceId(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StateId(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ListId(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FieldId(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ViewBindingId(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceUnitId(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FunctionId(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DiagnosticSpanId(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SemanticSymbolId(pub usize);

impl ExprId {
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl NodeId {
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl ScopeId {
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl SourceId {
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl StateId {
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl ListId {
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl FieldId {
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl ViewBindingId {
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl SourceUnitId {
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl FunctionId {
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl DiagnosticSpanId {
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl SemanticSymbolId {
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl fmt::Display for ExprId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Display for ScopeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Display for StateId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Display for ListId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Display for FieldId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Display for ViewBindingId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Display for SourceUnitId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Display for FunctionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Display for DiagnosticSpanId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Display for SemanticSymbolId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SemanticIndex {
    pub version: u32,
    pub computed_from: String,
    pub parser_policy_phase: String,
    pub reuse_key: String,
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

impl SemanticIndex {
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "present": true,
            "version": self.version,
            "computed_from": self.computed_from,
            "parser_policy_phase": self.parser_policy_phase,
            "reuse_key": self.reuse_key,
            "source_unit_count": self.source_units.len(),
            "source_count": self.sources.len(),
            "list_count": self.lists.len(),
            "row_scope_count": self.row_scopes.len(),
            "function_count": self.functions.len(),
            "field_count": self.fields.len(),
            "view_binding_count": self.view_bindings.len(),
            "diagnostic_span_count": self.diagnostic_spans.len(),
            "symbol_count": self.symbols.len(),
            "symbol_categories": semantic_symbol_category_counts(&self.symbols),
            "readiness": &self.readiness,
            "reuse": &self.reuse,
        })
    }
}

fn semantic_symbol_category_counts(symbols: &[SemanticSymbolEntry]) -> BTreeMap<&str, usize> {
    let mut counts = BTreeMap::new();
    for symbol in symbols {
        *counts.entry(symbol.category.as_str()).or_insert(0) += 1;
    }
    counts
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExpressionCoverage {
    pub computed_from: String,
    pub ast_expression_count: usize,
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
    pub scoped: bool,
    pub scope_id: Option<ScopeId>,
    pub payload_schema: SourcePayloadSchema,
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
    pub address_lookup_field: Option<String>,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum SourcePayloadField {
    Address,
    Key,
    Named(String),
    Text,
}

impl SourcePayloadField {
    fn from_name(name: &str) -> Self {
        match name {
            "address" => Self::Address,
            "key" => Self::Key,
            "text" => Self::Text,
            _ => Self::Named(name.to_owned()),
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Address => "address",
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
    pub scope_id: Option<ScopeId>,
    pub hold_name: String,
    pub initial_value: InitialValue,
    pub indexed: bool,
    pub source_line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum InitialValue {
    Text { value: String },
    Number { value: i64 },
    Bool { value: bool },
    Enum { value: String },
    RootInitialField { path: String },
    RowInitialField { path: String },
    Unknown { summary: String },
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
    Find {
        field: String,
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
    pub path: String,
    pub kind: DerivedValueKind,
    pub sources: Vec<String>,
    pub indexed: bool,
    pub scope_id: Option<ScopeId>,
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
    pub name: String,
    pub args: Vec<String>,
    pub statement: AstStatement,
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
pub struct UpdateBranch {
    pub target: String,
    pub source: String,
    pub expression: UpdateExpression,
    pub indexed: bool,
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
    NumberInfix {
        left: String,
        op: String,
        right: String,
    },
    MatchNumberInfixConst {
        left: String,
        op: String,
        right: String,
        arms: Vec<UpdateValueMatchArm>,
    },
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
    MatchConst {
        input: String,
        arms: Vec<UpdateMatchArm>,
    },
    MatchValueConst {
        input: String,
        arms: Vec<UpdateValueMatchArm>,
    },
    MatchNumberInfixConst {
        left: String,
        op: String,
        right: String,
        arms: Vec<UpdateValueMatchArm>,
    },
    ListFindValue {
        list: String,
        field: String,
        expected: Box<UpdateValueExpression>,
        target: String,
        fallback: Option<Box<UpdateValueExpression>>,
    },
    Unknown {
        summary: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListOperation {
    pub list: String,
    pub kind: ListOperationKind,
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
    pub kind: ViewBindingKind,
    pub scope_id: Option<ScopeId>,
    pub source_id: Option<SourceId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ViewBindingKind {
    Data,
    Source,
    Target,
}

pub fn lower(program: &ParsedProgram) -> Result<TypedProgram, String> {
    Ok(lower_profiled(program)?.0)
}

pub fn lower_profiled(
    program: &ParsedProgram,
) -> Result<(TypedProgram, serde_json::Value), String> {
    lower_profiled_with_typecheck(program, true)
}

pub fn lower_runtime_profiled(
    program: &ParsedProgram,
) -> Result<(TypedProgram, serde_json::Value), String> {
    lower_profiled_with_typecheck(program, false)
}

fn lower_profiled_with_typecheck(
    program: &ParsedProgram,
    include_type_hints: bool,
) -> Result<(TypedProgram, serde_json::Value), String> {
    let total_started = Instant::now();
    let typecheck_started = Instant::now();
    let (typecheck_report, typecheck_profile) = if include_type_hints {
        boon_typecheck::check_profiled(program)
    } else {
        boon_typecheck::check_runtime_profiled(program)
    };
    let typecheck_ms = lower_elapsed_ms(typecheck_started);
    if typecheck_report.has_errors() {
        let messages = typecheck_report
            .diagnostics
            .iter()
            .map(|diagnostic| format!("line {}: {}", diagnostic.line, diagnostic.message))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "typecheck failed with {} diagnostic(s): {messages}",
            typecheck_report.diagnostics.len(),
        ));
    }
    let nodes_started = Instant::now();
    let nodes = source_driven_nodes(program);
    let nodes_ms = lower_elapsed_ms(nodes_started);
    let fields_started = Instant::now();
    let fields = typed_field_defs(program);
    let fields_ms = lower_elapsed_ms(fields_started);
    let direct_sources_started = Instant::now();
    let direct_sources = direct_source_refs_by_path(&fields, program);
    let direct_sources_ms = lower_elapsed_ms(direct_sources_started);
    let row_scopes_started = Instant::now();
    let row_scopes = row_scopes(program);
    let row_scopes_ms = lower_elapsed_ms(row_scopes_started);
    let sources_started = Instant::now();
    let sources = program
        .source_ports
        .iter()
        .enumerate()
        .map(|(id, source)| SourcePort {
            id: SourceId(id),
            scoped: source.scoped,
            scope_id: scope_id_for_path(&row_scopes, &source.path),
            payload_schema: source_payload_schema(program, &fields, &direct_sources, &source.path),
            path: source.path.clone(),
        })
        .collect::<Vec<_>>();
    let sources_ms = lower_elapsed_ms(sources_started);
    let state_cells_started = Instant::now();
    let state_cells = program
        .state_cells
        .iter()
        .enumerate()
        .map(|(id, cell)| StateCell {
            id: StateId(id),
            path: cell.path.clone(),
            scope_id: scope_id_for_path(&row_scopes, &cell.path),
            hold_name: cell.hold_name.clone(),
            initial_value: fields
                .iter()
                .find(|field| field.path == cell.path)
                .map(|field| field_initial_value(field, &row_scopes))
                .unwrap_or_else(|| InitialValue::Unknown {
                    summary: "missing initial value".to_owned(),
                }),
            indexed: cell.indexed,
            source_line: cell.line,
        })
        .collect::<Vec<_>>();
    let state_cells_ms = lower_elapsed_ms(state_cells_started);
    let verify_cycles_started = Instant::now();
    verify_combinational_field_cycles(&fields, &state_cells)?;
    let verify_cycles_ms = lower_elapsed_ms(verify_cycles_started);
    let lists_started = Instant::now();
    let lists = program
        .list_memories
        .iter()
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
    if nodes
        .iter()
        .any(|node| matches!(node.kind, IrNodeKind::ListMap) && !node.indexed)
    {
        return Err("List/map node must be indexed".to_owned());
    }
    let dependencies_started = Instant::now();
    let mut candidate_sources = CandidateSourceIndex::new(&fields, &direct_sources);
    let dependencies = dependency_edges(program, &state_cells, &mut candidate_sources);
    let dependencies_ms = lower_elapsed_ms(dependencies_started);
    let possible_causes_started = Instant::now();
    let possible_causes = possible_causes(&state_cells, &mut candidate_sources);
    let possible_causes_ms = lower_elapsed_ms(possible_causes_started);
    let update_branches_started = Instant::now();
    let update_branches = update_branches(
        program,
        &state_cells,
        &fields,
        &direct_sources,
        &mut candidate_sources,
    );
    let update_branches_ms = lower_elapsed_ms(update_branches_started);
    let list_operations_started = Instant::now();
    let list_operations = list_operations(program);
    let list_operations_ms = lower_elapsed_ms(list_operations_started);
    let list_projections_started = Instant::now();
    let list_projections = list_projections(program);
    let list_projections_ms = lower_elapsed_ms(list_projections_started);
    let functions_started = Instant::now();
    let functions = function_definitions(program);
    let functions_ms = lower_elapsed_ms(functions_started);
    let derived_values_started = Instant::now();
    let derived_values =
        derived_values(program, &row_scopes, &fields, &state_cells, &direct_sources);
    let derived_values_ms = lower_elapsed_ms(derived_values_started);
    let view_bindings_started = Instant::now();
    let view_bindings = view_bindings(program, &row_scopes, &sources, &typecheck_report);
    let view_bindings_ms = lower_elapsed_ms(view_bindings_started);
    let expression_coverage_started = Instant::now();
    let expression_coverage = expression_coverage(
        program,
        &nodes,
        &state_cells,
        &lists,
        &derived_values,
        &update_branches,
        &list_operations,
    );
    let expression_coverage_ms = lower_elapsed_ms(expression_coverage_started);
    let semantic_index_started = Instant::now();
    let semantic_index = semantic_index(
        program,
        &fields,
        &row_scopes,
        &sources,
        &state_cells,
        &lists,
        &functions,
        &view_bindings,
        &typecheck_report,
    );
    let semantic_index_ms = lower_elapsed_ms(semantic_index_started);
    let typed = TypedProgram {
        kind: program.kind,
        expression_count: program.expressions.len(),
        expressions: program.expressions.clone(),
        expression_coverage,
        semantic_index,
        graph_node_count: nodes.len(),
        nodes,
        row_scopes,
        sources,
        dependencies,
        possible_causes,
        update_branches,
        list_operations,
        list_projections,
        functions,
        view_bindings,
        typecheck_report,
        derived_values,
        state_cells,
        lists,
        hidden_identity_verified: true,
        static_schedule_verified: true,
    };
    let verify_static_started = Instant::now();
    verify_static_schedule(&typed)?;
    let verify_static_ms = lower_elapsed_ms(verify_static_started);
    let verify_hidden_started = Instant::now();
    verify_hidden_identity(&typed)?;
    let verify_hidden_ms = lower_elapsed_ms(verify_hidden_started);
    let representation_analysis_started = Instant::now();
    let representation_analysis = representation_analysis(program, &typed);
    let representation_analysis_ms = lower_elapsed_ms(representation_analysis_started);
    let profile = serde_json::json!({
        "typecheck_ms": typecheck_ms,
        "typecheck_profile": typecheck_profile,
        "source_driven_nodes_ms": nodes_ms,
        "typed_field_defs_ms": fields_ms,
        "direct_source_refs_ms": direct_sources_ms,
        "row_scopes_ms": row_scopes_ms,
        "sources_ms": sources_ms,
        "state_cells_ms": state_cells_ms,
        "verify_combinational_field_cycles_ms": verify_cycles_ms,
        "lists_ms": lists_ms,
        "dependency_edges_ms": dependencies_ms,
        "possible_causes_ms": possible_causes_ms,
        "update_branches_ms": update_branches_ms,
        "list_operations_ms": list_operations_ms,
        "list_projections_ms": list_projections_ms,
        "function_definitions_ms": functions_ms,
        "derived_values_ms": derived_values_ms,
        "view_bindings_ms": view_bindings_ms,
        "expression_coverage_ms": expression_coverage_ms,
        "semantic_index_ms": semantic_index_ms,
        "verify_static_schedule_ms": verify_static_ms,
        "verify_hidden_identity_ms": verify_hidden_ms,
        "representation_analysis_ms": representation_analysis_ms,
        "representation_analysis": representation_analysis,
        "expression_count": typed.expression_count,
        "graph_node_count": typed.graph_node_count,
        "total_ms": lower_elapsed_ms(total_started)
    });
    Ok((typed, profile))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RepresentationExprClass {
    LiteralConstant,
    StaticComposite,
    RowDependent,
    SourceOrHoldDynamic,
    RuntimeDynamic,
    UnknownDynamic,
}

impl RepresentationExprClass {
    fn label(self) -> &'static str {
        match self {
            Self::LiteralConstant => "literal_constant",
            Self::StaticComposite => "static_composite",
            Self::RowDependent => "row_dependent",
            Self::SourceOrHoldDynamic => "source_or_hold_dynamic",
            Self::RuntimeDynamic => "runtime_dynamic",
            Self::UnknownDynamic => "unknown_dynamic",
        }
    }

    fn is_static(self) -> bool {
        matches!(self, Self::LiteralConstant | Self::StaticComposite)
    }
}

fn representation_analysis(program: &ParsedProgram, typed: &TypedProgram) -> serde_json::Value {
    let mut cache = vec![None; program.expressions.len()];
    let row_scope_names = representation_row_binding_names(program);
    let source_paths = program
        .source_ports
        .iter()
        .map(|source| source.path.as_str())
        .collect::<BTreeSet<_>>();
    let state_paths = program
        .state_cells
        .iter()
        .map(|cell| cell.path.as_str())
        .collect::<BTreeSet<_>>();
    let list_names = program
        .list_memories
        .iter()
        .map(|list| list.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut expression_class_counts = BTreeMap::<&'static str, usize>::new();
    let mut static_list_literal_count = 0usize;
    let mut dynamic_list_literal_count = 0usize;
    let mut list_literal_item_count = 0usize;
    for expr in &program.expressions {
        let class = representation_expr_class(
            expr.id,
            program,
            &row_scope_names,
            &source_paths,
            &state_paths,
            &list_names,
            &mut cache,
        );
        *expression_class_counts.entry(class.label()).or_default() += 1;
        if let AstExprKind::ListLiteral { items, .. } = &expr.kind {
            list_literal_item_count = list_literal_item_count.saturating_add(items.len());
            if class.is_static() {
                static_list_literal_count = static_list_literal_count.saturating_add(1);
            } else {
                dynamic_list_literal_count = dynamic_list_literal_count.saturating_add(1);
            }
        }
    }
    let list_storage_mode_candidates = representation_list_storage_mode_candidates(typed);
    let root_derived_samples = representation_root_derived_samples(
        program,
        typed,
        &row_scope_names,
        &source_paths,
        &state_paths,
        &list_names,
        &mut cache,
    );
    serde_json::json!({
        "version": 1,
        "computed_from": "parser_ast_and_typed_ir",
        "policy": "diagnostic_only_no_folding_or_storage_rewrite",
        "expression_class_counts": expression_class_counts,
        "list_literal_counts": {
            "static": static_list_literal_count,
            "dynamic": dynamic_list_literal_count,
            "item_count": list_literal_item_count
        },
        "list_storage_mode_candidates": list_storage_mode_candidates,
        "root_derived_samples": root_derived_samples,
    })
}

fn representation_row_binding_names(program: &ParsedProgram) -> BTreeSet<String> {
    let mut names = program
        .row_scope_functions
        .iter()
        .map(|scope| scope.row_scope.clone())
        .collect::<BTreeSet<_>>();
    for expr in &program.expressions {
        match &expr.kind {
            AstExprKind::Pipe { op, args, .. } if representation_op_binds_row(op) => {
                if let Some(name) = representation_positional_identifier_arg(program, args) {
                    names.insert(name);
                }
            }
            AstExprKind::Call { function, args } if representation_op_binds_row(function) => {
                if let Some(name) = representation_positional_identifier_arg(program, args) {
                    names.insert(name);
                }
            }
            _ => {}
        }
    }
    names
}

fn representation_op_binds_row(op: &str) -> bool {
    matches!(
        op,
        "List/map"
            | "List/retain"
            | "List/remove"
            | "List/every"
            | "List/filter_text_contains"
            | "List/filter_field_equal"
            | "List/filter_field_not_equal"
            | "List/move_field_first"
            | "List/move_field_last"
    )
}

fn representation_positional_identifier_arg(
    program: &ParsedProgram,
    args: &[AstCallArg],
) -> Option<String> {
    let value = args.iter().find(|arg| arg.name.is_none())?.value;
    match program.expressions.get(value).map(|expr| &expr.kind) {
        Some(AstExprKind::Identifier(name)) => Some(name.clone()),
        _ => None,
    }
}

fn representation_expr_class(
    expr_id: usize,
    program: &ParsedProgram,
    row_scope_names: &BTreeSet<String>,
    source_paths: &BTreeSet<&str>,
    state_paths: &BTreeSet<&str>,
    list_names: &BTreeSet<&str>,
    cache: &mut [Option<RepresentationExprClass>],
) -> RepresentationExprClass {
    if let Some(class) = cache.get(expr_id).copied().flatten() {
        return class;
    }
    let Some(expr) = program.expressions.iter().find(|expr| expr.id == expr_id) else {
        return RepresentationExprClass::UnknownDynamic;
    };
    let class = match &expr.kind {
        AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_) => RepresentationExprClass::LiteralConstant,
        AstExprKind::Object(fields)
        | AstExprKind::Record(fields)
        | AstExprKind::TaggedObject { fields, .. } => {
            representation_merge_child_classes(fields.iter().map(|field| {
                representation_expr_class(
                    field.value,
                    program,
                    row_scope_names,
                    source_paths,
                    state_paths,
                    list_names,
                    cache,
                )
            }))
            .filter(
                RepresentationExprClass::is_static,
                RepresentationExprClass::StaticComposite,
            )
        }
        AstExprKind::ListLiteral { items, .. } => {
            representation_merge_child_classes(items.iter().map(|item| {
                representation_expr_class(
                    *item,
                    program,
                    row_scope_names,
                    source_paths,
                    state_paths,
                    list_names,
                    cache,
                )
            }))
            .filter(
                RepresentationExprClass::is_static,
                RepresentationExprClass::StaticComposite,
            )
        }
        AstExprKind::Identifier(name) => representation_symbol_class(
            name,
            row_scope_names,
            source_paths,
            state_paths,
            list_names,
        ),
        AstExprKind::Path(parts) => representation_symbol_class(
            &parts.join("."),
            row_scope_names,
            source_paths,
            state_paths,
            list_names,
        ),
        AstExprKind::Source | AstExprKind::Hold { .. } => {
            RepresentationExprClass::SourceOrHoldDynamic
        }
        AstExprKind::Pipe { input, op, args } => {
            if op == "HOLD" {
                RepresentationExprClass::SourceOrHoldDynamic
            } else {
                let input_class = representation_expr_class(
                    *input,
                    program,
                    row_scope_names,
                    source_paths,
                    state_paths,
                    list_names,
                    cache,
                );
                let arg_class = representation_merge_child_classes(args.iter().map(|arg| {
                    representation_expr_class(
                        arg.value,
                        program,
                        row_scope_names,
                        source_paths,
                        state_paths,
                        list_names,
                        cache,
                    )
                }));
                representation_merge_child_classes([input_class, arg_class])
                    .promote_dynamic_default()
            }
        }
        AstExprKind::Call { args, .. } => {
            representation_merge_child_classes(args.iter().map(|arg| {
                representation_expr_class(
                    arg.value,
                    program,
                    row_scope_names,
                    source_paths,
                    state_paths,
                    list_names,
                    cache,
                )
            }))
            .promote_dynamic_default()
        }
        AstExprKind::Infix { left, right, .. } => representation_merge_child_classes([
            representation_expr_class(
                *left,
                program,
                row_scope_names,
                source_paths,
                state_paths,
                list_names,
                cache,
            ),
            representation_expr_class(
                *right,
                program,
                row_scope_names,
                source_paths,
                state_paths,
                list_names,
                cache,
            ),
        ])
        .promote_dynamic_default(),
        AstExprKind::When { .. } | AstExprKind::Then { .. } | AstExprKind::Latest => {
            RepresentationExprClass::RuntimeDynamic
        }
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => representation_expr_class(
            *output,
            program,
            row_scope_names,
            source_paths,
            state_paths,
            list_names,
            cache,
        )
        .promote_dynamic_default(),
        AstExprKind::MatchArm { output: None, .. }
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => RepresentationExprClass::UnknownDynamic,
    };
    if let Some(slot) = cache.get_mut(expr_id) {
        *slot = Some(class);
    }
    class
}

trait RepresentationClassExt {
    fn filter(self, predicate: impl FnOnce(Self) -> bool, replacement: Self) -> Self
    where
        Self: Sized;
    fn promote_dynamic_default(self) -> Self;
}

impl RepresentationClassExt for RepresentationExprClass {
    fn filter(self, predicate: impl FnOnce(Self) -> bool, replacement: Self) -> Self {
        if predicate(self) { replacement } else { self }
    }

    fn promote_dynamic_default(self) -> Self {
        if self.is_static() {
            RepresentationExprClass::RuntimeDynamic
        } else {
            self
        }
    }
}

fn representation_merge_child_classes(
    classes: impl IntoIterator<Item = RepresentationExprClass>,
) -> RepresentationExprClass {
    let mut saw_static = false;
    let mut saw_unknown = false;
    let mut saw_runtime = false;
    let mut saw_row = false;
    let mut saw_source_or_hold = false;
    for class in classes {
        match class {
            RepresentationExprClass::SourceOrHoldDynamic => {
                saw_source_or_hold = true;
            }
            RepresentationExprClass::RowDependent => saw_row = true,
            RepresentationExprClass::RuntimeDynamic => saw_runtime = true,
            RepresentationExprClass::UnknownDynamic => saw_unknown = true,
            RepresentationExprClass::LiteralConstant | RepresentationExprClass::StaticComposite => {
                saw_static = true;
            }
        }
    }
    if saw_source_or_hold {
        RepresentationExprClass::SourceOrHoldDynamic
    } else if saw_row {
        RepresentationExprClass::RowDependent
    } else if saw_runtime {
        RepresentationExprClass::RuntimeDynamic
    } else if saw_unknown {
        RepresentationExprClass::UnknownDynamic
    } else if saw_static {
        RepresentationExprClass::StaticComposite
    } else {
        RepresentationExprClass::LiteralConstant
    }
}

fn representation_symbol_class(
    name: &str,
    row_scope_names: &BTreeSet<String>,
    source_paths: &BTreeSet<&str>,
    state_paths: &BTreeSet<&str>,
    list_names: &BTreeSet<&str>,
) -> RepresentationExprClass {
    if row_scope_names.iter().any(|scope| {
        name == scope.as_str()
            || name
                .strip_prefix(scope.as_str())
                .is_some_and(|suffix| suffix.starts_with('.'))
    }) {
        return RepresentationExprClass::RowDependent;
    }
    if source_paths.contains(name) || state_paths.contains(name) || list_names.contains(name) {
        return RepresentationExprClass::RuntimeDynamic;
    }
    if name == "SOURCE" || name == "HOLD" || name.contains(".event.") {
        return RepresentationExprClass::SourceOrHoldDynamic;
    }
    RepresentationExprClass::UnknownDynamic
}

fn representation_list_storage_mode_candidates(
    typed: &TypedProgram,
) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::<&'static str, usize>::new();
    for list in &typed.lists {
        let mode = match &list.initializer {
            ListInitializer::RecordLiteral { rows } => {
                if list_initializer_has_dynamic_fields(rows) {
                    "dense_vec_dynamic_initializer"
                } else {
                    "constant_array_literal"
                }
            }
            ListInitializer::Range { .. } => "virtual_range",
            ListInitializer::Empty => "dense_vec_empty",
            ListInitializer::Unknown { .. } => "unknown_initializer",
        };
        *counts.entry(mode).or_default() += 1;
    }
    for value in typed
        .derived_values
        .iter()
        .filter(|value| matches!(value.kind, DerivedValueKind::ListView))
    {
        for mode in representation_list_view_mode_hints(&value.statement, &typed.expressions) {
            *counts.entry(mode).or_default() += 1;
        }
    }
    counts
}

fn representation_root_derived_samples(
    program: &ParsedProgram,
    typed: &TypedProgram,
    row_scope_names: &BTreeSet<String>,
    source_paths: &BTreeSet<&str>,
    state_paths: &BTreeSet<&str>,
    list_names: &BTreeSet<&str>,
    cache: &mut [Option<RepresentationExprClass>],
) -> Vec<serde_json::Value> {
    typed
        .derived_values
        .iter()
        .filter(|value| !value.indexed && value.scope_id.is_none())
        .take(64)
        .map(|value| {
            let class = representation_statement_class(
                &value.statement,
                program,
                row_scope_names,
                source_paths,
                state_paths,
                list_names,
                cache,
            );
            serde_json::json!({
                "path": value.path,
                "line": value.statement.line,
                "kind": derived_value_kind_label_ir(&value.kind),
                "class": class.label(),
                "list_storage_hints": if matches!(value.kind, DerivedValueKind::ListView) {
                    representation_list_view_mode_hints(&value.statement, &typed.expressions)
                } else {
                    Vec::new()
                }
            })
        })
        .collect()
}

fn representation_statement_class(
    statement: &AstStatement,
    program: &ParsedProgram,
    row_scope_names: &BTreeSet<String>,
    source_paths: &BTreeSet<&str>,
    state_paths: &BTreeSet<&str>,
    list_names: &BTreeSet<&str>,
    cache: &mut [Option<RepresentationExprClass>],
) -> RepresentationExprClass {
    let ids = representation_statement_expr_ids(statement, &program.expressions);
    representation_merge_child_classes(ids.into_iter().map(|expr_id| {
        representation_expr_class(
            expr_id,
            program,
            row_scope_names,
            source_paths,
            state_paths,
            list_names,
            cache,
        )
    }))
}

fn representation_statement_expr_ids(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Vec<usize> {
    let mut ids = Vec::new();
    representation_collect_statement_expr_ids(statement, expressions, &mut ids);
    ids
}

fn representation_collect_statement_expr_ids(
    statement: &AstStatement,
    expressions: &[AstExpr],
    ids: &mut Vec<usize>,
) {
    if let Some(expr_id) = statement.expr {
        representation_push_expr_id(expr_id, expressions, ids);
    }
    for child in &statement.children {
        representation_collect_statement_expr_ids(child, expressions, ids);
    }
}

fn representation_push_expr_id(expr_id: usize, expressions: &[AstExpr], ids: &mut Vec<usize>) {
    if !ids.contains(&expr_id) {
        ids.push(expr_id);
    }
    let Some(expr) = expressions.iter().find(|expr| expr.id == expr_id) else {
        return;
    };
    match &expr.kind {
        AstExprKind::Call { args, .. } => {
            for arg in args {
                representation_push_expr_id(arg.value, expressions, ids);
            }
        }
        AstExprKind::Pipe { input, args, .. } => {
            representation_push_expr_id(*input, expressions, ids);
            for arg in args {
                representation_push_expr_id(arg.value, expressions, ids);
            }
        }
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial } => {
            representation_push_expr_id(*initial, expressions, ids);
        }
        AstExprKind::Then { input, output } => {
            representation_push_expr_id(*input, expressions, ids);
            if let Some(output) = output {
                representation_push_expr_id(*output, expressions, ids);
            }
        }
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => representation_push_expr_id(*output, expressions, ids),
        AstExprKind::Infix { left, right, .. } => {
            representation_push_expr_id(*left, expressions, ids);
            representation_push_expr_id(*right, expressions, ids);
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => {
            for field in fields {
                representation_push_expr_id(field.value, expressions, ids);
            }
        }
        AstExprKind::ListLiteral { items, .. } => {
            for item in items {
                representation_push_expr_id(*item, expressions, ids);
            }
        }
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
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

fn representation_list_view_mode_hints(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> Vec<&'static str> {
    let mut hints = BTreeSet::new();
    for expr_id in representation_statement_expr_ids(statement, expressions) {
        let Some(expr) = expressions.iter().find(|expr| expr.id == expr_id) else {
            continue;
        };
        match &expr.kind {
            AstExprKind::Pipe { op, .. } | AstExprKind::Call { function: op, .. } => {
                match op.as_str() {
                    "List/map" => {
                        hints.insert("incremental_projection");
                    }
                    "List/retain"
                    | "List/filter_text_contains"
                    | "List/filter_field_equal"
                    | "List/filter_field_not_equal"
                    | "List/move_field_first"
                    | "List/move_field_last" => {
                        hints.insert("selection_view");
                    }
                    "List/chunk" => {
                        hints.insert("page_or_chunk_view");
                    }
                    "List/range" => {
                        hints.insert("virtual_range");
                    }
                    _ => {}
                }
            }
            AstExprKind::ListLiteral { .. } => {
                hints.insert("constant_or_dense_literal");
            }
            _ => {}
        }
    }
    if hints.is_empty() {
        hints.insert("dense_vec_materialized");
    }
    hints.into_iter().collect()
}

fn derived_value_kind_label_ir(kind: &DerivedValueKind) -> &'static str {
    match kind {
        DerivedValueKind::SourceEventTransform => "source_event_transform",
        DerivedValueKind::ListView => "list_view",
        DerivedValueKind::Aggregate => "aggregate",
        DerivedValueKind::Pure => "pure",
        DerivedValueKind::Unknown => "unknown",
    }
}

fn semantic_index(
    program: &ParsedProgram,
    fields: &[FieldDef],
    row_scopes: &[RowScope],
    sources: &[SourcePort],
    state_cells: &[StateCell],
    lists: &[ListMemory],
    functions: &[FunctionDefinition],
    view_bindings: &[ViewBinding],
    typecheck_report: &boon_typecheck::TypeCheckReport,
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
    let fields = fields
        .iter()
        .enumerate()
        .map(|(id, field)| SemanticFieldEntry {
            id: FieldId(id),
            path: field.path.clone(),
            local_name: field.local_name.clone(),
            parent_path: field.parent_path.clone(),
            scope_id: scope_id_for_path(
                row_scopes_from_entries(row_scopes.as_slice()).as_slice(),
                &field.path,
            ),
            statement_id: field.statement.id,
            line: field.statement.line,
            kind: semantic_field_kind(
                field,
                state_cells,
                lists_from_entries(lists.as_slice()).as_slice(),
            ),
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
        &sources,
        &lists,
        &row_scopes,
        &functions,
        &fields,
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
        source_units,
        sources,
        lists,
        row_scopes,
        functions,
        fields,
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
                "TypedProgram.view_bindings".to_owned(),
            ],
        },
    }
}

fn semantic_symbols(
    program: &ParsedProgram,
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
            AstExprKind::Call { function, args } => {
                table.intern("operator_name", function);
                for arg in args {
                    if let Some(name) = &arg.name {
                        table.intern("document_attr", name);
                    }
                }
            }
            AstExprKind::Pipe { op, args, .. } => {
                table.intern("operator_name", op);
                for arg in args {
                    if let Some(name) = &arg.name {
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

fn row_scopes_from_entries(entries: &[SemanticRowScopeEntry]) -> Vec<RowScope> {
    entries
        .iter()
        .map(|entry| RowScope {
            id: entry.id,
            list: entry.list.clone(),
            function: entry.function.clone(),
            row_scope: entry.row_scope.clone(),
        })
        .collect()
}

fn lists_from_entries(entries: &[SemanticListEntry]) -> Vec<ListMemory> {
    entries
        .iter()
        .map(|entry| ListMemory {
            id: entry.id,
            name: entry.name.clone(),
            row_scope_id: entry.row_scope_id,
            hidden_key_type: hidden_key_type(&entry.name),
            has_generation: true,
            graph_clones_per_item: 0,
            capacity: entry.capacity,
            initializer: if entry.initializer_known {
                ListInitializer::Empty
            } else {
                ListInitializer::Unknown {
                    summary: "semantic index entry carried fallback initializer".to_owned(),
                }
            },
        })
        .collect()
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
        if let Some(existing_list) = seen.insert(scope.row_scope.as_str(), scope.list.as_str()) {
            if existing_list != scope.list {
                reasons.push(format!(
                    "row scope `{}` is shared by lists `{}` and `{}`",
                    scope.row_scope, existing_list, scope.list
                ));
            }
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

pub fn document_view_bindings_with_typecheck(
    program: &ParsedProgram,
    typecheck_report: &boon_typecheck::TypeCheckReport,
) -> Vec<ViewBinding> {
    let row_scopes = row_scopes(program);
    let fields = typed_field_defs(program);
    let direct_sources = direct_source_refs_by_path(&fields, program);
    let sources = program
        .source_ports
        .iter()
        .enumerate()
        .map(|(id, source)| SourcePort {
            id: SourceId(id),
            scoped: source.scoped,
            scope_id: scope_id_for_path(&row_scopes, &source.path),
            payload_schema: source_payload_schema(program, &fields, &direct_sources, &source.path),
            path: source.path.clone(),
        })
        .collect::<Vec<_>>();
    view_bindings(program, &row_scopes, &sources, typecheck_report)
}

pub fn verify_hidden_identity(program: &TypedProgram) -> Result<(), String> {
    if !program.hidden_identity_verified {
        return Err("hidden identity verification did not run".to_owned());
    }
    if program.lists.iter().any(|list| !list.has_generation) {
        return Err("all list memories must carry generation guards".to_owned());
    }
    verify_identity_clean_identifiers(program)?;
    Ok(())
}

pub fn verify_static_schedule(program: &TypedProgram) -> Result<(), String> {
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
    let known_symbols = source_paths
        .iter()
        .chain(state_paths.iter())
        .chain(list_names.iter())
        .chain(derived_paths.iter())
        .copied()
        .chain(store_list_names.iter().map(String::as_str))
        .chain(source_payload_paths.iter().map(String::as_str))
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
    for branch in &program.update_branches {
        if !state_paths.contains(branch.target.as_str()) {
            return Err(format!(
                "update branch target `{}` is not a scheduled state cell",
                branch.target
            ));
        }
        if !source_paths.contains(branch.source.as_str()) {
            return Err(format!(
                "update branch source `{}` is not a declared source port",
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
        if !list_names.contains(operation.list.as_str()) {
            return Err(format!(
                "list operation references unknown list `{}`",
                operation.list
            ));
        }
        verify_scheduled_list_operation(&operation.kind, &source_paths, &known_symbols)?;
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
    program: &TypedProgram,
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

fn source_payload_schema(
    program: &ParsedProgram,
    fields: &[FieldDef],
    direct_sources: &BTreeMap<String, Vec<String>>,
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
    let address_lookup_field = source_address_lookup_field(program, fields, source);
    if address_lookup_field.is_some() {
        payload_fields.insert(SourcePayloadField::Address);
    }
    SourcePayloadSchema {
        fields: payload_fields.into_iter().collect(),
        address_lookup_field,
    }
}

fn source_address_lookup_field(
    program: &ParsedProgram,
    fields: &[FieldDef],
    source: &str,
) -> Option<String> {
    let Some(source_scope) = source.split('.').next() else {
        return None;
    };
    let scope = program
        .row_scope_functions
        .iter()
        .find(|scope| scope.row_scope == source_scope);
    if let Some(scope) = scope {
        if let Some(explicit_address) = fields.iter().find_map(|field| {
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
        }) {
            return Some(explicit_address);
        }
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
    select_source_address_lookup_field(source, candidates)
}

fn select_source_address_lookup_field(source: &str, candidates: Vec<String>) -> Option<String> {
    candidates
        .into_iter()
        .enumerate()
        .max_by_key(|(index, candidate)| {
            (
                source_address_lookup_field_score(source, candidate),
                std::cmp::Reverse(*index),
            )
        })
        .map(|(_, candidate)| candidate)
}

fn source_address_lookup_field_score(source: &str, candidate: &str) -> i32 {
    let terms = source_address_intent_terms(source);
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

fn source_address_intent_terms(source: &str) -> Vec<String> {
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

fn view_bindings(
    program: &ParsedProgram,
    row_scopes: &[RowScope],
    sources: &[SourcePort],
    typecheck_report: &boon_typecheck::TypeCheckReport,
) -> Vec<ViewBinding> {
    let source_paths = sources
        .iter()
        .map(|source| (source.path.as_str(), source.id))
        .collect::<Vec<_>>();
    let mut bindings = Vec::new();
    let render_slots = RenderSlotBindingLookup::new(typecheck_report);
    let mut visited_expr_contexts = BTreeSet::new();
    if let Some(document) = boon_parser::parsed_document(program) {
        let document_functions = DocumentViewFunctionRegistry::new(&program.ast.statements);
        collect_document_view_bindings(
            std::slice::from_ref(&document.root),
            &program.source,
            &document.expressions,
            &document_functions,
            row_scopes,
            &source_paths,
            &render_slots,
            &mut bindings,
            &mut Vec::new(),
            &mut visited_expr_contexts,
            &DocumentViewBindingContext::default(),
        );
    }
    if let Some(scene) = render_root_statement(program, "scene") {
        let document_functions = DocumentViewFunctionRegistry::new(&program.ast.statements);
        collect_document_view_bindings(
            std::slice::from_ref(scene),
            &program.source,
            &program.ast.expressions,
            &document_functions,
            row_scopes,
            &source_paths,
            &render_slots,
            &mut bindings,
            &mut Vec::new(),
            &mut visited_expr_contexts,
            &DocumentViewBindingContext::default(),
        );
    }
    normalize_view_binding_ids(&mut bindings);
    bindings
}

fn normalize_view_binding_ids(bindings: &mut Vec<ViewBinding>) {
    let mut seen = BTreeSet::new();
    bindings.retain(|binding| {
        seen.insert((
            binding.node_kind.clone(),
            binding.attr.clone(),
            binding.path.clone(),
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

fn render_root_statement<'a>(program: &'a ParsedProgram, name: &str) -> Option<&'a AstStatement> {
    program
        .ast
        .statements
        .iter()
        .find(|statement| matches!(&statement.kind, AstStatementKind::Field { name: field } if field == name))
}

struct DocumentViewFunctionRegistry<'a> {
    functions: BTreeMap<&'a str, &'a AstStatement>,
}

struct RenderSlotBindingLookup<'a> {
    by_statement_id: BTreeMap<usize, &'a boon_typecheck::ListMapBinding>,
    by_expr_id: BTreeMap<usize, &'a boon_typecheck::ListMapBinding>,
}

impl<'a> RenderSlotBindingLookup<'a> {
    fn new(typecheck_report: &'a boon_typecheck::TypeCheckReport) -> Self {
        let mut by_statement_id = BTreeMap::new();
        let mut by_expr_id = BTreeMap::new();
        for slot in &typecheck_report.render_slot_table.slots {
            let Some(binding_id) = slot.optional_list_map_binding_id else {
                continue;
            };
            let Some(binding) = typecheck_report.list_map_bindings.get(binding_id) else {
                continue;
            };
            by_statement_id.insert(slot.slot_statement_id, binding);
            if let Some(expr_id) = slot.value_expr_id {
                by_expr_id.insert(expr_id, binding);
            }
        }
        Self {
            by_statement_id,
            by_expr_id,
        }
    }

    fn for_statement(&self, statement_id: usize) -> Option<&'a boon_typecheck::ListMapBinding> {
        self.by_statement_id.get(&statement_id).copied()
    }

    fn for_expr(&self, expr_id: usize) -> Option<&'a boon_typecheck::ListMapBinding> {
        self.by_expr_id.get(&expr_id).copied()
    }
}

impl<'a> DocumentViewFunctionRegistry<'a> {
    fn new(statements: &'a [AstStatement]) -> Self {
        let mut functions = BTreeMap::new();
        Self::collect(statements, &mut functions);
        Self { functions }
    }

    fn collect(
        statements: &'a [AstStatement],
        functions: &mut BTreeMap<&'a str, &'a AstStatement>,
    ) {
        for statement in statements {
            if let AstStatementKind::Function { name, .. } = &statement.kind {
                functions.insert(name.as_str(), statement);
            }
            Self::collect(&statement.children, functions);
        }
    }

    fn get(&self, name: &str) -> Option<&'a AstStatement> {
        if let Some(statement) = self.functions.get(name).copied() {
            return Some(statement);
        }
        let suffix = format!("/{name}");
        let mut matches = self
            .functions
            .iter()
            .filter_map(|(function_name, statement)| {
                function_name.ends_with(&suffix).then_some(*statement)
            });
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }
}

#[derive(Clone, Default)]
struct DocumentViewBindingContext {
    arg_exprs: Vec<BTreeMap<String, usize>>,
    source_bases: Vec<String>,
}

impl DocumentViewBindingContext {
    fn arg_expr(&self, name: &str) -> Option<usize> {
        self.arg_exprs
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn source_base(&self) -> Option<&str> {
        self.source_bases.last().map(String::as_str)
    }

    fn with_function_call(
        &self,
        function: &AstStatement,
        call: &AstStatement,
        expressions: &[AstExpr],
    ) -> Self {
        if let Some(args) = document_call_args(call, expressions) {
            return self.with_function_args(function, args);
        }
        self.clone()
    }

    fn with_function_args(
        &self,
        function: &AstStatement,
        args: &[boon_parser::AstCallArg],
    ) -> Self {
        let mut next = self.clone();
        let formals = match &function.kind {
            AstStatementKind::Function { args, .. } => args.as_slice(),
            _ => &[],
        };
        let mut scope = BTreeMap::new();
        for (index, arg) in args.iter().enumerate() {
            let Some(name) = arg
                .name
                .as_deref()
                .or_else(|| formals.get(index).map(String::as_str))
            else {
                continue;
            };
            scope.insert(name.to_owned(), arg.value);
        }
        next.arg_exprs.push(scope);
        next
    }

    fn with_pipe_function_call(
        &self,
        function: &AstStatement,
        input_expr_id: usize,
        args: &[boon_parser::AstCallArg],
    ) -> Self {
        let mut next = self.clone();
        let formals = match &function.kind {
            AstStatementKind::Function { args, .. } => args.as_slice(),
            _ => &[],
        };
        let mut scope = BTreeMap::new();
        if let Some(first_formal) = formals.first() {
            scope.insert(first_formal.clone(), input_expr_id);
        }
        for (index, arg) in args.iter().enumerate() {
            let Some(name) = arg
                .name
                .as_deref()
                .or_else(|| formals.get(index + 1).map(String::as_str))
            else {
                continue;
            };
            scope.insert(name.to_owned(), arg.value);
        }
        next.arg_exprs.push(scope);
        next
    }

    fn with_function_item_expr(&self, function: &AstStatement, item_expr_id: usize) -> Self {
        let mut next = self.clone();
        let mut scope = BTreeMap::new();
        if let AstStatementKind::Function { args, .. } = &function.kind
            && let Some(first_formal) = args.first()
        {
            scope.insert(first_formal.clone(), item_expr_id);
        }
        next.arg_exprs.push(scope);
        next
    }

    fn with_local_scope(&self) -> Self {
        let mut next = self.clone();
        next.arg_exprs.push(BTreeMap::new());
        next
    }

    fn insert_local_expr(&mut self, name: String, expr_id: usize) {
        if let Some(scope) = self.arg_exprs.last_mut() {
            scope.insert(name, expr_id);
        }
    }

    fn with_source_base(&self, path: String) -> Self {
        let mut next = self.clone();
        next.source_bases.push(path);
        next
    }

    fn cache_key(&self) -> String {
        let mut parts = Vec::new();
        for scope in &self.arg_exprs {
            let scope_key = scope
                .iter()
                .map(|(name, expr_id)| format!("{name}:{expr_id}"))
                .collect::<Vec<_>>()
                .join(",");
            parts.push(format!("args[{scope_key}]"));
        }
        if !self.source_bases.is_empty() {
            parts.push(format!("source[{}]", self.source_bases.join(">")));
        }
        parts.join("|")
    }
}

fn view_data_path(value: &str) -> Option<String> {
    let path = value.strip_prefix('$')?;
    let path = path.split_once(':').map_or(path, |(path, _)| path);
    (!path.trim().is_empty()).then(|| normalized_view_data_path(path))
}

fn normalized_view_data_path(path: &str) -> String {
    path.split('.')
        .filter(|part| *part != "PASSED")
        .collect::<Vec<_>>()
        .join(".")
}

fn source_path_for_source_pipe(expr: &AstExpr, source_text: &str) -> Option<String> {
    let snippet = source_text.get(expr.start..expr.end)?;
    let source = snippet.find("SOURCE")?;
    let after_source = &snippet[source + "SOURCE".len()..];
    let open = after_source.find('{')?;
    let close = after_source.rfind('}')?;
    if close <= open {
        return None;
    }
    let compact_path = after_source[open + 1..close]
        .split_whitespace()
        .collect::<String>();
    let path = compact_path.trim();
    (!path.is_empty()).then(|| normalized_view_data_path(path))
}

fn source_path_for_source_pipe_expr(
    expr: &AstExpr,
    source_text: &str,
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
) -> Option<String> {
    if let AstExprKind::Pipe { op, args, .. } = &expr.kind
        && op == "SOURCE"
        && let Some(arg) = args.first()
        && let Some(path) = document_expr_value_by_id(arg.value, expressions, context)
    {
        return Some(normalized_view_data_path(&path));
    }
    source_path_for_source_pipe(expr, source_text)
}

fn view_data_path_for_expr_id(
    expr_id: usize,
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
) -> Option<String> {
    view_data_path_for_expr_id_inner(expr_id, expressions, context, &mut BTreeSet::new())
}

fn view_data_path_for_expr_id_inner(
    expr_id: usize,
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
    seen: &mut BTreeSet<usize>,
) -> Option<String> {
    if !seen.insert(expr_id) {
        return None;
    }
    let resolved_expr_id =
        document_resolved_expr_id(expr_id, expressions, context, &mut BTreeSet::new())?;
    if resolved_expr_id != expr_id {
        return view_data_path_for_expr_id_inner(resolved_expr_id, expressions, context, seen);
    }
    view_data_path_for_expr_inner(expressions.get(expr_id)?, expressions, context, seen)
}

fn view_data_path_for_expr_inner(
    expr: &AstExpr,
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
    seen: &mut BTreeSet<usize>,
) -> Option<String> {
    match &expr.kind {
        AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => {
            view_data_path(value)
        }
        AstExprKind::Identifier(value) => context
            .arg_expr(value)
            .and_then(|expr_id| {
                view_data_path_for_expr_id_inner(expr_id, expressions, context, seen)
            })
            .or_else(|| Some(value.clone())),
        AstExprKind::Path(parts) if parts.first().is_some_and(|part| part == "element") => None,
        AstExprKind::Path(parts) => document_path_value(parts, expressions, context)
            .map(|path| normalized_view_data_path(&path)),
        AstExprKind::Infix { left, .. } => {
            view_data_path_for_expr_id_inner(*left, expressions, context, seen)
        }
        _ => None,
    }
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

fn attr_can_contain_render(attr: &str) -> bool {
    matches!(
        attr,
        "root" | "child" | "children" | "items" | "contents" | "label" | "icon" | "placeholder"
    )
}

fn push_data_view_binding_for_expr(
    node_kind: &str,
    attr: &str,
    expr_id: usize,
    expressions: &[AstExpr],
    row_scopes: &[RowScope],
    bindings: &mut Vec<ViewBinding>,
    context: &DocumentViewBindingContext,
) {
    if !attr_can_bind_data(attr) {
        return;
    }
    let Some(path) = view_data_path_for_expr_id(expr_id, expressions, context) else {
        return;
    };
    if !view_data_binding_is_schedulable(&path, row_scopes, context) {
        return;
    }
    bindings.push(ViewBinding {
        id: ViewBindingId(bindings.len()),
        node_kind: node_kind.to_owned(),
        attr: attr.to_owned(),
        scope_id: scope_id_for_path(row_scopes, &path),
        source_id: None,
        kind: if attr == "target" {
            ViewBindingKind::Target
        } else {
            ViewBindingKind::Data
        },
        path,
    });
}

fn view_data_binding_is_schedulable(
    path: &str,
    row_scopes: &[RowScope],
    context: &DocumentViewBindingContext,
) -> bool {
    if scope_id_for_path(row_scopes, path).is_some() {
        return true;
    }
    let Some(first_segment) = path.split('.').next() else {
        return false;
    };
    if context.arg_expr(first_segment).is_some() {
        return false;
    }
    path.contains('.')
}

fn statement_expr_can_contain_render(statement: &AstStatement) -> bool {
    match &statement.kind {
        AstStatementKind::Expression | AstStatementKind::Source { .. } => true,
        AstStatementKind::Field { name }
        | AstStatementKind::List {
            field: Some(name), ..
        } => name == "document" || name == "scene" || attr_can_contain_render(name),
        AstStatementKind::Function { .. }
        | AstStatementKind::Hold { .. }
        | AstStatementKind::List { field: None, .. }
        | AstStatementKind::Block => false,
    }
}

fn statement_children_can_contain_render(statement: &AstStatement) -> bool {
    match &statement.kind {
        AstStatementKind::Field { name }
        | AstStatementKind::List {
            field: Some(name), ..
        } => {
            name == "document"
                || name == "scene"
                || name == "element"
                || attr_can_contain_render(name)
        }
        AstStatementKind::Function { .. }
        | AstStatementKind::Source { .. }
        | AstStatementKind::Hold { .. }
        | AstStatementKind::List { field: None, .. }
        | AstStatementKind::Block
        | AstStatementKind::Expression => true,
    }
}

fn source_pipe_continuation_base(
    statement: &AstStatement,
    source_text: &str,
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
) -> Option<String> {
    let expr = expressions.get(statement.expr?)?;
    let AstExprKind::Pipe { input, op, .. } = &expr.kind else {
        return None;
    };
    if op != "SOURCE" || !matches!(expressions.get(*input)?.kind, AstExprKind::Delimiter) {
        return None;
    }
    source_path_for_source_pipe_expr(expr, source_text, expressions, context)
}

fn expr_is_source_pipe_continuation(expr_id: usize, expressions: &[AstExpr]) -> bool {
    expressions.get(expr_id).is_some_and(|expr| {
        matches!(
            &expr.kind,
            AstExprKind::Pipe { input, op, .. }
                if op == "SOURCE"
                    && expressions
                        .get(*input)
                        .is_some_and(|input| matches!(input.kind, AstExprKind::Delimiter))
        )
    })
}

fn collect_document_function_body_view_bindings(
    function_statement: &AstStatement,
    source_text: &str,
    expressions: &[AstExpr],
    functions: &DocumentViewFunctionRegistry<'_>,
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    render_slots: &RenderSlotBindingLookup<'_>,
    bindings: &mut Vec<ViewBinding>,
    function_stack: &mut Vec<String>,
    visited_expr_contexts: &mut BTreeSet<(usize, String)>,
    context: &DocumentViewBindingContext,
) {
    if let Some(expr_id) = function_statement.expr {
        collect_document_expr_view_bindings(
            expr_id,
            source_text,
            expressions,
            functions,
            row_scopes,
            source_paths,
            render_slots,
            bindings,
            function_stack,
            context,
            visited_expr_contexts,
            &mut Vec::new(),
        );
    }
    collect_document_view_bindings(
        &function_statement.children,
        source_text,
        expressions,
        functions,
        row_scopes,
        source_paths,
        render_slots,
        bindings,
        function_stack,
        visited_expr_contexts,
        context,
    );
}

fn collect_document_view_bindings(
    statements: &[AstStatement],
    source_text: &str,
    expressions: &[AstExpr],
    functions: &DocumentViewFunctionRegistry<'_>,
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    render_slots: &RenderSlotBindingLookup<'_>,
    bindings: &mut Vec<ViewBinding>,
    function_stack: &mut Vec<String>,
    visited_expr_contexts: &mut BTreeSet<(usize, String)>,
    context: &DocumentViewBindingContext,
) {
    let mut sibling_context = context.with_local_scope();
    let mut previous_render_expr_id = None;
    let mut previous_render_statement = None;
    for statement in statements {
        if let Some(source_base) =
            source_pipe_continuation_base(statement, source_text, expressions, &sibling_context)
        {
            let source_context = sibling_context.with_source_base(source_base);
            if let Some(previous_statement) = previous_render_statement {
                collect_document_statement_source_bindings(
                    previous_statement,
                    source_text,
                    expressions,
                    functions,
                    row_scopes,
                    source_paths,
                    render_slots,
                    bindings,
                    function_stack,
                    visited_expr_contexts,
                    &source_context,
                );
            } else if let Some(previous_expr_id) = previous_render_expr_id {
                collect_document_expr_view_bindings(
                    previous_expr_id,
                    source_text,
                    expressions,
                    functions,
                    row_scopes,
                    source_paths,
                    render_slots,
                    bindings,
                    function_stack,
                    &source_context,
                    visited_expr_contexts,
                    &mut Vec::new(),
                );
            }
        }
        if matches!(
            document_statement_field(statement).as_deref(),
            Some("items" | "children")
        ) && let Some(binding) = render_slots.for_statement(statement.id)
        {
            if let Some(function_name) = binding.template_function.as_deref()
                && let Some(function_statement) = functions.get(function_name)
                && !function_stack.iter().any(|active| active == function_name)
            {
                function_stack.push(function_name.to_owned());
                let scoped_context = if !binding.template_args.is_empty() {
                    sibling_context.with_function_args(function_statement, &binding.template_args)
                } else {
                    sibling_context
                        .with_function_item_expr(function_statement, binding.item_expr_id)
                };
                collect_document_function_body_view_bindings(
                    function_statement,
                    source_text,
                    expressions,
                    functions,
                    row_scopes,
                    source_paths,
                    render_slots,
                    bindings,
                    function_stack,
                    visited_expr_contexts,
                    &scoped_context,
                );
                function_stack.pop();
            }
            continue;
        }
        if let Some(function) = document_statement_call(statement, expressions)
            && boon_typecheck::is_registered_element_constructor(function)
        {
            collect_canonical_element_view_bindings(
                function,
                statement,
                expressions,
                row_scopes,
                source_paths,
                bindings,
                &sibling_context,
            );
        } else if let Some(function) = document_statement_call(statement, expressions)
            && let Some(function_statement) = functions.get(function)
            && !function_stack.iter().any(|active| active == function)
        {
            function_stack.push(function.to_owned());
            let scoped_context =
                sibling_context.with_function_call(function_statement, statement, expressions);
            collect_document_function_body_view_bindings(
                function_statement,
                source_text,
                expressions,
                functions,
                row_scopes,
                source_paths,
                render_slots,
                bindings,
                function_stack,
                visited_expr_contexts,
                &scoped_context,
            );
            function_stack.pop();
        } else if document_statement_field(statement).as_deref() == Some("element")
            && let Some(kind) = document_child_value(statement, "kind", expressions)
        {
            collect_document_element_bindings(
                &kind,
                statement,
                expressions,
                row_scopes,
                source_paths,
                bindings,
                &sibling_context,
            );
        }
        if statement_expr_can_contain_render(statement)
            && let Some(expr_id) = statement.expr
        {
            collect_document_expr_view_bindings(
                expr_id,
                source_text,
                expressions,
                functions,
                row_scopes,
                source_paths,
                render_slots,
                bindings,
                function_stack,
                &sibling_context,
                visited_expr_contexts,
                &mut Vec::new(),
            );
        }
        if let Some(parent_expr_id) = statement.expr {
            for child in &statement.children {
                if let Some(source_base) =
                    source_pipe_continuation_base(child, source_text, expressions, &sibling_context)
                {
                    let source_context = sibling_context.with_source_base(source_base);
                    collect_document_expr_view_bindings(
                        parent_expr_id,
                        source_text,
                        expressions,
                        functions,
                        row_scopes,
                        source_paths,
                        render_slots,
                        bindings,
                        function_stack,
                        &source_context,
                        visited_expr_contexts,
                        &mut Vec::new(),
                    );
                }
            }
        }
        if statement_children_can_contain_render(statement) {
            collect_document_view_bindings(
                &statement.children,
                source_text,
                expressions,
                functions,
                row_scopes,
                source_paths,
                render_slots,
                bindings,
                function_stack,
                visited_expr_contexts,
                &sibling_context,
            );
        }
        if let AstStatementKind::Field { name } = &statement.kind
            && let Some(expr_id) = statement.expr
        {
            sibling_context.insert_local_expr(name.clone(), expr_id);
        }
        if statement_expr_can_contain_render(statement)
            && let Some(expr_id) = statement.expr
            && !expr_is_source_pipe_continuation(expr_id, expressions)
        {
            previous_render_expr_id = Some(expr_id);
            previous_render_statement = Some(statement);
        }
    }
}

fn collect_document_statement_source_bindings(
    statement: &AstStatement,
    source_text: &str,
    expressions: &[AstExpr],
    functions: &DocumentViewFunctionRegistry<'_>,
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    render_slots: &RenderSlotBindingLookup<'_>,
    bindings: &mut Vec<ViewBinding>,
    function_stack: &mut Vec<String>,
    visited_expr_contexts: &mut BTreeSet<(usize, String)>,
    context: &DocumentViewBindingContext,
) {
    if let Some(function) = document_statement_call(statement, expressions)
        && boon_typecheck::is_registered_element_constructor(function)
    {
        collect_canonical_element_view_bindings(
            function,
            statement,
            expressions,
            row_scopes,
            source_paths,
            bindings,
            context,
        );
        return;
    }
    if let Some(expr_id) = statement.expr {
        collect_document_expr_view_bindings(
            expr_id,
            source_text,
            expressions,
            functions,
            row_scopes,
            source_paths,
            render_slots,
            bindings,
            function_stack,
            context,
            visited_expr_contexts,
            &mut Vec::new(),
        );
    }
}

fn collect_document_expr_view_bindings(
    expr_id: usize,
    source_text: &str,
    expressions: &[AstExpr],
    functions: &DocumentViewFunctionRegistry<'_>,
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    render_slots: &RenderSlotBindingLookup<'_>,
    bindings: &mut Vec<ViewBinding>,
    function_stack: &mut Vec<String>,
    context: &DocumentViewBindingContext,
    visited_expr_contexts: &mut BTreeSet<(usize, String)>,
    expr_stack: &mut Vec<usize>,
) {
    let Some(expr_id) =
        document_resolved_expr_id(expr_id, expressions, context, &mut BTreeSet::new())
    else {
        return;
    };
    if expr_stack.contains(&expr_id) {
        return;
    }
    if !visited_expr_contexts.insert((expr_id, context.cache_key())) {
        return;
    }
    let Some(expr) = expressions.get(expr_id) else {
        return;
    };
    expr_stack.push(expr_id);
    match &expr.kind {
        AstExprKind::Call { function, args } => {
            if boon_typecheck::is_registered_element_constructor(function) {
                let node_kind = canonical_view_node_kind(function).to_owned();
                collect_canonical_call_arg_view_bindings(
                    &node_kind,
                    expr_id,
                    expressions,
                    row_scopes,
                    source_paths,
                    bindings,
                    context,
                );
                for arg in args {
                    if !arg.name.as_deref().is_none_or(attr_can_contain_render) {
                        continue;
                    }
                    collect_document_expr_view_bindings(
                        arg.value,
                        source_text,
                        expressions,
                        functions,
                        row_scopes,
                        source_paths,
                        render_slots,
                        bindings,
                        function_stack,
                        context,
                        visited_expr_contexts,
                        expr_stack,
                    );
                }
            } else if let Some(function_statement) = functions.get(function)
                && !function_stack.iter().any(|active| active == function)
            {
                function_stack.push(function.to_owned());
                let scoped_context = context.with_function_args(function_statement, args);
                collect_document_function_body_view_bindings(
                    function_statement,
                    source_text,
                    expressions,
                    functions,
                    row_scopes,
                    source_paths,
                    render_slots,
                    bindings,
                    function_stack,
                    visited_expr_contexts,
                    &scoped_context,
                );
                function_stack.pop();
            } else {
                for arg in args {
                    collect_document_expr_view_bindings(
                        arg.value,
                        source_text,
                        expressions,
                        functions,
                        row_scopes,
                        source_paths,
                        render_slots,
                        bindings,
                        function_stack,
                        context,
                        visited_expr_contexts,
                        expr_stack,
                    );
                }
            }
        }
        AstExprKind::Pipe { input, op, args } => {
            let scoped_context = if op == "SOURCE" {
                source_path_for_source_pipe_expr(expr, source_text, expressions, context)
                    .map(|path| context.with_source_base(path))
                    .unwrap_or_else(|| context.clone())
            } else {
                context.clone()
            };
            if boon_typecheck::is_registered_element_constructor(op) {
                let node_kind = canonical_view_node_kind(op).to_owned();
                collect_canonical_call_arg_view_bindings(
                    &node_kind,
                    expr_id,
                    expressions,
                    row_scopes,
                    source_paths,
                    bindings,
                    &scoped_context,
                );
                collect_document_expr_view_bindings(
                    *input,
                    source_text,
                    expressions,
                    functions,
                    row_scopes,
                    source_paths,
                    render_slots,
                    bindings,
                    function_stack,
                    &scoped_context,
                    visited_expr_contexts,
                    expr_stack,
                );
                for arg in args {
                    if !arg.name.as_deref().is_none_or(attr_can_contain_render) {
                        continue;
                    }
                    collect_document_expr_view_bindings(
                        arg.value,
                        source_text,
                        expressions,
                        functions,
                        row_scopes,
                        source_paths,
                        render_slots,
                        bindings,
                        function_stack,
                        &scoped_context,
                        visited_expr_contexts,
                        expr_stack,
                    );
                }
            } else if let Some(function_statement) = functions.get(op)
                && !function_stack.iter().any(|active| active == op)
            {
                function_stack.push(op.to_owned());
                let function_context =
                    scoped_context.with_pipe_function_call(function_statement, *input, args);
                collect_document_function_body_view_bindings(
                    function_statement,
                    source_text,
                    expressions,
                    functions,
                    row_scopes,
                    source_paths,
                    render_slots,
                    bindings,
                    function_stack,
                    visited_expr_contexts,
                    &function_context,
                );
                function_stack.pop();
            } else {
                collect_document_expr_view_bindings(
                    *input,
                    source_text,
                    expressions,
                    functions,
                    row_scopes,
                    source_paths,
                    render_slots,
                    bindings,
                    function_stack,
                    &scoped_context,
                    visited_expr_contexts,
                    expr_stack,
                );
                if op == "List/map"
                    && let Some(binding) = render_slots.for_expr(expr_id)
                    && let Some(function_name) = binding.template_function.as_deref()
                    && let Some(function_statement) = functions.get(function_name)
                    && !function_stack.iter().any(|active| active == function_name)
                {
                    function_stack.push(function_name.to_owned());
                    let function_context = if !binding.template_args.is_empty() {
                        scoped_context
                            .with_function_args(function_statement, &binding.template_args)
                    } else {
                        scoped_context
                            .with_function_item_expr(function_statement, binding.item_expr_id)
                    };
                    collect_document_function_body_view_bindings(
                        function_statement,
                        source_text,
                        expressions,
                        functions,
                        row_scopes,
                        source_paths,
                        render_slots,
                        bindings,
                        function_stack,
                        visited_expr_contexts,
                        &function_context,
                    );
                    function_stack.pop();
                }
                for arg in args {
                    collect_document_expr_view_bindings(
                        arg.value,
                        source_text,
                        expressions,
                        functions,
                        row_scopes,
                        source_paths,
                        render_slots,
                        bindings,
                        function_stack,
                        &scoped_context,
                        visited_expr_contexts,
                        expr_stack,
                    );
                }
            }
        }
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial } => {
            collect_document_expr_view_bindings(
                *initial,
                source_text,
                expressions,
                functions,
                row_scopes,
                source_paths,
                render_slots,
                bindings,
                function_stack,
                context,
                visited_expr_contexts,
                expr_stack,
            );
        }
        AstExprKind::Then { input, output } => {
            collect_document_expr_view_bindings(
                *input,
                source_text,
                expressions,
                functions,
                row_scopes,
                source_paths,
                render_slots,
                bindings,
                function_stack,
                context,
                visited_expr_contexts,
                expr_stack,
            );
            if let Some(output) = output {
                collect_document_expr_view_bindings(
                    *output,
                    source_text,
                    expressions,
                    functions,
                    row_scopes,
                    source_paths,
                    render_slots,
                    bindings,
                    function_stack,
                    context,
                    visited_expr_contexts,
                    expr_stack,
                );
            }
        }
        AstExprKind::Infix { left, right, .. } => {
            for value in [*left, *right] {
                collect_document_expr_view_bindings(
                    value,
                    source_text,
                    expressions,
                    functions,
                    row_scopes,
                    source_paths,
                    render_slots,
                    bindings,
                    function_stack,
                    context,
                    visited_expr_contexts,
                    expr_stack,
                );
            }
        }
        AstExprKind::MatchArm { output, .. } => {
            if let Some(output) = output {
                collect_document_expr_view_bindings(
                    *output,
                    source_text,
                    expressions,
                    functions,
                    row_scopes,
                    source_paths,
                    render_slots,
                    bindings,
                    function_stack,
                    context,
                    visited_expr_contexts,
                    expr_stack,
                );
            }
        }
        AstExprKind::Object(fields)
        | AstExprKind::Record(fields)
        | AstExprKind::TaggedObject { fields, .. } => {
            for field in fields {
                collect_document_expr_view_bindings(
                    field.value,
                    source_text,
                    expressions,
                    functions,
                    row_scopes,
                    source_paths,
                    render_slots,
                    bindings,
                    function_stack,
                    context,
                    visited_expr_contexts,
                    expr_stack,
                );
            }
        }
        AstExprKind::ListLiteral { items, .. } => {
            for item in items {
                collect_document_expr_view_bindings(
                    *item,
                    source_text,
                    expressions,
                    functions,
                    row_scopes,
                    source_paths,
                    render_slots,
                    bindings,
                    function_stack,
                    context,
                    visited_expr_contexts,
                    expr_stack,
                );
            }
        }
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => {}
    }
    expr_stack.pop();
}

fn collect_canonical_element_view_bindings(
    function: &str,
    element: &AstStatement,
    expressions: &[AstExpr],
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    bindings: &mut Vec<ViewBinding>,
    context: &DocumentViewBindingContext,
) {
    let node_kind = canonical_view_node_kind(function).to_owned();
    if let Some(expr_id) = element.expr {
        collect_canonical_call_arg_view_bindings(
            &node_kind,
            expr_id,
            expressions,
            row_scopes,
            source_paths,
            bindings,
            context,
        );
    }
    for child in &element.children {
        let Some(attr) = document_statement_field(child) else {
            continue;
        };
        if attr == "element" {
            collect_canonical_element_source_bindings(
                &node_kind,
                child,
                expressions,
                row_scopes,
                source_paths,
                bindings,
                context,
            );
            collect_canonical_element_data_bindings(
                &node_kind,
                child,
                expressions,
                row_scopes,
                bindings,
                context,
            );
            continue;
        }
        if attr == "style" {
            collect_style_statement_view_bindings(
                &node_kind,
                child,
                expressions,
                row_scopes,
                bindings,
                context,
            );
            continue;
        }
        if let Some(expr_id) = child.expr {
            push_data_view_binding_for_expr(
                &node_kind,
                &attr,
                expr_id,
                expressions,
                row_scopes,
                bindings,
                context,
            );
        }
        for nested in &child.children {
            if attr_can_bind_data(&attr)
                && document_statement_field(nested).as_deref() == Some("text")
                && let Some(expr_id) = nested.expr
                && let Some(path) = view_data_path_for_expr_id(expr_id, expressions, context)
                && view_data_binding_is_schedulable(&path, row_scopes, context)
            {
                bindings.push(ViewBinding {
                    id: ViewBindingId(bindings.len()),
                    node_kind: node_kind.clone(),
                    attr: attr.clone(),
                    scope_id: scope_id_for_path(row_scopes, &path),
                    source_id: None,
                    kind: ViewBindingKind::Data,
                    path,
                });
            }
        }
    }
}

fn collect_canonical_call_arg_view_bindings(
    node_kind: &str,
    expr_id: usize,
    expressions: &[AstExpr],
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    bindings: &mut Vec<ViewBinding>,
    context: &DocumentViewBindingContext,
) {
    let Some(expr) = expressions.get(expr_id) else {
        return;
    };
    let args = match &expr.kind {
        AstExprKind::Call { args, .. } | AstExprKind::Pipe { args, .. } => args.as_slice(),
        _ => return,
    };
    for arg in args {
        let Some(attr) = arg.name.as_deref() else {
            continue;
        };
        if attr == "style" {
            collect_style_expr_view_bindings(
                node_kind,
                arg.value,
                expressions,
                row_scopes,
                bindings,
                context,
                &mut BTreeSet::new(),
            );
            continue;
        }
        if attr == "element" {
            collect_canonical_element_source_bindings_from_expr(
                node_kind,
                arg.value,
                expressions,
                row_scopes,
                source_paths,
                bindings,
                context,
            );
            collect_canonical_element_data_bindings_from_expr(
                node_kind,
                arg.value,
                expressions,
                row_scopes,
                bindings,
                context,
            );
            continue;
        }
        push_data_view_binding_for_expr(
            node_kind,
            attr,
            arg.value,
            expressions,
            row_scopes,
            bindings,
            context,
        );
    }
}

fn collect_style_expr_view_bindings(
    node_kind: &str,
    expr_id: usize,
    expressions: &[AstExpr],
    row_scopes: &[RowScope],
    bindings: &mut Vec<ViewBinding>,
    context: &DocumentViewBindingContext,
    seen: &mut BTreeSet<usize>,
) {
    if !seen.insert(expr_id) {
        return;
    }
    let Some(resolved_expr_id) =
        document_resolved_expr_id(expr_id, expressions, context, &mut BTreeSet::new())
    else {
        return;
    };
    if resolved_expr_id != expr_id {
        collect_style_expr_view_bindings(
            node_kind,
            resolved_expr_id,
            expressions,
            row_scopes,
            bindings,
            context,
            seen,
        );
        return;
    }
    let Some(expr) = expressions.get(expr_id) else {
        return;
    };
    match &expr.kind {
        AstExprKind::Identifier(name) => {
            if let Some(arg_expr) = context.arg_expr(name) {
                collect_style_expr_view_bindings(
                    node_kind,
                    arg_expr,
                    expressions,
                    row_scopes,
                    bindings,
                    context,
                    seen,
                );
            }
        }
        AstExprKind::Object(fields)
        | AstExprKind::Record(fields)
        | AstExprKind::TaggedObject { fields, .. } => {
            for field in fields {
                push_data_view_binding_for_expr(
                    node_kind,
                    &field.name,
                    field.value,
                    expressions,
                    row_scopes,
                    bindings,
                    context,
                );
                collect_style_expr_view_bindings(
                    node_kind,
                    field.value,
                    expressions,
                    row_scopes,
                    bindings,
                    context,
                    seen,
                );
            }
        }
        AstExprKind::ListLiteral { items, .. } => {
            for item in items {
                collect_style_expr_view_bindings(
                    node_kind,
                    *item,
                    expressions,
                    row_scopes,
                    bindings,
                    context,
                    seen,
                );
            }
        }
        _ => {}
    }
}

fn collect_style_statement_view_bindings(
    node_kind: &str,
    statement: &AstStatement,
    expressions: &[AstExpr],
    row_scopes: &[RowScope],
    bindings: &mut Vec<ViewBinding>,
    context: &DocumentViewBindingContext,
) {
    if let Some(expr_id) = statement.expr {
        collect_style_expr_view_bindings(
            node_kind,
            expr_id,
            expressions,
            row_scopes,
            bindings,
            context,
            &mut BTreeSet::new(),
        );
    }
    for child in &statement.children {
        let Some(attr) = document_statement_field(child) else {
            continue;
        };
        if let Some(expr_id) = child.expr {
            push_data_view_binding_for_expr(
                node_kind,
                &attr,
                expr_id,
                expressions,
                row_scopes,
                bindings,
                context,
            );
            collect_style_expr_view_bindings(
                node_kind,
                expr_id,
                expressions,
                row_scopes,
                bindings,
                context,
                &mut BTreeSet::new(),
            );
        }
        if !child.children.is_empty() {
            collect_style_statement_view_bindings(
                node_kind,
                child,
                expressions,
                row_scopes,
                bindings,
                context,
            );
        }
    }
}

fn canonical_view_node_kind(function: &str) -> &str {
    let function = function
        .strip_prefix("Scene/Element/")
        .or_else(|| function.strip_prefix("Element/"))
        .unwrap_or(function);
    match function {
        "text_input" => "Input",
        "checkbox" => "Checkbox",
        "button" => "Button",
        "label" | "text" | "paragraph" | "link" => "Text",
        "stripe" => "Stripe",
        _ => function,
    }
}

fn collect_canonical_element_source_bindings(
    node_kind: &str,
    element_field: &AstStatement,
    expressions: &[AstExpr],
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    bindings: &mut Vec<ViewBinding>,
    context: &DocumentViewBindingContext,
) {
    if let Some(fields) = record_fields_for_statement(element_field, expressions) {
        collect_canonical_element_source_bindings_from_fields(
            node_kind,
            fields,
            expressions,
            row_scopes,
            source_paths,
            bindings,
            context,
        );
    }
    for event_field in &element_field.children {
        if let AstStatementKind::Source {
            field: Some(field),
            event,
        } = &event_field.kind
        {
            let attr = event.as_deref().unwrap_or(field.as_str());
            if let Some(value) = document_source_statement_value(event_field, expressions, context)
                .or_else(|| {
                    event.as_ref().and_then(|event| {
                        source_record_event_value(event_field, event, expressions, context)
                    })
                })
            {
                push_canonical_view_source_binding(
                    node_kind,
                    attr,
                    &value,
                    row_scopes,
                    source_paths,
                    bindings,
                );
            }
            continue;
        }
        if document_statement_field(event_field).as_deref() == Some("events") {
            if let Some(group_path) = document_statement_value(event_field, expressions, context) {
                push_canonical_view_event_group_bindings(
                    node_kind,
                    &group_path,
                    row_scopes,
                    source_paths,
                    bindings,
                );
            }
            continue;
        }
        if document_statement_field(event_field).as_deref() != Some("event") {
            continue;
        }
        if let Some(event_fields) = record_fields_for_statement(event_field, expressions) {
            for source_field in event_fields {
                let Some(value) =
                    document_source_expr_value_by_id(source_field.value, expressions, context)
                else {
                    continue;
                };
                push_canonical_view_source_binding(
                    node_kind,
                    &source_field.name,
                    &value,
                    row_scopes,
                    source_paths,
                    bindings,
                );
            }
            continue;
        }
        for source_field in &event_field.children {
            let Some(attr) = document_statement_field(source_field) else {
                continue;
            };
            let Some(value) = document_source_statement_value(source_field, expressions, context)
            else {
                continue;
            };
            push_canonical_view_source_binding(
                node_kind,
                &attr,
                &value,
                row_scopes,
                source_paths,
                bindings,
            );
        }
    }
}

fn source_record_event_value(
    statement: &AstStatement,
    event: &str,
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
) -> Option<String> {
    record_fields_for_statement(statement, expressions)?
        .iter()
        .find(|field| field.name == event)
        .and_then(|field| document_source_expr_value_by_id(field.value, expressions, context))
}

fn collect_canonical_element_source_bindings_from_expr(
    node_kind: &str,
    expr_id: usize,
    expressions: &[AstExpr],
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    bindings: &mut Vec<ViewBinding>,
    context: &DocumentViewBindingContext,
) {
    if let Some(fields) = record_fields_for_expr(expr_id, expressions) {
        collect_canonical_element_source_bindings_from_fields(
            node_kind,
            fields,
            expressions,
            row_scopes,
            source_paths,
            bindings,
            context,
        );
    }
}

fn collect_canonical_element_source_bindings_from_fields(
    node_kind: &str,
    fields: &[AstRecordField],
    expressions: &[AstExpr],
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    bindings: &mut Vec<ViewBinding>,
    context: &DocumentViewBindingContext,
) {
    for field in fields {
        if field.name == "events" {
            if let Some(group_path) = document_expr_value_by_id(field.value, expressions, context) {
                push_canonical_view_event_group_bindings(
                    node_kind,
                    &group_path,
                    row_scopes,
                    source_paths,
                    bindings,
                );
            }
            continue;
        }
        if field.name == "event"
            && let Some(event_fields) = record_fields_for_expr(field.value, expressions)
        {
            for source_field in event_fields {
                if let Some(value) =
                    document_source_expr_value_by_id(source_field.value, expressions, context)
                {
                    push_canonical_view_source_binding(
                        node_kind,
                        &source_field.name,
                        &value,
                        row_scopes,
                        source_paths,
                        bindings,
                    );
                }
            }
        }
    }
}

fn collect_canonical_element_data_bindings(
    node_kind: &str,
    element_field: &AstStatement,
    expressions: &[AstExpr],
    row_scopes: &[RowScope],
    bindings: &mut Vec<ViewBinding>,
    context: &DocumentViewBindingContext,
) {
    if let Some(fields) = record_fields_for_statement(element_field, expressions) {
        collect_canonical_element_data_bindings_from_fields(
            node_kind,
            fields,
            expressions,
            row_scopes,
            bindings,
            context,
        );
    }
    for child in &element_field.children {
        let Some(attr) = document_statement_field(child) else {
            continue;
        };
        if attr != "target" {
            continue;
        }
        if let Some(expr_id) = child.expr {
            push_data_view_binding_for_expr(
                node_kind,
                &attr,
                expr_id,
                expressions,
                row_scopes,
                bindings,
                context,
            );
        }
    }
}

fn collect_canonical_element_data_bindings_from_expr(
    node_kind: &str,
    expr_id: usize,
    expressions: &[AstExpr],
    row_scopes: &[RowScope],
    bindings: &mut Vec<ViewBinding>,
    context: &DocumentViewBindingContext,
) {
    if let Some(fields) = record_fields_for_expr(expr_id, expressions) {
        collect_canonical_element_data_bindings_from_fields(
            node_kind,
            fields,
            expressions,
            row_scopes,
            bindings,
            context,
        );
    }
}

fn collect_canonical_element_data_bindings_from_fields(
    node_kind: &str,
    fields: &[AstRecordField],
    expressions: &[AstExpr],
    row_scopes: &[RowScope],
    bindings: &mut Vec<ViewBinding>,
    context: &DocumentViewBindingContext,
) {
    for field in fields {
        if field.name != "target" {
            continue;
        }
        push_data_view_binding_for_expr(
            node_kind,
            &field.name,
            field.value,
            expressions,
            row_scopes,
            bindings,
            context,
        );
    }
}

fn push_canonical_view_event_group_bindings(
    node_kind: &str,
    group_path: &str,
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    bindings: &mut Vec<ViewBinding>,
) {
    let normalized_group = normalized_document_source_path(group_path);
    let canonical_group = canonical_view_source_group_path(source_paths, &normalized_group)
        .unwrap_or(normalized_group);
    let prefix = format!("{canonical_group}.");
    for (path, source_id) in source_paths
        .iter()
        .filter(|(source_path, _)| source_path.starts_with(&prefix))
    {
        let Some(attr) = path.rsplit('.').next() else {
            continue;
        };
        let binding_attr = if attr == "key_down" { "submit" } else { attr };
        bindings.push(ViewBinding {
            id: ViewBindingId(bindings.len()),
            node_kind: node_kind.to_owned(),
            attr: binding_attr.to_owned(),
            path: (*path).to_owned(),
            kind: ViewBindingKind::Source,
            scope_id: scope_id_for_path(row_scopes, path),
            source_id: Some(*source_id),
        });
    }
}

fn normalized_document_source_path(path: &str) -> String {
    path.split('.')
        .filter(|part| *part != "PASSED" && *part != "events")
        .collect::<Vec<_>>()
        .join(".")
}

fn canonical_view_source_path<'a>(
    source_paths: &'a [(&'a str, SourceId)],
    normalized_value: &str,
) -> Option<(&'a str, SourceId)> {
    if let Some((path, source_id)) = source_paths
        .iter()
        .find(|(source_path, _)| *source_path == normalized_value)
    {
        return Some((*path, *source_id));
    }
    let suffix = normalized_value.split_once('.')?.1;
    let suffix = format!(".{suffix}");
    let mut matches = source_paths
        .iter()
        .filter(|(source_path, _)| source_path.ends_with(&suffix));
    let first = matches.next()?;
    matches.next().is_none().then_some((first.0, first.1))
}

fn canonical_view_source_group_path(
    source_paths: &[(&str, SourceId)],
    normalized_group: &str,
) -> Option<String> {
    if source_paths
        .iter()
        .any(|(source_path, _)| source_path.starts_with(&format!("{normalized_group}.")))
    {
        return Some(normalized_group.to_owned());
    }
    let group_suffix = normalized_group.split_once('.')?.1;
    let suffix = format!(".{group_suffix}.");
    let mut prefixes = source_paths.iter().filter_map(|(source_path, _)| {
        let prefix_end = source_path.find(&suffix)? + suffix.len() - 1;
        Some(source_path[..prefix_end].to_owned())
    });
    let first = prefixes.next()?;
    prefixes.all(|prefix| prefix == first).then_some(first)
}

fn push_canonical_view_source_binding(
    node_kind: &str,
    attr: &str,
    value: &str,
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    bindings: &mut Vec<ViewBinding>,
) {
    let normalized_value = normalized_document_source_path(value);
    if let Some((path, source_id)) = canonical_view_source_path(source_paths, &normalized_value) {
        let binding_attr = if attr == "key_down" { "submit" } else { attr };
        bindings.push(ViewBinding {
            id: ViewBindingId(bindings.len()),
            node_kind: node_kind.to_owned(),
            attr: binding_attr.to_owned(),
            path: path.to_owned(),
            kind: ViewBindingKind::Source,
            scope_id: scope_id_for_path(row_scopes, path),
            source_id: Some(source_id),
        });
    }
}

fn collect_document_element_bindings(
    node_kind: &str,
    element: &AstStatement,
    expressions: &[AstExpr],
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    bindings: &mut Vec<ViewBinding>,
    context: &DocumentViewBindingContext,
) {
    for child in &element.children {
        let Some(attr) = document_statement_field(child) else {
            continue;
        };
        if matches!(attr.as_str(), "kind" | "children") {
            continue;
        }
        let Some(value) = document_statement_value(child, expressions, context) else {
            continue;
        };
        if attr != "target"
            && let Some((path, source_id)) = source_paths
                .iter()
                .find(|(source_path, _)| *source_path == value)
        {
            bindings.push(ViewBinding {
                id: ViewBindingId(bindings.len()),
                node_kind: node_kind.to_owned(),
                attr,
                path: (*path).to_owned(),
                kind: ViewBindingKind::Source,
                scope_id: scope_id_for_path(row_scopes, path),
                source_id: Some(*source_id),
            });
        } else if attr_can_bind_data(&attr)
            && let Some(expr_id) = child.expr
            && let Some(path) = view_data_path_for_expr_id(expr_id, expressions, context)
            && view_data_binding_is_schedulable(&path, row_scopes, context)
        {
            bindings.push(ViewBinding {
                id: ViewBindingId(bindings.len()),
                node_kind: node_kind.to_owned(),
                attr: attr.clone(),
                scope_id: scope_id_for_path(row_scopes, &path),
                source_id: None,
                kind: if attr == "target" {
                    ViewBindingKind::Target
                } else {
                    ViewBindingKind::Data
                },
                path,
            });
        }
    }
}

fn document_child_value(
    statement: &AstStatement,
    field: &str,
    expressions: &[AstExpr],
) -> Option<String> {
    statement
        .children
        .iter()
        .find(|child| document_statement_field(child).as_deref() == Some(field))
        .and_then(|child| {
            document_statement_value(child, expressions, &DocumentViewBindingContext::default())
        })
}

fn document_statement_field(statement: &AstStatement) -> Option<String> {
    match &statement.kind {
        AstStatementKind::Field { name } => Some(name.clone()),
        AstStatementKind::Source {
            field: Some(name), ..
        } => Some(name.clone()),
        AstStatementKind::List {
            field: Some(name), ..
        } => Some(name.clone()),
        _ => None,
    }
}

fn document_statement_call<'a>(
    statement: &AstStatement,
    expressions: &'a [AstExpr],
) -> Option<&'a str> {
    let expr = expressions.get(statement.expr?)?;
    match &expr.kind {
        AstExprKind::Call { function, .. } => Some(function.as_str()),
        _ => None,
    }
}

fn document_call_args<'a>(
    statement: &AstStatement,
    expressions: &'a [AstExpr],
) -> Option<&'a [boon_parser::AstCallArg]> {
    let expr = expressions.get(statement.expr?)?;
    match &expr.kind {
        AstExprKind::Call { args, .. } => Some(args.as_slice()),
        _ => None,
    }
}

fn record_fields_for_statement<'a>(
    statement: &AstStatement,
    expressions: &'a [AstExpr],
) -> Option<&'a [AstRecordField]> {
    record_fields_for_expr(statement.expr?, expressions)
}

fn record_fields_for_expr(expr_id: usize, expressions: &[AstExpr]) -> Option<&[AstRecordField]> {
    match &expressions.get(expr_id)?.kind {
        AstExprKind::Record(fields) | AstExprKind::Object(fields) => Some(fields.as_slice()),
        _ => None,
    }
}

fn document_expr_value_by_id(
    expr_id: usize,
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
) -> Option<String> {
    let expr_id = document_resolved_expr_id(expr_id, expressions, context, &mut BTreeSet::new())?;
    document_expr_value(expressions.get(expr_id)?, expressions, context)
}

fn document_source_expr_value_by_id(
    expr_id: usize,
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
) -> Option<String> {
    let expr_id = document_resolved_expr_id(expr_id, expressions, context, &mut BTreeSet::new())?;
    match &expressions.get(expr_id)?.kind {
        AstExprKind::Source => context.source_base().map(str::to_owned),
        _ => document_expr_value(expressions.get(expr_id)?, expressions, context),
    }
}

fn document_source_statement_value(
    statement: &AstStatement,
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
) -> Option<String> {
    let expr_id = statement.expr?;
    document_source_expr_value_by_id(expr_id, expressions, context)
}

fn document_statement_value(
    statement: &AstStatement,
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
) -> Option<String> {
    let expr_id = statement.expr?;
    let expr_id = document_resolved_expr_id(expr_id, expressions, context, &mut BTreeSet::new())?;
    document_expr_value(expressions.get(expr_id)?, expressions, context)
}

fn document_resolved_expr_id(
    expr_id: usize,
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
    seen: &mut BTreeSet<usize>,
) -> Option<usize> {
    if !seen.insert(expr_id) {
        return Some(expr_id);
    }
    match &expressions.get(expr_id)?.kind {
        AstExprKind::Identifier(value) => context
            .arg_expr(value)
            .and_then(|mapped| document_resolved_expr_id(mapped, expressions, context, seen)),
        AstExprKind::Path(parts) if parts.len() == 1 => context
            .arg_expr(&parts[0])
            .and_then(|mapped| document_resolved_expr_id(mapped, expressions, context, seen)),
        _ => None,
    }
    .or(Some(expr_id))
}

fn document_expr_value(
    expr: &AstExpr,
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
) -> Option<String> {
    match &expr.kind {
        AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => Some(value.clone()),
        AstExprKind::Number(value) | AstExprKind::Enum(value) | AstExprKind::Tag(value) => {
            Some(value.clone())
        }
        AstExprKind::TaggedObject { tag, fields } => {
            Some(tagged_object_value(tag, fields, expressions, context))
        }
        AstExprKind::Identifier(value) => context
            .arg_expr(value)
            .filter(|expr_id| {
                !expressions
                    .get(*expr_id)
                    .is_some_and(|expr| expr_is_same_identifier_path(expr, value))
            })
            .and_then(|expr_id| document_expr_value_by_id(expr_id, expressions, context))
            .or_else(|| Some(value.clone())),
        AstExprKind::Bool(value) => Some(value.to_string()),
        AstExprKind::Path(parts) => document_path_value(parts, expressions, context),
        AstExprKind::Pipe { input, op, args } => {
            let mut value = document_expr_value_by_id(*input, expressions, context)?;
            value.push_str("|>");
            value.push_str(op);
            if !args.is_empty() {
                value.push('(');
                value.push_str(
                    &args
                        .iter()
                        .filter_map(|arg| {
                            let mut arg_value =
                                document_expr_value_by_id(arg.value, expressions, context)?;
                            if let Some(name) = &arg.name {
                                arg_value = format!("{name}:{arg_value}");
                            }
                            Some(arg_value)
                        })
                        .collect::<Vec<_>>()
                        .join(","),
                );
                value.push(')');
            }
            Some(value)
        }
        _ => None,
    }
}

fn document_path_value(
    parts: &[String],
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
) -> Option<String> {
    let first = parts.first()?;
    if parts.len() > 1
        && let Some(expr_id) = context.arg_expr(first)
        && !expressions
            .get(expr_id)
            .is_some_and(|expr| expr_is_same_identifier_path(expr, first))
        && let Some(mut value) = document_expr_value_by_id(expr_id, expressions, context)
    {
        value.push('.');
        value.push_str(&parts[1..].join("."));
        return Some(value);
    }
    Some(parts.join("."))
}

fn expr_is_same_identifier_path(expr: &AstExpr, name: &str) -> bool {
    match &expr.kind {
        AstExprKind::Identifier(value) => value == name,
        AstExprKind::Path(parts) => parts.as_slice() == [name],
        _ => false,
    }
}

fn tagged_object_value(
    tag: &str,
    fields: &[AstRecordField],
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
) -> String {
    let body = fields
        .iter()
        .filter_map(|field| {
            let value = document_expr_value_by_id(field.value, expressions, context)?;
            Some(format!("{}:{value}", field.name))
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{tag}[{body}]")
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
            known
                .rsplit_once('.')
                .is_some_and(|(_, local)| local == value)
        })
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

fn list_projection_view_symbols(program: &TypedProgram) -> BTreeSet<String> {
    let mut symbols = BTreeSet::new();
    for projection in &program.list_projections {
        symbols.insert(projection.target.clone());
        if !matches!(projection.kind, ListProjectionKind::Find { .. }) {
            continue;
        }
        let Some(row_scope) = program
            .row_scopes
            .iter()
            .find(|scope| scope.list == projection.list)
            .map(|scope| scope.row_scope.as_str())
        else {
            continue;
        };
        let prefix = format!("{row_scope}.");
        for field in program
            .state_cells
            .iter()
            .map(|field| field.path.as_str())
            .chain(
                program
                    .derived_values
                    .iter()
                    .map(|field| field.path.as_str()),
            )
            .filter_map(|path| path.strip_prefix(&prefix))
        {
            symbols.insert(format!(
                "{}.{}",
                projection.target,
                projection_field_name(field)
            ));
        }
    }
    symbols
}

fn projection_field_name(path: &str) -> &str {
    path.rsplit_once('.')
        .map(|(_, field)| field)
        .unwrap_or(path)
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
            if left.parse::<i64>().is_err() {
                require_known_symbol("number infix left", left, known_symbols)?;
            }
            if right.parse::<i64>().is_err() {
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
                if path.parse::<i64>().is_err() && !source_payload_input_matches(path, source) {
                    require_known_symbol(context, path, known_symbols)?;
                }
            }
            Ok(())
        }
        UpdateExpression::MatchNumberInfixConst {
            left,
            op,
            right,
            arms,
        } => {
            require_supported_numeric_update_op(op, "match number infix")?;
            if left.parse::<i64>().is_err() {
                require_known_symbol("match number infix left", left, known_symbols)?;
            }
            if right.parse::<i64>().is_err() {
                require_known_symbol("match number infix right", right, known_symbols)?;
            }
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
        UpdateExpression::MatchValueConst { input, arms } => {
            if !source_payload_input_matches(input, source) {
                require_known_symbol("match value input", input, known_symbols)?;
            }
            for arm in arms {
                verify_update_value_expression(&arm.output, known_symbols, "match value arm")?;
            }
            Ok(())
        }
        UpdateExpression::ListFindValue {
            list,
            expected,
            fallback,
            ..
        } => {
            require_known_symbol("list find value list", list, known_symbols)?;
            verify_update_value_expression(expected, known_symbols, "list find value expected")?;
            if let Some(fallback) = fallback {
                verify_update_value_expression(
                    fallback,
                    known_symbols,
                    "list find value fallback",
                )?;
            }
            Ok(())
        }
        UpdateExpression::Unknown { summary } => Err(format!(
            "static schedule contains unsupported update expression for `{target}` from `{source}`: `{summary}`"
        )),
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
        UpdateValueExpression::NumberInfix { left, op, right } => {
            require_supported_numeric_update_op(op, &format!("{context} number infix"))?;
            if left.parse::<i64>().is_err() {
                require_known_symbol(&format!("{context} number infix left"), left, known_symbols)?;
            }
            if right.parse::<i64>().is_err() {
                require_known_symbol(
                    &format!("{context} number infix right"),
                    right,
                    known_symbols,
                )?;
            }
            Ok(())
        }
        UpdateValueExpression::MatchNumberInfixConst {
            left,
            op,
            right,
            arms,
        } => {
            require_supported_numeric_update_op(op, &format!("{context} match number infix"))?;
            if left.parse::<i64>().is_err() {
                require_known_symbol(
                    &format!("{context} match number infix left"),
                    left,
                    known_symbols,
                )?;
            }
            if right.parse::<i64>().is_err() {
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
    let mentioned_identifiers = fields
        .iter()
        .map(|field| {
            let mut identifiers = BTreeSet::new();
            for item in &field.ast_items {
                for symbol in &item.symbols {
                    identifiers.insert(symbol.as_str());
                }
            }
            identifiers
        })
        .collect::<Vec<_>>();
    let mut fields_by_parent = BTreeMap::<&str, Vec<usize>>::new();
    for (index, field) in fields.iter().enumerate() {
        fields_by_parent
            .entry(field.parent_path.as_str())
            .or_default()
            .push(index);
    }
    let mut dependency_edges = vec![Vec::<usize>::new(); fields.len()];
    for (field_index, field) in fields.iter().enumerate() {
        if excluded_field[field_index] {
            continue;
        }
        let Some(siblings) = fields_by_parent.get(field.parent_path.as_str()) else {
            continue;
        };
        for &dependency_index in siblings {
            let dependency = &fields[dependency_index];
            if dependency_index == field_index
                || excluded_field[dependency_index]
                || dependency.local_name == field.local_name
            {
                continue;
            }
            if mentioned_identifiers[field_index].contains(dependency.local_name.as_str()) {
                dependency_edges[field_index].push(dependency_index);
            }
        }
    }
    (excluded_field, dependency_edges)
}

fn verify_combinational_field_cycles(
    fields: &[FieldDef],
    state_cells: &[StateCell],
) -> Result<(), String> {
    let state_paths = state_cells
        .iter()
        .map(|cell| cell.path.as_str())
        .collect::<BTreeSet<_>>();
    let (state_field, dependency_edges) = field_symbol_dependency_graph(fields, &state_paths);

    let mut visits = vec![FieldCycleVisit::Pending; fields.len()];
    for (field_index, is_state_field) in state_field.iter().enumerate() {
        if *is_state_field {
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
                "combinational dependency cycle through pure/WHILE expressions must be broken by HOLD: {}",
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

fn verify_identity_clean_identifiers(program: &TypedProgram) -> Result<(), String> {
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
            ListProjectionKind::Find { field, value } => {
                reject_hidden_identity_identifier("list find field", field)?;
                reject_hidden_identity_identifier("list find value", value)?;
            }
        }
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
        InitialValue::Text { .. } | InitialValue::Number { .. } | InitialValue::Bool { .. } => {
            Ok(())
        }
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
        | UpdateExpression::BoolNot { path } => {
            reject_hidden_identity_identifier("update expression path", path)
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
        UpdateExpression::MatchNumberInfixConst {
            left, right, arms, ..
        } => {
            reject_hidden_identity_identifier("match number infix left", left)?;
            reject_hidden_identity_identifier("match number infix right", right)?;
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
        UpdateExpression::MatchValueConst { input, arms } => {
            reject_hidden_identity_identifier("match value input", input)?;
            for arm in arms {
                reject_hidden_identity_identifier("match pattern", &arm.pattern)?;
                reject_update_value_expression_identity(&arm.output)?;
            }
            Ok(())
        }
        UpdateExpression::ListFindValue {
            list,
            field,
            expected,
            target,
            fallback,
        } => {
            reject_hidden_identity_identifier("list find value list", list)?;
            reject_hidden_identity_identifier("list find value field", field)?;
            reject_update_value_expression_identity(expected)?;
            reject_hidden_identity_identifier("list find value target", target)?;
            if let Some(fallback) = fallback {
                reject_update_value_expression_identity(fallback)?;
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
        UpdateValueExpression::NumberInfix { left, right, .. } => {
            reject_hidden_identity_identifier("match output number infix left", left)?;
            reject_hidden_identity_identifier("match output number infix right", right)
        }
        UpdateValueExpression::MatchNumberInfixConst {
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

pub fn debug_tables(program: &TypedProgram) -> serde_json::Value {
    serde_json::json!({
        "semantic_index": program.semantic_index,
        "semantic_index_report": program.semantic_index.report(),
        "expression_coverage": program.expression_coverage,
        "row_scopes": program.row_scopes,
        "sources": program.sources,
        "state_cells": program.state_cells,
        "lists": program.lists,
        "derived_values": program.derived_values,
        "dependencies": program.dependencies,
        "possible_causes": program.possible_causes,
        "update_branches": program.update_branches,
        "list_operations": program.list_operations,
        "list_projections": program.list_projections,
        "functions": program.functions,
        "view_bindings": program.view_bindings,
        "typecheck_report": program.typecheck_report,
        "render_slot_table": program.typecheck_report.render_slot_table,
        "list_map_bindings": program.typecheck_report.list_map_bindings,
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
) -> ExpressionCoverage {
    let mut coverage = ExpressionCoverage {
        ast_expression_count: program.expressions.len(),
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
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::TaggedObject { .. }
        | AstExprKind::Infix { .. }
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
        AstExprKind::Path(parts) => parts.join("."),
        AstExprKind::StringLiteral(_) => "string_literal".to_owned(),
        AstExprKind::TextLiteral(_) => "text_literal".to_owned(),
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
) -> Vec<UpdateBranch> {
    let state_paths = cells
        .iter()
        .map(|cell| cell.path.as_str())
        .collect::<BTreeSet<_>>();
    cells
        .iter()
        .flat_map(|cell| {
            let Some(field) = fields.iter().find(|field| field.path == cell.path) else {
                return Vec::new();
            };
            let mut branches = direct_sources_for_field(direct_sources, field)
                .cloned()
                .map(|source| UpdateBranch {
                    expression: update_expression_for_source(
                        program, &cell.path, field, fields, &source,
                    ),
                    indexed: cell.indexed,
                    target: cell.path.clone(),
                    source,
                })
                .collect::<Vec<_>>();
            branches.extend(derived_dependency_update_branches(
                program,
                fields,
                field,
                cell,
                &state_paths,
                &branches,
                candidate_sources,
            ));
            branches.extend(derived_then_empty_update_branches(
                &fields,
                field,
                cell,
                direct_sources,
            ));
            branches
        })
        .collect()
}

fn derived_dependency_update_branches(
    program: &ParsedProgram,
    fields: &[FieldDef],
    field: &FieldDef,
    cell: &StateCell,
    state_paths: &BTreeSet<&str>,
    existing_branches: &[UpdateBranch],
    candidate_sources: &mut CandidateSourceIndex<'_>,
) -> Vec<UpdateBranch> {
    let mut branches = Vec::new();
    for dependency in fields.iter().filter(|dependency| {
        dependency.parent_path == field.parent_path
            && dependency.path != field.path
            && !state_paths.contains(dependency.path.as_str())
            && field.mentions_identifier_expr(&dependency.local_name)
    }) {
        for source in candidate_sources.candidate_sources(&dependency.path) {
            if existing_branches
                .iter()
                .chain(branches.iter())
                .any(|branch: &UpdateBranch| branch.source == source)
            {
                continue;
            }
            let Some(expression) = update_expression_for_derived_dependency_source(
                program, &cell.path, field, fields, dependency, &source,
            ) else {
                continue;
            };
            branches.push(UpdateBranch {
                expression,
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
                indexed: cell.indexed,
                target: cell.path.clone(),
                source,
            });
        }
    }
    branches
}

fn list_operations(program: &ParsedProgram) -> Vec<ListOperation> {
    let fields = typed_field_defs(program);
    let mut operations = Vec::new();
    for field in &fields {
        let Some(list_name) = field.path.strip_prefix("store.") else {
            continue;
        };
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
            operations.push(ListOperation {
                list: list_name.to_owned(),
                kind: ListOperationKind::Append { trigger, fields },
            });
        }
        for source in direct_source_refs(field, program) {
            let branch = field.source_branch(&source).unwrap_or_default();
            if branch.has_token("List/remove")
                || field.has_token("List/remove")
                || (field.has_operator("List/retain") && branch.has_token("False"))
            {
                let row_scope = row_scope_for_list(program, list_name);
                operations.push(ListOperation {
                    list: list_name.to_owned(),
                    kind: ListOperationKind::Remove {
                        predicate: list_remove_predicate(field, &source, &branch, row_scope),
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
            operations.push(ListOperation {
                list,
                kind: ListOperationKind::Count {
                    target: field.path.clone(),
                    predicate: list_retain_predicate(field, row_scope.as_deref()),
                },
            });
        } else if field.has_operator("List/retain") && !field_retains_into_materialized_map(field) {
            let Some(list) = count_or_retain_source_list(field, program) else {
                continue;
            };
            let row_scope = ast_call_argument(field, "List/retain")
                .or_else(|| row_scope_for_list(program, &list).map(str::to_owned));
            for source in retain_remove_sources(field, program, row_scope.as_deref()) {
                let branch = field.source_branch(&source).unwrap_or_default();
                operations.push(ListOperation {
                    list: list.clone(),
                    kind: ListOperationKind::Remove {
                        predicate: list_retain_remove_predicate(
                            field,
                            &source,
                            &branch,
                            row_scope.as_deref(),
                        ),
                        source,
                    },
                });
            }
            operations.push(ListOperation {
                list,
                kind: ListOperationKind::Retain {
                    target: field.path.clone(),
                    predicate: list_retain_predicate(field, row_scope.as_deref()),
                },
            });
        }
    }
    operations
}

fn field_retains_into_materialized_map(field: &FieldDef) -> bool {
    let Some(first_retain_id) = field
        .ast_exprs
        .iter()
        .filter_map(|expr| {
            matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "List/retain")
                .then_some(expr.id)
        })
        .min()
    else {
        return false;
    };
    field.ast_exprs.iter().any(|expr| {
        expr.id > first_retain_id
            && matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "List/map")
    })
}

fn list_projections(program: &ParsedProgram) -> Vec<ListProjection> {
    typed_field_defs(program)
        .into_iter()
        .filter_map(|field| {
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
            if field.has_operator("List/find") {
                return Some(ListProjection {
                    target: field.path.clone(),
                    list: ast_list_projection_argument(program, &field, "List/find")?,
                    kind: ListProjectionKind::Find {
                        field: ast_named_call_argument(&field, "List/find", "field")?,
                        value: canonical_local_path(
                            &ast_named_call_argument(&field, "List/find", "value")?,
                            &field.parent_path,
                        ),
                    },
                });
            }
            None
        })
        .collect()
}

fn ast_list_projection_argument(
    program: &ParsedProgram,
    field: &FieldDef,
    function: &str,
) -> Option<String> {
    let raw = ast_call_argument(field, function)?;
    Some(resolve_list_memory_argument(program, &raw, &field.parent_path).unwrap_or(raw))
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
    row_scopes: &[RowScope],
    fields: &[FieldDef],
    state_cells: &[StateCell],
    direct_sources: &BTreeMap<String, Vec<String>>,
) -> Vec<DerivedValue> {
    fields
        .iter()
        .filter(|field| {
            let indexed_field = path_has_parsed_row_scope(program, &field.path);
            let list_memory_path = field_is_list_memory_path(field, program);
            !state_cells.iter().any(|cell| cell.path == field.path)
                && (indexed_field
                    || !list_memory_path
                    || field_is_derived_list_memory_view(field, program))
        })
        .enumerate()
        .map(|(id, field)| {
            let sources = direct_sources_for_field(direct_sources, field)
                .cloned()
                .collect::<Vec<_>>();
            let list_memory_view = field_is_derived_list_memory_view(field, program);
            DerivedValue {
                id: FieldId(id),
                indexed: path_has_parsed_row_scope(program, &field.path),
                scope_id: scope_id_for_path(row_scopes, &field.path),
                kind: if list_memory_view {
                    DerivedValueKind::ListView
                } else {
                    derived_value_kind(field, &sources)
                },
                path: field.path.clone(),
                sources,
                statement: field.statement.clone(),
            }
        })
        .collect()
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
    "List/retain",
    "List/filter_text_contains",
    "List/filter_field_equal",
    "List/filter_field_not_equal",
    "List/move_field_first",
    "List/move_field_last",
    "WHEN",
];

fn list_initializer_has_dynamic_fields(rows: &[ListInitialRecord]) -> bool {
    rows.iter().any(|row| {
        row.fields
            .iter()
            .any(|field| matches!(field.value, InitialValue::Unknown { .. }))
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
        if let AstStatementKind::Function { name, args } = &statement.kind {
            functions.push(FunctionDefinition {
                name: name.clone(),
                args: args.clone(),
                statement: statement.clone(),
            });
        }
        collect_function_definitions(&statement.children, functions);
    }
}

fn derived_value_kind(field: &FieldDef, sources: &[String]) -> DerivedValueKind {
    if field.has_operator("List/count") || field.has_operator("List/every") {
        DerivedValueKind::Aggregate
    } else if field.has_operator("List/latest") {
        if !sources.is_empty() || field.has_then_expr() {
            DerivedValueKind::SourceEventTransform
        } else {
            DerivedValueKind::Pure
        }
    } else if field.has_any_operator(&[
        "List/retain",
        "List/map",
        "List/chunk",
        "List/find",
        "List/filter_text_contains",
        "List/filter_field_equal",
        "List/filter_field_not_equal",
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

fn field_initial_value(field: &FieldDef, row_scopes: &[RowScope]) -> InitialValue {
    let initial_expr = if let Some(initial) =
        field.ast_exprs.iter().find_map(|expr| match expr.kind {
            AstExprKind::Hold { initial, .. } => Some(initial),
            AstExprKind::Pipe { input, ref op, .. } if op == "HOLD" => Some(input),
            _ => None,
        }) {
        field.ast_exprs.iter().find(|expr| expr.id == initial)
    } else {
        field
            .ast_exprs
            .iter()
            .find(|expr| !matches!(expr.kind, AstExprKind::Latest))
    };
    let Some(expr) = initial_expr else {
        return InitialValue::Unknown {
            summary: "missing initial value".to_owned(),
        };
    };
    let current_row_scope = row_scopes
        .iter()
        .find(|scope| field.path.starts_with(&format!("{}.", scope.row_scope)))
        .map(|scope| scope.row_scope.as_str());
    ast_initial_value(expr, row_scopes, current_row_scope)
}

fn ast_initial_value(
    expr: &AstExpr,
    row_scopes: &[RowScope],
    current_row_scope: Option<&str>,
) -> InitialValue {
    match &expr.kind {
        AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => InitialValue::Text {
            value: value.clone(),
        },
        AstExprKind::Number(value) => value
            .parse::<i64>()
            .map(|value| InitialValue::Number { value })
            .unwrap_or_else(|_| InitialValue::Unknown {
                summary: value.clone(),
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
        return InitialValue::Number { value };
    }
    let value = tokens_to_path(tokens);
    match value.as_str() {
        "True" => InitialValue::Bool { value: true },
        "False" => InitialValue::Bool { value: false },
        value if value.parse::<i64>().is_ok() => InitialValue::Number {
            value: value.parse().unwrap_or_default(),
        },
        value if value_starts_uppercase_identifier(value) => InitialValue::Enum {
            value: value.to_owned(),
        },
        value => InitialValue::Unknown {
            summary: value.to_owned(),
        },
    }
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
    field
        .ast_exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            AstExprKind::Call {
                function: call_function,
                args,
            } if call_function == function => Some(args.as_slice()),
            AstExprKind::Pipe { op, args, .. } if op == function => Some(args.as_slice()),
            _ => None,
        })
        .into_iter()
        .flatten()
        .filter(|arg| arg.name.is_none())
        .filter_map(|arg| ast_argument_value(field, arg.value))
        .collect()
}

fn ast_named_call_argument(field: &FieldDef, function: &str, name: &str) -> Option<String> {
    field
        .ast_exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            AstExprKind::Call {
                function: call_function,
                args,
            } if call_function == function => Some(args.as_slice()),
            AstExprKind::Pipe { op, args, .. } if op == function => Some(args.as_slice()),
            _ => None,
        })?
        .iter()
        .find(|arg| arg.name.as_deref() == Some(name))
        .and_then(|arg| ast_argument_value(field, arg.value))
}

fn ast_argument_value(field: &FieldDef, expr_id: usize) -> Option<String> {
    ast_argument_value_in_exprs(&field.ast_exprs, expr_id)
}

fn scalar_number_operand(field: &FieldDef, expr_id: usize, target: &str) -> Option<String> {
    let value = ast_argument_value(field, expr_id)?;
    if value.parse::<i64>().is_ok() {
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
        AstExprKind::Path(parts) => parts.join("."),
        AstExprKind::Bool(true) => "True".to_owned(),
        AstExprKind::Bool(false) => "False".to_owned(),
        AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => value.clone(),
        AstExprKind::Unknown(tokens) => tokens_to_path(tokens),
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
        | AstExprKind::ListLiteral { .. } => ast_expr_label(expr),
    })
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
        AstExprKind::Bool(true) => Some(SimpleThenUpdateValue::Const("True".to_owned())),
        AstExprKind::Bool(false) => Some(SimpleThenUpdateValue::Const("False".to_owned())),
        _ => None,
    }
}

fn list_append_trigger(field: &FieldDef, append_expr: &AstExpr) -> Option<String> {
    let AstExprKind::Pipe { args, .. } = &append_expr.kind else {
        return None;
    };
    let item_arg = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some("item"))?;
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
    list_append_function_constructor_fields(field, program, fields, append_expr)
}

fn list_append_item_record_fields<'a>(
    field: &'a FieldDef,
    append_expr: &AstExpr,
) -> Option<&'a [AstRecordField]> {
    let item_expr = list_append_item_expr(field, append_expr)?;
    append_item_record_fields_from_expr(field, item_expr.id).or_else(|| {
        field
            .ast_exprs
            .iter()
            .filter(|expr| expr.line >= item_expr.line)
            .find_map(record_fields_from_expr)
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
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial } => {
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
                field_initial_value(candidate, &row_scopes)
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
        AstExprKind::Pipe { input, op, args } => Some((
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
        AstExprKind::Call { function, args } => Some((
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
    let mut positional_index = 0usize;
    if let Some(input) = piped_input {
        let arg_name = function_args.first()?;
        let source = ast_argument_value(field, input)?;
        sources.insert(arg_name.clone(), canonical_local_path(&source, parent_path));
        positional_index = 1;
    }
    for arg in args {
        let arg_name = if let Some(name) = arg.name.as_ref() {
            name.clone()
        } else {
            let name = function_args.get(positional_index)?.clone();
            positional_index += 1;
            name
        };
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
    let item_arg = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some("item"))?;
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
) -> Vec<String> {
    let mut sources = direct_source_refs(field, program)
        .into_iter()
        .filter(|source| {
            let scoped = program
                .source_ports
                .iter()
                .find(|port| port.path == *source)
                .is_some_and(|port| port.scoped);
            scoped || retain_source_predicate(field, source, row_scope).is_some()
        })
        .collect::<Vec<_>>();
    for source in &program.source_ports {
        if retain_source_predicate(field, &source.path, row_scope).is_some() {
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
    if let Some(retain_predicate) = retain_source_predicate(field, source, row_scope)
        && let Some(remove_predicate) = invert_retain_predicate(retain_predicate)
    {
        return remove_predicate;
    }
    list_remove_predicate(field, source, branch, row_scope)
}

fn retain_source_predicate(
    field: &FieldDef,
    source: &str,
    row_scope: Option<&str>,
) -> Option<ListPredicate> {
    list_remove_predicate_from_then_output(field, source, row_scope).or_else(|| {
        let branch = field.source_branch(source)?;
        let path = row_field_path_in_exprs(branch.ast_exprs(), row_scope)?;
        if branch.bool_not_path().as_deref() == Some(path.as_str()) {
            Some(ListPredicate::RowFieldBoolNot { path })
        } else {
            Some(ListPredicate::RowFieldBool { path })
        }
    })
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
) -> ListPredicate {
    if source
        .split('.')
        .any(|segment| segment.contains("remove") || segment.contains("delete"))
    {
        return ListPredicate::AlwaysTrue;
    }
    if let Some(predicate) = list_remove_predicate_from_then_output(field, source, row_scope) {
        return predicate;
    }
    if branch.has_bool_expr(true) {
        return ListPredicate::AlwaysTrue;
    }
    if let Some(path) = row_field_path_in_exprs(branch.ast_exprs(), row_scope)
        && branch.bool_not_path().as_deref() == Some(path.as_str())
    {
        return ListPredicate::RowFieldBoolNot { path };
    }
    if let Some(path) = row_field_path_in_exprs(branch.ast_exprs(), row_scope) {
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
        list_predicate_from_expr(field, output, row_scope)
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
) -> Option<ListPredicate> {
    let expr = field.ast_exprs.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::Bool(true) => Some(ListPredicate::AlwaysTrue),
        AstExprKind::Latest => latest_default_list_predicate(field, expr_id, row_scope),
        AstExprKind::Pipe { input, op, .. } if op == "Bool/not" => {
            row_field_path_from_expr(field, *input, row_scope)
                .map(|path| ListPredicate::RowFieldBoolNot { path })
        }
        _ => row_field_path_from_expr(field, expr_id, row_scope)
            .map(|path| ListPredicate::RowFieldBool { path }),
    }
}

fn latest_default_list_predicate(
    field: &FieldDef,
    latest_expr_id: usize,
    row_scope: Option<&str>,
) -> Option<ListPredicate> {
    let statement = statement_containing_expr(&field.statement, latest_expr_id)?;
    statement
        .children
        .iter()
        .find_map(|child| child.expr)
        .and_then(|expr_id| list_predicate_from_expr(field, expr_id, row_scope))
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

fn row_field_path_from_expr(
    field: &FieldDef,
    expr_id: usize,
    row_scope: Option<&str>,
) -> Option<String> {
    let row_scope = row_scope?;
    let expr = field.ast_exprs.iter().find(|expr| expr.id == expr_id)?;
    let AstExprKind::Path(parts) = &expr.kind else {
        return None;
    };
    row_field_path_from_parts(parts, row_scope)
}

fn list_retain_predicate(field: &FieldDef, row_scope: Option<&str>) -> ListPredicate {
    if let Some(selector) = selected_filter_selector(field)
        && let Some(row_field) = row_field_path_in_exprs(&field.ast_exprs, row_scope)
    {
        return ListPredicate::SelectedFilterVisibility {
            selector,
            row_field,
        };
    }
    if let Some(predicate) = list_retain_predicate_from_ast_arg(field, row_scope) {
        return predicate;
    }
    if let Some(path) = bool_not_path_in_exprs(&field.ast_exprs) {
        return ListPredicate::RowFieldBoolNot { path };
    }
    if let Some(path) = row_field_path_in_exprs(&field.ast_exprs, row_scope) {
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
        .find(|arg| arg.name.as_deref() == Some("if"))
        .or_else(|| args.get(1))?;
    list_predicate_from_expr(field, predicate_arg.value, row_scope)
}

fn count_or_retain_source_list(field: &FieldDef, program: &ParsedProgram) -> Option<String> {
    if let Some(list_name) = field.path.strip_prefix("store.")
        && program
            .list_memories
            .iter()
            .any(|list| list.name == list_name)
    {
        return Some(list_name.to_owned());
    }
    let count_or_retain = field.ast_exprs.iter().find(|expr| {
        matches!(
            &expr.kind,
            AstExprKind::Pipe { op, .. }
                if op == "List/count" || op == "List/retain" || op == "List/every"
        )
    })?;
    let source = source_list_from_expr(field, count_or_retain.id)?;
    let list_name = source.strip_prefix("store.").unwrap_or(&source);
    program
        .list_memories
        .iter()
        .any(|list| list.name == list_name)
        .then(|| list_name.to_owned())
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

fn row_field_path_in_exprs(exprs: &[AstExpr], row_scope: Option<&str>) -> Option<String> {
    let row_scope = row_scope?;
    exprs.iter().find_map(|expr| match &expr.kind {
        AstExprKind::Path(parts) => row_field_path_from_parts(parts, row_scope),
        _ => None,
    })
}

fn selected_filter_selector(field: &FieldDef) -> Option<String> {
    field.ast_exprs.iter().find_map(|expr| {
        let AstExprKind::When { input } = expr.kind else {
            return None;
        };
        let selector = ast_argument_value(field, input)?;
        (!selector.is_empty()).then(|| canonical_local_path(&selector, &field.parent_path))
    })
}

fn row_field_path_from_parts(parts: &[String], row_scope: &str) -> Option<String> {
    parts.windows(2).find_map(|window| {
        (window[0] == row_scope && is_name(&window[1]))
            .then(|| format!("{row_scope}.{}", window[1]))
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
        .filter(|part| part.as_str() != "PASSED" && part.as_str() != "events")
        .map(String::as_str)
        .collect::<Vec<_>>();
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
            | "List/range"
            | "List/chunk"
            | "List/find"
            | "List/find_value"
            | "List/filter_text_contains"
            | "List/filter_field_equal"
            | "List/filter_field_not_equal"
            | "List/move_field_first"
            | "List/move_field_last"
            | "List/join_field"
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
    if path.contains('.') || parent_path.is_empty() {
        path.to_owned()
    } else {
        format!("{parent_path}.{path}")
    }
}

fn update_expression_for_source(
    program: &ParsedProgram,
    target: &str,
    field: &FieldDef,
    fields: &[FieldDef],
    source: &str,
) -> UpdateExpression {
    let variants = source_ref_variants(source);
    let branch = field.source_branch(source).unwrap_or_default();
    update_expression_for_routed_branch(program, target, field, fields, source, &variants, branch)
}

fn update_expression_for_derived_dependency_source(
    program: &ParsedProgram,
    target: &str,
    field: &FieldDef,
    fields: &[FieldDef],
    dependency: &FieldDef,
    source: &str,
) -> Option<UpdateExpression> {
    let branch = field
        .source_trigger_branch(&dependency.path)
        .or_else(|| field.source_trigger_branch(&dependency.local_name))?;
    let variants = source_ref_variants(source);
    Some(update_expression_for_routed_branch(
        program,
        target,
        field,
        fields,
        &dependency.path,
        &variants,
        branch,
    ))
}

fn update_expression_for_routed_branch(
    program: &ParsedProgram,
    target: &str,
    field: &FieldDef,
    fields: &[FieldDef],
    branch_source: &str,
    variants: &[String],
    branch: RoutedBranch,
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
    if let Some(expression) =
        then_list_find_value_update_expression(field, target, fields, branch_source, &branch)
    {
        return expression;
    }
    if let Some(expression) = branch.then_prefix_payload_concat_expression(&variants) {
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
        let AstExprKind::Call { function, args } = &output.kind else {
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
        let output = output.or_else(|| following_when_expr_id(field, expr.line))?;
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
    let AstExprKind::When { input } = expr.kind else {
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
        return Some(UpdateExpression::MatchNumberInfixConst {
            left: scalar_update_operand_for_source(field, target, fields, *left, source)?,
            op: op.clone(),
            right: scalar_update_operand_for_source(field, target, fields, *right, source)?,
            arms,
        });
    }
    let raw_input = ast_argument_value(field, input)?;
    let input = canonical_scalar_update_path_for_source(field, target, &raw_input, fields, source);
    Some(UpdateExpression::MatchValueConst { input, arms })
}

fn scalar_update_operand_for_source(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    expr_id: usize,
    source: &str,
) -> Option<String> {
    let value = ast_argument_value(field, expr_id)?;
    if value.parse::<i64>().is_ok() {
        return Some(value);
    }
    if let Some((_, value_tail)) = value.split_once('.')
        && let Some((target_parent, _)) = target.rsplit_once('.')
    {
        let sibling = format!("{target_parent}.{value_tail}");
        if fields.iter().any(|candidate| candidate.path == sibling) {
            return Some(sibling);
        }
    }
    Some(canonical_scalar_update_path_for_source(
        field, target, &value, fields, source,
    ))
}

fn guarded_match_value_arms_after_when_expr(
    program: &ParsedProgram,
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    when_expr_id: usize,
    source: &str,
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
        .filter_map(|expr| {
            guarded_match_value_arm_expr(program, field, target, fields, expr, source)
        })
        .collect()
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
    (!pattern.is_empty()).then(|| UpdateValueMatchArm { pattern, output })
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
    if let AstExprKind::Call { function, args } = &expr.kind {
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

fn then_list_find_value_update_expression(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    source: &str,
    branch: &RoutedBranch,
) -> Option<UpdateExpression> {
    branch.ast_exprs.iter().find_map(|expr| {
        let AstExprKind::Then { output, .. } = expr.kind else {
            return None;
        };
        let output = output.or_else(|| {
            field
                .ast_exprs
                .iter()
                .filter(|candidate| candidate.line > expr.line)
                .find_map(|candidate| match candidate.kind {
                    AstExprKind::Call { .. } => Some(candidate.id),
                    _ => None,
                })
        })?;
        list_find_value_update_expression_from_expr(field, target, fields, source, output)
    })
}

fn list_find_value_update_expression_from_expr(
    field: &FieldDef,
    target: &str,
    fields: &[FieldDef],
    source: &str,
    expr_id: usize,
) -> Option<UpdateExpression> {
    let expr = field_expr(field, expr_id)?;
    let AstExprKind::Call { function, args } = &expr.kind else {
        return None;
    };
    if function != "List/find_value" {
        return None;
    }
    let list = args
        .iter()
        .filter(|arg| arg.name.is_none())
        .next()
        .and_then(|arg| ast_argument_value(field, arg.value))
        .map(|path| {
            canonical_scalar_update_path_for_source(field, target, &path, fields, source)
        })?;
    let field_name = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some("field"))
        .and_then(|arg| ast_argument_value(field, arg.value))?;
    let expected = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some("value"))
        .and_then(|arg| {
            update_value_expression_from_expr(field, target, fields, arg.value, Some(source))
        })?;
    let target_field = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some("target"))
        .and_then(|arg| ast_argument_value(field, arg.value))?;
    let fallback = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some("fallback"))
        .and_then(|arg| {
            update_value_expression_from_expr(field, target, fields, arg.value, Some(source))
        })
        .map(Box::new);
    Some(UpdateExpression::ListFindValue {
        list,
        field: field_name,
        expected: Box::new(expected),
        target: target_field,
        fallback,
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
        let AstExprKind::When { input } = expr.kind else {
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

fn function_definition_for_call<'a>(
    program: &'a ParsedProgram,
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
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial } => {
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
        AstExprKind::ListLiteral { .. }
        | AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
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
        .find(|arg| arg.name.as_deref() == Some(input_name))
        .and_then(|arg| ast_argument_value_in_exprs(call_exprs, arg.value));
    let positional_arg = formals
        .iter()
        .position(|formal| formal == input_name)
        .and_then(|index| {
            args.iter()
                .filter(|arg| arg.name.is_none())
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
            let AstExprKind::When { input } = expr.kind else {
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
                matches!(expr.kind, AstExprKind::When { .. })
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
}

fn then_input_matches_source(field: &FieldDef, expr_id: usize, source: &str) -> bool {
    expr_matches_source(field, expr_id, source)
}

fn expr_matches_source(field: &FieldDef, expr_id: usize, source: &str) -> bool {
    let Some(input_path) = ast_argument_value(field, expr_id) else {
        return false;
    };
    source_ref_variants(source).iter().any(|variant| {
        let canonical = canonical_local_path(&input_path, &field.parent_path);
        input_path == *variant
            || input_path
                .strip_prefix(variant)
                .is_some_and(|suffix| suffix.starts_with('.'))
            || canonical == *variant
            || canonical
                .strip_prefix(variant)
                .is_some_and(|suffix| suffix.starts_with('.'))
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
                .find(|arg| arg.name.as_deref() == Some("when"))
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
    let output = output.or_else(|| following_when_expr_id(field, expr.line))?;
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

fn following_when_expr_id(field: &FieldDef, line: usize) -> Option<usize> {
    field
        .ast_exprs
        .iter()
        .find(|candidate| {
            candidate.line > line && matches!(candidate.kind, AstExprKind::When { .. })
        })
        .map(|expr| expr.id)
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
        AstExprKind::When { input } => {
            if let Some(expression) = match_number_infix_const_update_expression_from_input(
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

fn match_number_infix_const_update_expression_from_input(
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
    let left = scalar_number_operand(field, *left, target)?;
    let right = scalar_number_operand(field, *right, target)?;
    let arms = match_value_arms_for_when(field, target, fields, when_expr_id, source);
    (!arms.is_empty()).then_some(UpdateExpression::MatchNumberInfixConst {
        left,
        op: op.clone(),
        right,
        arms,
    })
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
    (!pattern.is_empty()).then(|| UpdateValueMatchArm { pattern, output })
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
        if path == target || fields.iter().any(|candidate| candidate.path == path) {
            return Some(UpdateValueExpression::ReadPath { path });
        }
    }
    if let Some(value) = ast_simple_update_value_in_exprs(&field.ast_exprs, expr_id) {
        return Some(UpdateValueExpression::Const { value });
    }
    if let AstExprKind::When { input } = expr.kind {
        if let Some(expression) = update_value_match_number_infix_from_input(
            field, target, fields, input, expr.id, source,
        ) {
            return Some(expression);
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

fn update_value_match_number_infix_from_input(
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
    (!arms.is_empty()).then_some(UpdateValueExpression::MatchNumberInfixConst {
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
    (!pattern.is_empty()).then(|| UpdateMatchArm { pattern, output })
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
    if root == needle {
        return true;
    }
    let Some(expr) = exprs.iter().find(|expr| expr.id == root) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::Call { args, .. } => args
            .iter()
            .any(|arg| expr_contains_expr_id_in_exprs(exprs, arg.value, needle)),
        AstExprKind::Pipe { input, args, .. } => {
            expr_contains_expr_id_in_exprs(exprs, *input, needle)
                || args
                    .iter()
                    .any(|arg| expr_contains_expr_id_in_exprs(exprs, arg.value, needle))
        }
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial } => {
            expr_contains_expr_id_in_exprs(exprs, *initial, needle)
        }
        AstExprKind::Then {
            input,
            output: Some(output),
        } => {
            expr_contains_expr_id_in_exprs(exprs, *input, needle)
                || expr_contains_expr_id_in_exprs(exprs, *output, needle)
        }
        AstExprKind::Then {
            input,
            output: None,
        } => expr_contains_expr_id_in_exprs(exprs, *input, needle),
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => expr_contains_expr_id_in_exprs(exprs, *output, needle),
        AstExprKind::Infix { left, right, .. } => {
            expr_contains_expr_id_in_exprs(exprs, *left, needle)
                || expr_contains_expr_id_in_exprs(exprs, *right, needle)
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => fields
            .iter()
            .any(|record_field| expr_contains_expr_id_in_exprs(exprs, record_field.value, needle)),
        AstExprKind::ListLiteral { .. }
        | AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
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
    if root == needle {
        return true;
    }
    let Some(expr) = field_expr(field, root) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::Call { args, .. } => args
            .iter()
            .any(|arg| expr_contains_expr_id(field, arg.value, needle)),
        AstExprKind::Pipe { input, args, .. } => {
            expr_contains_expr_id(field, *input, needle)
                || args
                    .iter()
                    .any(|arg| expr_contains_expr_id(field, arg.value, needle))
        }
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial } => {
            expr_contains_expr_id(field, *initial, needle)
        }
        AstExprKind::Then {
            input,
            output: Some(output),
        } => {
            expr_contains_expr_id(field, *input, needle)
                || expr_contains_expr_id(field, *output, needle)
        }
        AstExprKind::Then {
            input,
            output: None,
        } => expr_contains_expr_id(field, *input, needle),
        AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => expr_contains_expr_id(field, *output, needle),
        AstExprKind::Infix { left, right, .. } => {
            expr_contains_expr_id(field, *left, needle)
                || expr_contains_expr_id(field, *right, needle)
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => fields
            .iter()
            .any(|record_field| expr_contains_expr_id(field, record_field.value, needle)),
        AstExprKind::ListLiteral { .. }
        | AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
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
    } else if !value.contains('.') {
        let child_path = format!("{}.{}", field.path, value);
        if fields.iter().any(|candidate| candidate.path == child_path) {
            child_path
        } else {
            canonical_local_path(value, &field.parent_path)
        }
    } else {
        canonical_local_path(value, &field.parent_path)
    }
}

fn canonical_scalar_update_path_for_source(
    field: &FieldDef,
    target: &str,
    value: &str,
    fields: &[FieldDef],
    source: &str,
) -> String {
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
    fields_by_path: BTreeMap<&'a str, usize>,
    dependencies_by_field: Vec<Vec<usize>>,
    cache: BTreeMap<String, Vec<String>>,
}

impl<'a> CandidateSourceIndex<'a> {
    fn new(
        fields: &'a [FieldDef],
        direct_sources: &'a BTreeMap<String, Vec<String>>,
    ) -> CandidateSourceIndex<'a> {
        let empty_exclusions = BTreeSet::new();
        let (_, dependencies_by_field) = field_symbol_dependency_graph(fields, &empty_exclusions);
        let fields_by_path = fields
            .iter()
            .enumerate()
            .map(|(index, field)| (field.path.as_str(), index))
            .collect();
        CandidateSourceIndex {
            fields,
            direct_sources,
            fields_by_path,
            dependencies_by_field,
            cache: BTreeMap::new(),
        }
    }

    fn candidate_sources(&mut self, target: &str) -> Vec<String> {
        if let Some(cached) = self.cache.get(target) {
            return cached.clone();
        }
        let Some(&field_index) = self.fields_by_path.get(target) else {
            self.cache.insert(target.to_owned(), Vec::new());
            return Vec::new();
        };
        let mut visiting = Vec::new();
        self.candidate_sources_for_index(field_index, &mut visiting)
    }

    fn candidate_sources_for_index(
        &mut self,
        field_index: usize,
        visiting: &mut Vec<usize>,
    ) -> Vec<String> {
        let path = self.fields[field_index].path.clone();
        if visiting.contains(&field_index) {
            return Vec::new();
        }
        if let Some(cached) = self.cache.get(&path) {
            return cached.clone();
        }
        visiting.push(field_index);
        let field = &self.fields[field_index];
        let mut candidates = direct_sources_for_field(self.direct_sources, field)
            .cloned()
            .collect::<Vec<_>>();
        for dependency_index in self.dependencies_by_field[field_index].clone() {
            for source in self.candidate_sources_for_index(dependency_index, visiting) {
                push_unique(&mut candidates, source);
            }
        }
        visiting.pop();
        self.cache.insert(path, candidates.clone());
        candidates
    }
}

#[cfg(test)]
fn candidate_sources_cached(
    _program: &ParsedProgram,
    fields: &[FieldDef],
    direct_sources: &BTreeMap<String, Vec<String>>,
    target: &str,
) -> Vec<String> {
    CandidateSourceIndex::new(fields, direct_sources).candidate_sources(target)
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
            let AstExprKind::Call { function, args } = &output.kind else {
                return None;
            };
            if function != "Number/project_time" {
                return None;
            }
            let arg = |name: &str| {
                args.iter()
                    .find(|arg| arg.name.as_deref() == Some(name))
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
    let AstExprKind::Pipe { op, input, args } = &expr.kind else {
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
        .find(|arg| arg.name.as_deref() == Some("with"))
        .or_else(|| args.iter().find(|arg| arg.name.is_none()))
        .and_then(|arg| ast_argument_value_in_exprs(exprs, arg.value))?;
    let path = canonical_scalar_update_path_with_fields(field, target, &raw_path, fields);
    let separator = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some("separator"))
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
        if source_payload_field_from_path(&payload_path, source_variants).is_none() {
            return None;
        }
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
    let AstExprKind::Pipe { op, input, args } = &expr.kind else {
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
        .find(|arg| arg.name.as_deref() == Some("with"))
        .or_else(|| args.iter().find(|arg| arg.name.is_none()))
        .and_then(|arg| ast_argument_value_in_exprs(exprs, arg.value))?;
    if source_payload_field_from_path(&payload_path, source_variants).is_none() {
        return None;
    }
    let separator = args
        .iter()
        .find(|arg| arg.name.as_deref() == Some("separator"))
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
                        ast_initial_value(output, &[], None)
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
            _ => false,
        })
    }

    fn first_referenced_payload_field(&self, source_variant: &str) -> Option<String> {
        self.referenced_payload_fields(source_variant)
            .into_iter()
            .next()
            .map(|field| match field {
                SourcePayloadField::Address => "address".to_owned(),
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

fn direct_source_refs_by_path(
    fields: &[FieldDef],
    program: &ParsedProgram,
) -> BTreeMap<String, Vec<String>> {
    fields
        .iter()
        .map(|field| (field.path.clone(), direct_source_refs(field, program)))
        .collect()
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

fn field_function_body_index<'a>(
    statements: &'a [AstStatement],
) -> BTreeMap<&'a str, &'a [AstStatement]> {
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
            AstStatementKind::Block
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
        AstStatementKind::Field { .. }
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
        AstExprKind::Call { function, args } => {
            calls.push(function.clone());
            for arg in args {
                collect_field_called_functions(arg.value, expressions, calls);
            }
        }
        AstExprKind::Pipe { input, op, args } => {
            collect_field_called_functions(*input, expressions, calls);
            calls.push(op.clone());
            for arg in args {
                collect_field_called_functions(arg.value, expressions, calls);
            }
        }
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial } => {
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
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
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
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial } => {
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
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::ListLiteral { .. }
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
    let top_level_data_scope = scope
        .first()
        .is_some_and(|root| !matches!(root.as_str(), "store" | "document" | "scene"));
    local_name != "sources"
        && !scope.iter().any(|name| name == "sources")
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
mod tests {
    use super::*;

    #[test]
    fn stripe_view_binding_uses_neutral_kind_metadata() {
        assert_eq!(canonical_view_node_kind("Element/stripe"), "Stripe");
    }

    #[test]
    fn scoped_source_lookup_prefers_source_intent_identity_field() {
        assert_eq!(
            select_source_address_lookup_field(
                "file_tree_row.scope_row_elements.select_scope",
                vec!["file".to_owned(), "scope_key".to_owned()]
            )
            .as_deref(),
            Some("scope_key")
        );
        assert_eq!(
            select_source_address_lookup_field(
                "file_tree_row.file_row_elements.select_file",
                vec!["file".to_owned(), "scope_key".to_owned()]
            )
            .as_deref(),
            Some("file")
        );
    }

    #[test]
    fn view_row_source_alias_resolves_to_unique_canonical_source_path() {
        let sources = [
            ("file_tree_row.file_row_elements.select_file", SourceId(0)),
            ("file_tree_row.scope_row_elements.select_scope", SourceId(1)),
        ];
        assert_eq!(
            canonical_view_source_path(&sources, "row.file_row_elements.select_file")
                .map(|(path, source_id)| (path, source_id.as_usize())),
            Some(("file_tree_row.file_row_elements.select_file", 0))
        );

        let ambiguous = [
            ("left.file_row_elements.select_file", SourceId(0)),
            ("right.file_row_elements.select_file", SourceId(1)),
        ];
        assert!(
            canonical_view_source_path(&ambiguous, "row.file_row_elements.select_file").is_none(),
            "view row aliases must not guess when suffixes are ambiguous"
        );
    }

    #[test]
    fn lower_profile_reports_representation_candidates_without_folding() {
        let source = r#"
store: [
    elements: [
        click: SOURCE
    ]
    active:
        TEXT { A } |> HOLD active {
            LATEST {
                elements.click.event.press |> THEN { TEXT { B } }
            }
        }
    rows:
        LIST {
            [id: TEXT { a }, label: TEXT { A }]
            [id: TEXT { b }, label: TEXT { B }]
        }
    selected_rows:
        rows
        |> List/filter_field_equal(field: "label", value: active)
        |> List/map(row, new: [id: row.id, label: row.label])
]

document: Document/new(root: Element/label(element: [], label: TEXT { Rows }))
"#;
        let parsed = boon_parser::parse_source("representation-candidates.bn", source).unwrap();
        let (_ir, profile) = lower_profiled(&parsed).unwrap();
        let analysis = &profile["representation_analysis"];

        assert_eq!(
            analysis["policy"],
            "diagnostic_only_no_folding_or_storage_rewrite"
        );
        assert!(
            profile["representation_analysis_ms"].as_f64().unwrap() >= 0.0,
            "lower profile should time representation diagnostics: {profile:?}"
        );
        assert!(
            analysis["expression_class_counts"]["static_composite"]
                .as_u64()
                .unwrap_or_default()
                > 0,
            "literal records/lists should be visible as static composites: {analysis:?}"
        );
        assert!(
            analysis["expression_class_counts"]["source_or_hold_dynamic"]
                .as_u64()
                .unwrap_or_default()
                > 0,
            "SOURCE/HOLD paths must be classified as dynamic blockers: {analysis:?}"
        );
        assert!(
            analysis["expression_class_counts"]["row_dependent"]
                .as_u64()
                .unwrap_or_default()
                > 0,
            "row-scoped fields must block global constant hoisting: {analysis:?}"
        );
        assert_eq!(
            analysis["list_storage_mode_candidates"]["constant_array_literal"], 1,
            "literal LIST rows should be reported as a constant-array candidate"
        );
        assert!(
            analysis["list_storage_mode_candidates"]["selection_view"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "filtered list views should be reported as selection-view candidates"
        );
        assert!(
            analysis["list_storage_mode_candidates"]["incremental_projection"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "List/map list views should be reported as projection candidates"
        );
        let samples = analysis["root_derived_samples"]
            .as_array()
            .expect("root derived samples should be an array");
        let selected = samples
            .iter()
            .find(|sample| sample["path"] == "store.selected_rows")
            .expect("selected_rows should be sampled");
        assert_eq!(selected["kind"], "list_view");
        assert_eq!(selected["class"], "row_dependent");
        assert!(
            selected["line"].as_u64().unwrap() > 0,
            "samples should point back to source lines: {selected:?}"
        );
    }

    #[test]
    fn document_view_bindings_resolve_function_arguments_generically() {
        let source = r#"
store: [
    sources: [
        increment_button: [press: SOURCE]
    ]

    count:
        0 |> HOLD count {
            LATEST {
                sources.increment_button.press |> THEN { count + 1 }
            }
        }
]

document: Document/new(
    root: counter_button(press: store.sources.increment_button.press, label: TEXT { + })
)

FUNCTION counter_button(press, label) {
    Element/button(
        element: [event: [press: press]]
        style: []
        label: label
    )
}
"#;
        let parsed = boon_parser::parse_source("function-arg-view-bindings.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();

        assert!(ir.view_bindings.iter().any(|binding| {
            binding.node_kind == "Button"
                && binding.attr == "press"
                && binding.kind == ViewBindingKind::Source
                && binding.path == "store.sources.increment_button.press"
        }));
        assert!(
            !ir.view_bindings
                .iter()
                .any(|binding| binding.kind == ViewBindingKind::Data && binding.path == "label")
        );
    }

    #[test]
    fn document_view_bindings_include_style_layout_expression_reads() {
        let source = r#"
store: [
    source: SOURCE

    offset:
        10 |> HOLD offset { LATEST {} }
]

document: Document/new(
    root: Element/container(
        element: []
        style: [width: PASSED.store.offset + 2, height: 20, paint: False]
        child: Element/text(element: [], style: [paint: False], text: TEXT { x })
    )
)
"#;
        let parsed = boon_parser::parse_source("style-width-view-binding.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();

        assert!(
            ir.view_bindings.iter().any(|binding| {
                binding.node_kind == "container"
                    && binding.attr == "width"
                    && binding.kind == ViewBindingKind::Data
                    && binding.path == "store.offset"
            }),
            "style width expression should make the root data path render-observed: {:?}",
            ir.view_bindings
        );
    }

    #[test]
    fn scene_view_bindings_include_element_target_expression_reads() {
        let source = r#"
SOURCE
HOLD
LATEST

store: [
    target_label:
        TEXT { 0 s } |> HOLD target_label { LATEST {} }

    target_offset:
        10 |> HOLD target_offset { LATEST {} }
]

scene: main_scene()

FUNCTION main_scene() {
    Scene/new(
        root: Scene/Element/stripe(
        element: [target: PASSED.store.target_label]
        direction: Row
        style: [width: PASSED.store.target_offset + 2, height: Fill, paint: False]
        items: LIST {
            Scene/Element/block(
                element: []
                style: [width: 2, height: Fill, paint: False]
                child: Scene/Element/text(element: [], style: [paint: False], text: TEXT { x })
            )
        }
    )
    )
}
"#;
        let parsed =
            boon_parser::parse_source("scene-element-target-view-binding.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();

        assert!(
            ir.view_bindings.iter().any(|binding| {
                binding.attr == "target"
                    && binding.kind == ViewBindingKind::Target
                    && binding.path == "store.target_label"
            }),
            "element target expression should make the root target path render-observed: {:?}",
            ir.view_bindings
        );
        assert!(
            ir.view_bindings.iter().any(|binding| {
                binding.attr == "width"
                    && binding.kind == ViewBindingKind::Data
                    && binding.path == "store.target_offset"
            }),
            "scene style width expression should still make the root data path render-observed: {:?}",
            ir.view_bindings
        );
    }

    #[test]
    fn scene_view_bindings_resolve_forwarded_style_arguments() {
        let source = r#"
SOURCE
HOLD
LATEST

store: [
    panel_width:
        430 |> HOLD panel_width { LATEST {} }
]

scene: main_scene()

FUNCTION main_scene() {
    Scene/new(root: left_panel())
}

FUNCTION left_panel() {
    left_panel_base(panel_width: PASSED.store.panel_width, panel_height: Fill)
}

FUNCTION left_panel_base(panel_width, panel_height) {
    Scene/Element/stripe(
        element: []
        direction: Column
        gap: 6
        style: [
            width: panel_width
            height: panel_height
            paint: False
        ]
        items: LIST {}
    )
}
"#;
        let parsed =
            boon_parser::parse_source("forwarded-style-arg-view-binding.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();

        assert!(
            ir.view_bindings.iter().any(|binding| {
                binding.attr == "width"
                    && binding.kind == ViewBindingKind::Data
                    && binding.path == "store.panel_width"
            }),
            "forwarded style width should make the root data path render-observed: {:?}",
            ir.view_bindings
        );
    }

    #[test]
    fn semantic_index_skeleton_reuses_parser_ir_and_typecheck_facts() {
        let source = r#"
store: [
    sources: [
        increment_button: [press: SOURCE]
    ]

    count:
        0 |> HOLD count {
            LATEST {
                sources.increment_button.press |> THEN { count + 1 }
            }
        }
]

document: Document/new(
    root: counter_button(press: store.sources.increment_button.press, label: TEXT { + })
)

FUNCTION counter_button(press, label) {
    Element/button(
        element: [event: [press: press]]
        style: []
        label: label
    )
}
"#;
        let parsed = boon_parser::parse_source("semantic-index-counter.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let index = &ir.semantic_index;

        assert_eq!(index.version, 1);
        assert_eq!(index.computed_from, "parser_ast_ir_typecheck_tables");
        assert_eq!(
            index.parser_policy_phase,
            "syntax_parse_then_semantic_index_policy_checks"
        );
        assert!(index.reuse.parser_reused_by_ir);
        assert!(index.reuse.typecheck_reused_by_ir);
        assert!(index.reuse.runtime_reports_reuse_index);
        assert!(index.sources.iter().any(|source| source.path
            == "store.sources.increment_button.press"
            && source.payload_schema_known));
        assert!(
            index
                .functions
                .iter()
                .any(|function| function.name == "counter_button" && function.type_known)
        );
        assert!(
            index
                .fields
                .iter()
                .any(|field| field.path == "store.count" && field.kind == "state_cell")
        );
        assert!(index.view_bindings.iter().any(|binding| binding.path
            == "store.sources.increment_button.press"
            && binding.source_id.is_some()
            && binding.render_contract_known));
        assert!(
            index
                .symbols
                .iter()
                .any(|symbol| symbol.category == "field_name" && symbol.text == "count")
        );
        assert!(
            index
                .symbols
                .iter()
                .any(|symbol| symbol.category == "source_label"
                    && symbol.text == "store.sources.increment_button.press")
        );
        assert!(index
            .symbols
            .iter()
            .any(|symbol| symbol.category == "operator_name" && symbol.text == "Element/button"));
        assert!(index.report()["symbol_count"].as_u64().unwrap() > 0);
        assert_eq!(index.readiness.source_payload_schemas.fallback_count, 0);
        assert_eq!(index.readiness.render_contracts.fallback_count, 0);
        assert!(index.report()["present"].as_bool().unwrap());
    }

    #[test]
    fn semantic_symbol_table_reuses_duplicate_category_text_pairs() {
        let mut table = SemanticSymbolTable::default();

        let first = table.intern("field_name", "count");
        let duplicate = table.intern("field_name", "count");
        let same_text_other_category = table.intern("source_label", "count");

        assert_eq!(first, duplicate);
        assert_ne!(first, same_text_other_category);

        let entries = table.into_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, first);
        assert_eq!(entries[0].category, "field_name");
        assert_eq!(entries[0].text, "count");
        assert_eq!(entries[1].id, same_text_other_category);
        assert_eq!(entries[1].category, "source_label");
        assert_eq!(entries[1].text, "count");
    }

    #[test]
    fn source_wrapper_binds_nested_element_events_generically() {
        let source = r#"
HOLD
LATEST
items: LIST {}
rows: items |> List/map(item, new: item)

store: [
    button: SOURCE
]

document: Document/new(root: wrapped_button())

FUNCTION wrapped_button() {
    button() |> SOURCE { PASSED.store.button }
}

FUNCTION button() {
    Scene/Element/button(
        element: [event: [press: SOURCE]]
        style: []
        label: TEXT { Go }
        )
}
"#;
        let parsed = boon_parser::parse_source("scene-source-wrapper.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();

        assert!(ir.view_bindings.iter().any(|binding| {
            binding.node_kind == "Button"
                && binding.attr == "press"
                && binding.kind == ViewBindingKind::Source
                && binding.path == "store.button"
                && binding.source_id.is_some()
        }));
    }

    #[test]
    fn source_continuation_wrapper_binds_nested_element_events_generically() {
        let parsed = boon_parser::parse_project(
            "examples/app/RUN.bn",
            [
                (
                    "examples/app/View/View.bn".to_owned(),
                    r#"
FUNCTION wrapped_button(source) {
    button()
        |> SOURCE { source }
}

FUNCTION button() {
    Element/button(
        element: [event: [press: SOURCE]]
        style: []
        label: TEXT { Go }
    )
}
"#
                    .to_owned(),
                ),
                (
                    "examples/app/RUN.bn".to_owned(),
                    r#"
HOLD
LATEST
items: LIST {}
rows: items |> List/map(item, new: item)

store: [
    button: SOURCE
]

scene: Scene/new(root: View/wrapped_button(source: PASSED.store.button))
"#
                    .to_owned(),
                ),
            ],
        )
        .unwrap();
        assert!(
            parsed.source_ports.iter().any(|source| source.path
                == "external_file_tree_row.file_row_elements.select_file"
                && source.scoped),
            "NovyWave parser must expose scoped external file row source ports, sources={:?}, row_scopes={:?}, lists={:?}",
            parsed
                .source_ports
                .iter()
                .filter(|source| source.path.contains("external"))
                .cloned()
                .collect::<Vec<_>>(),
            parsed.row_scope_functions,
            parsed.list_memories
        );
        let fields = typed_field_defs(&parsed);
        let direct_sources = direct_source_refs_by_path(&fields, &parsed);
        let external_selector_sources = direct_sources
            .get("store.external_file_tree_file_selected_scope")
            .cloned()
            .unwrap_or_default();
        assert!(
            external_selector_sources
                .iter()
                .any(|source| source == "external_file_tree_row.file_row_elements.select_file"),
            "external selector field must directly reference external file source, sources={external_selector_sources:?}"
        );
        let active_scope_sources =
            candidate_sources_cached(&parsed, &fields, &direct_sources, "store.active_scope");
        assert!(
            active_scope_sources
                .iter()
                .any(|source| source == "external_file_tree_row.file_row_elements.select_file"),
            "active_scope candidates must include external file source, candidates={active_scope_sources:?}"
        );
        let ir = lower(&parsed).unwrap();

        assert!(ir.view_bindings.iter().any(|binding| {
            binding.node_kind == "Button"
                && binding.attr == "press"
                && binding.kind == ViewBindingKind::Source
                && binding.path == "store.button"
                && binding.source_id.is_some()
        }));
    }

    #[test]
    fn inline_empty_render_slot_lists_inside_row_constructors_get_unique_names() {
        let source = r#"
SOURCE
HOLD
LATEST
items:
    LIST { [label: TEXT { A }] }
rows:
    items |> List/map(item, new: row(item: item))

document:
    root:
        Element/stripe(items: rows)

FUNCTION row(item) {
    Element/stripe(
        items: LIST {}
    )
}
"#;
        let parsed = boon_parser::parse_source("inline-row-render-slot-list.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let mut names = std::collections::BTreeSet::new();
        for list in &ir.lists {
            assert!(
                names.insert(list.name.as_str()),
                "list names must be unique after lowering inline row lists: {:?}",
                ir.lists
                    .iter()
                    .map(|list| list.name.as_str())
                    .collect::<Vec<_>>()
            );
        }
        assert!(
            ir.lists
                .iter()
                .any(|list| list.name != "items" && list.name.contains("items_list")),
            "anonymous row render-slot LIST should get a generated name, got {:?}",
            ir.lists
                .iter()
                .map(|list| list.name.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn source_named_argument_view_binding_does_not_recurse_into_itself() {
        let source = r#"
HOLD
LATEST

store: [
    button: SOURCE
]

document: Document/new(root: wrapped_button(source: store.button))

FUNCTION wrapped_button(source) {
    Element/button(
        element: [event: [press: source], hovered: source]
        style: []
        label: TEXT { Go }
    )
}
"#;
        let parsed = boon_parser::parse_source("self-named-source-arg.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();

        assert!(ir.view_bindings.iter().any(|binding| {
            binding.node_kind == "Button"
                && binding.attr == "press"
                && binding.kind == ViewBindingKind::Source
                && binding.path == "store.button"
                && binding.source_id.is_some()
        }));
    }

    #[test]
    fn novywave_project_lowers_source_wrapped_controls() {
        let parsed = boon_parser::parse_project(
            "examples/novywave/RUN.bn",
            [
                (
                    "examples/novywave/Bridge/NovyBridge.bn".to_owned(),
                    include_str!("../../../examples/novywave/Bridge/NovyBridge.bn").to_owned(),
                ),
                (
                    "examples/novywave/Generated/Assets.bn".to_owned(),
                    include_str!("../../../examples/novywave/Generated/Assets.bn").to_owned(),
                ),
                (
                    "examples/novywave/Generated/NovyReference.bn".to_owned(),
                    include_str!("../../../examples/novywave/Generated/NovyReference.bn")
                        .to_owned(),
                ),
                (
                    "examples/novywave/Model/NovyModel.bn".to_owned(),
                    include_str!("../../../examples/novywave/Model/NovyModel.bn").to_owned(),
                ),
                (
                    "examples/novywave/Theme/NovyTheme.bn".to_owned(),
                    include_str!("../../../examples/novywave/Theme/NovyTheme.bn").to_owned(),
                ),
                (
                    "examples/novywave/View/NovyView.bn".to_owned(),
                    include_str!("../../../examples/novywave/View/NovyView.bn").to_owned(),
                ),
                (
                    "examples/novywave/RUN.bn".to_owned(),
                    include_str!("../../../examples/novywave/RUN.bn").to_owned(),
                ),
            ],
        )
        .unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(
            !ir.derived_values
                .iter()
                .any(|value| value.path == "store.file_tree_rows"),
            "inline initialized file_tree_rows must not be emitted as a derived list view: {:#?}",
            ir.derived_values
                .iter()
                .filter(|value| matches!(value.kind, DerivedValueKind::ListView))
                .map(|value| value.path.as_str())
                .collect::<Vec<_>>()
        );
        for expected_view in [
            "store.external_catalog_file_tree_rows",
            "store.external_fallback_file_tree_rows",
            "store.external_file_tree_rows",
        ] {
            assert!(
                ir.derived_values.iter().any(|value| {
                    value.path == expected_view && value.kind == DerivedValueKind::ListView
                }),
                "{expected_view} must be emitted as a materialized derived list view: {:#?}",
                ir.derived_values
                    .iter()
                    .filter(|value| matches!(value.kind, DerivedValueKind::ListView))
                    .map(|value| value.path.as_str())
                    .collect::<Vec<_>>()
            );
        }
        assert!(
            ir.lists.iter().any(|list| {
                list.name == "external_catalog_file_tree_rows"
                    && list
                        .row_scope_id
                        .and_then(|scope_id| ir.row_scopes.get(scope_id.as_usize()))
                        .is_some_and(|scope| scope.row_scope == "external_file_tree_row")
            }),
            "external catalog list must own external_file_tree_row scope, lists={:?}, row_scopes={:?}",
            ir.lists,
            ir.row_scopes
        );

        for expected_path in [
            "store.elements.signal_search_input",
            "store.elements.keyboard_capture",
            "store.elements.format_cycle",
        ] {
            assert!(
                ir.view_bindings.iter().any(|binding| {
                    binding.kind == ViewBindingKind::Source
                        && binding.path == expected_path
                        && binding.source_id.is_some()
                }),
                "missing source view binding for {expected_path}"
            );
        }
        assert!(ir.state_cells.iter().any(|cell| {
            cell.path == "selected_signal.formatter"
                && cell.indexed
                && cell.initial_value
                    == InitialValue::RowInitialField {
                        path: "formatter".to_owned(),
                    }
        }));
        let external_file_tree_file = ir
            .state_cells
            .iter()
            .find(|cell| cell.path == "store.external_file_tree_file")
            .expect("NovyWave external loaded file should be a state cell");
        assert_eq!(
            external_file_tree_file.initial_value,
            InitialValue::Text {
                value: "none".to_owned()
            }
        );
        assert!(
            ir.state_cells
                .iter()
                .any(|cell| cell.path == "store.active_file" && !cell.indexed),
            "NovyWave active_file must remain a root state cell: {:#?}",
            ir.state_cells
                .iter()
                .filter(|cell| cell.path.contains("active_file"))
                .collect::<Vec<_>>()
        );
        assert!(
            !ir.derived_values.iter().any(|value| value.path == "store"),
            "container path `store` must not be emitted as a derived value"
        );
        assert!(
            ir.update_branches.iter().any(|branch| {
                branch.source == "external_file_tree_row.file_row_elements.select_file"
                    && branch.target == "store.active_scope"
                    && branch.expression
                        == UpdateExpression::Const {
                            value: "none".to_owned(),
                        }
            }),
            "external file row source must clear active_scope, branches={:?}",
            ir.update_branches
                .iter()
                .filter(|branch| branch.source.contains("file_row_elements.select_file"))
                .cloned()
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn function_returned_render_list_keeps_typed_materialization_metadata() {
        let source = r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
store:
    todos: LIST[4] {}

FUNCTION todo_row(todo) {
    Element/label(label: todo.title)
}

FUNCTION make_rows(todos) {
    todos
    |> List/map(todo, new: todo_row(todo: todo))
}

document:
    root:
        Element/stripe(
            items: make_rows(todos: store.todos)
        )
"#;
        let parsed = boon_parser::parse_source("function-render-list-ir.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let slot = ir
            .typecheck_report
            .render_slot_table
            .slots
            .iter()
            .find(|slot| slot.slot_name == "items")
            .expect("items slot should be typed");
        let binding = slot
            .optional_list_map_binding_id
            .and_then(|id| ir.typecheck_report.list_map_bindings.get(id))
            .expect("function-returned render list should expose materialization metadata");
        assert_eq!(binding.template_function.as_deref(), Some("todo_row"));
        assert!(matches!(
            parsed.expressions.get(binding.list_expr_id).map(|expr| &expr.kind),
            Some(AstExprKind::Path(parts)) if parts == &vec!["store".to_owned(), "todos".to_owned()]
        ));
    }

    #[test]
    fn todomvc_lowering_is_static_and_keyed() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let ir = lower(&parsed).unwrap();
        assert_eq!(ir.kind, ProgramKind::Generic);
        assert!(
            ir.nodes
                .iter()
                .filter(|node| node.expr_id.is_some())
                .count()
                > 10
        );
        assert_eq!(ir.lists[0].graph_clones_per_item, 0);
        assert_eq!(ir.lists[0].capacity, None);
        assert_eq!(
            ir.lists[0].initializer,
            ListInitializer::RecordLiteral {
                rows: vec![
                    ListInitialRecord {
                        fields: vec![
                            ListRowInitialField {
                                name: "title".to_owned(),
                                value: InitialValue::Text {
                                    value: "Read documentation".to_owned(),
                                },
                            },
                            ListRowInitialField {
                                name: "completed".to_owned(),
                                value: InitialValue::Bool { value: false },
                            },
                        ],
                    },
                    ListInitialRecord {
                        fields: vec![
                            ListRowInitialField {
                                name: "title".to_owned(),
                                value: InitialValue::Text {
                                    value: "Finish TodoMVC renderer".to_owned(),
                                },
                            },
                            ListRowInitialField {
                                name: "completed".to_owned(),
                                value: InitialValue::Bool { value: true },
                            },
                        ],
                    },
                    ListInitialRecord {
                        fields: vec![
                            ListRowInitialField {
                                name: "title".to_owned(),
                                value: InitialValue::Text {
                                    value: "Walk the dog".to_owned(),
                                },
                            },
                            ListRowInitialField {
                                name: "completed".to_owned(),
                                value: InitialValue::Bool { value: false },
                            },
                        ],
                    },
                    ListInitialRecord {
                        fields: vec![
                            ListRowInitialField {
                                name: "title".to_owned(),
                                value: InitialValue::Text {
                                    value: "Buy groceries".to_owned(),
                                },
                            },
                            ListRowInitialField {
                                name: "completed".to_owned(),
                                value: InitialValue::Bool { value: false },
                            },
                        ],
                    },
                ],
            }
        );
        assert!(
            ir.state_cells
                .iter()
                .any(|cell| cell.path == "todo.completed" && cell.indexed)
        );
        let todo_scope = ir
            .row_scopes
            .iter()
            .find(|scope| scope.list == "todos" && scope.row_scope == "todo")
            .expect("TodoMVC row scope must lower into typed IR");
        assert!(
            ir.lists
                .iter()
                .any(|list| list.name == "todos" && list.row_scope_id == Some(todo_scope.id))
        );
        assert!(ir.sources.iter().any(|source| {
            source.path == "todo.sources.todo_checkbox.click"
                && source.scoped
                && source.scope_id == Some(todo_scope.id)
        }));
        assert!(ir.sources.iter().any(|source| {
            source.path == "store.sources.new_todo_input.key_down"
                && source.payload_schema.fields == vec![SourcePayloadField::Key]
        }));
        assert!(ir.sources.iter().any(|source| {
            source.path == "store.sources.new_todo_input.change"
                && source.payload_schema.fields == vec![SourcePayloadField::Text]
        }));
        assert!(ir.sources.iter().any(|source| {
            source.path == "todo.sources.todo_checkbox.click"
                && source.payload_schema.fields.is_empty()
        }));
        assert!(ir.view_bindings.iter().any(|binding| {
            binding.node_kind == "Input"
                && binding.attr == "change"
                && binding.kind == ViewBindingKind::Source
                && binding.path == "store.sources.new_todo_input.change"
                && binding.source_id.is_some()
        }));
        assert!(ir.view_bindings.iter().any(|binding| {
            binding.node_kind == "Checkbox"
                && binding.attr == "checked"
                && binding.kind == ViewBindingKind::Data
                && binding.path == "todo.completed"
                && binding.scope_id == Some(todo_scope.id)
        }));
        assert!(ir.view_bindings.iter().any(|binding| {
            binding.node_kind == "Button"
                && binding.attr == "target"
                && binding.kind == ViewBindingKind::Target
                && binding.path == "todo.title"
                && binding.scope_id == Some(todo_scope.id)
        }));
        assert!(ir.state_cells.iter().any(|cell| {
            cell.path == "todo.completed" && cell.indexed && cell.scope_id == Some(todo_scope.id)
        }));
        assert!(ir.state_cells.iter().any(|cell| {
            cell.path == "todo.title"
                && cell.initial_value
                    == InitialValue::RowInitialField {
                        path: "title".to_owned(),
                    }
        }));
        assert!(ir.state_cells.iter().any(|cell| {
            cell.path == "store.new_todo_text"
                && cell.initial_value
                    == InitialValue::Text {
                        value: String::new(),
                    }
        }));
        assert!(ir.derived_values.iter().any(|value| {
            value.path == "store.title_to_add"
                && value.kind == DerivedValueKind::SourceEventTransform
                && value
                    .sources
                    .contains(&"store.sources.new_todo_input.key_down".to_owned())
        }));
        assert!(ir.possible_causes.iter().any(|entry| {
            entry.target == "todo.completed"
                && entry
                    .sources
                    .contains(&"todo.sources.todo_checkbox.click".to_owned())
        }));
        assert!(
            ir.nodes
                .iter()
                .any(|node| matches!(node.kind, IrNodeKind::ListRemove))
        );
        assert!(ir.list_operations.iter().any(|operation| {
            operation.list == "todos"
                && operation.kind
                    == ListOperationKind::Append {
                        trigger: "store.title_to_add".to_owned(),
                        fields: vec![ListAppendField {
                            name: "title".to_owned(),
                            value: ListAppendFieldValue::Source {
                                path: "store.title_to_add".to_owned(),
                            },
                        }],
                    }
        }));
        assert!(ir.list_operations.iter().any(|operation| {
            operation.list == "todos"
                && operation.kind
                    == ListOperationKind::Remove {
                        source: "todo.sources.remove_todo_button.press".to_owned(),
                        predicate: ListPredicate::AlwaysTrue,
                    }
        }));
        assert!(ir.list_operations.iter().any(|operation| {
            operation.list == "todos"
                && operation.kind
                    == ListOperationKind::Remove {
                        source: "store.sources.clear_completed_button.press".to_owned(),
                        predicate: ListPredicate::RowFieldBool {
                            path: "todo.completed".to_owned(),
                        },
                    }
        }));
        assert!(ir.list_operations.iter().any(|operation| {
            operation.list == "todos"
                && operation.kind
                    == ListOperationKind::Retain {
                        target: "store.visible_todos".to_owned(),
                        predicate: ListPredicate::SelectedFilterVisibility {
                            selector: "store.selected_filter".to_owned(),
                            row_field: "todo.completed".to_owned(),
                        },
                    }
        }));
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "store.selected_filter"
                && branch.source == "store.sources.filter_active.press"
                && branch.expression
                    == UpdateExpression::Const {
                        value: "Active".to_owned(),
                    }
        }));
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "todo.completed"
                && branch.source == "todo.sources.todo_checkbox.click"
                && matches!(branch.expression, UpdateExpression::BoolNot { .. })
                && branch.indexed
        }));
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "todo.editing"
                && branch.source == "todo.sources.editing_todo_title_element.key_down"
                && branch.expression
                    == UpdateExpression::Const {
                        value: "False".to_owned(),
                    }
                && branch.indexed
        }));
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "todo.title"
                && branch.source == "todo.sources.editing_todo_title_element.key_down"
                && branch.expression
                    == UpdateExpression::TextTrimOrPrevious {
                        path: "edit_text".to_owned(),
                        previous: "title".to_owned(),
                    }
                && branch.indexed
        }));
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "todo.title"
                && branch.source == "todo.sources.editing_todo_title_element.blur"
                && branch.expression
                    == UpdateExpression::TextTrimOrPrevious {
                        path: "edit_text".to_owned(),
                        previous: "title".to_owned(),
                    }
                && branch.indexed
        }));
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "todo.edit_text"
                && branch.source == "todo.sources.editing_todo_title_element.change"
                && branch.expression
                    == UpdateExpression::TextTrimOrPrevious {
                        path: "text".to_owned(),
                        previous: "edit_text".to_owned(),
                    }
                && branch.indexed
        }));
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "todo.edit_text"
                && branch.source == "todo.sources.editing_todo_title_element.key_down"
                && branch.expression
                    == UpdateExpression::PreviousValue {
                        path: "title".to_owned(),
                    }
                && branch.indexed
        }));
        assert!(ir.nodes.iter().any(|node| {
            matches!(node.kind, IrNodeKind::RenderLowering) && node.name == "render_todos_template"
        }));
        verify_hidden_identity(&ir).unwrap();
    }

    #[test]
    fn list_record_literal_signed_numbers_lower_to_numeric_initializers() {
        let source = r#"
SOURCE
HOLD
LATEST
items: LIST {
    [kind: Inset, elevation: -4]
}
document: []
"#;
        let parsed = boon_parser::parse_source("signed-list-literal.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let items = ir
            .lists
            .iter()
            .find(|list| list.name == "items")
            .expect("items list should lower");
        let ListInitializer::RecordLiteral { rows } = &items.initializer else {
            panic!(
                "items should lower as record literal, got {:?}",
                items.initializer
            );
        };
        assert!(rows.iter().any(|row| {
            row.fields.iter().any(|field| {
                field.name == "elevation" && field.value == InitialValue::Number { value: -4 }
            })
        }));
    }

    #[test]
    fn nested_then_when_lowers_to_match_const_before_path_readback() {
        let source = r#"
store: [
    elements: [
        select_data: SOURCE
    ]
    active_signal:
        TEXT { none } |> HOLD active_signal {
            LATEST {
                elements.select_data.event.press |> THEN {
                    active_signal |> WHEN {
                        data_bus => TEXT { none }
                        __ => TEXT { data_bus }
                    }
                }
            }
        }
]
"#;
        let parsed = boon_parser::parse_source("nested-then-when.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "store.active_signal"
                && branch.source == "store.elements.select_data"
                && branch.expression
                    == UpdateExpression::MatchConst {
                        input: "store.active_signal".to_owned(),
                        arms: vec![
                            UpdateMatchArm {
                                pattern: "data_bus".to_owned(),
                                output: "none".to_owned(),
                            },
                            UpdateMatchArm {
                                pattern: "__".to_owned(),
                                output: "data_bus".to_owned(),
                            },
                        ],
                    }
        }));
    }

    #[test]
    fn hold_text_initializer_survives_later_list_filter_reference() {
        let source = r#"
store: [
    elements: [
        load: SOURCE
    ]
    external_file_tree_file:
        TEXT { none } |> HOLD external_file_tree_file {
            LATEST {
                elements.load.text
            }
        }
    file_tree_rows:
        LIST {
            [file: TEXT { simple.vcd }]
        }
    external_catalog_file_tree_rows:
        file_tree_rows
        |> List/filter_field_equal(field: "file", value: external_file_tree_file)
]
"#;
        let parsed = boon_parser::parse_source("hold-filter-reference.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let cell = ir
            .state_cells
            .iter()
            .find(|cell| cell.path == "store.external_file_tree_file")
            .expect("external loaded file should be a state cell");
        assert_eq!(
            cell.initial_value,
            InitialValue::Text {
                value: "none".to_owned()
            }
        );
    }

    #[test]
    fn then_call_to_pure_match_function_lowers_to_match_const() {
        let source = r#"
store: [
    elements: [
        format_cycle: SOURCE
    ]
    value_format:
        Hexadecimal |> HOLD value_format {
            LATEST {
                elements.format_cycle.event.press |> THEN { next_format(format: value_format) }
            }
        }
]

FUNCTION next_format(format) {
    format |> WHEN {
        Hexadecimal => Binary
        Binary => GroupedBinary
        __ => Hexadecimal
    }
        }
"#;
        let parsed = boon_parser::parse_source("then-call-pure-match-function.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "store.value_format"
                && branch.source == "store.elements.format_cycle"
                && branch.expression
                    == UpdateExpression::MatchConst {
                        input: "store.value_format".to_owned(),
                        arms: vec![
                            UpdateMatchArm {
                                pattern: "Hexadecimal".to_owned(),
                                output: "Binary".to_owned(),
                            },
                            UpdateMatchArm {
                                pattern: "Binary".to_owned(),
                                output: "GroupedBinary".to_owned(),
                            },
                            UpdateMatchArm {
                                pattern: "__".to_owned(),
                                output: "Hexadecimal".to_owned(),
                            },
                        ],
                    }
        }));
        verify_hidden_identity(&ir).unwrap();
    }

    #[test]
    fn guarded_then_call_to_pure_match_function_preserves_row_guard() {
        let source = r#"
store: [
    elements: [
        format_cycle: SOURCE
    ]
    active_signal:
        TEXT { reset_n } |> HOLD active_signal { LATEST {} }
    selected_signal_defaults:
        LIST {
            [id: TEXT { clk }, formatter: Hexadecimal]
            [id: TEXT { reset_n }, formatter: Hexadecimal]
        }
    selected_signal_rows:
        selected_signal_defaults
        |> List/map(selected_signal, new: [
            id: selected_signal.id
            formatter:
                selected_signal.formatter |> HOLD formatter {
                    LATEST {
                        elements.format_cycle.event.press |> THEN {
                            selected_signal.id == store.active_signal |> WHEN {
                                True => next_format(format: formatter)
                                False => formatter
                            }
                        }
                    }
                }
        ])
]

FUNCTION next_format(format) {
    format |> WHEN {
        Hexadecimal => Binary
        Binary => GroupedBinary
        __ => Hexadecimal
    }
}
"#;
        let parsed =
            boon_parser::parse_source("guarded-then-call-pure-match-function.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let branch = ir
            .update_branches
            .iter()
            .find(|branch| {
                branch.target.ends_with(".formatter")
                    && branch.source == "store.elements.format_cycle"
            })
            .unwrap_or_else(|| {
                panic!(
                    "row formatter branch should lower, branches={:#?}",
                    ir.update_branches
                )
            });
        let UpdateExpression::MatchNumberInfixConst {
            left,
            op,
            right,
            arms,
        } = &branch.expression
        else {
            panic!(
                "guarded formatter update should preserve row guard, got {:#?}",
                branch.expression
            );
        };
        assert!(
            left.ends_with(".id"),
            "left operand should read row id: {left}"
        );
        assert_eq!(op, "==");
        assert_eq!(right, "store.active_signal");
        assert!(arms.iter().any(|arm| {
            arm.pattern == "True"
                && matches!(
                    &arm.output,
                    UpdateValueExpression::MatchConst { input, arms }
                        if input.ends_with(".formatter")
                            && arms.iter().any(|nested| {
                                nested.pattern == "Hexadecimal"
                                    && matches!(
                                        &nested.output,
                                        UpdateValueExpression::Const { value } if value == "Binary"
                                    )
                            })
                )
        }));
        assert!(arms.iter().any(|arm| {
            arm.pattern == "False"
                && matches!(
                    &arm.output,
                    UpdateValueExpression::ReadPath { path } if path.ends_with(".formatter")
                )
        }));
        verify_static_schedule(&ir).unwrap();
    }

    #[test]
    fn numeric_infix_when_with_arithmetic_arm_lowers_to_structured_update() {
        let source = r#"
store: [
    elements: [
        zoom_in: SOURCE
    ]
    zoom_step:
        0 |> HOLD zoom_step {
            LATEST {
                elements.zoom_in.event.press |> THEN {
                    zoom_step >= 3 |> WHEN {
                        True => 3
                        False => zoom_step + 1
                    }
                }
            }
        }
]
"#;
        let parsed =
            boon_parser::parse_source("numeric-infix-when-arithmetic-arm.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "store.zoom_step"
                && branch.source == "store.elements.zoom_in"
                && branch.expression
                    == UpdateExpression::MatchNumberInfixConst {
                        left: "store.zoom_step".to_owned(),
                        op: ">=".to_owned(),
                        right: "3".to_owned(),
                        arms: vec![
                            UpdateValueMatchArm {
                                pattern: "True".to_owned(),
                                output: UpdateValueExpression::Const {
                                    value: "3".to_owned(),
                                },
                            },
                            UpdateValueMatchArm {
                                pattern: "False".to_owned(),
                                output: UpdateValueExpression::NumberInfix {
                                    left: "store.zoom_step".to_owned(),
                                    op: "+".to_owned(),
                                    right: "1".to_owned(),
                                },
                            },
                        ],
                    }
        }));
        verify_hidden_identity(&ir).unwrap();
        verify_static_schedule(&ir).unwrap();
    }

    #[test]
    fn source_payload_match_can_emit_nested_numeric_infix_match_update() {
        let source = r#"
store: [
    elements: [
        keyboard_capture: SOURCE
    ]
    zoom_step:
        0 |> HOLD zoom_step {
            LATEST {
                elements.keyboard_capture.key |> WHEN {
                    W => zoom_step >= 3 |> WHEN {
                        True => 3
                        False => zoom_step + 1
                    }
                    R => 0
                    __ => SKIP
                }
            }
        }
]
"#;
        let parsed =
            boon_parser::parse_source("source-payload-nested-match-value.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let branch = ir
            .update_branches
            .iter()
            .find(|branch| {
                branch.target == "store.zoom_step"
                    && branch.source == "store.elements.keyboard_capture"
            })
            .expect("keyboard_capture should route to zoom_step");
        let expected = UpdateExpression::MatchValueConst {
            input: "elements.keyboard_capture.key".to_owned(),
            arms: vec![
                UpdateValueMatchArm {
                    pattern: "W".to_owned(),
                    output: UpdateValueExpression::MatchNumberInfixConst {
                        left: "store.zoom_step".to_owned(),
                        op: ">=".to_owned(),
                        right: "3".to_owned(),
                        arms: vec![
                            UpdateValueMatchArm {
                                pattern: "True".to_owned(),
                                output: UpdateValueExpression::Const {
                                    value: "3".to_owned(),
                                },
                            },
                            UpdateValueMatchArm {
                                pattern: "False".to_owned(),
                                output: UpdateValueExpression::NumberInfix {
                                    left: "store.zoom_step".to_owned(),
                                    op: "+".to_owned(),
                                    right: "1".to_owned(),
                                },
                            },
                        ],
                    },
                },
                UpdateValueMatchArm {
                    pattern: "R".to_owned(),
                    output: UpdateValueExpression::Const {
                        value: "0".to_owned(),
                    },
                },
                UpdateValueMatchArm {
                    pattern: "__".to_owned(),
                    output: UpdateValueExpression::Const {
                        value: "SKIP".to_owned(),
                    },
                },
            ],
        };
        assert_eq!(
            branch.expression, expected,
            "keyboard branch expression should preserve nested structured outputs; actual={:#?}",
            branch.expression
        );
        verify_static_schedule(&ir).unwrap();
    }

    #[test]
    fn source_payload_match_rejects_unsupported_nested_numeric_infix_operator() {
        let source = r#"
store: [
    elements: [
        keyboard_capture: SOURCE
    ]
    zoom_step:
        0 |> HOLD zoom_step {
            LATEST {
                elements.keyboard_capture.key |> WHEN {
                    W => zoom_step * 2
                    __ => SKIP
                }
            }
        }
]
"#;
        let parsed =
            boon_parser::parse_source("source-payload-unsupported-nested-op.bn", source).unwrap();
        let error =
            lower(&parsed).expect_err("unsupported nested numeric operator should fail lowering");
        assert!(
            error.contains("unsupported numeric operator `*`"),
            "unexpected static verification error: {error}"
        );
    }

    #[test]
    fn state_initial_values_are_lowered_from_ast_exprs() {
        let source = r#"
-- True False TEXT { comment } todo.title must not become an initializer
store: [
    sources: [
        click: SOURCE
    ]
    empty_text:
        Text/empty |> HOLD empty_text { LATEST {} }
    flag:
        False |> HOLD flag { LATEST {} }
    filter:
        All |> HOLD filter { LATEST {} }
    todos:
        LIST { [title: TEXT { Initial }, completed: False] }
        |> List/map(todo, new: new_todo(todo: todo))
]
FUNCTION new_todo(todo) {
    [
        title:
            todo.title |> HOLD title { LATEST {} }
        completed:
            False |> HOLD completed { LATEST {} }
    ]
}
"#;
        let parsed = boon_parser::parse_source("ast-initial-values.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(ir.state_cells.iter().any(|cell| {
            cell.path == "store.empty_text"
                && cell.initial_value
                    == InitialValue::Text {
                        value: String::new(),
                    }
        }));
        assert!(ir.state_cells.iter().any(|cell| {
            cell.path == "store.flag" && cell.initial_value == InitialValue::Bool { value: false }
        }));
        assert!(ir.state_cells.iter().any(|cell| {
            cell.path == "store.filter"
                && cell.initial_value
                    == InitialValue::Enum {
                        value: "All".to_owned(),
                    }
        }));
        assert!(ir.state_cells.iter().any(|cell| {
            cell.path == "todo.title"
                && cell.initial_value
                    == InitialValue::RowInitialField {
                        path: "title".to_owned(),
                    }
        }));
    }

    #[test]
    fn physical_todomvc_mode_toggle_lowers_to_match_const_expression() {
        let parsed = boon_parser::parse_project(
            "examples/todo_mvc_physical/RUN.bn",
            [
                (
                    "examples/todo_mvc_physical/Theme/Classic.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/Theme/Classic.bn").to_owned(),
                ),
                (
                    "examples/todo_mvc_physical/Theme/Professional.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/Theme/Professional.bn")
                        .to_owned(),
                ),
                (
                    "examples/todo_mvc_physical/Theme/Glassmorphism.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/Theme/Glassmorphism.bn")
                        .to_owned(),
                ),
                (
                    "examples/todo_mvc_physical/Theme/Neobrutalism.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/Theme/Neobrutalism.bn")
                        .to_owned(),
                ),
                (
                    "examples/todo_mvc_physical/Theme/Neumorphism.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/Theme/Neumorphism.bn")
                        .to_owned(),
                ),
                (
                    "examples/todo_mvc_physical/Theme/Theme.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/Theme/Theme.bn").to_owned(),
                ),
                (
                    "examples/todo_mvc_physical/Generated/Assets.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/Generated/Assets.bn")
                        .to_owned(),
                ),
                (
                    "examples/todo_mvc_physical/RUN.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/RUN.bn").to_owned(),
                ),
            ],
        )
        .unwrap();
        let fields = typed_field_defs(&parsed);
        let field = fields
            .iter()
            .find(|field| field.path == "theme_options.mode")
            .expect("theme_options.mode field");
        let source = "store.elements.theme_switcher.mode_toggle";
        let routed_branch = field
            .source_branch(source)
            .expect("mode toggle routed branch");
        let expected = UpdateExpression::MatchConst {
            input: "theme_options.mode".to_owned(),
            arms: vec![
                UpdateMatchArm {
                    pattern: "Light".to_owned(),
                    output: "Dark".to_owned(),
                },
                UpdateMatchArm {
                    pattern: "Dark".to_owned(),
                    output: "Light".to_owned(),
                },
            ],
        };
        assert_eq!(
            match_const_update_expression(
                field,
                "theme_options.mode",
                &fields,
                source,
                &routed_branch
            ),
            Some(expected.clone())
        );
        let row_scopes = row_scopes(&parsed);
        let fields = typed_field_defs(&parsed);
        let direct_sources = direct_source_refs_by_path(&fields, &parsed);
        let state_cells = parsed
            .state_cells
            .iter()
            .enumerate()
            .map(|(id, cell)| StateCell {
                id: StateId(id),
                path: cell.path.clone(),
                scope_id: scope_id_for_path(&row_scopes, &cell.path),
                hold_name: cell.hold_name.clone(),
                initial_value: InitialValue::Unknown {
                    summary: "test probe".to_owned(),
                },
                indexed: cell.indexed,
                source_line: cell.line,
            })
            .collect::<Vec<_>>();
        let mut candidate_sources = CandidateSourceIndex::new(&fields, &direct_sources);
        let unknown_branches = update_branches(
            &parsed,
            &state_cells,
            &fields,
            &direct_sources,
            &mut candidate_sources,
        )
        .into_iter()
        .filter(|branch| matches!(branch.expression, UpdateExpression::Unknown { .. }))
        .collect::<Vec<_>>();
        assert!(unknown_branches.is_empty(), "{unknown_branches:#?}");
        let ir = lower(&parsed).unwrap();
        assert!(
            parsed
                .source_ports
                .iter()
                .any(|source| source.path == "store.elements.remove_completed_button"),
            "{:#?}",
            parsed
                .source_ports
                .iter()
                .map(|source| &source.path)
                .collect::<Vec<_>>()
        );
        let todos_field = fields
            .iter()
            .find(|field| field.path == "store.todos")
            .expect("store.todos field");
        assert!(
            todos_field
                .source_branch("store.elements.remove_completed_button")
                .is_some(),
            "{:#?}",
            todos_field
                .ast_exprs
                .iter()
                .filter_map(|expr| match &expr.kind {
                    AstExprKind::Path(parts) => Some(parts.join(".")),
                    _ => None,
                })
                .collect::<Vec<_>>()
        );
        let remove_completed_branch = todos_field
            .source_branch("store.elements.remove_completed_button")
            .unwrap();
        assert_eq!(
            retain_source_predicate(
                todos_field,
                "store.elements.remove_completed_button",
                Some("item"),
            ),
            Some(ListPredicate::RowFieldBoolNot {
                path: "item.completed".to_owned(),
            }),
            "{:#?}",
            remove_completed_branch
                .ast_exprs()
                .iter()
                .map(|expr| format!("{:?}", expr.kind))
                .collect::<Vec<_>>()
        );
        assert!(
            ir.row_scopes
                .iter()
                .any(|scope| { scope.list == "todos" && scope.row_scope == "todo" })
        );
        assert!(ir.sources.iter().any(|source| {
            source.path == "todo.todo_elements.todo_checkbox"
                && source.scoped
                && source.scope_id.is_some()
        }));
        assert!(ir.view_bindings.iter().any(|binding| {
            binding.node_kind == "Input"
                && binding.attr == "change"
                && binding.kind == ViewBindingKind::Source
                && binding.path == "store.elements.new_todo_title_text_input"
                && binding.source_id.is_some()
        }));
        assert!(ir.view_bindings.iter().any(|binding| {
            binding.node_kind == "Input"
                && binding.attr == "submit"
                && binding.kind == ViewBindingKind::Source
                && binding.path == "store.elements.new_todo_title_text_input"
                && binding.source_id.is_some()
        }));
        assert!(ir.state_cells.iter().any(|cell| {
            cell.path == "todo.title"
                && cell.initial_value
                    == InitialValue::RowInitialField {
                        path: "title".to_owned(),
                    }
        }));
        assert!(ir.list_operations.iter().any(|operation| {
            operation.list == "todos"
                && operation.kind
                    == ListOperationKind::Append {
                        trigger: "store.title_to_save".to_owned(),
                        fields: vec![ListAppendField {
                            name: "title".to_owned(),
                            value: ListAppendFieldValue::Source {
                                path: "store.title_to_save".to_owned(),
                            },
                        }],
                    }
        }));
        assert!(
            ir.list_operations.iter().any(|operation| {
                operation.list == "todos"
                    && operation.kind
                        == ListOperationKind::Remove {
                            source: "store.elements.remove_completed_button".to_owned(),
                            predicate: ListPredicate::RowFieldBool {
                                path: "item.completed".to_owned(),
                            },
                        }
            }),
            "{:#?}",
            ir.list_operations
        );
        let branch = ir
            .update_branches
            .iter()
            .find(|branch| {
                branch.target == "theme_options.mode"
                    && branch.source == "store.elements.theme_switcher.mode_toggle"
            })
            .expect("physical TodoMVC mode toggle update branch");
        assert_eq!(branch.expression, expected);
        assert!(ir.state_cells.iter().any(|cell| {
            cell.path == "todo.edited_title.draft_title" && cell.indexed && cell.scope_id.is_some()
        }));
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "todo.edited_title"
                && branch.source == "todo.todo_elements.todo_title_element"
                && branch.expression
                    == UpdateExpression::ReadPath {
                        path: "todo.edited_title.draft_title".to_owned(),
                    }
                && branch.indexed
        }));
        assert!(!ir.update_branches.iter().any(|branch| {
            matches!(
                &branch.expression,
                UpdateExpression::ReadPath { path }
                    | UpdateExpression::PreviousValue { path }
                    | UpdateExpression::BoolNot { path }
                    if path == "todo.draft_title"
            )
        }));
    }

    #[test]
    fn derived_value_kind_uses_ast_operators_not_text_tokens() {
        let source = r#"
store: [
    sources: [
        click: SOURCE
    ]
    note:
        TEXT { List/count List/retain WHEN THEN }
    todos:
        LIST {}
        |> List/map(todo, new: new_todo(todo: todo))
]
FUNCTION new_todo(todo) {
    [
        title:
            Text/empty |> HOLD title { LATEST {} }
    ]
}
"#;
        let parsed = boon_parser::parse_source("ast-derived-kind.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(
            ir.derived_values.iter().any(|value| {
                value.path == "store.note" && value.kind == DerivedValueKind::Pure
            })
        );
    }

    #[test]
    fn pure_when_over_root_state_lowers_as_pure_derived_value() {
        let source = r#"
store: [
    sources: [
        open: [press: SOURCE]
    ]
    dialog:
        Closed |> HOLD dialog {
        LATEST {
            Closed
            sources.open.press |> THEN { Open }
        }
        }
    dialog_title:
        dialog == Open |> WHEN {
            True => TEXT { Load files }
            False => TEXT { No file dialog }
        }
]
"#;
        let parsed = boon_parser::parse_source("pure-when-root-state.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(
            ir.state_cells
                .iter()
                .any(|cell| cell.path == "store.dialog")
        );
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "store.dialog" && branch.source == "store.sources.open.press"
        }));
        let title = ir
            .derived_values
            .iter()
            .find(|value| value.path == "store.dialog_title")
            .expect("dialog title should be a pure derived value");
        assert_eq!(title.kind, DerivedValueKind::Pure);
        assert!(title.sources.is_empty());
    }

    #[test]
    fn direct_source_refs_use_ast_paths_not_text_literals() {
        let source = r#"
store: [
    sources: [
        real_button: [press: SOURCE]
        fake_button: [press: SOURCE]
    ]
    note:
        TEXT { sources.fake_button.press }
    changed:
        sources.real_button.press |> THEN { True }
    todos:
        LIST {}
        |> List/map(todo, new: new_todo(todo: todo))
]
FUNCTION new_todo(todo) {
    [
        title:
            Text/empty |> HOLD title { LATEST {} }
    ]
}
"#;
        let parsed = boon_parser::parse_source("ast-source-refs.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let note = ir
            .derived_values
            .iter()
            .find(|value| value.path == "store.note")
            .expect("note derived value");
        assert!(note.sources.is_empty());
        let changed = ir
            .derived_values
            .iter()
            .find(|value| value.path == "store.changed")
            .expect("changed derived value");
        assert_eq!(
            changed.sources,
            vec!["store.sources.real_button.press".to_owned()]
        );
    }

    #[test]
    fn lower_case_text_literals_in_then_outputs_are_update_constants() {
        let source = r#"
store: [
    sources: [
        select_clk: SOURCE
    ]
    active_signal:
        TEXT { reset_n } |> HOLD active_signal {
            LATEST {
                sources.select_clk.event.press |> THEN { TEXT { clk } }
            }
        }
]
"#;
        let parsed = boon_parser::parse_source("lowercase-text-then-const.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let branch = ir
            .update_branches
            .iter()
            .find(|branch| branch.target == "store.active_signal")
            .expect("active signal update branch");
        assert_eq!(
            branch.expression,
            UpdateExpression::Const {
                value: "clk".to_owned()
            }
        );
        assert!(ir.static_schedule_verified);
    }

    #[test]
    fn source_payload_concat_inside_latest_lowers_without_then_wrapper() {
        let source = r#"
store: [
    elements: [
        external_file_loaded_name: SOURCE
        show_empty: SOURCE
    ]
    external_file_tree_label:
        TEXT { - local file } |> HOLD external_file_tree_label {
            LATEST {
                TEXT { - } |> Text/concat(with: elements.external_file_loaded_name.text, separator: " ")
                elements.show_empty.event.press |> THEN { TEXT { - local file } }
            }
        }
]
"#;
        let parsed = boon_parser::parse_source("source-payload-concat-latest.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let loaded_branch = ir
            .update_branches
            .iter()
            .find(|branch| {
                branch.target == "store.external_file_tree_label"
                    && branch.source == "store.elements.external_file_loaded_name"
            })
            .expect("loaded file source should update the held label");
        assert_eq!(
            loaded_branch.expression,
            UpdateExpression::PrefixPayloadConcat {
                prefix: "-".to_owned(),
                payload_path: "elements.external_file_loaded_name.text".to_owned(),
                separator: " ".to_owned(),
            }
        );
        let reset_branch = ir
            .update_branches
            .iter()
            .find(|branch| {
                branch.target == "store.external_file_tree_label"
                    && branch.source == "store.elements.show_empty"
            })
            .expect("show empty source should reset the held label");
        assert_eq!(
            reset_branch.expression,
            UpdateExpression::Const {
                value: "- local file".to_owned()
            }
        );
        assert!(ir.static_schedule_verified);
    }

    #[test]
    fn derived_local_event_branches_lower_dependent_branch_expressions() {
        let source = r#"
store: [
    elements: [
        select_next: SOURCE
    ]
    selected_event:
        LATEST {
            elements.select_next.event.press |> THEN { TEXT { second } }
        }
    selected:
        TEXT { first } |> HOLD selected {
            LATEST {
                selected_event
            }
        }
    response:
        TEXT { response:first } |> HOLD response {
            LATEST {
                selected_event |> THEN {
                    TEXT { response } |> Text/concat(with: selected_event, separator: ":")
                }
            }
        }
]
"#;
        let parsed = boon_parser::parse_source("derived-local-event-branches.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let selected_branch = ir
            .update_branches
            .iter()
            .find(|branch| {
                branch.target == "store.selected" && branch.source == "store.elements.select_next"
            })
            .expect("selected should be driven by the source behind selected_event");
        assert_eq!(
            selected_branch.expression,
            UpdateExpression::ReadPath {
                path: "store.selected_event".to_owned()
            }
        );
        let response_branch = ir
            .update_branches
            .iter()
            .find(|branch| {
                branch.target == "store.response" && branch.source == "store.elements.select_next"
            })
            .expect("response should be driven by the source behind selected_event");
        assert_eq!(
            response_branch.expression,
            UpdateExpression::PrefixRootConcat {
                prefix: "response".to_owned(),
                path: "store.selected_event".to_owned(),
                separator: ":".to_owned()
            }
        );
        assert!(ir.static_schedule_verified);
    }

    #[test]
    fn derived_local_event_branches_lower_dependent_match_expressions() {
        let source = r#"
store: [
    rows:
        LIST {
            [file: TEXT { wave_27.fst }]
            [file: TEXT { simple.vcd }]
        }
        |> List/map(row, new: [
            file: row.file
            elements: [
                select: SOURCE
            ]
        ])
    selected_file:
        LATEST {
            rows
                |> List/map(row, new: LATEST {
                    row.elements.select.event.press |> THEN { row.file }
                })
                |> List/latest()
        }
    active_signal_key:
        TEXT { A_SIGNAL } |> HOLD active_signal_key {
            LATEST {
                selected_file |> THEN {
                    selected_file |> WHEN {
                        TEXT { wave_27.fst } => TEXT { TX_DATA }
                        TEXT { simple.vcd } => TEXT { A_SIGNAL }
                        __ => TEXT { NONE }
                    }
                }
            }
        }
]
"#;
        let parsed = boon_parser::parse_source("derived-local-match-branches.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let branch = ir
            .update_branches
            .iter()
            .find(|branch| {
                branch.target == "store.active_signal_key"
                    && branch.source == "store.rows.elements.select"
            })
            .expect("dependent match should be routed to the row source behind selected_file");
        assert_eq!(
            branch.expression,
            UpdateExpression::MatchConst {
                input: "store.selected_file".to_owned(),
                arms: vec![
                    UpdateMatchArm {
                        pattern: "wave_27.fst".to_owned(),
                        output: "TX_DATA".to_owned(),
                    },
                    UpdateMatchArm {
                        pattern: "simple.vcd".to_owned(),
                        output: "A_SIGNAL".to_owned(),
                    },
                    UpdateMatchArm {
                        pattern: "__".to_owned(),
                        output: "NONE".to_owned(),
                    },
                ],
            }
        );
        assert!(ir.static_schedule_verified);
    }

    #[test]
    fn transitive_derived_event_branches_reach_dependent_match_expressions() {
        let source = r#"
store: [
    rows:
        LIST {
            [file: TEXT { wave_27.fst }]
            [file: TEXT { simple.vcd }]
        }
        |> List/map(row, new: [
            file: row.file
            elements: [
                select: SOURCE
            ]
        ])
    selected_file_leaf:
        LATEST {
            rows
                |> List/map(row, new: LATEST {
                    row.elements.select.event.press |> THEN { row.file }
                })
                |> List/latest()
        }
    selected_file:
        LATEST {
            selected_file_leaf
        }
    active_signal_key:
        TEXT { A_SIGNAL } |> HOLD active_signal_key {
            LATEST {
                selected_file |> THEN {
                    selected_file |> WHEN {
                        TEXT { wave_27.fst } => TEXT { TX_DATA }
                        TEXT { simple.vcd } => TEXT { A_SIGNAL }
                        __ => TEXT { NONE }
                    }
                }
            }
        }
]
"#;
        let parsed =
            boon_parser::parse_source("transitive-derived-event-branches.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let branch = ir
            .update_branches
            .iter()
            .find(|branch| {
                branch.target == "store.active_signal_key"
                    && branch.source == "store.rows.elements.select"
            })
            .expect("dependent match should be routed through transitive derived fields");
        assert_eq!(
            branch.expression,
            UpdateExpression::MatchConst {
                input: "store.selected_file".to_owned(),
                arms: vec![
                    UpdateMatchArm {
                        pattern: "wave_27.fst".to_owned(),
                        output: "TX_DATA".to_owned(),
                    },
                    UpdateMatchArm {
                        pattern: "simple.vcd".to_owned(),
                        output: "A_SIGNAL".to_owned(),
                    },
                    UpdateMatchArm {
                        pattern: "__".to_owned(),
                        output: "NONE".to_owned(),
                    },
                ],
            }
        );
        assert!(ir.static_schedule_verified);
    }

    #[test]
    fn derived_dependency_routes_do_not_borrow_payload_specific_branches() {
        let source = r#"
store: [
    elements: [
        hover: SOURCE
        scope: SOURCE
    ]
    range_start:
        0 |> HOLD range_start {
            LATEST {
                elements.scope.event.press |> THEN { 10 }
            }
        }
    range_end: 100
    zoom_center:
        range_start |> HOLD zoom_center {
            LATEST {
                elements.hover.pointer_x |> THEN { Number/project_time(pointer_x: elements.hover.pointer_x, pointer_width: elements.hover.pointer_width, viewport_start: range_start, viewport_end: range_end, fallback: zoom_center) }
            }
        }
]
"#;
        let parsed =
            boon_parser::parse_source("derived-dependency-non-trigger.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(
            !ir.update_branches.iter().any(|branch| {
                branch.target == "store.zoom_center"
                    && branch.source == "store.elements.scope"
                    && matches!(branch.expression, UpdateExpression::ProjectTime { .. })
            }),
            "scope changes range_start, but it must not reuse the hover pointer payload branch"
        );
        assert!(ir.static_schedule_verified);
    }

    #[test]
    fn list_latest_source_field_is_scalar_source_event_transform_not_list_view() {
        let source = r#"
store: [
    rows:
        LIST {
            [file: TEXT { wave_27.fst }]
            [file: TEXT { simple.vcd }]
        }
        |> List/map(row, new: [
            file: row.file
            elements: [
                select: SOURCE
            ]
        ])
    selected_file:
        rows
            |> List/map(row, new: LATEST {
                row.elements.select.event.press |> THEN { row.file }
            })
            |> List/latest()
    noop: TEXT { ready } |> HOLD noop { LATEST {} }
]
"#;
        let parsed = boon_parser::parse_source("list-latest-source-scalar.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let selected_file = ir
            .derived_values
            .iter()
            .find(|value| value.path == "store.selected_file")
            .expect("selected_file should be a derived scalar");
        assert_eq!(
            selected_file.kind,
            DerivedValueKind::SourceEventTransform,
            "List/latest selects one scalar event payload; it is not a list view"
        );
        assert_eq!(
            selected_file.sources,
            vec!["store.rows.elements.select".to_owned()]
        );
        assert!(ir.static_schedule_verified);
    }

    #[test]
    fn row_path_then_outputs_are_update_path_reads_not_previous_values() {
        let source = r#"
store: [
    rows:
        LIST {
            [key: TEXT { clk }]
        }
        |> List/map(row, new: new_row(row: row))
    active:
        TEXT { reset_n } |> HOLD active {
            LATEST {
                rows
                    |> List/map(row, new: LATEST {
                        row.sources.select.event.press |> THEN { row.key }
                    })
                    |> List/latest()
            }
        }
]
FUNCTION new_row(row) {
    [
        sources: [
            select: SOURCE
        ]
        key: row.key
    ]
}
"#;
        let parsed = boon_parser::parse_source("row-path-then-read.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let branch = ir
            .update_branches
            .iter()
            .find(|branch| branch.target == "store.active" && branch.source == "row.sources.select")
            .expect("row source active update branch");
        assert_eq!(
            branch.expression,
            UpdateExpression::ReadPath {
                path: "row.key".to_owned()
            }
        );
        assert!(ir.static_schedule_verified);
    }

    #[test]
    fn row_source_alias_inside_list_latest_lowers_to_root_update_branch() {
        let source = r#"
store: [
    signal_catalog:
        LIST {
            [key: TEXT { clk }, name: TEXT { A[3:0] }]
        }
        |> List/map(signal, new: new_signal(signal: signal))
    active_signal:
        TEXT { none } |> HOLD active_signal {
            LATEST {
                signal_catalog
                    |> List/map(signal_catalog_item, new: LATEST {
                        signal_catalog_item.signal_elements.select_signal.event.press
                            |> THEN { signal_catalog_item.key }
                    })
                    |> List/latest()
            }
        }
]

FUNCTION new_signal(signal) {
    [
        signal_elements: [
            select_signal: SOURCE
        ]
        key: signal.key
        name: signal.name
    ]
}
"#;
        let parsed = boon_parser::parse_source("row-source-alias-latest.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let branch = ir
            .update_branches
            .iter()
            .find(|branch| {
                branch.target == "store.active_signal"
                    && branch.source == "signal.signal_elements.select_signal"
            })
            .expect("row source alias should lower to active signal update branch");
        assert_eq!(
            branch.expression,
            UpdateExpression::ReadPath {
                path: "signal.key".to_owned()
            }
        );
        assert!(ir.static_schedule_verified);
    }

    #[test]
    fn row_source_alias_then_when_canonicalizes_match_input() {
        let source = r#"
store: [
    signal_catalog:
        LIST {
            [key: TEXT { clk }, name: TEXT { A[3:0] }]
        }
        |> List/map(signal, new: new_signal(signal: signal))
    active_label:
        TEXT { none } |> HOLD active_label {
            LATEST {
                signal_catalog
                    |> List/map(signal_catalog_item, new:
                        signal_catalog_item.signal_elements.select_signal.event.press
                        |> THEN {
                            signal_catalog_item.key |> WHEN {
                                clk => TEXT { simple_tb.s.A[3:0] }
                                __ => SKIP
                            }
                        }
                    )
                    |> List/latest()
            }
        }
]

FUNCTION new_signal(signal) {
    [
        signal_elements: [
            select_signal: SOURCE
        ]
        key: signal.key
        name: signal.name
    ]
}
"#;
        let parsed = boon_parser::parse_source("row-source-alias-then-when.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let branch = ir
            .update_branches
            .iter()
            .find(|branch| {
                branch.target == "store.active_label"
                    && branch.source == "signal.signal_elements.select_signal"
            })
            .expect("row source alias should lower active label update branch");
        assert_eq!(
            branch.expression,
            UpdateExpression::MatchConst {
                input: "signal.key".to_owned(),
                arms: vec![
                    UpdateMatchArm {
                        pattern: "clk".to_owned(),
                        output: "simple_tb.s.A[3:0]".to_owned(),
                    },
                    UpdateMatchArm {
                        pattern: "__".to_owned(),
                        output: "SKIP".to_owned(),
                    },
                ],
            }
        );
        assert!(ir.static_schedule_verified);
    }

    #[test]
    fn source_payload_match_with_skip_does_not_lower_to_sibling_const() {
        let source = r#"
store: [
    elements: [
        keyboard_capture: SOURCE
        load_default_file: SOURCE
    ]
    zoom_step:
        0 |> HOLD zoom_step {
            LATEST {
                elements.keyboard_capture.key |> WHEN {
                    W => zoom_step >= 3 |> WHEN {
                        True => 3
                        False => zoom_step + 1
                    }
                    S => zoom_step <= -2 |> WHEN {
                        True => -2
                        False => zoom_step - 1
                    }
                    R => 0
                    __ => SKIP
                }
                elements.load_default_file.event.press |> THEN { 0 }
            }
        }
]
"#;
        let parsed = boon_parser::parse_source("keyboard-skip-zoom-step.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let branch = ir
            .update_branches
            .iter()
            .find(|branch| {
                branch.target == "store.zoom_step"
                    && branch.source == "store.elements.keyboard_capture"
            })
            .expect("keyboard capture should route to zoom_step");
        assert!(
            matches!(branch.expression, UpdateExpression::MatchValueConst { .. }),
            "keyboard match should stay structured and skip unmatched keys, got {:#?}",
            branch.expression
        );
        assert!(ir.static_schedule_verified);
    }

    #[test]
    fn list_append_lowering_uses_ast_then_record() {
        let source = r#"
store: [
    sources: [
        input: [
            key_down: SOURCE
        ]
    ]
    misleading_text:
        TEXT { List/append item: title_to_add |> THEN { [title: wrong] } }
    pending_title:
        sources.input.key_down |> THEN { typed_title }
    todos:
        LIST {}
        |> List/append(item: pending_title |> THEN {
            [title: pending_title]
        })
        |> List/map(todo, new: new_todo(todo: todo))
]
FUNCTION new_todo(todo) {
    [
        title:
            todo.title |> HOLD title { LATEST {} }
    ]
}
"#;
        let parsed = boon_parser::parse_source("ast-list-append.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(ir.list_operations.iter().any(|operation| {
            operation.list == "todos"
                && operation.kind
                    == ListOperationKind::Append {
                        trigger: "store.pending_title".to_owned(),
                        fields: vec![ListAppendField {
                            name: "title".to_owned(),
                            value: ListAppendFieldValue::Source {
                                path: "store.pending_title".to_owned(),
                            },
                        }],
                    }
        }));
    }

    #[test]
    fn list_append_function_constructor_maps_piped_input_to_row_initial_field() {
        let source = r#"
store: [
    sources: [
        input: [key_down: SOURCE]
    ]
    title_to_save:
        sources.input.key_down |> THEN { typed_title }
    todos:
        LIST {}
        |> List/append(item: title_to_save |> new_todo())
]
FUNCTION new_todo(title) {
    [
        title:
            title |> HOLD title { LATEST {} }
        completed:
            False |> HOLD completed { LATEST {} }
    ]
}
"#;
        let parsed = boon_parser::parse_source("append-function-constructor.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(ir.list_operations.iter().any(|operation| {
            operation.list == "todos"
                && operation.kind
                    == ListOperationKind::Append {
                        trigger: "store.title_to_save".to_owned(),
                        fields: vec![ListAppendField {
                            name: "title".to_owned(),
                            value: ListAppendFieldValue::Source {
                                path: "store.title_to_save".to_owned(),
                            },
                        }],
                    }
        }));
    }

    #[test]
    fn list_append_record_fields_can_mix_source_and_constants() {
        let source = r#"
store: [
    sources: [
        input: [key_down: SOURCE]
    ]
    pending_title:
        sources.input.key_down |> THEN { typed_title }
    todos:
        LIST {}
        |> List/append(item: pending_title |> THEN {
            [title: pending_title, kind: TEXT { Signal }, visible: True]
        })
        |> List/map(todo, new: new_todo(todo: todo))
]
FUNCTION new_todo(todo) {
    [
        title:
            todo.title |> HOLD title { LATEST {} }
        kind:
            todo.kind |> HOLD kind { LATEST {} }
        visible:
            todo.visible |> HOLD visible { LATEST {} }
    ]
}
"#;
        let parsed = boon_parser::parse_source("append-mixed-fields.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(ir.list_operations.iter().any(|operation| {
            operation.list == "todos"
                && operation.kind
                    == ListOperationKind::Append {
                        trigger: "store.pending_title".to_owned(),
                        fields: vec![
                            ListAppendField {
                                name: "title".to_owned(),
                                value: ListAppendFieldValue::Source {
                                    path: "store.pending_title".to_owned(),
                                },
                            },
                            ListAppendField {
                                name: "kind".to_owned(),
                                value: ListAppendFieldValue::Const {
                                    value: "Signal".to_owned(),
                                },
                            },
                            ListAppendField {
                                name: "visible".to_owned(),
                                value: ListAppendFieldValue::Const {
                                    value: "True".to_owned(),
                                },
                            },
                        ],
                    }
        }));
    }

    #[test]
    fn list_remove_predicates_use_ast_then_outputs() {
        let source = r#"
store: [
    sources: [
        clear_done: [press: SOURCE]
    ]
    misleading_text:
        TEXT { todo.sources.delete_button.press |> THEN { True } sources.clear_done.press |> THEN { todo.completed } }
    todos:
        LIST { [title: TEXT { A }, completed: False] }
        |> List/remove(todo, when:
            LATEST {
                todo.sources.delete_button.press |> THEN { True }
                sources.clear_done.press |> THEN { todo.completed }
            }
        )
        |> List/map(todo, new: new_todo(todo: todo))
]
FUNCTION new_todo(todo) {
    sources: [
        delete_button: [press: SOURCE]
    ]
    [
        title:
            todo.title |> HOLD title { LATEST {} }
        completed:
            todo.completed |> HOLD completed { LATEST {} }
    ]
}
"#;
        let parsed = boon_parser::parse_source("ast-list-remove.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(ir.list_operations.iter().any(|operation| {
            operation.list == "todos"
                && operation.kind
                    == ListOperationKind::Remove {
                        source: "todo.sources.delete_button.press".to_owned(),
                        predicate: ListPredicate::AlwaysTrue,
                    }
        }));
        assert!(ir.list_operations.iter().any(|operation| {
            operation.list == "todos"
                && operation.kind
                    == ListOperationKind::Remove {
                        source: "store.sources.clear_done.press".to_owned(),
                        predicate: ListPredicate::RowFieldBool {
                            path: "todo.completed".to_owned(),
                        },
                    }
        }));
    }

    #[test]
    fn cells_lowering_has_dependency_index() {
        let parsed = boon_parser::parse_project(
            "examples/cells.bn",
            [
                (
                    "examples/cells/defaults.bn".to_owned(),
                    include_str!("../../../examples/cells/defaults.bn").to_owned(),
                ),
                (
                    "examples/cells/formula.bn".to_owned(),
                    include_str!("../../../examples/cells/formula.bn").to_owned(),
                ),
                (
                    "examples/cells/cell.bn".to_owned(),
                    include_str!("../../../examples/cells/cell.bn").to_owned(),
                ),
                (
                    "examples/cells/model.bn".to_owned(),
                    include_str!("../../../examples/cells/model.bn").to_owned(),
                ),
                (
                    "examples/cells/columns.bn".to_owned(),
                    include_str!("../../../examples/cells/columns.bn").to_owned(),
                ),
                (
                    "examples/cells/store.bn".to_owned(),
                    include_str!("../../../examples/cells/store.bn").to_owned(),
                ),
                (
                    "examples/cells/view.bn".to_owned(),
                    include_str!("../../../examples/cells/view.bn").to_owned(),
                ),
                (
                    "examples/cells.bn".to_owned(),
                    include_str!("../../../examples/cells.bn").to_owned(),
                ),
            ],
        )
        .unwrap();
        let ir = lower(&parsed).unwrap();
        assert_eq!(ir.kind, ProgramKind::Generic);
        let cells_list = ir
            .lists
            .iter()
            .find(|list| list.name == "cells")
            .expect("Cells source should lower a cells list");
        assert_eq!(
            cells_list.initializer,
            ListInitializer::Range { from: 0, to: 2599 }
        );
        let defaults_list = ir
            .lists
            .iter()
            .find(|list| list.name == "cells_default_values")
            .expect("Cells source should lower generic default values");
        let ListInitializer::RecordLiteral { rows } = &defaults_list.initializer else {
            panic!(
                "Cells defaults should be a generic record literal, got {:?}",
                defaults_list.initializer
            );
        };
        assert_eq!(rows.len(), 5);
        assert!(rows.iter().any(|row| {
            row.fields.iter().any(|field| {
                field.name == "address"
                    && matches!(&field.value, InitialValue::Text { value } if value == "B0")
            }) && row.fields.iter().any(|field| {
                field.name == "value"
                    && matches!(&field.value, InitialValue::Text { value } if value == "=add(A0,A1)")
            })
        }));
        assert!(ir.sources.iter().any(|source| {
            source.path == "cell.sources.editor.commit"
                && source.payload_schema.fields
                    == vec![SourcePayloadField::Address, SourcePayloadField::Text]
                && source.payload_schema.address_lookup_field.as_deref() == Some("address")
        }));
        assert!(ir.sources.iter().any(|source| {
            source.path == "cell.sources.editor.cancel"
                && source.payload_schema.fields == vec![SourcePayloadField::Address]
                && source.payload_schema.address_lookup_field.as_deref() == Some("address")
        }));
        assert!(ir.view_bindings.iter().any(|binding| {
            binding.node_kind == "Input"
                && binding.attr == "submit"
                && binding.kind == ViewBindingKind::Source
                && binding.path == "cell.sources.editor.commit"
                && binding.source_id.is_some()
        }));
        assert!(ir.lists.iter().any(|list| {
            list.name == "sheet_columns"
                && matches!(list.initializer, ListInitializer::RecordLiteral { .. })
        }));
        assert!(ir.list_projections.iter().any(|projection| {
            projection.target == "store.sheet_rows"
                && projection.list == "cells"
                && projection.kind
                    == ListProjectionKind::Chunk {
                        size: Some(26),
                        item_field: "cells".to_owned(),
                        label_field: "row_number".to_owned(),
                    }
        }));
        assert!(ir.list_projections.iter().any(|projection| {
            projection.target == "store.selected_input"
                && projection.list == "cells"
                && projection.kind
                    == ListProjectionKind::Find {
                        field: "address".to_owned(),
                        value: "store.selected_address".to_owned(),
                    }
        }));
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "store.selected_address"
                && branch.source == "cell.sources.editor.commit"
                && branch.expression
                    == UpdateExpression::SourcePayload {
                        path: "address".to_owned(),
                    }
        }));
        assert!(ir.view_bindings.iter().any(|binding| {
            binding.node_kind == "Input"
                && binding.attr == "key"
                && binding.kind == ViewBindingKind::Data
                && binding.path == "cell.address"
                && binding.scope_id.is_some()
        }));
        assert!(ir.nodes.iter().any(|node| {
            matches!(node.kind, IrNodeKind::RenderLowering) && node.name == "render_cells_template"
        }));
        assert!(
            ir.state_cells
                .iter()
                .any(|cell| cell.path == "cell.formula_text" && cell.indexed)
        );
        assert!(ir.state_cells.iter().any(|cell| {
            cell.path == "cell.formula_text"
                && cell.initial_value
                    == InitialValue::RowInitialField {
                        path: "default_formula".to_owned(),
                    }
        }));
        assert!(ir.derived_values.iter().any(|value| {
            value.path == "cell.value" && value.kind == DerivedValueKind::Pure && value.indexed
        }));
        assert!(ir.derived_values.iter().any(|value| {
            value.path == "cell.error" && value.kind == DerivedValueKind::Pure && value.indexed
        }));
        assert!(ir.dependencies.iter().any(|edge| {
            edge.from == "cell.sources.editor.commit" && edge.to == "cell.formula_text"
        }));
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "cell.editing_text"
                && branch.source == "cell.sources.editor.cancel"
                && branch.expression
                    == UpdateExpression::ReadPath {
                        path: "cell.formula_text".to_owned(),
                    }
        }));
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "cell.editing"
                && branch.source == "cell.sources.editor.change"
                && branch.expression
                    == UpdateExpression::Const {
                        value: "True".to_owned(),
                    }
        }));
        assert!(
            ir.nodes
                .iter()
                .filter(|node| node.expr_id.is_some())
                .all(|node| node.expr_id.unwrap().as_usize() < parsed.expressions.len())
        );
        verify_hidden_identity(&ir).unwrap();
    }

    #[test]
    fn widget_prefixed_symbols_do_not_lower_as_table_or_projection_shortcuts() {
        let source = r#"
items:
    LIST {}
    |> List/map(item, new: row(item: item))
legacy:
    Widget/table(columns: 1, rows: 1)
store: [
    sources: [
        noop: SOURCE
    ]
    selected:
        Widget/selected(items, address: wanted)
    rows:
        Widget/rows(items)
    wanted:
        TEXT { A0 } |> HOLD wanted {
            LATEST {}
        }
]
FUNCTION row(item) {
    [
        address: item.address
        value: item.value
    ]
}
"#;
        let parsed =
            boon_parser::parse_source("unknown-widget-prefix-shortcuts.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(
            !ir.lists.iter().any(|list| list.name == "legacy"),
            "Widget/table must not lower to a table initializer"
        );
        assert!(
            ir.list_projections.is_empty(),
            "Widget/selected and Widget/rows must not lower to generic projections"
        );
    }

    #[test]
    fn list_unknown_alias_does_not_lower_as_table_shortcut() {
        let source = r#"
items:
    LIST {}
    |> List/map(item, new: row(item: item))
legacy:
    List/spreadsheet_rows(columns: 1, rows: 1)
store:
    sources:
        noop: SOURCE
    noop:
        TEXT {} |> HOLD noop {
            LATEST {}
        }
FUNCTION row(item) {
    [
        address: item.address
        value: item.value
    ]
}
"#;
        let parsed = boon_parser::parse_source("unknown-list-table-alias.bn", source).unwrap();
        let error = lower(&parsed).unwrap_err();
        assert!(
            error.contains("unknown function or operator `List/spreadsheet_rows`"),
            "List/spreadsheet_rows must be rejected by typechecking before IR lowering, got {error}"
        );
    }

    #[test]
    fn hidden_identity_verifier_scans_boon_facing_ir_identifiers() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(
            ir.lists
                .iter()
                .any(|list| list.hidden_key_type.ends_with("Key")),
            "internal list key types should remain IR metadata"
        );
        verify_hidden_identity(&ir).unwrap();

        let mut with_bad_source = ir.clone();
        with_bad_source.sources[0].path = "todo.sources.source_id.press".to_owned();
        assert!(
            verify_hidden_identity(&with_bad_source)
                .unwrap_err()
                .contains("source_id")
        );

        let mut with_bad_state = ir.clone();
        with_bad_state.state_cells[0].path = "todo.hidden_generation".to_owned();
        assert!(
            verify_hidden_identity(&with_bad_state)
                .unwrap_err()
                .contains("hidden_generation")
        );

        let mut with_bad_branch = ir.clone();
        with_bad_branch.update_branches[0].expression = UpdateExpression::PreviousValue {
            path: "bind_epoch".to_owned(),
        };
        assert!(
            verify_hidden_identity(&with_bad_branch)
                .unwrap_err()
                .contains("bind_epoch")
        );

        let mut with_bad_row_key = ir.clone();
        with_bad_row_key.sources[0].path = "todo.sources.$boon.row_key.press".to_owned();
        let row_key_error = verify_hidden_identity(&with_bad_row_key).unwrap_err();
        assert!(
            row_key_error.contains("$boon") || row_key_error.contains("row_key"),
            "{row_key_error}"
        );

        let mut with_bad_target_key = ir.clone();
        with_bad_target_key.update_branches[0].target = "store.target_key".to_owned();
        assert!(
            verify_hidden_identity(&with_bad_target_key)
                .unwrap_err()
                .contains("target_key")
        );

        let mut with_bad_list_operation = ir.clone();
        with_bad_list_operation.list_operations[0].kind = ListOperationKind::Retain {
            target: "store.visible_todos".to_owned(),
            predicate: ListPredicate::RowFieldBool {
                path: "todo.hidden_key".to_owned(),
            },
        };
        assert!(
            verify_hidden_identity(&with_bad_list_operation)
                .unwrap_err()
                .contains("hidden_key")
        );

        let mut with_bad_chunk_projection = ir.clone();
        with_bad_chunk_projection
            .list_projections
            .push(ListProjection {
                target: "store.rows".to_owned(),
                list: "store.todos".to_owned(),
                kind: ListProjectionKind::Chunk {
                    size: Some(4),
                    item_field: "row_key".to_owned(),
                    label_field: "row_number".to_owned(),
                },
            });
        assert!(
            verify_hidden_identity(&with_bad_chunk_projection)
                .unwrap_err()
                .contains("row_key")
        );
    }

    #[test]
    fn static_schedule_verifier_checks_order_and_symbol_tables() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let ir = lower(&parsed).unwrap();
        verify_static_schedule(&ir).unwrap();

        let mut bad_node_order = ir.clone();
        bad_node_order.nodes[0].id = NodeId(99);
        assert!(
            verify_static_schedule(&bad_node_order)
                .unwrap_err()
                .contains("expected 0")
        );

        let mut bad_expr_id = ir.clone();
        bad_expr_id.nodes[0].expr_id = Some(ExprId(ir.expression_count));
        assert!(
            verify_static_schedule(&bad_expr_id)
                .unwrap_err()
                .contains("missing ExprId")
        );

        let mut bad_branch_source = ir.clone();
        bad_branch_source.update_branches[0].source = "store.sources.missing.press".to_owned();
        assert!(
            verify_static_schedule(&bad_branch_source)
                .unwrap_err()
                .contains("not a declared source port")
        );

        let mut bad_list_target = ir.clone();
        bad_list_target.list_operations[0].list = "missing_list".to_owned();
        assert!(
            verify_static_schedule(&bad_list_target)
                .unwrap_err()
                .contains("unknown list")
        );

        let mut bad_scope_ref = ir.clone();
        bad_scope_ref.sources[0].scope_id = Some(ScopeId(ir.row_scopes.len()));
        assert!(
            verify_static_schedule(&bad_scope_ref)
                .unwrap_err()
                .contains("missing ScopeId")
        );
    }

    #[test]
    fn while_is_scheduled_as_combinational_selection() {
        let source = include_str!("../../../examples/todomvc.bn").replace(
            "\n    selected_filter:",
            "\n    visible_when_selected:\n        selected_filter |> WHILE { True }\n\n    selected_filter:",
        );
        let parsed = boon_parser::parse_source("row-scope-fixture.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(
            ir.nodes
                .iter()
                .any(|node| matches!(node.kind, IrNodeKind::While))
        );
    }

    #[test]
    fn combinational_cycles_must_be_broken_by_hold() {
        let source = include_str!("../../../examples/todomvc.bn").replace(
            "\n    selected_filter:",
            "\n    cycle_left:\n        cycle_right |> WHILE { cycle_right }\n\n    cycle_right:\n        cycle_left |> WHILE { cycle_left }\n\n    selected_filter:",
        );
        let parsed = boon_parser::parse_source("row-scope-fixture.bn", source).unwrap();
        let error = lower(&parsed).unwrap_err();
        assert!(error.contains("combinational dependency cycle"));
        assert!(error.contains("broken by HOLD"));
    }

    #[test]
    fn cause_tables_are_derived_from_source_names() {
        let source = include_str!("../../../examples/todomvc.bn")
            .replace("filter_active", "filter_live")
            .replace("filter_completed", "filter_done");
        let parsed = boon_parser::parse_source("examples/todomvc.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let filter_causes = ir
            .possible_causes
            .iter()
            .find(|entry| entry.target == "store.selected_filter")
            .unwrap();
        assert!(
            filter_causes
                .sources
                .contains(&"store.sources.filter_live.press".to_owned())
        );
        assert!(
            filter_causes
                .sources
                .contains(&"store.sources.filter_done.press".to_owned())
        );
        assert!(
            !filter_causes
                .sources
                .contains(&"store.sources.filter_active.press".to_owned())
        );
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "store.selected_filter"
                && branch.source == "store.sources.filter_live.press"
                && branch.expression
                    == UpdateExpression::Const {
                        value: "Active".to_owned(),
                    }
        }));
    }

    #[test]
    fn cause_tables_derive_row_scope_from_list_map_function() {
        let source = include_str!("../../../examples/todomvc.bn")
            .replace(
                "new_todo(todo: todo, store: store)",
                "make_item(todo: todo, store: store)",
            )
            .replace(
                "FUNCTION new_todo(todo, store)",
                "FUNCTION make_item(todo, store)",
            );
        let parsed = boon_parser::parse_source("examples/todomvc.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(parsed.row_scope_functions.iter().any(|scope| {
            scope.function == "make_item" && scope.list == "todos" && scope.row_scope == "todo"
        }));
        assert!(
            ir.state_cells
                .iter()
                .any(|cell| cell.path == "todo.completed" && cell.indexed)
        );
        assert!(ir.possible_causes.iter().any(|entry| {
            entry.target == "todo.completed"
                && entry
                    .sources
                    .contains(&"todo.sources.todo_checkbox.click".to_owned())
        }));
    }

    #[test]
    fn indexed_lowering_uses_parsed_row_scopes_not_fixed_names() {
        let source = r#"
	store:
	    selected:
	        "All" |> HOLD selected { LATEST {} }
	    entries:
	        LIST[4] {}
	        |> List/map(entry, new: make_entry(entry: entry))
	    visible_entries:
	        entries
	        |> List/retain(entry, if:
	            selected |> WHEN {
	                All => True
	                Active => entry.completed |> Bool/not
	                Completed => entry.completed
	            }
	        )
	    active_count:
	        entries
	        |> List/retain(entry, if: entry.completed |> Bool/not)
	        |> List/count
	FUNCTION make_entry(entry) {
    sources:
        checkbox: [click: SOURCE]
    title:
        entry.title |> HOLD title { LATEST {} }
    completed:
        False |> HOLD completed {
            LATEST {
                sources.checkbox.click |> THEN { completed |> Bool/not() }
            }
        }
}
document:
    children:
"#;
        let parsed = boon_parser::parse_source("row-scope-fixture.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(parsed.row_scope_functions.iter().any(|scope| {
            scope.function == "make_entry" && scope.list == "entries" && scope.row_scope == "entry"
        }));
        assert!(
            ir.state_cells
                .iter()
                .any(|cell| cell.path == "entry.completed" && cell.indexed)
        );
        assert!(ir.dependencies.iter().any(|edge| {
            edge.from == "entry.sources.checkbox.click"
                && edge.to == "entry.completed"
                && edge.indexed
        }));
        assert!(ir.update_branches.iter().any(|branch| {
            branch.target == "entry.completed"
                && branch.source == "entry.sources.checkbox.click"
                && branch.indexed
                && branch.expression
                    == UpdateExpression::BoolNot {
                        path: "completed".to_owned(),
                    }
        }));
        assert!(
            ir.state_cells
                .iter()
                .any(|cell| cell.path == "store.selected" && !cell.indexed)
        );
        assert!(ir.list_operations.iter().any(|operation| {
            operation.list == "entries"
                && operation.kind
                    == ListOperationKind::Retain {
                        target: "store.visible_entries".to_owned(),
                        predicate: ListPredicate::SelectedFilterVisibility {
                            selector: "store.selected".to_owned(),
                            row_field: "entry.completed".to_owned(),
                        },
                    }
        }));
        assert!(ir.list_operations.iter().any(|operation| {
            operation.list == "entries"
                && operation.kind
                    == ListOperationKind::Count {
                        target: "store.active_count".to_owned(),
                        predicate: ListPredicate::RowFieldBoolNot {
                            path: "entry.completed".to_owned(),
                        },
                    }
        }));
    }

    #[test]
    fn derived_retain_views_do_not_register_as_list_memory_operations() {
        let source = r#"
SOURCE
HOLD
LATEST
store:
    active_file:
        TEXT { simple.vcd }
    viewport_label_start:
        0
    waveform_segment_records:
        [
            [file: TEXT { simple.vcd }, signal_id: TEXT { clk }, start_time_value: 0, end_time_value: 50, label: TEXT { 0xa }]
            [file: TEXT { simple.vcd }, signal_id: TEXT { clk }, start_time_value: 50, end_time_value: 150, label: TEXT { 0xc }]
        ]
    selected_waveform_segments:
        waveform_segment_records
        |> List/filter_field_equal(field: "file", value: active_file)
        |> List/retain(segment, if: segment.end_time_value > viewport_label_start)
        |> List/map(segment, new: segment)
document:
    children:
        []
"#;
        let parsed = boon_parser::parse_source("derived-retain-view.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        verify_static_schedule(&ir).unwrap();
        assert!(
            ir.list_operations.iter().all(|operation| {
                operation.list != "waveform_segment_records"
                    && !matches!(
                        operation.kind,
                        ListOperationKind::Retain { ref target, .. }
                            if target == "store.selected_waveform_segments"
                    )
            }),
            "derived retain/map views should stay derived values, not scheduled list memory operations"
        );
        assert!(ir.derived_values.iter().any(|value| {
            value.path == "store.selected_waveform_segments"
                && value.kind == DerivedValueKind::ListView
        }));
    }
}
