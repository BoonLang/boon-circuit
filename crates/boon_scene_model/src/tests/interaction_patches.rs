use super::*;

#[test]
fn hello_cube_rotation_is_transform_only_patch() {
    let scene = WorldScene::hello_cube_fixture();
    let mut rotated = scene.clone();
    rotated
        .instances
        .get_mut(&InstanceId(1))
        .expect("fixture instance")
        .transform = Transform3D::IDENTITY.with_rotation_z_degrees(45.0);

    let patch = WorldScene::diff(&scene, &rotated);
    assert_eq!(patch.operations.len(), 1);
    assert!(matches!(
        patch.operations[0],
        WorldPatchOperation::SetTransform {
            instance: InstanceId(1),
            ..
        }
    ));

    let mut applied = scene;
    let report = applied.apply_patch(&patch).expect("transform patch");
    assert_eq!(report.transform_update_count, 1);
    assert_eq!(report.geometry_rebuild_count, 0);
    assert_eq!(
        applied.instances[&InstanceId(1)].geometry_revision,
        GeometryRevision(1)
    );
}

#[test]
fn hello_cube_color_is_material_only_patch() {
    let scene = WorldScene::hello_cube_fixture();
    let mut recolored = scene.clone();
    recolored
        .appearances
        .get_mut(&AppearanceMaterialId(1))
        .expect("fixture material")
        .base_color = [0.95, 0.25, 0.15, 1.0];

    let patch = WorldScene::diff(&scene, &recolored);
    assert_eq!(patch.operations.len(), 1);
    assert!(matches!(
        patch.operations[0],
        WorldPatchOperation::SetAppearanceMaterial {
            material: AppearanceMaterialId(1),
            ..
        }
    ));

    let mut applied = scene;
    let report = applied.apply_patch(&patch).expect("material patch");
    assert_eq!(report.material_update_count, 1);
    assert_eq!(report.geometry_rebuild_count, 0);
    assert_eq!(
        applied.instances[&InstanceId(1)].geometry_revision,
        GeometryRevision(1)
    );
}

#[test]
fn hello_cube_pick_target_carries_world_semantic_identity() {
    let scene = WorldScene::hello_cube_fixture();
    let target = scene.pick_target(PickId(1)).expect("pick target");
    assert_eq!(target.instance, InstanceId(1));
    assert_eq!(target.geometry, GeometryLogicalId(1));
    assert_eq!(target.geometry_revision, GeometryRevision(1));
    assert_eq!(target.part_id, PartId(1));
    assert_eq!(target.feature_id, FeatureId(1));
    assert_eq!(target.semantic_id.as_deref(), Some("world:cube"));
    assert_eq!(target.label.as_deref(), Some("Hello cube"));
}

#[test]
fn picked_solid_instance_routes_to_selected_semantic_editor_node() {
    let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("car fixture should compile to visual proxy scene");
    let wheel_instance = visual
        .scene
        .semantics
        .iter()
        .find_map(|(instance, binding)| (binding.label == "Front-left wheel").then_some(*instance))
        .expect("front-left wheel semantic binding");
    let pick_id = visual.scene.instances[&wheel_instance].pick_id;

    let route = visual
        .scene
        .semantic_editor_route_for_pick(&bundle, "Car editor", pick_id)
        .expect("semantic route should build")
        .expect("pick should route to semantic editor node");

    assert_eq!(route.pick_id, pick_id);
    assert_eq!(route.selection.instance, wheel_instance);
    assert_eq!(route.semantic_id, route.selection.semantic_id);
    assert!(
        route
            .semantic_id
            .as_deref()
            .is_some_and(|semantic_id| semantic_id.starts_with("solid:part:2:instance:"))
    );
    assert_eq!(
        route.focused_node,
        WorldSemanticEditorNodeId(format!(
            "world-editor:assembly:instance:{}",
            wheel_instance.0
        ))
    );
    assert_eq!(route.label.as_deref(), Some("Front-left wheel"));
}

#[test]
fn solid_feature_routes_to_visible_semantic_editor_nodes() {
    let bundle = boon_solid_model::SolidModelBundle::parametric_car_fixture();
    let visual = WorldScene::visual_proxy_with_chunks_from_solid_model(&bundle)
        .expect("car fixture should compile to visual proxy scene");
    let feature_id = visual
        .scene
        .instances
        .values()
        .next()
        .expect("visible car instance")
        .feature_id;

    let routes = visual
        .scene
        .semantic_editor_routes_for_feature(&bundle, "Car editor", feature_id)
        .expect("feature routes should build");

    assert_eq!(routes.len(), visual.scene.metrics().pickable_instance_count);
    assert!(routes.iter().all(|route| {
        route.selection.feature_id == feature_id
            && route.selection.semantic_id.is_some()
            && route
                .focused_node
                .0
                .starts_with("world-editor:assembly:instance:")
    }));
}

