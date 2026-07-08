// Included by `../source_route_reports.rs`.

// test: list_remove_predicate_evaluation_is_executor_owned
#[test]
fn list_remove_predicate_evaluation_is_executor_owned() {
    let state_id = StateId(5);
    let plan = MachinePlan {
        version: boon_plan::PlanVersion::default(),
        target_profile: boon_plan::TargetProfile::SoftwareDefault,
        constants: Vec::new(),
        source_routes: Vec::new(),
        storage_layout: boon_plan::StorageLayout {
            scalar_slots: Vec::new(),
            list_slots: Vec::new(),
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
            source_route_count: 0,
            scalar_storage_count: 0,
            list_storage_count: 0,
            byte_bank_storage_count: 0,
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
                id: "state:5".to_owned(),
                label: "row.done".to_owned(),
            }],
            list_slots: Vec::new(),
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };
    let row = PlanExecutorListRow {
        key: 11,
        generation: 2,
        fields: BTreeMap::from([("done".to_owned(), json!(true))]),
    };
    let predicate = boon_plan::PlanListRemovePredicate::RowFieldBool {
        input: ValueRef::State(state_id),
    };
    let evaluation = evaluate_list_remove_predicate(&plan, &predicate, &row)
        .expect("row-field bool predicate should evaluate in executor");
    assert!(evaluation.matches);
    assert_eq!(
        evaluation.executor_report["executor"],
        "cpu-plan-list-remove-predicate-evaluator-v1"
    );
    assert_eq!(evaluation.executor_report["key"], 11);

    let report = build_list_remove_predicate_row_resolution_report(&plan, &predicate, 3, &row)
        .expect("predicate row-resolution report should be executor-owned");
    assert_eq!(
        report["executor"],
        "cpu-plan-list-remove-predicate-row-resolution-v1"
    );
    assert_eq!(report["predicate"], "row_field_bool");
    assert_eq!(report["predicate_field"], "done");
    assert_eq!(report["row_index"], 3);

    let not_predicate = boon_plan::PlanListRemovePredicate::RowFieldBoolNot {
        input: ValueRef::State(state_id),
    };
    let not_evaluation = evaluate_list_remove_predicate(&plan, &not_predicate, &row)
        .expect("row-field bool-not predicate should evaluate in executor");
    assert!(!not_evaluation.matches);
}

// test: list_append_value_resolution_is_executor_owned
#[test]
fn list_append_value_resolution_is_executor_owned() {
    let plan = MachinePlan {
        version: boon_plan::PlanVersion::default(),
        target_profile: boon_plan::TargetProfile::SoftwareDefault,
        constants: Vec::new(),
        source_routes: Vec::new(),
        storage_layout: boon_plan::StorageLayout {
            scalar_slots: Vec::new(),
            list_slots: Vec::new(),
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
            source_route_count: 0,
            scalar_storage_count: 0,
            list_storage_count: 0,
            byte_bank_storage_count: 0,
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
                id: "state:8".to_owned(),
                label: "todo.title".to_owned(),
            }],
            list_slots: Vec::new(),
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };
    let derived_values = BTreeMap::from([(FieldId(2), json!("derived title"))]);
    let row_fields = BTreeMap::from([("title".to_owned(), json!("row title"))]);

    assert_eq!(
        resolve_plan_value_ref(
            &plan,
            &ValueRef::Field(FieldId(2)),
            &derived_values,
            Some(&row_fields),
        )
        .expect("field ref should resolve"),
        Some(json!("derived title"))
    );
    assert_eq!(
        resolve_plan_value_ref(
            &plan,
            &ValueRef::State(StateId(8)),
            &derived_values,
            Some(&row_fields),
        )
        .expect("state ref should resolve from row fields"),
        Some(json!("row title"))
    );

    let bytes_constant = boon_plan::PlanConstant {
        id: PlanConstantId(9),
        value: PlanConstantValue::Bytes {
            byte_len: 3,
            sha256: sha256_bytes(&[1, 2, 3]),
            inline_bytes: Some(vec![1, 2, 3]),
        },
    };
    let bytes_json =
        plan_constant_json_value(&bytes_constant).expect("BYTES constant should report JSON");
    assert_eq!(bytes_json["$boon_type"], "BYTES");
    assert_eq!(bytes_json["byte_len"], 3);
}

