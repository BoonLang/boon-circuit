#[test]
fn renderer_helpers_accept_prelowered_render_scene_without_layout_frame() {
    let item = RenderSceneItem {
        node: DocumentNodeId("primitive-node".to_owned()),
        retained_chunk_id: "chunk:primitive-node".to_owned(),
        source_kind: "Stack".to_owned(),
        bounds: Rect {
            x: 8.0,
            y: 10.0,
            width: 64.0,
            height: 32.0,
        },
        clip: None,
        transform: [1.0, 0.0, 0.0, 1.0, 8.0, 10.0],
        style_identity: test_style_identity(),
        dependency_set: vec![
            "node:primitive-node".to_owned(),
            "kind:Stack".to_owned(),
            "style:1".to_owned(),
        ],
        texture_asset_refs: Vec::new(),
        estimated_vertex_count: 6,
    };
    let mut builder = QuadBuilder::default();
    builder.set_retained_chunk_id(&item.retained_chunk_id);
    push_rect(
        &mut builder,
        item.bounds,
        320.0,
        200.0,
        [0.1, 0.2, 0.3, 1.0],
    );
    let scene = RenderScene {
        viewport: Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 200.0,
        },
        items: vec![item],
        quad_batches: builder.batches,
        rect_metrics: RectVertexMetrics {
            visible_display_item_count: 1,
            rendered_rect_count: 1,
            cap_hit: false,
        },
        text_runs: Vec::new(),
    };

    let (batches, metrics) = rect_vertices_from_scene(&scene, 320.0, 200.0);
    let chunk_summary = sampled_retained_render_chunks(&scene, 3, None, 16);
    let chunks = chunk_summary.retained_chunks;

    assert_eq!(batches.len(), 1);
    assert_eq!(metrics.visible_display_item_count, 1);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].node.0, "primitive-node");
    assert_eq!(chunks[0].dependency_set[0], "node:primitive-node");
}


#[test]
fn renderer_adapts_external_document_render_scene_without_layout_frame() {
    let style_identity = test_style_identity();
    let document_scene = DocumentRenderScene {
        viewport: Rect {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 200.0,
        },
        items: vec![boon_document::RenderSceneItem {
            node: DocumentNodeId("external-node".to_owned()),
            retained_chunk_id: "chunk:external-node".to_owned(),
            source_kind: DocumentNodeKind::Button,
            bounds: Rect {
                x: 24.0,
                y: 32.0,
                width: 80.0,
                height: 28.0,
            },
            clip: None,
            transform: [1.0, 0.0, 0.0, 1.0, 24.0, 32.0],
            style_identity,
            dependency_set: vec!["prelowered:button".to_owned()],
            texture_asset_refs: Vec::new(),
            estimated_vertex_count: 6,
        }],
        visual_primitives: vec![RenderVisualPrimitive {
            node: DocumentNodeId("external-node".to_owned()),
            retained_chunk_id: "chunk:external-node".to_owned(),
            source_kind: DocumentNodeKind::Button,
            primitive: RenderVisualPrimitiveKind::Fill,
            bounds: Rect {
                x: 24.0,
                y: 32.0,
                width: 80.0,
                height: 28.0,
            },
            clip: None,
            radius: 4.0,
            stroke_width: 0.0,
            color: [20, 80, 160, 255],
            secondary_color: [0, 0, 0, 0],
            antialias: 0.0,
            control_points: Vec::new(),
            texture: RenderTextureRef::Solid,
            style_identity,
            dependency_set: vec!["prelowered:fill".to_owned()],
        }],
        quad_batches: Vec::new(),
        text_runs: Vec::new(),
        metrics: boon_document::RenderSceneMetrics {
            visible_source_item_count: 1,
            visual_primitive_count: 1,
            rendered_rect_count: 1,
            cap_hit: false,
        },
    };

    let scene = render_scene_from_document_scene(&document_scene, 320, 200);
    let (batches, metrics) = rect_vertices_from_scene(&scene, 320.0, 200.0);
    let chunk_summary = sampled_retained_render_chunks(&scene, 11, None, 16);
    let chunks = chunk_summary.retained_chunks;

    assert_eq!(scene.items[0].source_kind, "Button");
    assert_eq!(batches.len(), 1);
    assert_eq!(metrics.visible_display_item_count, 1);
    assert_eq!(metrics.rendered_rect_count, 1);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].kind, "Button");
    assert_eq!(chunks[0].dependency_set, vec!["prelowered:button"]);
}


