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
            item.controls.select |> THEN { item.name }
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
    let output = check_program(&parsed);
    let report = &output.report;
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
    let checked = output
        .program
        .as_ref()
        .expect("mapped row event has checked program");
    let map = checked
        .calls
        .iter()
        .filter(|call| call.function == "List/map")
        .find(|call| {
            checked_call_input(call, "new").is_some_and(|new| {
                matches!(
                    checked.expressions[new.0 as usize].kind,
                    CheckedExpressionKind::Then { .. }
                )
            })
        })
        .expect("event projection map call");
    let new = checked_call_input(map, "new").expect("map new argument");
    assert_eq!(
        checked.expressions[new.0 as usize].flow_type.mode,
        FlowMode::PresentOrAbsent
    );
    assert_eq!(map.result.mode, FlowMode::PresentOrAbsent);
    let latest = checked
        .calls
        .iter()
        .find(|call| call.function == "List/latest")
        .expect("latest call");
    assert_eq!(latest.result.mode, FlowMode::PresentOrAbsent);
    assert_eq!(
        report
            .named_value_type_table
            .entries
            .iter()
            .find(|entry| entry.path == "store.selected")
            .map(|entry| entry.flow_type.mode),
        Some(FlowMode::PresentOrAbsent)
    );
}

#[test]
fn mapped_row_event_projection_survives_user_function_selection() {
    let parsed = boon_parser::parse_source(
        "mapped-selected-row-event.bn",
        r#"
store: [
    rows:
        LIST {
            [kind: Primary, name: TEXT { one }]
            [kind: Secondary, name: TEXT { two }]
        }
        |> List/map(item, new: selected_row(row: item))
    selected:
        rows
        |> List/map(item, new:
            item.controls.select |> THEN { item.name }
        )
        |> List/latest()
]

FUNCTION selected_row(row) {
    row.kind |> WHEN {
        Primary => [controls: [select: SOURCE], name: row.name]
        __ => [controls: [select: SOURCE], name: row.name]
    }
}
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "selected mapped row diagnostics: {:#?}",
        output.report.diagnostics
    );
    let checked = output.program.expect("selected mapped row is checked");
    let projected = checked
        .expressions
        .iter()
        .find(|expression| {
            matches!(
                &expression.kind,
                CheckedExpressionKind::Read { projection, .. }
                    if projection == &["controls".to_owned(), "select".to_owned()]
            )
        })
        .expect("mapped row event projection");
    assert_eq!(projected.flow_type.mode, FlowMode::PresentOrAbsent);
}

