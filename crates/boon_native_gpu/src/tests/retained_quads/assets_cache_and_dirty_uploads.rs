#[test]
fn svg_asset_data_url_renders_into_app_owned_pixels() {
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
                eprintln!("skipping SVG asset readback test: request_adapter failed: {error}");
                return;
            }
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-native-gpu-svg-asset-test-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
            })
            .await
            .expect("test WGPU device should be available when adapter exists");

        let mut style = StyleMap::new();
        style.insert(
            "asset_url".to_owned(),
            StyleValue::Text(
                "data:image/svg+xml;utf8,%3Csvg%20xmlns%3D%22http%3A//www.w3.org/2000/svg%22%20width%3D%2240%22%20height%3D%2240%22%3E%3Crect%20x%3D%228%22%20y%3D%228%22%20width%3D%2224%22%20height%3D%2224%22%20fill%3D%22%2300ff00%22/%3E%3C/svg%3E".to_owned(),
            ),
        );
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("svg-asset".to_owned()),
                kind: DocumentNodeKind::Stack,
                bounds: Rect {
                    x: 20.0,
                    y: 20.0,
                    width: 40.0,
                    height: 40.0,
                },
                text: None,
                style,
                focused: false,
                style_identity: test_style_identity(),
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            materialization: Vec::new(),
            metrics: LayoutMetrics::default(),
        };
        let artifact_dir = Path::new("target/artifacts/native-gpu/tests");
        let mut columns = GlyphonRenderTextColumnMeasurer::new();
        let scene = boon_document::render_scene::lower_layout_frame_to_render_scene(
            &frame,
            80,
            80,
            &mut columns,
        );
        let render_identity_hash = format!(
            "{:x}",
            Sha256::digest(format!("{scene:?}").as_bytes())
        );
        let proof = render_app_owned_scene_pixels(AppOwnedRenderSceneRequest {
            device: &device,
            queue: &queue,
            scene: &scene,
            render_identity_hash: &render_identity_hash,
            surface_id: SurfaceId("svg-asset-test".to_owned()),
            surface_epoch: 1,
            width: 80,
            height: 80,
            artifact_dir,
            artifact_label: "svg-asset-readback",
        })
        .expect("SVG asset frame should render to app-owned pixels");
        let RenderProofArtifact::AppOwnedPixels { artifact_path, .. } = proof.artifact else {
            panic!("expected app-owned pixel artifact");
        };
        let image = image::open(&artifact_path)
            .expect("readback PNG should decode")
            .to_rgba8();
        let center = image.get_pixel(40, 40).0;
        assert!(
            center[1] > center[0].saturating_add(48) && center[1] > center[2].saturating_add(48),
            "SVG asset center should be green-dominant after texture rendering, got {center:?}"
        );
        assert!(
            proof.metrics.draw_calls >= 2,
            "asset rendering should add a textured batch draw call"
        );
    });
}


