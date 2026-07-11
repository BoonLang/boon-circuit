// Included by `../tests.rs`; kept in the parent test module for private document helper access.

#[test]
fn materialization_layout_demands_visible_overscan_and_stable_keys() {
    let frame = fixture_frame_with_virtualized_table();
    let mut measurer = SimpleTextMeasurer;
    let layout = layout(LayoutInput {
        document: &frame,
        viewport: Viewport {
            surface: 1,
            width: 640.0,
            height: 480.0,
            scale: 1.0,
        },
        text: &mut measurer,
        capabilities: RenderCapabilities::fake_portable(),
    });

    assert_eq!(layout.demands.len(), 2);
    assert_eq!(layout.materialization.len(), 2);
    let vertical = layout
        .demands
        .iter()
        .find(|demand| demand.axis == Axis::Vertical)
        .expect("vertical materialization demand should exist");
    assert_eq!(vertical.visible, 0..20);
    assert_eq!(vertical.overscan, 0..28);
    assert_eq!(vertical.materialization, Some(1));
    assert_eq!(vertical.logical_item_count, 2_600);
    assert_eq!(vertical.materialized_item_count, 28);
    assert_eq!(vertical.stable_key_prefix, "materialized:virtual-table:y");
    assert_eq!(
        vertical.last_stable_key.as_deref(),
        Some("materialized:virtual-table:y:27")
    );
    assert_eq!(layout.metrics.materialized_range_count, 2);
}


#[test]
fn materialized_scroll_node_marks_descendants_with_clip_rect() {
    let mut frame = DocumentFrame::empty("root");

    let mut scroll = DocumentNode::new("scroll", DocumentNodeKind::Stack);
    scroll.parent = Some(frame.root.clone());
    scroll
        .style
        .insert("width".to_owned(), StyleValue::Number(200.0));
    scroll
        .style
        .insert("height".to_owned(), StyleValue::Number(80.0));
    scroll.materialized.push(MaterializedRange {
        materialization: Some(1),
        axis: Axis::Vertical,
        visible: 0..4,
        overscan: 0..8,
        logical_item_count: 8,
    });
    scroll.children.push(DocumentNodeId("row".to_owned()));

    let mut row = DocumentNode::new("row", DocumentNodeKind::Text);
    row.parent = Some(scroll.id.clone());
    row.text = Some(TextValue {
        text: "oversized row".to_owned(),
    });
    row.style
        .insert("width".to_owned(), StyleValue::Number(200.0));
    row.style
        .insert("height".to_owned(), StyleValue::Number(160.0));

    frame
        .nodes
        .get_mut(&frame.root)
        .unwrap()
        .children
        .push(scroll.id.clone());
    frame.nodes.insert(scroll.id.clone(), scroll);
    frame.nodes.insert(row.id.clone(), row);

    let mut text = SimpleTextMeasurer;
    let layout = layout(LayoutInput {
        document: &frame,
        viewport: Viewport {
            surface: 1,
            width: 300.0,
            height: 200.0,
            scale: 1.0,
        },
        text: &mut text,
        capabilities: RenderCapabilities::fake_portable(),
    });

    let row = layout
        .display_list
        .iter()
        .find(|item| item.node.0 == "row")
        .expect("scroll child should be laid out");

    assert_eq!(style_spacing(&row.style, "__clip_x"), Some(0.0));
    assert_eq!(style_spacing(&row.style, "__clip_y"), Some(0.0));
    assert_eq!(style_spacing(&row.style, "__clip_width"), Some(200.0));
    assert_eq!(style_spacing(&row.style, "__clip_height"), Some(80.0));
}
