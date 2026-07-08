// Included by `../todomvc.rs`.

// test: persistent_plan_executor_runtime_matches_whole_todomvc_scenario_runner
#[test]
fn persistent_plan_executor_runtime_matches_whole_todomvc_scenario_runner() {
    let compiled = compile_source_path_to_machine_plan(
        Path::new("../../examples/todomvc.bn"),
        TargetProfile::SoftwareDefault,
    )
    .expect("TodoMVC should compile to a MachinePlan");
    let scenario = parse_scenario(Path::new("../../examples/todomvc.scn"))
        .expect("TodoMVC scenario should parse");
    let selected_step_ids = vec![
        "add-test-todo-type".to_owned(),
        "add-test-todo-submit".to_owned(),
        "filter-active".to_owned(),
        "toggle-dynamic-test-todo-under-active-filter".to_owned(),
    ];
    let scenario_step_meta = plan_executor_scenario_step_meta(&scenario.step);
    let step_selection = select_plan_explicit_root_scenario_steps(
        &scenario.name,
        &scenario_step_meta,
        &selected_step_ids,
    )
    .expect("selected TodoMVC steps should be accepted");
    let selected_steps = step_selection
        .selected_indices
        .iter()
        .map(|index| &scenario.step[*index])
        .collect::<Vec<_>>();

    let whole = execute_machine_plan_root_scenario_inner(
        &compiled.plan,
        &selected_steps,
        Path::new("../../examples/todomvc.bn").parent(),
    )
    .expect("whole scenario PlanExecutor runner should pass");

    let mut persistent = PlanExecutorRuntimeState::new(&compiled.plan)
        .expect("persistent PlanExecutor runtime should initialize");
    for step in &selected_steps {
        persistent
            .apply_step(
                &compiled.plan,
                step,
                Path::new("../../examples/todomvc.bn").parent(),
            )
            .expect("persistent PlanExecutor step should apply");
    }
    let incremental = persistent
        .finish(&compiled.plan)
        .expect("persistent PlanExecutor runtime should finish");

    assert_plan_executor_execution_matches("todomvc", "persistent", &incremental, &whole);
    assert_eq!(
        incremental.executor_report["executor"],
        "cpu-plan-root-list-scenario-v1"
    );
    assert_eq!(
        incremental.executor_report["runtime_ast_eval_count"],
        json!(0)
    );
    assert_eq!(
        incremental.executor_report["executable_string_path_count"],
        json!(0)
    );
    assert_eq!(
        incremental.executor_report["unknown_plan_op_count"],
        json!(0)
    );
    assert_eq!(
        incremental.executor_report["executed_indexed_update_count"],
        json!(1)
    );
}

