// Included by `../tests.rs`; kept in the parent test module for private executor-helper access.

#[test]
fn root_update_execution_surface_is_executor_owned() {
    let mut evaluation = RootJsonUpdateEvaluation {
        supported: true,
        skipped_by_guard: false,
        unsupported_reason: None,
        target_state_id: Some(StateId(3)),
        value: Some(json!("hello")),
        expression_kind: Some("source_payload"),
        source_payload_field: json!("Text"),
        update_constant_id: JsonValue::Null,
        update_constant_value: JsonValue::Null,
        executor_report: json!({}),
    };

    let scalar_surface = select_root_update_execution_surface(PlanOpId(4), &evaluation);
    assert_eq!(
        scalar_surface.kind,
        RootUpdateExecutionSurfaceKind::PlanJson
    );
    assert_eq!(
        scalar_surface.executor_report["execution_surface"],
        "plan-json"
    );

    evaluation.value = Some(json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "abc",
        "byte_len": 3
    }));
    let bytes_surface = select_root_update_execution_surface(PlanOpId(4), &evaluation);
    assert_eq!(
        bytes_surface.kind,
        RootUpdateExecutionSurfaceKind::RuntimeBranch
    );
    assert_eq!(bytes_surface.executor_report["core_value_is_bytes"], true);

    evaluation.skipped_by_guard = true;
    let skipped_surface = select_root_update_execution_surface(PlanOpId(4), &evaluation);
    assert_eq!(
        skipped_surface.kind,
        RootUpdateExecutionSurfaceKind::SkippedByGuard
    );
    assert_eq!(
        skipped_surface.executor_report["execution_surface"],
        "skipped-by-guard"
    );
}


#[test]
fn scenario_checkpoint_assertions_are_executor_owned() {
    let plan = empty_executor_test_plan();
    let root_state = JsonMap::from_iter([(
        "store".to_owned(),
        json!({
            "selected_filter": "Active",
            "new_todo_text": "Draft",
        }),
    )]);
    let list_state = BTreeMap::from([
        (
            1,
            vec![
                PlanExecutorListRow {
                    key: 10,
                    generation: 1,
                    fields: BTreeMap::from([
                        ("title".to_owned(), json!("Write tests")),
                        ("completed".to_owned(), json!(false)),
                        ("editing".to_owned(), json!(true)),
                        ("edit_text".to_owned(), json!("Draft title")),
                    ]),
                },
                PlanExecutorListRow {
                    key: 11,
                    generation: 1,
                    fields: BTreeMap::from([
                        ("title".to_owned(), json!("Compile")),
                        ("completed".to_owned(), json!(true)),
                        ("editing".to_owned(), json!(false)),
                        ("edit_text".to_owned(), json!("Compile")),
                    ]),
                },
            ],
        ),
        (
            2,
            vec![
                PlanExecutorListRow {
                    key: 20,
                    generation: 1,
                    fields: BTreeMap::from([
                        ("address".to_owned(), json!("A0")),
                        ("value".to_owned(), json!("5")),
                        ("formula_text".to_owned(), json!("5")),
                        ("editing_text".to_owned(), json!("5")),
                        ("editing".to_owned(), json!(false)),
                    ]),
                },
                PlanExecutorListRow {
                    key: 21,
                    generation: 1,
                    fields: BTreeMap::from([
                        ("address".to_owned(), json!("B0")),
                        ("error".to_owned(), json!("Cycle")),
                    ]),
                },
            ],
        ),
    ]);

    let report = assert_scenario_checkpoint(
        &plan,
        &root_state,
        &list_state,
        PlanExecutorScenarioCheckpointInput {
            step_id: "checkpoint".to_owned(),
            source_intent_exemption: Some("assertion-only".to_owned()),
            expect_titles: Some(vec!["Write tests".to_owned(), "Compile".to_owned()]),
            expect_completed_titles: Some(vec!["Compile".to_owned()]),
            expect_active_count: Some(1),
            expect_completed_count: Some(1),
            expect_filter: Some("Active".to_owned()),
            expect_new_text: Some("Draft".to_owned()),
            expect_editing_title: Some("Write tests".to_owned()),
            expect_edit_text: Some("Draft title".to_owned()),
            expect_no_editing: Some(false),
            expect_cell: Some(PlanExecutorScenarioCheckpointCellExpectation {
                address: "A0".to_owned(),
                value: Some("5".to_owned()),
                formula: Some("5".to_owned()),
                editing_text: Some("5".to_owned()),
                editing: Some(false),
            }),
            expect_error: Some(PlanExecutorScenarioCheckpointErrorExpectation {
                address: "B0".to_owned(),
                error: "Cycle".to_owned(),
            }),
            expect_root_text: BTreeMap::from([(
                "store.selected_filter".to_owned(),
                "Active".to_owned(),
            )]),
            ..PlanExecutorScenarioCheckpointInput::default()
        },
    )
    .expect("PlanExecutor should own assertion-only checkpoint evaluation");

    assert_eq!(report.report["passed"], true);
    assert_eq!(report.report["source_intent_exemption"], "assertion-only");
    assert_eq!(report.report["checked_expectation_count"], json!(15));
    assert_eq!(
        report.report["checked_expectations"]
            .as_array()
            .expect("checked expectations should be an array")
            .iter()
            .filter(|item| item.as_str() == Some("expect_cell.value"))
            .count(),
        1
    );
}


