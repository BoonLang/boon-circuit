use boon_parser::{
    AstExpr, AstExprKind, AstRecordField, AstStatement, AstStatementKind, ParsedProgram,
    ParserItem as AstItem, ProgramKind,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypedProgram {
    pub kind: ProgramKind,
    pub expression_count: usize,
    pub expressions: Vec<AstExpr>,
    pub expression_coverage: ExpressionCoverage,
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

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum SourcePayloadField {
    Address,
    Key,
    Text,
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
    PreviousValue {
        path: String,
    },
    TextTrimOrPrevious {
        path: String,
        previous: String,
    },
    BoolNot {
        path: String,
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
    pub source: String,
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
    let typecheck_report = boon_typecheck::check(program);
    if typecheck_report.has_errors() {
        let messages = typecheck_report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "typecheck failed with {} diagnostic(s): {messages}",
            typecheck_report.diagnostics.len(),
        ));
    }
    let nodes = source_driven_nodes(program);
    let fields = typed_field_defs(program);
    let row_scopes = row_scopes(program);
    let sources = program
        .source_ports
        .iter()
        .enumerate()
        .map(|(id, source)| SourcePort {
            id: SourceId(id),
            scoped: source.scoped,
            scope_id: scope_id_for_path(&row_scopes, &source.path),
            payload_schema: source_payload_schema(program, &source.path),
            path: source.path.clone(),
        })
        .collect::<Vec<_>>();
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
    verify_combinational_field_cycles(&fields, &state_cells)?;
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
            initializer: list_initializer(program, &list.name),
        })
        .collect::<Vec<_>>();
    if nodes
        .iter()
        .any(|node| matches!(node.kind, IrNodeKind::ListMap) && !node.indexed)
    {
        return Err("List/map node must be indexed".to_owned());
    }
    let dependencies = dependency_edges(program, &state_cells);
    let possible_causes = possible_causes(program, &state_cells);
    let update_branches = update_branches(program, &state_cells);
    let list_operations = list_operations(program);
    let list_projections = list_projections(program);
    let functions = function_definitions(program);
    let derived_values = derived_values(program, &row_scopes, &fields, &state_cells);
    let view_bindings = view_bindings(program, &row_scopes, &sources, &typecheck_report);
    let expression_coverage = expression_coverage(
        program,
        &nodes,
        &state_cells,
        &lists,
        &derived_values,
        &update_branches,
        &list_operations,
    );
    let typed = TypedProgram {
        kind: program.kind,
        expression_count: program.expressions.len(),
        expressions: program.expressions.clone(),
        expression_coverage,
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
    verify_static_schedule(&typed)?;
    verify_hidden_identity(&typed)?;
    Ok(typed)
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
    let known_symbols = source_paths
        .iter()
        .chain(state_paths.iter())
        .chain(list_names.iter())
        .chain(derived_paths.iter())
        .copied()
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
        verify_scheduled_update_expression(&branch.expression, &known_symbols)?;
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
    program
        .row_scope_functions
        .iter()
        .enumerate()
        .map(|(id, scope)| RowScope {
            id: ScopeId(id),
            list: scope.list.clone(),
            function: scope.function.clone(),
            row_scope: scope.row_scope.clone(),
        })
        .collect()
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

fn source_payload_schema(program: &ParsedProgram, source: &str) -> SourcePayloadSchema {
    let fields = typed_field_defs(program);
    let variants = source_ref_variants(source);
    let mut payload_fields = BTreeSet::new();
    for field in &fields {
        if !direct_source_refs(field, program)
            .iter()
            .any(|direct_source| direct_source == source)
        {
            continue;
        }
        for variant in &variants {
            if field.references_payload_path(variant, "text")
                || field.match_arm_destructures_payload("text")
            {
                payload_fields.insert(SourcePayloadField::Text);
            }
            if field.references_payload_path(variant, "key")
                || field.match_arm_destructures_payload("key")
            {
                payload_fields.insert(SourcePayloadField::Key);
            }
            if field.references_payload_path(variant, "address")
                || field.match_arm_destructures_payload("address")
            {
                payload_fields.insert(SourcePayloadField::Address);
            }
        }
    }
    let address_lookup_field = source_address_lookup_field(program, source);
    if address_lookup_field.is_some() {
        payload_fields.insert(SourcePayloadField::Address);
    }
    SourcePayloadSchema {
        fields: payload_fields.into_iter().collect(),
        address_lookup_field,
    }
}

fn source_address_lookup_field(program: &ParsedProgram, source: &str) -> Option<String> {
    let Some(source_scope) = source.split('.').next() else {
        return None;
    };
    let fields = typed_field_defs(program);
    program
        .row_scope_functions
        .iter()
        .find(|scope| scope.row_scope == source_scope)
        .and_then(|scope| {
            fields.iter().find_map(|field| {
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
        })
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
    if let Some(document) = boon_parser::parsed_document(program) {
        let document_functions = DocumentViewFunctionRegistry::new(&program.ast.statements);
        collect_document_view_bindings(
            &document.root.children,
            &document.expressions,
            &document_functions,
            row_scopes,
            &source_paths,
            typecheck_report,
            &mut bindings,
            &mut Vec::new(),
            &DocumentViewBindingContext::default(),
        );
    }
    bindings
}

struct DocumentViewFunctionRegistry<'a> {
    functions: BTreeMap<&'a str, &'a AstStatement>,
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
        self.functions.get(name).copied()
    }
}

#[derive(Clone, Default)]
struct DocumentViewBindingContext {
    arg_exprs: Vec<BTreeMap<String, usize>>,
}

impl DocumentViewBindingContext {
    fn arg_expr(&self, name: &str) -> Option<usize> {
        self.arg_exprs
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
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

fn view_data_path_for_expr_id(
    expr_id: usize,
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
) -> Option<String> {
    let expr_id = document_resolved_expr_id(expr_id, expressions, context, &mut BTreeSet::new())?;
    view_data_path_for_expr(expressions.get(expr_id)?, expressions, context)
}

fn view_data_path_for_expr(
    expr: &AstExpr,
    expressions: &[AstExpr],
    context: &DocumentViewBindingContext,
) -> Option<String> {
    match &expr.kind {
        AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => {
            view_data_path(value)
        }
        AstExprKind::Identifier(value) => context
            .arg_expr(value)
            .and_then(|expr_id| view_data_path_for_expr_id(expr_id, expressions, context))
            .or_else(|| Some(value.clone())),
        AstExprKind::Path(parts) if parts.first().is_some_and(|part| part == "element") => None,
        AstExprKind::Path(parts) => Some(normalized_view_data_path(&parts.join("."))),
        AstExprKind::Infix { left, .. } => view_data_path_for_expr_id(*left, expressions, context),
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
    )
}

fn collect_document_view_bindings(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    functions: &DocumentViewFunctionRegistry<'_>,
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    typecheck_report: &boon_typecheck::TypeCheckReport,
    bindings: &mut Vec<ViewBinding>,
    function_stack: &mut Vec<String>,
    context: &DocumentViewBindingContext,
) {
    for statement in statements {
        if matches!(
            document_statement_field(statement).as_deref(),
            Some("items" | "children")
        ) && let Some(binding) = render_slot_list_map_binding(statement.id, typecheck_report)
        {
            if let Some(function_name) = binding.template_function.as_deref()
                && let Some(function_statement) = functions.get(function_name)
                && !function_stack.iter().any(|active| active == function_name)
            {
                function_stack.push(function_name.to_owned());
                let scoped_context = if !binding.template_args.is_empty() {
                    context.with_function_args(function_statement, &binding.template_args)
                } else {
                    context.with_function_item_expr(function_statement, binding.item_expr_id)
                };
                collect_document_view_bindings(
                    &function_statement.children,
                    expressions,
                    functions,
                    row_scopes,
                    source_paths,
                    typecheck_report,
                    bindings,
                    function_stack,
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
                context,
            );
        } else if let Some(function) = document_statement_call(statement, expressions)
            && let Some(function_statement) = functions.get(function)
            && !function_stack.iter().any(|active| active == function)
        {
            function_stack.push(function.to_owned());
            let scoped_context =
                context.with_function_call(function_statement, statement, expressions);
            collect_document_view_bindings(
                &function_statement.children,
                expressions,
                functions,
                row_scopes,
                source_paths,
                typecheck_report,
                bindings,
                function_stack,
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
                context,
            );
        }
        collect_document_view_bindings(
            &statement.children,
            expressions,
            functions,
            row_scopes,
            source_paths,
            typecheck_report,
            bindings,
            function_stack,
            context,
        );
    }
}

fn render_slot_list_map_binding<'a>(
    statement_id: usize,
    typecheck_report: &'a boon_typecheck::TypeCheckReport,
) -> Option<&'a boon_typecheck::ListMapBinding> {
    let slot = typecheck_report
        .render_slot_table
        .slots
        .iter()
        .find(|slot| slot.slot_statement_id == statement_id)?;
    let binding_id = slot.optional_list_map_binding_id?;
    typecheck_report.list_map_bindings.get(binding_id)
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
            continue;
        }
        if attr_can_bind_data(&attr)
            && let Some(expr_id) = child.expr
            && let Some(path) = view_data_path_for_expr_id(expr_id, expressions, context)
        {
            bindings.push(ViewBinding {
                id: ViewBindingId(bindings.len()),
                node_kind: node_kind.clone(),
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
        for nested in &child.children {
            if attr_can_bind_data(&attr)
                && document_statement_field(nested).as_deref() == Some("text")
                && let Some(expr_id) = nested.expr
                && let Some(path) = view_data_path_for_expr_id(expr_id, expressions, context)
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

fn canonical_view_node_kind(function: &str) -> &str {
    match function {
        "Element/text_input" => "Input",
        "Element/checkbox" => "Checkbox",
        "Element/button" => "Button",
        "Element/label" | "Element/text" | "Element/paragraph" | "Element/link" => "Text",
        "Element/stripe" if function.ends_with("stripe") => "Stripe",
        _ => function.strip_prefix("Element/").unwrap_or(function),
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
        for field in fields {
            if field.name == "events" {
                if let Some(group_path) =
                    document_expr_value_by_id(field.value, expressions, context)
                {
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
                        document_expr_value_by_id(source_field.value, expressions, context)
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
    for event_field in &element_field.children {
        if document_statement_field(event_field).as_deref() != Some("event") {
            continue;
        }
        for source_field in &event_field.children {
            let Some(attr) = document_statement_field(source_field) else {
                continue;
            };
            let Some(value) = document_statement_value(source_field, expressions, context) else {
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

fn push_canonical_view_event_group_bindings(
    node_kind: &str,
    group_path: &str,
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    bindings: &mut Vec<ViewBinding>,
) {
    let normalized_group = normalized_document_source_path(group_path);
    let prefix = format!("{normalized_group}.");
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

fn push_canonical_view_source_binding(
    node_kind: &str,
    attr: &str,
    value: &str,
    row_scopes: &[RowScope],
    source_paths: &[(&str, SourceId)],
    bindings: &mut Vec<ViewBinding>,
) {
    if let Some((path, source_id)) = source_paths
        .iter()
        .find(|(source_path, _)| *source_path == value)
    {
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
            .and_then(|expr_id| document_expr_value_by_id(expr_id, expressions, context))
            .or_else(|| Some(value.clone())),
        AstExprKind::Bool(value) => Some(value.to_string()),
        AstExprKind::Path(parts) => Some(parts.join(".")),
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
    known_symbols: &BTreeSet<&str>,
) -> Result<(), String> {
    match value {
        UpdateExpression::SourcePayload { .. } | UpdateExpression::Const { .. } => Ok(()),
        UpdateExpression::NumberInfix { left, right, .. } => {
            if left.parse::<i64>().is_err() {
                require_known_symbol("number infix left", left, known_symbols)?;
            }
            if right.parse::<i64>().is_err() {
                require_known_symbol("number infix right", right, known_symbols)?;
            }
            Ok(())
        }
        UpdateExpression::PreviousValue { path } | UpdateExpression::BoolNot { path } => {
            require_known_symbol("update expression path", path, known_symbols)
        }
        UpdateExpression::TextTrimOrPrevious { path, previous } => {
            if path != "text" && path != "key" {
                require_known_symbol("trim source", path, known_symbols)?;
            }
            require_known_symbol("trim previous", previous, known_symbols)
        }
        UpdateExpression::Unknown { summary } => Err(format!(
            "static schedule contains unsupported update expression `{summary}`"
        )),
    }
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
                require_known_symbol("append field source", &field.source, known_symbols)?;
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
            require_known_symbol("list predicate field", path, known_symbols)
        }
        ListPredicate::SelectedFilterVisibility {
            selector,
            row_field,
        } => {
            require_known_symbol("list predicate selector", selector, known_symbols)?;
            require_known_symbol("list predicate row field", row_field, known_symbols)
        }
        ListPredicate::Unknown { summary } => Err(format!(
            "static schedule contains unsupported list predicate `{summary}`"
        )),
    }
}

fn verify_combinational_field_cycles(
    fields: &[FieldDef],
    state_cells: &[StateCell],
) -> Result<(), String> {
    let state_paths = state_cells
        .iter()
        .map(|cell| cell.path.as_str())
        .collect::<BTreeSet<_>>();
    for field in fields
        .iter()
        .filter(|field| !state_paths.contains(field.path.as_str()))
    {
        let mut visiting = Vec::new();
        verify_combinational_field_cycles_from(field, fields, &state_paths, &mut visiting)?;
    }
    Ok(())
}

fn verify_combinational_field_cycles_from<'a>(
    field: &'a FieldDef,
    fields: &'a [FieldDef],
    state_paths: &BTreeSet<&str>,
    visiting: &mut Vec<&'a str>,
) -> Result<(), String> {
    if let Some(position) = visiting.iter().position(|path| *path == field.path) {
        let mut cycle = visiting[position..].to_vec();
        cycle.push(field.path.as_str());
        return Err(format!(
            "combinational dependency cycle through pure/WHILE expressions must be broken by HOLD: {}",
            cycle.join(" -> ")
        ));
    }
    if state_paths.contains(field.path.as_str()) {
        return Ok(());
    }
    visiting.push(field.path.as_str());
    for dependency in fields.iter().filter(|candidate| {
        candidate.parent_path == field.parent_path
            && candidate.path != field.path
            && candidate.local_name != field.local_name
            && field.mentions_identifier(&candidate.local_name)
    }) {
        if state_paths.contains(dependency.path.as_str()) {
            continue;
        }
        verify_combinational_field_cycles_from(dependency, fields, state_paths, visiting)?;
    }
    visiting.pop();
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
        if let ListProjectionKind::Find { field, value } = &projection.kind {
            reject_hidden_identity_identifier("list find field", field)?;
            reject_hidden_identity_identifier("list find value", value)?;
        }
    }
    Ok(())
}

fn reject_initial_value_identity(value: &InitialValue) -> Result<(), String> {
    match value {
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
        UpdateExpression::PreviousValue { path } | UpdateExpression::BoolNot { path } => {
            reject_hidden_identity_identifier("update expression path", path)
        }
        UpdateExpression::TextTrimOrPrevious { path, previous } => {
            reject_hidden_identity_identifier("trim source", path)?;
            reject_hidden_identity_identifier("trim previous", previous)
        }
        UpdateExpression::NumberInfix { left, right, .. } => {
            reject_hidden_identity_identifier("number infix left", left)?;
            reject_hidden_identity_identifier("number infix right", right)
        }
        UpdateExpression::Unknown { summary } => {
            reject_hidden_identity_identifier("unknown update expression", summary)
        }
        UpdateExpression::Const { value } => {
            reject_hidden_identity_identifier("const value", value)
        }
    }
}

fn reject_list_operation_identity(value: &ListOperationKind) -> Result<(), String> {
    match value {
        ListOperationKind::Append { trigger, fields } => {
            reject_hidden_identity_identifier("append trigger", trigger)?;
            for field in fields {
                reject_hidden_identity_identifier("append field", &field.name)?;
                reject_hidden_identity_identifier("append field source", &field.source)?;
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
    let tokens = lower
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .filter(|token| !token.is_empty());
    const FORBIDDEN: &[&str] = &[
        "runtime_key",
        "item_key",
        "hidden_key",
        "hidden_keys",
        "generation",
        "hidden_generation",
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
    } else if operators.iter().any(|operator| operator == "List/count") {
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

fn dependency_edges(program: &ParsedProgram, cells: &[StateCell]) -> Vec<DependencyEdge> {
    let mut edges = Vec::new();
    for cell in cells {
        for source in candidate_sources(program, &cell.path) {
            edges.push(DependencyEdge {
                indexed: cell.indexed || path_has_parsed_row_scope(program, &source),
                from: source,
                to: cell.path.clone(),
            });
        }
    }
    edges
}

fn possible_causes(program: &ParsedProgram, cells: &[StateCell]) -> Vec<PossibleCause> {
    cells
        .iter()
        .map(|cell| PossibleCause {
            target: cell.path.clone(),
            sources: candidate_sources(program, &cell.path),
        })
        .collect()
}

fn update_branches(program: &ParsedProgram, cells: &[StateCell]) -> Vec<UpdateBranch> {
    let fields = typed_field_defs(program);
    cells
        .iter()
        .flat_map(|cell| {
            let Some(field) = fields.iter().find(|field| field.path == cell.path) else {
                return Vec::new();
            };
            let mut branches = direct_source_refs(field, program)
                .into_iter()
                .map(|source| UpdateBranch {
                    expression: update_expression_for_source(program, &cell.path, field, &source),
                    indexed: cell.indexed,
                    target: cell.path.clone(),
                    source,
                })
                .collect::<Vec<_>>();
            branches.extend(derived_then_empty_update_branches(
                program, &fields, field, cell,
            ));
            branches
        })
        .collect()
}

fn derived_then_empty_update_branches(
    program: &ParsedProgram,
    fields: &[FieldDef],
    field: &FieldDef,
    cell: &StateCell,
) -> Vec<UpdateBranch> {
    let mut branches = Vec::new();
    for dependency in fields.iter().filter(|dependency| {
        dependency.parent_path == field.parent_path
            && dependency.path != field.path
            && field.mentions_identifier_expr(&dependency.local_name)
            && field.has_then_from_local_with_empty_output(&dependency.local_name)
    }) {
        for source in direct_source_refs(dependency, program) {
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
        if let Some(trigger) = list_append_trigger(field) {
            let fields = list_append_fields(field);
            operations.push(ListOperation {
                list: list_name.to_owned(),
                kind: ListOperationKind::Append { trigger, fields },
            });
        }
        for source in direct_source_refs(field, program) {
            let branch = field.source_branch(&source).unwrap_or_default();
            if branch.has_token("List/remove") || field.has_token("List/remove") {
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
            let Some(list) = count_or_retain_source_list(field) else {
                continue;
            };
            let row_scope = row_scope_for_list(program, &list);
            operations.push(ListOperation {
                list,
                kind: ListOperationKind::Count {
                    target: field.path.clone(),
                    predicate: list_retain_predicate(field, row_scope),
                },
            });
        } else if field.has_operator("List/retain") {
            let Some(list) = count_or_retain_source_list(field) else {
                continue;
            };
            let row_scope = row_scope_for_list(program, &list);
            operations.push(ListOperation {
                list,
                kind: ListOperationKind::Retain {
                    target: field.path.clone(),
                    predicate: list_retain_predicate(field, row_scope),
                },
            });
        }
    }
    operations
}

fn list_projections(program: &ParsedProgram) -> Vec<ListProjection> {
    typed_field_defs(program)
        .into_iter()
        .filter_map(|field| {
            if field.has_operator("List/chunk") {
                return Some(ListProjection {
                    target: field.path.clone(),
                    list: ast_call_argument(&field, "List/chunk")?,
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
                    list: ast_call_argument(&field, "List/find")?,
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

fn derived_values(
    program: &ParsedProgram,
    row_scopes: &[RowScope],
    fields: &[FieldDef],
    state_cells: &[StateCell],
) -> Vec<DerivedValue> {
    fields
        .iter()
        .filter(|field| {
            !state_cells.iter().any(|cell| cell.path == field.path)
                && !program.list_memories.iter().any(|list| {
                    field.path.ends_with(&format!(".{}", list.name)) || field.path == list.name
                })
        })
        .enumerate()
        .map(|(id, field)| {
            let sources = direct_source_refs(field, program);
            DerivedValue {
                id: FieldId(id),
                indexed: path_has_parsed_row_scope(program, &field.path),
                scope_id: scope_id_for_path(row_scopes, &field.path),
                kind: derived_value_kind(field, &sources),
                path: field.path.clone(),
                sources,
                statement: field.statement.clone(),
            }
        })
        .collect()
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
    if field.has_operator("List/count") {
        DerivedValueKind::Aggregate
    } else if field.has_any_operator(&["List/retain", "List/map", "List/chunk", "List/find"]) {
        DerivedValueKind::ListView
    } else if !sources.is_empty() || field.has_when_or_then_expr() {
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
        _ => InitialValue::Unknown {
            summary: ast_expr_label(expr),
        },
    }
}

fn list_initializer(program: &ParsedProgram, list_name: &str) -> ListInitializer {
    let Some(items) = list_body_items(program, list_name) else {
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
    Some(tokens[2..close].join(" "))
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

fn list_append_trigger(field: &FieldDef) -> Option<String> {
    let AstExprKind::Pipe { args, .. } = &list_append_expr(field)?.kind else {
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
        _ => ast_argument_value(field, item_arg.value)?,
    };
    (!trigger.is_empty()).then(|| canonical_local_path(&trigger, &field.parent_path))
}

fn list_append_fields(field: &FieldDef) -> Vec<ListAppendField> {
    let Some(append_expr) = list_append_expr(field) else {
        return Vec::new();
    };
    field
        .ast_exprs
        .iter()
        .filter(|expr| expr.id > append_expr.id)
        .find_map(|expr| match &expr.kind {
            AstExprKind::Record(fields) | AstExprKind::Object(fields) => Some(
                fields
                    .iter()
                    .filter_map(|record_field| {
                        let source = ast_argument_value(field, record_field.value)?;
                        (!record_field.name.is_empty() && !source.is_empty()).then(|| {
                            ListAppendField {
                                name: record_field.name.clone(),
                                source: canonical_local_path(&source, &field.parent_path),
                            }
                        })
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn list_append_expr(field: &FieldDef) -> Option<&AstExpr> {
    field.ast_exprs.iter().find(|expr| {
        matches!(
            &expr.kind,
            AstExprKind::Pipe { op, .. } if op == "List/append"
        )
    })
}

fn list_remove_predicate(
    field: &FieldDef,
    source: &str,
    branch: &RoutedBranch,
    row_scope: Option<&str>,
) -> ListPredicate {
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
            input_path == *variant
                || canonical_local_path(&input_path, &field.parent_path) == *variant
        });
        if !matches_source {
            return None;
        }
        list_predicate_from_expr(field, output, row_scope)
    })
}

fn list_predicate_from_expr(
    field: &FieldDef,
    expr_id: usize,
    row_scope: Option<&str>,
) -> Option<ListPredicate> {
    let expr = field.ast_exprs.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::Bool(true) => Some(ListPredicate::AlwaysTrue),
        AstExprKind::Pipe { input, op, .. } if op == "Bool/not" => {
            row_field_path_from_expr(field, *input, row_scope)
                .map(|path| ListPredicate::RowFieldBoolNot { path })
        }
        _ => row_field_path_from_expr(field, expr_id, row_scope)
            .map(|path| ListPredicate::RowFieldBool { path }),
    }
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
    if let Some(path) = row_field_path_in_exprs(&field.ast_exprs, row_scope)
        && bool_not_path_in_exprs(&field.ast_exprs).as_deref() == Some(path.as_str())
    {
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

fn count_or_retain_source_list(field: &FieldDef) -> Option<String> {
    let count_or_retain = field.ast_exprs.iter().find(|expr| {
        matches!(
            &expr.kind,
            AstExprKind::Pipe { op, .. } if op == "List/count" || op == "List/retain"
        )
    })?;
    source_list_from_expr(field, count_or_retain.id)
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PathMatch {
    Exact,
    Prefix,
}

fn path_parts_match(candidate: &[String], expected: &[&str], path_match: PathMatch) -> bool {
    let candidate = candidate
        .iter()
        .filter(|part| part.as_str() != "PASSED" && part.as_str() != "events")
        .map(String::as_str)
        .collect::<Vec<_>>();
    (match path_match {
        PathMatch::Exact => candidate.len() == expected.len(),
        PathMatch::Prefix => candidate.len() >= expected.len(),
    }) && candidate
        .iter()
        .take(expected.len())
        .copied()
        .eq(expected.iter().copied())
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

fn symbol_is_list_operator(symbol: &str) -> bool {
    matches!(
        symbol,
        "List/map"
            | "List/range"
            | "List/chunk"
            | "List/find"
            | "List/find_value"
            | "List/get"
            | "List/append"
            | "List/remove"
            | "List/retain"
            | "List/count"
            | "List/sum"
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
    source: &str,
) -> UpdateExpression {
    let variants = source_ref_variants(source);
    let branch = field.source_branch(source).unwrap_or_default();
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
    if let Some(path) = branch.bool_not_path() {
        return UpdateExpression::BoolNot { path };
    }
    if let Some(expression) = text_trim_or_previous_update(program, target, &branch) {
        return expression;
    }
    if let Some(expression) = branch.then_number_infix_expression(field, target) {
        return expression;
    }
    if let Some(value) = branch.then_simple_value() {
        return if value_starts_lowercase_identifier(&value) {
            UpdateExpression::PreviousValue { path: value }
        } else {
            UpdateExpression::Const { value }
        };
    }
    if variants
        .iter()
        .any(|variant| field.references_payload_path(variant, "text"))
    {
        return UpdateExpression::SourcePayload {
            path: "text".to_owned(),
        };
    }
    if variants
        .iter()
        .any(|variant| field.references_payload_path(variant, "key"))
    {
        return UpdateExpression::SourcePayload {
            path: "key".to_owned(),
        };
    }
    if variants
        .iter()
        .any(|variant| field.references_payload_path(variant, "address"))
    {
        return UpdateExpression::SourcePayload {
            path: "address".to_owned(),
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

fn candidate_sources(program: &ParsedProgram, target: &str) -> Vec<String> {
    let fields = typed_field_defs(program);
    let mut visited = Vec::new();
    candidate_sources_for_path(target, &fields, program, &mut visited)
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

    fn then_simple_value(&self) -> Option<String> {
        self.ast_exprs.iter().find_map(|expr| {
            let AstExprKind::Then { output, .. } = expr.kind else {
                return None;
            };
            if let Some(output) = output {
                return ast_simple_update_value_in_exprs(&self.ast_exprs, output);
            }
            self.ast_exprs
                .iter()
                .filter(|candidate| candidate.line > expr.line)
                .find_map(|candidate| {
                    ast_simple_update_value_in_exprs(&self.ast_exprs, candidate.id)
                })
        })
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

    fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
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

    fn has_when_or_then_expr(&self) -> bool {
        self.ast_exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                AstExprKind::When { .. } | AstExprKind::Then { .. }
            )
        })
    }

    fn mentions_identifier(&self, identifier: &str) -> bool {
        self.ast_items
            .iter()
            .any(|item| item.symbols.iter().any(|lexeme| lexeme == identifier))
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
        self.references_path_expr(source_variant, PathMatch::Prefix)
    }

    fn references_payload_path(&self, source_variant: &str, payload_field: &str) -> bool {
        let payload_path = format!("{source_variant}.{payload_field}");
        self.references_path_expr(&payload_path, PathMatch::Exact)
    }

    fn match_arm_destructures_payload(&self, payload_field: &str) -> bool {
        self.ast_exprs.iter().any(|expr| match &expr.kind {
            AstExprKind::MatchArm { pattern, .. } => {
                pattern.iter().any(|part| part == payload_field)
            }
            _ => false,
        })
    }

    fn references_path_expr(&self, path: &str, path_match: PathMatch) -> bool {
        let path_parts = dotted_path_parts(path);
        self.ast_exprs.iter().any(|expr| match &expr.kind {
            AstExprKind::Path(parts) => path_parts_match(parts, &path_parts, path_match),
            _ => false,
        })
    }

    fn source_branch(&self, source: &str) -> Option<RoutedBranch> {
        source_ref_variants(source)
            .iter()
            .find_map(|variant| self.source_branch_variant(variant))
    }

    fn source_branch_variant(&self, source_variant: &str) -> Option<RoutedBranch> {
        let source_parts = dotted_path_parts(source_variant);
        let start_line = self.ast_exprs.iter().find_map(|expr| match &expr.kind {
            AstExprKind::Path(parts)
                if path_parts_match(parts, &source_parts, PathMatch::Prefix) =>
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
        for (offset, item) in self.ast_items.iter().skip(start).take(6).enumerate() {
            if offset > 0 && item.indent <= start_indent {
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
            if offset == 0 && depth == 0 && scope_delta == 0 {
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

fn candidate_sources_for_path(
    target: &str,
    fields: &[FieldDef],
    program: &ParsedProgram,
    visited: &mut Vec<String>,
) -> Vec<String> {
    if visited.iter().any(|path| path == target) {
        return Vec::new();
    }
    visited.push(target.to_owned());
    let Some(field) = fields.iter().find(|field| field.path == target) else {
        visited.pop();
        return Vec::new();
    };
    let mut candidates = direct_source_refs(field, program);
    for dependency in fields.iter().filter(|candidate| {
        candidate.parent_path == field.parent_path
            && candidate.path != field.path
            && candidate.local_name != field.local_name
            && field.mentions_identifier(&candidate.local_name)
    }) {
        for source in candidate_sources_for_path(&dependency.path, fields, program, visited) {
            push_unique(&mut candidates, source);
        }
    }
    visited.pop();
    candidates
}

fn direct_source_refs(field: &FieldDef, program: &ParsedProgram) -> Vec<String> {
    let mut sources = Vec::new();
    for source in &program.source_ports {
        if source_ref_variants(&source.path)
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
    }
    variants
}

fn typed_field_defs(program: &ParsedProgram) -> Vec<FieldDef> {
    let mut fields = Vec::new();
    let items = program.ast.semantic_parser_items().collect::<Vec<_>>();
    gather_field_defs_from_statements(
        &program.ast.statements,
        &mut Vec::new(),
        program,
        &items,
        &mut fields,
    );
    fields
}

fn gather_field_defs_from_statements(
    statements: &[AstStatement],
    scope: &mut Vec<String>,
    program: &ParsedProgram,
    items: &[&AstItem],
    fields: &mut Vec<FieldDef>,
) {
    for statement in statements {
        match &statement.kind {
            AstStatementKind::Function { name, .. } => {
                if let Some(row_scope) = function_row_scope(name, program) {
                    scope.push(row_scope.to_owned());
                    gather_field_defs_from_statements(
                        &statement.children,
                        scope,
                        program,
                        items,
                        fields,
                    );
                    scope.pop();
                }
            }
            AstStatementKind::Field { name } => {
                if should_record_field_statement(name, scope, program) {
                    let parent_path = scope.join(".");
                    let path = if parent_path.is_empty() {
                        name.clone()
                    } else {
                        format!("{parent_path}.{name}")
                    };
                    fields.push(FieldDef {
                        path,
                        local_name: name.clone(),
                        parent_path,
                        statement: statement.clone(),
                        ast_items: collect_statement_ast_items(statement, items),
                        ast_exprs: collect_statement_ast_exprs(statement, program),
                    });
                }
                if !statement.children.is_empty() {
                    scope.push(name.clone());
                    gather_field_defs_from_statements(
                        &statement.children,
                        scope,
                        program,
                        items,
                        fields,
                    );
                    scope.pop();
                }
            }
            AstStatementKind::Block
            | AstStatementKind::Expression
            | AstStatementKind::Hold { .. }
            | AstStatementKind::List { .. }
            | AstStatementKind::Source { .. } => {
                gather_field_defs_from_statements(
                    &statement.children,
                    scope,
                    program,
                    items,
                    fields,
                );
            }
        }
    }
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
    local_name != "sources"
        && !scope.iter().any(|name| name == "sources")
        && scope.iter().any(|name| {
            name == "store"
                || program
                    .row_scope_functions
                    .iter()
                    .any(|scope| scope.row_scope == *name)
        })
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

fn function_row_scope<'a>(name: &str, program: &'a ParsedProgram) -> Option<&'a str> {
    program
        .row_scope_functions
        .iter()
        .find(|scope| scope.function == name)
        .map(|scope| scope.row_scope.as_str())
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
                            source: "store.title_to_add".to_owned(),
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
                            source: "store.pending_title".to_owned(),
                        }],
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
                    == UpdateExpression::PreviousValue {
                        path: "formula_text".to_owned(),
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
        with_bad_state.state_cells[0].path = "todo.generation".to_owned();
        assert!(
            verify_hidden_identity(&with_bad_state)
                .unwrap_err()
                .contains("generation")
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
}
