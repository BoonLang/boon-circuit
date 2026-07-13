// Included by `../tests.rs`; kept in the parent test module for private document helper access.

#[test]
fn semantic_scene_derives_stable_roles_bounds_actions_and_patch() {
    let mut frame = DocumentFrame::empty("root");
    let mut title = node("title", DocumentNodeKind::Text, Some("root"));
    title.text = Some(TextValue {
        text: "Inbox".to_owned(),
    });
    title
        .style
        .insert("heading_level".to_owned(), StyleValue::Number(2.0));

    let mut button = node("save", DocumentNodeKind::Button, Some("root"));
    button.text = Some(TextValue {
        text: "Save".to_owned(),
    });
    button.set_primary_source_binding(boon_document_model::SourceBinding {
        id: SourceBindingId("source:save:press".to_owned()),
        source_path: "toolbar.save".to_owned(),
        intent: "press".to_owned(),
    });

    let mut checkbox = node("done", DocumentNodeKind::Checkbox, Some("root"));
    checkbox
        .style
        .insert("checked".to_owned(), StyleValue::Bool(true));
    checkbox.style.insert(
        "accessibility_label".to_owned(),
        StyleValue::Text("Done".to_owned()),
    );

    let mut input = node("filter", DocumentNodeKind::TextInput, Some("root"));
    input.text = Some(TextValue {
        text: "abc".to_owned(),
    });
    input.style.insert(
        "placeholder".to_owned(),
        StyleValue::Text("Filter".to_owned()),
    );
    frame.focus = Some(input.id.clone());

    let mut link = node("docs", DocumentNodeKind::Text, Some("root"));
    link.text = Some(TextValue {
        text: "Documentation".to_owned(),
    });
    link.style.insert("link".to_owned(), StyleValue::Bool(true));
    link.style.insert(
        "href".to_owned(),
        StyleValue::Text("https://example.com/docs".to_owned()),
    );

    frame.nodes.get_mut(&frame.root).unwrap().children.extend([
        title.id.clone(),
        button.id.clone(),
        checkbox.id.clone(),
        input.id.clone(),
        link.id.clone(),
    ]);
    frame.nodes.insert(title.id.clone(), title);
    frame.nodes.insert(button.id.clone(), button);
    frame.nodes.insert(checkbox.id.clone(), checkbox);
    frame.nodes.insert(input.id.clone(), input);
    frame.nodes.insert(link.id.clone(), link);

    let mut text = SimpleTextMeasurer;
    let layout = layout(LayoutInput {
        document: &frame,
        viewport: Viewport {
            surface: 1,
            width: 320.0,
            height: 180.0,
            scale: 1.0,
        },
        text: &mut text,
        capabilities: RenderCapabilities::fake_portable(),
    });

    let scene = semantic_scene_from_document_layout(&frame, &layout);
    assert_eq!(
        scene.root,
        Some(SemanticId("semantic:root".to_owned())),
        "root semantic id must be stable and document-derived"
    );
    assert_eq!(
        scene.focused,
        Some(SemanticId("semantic:filter".to_owned())),
        "document focus must project into the SemanticScene"
    );

    let button_semantic = scene
        .nodes
        .get(&SemanticId("semantic:save".to_owned()))
        .expect("button semantic node should exist");
    assert_eq!(button_semantic.role, SemanticRole::Button);
    assert_eq!(button_semantic.name.as_deref(), Some("Save"));
    assert!(button_semantic.actions.press);
    assert!(button_semantic.actions.focus);
    assert!(button_semantic.bounds.is_some());
    assert_eq!(
        button_semantic.source_binding_id,
        Some(SourceBindingId("source:save:press".to_owned()))
    );
    assert_eq!(button_semantic.source_path.as_deref(), Some("toolbar.save"));

    let checkbox_semantic = scene
        .nodes
        .get(&SemanticId("semantic:done".to_owned()))
        .expect("checkbox semantic node should exist");
    assert_eq!(checkbox_semantic.role, SemanticRole::Checkbox);
    assert_eq!(checkbox_semantic.name.as_deref(), Some("Done"));
    assert_eq!(checkbox_semantic.state.checked, Some(true));
    assert_eq!(
        checkbox_semantic.value,
        Some(SemanticValue::Bool { value: true })
    );

    let title_semantic = scene
        .nodes
        .get(&SemanticId("semantic:title".to_owned()))
        .expect("text semantic node should exist");
    assert_eq!(title_semantic.role, SemanticRole::Text);
    assert_eq!(title_semantic.heading_level, Some(2));

    let link_semantic = scene
        .nodes
        .get(&SemanticId("semantic:docs".to_owned()))
        .expect("link semantic node should exist");
    assert_eq!(link_semantic.role, SemanticRole::Link);
    assert_eq!(link_semantic.href.as_deref(), Some("https://example.com/docs"));
    assert!(link_semantic.actions.press);

    let mut next = scene.clone();
    next.nodes.remove(&SemanticId("semantic:done".to_owned()));
    let mut changed_button = next
        .nodes
        .remove(&SemanticId("semantic:save".to_owned()))
        .unwrap();
    changed_button.name = Some("Save now".to_owned());
    next.nodes.insert(changed_button.id.clone(), changed_button);
    next.focused = Some(SemanticId("semantic:save".to_owned()));

    let patch = scene.diff(&next);
    assert!(patch.operations.iter().any(|operation| matches!(
        operation,
        SemanticPatchOperation::RemoveNode { id } if id.0 == "semantic:done"
    )));
    assert!(patch.operations.iter().any(|operation| matches!(
        operation,
        SemanticPatchOperation::UpsertNode { node } if node.id.0 == "semantic:save"
            && node.name.as_deref() == Some("Save now")
    )));
    assert!(patch.operations.iter().any(|operation| matches!(
        operation,
        SemanticPatchOperation::SetFocus { focused: Some(id) } if id.0 == "semantic:save"
    )));
}