// test: list_row_default_fields_are_executor_owned
#[test]
fn list_row_default_fields_are_executor_owned() {
    let scope_id = boon_plan::ScopeId(23);
    let text_state_id = StateId(11);
    let bytes_state_id = StateId(12);
    let list_slot = boon_plan::ListStorageSlot {
        id: boon_plan::PlanStorageId(1),
        list_id: boon_plan::ListId(7),
        scope_id: Some(scope_id),
        row_field_ids: Vec::new(),
        capacity: None,
        hidden_key_type: "RowKey".to_owned(),
        has_generation: true,
        initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: Vec::new(),
    };
    let plan = MachinePlan {
        version: boon_plan::PlanVersion::default(),
        target_profile: boon_plan::TargetProfile::SoftwareDefault,
        constants: vec![
            boon_plan::PlanConstant {
                id: PlanConstantId(1),
                value: PlanConstantValue::Text {
                    value: "hello".to_owned(),
                },
            },
            boon_plan::PlanConstant {
                id: PlanConstantId(2),
                value: PlanConstantValue::Bytes {
                    byte_len: 3,
                    sha256: sha256_bytes(&[1, 2, 3]),
                    inline_bytes: Some(vec![1, 2, 3]),
                },
            },
        ],
        source_routes: Vec::new(),
        storage_layout: boon_plan::StorageLayout {
            scalar_slots: vec![
                boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(2),
                    state_id: text_state_id,
                    value_type: PlanValueType::Text,
                    scope_id: Some(scope_id),
                    indexed: true,
                    initial_value_kind: InitialValueKind::Text,
                    initial_constant_id: Some(PlanConstantId(1)),
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                },
                boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(3),
                    state_id: bytes_state_id,
                    value_type: PlanValueType::Bytes { fixed_len: Some(3) },
                    scope_id: Some(scope_id),
                    indexed: true,
                    initial_value_kind: InitialValueKind::Bytes,
                    initial_constant_id: Some(PlanConstantId(2)),
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                },
            ],
            list_slots: vec![list_slot.clone()],
            byte_banks: vec![boon_plan::ByteStorageBank {
                id: boon_plan::PlanStorageId(4),
                state_storage_id: boon_plan::PlanStorageId(3),
                state_id: bytes_state_id,
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
            constant_count: 2,
            source_route_count: 0,
            scalar_storage_count: 2,
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
            state_slots: vec![
                boon_plan::DebugEntry {
                    id: "state:11".to_owned(),
                    label: "row.title".to_owned(),
                },
                boon_plan::DebugEntry {
                    id: "state:12".to_owned(),
                    label: "row.payload".to_owned(),
                },
            ],
            list_slots: Vec::new(),
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };

    let defaults = list_row_default_fields(&plan, &list_slot)
        .expect("row default fields should be assembled by executor");
    assert_eq!(defaults.fields["title"], json!("hello"));
    assert_eq!(defaults.fields["payload"]["$boon_type"], "BYTES");
    assert_eq!(defaults.private_bytes["payload"].inline_bytes(), &[1, 2, 3]);
    assert_eq!(defaults.fixed_byte_banks["payload"], vec![1, 2, 3]);
    assert_eq!(
        defaults.executor_report["executor"],
        "cpu-plan-list-row-default-fields-v1"
    );
    assert_eq!(defaults.executor_report["default_field_count"], 2);
    assert_eq!(defaults.executor_report["fixed_byte_bank_count"], 1);
}

// test: list_append_insert_and_row_refresh_deltas_are_executor_owned
#[test]
fn list_append_insert_and_row_refresh_deltas_are_executor_owned() {
    let list_slot = boon_plan::ListStorageSlot {
        id: boon_plan::PlanStorageId(0),
        list_id: boon_plan::ListId(7),
        scope_id: Some(boon_plan::ScopeId(3)),
        row_field_ids: vec![FieldId(8), FieldId(9)],
        capacity: None,
        hidden_key_type: "u64".to_owned(),
        has_generation: true,
        initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: Vec::new(),
    };
    let plan = MachinePlan {
        version: boon_plan::PlanVersion::default(),
        target_profile: boon_plan::TargetProfile::SoftwareDefault,
        constants: Vec::new(),
        source_routes: vec![SourceRoute {
            id: boon_plan::PlanSourceRouteId(1),
            source_id: SourceId(1),
            path: "store.submit".to_owned(),
            scoped: false,
            scope_id: None,
            payload_schema: boon_plan::SourcePayloadSchema {
                fields: vec![SourcePayloadField::Text, SourcePayloadField::Key],
                typed_fields: Vec::new(),
                row_lookup_field: None,
            },
        }],
        storage_layout: boon_plan::StorageLayout {
            scalar_slots: vec![boon_plan::ScalarStorageSlot {
                id: boon_plan::PlanStorageId(1),
                state_id: StateId(30),
                value_type: boon_plan::PlanValueType::Text,
                scope_id: Some(boon_plan::ScopeId(3)),
                indexed: true,
                initial_value_kind: boon_plan::InitialValueKind::Text,
                initial_constant_id: None,
                initial_root_field_path: None,
                initial_row_field_path: None,
            }],
            list_slots: vec![list_slot.clone()],
            byte_banks: Vec::new(),
        },
        regions: vec![
            boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(1),
                kind: RegionKind::ListOperations,
                ops: vec![PlanOp {
                    id: PlanOpId(10),
                    kind: PlanOpKind::ListOperation {
                        operation_kind: PlanListOperationKind::Append,
                        append: Some(boon_plan::PlanListAppend {
                            trigger: ValueRef::Field(FieldId(40)),
                            fields: vec![boon_plan::PlanListAppendField {
                                name: "title".to_owned(),
                                field_id: Some(FieldId(8)),
                                value_ref: Some(ValueRef::Field(FieldId(40))),
                                constant_id: None,
                            }],
                        }),
                        remove: None,
                        retain: None,
                        count: None,
                    },
                    inputs: Vec::new(),
                    output: Some(ValueRef::List(boon_plan::ListId(7))),
                    indexed: false,
                    unresolved_executable_ref_count: 0,
                }],
            },
            boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(2),
                kind: RegionKind::DerivedEvaluation,
                ops: vec![PlanOp {
                    id: PlanOpId(11),
                    kind: PlanOpKind::DerivedValue {
                        derived_kind: boon_plan::PlanDerivedKind::Pure,
                        startup_recompute: true,
                        expression: Some(PlanDerivedExpression::RowExpression {
                            expression: boon_plan::PlanRowExpression::Field {
                                input: ValueRef::State(StateId(30)),
                            },
                        }),
                    },
                    inputs: Vec::new(),
                    output: Some(ValueRef::Field(FieldId(9))),
                    indexed: true,
                    unresolved_executable_ref_count: 0,
                }],
            },
        ],
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
            byte_bank_storage_count: 0,
            operation_count: 2,
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
                id: "state:30".to_owned(),
                label: "todo.title".to_owned(),
            }],
            list_slots: vec![boon_plan::DebugEntry {
                id: "list:7".to_owned(),
                label: "todos".to_owned(),
            }],
            derived_values: Vec::new(),
            fields: vec![
                boon_plan::DebugEntry {
                    id: "field:8".to_owned(),
                    label: "todo.title".to_owned(),
                },
                boon_plan::DebugEntry {
                    id: "field:9".to_owned(),
                    label: "todo.normalized_title".to_owned(),
                },
            ],
            unresolved_executable_refs: Vec::new(),
        },
    };

    let insert = build_list_insert_delta("todos", 4, 1, json!("Write tests"));
    assert_eq!(insert["kind"], "ListInsert");
    assert_eq!(insert["list_id"], "todos");
    assert_eq!(insert["key"], 4);
    assert_eq!(insert["value"], "Write tests");

    let fields = row_expression_output_field_names(&plan, &list_slot);
    assert!(fields.contains("normalized_title"));
    assert!(!fields.contains("title"));

    let before = BTreeMap::from([
        ("title".to_owned(), json!("Write tests")),
        ("normalized_title".to_owned(), json!("old")),
    ]);
    let after = BTreeMap::from([
        ("title".to_owned(), json!("Changed but not row expression")),
        ("normalized_title".to_owned(), json!("Write tests")),
    ]);
    let deltas = build_row_refresh_field_deltas(&plan, &list_slot, "todos", 4, 1, &before, &after);
    assert_eq!(deltas.len(), 1);
    assert_eq!(deltas[0]["kind"], "FieldSet");
    assert_eq!(deltas[0]["list_id"], "todos");
    assert_eq!(deltas[0]["field_path"], "normalized_title");
    assert_eq!(deltas[0]["value"], "Write tests");
}

