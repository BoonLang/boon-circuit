// Included by `../tests.rs`; kept in the parent test module for private schema helper access.

#[test]
fn bytes_machine_plan_child_argv_accepts_boon_cli_run_source_replay_commands() {
    let source_replay_argv = json!([
        "target/debug/boon_cli",
        "run",
        "examples/todomvc.bn",
        "--scenario",
        "examples/todomvc.scn",
        "--report",
        "target/reports/bytes-plan/todomvc-scenario-events-full.json"
    ]);
    let source_replay_argv = source_replay_argv.as_array().unwrap();
    assert!(child_command_argv_proves_expected_command(
        source_replay_argv,
        "run-plan-scenario-events"
    ));
    assert!(!child_command_argv_proves_expected_command(
        source_replay_argv,
        "dump-plan"
    ));
    let semantic_argv = json!([
        "target/debug/boon_cli",
        "run",
        "examples/cells.bn",
        "--scenario",
        "examples/cells.scn",
        "--report",
        "target/reports/bytes-plan/cells-ascii-formula-run.json"
    ]);
    let semantic_argv = semantic_argv.as_array().unwrap();
    assert!(child_command_argv_proves_expected_command(
        semantic_argv,
        "semantic"
    ));
    assert!(child_command_argv_proves_expected_command(
        semantic_argv,
        "run-plan-scenario-events"
    ));
}

fn interaction_report() -> JsonValue {
    let mut report = base_report();
    report["measurement_mode"] = json!("interaction");
    report["interaction_flow_id"] = json!("test-flow");
    report["stage_counters"] = json!({
        "runtime_turn": {
            "p50": 1.0,
            "p95": 2.0,
            "p99": 3.0,
            "max": 4.0,
            "sample_count": 2
        }
    });
    report["hot_path_png_write_count"] = json!(0);
    report["hot_path_report_write_count"] = json!(0);
    report["hot_path_report_serialization_count"] = json!(0);
    report["hot_path_heavy_json_summary_count"] = json!(0);
    report["hot_path_proof_readback_count"] = json!(0);
    report["hot_path_verbose_trace_event_count"] = json!(0);
    report["hot_path_dev_blocking_ipc_count"] = json!(0);
    report
}

fn native_frame_evidence_key() -> JsonValue {
    json!({
        "frame_seq": 3,
        "content_revision": 7,
        "layout_revision": 7,
        "render_scene_revision": 7,
        "surface_id": "surface:test",
        "surface_epoch": 2,
        "input_event_seq": 11,
        "present_id": 3,
        "proof_request_id": 3
    })
}

fn native_frame_pacing() -> JsonValue {
    json!({
        "state": "idle",
        "target_frame_interval_ms": 16.7,
        "last_frame_interval_ms": 16.4,
        "last_frame_lateness_ms": 0.0,
        "timer_due": false,
        "requested_animation_burst_frames_remaining": 0,
        "requested_animation_burst_started_elapsed_ms": null,
        "requested_animation_burst_quiet_until_elapsed_ms": null,
        "requested_animation_burst_hard_stop_elapsed_ms": null,
        "requested_animation_burst_min_frames": 2,
        "requested_animation_quiet_ms": 100,
        "requested_animation_hard_cap_ms": 1000,
        "requested_animation_max_pending_snapshots": 1
    })
}

