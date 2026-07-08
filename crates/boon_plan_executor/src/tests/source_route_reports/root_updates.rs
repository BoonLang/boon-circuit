// Included by `../source_route_reports.rs`.

// test: root_match_const_update_executes_on_json_surface
#[test]
fn root_match_const_update_executes_on_json_surface() {
    let source_id = SourceId(2);
    let state_id = StateId(3);
    let update_op_id = PlanOpId(4);
    let constants = vec![
        boon_plan::PlanConstant {
            id: PlanConstantId(0),
            value: PlanConstantValue::Enum {
                value: "Light".to_owned(),
            },
        },
        boon_plan::PlanConstant {
            id: PlanConstantId(1),
            value: PlanConstantValue::Text {
                value: "Light".to_owned(),
            },
        },
        boon_plan::PlanConstant {
            id: PlanConstantId(2),
            value: PlanConstantValue::Enum {
                value: "Dark".to_owned(),
            },
        },
        boon_plan::PlanConstant {
            id: PlanConstantId(3),
            value: PlanConstantValue::Text {
                value: "Dark".to_owned(),
            },
        },
        boon_plan::PlanConstant {
            id: PlanConstantId(4),
            value: PlanConstantValue::Enum {
                value: "Light".to_owned(),
            },
        },
        boon_plan::PlanConstant {
            id: PlanConstantId(5),
            value: PlanConstantValue::Text {
                value: "__".to_owned(),
            },
        },
        boon_plan::PlanConstant {
            id: PlanConstantId(6),
            value: PlanConstantValue::Text {
                value: "SKIP".to_owned(),
            },
        },
    ];
    let source_route = SourceRoute {
        id: boon_plan::PlanSourceRouteId(0),
        source_id,
        path: "store.mode_toggle".to_owned(),
        scoped: false,
        scope_id: None,
        payload_schema: boon_plan::SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
            row_lookup_field: None,
        },
    };
    let update_op = PlanOp {
        id: update_op_id,
        kind: PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::MatchConst,
            ordered_inputs: vec![
                ValueRef::State(state_id),
                ValueRef::Constant(PlanConstantId(1)),
                ValueRef::Constant(PlanConstantId(2)),
                ValueRef::Constant(PlanConstantId(3)),
                ValueRef::Constant(PlanConstantId(4)),
                ValueRef::Constant(PlanConstantId(5)),
                ValueRef::Constant(PlanConstantId(6)),
            ],
            source_payload_field: None,
            update_constant_id: None,
            source_guard: None,
        },
        inputs: vec![ValueRef::Source(source_id), ValueRef::State(state_id)],
        output: Some(ValueRef::State(state_id)),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let plan = MachinePlan {
        version: boon_plan::PlanVersion::default(),
        target_profile: boon_plan::TargetProfile::SoftwareDefault,
        constants,
        source_routes: vec![source_route.clone()],
        storage_layout: boon_plan::StorageLayout {
            scalar_slots: vec![boon_plan::ScalarStorageSlot {
                id: boon_plan::PlanStorageId(1),
                state_id,
                value_type: PlanValueType::Enum,
                scope_id: None,
                indexed: false,
                initial_value_kind: InitialValueKind::Enum,
                initial_constant_id: Some(PlanConstantId(0)),
                initial_root_field_path: None,
                initial_row_field_path: None,
            }],
            list_slots: Vec::new(),
            byte_banks: Vec::new(),
        },
        regions: vec![boon_plan::OperationRegion {
            id: boon_plan::PlanRegionId(3),
            kind: RegionKind::UpdateBranches,
            ops: vec![update_op.clone()],
        }],
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
            constant_count: 7,
            source_route_count: 1,
            scalar_storage_count: 1,
            list_storage_count: 0,
            byte_bank_storage_count: 0,
            operation_count: 1,
            typed_value_ref_count: 9,
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
                label: "store.mode_toggle".to_owned(),
            }],
            state_slots: vec![boon_plan::DebugEntry {
                id: "state:3".to_owned(),
                label: "store.mode".to_owned(),
            }],
            list_slots: Vec::new(),
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };

    let root_state = JsonMap::from_iter([("store.mode".to_owned(), json!("Light"))]);
    let evaluation = evaluate_root_json_update_branch(
        &plan,
        &update_op,
        source_id,
        &source_route,
        &RootJsonSourceEvent::default(),
        &root_state,
    )
    .expect("root MatchConst should evaluate");
    assert!(evaluation.supported);
    assert!(!evaluation.skipped_by_guard);
    assert_eq!(evaluation.expression_kind, Some("match_const"));
    assert_eq!(evaluation.value, Some(json!("Dark")));

    let execution = execute_root_json_update_branch(
        &plan,
        &update_op,
        source_id,
        &source_route,
        &RootJsonSourceEvent::default(),
        &root_state,
    )
    .expect("root MatchConst should execute on JSON surface");
    assert_eq!(
        execution.surface_kind,
        RootUpdateExecutionSurfaceKind::PlanJson
    );
    assert_eq!(execution.executed.unwrap().value, json!("Dark"));

    let skipped_root_state = JsonMap::from_iter([("store.mode".to_owned(), json!("System"))]);
    let skipped = evaluate_root_json_update_branch(
        &plan,
        &update_op,
        source_id,
        &source_route,
        &RootJsonSourceEvent::default(),
        &skipped_root_state,
    )
    .expect("fallback SKIP should evaluate as a no-op");
    assert!(skipped.supported);
    assert!(skipped.skipped_by_guard);
    assert_eq!(skipped.value, None);

    let field_id = FieldId(9);
    let mut field_update_op = update_op.clone();
    if let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &mut field_update_op.kind {
        ordered_inputs[0] = ValueRef::Field(field_id);
    }
    field_update_op.inputs = vec![ValueRef::Source(source_id), ValueRef::Field(field_id)];
    let mut field_plan = plan.clone();
    field_plan.regions[0].ops = vec![field_update_op.clone()];
    field_plan.debug_map.derived_values = vec![boon_plan::DebugEntry {
        id: "field:9".to_owned(),
        label: "store.selected_mode".to_owned(),
    }];
    let field_root_state = JsonMap::from_iter([
        ("store.mode".to_owned(), json!("System")),
        ("store.selected_mode".to_owned(), json!("Light")),
    ]);
    let field_evaluation = evaluate_root_json_update_branch(
        &field_plan,
        &field_update_op,
        source_id,
        &source_route,
        &RootJsonSourceEvent::default(),
        &field_root_state,
    )
    .expect("root MatchConst should read root derived fields");
    assert!(field_evaluation.supported);
    assert_eq!(field_evaluation.value, Some(json!("Dark")));
}

