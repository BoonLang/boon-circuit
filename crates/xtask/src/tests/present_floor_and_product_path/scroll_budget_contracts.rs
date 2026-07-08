#[test]
fn product_path_timing_rejects_idle_samples_for_scroll_budget() {
    let mut report = json!({
        "preview_frame_ms_p95": 10.0,
        "speed_timing_window": "post-real-window-input",
        "post_input_frame_timing": {
            "measured_frame_count": 30
        },
        "render_loop_state": {
            "requested_animation_burst_count": 4
        },
        "preview_perf_stats": {
            "render_loop_mode": "demand_driven",
            "frame_pacing": {"state": "idle"},
            "input_to_present_ms_p50_p95_p99_max": {
                "sample_count": 4,
                "p50": 7.0,
                "p95": 8.0,
                "p99": 8.5,
                "max": 9.0
            }
        },
        "operator_host_wheel_input": true,
        "app_owned_window_input": true,
        "real_window_input": true,
        "native_input_adapter": {
            "installed": true,
            "mouse_scroll_event_count": 2,
            "scroll_delta_x": 240.0,
            "scroll_delta_y": 360.0
        },
        "preview_surface_proof": preview_surface_with_metrics(json!({
            "adapter_name": "hardware",
            "adapter_device_type": "DiscreteGpu",
            "adapter_is_software": false,
            "present_mode": "Mailbox"
        }), json!({
                "upload_bytes": 0,
                "draw_calls": 1,
                "queue_write_count": 0
        }))
    });

    add_native_scroll_model_evidence(&mut report, "generic", false);

    assert_eq!(
        report
            .pointer("/product_path_ux_timing/status")
            .and_then(serde_json::Value::as_str),
        Some("not-requested-animation-burst-sample")
    );
    assert_eq!(
        report
            .get("product_path_ux_timing_proven")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report
            .get("speed_budget_timing_window")
            .and_then(serde_json::Value::as_str),
        Some("post-real-window-input")
    );
}


#[test]
fn product_path_timing_accepts_product_interaction_samples_after_burst_exits() {
    let mut report = json!({
        "preview_frame_ms_p95": 135.0,
        "speed_timing_window": "product-path-preview-perf-stats",
        "post_input_frame_timing": {
            "measured_frame_count": 30
        },
        "render_loop_state": {
            "requested_animation_burst_count": 1
        },
        "preview_perf_stats": {
            "render_loop_mode": "demand_driven",
            "frame_lane": "product_interaction",
            "product_frame_count": 30,
            "frame_pacing": {"state": "idle"},
            "input_to_present_ms_p50_p95_p99_max": {
                "sample_count": 30,
                "p50": 0.35,
                "p95": 0.75,
                "p99": 1.2,
                "max": 1.45
            },
            "render_hook_ms_p50_p95_p99_max": {
                "sample_count": 37,
                "p50": 0.17,
                "p95": 1.12,
                "p99": 25.0,
                "max": 25.3
            },
            "present_path_ms_p50_p95_p99_max": {
                "sample_count": 37,
                "p50": 0.12,
                "p95": 0.23,
                "p99": 0.29,
                "max": 0.30
            }
        },
        "operator_host_wheel_input": true,
        "app_owned_window_input": true,
        "real_window_input": true,
        "native_input_adapter": {
            "installed": true,
            "mouse_scroll_event_count": 30,
            "scroll_delta_x": 240.0,
            "scroll_delta_y": 360.0
        },
        "preview_surface_proof": preview_surface_with_metrics(json!({
            "adapter_name": "hardware",
            "adapter_device_type": "DiscreteGpu",
            "adapter_is_software": false,
            "present_mode": "Mailbox"
        }), json!({
                "upload_bytes": 0,
                "draw_calls": 3,
                "queue_write_count": 0
        }))
    });

    add_native_scroll_model_evidence(&mut report, "cells", false);

    assert_eq!(
        report
            .pointer("/product_path_ux_timing/status")
            .and_then(serde_json::Value::as_str),
        Some("pass")
    );
    assert_eq!(
        report
            .pointer("/product_path_ux_timing/frame_pacing_state_at_sample")
            .and_then(serde_json::Value::as_str),
        Some("idle")
    );
    assert_eq!(
        report
            .pointer("/product_path_ux_timing/product_interaction_lane_sample")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("product_path_ux_timing_proven")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("speed_budget_timing_window")
            .and_then(serde_json::Value::as_str),
        Some("product-path-input-to-present")
    );
    assert_eq!(
        report
            .get("speed_budget_frame_ms_p95")
            .and_then(serde_json::Value::as_f64),
        Some(0.75)
    );
    assert_eq!(
        report
            .get("budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}


#[test]
fn scroll_budget_contract_uses_product_ux_lane_when_selected() {
    let report = json!({
        "display_server": "wayland",
        "budget_pass": true,
        "preview_frame_ms_p95": 29.0,
        "speed_budget_timing_window": "product-path-input-to-present",
        "speed_budget_frame_ms_p95": 2.0,
        "wheel_to_visible_ms_p95": 2.0,
        "ux_frame_budget_pass": true,
        "product_path_ux_timing_proven": true,
        "product_path_ux_timing": {
            "status": "pass",
            "proof_latency_excluded": true
        },
        "missed_frame_count": 0,
        "dropped_frame_count": 0,
        "longest_visible_stall_ms": 2.8,
        "non_os_scroll_model": {
            "status": "pass",
            "frame_budget_model_pass": true
        }
    });
    let mut blockers = Vec::new();

    require_scroll_budget_fields(&mut blockers, &report);

    assert!(
        blockers.is_empty(),
        "product UX lane should own scroll speed acceptance; blockers={blockers:?}"
    );
}


#[test]
fn scroll_budget_contract_rejects_polluted_product_ux_lane() {
    let report = json!({
        "display_server": "wayland",
        "budget_pass": true,
        "preview_frame_ms_p95": 12.0,
        "speed_budget_timing_window": "product-path-input-to-present",
        "speed_budget_frame_ms_p95": 18.0,
        "wheel_to_visible_ms_p95": 18.0,
        "ux_frame_budget_pass": false,
        "product_path_ux_timing_proven": true,
        "product_path_ux_timing": {
            "status": "pass",
            "proof_latency_excluded": false
        },
        "missed_frame_count": 0,
        "dropped_frame_count": 0,
        "longest_visible_stall_ms": 18.0,
        "non_os_scroll_model": {
            "status": "pass",
            "frame_budget_model_pass": true
        }
    });
    let mut blockers = Vec::new();

    require_scroll_budget_fields(&mut blockers, &report);

    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("ux_frame_budget_pass")),
        "over-budget UX lane must fail; blockers={blockers:?}"
    );
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("proof_latency_excluded")),
        "product UX lane must exclude proof latency; blockers={blockers:?}"
    );
}