#[test]
fn semantic_dom_snapshot_exposes_minimal_web_semantics_not_visual_dom() {
    let mut scene = SemanticScene::default();
    scene.root = Some(SemanticId("semantic:root".to_owned()));
    scene.focused = Some(SemanticId("semantic:filter".to_owned()));
    scene.nodes.insert(
        SemanticId("semantic:root".to_owned()),
        SemanticNode {
            id: SemanticId("semantic:root".to_owned()),
            node: DocumentNodeId("root".to_owned()),
            role: SemanticRole::Application,
            name: Some("Boon app".to_owned()),
            description: None,
            value: None,
            state: SemanticState::default(),
            actions: SemanticActions::default(),
            relations: SemanticRelations::default(),
            bounds: None,
            language: Some("en".to_owned()),
            heading_level: None,
            href: None,
            source_binding_id: None,
            source_path: None,
            source_intent: None,
        },
    );
    scene.nodes.insert(
        SemanticId("semantic:save".to_owned()),
        SemanticNode {
            id: SemanticId("semantic:save".to_owned()),
            node: DocumentNodeId("save".to_owned()),
            role: SemanticRole::Button,
            name: Some("Save & <Close>".to_owned()),
            description: None,
            value: None,
            state: SemanticState::default(),
            actions: SemanticActions {
                focus: true,
                press: true,
                set_text: false,
                increment: false,
                decrement: false,
            },
            relations: SemanticRelations::default(),
            bounds: None,
            language: None,
            heading_level: None,
            href: None,
            source_binding_id: Some(SourceBindingId("source:save".to_owned())),
            source_path: Some("toolbar.save".to_owned()),
            source_intent: Some("press".to_owned()),
        },
    );
    scene.nodes.insert(
        SemanticId("semantic:done".to_owned()),
        SemanticNode {
            id: SemanticId("semantic:done".to_owned()),
            node: DocumentNodeId("done".to_owned()),
            role: SemanticRole::Checkbox,
            name: Some("Done".to_owned()),
            description: None,
            value: Some(SemanticValue::Bool { value: true }),
            state: SemanticState {
                checked: Some(true),
                ..SemanticState::default()
            },
            actions: SemanticActions {
                focus: true,
                press: true,
                set_text: false,
                increment: false,
                decrement: false,
            },
            relations: SemanticRelations::default(),
            bounds: None,
            language: None,
            heading_level: None,
            href: None,
            source_binding_id: None,
            source_path: None,
            source_intent: None,
        },
    );
    scene.nodes.insert(
        SemanticId("semantic:filter".to_owned()),
        SemanticNode {
            id: SemanticId("semantic:filter".to_owned()),
            node: DocumentNodeId("filter".to_owned()),
            role: SemanticRole::TextInput,
            name: Some("Filter".to_owned()),
            description: None,
            value: Some(SemanticValue::Text {
                text: "a\"b".to_owned(),
            }),
            state: SemanticState {
                focused: true,
                ..SemanticState::default()
            },
            actions: SemanticActions {
                focus: true,
                press: false,
                set_text: true,
                increment: false,
                decrement: false,
            },
            relations: SemanticRelations::default(),
            bounds: None,
            language: None,
            heading_level: None,
            href: None,
            source_binding_id: Some(SourceBindingId("source:filter:change".to_owned())),
            source_path: Some("toolbar.filter".to_owned()),
            source_intent: Some("change".to_owned()),
        },
    );

    let snapshot = SemanticDomSnapshot::from_scene(&scene);
    let html = snapshot.to_html_fragment();

    assert_eq!(snapshot.metrics.semantic_node_count, 4);
    assert_eq!(snapshot.metrics.dom_node_count, 4);
    assert_eq!(snapshot.metrics.data_boon_id_count, 4);
    assert_eq!(snapshot.metrics.text_input_endpoint_count, 1);
    assert_eq!(snapshot.metrics.visual_dom_node_count, 0);
    assert!(html.contains("id=\"boon-semantic-save\""));
    assert!(html.contains("data-boon-id=\"semantic:save\""));
    assert!(html.contains("data-boon-source-binding-id=\"source:save\""));
    assert!(html.contains("data-boon-source-path=\"toolbar.save\""));
    assert!(html.contains("data-boon-action-press=\"true\""));
    assert!(html.contains("Save &amp; &lt;Close&gt;"));
    assert!(html.contains("type=\"checkbox\""));
    assert!(html.contains("aria-checked=\"true\""));
    assert!(html.contains("data-boon-ime-endpoint=\"true\""));
    assert!(html.contains("value=\"a&quot;b\""));
    assert!(html.contains("data-boon-focused=\"true\""));
    assert!(!html.contains("<canvas"));
    assert!(!html.contains("<style"));
    assert!(!html.contains("<svg"));
}


