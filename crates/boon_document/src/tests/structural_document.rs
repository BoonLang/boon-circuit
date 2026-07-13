// Included by `../tests.rs`; kept in the parent test module for private document helper access.

#[test]
fn row_fill_uses_remaining_width_after_fixed_siblings() {
    let mut frame = DocumentFrame::empty("root");

    let mut row = DocumentNode::new("row", DocumentNodeKind::Row);
    row.parent = Some(frame.root.clone());
    row.style
        .insert("width".to_owned(), StyleValue::Number(300.0));
    row.style
        .insert("height".to_owned(), StyleValue::Number(40.0));
    row.style.insert("gap".to_owned(), StyleValue::Number(8.0));
    row.children.push(DocumentNodeId("fixed".to_owned()));
    row.children.push(DocumentNodeId("fill".to_owned()));

    let mut fixed = DocumentNode::new("fixed", DocumentNodeKind::Text);
    fixed.parent = Some(row.id.clone());
    fixed
        .style
        .insert("width".to_owned(), StyleValue::Number(50.0));
    fixed
        .style
        .insert("height".to_owned(), StyleValue::Number(20.0));

    let mut fill = DocumentNode::new("fill", DocumentNodeKind::Text);
    fill.parent = Some(row.id.clone());
    fill.style
        .insert("width".to_owned(), StyleValue::Text("fill".to_owned()));
    fill.style
        .insert("height".to_owned(), StyleValue::Number(20.0));

    frame
        .nodes
        .get_mut(&frame.root)
        .unwrap()
        .children
        .push(row.id.clone());
    frame.nodes.insert(row.id.clone(), row);
    frame.nodes.insert(fixed.id.clone(), fixed);
    frame.nodes.insert(fill.id.clone(), fill);

    let mut text = SimpleTextMeasurer;
    let layout = layout(LayoutInput {
        document: &frame,
        viewport: Viewport {
            surface: 1,
            width: 300.0,
            height: 80.0,
            scale: 1.0,
        },
        text: &mut text,
        capabilities: RenderCapabilities::fake_portable(),
    });

    let fixed = layout
        .display_list
        .iter()
        .find(|item| item.node.0 == "fixed")
        .expect("fixed child should be laid out");
    let fill = layout
        .display_list
        .iter()
        .find(|item| item.node.0 == "fill")
        .expect("fill child should be laid out");

    assert_eq!(fixed.bounds.width, 50.0);
    assert_eq!(fill.bounds.x, 58.0);
    assert_eq!(fill.bounds.width, 242.0);
    assert!(fill.bounds.x + fill.bounds.width <= 300.0);
}


