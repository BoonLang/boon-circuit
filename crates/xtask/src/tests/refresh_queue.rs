// Included by `../tests.rs`; kept in the parent test module for private verifier-helper access.

#[test]
fn refresh_queue_dry_run_consumes_structured_argv() {
    let dir = std::env::temp_dir().join(format!(
        "boon-xtask-refresh-queue-{}-{}",
        std::process::id(),
        monotonic_now_ns().unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let aggregate = dir.join("aggregate.json");
    let output = dir.join("refresh-report.json");
    let child = dir.join("child.json");
    write_json(
        &aggregate,
        &json!({
            "status": "fail",
            "command": "verify-native-gpu-all",
            "refresh_commands": [{
                "label": "platform-contract",
                "path": child.display().to_string(),
                "reason": "identity-freshness",
                "command": "stale display command ignored by runner",
                "argv": [
                    current_binary_path(),
                    "verify-platform-contract",
                    "--report",
                    child.display().to_string()
                ]
            }]
        }),
    )
    .unwrap();
    run_report_refresh_queue(&[
        "run-report-refresh-queue".to_owned(),
        aggregate.display().to_string(),
        "--dry-run".to_owned(),
        "--report".to_owned(),
        output.display().to_string(),
    ])
    .unwrap();
    let report = read_json(&output).unwrap();
    assert_eq!(
        report.get("status").and_then(serde_json::Value::as_str),
        Some("pass")
    );
    assert_eq!(
        report
            .get("selected_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        report
            .pointer("/results/kind")
            .and_then(serde_json::Value::as_str),
        Some("json-sidecar-ref")
    );
    let results = json_pointer_value_or_sidecar(&report, "/results").unwrap();
    assert_eq!(
        results
            .pointer("/0/status")
            .and_then(serde_json::Value::as_str),
        Some("dry-run")
    );
    std::fs::remove_dir_all(dir).unwrap();
}


#[test]
fn native_refresh_queue_rejects_source_replay_commands() {
    let dir = std::env::temp_dir().join(format!(
        "boon-xtask-native-refresh-queue-rejects-source-replay-{}-{}",
        std::process::id(),
        monotonic_now_ns().unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let aggregate = dir.join("aggregate.json");
    let output = dir.join("refresh-report.json");
    let child = dir.join("child.json");
    write_json(
        &aggregate,
        &json!({
            "status": "fail",
            "command": "verify-native-gpu-all",
            "refresh_commands": [{
                "label": "bad-source-replay",
                "path": child.display().to_string(),
                "reason": "identity-freshness",
                "command": "boon_cli run examples/cells.bn --report child.json",
                "argv": [
                    "boon_cli",
                    "run",
                    "examples/cells.bn",
                    "--report",
                    child.display().to_string()
                ]
            }]
        }),
    )
    .unwrap();
    let result = run_report_refresh_queue(&[
        "run-report-refresh-queue".to_owned(),
        aggregate.display().to_string(),
        "--dry-run".to_owned(),
        "--report".to_owned(),
        output.display().to_string(),
    ]);
    assert!(
        result
            .err()
            .is_some_and(|error| error.to_string().contains("blocked")),
        "invalid native refresh command should block the gate"
    );
    let report = read_json(&output).unwrap();
    assert_eq!(
        report.get("status").and_then(serde_json::Value::as_str),
        Some("fail")
    );
    assert_eq!(
        report
            .get("invalid_command_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        report.pointer("/boon_cli_prebuild/required"),
        Some(&json!(false)),
        "native refresh must reject boon_cli entries without entering the BYTES prebuild lane"
    );
    let results = json_pointer_value_or_sidecar(&report, "/results").unwrap();
    assert_eq!(
        results
            .pointer("/0/reason")
            .and_then(serde_json::Value::as_str),
        Some("invalid-command")
    );
    std::fs::remove_dir_all(dir).unwrap();
}


#[test]
fn refresh_queue_selection_expands_report_dependency_graph_edges() {
    let aggregate = json!({
        "refresh_commands": [
            {
                "label": "preview-e2e-cells",
                "path": "target/reports/native-gpu/preview-e2e-cells.json",
                "reason": "upstream-identity-freshness",
                "required_by": "preview-e2e-todo_mvc_physical",
                "owner_aggregate": "verify-native-gpu-all",
                "owner_aggregate_report_path": "target/reports/native-gpu-all.json",
                "argv": [current_binary_path(), "verify-native-gpu-preview-e2e", "--example", "cells"]
            },
            {
                "label": "preview-e2e-todo_mvc_physical",
                "path": "target/reports/native-gpu/preview-e2e-todo_mvc_physical.json",
                "reason": "upstream-identity-freshness",
                "required_by": "todomvc-physical-reference-parity",
                "owner_aggregate": "verify-native-gpu-all",
                "owner_aggregate_report_path": "target/reports/native-gpu-all.json",
                "argv": [current_binary_path(), "verify-native-gpu-preview-e2e"]
            },
            {
                "label": "preview-e2e-todo_mvc_physical",
                "path": "target/reports/native-gpu/preview-e2e-todo_mvc_physical.json",
                "reason": "identity-freshness-fast-path",
                "argv": [current_binary_path(), "verify-native-gpu-preview-e2e"]
            },
            {
                "label": "todomvc-physical-reference-parity",
                "path": "target/reports/native-gpu/todomvc-physical-reference-parity.json",
                "reason": "identity-freshness-fast-path",
                "argv": [current_binary_path(), "verify-native-todomvc-physical-reference-parity"]
            }
        ],
        "report_dependency_graph": {
            "kind": "report-dependency-dag-v1",
            "edges": [
                {
                    "from": "todomvc-physical-reference-parity",
                    "to": "preview-e2e-todo_mvc_physical",
                    "kind": "consumes-native-report"
                },
                {
                    "from": "preview-e2e-todo_mvc_physical",
                    "to": "preview-e2e-cells",
                    "kind": "consumes-native-report"
                }
            ]
        }
    });
    let labels = ["todomvc-physical-reference-parity".to_owned()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let plan = plan_refresh_queue_entries(&aggregate, &labels, 1);
    let selected_labels = selected_labels_for_refresh_entries(&plan.selected)
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(
        selected_labels,
        vec![
            "preview-e2e-cells".to_owned(),
            "preview-e2e-todo_mvc_physical".to_owned(),
            "todomvc-physical-reference-parity".to_owned()
        ]
    );
    assert_eq!(plan.dependency_expansion_count, 2);
    assert_eq!(plan.selected.len(), 3);
    assert_eq!(
        plan.selected[0]
            .get("label")
            .and_then(serde_json::Value::as_str),
        Some("preview-e2e-cells")
    );
    assert_eq!(
        plan.selected[1]
            .get("label")
            .and_then(serde_json::Value::as_str),
        Some("preview-e2e-todo_mvc_physical")
    );
    assert_eq!(
        plan.selected[1]
            .get("refresh_phase")
            .and_then(serde_json::Value::as_str),
        Some("upstream-dependency")
    );
    assert_eq!(
        plan.selected[2]
            .get("label")
            .and_then(serde_json::Value::as_str),
        Some("todomvc-physical-reference-parity")
    );
}


#[test]
fn refresh_queue_until_clean_dry_run_reports_closed_loop_without_rerun() {
    let dir = std::env::temp_dir().join(format!(
        "boon-xtask-refresh-queue-loop-dry-run-{}-{}",
        std::process::id(),
        monotonic_now_ns().unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let aggregate = dir.join("aggregate.json");
    let output = dir.join("refresh-report.json");
    let child = dir.join("child.json");
    write_json(
        &aggregate,
        &json!({
            "status": "fail",
            "command": "verify-native-gpu-all",
            "refresh_debt_child_count": 1,
            "refresh_commands": [{
                "label": "platform-contract",
                "path": child.display().to_string(),
                "reason": "identity-freshness",
                "argv": [
                    current_binary_path(),
                    "verify-platform-contract",
                    "--report",
                    child.display().to_string()
                ]
            }]
        }),
    )
    .unwrap();
    run_report_refresh_queue(&[
        "run-report-refresh-queue".to_owned(),
        aggregate.display().to_string(),
        "--dry-run".to_owned(),
        "--until-clean".to_owned(),
        "--max-runs".to_owned(),
        "4".to_owned(),
        "--report".to_owned(),
        output.display().to_string(),
    ])
    .unwrap();
    let report = read_json(&output).unwrap();
    assert_eq!(
        report.get("status").and_then(serde_json::Value::as_str),
        Some("pass")
    );
    assert_eq!(
        report
            .get("closed_loop_requested")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("closed_loop_max_runs")
            .and_then(serde_json::Value::as_u64),
        Some(4)
    );
    assert_eq!(
        report
            .get("closed_loop_stop_reason")
            .and_then(serde_json::Value::as_str),
        Some("dry-run")
    );
    assert_eq!(
        report
            .get("post_refresh_aggregate_rerun_requested")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("post_refresh_aggregate_rerun_executed")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    std::fs::remove_dir_all(dir).unwrap();
}


#[test]
fn refresh_queue_closed_loop_alias_requests_until_clean() {
    let dir = std::env::temp_dir().join(format!(
        "boon-xtask-refresh-queue-closed-loop-alias-{}-{}",
        std::process::id(),
        monotonic_now_ns().unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let aggregate = dir.join("aggregate.json");
    let output = dir.join("refresh-report.json");
    let child = dir.join("child.json");
    write_json(
        &aggregate,
        &json!({
            "status": "fail",
            "command": "verify-native-gpu-all",
            "refresh_debt_child_count": 1,
            "refresh_commands": [{
                "label": "platform-contract",
                "path": child.display().to_string(),
                "reason": "identity-freshness",
                "argv": [
                    current_binary_path(),
                    "verify-platform-contract",
                    "--report",
                    child.display().to_string()
                ]
            }]
        }),
    )
    .unwrap();
    run_report_refresh_queue(&[
        "run-report-refresh-queue".to_owned(),
        aggregate.display().to_string(),
        "--dry-run".to_owned(),
        "--closed-loop".to_owned(),
        "--max-runs".to_owned(),
        "2".to_owned(),
        "--report".to_owned(),
        output.display().to_string(),
    ])
    .unwrap();
    let report = read_json(&output).unwrap();
    assert_eq!(
        report
            .get("closed_loop_requested")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        report
            .get("closed_loop_stop_reason")
            .and_then(serde_json::Value::as_str),
        Some("dry-run")
    );
    assert_eq!(
        report
            .get("post_refresh_aggregate_rerun_requested")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    std::fs::remove_dir_all(dir).unwrap();
}


#[test]
fn refresh_queue_reruns_native_aggregate_argv() {
    let aggregate = json!({
        "command": "verify-native-gpu-all",
        "refresh_debt_child_count": 3,
        "refresh_commands": []
    });
    let path = Path::new("target/reports/native-gpu-all.json");
    let argv = aggregate_rerun_argv(&aggregate, path).expect("native aggregate rerun argv");
    assert!(
        Path::new(&argv[0])
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|file_name| file_name.starts_with("xtask"))
    );
    assert_eq!(
        argv.get(1).map(String::as_str),
        Some("verify-native-gpu-all")
    );
    assert!(argv.iter().any(|arg| arg == "--check-existing"));
    assert!(string_args_contains_pair(
        &argv,
        "--report",
        "target/reports/native-gpu-all.json"
    ));
    assert!(!refresh_queue_command_allowed(
        &argv,
        "run-report-refresh-queue",
        "verify-native-gpu-all"
    ));
}


#[test]
fn refresh_queue_partial_mode_detects_selected_label_burndown() {
    let post = json!({
        "failure_taxonomy": {
            "refresh_debt_child_count": 2
        },
        "refresh_commands": [
            {"label": "cells-visible-click-e2e-release", "path": "cells.json"},
            {"label": "preview-e2e-cells", "path": "preview.json"}
        ]
    });
    let selected = ["cells-visible-click-e2e-release".to_owned()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let remaining = refresh_commands_for_labels(&post, &selected);
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        remaining[0]
            .get("label")
            .and_then(serde_json::Value::as_str),
        Some("cells-visible-click-e2e-release")
    );
    let burned_down = ["preview-e2e-todomvc".to_owned()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert!(refresh_commands_for_labels(&post, &burned_down).is_empty());
    assert_eq!(aggregate_refresh_debt_child_count(&post), 2);
}


#[test]
fn refresh_queue_closed_loop_selection_honors_labels_and_limit() {
    let aggregate = json!({
        "refresh_commands": [
            {"label": "a", "path": "a.json"},
            {"label": "b", "path": "b.json"},
            {"label": "c", "path": "c.json"}
        ]
    });
    let labels = ["b".to_owned(), "c".to_owned()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let (selected, skipped) = select_refresh_queue_entries(&aggregate, &labels, 1);
    assert_eq!(selected.len(), 1);
    assert_eq!(skipped, 2);
    assert_eq!(
        selected[0].get("label").and_then(serde_json::Value::as_str),
        Some("b")
    );
    assert_eq!(
        selected_labels_for_refresh_entries(&selected),
        ["b".to_owned()].into_iter().collect::<BTreeSet<_>>()
    );
}