#[test]
fn semantic_web_bridge_maps_ime_events_to_source_dispatch_without_visual_dom() {
    let mut scene = SemanticScene::default();
    scene.root = Some(SemanticId("semantic:root".to_owned()));
    scene.focused = Some(SemanticId("semantic:filter".to_owned()));
    scene.nodes.insert(
        SemanticId("semantic:filter".to_owned()),
        SemanticNode {
            id: SemanticId("semantic:filter".to_owned()),
            node: DocumentNodeId("filter".to_owned()),
            role: SemanticRole::TextInput,
            name: Some("Filter".to_owned()),
            description: None,
            value: Some(SemanticValue::Text {
                text: "abc".to_owned(),
            }),
            state: SemanticState {
                focused: true,
                ..SemanticState::default()
            },
            actions: SemanticActions {
                focus: true,
                press: false,
                set_text: true,
                increment: false,
                decrement: false,
            },
            relations: SemanticRelations::default(),
            bounds: None,
            language: None,
            heading_level: None,
            href: None,
            source_binding_id: Some(SourceBindingId("source:filter:change".to_owned())),
            source_path: Some("toolbar.filter".to_owned()),
            source_intent: Some("change".to_owned()),
        },
    );
    scene.nodes.insert(
        SemanticId("semantic:save".to_owned()),
        SemanticNode {
            id: SemanticId("semantic:save".to_owned()),
            node: DocumentNodeId("save".to_owned()),
            role: SemanticRole::Button,
            name: Some("Save".to_owned()),
            description: None,
            value: None,
            state: SemanticState::default(),
            actions: SemanticActions {
                focus: true,
                press: true,
                set_text: false,
                increment: false,
                decrement: false,
            },
            relations: SemanticRelations::default(),
            bounds: None,
            language: None,
            heading_level: None,
            href: None,
            source_binding_id: Some(SourceBindingId("source:save:press".to_owned())),
            source_path: Some("toolbar.save".to_owned()),
            source_intent: Some("press".to_owned()),
        },
    );
    scene.nodes.insert(
        SemanticId("semantic:zoom-in".to_owned()),
        SemanticNode {
            id: SemanticId("semantic:zoom-in".to_owned()),
            node: DocumentNodeId("zoom-in".to_owned()),
            role: SemanticRole::Button,
            name: Some("Zoom in".to_owned()),
            description: None,
            value: Some(SemanticValue::Number { value: 1.0 }),
            state: SemanticState::default(),
            actions: SemanticActions {
                focus: true,
                press: false,
                set_text: false,
                increment: true,
                decrement: false,
            },
            relations: SemanticRelations::default(),
            bounds: None,
            language: None,
            heading_level: None,
            href: None,
            source_binding_id: Some(SourceBindingId("source:zoom:increment".to_owned())),
            source_path: Some("viewport.zoom".to_owned()),
            source_intent: Some("increment".to_owned()),
        },
    );
    scene.nodes.insert(
        SemanticId("semantic:zoom-out".to_owned()),
        SemanticNode {
            id: SemanticId("semantic:zoom-out".to_owned()),
            node: DocumentNodeId("zoom-out".to_owned()),
            role: SemanticRole::Button,
            name: Some("Zoom out".to_owned()),
            description: None,
            value: Some(SemanticValue::Number { value: -1.0 }),
            state: SemanticState::default(),
            actions: SemanticActions {
                focus: true,
                press: false,
                set_text: false,
                increment: false,
                decrement: true,
            },
            relations: SemanticRelations::default(),
            bounds: None,
            language: None,
            heading_level: None,
            href: None,
            source_binding_id: Some(SourceBindingId("source:zoom:decrement".to_owned())),
            source_path: Some("viewport.zoom".to_owned()),
            source_intent: Some("decrement".to_owned()),
        },
    );

    let bridge = SemanticWebBridgeSnapshot::from_scene(&scene);
    let html = bridge.to_html_fragment();

    assert_eq!(bridge.metrics.semantic_node_count, 4);
    assert_eq!(bridge.metrics.dom_node_count, 4);
    assert_eq!(bridge.metrics.visual_dom_node_count, 0);
    assert_eq!(bridge.metrics.ime_endpoint_count, 1);
    assert_eq!(bridge.metrics.source_routed_action_count, 4);
    assert_eq!(bridge.ime_endpoints[0].dom_id, "boon-semantic-filter");
    assert_eq!(
        bridge.ime_endpoints[0].source_path.as_deref(),
        Some("toolbar.filter")
    );
    assert!(html.contains("data-boon-ime-endpoint=\"true\""));
    assert!(html.contains("data-boon-action-increment=\"true\""));
    assert!(html.contains("data-boon-action-decrement=\"true\""));
    assert!(!html.contains("<canvas"));
    assert!(!html.contains("<style"));
    assert!(!html.contains("<svg"));

    let text_dispatch = bridge
        .source_dispatch_for_event(SemanticWebInputEvent::SetText {
            semantic_id: SemanticId("semantic:filter".to_owned()),
            text: "next".to_owned(),
        })
        .expect("text input route should dispatch to a Boon source");
    assert_eq!(text_dispatch.source_path, "toolbar.filter");
    assert_eq!(text_dispatch.source_intent.as_deref(), Some("change"));
    assert_eq!(text_dispatch.text.as_deref(), Some("next"));

    let press_dispatch = bridge
        .source_dispatch_for_event(SemanticWebInputEvent::Press {
            semantic_id: SemanticId("semantic:save".to_owned()),
        })
        .expect("button route should dispatch to a Boon source");
    assert_eq!(press_dispatch.source_path, "toolbar.save");
    assert_eq!(press_dispatch.source_intent.as_deref(), Some("press"));
    assert_eq!(press_dispatch.text, None);

    let increment_dispatch = bridge
        .source_dispatch_for_event(SemanticWebInputEvent::Increment {
            semantic_id: SemanticId("semantic:zoom-in".to_owned()),
        })
        .expect("increment route should dispatch to a Boon source");
    assert_eq!(increment_dispatch.source_path, "viewport.zoom");
    assert_eq!(
        increment_dispatch.source_intent.as_deref(),
        Some("increment")
    );
    assert_eq!(increment_dispatch.text, None);

    let decrement_dispatch = bridge
        .source_dispatch_for_event(SemanticWebInputEvent::Decrement {
            semantic_id: SemanticId("semantic:zoom-out".to_owned()),
        })
        .expect("decrement route should dispatch to a Boon source");
    assert_eq!(decrement_dispatch.source_path, "viewport.zoom");
    assert_eq!(
        decrement_dispatch.source_intent.as_deref(),
        Some("decrement")
    );
    assert_eq!(decrement_dispatch.text, None);
}


