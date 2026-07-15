use super::*;

#[test]
fn then_preserves_transactional_file_write_as_its_output_expression() {
    let program = parse_source(
        "file-write-effect.bn",
        include_str!("../../../examples/bytes_file_write_effect.bn"),
    )
    .unwrap();
    let then_output = program
        .ast
        .expressions
        .iter()
        .find_map(|expression| match expression.kind {
            AstExprKind::Then {
                output: Some(output),
                ..
            } => program
                .ast
                .expressions
                .iter()
                .find(|candidate| candidate.id == output),
            _ => None,
        })
        .expect("THEN output expression");

    assert!(
        matches!(
            &then_output.kind,
            AstExprKind::Call { function, .. } if function == "File/write_bytes"
        ),
        "unexpected THEN output: {:?}",
        then_output.kind
    );
}

#[test]
fn multiline_control_flow_expression_graph_is_acyclic() {
    let ast = parse_ast(
        "todo-migration-v4.bn",
        include_str!("../../../examples/migrations/todo/v4.bn"),
    )
    .unwrap();
    let mut state = vec![0u8; ast.expressions.len()];
    let mut stack = Vec::new();
    for expression in &ast.expressions {
        visit_expression(expression.id, &ast.expressions, &mut state, &mut stack);
    }
}

fn visit_expression(id: usize, expressions: &[AstExpr], state: &mut [u8], stack: &mut Vec<usize>) {
    match state[id] {
        2 => return,
        1 => panic!("expression graph cycle: {stack:?} -> {id}"),
        _ => {}
    }
    state[id] = 1;
    stack.push(id);
    for child in expression_children(&expressions[id].kind) {
        visit_expression(child, expressions, state, stack);
    }
    stack.pop();
    state[id] = 2;
}

fn expression_children(kind: &AstExprKind) -> Vec<usize> {
    match kind {
        AstExprKind::Call { args, .. } => args.iter().map(|arg| arg.value).collect(),
        AstExprKind::Pipe { input, args, .. } => std::iter::once(*input)
            .chain(args.iter().map(|arg| arg.value))
            .collect(),
        AstExprKind::Draining { input } | AstExprKind::When { input } => vec![*input],
        AstExprKind::Hold { initial, .. } => vec![*initial],
        AstExprKind::Then { input, output } => std::iter::once(*input)
            .chain(output.iter().copied())
            .collect(),
        AstExprKind::Infix { left, right, .. } => vec![*left, *right],
        AstExprKind::MatchArm { output, .. } => output.iter().copied().collect(),
        AstExprKind::Object(fields)
        | AstExprKind::Record(fields)
        | AstExprKind::TaggedObject { fields, .. } => {
            fields.iter().map(|field| field.value).collect()
        }
        AstExprKind::ListLiteral { items, .. } | AstExprKind::BytesLiteral { items, .. } => {
            items.clone()
        }
        _ => Vec::new(),
    }
}

#[test]
fn timer_intervals_are_typed_as_scheduled_source_ports() {
    let program = parse_source(
        "timer-sources.bn",
        r#"
fast: Duration[milliseconds: 250] |> Timer/interval()
slow: Duration[seconds: 2] |> Timer/interval()
"#,
    )
    .unwrap();

    assert_eq!(
        program
            .source_ports
            .iter()
            .map(|source| (source.path.as_str(), source.interval_ms))
            .collect::<Vec<_>>(),
        [("fast", Some(250)), ("slow", Some(2_000))]
    );
}

#[test]
fn formatter_keeps_todomvc_source_declarations_in_designed_compact_shape() {
    let source = include_str!("../../../examples/todomvc.bn");
    let formatted = format_source("examples/todomvc.bn", source).unwrap();

    assert!(formatted.contains("toggle_all_checkbox: [events: [click: SOURCE]]"));
    assert!(formatted.contains("remove_todo_button: [events: [press: SOURCE]]"));
    assert!(formatted.contains(
        "editing_todo_title_element: [\n                events: [\n                    change: SOURCE\n                    key_down: SOURCE\n                    blur: SOURCE\n                ]\n            ]"
    ));
    assert!(formatted.contains("todo_title_element: [events: [double_click: SOURCE]]"));
}

#[test]
fn formatter_accepts_manifest_entry_file_as_source_unit() {
    let source = include_str!("../../../examples/cells.bn");
    let standalone = format_source("examples/cells.bn", source).unwrap();
    assert!(standalone.contains("cells_app()"));

    let formatted = format_source_unit("examples/cells.bn", source).unwrap();
    assert!(formatted.contains("cells_app()"));
    assert!(formatted.ends_with('\n'));
}