// test: persistent_plan_executor_runtime_accepts_live_source_events
#[test]
fn persistent_plan_executor_runtime_accepts_live_source_events() {
    let compiled = compile_source_path_to_machine_plan(
        Path::new("../../examples/todomvc.bn"),
        TargetProfile::SoftwareDefault,
    )
    .expect("TodoMVC should compile to a MachinePlan");
    let scenario = parse_scenario(Path::new("../../examples/todomvc.scn"))
        .expect("TodoMVC scenario should parse");
    let selected_step_ids = vec![
        "add-test-todo-type".to_owned(),
        "add-test-todo-submit".to_owned(),
        "filter-active".to_owned(),
        "toggle-dynamic-test-todo-under-active-filter".to_owned(),
    ];
    let scenario_step_meta = plan_executor_scenario_step_meta(&scenario.step);
    let step_selection = select_plan_explicit_root_scenario_steps(
        &scenario.name,
        &scenario_step_meta,
        &selected_step_ids,
    )
    .expect("selected TodoMVC steps should be accepted");
    let selected_steps = step_selection
        .selected_indices
        .iter()
        .map(|index| &scenario.step[*index])
        .collect::<Vec<_>>();
    let whole = execute_machine_plan_root_scenario_inner(
        &compiled.plan,
        &selected_steps,
        Path::new("../../examples/todomvc.bn").parent(),
    )
    .expect("whole scenario PlanExecutor runner should pass");

    let mut live_session = PlanExecutorRuntimeState::new(&compiled.plan)
        .expect("persistent PlanExecutor runtime should initialize");
    for (sequence, step) in selected_steps.iter().enumerate() {
        let generic_event =
            GenericSourceEvent::require(step).expect("scenario step should contain source");
        live_session
            .apply_live_source_event(
                &compiled.plan,
                live_source_event_from_generic(&generic_event),
                sequence + 1,
                Path::new("../../examples/todomvc.bn").parent(),
            )
            .expect("PlanExecutor live source event should apply");
    }
    let live = live_session
        .finish(&compiled.plan)
        .expect("persistent PlanExecutor live runtime should finish");

    assert_eq!(live.state_summary, whole.state_summary);
    assert_eq!(
        live.semantic_delta_signatures,
        whole.semantic_delta_signatures
    );
    assert_eq!(live.semantic_deltas, whole.semantic_deltas);
    assert_eq!(live.per_step.len(), whole.per_step.len());
    for (live_step, whole_step) in live.per_step.iter().zip(whole.per_step.iter()) {
        assert_eq!(live_step["source"], whole_step["source"]);
        assert_eq!(
            live_step["semantic_delta_signatures"],
            whole_step["semantic_delta_signatures"]
        );
        assert_eq!(live_step["semantic_deltas"], whole_step["semantic_deltas"]);
        assert_eq!(
            live_step["executed_update_branch_count"],
            whole_step["executed_update_branch_count"]
        );
        assert_eq!(
            live_step["executed_indexed_update_count"],
            whole_step["executed_indexed_update_count"]
        );
        assert_eq!(
            live_step["executed_list_append_count"],
            whole_step["executed_list_append_count"]
        );
    }
    assert_eq!(
        live.executor_report["executed_update_branch_count"],
        whole.executor_report["executed_update_branch_count"]
    );
    assert_eq!(
        live.executor_report["executed_indexed_update_count"],
        whole.executor_report["executed_indexed_update_count"]
    );
    assert_eq!(
        live.executor_report["executed_list_append_count"],
        whole.executor_report["executed_list_append_count"]
    );
    assert_eq!(
        live.executor_report["runtime_ast_eval_count"],
        whole.executor_report["runtime_ast_eval_count"]
    );
    assert_eq!(
        live.executor_report["executable_string_path_count"],
        whole.executor_report["executable_string_path_count"]
    );
    assert_eq!(
        live.executor_report["unknown_plan_op_count"],
        whole.executor_report["unknown_plan_op_count"]
    );
}

// test: plan_executor_live_session_from_project_accepts_live_source_events
#[test]
fn plan_executor_live_session_from_project_accepts_live_source_events() {
    let compiler_units = compiler_source_units_for_path(Path::new("../../examples/todomvc.bn"))
        .expect("TodoMVC source units should load");
    let runtime_units = compiler_units
        .into_iter()
        .map(|unit| RuntimeSourceUnit {
            path: unit.path,
            source: unit.source,
        })
        .collect::<Vec<_>>();
    let compiled = compile_source_units_to_machine_plan(
        "examples/todomvc.bn",
        &compiler_source_units_from_runtime_units(&runtime_units),
        TargetProfile::SoftwareDefault,
    )
    .expect("TodoMVC source units should compile to a MachinePlan");
    let scenario = parse_scenario(Path::new("../../examples/todomvc.scn"))
        .expect("TodoMVC scenario should parse");
    let selected_step_ids = vec![
        "add-test-todo-type".to_owned(),
        "add-test-todo-submit".to_owned(),
        "filter-active".to_owned(),
        "toggle-dynamic-test-todo-under-active-filter".to_owned(),
    ];
    let scenario_step_meta = plan_executor_scenario_step_meta(&scenario.step);
    let step_selection = select_plan_explicit_root_scenario_steps(
        &scenario.name,
        &scenario_step_meta,
        &selected_step_ids,
    )
    .expect("selected TodoMVC steps should be accepted");
    let selected_steps = step_selection
        .selected_indices
        .iter()
        .map(|index| &scenario.step[*index])
        .collect::<Vec<_>>();
    let whole = execute_machine_plan_root_scenario_inner(
        &compiled.plan,
        &selected_steps,
        Path::new("../../examples/todomvc.bn").parent(),
    )
    .expect("whole scenario PlanExecutor runner should pass");

    let mut session = PlanExecutorLiveSession::from_project(
        "examples/todomvc.bn",
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
            .expect("PlanExecutor live source event should apply");
        assert_eq!(report["source"], generic_event.source);
    }
    let live = session
        .finish()
        .expect("PlanExecutor live session should finish");

    assert_observed_live_state_matches_required_root_values(
        "todomvc",
        "PlanExecutorLiveSession",
        &live.state_summary,
        &whole.state_summary,
        &["store.new_todo_text", "store.selected_filter"],
    );
    assert_semantic_deltas_are_ordered_subset(
        "todomvc",
        "PlanExecutorLiveSession",
        &live.semantic_deltas,
        &whole.semantic_deltas,
    );
    assert_eq!(
        live.executor_report["executed_indexed_update_count"],
        whole.executor_report["executed_indexed_update_count"]
    );
    assert_eq!(live.executor_report["runtime_ast_eval_count"], json!(0));
    assert_eq!(
        live.executor_report["executable_string_path_count"],
        json!(0)
    );
    assert_eq!(live.executor_report["unknown_plan_op_count"], json!(0));
}

