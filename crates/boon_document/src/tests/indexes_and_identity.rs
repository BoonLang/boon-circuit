// Included by `../tests.rs`; kept in the parent test module for private document helper access.

#[test]
fn derived_index_bundle_builds_typed_layout_and_hit_indexes_together() {
    let mut frame = DocumentFrame::empty("root");
    let mut button = node("button", DocumentNodeKind::Button, Some("root"));
    button.text = Some(TextValue {
        text: "Press".to_owned(),
    });
    button
        .style
        .insert("width".to_owned(), StyleValue::Text("auto".to_owned()));
    button
        .style
        .insert("padding".to_owned(), StyleValue::Number(4.0));
    button
        .style
        .insert("size".to_owned(), StyleValue::Number(12.0));
    button.set_primary_source_binding(boon_document_model::SourceBinding {
        id: SourceBindingId("source:button:press".to_owned()),
        source_path: "controls.primary.press".to_owned(),
        intent: "press".to_owned(),
    });
    frame
        .nodes
        .get_mut(&frame.root)
        .unwrap()
        .children
        .push(button.id.clone());
    frame.nodes.insert(button.id.clone(), button);

    let bundle = DocumentDerivedIndexBundle::from_frame(&frame).unwrap();
    let standalone_hot_ids = DocumentHotIdTable::from_frame(&frame).unwrap();
    let standalone_intern = DocumentInternIndex::from_frame(&frame, &standalone_hot_ids).unwrap();
    let standalone_styles =
        DocumentTypedStyleIndex::from_frame(&frame, &standalone_hot_ids).unwrap();
    let standalone_bindings =
        DocumentTypedBindingIndex::from_frame(&frame, &standalone_hot_ids, &standalone_intern)
            .unwrap();

    assert_eq!(bundle.hot_ids, standalone_hot_ids);
    assert_eq!(bundle.intern_index, standalone_intern);
    assert_eq!(bundle.typed_styles, standalone_styles);
    assert_eq!(bundle.typed_bindings, standalone_bindings);

    let viewport = Viewport {
        surface: 1,
        width: 240.0,
        height: 80.0,
        scale: 1.0,
    };
    let mut typed_text = SimpleTextMeasurer;
    let typed = bundle
        .try_layout(LayoutInput {
            document: &frame,
            viewport,
            text: &mut typed_text,
            capabilities: RenderCapabilities::fake_portable(),
        })
        .unwrap();

    let hit_table = bundle.try_hit_side_table(&frame, &typed).unwrap();
    let hit = hit_table
        .entry_for_source_path("controls.primary.press")
        .expect("typed bundle hit table should preserve source path lookup");
    let button_hot = bundle
        .hot_ids
        .hot_id(&DocumentNodeId("button".to_owned()))
        .unwrap();
    let retained_layout_cache = bundle.try_retained_layout_cache(&frame, &typed).unwrap();
    assert!(
        retained_layout_cache.entries.contains_key(&button_hot),
        "derived bundle should build retained layout geometry for hot document nodes"
    );
    assert_eq!(
        hit.source_binding_refs,
        vec![DocumentTypedBindingRef {
            node: button_hot,
            ordinal: 0,
        }]
    );
}


#[test]
fn style_identity_splits_layout_paint_material_font_and_pseudo_state() {
    let mut base = StyleMap::new();
    base.insert("width".to_owned(), StyleValue::Number(120.0));
    base.insert("color".to_owned(), StyleValue::Text("red".to_owned()));
    base.insert("material".to_owned(), StyleValue::Text("flat".to_owned()));
    base.insert(
        "font_weight".to_owned(),
        StyleValue::Text("bold".to_owned()),
    );
    base.insert("__hover_scope".to_owned(), StyleValue::Bool(true));

    let identity = computed_style_identity(&base);
    let same_identity = computed_style_identity(&base);
    assert_eq!(identity, same_identity);

    let mut paint_change = base.clone();
    paint_change.insert("color".to_owned(), StyleValue::Text("blue".to_owned()));
    let paint_identity = computed_style_identity(&paint_change);
    assert_ne!(identity.style_id, paint_identity.style_id);
    assert_eq!(identity.layout_id, paint_identity.layout_id);
    assert_ne!(identity.paint_id, paint_identity.paint_id);
    assert_eq!(identity.material_id, paint_identity.material_id);
    assert_eq!(identity.font_id, paint_identity.font_id);
    assert_eq!(identity.pseudo_state_id, paint_identity.pseudo_state_id);

    let mut layout_change = base.clone();
    layout_change.insert("width".to_owned(), StyleValue::Number(180.0));
    let layout_identity = computed_style_identity(&layout_change);
    assert_ne!(identity.layout_id, layout_identity.layout_id);
    assert_eq!(identity.paint_id, layout_identity.paint_id);
}