#[test]
fn document_batch_accepts_ui_semantic_change_batch() {
    let mut state = DocumentState::new("root");
    let change_set = state
        .apply_ui_semantic_batch(ChangeBatch {
            epoch: 7,
            changes: vec![
                UiSemanticChange::InsertNode {
                    parent: DocumentNodeId("root".to_owned()),
                    index: 0,
                    node: node("label", DocumentNodeKind::Text, None),
                },
                UiSemanticChange::SetText {
                    id: DocumentNodeId("label".to_owned()),
                    text: TextValue {
                        text: "Semantic".to_owned(),
                    },
                },
                UiSemanticChange::SetVisibility {
                    id: DocumentNodeId("label".to_owned()),
                    visible: false,
                },
            ],
        })
        .unwrap();

    assert_eq!(change_set.patch_count, 4);
    assert_eq!(change_set.node_count_before, 1);
    assert_eq!(change_set.node_count_after, 2);
    assert_eq!(
        state.frame().nodes[&DocumentNodeId("root".to_owned())].children,
        vec![DocumentNodeId("label".to_owned())]
    );
    let label = &state.frame().nodes[&DocumentNodeId("label".to_owned())];
    assert_eq!(label.parent, Some(DocumentNodeId("root".to_owned())));
    assert_eq!(label.text.as_ref().unwrap().text, "Semantic");
    assert_eq!(label.style.get("visible"), Some(&StyleValue::Bool(false)));
    assert!(
        change_set
            .invalidation
            .contains(&PatchInvalidationClass::Structure)
    );
    assert!(
        change_set
            .invalidation
            .contains(&PatchInvalidationClass::Text)
    );
    assert!(
        change_set
            .invalidation
            .contains(&PatchInvalidationClass::Style)
    );
}