// test: live_runtime_plan_executor_batch_matches_whole_todomvc_scenario_runner
#[test]
fn live_runtime_plan_executor_batch_matches_whole_todomvc_scenario_runner() {
    let compiler_units = compiler_source_units_for_path(Path::new("../../examples/todomvc.bn"))
        .expect("TodoMVC source units should load");
    let runtime_units = compiler_units
        .into_iter()
        .map(|unit| RuntimeSourceUnit {
            path: unit.path,
            source: unit.source,
        })
        .collect::<Vec<_>>();
    let compiled = compile_source_units_to_machine_plan(
        "examples/todomvc.bn",
        &compiler_source_units_from_runtime_units(&runtime_units),
        TargetProfile::SoftwareDefault,
    )
    .expect("TodoMVC source units should compile to a MachinePlan");
    let scenario = parse_scenario(Path::new("../../examples/todomvc.scn"))
        .expect("TodoMVC scenario should parse");
    let selected_step_ids = vec![
        "add-test-todo-type".to_owned(),
        "add-test-todo-submit".to_owned(),
        "filter-active".to_owned(),
        "toggle-dynamic-test-todo-under-active-filter".to_owned(),
    ];
    let scenario_step_meta = plan_executor_scenario_step_meta(&scenario.step);
    let step_selection = select_plan_explicit_root_scenario_steps(
        &scenario.name,
        &scenario_step_meta,
        &selected_step_ids,
    )
    .expect("selected TodoMVC steps should be accepted");
    let selected_steps = step_selection
        .selected_indices
        .iter()
        .map(|index| &scenario.step[*index])
        .collect::<Vec<_>>();
    let whole = execute_machine_plan_root_scenario_inner(
        &compiled.plan,
        &selected_steps,
        Path::new("../../examples/todomvc.bn").parent(),
    )
    .expect("whole scenario PlanExecutor runner should pass");
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

    let mut runtime = LiveRuntime::from_project("examples/todomvc.bn", &runtime_units)
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
        "todomvc",
        "LiveRuntime batch",
        &runtime.state_summary(),
        &whole.state_summary,
    );
    let output_deltas = serde_json::to_value(&output.semantic_deltas)
        .expect("typed semantic deltas should serialize");
    assert_semantic_deltas_are_ordered_subset(
        "todomvc",
        "LiveRuntime batch",
        &output_deltas,
        &whole.semantic_deltas,
    );
    assert_eq!(
        runtime.engine_provenance_report()["generic_fallback_enabled"],
        false
    );
    assert_live_render_patches_are_targeted("todomvc", "LiveRuntime batch", &output.render_patches);
}

