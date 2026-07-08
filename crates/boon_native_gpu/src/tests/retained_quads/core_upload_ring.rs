// Included by `../tests.rs`; kept in the parent test module for private renderer helper access.

#[test]
fn native_gpu_quad_vertex_pod_layout_matches_shader_locations() {
    assert_eq!(std::mem::size_of::<NativeGpuQuadVertex>(), 20);
    assert_eq!(std::mem::align_of::<NativeGpuQuadVertex>(), 4);
    assert_eq!(NATIVE_GPU_QUAD_VERTEX_STRIDE, 20);
    assert_eq!(NATIVE_GPU_QUAD_VERTEX_POSITION_OFFSET, 0);
    assert_eq!(NATIVE_GPU_QUAD_VERTEX_COLOR_OFFSET, 8);
    assert_eq!(NATIVE_GPU_QUAD_VERTEX_UV_OFFSET, 12);

    let layout = native_gpu_quad_vertex_buffer_layout();
    assert_eq!(layout.array_stride, NATIVE_GPU_QUAD_VERTEX_STRIDE);
    assert_eq!(layout.step_mode, wgpu::VertexStepMode::Vertex);
    assert_eq!(layout.attributes.len(), 3);
    assert_eq!(
        layout.attributes[0],
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: NATIVE_GPU_QUAD_VERTEX_POSITION_OFFSET,
            shader_location: 0,
        }
    );
    assert_eq!(
        layout.attributes[1],
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Uint32,
            offset: NATIVE_GPU_QUAD_VERTEX_COLOR_OFFSET,
            shader_location: 1,
        }
    );
    assert_eq!(
        layout.attributes[2],
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: NATIVE_GPU_QUAD_VERTEX_UV_OFFSET,
            shader_location: 2,
        }
    );

    let generated = generated::shader_bindings::native_gpu_rect::vs_main_entry(
        wgpu::VertexStepMode::Vertex,
        wgpu::VertexStepMode::Vertex,
        wgpu::VertexStepMode::Vertex,
    );
    let generated_inputs = generated
        .buffers
        .iter()
        .flat_map(|buffer| buffer.attributes.iter().copied())
        .map(|attribute| (attribute.shader_location, attribute.format))
        .collect::<Vec<_>>();
    assert_eq!(
        generated_inputs,
        NATIVE_GPU_QUAD_VERTEX_ATTRIBUTES
            .iter()
            .map(|attribute| (attribute.shader_location, attribute.format))
            .collect::<Vec<_>>(),
        "the host-interleaved POD buffer must feed the same generated shader locations and formats"
    );
}


#[test]
fn split_document_quad_batch_interleaves_without_value_drift() {
    let batch = boon_document::RenderQuadBatch {
        retained_chunk_id: None,
        texture: RenderTextureRef::Solid,
        positions: vec![1.0, 2.0, 3.0, 4.0],
        colors: vec![0x4433_2211, 0x8877_6655],
        uvs: vec![0.25, 0.5, 0.75, 1.0],
    };

    let converted = quad_batch_from_document_batch(&batch, 0);

    assert_eq!(
        converted.vertices,
        vec![
            NativeGpuQuadVertex {
                position: [1.0, 2.0],
                color: 0x4433_2211,
                uv: [0.25, 0.5],
            },
            NativeGpuQuadVertex {
                position: [3.0, 4.0],
                color: 0x8877_6655,
                uv: [0.75, 1.0],
            },
        ]
    );
}


