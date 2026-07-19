#[test]
fn list_latest_accepts_a_direct_mapped_row_event_projection() {
    let parsed = boon_parser::parse_source(
        "mapped-row-event.bn",
        r#"
store: [
    rows:
        LIST {
            [name: TEXT { one }]
            [name: TEXT { two }]
        }
        |> List/map(item, new: selectable_row(row: item))
    selected:
        rows
        |> List/map(item, new:
            item.controls.select.event.press |> THEN { item.name }
        )
        |> List/latest()
]

FUNCTION selectable_row(row) {
    [
        controls: [select: SOURCE]
        name: row.name
    ]
}
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(
        !report.has_errors(),
        "direct mapped row events must not require a singleton LATEST wrapper: {:?}",
        report.diagnostics
    );
    assert_eq!(
        report
            .named_value_type_table
            .entries
            .iter()
            .find(|entry| entry.path == "store.selected")
            .map(|entry| &entry.flow_type.ty),
        Some(&Type::Text)
    );
}

#[test]
fn singleton_latest_is_rejected_as_a_meaningless_merge() {
    let parsed = boon_parser::parse_source(
        "singleton-latest.bn",
        r#"
store: [
    press: SOURCE
    selected:
        LATEST {
            press |> THEN { TEXT { selected } }
        }
]
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(report.has_errors());
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("`LATEST` merges two or more branches")
    }));
}

#[test]
fn function_returning_a_multiline_list_pipeline_is_not_typed_as_its_call_arguments() {
    let parsed = boon_parser::parse_source(
        "multiline-list-function-result.bn",
        r#"
store: [
    groups: LIST {
        [id: TEXT { one }, values: LIST { [label: TEXT { selected }] }]
    }
    selected:
        selected_values()
        |> List/map(item, new: item)
]

FUNCTION selected_values() {
    store.groups
    |> List/find(item, if: item.id == TEXT { one })
    |> WHEN {
        Found[value] => value.values
        NotFound => LIST {}
    }
    |> List/map(item, new: item)
}
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    let report = &output.report;
    assert!(
        !report.has_errors(),
        "multiline call arguments became the function result: {:?}",
        report.diagnostics
    );
    assert!(matches!(
        report
            .named_value_type_table
            .entries
            .iter()
            .find(|entry| entry.path == "store.selected")
            .map(|entry| &entry.flow_type.ty),
        Some(Type::List(_))
    ));
    let checked = output.program.expect("checked program");
    let mapped_inputs = checked
        .calls
        .iter()
        .filter(|call| call.function == "List/map")
        .filter_map(|call| {
            call.entries.iter().find_map(|entry| match entry {
                CheckedCallEntry::Input {
                    value,
                    from_pipe: true,
                    ..
                } => Some(*value),
                _ => None,
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(mapped_inputs.len(), 2);
    assert!(mapped_inputs.iter().all(|input| {
        checked
            .expressions
            .iter()
            .find(|expression| expression.id == *input)
            .is_some_and(|expression| !matches!(expression.kind, CheckedExpressionKind::Delimiter))
    }));
}
#[test]
fn multiline_list_helper_result_is_the_terminal_pipeline_call() {
    let parsed = boon_parser::parse_source(
        "terminal-list-helper-result.bn",
        r#"
FUNCTION select_items(items) {
    items
        |> List/filter(item, if: item.family == TEXT { kept })
        |> List/map(item, new: [label: item.id])
}

items: LIST { [id: TEXT { a }, family: TEXT { kept }] }
mapped: select_items(items: items)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(!output.report.has_errors(), "{:#?}", output.report.diagnostics);
    let program = output.program.unwrap();
    let callable = program
        .callables
        .iter()
        .find(|callable| callable.name == "select_items")
        .unwrap();
    let result = callable.result_expression.unwrap();
    assert!(
        program.expressions.iter().any(|expression| {
            expression.id == result
                && matches!(
                    expression.kind,
                    CheckedExpressionKind::Call { call }
                        if program.calls.iter().any(|candidate| {
                            candidate.id == call && candidate.function == "List/map"
                        })
                )
        }),
        "result: {result:?}; statements: {:#?}",
        parsed.ast.statements,
    );
}

#[test]
fn contextual_filter_predicate_keeps_its_lexical_capture_typed() {
    let parsed = boon_parser::parse_source(
        "typed-filter-capture.bn",
        r#"
store: [
    selected_file: TEXT { first.vcd }
    rows: LIST { [file: TEXT { first.vcd }] }
    selected:
        rows
        |> List/filter(item, if: item.file == selected_file)
]
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(!output.report.has_errors(), "{:#?}", output.report.diagnostics);
    let program = output.program.unwrap();
    let selected_file = program
        .declarations
        .iter()
        .find(|declaration| declaration.name == "selected_file")
        .expect("selected_file declaration")
        .id;
    let predicate = program
        .calls
        .iter()
        .find(|call| call.function == "List/filter")
        .and_then(|call| {
            call.entries.iter().find_map(|entry| match entry {
                CheckedCallEntry::Input { name, value, .. } if name == "if" => Some(*value),
                _ => None,
            })
        })
        .expect("typed filter predicate");
    let right = program
        .expressions
        .iter()
        .find(|expression| expression.id == predicate)
        .and_then(|expression| match expression.kind {
            CheckedExpressionKind::Infix { right, .. } => Some(right),
            _ => None,
        })
        .expect("filter equality right operand");
    let right = program
        .expressions
        .iter()
        .find(|expression| expression.id == right)
        .expect("checked filter capture");
    assert!(
        matches!(
            right.kind,
            CheckedExpressionKind::Read {
                target,
                ref projection,
            } if target == selected_file && projection.is_empty()
        ),
        "filter capture lost lexical identity: {right:#?}"
    );
}
