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
        |> List/map(row, new: selectable_row(row: row))
    selected:
        rows
        |> List/map(row, new:
            row.controls.select.event.press |> THEN { row.name }
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
        |> List/map(old, new: old)
]

FUNCTION selected_values() {
    store.groups
    |> List/find_value(
        field: "id"
        value: TEXT { one }
        target: "values"
        fallback: LIST {}
    )
    |> List/map(old, new: old)
}
"#,
    )
    .unwrap();
    let report = check(&parsed);
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
}