#[test]
fn document_batch_set_binding_at_updates_canonical_ordinal_only() {
    let mut state = DocumentState::new("root");
    let mut button = node("button", DocumentNodeKind::Button, Some("root"));
    button.set_primary_source_binding(boon_document_model::SourceBinding {
        id: SourceBindingId("source:button:press".to_owned()),
        source_path: "old.press".to_owned(),
        intent: "press".to_owned(),
    });
    button
        .source_bindings
        .push(boon_document_model::SourceBinding {
            id: SourceBindingId("source:button:change".to_owned()),
            source_path: "old.change".to_owned(),
            intent: "change".to_owned(),
        });
    state
        .apply_patch(DocumentPatch::UpsertNode(button))
        .unwrap();

    let change_set = state
        .apply_ui_semantic_batch(ChangeBatch {
            epoch: 12,
            changes: vec![UiSemanticChange::SetBindingAt {
                id: DocumentNodeId("button".to_owned()),
                ordinal: 1,
                binding: boon_document_model::SourceBinding {
                    id: SourceBindingId("source:button:change".to_owned()),
                    source_path: "new.change".to_owned(),
                    intent: "change".to_owned(),
                },
            }],
        })
        .unwrap();

    let button = &state.frame().nodes[&DocumentNodeId("button".to_owned())];
    assert_eq!(change_set.patch_count, 1);
    assert_eq!(
        change_set
            .reports
            .iter()
            .map(|report| report.patch_kind)
            .collect::<Vec<_>>(),
        vec!["set_binding_at"]
    );
    assert!(
        change_set
            .invalidation
            .contains(&PatchInvalidationClass::SourceBinding)
    );
    assert_eq!(
        button
            .source_bindings
            .first()
            .map(|binding| binding.source_path.as_str()),
        Some("old.press"),
        "secondary binding updates must not rewrite ordinal zero"
    );
    assert_eq!(
        button
            .source_bindings
            .get(1)
            .map(|binding| binding.source_path.as_str()),
        Some("new.change")
    );
}


