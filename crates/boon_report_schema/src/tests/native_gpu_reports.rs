// Included by `../tests.rs`; kept in the parent test module for private schema helper access.

#[test]
fn native_gpu_schema_accepts_structured_frame_evidence() {
    assert!(schema_accepts(
        native_gpu_report_with_frame_evidence(),
        "native-frame-evidence-valid"
    ));
}

macro_rules! native_gpu_rejects {
    ($name:ident, $label:literal, |$report:ident| $mutate:block) => {

        #[test]
        fn $name() {
            let mut $report = native_gpu_report_with_frame_evidence();
            $mutate
            assert!(!schema_accepts($report, $label));
        }
    };
}

native_gpu_rejects!(
    native_gpu_schema_rejects_missing_frame_evidence_key,
    "native-frame-evidence-missing-key",
    |report| {
        report["last_interactive_readback_artifact"]
            .as_object_mut()
            .unwrap()
            .remove("frame_evidence_key");
    }
);


#[test]
fn refresh_queue_schema_rejects_pass_with_failed_child() {
    let mut report = refresh_queue_report();
    report["results"][0]["status"] = json!("fail");
    report["pass_count"] = json!(0);
    report["fail_count"] = json!(1);
    assert!(!schema_accepts(report, "refresh-queue-failed-child"));
}

