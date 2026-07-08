// Included by `../source_route_reports.rs`.

// test: source_route_command_argv_prefers_artifact_paths_and_preserves_live_invocation
#[test]
fn source_route_command_argv_prefers_artifact_paths_and_preserves_live_invocation() {
    let current = vec![
        "target/debug/boon_cli".to_owned(),
        "run-plan-route".to_owned(),
        "examples/bytes.bn".to_owned(),
    ];
    let preserved = build_source_route_command_argv(SourceRouteCommandArgvInput {
        current_args: current.clone(),
        source_path: "examples/ignored.bn".to_owned(),
        target_profile: "software-default".to_owned(),
        source_route: "store.input.change".to_owned(),
        target_state: "store.input".to_owned(),
        text: None,
        key: None,
        address: None,
        payload: BTreeMap::new(),
        payload_bytes: BTreeMap::new(),
        payload_byte_artifact_paths: BTreeMap::new(),
        report_path: None,
    });
    assert_eq!(preserved, current);

    let argv = build_source_route_command_argv(SourceRouteCommandArgvInput {
        current_args: vec!["xtask".to_owned(), "verify".to_owned()],
        source_path: "examples/bytes.bn".to_owned(),
        target_profile: "software-default".to_owned(),
        source_route: "store.input.change".to_owned(),
        target_state: "store.input".to_owned(),
        text: Some("Typed".to_owned()),
        key: Some("Enter".to_owned()),
        address: Some("A1".to_owned()),
        payload: BTreeMap::from([("mode".to_owned(), "replace".to_owned())]),
        payload_bytes: BTreeMap::from([("bytes".to_owned(), vec![0xde, 0xad, 0xbe, 0xef])]),
        payload_byte_artifact_paths: BTreeMap::from([(
            "bytes".to_owned(),
            "target/reports/event-bytes.bin".to_owned(),
        )]),
        report_path: Some("target/reports/route.json".to_owned()),
    });
    assert_eq!(
        argv,
        vec![
            "target/debug/boon_cli",
            "run-plan-route",
            "examples/bytes.bn",
            "--source",
            "store.input.change",
            "--target-state",
            "store.input",
            "--text",
            "Typed",
            "--key",
            "Enter",
            "--address",
            "A1",
            "--payload",
            "mode=replace",
            "--payload-bytes-file",
            "bytes=target/reports/event-bytes.bin",
            "--report",
            "target/reports/route.json",
        ]
    );
}

// test: source_route_source_event_report_preserves_event_shape
#[test]
fn source_route_source_event_report_preserves_event_shape() {
    let report = build_source_route_source_event_report(SourceRouteSourceEventReportInput {
        source: "store.input.change".to_owned(),
        source_id: 7,
        text: Some("Typed".to_owned()),
        key: Some("Enter".to_owned()),
        list_id: Some("todos".to_owned()),
        address: Some("A1".to_owned()),
        target_text: Some("target".to_owned()),
        target_occurrence: Some(2),
        target_key: Some(42),
        target_generation: Some(3),
        bind_epoch: Some(4),
        source_epoch: Some(5),
        payload: BTreeMap::from([("mode".to_owned(), "replace".to_owned())]),
        payload_bytes_report: json!({
            "bytes": {
                "$boon_type": "BYTES",
                "storage": "artifact",
                "artifact_path": "target/reports/event-bytes.bin"
            }
        }),
        pointer_x: Some("10".to_owned()),
        pointer_y: Some("11".to_owned()),
        pointer_width: Some("12".to_owned()),
        pointer_height: Some("13".to_owned()),
    });

    assert_eq!(report["source"], "store.input.change");
    assert_eq!(report["source_id"], 7);
    assert_eq!(report["text"], "Typed");
    assert_eq!(report["payload"]["mode"], "replace");
    assert_eq!(
        report["payload_bytes"]["bytes"]["artifact_path"],
        "target/reports/event-bytes.bin"
    );
    assert_eq!(report["pointer_height"], "13");
}