#[test]
fn asset_cache_reports_hits_and_avoids_repeat_raster_upload_for_known_svg() {
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
                eprintln!("skipping SVG asset cache test: request_adapter failed: {error}");
                return;
            }
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-native-gpu-svg-asset-cache-test-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
            })
            .await
            .expect("test WGPU device should be available when adapter exists");

        let mut style = StyleMap::new();
        style.insert(
            "asset_url".to_owned(),
            StyleValue::Text(
                "data:image/svg+xml;utf8,%3Csvg%20xmlns%3D%22http%3A//www.w3.org/2000/svg%22%20width%3D%2240%22%20height%3D%2240%22%3E%3Crect%20x%3D%228%22%20y%3D%228%22%20width%3D%2224%22%20height%3D%2224%22%20fill%3D%22%2300ff00%22/%3E%3C/svg%3E".to_owned(),
            ),
        );
        let frame = LayoutFrame {
            display_list: vec![DisplayItem {
                node: DocumentNodeId("svg-asset-cache".to_owned()),
                kind: DocumentNodeKind::Stack,
                bounds: Rect {
                    x: 20.0,
                    y: 20.0,
                    width: 40.0,
                    height: 40.0,
                },
                text: None,
                style,
                focused: false,
                style_identity: test_style_identity(),
            }],
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: AccessibilityTree::default(),
            demands: Vec::new(),
            materialization: Vec::new(),
            metrics: LayoutMetrics::default(),
        };
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("boon-native-gpu-svg-asset-cache-target"),
            size: wgpu::Extent3d {
                width: 80,
                height: 80,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut renderer = VisibleLayoutRenderer::new(&device, &queue, format);
        let (scene, scene_identity) = test_document_scene_from_layout_frame(&frame, 80, 80);

        let mut first_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("boon-native-gpu-svg-asset-cache-first"),
        });
        let first = renderer
            .encode_scene(SurfaceRenderSceneRequest {
                device: &device,
                queue: &queue,
                encoder: &mut first_encoder,
                view: &view,
                scene: &scene,
                scene_identity: Some(&scene_identity),
                format,
                width: 80,
                height: 80,
            })
            .expect("first SVG asset frame should encode");
        queue.submit([first_encoder.finish()]);

        let mut second_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("boon-native-gpu-svg-asset-cache-second"),
        });
        let second = renderer
            .encode_scene(SurfaceRenderSceneRequest {
                device: &device,
                queue: &queue,
                encoder: &mut second_encoder,
                view: &view,
                scene: &scene,
                scene_identity: Some(&scene_identity),
                format,
                width: 80,
                height: 80,
            })
            .expect("second SVG asset frame should encode");
        queue.submit([second_encoder.finish()]);

        assert_eq!(first.asset_ref_count, 1);
        assert_eq!(first.asset_cache_misses, 1);
        assert_eq!(first.asset_decode_count, 1);
        assert_eq!(first.asset_raster_count, 1);
        assert_eq!(first.asset_upload_count, 1);
        assert!(first.asset_upload_bytes >= 40 * 40 * 4);
        assert_eq!(first.asset_failure_diagnostics, Vec::<String>::new());
        assert!(first.queue_write_count > 0);
        assert_eq!(first.queue_write_count, first.dirty_upload_range_count);
        assert_eq!(
            first.dirty_upload_ranges.len(),
            first.dirty_upload_range_count as usize
        );
        assert!(
            first.queue_write_count < first.dirty_upload_range_count.saturating_mul(3),
            "interleaved POD uploads should use one queue write per dirty batch"
        );
        assert!(first.allocated_gpu_bytes >= first.upload_bytes);
        assert_eq!(first.upload_bytes % NATIVE_GPU_QUAD_VERTEX_STRIDE, 0);
        assert_eq!(
            first
                .dirty_upload_ranges
                .iter()
                .map(|range| range.size)
                .sum::<u64>(),
            first.upload_bytes
        );
        assert_eq!(first.buffer_reuse_count, 0);
        assert_eq!(first.staging_wrap_count, 0);
        assert_eq!(first.quad_cache_eviction_count, 0);

        assert_eq!(second.asset_ref_count, 1);
        assert!(second.asset_cache_hits >= 1);
        assert_eq!(second.asset_cache_misses, 0);
        assert_eq!(second.asset_decode_count, 0);
        assert_eq!(second.asset_raster_count, 0);
        assert_eq!(second.asset_upload_count, 0);
        assert_eq!(second.asset_upload_bytes, 0);
        assert_eq!(second.asset_cache_entry_count, 1);
        assert!(second.asset_cache_byte_count >= 40 * 40 * 4);
        assert_eq!(first.asset_refs, second.asset_refs);
        assert_eq!(second.queue_write_count, 0);
        assert_eq!(second.dirty_upload_range_count, 0);
        assert!(second.dirty_upload_ranges.is_empty());
        assert_eq!(second.upload_bytes, 0);
        assert_eq!(second.allocated_gpu_bytes, 0);
        assert!(second.buffer_reuse_count >= 1);
        assert_eq!(second.staging_wrap_count, 0);
        assert_eq!(second.quad_cache_eviction_count, 0);
    });
}