#[test]
fn document_batch_ui_semantic_typed_style_preserves_typed_patch_kinds() {
    let mut state = DocumentState::new("root");
    state
        .apply_patch(DocumentPatch::UpsertNode(node(
            "label",
            DocumentNodeKind::Text,
            Some("root"),
        )))
        .unwrap();

    let change_set = state
        .apply_ui_semantic_batch(ChangeBatch {
            epoch: 8,
            changes: vec![
                UiSemanticChange::SetLayoutStyle {
                    id: DocumentNodeId("label".to_owned()),
                    patch: LayoutStylePatch {
                        patch: BTreeMap::from([(
                            "width".to_owned(),
                            Some(StyleValue::Number(120.0)),
                        )]),
                    },
                },
                UiSemanticChange::SetPaintStyle {
                    id: DocumentNodeId("label".to_owned()),
                    patch: PaintStylePatch {
                        patch: BTreeMap::from([(
                            "background".to_owned(),
                            Some(StyleValue::Text("#fff".to_owned())),
                        )]),
                    },
                },
            ],
        })
        .unwrap();

    assert_eq!(change_set.patch_count, 2);
    assert_eq!(
        change_set
            .reports
            .iter()
            .map(|report| report.patch_kind)
            .collect::<Vec<_>>(),
        vec!["set_layout_style", "set_paint_style"]
    );
    assert!(
        change_set
            .invalidation
            .contains(&PatchInvalidationClass::Layout)
    );
    assert!(
        change_set
            .invalidation
            .contains(&PatchInvalidationClass::PaintOnly)
    );
    let label = &state.frame().nodes[&DocumentNodeId("label".to_owned())];
    assert_eq!(label.style.get("width"), Some(&StyleValue::Number(120.0)));
    assert_eq!(
        label.style.get("background"),
        Some(&StyleValue::Text("#fff".to_owned()))
    );
}


