#[test]
fn document_patch_missing_targets_fail_closed() {
    let mut state = DocumentState::new("root");

    let text_error = state
        .apply_patch(DocumentPatch::SetText {
            id: DocumentNodeId("missing".to_owned()),
            text: TextValue {
                text: "Lost".to_owned(),
            },
        })
        .unwrap_err();
    assert!(matches!(
        text_error,
        PatchApplyError::MissingTarget {
            patch_kind: "set_text",
            id
        } if id.0 == "missing"
    ));

    let style_error = state
        .apply_patch(DocumentPatch::SetStyle {
            id: DocumentNodeId("missing".to_owned()),
            patch: StylePatch::new(),
        })
        .unwrap_err();
    assert!(matches!(
        style_error,
        PatchApplyError::MissingTarget {
            patch_kind: "set_style",
            id
        } if id.0 == "missing"
    ));

    let materialized_error = state
        .apply_patch(DocumentPatch::SetListMaterialization {
            id: DocumentNodeId("missing".to_owned()),
            materialized: MaterializedRange {
                axis: Axis::Vertical,
                visible: 0..1,
                overscan: 0..2,
            },
        })
        .unwrap_err();
    assert!(matches!(
        materialized_error,
        PatchApplyError::MissingTarget {
            patch_kind: "set_list_materialization",
            id
        } if id.0 == "missing"
    ));
}


#[test]
fn owned_frame_batch_matches_stateful_batch_patch_result() {
    let mut initial = DocumentState::new("root");
    initial
        .apply_patch(DocumentPatch::UpsertNode(node(
            "title",
            DocumentNodeKind::Text,
            Some("root"),
        )))
        .unwrap();
    let batch = DocumentChangeBatch {
        patches: vec![
            DocumentPatch::SetText {
                id: DocumentNodeId("title".to_owned()),
                text: TextValue {
                    text: "Ready".to_owned(),
                },
            },
            DocumentPatch::SetStyle {
                id: DocumentNodeId("title".to_owned()),
                patch: BTreeMap::from([(
                    "color".to_owned(),
                    Some(StyleValue::Text("green".to_owned())),
                )]),
            },
        ],
    };

    let mut stateful = DocumentState::from_frame(initial.frame().clone()).unwrap();
    let stateful_change_set = stateful.apply_batch(batch.clone()).unwrap();
    let (owned_frame, owned_change_set) =
        DocumentState::apply_batch_to_owned_frame(initial.into_frame(), batch).unwrap();

    assert_eq!(owned_frame, stateful.into_frame());
    assert_eq!(owned_change_set, stateful_change_set);
}


#[test]
fn trusted_nonstructural_owned_frame_batch_matches_stateful_batch_patch_result() {
    let mut initial = DocumentState::new("root");
    initial
        .apply_patch(DocumentPatch::UpsertNode(node(
            "title",
            DocumentNodeKind::Text,
            Some("root"),
        )))
        .unwrap();
    let batch = DocumentChangeBatch {
        patches: vec![
            DocumentPatch::SetText {
                id: DocumentNodeId("title".to_owned()),
                text: TextValue {
                    text: "Ready".to_owned(),
                },
            },
            DocumentPatch::SetStyle {
                id: DocumentNodeId("title".to_owned()),
                patch: BTreeMap::from([(
                    "color".to_owned(),
                    Some(StyleValue::Text("green".to_owned())),
                )]),
            },
        ],
    };

    let mut stateful = DocumentState::from_frame(initial.frame().clone()).unwrap();
    let stateful_change_set = stateful.apply_batch(batch.clone()).unwrap();
    let (owned_frame, owned_change_set) =
        DocumentState::apply_nonstructural_batch_to_valid_owned_frame(initial.into_frame(), batch)
            .unwrap();

    assert_eq!(owned_frame, stateful.into_frame());
    assert_eq!(owned_change_set, stateful_change_set);
}


#[test]
fn trusted_nonstructural_owned_frame_batch_rejects_structural_patch() {
    let initial = DocumentState::new("root");
    let error = DocumentState::apply_nonstructural_batch_to_valid_owned_frame(
        initial.into_frame(),
        DocumentChangeBatch {
            patches: vec![DocumentPatch::UpsertNode(node(
                "title",
                DocumentNodeKind::Text,
                Some("root"),
            ))],
        },
    )
    .unwrap_err();

    assert!(matches!(
        error,
        PatchApplyError::UnsupportedTrustedNonstructuralPatch {
            patch_kind: "upsert_node"
        }
    ));
}


