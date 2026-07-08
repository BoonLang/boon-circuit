#[test]
fn cells_visible_click_visual_proof_requires_exact_product_frame_key() {
    let product_key = json!({
        "frame_seq": 7,
        "present_id": 7,
        "input_event_seq": 4,
        "surface_id": "preview:test",
        "surface_epoch": 1,
        "content_revision": 6,
        "layout_revision": 1,
        "render_scene_revision": 4
    });
    let later_key = json!({
        "frame_seq": 10,
        "present_id": 10,
        "input_event_seq": 5,
        "surface_id": "preview:test",
        "surface_epoch": 1,
        "content_revision": 6,
        "layout_revision": 1,
        "render_scene_revision": 4
    });
    let product_present_probe = json!({
        "status": "pass",
        "frame_evidence_key": product_key
    });
    let later_readback_probe = json!({
        "status": "pass",
        "frame_evidence_key": later_key,
        "last_external_render_proof": {
            "proof": {
                "frame_evidence_key": later_key
            }
        }
    });
    let later_visual_probe = json!({
        "status": "pass",
        "structured_external_visible_surface_probe": {
            "proof_frame_evidence_key": later_key
        }
    });

    assert!(
        cells_visual_formula_probe_needs_exact_product_replacement(
            &product_present_probe,
            Some(&later_readback_probe),
            Some(&later_visual_probe),
        ),
        "a later-frame proof is proof lag, not the product sample's visual proof"
    );
    assert!(!cells_visual_formula_probe_proves_frame_key(
        Some(&later_readback_probe),
        Some(&later_visual_probe),
        product_present_probe.get("frame_evidence_key").unwrap(),
    ));

    let exact_readback_probe = json!({
        "status": "pass",
        "frame_evidence_key": product_present_probe["frame_evidence_key"],
        "last_external_render_proof": {
            "proof": {
                "frame_evidence_key": product_present_probe["frame_evidence_key"]
            }
        }
    });
    let exact_visual_probe = json!({
        "status": "pass",
        "structured_external_visible_surface_probe": {
            "proof_frame_evidence_key": product_present_probe["frame_evidence_key"]
        }
    });

    assert!(cells_visual_formula_probe_proves_frame_key(
        Some(&exact_readback_probe),
        Some(&exact_visual_probe),
        product_present_probe.get("frame_evidence_key").unwrap(),
    ));
    assert!(!cells_visual_formula_probe_needs_exact_product_replacement(
        &product_present_probe,
        Some(&exact_readback_probe),
        Some(&exact_visual_probe),
    ));
}


#[test]
fn native_gpu_handoff_requires_cells_visible_click_release_report() {
    let reports = native_gpu_handoff_required_reports();
    let report = reports
        .iter()
        .find(|report| report.label == "cells-visible-click-e2e-release")
        .expect("native GPU handoff must require the Cells visible-click release gate");
    assert_eq!(
        report.path,
        PathBuf::from("target/reports/native-gpu/cells-visible-click-e2e-release.json")
    );
    assert_eq!(report.command, "verify-native-cells-visible-click-e2e");
    assert_eq!(report.required_argv, &[("--profile", "release")]);
}


