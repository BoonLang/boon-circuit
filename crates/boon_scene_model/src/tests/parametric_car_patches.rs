#[test]
fn parametric_car_paint_is_material_only_patch() {
    let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("car fixture should compile to visual proxy scene");
    let scene = visual.scene;
    let body_instance = scene
        .semantics
        .iter()
        .find_map(|(instance, binding)| (binding.label == "Car body").then_some(*instance))
        .expect("car body semantic binding");
    let body_material = scene.instances[&body_instance].appearance;
    let mut repainted = scene.clone();
    repainted
        .appearances
        .get_mut(&body_material)
        .expect("body material")
        .base_color = [0.75, 0.08, 0.06, 1.0];

    let patch = WorldScene::diff(&scene, &repainted);
    let mut applied = scene;
    let report = applied.apply_patch(&patch).expect("car paint patch");

    assert_eq!(patch.operations.len(), 1);
    assert!(matches!(
        patch.operations[0],
        WorldPatchOperation::SetAppearanceMaterial { .. }
    ));
    assert_eq!(report.material_update_count, 1);
    assert_eq!(report.transform_update_count, 0);
    assert_eq!(report.geometry_rebuild_count, 0);
}

#[test]
fn parametric_car_one_wheel_move_is_transform_only_patch() {
    let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("car fixture should compile to visual proxy scene");
    let scene = visual.scene;
    let wheel_instance = scene
        .semantics
        .iter()
        .find_map(|(instance, binding)| (binding.label == "Front-left wheel").then_some(*instance))
        .expect("front-left wheel semantic binding");
    let mut moved = scene.clone();
    moved
        .instances
        .get_mut(&wheel_instance)
        .expect("wheel instance")
        .transform
        .translation[0] -= 4.0;

    let patch = WorldScene::diff(&scene, &moved);
    let mut applied = scene.clone();
    let report = applied.apply_patch(&patch).expect("car wheel patch");

    assert_eq!(patch.operations.len(), 1);
    assert!(matches!(
        patch.operations[0],
        WorldPatchOperation::SetTransform { .. }
    ));
    assert_eq!(report.transform_update_count, 1);
    assert_eq!(report.material_update_count, 0);
    assert_eq!(report.geometry_rebuild_count, 0);
    assert_eq!(applied.metrics().shared_geometry_instance_count, 4);
}

#[test]
fn parametric_car_wheel_radius_updates_one_shared_geometry() {
    let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("car fixture should compile to visual proxy scene");
    let scene = visual.scene;
    let wheel_instance = scene
        .semantics
        .iter()
        .find_map(|(instance, binding)| (binding.label == "Front-left wheel").then_some(*instance))
        .expect("front-left wheel semantic binding");
    let wheel_geometry = scene.instances[&wheel_instance].geometry;
    let mut larger_wheel = scene.clone();
    larger_wheel
        .geometries
        .get_mut(&wheel_geometry)
        .expect("wheel geometry")
        .revision = GeometryRevision(2);

    let patch = WorldScene::diff(&scene, &larger_wheel);
    let mut applied = scene.clone();
    let report = applied.apply_patch(&patch).expect("wheel geometry patch");

    assert_eq!(patch.operations.len(), 1);
    assert!(matches!(
        patch.operations[0],
        WorldPatchOperation::UpsertGeometry(_)
    ));
    assert_eq!(report.geometry_update_count, 1);
    assert_eq!(report.geometry_rebuild_count, 1);
    assert_eq!(report.instance_upsert_count, 0);
    assert_eq!(applied.metrics().shared_geometry_instance_count, 4);
    assert_eq!(
        applied
            .instances
            .values()
            .filter(|instance| instance.geometry == wheel_geometry)
            .filter(|instance| instance.geometry_revision == GeometryRevision(2))
            .count(),
        4
    );
}