// test: source_route_command_output_assembles_event_argv_report_and_artifacts
#[test]
fn source_route_command_output_assembles_event_argv_report_and_artifacts() {
    let temp_dir = std::env::temp_dir().join(format!(
        "boon-plan-executor-source-route-output-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    let report_path = temp_dir.join("route-report.json");
    let payload = vec![9, 8, 7, 6];

    let output = assemble_source_route_command_output(SourceRouteCommandOutputInput {
        current_args: vec!["xtask".to_owned()],
        generated_at_utc: "2026-06-28T00:00:00Z".to_owned(),
        git_commit: "abc123".to_owned(),
        worktree_fingerprint: "worktreehash".to_owned(),
        binary_hash: "binhash".to_owned(),
        binary_path: "target/debug/boon_cli".to_owned(),
        source_path: "examples/bytes.bn".to_owned(),
        source_hash: "sourcehash".to_owned(),
        source_files: vec!["examples/bytes.bn".to_owned()],
        program_hash: "programhash".to_owned(),
        program_kind: "single-file".to_owned(),
        program_file_count: 1,
        graph_node_count: 2,
        load_pipeline_profile: json!({"total_ms": 1.0}),
        target_profile: "software-default".to_owned(),
        source_route: "store.receive".to_owned(),
        target_state: "store.blob".to_owned(),
        event: SourceRouteSourceEventReportInput {
            source: "store.receive".to_owned(),
            source_id: 3,
            text: Some("ignored".to_owned()),
            key: Some("Enter".to_owned()),
            list_id: None,
            address: Some("A1".to_owned()),
            target_text: None,
            target_occurrence: None,
            target_key: None,
            target_generation: None,
            bind_epoch: Some(4),
            source_epoch: Some(5),
            payload: BTreeMap::from([("mode".to_owned(), "replace".to_owned())]),
            payload_bytes_report: JsonValue::Null,
            pointer_x: Some("10".to_owned()),
            pointer_y: Some("11".to_owned()),
            pointer_width: None,
            pointer_height: None,
        },
        payload_bytes: BTreeMap::from([("bytes".to_owned(), payload.clone())]),
        report_path: Some(report_path),
        plan_hash: "planhash".to_owned(),
        plan_version: json!({"major": 1}),
        capability_summary: json!({"executable": true}),
        route_surface: json!({"expression_kind": "SourcePayload"}),
        state_summary: json!({"store": {"blob": "ok"}}),
        semantic_delta_signatures: vec!["FieldSet:store.blob".to_owned()],
        semantic_deltas: json!([{"kind": "FieldSet"}]),
        plan_executor: json!({"executor": "cpu-plan-source-route-v1"}),
        inline_byte_limit: 3,
    })
    .unwrap();

    let digest = sha256_bytes(&payload);
    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.report["plan_executor_status"], "pass");
    assert!(output.report.get("accepted_for_product_status").is_none());
    assert!(output.report.get("comparison_status").is_none());
    assert_eq!(
        output.report["plan_executor"]["command_output_core"]["executor"],
        "cpu-plan-source-route-command-output-v1"
    );
    assert_eq!(
        output.source_event["payload_bytes"]["bytes"]["storage"],
        "artifact"
    );
    assert_eq!(
        output.source_event["payload_bytes"]["bytes"]["artifact_sha256"],
        digest
    );
    assert_eq!(output.artifact_sha256s.len(), 1);
    let artifact_path = output.artifact_sha256s[0]["path"].as_str().unwrap();
    assert_eq!(fs::read(artifact_path).unwrap(), payload);
    assert!(
        output
            .command_argv
            .windows(2)
            .any(|window| window == ["--payload-bytes-file", &format!("bytes={artifact_path}")])
    );
    assert_eq!(output.report["source_event"], output.source_event);

    let _ = fs::remove_dir_all(&temp_dir);
}

// test: root_scenario_command_output_assembles_report_and_executor_core
#[test]
fn root_scenario_command_output_assembles_report_and_executor_core() {
    let output = assemble_root_scenario_command_output(RootScenarioCommandOutputInput {
        command_argv: vec![
            "target/debug/boon_cli".to_owned(),
            "run-plan-root-scalar-scenario".to_owned(),
            "examples/counter.bn".to_owned(),
        ],
        generated_at_utc: "2026-06-29T00:00:00Z".to_owned(),
        git_commit: "abc123".to_owned(),
        worktree_fingerprint: "worktreehash".to_owned(),
        binary_hash: "binhash".to_owned(),
        binary_path: "target/debug/boon_cli".to_owned(),
        source_path: "examples/counter.bn".to_owned(),
        source_hash: "sourcehash".to_owned(),
        source_files: vec!["examples/counter.bn".to_owned()],
        scenario_path: "examples/counter.scn".to_owned(),
        scenario_hash: "scenariohash".to_owned(),
        program_hash: "programhash".to_owned(),
        program_kind: "single-file".to_owned(),
        program_file_count: 1,
        graph_node_count: 3,
        load_pipeline_profile: json!({"total_ms": 1.0}),
        target_profile: "software-default".to_owned(),
        plan_hash: "planhash".to_owned(),
        plan_version: json!({"major": 1}),
        capability_summary: json!({"executable": true}),
        selected_step_ids: vec!["increment".to_owned(), "inspect".to_owned()],
        state_summary: json!({"store": {"count": 1}}),
        semantic_delta_signatures: vec!["FieldSet:store.count".to_owned()],
        semantic_deltas: json!([{"kind": "FieldSet"}]),
        plan_executor: json!({"executor": "cpu-plan-root-scenario-v1"}),
    });

    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.report["plan_executor_status"], "pass");
    assert!(output.report.get("accepted_for_product_status").is_none());
    assert!(output.report.get("comparison_status").is_none());
    assert_eq!(output.report["command"], "run-plan-root-scalar-scenario");
    assert_eq!(
        output.report["selected_step_ids"],
        json!(["increment", "inspect"])
    );
    assert_eq!(json!(output.command_argv), output.report["command_argv"]);
    assert_eq!(
        output.report["plan_executor"]["command_output_core"]["executor"],
        "cpu-plan-root-scenario-command-output-v1"
    );
    assert_eq!(
        output.report["plan_executor"]["command_output_core"]["selected_step_count"],
        2
    );
    assert_eq!(
        output.executor_report["executor"],
        "cpu-plan-root-scenario-command-output-v1"
    );
}