#[test]
fn native_gpu_label_contract_rejects_cells_visible_click_address_selection_fallback() {
    let report = json!({
        "command": "verify-native-cells-visible-click-e2e",
        "profile": "release",
        "operator_host_input": true,
        "real_os_input": false,
        "input_injection_method": "preview_verifier_app_owned_native_input_adapter",
        "direct_runtime_state_mutation": false,
        "target_count": 16,
        "input_accept_to_formula_visible_ms_p95": 12.0,
        "input_wake_to_formula_visible_ms_p95": 12.0,
        "click_to_formula_visible_ms_p95": 24.0,
        "max_click_to_formula_ms": 16.7,
        "max_click_to_present_ms": 33.4,
        "steady_input_accept_to_formula_visible_ms": {
            "p95": 12.0
        },
        "steady_input_accept_to_present_ms": {
            "p95": 11.0
        },
        "steady_input_wake_to_input_accept_ms": {
            "p95": 4.0
        },
        "steady_input_wake_to_present_ms": {
            "p95": 12.0
        },
        "steady_input_wake_to_formula_visible_ms": {
            "p95": 12.0
        },
        "preview_perf_stats": {
            "input_to_present_ms_p50_p95_p99_max": {
                "p95": 12.0,
                "sample_count": 16
            },
            "render_loop_mode": "demand_driven",
            "missed_frame_count": 0
        },
        "post_present_proof_isolation": {
            "status": "pass",
            "product_path_status": "pass",
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
            "recent_queue_count": 16
        },
        "product_only_ux_contract": {
            "status": "pass",
            "source": "app_window_product_frame_commits",
            "proof_completion_required": false,
            "input_to_present_ms": {
                "p95": 12.0,
                "max": 12.0
            },
            "input_to_formula_visible_ms": {
                "p95": 12.0,
                "max": 12.0
            },
            "product_latency_includes_proof_completion": false,
            "product_blocks_on_proof_subscribers": false,
            "hot_path_report_write_count": 0,
            "product_render_graph_count": 16,
            "product_render_graph_renderer_owned_count": 16,
            "present_plan_count": 16,
            "product_render_graph_missing_count": 0,
            "product_render_graph_renderer_owned_missing_count": 0,
            "present_plan_missing_count": 0,
            "product_render_graph_proof_readback_count": 0,
            "present_plan_proof_readback_in_product_pass_count": 0,
            "product_patch_full_scene_build_count": 0
        },
        "proof_only_contract": {
            "status": "pass",
            "visual_capture_method": "app-owned-wgpu-readback",
            "click_sample_count": 16,
            "exact_visual_proof_sample_count": 0,
            "current_structured_visual_proof_sample_count": 16,
            "proof_lag_max_frames": 1,
            "proof_lag_frame_budget": 8
        },
        "proof_isolation_contract": {
            "status": "pass",
            "product_latency_includes_proof_completion": false,
            "hot_path_report_write_count": 0
        },
        "preview_loop_product_path_contract": {
            "status": "pass",
            "source": "app_window_product_frame_commits",
            "input_to_present_ms_p95": 12.0,
            "input_to_present_sample_count": 16,
            "missed_frame_count": 0,
            "product_frame_scope": {
                "input_to_present_ms_max": 12.0,
                "hard_failure_count": 0
            },
            "renders_per_second": 60.0,
            "render_loop_mode": "demand_driven",
            "frame_pacing_state": "requested_animation_burst",
            "budget_ms": 16.7
        },
        "preview_loop_input_to_present_ms_p95": 230.0,
        "preview_loop_input_to_present_sample_count": 16,
        "preview_loop_missed_frame_count": 2,
        "preview_loop_render_loop_mode": "demand_driven",
        "click_to_visible_max_budget_or_bounded_outliers_pass": true,
        "proof_current_changed": true,
        "retained_update_contract": {
            "status": "pass",
            "address_selection_fallback_count": 0
        },
        "runtime_work_contract": {
            "status": "pass"
        },
        "formula_transition_contract": {
            "status": "pass"
        },
        "selected_cell_transition_contract": {
            "status": "pass"
        }
    });
    assert!(
        native_gpu_label_contract_blockers("cells-visible-click-e2e-release", &report).is_empty(),
        "well-formed release visible-click report should satisfy the label contract"
    );
    let mut isolated_real_window_report = report.clone();
    isolated_real_window_report["operator_host_input"] = json!(false);
    isolated_real_window_report["real_os_input"] = json!(true);
    isolated_real_window_report["input_injection_method"] = json!(
        "weston_test_control_real_wayland_pointer_move_settle_then_button_only_no_preview_ipc_fallback"
    );
    let blockers = native_gpu_label_contract_blockers(
        "cells-visible-click-e2e-release",
        &isolated_real_window_report,
    );
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("canonical headed app-owned host input")),
        "isolated Weston must not satisfy the handoff visible-click label contract: {blockers:?}"
    );
    let headed_report = report.clone();
    let mut headed_warmup_report = headed_report.clone();
    headed_warmup_report["target_count"] = json!(64);
    headed_warmup_report["steady_input_wake_to_input_accept_ms"] = json!({"p95": null});
    headed_warmup_report["steady_input_wake_to_present_ms"] = json!({"p95": null});
    headed_warmup_report["steady_input_wake_to_formula_visible_ms"] = json!({"p95": null});
    headed_warmup_report["product_only_ux_contract"]["sample_count"] = json!(60);
    headed_warmup_report["product_only_ux_contract"]["product_render_graph_count"] = json!(60);
    headed_warmup_report["product_only_ux_contract"]["product_render_graph_renderer_owned_count"] =
        json!(60);
    headed_warmup_report["product_only_ux_contract"]["present_plan_count"] = json!(60);
    headed_warmup_report["proof_only_contract"]["click_sample_count"] = json!(64);
    headed_warmup_report["proof_only_contract"]["current_structured_visual_proof_sample_count"] =
        json!(64);
    headed_warmup_report["preview_loop_product_path_contract"]
        .as_object_mut()
        .unwrap()
        .remove("input_to_present_sample_count");
    headed_warmup_report["preview_loop_product_path_contract"]["product_frame_scope"]["required_sample_count"] =
        json!(64);
    headed_warmup_report["preview_loop_product_path_contract"]["product_frame_scope"]["warmup_sample_count"] =
        json!(4);
    headed_warmup_report["preview_loop_product_path_contract"]["product_frame_scope"]["measured_required_sample_count"] =
        json!(60);
    headed_warmup_report["preview_loop_product_path_contract"]["product_frame_scope"]["product_frame_sample_count"] =
        json!(60);
    assert!(
        native_gpu_label_contract_blockers(
            "cells-visible-click-e2e-release",
            &headed_warmup_report
        )
        .is_empty(),
        "headed reports with warmup-excluded product samples and null wake timing should satisfy the label contract: {:?}",
        native_gpu_label_contract_blockers(
            "cells-visible-click-e2e-release",
            &headed_warmup_report
        )
    );

    let mut proof_coupled_report = report.clone();
    proof_coupled_report["post_present_proof_isolation"]["product_latency_includes_proof_completion"] =
        json!(true);
    let blockers = native_gpu_label_contract_blockers(
        "cells-visible-click-e2e-release",
        &proof_coupled_report,
    );
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("product_latency_includes_proof_completion")),
        "label contract must reject product/proof latency coupling: {blockers:?}"
    );

    let mut missing_graph_report = report.clone();
    missing_graph_report["product_only_ux_contract"]["product_render_graph_count"] = json!(0);
    let blockers = native_gpu_label_contract_blockers(
        "cells-visible-click-e2e-release",
        &missing_graph_report,
    );
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("product_render_graph_count")),
        "label contract must reject missing product render graph evidence: {blockers:?}"
    );

    let mut missing_proof_report = report.clone();
    missing_proof_report["proof_only_contract"]["current_structured_visual_proof_sample_count"] =
        json!(4);
    missing_proof_report["proof_only_contract"]["status"] = json!("fail");
    let blockers = native_gpu_label_contract_blockers(
        "cells-visible-click-e2e-release",
        &missing_proof_report,
    );
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("proof_only_contract")),
        "label contract must reject missing proof-lane samples: {blockers:?}"
    );

    let mut bounded_outlier_report = report.clone();
    bounded_outlier_report["preview_loop_product_path_contract"]["missed_frame_count"] = json!(1);
    bounded_outlier_report["preview_loop_product_path_contract"]["product_frame_scope"]["input_to_present_ms_max"] =
        json!(19.0);
    assert!(
        native_gpu_label_contract_blockers(
            "cells-visible-click-e2e-release",
            &bounded_outlier_report
        )
        .is_empty(),
        "release label contract should accept p95-good product frames with bounded max outliers"
    );

    let mut unbounded_outlier_report = bounded_outlier_report;
    unbounded_outlier_report["preview_loop_product_path_contract"]["product_frame_scope"]["input_to_present_ms_max"] =
        json!(40.0);
    let blockers = native_gpu_label_contract_blockers(
        "cells-visible-click-e2e-release",
        &unbounded_outlier_report,
    );
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("bounded max")),
        "label contract must reject product max outliers above max_click_to_present_ms: {blockers:?}"
    );

    let mut fallback_report = report;
    fallback_report["retained_update_contract"]["address_selection_fallback_count"] = json!(1);
    let blockers =
        native_gpu_label_contract_blockers("cells-visible-click-e2e-release", &fallback_report);
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("address selection fallback")),
        "label contract must reject selection fallback: {blockers:?}"
    );
}


