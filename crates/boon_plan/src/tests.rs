use super::*;

fn empty_plan() -> MachinePlan {
    MachinePlan {
        version: PlanVersion::default(),
        target_profile: TargetProfile::SoftwareDefault,
        demand: DemandPlan {
            root_derived_outputs: RootOutputDemand::Selected(Vec::new()),
        },
        document: None,
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
    }
}

#[test]
fn root_output_demand_keeps_empty_selected_distinct_from_all() {
    assert_ne!(
        RootOutputDemand::All,
        RootOutputDemand::Selected(Vec::new())
    );
}

#[test]
fn typed_binary_plan_hash_is_deterministic_and_field_sensitive() {
    let plan = empty_plan();
    assert_eq!(plan_sha256(&plan).unwrap(), plan_sha256(&plan).unwrap());

    let mut changed = plan.clone();
    changed.demand.root_derived_outputs = RootOutputDemand::All;
    assert_ne!(plan_sha256(&plan).unwrap(), plan_sha256(&changed).unwrap());
}
