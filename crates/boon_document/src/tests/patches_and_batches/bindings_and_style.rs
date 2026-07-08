#[test]
fn typed_binding_index_exposes_single_binding_from_canonical_vector() {
    let mut button = node("button", DocumentNodeKind::Button, Some("root"));
    button.set_primary_source_binding(boon_document_model::SourceBinding {
        id: SourceBindingId("source:button:press".to_owned()),
        source_path: "todos[0].done".to_owned(),
        intent: "toggle".to_owned(),
    });

    let mut state = DocumentState::new("root");
    state
        .apply_patch(DocumentPatch::UpsertNode(button))
        .unwrap();
    let hot_ids = DocumentHotIdTable::from_frame(state.frame()).unwrap();
    let intern_index = DocumentInternIndex::from_frame(state.frame(), &hot_ids).unwrap();
    let bindings =
        DocumentTypedBindingIndex::from_frame(state.frame(), &hot_ids, &intern_index).unwrap();
    let button_hot = hot_ids
        .hot_id(&DocumentNodeId("button".to_owned()))
        .unwrap();
    let binding = bindings
        .bindings_for_node(button_hot)
        .first()
        .expect("button should expose its source binding");

    assert_eq!(binding.reference.node, button_hot);
    assert_eq!(binding.reference.ordinal, 0);
    assert_eq!(binding.binding_id.0, "source:button:press");
    assert_eq!(binding.route.source_path, "todos[0].done");
    assert_eq!(binding.route.intent, "toggle");
    assert_eq!(
        Some(binding.intern_id),
        intern_index
            .nodes
            .get(&button_hot)
            .unwrap()
            .source_bindings
            .first()
            .copied()
    );
    assert_eq!(
        bindings.refs_for_binding_id(&SourceBindingId("source:button:press".to_owned())),
        &[binding.reference]
    );
    assert_eq!(
        bindings.refs_for_route(&DocumentTypedBindingRoute {
            source_path: "todos[0].done".to_owned(),
            intent: "toggle".to_owned(),
        }),
        &[binding.reference]
    );
    assert!(
        bindings
            .bindings_for_node(DocumentHotNodeId(999))
            .is_empty()
    );

    let stale_hot = DocumentHotIdTable::from_frame(&DocumentFrame::empty("root")).unwrap();
    let stale_hot_err =
        DocumentTypedBindingIndex::from_frame(state.frame(), &stale_hot, &intern_index)
            .unwrap_err();
    assert!(matches!(
        stale_hot_err,
        PatchApplyError::StaleReference {
            reference_kind: "hot_id_table",
            ..
        }
    ));

    let stale_intern =
        DocumentInternIndex::from_frame(&DocumentFrame::empty("root"), &stale_hot).unwrap();
    let stale_intern_err =
        DocumentTypedBindingIndex::from_frame(state.frame(), &hot_ids, &stale_intern).unwrap_err();
    assert!(matches!(
        stale_intern_err,
        PatchApplyError::StaleReference {
            reference_kind: "document_intern_index",
            ..
        }
    ));
}


