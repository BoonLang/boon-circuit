use super::*;

fn simple_text_source_payload_plan() -> (MachinePlan, SourceId, StateId, PlanOpId) {
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
    (plan, source_id, state_id, update_op_id)
}

// PlanExecutor tests are grouped by execution/report area while staying in this module for private helper access.
include!("tests/bytes_execution.rs");
include!("tests/indexed_updates_and_deltas.rs");
include!("tests/list_execution.rs");
include!("tests/root_updates.rs");
include!("tests/scenario_reports.rs");
include!("tests/source_route_reports.rs");
