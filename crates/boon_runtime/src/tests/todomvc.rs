// Included by `../tests.rs`; kept in the parent test module for private invariant access.








































































fn assert_root_scenario_product_only(report: &JsonValue) {
    assert!(report.get("comparison_status").is_none());
}







fn selected_plan_executor_source_steps<'a>(
    scenario: &'a Scenario,
    selected_step_ids: Option<&[&str]>,
) -> Vec<&'a ScenarioStep> {
    let scenario_step_meta = plan_executor_scenario_step_meta(&scenario.step);
    let selected_indices = if let Some(selected_step_ids) = selected_step_ids {
        let selected_step_ids = selected_step_ids
            .iter()
            .map(|step| (*step).to_owned())
            .collect::<Vec<_>>();
        select_plan_explicit_root_scenario_steps(
            &scenario.name,
            &scenario_step_meta,
            &selected_step_ids,
        )
        .expect("explicit PlanExecutor scenario steps should be accepted")
        .selected_indices
    } else {
        select_plan_scenario_event_steps(&scenario.name, &scenario_step_meta)
            .expect("PlanExecutor source-event scenario steps should be accepted")
            .selected_indices
    };
    selected_indices
        .into_iter()
        .map(|index| &scenario.step[index])
        .collect::<Vec<_>>()
}

fn runtime_units_for_source_path(source_path: &Path) -> Vec<RuntimeSourceUnit> {
    compiler_source_units_for_path(source_path)
        .expect("source units should load")
        .into_iter()
        .map(|unit| RuntimeSourceUnit {
            path: unit.path,
            source: unit.source,
        })
        .collect::<Vec<_>>()
}

fn assert_no_plan_executor_fallback_counters(label: &str, surface: &str, report: &JsonValue) {
    for key in [
        "runtime_ast_eval_count",
        "executable_string_path_count",
        "unknown_plan_op_count",
        "graph_rebuild_count",
        "graph_clones_per_item",
    ] {
        assert_eq!(
            report[key],
            json!(0),
            "{label} {surface} fallback counter {key} must stay zero"
        );
    }
}

fn assert_plan_executor_execution_matches(
    label: &str,
    surface: &str,
    actual: &RootScenarioExecution,
    expected: &RootScenarioExecution,
) {
    assert_eq!(
        actual.state_summary, expected.state_summary,
        "{label} {surface} final state must match whole scenario PlanExecutor"
    );
    assert_eq!(
        actual.semantic_delta_signatures, expected.semantic_delta_signatures,
        "{label} {surface} semantic delta signatures must match whole scenario PlanExecutor"
    );
    assert_eq!(
        actual.semantic_deltas, expected.semantic_deltas,
        "{label} {surface} semantic deltas must match whole scenario PlanExecutor"
    );
    assert_eq!(
        actual.per_step.len(),
        expected.per_step.len(),
        "{label} {surface} per-step count must match whole scenario PlanExecutor"
    );
    for (actual_step, expected_step) in actual.per_step.iter().zip(expected.per_step.iter()) {
        for key in [
            "source",
            "semantic_delta_signatures",
            "semantic_deltas",
            "executed_update_branch_count",
            "executed_indexed_update_count",
            "executed_list_append_count",
        ] {
            assert_eq!(
                actual_step[key], expected_step[key],
                "{label} {surface} per-step field {key} must match"
            );
        }
    }
    assert_eq!(
        actual.executor_report["executed_update_branch_count"],
        expected.executor_report["executed_update_branch_count"],
        "{label} {surface} root update count must match"
    );
    assert_eq!(
        actual.executor_report["executed_indexed_update_count"],
        expected.executor_report["executed_indexed_update_count"],
        "{label} {surface} indexed update count must match"
    );
    assert_eq!(
        actual.executor_report["executed_list_append_count"],
        expected.executor_report["executed_list_append_count"],
        "{label} {surface} list append count must match"
    );
    assert_no_plan_executor_fallback_counters(label, surface, &actual.executor_report);
}

fn assert_semantic_deltas_are_ordered_subset(
    label: &str,
    surface: &str,
    actual: &JsonValue,
    expected: &JsonValue,
) {
    let actual = actual
        .as_array()
        .expect("actual semantic deltas should be an array");
    let expected = expected
        .as_array()
        .expect("expected semantic deltas should be an array");
    let mut expected_index = 0usize;
    for delta in actual {
        while expected_index < expected.len() && &expected[expected_index] != delta {
            expected_index += 1;
        }
        assert!(
            expected_index < expected.len(),
            "{label} {surface} emitted semantic delta not present in full scenario: {delta}"
        );
        expected_index += 1;
    }
}

