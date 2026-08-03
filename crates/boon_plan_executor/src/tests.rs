use super::*;
use boon_plan::*;
use std::collections::{BTreeMap, BTreeSet};

fn number(value: i64) -> Value {
    Value::integer(value).unwrap()
}

fn stored_number(value: i64) -> boon_persistence::StoredValue {
    boon_persistence::StoredValue::integer(value).unwrap()
}

fn number_constant(value: i64) -> PlanConstantValue {
    PlanConstantValue::Number {
        value: ExactNumber::from_i64(value),
    }
}

fn tag_constant(name: impl Into<String>) -> PlanConstantValue {
    PlanConstantValue::Tag { name: name.into() }
}

fn truth_constant(value: bool) -> PlanConstantValue {
    tag_constant(if value { "True" } else { "False" })
}

fn initial(value: PlanConstantValue) -> PlanInitialListFieldInitializer {
    PlanInitialListFieldInitializer::Constant { value }
}

fn row(arena: &mut PlanRowExpressionArena, node: PlanRowExpressionNode) -> PlanRowExpressionId {
    arena.push(node).unwrap()
}

fn row_field(arena: &mut PlanRowExpressionArena, input: ValueRef) -> PlanRowExpressionId {
    row(arena, PlanRowExpressionNode::Field { input })
}

fn row_constant(
    arena: &mut PlanRowExpressionArena,
    constant_id: PlanConstantId,
) -> PlanRowExpressionId {
    row(arena, PlanRowExpressionNode::Constant { constant_id })
}

fn row_list_ref(arena: &mut PlanRowExpressionArena, list_id: ListId) -> PlanRowExpressionId {
    row(arena, PlanRowExpressionNode::ListRef { list_id })
}

fn row_authority_list_ref(
    arena: &mut PlanRowExpressionArena,
    list_id: ListId,
) -> PlanRowExpressionId {
    row(arena, PlanRowExpressionNode::AuthorityListRef { list_id })
}

#[allow(clippy::too_many_arguments)]
fn plan(
    demand: RootOutputDemand,
    row_expressions: PlanRowExpressionArena,
    constants: Vec<PlanConstant>,
    routes: Vec<SourceRoute>,
    scalar_slots: Vec<ScalarStorageSlot>,
    list_slots: Vec<ListStorageSlot>,
    ops: Vec<PlanOp>,
    state_labels: Vec<(StateId, &str)>,
    list_labels: Vec<(ListId, &str)>,
    field_labels: Vec<(FieldId, &str)>,
) -> MachinePlan {
    let application = ApplicationPlan::new(ApplicationIdentity::new(
        "dev.boon.plan-executor-tests",
        "test",
        "local",
    ))
    .unwrap();
    let state_label_map = state_labels.iter().copied().collect::<BTreeMap<_, _>>();
    let list_label_map = list_labels.iter().copied().collect::<BTreeMap<_, _>>();
    let field_label_map = field_labels.iter().copied().collect::<BTreeMap<_, _>>();
    let mutated_lists = ops
        .iter()
        .filter_map(|operation| match (&operation.kind, &operation.output) {
            (PlanOpKind::ListMutation { .. }, Some(ValueRef::List(list))) => Some(*list),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let memory = scalar_slots
        .iter()
        .map(|slot| {
            let path = state_label_map
                .get(&slot.state_id)
                .copied()
                .unwrap_or("state");
            MemoryPlan::new(
                slot.id,
                if slot.indexed {
                    MemoryKind::IndexedField
                } else {
                    MemoryKind::Scalar
                },
                path,
                test_data_type(slot.value_type),
                InitialProvenance::ReconstructableDefault,
                MemoryOwnerPath {
                    canonical_module: "tests".to_owned(),
                    named_owner_path: path
                        .rsplit_once('.')
                        .map(|(owner, _)| owner)
                        .unwrap_or("root")
                        .to_owned(),
                },
            )
            .unwrap()
        })
        .collect();
    let lists = list_slots
        .iter()
        .map(|slot| {
            let path = list_label_map.get(&slot.list_id).copied().unwrap_or("list");
            let owner = MemoryOwnerPath {
                canonical_module: "tests".to_owned(),
                named_owner_path: "root".to_owned(),
            };
            let memory_id = MemoryId::from_identity(&owner, path, MemoryKind::List).unwrap();
            let row_fields = slot
                .row_fields
                .iter()
                .map(|field| {
                    MemoryLeafPlan::new(
                        memory_id,
                        Some(field.field_id),
                        field_label_map
                            .get(&field.field_id)
                            .copied()
                            .unwrap_or("field"),
                        DataTypePlan::Unknown,
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();
            ListMemoryPlan::new(
                slot.id,
                path,
                DataTypePlan::List {
                    item: Box::new(DataTypePlan::Record {
                        fields: Vec::new(),
                        open: true,
                    }),
                },
                if slot.initializer_kind == ListInitializerKind::Unknown
                    || mutated_lists.contains(&slot.list_id)
                {
                    InitialProvenance::MaterializedAuthority
                } else {
                    InitialProvenance::ReconstructableDefault
                },
                owner,
                slot.hidden_key_type.clone(),
                slot.has_generation,
                row_fields,
            )
            .unwrap()
        })
        .collect();
    let persistence = PersistencePlan::new(&application, 1, memory, lists, Vec::new()).unwrap();
    let list_dataflow = list_slots
        .iter()
        .map(|slot| {
            let activation_mode = if slot.initializer_kind == ListInitializerKind::Unknown
                || mutated_lists.contains(&slot.list_id)
            {
                ListActivationMode::MaterializedAuthority
            } else {
                ListActivationMode::StaticDefault
            };
            let memory = persistence
                .lists
                .iter()
                .find(|memory| memory.runtime_slot == slot.id)
                .expect("list fixture semantic identity");
            PlanListDataflow::new(
                slot.list_id,
                memory.memory_id,
                memory.type_fingerprint,
                activation_mode,
                None,
                Vec::new(),
            )
            .unwrap()
        })
        .collect();
    let mut plan = MachinePlan {
        version: PlanVersion::default(),
        target_profile: TargetProfile::SoftwareDefault,
        program_role: ProgramRole::Server,
        distributed_endpoint: None,
        producer_function_instances: Vec::new(),
        application,
        persistence,
        effects: Vec::new(),
        outputs: Vec::new(),
        host_ports: Vec::new(),
        list_indexes: Vec::new(),
        list_dataflow,
        demand: DemandPlan {
            root_derived_outputs: demand,
        },
        document: None,
        row_expressions,
        constants,
        source_routes: routes,
        activations: Vec::new(),
        pulse_batches: Vec::new(),
        storage_layout: StorageLayout {
            scalar_slots,
            list_slots,
            byte_banks: Vec::new(),
        },
        regions: vec![OperationRegion {
            id: PlanRegionId(0),
            kind: RegionKind::DerivedEvaluation,
            ops,
        }],
        dirty_plan: DirtyPlan {
            dependency_edges: 0,
            unresolved_dependency_edges: 0,
        },
        commit_plan: CommitPlan {
            state_update_count: 0,
            unresolved_state_update_count: 0,
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
            state_slots: state_labels
                .into_iter()
                .map(|(id, label)| DebugEntry {
                    id: format!("state:{}", id.0),
                    label: label.to_owned(),
                })
                .collect(),
            list_slots: list_labels
                .into_iter()
                .map(|(id, label)| DebugEntry {
                    id: format!("list:{}", id.0),
                    label: label.to_owned(),
                })
                .collect(),
            derived_values: Vec::new(),
            fields: field_labels
                .into_iter()
                .map(|(id, label)| DebugEntry {
                    id: format!("field:{}", id.0),
                    label: label.to_owned(),
                })
                .collect(),
            unresolved_executable_refs: Vec::new(),
        },
    };
    for operation in plan.regions.iter_mut().flat_map(|region| &mut region.ops) {
        operation
            .synchronize_expression_inputs(&plan.row_expressions)
            .unwrap();
    }
    plan.capability_summary = derive_capability_summary(&plan);
    plan
}

fn set_list_activation_mode(
    plan: &mut MachinePlan,
    list_id: ListId,
    activation_mode: ListActivationMode,
) {
    let mut lists = plan.persistence.lists.clone();
    let memory = lists
        .iter_mut()
        .find(|memory| {
            plan.storage_layout
                .list_slots
                .iter()
                .any(|slot| slot.id == memory.runtime_slot && slot.list_id == list_id)
        })
        .expect("persistent list activation fixture");
    memory.initial_provenance = activation_mode.initial_provenance();
    plan.persistence = PersistencePlan::new(
        &plan.application,
        plan.persistence.schema_version,
        plan.persistence.memory.clone(),
        lists,
        Vec::new(),
    )
    .unwrap();
    let (reconstruction_output, dependencies) =
        if activation_mode == ListActivationMode::DerivedDefault {
            let operation = plan
                .regions
                .iter()
                .flat_map(|region| &region.ops)
                .find(|operation| {
                    matches!(operation.output, Some(ValueRef::List(output)) if output == list_id)
                        && match &operation.kind {
                            PlanOpKind::DerivedValue {
                                materialization: Some(materialization),
                                ..
                            } => materialization.target_list == list_id,
                            PlanOpKind::ListProjection { .. } => true,
                            _ => false,
                        }
                })
                .expect("derived list activation producer");
            (
                Some(list_id),
                operation
                    .inputs
                    .iter()
                    .filter_map(|input| match input {
                        ValueRef::List(dependency) if *dependency != list_id => Some(*dependency),
                        _ => None,
                    })
                    .collect(),
            )
        } else {
            (None, Vec::new())
        };
    let dataflow = plan
        .list_dataflow
        .iter_mut()
        .find(|entry| entry.list_id == list_id)
        .expect("list activation fixture");
    *dataflow = PlanListDataflow::new(
        list_id,
        dataflow.semantic_identity,
        dataflow.type_fingerprint,
        activation_mode,
        reconstruction_output,
        dependencies,
    )
    .unwrap();
    plan.list_dataflow.sort_by_key(|entry| entry.list_id);
}

fn test_data_type(value_type: PlanValueType) -> DataTypePlan {
    match value_type {
        PlanValueType::Text => DataTypePlan::Text,
        PlanValueType::Number => DataTypePlan::Number,
        PlanValueType::Bytes { fixed_len } => DataTypePlan::Bytes { fixed_len },
        PlanValueType::Tag => DataTypePlan::Variant {
            variants: Vec::new(),
        },
        PlanValueType::Data => DataTypePlan::Unknown,
        PlanValueType::Unknown => DataTypePlan::Unknown,
        PlanValueType::Bits { width } => DataTypePlan::Bits { width },
    }
}

fn constant(id: usize, value: PlanConstantValue) -> PlanConstant {
    PlanConstant {
        id: PlanConstantId(id),
        value,
    }
}

fn number_slot(state: usize, constant: usize) -> ScalarStorageSlot {
    ScalarStorageSlot {
        id: PlanStorageId(state),
        state_id: StateId(state),
        owner: PlanOwner::root(),
        value_type: PlanValueType::Number,
        scope_id: None,
        indexed: false,
        indexed_field_id: None,
        lifetime: PlanStateLifetime::Persistent,
        initializer: ScalarInitializerPlan::Constant {
            constant_id: PlanConstantId(constant),
        },
    }
}

fn route(source: usize, scope: Option<usize>) -> SourceRoute {
    SourceRoute {
        id: PlanSourceRouteId(source),
        source_id: SourceId(source),
        owner: scope.map_or_else(PlanOwner::root, |scope| PlanOwner {
            static_owner: PlanStaticOwnerId(source),
            ancestors: vec![PlanOwnerAncestor {
                static_owner: PlanStaticOwnerId(source),
                scope: ScopeId(scope),
                list: ListId(scope),
            }],
        }),
        path: format!("source.{source}"),
        scoped: scope.is_some(),
        scope_id: scope.map(ScopeId),
        row_projections: Vec::new(),
        interval_ms: None,
        payload_schema: SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
        },
    }
}

fn derived(
    id: usize,
    output: usize,
    inputs: Vec<ValueRef>,
    expression: Option<PlanRowExpressionId>,
) -> PlanOp {
    PlanOp {
        id: PlanOpId(id),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::Pure,
            startup_recompute: true,
            materialization: None,
            expression: expression
                .map(|expression| PlanDerivedExpression::RowExpression { expression }),
        },
        inputs,
        output: Some(ValueRef::Field(FieldId(output))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    }
}

fn contextual_collection(
    arena: &mut PlanRowExpressionArena,
    owner: usize,
    operation: PlanContextualOperationKind,
    source: PlanRowExpressionId,
    body: PlanRowExpressionId,
) -> PlanRowExpressionId {
    row(
        arena,
        PlanRowExpressionNode::ContextualCollection {
            owner: PlanStaticOwnerId(owner),
            operation,
            source,
            row_local: PlanLocalId(0),
            body,
            captures: Vec::new(),
            indexed_access: None,
        },
    )
}

fn contextual_local(
    arena: &mut PlanRowExpressionArena,
    owner: usize,
    projection: &[&str],
) -> PlanRowExpressionId {
    row(
        arena,
        PlanRowExpressionNode::Local {
            owner: PlanStaticOwnerId(owner),
            local: PlanLocalId(0),
            projection: projection.iter().map(|field| (*field).to_owned()).collect(),
        },
    )
}

fn contextual_row_field(
    arena: &mut PlanRowExpressionArena,
    owner: usize,
    list: usize,
    field: usize,
) -> PlanRowExpressionId {
    let local = row(
        arena,
        PlanRowExpressionNode::LocalRow {
            owner: PlanStaticOwnerId(owner),
            local: PlanLocalId(0),
        },
    );
    row(
        arena,
        PlanRowExpressionNode::ListRowField {
            row: local,
            list_id: ListId(list),
            field: FieldId(field),
        },
    )
}

fn text_field_index(
    arena: &mut PlanRowExpressionArena,
    index: usize,
    list: usize,
    field: usize,
) -> PlanListIndex {
    let local = row(
        arena,
        PlanRowExpressionNode::LocalRow {
            owner: PlanStaticOwnerId::ROOT,
            local: PlanLocalId(0),
        },
    );
    let expression = row(
        arena,
        PlanRowExpressionNode::ListRowField {
            row: local,
            list_id: ListId(list),
            field: FieldId(field),
        },
    );
    PlanListIndex {
        id: PlanListIndexId(index),
        source_list: ListId(list),
        keys: vec![PlanListIndexKey {
            owner: PlanStaticOwnerId::ROOT,
            row_local: PlanLocalId(0),
            expression,
            kind: PlanListIndexKeyKind::Text,
            closed_tags: Vec::new(),
            direction: PlanOrderDirection::Ascending,
            multiplicity: PlanListIndexKeyMultiplicity::One,
        }],
    }
}

fn const_update(
    arena: &mut PlanRowExpressionArena,
    id: usize,
    source: usize,
    state: usize,
    constant: usize,
) -> PlanOp {
    PlanOp {
        id: PlanOpId(id),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::Source(SourceId(source)),
            value: Some(row_constant(arena, PlanConstantId(constant))),
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(source))],
        output: Some(ValueRef::State(StateId(state))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    }
}

fn event(
    machine: &MachineInstance,
    sequence: u64,
    source: usize,
    target: Option<RowId>,
) -> SourceEvent {
    SourceEvent {
        sequence,
        source: SourceId(source),
        route: route_token(machine, SourceId(source), target),
        target,
        payload: SourcePayload::default(),
    }
}

fn route_token(
    machine: &MachineInstance,
    source: SourceId,
    target: Option<RowId>,
) -> SourceRouteToken {
    machine
        .source_route_token(source, target.as_slice())
        .unwrap()
}

#[test]
fn root_value_comparison_tracks_both_state_inputs() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let update = const_update(&mut row_expressions, 1, 0, 1, 1);
    let comparison = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::Pure,
            startup_recompute: true,
            materialization: None,
            expression: Some(PlanDerivedExpression::ValueCompare {
                left: ValueRef::State(StateId(0)),
                op: PlanInfixOp::Equal,
                right: ValueRef::State(StateId(1)),
            }),
        },
        inputs: vec![ValueRef::State(StateId(0)), ValueRef::State(StateId(1))],
        output: Some(ValueRef::Field(FieldId(0))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            vec![
                constant(0, number_constant(3)),
                constant(1, number_constant(4)),
            ],
            vec![route(0, None)],
            vec![number_slot(0, 0), number_slot(1, 0)],
            Vec::new(),
            vec![comparison, update],
            vec![(StateId(0), "store.left"), (StateId(1), "store.right")],
            Vec::new(),
            vec![(FieldId(0), "store.same")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    assert_eq!(
        session.root_value_current("store.same").unwrap(),
        Value::truth(true)
    );
    session.apply(event(&session, 1, 0, None)).unwrap();
    assert_eq!(
        session.root_value_current("store.same").unwrap(),
        Value::truth(false)
    );
}

#[cfg(feature = "phase0-instrumentation")]
#[test]
fn phase_elapsed_metrics_are_scoped_to_source_and_boundary_work() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let update = const_update(&mut row_expressions, 1, 0, 1, 1);
    let comparison = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::Pure,
            startup_recompute: true,
            materialization: None,
            expression: Some(PlanDerivedExpression::ValueCompare {
                left: ValueRef::State(StateId(0)),
                op: PlanInfixOp::Equal,
                right: ValueRef::State(StateId(1)),
            }),
        },
        inputs: vec![ValueRef::State(StateId(0)), ValueRef::State(StateId(1))],
        output: Some(ValueRef::Field(FieldId(0))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            vec![
                constant(0, number_constant(3)),
                constant(1, number_constant(4)),
            ],
            vec![route(0, None)],
            vec![number_slot(0, 0), number_slot(1, 0)],
            Vec::new(),
            vec![comparison, update],
            vec![(StateId(0), "store.left"), (StateId(1), "store.right")],
            Vec::new(),
            vec![(FieldId(0), "store.same")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    let (_, boundary_metrics) = session
        .root_value_current_with_metrics("store.same")
        .unwrap();
    assert_eq!(boundary_metrics.elapsed_ingest_ns, 0);
    assert_eq!(boundary_metrics.elapsed_evaluate_ns, 0);
    assert_eq!(boundary_metrics.elapsed_commit_ns, 0);
    assert_eq!(boundary_metrics.elapsed_delta_ns, 0);

    let turn = session.apply(event(&session, 1, 0, None)).unwrap();
    assert_eq!(turn.metrics.elapsed_boundary_ns, 0);
}

#[test]
fn fully_qualified_state_lookup_wins_over_an_unrelated_field_local_name() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let expression = row_constant(&mut row_expressions, PlanConstantId(1));
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            vec![
                constant(0, number_constant(1)),
                constant(1, number_constant(0)),
            ],
            Vec::new(),
            vec![number_slot(0, 0)],
            Vec::new(),
            vec![derived(
                0,
                0,
                vec![ValueRef::Constant(PlanConstantId(1))],
                Some(expression),
            )],
            vec![(StateId(0), "store.draft_revision")],
            Vec::new(),
            vec![(FieldId(0), "revision.draft_revision")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    assert_eq!(
        session.root_value_current("store.draft_revision").unwrap(),
        number(1)
    );
    assert_eq!(
        session.root_value_current("draft_revision").unwrap(),
        number(1)
    );
    assert_eq!(
        session
            .root_value_current("revision.draft_revision")
            .unwrap(),
        number(0)
    );
}

#[test]
fn authority_restore_preserves_touched_value_equal_to_old_default() {
    let make_plan = |default: i64| {
        let mut row_expressions = PlanRowExpressionArena::new();
        let (constants, update_constant) = if default == 0 {
            (vec![constant(0, number_constant(0))], 0)
        } else {
            (
                vec![
                    constant(0, number_constant(default)),
                    constant(1, number_constant(0)),
                ],
                1,
            )
        };
        let update = const_update(&mut row_expressions, 0, 0, 0, update_constant);
        plan(
            RootOutputDemand::Selected(Vec::new()),
            row_expressions,
            constants,
            vec![route(0, None)],
            vec![number_slot(0, 0)],
            Vec::new(),
            vec![update],
            vec![(StateId(0), "count")],
            Vec::new(),
            Vec::new(),
        )
    };

    let untouched = MachineInstance::new(make_plan(0), SessionOptions::default()).unwrap();
    let semantic_default = untouched.semantic_value_image().unwrap();
    let mut original = MachineInstance::new(make_plan(0), SessionOptions::default()).unwrap();
    let turn = original.apply(event(&original, 1, 0, None)).unwrap();
    assert!(turn.deltas.is_empty());
    assert_eq!(
        turn.authority_deltas,
        vec![AuthorityDelta::SetRoot {
            state: StateId(0),
            value: number(0),
        }]
    );
    let authority = original.authority_snapshot().unwrap();
    assert!(authority.states[&StateId(0)].touched);
    assert_eq!(authority.through_turn_sequence, 1);

    let durable = original
        .durable_restore_image(7, Default::default())
        .unwrap();
    assert_eq!(durable.epoch, 7);
    assert_eq!(durable.scalars.len(), 1);
    assert_eq!(original.semantic_value_image().unwrap(), semantic_default);

    let restored = MachineInstanceBuilder::new(make_plan(10), SessionOptions::default())
        .unwrap()
        .restore_durable(durable)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(
        restored.authority_snapshot().unwrap().states[&StateId(0)].value,
        number(0)
    );
}

#[test]
fn failed_turn_rolls_back_authority_and_touch_provenance() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let updates = vec![
        const_update(&mut row_expressions, 0, 0, 0, 1),
        const_update(&mut row_expressions, 1, 0, 1, 99),
    ];
    let machine = plan(
        RootOutputDemand::Selected(Vec::new()),
        row_expressions,
        vec![
            constant(0, number_constant(1)),
            constant(1, number_constant(2)),
        ],
        vec![route(0, None)],
        vec![number_slot(0, 0), number_slot(1, 0)],
        Vec::new(),
        updates,
        vec![(StateId(0), "first"), (StateId(1), "second")],
        Vec::new(),
        Vec::new(),
    );
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let before = session.authority_snapshot().unwrap();

    assert!(session.apply(event(&session, 1, 0, None)).is_err());
    assert_eq!(session.authority_snapshot().unwrap(), before);
}

#[test]
fn unsettled_turn_can_rollback_authority_sequence_and_durable_delta() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let update = const_update(&mut row_expressions, 0, 0, 0, 1);
    let machine = plan(
        RootOutputDemand::Selected(Vec::new()),
        row_expressions,
        vec![
            constant(0, number_constant(1)),
            constant(1, number_constant(2)),
        ],
        vec![route(0, None)],
        vec![number_slot(0, 0)],
        Vec::new(),
        vec![update],
        vec![(StateId(0), "count")],
        Vec::new(),
        Vec::new(),
    );
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let before = session.authority_snapshot().unwrap();

    let turn = session.apply(event(&session, 1, 0, None)).unwrap();
    assert_eq!(turn.durable_changes.len(), 1);
    assert_eq!(
        session.authority_snapshot().unwrap().through_turn_sequence,
        1
    );

    session.rollback_unsettled_turn().unwrap();
    assert_eq!(session.authority_snapshot().unwrap(), before);

    let retried = session.apply(event(&session, 1, 0, None)).unwrap();
    assert_eq!(retried.durable_changes, turn.durable_changes);
    session.settle_turn();
    assert_eq!(
        session.authority_snapshot().unwrap().through_turn_sequence,
        1
    );
}

#[test]
fn contextual_any_evaluates_typed_local_projections() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let row = |selected: bool| PlanInitialListRow {
        fields: vec![PlanInitialListField {
            name: "selected".into(),
            field_id: Some(FieldId(10)),
            initializer: initial(truth_constant(selected)),
        }],
    };
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: None,
        row_fields: vec![PlanListRowField {
            field_id: FieldId(10),
            name: "selected".into(),
            role: PlanListRowFieldRole::Authority,
        }],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![row(false), row(true)],
    };
    let source = row_list_ref(&mut row_expressions, ListId(0));
    let body = contextual_row_field(&mut row_expressions, 0, 0, 10);
    let expression = contextual_collection(
        &mut row_expressions,
        0,
        PlanContextualOperationKind::Any,
        source,
        body,
    );
    let session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![list],
            vec![derived(
                0,
                20,
                vec![ValueRef::List(ListId(0))],
                Some(expression),
            )],
            Vec::new(),
            vec![(ListId(0), "rows")],
            vec![(FieldId(10), "rows.selected"), (FieldId(20), "any")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    assert_eq!(
        session.snapshot().unwrap().fields[&FieldId(20)],
        Value::truth(true)
    );
}