// test: root_match_value_const_update_executes_read_path_arms
#[test]
fn root_match_value_const_update_executes_read_path_arms() {
    let source_id = SourceId(7);
    let selector_state_id = StateId(8);
    let value_a_state_id = StateId(9);
    let output_state_id = StateId(10);
    let update_op_id = PlanOpId(11);
    let constants = vec![
        boon_plan::PlanConstant {
            id: PlanConstantId(0),
            value: PlanConstantValue::Text {
                value: "A".to_owned(),
            },
        },
        boon_plan::PlanConstant {
            id: PlanConstantId(1),
            value: PlanConstantValue::Text {
                value: "__".to_owned(),
            },
        },
        boon_plan::PlanConstant {
            id: PlanConstantId(2),
            value: PlanConstantValue::Text {
                value: "SKIP".to_owned(),
            },
        },
        boon_plan::PlanConstant {
            id: PlanConstantId(3),
            value: PlanConstantValue::Text {
                value: "old".to_owned(),
            },
        },
        boon_plan::PlanConstant {
            id: PlanConstantId(4),
            value: PlanConstantValue::Text {
                value: "alpha".to_owned(),
            },
        },
    ];
    let source_route = SourceRoute {
        id: boon_plan::PlanSourceRouteId(0),
        source_id,
        path: "store.trigger".to_owned(),
        scoped: false,
        scope_id: None,
        payload_schema: boon_plan::SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
            row_lookup_field: None,
        },
    };
    let update_op = PlanOp {
        id: update_op_id,
        kind: PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::MatchValueConst,
            ordered_inputs: vec![
                ValueRef::State(selector_state_id),
                ValueRef::Constant(PlanConstantId(0)),
                ValueRef::State(value_a_state_id),
                ValueRef::Constant(PlanConstantId(1)),
                ValueRef::Constant(PlanConstantId(2)),
            ],
            source_payload_field: None,
            update_constant_id: None,
            source_guard: None,
        },
        inputs: vec![
            ValueRef::Source(source_id),
            ValueRef::State(selector_state_id),
            ValueRef::State(value_a_state_id),
        ],
        output: Some(ValueRef::State(output_state_id)),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let plan = MachinePlan {
        version: boon_plan::PlanVersion::default(),
        target_profile: boon_plan::TargetProfile::SoftwareDefault,
        constants,
        source_routes: vec![source_route.clone()],
        storage_layout: boon_plan::StorageLayout {
            scalar_slots: vec![
                boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(0),
                    state_id: selector_state_id,
                    value_type: PlanValueType::Text,
                    scope_id: None,
                    indexed: false,
                    initial_value_kind: InitialValueKind::Text,
                    initial_constant_id: Some(PlanConstantId(0)),
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                },
                boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(1),
                    state_id: value_a_state_id,
                    value_type: PlanValueType::Text,
                    scope_id: None,
                    indexed: false,
                    initial_value_kind: InitialValueKind::Text,
                    initial_constant_id: Some(PlanConstantId(4)),
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                },
                boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(2),
                    state_id: output_state_id,
                    value_type: PlanValueType::Text,
                    scope_id: None,
                    indexed: false,
                    initial_value_kind: InitialValueKind::Text,
                    initial_constant_id: Some(PlanConstantId(3)),
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                },
            ],
            list_slots: Vec::new(),
            byte_banks: Vec::new(),
        },
        regions: vec![boon_plan::OperationRegion {
            id: boon_plan::PlanRegionId(3),
            kind: RegionKind::UpdateBranches,
            ops: vec![update_op.clone()],
        }],
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
            constant_count: 5,
            source_route_count: 1,
            scalar_storage_count: 3,
            list_storage_count: 0,
            byte_bank_storage_count: 0,
            operation_count: 1,
            typed_value_ref_count: 8,
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
                id: "source:7".to_owned(),
                label: "store.trigger".to_owned(),
            }],
            state_slots: vec![
                boon_plan::DebugEntry {
                    id: "state:8".to_owned(),
                    label: "store.selector".to_owned(),
                },
                boon_plan::DebugEntry {
                    id: "state:9".to_owned(),
                    label: "store.value_a".to_owned(),
                },
                boon_plan::DebugEntry {
                    id: "state:10".to_owned(),
                    label: "store.selected".to_owned(),
                },
            ],
            list_slots: Vec::new(),
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };

    let root_state = JsonMap::from_iter([
        ("store.selector".to_owned(), json!("A")),
        ("store.value_a".to_owned(), json!("alpha")),
        ("store.selected".to_owned(), json!("old")),
    ]);
    let evaluation = evaluate_root_json_update_branch(
        &plan,
        &update_op,
        source_id,
        &source_route,
        &RootJsonSourceEvent::default(),
        &root_state,
    )
    .expect("root MatchValueConst should evaluate");
    assert!(evaluation.supported);
    assert_eq!(evaluation.value, Some(json!("alpha")));
    assert_eq!(evaluation.expression_kind, Some("match_value_const"));

    let skipped_root_state = JsonMap::from_iter([
        ("store.selector".to_owned(), json!("B")),
        ("store.value_a".to_owned(), json!("alpha")),
        ("store.selected".to_owned(), json!("old")),
    ]);
    let skipped = evaluate_root_json_update_branch(
        &plan,
        &update_op,
        source_id,
        &source_route,
        &RootJsonSourceEvent::default(),
        &skipped_root_state,
    )
    .expect("root MatchValueConst fallback SKIP should evaluate");
    assert!(skipped.supported);
    assert!(skipped.skipped_by_guard);
    assert_eq!(skipped.value, None);
}

