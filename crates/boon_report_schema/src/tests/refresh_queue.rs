// Included by `../tests.rs`; kept in the parent test module for private schema helper access.

#[test]
fn refresh_queue_schema_accepts_closed_loop_dry_run() {
    assert!(schema_accepts(
        refresh_queue_report(),
        "refresh-queue-closed-loop-dry-run"
    ));
}


#[test]
fn refresh_queue_schema_accepts_closed_loop_appended_results() {
    let mut report = refresh_queue_report();
    report["dry_run"] = json!(false);
    report["closed_loop_stop_reason"] = json!("max-runs");
    report["closed_loop_executed_run_count"] = json!(2);
    report["post_refresh_aggregate_rerun_executed"] = json!(true);
    report["boon_cli_prebuild"] = json!({
        "required": true,
        "executed": true,
        "status": "pass",
        "argv": ["cargo", "build", "-p", "boon_cli"],
        "command": "cargo build -p boon_cli",
        "exit_status": "exit status: 0",
        "exit_code": 0,
        "elapsed_ms": 1.0,
        "stdout": "",
        "stdout_truncated": false,
        "stderr": "",
        "stderr_truncated": false
    });
    report["closed_loop_cycles"] = json!([{
        "cycle": 2,
        "pre_refresh_debt_child_count": 2,
        "selected_count": 1,
        "selected_labels": ["preview-e2e-cells"],
        "skipped_label_count": 0,
        "run_count": 1,
        "pass_count": 1,
        "fail_count": 0,
        "missing_argv_count": 0,
        "invalid_command_count": 0,
        "boon_cli_prebuild": {
            "required": false,
            "executed": false,
            "status": "not-required"
        }
    }]);
    report["run_count"] = json!(2);
    report["pass_count"] = json!(2);
    report["fail_count"] = json!(0);
    report["results"] = json!([
        {
            "label": "cells-native-preview-source-replay",
            "path": "target/reports/bytes-plan/cells-scenario-events-full.json",
            "status": "pass",
            "argv": ["boon_cli", "run", "examples/cells.bn"],
            "command": "boon_cli run examples/cells.bn"
        },
        {
            "label": "preview-e2e-cells",
            "path": "target/reports/native-gpu/preview-e2e-cells.json",
            "status": "pass",
            "argv": ["xtask", "verify-native-gpu-preview-e2e"],
            "command": "xtask verify-native-gpu-preview-e2e"
        }
    ]);
    report["post_refresh_aggregate"] = json!({
        "rerun_requested": true,
        "rerun_executed": true,
        "refresh_debt_child_count": 2,
        "remaining_refresh_command_count": 2,
        "remaining_selected_refresh_command_count": 1,
        "remaining_selected_refresh_commands": [{
            "label": "preview-e2e-cells",
            "path": "target/reports/native-gpu/preview-e2e-cells.json"
        }],
        "selected_burndown": false
    });
    assert!(schema_accepts(
        report,
        "refresh-queue-closed-loop-appended-results"
    ));
}