#[test]
fn contextual_collection_operations_cover_map_filter_retain_every_any_and_find() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let row = |value: i64, keep: bool| PlanInitialListRow {
        fields: vec![
            PlanInitialListField {
                name: "value".into(),
                field_id: Some(FieldId(10)),
                initializer: initial(number_constant(value)),
            },
            PlanInitialListField {
                name: "keep".into(),
                field_id: Some(FieldId(11)),
                initializer: initial(truth_constant(keep)),
            },
        ],
    };
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: None,
        row_fields: vec![
            PlanListRowField {
                field_id: FieldId(10),
                name: "value".into(),
                role: PlanListRowFieldRole::Authority,
            },
            PlanListRowField {
                field_id: FieldId(11),
                name: "keep".into(),
                role: PlanListRowFieldRole::Authority,
            },
        ],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![row(1, false), row(2, true), row(3, true)],
    };
    let mut operations = Vec::new();
    for (id, operation, field) in [
        (0, PlanContextualOperationKind::Map, 10),
        (1, PlanContextualOperationKind::Filter, 11),
        (2, PlanContextualOperationKind::Retain, 11),
        (3, PlanContextualOperationKind::Every, 11),
        (4, PlanContextualOperationKind::Any, 11),
        (5, PlanContextualOperationKind::Find, 11),
    ] {
        let source = row_list_ref(&mut row_expressions, ListId(0));
        let body = contextual_row_field(&mut row_expressions, id, 0, field);
        let expression = contextual_collection(&mut row_expressions, id, operation, source, body);
        operations.push(derived(
            id,
            20 + id,
            vec![ValueRef::List(ListId(0))],
            Some(expression),
        ));
    }
    let source = row_list_ref(&mut row_expressions, ListId(0));
    let body = row_constant(&mut row_expressions, PlanConstantId(0));
    let expression = contextual_collection(
        &mut row_expressions,
        6,
        PlanContextualOperationKind::Find,
        source,
        body,
    );
    operations.push(derived(
        6,
        26,
        vec![ValueRef::List(ListId(0))],
        Some(expression),
    ));
    let session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            vec![constant(0, truth_constant(false))],
            Vec::new(),
            Vec::new(),
            vec![list],
            operations,
            Vec::new(),
            vec![(ListId(0), "rows")],
            vec![
                (FieldId(10), "rows.value"),
                (FieldId(11), "rows.keep"),
                (FieldId(20), "mapped"),
                (FieldId(21), "filtered"),
                (FieldId(22), "retained"),
                (FieldId(23), "every"),
                (FieldId(24), "any"),
                (FieldId(25), "found"),
                (FieldId(26), "not_found"),
            ],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    let snapshot = session.snapshot().unwrap();
    assert_eq!(
        snapshot.fields[&FieldId(20)],
        Value::List(vec![number(1), number(2), number(3)])
    );
    let expected_rows = snapshot.lists[&ListId(0)][1..]
        .iter()
        .map(|row| Value::Row {
            id: row.id,
            fields: BTreeMap::new(),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        snapshot.fields[&FieldId(21)],
        Value::List(expected_rows.clone())
    );
    assert_eq!(snapshot.fields[&FieldId(22)], Value::List(expected_rows));
    assert_eq!(snapshot.fields[&FieldId(23)], Value::truth(false));
    assert_eq!(snapshot.fields[&FieldId(24)], Value::truth(true));
    assert_eq!(
        snapshot.fields[&FieldId(25)],
        Value::tagged(
            "Found",
            BTreeMap::from([(
                "value".to_owned(),
                Value::Row {
                    id: snapshot.lists[&ListId(0)][1].id,
                    fields: BTreeMap::new(),
                },
            )]),
        )
    );
    assert_eq!(snapshot.fields[&FieldId(26)], Value::tag("NotFound"));
}