// test: live_runtime_plan_executor_rejects_batch_sequence_and_event_id_conflicts
#[test]
fn live_runtime_plan_executor_rejects_batch_sequence_and_event_id_conflicts() {
    let runtime_units = compiler_source_units_for_path(Path::new("../../examples/todomvc.bn"))
        .expect("TodoMVC source units should load")
        .into_iter()
        .map(|unit| RuntimeSourceUnit {
            path: unit.path,
            source: unit.source,
        })
        .collect::<Vec<_>>();
    let mut runtime = LiveRuntime::from_project("examples/todomvc.bn", &runtime_units)
        .expect("TodoMVC should initialize through PlanExecutor");
    assert_eq!(
        runtime.engine_provenance_report()["generic_fallback_enabled"],
        false
    );

    let output = runtime
        .apply_source_batch_turn(SourceBatch {
            sequence_id: 1,
            events: vec![
                SourceBatchEvent {
                    event_id: 1,
                    event: LiveSourceEvent {
                        source: "store.sources.new_todo_input.change".to_owned(),
                        text: Some("Batch todo".to_owned()),
                        ..LiveSourceEvent::default()
                    },
                },
                SourceBatchEvent {
                    event_id: 2,
                    event: LiveSourceEvent {
                        source: "store.sources.new_todo_input.key_down".to_owned(),
                        key: Some("Enter".to_owned()),
                        ..LiveSourceEvent::default()
                    },
                },
            ],
        })
        .expect("PlanExecutor batch should dispatch ordered public source events");
    assert!(
        output
            .semantic_deltas
            .iter()
            .any(|delta| delta.kind == "ListInsert" && delta.list_id.as_deref() == Some("todos")),
        "ordered source batch must dispatch through public source events"
    );

    let error = runtime
        .apply_source_batch_turn(SourceBatch::single(
            1,
            3,
            LiveSourceEvent {
                source: "store.sources.new_todo_input.change".to_owned(),
                text: Some("stale sequence".to_owned()),
                ..LiveSourceEvent::default()
            },
        ))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("sequence conflict"),
        "equal-sequence LATEST conflicts must be rejected deterministically, got `{error}`"
    );

    let error = runtime
        .apply_source_batch_turn(SourceBatch {
            sequence_id: 2,
            events: vec![
                SourceBatchEvent {
                    event_id: 9,
                    event: LiveSourceEvent {
                        source: "store.sources.new_todo_input.change".to_owned(),
                        text: Some("duplicate event id".to_owned()),
                        ..LiveSourceEvent::default()
                    },
                },
                SourceBatchEvent {
                    event_id: 9,
                    event: LiveSourceEvent {
                        source: "store.sources.new_todo_input.key_down".to_owned(),
                        key: Some("Enter".to_owned()),
                        ..LiveSourceEvent::default()
                    },
                },
            ],
        })
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("event IDs must be strictly increasing"),
        "batch event ID conflicts must be reported deterministically, got `{error}`"
    );
}

// test: live_runtime_plan_executor_routes_duplicate_todo_title_by_occurrence
#[test]
fn live_runtime_plan_executor_routes_duplicate_todo_title_by_occurrence() {
    let runtime_units = compiler_source_units_for_path(Path::new("../../examples/todomvc.bn"))
        .expect("TodoMVC source units should load")
        .into_iter()
        .map(|unit| RuntimeSourceUnit {
            path: unit.path,
            source: unit.source,
        })
        .collect::<Vec<_>>();
    let mut runtime = LiveRuntime::from_project("examples/todomvc.bn", &runtime_units)
        .expect("TodoMVC should initialize through PlanExecutor");
    assert_eq!(
        runtime.engine_provenance_report()["generic_fallback_enabled"],
        false
    );

    for sequence_id in 1..=2 {
        runtime
            .apply_source_batch_turn(SourceBatch {
                sequence_id,
                events: vec![
                    SourceBatchEvent {
                        event_id: 1,
                        event: LiveSourceEvent {
                            source: "store.sources.new_todo_input.change".to_owned(),
                            text: Some("Duplicate".to_owned()),
                            ..LiveSourceEvent::default()
                        },
                    },
                    SourceBatchEvent {
                        event_id: 2,
                        event: LiveSourceEvent {
                            source: "store.sources.new_todo_input.key_down".to_owned(),
                            key: Some("Enter".to_owned()),
                            ..LiveSourceEvent::default()
                        },
                    },
                ],
            })
            .expect("duplicate TodoMVC submit should apply through PlanExecutor");
    }

    let output = runtime
        .apply_source_batch_turn(SourceBatch::single(
            3,
            1,
            LiveSourceEvent {
                source: "todo.sources.todo_checkbox.click".to_owned(),
                target_text: Some("Duplicate".to_owned()),
                target_occurrence: Some(2),
                ..LiveSourceEvent::default()
            },
        ))
        .expect("second duplicate checkbox click should route by occurrence");
    assert!(output.semantic_deltas.iter().any(|delta| {
        delta.kind == "FieldSet" && delta.field_path.as_deref() == Some("completed")
    }));

    let state = runtime.document_state_summary();
    let duplicates = state
        .get("todos")
        .and_then(JsonValue::as_array)
        .expect("TodoMVC summary should include todos")
        .iter()
        .filter(|todo| todo["title"] == "Duplicate")
        .collect::<Vec<_>>();
    assert_eq!(duplicates.len(), 2);
    assert_eq!(duplicates[0]["completed"], false);
    assert_eq!(duplicates[1]["completed"], true);
}