fn native_gpu_report_with_frame_evidence() -> JsonValue {
    let key = native_frame_evidence_key();
    let pacing = native_frame_pacing();
    let mut report = base_report();
    report["command"] = json!("verify-native-gpu-idle-wake");
    report["command_argv"] = json!(["verify-native-gpu-idle-wake"]);
    report["native_gpu_contract"] = json!(true);
    report["render_loop_mode"] = json!("demand_driven");
    report["surface_id"] = json!("surface:test");
    report["surface_epoch"] = json!(2);
    report["rendered_frame_count"] = json!(3);
    report["last_render_content_revision"] = json!(7);
    report["last_render_layout_revision"] = json!(7);
    report["last_render_scene_revision"] = json!(7);
    report["proof_lag_frames"] = json!(0);
    report["frame_pacing"] = pacing.clone();
    report["frame_evidence_key"] = key.clone();
    report["preview_perf_stats"] = json!({
        "kind": "preview-perf-stats",
        "status": "pass",
        "role": "preview",
        "frame_seq": 3,
        "sample_elapsed_ms": 42.0,
        "render_loop_mode": "demand_driven",
        "frame_pacing": pacing,
        "renders_per_second": 60.0,
        "render_hook_ms": 1.2,
        "present_call_ms": 2.4,
        "frame_present_call_ms": 2.4,
        "surface_acquire_call_ms": 0.2,
        "queue_submit_call_ms": 0.4,
        "present_path_ms": 3.0,
        "input_to_present_ms": 8.0,
        "render_hook_ms_p50_p95_p99_max": {
            "p50": 1.2,
            "p95": 1.2,
            "p99": 1.2,
            "max": 1.2,
            "sample_count": 1
        },
        "layout_ms_p50_p95_p99_max": {
            "p50": 1.0,
            "p95": 1.0,
            "p99": 1.0,
            "max": 1.0,
            "sample_count": 1
        },
        "present_call_ms_p50_p95_p99_max": {
            "p50": 2.4,
            "p95": 2.4,
            "p99": 2.4,
            "max": 2.4,
            "sample_count": 1
        },
        "frame_present_call_ms_p50_p95_p99_max": {
            "p50": 2.4,
            "p95": 2.4,
            "p99": 2.4,
            "max": 2.4,
            "sample_count": 1
        },
        "surface_acquire_call_ms_p50_p95_p99_max": {
            "p50": 0.2,
            "p95": 0.2,
            "p99": 0.2,
            "max": 0.2,
            "sample_count": 1
        },
        "queue_submit_call_ms_p50_p95_p99_max": {
            "p50": 0.4,
            "p95": 0.4,
            "p99": 0.4,
            "max": 0.4,
            "sample_count": 1
        },
        "present_path_ms_p50_p95_p99_max": {
            "p50": 3.0,
            "p95": 3.0,
            "p99": 3.0,
            "max": 3.0,
            "sample_count": 1
        },
        "input_to_present_ms_p50_p95_p99_max": {
            "p50": 8.0,
            "p95": 8.0,
            "p99": 8.0,
            "max": 8.0,
            "sample_count": 1
        },
        "upload_bytes_p50_p95_max": {
            "p50": 2048.0,
            "p95": 2048.0,
            "p99": 2048.0,
            "max": 2048.0,
            "sample_count": 1
        },
        "draw_call_count_p50_p95_max": {
            "p50": 8.0,
            "p95": 8.0,
            "p99": 8.0,
            "max": 8.0,
            "sample_count": 1
        },
        "glyph_cache_hit_rate": 0.98,
        "glyph_cache_hit_rate_p50_p95_max": {
            "p50": 0.98,
            "p95": 0.98,
            "p99": 0.98,
            "max": 0.98,
            "sample_count": 1
        },
        "materialized_item_count": 24,
        "materialized_item_count_p50_p95_max": {
            "p50": 24.0,
            "p95": 24.0,
            "p99": 24.0,
            "max": 24.0,
            "sample_count": 1
        },
        "missed_frame_count": 0,
        "proof_mode": "readback",
        "proof_overhead_ms": 4.0,
        "proof_overhead_ms_p50_p95_max": {
            "p50": 4.0,
            "p95": 4.0,
            "p99": 4.0,
            "max": 4.0,
            "sample_count": 1
        },
        "telemetry_drop_count": 0,
        "last_missed_frame_cause": null,
        "frame_evidence_key": key.clone()
    });
    report["last_interactive_readback_artifact"] = json!({
        "path": "target/reports/native-gpu/test-readback.png",
        "sha256": "test-readback-sha",
        "width": 640,
        "height": 480,
        "presented_revision": 7,
        "content_revision": 7,
        "rendered_frame_count": 3,
        "frame_evidence_key": key,
        "capture_method": "wgpu-visible-surface-copy-src-readback",
        "texture_format": "Bgra8UnormSrgb",
        "nonblank_samples": 100,
        "unique_rgba_values": 4,
        "readback_deadline_ms": 250,
        "readback_poll_status": "completed_before_deadline"
    });
    report
}

fn schema_accepts(report: JsonValue, name: &str) -> bool {
    let path = temp_report_path(name);
    write_json(&path, &report).unwrap();
    let accepted = verify_report_schema(&path).is_ok();
    let _ = fs::remove_file(path);
    accepted
}