#[test]
fn parses_todomvc_marker_and_constructs() {
    let source = include_str!("../../../examples/todomvc.bn");
    let program = parse_source("examples/todomvc.bn", source).unwrap();
    assert_eq!(program.kind, ProgramKind::Generic);
    assert!(
        program
            .expressions
            .iter()
            .any(|expr| matches!(expr.kind, AstExprKind::Hold { .. }))
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
            .source_ports
            .iter()
            .any(|port| port.path == "store.sources.toggle_all_checkbox.click" && !port.scoped)
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
    assert!(program.row_scope_functions.iter().any(|scope| {
        scope.function == "new_todo" && scope.list == "todos" && scope.row_scope == "todo"
    }));
    assert!(
        !program
            .expressions
            .iter()
            .any(|expr| matches!(expr.kind, AstExprKind::Unknown(_)))
    );
}

#[test]
fn builds_hierarchical_statement_and_expression_ast() {
    let source = include_str!("../../../examples/todomvc.bn");
    let program = parse_source("examples/todomvc.bn", source).unwrap();
    let store = find_statement(&program.ast.statements, |statement| {
        matches!(
            &statement.kind,
            AstStatementKind::Field { name } if name == "store"
        )
    })
    .expect("store field statement should exist");
    assert!(
        !store.children.is_empty(),
        "field block must own nested statements"
    );
    assert!(
        find_statement(&store.children, |statement| {
            matches!(&statement.kind, AstStatementKind::Source { .. })
        })
        .is_some(),
        "nested SOURCE declarations should be structured statements"
    );

    let hold = program
        .ast
        .expressions
        .iter()
        .find(|expr| matches!(expr.kind, AstExprKind::Hold { ref name, .. } if name == "text"))
        .expect("new_todo_text HOLD expression should be parsed");
    let AstExprKind::Hold { initial, .. } = hold.kind else {
        panic!("expected HOLD expression");
    };
    assert!(matches!(
        program.ast.expressions[initial].kind,
        AstExprKind::TextLiteral(ref text) if text.is_empty()
    ));
    assert!(
        program
            .ast
            .expressions
            .iter()
            .any(|expr| matches!(expr.kind, AstExprKind::Latest)),
        "LATEST should be a structured expression node"
    );
    assert!(
        program
            .ast
            .expressions
            .iter()
            .any(|expr| matches!(expr.kind, AstExprKind::When { .. })),
        "WHEN should be a structured expression node"
    );
    assert!(
        program
            .ast
            .expressions
            .iter()
            .any(|expr| matches!(expr.kind, AstExprKind::Then { .. })),
        "THEN should be a structured expression node"
    );
    let nested_then = parse_source(
        "nested-then-bool-not.bn",
        r#"
store: [
sources: [button: [press: SOURCE]]
value:
    False |> HOLD value {
        LATEST {
            sources.button.press |> THEN { value |> Bool/not() }
        }
    }
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
"#,
    )
    .unwrap();
    assert!(
        program.ast.expressions.iter().any(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return false;
            };
            matches!(
                program.ast.expressions[output].kind,
                AstExprKind::Bool(true)
            )
        }),
        "THEN should keep its output block as a structured expression"
    );
    assert!(
        nested_then.ast.expressions.iter().any(|expr| {
            let AstExprKind::Then {
                output: Some(output),
                ..
            } = expr.kind
            else {
                return false;
            };
            matches!(
                &nested_then.ast.expressions[output].kind,
                AstExprKind::Pipe { op, .. } if op == "Bool/not"
            )
        }),
        "THEN should keep nested call output blocks such as Bool/not()"
    );
}

#[test]
fn pipe_takes_precedence_over_infix_expression_input() {
    let program = parse_source(
        "pipe-infix-precedence.bn",
        r#"
source: SOURCE
value: "" |> HOLD value { LATEST {} }
visible: active_count == 0 |> Bool/and(completed_count > 0)
"#,
    )
    .unwrap();

    let pipe = program
        .ast
        .expressions
        .iter()
        .find(|expr| matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "Bool/and"))
        .expect("Bool/and pipe should be parsed as the top-level expression");
    let AstExprKind::Pipe { input, args, .. } = &pipe.kind else {
        panic!("expected Bool/and pipe");
    };
    assert!(matches!(
        &program.ast.expressions[*input].kind,
        AstExprKind::Infix { op, .. } if op == "=="
    ));
    let arg = args
        .first()
        .expect("Bool/and should keep its comparison arg");
    assert!(matches!(
        &program.ast.expressions[arg.value].kind,
        AstExprKind::Infix { op, .. } if op == ">"
    ));
}

#[test]
fn structured_expression_ast_ignores_comment_and_string_operators() {
    let source = r#"
-- LATEST { fake |> THEN { bad } }
label: "fake |> WHEN { SOURCE }"
cells:
List/range(from: 0, to: 0)
|> List/map(cell, new: new_cell(cell: cell))
FUNCTION new_cell(cell) {
sources: [editor: [commit: SOURCE]]
[
    value:
        TEXT {} |> HOLD value {
            LATEST {
                sources.editor.commit.text
            }
        }
]
}
"#;
    let program = parse_source("comments-and-strings.bn", source).unwrap();
    let latest_count = program
        .ast
        .expressions
        .iter()
        .filter(|expr| matches!(expr.kind, AstExprKind::Latest))
        .count();
    let when_count = program
        .ast
        .expressions
        .iter()
        .filter(|expr| matches!(expr.kind, AstExprKind::When { .. }))
        .count();
    assert_eq!(latest_count, 1);
    assert_eq!(when_count, 0);
}

