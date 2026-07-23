// Included by `../tests.rs`; kept in the parent test module for private IR helper access.

#[test]
fn nested_source_structural_projection_is_not_payload() {
    assert!(
        executable_source_payload_projection(
            "controls.select",
            "controls",
            &["select".to_owned()],
        )
        .is_empty()
    );
    assert_eq!(
        executable_source_payload_projection(
            "controls.select",
            "controls",
            &["select".to_owned(), "text".to_owned()],
        ),
        ["text"]
    );
    assert_eq!(
        executable_source_payload_projection("submit", "submit", &["text".to_owned()]),
        ["text"]
    );
}

#[test]
fn structural_group_is_erased_without_losing_child_event_flow() {
    let parsed = boon_parser::parse_source(
        "structural-group-event-flow.bn",
        r#"
store: [
    trigger: SOURCE
    results: [
        child:
            Idle |> HOLD child {
                trigger |> THEN { Done }
            }
    ]
]
"#,
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    assert!(
        ir.derived_values
            .iter()
            .all(|value| value.path != "store.results"),
        "an ordinary structural parent must not duplicate its child storage"
    );
    assert!(
        ir.state_cells
            .iter()
            .any(|state| state.path == "store.results.child")
    );
    exact_state_arm(
        &ir,
        "store.results.child",
        exact_source_cause(&ir, "store.trigger"),
    );
}

#[test]
fn nested_effect_result_is_an_independent_state_event_cause() {
    let parsed = boon_parser::parse_source(
        "nested-effect-result-cause.bn",
        r#"
store: [
    start: SOURCE
    effect: [
        result:
            NotRequested |> HOLD result {
                start |> THEN { Clock/wall() }
            }
    ]
    workflow:
        Idle |> HOLD workflow {
            start |> THEN { Working }
            effect.result |> THEN { Done }
        }
]
"#,
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    let workflow = ir
        .state_cells
        .iter()
        .find(|state| state.path == "store.workflow")
        .expect("workflow state");
    let workflow_causes = ir
        .state_update_arms
        .iter()
        .filter(|arm| arm.state == workflow.id)
        .map(|arm| arm.cause)
        .collect::<BTreeSet<_>>();

    assert!(workflow_causes.contains(&exact_source_cause(&ir, "store.start")));
    assert!(
        workflow_causes.contains(&exact_state_cause(&ir, "store.effect.result")),
        "effect completion must schedule its own turn instead of being folded into the initiating source: {workflow_causes:?}"
    );
}

#[test]
fn held_effect_results_remain_continuous_values_outside_state_update_arms() {
    let parsed = boon_parser::parse_source(
        "held-effect-result-continuous-value.bn",
        r#"
store: [
    start: SOURCE
    change_suffix: SOURCE
    effect_result:
        NotRequested |> HOLD effect_result {
            start |> THEN { Clock/wall() }
        }
    suffix:
        TEXT { first } |> HOLD suffix {
            change_suffix |> THEN { TEXT { second } }
        }
    label:
        effect_result |> WHEN {
            __ => suffix
        }
    workflow:
        Idle |> HOLD workflow {
            effect_result |> THEN { Done }
        }
]
"#,
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    let label = ir
        .derived_values
        .iter()
        .find(|value| value.path == "store.label")
        .expect("derived label");
    assert_eq!(label.kind, DerivedValueKind::Pure);
    assert!(label.trigger_arms.is_empty());

    let workflow = ir
        .state_cells
        .iter()
        .find(|state| state.path == "store.workflow")
        .expect("workflow state");
    assert!(ir.state_update_arms.iter().any(|arm| {
        arm.state == workflow.id
            && arm.cause == exact_state_cause(&ir, "store.effect_result")
    }));
}

#[test]
fn latest_in_hold_body_merges_updates_without_owning_hidden_state() {
    let parsed = boon_parser::parse_source(
        "held-effect-latest-update-merger.bn",
        r#"
store: [
    start: SOURCE
    move: SOURCE
    clock_result:
        NotRequested |> HOLD clock_result {
            start |> THEN { Clock/wall() }
        }
    cursor_result:
        NotRequested |> HOLD cursor_result {
            LATEST {
                clock_result |> WHEN {
                    WallClockRead => Random/bytes(byte_count: 4)
                    __ => SKIP
                }
                move |> THEN {
                    clock_result |> WHEN {
                        WallClockRead => Random/bytes(byte_count: 8)
                        __ => SKIP
                    }
                }
            }
        }
]
"#,
    )
    .unwrap();
    let ir = lower(&parsed).expect("held effect update merger must lower");
    assert_eq!(
        ir.state_cells
            .iter()
            .map(|state| state.path.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["store.clock_result", "store.cursor_result"]),
        "the HOLD body LATEST is an update merger, not a second state cell"
    );
    let cursor = ir
        .state_cells
        .iter()
        .find(|state| state.path == "store.cursor_result")
        .expect("cursor state");
    let causes = ir
        .state_update_arms
        .iter()
        .filter(|arm| arm.state == cursor.id)
        .map(|arm| arm.cause)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        causes,
        BTreeSet::from([
            exact_source_cause(&ir, "store.move"),
            exact_state_cause(&ir, "store.clock_result"),
        ])
    );
}

#[test]
fn latest_nested_in_hold_update_block_does_not_gain_state_authority() {
    let parsed = boon_parser::parse_source(
        "held-effect-block-latest-update-merger.bn",
        r#"
store: [
    start: SOURCE
    move: SOURCE
    clock_result:
        NotRequested |> HOLD clock_result {
            start |> THEN { Clock/wall() }
        }
    cursor_result:
        NotRequested |> HOLD cursor_result {
            BLOCK {
                merged:
                    LATEST {
                        clock_result |> WHEN {
                            WallClockRead => Random/bytes(byte_count: 4)
                            __ => SKIP
                        }
                        move |> THEN {
                            clock_result |> WHEN {
                                WallClockRead => Random/bytes(byte_count: 8)
                                __ => SKIP
                            }
                        }
                    }
                merged
            }
        }
]
"#,
    )
    .unwrap();
    let ir = lower(&parsed).expect("nested update merger must not become a state initializer");
    assert_eq!(
        ir.state_cells
            .iter()
            .map(|state| state.path.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["store.clock_result", "store.cursor_result"])
    );
    assert!(
        ir.derived_values
            .iter()
            .all(|value| value.path != "store.cursor_result.merged"),
        "a block-local HOLD update binding must not become a global derived field"
    );
}

#[test]
fn list_collection_changes_do_not_become_row_event_causes() {
    let parsed = boon_parser::parse_source(
        "list-collection-event-cause.bn",
        r#"
store: [
    clear: SOURCE
    active_file:
        TEXT { first } |> HOLD active_file {
            clear |> THEN { TEXT { none } }
        }
    rows:
        LIST {
            [file: TEXT { first }]
            [file: TEXT { second }]
        }
        |> List/map(item, new: selectable_row(row: item))
    visible_rows:
        rows
        |> List/filter(item, if: item.file == active_file)
    selected:
        visible_rows
        |> List/map(item, new:
            item.controls.select.event.press
                |> THEN { item.file }
        )
        |> List/latest()
]

FUNCTION selectable_row(row) {
    [
        controls: [select: SOURCE]
        file: row.file
    ]
}
"#,
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    let selected = ir
        .derived_values
        .iter()
        .find(|value| value.path == "store.selected")
        .expect("selected row event projection");

    assert!(
        selected
            .sources
            .iter()
            .any(|source| source.ends_with(".controls.select")),
        "row selection source is missing: {:?}",
        selected.sources
    );
    assert!(
        !selected
            .sources
            .iter()
            .any(|source| source == "store.clear"),
        "changing list membership must not masquerade as a row selection event: {:?}",
        selected.sources
    );
}

#[test]
fn press_payload_fields_are_bool_typed() {
    assert_eq!(
        source_payload_data_type(&SourcePayloadField::Named("press".to_owned())),
        SemanticDataType::Bool
    );
    assert_eq!(
        source_payload_data_type(&SourcePayloadField::Named("pointer_x".to_owned())),
        SemanticDataType::Text
    );
}

#[test]
fn view_row_source_alias_resolves_to_unique_canonical_source_path() {
    let sources = [
        ("file_tree_row.file_row_elements.select_file", SourceId(0)),
        ("file_tree_row.scope_row_elements.select_scope", SourceId(1)),
    ];
    assert_eq!(
        canonical_view_source_path(&sources, "row.file_row_elements.select_file")
            .map(|(path, source_id)| (path, source_id.as_usize())),
        Some(("file_tree_row.file_row_elements.select_file", 0))
    );

    let ambiguous = [
        ("left.file_row_elements.select_file", SourceId(0)),
        ("right.file_row_elements.select_file", SourceId(1)),
    ];
    assert!(
        canonical_view_source_path(&ambiguous, "row.file_row_elements.select_file").is_none(),
        "view row aliases must not guess when suffixes are ambiguous"
    );
}

#[test]
fn selected_row_source_projection_resolves_by_unique_source_suffix() {
    let sources = [
        ("item.sources.editor.change", SourceId(0)),
        ("item.sources.editor.commit", SourceId(1)),
    ];
    assert_eq!(
        canonical_view_source_path(&sources, "store.selected_input.sources.editor.change")
            .map(|(path, source_id)| (path, source_id.as_usize())),
        Some(("item.sources.editor.change", 0))
    );

    let ambiguous = [
        ("left.sources.editor.change", SourceId(0)),
        ("right.sources.editor.change", SourceId(1)),
    ];
    assert!(
        canonical_view_source_path(&ambiguous, "store.selected_input.sources.editor.change")
            .is_none(),
        "selected-row source aliases must remain ambiguity-safe"
    );
}

#[test]
fn semantic_symbol_table_reuses_duplicate_category_text_pairs() {
    let mut table = SemanticSymbolTable::default();

    let first = table.intern("field_name", "count");
    let duplicate = table.intern("field_name", "count");
    let same_text_other_category = table.intern("source_label", "count");

    assert_eq!(first, duplicate);
    assert_ne!(first, same_text_other_category);

    let entries = table.into_entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id, first);
    assert_eq!(entries[0].category, "field_name");
    assert_eq!(entries[0].text, "count");
    assert_eq!(entries[1].id, same_text_other_category);
    assert_eq!(entries[1].category, "source_label");
    assert_eq!(entries[1].text, "count");
}

#[test]
fn source_payload_match_preserves_nested_numeric_infix_operator() {
    let source = r#"
store: [
elements: [
    keyboard_capture: SOURCE
]
zoom_step:
    0 |> HOLD zoom_step {
        elements.keyboard_capture.key |> WHEN {
            W => zoom_step * 2
            __ => SKIP
        }
    }
]
"#;
    let parsed =
        boon_parser::parse_source("source-payload-unsupported-nested-op.bn", source).unwrap();
    let program = lower(&parsed).expect("typed numeric operators must survive executable lowering");
    let arm = exact_state_arm(
        &program,
        "zoom_step",
        exact_source_cause(&program, "elements.keyboard_capture"),
    );
    assert!(
        exact_subtree(&program, arm.output_expression_id)
            .iter()
            .any(|expression| matches!(
                &expression.kind,
                ExecutableExpressionKind::Infix { op, .. } if op == "*"
            )),
        "nested multiplication must remain an exact executable expression"
    );
}

#[test]
fn projected_helper_field_access_does_not_create_persistent_helper_fields() {
    let source = r#"
store: [
    flavors:
        LIST {
            [id: TEXT { left }, suffix: TEXT { left }]
            [id: TEXT { right }, suffix: TEXT { right }]
        }
    rows:
        LIST {
            [id: TEXT { a }, name: TEXT { A }]
        }
    projected:
        flavors |> List/map(item, new: projected_flavor(flavor: item))
]

FUNCTION projected_flavor(flavor) {
    [
        flavor_id: flavor.id
        detail_label:
            rows
            |> List/map(item, new: detail_row(row: item, suffix: flavor.suffix).label)
            |> List/latest()
    ]
}

FUNCTION detail_row(row, suffix) {
    [
        label: row.name |> Text/concat(with: suffix, separator: ":")
    ]
}

document: Document/new(root: Element/label(element: [], label: TEXT { Rows }))
"#;
    let parsed = boon_parser::parse_source("projected-helper-field-access.bn", source).unwrap();
    let ir = lower(&parsed).unwrap();

    assert!(
        !ir.derived_values
            .iter()
            .any(|value| value.path == "flavor.detail_label.label"),
        "helper-local record fields projected through `.label` must not become persistent row fields: {:?}",
        ir.derived_values
            .iter()
            .map(|value| (&value.path, &value.kind))
            .collect::<Vec<_>>()
    );
    assert!(
        !ir.derived_values.iter().any(|value| {
            value.path == "detail_label" && value.kind == DerivedValueKind::ListView
        }),
        "helper-local projected fields must not create a top-level detail_label list view"
    );
    assert!(ir.static_schedule_verified);
}

#[test]
fn source_named_events_keeps_distinct_payload_and_view_bindings() {
    let source = r#"
store: [
    controls: [
        admin: [
            status: SOURCE
            events: SOURCE
        ]
    ]

    page:
        Public |> HOLD page {
            LATEST {
                controls.admin.status.event.press |> THEN { Status }
                controls.admin.events.event.press |> THEN { Events }
            }
        }
]

scene: Scene/new(
    root: Scene/Element/stripe(
        element: []
        direction: Row
        gap: 4
        style: [width: Fill, height: Fill]
        items: LIST {
            Scene/Element/button(
                element: [events: [press: store.controls.admin.status]]
                style: [width: 80, height: 40]
                label: TEXT { Status }
            )
            Scene/Element/button(
                element: [events: [press: store.controls.admin.events]]
                style: [width: 80, height: 40]
                label: TEXT { Events }
            )
        }
    )
)
"#;
    let parsed = boon_parser::parse_source("nested-source-events-collision.bn", source).unwrap();
    let ir = lower(&parsed).expect("source names must not collide with event payload markers");

    assert_eq!(
        ir.sources
            .iter()
            .map(|source| source.path.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["store.controls.admin.events", "store.controls.admin.status",])
    );
    for source_path in ["store.controls.admin.status", "store.controls.admin.events"] {
        let source = ir
            .sources
            .iter()
            .find(|source| source.path == source_path)
            .unwrap_or_else(|| panic!("missing source {source_path}"));
        assert!(
            source
                .payload_schema
                .fields
                .contains(&SourcePayloadField::Named("press".to_owned())),
            "missing press payload for {source_path}: {:?}",
            source.payload_schema
        );
        exact_state_arm(&ir, "store.page", EventCause::Source(source.id));
        assert!(ir.view_bindings.iter().any(|binding| {
            binding.path == source_path
                && binding.target == ViewBindingTarget::Source { source: source.id }
        }));
    }
}

#[test]
fn data_view_bindings_reference_the_authoritative_erased_read() {
    let parsed = boon_parser::parse_source(
        "exact-view-read.bn",
        r#"
store: [
    increment: SOURCE
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
]

document: Document/new(
    root: Element/label(element: [], style: [], label: store.count)
)
"#,
    )
    .unwrap();
    let mut ir = lower(&parsed).expect("data view read must lower exactly");
    let binding = ir
        .view_bindings
        .iter()
        .find(|binding| binding.attr == "label" && binding.kind == ViewBindingKind::Data)
        .expect("label data binding");
    let ViewBindingTarget::Read {
        read,
        additional_projection,
    } = &binding.target
    else {
        panic!("data binding did not retain ErasedReadId: {binding:?}");
    };
    assert!(additional_projection.is_empty());
    let exact = &ir.scope_index.reads[read.as_usize()];
    assert_eq!(exact.id, *read);
    assert!(matches!(exact.target, ErasedReadTarget::Binding { .. }));

    let binding = ir
        .view_bindings
        .iter_mut()
        .find(|binding| binding.attr == "label" && binding.kind == ViewBindingKind::Data)
        .expect("label data binding");
    binding.target = ViewBindingTarget::Read {
        read: ErasedReadId(ir.scope_index.reads.len()),
        additional_projection: Vec::new(),
    };
    assert!(
        verify_static_schedule(&ir)
            .unwrap_err()
            .contains("references missing erased read")
    );
}

#[test]
fn nested_data_view_projection_is_owned_by_the_erased_read() {
    let parsed = boon_parser::parse_source(
        "nested-exact-view-read.bn",
        r#"
store: [
    value: 7
    outer: [inner: [value: value]]
]

document: Document/new(
    root: Element/label(element: [], style: [], label: store.outer.inner.value)
)
"#,
    )
    .unwrap();
    let ir = lower(&parsed).expect("nested data view read must lower exactly");
    let binding = ir
        .view_bindings
        .iter()
        .find(|binding| binding.attr == "label" && binding.kind == ViewBindingKind::Data)
        .expect("nested label data binding");
    assert_eq!(binding.path, "store.outer.inner.value");
    let ViewBindingTarget::Read {
        read,
        additional_projection,
    } = &binding.target
    else {
        panic!("nested data binding did not retain ErasedReadId: {binding:?}");
    };
    assert!(
        additional_projection.is_empty(),
        "the canonical read owns its intrinsic projection"
    );
    let exact = &ir.scope_index.reads[read.as_usize()];
    assert_eq!(exact.id, *read);
    assert!(matches!(
        exact.target,
        ErasedReadTarget::Binding { .. } | ErasedReadTarget::Expression { .. }
    ));
}

#[test]
fn element_events_resolve_source_leaves_through_function_arguments() {
    let source = r#"
store: [submit: SOURCE]

FUNCTION submit_button(events) {
    Scene/Element/button(
        element: [events: events]
        style: [width: 80, height: 40]
        label: TEXT { Submit }
    )
}

scene: Scene/new(
    root: submit_button(events: [press: store.submit])
)
"#;
    let parsed = boon_parser::parse_source("constructor-source-argument.bn", source).unwrap();
    let ir = lower(&parsed).expect("constructor event arguments must resolve source leaves");

    assert!(ir.view_bindings.iter().any(|binding| {
        binding.node_kind == "Button"
            && binding.attr == "press"
            && binding.path == "store.submit"
            && binding.kind == ViewBindingKind::Source
            && binding.target
                == ViewBindingTarget::Source {
                    source: SourceId(0),
                }
    }));
}

#[test]
fn render_list_map_alias_uses_derived_list_storage_row_scope() {
    let source = r#"
store: [
    rows:
        LIST {
            [title: TEXT { First }, visible: True]
            [title: TEXT { Hidden }, visible: False]
        }
        |> List/map(item, new: new_row(title: item.title, visible: item.visible))
    visible_rows:
        rows
        |> List/retain(item, if: item.visible)
]

FUNCTION new_row(title, visible) {
    BLOCK {
        row_title: title
        row_visible: visible
        [
            controls: [press: SOURCE]
            title: row_title
            visible: row_visible
        ]
    }
}

FUNCTION render_row(item) {
    Element/button(
        element: [events: [press: item.controls.press]]
        style: []
        label: item.title
    )
}

FUNCTION render_rows(list, old: OUT, new) {
    list |> List/map(item: old, new: new)
}

document: Document/new(
    root: Element/stripe(
        element: []
        direction: Column
        style: []
        items: store.visible_rows
            |> render_rows(old, new: render_row(item: old))
    )
)
"#;
    let parsed = boon_parser::parse_source("render-row-alias-retain.bn", source).unwrap();
    let ir = lower(&parsed).expect("render aliases must preserve the storage row scope");

    let rows = ir
        .lists
        .iter()
        .find(|list| list.name == "store.rows")
        .expect("rows storage");
    let row_scope = &ir.row_scopes[rows.row_scope_id.expect("rows scope").as_usize()];
    let visible_rows = ir
        .lists
        .iter()
        .find(|list| list.name == "store.visible_rows")
        .expect("visible rows storage");
    let visible_scope = &ir.row_scopes[visible_rows
        .row_scope_id
        .expect("visible rows scope")
        .as_usize()];
    let source = ir
        .sources
        .iter()
        .find(|source| source.path == "store.rows.controls.press")
        .expect("row press source");

    assert_eq!(source.scope_id, Some(row_scope.id));
    assert!(ir.view_bindings.iter().any(|binding| {
        binding.node_kind == "Button"
            && binding.attr == "press"
            && binding.path == "store.rows.controls.press"
            && binding.kind == ViewBindingKind::Source
            && binding.scope_id == Some(row_scope.id)
            && binding.target == ViewBindingTarget::Source { source: source.id }
    }));
    assert!(ir.view_bindings.iter().any(|binding| {
        binding.node_kind == "Button"
            && binding.attr == "label"
            && binding.path == "store.visible_rows.title"
            && binding.kind == ViewBindingKind::Data
            && binding.scope_id == Some(visible_scope.id)
            && matches!(binding.target, ViewBindingTarget::Read { .. })
    }));
    assert!(
        ir.view_bindings
            .iter()
            .all(|binding| !binding.path.starts_with("old."))
    );
}

#[test]
fn element_events_without_concrete_source_leaves_fail_lowering() {
    let source = r#"
store: [enabled: True]

scene: Scene/new(
    root: Scene/Element/button(
        element: [events: [press: store.enabled]]
        style: [width: 80, height: 40]
        label: TEXT { Submit }
    )
    )
"#;
    let parsed = boon_parser::parse_source("constructor-without-source-leaves.bn", source).unwrap();
    let errors = [
        lower(&parsed).expect_err("non-source event leaves must fail lowering"),
        lower_runtime(&parsed)
            .expect_err("non-source event leaves must fail runtime-profile lowering"),
    ];

    for error in errors {
        assert!(
            error.contains("Element constructor `Scene/Element/button`")
                && error.contains("`element.events`")
                && error.contains("no concrete SOURCE leaves"),
            "unexpected source-binding error: {error}"
        );
    }
}

#[test]
fn inline_match_over_event_derived_value_lowers_to_static_update() {
    let source = r#"
store: [
    elements: [open: SOURCE, editor: SOURCE]
    requested:
        elements.open.event.press |> THEN { selected }
    selected:
        TEXT { none } |> HOLD selected {
            elements.editor.text
        }
    dialog:
        Open |> HOLD dialog {
            requested |> WHEN { TEXT { none } => Open, __ => Closed }
        }
]
"#;
    let parsed = boon_parser::parse_source("inline-event-derived-match.bn", source).unwrap();
    let ir = lower(&parsed).expect("inline event-derived matches must have a static schedule");

    let requested = ir
        .derived_values
        .iter()
        .find(|value| value.path == "store.requested")
        .expect("request transform");
    assert_eq!(
        requested.sources,
        vec!["store.elements.open"],
        "state sampled by THEN must not become an event cause"
    );
    let dialog = ir
        .possible_causes
        .iter()
        .find(|value| value.target == "store.dialog")
        .expect("dialog causes");
    assert_eq!(
        dialog.sources,
        vec!["store.elements.open"],
        "transitive event transforms must preserve the original trigger only"
    );

    let dialog_state = ir
        .state_cells
        .iter()
        .find(|state| state.path == "store.dialog")
        .expect("dialog state");
    let open_source = ir
        .sources
        .iter()
        .find(|source| source.path == "store.elements.open")
        .expect("open source");
    let arm = ir
        .state_update_arms
        .iter()
        .find(|arm| arm.state == dialog_state.id && arm.cause == EventCause::Source(open_source.id))
        .expect("exact dialog update arm");
    let output = ir
        .executable
        .expressions
        .get(arm.output_expression_id.as_usize())
        .expect("dialog update expression");
    let ExecutableExpressionKind::When { arms, .. } = &output.kind else {
        panic!("dialog update is not an executable match: {output:#?}");
    };
    let closed = arms
        .iter()
        .find(|candidate| {
            matches!(
                candidate.pattern,
                boon_typecheck::CheckedMatchPattern::Wildcard
            )
        })
        .and_then(|candidate| ir.executable.expressions.get(candidate.output.as_usize()))
        .expect("fallback match output");
    assert_eq!(
        closed.kind,
        ExecutableExpressionKind::Tag("Closed".to_owned()),
        "exact executable state arm must retain the fallback match output"
    );
}

#[test]
fn match_arm_payload_dependencies_do_not_become_event_causes() {
    let source = r#"
store: [
    elements: [ready: SOURCE, fire: SOURCE, payload: SOURCE]
    payload_value:
        TEXT { initial } |> HOLD payload_value {
            elements.payload.text
        }
    fingerprint:
        TEXT { request }
        |> Text/concat(with: payload_value, separator: ":")
    request:
        LATEST {
            elements.ready.event.press |> WHEN {
                True => fingerprint
                False => SKIP
            }
            elements.fire.event.press |> THEN { fingerprint }
        }
]
"#;
    let parsed = boon_parser::parse_source("match-arm-sampled-payload.bn", source).unwrap();
    let ir = lower(&parsed).expect("match-arm payload sampling must lower");

    let request = ir
        .derived_values
        .iter()
        .find(|value| value.path == "store.request")
        .expect("request transform");
    assert_eq!(
        request.sources,
        vec!["store.elements.fire", "store.elements.ready"],
        "sources sampled from WHEN/THEN outputs must not become event causes"
    );
}

#[test]
fn derived_when_input_remains_an_event_cause_beside_sampled_then_outputs() {
    let source = r#"
store: [
    start: SOURCE
    reset: SOURCE
    seed_rows: LIST { [key: TEXT { row }] }
    rows:
        seed_rows |> List/map(item, new: selectable_row(seed_row: item))
    clock_result:
        ClockNotRequested |> HOLD clock_result {
            start |> THEN { Clock/wall() }
        }
    projected:
        clock_result |> WHEN {
            WallClockRead => TEXT { canonical }
            __ => TEXT { none }
        }
    active:
        TEXT { fallback } |> HOLD active {
            LATEST {
                projected |> WHEN {
                    TEXT { none } => SKIP
                    __ => projected
                }
                reset |> THEN { TEXT { fallback } }
                rows
                    |> List/map(item, new:
                        item.select |> THEN { item.key }
                    )
                    |> List/latest()
            }
        }
]

FUNCTION selectable_row(seed_row) {
    [key: seed_row.key, select: SOURCE]
}
"#;
    let parsed = boon_parser::parse_source("derived-when-event-cause.bn", source).unwrap();
    let ir = lower(&parsed).expect("derived WHEN input must lower as an event cause");

    exact_state_arm(
        &ir,
        "store.active",
        exact_state_cause(&ir, "store.clock_result"),
    );
}

#[test]
fn contextual_sources_reused_by_two_lists_keep_distinct_owned_paths() {
    let source = r#"
store: [
    left_seed: LIST { [key: TEXT { left }] }
    right_seed: LIST { [key: TEXT { right }] }
    left_rows:
        left_seed |> List/map(item, new: selectable_row(seed: item))
    right_rows:
        right_seed |> List/map(item, new: selectable_row(seed: item))
    left_selected:
        left_rows
        |> List/map(item, new: item.select |> THEN { item.key })
        |> List/latest()
    right_selected:
        right_rows
        |> List/map(item, new: item.select |> THEN { item.key })
        |> List/latest()
]

FUNCTION selectable_row(seed) {
    [key: seed.key, select: SOURCE]
}
"#;
    let parsed = boon_parser::parse_source("two-owned-contextual-sources.bn", source).unwrap();
    let ir = lower(&parsed).expect("each materialized list must own its contextual source");

    let paths = ir
        .sources
        .iter()
        .map(|source| source.path.as_str())
        .collect::<BTreeSet<_>>();
    assert!(paths.contains("store.left_rows.select"), "{paths:?}");
    assert!(paths.contains("store.right_rows.select"), "{paths:?}");

    let left = ir
        .derived_values
        .iter()
        .find(|value| value.path == "store.left_selected")
        .expect("left selected value");
    let right = ir
        .derived_values
        .iter()
        .find(|value| value.path == "store.right_selected")
        .expect("right selected value");
    assert_eq!(left.sources, vec!["store.left_rows.select"]);
    assert_eq!(right.sources, vec!["store.right_rows.select"]);

    let owned_sources = ir
        .scope_index
        .locals
        .iter()
        .flat_map(|local| local.members.iter())
        .filter_map(|member| match member.target {
            ErasedLocalMemberTarget::Source(source) if member.path == ["select"] => Some(source),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(owned_sources.len(), 2, "{owned_sources:?}");
    assert_eq!(
        owned_sources
            .into_iter()
            .map(|source| ir.sources[source.as_usize()].path.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["store.left_rows.select", "store.right_rows.select"])
    );
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum TestRuntimeResourceIdentity {
    Source(SourceId),
    State(StateId),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TestOwnedResourceIdentity {
    static_owner: Option<StaticOwnerId>,
    owner_ancestry: Vec<StaticOwnerId>,
    runtime: TestRuntimeResourceIdentity,
}

fn test_owner_forest(program: &ErasedProgram) -> Vec<(StaticOwnerId, Option<StaticOwnerId>, u32)> {
    program
        .scope_index
        .owners
        .iter()
        .map(|owner| (owner.id, owner.parent, owner.child_ordinal))
        .collect()
}

fn test_owned_resource_identities(program: &ErasedProgram) -> Vec<TestOwnedResourceIdentity> {
    let mut resources = program
        .scope_index
        .bindings
        .iter()
        .filter_map(|binding| {
            let runtime = match binding.target {
                ErasedBindingTarget::Source { runtime, .. } => {
                    TestRuntimeResourceIdentity::Source(runtime)
                }
                ErasedBindingTarget::State { runtime, .. } => {
                    TestRuntimeResourceIdentity::State(runtime)
                }
                ErasedBindingTarget::Value { .. } => return None,
            };
            Some(TestOwnedResourceIdentity {
                static_owner: binding.static_owner,
                owner_ancestry: binding.owner_ancestry.clone(),
                runtime,
            })
        })
        .collect::<Vec<_>>();
    resources.sort();
    resources
}

fn lower_two_root_local_resource_calls(
    file_name: &str,
    wrappers: &str,
    constructor: &str,
) -> ErasedProgram {
    let source = format!(
        r#"
FUNCTION local_resource(initial) {{
    [
        change: SOURCE
        current:
            initial |> HOLD current {{
                change |> THEN {{ initial }}
            }}
    ]
}}

{wrappers}

left: {constructor}(initial: TEXT {{ left }})
right: {constructor}(initial: TEXT {{ right }})
"#
    );
    let parsed = boon_parser::parse_source(file_name, &source).unwrap();
    lower(&parsed).expect("root-local resources must lower with per-call ownership")
}

#[test]
fn two_root_calls_to_one_local_resource_function_do_not_alias() {
    let program = lower_two_root_local_resource_calls(
        "two-root-local-resource-calls.bn",
        "",
        "local_resource",
    );

    assert_eq!(
        test_owner_forest(&program),
        vec![(StaticOwnerId(0), None, 0), (StaticOwnerId(1), None, 1),]
    );
    assert_eq!(
        test_owned_resource_identities(&program),
        vec![
            TestOwnedResourceIdentity {
                static_owner: Some(StaticOwnerId(0)),
                owner_ancestry: vec![StaticOwnerId(0)],
                runtime: TestRuntimeResourceIdentity::Source(SourceId(0)),
            },
            TestOwnedResourceIdentity {
                static_owner: Some(StaticOwnerId(0)),
                owner_ancestry: vec![StaticOwnerId(0)],
                runtime: TestRuntimeResourceIdentity::State(StateId(0)),
            },
            TestOwnedResourceIdentity {
                static_owner: Some(StaticOwnerId(1)),
                owner_ancestry: vec![StaticOwnerId(1)],
                runtime: TestRuntimeResourceIdentity::Source(SourceId(1)),
            },
            TestOwnedResourceIdentity {
                static_owner: Some(StaticOwnerId(1)),
                owner_ancestry: vec![StaticOwnerId(1)],
                runtime: TestRuntimeResourceIdentity::State(StateId(1)),
            },
        ]
    );
}

#[test]
fn transparent_wrappers_preserve_root_resource_owner_forest_and_identities() {
    let direct = lower_two_root_local_resource_calls(
        "direct-root-local-resource-calls.bn",
        "",
        "local_resource",
    );
    let one_wrapper = lower_two_root_local_resource_calls(
        "one-wrapper-root-local-resource-calls.bn",
        r#"
FUNCTION wrapped_resource(initial) {
    local_resource(initial: initial)
}
"#,
        "wrapped_resource",
    );
    let two_wrappers = lower_two_root_local_resource_calls(
        "two-wrappers-root-local-resource-calls.bn",
        r#"
FUNCTION wrapped_resource(initial) {
    local_resource(initial: initial)
}

FUNCTION twice_wrapped_resource(initial) {
    wrapped_resource(initial: initial)
}
"#,
        "twice_wrapped_resource",
    );

    let expected_owners = test_owner_forest(&direct);
    let expected_resources = test_owned_resource_identities(&direct);
    for (label, wrapped) in [("one wrapper", one_wrapper), ("two wrappers", two_wrappers)] {
        assert_eq!(
            test_owner_forest(&wrapped),
            expected_owners,
            "{label} changed the static owner forest"
        );
        assert_eq!(
            test_owned_resource_identities(&wrapped),
            expected_resources,
            "{label} changed the per-owner runtime resource identities"
        );
    }
}

#[test]
fn distributed_producer_occurrences_become_distinct_erased_graph_instances() {
    let parsed = boon_parser::parse_source(
        "producer-instances.bn",
        r#"
FUNCTION combine(left, right) {
    left + right
}

seed: 0
"#,
    )
    .unwrap();
    let checked = boon_typecheck::check_runtime_program_profiled_with_external_types(
        &parsed,
        &boon_typecheck::ExternalTypeEnvironment::default(),
    )
    .0
    .program
    .expect("valid producer fixture has one authoritative checked program");
    let checked_before_overlay = checked.clone();
    let requests = vec![
        ProducerFunctionLoweringRequest {
            identity: [2; 32],
            local_function: "combine".to_owned(),
            mode: ProducerFunctionMode::Current,
        },
        ProducerFunctionLoweringRequest {
            identity: [1; 32],
            local_function: "combine".to_owned(),
            mode: ProducerFunctionMode::Current,
        },
        ProducerFunctionLoweringRequest {
            identity: [1; 32],
            local_function: "combine".to_owned(),
            mode: ProducerFunctionMode::Current,
        },
    ];
    let program = lower_checked(checked.clone(), &requests).unwrap();
    assert_eq!(
        checked, checked_before_overlay,
        "producer elaboration mutated the authoritative checked graph"
    );
    let reordered = requests.iter().cloned().rev().collect::<Vec<_>>();
    let reordered_program = lower_checked(checked.clone(), &reordered).unwrap();
    assert_eq!(
        program, reordered_program,
        "producer overlay order changed erased IDs or graph identity"
    );
    let [first, second] = program.producer_function_instances.as_slice() else {
        panic!(
            "expected two concrete producer instances, got {:#?}",
            program.producer_function_instances
        );
    };

    assert_eq!(first.identity, [1; 32]);
    assert_eq!(second.identity, [2; 32]);
    assert_ne!(first.owner, second.owner);
    assert_ne!(first.result_field, second.result_field);
    assert_ne!(first.result_path, second.result_path);
    assert_ne!(first.function, second.function);
    assert_ne!(first.root, second.root);
    assert_eq!(
        first
            .arguments
            .iter()
            .map(|argument| argument.name.as_str())
            .collect::<Vec<_>>(),
        vec!["left", "right"]
    );
    let first_parameters = first
        .arguments
        .iter()
        .map(|argument| argument.parameter)
        .collect::<BTreeSet<_>>();
    let second_parameters = second
        .arguments
        .iter()
        .map(|argument| argument.parameter)
        .collect::<BTreeSet<_>>();
    assert!(first_parameters.is_disjoint(&second_parameters));
    for instance in [first, second] {
        assert!(
            program
                .scope_index
                .owners
                .iter()
                .any(|owner| { owner.id == instance.owner && owner.parent.is_none() })
        );
        assert!(program.scope_index.fields.iter().any(|field| {
            field.id == instance.result_field
                && field.static_owner == Some(instance.owner)
                && field.diagnostic_path == instance.result_path
                && field.producer == Some(instance.root)
        }));
        assert!(program.derived_values.iter().any(|value| {
            value.id == instance.result_field && value.path == instance.result_path
        }));
        for argument in &instance.arguments {
            assert!(!argument.input_expressions.is_empty());
            for expression in &argument.input_expressions {
                assert!(program.executable.expressions.iter().any(|candidate| {
                    candidate.id == *expression
                        && candidate.owner == Some(instance.owner)
                        && matches!(
                            candidate.kind,
                            ExecutableExpressionKind::FunctionParameter {
                                parameter,
                                ..
                            } if parameter == argument.parameter
                        )
                }));
                assert!(program.scope_index.reads.iter().any(|read| {
                    read.expression == *expression
                        && matches!(
                            read.target,
                            ErasedReadTarget::FunctionParameter { parameter, .. }
                                if parameter == argument.parameter
                        )
                }));
            }
        }
    }
    assert!(program.static_schedule_verified);
    assert!(program.hidden_identity_verified);
}

#[test]
fn hold_backed_producer_is_resource_bound_before_final_ir_verification() {
    let parsed = boon_parser::parse_source(
        "hold-backed-producer.bn",
        r#"
FUNCTION local_resource(initial) {
    [
        change: SOURCE
        current:
            initial |> HOLD current {
                change |> THEN { initial }
            }
    ]
}

seed: 0
"#,
    )
    .unwrap();
    let program = lower_runtime_with_external_types_and_producer_functions(
        &parsed,
        &boon_typecheck::ExternalTypeEnvironment::default(),
        &[ProducerFunctionLoweringRequest {
            identity: [3; 32],
            local_function: "local_resource".to_owned(),
            mode: ProducerFunctionMode::Current,
        }],
    )
    .unwrap();
    let [instance] = program.producer_function_instances.as_slice() else {
        panic!(
            "expected one concrete producer instance, got {:#?}",
            program.producer_function_instances
        );
    };

    assert!(
        program
            .executable
            .sources
            .iter()
            .any(|source| { source.owner == Some(instance.owner) })
    );
    assert!(
        program
            .executable
            .states
            .iter()
            .any(|state| { state.owner == Some(instance.owner) })
    );
    let bound_resources = program
        .scope_index
        .bindings
        .iter()
        .filter(|binding| {
            binding.static_owner == Some(instance.owner)
                && binding.owner_ancestry.contains(&instance.owner)
                && matches!(
                    binding.target,
                    ErasedBindingTarget::Source { .. } | ErasedBindingTarget::State { .. }
                )
        })
        .collect::<Vec<_>>();
    assert_eq!(bound_resources.len(), 2, "{bound_resources:#?}");
    assert!(
        program.derived_values.iter().any(|value| {
            value.id == instance.result_field && value.path == instance.result_path
        })
    );
    assert!(program.static_schedule_verified);
    assert!(program.hidden_identity_verified);
}

#[test]
fn runtime_resource_aliases_follow_owner_ancestry_without_sibling_guessing() {
    let mut aliases = RuntimeResourceAliases::default();
    aliases
        .bind_owner_parents(&[
            StaticOwnerDef {
                id: StaticOwnerId(0),
                parent: None,
                child_ordinal: 0,
            },
            StaticOwnerDef {
                id: StaticOwnerId(1),
                parent: Some(StaticOwnerId(0)),
                child_ordinal: 0,
            },
            StaticOwnerDef {
                id: StaticOwnerId(2),
                parent: None,
                child_ordinal: 1,
            },
        ])
        .unwrap();
    insert_resource_alias(
        &mut aliases,
        Some(StaticOwnerId(0)),
        "item.select",
        RuntimeResourceAliasTarget::State(StateId(0)),
    )
    .unwrap();
    insert_resource_alias(
        &mut aliases,
        Some(StaticOwnerId(2)),
        "item.select",
        RuntimeResourceAliasTarget::State(StateId(1)),
    )
    .unwrap();
    insert_resource_alias(
        &mut aliases,
        None,
        "store.root",
        RuntimeResourceAliasTarget::State(StateId(2)),
    )
    .unwrap();
    let state_paths = vec![
        "store.left.select".to_owned(),
        "store.right.select".to_owned(),
        "store.root".to_owned(),
    ];

    assert_eq!(
        canonical_resource_path(
            "item.select.event.press",
            Some(StaticOwnerId(1)),
            &aliases,
            &[],
            &state_paths,
        )
        .unwrap(),
        "store.left.select.event.press"
    );
    assert_eq!(
        canonical_resource_path(
            "item.select",
            Some(StaticOwnerId(2)),
            &aliases,
            &[],
            &state_paths,
        )
        .unwrap(),
        "store.right.select"
    );
    assert_eq!(
        canonical_resource_path("item.select", None, &aliases, &[], &state_paths).unwrap(),
        "item.select",
        "ownerless metadata must not guess between contextual owners"
    );
    assert_eq!(
        canonical_resource_path(
            "store.root",
            Some(StaticOwnerId(1)),
            &aliases,
            &[],
            &state_paths,
        )
        .unwrap(),
        "store.root"
    );

    let error = insert_resource_alias(
        &mut aliases,
        Some(StaticOwnerId(0)),
        "item.select",
        RuntimeResourceAliasTarget::State(StateId(1)),
    )
    .unwrap_err();
    assert!(error.contains("resolves to both"), "{error}");
}

#[test]
fn contextual_when_wrapper_keeps_indexed_hold_field_ownership() {
    let source = r#"
store: [
    change: SOURCE
    seed:
        LIST {
            [kind: Stateful, formatter: TEXT { hex }]
            [kind: Plain, formatter: TEXT { text }]
        }
    rows:
        seed |> List/map(item, new: maybe_stateful(row: item))
]

FUNCTION maybe_stateful(row) {
    row.kind |> WHEN {
        Stateful => stateful(row: row)
        __ => row
    }
}

FUNCTION stateful(row) {
    [
        kind: row.kind
        formatter:
            row.formatter |> HOLD formatter {
                store.change |> THEN { TEXT { changed } }
            }
    ]
}
"#;
    let parsed = boon_parser::parse_source("contextual-when-indexed-hold.bn", source).unwrap();
    let ir = lower(&parsed).expect("the contextual HOLD must retain its exact keyed row field");
    let state = ir
        .state_cells
        .iter()
        .find(|state| state.semantic_path.as_deref() == Some("store.rows.formatter"))
        .expect("indexed formatter state");
    let binding = ir
        .scope_index
        .bindings
        .iter()
        .find(|binding| {
            matches!(
                binding.target,
                ErasedBindingTarget::State { runtime, .. } if runtime == state.id
            )
        })
        .expect("indexed formatter binding");
    assert!(matches!(
        binding.target,
        ErasedBindingTarget::State {
            field: Some(_),
            row: Some(_),
            ..
        }
    ));
}

#[test]
fn nested_match_over_grouped_key_event_uses_the_canonical_source_path() {
    let source = r#"
store: [
    elements: [search: [events: [key_down: SOURCE]]]
    highlighted:
        First |> HOLD highlighted {
            elements.search.events.key_down.key |> WHEN {
                ArrowDown => highlighted |> WHEN {
                    First => Second
                    Second => First
                }
                __ => SKIP
            }
        }
]
"#;
    let parsed = boon_parser::parse_source("grouped-key-event-nested-match.bn", source).unwrap();
    let ir = lower(&parsed).expect("grouped key events must lower through their SOURCE owner");

    let arm = exact_state_arm(
        &ir,
        "store.highlighted",
        exact_source_cause(&ir, "store.elements.search.events.key_down"),
    );
    assert!(
        exact_subtree(&ir, arm.output_expression_id)
            .into_iter()
            .filter(|expression| matches!(expression.kind, ExecutableExpressionKind::When { .. }))
            .count()
            >= 2,
        "nested key match was flattened"
    );
}

#[test]
fn nested_event_match_preserves_structured_effect_result_projection_reads() {
    let source = r#"
store: [
    start: SOURCE
    next: SOURCE
    clock_result:
        NotStarted |> HOLD clock_result {
            start |> THEN { Clock/wall() }
        }
    offset:
        0 |> HOLD offset {
            next.event.press |> THEN {
                clock_result |> WHEN {
                    WallClockRead => clock_result.nanoseconds == 0 |> WHEN {
                        True => clock_result.unix_seconds
                        False => clock_result.nanoseconds
                    }
                    __ => SKIP
                }
            }
        }
]
"#;
    let parsed = boon_parser::parse_source("structured-effect-projection-read.bn", source).unwrap();
    let ir = lower(&parsed).expect("structured effect projections must remain typed reads");
    let arm = exact_state_arm(&ir, "store.offset", exact_source_cause(&ir, "store.next"));
    let clock_state = ir
        .state_cells
        .iter()
        .find(|state| state.path == "store.clock_result")
        .expect("clock result state");
    let projections = exact_subtree(&ir, arm.output_expression_id)
        .into_iter()
        .filter_map(|expression| match &expression.kind {
            ExecutableExpressionKind::CanonicalRead { projection, .. } => {
                let read = ir
                    .scope_index
                    .reads
                    .iter()
                    .find(|read| read.expression == expression.id)?;
                let binding = match read.target {
                    ErasedReadTarget::Binding { binding, .. }
                    | ErasedReadTarget::StateProjection { binding, .. } => binding,
                    _ => return None,
                };
                matches!(
                    ir.scope_index.bindings[binding.as_usize()].target,
                    ErasedBindingTarget::State { runtime, .. } if runtime == clock_state.id
                )
                .then(|| projection.clone())
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    for expected in ["unix_seconds", "nanoseconds"] {
        assert!(
            projections.contains(&vec![expected.to_owned()]),
            "structured projection `{expected}` was lost: {projections:?}"
        );
    }
}

#[test]
fn inline_text_comparison_match_preserves_literal_operand() {
    let source = r#"
store: [
    elements: [remove: SOURCE]
    key:
        TEXT { clk } |> HOLD key {
            elements.remove.text
        }
    selected:
        True |> HOLD selected {
            elements.remove.event.press |> THEN {
                key == TEXT { clk } |> WHEN { True => False, False => selected }
            }
        }
]
"#;
    let parsed = boon_parser::parse_source("inline-text-comparison-match.bn", source).unwrap();
    let ir = lower(&parsed).expect("text comparison matches must have a static schedule");

    let arm = exact_state_arm(
        &ir,
        "store.selected",
        exact_source_cause(&ir, "store.elements.remove"),
    );
    let subtree = exact_subtree(&ir, arm.output_expression_id);
    assert!(subtree.iter().any(|expression| matches!(
        &expression.kind,
        ExecutableExpressionKind::Infix { op, .. } if op == "=="
    )));
    assert!(subtree.iter().any(|expression| matches!(
        &expression.kind,
        ExecutableExpressionKind::Text(value) if value == "clk"
    )));
    assert!(
        subtree
            .iter()
            .any(|expression| matches!(expression.kind, ExecutableExpressionKind::Bool(false)))
    );
}

#[test]
fn when_consumes_the_record_comparison_result() {
    let source = r#"
store: [
    start: SOURCE
    left: [value: 1]
    right: [value: 1]
    result:
        RandomNotRequested |> HOLD result {
            start |> THEN {
                left
                == right
                |> WHEN {
                    True => Random/bytes(byte_count: 1)
                    False => SKIP
                }
            }
        }
]
"#;
    let parsed = boon_parser::parse_source("record-comparison-when.bn", source).unwrap();
    let ir = lower(&parsed).expect("record comparison must lower");
    let when = ir
        .executable
        .expressions
        .iter()
        .find(|expression| matches!(expression.kind, ExecutableExpressionKind::When { .. }))
        .expect("executable WHEN expression");
    let ExecutableExpressionKind::When { input, .. } = when.kind else {
        unreachable!();
    };
    assert!(matches!(
        ir.executable.expressions[input.as_usize()].kind,
        ExecutableExpressionKind::Infix { ref op, .. } if op == "=="
    ));
}

#[test]
fn multiline_list_append_record_preserves_owned_fields() {
    let source = r#"
store: [
    elements: [create: SOURCE]
    group_to_create:
        elements.create.event.press |> THEN { TEXT { core } }
    groups:
        LIST {}
        |> List/append(item: group_to_create |> THEN {
            [
                name: group_to_create
                members: TEXT { A, B }
            ]
        })
        |> List/map(item, new: [name: item.name, members: item.members])
]
"#;
    let parsed = boon_parser::parse_source("multiline-list-append.bn", source).unwrap();
    let ir = lower(&parsed).expect("multiline append records must have a static schedule");
    let list = ir
        .lists
        .iter()
        .find(|list| list.name == "store.groups")
        .expect("groups list memory");
    let append = ir
        .list_mutations
        .iter()
        .find(|mutation| {
            mutation.list_id == list.id && matches!(mutation.kind, ListMutationKind::Append { .. })
        })
        .expect("groups append mutation");
    let ListMutationKind::Append { item, .. } = &append.kind else {
        unreachable!();
    };
    assert_eq!(
        event_cause_path_owned(append.cause, &ir.sources, &ir.state_cells).as_deref(),
        Ok("store.elements.create")
    );
    let authority_fields = ir
        .scope_index
        .fields
        .iter()
        .filter(|field| field.row.map(|row| row.list) == Some(list.id) && field.role.is_authority())
        .map(|field| field.name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(authority_fields, BTreeSet::from(["members", "name"]));
    let item = &ir.executable.expressions[item.as_usize()];
    let fields = match &item.kind {
        ExecutableExpressionKind::Object(fields) | ExecutableExpressionKind::Record(fields) => {
            fields
        }
        other => panic!("append item must be an exact record, got {other:?}"),
    };
    assert_eq!(
        fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["members", "name"])
    );
}

#[test]
fn multiline_list_literal_preserves_typed_initial_rows() {
    let parsed = boon_parser::parse_source(
        "multiline-list-literal.bn",
        r#"
store: [
    items: LIST {
        [
            id: TEXT { a }
            value: 7
        ]
        [
            id: TEXT { b }
            value: 9
        ]
    }
]
"#,
    )
    .unwrap();
    let ir = lower(&parsed).expect("multiline list literals must lower through the structured AST");
    let items = ir
        .lists
        .iter()
        .find(|list| list.name == "store.items")
        .expect("items list memory");
    let ListInitializer::RecordLiteral { rows } = &items.initializer else {
        panic!("expected typed record rows, got {:?}", items.initializer);
    };

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].fields[0].name, "id");
    assert_eq!(
        rows[0].fields[0].value,
        InitialValue::Text {
            value: "a".to_owned()
        }
    );
    assert_eq!(
        rows[1].fields[1].value,
        InitialValue::Number {
            value: "9".to_owned()
        }
    );
}

#[test]
fn list_literal_preserves_demand_current_root_field_initializers() {
    let parsed = boon_parser::parse_source(
        "dynamic-list-literal.bn",
        r#"
store: [
    change: SOURCE
    current_name:
        TEXT { initial } |> HOLD current_name {
            change.text
        }
    items: LIST {
        [id: TEXT { only }, name: current_name]
    }
]
"#,
    )
    .unwrap();
    let ir = lower(&parsed).expect("a dynamic row field must not erase its logical list row");
    let items = ir
        .lists
        .iter()
        .find(|list| list.name == "store.items")
        .expect("items list memory");
    let ListInitializer::RecordLiteral { rows } = &items.initializer else {
        panic!(
            "expected one logical record row, got {:?}",
            items.initializer
        );
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]
            .fields
            .iter()
            .find(|field| field.name == "name")
            .expect("dynamic name field")
            .value,
        InitialValue::RootInitialField {
            path: "store.current_name".to_owned(),
        }
    );
}

#[test]
fn pure_record_functions_initialize_recursive_list_rows() {
    let parsed = boon_parser::parse_source(
        "function-list-initializer.bn",
        r#"
store: [
    places: LIST {
        place(id: TEXT { alpha }, name: TEXT { Alpha }, x: 1.5, y: 2.5)
        place(id: TEXT { beta }, name: TEXT { Beta }, x: 3.5, y: 4.5)
    }
]

FUNCTION point(x, y) {
    BLOCK {
        point_x: x
        point_y: y
        [x: point_x, y: point_y]
    }
}

FUNCTION place(id, name, x, y) {
    BLOCK {
        place_id: id
        place_name: name
        [
            id: place_id
            name: place_name
            point: point(x: x, y: y)
        ]
    }
}
"#,
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    let places = ir
        .lists
        .iter()
        .find(|list| list.name == "store.places")
        .unwrap();
    let ListInitializer::RecordLiteral { rows } = &places.initializer else {
        panic!("function-built records must be authoritative rows");
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0]
            .fields
            .iter()
            .find(|field| field.name == "name")
            .unwrap()
            .value,
        InitialValue::Text {
            value: "Alpha".to_owned(),
        }
    );
    assert!(matches!(
        &rows[0]
            .fields
            .iter()
            .find(|field| field.name == "point")
            .unwrap()
            .value,
        InitialValue::Data {
            value: boon_data::Value::Record(fields),
        } if fields.len() == 2
    ));
}

#[test]
fn root_latest_lowers_to_semantic_memory_without_promoting_transient_latest() {
    let source = r#"
store: [
    pulse: SOURCE
    count:
        LATEST {
            0
            pulse |> THEN { count + 1 }
        }
    transient:
        pulse |> THEN { count + 10 }
    derived: count + 20
]
"#;
    let parsed = boon_parser::parse_source("root-latest-memory.bn", source).unwrap();
    let ir = lower(&parsed).unwrap();

    assert_eq!(
        ir.state_cells
            .iter()
            .map(|state| state.path.as_str())
            .collect::<Vec<_>>(),
        ["store.count"]
    );
    assert_eq!(
        ir.semantic_memory
            .iter()
            .map(|memory| (memory.identity.semantic_path.as_str(), memory.identity.kind,))
            .collect::<Vec<_>>(),
        [("store.count", SemanticMemoryKind::RootScalar)]
    );
    let transient = ir
        .derived_values
        .iter()
        .find(|value| value.path == "store.transient")
        .expect("transient event transform");
    assert_eq!(transient.kind, DerivedValueKind::SourceEventTransform);
    assert_eq!(transient.trigger_arms.len(), 1);
    let arm = &transient.trigger_arms[0];
    assert!(matches!(arm.cause, EventCause::Source(_)));
    let gate = &ir.executable.expressions[arm.gate_expression_id.as_usize()];
    assert_eq!(gate.id, arm.gate_expression_id);
    assert_eq!(gate.checked_expr_id, arm.gate_checked_expr_id);
    assert_eq!(gate.owner, arm.owner);
    assert!(
        ir.executable
            .expressions
            .get(arm.output_expression_id.as_usize())
            .is_some_and(|output| output.id == arm.output_expression_id)
    );
    assert!(transient.default_roots.is_empty());
    assert!(
        ir.derived_values
            .iter()
            .any(|value| value.path == "store.derived" && value.kind == DerivedValueKind::Pure)
    );
}

#[test]
fn authoritative_list_append_breaks_feedback_cycle_for_unique_candidate() {
    let source = r#"
store: [
    add: SOURCE
    candidate:
        add |> THEN {
            entries
            |> List/any(item, if:
                item.id == add.text
            )
            |> WHEN {
                True => SKIP
                False => [
                    id: add.text
                ]
            }
        }
    entries:
        LIST {}
        |> List/append(item: candidate)
        |> List/map(item, new: entry_view(entry: item))
]

FUNCTION entry_view(entry) {
    [
        id: entry.id
    ]
}
    "#;
    let parsed = boon_parser::parse_source("unique-append.bn", source).unwrap();
    let ir = lower(&parsed).expect("authoritative list ownership must break the feedback cycle");

    assert!(ir.lists.iter().any(|list| list.name == "store.entries"));
    let list = ir
        .lists
        .iter()
        .find(|list| list.name == "store.entries")
        .expect("entries list memory");
    let append = ir
        .list_mutations
        .iter()
        .find(|mutation| {
            mutation.list_id == list.id && matches!(mutation.kind, ListMutationKind::Append { .. })
        })
        .expect("entries append mutation");
    let ListMutationKind::Append { item, .. } = &append.kind else {
        unreachable!();
    };
    assert_eq!(
        event_cause_path_owned(append.cause, &ir.sources, &ir.state_cells).as_deref(),
        Ok("store.add")
    );
    let item = &ir.executable.expressions[item.as_usize()];
    assert!(
        matches!(&item.kind, ExecutableExpressionKind::When { .. }),
        "append must retain the exact conditional item expression: {item:#?}"
    );
    let exact_fields = exact_list_item_field_types(&ir.executable, item.id).unwrap();
    assert_eq!(
        exact_fields.get("id"),
        Some(&boon_typecheck::Type::Text),
        "exact append item fields: {exact_fields:#?}"
    );
    assert!(
        ir.scope_index.fields.iter().any(|field| {
            field.row.map(|row| row.list) == Some(list.id)
                && field.role.is_authority()
                && field.name == "id"
        }),
        "exact item branches: {:#?}",
        match &item.kind {
            ExecutableExpressionKind::When { arms, .. } => arms
                .iter()
                .map(|arm| &ir.executable.expressions[arm.output.as_usize()])
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        }
    );
    assert!(
        ir.derived_values
            .iter()
            .any(|value| value.path == "store.candidate")
    );
}

#[test]
fn effect_result_append_keeps_state_trigger_and_exact_record_schema() {
    let parsed = boon_parser::parse_source(
        "typed-passkey-effect-list-schema.bn",
        include_str!("../../../../testdata/typed_passkey_effects.bn"),
    )
    .unwrap();
    let ir = lower(&parsed).expect("effect-result append must lower through typed state authority");
    let list = ir
        .lists
        .iter()
        .find(|list| list.name == "store.credentials")
        .expect("credentials list memory");
    let append = ir
        .list_mutations
        .iter()
        .find(|mutation| mutation.list_id == list.id)
        .expect("credentials append mutation");
    assert_eq!(
        event_cause_path_owned(append.cause, &ir.sources, &ir.state_cells).as_deref(),
        Ok("store.registration_result")
    );
    let ListMutationKind::Append { item, .. } = &append.kind else {
        panic!("credentials mutation must append");
    };
    let exact_fields = exact_list_item_field_types(&ir.executable, *item).unwrap();
    assert_eq!(
        exact_fields,
        BTreeMap::from([
            ("credential_id".to_owned(), boon_typecheck::Type::Text),
            ("label".to_owned(), boon_typecheck::Type::Text),
        ])
    );
    assert!(ir.scope_index.fields.iter().any(|field| {
        field.row.map(|row| row.list) == Some(list.id)
            && field.role.is_authority()
            && field.name == "credential_id"
    }));
    assert!(ir.scope_index.fields.iter().any(|field| {
        field.row.map(|row| row.list) == Some(list.id)
            && field.role.is_authority()
            && field.name == "label"
    }));
    assert_eq!(
        ir.derived_values
            .iter()
            .filter(|value| value.path.starts_with("store.registration_candidate"))
            .map(|value| value.path.as_str())
            .collect::<Vec<_>>(),
        vec!["store.registration_candidate"],
        "branch-local record fields must remain owned by the guarded record computation"
    );
    assert!(ir.scope_index.fields.iter().all(|field| {
        field.row.is_some()
            || !matches!(
                field.diagnostic_path.as_str(),
                "store.registration_candidate.credential_id" | "store.registration_candidate.label"
            )
    }));
}

#[test]
fn mapped_row_state_ignores_sources_owned_by_sibling_fields_and_list_mutations() {
    let source = r#"
store: [
    controls: [
        append: SOURCE
        toggle_all: SOURCE
    ]
    rows:
        LIST {
            [completed: False]
            [completed: True]
        }
        |> List/append(item: controls.append |> THEN {
            [completed: False]
        })
        |> List/map(item, new: new_row(initial_completed: item.completed))
    all_completed: rows |> List/every(item, if: item.completed)
]

FUNCTION new_row(initial_completed) {
    [
        controls: [
            edit: SOURCE
            toggle: SOURCE
        ]
        completed:
            LATEST {
                initial_completed
                store.controls.toggle_all |> THEN {
                    store.all_completed |> Bool/not()
                }
            }
            |> Bool/toggle(when: controls.toggle)
    ]
}

    "#;
    let parsed = boon_parser::parse_source("mapped-row-source-ownership.bn", source).unwrap();
    let ir = lower(&parsed).expect("mapped row source ownership must lower");
    let completed_state = ir
        .state_cells
        .iter()
        .find(|state| state.semantic_path.as_deref() == Some("store.rows.completed"))
        .unwrap_or_else(|| {
            panic!(
                "canonical mapped-row completed state; available={:?}",
                ir.state_cells
                    .iter()
                    .map(|state| (&state.path, &state.semantic_path, state.scope_id))
                    .collect::<Vec<_>>()
            )
        });
    let sources = ir
        .state_update_arms
        .iter()
        .filter(|arm| arm.state == completed_state.id)
        .map(|arm| {
            event_cause_path_owned(arm.cause, &ir.sources, &ir.state_cells)
                .expect("mapped row cause path")
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(
        sources,
        BTreeSet::from([
            "store.controls.toggle_all".to_owned(),
            "store.rows.controls.toggle".to_owned(),
        ]),
        "row state must not inherit append or sibling-field event sources"
    );

    let completed_executable = ir.executable.states[completed_state
        .executable_state_id
        .expect("published state has executable identity")
        .as_usize()]
    .clone();
    let completed_states = ir
        .state_cells
        .iter()
        .filter(|state| {
            state.executable_state_id.is_some_and(|state_id| {
                let executable = &ir.executable.states[state_id.as_usize()];
                executable.declaration == completed_executable.declaration
                    && executable.owner == completed_executable.owner
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(completed_states.len(), 2);
    assert_eq!(
        completed_states
            .iter()
            .filter(|state| state.published)
            .count(),
        1
    );
    let internal_state = completed_states
        .iter()
        .find(|state| !state.published)
        .expect("LATEST has a distinct internal state");
    for state in &completed_states {
        assert_eq!(
            state.id.as_usize(),
            state
                .executable_state_id
                .expect("runtime state has executable identity")
                .as_usize(),
            "runtime and executable state IDs must be bijective"
        );
    }

    let source_id = |path: &str| {
        ir.sources
            .iter()
            .find(|source| source.path == path)
            .unwrap_or_else(|| panic!("missing source `{path}`"))
            .id
    };
    let toggle_all = EventCause::Source(source_id("store.controls.toggle_all"));
    let toggle_row = EventCause::Source(source_id("store.rows.controls.toggle"));
    let causes_for = |state: StateId| {
        ir.state_update_arms
            .iter()
            .filter(|arm| arm.state == state)
            .map(|arm| arm.cause)
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(causes_for(internal_state.id), BTreeSet::from([toggle_all]));
    assert_eq!(
        causes_for(completed_state.id),
        BTreeSet::from([toggle_all, toggle_row])
    );
    assert_eq!(
        ir.semantic_memory
            .iter()
            .filter(|memory| {
                memory.identity.kind == SemanticMemoryKind::IndexedField
                    && memory.identity.semantic_path == "store.rows.completed"
            })
            .count(),
        1,
        "only the published completed state has semantic persistence identity"
    );
}

#[test]
fn todomvc_completed_state_has_only_its_declared_event_causes() {
    let parsed = boon_parser::parse_source(
        "todomvc-completed-causes.bn",
        include_str!("../../../../examples/todomvc.bn"),
    )
    .unwrap();
    let ir = lower(&parsed).expect("TodoMVC event ownership must lower");
    let completed = ir
        .state_cells
        .iter()
        .find(|state| state.semantic_path.as_deref() == Some("store.todos.completed"))
        .expect("published TodoMVC completed state");
    let causes = ir
        .state_update_arms
        .iter()
        .filter(|arm| arm.state == completed.id)
        .map(|arm| {
            event_cause_path_owned(arm.cause, &ir.sources, &ir.state_cells)
                .expect("TodoMVC completed cause path")
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        causes,
        BTreeSet::from([
            "store.sources.toggle_all_checkbox.events.click".to_owned(),
            "store.todos.sources.todo_checkbox.events.click".to_owned(),
        ]),
        "TodoMVC completed must not inherit unrelated row, list, aggregate, or root causes"
    );
}

#[test]
fn inline_literal_row_holds_bind_to_sibling_sources() {
    let parsed = boon_parser::parse_source(
        "inline-literal-row-sibling-sources.bn",
        include_str!("../../../../examples/migrations/todo/v1.bn"),
    )
    .unwrap();
    let ir = lower(&parsed).expect("inline literal row sibling sources must lower");

    for (state_path, source_path) in [
        (
            "store.todos.title",
            "store.todos.sources.rename_button.events.press",
        ),
        (
            "store.todos.completed",
            "store.todos.sources.toggle_button.events.press",
        ),
    ] {
        let state = ir
            .state_cells
            .iter()
            .find(|state| state.path == state_path)
            .unwrap_or_else(|| panic!("missing state `{state_path}`"));
        let causes = ir
            .state_update_arms
            .iter()
            .filter(|arm| arm.state == state.id)
            .map(|arm| {
                event_cause_path_owned(arm.cause, &ir.sources, &ir.state_cells)
                    .expect("state update cause path")
            })
            .collect::<BTreeSet<_>>();
        let executable_state = state
            .executable_state_id
            .map(|id| &ir.executable.states[id.as_usize()]);
        let subtree = executable_state
            .map(|state| exact_subtree(&ir, state.expression))
            .unwrap_or_default();
        assert_eq!(
            causes,
            BTreeSet::from([source_path.to_owned()]),
            "`{state_path}` must use its exact sibling source; executable_sources={:#?}; executable_state={executable_state:#?}; subtree={subtree:#?}",
            ir.executable.sources
        );
    }
}

#[test]
fn runtime_lowering_binds_named_root_hold_to_exact_executable_state() {
    let parsed = boon_parser::parse_source(
        "runtime-root-hold-identity.bn",
        "value: TEXT { one } |> HOLD value { LATEST {} }\n",
    )
    .unwrap();
    let ir = lower_runtime(&parsed).expect("runtime root HOLD must lower");
    let state = ir
        .state_cells
        .iter()
        .find(|state| state.path == "value")
        .expect("root state cell");
    let executable = state
        .executable_state_id
        .unwrap_or_else(|| {
            panic!(
                "root state has no exact executable identity; executable states={:#?}; roots={:#?}; expressions={:#?}; statements={:#?}; state={state:#?}",
                ir.executable.states,
                ir.executable.roots,
                ir.executable.expressions,
                ir.executable.statements,
            )
        });
    let binding = ir
        .scope_index
        .bindings
        .iter()
        .find(|binding| {
            matches!(
                binding.target,
                ErasedBindingTarget::State {
                    executable: candidate,
                    ..
                } if candidate == executable
            )
        })
        .expect("state storage binding");
    assert!(
        ir.executable
            .states
            .iter()
            .any(|state| { state.id == executable && state.declaration == binding.declaration })
    );
}
