fn cells_visible_click_test_post_present_isolation() -> serde_json::Value {
    json!({
        "status": "pass",
        "product_path_status": "pass",
        "proof_worker_status": "lagging",
        "product_latency_includes_proof_completion": false,
        "product_blocks_on_proof_subscribers": false,
        "proof_latency_reported_separately": true,
        "proof_completion_required_for_product_present": false,
        "report_write_in_hot_path": false,
        "report_serialization_in_hot_path": false,
        "pre_present_request_count": 0,
        "hot_path_report_write_count": 0,
        "hot_path_report_serialization_count": 0,
        "subscriber_error_count": 0,
        "worker_error_count": 0,
        "queued_request_count": 2,
        "recent_queue_count": 4
    })
}

fn cells_visible_click_test_hardware_adapter() -> serde_json::Value {
    json!({
        "adapter_name": "test-gpu",
        "adapter_backend": "Vulkan",
        "adapter_device": 1,
        "adapter_vendor": 2,
        "adapter_device_type": "DiscreteGpu",
        "adapter_is_software": false
    })
}

fn cells_visible_click_test_software_adapter() -> serde_json::Value {
    json!({
        "adapter_name": "llvmpipe",
        "adapter_backend": "Vulkan",
        "adapter_device": 0,
        "adapter_vendor": 0,
        "adapter_device_type": "Cpu",
        "adapter_is_software": true
    })
}

fn cells_visible_click_test_product_patch() -> serde_json::Value {
    json!({
        "schema_version": 1,
        "status": "pass",
        "owner": "preview_active_scene",
        "patch_kind": "direct_input_overlay_render_scene_patch",
        "source": "retained_bound_sync",
        "active_scene_identity": "active-preview-scene:test",
        "route_identity": "route:test",
        "touched_node_count": 3,
        "touched_node_samples": ["input-alpha", "choice-alpha", "choice-beta"],
        "retained_text_update_count": 1,
        "retained_style_update_count": 2,
        "hover_node_count": 0,
        "focus_node_count": 1,
        "direct_render_scene_patch": true,
        "full_scene_build_before_present": false,
        "proof_json_required": false,
        "latest_report_required": false
    })
}

fn cells_visible_click_test_product_render_graph() -> serde_json::Value {
    json!({
        "schema_version": 1,
        "status": "pass",
        "owner": "preview_active_scene",
        "graph_kind": "product_render_graph",
        "renderer_graph_kind": "boon_native_gpu_product_frame_graph",
        "renderer_graph_execution_kind": "retained_product_frame_graph_linear_v1",
        "renderer_graph_plan_hash": "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd",
        "renderer_graph_pass_count": 5,
        "renderer_graph_product_pass_count": 4,
        "renderer_graph_proof_pass_count": 0,
        "renderer_graph_resource_count": 7,
        "renderer_graph_product_resource_count": 6,
        "renderer_graph_resource_lifetime_hash": "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "renderer_graph_retained_resource_epoch_hash": "23456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef01",
        "renderer_graph_retained_state_resource_count": 7,
        "renderer_graph_retained_dirty_resource_count": 2,
        "renderer_graph_retained_reused_resource_count": 5,
        "renderer_graph_scheduler_kind": "renderer_owned_product_frame_schedule_v1",
        "renderer_graph_schedule_hash": "3456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef012",
        "renderer_graph_schedule_decision_count": 7,
        "renderer_graph_dirty_resource_decision_count": 2,
        "renderer_graph_reuse_resource_decision_count": 3,
        "renderer_graph_per_present_resource_decision_count": 2,
        "active_scene_identity": "active-preview-scene:test",
        "render_scene_identity": "render-scene:test",
        "pass_count": 5,
        "product_pass_count": 4,
        "proof_pass_count": 0,
        "dirty_chunk_count": 3,
        "upload_bytes": 128,
        "encode_time_ms": 0.7,
        "cache_hit": false,
        "proof_readback_in_product_graph": false,
        "stale_epoch_rejection_count": 0,
        "plan_hash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "passes": [
            {
                "schema_version": 1,
                "pass_id": "native-gpu:renderer-scene-key",
                "pass_kind": "scene_identity",
                "source": "RenderScene",
                "target": "SceneCacheKey",
                "product_visible": true,
                "proof_or_readback": false
            },
            {
                "schema_version": 1,
                "pass_id": "native-gpu:renderer-quad-prepare-upload",
                "pass_kind": "retained_quad_prepare_and_dirty_upload",
                "source": "RenderSceneItems",
                "target": "RetainedGpuBuffers",
                "product_visible": true,
                "proof_or_readback": false
            },
            {
                "schema_version": 1,
                "pass_id": "native-gpu:renderer-ui-draw",
                "pass_kind": "ui_draw_pass",
                "source": "RetainedGpuBuffers",
                "target": "ColorTarget",
                "product_visible": true,
                "proof_or_readback": false
            },
            {
                "schema_version": 1,
                "pass_id": "native-gpu:renderer-retained-metrics",
                "pass_kind": "retained_metrics",
                "source": "RenderScene",
                "target": "FrameMetrics",
                "product_visible": false,
                "proof_or_readback": false
            },
            {
                "schema_version": 1,
                "pass_id": "native-gpu:renderer-text-draw",
                "pass_kind": "text_draw_pass",
                "source": "TextRuns",
                "target": "ColorTarget",
                "product_visible": true,
                "proof_or_readback": false
            }
        ]
    })
}

fn cells_visible_click_test_present_plan() -> serde_json::Value {
    json!({
        "schema_version": 1,
        "status": "pass",
        "owner": "preview_active_scene",
        "plan_kind": "product_present_plan",
        "render_target_kind": "visible-surface-direct",
        "pass_count": 5,
        "product_pass_count": 4,
        "proof_pass_count": 0,
        "proof_readback_in_product_passes": false,
        "plan_hash": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    })
}