#[test]
fn nested_contextual_collections_disambiguate_same_local_id_by_owner() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let outer_items =
        [0, 1].map(|constant_id| row_constant(&mut row_expressions, PlanConstantId(constant_id)));
    let outer_source = row(
        &mut row_expressions,
        PlanRowExpressionNode::ListLiteral {
            items: outer_items.to_vec(),
        },
    );
    let inner_items =
        [2, 3].map(|constant_id| row_constant(&mut row_expressions, PlanConstantId(constant_id)));
    let inner_source = row(
        &mut row_expressions,
        PlanRowExpressionNode::ListLiteral {
            items: inner_items.to_vec(),
        },
    );
    let left = contextual_local(&mut row_expressions, 0, &[]);
    let right = contextual_local(&mut row_expressions, 1, &[]);
    let sum = row(
        &mut row_expressions,
        PlanRowExpressionNode::NumberInfix {
            op: PlanInfixOp::Add,
            left,
            right,
        },
    );
    let inner = contextual_collection(
        &mut row_expressions,
        1,
        PlanContextualOperationKind::Map,
        inner_source,
        sum,
    );
    let expression = contextual_collection(
        &mut row_expressions,
        0,
        PlanContextualOperationKind::Map,
        outer_source,
        inner,
    );
    let session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            [1, 10, 2, 3]
                .into_iter()
                .enumerate()
                .map(|(id, value)| constant(id, number_constant(value)))
                .collect(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![derived(
                0,
                20,
                (0..4)
                    .map(|id| ValueRef::Constant(PlanConstantId(id)))
                    .collect(),
                Some(expression),
            )],
            Vec::new(),
            Vec::new(),
            vec![(FieldId(20), "nested")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    assert_eq!(
        session.snapshot().unwrap().fields[&FieldId(20)],
        Value::List(vec![
            Value::List(vec![number(3), number(4)]),
            Value::List(vec![number(12), number(13)]),
        ])
    );
}

#[test]
fn contextual_collection_validation_visitors_and_hashing_are_structural() {
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: None,
        row_fields: vec![PlanListRowField {
            field_id: FieldId(10),
            name: "label".into(),
            role: PlanListRowFieldRole::Authority,
        }],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![PlanInitialListField {
                name: "label".into(),
                field_id: Some(FieldId(10)),
                initializer: initial(PlanConstantValue::Text {
                    value: "first".into(),
                }),
            }],
        }],
    };
    let expression =
        |owner: usize, operation: PlanContextualOperationKind, local: usize, projection: &str| {
            let mut row_expressions = PlanRowExpressionArena::new();
            let source = row_field(&mut row_expressions, ValueRef::List(ListId(0)));
            let local_value = row(
                &mut row_expressions,
                PlanRowExpressionNode::Local {
                    owner: PlanStaticOwnerId(owner),
                    local: PlanLocalId(local),
                    projection: vec![projection.to_owned()],
                },
            );
            let status = row(
                &mut row_expressions,
                PlanRowExpressionNode::Intrinsic {
                    intrinsic: PlanIntrinsic::SessionInfoStatus,
                },
            );
            let body = row(
                &mut row_expressions,
                PlanRowExpressionNode::Object {
                    fields: vec![
                        PlanRowObjectField {
                            name: "row".into(),
                            value: local_value,
                            spread: false,
                        },
                        PlanRowObjectField {
                            name: "status".into(),
                            value: status,
                            spread: false,
                        },
                    ],
                },
            );
            let root = row(
                &mut row_expressions,
                PlanRowExpressionNode::ContextualCollection {
                    owner: PlanStaticOwnerId(owner),
                    operation,
                    source,
                    row_local: PlanLocalId(local),
                    body,
                    captures: Vec::new(),
                    indexed_access: None,
                },
            );
            (row_expressions, root)
        };
    let machine = |(row_expressions, expression)| {
        plan(
            RootOutputDemand::All,
            row_expressions,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![list.clone()],
            vec![derived(
                0,
                20,
                vec![ValueRef::List(ListId(0))],
                Some(expression),
            )],
            Vec::new(),
            vec![(ListId(0), "rows")],
            vec![(FieldId(10), "rows.label"), (FieldId(20), "mapped")],
        )
    };

    let (valid_row_expressions, valid_expression) =
        expression(7, PlanContextualOperationKind::Map, 3, "label");
    let mut refs = Vec::new();
    valid_row_expressions
        .visit_value_refs(valid_expression, &mut |value| refs.push(value.clone()))
        .unwrap();
    assert_eq!(refs, vec![ValueRef::List(ListId(0))]);
    let mut intrinsics = Vec::new();
    valid_row_expressions
        .visit_intrinsics(valid_expression, &mut |intrinsic| {
            intrinsics.push(intrinsic)
        })
        .unwrap();
    assert_eq!(intrinsics, vec![PlanIntrinsic::SessionInfoStatus]);

    let valid_plan = machine((valid_row_expressions, valid_expression));
    let valid_verification = verify_plan(&valid_plan).unwrap();
    assert!(
        valid_verification
            .checks
            .iter()
            .any(|check| { check.id == "row-expression-contextual-locals-resolve" && check.pass })
    );

    let mut invalid_row_expressions = PlanRowExpressionArena::new();
    let invalid_source = row_list_ref(&mut invalid_row_expressions, ListId(0));
    let invalid_body = row(
        &mut invalid_row_expressions,
        PlanRowExpressionNode::Local {
            owner: PlanStaticOwnerId(8),
            local: PlanLocalId(3),
            projection: Vec::new(),
        },
    );
    let invalid_expression = row(
        &mut invalid_row_expressions,
        PlanRowExpressionNode::ContextualCollection {
            owner: PlanStaticOwnerId(7),
            operation: PlanContextualOperationKind::Map,
            source: invalid_source,
            row_local: PlanLocalId(3),
            body: invalid_body,
            captures: Vec::new(),
            indexed_access: None,
        },
    );
    let invalid_plan = machine((invalid_row_expressions, invalid_expression));
    let invalid_verification = verify_plan(&invalid_plan).unwrap();
    assert!(
        invalid_verification
            .checks
            .iter()
            .any(|check| { check.id == "row-expression-contextual-locals-resolve" && !check.pass })
    );

    let base_hash = plan_sha256(&valid_plan).unwrap();
    assert_eq!(base_hash, plan_sha256(&valid_plan).unwrap());
    for changed in [
        expression(8, PlanContextualOperationKind::Map, 3, "label"),
        expression(7, PlanContextualOperationKind::Filter, 3, "label"),
        expression(7, PlanContextualOperationKind::Map, 4, "label"),
        expression(7, PlanContextualOperationKind::Map, 3, "other"),
    ] {
        assert_ne!(base_hash, plan_sha256(&machine(changed)).unwrap());
    }
}

#[test]
fn dynamic_row_dependencies_invalidate_consumers_across_lists() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let source_rows = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_fields: vec![
            PlanListRowField {
                field_id: FieldId(10),
                name: "key".into(),
                role: PlanListRowFieldRole::Authority,
            },
            PlanListRowField {
                field_id: FieldId(11),
                name: "selected".into(),
                role: PlanListRowFieldRole::Value,
            },
            PlanListRowField {
                field_id: FieldId(12),
                name: "initial".into(),
                role: PlanListRowFieldRole::Authority,
            },
        ],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![
                PlanInitialListField {
                    name: "key".into(),
                    field_id: Some(FieldId(10)),
                    initializer: initial(PlanConstantValue::Text {
                        value: "candidate".into(),
                    }),
                },
                PlanInitialListField {
                    name: "initial".into(),
                    field_id: Some(FieldId(12)),
                    initializer: initial(truth_constant(false)),
                },
            ],
        }],
    };
    let projected_rows = ListStorageSlot {
        id: PlanStorageId(1),
        list_id: ListId(1),
        scope_id: None,
        row_fields: vec![
            PlanListRowField {
                field_id: FieldId(20),
                name: "id".into(),
                role: PlanListRowFieldRole::Authority,
            },
            PlanListRowField {
                field_id: FieldId(21),
                name: "selected".into(),
                role: PlanListRowFieldRole::Value,
            },
        ],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![PlanInitialListField {
                name: "id".into(),
                field_id: Some(FieldId(20)),
                initializer: initial(PlanConstantValue::Text {
                    value: "projected".into(),
                }),
            }],
        }],
    };
    let selected_state = ScalarStorageSlot {
        id: PlanStorageId(2),
        state_id: StateId(0),
        owner: PlanOwner {
            static_owner: PlanStaticOwnerId(0),
            ancestors: vec![PlanOwnerAncestor {
                static_owner: PlanStaticOwnerId(0),
                scope: ScopeId(0),
                list: ListId(0),
            }],
        },
        value_type: PlanValueType::Tag,
        scope_id: Some(ScopeId(0)),
        indexed: true,
        indexed_field_id: Some(FieldId(11)),
        lifetime: PlanStateLifetime::Persistent,
        initializer: ScalarInitializerPlan::Expression {
            expression: row_field(&mut row_expressions, ValueRef::Field(FieldId(12))),
        },
    };
    let select_route = SourceRoute {
        id: PlanSourceRouteId(0),
        source_id: SourceId(0),
        owner: PlanOwner {
            static_owner: PlanStaticOwnerId(0),
            ancestors: vec![PlanOwnerAncestor {
                static_owner: PlanStaticOwnerId(0),
                scope: ScopeId(0),
                list: ListId(0),
            }],
        },
        path: "source.select".into(),
        scoped: true,
        scope_id: Some(ScopeId(0)),
        row_projections: Vec::new(),
        interval_ms: None,
        payload_schema: SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
        },
    };
    let select_update = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::Source(SourceId(0)),
            value: Some(row_constant(&mut row_expressions, PlanConstantId(0))),
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0))],
        output: Some(ValueRef::State(StateId(0))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let projected_source = row_list_ref(&mut row_expressions, ListId(0));
    let projected_body = contextual_row_field(&mut row_expressions, 1, 0, 11);
    let projected_expression = contextual_collection(
        &mut row_expressions,
        1,
        PlanContextualOperationKind::Any,
        projected_source,
        projected_body,
    );
    let projected_selected = PlanOp {
        id: PlanOpId(1),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::Pure,
            startup_recompute: true,
            materialization: None,
            expression: Some(PlanDerivedExpression::RowExpression {
                expression: projected_expression,
            }),
        },
        inputs: vec![ValueRef::List(ListId(0))],
        output: Some(ValueRef::Field(FieldId(21))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let visible_source = row_list_ref(&mut row_expressions, ListId(1));
    let visible_body = contextual_row_field(&mut row_expressions, 2, 1, 21);
    let visible_expression = contextual_collection(
        &mut row_expressions,
        2,
        PlanContextualOperationKind::Filter,
        visible_source,
        visible_body,
    );
    let visible_rows = derived(
        2,
        30,
        vec![ValueRef::List(ListId(1))],
        Some(visible_expression),
    );
    let mut machine_plan = plan(
        RootOutputDemand::All,
        row_expressions,
        vec![constant(0, truth_constant(true))],
        vec![select_route],
        vec![selected_state],
        vec![source_rows, projected_rows],
        vec![select_update, projected_selected, visible_rows],
        vec![(StateId(0), "source.selected")],
        vec![(ListId(0), "source"), (ListId(1), "projected")],
        vec![
            (FieldId(10), "source.key"),
            (FieldId(11), "source.selected"),
            (FieldId(12), "source.initial"),
            (FieldId(20), "projected.id"),
            (FieldId(21), "projected.selected"),
            (FieldId(30), "visible"),
        ],
    );
    machine_plan.list_indexes.push(text_field_index(
        &mut machine_plan.row_expressions,
        0,
        0,
        10,
    ));
    let mut session = MachineInstance::new(machine_plan, SessionOptions::default()).unwrap();

    assert!(matches!(
        session.snapshot().unwrap().fields[&FieldId(30)],
        Value::List(ref rows) if rows.is_empty()
    ));

    session
        .apply(event(
            &session,
            1,
            0,
            Some(RowId {
                list: ListId(0),
                key: 1,
                generation: 1,
            }),
        ))
        .unwrap();

    assert!(matches!(
        session.snapshot().unwrap().fields[&FieldId(30)],
        Value::List(ref rows) if rows.len() == 1
    ));
}

#[test]
fn mapped_range_initializes_range_columns() {
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: None,
        row_fields: vec![
            PlanListRowField {
                field_id: FieldId(10),
                name: "index".into(),
                role: PlanListRowFieldRole::Authority,
            },
            PlanListRowField {
                field_id: FieldId(11),
                name: "value".into(),
                role: PlanListRowFieldRole::Authority,
            },
        ],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::Range,
        range: Some(PlanRangeInitializer { from: 3, to: 4 }),
        initial_rows: Vec::new(),
    };
    let session = MachineInstance::new(
        plan(
            RootOutputDemand::Selected(Vec::new()),
            PlanRowExpressionArena::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![list],
            Vec::new(),
            Vec::new(),
            vec![(ListId(0), "items")],
            vec![(FieldId(10), "items.index"), (FieldId(11), "items.value")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    let rows = &session.snapshot().unwrap().lists[&ListId(0)];
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].fields[&FieldId(10)], Value::integer(3).unwrap());
    assert_eq!(rows[0].fields[&FieldId(11)], Value::integer(3).unwrap());
    assert_eq!(rows[1].fields[&FieldId(10)], Value::integer(4).unwrap());
    assert_eq!(rows[1].fields[&FieldId(11)], Value::integer(4).unwrap());
}

#[test]
fn unscoped_source_updates_every_row_owned_by_indexed_state() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let initial_row = |id: &str| PlanInitialListRow {
        fields: vec![
            PlanInitialListField {
                name: "id".into(),
                field_id: Some(FieldId(10)),
                initializer: initial(PlanConstantValue::Text { value: id.into() }),
            },
            PlanInitialListField {
                name: "initial".into(),
                field_id: Some(FieldId(12)),
                initializer: initial(tag_constant("Hexadecimal")),
            },
        ],
    };
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_fields: vec![
            PlanListRowField {
                field_id: FieldId(10),
                name: "id".into(),
                role: PlanListRowFieldRole::Authority,
            },
            PlanListRowField {
                field_id: FieldId(11),
                name: "formatter".into(),
                role: PlanListRowFieldRole::Value,
            },
            PlanListRowField {
                field_id: FieldId(12),
                name: "initial".into(),
                role: PlanListRowFieldRole::Authority,
            },
        ],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![initial_row("active"), initial_row("other")],
    };
    let indexed_state = ScalarStorageSlot {
        id: PlanStorageId(1),
        state_id: StateId(0),
        owner: PlanOwner {
            static_owner: PlanStaticOwnerId(0),
            ancestors: vec![PlanOwnerAncestor {
                static_owner: PlanStaticOwnerId(0),
                scope: ScopeId(0),
                list: ListId(0),
            }],
        },
        value_type: PlanValueType::Tag,
        scope_id: Some(ScopeId(0)),
        indexed: true,
        indexed_field_id: Some(FieldId(11)),
        lifetime: PlanStateLifetime::Persistent,
        initializer: ScalarInitializerPlan::Expression {
            expression: row_field(&mut row_expressions, ValueRef::Field(FieldId(12))),
        },
    };
    let row_id = row_field(&mut row_expressions, ValueRef::Field(FieldId(10)));
    let active = row_constant(&mut row_expressions, PlanConstantId(1));
    let is_active = row(
        &mut row_expressions,
        PlanRowExpressionNode::NumberInfix {
            op: PlanInfixOp::Equal,
            left: row_id,
            right: active,
        },
    );
    let current = row_field(&mut row_expressions, ValueRef::State(StateId(0)));
    let binary = row_constant(&mut row_expressions, PlanConstantId(7));
    let toggled = row(
        &mut row_expressions,
        PlanRowExpressionNode::Select {
            input: current,
            arms: vec![
                PlanRowSelectArm {
                    pattern: PlanRowSelectPattern::Tag {
                        name: "Hexadecimal".to_owned(),
                    },
                    value: binary,
                },
                PlanRowSelectArm {
                    pattern: PlanRowSelectPattern::Wildcard,
                    value: current,
                },
            ],
        },
    );
    let next = row(
        &mut row_expressions,
        PlanRowExpressionNode::Select {
            input: is_active,
            arms: vec![
                PlanRowSelectArm {
                    pattern: PlanRowSelectPattern::Tag {
                        name: "True".to_owned(),
                    },
                    value: toggled,
                },
                PlanRowSelectArm {
                    pattern: PlanRowSelectPattern::Wildcard,
                    value: current,
                },
            ],
        },
    );
    let update = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::Source(SourceId(0)),
            value: Some(next),
            effect: None,
        },
        inputs: vec![
            ValueRef::Source(SourceId(0)),
            ValueRef::Field(FieldId(10)),
            ValueRef::State(StateId(0)),
        ],
        output: Some(ValueRef::State(StateId(0))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::Selected(Vec::new()),
            row_expressions,
            vec![
                constant(0, PlanConstantValue::Text { value: "==".into() }),
                constant(
                    1,
                    PlanConstantValue::Text {
                        value: "active".into(),
                    },
                ),
                constant(
                    2,
                    PlanConstantValue::Text {
                        value: "True".into(),
                    },
                ),
                constant(
                    3,
                    PlanConstantValue::Text {
                        value: "match_const".into(),
                    },
                ),
                constant(4, number_constant(2)),
                constant(
                    5,
                    PlanConstantValue::Text {
                        value: "Hexadecimal".into(),
                    },
                ),
                constant(
                    6,
                    PlanConstantValue::Text {
                        value: "ref".into(),
                    },
                ),
                constant(7, tag_constant("Binary")),
                constant(8, PlanConstantValue::Text { value: "__".into() }),
                constant(
                    9,
                    PlanConstantValue::Text {
                        value: "False".into(),
                    },
                ),
            ],
            vec![route(0, None)],
            vec![indexed_state],
            vec![list],
            vec![update],
            vec![(StateId(0), "items.formatter")],
            vec![(ListId(0), "items")],
            vec![
                (FieldId(10), "items.id"),
                (FieldId(11), "items.formatter"),
                (FieldId(12), "items.initial"),
            ],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    session.apply(event(&session, 1, 0, None)).unwrap();

    let rows = &session.snapshot().unwrap().lists[&ListId(0)];
    assert_eq!(rows[0].fields[&FieldId(11)], Value::tag("Binary"));
    assert_eq!(rows[1].fields[&FieldId(11)], Value::tag("Hexadecimal"));
}