// test: root_read_path_update_reads_derived_field_value
#[test]
fn root_read_path_update_reads_derived_field_value() {
    let source_id = SourceId(12);
    let output_state_id = StateId(13);
    let field_id = FieldId(14);
    let update_op = PlanOp {
        id: PlanOpId(15),
        kind: PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::ReadPath,
            ordered_inputs: Vec::new(),
            source_payload_field: None,
            update_constant_id: None,
            source_guard: None,
        },
        inputs: vec![ValueRef::Source(source_id), ValueRef::Field(field_id)],
        output: Some(ValueRef::State(output_state_id)),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let source_route = SourceRoute {
        id: boon_plan::PlanSourceRouteId(0),
        source_id,
        path: "store.trigger".to_owned(),
        scoped: false,
        scope_id: None,
        payload_schema: boon_plan::SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
            row_lookup_field: None,
        },
    };
    let plan = MachinePlan {
        version: boon_plan::PlanVersion::default(),
        target_profile: boon_plan::TargetProfile::SoftwareDefault,
        constants: Vec::new(),
        source_routes: vec![source_route.clone()],
        storage_layout: boon_plan::StorageLayout {
            scalar_slots: vec![boon_plan::ScalarStorageSlot {
                id: boon_plan::PlanStorageId(0),
                state_id: output_state_id,
                value_type: PlanValueType::Text,
                scope_id: None,
                indexed: false,
                initial_value_kind: InitialValueKind::Text,
                initial_constant_id: None,
                initial_root_field_path: None,
                initial_row_field_path: None,
            }],
            list_slots: Vec::new(),
            byte_banks: Vec::new(),
        },
        regions: vec![boon_plan::OperationRegion {
            id: boon_plan::PlanRegionId(3),
            kind: RegionKind::UpdateBranches,
            ops: vec![update_op.clone()],
        }],
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
            constant_count: 0,
            source_route_count: 1,
            scalar_storage_count: 1,
            list_storage_count: 0,
            byte_bank_storage_count: 0,
            operation_count: 1,
            typed_value_ref_count: 4,
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
                id: "source:12".to_owned(),
                label: "store.trigger".to_owned(),
            }],
            state_slots: vec![boon_plan::DebugEntry {
                id: "state:13".to_owned(),
                label: "store.selected".to_owned(),
            }],
            list_slots: Vec::new(),
            derived_values: vec![boon_plan::DebugEntry {
                id: "field:14".to_owned(),
                label: "store.derived_selected".to_owned(),
            }],
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };
    let root_state = JsonMap::from_iter([
        ("store.selected".to_owned(), json!("old")),
        ("store.derived_selected".to_owned(), json!("new")),
    ]);
    let evaluation = evaluate_root_json_update_branch(
        &plan,
        &update_op,
        source_id,
        &source_route,
        &RootJsonSourceEvent::default(),
        &root_state,
    )
    .expect("root ReadPath should read root derived fields");

    assert!(evaluation.supported);
    assert_eq!(evaluation.value, Some(json!("new")));
    assert_eq!(evaluation.expression_kind, Some("read_path"));
}

