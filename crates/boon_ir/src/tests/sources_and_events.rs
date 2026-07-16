// Included by `../tests.rs`; kept in the parent test module for private IR helper access.

#[test]
fn source_payload_schema_row_lookup_field_uses_generic_name() {
    let schema = SourcePayloadSchema {
        fields: Vec::new(),
        typed_fields: Vec::new(),
        row_lookup_field: Some("file".to_owned()),
    };
    assert_eq!(schema.row_lookup_field_name(), Some("file"));

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
fn scoped_source_lookup_prefers_source_intent_identity_field() {
    assert_eq!(
        select_source_row_lookup_field(
            "file_tree_row.scope_row_elements.select_scope",
            vec!["file".to_owned(), "scope_key".to_owned()]
        )
        .as_deref(),
        Some("scope_key")
    );
    assert_eq!(
        select_source_row_lookup_field(
            "file_tree_row.file_row_elements.select_file",
            vec!["file".to_owned(), "scope_key".to_owned()]
        )
        .as_deref(),
        Some("file")
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
        ("cell.sources.editor.change", SourceId(0)),
        ("cell.sources.editor.commit", SourceId(1)),
    ];
    assert_eq!(
        canonical_view_source_path(
            &sources,
            "store.selected_input.sources.editor.change"
        )
        .map(|(path, source_id)| (path, source_id.as_usize())),
        Some(("cell.sources.editor.change", 0))
    );

    let ambiguous = [
        ("left.sources.editor.change", SourceId(0)),
        ("right.sources.editor.change", SourceId(1)),
    ];
    assert!(
        canonical_view_source_path(
            &ambiguous,
            "store.selected_input.sources.editor.change"
        )
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
fn source_payload_match_rejects_unsupported_nested_numeric_infix_operator() {
    let source = r#"
store: [
elements: [
    keyboard_capture: SOURCE
]
zoom_step:
    0 |> HOLD zoom_step {
        LATEST {
            elements.keyboard_capture.key |> WHEN {
                W => zoom_step * 2
                __ => SKIP
            }
        }
    }
]
"#;
    let parsed =
        boon_parser::parse_source("source-payload-unsupported-nested-op.bn", source).unwrap();
    let error =
        lower(&parsed).expect_err("unsupported nested numeric operator should fail lowering");
    assert!(
        error.contains("unsupported numeric operator `*`"),
        "unexpected static verification error: {error}"
    );
}


#[test]
fn projected_helper_field_access_does_not_create_persistent_helper_fields() {
    let source = r#"
SOURCE
HOLD
LATEST
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
    flavors |> List/map(flavor, new: projected_flavor(flavor: flavor))
]

FUNCTION projected_flavor(flavor) {
[
    flavor_id: flavor.id
    detail_label:
        rows
        |> List/map(row, new: detail_row(row: row, suffix: flavor.suffix).label)
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
fn event_press_pulse_is_not_payload_guard_field() {
    let variants = source_ref_variants("store.elements.select_clk");
    assert_eq!(
        source_payload_field_from_path("store.elements.select_clk.event.press", &variants),
        Some("press".to_owned())
    );
    assert_eq!(
        source_payload_guard_field_from_path("store.elements.select_clk.event.press", &variants),
        None
    );
    assert_eq!(
        source_payload_guard_field_from_path(
            "store.elements.select_clk.event.key_down.key",
            &variants
        ),
        Some("key".to_owned())
    );
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
                element: [event: [press: SOURCE]]
                style: [width: 80, height: 40]
                label: TEXT { Status }
            ) |> SOURCE { store.controls.admin.status }
            Scene/Element/button(
                element: [event: [press: SOURCE]]
                style: [width: 80, height: 40]
                label: TEXT { Events }
            ) |> SOURCE { store.controls.admin.events }
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
        BTreeSet::from([
            "store.controls.admin.events",
            "store.controls.admin.status",
        ])
    );
    for source_path in [
        "store.controls.admin.status",
        "store.controls.admin.events",
    ] {
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
        assert!(ir
            .update_branches
            .iter()
            .any(|branch| branch.source == source_path && branch.target == "store.page"));
        assert!(ir
            .view_bindings
            .iter()
            .any(|binding| binding.path == source_path && binding.source_id == Some(source.id)));
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

    assert!(ir.update_branches.iter().any(|branch| {
        branch.source == "store.elements.open"
            && branch.target == "store.dialog"
            && matches!(
                &branch.expression,
                UpdateExpression::MatchValueConst { input, arms }
                    if input == "store.requested"
                        && arms.iter().any(|arm| {
                            arm.pattern == "__"
                                && arm.output
                                    == UpdateValueExpression::Const {
                                        value: "Closed".to_owned(),
                                    }
                        })
            )
    }), "event-derived inline match was not lowered: {:#?}", ir.update_branches);
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

    assert!(ir.update_branches.iter().any(|branch| {
        branch.source == "store.elements.remove"
            && branch.target == "store.selected"
            && matches!(
                &branch.expression,
                UpdateExpression::MatchInfixConst {
                    left: UpdateValueExpression::ReadPath { path: left },
                    op,
                    right: UpdateValueExpression::Const { value: right },
                    arms,
                } if left == "store.key"
                    && op == "=="
                    && right == "clk"
                    && arms.iter().any(|arm| {
                        arm.pattern == "True"
                            && arm.output == UpdateValueExpression::Const {
                                value: "False".to_owned(),
                            }
                    })
            )
    }), "text comparison was not lowered as typed infix operands: {:#?}", ir.update_branches);
}

#[test]
fn multiline_list_append_record_preserves_owned_fields() {
    let source = r#"
store: [
    elements: [create: SOURCE]
    group_to_create:
        LATEST {
            elements.create.event.press |> THEN { TEXT { core } }
        }
    groups:
        LIST {}
        |> List/append(item: group_to_create |> THEN {
            [
                name: group_to_create
                members: TEXT { A, B }
            ]
        })
        |> List/map(group, new: [name: group.name, members: group.members])
]
"#;
    let parsed = boon_parser::parse_source("multiline-list-append.bn", source).unwrap();
    let ir = lower(&parsed).expect("multiline append records must have a static schedule");
    let append = ir
        .list_operations
        .iter()
        .find(|operation| {
            operation.list == "groups"
                && matches!(operation.kind, ListOperationKind::Append { .. })
        })
        .expect("groups append operation");
    let ListOperationKind::Append { trigger, fields } = &append.kind else {
        unreachable!();
    };
    assert_eq!(trigger, "store.group_to_create");
    assert_eq!(
        fields,
        &vec![
            ListAppendField {
                name: "name".to_owned(),
                value: ListAppendFieldValue::Source {
                    path: "store.group_to_create".to_owned(),
                },
            },
            ListAppendField {
                name: "members".to_owned(),
                value: ListAppendFieldValue::Const {
                    value: "A, B".to_owned(),
                },
            },
        ]
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
        .find(|list| list.name == "items")
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
    [x: x, y: y]
}

FUNCTION place(id, name, x, y) {
    [
        id: id
        name: name
        point: point(x: x, y: y)
    ]
}
"#,
    )
    .unwrap();
    let ir = lower(&parsed).unwrap();
    let places = ir.lists.iter().find(|list| list.name == "places").unwrap();
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
        LATEST {
            pulse |> THEN { count + 10 }
        }
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
            .map(|memory| (
                memory.identity.semantic_path.as_str(),
                memory.identity.kind,
            ))
            .collect::<Vec<_>>(),
        [("store.count", SemanticMemoryKind::RootScalar)]
    );
    assert!(ir.derived_values.iter().any(|value| {
        value.path == "store.transient" && value.kind == DerivedValueKind::SourceEventTransform
    }));
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
            |> List/any(entry, if:
                entry.id == add.text
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
        |> List/map(entry, new: entry_view(entry: entry))
]

FUNCTION entry_view(entry) {
[
    id: entry.id
]
}
"#;
    let parsed = boon_parser::parse_source("unique-append.bn", source).unwrap();
    let ir = lower(&parsed).expect("authoritative list ownership must break the feedback cycle");

    assert!(ir.lists.iter().any(|list| list.name == "entries"));
    let append = ir
        .list_operations
        .iter()
        .find(|operation| {
            operation.list == "entries"
                && matches!(operation.kind, ListOperationKind::Append { .. })
        })
        .expect("entries append operation");
    let ListOperationKind::Append { trigger, fields } = &append.kind else {
        unreachable!();
    };
    assert_eq!(trigger, "store.candidate");
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "id");
    assert!(matches!(
        &fields[0].value,
        ListAppendFieldValue::Source { path } if path == "add.text"
    ));
    assert!(ir
        .derived_values
        .iter()
        .any(|value| value.path == "store.candidate"));
}