#[test]
fn machine_build_task_slices_full_authority_restore_and_runtime_rebuild() {
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: None,
        row_fields: vec![
            PlanListRowField {
                field_id: FieldId(10),
                name: "index".into(),
                role: PlanListRowFieldRole::Authority,
            },
            PlanListRowField {
                field_id: FieldId(11),
                name: "value".into(),
                role: PlanListRowFieldRole::Authority,
            },
        ],
        capacity: None,
        hidden_key_type: "ItemKey".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::Range,
        range: Some(PlanRangeInitializer { from: 0, to: 999 }),
        initial_rows: Vec::new(),
    };
    let machine = plan(
        RootOutputDemand::Selected(Vec::new()),
        PlanRowExpressionArena::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![list.clone()],
        Vec::new(),
        Vec::new(),
        vec![(list.list_id, "items")],
        vec![(FieldId(10), "items.index"), (FieldId(11), "items.value")],
    );
    let authority_fields = list
        .row_fields
        .iter()
        .filter(|field| field.role.is_authority())
        .map(|field| field.field_id)
        .collect::<BTreeSet<_>>();
    assert!(!authority_fields.is_empty());
    let rows = (0_u64..1_000)
        .map(|offset| {
            let id = RowId {
                list: list.list_id,
                key: offset + 1,
                generation: 1,
            };
            RowAuthority {
                id,
                source_order_token: u128::from(offset + 1),
                owner_ancestors: vec![OwnerInstanceRow {
                    list: id.list,
                    key: id.key,
                    generation: id.generation,
                }],
                materialization_origin: None,
                fields: authority_fields
                    .iter()
                    .map(|field| (*field, number(offset as i64)))
                    .collect(),
                touched_fields: authority_fields.clone(),
            }
        })
        .collect();
    let authority = AuthoritySnapshot {
        through_turn_sequence: 17,
        states: BTreeMap::new(),
        lists: BTreeMap::from([(
            list.list_id,
            ListAuthority {
                touched: true,
                revision: 9,
                next_key: 1_001,
                next_order_token: 1_001,
                rows,
            },
        )]),
        maps: BTreeMap::new(),
        sets: BTreeMap::new(),
    };
    let synchronous = MachineInstanceBuilder::new(machine.clone(), SessionOptions::default())
        .unwrap()
        .restore(authority.clone())
        .build()
        .unwrap();
    let durable_machine = machine.clone();
    let mut task = MachineInstanceBuilder::new(machine, SessionOptions::default())
        .unwrap()
        .restore(authority)
        .into_build_task();
    let mut restore_polls = 0_u64;
    let mut rebuild_polls = 0_u64;
    let sliced = loop {
        match task.poll(1).unwrap() {
            MachineBuildPoll::Pending(progress) => match progress.phase {
                MachineBuildPhase::RestoreAuthority => restore_polls += 1,
                MachineBuildPhase::RuntimeState => rebuild_polls += 1,
                _ => {}
            },
            MachineBuildPoll::Ready(session) => break session,
        }
    };
    assert!(
        restore_polls >= 2_000,
        "restore rows were not cooperatively sliced"
    );
    assert!(
        rebuild_polls >= 2_000,
        "runtime-state reconstruction was not cooperatively sliced"
    );
    assert_eq!(sliced.snapshot().unwrap(), synchronous.snapshot().unwrap());
    assert_eq!(
        sliced.authority_snapshot().unwrap(),
        synchronous.authority_snapshot().unwrap()
    );
    assert_eq!(sliced.startup_metrics(), synchronous.startup_metrics());

    let durable = synchronous
        .durable_restore_image(3, BTreeSet::new())
        .unwrap();
    let mut task = MachineInstanceBuilder::new(durable_machine, SessionOptions::default())
        .unwrap()
        .restore_durable(durable)
        .unwrap()
        .into_build_task();
    let mut translation_polls = 0_u64;
    let mut authority_polls = 0_u64;
    let durable_sliced = loop {
        match task.poll(1).unwrap() {
            MachineBuildPoll::Pending(progress) => match progress.phase {
                MachineBuildPhase::TranslateDurableRestore => translation_polls += 1,
                MachineBuildPhase::RestoreAuthority => authority_polls += 1,
                _ => {}
            },
            MachineBuildPoll::Ready(session) => break session,
        }
    };
    assert!(
        translation_polls >= 1_000,
        "durable row translation was not cooperatively sliced"
    );
    assert!(
        authority_polls >= 2_000,
        "translated durable authority was not cooperatively applied"
    );
    assert_eq!(
        durable_sliced.snapshot().unwrap(),
        synchronous.snapshot().unwrap()
    );
    let restored_authority = durable_sliced.authority_snapshot().unwrap();
    assert!(
        !restored_authority.lists[&list.list_id].touched,
        "a reconstructable static row domain restores sparse field overlays, not a complete durable replacement"
    );
}

#[test]
fn list_map_records_preserve_source_row_identity() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let list = ListStorageSlot {
        id: PlanStorageId(1),
        list_id: ListId(0),
        scope_id: None,
        row_fields: vec![PlanListRowField {
            field_id: FieldId(10),
            name: "label".into(),
            role: PlanListRowFieldRole::Authority,
        }],
        capacity: None,
        hidden_key_type: "Key".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![PlanInitialListField {
                name: "label".into(),
                field_id: Some(FieldId(10)),
                initializer: initial(PlanConstantValue::Text {
                    value: "first".into(),
                }),
            }],
        }],
    };
    let spread = contextual_local(&mut row_expressions, 0, &[]);
    let title = contextual_row_field(&mut row_expressions, 0, 0, 10);
    let body = row(
        &mut row_expressions,
        PlanRowExpressionNode::Object {
            fields: vec![
                PlanRowObjectField {
                    name: String::new(),
                    value: spread,
                    spread: true,
                },
                PlanRowObjectField {
                    name: "title".into(),
                    value: title,
                    spread: false,
                },
            ],
        },
    );
    let source = row_list_ref(&mut row_expressions, ListId(0));
    let expression = contextual_collection(
        &mut row_expressions,
        0,
        PlanContextualOperationKind::Map,
        source,
        body,
    );
    let map = derived(0, 0, vec![ValueRef::List(ListId(0))], Some(expression));
    let session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![list],
            vec![map],
            Vec::new(),
            vec![(ListId(0), "items")],
            vec![(FieldId(0), "mapped"), (FieldId(10), "items.label")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    let snapshot = session.snapshot().unwrap();
    let source_row = snapshot.lists[&ListId(0)][0].id;
    let Value::List(mapped) = &snapshot.fields[&FieldId(0)] else {
        panic!("mapped value is not a list");
    };
    let Value::MappedRow { id, fields } = &mapped[0] else {
        panic!("List/map object result lost its source row identity");
    };
    assert_eq!(*id, source_row);
    assert_eq!(fields["label"], Value::Text("first".into()));
    assert_eq!(fields["title"], Value::Text("first".into()));
}

#[test]
fn selected_demand_stays_current_without_eager_unrequested_work() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let demanded_expression = row_field(&mut row_expressions, ValueRef::State(StateId(0)));
    let demanded = derived(
        0,
        0,
        vec![ValueRef::State(StateId(0))],
        Some(demanded_expression),
    );
    let unsupported_unrequested = derived(1, 1, Vec::new(), None);
    let update = const_update(&mut row_expressions, 2, 0, 0, 1);
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::Selected(vec![FieldId(0)]),
            row_expressions,
            vec![
                constant(0, number_constant(1)),
                constant(1, number_constant(2)),
            ],
            vec![route(0, None)],
            vec![number_slot(0, 0)],
            Vec::new(),
            vec![demanded, unsupported_unrequested, update],
            vec![(StateId(0), "count")],
            Vec::new(),
            vec![(FieldId(0), "current"), (FieldId(1), "unused")],
        ),
        SessionOptions::default(),
    )
    .unwrap();
    assert_eq!(
        session
            .project_current(&[ValueTarget::Field(FieldId(0))])
            .unwrap()[&ValueTarget::Field(FieldId(0))],
        number(1)
    );
    assert_eq!(
        session
            .project_current(&[ValueTarget::Field(FieldId(1))])
            .unwrap_err(),
        Error::NotDemanded(FieldId(1))
    );

    let turn = session.apply(event(&session, 1, 0, None)).unwrap();
    assert_eq!(turn.metrics.recomputed_field_count, 1);
    assert_eq!(session.snapshot().unwrap().fields[&FieldId(0)], number(2));
}

fn deep_acyclic_dependency_plan(depth: usize) -> MachinePlan {
    assert!(depth > 0);
    let mut row_expressions = PlanRowExpressionArena::new();
    let mut ops = Vec::with_capacity(depth);
    for field in 0..depth {
        let (inputs, expression) = if field == 0 {
            (
                vec![ValueRef::Constant(PlanConstantId(0))],
                row_constant(&mut row_expressions, PlanConstantId(0)),
            )
        } else {
            let input = ValueRef::Field(FieldId(field - 1));
            (vec![input.clone()], row_field(&mut row_expressions, input))
        };
        ops.push(derived(field, field, inputs, Some(expression)));
    }
    let labels = (0..depth)
        .map(|field| format!("chain.{field}"))
        .collect::<Vec<_>>();
    let field_labels = labels
        .iter()
        .enumerate()
        .map(|(field, label)| (FieldId(field), label.as_str()))
        .collect();
    plan(
        RootOutputDemand::Selected(Vec::new()),
        row_expressions,
        vec![constant(0, number_constant(7))],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        ops,
        Vec::new(),
        Vec::new(),
        field_labels,
    )
}

fn state_backed_deep_dependency_plan(depth: usize) -> MachinePlan {
    assert!(depth > 0);
    let mut row_expressions = PlanRowExpressionArena::new();
    let mut ops = Vec::with_capacity(depth + 1);
    for field in 0..depth {
        let input = if field == 0 {
            ValueRef::State(StateId(0))
        } else {
            ValueRef::Field(FieldId(field - 1))
        };
        let expression = row_field(&mut row_expressions, input.clone());
        ops.push(derived(field, field, vec![input], Some(expression)));
    }
    ops.push(const_update(&mut row_expressions, depth, 0, 0, 1));
    let labels = (0..depth)
        .map(|field| format!("chain.{field}"))
        .collect::<Vec<_>>();
    let field_labels = labels
        .iter()
        .enumerate()
        .map(|(field, label)| (FieldId(field), label.as_str()))
        .collect();
    plan(
        RootOutputDemand::Selected(Vec::new()),
        row_expressions,
        vec![
            constant(0, number_constant(7)),
            constant(1, number_constant(11)),
        ],
        vec![route(0, None)],
        vec![number_slot(0, 0)],
        Vec::new(),
        ops,
        vec![(StateId(0), "chain.state")],
        Vec::new(),
        field_labels,
    )
}

#[test]
fn default_stack_state_backed_dependency_chain_survives_dirty_and_redemand() {
    const DEPTH: usize = 4_096;
    let mut session = MachineInstance::new(
        state_backed_deep_dependency_plan(DEPTH),
        SessionOptions::default(),
    )
    .unwrap();
    let deepest = format!("chain.{}", DEPTH - 1);

    let (initial, initial_metrics) = session.root_value_current_with_metrics(&deepest).unwrap();
    assert_eq!(initial, number(7));
    assert_eq!(initial_metrics.recomputed_field_count, DEPTH);

    let turn = session.apply(event(&session, 1, 0, None)).unwrap();
    assert_eq!(turn.metrics.recomputed_field_count, 0);
    assert_eq!(session.snapshot().unwrap().states[&StateId(0)], number(11));

    let (updated, updated_metrics) = session.root_value_current_with_metrics(&deepest).unwrap();
    assert_eq!(updated, number(11));
    assert_eq!(updated_metrics.recomputed_field_count, DEPTH);
}

fn projected_chunk_slot(list: usize, label_field: usize, items_field: usize) -> ListStorageSlot {
    ListStorageSlot {
        id: PlanStorageId(list),
        list_id: ListId(list),
        scope_id: Some(ScopeId(list)),
        row_fields: vec![
            PlanListRowField {
                field_id: FieldId(label_field),
                name: "label".to_owned(),
                role: PlanListRowFieldRole::Value,
            },
            PlanListRowField {
                field_id: FieldId(items_field),
                name: "items".to_owned(),
                role: PlanListRowFieldRole::Value,
            },
        ],
        capacity: None,
        hidden_key_type: "Key".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::Empty,
        range: None,
        initial_rows: Vec::new(),
    }
}

fn chunk_projection_op(id: usize, list: usize, source_list: usize) -> PlanOp {
    PlanOp {
        id: PlanOpId(id),
        kind: PlanOpKind::ListProjection {
            projection: PlanListProjection::Chunk {
                source_list: ListId(source_list),
                size: 1,
            },
        },
        inputs: vec![ValueRef::List(ListId(source_list))],
        output: Some(ValueRef::List(ListId(list))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    }
}

fn deep_chunk_chain_plan(depth: usize) -> MachinePlan {
    assert!(depth > 0);
    let mut list_slots = Vec::with_capacity(depth + 1);
    list_slots.push(ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_fields: vec![PlanListRowField {
            field_id: FieldId(0),
            name: "value".to_owned(),
            role: PlanListRowFieldRole::Authority,
        }],
        capacity: None,
        hidden_key_type: "Key".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![PlanInitialListField {
                name: "value".to_owned(),
                field_id: Some(FieldId(0)),
                initializer: initial(number_constant(1)),
            }],
        }],
    });
    let mut ops = Vec::with_capacity(depth);
    for list in 1..=depth {
        list_slots.push(projected_chunk_slot(list, list * 2 - 1, list * 2));
        ops.push(chunk_projection_op(list - 1, list, list - 1));
    }

    let list_label_storage = (0..=depth)
        .map(|list| {
            if list == 0 {
                "chunks.base".to_owned()
            } else {
                format!("chunks.level_{list}")
            }
        })
        .collect::<Vec<_>>();
    let list_labels = list_label_storage
        .iter()
        .enumerate()
        .map(|(list, label)| (ListId(list), label.as_str()))
        .collect();
    let mut field_label_storage = vec![(FieldId(0), "chunks.base.value".to_owned())];
    for list in 1..=depth {
        field_label_storage.push((FieldId(list * 2 - 1), format!("chunks.level_{list}.label")));
        field_label_storage.push((FieldId(list * 2), format!("chunks.level_{list}.items")));
    }
    let field_labels = field_label_storage
        .iter()
        .map(|(field, label)| (*field, label.as_str()))
        .collect();

    plan(
        RootOutputDemand::Selected(Vec::new()),
        PlanRowExpressionArena::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        list_slots,
        ops,
        Vec::new(),
        list_labels,
        field_labels,
    )
}

