// Included by `../source_route_reports.rs`.

// test: indexed_fixed_byte_bank_lookup_is_executor_owned
#[test]
fn indexed_fixed_byte_bank_lookup_is_executor_owned() {
    let scope_id = boon_plan::ScopeId(17);
    let state_id = StateId(42);
    let plan = MachinePlan {
        version: boon_plan::PlanVersion::default(),
        target_profile: boon_plan::TargetProfile::SoftwareDefault,
        constants: Vec::new(),
        source_routes: Vec::new(),
        storage_layout: boon_plan::StorageLayout {
            scalar_slots: vec![boon_plan::ScalarStorageSlot {
                id: boon_plan::PlanStorageId(1),
                state_id,
                value_type: PlanValueType::Bytes { fixed_len: Some(3) },
                scope_id: Some(scope_id),
                indexed: true,
                initial_value_kind: InitialValueKind::Bytes,
                initial_constant_id: None,
                initial_root_field_path: None,
                initial_row_field_path: None,
            }],
            list_slots: vec![boon_plan::ListStorageSlot {
                id: boon_plan::PlanStorageId(2),
                list_id: boon_plan::ListId(3),
                scope_id: Some(scope_id),
                row_field_ids: Vec::new(),
                capacity: None,
                hidden_key_type: "RowKey".to_owned(),
                has_generation: true,
                initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
                range: None,
                initial_rows: Vec::new(),
            }],
            byte_banks: vec![boon_plan::ByteStorageBank {
                id: boon_plan::PlanStorageId(4),
                state_storage_id: boon_plan::PlanStorageId(1),
                state_id,
                scope_id: Some(scope_id),
                indexed: true,
                fixed_len: 3,
                capacity: None,
            }],
        },
        regions: Vec::new(),
        dirty_plan: boon_plan::DirtyPlan {
            dependency_edges: 0,
            unresolved_dependency_edges: 0,
        },
        commit_plan: boon_plan::CommitPlan {
            update_branch_count: 0,
            unresolved_update_branch_count: 0,
        },
        delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
        capability_summary: boon_plan::CapabilitySummary {
            executable: true,
            typed_lowering_executable: true,
            cpu_plan_executor_complete: false,
            constant_count: 0,
            source_route_count: 0,
            scalar_storage_count: 1,
            list_storage_count: 1,
            byte_bank_storage_count: 1,
            operation_count: 0,
            typed_value_ref_count: 0,
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
            source_routes: Vec::new(),
            state_slots: vec![boon_plan::DebugEntry {
                id: "state:42".to_owned(),
                label: "row.payload".to_owned(),
            }],
            list_slots: Vec::new(),
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };

    assert!(indexed_state_has_fixed_byte_bank(&plan, state_id));
    assert_eq!(
        indexed_fixed_byte_bank_len(&plan, state_id).expect("fixed bank length should validate"),
        Some(3)
    );
    assert!(indexed_field_has_fixed_byte_bank(
        &plan,
        Some(scope_id),
        "payload"
    ));
    assert!(!indexed_field_has_fixed_byte_bank(
        &plan,
        Some(scope_id),
        "other"
    ));
    assert!(!indexed_field_has_fixed_byte_bank(&plan, None, "payload"));
}

// test: indexed_update_batch_execution_is_executor_owned
#[test]
fn indexed_update_batch_execution_is_executor_owned() {
    let scope_id = boon_plan::ScopeId(3);
    let mut plan = empty_executor_test_plan();
    plan.source_routes = vec![SourceRoute {
        id: boon_plan::PlanSourceRouteId(1),
        source_id: SourceId(1),
        path: "store.toggle_all".to_owned(),
        scoped: false,
        scope_id: None,
        payload_schema: boon_plan::SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
            row_lookup_field: None,
        },
    }];
    plan.storage_layout.list_slots = vec![boon_plan::ListStorageSlot {
        id: boon_plan::PlanStorageId(0),
        list_id: boon_plan::ListId(7),
        scope_id: Some(scope_id),
        row_field_ids: Vec::new(),
        capacity: None,
        hidden_key_type: "u64".to_owned(),
        has_generation: true,
        initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: Vec::new(),
    }];
    plan.storage_layout.scalar_slots = vec![boon_plan::ScalarStorageSlot {
        id: boon_plan::PlanStorageId(1),
        state_id: StateId(30),
        value_type: PlanValueType::Bool,
        scope_id: Some(scope_id),
        indexed: true,
        initial_value_kind: InitialValueKind::Bool,
        initial_constant_id: None,
        initial_root_field_path: None,
        initial_row_field_path: None,
    }];
    plan.debug_map.list_slots = vec![boon_plan::DebugEntry {
        id: "list:7".to_owned(),
        label: "todos".to_owned(),
    }];
    let op = PlanOp {
        id: PlanOpId(12),
        kind: PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::Const,
            ordered_inputs: Vec::new(),
            source_payload_field: None,
            update_constant_id: None,
            source_guard: None,
        },
        inputs: Vec::new(),
        output: Some(ValueRef::State(StateId(30))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let list_rows = BTreeMap::from([(
        7usize,
        vec![
            PlanExecutorListRow {
                key: 1,
                generation: 1,
                fields: BTreeMap::new(),
            },
            PlanExecutorListRow {
                key: 2,
                generation: 1,
                fields: BTreeMap::new(),
            },
        ],
    )]);
    let event = IndexedUpdateTargetEvent {
        source: "store.toggle_all".to_owned(),
        target_text: Some("visible toggle label".to_owned()),
        ..IndexedUpdateTargetEvent::default()
    };
    let mut callback_targets = Vec::new();

    let execution = execute_indexed_update_batch_with(
        &plan,
        &op,
        &plan.source_routes[0],
        &event,
        &list_rows,
        |target| {
            let target = target.expect("unscoped source should bulk-target rows");
            callback_targets.push((target.list_label.clone(), target.key, target.generation));
            let primary_value = format!("primary-{}", target.key);
            let derived_value = format!("derived-{}", target.key);
            Ok(IndexedUpdateBranchExecution {
                semantic_deltas: vec![
                    json!({
                        "kind": "FieldSet",
                        "field_path": "completed",
                        "key": target.key,
                        "generation": target.generation,
                        "value": primary_value,
                    }),
                    json!({
                        "kind": "FieldSet",
                        "field_path": "visible",
                        "key": target.key,
                        "generation": target.generation,
                        "value": derived_value,
                    }),
                ],
                report_rows: vec![json!({
                    "update_op_id": 12,
                    "list": target.list_label,
                    "key": target.key,
                    "generation": target.generation,
                    "field_path": "completed",
                    "value": primary_value,
                })],
                updated_row_count: 1,
            })
        },
    )
    .expect("batch execution should succeed");

    assert_eq!(
        callback_targets,
        vec![("todos".to_owned(), 1, 1), ("todos".to_owned(), 2, 1)]
    );
    assert_eq!(execution.updated_row_count, 2);
    assert!(execution.bulk_indexed_update);
    assert_eq!(execution.report_rows.len(), 2);
    assert_eq!(
        execution.executor_report["executor"],
        "cpu-plan-indexed-update-batch-execution-v1"
    );
    let ordered = execution
        .semantic_deltas
        .iter()
        .map(|delta| {
            (
                delta["field_path"].as_str().unwrap().to_owned(),
                delta["key"].as_u64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ordered,
        vec![
            ("completed".to_owned(), 1),
            ("completed".to_owned(), 2),
            ("visible".to_owned(), 1),
            ("visible".to_owned(), 2),
        ]
    );
}

// test: indexed_json_update_evaluator_handles_bool_not_and_text_trim
#[test]
fn indexed_json_update_evaluator_handles_bool_not_and_text_trim() {
    let scope_id = boon_plan::ScopeId(3);
    let mut plan = empty_executor_test_plan();
    plan.source_routes = vec![SourceRoute {
        id: boon_plan::PlanSourceRouteId(1),
        source_id: SourceId(1),
        path: "todo.title.change".to_owned(),
        scoped: true,
        scope_id: Some(scope_id),
        payload_schema: boon_plan::SourcePayloadSchema {
            fields: vec![SourcePayloadField::Named("title".to_owned())],
            typed_fields: Vec::new(),
            row_lookup_field: None,
        },
    }];
    plan.storage_layout.scalar_slots = vec![
        boon_plan::ScalarStorageSlot {
            id: boon_plan::PlanStorageId(1),
            state_id: StateId(30),
            value_type: PlanValueType::Text,
            scope_id: Some(scope_id),
            indexed: true,
            initial_value_kind: InitialValueKind::Text,
            initial_constant_id: None,
            initial_root_field_path: None,
            initial_row_field_path: None,
        },
        boon_plan::ScalarStorageSlot {
            id: boon_plan::PlanStorageId(2),
            state_id: StateId(31),
            value_type: PlanValueType::Bool,
            scope_id: Some(scope_id),
            indexed: true,
            initial_value_kind: InitialValueKind::Bool,
            initial_constant_id: None,
            initial_root_field_path: None,
            initial_row_field_path: None,
        },
        boon_plan::ScalarStorageSlot {
            id: boon_plan::PlanStorageId(3),
            state_id: StateId(32),
            value_type: PlanValueType::Text,
            scope_id: Some(scope_id),
            indexed: true,
            initial_value_kind: InitialValueKind::Text,
            initial_constant_id: None,
            initial_root_field_path: None,
            initial_row_field_path: None,
        },
        boon_plan::ScalarStorageSlot {
            id: boon_plan::PlanStorageId(4),
            state_id: StateId(33),
            value_type: PlanValueType::Text,
            scope_id: Some(scope_id),
            indexed: true,
            initial_value_kind: InitialValueKind::Text,
            initial_constant_id: None,
            initial_root_field_path: None,
            initial_row_field_path: None,
        },
    ];
    plan.constants = vec![boon_plan::PlanConstant {
        id: PlanConstantId(0),
        value: PlanConstantValue::Text {
            value: "SKIP".to_owned(),
        },
    }];
    plan.debug_map.state_slots = vec![
        boon_plan::DebugEntry {
            id: "state:30".to_owned(),
            label: "todo.title".to_owned(),
        },
        boon_plan::DebugEntry {
            id: "state:31".to_owned(),
            label: "todo.completed".to_owned(),
        },
        boon_plan::DebugEntry {
            id: "state:32".to_owned(),
            label: "todo.edited_title".to_owned(),
        },
        boon_plan::DebugEntry {
            id: "state:33".to_owned(),
            label: "todo.edited_title.draft_title".to_owned(),
        },
    ];
    plan.debug_map.derived_values = vec![boon_plan::DebugEntry {
        id: "field:80".to_owned(),
        label: "store.all_completed".to_owned(),
    }];
    let row = PlanExecutorListRowState {
        key: 1,
        generation: 1,
        fields: BTreeMap::from([
            ("title".to_owned(), json!("Old")),
            ("completed".to_owned(), json!(false)),
            ("edited_title".to_owned(), json!("")),
            ("draft_title".to_owned(), json!("")),
        ]),
        private_bytes: BTreeMap::new(),
        fixed_bytes_banks: BTreeMap::new(),
    };
    let bool_op = PlanOp {
        id: PlanOpId(10),
        kind: PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BoolNot,
            ordered_inputs: Vec::new(),
            source_payload_field: None,
            update_constant_id: None,
            source_guard: None,
        },
        inputs: vec![ValueRef::Field(FieldId(80))],
        output: Some(ValueRef::State(StateId(31))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let root_derived_values = BTreeMap::from([(80usize, json!(true))]);
    let event = RootJsonSourceEvent::default();
    let bool_eval = evaluate_indexed_json_update_branch(
        &plan,
        &bool_op,
        SourceId(1),
        &plan.source_routes[0],
        &event,
        &row,
        &serde_json::Map::new(),
        &root_derived_values,
    )
    .expect("Bool/not evaluation should succeed");
    assert!(bool_eval.supported);
    assert_eq!(bool_eval.expression_kind, Some("bool_not"));
    assert_eq!(bool_eval.value, Some(json!(false)));
    assert_eq!(
        bool_eval.executor_report["executor"],
        "cpu-plan-indexed-json-update-evaluator-v1"
    );

    let text_op = PlanOp {
        id: PlanOpId(11),
        kind: PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::TextTrimOrPrevious,
            ordered_inputs: Vec::new(),
            source_payload_field: Some(SourcePayloadField::Named("title".to_owned())),
            update_constant_id: None,
            source_guard: None,
        },
        inputs: vec![ValueRef::SourcePayload {
            source_id: SourceId(1),
            field: SourcePayloadField::Named("title".to_owned()),
        }],
        output: Some(ValueRef::State(StateId(30))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let event = RootJsonSourceEvent {
        payload: BTreeMap::from([("title".to_owned(), "  New title  ".to_owned())]),
        ..RootJsonSourceEvent::default()
    };
    let text_eval = evaluate_indexed_json_update_branch(
        &plan,
        &text_op,
        SourceId(1),
        &plan.source_routes[0],
        &event,
        &row,
        &serde_json::Map::new(),
        &BTreeMap::new(),
    )
    .expect("TextTrimOrPrevious evaluation should succeed");
    assert!(text_eval.supported);
    assert_eq!(text_eval.expression_kind, Some("text_trim_or_previous"));
    assert_eq!(text_eval.value, Some(json!("New title")));
    assert_eq!(
        text_eval.source_payload_field,
        serde_json::to_value(SourcePayloadField::Named("title".to_owned())).unwrap()
    );

    plan.storage_layout.scalar_slots[0].initial_row_field_path = Some("title".to_owned());
    let read_path_op = PlanOp {
        id: PlanOpId(13),
        kind: PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::ReadPath,
            ordered_inputs: Vec::new(),
            source_payload_field: None,
            update_constant_id: None,
            source_guard: None,
        },
        inputs: Vec::new(),
        output: Some(ValueRef::State(StateId(30))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let read_path_eval = evaluate_indexed_json_update_branch(
        &plan,
        &read_path_op,
        SourceId(1),
        &plan.source_routes[0],
        &RootJsonSourceEvent::default(),
        &row,
        &serde_json::Map::new(),
        &BTreeMap::new(),
    )
    .expect("indexed ReadPath should read the output row initializer field");
    assert!(read_path_eval.supported);
    assert_eq!(read_path_eval.expression_kind, Some("read_path"));
    assert_eq!(read_path_eval.value, Some(json!("Old")));

    let match_op = PlanOp {
        id: PlanOpId(12),
        kind: PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::MatchTextIsEmptyConst,
            ordered_inputs: vec![
                ValueRef::State(StateId(33)),
                ValueRef::State(StateId(30)),
                ValueRef::Constant(PlanConstantId(0)),
            ],
            source_payload_field: None,
            update_constant_id: None,
            source_guard: None,
        },
        inputs: vec![
            ValueRef::State(StateId(33)),
            ValueRef::State(StateId(30)),
            ValueRef::Constant(PlanConstantId(0)),
        ],
        output: Some(ValueRef::State(StateId(32))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let match_eval = evaluate_indexed_json_update_branch(
        &plan,
        &match_op,
        SourceId(1),
        &plan.source_routes[0],
        &RootJsonSourceEvent::default(),
        &row,
        &serde_json::Map::new(),
        &BTreeMap::new(),
    )
    .expect("MatchTextIsEmptyConst evaluation should succeed");
    assert!(match_eval.supported);
    assert_eq!(
        match_eval.expression_kind,
        Some("match_text_is_empty_const")
    );
    assert_eq!(match_eval.value, Some(json!("Old")));

    let mut non_empty_row = row.clone();
    non_empty_row
        .fields
        .insert("draft_title".to_owned(), json!("Draft"));
    let skip_eval = evaluate_indexed_json_update_branch(
        &plan,
        &match_op,
        SourceId(1),
        &plan.source_routes[0],
        &RootJsonSourceEvent::default(),
        &non_empty_row,
        &serde_json::Map::new(),
        &BTreeMap::new(),
    )
    .expect("MatchTextIsEmptyConst SKIP evaluation should succeed");
    assert!(skip_eval.supported);
    assert_eq!(skip_eval.expression_kind, Some("match_text_is_empty_const"));
    assert_eq!(skip_eval.value, None);
}

// test: indexed_update_target_selection_is_executor_owned
#[test]
fn indexed_update_target_selection_is_executor_owned() {
    let scope_id = boon_plan::ScopeId(7);
    let output_state_id = StateId(12);
    let source_route = SourceRoute {
        id: boon_plan::PlanSourceRouteId(1),
        source_id: SourceId(3),
        path: "rows.toggle".to_owned(),
        scoped: false,
        scope_id: None,
        payload_schema: boon_plan::SourcePayloadSchema {
            fields: vec![SourcePayloadField::Text],
            typed_fields: Vec::new(),
            row_lookup_field: None,
        },
    };
    let op = PlanOp {
        id: PlanOpId(9),
        kind: PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BoolNot,
            ordered_inputs: Vec::new(),
            source_payload_field: None,
            update_constant_id: None,
            source_guard: None,
        },
        inputs: Vec::new(),
        output: Some(ValueRef::State(output_state_id)),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let plan = MachinePlan {
        version: boon_plan::PlanVersion::default(),
        target_profile: boon_plan::TargetProfile::SoftwareDefault,
        constants: Vec::new(),
        source_routes: vec![source_route.clone()],
        storage_layout: boon_plan::StorageLayout {
            scalar_slots: vec![boon_plan::ScalarStorageSlot {
                id: boon_plan::PlanStorageId(1),
                state_id: output_state_id,
                value_type: PlanValueType::Bool,
                scope_id: Some(scope_id),
                indexed: true,
                initial_value_kind: InitialValueKind::Bool,
                initial_constant_id: None,
                initial_root_field_path: None,
                initial_row_field_path: None,
            }],
            list_slots: vec![boon_plan::ListStorageSlot {
                id: boon_plan::PlanStorageId(2),
                list_id: boon_plan::ListId(5),
                scope_id: Some(scope_id),
                row_field_ids: Vec::new(),
                capacity: None,
                hidden_key_type: "u64".to_owned(),
                has_generation: true,
                initializer_kind: boon_plan::ListInitializerKind::Empty,
                range: None,
                initial_rows: Vec::new(),
            }],
            byte_banks: Vec::new(),
        },
        regions: Vec::new(),
        dirty_plan: boon_plan::DirtyPlan {
            dependency_edges: 0,
            unresolved_dependency_edges: 0,
        },
        commit_plan: boon_plan::CommitPlan {
            update_branch_count: 0,
            unresolved_update_branch_count: 0,
        },
        delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
        capability_summary: boon_plan::CapabilitySummary {
            executable: true,
            typed_lowering_executable: true,
            cpu_plan_executor_complete: false,
            constant_count: 0,
            source_route_count: 1,
            scalar_storage_count: 1,
            list_storage_count: 1,
            byte_bank_storage_count: 0,
            operation_count: 1,
            typed_value_ref_count: 0,
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
            source_routes: Vec::new(),
            state_slots: Vec::new(),
            list_slots: vec![boon_plan::DebugEntry {
                id: "list:5".to_owned(),
                label: "rows".to_owned(),
            }],
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };
    let list_rows = BTreeMap::from([(
        5,
        vec![
            PlanExecutorListRow {
                key: 4,
                generation: 1,
                fields: BTreeMap::new(),
            },
            PlanExecutorListRow {
                key: 9,
                generation: 2,
                fields: BTreeMap::new(),
            },
        ],
    )]);

    let selection = select_unscoped_indexed_update_targets(
        &plan,
        &op,
        &source_route,
        &IndexedUpdateTargetEvent {
            source: "rows.toggle".to_owned(),
            ..IndexedUpdateTargetEvent::default()
        },
        &list_rows,
    )
    .expect("unscoped indexed update should fan out through executor-owned target selection");
    assert!(selection.bulk_indexed_update);
    assert_eq!(selection.list_id, Some(5));
    assert_eq!(selection.list_label.as_deref(), Some("rows"));
    assert_eq!(
        selection.targets,
        vec![
            IndexedUpdateTargetRow {
                key: 4,
                generation: 1,
            },
            IndexedUpdateTargetRow {
                key: 9,
                generation: 2,
            },
        ]
    );
    assert_eq!(
        selection.executor_report["executor"],
        "cpu-plan-indexed-update-target-selection-v1"
    );
    assert_eq!(selection.executor_report["target_count"], 2);

    let targeted = select_unscoped_indexed_update_targets(
        &plan,
        &op,
        &source_route,
        &IndexedUpdateTargetEvent {
            source: "rows.toggle".to_owned(),
            target_key: Some(4),
            ..IndexedUpdateTargetEvent::default()
        },
        &list_rows,
    )
    .expect("already targeted events should skip bulk fanout");
    assert!(!targeted.bulk_indexed_update);
    assert_eq!(targeted.executor_report["skip_reason"], "event-target-key");

    let wrong_list = select_unscoped_indexed_update_targets(
        &plan,
        &op,
        &source_route,
        &IndexedUpdateTargetEvent {
            source: "rows.toggle".to_owned(),
            list_id: Some("other_rows".to_owned()),
            ..IndexedUpdateTargetEvent::default()
        },
        &list_rows,
    )
    .expect_err("wrong event list should be rejected by executor target selection");
    assert!(
        wrong_list.to_string().contains("expected `rows`"),
        "unexpected error: {wrong_list}"
    );
}
