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
        source_payload_value_type(&SourcePayloadField::Named("press".to_owned())),
        SourcePayloadValueType::Bool
    );
    assert_eq!(
        source_payload_value_type(&SourcePayloadField::Named("pointer_x".to_owned())),
        SourcePayloadValueType::Text
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