#[test]
fn typed_hit_side_table_carries_route_identity_and_bucket_index() {
    let mut frame = DocumentFrame::empty("root");
    let mut scroll = node("scroll", DocumentNodeKind::ScrollRoot, Some("root"));
    scroll
        .style
        .insert("height".to_owned(), StyleValue::Number(120.0));
    scroll.materialized.push(MaterializedRange {
        materialization: Some(1),
        axis: Axis::Vertical,
        visible: 0..4,
        overscan: 0..8,
        logical_item_count: 8,
    });
    let mut materialized_row = node(
        "materialized-row",
        DocumentNodeKind::Stack,
        Some("scroll"),
    );
    materialized_row.scroll = Some(boon_document_model::ScrollState { x: 0.0, y: 0.0 });
    let mut button = node(
        "row-button",
        DocumentNodeKind::Button,
        Some("materialized-row"),
    );
    button
        .style
        .insert("width".to_owned(), StyleValue::Number(80.0));
    button
        .style
        .insert("height".to_owned(), StyleValue::Number(24.0));
    button
        .style
        .insert("row_key".to_owned(), StyleValue::Number(42.0));
    button.style.insert(
        "row_generation".to_owned(),
        StyleValue::Text("7".to_owned()),
    );
    button.set_primary_source_binding(boon_document_model::SourceBinding {
        id: SourceBindingId("source:row-button:press".to_owned()),
        source_path: "rows.press".to_owned(),
        intent: "press".to_owned(),
    });
    frame
        .nodes
        .get_mut(&DocumentNodeId("root".to_owned()))
        .unwrap()
        .children
        .push(DocumentNodeId("scroll".to_owned()));
    scroll
        .children
        .push(DocumentNodeId("materialized-row".to_owned()));
    materialized_row
        .children
        .push(DocumentNodeId("row-button".to_owned()));
    frame
        .nodes
        .insert(DocumentNodeId("scroll".to_owned()), scroll);
    frame.nodes.insert(
        DocumentNodeId("materialized-row".to_owned()),
        materialized_row,
    );
    frame
        .nodes
        .insert(DocumentNodeId("row-button".to_owned()), button);
    frame.scroll_roots.insert(
        ScrollRootId("scroll".to_owned()),
        boon_document_model::ScrollState { x: 0.0, y: 0.0 },
    );

    let mut measurer = SimpleTextMeasurer;
    let layout = layout(LayoutInput {
        document: &frame,
        viewport: boon_host::Viewport {
            surface: 1,
            width: 320.0,
            height: 240.0,
            scale: 1.0,
        },
        text: &mut measurer,
        capabilities: RenderCapabilities::fake_portable(),
    });
    let hot_ids = DocumentHotIdTable::from_frame(&frame).unwrap();
    let intern_index = DocumentInternIndex::from_frame(&frame, &hot_ids).unwrap();
    let typed_bindings =
        DocumentTypedBindingIndex::from_frame(&frame, &hot_ids, &intern_index).unwrap();
    let table = HitSideTable::try_from_document_layout_with_typed_bindings(
        &frame,
        &hot_ids,
        &typed_bindings,
        &layout,
        64.0,
    )
    .unwrap();

    let entry = table
        .entry_for_source_path("rows.press")
        .expect("source path should have a typed hit entry");
    let row_button_hot = hot_ids
        .hot_id(&DocumentNodeId("row-button".to_owned()))
        .unwrap();
    assert_eq!(entry.node, DocumentNodeId("row-button".to_owned()));
    assert_eq!(
        entry.source_binding_id,
        Some(SourceBindingId("source:row-button:press".to_owned()))
    );
    assert_eq!(entry.source_intent.as_deref(), Some("press"));
    assert_eq!(
        entry.source_binding_refs,
        vec![DocumentTypedBindingRef {
            node: row_button_hot,
            ordinal: 0,
        }]
    );
    assert_eq!(entry.scroll_root, Some(ScrollRootId("scroll".to_owned())));
    assert_eq!(entry.row_key, Some(42));
    assert_eq!(entry.row_generation, Some(7));
    assert_eq!(entry.z_depth, 0);
    assert!(
        table
            .bucket_indices(entry.spatial_bucket)
            .is_some_and(|bucket| !bucket.is_empty())
    );
    let hit = table
        .hit_test(entry.bounds.x + 1.0, entry.bounds.y + 1.0)
        .expect("typed hit side table should route by point");
    assert_eq!(hit.source_path.as_deref(), Some("rows.press"));
    assert_eq!(hit.source_binding_refs, entry.source_binding_refs);
}
