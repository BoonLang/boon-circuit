// Included by `../tests.rs`; kept in the parent test module for private verifier-helper access.

#[test]
fn present_floor_verifier_identity_ignores_inner_probe_arg() {
    let public_args =
        required_xtask_refresh_argv("verify-native-gpu-present-floor", &[], Path::new("r.json"));
    let inner_args = vec![
        "verify-native-gpu-present-floor".to_owned(),
        "--inner-app-window".to_owned(),
        "--report".to_owned(),
        "r.json".to_owned(),
    ];
    assert_eq!(
        verifier_identity_for_command_args("verify-native-gpu-present-floor", "proof", &inner_args),
        verifier_identity_for_command_args(
            "verify-native-gpu-present-floor",
            "proof",
            &public_args
        )
    );
}


#[test]
fn bytes_verifier_identity_accepts_boon_cli_run_plan_args() {
    let args = vec![
        "target/debug/boon_cli".to_owned(),
        "run-plan".to_owned(),
        "examples/bytes_initial.bn".to_owned(),
        "--report".to_owned(),
        "ignored-report-path.json".to_owned(),
    ];
    let identity = verifier_identity_for_command_args("run-plan", "proof", &args).unwrap();
    assert_eq!(
        identity
            .get("contract_version")
            .and_then(serde_json::Value::as_str),
        Some(BYTES_MACHINE_PLAN_VERIFIER_CONTRACT_VERSION)
    );
    assert!(
        !identity
            .get("canonical_args")
            .and_then(serde_json::Value::as_array)
            .unwrap()
            .iter()
            .any(|arg| arg.as_str() == Some("--report"))
    );
    let report = json!({
        "command": "run-plan",
        "command_argv": args,
        "measurement_mode": "proof",
        "verifier_identity": identity
    });
    assert!(report_verifier_identity_matches_report(&report));
}


#[test]
fn source_replay_worktree_fingerprint_scope_tracks_execution_inputs_not_plan_ledgers() {
    assert_eq!(
        worktree_fingerprint_scope_for_command("run-plan-scenario-events"),
        boon_runtime::PLAN_EXECUTOR_SOURCE_REPLAY_WORKTREE_FINGERPRINT_SCOPE
    );
    let paths = worktree_fingerprint_scope_paths(
        boon_runtime::PLAN_EXECUTOR_SOURCE_REPLAY_WORKTREE_FINGERPRINT_SCOPE,
    );
    assert!(paths.contains(&"crates/boon_runtime"));
    assert!(paths.contains(&"crates/boon_plan_executor"));
    assert!(paths.contains(&"crates/boon_compiler"));
    assert!(!paths.contains(&"crates/boon_native_gpu"));
    assert!(!paths.contains(&"docs/plans/GOAL_PROMPT.md"));
    assert!(!paths.contains(&"docs/architecture/NATIVE_GPU_PIPELINE.md"));
}


#[test]
fn report_worktree_freshness_prefers_matching_native_scoped_fingerprint() {
    let current_scoped = worktree_fingerprint_for_scope(NATIVE_GPU_WORKTREE_FINGERPRINT_SCOPE);
    let mut fingerprints = serde_json::Map::new();
    fingerprints.insert(
        NATIVE_GPU_WORKTREE_FINGERPRINT_SCOPE.to_owned(),
        json!(current_scoped.clone()),
    );
    let report = json!({
        "worktree_fingerprint": "stale-full-worktree",
        "worktree_fingerprints": fingerprints
    });
    let scoped = report_worktree_freshness(&report, NATIVE_GPU_WORKTREE_FINGERPRINT_SCOPE);
    assert!(scoped.fresh);
    assert_eq!(scoped.basis, "scoped");
    assert_eq!(scoped.scope, NATIVE_GPU_WORKTREE_FINGERPRINT_SCOPE);
    assert_eq!(
        scoped.report_fingerprint.as_deref(),
        Some(current_scoped.as_str())
    );

    let missing_scoped_report = json!({
        "worktree_fingerprint": "stale-full-worktree"
    });
    let missing_scoped = report_worktree_freshness(
        &missing_scoped_report,
        NATIVE_GPU_WORKTREE_FINGERPRINT_SCOPE,
    );
    assert!(!missing_scoped.fresh);
    assert_eq!(missing_scoped.basis, "missing-scoped");
    assert_eq!(missing_scoped.scope, NATIVE_GPU_WORKTREE_FINGERPRINT_SCOPE);
}