#[test]
fn quad_upload_ring_preserves_cached_ranges_until_growth_is_needed() {
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
                eprintln!("skipping quad upload ring test: request_adapter failed: {error}");
                return;
            }
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-native-gpu-quad-upload-ring-test-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
            })
            .await
            .expect("test WGPU device should be available when adapter exists");

        let mut ring = QuadUploadRing::default();
        let mut cache = BTreeMap::new();
        let first_vertices = vec![
            NativeGpuQuadVertex {
                position: [0.0, 0.0],
                color: 0xff00_ffff,
                uv: [0.0, 0.0],
            };
            5_000
        ];
        let first_bytes = bytemuck::cast_slice(&first_vertices);
        let first_key = QuadBatchCacheKey {
            retained_chunk_id: "test-first".to_owned(),
            texture: QuadTexture::Solid,
            vertex_count: first_vertices.len() as u32,
            content_key: quad_batch_content_key(first_bytes),
        };
        let first_begin_stats = ring
            .begin_frame(
                &device,
                quad_upload_reservation_size(first_bytes.len() as u64),
                quad_upload_reservation_size(first_bytes.len() as u64),
                Some(&mut cache),
            )
            .expect("first frame should reserve the minimum ring");
        let (first_batch, first_upload_stats) = ring
            .upload_reserved(
                &queue,
                first_bytes,
                first_vertices.len() as u32,
                Some("test-first".to_owned()),
            )
            .expect("first upload should fit the reserved ring");
        cache.insert(first_key, first_batch);

        assert_eq!(
            first_begin_stats.allocated_gpu_bytes,
            QUAD_UPLOAD_RING_GROW_ON_WRAP_MIN_BYTES
        );
        assert_eq!(first_begin_stats.staging_wrap_count, 0);
        assert_eq!(first_begin_stats.queue_write_count, 0);
        assert_eq!(first_upload_stats.queue_write_count, 1);
        assert_eq!(first_upload_stats.dirty_upload_ranges.len(), 1);
        assert_eq!(first_upload_stats.dirty_upload_ranges[0].offset, 0);
        assert_eq!(
            first_upload_stats.dirty_upload_ranges[0].size,
            first_bytes.len() as u64
        );
        assert_eq!(
            first_upload_stats.dirty_upload_ranges[0]
                .retained_chunk_id
                .as_deref(),
            Some("test-first")
        );
        assert_eq!(cache.len(), 1);

        let second_vertices = vec![
            NativeGpuQuadVertex {
                position: [1.0, 1.0],
                color: 0xffff_00ff,
                uv: [1.0, 1.0],
            };
            10_000
        ];
        let second_bytes = bytemuck::cast_slice(&second_vertices);
        let second_begin_stats = ring
            .begin_frame(
                &device,
                quad_upload_reservation_size(second_bytes.len() as u64),
                quad_upload_reservation_size(second_bytes.len() as u64),
                Some(&mut cache),
            )
            .expect("second frame should fit the interaction-sized retained ring");
        let (_second_batch, second_upload_stats) = ring
            .upload_reserved(
                &queue,
                second_bytes,
                second_vertices.len() as u32,
                Some("test-second".to_owned()),
            )
            .expect("second upload should fit without invalidating cached ranges");

        assert_eq!(second_begin_stats.allocated_gpu_bytes, 0);
        assert_eq!(second_begin_stats.staging_wrap_count, 0);
        assert_eq!(second_begin_stats.cache_eviction_count, 0);
        assert!(!second_begin_stats.invalidated_cached_ranges);
        assert_eq!(second_upload_stats.queue_write_count, 1);
        assert_eq!(second_upload_stats.dirty_upload_ranges.len(), 1);
        assert_eq!(
            second_upload_stats.dirty_upload_ranges[0].offset,
            quad_upload_reservation_size(first_bytes.len() as u64)
        );
        assert_eq!(
            second_upload_stats.dirty_upload_ranges[0].size,
            second_bytes.len() as u64
        );
        assert_eq!(
            second_upload_stats.dirty_upload_ranges[0].ring_generation,
            first_upload_stats.dirty_upload_ranges[0].ring_generation
        );
        assert!(
            !cache.is_empty(),
            "retained ranges should survive normal small interaction uploads"
        );

        ring.cursor_bytes = ring
            .capacity_bytes
            .saturating_sub(quad_upload_reservation_size(second_bytes.len() as u64) / 2);
        let growth_begin_stats = ring
            .begin_frame(
                &device,
                quad_upload_reservation_size(second_bytes.len() as u64),
                quad_upload_reservation_size(second_bytes.len() as u64),
                Some(&mut cache),
            )
            .expect("ring should grow before overwriting cached retained ranges");
        let (_growth_batch, growth_upload_stats) = ring
            .upload_reserved(
                &queue,
                second_bytes,
                second_vertices.len() as u32,
                Some("test-growth".to_owned()),
            )
            .expect("growth upload should fit the expanded ring");

        assert!(growth_begin_stats.allocated_gpu_bytes > QUAD_UPLOAD_RING_GROW_ON_WRAP_MIN_BYTES);
        assert_eq!(growth_begin_stats.staging_wrap_count, 0);
        assert_eq!(growth_begin_stats.cache_eviction_count, 1);
        assert!(growth_begin_stats.invalidated_cached_ranges);
        assert_eq!(growth_upload_stats.dirty_upload_ranges[0].offset, 0);
        assert_eq!(
            growth_upload_stats.dirty_upload_ranges[0].ring_generation,
            first_upload_stats.dirty_upload_ranges[0].ring_generation + 1
        );
        assert!(
            cache.is_empty(),
            "growing the backing buffer must invalidate ranges stored in the old buffer"
        );
    });
}


#[test]
fn quad_upload_ring_grows_before_multi_batch_frame_can_overwrite_live_ranges() {
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
                eprintln!("skipping multi-batch upload ring test: request_adapter failed: {error}");
                return;
            }
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("boon-native-gpu-quad-upload-ring-multi-batch-test-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
            })
            .await
            .expect("test WGPU device should be available when adapter exists");

        let mut ring = QuadUploadRing::default();
        let first_vertices = vec![
            NativeGpuQuadVertex {
                position: [0.0, 0.0],
                color: 0xff00_00ff,
                uv: [0.0, 0.0],
            };
            10_000
        ];
        let second_vertices = vec![
            NativeGpuQuadVertex {
                position: [1.0, 1.0],
                color: 0x00ff_00ff,
                uv: [1.0, 1.0],
            };
            5_000
        ];
        let first_bytes = bytemuck::cast_slice(&first_vertices);
        let second_bytes = bytemuck::cast_slice(&second_vertices);
        let frame_reservation = quad_upload_reservation_size(first_bytes.len() as u64)
            .saturating_add(quad_upload_reservation_size(second_bytes.len() as u64));
        let begin_stats = ring
            .begin_frame(&device, frame_reservation, frame_reservation, None)
            .expect("large multi-batch frame should reserve enough ring space up front");
        assert!(begin_stats.allocated_gpu_bytes >= QUAD_UPLOAD_RING_GROW_ON_WRAP_MIN_BYTES);
        assert_eq!(begin_stats.staging_wrap_count, 0);

        let (_first_batch, first_upload) = ring
            .upload_reserved(
                &queue,
                first_bytes,
                first_vertices.len() as u32,
                Some("test-first".to_owned()),
            )
            .expect("first frame batch should upload into reserved ring");
        let (_second_batch, second_upload) = ring
            .upload_reserved(
                &queue,
                second_bytes,
                second_vertices.len() as u32,
                Some("test-second".to_owned()),
            )
            .expect("second frame batch should upload without wrapping over the first batch");
        let first_range = &first_upload.dirty_upload_ranges[0];
        let second_range = &second_upload.dirty_upload_ranges[0];
        assert_eq!(first_range.offset, 0);
        assert_eq!(second_range.offset, first_range.size);
        assert_eq!(first_range.ring_generation, second_range.ring_generation);
    });
}


