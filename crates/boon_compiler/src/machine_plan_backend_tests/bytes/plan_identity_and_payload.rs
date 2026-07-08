// Included by `../bytes.rs`.

// test: source_payload_bool_type_maps_from_ir
#[test]
fn source_payload_bool_type_maps_from_ir() {
    assert_eq!(
        source_payload_value_type_from_ir(ir::SourcePayloadValueType::Bool),
        SourcePayloadValueType::Bool
    );
}

// test: plan_hash_is_stable_for_same_plan
#[test]
fn plan_hash_is_stable_for_same_plan() {
    let plan = MachinePlan {
        version: PlanVersion::default(),
        target_profile: TargetProfile::SoftwareDefault,
        constants: Vec::new(),
        source_routes: Vec::new(),
        storage_layout: StorageLayout {
            scalar_slots: Vec::new(),
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
    assert_eq!(plan_sha256(&plan).unwrap(), plan_sha256(&plan).unwrap());
    assert_eq!(verify_plan(&plan).unwrap().status, "pass");
}