#[test]
fn parses_structural_objects_tagged_objects_tags_and_decimals() {
    let source = r#"
source: SOURCE
value: 1.25 |> HOLD value { LATEST {} }
items: LIST[1] {}
items |> List/map(item, new: item)
style: [color: Oklch[lightness:0.97,chroma:0.02,hue:18.6], mode: Completed]
document: []
"#;
    let program = parse_source("structural-types.bn", source).unwrap();
    assert!(
        program
            .ast
            .expressions
            .iter()
            .any(|expr| { matches!(&expr.kind, AstExprKind::Number(value) if value == "1.25") })
    );
    assert!(
        program
            .ast
            .expressions
            .iter()
            .any(|expr| { matches!(&expr.kind, AstExprKind::Number(value) if value == "0.97") })
    );
    assert!(
        program
            .ast
            .expressions
            .iter()
            .any(|expr| { matches!(&expr.kind, AstExprKind::Number(value) if value == "0.02") })
    );
    assert!(
        program
            .ast
            .expressions
            .iter()
            .any(|expr| { matches!(&expr.kind, AstExprKind::Tag(value) if value == "Completed") })
    );
    assert!(program.ast.expressions.iter().any(|expr| {
        matches!(&expr.kind, AstExprKind::TaggedObject { tag, fields }
            if tag == "Oklch" && fields.iter().any(|field| field.name == "lightness"))
    }));
    assert!(program.ast.expressions.iter().any(|expr| {
        matches!(&expr.kind, AstExprKind::Object(fields)
            if fields.iter().any(|field| field.name == "color"))
    }));
    let oklch = program
        .ast
        .expressions
        .iter()
        .find_map(|expr| match &expr.kind {
            AstExprKind::TaggedObject { tag, fields } if tag == "Oklch" => Some(fields),
            _ => None,
        })
        .expect("Oklch tagged object should parse");
    let chroma = oklch
        .iter()
        .find(|field| field.name == "chroma")
        .expect("chroma field should parse");
    assert_eq!(&program.source[chroma.start..chroma.end], "chroma:0.02");
    assert_eq!(
        &program.source
            [program.expressions[chroma.value].start..program.expressions[chroma.value].end],
        "0.02"
    );
    let map_call = program
        .ast
        .expressions
        .iter()
        .find_map(|expr| match &expr.kind {
            AstExprKind::Pipe { op, args, .. } if op == "List/map" => Some(args),
            _ => None,
        })
        .expect("List/map pipe should parse");
    let new_arg = map_call
        .iter()
        .find(|arg| arg.name.as_deref() == Some("new"))
        .expect("new arg should parse");
    assert_eq!(&program.source[new_arg.start..new_arg.end], "new: item");
}

