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
fn inline_multiline_list_append_preserves_the_base_row_shape() {
    let parsed = boon_parser::parse_source(
        "inline-multiline-list-append-row.bn",
        r#"
rows: LIST {
        [title: TEXT { existing }, completed: True]
    }
    |> List/append(item: [title: TEXT { added }])
    |> List/map(item, new: item.completed |> WHEN {
        True => True
        False => False
        __ => False
    })
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "inline list append diagnostics: {:#?}",
        output.report.diagnostics
    );
    assert!(matches!(
        output
            .report
            .named_value_type_table
            .entries
            .iter()
            .find(|entry| entry.path == "rows")
            .map(|entry| &entry.flow_type.ty),
        Some(Type::List(item)) if matches!(item.as_ref(), Type::VariantSet(_))
    ));
}

#[test]
fn recovery_tagged_pattern_domains_preserve_payload_shape_through_wrappers() {
    let parsed = boon_parser::parse_source(
        "tagged-pattern-wrapper.bn",
        r#"
FUNCTION interactive_state(of) {
    of |> WHEN {
        Interactive[hovered] => hovered
        Plain => False
    }
}

FUNCTION wrapped_state(of) {
    interactive_state(of: of)
}

result: wrapped_state(of: Interactive[hovered: True])
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "tagged wrapper diagnostics: {:#?}",
        output.report.diagnostics
    );
}