// test: live_runtime_plan_executor_profiled_constructor_reports_no_fallback_provenance
#[test]
fn live_runtime_plan_executor_profiled_constructor_reports_no_fallback_provenance() {
    let runtime_units = compiler_source_units_for_path(Path::new("../../examples/todomvc.bn"))
        .expect("TodoMVC source units should load")
        .into_iter()
        .map(|unit| RuntimeSourceUnit {
            path: unit.path,
            source: unit.source,
        })
        .collect::<Vec<_>>();

    let (runtime, profile) =
        LiveRuntime::from_project_profiled("examples/todomvc.bn", &runtime_units)
            .expect("profiled PlanExecutor constructor should initialize TodoMVC");

    assert_eq!(
        runtime.engine_provenance_report()["engine"],
        "plan_executor"
    );
    assert_eq!(
        runtime.engine_provenance_report()["generic_fallback_enabled"],
        false
    );
    assert_eq!(profile["engine"], "plan_executor");
    assert_eq!(profile["generic_fallback_enabled"], false);
    assert_eq!(
        profile["runtime"]["provenance"]["engine"],
        json!("plan_executor")
    );
}

// test: live_runtime_default_project_constructor_uses_plan_executor_for_document_programs
#[test]
fn live_runtime_default_project_constructor_uses_plan_executor_for_document_programs() {
    let runtime_units = compiler_source_units_for_path(Path::new("../../examples/todomvc.bn"))
        .expect("TodoMVC source units should load")
        .into_iter()
        .map(|unit| RuntimeSourceUnit {
            path: unit.path,
            source: unit.source,
        })
        .collect::<Vec<_>>();
    let runtime = LiveRuntime::from_project("examples/todomvc.bn", &runtime_units)
        .expect("TodoMVC should initialize through the default LiveRuntime project constructor");

    assert_eq!(
        runtime.engine_provenance_report()["engine"],
        "plan_executor"
    );
    assert_eq!(
        runtime.engine_provenance_report()["generic_fallback_enabled"],
        false
    );
}

// test: live_runtime_default_profiled_constructor_reports_plan_executor_selection
#[test]
fn live_runtime_default_profiled_constructor_reports_plan_executor_selection() {
    let runtime_units = compiler_source_units_for_path(Path::new("../../examples/todomvc.bn"))
        .expect("TodoMVC source units should load")
        .into_iter()
        .map(|unit| RuntimeSourceUnit {
            path: unit.path,
            source: unit.source,
        })
        .collect::<Vec<_>>();
    let (runtime, profile) =
        LiveRuntime::from_project_profiled("examples/todomvc.bn", &runtime_units)
            .expect("TodoMVC profiled default constructor should initialize");

    assert_eq!(
        runtime.engine_provenance_report()["engine"],
        "plan_executor"
    );
    assert_eq!(
        runtime.engine_provenance_report()["generic_fallback_enabled"],
        false
    );
    assert_eq!(profile["engine"], "plan_executor");
    assert_eq!(profile["generic_fallback_enabled"], false);
    assert_eq!(
        profile["default_runtime_selection"],
        "plan_executor_document_runtime"
    );
}

// test: live_runtime_plan_executor_document_summary_exposes_todomvc_rows_and_sources
#[test]
fn live_runtime_plan_executor_document_summary_exposes_todomvc_rows_and_sources() {
    let compiler_units = compiler_source_units_for_path(Path::new("../../examples/todomvc.bn"))
        .expect("TodoMVC source units should load");
    let runtime_units = compiler_units
        .into_iter()
        .map(|unit| RuntimeSourceUnit {
            path: unit.path,
            source: unit.source,
        })
        .collect::<Vec<_>>();
    let scenario = parse_scenario(Path::new("../../examples/todomvc.scn"))
        .expect("TodoMVC scenario should parse");
    let selected_step_ids = vec![
        "add-test-todo-type".to_owned(),
        "add-test-todo-submit".to_owned(),
        "filter-active".to_owned(),
        "toggle-dynamic-test-todo-under-active-filter".to_owned(),
    ];
    let scenario_step_meta = plan_executor_scenario_step_meta(&scenario.step);
    let step_selection = select_plan_explicit_root_scenario_steps(
        &scenario.name,
        &scenario_step_meta,
        &selected_step_ids,
    )
    .expect("selected TodoMVC steps should be accepted");
    let batch_events = step_selection
        .selected_indices
        .iter()
        .enumerate()
        .map(|(index, step_index)| {
            let step = &scenario.step[*step_index];
            let generic_event =
                GenericSourceEvent::require(step).expect("scenario step should contain source");
            SourceBatchEvent {
                event_id: (index + 1) as u64,
                event: live_source_event_from_generic(&generic_event),
            }
        })
        .collect::<Vec<_>>();
    let mut runtime = LiveRuntime::from_project("examples/todomvc.bn", &runtime_units)
        .expect("LiveRuntime PlanExecutor mode should initialize");
    runtime
        .apply_source_batch_turn(SourceBatch {
            sequence_id: 1,
            events: batch_events,
        })
        .expect("PlanExecutor batch should apply");

    let summary = runtime.document_state_summary();
    assert_eq!(summary["store"]["selected_filter"], "Active");
    let todos = summary["todos"]
        .as_array()
        .expect("todos should be summarized");
    let dynamic = todos
        .iter()
        .find(|row| row["title"] == "Test todo")
        .expect("dynamic TodoMVC row should be present");
    assert_eq!(dynamic["completed"], true);
    assert_eq!(dynamic["$boon"]["row_key"], 5);
    assert_eq!(
        dynamic["sources"]["todo_checkbox"]["click"]["list_id"],
        "todos"
    );
    assert_eq!(
        dynamic["sources"]["todo_checkbox"]["click"]["target_key"],
        dynamic["$boon"]["row_key"]
    );
    let visible = summary["store"]["visible_todos"]
        .as_array()
        .expect("visible_todos retain should be a document array");
    assert!(
        visible.iter().all(|row| row["completed"] == false),
        "Active filter document summary should expose only active visible rows: {visible:?}"
    );
}