// test: list_row_expression_refresh_loop_is_executor_owned
#[test]
fn list_row_expression_refresh_loop_is_executor_owned() {
    let scope_id = boon_plan::ScopeId(3);
    let list_slot = boon_plan::ListStorageSlot {
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
    };
    let mut plan = empty_executor_test_plan();
    plan.source_routes = vec![SourceRoute {
        id: boon_plan::PlanSourceRouteId(1),
        source_id: SourceId(1),
        path: "todo.title.change".to_owned(),
        scoped: true,
        scope_id: Some(scope_id),
        payload_schema: boon_plan::SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
            row_lookup_field: None,
        },
    }];
    plan.storage_layout.list_slots = vec![list_slot.clone()];
    plan.storage_layout.scalar_slots = vec![boon_plan::ScalarStorageSlot {
        id: boon_plan::PlanStorageId(1),
        state_id: StateId(30),
        value_type: PlanValueType::Text,
        scope_id: Some(scope_id),
        indexed: true,
        initial_value_kind: InitialValueKind::Text,
        initial_constant_id: None,
        initial_root_field_path: None,
        initial_row_field_path: None,
    }];
    plan.debug_map.state_slots = vec![boon_plan::DebugEntry {
        id: "state:30".to_owned(),
        label: "todo.title".to_owned(),
    }];
    plan.debug_map.fields = vec![boon_plan::DebugEntry {
        id: "field:9".to_owned(),
        label: "todo.normalized_title".to_owned(),
    }];
    plan.regions = vec![boon_plan::OperationRegion {
        id: boon_plan::PlanRegionId(2),
        kind: RegionKind::DerivedEvaluation,
        ops: vec![PlanOp {
            id: PlanOpId(11),
            kind: PlanOpKind::DerivedValue {
                derived_kind: boon_plan::PlanDerivedKind::Pure,
                startup_recompute: true,
                expression: Some(PlanDerivedExpression::RowExpression {
                    expression: PlanRowExpression::Field {
                        input: ValueRef::State(StateId(30)),
                    },
                }),
            },
            inputs: Vec::new(),
            output: Some(ValueRef::Field(FieldId(9))),
            indexed: true,
            unresolved_executable_ref_count: 0,
        }],
    }];
    let mut row = PlanExecutorListRowState {
        key: 4,
        generation: 1,
        fields: BTreeMap::from([("title".to_owned(), json!("Write tests"))]),
        private_bytes: BTreeMap::new(),
        fixed_bytes_banks: BTreeMap::new(),
    };
    let list_state = BTreeMap::from([(7usize, vec![row.clone()])]);

    refresh_list_row_expression_fields_with(
        &plan,
        &list_slot,
        &list_state,
        &mut row,
        |plan, _list_state, row, expression| {
            let PlanRowExpression::Field {
                input: ValueRef::State(state_id),
            } = expression
            else {
                return Err("unexpected row expression".into());
            };
            let field_name = local_field_name(&state_label(plan, *state_id));
            row.fields
                .get(&field_name)
                .cloned()
                .ok_or_else(|| "missing row field".into())
        },
    )
    .expect("strict row-expression refresh should evaluate");
    assert_eq!(row.fields["normalized_title"], json!("Write tests"));

    row.fields.remove("normalized_title");
    refresh_list_row_expression_fields_best_effort_with(
        &plan,
        &list_slot,
        &list_state,
        &mut row,
        |_plan, _list_state, _row, _expression| Err("deferred expression".into()),
    );
    assert!(!row.fields.contains_key("normalized_title"));
}

