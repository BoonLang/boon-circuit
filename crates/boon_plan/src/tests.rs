use super::*;

#[test]
fn source_payload_schema_row_lookup_field_uses_generic_name() {
    let schema = SourcePayloadSchema {
        fields: Vec::new(),
        typed_fields: Vec::new(),
        row_lookup_field: Some("file".to_owned()),
    };
    assert_eq!(schema.row_lookup_field_name(), Some("file"));

    let decoded: SourcePayloadSchema = serde_json::from_value(serde_json::json!({
        "fields": [],
        "row_lookup_field": "file"
    }))
    .unwrap();
    assert_eq!(decoded.row_lookup_field_name(), Some("file"));
}

#[test]
fn bool_source_payload_type_matches_bool_plan_state_only() {
    assert!(source_payload_value_type_matches_plan_type(
        SourcePayloadValueType::Bool,
        &PlanValueType::Bool
    ));
    assert!(!source_payload_value_type_matches_plan_type(
        SourcePayloadValueType::Bool,
        &PlanValueType::Text
    ));
    assert!(!source_payload_value_type_matches_plan_type(
        SourcePayloadValueType::Text,
        &PlanValueType::Bool
    ));
}

#[test]
fn root_row_expression_cpu_support_matches_source_transform_subset() {
    let expression = PlanRowExpression::TextToNumber {
        input: Box::new(PlanRowExpression::ListFindValue {
            list_id: ListId(3),
            field: FieldId(4),
            value: Box::new(PlanRowExpression::Field {
                input: ValueRef::State(StateId(5)),
            }),
            target: FieldId(6),
            fallback: Some(Box::new(PlanRowExpression::Constant {
                constant_id: PlanConstantId(7),
            })),
        }),
    };
    assert!(root_row_expression_cpu_evaluable(&expression));

    let unsupported = PlanRowExpression::TextLength {
        input: Box::new(PlanRowExpression::Constant {
            constant_id: PlanConstantId(8),
        }),
    };
    assert!(!root_row_expression_cpu_evaluable(&unsupported));
}

#[test]
fn root_read_path_supports_declared_derived_field_input() {
    let scalar_slots = vec![ScalarStorageSlot {
        id: PlanStorageId(0),
        state_id: StateId(1),
        value_type: PlanValueType::Text,
        scope_id: None,
        indexed: false,
        initial_value_kind: InitialValueKind::Text,
        initial_constant_id: None,
        initial_root_field_path: None,
        initial_row_field_path: None,
    }];
    let op = PlanOp {
        id: PlanOpId(2),
        kind: PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::ReadPath,
            ordered_inputs: Vec::new(),
            source_payload_field: None,
            update_constant_id: None,
            source_guard: None,
        },
        inputs: vec![ValueRef::Source(SourceId(3)), ValueRef::Field(FieldId(4))],
        output: Some(ValueRef::State(StateId(1))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };

    assert!(read_path_inputs_supported(&scalar_slots, &op));
}