#[test]
fn default_stack_deep_acyclic_chunk_length_and_window_chain_is_iterative() {
    const DEPTH: usize = 1_024;
    let deepest = ListId(DEPTH);
    let mut session =
        MachineInstance::new(deep_chunk_chain_plan(DEPTH), SessionOptions::default()).unwrap();

    assert_eq!(session.list_logical_len_current(deepest).unwrap(), 1);
    assert!(
        session.list_row_snapshots(deepest).unwrap().is_empty(),
        "logical-length demand must not materialize the deepest chunk row"
    );

    let (logical_len, rows) = session
        .list_row_snapshots_window_current(deepest, 0..1)
        .unwrap();
    assert_eq!(logical_len, 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id.list, deepest);
    let Value::List(items) = &rows[0].fields[&FieldId(DEPTH * 2)] else {
        panic!("deepest chunk items must remain a typed list");
    };
    assert!(matches!(
        items.as_slice(),
        [Value::Row { id, .. }] if id.list == ListId(DEPTH - 1)
    ));
}

fn cyclic_chunk_plan() -> MachinePlan {
    let list_label_storage = ["cycle.left".to_owned(), "cycle.right".to_owned()];
    let list_labels = list_label_storage
        .iter()
        .enumerate()
        .map(|(list, label)| (ListId(list), label.as_str()))
        .collect();
    let field_label_storage = [
        (FieldId(0), "cycle.left.label".to_owned()),
        (FieldId(1), "cycle.left.items".to_owned()),
        (FieldId(2), "cycle.right.label".to_owned()),
        (FieldId(3), "cycle.right.items".to_owned()),
    ];
    let field_labels = field_label_storage
        .iter()
        .map(|(field, label)| (*field, label.as_str()))
        .collect();
    plan(
        RootOutputDemand::Selected(Vec::new()),
        PlanRowExpressionArena::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![projected_chunk_slot(0, 0, 1), projected_chunk_slot(1, 2, 3)],
        vec![chunk_projection_op(0, 0, 1), chunk_projection_op(1, 1, 0)],
        Vec::new(),
        list_labels,
        field_labels,
    )
}

#[test]
fn default_stack_worklist_failures_clean_currentness_for_retry() {
    const DEPTH: usize = 512;
    let mut budgeted = MachineInstance::new(
        deep_acyclic_dependency_plan(DEPTH),
        SessionOptions {
            max_work_units_per_transaction: Some(32),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    let deepest = format!("chain.{}", DEPTH - 1);

    for _ in 0..2 {
        assert!(matches!(
            budgeted.root_value_current(&deepest),
            Err(Error::WorkBudgetExceeded { limit: 32, .. })
        ));
        assert_eq!(budgeted.root_value_current("chain.0").unwrap(), number(7));
    }

    let mut cyclic = MachineInstance::new(cyclic_chunk_plan(), SessionOptions::default()).unwrap();
    for entry in [ListId(0), ListId(1), ListId(0)] {
        assert_eq!(
            cyclic.list_logical_len_current(entry),
            Err(Error::ListCycle { list: entry }),
            "cycle cleanup must let each fresh entry discover its own back-edge"
        );
    }
}

#[test]
fn deterministic_work_budget_bounds_startup_without_affecting_unbounded_sessions() {
    let make_plan = || {
        let mut row_expressions = PlanRowExpressionArena::new();
        let expression = row_field(&mut row_expressions, ValueRef::State(StateId(0)));
        plan(
            RootOutputDemand::Selected(vec![FieldId(0)]),
            row_expressions,
            vec![constant(0, number_constant(1))],
            Vec::new(),
            vec![number_slot(0, 0)],
            Vec::new(),
            vec![derived(
                0,
                0,
                vec![ValueRef::State(StateId(0))],
                Some(expression),
            )],
            vec![(StateId(0), "count")],
            Vec::new(),
            vec![(FieldId(0), "current")],
        )
    };

    MachineInstance::new(make_plan(), SessionOptions::default())
        .expect("trusted sessions remain unbounded by default");
    let error = MachineInstance::new(
        make_plan(),
        SessionOptions {
            max_work_units_per_transaction: Some(0),
            ..SessionOptions::default()
        },
    )
    .err()
    .expect("a zero-unit startup budget must fail closed");
    assert_eq!(
        error,
        Error::WorkBudgetExceeded {
            limit: 0,
            attempted: 1,
        }
    );
}

#[test]
fn source_turn_work_budget_rolls_back_authority_and_current_outputs() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let update_value = row_field(&mut row_expressions, ValueRef::Field(FieldId(0)));
    let read_update = PlanOp {
        id: PlanOpId(2),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::Source(SourceId(0)),
            value: Some(update_value),
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0)), ValueRef::Field(FieldId(0))],
        output: Some(ValueRef::State(StateId(1))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let source_value = row_field(&mut row_expressions, ValueRef::State(StateId(0)));
    let current_value = row_field(&mut row_expressions, ValueRef::State(StateId(1)));
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::Selected(vec![FieldId(1)]),
            row_expressions,
            vec![constant(0, number_constant(1))],
            vec![route(0, None)],
            vec![number_slot(0, 0), number_slot(1, 0)],
            Vec::new(),
            vec![
                derived(0, 0, vec![ValueRef::State(StateId(0))], Some(source_value)),
                derived(1, 1, vec![ValueRef::State(StateId(1))], Some(current_value)),
                read_update,
            ],
            vec![(StateId(0), "source"), (StateId(1), "destination")],
            Vec::new(),
            vec![(FieldId(0), "source_value"), (FieldId(1), "current")],
        ),
        SessionOptions {
            max_work_units_per_transaction: Some(4),
            ..SessionOptions::default()
        },
    )
    .expect("four work units admit the initial currentness barrier");
    let before = session.snapshot().unwrap();

    let error = session
        .apply(event(&session, 1, 0, None))
        .expect_err("the update plus currentness barrier must exceed four units");
    assert!(matches!(error, Error::WorkBudgetExceeded { limit: 4, .. }));
    assert_eq!(session.snapshot().unwrap(), before);
}

#[test]
fn materializing_a_row_field_does_not_invalidate_list_structure_consumers() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_fields: vec![
            PlanListRowField {
                field_id: FieldId(10),
                name: "raw".into(),
                role: PlanListRowFieldRole::Authority,
            },
            PlanListRowField {
                field_id: FieldId(11),
                name: "copy".into(),
                role: PlanListRowFieldRole::Value,
            },
        ],
        capacity: None,
        hidden_key_type: "Key".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![PlanInitialListField {
                name: "raw".to_owned(),
                field_id: Some(FieldId(10)),
                initializer: initial(PlanConstantValue::Text {
                    value: "value".to_owned(),
                }),
            }],
        }],
    };
    let list_expression = row_list_ref(&mut row_expressions, ListId(0));
    let list_view = derived(0, 0, vec![ValueRef::List(ListId(0))], Some(list_expression));
    let copy_expression = row_field(&mut row_expressions, ValueRef::Field(FieldId(10)));
    let row_copy = PlanOp {
        id: PlanOpId(1),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::Pure,
            startup_recompute: false,
            materialization: None,
            expression: Some(PlanDerivedExpression::RowExpression {
                expression: copy_expression,
            }),
        },
        inputs: vec![ValueRef::Field(FieldId(10))],
        output: Some(ValueRef::Field(FieldId(11))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::Selected(vec![FieldId(0)]),
            row_expressions,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![list],
            vec![list_view, row_copy],
            Vec::new(),
            vec![(ListId(0), "items")],
            vec![
                (FieldId(0), "visible_items"),
                (FieldId(10), "items.raw"),
                (FieldId(11), "items.copy"),
            ],
        ),
        SessionOptions::default(),
    )
    .unwrap();
    let row = session.snapshot().unwrap().lists[&ListId(0)][0].id;

    session
        .project_current(&[ValueTarget::RowField {
            row,
            field: FieldId(11),
        }])
        .unwrap();

    assert!(session.snapshot().is_ok());
}

#[test]
fn source_transform_captures_event_before_later_demand() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let default = row_constant(&mut row_expressions, PlanConstantId(0));
    let value = row_constant(&mut row_expressions, PlanConstantId(1));
    let source_transform = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::SourceEventTransform,
            startup_recompute: false,
            materialization: None,
            expression: Some(PlanDerivedExpression::SourceEventTransform {
                default,
                arms: vec![PlanSourceEventTransformArm {
                    trigger: ValueRef::Source(SourceId(0)),
                    value,
                }],
                router_route: false,
            }),
        },
        inputs: vec![
            ValueRef::Source(SourceId(0)),
            ValueRef::Constant(PlanConstantId(0)),
            ValueRef::Constant(PlanConstantId(1)),
        ],
        output: Some(ValueRef::Field(FieldId(0))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::Selected(Vec::new()),
            row_expressions,
            vec![
                constant(0, PlanConstantValue::Text { value: "".into() }),
                constant(
                    1,
                    PlanConstantValue::Text {
                        value: "captured".into(),
                    },
                ),
            ],
            vec![route(0, None)],
            Vec::new(),
            Vec::new(),
            vec![source_transform],
            Vec::new(),
            Vec::new(),
            vec![(FieldId(0), "event_value")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    session.apply(event(&session, 1, 0, None)).unwrap();

    assert_eq!(
        session.root_value_current("event_value").unwrap(),
        Value::Text("captured".into())
    );
}

#[test]
fn source_transform_keeps_precommit_state_for_the_event_turn() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let default = row_constant(&mut row_expressions, PlanConstantId(1));
    let event_value = row_field(&mut row_expressions, ValueRef::State(StateId(0)));
    let source_transform = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::SourceEventTransform,
            startup_recompute: true,
            materialization: None,
            expression: Some(PlanDerivedExpression::SourceEventTransform {
                default,
                arms: vec![PlanSourceEventTransformArm {
                    trigger: ValueRef::Source(SourceId(0)),
                    value: event_value,
                }],
                router_route: false,
            }),
        },
        inputs: vec![
            ValueRef::Source(SourceId(0)),
            ValueRef::State(StateId(0)),
            ValueRef::Constant(PlanConstantId(1)),
        ],
        output: Some(ValueRef::Field(FieldId(0))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let clear_value = row_constant(&mut row_expressions, PlanConstantId(1));
    let clear_state = PlanOp {
        id: PlanOpId(1),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::Source(SourceId(0)),
            value: Some(clear_value),
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0))],
        output: Some(ValueRef::State(StateId(0))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let text_slot = ScalarStorageSlot {
        id: PlanStorageId(0),
        state_id: StateId(0),
        owner: PlanOwner::root(),
        value_type: PlanValueType::Text,
        scope_id: None,
        indexed: false,
        indexed_field_id: None,
        lifetime: PlanStateLifetime::Persistent,
        initializer: ScalarInitializerPlan::Constant {
            constant_id: PlanConstantId(0),
        },
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            vec![
                constant(
                    0,
                    PlanConstantValue::Text {
                        value: "before".into(),
                    },
                ),
                constant(1, PlanConstantValue::Text { value: "".into() }),
            ],
            vec![route(0, None)],
            vec![text_slot],
            Vec::new(),
            vec![source_transform, clear_state],
            vec![(StateId(0), "input")],
            Vec::new(),
            vec![(FieldId(0), "captured")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    session.apply(event(&session, 1, 0, None)).unwrap();

    assert_eq!(
        session.root_value_current("captured").unwrap(),
        Value::Text("before".into())
    );
}

#[test]
fn reverse_dependencies_recompute_every_dependent_once() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let left = row_field(&mut row_expressions, ValueRef::State(StateId(0)));
    let right = row_field(&mut row_expressions, ValueRef::State(StateId(0)));
    let update = const_update(&mut row_expressions, 2, 0, 0, 1);
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            vec![
                constant(0, number_constant(0)),
                constant(1, number_constant(1)),
            ],
            vec![route(0, None)],
            vec![number_slot(0, 0)],
            Vec::new(),
            vec![
                derived(0, 0, vec![ValueRef::State(StateId(0))], Some(left)),
                derived(1, 1, vec![ValueRef::State(StateId(0))], Some(right)),
                update,
            ],
            vec![(StateId(0), "source")],
            Vec::new(),
            vec![(FieldId(0), "left"), (FieldId(1), "right")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    let turn = session.apply(event(&session, 1, 0, None)).unwrap();
    assert_eq!(turn.metrics.recomputed_field_count, 2);
    assert_eq!(turn.metrics.dirty_field_count, 2);
    assert_eq!(session.snapshot().unwrap().fields.len(), 2);
}

#[test]
fn same_turn_recompute_does_not_suppress_later_invalidation() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let middle = row_field(&mut row_expressions, ValueRef::State(StateId(0)));
    let leaf = row_field(&mut row_expressions, ValueRef::Field(FieldId(0)));
    let captured = row_field(&mut row_expressions, ValueRef::Field(FieldId(1)));
    let read_update = |id| PlanOp {
        id: PlanOpId(id),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::Source(SourceId(0)),
            value: Some(captured),
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0)), ValueRef::Field(FieldId(1))],
        output: Some(ValueRef::State(StateId(1))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let first_update = const_update(&mut row_expressions, 2, 0, 0, 1);
    let second_update = const_update(&mut row_expressions, 4, 0, 0, 2);
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            vec![
                constant(0, number_constant(0)),
                constant(1, number_constant(1)),
                constant(2, number_constant(2)),
            ],
            vec![route(0, None)],
            vec![number_slot(0, 0), number_slot(1, 0)],
            Vec::new(),
            vec![
                derived(0, 0, vec![ValueRef::State(StateId(0))], Some(middle)),
                derived(1, 1, vec![ValueRef::Field(FieldId(0))], Some(leaf)),
                first_update,
                read_update(3),
                second_update,
                read_update(5),
            ],
            vec![(StateId(0), "source"), (StateId(1), "captured")],
            Vec::new(),
            vec![(FieldId(0), "middle"), (FieldId(1), "leaf")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    session.apply(event(&session, 1, 0, None)).unwrap();

    assert_eq!(session.snapshot().unwrap().states[&StateId(1)], number(2));
}

#[test]
fn recursive_derived_reentry_returns_typed_cycle_error() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let left_expression = row_field(&mut row_expressions, ValueRef::Field(FieldId(1)));
    let left = derived(
        0,
        0,
        vec![ValueRef::Field(FieldId(1))],
        Some(left_expression),
    );
    let right_expression = row_field(&mut row_expressions, ValueRef::Field(FieldId(0)));
    let right = derived(
        1,
        1,
        vec![ValueRef::Field(FieldId(0))],
        Some(right_expression),
    );
    let error = MachineInstance::new(
        plan(
            RootOutputDemand::Selected(vec![FieldId(0)]),
            row_expressions,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![left, right],
            Vec::new(),
            Vec::new(),
            vec![(FieldId(0), "left"), (FieldId(1), "right")],
        ),
        SessionOptions::default(),
    )
    .err()
    .expect("cycle must fail construction");
    assert!(matches!(error, Error::Cycle { row: None, .. }));
}