#[test]
fn mapped_user_function_preserves_a_parameter_seeded_recursive_hold_field() {
    let parsed = boon_parser::parse_source(
        "mapped-recursive-hold-field.bn",
        r#"
store: [
    cycle: SOURCE
    rows:
        LIST {
            [kind: VariableRow, formatter: Hexadecimal]
            [kind: GroupRow, formatter: Binary]
        }
        |> List/map(item, new: selected_row(row: item))
    variable_rows:
        rows
        |> List/filter(item, if: item.kind == VariableRow)
    formatter_source_rows:
        True |> WHEN {
            True => variable_rows
            False => variable_rows
        }
    formatters:
        formatter_source_rows
        |> List/map(item, new: formatter_value(row: item))
]

FUNCTION selected_row(row) {
    row.kind |> WHEN {
        VariableRow => stateful_row(row: row)
        __ => row
    }
}

FUNCTION stateful_row(row) {
    [
        kind: row.kind
        formatter:
            row.formatter |> HOLD formatter {
                store.cycle |> THEN { formatter }
            }
    ]
}

FUNCTION formatter_value(row) {
    row.formatter
}
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "recursive HOLD result field diagnostics: {:#?}",
        output.report.diagnostics
    );
    assert_eq!(
        output
            .report
            .named_value_type_table
            .entries
            .iter()
            .find(|entry| entry.path == "store.formatters")
            .map(|entry| &entry.flow_type.ty),
        Some(&Type::List(Box::new(Type::VariantSet(vec![
            Variant::Tag("Binary".to_owned()),
            Variant::Tag("Hexadecimal".to_owned()),
        ]))))
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
fn append_candidate_keeps_its_event_record_type_inside_a_feedback_list() {
    let parsed = boon_parser::parse_source(
        "append-candidate-record-type.bn",
        r#"
store: [
    add: SOURCE
    candidate:
        add |> THEN {
            entries |> List/any(item, if: item.id == add.text) |> WHEN {
                True => SKIP
                False => [id: add.text]
            }
        }
    entries:
        LIST {}
        |> List/append(item: candidate)
        |> List/map(item, new: [id: item.id])
]
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "append feedback source must typecheck: {:?}",
        output.report.diagnostics
    );
    let candidate = output
        .report
        .named_value_type_table
        .entries
        .iter()
        .find(|entry| entry.path == "store.candidate")
        .expect("candidate type");
    assert_eq!(
        candidate.flow_type.ty,
        Type::Object(ObjectShape::from_ordered_fields(
            [("id".to_owned(), Type::Text)],
            false,
        ))
    );
    assert_eq!(candidate.flow_type.mode, FlowMode::PresentOrAbsent);
    let checked = output.program.expect("checked append feedback program");
    let declaration = checked
        .declarations
        .iter()
        .find(|declaration| {
            declaration.name == "candidate" && declaration.kind == CheckedDeclarationKind::Field
        })
        .expect("candidate declaration");
    let value = declaration.value.expect("candidate expression");
    assert_eq!(
        checked.expressions[value.0 as usize].flow_type, candidate.flow_type,
        "the checked declaration and its executable root must have one type"
    );
}

#[test]
fn empty_list_append_exposes_a_closed_checked_output_type() {
    let parsed = boon_parser::parse_source(
        "append-closed-output.bn",
        r#"
store: [
    add: SOURCE
    entries:
        LIST {}
        |> List/append(item: add |> THEN { [id: TEXT { one }] })
]
outputs: [
    entries: store.entries
]
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "generic List/append must specialize the checked output: {:?}",
        output.report.diagnostics
    );
    assert_eq!(
        output.report.output_root_types[0].ty,
        Type::List(Box::new(Type::Object(ObjectShape::from_ordered_fields(
            [("id".to_owned(), Type::Text)],
            false,
        ))))
    );
}

#[test]
fn tagged_effect_result_candidate_keeps_exact_projected_record_fields() {
    let parsed = boon_parser::parse_source(
        "typed-passkey-effect-candidate.bn",
        include_str!("../../../../testdata/typed_passkey_effects.bn"),
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "typed effect fixture must typecheck: {:?}",
        output.report.diagnostics
    );
    let expected = Type::Object(ObjectShape::from_ordered_fields(
        [
            ("credential_id".to_owned(), Type::Text),
            ("label".to_owned(), Type::Text),
        ],
        false,
    ));
    let reported = output
        .report
        .named_value_type_table
        .entries
        .iter()
        .find(|entry| entry.path == "store.registration_candidate")
        .map(|entry| entry.flow_type.ty.clone());
    let checked = output.program.expect("checked typed effect program");
    let declaration = checked
        .declarations
        .iter()
        .find(|declaration| {
            declaration.name == "registration_candidate"
                && declaration.kind == CheckedDeclarationKind::Field
        })
        .expect("registration candidate declaration");
    assert_eq!(reported, Some(expected.clone()));
    assert_eq!(declaration.flow_type.ty, expected);
    let value = declaration
        .value
        .expect("registration candidate expression");
    assert_eq!(checked.expressions[value.0 as usize].flow_type.ty, expected);
}

#[test]
fn contextual_remove_latest_owns_two_checked_predicate_branches() {
    let parsed = boon_parser::parse_source(
        "contextual-remove-latest.bn",
        r#"
store: [
    clear: SOURCE
    rows:
        LIST { [completed: False] }
        |> List/map(item, new: [
            ...item
            remove: SOURCE
        ])
        |> List/remove(item, when:
            LATEST {
                item.remove |> THEN { True }
                clear |> THEN { item.completed }
            }
        )
]
"#,
    )
    .unwrap();
    let latest = parsed
        .expressions
        .iter()
        .find(|expression| matches!(expression.kind, AstExprKind::Latest { .. }))
        .expect("nested LATEST expression");
    let AstExprKind::Latest { branches } = &latest.kind else {
        unreachable!("matched LATEST")
    };
    assert_eq!(
        branches.len(),
        2,
        "nested LATEST must own its two predicate branches: {branches:?}"
    );

    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "nested LATEST predicate must be BOOL: {:#?}",
        output.report.diagnostics
    );
    let checked = output.program.expect("checked program");
    assert!(checked.expressions.iter().any(|expression| {
        matches!(
            &expression.kind,
            CheckedExpressionKind::Latest { branches } if branches.len() == 2
        )
    }));
}

#[test]
fn transient_latest_branches_remain_event_flows() {
    let parsed = boon_parser::parse_source(
        "transient-latest-flow.bn",
        r#"
store: [
    elements: [ready: SOURCE, fire: SOURCE]
    fingerprint: TEXT { request }
    request:
        LATEST {
            elements.ready.event.press |> WHEN {
                True => fingerprint
                False => SKIP
            }
            elements.fire.event.press |> THEN { fingerprint }
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
    let checked = output.program.expect("checked program");
    let latest = checked
        .expressions
        .iter()
        .find_map(|expression| match &expression.kind {
            CheckedExpressionKind::Latest { branches } => Some((expression, branches)),
            _ => None,
        })
        .expect("checked LATEST");
    assert!(
        latest.1.iter().all(|branch| matches!(
            checked.expressions[branch.0 as usize].flow_type.mode,
            FlowMode::TickPresent | FlowMode::PresentOrAbsent
        )),
        "transient LATEST branch flows: {:#?}",
        latest
            .1
            .iter()
            .map(|branch| &checked.expressions[branch.0 as usize])
            .collect::<Vec<_>>()
    );
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
    store.groups |> List/map(item, new: item)
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
    items |> List/filter(item, if: item.family == TEXT { kept }) |> List/map(item, new: [label: item.id])
}

items: LIST { [id: TEXT { a }, family: TEXT { kept }] }
mapped: select_items(items: items)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "{:#?}",
        output.report.diagnostics
    );
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
    assert!(
        !output.report.has_errors(),
        "{:#?}",
        output.report.diagnostics
    );
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
                ..
            } if target == selected_file && projection.is_empty()
        ),
        "filter capture lost lexical identity: {right:#?}"
    );
}