// test: scenario_events_command_output_assembles_report_and_executor_core
#[test]
fn scenario_events_command_output_assembles_report_and_executor_core() {
    let output = assemble_scenario_events_command_output(ScenarioEventsCommandOutputInput {
        command_argv: vec![
            "target/debug/boon_cli".to_owned(),
            "run-plan-scenario-events".to_owned(),
            "examples/counter.bn".to_owned(),
        ],
        generated_at_utc: "2026-06-29T00:00:00Z".to_owned(),
        git_commit: "abc123".to_owned(),
        worktree_fingerprint: "worktreehash".to_owned(),
        binary_hash: "binhash".to_owned(),
        binary_path: "target/debug/boon_cli".to_owned(),
        source_path: "examples/counter.bn".to_owned(),
        source_hash: "sourcehash".to_owned(),
        source_files: vec!["examples/counter.bn".to_owned()],
        scenario_path: "examples/counter.scn".to_owned(),
        scenario_hash: "scenariohash".to_owned(),
        program_hash: "programhash".to_owned(),
        program_kind: "single-file".to_owned(),
        program_file_count: 1,
        graph_node_count: 3,
        load_pipeline_profile: json!({"total_ms": 1.0}),
        target_profile: "software-default".to_owned(),
        plan_hash: "planhash".to_owned(),
        plan_version: json!({"major": 1}),
        capability_summary: json!({"executable": true}),
        state_summary: json!({"store": {"count": 1}}),
        semantic_delta_signatures: vec!["FieldSet:store.count".to_owned()],
        semantic_deltas: json!([{"kind": "FieldSet"}]),
        plan_executor_coverage: json!({
            "selected_step_ids": ["increment", "inspect"],
            "covers_assertion_only_steps": true
        }),
        assertion_only_covered: true,
        plan_executor: json!({"executor": "cpu-plan-scenario-events-v1"}),
    });

    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.report["plan_executor_status"], "pass");
    assert!(output.report.get("accepted_for_product_status").is_none());
    assert!(output.report.get("comparison_status").is_none());
    assert_eq!(output.report["command"], "run-plan-scenario-events");
    assert_eq!(
        output.report["selected_step_ids"],
        json!(["increment", "inspect"])
    );
    assert_eq!(json!(output.command_argv), output.report["command_argv"]);
    assert_eq!(
        output.report["plan_executor"]["command_output_core"]["executor"],
        "cpu-plan-scenario-events-command-output-v1"
    );
    assert_eq!(
        output.report["plan_executor"]["command_output_core"]["selected_step_count"],
        2
    );
    assert_eq!(
        output.executor_report["executor"],
        "cpu-plan-scenario-events-command-output-v1"
    );
}