// test: root_json_update_execution_assembles_plan_json_executed_update
#[test]
fn root_json_update_execution_assembles_plan_json_executed_update() {
    let (plan, source_id, state_id, update_op_id) = simple_text_source_payload_plan();
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
    let update_op = plan
        .regions
        .iter()
        .flat_map(|region| region.ops.iter())
        .find(|op| op.id == update_op_id)
        .expect("test plan should include update op");
    let event = RootJsonSourceEvent {
        text: Some("hello".to_owned()),
        ..RootJsonSourceEvent::default()
    };
    let execution = execute_root_json_update_branch(
        &plan,
        update_op,
        source_id,
        &plan.source_routes[0],
        &event,
        &JsonMap::new(),
    )
    .expect("source-payload text branch should execute in PlanExecutor JSON surface");

    assert_eq!(
        execution.surface_kind,
        RootUpdateExecutionSurfaceKind::PlanJson
    );
    assert_eq!(
        execution.executor_report["executor"],
        "cpu-plan-root-json-update-execution-v1"
    );
    assert_eq!(execution.executor_report["surface"], "plan-json");
    assert_eq!(
        execution.evaluator_report["execution_surface_core"]["execution_surface"],
        "plan-json"
    );
    let executed = execution
        .executed
        .expect("Plan JSON branch should assemble an executed root update");
    assert_eq!(executed.value, json!("hello"));
    assert_eq!(executed.expression_kind, "source_payload");
    assert_eq!(executed.source_payload_field, json!("Text"));
    assert_eq!(executed.update_constant_id, JsonValue::Null);
    assert_eq!(executed.executor_core["expression_kind"], "source_payload");
    assert_eq!(
        executed.executor_core["execution_surface_core"]["execution_surface"],
        "plan-json"
    );
    let mut root_state = JsonMap::new();
    assert_eq!(
        apply_root_json_state_value(
            &plan,
            &mut root_state,
            state_id,
            executed.value,
            update_op_id
        )
        .expect("executed value should apply to root state")
        .target_state_label,
        "store.input"
    );
}

// test: root_update_branch_collection_stages_plan_json_candidate
#[test]
fn root_update_branch_collection_stages_plan_json_candidate() {
    let (plan, source_id, state_id, update_op_id) = simple_text_source_payload_plan();
    let source_route_slot = plan
        .source_routes
        .iter()
        .find(|route| route.source_id == source_id)
        .expect("test plan should include source route");
    let op = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter())
        .find(|op| op.id == update_op_id)
        .expect("test plan should include update op");
    let mut staged_root_state = initialize_root_state(&plan).expect("root state should initialize");
    let root_json_event = RootJsonSourceEvent {
        text: Some("Typed text".to_owned()),
        ..RootJsonSourceEvent::default()
    };
    let mut tracker = RootUpdateCandidateTracker::default();
    let mut touched_updates = BTreeMap::new();
    let mut runtime_branch =
        |_op: &PlanOp, _state: &PlanExecutorRootState| -> PlanExecutorResult<_> {
            panic!("plan-json source payload update should not call runtime branch")
        };

    let collection = collect_root_update_candidate_for_step(
        &plan,
        op,
        source_id,
        "store.input.change",
        source_route_slot,
        &root_json_event,
        &mut staged_root_state,
        &mut tracker,
        &mut touched_updates,
        &mut runtime_branch,
    )
    .expect("PlanExecutor should collect root update candidate");

    assert_eq!(collection.target_state_id, Some(state_id));
    assert!(collection.inserted_update);
    assert!(!collection.runtime_branch_used);
    assert_eq!(staged_root_state.root_state["store.input"], "Typed text");
    assert_eq!(touched_updates.len(), 1);
    assert_eq!(touched_updates[&state_id.0].value, json!("Typed text"));
    assert_eq!(tracker.ordered_candidates().len(), 1);
    assert_eq!(
        collection.executor_report["executor"],
        "cpu-plan-root-update-branch-collection-v1"
    );
}