#[test]
fn root_scenario_materialized_work_validation_is_executor_owned() {
    let update_work = validate_root_scenario_materialized_work("store.input.change", 1, 0, false)
        .expect("update op work should be executable");
    assert_eq!(
        update_work.executor_report["executor"],
        "cpu-plan-root-scenario-materialized-work-v1"
    );
    assert_eq!(update_work.executor_report["update_op_count"], 1);
    assert_eq!(update_work.executor_report["executable_work"], true);

    let derived_work = validate_root_scenario_materialized_work("store.input.change", 0, 1, false)
        .expect("derived value work should be executable");
    assert_eq!(derived_work.executor_report["derived_value_count"], 1);

    let remove_work = validate_root_scenario_materialized_work("todo.remove.click", 0, 0, true)
        .expect("list remove work should be executable");
    assert_eq!(remove_work.executor_report["has_list_remove_work"], true);

    let error = validate_root_scenario_materialized_work("store.input.change", 0, 0, false)
        .expect_err("empty materialized work should be rejected");
    assert!(
        error
            .to_string()
            .contains("found no executable selected-surface work"),
        "unexpected error: {error}"
    );
}


#[test]
fn decode_expected_source_event_extracts_payload_and_reserved_fields() {
    let expected = BTreeMap::from([
        (
            "source".to_owned(),
            toml::Value::String("store.input.change".to_owned()),
        ),
        ("text".to_owned(), toml::Value::String("Typed".to_owned())),
        ("key".to_owned(), toml::Value::String("Enter".to_owned())),
        ("address".to_owned(), toml::Value::String("B2".to_owned())),
        ("target_occurrence".to_owned(), toml::Value::Integer(3)),
        ("target_key".to_owned(), toml::Value::Integer(42)),
        ("target_generation".to_owned(), toml::Value::Integer(7)),
        ("bind_epoch".to_owned(), toml::Value::Integer(9)),
        ("source_epoch".to_owned(), toml::Value::Integer(11)),
        (
            "payload_name".to_owned(),
            toml::Value::String("custom".to_owned()),
        ),
        (
            "bytes_hex".to_owned(),
            toml::Value::String("41 42 43".to_owned()),
        ),
        ("pointer_x".to_owned(), toml::Value::String("12".to_owned())),
    ]);

    let event = decode_expected_source_event("type-input", &expected)
        .expect("expected_source_event should decode in executor");
    assert_eq!(event.source, "store.input.change");
    assert_eq!(event.text, Some("Typed"));
    assert_eq!(event.key, Some("Enter"));
    assert_eq!(event.address, Some("B2"));
    assert_eq!(event.target_occurrence, Some(3));
    assert_eq!(event.target_key, Some(42));
    assert_eq!(event.target_generation, Some(7));
    assert_eq!(event.bind_epoch, Some(9));
    assert_eq!(event.source_epoch, Some(11));
    assert_eq!(event.payload.get("payload_name"), Some(&"custom"));
    assert_eq!(event.payload.get("pointer_x"), Some(&"12"));
    assert_eq!(event.pointer_x, Some("12"));
    assert_eq!(event.payload_bytes.get("bytes"), Some(&b"ABC".to_vec()));
}