fn assert_live_render_patches_are_targeted(
    label: &str,
    surface: &str,
    patches: &[RenderPatch<'static>],
) {
    assert!(
        !patches.is_empty(),
        "{label} {surface} should emit targeted render patches for observed live deltas"
    );
    assert!(
        patches
            .iter()
            .all(|patch| patch.invalidation != RenderInvalidation::DocumentStructure),
        "{label} {surface} must not fall back to full document render invalidation"
    );
}

fn assert_live_state_contains_root_scenario_values(
    label: &str,
    surface: &str,
    actual: &JsonValue,
    expected: &JsonValue,
) {
    let expected = expected
        .as_object()
        .expect("root scenario summary should be an object");
    for (path, expected_value) in expected {
        let actual_value = test_runtime_value_at_path_or_flat(actual, path).unwrap_or_else(|| {
            panic!("{label} {surface} live state is missing root path `{path}`")
        });
        let actual_value = comparable_live_state_value(actual_value, expected_value);
        assert_eq!(
            &actual_value, expected_value,
            "{label} {surface} root path `{path}` must match whole scenario PlanExecutor"
        );
    }
}

fn assert_observed_live_state_matches_required_root_values(
    label: &str,
    surface: &str,
    actual: &JsonValue,
    expected: &JsonValue,
    required_paths: &[&str],
) {
    for path in required_paths {
        let actual_value = test_runtime_value_at_path_or_flat(actual, path).unwrap_or_else(|| {
            panic!("{label} {surface} observed live state is missing root path `{path}`")
        });
        let expected_value = test_runtime_value_at_path_or_flat(expected, path).unwrap_or_else(|| {
            panic!("{label} {surface} whole scenario state is missing root path `{path}`")
        });
        let actual_value = comparable_live_state_value(actual_value, expected_value);
        assert_eq!(
            &actual_value, expected_value,
            "{label} {surface} root path `{path}` must match whole scenario PlanExecutor"
        );
    }
}

fn assert_observed_live_state_values_match_scenario(
    label: &str,
    surface: &str,
    actual: &JsonValue,
    expected: &JsonValue,
) {
    let actual = actual
        .as_object()
        .expect("observed live state summary should be an object");
    for (path, actual_value) in actual {
        if let Some(expected_value) = test_runtime_value_at_path_or_flat(expected, path) {
            let actual_value = comparable_live_state_value(actual_value, expected_value);
            assert_eq!(
                &actual_value, expected_value,
                "{label} {surface} observed root path `{path}` must match whole scenario PlanExecutor"
            );
        }
    }
}

fn comparable_live_state_value(actual: &JsonValue, expected: &JsonValue) -> JsonValue {
    match (actual, expected) {
        (JsonValue::Object(actual), JsonValue::Object(expected)) => {
            let mut comparable = actual.clone();
            if !expected.contains_key("$boon") {
                comparable.remove("$boon");
            }
            if !expected.contains_key("sources") {
                comparable.remove("sources");
            }
            JsonValue::Object(comparable)
        }
        _ => actual.clone(),
    }
}

fn test_runtime_value_at_path_or_flat<'a>(root: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    root.get(path).or_else(|| runtime_value_at_path(root, path))
}

