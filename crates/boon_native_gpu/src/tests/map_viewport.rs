fn generic_native_map_frame() -> LayoutFrame {
    use boon_document::{
        MapCamera, MapCoordinate, MapHitIdentity, MapInteractionPolicy, MapOverlayDescriptor,
        MapOverlayGeometry, MapOverlayId, MapOverlayPaint, MapTileSourceId, MapTileSourceRef,
        MapViewportBounds, MapViewportDescriptor, MapViewportGeneration,
    };

    let descriptor = MapViewportDescriptor {
        generation: MapViewportGeneration(1),
        camera: MapCamera {
            longitude: 10.75,
            latitude: 59.91,
            zoom: 4.0,
            bearing: 0.0,
        },
        bounds: MapViewportBounds {
            width: 112.0,
            height: 80.0,
            scale: 1.0,
        },
        tile_source: MapTileSourceRef {
            id: MapTileSourceId("readback-fixture".to_owned()),
            url_template_capability: "readback_fixture_tiles".to_owned(),
            min_zoom: 0,
            max_zoom: 8,
            tile_size: 256,
            attribution: "Readback fixture".to_owned(),
            allowed_origins: vec!["boon-local://readback-map".to_owned()],
        },
        interaction: MapInteractionPolicy {
            pan: true,
            wheel_zoom: true,
            pinch_zoom: true,
            keyboard_zoom: true,
        },
        overlays: vec![MapOverlayDescriptor {
            id: MapOverlayId("center".to_owned()),
            hit_identity: MapHitIdentity("center-hit".to_owned()),
            z_order: 10,
            selected: true,
            focused: false,
            paint: MapOverlayPaint {
                fill: Some("#15a36d".to_owned()),
                stroke: Some("#ffffff".to_owned()),
                stroke_width: 2.0,
                opacity: 1.0,
            },
            geometry: MapOverlayGeometry::Point {
                position: MapCoordinate {
                    longitude: 10.75,
                    latitude: 59.91,
                },
                radius: 7.0,
                symbol_ref: None,
            },
        }],
    };
    LayoutFrame {
        display_list: vec![DisplayItem {
            node: DocumentNodeId("generic-native-map".to_owned()),
            kind: DocumentNodeKind::MapViewport,
            map_viewport: Some(Box::new(descriptor)),
            bounds: Rect {
                x: 8.0,
                y: 8.0,
                width: 112.0,
                height: 80.0,
            },
            text: None,
            style: StyleMap::new(),
            focused: false,
            style_identity: test_style_identity(),
        }],
        hit_regions: Vec::new(),
        scroll_regions: Vec::new(),
        accessibility: AccessibilityTree::default(),
        demands: Vec::new(),
        materialization: Vec::new(),
        metrics: LayoutMetrics::default(),
    }
}

#[test]
fn map_tile_quads_are_clipped_to_the_retained_viewport() {
    let scale = boon_document::MapTileScaleKey::from_viewport_scale(1.0).unwrap();
    let part = MapTileRenderPart {
        retained_chunk_id: "map-tile:clip".to_owned(),
        texture: MapTileCacheKey {
            source: boon_document::MapTileSourceId("fixture".to_owned()),
            z: 2,
            x: 1,
            y: 1,
            scale,
        },
        points: [[-20.0, -10.0], [90.0, -10.0], [90.0, 90.0], [-20.0, 90.0]],
        uvs: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        clip: Rect {
            x: 10.0,
            y: 15.0,
            width: 60.0,
            height: 50.0,
        },
    };
    let batches = quad_batches_from_map_tile_part(&part, 100, 100);
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].vertices.len(), 6);
    for vertex in batches[0].vertices.iter() {
        let x = (vertex.position[0] + 1.0) * 50.0;
        let y = (1.0 - vertex.position[1]) * 50.0;
        assert!((9.99..=70.01).contains(&x), "clipped x was {x}");
        assert!((14.99..=65.01).contains(&y), "clipped y was {y}");
    }
}

#[test]
fn retained_map_tiles_and_overlays_render_into_app_owned_wgpu_pixels() {
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
                eprintln!("skipping map readback test: request_adapter failed: {error}");
                return;
            }
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-native-gpu-map-readback-test-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
            })
            .await
            .expect("test WGPU device should be available when adapter exists");
        let frame = generic_native_map_frame();
        let (scene, identity) = test_document_scene_from_layout_frame(&frame, 128, 96);
        assert_eq!(scene.map_viewports.len(), 1);
        let artifact_dir = Path::new("target/artifacts/native-gpu/tests");
        let mut renderer = AppOwnedProofRenderer::new(&device, &queue);

        renderer
            .render_scene_pixels(AppOwnedRenderSceneRequest {
                device: &device,
                queue: &queue,
                scene: &scene,
                render_identity_hash: &identity,
                surface_id: SurfaceId("generic-map-readback".to_owned()),
                surface_epoch: 1,
                width: 128,
                height: 96,
                artifact_dir,
                artifact_label: "generic-map-before-tiles",
            })
            .expect("map placeholder and overlays should render before tiles arrive");
        let requests = renderer.take_map_tile_requests(64);
        assert!(!requests.is_empty());
        for request in requests {
            renderer
                .submit_map_tile(DecodedMapTile {
                    viewport: request.viewport,
                    identity: request.identity,
                    width: 4,
                    height: 4,
                    rgba: [238, 30, 190, 255].repeat(16),
                })
                .expect("fixture tile should enter the bounded decoded cache");
        }
        let prepared = renderer
            .prepare_map_tile_uploads(&device, &queue)
            .expect("tile upload should complete outside the product interaction frame");
        assert!(prepared.upload_count > 0);
        let proof = renderer
            .render_scene_pixels(AppOwnedRenderSceneRequest {
                device: &device,
                queue: &queue,
                scene: &scene,
                render_identity_hash: &identity,
                surface_id: SurfaceId("generic-map-readback".to_owned()),
                surface_epoch: 1,
                width: 128,
                height: 96,
                artifact_dir,
                artifact_label: "generic-map-with-tiles",
            })
            .expect("decoded map tiles should render through the product graph");
        let RenderProofArtifact::AppOwnedPixels { artifact_path, .. } = proof.artifact else {
            panic!("expected app-owned map pixel artifact");
        };
        let pixels = image::open(&artifact_path)
            .expect("map readback PNG should decode")
            .to_rgba8();
        let magenta_pixels = pixels
            .pixels()
            .filter(|pixel| pixel[0] > 180 && pixel[2] > 140 && pixel[1] < 80)
            .count();
        assert!(
            magenta_pixels > 1_000,
            "fixture tiles should occupy a nonblank retained map surface, got {magenta_pixels} pixels"
        );
        assert!(proof.metrics.map_tiles.ready_visible_tile_count > 0);
        assert_eq!(proof.metrics.map_tile_upload_count, 0);
        assert!(proof.metrics.map_tile_gpu_cache_hits > 0);
        assert!(proof.metrics.map_tile_gpu_cache_entry_count > 0);
    });
}
