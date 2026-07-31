use super::*;
use boon_plan::*;
use std::collections::{BTreeMap, BTreeSet};

fn compile_test_source(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
    program_role: ProgramRole,
) -> boon_compiler::CompilerResult<boon_compiler::CompiledMachinePlanFromSource> {
    boon_compiler::compile_machine_plan(boon_compiler::CompileRequest::source_text(
        source_label,
        source_text,
        target_profile,
        program_role,
        ApplicationIdentity::compiler_default(),
    ))
}

fn compile_test_path(
    source_path: &std::path::Path,
    target_profile: TargetProfile,
    program_role: ProgramRole,
) -> boon_compiler::CompilerResult<boon_compiler::CompiledMachinePlanFromSource> {
    boon_compiler::compile_machine_plan(boon_compiler::CompileRequest::source_path(
        source_path,
        target_profile,
        program_role,
        ApplicationIdentity::compiler_default(),
    ))
}

fn compile_server_source(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
) -> boon_compiler::CompilerResult<boon_compiler::CompiledMachinePlanFromSource> {
    compile_test_source(
        source_label,
        source_text,
        target_profile,
        ProgramRole::Server,
    )
}

fn compile_server_path(
    source_path: &std::path::Path,
    target_profile: TargetProfile,
) -> boon_compiler::CompilerResult<boon_compiler::CompiledMachinePlanFromSource> {
    compile_test_path(source_path, target_profile, ProgramRole::Server)
}

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
                InitialProvenance::ReconstructableDefault,
                owner,
                slot.hidden_key_type.clone(),
                slot.has_generation,
                row_fields,
            )
            .unwrap()
        })
        .collect();
    let persistence = PersistencePlan::new(&application, 1, memory, lists, Vec::new()).unwrap();
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
        durable_sliced.authority_snapshot().unwrap(),
        synchronous.authority_snapshot().unwrap()
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

