// Included by `../tests.rs`; kept in the parent test module for private verifier-helper access.

#[test]
fn native_gpu_handoff_requires_present_floor_report() {
    let reports = native_gpu_handoff_required_reports();
    let report = reports
        .iter()
        .find(|report| report.label == "present-floor")
        .expect("native GPU handoff must require the focus-safe present-floor gate");
    assert_eq!(
        report.path,
        PathBuf::from("target/reports/native-gpu/present-floor.json")
    );
    assert_eq!(report.command, "verify-native-gpu-present-floor");
    assert_eq!(report.required_argv, &[]);
}


#[test]
fn native_gpu_handoff_preview_e2e_requires_release_hardware_reports() {
    let reports = native_gpu_handoff_required_reports();
    for label in [
        "preview-e2e-todomvc",
        "preview-e2e-cells",
        "preview-e2e-todo_mvc_physical",
    ] {
        let report = reports
            .iter()
            .find(|report| report.label == label)
            .expect("native GPU handoff must require preview E2E reports");
        assert!(report.required_argv.contains(&("--profile", "release")));
        assert!(
            report
                .required_argv
                .contains(&("--require-hardware-adapter", ""))
        );
    }
}


#[test]
fn native_gpu_handoff_manifest_rejects_preview_source_replay_dependencies() {
    let reports = native_gpu_handoff_required_reports();
    for label in [
        "preview-e2e-todomvc",
        "preview-e2e-cells",
        "preview-e2e-todo_mvc_physical",
    ] {
        let report = reports
            .iter()
            .find(|report| report.label == label)
            .expect("native GPU handoff must include preview E2E report");
        assert!(
            report.upstream_dependencies.is_empty(),
            "{label} must not consume PlanExecutor source replay as native proof"
        );
    }
}


#[test]
fn native_gpu_handoff_manifest_has_unique_bounded_reports_and_docs_source() {
    let reports = native_gpu_handoff_required_reports();
    assert!(reports.len() >= 17);
    let mut labels = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for report in &reports {
        assert!(
            labels.insert(report.label),
            "duplicate label {}",
            report.label
        );
        assert!(
            paths.insert(report.path.clone()),
            "duplicate path {}",
            report.path.display()
        );
        assert!(xtask_command_exists(report.command));
        assert!(report.max_report_bytes > 0);
        let mut upstream_labels = BTreeSet::new();
        for dependency in report.upstream_dependencies {
            assert!(
                upstream_labels.insert(dependency.label),
                "duplicate upstream label {} for native report {}",
                dependency.label,
                report.label
            );
            assert_eq!(dependency.measurement_mode, "proof");
            assert_eq!(dependency.kind, "consumes-native-report");
            assert!(xtask_command_exists(dependency.command));
            assert_eq!(dependency.owner_aggregate, "verify-native-gpu-all");
        }
    }
    let cells = reports
        .iter()
        .find(|report| report.label == "cells-visible-click-e2e-release")
        .unwrap();
    assert!(cells.max_sidecar_bytes > 0);
    let present_floor = reports
        .iter()
        .find(|report| report.label == "present-floor")
        .unwrap();
    assert_eq!(present_floor.max_sidecar_bytes, 0);

    let agents = fs::read_to_string(workspace_relative_path("AGENTS.md")).unwrap();
    let architecture = fs::read_to_string(workspace_relative_path(
        "docs/architecture/NATIVE_GPU_PIPELINE.md",
    ))
    .unwrap();
    assert!(agents.contains(NATIVE_GPU_HANDOFF_MANIFEST_PATH));
    assert!(architecture.contains(NATIVE_GPU_HANDOFF_MANIFEST_PATH));
    assert!(!agents.contains("cargo xtask verify-platform-contract --report"));
    assert!(!architecture.contains("cargo xtask verify-platform-contract --report"));
}


#[test]
fn blocker_audit_treats_manifest_commands_as_manifest_owned() {
    assert!(report_is_blocker_audit(&json!({
        "command": "verify-native-gpu-preview-e2e"
    })));
    assert!(report_is_blocker_audit(&json!({
        "command": "verify-native-cells-visible-click-e2e"
    })));
    assert!(report_is_blocker_audit(&json!({
        "command": "verify-native-gpu-novywave-visual"
    })));
    assert!(!report_is_blocker_audit(&json!({
        "command": "verify-obsolete-native-proof"
    })));
}