// test: list_append_row_construction_is_executor_owned
#[test]
fn list_append_row_construction_is_executor_owned() {
    let scope_id = boon_plan::ScopeId(3);
    let list_slot = boon_plan::ListStorageSlot {
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
    };
    let append = boon_plan::PlanListAppend {
        trigger: ValueRef::Field(FieldId(40)),
        fields: vec![boon_plan::PlanListAppendField {
            name: "title".to_owned(),
            field_id: Some(FieldId(8)),
            value_ref: Some(ValueRef::Field(FieldId(40))),
            constant_id: None,
        }],
    };
    let mut plan = empty_executor_test_plan();
    plan.source_routes = vec![SourceRoute {
        id: boon_plan::PlanSourceRouteId(1),
        source_id: SourceId(1),
        path: "todo.title.change".to_owned(),
        scoped: true,
        scope_id: Some(scope_id),
        payload_schema: boon_plan::SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
            row_lookup_field: None,
        },
    }];
    plan.storage_layout.list_slots = vec![list_slot.clone()];
    plan.storage_layout.scalar_slots = vec![boon_plan::ScalarStorageSlot {
        id: boon_plan::PlanStorageId(1),
        state_id: StateId(30),
        value_type: PlanValueType::Text,
        scope_id: Some(scope_id),
        indexed: true,
        initial_value_kind: InitialValueKind::Text,
        initial_constant_id: None,
        initial_root_field_path: None,
        initial_row_field_path: None,
    }];
    plan.debug_map.state_slots = vec![boon_plan::DebugEntry {
        id: "state:30".to_owned(),
        label: "todo.title".to_owned(),
    }];
    plan.debug_map.fields = vec![
        boon_plan::DebugEntry {
            id: "field:8".to_owned(),
            label: "todo.title".to_owned(),
        },
        boon_plan::DebugEntry {
            id: "field:9".to_owned(),
            label: "todo.normalized_title".to_owned(),
        },
        boon_plan::DebugEntry {
            id: "field:40".to_owned(),
            label: "store.title_to_add".to_owned(),
        },
    ];
    plan.regions = vec![boon_plan::OperationRegion {
        id: boon_plan::PlanRegionId(2),
        kind: RegionKind::DerivedEvaluation,
        ops: vec![PlanOp {
            id: PlanOpId(11),
            kind: PlanOpKind::DerivedValue {
                derived_kind: boon_plan::PlanDerivedKind::Pure,
                startup_recompute: true,
                expression: Some(PlanDerivedExpression::RowExpression {
                    expression: PlanRowExpression::Field {
                        input: ValueRef::State(StateId(30)),
                    },
                }),
            },
            inputs: Vec::new(),
            output: Some(ValueRef::Field(FieldId(9))),
            indexed: true,
            unresolved_executable_ref_count: 0,
        }],
    }];
    let list_state = BTreeMap::from([(7usize, Vec::new())]);
    let derived_values = BTreeMap::from([(FieldId(40), json!("Write tests"))]);

    let constructed = construct_list_append_row_with(
        &plan,
        &list_slot,
        10,
        &append,
        7,
        "todos",
        &list_state,
        4,
        1,
        &derived_values,
        true,
        |plan, _list_state, row, expression| {
            let PlanRowExpression::Field {
                input: ValueRef::State(state_id),
            } = expression
            else {
                return Err("unexpected row expression".into());
            };
            let field_name = local_field_name(&state_label(plan, *state_id));
            row.fields
                .get(&field_name)
                .cloned()
                .ok_or_else(|| "missing row field".into())
        },
    )
    .expect("append row construction should succeed");

    assert_eq!(constructed.row.key, 4);
    assert_eq!(constructed.row.fields["title"], json!("Write tests"));
    assert_eq!(
        constructed.row.fields["normalized_title"],
        json!("Write tests")
    );
    assert_eq!(constructed.source_paths, vec!["todo.title.change"]);
    assert_eq!(
        constructed.executor_report["executor"],
        "cpu-plan-list-append-row-construction-v1"
    );
}