fn assert_plan_executor_live_surfaces_match_scenario_events(
    label: &str,
    source_label: &str,
    source_path: &Path,
    scenario_path: &Path,
    selected_step_ids: Option<&[&str]>,
) {
    let runtime_units = runtime_units_for_source_path(source_path);
    let compiled = compile_source_units_to_machine_plan(
        source_label,
        &compiler_source_units_from_runtime_units(&runtime_units),
        TargetProfile::SoftwareDefault,
    )
    .expect("source units should compile to a MachinePlan");
    let scenario = parse_scenario(scenario_path).expect("scenario should parse");
    let selected_steps = selected_plan_executor_source_steps(&scenario, selected_step_ids);
    let whole = execute_machine_plan_root_scenario_inner(
        &compiled.plan,
        &selected_steps,
        source_path.parent(),
    )
    .expect("whole scenario PlanExecutor runner should pass");
    assert_no_plan_executor_fallback_counters(label, "whole", &whole.executor_report);

    let mut persistent = PlanExecutorRuntimeState::new(&compiled.plan)
        .expect("persistent PlanExecutor runtime should initialize");
    for step in &selected_steps {
        persistent
            .apply_step(&compiled.plan, step, source_path.parent())
            .expect("persistent PlanExecutor step should apply");
    }
    let incremental = persistent
        .finish(&compiled.plan)
        .expect("persistent PlanExecutor runtime should finish");
    assert_plan_executor_execution_matches(label, "apply_step", &incremental, &whole);

    let mut live_state = PlanExecutorRuntimeState::new(&compiled.plan)
        .expect("persistent PlanExecutor live runtime should initialize");
    for (sequence, step) in selected_steps.iter().enumerate() {
        let generic_event =
            GenericSourceEvent::require(step).expect("scenario step should contain source");
        live_state
            .apply_live_source_event(
                &compiled.plan,
                live_source_event_from_generic(&generic_event),
                sequence + 1,
                source_path.parent(),
            )
            .expect("PlanExecutor live source event should apply");
    }
    let live = live_state
        .finish(&compiled.plan)
        .expect("persistent PlanExecutor live runtime should finish");
    assert_plan_executor_execution_matches(label, "live_source_event", &live, &whole);

    let mut session = PlanExecutorLiveSession::from_project(
        source_label,
        &runtime_units,
        TargetProfile::SoftwareDefault,
    )
    .expect("PlanExecutor live session should initialize from source units");
    assert_eq!(session.provenance_report()["engine"], "plan_executor");
    assert_eq!(
        session.provenance_report()["generic_fallback_enabled"],
        false
    );
    for step in &selected_steps {
        let generic_event =
            GenericSourceEvent::require(step).expect("scenario step should contain source");
        let report = session
            .apply_source_event(live_source_event_from_generic(&generic_event))
            .expect("PlanExecutor live session source event should apply");
        assert_eq!(
            report["source"], generic_event.source,
            "{label} PlanExecutorLiveSession should report the applied source"
        );
    }
    let session_output = session
        .finish()
        .expect("PlanExecutor live session should finish");
    assert_observed_live_state_values_match_scenario(
        label,
        "PlanExecutorLiveSession",
        &session_output.state_summary,
        &whole.state_summary,
    );
    assert_semantic_deltas_are_ordered_subset(
        label,
        "PlanExecutorLiveSession",
        &session_output.semantic_deltas,
        &whole.semantic_deltas,
    );
    assert_no_plan_executor_fallback_counters(
        label,
        "PlanExecutorLiveSession",
        &session_output.executor_report,
    );

    let batch_events = selected_steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let generic_event =
                GenericSourceEvent::require(step).expect("scenario step should contain source");
            SourceBatchEvent {
                event_id: (index + 1) as u64,
                event: live_source_event_from_generic(&generic_event),
            }
        })
        .collect::<Vec<_>>();
    let mut runtime = LiveRuntime::from_project(source_label, &runtime_units)
        .expect("LiveRuntime PlanExecutor mode should initialize");
    assert_eq!(
        runtime.engine_provenance_report()["engine"],
        "plan_executor"
    );
    assert_eq!(
        runtime.engine_provenance_report()["generic_fallback_enabled"],
        false
    );
    let output = runtime
        .apply_source_batch_turn(SourceBatch {
            sequence_id: 1,
            events: batch_events,
        })
        .expect("LiveRuntime PlanExecutor batch should apply");
    assert_live_state_contains_root_scenario_values(
        label,
        "LiveRuntime batch",
        &runtime.state_summary(),
        &whole.state_summary,
    );
    let output_deltas = serde_json::to_value(&output.semantic_deltas)
        .expect("typed semantic deltas should serialize");
    assert_semantic_deltas_are_ordered_subset(
        label,
        "LiveRuntime batch",
        &output_deltas,
        &whole.semantic_deltas,
    );
    assert_live_render_patches_are_targeted(label, "LiveRuntime batch", &output.render_patches);
    assert_eq!(
        runtime.engine_provenance_report()["generic_fallback_enabled"],
        false
    );
}

// Nested behavior-area shards keep broad test groups navigable without widening production APIs.
include!("todomvc/compiled_artifacts.rs");
include!("todomvc/list_identity_and_indexes.rs");
include!("todomvc/live_runtime_plan_executor.rs");
include!("todomvc/root_plan_executor.rs");