#[test]
fn layout_subtree_matches_whole_frame_row_geometry() {
    let mut frame = DocumentFrame::empty("root");

    let mut row = DocumentNode::new("row", DocumentNodeKind::Row);
    row.parent = Some(frame.root.clone());
    row.style
        .insert("width".to_owned(), StyleValue::Number(300.0));
    row.style
        .insert("height".to_owned(), StyleValue::Number(80.0));
    row.style.insert("gap".to_owned(), StyleValue::Number(0.0));
    row.children.push(DocumentNodeId("panel".to_owned()));
    row.children.push(DocumentNodeId("sibling".to_owned()));

    let mut panel = DocumentNode::new("panel", DocumentNodeKind::Stack);
    panel.parent = Some(row.id.clone());
    panel
        .style
        .insert("width".to_owned(), StyleValue::Number(180.0));
    panel
        .style
        .insert("height".to_owned(), StyleValue::Text("fill".to_owned()));
    panel
        .style
        .insert("padding".to_owned(), StyleValue::Number(10.0));
    panel
        .style
        .insert("gap".to_owned(), StyleValue::Number(4.0));
    panel.children.push(DocumentNodeId("header".to_owned()));

    let mut header = DocumentNode::new("header", DocumentNodeKind::Row);
    header.parent = Some(panel.id.clone());
    header
        .style
        .insert("width".to_owned(), StyleValue::Text("fill".to_owned()));
    header
        .style
        .insert("height".to_owned(), StyleValue::Number(20.0));

    let mut sibling = DocumentNode::new("sibling", DocumentNodeKind::Stack);
    sibling.parent = Some(row.id.clone());
    sibling
        .style
        .insert("width".to_owned(), StyleValue::Number(50.0));
    sibling
        .style
        .insert("height".to_owned(), StyleValue::Text("fill".to_owned()));

    frame
        .nodes
        .get_mut(&frame.root)
        .unwrap()
        .children
        .push(row.id.clone());
    frame.nodes.insert(row.id.clone(), row);
    frame.nodes.insert(panel.id.clone(), panel);
    frame.nodes.insert(header.id.clone(), header);
    frame.nodes.insert(sibling.id.clone(), sibling);

    let mut full_text = SimpleTextMeasurer;
    let full = layout(LayoutInput {
        document: &frame,
        viewport: Viewport {
            surface: 1,
            width: 300.0,
            height: 100.0,
            scale: 1.0,
        },
        text: &mut full_text,
        capabilities: RenderCapabilities::fake_portable(),
    });
    let mut subtree_text = SimpleTextMeasurer;
    let subtree = layout_subtree(LayoutSubtreeInput {
        document: &frame,
        root: &DocumentNodeId("row".to_owned()),
        x: 0.0,
        y: 0.0,
        available_width: 300.0,
        available_height: 80.0,
        text: &mut subtree_text,
        capabilities: RenderCapabilities::fake_portable(),
    });

    for id in ["row", "panel", "header", "sibling"] {
        let full_bounds = full
            .display_list
            .iter()
            .find(|item| item.node.0 == id)
            .unwrap()
            .bounds;
        let subtree_bounds = subtree
            .display_list
            .iter()
            .find(|item| item.node.0 == id)
            .unwrap()
            .bounds;
        assert_eq!(subtree_bounds, full_bounds, "bounds differ for {id}");
    }
    assert_eq!(subtree.metrics.node_count, 4);
    assert_eq!(subtree.metrics.display_item_count, 4);
}


#[test]
fn row_multiple_fill_children_share_remaining_width() {
    let mut frame = DocumentFrame::empty("root");

    let mut row = DocumentNode::new("row", DocumentNodeKind::Row);
    row.parent = Some(frame.root.clone());
    row.style
        .insert("width".to_owned(), StyleValue::Number(330.0));
    row.style
        .insert("height".to_owned(), StyleValue::Number(40.0));
    row.style.insert("gap".to_owned(), StyleValue::Number(15.0));

    for id in ["left", "middle", "right"] {
        row.children.push(DocumentNodeId(id.to_owned()));
        let mut child = DocumentNode::new(id, DocumentNodeKind::Stack);
        child.parent = Some(row.id.clone());
        child
            .style
            .insert("width".to_owned(), StyleValue::Text("Fill".to_owned()));
        child
            .style
            .insert("height".to_owned(), StyleValue::Number(20.0));
        frame.nodes.insert(child.id.clone(), child);
    }

    frame
        .nodes
        .get_mut(&frame.root)
        .unwrap()
        .children
        .push(row.id.clone());
    frame.nodes.insert(row.id.clone(), row);

    let mut text = SimpleTextMeasurer;
    let layout = layout(LayoutInput {
        document: &frame,
        viewport: Viewport {
            surface: 1,
            width: 330.0,
            height: 80.0,
            scale: 1.0,
        },
        text: &mut text,
        capabilities: RenderCapabilities::fake_portable(),
    });

    let child = |id: &str| {
        layout
            .display_list
            .iter()
            .find(|item| item.node.0 == id)
            .unwrap_or_else(|| panic!("child `{id}` should be laid out"))
    };
    let left = child("left");
    let middle = child("middle");
    let right = child("right");

    assert_eq!(left.bounds.width, 100.0);
    assert_eq!(middle.bounds.width, 100.0);
    assert_eq!(right.bounds.width, 100.0);
    assert_eq!(middle.bounds.x, 115.0);
    assert_eq!(right.bounds.x, 230.0);
    assert!(right.bounds.x + right.bounds.width <= 330.0);
}

