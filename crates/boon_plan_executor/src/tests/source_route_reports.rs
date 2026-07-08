// Included by `../tests.rs`; kept in the parent test module for private executor-helper access.

























































fn empty_executor_test_plan() -> MachinePlan {
    MachinePlan {
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
            state_slots: Vec::new(),
            list_slots: Vec::new(),
            derived_values: Vec::new(),
            fields: Vec::new(),
            unresolved_executable_refs: Vec::new(),
        },
    }
}

// Nested behavior-area shards keep broad test groups navigable without widening production APIs.
include!("source_route_reports/commands_and_reports.rs");
include!("source_route_reports/debug_and_summary.rs");
include!("source_route_reports/indexed_updates.rs");
include!("source_route_reports/list_execution.rs");
include!("source_route_reports/root_updates.rs");
include!("source_route_reports/source_derived_values.rs");