// test: root_executed_update_candidate_and_state_apply_are_executor_owned
#[test]
fn root_executed_update_candidate_and_state_apply_are_executor_owned() {
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
            cpu_plan_executor_complete: true,
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
                id: "state:7".to_owned(),
                label: "store.payload".to_owned(),
            }],
            list_slots: Vec::new(),
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };
    let inline = vec![7, 8, 9];
    let bytes = PlanExecutorBytes::from_inline(
        sha256_bytes(&inline),
        inline.len() as u64,
        inline.clone(),
        "root executed update test",
    )
    .expect("test bytes should be valid");
    let executed = RootExecutedUpdate {
        value: json!({"$boon_type": "BYTES", "byte_len": 3}),
        bytes_value: Some(bytes),
        fixed_bytes_mutation: Some(RootBytesFixedMutation {
            input_state_id: StateId(7),
            output_state_id: StateId(7),
            patches: vec![(1, 10)],
        }),
        bytes_access: JsonValue::Null,
        executor_core: json!({"executor": "test"}),
        state_write_core: JsonValue::Null,
        bytes_state_core: JsonValue::Null,
        expression_kind: "bytes_set".to_owned(),
        source_payload_field: JsonValue::Null,
        update_constant_id: JsonValue::Null,
        update_constant_value: JsonValue::Null,
        host_effect: JsonValue::Null,
    };

    let candidate = root_update_candidate_from_executed(7, 99, &executed);
    assert_eq!(candidate.state_id, 7);
    assert_eq!(candidate.op_id, 99);
    assert_eq!(candidate.bytes_value.as_ref().unwrap()["byte_len"], 3);
    assert_eq!(
        candidate.fixed_bytes_mutation.as_ref().unwrap()["patches"],
        json!([[1, 10]])
    );

    let mut bytes_executed = executed.clone();
    bytes_executed.fixed_bytes_mutation = None;
    let mut root_state = JsonMap::new();
    let mut private_bytes = BTreeMap::new();
    let mut fixed_byte_banks = BTreeMap::new();
    let report = apply_executed_root_update_to_state(
        &mut root_state,
        &mut private_bytes,
        &mut fixed_byte_banks,
        &plan,
        7,
        &bytes_executed,
        99,
    )
    .expect("executed root update should apply through PlanExecutor");
    assert_eq!(root_state["store.payload"]["byte_len"], 3);
    assert_eq!(
        private_bytes.get(&7).unwrap().inline_bytes,
        inline,
        "bytes_value takes the direct bytes commit path"
    );
    assert_eq!(
        report["executor"],
        "cpu-plan-root-update-storage-transition-v1"
    );
}

