#[test]
fn dev_editor_scroll_budget_uses_dev_surface_adapter_flag() {
    let mut report = json!({
        "preview_frame_ms_p95": 12.0,
        "speed_timing_window": "post-real-window-input",
        "post_input_frame_timing": {
            "measured_frame_count": 30
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
            "adapter_is_software": true
        }), json!({
            "upload_bytes": 0,
            "draw_calls": 1,
            "queue_write_count": 0,
            "visible_text_runs": 64,
            "shaped_text_runs": 64,
            "shaped_run_cache_hits": 64,
            "shaped_run_cache_misses": 0
        })),
        "dev_surface_proof": {
            "adapter_name": "discrete-gpu",
            "adapter_backend": "Vulkan",
            "adapter_device_type": "DiscreteGpu",
            "adapter_is_software": false,
            "present_mode": "Mailbox",
            "supported_present_modes": ["Fifo", "Mailbox"],
            "non_vsync_present_mode_available": true,
            "desired_maximum_frame_latency": 1,
            "surface_format": "Bgra8UnormSrgb"
        },
        "line_count": 10000,
        "longest_line_bytes": 2000
    });

    add_native_scroll_model_evidence(&mut report, "dev-code-editor", true);

    assert_eq!(
        report
            .get("software_adapter_wall_clock_budget_exempt")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report
            .get("real_window_speed_adapter_proven")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("measured_adapter_name")
            .and_then(serde_json::Value::as_str),
        Some("discrete-gpu")
    );
    assert_eq!(
        report
            .get("measured_present_mode")
            .and_then(serde_json::Value::as_str),
        Some("Mailbox")
    );
    assert_eq!(
        report
            .get("wall_clock_frame_budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("required_real_window_speed_proven")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}


#[test]
fn axis_specific_real_window_input_overrides_planned_operator_wheel_input() {
    let mut report = json!({
        "preview_frame_ms_p95": 24.0,
        "operator_host_wheel_input": true,
        "operator_host_input_evidence": {
            "host_events": [{"kind": "wheel"}, {"kind": "wheel"}],
            "deltas": {"vertical_px": 720.0, "horizontal_px": 480.0}
        },
        "app_owned_window_input": true,
        "real_window_input": true,
        "native_input_adapter": {
            "mouse_scroll_event_count": 0,
            "scroll_delta_x": 0.0,
            "scroll_delta_y": 0.0
        },
        "preview_surface_proof": {
            "adapter_is_software": false
        },
        "axis_specific_real_window_scroll_observation": {
            "status": "pass",
            "combined_input_adapter": {
                "mouse_scroll_event_count": 4,
                "scroll_delta_x": 480.0,
                "scroll_delta_y": 720.0
            },
            "vertical_observation": {
                "surface_post_input_frame_timing": {
                    "measured_frame_count": 59,
                    "presented_frame_ms_p95": 11.0,
                    "sample_frame_count": 60,
                    "warmup_frame_count": 3
                }
            },
            "horizontal_observation": {
                "surface_post_input_frame_timing": {
                    "measured_frame_count": 59,
                    "presented_frame_ms_p95": 12.0,
                    "sample_frame_count": 60,
                    "warmup_frame_count": 3
                }
            }
        }
    });

    assert!(promote_axis_specific_scroll_timing(&mut report));
    add_native_scroll_model_evidence(&mut report, "cells", false);

    assert_eq!(
        report
            .get("wheel_input_evidence_source")
            .and_then(serde_json::Value::as_str),
        Some("axis-specific-real-window-adapter")
    );
    assert_eq!(
        report
            .get("selected_wheel_input_observed")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("real_window_timing_proven")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("required_real_window_speed_proven")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("operator_vertical_wheel_input")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report
            .get("real_vertical_wheel_input")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .pointer("/materialized_range_before_after/status")
            .and_then(serde_json::Value::as_str),
        Some("real-window-wheel-input")
    );
    assert_eq!(
        report
            .get("budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}
