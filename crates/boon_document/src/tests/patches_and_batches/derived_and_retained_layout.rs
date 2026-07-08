#[test]
fn derived_index_bundle_incrementally_updates_nonstructural_nodes() {
    let mut alpha = node("alpha", DocumentNodeKind::Text, Some("root"));
    alpha.text = Some(TextValue {
        text: "before".to_owned(),
    });
    alpha
        .style
        .insert("width".to_owned(), StyleValue::Number(120.0));
    alpha.set_primary_source_binding(boon_document_model::SourceBinding {
        id: SourceBindingId("alpha-binding".to_owned()),
        source_path: "store.before".to_owned(),
        intent: "edit".to_owned(),
    });

    let mut state = DocumentState::new("root");
    state.apply_patch(DocumentPatch::UpsertNode(alpha)).unwrap();
    let previous_bundle = DocumentDerivedIndexBundle::from_frame(state.frame()).unwrap();
    let alpha_node = DocumentNodeId("alpha".to_owned());
    let alpha_hot = previous_bundle.hot_ids.hot_id(&alpha_node).unwrap();

    state
        .apply_batch(DocumentChangeBatch {
            patches: vec![
                DocumentPatch::SetText {
                    id: alpha_node.clone(),
                    text: TextValue {
                        text: "after".to_owned(),
                    },
                },
                DocumentPatch::SetStyle {
                    id: alpha_node.clone(),
                    patch: BTreeMap::from([("width".to_owned(), Some(StyleValue::Number(180.0)))]),
                },
                DocumentPatch::SetBinding {
                    id: alpha_node.clone(),
                    binding: boon_document_model::SourceBinding {
                        id: SourceBindingId("alpha-binding".to_owned()),
                        source_path: "store.after".to_owned(),
                        intent: "edit".to_owned(),
                    },
                },
            ],
        })
        .unwrap();

    let changed_nodes = BTreeSet::from([alpha_node]);
    let incremental = DocumentDerivedIndexBundle::from_previous_nonstructural_patch(
        &previous_bundle,
        state.frame(),
        &changed_nodes,
    )
    .unwrap();
    let full = DocumentDerivedIndexBundle::from_frame(state.frame()).unwrap();
    let after_route = DocumentTypedBindingRoute {
        source_path: "store.after".to_owned(),
        intent: "edit".to_owned(),
    };
    let before_route = DocumentTypedBindingRoute {
        source_path: "store.before".to_owned(),
        intent: "edit".to_owned(),
    };

    assert_eq!(
        incremental
            .hot_ids
            .hot_id(&DocumentNodeId("alpha".to_owned())),
        Some(alpha_hot)
    );
    let incremental_key = &incremental
        .retained_layout_keys
        .entry(alpha_hot)
        .unwrap()
        .key;
    let full_key = &full.retained_layout_keys.entry(alpha_hot).unwrap().key;
    assert_eq!(incremental_key.kind, full_key.kind);
    assert_eq!(incremental_key.children, full_key.children);
    assert_eq!(incremental_key.materialized, full_key.materialized);
    assert_eq!(
        incremental
            .intern_index
            .layout_styles
            .key(incremental_key.layout_style),
        full.intern_index.layout_styles.key(full_key.layout_style)
    );
    assert_eq!(
        incremental
            .intern_index
            .text_styles
            .key(incremental_key.text_style),
        full.intern_index.text_styles.key(full_key.text_style)
    );
    assert_eq!(
        incremental_key
            .text
            .and_then(|id| incremental.intern_index.texts.key(id)),
        full_key.text.and_then(|id| full.intern_index.texts.key(id))
    );
    assert_eq!(
        incremental.typed_styles.record(alpha_hot),
        full.typed_styles.record(alpha_hot)
    );
    assert_eq!(
        incremental.typed_bindings.refs_for_route(&after_route),
        full.typed_bindings.refs_for_route(&after_route)
    );
    assert!(
        incremental
            .typed_bindings
            .refs_for_route(&before_route)
            .is_empty()
    );
}