#[test]
fn app_owned_scene_readback_uses_prelowered_render_scene_identity() {
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
                    "skipping app-owned scene readback test: request_adapter failed: {error}"
                );
                return;
            }
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-native-gpu-scene-readback-test-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
            })
            .await
            .expect("test WGPU device should be available when adapter exists");
        let style_identity = test_style_identity();
        let document_scene = DocumentRenderScene {
            viewport: Rect {
                x: 0.0,
                y: 0.0,
                width: 96.0,
                height: 64.0,
            },
            items: vec![boon_document::RenderSceneItem {
                node: DocumentNodeId("prelowered".to_owned()),
                retained_chunk_id: "chunk:prelowered".to_owned(),
                source_kind: DocumentNodeKind::Stack,
                bounds: Rect {
                    x: 8.0,
                    y: 10.0,
                    width: 48.0,
                    height: 32.0,
                },
                clip: None,
                transform: [1.0, 0.0, 0.0, 1.0, 8.0, 10.0],
                style_identity,
                dependency_set: vec!["prelowered:test".to_owned()],
                texture_asset_refs: Vec::new(),
                estimated_vertex_count: 6,
            }],
            visual_primitives: vec![RenderVisualPrimitive {
                node: DocumentNodeId("prelowered".to_owned()),
                retained_chunk_id: "chunk:prelowered".to_owned(),
                source_kind: DocumentNodeKind::Stack,
                primitive: RenderVisualPrimitiveKind::Fill,
                bounds: Rect {
                    x: 8.0,
                    y: 10.0,
                    width: 48.0,
                    height: 32.0,
                },
                clip: None,
                radius: 0.0,
                stroke_width: 0.0,
                color: [240, 32, 16, 255],
                secondary_color: [0, 0, 0, 0],
                antialias: 0.0,
                control_points: Vec::new(),
                texture: RenderTextureRef::Solid,
                style_identity,
                dependency_set: vec!["primitive:fill".to_owned()],
            }],
            quad_batches: Vec::new(),
            text_runs: Vec::new(),
            metrics: boon_document::RenderSceneMetrics {
                visible_source_item_count: 1,
                visual_primitive_count: 1,
                rendered_rect_count: 1,
                cap_hit: false,
            },
        };
        let render_identity_hash = "scene-identity-test";
        let proof = render_app_owned_scene_pixels(AppOwnedRenderSceneRequest {
            device: &device,
            queue: &queue,
            scene: &document_scene,
            render_identity_hash,
            surface_id: SurfaceId("scene-readback-test".to_owned()),
            surface_epoch: 3,
            width: 96,
            height: 64,
            artifact_dir: Path::new("target/artifacts/native-gpu/tests"),
            artifact_label: "prelowered-scene-readback",
        })
        .expect("prelowered render scene should render to app-owned pixels");

        let RenderProofArtifact::AppOwnedPixels {
            layout_frame_hash,
            render_scene_identity_hash,
            nonblank_samples,
            ..
        } = proof.artifact
        else {
            panic!("expected app-owned pixel artifact");
        };
        assert_eq!(layout_frame_hash, None);
        assert_eq!(render_scene_identity_hash, render_identity_hash);
        assert!(nonblank_samples > 0);
        assert_eq!(
            proof.metrics.render_scene_source,
            RENDER_SCENE_SOURCE_APP_OWNED_DOCUMENT_RENDER_SCENE
        );
    });
}


#[test]
fn world_scene_projection_adds_retained_selection_outline() {
    let scene = boon_scene_model::WorldScene::hello_cube_fixture();
    let unselected = world_scene_projection_render_scene(&scene, 128, 96);
    assert_eq!(unselected.items.len(), 1);
    assert_eq!(unselected.visual_primitives.len(), 4);
    assert!(
        !unselected
            .visual_primitives
            .iter()
            .any(|primitive| primitive.primitive == RenderVisualPrimitiveKind::Border),
        "unselected world scene should not synthesize a selection outline"
    );

    let pick_id = scene.instances.values().next().unwrap().pick_id;
    let mut selected_scene = scene.clone();
    selected_scene.selection = Some(
        scene
            .selection_for_pick(pick_id)
            .expect("hello cube pick should resolve to a selection"),
    );
    let selected = world_scene_projection_render_scene(&selected_scene, 128, 96);

    assert_eq!(
        selected.items.len(),
        1,
        "selection outline must not invent a second source item"
    );
    assert_eq!(selected.visual_primitives.len(), 5);
    assert_eq!(selected.metrics.visible_source_item_count, 1);
    assert_eq!(selected.metrics.visual_primitive_count, 5);
    assert_eq!(selected.metrics.rendered_rect_count, 5);
    let outline = selected
        .visual_primitives
        .iter()
        .find(|primitive| primitive.primitive == RenderVisualPrimitiveKind::Border)
        .expect("selected world scene should synthesize one retained outline primitive");
    assert!(
        outline
            .retained_chunk_id
            .starts_with("chunk:world:selection:instance:")
    );
    assert!(
        outline
            .dependency_set
            .iter()
            .any(|key| key == "world:selection")
    );
    assert!(
        outline
            .dependency_set
            .iter()
            .any(|key| key.starts_with("world:selection:pick:")),
        "selection outline must retain a pick-specific dependency"
    );
    assert!(
        outline
            .dependency_set
            .iter()
            .any(|key| key.starts_with("world:feature:")),
        "selection outline must retain feature identity"
    );
    assert_eq!(outline.stroke_width, 3.0);
    assert_eq!(outline.color, [255, 214, 10, 255]);
}