#[test]
fn renderer_uploads_only_changed_retained_chunk_after_document_scene_interaction() {
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
                    "skipping retained chunk dirty upload test: request_adapter failed: {error}"
                );
                return;
            }
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-native-gpu-retained-chunk-upload-test-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
            })
            .await
            .expect("test WGPU device should be available when adapter exists");

        fn retained_chunk_test_scene(right_color: [u8; 4]) -> DocumentRenderScene {
            let style_identity = test_style_identity();
            let item = |node: &str, x: f32| boon_document::RenderSceneItem {
                node: DocumentNodeId(node.to_owned()),
                retained_chunk_id: format!("chunk:{node}"),
                source_kind: DocumentNodeKind::Stack,
                bounds: Rect {
                    x,
                    y: 12.0,
                    width: 48.0,
                    height: 36.0,
                },
                clip: None,
                transform: [1.0, 0.0, 0.0, 1.0, x, 12.0],
                style_identity,
                dependency_set: vec![format!("node:{node}")],
                texture_asset_refs: Vec::new(),
                estimated_vertex_count: 6,
            };
            let primitive = |node: &str, x: f32, color: [u8; 4]| RenderVisualPrimitive {
                node: DocumentNodeId(node.to_owned()),
                retained_chunk_id: format!("chunk:{node}"),
                source_kind: DocumentNodeKind::Stack,
                primitive: RenderVisualPrimitiveKind::Fill,
                bounds: Rect {
                    x,
                    y: 12.0,
                    width: 48.0,
                    height: 36.0,
                },
                clip: None,
                radius: 0.0,
                stroke_width: 0.0,
                color,
                secondary_color: [0, 0, 0, 0],
                antialias: 0.0,
                control_points: Vec::new(),
                texture: RenderTextureRef::Solid,
                style_identity,
                dependency_set: vec![format!("primitive:{node}:fill")],
            };
            DocumentRenderScene {
                viewport: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 128.0,
                    height: 72.0,
                },
                items: vec![item("left", 12.0), item("right", 68.0)],
                visual_primitives: vec![
                    primitive("left", 12.0, [30, 90, 150, 255]),
                    primitive("right", 68.0, right_color),
                ],
                quad_batches: Vec::new(),
                text_runs: Vec::new(),
                metrics: boon_document::RenderSceneMetrics {
                    visible_source_item_count: 2,
                    visual_primitive_count: 2,
                    rendered_rect_count: 2,
                    cap_hit: false,
                },
            }
        }

        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("boon-native-gpu-retained-chunk-upload-target"),
            size: wgpu::Extent3d {
                width: 128,
                height: 72,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut renderer = VisibleLayoutRenderer::new(&device, &queue, format);
        let first_scene = retained_chunk_test_scene([170, 80, 40, 255]);
        let second_scene = retained_chunk_test_scene([210, 110, 60, 255]);

        let mut first_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("boon-native-gpu-retained-chunk-upload-first"),
        });
        let first = renderer
            .encode_scene(SurfaceRenderSceneRequest {
                device: &device,
                queue: &queue,
                encoder: &mut first_encoder,
                view: &view,
                scene: &first_scene,
                scene_identity: None,
                format,
                width: 128,
                height: 72,
            })
            .expect("first retained chunk scene should encode");
        queue.submit([first_encoder.finish()]);

        let mut second_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("boon-native-gpu-retained-chunk-upload-second"),
        });
        let second = renderer
            .encode_scene(SurfaceRenderSceneRequest {
                device: &device,
                queue: &queue,
                encoder: &mut second_encoder,
                view: &view,
                scene: &second_scene,
                scene_identity: None,
                format,
                width: 128,
                height: 72,
            })
            .expect("second retained chunk scene should encode");
        queue.submit([second_encoder.finish()]);

        assert_eq!(first.dirty_upload_chunk_count, 2);
        assert_eq!(
            first.dirty_upload_chunk_ids,
            vec!["chunk:left", "chunk:right"]
        );
        assert_eq!(second.dirty_upload_range_count, 1);
        assert_eq!(second.dirty_upload_chunk_count, 1);
        assert_eq!(second.dirty_upload_chunk_ids, vec!["chunk:right"]);
        assert_eq!(
            second.dirty_upload_ranges[0].retained_chunk_id.as_deref(),
            Some("chunk:right")
        );
        assert_eq!(second.queue_write_count, 1);
        assert!(second.buffer_reuse_count >= 1);
        assert!(
            second.upload_bytes < first.upload_bytes,
            "one changed retained chunk should not upload the whole scene again: first={} second={}",
            first.upload_bytes,
            second.upload_bytes
        );
        assert_eq!(second.staging_wrap_count, 0);
        assert_eq!(second.quad_cache_eviction_count, 0);
    });
}


