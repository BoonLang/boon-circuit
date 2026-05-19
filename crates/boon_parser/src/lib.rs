use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProgramKind {
    TodoMvc,
    Cells,
}

impl ProgramKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TodoMvc => "todomvc",
            Self::Cells => "cells",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParsedProgram {
    pub path: String,
    pub source: String,
    pub kind: ProgramKind,
    pub expressions: Vec<ParsedExpression>,
    pub sources: Vec<String>,
    pub source_ports: Vec<ParsedSourcePort>,
    pub holds: Vec<String>,
    pub state_cells: Vec<ParsedStateCell>,
    pub lists: Vec<String>,
    pub list_memories: Vec<ParsedListMemory>,
    pub row_scope_functions: Vec<ParsedRowScopeFunction>,
    pub functions: Vec<String>,
    pub operators: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParsedExpression {
    pub id: usize,
    pub line: usize,
    pub kind: ParsedExpressionKind,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParsedSourcePort {
    pub path: String,
    pub line: usize,
    pub scoped: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParsedStateCell {
    pub path: String,
    pub hold_name: String,
    pub line: usize,
    pub indexed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParsedListMemory {
    pub name: String,
    pub line: usize,
    pub capacity: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParsedRowScopeFunction {
    pub function: String,
    pub row_scope: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ParsedExpressionKind {
    Source,
    Hold,
    List,
    Function,
    Operator,
    Field,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub path: String,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for ParseError {}

pub fn parse_source(
    path: impl Into<String>,
    source: impl Into<String>,
) -> Result<ParsedProgram, ParseError> {
    let path = path.into();
    let source = source.into();
    let kind = detect_kind(&path, &source)?;
    validate_balanced_brackets(&path, &source)?;
    validate_required_constructs(&path, &source)?;
    validate_no_reducer_style_update(&path, &source)?;
    validate_no_hidden_identity_leak(&path, &source, kind)?;
    let row_scope_functions = collect_row_scope_functions(&source);
    let structure = collect_structure(&source, &row_scope_functions);
    Ok(ParsedProgram {
        expressions: collect_expressions(&source),
        sources: collect_sources(&source),
        source_ports: structure.source_ports,
        holds: collect_named_lines(&source, "HOLD"),
        state_cells: structure.state_cells,
        lists: collect_named_lines(&source, "LIST"),
        list_memories: structure.list_memories,
        row_scope_functions,
        functions: collect_functions(&source),
        operators: collect_operators(&source),
        path,
        source,
        kind,
    })
}

fn detect_kind(path: &str, source: &str) -> Result<ProgramKind, ParseError> {
    if source.contains("EXAMPLE TodoMVC") {
        Ok(ProgramKind::TodoMvc)
    } else if source.contains("EXAMPLE Cells") {
        Ok(ProgramKind::Cells)
    } else if path.contains("todomvc") {
        Ok(ProgramKind::TodoMvc)
    } else if path.contains("cells") {
        Ok(ProgramKind::Cells)
    } else {
        Err(ParseError {
            path: path.to_owned(),
            message: "missing `EXAMPLE TodoMVC` or `EXAMPLE Cells` marker".to_owned(),
        })
    }
}

fn validate_balanced_brackets(path: &str, source: &str) -> Result<(), ParseError> {
    let mut stack = Vec::new();
    for (line_index, ch) in source.chars().enumerate() {
        match ch {
            '[' | '{' | '(' => stack.push(ch),
            ']' => {
                if stack.pop() != Some('[') {
                    return Err(error(path, line_index, "unbalanced `]`"));
                }
            }
            '}' => {
                if stack.pop() != Some('{') {
                    return Err(error(path, line_index, "unbalanced `}`"));
                }
            }
            ')' => {
                if stack.pop() != Some('(') {
                    return Err(error(path, line_index, "unbalanced `)`"));
                }
            }
            _ => {}
        }
    }
    if stack.is_empty() {
        Ok(())
    } else {
        Err(ParseError {
            path: path.to_owned(),
            message: "unclosed bracket/brace/paren".to_owned(),
        })
    }
}

fn validate_required_constructs(path: &str, source: &str) -> Result<(), ParseError> {
    for required in ["SOURCE", "HOLD", "LATEST", "List/map"] {
        if !source.contains(required) {
            return Err(ParseError {
                path: path.to_owned(),
                message: format!("required construct `{required}` is missing"),
            });
        }
    }
    if !source.contains("LIST") && !source.contains("Grid/cells") {
        return Err(ParseError {
            path: path.to_owned(),
            message: "required list source `LIST` or `Grid/cells` is missing".to_owned(),
        });
    }
    Ok(())
}

fn validate_no_reducer_style_update(path: &str, source: &str) -> Result<(), ParseError> {
    if source
        .lines()
        .any(|line| reducer_update_signature(line.trim()))
    {
        return Err(ParseError {
            path: path.to_owned(),
            message: "central reducer `FUNCTION update(state, event)` is not allowed; define local HOLD equations for each value".to_owned(),
        });
    }
    if source.contains("event.source |> WHEN") && source.contains("state |>") {
        return Err(ParseError {
            path: path.to_owned(),
            message: "global event-source reducer over `state` is not allowed; each value must declare its own sources".to_owned(),
        });
    }
    Ok(())
}

fn reducer_update_signature(trimmed: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix("FUNCTION update(") else {
        return false;
    };
    let Some(args) = rest.split(')').next() else {
        return false;
    };
    let args = args.split(',').map(str::trim).collect::<Vec<_>>();
    args.contains(&"state") && args.contains(&"event")
}

fn validate_no_hidden_identity_leak(
    path: &str,
    source: &str,
    kind: ProgramKind,
) -> Result<(), ParseError> {
    let forbidden = [
        "runtime_key",
        "item_key",
        "ListKey",
        "Option[ListKey",
        "TodoId",
        "selected_todo_id",
        "next_todo_id",
        "generation:",
        "source_id:",
    ];
    for needle in forbidden {
        if source.contains(needle) {
            return Err(ParseError {
                path: path.to_owned(),
                message: format!("Boon source exposes hidden runtime identity `{needle}`"),
            });
        }
    }
    if matches!(kind, ProgramKind::TodoMvc) {
        for (line, text) in source.lines().enumerate() {
            let trimmed = text.trim_start();
            if trimmed.starts_with("id:") {
                return Err(ParseError {
                    path: path.to_owned(),
                    message: format!(
                        "TodoMVC must not expose app-visible `id` at line {}",
                        line + 1
                    ),
                });
            }
            if trimmed.starts_with("alive:") {
                return Err(ParseError {
                    path: path.to_owned(),
                    message: format!(
                        "TodoMVC must use list removal, not app-visible `alive`, at line {}",
                        line + 1
                    ),
                });
            }
        }
    }
    Ok(())
}

fn collect_sources(source: &str) -> Vec<String> {
    source
        .lines()
        .filter(|line| line.contains("SOURCE"))
        .map(|line| line.trim().trim_end_matches(',').to_owned())
        .collect()
}

fn collect_expressions(source: &str) -> Vec<ParsedExpression> {
    source
        .lines()
        .enumerate()
        .filter_map(|(line, text)| {
            let trimmed = text.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let kind = if trimmed.contains("SOURCE") {
                ParsedExpressionKind::Source
            } else if trimmed.contains("HOLD") {
                ParsedExpressionKind::Hold
            } else if trimmed.contains("LIST") || trimmed.contains("Grid/cells") {
                ParsedExpressionKind::List
            } else if trimmed.starts_with("FUNCTION ") {
                ParsedExpressionKind::Function
            } else if [
                "THEN",
                "WHEN",
                "WHILE",
                "LATEST",
                "List/map",
                "List/append",
                "List/remove",
                "List/retain",
                "List/count",
            ]
            .iter()
            .any(|operator| trimmed.contains(operator))
            {
                ParsedExpressionKind::Operator
            } else if trimmed.ends_with(':') || trimmed.contains(": ") {
                ParsedExpressionKind::Field
            } else {
                return None;
            };
            Some((line, kind, trimmed.to_owned()))
        })
        .enumerate()
        .map(|(id, (line, kind, text))| ParsedExpression {
            id,
            line: line + 1,
            kind,
            text,
        })
        .collect()
}

fn collect_named_lines(source: &str, needle: &str) -> Vec<String> {
    source
        .lines()
        .filter(|line| line.contains(needle))
        .map(|line| line.trim().to_owned())
        .collect()
}

#[derive(Default)]
struct StructureTables {
    source_ports: Vec<ParsedSourcePort>,
    state_cells: Vec<ParsedStateCell>,
    list_memories: Vec<ParsedListMemory>,
}

fn collect_structure(source: &str, row_scopes: &[ParsedRowScopeFunction]) -> StructureTables {
    let mut tables = StructureTables::default();
    let mut scope: Vec<(usize, String)> = Vec::new();
    for (line_index, line) in source.lines().enumerate() {
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
            if let Some(row_scope) = function_name(trimmed)
                .and_then(|function| row_scope_for_function(&row_scopes, function))
            {
                scope.push((indent, row_scope.to_owned()));
            }
            continue;
        }
        if trimmed.contains("SOURCE") {
            collect_source_line(&scope, trimmed, line_index + 1, &mut tables.source_ports);
        }
        if trimmed.contains("HOLD") {
            let path = scope_path(&scope).unwrap_or_else(|| format!("hold_{}", line_index + 1));
            let hold_name = hold_name(trimmed).unwrap_or_else(|| path.clone());
            tables.state_cells.push(ParsedStateCell {
                indexed: path_has_indexed_scope(&path),
                path,
                hold_name,
                line: line_index + 1,
            });
        }
        if trimmed.contains("LIST") || trimmed.contains("Grid/cells") {
            let name = scope
                .last()
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| format!("list_{}", tables.list_memories.len()));
            tables.list_memories.push(ParsedListMemory {
                name,
                line: line_index + 1,
                capacity: list_capacity(trimmed),
            });
        }
        if let Some(field) = leading_field_name(trimmed)
            && opens_scope(trimmed)
        {
            scope.push((indent, field.to_owned()));
        }
    }
    tables
}

fn collect_row_scope_functions(source: &str) -> Vec<ParsedRowScopeFunction> {
    let mut functions = Vec::new();
    let mut scope: Vec<(usize, String)> = Vec::new();
    for line in source.lines() {
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
        if let Some(function) = map_function_name(trimmed)
            && let Some((_, list_name)) = scope.last()
        {
            functions.push(ParsedRowScopeFunction {
                function: function.to_owned(),
                row_scope: singular_row_scope(list_name),
            });
        }
        if trimmed.starts_with("EXAMPLE ") || matches!(trimmed, "[" | "]" | "{" | "}") {
            continue;
        }
        if trimmed.starts_with("FUNCTION ") {
            continue;
        }
        if let Some(field) = leading_field_name(trimmed)
            && opens_scope(trimmed)
        {
            scope.push((indent, field.to_owned()));
        }
    }
    functions
}

fn row_scope_for_function<'a>(
    row_scopes: &'a [ParsedRowScopeFunction],
    function: &str,
) -> Option<&'a str> {
    row_scopes
        .iter()
        .find(|scope| scope.function == function)
        .map(|scope| scope.row_scope.as_str())
}

fn collect_source_line(
    scope: &[(usize, String)],
    trimmed: &str,
    line: usize,
    ports: &mut Vec<ParsedSourcePort>,
) {
    let Some(field) = leading_field_name(trimmed) else {
        return;
    };
    if let Some(event) = inline_source_event(trimmed) {
        ports.push(ParsedSourcePort {
            path: join_path(scope, [field, event]),
            line,
            scoped: source_scope_is_scoped(scope),
        });
    } else {
        ports.push(ParsedSourcePort {
            path: join_path(scope, [field]),
            line,
            scoped: source_scope_is_scoped(scope),
        });
    }
}

fn join_path<'a>(scope: &[(usize, String)], tail: impl IntoIterator<Item = &'a str>) -> String {
    let mut path = String::new();
    for (_, name) in scope {
        if !path.is_empty() {
            path.push('.');
        }
        path.push_str(name);
    }
    for item in tail {
        if !path.is_empty() {
            path.push('.');
        }
        path.push_str(item);
    }
    path
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

fn source_scope_is_scoped(scope: &[(usize, String)]) -> bool {
    scope
        .iter()
        .any(|(_, name)| matches!(name.as_str(), "todo" | "cell" | "seed"))
}

fn function_name(trimmed: &str) -> Option<&str> {
    trimmed
        .strip_prefix("FUNCTION ")?
        .split('(')
        .next()
        .map(str::trim)
        .filter(|name| !name.is_empty())
}

fn map_function_name(trimmed: &str) -> Option<&str> {
    let (_, rest) = trimmed.split_once("List/map")?;
    let (_, after_new) = rest.split_once("new:")?;
    after_new
        .trim_start()
        .split('(')
        .next()
        .map(str::trim)
        .filter(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        })
}

fn singular_row_scope(list_name: &str) -> String {
    list_name
        .strip_suffix("ies")
        .map(|prefix| format!("{prefix}y"))
        .or_else(|| list_name.strip_suffix('s').map(str::to_owned))
        .unwrap_or_else(|| format!("{list_name}_item"))
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

fn inline_source_event(trimmed: &str) -> Option<&str> {
    let (_, rest) = trimmed.split_once('[')?;
    let event = rest.split_once(':')?.0.trim();
    (!event.is_empty()
        && event
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_'))
    .then_some(event)
}

fn hold_name(trimmed: &str) -> Option<String> {
    let (_, rest) = trimmed.split_once("|> HOLD ")?;
    let name = rest
        .split(|ch: char| ch.is_whitespace() || ch == '{' || ch == '(')
        .next()?
        .trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn list_capacity(trimmed: &str) -> Option<usize> {
    let (_, rest) = trimmed.split_once("LIST[")?;
    rest.split_once(']')?.0.trim().parse().ok()
}

fn path_has_indexed_scope(path: &str) -> bool {
    path.split('.')
        .any(|segment| matches!(segment, "todo" | "cell" | "seed"))
}

fn collect_functions(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("FUNCTION ")
                .and_then(|rest| rest.split('(').next())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(ToOwned::to_owned)
        })
        .collect()
}

fn collect_operators(source: &str) -> Vec<String> {
    let mut operators = Vec::new();
    for name in [
        "SOURCE",
        "HOLD",
        "THEN",
        "WHEN",
        "WHILE",
        "LATEST",
        "LIST",
        "List/map",
        "List/append",
        "List/remove",
        "List/retain",
        "List/count",
    ] {
        if source.contains(name) {
            operators.push(name.to_owned());
        }
    }
    operators
}

fn error(path: &str, char_index: usize, message: &str) -> ParseError {
    ParseError {
        path: path.to_owned(),
        message: format!("{message} near char {char_index}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_todomvc_marker_and_constructs() {
        let source = include_str!("../../../examples/todomvc.bn");
        let program = parse_source("examples/todomvc.bn", source).unwrap();
        assert_eq!(program.kind, ProgramKind::TodoMvc);
        assert!(
            program
                .expressions
                .iter()
                .any(|expr| expr.kind == ParsedExpressionKind::Hold)
        );
        assert!(program.operators.contains(&"List/remove".to_owned()));
        assert!(program.functions.contains(&"new_todo".to_owned()));
        assert!(
            program
                .source_ports
                .iter()
                .any(|port| port.path == "store.sources.new_todo_input.change")
        );
        assert!(
            program
                .source_ports
                .iter()
                .any(|port| port.path == "store.sources.new_todo_input.change" && !port.scoped)
        );
        assert!(
            program
                .state_cells
                .iter()
                .any(|cell| cell.path == "store.new_todo_text" && cell.hold_name == "text")
        );
        assert!(
            program
                .state_cells
                .iter()
                .any(|cell| cell.path == "store.new_todo_text" && !cell.indexed)
        );
        assert!(
            program
                .state_cells
                .iter()
                .any(|cell| cell.path == "todo.completed" && cell.indexed)
        );
        assert!(
            program
                .source_ports
                .iter()
                .any(|port| port.path == "todo.sources.todo_checkbox.click" && port.scoped)
        );
        assert!(
            program
                .list_memories
                .iter()
                .any(|list| list.name == "todos")
        );
        assert!(
            program
                .row_scope_functions
                .iter()
                .any(|scope| { scope.function == "new_todo" && scope.row_scope == "todo" })
        );
    }

    #[test]
    fn row_template_scope_comes_from_list_map_not_function_name() {
        let source = include_str!("../../../examples/todomvc.bn").replace("new_todo", "make_item");
        let program = parse_source("examples/todomvc.bn", source).unwrap();
        assert!(program.functions.contains(&"make_item".to_owned()));
        assert!(
            program
                .row_scope_functions
                .iter()
                .any(|scope| { scope.function == "make_item" && scope.row_scope == "todo" })
        );
        assert!(
            program
                .source_ports
                .iter()
                .any(|port| port.path == "todo.sources.todo_checkbox.click" && port.scoped)
        );
        assert!(
            program
                .state_cells
                .iter()
                .any(|cell| cell.path == "todo.completed" && cell.indexed)
        );
    }

    #[test]
    fn parses_cells_marker_and_constructs() {
        let source = include_str!("../../../examples/cells.bn");
        let program = parse_source("examples/cells.bn", source).unwrap();
        assert_eq!(program.kind, ProgramKind::Cells);
        assert!(
            program
                .expressions
                .iter()
                .any(|expr| expr.kind == ParsedExpressionKind::Source)
        );
        assert!(program.functions.contains(&"new_cell".to_owned()));
        assert!(
            program
                .source_ports
                .iter()
                .any(|port| port.path == "cell.sources.editor.commit")
        );
        assert!(
            program
                .state_cells
                .iter()
                .any(|cell| cell.path == "cell.formula_text" && cell.indexed)
        );
        assert!(
            program
                .list_memories
                .iter()
                .any(|list| list.name == "cells")
        );
    }

    #[test]
    fn rejects_hidden_todo_id() {
        let source = "EXAMPLE TodoMVC\nLIST {}\nid: TodoId[id: Ulid/generate()]\nSOURCE\nHOLD\nLATEST\nList/map";
        let err = parse_source("bad.bn", source).unwrap_err();
        assert!(err.message.contains("hidden runtime identity") || err.message.contains("id"));
    }

    #[test]
    fn rejects_global_reducer_update_shape() {
        let source = r#"
EXAMPLE TodoMVC
FUNCTION update(state, event) {
    event.source |> WHEN {
        ToggleTodo => state |> TodoTable/update(completed: True)
    }
}
items: LIST {}
click: SOURCE
value: False |> HOLD value { LATEST { click |> THEN { True } } }
items |> List/map(item, new: new_item(seed: item))
"#;
        let err = parse_source("bad-reducer.bn", source).unwrap_err();
        assert!(err.message.contains("central reducer"));
    }
}
