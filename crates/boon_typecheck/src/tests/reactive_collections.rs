#[test]
fn function_return_inference_uses_while_branch_values_instead_of_the_selector() {
    let parsed = boon_parser::parse_source(
        "function-while-list-result.bn",
        r#"
FUNCTION values_for(number) {
    number == 1 |> WHILE {
        True => LIST { TEXT { selected }, TEXT { shared } }
        False => LIST { TEXT { other } }
    }
}

store: [
    rows:
        List/range(from: 0, to: 1)
        |> List/map(item, new: [number: item, values: values_for(number: item)])
    selected:
        rows
        |> List/filter(item, if:
            item.values
            |> List/any(item, if: item == TEXT { selected })
        )
]
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "WHILE-selected function result diagnostics: {:#?}",
        output.report.diagnostics
    );
    assert!(matches!(
        output
            .report
            .named_value_type_table
            .entries
            .iter()
            .find(|entry| entry.path == "store.selected")
            .map(|entry| &entry.flow_type.ty),
        Some(Type::List(_))
    ));
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
fn closed_truth_set_has_no_public_bool_type_alias() {
    let truth = Type::VariantSet(vec![
        Variant::Tag("False".to_owned()),
        Variant::Tag("True".to_owned()),
    ]);
    assert_eq!(boon_facing_type_label(&truth), "True | False");
    assert_eq!(
        boon_facing_type_display_tree(&truth),
        TypeDisplayNode::Union {
            variants: vec![
                TypeDisplayNode::Scalar {
                    label: "True".to_owned(),
                },
                TypeDisplayNode::Scalar {
                    label: "False".to_owned(),
                },
            ],
        }
    );

    let parsed = boon_parser::parse_source(
        "truth-diagnostic.bn",
        "value: TEXT { no } |> Bool/not()\n",
    )
    .unwrap();
    let output = check_program(&parsed);
    let diagnostic = output
        .report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("Bool/not"))
        .expect("Bool/not rejects non-Tag input");
    assert!(diagnostic.message.contains("expected: True | False"));
    assert!(!diagnostic.message.contains(concat!("BO", "OL")));
}











#[test]
fn then_by_rejects_a_plain_or_chain_clearing_input() {
    for source in [
        r#"
rows: LIST { [rank: 1] }
ordered: rows |> List/then_by(item, key: item.rank, direction: Ascending)
"#,
        r#"
rows: LIST { [rank: 1] }
ordered:
    rows
    |> List/sort_by(item, key: item.rank, direction: Ascending)
    |> List/append(item: [rank: 2])
    |> List/then_by(item, key: item.rank, direction: Ascending)
"#,
    ] {
        let parsed = boon_parser::parse_source("invalid-then-by.bn", source).unwrap();
        let output = check_program(&parsed);
        assert!(
            output.program.is_none(),
            "invalid chain was accepted: {source}"
        );
        assert!(output.report.diagnostics.iter().any(|diagnostic| {
            diagnostic
                .message
                .contains("requires a compatible preceding `List/sort_by` order chain")
        }));
    }
}

#[test]
fn order_keys_reject_objects_and_impure_values_through_wrappers() {
    for (source, expected) in [
        (
            r#"
FUNCTION sorted(list, entry: OUT, key) {
    list |> List/sort_by(item: entry, key: key, direction: Ascending)
}
rows: LIST { [name: TEXT { Alpha }] }
ordered: rows |> sorted(entry, key: [name: entry.name])
"#,
            "list order key has unsupported type",
        ),
        (
            r#"
rows: LIST { [name: TEXT { Alpha }] }
ordered:
    rows
    |> List/sort_by(
        item
        key: File/read_text(path: item.name)
        direction: Ascending
    )
"#,
            "list order key must be a continuous pure expression",
        ),
    ] {
        let parsed = boon_parser::parse_source("invalid-order-key.bn", source).unwrap();
        let output = check_program(&parsed);
        assert!(
            output.program.is_none(),
            "invalid key was accepted: {source}"
        );
        assert!(
            output
                .report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected))
        );
    }
}

#[test]
fn order_keys_reject_error_capable_conversions() {
    let parsed = boon_parser::parse_source(
        "partial-order-key.bn",
        r#"
rows: LIST { [rank: TEXT { 1 }] }
ordered: rows |> List/sort_by(item, key: item.rank |> Text/to_number())
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(output.program.is_none(), "partial order key was accepted");
    assert!(
        output
            .report
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.message.contains("list order key must be total") })
    );
}