#[test]
fn renderer_reuses_prepared_quad_cache_across_alternating_scene_identities() {
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
                    "skipping alternating prepared quad cache test: request_adapter failed: {error}"
                );
                return;
            }
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-native-gpu-prepared-quad-cache-test-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
            })
            .await
            .expect("test WGPU device should be available when adapter exists");

        fn prepared_cache_scene(node: &str, color: [u8; 4]) -> DocumentRenderScene {
            let style_identity = test_style_identity();
            DocumentRenderScene {
                viewport: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 96.0,
                    height: 64.0,
                },
                items: vec![boon_document::RenderSceneItem {
                    node: DocumentNodeId(node.to_owned()),
                    retained_chunk_id: format!("chunk:{node}"),
                    source_kind: DocumentNodeKind::Stack,
                    bounds: Rect {
                        x: 16.0,
                        y: 12.0,
                        width: 64.0,
                        height: 40.0,
                    },
                    clip: None,
                    transform: [1.0, 0.0, 0.0, 1.0, 16.0, 12.0],
                    style_identity,
                    dependency_set: vec![format!("node:{node}")],
                    texture_asset_refs: Vec::new(),
                    estimated_vertex_count: 6,
                }],
                visual_primitives: vec![RenderVisualPrimitive {
                    node: DocumentNodeId(node.to_owned()),
                    retained_chunk_id: format!("chunk:{node}"),
                    source_kind: DocumentNodeKind::Stack,
                    primitive: RenderVisualPrimitiveKind::Fill,
                    bounds: Rect {
                        x: 16.0,
                        y: 12.0,
                        width: 64.0,
                        height: 40.0,
                    },
                    clip: None,
                    radius: 0.0,
                    stroke_width: 0.0,
                    color,
                    secondary_color: [0, 0, 0, 0],
                    antialias: 0.0,
                    control_points: Vec::new(),
                    texture: RenderTextureRef::Solid,
                    style_identity,
                    dependency_set: vec![format!("primitive:{node}:fill")],
                }],
                quad_batches: Vec::new(),
                text_runs: Vec::new(),
                metrics: boon_document::RenderSceneMetrics {
                    visible_source_item_count: 1,
                    visual_primitive_count: 1,
                    rendered_rect_count: 1,
                    cap_hit: false,
                },
            }
        }

        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("boon-native-gpu-prepared-quad-cache-target"),
            size: wgpu::Extent3d {
                width: 96,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut renderer = VisibleLayoutRenderer::new(&device, &queue, format);
        let scene_a = prepared_cache_scene("selected-a", [70, 120, 230, 255]);
        let scene_b = prepared_cache_scene("selected-b", [230, 120, 70, 255]);

        let mut first_a_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("boon-native-gpu-prepared-quad-cache-first-a"),
        });
        let first_a = renderer
            .encode_scene(SurfaceRenderSceneRequest {
                device: &device,
                queue: &queue,
                encoder: &mut first_a_encoder,
                view: &view,
                scene: &scene_a,
                scene_identity: Some("scene-a"),
                format,
                width: 96,
                height: 64,
            })
            .expect("first scene A should encode");
        queue.submit([first_a_encoder.finish()]);

        let mut first_b_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("boon-native-gpu-prepared-quad-cache-first-b"),
        });
        let first_b = renderer
            .encode_scene(SurfaceRenderSceneRequest {
                device: &device,
                queue: &queue,
                encoder: &mut first_b_encoder,
                view: &view,
                scene: &scene_b,
                scene_identity: Some("scene-b"),
                format,
                width: 96,
                height: 64,
            })
            .expect("first scene B should encode");
        queue.submit([first_b_encoder.finish()]);

        let mut second_a_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("boon-native-gpu-prepared-quad-cache-second-a"),
        });
        let second_a = renderer
            .encode_scene(SurfaceRenderSceneRequest {
                device: &device,
                queue: &queue,
                encoder: &mut second_a_encoder,
                view: &view,
                scene: &scene_a,
                scene_identity: Some("scene-a"),
                format,
                width: 96,
                height: 64,
            })
            .expect("second scene A should encode");
        queue.submit([second_a_encoder.finish()]);

        assert!(!first_a.quad_cache_hit);
        assert!(!first_b.quad_cache_hit);
        assert!(second_a.quad_cache_hit);
        assert_eq!(second_a.queue_write_count, 0);
        assert_eq!(second_a.upload_bytes, 0);
        assert!(second_a.buffer_reuse_count >= 1);
    });
}


#[test]
fn coalesced_quad_draw_ranges_merge_only_adjacent_compatible_batches() {
    let ranges = coalesced_gpu_quad_draw_ranges_from_parts([
        GpuQuadDrawRange {
            texture: QuadTexture::Solid,
            vertex_count: 6,
            byte_range: 0..96,
            ring_generation: 7,
            first_batch_index: 0,
            source_batch_count: 1,
        },
        GpuQuadDrawRange {
            texture: QuadTexture::Solid,
            vertex_count: 12,
            byte_range: 96..288,
            ring_generation: 7,
            first_batch_index: 1,
            source_batch_count: 1,
        },
        GpuQuadDrawRange {
            texture: QuadTexture::Solid,
            vertex_count: 6,
            byte_range: 320..416,
            ring_generation: 7,
            first_batch_index: 2,
            source_batch_count: 1,
        },
        GpuQuadDrawRange {
            texture: QuadTexture::Solid,
            vertex_count: 6,
            byte_range: 416..512,
            ring_generation: 8,
            first_batch_index: 3,
            source_batch_count: 1,
        },
    ]);

    assert_eq!(ranges.len(), 3);
    assert_eq!(ranges[0].byte_range, 0..288);
    assert_eq!(ranges[0].vertex_count, 18);
    assert_eq!(ranges[0].source_batch_count, 2);
    assert_eq!(ranges[1].byte_range, 320..416);
    assert_eq!(ranges[2].ring_generation, 8);
}
