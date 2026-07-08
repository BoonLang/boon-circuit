#[test]
fn cells_visible_click_product_commit_currentness_does_not_require_readback_proof() {
    let frame_key = json!({
        "frame_seq": 12,
        "present_id": 12,
        "input_event_seq": 9,
        "surface_id": "preview:test",
        "surface_epoch": 1,
        "content_revision": 7,
        "layout_revision": 2,
        "render_scene_revision": 5
    });
    let product_commit = json!({
        "frame_lane": "product_interaction",
        "input_to_present_ms": 8.0,
        "frame_evidence_key": frame_key,
        "product_frame": {
            "product_patch": cells_visible_click_test_product_patch()
        },
        "render_graph": cells_visible_click_test_product_render_graph(),
        "present_plan": cells_visible_click_test_present_plan()
    });
    let product_match = json!({
        "status": "pass",
        "match_method": "exact_product_commit",
        "product_frame_commit": product_commit
    });

    assert!(cells_visible_click_product_commit_proves_visible_update(
        &product_match,
        product_match
            .get("product_frame_commit")
            .expect("test product commit"),
        8.0,
    ));
    assert!(
        cells_visible_click_product_present_probe_proves_visible_update(&json!({
            "status": "pass",
            "input_accept_to_present_ms": 8.0,
            "product_frame_commit": product_match["product_frame_commit"].clone(),
            "readback_probe": {
                "status": "proof-pending"
            }
        }))
    );
}


#[test]
fn cells_visible_click_lane_contracts_accept_split_product_and_proof_paths() {
    let product_frames = json!({
        "status": "pass",
        "source": "app_window_product_frame_commits",
        "adapter_identity": cells_visible_click_test_hardware_adapter(),
        "adapter_status": "hardware",
        "product_frame_sample_count": 16,
        "exact_product_commit_match_count": 16,
        "typed_product_patch_count": 16,
        "typed_product_result_count": 16,
        "product_render_graph_count": 16,
        "product_render_graph_renderer_owned_count": 16,
        "present_plan_count": 16,
        "product_result_missing_count": 0,
        "product_patch_full_scene_build_count": 0,
        "product_patch_proof_json_required_count": 0,
        "product_patch_latest_report_required_count": 0,
        "product_render_graph_missing_count": 0,
        "product_render_graph_renderer_owned_missing_count": 0,
        "present_plan_missing_count": 0,
        "product_render_graph_proof_readback_count": 0,
        "present_plan_proof_readback_in_product_pass_count": 0,
        "hard_failure_count": 0,
        "input_to_present_ms": {
            "p95": 12.0,
            "max": 20.0
        },
        "input_to_formula_visible_ms": {
            "p95": 11.0,
            "max": 19.0
        }
    });
    let runtime = json!({"status": "pass"});
    let retained = json!({"status": "pass"});
    let isolation = cells_visible_click_test_post_present_isolation();
    let product_contract = cells_visible_click_product_only_ux_contract(
        &product_frames,
        &runtime,
        &retained,
        &isolation,
        true,
        "demand_driven",
        16.7,
        33.4,
    );
    assert_eq!(
        product_contract
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("pass")
    );

    let probe = json!({
        "visual_capture_method": "app-owned-wgpu-readback",
        "readback_ok": true,
        "proof_current_changed": true,
        "click_samples": [
            {
                "index": 0,
                "visual_proof_proves_presented_frame": true,
                "proof_current_changed": true,
                "product_frame_evidence_key": {
                    "frame_seq": 10,
                    "input_event_seq": 7
                },
                "proof_frame_evidence_key": {
                    "frame_seq": 11,
                    "input_event_seq": 7
                }
            }
        ]
    });
    let proof_contract = cells_visible_click_proof_only_contract(&probe, 3);
    assert_eq!(
        proof_contract
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("pass")
    );

    let isolation_contract = cells_visible_click_proof_isolation_contract(&isolation);
    assert_eq!(
        isolation_contract
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("pass")
    );
}


#[test]
fn cells_visible_click_lane_contracts_reject_proof_coupling_and_missing_proof() {
    let product_frames = json!({
        "status": "pass",
        "source": "app_window_product_frame_commits",
        "adapter_identity": cells_visible_click_test_hardware_adapter(),
        "adapter_status": "hardware",
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
            "p95": 12.0,
            "max": 20.0
        },
        "input_to_formula_visible_ms": {
            "p95": 11.0,
            "max": 19.0
        }
    });
    let runtime = json!({"status": "pass"});
    let retained = json!({"status": "pass"});
    let mut isolation = cells_visible_click_test_post_present_isolation();
    isolation["product_latency_includes_proof_completion"] = json!(true);
    let product_contract = cells_visible_click_product_only_ux_contract(
        &product_frames,
        &runtime,
        &retained,
        &isolation,
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

    let proof_contract = cells_visible_click_proof_only_contract(
        &json!({
            "visual_capture_method": "app-owned-wgpu-readback",
            "readback_ok": true,
            "proof_current_changed": true,
            "click_samples": [{
                "index": 0,
                "visual_proof_proves_presented_frame": false,
                "proof_current_changed": true,
                "product_frame_evidence_key": {
                    "frame_seq": 10,
                    "input_event_seq": 7
                }
            }]
        }),
        3,
    );
    assert_eq!(
        proof_contract
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("fail")
    );

    let isolation_contract = cells_visible_click_proof_isolation_contract(&isolation);
    assert_eq!(
        isolation_contract
            .get("status")
            .and_then(serde_json::Value::as_str),
        Some("fail")
    );
}