// test: scenario_events_report_without_compare_is_product_status
#[test]
fn scenario_events_report_without_compare_is_product_status() {
    let output = assemble_scenario_events_command_output(ScenarioEventsCommandOutputInput {
        command_argv: vec![
            "target/debug/boon_cli".to_owned(),
            "run-plan-scenario-events".to_owned(),
            "examples/counter.bn".to_owned(),
        ],
        generated_at_utc: "2026-06-29T00:00:00Z".to_owned(),
        git_commit: "abc123".to_owned(),
        worktree_fingerprint: "worktreehash".to_owned(),
        binary_hash: "binhash".to_owned(),
        binary_path: "target/debug/boon_cli".to_owned(),
        source_path: "examples/counter.bn".to_owned(),
        source_hash: "sourcehash".to_owned(),
        source_files: vec!["examples/counter.bn".to_owned()],
        scenario_path: "examples/counter.scn".to_owned(),
        scenario_hash: "scenariohash".to_owned(),
        program_hash: "programhash".to_owned(),
        program_kind: "single-file".to_owned(),
        program_file_count: 1,
        graph_node_count: 3,
        load_pipeline_profile: json!({"total_ms": 1.0}),
        target_profile: "software-default".to_owned(),
        plan_hash: "planhash".to_owned(),
        plan_version: json!({"major": 1}),
        capability_summary: json!({"executable": true}),
        state_summary: json!({"store": {"count": 1}}),
        semantic_delta_signatures: vec!["FieldSet:store.count".to_owned()],
        semantic_deltas: json!([{"kind": "FieldSet"}]),
        plan_executor_coverage: json!({
            "selected_step_ids": ["increment"],
            "covers_assertion_only_steps": true
        }),
        assertion_only_covered: true,
        plan_executor: json!({"executor": "cpu-plan-scenario-events-v1"}),
    });

    assert_eq!(output.report["status"], "pass");
    assert!(output.report.get("comparison_status").is_none());
    assert_eq!(
        output.report["report_status_basis"],
        "plan-executor-product-plus-assertion-coverage"
    );
    assert!(
        output
            .report
            .pointer("/command_report_assembly_core/compare_required_for_status")
            .is_none()
    );
    assert_eq!(output.report["per_step_pass_fail"][2]["pass"], true);
    assert_eq!(
        output.report["per_step_pass_fail"][2]["id"],
        "scenario-event-product-path-is-plan-executor-only"
    );
}

// test: source_route_command_argv_encodes_inline_bytes_and_non_default_target
#[test]
fn source_route_command_argv_encodes_inline_bytes_and_non_default_target() {
    let argv = build_source_route_command_argv(SourceRouteCommandArgvInput {
        current_args: vec!["xtask".to_owned()],
        source_path: "examples/bytes.bn".to_owned(),
        target_profile: "software-wasm".to_owned(),
        source_route: "store.input.change".to_owned(),
        target_state: "store.input".to_owned(),
        text: None,
        key: None,
        address: None,
        payload: BTreeMap::new(),
        payload_bytes: BTreeMap::from([("bytes".to_owned(), vec![0, 1, 2, 255])]),
        payload_byte_artifact_paths: BTreeMap::new(),
        report_path: None,
    });
    assert_eq!(
        argv,
        vec![
            "target/debug/boon_cli",
            "run-plan-route",
            "examples/bytes.bn",
            "--source",
            "store.input.change",
            "--target-state",
            "store.input",
            "--payload-bytes-hex",
            "bytes=000102ff",
            "--target",
            "software-wasm",
        ]
    );
}

