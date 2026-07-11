use super::*;
use boon_document::{
    AccessibilityTree, ComputedStyleIdentity, DisplayItem, DocumentNodeId, LayoutMetrics,
};

fn test_style_identity() -> ComputedStyleIdentity {
    ComputedStyleIdentity {
        style_id: 1,
        layout_id: 2,
        paint_id: 3,
        material_id: 4,
        font_id: 5,
        pseudo_state_id: 6,
    }
}

fn test_document_scene_from_layout_frame(
    frame: &LayoutFrame,
    width: u32,
    height: u32,
) -> (DocumentRenderScene, String) {
    let mut columns = GlyphonRenderTextColumnMeasurer::new();
    let scene = boon_document::render_scene::lower_layout_frame_to_render_scene(
        frame,
        width,
        height,
        &mut columns,
    );
    let scene_identity = format!("{:x}", Sha256::digest(format!("{scene:?}").as_bytes()));
    (scene, scene_identity)
}

fn flatten_quad_batches(batches: &[QuadBatch]) -> (Vec<f32>, Vec<u8>) {
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    for batch in batches {
        for vertex in &batch.vertices {
            positions.extend_from_slice(&vertex.position);
            colors.extend_from_slice(&rgba8_from_packed(vertex.color));
        }
    }
    (positions, colors)
}

fn test_graph_pass(upload_bytes: u64, dirty_chunk_count: u32) -> RendererRenderGraphPassMetric {
    RendererRenderGraphPassMetric {
        schema_version: 1,
        pass_id: "prepare".to_owned(),
        pass_kind: "retained_quad_prepare_and_dirty_upload".to_owned(),
        input: "RenderSceneItems".to_owned(),
        output: "RetainedGpuBuffers".to_owned(),
        read_resources: vec!["RenderSceneItems".to_owned()],
        write_resources: vec!["RetainedGpuBuffers".to_owned()],
        product_visible: true,
        proof_or_readback: false,
        duration_ms: 1.0,
        upload_bytes,
        dirty_chunk_count,
        queue_write_count: 1,
        draw_call_count: 1,
    }
}

// Native GPU tests are grouped by renderer area while staying in this module for private helper access.
include!("tests/document_primitives.rs");
include!("tests/render_graph.rs");
include!("tests/retained_quads.rs");
include!("tests/text_and_fonts.rs");
