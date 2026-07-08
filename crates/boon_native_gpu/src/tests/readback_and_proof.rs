// Included by `../tests.rs`; kept in the parent test module for private renderer helper access.

#[test]
fn app_owned_world_scene_readback_uses_world_scene_identity() {
    futures::executor::block_on(async {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: true,
                compatible_surface: None,
            })
            .await
        {
            Ok(adapter) => adapter,
            Err(error) => {
                eprintln!(
                    "skipping app-owned world scene readback test: request_adapter failed: {error}"
                );
                return;
            }
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-native-gpu-world-scene-readback-test-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
            })
            .await
            .expect("test WGPU device should be available when adapter exists");
        let scene = boon_scene_model::WorldScene::hello_cube_fixture();
        let expected_identity = world_scene_identity_hash(&scene);
        let proof = render_app_owned_world_scene_pixels(AppOwnedWorldSceneRenderRequest {
            device: &device,
            queue: &queue,
            scene: &scene,
            surface_id: SurfaceId("world-scene-readback-test".to_owned()),
            surface_epoch: 4,
            width: 128,
            height: 96,
            artifact_dir: Path::new("target/artifacts/native-gpu/tests"),
            artifact_label: "world-scene-readback",
        })
        .expect("world scene should render to app-owned pixels");

        let RenderProofArtifact::AppOwnedPixels {
            layout_frame_hash,
            render_scene_identity_hash,
            nonblank_samples,
            unique_rgba_values,
            ..
        } = proof.artifact
        else {
            panic!("expected app-owned pixel artifact");
        };
        assert_eq!(layout_frame_hash, None);
        assert_eq!(render_scene_identity_hash, expected_identity);
        assert!(nonblank_samples > 0);
        assert!(unique_rgba_values > 1);
        assert_eq!(
            proof.metrics.render_scene_source,
            RENDER_SCENE_SOURCE_APP_OWNED_WORLD_SCENE_PROJECTION
        );
        assert_eq!(proof.metrics.visible_display_item_count, 1);
        assert_eq!(proof.metrics.rendered_rect_count, 4);
    });
}


#[test]
fn world_scene_pick_readback_encodes_stable_pick_ids() {
    let scene = boon_scene_model::WorldScene::hello_cube_fixture();
    let expected_pick_id = scene.instances.values().next().unwrap().pick_id.0;
    let proof = render_app_owned_world_scene_pick_ids(
        &scene,
        128,
        96,
        Path::new("target/artifacts/native-gpu/tests"),
        "world-scene-pick-readback",
    )
    .expect("world scene pick readback should write an app-owned pick target");

    assert_eq!(
        proof.capture_method,
        "app-owned-world-scene-projection-pick-id-readback"
    );
    assert_eq!(
        proof.render_identity_hash,
        world_scene_identity_hash(&scene)
    );
    assert_eq!(proof.projected_pickable_item_count, 1);
    assert_eq!(proof.sampled_pick_id_count, 1);
    assert_eq!(proof.unique_pick_id_count, 1);
    assert_eq!(proof.sampled_pick_ids, vec![expected_pick_id]);
    assert!(proof.artifact_sha256.len() >= 64);
    assert!(Path::new(&proof.artifact_path).exists());
}


#[test]
fn world_scene_feature_depth_readback_encodes_feature_identity() {
    let scene = boon_scene_model::WorldScene::hello_cube_fixture();
    let expected_feature_id = scene.instances.values().next().unwrap().feature_id.0;
    let proof = render_app_owned_world_scene_feature_depth(
        &scene,
        128,
        96,
        Path::new("target/artifacts/native-gpu/tests"),
        "world-scene-feature-depth-readback",
    )
    .expect("world scene feature/depth readback should write an app-owned metadata target");

    assert_eq!(
        proof.capture_method,
        "app-owned-world-scene-projection-feature-depth-readback"
    );
    assert_eq!(
        proof.render_identity_hash,
        world_scene_identity_hash(&scene)
    );
    assert_eq!(proof.projected_instance_count, 1);
    assert_eq!(proof.sampled_feature_id_count, 1);
    assert_eq!(proof.unique_feature_id_count, 1);
    assert_eq!(proof.sampled_feature_ids, vec![expected_feature_id]);
    assert_eq!(proof.min_projection_depth, 0.0);
    assert_eq!(proof.max_projection_depth, 0.0);
    assert!(proof.artifact_sha256.len() >= 64);
    assert!(Path::new(&proof.artifact_path).exists());
}