#[test]
fn solid_visual_scene_mesh_pipeline_draws_retained_chunk_payloads() {
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
                    "skipping retained chunk mesh pipeline test: request_adapter failed: {error}"
                );
                return;
            }
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-native-gpu-solid-visual-retained-chunk-test-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
            })
            .await
            .expect("test WGPU device should be available when adapter exists");
        let visual = boon_scene_model::WorldScene::visual_proxy_with_chunks_from_solid_model(
            &boon_solid_model::SolidModelBundle::parametric_car_fixture(),
        )
        .expect("parametric car should compile to a solid visual scene");
        let proof = render_app_owned_solid_visual_scene_mesh_pipeline(
            &device,
            &queue,
            &visual,
            240,
            160,
            Path::new("target/artifacts/native-gpu/tests"),
            "solid-visual-retained-chunk-mesh",
        )
        .expect("solid visual scene should render retained chunk meshes");

        assert_eq!(
            proof.capture_method,
            "app-owned-solid-visual-scene-retained-chunk-mesh-depth-readback"
        );
        assert_eq!(
            proof.camera_projection_method,
            "shader-camera-uniform-world-to-clip"
        );
        assert_eq!(
            proof.geometry_source,
            "solid-visual-retained-surface-chunks"
        );
        assert_eq!(proof.visible_instance_count, 6);
        assert_eq!(proof.rendered_instance_count, 6);
        assert_eq!(proof.unsupported_geometry_count, 0);
        let chunks_by_geometry = surface_chunks_by_geometry(&visual.chunks);
        let (
            expected_retained_chunk_count,
            expected_retained_vertex_count,
            expected_retained_index_count,
        ) = visual
            .scene
            .instances
            .values()
            .filter(|instance| instance.visibility != boon_scene_model::Visibility::Hidden)
            .fold(
                (0_usize, 0_usize, 0_usize),
                |(chunks, vertices, indices), instance| {
                    let Some(mesh_sources) = chunks_by_geometry.get(&instance.geometry) else {
                        return (chunks, vertices, indices);
                    };
                    let source_vertices = mesh_sources
                        .iter()
                        .map(|source| source.vertex_count)
                        .sum::<usize>();
                    let source_indices = mesh_sources
                        .iter()
                        .map(|source| source.index_count)
                        .sum::<usize>();
                    (
                        chunks + mesh_sources.len(),
                        vertices + source_vertices,
                        indices + source_indices,
                    )
                },
            );
        assert_eq!(proof.retained_chunk_count, expected_retained_chunk_count);
        assert_eq!(
            proof.retained_chunk_vertex_count,
            expected_retained_vertex_count
        );
        assert_eq!(
            proof.retained_chunk_index_count,
            expected_retained_index_count
        );
        assert_eq!(proof.vertex_count, expected_retained_vertex_count);
        assert_eq!(proof.index_count, expected_retained_index_count);
        assert_eq!(proof.triangle_count, expected_retained_index_count / 3);
        assert!(proof.nonblank_samples > 0);
        assert!(proof.unique_rgba_values > 1);
        assert_eq!(proof.normal_format, "Rgba8Unorm");
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
        let expected_feature_ids = visual
            .scene
            .instances
            .values()
            .map(|instance| instance.feature_id.0)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        assert_eq!(proof.sampled_feature_ids, expected_feature_ids);
        assert_eq!(
            proof.sampled_feature_id_count,
            proof.sampled_feature_ids.len()
        );
        assert_eq!(
            proof.unique_feature_id_count,
            proof.sampled_feature_ids.len()
        );
        let expected_pick_ids = visual
            .scene
            .instances
            .values()
            .filter(|instance| {
                instance.visibility != boon_scene_model::Visibility::Hidden
                    && instance.pick_id.0 != 0
            })
            .map(|instance| instance.pick_id.0)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        assert_eq!(proof.sampled_pick_ids, expected_pick_ids);
        assert_eq!(proof.sampled_pick_id_count, proof.sampled_pick_ids.len());
        assert_eq!(proof.unique_pick_id_count, proof.sampled_pick_ids.len());
        assert_eq!(
            proof.hit_test_capture_method,
            "app-owned-world-scene-mesh-feature-target-hit-test"
        );
        assert_eq!(proof.hit_test_status, "feature-target-hit");
        assert!(
            proof
                .hit_test_feature_id
                .is_some_and(|feature_id| proof.sampled_feature_ids.contains(&feature_id))
        );
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


