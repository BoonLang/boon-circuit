use super::*;

fn temp_report_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "boon-report-schema-{name}-{}-{}.json",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

fn base_report() -> JsonValue {
    json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string(),
        "command": "verify-report-schema-test",
        "command_argv": ["verify-report-schema-test"],
        "measurement_mode": "proof",
        "exit_status": 0,
        "git_commit": "test",
        "binary_hash": "test",
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "budget_hash": "n/a",
        "graph_node_count": 0,
        "per_step_pass_fail": [{"id": "shape", "pass": true}],
        "artifact_sha256s": []
    })
}

// Report-schema tests are grouped by report domain while staying in this module for private helper access.
include!("tests/common_identity.rs");
include!("tests/native_gpu_aggregate.rs");
include!("tests/native_gpu_reports.rs");
include!("tests/refresh_queue.rs");
