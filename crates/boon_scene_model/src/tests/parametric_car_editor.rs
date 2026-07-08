#[test]
fn parametric_car_export_preparation_routes_selection_to_printable_parts_only() {
    let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("car fixture should compile to visual proxy scene");
    let scene = visual.scene;
    let wheel_instance = scene
        .semantics
        .iter()
        .find_map(|(instance, binding)| (binding.label == "Front-left wheel").then_some(*instance))
        .expect("front-left wheel semantic binding");
    let window_instance = scene
        .semantics
        .iter()
        .find_map(|(instance, binding)| {
            (binding.label == "Visual-only windows").then_some(*instance)
        })
        .expect("visual-only window semantic binding");

    let no_selection = scene
        .manufacturing_export_preparation(&bundle)
        .expect("no-selection export preparation");
    assert_eq!(
        no_selection.status,
        WorldManufacturingExportStatus::ReadyNoSelection
    );
    assert_eq!(no_selection.printable_part_count, 2);
    assert_eq!(no_selection.printable_instance_count, 5);
    assert_eq!(no_selection.excluded_visual_only_instance_count, 1);

    let mut selected_wheel = scene.clone();
    selected_wheel.selection = scene.selection_for_pick(scene.instances[&wheel_instance].pick_id);
    let wheel_preparation = selected_wheel
        .manufacturing_export_preparation(&bundle)
        .expect("wheel export preparation");
    assert_eq!(
        wheel_preparation.status,
        WorldManufacturingExportStatus::ReadySelectedPrintable
    );
    assert_eq!(wheel_preparation.selected_instance, Some(wheel_instance));
    assert_eq!(wheel_preparation.selected_part, Some(PartId(2)));
    assert_eq!(
        wheel_preparation.selected_physical_material,
        Some(PhysicalMaterialId(2))
    );
    assert!(wheel_preparation.selected_part_exportable);
    assert_eq!(wheel_preparation.printable_instance_count, 5);
    assert_eq!(wheel_preparation.excluded_visual_only_instance_count, 1);

    let mut selected_windows = scene.clone();
    selected_windows.selection =
        scene.selection_for_pick(scene.instances[&window_instance].pick_id);
    let window_preparation = selected_windows
        .manufacturing_export_preparation(&bundle)
        .expect("window export preparation");
    assert_eq!(
        window_preparation.status,
        WorldManufacturingExportStatus::SelectionNotPrintable
    );
    assert_eq!(window_preparation.selected_instance, Some(window_instance));
    assert_eq!(window_preparation.selected_part, Some(PartId(3)));
    assert_eq!(window_preparation.selected_physical_material, None);
    assert!(!window_preparation.selected_part_exportable);
    assert_eq!(window_preparation.printable_part_count, 2);
    assert_eq!(window_preparation.printable_instance_count, 5);
    assert_eq!(window_preparation.excluded_visual_only_instance_count, 1);
}

#[test]
fn parametric_car_editor_source_actions_select_and_export_from_model() {
    let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("car fixture should compile to visual proxy scene");
    let scene = visual.scene;
    let wheel_instance = scene
        .semantics
        .iter()
        .find_map(|(instance, binding)| (binding.label == "Front-left wheel").then_some(*instance))
        .expect("front-left wheel semantic binding");
    let select_action = WorldEditorSourceAction {
        source_path: format!("world.instance.{}.select", wheel_instance.0),
        source_intent: Some("select".to_owned()),
    };

    let select_outcome = scene
        .editor_source_action_outcome(&bundle, &select_action)
        .expect("selection source action");
    let mut selected = scene.clone();
    let select_report = selected
        .apply_patch(select_outcome.patch.as_ref().expect("selection patch"))
        .expect("selection patch should apply");

    assert_eq!(
        select_outcome.action,
        WorldEditorActionKind::SelectInstance {
            instance: wheel_instance
        }
    );
    assert_eq!(select_report.selection_update_count, 1);
    assert_eq!(select_report.transform_update_count, 0);
    assert_eq!(select_report.geometry_rebuild_count, 0);
    assert_eq!(
        selected
            .selection
            .as_ref()
            .map(|selection| selection.instance),
        Some(wheel_instance)
    );

    let export_action = WorldEditorSourceAction {
        source_path: "world.manufacturing.export_3mf".to_owned(),
        source_intent: Some("press".to_owned()),
    };
    let export_outcome = selected
        .editor_source_action_outcome(&bundle, &export_action)
        .expect("export source action");
    let preparation = export_outcome
        .export_preparation
        .as_ref()
        .expect("export preparation");

    assert_eq!(export_outcome.action, WorldEditorActionKind::Export3Mf);
    assert!(export_outcome.patch.is_none());
    assert_eq!(
        preparation.status,
        WorldManufacturingExportStatus::ReadySelectedPrintable
    );
    assert_eq!(preparation.selected_instance, Some(wheel_instance));
    assert!(preparation.selected_part_exportable);
    assert_eq!(preparation.printable_instance_count, 5);
    assert_eq!(preparation.excluded_visual_only_instance_count, 1);
}