#[test]
fn report_worktree_freshness_prefers_matching_source_replay_scoped_fingerprint() {
    let scope = boon_runtime::PLAN_EXECUTOR_SOURCE_REPLAY_WORKTREE_FINGERPRINT_SCOPE;
    let current_scoped = worktree_fingerprint_for_scope(scope);
    let mut fingerprints = serde_json::Map::new();
    fingerprints.insert(scope.to_owned(), json!(current_scoped.clone()));
    let report = json!({
        "worktree_fingerprint": "stale-full-worktree",
        "worktree_fingerprints": fingerprints
    });
    let scoped = report_worktree_freshness(&report, scope);
    assert!(scoped.fresh);
    assert_eq!(scoped.basis, "scoped");
    assert_eq!(scoped.scope, scope);
    assert_eq!(
        scoped.report_fingerprint.as_deref(),
        Some(current_scoped.as_str())
    );
}


#[test]
fn native_gpu_integrity_accepts_current_scoped_fingerprint_with_stale_full_fingerprint() {
    let current_scoped = worktree_fingerprint_for_scope(NATIVE_GPU_WORKTREE_FINGERPRINT_SCOPE);
    let mut fingerprints = serde_json::Map::new();
    fingerprints.insert(
        NATIVE_GPU_WORKTREE_FINGERPRINT_SCOPE.to_owned(),
        json!(current_scoped),
    );
    let report = json!({
        "status": "pass",
        "command": "verify-native-gpu-preview-e2e",
        "native_gpu_contract": true,
        "generated_at_utc": current_unix_seconds().to_string(),
        "git_commit": git_commit(),
        "worktree_fingerprint": "stale-full-worktree",
        "worktree_fingerprints": fingerprints,
        "binary_hash": current_binary_hash()
    });
    let reasons = native_gpu_report_integrity_reasons(&report, false, true);
    assert!(
        reasons.is_empty(),
        "current scoped native fingerprint should satisfy integrity: {reasons:?}"
    );

    let mut stale_scoped = report.clone();
    stale_scoped["worktree_fingerprints"][NATIVE_GPU_WORKTREE_FINGERPRINT_SCOPE] =
        json!("stale-scoped-worktree");
    let stale_reasons = native_gpu_report_integrity_reasons(&stale_scoped, false, true);
    assert!(
        stale_reasons
            .iter()
            .any(|reason| reason.contains("worktree_fingerprint is stale")),
        "stale scoped native fingerprint must fail integrity: {stale_reasons:?}"
    );
}


#[test]
fn native_real_window_input_method_does_not_use_operator_host_token() {
    let report = json!({
        "status": "pass",
        "native_gpu_contract": true,
        "generated_at_utc": current_unix_seconds().to_string(),
        "git_commit": git_commit(),
        "worktree_fingerprint": worktree_fingerprint(),
        "binary_hash": current_binary_hash(),
        "real_os_input": true,
        "operator_host_input": false,
        "input_injection_method": "weston_test_control_real_wayland_pointer_move_settle_then_button_only_no_preview_ipc_fallback"
    });
    let reasons = native_gpu_report_integrity_reasons(&report, false, true);
    assert!(
        !reasons
            .iter()
            .any(|reason| reason == "operator host input cannot claim real_os_input=true"),
        "real-window driver evidence must not be classified as operator-host input: {reasons:?}"
    );

    let mut bad_report = report;
    bad_report["input_injection_method"] = json!(
        "weston_test_control_real_wayland_pointer_move_settle_then_button_only_no_operator_host_fallback"
    );
    let bad_reasons = native_gpu_report_integrity_reasons(&bad_report, false, true);
    assert!(
        bad_reasons
            .iter()
            .any(|reason| reason == "operator host input cannot claim real_os_input=true"),
        "operator-host token should still be rejected: {bad_reasons:?}"
    );
}
