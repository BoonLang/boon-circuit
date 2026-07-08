#[test]
fn scroll_loop_promotion_accepts_newer_epoch_same_frame_readback_without_external_proof() {
    let frame_key = json!({
        "frame_seq": 36,
        "content_revision": 26,
        "layout_revision": 3,
        "render_scene_revision": 4,
        "surface_id": "preview:test-surface",
        "surface_epoch": 3,
        "input_event_seq": 60,
        "present_id": 36,
        "proof_request_id": null
    });
    let loop_report = json!({
        "status": "pass",
        "surface_id": "preview:test-surface",
        "surface_epoch": 3,
        "frame_evidence_key": frame_key.clone(),
        "preview_perf_stats": {
            "render_loop_mode": "demand_driven",
            "product_frame_count": 4,
            "product_path_input_to_present_ms_p50_p95_p99_max": {
                "sample_count": 4,
                "p50": 6.0,
                "p95": 8.0,
                "p99": 9.0,
                "max": 10.0
            }
        },
        "observed_input_adapter": {
            "installed": true,
            "real_os_events_observed": true,
            "synthetic_input_probe": true,
            "input_injection_method": "app_window_per_window_interactive_synthetic_scroll_harness",
            "mouse_scroll_event_count": 30,
            "mouse_motion_event_count": 30,
            "scroll_delta_x": 9600.0,
            "scroll_delta_y": 19200.0,
            "mouse_last_window_protocol_id": 7,
            "mouse_window_pos": {
                "x": 514.0,
                "y": 545.0,
                "window_width": 1020.0,
                "window_height": 1082.0
            }
        },
        "last_interactive_readback_artifact": {
            "capture_method": "wgpu-visible-surface-copy-src-readback",
            "readback_poll_status": "completed_before_deadline",
            "nonblank_samples": 32,
            "frame_evidence_key": frame_key.clone()
        },
        "matching_interactive_readback_artifact_for_frame_status": "matched"
    });
    let mut report = json!({
        "preview_surface_proof": {
            "surface_id": "preview:test-surface",
            "surface_epoch": 1,
            "input_adapter": {
                "installed": true,
                "real_os_events_observed": false,
                "synthetic_input_probe": true,
                "mouse_scroll_event_count": 0,
                "scroll_delta_x": 0.0,
                "scroll_delta_y": 0.0
            }
        }
    });

    assert!(same_frame_scroll_readback_proven(&loop_report));
    promote_scroll_loop_report_evidence(
        &mut report,
        &loop_report,
        "preview_surface_proof",
        "preview_loop_report",
    );

    assert_eq!(
        report
            .get("scroll_loop_report_evidence_promoted")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("scroll_loop_report_surface_id_match")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("scroll_loop_report_surface_epoch_advanced")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("scroll_loop_report_same_frame_readback_proven")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .pointer("/native_input_adapter/mouse_scroll_event_count")
            .and_then(serde_json::Value::as_u64),
        Some(30)
    );
    assert_eq!(
        report
            .get("app_owned_window_input")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("real_window_input")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report.pointer("/readback_artifacts/0/frame_evidence_key"),
        Some(&frame_key)
    );
}


