// Included by `../source_route_reports.rs`.

// test: source_derived_values_are_evaluated_by_executor
#[test]
fn source_derived_values_are_evaluated_by_executor() {
    let plan = MachinePlan {
        version: boon_plan::PlanVersion::default(),
        target_profile: boon_plan::TargetProfile::SoftwareDefault,
        constants: Vec::new(),
        source_routes: Vec::new(),
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
        regions: vec![boon_plan::OperationRegion {
            id: boon_plan::PlanRegionId(1),
            kind: RegionKind::DerivedEvaluation,
            ops: vec![PlanOp {
                id: PlanOpId(10),
                kind: PlanOpKind::DerivedValue {
                    derived_kind: boon_plan::PlanDerivedKind::SourceEventTransform,
                    startup_recompute: true,
                    expression: Some(PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
                        source_id: SourceId(2),
                        key_field: SourcePayloadField::Key,
                        required_key: "Enter".to_owned(),
                        state: ValueRef::SourcePayload {
                            source_id: SourceId(2),
                            field: SourcePayloadField::Text,
                        },
                        skip_empty: true,
                    }),
                },
                inputs: vec![
                    ValueRef::Source(SourceId(2)),
                    ValueRef::SourcePayload {
                        source_id: SourceId(2),
                        field: SourcePayloadField::Key,
                    },
                    ValueRef::SourcePayload {
                        source_id: SourceId(2),
                        field: SourcePayloadField::Text,
                    },
                ],
                output: Some(ValueRef::Field(FieldId(9))),
                indexed: false,
                unresolved_executable_ref_count: 0,
            }],
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
            cpu_plan_executor_complete: false,
            constant_count: 0,
            source_route_count: 1,
            scalar_storage_count: 1,
            list_storage_count: 0,
            byte_bank_storage_count: 0,
            operation_count: 1,
            typed_value_ref_count: 3,
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
                id: "state:4".to_owned(),
                label: "store.input".to_owned(),
            }],
            list_slots: Vec::new(),
            derived_values: vec![boon_plan::DebugEntry {
                id: "field:9".to_owned(),
                label: "store.title_to_add".to_owned(),
            }],
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };
    let root_state = JsonMap::new();

    let values = evaluate_source_derived_values_for_event(
        &plan,
        SourceId(2),
        &RootJsonSourceEvent {
            text: Some("  Write tests  ".to_owned()),
            key: Some("Enter".to_owned()),
            ..RootJsonSourceEvent::default()
        },
        &root_state,
    )
    .expect("source-derived evaluation should stay executor-owned");
    assert_eq!(values.get(&FieldId(9)), Some(&json!("Write tests")));
    let delta_reports = build_source_derived_value_deltas(&plan, &values);
    assert_eq!(delta_reports.len(), 1);
    assert_eq!(delta_reports[0].0, "FieldSet:store.title_to_add");
    assert_eq!(delta_reports[0].1["kind"], "FieldSet");
    assert_eq!(delta_reports[0].1["field_path"], "store.title_to_add");
    assert_eq!(delta_reports[0].1["value"], "Write tests");
    assert_eq!(delta_reports[0].2["field_id"], 9);
    assert_eq!(delta_reports[0].2["field_path"], "store.title_to_add");
    let step_deltas = assemble_source_derived_step_deltas(&plan, &values);
    assert_eq!(
        step_deltas.semantic_delta_signatures,
        vec!["FieldSet:store.title_to_add"]
    );
    assert_eq!(step_deltas.semantic_deltas.len(), 1);
    assert_eq!(step_deltas.semantic_deltas[0], delta_reports[0].1.clone());
    assert_eq!(step_deltas.reports, vec![delta_reports[0].2.clone()]);
    assert_eq!(
        step_deltas.executor_report["executor"],
        "cpu-plan-source-derived-step-deltas-v1"
    );
    assert_eq!(step_deltas.executor_report["semantic_delta_count"], 1);

    let skipped = evaluate_source_derived_values_for_event(
        &plan,
        SourceId(2),
        &RootJsonSourceEvent {
            text: Some("  Write tests  ".to_owned()),
            key: Some("Escape".to_owned()),
            ..RootJsonSourceEvent::default()
        },
        &root_state,
    )
    .expect("non-matching key should skip the source-derived value");
    assert!(skipped.is_empty());

    let skipped_empty = evaluate_source_derived_values_for_event(
        &plan,
        SourceId(2),
        &RootJsonSourceEvent {
            text: Some("   ".to_owned()),
            key: Some("Enter".to_owned()),
            ..RootJsonSourceEvent::default()
        },
        &root_state,
    )
    .expect("empty trimmed text should be skipped when skip_empty=true");
    assert!(skipped_empty.is_empty());
}

