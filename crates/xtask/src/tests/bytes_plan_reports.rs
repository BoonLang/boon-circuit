// Included by `../tests.rs`; kept in the parent test module for private verifier-helper access.

#[test]
fn native_report_sidecar_total_raw_bytes_derives_unique_paths_when_total_missing() {
    let report = json!({
        "report_json_sidecars": [
            {"path": "target/a.json", "byte_len": 10},
            {"path": "target/a.json", "byte_len": 10, "deduplicated_ref": true},
            {"path": "target/b.json", "byte_len": 25}
        ]
    });
    assert_eq!(native_report_sidecar_total_raw_bytes(&report), 35);

    let declared = json!({
        "report_json_sidecar_total_raw_bytes": 99,
        "report_json_sidecars": [
            {"path": "target/a.json", "byte_len": 10}
        ]
    });
    assert_eq!(native_report_sidecar_total_raw_bytes(&declared), 99);
}


#[test]
fn bytes_machine_plan_proof_commands_require_plan_executor_counters() {
    let required = BytesMachinePlanRequiredReport {
        label: "root-scalar-plan-ops-scenario",
        path: "target/reports/bytes-plan/root-scalar-plan-ops-scenario-run-plan.json",
        command: "run-plan-root-scalar-scenario",
        measurement_mode: "proof",
    };
    assert!(bytes_machine_plan_report_requires_plan_executor_counters(
        &required
    ));
}


#[test]
fn bytes_machine_plan_diagnostic_commands_do_not_require_plan_executor_counters() {
    let required = BytesMachinePlanRequiredReport {
        label: "root-scalar-plan-ops-dump-plan",
        path: "target/reports/bytes-plan/root-scalar-plan-ops-dump-plan.json",
        command: "dump-plan",
        measurement_mode: "diagnostic",
    };
    assert!(!bytes_machine_plan_report_requires_plan_executor_counters(
        &required
    ));
}