#[test]
fn root_initial_field_paths_resolve() {
    let mut plan = MachinePlan {
        version: PlanVersion::default(),
        target_profile: TargetProfile::SoftwareDefault,
        constants: Vec::new(),
        source_routes: Vec::new(),
        storage_layout: StorageLayout {
            scalar_slots: vec![ScalarStorageSlot {
                id: PlanStorageId(0),
                state_id: StateId(1),
                value_type: PlanValueType::RootInitialField,
                scope_id: None,
                indexed: false,
                initial_value_kind: InitialValueKind::RootInitialField,
                initial_constant_id: None,
                initial_root_field_path: Some("input".to_owned()),
                initial_row_field_path: None,
            }],
            list_slots: Vec::new(),
            byte_banks: Vec::new(),
        },
        regions: Vec::new(),
        dirty_plan: DirtyPlan {
            dependency_edges: 0,
            unresolved_dependency_edges: 0,
        },
        commit_plan: CommitPlan {
            update_branch_count: 0,
            unresolved_update_branch_count: 0,
        },
        delta_plan: DeltaPlan { deltas: Vec::new() },
        capability_summary: CapabilitySummary {
            executable: true,
            typed_lowering_executable: true,
            cpu_plan_executor_complete: true,
            constant_count: 0,
            source_route_count: 0,
            scalar_storage_count: 1,
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
        debug_map: DebugMap {
            source_units: Vec::new(),
            source_routes: Vec::new(),
            state_slots: Vec::new(),
            list_slots: Vec::new(),
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };

    assert!(initial_field_paths_resolve(&plan));
    plan.storage_layout.scalar_slots[0].initial_root_field_path = None;
    assert!(!initial_field_paths_resolve(&plan));
    plan.storage_layout.scalar_slots[0].initial_root_field_path = Some("input".to_owned());
    plan.storage_layout.scalar_slots[0].initial_row_field_path = Some("row.input".to_owned());
    assert!(!initial_field_paths_resolve(&plan));
}

fn bytes_guarded_const_update_op() -> PlanOp {
    PlanOp {
        id: PlanOpId(7),
        kind: PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::Const,
            ordered_inputs: Vec::new(),
            source_payload_field: None,
            update_constant_id: Some(PlanConstantId(0)),
            source_guard: Some(PlanSourceGuard::SourcePayloadOneOf {
                source_id: SourceId(3),
                field: SourcePayloadField::Bytes,
                values: vec!["01fe04".to_owned()],
            }),
        },
        inputs: vec![
            ValueRef::Source(SourceId(3)),
            ValueRef::SourcePayload {
                source_id: SourceId(3),
                field: SourcePayloadField::Bytes,
            },
        ],
        output: Some(ValueRef::State(StateId(11))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    }
}

#[test]
fn bytes_source_payload_guards_are_executable_plan_inputs() {
    let op = bytes_guarded_const_update_op();
    let constants = vec![PlanConstant {
        id: PlanConstantId(0),
        value: PlanConstantValue::Text {
            value: "matched".to_owned(),
        },
    }];
    let scalar_slots = vec![ScalarStorageSlot {
        id: PlanStorageId(0),
        state_id: StateId(11),
        value_type: PlanValueType::Text,
        scope_id: None,
        indexed: false,
        initial_value_kind: InitialValueKind::Text,
        initial_constant_id: None,
        initial_root_field_path: None,
        initial_row_field_path: None,
    }];
    let PlanOpKind::UpdateBranch { source_guard, .. } = &op.kind else {
        panic!("test op should be an update branch");
    };
    assert!(source_guard_refs_resolve(&op, source_guard));
    assert!(cpu_plan_executor_supports_whole_plan_op(
        &scalar_slots,
        &[],
        &constants,
        &op,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
    ));
}

#[test]
fn indexed_bytes_set_is_cpu_plan_executor_supported() {
    let op = PlanOp {
        id: PlanOpId(8),
        kind: PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesSet,
            ordered_inputs: vec![
                ValueRef::State(StateId(0)),
                ValueRef::Constant(PlanConstantId(0)),
                ValueRef::Constant(PlanConstantId(1)),
            ],
            source_payload_field: None,
            update_constant_id: None,
            source_guard: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0)), ValueRef::State(StateId(0))],
        output: Some(ValueRef::State(StateId(1))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let scalar_slots = vec![
        ScalarStorageSlot {
            id: PlanStorageId(0),
            state_id: StateId(0),
            value_type: PlanValueType::Bytes { fixed_len: Some(3) },
            scope_id: Some(ScopeId(0)),
            indexed: true,
            initial_value_kind: InitialValueKind::RowInitialField,
            initial_constant_id: None,
            initial_root_field_path: None,
            initial_row_field_path: Some("row.payload".to_owned()),
        },
        ScalarStorageSlot {
            id: PlanStorageId(1),
            state_id: StateId(1),
            value_type: PlanValueType::Bytes { fixed_len: Some(3) },
            scope_id: Some(ScopeId(0)),
            indexed: true,
            initial_value_kind: InitialValueKind::RowInitialField,
            initial_constant_id: None,
            initial_root_field_path: None,
            initial_row_field_path: Some("row.patched".to_owned()),
        },
    ];
    let constants = vec![
        PlanConstant {
            id: PlanConstantId(0),
            value: PlanConstantValue::Number { value: 0 },
        },
        PlanConstant {
            id: PlanConstantId(1),
            value: PlanConstantValue::Byte { value: 0xaa },
        },
    ];
    assert!(cpu_plan_executor_supports_whole_plan_op(
        &scalar_slots,
        &[],
        &constants,
        &op,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
    ));
}