#[test]
fn typed_find_branch_and_fallback_propagate_the_complete_wrapper_domain() {
    let parsed = boon_parser::parse_source(
        "tagged-selector-find-domain.bn",
        r#"
rows: LIST {
    [id: 1, value: BinaryValue[bits: TEXT { 01 }]]
    [id: 2, value: StringValue[text: TEXT { ready }]]
}

selected:
    rows
    |> List/find(item, if: item.id == 1)
    |> WHEN {
        Found[value] => value.value
        NotFound => StringValue[text: Text/empty()]
    }

label: value_text(value: selected)
segments:
    rows
    |> List/map(item, new: segment(transition: item))

FUNCTION segment(transition) {
    [label: value_text(value: transition.value)]
}

FUNCTION value_text(value) {
    value |> WHEN {
        BinaryValue => value.bits
        StringValue => value.text
        __ => TEXT { ? }
    }
}
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("typed find program");
    let value_type = &program
        .callables
        .iter()
        .find(|callable| callable.name == "value_text")
        .expect("value_text callable")
        .parameters[0]
        .flow_type
        .ty;
    let Type::VariantSet(variants) = value_type else {
        panic!("value_text domain is not a variant set: {value_type:?}");
    };
    assert!(variants.iter().any(
        |variant| matches!(variant, Variant::Tagged { tag, .. } if tag == "BinaryValue")
    ));
    assert!(variants.iter().any(
        |variant| matches!(variant, Variant::Tagged { tag, .. } if tag == "StringValue")
    ));
    let transition = &program
        .callables
        .iter()
        .find(|callable| callable.name == "segment")
        .expect("segment callable")
        .parameters[0]
        .flow_type
        .ty;
    let Type::Object(transition) = transition else {
        panic!("segment transition is not an object: {transition:?}");
    };
    assert_eq!(transition.fields.get("value"), Some(value_type));
}

#[test]
fn recovery_tagged_call_discriminants_specialize_user_results() {
    let parsed = boon_parser::parse_source(
        "tagged-call-result.bn",
        r#"
FUNCTION dispatch(request) {
    request |> WHEN {
        TextRequest[value] => value
        ListRequest[value] => LIST { value }
    }
}

FUNCTION requires_text(value) {
    value |> Text/is_empty()
}

FUNCTION wrapper(request) {
    dispatch(request: request)
}

result:
    wrapper(request: TextRequest[value: TEXT { ready }])
    |> requires_text()
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "tag-discriminated result diagnostics: {:#?}",
        output.report.diagnostics
    );
}

#[test]
fn projected_tag_discriminants_specialize_only_the_concrete_call_occurrence() {
    let parsed = boon_parser::parse_source(
        "projected-tag-call-result.bn",
        r#"
FUNCTION make_row() {
    [item_kind: VariableRow, label: TEXT { ready }]
}

FUNCTION selected_item(row) {
    row.item_kind |> WHEN {
        VariableRow => [item_kind: VariableRow, label: row.label]
        __ => row
    }
}

rows:
    LIST { 1 }
    |> List/map(item, new: selected_item(row: make_row()))
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "projected discriminant diagnostics: {:#?}",
        output.report.diagnostics
    );
    let rows = output
        .report
        .named_value_type_table
        .entries
        .iter()
        .find(|entry| entry.path == "rows")
        .expect("rows type");
    let Type::List(item) = &rows.flow_type.ty else {
        panic!("rows must remain a list: {:#?}", rows.flow_type.ty);
    };
    let Type::Object(item) = item.as_ref() else {
        panic!("rows item must be an object: {item:#?}");
    };
    assert!(
        !item.open,
        "the exact selected_item occurrence must close the mapped item: {item:#?}"
    );

    let principal = output
        .report
        .function_type_table
        .entries
        .iter()
        .find(|entry| entry.name == "selected_item")
        .expect("selected_item principal signature");
    assert!(
        matches!(&principal.result.ty, Type::Object(shape) if shape.open),
        "the wildcard callable principal must remain open: {:#?}",
        principal.result.ty
    );
}

#[test]
fn nested_tag_dispatch_preserves_structured_delimiter_results_for_spreads() {
    let parsed = boon_parser::parse_source(
        "nested-tag-dispatch-delimiter-result.bn",
        r#"
FUNCTION text_style(of, backend) {
    backend |> WHEN {
        Classic => of |> WHEN {
            Hero => [font_size: 64, weight: 300]
            Body => [font_size: 16, weight: 400]
        }
        Alternate => of |> WHEN {
            Hero => [font_size: 62, weight: 500, tracking: 1]
            Body => [font_size: 15, weight: 450, tracking: 0]
        }
    }
}

FUNCTION theme_get(request, backend) {
    request |> WHEN {
        Text[of] => text_style(of: of, backend: backend)
        Spacing[of] => 10
    }
}

FUNCTION theme_text(of, backend) {
    theme_get(request: Text[of: of], backend: backend)
}

FUNCTION make_style(backend) {
    [
        ...theme_text(of: Hero, backend: backend)
        width: 320
    ]
}

FUNCTION make_fixed_style() {
    [
        ...theme_get(request: Text[of: Hero], backend: Classic)
        width: 320
    ]
}

classic_style: make_style(backend: Classic)
alternate_style: make_style(backend: Alternate)
fixed_style: make_fixed_style()
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "nested delimiter result diagnostics: {:#?}",
        output.report.diagnostics
    );
    let style = output
        .report
        .named_value_type_table
        .entries
        .iter()
        .find(|entry| entry.path == "classic_style")
        .expect("classic style type");
    let Type::Object(style) = &style.flow_type.ty else {
        panic!("spread result must remain an object: {:#?}", style.flow_type.ty);
    };
    assert_eq!(style.fields.get("width"), Some(&Type::Number));
}

#[test]
fn recovery_tagged_constructor_wrappers_propagate_payload_domains() {
    let parsed = boon_parser::parse_source(
        "tagged-constructor-wrapper.bn",
        r#"
FUNCTION leaf(value) {
    value |> WHEN {
        Plain => 1
        Fancy[hovered] => 2
    }
}

FUNCTION dispatch(request) {
    request |> WHEN {
        Request[of] => leaf(value: of)
    }
}

FUNCTION wrapper(of) {
    dispatch(request: Request[of: of])
}

first: wrapper(of: Plain)
second: wrapper(of: Fancy[hovered: True])
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "tagged constructor wrapper diagnostics: {:#?}",
        output.report.diagnostics
    );
    let request = output
        .report
        .function_type_table
        .entries
        .iter()
        .find(|entry| entry.name == "dispatch")
        .and_then(|entry| entry.parameters.iter().find(|parameter| parameter.name == "request"))
        .expect("dispatch request parameter");
    assert!(matches!(
        &request.flow_type.ty,
        Type::VariantSet(variants) if variants.iter().any(|variant| matches!(
            variant,
            Variant::Tagged { tag, fields } if tag == "Request"
                && matches!(fields.fields.get("of"), Some(Type::VariantSet(payloads))
                    if payloads.iter().any(|payload| matches!(payload,
                        Variant::Tagged { tag, .. } if tag == "Fancy")))
        ))
    ));
}

#[test]
fn recovery_when_arm_reads_use_the_reachable_selector_domain() {
    let parsed = boon_parser::parse_source(
        "narrowed-when-call-argument.bn",
        r#"
FUNCTION exact(of) {
    of |> WHEN { Exact => 1 }
}

FUNCTION residual(of) {
    of |> WHEN { Other => 2, Third => 3 }
}

FUNCTION wrapper(of) {
    of |> WHEN {
        Exact => exact(of: of)
        __ => residual(of: of)
    }
}

first: wrapper(of: Exact)
second: wrapper(of: Other)
third: wrapper(of: Third)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "narrowed selector diagnostics: {:#?}",
        output.report.diagnostics
    );
}

#[test]
fn recovery_reactive_latest_boundaries_do_not_form_pure_expansion_cycles() {
    let parsed = boon_parser::parse_source(
        "reactive-local-cycle.bn",
        r#"
FUNCTION editor(events) {
    committed:
        LATEST {
            events.blur |> THEN { draft }
            events.key_down |> THEN { draft }
        }
    draft:
        LATEST {
            TEXT {}
            events.change.text
            committed |> THEN { TEXT {} }
        }
    draft
}

store: [
    events: [
        blur: SOURCE
        change: SOURCE
        key_down: SOURCE
    ]
    value: editor(events: events) |> Text/is_empty()
]
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "reactive boundary diagnostics: {:#?}",
        output.report.diagnostics
    );
}

#[test]
fn recovery_block_locals_do_not_replace_the_final_value() {
    let parsed = boon_parser::parse_source(
        "block-local-result.bn",
        r#"
FUNCTION make() {
    [
        edited_title: BLOCK {
            draft_title: TEXT { ready }
            draft_title
        }
    ]
}

FUNCTION consume(item) {
    item.edited_title |> Text/is_empty()
}

result: make() |> consume()
"#,
    )
    .unwrap();
    {
        let (checker, _) = Checker::new_profiled(&parsed);
        let statement = checker
            .function_statements
            .get("make")
            .copied()
            .expect("make function statement");
        let Type::Object(result) = checker
            .function_body_return_type("make", statement, &mut BTreeSet::new())
            .expect("static make result")
        else {
            panic!("make must return an object");
        };
        assert!(
            matches!(result.fields.get("edited_title"), Some(Type::Text)),
            "static make result: {result:#?}"
        );
    }
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "block-local result diagnostics: {:#?}",
        output.report.diagnostics
    );
}