#[test]
fn typed_binding_index_preserves_multiple_bindings_per_node() {
    let mut button = node("button", DocumentNodeKind::Button, Some("root"));
    button
        .style
        .insert("width".to_owned(), StyleValue::Number(120.0));
    button
        .style
        .insert("height".to_owned(), StyleValue::Number(32.0));
    button.set_primary_source_binding(boon_document_model::SourceBinding {
        id: SourceBindingId("source:button:press".to_owned()),
        source_path: "controls.primary.press".to_owned(),
        intent: "press".to_owned(),
    });
    button
        .source_bindings
        .push(boon_document_model::SourceBinding {
            id: SourceBindingId("source:button:change".to_owned()),
            source_path: "controls.primary.change".to_owned(),
            intent: "change".to_owned(),
        });

    let mut state = DocumentState::new("root");
    state
        .apply_patch(DocumentPatch::UpsertNode(button))
        .unwrap();
    let hot_ids = DocumentHotIdTable::from_frame(state.frame()).unwrap();
    let intern_index = DocumentInternIndex::from_frame(state.frame(), &hot_ids).unwrap();
    let bindings =
        DocumentTypedBindingIndex::from_frame(state.frame(), &hot_ids, &intern_index).unwrap();
    let button_hot = hot_ids
        .hot_id(&DocumentNodeId("button".to_owned()))
        .unwrap();
    let node_bindings = bindings.bindings_for_node(button_hot);

    assert_eq!(node_bindings.len(), 2);
    assert_eq!(
        node_bindings
            .iter()
            .map(|binding| binding.reference.ordinal)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(
        intern_index
            .nodes
            .get(&button_hot)
            .unwrap()
            .source_bindings
            .len(),
        2
    );
    assert_eq!(
        Some(node_bindings[0].intern_id),
        intern_index
            .nodes
            .get(&button_hot)
            .unwrap()
            .source_bindings
            .first()
            .copied()
    );
    assert_eq!(
        bindings.refs_for_route(&DocumentTypedBindingRoute {
            source_path: "controls.primary.press".to_owned(),
            intent: "press".to_owned(),
        }),
        &[DocumentTypedBindingRef {
            node: button_hot,
            ordinal: 0,
        }]
    );
    assert_eq!(
        bindings.refs_for_route(&DocumentTypedBindingRoute {
            source_path: "controls.primary.change".to_owned(),
            intent: "change".to_owned(),
        }),
        &[DocumentTypedBindingRef {
            node: button_hot,
            ordinal: 1,
        }]
    );

    let mut measurer = SimpleTextMeasurer;
    let layout = layout(LayoutInput {
        document: state.frame(),
        viewport: boon_host::Viewport {
            surface: 1,
            width: 240.0,
            height: 80.0,
            scale: 1.0,
        },
        text: &mut measurer,
        capabilities: RenderCapabilities::fake_portable(),
    });
    let hit_table = HitSideTable::try_from_document_layout_with_typed_bindings(
        state.frame(),
        &hot_ids,
        &bindings,
        &layout,
        64.0,
    )
    .unwrap();
    let entry = hit_table
        .entry_for_source_path("controls.primary.change")
        .expect("secondary binding should be discoverable through typed hit routes");
    assert_eq!(entry.source_path.as_deref(), Some("controls.primary.press"));
    assert_eq!(
        entry.source_binding_refs,
        vec![
            DocumentTypedBindingRef {
                node: button_hot,
                ordinal: 0,
            },
            DocumentTypedBindingRef {
                node: button_hot,
                ordinal: 1,
            },
        ]
    );
    assert_eq!(
        entry
            .source_routes
            .iter()
            .map(|route| (route.source_path.as_str(), route.intent.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("controls.primary.press", "press"),
            ("controls.primary.change", "change"),
        ]
    );
}


#[test]
fn style_patch_reports_precise_invalidation_classes() {
    let mut state = DocumentState::new("root");
    state
        .apply_patch(DocumentPatch::UpsertNode(node(
            "label",
            DocumentNodeKind::Text,
            Some("root"),
        )))
        .unwrap();

    let mut patch = StylePatch::new();
    patch.insert("width".to_owned(), Some(StyleValue::Number(240.0)));
    patch.insert(
        "background_color".to_owned(),
        Some(StyleValue::Text("black".to_owned())),
    );
    patch.insert("__hover_scope".to_owned(), Some(StyleValue::Bool(true)));
    patch.insert(
        "source_intent".to_owned(),
        Some(StyleValue::Text("activate".to_owned())),
    );

    let report = state
        .apply_patch(DocumentPatch::SetStyle {
            id: DocumentNodeId("label".to_owned()),
            patch,
        })
        .unwrap();

    for class in [
        PatchInvalidationClass::Style,
        PatchInvalidationClass::Layout,
        PatchInvalidationClass::LayoutOnly,
        PatchInvalidationClass::PaintOnly,
        PatchInvalidationClass::ConditionalStructure,
        PatchInvalidationClass::SourceBinding,
        PatchInvalidationClass::HitRegion,
    ] {
        assert!(
            report.invalidation.contains(&class),
            "missing invalidation class {class:?}"
        );
    }
}


#[test]
fn style_patch_unknown_keys_fail_toward_full_document_invalidation() {
    let mut state = DocumentState::new("root");
    state
        .apply_patch(DocumentPatch::UpsertNode(node(
            "label",
            DocumentNodeKind::Text,
            Some("root"),
        )))
        .unwrap();

    let mut patch = StylePatch::new();
    patch.insert(
        "future_renderer_knob".to_owned(),
        Some(StyleValue::Text("unknown".to_owned())),
    );

    let report = state
        .apply_patch(DocumentPatch::SetStyle {
            id: DocumentNodeId("label".to_owned()),
            patch,
        })
        .unwrap();

    assert!(
        report
            .invalidation
            .contains(&PatchInvalidationClass::FullDocument),
        "unknown style keys must invalidate conservatively"
    );
}