#[test]
fn explicit_dependency_cycle_boundary_returns_application_tag_and_recovers() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let use_reference = row_field(&mut row_expressions, ValueRef::Field(FieldId(10)));
    let target_index = row_field(&mut row_expressions, ValueRef::Field(FieldId(11)));
    let referenced_result = row(
        &mut row_expressions,
        PlanRowExpressionNode::ListGetField {
            list_id: ListId(0),
            index: target_index,
            field: FieldId(13),
        },
    );
    let cycle_tag = row_constant(&mut row_expressions, PlanConstantId(0));
    let caught_reference = row(
        &mut row_expressions,
        PlanRowExpressionNode::CatchCycle {
            input: referenced_result,
            on_cycle: cycle_tag,
        },
    );
    let literal = row_field(&mut row_expressions, ValueRef::Field(FieldId(12)));
    let result = row(
        &mut row_expressions,
        PlanRowExpressionNode::Select {
            input: use_reference,
            arms: vec![
                PlanRowSelectArm {
                    pattern: PlanRowSelectPattern::Tag {
                        name: "True".to_owned(),
                    },
                    value: caught_reference,
                },
                PlanRowSelectArm {
                    pattern: PlanRowSelectPattern::Wildcard,
                    value: literal,
                },
            ],
        },
    );
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_fields: vec![
            PlanListRowField {
                field_id: FieldId(10),
                name: "use_reference".to_owned(),
                role: PlanListRowFieldRole::Authority,
            },
            PlanListRowField {
                field_id: FieldId(11),
                name: "target_index".to_owned(),
                role: PlanListRowFieldRole::Authority,
            },
            PlanListRowField {
                field_id: FieldId(12),
                name: "literal".to_owned(),
                role: PlanListRowFieldRole::Authority,
            },
            PlanListRowField {
                field_id: FieldId(13),
                name: "result".to_owned(),
                role: PlanListRowFieldRole::Value,
            },
        ],
        capacity: None,
        hidden_key_type: "Key".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![
            PlanInitialListRow {
                fields: vec![
                    PlanInitialListField {
                        name: "use_reference".to_owned(),
                        field_id: Some(FieldId(10)),
                        initializer: initial(truth_constant(true)),
                    },
                    PlanInitialListField {
                        name: "target_index".to_owned(),
                        field_id: Some(FieldId(11)),
                        initializer: initial(number_constant(1)),
                    },
                    PlanInitialListField {
                        name: "literal".to_owned(),
                        field_id: Some(FieldId(12)),
                        initializer: initial(number_constant(7)),
                    },
                ],
            },
            PlanInitialListRow {
                fields: vec![
                    PlanInitialListField {
                        name: "use_reference".to_owned(),
                        field_id: Some(FieldId(10)),
                        initializer: initial(truth_constant(true)),
                    },
                    PlanInitialListField {
                        name: "target_index".to_owned(),
                        field_id: Some(FieldId(11)),
                        initializer: initial(number_constant(0)),
                    },
                    PlanInitialListField {
                        name: "literal".to_owned(),
                        field_id: Some(FieldId(12)),
                        initializer: initial(number_constant(5)),
                    },
                ],
            },
        ],
    };
    let computation = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::Pure,
            startup_recompute: false,
            materialization: None,
            expression: Some(PlanDerivedExpression::RowExpression { expression: result }),
        },
        inputs: vec![
            ValueRef::Field(FieldId(10)),
            ValueRef::Field(FieldId(11)),
            ValueRef::Field(FieldId(12)),
            ValueRef::List(ListId(0)),
        ],
        output: Some(ValueRef::Field(FieldId(13))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::Selected(Vec::new()),
            row_expressions,
            vec![constant(0, tag_constant("CycleError"))],
            Vec::new(),
            Vec::new(),
            vec![list],
            vec![computation],
            Vec::new(),
            vec![(ListId(0), "rows")],
            vec![
                (FieldId(10), "rows.use_reference"),
                (FieldId(11), "rows.target_index"),
                (FieldId(12), "rows.literal"),
                (FieldId(13), "rows.result"),
            ],
        ),
        SessionOptions::default(),
    )
    .unwrap();
    let rows = session.list_rows(ListId(0));
    let target = |row| ValueTarget::RowField {
        row,
        field: FieldId(13),
    };

    assert_eq!(
        session.project_current(&[target(rows[0])]).unwrap()[&target(rows[0])],
        Value::Tag {
            tag: "CycleError".to_owned(),
            fields: BTreeMap::new(),
        }
    );
    assert_eq!(
        session.project_current(&[target(rows[1])]).unwrap()[&target(rows[1])],
        Value::Tag {
            tag: "CycleError".to_owned(),
            fields: BTreeMap::new(),
        }
    );

    session
        .test_set_row_field(rows[1], FieldId(10), Value::truth(false))
        .unwrap();
    assert_eq!(
        session.project_current(&[target(rows[0])]).unwrap()[&target(rows[0])],
        number(5)
    );
    assert_eq!(
        session.project_current(&[target(rows[1])]).unwrap()[&target(rows[1])],
        number(5)
    );
}

#[test]
fn remove_then_append_allocates_a_new_row_identity() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_fields: vec![PlanListRowField {
            field_id: FieldId(0),
            name: "value".into(),
            role: PlanListRowFieldRole::Authority,
        }],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![PlanInitialListField {
                name: "value".into(),
                field_id: Some(FieldId(0)),
                initializer: initial(PlanConstantValue::Text {
                    value: "old".into(),
                }),
            }],
        }],
    };
    let remove_trigger = row_field(&mut row_expressions, ValueRef::Source(SourceId(0)));
    let remove = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::ListMutation {
            mutation: PlanListMutation::Remove(PlanListRemove {
                site: 0,
                ordinal: 0,
                owner: PlanOwner::root(),
                trigger: ValueRef::Source(SourceId(0)),
                gate: remove_trigger,
                local_owner: PlanStaticOwnerId(0),
                row_local: PlanLocalId(0),
                predicate: remove_trigger,
                remove_when: true,
            }),
        },
        inputs: vec![ValueRef::Source(SourceId(0))],
        output: Some(ValueRef::List(ListId(0))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let append_gate = row_field(&mut row_expressions, ValueRef::Source(SourceId(1)));
    let append_item = row_constant(&mut row_expressions, PlanConstantId(0));
    let append = PlanOp {
        id: PlanOpId(1),
        kind: PlanOpKind::ListMutation {
            mutation: PlanListMutation::Append(PlanListAppend {
                site: 1,
                ordinal: 1,
                owner: PlanOwner::root(),
                trigger: ValueRef::Source(SourceId(1)),
                gate: append_gate,
                item: append_item,
                fields: vec![PlanListAppendField {
                    name: "value".into(),
                    field_id: FieldId(0),
                }],
                row_field_copies: Vec::new(),
            }),
        },
        inputs: vec![
            ValueRef::Source(SourceId(1)),
            ValueRef::Constant(PlanConstantId(0)),
        ],
        output: Some(ValueRef::List(ListId(0))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::Selected(Vec::new()),
            row_expressions,
            vec![constant(
                0,
                PlanConstantValue::Text {
                    value: "new".into(),
                },
            )],
            vec![route(0, Some(0)), route(1, None)],
            Vec::new(),
            vec![list],
            vec![remove, append],
            Vec::new(),
            vec![(ListId(0), "items")],
            vec![(FieldId(0), "items.value")],
        ),
        SessionOptions::default(),
    )
    .unwrap();
    let original = RowId {
        list: ListId(0),
        key: 1,
        generation: 1,
    };
    session
        .apply(event(&session, 1, 0, Some(original)))
        .unwrap();
    let turn = session.apply(event(&session, 2, 1, None)).unwrap();
    let inserted = turn
        .deltas
        .iter()
        .find_map(|delta| match delta {
            Delta::InsertRow { row } => Some(row.id),
            _ => None,
        })
        .unwrap();
    assert_ne!(inserted, original);
    assert_eq!(session.list_rows(ListId(0)), vec![inserted]);
}

#[test]
fn authority_restore_preserves_an_explicitly_emptied_list_and_allocator() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_fields: vec![PlanListRowField {
            field_id: FieldId(0),
            name: "value".into(),
            role: PlanListRowFieldRole::Authority,
        }],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![PlanInitialListField {
                name: "value".into(),
                field_id: Some(FieldId(0)),
                initializer: initial(PlanConstantValue::Text {
                    value: "default".into(),
                }),
            }],
        }],
    };
    let remove_trigger = row_field(&mut row_expressions, ValueRef::Source(SourceId(0)));
    let remove = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::ListMutation {
            mutation: PlanListMutation::Remove(PlanListRemove {
                site: 0,
                ordinal: 0,
                owner: PlanOwner::root(),
                trigger: ValueRef::Source(SourceId(0)),
                gate: remove_trigger,
                local_owner: PlanStaticOwnerId(0),
                row_local: PlanLocalId(0),
                predicate: remove_trigger,
                remove_when: true,
            }),
        },
        inputs: vec![ValueRef::Source(SourceId(0))],
        output: Some(ValueRef::List(ListId(0))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let machine = plan(
        RootOutputDemand::Selected(Vec::new()),
        row_expressions,
        Vec::new(),
        vec![route(0, Some(0))],
        Vec::new(),
        vec![list],
        vec![remove],
        Vec::new(),
        vec![(ListId(0), "items")],
        vec![(FieldId(0), "items.value")],
    );
    let mut session = MachineInstance::new(machine.clone(), SessionOptions::default()).unwrap();
    let original = session.list_rows(ListId(0))[0];
    session
        .apply(event(&session, 1, 0, Some(original)))
        .unwrap();
    let authority = session.authority_snapshot().unwrap();
    assert!(authority.lists[&ListId(0)].touched);
    assert!(authority.lists[&ListId(0)].rows.is_empty());
    assert_eq!(authority.lists[&ListId(0)].next_key, 2);
    let durable = session
        .durable_restore_image(3, Default::default())
        .unwrap();
    assert_eq!(durable.lists.len(), 1);
    assert!(durable.lists.values().next().unwrap().rows.is_empty());

    let restored = MachineInstanceBuilder::new(machine, SessionOptions::default())
        .unwrap()
        .restore_durable(durable)
        .unwrap()
        .build()
        .unwrap();
    assert!(restored.list_rows(ListId(0)).is_empty());
    assert_eq!(
        restored.authority_snapshot().unwrap().lists[&ListId(0)].next_key,
        2
    );
}

#[test]
fn indexed_override_does_not_materialize_the_whole_default_list() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_fields: vec![PlanListRowField {
            field_id: FieldId(0),
            name: "formula".into(),
            role: PlanListRowFieldRole::Authority,
        }],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: (0..2)
            .map(|_| PlanInitialListRow {
                fields: vec![PlanInitialListField {
                    name: "formula".into(),
                    field_id: Some(FieldId(0)),
                    initializer: initial(PlanConstantValue::Text {
                        value: "default".into(),
                    }),
                }],
            })
            .collect(),
    };
    let indexed = ScalarStorageSlot {
        id: PlanStorageId(1),
        state_id: StateId(0),
        owner: PlanOwner {
            static_owner: PlanStaticOwnerId(0),
            ancestors: vec![PlanOwnerAncestor {
                static_owner: PlanStaticOwnerId(0),
                scope: ScopeId(0),
                list: ListId(0),
            }],
        },
        value_type: PlanValueType::Text,
        scope_id: Some(ScopeId(0)),
        indexed: true,
        indexed_field_id: Some(FieldId(0)),
        lifetime: PlanStateLifetime::Persistent,
        initializer: ScalarInitializerPlan::Constant {
            constant_id: PlanConstantId(0),
        },
    };
    let update_value = row_constant(&mut row_expressions, PlanConstantId(1));
    let update = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::Source(SourceId(0)),
            value: Some(update_value),
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0))],
        output: Some(ValueRef::State(StateId(0))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let machine = plan(
        RootOutputDemand::Selected(Vec::new()),
        row_expressions,
        vec![
            constant(
                0,
                PlanConstantValue::Text {
                    value: "default".into(),
                },
            ),
            constant(
                1,
                PlanConstantValue::Text {
                    value: "=A1+1".into(),
                },
            ),
        ],
        vec![route(0, Some(0))],
        vec![indexed],
        vec![list],
        vec![update],
        vec![(StateId(0), "formula")],
        vec![(ListId(0), "cells")],
        vec![(FieldId(0), "cells.formula")],
    );
    let mut session = MachineInstance::new(machine.clone(), SessionOptions::default()).unwrap();
    let selected = session.list_rows(ListId(0))[1];
    let turn = session
        .apply(event(&session, 1, 0, Some(selected)))
        .unwrap();
    assert!(matches!(
        turn.authority_deltas.as_slice(),
        [AuthorityDelta::SetRowField { row, .. }] if *row == selected
    ));
    let durable = session
        .durable_restore_image(1, Default::default())
        .unwrap();
    let stored = durable.lists.values().next().unwrap();
    assert!(!stored.touched);
    assert_eq!(stored.next_key, 0);
    assert_eq!(stored.rows.len(), 1);
    assert_eq!(stored.rows[0].key, selected.key);

    let restored = MachineInstanceBuilder::new(machine, SessionOptions::default())
        .unwrap()
        .restore_durable(durable)
        .unwrap()
        .build()
        .unwrap();
    let snapshot = restored.snapshot().unwrap();
    assert_eq!(snapshot.lists[&ListId(0)].len(), 2);
    assert_eq!(
        snapshot.lists[&ListId(0)][0].fields[&FieldId(0)],
        Value::Text("default".into())
    );
    assert_eq!(
        snapshot.lists[&ListId(0)][1].fields[&FieldId(0)],
        Value::Text("=A1+1".into())
    );
}

#[test]
fn activation_restored_root_state_is_available_to_indexed_default_reconstruction() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let restored_default = row_field(&mut row_expressions, ValueRef::State(StateId(0)));
    let list = ListStorageSlot {
        id: PlanStorageId(2),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_fields: vec![PlanListRowField {
            field_id: FieldId(0),
            name: "value".into(),
            role: PlanListRowFieldRole::Authority,
        }],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![PlanInitialListField {
                name: "value".into(),
                field_id: Some(FieldId(0)),
                initializer: PlanInitialListFieldInitializer::Expression {
                    expression: restored_default,
                },
            }],
        }],
    };
    let indexed = ScalarStorageSlot {
        id: PlanStorageId(1),
        state_id: StateId(1),
        owner: PlanOwner {
            static_owner: PlanStaticOwnerId(0),
            ancestors: vec![PlanOwnerAncestor {
                static_owner: PlanStaticOwnerId(0),
                scope: ScopeId(0),
                list: ListId(0),
            }],
        },
        value_type: PlanValueType::Number,
        scope_id: Some(ScopeId(0)),
        indexed: true,
        indexed_field_id: Some(FieldId(0)),
        lifetime: PlanStateLifetime::Persistent,
        initializer: ScalarInitializerPlan::Expression {
            expression: restored_default,
        },
    };
    let update = const_update(&mut row_expressions, 0, 0, 0, 1);
    let machine = plan(
        RootOutputDemand::Selected(Vec::new()),
        row_expressions,
        vec![
            constant(0, number_constant(7)),
            constant(1, number_constant(9)),
        ],
        vec![route(0, None)],
        vec![number_slot(0, 0), indexed],
        vec![list],
        vec![update],
        vec![(StateId(0), "store.seed"), (StateId(1), "rows.value")],
        vec![(ListId(0), "rows")],
        vec![(FieldId(0), "rows.value")],
    );
    let mut session = MachineInstance::new(machine.clone(), SessionOptions::default()).unwrap();
    session.apply(event(&session, 1, 0, None)).unwrap();
    let durable = session
        .durable_restore_image(1, Default::default())
        .unwrap();
    let mut restored = MachineInstanceBuilder::new(machine, SessionOptions::default())
        .unwrap()
        .restore_durable(durable)
        .unwrap()
        .build()
        .unwrap();
    let Value::List(rows) = restored
        .inspect_list_field_current(ListId(0), FieldId(0), 1)
        .unwrap()
    else {
        panic!("indexed default inspection is not a list");
    };
    let Value::Record(fields) = &rows[0] else {
        panic!("indexed default inspection row is not a record");
    };
    assert_eq!(fields["value"], number(9));
}

