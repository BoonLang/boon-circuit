// Included by `../tests.rs`; kept in the parent test module for private invariant access.

#[test]
fn plan_executor_source_replay_report_carries_scoped_freshness_and_identity() {
    let mut report = json!({
        "command": "run-plan-scenario-events",
        "command_argv": [
            "target/debug/boon_cli",
            "run",
            "examples/cells.bn",
            "--scenario",
            "examples/cells.scn",
            "--report",
            "target/reports/bytes-plan/cells-scenario-events-full.json"
        ],
        "measurement_mode": "proof",
        "worktree_fingerprint": "stale-full-worktree",
        "source_hash": "sourcehash",
        "scenario_hash": "scenariohash",
        "target_profile": "native",
        "plan_hash": "planhash",
        "plan_version": {"major": 1, "minor": 0},
        "selected_step_ids": ["edit-a1"],
        "plan_executor_coverage": {"full_scenario_parity": true}
    });
    insert_plan_executor_source_replay_worktree_fields(&mut report);
    insert_plan_executor_source_replay_identity(&mut report);

    assert_eq!(
        report
            .get("worktree_fingerprint_scope")
            .and_then(JsonValue::as_str),
        Some(PLAN_EXECUTOR_SOURCE_REPLAY_WORKTREE_FINGERPRINT_SCOPE)
    );
    assert_eq!(
        report
            .get("worktree_fingerprint")
            .and_then(JsonValue::as_str),
        Some("stale-full-worktree")
    );
    assert_eq!(
        report
            .get("worktree_scoped_fingerprint")
            .and_then(JsonValue::as_str),
        report
            .get("worktree_fingerprints")
            .and_then(JsonValue::as_object)
            .and_then(|fingerprints| {
                fingerprints.get(PLAN_EXECUTOR_SOURCE_REPLAY_WORKTREE_FINGERPRINT_SCOPE)
            })
            .and_then(JsonValue::as_str)
    );
    let inputs = report
        .get("worktree_fingerprint_scope_inputs")
        .and_then(JsonValue::as_object)
        .and_then(|inputs| inputs.get(PLAN_EXECUTOR_SOURCE_REPLAY_WORKTREE_FINGERPRINT_SCOPE))
        .and_then(JsonValue::as_array)
        .expect("scope inputs should be recorded for audit");
    assert!(
        inputs
            .iter()
            .any(|path| path.as_str() == Some("crates/boon_runtime"))
    );
    assert!(
        !inputs
            .iter()
            .any(|path| path.as_str() == Some("docs/plans/GOAL_PROMPT.md"))
    );
    let identity = report
        .get("source_replay_identity")
        .expect("source replay identity should be recorded");
    assert_eq!(
        identity.get("kind").and_then(JsonValue::as_str),
        Some(PLAN_EXECUTOR_SOURCE_REPLAY_IDENTITY_KIND)
    );
    let canonical_args = identity
        .get("canonical_args")
        .and_then(JsonValue::as_array)
        .expect("canonical args should be recorded");
    assert!(
        !canonical_args
            .iter()
            .any(|arg| arg.as_str() == Some("--report"))
    );
    assert!(!canonical_args.iter().any(|arg| {
        arg.as_str() == Some("target/reports/bytes-plan/cells-scenario-events-full.json")
    }));
    assert_eq!(
        report.get("source_replay_identity"),
        plan_executor_source_replay_identity_for_report(&report).as_ref()
    );
}


#[test]
fn cells_formula_scans_with_ascii_bytes_after_text_boundary() {
    let source = include_str!("../../../../../examples/cells/formula.bn");

    assert!(
        source.contains("formula_ascii_bytes"),
        "Cells should keep a named formula TEXT/BYTES boundary"
    );
    assert!(
        source.contains("Text/to_bytes(encoding: Ascii)"),
        "Cells formula grammar scanning should use an explicit ASCII boundary"
    );
    assert!(
        source.contains("Bytes/find(needle: BYTES[1] { 16u2B })"),
        "operator scanning should use BYTES, not TEXT"
    );
    assert!(
        source.contains("Bytes/starts_with(prefix: BYTES[4]"),
        "function-call prefix scanning should use BYTES prefixes"
    );
    assert!(
        !source.contains("Text/find"),
        "Cells formula grammar scanning should not use text search"
    );
}