#[test]
fn typed_order_take_and_page_preserve_exact_rows_and_order_provenance() {
    let parsed = boon_parser::parse_source(
        "typed-order-page.bn",
        r#"
rows: LIST {
    [name: TEXT { Beta }, rank: 2]
    [name: TEXT { Alpha }, rank: 1]
}
ordered:
    rows
    |> List/sort_by(item, key: item.name)
    |> List/filter(item, if: True)
    |> List/then_by(item, key: item.rank, direction: Descending)
preview: ordered |> List/take(count: 20)
page: preview |> List/page(size: 20, after: Start)
either: Bool/or(left: True, right: False)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("typed list pipeline is checked");
    let row_type = Type::Object(ObjectShape::from_ordered_fields(
        [
            ("name".to_owned(), Type::Text),
            ("rank".to_owned(), Type::Number),
        ],
        false,
    ));
    for function in ["List/sort_by", "List/then_by", "List/take"] {
        let call = program
            .calls
            .iter()
            .find(|call| call.function == function)
            .unwrap_or_else(|| panic!("missing `{function}` call"));
        assert_eq!(call.result.ty, Type::List(Box::new(row_type.clone())));
    }
    let page = program
        .calls
        .iter()
        .find(|call| call.function == "List/page")
        .expect("page call");
    let chain = program
        .order_chain_for_call(page.id)
        .expect("filter, take, and page preserve the order chain");
    assert_eq!(chain.keys.len(), 2);
    assert_eq!(chain.keys[0].key_type, Type::Text);
    assert_eq!(chain.keys[1].key_type, Type::Number);
    assert!(chain.keys.iter().all(|key| key.pure));
    assert_eq!(chain.keys[0].direction, CheckedOrderDirection::Ascending);
    assert_eq!(chain.keys[1].direction, CheckedOrderDirection::Descending);
    assert!(chain.keys.iter().all(|key| key.total));

    let Type::VariantSet(variants) = &page.result.ty else {
        panic!(
            "page result must be a closed variant set: {:#?}",
            page.result.ty
        )
    };
    let items = variants.iter().find_map(|variant| match variant {
        Variant::Tagged { tag, fields } if tag == "Page" => fields.fields.get("items"),
        _ => None,
    });
    assert_eq!(items, Some(&Type::List(Box::new(row_type))));
}

#[test]
fn transparent_generic_sort_wrapper_preserves_exact_order_chain() {
    let parsed = boon_parser::parse_source(
        "wrapped-order-chain.bn",
        r#"
FUNCTION sorted(list, entry: OUT, key) {
    list |> List/sort_by(item: entry, key: key, direction: Ascending)
}

rows: LIST { [name: TEXT { Alpha }, rank: 1] }
primary: rows |> sorted(entry, key: entry.name)
ordered: primary |> List/then_by(item, key: item.rank, direction: Descending)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("wrapped ordering is checked");
    let then_by = program
        .calls
        .iter()
        .find(|call| call.function == "List/then_by")
        .expect("then_by call");
    let chain = program
        .order_chain_for_call(then_by.id)
        .expect("transparent wrapper preserves primary ordering");
    assert_eq!(chain.keys.len(), 2);
    assert_eq!(chain.keys[0].key_type, Type::Text);
    assert_eq!(chain.keys[0].call_path.len(), 2);
    assert_eq!(chain.keys[1].key_type, Type::Number);
    assert_eq!(chain.keys[1].call_path.len(), 1);
}

#[test]
fn order_provenance_is_stored_normalized_and_semantic_across_branches() {
    let parsed = boon_parser::parse_source(
        "semantic-order-branches.bn",
        r#"
rows: LIST {
    [name: TEXT { Alpha }, rank: 2]
    [name: TEXT { Beta }, rank: 1]
}
primary:
    True |> WHEN {
        True => rows |> List/sort_by(item, key: item.name)
        False => rows |> List/sort_by(item, key: item.name, direction: Ascending)
    }
ordered: primary |> List/then_by(item, key: item.rank, direction: Descending)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("semantic branch ordering is checked");
    let then_by = program
        .calls
        .iter()
        .find(|call| call.function == "List/then_by")
        .expect("then_by call");
    let stored = program
        .order_chains
        .iter()
        .find(|entry| entry.call == then_by.id)
        .expect("authoritative stored order chain");
    assert_eq!(stored.chain.keys.len(), 2);
    assert_eq!(
        stored.chain.keys[0].direction,
        CheckedOrderDirection::Ascending
    );
    assert_eq!(
        stored.chain.keys[1].direction,
        CheckedOrderDirection::Descending
    );
}

#[test]
fn dynamic_order_direction_is_explicit_in_the_checked_chain() {
    let parsed = boon_parser::parse_source(
        "dynamic-order-direction.bn",
        r#"
direction: Ascending |> HOLD direction {}
rows: LIST { [name: TEXT { Alpha }] }
ordered: rows |> List/sort_by(item, key: item.name, direction: direction)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("dynamic direction is checked");
    let sort = program
        .calls
        .iter()
        .find(|call| call.function == "List/sort_by")
        .expect("sort call");
    assert!(matches!(
        program.order_chain_for_call(sort.id).unwrap().keys[0].direction,
        CheckedOrderDirection::Dynamic { .. }
    ));
}

#[test]
fn bool_is_a_canonical_order_key() {
    let parsed = boon_parser::parse_source(
        "bool-order-key.bn",
        r#"
rows: LIST {
    [active: True]
    [active: False]
}
ordered: rows |> List/sort_by(item, key: item.active, direction: Ascending)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("Bool ordering is checked");
    let sort = program
        .calls
        .iter()
        .find(|call| call.function == "List/sort_by")
        .expect("sort call");
    assert!(matches!(
        program.order_chain_for_call(sort.id).unwrap().keys[0].key_type,
        Type::VariantSet(_)
    ));
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