// test: list_append_execution_is_executor_owned
#[test]
fn list_append_execution_is_executor_owned() {
    let scope_id = boon_plan::ScopeId(3);
    let list_slot = boon_plan::ListStorageSlot {
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
    };
    let append = boon_plan::PlanListAppend {
        trigger: ValueRef::Field(FieldId(40)),
        fields: vec![boon_plan::PlanListAppendField {
            name: "title".to_owned(),
            field_id: Some(FieldId(8)),
            value_ref: Some(ValueRef::Field(FieldId(40))),
            constant_id: None,
        }],
    };
    let mut plan = empty_executor_test_plan();
    plan.source_routes = vec![SourceRoute {
        id: boon_plan::PlanSourceRouteId(1),
        source_id: SourceId(1),
        path: "todo.title.change".to_owned(),
        scoped: true,
        scope_id: Some(scope_id),
        payload_schema: boon_plan::SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
            row_lookup_field: None,
        },
    }];
    plan.storage_layout.list_slots = vec![list_slot];
    plan.storage_layout.scalar_slots = vec![boon_plan::ScalarStorageSlot {
        id: boon_plan::PlanStorageId(1),
        state_id: StateId(30),
        value_type: PlanValueType::Text,
        scope_id: Some(scope_id),
        indexed: true,
        initial_value_kind: InitialValueKind::Text,
        initial_constant_id: None,
        initial_root_field_path: None,
        initial_row_field_path: None,
    }];
    plan.debug_map.list_slots = vec![boon_plan::DebugEntry {
        id: "list:7".to_owned(),
        label: "todos".to_owned(),
    }];
    plan.debug_map.state_slots = vec![boon_plan::DebugEntry {
        id: "state:30".to_owned(),
        label: "todo.title".to_owned(),
    }];
    plan.debug_map.fields = vec![
        boon_plan::DebugEntry {
            id: "field:8".to_owned(),
            label: "todo.title".to_owned(),
        },
        boon_plan::DebugEntry {
            id: "field:9".to_owned(),
            label: "todo.normalized_title".to_owned(),
        },
        boon_plan::DebugEntry {
            id: "field:40".to_owned(),
            label: "store.title_to_add".to_owned(),
        },
    ];
    plan.regions = vec![
        boon_plan::OperationRegion {
            id: boon_plan::PlanRegionId(1),
            kind: RegionKind::ListOperations,
            ops: vec![PlanOp {
                id: PlanOpId(10),
                kind: PlanOpKind::ListOperation {
                    operation_kind: boon_plan::PlanListOperationKind::Append,
                    append: Some(append),
                    remove: None,
                    retain: None,
                    count: None,
                },
                inputs: vec![ValueRef::Field(FieldId(40))],
                output: Some(ValueRef::List(boon_plan::ListId(7))),
                indexed: false,
                unresolved_executable_ref_count: 0,
            }],
        },
        boon_plan::OperationRegion {
            id: boon_plan::PlanRegionId(2),
            kind: RegionKind::DerivedEvaluation,
            ops: vec![PlanOp {
                id: PlanOpId(11),
                kind: PlanOpKind::DerivedValue {
                    derived_kind: boon_plan::PlanDerivedKind::Pure,
                    startup_recompute: true,
                    expression: Some(PlanDerivedExpression::RowExpression {
                        expression: PlanRowExpression::Field {
                            input: ValueRef::State(StateId(30)),
                        },
                    }),
                },
                inputs: Vec::new(),
                output: Some(ValueRef::Field(FieldId(9))),
                indexed: true,
                unresolved_executable_ref_count: 0,
            }],
        },
    ];
    let mut list_state = BTreeMap::from([(7usize, Vec::new())]);
    let mut list_next_keys = BTreeMap::new();
    let mut bool_delta_lists = BTreeSet::new();
    let derived_values = BTreeMap::from([(FieldId(40), json!("Write tests"))]);

    let execution = append_list_rows_for_derived_values_with(
        &plan,
        &mut list_state,
        &mut list_next_keys,
        &mut bool_delta_lists,
        &derived_values,
        |plan, _list_state, row, expression| {
            let PlanRowExpression::Field {
                input: ValueRef::State(state_id),
            } = expression
            else {
                return Err("unexpected row expression".into());
            };
            let field_name = local_field_name(&state_label(plan, *state_id));
            row.fields
                .get(&field_name)
                .cloned()
                .ok_or_else(|| "missing row field".into())
        },
    )
    .expect("append execution should succeed");

    assert_eq!(execution.appended_row_count, 1);
    assert_eq!(execution.source_bind_count, 1);
    assert_eq!(
        execution.executor_report["executor"],
        "cpu-plan-list-append-execution-v1"
    );
    let rows = list_state.get(&7).expect("list should exist");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, 1);
    assert_eq!(rows[0].fields["title"], json!("Write tests"));
    assert_eq!(rows[0].fields["normalized_title"], json!("Write tests"));
    assert_eq!(execution.report_rows[0]["list"], "todos");
    assert!(
        execution
            .semantic_deltas
            .iter()
            .any(|delta| delta["kind"] == "ListInsert")
    );
    assert!(
        execution
            .semantic_deltas
            .iter()
            .any(|delta| delta["kind"] == "SourceBind")
    );
    assert!(
        execution
            .semantic_deltas
            .iter()
            .any(|delta| delta["kind"] == "FieldSet" && delta["field_path"] == "normalized_title")
    );
}