// test: source_route_execution_surface_is_executor_owned
#[test]
fn source_route_execution_surface_is_executor_owned() {
    let mut execution = SourceRouteJsonExecution {
        plan_hash: "plan".to_owned(),
        source_label: "store.input.change".to_owned(),
        source_id: SourceId(2),
        target_state_label: "store.input".to_owned(),
        target_state_id: StateId(3),
        update_op_id: PlanOpId(4),
        supported: true,
        skipped_by_guard: false,
        unsupported_reason: None,
        value: Some(json!("hello")),
        state_summary: json!({ "store.input": "hello" }),
        semantic_delta_signatures: Vec::new(),
        semantic_deltas: Vec::new(),
        expression_kind: Some("source_payload_text"),
        source_payload_field: json!("Text"),
        update_constant_id: JsonValue::Null,
        update_constant_value: JsonValue::Null,
        executor_report: json!({}),
    };

    let scalar_surface = select_source_route_execution_surface(&execution)
        .expect("scalar JSON execution should classify");
    assert_eq!(
        scalar_surface.kind,
        SourceRouteExecutionSurfaceKind::PlanJson
    );
    assert_eq!(
        scalar_surface.executor_report["execution_surface"],
        "plan-json"
    );

    execution.value = Some(json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "abc",
        "byte_len": 3
    }));
    let bytes_surface =
        select_source_route_execution_surface(&execution).expect("BYTES execution should classify");
    assert_eq!(
        bytes_surface.kind,
        SourceRouteExecutionSurfaceKind::FullExecution
    );
    assert_eq!(
        bytes_surface.executor_report["execution_surface"],
        "full-execution"
    );
    assert_eq!(
        bytes_surface.executor_report["route_core_value_is_bytes"],
        true
    );

    execution.skipped_by_guard = true;
    let error = select_source_route_execution_surface(&execution)
        .expect_err("guard-skipped selected execution should be rejected");
    assert!(
        error
            .to_string()
            .contains("source guard did not match the supplied event"),
        "unexpected error: {error}"
    );
}