fn typed_passkey_effect_machine() -> MachinePlan {
    compile_server_source(
        "typed-passkey-effects-executor.bn",
        include_str!("../../../testdata/typed_passkey_effects.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan
}

fn outbound_http_effect_machine() -> MachinePlan {
    compile_server_path(
        std::path::Path::new("examples/outbound_http_effect.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan
}

fn state_triggered_effect_chain_machine() -> MachinePlan {
    compile_server_source(
        "state-triggered-effect-chain.bn",
        include_str!("../../../testdata/state_triggered_effect_chain.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan
}

fn file_stream_effect_machine() -> MachinePlan {
    compile_test_source(
        "file-stream-effect-executor.bn",
        r#"
store: [
    read: SOURCE
    stream_result:
        NotStarted |> HOLD stream_result {
            read |> THEN {
                File/read_stream(
                    file: read.file
                    chunk_bytes: 4
                    retain_content: False
                )
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
        ProgramRole::Server,
    )
    .unwrap()
    .plan
}

fn content_import_effect_machine() -> MachinePlan {
    compile_test_source(
        "content-import-effect-executor.bn",
        r#"
store: [
    import: SOURCE
    import_result:
        NotStarted |> HOLD import_result {
            import |> THEN {
                Content/import(file: import.file)
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
        ProgramRole::Server,
    )
    .unwrap()
    .plan
}

fn nested_stream_effect_chain_machine() -> MachinePlan {
    compile_server_source(
        "nested-stream-effect-chain.bn",
        r#"
store: [
    read: SOURCE
    selected: PackageAsset[url: TEXT { asset://files/primary.vcd }]
    file_result:
        NotStarted |> HOLD file_result {
            read |> THEN {
                File/read_stream(file: selected, retain_content: True)
            }
        }
    waveform_result:
        NotStarted |> HOLD waveform_result {
            file_result |> WHEN {
                Finished => file_result.retained |> WHEN {
                    Retained => Wellen/open(content: file_result.retained.content)
                    __ => SKIP
                }
                __ => SKIP
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan
}

fn indexed_file_stream_effect_machine() -> MachinePlan {
    compile_test_source(
        "indexed-file-stream-effect-executor.bn",
        r#"
store: [
    asset:
        PackageAsset[url: TEXT { asset://files/primary.bin }]
    rows:
        LIST {
            [name: TEXT { primary }]
        }
        |> List/map(item, new: stream_row(row: item, asset: asset))
        |> List/remove(item, when:
            item.remove |> THEN { True }
        )
]

FUNCTION stream_row(row, asset) {
    [
        name: row.name
        open: SOURCE
        remove: SOURCE
        stream_result:
            NotStarted |> HOLD stream_result {
                open |> THEN {
                    File/read_stream(
                        file: asset
                        chunk_bytes: 4
                        retain_content: False
                    )
                }
            }
    ]
}
"#,
        TargetProfile::SoftwareDefault,
        ProgramRole::Server,
    )
    .unwrap()
    .plan
}

fn mapped_request_root_file_stream_effect_machine() -> MachinePlan {
    compile_test_source(
        "mapped-request-root-file-stream.bn",
        r#"
store: [
    root_open: SOURCE
    primary_asset:
        PackageAsset[url: TEXT { asset://files/primary.bin }]
    secondary_asset:
        PackageAsset[url: TEXT { asset://files/secondary.bin }]
    rows:
        LIST {
            [name: TEXT { primary }]
        }
        |> List/map(item, new: mapped_effect_row(row: item))
    mapped_request:
        rows
        |> List/map(item, new: item.open |> THEN { Primary })
        |> List/latest()
    request:
        LATEST {
            root_open |> THEN { Primary }
            mapped_request
        }
    selected:
        primary_asset |> HOLD selected {
            request |> WHEN {
                Primary => primary_asset
                Secondary => secondary_asset
                __ => SKIP
            }
        }
    stream_result:
        NotStarted |> HOLD stream_result {
            request |> THEN {
                File/read_stream(
                    file: selected
                    chunk_bytes: 4
                    retain_content: False
                )
            }
        }
]

outputs: [
    stream_result: store.stream_result
]

FUNCTION mapped_effect_row(row) {
    [
        name: row.name
        open: SOURCE
    ]
}
"#,
        TargetProfile::SoftwareDefault,
        ProgramRole::Server,
    )
    .unwrap()
    .plan
}

fn file_stream_payload() -> SourcePayload {
    let binding = HostValueIssuer::new([3; 32]).mint([7; 32], 1).unwrap();
    SourcePayload {
        fields: BTreeMap::from([(
            "file".to_owned(),
            Value::host_bound(Value::tag("FileSelected"), binding),
        )]),
        ..SourcePayload::default()
    }
}

#[test]
fn host_value_issuers_isolate_bindings_and_fully_redact_debug_output() {
    let issuer = HostValueIssuer::new([0x11; 32]);
    let foreign = HostValueIssuer::new([0x22; 32]);
    let binding = issuer.mint([0x33; 32], 17).unwrap();

    assert_eq!(issuer.open(&binding), Some(([0x33; 32], 17)));
    assert_eq!(foreign.open(&binding), None);
    assert!(issuer.mint([0x33; 32], 0).is_err());
    assert_eq!(format!("{issuer:?}"), "HostValueIssuer(<opaque>)");
    assert_eq!(format!("{binding:?}"), "HostValueBinding(<opaque>)");
}

#[test]
fn host_bound_projection_preserves_authority_without_exposing_a_fake_tag_field() {
    let binding = HostValueIssuer::new([1; 32]).mint([2; 32], 1).unwrap();
    let value = Value::host_bound(Value::tag("FileSelected"), binding);
    let enclosing = Value::Record(BTreeMap::from([("file".to_owned(), value.clone())]));
    let fake_tag_field = vec!["tag".to_owned()];
    let file = vec!["file".to_owned()];
    let nested_fake_tag_field = vec!["file".to_owned(), "tag".to_owned()];

    assert!(value.host_binding().is_some());
    assert!(value.contains_host_binding());
    assert_eq!(crate::machine::project_value(&value, &[]), Some(&value));
    assert_eq!(crate::machine::project_value(&value, &fake_tag_field), None);
    assert_eq!(
        crate::machine::project_value(&enclosing, &file),
        Some(&value)
    );
    assert_eq!(
        crate::machine::project_value(&enclosing, &nested_fake_tag_field),
        None
    );
}

#[test]
fn inspection_reports_hide_nested_bindings_while_boundaries_fail_closed() {
    let binding = HostValueIssuer::new([4; 32]).mint([5; 32], 9).unwrap();
    let bound = Value::host_bound(Value::tag("FileSelected"), binding);
    let value = Value::Record(BTreeMap::from([(
        "nested".to_owned(),
        Value::List(vec![bound.clone()]),
    )]));
    let visible = Value::Record(BTreeMap::from([(
        "nested".to_owned(),
        Value::List(vec![bound.visible().clone()]),
    )]));

    let machine = plan(
        RootOutputDemand::Selected(Vec::new()),
        PlanRowExpressionArena::new(),
        vec![constant(0, number_constant(1))],
        Vec::new(),
        vec![number_slot(0, 0)],
        Vec::new(),
        Vec::new(),
        vec![(StateId(0), "store.bound")],
        Vec::new(),
        Vec::new(),
    );
    let authority = AuthoritySnapshot {
        through_turn_sequence: 0,
        states: BTreeMap::from([(
            StateId(0),
            ScalarAuthority {
                touched: true,
                value: value.clone(),
            },
        )]),
        lists: BTreeMap::new(),
        maps: BTreeMap::new(),
        sets: BTreeMap::new(),
    };
    let mut session = MachineInstanceBuilder::new(machine, SessionOptions::default())
        .unwrap()
        .restore(authority)
        .build()
        .unwrap();

    assert_eq!(session.root_value_current("store.bound").unwrap(), visible);
    assert_eq!(session.snapshot().unwrap().states[&StateId(0)], visible);
    assert_eq!(
        session
            .project_current(&[ValueTarget::State(StateId(0))])
            .unwrap()[&ValueTarget::State(StateId(0))],
        visible
    );
    assert!(
        session.authority_snapshot().unwrap().states[&StateId(0)]
            .value
            .contains_host_binding()
    );
    assert_eq!(
        crate::machine::report_deltas(vec![Delta::SetValue {
            target: ValueTarget::State(StateId(0)),
            value: value.clone(),
        }]),
        vec![Delta::SetValue {
            target: ValueTarget::State(StateId(0)),
            value: visible.clone(),
        }]
    );
    assert!(value.to_data().is_err());
    assert!(crate::machine::stored_value(&value).is_err());
    assert!(crate::machine::normalize_host_output_value(value).is_err());
    assert!(session.durable_restore_image(0, BTreeSet::new()).is_err());
}

#[test]
fn host_bound_persistence_failure_rolls_back_authority_and_sequence() {
    let payload_field = SourcePayloadField::Named("value".to_owned());
    let mut row_expressions = PlanRowExpressionArena::new();
    let update_value = row_field(
        &mut row_expressions,
        ValueRef::SourcePayload {
            source_id: SourceId(0),
            field: payload_field.clone(),
        },
    );
    let update = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::StateUpdate {
            trigger: ValueRef::Source(SourceId(0)),
            value: Some(update_value),
            effect: None,
        },
        inputs: vec![
            ValueRef::Source(SourceId(0)),
            ValueRef::SourcePayload {
                source_id: SourceId(0),
                field: payload_field.clone(),
            },
        ],
        output: Some(ValueRef::State(StateId(0))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let machine = plan(
        RootOutputDemand::Selected(Vec::new()),
        row_expressions,
        vec![constant(0, number_constant(1))],
        vec![route(0, None)],
        vec![number_slot(0, 0)],
        Vec::new(),
        vec![update],
        vec![(StateId(0), "store.value")],
        Vec::new(),
        Vec::new(),
    );
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let before = session.authority_snapshot().unwrap();
    let bound = Value::host_bound(
        Value::Text("visible".to_owned()),
        HostValueIssuer::new([8; 32]).mint([9; 32], 1).unwrap(),
    );

    assert!(
        session
            .apply(SourceEvent {
                sequence: 1,
                source: SourceId(0),
                route: route_token(&session, SourceId(0), None),
                target: None,
                payload: SourcePayload {
                    fields: BTreeMap::from([("value".to_owned(), bound)]),
                    ..SourcePayload::default()
                },
            })
            .is_err()
    );
    assert_eq!(session.authority_snapshot().unwrap(), before);

    let retry = session
        .apply(SourceEvent {
            sequence: 1,
            source: SourceId(0),
            route: route_token(&session, SourceId(0), None),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([("value".to_owned(), number(2))]),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    assert_eq!(retry.sequence, 1);
    assert_eq!(
        session.root_value_current("store.value").unwrap(),
        number(2)
    );
}

fn file_stream_outcome(
    tag: &str,
    fields: impl IntoIterator<Item = (&'static str, Value)>,
) -> Value {
    Value::tagged(
        tag,
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    )
}

fn retained_content_outcome(tag: &str, content: Option<Value>) -> Value {
    let mut fields = BTreeMap::new();
    if let Some(content) = content {
        fields.insert("content".to_owned(), content);
    }
    Value::tagged(tag, fields)
}

fn content_ref_value() -> Value {
    Value::Record(BTreeMap::from([
        ("digest".to_owned(), Value::Bytes(vec![9; 32].into())),
        ("size".to_owned(), number(3)),
        (
            "media".to_owned(),
            Value::Text("application/octet-stream".to_owned()),
        ),
    ]))
}

fn outbound_http_payload() -> SourcePayload {
    SourcePayload {
        fields: BTreeMap::from([
            ("endpoint".to_owned(), Value::Text("catalog".to_owned())),
            ("method".to_owned(), Value::tag("Get")),
            (
                "path_segments".to_owned(),
                Value::List(vec![
                    Value::Text("v1".to_owned()),
                    Value::Text("items".to_owned()),
                ]),
            ),
            (
                "query".to_owned(),
                Value::List(vec![Value::Record(BTreeMap::from([
                    ("name".to_owned(), Value::Text("limit".to_owned())),
                    ("value".to_owned(), Value::Text("10".to_owned())),
                ]))]),
            ),
            (
                "headers".to_owned(),
                Value::List(vec![Value::Record(BTreeMap::from([
                    ("name".to_owned(), Value::Text("accept".to_owned())),
                    (
                        "value".to_owned(),
                        Value::Bytes(b"application/json".to_vec().into()),
                    ),
                ]))]),
            ),
            ("body".to_owned(), Value::Bytes(Vec::new().into())),
            ("connect_timeout_ms".to_owned(), number(500)),
            ("overall_timeout_ms".to_owned(), number(2_000)),
        ]),
        ..SourcePayload::default()
    }
}

fn outbound_http_success(status: i64) -> Value {
    Value::tagged(
        "HttpSucceeded",
        BTreeMap::from([
            ("endpoint".to_owned(), Value::Text("catalog".to_owned())),
            ("status".to_owned(), number(status)),
            (
                "headers".to_owned(),
                Value::List(vec![Value::Record(BTreeMap::from([
                    ("name".to_owned(), Value::Text("content-type".to_owned())),
                    (
                        "value".to_owned(),
                        Value::Bytes(b"application/json".to_vec().into()),
                    ),
                ]))]),
            ),
            (
                "body".to_owned(),
                Value::Bytes(br#"{"ok":true}"#.to_vec().into()),
            ),
            ("redirects_followed".to_owned(), number(0)),
        ]),
    )
}

#[test]
fn read_only_http_effect_is_transient_typed_correlated_and_cycle_safe() {
    let machine = outbound_http_effect_machine();
    let last_status = match &machine.output_root("last_status").unwrap().value {
        OutputValueRef::RuntimeValue {
            value: ValueRef::Field(field),
            ..
        } => *field,
        other => panic!("unexpected last_status output ref: {other:?}"),
    };
    let contract = machine
        .effects
        .iter()
        .find(|contract| contract.host_operation == "Http/request")
        .unwrap();
    assert_eq!(contract.replay, EffectReplay::ReadOnly);
    assert_eq!(contract.barrier, EffectBarrier::None);
    assert!(machine.persistence.effect_outbox.is_empty());

    let request = source_id(&machine, "store.request");
    let mut session = MachineInstance::new(machine.clone(), SessionOptions::default()).unwrap();
    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source: request,
            route: route_token(&session, request, None),
            target: None,
            payload: outbound_http_payload(),
        })
        .unwrap();
    assert!(turn.outbox_changes.is_empty());
    let [invocation] = turn.transient_effects.as_slice() else {
        panic!("HTTP request must emit exactly one transient effect");
    };
    assert_eq!(invocation.effect_id, contract.effect_id);
    assert_eq!(invocation.trigger_sequence, 1);
    assert!(matches!(
        &invocation.intent,
        Value::Record(fields)
            if matches!(fields.get("path_segments"), Some(Value::List(values)) if values.len() == 2)
    ));
    assert_eq!(session.pending_transient_effect_count(), 1);

    let completion = session
        .complete_transient_effect_with_demand(
            invocation.call_id,
            outbound_http_success(201),
            &[ValueTarget::Field(last_status)],
        )
        .unwrap();
    assert!(completion.outbox_changes.is_empty());
    assert!(completion.durable_changes.is_empty());
    assert!(completion.transient_effects.is_empty());
    assert!(completion.deltas.iter().any(|delta| matches!(
        delta,
        Delta::SetValue {
            target: ValueTarget::Field(field),
            value,
        } if *field == last_status && value == &number(201)
    )));
    assert_eq!(
        session.root_value_current("store.last_status").unwrap(),
        number(201)
    );
    assert_eq!(session.pending_transient_effect_count(), 0);
    assert!(
        session
            .complete_transient_effect(invocation.call_id, outbound_http_success(202))
            .is_err()
    );

    let stale = MachineInstance::new(machine, SessionOptions::default())
        .unwrap()
        .complete_transient_effect(invocation.call_id, outbound_http_success(200));
    assert!(
        matches!(stale, Err(Error::InvalidEvent(detail)) if detail.contains("different session launch"))
    );
}

#[test]
fn effect_completion_triggers_the_next_effect_even_when_the_value_repeats() {
    let machine = state_triggered_effect_chain_machine();
    let start = source_id(&machine, "store.start");
    let clock_effect = machine
        .effects
        .iter()
        .find(|effect| effect.host_operation == "Clock/wall")
        .unwrap()
        .effect_id;
    let random_effect = machine
        .effects
        .iter()
        .find(|effect| effect.host_operation == "Random/bytes")
        .unwrap()
        .effect_id;
    let wall_result = Value::tagged(
        "WallClockRead",
        BTreeMap::from([
            ("unix_seconds".to_owned(), number(1_700_000_000)),
            ("nanoseconds".to_owned(), number(123)),
        ]),
    );
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    for sequence in 1..=2 {
        let clock = session
            .apply(SourceEvent {
                sequence,
                source: start,
                route: route_token(&session, start, None),
                target: None,
                payload: SourcePayload::default(),
            })
            .unwrap()
            .transient_effects
            .remove(0);
        assert_eq!(clock.effect_id, clock_effect);
        let completion = session
            .complete_transient_effect(clock.call_id, wall_result.clone())
            .unwrap();
        let [random] = completion.transient_effects.as_slice() else {
            panic!("every wall-clock completion must trigger Random/bytes");
        };
        assert_eq!(random.effect_id, random_effect);
        session
            .complete_transient_effect(
                random.call_id,
                Value::tagged(
                    "RandomBytesReady",
                    BTreeMap::from([(
                        "bytes".to_owned(),
                        Value::Bytes(vec![sequence as u8; 16].into()),
                    )]),
                ),
            )
            .unwrap();
    }

    let clock = session
        .apply(SourceEvent {
            sequence: 3,
            source: start,
            route: route_token(&session, start, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap()
        .transient_effects
        .remove(0);
    let failure = session
        .complete_transient_effect(
            clock.call_id,
            Value::tagged(
                "HostServiceFailed",
                BTreeMap::from([
                    ("code".to_owned(), Value::Text("clock_failed".to_owned())),
                    (
                        "diagnostic".to_owned(),
                        Value::Text("clock unavailable".to_owned()),
                    ),
                ]),
            ),
        )
        .unwrap();
    assert!(failure.transient_effects.is_empty());
}

#[test]
fn transient_http_cancel_and_rollback_preserve_one_shot_ownership() {
    let machine = outbound_http_effect_machine();
    let request = source_id(&machine, "store.request");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    let first = session
        .apply(SourceEvent {
            sequence: 1,
            source: request,
            route: route_token(&session, request, None),
            target: None,
            payload: outbound_http_payload(),
        })
        .unwrap()
        .transient_effects
        .remove(0);
    assert!(session.cancel_transient_effect(first.call_id).unwrap());
    assert!(!session.cancel_transient_effect(first.call_id).unwrap());
    assert!(
        session
            .complete_transient_effect(first.call_id, outbound_http_success(200))
            .is_err()
    );

    let second = session
        .apply(SourceEvent {
            sequence: 2,
            source: request,
            route: route_token(&session, request, None),
            target: None,
            payload: outbound_http_payload(),
        })
        .unwrap()
        .transient_effects
        .remove(0);
    session
        .complete_transient_effect(second.call_id, outbound_http_success(204))
        .unwrap();
    assert_eq!(session.pending_transient_effect_count(), 0);
    session.rollback_unsettled_turn().unwrap();
    assert_eq!(session.pending_transient_effect_count(), 1);
    assert_eq!(
        session.root_value_current("store.last_status").unwrap(),
        number(0)
    );
    session
        .complete_transient_effect(second.call_id, outbound_http_success(205))
        .unwrap();
}

#[test]
fn stream_effect_delivery_is_ordered_bounded_terminal_and_replaced_by_owner() {
    let machine = file_stream_effect_machine();
    let read = source_id(&machine, "store.read");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let first_turn = session
        .apply(SourceEvent {
            sequence: 1,
            source: read,
            route: route_token(&session, read, None),
            target: None,
            payload: file_stream_payload(),
        })
        .unwrap();
    let [first] = first_turn.transient_effects.as_slice() else {
        panic!("file read must launch exactly one stream");
    };
    assert!(matches!(
        first.delivery,
        EffectDeliveryCardinality::Stream {
            initial_credits: 4,
            max_in_flight: 4,
            ref credit_result_tags,
            ..
        } if credit_result_tags == &["Chunk".to_owned()]
    ));
    assert_eq!(
        session.pending_transient_effect_credits(first.call_id),
        Some(4)
    );
    assert!(
        session
            .complete_transient_effect(
                first.call_id,
                file_stream_outcome(
                    "Opened",
                    [
                        ("size", number(3)),
                        ("content_type", Value::Text("audio/wav".to_owned())),
                        ("display_name", Value::Text("fixture.wav".to_owned())),
                    ],
                ),
            )
            .is_err()
    );
    assert!(
        session
            .deliver_transient_effect_result(
                first.call_id,
                1,
                file_stream_outcome(
                    "Opened",
                    [
                        ("size", number(3)),
                        ("content_type", Value::Text("audio/wav".to_owned())),
                        ("display_name", Value::Text("fixture.wav".to_owned())),
                    ],
                ),
            )
            .is_err()
    );

    let opened = session
        .deliver_transient_effect_result(
            first.call_id,
            0,
            file_stream_outcome(
                "Opened",
                [
                    ("size", number(3)),
                    ("content_type", Value::Text("audio/wav".to_owned())),
                    ("display_name", Value::Text("fixture.wav".to_owned())),
                ],
            ),
        )
        .unwrap();
    assert!(opened.transient_effect_credit_grants.is_empty());
    assert_eq!(
        session.pending_transient_effect_credits(first.call_id),
        Some(4)
    );
    assert!(matches!(
        session.root_value_current("store.stream_result").unwrap(),
        Value::Tag { tag, .. } if tag == "Opened"
    ));

    let chunk = session
        .deliver_transient_effect_result(
            first.call_id,
            1,
            file_stream_outcome(
                "Chunk",
                [
                    ("sequence", number(0)),
                    ("offset", number(0)),
                    ("bytes", Value::Bytes(vec![1, 2, 3].into())),
                ],
            ),
        )
        .unwrap();
    assert_eq!(chunk.transient_effect_credit_grants[0].credits, 1);
    let finished = session
        .deliver_transient_effect_result(
            first.call_id,
            2,
            file_stream_outcome(
                "Finished",
                [
                    ("byte_count", number(3)),
                    ("digest", Value::Bytes(vec![9; 32].into())),
                    ("retained", retained_content_outcome("NotRetained", None)),
                ],
            ),
        )
        .unwrap();
    assert!(finished.transient_effect_credit_grants.is_empty());
    assert_eq!(session.pending_transient_effect_count(), 0);
    assert!(session
        .deliver_transient_effect_result(first.call_id, 3, file_stream_outcome("Cancelled", []),)
        .is_err());

    let replacement_source = session
        .apply(SourceEvent {
            sequence: 2,
            source: read,
            route: route_token(&session, read, None),
            target: None,
            payload: file_stream_payload(),
        })
        .unwrap();
    let replacement = replacement_source.transient_effects[0].clone();
    let replaced_again = session
        .apply(SourceEvent {
            sequence: 3,
            source: read,
            route: route_token(&session, read, None),
            target: None,
            payload: file_stream_payload(),
        })
        .unwrap();
    assert_eq!(
        replaced_again.cancelled_transient_effects,
        vec![replacement.call_id]
    );
    assert_eq!(session.pending_transient_effect_count(), 1);
    assert!(
        session
            .deliver_transient_effect_result(
                replacement.call_id,
                0,
                file_stream_outcome("Cancelled", []),
            )
            .is_err()
    );
}

#[test]
fn byte_stream_semantics_reject_malformed_results_without_advancing_the_call() {
    let machine = file_stream_effect_machine();
    let read = source_id(&machine, "store.read");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let invocation = session
        .apply(SourceEvent {
            sequence: 1,
            source: read,
            route: route_token(&session, read, None),
            target: None,
            payload: file_stream_payload(),
        })
        .unwrap()
        .transient_effects
        .remove(0);

    let opened = file_stream_outcome(
        "Opened",
        [
            ("size", number(3)),
            (
                "content_type",
                Value::Text("application/octet-stream".to_owned()),
            ),
            ("display_name", Value::Text("fixture.bin".to_owned())),
        ],
    );
    session
        .deliver_transient_effect_result(invocation.call_id, 0, opened.clone())
        .unwrap();

    let repeated_open = session
        .deliver_transient_effect_result(invocation.call_id, 1, opened)
        .unwrap_err();
    assert!(
        repeated_open
            .to_string()
            .contains("one non-terminal first result")
    );

    session
        .deliver_transient_effect_result(
            invocation.call_id,
            1,
            file_stream_outcome(
                "Chunk",
                [
                    ("sequence", number(0)),
                    ("offset", number(0)),
                    ("bytes", Value::Bytes(vec![1, 2, 3].into())),
                ],
            ),
        )
        .unwrap();

    let wrong_count = session
        .deliver_transient_effect_result(
            invocation.call_id,
            2,
            file_stream_outcome(
                "Finished",
                [
                    ("byte_count", number(2)),
                    ("digest", Value::Bytes(vec![9; 32].into())),
                    ("retained", retained_content_outcome("NotRetained", None)),
                ],
            ),
        )
        .unwrap_err();
    assert!(wrong_count.to_string().contains("declared size"));

    session
        .deliver_transient_effect_result(
            invocation.call_id,
            2,
            file_stream_outcome(
                "Finished",
                [
                    ("byte_count", number(3)),
                    ("digest", Value::Bytes(vec![9; 32].into())),
                    ("retained", retained_content_outcome("NotRetained", None)),
                ],
            ),
        )
        .unwrap();
}

#[test]
fn content_progress_semantics_reject_unstable_totals_without_advancing_the_call() {
    let machine = content_import_effect_machine();
    let import = source_id(&machine, "store.import");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let invocation = session
        .apply(SourceEvent {
            sequence: 1,
            source: import,
            route: route_token(&session, import, None),
            target: None,
            payload: file_stream_payload(),
        })
        .unwrap()
        .transient_effects
        .remove(0);

    session
        .deliver_transient_effect_result(
            invocation.call_id,
            0,
            file_stream_outcome(
                "Started",
                [
                    ("byte_count", number(3)),
                    ("media", Value::Text("application/octet-stream".to_owned())),
                    ("display_name", Value::Text("fixture.bin".to_owned())),
                ],
            ),
        )
        .unwrap();

    let unstable_total = session
        .deliver_transient_effect_result(
            invocation.call_id,
            1,
            file_stream_outcome(
                "Progress",
                [("completed_bytes", number(2)), ("total_bytes", number(4))],
            ),
        )
        .unwrap_err();
    assert!(unstable_total.to_string().contains("one total byte count"));

    session
        .deliver_transient_effect_result(
            invocation.call_id,
            1,
            file_stream_outcome(
                "Progress",
                [("completed_bytes", number(2)), ("total_bytes", number(3))],
            ),
        )
        .unwrap();
    session
        .deliver_transient_effect_result(
            invocation.call_id,
            2,
            file_stream_outcome("Imported", [("content", content_ref_value())]),
        )
        .unwrap();
}

#[test]
fn nested_effect_guards_ignore_partial_variants_and_invoke_only_the_retained_branch() {
    let machine = nested_stream_effect_chain_machine();
    let read = source_id(&machine, "store.read");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    let first_file = session
        .apply(SourceEvent {
            sequence: 1,
            source: read,
            route: route_token(&session, read, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap()
        .transient_effects
        .remove(0);
    for (sequence, result) in [
        (
            0,
            file_stream_outcome(
                "Opened",
                [
                    ("size", number(3)),
                    (
                        "content_type",
                        Value::Text("application/octet-stream".to_owned()),
                    ),
                    ("display_name", Value::Text("primary.vcd".to_owned())),
                ],
            ),
        ),
        (
            1,
            file_stream_outcome(
                "Chunk",
                [
                    ("sequence", number(0)),
                    ("offset", number(0)),
                    ("bytes", Value::Bytes(vec![1, 2, 3].into())),
                ],
            ),
        ),
        (
            2,
            file_stream_outcome(
                "Finished",
                [
                    ("byte_count", number(3)),
                    ("digest", Value::Bytes(vec![9; 32].into())),
                    ("retained", retained_content_outcome("NotRetained", None)),
                ],
            ),
        ),
    ] {
        let turn = session
            .deliver_transient_effect_result(first_file.call_id, sequence, result)
            .unwrap();
        assert!(
            turn.transient_effects.is_empty(),
            "a non-retained or nonterminal stream result invoked the nested effect"
        );
    }

    let second_file = session
        .apply(SourceEvent {
            sequence: 2,
            source: read,
            route: route_token(&session, read, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap()
        .transient_effects
        .remove(0);
    let content = content_ref_value();
    session
        .deliver_transient_effect_result(
            second_file.call_id,
            0,
            file_stream_outcome(
                "Opened",
                [
                    ("size", number(3)),
                    (
                        "content_type",
                        Value::Text("application/octet-stream".to_owned()),
                    ),
                    ("display_name", Value::Text("primary.vcd".to_owned())),
                ],
            ),
        )
        .unwrap();
    session
        .deliver_transient_effect_result(
            second_file.call_id,
            1,
            file_stream_outcome(
                "Chunk",
                [
                    ("sequence", number(0)),
                    ("offset", number(0)),
                    ("bytes", Value::Bytes(vec![1, 2, 3].into())),
                ],
            ),
        )
        .unwrap();
    let turn = session
        .deliver_transient_effect_result(
            second_file.call_id,
            2,
            file_stream_outcome(
                "Finished",
                [
                    ("byte_count", number(3)),
                    ("digest", Value::Bytes(vec![9; 32].into())),
                    (
                        "retained",
                        retained_content_outcome("Retained", Some(content.clone())),
                    ),
                ],
            ),
        )
        .unwrap();
    let [waveform] = turn.transient_effects.as_slice() else {
        panic!(
            "the retained terminal result did not invoke exactly one nested effect: {:?}",
            turn.transient_effects
        );
    };
    assert_eq!(
        waveform.intent,
        Value::Record(BTreeMap::from([("content".to_owned(), content)]))
    );
}

#[test]
fn nonmatching_source_guard_invalidates_the_owned_transient_effect() {
    let mut machine = file_stream_effect_machine();
    let read = source_id(&machine, "store.read");
    let start_constant = PlanConstantId(machine.constants.len());
    machine.constants.push(PlanConstant {
        id: start_constant,
        value: PlanConstantValue::Text {
            value: "start".to_owned(),
        },
    });
    let left = row_field(
        &mut machine.row_expressions,
        ValueRef::SourcePayload {
            source_id: read,
            field: SourcePayloadField::Text,
        },
    );
    let right = row_constant(&mut machine.row_expressions, start_constant);
    let gate = row(
        &mut machine.row_expressions,
        PlanRowExpressionNode::NumberInfix {
            op: PlanInfixOp::Equal,
            left,
            right,
        },
    );
    let effect_op = machine
        .regions
        .iter_mut()
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::StateUpdate {
                    effect: Some(_),
                    ..
                }
            )
        })
        .expect("file stream plan has an effect update");
    let PlanOpKind::StateUpdate {
        effect: Some(effect),
        ..
    } = &mut effect_op.kind
    else {
        unreachable!();
    };
    effect.gate = gate;
    effect_op
        .synchronize_expression_inputs(&machine.row_expressions)
        .unwrap();
    machine.capability_summary = derive_capability_summary(&machine);

    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let mut start_payload = file_stream_payload();
    start_payload.text = Some("start".to_owned());
    let first = session
        .apply(SourceEvent {
            sequence: 1,
            source: read,
            route: route_token(&session, read, None),
            target: None,
            payload: start_payload,
        })
        .unwrap()
        .transient_effects
        .remove(0);

    let mut stop_payload = file_stream_payload();
    stop_payload.text = Some("stop".to_owned());
    let stopped = session
        .apply(SourceEvent {
            sequence: 2,
            source: read,
            route: route_token(&session, read, None),
            target: None,
            payload: stop_payload,
        })
        .unwrap();

    assert!(stopped.transient_effects.is_empty());
    assert_eq!(stopped.cancelled_transient_effects, vec![first.call_id]);
    assert_eq!(session.pending_transient_effect_count(), 0);
    assert!(session
        .deliver_transient_effect_result(
            first.call_id,
            0,
            file_stream_outcome("Cancelled", []),
        )
        .is_err());

    let mut restart_payload = file_stream_payload();
    restart_payload.text = Some("start".to_owned());
    let restarted = session
        .apply(SourceEvent {
            sequence: 3,
            source: read,
            route: route_token(&session, read, None),
            target: None,
            payload: restart_payload,
        })
        .unwrap();
    let [second] = restarted.transient_effects.as_slice() else {
        panic!("re-entering the active WHILE branch must start a fresh stream");
    };
    assert_ne!(second.call_id, first.call_id);
    assert_eq!(session.pending_transient_effect_count(), 1);
}

#[test]
fn rollback_of_owner_replacement_restores_the_previous_transient_effect() {
    let machine = file_stream_effect_machine();
    let read = source_id(&machine, "store.read");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let first = session
        .apply(SourceEvent {
            sequence: 1,
            source: read,
            route: route_token(&session, read, None),
            target: None,
            payload: file_stream_payload(),
        })
        .unwrap()
        .transient_effects
        .remove(0);
    let replacement_turn = session
        .apply(SourceEvent {
            sequence: 2,
            source: read,
            route: route_token(&session, read, None),
            target: None,
            payload: file_stream_payload(),
        })
        .unwrap();
    let replacement = replacement_turn.transient_effects[0].clone();
    assert_eq!(
        replacement_turn.cancelled_transient_effects,
        vec![first.call_id]
    );

    session.rollback_unsettled_turn().unwrap();

    assert_eq!(session.pending_transient_effect_count(), 1);
    assert!(
        session
            .deliver_transient_effect_result(
                replacement.call_id,
                0,
                file_stream_outcome("Cancelled", []),
            )
            .is_err()
    );
    session
        .deliver_transient_effect_result(
            first.call_id,
            0,
            file_stream_outcome(
                "Opened",
                [
                    ("size", number(3)),
                    (
                        "content_type",
                        Value::Text("application/octet-stream".to_owned()),
                    ),
                    ("display_name", Value::Text("primary.bin".to_owned())),
                ],
            ),
        )
        .unwrap();
}

#[test]
fn removing_an_indexed_row_invalidates_its_owned_transient_effect() {
    let machine = indexed_file_stream_effect_machine();
    let list_slot = &machine.storage_layout.list_slots[0];
    let row_field_names = list_slot
        .row_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<BTreeSet<_>>();
    assert!(!row_field_names.contains("open"));
    assert!(!row_field_names.contains("remove"));
    let stream_result = list_slot
        .row_fields
        .iter()
        .find(|field| field.name == "stream_result")
        .expect("stream result runtime field")
        .field_id;
    assert!(machine.persistence.lists.iter().all(|memory| {
        memory
            .row_fields
            .iter()
            .all(|field| field.runtime_field_id != Some(stream_result))
    }));
    let open = machine
        .source_routes
        .iter()
        .find(|route| route.path.ends_with(".open"))
        .expect("mapped row exposes open")
        .source_id;
    let remove = machine
        .source_routes
        .iter()
        .find(|route| route.path.ends_with(".remove"))
        .expect("mapped row exposes remove")
        .source_id;
    let list = machine.storage_layout.list_slots[0].list_id;
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let row = session.list_rows_current(list).unwrap()[0];
    let invocation = session
        .apply(SourceEvent {
            sequence: 1,
            source: open,
            route: route_token(&session, open, Some(row)),
            target: Some(row),
            payload: SourcePayload::default(),
        })
        .unwrap()
        .transient_effects
        .remove(0);
    assert_eq!(invocation.target, Some(row));

    let removed = session
        .apply(SourceEvent {
            sequence: 2,
            source: remove,
            route: route_token(&session, remove, Some(row)),
            target: Some(row),
            payload: SourcePayload::default(),
        })
        .unwrap();

    assert!(session.list_rows_current(list).unwrap().is_empty());
    assert_eq!(
        removed.cancelled_transient_effects,
        vec![invocation.call_id]
    );
    assert_eq!(session.pending_transient_effect_count(), 0);
}

#[test]
fn mapped_source_row_does_not_become_the_root_effect_result_owner() {
    let machine = mapped_request_root_file_stream_effect_machine();
    let route = machine
        .source_routes
        .iter()
        .find(|route| route.scope_id.is_some() && route.path.ends_with(".open"))
        .unwrap();
    let source = route.source_id;
    let scope = route.scope_id.unwrap();
    let list = machine
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.scope_id == Some(scope))
        .unwrap()
        .list_id;
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let row = session.list_rows_current(list).unwrap()[0];
    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: route_token(&session, source, Some(row)),
            target: Some(row),
            payload: SourcePayload::default(),
        })
        .unwrap();
    let [invocation] = turn.transient_effects.as_slice() else {
        panic!("mapped source must emit exactly one root stream invocation");
    };
    assert_eq!(invocation.target, None);
    session
        .deliver_transient_effect_result(
            invocation.call_id,
            0,
            file_stream_outcome(
                "Opened",
                [
                    ("size", number(4)),
                    (
                        "content_type",
                        Value::Text("application/octet-stream".to_owned()),
                    ),
                    ("display_name", Value::Text("primary.bin".to_owned())),
                ],
            ),
        )
        .unwrap();
    assert!(matches!(
        session.root_value_current("store.stream_result").unwrap(),
        Value::Tag { tag, .. } if tag == "Opened"
    ));
}

fn source_id(machine: &MachinePlan, path: &str) -> SourceId {
    machine
        .source_routes
        .iter()
        .find(|route| route.path == path)
        .unwrap_or_else(|| panic!("missing SOURCE route `{path}`"))
        .source_id
}

fn state_id(machine: &MachinePlan, label: &str) -> StateId {
    let id = &machine
        .debug_map
        .state_slots
        .iter()
        .find(|entry| entry.label == label)
        .unwrap_or_else(|| panic!("missing state debug label `{label}`"))
        .id;
    StateId(id.strip_prefix("state:").unwrap().parse().unwrap())
}

fn enqueue_item(turn: &Turn) -> boon_persistence::DurableOutboxItem {
    let [boon_persistence::DurableOutboxChange::Enqueue { item }] = turn.outbox_changes.as_slice()
    else {
        panic!("expected one outbox enqueue, got {:?}", turn.outbox_changes);
    };
    item.clone()
}

fn dispatch_item(
    session: &mut MachineInstance,
    item: &boon_persistence::DurableOutboxItem,
) -> boon_persistence::DurableOutboxItem {
    let turn = session.begin_effect_dispatch(item).unwrap();
    let [
        boon_persistence::DurableOutboxChange::BeginDispatch {
            item_id,
            expected_revision,
            next_revision,
            attempt,
            turn_sequence,
        },
    ] = turn.outbox_changes.as_slice()
    else {
        panic!("expected one begin-dispatch change");
    };
    assert_eq!(*item_id, item.item_id);
    assert_eq!(*expected_revision, item.revision);
    let mut dispatched = item.clone();
    dispatched.revision = *next_revision;
    dispatched.updated_turn_sequence = *turn_sequence;
    dispatched.state = boon_persistence::DurableOutboxState::Dispatching { attempt: *attempt };
    dispatched
}

fn result_variant(
    tag: &str,
    fields: impl IntoIterator<Item = (&'static str, boon_persistence::StoredValue)>,
) -> boon_persistence::StoredValue {
    boon_persistence::StoredValue::Tag {
        tag: tag.to_owned(),
        fields: fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    }
}

fn apply_register_effect(
    machine: &MachinePlan,
    sequence: u64,
) -> (MachineInstance, boon_persistence::DurableOutboxItem) {
    let mut session = MachineInstance::new(machine.clone(), SessionOptions::default()).unwrap();
    let register = source_id(machine, "store.register");
    let turn = session
        .apply(SourceEvent {
            sequence,
            source: register,
            route: route_token(&session, register, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    let pending = enqueue_item(&turn);
    let boon_persistence::StoredValue::Object(intent) = &pending.intent else {
        panic!("effect intent must be a durable record");
    };
    assert_eq!(
        intent["simulation"],
        boon_persistence::StoredValue::Tag {
            tag: "Success".to_owned(),
            fields: BTreeMap::new(),
        }
    );
    let dispatched = dispatch_item(&mut session, &pending);
    (session, dispatched)
}

#[test]
fn correlated_effect_completion_routes_each_registration_variant_with_typed_fields() {
    let machine = typed_passkey_effect_machine();
    let cases = [
        (
            result_variant(
                "RegistrationSucceeded",
                [
                    (
                        "account_id",
                        boon_persistence::StoredValue::Text("account-success".to_owned()),
                    ),
                    (
                        "credential_id",
                        boon_persistence::StoredValue::Text("credential-success".to_owned()),
                    ),
                    (
                        "label",
                        boon_persistence::StoredValue::Text("Primary".to_owned()),
                    ),
                    (
                        "workspace_grant_bound",
                        boon_persistence::StoredValue::truth(true),
                    ),
                ],
            ),
            "RegistrationSucceeded",
            Some((
                "store.result_account_id",
                Value::Text("account-success".to_owned()),
            )),
        ),
        (
            result_variant("RegistrationCancelled", []),
            "RegistrationCancelled",
            None,
        ),
        (
            result_variant(
                "RegistrationFailed",
                [
                    (
                        "code",
                        boon_persistence::StoredValue::Text("not_allowed".to_owned()),
                    ),
                    (
                        "message",
                        boon_persistence::StoredValue::Text("Not allowed".to_owned()),
                    ),
                    ("retryable", boon_persistence::StoredValue::truth(true)),
                ],
            ),
            "RegistrationFailed",
            Some(("store.failure_retryable", Value::truth(true))),
        ),
        (
            result_variant(
                "DuplicateCredential",
                [
                    (
                        "account_id",
                        boon_persistence::StoredValue::Text("account-duplicate".to_owned()),
                    ),
                    (
                        "credential_id",
                        boon_persistence::StoredValue::Text("credential-duplicate".to_owned()),
                    ),
                ],
            ),
            "DuplicateCredential",
            Some((
                "store.result_credential_id",
                Value::Text("credential-duplicate".to_owned()),
            )),
        ),
    ];

    for (index, (outcome, expected_tag, typed_field)) in cases.into_iter().enumerate() {
        let (mut session, item) = apply_register_effect(&machine, index as u64 + 1);
        let turn = session.complete_effect(&item, outcome.clone()).unwrap();
        assert!(matches!(
            turn.outbox_changes.as_slice(),
            [boon_persistence::DurableOutboxChange::Complete {
                item_id,
                expected_revision: 1,
                next_revision: 2,
                attempt: 1,
                outcome: completed,
                ..
            }] if *item_id == item.item_id && *completed == outcome
        ));
        let snapshot = session.snapshot().unwrap();
        assert_eq!(
            snapshot.states[&state_id(&machine, "store.last_result")],
            Value::tag(expected_tag)
        );
        if let Some((label, expected)) = typed_field {
            assert_eq!(snapshot.states[&state_id(&machine, label)], expected);
        }
        assert_eq!(
            snapshot
                .lists
                .values()
                .map(|rows| rows.len())
                .sum::<usize>(),
            usize::from(expected_tag == "RegistrationSucceeded"),
            "only a successful effect result may append a credential"
        );
    }
}

#[test]
fn correlated_effect_completion_rejects_wrong_variant_and_shape_atomically() {
    let machine = typed_passkey_effect_machine();
    let (mut session, item) = apply_register_effect(&machine, 1);
    let before = session.authority_snapshot().unwrap();

    assert!(
        session
            .complete_effect(&item, result_variant("UnknownResult", []))
            .is_err()
    );
    assert_eq!(session.authority_snapshot().unwrap(), before);
    assert!(
        session
            .complete_effect(
                &item,
                result_variant(
                    "RegistrationSucceeded",
                    [
                        (
                            "account_id",
                            boon_persistence::StoredValue::Text("account-1".to_owned()),
                        ),
                        (
                            "credential_id",
                            boon_persistence::StoredValue::Text("credential-1".to_owned()),
                        ),
                    ],
                ),
            )
            .is_err()
    );
    assert_eq!(session.authority_snapshot().unwrap(), before);

    session
        .complete_effect(&item, result_variant("RegistrationCancelled", []))
        .unwrap();
    assert_eq!(
        session.snapshot().unwrap().states[&state_id(&machine, "store.last_result")],
        Value::tag("RegistrationCancelled")
    );
}

#[test]
fn effect_result_is_a_state_not_an_externally_dispatchable_source() {
    let machine = typed_passkey_effect_machine();
    assert!(
        machine
            .source_routes
            .iter()
            .all(|source| source.path != "store.registration_result")
    );
    assert!(
        machine
            .debug_map
            .state_slots
            .iter()
            .any(|state| { state.label == "store.registration_result" })
    );
}

#[test]
fn identical_effect_intents_on_distinct_source_turns_have_distinct_identities() {
    let machine = typed_passkey_effect_machine();
    let register = source_id(&machine, "store.register");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let first = enqueue_item(
        &session
            .apply(SourceEvent {
                sequence: 10,
                source: register,
                route: route_token(&session, register, None),
                target: None,
                payload: SourcePayload::default(),
            })
            .unwrap(),
    );
    let second = enqueue_item(
        &session
            .apply(SourceEvent {
                sequence: 11,
                source: register,
                route: route_token(&session, register, None),
                target: None,
                payload: SourcePayload::default(),
            })
            .unwrap(),
    );

    assert_eq!(first.invocation_id, second.invocation_id);
    assert_eq!(first.intent, second.intent);
    assert_ne!(first.created_turn_sequence, second.created_turn_sequence);
    assert_ne!(first.idempotency_key, second.idempotency_key);
    assert_ne!(first.item_id, second.item_id);
}

const SESSION_INFO_SOURCE: &str = r#"
outputs: [
    status: SessionInfo/status()
    principal: SessionInfo/principal()
]
"#;

fn session_info_plan() -> MachinePlan {
    compile_test_source(
        "session-info.bn",
        SESSION_INFO_SOURCE,
        TargetProfile::SoftwareDefault,
        ProgramRole::Session,
    )
    .unwrap()
    .plan
}

#[test]
fn machine_template_shares_verified_plan_metadata_across_isolated_instances() {
    let template = MachineTemplate::new(session_info_plan()).unwrap();
    let first = template
        .instantiate(SessionOptions::default())
        .unwrap()
        .build()
        .unwrap();
    let second = template
        .instantiate(SessionOptions::default())
        .unwrap()
        .build()
        .unwrap();
    assert!(first.shares_template_metadata(&template));
    assert!(second.shares_template_metadata(&template));
}

#[test]
fn session_info_intrinsics_default_to_current_and_anonymous_without_hidden_identity() {
    let mut session = MachineInstance::new(session_info_plan(), SessionOptions::default()).unwrap();
    assert_eq!(
        session.output_value_current("status").unwrap(),
        Value::tag("Current")
    );
    assert_eq!(
        session.output_value_current("principal").unwrap(),
        Value::tag("Anonymous")
    );
}

#[test]
fn session_info_context_updates_are_current_and_canonical() {
    let options = SessionOptions {
        session_context: SessionContext::Available {
            status: SessionConnectionStatus::Connecting,
            principal: SessionPrincipal::authenticated(
                "person-42",
                ["viewer", "operator", "viewer"],
            )
            .unwrap(),
        },
        ..SessionOptions::default()
    };
    let mut session = MachineInstance::new(session_info_plan(), options).unwrap();

    assert_eq!(
        session.output_value_current("status").unwrap(),
        Value::tag("Connecting")
    );
    assert_eq!(
        session.output_value_current("principal").unwrap(),
        Value::tagged(
            "Authenticated",
            BTreeMap::from([
                ("subject".to_owned(), Value::Text("person-42".to_owned()),),
                (
                    "roles".to_owned(),
                    Value::List(vec![
                        Value::Text("operator".to_owned()),
                        Value::Text("viewer".to_owned()),
                    ]),
                ),
            ]),
        )
    );

    assert!(
        session
            .update_session_context(
                SessionConnectionStatus::Failed {
                    code: "transport_timeout".to_owned(),
                },
                SessionPrincipal::Anonymous,
            )
            .unwrap()
            .is_some()
    );
    assert_eq!(
        session.output_value_current("status").unwrap(),
        Value::tagged(
            "Failed",
            BTreeMap::from([(
                "code".to_owned(),
                Value::Text("transport_timeout".to_owned()),
            ),]),
        )
    );
    assert_eq!(
        session.output_value_current("principal").unwrap(),
        Value::tag("Anonymous")
    );
}

#[test]
fn session_info_context_rejects_unbounded_or_noncanonical_host_values() {
    let invalid = [
        SessionOptions {
            session_context: SessionContext::Available {
                status: SessionConnectionStatus::Failed {
                    code: "contains spaces".to_owned(),
                },
                principal: SessionPrincipal::Anonymous,
            },
            ..SessionOptions::default()
        },
        SessionOptions {
            session_context: SessionContext::Available {
                status: SessionConnectionStatus::Current,
                principal: SessionPrincipal::Authenticated {
                    subject: "person".to_owned(),
                    roles: vec!["viewer".to_owned(), "operator".to_owned()],
                },
            },
            ..SessionOptions::default()
        },
        SessionOptions {
            session_context: SessionContext::Available {
                status: SessionConnectionStatus::Current,
                principal: SessionPrincipal::Authenticated {
                    subject: "s".repeat(MAX_SESSION_INFO_TEXT_BYTES + 1),
                    roles: Vec::new(),
                },
            },
            ..SessionOptions::default()
        },
    ];
    for options in invalid {
        let error = MachineInstance::new(session_info_plan(), options)
            .err()
            .expect("invalid SessionInfo context must be rejected");
        assert!(matches!(error, Error::InvalidOptions(_)));
    }
}

struct DistributedSessionFixture {
    plan: MachinePlan,
    import_id: ImportId,
    value_export_id: ExportId,
    call_site_id: RemoteCallSiteId,
    second_call_site_id: RemoteCallSiteId,
    function_export_id: ExportId,
    function_argument_id: DistributedArgumentId,
    producer_argument_import: ImportId,
    second_producer_argument_import: ImportId,
    undeclared_import_id: ImportId,
    undeclared_export_id: ExportId,
}

fn executor_distributed_declaration(semantic_path: &str) -> DistributedDeclarationId {
    DistributedDeclarationId::from_semantic_path("PlanExecutorDistributedFixture", semantic_path)
        .unwrap()
}

fn distributed_session_fixture() -> DistributedSessionFixture {
    let mut row_expressions = PlanRowExpressionArena::new();
    let application_identity =
        ApplicationIdentity::new("dev.boon.plan-executor-tests", "test", "local");
    let graph = DistributedGraphIdentityPlan::new(
        &application_identity,
        executor_distributed_declaration("graph"),
        1,
    )
    .unwrap();

    let server_declaration = executor_distributed_declaration("endpoint.server");
    let server_endpoint_id = DistributedEndpointId::from_identity(
        graph.graph_id,
        ProgramRole::Server,
        server_declaration,
    )
    .unwrap();
    let server_value = DistributedValueExportPlan::new(
        graph.graph_id,
        server_endpoint_id,
        executor_distributed_declaration("server.value.count"),
        1,
        ProgramRole::Server,
        false,
        ValueRef::Constant(PlanConstantId(99)),
        DataTypePlan::Number,
    )
    .unwrap();

    let session_declaration = executor_distributed_declaration("endpoint.session");
    let session_endpoint_id = DistributedEndpointId::from_identity(
        graph.graph_id,
        ProgramRole::Session,
        session_declaration,
    )
    .unwrap();
    let value_import = DistributedValueImportPlan::new(
        graph.graph_id,
        session_endpoint_id,
        executor_distributed_declaration("session.import.server_count"),
        1,
        ProgramRole::Session,
        &server_value,
    )
    .unwrap();
    let value_export = DistributedValueExportPlan::new(
        graph.graph_id,
        session_endpoint_id,
        executor_distributed_declaration("session.value.current_count"),
        1,
        ProgramRole::Session,
        false,
        ValueRef::Field(FieldId(0)),
        DataTypePlan::Number,
    )
    .unwrap();

    let function_declaration = executor_distributed_declaration("session.function.double");
    let function_export = DistributedFunctionExportPlan::new(
        graph.graph_id,
        session_endpoint_id,
        function_declaration,
        1,
        ProgramRole::Session,
        vec![("value".to_owned(), DataTypePlan::Number)],
        DataTypePlan::Number,
    )
    .unwrap();
    let function_export_id = function_export.export_id;
    let function_argument_id =
        DistributedArgumentId::from_parameter_name(function_export_id, "value").unwrap();
    let client_declaration = executor_distributed_declaration("endpoint.client");
    let client_endpoint_id = DistributedEndpointId::from_identity(
        graph.graph_id,
        ProgramRole::Client,
        client_declaration,
    )
    .unwrap();
    let remote_argument = row_constant(&mut row_expressions, PlanConstantId(0));
    let remote_call = RemoteCallSitePlan::new(
        graph.graph_id,
        client_endpoint_id,
        executor_distributed_declaration("client.call.session_double"),
        1,
        ProgramRole::Client,
        PlanOwner::root(),
        &function_export,
        vec![("value".to_owned(), remote_argument)],
        Vec::new(),
        DistributedCallMode::Current,
        None,
        Vec::new(),
    )
    .unwrap();
    let second_remote_argument = row_constant(&mut row_expressions, PlanConstantId(1));
    let second_remote_call = RemoteCallSitePlan::new(
        graph.graph_id,
        client_endpoint_id,
        executor_distributed_declaration("client.call.session_double_second"),
        1,
        ProgramRole::Client,
        PlanOwner::root(),
        &function_export,
        vec![("value".to_owned(), second_remote_argument)],
        Vec::new(),
        DistributedCallMode::Current,
        None,
        Vec::new(),
    )
    .unwrap();
    let endpoint = DistributedEndpointContractPlan::new(
        &graph,
        session_declaration,
        1,
        ProgramRole::Session,
        vec![value_export.clone()],
        vec![value_import.clone()],
        Vec::new(),
        Vec::new(),
        vec![function_export.clone()],
        Vec::new(),
    )
    .unwrap();
    let client_endpoint = DistributedEndpointContractPlan::new(
        &graph,
        client_declaration,
        1,
        ProgramRole::Client,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![remote_call.clone(), second_remote_call.clone()],
    )
    .unwrap();
    let server_endpoint = DistributedEndpointContractPlan::new(
        &graph,
        server_declaration,
        1,
        ProgramRole::Server,
        vec![server_value.clone()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    let linked_graph = DistributedGraphPlan::new(
        &application_identity,
        graph.clone(),
        vec![client_endpoint, endpoint, server_endpoint],
    )
    .unwrap();

    let remote_count = row_field(
        &mut row_expressions,
        ValueRef::DistributedImport(value_import.import_id),
    );
    let producer_instance = ProducerFunctionInstancePlan::new(
        remote_call.call_site_id,
        &function_export,
        PlanOwner {
            static_owner: PlanStaticOwnerId(0),
            ancestors: Vec::new(),
        },
        DistributedCallMode::Current,
        None,
        ProducerFunctionOwnershipPlan::new(
            vec![PlanStaticOwnerId(0)],
            Vec::new(),
            Vec::new(),
            vec![FieldId(1)],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        ValueRef::Field(FieldId(1)),
    )
    .unwrap();
    let producer_argument_import = producer_instance.arguments[0].import_id;
    let producer_argument = row_field(
        &mut row_expressions,
        ValueRef::DistributedImport(producer_argument_import),
    );
    let producer_expression = row(
        &mut row_expressions,
        PlanRowExpressionNode::NumberInfix {
            op: PlanInfixOp::Add,
            left: producer_argument,
            right: producer_argument,
        },
    );
    let mut producer_result = derived(
        1,
        1,
        vec![ValueRef::DistributedImport(producer_argument_import)],
        Some(producer_expression),
    );
    let PlanOpKind::DerivedValue {
        startup_recompute, ..
    } = &mut producer_result.kind
    else {
        unreachable!("derived helper always constructs a derived value")
    };
    *startup_recompute = false;
    let second_producer_instance = ProducerFunctionInstancePlan::new(
        second_remote_call.call_site_id,
        &function_export,
        PlanOwner {
            static_owner: PlanStaticOwnerId(1),
            ancestors: Vec::new(),
        },
        DistributedCallMode::Current,
        None,
        ProducerFunctionOwnershipPlan::new(
            vec![PlanStaticOwnerId(1)],
            Vec::new(),
            Vec::new(),
            vec![FieldId(2)],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        ValueRef::Field(FieldId(2)),
    )
    .unwrap();
    let second_producer_argument_import = second_producer_instance.arguments[0].import_id;
    let second_producer_argument = row_field(
        &mut row_expressions,
        ValueRef::DistributedImport(second_producer_argument_import),
    );
    let second_producer_expression = row(
        &mut row_expressions,
        PlanRowExpressionNode::NumberInfix {
            op: PlanInfixOp::Add,
            left: second_producer_argument,
            right: second_producer_argument,
        },
    );
    let mut second_producer_result = derived(
        2,
        2,
        vec![ValueRef::DistributedImport(second_producer_argument_import)],
        Some(second_producer_expression),
    );
    let PlanOpKind::DerivedValue {
        startup_recompute, ..
    } = &mut second_producer_result.kind
    else {
        unreachable!("derived helper always constructs a derived value")
    };
    *startup_recompute = false;
    let mut machine = plan(
        RootOutputDemand::Selected(vec![FieldId(0)]),
        row_expressions,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![
            derived(
                0,
                0,
                vec![ValueRef::DistributedImport(value_import.import_id)],
                Some(remote_count),
            ),
            producer_result,
            second_producer_result,
        ],
        Vec::new(),
        Vec::new(),
        vec![
            (FieldId(0), "store.remote_count"),
            (FieldId(1), "producer.double.result"),
            (FieldId(2), "producer.double_second.result"),
        ],
    );
    assert_eq!(machine.application.identity, application_identity);
    machine.program_role = ProgramRole::Session;
    machine.distributed_endpoint = Some(
        DistributedEndpointPlan::new(&application_identity, &linked_graph, ProgramRole::Session)
            .unwrap(),
    );
    machine.producer_function_instances = vec![producer_instance, second_producer_instance];
    machine
        .producer_function_instances
        .sort_by_key(|instance| instance.call_site_id);

    let undeclared_import_id = ImportId::from_value_identity(
        graph.graph_id,
        session_endpoint_id,
        executor_distributed_declaration("session.import.undeclared"),
    )
    .unwrap();

    DistributedSessionFixture {
        plan: machine,
        import_id: value_import.import_id,
        value_export_id: value_export.export_id,
        call_site_id: remote_call.call_site_id,
        second_call_site_id: second_remote_call.call_site_id,
        function_export_id,
        function_argument_id,
        producer_argument_import,
        second_producer_argument_import,
        undeclared_import_id,
        undeclared_export_id: server_value.export_id,
    }
}

struct AtomicDistributedContextFixture {
    plan: MachinePlan,
    first_import_id: ImportId,
    second_import_id: ImportId,
    call_site_id: RemoteCallSiteId,
    call_result_import_id: ImportId,
    undeclared_import_id: ImportId,
}

fn atomic_distributed_context_fixture() -> AtomicDistributedContextFixture {
    let mut row_expressions = PlanRowExpressionArena::new();
    let application_identity =
        ApplicationIdentity::new("dev.boon.plan-executor-tests", "test", "local");
    let graph = DistributedGraphIdentityPlan::new(
        &application_identity,
        executor_distributed_declaration("atomic.graph"),
        1,
    )
    .unwrap();
    let server_declaration = executor_distributed_declaration("atomic.endpoint.server");
    let server_endpoint_id = DistributedEndpointId::from_identity(
        graph.graph_id,
        ProgramRole::Server,
        server_declaration,
    )
    .unwrap();
    let first_export = DistributedValueExportPlan::new(
        graph.graph_id,
        server_endpoint_id,
        executor_distributed_declaration("atomic.server.value.first"),
        1,
        ProgramRole::Server,
        true,
        ValueRef::Constant(PlanConstantId(98)),
        DataTypePlan::Number,
    )
    .unwrap();
    let second_export = DistributedValueExportPlan::new(
        graph.graph_id,
        server_endpoint_id,
        executor_distributed_declaration("atomic.server.value.second"),
        1,
        ProgramRole::Server,
        true,
        ValueRef::Constant(PlanConstantId(99)),
        DataTypePlan::Number,
    )
    .unwrap();
    let function_declaration = executor_distributed_declaration("atomic.server.function.identity");
    let server_function = DistributedFunctionExportPlan::new(
        graph.graph_id,
        server_endpoint_id,
        function_declaration,
        1,
        ProgramRole::Server,
        vec![("value".to_owned(), DataTypePlan::Number)],
        DataTypePlan::Number,
    )
    .unwrap();

    let session_declaration = executor_distributed_declaration("atomic.endpoint.session");
    let session_endpoint_id = DistributedEndpointId::from_identity(
        graph.graph_id,
        ProgramRole::Session,
        session_declaration,
    )
    .unwrap();
    let first_import = DistributedValueImportPlan::new(
        graph.graph_id,
        session_endpoint_id,
        executor_distributed_declaration("atomic.session.import.first"),
        1,
        ProgramRole::Session,
        &first_export,
    )
    .unwrap();
    let second_import = DistributedValueImportPlan::new(
        graph.graph_id,
        session_endpoint_id,
        executor_distributed_declaration("atomic.session.import.second"),
        1,
        ProgramRole::Session,
        &second_export,
    )
    .unwrap();
    let remote_argument = row_field(
        &mut row_expressions,
        ValueRef::DistributedImport(first_import.import_id),
    );
    let remote_call = RemoteCallSitePlan::new(
        graph.graph_id,
        session_endpoint_id,
        executor_distributed_declaration("atomic.session.call.identity"),
        1,
        ProgramRole::Session,
        PlanOwner::root(),
        &server_function,
        vec![("value".to_owned(), remote_argument)],
        Vec::new(),
        DistributedCallMode::Current,
        None,
        Vec::new(),
    )
    .unwrap();
    let call_result_import_id = remote_call.result.current_import_id().unwrap();
    let endpoint = DistributedEndpointContractPlan::new(
        &graph,
        session_declaration,
        1,
        ProgramRole::Session,
        Vec::new(),
        vec![first_import.clone(), second_import.clone()],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![remote_call.clone()],
    )
    .unwrap();
    let client_endpoint = DistributedEndpointContractPlan::new(
        &graph,
        executor_distributed_declaration("atomic.endpoint.client"),
        1,
        ProgramRole::Client,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    let server_endpoint = DistributedEndpointContractPlan::new(
        &graph,
        server_declaration,
        1,
        ProgramRole::Server,
        vec![first_export, second_export],
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![server_function],
        Vec::new(),
    )
    .unwrap();
    let linked_graph = DistributedGraphPlan::new(
        &application_identity,
        graph.clone(),
        vec![client_endpoint, endpoint, server_endpoint],
    )
    .unwrap();
    let status = row(
        &mut row_expressions,
        PlanRowExpressionNode::Intrinsic {
            intrinsic: PlanIntrinsic::SessionInfoStatus,
        },
    );
    let principal = row(
        &mut row_expressions,
        PlanRowExpressionNode::Intrinsic {
            intrinsic: PlanIntrinsic::SessionInfoPrincipal,
        },
    );
    let first = row_field(
        &mut row_expressions,
        ValueRef::DistributedImport(first_import.import_id),
    );
    let second = row_field(
        &mut row_expressions,
        ValueRef::DistributedImport(second_import.import_id),
    );
    let call_result = row_field(
        &mut row_expressions,
        ValueRef::DistributedImport(call_result_import_id),
    );
    let context_expression = row(
        &mut row_expressions,
        PlanRowExpressionNode::Object {
            fields: vec![
                PlanRowObjectField {
                    name: "status".to_owned(),
                    value: status,
                    spread: false,
                },
                PlanRowObjectField {
                    name: "principal".to_owned(),
                    value: principal,
                    spread: false,
                },
                PlanRowObjectField {
                    name: "first".to_owned(),
                    value: first,
                    spread: false,
                },
                PlanRowObjectField {
                    name: "second".to_owned(),
                    value: second,
                    spread: false,
                },
                PlanRowObjectField {
                    name: "call_result".to_owned(),
                    value: call_result,
                    spread: false,
                },
            ],
        },
    );
    let mut machine = plan(
        RootOutputDemand::All,
        row_expressions,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![derived(
            0,
            0,
            vec![
                ValueRef::DistributedImport(first_import.import_id),
                ValueRef::DistributedImport(second_import.import_id),
                ValueRef::DistributedImport(call_result_import_id),
            ],
            Some(context_expression),
        )],
        Vec::new(),
        Vec::new(),
        vec![(FieldId(0), "store.distributed_context")],
    );
    machine.program_role = ProgramRole::Session;
    machine.distributed_endpoint = Some(
        DistributedEndpointPlan::new(&application_identity, &linked_graph, ProgramRole::Session)
            .unwrap(),
    );

    AtomicDistributedContextFixture {
        plan: machine,
        first_import_id: first_import.import_id,
        second_import_id: second_import.import_id,
        call_site_id: remote_call.call_site_id,
        call_result_import_id,
        undeclared_import_id: ImportId::from_value_identity(
            graph.graph_id,
            session_endpoint_id,
            executor_distributed_declaration("atomic.session.import.undeclared"),
        )
        .unwrap(),
    }
}

fn distributed_context_value(
    status: Value,
    principal: Value,
    first: Value,
    second: Value,
    call_result: Value,
) -> Value {
    Value::Record(BTreeMap::from([
        ("call_result".to_owned(), call_result),
        ("first".to_owned(), first),
        ("principal".to_owned(), principal),
        ("second".to_owned(), second),
        ("status".to_owned(), status),
    ]))
}

fn authenticated_principal_value(subject: &str, roles: &[&str]) -> Value {
    Value::tagged(
        "Authenticated",
        BTreeMap::from([
            ("subject".to_owned(), Value::Text(subject.to_owned())),
            (
                "roles".to_owned(),
                Value::List(
                    roles
                        .iter()
                        .map(|role| Value::Text((*role).to_owned()))
                        .collect(),
                ),
            ),
        ]),
    )
}

fn update_atomic_call_result(
    session: &mut MachineInstance,
    call_site_id: RemoteCallSiteId,
    content_revision: u64,
    value: Value,
) {
    let instances = session
        .distributed_call_instances_current(call_site_id)
        .unwrap();
    assert_eq!(instances.len(), 1, "fixture has one demanded current call");
    session
        .update_distributed_call_result(
            call_site_id,
            instances[0].call_instance_id,
            content_revision,
            value,
        )
        .unwrap();
}

#[test]
fn distributed_context_transaction_recomputes_dependents_once_with_the_complete_batch() {
    let fixture = atomic_distributed_context_fixture();
    let mut session = MachineInstance::new(
        fixture.plan,
        SessionOptions {
            session_context: SessionContext::Available {
                status: SessionConnectionStatus::Connecting,
                principal: SessionPrincipal::Anonymous,
            },
            ..SessionOptions::default()
        },
    )
    .unwrap();
    let principal = SessionPrincipal::authenticated("person-42", ["operator", "viewer"]).unwrap();
    let turn = session
        .update_distributed_context(
            SessionConnectionStatus::Current,
            principal,
            vec![
                DistributedImportUpdate::new(fixture.second_import_id, 7, number(22)),
                DistributedImportUpdate::new(fixture.first_import_id, 3, number(11)),
            ],
        )
        .unwrap()
        .expect("new distributed context must produce one internal turn");

    assert_eq!(turn.sequence, 1);
    assert_eq!(turn.source_sequence, None);
    assert_eq!(turn.metrics.recomputed_field_count, 1);
    assert_eq!(
        turn.metrics.recomputed_targets,
        vec![ValueTarget::Field(FieldId(0))]
    );
    assert!(
        turn.deltas
            .iter()
            .filter_map(|delta| match delta {
                Delta::SetValue {
                    target: ValueTarget::Field(FieldId(0)),
                    value,
                } => Some(value.clone()),
                _ => None,
            })
            .next()
            .is_none()
    );
    assert_eq!(
        turn.deltas
            .iter()
            .filter(|delta| matches!(delta, Delta::SetDistributedImport { .. }))
            .count(),
        2
    );
    assert!(turn.transient_effects.is_empty());
    assert!(turn.cancelled_transient_effects.is_empty());
    assert!(
        session
            .root_value_current("store.distributed_context")
            .is_err()
    );
    assert_eq!(
        session.distributed_import_revision(fixture.first_import_id),
        Some(3)
    );
    assert_eq!(
        session.distributed_import_revision(fixture.second_import_id),
        Some(7)
    );
}

#[test]
fn distributed_context_patch_makes_session_available_and_preserves_omitted_imports() {
    let fixture = atomic_distributed_context_fixture();
    let mut session = MachineInstance::new(
        fixture.plan,
        SessionOptions {
            session_context: SessionContext::Unavailable,
            ..SessionOptions::default()
        },
    )
    .unwrap();
    session
        .update_distributed_context(
            SessionConnectionStatus::Current,
            SessionPrincipal::authenticated("origin-a", ["viewer"]).unwrap(),
            vec![
                DistributedImportUpdate::new(fixture.first_import_id, 5, number(11)),
                DistributedImportUpdate::new(fixture.second_import_id, 5, number(22)),
            ],
        )
        .unwrap()
        .expect("the patch API must install an available Session context");
    update_atomic_call_result(&mut session, fixture.call_site_id, 5, number(33));

    session
        .update_session_context(SessionConnectionStatus::Stale, SessionPrincipal::Anonymous)
        .unwrap()
        .expect("the context-only patch must become visible");

    assert_eq!(
        session
            .root_value_current("store.distributed_context")
            .unwrap(),
        distributed_context_value(
            Value::tag("Stale"),
            Value::tag("Anonymous"),
            number(11),
            number(22),
            number(33),
        )
    );
    for import_id in [fixture.first_import_id, fixture.second_import_id] {
        assert_eq!(session.distributed_import_revision(import_id), Some(5));
    }
    assert_eq!(
        session.distributed_import_revision(fixture.call_result_import_id),
        None
    );
}

#[test]
fn distributed_context_rejects_current_call_results_as_generic_imports() {
    let fixture = atomic_distributed_context_fixture();
    let mut session = MachineInstance::new(fixture.plan, SessionOptions::default()).unwrap();

    assert!(matches!(
        session.update_distributed_context(
            SessionConnectionStatus::Current,
            SessionPrincipal::Anonymous,
            vec![DistributedImportUpdate::new(
                fixture.call_result_import_id,
                1,
                number(33),
            )],
        ),
        Err(Error::InvalidEvent(detail)) if detail.contains("not declared")
    ));
    assert_eq!(
        session.distributed_import_revision(fixture.call_result_import_id),
        None
    );
}

#[test]
fn distributed_context_argument_change_invalidates_result_and_restarts_its_revision() {
    let fixture = atomic_distributed_context_fixture();
    let mut session = MachineInstance::new(fixture.plan, SessionOptions::default()).unwrap();
    let principal = SessionPrincipal::authenticated("origin-a", ["viewer"]).unwrap();
    session
        .replace_distributed_context(
            SessionContext::Available {
                status: SessionConnectionStatus::Current,
                principal: principal.clone(),
            },
            vec![
                DistributedImportUpdate::new(fixture.first_import_id, 5, number(11)),
                DistributedImportUpdate::new(fixture.second_import_id, 5, number(22)),
            ],
        )
        .unwrap()
        .expect("initial context must become current");
    let original_instance = session
        .distributed_call_instances_current(fixture.call_site_id)
        .unwrap()
        .into_iter()
        .next()
        .expect("initial call demand");
    update_atomic_call_result(&mut session, fixture.call_site_id, 5, number(33));

    session
        .update_distributed_context(
            SessionConnectionStatus::Current,
            principal,
            vec![DistributedImportUpdate::new(
                fixture.first_import_id,
                6,
                number(44),
            )],
        )
        .unwrap()
        .expect("argument update must recompute the current demand");
    let changed_instance = session
        .distributed_call_instances_current(fixture.call_site_id)
        .unwrap()
        .into_iter()
        .next()
        .expect("changed call demand");
    assert_eq!(
        changed_instance.call_instance_id, original_instance.call_instance_id,
        "call identity is stable while argument freshness is tracked separately"
    );
    assert_ne!(changed_instance.arguments, original_instance.arguments);
    assert!(
        session
            .root_value_current("store.distributed_context")
            .is_err()
    );

    update_atomic_call_result(&mut session, fixture.call_site_id, 1, number(55));
    assert_eq!(
        session
            .root_value_current("store.distributed_context")
            .unwrap(),
        distributed_context_value(
            Value::tag("Current"),
            authenticated_principal_value("origin-a", &["viewer"]),
            number(44),
            number(22),
            number(55),
        )
    );
}

#[test]
fn distributed_context_replacement_resets_omitted_bindings_and_the_revision_namespace() {
    let fixture = atomic_distributed_context_fixture();
    let mut session = MachineInstance::new(fixture.plan, SessionOptions::default()).unwrap();
    session
        .replace_distributed_context(
            SessionContext::Available {
                status: SessionConnectionStatus::Current,
                principal: SessionPrincipal::authenticated("origin-a", ["viewer"]).unwrap(),
            },
            vec![
                DistributedImportUpdate::new(fixture.first_import_id, 5, number(11)),
                DistributedImportUpdate::new(fixture.second_import_id, 5, number(22)),
            ],
        )
        .unwrap()
        .expect("origin A must install a complete context");
    update_atomic_call_result(&mut session, fixture.call_site_id, 5, number(33));

    let turn = session
        .replace_distributed_context(
            SessionContext::Available {
                status: SessionConnectionStatus::Current,
                principal: SessionPrincipal::authenticated("origin-b", ["operator"]).unwrap(),
            },
            vec![DistributedImportUpdate::new(
                fixture.second_import_id,
                1,
                number(222),
            )],
        )
        .unwrap()
        .expect("origin B revision one must not be compared with origin A revision five");

    assert_eq!(turn.metrics.recomputed_field_count, 1);
    assert_eq!(
        turn.deltas
            .iter()
            .filter(|delta| {
                matches!(
                    delta,
                    Delta::SetDistributedImport { .. } | Delta::ClearDistributedImport { .. }
                )
            })
            .count(),
        2
    );
    assert!(
        session
            .distributed_import_value_current(fixture.first_import_id)
            .is_err()
    );
    assert_eq!(
        session.distributed_import_revision(fixture.first_import_id),
        None
    );
    assert_eq!(
        session.distributed_import_revision(fixture.second_import_id),
        Some(1)
    );
    assert_eq!(
        session.distributed_import_revision(fixture.call_result_import_id),
        None
    );
    assert!(
        session
            .root_value_current("store.distributed_context")
            .is_err()
    );
}

#[test]
fn distributed_execution_context_does_not_consume_an_authority_turn_sequence() {
    let fixture = atomic_distributed_context_fixture();
    let mut session = MachineInstance::new(fixture.plan, SessionOptions::default()).unwrap();

    let execution = session
        .replace_distributed_execution_context(
            SessionContext::Available {
                status: SessionConnectionStatus::Current,
                principal: SessionPrincipal::authenticated("origin-a", ["viewer"]).unwrap(),
            },
            vec![DistributedImportUpdate::new(
                fixture.first_import_id,
                1,
                number(11),
            )],
        )
        .unwrap()
        .expect("execution context must become current");
    assert_eq!(execution.sequence, 0);
    session.settle_turn();

    let authority = session
        .update_session_context(SessionConnectionStatus::Stale, SessionPrincipal::Anonymous)
        .unwrap()
        .expect("authority context patch must emit a turn");
    assert_eq!(authority.sequence, 1);
}

#[test]
fn unavailable_distributed_context_clears_all_imports_and_session_info() {
    let fixture = atomic_distributed_context_fixture();
    let mut session = MachineInstance::new(fixture.plan, SessionOptions::default()).unwrap();
    session
        .replace_distributed_context(
            SessionContext::Available {
                status: SessionConnectionStatus::Current,
                principal: SessionPrincipal::authenticated("origin-a", ["viewer"]).unwrap(),
            },
            vec![
                DistributedImportUpdate::new(fixture.first_import_id, 5, number(11)),
                DistributedImportUpdate::new(fixture.second_import_id, 5, number(22)),
            ],
        )
        .unwrap()
        .expect("origin A must install a complete context");
    update_atomic_call_result(&mut session, fixture.call_site_id, 5, number(33));

    let turn = session
        .replace_distributed_context(SessionContext::Unavailable, Vec::new())
        .unwrap()
        .expect("the global context must clear origin A");
    assert_eq!(turn.metrics.recomputed_field_count, 1);
    assert!(
        session
            .root_value_current("store.distributed_context")
            .is_err()
    );
    for import_id in [fixture.first_import_id, fixture.second_import_id] {
        assert!(session.distributed_import_value_current(import_id).is_err());
        assert_eq!(session.distributed_import_revision(import_id), None);
    }
}

#[test]
fn distributed_context_replacement_rejects_a_batch_without_exposing_its_valid_prefix() {
    let fixture = atomic_distributed_context_fixture();
    let mut session = MachineInstance::new(fixture.plan, SessionOptions::default()).unwrap();
    let initial = distributed_context_value(
        Value::tag("Current"),
        authenticated_principal_value("origin-a", &["viewer"]),
        number(11),
        number(22),
        number(33),
    );
    session
        .replace_distributed_context(
            SessionContext::Available {
                status: SessionConnectionStatus::Current,
                principal: SessionPrincipal::authenticated("origin-a", ["viewer"]).unwrap(),
            },
            vec![
                DistributedImportUpdate::new(fixture.first_import_id, 5, number(11)),
                DistributedImportUpdate::new(fixture.second_import_id, 5, number(22)),
            ],
        )
        .unwrap()
        .expect("origin A must install a complete context");
    update_atomic_call_result(&mut session, fixture.call_site_id, 5, number(33));

    assert!(matches!(
        session.replace_distributed_context(
            SessionContext::Available {
                status: SessionConnectionStatus::Stale,
                principal: SessionPrincipal::authenticated("origin-b", ["operator"]).unwrap(),
            },
            vec![
                DistributedImportUpdate::new(fixture.first_import_id, 1, number(11)),
                DistributedImportUpdate::new(fixture.undeclared_import_id, 1, number(99)),
            ],
        ),
        Err(Error::InvalidEvent(detail)) if detail.contains("not declared")
    ));
    assert_eq!(
        session.distributed_import_revision(fixture.first_import_id),
        Some(5)
    );
    assert_eq!(
        session.distributed_import_revision(fixture.second_import_id),
        Some(5)
    );
    assert_eq!(
        session.distributed_import_revision(fixture.call_result_import_id),
        None
    );
    assert_eq!(
        session
            .root_value_current("store.distributed_context")
            .unwrap(),
        initial
    );
}

#[test]
fn distributed_context_replacement_rolls_back_context_values_and_revisions_together() {
    let fixture = atomic_distributed_context_fixture();
    let initial = distributed_context_value(
        Value::tag("Current"),
        authenticated_principal_value("origin-a", &["viewer"]),
        number(11),
        number(22),
        number(33),
    );
    let mut session = MachineInstance::new(fixture.plan, SessionOptions::default()).unwrap();

    session
        .replace_distributed_context(
            SessionContext::Available {
                status: SessionConnectionStatus::Current,
                principal: SessionPrincipal::authenticated("origin-a", ["viewer"]).unwrap(),
            },
            vec![
                DistributedImportUpdate::new(fixture.first_import_id, 5, number(11)),
                DistributedImportUpdate::new(fixture.second_import_id, 5, number(22)),
            ],
        )
        .unwrap()
        .expect("origin A must install a complete context");
    update_atomic_call_result(&mut session, fixture.call_site_id, 5, number(33));
    session.settle_turn();

    session
        .replace_distributed_context(
            SessionContext::Available {
                status: SessionConnectionStatus::Stale,
                principal: SessionPrincipal::Anonymous,
            },
            vec![DistributedImportUpdate::new(
                fixture.second_import_id,
                1,
                number(222),
            )],
        )
        .unwrap()
        .expect("origin B must replace the complete context");
    session.rollback_unsettled_turn().unwrap();

    assert_eq!(
        session.distributed_import_revision(fixture.first_import_id),
        Some(5)
    );
    assert_eq!(
        session.distributed_import_revision(fixture.second_import_id),
        Some(5)
    );
    assert_eq!(
        session.distributed_import_revision(fixture.call_result_import_id),
        None
    );
    assert_eq!(
        session
            .root_value_current("store.distributed_context")
            .unwrap(),
        initial
    );
}

#[test]
fn distributed_import_updates_are_current_monotonic_and_idempotent() {
    let fixture = distributed_session_fixture();
    let mut session = MachineInstance::new(fixture.plan, SessionOptions::default()).unwrap();

    assert_eq!(session.distributed_import_revision(fixture.import_id), None);
    assert!(session.root_value_current("store.remote_count").is_err());
    assert!(
        session
            .distributed_export_value_current(fixture.value_export_id)
            .is_err()
    );

    assert!(matches!(
        session.update_distributed_import(fixture.import_id, 0, number(40)),
        Err(Error::InvalidEvent(detail)) if detail.contains("must be positive")
    ));
    assert!(matches!(
        session.update_distributed_import(fixture.undeclared_import_id, 1, number(40)),
        Err(Error::InvalidEvent(detail)) if detail.contains("not declared")
    ));
    assert!(matches!(
        session.update_distributed_import(
            fixture.import_id,
            1,
            Value::Text("not a number".to_owned()),
        ),
        Err(Error::Evaluation(detail)) if detail.contains("declared data type")
    ));
    assert_eq!(session.distributed_import_revision(fixture.import_id), None);

    let first = session
        .update_distributed_import(fixture.import_id, 1, number(41))
        .unwrap()
        .expect("a newer import revision must produce an internal turn");
    assert_eq!(first.source_sequence, None);
    assert_eq!(first.metrics.recomputed_field_count, 1);
    assert_eq!(
        first.metrics.recomputed_targets,
        vec![ValueTarget::Field(FieldId(0))]
    );
    assert!(first.deltas.iter().any(|delta| matches!(
        delta,
        Delta::SetValue {
            target: ValueTarget::Field(FieldId(0)),
            value,
        } if value == &number(41)
    )));
    assert_eq!(
        session.root_value_current("store.remote_count").unwrap(),
        number(41)
    );
    assert_eq!(
        session
            .distributed_export_value_current(fixture.value_export_id)
            .unwrap(),
        number(41)
    );
    assert_eq!(
        session.distributed_import_revision(fixture.import_id),
        Some(1)
    );

    assert_eq!(
        session
            .update_distributed_import(fixture.import_id, 1, number(41))
            .unwrap(),
        None
    );
    assert!(matches!(
        session.update_distributed_import(fixture.import_id, 1, number(42)),
        Err(Error::InvalidEvent(detail)) if detail.contains("conflicts")
    ));

    let second = session
        .update_distributed_import(fixture.import_id, 2, number(42))
        .unwrap()
        .expect("a second newer revision must produce an internal turn");
    assert_eq!(second.metrics.recomputed_field_count, 1);
    assert_eq!(
        session.root_value_current("store.remote_count").unwrap(),
        number(42)
    );
    assert!(matches!(
        session.update_distributed_import(fixture.import_id, 1, number(41)),
        Err(Error::InvalidEvent(detail)) if detail.contains("stale")
    ));
    assert_eq!(
        session.distributed_import_revision(fixture.import_id),
        Some(2)
    );
    assert_eq!(
        session.root_value_current("store.remote_count").unwrap(),
        number(42)
    );
}

#[test]
fn distributed_function_instances_use_graph_currentness_and_fail_closed() {
    let fixture = distributed_session_fixture();
    let mut session = MachineInstance::new(fixture.plan, SessionOptions::default()).unwrap();
    let call_instance_id = DistributedCallInstanceId::from_rows(fixture.call_site_id, &[]).unwrap();

    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                fixture.call_site_id,
                call_instance_id,
                fixture.function_export_id,
                1,
                BTreeMap::from([(fixture.function_argument_id, number(7))]),
            )
            .unwrap(),
        number(14)
    );
    assert_eq!(
        session.distributed_import_revision(fixture.producer_argument_import),
        Some(1)
    );
    assert!(
        !session
            .recovery_image()
            .unwrap()
            .distributed_imports
            .contains_key(&fixture.producer_argument_import),
        "producer call arguments are transient graph inputs, not resumable endpoint state"
    );

    assert!(matches!(
        session.evaluate_distributed_function_instance(
            fixture.call_site_id,
            call_instance_id,
            fixture.function_export_id,
            2,
            BTreeMap::from([(
                fixture.function_argument_id,
                Value::Text("wrong type".to_owned()),
            )]),
        ),
        Err(Error::Evaluation(detail)) if detail.contains("declared data type")
    ));

    let wrong_argument_id =
        DistributedArgumentId::from_parameter_name(fixture.function_export_id, "other").unwrap();
    assert!(matches!(
        session.evaluate_distributed_function_instance(
            fixture.call_site_id,
            call_instance_id,
            fixture.function_export_id,
            2,
            BTreeMap::from([(wrong_argument_id, number(7))]),
        ),
        Err(Error::InvalidEvent(detail)) if detail.contains("missing argument `value`")
    ));
    assert!(matches!(
        session.evaluate_distributed_function_instance(
            fixture.call_site_id,
            call_instance_id,
            fixture.function_export_id,
            2,
            BTreeMap::new(),
        ),
        Err(Error::InvalidEvent(detail)) if detail.contains("expected 1")
    ));
    assert!(matches!(
        session.evaluate_distributed_function_instance(
            fixture.call_site_id,
            call_instance_id,
            fixture.undeclared_export_id,
            2,
            BTreeMap::from([(fixture.function_argument_id, number(7))]),
        ),
        Err(Error::InvalidEvent(detail)) if detail.contains("targets")
    ));
    let undeclared_call_site_id = RemoteCallSiteId([91; 32]);
    let undeclared_call_instance_id =
        DistributedCallInstanceId::from_rows(undeclared_call_site_id, &[]).unwrap();
    let undeclared_error = session
        .evaluate_distributed_function_instance(
            undeclared_call_site_id,
            undeclared_call_instance_id,
            fixture.function_export_id,
            2,
            BTreeMap::from([(fixture.function_argument_id, number(7))]),
        )
        .unwrap_err();
    let Error::InvalidEvent(detail) = undeclared_error else {
        panic!("unexpected undeclared-call error kind: {undeclared_error}");
    };
    assert!(detail.contains("not declared"));
    assert!(
        !detail.contains(&"5b".repeat(32)),
        "call-site identity leaked"
    );
    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                fixture.call_site_id,
                call_instance_id,
                fixture.function_export_id,
                2,
                BTreeMap::from([(fixture.function_argument_id, number(8))]),
            )
            .unwrap(),
        number(16)
    );
    assert!(matches!(
        session.evaluate_distributed_function_instance(
            fixture.call_site_id,
            call_instance_id,
            fixture.function_export_id,
            1,
            BTreeMap::from([(fixture.function_argument_id, number(7))]),
        ),
        Err(Error::InvalidEvent(detail)) if detail.contains("stale")
    ));
    let (unsettled, turn) = session
        .evaluate_distributed_function_instance_unsettled(
            fixture.call_site_id,
            call_instance_id,
            fixture.function_export_id,
            3,
            BTreeMap::from([(fixture.function_argument_id, number(9))]),
        )
        .unwrap();
    assert_eq!(unsettled, number(18));
    assert!(turn.is_some());
    session.rollback_unsettled_turn().unwrap();
    assert_eq!(
        session.distributed_import_revision(fixture.producer_argument_import),
        Some(2)
    );
    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                fixture.call_site_id,
                call_instance_id,
                fixture.function_export_id,
                3,
                BTreeMap::from([(fixture.function_argument_id, number(10))]),
            )
            .unwrap(),
        number(20),
        "rolling back a call must not retain its graph result cache"
    );
    assert!(matches!(
        session.distributed_export_value_current(fixture.function_export_id),
        Err(Error::InvalidEvent(detail)) if detail.contains("not declared")
    ));
}

#[test]
fn distributed_function_leases_isolate_origins_and_generations() {
    let fixture = distributed_session_fixture();
    let mut session = MachineInstance::new(fixture.plan, SessionOptions::default()).unwrap();
    let call_instance_id = DistributedCallInstanceId::from_rows(fixture.call_site_id, &[]).unwrap();
    let first_origin = MachineOrigin::new(7, 1).unwrap();
    let second_origin = MachineOrigin::new(8, 1).unwrap();

    session.set_machine_origin(first_origin).unwrap();
    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                fixture.call_site_id,
                call_instance_id,
                fixture.function_export_id,
                1,
                BTreeMap::from([(fixture.function_argument_id, number(4))]),
            )
            .unwrap(),
        number(8)
    );
    assert_eq!(
        session.distributed_import_revision(fixture.producer_argument_import),
        Some(1)
    );

    session.set_machine_origin(second_origin).unwrap();
    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                fixture.call_site_id,
                call_instance_id,
                fixture.function_export_id,
                1,
                BTreeMap::from([(fixture.function_argument_id, number(9))]),
            )
            .unwrap(),
        number(18),
        "a different origin must not inherit the first origin's argument revision or cache"
    );

    session.set_machine_origin(first_origin).unwrap();
    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                fixture.call_site_id,
                call_instance_id,
                fixture.function_export_id,
                2,
                BTreeMap::from([(fixture.function_argument_id, number(5))]),
            )
            .unwrap(),
        number(10)
    );
    assert_eq!(
        session.distributed_import_revision(fixture.producer_argument_import),
        Some(2)
    );

    assert!(
        session
            .drop_producer_origin(first_origin)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        session.distributed_import_revision(fixture.producer_argument_import),
        None
    );
    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                fixture.call_site_id,
                call_instance_id,
                fixture.function_export_id,
                1,
                BTreeMap::from([(fixture.function_argument_id, number(6))]),
            )
            .unwrap(),
        number(12),
        "an expired generation must restart from an empty producer lease"
    );
}

#[test]
fn distributed_function_leases_isolate_call_sites_within_one_origin() {
    let fixture = distributed_session_fixture();
    let mut session = MachineInstance::new(fixture.plan, SessionOptions::default()).unwrap();
    let call_instance_id = DistributedCallInstanceId::from_rows(fixture.call_site_id, &[]).unwrap();
    let second_call_instance_id =
        DistributedCallInstanceId::from_rows(fixture.second_call_site_id, &[]).unwrap();
    session
        .set_machine_origin(MachineOrigin::new(11, 4).unwrap())
        .unwrap();

    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                fixture.call_site_id,
                call_instance_id,
                fixture.function_export_id,
                1,
                BTreeMap::from([(fixture.function_argument_id, number(3))]),
            )
            .unwrap(),
        number(6)
    );
    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                fixture.second_call_site_id,
                second_call_instance_id,
                fixture.function_export_id,
                1,
                BTreeMap::from([(fixture.function_argument_id, number(8))]),
            )
            .unwrap(),
        number(16)
    );
    assert_eq!(
        session.distributed_import_revision(fixture.producer_argument_import),
        Some(1)
    );
    assert_eq!(
        session.distributed_import_revision(fixture.second_producer_argument_import),
        Some(1)
    );

    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                fixture.call_site_id,
                call_instance_id,
                fixture.function_export_id,
                2,
                BTreeMap::from([(fixture.function_argument_id, number(4))]),
            )
            .unwrap(),
        number(8)
    );
    assert_eq!(
        session.distributed_import_revision(fixture.second_producer_argument_import),
        Some(1),
        "advancing one call site must not advance another call site's lease"
    );
}

#[test]
fn row_owned_distributed_calls_are_demand_traced_and_instance_scoped() {
    let program = |role, path: &str, source: &str| boon_compiler::DistributedCompilerProgram {
        revision: 1,
        role,
        source_label: path.to_owned(),
        units: vec![boon_compiler::CompilerSourceUnit {
            path: path.to_owned(),
            source: source.to_owned(),
        }],
        application: ApplicationIdentity::new(
            "dev.boon.plan-executor-row-calls",
            format!("{}-state", role.as_str()),
            "local",
        ),
        schema_version: 1,
        migration_predecessors: Vec::new(),
    };
    let compiled = boon_compiler::compile_distributed_runtime_source_programs(
        &[
            program(
                ProgramRole::Client,
                "Client/RUN.bn",
                r#"
store: [
    hide: SOURCE
    show: SOURCE
    visible:
        True |> HOLD visible {
            LATEST {
                hide |> THEN { False }
                show |> THEN { True }
            }
        }
    items: LIST { [value: 1], [value: 2] }
    rows:
        items
        |> List/filter(item, if: visible)
        |> List/map(item, new: [result: Session/add(value: item.value)])
]

document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: TEXT { Distributed row calls }
    )
)
"#,
            ),
            program(
                ProgramRole::Session,
                "Session/RUN.bn",
                r#"
store: [ready: True]

FUNCTION add(value) {
    value |> HOLD remembered { LATEST {} }
}
"#,
            ),
            program(
                ProgramRole::Server,
                "Server/RUN.bn",
                "store: [ready: True]\n",
            ),
        ],
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let client_endpoint = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Client)
        .expect("client endpoint");
    let [call] = client_endpoint.remote_call_sites.as_slice() else {
        panic!("expected one row-owned call site")
    };
    assert_eq!(call.row_bindings.len(), 1);
    let [argument] = call.arguments.as_slice() else {
        panic!("expected one call argument")
    };
    let client_plan = compiled.program(ProgramRole::Client).unwrap().plan.clone();
    let rows_list = client_plan
        .debug_map
        .list_slots
        .iter()
        .find(|entry| entry.label == "store.rows")
        .and_then(|entry| entry.id.strip_prefix("list:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(ListId)
        .expect("store.rows list ID");
    let result_field = client_plan
        .debug_map
        .fields
        .iter()
        .find(|entry| entry.label == "store.rows.result")
        .and_then(|entry| entry.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .expect("store.rows.result field ID");
    let hide = source_id(&client_plan, "store.hide");
    let mut client = MachineInstance::new(client_plan, SessionOptions::default()).unwrap();

    let initial_rows = client.list_row_snapshots_current(rows_list).unwrap();
    assert_eq!(initial_rows.len(), 2);
    let instances = client
        .distributed_call_instances_current(call.call_site_id)
        .unwrap();
    assert_eq!(instances.len(), 2);
    let first = instances
        .iter()
        .find(|instance| instance.arguments.get(&argument.argument_id) == Some(&number(1)))
        .expect("first row call instance");
    let second = instances
        .iter()
        .find(|instance| instance.arguments.get(&argument.argument_id) == Some(&number(2)))
        .expect("second row call instance");
    assert_ne!(first.call_instance_id, second.call_instance_id);

    client
        .update_distributed_call_result(call.call_site_id, first.call_instance_id, 1, number(11))
        .unwrap()
        .expect("first result update");
    let current_rows = client.list_row_snapshots_current(rows_list).unwrap();
    assert_eq!(current_rows[0].fields.get(&result_field), Some(&number(11)));
    assert_eq!(current_rows[1].fields.get(&result_field), None);

    client
        .apply(SourceEvent {
            sequence: 1,
            source: hide,
            route: route_token(&client, hide, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert!(
        client
            .list_row_snapshots_current(rows_list)
            .unwrap()
            .is_empty()
    );
    assert!(
        client
            .distributed_call_instances_current(call.call_site_id)
            .unwrap()
            .is_empty(),
        "inactive rows must not retain remote call demand"
    );

    let mut producer = MachineInstance::new(
        compiled.program(ProgramRole::Session).unwrap().plan.clone(),
        SessionOptions::default(),
    )
    .unwrap();
    assert_eq!(
        producer
            .evaluate_distributed_function_instance(
                call.call_site_id,
                first.call_instance_id,
                call.function_export_id,
                1,
                BTreeMap::from([(argument.argument_id, number(1))]),
            )
            .unwrap(),
        number(1)
    );
    assert_eq!(
        producer
            .evaluate_distributed_function_instance(
                call.call_site_id,
                second.call_instance_id,
                call.function_export_id,
                1,
                BTreeMap::from([(argument.argument_id, number(2))]),
            )
            .unwrap(),
        number(2)
    );
    assert_eq!(
        producer
            .evaluate_distributed_function_instance(
                call.call_site_id,
                first.call_instance_id,
                call.function_export_id,
                2,
                BTreeMap::from([(argument.argument_id, number(9))]),
            )
            .unwrap(),
        number(1),
        "one instance must retain its own HOLD state"
    );
    assert_eq!(
        producer
            .evaluate_distributed_function_instance(
                call.call_site_id,
                second.call_instance_id,
                call.function_export_id,
                2,
                BTreeMap::from([(argument.argument_id, number(8))]),
            )
            .unwrap(),
        number(2),
        "another instance must retain a different HOLD state"
    );
    producer
        .drop_producer_call_instance(call.call_site_id, first.call_instance_id)
        .unwrap()
        .expect("first producer lease detach");
    assert_eq!(
        producer
            .evaluate_distributed_function_instance(
                call.call_site_id,
                first.call_instance_id,
                call.function_export_id,
                1,
                BTreeMap::from([(argument.argument_id, number(9))]),
            )
            .unwrap(),
        number(9),
        "a detached instance must start with fresh function state"
    );
}

#[test]
fn nested_current_calls_inherit_their_parent_instance_and_update_only_that_lease() {
    let program = |role, path: &str, source: &str| boon_compiler::DistributedCompilerProgram {
        revision: 1,
        role,
        source_label: path.to_owned(),
        units: vec![boon_compiler::CompilerSourceUnit {
            path: path.to_owned(),
            source: source.to_owned(),
        }],
        application: ApplicationIdentity::new(
            "dev.boon.plan-executor-nested-calls",
            format!("{}-state", role.as_str()),
            "local",
        ),
        schema_version: 1,
        migration_predecessors: Vec::new(),
    };
    let compiled = boon_compiler::compile_distributed_runtime_source_programs(
        &[
            program(
                ProgramRole::Client,
                "Client/RUN.bn",
                r#"
store: [result: Session/outer(value: 3)]

document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: TEXT { Nested calls }
    )
)
"#,
            ),
            program(
                ProgramRole::Session,
                "Session/RUN.bn",
                r#"
store: [ready: True]

FUNCTION outer(value) {
    Server/double(value: value)
}
"#,
            ),
            program(
                ProgramRole::Server,
                "Server/RUN.bn",
                r#"
store: [ready: True]

FUNCTION double(value) {
    value * 2
}
"#,
            ),
        ],
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let outer_call = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Client)
        .and_then(|endpoint| endpoint.remote_call_sites.first())
        .cloned()
        .expect("Client to Session call");
    let nested_call = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Session)
        .and_then(|endpoint| endpoint.remote_call_sites.first())
        .cloned()
        .expect("Session to Server call");
    assert_eq!(outer_call.callee_role, ProgramRole::Session);
    assert_eq!(nested_call.callee_role, ProgramRole::Server);
    let outer_argument = outer_call.arguments[0].argument_id;
    let nested_argument = nested_call.arguments[0].argument_id;
    let first_outer = DistributedCallInstanceId([0x21; 32]);
    let second_outer = DistributedCallInstanceId([0x22; 32]);
    let mut session = MachineInstance::new(
        compiled.program(ProgramRole::Session).unwrap().plan.clone(),
        SessionOptions::default(),
    )
    .unwrap();

    for (instance, value) in [(first_outer, 3), (second_outer, 4)] {
        let (initial, turn) = session
            .activate_distributed_current_function_instance_unsettled(
                outer_call.call_site_id,
                instance,
                outer_call.function_export_id,
                1,
                BTreeMap::from([(outer_argument, number(value))]),
            )
            .unwrap();
        assert_eq!(initial, None);
        assert!(
            turn.is_some(),
            "pending Current activation must retain its demand"
        );
        session.settle_turn();
    }

    let nested = session
        .distributed_call_instances_current(nested_call.call_site_id)
        .unwrap();
    assert_eq!(nested.len(), 2);
    let first_nested = nested
        .iter()
        .find(|instance| instance.arguments.get(&nested_argument) == Some(&number(3)))
        .expect("first nested demand");
    let second_nested = nested
        .iter()
        .find(|instance| instance.arguments.get(&nested_argument) == Some(&number(4)))
        .expect("second nested demand");
    assert_eq!(
        first_nested.call_instance_id,
        DistributedCallInstanceId::from_context(nested_call.call_site_id, Some(first_outer), &[],)
            .unwrap()
    );
    assert_eq!(
        second_nested.call_instance_id,
        DistributedCallInstanceId::from_context(nested_call.call_site_id, Some(second_outer), &[],)
            .unwrap()
    );
    assert_ne!(
        first_nested.call_instance_id,
        second_nested.call_instance_id
    );

    session
        .update_distributed_call_result(
            nested_call.call_site_id,
            first_nested.call_instance_id,
            1,
            number(6),
        )
        .unwrap()
        .expect("first nested result turn");
    assert_eq!(
        session
            .distributed_producer_call_result_current(outer_call.call_site_id, first_outer)
            .unwrap(),
        number(6)
    );
    assert!(
        session
            .distributed_producer_call_result_current(outer_call.call_site_id, second_outer)
            .is_err()
    );

    session
        .update_distributed_call_result(
            nested_call.call_site_id,
            second_nested.call_instance_id,
            1,
            number(8),
        )
        .unwrap()
        .expect("second nested result turn");
    assert_eq!(
        session
            .distributed_producer_call_result_current(outer_call.call_site_id, second_outer)
            .unwrap(),
        number(8)
    );

    session
        .drop_producer_call_instance(outer_call.call_site_id, first_outer)
        .unwrap()
        .expect("first outer lease detach");
    let remaining = session
        .distributed_call_instances_current(nested_call.call_site_id)
        .unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        remaining[0].call_instance_id,
        second_nested.call_instance_id
    );
}

#[test]
fn hold_backed_distributed_function_state_is_lease_local_and_generation_scoped() {
    let program = |role, path: &str, source: &str| boon_compiler::DistributedCompilerProgram {
        revision: 1,
        role,
        source_label: path.to_owned(),
        units: vec![boon_compiler::CompilerSourceUnit {
            path: path.to_owned(),
            source: source.to_owned(),
        }],
        application: ApplicationIdentity::new(
            "dev.boon.plan-executor-stateful-producer",
            format!("{}-state", role.as_str()),
            "local",
        ),
        schema_version: 1,
        migration_predecessors: Vec::new(),
    };
    let compiled = boon_compiler::compile_distributed_runtime_source_programs(
        &[
            program(
                ProgramRole::Client,
                "Client/RUN.bn",
                r#"
store: [
    remembered: Session/remember(value: 5)
    constant: Session/constant()
]

document: Document/new(
    root: Element/label(
        element: []
        style: []
        label: TEXT { Stateful producer }
    )
)
"#,
            ),
            program(
                ProgramRole::Session,
                "Session/RUN.bn",
                r#"
store: [ready: True]

FUNCTION remember(value) {
    value |> HOLD current { LATEST {} }
}

FUNCTION constant() {
    42
}
"#,
            ),
            program(
                ProgramRole::Server,
                "Server/RUN.bn",
                "store: [ready: True]\n",
            ),
        ],
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let client_endpoint = compiled
        .graph
        .endpoints
        .iter()
        .find(|endpoint| endpoint.role == ProgramRole::Client)
        .unwrap();
    let call = client_endpoint
        .remote_call_sites
        .iter()
        .find(|call| call.arguments.len() == 1)
        .expect("remember call");
    let constant_call = client_endpoint
        .remote_call_sites
        .iter()
        .find(|call| call.arguments.is_empty())
        .expect("constant call");
    let call_instance_id = DistributedCallInstanceId::from_rows(call.call_site_id, &[]).unwrap();
    let constant_call_instance_id =
        DistributedCallInstanceId::from_rows(constant_call.call_site_id, &[]).unwrap();
    let [argument] = call.arguments.as_slice() else {
        panic!("expected one remote argument")
    };
    let session_plan = compiled.program(ProgramRole::Session).unwrap().plan.clone();
    let producer_states = session_plan
        .producer_function_instances
        .iter()
        .find(|instance| instance.call_site_id == call.call_site_id)
        .expect("remember producer instance")
        .ownership
        .states
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    assert!(!producer_states.is_empty());
    for slot in session_plan
        .storage_layout
        .scalar_slots
        .iter()
        .filter(|slot| producer_states.contains(&slot.state_id))
    {
        assert!(
            session_plan
                .persistence
                .memory
                .iter()
                .all(|memory| memory.runtime_slot != slot.id),
            "producer lease HOLD authority must not enter durable global memory"
        );
    }
    let mut session = MachineInstance::new(session_plan, SessionOptions::default()).unwrap();
    session.recovery_image().unwrap();
    let first_origin = MachineOrigin::new(20, 3).unwrap();
    let second_origin = MachineOrigin::new(21, 1).unwrap();
    let rolled_back_origin = MachineOrigin::new(22, 1).unwrap();

    session.set_machine_origin(first_origin).unwrap();
    let (constant, constant_turn) = session
        .evaluate_distributed_function_instance_unsettled(
            constant_call.call_site_id,
            constant_call_instance_id,
            constant_call.function_export_id,
            1,
            BTreeMap::new(),
        )
        .unwrap();
    assert_eq!(constant, number(42));
    assert!(
        constant_turn.is_some(),
        "a zero-argument producer call must remain prepared until publication commits"
    );
    session.rollback_unsettled_turn().unwrap();
    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                constant_call.call_site_id,
                constant_call_instance_id,
                constant_call.function_export_id,
                1,
                BTreeMap::new(),
            )
            .unwrap(),
        number(42)
    );
    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                call.call_site_id,
                call_instance_id,
                call.function_export_id,
                1,
                BTreeMap::from([(argument.argument_id, number(7))]),
            )
            .unwrap(),
        number(7)
    );
    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                call.call_site_id,
                call_instance_id,
                call.function_export_id,
                2,
                BTreeMap::from([(argument.argument_id, number(8))]),
            )
            .unwrap(),
        number(7),
        "HOLD authority must persist within one origin/call-site lease"
    );

    session.set_machine_origin(second_origin).unwrap();
    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                call.call_site_id,
                call_instance_id,
                call.function_export_id,
                1,
                BTreeMap::from([(argument.argument_id, number(9))]),
            )
            .unwrap(),
        number(9),
        "another origin must initialize independent HOLD authority"
    );

    session.set_machine_origin(rolled_back_origin).unwrap();
    let (rolled_back_value, rolled_back_turn) = session
        .evaluate_distributed_function_instance_unsettled(
            call.call_site_id,
            call_instance_id,
            call.function_export_id,
            1,
            BTreeMap::from([(argument.argument_id, number(11))]),
        )
        .unwrap();
    assert_eq!(rolled_back_value, number(11));
    assert!(rolled_back_turn.is_some());
    session.rollback_unsettled_turn().unwrap();
    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                call.call_site_id,
                call_instance_id,
                call.function_export_id,
                1,
                BTreeMap::from([(argument.argument_id, number(12))]),
            )
            .unwrap(),
        number(12),
        "rolling back the first call must discard its newly initialized HOLD lease"
    );

    session.set_machine_origin(first_origin).unwrap();
    session.drop_producer_origin(first_origin).unwrap();
    assert_eq!(
        session
            .evaluate_distributed_function_instance(
                call.call_site_id,
                call_instance_id,
                call.function_export_id,
                1,
                BTreeMap::from([(argument.argument_id, number(10))]),
            )
            .unwrap(),
        number(10),
        "expired generation authority must not survive lease removal"
    );
    session.recovery_image().unwrap();
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
