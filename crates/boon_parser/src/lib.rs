use chumsky::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProgramKind {
    Generic,
}

impl ProgramKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParsedProgram {
    pub path: String,
    pub source: String,
    pub files: Vec<ParsedSourceFile>,
    pub kind: ProgramKind,
    pub ast: AstProgram,
    pub expressions: Vec<AstExpr>,
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
pub struct ParsedSourceFile {
    pub path: String,
    pub source: String,
    pub start_line: usize,
    pub module: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstProgram {
    pub tokens: Vec<AstToken>,
    pub lines: Vec<ParserLine>,
    pub items: Vec<ParserItem>,
    pub statements: Vec<AstStatement>,
    pub expressions: Vec<AstExpr>,
}

impl AstProgram {
    pub fn semantic_tokens(&self) -> impl Iterator<Item = &AstToken> {
        self.tokens.iter().filter(|token| {
            !matches!(token.kind, AstTokenKind::Comment | AstTokenKind::String)
                && !self.line_is_document(token.line)
        })
    }

    pub fn semantic_parser_lines(&self) -> impl Iterator<Item = &ParserLine> {
        self.lines
            .iter()
            .filter(|line| !line.symbols.is_empty() && !self.line_is_document(line.line))
    }

    pub fn semantic_parser_items(&self) -> impl Iterator<Item = &ParserItem> {
        self.items
            .iter()
            .filter(|item| !self.line_is_document(item.line))
    }

