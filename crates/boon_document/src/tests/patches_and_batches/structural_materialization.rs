#[test]
fn materialization_patch_reports_logical_counts_and_stable_keys() {
    let mut state = DocumentState::new("root");
    state
        .apply_patch(DocumentPatch::UpsertNode(node(
            "virtual-list",
            DocumentNodeKind::ScrollRoot,
            Some("root"),
        )))
        .unwrap();

    let report = state
        .apply_patch(DocumentPatch::SetListMaterialization {
            id: DocumentNodeId("virtual-list".to_owned()),
            materialized: MaterializedRange {
                materialization: Some(1),
                axis: Axis::Vertical,
                visible: 10..20,
                overscan: 8..24,
                logical_item_count: 24,
            },
        })
        .unwrap();
    let materialization = report
        .materialization
        .expect("materialization patch should report protocol metadata");

    assert_eq!(materialization.node.0, "virtual-list");
    assert_eq!(materialization.visible, 10..20);
    assert_eq!(materialization.overscan, 8..24);
    assert_eq!(materialization.logical_item_count, 24);
    assert_eq!(materialization.materialized_item_count, 16);
    assert_eq!(
        materialization.stable_key_prefix,
        "materialized:virtual-list:y"
    );
    assert_eq!(
        materialization.first_stable_key.as_deref(),
        Some("materialized:virtual-list:y:8")
    );
    assert_eq!(
        materialization.last_stable_key.as_deref(),
        Some("materialized:virtual-list:y:23")
    );
    assert!(
        report
            .invalidation
            .contains(&PatchInvalidationClass::Materialization)
    );
}


#[test]
fn document_upsert_rejects_orphaned_children_and_bad_parent_links() {
    let mut state = DocumentState::new("root");
    let mut parent = node("parent", DocumentNodeKind::Stack, Some("root"));
    parent
        .children
        .push(DocumentNodeId("missing-child".to_owned()));
    let orphan_error = state
        .apply_patch(DocumentPatch::UpsertNode(parent))
        .unwrap_err();
    assert!(matches!(
        orphan_error,
        PatchApplyError::OrphanedChild { parent, child }
            if parent.0 == "parent" && child.0 == "missing-child"
    ));

    state
        .apply_patch(DocumentPatch::UpsertNode(node(
            "parent",
            DocumentNodeKind::Stack,
            Some("root"),
        )))
        .unwrap();
    state
        .apply_patch(DocumentPatch::UpsertNode(node(
            "child",
            DocumentNodeKind::Text,
            Some("root"),
        )))
        .unwrap();
    let mut parent = node("parent", DocumentNodeKind::Stack, Some("root"));
    parent.children.push(DocumentNodeId("child".to_owned()));
    let link_error = state
        .apply_patch(DocumentPatch::UpsertNode(parent))
        .unwrap_err();
    assert!(matches!(
        link_error,
        PatchApplyError::InvalidParentChildLink {
            parent,
            child,
            actual_parent: Some(actual_parent),
        } if parent.0 == "parent" && child.0 == "child" && actual_parent.0 == "root"
    ));
}


#[test]
fn document_remove_node_removes_subtree_and_detaches_parent() {
    let mut state = DocumentState::new("root");
    state
        .apply_patch(DocumentPatch::UpsertNode(node(
            "panel",
            DocumentNodeKind::Stack,
            Some("root"),
        )))
        .unwrap();
    state
        .apply_patch(DocumentPatch::UpsertNode(node(
            "label",
            DocumentNodeKind::Text,
            Some("panel"),
        )))
        .unwrap();

    let report = state
        .apply_patch(DocumentPatch::RemoveNode {
            id: DocumentNodeId("panel".to_owned()),
        })
        .unwrap();

    assert_eq!(report.patch_kind, "remove_node");
    assert_eq!(
        report.removed_nodes,
        vec![
            DocumentNodeId("panel".to_owned()),
            DocumentNodeId("label".to_owned())
        ]
    );
    assert!(
        report
            .invalidation
            .contains(&PatchInvalidationClass::Structure)
    );
    assert!(
        report
            .invalidation
            .contains(&PatchInvalidationClass::HitRegion)
    );
    assert!(
        !state
            .frame()
            .nodes
            .contains_key(&DocumentNodeId("panel".to_owned()))
    );
    assert!(
        !state
            .frame()
            .nodes
            .contains_key(&DocumentNodeId("label".to_owned()))
    );
    assert!(
        state.frame().nodes[&DocumentNodeId("root".to_owned())]
            .children
            .is_empty()
    );
}