// test: source_route_orchestration_is_executor_owned
#[test]
fn source_route_orchestration_is_executor_owned() {
    let source_id = SourceId(2);
    let state_id = StateId(3);
    let update_op_id = PlanOpId(4);
    let plan = MachinePlan {
        version: boon_plan::PlanVersion::default(),
        target_profile: boon_plan::TargetProfile::SoftwareDefault,
        constants: vec![boon_plan::PlanConstant {
            id: PlanConstantId(0),
            value: PlanConstantValue::Text {
                value: "".to_owned(),
            },
        }],
        source_routes: vec![SourceRoute {
            id: boon_plan::PlanSourceRouteId(0),
            source_id,
            path: "store.input.change".to_owned(),
            scoped: false,
            scope_id: None,
            payload_schema: boon_plan::SourcePayloadSchema {
                fields: vec![SourcePayloadField::Text],
                typed_fields: vec![boon_plan::SourcePayloadDescriptor {
                    field: SourcePayloadField::Text,
                    value_type: boon_plan::SourcePayloadValueType::Text,
                }],
                row_lookup_field: None,
            },
        }],
        storage_layout: boon_plan::StorageLayout {
            scalar_slots: vec![boon_plan::ScalarStorageSlot {
                id: boon_plan::PlanStorageId(1),
                state_id,
                value_type: PlanValueType::Text,
                scope_id: None,
                indexed: false,
                initial_value_kind: InitialValueKind::Text,
                initial_constant_id: Some(PlanConstantId(0)),
                initial_root_field_path: None,
                initial_row_field_path: None,
            }],
            list_slots: Vec::new(),
            byte_banks: Vec::new(),
        },
        regions: vec![
            boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(1),
                kind: RegionKind::SourceRouting,
                ops: vec![PlanOp {
                    id: PlanOpId(1),
                    kind: PlanOpKind::SourceRoute,
                    inputs: Vec::new(),
                    output: Some(ValueRef::Source(source_id)),
                    indexed: false,
                    unresolved_executable_ref_count: 0,
                }],
            },
            boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(2),
                kind: RegionKind::StateInitialization,
                ops: vec![PlanOp {
                    id: PlanOpId(2),
                    kind: PlanOpKind::StateInitialize {
                        initial_value_kind: InitialValueKind::Text,
                        initial_constant_id: Some(PlanConstantId(0)),
                    },
                    inputs: Vec::new(),
                    output: Some(ValueRef::State(state_id)),
                    indexed: false,
                    unresolved_executable_ref_count: 0,
                }],
            },
            boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(3),
                kind: RegionKind::UpdateBranches,
                ops: vec![PlanOp {
                    id: update_op_id,
                    kind: PlanOpKind::UpdateBranch {
                        expression_kind: PlanExpressionKind::SourcePayload,
                        ordered_inputs: Vec::new(),
                        source_payload_field: Some(SourcePayloadField::Text),
                        update_constant_id: None,
                        source_guard: None,
                    },
                    inputs: vec![
                        ValueRef::Source(source_id),
                        ValueRef::SourcePayload {
                            source_id,
                            field: SourcePayloadField::Text,
                        },
                    ],
                    output: Some(ValueRef::State(state_id)),
                    indexed: false,
                    unresolved_executable_ref_count: 0,
                }],
            },
        ],
        dirty_plan: boon_plan::DirtyPlan {
            dependency_edges: 0,
            unresolved_dependency_edges: 0,
        },
        commit_plan: boon_plan::CommitPlan {
            update_branch_count: 1,
            unresolved_update_branch_count: 0,
        },
        delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
        capability_summary: boon_plan::CapabilitySummary {
            executable: true,
            typed_lowering_executable: true,
            cpu_plan_executor_complete: true,
            constant_count: 1,
            source_route_count: 1,
            scalar_storage_count: 1,
            list_storage_count: 0,
            byte_bank_storage_count: 0,
            operation_count: 3,
            typed_value_ref_count: 5,
            executable_string_path_count: 0,
            unresolved_executable_ref_count: 0,
            unknown_plan_op_count: 0,
            cpu_plan_executor_unsupported_op_count: 0,
            runtime_ast_dependency_count: 0,
            graph_rebuild_count: 0,
            graph_clones_per_item: 0,
        },
        debug_map: boon_plan::DebugMap {
            source_units: Vec::new(),
            source_routes: vec![boon_plan::DebugEntry {
                id: "source:2".to_owned(),
                label: "store.input.change".to_owned(),
            }],
            state_slots: vec![boon_plan::DebugEntry {
                id: "state:3".to_owned(),
                label: "store.input".to_owned(),
            }],
            list_slots: Vec::new(),
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };
    let event = RootJsonSourceEvent {
        text: Some("hello".to_owned()),
        ..RootJsonSourceEvent::default()
    };
    let verification = verify_plan(&plan).expect("test plan should verify");
    assert_eq!(
        verification.status,
        "pass",
        "test plan verification failed: {:?}",
        verification
            .checks
            .iter()
            .filter(|check| !check.pass)
            .collect::<Vec<_>>()
    );
    let output = execute_source_route_with_full_execution(
        &plan,
        "store.input.change",
        "store.input",
        &event,
        || {
            Ok(SourceRouteFullExecution {
                state_summary: json!({ "store.input": "hello" }),
                semantic_delta_signatures: vec!["FieldSet:store.input".to_owned()],
                semantic_deltas: json!([{
                    "kind": "FieldSet",
                    "field_path": "store.input",
                    "value": "hello"
                }]),
                per_step: Vec::new(),
                executor_report: json!({ "executor": "test-full-execution" }),
            })
        },
    )
    .expect("source-route orchestration should execute through PlanExecutor");

    assert_eq!(output.value, json!("hello"));
    assert_eq!(output.state_summary, json!({ "store.input": "hello" }));
    assert_eq!(output.route_surface["expression_kind"], "source_payload");
    assert_eq!(
        output.route_surface["route_execution_core"]["execution_surface_core"]["execution_surface"],
        "plan-json"
    );
    assert_eq!(
        output.executor_report["executor"],
        "cpu-plan-source-route-v1"
    );
}