// test: list_remove_execution_is_executor_owned
#[test]
fn list_remove_execution_is_executor_owned() {
    let scope_id = boon_plan::ScopeId(3);
    let list_slot = boon_plan::ListStorageSlot {
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
    };
    let mut plan = empty_executor_test_plan();
    plan.source_routes = vec![SourceRoute {
        id: boon_plan::PlanSourceRouteId(1),
        source_id: SourceId(1),
        path: "todo.remove".to_owned(),
        scoped: true,
        scope_id: Some(scope_id),
        payload_schema: boon_plan::SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
            row_lookup_field: None,
        },
    }];
    plan.storage_layout.list_slots = vec![list_slot];
    plan.debug_map.list_slots = vec![boon_plan::DebugEntry {
        id: "list:7".to_owned(),
        label: "todos".to_owned(),
    }];
    plan.regions = vec![boon_plan::OperationRegion {
        id: boon_plan::PlanRegionId(1),
        kind: RegionKind::ListOperations,
        ops: vec![PlanOp {
            id: PlanOpId(10),
            kind: PlanOpKind::ListOperation {
                operation_kind: boon_plan::PlanListOperationKind::Remove,
                append: None,
                remove: Some(boon_plan::PlanListRemove {
                    source: ValueRef::Source(SourceId(1)),
                    predicate: boon_plan::PlanListRemovePredicate::AlwaysTrue,
                }),
                retain: None,
                count: None,
            },
            inputs: vec![ValueRef::Source(SourceId(1))],
            output: Some(ValueRef::List(boon_plan::ListId(7))),
            indexed: false,
            unresolved_executable_ref_count: 0,
        }],
    }];
    let mut list_state = BTreeMap::from([(
        7usize,
        vec![PlanExecutorListRowState {
            key: 1,
            generation: 1,
            fields: BTreeMap::from([("title".to_owned(), json!("Write tests"))]),
            private_bytes: BTreeMap::new(),
            fixed_bytes_banks: BTreeMap::new(),
        }],
    )]);
    let event = PlanExecutorLiveSourceEvent {
        source: "todo.remove",
        text: None,
        key: None,
        list_id: Some("todos"),
        address: None,
        target_text: None,
        target_occurrence: None,
        target_key: Some(1),
        target_generation: Some(1),
        bind_epoch: Some(1),
        source_epoch: None,
        source_id: None,
    };

    let execution = remove_list_rows_for_source_event(
        &plan,
        SourceId(1),
        &plan.source_routes[0],
        &event,
        &mut list_state,
    )
    .expect("remove execution should succeed");

    assert_eq!(execution.removed_row_count, 1);
    assert_eq!(execution.source_unbind_count, 1);
    assert_eq!(
        execution.executor_report["executor"],
        "cpu-plan-list-remove-execution-v1"
    );
    assert!(list_state.get(&7).unwrap().is_empty());
    assert_eq!(execution.report_rows[0]["list"], "todos");
    assert_eq!(
        execution.report_rows[0]["row_resolution"]["method"],
        "key_generation"
    );
    assert!(
        execution
            .semantic_deltas
            .iter()
            .any(|delta| delta["kind"] == "SourceUnbind")
    );
    assert!(
        execution
            .semantic_deltas
            .iter()
            .any(|delta| delta["kind"] == "ListRemove")
    );
}