#[test]
fn structural_child_patches_reorder_move_and_remove_precisely() {
    let mut state = DocumentState::new("root");
    for (id, kind, parent) in [
        ("left", DocumentNodeKind::Stack, "root"),
        ("right", DocumentNodeKind::Stack, "root"),
        ("a", DocumentNodeKind::Text, "left"),
        ("b", DocumentNodeKind::Text, "left"),
        ("c", DocumentNodeKind::Text, "left"),
        ("nested", DocumentNodeKind::Text, "c"),
    ] {
        state
            .apply_patch(DocumentPatch::UpsertNode(node(id, kind, Some(parent))))
            .unwrap();
    }

    let reorder = state
        .apply_patch(DocumentPatch::InsertChild {
            parent: DocumentNodeId("left".to_owned()),
            child: DocumentNodeId("c".to_owned()),
            index: 0,
        })
        .unwrap();
    assert_eq!(reorder.patch_kind, "insert_child");
    assert_eq!(
        state.frame().nodes[&DocumentNodeId("left".to_owned())].children,
        vec![
            DocumentNodeId("c".to_owned()),
            DocumentNodeId("a".to_owned()),
            DocumentNodeId("b".to_owned()),
        ]
    );
    assert!(
        reorder
            .invalidation
            .contains(&PatchInvalidationClass::Structure)
    );
    assert!(
        !reorder
            .invalidation
            .contains(&PatchInvalidationClass::FullDocument),
        "precise child reorders should not force full-document invalidation"
    );

    let moved = state
        .apply_patch(DocumentPatch::MoveChild {
            child: DocumentNodeId("b".to_owned()),
            new_parent: DocumentNodeId("right".to_owned()),
            index: 0,
        })
        .unwrap();
    assert_eq!(moved.patch_kind, "move_child");
    assert_eq!(
        state.frame().nodes[&DocumentNodeId("b".to_owned())].parent,
        Some(DocumentNodeId("right".to_owned()))
    );
    assert_eq!(
        state.frame().nodes[&DocumentNodeId("right".to_owned())].children,
        vec![DocumentNodeId("b".to_owned())]
    );
    assert_eq!(
        state.frame().nodes[&DocumentNodeId("left".to_owned())].children,
        vec![
            DocumentNodeId("c".to_owned()),
            DocumentNodeId("a".to_owned())
        ]
    );

    let removed = state
        .apply_patch(DocumentPatch::RemoveChild {
            parent: DocumentNodeId("left".to_owned()),
            child: DocumentNodeId("c".to_owned()),
        })
        .unwrap();
    assert_eq!(removed.patch_kind, "remove_child");
    assert_eq!(
        removed.removed_nodes,
        vec![
            DocumentNodeId("c".to_owned()),
            DocumentNodeId("nested".to_owned())
        ]
    );
    assert!(
        !state
            .frame()
            .nodes
            .contains_key(&DocumentNodeId("nested".to_owned()))
    );
}


#[test]
fn structural_child_patches_reject_cycles_and_bad_indices() {
    let mut state = DocumentState::new("root");
    for (id, kind, parent) in [
        ("panel", DocumentNodeKind::Stack, "root"),
        ("child", DocumentNodeKind::Stack, "panel"),
        ("leaf", DocumentNodeKind::Text, "child"),
    ] {
        state
            .apply_patch(DocumentPatch::UpsertNode(node(id, kind, Some(parent))))
            .unwrap();
    }

    let cycle = state
        .apply_patch(DocumentPatch::MoveChild {
            child: DocumentNodeId("panel".to_owned()),
            new_parent: DocumentNodeId("leaf".to_owned()),
            index: 0,
        })
        .unwrap_err();
    assert!(matches!(
        cycle,
        PatchApplyError::Cycle { id } if id.0 == "panel"
    ));

    let bad_index = state
        .apply_patch(DocumentPatch::InsertChild {
            parent: DocumentNodeId("panel".to_owned()),
            child: DocumentNodeId("child".to_owned()),
            index: 9,
        })
        .unwrap_err();
    assert!(matches!(
        bad_index,
        PatchApplyError::ChildIndexOutOfBounds {
            parent,
            index: 9,
            child_count: 0
        } if parent.0 == "panel"
    ));
    assert_eq!(
        state.frame().nodes[&DocumentNodeId("panel".to_owned())].children,
        vec![DocumentNodeId("child".to_owned())],
        "failed reorders must not mutate committed state"
    );
}


#[test]
fn document_remove_root_is_explicit_error() {
    let mut state = DocumentState::new("root");
    let error = state
        .apply_patch(DocumentPatch::RemoveNode {
            id: DocumentNodeId("root".to_owned()),
        })
        .unwrap_err();
    assert!(matches!(
        error,
        PatchApplyError::CannotRemoveRoot { id } if id.0 == "root"
    ));
}


#[test]
fn layout_rejects_stale_focus_and_orphan_child_references() {
    let mut frame = DocumentFrame::empty("root");
    frame.focus = Some(DocumentNodeId("missing-focus".to_owned()));
    let mut text = SimpleTextMeasurer;
    let error = try_layout(LayoutInput {
        document: &frame,
        viewport: Viewport {
            surface: 1,
            width: 100.0,
            height: 100.0,
            scale: 1.0,
        },
        text: &mut text,
        capabilities: RenderCapabilities::fake_portable(),
    })
    .unwrap_err();
    assert!(matches!(
        error,
        PatchApplyError::StaleReference {
            reference_kind: "focus",
            id
        } if id.0 == "missing-focus"
    ));

    let mut frame = DocumentFrame::empty("root");
    frame
        .nodes
        .get_mut(&DocumentNodeId("root".to_owned()))
        .unwrap()
        .children
        .push(DocumentNodeId("missing-child".to_owned()));
    let mut text = SimpleTextMeasurer;
    let error = try_layout(LayoutInput {
        document: &frame,
        viewport: Viewport {
            surface: 1,
            width: 100.0,
            height: 100.0,
            scale: 1.0,
        },
        text: &mut text,
        capabilities: RenderCapabilities::fake_portable(),
    })
    .unwrap_err();
    assert!(matches!(
        error,
        PatchApplyError::OrphanedChild { parent, child }
            if parent.0 == "root" && child.0 == "missing-child"
    ));
}