    fn line_is_document(&self, line: usize) -> bool {
        self.statements
            .iter()
            .find(|statement| {
                matches!(
                    &statement.kind,
                    AstStatementKind::Field { name } if name == "document"
                )
            })
            .is_some_and(|statement| statement_contains_line(statement, line))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParserLine {
    pub line: usize,
    pub indent: usize,
    pub symbols: Vec<String>,
    pub symbol_spans: Vec<(usize, usize)>,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParserItem {
    pub line: usize,
    pub indent: usize,
    pub start: usize,
    pub end: usize,
    pub symbols: Vec<String>,
    pub symbol_spans: Vec<(usize, usize)>,
    pub field: Option<String>,
    pub example: Option<String>,
    pub function: Option<String>,
    pub map_new_function: Option<String>,
    pub source_event: Option<String>,
    pub hold: Option<String>,
    pub list_capacity: Option<usize>,
    pub is_list: bool,
    pub opens_scope: bool,
    pub closes_scope: bool,
    pub operators: Vec<String>,
}

impl ParserItem {
    pub fn has_lexeme(&self, lexeme: &str) -> bool {
        self.symbols.iter().any(|candidate| candidate == lexeme)
    }

    pub fn contains_sequence(&self, sequence: &[&str]) -> bool {
        if sequence.is_empty() {
            return true;
        }
        self.symbols.windows(sequence.len()).any(|window| {
            window
                .iter()
                .map(String::as_str)
                .eq(sequence.iter().copied())
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstStatement {
    pub id: usize,
    pub line: usize,
    pub indent: usize,
    pub start: usize,
    pub end: usize,
    pub kind: AstStatementKind,
    pub expr: Option<usize>,
    pub children: Vec<AstStatement>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AstStatementKind {
    Function {
        name: String,
        args: Vec<String>,
    },
    Field {
        name: String,
    },
    Source {
        field: Option<String>,
        event: Option<String>,
    },
    Hold {
        field: Option<String>,
        name: Option<String>,
    },
    List {
        field: Option<String>,
        capacity: Option<usize>,
    },
    Block,
    Spread,
    Expression,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstExpr {
    pub id: usize,
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub kind: AstExprKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AstExprKind {
    Identifier(String),
    Path(Vec<String>),
    Drain {
        path: AstDrainPath,
    },
    StringLiteral(String),
    TextLiteral(String),
    Number(String),
    ByteLiteral {
        radix: u8,
        digits: String,
        value: u8,
    },
    Bool(bool),
    Enum(String),
    Tag(String),
    TaggedObject {
        tag: String,
        fields: Vec<AstRecordField>,
    },
    Source,
    Call {
        function: String,
        args: Vec<AstCallArg>,
    },
    Pipe {
        input: usize,
        op: String,
        args: Vec<AstCallArg>,
    },
    Draining {
        input: usize,
    },
    Hold {
        initial: usize,
        name: String,
    },
    Latest,
    When {
        input: usize,
    },
    Then {
        input: usize,
        output: Option<usize>,
    },
    Infix {
        left: usize,
        op: String,
        right: usize,
    },
    MatchArm {
        pattern: Vec<String>,
        output: Option<usize>,
    },
    Object(Vec<AstRecordField>),
    Record(Vec<AstRecordField>),
    ListLiteral {
        capacity: Option<usize>,
        #[serde(default)]
        items: Vec<usize>,
    },
    BytesLiteral {
        size: BytesSizeSyntax,
        #[serde(default)]
        items: Vec<usize>,
    },
    Delimiter,
    Unknown(Vec<String>),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AstDrainPath {
    Binding {
        name: String,
    },
    Field {
        binding: String,
        fields: Vec<String>,
    },
    Passed {
        fields: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BytesSizeSyntax {
    Dynamic,
    Infer,
    Fixed(usize),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstCallArg {
    pub name: Option<String>,
    pub value: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstRecordField {
    pub name: String,
    pub value: usize,
    pub start: usize,
    pub end: usize,
    #[serde(default)]
    pub spread: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AstToken {
    pub kind: AstTokenKind,
    pub lexeme: String,
    pub line: usize,
    pub column: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AstTokenKind {
    Identifier,
    Number,
    String,
    Comment,
    Operator,
    Symbol,
    Newline,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ParsedSourcePort {
    pub path: String,
    pub line: usize,
    pub scoped: bool,
    pub interval_ms: Option<u64>,
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
    pub list: String,
    pub row_scope: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DocumentAst {
    pub root: AstStatement,
    pub expressions: Vec<AstExpr>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub path: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
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
    let files = vec![ParsedSourceFile {
        path: path.clone(),
        source: source.clone(),
        start_line: 1,
        module: None,
    }];
    parse_combined_source(path, source, files)
}

pub fn parse_project(
    path: impl Into<String>,
    files: impl IntoIterator<Item = (String, String)>,
) -> Result<ParsedProgram, ParseError> {
    let path = path.into();
    let mut parsed_files = Vec::new();
    let mut source = String::new();
    let mut next_line = 1usize;
    for (file_path, file_source) in files {
        if !source.is_empty() && !source.ends_with('\n') {
            source.push('\n');
            next_line += 1;
        }
        let start_line = next_line;
        source.push_str(&file_source);
        if !file_source.ends_with('\n') {
            source.push('\n');
        }
        next_line += file_source.lines().count().max(1);
        parsed_files.push(ParsedSourceFile {
            module: module_name_for_project_file(&path, &file_path),
            path: file_path,
            source: file_source,
            start_line,
        });
    }
    if parsed_files.is_empty() {
        return Err(ParseError {
            path,
            line: None,
            column: None,
            message: "project has no source files".to_owned(),
        });
    }
    parse_combined_source(path, source, parsed_files)
}

fn parse_combined_source(
    path: String,
    source: String,
    files: Vec<ParsedSourceFile>,
) -> Result<ParsedProgram, ParseError> {
    let mut ast = parse_ast(&path, &source)?;
    namespace_project_modules(&mut ast, &files);
    validate_source_syntax(&path, &ast)?;
    validate_balanced_brackets(&path, &ast)?;
    validate_list_capacities(&path, &ast)?;
    validate_no_reducer_style_update(&path, &ast)?;
    let kind = detect_program_kind();
    validate_no_hidden_identity_leak(&path, &ast)?;
    let list_memory_names = collect_list_memory_names(&ast);
    let source_row_scope_functions = collect_row_scope_functions(&ast, true, &list_memory_names);
    let mut row_scope_functions = collect_row_scope_functions(&ast, false, &list_memory_names);
    let mut structure_row_scope_functions = row_scope_functions.clone();
    for source_scope in &source_row_scope_functions {
        if !structure_row_scope_functions.iter().any(|existing| {
            existing.list == source_scope.list
                && existing.function == source_scope.function
                && existing.row_scope == source_scope.row_scope
        }) {
            structure_row_scope_functions.push(source_scope.clone());
        }
    }
    for source_scope in &source_row_scope_functions {
        if !row_scope_functions.iter().any(|existing| {
            existing.list == source_scope.list && existing.row_scope == source_scope.row_scope
        }) {
            row_scope_functions.push(ParsedRowScopeFunction {
                function: format!("__source_row_scope_{}", source_scope.function),
                list: source_scope.list.clone(),
                row_scope: source_scope.row_scope.clone(),
            });
        }
    }
    let structure = derive_program_tables(&ast, &structure_row_scope_functions);
    Ok(ParsedProgram {
        expressions: ast.expressions.clone(),
        sources: collect_sources(&ast),
        source_ports: structure.source_ports,
        holds: collect_named_statements(&ast, "HOLD"),
        state_cells: structure.state_cells,
        lists: collect_named_statements(&ast, "LIST"),
        list_memories: structure.list_memories,
        row_scope_functions,
        functions: collect_functions(&ast),
        operators: collect_operators(&ast),
        path,
        source,
        files,
        kind,
        ast,
    })
}

fn module_name_for_project_file(entry_path: &str, file_path: &str) -> Option<String> {
    if entry_path == file_path {
        return None;
    }
    let stem = std::path::Path::new(file_path)
        .file_stem()
        .and_then(|stem| stem.to_str())?;
    if stem.chars().next().is_some_and(char::is_uppercase) {
        Some(stem.to_owned())
    } else {
        None
    }
}

fn namespace_project_modules(ast: &mut AstProgram, files: &[ParsedSourceFile]) {
    let ranges = files
        .iter()
        .filter_map(|file| {
            let module = file.module.as_ref()?;
            let line_count = file.source.lines().count().max(1);
            Some((
                file.start_line..file.start_line + line_count,
                module.clone(),
            ))
        })
        .collect::<Vec<_>>();
    if ranges.is_empty() {
        return;
    }
    let mut functions_by_module = std::collections::BTreeMap::<String, Vec<String>>::new();
    collect_module_functions(&ast.statements, &ranges, &mut functions_by_module);
    namespace_statement_functions(&mut ast.statements, &ranges, &functions_by_module);
    namespace_expr_functions(&mut ast.expressions, &ranges, &functions_by_module);
    namespace_parser_items(&mut ast.items, &ranges, &functions_by_module);
}

fn module_for_line(line: usize, ranges: &[(std::ops::Range<usize>, String)]) -> Option<&str> {
    ranges
        .iter()
        .find(|(range, _)| range.contains(&line))
        .map(|(_, module)| module.as_str())
}

fn collect_module_functions(
    statements: &[AstStatement],
    ranges: &[(std::ops::Range<usize>, String)],
    functions_by_module: &mut std::collections::BTreeMap<String, Vec<String>>,
) {
    for statement in statements {
        if let AstStatementKind::Function { name, .. } = &statement.kind
            && let Some(module) = module_for_line(statement.line, ranges)
        {
            functions_by_module
                .entry(module.to_owned())
                .or_default()
                .push(name.clone());
        }
        collect_module_functions(&statement.children, ranges, functions_by_module);
    }
}

fn module_function_name(
    module: &str,
    function: &str,
    functions_by_module: &std::collections::BTreeMap<String, Vec<String>>,
) -> Option<String> {
    if function.contains('/') {
        return None;
    }
    functions_by_module
        .get(module)
        .is_some_and(|functions| functions.iter().any(|name| name == function))
        .then(|| format!("{module}/{function}"))
}

fn namespace_statement_functions(
    statements: &mut [AstStatement],
    ranges: &[(std::ops::Range<usize>, String)],
    functions_by_module: &std::collections::BTreeMap<String, Vec<String>>,
) {
    for statement in statements {
        if let AstStatementKind::Function { name, .. } = &mut statement.kind
            && let Some(module) = module_for_line(statement.line, ranges)
            && !name.contains('/')
        {
            *name = format!("{module}/{name}");
        }
        namespace_statement_functions(&mut statement.children, ranges, functions_by_module);
        let _ = functions_by_module;
    }
}

fn namespace_expr_functions(
    expressions: &mut [AstExpr],
    ranges: &[(std::ops::Range<usize>, String)],
    functions_by_module: &std::collections::BTreeMap<String, Vec<String>>,
) {
    for expr in expressions {
        let Some(module) = module_for_line(expr.line, ranges) else {
            continue;
        };
        match &mut expr.kind {
            AstExprKind::Call { function, .. } => {
                if let Some(namespaced) =
                    module_function_name(module, function, functions_by_module)
                {
                    *function = namespaced;
                }
            }
            AstExprKind::Pipe { op, .. } => {
                if let Some(namespaced) = module_function_name(module, op, functions_by_module) {
                    *op = namespaced;
                }
            }
            _ => {}
        }
    }
}

fn namespace_parser_items(
    items: &mut [ParserItem],
    ranges: &[(std::ops::Range<usize>, String)],
    functions_by_module: &std::collections::BTreeMap<String, Vec<String>>,
) {
    for item in items {
        let Some(module) = module_for_line(item.line, ranges) else {
            continue;
        };
        if let Some(function) = &mut item.function
            && !function.contains('/')
        {
            *function = format!("{module}/{function}");
        }
        for operator in &mut item.operators {
            if let Some(namespaced) = module_function_name(module, operator, functions_by_module) {
                *operator = namespaced;
            }
        }
    }
}

pub fn parsed_document(program: &ParsedProgram) -> Option<DocumentAst> {
    document_statement(&program.ast)
        .cloned()
        .map(|root| DocumentAst {
            root,
            expressions: program.ast.expressions.clone(),
        })
}

pub fn parsed_scene(program: &ParsedProgram) -> Option<DocumentAst> {
    scene_statement(&program.ast)
        .cloned()
        .map(|root| DocumentAst {
            root,
            expressions: program.ast.expressions.clone(),
        })
}

pub fn format_source(
    path: impl Into<String>,
    source: impl Into<String>,
) -> Result<String, ParseError> {
    let path = path.into();
    let source = source.into();
    parse_source(path, source.clone())?;
    Ok(format_source_text(&source))
}

pub fn format_source_unit(
    path: impl Into<String>,
    source: impl Into<String>,
) -> Result<String, ParseError> {
    let path = path.into();
    let source = source.into();
    let ast = parse_ast(&path, &source)?;
    validate_source_syntax(&path, &ast)?;
    validate_balanced_brackets(&path, &ast)?;
    validate_list_capacities(&path, &ast)?;
    Ok(format_source_text(&source))
}

fn format_source_text(source: &str) -> String {
    let mut formatted_lines = Vec::new();
    let mut previous_blank = false;
    for line in source.lines() {
        let trimmed_end = line.trim_end();
        if trimmed_end.is_empty() {
            if !previous_blank {
                formatted_lines.push(String::new());
            }
            previous_blank = true;
            continue;
        }
        previous_blank = false;
        let content = trimmed_end.trim_start_matches([' ', '\t']);
        let raw_indent_columns = trimmed_end
            .chars()
            .take_while(|character| *character == ' ' || *character == '\t')
            .map(|character| if character == '\t' { 4 } else { 1 })
            .sum::<usize>();
        let indent_columns = if raw_indent_columns > 0 {
            // Parser-gated indentation normalization: every non-empty source line
            // keeps its block depth, but mixed/two-space indentation is rewritten to
            // the canonical four-column editor grid after the parser has accepted
            // the source.
            raw_indent_columns.div_ceil(4) * 4
        } else {
            raw_indent_columns
        };
        formatted_lines.push(format!("{}{}", " ".repeat(indent_columns), content));
    }
    formatted_lines = compact_format_bracket_blocks(formatted_lines);
    while formatted_lines.last().is_some_and(|line| line.is_empty()) {
        formatted_lines.pop();
    }
    let mut formatted = formatted_lines.join("\n");
    formatted.push('\n');
    formatted
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FormatNode {
    Blank,
    Line {
        indent: usize,
        content: String,
    },
    BracketBlock {
        indent: usize,
        prefix: String,
        children: Vec<FormatNode>,
    },
}

fn compact_format_bracket_blocks(lines: Vec<String>) -> Vec<String> {
    let mut index = 0;
    let nodes = parse_format_nodes(&lines, &mut index, None);
    let mut formatted = Vec::new();
    render_format_nodes(&nodes, &mut formatted);
    formatted
}

fn parse_format_nodes(
    lines: &[String],
    index: &mut usize,
    close_indent: Option<usize>,
) -> Vec<FormatNode> {
    let mut nodes = Vec::new();
    while *index < lines.len() {
        let line = &lines[*index];
        if line.is_empty() {
            nodes.push(FormatNode::Blank);
            *index += 1;
            continue;
        }
        let indent = line.chars().take_while(|ch| *ch == ' ').count();
        let content = line[indent..].to_owned();
        if close_indent == Some(indent) && content == "]" {
            *index += 1;
            break;
        }
        if let Some(prefix) = format_bracket_block_prefix(&content) {
            *index += 1;
            let children = parse_format_nodes(lines, index, Some(indent));
            nodes.push(FormatNode::BracketBlock {
                indent,
                prefix,
                children,
            });
        } else {
            nodes.push(FormatNode::Line { indent, content });
            *index += 1;
        }
    }
    nodes
}

fn format_bracket_block_prefix(content: &str) -> Option<String> {
    let prefix = content.strip_suffix('[')?.trim_end();
    if prefix.contains("--") {
        return None;
    }
    Some(prefix.to_owned())
}

fn render_format_nodes(nodes: &[FormatNode], output: &mut Vec<String>) {
    let nonblank = nodes
        .iter()
        .filter(|node| !matches!(node, FormatNode::Blank))
        .collect::<Vec<_>>();
    let object_of_objects = nonblank.len() > 1
        && nonblank
            .iter()
            .all(|node| matches!(node, FormatNode::BracketBlock { .. }));
    let mut previous_multiline = false;
    let mut rendered_any = false;
    for node in nodes {
        if matches!(node, FormatNode::Blank) {
            if !object_of_objects {
                push_format_blank(output);
            }
            continue;
        }
        let multiline = format_node_inline_text(node).is_none();
        if object_of_objects && rendered_any && (previous_multiline || multiline) {
            push_format_blank(output);
        }
        render_format_node(node, output);
        previous_multiline = multiline;
        rendered_any = true;
    }
}

fn render_format_node(node: &FormatNode, output: &mut Vec<String>) {
    match node {
        FormatNode::Blank => push_format_blank(output),
        FormatNode::Line { indent, content } => {
            output.push(format!("{}{}", " ".repeat(*indent), content));
        }
        FormatNode::BracketBlock {
            indent,
            prefix,
            children,
        } => {
            if let Some(inline) = format_node_inline_text(node) {
                output.push(format!("{}{}", " ".repeat(*indent), inline));
                return;
            }
            output.push(format!("{}{}", " ".repeat(*indent), bracket_open(prefix)));
            render_format_nodes(children, output);
            output.push(format!("{}]", " ".repeat(*indent)));
        }
    }
}

fn push_format_blank(output: &mut Vec<String>) {
    if output.last().is_some_and(|line| line.is_empty()) {
        return;
    }
    output.push(String::new());
}

fn format_node_inline_text(node: &FormatNode) -> Option<String> {
    const MAX_INLINE_CHARS: usize = 96;
    let text = format_node_inline_text_unbounded(node)?;
    (text.chars().count() <= MAX_INLINE_CHARS).then_some(text)
}

fn format_node_inline_text_unbounded(node: &FormatNode) -> Option<String> {
    match node {
        FormatNode::Blank => None,
        FormatNode::Line { content, .. } => {
            if content.starts_with("--") {
                None
            } else {
                Some(content.clone())
            }
        }
        FormatNode::BracketBlock {
            prefix, children, ..
        } => {
            let nonblank = children
                .iter()
                .filter(|child| !matches!(child, FormatNode::Blank))
                .collect::<Vec<_>>();
            match nonblank.as_slice() {
                [] => Some(bracket_inline(prefix, "")),
                [child] => {
                    let child = format_node_inline_text_unbounded(child)?;
                    Some(bracket_inline(prefix, &child))
                }
                _ => None,
            }
        }
    }
}

fn bracket_open(prefix: &str) -> String {
    if prefix.is_empty() {
        "[".to_owned()
    } else {
        format!("{prefix} [")
    }
}

fn bracket_inline(prefix: &str, inner: &str) -> String {
    if prefix.is_empty() {
        format!("[{inner}]")
    } else if inner.is_empty() {
        format!("{prefix} []")
    } else {
        format!("{prefix} [{inner}]")
    }
}

pub fn parse_ast(path: &str, source: &str) -> Result<AstProgram, ParseError> {
    let tokens = lex_source(path, source)?;
    let text_body_line_ranges = text_literal_body_line_ranges(&tokens);
    let lines = parser_lines(&tokens);
    let item_lines = merge_multiline_bytes_lines(&lines, &text_body_line_ranges);
    let item_lines = merge_multiline_drain_lines(&item_lines, &text_body_line_ranges);
    let items = parser_items(&item_lines, &text_body_line_ranges);
    let mut expressions = Vec::new();
    let statements = ast_statement_tree(&items, &mut expressions, source);
    link_multiline_block_outputs(&statements, &mut expressions);
    Ok(AstProgram {
        tokens,
        lines,
        items,
        statements,
        expressions,
    })
}

fn link_multiline_block_outputs(statements: &[AstStatement], expressions: &mut [AstExpr]) {
    for statement in statements {
        link_multiline_block_outputs(&statement.children, expressions);
        let Some(parent_expr) = statement.expr else {
            continue;
        };
        let Some(output) = first_statement_expression(&statement.children) else {
            continue;
        };
        let Some(expression) = expressions.get_mut(parent_expr) else {
            continue;
        };
        if let AstExprKind::Then {
            output: current, ..
        } = &mut expression.kind
            && current.is_none()
        {
            *current = Some(output);
        }
    }
}

fn first_statement_expression(statements: &[AstStatement]) -> Option<usize> {
    statements.iter().find_map(|statement| {
        statement
            .expr
            .or_else(|| first_statement_expression(&statement.children))
    })
}

/// Tokenizes Boon source with the same lexer used by the parser.
///
/// Editor tooling uses this surface so highlighting and semantic inspection do
/// not need a second, drifting approximation of the language grammar.
pub fn lex_source(path: &str, source: &str) -> Result<Vec<AstToken>, ParseError> {
    let source_index = SourceIndex::new(source);
    let spanned = token_parser()
        .repeated()
        .then_ignore(end())
        .parse(source)
        .map_err(|errors| {
            let message = errors
                .into_iter()
                .next()
                .map(|error| format!("syntax error near {:?}", error.span()))
                .unwrap_or_else(|| "syntax error".to_owned());
            ParseError {
                path: path.to_owned(),
                line: None,
                column: None,
                message,
            }
        })?;
    Ok(spanned
        .into_iter()
        .map(|(kind, span)| {
            let start = source_index.char_offset_to_byte(span.start);
            let end = source_index.char_offset_to_byte(span.end);
            let (line, column) = source_index.line_column(source, start);
            let raw_lexeme = source.get(start..end).unwrap_or_default();
            let lexeme = match kind {
                AstTokenKind::String | AstTokenKind::Comment | AstTokenKind::Newline => raw_lexeme,
                _ => raw_lexeme.trim_matches(|ch| matches!(ch, ' ' | '\t' | '\r')),
            };
            AstToken {
                kind,
                lexeme: lexeme.to_owned(),
                line,
                column,
                start,
                end,
            }
        })
        .collect())
}

fn document_statement(ast: &AstProgram) -> Option<&AstStatement> {
    ast.statements.iter().find(|statement| {
        matches!(
            &statement.kind,
            AstStatementKind::Field { name } if name == "document"
        )
    })
}

fn scene_statement(ast: &AstProgram) -> Option<&AstStatement> {
    ast.statements.iter().find(|statement| {
        matches!(
            &statement.kind,
            AstStatementKind::Field { name } if name == "scene"
        )
    })
}

fn statement_contains_line(statement: &AstStatement, line: usize) -> bool {
    statement.line == line
        || statement
            .children
            .iter()
            .any(|child| statement_contains_line(child, line))
}

fn token_parser() -> impl Parser<char, (AstTokenKind, std::ops::Range<usize>), Error = Simple<char>>
{
    let horizontal_space = one_of(" \t\r").repeated().ignored();
    let ident_start = filter(|ch: &char| ch.is_ascii_alphabetic() || *ch == '_');
    let ident_tail =
        filter(|ch: &char| ch.is_ascii_alphanumeric() || matches!(*ch, '_' | '-' | '/'));
    let identifier = ident_start
        .then(ident_tail.repeated())
        .to(AstTokenKind::Identifier);
    let number = text::int(10).to(AstTokenKind::Number);
    let string = just('"')
        .ignore_then(
            choice((
                just('\\').ignore_then(any()).ignored(),
                none_of('"').ignored(),
            ))
            .repeated(),
        )
        .then_ignore(just('"'))
        .to(AstTokenKind::String);
    let comment = just("--")
        .ignore_then(none_of('\n').repeated())
        .to(AstTokenKind::Comment);
    let operator = choice((
        just("=>").ignored(),
        just("|>").ignored(),
        just("==").ignored(),
        just(">=").ignored(),
        just("<=").ignored(),
        just("!=").ignored(),
        one_of("><=|+-%*/").ignored(),
    ))
    .to(AstTokenKind::Operator);
    let symbol = one_of("[]{}():,.$#").to(AstTokenKind::Symbol);
    let newline = just('\n').to(AstTokenKind::Newline);
    let unknown = any().to(AstTokenKind::Unknown);

    choice((
        string, comment, identifier, number, operator, symbol, newline, unknown,
    ))
    .padded_by(horizontal_space)
    .map_with_span(|kind, span| (kind, span))
}

struct SourceIndex {
    char_to_byte: Vec<usize>,
    line_starts: Vec<usize>,
}

impl SourceIndex {
    fn new(source: &str) -> Self {
        let mut char_to_byte = source
            .char_indices()
            .map(|(byte, _)| byte)
            .collect::<Vec<_>>();
        char_to_byte.push(source.len());
        let mut line_starts = vec![0];
        for (byte, ch) in source.char_indices() {
            if ch == '\n' {
                line_starts.push(byte + ch.len_utf8());
            }
        }
        Self {
            char_to_byte,
            line_starts,
        }
    }

    fn char_offset_to_byte(&self, char_offset: usize) -> usize {
        self.char_to_byte
            .get(char_offset)
            .copied()
            .unwrap_or_else(|| *self.char_to_byte.last().unwrap_or(&0))
    }

    fn line_column(&self, source: &str, byte_index: usize) -> (usize, usize) {
        let line_index = self
            .line_starts
            .partition_point(|start| *start <= byte_index);
        let line = line_index.max(1);
        let line_start = self.line_starts[line - 1];
        let column = source
            .get(line_start..byte_index)
            .map(|slice| slice.chars().count() + 1)
            .unwrap_or(1);
        (line, column)
    }
}

fn parser_lines(tokens: &[AstToken]) -> Vec<ParserLine> {
    let mut lines = Vec::new();
    let mut current_line = None;
    let mut indent = 0usize;
    let mut start = 0usize;
    let mut end = 0usize;
    let mut symbols = Vec::new();
    let mut symbol_spans = Vec::new();
    for token in tokens {
        if current_line != Some(token.line) {
            if let Some(line) = current_line {
                lines.push(ParserLine {
                    line,
                    indent,
                    symbols: std::mem::take(&mut symbols),
                    symbol_spans: std::mem::take(&mut symbol_spans),
                    start,
                    end,
                });
            }
            current_line = Some(token.line);
            indent = token.column.saturating_sub(1);
            start = token.start;
        }
        end = token.end;
        if !matches!(token.kind, AstTokenKind::Comment | AstTokenKind::Newline)
            && !token.lexeme.is_empty()
        {
            symbols.push(token.lexeme.clone());
            symbol_spans.push((token.start, token.end));
        }
    }
    if let Some(line) = current_line {
        lines.push(ParserLine {
            line,
            indent,
            symbols,
            symbol_spans,
            start,
            end,
        });
    }
    lines
}

fn merge_multiline_bytes_lines(
    lines: &[ParserLine],
    text_body_line_ranges: &[(usize, usize)],
) -> Vec<ParserLine> {
    merge_multiline_braced_lines(lines, text_body_line_ranges, unclosed_bytes_body_open)
}

fn merge_multiline_drain_lines(
    lines: &[ParserLine],
    text_body_line_ranges: &[(usize, usize)],
) -> Vec<ParserLine> {
    merge_multiline_braced_lines(lines, text_body_line_ranges, unclosed_drain_body_open)
}

fn merge_multiline_braced_lines(
    lines: &[ParserLine],
    text_body_line_ranges: &[(usize, usize)],
    unclosed_body_open: fn(&[String]) -> Option<usize>,
) -> Vec<ParserLine> {
    let mut merged = Vec::new();
    let mut index = 0usize;
    while let Some(line) = lines.get(index) {
        if line_is_in_ranges(line.line, text_body_line_ranges) {
            merged.push(line.clone());
            index += 1;
            continue;
        }
        let Some(body_open) = unclosed_body_open(&line.symbols) else {
            merged.push(line.clone());
            index += 1;
            continue;
        };
        let mut current = line.clone();
        index += 1;
        while matching_close(&current.symbols, body_open).is_none() {
            let Some(next) = lines.get(index) else {
                break;
            };
            if line_is_in_ranges(next.line, text_body_line_ranges) {
                break;
            }
            current.end = next.end;
            current.symbols.extend(next.symbols.iter().cloned());
            current
                .symbol_spans
                .extend(next.symbol_spans.iter().copied());
            index += 1;
        }
        merged.push(current);
    }
    merged
}

fn line_is_in_ranges(line: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|(start, end)| line >= *start && line <= *end)
}

fn unclosed_bytes_body_open(symbols: &[String]) -> Option<usize> {
    let bytes = symbols.iter().position(|symbol| symbol == "BYTES")?;
    let body_open = match symbols.get(bytes + 1).map(String::as_str) {
        Some("{") => bytes + 1,
        Some("[") => {
            let size_close = matching_close(symbols, bytes + 1)?;
            (symbols.get(size_close + 1).map(String::as_str) == Some("{"))
                .then_some(size_close + 1)?
        }
        _ => return None,
    };
    matching_close(symbols, body_open)
        .is_none()
        .then_some(body_open)
}

fn unclosed_drain_body_open(symbols: &[String]) -> Option<usize> {
    let drain = symbols.iter().position(|symbol| symbol == "DRAIN")?;
    let body_open =
        (symbols.get(drain + 1).map(String::as_str) == Some("{")).then_some(drain + 1)?;
    matching_close(symbols, body_open)
        .is_none()
        .then_some(body_open)
}

fn parser_items(lines: &[ParserLine], text_body_line_ranges: &[(usize, usize)]) -> Vec<ParserItem> {
    lines
        .iter()
        .filter(|line| {
            !text_body_line_ranges
                .iter()
                .any(|(start, end)| line.line >= *start && line.line <= *end)
        })
        .filter(|line| !line.symbols.is_empty())
        .map(parser_item)
        .collect()
}

fn parser_item(line: &ParserLine) -> ParserItem {
    let symbols = line.symbols.clone();
    let symbol_spans = line.symbol_spans.clone();
    let field = ast_field_name(&symbols).map(ToOwned::to_owned);
    let function = (symbols.first().map(String::as_str) == Some("FUNCTION"))
        .then(|| symbols.get(1).cloned())
        .flatten();
    let source_event = ast_insource_slice_event(&symbols).map(ToOwned::to_owned);
    let operators = ast_expression_operators(&symbols);
    let is_list = symbols.iter().any(|lexeme| is_list_constructor(lexeme))
        && find_top_level_pipe(&symbols).is_none();
    ParserItem {
        line: line.line,
        indent: line.indent,
        start: line.start,
        end: line.end,
        map_new_function: ast_map_new_function(&symbols).map(ToOwned::to_owned),
        source_event,
        hold: ast_hold_name(&symbols).map(ToOwned::to_owned),
        list_capacity: ast_list_capacity(&symbols),
        opens_scope: ast_opens_scope(&symbols),
        closes_scope: symbols.len() == 1
            && matches!(symbols.first().map(String::as_str), Some("}" | "]" | ")")),
        operators,
        symbols,
        symbol_spans,
        field,
        example: None,
        function,
        is_list,
    }
}

fn is_list_constructor(lexeme: &str) -> bool {
    matches!(lexeme, "LIST" | "List/range")
}

fn ast_statement_tree(
    items: &[ParserItem],
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<AstStatement> {
    let mut index = 0usize;
    let mut next_id = 0usize;
    ast_statement_block(items, &mut index, 0, expressions, &mut next_id, source)
}

fn ast_statement_block(
    items: &[ParserItem],
    index: &mut usize,
    min_indent: usize,
    expressions: &mut Vec<AstExpr>,
    next_id: &mut usize,
    source: &str,
) -> Vec<AstStatement> {
    let mut statements = Vec::new();
    while let Some(item) = items.get(*index) {
        if item.indent < min_indent {
            break;
        }
        if item.closes_scope {
            *index += 1;
            continue;
        }
        let indent = item.indent;
        let mut statement = ast_statement(item, expressions, *next_id, source);
        *next_id += 1;
        *index += 1;
        if item.opens_scope || items.get(*index).is_some_and(|next| next.indent > indent) {
            statement.children =
                ast_statement_block(items, index, indent + 1, expressions, next_id, source);
        }
        statements.push(statement);
    }
    statements
}

fn ast_statement(
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    id: usize,
    source: &str,
) -> AstStatement {
    let is_semantic_block = item.symbols.first().map(String::as_str) == Some("BLOCK")
        && item.symbols.last().map(String::as_str) == Some("{");
    let kind = if let Some(function) = item.function.clone() {
        AstStatementKind::Function {
            name: function,
            args: ast_function_args(&item.symbols),
        }
    } else if item.has_lexeme("SOURCE") {
        AstStatementKind::Source {
            field: item.field.clone(),
            event: item.source_event.clone(),
        }
    } else if item.has_lexeme("HOLD") {
        AstStatementKind::Hold {
            field: item.field.clone(),
            name: item.hold.clone(),
        }
    } else if item.is_list {
        AstStatementKind::List {
            field: item.field.clone(),
            capacity: item.list_capacity,
        }
    } else if let Some(field) = item.field.clone() {
        AstStatementKind::Field { name: field }
    } else if is_semantic_block
        || matches!(item.symbols.as_slice(), [one] if matches!(one.as_str(), "[" | "{" | "(" | "]" | "}" | ")"))
    {
        AstStatementKind::Block
    } else if item
        .symbols
        .starts_with(&[".".to_owned(), ".".to_owned(), ".".to_owned()])
    {
        AstStatementKind::Spread
    } else {
        AstStatementKind::Expression
    };
    let expr = if matches!(kind, AstStatementKind::Function { .. })
        || (matches!(kind, AstStatementKind::Block) && !is_semantic_block)
    {
        None
    } else {
        let expr_tokens = statement_expression_tokens(item);
        (!expr_tokens.is_empty()).then(|| parse_ast_expr(&expr_tokens, item, expressions, source))
    };
    AstStatement {
        id,
        line: item.line,
        indent: item.indent,
        start: item.start,
        end: item.end,
        kind,
        expr,
        children: Vec::new(),
    }
}

fn statement_expression_tokens(item: &ParserItem) -> Vec<String> {
    if item.field.is_some() && item.symbols.get(1).map(String::as_str) == Some(":") {
        if matches!(
            item.symbols.get(2).map(String::as_str),
            Some("[") | Some("{")
        ) && item.symbols.len() == 3
        {
            return Vec::new();
        }
        return item.symbols[2..].to_vec();
    }
    item.symbols.clone()
}

fn parse_ast_expr(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> usize {
    let kind = ast_expr_kind(tokens, item, expressions, source);
    let (start, end) = span_for_tokens(tokens, item).unwrap_or((item.start, item.end));
    push_ast_expr(item, expressions, kind, start, end)
}

fn push_ast_expr(
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    kind: AstExprKind,
    start: usize,
    end: usize,
) -> usize {
    let id = expressions.len();
    expressions.push(AstExpr {
        id,
        line: item.line,
        start,
        end,
        kind,
    });
    id
}

fn span_for_tokens(tokens: &[String], item: &ParserItem) -> Option<(usize, usize)> {
    if tokens.is_empty() {
        return None;
    }
    item.symbols
        .windows(tokens.len())
        .position(|window| window == tokens)
        .and_then(|start_index| {
            let end_index = start_index + tokens.len() - 1;
            Some((
                item.symbol_spans.get(start_index)?.0,
                item.symbol_spans.get(end_index)?.1,
            ))
        })
}

fn ast_expr_kind(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> AstExprKind {
    if tokens.is_empty() {
        return AstExprKind::Delimiter;
    }
    if tokens.len() > 3 && tokens[0] == "." && tokens[1] == "." && tokens[2] == "." {
        return ast_expr_kind(&tokens[3..], item, expressions, source);
    }
    if tokens
        .iter()
        .all(|token| matches!(token.as_str(), "[" | "]" | "{" | "}" | "(" | ")"))
    {
        return AstExprKind::Delimiter;
    }
    if tokens == ["SOURCE"] {
        return AstExprKind::Source;
    }
    if tokens == ["True"] {
        return AstExprKind::Bool(true);
    }
    if tokens == ["False"] {
        return AstExprKind::Bool(false);
    }
    if let Some(number) = ast_number_literal(tokens) {
        return AstExprKind::Number(number);
    }
    if let Some((radix, digits, value)) = ast_byte_literal(tokens, item, source) {
        return AstExprKind::ByteLiteral {
            radix,
            digits,
            value,
        };
    }
    if let Some(arrow) = find_top_level_token(tokens, "=>") {
        return AstExprKind::MatchArm {
            pattern: tokens[..arrow].to_vec(),
            output: (!tokens[arrow + 1..].is_empty())
                .then(|| parse_ast_expr(&tokens[arrow + 1..], item, expressions, source)),
        };
    }
    if let Some(value) = string_literal_value(tokens) {
        return AstExprKind::StringLiteral(value);
    }
    if let Some(text) = text_literal_value(tokens, item, source) {
        return AstExprKind::TextLiteral(text);
    }
    if tokens == ["Text/empty", "(", ")"] {
        return AstExprKind::TextLiteral(String::new());
    }
    if tokens == ["Text/empty"] {
        return AstExprKind::TextLiteral(String::new());
    }
    if tokens.first().map(String::as_str) == Some("BLOCK")
        && tokens.last().map(String::as_str) == Some("{")
    {
        return AstExprKind::Identifier("BLOCK".to_owned());
    }
    if let Some(path) = ast_drain_path(tokens) {
        return AstExprKind::Drain { path };
    }
    if let Some(pipe) = find_top_level_pipe(tokens) {
        return ast_pipe_expr_kind(tokens, pipe, item, expressions, source);
    }
    if tokens.first().map(String::as_str) == Some("LATEST") {
        return AstExprKind::Latest;
    }
    if tokens.first().map(String::as_str) == Some("LIST") {
        return AstExprKind::ListLiteral {
            capacity: ast_list_capacity(tokens),
            items: ast_list_items(tokens, item, expressions, source),
        };
    }
    if let Some((size, items)) = ast_bytes_literal(tokens, item, expressions, source) {
        return AstExprKind::BytesLiteral { size, items };
    }
    if tokens.first().map(String::as_str) == Some("[")
        && tokens.last().map(String::as_str) == Some("]")
    {
        return AstExprKind::Object(ast_record_fields(tokens, item, expressions, source));
    }
    if tokens.first().map(String::as_str) == Some("[")
        && tokens.get(2).map(String::as_str) == Some(":")
        && tokens.len() > 3
    {
        let value = parse_ast_expr(&tokens[3..], item, expressions, source);
        let (start, end) = span_for_tokens(tokens, item).unwrap_or((item.start, item.end));
        return AstExprKind::Object(vec![AstRecordField {
            name: tokens[1].clone(),
            value,
            start,
            end,
            spread: false,
        }]);
    }
    if tokens.len() >= 3
        && tokens.get(1).map(String::as_str) == Some("[")
        && tokens.last().map(String::as_str) == Some("]")
        && tokens
            .first()
            .is_some_and(|token| value_starts_uppercase_identifier(token))
    {
        return AstExprKind::TaggedObject {
            tag: tokens[0].clone(),
            fields: ast_record_fields(&tokens[1..], item, expressions, source),
        };
    }
    if let Some((left, op, right)) = split_infix(tokens) {
        let left = parse_ast_expr(left, item, expressions, source);
        let right = parse_ast_expr(right, item, expressions, source);
        return AstExprKind::Infix {
            left,
            op: op.to_owned(),
            right,
        };
    }
    if let Some((input_tokens, field)) = split_postfix_field_access(tokens) {
        let input = parse_ast_expr(input_tokens, item, expressions, source);
        return AstExprKind::Pipe {
            input,
            op: format!("Field/{field}"),
            args: Vec::new(),
        };
    }
    if let Some((function, args)) = ast_call(tokens, item, expressions, source) {
        return AstExprKind::Call { function, args };
    }
    if tokens.len() == 1 && is_name(&tokens[0]) {
        let token = tokens[0].clone();
        if value_starts_uppercase_identifier(&token) {
            AstExprKind::Tag(token)
        } else {
            AstExprKind::Identifier(token)
        }
    } else if tokens.iter().any(|token| token == ".") {
        AstExprKind::Path(path_segments(tokens))
    } else {
        AstExprKind::Unknown(tokens.to_vec())
    }
}

fn ast_drain_path(tokens: &[String]) -> Option<AstDrainPath> {
    if tokens.first().map(String::as_str) != Some("DRAIN")
        || tokens.get(1).map(String::as_str) != Some("{")
        || matching_close(tokens, 1) != Some(tokens.len().checked_sub(1)?)
    {
        return None;
    }
    drain_path_from_symbols(&tokens[2..tokens.len() - 1])
}

fn drain_path_from_symbols(symbols: &[String]) -> Option<AstDrainPath> {
    if symbols.is_empty() || symbols.len().is_multiple_of(2) {
        return None;
    }
    let mut segments = Vec::with_capacity(symbols.len().div_ceil(2));
    for (index, symbol) in symbols.iter().enumerate() {
        if index % 2 == 1 {
            if symbol != "." {
                return None;
            }
            continue;
        }
        if !is_drain_path_segment(symbol) {
            return None;
        }
        segments.push(symbol.clone());
    }
    let root = segments.remove(0);
    if root == "PASSED" {
        return (!segments.is_empty()).then_some(AstDrainPath::Passed { fields: segments });
    }
    if value_starts_uppercase_identifier(&root) {
        return None;
    }
    if segments.is_empty() {
        Some(AstDrainPath::Binding { name: root })
    } else {
        Some(AstDrainPath::Field {
            binding: root,
            fields: segments,
        })
    }
}

fn is_drain_path_segment(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && is_name(value)
}

fn ast_number_literal(tokens: &[String]) -> Option<String> {
    match tokens {
        [value] if value.chars().all(|ch| ch.is_ascii_digit()) => Some(value.clone()),
        [left, dot, right]
            if dot == "."
                && left.chars().all(|ch| ch.is_ascii_digit())
                && right.chars().all(|ch| ch.is_ascii_digit()) =>
        {
            Some(format!("{left}.{right}"))
        }
        [left, dot, right @ ..]
            if dot == "."
                && left.chars().all(|ch| ch.is_ascii_digit())
                && !right.is_empty()
                && right
                    .iter()
                    .all(|part| part.chars().all(|ch| ch.is_ascii_digit())) =>
        {
            Some(format!("{left}.{}", right.join("")))
        }
        [minus, value] if minus == "-" && value.chars().all(|ch| ch.is_ascii_digit()) => {
            Some(format!("-{value}"))
        }
        [minus, left, dot, right]
            if minus == "-"
                && dot == "."
                && left.chars().all(|ch| ch.is_ascii_digit())
                && right.chars().all(|ch| ch.is_ascii_digit()) =>
        {
            Some(format!("-{left}.{right}"))
        }
        [minus, left, dot, right @ ..]
            if minus == "-"
                && dot == "."
                && left.chars().all(|ch| ch.is_ascii_digit())
                && !right.is_empty()
                && right
                    .iter()
                    .all(|part| part.chars().all(|ch| ch.is_ascii_digit())) =>
        {
            Some(format!("-{left}.{}", right.join("")))
        }
        _ => None,
    }
}

fn ast_byte_literal(
    tokens: &[String],
    item: &ParserItem,
    source: &str,
) -> Option<(u8, String, u8)> {
    let [base, suffix] = tokens else {
        return None;
    };
    let adjacent_text = format!("{base}{suffix}");
    let adjacent_in_source = source
        .get(item.start..item.end)
        .is_some_and(|item_source| item_source.contains(&adjacent_text));
    if !adjacent_in_source {
        return None;
    }
    parse_byte_literal_parts(base, suffix).ok()
}

fn parse_byte_literal_parts(base: &str, suffix: &str) -> Result<(u8, String, u8), String> {
    let radix = match base {
        "2" => 2,
        "8" => 8,
        "10" => 10,
        "16" => 16,
        _ => {
            return Err("byte literal base must be one of `2`, `8`, `10`, or `16`".to_owned());
        }
    };
    let Some(digits) = suffix.strip_prefix('u') else {
        return Err("byte literal must use explicit base notation such as `16uFF`".to_owned());
    };
    if digits.is_empty() {
        return Err("byte literal must include digits after `u`".to_owned());
    }
    if !digits.chars().all(|ch| ch.is_digit(radix as u32)) {
        return Err(format!(
            "byte literal `{base}u{digits}` contains digits outside base {radix}"
        ));
    }
    let value = u16::from_str_radix(digits, radix as u32).map_err(|_| {
        format!("byte literal `{base}u{digits}` could not be parsed in base {radix}")
    })?;
    if value > u8::MAX as u16 {
        return Err(format!(
            "byte literal `{base}u{digits}` evaluates to {value}, but bytes must be 0..255"
        ));
    }
    Ok((radix, digits.to_owned(), value as u8))
}

fn ast_pipe_expr_kind(
    tokens: &[String],
    pipe: usize,
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> AstExprKind {
    let input = parse_ast_expr(&tokens[..pipe], item, expressions, source);
    let op = tokens
        .get(pipe + 1)
        .cloned()
        .unwrap_or_else(|| "pipe".to_owned());
    if op == "DRAINING" && pipe + 2 == tokens.len() {
        return AstExprKind::Draining { input };
    }
    if op == "HOLD" {
        let name = tokens
            .get(pipe + 2)
            .cloned()
            .unwrap_or_else(|| "hold".to_owned());
        push_inline_hold_latest_exprs(&tokens[pipe + 3..], item, expressions, source);
        return AstExprKind::Hold {
            initial: input,
            name,
        };
    }
    if op == "WHEN" {
        push_inline_when_match_arms(&tokens[pipe + 1..], item, expressions, source);
        return AstExprKind::When { input };
    }
    if op == "THEN" {
        return AstExprKind::Then {
            input,
            output: ast_operator_block_expr(&tokens[pipe + 1..], item, expressions, source),
        };
    }
    if op == "SOURCE"
        && let Some(value) = ast_operator_block_expr(&tokens[pipe + 1..], item, expressions, source)
    {
        let (start, end) =
            span_for_tokens(&tokens[pipe + 1..], item).unwrap_or((item.start, item.end));
        return AstExprKind::Pipe {
            input,
            op,
            args: vec![AstCallArg {
                name: None,
                value,
                start,
                end,
            }],
        };
    }
    AstExprKind::Pipe {
        input,
        op,
        args: ast_call_args_after_operator(&tokens[pipe + 1..], item, expressions, source),
    }
}

fn ast_record_fields(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<AstRecordField> {
    split_top_level(&tokens[1..tokens.len() - 1], ",")
        .into_iter()
        .enumerate()
        .filter_map(|(index, part)| {
            if part.starts_with(&[".".to_owned(), ".".to_owned(), ".".to_owned()]) && part.len() > 3
            {
                let (start, end) = span_for_tokens(&part, item).unwrap_or((item.start, item.end));
                return Some(AstRecordField {
                    name: format!("__spread_{index}"),
                    value: parse_ast_expr(&part[3..], item, expressions, source),
                    start,
                    end,
                    spread: true,
                });
            }
            if part.len() < 3 || part.get(1).map(String::as_str) != Some(":") {
                return None;
            }
            let (start, end) = span_for_tokens(&part, item).unwrap_or((item.start, item.end));
            Some(AstRecordField {
                name: part[0].clone(),
                value: parse_ast_expr(&part[2..], item, expressions, source),
                start,
                end,
                spread: false,
            })
        })
        .collect()
}

fn ast_call(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Option<(String, Vec<AstCallArg>)> {
    let open = tokens.iter().position(|token| token == "(")?;
    if open == 0 {
        return None;
    }
    let function = tokens[..open].join("");
    let close = matching_close(tokens, open).unwrap_or(tokens.len() - 1);
    let arg_tokens = if close > open {
        &tokens[open + 1..close]
    } else {
        &[]
    };
    Some((
        function,
        split_top_level(arg_tokens, ",")
            .into_iter()
            .filter_map(|part| ast_call_arg(&part, item, expressions, source))
            .collect(),
    ))
}

fn ast_call_args_after_operator(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<AstCallArg> {
    let Some(open) = tokens.iter().position(|token| token == "(") else {
        return Vec::new();
    };
    let close = matching_close(tokens, open).unwrap_or(tokens.len() - 1);
    let arg_tokens = if close > open {
        &tokens[open + 1..close]
    } else {
        &[]
    };
    split_top_level(arg_tokens, ",")
        .into_iter()
        .filter_map(|part| ast_call_arg(&part, item, expressions, source))
        .collect()
}

fn ast_operator_block_expr(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Option<usize> {
    let open = tokens.iter().position(|token| token == "{")?;
    let close = matching_close(tokens, open)?;
    (close > open + 1).then(|| parse_ast_expr(&tokens[open + 1..close], item, expressions, source))
}

fn push_inline_hold_latest_exprs(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) {
    let Some(latest) = tokens.iter().position(|token| token == "LATEST") else {
        return;
    };
    let _ = parse_ast_expr(&tokens[latest..=latest], item, expressions, source);
    let _ = ast_operator_block_expr(&tokens[latest..], item, expressions, source);
}

fn push_inline_when_match_arms(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) {
    let Some(open) = tokens.iter().position(|token| token == "{") else {
        return;
    };
    let Some(close) = matching_close(tokens, open) else {
        return;
    };
    if close <= open + 1 {
        return;
    }
    for part in split_top_level(&tokens[open + 1..close], ",") {
        if part.iter().any(|token| token == "=>") {
            parse_ast_expr(&part, item, expressions, source);
        }
    }
}

fn ast_call_arg(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Option<AstCallArg> {
    if tokens.is_empty() {
        return None;
    }
    if tokens.get(1).map(String::as_str) == Some(":") {
        let (start, end) = span_for_tokens(tokens, item).unwrap_or((item.start, item.end));
        return Some(AstCallArg {
            name: Some(tokens[0].clone()),
            value: parse_ast_expr(&tokens[2..], item, expressions, source),
            start,
            end,
        });
    }
    let (start, end) = span_for_tokens(tokens, item).unwrap_or((item.start, item.end));
    Some(AstCallArg {
        name: None,
        value: parse_ast_expr(tokens, item, expressions, source),
        start,
        end,
    })
}

fn ast_function_args(tokens: &[String]) -> Vec<String> {
    let Some(open) = tokens.iter().position(|token| token == "(") else {
        return Vec::new();
    };
    let close = matching_close(tokens, open).unwrap_or(tokens.len() - 1);
    split_top_level(&tokens[open + 1..close], ",")
        .into_iter()
        .filter_map(|part| part.first().cloned())
        .collect()
}

fn find_top_level_pipe(tokens: &[String]) -> Option<usize> {
    let mut depth = 0i32;
    let mut pipe = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            _ => {}
        }
        if token == "|>" && depth == 0 {
            pipe = Some(index);
        }
    }
    pipe
}

fn find_top_level_token(tokens: &[String], needle: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            _ => {}
        }
        if token == needle && depth == 0 {
            return Some(index);
        }
    }
    None
}

fn split_infix(tokens: &[String]) -> Option<(&[String], &str, &[String])> {
    let mut depth = 0i32;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            "==" | ">" | "<" | ">=" | "<=" | "!=" | "+" | "-" | "*" | "/" | "%"
                if depth == 0 && index > 0 && index + 1 < tokens.len() =>
            {
                return Some((&tokens[..index], token, &tokens[index + 1..]));
            }
            _ => {}
        }
    }
    None
}

fn split_postfix_field_access(tokens: &[String]) -> Option<(&[String], String)> {
    let mut depth = 0i32;
    let mut dot = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.as_str() {
            "[" | "{" | "(" => depth += 1,
            "]" | "}" | ")" => depth -= 1,
            "." if depth == 0 && index > 0 && index + 1 < tokens.len() => dot = Some(index),
            _ => {}
        }
    }
    let dot = dot?;
    let input = &tokens[..dot];
    if !input.iter().any(|token| token == ")") {
        return None;
    }
    let field_tokens = &tokens[dot + 1..];
    if field_tokens.is_empty()
        || field_tokens.iter().any(|token| token == ".")
        || !field_tokens.iter().all(|token| is_name(token))
    {
        return None;
    }
    Some((input, field_tokens.join("")))
}

fn matching_close(tokens: &[String], open: usize) -> Option<usize> {
    let close_token = match tokens.get(open).map(String::as_str)? {
        "(" => ")",
        "[" => "]",
        "{" => "}",
        _ => return None,
    };
    let mut stack = vec![close_token];
    for (index, token) in tokens.iter().enumerate().skip(open + 1) {
        match token.as_str() {
            "(" => stack.push(")"),
            "[" => stack.push("]"),
            "{" => stack.push("}"),
            ")" | "]" | "}" => {
                if stack.pop() != Some(token.as_str()) {
                    return None;
                }
                if stack.is_empty() {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
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

fn path_segments(tokens: &[String]) -> Vec<String> {
    tokens
        .iter()
        .filter(|token| token.as_str() != ".")
        .filter(|token| is_name(token))
        .cloned()
        .collect()
}

fn text_literal_value(tokens: &[String], item: &ParserItem, source: &str) -> Option<String> {
    if tokens.first().map(String::as_str) != Some("TEXT")
        || tokens.get(1).map(String::as_str) != Some("{")
    {
        return None;
    }
    if tokens == ["TEXT", "{"] {
        return text_literal_source_value_from_start(item.start, source);
    }
    let close = tokens.iter().rposition(|token| token == "}")?;
    if close + 1 != tokens.len() {
        return None;
    }
    if let Some(text) = text_literal_source_value(tokens, item, source) {
        return Some(text);
    }
    Some(join_text_literal_tokens(&tokens[2..close]))
}

fn text_literal_source_value(tokens: &[String], item: &ParserItem, source: &str) -> Option<String> {
    let (start, end) = span_for_tokens(tokens, item)?;
    let slice = source.get(start..end)?;
    let text_start = slice.find("TEXT")?;
    let open = text_start + slice[text_start..].find('{')?;
    let content_start = open + 1;
    let mut depth = 1i32;
    for (offset, ch) in slice[content_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(
                        slice[content_start..content_start + offset]
                            .trim()
                            .to_owned(),
                    );
                }
            }
            _ => {}
        }
    }
    None
}

fn text_literal_source_value_from_start(start: usize, source: &str) -> Option<String> {
    let slice = source.get(start..)?;
    let text_start = slice.find("TEXT")?;
    let open = text_start + slice[text_start..].find('{')?;
    let content_start = open + 1;
    let mut depth = 1i32;
    for (offset, ch) in slice[content_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(
                        slice[content_start..content_start + offset]
                            .trim()
                            .to_owned(),
                    );
                }
            }
            _ => {}
        }
    }
    None
}

fn string_literal_value(tokens: &[String]) -> Option<String> {
    if tokens.len() != 1 {
        return None;
    }
    tokens[0]
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(unescape_string_literal)
}

fn unescape_string_literal(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                output.push(match next {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '\\' => '\\',
                    '"' => '"',
                    other => other,
                });
            }
        } else {
            output.push(ch);
        }
    }
    output
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

fn value_starts_uppercase_identifier(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn ast_field_name(symbols: &[String]) -> Option<&str> {
    if symbols.get(1).map(String::as_str) != Some(":") {
        return None;
    }
    let name = symbols.first()?.as_str();
    is_name(name).then_some(name)
}

fn ast_insource_slice_event(symbols: &[String]) -> Option<&str> {
    let open = symbols.iter().position(|lexeme| lexeme == "[")?;
    let event = symbols.get(open + 1)?.as_str();
    (symbols.get(open + 2).map(String::as_str) == Some(":")
        && symbols.iter().any(|lexeme| lexeme == "SOURCE")
        && is_name(event))
    .then_some(event)
}

fn ast_hold_name(symbols: &[String]) -> Option<&str> {
    let hold = symbols.iter().position(|lexeme| lexeme == "HOLD")?;
    symbols
        .get(hold + 1)
        .map(String::as_str)
        .filter(|name| is_name(name))
}

fn ast_list_capacity(symbols: &[String]) -> Option<usize> {
    let list = symbols.iter().position(|lexeme| lexeme == "LIST")?;
    (symbols.get(list + 1).map(String::as_str) == Some("["))
        .then(|| symbols.get(list + 2))?
        .and_then(|value| value.parse().ok())
}

fn ast_list_items(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Vec<usize> {
    let Some(open) = tokens.iter().position(|token| token == "{") else {
        return Vec::new();
    };
    let Some(close) = matching_close(tokens, open) else {
        return Vec::new();
    };
    if close <= open + 1 {
        return Vec::new();
    }
    split_top_level(&tokens[open + 1..close], ",")
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(|part| parse_ast_expr(&part, item, expressions, source))
        .collect()
}

fn ast_bytes_literal(
    tokens: &[String],
    item: &ParserItem,
    expressions: &mut Vec<AstExpr>,
    source: &str,
) -> Option<(BytesSizeSyntax, Vec<usize>)> {
    if tokens.first().map(String::as_str) != Some("BYTES") {
        return None;
    }
    let (size, body_open) = match tokens.get(1).map(String::as_str) {
        Some("{") => (BytesSizeSyntax::Dynamic, 1),
        Some("[") => {
            let close = matching_close(tokens, 1)?;
            let size_tokens = &tokens[2..close];
            let size = match size_tokens {
                [value] if value == "__" => BytesSizeSyntax::Infer,
                [value] => BytesSizeSyntax::Fixed(value.parse().ok()?),
                _ => return None,
            };
            if tokens.get(close + 1).map(String::as_str) != Some("{") {
                return None;
            }
            (size, close + 1)
        }
        _ => return None,
    };
    let body_close = matching_close(tokens, body_open)?;
    if body_close + 1 != tokens.len() {
        return None;
    }
    let items = if body_close <= body_open + 1 {
        Vec::new()
    } else {
        split_top_level(&tokens[body_open + 1..body_close], ",")
            .into_iter()
            .filter(|part| !part.is_empty())
            .map(|part| parse_ast_expr(&part, item, expressions, source))
            .collect()
    };
    Some((size, items))
}

fn ast_map_new_function(symbols: &[String]) -> Option<&str> {
    let map = symbols.iter().position(|lexeme| lexeme == "List/map")?;
    let new = symbols[map..].iter().position(|lexeme| lexeme == "new")? + map;
    (symbols.get(new + 1).map(String::as_str) == Some(":"))
        .then(|| symbols.get(new + 2))?
        .map(String::as_str)
        .filter(|name| is_name(name))
}

fn ast_opens_scope(symbols: &[String]) -> bool {
    if symbols.iter().any(|lexeme| lexeme == "SOURCE") {
        return false;
    }
    matches!(
        symbols.last().map(String::as_str),
        Some(":") | Some("[") | Some("{")
    ) || symbols.windows(2).any(|window| {
        window[0] == ":" && window[1] == "[" && !symbols.iter().any(|lexeme| lexeme == "]")
    })
}

fn ast_expression_operators(symbols: &[String]) -> Vec<String> {
    let refs = symbols.iter().map(String::as_str).collect::<Vec<_>>();
    expression_operators(&refs)
}

fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn detect_program_kind() -> ProgramKind {
    ProgramKind::Generic
}

fn validate_source_syntax(path: &str, ast: &AstProgram) -> Result<(), ParseError> {
    let example_source = path.contains("/examples/") || path.starts_with("examples/");
    let mut text_literal_spans = ast
        .expressions
        .iter()
        .filter_map(|expr| {
            matches!(expr.kind, AstExprKind::TextLiteral(_)).then_some((expr.start, expr.end))
        })
        .collect::<Vec<_>>();
    text_literal_spans.extend(text_literal_token_spans(&ast.tokens));
    for token in &ast.tokens {
        if matches!(token.kind, AstTokenKind::String | AstTokenKind::Comment)
            || text_literal_spans
                .iter()
                .any(|(start, end)| token.start >= *start && token.end <= *end)
        {
            continue;
        }
        if token.lexeme == "EXAMPLE" {
            return Err(error(
                path,
                token.line,
                token.column,
                "`EXAMPLE` is not Boon syntax; put example identity in the manifest/dev metadata",
            ));
        }
        if token.lexeme == "#" {
            return Err(error(
                path,
                token.line,
                token.column,
                "`#` comments are not supported in Boon source; use `--` comments",
            ));
        }
        if token.lexeme == "LINK" {
            return Err(error(
                path,
                token.line,
                token.column,
                "`LINK` is not supported in boon-circuit examples; declare input ports with `SOURCE`",
            ));
        }
        if example_source && matches!(token.lexeme.as_str(), "bg" | "fill" | "true" | "false") {
            return Err(error(
                path,
                token.line,
                token.column,
                "Boon examples must use canonical names such as `background`, `Fill`, `True`, and `False`",
            ));
        }
    }
    for item in &ast.items {
        for window in item.symbols.windows(2) {
            if matches!(window, [pipe, op] if pipe == "|>" && op == "LINK") {
                return Err(error(
                    path,
                    item.line,
                    item.indent + 1,
                    "`|> LINK` is not supported; use `|> SOURCE` for source-port binding",
                ));
            }
        }
    }
    if example_source && let Some(document) = document_statement(ast) {
        let document_is_canonical = document.expr.is_some_and(|expr_id| {
            ast.expressions.get(expr_id).is_some_and(|expr| {
                matches!(&expr.kind, AstExprKind::Call { function, .. } if function == "Document/new")
            })
        });
        if !document_is_canonical || statement_has_field(document, "kind") {
            return Err(error(
                path,
                document.line,
                document.indent + 1,
                "example documents must use `Document/new(root: Element/...)`",
            ));
        }
    }
    validate_drain_syntax(path, ast, &text_literal_spans)?;
    validate_bytes_syntax(path, ast, &text_literal_spans)?;
    Ok(())
}

fn validate_drain_syntax(
    path: &str,
    ast: &AstProgram,
    text_literal_spans: &[(usize, usize)],
) -> Result<(), ParseError> {
    let tokens = ast
        .tokens
        .iter()
        .filter(|token| {
            if matches!(token.kind, AstTokenKind::Comment | AstTokenKind::Newline) {
                return false;
            }
            let containing_text = text_literal_spans
                .iter()
                .find(|(start, end)| token.start >= *start && token.end <= *end);
            containing_text.is_none_or(|(start, _)| token.start == *start)
        })
        .collect::<Vec<_>>();

    let mut index = 0usize;
    while let Some(token) = tokens.get(index) {
        if token.lexeme != "DRAIN" {
            index += 1;
            continue;
        }
        if tokens.get(index + 1).map(|token| token.lexeme.as_str()) != Some("{") {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAIN` requires a `{ path }` body",
            ));
        }
        let Some(close) = matching_semantic_brace(&tokens, index + 1) else {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAIN` is missing closing `}`",
            ));
        };
        let body = tokens[index + 2..close]
            .iter()
            .map(|token| token.lexeme.clone())
            .collect::<Vec<_>>();
        if drain_path_from_symbols(&body).is_none() {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAIN` body must contain exactly one named binding, field path, or `PASSED` path",
            ));
        }
        index = close + 1;
    }

    let depths = semantic_token_depths(&tokens);
    for (index, token) in tokens.iter().enumerate() {
        if token.lexeme != "DRAINING" {
            continue;
        }
        if index
            .checked_sub(1)
            .and_then(|index| tokens.get(index))
            .is_none_or(|previous| previous.lexeme != "|>")
            || !draining_pipe_has_input(&tokens, index)
        {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAINING` must be used as terminal `input |> DRAINING` syntax",
            ));
        }
        if draining_has_trailing_pipeline_syntax(&tokens, &depths, index) {
            return Err(error(
                path,
                token.line,
                token.column,
                "`DRAINING` must be terminal in its pipeline",
            ));
        }
    }
    Ok(())
}

fn matching_semantic_brace(tokens: &[&AstToken], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token.lexeme.as_str() {
            "{" => depth += 1,
            "}" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn semantic_token_depths(tokens: &[&AstToken]) -> Vec<i32> {
    let mut depth = 0i32;
    tokens
        .iter()
        .map(|token| {
            let current = depth;
            match token.lexeme.as_str() {
                "[" | "{" | "(" => depth += 1,
                "]" | "}" | ")" => depth -= 1,
                _ => {}
            }
            current
        })
        .collect()
}

fn draining_pipe_has_input(tokens: &[&AstToken], draining: usize) -> bool {
    let Some(pipe) = draining.checked_sub(1) else {
        return false;
    };
    let Some(previous) = pipe.checked_sub(1).and_then(|index| tokens.get(index)) else {
        return false;
    };
    !matches!(
        previous.lexeme.as_str(),
        ":" | "," | "(" | "[" | "{" | "=>" | "|>"
    )
}

fn draining_has_trailing_pipeline_syntax(
    tokens: &[&AstToken],
    depths: &[i32],
    draining: usize,
) -> bool {
    let Some(marker) = tokens.get(draining) else {
        return false;
    };
    let marker_depth = depths.get(draining).copied().unwrap_or_default();
    let Some(next) = tokens.get(draining + 1) else {
        return false;
    };
    if next.line == marker.line {
        return match next.lexeme.as_str() {
            "," => marker_depth == 0,
            ")" | "]" | "}" => false,
            _ => true,
        };
    }
    let next_depth = depths.get(draining + 1).copied().unwrap_or_default();
    if matches!(next.lexeme.as_str(), ")" | "]" | "}") && next_depth <= marker_depth {
        return false;
    }
    next.lexeme == "|>" && next_depth == marker_depth
}

fn validate_bytes_syntax(
    path: &str,
    ast: &AstProgram,
    text_literal_spans: &[(usize, usize)],
) -> Result<(), ParseError> {
    for window in ast.tokens.windows(2) {
        let [base_token, suffix_token] = window else {
            continue;
        };
        if base_token.line != suffix_token.line
            || base_token.end != suffix_token.start
            || matches!(
                base_token.kind,
                AstTokenKind::Comment | AstTokenKind::String | AstTokenKind::Newline
            )
            || matches!(
                suffix_token.kind,
                AstTokenKind::Comment | AstTokenKind::String | AstTokenKind::Newline
            )
            || text_literal_spans
                .iter()
                .any(|(start, end)| base_token.start >= *start && base_token.end <= *end)
            || text_literal_spans
                .iter()
                .any(|(start, end)| suffix_token.start >= *start && suffix_token.end <= *end)
        {
            continue;
        }
        if !matches!(base_token.kind, AstTokenKind::Number) || !suffix_token.lexeme.starts_with('u')
        {
            continue;
        }
        parse_byte_literal_parts(&base_token.lexeme, &suffix_token.lexeme)
            .map_err(|message| error(path, base_token.line, base_token.column, message.as_str()))?;
    }
    for item in &ast.items {
        validate_bytes_item_syntax(path, ast, item, text_literal_spans)?;
    }
    Ok(())
}

fn validate_bytes_item_syntax(
    path: &str,
    _ast: &AstProgram,
    item: &ParserItem,
    text_literal_spans: &[(usize, usize)],
) -> Result<(), ParseError> {
    for bytes_index in item
        .symbols
        .iter()
        .enumerate()
        .filter_map(|(index, symbol)| (symbol == "BYTES").then_some(index))
    {
        let token_start = item
            .symbol_spans
            .get(bytes_index)
            .map(|(start, _)| *start)
            .unwrap_or(item.start);
        if text_literal_spans
            .iter()
            .any(|(start, end)| token_start >= *start && token_start <= *end)
        {
            continue;
        }
        match item.symbols.get(bytes_index + 1).map(String::as_str) {
            Some("{") => {
                validate_bytes_body_consumption(path, item, bytes_index + 1)?;
            }
            Some("[") => {
                let Some(close) = matching_close(&item.symbols, bytes_index + 1) else {
                    return Err(error(
                        path,
                        item.line,
                        item.indent + 1,
                        "BYTES size is missing closing `]`",
                    ));
                };
                let size_tokens = &item.symbols[bytes_index + 2..close];
                let valid_size = matches!(size_tokens, [value] if value == "__" || value.parse::<usize>().is_ok());
                if !valid_size {
                    return Err(error(
                        path,
                        item.line,
                        item.indent + 1,
                        "BYTES size must be `__` or a non-negative decimal integer in v1",
                    ));
                }
                if item.symbols.get(close + 1).map(String::as_str) != Some("{") {
                    return Err(error(
                        path,
                        item.line,
                        item.indent + 1,
                        "BYTES constructor requires a `{ ... }` body",
                    ));
                }
                validate_bytes_body_consumption(path, item, close + 1)?;
            }
            _ => {
                return Err(error(
                    path,
                    item.line,
                    item.indent + 1,
                    "BYTES constructor requires a `{ ... }` body",
                ));
            }
        }
    }
    Ok(())
}

fn validate_bytes_body_consumption(
    path: &str,
    item: &ParserItem,
    body_open: usize,
) -> Result<(), ParseError> {
    let Some(body_close) = matching_close(&item.symbols, body_open) else {
        return Err(error(
            path,
            item.line,
            item.indent + 1,
            "BYTES constructor is missing closing `}`",
        ));
    };
    let Some(next) = item.symbols.get(body_close + 1) else {
        return Ok(());
    };
    if matches!(next.as_str(), "," | ")" | "]" | "}" | "|>") {
        return Ok(());
    }
    Err(error(
        path,
        item.line,
        item.indent + 1,
        format!("BYTES constructor has unexpected trailing token `{next}`").as_str(),
    ))
}

fn text_literal_token_spans(tokens: &[AstToken]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut index = 0usize;
    while index + 1 < tokens.len() {
        if tokens[index].lexeme == "TEXT" && tokens[index + 1].lexeme == "{" {
            let start = tokens[index].start;
            let mut depth = 0i32;
            let mut cursor = index + 1;
            while cursor < tokens.len() {
                match tokens[cursor].lexeme.as_str() {
                    "{" => depth += 1,
                    "}" => {
                        depth -= 1;
                        if depth == 0 {
                            spans.push((start, tokens[cursor].end));
                            index = cursor;
                            break;
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
        }
        index += 1;
    }
    spans
}

fn text_literal_body_line_ranges(tokens: &[AstToken]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut index = 0usize;
    while index + 1 < tokens.len() {
        if tokens[index].lexeme == "TEXT" && tokens[index + 1].lexeme == "{" {
            let start_line = tokens[index].line;
            let mut depth = 0i32;
            let mut cursor = index + 1;
            while cursor < tokens.len() {
                match tokens[cursor].lexeme.as_str() {
                    "{" => depth += 1,
                    "}" => {
                        depth -= 1;
                        if depth == 0 {
                            let end_line = tokens[cursor].line;
                            if end_line > start_line {
                                ranges.push((start_line + 1, end_line));
                            }
                            index = cursor;
                            break;
                        }
                    }
                    _ => {}
                }
                cursor += 1;
            }
        }
        index += 1;
    }
    ranges
}

fn statement_has_field(statement: &AstStatement, needle: &str) -> bool {
    matches!(&statement.kind, AstStatementKind::Field { name } if name == needle)
        || statement
            .children
            .iter()
            .any(|child| statement_has_field(child, needle))
}

fn validate_balanced_brackets(path: &str, ast: &AstProgram) -> Result<(), ParseError> {
    let mut stack = Vec::new();
    for token in ast.tokens.iter().filter(|token| {
        !matches!(
            token.kind,
            AstTokenKind::Comment | AstTokenKind::String | AstTokenKind::Newline
        )
    }) {
        match token.lexeme.as_str() {
            "[" | "{" | "(" => stack.push((token.lexeme.as_str(), token.line, token.column)),
            "]" if stack.pop().map(|(ch, _, _)| ch) != Some("[") => {
                return Err(error(path, token.line, token.column, "unbalanced `]`"));
            }
            "}" if stack.pop().map(|(ch, _, _)| ch) != Some("{") => {
                return Err(error(path, token.line, token.column, "unbalanced `}`"));
            }
            ")" if stack.pop().map(|(ch, _, _)| ch) != Some("(") => {
                return Err(error(path, token.line, token.column, "unbalanced `)`"));
            }
            "]" | "}" | ")" => {}
            _ => {}
        }
    }
    if stack.is_empty() {
        Ok(())
    } else {
        let (ch, line, column) = stack
            .last()
            .copied()
            .expect("stack is known to be nonempty");
        Err(ParseError {
            path: path.to_owned(),
            line: Some(line),
            column: Some(column),
            message: format!("unclosed `{ch}` at line {line}, column {column}"),
        })
    }
}

fn validate_list_capacities(path: &str, ast: &AstProgram) -> Result<(), ParseError> {
    for line in ast.semantic_parser_lines() {
        let Some(list_index) = line.symbols.iter().position(|lexeme| lexeme == "LIST") else {
            continue;
        };
        if line.symbols.get(list_index + 1).map(String::as_str) != Some("[") {
            continue;
        }
        let capacity_column = ast_token_for_parser_line_symbol(ast, line, list_index + 2)
            .map(|token| token.column)
            .unwrap_or(line.indent + 1);
        let Some(close_offset) = line.symbols[list_index + 2..]
            .iter()
            .position(|lexeme| lexeme == "]")
        else {
            return Err(error(
                path,
                line.line,
                capacity_column,
                "LIST capacity is missing closing `]`",
            ));
        };
        let capacity_parts = &line.symbols[list_index + 2..list_index + 2 + close_offset];
        if capacity_parts.len() != 1
            || capacity_parts
                .first()
                .is_none_or(|capacity| capacity.is_empty())
        {
            return Err(error(
                path,
                line.line,
                capacity_column,
                "LIST capacity must be a positive integer",
            ));
        }
        match capacity_parts[0].parse::<usize>() {
            Ok(value) if value > 0 => {}
            _ => {
                return Err(error(
                    path,
                    line.line,
                    capacity_column,
                    "LIST capacity must be a positive integer",
                ));
            }
        }
    }
    Ok(())
}

fn validate_no_reducer_style_update(path: &str, ast: &AstProgram) -> Result<(), ParseError> {
    if ast.semantic_parser_items().any(reducer_update_signature) {
        return Err(ParseError {
            path: path.to_owned(),
            line: None,
            column: None,
            message: "central reducer `FUNCTION update(state, event)` is not allowed; define local HOLD equations for each value".to_owned(),
        });
    }
    let has_event_source_when = ast
        .semantic_parser_items()
        .any(|item| item.contains_sequence(&["event", ".", "source", "|>", "WHEN"]));
    let has_state_pipe = ast
        .semantic_parser_items()
        .any(|item| item.contains_sequence(&["state", "|>"]));
    if has_event_source_when && has_state_pipe {
        return Err(ParseError {
            path: path.to_owned(),
            line: None,
            column: None,
            message: "global event-source reducer over `state` is not allowed; each value must declare its own sources".to_owned(),
        });
    }
    Ok(())
}

fn reducer_update_signature(item: &ParserItem) -> bool {
    item.function.as_deref() == Some("update")
        && item.has_lexeme("state")
        && item.has_lexeme("event")
}

fn validate_no_hidden_identity_leak(path: &str, ast: &AstProgram) -> Result<(), ParseError> {
    for token in ast.semantic_tokens() {
        if let Some(needle) = hidden_runtime_identity_token(&token.lexeme) {
            return Err(ParseError {
                path: path.to_owned(),
                line: Some(token.line),
                column: Some(token.column),
                message: format!("Boon source exposes hidden runtime identity `{needle}`"),
            });
        }
    }
    for item in ast.semantic_parser_items() {
        if item.field.as_deref() == Some("alive") {
            return Err(ParseError {
                path: path.to_owned(),
                line: Some(item.line),
                column: None,
                message: format!(
                    "Boon source exposes app-visible liveness field `alive` at line {}",
                    item.line
                ),
            });
        }
    }
    Ok(())
}

fn hidden_runtime_identity_token(value: &str) -> Option<&'static str> {
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

fn collect_sources(ast: &AstProgram) -> Vec<String> {
    ast.semantic_parser_items()
        .filter(|item| item.has_lexeme("SOURCE"))
        .map(|item| item.symbols.join(" "))
        .collect()
}

fn expression_operators(symbols: &[&str]) -> Vec<String> {
    let mut operators = Vec::new();
    for lexeme in symbols {
        if is_operator_lexeme(lexeme) && !operators.iter().any(|operator| operator == lexeme) {
            operators.push((*lexeme).to_owned());
        }
    }
    operators
}

fn is_operator_lexeme(lexeme: &str) -> bool {
    matches!(
        lexeme,
        "SOURCE"
            | "DRAIN"
            | "DRAINING"
            | "HOLD"
            | "THEN"
            | "WHEN"
            | "WHILE"
            | "LATEST"
            | "LIST"
            | "BLOCK"
            | "List/map"
            | "List/append"
            | "List/range"
            | "List/get"
            | "List/find"
            | "List/find_value"
            | "List/filter_text_contains"
            | "List/filter_field_equal"
            | "List/filter_field_not_equal"
            | "List/move_field_first"
            | "List/move_field_last"
            | "List/join_field"
            | "List/chunk"
            | "List/remove"
            | "List/retain"
            | "List/count"
            | "List/length"
            | "List/sum"
            | "List/every"
            | "List/any"
            | "List/is_not_empty"
            | "List/latest"
            | "Text/empty"
            | "Text/concat"
            | "Text/time_range_label"
            | "Text/trim"
            | "Text/to_uppercase"
            | "Text/substring"
            | "Text/length"
            | "Text/find"
            | "Text/contains"
            | "Text/starts_with"
            | "Text/all_chars_in"
            | "Text/to_number"
            | "Text/to_bytes"
            | "Text/is_empty"
            | "Text/is_not_empty"
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
            | "Bytes/from_hex"
            | "Bytes/to_hex"
            | "Bytes/from_base64"
            | "Bytes/to_base64"
            | "Bytes/read_unsigned"
            | "Bytes/read_signed"
            | "Bytes/write_unsigned"
            | "Bytes/write_signed"
            | "File/read_bytes"
            | "File/read_text"
            | "File/write_bytes"
            | "Number/bit_width"
            | "Number/to_text"
            | "Number/to_codepoint_text"
            | "Number/to_ascii_text"
            | "Number/interpolate"
            | "Number/project_width"
            | "Number/project_offset"
            | "Number/project_time"
            | "Bool/not"
            | "Bool/and"
            | "Bool/toggle"
            | "Timer/interval"
            | "Router/route"
            | "Router/go_to"
            | "Ulid/generate"
            | "Light/directional"
            | "Light/ambient"
            | "Light/spot"
            | "Error/new"
            | "Error/text"
    )
}

fn collect_named_statements(ast: &AstProgram, needle: &str) -> Vec<String> {
    ast.semantic_parser_items()
        .filter(|item| item.has_lexeme(needle))
        .map(parser_item_summary)
        .collect()
}

fn ast_token_for_parser_line_symbol<'a>(
    ast: &'a AstProgram,
    line: &ParserLine,
    lexeme_index: usize,
) -> Option<&'a AstToken> {
    ast.semantic_tokens()
        .filter(|token| token.line == line.line)
        .nth(lexeme_index)
}

#[derive(Default)]
struct StructureTables {
    source_ports: Vec<ParsedSourcePort>,
    state_cells: Vec<ParsedStateCell>,
    list_memories: Vec<ParsedListMemory>,
}

fn derive_program_tables(
    ast: &AstProgram,
    row_scopes: &[ParsedRowScopeFunction],
) -> StructureTables {
    let mut tables = StructureTables::default();
    let function_bodies = function_body_index(&ast.statements);
    let inferred_list_memory_names = collect_list_memory_names(ast);
    derive_structure_from_statements(
        &ast.statements,
        &ast.expressions,
        &ast.lines,
        row_scopes,
        &function_bodies,
        &mut Vec::new(),
        &mut Vec::new(),
        &mut tables,
    );
    add_missing_row_scope_list_memories(&mut tables, row_scopes);
    let published_list_memory_names = tables
        .list_memories
        .iter()
        .map(|list| list.name.clone())
        .collect::<BTreeSet<_>>();
    add_inferred_list_memories(
        &mut tables,
        ast,
        &inferred_list_memory_names,
        &published_list_memory_names,
    );
    tables
}

fn add_inferred_list_memories(
    tables: &mut StructureTables,
    ast: &AstProgram,
    candidate_names: &BTreeSet<String>,
    published_list_names: &BTreeSet<String>,
) {
    for name in candidate_names {
        if matches!(name.as_str(), "store" | "document" | "scene")
            || tables.list_memories.iter().any(|list| &list.name == name)
        {
            continue;
        }
        let Some(statement) = list_memory_statement(ast, name) else {
            continue;
        };
        if !statement_returns_existing_list_from_branch(statement, ast, published_list_names) {
            continue;
        }
        tables.list_memories.push(ParsedListMemory {
            name: name.clone(),
            line: statement.line,
            capacity: None,
        });
    }
}

fn list_memory_statement<'a>(ast: &'a AstProgram, name: &str) -> Option<&'a AstStatement> {
    list_memory_statement_in_statements(&ast.statements, name)
}

fn list_memory_statement_in_statements<'a>(
    statements: &'a [AstStatement],
    name: &str,
) -> Option<&'a AstStatement> {
    for statement in statements {
        match &statement.kind {
            AstStatementKind::Field { name: field }
            | AstStatementKind::List {
                field: Some(field), ..
            } if field == name => return Some(statement),
            _ => {}
        }
        if let Some(statement) = list_memory_statement_in_statements(&statement.children, name) {
            return Some(statement);
        }
    }
    None
}

fn statement_returns_existing_list_from_branch(
    statement: &AstStatement,
    ast: &AstProgram,
    list_names: &BTreeSet<String>,
) -> bool {
    statement.children.iter().any(|child| {
        statement_returns_existing_list_from_branch_inner(child, ast, list_names, false)
    })
}

fn statement_returns_existing_list_from_branch_inner(
    statement: &AstStatement,
    ast: &AstProgram,
    list_names: &BTreeSet<String>,
    allow_list_reference: bool,
) -> bool {
    let is_when = statement
        .expr
        .and_then(|expr_id| ast.expressions.get(expr_id))
        .is_some_and(|expr| {
            matches!(expr.kind, AstExprKind::When { .. })
                || matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "WHEN")
        });
    if allow_list_reference
        && statement.expr.is_some_and(|expr_id| {
            expr_returns_list_collection_inner(expr_id, ast, list_names, true)
        })
    {
        return true;
    }
    statement.children.iter().any(|child| {
        statement_returns_existing_list_from_branch_inner(
            child,
            ast,
            list_names,
            allow_list_reference || is_when,
        )
    })
}

fn add_missing_row_scope_list_memories(
    tables: &mut StructureTables,
    row_scopes: &[ParsedRowScopeFunction],
) {
    for row_scope in row_scopes {
        if matches!(row_scope.list.as_str(), "items" | "children") {
            continue;
        }
        if !tables
            .list_memories
            .iter()
            .any(|list| list.name == row_scope.list)
        {
            tables.list_memories.push(ParsedListMemory {
                name: row_scope.list.clone(),
                line: 0,
                capacity: None,
            });
        }
    }
}

fn function_body_index(statements: &[AstStatement]) -> BTreeMap<&str, &[AstStatement]> {
    let mut functions = BTreeMap::new();
    collect_function_body_index(statements, &mut functions);
    functions
}

fn collect_function_body_index<'a>(
    statements: &'a [AstStatement],
    functions: &mut BTreeMap<&'a str, &'a [AstStatement]>,
) {
    for statement in statements {
        if let AstStatementKind::Function { name, .. } = &statement.kind {
            functions.insert(name.as_str(), statement.children.as_slice());
        }
        collect_function_body_index(&statement.children, functions);
    }
}

fn collect_row_scope_functions(
    ast: &AstProgram,
    include_append_constructors: bool,
    list_memory_names: &BTreeSet<String>,
) -> Vec<ParsedRowScopeFunction> {
    let mut functions = Vec::new();
    collect_row_scope_statements(
        &ast.statements,
        ast,
        include_append_constructors,
        list_memory_names,
        &mut Vec::new(),
        &mut functions,
    );
    functions
}

fn collect_list_memory_names(ast: &AstProgram) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    collect_list_memory_name_statements(ast, &ast.statements, &mut Vec::new(), &mut names);
    names
}

fn collect_list_memory_name_statements(
    ast: &AstProgram,
    statements: &[AstStatement],
    scope: &mut Vec<String>,
    names: &mut BTreeSet<String>,
) {
    for statement in statements {
        match &statement.kind {
            AstStatementKind::List {
                field: Some(name), ..
            } => {
                names.insert(name.clone());
                scope.push(name.clone());
                collect_list_memory_name_statements(ast, &statement.children, scope, names);
                scope.pop();
            }
            AstStatementKind::List { field: None, .. } => {
                if let Some(name) = scope.last()
                    && !scope_names_nested_render_slot(scope)
                {
                    names.insert(name.clone());
                }
                collect_list_memory_name_statements(ast, &statement.children, scope, names);
            }
            AstStatementKind::Field { name } => {
                if name != "document"
                    && !field_name_is_nested_render_slot(name, scope)
                    && (statement
                        .expr
                        .is_some_and(|expr_id| expr_returns_list_collection(expr_id, ast, names))
                        || statement_children_return_list_collection_with_names(
                            statement, ast, names,
                        ))
                {
                    names.insert(name.clone());
                }
                if !statement.children.is_empty() && name != "document" {
                    scope.push(name.clone());
                    collect_list_memory_name_statements(ast, &statement.children, scope, names);
                    scope.pop();
                }
            }
            AstStatementKind::Function { .. }
            | AstStatementKind::Source { .. }
            | AstStatementKind::Hold { .. }
            | AstStatementKind::Block
            | AstStatementKind::Spread
            | AstStatementKind::Expression => {
                collect_list_memory_name_statements(ast, &statement.children, scope, names);
            }
        }
    }
}

fn field_name_is_nested_render_slot(name: &str, scope: &[String]) -> bool {
    matches!(name, "items" | "children") && !scope.is_empty()
}

fn scope_names_nested_render_slot(scope: &[String]) -> bool {
    scope
        .last()
        .is_some_and(|name| matches!(name.as_str(), "items" | "children"))
        && scope.len() > 1
}

#[allow(clippy::too_many_arguments)]
fn derive_structure_from_statements(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    lines: &[ParserLine],
    row_scopes: &[ParsedRowScopeFunction],
    function_bodies: &BTreeMap<&str, &[AstStatement]>,
    function_stack: &mut Vec<String>,
    scope: &mut Vec<String>,
    tables: &mut StructureTables,
) {
    for statement in statements {
        derive_structure_from_called_functions(
            statement,
            expressions,
            lines,
            row_scopes,
            function_bodies,
            function_stack,
            scope,
            tables,
        );
        match &statement.kind {
            AstStatementKind::Function { name, .. } => {
                let function_row_scopes = row_scopes_for_function(row_scopes, name)
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                for row_scope in function_row_scopes {
                    scope.push(row_scope);
                    function_stack.push(name.clone());
                    if function_body_defines_record_fields(&statement.children, expressions, lines)
                    {
                        derive_structure_from_statements(
                            &statement.children,
                            expressions,
                            lines,
                            row_scopes,
                            function_bodies,
                            function_stack,
                            scope,
                            tables,
                        );
                    } else {
                        derive_structure_from_called_functions_in_statements(
                            &statement.children,
                            expressions,
                            lines,
                            row_scopes,
                            function_bodies,
                            function_stack,
                            scope,
                            tables,
                        );
                    }
                    function_stack.pop();
                    scope.pop();
                }
            }
            AstStatementKind::Field { name } => {
                if name == "document" {
                    continue;
                }
                if scope.is_empty()
                    && matches!(name.as_str(), "items" | "children")
                    && field_statement_contains_list_literal(statement, expressions)
                    && !tables.list_memories.iter().any(|list| list.name == *name)
                {
                    tables.list_memories.push(ParsedListMemory {
                        name: name.clone(),
                        line: statement.line,
                        capacity: None,
                    });
                }
                if scope_is_indexed(scope, row_scopes)
                    && statement_direct_stateful_operator(statement, expressions)
                {
                    let path = join_path(scope, [name.as_str()]);
                    if !tables.state_cells.iter().any(|cell| cell.path == path) {
                        push_state_cell(
                            tables,
                            ParsedStateCell {
                                indexed: true,
                                hold_name: path.clone(),
                                path,
                                line: statement.line,
                            },
                        );
                    }
                }
                collect_source_ports_from_statement_expr(
                    statement,
                    expressions,
                    scope,
                    row_scopes,
                    tables,
                );
                if !statement.children.is_empty() {
                    scope.push(name.clone());
                    derive_structure_from_statements(
                        &statement.children,
                        expressions,
                        lines,
                        row_scopes,
                        function_bodies,
                        function_stack,
                        scope,
                        tables,
                    );
                    scope.pop();
                }
            }
            AstStatementKind::Source { field, event } => {
                let collected_from_expr = collect_source_ports_from_statement_expr(
                    statement,
                    expressions,
                    scope,
                    row_scopes,
                    tables,
                );
                if !collected_from_expr && let Some(field) = field.as_deref() {
                    let source_scope = source_scope_without_events(scope);
                    let path = match event.as_deref() {
                        Some(event) => join_path(&source_scope, [field, event]),
                        None => join_path(&source_scope, [field]),
                    };
                    push_source_port(
                        tables,
                        ParsedSourcePort {
                            path,
                            line: statement.line,
                            scoped: source_scope_is_scoped(scope, row_scopes),
                            interval_ms: None,
                        },
                    );
                }
                derive_structure_from_statements(
                    &statement.children,
                    expressions,
                    lines,
                    row_scopes,
                    function_bodies,
                    function_stack,
                    scope,
                    tables,
                );
            }
            AstStatementKind::Hold { field, name } => {
                let path = field
                    .as_ref()
                    .map(|field| join_path(scope, [field.as_str()]))
                    .or_else(|| scope_path(scope))
                    .unwrap_or_else(|| format!("hold_{}", statement.line));
                push_state_cell(
                    tables,
                    ParsedStateCell {
                        indexed: scope_is_indexed(scope, row_scopes),
                        hold_name: name.clone().unwrap_or_else(|| path.clone()),
                        path,
                        line: statement.line,
                    },
                );
                derive_structure_from_statements(
                    &statement.children,
                    expressions,
                    lines,
                    row_scopes,
                    function_bodies,
                    function_stack,
                    scope,
                    tables,
                );
            }
            AstStatementKind::List { field, capacity } => {
                let name = match field.as_deref() {
                    Some("items" | "children") if scope_is_indexed(scope, row_scopes) => {
                        generated_local_list_memory_name(
                            scope,
                            field.as_deref(),
                            statement.line,
                            tables,
                        )
                    }
                    Some(name) => name.to_owned(),
                    None => anonymous_list_memory_name(scope, statement.line, tables),
                };
                tables.list_memories.push(ParsedListMemory {
                    name,
                    line: statement.line,
                    capacity: *capacity,
                });
                derive_structure_from_statements(
                    &statement.children,
                    expressions,
                    lines,
                    row_scopes,
                    function_bodies,
                    function_stack,
                    scope,
                    tables,
                );
            }
            AstStatementKind::Block | AstStatementKind::Spread | AstStatementKind::Expression => {
                derive_structure_from_statements(
                    &statement.children,
                    expressions,
                    lines,
                    row_scopes,
                    function_bodies,
                    function_stack,
                    scope,
                    tables,
                );
            }
        }
    }
}

fn field_statement_contains_list_literal(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> bool {
    statement
        .expr
        .is_some_and(|expr_id| expr_id_contains_list_literal(expr_id, expressions))
        || statement
            .children
            .iter()
            .any(|child| field_statement_contains_list_literal(child, expressions))
}

fn expr_id_contains_list_literal(expr_id: usize, expressions: &[AstExpr]) -> bool {
    let Some(expr) = expressions.get(expr_id) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::ListLiteral { .. } => true,
        AstExprKind::Draining { input } => expr_id_contains_list_literal(*input, expressions),
        AstExprKind::Pipe { input, args, .. } => {
            expr_id_contains_list_literal(*input, expressions)
                || args
                    .iter()
                    .any(|arg| expr_id_contains_list_literal(arg.value, expressions))
        }
        AstExprKind::Call { args, .. } => args
            .iter()
            .any(|arg| expr_id_contains_list_literal(arg.value, expressions)),
        _ => false,
    }
}

fn anonymous_list_memory_name(scope: &[String], line: usize, tables: &StructureTables) -> String {
    let scoped_candidate = scope.last().cloned();
    let candidate_is_generic_render_slot = scoped_candidate
        .as_deref()
        .is_some_and(|name| matches!(name, "items" | "children"));
    if let Some(candidate) = scoped_candidate
        && !candidate_is_generic_render_slot
        && !tables
            .list_memories
            .iter()
            .any(|list| list.name == candidate)
    {
        return candidate;
    }

    let scope_label = if scope.is_empty() {
        "list".to_owned()
    } else {
        sanitize_generated_list_name(&scope.join("_"))
    };
    unique_generated_list_name(format!("{scope_label}_list_{line}"), tables)
}

fn generated_local_list_memory_name(
    scope: &[String],
    local_name: Option<&str>,
    line: usize,
    tables: &StructureTables,
) -> String {
    let mut parts = scope.to_vec();
    if let Some(local_name) = local_name {
        parts.push(local_name.to_owned());
    }
    let scope_label = if parts.is_empty() {
        "list".to_owned()
    } else {
        sanitize_generated_list_name(&parts.join("_"))
    };
    unique_generated_list_name(format!("{scope_label}_list_{line}"), tables)
}

fn unique_generated_list_name(base: String, tables: &StructureTables) -> String {
    if !tables.list_memories.iter().any(|list| list.name == base) {
        return base;
    }
    let mut index = 1usize;
    loop {
        let candidate = format!("{base}_{index}");
        if !tables
            .list_memories
            .iter()
            .any(|list| list.name == candidate)
        {
            return candidate;
        }
        index += 1;
    }
}

fn sanitize_generated_list_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "anonymous".to_owned()
    } else {
        sanitized
    }
}

#[allow(clippy::too_many_arguments)]
fn derive_structure_from_called_functions(
    statement: &AstStatement,
    expressions: &[AstExpr],
    lines: &[ParserLine],
    row_scopes: &[ParsedRowScopeFunction],
    function_bodies: &BTreeMap<&str, &[AstStatement]>,
    function_stack: &mut Vec<String>,
    scope: &mut Vec<String>,
    tables: &mut StructureTables,
) {
    if !scope_is_indexed(scope, row_scopes) {
        return;
    }
    let Some(expr_id) = statement.expr else {
        return;
    };
    let mut calls = Vec::new();
    collect_called_functions(expr_id, expressions, &mut calls);
    for function in calls {
        derive_structure_from_helper_function(
            &function,
            expressions,
            lines,
            row_scopes,
            function_bodies,
            function_stack,
            scope,
            tables,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn derive_structure_from_helper_function(
    function: &str,
    expressions: &[AstExpr],
    lines: &[ParserLine],
    row_scopes: &[ParsedRowScopeFunction],
    function_bodies: &BTreeMap<&str, &[AstStatement]>,
    function_stack: &mut Vec<String>,
    scope: &mut Vec<String>,
    tables: &mut StructureTables,
) {
    if function_stack.iter().any(|entry| entry == function) {
        return;
    }
    let Some(children) = function_bodies.get(function) else {
        return;
    };
    if function_has_row_scope(row_scopes, function) {
        return;
    }
    function_stack.push(function.to_owned());
    if function_body_defines_record_fields(children, expressions, lines) {
        derive_structure_from_statements(
            children,
            expressions,
            lines,
            row_scopes,
            function_bodies,
            function_stack,
            scope,
            tables,
        );
    } else {
        derive_structure_from_called_functions_in_statements(
            children,
            expressions,
            lines,
            row_scopes,
            function_bodies,
            function_stack,
            scope,
            tables,
        );
    }
    function_stack.pop();
}

#[allow(clippy::too_many_arguments)]
fn derive_structure_from_called_functions_in_statements(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    lines: &[ParserLine],
    row_scopes: &[ParsedRowScopeFunction],
    function_bodies: &BTreeMap<&str, &[AstStatement]>,
    function_stack: &mut Vec<String>,
    scope: &mut Vec<String>,
    tables: &mut StructureTables,
) {
    for statement in statements {
        if let Some(expr_id) = statement.expr {
            let mut calls = Vec::new();
            collect_called_functions(expr_id, expressions, &mut calls);
            for function in calls {
                derive_structure_from_helper_function(
                    &function,
                    expressions,
                    lines,
                    row_scopes,
                    function_bodies,
                    function_stack,
                    scope,
                    tables,
                );
            }
        }
        derive_structure_from_called_functions_in_statements(
            &statement.children,
            expressions,
            lines,
            row_scopes,
            function_bodies,
            function_stack,
            scope,
            tables,
        );
    }
}

fn function_body_defines_record_fields(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    lines: &[ParserLine],
) -> bool {
    statements.iter().any(|statement| {
        if statement
            .expr
            .and_then(|expr_id| expressions.get(expr_id))
            .is_some_and(expr_is_render_constructor_like)
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

fn expr_is_render_constructor_like(expr: &AstExpr) -> bool {
    match &expr.kind {
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. } => {
            function == "Document/new"
                || function == "Scene/new"
                || function.starts_with("Element/")
                || function.starts_with("Scene/Element/")
        }
        _ => false,
    }
}

fn statement_is_record_constructor_block(statement: &AstStatement, lines: &[ParserLine]) -> bool {
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

fn collect_called_functions(expr_id: usize, expressions: &[AstExpr], calls: &mut Vec<String>) {
    let Some(expr) = expressions.get(expr_id) else {
        return;
    };
    match &expr.kind {
        AstExprKind::Call { function, args } => {
            calls.push(function.clone());
            for arg in args {
                collect_called_functions(arg.value, expressions, calls);
            }
        }
        AstExprKind::Pipe { input, op, args } => {
            collect_called_functions(*input, expressions, calls);
            calls.push(op.clone());
            for arg in args {
                collect_called_functions(arg.value, expressions, calls);
            }
        }
        AstExprKind::Draining { input } => {
            collect_called_functions(*input, expressions, calls);
        }
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial } => {
            collect_called_functions(*initial, expressions, calls);
        }
        AstExprKind::Then { input, output } => {
            collect_called_functions(*input, expressions, calls);
            if let Some(output) = output {
                collect_called_functions(*output, expressions, calls);
            }
        }
        AstExprKind::Infix { left, right, .. } => {
            collect_called_functions(*left, expressions, calls);
            collect_called_functions(*right, expressions, calls);
        }
        AstExprKind::MatchArm { output, .. } => {
            if let Some(output) = output {
                collect_called_functions(*output, expressions, calls);
            }
        }
        AstExprKind::Object(fields) | AstExprKind::Record(fields) => {
            for field in fields {
                collect_called_functions(field.value, expressions, calls);
            }
        }
        AstExprKind::TaggedObject { fields, .. } => {
            for field in fields {
                collect_called_functions(field.value, expressions, calls);
            }
        }
        AstExprKind::ListLiteral { items, .. } => {
            for item in items {
                collect_called_functions(*item, expressions, calls);
            }
        }
        AstExprKind::BytesLiteral { items, .. } => {
            for item in items {
                collect_called_functions(*item, expressions, calls);
            }
        }
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::Drain { .. }
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::ByteLiteral { .. }
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => {}
    }
}

fn push_source_port(tables: &mut StructureTables, source: ParsedSourcePort) {
    if !tables
        .source_ports
        .iter()
        .any(|existing| existing.path == source.path)
    {
        tables.source_ports.push(source);
    }
}

fn push_state_cell(tables: &mut StructureTables, cell: ParsedStateCell) {
    if !tables
        .state_cells
        .iter()
        .any(|existing| existing.path == cell.path)
    {
        tables.state_cells.push(cell);
    }
}

fn statement_direct_stateful_operator(statement: &AstStatement, expressions: &[AstExpr]) -> bool {
    statement
        .expr
        .and_then(|expr_id| expressions.get(expr_id))
        .is_some_and(|expr| expr_is_stateful_statement_expr(expr, statement, expressions))
        || statement
            .children
            .iter()
            .any(|child| child_statement_is_stateful(child, expressions))
}

fn child_statement_is_stateful(statement: &AstStatement, expressions: &[AstExpr]) -> bool {
    statement
        .expr
        .and_then(|expr_id| expressions.get(expr_id))
        .is_some_and(|expr| expr_is_stateful_statement_expr(expr, statement, expressions))
}

fn expr_is_stateful_statement_expr(
    expr: &AstExpr,
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> bool {
    match &expr.kind {
        AstExprKind::Latest => latest_statement_has_initial(statement, expressions),
        AstExprKind::Pipe { op, .. } => matches!(op.as_str(), "Bool/toggle" | "List/latest"),
        _ => false,
    }
}

fn latest_statement_has_initial(statement: &AstStatement, expressions: &[AstExpr]) -> bool {
    let Some(first) = statement.children.first() else {
        return false;
    };
    if statement_has_then_or_when_continuation(first, expressions) {
        return false;
    }
    let Some(expr) = first.expr.and_then(|expr_id| expressions.get(expr_id)) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::Then { .. } | AstExprKind::When { .. } => false,
        AstExprKind::Pipe { op, .. } if op == "THEN" || op == "WHEN" => false,
        _ => true,
    }
}

fn statement_has_then_or_when_continuation(
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> bool {
    statement.children.iter().any(|child| {
        child
            .expr
            .and_then(|expr_id| expressions.get(expr_id))
            .is_some_and(|expr| match &expr.kind {
                AstExprKind::Then { .. } | AstExprKind::When { .. } => true,
                AstExprKind::Pipe { op, .. } if op == "THEN" || op == "WHEN" => true,
                _ => false,
            })
    })
}

fn collect_source_ports_from_statement_expr(
    statement: &AstStatement,
    expressions: &[AstExpr],
    scope: &[String],
    row_scopes: &[ParsedRowScopeFunction],
    tables: &mut StructureTables,
) -> bool {
    let Some(expr_id) = statement.expr else {
        return false;
    };
    let mut expr_scope = scope.to_vec();
    match &statement.kind {
        AstStatementKind::Field { name }
        | AstStatementKind::Source {
            field: Some(name), ..
        } => {
            expr_scope.push(name.clone());
        }
        _ => {}
    }
    let before = tables.source_ports.len();
    let scoped = source_scope_is_scoped(&expr_scope, row_scopes);
    collect_source_ports_from_expr(
        expr_id,
        expressions,
        &mut expr_scope,
        statement.line,
        scoped,
        tables,
    );
    tables.source_ports.len() > before
}

fn collect_source_ports_from_expr(
    expr_id: usize,
    expressions: &[AstExpr],
    scope: &mut Vec<String>,
    line: usize,
    scoped: bool,
    tables: &mut StructureTables,
) {
    let Some(expr) = expressions.get(expr_id) else {
        return;
    };
    match &expr.kind {
        AstExprKind::Source => {
            let source_scope = source_scope_without_events(scope);
            if let Some(path) = scope_path(&source_scope) {
                tables.source_ports.push(ParsedSourcePort {
                    path,
                    line,
                    scoped,
                    interval_ms: None,
                });
            }
        }
        AstExprKind::Pipe { input, op, .. } if op == "Timer/interval" => {
            if let (Some(path), Some(interval_ms)) = (
                scope_path(scope),
                duration_milliseconds(*input, expressions),
            ) {
                push_source_port(
                    tables,
                    ParsedSourcePort {
                        path,
                        line,
                        scoped,
                        interval_ms: Some(interval_ms),
                    },
                );
            }
        }
        AstExprKind::Object(fields)
        | AstExprKind::Record(fields)
        | AstExprKind::TaggedObject { fields, .. } => {
            for field in fields {
                scope.push(field.name.clone());
                collect_source_ports_from_expr(
                    field.value,
                    expressions,
                    scope,
                    line,
                    scoped,
                    tables,
                );
                scope.pop();
            }
        }
        _ => {}
    }
}

fn duration_milliseconds(expr_id: usize, expressions: &[AstExpr]) -> Option<u64> {
    let AstExprKind::TaggedObject { tag, fields } = &expressions.get(expr_id)?.kind else {
        return None;
    };
    if tag != "Duration" {
        return None;
    }
    let (scale, value) = fields.iter().find_map(|field| {
        let scale = match field.name.as_str() {
            "milliseconds" => 1.0,
            "seconds" => 1_000.0,
            _ => return None,
        };
        let AstExprKind::Number(value) = &expressions.get(field.value)?.kind else {
            return None;
        };
        Some((scale, value.parse::<f64>().ok()?))
    })?;
    let milliseconds = value * scale;
    (milliseconds.is_finite() && milliseconds >= 1.0 && milliseconds <= u64::MAX as f64)
        .then_some(milliseconds.round() as u64)
}

fn collect_row_scope_statements(
    statements: &[AstStatement],
    ast: &AstProgram,
    include_append_constructors: bool,
    list_memory_names: &BTreeSet<String>,
    scope: &mut Vec<String>,
    functions: &mut Vec<ParsedRowScopeFunction>,
) {
    let mut previous_collection_list = None;
    for statement in statements {
        if let Some(row_scope_function) = statement_row_scope_function(
            statement,
            ast,
            scope,
            previous_collection_list.as_deref(),
            include_append_constructors,
        ) && !matches!(row_scope_function.list.as_str(), "items" | "children")
            && list_memory_names.contains(&row_scope_function.list)
            && !functions.iter().any(|existing| {
                existing.list == row_scope_function.list
                    && existing.row_scope == row_scope_function.row_scope
                    && existing.function == row_scope_function.function
            })
        {
            push_row_scope_function(functions, row_scope_function, ast);
        }
        let updates_collection_context = !matches!(
            &statement.kind,
            AstStatementKind::List { field: None, .. }
        ) && !matches!(
            &statement.kind,
            AstStatementKind::Field { name } if matches!(name.as_str(), "items" | "children")
        );
        if let Some(expr_id) = statement.expr
            && let Some(list) =
                statement_collection_list_name(expr_id, ast, previous_collection_list.as_deref())
            && updates_collection_context
        {
            previous_collection_list = Some(list);
        }
        match &statement.kind {
            AstStatementKind::Field { name } => {
                if name == "document" {
                    continue;
                }
                if !statement.children.is_empty() {
                    scope.push(name.clone());
                    collect_row_scope_statements(
                        &statement.children,
                        ast,
                        include_append_constructors,
                        list_memory_names,
                        scope,
                        functions,
                    );
                    scope.pop();
                }
            }
            AstStatementKind::Function { .. } => {
                collect_row_scope_statements(
                    &statement.children,
                    ast,
                    include_append_constructors,
                    list_memory_names,
                    scope,
                    functions,
                );
            }
            AstStatementKind::List {
                field: Some(name), ..
            } => {
                scope.push(name.clone());
                collect_row_scope_statements(
                    &statement.children,
                    ast,
                    include_append_constructors,
                    list_memory_names,
                    scope,
                    functions,
                );
                scope.pop();
            }
            AstStatementKind::Block
            | AstStatementKind::Spread
            | AstStatementKind::Expression
            | AstStatementKind::Hold { .. }
            | AstStatementKind::List { field: None, .. }
            | AstStatementKind::Source { .. } => {
                collect_row_scope_statements(
                    &statement.children,
                    ast,
                    include_append_constructors,
                    list_memory_names,
                    scope,
                    functions,
                );
            }
        }
    }
}

fn push_row_scope_function(
    functions: &mut Vec<ParsedRowScopeFunction>,
    mut row_scope_function: ParsedRowScopeFunction,
    ast: &AstProgram,
) {
    let candidate_defines_runtime_fields =
        row_scope_function_defines_runtime_fields(&row_scope_function.function, ast);
    while let Some(existing_index) = functions.iter().position(|existing| {
        existing.row_scope == row_scope_function.row_scope
            && existing.list != row_scope_function.list
    }) {
        let existing_defines_runtime_fields =
            row_scope_function_defines_runtime_fields(&functions[existing_index].function, ast);
        match (
            candidate_defines_runtime_fields,
            existing_defines_runtime_fields,
        ) {
            (true, false) => {
                functions.remove(existing_index);
            }
            (false, true) => {
                return;
            }
            _ => {
                row_scope_function.row_scope =
                    unique_row_scope_name(&row_scope_function.list, functions);
            }
        }
    }
    functions.push(row_scope_function);
}

fn row_scope_function_defines_runtime_fields(function: &str, ast: &AstProgram) -> bool {
    let mut function_bodies = BTreeMap::new();
    collect_function_body_index(&ast.statements, &mut function_bodies);
    function_defines_runtime_fields_transitively(
        function,
        &function_bodies,
        &ast.expressions,
        &mut Vec::new(),
    )
}

fn function_defines_runtime_fields_transitively(
    function: &str,
    function_bodies: &BTreeMap<&str, &[AstStatement]>,
    expressions: &[AstExpr],
    stack: &mut Vec<String>,
) -> bool {
    if stack.iter().any(|entry| entry == function) {
        return false;
    }
    let Some(children) = function_bodies.get(function) else {
        return false;
    };
    if function_body_defines_runtime_fields(children) {
        return true;
    }
    stack.push(function.to_owned());
    let mut calls = Vec::new();
    collect_called_functions_in_statements(children, expressions, &mut calls);
    let found = calls.iter().any(|call| {
        function_defines_runtime_fields_transitively(call, function_bodies, expressions, stack)
    });
    stack.pop();
    found
}

fn function_body_defines_runtime_fields(statements: &[AstStatement]) -> bool {
    statements.iter().any(|statement| {
        matches!(
            statement.kind,
            AstStatementKind::Source { .. }
                | AstStatementKind::Hold { .. }
                | AstStatementKind::List { field: Some(_), .. }
        ) || function_body_defines_runtime_fields(&statement.children)
    })
}

fn collect_called_functions_in_statements(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    calls: &mut Vec<String>,
) {
    for statement in statements {
        if let Some(expr_id) = statement.expr {
            collect_called_functions(expr_id, expressions, calls);
        }
        collect_called_functions_in_statements(&statement.children, expressions, calls);
    }
}

fn unique_row_scope_name(list: &str, functions: &[ParsedRowScopeFunction]) -> String {
    let base = singular_row_scope(list);
    if !functions.iter().any(|scope| scope.row_scope == base) {
        return base;
    }
    let mut index = 1usize;
    loop {
        let candidate = format!("{base}_{index}");
        if !functions.iter().any(|scope| scope.row_scope == candidate) {
            return candidate;
        }
        index += 1;
    }
}

fn statement_row_scope_function(
    statement: &AstStatement,
    ast: &AstProgram,
    scope: &[String],
    previous_collection_list: Option<&str>,
    include_append_constructors: bool,
) -> Option<ParsedRowScopeFunction> {
    let expr = ast.expressions.get(statement.expr?)?;
    match &expr.kind {
        AstExprKind::Pipe { input, op, args }
            if op == "List/map" || (include_append_constructors && op == "List/append") =>
        {
            let output_list = statement_row_scope_output_list(statement, op);
            let input_list = collection_list_name(*input, ast);
            let parent_storage_scope = scope
                .last()
                .is_some_and(|name| !matches!(name.as_str(), "items" | "children"));
            let in_render_slot_scope = scope
                .last()
                .is_some_and(|name| matches!(name.as_str(), "items" | "children"));
            let scoped_output_list = (op == "List/map" && parent_storage_scope)
                .then(|| scope.last().cloned())
                .flatten();
            match &statement.kind {
                AstStatementKind::Field { name }
                    if matches!(name.as_str(), "items" | "children") =>
                {
                    return None;
                }
                AstStatementKind::Field { .. } if output_list.is_some() => {}
                AstStatementKind::List { field: Some(_), .. } => {}
                AstStatementKind::List { field: None, .. } if parent_storage_scope => {}
                AstStatementKind::Field { .. } if previous_collection_list.is_some() => {}
                AstStatementKind::Expression
                    if !in_render_slot_scope
                        && (input_list.is_some()
                            || previous_collection_list.is_some()
                            || parent_storage_scope) => {}
                _ => return None,
            }
            let list = output_list
                .or(scoped_output_list)
                .or(input_list)
                .or_else(|| previous_collection_list.map(str::to_owned))
                .or_else(|| scope.last().cloned())?;
            let function = if op == "List/map" {
                args.iter()
                    .find(|arg| arg.name.as_deref() == Some("new"))
                    .and_then(|arg| function_name_from_expr(arg.value, ast))
            } else {
                args.iter()
                    .find(|arg| arg.name.as_deref() == Some("item"))
                    .and_then(|arg| function_name_from_expr(arg.value, ast))
            }?;
            let row_scope = if op == "List/map" {
                list_map_binding_name(args, ast).unwrap_or_else(|| singular_row_scope(&list))
            } else {
                singular_row_scope(&list)
            };
            Some(ParsedRowScopeFunction {
                function,
                list,
                row_scope,
            })
        }
        _ => None,
    }
}

fn statement_children_return_list_collection_with_names(
    statement: &AstStatement,
    ast: &AstProgram,
    list_names: &BTreeSet<String>,
) -> bool {
    statement
        .children
        .iter()
        .any(|child| statement_returns_list_collection_with_names(child, ast, list_names, false))
}

fn statement_returns_list_collection_with_names(
    statement: &AstStatement,
    ast: &AstProgram,
    list_names: &BTreeSet<String>,
    allow_branch_list_reference: bool,
) -> bool {
    let is_when = statement
        .expr
        .and_then(|expr_id| ast.expressions.get(expr_id))
        .is_some_and(|expr| {
            matches!(expr.kind, AstExprKind::When { .. })
                || matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "WHEN")
        });
    statement.expr.is_some_and(|expr_id| {
        expr_returns_list_collection_inner(expr_id, ast, list_names, allow_branch_list_reference)
    }) || statement.children.iter().any(|child| {
        statement_returns_list_collection_with_names(
            child,
            ast,
            list_names,
            allow_branch_list_reference || is_when,
        )
    })
}

fn statement_row_scope_output_list(statement: &AstStatement, op: &str) -> Option<String> {
    if op != "List/map" {
        return None;
    }
    match &statement.kind {
        AstStatementKind::Field { name }
        | AstStatementKind::List {
            field: Some(name), ..
        } if !matches!(name.as_str(), "items" | "children") => Some(name.clone()),
        _ => None,
    }
}

fn list_map_binding_name(args: &[AstCallArg], ast: &AstProgram) -> Option<String> {
    let arg = args.iter().find(|arg| arg.name.is_none())?;
    let expr = ast.expressions.get(arg.value)?;
    match &expr.kind {
        AstExprKind::Identifier(name) => Some(name.clone()),
        AstExprKind::Path(parts) if parts.len() == 1 => parts.first().cloned(),
        _ => None,
    }
}

fn statement_collection_list_name(
    expr_id: usize,
    ast: &AstProgram,
    previous_collection_list: Option<&str>,
) -> Option<String> {
    let expr = ast.expressions.get(expr_id)?;
    match &expr.kind {
        AstExprKind::Pipe { input, op, .. }
            if matches!(
                op.as_str(),
                "List/map"
                    | "List/append"
                    | "List/retain"
                    | "List/remove"
                    | "List/count"
                    | "List/every"
                    | "List/any"
                    | "List/is_not_empty"
                    | "List/latest"
            ) =>
        {
            collection_list_name(*input, ast)
                .or_else(|| previous_collection_list.map(str::to_owned))
        }
        _ => collection_list_name(expr_id, ast),
    }
}

fn collection_list_name(expr_id: usize, ast: &AstProgram) -> Option<String> {
    let expr = ast.expressions.get(expr_id)?;
    match &expr.kind {
        AstExprKind::Identifier(value) => Some(value.clone()),
        AstExprKind::Path(parts) => parts.last().cloned(),
        AstExprKind::Pipe { input, .. } | AstExprKind::Draining { input } => {
            collection_list_name(*input, ast)
        }
        _ => None,
    }
}

fn expr_returns_list_collection(
    expr_id: usize,
    ast: &AstProgram,
    list_names: &BTreeSet<String>,
) -> bool {
    expr_returns_list_collection_inner(expr_id, ast, list_names, false)
}

fn expr_returns_list_collection_inner(
    expr_id: usize,
    ast: &AstProgram,
    list_names: &BTreeSet<String>,
    allow_list_reference: bool,
) -> bool {
    let Some(expr) = ast.expressions.get(expr_id) else {
        return false;
    };
    match &expr.kind {
        AstExprKind::Identifier(value) => allow_list_reference && list_names.contains(value),
        AstExprKind::Path(parts) => {
            allow_list_reference
                && parts
                    .last()
                    .is_some_and(|value| list_names.contains(value.as_str()))
        }
        AstExprKind::ListLiteral { .. } => true,
        AstExprKind::Call { function, .. } => list_returning_operator(function),
        AstExprKind::Pipe { op, .. } => list_returning_operator(op),
        AstExprKind::Draining { input } => {
            expr_returns_list_collection_inner(*input, ast, list_names, allow_list_reference)
        }
        AstExprKind::Then {
            output: Some(output),
            ..
        }
        | AstExprKind::MatchArm {
            output: Some(output),
            ..
        } => expr_returns_list_collection_inner(*output, ast, list_names, true),
        _ => false,
    }
}

fn list_returning_operator(op: &str) -> bool {
    matches!(
        op,
        "List/range"
            | "List/map"
            | "List/append"
            | "List/retain"
            | "List/remove"
            | "List/filter_text_contains"
            | "List/filter_field_equal"
            | "List/filter_field_not_equal"
            | "List/move_field_first"
            | "List/move_field_last"
            | "List/chunk"
            | "List/sort_by"
    )
}

fn function_name_from_expr(expr_id: usize, ast: &AstProgram) -> Option<String> {
    let expr = ast.expressions.get(expr_id)?;
    match &expr.kind {
        AstExprKind::Call { function, .. } | AstExprKind::Pipe { op: function, .. } => {
            (!is_operator_lexeme(function)).then(|| function.clone())
        }
        AstExprKind::Identifier(function) => Some(function.clone()),
        _ => None,
    }
}

fn row_scopes_for_function<'a>(
    row_scopes: &'a [ParsedRowScopeFunction],
    function: &str,
) -> impl Iterator<Item = &'a str> {
    row_scopes
        .iter()
        .filter(move |scope| scope.function == function)
        .map(|scope| scope.row_scope.as_str())
}

fn function_has_row_scope(row_scopes: &[ParsedRowScopeFunction], function: &str) -> bool {
    row_scopes.iter().any(|scope| scope.function == function)
}

fn join_path<'a>(scope: &[String], tail: impl IntoIterator<Item = &'a str>) -> String {
    let mut path = String::new();
    for name in scope {
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

fn scope_path(scope: &[String]) -> Option<String> {
    (!scope.is_empty()).then(|| {
        scope
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(".")
    })
}

fn source_scope_without_events(scope: &[String]) -> Vec<String> {
    scope
        .iter()
        .filter(|segment| segment.as_str() != "events")
        .cloned()
        .collect()
}

fn source_scope_is_scoped(scope: &[String], row_scopes: &[ParsedRowScopeFunction]) -> bool {
    scope_is_indexed(scope, row_scopes)
}

fn scope_is_indexed(scope: &[String], row_scopes: &[ParsedRowScopeFunction]) -> bool {
    scope.iter().any(|name| {
        row_scopes
            .iter()
            .any(|row_scope| row_scope.row_scope == *name)
    })
}

fn singular_row_scope(list_name: &str) -> String {
    list_name
        .strip_suffix("ies")
        .map(|prefix| format!("{prefix}y"))
        .or_else(|| list_name.strip_suffix('s').map(str::to_owned))
        .unwrap_or_else(|| format!("{list_name}_item"))
}

fn collect_functions(ast: &AstProgram) -> Vec<String> {
    ast.semantic_parser_items()
        .filter_map(|item| item.function.clone())
        .collect()
}

fn parser_item_summary(item: &ParserItem) -> String {
    item.symbols.join(" ")
}

fn collect_operators(ast: &AstProgram) -> Vec<String> {
    let mut operators = Vec::new();
    for token in ast.semantic_tokens() {
        if is_operator_lexeme(&token.lexeme)
            && !operators.iter().any(|operator| operator == &token.lexeme)
        {
            operators.push(token.lexeme.clone());
        }
    }
    operators
}

fn error(path: &str, line: usize, column: usize, message: &str) -> ParseError {
    ParseError {
        path: path.to_owned(),
        line: Some(line),
        column: Some(column),
        message: format!("{message} at line {line}, column {column}"),
    }
}

#[cfg(test)]
mod tests;