fn refresh_queue_report() -> JsonValue {
    let mut report = base_report();
    report["command"] = json!("run-report-refresh-queue");
    report["command_argv"] = json!([
        "xtask",
        "run-report-refresh-queue",
        "target/reports/native-gpu-all.json",
        "--dry-run"
    ]);
    report["measurement_mode"] = json!("diagnostic");
    report["aggregate_report_path"] = json!("target/reports/native-gpu-all.json");
    report["aggregate_status"] = json!("fail");
    report["dry_run"] = json!(true);
    report["closed_loop_requested"] = json!(true);
    report["closed_loop_max_runs"] = json!(4);
    report["closed_loop_stop_reason"] = json!("dry-run");
    report["closed_loop_executed_run_count"] = json!(0);
    report["closed_loop_final_refresh_debt_child_count"] = json!(3);
    report["closed_loop_final_selected_refresh_command_count"] = json!(1);
    report["closed_loop_cycles"] = json!([]);
    report["post_refresh_aggregate_rerun_requested"] = json!(true);
    report["post_refresh_aggregate_rerun_executed"] = json!(false);
    report["label_filter"] = json!(["cells-native-preview-source-replay"]);
    report["limit"] = JsonValue::Null;
    report["output_byte_limit"] = json!(4096);
    report["pre_refresh_debt_child_count"] = json!(3);
    report["pre_product_contract_child_count"] = json!(0);
    report["pre_refresh_first_product_contract_child_count"] = json!(2);
    report["pre_true_blocker_child_count"] = json!(0);
    report["refresh_entry_count"] = json!(3);
    report["selected_count"] = json!(1);
    report["selection_mode"] = json!("dependency-expanded-label-filter");
    report["dependency_expansion_count"] = json!(0);
    report["dependency_deferred_count"] = json!(0);
    report["refresh_phase_summaries"] = json!([{
        "refresh_phase": "upstream-dependency",
        "selected_count": 1
    }]);
    report["refresh_execution_plan"] = json!([{
        "index": 0,
        "label": "cells-native-preview-source-replay",
        "path": "target/reports/bytes-plan/cells-scenario-events-full.json",
        "refresh_phase": "upstream-dependency",
        "dependency_depth": 1,
        "required_by": "preview-e2e-cells",
        "owner_aggregate": "verify-native-gpu-all",
        "selected_by_label_filter": true
    }]);
    report["boon_cli_prebuild"] = json!({
        "required": true,
        "executed": false,
        "dry_run": true,
        "status": "skipped-dry-run",
        "argv": ["cargo", "build", "-p", "boon_cli"]
    });
    report["selected_labels"] = json!(["cells-native-preview-source-replay"]);
    report["full_queue_mode"] = json!(false);
    report["skipped_label_count"] = json!(2);
    report["run_count"] = json!(0);
    report["pass_count"] = json!(1);
    report["fail_count"] = json!(0);
    report["missing_argv_count"] = json!(0);
    report["invalid_command_count"] = json!(0);
    report["results"] = json!([{
        "label": "cells-native-preview-source-replay",
        "path": "target/reports/bytes-plan/cells-scenario-events-full.json",
        "status": "dry-run",
        "argv": ["boon_cli", "run", "examples/cells.bn"],
        "command": "boon_cli run examples/cells.bn"
    }]);
    report["post_refresh_aggregate"] = json!({
        "rerun_requested": true,
        "rerun_executed": false
    });
    report
}