#[test]
fn hello_cube_orbit_camera_drag_is_camera_only_patch() {
    let scene = WorldScene::hello_cube_fixture();
    let patch = scene
        .orbit_camera_drag(
            CameraId(1),
            WorldOrbitCameraDrag::around_origin(0.35, -0.15),
        )
        .expect("orbit camera patch");

    assert_eq!(patch.operations.len(), 1);
    assert!(matches!(
        patch.operations[0],
        WorldPatchOperation::SetCameraTransform {
            camera: CameraId(1),
            ..
        }
    ));

    let mut applied = scene.clone();
    let report = applied.apply_patch(&patch).expect("camera orbit patch");
    assert_eq!(report.camera_transform_update_count, 1);
    assert_eq!(report.transform_update_count, 0);
    assert_eq!(report.geometry_rebuild_count, 0);
    assert_ne!(
        applied.cameras[&CameraId(1)].transform,
        scene.cameras[&CameraId(1)].transform
    );

    let diff = WorldScene::diff(&scene, &applied);
    assert_eq!(diff.operations, patch.operations);
}

#[test]
fn hello_cube_pointer_drag_orbits_camera_only_patch() {
    let scene = WorldScene::hello_cube_fixture();
    let patch = scene
        .orbit_camera_pointer_drag(
            CameraId(1),
            WorldPointerOrbitDrag::around_origin([96.0, -48.0], [320.0, 240.0]),
        )
        .expect("pointer orbit camera patch");

    assert_eq!(patch.operations.len(), 1);
    assert!(matches!(
        patch.operations[0],
        WorldPatchOperation::SetCameraTransform {
            camera: CameraId(1),
            ..
        }
    ));

    let mut applied = scene.clone();
    let report = applied.apply_patch(&patch).expect("pointer orbit patch");
    assert_eq!(report.camera_transform_update_count, 1);
    assert_eq!(report.transform_update_count, 0);
    assert_eq!(report.geometry_rebuild_count, 0);
    assert_ne!(
        applied.cameras[&CameraId(1)].transform,
        scene.cameras[&CameraId(1)].transform
    );

    let diff = WorldScene::diff(&scene, &applied);
    assert_eq!(diff.operations, patch.operations);
}

#[test]
fn host_pointer_orbit_controller_emits_camera_only_drag_patch() {
    let scene = WorldScene::hello_cube_fixture();
    let mut controller = WorldHostPointerOrbitController::around_origin();
    let viewport = [320.0, 240.0];

    assert_eq!(
        controller
            .handle_event(
                &scene,
                CameraId(1),
                WorldHostPointerOrbitEvent::drag([112.0, 92.0], viewport),
            )
            .expect("move without press should be valid"),
        None
    );
    assert_eq!(controller.active_camera(), None);
    assert_eq!(
        controller
            .handle_event(
                &scene,
                CameraId(1),
                WorldHostPointerOrbitEvent::press([96.0, 96.0], viewport),
            )
            .expect("press should start drag"),
        None
    );
    assert_eq!(controller.active_camera(), Some(CameraId(1)));

    let patch = controller
        .handle_event(
            &scene,
            CameraId(1),
            WorldHostPointerOrbitEvent::drag([128.0, 72.0], viewport),
        )
        .expect("drag should produce a patch")
        .expect("drag patch");
    assert_eq!(patch.operations.len(), 1);
    assert!(matches!(
        patch.operations[0],
        WorldPatchOperation::SetCameraTransform {
            camera: CameraId(1),
            ..
        }
    ));

    let mut applied = scene.clone();
    let report = applied.apply_patch(&patch).expect("host pointer patch");
    assert_eq!(report.camera_transform_update_count, 1);
    assert_eq!(report.transform_update_count, 0);
    assert_eq!(report.geometry_rebuild_count, 0);
    assert_eq!(
        WorldScene::diff(&scene, &applied).operations,
        patch.operations
    );

    assert_eq!(
        controller
            .handle_event(
                &applied,
                CameraId(1),
                WorldHostPointerOrbitEvent::release([128.0, 72.0], viewport),
            )
            .expect("release should clear drag"),
        None
    );
    assert_eq!(controller.active_camera(), None);
    assert_eq!(
        controller
            .handle_event(
                &applied,
                CameraId(1),
                WorldHostPointerOrbitEvent::drag([160.0, 72.0], viewport),
            )
            .expect("post-release move should be ignored"),
        None
    );
}