#[test]
fn schema_summary_large_native_gpu_skip_excludes_handoff_and_roles() {
    let mut handoff_paths = BTreeSet::new();
    handoff_paths.insert(PathBuf::from(
        "target/reports/native-gpu/cells-visible-click-e2e-release.json",
    ));
    assert!(schema_summary_native_gpu_large_non_handoff_report(
        Path::new("target/reports/native-gpu/cells-visible-click-e2e-experiment.json"),
        20 * 1024 * 1024,
        &handoff_paths
    ));
    assert!(!schema_summary_native_gpu_large_non_handoff_report(
        Path::new("target/reports/native-gpu/cells-visible-click-e2e-release.json"),
        20 * 1024 * 1024,
        &handoff_paths
    ));
    assert!(!schema_summary_native_gpu_large_non_handoff_report(
        Path::new("target/reports/native-gpu/roles/preview-loop.json"),
        20 * 1024 * 1024,
        &handoff_paths
    ));
    assert!(!schema_summary_native_gpu_large_non_handoff_report(
        Path::new("target/reports/native-gpu/cells-visible-click-e2e-experiment.json"),
        1024,
        &handoff_paths
    ));
}


#[test]
fn refresh_queue_selection_expands_upstream_dependencies() {
    let aggregate = json!({
        "refresh_commands": [
            {
                "label": "preview-e2e-cells",
                "path": "target/reports/native-gpu/preview-e2e-cells.json",
                "reason": "identity-freshness",
                "argv": [current_binary_path(), "verify-native-gpu-preview-e2e"]
            },
            {
                "label": "cells-visible-click-e2e-release",
                "path": "target/reports/native-gpu/cells-visible-click-e2e-release.json",
                "reason": "upstream-schema-or-identity-freshness",
                "required_by": "preview-e2e-cells",
                "owner_aggregate": "verify-native-gpu-all",
                "owner_aggregate_report_path": "target/reports/native-gpu-all.json",
                "argv": [current_binary_path(), "verify-native-cells-visible-click-e2e"]
            }
        ]
    });
    let labels = ["preview-e2e-cells".to_owned()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let plan = plan_refresh_queue_entries(&aggregate, &labels, 1);
    let selected_labels = selected_labels_for_refresh_entries(&plan.selected);
    assert_eq!(plan.dependency_expansion_count, 1);
    assert_eq!(plan.selected.len(), 2);
    assert!(selected_labels.contains("preview-e2e-cells"));
    assert!(selected_labels.contains("cells-visible-click-e2e-release"));
    assert_eq!(
        plan.selected[0]
            .get("label")
            .and_then(serde_json::Value::as_str),
        Some("cells-visible-click-e2e-release")
    );
    assert_eq!(
        plan.selected[0]
            .get("refresh_phase")
            .and_then(serde_json::Value::as_str),
        Some("upstream-dependency")
    );
}


#[test]
fn refresh_queue_limit_prioritizes_upstream_dependencies() {
    let aggregate = json!({
        "refresh_commands": [
            {
                "label": "platform-contract",
                "path": "target/reports/native-gpu/platform-contract.json",
                "reason": "identity-freshness",
                "argv": [current_binary_path(), "verify-platform-contract"]
            },
            {
                "label": "cells-visible-click-e2e-release",
                "path": "target/reports/native-gpu/cells-visible-click-e2e-release.json",
                "reason": "upstream-schema-or-identity-freshness",
                "required_by": "preview-e2e-cells",
                "owner_aggregate": "verify-native-gpu-all",
                "owner_aggregate_report_path": "target/reports/native-gpu-all.json",
                "argv": [current_binary_path(), "verify-native-cells-visible-click-e2e"]
            }
        ]
    });
    let plan = plan_refresh_queue_entries(&aggregate, &BTreeSet::new(), 1);
    assert_eq!(plan.selected.len(), 1);
    assert_eq!(
        plan.selected[0]
            .get("label")
            .and_then(serde_json::Value::as_str),
        Some("cells-visible-click-e2e-release")
    );
    assert_eq!(
        plan.selection_mode,
        "dependency-order-full-queue".to_owned()
    );
}


#[test]
fn native_gpu_worktree_fingerprint_scope_tracks_product_inputs_not_plan_ledgers() {
    let paths = worktree_fingerprint_scope_paths(NATIVE_GPU_WORKTREE_FINGERPRINT_SCOPE);
    assert!(paths.contains(&"crates"));
    assert!(paths.contains(&"examples"));
    assert!(paths.contains(&"budgets/native-gpu.toml"));
    assert!(paths.contains(&"docs/architecture/NATIVE_GPU_PIPELINE.md"));
    assert!(paths.contains(&"docs/architecture/native_gpu_handoff_manifest.json"));
    assert!(!paths.contains(&"docs/plans/GOAL_PROMPT.md"));
}


#[test]
fn native_gpu_aggregate_fast_paths_stale_identity_before_child_contract_validation() {
    const REQUIRED_ARGV: &[(&str, &str)] = &[("--example", "cells")];
    let dir = std::env::temp_dir().join(format!(
        "boon-xtask-native-stale-fast-path-{}-{}",
        std::process::id(),
        monotonic_now_ns().unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let child = dir.join("stale-child.json");
    let aggregate = dir.join("aggregate.json");
    write_json(
        &child,
        &json!({
            "status": "fail",
            "report_version": 1,
            "generated_at_utc": "1",
            "command": "verify-native-gpu-preview-e2e",
            "command_argv": [
                current_binary_path(),
                "verify-native-gpu-preview-e2e",
                "--example",
                "cells",
                "--report",
                child.display().to_string()
            ],
            "measurement_mode": "proof",
            "exit_status": 1,
            "git_commit": "stale-git",
            "worktree_fingerprint": "stale-worktree",
            "binary_hash": "stale-binary",
            "binary_path": current_binary_path(),
            "source_hash": "n/a",
            "scenario_hash": "n/a",
            "program_hash": "n/a",
            "budget_hash": "n/a",
            "graph_node_count": 0,
            "per_step_pass_fail": [],
            "artifact_sha256s": [],
            "native_gpu_contract": false,
            "blockers": ["this would be a product-contract blocker if stale validation were not fast-pathed"]
        }),
    )
    .unwrap();
    let result = verify_native_gpu_report_bundle(
        &[
            "verify-native-gpu-all".to_owned(),
            "--check-existing".to_owned(),
            "--report".to_owned(),
            aggregate.display().to_string(),
        ],
        "verify-native-gpu-all",
        vec![NativeGpuRequiredReport {
            label: "stale-child",
            path: child.clone(),
            command: "verify-native-gpu-preview-e2e",
            required_argv: REQUIRED_ARGV,
            upstream_dependencies: &[],
            requires_native_gpu_contract: true,
            max_report_bytes: u64::MAX,
            max_sidecar_bytes: u64::MAX,
        }],
        "test-native-stale-fast-path",
    );
    assert!(result.is_err());
    let report = read_json(&aggregate).unwrap();
    assert_eq!(
        report
            .get("refresh_debt_child_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        report
            .get("product_contract_child_count")
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );
    assert_eq!(
        report
            .get("identity_fast_refresh_child_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        report
            .pointer("/child_reports/0/control_plane_validation_mode")
            .and_then(serde_json::Value::as_str),
        Some("identity-fast-refresh")
    );
    assert_eq!(
        report
            .pointer("/child_reports/0/schema_validation_skipped")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .pointer("/refresh_commands/0/reason")
            .and_then(serde_json::Value::as_str),
        Some("identity-freshness-fast-path")
    );
    std::fs::remove_dir_all(dir).unwrap();
}


#[test]
fn native_gpu_aggregate_does_not_refresh_scoped_verifier_identity_with_stale_binary_hash() {
    const REQUIRED_ARGV: &[(&str, &str)] = &[("--example", "cells")];
    let dir = std::env::temp_dir().join(format!(
        "boon-xtask-native-scoped-identity-{}-{}",
        std::process::id(),
        monotonic_now_ns().unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let child = dir.join("scoped-child.json");
    let aggregate = dir.join("aggregate.json");
    let command_argv =
        required_xtask_refresh_argv("verify-native-gpu-preview-e2e", REQUIRED_ARGV, &child);
    let mut child_report = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-native-gpu-preview-e2e",
        "command_argv": command_argv,
        "measurement_mode": "proof",
        "exit_status": 0,
        "git_commit": git_commit(),
        "binary_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "binary_path": current_binary_path(),
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "budget_hash": file_hash("budgets/native-gpu.toml"),
        "graph_node_count": 0,
        "per_step_pass_fail": [{"id": "shape", "pass": true}],
        "artifact_sha256s": [],
        "native_gpu_contract": true
    });
    if let Some(object) = child_report.as_object_mut() {
        insert_worktree_fingerprint_fields(object, NATIVE_GPU_WORKTREE_FINGERPRINT_SCOPE);
        object.insert(
            "verifier_identity".to_owned(),
            verifier_identity_for_command_args(
                "verify-native-gpu-preview-e2e",
                "proof",
                object
                    .get("command_argv")
                    .and_then(serde_json::Value::as_array)
                    .unwrap()
                    .iter()
                    .map(|arg| arg.as_str().unwrap().to_owned())
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
            .unwrap(),
        );
    }
    write_json(&child, &child_report).unwrap();

    let result = verify_native_gpu_report_bundle(
        &[
            "verify-native-gpu-all".to_owned(),
            "--check-existing".to_owned(),
            "--report".to_owned(),
            aggregate.display().to_string(),
        ],
        "verify-native-gpu-all",
        vec![NativeGpuRequiredReport {
            label: "scoped-child",
            path: child.clone(),
            command: "verify-native-gpu-preview-e2e",
            required_argv: REQUIRED_ARGV,
            upstream_dependencies: &[],
            requires_native_gpu_contract: true,
            max_report_bytes: u64::MAX,
            max_sidecar_bytes: u64::MAX,
        }],
        "test-native-scoped-identity",
    );
    assert!(
        result
            .as_ref()
            .err()
            .is_some_and(|error| error.to_string().contains("aggregate_scope")),
        "one-child unit fixture should fail only the canonical handoff aggregate shape: {result:?}"
    );
    let report = read_json(&aggregate).unwrap();
    assert_eq!(
        report
            .get("refresh_debt_child_count")
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );
    assert_eq!(
        report
            .get("true_blocker_child_count")
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );
    assert_eq!(
        report
            .get("identity_fast_refresh_child_count")
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );
    assert_eq!(
        report
            .pointer("/child_reports/0/binary_hash_matches")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report
            .pointer("/child_reports/0/verifier_identity_fresh")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .pointer("/child_reports/0/binary_freshness_basis")
            .and_then(serde_json::Value::as_str),
        Some("scoped-verifier-identity")
    );
    std::fs::remove_dir_all(dir).unwrap();
}


#[test]
fn native_gpu_aggregate_refreshes_stale_scoped_verifier_identity() {
    const REQUIRED_ARGV: &[(&str, &str)] = &[("--example", "cells")];
    let dir = std::env::temp_dir().join(format!(
        "boon-xtask-native-stale-scoped-identity-{}-{}",
        std::process::id(),
        monotonic_now_ns().unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let child = dir.join("stale-scoped-child.json");
    let aggregate = dir.join("aggregate.json");
    let command_argv =
        required_xtask_refresh_argv("verify-native-gpu-preview-e2e", REQUIRED_ARGV, &child);
    let stale_identity_argv = required_xtask_refresh_argv(
        "verify-native-gpu-preview-e2e",
        &[("--example", "todomvc")],
        &child,
    );
    let mut child_report = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-native-gpu-preview-e2e",
        "command_argv": command_argv,
        "measurement_mode": "proof",
        "exit_status": 0,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "binary_path": current_binary_path(),
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "budget_hash": file_hash("budgets/native-gpu.toml"),
        "graph_node_count": 0,
        "per_step_pass_fail": [{"id": "shape", "pass": true}],
        "artifact_sha256s": [],
        "native_gpu_contract": true
    });
    if let Some(object) = child_report.as_object_mut() {
        insert_worktree_fingerprint_fields(object, NATIVE_GPU_WORKTREE_FINGERPRINT_SCOPE);
        object.insert(
            "verifier_identity".to_owned(),
            verifier_identity_for_command_args(
                "verify-native-gpu-preview-e2e",
                "proof",
                &stale_identity_argv,
            )
            .unwrap(),
        );
    }
    write_json(&child, &child_report).unwrap();

    let result = verify_native_gpu_report_bundle(
        &[
            "verify-native-gpu-all".to_owned(),
            "--check-existing".to_owned(),
            "--report".to_owned(),
            aggregate.display().to_string(),
        ],
        "verify-native-gpu-all",
        vec![NativeGpuRequiredReport {
            label: "stale-scoped-child",
            path: child.clone(),
            command: "verify-native-gpu-preview-e2e",
            required_argv: REQUIRED_ARGV,
            upstream_dependencies: &[],
            requires_native_gpu_contract: true,
            max_report_bytes: u64::MAX,
            max_sidecar_bytes: u64::MAX,
        }],
        "test-native-stale-scoped-identity",
    );
    assert!(result.is_err());
    let report = read_json(&aggregate).unwrap();
    assert_eq!(
        report
            .get("refresh_debt_child_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        report
            .get("identity_fast_refresh_child_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        report
            .pointer("/child_reports/0/binary_hash_matches")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .pointer("/child_reports/0/verifier_identity_fresh")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        report
            .pointer("/child_reports/0/binary_freshness_basis")
            .and_then(serde_json::Value::as_str),
        Some("stale-verifier-identity")
    );
    std::fs::remove_dir_all(dir).unwrap();
}


#[test]
fn native_gpu_aggregate_treats_child_reported_stale_dependency_as_refresh_debt() {
    let dir = std::env::temp_dir().join(format!(
        "boon-xtask-native-child-reported-freshness-{}-{}",
        std::process::id(),
        monotonic_now_ns().unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let child = dir.join("stale-preview-dependent-child.json");
    let aggregate = dir.join("aggregate.json");
    let command_argv = required_xtask_refresh_argv(
        "verify-native-todomvc-physical-reference-parity",
        &[],
        &child,
    );
    let mut child_report = json!({
        "status": "fail",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-native-todomvc-physical-reference-parity",
        "command_argv": command_argv,
        "measurement_mode": "proof",
        "exit_status": 1,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "binary_path": current_binary_path(),
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "budget_hash": file_hash("budgets/native-gpu.toml"),
        "graph_node_count": 0,
        "per_step_pass_fail": [{
            "id": "fresh-current-preview-evidence",
            "pass": false,
            "detail": "preview E2E report is stale"
        }],
        "artifact_sha256s": [],
        "native_gpu_contract": true,
        "blockers": [
            "physical TodoMVC reference parity is using a stale preview E2E report or framebuffer artifact"
        ]
    });
    if let Some(object) = child_report.as_object_mut() {
        insert_worktree_fingerprint_fields(object, NATIVE_GPU_WORKTREE_FINGERPRINT_SCOPE);
        object.insert(
            "verifier_identity".to_owned(),
            verifier_identity_for_command_args(
                "verify-native-todomvc-physical-reference-parity",
                "proof",
                object
                    .get("command_argv")
                    .and_then(serde_json::Value::as_array)
                    .unwrap()
                    .iter()
                    .map(|arg| arg.as_str().unwrap().to_owned())
                    .collect::<Vec<_>>()
                    .as_slice(),
            )
            .unwrap(),
        );
    }
    write_json(&child, &child_report).unwrap();

    let result = verify_native_gpu_report_bundle(
        &[
            "verify-native-gpu-all".to_owned(),
            "--check-existing".to_owned(),
            "--report".to_owned(),
            aggregate.display().to_string(),
        ],
        "verify-native-gpu-all",
        vec![NativeGpuRequiredReport {
            label: "stale-preview-dependent-child",
            path: child.clone(),
            command: "verify-native-todomvc-physical-reference-parity",
            required_argv: &[],
            upstream_dependencies: &[],
            requires_native_gpu_contract: true,
            max_report_bytes: u64::MAX,
            max_sidecar_bytes: u64::MAX,
        }],
        "test-native-child-reported-freshness",
    );
    assert!(result.is_err());
    let report = read_json(&aggregate).unwrap();
    assert_eq!(
        report
            .get("refresh_debt_child_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        report
            .get("true_blocker_child_count")
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );
    assert_eq!(
        report
            .pointer("/child_reports/0/child_report_blockers_freshness_only")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .pointer("/refresh_commands/0/reason")
            .and_then(serde_json::Value::as_str),
        Some("child-reported-freshness")
    );
    std::fs::remove_dir_all(dir).unwrap();
}


#[test]
fn manifest_refresh_argv_does_not_inherit_observed_cells_flags() {
    let path = Path::new("target/reports/native-gpu/cells-visible-click-e2e-release.json");
    let canonical = required_xtask_refresh_argv(
        "verify-native-cells-visible-click-e2e",
        &[("--profile", "release")],
        path,
    );
    assert!(string_args_contains_pair(
        &canonical,
        "--profile",
        "release"
    ));
    assert!(
        !canonical.iter().any(|arg| arg == "--headed-host-input"),
        "manifest-canonical refresh argv must not inherit stale observed flags: {canonical:?}"
    );
}


#[test]
fn bytes_required_reports_have_canonical_refresh_argv() {
    for required in bytes_machine_plan_required_reports() {
        let path = Path::new(required.path);
        let replay = bytes_machine_plan_required_report_refresh_argv(required, path);
        assert!(
            refresh_queue_command_allowed(
                &replay,
                "run-report-refresh-queue",
                "verify-bytes-machine-plan-all"
            ),
            "{} canonical refresh argv is not allowed: {replay:?}",
            required.label
        );
        assert!(string_args_contains_pair(
            &replay,
            "--report",
            required.path
        ));
        assert!(
            !replay.iter().any(|arg| matches!(arg.as_str(), "--engine")),
            "{} canonical refresh argv contains engine-selection args: {replay:?}",
            required.label
        );
    }
}


#[test]
fn native_preview_e2e_has_no_plan_executor_replay_dependency() {
    let dependencies = native_gpu_required_report_upstream_dependencies("preview-e2e-cells");
    assert!(
        dependencies.is_empty(),
        "native preview E2E must prove native behavior from native reports, not PlanExecutor source replay"
    );
}


#[test]
fn native_gpu_handoff_manifest_models_physical_parity_preview_dependency() {
    let dependencies =
        native_gpu_required_report_upstream_dependencies("todomvc-physical-reference-parity");
    assert_eq!(dependencies.len(), 1);
    let dependency = &dependencies[0];
    assert_eq!(dependency.kind, "consumes-native-report");
    assert_eq!(dependency.label, "preview-e2e-todo_mvc_physical");
    assert_eq!(
        dependency.path,
        "target/reports/native-gpu/preview-e2e-todo_mvc_physical.json"
    );
    assert_eq!(dependency.command, "verify-native-gpu-preview-e2e");
    assert_eq!(dependency.owner_aggregate, "verify-native-gpu-all");
    assert_eq!(
        dependency.owner_aggregate_report_path,
        "target/reports/native-gpu-all.json"
    );
    let replay =
        native_gpu_upstream_dependency_refresh_argv(dependency, Path::new(dependency.path));
    assert_eq!(
        replay.get(1).map(String::as_str),
        Some("verify-native-gpu-preview-e2e")
    );
    assert!(string_args_contains_pair(
        &replay,
        "--example",
        "todo_mvc_physical"
    ));
    assert!(string_args_contains_pair(&replay, "--profile", "release"));
    assert!(replay.iter().any(|arg| arg == "--require-hardware-adapter"));
}


#[test]
fn non_preview_native_reports_have_no_source_replay_dependency() {
    assert!(native_gpu_required_report_upstream_dependencies("present-floor").is_empty());
    assert!(
        native_gpu_required_report_upstream_dependencies("cells-visible-click-e2e-release")
            .is_empty()
    );
}

fn present_floor_contract_report(p95: f64) -> serde_json::Value {
    json!({
        "command": "verify-native-gpu-present-floor",
        "product_only": true,
        "operator_host_input": false,
        "real_os_input": false,
        "sample_input_after_initial_frames": false,
        "observed_real_os_input": false,
        "observed_input_event_wake_count": 0,
        "readback_in_hot_path": false,
        "proof_readback_in_hot_path": false,
        "hot_path_proof_readback_count": 0,
        "measured_frame_count": 32,
        "max_presented_frame_ms_p95": 16.7,
        "max_presented_frame_ms_max": 33.4,
        "max_presented_frame_ms_bounded_outlier_count": 1,
        "max_presented_frame_ms_bounded_outlier_cap": 66.8,
        "presented_frame_ms_bounded_outlier_count": 0,
        "presented_frame_ms_bounded_outlier_policy_pass": true,
        "proof_mode": "counters",
        "render_hook_ms_p95": null,
        "render_loop_mode": "demand_driven",
        "surface_class": "product-preview-app-window-surface",
        "focus_safe": true,
        "hardware_requested": true,
        "presented_frame_ms_p50_p95_p99_max": {
            "p50": 8.0,
            "p95": p95,
            "p99": p95,
            "max": p95,
            "sample_count": 32
        },
        "surface_acquire_ms_p50_p95_p99_max": {
            "p50": 0.1,
            "p95": 0.2,
            "p99": 0.2,
            "max": 0.2,
            "sample_count": 32
        },
        "queue_submit_ms_p50_p95_p99_max": {
            "p50": 0.1,
            "p95": 0.2,
            "p99": 0.2,
            "max": 0.2,
            "sample_count": 32
        },
        "frame_present_ms_p50_p95_p99_max": {
            "p50": 0.1,
            "p95": 0.2,
            "p99": 0.2,
            "max": 0.2,
            "sample_count": 32
        }
    })
}


#[test]
fn native_gpu_label_contract_rejects_isolated_weston_preview_e2e_input() {
    let report = json!({
        "command": "verify-native-gpu-preview-e2e",
        "input_injection_method": "isolated-weston-headless-with-weston-test-control",
        "operator_host_input": false,
        "real_os_input": true
    });
    let blockers = native_gpu_label_contract_blockers("preview-e2e-cells", &report);
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("BoonDriver/app-owned host input")),
        "isolated Weston must not satisfy preview E2E handoff input contract: {blockers:?}"
    );
}


#[test]
fn manifest_scroll_coverage_accepts_real_window_report_keys() {
    let report = json!({
        "status": "fail",
        "app_owned_window_vertical_wheel_input": true,
        "real_horizontal_wheel_input": true,
        "materialized_range_before_after": {
            "status": "real-window-wheel-input"
        },
        "required_real_window_speed_proven": false,
        "preview_frame_ms_p95": 20.1
    });

    assert!(native_scroll_report_vertical_input_covered(&report));
    assert!(native_scroll_report_horizontal_input_covered(&report));
    assert!(native_scroll_report_materialized_range_covered(&report));
    assert_eq!(
        report
            .get("required_real_window_speed_proven")
            .and_then(serde_json::Value::as_bool),
        Some(false),
        "scenario coverage must not rewrite an over-budget scroll-speed report into a speed pass"
    );
}


#[test]
fn manifest_scroll_coverage_rejects_planned_wheel_without_axis_evidence() {
    let report = json!({
        "operator_host_wheel_input": true,
        "materialized_range_before_after": {
            "status": "waiting-for-host-wheel-input"
        }
    });

    assert!(!native_scroll_report_vertical_input_covered(&report));
    assert!(!native_scroll_report_horizontal_input_covered(&report));
    assert!(!native_scroll_report_materialized_range_covered(&report));
}


#[test]
fn preview_e2e_delegates_full_manifest_inputs_when_native_smoke_passes() {
    let report = json!({
        "evidence_tier": "boon-driver",
        "visible_reality_harness": {"status": "pass"},
        "dev_shell_interaction_probe": {"status": "pass"},
        "native_host_input_route_evidence": {"status": "pass"},
        "native_runtime_assertion_evidence": {"status": "pass"},
        "runtime_state_assertions": [
            {"id": "preview-ipc-host-input-0", "pass": true},
            {"id": "preview-ipc-host-input-25", "pass": true}
        ]
    });

    let evidence = native_preview_manifest_scenario_evidence("todomvc", &report);

    assert_eq!(
        evidence.get("status").and_then(serde_json::Value::as_str),
        Some("pass")
    );
    assert!(
        evidence
            .get("delegated_input_scenario_count")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|count| count > 0),
        "full semantic scenario steps should be explicit delegated entries: {evidence:?}"
    );
    assert!(
        evidence
            .pointer("/semantic_input_scenario_coverage/entries")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|entries| entries.iter().any(|entry| {
                entry.get("kind").and_then(serde_json::Value::as_str) == Some("input-scenario")
                    && entry.get("status").and_then(serde_json::Value::as_str) == Some("delegated")
            })),
        "input scenario entries should disclose delegated coverage: {evidence:?}"
    );
}


#[test]
fn scroll_hot_path_rejects_isolated_weston_handoff_evidence() {
    let mut blockers = Vec::new();
    require_common_scroll_hot_path_fields(
        &mut blockers,
        &json!({
            "input_injection_method": "isolated-weston-test-control-axis-specific-scroll-only",
            "launcher_command": "isolated-weston-real-window",
            "wheel_input_evidence_source": "axis-specific-real-window-adapter",
            "speed_timing_window": "post-real-window-input",
            "runtime_dispatch_count_for_passive_scroll": 0,
            "graph_rebuild_count": 0,
            "preview_blocked_on_ipc_count": 0,
            "scroll_root_ids": ["preview"],
            "hit_region_ids": ["hit:preview"],
            "invalidation_classes": ["scroll-transform"],
            "passive_scroll_path_kind": "retained-property-tree",
            "generalized_passive_scroll_path": true,
            "dev_editor_fast_path_kind": "retained-property-tree",
            "passive_scroll_targeting_policy": "generic-layout-axis-largest-area-scroll-region",
            "native_scroll_input_route_evidence": {
                "status": "pass",
                "private_runtime_dispatch_used": false
            },
            "passive_scroll_property_tree_proof": {"status": "pass"},
            "passive_scroll_repaint_proof": {"status": "pass"},
            "scroll_retained_scene_contract": {"status": "pass"},
            "render_graph_contract": {"status": "pass"},
            "post_present_proof_isolation": {"status": "pass"},
            "preview_loop_product_path_contract": {"status": "pass"},
            "product_path_ux_timing": {"status": "pass"}
        }),
    );
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.contains("isolated Weston evidence")),
        "isolated Weston scroll evidence must not satisfy handoff hot-path contract: {blockers:?}"
    );
}


#[test]
fn removed_dev_editor_scroll_surface_selector_fails_closed() {
    let args = vec![
        "verify-native-gpu-scroll-speed".to_owned(),
        "--surface".to_owned(),
        "dev-code-editor".to_owned(),
    ];
    let selector = native_gpu_scroll_selector(&args);

    assert_eq!(selector.label, "dev-code-editor");
    assert!(
        selector
            .blockers
            .iter()
            .any(|blocker| blocker.contains("manifest examples only")),
        "blockers={:?}",
        selector.blockers
    );
}


#[test]
fn removed_dev_editor_scroll_surface_report_is_not_handoff_child() {
    let required_reports = native_gpu_handoff_required_reports();
    assert!(
        required_reports
            .iter()
            .all(|requirement| requirement.label != "scroll-speed-dev-code-editor"),
        "removed dev-editor scroll surface report must stay out of the handoff manifest"
    );
    assert!(
        required_reports.iter().all(|requirement| {
            requirement.path
                != Path::new("target/reports/native-gpu/scroll-speed-dev-code-editor.json")
        }),
        "handoff manifest must not refresh the removed dev-editor scroll surface report"
    );
}