#[test]
fn parametric_car_body_length_updates_body_geometry_without_touching_wheels() {
    let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("car fixture should compile to visual proxy scene");
    let scene = visual.scene;
    let body_instance = scene
        .semantics
        .iter()
        .find_map(|(instance, binding)| (binding.label == "Car body").then_some(*instance))
        .expect("body semantic binding");
    let wheel_instance = scene
        .semantics
        .iter()
        .find_map(|(instance, binding)| (binding.label == "Front-left wheel").then_some(*instance))
        .expect("front-left wheel semantic binding");
    let body_geometry = scene.instances[&body_instance].geometry;
    let wheel_geometry = scene.instances[&wheel_instance].geometry;
    let mut longer_body = scene.clone();
    let body = longer_body
        .geometries
        .get_mut(&body_geometry)
        .expect("body geometry");
    body.revision = GeometryRevision(2);
    if let GeometryKind::IndexedMeshSummary { bounds, .. } = &mut body.kind {
        bounds.min[0] -= 4.0;
        bounds.max[0] += 4.0;
    }

    let patch = WorldScene::diff(&scene, &longer_body);
    let mut applied = scene.clone();
    let report = applied.apply_patch(&patch).expect("body geometry patch");

    assert_eq!(patch.operations.len(), 1);
    assert!(matches!(
        patch.operations[0],
        WorldPatchOperation::UpsertGeometry(_)
    ));
    assert_eq!(report.geometry_update_count, 1);
    assert_eq!(report.geometry_rebuild_count, 1);
    assert_eq!(report.instance_upsert_count, 0);
    assert_eq!(
        applied.instances[&body_instance].geometry_revision,
        GeometryRevision(2)
    );
    assert_eq!(
        applied
            .instances
            .values()
            .filter(|instance| instance.geometry == wheel_geometry)
            .filter(|instance| instance.geometry_revision == GeometryRevision(1))
            .count(),
        4
    );
    assert_eq!(applied.metrics().shared_geometry_instance_count, 4);
}

#[test]
fn parametric_car_selection_is_selection_only_patch() {
    let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("car fixture should compile to visual proxy scene");
    let scene = visual.scene;
    let wheel_instance = scene
        .semantics
        .iter()
        .find_map(|(instance, binding)| (binding.label == "Front-left wheel").then_some(*instance))
        .expect("front-left wheel semantic binding");
    let wheel_pick_id = scene.instances[&wheel_instance].pick_id;
    let selection = scene
        .selection_for_pick(wheel_pick_id)
        .expect("front-left wheel selection");
    let mut selected = scene.clone();
    selected.selection = Some(selection.clone());

    let patch = WorldScene::diff(&scene, &selected);
    let mut applied = scene.clone();
    let report = applied.apply_patch(&patch).expect("car selection patch");

    assert_eq!(patch.operations.len(), 1);
    assert!(matches!(
        patch.operations[0],
        WorldPatchOperation::SetSelection(_)
    ));
    assert_eq!(report.selection_update_count, 1);
    assert_eq!(report.transform_update_count, 0);
    assert_eq!(report.visibility_update_count, 0);
    assert_eq!(report.geometry_rebuild_count, 0);
    assert_eq!(report.instance_upsert_count, 0);
    assert_eq!(applied.selection, Some(selection));
    assert_eq!(applied.metrics().selected_instance_count, 1);
    assert_eq!(applied.metrics().shared_geometry_instance_count, 4);
}

#[test]
fn parametric_car_hidden_windows_are_visibility_only_patch() {
    let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("car fixture should compile to visual proxy scene");
    let scene = visual.scene;
    let window_instance = scene
        .semantics
        .iter()
        .find_map(|(instance, binding)| {
            (binding.label == "Visual-only windows").then_some(*instance)
        })
        .expect("visual-only window semantic binding");
    let window_pick_id = scene.instances[&window_instance].pick_id;
    let mut hidden_windows = scene.clone();
    hidden_windows
        .instances
        .get_mut(&window_instance)
        .expect("window instance")
        .visibility = Visibility::Hidden;

    let patch = WorldScene::diff(&scene, &hidden_windows);
    let mut applied = scene.clone();
    let report = applied
        .apply_patch(&patch)
        .expect("car window visibility patch");

    assert_eq!(patch.operations.len(), 1);
    assert!(matches!(
        patch.operations[0],
        WorldPatchOperation::SetVisibility { .. }
    ));
    assert_eq!(report.visibility_update_count, 1);
    assert_eq!(report.transform_update_count, 0);
    assert_eq!(report.selection_update_count, 0);
    assert_eq!(report.geometry_rebuild_count, 0);
    assert_eq!(report.instance_upsert_count, 0);
    assert_eq!(applied.metrics().pickable_instance_count, 5);
    assert_eq!(applied.metrics().shared_geometry_instance_count, 4);
    assert!(
        applied.pick_target(window_pick_id).is_none(),
        "hidden windows must be removed from pick routing"
    );
}