#[test]
fn activation_derived_default_is_reconstructed_before_sparse_row_override() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let row_id = row_constant(&mut row_expressions, PlanConstantId(0));
    let item = row(
        &mut row_expressions,
        PlanRowExpressionNode::Object {
            fields: vec![PlanRowObjectField {
                name: "id".into(),
                value: row_id,
                spread: false,
            }],
        },
    );
    let list_value = row(
        &mut row_expressions,
        PlanRowExpressionNode::ListLiteral { items: vec![item] },
    );
    let update_value = row_constant(&mut row_expressions, PlanConstantId(2));
    let list = ListStorageSlot {
        id: PlanStorageId(1),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_fields: vec![
            PlanListRowField {
                field_id: FieldId(0),
                name: "id".into(),
                role: PlanListRowFieldRole::Authority,
            },
            PlanListRowField {
                field_id: FieldId(1),
                name: "value".into(),
                role: PlanListRowFieldRole::Authority,
            },
        ],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::Empty,
        range: None,
        initial_rows: Vec::new(),
    };
    let indexed = ScalarStorageSlot {
        id: PlanStorageId(0),
        state_id: StateId(0),
        owner: PlanOwner {
            static_owner: PlanStaticOwnerId(0),
            ancestors: vec![PlanOwnerAncestor {
                static_owner: PlanStaticOwnerId(0),
                scope: ScopeId(0),
                list: ListId(0),
            }],
        },
        value_type: PlanValueType::Text,
        scope_id: Some(ScopeId(0)),
        indexed: true,
        indexed_field_id: Some(FieldId(1)),
        lifetime: PlanStateLifetime::Persistent,
        initializer: ScalarInitializerPlan::Constant {
            constant_id: PlanConstantId(1),
        },
    };
    let materializer = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::Pure,
            startup_recompute: false,
            materialization: Some(PlanListMaterialization {
                target_list: ListId(0),
                authority_source_list: None,
                fields: BTreeMap::from([("id".to_owned(), FieldId(0))]),
                row_field_copies: Vec::new(),
                value_list_authorities: Vec::new(),
            }),
            expression: Some(PlanDerivedExpression::RowExpression {
                expression: list_value,
            }),
        },
        inputs: vec![ValueRef::Constant(PlanConstantId(0))],
        output: Some(ValueRef::List(ListId(0))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let update = PlanOp {
        id: PlanOpId(1),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::Source(SourceId(0)),
            value: Some(update_value),
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0))],
        output: Some(ValueRef::State(StateId(0))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let mut machine = plan(
        RootOutputDemand::Selected(Vec::new()),
        row_expressions,
        vec![
            constant(
                0,
                PlanConstantValue::Text {
                    value: "row".into(),
                },
            ),
            constant(
                1,
                PlanConstantValue::Text {
                    value: "default".into(),
                },
            ),
            constant(
                2,
                PlanConstantValue::Text {
                    value: "override".into(),
                },
            ),
        ],
        vec![route(0, Some(0))],
        vec![indexed],
        vec![list],
        vec![materializer, update],
        vec![(StateId(0), "rows.value")],
        vec![(ListId(0), "rows")],
        vec![(FieldId(0), "rows.id"), (FieldId(1), "rows.value")],
    );
    set_list_activation_mode(&mut machine, ListId(0), ListActivationMode::DerivedDefault);
    let mut session = MachineInstance::new(machine.clone(), SessionOptions::default()).unwrap();
    let rows = session.list_rows_current(ListId(0)).unwrap();
    assert_eq!(rows.len(), 1);
    session.apply(event(&session, 1, 0, Some(rows[0]))).unwrap();
    let durable = session
        .durable_restore_image(1, Default::default())
        .unwrap();
    let stored = durable.lists.values().next().unwrap();
    assert!(!stored.touched);
    assert_eq!(stored.rows.len(), 1);

    let restored = MachineInstanceBuilder::new(machine, SessionOptions::default())
        .unwrap()
        .restore_durable(durable)
        .unwrap()
        .build()
        .unwrap();
    let snapshot = restored.snapshot().unwrap();
    assert_eq!(snapshot.lists[&ListId(0)].len(), 1);
    assert_eq!(
        snapshot.lists[&ListId(0)][0].fields[&FieldId(1)],
        Value::Text("override".into())
    );
}

#[test]
fn activation_materialized_authority_stays_durable_even_with_a_list_computation() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let empty = row(
        &mut row_expressions,
        PlanRowExpressionNode::ListLiteral { items: Vec::new() },
    );
    let append_gate = row_field(&mut row_expressions, ValueRef::Source(SourceId(0)));
    let append_item = row_constant(&mut row_expressions, PlanConstantId(0));
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_fields: vec![PlanListRowField {
            field_id: FieldId(0),
            name: "value".into(),
            role: PlanListRowFieldRole::Authority,
        }],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::Empty,
        range: None,
        initial_rows: Vec::new(),
    };
    let computation = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::Pure,
            startup_recompute: false,
            materialization: None,
            expression: Some(PlanDerivedExpression::RowExpression { expression: empty }),
        },
        inputs: Vec::new(),
        output: Some(ValueRef::List(ListId(0))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let append = PlanOp {
        id: PlanOpId(1),
        kind: PlanOpKind::ListMutation {
            mutation: PlanListMutation::Append(PlanListAppend {
                site: 0,
                ordinal: 0,
                owner: PlanOwner::root(),
                trigger: ValueRef::Source(SourceId(0)),
                gate: append_gate,
                item: append_item,
                fields: vec![PlanListAppendField {
                    name: "value".into(),
                    field_id: FieldId(0),
                }],
                row_field_copies: Vec::new(),
            }),
        },
        inputs: vec![
            ValueRef::Source(SourceId(0)),
            ValueRef::Constant(PlanConstantId(0)),
        ],
        output: Some(ValueRef::List(ListId(0))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let mut machine = plan(
        RootOutputDemand::Selected(Vec::new()),
        row_expressions,
        vec![constant(
            0,
            PlanConstantValue::Text {
                value: "host-row".into(),
            },
        )],
        vec![route(0, None)],
        Vec::new(),
        vec![list],
        vec![computation, append],
        Vec::new(),
        vec![(ListId(0), "host_rows")],
        vec![(FieldId(0), "host_rows.value")],
    );
    set_list_activation_mode(
        &mut machine,
        ListId(0),
        ListActivationMode::MaterializedAuthority,
    );
    let mut session = MachineInstance::new(machine.clone(), SessionOptions::default()).unwrap();
    let turn = session.apply(event(&session, 1, 0, None)).unwrap();
    assert!(matches!(
        turn.durable_changes.as_slice(),
        [boon_persistence::DurableChange::SetList { .. }]
    ));
    let durable = session
        .durable_restore_image(1, Default::default())
        .unwrap();
    let stored = durable.lists.values().next().unwrap();
    assert!(stored.touched);
    assert_eq!(stored.rows.len(), 1);
    let restored = MachineInstanceBuilder::new(machine, SessionOptions::default())
        .unwrap()
        .restore_durable(durable)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(restored.list_rows(ListId(0)).len(), 1);
}

#[test]
fn non_monotonic_source_sequences_are_rejected() {
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::Selected(Vec::new()),
            PlanRowExpressionArena::new(),
            Vec::new(),
            vec![route(0, None)],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        SessionOptions::default(),
    )
    .unwrap();
    session.apply(event(&session, 1, 0, None)).unwrap();
    assert!(matches!(
        session.apply(event(&session, 1, 0, None)),
        Err(Error::InvalidEvent(_))
    ));
}

#[test]
fn durable_variants_round_trip_tag_only_and_structured_values() {
    assert_eq!(
        crate::machine::runtime_value(boon_persistence::StoredValue::Tag {
            tag: "Done".to_owned(),
            fields: BTreeMap::new(),
        })
        .unwrap(),
        Value::tag("Done")
    );

    let runtime = Value::tagged("Ready", BTreeMap::from([("count".to_owned(), number(4))]));
    let stored = crate::machine::stored_value(&runtime).unwrap();
    assert!(matches!(
        &stored,
        boon_persistence::StoredValue::Tag { tag, fields }
            if tag == "Ready" && fields["count"] == stored_number(4)
    ));
    assert_eq!(crate::machine::runtime_value(stored).unwrap(), runtime);
}

#[test]
fn whole_and_decimal_numbers_share_one_value_identity() {
    let whole = Value::Number(ExactNumber::one());
    let decimal = Value::Number("1.0".parse().unwrap());
    assert_eq!(whole, decimal);
}

#[test]
fn flush_is_private_until_the_named_root_boundary() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let payload = row_constant(&mut row_expressions, PlanConstantId(0));
    let flush = row(
        &mut row_expressions,
        PlanRowExpressionNode::Flush { payload },
    );
    let boundary = row(
        &mut row_expressions,
        PlanRowExpressionNode::FlushBoundary { input: flush },
    );
    assert!(
        row_expressions
            .iter()
            .any(|(_, node)| { matches!(node, PlanRowExpressionNode::Flush { .. }) })
    );
    assert!(
        row_expressions
            .iter()
            .any(|(_, node)| { matches!(node, PlanRowExpressionNode::FlushBoundary { .. }) })
    );

    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            vec![constant(0, tag_constant("InvalidInput"))],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![derived(0, 0, Vec::new(), Some(boundary))],
            Vec::new(),
            Vec::new(),
            vec![(FieldId(0), "result")],
        ),
        SessionOptions::default(),
    )
    .unwrap();
    assert_eq!(
        session.root_value_current("result").unwrap(),
        Value::tag("InvalidInput")
    );
}