#[test]
fn retained_layout_keys_ignore_paint_only_changes_but_track_layout_inputs() {
    let mut alpha = node("alpha", DocumentNodeKind::Text, Some("root"));
    alpha.text = Some(TextValue {
        text: "shared".to_owned(),
    });
    alpha
        .style
        .insert("width".to_owned(), StyleValue::Number(120.0));
    alpha
        .style
        .insert("color".to_owned(), StyleValue::Text("red".to_owned()));

    let mut state = DocumentState::new("root");
    state.apply_patch(DocumentPatch::UpsertNode(alpha)).unwrap();
    let initial_frame = state.frame().clone();
    let initial_hot = DocumentHotIdTable::from_frame(&initial_frame).unwrap();
    let initial_intern = DocumentInternIndex::from_frame(&initial_frame, &initial_hot).unwrap();
    let initial_keys =
        DocumentRetainedLayoutKeyTable::from_frame(&initial_frame, &initial_hot, &initial_intern)
            .unwrap();
    let alpha_hot = initial_hot
        .hot_id(&DocumentNodeId("alpha".to_owned()))
        .unwrap();
    let initial_alpha = initial_keys.entry(alpha_hot).unwrap().clone();

    state
        .apply_patch(DocumentPatch::SetStyle {
            id: DocumentNodeId("alpha".to_owned()),
            patch: BTreeMap::from([(
                "color".to_owned(),
                Some(StyleValue::Text("blue".to_owned())),
            )]),
        })
        .unwrap();
    let paint_frame = state.frame().clone();
    let paint_hot =
        DocumentHotIdTable::from_previous_frames(&initial_hot, &initial_frame, &paint_frame)
            .unwrap();
    let paint_intern =
        DocumentInternIndex::from_previous_frame(&initial_intern, &paint_frame, &paint_hot)
            .unwrap();
    let paint_keys =
        DocumentRetainedLayoutKeyTable::from_frame(&paint_frame, &paint_hot, &paint_intern)
            .unwrap();
    let paint_alpha = paint_keys.entry(alpha_hot).unwrap();

    assert_eq!(paint_alpha.node.id, initial_alpha.node.id);
    assert_ne!(paint_alpha.node.generation, initial_alpha.node.generation);
    assert_eq!(
        paint_alpha.key, initial_alpha.key,
        "paint-only style changes must not invalidate the retained layout key"
    );
    let paint_delta = paint_keys.diff_from(&initial_keys);
    assert!(
        paint_delta.reused.iter().any(|entry| entry.id == alpha_hot),
        "paint-only changes should reuse the retained layout entry"
    );
    assert!(
        paint_delta
            .dirty
            .iter()
            .all(|entry| entry.node != alpha_hot),
        "paint-only changes should not dirty the retained layout entry"
    );

    state
        .apply_patch(DocumentPatch::SetStyle {
            id: DocumentNodeId("alpha".to_owned()),
            patch: BTreeMap::from([("width".to_owned(), Some(StyleValue::Number(180.0)))]),
        })
        .unwrap();
    let layout_frame = state.frame().clone();
    let layout_hot =
        DocumentHotIdTable::from_previous_frames(&paint_hot, &paint_frame, &layout_frame).unwrap();
    let layout_intern =
        DocumentInternIndex::from_previous_frame(&paint_intern, &layout_frame, &layout_hot)
            .unwrap();
    let layout_keys =
        DocumentRetainedLayoutKeyTable::from_frame(&layout_frame, &layout_hot, &layout_intern)
            .unwrap();

    assert_ne!(
        layout_keys.entry(alpha_hot).unwrap().key,
        initial_alpha.key,
        "layout-affecting style changes must update the retained layout key"
    );
    let layout_delta = layout_keys.diff_from(&paint_keys);
    let layout_dirty = layout_delta
        .dirty
        .iter()
        .find(|entry| entry.node == alpha_hot)
        .expect("layout-affecting style change should dirty alpha");
    assert_eq!(
        layout_dirty.reasons,
        vec![DocumentRetainedLayoutDirtyReason::LayoutStyle]
    );

    state
        .apply_patch(DocumentPatch::UpsertNode(node(
            "child",
            DocumentNodeKind::Button,
            Some("alpha"),
        )))
        .unwrap();
    let child_frame = state.frame().clone();
    let child_hot =
        DocumentHotIdTable::from_previous_frames(&layout_hot, &layout_frame, &child_frame).unwrap();
    let child_intern =
        DocumentInternIndex::from_previous_frame(&layout_intern, &child_frame, &child_hot).unwrap();
    let child_keys =
        DocumentRetainedLayoutKeyTable::from_frame(&child_frame, &child_hot, &child_intern)
            .unwrap();
    let child_id = child_hot
        .hot_id(&DocumentNodeId("child".to_owned()))
        .unwrap();
    assert!(
        child_keys
            .entry(alpha_hot)
            .unwrap()
            .key
            .children
            .contains(&child_id),
        "structural child changes must be represented in the retained layout key"
    );
    let child_delta = child_keys.diff_from(&layout_keys);
    let child_dirty = child_delta
        .dirty
        .iter()
        .find(|entry| entry.node == alpha_hot)
        .expect("child insertion should dirty the parent layout entry");
    assert_eq!(
        child_dirty.reasons,
        vec![DocumentRetainedLayoutDirtyReason::Children]
    );
    let child_added = child_delta
        .dirty
        .iter()
        .find(|entry| entry.node == child_id)
        .expect("new child should be an added layout entry");
    assert_eq!(
        child_added.reasons,
        vec![DocumentRetainedLayoutDirtyReason::Added]
    );

    state
        .apply_patch(DocumentPatch::RemoveNode {
            id: DocumentNodeId("child".to_owned()),
        })
        .unwrap();
    let removed_frame = state.frame().clone();
    let removed_hot =
        DocumentHotIdTable::from_previous_frames(&child_hot, &child_frame, &removed_frame).unwrap();
    let removed_intern =
        DocumentInternIndex::from_previous_frame(&child_intern, &removed_frame, &removed_hot)
            .unwrap();
    let removed_keys =
        DocumentRetainedLayoutKeyTable::from_frame(&removed_frame, &removed_hot, &removed_intern)
            .unwrap();
    let removed_delta = removed_keys.diff_from(&child_keys);
    let removed_child = removed_delta
        .removed
        .iter()
        .find(|entry| entry.node == child_id)
        .expect("removed child should be reported as removed");
    assert_eq!(
        removed_child.reasons,
        vec![DocumentRetainedLayoutDirtyReason::Removed]
    );

    let stale_err =
        DocumentRetainedLayoutKeyTable::from_frame(&child_frame, &initial_hot, &initial_intern)
            .unwrap_err();
    assert!(matches!(
        stale_err,
        PatchApplyError::StaleReference {
            reference_kind: "document_intern_index" | "hot_id_table" | "hot_id_table_child",
            ..
        }
    ));
}


