// Included by `../source_route_reports.rs`.

// test: machine_plan_debug_label_helpers_are_executor_owned
#[test]
fn machine_plan_debug_label_helpers_are_executor_owned() {
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
                id: "state:4".to_owned(),
                label: "store.input".to_owned(),
            }],
            list_slots: vec![boon_plan::DebugEntry {
                id: "list:2".to_owned(),
                label: "todos".to_owned(),
            }],
            derived_values: vec![boon_plan::DebugEntry {
                id: "field:9".to_owned(),
                label: "store.has_todos".to_owned(),
            }],
            fields: vec![boon_plan::DebugEntry {
                id: "field:8".to_owned(),
                label: "todo.title".to_owned(),
            }],
            unresolved_executable_refs: Vec::new(),
        },
    };

    assert_eq!(state_label(&plan, StateId(4)), "store.input");
    assert_eq!(state_label_by_id(&plan, 4), "store.input");
    assert_eq!(list_label(&plan, 2), "todos");
    assert_eq!(field_label(&plan, 8), "todo.title");
    assert_eq!(semantic_field_label(&plan, 8), "todo.title");
    assert_eq!(derived_field_label(&plan, 9), "store.has_todos");
    assert_eq!(local_field_name("todo.title"), "title");
    assert!(root_state_is_scalar(&plan, StateId(4)));
    assert_eq!(state_label_by_id(&plan, 99), "state:99");
}

// test: summarize_plan_lists_reports_counts_titles_and_rows
#[test]
fn summarize_plan_lists_reports_counts_titles_and_rows() {
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
            list_storage_count: 1,
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
            state_slots: Vec::new(),
            list_slots: vec![boon_plan::DebugEntry {
                id: "list:7".to_owned(),
                label: "todos".to_owned(),
            }],
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    };
    let mut active_fields = BTreeMap::new();
    active_fields.insert("title".to_owned(), json!("Write tests"));
    active_fields.insert("completed".to_owned(), json!(false));
    let mut completed_fields = BTreeMap::new();
    completed_fields.insert("title".to_owned(), json!("Compile"));
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

    let summary = summarize_plan_lists(&plan, &list_state);
    assert_eq!(summary["todos"]["row_count"], 2);
    assert_eq!(summary["todos"]["active_count"], 1);
    assert_eq!(summary["todos"]["completed_count"], 1);
    assert_eq!(
        summary["todos"]["titles"],
        json!(["Write tests", "Compile"])
    );
    assert_eq!(summary["todos"]["rows"][0]["key"], 1);
}