#[test]
fn typed_list_boundaries_reject_invalid_direction_page_size_and_bool_or_inputs() {
    for (source, expected) in [
        (
            r#"
rows: LIST { [rank: 1] }
ordered: rows |> List/sort_by(item, key: item.rank, direction: Sideways)
"#,
            "argument `direction` has incompatible type",
        ),
        (
            r#"
rows: LIST { [rank: 1] }
page: rows |> List/page(size: 0, after: Start)
"#,
            "size must be a whole Number between 1 and 10000",
        ),
        (
            "value: Bool/or(left: True, right: TEXT { no })\n",
            "argument `right` has incompatible type",
        ),
    ] {
        let parsed = boon_parser::parse_source("invalid-typed-list-boundary.bn", source).unwrap();
        let output = check_program(&parsed);
        assert!(
            output.program.is_none(),
            "invalid input was accepted: {source}"
        );
        assert!(
            output
                .report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected))
        );
    }
}
#[test]
fn then_accepts_transitions_from_initial_latest_state() {
    let parsed = boon_parser::parse_source(
        "initial-latest-state-transition.bn",
        r#"
store: [
    update: SOURCE
    selected:
        LATEST {
            TEXT { initial }
            update |> THEN { TEXT { changed } }
        }
    observed:
        TEXT { waiting } |> HOLD observed {
            selected |> THEN { selected }
        }
]
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
}

#[test]
fn function_capture_preserves_global_event_flow_mode() {
    let parsed = boon_parser::parse_source(
        "global-event-function-capture.bn",
        r#"
store: [
    choose: SOURCE
    chosen:
        choose |> THEN { TEXT { chosen } }
]

FUNCTION remember() {
    TEXT { initial } |> HOLD remembered {
        chosen |> THEN { chosen }
    }
}

result: remember()
"#,
    )
    .unwrap();
    let modes = flow_bindings(&parsed, &ExternalTypeEnvironment::default());
    assert_eq!(
        flow_binding_mode(&modes, "chosen"),
        Some(FlowMode::PresentOrAbsent)
    );
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("captured event program");
    assert!(program.expressions.iter().any(|expression| {
        matches!(
            expression.kind,
            CheckedExpressionKind::ExternalRead {
                ref canonical_path,
                external_identity: None,
            } if canonical_path == "chosen"
        ) && expression.flow_type.mode == FlowMode::PresentOrAbsent
    }));
}

#[test]
fn function_capture_can_trigger_on_global_initial_latest_state_transition() {
    let parsed = boon_parser::parse_source(
        "global-state-function-capture.bn",
        r#"
store: [
    choose: SOURCE
    chosen:
        LATEST {
            TEXT { initial }
            choose |> THEN { TEXT { chosen } }
        }
]

FUNCTION remember() {
    TEXT { waiting } |> HOLD remembered {
        chosen |> THEN { chosen }
    }
}

result: remember()
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("captured state program");
    assert!(program.expressions.iter().any(|expression| {
        matches!(
            expression.kind,
            CheckedExpressionKind::ExternalRead {
                ref canonical_path,
                external_identity: None,
            } if canonical_path == "chosen"
        ) && expression.flow_type.mode == FlowMode::Continuous
    }));
}

#[test]
fn function_capture_preserves_event_only_latest_flow_mode() {
    let parsed = boon_parser::parse_source(
        "global-event-latest-function-capture.bn",
        r#"
store: [
    choose_first: SOURCE
    choose_second: SOURCE
    first:
        choose_first |> THEN { TEXT { first } }
    second:
        choose_second |> THEN { TEXT { second } }
    chosen:
        LATEST {
            first
            second
        }
]

FUNCTION remember() {
    TEXT { initial } |> HOLD remembered {
        chosen |> THEN { chosen }
    }
}

result: remember()
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("captured event-only latest program");
    assert!(program.expressions.iter().any(|expression| {
        matches!(
            expression.kind,
            CheckedExpressionKind::ExternalRead {
                ref canonical_path,
                external_identity: None,
            } if canonical_path == "chosen"
        ) && expression.flow_type.mode == FlowMode::PresentOrAbsent
    }));
}
