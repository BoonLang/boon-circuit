#[test]
fn planned_operator_wheel_input_is_not_observed_scroll_speed_evidence() {
    let mut report = json!({
        "preview_frame_ms_p95": 4.0,
        "operator_host_wheel_input": true,
        "operator_host_input_evidence": {
            "host_events": [{"kind": "wheel"}, {"kind": "wheel"}],
            "deltas": {"vertical_px": 720.0, "horizontal_px": 480.0}
        },
        "native_input_adapter": {
            "mouse_scroll_event_count": 0,
            "scroll_delta_x": 0.0,
            "scroll_delta_y": 0.0
        },
        "preview_surface_proof": {
            "adapter_is_software": false
        }
    });

    add_native_scroll_model_evidence(&mut report, "cells", false);

    assert_eq!(
        report
            .get("wheel_input_evidence_source")
            .and_then(serde_json::Value::as_str),
        Some("operator-host-plan")
    );
    assert_eq!(
        report
            .get("selected_wheel_input_observed")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report
            .get("budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
}


#[test]
fn software_adapter_over_budget_does_not_prove_scroll_speed() {
    let mut report = json!({
        "preview_frame_ms_p95": 24.0,
        "speed_timing_window": "post-real-window-input",
        "post_input_frame_timing": {
            "measured_frame_count": 30,
            "surface_acquire_ms_p95": 0.2,
            "command_record_ms_p95": 3.0,
            "encoder_finish_ms_p95": 0.4,
            "queue_submit_ms_p95": 12.0,
            "frame_present_ms_p95": 8.6
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
            "adapter_name": "llvmpipe",
            "adapter_backend": "Vulkan",
            "adapter_device_type": "Cpu",
            "adapter_is_software": true,
            "present_mode": "Immediate",
            "supported_present_modes": ["Immediate", "Fifo"],
            "non_vsync_present_mode_available": true,
            "desired_maximum_frame_latency": 1,
            "surface_format": "Bgra8UnormSrgb"
        }), json!({
                "upload_bytes": 0,
                "draw_calls": 1,
                "queue_write_count": 0
        }))
    });

    add_native_scroll_model_evidence(&mut report, "generic", false);

    assert_eq!(
        report
            .get("software_adapter_wall_clock_budget_exempt")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("real_window_speed_adapter_proven")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report
            .get("real_window_speed_adapter_policy")
            .and_then(serde_json::Value::as_str),
        Some("software-diagnostic-only")
    );
    assert_eq!(
        report
            .get("measured_adapter_name")
            .and_then(serde_json::Value::as_str),
        Some("llvmpipe")
    );
    assert_eq!(
        report
            .get("measured_present_mode")
            .and_then(serde_json::Value::as_str),
        Some("Immediate")
    );
    assert_eq!(
        report
            .get("measured_non_vsync_present_mode_available")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("wall_clock_frame_budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
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
            .pointer("/non_os_scroll_model/frame_budget_model_pass")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("renderer_cpu_frame_ms_p95")
            .and_then(serde_json::Value::as_f64),
        Some(3.4)
    );
    assert_eq!(
        report
            .get("present_blocking_ms_p95")
            .and_then(serde_json::Value::as_f64),
        Some(20.6)
    );
    assert_eq!(
        report
            .get("required_real_window_speed_proven")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report
            .get("budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
}


#[test]
fn software_adapter_under_budget_remains_diagnostic_only() {
    let mut report = json!({
        "preview_frame_ms_p95": 12.0,
        "speed_timing_window": "post-real-window-input",
        "post_input_frame_timing": {
            "measured_frame_count": 30,
            "surface_acquire_ms_p95": 0.2,
            "command_record_ms_p95": 3.0,
            "encoder_finish_ms_p95": 0.4,
            "queue_submit_ms_p95": 4.0,
            "frame_present_ms_p95": 4.0
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
            "adapter_name": "llvmpipe",
            "adapter_device_type": "Cpu",
            "adapter_is_software": true,
            "present_mode": "Immediate",
            "supported_present_modes": ["Immediate", "Fifo"],
            "non_vsync_present_mode_available": true
        }), json!({
                "upload_bytes": 0,
                "draw_calls": 1,
                "queue_write_count": 0
        }))
    });

    add_native_scroll_model_evidence(&mut report, "generic", false);

    assert_eq!(
        report
            .get("wall_clock_frame_budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("real_window_speed_adapter_proven")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report
            .get("required_real_window_speed_proven")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report
            .get("budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
}


#[test]
fn product_path_input_to_present_timing_drives_scroll_budget_when_proven() {
    let mut report = json!({
        "preview_frame_ms_p95": 24.0,
        "speed_timing_window": "post-real-window-input",
        "post_input_frame_timing": {
            "measured_frame_count": 30,
            "surface_acquire_ms_p95": 0.2,
            "command_record_ms_p95": 3.0,
            "encoder_finish_ms_p95": 0.4,
            "queue_submit_ms_p95": 12.0,
            "frame_present_ms_p95": 8.6
        },
        "render_loop_state": {
            "requested_animation_burst_count": 1
        },
        "preview_perf_stats": {
            "render_loop_mode": "demand_driven",
            "frame_pacing": {"state": "requested_animation_burst"},
            "input_to_present_ms_p50_p95_p99_max": {
                "sample_count": 5,
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
            .get("wall_clock_frame_budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report
            .get("ux_frame_budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("scroll_frame_ms_p95")
            .and_then(serde_json::Value::as_f64),
        Some(8.0)
    );
    assert_eq!(
        report
            .get("wheel_to_visible_ms_p95")
            .and_then(serde_json::Value::as_f64),
        Some(8.0)
    );
    assert_eq!(
        report
            .get("required_real_window_speed_proven")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("budget_pass")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}


