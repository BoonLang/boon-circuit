use super::*;

fn test_readback_artifact(key: FrameEvidenceKey, sequence: u64) -> AppWindowReadbackArtifact {
    AppWindowReadbackArtifact {
        path: format!("artifact-{sequence}.png"),
        sha256: format!("sha-{sequence}"),
        width: 8,
        height: 8,
        presented_revision: Some(key.frame_seq),
        content_revision: Some(key.content_revision),
        rendered_frame_count: Some(key.frame_seq),
        frame_evidence_key: Some(key),
        capture_method: "wgpu-visible-surface-copy-src-readback".to_owned(),
        texture_format: "Rgba8Unorm".to_owned(),
        nonblank_samples: 1,
        unique_rgba_values: 1,
        readback_deadline_ms: 5_000,
        readback_poll_status: "completed_before_deadline".to_owned(),
    }
}

fn test_product_result(
    product_frame: NativeRenderedProductFrame,
    post_present_proof_requests: Vec<NativePostPresentProofRequestSummary>,
) -> NativeProductFrameResult {
    NativeProductFrameResult {
        schema_version: 1,
        owner: "preview_active_scene".to_owned(),
        result_kind: "active_preview_scene_patch".to_owned(),
        product_frame,
        render_graph: None,
        present_plan: None,
        post_present_proof_requests,
    }
}

// App-window tests are grouped by frame/input/proof area while staying in this module for private helper access.
include!("tests/accessibility.rs");
include!("tests/input_timing_and_cursor.rs");
include!("tests/proof_and_readback.rs");
include!("tests/render_result_revisions.rs");
include!("tests/reports_and_telemetry.rs");
include!("tests/scheduler_and_frame_pacing.rs");
include!("tests/surface_and_present_modes.rs");
