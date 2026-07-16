#[test]
fn surface_render_extent_preserves_the_exact_target_size() {
    assert_eq!(surface_render_extent(2_560, 1_440), (2_560, 1_440));
    assert_eq!(surface_render_extent(1_020, 1_082), (1_020, 1_082));
    assert_eq!(surface_render_extent(0, 0), (1, 1));
}

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
        overlay_batch_start: 1,
        rect_metrics: RectVertexMetrics {
            visible_display_item_count: 1,
            rendered_rect_count: 1,
            cap_hit: false,
        },
        text_runs: Vec::new(),
    };

    let (batches, metrics) = rect_vertices_from_scene(&scene, 320.0, 200.0);
    let chunk_summary =
        sampled_retained_render_chunks(&scene.items, &scene.text_runs, 3, None, 16);
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
    let mut document_scene = DocumentRenderScene {
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
        overlay_visual_primitives: Vec::new(),
        quad_batches: Vec::new(),
        text_runs: Vec::new(),
        metrics: boon_document::RenderSceneMetrics {
            visible_source_item_count: 1,
            visual_primitive_count: 1,
            rendered_rect_count: 1,
            cap_hit: false,
        },
    };

    document_scene
        .overlay_visual_primitives
        .push(document_scene.visual_primitives[0].clone());
    let scene = render_scene_from_document_scene(&document_scene, 320, 200);
    let (batches, metrics) = rect_vertices_from_scene(&scene, 320.0, 200.0);
    let chunk_summary =
        sampled_retained_render_chunks(&scene.items, &scene.text_runs, 11, None, 16);
    let chunks = chunk_summary.retained_chunks;

    assert_eq!(scene.items[0].source_kind, "Button");
    assert_eq!(scene.overlay_batch_start, 1);
    assert_eq!(batches.len(), 2);
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
            overlay_visual_primitives: Vec::new(),
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
