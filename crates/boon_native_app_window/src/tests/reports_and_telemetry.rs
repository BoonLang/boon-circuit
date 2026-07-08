// Included by `../tests.rs`; kept in the parent test module for private app-window helper access.

#[test]
fn elapsed_delta_ms_only_reports_forward_time() {
    assert_eq!(elapsed_delta_ms(Some(10.0), Some(14.5)), Some(4.5));
    assert_eq!(elapsed_delta_ms(Some(14.5), Some(10.0)), None);
    assert_eq!(elapsed_delta_ms(None, Some(10.0)), None);
    assert_eq!(elapsed_delta_ms(Some(10.0), None), None);
}


#[test]
fn accessibility_action_requests_drive_world_editor_session_actions() {
    let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
    let visual = boon_scene_model::WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("car fixture should compile to visual proxy scene");
    let mut session = boon_scene_model::WorldEditorSession::new(visual.scene);
    let scene_for_session = |session: &boon_scene_model::WorldEditorSession| {
        let tree = session
            .semantic_editor_tree(&bundle, "Car editor")
            .expect("world editor semantic tree");
        boon_host::SemanticScene::from_world_editor_tree(&tree)
    };
    let node_id_for_name = |scene: &boon_host::SemanticScene, name: &str| -> accesskit::NodeId {
        let semantic_id = scene
            .nodes
            .values()
            .find(|node| node.name.as_deref() == Some(name))
            .expect("semantic node by name")
            .id
            .clone();
        let node_id =
            accesskit_tree_update_from_semantic_scene(scene, "boon-native", "test-version")
                .semantic_node_ids
                .iter()
                .find(|mapping| mapping.semantic_id == semantic_id.0)
                .expect("semantic node should map to AccessKit")
                .accesskit_node_id;
        accesskit::NodeId(node_id)
    };

    let scene = scene_for_session(&session);
    let wheel_node_id = node_id_for_name(&scene, "Front-left wheel");
    let select_requests =
        native_accessibility_action_requests_from_accesskit(vec![accesskit::ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: wheel_node_id,
            data: None,
        }]);
    let select_reports = native_accessibility_world_editor_session_reports_from_requests(
        &scene,
        &select_requests,
        &mut session,
        &bundle,
    );

    assert_eq!(select_reports.len(), 1);
    assert_eq!(select_reports[0].error, None);
    let select_report = select_reports[0]
        .session_report
        .as_ref()
        .expect("selection session report");
    assert!(matches!(
        select_report.outcome.action,
        boon_scene_model::WorldEditorActionKind::SelectInstance { .. }
    ));
    assert_eq!(
        select_report
            .patch_report
            .as_ref()
            .map(|report| report.selection_update_count),
        Some(1)
    );
    assert_eq!(select_report.selected_instance_count, 1);

    let selected_scene = scene_for_session(&session);
    assert_eq!(
        selected_scene
            .nodes
            .values()
            .filter(|node| node.state.selected)
            .count(),
        1
    );
    let export_node_id = node_id_for_name(&selected_scene, "Export 3MF");
    let export_requests =
        native_accessibility_action_requests_from_accesskit(vec![accesskit::ActionRequest {
            action: accesskit::Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: export_node_id,
            data: None,
        }]);
    let export_reports = native_accessibility_world_editor_session_reports_from_requests(
        &selected_scene,
        &export_requests,
        &mut session,
        &bundle,
    );

    assert_eq!(export_reports.len(), 1);
    assert_eq!(export_reports[0].error, None);
    let export_report = export_reports[0]
        .session_report
        .as_ref()
        .expect("export session report");
    let preparation = export_report
        .outcome
        .export_preparation
        .as_ref()
        .expect("export preparation");
    assert_eq!(
        export_report.outcome.action,
        boon_scene_model::WorldEditorActionKind::Export3Mf
    );
    assert_eq!(
        preparation.status,
        boon_scene_model::WorldManufacturingExportStatus::ReadySelectedPrintable
    );
    assert!(preparation.selected_part_exportable);
    assert_eq!(preparation.excluded_visual_only_instance_count, 1);
}
