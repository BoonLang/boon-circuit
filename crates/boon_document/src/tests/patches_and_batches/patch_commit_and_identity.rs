// Included by `../tests.rs`; kept in the parent test module for private document helper access.

#[test]
fn document_patch_reports_text_and_layout_invalidation() {
    let mut state = DocumentState::new("root");
    state
        .apply_patch(DocumentPatch::UpsertNode(node(
            "label",
            DocumentNodeKind::Text,
            Some("root"),
        )))
        .unwrap();

    let report = state
        .apply_patch(DocumentPatch::SetText {
            id: DocumentNodeId("label".to_owned()),
            text: TextValue {
                text: "Updated".to_owned(),
            },
        })
        .unwrap();

    assert_eq!(report.patch_kind, "set_text");
    assert_eq!(report.target, Some(DocumentNodeId("label".to_owned())));
    assert!(report.invalidation.contains(&PatchInvalidationClass::Text));
    assert!(
        report
            .invalidation
            .contains(&PatchInvalidationClass::Layout)
    );
    assert!(
        report
            .invalidation
            .contains(&PatchInvalidationClass::HitRegion)
    );
    assert_eq!(report.node_count_after, 2);
    assert_eq!(
        state.frame().nodes[&DocumentNodeId("label".to_owned())]
            .text
            .as_ref()
            .unwrap()
            .text,
        "Updated"
    );
}


#[test]
fn document_batch_commits_atomically_and_merges_dirty_facts() {
    let mut state = DocumentState::new("root");
    let mut style = StylePatch::new();
    style.insert(
        "color".to_owned(),
        Some(StyleValue::Text("blue".to_owned())),
    );

    let change_set = state
        .apply_batch(DocumentChangeBatch {
            patches: vec![
                DocumentPatch::UpsertNode(node("label", DocumentNodeKind::Text, Some("root"))),
                DocumentPatch::SetText {
                    id: DocumentNodeId("label".to_owned()),
                    text: TextValue {
                        text: "Ready".to_owned(),
                    },
                },
                DocumentPatch::SetStyle {
                    id: DocumentNodeId("label".to_owned()),
                    patch: style,
                },
            ],
        })
        .unwrap();

    assert_eq!(change_set.patch_count, 3);
    assert_eq!(change_set.node_count_before, 1);
    assert_eq!(change_set.node_count_after, 2);
    assert_eq!(change_set.targets, vec![DocumentNodeId("label".to_owned())]);
    for class in [
        PatchInvalidationClass::Structure,
        PatchInvalidationClass::Text,
        PatchInvalidationClass::Style,
        PatchInvalidationClass::Layout,
        PatchInvalidationClass::PaintOnly,
        PatchInvalidationClass::HitRegion,
        PatchInvalidationClass::FullDocument,
    ] {
        assert!(
            change_set.invalidation.contains(&class),
            "missing merged invalidation class {class:?}"
        );
    }
    assert_eq!(
        state.frame().nodes[&DocumentNodeId("label".to_owned())]
            .text
            .as_ref()
            .unwrap()
            .text,
        "Ready"
    );
}


#[test]
fn document_batch_rolls_back_when_later_patch_fails() {
    let mut state = DocumentState::new("root");
    let error = state
        .apply_batch(DocumentChangeBatch {
            patches: vec![
                DocumentPatch::UpsertNode(node("label", DocumentNodeKind::Text, Some("root"))),
                DocumentPatch::SetText {
                    id: DocumentNodeId("missing".to_owned()),
                    text: TextValue {
                        text: "Should not commit".to_owned(),
                    },
                },
            ],
        })
        .unwrap_err();

    assert!(matches!(
        error,
        PatchApplyError::MissingTarget {
            patch_kind: "set_text",
            id
        } if id.0 == "missing"
    ));
    assert!(
        !state
            .frame()
            .nodes
            .contains_key(&DocumentNodeId("label".to_owned())),
        "the successful first patch must not commit when a later patch fails"
    );
    assert_eq!(state.frame().nodes.len(), 1);
}


#[test]
fn document_hot_id_table_is_numeric_stable_and_debuggable() {
    let mut state = DocumentState::new("root");
    state
        .apply_batch(DocumentChangeBatch {
            patches: vec![
                DocumentPatch::UpsertNode(node("zeta", DocumentNodeKind::Text, Some("root"))),
                DocumentPatch::UpsertNode(node("alpha", DocumentNodeKind::Text, Some("root"))),
                DocumentPatch::UpsertNode(node("panel", DocumentNodeKind::Stack, Some("root"))),
            ],
        })
        .unwrap();

    let table = DocumentHotIdTable::from_frame(state.frame()).unwrap();
    assert_eq!(table.root, DocumentHotNodeId(0));
    assert_eq!(
        table.hot_id(&DocumentNodeId("root".to_owned())),
        Some(DocumentHotNodeId(0))
    );
    assert_eq!(
        table.hot_id(&DocumentNodeId("alpha".to_owned())),
        Some(DocumentHotNodeId(1)),
        "non-root IDs should be assigned deterministically by stable node ID"
    );
    assert_eq!(
        table.debug_name(DocumentHotNodeId(3)),
        Some(&DocumentNodeId("zeta".to_owned()))
    );
    assert_eq!(table.root, DocumentHotNodeId(0));
    assert_eq!(
        table.debug_names.node_names.get(&DocumentHotNodeId(0)),
        Some(&DocumentNodeId("root".to_owned()))
    );
}


