use serde::{Deserialize, Serialize};

pub const STATIC_INDEX_HTML: &str = include_str!("../static/index.html");
pub const STATIC_WORLD_SCENE_HOST_JS: &str = include_str!("../static/world_scene_host.js");
pub const RETAINED_SCENE_PACKET_SCHEMA_VERSION: u32 = 1;
pub const SURFACE_REPRESENTATION_PACKET_ENCODING: &str =
    "serde-externally-tagged-SurfaceRepresentation";
pub const WEB_HOST_SOURCE_BUDGET_BYTES: usize = 32_000;
pub const WEB_HOST_VERTEX_STRIDE_BYTES: u32 = 80;
pub const WEB_HOST_INDEX_STRIDE_BYTES: u32 = 4;
pub const WEB_HOST_CAMERA_UNIFORM_BYTES: u32 = 64;
pub const WEB_HOST_DRAW_DESCRIPTOR_BYTES: u32 = 12;
pub const RETAINED_COMMAND_STREAM_WORD_COUNT: u32 = 8;
pub const RETAINED_COMMAND_STREAM_WORD_MAGIC: u32 = 0xB00C_0001;
pub const RETAINED_PACKED_BUFFER_LAYOUT_WORD_COUNT: u32 = 12;
pub const RETAINED_PACKED_BUFFER_LAYOUT_MAGIC: u32 = 0xB00C_0002;
pub const SUPPORTED_SURFACE_REPRESENTATION_TAGS: [&str; 3] = [
    "IndexedMesh",
    "IndexedMeshSummary",
    "DirectedDualGridSummary",
];

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_scene_packet_schema_version() -> u32 {
    RETAINED_SCENE_PACKET_SCHEMA_VERSION
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_vertex_stride_bytes() -> u32 {
    WEB_HOST_VERTEX_STRIDE_BYTES
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_index_stride_bytes() -> u32 {
    WEB_HOST_INDEX_STRIDE_BYTES
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_camera_uniform_bytes() -> u32 {
    WEB_HOST_CAMERA_UNIFORM_BYTES
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_draw_descriptor_bytes(draw_count: u32) -> u32 {
    draw_count.saturating_mul(WEB_HOST_DRAW_DESCRIPTOR_BYTES)
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_static_source_budget_bytes() -> u32 {
    WEB_HOST_SOURCE_BUDGET_BYTES as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_vertex_buffer_bytes(vertex_count: u32) -> u32 {
    vertex_count.saturating_mul(WEB_HOST_VERTEX_STRIDE_BYTES)
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_index_buffer_bytes(index_count: u32) -> u32 {
    index_count.saturating_mul(WEB_HOST_INDEX_STRIDE_BYTES)
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_upload_total_bytes(
    vertex_count: u32,
    index_count: u32,
) -> u32 {
    boon_web_host_retained_vertex_buffer_bytes(vertex_count)
        .saturating_add(boon_web_host_retained_index_buffer_bytes(index_count))
        .saturating_add(WEB_HOST_CAMERA_UNIFORM_BYTES)
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_packed_buffer_layout_total_bytes(
    packed_vertex_count: u32,
    packed_index_count: u32,
    packed_draw_count: u32,
) -> u32 {
    boon_web_host_retained_vertex_buffer_bytes(packed_vertex_count)
        .saturating_add(boon_web_host_retained_index_buffer_bytes(
            packed_index_count,
        ))
        .saturating_add(WEB_HOST_CAMERA_UNIFORM_BYTES)
        .saturating_add(boon_web_host_draw_descriptor_bytes(packed_draw_count))
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_upload_plan_valid(
    vertex_count: u32,
    index_count: u32,
    chunk_count: u32,
    draw_count: u32,
    total_upload_bytes: u32,
) -> u32 {
    let expected_total = boon_web_host_retained_upload_total_bytes(vertex_count, index_count);
    u32::from(
        vertex_count > 0
            && index_count > 0
            && chunk_count > 0
            && draw_count > 0
            && draw_count <= chunk_count
            && total_upload_bytes == expected_total,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_upload_plan_fingerprint(
    vertex_count: u32,
    index_count: u32,
    chunk_count: u32,
    draw_count: u32,
    total_upload_bytes: u32,
) -> u32 {
    let mut hash = 2_166_136_261_u32;
    for value in [
        RETAINED_SCENE_PACKET_SCHEMA_VERSION,
        WEB_HOST_VERTEX_STRIDE_BYTES,
        WEB_HOST_INDEX_STRIDE_BYTES,
        WEB_HOST_CAMERA_UNIFORM_BYTES,
        vertex_count,
        index_count,
        chunk_count,
        draw_count,
        total_upload_bytes,
        boon_web_host_retained_upload_plan_valid(
            vertex_count,
            index_count,
            chunk_count,
            draw_count,
            total_upload_bytes,
        ),
    ] {
        for byte in value.to_le_bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    hash
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_packed_buffer_layout_word_count() -> u32 {
    RETAINED_PACKED_BUFFER_LAYOUT_WORD_COUNT
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_packed_buffer_layout_word_at(
    index: u32,
    packed_vertex_count: u32,
    packed_index_count: u32,
    packed_draw_count: u32,
) -> u32 {
    let vertex_offset = 0;
    let vertex_bytes = boon_web_host_retained_vertex_buffer_bytes(packed_vertex_count);
    let index_offset = vertex_offset + vertex_bytes;
    let index_bytes = boon_web_host_retained_index_buffer_bytes(packed_index_count);
    let camera_offset = index_offset.saturating_add(index_bytes);
    let draw_descriptor_offset = camera_offset.saturating_add(WEB_HOST_CAMERA_UNIFORM_BYTES);
    let draw_descriptor_bytes = boon_web_host_draw_descriptor_bytes(packed_draw_count);
    let total_bytes = draw_descriptor_offset.saturating_add(draw_descriptor_bytes);
    match index {
        0 => RETAINED_PACKED_BUFFER_LAYOUT_MAGIC,
        1 => vertex_offset,
        2 => vertex_bytes,
        3 => index_offset,
        4 => index_bytes,
        5 => camera_offset,
        6 => WEB_HOST_CAMERA_UNIFORM_BYTES,
        7 => draw_descriptor_offset,
        8 => draw_descriptor_bytes,
        9 => total_bytes,
        10 => WEB_HOST_VERTEX_STRIDE_BYTES,
        11 => WEB_HOST_INDEX_STRIDE_BYTES,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_packed_buffer_layout_valid(
    packed_vertex_count: u32,
    packed_index_count: u32,
    packed_draw_count: u32,
    total_bytes: u32,
) -> u32 {
    let expected_total = boon_web_host_retained_packed_buffer_layout_total_bytes(
        packed_vertex_count,
        packed_index_count,
        packed_draw_count,
    );
    let vertex_bytes = boon_web_host_retained_packed_buffer_layout_word_at(
        2,
        packed_vertex_count,
        packed_index_count,
        packed_draw_count,
    );
    let index_offset = boon_web_host_retained_packed_buffer_layout_word_at(
        3,
        packed_vertex_count,
        packed_index_count,
        packed_draw_count,
    );
    let index_bytes = boon_web_host_retained_packed_buffer_layout_word_at(
        4,
        packed_vertex_count,
        packed_index_count,
        packed_draw_count,
    );
    let camera_offset = boon_web_host_retained_packed_buffer_layout_word_at(
        5,
        packed_vertex_count,
        packed_index_count,
        packed_draw_count,
    );
    let draw_descriptor_offset = boon_web_host_retained_packed_buffer_layout_word_at(
        7,
        packed_vertex_count,
        packed_index_count,
        packed_draw_count,
    );
    let draw_descriptor_bytes = boon_web_host_draw_descriptor_bytes(packed_draw_count);
    u32::from(
        packed_vertex_count > 0
            && packed_index_count > 0
            && packed_index_count % 3 == 0
            && packed_draw_count > 0
            && vertex_bytes > 0
            && index_bytes > 0
            && draw_descriptor_bytes > 0
            && index_offset == vertex_bytes
            && camera_offset == index_offset.saturating_add(index_bytes)
            && draw_descriptor_offset
                == camera_offset.saturating_add(WEB_HOST_CAMERA_UNIFORM_BYTES)
            && total_bytes == expected_total,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_packed_buffer_layout_fingerprint(
    packed_vertex_count: u32,
    packed_index_count: u32,
    packed_draw_count: u32,
    total_bytes: u32,
) -> u32 {
    let mut hash = 2_166_136_261_u32;
    for index in 0..RETAINED_PACKED_BUFFER_LAYOUT_WORD_COUNT {
        let value = boon_web_host_retained_packed_buffer_layout_word_at(
            index,
            packed_vertex_count,
            packed_index_count,
            packed_draw_count,
        );
        for byte in value.to_le_bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    for value in [
        total_bytes,
        boon_web_host_retained_packed_buffer_layout_valid(
            packed_vertex_count,
            packed_index_count,
            packed_draw_count,
            total_bytes,
        ),
    ] {
        for byte in value.to_le_bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    hash
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_draw_plan_valid(
    packet_vertex_count: u32,
    packet_index_count: u32,
    packet_chunk_count: u32,
    packet_draw_count: u32,
    packed_vertex_count: u32,
    packed_index_count: u32,
    packed_draw_count: u32,
    selection_overlay_draw_count: u32,
    selection_outline_draw_count: u32,
    selection_restore_draw_count: u32,
) -> u32 {
    u32::from(
        packet_vertex_count > 0
            && packet_index_count > 0
            && packet_chunk_count > 0
            && packet_draw_count > 0
            && packed_vertex_count >= packet_vertex_count
            && packed_index_count >= packet_index_count
            && packed_index_count % 3 == 0
            && packed_draw_count >= packet_draw_count
            && packed_draw_count > 0
            && selection_outline_draw_count <= selection_overlay_draw_count
            && selection_restore_draw_count <= selection_overlay_draw_count,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_draw_plan_fingerprint(
    packet_vertex_count: u32,
    packet_index_count: u32,
    packet_chunk_count: u32,
    packet_draw_count: u32,
    packed_vertex_count: u32,
    packed_index_count: u32,
    packed_draw_count: u32,
    selection_overlay_draw_count: u32,
    selection_outline_draw_count: u32,
    selection_restore_draw_count: u32,
) -> u32 {
    let mut hash = 2_166_136_261_u32;
    for value in [
        RETAINED_SCENE_PACKET_SCHEMA_VERSION,
        WEB_HOST_VERTEX_STRIDE_BYTES,
        WEB_HOST_INDEX_STRIDE_BYTES,
        WEB_HOST_CAMERA_UNIFORM_BYTES,
        packet_vertex_count,
        packet_index_count,
        packet_chunk_count,
        packet_draw_count,
        packed_vertex_count,
        packed_index_count,
        packed_draw_count,
        selection_overlay_draw_count,
        selection_outline_draw_count,
        selection_restore_draw_count,
        boon_web_host_retained_draw_plan_valid(
            packet_vertex_count,
            packet_index_count,
            packet_chunk_count,
            packet_draw_count,
            packed_vertex_count,
            packed_index_count,
            packed_draw_count,
            selection_overlay_draw_count,
            selection_outline_draw_count,
            selection_restore_draw_count,
        ),
    ] {
        for byte in value.to_le_bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    hash
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_renderer_dispatch_valid(
    packed_vertex_count: u32,
    packed_index_count: u32,
    packed_draw_count: u32,
    selection_overlay_draw_count: u32,
    selection_outline_draw_count: u32,
    selection_restore_draw_count: u32,
) -> u32 {
    u32::from(
        packed_vertex_count > 0
            && packed_index_count > 0
            && packed_index_count % 3 == 0
            && packed_draw_count > 0
            && selection_outline_draw_count <= selection_overlay_draw_count
            && selection_restore_draw_count <= selection_overlay_draw_count,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_renderer_dispatch_fingerprint(
    packed_vertex_count: u32,
    packed_index_count: u32,
    packed_draw_count: u32,
    selection_overlay_draw_count: u32,
    selection_outline_draw_count: u32,
    selection_restore_draw_count: u32,
) -> u32 {
    let mut hash = 2_166_136_261_u32;
    for value in [
        RETAINED_SCENE_PACKET_SCHEMA_VERSION,
        WEB_HOST_VERTEX_STRIDE_BYTES,
        WEB_HOST_INDEX_STRIDE_BYTES,
        WEB_HOST_CAMERA_UNIFORM_BYTES,
        packed_vertex_count,
        packed_index_count,
        packed_draw_count,
        selection_overlay_draw_count,
        selection_outline_draw_count,
        selection_restore_draw_count,
        boon_web_host_retained_renderer_dispatch_valid(
            packed_vertex_count,
            packed_index_count,
            packed_draw_count,
            selection_overlay_draw_count,
            selection_outline_draw_count,
            selection_restore_draw_count,
        ),
    ] {
        for byte in value.to_le_bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    hash
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_packed_buffer_checksums_valid(
    packed_vertex_count: u32,
    packed_index_count: u32,
    packed_draw_count: u32,
    selection_overlay_draw_count: u32,
    selection_outline_draw_count: u32,
    selection_restore_draw_count: u32,
    packed_vertex_checksum: u32,
    packed_index_checksum: u32,
    camera_uniform_checksum: u32,
    packed_draw_descriptor_checksum: u32,
) -> u32 {
    u32::from(
        boon_web_host_retained_renderer_dispatch_valid(
            packed_vertex_count,
            packed_index_count,
            packed_draw_count,
            selection_overlay_draw_count,
            selection_outline_draw_count,
            selection_restore_draw_count,
        ) == 1
            && packed_vertex_checksum != 0
            && packed_index_checksum != 0
            && camera_uniform_checksum != 0
            && packed_draw_descriptor_checksum != 0,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_packed_buffer_checksums_fingerprint(
    packed_vertex_count: u32,
    packed_index_count: u32,
    packed_draw_count: u32,
    selection_overlay_draw_count: u32,
    selection_outline_draw_count: u32,
    selection_restore_draw_count: u32,
    packed_vertex_checksum: u32,
    packed_index_checksum: u32,
    camera_uniform_checksum: u32,
    packed_draw_descriptor_checksum: u32,
) -> u32 {
    let mut hash = 2_166_136_261_u32;
    for value in [
        RETAINED_SCENE_PACKET_SCHEMA_VERSION,
        WEB_HOST_VERTEX_STRIDE_BYTES,
        WEB_HOST_INDEX_STRIDE_BYTES,
        WEB_HOST_CAMERA_UNIFORM_BYTES,
        packed_vertex_count,
        packed_index_count,
        packed_draw_count,
        selection_overlay_draw_count,
        selection_outline_draw_count,
        selection_restore_draw_count,
        packed_vertex_checksum,
        packed_index_checksum,
        camera_uniform_checksum,
        packed_draw_descriptor_checksum,
        boon_web_host_retained_packed_buffer_checksums_valid(
            packed_vertex_count,
            packed_index_count,
            packed_draw_count,
            selection_overlay_draw_count,
            selection_outline_draw_count,
            selection_restore_draw_count,
            packed_vertex_checksum,
            packed_index_checksum,
            camera_uniform_checksum,
            packed_draw_descriptor_checksum,
        ),
    ] {
        for byte in value.to_le_bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    hash
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_command_stream_valid(
    render_pass_count: u32,
    color_attachment_count: u32,
    depth_attachment_count: u32,
    draw_count: u32,
    overlay_draw_count: u32,
    copy_texture_target_count: u32,
    queue_submit_count: u32,
) -> u32 {
    let expected_render_pass_count = 1 + u32::from(overlay_draw_count > 0);
    u32::from(
        render_pass_count == expected_render_pass_count
            && color_attachment_count == 4
            && depth_attachment_count == 1
            && draw_count > 0
            && copy_texture_target_count >= 5
            && queue_submit_count >= 1 + copy_texture_target_count,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_command_stream_fingerprint(
    render_pass_count: u32,
    color_attachment_count: u32,
    depth_attachment_count: u32,
    draw_count: u32,
    overlay_draw_count: u32,
    copy_texture_target_count: u32,
    queue_submit_count: u32,
) -> u32 {
    let mut hash = 2_166_136_261_u32;
    for value in [
        RETAINED_SCENE_PACKET_SCHEMA_VERSION,
        WEB_HOST_VERTEX_STRIDE_BYTES,
        WEB_HOST_INDEX_STRIDE_BYTES,
        WEB_HOST_CAMERA_UNIFORM_BYTES,
        render_pass_count,
        color_attachment_count,
        depth_attachment_count,
        draw_count,
        overlay_draw_count,
        copy_texture_target_count,
        queue_submit_count,
        boon_web_host_retained_command_stream_valid(
            render_pass_count,
            color_attachment_count,
            depth_attachment_count,
            draw_count,
            overlay_draw_count,
            copy_texture_target_count,
            queue_submit_count,
        ),
    ] {
        for byte in value.to_le_bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    hash
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_command_stream_word_count() -> u32 {
    RETAINED_COMMAND_STREAM_WORD_COUNT
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_command_stream_word_at(
    index: u32,
    render_pass_count: u32,
    color_attachment_count: u32,
    depth_attachment_count: u32,
    draw_count: u32,
    overlay_draw_count: u32,
    copy_texture_target_count: u32,
    queue_submit_count: u32,
) -> u32 {
    match index {
        0 => RETAINED_COMMAND_STREAM_WORD_MAGIC,
        1 => render_pass_count,
        2 => color_attachment_count,
        3 => depth_attachment_count,
        4 => draw_count,
        5 => overlay_draw_count,
        6 => copy_texture_target_count,
        7 => queue_submit_count,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn boon_web_host_retained_command_stream_words_fingerprint(
    render_pass_count: u32,
    color_attachment_count: u32,
    depth_attachment_count: u32,
    draw_count: u32,
    overlay_draw_count: u32,
    copy_texture_target_count: u32,
    queue_submit_count: u32,
) -> u32 {
    let mut hash = 2_166_136_261_u32;
    for index in 0..RETAINED_COMMAND_STREAM_WORD_COUNT {
        let value = boon_web_host_retained_command_stream_word_at(
            index,
            render_pass_count,
            color_attachment_count,
            depth_attachment_count,
            draw_count,
            overlay_draw_count,
            copy_texture_target_count,
            queue_submit_count,
        );
        for byte in value.to_le_bytes() {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    for byte in boon_web_host_retained_command_stream_valid(
        render_pass_count,
        color_attachment_count,
        depth_attachment_count,
        draw_count,
        overlay_draw_count,
        copy_texture_target_count,
        queue_submit_count,
    )
    .to_le_bytes()
    {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WebWorldScenePipelineContract {
    pub status: &'static str,
    pub shader_language: &'static str,
    pub required_features: Vec<&'static str>,
    pub required_limits_profile: &'static str,
    pub vertex_entry_point: &'static str,
    pub fragment_entry_point: &'static str,
    pub vertex_stride_bytes: usize,
    pub vertex_attributes: Vec<&'static str>,
    pub camera_uniform_size_bytes: usize,
    pub primitive_topology: &'static str,
    pub index_format: &'static str,
    pub color_format_policy: &'static str,
    pub required_color_formats: Vec<&'static str>,
    pub depth_format: &'static str,
    pub depth_compare: &'static str,
    pub feature_target_format: &'static str,
    pub pick_target_format: &'static str,
    pub normal_target_format: &'static str,
    pub multisample_count: u32,
    pub buffer_usages: Vec<&'static str>,
    pub texture_usages: Vec<&'static str>,
    pub uses_push_constants: bool,
    pub uses_storage_buffers: bool,
    pub uses_storage_textures: bool,
    pub uses_texture_sampling: bool,
    pub uses_timestamp_queries: bool,
    pub uses_indirect_draw: bool,
    pub browser_render_executed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WebHostArtifactManifest {
    pub status: &'static str,
    pub host_kind: &'static str,
    pub visual_surface: &'static str,
    pub semantic_dom_scope: &'static str,
    pub uses_webgpu: bool,
    pub uses_canvas_surface: bool,
    pub mirrors_visual_dom: bool,
    pub static_source_files: Vec<WebHostStaticFile>,
    pub browser_render_executed: bool,
    pub browser_capture_report_path: &'static str,
    pub native_browser_comparison_report_path: &'static str,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WebHostStaticFile {
    pub path: &'static str,
    pub byte_count: usize,
    pub role: &'static str,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WebRetainedScenePacket {
    pub schema_version: u32,
    pub surface_representation_encoding: &'static str,
    pub supported_surface_representation_tags: Vec<&'static str>,
    pub scene: boon_scene_model::WorldScene,
    pub chunks: Vec<boon_scene_model::SurfaceChunk>,
    pub chunk_count: usize,
    pub indexed_mesh_chunk_count: usize,
    pub vertex_count: usize,
    pub index_count: usize,
    pub index_multiple_of_three: bool,
    pub manufacturing_mesh_used: bool,
    pub browser_render_executed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WebRetainedSceneUploadPlan {
    pub status: &'static str,
    pub vertex_stride_bytes: usize,
    pub index_stride_bytes: usize,
    pub camera_uniform_size_bytes: usize,
    pub chunk_count: usize,
    pub draw_count: usize,
    pub vertex_count: usize,
    pub index_count: usize,
    pub vertex_buffer_bytes: usize,
    pub index_buffer_bytes: usize,
    pub camera_uniform_bytes: usize,
    pub total_upload_bytes: usize,
    pub uses_copy_dst_uploads: bool,
    pub browser_upload_executed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WebRetainedPackedBufferLayout {
    pub status: &'static str,
    pub word_count: u32,
    pub words: Vec<u32>,
    pub packed_vertex_count: usize,
    pub packed_index_count: usize,
    pub packed_draw_count: usize,
    pub vertex_offset_bytes: usize,
    pub vertex_buffer_bytes: usize,
    pub index_offset_bytes: usize,
    pub index_buffer_bytes: usize,
    pub camera_uniform_offset_bytes: usize,
    pub camera_uniform_bytes: usize,
    pub draw_descriptor_offset_bytes: usize,
    pub draw_descriptor_bytes: usize,
    pub total_bytes: usize,
    pub valid_by_wasm_contract: bool,
    pub wasm_fingerprint: u32,
    pub browser_upload_executed: bool,
    pub browser_render_executed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WebBrowserRenderSourceContract {
    pub status: &'static str,
    pub shader_source_present: bool,
    pub renderer_factory: &'static str,
    pub render_submit_function: &'static str,
    pub retained_mesh_packer: &'static str,
    pub vertex_stride_bytes: usize,
    pub index_format: &'static str,
    pub color_target_count: u32,
    pub depth_target_format: &'static str,
    pub normal_target_format: &'static str,
    pub feature_target_format: &'static str,
    pub pick_target_format: &'static str,
    pub uses_queue_write_buffer: bool,
    pub uses_draw_indexed: bool,
    pub uses_app_owned_readback_targets: bool,
    pub browser_upload_executed: bool,
    pub browser_render_executed: bool,
    pub browser_capture_executed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WebSemanticBridgeContract {
    pub status: &'static str,
    pub visual_surface: &'static str,
    pub semantic_dom_scope: &'static str,
    pub semantic_scene_type: &'static str,
    pub semantic_bridge_type: &'static str,
    pub source_dispatch_type: &'static str,
    pub live_bridge_function: &'static str,
    pub supported_input_events: Vec<&'static str>,
    pub supports_ime_endpoint: bool,
    pub supports_action_routes: bool,
    pub supports_source_dispatch: bool,
    pub supports_live_dom_mount: bool,
    pub supports_focus_sync: bool,
    pub supports_composition_events: bool,
    pub mirrors_visual_dom: bool,
    pub browser_render_executed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WebSemanticBridgeProof {
    pub status: &'static str,
    pub semantic_node_count: usize,
    pub dom_node_count: usize,
    pub ime_endpoint_count: usize,
    pub action_route_count: usize,
    pub source_routed_action_count: usize,
    pub source_dispatch_count: usize,
    pub visual_dom_node_count: usize,
    pub html_contains_visual_renderer_marker: bool,
    pub supports_press_dispatch: bool,
    pub supports_set_text_dispatch: bool,
    pub supports_replace_selected_text_dispatch: bool,
    pub supports_increment_dispatch: bool,
    pub supports_decrement_dispatch: bool,
    pub live_bridge_function_present: bool,
    pub live_bridge_mount_present: bool,
    pub live_bridge_dispatch_present: bool,
    pub live_bridge_focus_sync_present: bool,
    pub live_bridge_composition_present: bool,
    pub browser_render_executed: bool,
}

impl WebHostArtifactManifest {
    pub fn total_static_source_bytes(&self) -> usize {
        self.static_source_files
            .iter()
            .map(|file| file.byte_count)
            .sum()
    }
}

impl WebRetainedScenePacket {
    pub fn from_visual_scene(visual: boon_scene_model::SolidVisualScene) -> Self {
        let mut indexed_mesh_chunk_count = 0_usize;
        let mut vertex_count = 0_usize;
        let mut index_count = 0_usize;
        let mut index_multiple_of_three = true;

        for chunk in &visual.chunks {
            match &chunk.representation {
                boon_scene_model::SurfaceRepresentation::IndexedMesh(mesh) => {
                    indexed_mesh_chunk_count += 1;
                    vertex_count += mesh.vertices.len();
                    index_count += mesh.indices.len();
                    index_multiple_of_three &= mesh.indices.len() % 3 == 0;
                }
                boon_scene_model::SurfaceRepresentation::IndexedMeshSummary {
                    vertex_count: vertices,
                    index_count: indices,
                } => {
                    indexed_mesh_chunk_count += 1;
                    vertex_count += *vertices as usize;
                    index_count += *indices as usize;
                    index_multiple_of_three &= *indices % 3 == 0;
                }
                boon_scene_model::SurfaceRepresentation::DirectedDualGridSummary { .. } => {
                    index_multiple_of_three = false;
                }
            }
        }

        Self {
            schema_version: RETAINED_SCENE_PACKET_SCHEMA_VERSION,
            surface_representation_encoding: SURFACE_REPRESENTATION_PACKET_ENCODING,
            supported_surface_representation_tags: SUPPORTED_SURFACE_REPRESENTATION_TAGS.to_vec(),
            scene: visual.scene,
            chunk_count: visual.chunks.len(),
            chunks: visual.chunks,
            indexed_mesh_chunk_count,
            vertex_count,
            index_count,
            index_multiple_of_three,
            manufacturing_mesh_used: visual.report.manufacturing_mesh_used,
            browser_render_executed: false,
        }
    }
}

impl WebRetainedSceneUploadPlan {
    pub fn from_packet_and_pipeline(
        packet: &WebRetainedScenePacket,
        pipeline: &WebWorldScenePipelineContract,
    ) -> Self {
        let index_stride_bytes = match pipeline.index_format {
            "Uint16" => 2,
            "Uint32" => 4,
            _ => 0,
        };
        let vertex_buffer_bytes = packet
            .vertex_count
            .saturating_mul(pipeline.vertex_stride_bytes);
        let index_buffer_bytes = packet.index_count.saturating_mul(index_stride_bytes);
        let camera_uniform_bytes = pipeline.camera_uniform_size_bytes;
        let uses_copy_dst_uploads = pipeline
            .buffer_usages
            .iter()
            .any(|usage| *usage == "VERTEX|COPY_DST")
            && pipeline
                .buffer_usages
                .iter()
                .any(|usage| *usage == "INDEX|COPY_DST")
            && pipeline
                .buffer_usages
                .iter()
                .any(|usage| *usage == "UNIFORM|COPY_DST");

        Self {
            status: "source-upload-plan-only",
            vertex_stride_bytes: pipeline.vertex_stride_bytes,
            index_stride_bytes,
            camera_uniform_size_bytes: pipeline.camera_uniform_size_bytes,
            chunk_count: packet.chunk_count,
            draw_count: packet.indexed_mesh_chunk_count,
            vertex_count: packet.vertex_count,
            index_count: packet.index_count,
            vertex_buffer_bytes,
            index_buffer_bytes,
            camera_uniform_bytes,
            total_upload_bytes: vertex_buffer_bytes
                .saturating_add(index_buffer_bytes)
                .saturating_add(camera_uniform_bytes),
            uses_copy_dst_uploads,
            browser_upload_executed: false,
        }
    }
}

impl WebRetainedPackedBufferLayout {
    pub fn from_counts(
        packed_vertex_count: usize,
        packed_index_count: usize,
        packed_draw_count: usize,
    ) -> Self {
        let vertex_count = packed_vertex_count as u32;
        let index_count = packed_index_count as u32;
        let draw_count = packed_draw_count as u32;
        let total_bytes = boon_web_host_retained_packed_buffer_layout_total_bytes(
            vertex_count,
            index_count,
            draw_count,
        );
        let words = (0..RETAINED_PACKED_BUFFER_LAYOUT_WORD_COUNT)
            .map(|index| {
                boon_web_host_retained_packed_buffer_layout_word_at(
                    index,
                    vertex_count,
                    index_count,
                    draw_count,
                )
            })
            .collect::<Vec<_>>();
        let valid_by_wasm_contract = boon_web_host_retained_packed_buffer_layout_valid(
            vertex_count,
            index_count,
            draw_count,
            total_bytes,
        ) == 1;
        let wasm_fingerprint = boon_web_host_retained_packed_buffer_layout_fingerprint(
            vertex_count,
            index_count,
            draw_count,
            total_bytes,
        );

        Self {
            status: "source-packed-buffer-layout-contract-only",
            word_count: RETAINED_PACKED_BUFFER_LAYOUT_WORD_COUNT,
            vertex_offset_bytes: words[1] as usize,
            vertex_buffer_bytes: words[2] as usize,
            index_offset_bytes: words[3] as usize,
            index_buffer_bytes: words[4] as usize,
            camera_uniform_offset_bytes: words[5] as usize,
            camera_uniform_bytes: words[6] as usize,
            draw_descriptor_offset_bytes: words[7] as usize,
            draw_descriptor_bytes: words[8] as usize,
            total_bytes: words[9] as usize,
            words,
            packed_vertex_count,
            packed_index_count,
            packed_draw_count,
            valid_by_wasm_contract,
            wasm_fingerprint,
            browser_upload_executed: false,
            browser_render_executed: false,
        }
    }
}

pub fn web_browser_render_source_contract() -> WebBrowserRenderSourceContract {
    WebBrowserRenderSourceContract {
        status: "source-renderer-present-not-executed",
        shader_source_present: true,
        renderer_factory: "createRetainedWorldSceneRenderer",
        render_submit_function: "renderRetainedWorldScene",
        retained_mesh_packer: "packRetainedWorldScene",
        vertex_stride_bytes: WEB_HOST_VERTEX_STRIDE_BYTES as usize,
        index_format: "Uint32",
        color_target_count: 4,
        depth_target_format: "Depth32Float",
        normal_target_format: "Rgba8Unorm",
        feature_target_format: "Rgba8Unorm",
        pick_target_format: "Rgba8Unorm",
        uses_queue_write_buffer: true,
        uses_draw_indexed: true,
        uses_app_owned_readback_targets: true,
        browser_upload_executed: false,
        browser_render_executed: false,
        browser_capture_executed: false,
    }
}

pub fn web_semantic_bridge_contract() -> WebSemanticBridgeContract {
    WebSemanticBridgeContract {
        status: "semantic-ime-action-bridge-contract",
        visual_surface: "single-webgpu-canvas",
        semantic_dom_scope: "accessibility-ime-links-only",
        semantic_scene_type: "boon_document::SemanticScene",
        semantic_bridge_type: "boon_document::SemanticWebBridgeSnapshot",
        source_dispatch_type: "boon_document::SemanticWebSourceDispatch",
        live_bridge_function: "createSemanticWebBridge",
        supported_input_events: vec![
            "focus",
            "press",
            "set_text",
            "replace_selected_text",
            "compositionstart",
            "compositionupdate",
            "compositionend",
            "increment",
            "decrement",
        ],
        supports_ime_endpoint: true,
        supports_action_routes: true,
        supports_source_dispatch: true,
        supports_live_dom_mount: true,
        supports_focus_sync: true,
        supports_composition_events: true,
        mirrors_visual_dom: false,
        browser_render_executed: false,
    }
}

pub fn web_semantic_bridge_proof(scene: &boon_document::SemanticScene) -> WebSemanticBridgeProof {
    let bridge = boon_document::SemanticWebBridgeSnapshot::from_scene(scene);
    let html = bridge.to_html_fragment();
    let mut source_dispatch_count = 0_usize;
    let mut supports_press_dispatch = false;
    let mut supports_set_text_dispatch = false;
    let mut supports_replace_selected_text_dispatch = false;
    let mut supports_increment_dispatch = false;
    let mut supports_decrement_dispatch = false;

    for route in &bridge.action_routes {
        let event = match route.action {
            boon_document::SemanticWebAction::Focus => {
                boon_document::SemanticWebInputEvent::Focus {
                    semantic_id: route.semantic_id.clone(),
                }
            }
            boon_document::SemanticWebAction::Press => {
                boon_document::SemanticWebInputEvent::Press {
                    semantic_id: route.semantic_id.clone(),
                }
            }
            boon_document::SemanticWebAction::SetText => {
                boon_document::SemanticWebInputEvent::SetText {
                    semantic_id: route.semantic_id.clone(),
                    text: "probe".to_owned(),
                }
            }
            boon_document::SemanticWebAction::Increment => {
                boon_document::SemanticWebInputEvent::Increment {
                    semantic_id: route.semantic_id.clone(),
                }
            }
            boon_document::SemanticWebAction::Decrement => {
                boon_document::SemanticWebInputEvent::Decrement {
                    semantic_id: route.semantic_id.clone(),
                }
            }
        };
        if bridge.source_dispatch_for_event(event).is_some() {
            source_dispatch_count += 1;
            match route.action {
                boon_document::SemanticWebAction::Focus => {}
                boon_document::SemanticWebAction::Press => supports_press_dispatch = true,
                boon_document::SemanticWebAction::SetText => {
                    supports_set_text_dispatch = true;
                    supports_replace_selected_text_dispatch = bridge
                        .source_dispatch_for_event(
                            boon_document::SemanticWebInputEvent::ReplaceSelectedText {
                                semantic_id: route.semantic_id.clone(),
                                text: "replacement".to_owned(),
                            },
                        )
                        .is_some();
                }
                boon_document::SemanticWebAction::Increment => supports_increment_dispatch = true,
                boon_document::SemanticWebAction::Decrement => supports_decrement_dispatch = true,
            }
        }
    }

    let html_contains_visual_renderer_marker =
        html.contains("<canvas") || html.contains("<style") || html.contains("<svg");
    let live_bridge_function_present =
        STATIC_WORLD_SCENE_HOST_JS.contains("createSemanticWebBridge");
    let live_bridge_mount_present = STATIC_WORLD_SCENE_HOST_JS.contains("replaceChildren");
    let live_bridge_dispatch_present = STATIC_WORLD_SCENE_HOST_JS.contains("source_path")
        && STATIC_WORLD_SCENE_HOST_JS.contains("source_intent")
        && STATIC_WORLD_SCENE_HOST_JS.contains("keydown");
    let live_bridge_focus_sync_present = STATIC_WORLD_SCENE_HOST_JS.contains("setFocus:")
        && STATIC_WORLD_SCENE_HOST_JS.contains("focusedSemanticId:")
        && STATIC_WORLD_SCENE_HOST_JS.contains("data-boon-focused");
    let live_bridge_composition_present = STATIC_WORLD_SCENE_HOST_JS.contains("compositionstart")
        && STATIC_WORLD_SCENE_HOST_JS.contains("compositionupdate")
        && STATIC_WORLD_SCENE_HOST_JS.contains("compositionend")
        && STATIC_WORLD_SCENE_HOST_JS.contains("compositionEvents:")
        && STATIC_WORLD_SCENE_HOST_JS.contains("beforeinput")
        && STATIC_WORLD_SCENE_HOST_JS.contains("insertReplacementText")
        && STATIC_WORLD_SCENE_HOST_JS.contains("inputEvents:");
    let status = if bridge.metrics.visual_dom_node_count == 0
        && !html_contains_visual_renderer_marker
        && source_dispatch_count == bridge.metrics.source_routed_action_count
        && live_bridge_function_present
        && live_bridge_mount_present
        && live_bridge_dispatch_present
        && live_bridge_focus_sync_present
        && live_bridge_composition_present
        && supports_replace_selected_text_dispatch
    {
        "semantic-ime-action-bridge-pass"
    } else {
        "semantic-ime-action-bridge-fail"
    };

    WebSemanticBridgeProof {
        status,
        semantic_node_count: bridge.metrics.semantic_node_count,
        dom_node_count: bridge.metrics.dom_node_count,
        ime_endpoint_count: bridge.metrics.ime_endpoint_count,
        action_route_count: bridge.metrics.action_route_count,
        source_routed_action_count: bridge.metrics.source_routed_action_count,
        source_dispatch_count,
        visual_dom_node_count: bridge.metrics.visual_dom_node_count,
        html_contains_visual_renderer_marker,
        supports_press_dispatch,
        supports_set_text_dispatch,
        supports_replace_selected_text_dispatch,
        supports_increment_dispatch,
        supports_decrement_dispatch,
        live_bridge_function_present,
        live_bridge_mount_present,
        live_bridge_dispatch_present,
        live_bridge_focus_sync_present,
        live_bridge_composition_present,
        browser_render_executed: false,
    }
}

pub fn web_world_scene_pipeline_contract() -> WebWorldScenePipelineContract {
    WebWorldScenePipelineContract {
        status: "source-pipeline-contract-only",
        shader_language: "WGSL",
        required_features: Vec::new(),
        required_limits_profile: "downlevel_webgl2_defaults",
        vertex_entry_point: "vs_main",
        fragment_entry_point: "fs_main",
        vertex_stride_bytes: 80,
        vertex_attributes: vec![
            "location0:Float32x4@0:world_position",
            "location1:Float32x4@16:color",
            "location2:Float32x4@32:normal_color",
            "location3:Float32x4@48:feature_color",
            "location4:Float32x4@64:pick_color",
        ],
        camera_uniform_size_bytes: WEB_HOST_CAMERA_UNIFORM_BYTES as usize,
        primitive_topology: "TriangleList",
        index_format: "Uint32",
        color_format_policy: "browser-preferred-surface-format-plus-app-owned-Rgba8-targets",
        required_color_formats: vec!["Rgba8UnormSrgb", "Rgba8Unorm"],
        depth_format: "Depth32Float",
        depth_compare: "LessEqual",
        feature_target_format: "Rgba8Unorm",
        pick_target_format: "Rgba8Unorm",
        normal_target_format: "Rgba8Unorm",
        multisample_count: 1,
        buffer_usages: vec!["VERTEX|COPY_DST", "INDEX|COPY_DST", "UNIFORM|COPY_DST"],
        texture_usages: vec!["RENDER_ATTACHMENT", "RENDER_ATTACHMENT|COPY_SRC"],
        uses_push_constants: false,
        uses_storage_buffers: false,
        uses_storage_textures: false,
        uses_texture_sampling: false,
        uses_timestamp_queries: false,
        uses_indirect_draw: false,
        browser_render_executed: false,
    }
}

pub fn web_host_artifact_manifest() -> WebHostArtifactManifest {
    WebHostArtifactManifest {
        status: "source-artifact-contract-only",
        host_kind: "browser-webgpu-canvas-host",
        visual_surface: "single-webgpu-canvas",
        semantic_dom_scope: "accessibility-ime-links-only",
        uses_webgpu: true,
        uses_canvas_surface: true,
        mirrors_visual_dom: false,
        static_source_files: vec![
            WebHostStaticFile {
                path: "crates/boon_web_host/static/index.html",
                byte_count: STATIC_INDEX_HTML.len(),
                role: "minimal canvas boot document",
            },
            WebHostStaticFile {
                path: "crates/boon_web_host/static/world_scene_host.js",
                byte_count: STATIC_WORLD_SCENE_HOST_JS.len(),
                role: "webgpu device and retained scene host skeleton",
            },
        ],
        browser_render_executed: false,
        browser_capture_report_path: "target/reports/browser/world-scene.json",
        native_browser_comparison_report_path: "target/reports/native-gpu/native-web-render-comparison.json",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_host_manifest_is_honest_and_canvas_only() {
        let manifest = web_host_artifact_manifest();

        assert_eq!(manifest.status, "source-artifact-contract-only");
        assert!(manifest.uses_webgpu);
        assert!(manifest.uses_canvas_surface);
        assert!(!manifest.mirrors_visual_dom);
        assert!(!manifest.browser_render_executed);
        assert!(manifest.total_static_source_bytes() > 0);
        assert!(manifest.total_static_source_bytes() < WEB_HOST_SOURCE_BUDGET_BYTES);

        let json = serde_json::to_value(&manifest).expect("manifest should serialize");
        assert_eq!(
            json.get("semantic_dom_scope")
                .and_then(serde_json::Value::as_str),
            Some("accessibility-ime-links-only")
        );
    }

    #[test]
    fn exported_wasm_metadata_matches_host_contract() {
        assert_eq!(
            boon_web_host_retained_scene_packet_schema_version(),
            RETAINED_SCENE_PACKET_SCHEMA_VERSION
        );
        assert_eq!(boon_web_host_vertex_stride_bytes(), 80);
        assert_eq!(
            boon_web_host_static_source_budget_bytes(),
            WEB_HOST_SOURCE_BUDGET_BYTES as u32
        );
        assert_eq!(
            boon_web_host_index_stride_bytes(),
            WEB_HOST_INDEX_STRIDE_BYTES
        );
        assert_eq!(
            boon_web_host_camera_uniform_bytes(),
            WEB_HOST_CAMERA_UNIFORM_BYTES
        );
        assert_eq!(boon_web_host_draw_descriptor_bytes(6), 72);
        assert_eq!(boon_web_host_retained_vertex_buffer_bytes(170), 13_600);
        assert_eq!(boon_web_host_retained_index_buffer_bytes(984), 3_936);
        assert_eq!(boon_web_host_retained_upload_total_bytes(170, 984), 17_600);
        assert_eq!(
            boon_web_host_retained_packed_buffer_layout_total_bytes(368, 2_136, 6),
            38_120
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_layout_word_count(),
            RETAINED_PACKED_BUFFER_LAYOUT_WORD_COUNT
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_layout_word_at(0, 368, 2_136, 6),
            RETAINED_PACKED_BUFFER_LAYOUT_MAGIC
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_layout_word_at(1, 368, 2_136, 6),
            0
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_layout_word_at(2, 368, 2_136, 6),
            29_440
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_layout_word_at(3, 368, 2_136, 6),
            29_440
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_layout_word_at(4, 368, 2_136, 6),
            8_544
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_layout_word_at(5, 368, 2_136, 6),
            37_984
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_layout_word_at(7, 368, 2_136, 6),
            38_048
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_layout_word_at(8, 368, 2_136, 6),
            72
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_layout_word_at(9, 368, 2_136, 6),
            38_120
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_layout_valid(368, 2_136, 6, 38_120),
            1
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_layout_valid(368, 2_135, 6, 38_116),
            0
        );
        assert_ne!(
            boon_web_host_retained_packed_buffer_layout_fingerprint(368, 2_136, 6, 38_120),
            0
        );
        assert_ne!(
            boon_web_host_retained_packed_buffer_layout_fingerprint(368, 2_136, 6, 38_120),
            boon_web_host_retained_packed_buffer_layout_fingerprint(368, 2_136, 6, 38_119)
        );
        assert_eq!(
            boon_web_host_retained_upload_plan_valid(170, 984, 3, 3, 17_600),
            1
        );
        assert_eq!(
            boon_web_host_retained_upload_plan_valid(170, 984, 3, 3, 17_599),
            0
        );
        assert_ne!(
            boon_web_host_retained_upload_plan_fingerprint(170, 984, 3, 3, 17_600),
            0
        );
        assert_ne!(
            boon_web_host_retained_upload_plan_fingerprint(170, 984, 3, 3, 17_600),
            boon_web_host_retained_upload_plan_fingerprint(170, 984, 3, 3, 17_599)
        );
        assert_eq!(
            boon_web_host_retained_draw_plan_valid(170, 984, 3, 3, 368, 2_136, 6, 0, 0, 0),
            1
        );
        assert_eq!(
            boon_web_host_retained_draw_plan_valid(170, 984, 3, 3, 368, 2_135, 6, 0, 0, 0),
            0
        );
        assert_eq!(
            boon_web_host_retained_draw_plan_valid(170, 984, 3, 3, 368, 2_136, 2, 0, 0, 0),
            0
        );
        assert_ne!(
            boon_web_host_retained_draw_plan_fingerprint(170, 984, 3, 3, 368, 2_136, 6, 0, 0, 0),
            0
        );
        assert_ne!(
            boon_web_host_retained_draw_plan_fingerprint(170, 984, 3, 3, 368, 2_136, 6, 0, 0, 0),
            boon_web_host_retained_draw_plan_fingerprint(170, 984, 3, 3, 368, 2_135, 6, 0, 0, 0)
        );
        assert_eq!(
            boon_web_host_retained_renderer_dispatch_valid(368, 2_136, 6, 0, 0, 0),
            1
        );
        assert_eq!(
            boon_web_host_retained_renderer_dispatch_valid(368, 2_135, 6, 0, 0, 0),
            0
        );
        assert_eq!(
            boon_web_host_retained_renderer_dispatch_valid(368, 2_136, 0, 0, 0, 0),
            0
        );
        assert_ne!(
            boon_web_host_retained_renderer_dispatch_fingerprint(368, 2_136, 6, 0, 0, 0),
            0
        );
        assert_ne!(
            boon_web_host_retained_renderer_dispatch_fingerprint(368, 2_136, 6, 0, 0, 0),
            boon_web_host_retained_renderer_dispatch_fingerprint(368, 2_135, 6, 0, 0, 0)
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_checksums_valid(
                368,
                2_136,
                6,
                0,
                0,
                0,
                4_000_065_801,
                792_248_305,
                1_234_567,
                3_402_653_121
            ),
            1
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_checksums_valid(
                368,
                2_136,
                6,
                0,
                0,
                0,
                0,
                792_248_305,
                1_234_567,
                3_402_653_121
            ),
            0
        );
        assert_eq!(
            boon_web_host_retained_packed_buffer_checksums_valid(
                368,
                2_135,
                6,
                0,
                0,
                0,
                4_000_065_801,
                792_248_305,
                1_234_567,
                3_402_653_121
            ),
            0
        );
        assert_ne!(
            boon_web_host_retained_packed_buffer_checksums_fingerprint(
                368,
                2_136,
                6,
                0,
                0,
                0,
                4_000_065_801,
                792_248_305,
                1_234_567,
                3_402_653_121
            ),
            0
        );
        assert_ne!(
            boon_web_host_retained_packed_buffer_checksums_fingerprint(
                368,
                2_136,
                6,
                0,
                0,
                0,
                4_000_065_801,
                792_248_305,
                1_234_567,
                3_402_653_121
            ),
            boon_web_host_retained_packed_buffer_checksums_fingerprint(
                368,
                2_136,
                6,
                0,
                0,
                0,
                4_000_065_802,
                792_248_305,
                1_234_567,
                3_402_653_121
            )
        );
        assert_eq!(
            boon_web_host_retained_command_stream_valid(1, 4, 1, 6, 0, 5, 6),
            1
        );
        assert_eq!(
            boon_web_host_retained_command_stream_valid(1, 3, 1, 6, 0, 5, 6),
            0
        );
        assert_eq!(
            boon_web_host_retained_command_stream_valid(1, 4, 1, 6, 1, 5, 6),
            0
        );
        assert_ne!(
            boon_web_host_retained_command_stream_fingerprint(1, 4, 1, 6, 0, 5, 6),
            0
        );
        assert_ne!(
            boon_web_host_retained_command_stream_fingerprint(1, 4, 1, 6, 0, 5, 6),
            boon_web_host_retained_command_stream_fingerprint(1, 3, 1, 6, 0, 5, 6)
        );
        assert_eq!(
            boon_web_host_retained_command_stream_word_count(),
            RETAINED_COMMAND_STREAM_WORD_COUNT
        );
        assert_eq!(
            boon_web_host_retained_command_stream_word_at(0, 1, 4, 1, 6, 0, 5, 6),
            RETAINED_COMMAND_STREAM_WORD_MAGIC
        );
        assert_eq!(
            boon_web_host_retained_command_stream_word_at(4, 1, 4, 1, 6, 0, 5, 6),
            6
        );
        assert_eq!(
            boon_web_host_retained_command_stream_word_at(99, 1, 4, 1, 6, 0, 5, 6),
            0
        );
        assert_ne!(
            boon_web_host_retained_command_stream_words_fingerprint(1, 4, 1, 6, 0, 5, 6),
            0
        );
        assert_ne!(
            boon_web_host_retained_command_stream_words_fingerprint(1, 4, 1, 6, 0, 5, 6),
            boon_web_host_retained_command_stream_words_fingerprint(1, 4, 1, 7, 0, 5, 6)
        );
    }

    #[test]
    fn web_world_scene_pipeline_contract_matches_shared_webgpu_limits() {
        let contract = web_world_scene_pipeline_contract();

        assert_eq!(contract.status, "source-pipeline-contract-only");
        assert_eq!(contract.shader_language, "WGSL");
        assert!(contract.required_features.is_empty());
        assert_eq!(
            contract.required_limits_profile,
            "downlevel_webgl2_defaults"
        );
        assert_eq!(contract.vertex_entry_point, "vs_main");
        assert_eq!(contract.fragment_entry_point, "fs_main");
        assert_eq!(contract.vertex_stride_bytes, 80);
        assert_eq!(contract.vertex_attributes.len(), 5);
        assert_eq!(contract.camera_uniform_size_bytes, 64);
        assert_eq!(contract.primitive_topology, "TriangleList");
        assert_eq!(contract.index_format, "Uint32");
        assert_eq!(contract.depth_format, "Depth32Float");
        assert_eq!(contract.depth_compare, "LessEqual");
        assert_eq!(contract.feature_target_format, "Rgba8Unorm");
        assert_eq!(contract.pick_target_format, "Rgba8Unorm");
        assert_eq!(contract.normal_target_format, "Rgba8Unorm");
        assert_eq!(contract.multisample_count, 1);
        assert!(!contract.uses_push_constants);
        assert!(!contract.uses_storage_buffers);
        assert!(!contract.uses_storage_textures);
        assert!(!contract.uses_texture_sampling);
        assert!(!contract.uses_timestamp_queries);
        assert!(!contract.uses_indirect_draw);
        assert!(!contract.browser_render_executed);
    }

    #[test]
    fn retained_scene_packet_serializes_parametric_car_chunks() {
        let visual = boon_scene_model::WorldScene::visual_proxy_with_chunks_from_solid_model(
            &boon_solid_model::SolidModelBundle::parametric_car_fixture(),
        )
        .expect("parametric car visual proxy should compile");
        let packet = WebRetainedScenePacket::from_visual_scene(visual);

        assert_eq!(packet.schema_version, 1);
        assert_eq!(
            packet.surface_representation_encoding,
            "serde-externally-tagged-SurfaceRepresentation"
        );
        assert_eq!(
            packet.supported_surface_representation_tags,
            [
                "IndexedMesh",
                "IndexedMeshSummary",
                "DirectedDualGridSummary"
            ]
        );
        assert_eq!(packet.chunk_count, 3);
        assert_eq!(packet.indexed_mesh_chunk_count, 3);
        assert_eq!(packet.vertex_count, 170);
        assert_eq!(packet.index_count, 984);
        assert!(packet.index_multiple_of_three);
        assert!(!packet.manufacturing_mesh_used);
        assert!(!packet.browser_render_executed);

        let json = serde_json::to_value(&packet).expect("packet should serialize");
        assert_eq!(
            json.get("chunk_count").and_then(serde_json::Value::as_u64),
            Some(3)
        );
        assert!(
            json.pointer("/chunks/0/representation/IndexedMesh")
                .is_some(),
            "SurfaceRepresentation must serialize as a serde externally tagged enum for the JS host"
        );
    }

    #[test]
    fn upload_plan_derives_exact_buffer_sizes_from_retained_packet() {
        let visual = boon_scene_model::WorldScene::visual_proxy_with_chunks_from_solid_model(
            &boon_solid_model::SolidModelBundle::parametric_car_fixture(),
        )
        .expect("parametric car visual proxy should compile");
        let packet = WebRetainedScenePacket::from_visual_scene(visual);
        let pipeline = web_world_scene_pipeline_contract();
        let plan = WebRetainedSceneUploadPlan::from_packet_and_pipeline(&packet, &pipeline);

        assert_eq!(plan.status, "source-upload-plan-only");
        assert_eq!(plan.vertex_stride_bytes, 80);
        assert_eq!(plan.index_stride_bytes, 4);
        assert_eq!(plan.camera_uniform_size_bytes, 64);
        assert_eq!(plan.chunk_count, 3);
        assert_eq!(plan.draw_count, 3);
        assert_eq!(plan.vertex_count, 170);
        assert_eq!(plan.index_count, 984);
        assert_eq!(plan.vertex_buffer_bytes, 13_600);
        assert_eq!(plan.index_buffer_bytes, 3_936);
        assert_eq!(plan.camera_uniform_bytes, 64);
        assert_eq!(plan.total_upload_bytes, 17_600);
        assert!(plan.uses_copy_dst_uploads);
        assert!(!plan.browser_upload_executed);
    }

    #[test]
    fn packed_buffer_layout_contract_derives_non_overlapping_offsets() {
        let layout = WebRetainedPackedBufferLayout::from_counts(368, 2_136, 6);

        assert_eq!(layout.status, "source-packed-buffer-layout-contract-only");
        assert_eq!(layout.word_count, RETAINED_PACKED_BUFFER_LAYOUT_WORD_COUNT);
        assert_eq!(
            layout.words.len(),
            RETAINED_PACKED_BUFFER_LAYOUT_WORD_COUNT as usize
        );
        assert_eq!(layout.words[0], RETAINED_PACKED_BUFFER_LAYOUT_MAGIC);
        assert_eq!(layout.packed_vertex_count, 368);
        assert_eq!(layout.packed_index_count, 2_136);
        assert_eq!(layout.packed_draw_count, 6);
        assert_eq!(layout.vertex_offset_bytes, 0);
        assert_eq!(layout.vertex_buffer_bytes, 29_440);
        assert_eq!(layout.index_offset_bytes, 29_440);
        assert_eq!(layout.index_buffer_bytes, 8_544);
        assert_eq!(layout.camera_uniform_offset_bytes, 37_984);
        assert_eq!(layout.camera_uniform_bytes, 64);
        assert_eq!(layout.draw_descriptor_offset_bytes, 38_048);
        assert_eq!(layout.draw_descriptor_bytes, 72);
        assert_eq!(layout.total_bytes, 38_120);
        assert!(layout.valid_by_wasm_contract);
        assert_ne!(layout.wasm_fingerprint, 0);
        assert!(!layout.browser_upload_executed);
        assert!(!layout.browser_render_executed);
    }

    #[test]
    fn browser_renderer_source_contract_is_present_but_not_executed() {
        let contract = web_browser_render_source_contract();

        assert_eq!(contract.status, "source-renderer-present-not-executed");
        assert!(contract.shader_source_present);
        assert_eq!(
            contract.renderer_factory,
            "createRetainedWorldSceneRenderer"
        );
        assert_eq!(contract.render_submit_function, "renderRetainedWorldScene");
        assert_eq!(contract.retained_mesh_packer, "packRetainedWorldScene");
        assert_eq!(contract.vertex_stride_bytes, 80);
        assert_eq!(contract.index_format, "Uint32");
        assert_eq!(contract.color_target_count, 4);
        assert_eq!(contract.depth_target_format, "Depth32Float");
        assert_eq!(contract.normal_target_format, "Rgba8Unorm");
        assert_eq!(contract.feature_target_format, "Rgba8Unorm");
        assert_eq!(contract.pick_target_format, "Rgba8Unorm");
        assert!(contract.uses_queue_write_buffer);
        assert!(contract.uses_draw_indexed);
        assert!(contract.uses_app_owned_readback_targets);
        assert!(!contract.browser_upload_executed);
        assert!(!contract.browser_render_executed);
        assert!(!contract.browser_capture_executed);

        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("createRetainedWorldSceneRenderer"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("renderRetainedWorldScene"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("packRetainedWorldScene"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("writeBuffer"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("drawIndexed"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("pickView"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("pickRetainedWorldSceneAtCanvasPoint"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("selectionOverlayDrawCount"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("selectionOutlineDrawCount"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("selectionRestoreDrawCount"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("writeMask:0"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("edgePixelSamples"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("mask-run-edge-pixels-v1"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("pixelSamplesByTarget"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("explicit-probe"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("depthPixelSamples"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("browserTriangleProbeSamples"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("browser-js-fround-packed-vertex-probe"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("drawCommandEncoding"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("retained-chunk-index-ranges"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("loadBoonWebHostWasmContract"));
        assert!(
            STATIC_WORLD_SCENE_HOST_JS
                .contains("browser-wasm-retained-packed-buffer-checksums-executed")
        );
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("retainedUploadPlanFingerprint"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("retainedUploadPlanValidatedByWasm"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("retainedDrawPlanFingerprint"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("retainedDrawPlanValidatedByWasm"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("retainedRendererDispatchFingerprint"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("retainedRendererDispatchValidatedByWasm"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("retainedPackedBufferChecksumsFingerprint"));
        assert!(
            STATIC_WORLD_SCENE_HOST_JS.contains("retainedPackedBufferChecksumsValidatedByWasm")
        );
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("packedDrawDescriptorChecksum"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("retainedCommandStreamFingerprint"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("retainedCommandStreamValidatedByWasm"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("retainedCommandStreamWordsFingerprint"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("retainedCommandStreamWordsFromWasm"));
    }

    #[test]
    fn browser_semantic_bridge_contract_routes_ime_and_actions_without_visual_dom() {
        let contract = web_semantic_bridge_contract();

        assert_eq!(contract.status, "semantic-ime-action-bridge-contract");
        assert_eq!(contract.visual_surface, "single-webgpu-canvas");
        assert_eq!(contract.semantic_dom_scope, "accessibility-ime-links-only");
        assert_eq!(contract.semantic_scene_type, "boon_document::SemanticScene");
        assert_eq!(
            contract.semantic_bridge_type,
            "boon_document::SemanticWebBridgeSnapshot"
        );
        assert_eq!(
            contract.source_dispatch_type,
            "boon_document::SemanticWebSourceDispatch"
        );
        assert_eq!(contract.live_bridge_function, "createSemanticWebBridge");
        assert!(contract.supported_input_events.contains(&"increment"));
        assert!(contract.supported_input_events.contains(&"decrement"));
        assert!(
            contract
                .supported_input_events
                .contains(&"compositionstart")
        );
        assert!(
            contract
                .supported_input_events
                .contains(&"compositionupdate")
        );
        assert!(contract.supported_input_events.contains(&"compositionend"));
        assert!(contract.supports_ime_endpoint);
        assert!(contract.supports_action_routes);
        assert!(contract.supports_source_dispatch);
        assert!(contract.supports_live_dom_mount);
        assert!(contract.supports_focus_sync);
        assert!(contract.supports_composition_events);
        assert!(!contract.mirrors_visual_dom);
        assert!(!contract.browser_render_executed);
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("createSemanticWebBridge"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("browser-semantic-live-bridge-ready"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("keydown"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("setFocus:"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("focusedSemanticId:"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("compositionstart"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("compositionend"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("beforeinput"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("insertReplacementText"));
        assert!(STATIC_WORLD_SCENE_HOST_JS.contains("inputEvents:"));

        let mut scene = boon_document::SemanticScene::default();
        scene.root = Some(boon_document::SemanticId("semantic:root".to_owned()));
        for (id, role, action, source_intent) in [
            (
                "semantic:save",
                boon_document::SemanticRole::Button,
                boon_document::SemanticWebAction::Press,
                "press",
            ),
            (
                "semantic:filter",
                boon_document::SemanticRole::TextInput,
                boon_document::SemanticWebAction::SetText,
                "change",
            ),
            (
                "semantic:zoom-in",
                boon_document::SemanticRole::Button,
                boon_document::SemanticWebAction::Increment,
                "increment",
            ),
            (
                "semantic:zoom-out",
                boon_document::SemanticRole::Button,
                boon_document::SemanticWebAction::Decrement,
                "decrement",
            ),
        ] {
            let node_id = id.trim_start_matches("semantic:");
            scene.nodes.insert(
                boon_document::SemanticId(id.to_owned()),
                boon_document::SemanticNode {
                    id: boon_document::SemanticId(id.to_owned()),
                    node: boon_document::DocumentNodeId(node_id.to_owned()),
                    role: role.clone(),
                    name: Some(node_id.replace('-', " ")),
                    description: None,
                    value: (role == boon_document::SemanticRole::TextInput).then(|| {
                        boon_document::SemanticValue::Text {
                            text: "abc".to_owned(),
                        }
                    }),
                    state: boon_document::SemanticState::default(),
                    actions: boon_document::SemanticActions {
                        focus: true,
                        press: action == boon_document::SemanticWebAction::Press,
                        set_text: action == boon_document::SemanticWebAction::SetText,
                        increment: action == boon_document::SemanticWebAction::Increment,
                        decrement: action == boon_document::SemanticWebAction::Decrement,
                    },
                    relations: boon_document::SemanticRelations::default(),
                    bounds: None,
                    language: None,
                    heading_level: None,
                    href: None,
                    source_binding_id: Some(boon_document::SourceBindingId(format!(
                        "source:{node_id}:{source_intent}"
                    ))),
                    source_path: Some(format!("browser.{node_id}")),
                    source_intent: Some(source_intent.to_owned()),
                },
            );
        }

        let proof = web_semantic_bridge_proof(&scene);

        assert_eq!(proof.status, "semantic-ime-action-bridge-pass");
        assert_eq!(proof.semantic_node_count, 4);
        assert_eq!(proof.dom_node_count, 4);
        assert_eq!(proof.ime_endpoint_count, 1);
        assert!(proof.action_route_count >= 4);
        assert_eq!(
            proof.source_routed_action_count,
            proof.source_dispatch_count
        );
        assert_eq!(proof.visual_dom_node_count, 0);
        assert!(!proof.html_contains_visual_renderer_marker);
        assert!(proof.supports_press_dispatch);
        assert!(proof.supports_set_text_dispatch);
        assert!(proof.supports_replace_selected_text_dispatch);
        assert!(proof.supports_increment_dispatch);
        assert!(proof.supports_decrement_dispatch);
        assert!(proof.live_bridge_function_present);
        assert!(proof.live_bridge_mount_present);
        assert!(proof.live_bridge_dispatch_present);
        assert!(proof.live_bridge_focus_sync_present);
        assert!(proof.live_bridge_composition_present);
        assert!(!proof.browser_render_executed);
    }
}