#[test]
fn compiled_artifact_decodes_cells_generic_derived_runtime_plan_without_ast() {
    let temp_root = TestTempRoot::new("compiled-artifact-generic-derived-test");
    let artifact_path = temp_root.join("cells.boonc");
    emit_compiled_artifact(Path::new("../../examples/cells.bn"), &artifact_path, None).unwrap();
    let artifact = CompiledArtifact::load_from_path(&artifact_path).unwrap();
    let decoded = artifact.runtime_generic_derived_plan().unwrap();
    assert_eq!(decoded.functions.len(), 21);
    assert!(
        decoded.functions.contains_key("formula_ascii_bytes"),
        "Cells BYTES formula scanner helper should be preserved in the compiled generic-derived plan"
    );
    assert_eq!(decoded.supported_root_count(), 2);
    assert_eq!(decoded.supported_indexed_count(), 7);
    assert!(
        decoded.unsupported_reasons.is_empty(),
        "decoded Cells generic-derived plan should preserve clean coverage: {:?}",
        decoded.unsupported_reasons
    );
    let decoded_function_count = decoded.functions.len();

    let mut corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["generic_derived"]["function_count"] =
        json!(decoded_function_count + 1);
    let error =
        RuntimeGenericDerivedPlan::from_artifact(&corrupt["runtime_plan"]["generic_derived"])
            .unwrap_err()
            .to_string();
    assert!(
        error.contains("function_count"),
        "wrong generic-derived count should fail decoding, got {error}"
    );
}