#[test]
fn product_path_timing_rejects_continuous_probe_for_scroll_budget() {
    let mut report = json!({
        "preview_frame_ms_p95": 0.0,
        "render_loop_state": {
            "requested_animation_burst_count": 1
        },
        "preview_perf_stats": {
            "render_loop_mode": "continuous_probe",
            "frame_pacing": {"state": "probe"},
            "input_to_present_ms_p50_p95_p99_max": {
                "sample_count": 3,
                "p50": 6.0,
                "p95": 7.0,
                "p99": 7.5,
                "max": 8.0
            }
        },
        "app_owned_window_input": true,
        "real_window_input": true,
        "native_input_adapter": {
            "installed": true,
            "mouse_scroll_event_count": 2,
            "scroll_delta_x": 240.0,
            "scroll_delta_y": 360.0
        },
        "preview_surface_proof": preview_surface_with_metrics(json!({
            "adapter_is_software": false
        }), json!({
                "upload_bytes": 0,
                "draw_calls": 1
        }))
    });

    add_native_scroll_model_evidence(&mut report, "generic", false);

    assert_eq!(
        report
            .pointer("/product_path_ux_timing/status")
            .and_then(serde_json::Value::as_str),
        Some("not-demand-driven-product-path")
    );
    assert_eq!(
        report
            .get("product_path_ux_timing_proven")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report
            .get("required_real_window_speed_proven")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
}


#[test]
fn product_path_rejects_single_frame_timing_without_sustained_samples() {
    let mut report = json!({
        "preview_frame_ms_p95": 24.0,
        "frame_input_to_present_ms": 9.0,
        "render_loop_state": {
            "requested_animation_burst_count": 1
        },
        "preview_perf_stats": {
            "render_loop_mode": "demand_driven",
            "frame_pacing": {"state": "requested_animation_burst"},
            "input_to_present_ms_p50_p95_p99_max": {
                "sample_count": 0,
                "p50": null,
                "p95": null,
                "p99": null,
                "max": null
            }
        },
        "operator_host_wheel_input": true,
        "app_owned_window_input": true,
        "real_window_input": true,
        "native_input_adapter": {
            "installed": true,
            "mouse_scroll_event_count": 2,
            "scroll_delta_x": 240.0,
            "scroll_delta_y": 360.0
        },
        "preview_surface_proof": preview_surface_with_metrics(json!({
            "adapter_is_software": false
        }), json!({
                "upload_bytes": 0,
                "draw_calls": 1
        }))
    });

    add_native_scroll_model_evidence(&mut report, "generic", false);

    assert_eq!(
        report
            .pointer("/product_path_ux_timing/timing_source")
            .and_then(serde_json::Value::as_str),
        Some("preview_perf_stats.input_to_present_ms_p50_p95_p99_max")
    );
    assert_eq!(
        report
            .pointer("/product_path_ux_timing/sample_count")
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );
    assert_eq!(
        report
            .pointer("/product_path_ux_timing/status")
            .and_then(serde_json::Value::as_str),
        Some("missing-input-to-present-samples")
    );
    assert_eq!(
        report
            .pointer("/product_path_ux_timing/min_sample_count")
            .and_then(serde_json::Value::as_u64),
        Some(4)
    );
    assert_eq!(
        report
            .pointer("/product_path_ux_timing/sustained_sample_count_pass")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report
            .get("product_path_input_to_present_ms_p95")
            .and_then(serde_json::Value::as_f64),
        None
    );
    assert_eq!(
        report
            .get("speed_budget_frame_ms_p95")
            .and_then(serde_json::Value::as_f64),
        Some(24.0)
    );
    assert_eq!(
        report
            .get("product_path_ux_timing_proven")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report
            .get("budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );

    let mut missing_burst = report.clone();
    missing_burst["render_loop_state"]["requested_animation_burst_count"] = json!(0);
    add_native_scroll_model_evidence(&mut missing_burst, "generic", false);
    assert_eq!(
        missing_burst
            .pointer("/product_path_ux_timing/status")
            .and_then(serde_json::Value::as_str),
        Some("missing-input-to-present-samples")
    );
    assert_eq!(
        missing_burst
            .get("product_path_ux_timing_proven")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
}