#[test]
fn row_fill_redistributes_width_after_min_max_constraints() {
    let mut frame = DocumentFrame::empty("root");
    let mut row = DocumentNode::new("row", DocumentNodeKind::Row);
    row.parent = Some(frame.root.clone());
    row.style
        .insert("width".to_owned(), StyleValue::Text("Fill".to_owned()));
    row.style
        .insert("height".to_owned(), StyleValue::Number(40.0));
    row.style.insert("gap".to_owned(), StyleValue::Number(4.0));

    let mut left = DocumentNode::new("left", DocumentNodeKind::Stack);
    left.parent = Some(row.id.clone());
    left.style
        .insert("width".to_owned(), StyleValue::Text("Fill".to_owned()));
    left.style
        .insert("min_width".to_owned(), StyleValue::Number(220.0));
    left.style
        .insert("max_width".to_owned(), StyleValue::Number(430.0));

    let mut right = DocumentNode::new("right", DocumentNodeKind::Stack);
    right.parent = Some(row.id.clone());
    right
        .style
        .insert("width".to_owned(), StyleValue::Text("Fill".to_owned()));

    row.children.push(left.id.clone());
    row.children.push(right.id.clone());
    frame
        .nodes
        .get_mut(&frame.root)
        .unwrap()
        .children
        .push(row.id.clone());
    frame.nodes.insert(row.id.clone(), row);
    frame.nodes.insert(left.id.clone(), left);
    frame.nodes.insert(right.id.clone(), right);

    let bounds_at = |width: f32| {
        let mut text = SimpleTextMeasurer;
        let layout = layout(LayoutInput {
            document: &frame,
            viewport: Viewport {
                surface: 1,
                width,
                height: 80.0,
                scale: 1.0,
            },
            text: &mut text,
            capabilities: RenderCapabilities::fake_portable(),
        });
        let child_width = |id: &str| {
            layout
                .display_list
                .iter()
                .find(|item| item.node.0 == id)
                .unwrap()
                .bounds
                .width
        };
        (child_width("left"), child_width("right"))
    };

    assert_eq!(bounds_at(508.0), (252.0, 252.0));
    assert_eq!(bounds_at(1_020.0), (430.0, 586.0));
}


#[test]
fn button_with_element_label_shrinks_to_label_child() {
    let mut frame = DocumentFrame::empty("root");

    let mut row = DocumentNode::new("row", DocumentNodeKind::Row);
    row.parent = Some(frame.root.clone());
    row.style
        .insert("width".to_owned(), StyleValue::Number(300.0));
    row.style.insert("gap".to_owned(), StyleValue::Number(10.0));
    row.children.push(DocumentNodeId("one-button".to_owned()));
    row.children.push(DocumentNodeId("two-button".to_owned()));

    let mut one_button = DocumentNode::new("one-button", DocumentNodeKind::Button);
    one_button.parent = Some(row.id.clone());
    one_button
        .children
        .push(DocumentNodeId("one-label".to_owned()));

    let mut one_label = DocumentNode::new("one-label", DocumentNodeKind::Text);
    one_label.parent = Some(one_button.id.clone());
    one_label.text = Some(TextValue {
        text: "One".to_owned(),
    });

    let mut two_button = DocumentNode::new("two-button", DocumentNodeKind::Button);
    two_button.parent = Some(row.id.clone());
    two_button
        .children
        .push(DocumentNodeId("two-label".to_owned()));

    let mut two_label = DocumentNode::new("two-label", DocumentNodeKind::Text);
    two_label.parent = Some(two_button.id.clone());
    two_label.text = Some(TextValue {
        text: "Two".to_owned(),
    });

    frame
        .nodes
        .get_mut(&frame.root)
        .unwrap()
        .children
        .push(row.id.clone());
    frame.nodes.insert(row.id.clone(), row);
    frame.nodes.insert(one_button.id.clone(), one_button);
    frame.nodes.insert(one_label.id.clone(), one_label);
    frame.nodes.insert(two_button.id.clone(), two_button);
    frame.nodes.insert(two_label.id.clone(), two_label);

    let mut text = SimpleTextMeasurer;
    let layout = layout(LayoutInput {
        document: &frame,
        viewport: Viewport {
            surface: 1,
            width: 300.0,
            height: 80.0,
            scale: 1.0,
        },
        text: &mut text,
        capabilities: RenderCapabilities::fake_portable(),
    });

    let one_button = layout
        .display_list
        .iter()
        .find(|item| item.node.0 == "one-button")
        .expect("first button should be laid out");
    let one_label = layout
        .display_list
        .iter()
        .find(|item| item.node.0 == "one-label")
        .expect("first label should be laid out");
    let two_button = layout
        .display_list
        .iter()
        .find(|item| item.node.0 == "two-button")
        .expect("second button should be laid out");

    assert_eq!(one_label.bounds.width, 23.1);
    assert_eq!(one_button.bounds.width, one_label.bounds.width);
    assert_eq!(two_button.bounds.x, one_button.bounds.width + 10.0);
    assert!(two_button.bounds.x + two_button.bounds.width < 300.0);
}