// test: live_runtime_plan_executor_source_inventory_and_values_are_plan_owned
#[test]
fn live_runtime_plan_executor_source_inventory_and_values_are_plan_owned() {
    let compiler_units = compiler_source_units_for_path(Path::new("../../examples/todomvc.bn"))
        .expect("TodoMVC source units should load");
    let runtime_units = compiler_units
        .into_iter()
        .map(|unit| RuntimeSourceUnit {
            path: unit.path,
            source: unit.source,
        })
        .collect::<Vec<_>>();
    let mut runtime = LiveRuntime::from_project("examples/todomvc.bn", &runtime_units)
        .expect("LiveRuntime PlanExecutor mode should initialize");

    assert!(runtime.has_source_path("store.sources.new_todo_input.change"));
    assert!(runtime.has_source_path("store.sources.new_todo_input.events.change"));
    assert!(runtime.source_payload_has_text("store.sources.new_todo_input.events.change"));
    assert!(!runtime.source_payload_has_text("store.sources.new_todo_input.events.key_down"));

    let output = runtime
        .apply_source_event_for_document(LiveSourceEvent {
            source: "store.sources.new_todo_input.events.change".to_owned(),
            text: Some("Plan-owned query".to_owned()),
            source_id: None,
            ..LiveSourceEvent::default()
        })
        .expect("PlanExecutor mode should resolve source_id from MachinePlan routes");
    assert_eq!(
        output.state_summary["store"]["new_todo_text"],
        "Plan-owned query"
    );

    let values = runtime.document_state_values(&["store.new_todo_text".to_owned()]);
    assert_eq!(values["store.new_todo_text"], "Plan-owned query");
    let summaries = runtime.runtime_value_summaries(&["store.new_todo_text".to_owned()], 3, 8, 4);
    assert_eq!(
        summaries["store.new_todo_text"],
        json!({"kind": "string", "value": "Plan-owned query"})
    );
    assert_eq!(
        runtime.engine_provenance_report()["generic_fallback_enabled"],
        false
    );

    let scenario = parse_scenario(Path::new("../../examples/todomvc.scn"))
        .expect("TodoMVC scenario should parse");
    let step = scenario
        .step
        .iter()
        .find(|step| step.id == "add-test-todo-submit")
        .expect("TodoMVC scenario should include add-test-todo-submit");
    let generic_event =
        GenericSourceEvent::require(step).expect("scenario step should contain source");
    let sparse = runtime
        .apply_source_event_for_step_value_summaries(
            step,
            live_source_event_from_generic(&generic_event),
            &["store.new_todo_text".to_owned()],
        )
        .expect("PlanExecutor sparse step helper should stay PlanExecutor-only");
    assert_eq!(
        sparse.value_summaries["store.new_todo_text"]["kind"],
        "string"
    );
    assert_eq!(sparse.value_summaries["store.new_todo_text"]["value"], "");
    assert_eq!(
        runtime.engine_provenance_report()["generic_fallback_enabled"],
        false
    );
}