#[test]
fn flushing_state_update_preserves_prior_state_and_later_activation_recovers() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let payload = row_constant(&mut row_expressions, PlanConstantId(1));
    let flush = row(
        &mut row_expressions,
        PlanRowExpressionNode::Flush { payload },
    );
    let valid = row_constant(&mut row_expressions, PlanConstantId(2));
    let state = row_field(&mut row_expressions, ValueRef::State(StateId(0)));
    let exposed = row(
        &mut row_expressions,
        PlanRowExpressionNode::FlushBoundary { input: state },
    );
    let flush_update = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::Source(SourceId(0)),
            value: Some(flush),
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0))],
        output: Some(ValueRef::State(StateId(0))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let valid_update = PlanOp {
        id: PlanOpId(1),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::Source(SourceId(1)),
            value: Some(valid),
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(1))],
        output: Some(ValueRef::State(StateId(0))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let exposed_result = derived(2, 0, vec![ValueRef::State(StateId(0))], Some(exposed));
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            vec![
                constant(0, number_constant(1)),
                constant(1, tag_constant("InvalidUpdate")),
                constant(2, number_constant(2)),
            ],
            vec![route(0, None), route(1, None)],
            vec![number_slot(0, 0)],
            Vec::new(),
            vec![flush_update, valid_update, exposed_result],
            vec![(StateId(0), "store.value")],
            Vec::new(),
            vec![(FieldId(0), "result")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    assert_eq!(
        session.root_value_current("store.value").unwrap(),
        number(1)
    );
    assert_eq!(session.root_value_current("result").unwrap(), number(1));

    let flushed = session.apply(event(&session, 1, 0, None)).unwrap();
    assert!(flushed.authority_deltas.is_empty());
    assert_eq!(
        session.root_value_current("store.value").unwrap(),
        number(1)
    );
    assert_eq!(
        session.root_value_current("result").unwrap(),
        Value::tag("InvalidUpdate")
    );

    let recovered = session.apply(event(&session, 2, 1, None)).unwrap();
    assert!(recovered.authority_deltas.iter().any(|delta| {
        matches!(
            delta,
            AuthorityDelta::SetRoot { state, value }
                if *state == StateId(0) && *value == number(2)
        )
    }));
    assert_eq!(
        session.root_value_current("store.value").unwrap(),
        number(2)
    );
    assert_eq!(session.root_value_current("result").unwrap(), number(2));
}

#[test]
fn collection_flush_uses_first_semantic_position_and_bypasses_downstream_work() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let one = row_constant(&mut row_expressions, PlanConstantId(0));
    let two = row_constant(&mut row_expressions, PlanConstantId(1));
    let three = row_constant(&mut row_expressions, PlanConstantId(2));
    let source = row(
        &mut row_expressions,
        PlanRowExpressionNode::ListLiteral {
            items: vec![one, two, three],
        },
    );
    let item = contextual_local(&mut row_expressions, 1, &[]);
    let fails = row(
        &mut row_expressions,
        PlanRowExpressionNode::NumberInfix {
            op: PlanInfixOp::GreaterOrEqual,
            left: item,
            right: two,
        },
    );
    let payload = row(
        &mut row_expressions,
        PlanRowExpressionNode::TaggedObject {
            tag: "InvalidItem".to_owned(),
            fields: vec![PlanRowObjectField {
                name: "position".to_owned(),
                value: item,
                spread: false,
            }],
        },
    );
    let flush = row(
        &mut row_expressions,
        PlanRowExpressionNode::Flush { payload },
    );
    let body = row(
        &mut row_expressions,
        PlanRowExpressionNode::Select {
            input: fails,
            arms: vec![
                PlanRowSelectArm {
                    pattern: PlanRowSelectPattern::Tag {
                        name: "True".to_owned(),
                    },
                    value: flush,
                },
                PlanRowSelectArm {
                    pattern: PlanRowSelectPattern::Wildcard,
                    value: item,
                },
            ],
        },
    );
    let mapped = contextual_collection(
        &mut row_expressions,
        1,
        PlanContextualOperationKind::Map,
        source,
        body,
    );
    let downstream_count = row(
        &mut row_expressions,
        PlanRowExpressionNode::BuiltinCall {
            function: PlanRowBuiltin::ListCount,
            input: Some(mapped),
            args: Vec::new(),
        },
    );
    let exposed = row(
        &mut row_expressions,
        PlanRowExpressionNode::FlushBoundary {
            input: downstream_count,
        },
    );
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            vec![
                constant(0, number_constant(1)),
                constant(1, number_constant(2)),
                constant(2, number_constant(3)),
            ],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![derived(0, 0, Vec::new(), Some(exposed))],
            Vec::new(),
            Vec::new(),
            vec![(FieldId(0), "result")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    assert_eq!(
        session.root_value_current("result").unwrap(),
        Value::tagged(
            "InvalidItem",
            BTreeMap::from([("position".to_owned(), number(2))]),
        )
    );
}

#[test]
fn flushed_state_chain_suppresses_dispatch_without_cancelling_prior_effect() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let payload = row_constant(&mut row_expressions, PlanConstantId(2));
    let flush = row(
        &mut row_expressions,
        PlanRowExpressionNode::Flush { payload },
    );
    let gate = row_field(&mut row_expressions, ValueRef::State(StateId(0)));
    let flush_update = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::Source(SourceId(0)),
            value: Some(flush),
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0))],
        output: Some(ValueRef::State(StateId(0))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let valid = row_constant(&mut row_expressions, PlanConstantId(3));
    let valid_update = PlanOp {
        id: PlanOpId(1),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::Source(SourceId(1)),
            value: Some(valid),
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(1))],
        output: Some(ValueRef::State(StateId(0))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let contract = builtin_effect_contract("Clock/wall")
        .unwrap()
        .expect("Clock/wall effect contract");
    let effect_update = PlanOp {
        id: PlanOpId(2),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::State(StateId(0)),
            value: None,
            effect: Some(EffectInvocationPlan {
                invocation_id: EffectInvocationId([7; 32]),
                effect_id: contract.effect_id,
                owner: PlanOwner::root(),
                gate,
                intent_fields: Vec::new(),
                idempotency_key: EffectIdempotencyKeyPlan::InvocationTurnIntentSha256,
                result: EffectResultRoute::Target {
                    target: ValueRef::State(StateId(1)),
                    policy: EffectResultPolicy::ReturnValue,
                },
                barrier: contract.barrier,
            }),
        },
        inputs: vec![ValueRef::State(StateId(0))],
        output: Some(ValueRef::State(StateId(1))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let tag_slot = |state: usize, constant: usize| ScalarStorageSlot {
        id: PlanStorageId(state),
        state_id: StateId(state),
        owner: PlanOwner::root(),
        value_type: PlanValueType::Tag,
        scope_id: None,
        indexed: false,
        indexed_field_id: None,
        lifetime: PlanStateLifetime::Persistent,
        initializer: ScalarInitializerPlan::Constant {
            constant_id: PlanConstantId(constant),
        },
    };
    let mut machine = plan(
        RootOutputDemand::All,
        row_expressions,
        vec![
            constant(0, tag_constant("False")),
            constant(1, tag_constant("RandomNotRequested")),
            constant(2, tag_constant("ClockUnavailable")),
            constant(3, tag_constant("True")),
        ],
        vec![route(0, None), route(1, None)],
        vec![tag_slot(0, 0), tag_slot(1, 1)],
        Vec::new(),
        vec![flush_update, valid_update, effect_update],
        vec![
            (StateId(0), "store.clock_result"),
            (StateId(1), "store.random_result"),
        ],
        Vec::new(),
        Vec::new(),
    );
    machine.effects.push(contract);
    machine.capability_summary = derive_capability_summary(&machine);

    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let dispatched = session.apply(event(&session, 1, 1, None)).unwrap();
    assert_eq!(dispatched.transient_effects.len(), 1);
    assert_eq!(session.pending_transient_effect_count(), 1);

    let flushed = session.apply(event(&session, 2, 0, None)).unwrap();

    assert!(flushed.transient_effects.is_empty());
    assert!(flushed.cancelled_transient_effects.is_empty());
    assert!(flushed.outbox_changes.is_empty());
    assert_eq!(session.pending_transient_effect_count(), 1);
    assert_eq!(
        session.root_value_current("store.clock_result").unwrap(),
        Value::tag("True")
    );
    assert_eq!(
        session.root_value_current("store.random_result").unwrap(),
        Value::tag("RandomNotRequested")
    );
    assert!(flushed.authority_deltas.is_empty());
}

#[test]
fn flushed_state_chain_discards_staged_list_mutation() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let payload = row_constant(&mut row_expressions, PlanConstantId(1));
    let flush = row(
        &mut row_expressions,
        PlanRowExpressionNode::Flush { payload },
    );
    let flush_update = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::Source(SourceId(0)),
            value: Some(flush),
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0))],
        output: Some(ValueRef::State(StateId(0))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let gate = row_field(&mut row_expressions, ValueRef::State(StateId(0)));
    let item = row_constant(&mut row_expressions, PlanConstantId(2));
    let append = PlanOp {
        id: PlanOpId(1),
        kind: PlanOpKind::ListMutation {
            mutation: PlanListMutation::Append(PlanListAppend {
                site: 0,
                ordinal: 0,
                owner: PlanOwner::root(),
                trigger: ValueRef::State(StateId(0)),
                gate,
                item,
                fields: vec![PlanListAppendField {
                    name: "value".to_owned(),
                    field_id: FieldId(10),
                }],
                row_field_copies: Vec::new(),
            }),
        },
        inputs: vec![ValueRef::State(StateId(0))],
        output: Some(ValueRef::List(ListId(0))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let state = ScalarStorageSlot {
        id: PlanStorageId(0),
        state_id: StateId(0),
        owner: PlanOwner::root(),
        value_type: PlanValueType::Tag,
        scope_id: None,
        indexed: false,
        indexed_field_id: None,
        lifetime: PlanStateLifetime::Persistent,
        initializer: ScalarInitializerPlan::Constant {
            constant_id: PlanConstantId(0),
        },
    };
    let list = ListStorageSlot {
        id: PlanStorageId(1),
        list_id: ListId(0),
        scope_id: None,
        row_fields: vec![PlanListRowField {
            field_id: FieldId(10),
            name: "value".to_owned(),
            role: PlanListRowFieldRole::Authority,
        }],
        capacity: None,
        hidden_key_type: "Key".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::Empty,
        range: None,
        initial_rows: Vec::new(),
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            vec![
                constant(0, tag_constant("Ready")),
                constant(1, tag_constant("InvalidMutation")),
                constant(
                    2,
                    PlanConstantValue::Text {
                        value: "must-not-append".to_owned(),
                    },
                ),
            ],
            vec![route(0, None)],
            vec![state],
            vec![list],
            vec![flush_update, append],
            vec![(StateId(0), "store.status")],
            vec![(ListId(0), "store.items")],
            vec![(FieldId(10), "store.items.value")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    let turn = session.apply(event(&session, 1, 0, None)).unwrap();
    assert!(session.list_rows(ListId(0)).is_empty());
    assert!(turn.authority_deltas.is_empty());
    assert_eq!(
        session.root_value_current("store.status").unwrap(),
        Value::tag("Ready")
    );
}

#[derive(Clone, Copy)]
enum DetachedCaptureDeclaration {
    TargetCapture,
    WrongListCapture,
    TargetValue,
}

fn detached_capture_source_list() -> ListStorageSlot {
    let row = |seed: &str| PlanInitialListRow {
        fields: vec![PlanInitialListField {
            name: "seed".to_owned(),
            field_id: Some(FieldId(10)),
            initializer: initial(PlanConstantValue::Text {
                value: seed.to_owned(),
            }),
        }],
    };
    ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_fields: vec![PlanListRowField {
            field_id: FieldId(10),
            name: "seed".to_owned(),
            role: PlanListRowFieldRole::Authority,
        }],
        capacity: None,
        hidden_key_type: "Key".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![row("alpha"), row("beta")],
    }
}

fn detached_capture_materialization_plan(declaration: DetachedCaptureDeclaration) -> MachinePlan {
    let mut row_expressions = PlanRowExpressionArena::new();
    let (capture_field, capture_role) = match declaration {
        DetachedCaptureDeclaration::TargetCapture => (FieldId(21), PlanListRowFieldRole::Capture),
        DetachedCaptureDeclaration::WrongListCapture => {
            (FieldId(31), PlanListRowFieldRole::Capture)
        }
        DetachedCaptureDeclaration::TargetValue => (FieldId(21), PlanListRowFieldRole::Value),
    };
    let mut target_fields = vec![
        PlanListRowField {
            field_id: FieldId(20),
            name: "seed".to_owned(),
            role: PlanListRowFieldRole::Value,
        },
        PlanListRowField {
            field_id: FieldId(22),
            name: "remembered".to_owned(),
            role: PlanListRowFieldRole::Value,
        },
    ];
    if !matches!(declaration, DetachedCaptureDeclaration::WrongListCapture) {
        target_fields.push(PlanListRowField {
            field_id: capture_field,
            name: "@capture/seed".to_owned(),
            role: capture_role,
        });
    }
    let target = ListStorageSlot {
        id: PlanStorageId(1),
        list_id: ListId(1),
        scope_id: Some(ScopeId(1)),
        row_fields: target_fields,
        capacity: None,
        hidden_key_type: "Key".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::Empty,
        range: None,
        initial_rows: Vec::new(),
    };
    let wrong_owner = ListStorageSlot {
        id: PlanStorageId(2),
        list_id: ListId(2),
        scope_id: Some(ScopeId(2)),
        row_fields: vec![PlanListRowField {
            field_id: FieldId(31),
            name: "@capture/seed".to_owned(),
            role: PlanListRowFieldRole::Capture,
        }],
        capacity: None,
        hidden_key_type: "Key".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::Empty,
        range: None,
        initial_rows: Vec::new(),
    };
    let indexed_state = ScalarStorageSlot {
        id: PlanStorageId(3),
        state_id: StateId(0),
        owner: PlanOwner {
            static_owner: PlanStaticOwnerId(7),
            ancestors: vec![PlanOwnerAncestor {
                static_owner: PlanStaticOwnerId(7),
                scope: ScopeId(1),
                list: ListId(1),
            }],
        },
        value_type: PlanValueType::Text,
        scope_id: Some(ScopeId(1)),
        indexed: true,
        indexed_field_id: Some(FieldId(22)),
        lifetime: PlanStateLifetime::Persistent,
        initializer: ScalarInitializerPlan::Expression {
            expression: row_field(&mut row_expressions, ValueRef::Field(capture_field)),
        },
    };
    let source = row_authority_list_ref(&mut row_expressions, ListId(0));
    let seed = contextual_row_field(&mut row_expressions, 7, 0, 10);
    let body = row(
        &mut row_expressions,
        PlanRowExpressionNode::Object {
            fields: vec![PlanRowObjectField {
                name: "seed".to_owned(),
                value: seed,
                spread: false,
            }],
        },
    );
    let capture = contextual_row_field(&mut row_expressions, 7, 0, 10);
    let map = row(
        &mut row_expressions,
        PlanRowExpressionNode::ContextualCollection {
            owner: PlanStaticOwnerId(7),
            operation: PlanContextualOperationKind::Map,
            source,
            row_local: PlanLocalId(0),
            body,
            captures: vec![PlanRowCapture {
                field: capture_field,
                value: capture,
            }],
            indexed_access: None,
        },
    );
    let materialize = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::ListView,
            startup_recompute: true,
            materialization: Some(PlanListMaterialization {
                target_list: ListId(1),
                authority_source_list: None,
                fields: BTreeMap::from([("seed".to_owned(), FieldId(20))]),
                row_field_copies: Vec::new(),
                value_list_authorities: Vec::new(),
            }),
            expression: Some(PlanDerivedExpression::RowExpression { expression: map }),
        },
        inputs: vec![ValueRef::List(ListId(0))],
        output: Some(ValueRef::List(ListId(1))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let mut lists = vec![detached_capture_source_list(), target];
    if matches!(declaration, DetachedCaptureDeclaration::WrongListCapture) {
        lists.push(wrong_owner);
    }
    plan(
        RootOutputDemand::All,
        row_expressions,
        Vec::new(),
        Vec::new(),
        vec![indexed_state],
        lists,
        vec![materialize],
        vec![(StateId(0), "rows.remembered")],
        vec![
            (ListId(0), "seeds"),
            (ListId(1), "rows"),
            (ListId(2), "wrong_capture_owner"),
        ],
        vec![
            (FieldId(10), "seeds.seed"),
            (FieldId(20), "rows.seed"),
            (FieldId(21), "rows.@capture/seed"),
            (FieldId(22), "rows.remembered"),
            (FieldId(31), "wrong_capture_owner.@capture/seed"),
        ],
    )
}

fn current_detached_capture_rows(session: &mut MachineInstance) -> Vec<RowSnapshot> {
    session.list_value_current(ListId(1)).unwrap();
    session.snapshot().unwrap().lists[&ListId(1)].clone()
}

#[test]
fn detached_state_captures_retain_distinct_source_row_values() {
    let mut session = MachineInstance::new(
        detached_capture_materialization_plan(DetachedCaptureDeclaration::TargetCapture),
        SessionOptions::default(),
    )
    .unwrap();

    let rows = current_detached_capture_rows(&mut session);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].fields[&FieldId(21)],
        Value::Text("alpha".to_owned())
    );
    assert_eq!(rows[1].fields[&FieldId(21)], Value::Text("beta".to_owned()));
    assert_ne!(rows[0].fields[&FieldId(21)], rows[1].fields[&FieldId(21)]);
}

#[test]
fn detached_state_capture_is_published_before_indexed_state_initialization() {
    let mut session = MachineInstance::new(
        detached_capture_materialization_plan(DetachedCaptureDeclaration::TargetCapture),
        SessionOptions::default(),
    )
    .unwrap();

    let rows = current_detached_capture_rows(&mut session);
    for row in rows {
        let captured = row.fields.get(&FieldId(21)).expect("hidden capture");
        let initialized = row
            .fields
            .get(&FieldId(22))
            .expect("indexed state initialized from capture");
        assert_eq!(initialized, captured);
    }
}

#[test]
fn detached_state_captures_do_not_escape_spread_materialization_or_facades() {
    let mut row_expressions = PlanRowExpressionArena::new();
    let capture_storage = ListStorageSlot {
        id: PlanStorageId(1),
        list_id: ListId(1),
        scope_id: Some(ScopeId(1)),
        row_fields: vec![PlanListRowField {
            field_id: FieldId(21),
            name: "@capture/seed".to_owned(),
            role: PlanListRowFieldRole::Capture,
        }],
        capacity: None,
        hidden_key_type: "Key".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::Empty,
        range: None,
        initial_rows: Vec::new(),
    };
    let source = row_authority_list_ref(&mut row_expressions, ListId(0));
    let spread = contextual_local(&mut row_expressions, 7, &[]);
    let body = row(
        &mut row_expressions,
        PlanRowExpressionNode::Object {
            fields: vec![PlanRowObjectField {
                name: String::new(),
                value: spread,
                spread: true,
            }],
        },
    );
    let capture = contextual_row_field(&mut row_expressions, 7, 0, 10);
    let expression = row(
        &mut row_expressions,
        PlanRowExpressionNode::ContextualCollection {
            owner: PlanStaticOwnerId(7),
            operation: PlanContextualOperationKind::Map,
            source,
            row_local: PlanLocalId(0),
            body,
            captures: vec![PlanRowCapture {
                field: FieldId(21),
                value: capture,
            }],
            indexed_access: None,
        },
    );
    let session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            row_expressions,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![detached_capture_source_list(), capture_storage],
            vec![derived(
                0,
                30,
                vec![ValueRef::List(ListId(0))],
                Some(expression),
            )],
            Vec::new(),
            vec![(ListId(0), "seeds"), (ListId(1), "capture_storage")],
            vec![
                (FieldId(10), "seeds.seed"),
                (FieldId(21), "capture_storage.@capture/seed"),
                (FieldId(30), "visible_rows"),
            ],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    let snapshot = session.snapshot().unwrap();
    let Value::List(rows) = &snapshot.fields[&FieldId(30)] else {
        panic!("mapped capture fixture did not publish a list facade");
    };
    assert_eq!(rows.len(), 2);
    for (row, seed) in rows.iter().zip(["alpha", "beta"]) {
        let Value::MappedRow { fields, .. } = row else {
            panic!("mapped capture fixture lost row identity");
        };
        assert_eq!(
            fields,
            &BTreeMap::from([("seed".to_owned(), Value::Text(seed.to_owned()))])
        );
        assert!(fields.keys().all(|name| !name.contains("capture")));
    }
}

#[test]
fn detached_state_capture_field_identity_fails_closed() {
    for (label, declaration) in [
        (
            "wrong-list capture",
            DetachedCaptureDeclaration::WrongListCapture,
        ),
        ("non-Capture field", DetachedCaptureDeclaration::TargetValue),
    ] {
        let error = match MachineInstance::new(
            detached_capture_materialization_plan(declaration),
            SessionOptions::default(),
        ) {
            Err(error) => error,
            Ok(mut session) => match session.list_value_current(ListId(1)) {
                Err(error) => error,
                Ok(_) => panic!("{label} was accepted"),
            },
        };
        if !matches!(error, Error::InvalidPlan(_)) {
            panic!("{label} returned the wrong error: {error}");
        }
    }
}
