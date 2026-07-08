#[test]
fn cells_visible_click_product_contract_rejects_software_adapter() {
    let product_frames = json!({
        "status": "pass",
        "source": "app_window_product_frame_commits",
        "adapter_identity": cells_visible_click_test_software_adapter(),
        "adapter_status": "software",
        "product_frame_sample_count": 16,
        "exact_product_commit_match_count": 16,
        "typed_product_patch_count": 16,
        "typed_product_result_count": 16,
        "product_result_missing_count": 0,
        "product_patch_full_scene_build_count": 0,
        "product_patch_proof_json_required_count": 0,
        "product_patch_latest_report_required_count": 0,
        "hard_failure_count": 0,
        "input_to_present_ms": {
            "p95": 8.0,
            "max": 12.0
        },
        "input_to_formula_visible_ms": {
            "p95": 8.0,
            "max": 12.0
        }
    });
    let product_contract = cells_visible_click_product_only_ux_contract(
        &product_frames,
        &json!({"status": "pass"}),
        &json!({"status": "pass"}),
        &cells_visible_click_test_post_present_isolation(),
        true,
        "demand_driven",
        16.7,
        33.4,
    );

    assert_eq!(
        product_contract
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("fail")
    );
    assert_eq!(
        product_contract
            .get("adapter_status")
            .and_then(serde_json::Value::as_str),
        Some("software")
    );
    assert_eq!(
        product_contract
            .get("software_adapter_wall_clock_budget_exempt")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
}


#[test]
fn cells_visible_click_product_contract_rejects_missing_typed_product_patch_summary() {
    let product_frames = json!({
        "status": "pass",
        "source": "app_window_product_frame_commits",
        "adapter_identity": cells_visible_click_test_hardware_adapter(),
        "adapter_status": "hardware",
        "product_frame_sample_count": 16,
        "exact_product_commit_match_count": 16,
        "typed_product_patch_count": 0,
        "typed_product_result_count": 16,
        "product_result_missing_count": 0,
        "product_patch_full_scene_build_count": 0,
        "product_patch_proof_json_required_count": 0,
        "product_patch_latest_report_required_count": 0,
        "hard_failure_count": 0,
        "input_to_present_ms": {
            "p95": 8.0,
            "max": 12.0
        },
        "input_to_formula_visible_ms": {
            "p95": 8.0,
            "max": 12.0
        }
    });
    let product_contract = cells_visible_click_product_only_ux_contract(
        &product_frames,
        &json!({"status": "pass"}),
        &json!({"status": "pass"}),
        &cells_visible_click_test_post_present_isolation(),
        true,
        "demand_driven",
        16.7,
        33.4,
    );

    assert_eq!(
        product_contract
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("fail")
    );
    assert_eq!(product_contract["typed_product_patch_count"], json!(0));
}


#[test]
fn cells_visible_click_product_contract_rejects_missing_typed_product_result() {
    let product_frames = json!({
        "status": "pass",
        "source": "app_window_product_frame_commits",
        "adapter_identity": cells_visible_click_test_hardware_adapter(),
        "adapter_status": "hardware",
        "product_frame_sample_count": 16,
        "exact_product_commit_match_count": 16,
        "typed_product_patch_count": 16,
        "typed_product_result_count": 0,
        "product_result_missing_count": 16,
        "product_patch_full_scene_build_count": 0,
        "product_patch_proof_json_required_count": 0,
        "product_patch_latest_report_required_count": 0,
        "hard_failure_count": 0,
        "input_to_present_ms": {
            "p95": 8.0,
            "max": 12.0
        },
        "input_to_formula_visible_ms": {
            "p95": 8.0,
            "max": 12.0
        }
    });
    let product_contract = cells_visible_click_product_only_ux_contract(
        &product_frames,
        &json!({"status": "pass"}),
        &json!({"status": "pass"}),
        &cells_visible_click_test_post_present_isolation(),
        true,
        "demand_driven",
        16.7,
        33.4,
    );

    assert_eq!(
        product_contract
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("fail")
    );
    assert_eq!(product_contract["typed_product_result_count"], json!(0));
    assert_eq!(product_contract["product_result_missing_count"], json!(16));
}


#[test]
fn cells_visible_click_product_commit_scope_separates_timing_from_adapter() {
    let key = json!({
        "frame_seq": 7,
        "present_id": 7,
        "input_event_seq": 4,
        "surface_id": "preview:test",
        "surface_epoch": 1,
        "content_revision": 6,
        "layout_revision": 1,
        "render_scene_revision": 4
    });
    let live_probe = json!({
        "adapter_identity": cells_visible_click_test_software_adapter(),
        "click_samples": [{
            "index": 0,
            "target_address": "A2",
            "product_frame_evidence_key": key,
            "input_accept_to_present_ms": 8.0,
            "input_accept_to_formula_visible_ms": 8.0,
            "product_formula_state_current": true
        }],
        "recent_product_frame_commits": [{
            "frame_lane": "product_interaction",
            "input_event_seq": 4,
            "input_to_present_ms": 8.0,
            "product_result_source": "native_product_render_result",
            "product_result_owner": "preview_active_scene",
            "product_result_kind": "active_preview_scene_patch",
            "input_timing": {
                "input_wake_to_input_accept_ms": 1.0,
                "input_wake_to_present_ms": 9.0
            },
            "frame_evidence_key": {
                "frame_seq": 7,
                "present_id": 7,
                "input_event_seq": 4,
                "surface_id": "preview:test",
                "surface_epoch": 1,
                "content_revision": 6,
                "layout_revision": 1,
                "render_scene_revision": 4
            },
            "product_frame": {
                "product_patch": cells_visible_click_test_product_patch()
            },
            "render_graph": cells_visible_click_test_product_render_graph(),
            "present_plan": cells_visible_click_test_present_plan()
        }]
    });

    let summary =
        cells_visible_click_app_window_product_commit_scope_summary(&live_probe, 1, 16.7, 33.4);

    assert_eq!(summary["timing_status"], json!("pass"));
    assert_eq!(summary["status"], json!("fail"));
    assert_eq!(summary["adapter_status"], json!("software"));
    assert_eq!(
        summary["input_to_present_ms"]["p95"],
        json!(8.0),
        "timing evidence should stay visible even when hardware evidence fails"
    );
    assert_eq!(summary["hard_failure_count"], json!(0));
    assert_eq!(summary["missed_frame_count"], json!(0));
    assert_eq!(summary["exact_product_commit_match_count"], json!(1));
    assert_eq!(summary["typed_product_patch_count"], json!(1));
    assert_eq!(summary["typed_product_result_count"], json!(1));
    assert_eq!(summary["product_result_missing_count"], json!(0));
    assert_eq!(summary["product_patch_missing_count"], json!(0));
}


#[test]
fn cells_visible_click_product_commit_scope_requires_typed_product_patch() {
    let key = json!({
        "frame_seq": 8,
        "present_id": 8,
        "input_event_seq": 5,
        "surface_id": "preview:test",
        "surface_epoch": 1,
        "content_revision": 7,
        "layout_revision": 1,
        "render_scene_revision": 5
    });
    let live_probe = json!({
        "adapter_identity": cells_visible_click_test_hardware_adapter(),
        "click_samples": [{
            "index": 0,
            "target_address": "A2",
            "product_frame_evidence_key": key,
            "input_accept_to_present_ms": 8.0,
            "input_accept_to_formula_visible_ms": 8.0,
            "product_formula_state_current": true
        }],
        "recent_product_frame_commits": [{
            "frame_lane": "product_interaction",
            "input_event_seq": 5,
            "input_to_present_ms": 8.0,
            "product_result_source": "native_product_render_result",
            "product_result_owner": "preview_active_scene",
            "product_result_kind": "active_preview_scene_patch",
            "input_timing": {
                "input_wake_to_input_accept_ms": 1.0,
                "input_wake_to_present_ms": 9.0
            },
            "frame_evidence_key": {
                "frame_seq": 8,
                "present_id": 8,
                "input_event_seq": 5,
                "surface_id": "preview:test",
                "surface_epoch": 1,
                "content_revision": 7,
                "layout_revision": 1,
                "render_scene_revision": 5
            }
        }]
    });

    let summary =
        cells_visible_click_app_window_product_commit_scope_summary(&live_probe, 1, 16.7, 33.4);

    assert_eq!(summary["status"], json!("fail"));
    assert_eq!(summary["timing_status"], json!("fail"));
    assert_eq!(summary["exact_product_commit_match_count"], json!(1));
    assert_eq!(summary["typed_product_patch_count"], json!(0));
    assert_eq!(summary["product_patch_missing_count"], json!(1));
    assert_eq!(
        summary["sample_failures"][0]["reason"],
        json!("missing_typed_product_patch")
    );
}


#[test]
fn cells_visible_click_product_commit_scope_requires_typed_product_result() {
    let key = json!({
        "frame_seq": 9,
        "present_id": 9,
        "input_event_seq": 6,
        "surface_id": "preview:test",
        "surface_epoch": 1,
        "content_revision": 8,
        "layout_revision": 1,
        "render_scene_revision": 6
    });
    let live_probe = json!({
        "adapter_identity": cells_visible_click_test_hardware_adapter(),
        "click_samples": [{
            "index": 0,
            "target_address": "A2",
            "product_frame_evidence_key": key,
            "input_accept_to_present_ms": 8.0,
            "input_accept_to_formula_visible_ms": 8.0,
            "product_formula_state_current": true
        }],
        "recent_product_frame_commits": [{
            "frame_lane": "product_interaction",
            "input_event_seq": 6,
            "input_to_present_ms": 8.0,
            "product_result_source": "missing_product_result",
            "input_timing": {
                "input_wake_to_input_accept_ms": 1.0,
                "input_wake_to_present_ms": 9.0
            },
            "frame_evidence_key": {
                "frame_seq": 9,
                "present_id": 9,
                "input_event_seq": 6,
                "surface_id": "preview:test",
                "surface_epoch": 1,
                "content_revision": 8,
                "layout_revision": 1,
                "render_scene_revision": 6
            },
            "product_frame": {
                "product_patch": cells_visible_click_test_product_patch()
            }
        }]
    });

    let summary =
        cells_visible_click_app_window_product_commit_scope_summary(&live_probe, 1, 16.7, 33.4);

    assert_eq!(summary["status"], json!("fail"));
    assert_eq!(summary["timing_status"], json!("fail"));
    assert_eq!(summary["exact_product_commit_match_count"], json!(1));
    assert_eq!(summary["typed_product_patch_count"], json!(1));
    assert_eq!(summary["typed_product_result_count"], json!(0));
    assert_eq!(
        summary["sample_failures"][0]["reason"],
        json!("missing_typed_product_result")
    );
}


