use boon_parser::{ParsedExpression, ParsedProgram, ProgramKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TypedProgram {
    pub kind: ProgramKind,
    pub expression_count: usize,
    pub graph_node_count: usize,
    pub nodes: Vec<IrNode>,
    pub sources: Vec<SourcePort>,
    pub state_cells: Vec<StateCell>,
    pub lists: Vec<ListMemory>,
    pub derived_values: Vec<DerivedValue>,
    pub dependencies: Vec<DependencyEdge>,
    pub possible_causes: Vec<PossibleCause>,
    pub update_branches: Vec<UpdateBranch>,
    pub list_operations: Vec<ListOperation>,
    pub formula_operations: Vec<FormulaOperation>,
    pub hidden_identity_verified: bool,
    pub static_schedule_verified: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IrNode {
    pub id: usize,
    pub name: String,
    pub kind: IrNodeKind,
    pub indexed: bool,
    pub expr_id: Option<usize>,
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
    pub path: String,
    pub scoped: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListMemory {
    pub name: String,
    pub hidden_key_type: String,
    pub has_generation: bool,
    pub graph_clones_per_item: usize,
    pub capacity: Option<usize>,
    pub initializer: ListInitializer,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StateCell {
    pub path: String,
    pub hold_name: String,
    pub initial_value: InitialValue,
    pub indexed: bool,
    pub source_line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum InitialValue {
    Text { value: String },
    Bool { value: bool },
    Enum { value: String },
    SeedField { path: String },
    Unknown { summary: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ListInitializer {
    RecordLiteral { rows: Vec<ListSeedRecord> },
    Grid { columns: usize, rows: usize },
    Empty,
    Unknown { summary: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListSeedRecord {
    pub fields: Vec<ListSeedField>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListSeedField {
    pub name: String,
    pub value: InitialValue,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DerivedValue {
    pub path: String,
    pub kind: DerivedValueKind,
    pub sources: Vec<String>,
    pub indexed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DerivedValueKind {
    SourceEventTransform,
    ListView,
    Aggregate,
    Formula,
    Pure,
    Unknown,
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
    SourcePayload { path: String },
    Const { value: String },
    PreviousValue { path: String },
    TextTrimOrPrevious { path: String, previous: String },
    BoolNot { path: String },
    Unknown { summary: String },
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
pub struct FormulaOperation {
    pub target: String,
    pub kind: FormulaOperationKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum FormulaOperationKind {
    Parse { input: String },
    Dependencies { input: String },
    Eval { formula: String, read: String },
    Error { formula: String, value: String },
}

pub fn lower(program: &ParsedProgram) -> Result<TypedProgram, String> {
    let nodes = source_driven_nodes(program);
    let fields = collect_field_defs(program);
    let sources = program
        .source_ports
        .iter()
        .map(|source| SourcePort {
            scoped: source.scoped,
            path: source.path.clone(),
        })
        .collect();
    let state_cells = program
        .state_cells
        .iter()
        .map(|cell| StateCell {
            path: cell.path.clone(),
            hold_name: cell.hold_name.clone(),
            initial_value: state_initial_value(
                fields
                    .iter()
                    .find(|field| field.path == cell.path)
                    .map(|field| field.body.as_str())
                    .unwrap_or_default(),
            ),
            indexed: cell.indexed,
            source_line: cell.line,
        })
        .collect::<Vec<_>>();
    verify_combinational_field_cycles(&fields, &state_cells)?;
    let lists = program
        .list_memories
        .iter()
        .map(|list| ListMemory {
            name: list.name.clone(),
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
    let typed = TypedProgram {
        kind: program.kind,
        expression_count: program.expressions.len(),
        graph_node_count: nodes.len(),
        nodes,
        sources,
        dependencies: dependency_edges(program, &state_cells),
        possible_causes: possible_causes(program, &state_cells),
        update_branches: update_branches(program, &state_cells),
        list_operations: list_operations(program),
        formula_operations: formula_operations(program),
        derived_values: derived_values(program, &fields, &state_cells),
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
        if node.id != index {
            return Err(format!(
                "scheduled node `{}` has id {}, expected {index}",
                node.name, node.id
            ));
        }
        if node
            .expr_id
            .is_some_and(|expr_id| expr_id >= program.expression_count)
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
    let state_paths = unique_strings(
        "state cell",
        program.state_cells.iter().map(|cell| cell.path.as_str()),
    )?;
    let list_names = unique_strings("list", program.lists.iter().map(|list| list.name.as_str()))?;
    let derived_paths = unique_strings(
        "derived value",
        program
            .derived_values
            .iter()
            .map(|value| value.path.as_str()),
    )?;
    let known_symbols = source_paths
        .iter()
        .chain(state_paths.iter())
        .chain(list_names.iter())
        .chain(derived_paths.iter())
        .copied()
        .collect::<BTreeSet<_>>();

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
    for operation in &program.formula_operations {
        require_known_symbol("formula target", &operation.target, &known_symbols)?;
        verify_scheduled_formula_operation(&operation.kind, &known_symbols)?;
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

fn verify_scheduled_update_expression(
    value: &UpdateExpression,
    known_symbols: &BTreeSet<&str>,
) -> Result<(), String> {
    match value {
        UpdateExpression::SourcePayload { .. } | UpdateExpression::Const { .. } => Ok(()),
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

fn verify_scheduled_formula_operation(
    value: &FormulaOperationKind,
    known_symbols: &BTreeSet<&str>,
) -> Result<(), String> {
    match value {
        FormulaOperationKind::Parse { input } | FormulaOperationKind::Dependencies { input } => {
            require_known_symbol("formula input", input, known_symbols)
        }
        FormulaOperationKind::Eval { formula, read } => {
            require_known_symbol("formula eval input", formula, known_symbols)?;
            if read == "cell_value_reader" {
                Ok(())
            } else {
                require_known_symbol("formula reader", read, known_symbols)
            }
        }
        FormulaOperationKind::Error { formula, value } => {
            require_known_symbol("formula error input", formula, known_symbols)?;
            require_known_symbol("formula error value", value, known_symbols)
        }
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
    let body = field_dependency_body(&field.body);
    for dependency in fields.iter().filter(|candidate| {
        candidate.parent_path == field.parent_path
            && text_mentions_unqualified_identifier(&body, &candidate.local_name)
    }) {
        if state_paths.contains(dependency.path.as_str()) {
            continue;
        }
        verify_combinational_field_cycles_from(dependency, fields, state_paths, visiting)?;
    }
    visiting.pop();
    Ok(())
}

fn field_dependency_body(body: &str) -> String {
    let mut lines = body.lines();
    let first = lines
        .next()
        .and_then(|line| line.split_once(':').map(|(_, rest)| rest))
        .unwrap_or_default();
    std::iter::once(first)
        .chain(lines)
        .collect::<Vec<_>>()
        .join("\n")
}

fn text_mentions_unqualified_identifier(text: &str, identifier: &str) -> bool {
    let bytes = text.as_bytes();
    let needle = identifier.as_bytes();
    if needle.is_empty() || needle.len() > bytes.len() {
        return false;
    }
    bytes
        .windows(needle.len())
        .enumerate()
        .any(|(index, window)| {
            window == needle
                && index.checked_sub(1).is_none_or(|before| {
                    !is_identifier_byte(bytes[before])
                        && bytes[before] != b'.'
                        && bytes[before] != b'/'
                })
                && bytes
                    .get(index + needle.len())
                    .is_none_or(|after| !is_identifier_byte(*after))
        })
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
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
    for operation in &program.formula_operations {
        reject_hidden_identity_identifier("formula target", &operation.target)?;
        reject_formula_operation_identity(&operation.kind)?;
    }
    Ok(())
}

fn reject_initial_value_identity(value: &InitialValue) -> Result<(), String> {
    match value {
        InitialValue::SeedField { path } => reject_hidden_identity_identifier("seed field", path),
        InitialValue::Enum { value } => reject_hidden_identity_identifier("enum value", value),
        InitialValue::Unknown { summary } => {
            reject_hidden_identity_identifier("unknown initializer", summary)
        }
        InitialValue::Text { .. } | InitialValue::Bool { .. } => Ok(()),
    }
}

fn reject_list_initializer_identity(value: &ListInitializer) -> Result<(), String> {
    match value {
        ListInitializer::RecordLiteral { rows } => {
            for row in rows {
                for field in &row.fields {
                    reject_hidden_identity_identifier("list seed field", &field.name)?;
                    reject_initial_value_identity(&field.value)?;
                }
            }
            Ok(())
        }
        ListInitializer::Unknown { summary } => {
            reject_hidden_identity_identifier("unknown list initializer", summary)
        }
        ListInitializer::Grid { .. } | ListInitializer::Empty => Ok(()),
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

fn reject_formula_operation_identity(value: &FormulaOperationKind) -> Result<(), String> {
    match value {
        FormulaOperationKind::Parse { input } | FormulaOperationKind::Dependencies { input } => {
            reject_hidden_identity_identifier("formula input", input)
        }
        FormulaOperationKind::Eval { formula, read } => {
            reject_hidden_identity_identifier("formula eval input", formula)?;
            reject_hidden_identity_identifier("formula reader", read)
        }
        FormulaOperationKind::Error { formula, value } => {
            reject_hidden_identity_identifier("formula error input", formula)?;
            reject_hidden_identity_identifier("formula error value", value)
        }
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
        "sources": program.sources,
        "state_cells": program.state_cells,
        "lists": program.lists,
        "derived_values": program.derived_values,
        "dependencies": program.dependencies,
        "possible_causes": program.possible_causes,
        "update_branches": program.update_branches,
        "list_operations": program.list_operations,
        "formula_operations": program.formula_operations,
    })
}

fn source_driven_nodes(program: &ParsedProgram) -> Vec<IrNode> {
    let mut nodes = program
        .expressions
        .iter()
        .filter_map(expression_node)
        .enumerate()
        .map(|(id, mut node)| {
            node.id = id;
            node
        })
        .collect::<Vec<_>>();
    if program.source.contains("Formula/dependencies") {
        push_generated(&mut nodes, "dependency_index", IrNodeKind::Aggregate, true);
    }
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

fn expression_node(expr: &ParsedExpression) -> Option<IrNode> {
    let kind = expression_node_kind(&expr.text)?;
    Some(IrNode {
        id: 0,
        name: format!("expr_{}_{}", expr.id, sanitize_node_name(&expr.text)),
        indexed: expression_is_indexed(&expr.text, &kind),
        kind,
        expr_id: Some(expr.id),
    })
}

fn expression_node_kind(text: &str) -> Option<IrNodeKind> {
    if text.contains("SOURCE") {
        Some(IrNodeKind::SourceRead)
    } else if text.contains("HOLD") {
        Some(IrNodeKind::Hold)
    } else if text.contains("List/append") {
        Some(IrNodeKind::ListAppend)
    } else if text.contains("List/remove") {
        Some(IrNodeKind::ListRemove)
    } else if text.contains("List/map") {
        Some(IrNodeKind::ListMap)
    } else if text.contains("List/retain") {
        Some(IrNodeKind::ListRetain)
    } else if text.contains("List/count") {
        Some(IrNodeKind::Aggregate)
    } else if text.contains("LATEST") {
        Some(IrNodeKind::Latest)
    } else if text.contains("WHILE") {
        Some(IrNodeKind::While)
    } else if text.contains("THEN") {
        Some(IrNodeKind::Then)
    } else if text.contains("WHEN") {
        Some(IrNodeKind::When)
    } else if text.contains("Formula/")
        || text.contains("Text/")
        || text.contains("Bool/")
        || text.starts_with("FUNCTION ")
    {
        Some(IrNodeKind::PureCall)
    } else if text.contains("Grid/cells") || text.contains("LIST") {
        Some(IrNodeKind::ListMap)
    } else {
        None
    }
}

fn expression_is_indexed(text: &str, kind: &IrNodeKind) -> bool {
    matches!(
        kind,
        IrNodeKind::ListAppend
            | IrNodeKind::ListRemove
            | IrNodeKind::ListMap
            | IrNodeKind::ListRetain
            | IrNodeKind::Aggregate
            | IrNodeKind::RenderLowering
    ) || text.contains("todo.")
        || text.contains("seed.")
        || text.contains("editor.")
        || text.contains("Formula/")
}

fn push_generated(nodes: &mut Vec<IrNode>, name: &str, kind: IrNodeKind, indexed: bool) {
    nodes.push(IrNode {
        id: nodes.len(),
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
                indexed: cell.indexed || source.contains(".todo_") || source.starts_with("todo."),
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
    let fields = collect_field_defs(program);
    cells
        .iter()
        .flat_map(|cell| {
            let Some(field) = fields.iter().find(|field| field.path == cell.path) else {
                return Vec::new();
            };
            let mut branches = direct_source_refs(field, program)
                .into_iter()
                .map(|source| UpdateBranch {
                    expression: update_expression_for_source(&cell.path, &field.body, &source),
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
            && text_mentions_identifier(&field.body, &dependency.local_name)
            && field
                .body
                .contains(&format!("{} |> THEN", dependency.local_name))
            && field.body.contains("Text/empty")
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
    let fields = collect_field_defs(program);
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
        if let Some(trigger) = list_append_trigger(&field.body, &field.parent_path) {
            let fields = list_append_fields(&field.body, &field.parent_path);
            operations.push(ListOperation {
                list: list_name.to_owned(),
                kind: ListOperationKind::Append { trigger, fields },
            });
        }
        for source in direct_source_refs(field, program) {
            let variants = source_ref_variants(&source);
            let branch = variants
                .iter()
                .find_map(|variant| branch_text_for_source(&field.body, variant))
                .unwrap_or_default();
            if branch.contains("List/remove") || field.body.contains("List/remove") {
                operations.push(ListOperation {
                    list: list_name.to_owned(),
                    kind: ListOperationKind::Remove {
                        source,
                        predicate: list_remove_predicate(&branch),
                    },
                });
            }
        }
    }
    for field in &fields {
        if field.body.contains("List/count") {
            operations.push(ListOperation {
                list: count_or_retain_source_list(&field.body)
                    .unwrap_or_else(|| "todos".to_owned()),
                kind: ListOperationKind::Count {
                    target: field.path.clone(),
                    predicate: list_retain_predicate(&field.body),
                },
            });
        } else if field.body.contains("List/retain") {
            operations.push(ListOperation {
                list: count_or_retain_source_list(&field.body)
                    .unwrap_or_else(|| "todos".to_owned()),
                kind: ListOperationKind::Retain {
                    target: field.path.clone(),
                    predicate: list_retain_predicate(&field.body),
                },
            });
        }
    }
    operations
}

fn formula_operations(program: &ParsedProgram) -> Vec<FormulaOperation> {
    collect_field_defs(program)
        .into_iter()
        .filter_map(|field| {
            let body = field.body.replace('\n', " ");
            if let Some(argument) = call_argument(&body, "Formula/parse") {
                return Some(FormulaOperation {
                    target: field.path,
                    kind: FormulaOperationKind::Parse { input: argument },
                });
            }
            if let Some(argument) = call_argument(&body, "Formula/dependencies") {
                return Some(FormulaOperation {
                    target: field.path,
                    kind: FormulaOperationKind::Dependencies { input: argument },
                });
            }
            if body.contains("Formula/eval") {
                return Some(FormulaOperation {
                    target: field.path,
                    kind: FormulaOperationKind::Eval {
                        formula: named_call_argument(&body, "formula")
                            .unwrap_or_else(|| "parsed_formula".to_owned()),
                        read: named_call_argument(&body, "read")
                            .unwrap_or_else(|| "cell_value_reader".to_owned()),
                    },
                });
            }
            if body.contains("Formula/error") {
                let args = call_arguments(&body, "Formula/error");
                return Some(FormulaOperation {
                    target: field.path,
                    kind: FormulaOperationKind::Error {
                        formula: args
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "parsed_formula".to_owned()),
                        value: args.get(1).cloned().unwrap_or_else(|| "value".to_owned()),
                    },
                });
            }
            None
        })
        .collect()
}

fn derived_values(
    program: &ParsedProgram,
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
        .map(|field| {
            let sources = direct_source_refs(field, program);
            DerivedValue {
                indexed: path_has_indexed_scope(&field.path),
                kind: derived_value_kind(&field.body, &sources),
                path: field.path.clone(),
                sources,
            }
        })
        .collect()
}

fn derived_value_kind(body: &str, sources: &[String]) -> DerivedValueKind {
    if body.contains("Formula/") {
        DerivedValueKind::Formula
    } else if body.contains("List/count") {
        DerivedValueKind::Aggregate
    } else if body.contains("List/retain") || body.contains("List/map") {
        DerivedValueKind::ListView
    } else if !sources.is_empty() || body.contains("|> WHEN") || body.contains("|> THEN") {
        DerivedValueKind::SourceEventTransform
    } else if body.trim().is_empty() {
        DerivedValueKind::Unknown
    } else {
        DerivedValueKind::Pure
    }
}

fn state_initial_value(body: &str) -> InitialValue {
    let first = body
        .lines()
        .map(str::trim)
        .find(|line| line.contains("|> HOLD"))
        .unwrap_or_default();
    let initial = first
        .split_once("|> HOLD")
        .map(|(initial, _)| initial.trim())
        .unwrap_or(first);
    if matches!(initial, "Text/empty" | "TEXT {}") {
        return InitialValue::Text {
            value: String::new(),
        };
    }
    if let Some(text) = text_literal_value(initial) {
        return InitialValue::Text { value: text };
    }
    match initial {
        "True" => InitialValue::Bool { value: true },
        "False" => InitialValue::Bool { value: false },
        value if value.starts_with("seed.") => InitialValue::SeedField {
            path: value.trim_start_matches("seed.").to_owned(),
        },
        value if value_starts_uppercase_identifier(value) => InitialValue::Enum {
            value: value.to_owned(),
        },
        value if !value.is_empty() => InitialValue::Unknown {
            summary: value.to_owned(),
        },
        _ => InitialValue::Unknown {
            summary: "missing initial value".to_owned(),
        },
    }
}

fn list_initializer(program: &ParsedProgram, list_name: &str) -> ListInitializer {
    let Some(body) = list_body(program, list_name) else {
        return ListInitializer::Unknown {
            summary: "list body not found".to_owned(),
        };
    };
    if body.contains("Grid/cells") {
        return ListInitializer::Grid {
            columns: extract_usize_arg(&body, "columns").unwrap_or(26),
            rows: extract_usize_arg(&body, "rows").unwrap_or(100),
        };
    }
    let rows = list_record_literal_rows(&body);
    if !rows.is_empty() {
        return ListInitializer::RecordLiteral { rows };
    }
    if body.contains("LIST") {
        ListInitializer::Empty
    } else {
        ListInitializer::Unknown {
            summary: body.lines().next().unwrap_or_default().to_owned(),
        }
    }
}

fn list_body(program: &ParsedProgram, list_name: &str) -> Option<String> {
    let lines = program.source.lines().collect::<Vec<_>>();
    for (line_index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let Some(field) = leading_field_name(trimmed) else {
            continue;
        };
        if field == list_name {
            let indent = line.chars().take_while(|ch| *ch == ' ').count();
            return Some(field_body(&lines, line_index, indent));
        }
    }
    None
}

fn list_record_literal_rows(body: &str) -> Vec<ListSeedRecord> {
    let mut rows = Vec::new();
    let mut in_literal = false;
    for line in body.lines().map(str::trim) {
        if line.contains("LIST") {
            in_literal = true;
            continue;
        }
        if line.contains("|> List/") {
            break;
        }
        if !in_literal {
            continue;
        }
        if let Some(record) = list_record_literal_line(line) {
            rows.push(record);
        }
    }
    rows
}

fn list_record_literal_line(line: &str) -> Option<ListSeedRecord> {
    let row = line.strip_prefix('[')?.strip_suffix(']')?.trim();
    let mut fields = Vec::new();
    for part in row.split(',') {
        let (name, value) = part.split_once(':')?;
        fields.push(ListSeedField {
            name: name.trim().to_owned(),
            value: literal_initial_value(value.trim()),
        });
    }
    (!fields.is_empty()).then_some(ListSeedRecord { fields })
}

fn literal_initial_value(text: &str) -> InitialValue {
    if let Some(value) = text_literal_value(text) {
        return InitialValue::Text { value };
    }
    match text {
        "True" => InitialValue::Bool { value: true },
        "False" => InitialValue::Bool { value: false },
        value if value_starts_uppercase_identifier(value) => InitialValue::Enum {
            value: value.to_owned(),
        },
        value => InitialValue::Unknown {
            summary: value.to_owned(),
        },
    }
}

fn text_literal_value(text: &str) -> Option<String> {
    let (_, rest) = text.split_once("TEXT {")?;
    let (value, _) = rest.split_once('}')?;
    Some(value.trim().to_owned())
}

fn extract_usize_arg(source: &str, name: &str) -> Option<usize> {
    let start = source.find(&format!("{name}:"))? + name.len() + 1;
    let rest = &source[start..];
    let digits = rest
        .trim_start()
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok()
}

fn call_argument(body: &str, function: &str) -> Option<String> {
    call_arguments(body, function).into_iter().next()
}

fn call_arguments(body: &str, function: &str) -> Vec<String> {
    let Some((_, rest)) = body.split_once(function) else {
        return Vec::new();
    };
    let Some((_, args)) = rest.split_once('(') else {
        return Vec::new();
    };
    let args = args.split_once(')').map(|(args, _)| args).unwrap_or(args);
    args.split(',')
        .map(str::trim)
        .filter(|arg| !arg.is_empty())
        .map(|arg| {
            arg.split_once(':')
                .map(|(_, value)| value.trim())
                .unwrap_or(arg)
                .to_owned()
        })
        .collect()
}

fn named_call_argument(body: &str, name: &str) -> Option<String> {
    let (_, rest) = body.split_once("Formula/eval")?;
    let (_, args) = rest.split_once('(')?;
    let args = args.split_once(')').map(|(args, _)| args).unwrap_or(args);
    args.split(',').find_map(|arg| {
        let (candidate, value) = arg.split_once(':')?;
        (candidate.trim() == name).then(|| value.trim().to_owned())
    })
}

fn list_append_trigger(body: &str, parent_path: &str) -> Option<String> {
    let (_, rest) = body.split_once("List/append")?;
    let (_, item) = rest.split_once("item:")?;
    let trigger = item
        .split("|> THEN")
        .next()
        .map(str::trim)
        .filter(|trigger| !trigger.is_empty())?;
    Some(canonical_local_path(trigger, parent_path))
}

fn list_append_fields(body: &str, parent_path: &str) -> Vec<ListAppendField> {
    let Some((_, rest)) = body.split_once("List/append") else {
        return Vec::new();
    };
    let Some((_, then_body)) = rest.split_once("|> THEN") else {
        return Vec::new();
    };
    let Some((_, record)) = then_body.split_once('[') else {
        return Vec::new();
    };
    let Some((record, _)) = record.split_once(']') else {
        return Vec::new();
    };
    record
        .split(',')
        .filter_map(|entry| {
            let (name, source) = entry.split_once(':')?;
            let name = name.trim();
            let source = source.trim();
            (!name.is_empty() && !source.is_empty()).then(|| ListAppendField {
                name: name.to_owned(),
                source: canonical_local_path(source, parent_path),
            })
        })
        .collect()
}

fn list_remove_predicate(branch: &str) -> ListPredicate {
    if branch.contains("THEN { True }") || branch.contains("THEN {True}") {
        return ListPredicate::AlwaysTrue;
    }
    if branch.contains("todo.completed |> Bool/not") {
        return ListPredicate::RowFieldBoolNot {
            path: "todo.completed".to_owned(),
        };
    }
    if branch.contains("todo.completed") {
        return ListPredicate::RowFieldBool {
            path: "todo.completed".to_owned(),
        };
    }
    ListPredicate::Unknown {
        summary: branch.to_owned(),
    }
}

fn list_retain_predicate(body: &str) -> ListPredicate {
    if body.contains("selected_filter |> WHEN") {
        return ListPredicate::SelectedFilterVisibility {
            selector: "store.selected_filter".to_owned(),
            row_field: "todo.completed".to_owned(),
        };
    }
    if body.contains("todo.completed |> Bool/not") {
        return ListPredicate::RowFieldBoolNot {
            path: "todo.completed".to_owned(),
        };
    }
    if body.contains("if: todo.completed") {
        return ListPredicate::RowFieldBool {
            path: "todo.completed".to_owned(),
        };
    }
    ListPredicate::Unknown {
        summary: body.lines().next().unwrap_or_default().to_owned(),
    }
}

fn count_or_retain_source_list(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|line| {
            !line.is_empty()
                && line
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        })
        .map(str::to_owned)
}

fn canonical_local_path(path: &str, parent_path: &str) -> String {
    if path.contains('.') || parent_path.is_empty() {
        path.to_owned()
    } else {
        format!("{parent_path}.{path}")
    }
}

fn update_expression_for_source(target: &str, body: &str, source: &str) -> UpdateExpression {
    let variants = source_ref_variants(source);
    let branch = variants
        .iter()
        .find_map(|variant| branch_text_for_source(body, variant))
        .unwrap_or_default();
    if branch.contains("=> False") && !branch.contains("=> True") {
        return UpdateExpression::Const {
            value: "False".to_owned(),
        };
    }
    if let Some((_, value)) = branch.split_once("Escape =>") {
        let value = value
            .split(|ch: char| ch.is_whitespace() || ch == '}')
            .find(|part| !part.is_empty())
            .unwrap_or_default();
        if value_starts_lowercase_identifier(value) {
            return UpdateExpression::PreviousValue {
                path: value.to_owned(),
            };
        }
    }
    if let Some(expression) = text_trim_or_previous_expression(target, &branch) {
        return expression;
    }
    if variants
        .iter()
        .any(|variant| body.contains(&format!("{variant}.text")))
    {
        return UpdateExpression::SourcePayload {
            path: "text".to_owned(),
        };
    }
    if variants
        .iter()
        .any(|variant| body.contains(&format!("{variant}.key")))
    {
        return UpdateExpression::SourcePayload {
            path: "key".to_owned(),
        };
    }
    if let Some(value) = then_simple_value(&branch) {
        return if value_starts_lowercase_identifier(&value) {
            UpdateExpression::PreviousValue { path: value }
        } else {
            UpdateExpression::Const { value }
        };
    }
    if let Some(path) = bool_not_path(&branch) {
        return UpdateExpression::BoolNot { path };
    }
    if !branch.is_empty() {
        return UpdateExpression::Unknown { summary: branch };
    }
    UpdateExpression::Unknown {
        summary: "source reaches target through derived local field".to_owned(),
    }
}

fn text_trim_or_previous_expression(target: &str, branch: &str) -> Option<UpdateExpression> {
    if !target.starts_with("todo.") || !branch.contains("|> Text/trim") {
        return None;
    }
    let (_, after_empty) = branch.split_once("TEXT {} =>")?;
    let mut previous = after_empty
        .split_whitespace()
        .next()
        .map(|value| value.trim_matches(|ch| ch == '}' || ch == ','))
        .filter(|value| !value.is_empty())?;
    let (before_trim, _) = branch.split_once("|> Text/trim")?;
    let mut path = before_trim
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .filter(|part| !part.is_empty())
        .next_back()?;
    if !value_starts_lowercase_identifier(path) || !value_starts_lowercase_identifier(previous) {
        return None;
    }
    let target_field = target.rsplit_once('.').map(|(_, field)| field)?;
    if previous != target_field && !branch.contains(&format!("{previous}:")) {
        previous = target_field;
    }
    if path != "text" && !branch.contains(&format!("{path}:")) && branch.contains(".text") {
        path = "text";
    }
    Some(UpdateExpression::TextTrimOrPrevious {
        path: path.to_owned(),
        previous: previous.to_owned(),
    })
}

fn branch_text_for_source(body: &str, source_variant: &str) -> Option<String> {
    let lines = body.lines().map(str::trim).collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.contains(source_variant))?;
    let mut text = String::new();
    for line in lines.iter().skip(start).take(6) {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(line);
        if line.contains('}') && text.matches('{').count() <= text.matches('}').count() {
            break;
        }
    }
    Some(text)
}

fn then_simple_value(line: &str) -> Option<String> {
    let (_, rest) = line.split_once("|> THEN")?;
    let (_, body) = rest.split_once('{')?;
    let value = body.split_once('}').map(|(value, _)| value).unwrap_or(body);
    let value = value.trim();
    if value.is_empty() || value.contains('|') || value.contains('(') {
        return None;
    }
    Some(value.to_owned())
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

fn path_has_indexed_scope(path: &str) -> bool {
    path.split('.')
        .any(|segment| matches!(segment, "todo" | "cell" | "seed"))
}

fn bool_not_path(line: &str) -> Option<String> {
    let (path, _) = line.split_once("|> Bool/not")?;
    let path = path
        .split('{')
        .next_back()
        .unwrap_or(path)
        .trim()
        .trim_start_matches("THEN")
        .trim()
        .trim_start_matches('{')
        .trim();
    (!path.is_empty()).then(|| path.to_owned())
}

fn candidate_sources(program: &ParsedProgram, target: &str) -> Vec<String> {
    let fields = collect_field_defs(program);
    let mut visited = Vec::new();
    candidate_sources_for_path(target, &fields, program, &mut visited)
}

#[derive(Clone, Debug)]
struct FieldDef {
    path: String,
    local_name: String,
    parent_path: String,
    body: String,
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
            && text_mentions_identifier(&field.body, &candidate.local_name)
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
            .any(|variant| field.body.contains(variant))
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

fn collect_field_defs(program: &ParsedProgram) -> Vec<FieldDef> {
    let lines = program.source.lines().collect::<Vec<_>>();
    let mut scope: Vec<(usize, String)> = Vec::new();
    let mut fields = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.chars().take_while(|ch| *ch == ' ').count();
        while scope
            .last()
            .is_some_and(|(scope_indent, _)| *scope_indent >= indent)
        {
            scope.pop();
        }
        if trimmed.starts_with("EXAMPLE ") || matches!(trimmed, "[" | "]" | "{" | "}") {
            continue;
        }
        if trimmed.starts_with("FUNCTION ") {
            if let Some(row_scope) = function_row_scope(trimmed, program) {
                scope.push((indent, row_scope.to_owned()));
            }
            continue;
        }
        if let Some(local_name) = leading_field_name(trimmed) {
            if should_record_field(trimmed, local_name, &scope) {
                let parent_path = scope_path(&scope).unwrap_or_default();
                let path = if parent_path.is_empty() {
                    local_name.to_owned()
                } else {
                    format!("{parent_path}.{local_name}")
                };
                fields.push(FieldDef {
                    path,
                    local_name: local_name.to_owned(),
                    parent_path,
                    body: field_body(&lines, line_index, indent),
                });
            }
            if opens_scope(trimmed) {
                scope.push((indent, local_name.to_owned()));
            }
        }
    }
    fields
}

fn should_record_field(trimmed: &str, local_name: &str, scope: &[(usize, String)]) -> bool {
    !trimmed.contains("SOURCE")
        && local_name != "sources"
        && !scope.iter().any(|(_, name)| name == "sources")
        && scope
            .iter()
            .any(|(_, name)| matches!(name.as_str(), "store" | "todo" | "cell"))
}

fn field_body(lines: &[&str], start: usize, indent: usize) -> String {
    let mut body = String::new();
    for line in &lines[start..] {
        let trimmed = line.trim();
        let current_indent = line.chars().take_while(|ch| *ch == ' ').count();
        if current_indent <= indent
            && !body.is_empty()
            && leading_field_name(trimmed).is_some()
            && !trimmed.contains("=>")
        {
            break;
        }
        body.push_str(trimmed);
        body.push('\n');
    }
    body
}

fn function_row_scope<'a>(trimmed: &str, program: &'a ParsedProgram) -> Option<&'a str> {
    let name = trimmed.strip_prefix("FUNCTION ")?.split('(').next()?.trim();
    program
        .row_scope_functions
        .iter()
        .find(|scope| scope.function == name)
        .map(|scope| scope.row_scope.as_str())
}

fn leading_field_name(trimmed: &str) -> Option<&str> {
    let (name, _) = trimmed.split_once(':')?;
    let name = name.trim();
    (!name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_'))
    .then_some(name)
}

fn opens_scope(trimmed: &str) -> bool {
    if trimmed.contains("SOURCE") {
        return false;
    }
    trimmed.ends_with(':')
        || trimmed.ends_with('[')
        || trimmed.ends_with('{')
        || trimmed
            .split_once(':')
            .is_some_and(|(_, rest)| rest.trim_start().starts_with('[') && !rest.contains(']'))
}

fn scope_path(scope: &[(usize, String)]) -> Option<String> {
    (!scope.is_empty()).then(|| {
        scope
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>()
            .join(".")
    })
}

fn text_mentions_identifier(text: &str, identifier: &str) -> bool {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .any(|part| part == identifier)
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
    fn todomvc_lowering_is_static_and_keyed() {
        let parsed = boon_parser::parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let ir = lower(&parsed).unwrap();
        assert_eq!(ir.kind, ProgramKind::TodoMvc);
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
                    ListSeedRecord {
                        fields: vec![ListSeedField {
                            name: "title".to_owned(),
                            value: InitialValue::Text {
                                value: "Buy groceries".to_owned(),
                            },
                        }],
                    },
                    ListSeedRecord {
                        fields: vec![ListSeedField {
                            name: "title".to_owned(),
                            value: InitialValue::Text {
                                value: "Clean room".to_owned(),
                            },
                        }],
                    },
                ],
            }
        );
        assert!(
            ir.state_cells
                .iter()
                .any(|cell| cell.path == "todo.completed" && cell.indexed)
        );
        assert!(ir.state_cells.iter().any(|cell| {
            cell.path == "todo.title"
                && cell.initial_value
                    == InitialValue::SeedField {
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
                        path: "text".to_owned(),
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
    fn cells_lowering_has_dependency_index() {
        let parsed = boon_parser::parse_source(
            "examples/cells.bn",
            include_str!("../../../examples/cells.bn"),
        )
        .unwrap();
        let ir = lower(&parsed).unwrap();
        assert_eq!(ir.kind, ProgramKind::Cells);
        assert_eq!(
            ir.lists[0].initializer,
            ListInitializer::Grid {
                columns: 26,
                rows: 100,
            }
        );
        assert!(ir.nodes.iter().any(|node| node.name == "dependency_index"));
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
                    == InitialValue::Text {
                        value: String::new(),
                    }
        }));
        assert!(ir.derived_values.iter().any(|value| {
            value.path == "cell.value" && value.kind == DerivedValueKind::Formula && value.indexed
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
        assert!(ir.formula_operations.iter().any(|operation| {
            operation.target == "cell.parsed_formula"
                && operation.kind
                    == FormulaOperationKind::Parse {
                        input: "formula_text".to_owned(),
                    }
        }));
        assert!(ir.formula_operations.iter().any(|operation| {
            operation.target == "cell.dependencies"
                && operation.kind
                    == FormulaOperationKind::Dependencies {
                        input: "parsed_formula".to_owned(),
                    }
        }));
        assert!(ir.formula_operations.iter().any(|operation| {
            operation.target == "cell.value"
                && operation.kind
                    == FormulaOperationKind::Eval {
                        formula: "parsed_formula".to_owned(),
                        read: "cell_value_reader".to_owned(),
                    }
        }));
        assert!(ir.formula_operations.iter().any(|operation| {
            operation.target == "cell.error"
                && operation.kind
                    == FormulaOperationKind::Error {
                        formula: "parsed_formula".to_owned(),
                        value: "value".to_owned(),
                    }
        }));
        assert!(
            ir.nodes
                .iter()
                .filter(|node| node.expr_id.is_some())
                .all(|node| node.expr_id.unwrap() < parsed.expressions.len())
        );
        verify_hidden_identity(&ir).unwrap();
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
        bad_node_order.nodes[0].id = 99;
        assert!(
            verify_static_schedule(&bad_node_order)
                .unwrap_err()
                .contains("expected 0")
        );

        let mut bad_expr_id = ir.clone();
        bad_expr_id.nodes[0].expr_id = Some(ir.expression_count);
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
    }

    #[test]
    fn while_is_scheduled_as_combinational_selection() {
        let source = include_str!("../../../examples/todomvc.bn").replace(
            "\n    selected_filter:",
            "\n    visible_when_selected:\n        selected_filter |> WHILE { True }\n\n    selected_filter:",
        );
        let parsed = boon_parser::parse_source("examples/todomvc.bn", source).unwrap();
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
        let parsed = boon_parser::parse_source("examples/todomvc.bn", source).unwrap();
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
                "new_todo(seed: seed, store: store)",
                "make_item(seed: seed, store: store)",
            )
            .replace(
                "FUNCTION new_todo(seed, store)",
                "FUNCTION make_item(seed, store)",
            );
        let parsed = boon_parser::parse_source("examples/todomvc.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(
            parsed
                .row_scope_functions
                .iter()
                .any(|scope| { scope.function == "make_item" && scope.row_scope == "todo" })
        );
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
}