#[test]
fn axis_specific_product_path_timing_promotes_sustained_samples() {
    let product_sample = |axis: &str, attempt: u64, p95: f64| {
        json!({
            "axis": axis,
            "attempt": attempt,
            "render_loop_mode": "demand_driven",
            "frame_pacing_state": "requested_animation_burst",
            "requested_animation_burst_count": 1,
            "input_to_present_ms_p50_p95_p99_max": {
                "sample_count": 1,
                "p50": p95 - 1.0,
                "p95": p95,
                "p99": p95 + 0.2,
                "max": p95 + 0.5
            }
        })
    };
    let mut report = json!({
        "preview_frame_ms_p95": 24.0,
        "speed_timing_window": "axis-specific-post-real-window-input",
        "axis_specific_real_window_scroll_observation": {
            "status": "pass",
            "combined_input_adapter": {
                "installed": true,
                "real_os_events_observed": true,
                "synthetic_input_probe": false,
                "mouse_scroll_event_count": 4,
                "scroll_delta_x": 240.0,
                "scroll_delta_y": 360.0
            },
            "product_path_timing": {
                "status": "pass",
                "source": "axis-specific-product-path-input-to-present",
                "render_loop_mode": "demand_driven",
                "frame_pacing": {"state": "requested_animation_burst"},
                "requested_animation_burst_count": 4,
                "input_to_present_ms_p50_p95_p99_max": {
                    "sample_count": 4,
                    "p50": 8.0,
                    "p95": 9.0,
                    "p99": 9.2,
                    "max": 9.5
                },
                "present_call_ms_p50_p95_p99_max": {
                    "sample_count": 4,
                    "p50": 5.0,
                    "p95": 6.0,
                    "p99": 6.2,
                    "max": 6.5
                },
                "frame_present_call_ms_p50_p95_p99_max": {
                    "sample_count": 4,
                    "p50": 5.0,
                    "p95": 6.0,
                    "p99": 6.2,
                    "max": 6.5
                },
                "surface_acquire_call_ms_p50_p95_p99_max": {
                    "sample_count": 4,
                    "p50": 0.2,
                    "p95": 0.3,
                    "p99": 0.4,
                    "max": 0.5
                },
                "queue_submit_call_ms_p50_p95_p99_max": {
                    "sample_count": 4,
                    "p50": 0.4,
                    "p95": 0.5,
                    "p99": 0.6,
                    "max": 0.7
                },
                "present_path_ms_p50_p95_p99_max": {
                    "sample_count": 4,
                    "p50": 5.6,
                    "p95": 6.8,
                    "p99": 7.1,
                    "max": 7.7
                },
                "samples": [
                    product_sample("vertical", 1, 7.0),
                    product_sample("vertical", 2, 8.0),
                    product_sample("horizontal", 1, 8.5),
                    product_sample("horizontal", 2, 9.0)
                ]
            },
            "vertical_observation": {
                "measured_loop_report": {
                    "present_path_mode": "direct_visible_surface",
                    "present_path_requested_mode": "direct_visible_surface",
                    "present_path_selection_reason": "default_direct_visible_surface_with_separate_readback",
                    "present_path_hooks_present": true,
                    "present_path_surface_copy_to_present_supported": true,
                    "present_path_readback_enabled": true,
                    "last_render_target_kind": "visible-surface-direct"
                },
                "render_hook_app_owned_proof_skipped": true,
                "surface_post_input_frame_timing": {
                    "measured_frame_count": 30,
                    "presented_frame_ms_p50": 18.0,
                    "presented_frame_ms_p95": 24.0,
                    "presented_frame_ms_p99": 25.0,
                    "presented_frame_ms_max": 25.0,
                    "command_record_ms_p95": 3.0,
                    "encoder_finish_ms_p95": 0.3,
                    "queue_submit_ms_p95": 12.0,
                    "frame_present_ms_p95": 8.0,
                    "sample_frame_count": 30,
                    "warmup_frame_count": 3
                }
            },
            "horizontal_observation": {
                "measured_loop_report": {
                    "present_path_mode": "direct_visible_surface",
                    "present_path_requested_mode": "direct_visible_surface",
                    "present_path_selection_reason": "default_direct_visible_surface_with_separate_readback",
                    "present_path_hooks_present": true,
                    "present_path_surface_copy_to_present_supported": true,
                    "present_path_readback_enabled": true,
                    "last_render_target_kind": "visible-surface-direct"
                },
                "render_hook_app_owned_proof_skipped": true,
                "surface_post_input_frame_timing": {
                    "measured_frame_count": 30,
                    "presented_frame_ms_p50": 19.0,
                    "presented_frame_ms_p95": 25.0,
                    "presented_frame_ms_p99": 26.0,
                    "presented_frame_ms_max": 26.0,
                    "command_record_ms_p95": 3.2,
                    "encoder_finish_ms_p95": 0.4,
                    "queue_submit_ms_p95": 12.2,
                    "frame_present_ms_p95": 8.1,
                    "sample_frame_count": 30,
                    "warmup_frame_count": 3
                }
            }
        },
        "app_owned_window_input": true,
        "real_window_input": true,
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

    assert!(promote_axis_specific_scroll_timing(&mut report));
    add_native_scroll_model_evidence(&mut report, "generic", false);

    assert_eq!(
        report
            .pointer("/preview_perf_stats/input_to_present_ms_p50_p95_p99_max/sample_count")
            .and_then(serde_json::Value::as_u64),
        Some(4)
    );
    assert_eq!(
        report
            .pointer("/preview_perf_stats/present_path_ms_p50_p95_p99_max/p95")
            .and_then(serde_json::Value::as_f64),
        Some(6.8)
    );
    assert_eq!(
        report
            .pointer("/product_path_ux_timing/present_path_ms_p95")
            .and_then(serde_json::Value::as_f64),
        Some(6.8)
    );
    assert_eq!(
        report
            .pointer("/frame_budget_split/preview_perf_frame_present_call_ms_p95")
            .and_then(serde_json::Value::as_f64),
        Some(6.0)
    );
    assert_eq!(
        report
            .get("present_path_mode")
            .and_then(serde_json::Value::as_str),
        Some("direct_visible_surface")
    );
    assert_eq!(
        report
            .get("present_path_requested_mode")
            .and_then(serde_json::Value::as_str),
        Some("direct_visible_surface")
    );
    assert_eq!(
        report
            .get("present_path_selection_reason")
            .and_then(serde_json::Value::as_str),
        Some("default_direct_visible_surface_with_separate_readback")
    );
    assert_eq!(
        report
            .get("present_path_hooks_present")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("present_path_surface_copy_to_present_supported")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("present_path_readback_enabled")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("last_render_target_kind")
            .and_then(serde_json::Value::as_str),
        Some("visible-surface-direct")
    );
    assert_eq!(
        report
            .pointer("/product_path_ux_timing/status")
            .and_then(serde_json::Value::as_str),
        Some("pass")
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
        Some(9.0)
    );
    assert_eq!(
        report
            .get("budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}


#[test]
fn preview_perf_render_hook_summary_proves_renderer_cpu_budget_when_post_input_split_absent() {
    let mut report = json!({
        "preview_frame_ms_p95": 4.0,
        "operator_host_wheel_input": true,
        "app_owned_window_input": true,
        "native_input_adapter": {
            "installed": true,
            "real_os_events_observed": true,
            "synthetic_input_probe": true,
            "mouse_scroll_event_count": 30,
            "scroll_delta_x": 9600.0,
            "scroll_delta_y": 19200.0,
            "mouse_last_window_protocol_id": 7
        },
        "preview_perf_stats": {
            "render_loop_mode": "demand_driven",
            "frame_pacing": {
                "state": "idle"
            },
            "input_to_present_ms_p50_p95_p99_max": {
                "sample_count": 30,
                "p50": 0.5,
                "p95": 2.8,
                "p99": 3.2,
                "max": 3.6
            },
            "render_hook_ms_p50_p95_p99_max": {
                "sample_count": 37,
                "p50": 0.2,
                "p95": 1.4,
                "p99": 3.0,
                "max": 23.0
            },
            "present_path_ms_p50_p95_p99_max": {
                "sample_count": 37,
                "p50": 0.17,
                "p95": 0.5,
                "p99": 0.6,
                "max": 0.7
            }
        },
        "preview_surface_proof": preview_surface_with_metrics(json!({
            "adapter_is_software": false
        }), json!({
                "upload_bytes": 0,
                "draw_calls": 1,
                "queue_write_count": 0
        }))
    });

    add_native_scroll_model_evidence(&mut report, "cells", false);

    assert_eq!(
        report
            .get("renderer_frame_budget_proven")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("renderer_frame_budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("renderer_cpu_frame_ms_p95")
            .and_then(serde_json::Value::as_f64),
        Some(1.4)
    );
    assert_eq!(
        report
            .get("renderer_cpu_frame_timing_source")
            .and_then(serde_json::Value::as_str),
        Some("post_input_frame_timing.command_record_ms_p95+encoder_finish_ms_p95")
    );
}