// test: root_state_initializer_owns_public_and_private_bytes_state
#[test]
fn root_state_initializer_owns_public_and_private_bytes_state() {
    let bytes = vec![4, 5, 6];
    let plan = MachinePlan {
        version: boon_plan::PlanVersion::default(),
        target_profile: boon_plan::TargetProfile::SoftwareDefault,
        constants: vec![boon_plan::PlanConstant {
            id: PlanConstantId(1),
            value: PlanConstantValue::Bytes {
                byte_len: bytes.len() as u64,
                sha256: sha256_bytes(&bytes),
                inline_bytes: Some(bytes.clone()),
            },
        }],
        source_routes: Vec::new(),
        storage_layout: boon_plan::StorageLayout {
            scalar_slots: vec![boon_plan::ScalarStorageSlot {
                id: boon_plan::PlanStorageId(10),
                state_id: StateId(7),
                value_type: PlanValueType::Bytes { fixed_len: Some(3) },
                scope_id: None,
                indexed: false,
                initial_value_kind: InitialValueKind::Bytes,
                initial_constant_id: Some(PlanConstantId(1)),
                initial_root_field_path: None,
                initial_row_field_path: None,
            }],
            list_slots: Vec::new(),
            byte_banks: vec![boon_plan::ByteStorageBank {
                id: boon_plan::PlanStorageId(11),
                state_storage_id: boon_plan::PlanStorageId(10),
                state_id: StateId(7),
                scope_id: None,
                indexed: false,
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
            cpu_plan_executor_complete: true,
            constant_count: 1,
            source_route_count: 0,
            scalar_storage_count: 1,
            list_storage_count: 0,
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
                id: "state:7".to_owned(),
                label: "store.payload".to_owned(),
            }],
            list_slots: Vec::new(),
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };

    let root = initialize_root_state(&plan).expect("root state should initialize");

    assert_eq!(root.initialized_state_count, 1);
    assert_eq!(root.root_state["store.payload"]["$boon_type"], "BYTES");
    assert_eq!(
        root.private_bytes
            .get(&7)
            .expect("private bytes should be initialized")
            .inline_bytes(),
        bytes.as_slice()
    );
    assert_eq!(root.fixed_byte_banks.get(&7), Some(&bytes));
    assert_eq!(
        root.executor_report["executor"],
        "cpu-plan-root-state-initializer-v1"
    );
    assert_eq!(
        root.executor_report["bytes_initialization_core"]["executor"],
        "cpu-plan-root-bytes-storage-initializer-v1"
    );
}

// test: root_state_initializer_copies_root_initial_fields_without_constants
#[test]
fn root_state_initializer_copies_root_initial_fields_without_constants() {
    let plan = MachinePlan {
        version: boon_plan::PlanVersion::default(),
        target_profile: boon_plan::TargetProfile::SoftwareDefault,
        constants: vec![boon_plan::PlanConstant {
            id: PlanConstantId(0),
            value: PlanConstantValue::Text {
                value: "draft.txt".to_owned(),
            },
        }],
        source_routes: Vec::new(),
        storage_layout: boon_plan::StorageLayout {
            scalar_slots: vec![
                boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(0),
                    state_id: StateId(1),
                    value_type: PlanValueType::Text,
                    scope_id: None,
                    indexed: false,
                    initial_value_kind: InitialValueKind::Text,
                    initial_constant_id: Some(PlanConstantId(0)),
                    initial_root_field_path: None,
                    initial_row_field_path: None,
                },
                boon_plan::ScalarStorageSlot {
                    id: boon_plan::PlanStorageId(1),
                    state_id: StateId(2),
                    value_type: PlanValueType::RootInitialField,
                    scope_id: None,
                    indexed: false,
                    initial_value_kind: InitialValueKind::RootInitialField,
                    initial_constant_id: None,
                    initial_root_field_path: Some("active_file".to_owned()),
                    initial_row_field_path: None,
                },
            ],
            list_slots: Vec::new(),
            byte_banks: Vec::new(),
        },
        regions: vec![boon_plan::OperationRegion {
            id: boon_plan::PlanRegionId(0),
            kind: RegionKind::StateInitialization,
            ops: vec![
                PlanOp {
                    id: PlanOpId(0),
                    kind: PlanOpKind::StateInitialize {
                        initial_value_kind: InitialValueKind::Text,
                        initial_constant_id: Some(PlanConstantId(0)),
                    },
                    inputs: Vec::new(),
                    output: Some(ValueRef::State(StateId(1))),
                    indexed: false,
                    unresolved_executable_ref_count: 0,
                },
                PlanOp {
                    id: PlanOpId(1),
                    kind: PlanOpKind::StateInitialize {
                        initial_value_kind: InitialValueKind::RootInitialField,
                        initial_constant_id: None,
                    },
                    inputs: Vec::new(),
                    output: Some(ValueRef::State(StateId(2))),
                    indexed: false,
                    unresolved_executable_ref_count: 0,
                },
            ],
        }],
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
            cpu_plan_executor_complete: true,
            constant_count: 1,
            source_route_count: 0,
            scalar_storage_count: 2,
            list_storage_count: 0,
            byte_bank_storage_count: 0,
            operation_count: 2,
            typed_value_ref_count: 2,
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
                    id: "state:1".to_owned(),
                    label: "store.active_file".to_owned(),
                },
                boon_plan::DebugEntry {
                    id: "state:2".to_owned(),
                    label: "store.selected_file".to_owned(),
                },
            ],
            list_slots: Vec::new(),
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };

    let root = initialize_root_state(&plan).expect("root state should initialize");
    assert_eq!(root.root_state["store.active_file"], "draft.txt");
    assert_eq!(root.root_state["store.selected_file"], "draft.txt");
    assert_eq!(root.initialized_state_count, 2);
    assert_eq!(root.executor_report["root_initial_field_copy_count"], 1);

    let executed =
        execute_initial_state(&plan).expect("initial-state execution should initialize copies");
    assert_eq!(
        executed.executor_report["state_summary"]["store.selected_file"],
        "draft.txt"
    );
    assert_eq!(executed.executor_report["root_initial_field_copy_count"], 1);
}