#[test]
fn typed_style_index_extracts_known_hot_style_properties() {
    let mut alpha = node("alpha", DocumentNodeKind::Text, Some("root"));
    alpha
        .style
        .insert("width".to_owned(), StyleValue::Text("Fill".to_owned()));
    alpha
        .style
        .insert("height".to_owned(), StyleValue::Text("auto".to_owned()));
    alpha
        .style
        .insert("min_width".to_owned(), StyleValue::Text("120".to_owned()));
    alpha
        .style
        .insert("gap".to_owned(), StyleValue::Number(8.0));
    alpha
        .style
        .insert("padding".to_owned(), StyleValue::Number(4.0));
    alpha
        .style
        .insert("padding_left".to_owned(), StyleValue::Number(10.0));
    alpha
        .style
        .insert("center".to_owned(), StyleValue::Bool(true));
    alpha
        .style
        .insert("align_x".to_owned(), StyleValue::Text("right".to_owned()));
    alpha
        .style
        .insert("color".to_owned(), StyleValue::Text("red".to_owned()));
    alpha
        .style
        .insert("opacity".to_owned(), StyleValue::Number(0.5));
    alpha.style.insert(
        "font_weight".to_owned(),
        StyleValue::Text("bold".to_owned()),
    );
    alpha
        .style
        .insert("line_height".to_owned(), StyleValue::Number(18.0));
    alpha
        .style
        .insert("material".to_owned(), StyleValue::Text("flat".to_owned()));
    alpha
        .style
        .insert("border_radius".to_owned(), StyleValue::Number(6.0));
    alpha
        .style
        .insert("__hover_scope".to_owned(), StyleValue::Bool(true));
    alpha
        .style
        .insert("__clip_x".to_owned(), StyleValue::Number(1.0));
    alpha
        .style
        .insert("__clip_y".to_owned(), StyleValue::Number(2.0));
    alpha
        .style
        .insert("__clip_width".to_owned(), StyleValue::Number(3.0));
    alpha
        .style
        .insert("__clip_height".to_owned(), StyleValue::Number(4.0));

    let mut state = DocumentState::new("root");
    state.apply_patch(DocumentPatch::UpsertNode(alpha)).unwrap();
    let hot_ids = DocumentHotIdTable::from_frame(state.frame()).unwrap();
    let alpha_hot = hot_ids.hot_id(&DocumentNodeId("alpha".to_owned())).unwrap();
    let typed = DocumentTypedStyleIndex::from_frame(state.frame(), &hot_ids).unwrap();
    let record = typed.record(alpha_hot).unwrap();

    assert_eq!(record.layout.width, Some(DocumentStyleDimension::Fill));
    assert_eq!(record.layout.height, Some(DocumentStyleDimension::Auto));
    assert_eq!(
        record.layout.min_width,
        Some(DocumentStyleDimension::Px { value: 120.0 })
    );
    assert_eq!(record.layout.gap, Some(8.0));
    assert_eq!(
        record.layout.padding,
        DocumentTypedEdgeSpacing {
            top: 4.0,
            right: 4.0,
            bottom: 4.0,
            left: 10.0,
        }
    );
    assert!(record.layout.center);
    assert_eq!(record.layout.align_x.as_deref(), Some("right"));
    assert_eq!(
        record.layout.clip,
        Some(Rect {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        })
    );
    assert_eq!(record.paint.color.as_deref(), Some("red"));
    assert_eq!(record.paint.opacity, Some(0.5));
    assert_eq!(record.text.font_weight.as_deref(), Some("bold"));
    assert_eq!(record.text.line_height, Some(18.0));
    assert_eq!(record.material.material.as_deref(), Some("flat"));
    assert_eq!(record.material.border_radius, Some(6.0));
    assert!(record.pseudo.hover_scope);

    let previous_hot_ids = DocumentHotIdTable::from_frame(&DocumentFrame::empty("root")).unwrap();
    let err = DocumentTypedStyleIndex::from_frame(state.frame(), &previous_hot_ids).unwrap_err();
    assert!(matches!(
        err,
        PatchApplyError::StaleReference {
            reference_kind: "hot_id_table",
            ..
        }
    ));
}


#[test]
fn checkbox_size_wins_over_accessibility_label_text() {
    let mut frame = DocumentFrame::empty("root");

    let mut checkbox = DocumentNode::new("checkbox", DocumentNodeKind::Checkbox);
    checkbox.parent = Some(frame.root.clone());
    checkbox
        .style
        .insert("size".to_owned(), StyleValue::Number(40.0));
    checkbox.text = Some(TextValue {
        text: "Reference[element:todo.title]".to_owned(),
    });

    frame
        .nodes
        .get_mut(&frame.root)
        .unwrap()
        .children
        .push(checkbox.id.clone());
    frame.nodes.insert(checkbox.id.clone(), checkbox);

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

    let checkbox = layout
        .display_list
        .iter()
        .find(|item| item.node.0 == "checkbox")
        .expect("checkbox should be laid out");

    assert_eq!(checkbox.bounds.width, 40.0);
    assert_eq!(checkbox.bounds.height, 40.0);
}