#[test]
fn decode_expected_source_event_rejects_named_bytes_payloads() {
    let expected = BTreeMap::from([
        (
            "source".to_owned(),
            toml::Value::String("store.input.change".to_owned()),
        ),
        (
            "image_bytes_hex".to_owned(),
            toml::Value::String("4142".to_owned()),
        ),
    ]);

    let error = decode_expected_source_event("named-bytes", &expected)
        .expect_err("v1 executor should reject named BYTES source payload keys");
    assert!(
        error.to_string().contains("named BYTES source payload key"),
        "unexpected error: {error}"
    );
}


#[test]
fn live_source_event_expectation_matcher_is_executor_owned() {
    let expected = BTreeMap::from([
        (
            "source".to_owned(),
            toml::Value::String("store.input.change".to_owned()),
        ),
        ("text".to_owned(), toml::Value::String("Typed".to_owned())),
        ("target_occurrence".to_owned(), toml::Value::Integer(2)),
        ("source_id".to_owned(), toml::Value::Integer(8)),
    ]);
    let event = PlanExecutorLiveSourceEvent {
        source: "store.input.change",
        text: Some("Typed"),
        key: None,
        list_id: None,
        address: None,
        target_text: None,
        target_occurrence: Some(2),
        target_key: None,
        target_generation: None,
        bind_epoch: None,
        source_epoch: None,
        source_id: Some(8),
    };

    assert_live_source_event_matches_expected("type-input", Some(&expected), event)
        .expect("matching live source event should be accepted");

    let error = assert_live_source_event_matches_expected(
        "type-input",
        Some(&expected),
        PlanExecutorLiveSourceEvent {
            text: Some("Wrong"),
            ..event
        },
    )
    .expect_err("field mismatch should be rejected");
    assert!(
        error
            .to_string()
            .contains("observed live source field `text`"),
        "unexpected error: {error}"
    );

    let error = assert_live_source_event_matches_expected("missing", None, event)
        .expect_err("missing expected_source_event should be rejected");
    assert!(
        error.to_string().contains("without expected_source_event"),
        "unexpected error: {error}"
    );
}


#[test]
fn select_explicit_root_scenario_steps_requires_source_events() {
    let steps = vec![
        PlanExecutorScenarioStepMeta {
            id: "initial".to_owned(),
            has_expected_source_event: false,
        },
        PlanExecutorScenarioStepMeta {
            id: "type".to_owned(),
            has_expected_source_event: true,
        },
    ];

    let selection = select_explicit_root_scenario_steps("counter", &steps, &["type".to_owned()])
        .expect("explicit selected source-event step should be accepted");
    assert_eq!(selection.selected_indices, vec![1]);
    assert_eq!(selection.selected_step_ids, vec!["type"]);
    assert_eq!(
        selection.executor_report["executor"],
        "cpu-plan-explicit-root-scenario-step-selection-v1"
    );

    let error = select_explicit_root_scenario_steps("counter", &steps, &["initial".to_owned()])
        .expect_err("assertion-only selected step should be rejected");
    assert!(
        error.to_string().contains("has no expected_source_event"),
        "unexpected error: {error}"
    );
}


#[test]
fn select_scenario_event_steps_reports_replay_and_assertion_steps() {
    let steps = vec![
        PlanExecutorScenarioStepMeta {
            id: "initial".to_owned(),
            has_expected_source_event: false,
        },
        PlanExecutorScenarioStepMeta {
            id: "type".to_owned(),
            has_expected_source_event: true,
        },
        PlanExecutorScenarioStepMeta {
            id: "assert".to_owned(),
            has_expected_source_event: false,
        },
    ];

    let selection = select_scenario_event_steps("counter", &steps)
        .expect("scenario with source-event steps should be accepted");
    assert_eq!(selection.all_indices, vec![0, 1, 2]);
    assert_eq!(selection.selected_indices, vec![1]);
    assert_eq!(selection.selected_step_ids, vec!["type"]);
    assert_eq!(selection.assertion_only_step_ids, vec!["initial", "assert"]);
    assert_eq!(
        selection.executor_report["executor"],
        "cpu-plan-scenario-events-step-selection-v1"
    );

    let error = select_scenario_event_steps(
        "empty",
        &[PlanExecutorScenarioStepMeta {
            id: "only-assert".to_owned(),
            has_expected_source_event: false,
        }],
    )
    .expect_err("scenario without source events should be rejected");
    assert!(
        error
            .to_string()
            .contains("has no expected_source_event steps"),
        "unexpected error: {error}"
    );
}