#[test]
fn compiled_artifact_decodes_storage_initialization_runtime_plan_without_ast() {
    let temp_root = TestTempRoot::new("compiled-artifact-storage-test");

    for_core_compiled_artifacts(&temp_root, |example, fixture| {
        let decoded = fixture
            .artifact
            .runtime_storage_initialization_plan()
            .unwrap();
        assert_eq!(
            decoded.root_slots.len(),
            fixture.compiled.storage_initialization.root_slots.len(),
            "decoded root slot count differs for {example}"
        );
        assert_eq!(
            decoded.list_slots.len(),
            fixture.compiled.storage_initialization.list_slots.len(),
            "decoded list slot count differs for {example}"
        );
        for (decoded_slot, planned_slot) in decoded
            .root_slots
            .iter()
            .zip(fixture.compiled.storage_initialization.root_slots.iter())
        {
            assert_eq!(
                decoded_slot.path, planned_slot.path,
                "decoded root path differs for {example}"
            );
            assert_eq!(
                decoded_slot.initial_value, planned_slot.initial_value,
                "decoded root initial value differs for {} in {example}",
                decoded_slot.path
            );
        }
        assert_eq!(
            decoded.list_slots.len(),
            fixture.compiled.storage_initialization.list_slots.len(),
            "decoded list slot count differs for {example}"
        );
        for (decoded_slot, planned_slot) in decoded
            .list_slots
            .iter()
            .zip(fixture.compiled.storage_initialization.list_slots.iter())
        {
            assert_eq!(
                decoded_slot.name, planned_slot.name,
                "decoded list name differs for {example}"
            );
            assert_eq!(
                decoded_slot.capacity, planned_slot.capacity,
                "decoded capacity differs for {} in {example}",
                decoded_slot.name
            );
            assert_eq!(
                decoded_slot.row_template.fields.len(),
                planned_slot.row_template.fields.len(),
                "decoded row-template field count differs for {} in {example}",
                decoded_slot.name
            );
            assert_eq!(
                decoded_slot.initial_rows, planned_slot.initial_rows,
                "decoded initial rows differ for {} in {example}",
                decoded_slot.name
            );
        }
    });

    let artifact_path = temp_root.join("cells.boonc");
    let artifact = CompiledArtifact::load_from_path(&artifact_path).unwrap();
    let decoded = artifact.runtime_storage_initialization_plan().unwrap();
    let mut corrupt = artifact.body.clone();
    corrupt["runtime_plan"]["storage_initialization"]["list_slot_count"] =
        json!(decoded.list_slots.len() + 1);
    let error = RuntimeStorageInitializationPlan::from_artifact(
        &corrupt["runtime_plan"]["storage_initialization"],
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("list_slot_count"),
        "wrong storage list count should fail decoding, got {error}"
    );
    corrupt = artifact.body.clone();
    let (slot_index, field_index) = corrupt["runtime_plan"]["storage_initialization"]["list_slots"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .find_map(|(slot_index, slot)| {
            slot["row_template"]["fields"]
                .as_array()
                .and_then(|fields| (!fields.is_empty()).then_some((slot_index, 0)))
        })
        .expect("Cells artifact should have at least one row-template field");
    corrupt["runtime_plan"]["storage_initialization"]["list_slots"][slot_index]["row_template"]["fields"]
        [field_index]["field_id"] = json!(usize::MAX);
    let error = RuntimeStorageInitializationPlan::from_artifact(
        &corrupt["runtime_plan"]["storage_initialization"],
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("field_id"),
        "wrong row-template field id should fail decoding, got {error}"
    );
}


#[test]
fn compiled_artifact_instantiates_plan_executor_without_source_or_ir() {
    let temp_root = TestTempRoot::new("compiled-artifact-runtime-load-test");

    for_core_compiled_artifacts(&temp_root, |example, fixture| {
        assert_eq!(
            fixture.artifact.body["typed_ir_required_for_mvp_loader"],
            json!(false),
            "{example} artifact should not require typed IR for runtime loading"
        );
        assert_eq!(
            fixture.artifact.body["runtime_plan"]["source_free_runtime_instantiation_ready"],
            json!(true),
            "{example} artifact should declare source-free runtime instantiation readiness"
        );
        let artifact_compiled = CompiledProgram::from_artifact(&fixture.artifact).unwrap();
        assert_eq!(
            artifact_compiled.report()["compiled_from_typed_ir"],
            json!(false),
            "{example} artifact-backed CompiledProgram report should be honest"
        );
        assert_eq!(
            artifact_compiled.report()["runtime_symbol_count"],
            fixture.compiled.report()["runtime_symbol_count"],
            "{example} decoded runtime symbol count should match source compilation"
        );
        assert_eq!(
            artifact_compiled.report()["source_route_op_streams"],
            fixture.compiled.report()["source_route_op_streams"],
            "{example} decoded source-route op streams should match source compilation"
        );

        let mut runtime =
            PlanExecutorLiveSession::from_compiled_artifact(&fixture.artifact).unwrap();
        let summary = runtime.state_summary();
        match example {
            "counter" => {
                assert!(
                    summary.is_object(),
                    "counter artifact runtime should produce a generic state summary"
                );
            }
            "todomvc" => {
                assert_eq!(
                    summary["todos"].as_array().map(Vec::len),
                    Some(4),
                    "TodoMVC artifact runtime should materialize initial todos"
                );
                assert_eq!(summary["store"]["active_count"], json!(3));
                assert_eq!(summary["store"]["completed_count"], json!(1));
            }
            "cells" => {
                assert_eq!(
                    summary["store"]["selected_input"]["address"], "A0",
                    "Cells artifact runtime should materialize List/find root view"
                );
                assert_eq!(
                    summary["store"]["sheet_rows"].as_array().unwrap().len(),
                    24,
                    "Cells artifact runtime should materialize bounded List/chunk root view"
                );
                assert_eq!(summary["cells"][0]["address"], "A0");
                assert_eq!(summary["cells"][0]["default_formula"], "5");
                assert_eq!(summary["cells"][0]["value"], "5");
            }
            _ => unreachable!(),
        }
    });

    let source = temp_root.join("source-deleted-counter.bn");
    std::fs::copy("../../examples/counter.bn", &source).unwrap();
    let artifact_path = temp_root.join("source-deleted-counter.boonc");
    emit_compiled_artifact(&source, &artifact_path, None).unwrap();
    std::fs::remove_file(&source).unwrap();
    let artifact = CompiledArtifact::load_from_path(&artifact_path).unwrap();
    let mut runtime = PlanExecutorLiveSession::from_compiled_artifact(&artifact).unwrap();
    assert!(
        runtime.state_summary().is_object(),
        "artifact runtime load must not depend on source file access"
    );
}

fn assert_compiled_artifact_scenario_paths_match_source_plan_executor(
    label: &str,
    source: &Path,
    scenario: &Path,
) {
    let temp_root = TestTempRoot::new(&format!("compiled-artifact-scenario-{label}"));
    let artifact_path = temp_root.join(format!("{label}.boonc"));

    emit_compiled_artifact(source, &artifact_path, None).unwrap();
    let inspection = inspect_compiled_artifact_report(&artifact_path, None).unwrap();
    assert_eq!(
        inspection["inspection_result"]["runtime_engine"], "plan_executor",
        "{label} artifact inspection must use the PlanExecutor runtime path"
    );
    assert_eq!(
        inspection["inspection_result"]["plan_executor_runtime_from_artifact"], true,
        "{label} artifact inspection must deserialize the embedded MachinePlan"
    );
    assert_eq!(
        inspection["inspection_result"]["source_reparse_attempted"], false,
        "{label} artifact inspection must not reparse source"
    );
    assert_eq!(
        inspection["inspection_result"]["source_file_access"], "not_attempted",
        "{label} artifact inspection must not touch source files"
    );
    let source_output =
        run_plan_scenario_events(source, scenario, TargetProfile::SoftwareDefault, None).unwrap();
    let artifact_output = run_compiled_artifact_scenario(&artifact_path, scenario).unwrap();

    assert_eq!(
        source_output.report["semantic_deltas"],
        serde_json::to_value(&artifact_output.semantic_deltas).unwrap(),
        "{label} artifact scenario semantic deltas must match source PlanExecutor"
    );
    assert_eq!(
        artifact_output.render_patches.len(),
        0,
        "{label} PlanExecutor artifact scenario should not claim obsolete render-patch output"
    );
    assert_eq!(
        source_output.state_summary, artifact_output.state_summary,
        "{label} artifact scenario final state must match source PlanExecutor"
    );
    assert_eq!(
        artifact_output.per_step.len() as u64,
        source_output.report["plan_executor"]["per_step"]
            .as_array()
            .unwrap()
            .len() as u64,
        "{label} artifact scenario should execute the same selected PlanExecutor steps"
    );
}

fn assert_compiled_artifact_example_scenario_matches_source_plan_executor(example: &str) {
    let (source, scenario, _) = example_paths(example).unwrap();
    assert_compiled_artifact_scenario_paths_match_source_plan_executor(example, &source, &scenario);
}


#[test]
fn compiled_artifact_runs_representative_scenarios_through_plan_executor() {
    assert_compiled_artifact_example_scenario_matches_source_plan_executor("todomvc");
    assert_compiled_artifact_scenario_paths_match_source_plan_executor(
        "bytes_indexed_source_payload_plan_ops",
        Path::new("../../examples/bytes_indexed_source_payload_plan_ops.bn"),
        Path::new("../../examples/bytes_indexed_source_payload_plan_ops.scn"),
    );
    std::thread::Builder::new()
        .name("compiled-artifact-cells-scenario".to_owned())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            assert_compiled_artifact_example_scenario_matches_source_plan_executor("cells");
        })
        .expect("Cells artifact scenario thread should start")
        .join()
        .expect("Cells artifact scenario should not panic");
}