#[test]
fn parametric_car_editor_session_applies_source_actions_to_scene_state() {
    let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("car fixture should compile to visual proxy scene");
    let scene = visual.scene;
    let wheel_instance = scene
        .semantics
        .iter()
        .find_map(|(instance, binding)| (binding.label == "Front-left wheel").then_some(*instance))
        .expect("front-left wheel semantic binding");
    let mut session = WorldEditorSession::new(scene);

    let select_report = session
        .handle_source_action(
            &bundle,
            &WorldEditorSourceAction {
                source_path: format!("world.instance.{}.select", wheel_instance.0),
                source_intent: Some("select".to_owned()),
            },
        )
        .expect("session selection action");
    assert_eq!(
        select_report.outcome.action,
        WorldEditorActionKind::SelectInstance {
            instance: wheel_instance
        }
    );
    assert_eq!(
        select_report
            .patch_report
            .as_ref()
            .map(|report| report.selection_update_count),
        Some(1)
    );
    assert_eq!(select_report.selected_instance_count, 1);
    assert_eq!(
        session
            .scene
            .selection
            .as_ref()
            .map(|selection| selection.instance),
        Some(wheel_instance)
    );
    assert_eq!(
        session
            .semantic_editor_tree(&bundle, "Car editor")
            .expect("selected editor tree")
            .metrics
            .selected_node_count,
        1
    );

    let export_report = session
        .handle_source_action(
            &bundle,
            &WorldEditorSourceAction {
                source_path: "world.manufacturing.export_3mf".to_owned(),
                source_intent: Some("press".to_owned()),
            },
        )
        .expect("session export action");
    let preparation = export_report
        .outcome
        .export_preparation
        .as_ref()
        .expect("session export preparation");
    assert_eq!(
        export_report.outcome.action,
        WorldEditorActionKind::Export3Mf
    );
    assert!(export_report.patch_report.is_none());
    assert_eq!(export_report.selected_instance_count, 1);
    assert_eq!(
        preparation.status,
        WorldManufacturingExportStatus::ReadySelectedPrintable
    );
    assert_eq!(preparation.selected_instance, Some(wheel_instance));
    assert!(preparation.selected_part_exportable);
    assert_eq!(session.last_action, Some(WorldEditorActionKind::Export3Mf));
    assert_eq!(
        session
            .last_export_preparation
            .as_ref()
            .map(|preparation| &preparation.status),
        Some(&WorldManufacturingExportStatus::ReadySelectedPrintable)
    );
}

#[test]
fn parametric_car_semantic_editor_tree_exposes_parts_parameters_and_export() {
    let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("car fixture should compile to visual proxy scene");
    let mut scene = visual.scene;
    let wheel_instance = scene
        .semantics
        .iter()
        .find_map(|(instance, binding)| (binding.label == "Front-left wheel").then_some(*instance))
        .expect("front-left wheel semantic binding");
    scene.selection = scene.selection_for_pick(scene.instances[&wheel_instance].pick_id);

    let tree = scene
        .semantic_editor_tree_from_solid_model(&bundle, "Car editor")
        .expect("car semantic editor tree");

    assert_eq!(
        tree.nodes[&tree.root].label, "Car editor",
        "root label should expose the editor identity"
    );
    assert_eq!(tree.metrics.part_instance_node_count, 6);
    assert_eq!(tree.metrics.printable_part_node_count, 5);
    assert_eq!(tree.metrics.visual_only_part_node_count, 1);
    assert_eq!(tree.metrics.parameter_node_count, 3);
    assert_eq!(tree.metrics.manufacturing_node_count, 3);
    assert_eq!(tree.metrics.selected_node_count, 1);
    assert_eq!(tree.metrics.exportable_action_count, 1);

    for label in [
        "3D viewport",
        "Car assembly",
        "Car body",
        "Visual-only windows",
        "Front-left wheel",
        "Front-right wheel",
        "Rear-left wheel",
        "Rear-right wheel",
        "Parameters",
        "Body length",
        "Wheel radius",
        "Paint",
        "Manufacturing",
        "Export 3MF",
    ] {
        assert!(
            tree.node_with_label(label).is_some(),
            "semantic editor tree missing `{label}`"
        );
    }

    let selected_wheel = tree
        .node_with_label("Front-left wheel")
        .expect("selected wheel node");
    assert!(selected_wheel.selected);
    assert!(selected_wheel.actions.select);
    assert!(selected_wheel.exportable);
    assert_eq!(
        selected_wheel.manufacturing_role,
        Some(boon_solid_model::ManufacturingRole::PrintableSolid)
    );

    let windows = tree
        .node_with_label("Visual-only windows")
        .expect("windows node");
    assert!(!windows.exportable);
    assert_eq!(
        windows.manufacturing_role,
        Some(boon_solid_model::ManufacturingRole::VisualOnly)
    );

    let export = tree.node_with_label("Export 3MF").expect("export action");
    assert!(export.actions.export_3mf);
    assert!(export.exportable);
    assert_eq!(export.part_id, selected_wheel.part_id);
}