#[test]
fn document_hot_id_table_carries_ids_and_generations_across_frames() {
    let mut state = DocumentState::new("root");
    state
        .apply_batch(DocumentChangeBatch {
            patches: vec![
                DocumentPatch::UpsertNode(node("zeta", DocumentNodeKind::Text, Some("root"))),
                DocumentPatch::UpsertNode(node("alpha", DocumentNodeKind::Text, Some("root"))),
            ],
        })
        .unwrap();
    let previous_frame = state.frame().clone();
    let previous_table = DocumentHotIdTable::from_frame(&previous_frame).unwrap();
    let root_ref = previous_table
        .hot_ref(&DocumentNodeId("root".to_owned()))
        .unwrap();
    let alpha_ref = previous_table
        .hot_ref(&DocumentNodeId("alpha".to_owned()))
        .unwrap();
    let zeta_ref = previous_table
        .hot_ref(&DocumentNodeId("zeta".to_owned()))
        .unwrap();

    state
        .apply_batch(DocumentChangeBatch {
            patches: vec![
                DocumentPatch::SetText {
                    id: DocumentNodeId("alpha".to_owned()),
                    text: TextValue {
                        text: "changed".to_owned(),
                    },
                },
                DocumentPatch::RemoveNode {
                    id: DocumentNodeId("zeta".to_owned()),
                },
                DocumentPatch::UpsertNode(node("beta", DocumentNodeKind::Button, Some("root"))),
            ],
        })
        .unwrap();

    let next_table =
        DocumentHotIdTable::from_previous_frames(&previous_table, &previous_frame, state.frame())
            .unwrap();
    let next_root_ref = next_table
        .hot_ref(&DocumentNodeId("root".to_owned()))
        .unwrap();
    let next_alpha_ref = next_table
        .hot_ref(&DocumentNodeId("alpha".to_owned()))
        .unwrap();
    let beta_ref = next_table
        .hot_ref(&DocumentNodeId("beta".to_owned()))
        .unwrap();

    assert_eq!(next_root_ref.id, root_ref.id);
    assert_eq!(
        next_root_ref.generation,
        DocumentHotNodeGeneration(root_ref.generation.0 + 1)
    );
    assert_eq!(next_alpha_ref.id, alpha_ref.id);
    assert_eq!(
        next_alpha_ref.generation,
        DocumentHotNodeGeneration(alpha_ref.generation.0 + 1)
    );
    assert!(beta_ref.id.0 >= previous_table.next_id);
    assert_eq!(beta_ref.generation, DocumentHotNodeGeneration(1));
    assert_eq!(next_table.hot_id(&DocumentNodeId("zeta".to_owned())), None);
    assert_eq!(next_table.debug_name(zeta_ref.id), None);
}


#[test]
fn document_intern_index_deduplicates_text_styles_materials_clips_and_bindings() {
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
    alpha
        .style
        .insert("material".to_owned(), StyleValue::Text("flat".to_owned()));
    alpha.style.insert(
        "__clip_rect".to_owned(),
        StyleValue::Text("viewport".to_owned()),
    );
    alpha.set_primary_source_binding(boon_document_model::SourceBinding {
        id: SourceBindingId("title-binding".to_owned()),
        source_path: "todos[0].title".to_owned(),
        intent: "edit".to_owned(),
    });

    let mut beta = node("beta", DocumentNodeKind::Text, Some("root"));
    beta.text = Some(TextValue {
        text: "shared".to_owned(),
    });
    beta.style = alpha.style.clone();
    beta.style
        .insert("color".to_owned(), StyleValue::Text("blue".to_owned()));
    beta.source_bindings = alpha.source_bindings.clone();

    let mut state = DocumentState::new("root");
    state
        .apply_batch(DocumentChangeBatch {
            patches: vec![
                DocumentPatch::UpsertNode(alpha),
                DocumentPatch::UpsertNode(beta),
            ],
        })
        .unwrap();

    let hot_ids = DocumentHotIdTable::from_frame(state.frame()).unwrap();
    let index = DocumentInternIndex::from_frame(state.frame(), &hot_ids).unwrap();
    let alpha_hot = hot_ids.hot_id(&DocumentNodeId("alpha".to_owned())).unwrap();
    let beta_hot = hot_ids.hot_id(&DocumentNodeId("beta".to_owned())).unwrap();
    let alpha_refs = index.nodes.get(&alpha_hot).unwrap();
    let beta_refs = index.nodes.get(&beta_hot).unwrap();

    assert_eq!(alpha_refs.text, beta_refs.text);
    assert_eq!(index.texts.keys_by_id.len(), 1);
    assert_eq!(alpha_refs.layout_style, beta_refs.layout_style);
    assert_ne!(alpha_refs.paint_style, beta_refs.paint_style);
    assert_eq!(alpha_refs.material, beta_refs.material);
    assert_eq!(alpha_refs.clip, beta_refs.clip);
    assert_eq!(alpha_refs.source_bindings, beta_refs.source_bindings);
    assert_eq!(index.source_bindings.keys_by_id.len(), 1);

    let previous_hot_ids = DocumentHotIdTable::from_frame(&DocumentFrame::empty("root")).unwrap();
    let err = DocumentInternIndex::from_frame(state.frame(), &previous_hot_ids).unwrap_err();
    assert!(matches!(
        err,
        PatchApplyError::StaleReference {
            reference_kind: "hot_id_table",
            ..
        }
    ));
}