#[test]
fn inherited_font_size_does_not_force_stack_box_size() {
    let mut frame = DocumentFrame::empty("root");

    let mut stack = DocumentNode::new("stack", DocumentNodeKind::Stack);
    stack.parent = Some(frame.root.clone());
    stack
        .style
        .insert("size".to_owned(), StyleValue::Number(14.0));
    stack.children.push(DocumentNodeId("child".to_owned()));

    let mut child = DocumentNode::new("child", DocumentNodeKind::Text);
    child.parent = Some(stack.id.clone());
    child
        .style
        .insert("width".to_owned(), StyleValue::Number(100.0));
    child
        .style
        .insert("height".to_owned(), StyleValue::Number(50.0));

    frame
        .nodes
        .get_mut(&frame.root)
        .unwrap()
        .children
        .push(stack.id.clone());
    frame.nodes.insert(stack.id.clone(), stack);
    frame.nodes.insert(child.id.clone(), child);

    let mut text = SimpleTextMeasurer;
    let layout = layout(LayoutInput {
        document: &frame,
        viewport: Viewport {
            surface: 1,
            width: 300.0,
            height: 100.0,
            scale: 1.0,
        },
        text: &mut text,
        capabilities: RenderCapabilities::fake_portable(),
    });

    let stack = layout
        .display_list
        .iter()
        .find(|item| item.node.0 == "stack")
        .expect("stack should be laid out");

    assert_eq!(stack.bounds.height, 50.0);
}


#[test]
fn stack_overlay_children_share_parent_origin() {
    let mut frame = DocumentFrame::empty("root");

    let mut stack = DocumentNode::new("stack", DocumentNodeKind::Stack);
    stack.parent = Some(frame.root.clone());
    stack
        .style
        .insert("width".to_owned(), StyleValue::Number(300.0));
    stack
        .style
        .insert("height".to_owned(), StyleValue::Number(180.0));
    stack
        .style
        .insert("overlay_children".to_owned(), StyleValue::Bool(true));
    stack.children.push(DocumentNodeId("content".to_owned()));
    stack.children.push(DocumentNodeId("modal".to_owned()));

    let mut content = DocumentNode::new("content", DocumentNodeKind::Stack);
    content.parent = Some(stack.id.clone());
    content
        .style
        .insert("width".to_owned(), StyleValue::Text("Fill".to_owned()));
    content
        .style
        .insert("height".to_owned(), StyleValue::Text("Fill".to_owned()));

    let mut modal = DocumentNode::new("modal", DocumentNodeKind::Stack);
    modal.parent = Some(stack.id.clone());
    modal
        .style
        .insert("width".to_owned(), StyleValue::Number(120.0));
    modal
        .style
        .insert("height".to_owned(), StyleValue::Number(60.0));
    modal
        .style
        .insert("center".to_owned(), StyleValue::Bool(true));

    frame
        .nodes
        .get_mut(&frame.root)
        .unwrap()
        .children
        .push(stack.id.clone());
    frame.nodes.insert(stack.id.clone(), stack);
    frame.nodes.insert(content.id.clone(), content);
    frame.nodes.insert(modal.id.clone(), modal);

    let mut text = SimpleTextMeasurer;
    let layout = layout(LayoutInput {
        document: &frame,
        viewport: Viewport {
            surface: 1,
            width: 300.0,
            height: 180.0,
            scale: 1.0,
        },
        text: &mut text,
        capabilities: RenderCapabilities::fake_portable(),
    });

    let content = layout
        .display_list
        .iter()
        .find(|item| item.node.0 == "content")
        .expect("content layer should be laid out");
    let modal = layout
        .display_list
        .iter()
        .find(|item| item.node.0 == "modal")
        .expect("modal layer should be laid out");

    assert_eq!(content.bounds.x, 0.0);
    assert_eq!(content.bounds.y, 0.0);
    assert_eq!(content.bounds.height, 180.0);
    assert_eq!(modal.bounds.x, 90.0);
    assert_eq!(modal.bounds.y, 0.0);
    assert_eq!(modal.bounds.height, 60.0);
}