#[test]
fn retained_layout_cache_reuses_paint_only_geometry_and_refreshes_layout_dirty_nodes() {
    let mut alpha = node("alpha", DocumentNodeKind::Text, Some("root"));
    alpha.text = Some(TextValue {
        text: "shared".to_owned(),
    });
    alpha
        .style
        .insert("width".to_owned(), StyleValue::Number(120.0));
    alpha
        .style
        .insert("color".to_owned(), StyleValue::Text("red".to_owned()));

    let mut state = DocumentState::new("root");
    state.apply_patch(DocumentPatch::UpsertNode(alpha)).unwrap();

    let initial_frame = state.frame().clone();
    let initial_hot = DocumentHotIdTable::from_frame(&initial_frame).unwrap();
    let initial_intern = DocumentInternIndex::from_frame(&initial_frame, &initial_hot).unwrap();
    let initial_keys =
        DocumentRetainedLayoutKeyTable::from_frame(&initial_frame, &initial_hot, &initial_intern)
            .unwrap();
    let mut text = SimpleTextMeasurer;
    let initial_layout = layout(LayoutInput {
        document: &initial_frame,
        viewport: Viewport {
            surface: 1,
            width: 500.0,
            height: 300.0,
            scale: 1.0,
        },
        text: &mut text,
        capabilities: RenderCapabilities::fake_portable(),
    });
    let initial_cache = DocumentRetainedLayoutCache::from_layout_frame(
        &initial_frame,
        &initial_hot,
        &initial_keys,
        &initial_layout,
    )
    .unwrap();
    let alpha_hot = initial_hot
        .hot_id(&DocumentNodeId("alpha".to_owned()))
        .unwrap();
    let initial_geometry = initial_cache
        .entries
        .get(&alpha_hot)
        .unwrap()
        .geometry
        .clone();

    state
        .apply_patch(DocumentPatch::SetStyle {
            id: DocumentNodeId("alpha".to_owned()),
            patch: BTreeMap::from([(
                "color".to_owned(),
                Some(StyleValue::Text("blue".to_owned())),
            )]),
        })
        .unwrap();
    let paint_frame = state.frame().clone();
    let paint_hot =
        DocumentHotIdTable::from_previous_frames(&initial_hot, &initial_frame, &paint_frame)
            .unwrap();
    let paint_intern =
        DocumentInternIndex::from_previous_frame(&initial_intern, &paint_frame, &paint_hot)
            .unwrap();
    let paint_keys =
        DocumentRetainedLayoutKeyTable::from_frame(&paint_frame, &paint_hot, &paint_intern)
            .unwrap();
    let mut text = SimpleTextMeasurer;
    let paint_layout = layout(LayoutInput {
        document: &paint_frame,
        viewport: Viewport {
            surface: 1,
            width: 500.0,
            height: 300.0,
            scale: 1.0,
        },
        text: &mut text,
        capabilities: RenderCapabilities::fake_portable(),
    });
    let paint_update = initial_cache
        .update_from_layout_frame(&paint_frame, &paint_hot, &paint_keys, &paint_layout)
        .unwrap();
    assert!(
        paint_update.refreshed.is_empty(),
        "paint-only changes should not refresh retained layout geometry"
    );
    assert_eq!(
        paint_update.cache.entries.get(&alpha_hot).unwrap().geometry,
        initial_geometry
    );
    assert_eq!(paint_update.patch.operations.len(), 1);
    assert!(matches!(
        &paint_update.patch.operations[0],
        DocumentRetainedLayoutPatchOperation::ReuseGeometry { node }
            if node.id == alpha_hot
    ));

    state
        .apply_patch(DocumentPatch::SetStyle {
            id: DocumentNodeId("alpha".to_owned()),
            patch: BTreeMap::from([("width".to_owned(), Some(StyleValue::Number(180.0)))]),
        })
        .unwrap();
    let layout_frame = state.frame().clone();
    let layout_hot =
        DocumentHotIdTable::from_previous_frames(&paint_hot, &paint_frame, &layout_frame).unwrap();
    let layout_intern =
        DocumentInternIndex::from_previous_frame(&paint_intern, &layout_frame, &layout_hot)
            .unwrap();
    let layout_keys =
        DocumentRetainedLayoutKeyTable::from_frame(&layout_frame, &layout_hot, &layout_intern)
            .unwrap();
    let mut text = SimpleTextMeasurer;
    let measured_layout = layout(LayoutInput {
        document: &layout_frame,
        viewport: Viewport {
            surface: 1,
            width: 500.0,
            height: 300.0,
            scale: 1.0,
        },
        text: &mut text,
        capabilities: RenderCapabilities::fake_portable(),
    });
    let layout_update = paint_update
        .cache
        .update_from_layout_frame(&layout_frame, &layout_hot, &layout_keys, &measured_layout)
        .unwrap();
    assert!(
        layout_update
            .refreshed
            .iter()
            .any(|entry| entry.id == alpha_hot),
        "layout-affecting changes should refresh retained layout geometry"
    );
    let upsert = layout_update
        .patch
        .operations
        .iter()
        .find_map(|operation| match operation {
            DocumentRetainedLayoutPatchOperation::UpsertGeometry {
                node,
                geometry,
                reasons,
                ..
            } if node.id == alpha_hot => Some((geometry, reasons)),
            _ => None,
        })
        .expect("layout-affecting update should emit an upsert geometry patch");
    assert_eq!(upsert.0.bounds.width, 180.0);
    assert_eq!(
        upsert.1,
        &vec![DocumentRetainedLayoutDirtyReason::LayoutStyle]
    );
    assert_eq!(
        layout_update
            .cache
            .entries
            .get(&alpha_hot)
            .unwrap()
            .geometry
            .bounds
            .width,
        180.0
    );
}