// test: ordered_root_update_ops_are_resolved_by_executor_dispatch
#[test]
fn ordered_root_update_ops_are_resolved_by_executor_dispatch() {
    let update_op = |id: usize| PlanOp {
        id: PlanOpId(id),
        kind: PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::Const,
            ordered_inputs: Vec::new(),
            source_payload_field: None,
            update_constant_id: Some(PlanConstantId(id + 100)),
            source_guard: None,
        },
        inputs: Vec::new(),
        output: Some(ValueRef::State(StateId(id + 200))),
        indexed: false,
        unresolved_executable_ref_count: 0,
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
            scalar_slots: Vec::new(),
            list_slots: Vec::new(),
            byte_banks: Vec::new(),
        },
        regions: vec![boon_plan::OperationRegion {
            id: boon_plan::PlanRegionId(1),
            kind: RegionKind::UpdateBranches,
            ops: vec![update_op(10), update_op(20)],
        }],
        dirty_plan: boon_plan::DirtyPlan {
            dependency_edges: 0,
            unresolved_dependency_edges: 0,
        },
        commit_plan: boon_plan::CommitPlan {
            update_branch_count: 2,
            unresolved_update_branch_count: 0,
        },
        delta_plan: boon_plan::DeltaPlan { deltas: Vec::new() },
        capability_summary: boon_plan::CapabilitySummary {
            executable: true,
            typed_lowering_executable: true,
            cpu_plan_executor_complete: false,
            constant_count: 0,
            source_route_count: 1,
            scalar_storage_count: 0,
            list_storage_count: 0,
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
            state_slots: Vec::new(),
            list_slots: Vec::new(),
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };
    let dispatch = RootScenarioStepDispatch {
        plan_hash: "test".to_owned(),
        source_label: "store.submit".to_owned(),
        source_id: SourceId(1),
        source_route_scoped: false,
        ordered_update_op_ids: vec![PlanOpId(20), PlanOpId(10)],
        derived_op_count: 0,
        has_list_remove_work: false,
        root_update_key_gate: None,
        root_update_key_matches: true,
        executable_work: true,
        executor_report: JsonValue::Null,
    };

    let ops = ordered_root_update_ops_for_dispatch(&plan, &dispatch)
        .expect("dispatch-selected update ops should resolve through PlanExecutor");
    assert_eq!(
        ops.iter().map(|op| op.id.0).collect::<Vec<_>>(),
        vec![20, 10]
    );
    let route = source_route_slot_for_dispatch(&plan, &dispatch)
        .expect("dispatch-selected source route should resolve through PlanExecutor");
    assert_eq!(route.path, "store.submit");
    assert_eq!(
        route.payload_schema.fields,
        vec![SourcePayloadField::Text, SourcePayloadField::Key]
    );

    let stale_dispatch = RootScenarioStepDispatch {
        ordered_update_op_ids: vec![PlanOpId(99)],
        ..dispatch.clone()
    };
    let error = ordered_root_update_ops_for_dispatch(&plan, &stale_dispatch)
        .expect_err("stale dispatch op ids must be rejected");
    assert!(
        error
            .to_string()
            .contains("root source-event selector chose missing update op 99"),
        "unexpected error: {error}"
    );
    let stale_route_dispatch = RootScenarioStepDispatch {
        source_id: SourceId(99),
        ..dispatch
    };
    let error = source_route_slot_for_dispatch(&plan, &stale_route_dispatch)
        .expect_err("stale dispatch source ids must be rejected");
    assert!(
        error
            .to_string()
            .contains("MachinePlan source route `store.submit` has no route slot"),
        "unexpected error: {error}"
    );
}

// test: root_scenario_step_preparation_is_executor_owned
#[test]
fn root_scenario_step_preparation_is_executor_owned() {
    let update_op = PlanOp {
        id: PlanOpId(10),
        kind: PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::SourcePayload,
            ordered_inputs: Vec::new(),
            source_payload_field: Some(SourcePayloadField::Text),
            update_constant_id: None,
            source_guard: None,
        },
        inputs: vec![
            ValueRef::Source(SourceId(1)),
            ValueRef::SourcePayload {
                source_id: SourceId(1),
                field: SourcePayloadField::Text,
            },
        ],
        output: Some(ValueRef::State(StateId(4))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let derived_op = PlanOp {
        id: PlanOpId(20),
        kind: PlanOpKind::DerivedValue {
            derived_kind: boon_plan::PlanDerivedKind::SourceEventTransform,
            startup_recompute: true,
            expression: Some(PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
                source_id: SourceId(1),
                key_field: SourcePayloadField::Key,
                required_key: "Enter".to_owned(),
                state: ValueRef::SourcePayload {
                    source_id: SourceId(1),
                    field: SourcePayloadField::Text,
                },
                skip_empty: true,
            }),
        },
        inputs: vec![
            ValueRef::Source(SourceId(1)),
            ValueRef::SourcePayload {
                source_id: SourceId(1),
                field: SourcePayloadField::Text,
            },
            ValueRef::SourcePayload {
                source_id: SourceId(1),
                field: SourcePayloadField::Key,
            },
        ],
        output: Some(ValueRef::Field(FieldId(9))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let plan = MachinePlan {
        version: boon_plan::PlanVersion::default(),
        target_profile: boon_plan::TargetProfile::SoftwareDefault,
        constants: Vec::new(),
        source_routes: vec![SourceRoute {
            id: boon_plan::PlanSourceRouteId(0),
            source_id: SourceId(1),
            path: "store.submit".to_owned(),
            scoped: false,
            scope_id: None,
            payload_schema: boon_plan::SourcePayloadSchema {
                fields: vec![SourcePayloadField::Text, SourcePayloadField::Key],
                typed_fields: vec![
                    boon_plan::SourcePayloadDescriptor {
                        field: SourcePayloadField::Text,
                        value_type: boon_plan::SourcePayloadValueType::Text,
                    },
                    boon_plan::SourcePayloadDescriptor {
                        field: SourcePayloadField::Key,
                        value_type: boon_plan::SourcePayloadValueType::Text,
                    },
                ],
                row_lookup_field: None,
            },
        }],
        storage_layout: boon_plan::StorageLayout {
            scalar_slots: vec![boon_plan::ScalarStorageSlot {
                id: boon_plan::PlanStorageId(0),
                state_id: StateId(4),
                value_type: boon_plan::PlanValueType::Text,
                scope_id: None,
                indexed: false,
                initial_value_kind: boon_plan::InitialValueKind::Text,
                initial_constant_id: None,
                initial_root_field_path: None,
                initial_row_field_path: None,
            }],
            list_slots: Vec::new(),
            byte_banks: Vec::new(),
        },
        regions: vec![
            boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(1),
                kind: RegionKind::UpdateBranches,
                ops: vec![update_op],
            },
            boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(2),
                kind: RegionKind::DerivedEvaluation,
                ops: vec![derived_op],
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
            constant_count: 0,
            source_route_count: 1,
            scalar_storage_count: 1,
            list_storage_count: 0,
            byte_bank_storage_count: 0,
            operation_count: 2,
            typed_value_ref_count: 7,
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
                id: "source:1".to_owned(),
                label: "store.submit".to_owned(),
            }],
            state_slots: vec![boon_plan::DebugEntry {
                id: "state:4".to_owned(),
                label: "store.input".to_owned(),
            }],
            list_slots: Vec::new(),
            derived_values: vec![boon_plan::DebugEntry {
                id: "field:9".to_owned(),
                label: "store.trimmed_submit".to_owned(),
            }],
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };

    let preparation = prepare_root_scenario_step(
        &plan,
        "store.submit",
        &RootJsonSourceEvent {
            text: Some("  Write tests  ".to_owned()),
            key: Some("Enter".to_owned()),
            ..RootJsonSourceEvent::default()
        },
        &JsonMap::new(),
    )
    .expect("PlanExecutor should prepare root scenario step work");

    assert_eq!(preparation.source_id, SourceId(1));
    assert_eq!(preparation.source_route_slot.path, "store.submit");
    assert_eq!(preparation.route_ops.len(), 1);
    assert_eq!(preparation.route_ops[0].id, PlanOpId(10));
    assert_eq!(
        preparation.derived_values.get(&FieldId(9)),
        Some(&json!("Write tests"))
    );
    assert!(preparation.root_update_key_matches);
    assert_eq!(
        preparation.executor_report["executor"],
        "cpu-plan-root-scenario-step-preparation-v1"
    );
    assert_eq!(
        preparation.root_dispatch_report["materialized_work_core"]["executor"],
        "cpu-plan-root-scenario-materialized-work-v1"
    );
}