native_gpu_rejects!(
    native_gpu_schema_rejects_app_owned_present_target_proof_without_frame_evidence,
    "native-app-owned-target-frame-evidence-missing-key",
    |report| {
        report["last_interactive_readback_artifact"]["capture_method"] =
            json!("wgpu-app-owned-present-target-copy-to-visible-surface-readback");
        report["last_interactive_readback_artifact"]
            .as_object_mut()
            .unwrap()
            .remove("frame_evidence_key");
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_mismatched_frame_evidence_key,
    "native-frame-evidence-mismatched-key",
    |report| {
        report["last_interactive_readback_artifact"]["frame_evidence_key"]["content_revision"] =
            json!(6);
    }
);


#[test]
fn native_gpu_schema_rejects_mismatched_top_level_layer_revisions() {
    let mut report = native_gpu_report_with_frame_evidence();
    report["last_render_layout_revision"] = json!(8);
    assert!(!schema_accepts(
        report,
        "native-frame-evidence-layout-mismatch"
    ));

    let mut report = native_gpu_report_with_frame_evidence();
    report["last_render_scene_revision"] = json!(9);
    assert!(!schema_accepts(
        report,
        "native-frame-evidence-render-scene-mismatch"
    ));
}

native_gpu_rejects!(
    native_gpu_schema_rejects_hash_only_readback_proof,
    "native-frame-evidence-hash-only",
    |report| {
        report["last_interactive_readback_artifact"]
            .as_object_mut()
            .unwrap()
            .remove("frame_evidence_key");
        report["last_interactive_readback_artifact"]["sha256"] = json!("hash-only-proof");
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_lagged_ux_proof,
    "native-lagged-ux-proof",
    |report| {
        report["rendered_frame_count"] = json!(4);
        report["proof_lag_frames"] = json!(1);
        report["frame_evidence_key"]["frame_seq"] = json!(4);
        report["frame_evidence_key"]["present_id"] = json!(4);
        report["preview_perf_stats"]["frame_seq"] = json!(4);
        report["preview_perf_stats"]["frame_evidence_key"]["frame_seq"] = json!(4);
        report["preview_perf_stats"]["frame_evidence_key"]["present_id"] = json!(4);
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_pending_ux_readback,
    "native-pending-ux-readback",
    |report| {
        report["last_interactive_surface_readback_pending"] = json!(true);
        report["last_interactive_readback_artifact"]["readback_poll_status"] = json!("pending");
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_same_frame_proof_identity_mismatch,
    "native-same-frame-proof-identity-mismatch",
    |report| {
        report["last_interactive_readback_artifact"]["frame_evidence_key"]["proof_request_id"] =
            json!(99);
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_preview_perf_missing_percentile_summary,
    "native-preview-perf-missing-summary",
    |report| {
        report["preview_perf_stats"]
            .as_object_mut()
            .unwrap()
            .remove("input_to_present_ms_p50_p95_p99_max");
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_continuous_probe_ux_report,
    "native-frame-evidence-probe-ux",
    |report| {
        report["render_loop_mode"] = json!("continuous_probe");
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_proof_required_visible_update,
    "native-proof-required-visible-update",
    |report| {
        report["proof_required_for_visible_update"] = json!(true);
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_below_host_input_injection,
    "native-below-host-input",
    |report| {
        report["input_injection_method"] = json!("direct-runtime-route-below-host-event");
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_preview_ipc_blocking,
    "native-preview-ipc-blocking",
    |report| {
        report["preview_blocked_on_ipc_count"] = json!(1);
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_passive_scroll_runtime_dispatch,
    "native-passive-scroll-runtime-dispatch",
    |report| {
        report["command"] = json!("verify-native-gpu-scroll-speed");
        report["runtime_dispatch_on_passive_scroll"] = json!(true);
        report["runtime_dispatch_count_for_passive_scroll"] = json!(1);
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_dev_perf_hot_path_query,
    "native-dev-perf-hot-path-query",
    |report| {
        report["preview_perf_hot_path_query_count"] = json!(1);
        report["dev_perf_row_queries_ipc_from_render_hook"] = json!(true);
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_incomplete_dev_perf_hot_path_guards,
    "native-dev-perf-hot-path-guards-incomplete",
    |report| {
        report["dev_hot_path_counters"] = json!({
            "preview_perf_hot_path_query_count": 0
        });
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_render_hook_offscreen_proof_hot_path,
    "native-render-hook-offscreen-proof-hot-path",
    |report| {
        report["command"] = json!("verify-native-gpu-scroll-speed");
        report["preview_surface_proof"] = json!({
            "external_render_proof": {
                "status": "pass",
                "render_backend_trait": "boon_native_gpu::render_app_owned_scene_pixels",
                "offscreen_app_owned_scene_readback_skipped": false
            }
        });
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_desktop_screenshot_proof,
    "native-desktop-screenshot-proof",
    |report| {
        report["visual_capture_method"] = json!("desktop-screenshot");
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_nested_private_runtime_dispatch,
    "native-nested-private-dispatch",
    |report| {
        report["native_host_input_route_evidence"] = json!({
            "private_runtime_dispatch_used": true
        });
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_browser_proof_substitution,
    "native-browser-proof-substitution",
    |report| {
        report["browser_render_executed"] = json!(true);
        report["browser_capture_method"] =
            json!("headless-chromium-webgpu-app-owned-copyTextureToBuffer");
    }
);

native_gpu_rejects!(
    native_gpu_schema_rejects_cosmic_toplevel_proof,
    "native-cosmic-toplevel-proof",
    |report| {
        report["cosmic_toplevel_probe"] = json!({
            "status": "pass"
        });
    }
);

fn native_gpu_negative_report() -> JsonValue {
    let ids = native_gpu_product_path_negative_case_ids();
    let mut report = base_report();
    report["command"] = json!("verify-native-gpu-negative");
    report["command_argv"] = json!(["verify-native-gpu-negative"]);
    report["native_gpu_contract"] = json!(true);
    report["negative_case_count"] = json!(ids.len());
    report["required_negative_cases"] = json!(ids);
    report
}


#[test]
fn native_gpu_negative_schema_requires_product_path_cases() {
    assert!(schema_accepts(
        native_gpu_negative_report(),
        "native-negative-product-path-valid"
    ));

    let mut missing = native_gpu_negative_report();
    missing["required_negative_cases"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(!schema_accepts(
        missing,
        "native-negative-product-path-missing"
    ));

    let mut duplicate = native_gpu_negative_report();
    let first = duplicate["required_negative_cases"][0].clone();
    duplicate["required_negative_cases"]
        .as_array_mut()
        .unwrap()
        .push(first);
    duplicate["negative_case_count"] = json!(
        duplicate["required_negative_cases"]
            .as_array()
            .unwrap()
            .len()
    );
    assert!(!schema_accepts(
        duplicate,
        "native-negative-product-path-duplicate"
    ));

    let mut wrong_count = native_gpu_negative_report();
    wrong_count["negative_case_count"] = json!(1);
    assert!(!schema_accepts(
        wrong_count,
        "native-negative-product-path-count"
    ));
}


#[test]
fn present_floor_scoped_verifier_identity_ignores_inner_probe_arg() {
    let args = vec![
        "verify-native-gpu-present-floor".to_owned(),
        "--inner-app-window".to_owned(),
        "--report".to_owned(),
        "target/reports/native-gpu/present-floor.json".to_owned(),
    ];
    let public_args = vec![
        "verify-native-gpu-present-floor".to_owned(),
        "--report".to_owned(),
        "target/reports/native-gpu/present-floor.json".to_owned(),
    ];
    assert_eq!(
        verifier_identity_for_command_args("verify-native-gpu-present-floor", "proof", &args),
        verifier_identity_for_command_args(
            "verify-native-gpu-present-floor",
            "proof",
            &public_args
        )
    );
}


#[test]
fn interaction_mode_rejects_hot_path_proof_and_diagnostic_work() {
    assert!(schema_accepts(interaction_report(), "clean-interaction"));

    let mut png = interaction_report();
    png["hot_path_png_write_count"] = json!(1);
    assert!(!schema_accepts(png, "interaction-png"));

    let mut readback = interaction_report();
    readback["proof_readback_in_hot_path"] = json!(true);
    assert!(!schema_accepts(readback, "interaction-readback"));

    let mut ipc = interaction_report();
    ipc["dev_blocking_ipc_count"] = json!(2);
    assert!(!schema_accepts(ipc, "interaction-ipc"));
}


#[test]
fn interaction_mode_requires_flow_id_and_stage_counters() {
    let mut missing_flow = interaction_report();
    missing_flow
        .as_object_mut()
        .unwrap()
        .remove("interaction_flow_id");
    assert!(!schema_accepts(missing_flow, "interaction-missing-flow"));

    let mut missing_stages = interaction_report();
    missing_stages
        .as_object_mut()
        .unwrap()
        .remove("stage_counters");
    assert!(!schema_accepts(
        missing_stages,
        "interaction-missing-stages"
    ));

    let mut empty_stage = interaction_report();
    empty_stage["stage_counters"]["runtime_turn"]["sample_count"] = json!(0);
    assert!(!schema_accepts(empty_stage, "interaction-empty-stage"));
}