fn native_gpu_all_aggregate_report() -> JsonValue {
    let manifest = native_gpu_handoff_manifest_json().unwrap();
    let manifest_reports = manifest
        .get("reports")
        .and_then(JsonValue::as_array)
        .unwrap();
    let mut required_reports = Vec::new();
    let mut child_reports = Vec::new();
    let mut dependency_edges = Vec::new();
    let mut upstream_reports = Vec::new();
    for manifest_report in manifest_reports {
        let label = manifest_report
            .get("label")
            .and_then(JsonValue::as_str)
            .unwrap();
        let path = manifest_report
            .get("path")
            .and_then(JsonValue::as_str)
            .unwrap();
        let command = manifest_report
            .get("command")
            .and_then(JsonValue::as_str)
            .unwrap();
        let required_argv = manifest_report.get("required_argv").cloned().unwrap();
        let upstream_dependencies = manifest_report
            .get("upstream_dependencies")
            .and_then(JsonValue::as_array)
            .cloned()
            .unwrap_or_default();
        required_reports.push(json!({
            "label": label,
            "path": path,
            "command": command,
            "required_argv": required_argv,
            "upstream_dependencies": upstream_dependencies,
            "requires_native_gpu_contract": manifest_report
                .get("requires_native_gpu_contract")
                .and_then(JsonValue::as_bool)
                .unwrap_or(true),
            "max_report_bytes": manifest_report
                .get("max_report_bytes")
                .and_then(JsonValue::as_u64)
                .unwrap(),
            "max_sidecar_bytes": manifest_report
                .get("max_sidecar_bytes")
                .and_then(JsonValue::as_u64)
                .unwrap()
        }));
        child_reports.push(json!({
            "manifest_index": child_reports.len(),
            "label": label,
            "path": path,
            "command": command,
            "required_argv": manifest_report.get("required_argv").cloned().unwrap(),
            "requires_native_gpu_contract": true,
            "max_report_bytes": manifest_report
                .get("max_report_bytes")
                .and_then(JsonValue::as_u64)
                .unwrap(),
            "max_sidecar_bytes": manifest_report
                .get("max_sidecar_bytes")
                .and_then(JsonValue::as_u64)
                .unwrap(),
            "exists": true,
            "observed_status": "pass",
            "observed_command": command,
            "status_pass": true,
            "schema_file_valid": true,
            "schema_valid": true,
            "native_contract_valid": true,
            "native_contract_ok_for_aggregate": true,
            "all_steps_pass": true,
            "all_steps_ok_for_aggregate": true,
            "status_ok_for_aggregate": true,
            "git_fresh": true,
            "worktree_fresh": true,
            "binary_fresh": true,
            "freshness_debt": false,
            "refresh_command": format!("xtask {command} --report {path}"),
            "refresh_argv": ["xtask", command, "--report", path],
            "report_size_ok": true,
            "sidecar_size_ok": true
        }));
        for dependency in upstream_dependencies {
            let dependency_label = dependency.get("label").and_then(JsonValue::as_str).unwrap();
            let dependency_path = dependency.get("path").and_then(JsonValue::as_str).unwrap();
            let dependency_kind = dependency
                .get("kind")
                .and_then(JsonValue::as_str)
                .unwrap_or("consumes-native-report");
            let owner_aggregate = dependency
                .get("owner_aggregate")
                .and_then(JsonValue::as_str)
                .unwrap_or("verify-native-gpu-all");
            let owner_aggregate_report_path = dependency
                .get("owner_aggregate_report_path")
                .and_then(JsonValue::as_str)
                .unwrap_or("target/reports/native-gpu-all.json");
            let dependency_command = dependency
                .get("command")
                .and_then(JsonValue::as_str)
                .unwrap_or("verify-native-gpu-preview-e2e");
            let dependency_measurement_mode = dependency
                .get("measurement_mode")
                .and_then(JsonValue::as_str)
                .unwrap_or("proof");
            dependency_edges.push(json!({
                "from": label,
                "to": dependency_label,
                "kind": dependency_kind,
                "owner_aggregate": owner_aggregate,
                "owner_aggregate_report_path": owner_aggregate_report_path
            }));
            upstream_reports.push(json!({
                "label": dependency_label,
                "path": dependency_path,
                "required_by": label,
                "kind": dependency_kind,
                "owner_aggregate": owner_aggregate,
                "owner_aggregate_report_path": owner_aggregate_report_path,
                "refresh_command": format!("xtask {dependency_command} --report {dependency_path}"),
                "refresh_argv": ["xtask", dependency_command, "--report", dependency_path],
                "exists": true,
                "sha256": "0".repeat(64),
                "status": "pass",
                "command": dependency_command,
                "measurement_mode": dependency_measurement_mode,
                "schema_valid": true,
                "schema_failure_kind": "none",
                "git_fresh": true,
                "worktree_fresh": true,
                "worktree_fingerprint_basis": "scoped",
                "worktree_fingerprint_scope": "native-gpu-handoff",
                "report_worktree_fingerprint_compared": "0".repeat(64),
                "current_worktree_fingerprint_compared": "0".repeat(64),
                "artifact_hashes_fresh": true,
                "artifact_detail": "fresh",
                "freshness_debt": false,
                "true_blocker": false
            }));
        }
    }
    let report_count = manifest_reports.len() as u64;
    let dependency_count = dependency_edges.len() as u64;
    let mut report = base_report();
    report["command"] = json!("verify-native-gpu-all");
    report["command_argv"] = json!([
        "xtask",
        "verify-native-gpu-all",
        "--check-existing",
        "--report",
        "target/reports/native-gpu-all.json"
    ]);
    report["measurement_mode"] = json!("proof");
    report["aggregate_scope"] = json!("agents-native-gpu-handoff");
    report["check_existing"] = json!(true);
    report["handoff_manifest_path"] = json!(NATIVE_GPU_HANDOFF_MANIFEST_PATH);
    report["handoff_manifest_id"] = json!("native-gpu-handoff");
    report["handoff_manifest_version"] = json!(1);
    report["handoff_manifest_sha256"] =
        json!(sha256_bytes(NATIVE_GPU_HANDOFF_MANIFEST_JSON.as_bytes()));
    report["required_report_count"] = json!(report_count);
    report["checked_report_count"] = json!(report_count);
    report["missing_report_count"] = json!(0);
    report["passed_report_count"] = json!(report_count);
    report["failed_report_count"] = json!(0);
    report["acknowledged_known_failure_count"] = json!(0);
    report["schema_file_valid_report_count"] = json!(report_count);
    report["schema_valid_report_count"] = json!(report_count);
    report["native_contract_valid_report_count"] = json!(report_count);
    report["all_steps_pass_report_count"] = json!(report_count);
    report["git_fresh_report_count"] = json!(report_count);
    report["worktree_fresh_report_count"] = json!(report_count);
    report["identity_fast_refresh_child_count"] = json!(0);
    report["refresh_debt_child_count"] = json!(0);
    report["true_blocker_child_count"] = json!(0);
    report["product_contract_child_count"] = json!(0);
    report["refresh_first_product_contract_child_count"] = json!(0);
    report["upstream_dependency_count"] = json!(dependency_count);
    report["upstream_dependency_refresh_debt_count"] = json!(0);
    report["upstream_dependency_true_blocker_count"] = json!(0);
    report["failure_taxonomy"] = json!({
        "missing_report_count": 0,
        "report_size_failure_count": 0,
        "sidecar_size_failure_count": 0,
        "schema_file_contract_failure_count": 0,
        "schema_file_freshness_failure_count": 0,
        "schema_shape_contract_failure_count": 0,
        "schema_shape_freshness_failure_count": 0,
        "native_product_contract_failure_count": 0,
        "native_refresh_first_product_contract_count": 0,
        "native_contract_freshness_failure_count": 0,
        "all_steps_failure_count": 0,
        "status_failure_count": 0,
        "git_stale_count": 0,
        "worktree_stale_count": 0,
        "binary_stale_count": 0,
        "identity_fast_refresh_child_count": 0,
        "refresh_debt_child_count": 0,
        "true_blocker_child_count": 0,
        "product_contract_child_count": 0,
        "refresh_first_product_contract_child_count": 0,
        "upstream_dependency_refresh_debt_count": 0,
        "upstream_dependency_true_blocker_count": 0
    });
    report["aggregate_checks_pass"] = json!(true);
    report["child_reports"] = json!(child_reports);
    report["refresh_commands"] = json!([]);
    report["true_blocker_children"] = json!([]);
    report["product_contract_children"] = json!([]);
    report["refresh_first_product_contract_children"] = json!([]);
    report["report_dependency_graph"] = json!({
        "kind": "report-dependency-dag-v1",
        "owner": "verify-native-gpu-all",
        "source": "native_gpu_handoff_manifest",
        "upstream_dependency_count": dependency_count,
        "upstream_dependency_refresh_debt_count": 0,
        "upstream_dependency_true_blocker_count": 0,
        "edges": dependency_edges,
        "upstream_reports": upstream_reports
    });
    report["required_reports"] = json!(required_reports);
    report["linked_report_artifacts"] = json!([]);
    report
}