#[test]
fn refresh_queue_schema_accepts_sidecarized_post_refresh_remaining_selected_commands() {
    let mut report = refresh_queue_report();
    report["dry_run"] = json!(false);
    report["closed_loop_stop_reason"] = json!("max-runs");
    report["post_refresh_aggregate_rerun_executed"] = json!(true);
    report["boon_cli_prebuild"] = json!({
        "required": false,
        "executed": false,
        "status": "not-required"
    });
    report["run_count"] = json!(1);
    report["results"][0]["status"] = json!("pass");
    report["results"][0]["exit_status"] = json!("exit status: 0");
    report["results"][0]["exit_code"] = json!(0);
    report["results"][0]["elapsed_ms"] = json!(1.0);
    report["results"][0]["stdout"] = json!("");
    report["results"][0]["stdout_truncated"] = json!(false);
    report["results"][0]["stderr"] = json!("");
    report["results"][0]["stderr_truncated"] = json!(false);
    report["post_refresh_aggregate"] = json!({
        "rerun_requested": true,
        "rerun_executed": true,
        "refresh_debt_child_count": 1,
        "remaining_refresh_command_count": 1,
        "remaining_selected_refresh_command_count": 1,
        "remaining_selected_refresh_commands": [{
            "label": "present-floor",
            "path": "target/reports/native-gpu/present-floor.json"
        }],
        "selected_burndown": false
    });
    let sidecar_payload =
        report["post_refresh_aggregate"]["remaining_selected_refresh_commands"].clone();
    let sidecar_path = temp_report_path("refresh-queue-post-refresh-selected-sidecar-payload");
    write_json(&sidecar_path, &sidecar_payload).unwrap();
    let sidecar_path_text = sidecar_path.display().to_string();
    let sidecar_sha256 = sha256_file(&sidecar_path).unwrap();
    let sidecar_byte_len = fs::metadata(&sidecar_path).unwrap().len();
    let sidecar_ref = json!({
        "kind": "json-sidecar-ref",
        "sidecar": true,
        "json_pointer_replaced": "/post_refresh_aggregate/remaining_selected_refresh_commands",
        "path": sidecar_path_text,
        "sha256": sidecar_sha256,
        "byte_len": sidecar_byte_len,
        "deduplicated_ref": false,
        "deduplicated_from": JsonValue::Null
    });
    let sidecar_entry = json!({
        "kind": "json-sidecar",
        "json_pointer_replaced": "/post_refresh_aggregate/remaining_selected_refresh_commands",
        "path": sidecar_path.display().to_string(),
        "sha256": sha256_file(&sidecar_path).unwrap(),
        "byte_len": sidecar_byte_len,
        "deduplicated_ref": false,
        "deduplicated_from": JsonValue::Null
    });
    report["post_refresh_aggregate"]["remaining_selected_refresh_commands"] = sidecar_ref;
    report["report_json_sidecar_count"] = json!(1);
    report["report_json_sidecars"] = json!([sidecar_entry]);
    report["report_json_sidecar_total_raw_bytes"] = json!(sidecar_byte_len);
    report["report_json_sidecar_total_ref_bytes"] = json!(sidecar_byte_len);
    report["report_json_sidecar_unique_artifact_count"] = json!(1);
    report["report_json_sidecar_duplicate_ref_count"] = json!(0);
    report["artifact_sha256s"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "path": sidecar_path.display().to_string(),
            "sha256": sha256_file(&sidecar_path).unwrap()
        }));
    let report_path = temp_report_path("refresh-queue-post-refresh-selected-sidecar");
    write_json(&report_path, &report).unwrap();
    let result = verify_report_schema(&report_path);
    let _ = fs::remove_file(&report_path);
    let _ = fs::remove_file(sidecar_path);
    assert!(result.is_ok(), "{result:?}");
}


#[test]
fn refresh_queue_schema_rejects_non_loop_extra_results() {
    let mut report = refresh_queue_report();
    report["closed_loop_requested"] = json!(false);
    report["closed_loop_stop_reason"] = json!("not-requested");
    report["pass_count"] = json!(2);
    report["results"].as_array_mut().unwrap().push(json!({
        "label": "preview-e2e-cells",
        "path": "target/reports/native-gpu/preview-e2e-cells.json",
        "status": "dry-run"
    }));
    assert!(!schema_accepts(
        report,
        "refresh-queue-non-loop-extra-results"
    ));
}


#[test]
fn refresh_queue_schema_rejects_missing_closed_loop_reason() {
    let mut report = refresh_queue_report();
    report
        .as_object_mut()
        .unwrap()
        .remove("closed_loop_stop_reason");
    assert!(!schema_accepts(
        report,
        "refresh-queue-missing-closed-loop-reason"
    ));
}


#[test]
fn refresh_queue_schema_rejects_missing_execution_plan() {
    let mut report = refresh_queue_report();
    report
        .as_object_mut()
        .unwrap()
        .remove("refresh_execution_plan");
    assert!(!schema_accepts(
        report,
        "refresh-queue-missing-execution-plan"
    ));
}


#[test]
fn refresh_queue_schema_rejects_execution_plan_label_drift() {
    let mut report = refresh_queue_report();
    report["refresh_execution_plan"][0]["label"] = json!("preview-e2e-cells");
    assert!(!schema_accepts(
        report,
        "refresh-queue-execution-plan-label-drift"
    ));
}


#[test]
fn refresh_queue_schema_rejects_missing_boon_cli_prebuild() {
    let mut report = refresh_queue_report();
    report.as_object_mut().unwrap().remove("boon_cli_prebuild");
    assert!(!schema_accepts(
        report,
        "refresh-queue-missing-boon-cli-prebuild"
    ));
}