#[test]
fn world_scene_mesh_pipeline_draws_indexed_triangles_with_depth() {
    futures::executor::block_on(async {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = match instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: true,
                compatible_surface: None,
            })
            .await
        {
            Ok(adapter) => adapter,
            Err(error) => {
                eprintln!(
                    "skipping app-owned world scene mesh pipeline test: request_adapter failed: {error}"
                );
                return;
            }
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-native-gpu-world-scene-mesh-pipeline-test-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
            })
            .await
            .expect("test WGPU device should be available when adapter exists");
        let scene = boon_scene_model::WorldScene::hello_cube_fixture();
        let proof = render_app_owned_world_scene_mesh_pipeline(
            &device,
            &queue,
            &scene,
            160,
            120,
            Path::new("target/artifacts/native-gpu/tests"),
            "world-scene-mesh-pipeline",
        )
        .expect("world scene should render through an indexed mesh pipeline");

        assert_eq!(
            proof.capture_method,
            "app-owned-world-scene-indexed-mesh-depth-readback"
        );
        assert_eq!(
            proof.camera_projection_method,
            "shader-camera-uniform-world-to-clip"
        );
        assert_eq!(
            proof.render_identity_hash,
            world_scene_identity_hash(&scene)
        );
        assert_eq!(proof.color_format, "Rgba8Unorm");
        assert_eq!(proof.feature_format, "Rgba8Unorm");
        assert_eq!(proof.normal_format, "Rgba8Unorm");
        assert_eq!(proof.depth_format, "Depth32Float");
        assert_eq!(proof.visible_instance_count, 1);
        assert_eq!(proof.rendered_instance_count, 1);
        assert_eq!(proof.unsupported_geometry_count, 0);
        assert_eq!(proof.vertex_count, 8);
        assert_eq!(proof.index_count, 36);
        assert_eq!(proof.triangle_count, 12);
        assert!(proof.nonblank_samples > 0);
        assert!(proof.unique_rgba_values > 1);
        assert_eq!(
            proof.normal_capture_method,
            "app-owned-world-scene-mesh-shader-normal-readback"
        );
        assert!(proof.sampled_normal_pixel_count > 0);
        assert!(proof.unique_normal_rgba_values > 1);
        assert_eq!(
            proof.depth_capture_method,
            "app-owned-world-scene-mesh-depth32float-readback"
        );
        assert_eq!(
            proof.sampled_depth_pixel_count,
            proof.width as usize * proof.height as usize
        );
        assert!(proof.visible_depth_pixel_count > 0);
        assert!(proof.min_depth >= 0.0 && proof.min_depth < 1.0);
        assert!(proof.max_depth <= 1.0);
        assert_eq!(
            proof.feature_capture_method,
            "app-owned-world-scene-mesh-shader-feature-id32-readback"
        );
        assert_eq!(proof.sampled_feature_id_count, 1);
        assert_eq!(proof.unique_feature_id_count, 1);
        assert_eq!(proof.sampled_feature_ids, vec![1]);
        assert_eq!(proof.sampled_pick_id_count, 1);
        assert_eq!(proof.unique_pick_id_count, 1);
        assert_eq!(proof.sampled_pick_ids, vec![1]);
        assert_eq!(
            proof.hit_test_capture_method,
            "app-owned-world-scene-mesh-feature-target-hit-test"
        );
        assert_eq!(proof.hit_test_status, "feature-target-hit");
        assert_eq!(proof.hit_test_feature_id, Some(1));
        assert!(proof.hit_test_x < proof.width);
        assert!(proof.hit_test_y < proof.height);
        assert!(proof.hit_test_sampled_pixel_count > 0);
        assert!(proof.artifact_sha256.len() >= 64);
        assert!(Path::new(&proof.artifact_path).exists());
        assert!(proof.normal_artifact_sha256.len() >= 64);
        assert!(Path::new(&proof.normal_artifact_path).exists());
        assert!(proof.feature_artifact_sha256.len() >= 64);
        assert!(Path::new(&proof.feature_artifact_path).exists());
        assert!(proof.pick_artifact_sha256.len() >= 64);
        assert!(Path::new(&proof.pick_artifact_path).exists());
    });
}