#[test]
fn native_gpu_all_schema_accepts_manifest_dependency_graph() {
    assert!(schema_accepts(
        native_gpu_all_aggregate_report(),
        "native-gpu-all-manifest-dependency-graph"
    ));
}


#[test]
fn native_gpu_all_schema_rejects_source_replay_dependency_graph_edge() {
    let mut report = native_gpu_all_aggregate_report();
    report["report_dependency_graph"]["edges"][0]["kind"] = json!("consumes-source-replay-report");
    report["report_dependency_graph"]["upstream_reports"][0]["kind"] =
        json!("consumes-source-replay-report");
    assert!(!schema_accepts(
        report,
        "native-gpu-all-source-replay-dependency"
    ));
}


#[test]
fn native_gpu_all_schema_rejects_bytes_owner_dependency_graph_edge() {
    let mut report = native_gpu_all_aggregate_report();
    report["report_dependency_graph"]["edges"][0]["owner_aggregate"] =
        json!("verify-bytes-machine-plan-all");
    report["report_dependency_graph"]["upstream_reports"][0]["owner_aggregate"] =
        json!("verify-bytes-machine-plan-all");
    assert!(!schema_accepts(
        report,
        "native-gpu-all-bytes-owner-dependency"
    ));
}


#[test]
fn native_gpu_all_schema_rejects_source_replay_refresh_command() {
    let mut report = native_gpu_all_aggregate_report();
    report["refresh_commands"] = json!([{
        "label": "preview-e2e-cells",
        "path": "target/reports/native-gpu/preview-e2e-cells.json",
        "reason": "identity-freshness",
        "command": "boon_cli run examples/cells.bn --report target/reports/native-gpu/preview-e2e-cells.json",
        "argv": [
            "boon_cli",
            "run",
            "examples/cells.bn",
            "--report",
            "target/reports/native-gpu/preview-e2e-cells.json"
        ]
    }]);
    assert!(!schema_accepts(
        report,
        "native-gpu-all-source-replay-refresh-command"
    ));
}


#[test]
fn native_gpu_all_schema_rejects_engine_refresh_flag() {
    let mut report = native_gpu_all_aggregate_report();
    report["refresh_commands"] = json!([{
        "label": "preview-e2e-cells",
        "path": "target/reports/native-gpu/preview-e2e-cells.json",
        "reason": "identity-freshness",
        "command": "xtask verify-native-gpu-preview-e2e --engine plan --report target/reports/native-gpu/preview-e2e-cells.json",
        "argv": [
            "xtask",
            "verify-native-gpu-preview-e2e",
            "--engine",
            "plan",
            "--report",
            "target/reports/native-gpu/preview-e2e-cells.json"
        ]
    }]);
    assert!(!schema_accepts(
        report,
        "native-gpu-all-engine-refresh-flag"
    ));
}