#[test]
fn row_template_scope_comes_from_list_map_not_function_name() {
    let source = include_str!("../../../examples/todomvc.bn").replace("new_todo", "make_item");
    let program = parse_source("examples/todomvc.bn", source).unwrap();
    assert!(program.functions.contains(&"make_item".to_owned()));
    assert!(program.row_scope_functions.iter().any(|scope| {
        scope.function == "make_item" && scope.list == "todos" && scope.row_scope == "todo"
    }));
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
fn list_map_row_scope_prefers_item_binding_over_singular_list_name() {
    let source = r#"
SOURCE
HOLD
LATEST
store:
selected_waveform_segments:
    LIST {
        [signal_id: TEXT { clk }, width: 28, state: High, label: TEXT { 1 }]
    }
    |> List/map(segment, new: new_waveform_segment(segment: segment))
FUNCTION new_waveform_segment(segment) {
signal_id: segment.signal_id
width: segment.width
state: segment.state
label: segment.label
}
"#;
    let program = parse_source("examples/novywave/RUN.bn", source).unwrap();
    assert!(program.row_scope_functions.iter().any(|scope| {
        scope.function == "new_waveform_segment"
            && scope.list == "selected_waveform_segments"
            && scope.row_scope == "segment"
    }));
    assert!(
        program
            .state_cells
            .iter()
            .all(|cell| !cell.path.starts_with("selected_waveform_segment."))
    );
    assert!(
        program
            .source_ports
            .iter()
            .all(|source| !source.path.starts_with("selected_waveform_segment."))
    );
}

#[test]
fn parses_cells_marker_and_constructs() {
    let program = parse_project(
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
    assert_eq!(program.kind, ProgramKind::Generic);
    assert!(
        program
            .expressions
            .iter()
            .any(|expr| matches!(expr.kind, AstExprKind::Source))
    );
    assert!(program.functions.contains(&"new_cell".to_owned()));
    assert!(program.functions.contains(&"new_sheet_column".to_owned()));
    assert!(program.functions.contains(&"cells_app".to_owned()));
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
    assert!(
        program
            .list_memories
            .iter()
            .any(|list| list.name == "sheet_columns")
    );
    assert!(
        !program
            .expressions
            .iter()
            .any(|expr| matches!(expr.kind, AstExprKind::Unknown(_)))
    );
}

#[test]
fn parses_bytes_literals_and_explicit_base_byte_literals() {
    let source = r#"
SOURCE
HOLD
LATEST
empty_dynamic: BYTES {}
binary_byte: BYTES[1] { 2u10101010 }
octal_byte: BYTES[1] { 8u377 }
decimal_byte: BYTES[1] { 10u255 }
png_magic: BYTES[__] { 16u89, 16u50, 16u4E, 16u47 }
header: BYTES[4] { 16u89, 16u50, 16u4E, 16u47 }
scratch: BYTES[64] {}
frame: BYTES[__] { header, BYTES[1] { 16u00 } }
patched: Bytes/set(input: header, index: 0, value: 16uFF)
"#;
    let program = parse_source("bytes-parser.bn", source).unwrap();
    let byte_values = program
        .expressions
        .iter()
        .filter_map(|expr| match &expr.kind {
            AstExprKind::ByteLiteral {
                radix,
                digits,
                value,
            } => Some((*radix, digits.clone(), *value)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(byte_values.contains(&(16, "89".to_owned(), 0x89)));
    assert!(byte_values.contains(&(16, "FF".to_owned(), 0xFF)));
    assert!(byte_values.contains(&(2, "10101010".to_owned(), 0b1010_1010)));
    assert!(byte_values.contains(&(8, "377".to_owned(), 0xFF)));
    assert!(byte_values.contains(&(10, "255".to_owned(), 0xFF)));

    let bytes_literals = program
        .expressions
        .iter()
        .filter_map(|expr| match &expr.kind {
            AstExprKind::BytesLiteral { size, items } => Some((size.clone(), items.len())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(bytes_literals.contains(&(BytesSizeSyntax::Dynamic, 0)));
    assert!(bytes_literals.contains(&(BytesSizeSyntax::Infer, 4)));
    assert!(bytes_literals.contains(&(BytesSizeSyntax::Fixed(4), 4)));
    assert!(bytes_literals.contains(&(BytesSizeSyntax::Fixed(64), 0)));
    assert!(bytes_literals.contains(&(BytesSizeSyntax::Fixed(1), 1)));
}

#[test]
fn bytes_literals_and_byte_literals_keep_exact_spans() {
    let source = "SOURCE\nHOLD\nLATEST\npayload: BYTES[2] { 16uAA, 10u7 }\ndocument: []\n";
    let program = parse_source("bytes-spans.bn", source).unwrap();
    let bytes_start = source.find("BYTES[2] { 16uAA, 10u7 }").unwrap();
    let bytes_end = bytes_start + "BYTES[2] { 16uAA, 10u7 }".len();
    assert!(
        program.expressions.iter().any(|expr| {
            matches!(
                expr.kind,
                AstExprKind::BytesLiteral {
                    size: BytesSizeSyntax::Fixed(2),
                    ..
                }
            ) && expr.start == bytes_start
                && expr.end == bytes_end
        }),
        "BYTES literal span should cover the full constructor body"
    );

    let first_byte_start = source.find("16uAA").unwrap();
    let first_byte_end = first_byte_start + "16uAA".len();
    assert!(
        program.expressions.iter().any(|expr| {
            matches!(
                &expr.kind,
                AstExprKind::ByteLiteral {
                    radix: 16,
                    digits,
                    value: 0xAA,
                } if digits == "AA"
            ) && expr.start == first_byte_start
                && expr.end == first_byte_end
        }),
        "byte literal span should cover the adjacent base+u+digits token"
    );
}

#[test]
fn parses_multiline_bytes_literals_with_comments() {
    let source = r#"
SOURCE
HOLD
LATEST
header: BYTES[4] {
-- PNG magic prefix
16u89,
16u50,
16u4E,
16u47
}
frame: BYTES[__] {
header,
-- nested constructor is flattened by later semantic phases
BYTES[2] {
    16u00,
    16uFF
}
}
trailing_comment: BYTES[1] { 16u2A } -- comment after inline constructor
"#;
    let program = parse_source("bytes-multiline-parser.bn", source).unwrap();
    let bytes_literals = program
        .expressions
        .iter()
        .filter_map(|expr| match &expr.kind {
            AstExprKind::BytesLiteral { size, items } => Some((size.clone(), items.len())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(bytes_literals.contains(&(BytesSizeSyntax::Fixed(4), 4)));
    assert!(bytes_literals.contains(&(BytesSizeSyntax::Infer, 2)));
    assert!(bytes_literals.contains(&(BytesSizeSyntax::Fixed(2), 2)));
    assert!(bytes_literals.contains(&(BytesSizeSyntax::Fixed(1), 1)));
    assert!(
        !program
            .expressions
            .iter()
            .any(|expr| matches!(expr.kind, AstExprKind::Unknown(_)))
    );
}

#[test]
fn rejects_malformed_bytes_syntax_with_targeted_diagnostics() {
    let invalid_hex = parse_source(
        "bad-bytes.bn",
        "SOURCE\nHOLD\nLATEST\nbad: BYTES[1] { 16uGG }\n",
    )
    .unwrap_err();
    assert!(invalid_hex.message.contains("outside base 16"));

    let overflow = parse_source(
        "bad-bytes.bn",
        "SOURCE\nHOLD\nLATEST\nbad: BYTES[1] { 16u100 }\n",
    )
    .unwrap_err();
    assert!(overflow.message.contains("bytes must be 0..255"));

    let bad_base = parse_source(
        "bad-bytes.bn",
        "SOURCE\nHOLD\nLATEST\nbad: BYTES[1] { 3u12 }\n",
    )
    .unwrap_err();
    assert!(
        bad_base
            .message
            .contains("byte literal base must be one of")
    );

    let missing_digits = parse_source(
        "bad-bytes.bn",
        "SOURCE\nHOLD\nLATEST\nbad: BYTES[1] { 16u }\n",
    )
    .unwrap_err();
    assert!(
        missing_digits
            .message
            .contains("byte literal must include digits after `u`")
    );

    let bad_size =
        parse_source("bad-bytes.bn", "SOURCE\nHOLD\nLATEST\nbad: BYTES[foo] {}\n").unwrap_err();
    assert!(bad_size.message.contains("BYTES size must be `__`"));

    let negative_size =
        parse_source("bad-bytes.bn", "SOURCE\nHOLD\nLATEST\nbad: BYTES[-1] {}\n").unwrap_err();
    assert!(
        negative_size
            .message
            .contains("non-negative decimal integer")
    );

    let missing_body =
        parse_source("bad-bytes.bn", "SOURCE\nHOLD\nLATEST\nbad: BYTES[4]\n").unwrap_err();
    assert!(
        missing_body
            .message
            .contains("BYTES constructor requires a `{ ... }` body")
    );

    let missing_closing_body = parse_source(
        "bad-bytes.bn",
        "SOURCE\nHOLD\nLATEST\nbad: BYTES[1] { 16u00\n",
    )
    .unwrap_err();
    assert!(
        missing_closing_body
            .message
            .contains("BYTES constructor is missing closing `}`")
    );

    let trailing = parse_source(
        "bad-bytes.bn",
        "SOURCE\nHOLD\nLATEST\nbad: BYTES[1] { 16u00 } trailing\n",
    )
    .unwrap_err();
    assert!(
        trailing
            .message
            .contains("BYTES constructor has unexpected trailing token `trailing`")
    );

    let multiline_invalid_hex = parse_source(
        "bad-bytes.bn",
        "SOURCE\nHOLD\nLATEST\nbad: BYTES[1] {\n    16uGG\n}\n",
    )
    .unwrap_err();
    assert!(multiline_invalid_hex.message.contains("outside base 16"));

    let multiline_trailing = parse_source(
        "bad-bytes.bn",
        "SOURCE\nHOLD\nLATEST\nbad: BYTES[1] {\n    16u00\n} trailing\n",
    )
    .unwrap_err();
    assert!(
        multiline_trailing
            .message
            .contains("BYTES constructor has unexpected trailing token `trailing`")
    );
}

#[test]
fn byte_literal_validation_does_not_cross_lines() {
    let parsed = parse_source(
        "bytes-line-boundary.bn",
        "SOURCE\nHOLD\nLATEST\na: 16\nupdated: 1\nspaced: 16 uFF\ndocument: []\n",
    )
    .unwrap();
    assert!(
        parsed
            .expressions
            .iter()
            .any(|expr| matches!(&expr.kind, AstExprKind::Number(value) if value == "16"))
    );
    assert!(!parsed.expressions.iter().any(|expr| {
        matches!(
            &expr.kind,
            AstExprKind::ByteLiteral {
                radix: 16,
                digits,
                value: 0xFF
            } if digits == "FF"
        )
    }));
}

#[test]
fn unsupported_example_keyword_rejected_but_comments_strings_are_ignored() {
    let err = parse_source(
        "examples/cells.bn",
        "EXAMPLE Cells\nSOURCE\nHOLD\nLATEST\nLIST {}\nList/map",
    )
    .unwrap_err();
    assert!(err.message.contains("`EXAMPLE` is not Boon syntax"));
    assert!(err.message.contains("manifest/dev metadata"));

    let source = r#"
-- label: "EXAMPLE TodoMVC"
cells:
List/range(from: 0, to: 0)
|> List/map(cell, new: new_cell(cell: cell))
SOURCE
HOLD
LATEST
"#;
    let program = parse_source("examples/todomvc-looking-path.bn", source).unwrap();
    assert_eq!(program.kind, ProgramKind::Generic);

    let missing = r#"
-- label: "EXAMPLE Cells"
SOURCE
HOLD
LATEST
List/map
LIST {}
"#;
    let program = parse_source("unknown-kind.bn", missing).unwrap();
    assert_eq!(program.kind, ProgramKind::Generic);

    let err = parse_source(
        "unknown-kind.bn",
        "# comment\nSOURCE\nHOLD\nLATEST\nLIST {}\nList/map",
    )
    .unwrap_err();
    assert!(err.message.contains("use `--` comments"));
}

#[test]
fn canonical_name_validation_ignores_text_literal_contents() {
    let source = r#"
SOURCE
HOLD
LATEST
LIST {}
document: Document/new(
root: Element/label(
    element: []
    style: []
    label: TEXT { data:image/svg+xml;utf8,%3Cpath%20fill%3D%22none%22/%3E }
    detail: TEXT {
        data:image/svg+xml;utf8,%3Cpath%20fill%3D%22none%22/%3E
    }
)
)
"#;

    let program = parse_source("examples/svg-text.bn", source).unwrap();
    assert!(program.ast.expressions.iter().any(|expr| {
        matches!(
            &expr.kind,
            AstExprKind::TextLiteral(text) if text.contains("%20fill%3D%22none%22")
        )
    }));
    assert!(!program.ast.expressions.iter().any(|expr| {
        matches!(&expr.kind, AstExprKind::Unknown(tokens) if tokens.iter().any(|token| token.contains("fill")))
    }));
}

#[test]
fn text_literals_preserve_compact_technical_punctuation() {
    let source = r#"
SOURCE
HOLD
LATEST
LIST {}
value:
TEXT { Binary } |> WHEN {
    TEXT { Binary } => TEXT { 0x2a }
    __ => TEXT { 42.8 C }
}
name: TEXT { data_bus[7:0] }
document: Document/new(root: Element/label(element: [], style: [], label: name))
"#;
    let program = parse_source("examples/technical-text.bn", source).unwrap();
    let texts: Vec<_> = program
        .ast
        .expressions
        .iter()
        .filter_map(|expr| match &expr.kind {
            AstExprKind::TextLiteral(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(texts.contains(&"0x2a"), "{texts:#?}");
    assert!(texts.contains(&"42.8 C"), "{texts:#?}");
    assert!(texts.contains(&"data_bus[7:0]"), "{texts:#?}");
}

#[test]
fn text_literal_pipe_on_same_line_is_parsed_as_pipe() {
    let source = r#"
SOURCE
HOLD
LATEST
LIST {}
store: [
path: TEXT { /tmp/wave.vcd }
label: TEXT { Path: } |> Text/concat(with: path, separator: " ")
]
document: Document/new(root: Element/label(element: [], label: store.label))
"#;
    let program = parse_source("examples/text-literal-pipe.bn", source).unwrap();
    let concat = program
        .ast
        .expressions
        .iter()
        .find(|expr| matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "Text/concat"))
        .expect("same-line text literal pipe should be preserved");
    let AstExprKind::Pipe { input, .. } = concat.kind else {
        unreachable!("checked pipe expression");
    };
    assert!(matches!(
        &program.ast.expressions[input].kind,
        AstExprKind::TextLiteral(text) if text == "Path:"
    ));
}

#[test]
fn call_result_field_access_keeps_call_input() {
    let source = r#"
SOURCE
HOLD
LATEST
LIST {}
FUNCTION assets() {
[icon: TEXT { data:image/svg+xml;utf8,%3Csvg/%3E }]
}
document: Document/new(
root: Element/label(label: assets().icon)
)
"#;

    let program = parse_source("examples/assets-field.bn", source).unwrap();
    let field_pipe = program
        .ast
        .expressions
        .iter()
        .find(|expr| matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "Field/icon"))
        .expect("postfix field access should become a field pipe");
    let AstExprKind::Pipe { input, .. } = field_pipe.kind else {
        unreachable!("checked pipe expression");
    };
    assert!(matches!(
        program.ast.expressions.get(input).map(|expr| &expr.kind),
        Some(AstExprKind::Call { function, .. }) if function == "assets"
    ));
}

#[test]
fn source_pipe_block_keeps_source_path_argument() {
    let source = r#"
SOURCE
HOLD
LATEST
LIST {}
document: Document/new(
root: Element/button(label: TEXT { Go }) |> SOURCE { PASSED.controls.go }
)
"#;

    let program = parse_source("examples/source-pipe-block.bn", source).unwrap();
    let source_pipe = program
        .ast
        .expressions
        .iter()
        .find(|expr| matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "SOURCE"))
        .expect("source pipe should parse");
    let AstExprKind::Pipe { args, .. } = &source_pipe.kind else {
        unreachable!("checked pipe expression");
    };
    assert_eq!(args.len(), 1);
    assert!(matches!(
        program.ast.expressions.get(args[0].value).map(|expr| &expr.kind),
        Some(AstExprKind::Path(parts))
            if parts.iter().map(String::as_str).eq(["PASSED", "controls", "go"])
    ));
}

#[test]
fn parses_record_spread_entries() {
    let program = parse_source(
        "examples/spread.bn",
        "LIST {}\nSOURCE\nHOLD\nLATEST\nbase: [a: 1]\nmerged: [...base, b: 2]\nList/map",
    )
    .unwrap();
    assert!(program.expressions.iter().any(|expr| {
        matches!(&expr.kind, AstExprKind::Object(fields) if fields.iter().any(|field| field.spread))
    }));
}

#[test]
fn parses_multiline_record_spread_lines_as_value_expressions() {
    let program = parse_source(
        "examples/spread-lines.bn",
        r#"
SOURCE
HOLD
LATEST
LIST {}
base: [a: 1]
merged: [
...base
b: 2
]
"#,
    )
    .unwrap();
    assert!(
        program
            .ast
            .statements
            .iter()
            .any(|statement| matches!(statement.kind, AstStatementKind::Spread))
    );
    assert!(!program.ast.expressions.iter().any(|expr| {
        matches!(&expr.kind, AstExprKind::Call { function, .. } if function.starts_with("..."))
    }));
}

#[test]
fn parses_multiline_inline_object_field_with_when_value() {
    let program = parse_source(
        "examples/object-field-when.bn",
        r#"
SOURCE
HOLD
LATEST
LIST {}
selected: True
style: [
move: [closer: selected |> WHEN {
    True => 4
    False => 0
}]
]
"#,
    )
    .unwrap();
    assert!(program.ast.expressions.iter().any(|expr| {
        matches!(
            &expr.kind,
            AstExprKind::Object(fields)
                if fields.iter().any(|field| field.name == "closer"
                    && matches!(program.ast.expressions[field.value].kind, AstExprKind::When { .. }))
        )
    }));
}

#[test]
fn parse_project_namespaces_uppercase_module_files() {
    let program = parse_project(
        "examples/app.bn",
        [
            (
                "examples/Theme/Theme.bn".to_owned(),
                "FUNCTION material() {\n    color()\n}\nFUNCTION color() {\n    TEXT { red }\n}\n"
                    .to_owned(),
            ),
            (
                "examples/app.bn".to_owned(),
                "LIST {}\nSOURCE\nHOLD\nLATEST\nvalue: Theme/material()\nList/map\n".to_owned(),
            ),
        ],
    )
    .unwrap();
    assert!(
        program
            .functions
            .iter()
            .any(|name| name == "Theme/material")
    );
    assert!(program.functions.iter().any(|name| name == "Theme/color"));
    assert!(program.expressions.iter().any(|expr| {
        matches!(&expr.kind, AstExprKind::Call { function, .. } if function == "Theme/color")
    }));
}

#[test]
fn parse_project_reports_errors_in_the_owning_source_file() {
    let error = parse_project(
        "RUN.bn",
        [
            ("RUN.bn".to_owned(), "scene: Theme/render()\n".to_owned()),
            ("Theme.bn".to_owned(), "FUNCTION render() {\n".to_owned()),
        ],
    )
    .unwrap_err();

    assert_eq!(error.path, "Theme.bn");
    assert!(error.line.is_some_and(|line| line <= 2), "{error:?}");
}

#[test]
fn parses_profiled_list_capacity() {
    let source = r#"
todos: LIST[10000] {}
click: SOURCE
value: False |> HOLD value { LATEST { click |> THEN { True } } }
todos |> List/map(todo, new: new_todo(todo: todo))
"#;
    let program = parse_source("profiled-list.bn", source).unwrap();
    let todos = program
        .list_memories
        .iter()
        .find(|list| list.name == "todos")
        .expect("expected todos list memory");
    assert_eq!(todos.capacity, Some(10_000));
}

#[test]
fn novywave_list_memory_names_are_unique() {
    let program = parse_project(
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
                include_str!("../../../examples/novywave/Generated/NovyReference.bn").to_owned(),
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
    let mut first_lines = BTreeMap::new();
    let mut duplicates = Vec::new();
    for list in &program.list_memories {
        if let Some(first_line) = first_lines.insert(list.name.clone(), list.line) {
            duplicates.push((list.name.clone(), first_line, list.line));
        }
    }
    assert!(
        duplicates.is_empty(),
        "duplicate list memory names with first/current lines: {duplicates:?}"
    );
    assert!(
        !program
            .list_memories
            .iter()
            .any(|list| list.name == "store"),
        "`store` is a declaration container and must not become a list memory"
    );
    let mut row_scope_lists = BTreeMap::new();
    let mut conflicting_scopes = Vec::new();
    for scope in &program.row_scope_functions {
        if let Some(first) = row_scope_lists.insert(scope.row_scope.clone(), scope.list.clone())
            && first != scope.list
        {
            conflicting_scopes.push((
                scope.row_scope.clone(),
                first,
                scope.list.clone(),
                scope.function.clone(),
            ));
        }
    }
    assert!(
        conflicting_scopes.is_empty(),
        "row scope names must not be shared across different lists/functions: {conflicting_scopes:?}"
    );
    let list_names = program
        .list_memories
        .iter()
        .map(|list| list.name.as_str())
        .collect::<BTreeSet<_>>();
    let unknown_scope_lists = program
        .row_scope_functions
        .iter()
        .filter(|scope| !list_names.contains(scope.list.as_str()))
        .map(|scope| {
            (
                scope.row_scope.clone(),
                scope.list.clone(),
                scope.function.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        unknown_scope_lists.is_empty(),
        "row scopes must reference known list memories: {unknown_scope_lists:?}"
    );
    assert!(
        program.row_scope_functions.iter().any(|scope| {
            scope.list == "selected_signal_defaults" && scope.row_scope == "selected_signal"
        }),
        "selected signal model rows must keep their declared row scope: {:#?}",
        program.row_scope_functions
    );
    assert!(
        program
            .list_memories
            .iter()
            .any(|list| list.name == "external_file_tree_rows"),
        "conditional external file rows must be a list memory: {:#?}",
        program
            .list_memories
            .iter()
            .filter(|list| list.name.contains("external"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn rejects_malformed_list_capacity() {
    let source = r#"
todos: LIST[many] {}
click: SOURCE
value: False |> HOLD value { LATEST { click |> THEN { True } } }
todos |> List/map(todo, new: new_todo(todo: todo))
"#;
    let err = parse_source("bad-list-capacity.bn", source).unwrap_err();
    assert!(
        err.message
            .contains("LIST capacity must be a positive integer")
    );
    assert!(err.message.contains("line 2"));
}

#[test]
fn rejects_zero_list_capacity() {
    let source = r#"
todos: LIST[0] {}
click: SOURCE
value: False |> HOLD value { LATEST { click |> THEN { True } } }
todos |> List/map(todo, new: new_todo(todo: todo))
"#;
    let err = parse_source("bad-zero-list-capacity.bn", source).unwrap_err();
    assert!(
        err.message
            .contains("LIST capacity must be a positive integer")
    );
}

#[test]
fn permits_user_structural_id_fields_and_todo_id_tags() {
    let source = "LIST {}\nid: TodoId[id: Ulid/generate()]\nSOURCE\nHOLD\nLATEST\nList/map";
    parse_source("examples/todomvc.bn", source).unwrap();
}

#[test]
fn rejects_hidden_runtime_key_after_view_block() {
    let source = format!(
        "{}\nruntime_key: TEXT {{ leak }}\n",
        include_str!("../../../examples/todomvc.bn")
    );
    let err = parse_source("bad-runtime-key.bn", source).unwrap_err();
    assert!(err.message.contains("hidden runtime identity"));
}

#[test]
fn rejects_runtime_identity_collision_names_but_permits_user_key_fields() {
    parse_source(
        "user-key-is-data.bn",
        "LIST {}\nrecord: [key: TEXT { visible }]\nSOURCE\nHOLD\nLATEST\nList/map",
    )
    .unwrap();

    for hidden in ["row_key", "target_key", "target_generation", "bind_epoch"] {
        let source =
            format!("LIST {{}}\n{hidden}: TEXT {{ leak }}\nSOURCE\nHOLD\nLATEST\nList/map");
        let err = parse_source("bad-hidden-identity.bn", &source).unwrap_err();
        assert!(
            err.message.contains(hidden),
            "expected `{hidden}` to be rejected, got {err}"
        );
    }
}

#[test]
fn parses_document_string_literals_and_comments() {
    let source = r##"
-- sibling Boon syntax comment
-- current boon-circuit syntax comment
store:
sources:
    new_todo_input: [change: SOURCE]
new_todo_text: "" |> HOLD new_todo_text { LATEST {} }
todos: LIST[4] {}
todos |> List/map(todo, new: new_todo(todo: todo))
FUNCTION new_todo(todo) {
title: todo.title |> HOLD title { LATEST {} }
}
document:
children:
    element:
        kind: Input
        id: "todo_new_input"
        value: "$new_todo_text"
        placeholder: "What needs to be done?"
"##;
    let parsed = parse_source("document-lines.bn", source).unwrap();
    let document = parsed_document(&parsed).expect("document should parse");
    assert!(statement_contains_line(&document.root, document.root.line));
    assert!(document.expressions.iter().any(|expr| {
        matches!(
            &expr.kind,
            AstExprKind::StringLiteral(value) if value == "What needs to be done?"
        )
    }));
}

#[test]
fn permits_app_visible_id_field_as_ordinary_data() {
    let source = "LIST {}\nid: TEXT { exposed }\nSOURCE\nHOLD\nLATEST\nList/map";
    parse_source("examples/todomvc.bn", source).unwrap();
}

#[test]
fn quoted_strings_decode_standard_escapes() {
    let parsed = parse_source(
        "string-escapes.bn",
        r#"value: "first\nsecond\t\"quoted\"\\tail"
"#,
    )
    .unwrap();

    assert!(parsed.expressions.iter().any(|expr| {
        matches!(
            &expr.kind,
            AstExprKind::StringLiteral(value)
                if value == "first\nsecond\t\"quoted\"\\tail"
        )
    }));
}

#[test]
fn permits_app_visible_todo_id_state_fields() {
    let source = r#"
SOURCE
HOLD
LATEST
LIST {}
selected_todo_id: LATEST {
TodoId[id: Ulid/generate()]
}
next_todo_id: TodoId[id: Ulid/generate()]
"#;
    parse_source("examples/todo_mvc_physical/RUN.bn", source).unwrap();
}

#[test]
fn rejects_global_reducer_update_shape() {
    let source = r#"
FUNCTION update(state, event) {
event.source |> WHEN {
    ToggleTodo => state |> TodoTable/update(completed: True)
}
}
items: LIST {}
click: SOURCE
value: False |> HOLD value { LATEST { click |> THEN { True } } }
items |> List/map(item, new: new_item(item: item))
"#;
    let err = parse_source("examples/todomvc.bn", source).unwrap_err();
    assert!(err.message.contains("central reducer"));
}

#[test]
fn parses_structured_drain_paths_and_terminal_draining_forms() {
    let program = parse_source(
        "drain-syntax.bn",
        r#"
old_count: 0 |> HOLD count { LATEST {} } |> DRAINING
old_text:
    TEXT { old }
    |> HOLD text { LATEST {} }
    |> DRAINING
count: DRAIN { old_count }
theme: DRAIN { settings.theme }
passed_theme:
    DRAIN {
        PASSED.store.settings.theme
    }
    |> Text/to_uppercase()
"#,
    )
    .unwrap();

    let drain_paths = program
        .expressions
        .iter()
        .filter_map(|expr| match &expr.kind {
            AstExprKind::Drain { path } => Some(path.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        drain_paths,
        [
            AstDrainPath::Binding {
                name: "old_count".to_owned(),
            },
            AstDrainPath::Field {
                binding: "settings".to_owned(),
                fields: vec!["theme".to_owned()],
            },
            AstDrainPath::Passed {
                fields: vec![
                    "store".to_owned(),
                    "settings".to_owned(),
                    "theme".to_owned(),
                ],
            },
        ]
    );
    assert_eq!(
        program
            .expressions
            .iter()
            .filter(|expr| matches!(expr.kind, AstExprKind::Draining { .. }))
            .count(),
        2
    );
    assert!(!program.expressions.iter().any(|expr| {
        matches!(&expr.kind, AstExprKind::Call { function, .. } if function == "DRAIN")
            || matches!(&expr.kind, AstExprKind::Pipe { op, .. } if op == "DRAINING")
    }));

    parse_source(
        "drain-keywords-in-text.bn",
        "note: TEXT { DRAIN { old() } |> DRAINING }\n",
    )
    .expect("drain keywords inside text literal content must not be parsed as syntax");
}

#[test]
fn rejects_non_path_drain_bodies_with_deterministic_errors() {
    for body in [
        "",
        "old()",
        "old + 1",
        "42",
        "True",
        "\"old\"",
        "old[index]",
        "old, newer",
        "PASSED",
        "old |> Number/to_text()",
    ] {
        let source = format!("value: DRAIN {{ {body} }}\n");
        let error = parse_source("bad-drain.bn", source).unwrap_err();
        assert_eq!(
            error.message,
            "`DRAIN` body must contain exactly one named binding, field path, or `PASSED` path at line 1, column 8",
            "unexpected diagnostic for `{body}`"
        );
    }

    let missing_body = parse_source("bad-drain.bn", "value: DRAIN old\n").unwrap_err();
    assert_eq!(
        missing_body.message,
        "`DRAIN` requires a `{ path }` body at line 1, column 8"
    );
    let missing_close = parse_source("bad-drain.bn", "value: DRAIN { old\n").unwrap_err();
    assert_eq!(
        missing_close.message,
        "`DRAIN` is missing closing `}` at line 1, column 8"
    );
}

#[test]
fn rejects_invalid_and_nonterminal_draining_placement() {
    for source in [
        "value: DRAINING\n",
        "|> DRAINING\n",
        "value:\n    |> DRAINING\n",
    ] {
        let error = parse_source("bad-draining.bn", source).unwrap_err();
        assert!(
            error
                .message
                .starts_with("`DRAINING` must be used as terminal `input |> DRAINING` syntax"),
            "unexpected diagnostic: {error}"
        );
    }

    for source in [
        "value: 1 |> DRAINING |> Number/to_text()\n",
        "value: 1 |> DRAINING()\n",
        "value: 1 |> DRAINING + 1\n",
        "value:\n    1\n    |> DRAINING\n    |> Number/to_text()\n",
    ] {
        let error = parse_source("bad-draining.bn", source).unwrap_err();
        assert!(
            error
                .message
                .starts_with("`DRAINING` must be terminal in its pipeline"),
            "unexpected diagnostic: {error}"
        );
    }
}

#[test]
fn stateful_root_latest_is_memory_but_event_only_latest_is_transient() {
    let program = parse_source(
        "root-latest-memory.bn",
        r#"
store: [
    pulse: SOURCE
    count:
        LATEST {
            0
            pulse |> THEN { count + 1 }
        }
    transient:
        LATEST {
            pulse |> THEN { count + 10 }
        }
    projected_transient:
        LATEST {
            transient
            |> List/latest()
        }
    derived: count + 20
]
"#,
    )
    .unwrap();

    assert_eq!(
        program
            .state_cells
            .iter()
            .map(|cell| (cell.path.as_str(), cell.indexed))
            .collect::<Vec<_>>(),
        [("store.count", false)]
    );
}

fn find_statement(
    statements: &[AstStatement],
    predicate: impl Fn(&AstStatement) -> bool + Copy,
) -> Option<&AstStatement> {
    statements.iter().find_map(|statement| {
        predicate(statement)
            .then_some(statement)
            .or_else(|| find_statement(&statement.children, predicate))
    })
}