// test: root_aggregate_evaluator_counts_rows_and_reports_changed_deltas
#[test]
fn root_aggregate_evaluator_counts_rows_and_reports_changed_deltas() {
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
        regions: vec![
            boon_plan::OperationRegion {
                id: boon_plan::PlanRegionId(1),
                kind: RegionKind::ListOperations,
                ops: vec![PlanOp {
                    id: PlanOpId(10),
                    kind: PlanOpKind::ListOperation {
                        operation_kind: PlanListOperationKind::Count,
                        append: None,
                        remove: None,
                        retain: None,
                        count: Some(boon_plan::PlanListCount {
                            target: ValueRef::Field(FieldId(20)),
                            predicate: boon_plan::PlanListRemovePredicate::RowFieldBoolNot {
                                input: ValueRef::State(StateId(30)),
                            },
                        }),
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
                        expression: Some(PlanDerivedExpression::NumberCompareConst {
                            left: ValueRef::Field(FieldId(20)),
                            op: ">".to_owned(),
                            right: 0,
                        }),
                    },
                    inputs: Vec::new(),
                    output: Some(ValueRef::Field(FieldId(21))),
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
                label: "todo.completed".to_owned(),
            }],
            list_slots: vec![boon_plan::DebugEntry {
                id: "list:7".to_owned(),
                label: "todos".to_owned(),
            }],
            derived_values: vec![boon_plan::DebugEntry {
                id: "field:21".to_owned(),
                label: "store.has_active".to_owned(),
            }],
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };

    let mut active_fields = BTreeMap::new();
    active_fields.insert("completed".to_owned(), json!(false));
    let mut completed_fields = BTreeMap::new();
    completed_fields.insert("completed".to_owned(), json!(true));
    let list_state = BTreeMap::from([(
        7,
        vec![
            PlanExecutorListRow {
                key: 1,
                generation: 1,
                fields: active_fields,
            },
            PlanExecutorListRow {
                key: 2,
                generation: 1,
                fields: completed_fields,
            },
        ],
    )]);

    let values = evaluate_root_pure_number_compare_values(&plan, &list_state)
        .expect("root aggregate evaluation should stay executor-owned");
    assert_eq!(values.get(&21), Some(&json!(true)));

    let changes = changed_root_derived_deltas(&plan, &BTreeMap::new(), &values);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].0["field_path"], "store.has_active");
    assert_eq!(changes[0].1["expression_kind"], "number_compare_const");
}