#[test]
fn recovery_reactive_block_forward_references_preserve_the_final_value() {
    let parsed = boon_parser::parse_source(
        "reactive-block-forward-result.bn",
        r#"
FUNCTION reactive_item(title, events) {
    [
        title_to_update:
            LATEST {
                events.blur |> THEN { edited_title }
                events.key_down |> THEN { edited_title }
            }
        title:
            LATEST {
                title
                title_to_update
            }
        edited_title: BLOCK {
            draft_title:
                LATEST {
                    Text/empty()
                    events.change.text
                    title_to_update |> THEN { Text/empty() }
                }
            LATEST {
                draft_title
                events.begin |> THEN {
                    draft_title
                        |> Text/is_empty()
                        |> WHEN { True => title, False => SKIP }
                }
            }
        }
    ]
}

FUNCTION consume(item) {
    item.edited_title |> Text/is_empty()
}

store: [
    events: [
        begin: SOURCE
        blur: SOURCE
        change: SOURCE
        key_down: SOURCE
    ]
    items:
        LIST { [title: TEXT { ready }] }
        |> List/map(item, new: reactive_item(title: item.title, events: events))
        |> List/retain(item, if: True)
    visible: items |> List/retain(item, if: True)
    value: visible |> List/map(item, new: consume(item: item))
]
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(
        !output.report.has_errors(),
        "reactive block diagnostics: {:#?}",
        output.report.diagnostics
    );
}

#[test]
fn recovery_pure_local_expansion_cycles_remain_rejected() {
    let parsed = boon_parser::parse_source(
        "pure-local-cycle.bn",
        r#"
FUNCTION invalid_cycle() {
    left: right
    right: left
    left
}

value: invalid_cycle()
"#,
    )
    .unwrap();
    let output = check_program(&parsed);

    assert!(output.report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("canonical checked value contains an expansion cycle")
    }));
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

fn assert_named_capture_mode(program: &CheckedProgram, name: &str, expected: FlowMode) {
    let targets = program
        .declarations
        .iter()
        .filter(|declaration| declaration.name == name)
        .map(|declaration| declaration.id)
        .collect::<Vec<_>>();
    assert!(!targets.is_empty(), "missing `{name}` declaration");
    assert!(
        program.expressions.iter().any(|expression| {
            matches!(
                expression.kind,
                CheckedExpressionKind::Read { target, .. } if targets.contains(&target)
            ) && expression.flow_type.mode == expected
        }),
        "no exact `{name}` capture has {expected:?} flow"
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
    assert_named_capture_mode(&program, "chosen", FlowMode::PresentOrAbsent);
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
    assert_named_capture_mode(&program, "chosen", FlowMode::Continuous);
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
    assert_named_capture_mode(&program, "chosen", FlowMode::PresentOrAbsent);
}
