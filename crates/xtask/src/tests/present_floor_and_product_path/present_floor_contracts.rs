// Included by `../tests.rs`; kept in the parent test module for private verifier-helper access.

#[test]
fn present_floor_default_path_is_focus_safe_product_hardware_only() {
    assert!(present_floor_default_focus_safe_hardware_requested(&[
        "verify-native-gpu-present-floor".to_owned()
    ]));
    assert!(!present_floor_default_focus_safe_hardware_requested(&[
        "verify-native-gpu-present-floor".to_owned(),
        "--unsupported-mode".to_owned()
    ]));
    assert!(!present_floor_default_focus_safe_hardware_requested(&[
        "verify-native-gpu-present-floor".to_owned(),
        "--surface".to_owned(),
        "raw-clear".to_owned()
    ]));
}


#[test]
fn present_floor_focus_safe_hardware_request_uses_product_surface_path() {
    assert!(present_floor_focus_safe_hardware_requested(&[
        "verify-native-gpu-present-floor".to_owned(),
        "--surface".to_owned(),
        "product-preview".to_owned(),
        "--hardware".to_owned(),
        "--focus-safe".to_owned()
    ]));
    assert!(!present_floor_focus_safe_hardware_requested(&[
        "verify-native-gpu-present-floor".to_owned(),
        "--surface".to_owned(),
        "product-preview".to_owned(),
        "--hardware".to_owned()
    ]));
    assert!(!present_floor_focus_safe_hardware_requested(&[
        "verify-native-gpu-present-floor".to_owned(),
        "--surface".to_owned(),
        "raw-clear".to_owned(),
        "--hardware".to_owned(),
        "--focus-safe".to_owned()
    ]));
}


#[test]
fn present_floor_label_contract_accepts_counters_only_product_report() {
    let report = present_floor_contract_report(12.0);
    assert!(
        native_gpu_label_contract_blockers("present-floor", &report).is_empty(),
        "{:?}",
        native_gpu_label_contract_blockers("present-floor", &report)
    );
}


#[test]
fn present_floor_label_contract_accepts_product_preview_surface_report() {
    let mut report = present_floor_contract_report(12.0);
    report["surface_class"] = json!("product-preview-app-window-surface");
    report["focus_safe"] = json!(true);
    report["hardware_requested"] = json!(true);
    assert!(
        native_gpu_label_contract_blockers("present-floor", &report).is_empty(),
        "{:?}",
        native_gpu_label_contract_blockers("present-floor", &report)
    );
}


#[test]
fn present_floor_label_contract_rejects_raw_current_window_report() {
    let mut report = present_floor_contract_report(12.0);
    report["surface_class"] = json!("raw-app-window-clear-only-preview-surface");
    report["focus_safe"] = json!(false);
    report["hardware_requested"] = json!(false);
    let blockers = native_gpu_label_contract_blockers("present-floor", &report);
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("focus_safe")),
        "present-floor contract must reject non-focus-safe reports: {blockers:?}"
    );
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("product-preview present-floor surface")),
        "present-floor contract must reject raw current-window surface reports: {blockers:?}"
    );
}


#[test]
fn present_floor_label_contract_accepts_bounded_max_outlier() {
    let mut report = present_floor_contract_report(12.0);
    report["presented_frame_ms_p50_p95_p99_max"]["max"] = json!(52.7);
    report["presented_frame_ms_bounded_outlier_count"] = json!(1);
    report["presented_frame_ms_bounded_outlier_policy_pass"] = json!(true);
    assert!(
        native_gpu_label_contract_blockers("present-floor", &report).is_empty(),
        "{:?}",
        native_gpu_label_contract_blockers("present-floor", &report)
    );
}


#[test]
fn present_floor_label_contract_rejects_unbounded_max_outlier() {
    let mut report = present_floor_contract_report(12.0);
    report["presented_frame_ms_p50_p95_p99_max"]["max"] = json!(80.0);
    report["presented_frame_ms_bounded_outlier_count"] = json!(2);
    report["presented_frame_ms_bounded_outlier_policy_pass"] = json!(false);
    let blockers = native_gpu_label_contract_blockers("present-floor", &report);
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("bounded outlier policy")),
        "present-floor contract must reject unbounded max outliers: {blockers:?}"
    );
}


#[test]
fn present_floor_label_contract_rejects_hot_path_readback() {
    let mut report = present_floor_contract_report(12.0);
    report["readback_in_hot_path"] = json!(true);
    report["proof_readback_in_hot_path"] = json!(true);
    report["hot_path_proof_readback_count"] = json!(1);
    let blockers = native_gpu_label_contract_blockers("present-floor", &report);
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("readback_in_hot_path")),
        "present-floor contract must reject readback in the product path: {blockers:?}"
    );
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("hot_path_proof_readback_count")),
        "present-floor contract must reject proof readback work in the product path: {blockers:?}"
    );
}


#[test]
fn native_ux_integrity_rejects_pre_present_proof_coupling() {
    let report = json!({
        "command": "verify-native-gpu-preview-e2e",
        "product_proof_built_pre_present": true,
        "pre_present_proof_request_count": 2,
        "last_product_render_frame": {
            "proof_json_built_pre_present": true,
            "render_hook_proof_built_pre_present": true
        },
        "post_present_proof_requests": [
            {
                "kind": "visible_bound_text",
                "built_pre_present": true
            }
        ]
    });
    let mut reasons = Vec::new();
    collect_native_gpu_ux_product_path_reasons(&report, &mut reasons);
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("product_proof_built_pre_present=true")),
        "native UX integrity must reject pre-present product proof coupling: {reasons:?}"
    );
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("pre_present_proof_request_count")),
        "native UX integrity must reject pre-present proof counters: {reasons:?}"
    );
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("built_pre_present=true")),
        "native UX integrity must reject pre-present proof request rows: {reasons:?}"
    );
}


#[test]
fn present_floor_label_contract_rejects_observed_input() {
    let mut report = present_floor_contract_report(12.0);
    report["observed_real_os_input"] = json!(true);
    report["observed_input_event_wake_count"] = json!(1);
    let blockers = native_gpu_label_contract_blockers("present-floor", &report);
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("observed_real_os_input")),
        "present-floor contract must reject observed input: {blockers:?}"
    );
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("observed_input_event_wake_count")),
        "present-floor contract must reject input wake events: {blockers:?}"
    );
}


#[test]
fn present_floor_label_contract_rejects_over_budget_p95() {
    let report = present_floor_contract_report(18.0);
    let blockers = native_gpu_label_contract_blockers("present-floor", &report);
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("presented_frame_ms.p95=18")),
        "present-floor contract must reject over-budget p95: {blockers:?}"
    );
}