#[test]
fn native_gpu_all_schema_rejects_wrong_dependency_graph_source() {
    let mut report = native_gpu_all_aggregate_report();
    report["report_dependency_graph"]["source"] = json!("hidden-code-table");
    assert!(!schema_accepts(
        report,
        "native-gpu-all-hidden-dependency-source"
    ));
}


#[test]
fn native_gpu_all_schema_rejects_upstream_dependency_count_drift() {
    let mut report = native_gpu_all_aggregate_report();
    report["report_dependency_graph"]["edges"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert!(!schema_accepts(
        report,
        "native-gpu-all-upstream-count-drift"
    ));
}


#[test]
fn native_gpu_all_schema_rejects_missing_true_blocker_children() {
    let mut report = native_gpu_all_aggregate_report();
    report
        .as_object_mut()
        .unwrap()
        .remove("true_blocker_children");
    assert!(!schema_accepts(
        report,
        "native-gpu-all-missing-true-blocker-children"
    ));
}


#[test]
fn compiled_artifact_report_links_real_artifact_and_sections() {
    let artifact_path = temp_report_path("compiled-artifact-file");
    write_json(
        &artifact_path,
        &json!({
            "artifact_kind": "boonc.compiled_program",
            "artifact_version": 1,
            "format": "boonc-json-v1"
        }),
    )
    .unwrap();
    let artifact_hash = sha256_file(&artifact_path).unwrap();
    let plan_hash = "plan";
    let mut report = json!({
        "status": "pass",
        "report_version": 1,
        "command": "compile-artifact",
        "command_argv": ["boon_cli", "compile"],
        "measurement_mode": "diagnostic",
        "exit_status": 0,
        "generated_at_utc": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string(),
        "git_commit": "test",
        "binary_hash": "test",
        "source_path": "examples/todomvc.bn",
        "source_hash": "source",
        "program_hash": "program",
        "machine_plan_hash": plan_hash,
        "plan_hash": plan_hash,
        "target_profile": "software_default",
        "graph_node_count": 1,
        "semantic_index": {},
        "compiled_schedule": {},
        "compiled_artifact": {
            "path": artifact_path.display().to_string(),
            "sha256": artifact_hash,
            "format": "boonc-json-v1",
            "artifact_version": 1,
            "program_hash": "program",
            "report_schema_hash": report_schema_hash(),
            "machine_plan_hash": plan_hash,
            "plan_hash": plan_hash,
            "target_profile": "software_default",
            "capability_summary": {},
            "machine_plan_verification": {
                "status": "pass",
                "error_count": 0
            },
            "source_unit_count": 1
        },
        "artifact_sections": {
            "semantic_index": true,
            "symbol_table": true,
            "storage_layout": true,
            "source_schemas": true,
            "route_op_streams": true,
            "dependency_graph": true,
            "document_lowering_tables": true,
            "bridge_schemas": true,
            "compiled_schedule": true,
            "runtime_plan": true,
            "machine_plan": true
        },
        "artifact_sha256s": [{
            "path": artifact_path.display().to_string(),
            "sha256": sha256_file(&artifact_path).unwrap()
        }]
    });
    assert!(schema_accepts(report.clone(), "compiled-artifact-valid"));
    report["artifact_sections"]["route_op_streams"] = json!(false);
    assert!(!schema_accepts(report, "compiled-artifact-missing-section"));
    let _ = fs::remove_file(artifact_path);
}


#[test]
fn inspected_compiled_artifact_report_rejects_fake_runtime_load_claims() {
    let artifact_path = temp_report_path("inspected-compiled-artifact-file");
    write_json(
        &artifact_path,
        &json!({
            "artifact_kind": "boonc.compiled_program",
            "artifact_version": 1,
            "format": "boonc-json-v1"
        }),
    )
    .unwrap();
    let artifact_hash = sha256_file(&artifact_path).unwrap();
    let plan_hash = "plan";
    let compiled_artifact = json!({
        "path": artifact_path.display().to_string(),
        "sha256": sha256_file(&artifact_path).unwrap(),
        "format": "boonc-json-v1",
        "artifact_version": 1,
        "program_hash": "program",
        "report_schema_hash": report_schema_hash(),
        "machine_plan_hash": plan_hash,
        "plan_hash": plan_hash,
        "target_profile": "software_default",
        "capability_summary": {},
        "machine_plan_verification": {
            "status": "pass",
            "error_count": 0
        },
        "source_unit_count": 1
    });
    let artifact_sections = json!({
        "semantic_index": true,
        "symbol_table": true,
        "storage_layout": true,
        "source_schemas": true,
        "route_op_streams": true,
        "dependency_graph": true,
        "document_lowering_tables": true,
        "bridge_schemas": true,
        "compiled_schedule": true,
        "runtime_plan": true,
        "machine_plan": true
    });
    let inspection_result = json!({
        "artifact_valid": true,
        "runtime_engine": "plan_executor",
        "plan_executor_runtime_from_artifact": true,
        "plan_executor_provenance": {
            "engine": "plan_executor",
            "generic_fallback_enabled": false
        },
        "runtime_instantiated_from_artifact": true,
        "machine_plan_deserialized_from_artifact": true,
        "machine_plan_hash": plan_hash,
        "plan_hash": plan_hash,
        "target_profile": "software_default",
        "machine_plan_verification": {
            "status": "pass",
            "error_count": 0
        },
        "runtime_plan_present": true,
        "runtime_plan_generic_derived_deserialized_from_artifact": true,
        "runtime_plan_generic_derived_deserialized_counts": {
            "function_count": 0,
            "root_supported_count": 1,
            "indexed_supported_count": 0,
            "unsupported_reason_count": 0
        },
        "runtime_plan_storage_deserialized_from_artifact": true,
        "runtime_plan_storage_deserialized_counts": {
            "root_slot_count": 1,
            "root_initial_field_copy_count": 0,
            "list_slot_count": 1,
            "indexed_row_initial_reset_count": 0,
            "initial_row_count": 0
        },
        "runtime_plan_document_lowering_deserialized_from_artifact": true,
        "runtime_plan_document_lowering_deserialized_counts": {
            "root_summary_path_count": 1,
            "list_summary_field_count": 0,
            "dynamic_list_view_list_count": 0,
            "projection_storage_resolution_count": 0,
            "unresolved_projection_storage_path_count": 0,
            "observed_root_path_count": 0,
            "render_slot_count": 0,
            "render_slot_failure_count": 0
        },
        "runtime_plan_non_route_tables_deserialized_from_artifact": true,
        "runtime_plan_non_route_tables_deserialized_counts": {
            "runtime_symbol_count": 4,
            "scalar_source_path_count": 1,
            "scalar_branch_count": 1,
            "derived_text_transform_count": 0,
            "list_operation_count": 0,
            "list_projection_count": 0,
            "list_source_binding_count": 0
        },
        "source_free_runtime_load_available": true,
        "source_reparse_required_for_current_runtime": false,
        "source_reparse_attempted": false,
        "source_file_access": "not_attempted",
        "parser_ast_required_for_execution": false,
        "typed_ir_required_for_mvp_loader": false,
        "scenario_execution_available": false,
        "blocked_task": "none",
        "scenario_execution_pending_task": "TASK-0901C",
        "missing_runtime_plan_sections": []
    });
    let mut report = json!({
        "status": "pass",
        "report_version": 1,
        "command": "inspect-compiled-artifact",
        "command_argv": ["boon_cli", "inspect-artifact"],
        "measurement_mode": "diagnostic",
        "exit_status": 0,
        "generated_at_utc": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string(),
        "git_commit": "test",
        "binary_hash": "test",
        "artifact_path": artifact_path.display().to_string(),
        "artifact_hash": artifact_hash,
        "program_hash": "program",
        "machine_plan_hash": plan_hash,
        "plan_hash": plan_hash,
        "target_profile": "software_default",
        "capability_summary": {},
        "compiled_artifact": compiled_artifact,
        "artifact_sections": artifact_sections,
        "artifact_sha256s": [{
            "path": artifact_path.display().to_string(),
            "sha256": sha256_file(&artifact_path).unwrap()
        }],
        "inspection_result": inspection_result
    });
    report["inspection_result"]["runtime_plan_source_routes_deserialized_from_artifact"] =
        json!(true);
    report["inspection_result"]["runtime_plan_source_routes_deserialized_counts"] = json!({
        "route_count": 1,
        "id_slot_count": 1,
        "label_slot_count": 1,
        "routes_with_ids": 1,
        "action_table_slot_count": 1,
        "action_op_stream_count": 1,
        "total_action_op_count": 1,
        "max_action_op_count": 1,
        "source_payload_schema_count": 1,
        "source_payload_field_count": 1,
        "source_payload_text_field_count": 1,
        "source_payload_key_field_count": 0,
        "source_payload_address_field_count": 0,
        "source_payload_bytes_field_count": 0,
        "source_payload_pointer_field_count": 0
    });
    assert!(schema_accepts(
        report.clone(),
        "inspected-compiled-artifact-valid"
    ));
    report["inspection_result"]["scenario_execution_available"] = json!(true);
    assert!(!schema_accepts(
        report,
        "inspected-compiled-artifact-fake-scenario-execution"
    ));
    let _ = fs::remove_file(artifact_path);
}


#[test]
fn compiled_artifact_scenario_report_requires_source_free_parity() {
    let artifact_path = temp_report_path("compiled-artifact-scenario-file");
    write_json(
        &artifact_path,
        &json!({
            "artifact_kind": "boonc.compiled_program",
            "artifact_version": 1,
            "format": "boonc-json-v1"
        }),
    )
    .unwrap();
    let artifact_hash = sha256_file(&artifact_path).unwrap();
    let source_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/counter.bn")
        .canonicalize()
        .unwrap();
    let scenario_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/counter.scn")
        .canonicalize()
        .unwrap();
    let source_hash = sha256_file(&source_path).unwrap();
    let scenario_hash = sha256_file(&scenario_path).unwrap();
    let signature_hash = sha256_bytes(b"matching-runtime-signature");
    let plan_hash = "plan";
    let compiled_artifact = json!({
        "path": artifact_path.display().to_string(),
        "sha256": artifact_hash.clone(),
        "format": "boonc-json-v1",
        "artifact_version": 1,
        "program_hash": "program",
        "report_schema_hash": report_schema_hash(),
        "machine_plan_hash": plan_hash,
        "plan_hash": plan_hash,
        "target_profile": "software_default",
        "capability_summary": {},
        "machine_plan_verification": {
            "status": "pass",
            "error_count": 0
        },
        "source_unit_count": 1
    });
    let artifact_sections = json!({
        "semantic_index": true,
        "symbol_table": true,
        "storage_layout": true,
        "source_schemas": true,
        "route_op_streams": true,
        "dependency_graph": true,
        "document_lowering_tables": true,
        "bridge_schemas": true,
        "compiled_schedule": true,
        "runtime_plan": true,
        "machine_plan": true
    });
    let artifact_scenario = json!({
        "scenario_execution_available": true,
        "scenario_execution_from_artifact": true,
        "runtime_instantiated_from_artifact": true,
        "source_reparse_attempted": false,
        "source_file_access": "not_attempted",
        "typed_ir_required_for_artifact_execution": false,
        "parser_ast_required_for_artifact_execution": false,
        "source_oracle_layer": "plan_executor",
        "runtime_engine": "plan_executor",
        "generic_fallback_enabled": false,
        "render_patch_surface": "not_owned_by_plan_executor",
        "artifact_run_step_count": 7,
        "source_run_step_count": 7,
        "source_total_semantic_deltas": 7,
        "artifact_total_semantic_deltas": 7,
        "source_total_render_patches": 0,
        "artifact_total_render_patches": 0,
        "semantic_deltas_match": true,
        "render_patches_match": true,
        "state_summary_match": true,
        "parity_passed": true,
        "source_signature_hash": signature_hash,
        "artifact_signature_hash": signature_hash,
        "artifact_per_step": []
    });
    let report = json!({
        "status": "pass",
        "report_version": 1,
        "command": "verify-compiled-artifact-scenario",
        "command_argv": ["cargo", "xtask", "verify-compiled-artifact-scenario", "counter"],
        "measurement_mode": "proof",
        "exit_status": 0,
        "generated_at_utc": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string(),
        "git_commit": "test",
        "binary_hash": "test",
        "source_path": source_path.display().to_string(),
        "source_hash": source_hash,
        "program_hash": "program",
        "machine_plan_hash": plan_hash,
        "plan_hash": plan_hash,
        "target_profile": "software_default",
        "scenario_path": scenario_path.display().to_string(),
        "scenario_hash": scenario_hash,
        "artifact_path": artifact_path.display().to_string(),
        "artifact_hash": artifact_hash.clone(),
        "compiled_artifact": compiled_artifact,
        "artifact_sections": artifact_sections,
        "artifact_sha256s": [{
            "path": artifact_path.display().to_string(),
            "sha256": artifact_hash
        }],
        "artifact_scenario": artifact_scenario
    });
    assert!(schema_accepts(
        report.clone(),
        "compiled-artifact-scenario-valid"
    ));

    let mut source_read = report.clone();
    source_read["artifact_scenario"]["source_file_access"] = json!("source_read");
    assert!(!schema_accepts(
        source_read,
        "compiled-artifact-scenario-source-read"
    ));

    let mut fake_parity = report.clone();
    fake_parity["artifact_scenario"]["parity_passed"] = json!(false);
    assert!(!schema_accepts(
        fake_parity,
        "compiled-artifact-scenario-fake-parity"
    ));

    let mut hash_mismatch = report.clone();
    hash_mismatch["artifact_scenario"]["artifact_signature_hash"] = json!("different");
    assert!(!schema_accepts(
        hash_mismatch,
        "compiled-artifact-scenario-hash-mismatch"
    ));

    let mut ast_required = report;
    ast_required["artifact_scenario"]["parser_ast_required_for_artifact_execution"] = json!(true);
    assert!(!schema_accepts(
        ast_required,
        "compiled-artifact-scenario-ast-required"
    ));
    let _ = fs::remove_file(artifact_path);
}
