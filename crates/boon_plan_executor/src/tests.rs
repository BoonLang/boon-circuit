use super::*;
use boon_plan::*;
use std::collections::{BTreeMap, BTreeSet};

fn compile_server_source(
    source_label: &str,
    source_text: &str,
    target_profile: TargetProfile,
) -> boon_compiler::CompilerResult<boon_compiler::CompiledMachinePlanFromSource> {
    boon_compiler::compile_source_text_to_machine_plan_for_role(
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
    boon_compiler::compile_source_path_to_machine_plan_for_role(
        source_path,
        target_profile,
        ProgramRole::Server,
    )
}

fn number(value: i64) -> Value {
    Value::integer(value).unwrap()
}

fn stored_number(value: i64) -> boon_persistence::StoredValue {
    boon_persistence::StoredValue::integer(value).unwrap()
}

fn number_constant(value: i64) -> PlanConstantValue {
    PlanConstantValue::Number {
        value: FiniteReal::from_i64_exact(value).unwrap(),
    }
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
        PlanValueType::Bool => DataTypePlan::Bool,
        PlanValueType::Bytes { fixed_len } => DataTypePlan::Bytes { fixed_len },
        PlanValueType::Enum => DataTypePlan::Variant {
            variants: Vec::new(),
        },
        PlanValueType::Data => DataTypePlan::Unknown,
        PlanValueType::Unknown => DataTypePlan::Unknown,
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
        Value::Bool(true)
    );
    session.apply(event(&session, 1, 0, None)).unwrap();
    assert_eq!(
        session.root_value_current("store.same").unwrap(),
        Value::Bool(false)
    );
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
            initializer: initial(PlanConstantValue::Bool { value: selected }),
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
        Value::Bool(true)
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
                initializer: initial(PlanConstantValue::Bool { value: keep }),
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
            vec![constant(0, PlanConstantValue::Bool { value: false })],
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
    assert_eq!(snapshot.fields[&FieldId(23)], Value::Bool(false));
    assert_eq!(snapshot.fields[&FieldId(24)], Value::Bool(true));
    assert_eq!(
        snapshot.fields[&FieldId(25)],
        Value::Record(BTreeMap::from([
            ("$tag".to_owned(), Value::Text("Found".to_owned())),
            (
                "value".to_owned(),
                Value::Row {
                    id: snapshot.lists[&ListId(0)][1].id,
                    fields: BTreeMap::new(),
                },
            ),
        ]))
    );
    assert_eq!(
        snapshot.fields[&FieldId(26)],
        Value::Record(BTreeMap::from([(
            "$tag".to_owned(),
            Value::Text("NotFound".to_owned()),
        )]))
    );
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
                    initializer: initial(PlanConstantValue::Bool { value: false }),
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
        value_type: PlanValueType::Bool,
        scope_id: Some(ScopeId(0)),
        indexed: true,
        indexed_field_id: Some(FieldId(11)),
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
        vec![constant(0, PlanConstantValue::Bool { value: true })],
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
                initializer: initial(PlanConstantValue::Enum {
                    value: "Hexadecimal".into(),
                }),
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
        value_type: PlanValueType::Enum,
        scope_id: Some(ScopeId(0)),
        indexed: true,
        indexed_field_id: Some(FieldId(11)),
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
                    pattern: PlanRowSelectPattern::Text {
                        value: "Hexadecimal".to_owned(),
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
                    pattern: PlanRowSelectPattern::Bool { value: true },
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
                constant(
                    7,
                    PlanConstantValue::Enum {
                        value: "Binary".into(),
                    },
                ),
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
    assert_eq!(rows[0].fields[&FieldId(11)], Value::Text("Binary".into()));
    assert_eq!(
        rows[1].fields[&FieldId(11)],
        Value::Text("Hexadecimal".into())
    );
}

#[test]
fn list_find_uses_typed_index_without_scanning() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-indexed-find-runtime.bn",
        r#"
store: [
    choose: SOURCE
    selector:
        TEXT { a } |> HOLD selector {
            choose.text |> THEN { choose.text }
        }
    items: LIST {
        [key: TEXT { a }, value: TEXT { A }]
        [key: TEXT { b }, value: TEXT { B }]
    }
    selected:
        items
        |> List/find(item, if: item.key == selector)
        |> WHEN {
            Found[value] => value.value
            NotFound => TEXT { missing }
        }
]
document: Document/new(
    root: Element/label(element: [], label: store.selected)
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let source = source_id(&compiled.plan, "store.choose");
    let selected = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.selected")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .expect("store.selected field id");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: route_token(&session, source, None),
            target: None,
            payload: SourcePayload {
                text: Some("b".to_owned()),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    assert!(turn.metrics.indexed_access_count >= 1);
    assert_eq!(turn.metrics.list_find_scan_count, 0);
    assert_eq!(
        session
            .project_current(&[ValueTarget::Field(selected)])
            .unwrap()[&ValueTarget::Field(selected)],
        Value::Text("B".into())
    );
}

#[test]
fn dynamic_literal_row_defaults_remain_reactive_and_update_indexes_in_place() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "dynamic-literal-row-default.bn",
        r#"
store: [
    rename: SOURCE
    current_name:
        TEXT { Alpha } |> HOLD current_name {
            rename.text |> THEN { rename.text }
        }
    items: LIST {
        [id: TEXT { only }, name: current_name |> Text/to_uppercase()]
        [id: TEXT { other }, name: TEXT { OMEGA }]
    }
    selected:
        items
        |> List/find(item, if: item.name == (current_name |> Text/to_uppercase()))
        |> WHEN {
            Found[value] => value.name
            NotFound => TEXT { missing }
        }
]
document: Document/new(
    root: Element/label(element: [], label: store.selected)
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let rename = source_id(&compiled.plan, "store.rename");
    let items = list_id(&compiled.plan, "store.items");
    let item_name = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == items)
        .and_then(|slot| {
            slot.row_fields
                .iter()
                .find(|field| field.name == "name" && field.role.is_value())
        })
        .map(|field| field.field_id)
        .expect("items.name value field");
    let name_initializer = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == items)
        .and_then(|slot| slot.initial_rows.first())
        .and_then(|row| {
            row.fields
                .iter()
                .find(|field| field.field_id == Some(item_name))
        })
        .expect("items first-row name initializer");
    let name_expression = name_initializer
        .initializer
        .expression()
        .expect("dynamic row initializer expression");
    assert!(matches!(
        compiled.plan.row_expression(name_expression).unwrap(),
        PlanRowExpressionNode::BuiltinCall {
            function: PlanRowBuiltin::TextToUppercase,
            ..
        }
    ));
    let selected = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.selected")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .expect("store.selected field id");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    let before = session.snapshot().unwrap();
    let row = before.lists[&items][0].id;
    assert_eq!(
        before.lists[&items][0].fields[&item_name],
        Value::Text("ALPHA".to_owned())
    );
    assert_eq!(before.fields[&selected], Value::Text("ALPHA".to_owned()));

    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source: rename,
            route: route_token(&session, rename, None),
            target: None,
            payload: SourcePayload {
                text: Some("Beta".to_owned()),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    let after = session.snapshot().unwrap();
    assert_eq!(after.lists[&items][0].id, row);
    assert_eq!(
        after.lists[&items][0].fields[&item_name],
        Value::Text("BETA".to_owned())
    );
    assert_eq!(after.fields[&selected], Value::Text("BETA".to_owned()));
    assert_eq!(turn.metrics.ordered_index_full_rebuild_count, 0);
    assert_eq!(turn.metrics.ordered_index_incremental_row_count, 1);

    session
        .test_set_row_field(row, item_name, Value::Text("MANUAL".to_owned()))
        .unwrap();
    assert_eq!(
        session
            .project_current(&[ValueTarget::Field(selected)])
            .unwrap()[&ValueTarget::Field(selected)],
        Value::Text("missing".to_owned())
    );
    let override_turn = session
        .apply(SourceEvent {
            sequence: 2,
            source: rename,
            route: route_token(&session, rename, None),
            target: None,
            payload: SourcePayload {
                text: Some("Gamma".to_owned()),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    let overridden = session.snapshot().unwrap();
    assert_eq!(
        overridden.lists[&items][0].fields[&item_name],
        Value::Text("MANUAL".to_owned())
    );
    assert_eq!(
        overridden.fields[&selected],
        Value::Text("missing".to_owned())
    );
    assert_eq!(override_turn.metrics.ordered_index_incremental_row_count, 0);
}

#[test]
fn stored_list_find_result_keeps_typed_row_identity_without_scanning() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "stored-typed-indexed-find-runtime.bn",
        r#"
store: [
    choose: SOURCE
    selector:
        TEXT { a } |> HOLD selector {
            choose.text |> THEN { choose.text }
        }
    items: LIST {
        [key: TEXT { a }, value: TEXT { A }]
        [key: TEXT { b }, value: TEXT { B }]
    }
    found:
        items
        |> List/find(item, if: item.key == selector)
    selected:
        found |> WHEN {
            Found[value] => value.value
            NotFound => TEXT { missing }
        }
]
document: Document/new(
    root: Element/label(element: [], label: store.selected)
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let source = source_id(&compiled.plan, "store.choose");
    let selected = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.selected")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .expect("store.selected field id");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: route_token(&session, source, None),
            target: None,
            payload: SourcePayload {
                text: Some("b".to_owned()),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    assert!(turn.metrics.indexed_access_count >= 1);
    assert_eq!(turn.metrics.list_find_scan_count, 0);
    assert_eq!(
        session
            .project_current(&[ValueTarget::Field(selected)])
            .unwrap()[&ValueTarget::Field(selected)],
        Value::Text("B".into())
    );
}

#[test]
fn stored_list_get_and_latest_results_read_exact_row_fields() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "stored-list-row-results-runtime.bn",
        r#"
store: [
    items: LIST {
        [key: TEXT { a }, value: TEXT { A }]
        [key: TEXT { b }, value: TEXT { B }]
    }
    indexed_row: List/get(list: items, index: 1)
    indexed_value: indexed_row.value
    latest_row: List/latest(list: items)
    latest_value: latest_row.value
]
document: Document/new(
    root: Element/label(
        element: []
        label: store.indexed_value |> Text/concat(with: store.latest_value, separator: Text/empty())
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let field = |path: &str| {
        compiled
            .plan
            .debug_map
            .fields
            .iter()
            .find(|field| field.label == path)
            .and_then(|field| field.id.strip_prefix("field:"))
            .and_then(|id| id.parse::<usize>().ok())
            .map(FieldId)
            .unwrap_or_else(|| panic!("{path} field id"))
    };
    let indexed = field("store.indexed_value");
    let latest = field("store.latest_value");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    let values = session
        .project_current(&[ValueTarget::Field(indexed), ValueTarget::Field(latest)])
        .unwrap();

    assert_eq!(
        values[&ValueTarget::Field(indexed)],
        Value::Text("B".into())
    );
    assert_eq!(values[&ValueTarget::Field(latest)], Value::Text("B".into()));
}

#[test]
fn list_inspection_uses_exact_semantic_owner_instead_of_contextual_field_names() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "exact-list-inspection-runtime.bn",
        r#"
store: [
    rows: LIST {
        [value: 2]
        [value: 3]
    }
    doubled:
        rows
        |> List/map(item, new: [formatter: item.value * 2])
    tripled:
        rows
        |> List/map(item, new: [formatter: item.value * 3])
]

document: Document/new(
    root: Element/label(element: [], label: TEXT { exact owners })
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let exact_target = |semantic_path: &str| {
        let memory = compiled
            .plan
            .persistence
            .lists
            .iter()
            .find(|memory| {
                memory
                    .row_fields
                    .iter()
                    .any(|field| field.semantic_path == semantic_path)
            })
            .expect("list memory for inspected semantic path");
        let field = memory
            .row_fields
            .iter()
            .find(|field| field.semantic_path == semantic_path)
            .and_then(|field| field.runtime_field_id)
            .expect("runtime field for inspected semantic path");
        let list = compiled
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.id == memory.runtime_slot)
            .map(|slot| slot.list_id)
            .expect("runtime list for inspected semantic path");
        (list, field)
    };
    let doubled_target = exact_target("store.doubled.formatter");
    let tripled_target = exact_target("store.tripled.formatter");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let doubled = session
        .inspect_list_field_current(doubled_target.0, doubled_target.1, 8)
        .unwrap();
    let tripled = session
        .inspect_list_field_current(tripled_target.0, tripled_target.1, 8)
        .unwrap();
    let inspected_values = |value: Value| {
        let Value::List(rows) = value else {
            panic!("list field inspection must return a list");
        };
        rows.into_iter()
            .map(|row| {
                let Value::Record(row) = row else {
                    panic!("list field inspection row must be a record");
                };
                row.get("value").cloned().expect("inspected row value")
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        inspected_values(doubled),
        vec![Value::integer(4).unwrap(), Value::integer(6).unwrap()]
    );
    assert_eq!(
        inspected_values(tripled),
        vec![Value::integer(6).unwrap(), Value::integer(9).unwrap()]
    );
    assert!(session.inspect_value_current("item.formatter", 8).is_err());
}

#[test]
fn mapped_row_sibling_reads_its_indexed_state_after_materialization() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "mapped-row-indexed-state-sibling-runtime.bn",
        r#"
FUNCTION selected_row(row) {
    [
        id: row.id
        item_kind: row.item_kind
        formatter:
            Hexadecimal |> HOLD formatter {
                LATEST {
                    store.use_binary |> THEN { Binary }
                    store.reset |> THEN { Hexadecimal }
                }
            }
        format_label:
            row.id
            |> Text/concat(
                with:
                    formatter |> WHEN {
                        Hexadecimal => TEXT { Hex }
                        Binary => TEXT { Bin }
                        __ => TEXT { Other }
                    }
                separator: TEXT { : }
            )
    ]
}

FUNCTION visible_row(row) {
    row.item_kind |> WHEN {
        VariableRow => selected_row(row: row)
        __ => row
    }
}

store: [
    use_binary: SOURCE
    reset: SOURCE
    rows: LIST {
        [id: TEXT { one }, item_kind: VariableRow, formatter: Hexadecimal, format_label: TEXT { seed-one }]
        [id: TEXT { two }, item_kind: VariableRow, formatter: Hexadecimal, format_label: TEXT { seed-two }]
        [id: TEXT { group }, item_kind: GroupRow, formatter: Hexadecimal, format_label: TEXT { Group }]
    }
    selected:
        rows
        |> List/map(item, new: visible_row(row: item))
    visible:
        selected
        |> List/filter(item, if: item.id != TEXT { missing })
]

document: Document/new(
    root: Element/label(element: [], label: TEXT { mapped row state })
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let selected = compiled
        .plan
        .debug_map
        .list_slots
        .iter()
        .find(|slot| slot.label == "store.selected")
        .and_then(|slot| slot.id.strip_prefix("list:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(ListId)
        .expect("selected list id");
    let format_label = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == selected)
        .and_then(|slot| {
            slot.row_fields.iter().find(|field| {
                field.name == "format_label" && field.role == PlanListRowFieldRole::Value
            })
        })
        .map(|field| field.field_id)
        .expect("selected format_label field");
    let visible = compiled
        .plan
        .debug_map
        .list_slots
        .iter()
        .find(|slot| slot.label == "store.visible")
        .and_then(|slot| slot.id.strip_prefix("list:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(ListId)
        .expect("visible list id");
    let visible_format_label = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == visible)
        .and_then(|slot| {
            slot.row_fields.iter().find(|field| {
                field.name == "format_label" && field.role == PlanListRowFieldRole::Value
            })
        })
        .map(|field| field.field_id)
        .expect("visible format_label field");
    let use_binary = source_id(&compiled.plan, "store.use_binary");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    let inspected_values = |value: Value| {
        let Value::List(rows) = value else {
            panic!("list field inspection must return a list");
        };
        rows.into_iter()
            .map(|row| {
                let Value::Record(row) = row else {
                    panic!("list field inspection row must be a record");
                };
                row.get("value").cloned().expect("inspected row value")
            })
            .collect::<Vec<_>>()
    };
    let rows = session.list_rows_current(selected).unwrap();
    let targets = rows
        .iter()
        .copied()
        .map(|row| ValueTarget::RowField {
            row,
            field: format_label,
        })
        .collect::<Vec<_>>();
    session.ensure_current(&targets).unwrap();

    assert_eq!(
        inspected_values(
            session
                .inspect_list_field_current(selected, format_label, 8)
                .unwrap()
        ),
        vec![
            Value::Text("one:Hex".into()),
            Value::Text("two:Hex".into()),
            Value::Text("Group".into())
        ]
    );
    assert_eq!(
        inspected_values(
            session
                .inspect_list_field_current(visible, visible_format_label, 8)
                .unwrap()
        ),
        vec![
            Value::Text("one:Hex".into()),
            Value::Text("two:Hex".into()),
            Value::Text("Group".into())
        ]
    );

    session
        .apply(SourceEvent {
            sequence: 1,
            source: use_binary,
            route: route_token(&session, use_binary, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert_eq!(
        inspected_values(
            session
                .inspect_list_field_current(selected, format_label, 8)
                .unwrap()
        ),
        vec![
            Value::Text("one:Bin".into()),
            Value::Text("two:Bin".into()),
            Value::Text("Group".into())
        ]
    );
    assert_eq!(
        inspected_values(
            session
                .inspect_list_field_current(visible, visible_format_label, 8)
                .unwrap()
        ),
        vec![
            Value::Text("one:Bin".into()),
            Value::Text("two:Bin".into()),
            Value::Text("Group".into())
        ]
    );
}

#[test]
fn list_append_copies_a_typed_row_through_compiler_owned_field_ids() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "exact-row-append-runtime.bn",
        r#"
store: [
    copy: SOURCE
    source_rows: LIST {
        [name: TEXT { Alpha }, score: 7]
    }
    target_rows:
        LIST {
            [name: TEXT { Seed }, score: 0]
        }
        |> List/append(item:
            copy |> THEN {
                source_rows
                |> List/find(item, if: item.name == TEXT { Alpha })
                |> WHEN {
                    Found[value] => value
                    NotFound => SKIP
                }
            }
        )
]

document: Document/new(
    root: Element/label(
        element: []
        label: store.target_rows |> List/length() |> Number/to_text(radix: 10)
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let source = source_id(&compiled.plan, "store.copy");
    let append = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match (&op.kind, &op.output) {
            (
                PlanOpKind::ListMutation {
                    mutation: PlanListMutation::Append(append),
                },
                Some(ValueRef::List(list)),
            ) => Some((*list, append)),
            _ => None,
        })
        .expect("typed append plan");
    assert_eq!(append.1.row_field_copies.len(), 2, "{:#?}", append.1);
    assert!(
        append
            .1
            .row_field_copies
            .iter()
            .all(|copy| copy.source_list != append.0)
    );
    let target_list = append.0;
    let target_fields = append
        .1
        .fields
        .iter()
        .map(|field| (field.name.clone(), field.field_id))
        .collect::<BTreeMap<_, _>>();
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: route_token(&session, source, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    let rows = session.list_row_snapshots_current(target_list).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[1].fields[&target_fields["name"]],
        Value::Text("Alpha".to_owned())
    );
    assert_eq!(
        rows[1].fields[&target_fields["score"]],
        Value::integer(7).unwrap()
    );
}

#[test]
fn list_snapshot_window_reports_logical_length_and_copies_only_the_requested_rows() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "list-snapshot-window-runtime.bn",
        r#"
store: [
    rows:
        List/range(from: 0, to: 99)
        |> List/map(item, new: [value: item])
]

document: Document/new(
    root: Element/label(
        element: []
        label: store.rows |> List/length() |> Number/to_text(radix: 10)
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let rows = compiled.plan.storage_layout.list_slots[0].list_id;
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let (logical_len, window) = session
        .list_row_snapshots_window_current(rows, 10..15)
        .unwrap();
    assert_eq!(logical_len, 100);
    assert_eq!(window.len(), 5);
    assert_eq!(
        window.iter().map(|row| row.id.key).collect::<Vec<_>>(),
        vec![11, 12, 13, 14, 15]
    );

    let (logical_len, beyond_end) = session
        .list_row_snapshots_window_current(rows, 150..180)
        .unwrap();
    assert_eq!(logical_len, 100);
    assert!(beyond_end.is_empty());
    assert!(
        session
            .list_row_snapshots_window_current(rows, 8..7)
            .is_err()
    );
}

#[test]
fn chunk_windows_keep_logical_length_sparse_and_preserve_overlapping_row_identity() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "virtual-chunk-window-runtime.bn",
        r#"
store: [
    cells:
        List/range(from: 0, to: 2599)
        |> List/map(item, new: [
            address: item |> Number/to_text(radix: 10)
            value: item
        ])
    sheet_rows: List/chunk(list: cells, size: 26)
]

document: Document/new(
    root: Element/label(element: [], label: TEXT { virtual chunks })
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let cells = list_id(&compiled.plan, "store.cells");
    let sheet_rows = list_id(&compiled.plan, "store.sheet_rows");
    let items_field = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|entry| entry.label == "store.sheet_rows.items")
        .and_then(|entry| entry.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .expect("chunk items field");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    assert_eq!(session.list_logical_len_current(sheet_rows).unwrap(), 100);
    assert!(
        session.list_row_snapshots(sheet_rows).unwrap().is_empty(),
        "a logical-length read must not materialize chunk rows"
    );

    let (logical_len, first) = session
        .list_row_snapshots_window_current(sheet_rows, 0..4)
        .unwrap();
    assert_eq!(logical_len, 100);
    assert_eq!(first.len(), 4);
    assert_eq!(session.list_row_snapshots(sheet_rows).unwrap().len(), 4);
    let Value::List(first_items) = &first[0].fields[&items_field] else {
        panic!("chunk items must remain a typed list");
    };
    assert_eq!(first_items.len(), 26);
    assert!(
        first_items
            .iter()
            .all(|value| matches!(value, Value::Row { id, .. } if id.list == cells))
    );

    let (_, forward) = session
        .list_row_snapshots_window_current(sheet_rows, 2..6)
        .unwrap();
    assert_eq!(forward.len(), 4);
    assert_eq!(first[2].id, forward[0].id);
    assert_eq!(first[3].id, forward[1].id);
    let materialized = session.list_row_snapshots(sheet_rows).unwrap();
    assert_eq!(
        materialized.len(),
        4,
        "materialized={:?}",
        materialized.iter().map(|row| row.id).collect::<Vec<_>>()
    );

    let (_, backward) = session
        .list_row_snapshots_window_current(sheet_rows, 1..3)
        .unwrap();
    assert_eq!(backward.len(), 2);
    assert_eq!(forward[0].id, backward[1].id);
    assert_eq!(session.list_row_snapshots(sheet_rows).unwrap().len(), 2);

    let (_, distant) = session
        .list_row_snapshots_window_current(sheet_rows, 90..93)
        .unwrap();
    assert_eq!(distant.len(), 3);
    assert_eq!(session.list_row_snapshots(sheet_rows).unwrap().len(), 3);
    let (_, returned) = session
        .list_row_snapshots_window_current(sheet_rows, 2..3)
        .unwrap();
    assert_eq!(returned.len(), 1);
    assert_eq!(
        first[2].id, returned[0].id,
        "virtual row identity must not depend on cache residency"
    );
    assert_eq!(session.list_row_snapshots(sheet_rows).unwrap().len(), 1);

    let Value::List(all_rows) = session.list_value_current(sheet_rows).unwrap() else {
        panic!("an explicit full-list read must return a list");
    };
    assert_eq!(all_rows.len(), 100);
    assert_eq!(session.list_row_snapshots(sheet_rows).unwrap().len(), 100);
}

#[test]
fn chunk_window_invalidates_on_source_structure_changes_without_full_materialization() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "virtual-chunk-invalidation-runtime.bn",
        r#"
store: [
    add: SOURCE
    remove: SOURCE
    rows:
        LIST {
            [value: 0]
            [value: 1]
            [value: 2]
            [value: 3]
        }
        |> List/append(item:
            add |> THEN { [value: 4] }
        )
        |> List/remove(item, when:
            remove |> THEN { item.value == 4 }
        )
    chunks: List/chunk(list: rows, size: 2)
]

document: Document/new(
    root: Element/label(element: [], label: TEXT { virtual chunks })
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let add = source_id(&compiled.plan, "store.add");
    let remove = source_id(&compiled.plan, "store.remove");
    let chunks = list_id(&compiled.plan, "store.chunks");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let (_, initial) = session
        .list_row_snapshots_window_current(chunks, 0..2)
        .unwrap();
    assert_eq!(initial.len(), 2);
    assert_eq!(session.list_row_snapshots(chunks).unwrap().len(), 2);

    session
        .apply(SourceEvent {
            sequence: 1,
            source: add,
            route: route_token(&session, add, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert_eq!(session.list_logical_len_current(chunks).unwrap(), 3);
    assert_eq!(
        session.list_row_snapshots(chunks).unwrap().len(),
        2,
        "a length refresh must retain but not expand the previous window"
    );

    let (_, appended) = session
        .list_row_snapshots_window_current(chunks, 2..3)
        .unwrap();
    assert_eq!(appended.len(), 1);
    assert_eq!(appended[0].id.key, 3);
    assert_eq!(session.list_row_snapshots(chunks).unwrap().len(), 1);

    session
        .apply(SourceEvent {
            sequence: 2,
            source: remove,
            route: route_token(&session, remove, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert_eq!(session.list_logical_len_current(chunks).unwrap(), 2);
    assert!(
        session.list_row_snapshots(chunks).unwrap().is_empty(),
        "a source shrink must evict now-out-of-range chunk rows"
    );

    let (_, returned) = session
        .list_row_snapshots_window_current(chunks, 0..2)
        .unwrap();
    assert_eq!(
        returned.iter().map(|row| row.id).collect::<Vec<_>>(),
        initial.iter().map(|row| row.id).collect::<Vec<_>>()
    );
    assert_eq!(session.list_row_snapshots(chunks).unwrap().len(), 2);
}

#[test]
fn list_filter_uses_the_same_typed_index_path() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-indexed-filter-runtime.bn",
        r#"
store: [
    choose: SOURCE
    selector:
        TEXT { a } |> HOLD selector {
            choose.text |> THEN { choose.text }
        }
    items: LIST {
        [key: TEXT { a }, value: TEXT { A }]
        [key: TEXT { b }, value: TEXT { B }]
    }
    filtered:
        items
        |> List/filter(item, if: item.key == selector)
    count: filtered |> List/length()
]
document: Document/new(
    root: Element/label(
        element: []
        label: store.count |> Number/to_text()
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let source = source_id(&compiled.plan, "store.choose");
    let filtered = compiled
        .plan
        .debug_map
        .list_slots
        .iter()
        .find(|list| list.label == "store.filtered")
        .and_then(|list| list.id.strip_prefix("list:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(ListId)
        .expect("store.filtered list id");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: route_token(&session, source, None),
            target: None,
            payload: SourcePayload {
                text: Some("b".to_owned()),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    assert!(turn.metrics.indexed_access_count >= 1);
    assert_eq!(turn.metrics.list_find_scan_count, 0);
    let snapshot = session.snapshot().unwrap();
    let rows = &snapshot.lists[&filtered];
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0]
            .fields
            .values()
            .any(|value| value == &Value::Text("B".to_owned()))
    );
}

#[test]
fn typed_order_chain_is_stable_and_take_is_bounded() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-order-runtime.bn",
        r#"
store: [
    items: LIST {
        [name: TEXT { first }, group: TEXT { B }, rank: 2]
        [name: TEXT { second }, group: TEXT { A }, rank: 2]
        [name: TEXT { third }, group: TEXT { A }, rank: 1]
        [name: TEXT { fourth }, group: TEXT { A }, rank: 1]
    }
    ordered:
        items
        |> List/sort_by(item, key: item.group)
        |> List/then_by(item, key: item.rank, direction: Descending)
        |> List/take(count: 3)
]
document: Document/new(
    root: Element/label(
        element: []
        label: TEXT { ordered test }
    )
)
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let ordered = compiled
        .plan
        .debug_map
        .list_slots
        .iter()
        .find(|list| list.label == "store.ordered")
        .and_then(|list| list.id.strip_prefix("list:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(ListId)
        .expect("store.ordered list id");
    assert_eq!(compiled.plan.list_indexes.len(), 1);
    let name_field = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == ordered)
        .and_then(|slot| slot.row_fields.iter().find(|field| field.name == "name"))
        .map(|field| field.field_id)
        .expect("store.ordered.name field id");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let (_, metrics) = session.list_value_current_with_metrics(ordered).unwrap();
    assert_eq!(metrics.access_index_seek_count, 1);
    assert_eq!(metrics.access_candidate_count, 3);
    assert_eq!(metrics.access_result_count, 3);
    let snapshot = session.snapshot().unwrap();
    let names = snapshot.lists[&ordered]
        .iter()
        .map(|row| row.fields[&name_field].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            Value::Text("second".to_owned()),
            Value::Text("third".to_owned()),
            Value::Text("fourth".to_owned()),
        ]
    );
}

#[test]
fn dynamic_order_direction_switches_between_prebuilt_indexes_without_rebuild() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "dynamic-typed-order-runtime.bn",
        r#"
store: [
    reverse: SOURCE
    direction:
        Ascending |> HOLD direction {
            reverse |> THEN { Descending }
        }
    items: LIST {
        [name: TEXT { Beta }]
        [name: TEXT { Alpha }]
        [name: TEXT { Gamma }]
    }
    ordered:
        items
        |> List/sort_by(item, key: item.name, direction: direction)
        |> List/take(count: 2)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert_eq!(compiled.plan.list_indexes.len(), 2);
    let reverse = source_id(&compiled.plan, "store.reverse");
    let ordered = list_id(&compiled.plan, "store.ordered");
    let name_field = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == ordered)
        .and_then(|slot| slot.row_fields.iter().find(|field| field.name == "name"))
        .map(|field| field.field_id)
        .expect("store.ordered.name field id");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let (_, initial_metrics) = session.list_value_current_with_metrics(ordered).unwrap();
    assert_eq!(initial_metrics.access_index_seek_count, 1);
    assert_eq!(initial_metrics.access_full_scan_count, 0);
    assert_eq!(
        session.snapshot().unwrap().lists[&ordered]
            .iter()
            .map(|row| row.fields[&name_field].clone())
            .collect::<Vec<_>>(),
        vec![
            Value::Text("Alpha".to_owned()),
            Value::Text("Beta".to_owned()),
        ]
    );

    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source: reverse,
            route: route_token(&session, reverse, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert_eq!(turn.metrics.ordered_index_full_rebuild_count, 0);
    let (_, switched_metrics) = session.list_value_current_with_metrics(ordered).unwrap();
    assert_eq!(switched_metrics.access_index_seek_count, 1);
    assert_eq!(switched_metrics.access_full_scan_count, 0);
    assert_eq!(switched_metrics.ordered_index_full_rebuild_count, 0);
    assert_eq!(
        session.snapshot().unwrap().lists[&ordered]
            .iter()
            .map(|row| row.fields[&name_field].clone())
            .collect::<Vec<_>>(),
        vec![
            Value::Text("Gamma".to_owned()),
            Value::Text("Beta".to_owned()),
        ]
    );
}

#[test]
fn named_intermediate_executes_the_authoritative_mixed_order_chain() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "named-typed-order-runtime.bn",
        r#"
store: [
    items: LIST {
        [name: TEXT { Beta }, rank: 2]
        [name: TEXT { Alpha }, rank: 1]
        [name: TEXT { Alpha }, rank: 3]
    }
    primary:
        items |> List/sort_by(item, key: item.name)
    ordered:
        primary
        |> List/then_by(item, key: item.rank, direction: Descending)
        |> List/take(count: 3)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let ordered = list_id(&compiled.plan, "store.ordered");
    let slot = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == ordered)
        .expect("store.ordered list slot");
    let name = slot
        .row_fields
        .iter()
        .find(|field| field.name == "name")
        .map(|field| field.field_id)
        .expect("store.ordered.name");
    let rank = slot
        .row_fields
        .iter()
        .find(|field| field.name == "rank")
        .map(|field| field.field_id)
        .expect("store.ordered.rank");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let (_, metrics) = session.list_value_current_with_metrics(ordered).unwrap();
    assert_eq!(metrics.access_index_seek_count, 1);
    assert_eq!(metrics.access_full_scan_count, 0);
    assert_eq!(
        session.snapshot().unwrap().lists[&ordered]
            .iter()
            .map(|row| (row.fields[&name].clone(), row.fields[&rank].clone()))
            .collect::<Vec<_>>(),
        vec![
            (Value::Text("Alpha".to_owned()), number(3)),
            (Value::Text("Alpha".to_owned()), number(1)),
            (Value::Text("Beta".to_owned()), number(2)),
        ]
    );
}

#[test]
fn transparent_user_wrappers_execute_the_typed_access_path() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "wrapped-typed-access-runtime.bn",
        r#"
FUNCTION matching(list, entry: OUT, predicate) {
    list |> List/filter(item: entry, if: predicate)
}

FUNCTION ordered(list, entry: OUT, key) {
    list |> List/sort_by(item: entry, key: key, direction: Ascending)
}

FUNCTION secondary(list, entry: OUT, key) {
    list |> List/then_by(item: entry, key: key, direction: Descending)
}

FUNCTION limited(list, count) {
    list |> List/take(count: count)
}

FUNCTION paged(list, size, after) {
    list |> List/page(size: size, after: after)
}

store: [
    items: LIST {
        [name: TEXT { Alpha }, rank: 1]
        [name: TEXT { Beta }, rank: 3]
        [name: TEXT { Alpha }, rank: 2]
    }
    matches:
        items
        |> matching(
            entry
            predicate: entry.name |> Text/starts_with(prefix: TEXT { A })
        )
        |> ordered(entry, key: entry.name)
        |> secondary(entry, key: entry.rank)
        |> limited(count: 2)
    page:
        items
        |> matching(
            entry
            predicate: entry.name |> Text/starts_with(prefix: TEXT { A })
        )
        |> ordered(entry, key: entry.name)
        |> secondary(entry, key: entry.rank)
        |> paged(size: 1, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert_eq!(compiled.plan.list_indexes.len(), 1);
    let matches = list_id(&compiled.plan, "store.matches");
    let name = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == matches)
        .and_then(|slot| slot.row_fields.iter().find(|field| field.name == "name"))
        .map(|field| field.field_id)
        .expect("wrapped result name field");
    let rank = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == matches)
        .and_then(|slot| slot.row_fields.iter().find(|field| field.name == "rank"))
        .map(|field| field.field_id)
        .expect("wrapped result rank field");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let (_, list_metrics) = session.list_value_current_with_metrics(matches).unwrap();
    assert_eq!(list_metrics.access_index_seek_count, 1);
    assert_eq!(list_metrics.access_full_scan_count, 0);
    assert_eq!(list_metrics.access_candidate_count, 2);
    let snapshot = session.snapshot().unwrap();
    assert_eq!(
        snapshot.lists[&matches]
            .iter()
            .map(|row| row.fields[&name].clone())
            .collect::<Vec<_>>(),
        [
            Value::Text("Alpha".to_owned()),
            Value::Text("Alpha".to_owned()),
        ]
    );
    assert_eq!(
        snapshot.lists[&matches]
            .iter()
            .map(|row| row.fields[&rank].clone())
            .collect::<Vec<_>>(),
        [number(2), number(1)]
    );

    let (page, page_metrics) = session
        .root_value_current_with_metrics("store.page")
        .unwrap();
    let (items, next) = page_parts(page);
    assert_eq!(page_names(&items), ["Alpha"]);
    assert!(matches!(
        &items[0],
        Value::Record(fields) if fields.get("rank") == Some(&number(2))
    ));
    assert!(matches!(next, Value::Record(_)));
    assert_eq!(page_metrics.access_index_seek_count, 1);
    assert_eq!(page_metrics.access_full_scan_count, 0);
    assert_eq!(page_metrics.access_candidate_count, 2);
}

#[test]
fn trailing_maps_execute_only_for_rows_selected_by_bounded_take_and_page() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-access-map-suffix.bn",
        r#"
store: [
    items: LIST {
        [name: TEXT { Alpha }, rank: 3]
        [name: TEXT { Beta }, rank: 9]
        [name: TEXT { Alpine }, rank: 2]
    }
    mapped:
        items
        |> List/filter(item, if:
            item.name |> Text/starts_with(prefix: TEXT { A })
        )
        |> List/sort_by(item, key: item.rank, direction: Descending)
        |> List/map(item, new: [label: item.name, score: item.rank])
        |> List/take(count: 2)
    page:
        items
        |> List/filter(item, if:
            item.name |> Text/starts_with(prefix: TEXT { A })
        )
        |> List/sort_by(item, key: item.rank, direction: Descending)
        |> List/map(item, new: [label: item.name, score: item.rank])
        |> List/page(size: 1, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert!(
        compiled
            .plan
            .list_indexes
            .iter()
            .all(|index| index.keys.len() == 1)
    );
    let mapped = list_id(&compiled.plan, "store.mapped");
    let slot = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == mapped)
        .expect("store.mapped list slot");
    let label = slot
        .row_fields
        .iter()
        .find(|field| field.name == "label")
        .map(|field| field.field_id)
        .expect("store.mapped.label");
    let score = slot
        .row_fields
        .iter()
        .find(|field| field.name == "score")
        .map(|field| field.field_id)
        .expect("store.mapped.score");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let (_, mapped_metrics) = session.list_value_current_with_metrics(mapped).unwrap();
    assert_eq!(mapped_metrics.access_index_seek_count, 1);
    assert_eq!(mapped_metrics.access_candidate_count, 2);
    assert_eq!(mapped_metrics.access_full_scan_count, 0);
    assert_eq!(
        session.snapshot().unwrap().lists[&mapped]
            .iter()
            .map(|row| (row.fields[&label].clone(), row.fields[&score].clone()))
            .collect::<Vec<_>>(),
        vec![
            (Value::Text("Alpha".to_owned()), number(3)),
            (Value::Text("Alpine".to_owned()), number(2)),
        ]
    );

    let (page, page_metrics) = session
        .root_value_current_with_metrics("store.page")
        .unwrap();
    let (items, next) = page_parts(page);
    assert!(matches!(
        items.as_slice(),
        [Value::Record(fields)]
            if fields.get("label") == Some(&Value::Text("Alpha".to_owned()))
                && fields.get("score") == Some(&number(3))
    ));
    assert!(matches!(next, Value::Record(_)));
    assert_eq!(page_metrics.access_index_seek_count, 1);
    assert_eq!(page_metrics.access_candidate_count, 2);
    assert_eq!(page_metrics.access_full_scan_count, 0);
    assert_eq!(page_metrics.bounded_page_scan_count, 1);
    assert_eq!(page_metrics.bounded_page_candidate_count, 2);
}

#[test]
fn token_list_membership_uses_deduplicated_expanded_keys_without_authority_scans() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-token-membership-runtime.bn",
        r#"
store: [
    selected_token: TEXT { rail }
    selected_other: TEXT { oslo }
    items: LIST {
        [name: TEXT { Alpha }, rank: 2, tokens: LIST { TEXT { rail }, TEXT { oslo }, TEXT { rail } }]
        [name: TEXT { Beta }, rank: 9, tokens: LIST { TEXT { bus } }]
        [name: TEXT { Gamma }, rank: 1, tokens: LIST { TEXT { rail } }]
        [name: TEXT { Empty }, rank: 0, tokens: LIST {}]
    }
    matches:
        items
        |> List/filter(item, if:
            item.tokens
            |> List/any(item, if: item == selected_token)
        )
        |> List/sort_by(item, key: item.rank, direction: Ascending)
        |> List/take(count: 10)
    both:
        items
        |> List/filter(item, if:
            item.tokens
            |> List/any(item, if: item == selected_token)
        )
        |> List/filter(item, if:
            item.tokens
            |> List/any(item, if: item == selected_other)
        )
        |> List/sort_by(item, key: item.rank, direction: Ascending)
        |> List/take(count: 10)
    page:
        items
        |> List/filter(item, if:
            item.tokens
            |> List/any(item, if: item == selected_token)
        )
        |> List/sort_by(item, key: item.rank, direction: Ascending)
        |> List/page(size: 1, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .expect("token-list membership must compile to expanded typed access");
    let matches = list_id(&compiled.plan, "store.matches");
    let both = list_id(&compiled.plan, "store.both");
    let source = list_id(&compiled.plan, "store.items");
    let source_tokens = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == source)
        .and_then(|slot| slot.row_fields.iter().find(|field| field.name == "tokens"))
        .map(|field| field.field_id)
        .expect("source token-list field");
    let result_name_field = |list| {
        compiled
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list)
            .and_then(|slot| slot.row_fields.iter().find(|field| field.name == "name"))
            .map(|field| field.field_id)
            .expect("token result name field")
    };
    let name = result_name_field(matches);
    let both_name = result_name_field(both);
    let plan = compiled.plan;
    let mut session = MachineInstance::new(plan.clone(), SessionOptions::default()).unwrap();
    assert_eq!(session.startup_metrics().ordered_index_current_count, 1);
    assert_eq!(
        session
            .startup_metrics()
            .ordered_index_current_logical_row_count,
        4
    );
    assert_eq!(
        session.startup_metrics().ordered_index_current_entry_count,
        4
    );
    assert_eq!(
        session
            .startup_metrics()
            .ordered_index_current_expanded_key_count,
        4
    );

    let (_, metrics) = session.list_value_current_with_metrics(matches).unwrap();
    assert_eq!(metrics.access_index_seek_count, 1);
    assert_eq!(metrics.access_candidate_count, 2);
    assert_eq!(metrics.access_full_scan_count, 0);
    assert_eq!(
        session.snapshot().unwrap().lists[&matches]
            .iter()
            .map(|row| row.fields[&name].clone())
            .collect::<Vec<_>>(),
        [
            Value::Text("Gamma".to_owned()),
            Value::Text("Alpha".to_owned()),
        ]
    );

    let (_, metrics) = session.list_value_current_with_metrics(both).unwrap();
    assert_eq!(metrics.access_index_seek_count, 2);
    assert!(metrics.access_candidate_count <= 3);
    assert_eq!(metrics.access_full_scan_count, 0);
    assert_eq!(
        session.snapshot().unwrap().lists[&both]
            .iter()
            .map(|row| row.fields[&both_name].clone())
            .collect::<Vec<_>>(),
        [Value::Text("Alpha".to_owned())]
    );

    let beta = session.list_rows(source)[1];
    session
        .test_set_row_field(
            beta,
            source_tokens,
            Value::List(vec![Value::Text("rail".to_owned())]),
        )
        .unwrap();
    let (_, mutation_metrics) = session.list_value_current_with_metrics(matches).unwrap();
    assert_eq!(mutation_metrics.ordered_index_full_rebuild_count, 0);
    assert_eq!(mutation_metrics.ordered_index_incremental_row_count, 1);
    assert_eq!(mutation_metrics.ordered_index_key_evaluation_count, 1);
    assert_eq!(mutation_metrics.ordered_index_update_count, 1);
    assert_eq!(mutation_metrics.access_full_scan_count, 0);
    assert_eq!(
        session.snapshot().unwrap().lists[&matches]
            .iter()
            .map(|row| row.fields[&name].clone())
            .collect::<Vec<_>>(),
        [
            Value::Text("Gamma".to_owned()),
            Value::Text("Alpha".to_owned()),
            Value::Text("Beta".to_owned()),
        ]
    );

    let options = SessionOptions {
        cursor_sealing_key: Some(CursorSealingKey::from_bytes([0x61; 32])),
        cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x62; 32])),
        ..SessionOptions::default()
    };
    let mut first = MachineInstance::new(plan.clone(), options.clone()).unwrap();
    let (page, first_metrics) = first.root_value_current_with_metrics("store.page").unwrap();
    let (first_items, cursor) = page_parts(page);
    assert_eq!(page_names(&first_items), ["Gamma"]);
    assert!(matches!(cursor, Value::Record(_)));
    assert_eq!(first_metrics.access_index_seek_count, 1);
    assert_eq!(first_metrics.access_candidate_count, 2);
    assert_eq!(first_metrics.access_full_scan_count, 0);

    let second_plan = plan_with_page_position(plan, cursor);
    let mut second = MachineInstance::new(second_plan, options).unwrap();
    let (page, second_metrics) = second
        .root_value_current_with_metrics("store.page")
        .unwrap();
    let (second_items, end) = page_parts(page);
    assert_eq!(page_names(&second_items), ["Alpha"]);
    assert_eq!(end, Value::Text("End".to_owned()));
    assert_eq!(second_metrics.access_cursor_seek_count, 1);
    assert_eq!(second_metrics.access_candidate_count, 1);
    assert_eq!(second_metrics.access_full_scan_count, 0);
}

#[test]
fn typed_list_page_preserves_scalar_items_across_direct_cursor_seeks() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-scalar-page.bn",
        r#"
store: [
    items: LIST { 3, 1, 2 }
    page:
        items
        |> List/page(size: 2, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let options = SessionOptions {
        cursor_sealing_key: Some(CursorSealingKey::from_bytes([0x47; 32])),
        cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x48; 32])),
        ..SessionOptions::default()
    };
    let mut first = MachineInstance::new(compiled.plan.clone(), options.clone()).unwrap();
    let (page, first_metrics) = first.root_value_current_with_metrics("store.page").unwrap();
    let (first_items, cursor) = page_parts(page);
    assert_eq!(first_items, vec![number(3), number(1)]);
    assert!(matches!(cursor, Value::Record(_)));
    assert_eq!(first_metrics.access_index_seek_count, 0);
    assert_eq!(first_metrics.access_candidate_count, 0);
    assert_eq!(first_metrics.access_full_scan_count, 0);
    assert_eq!(first_metrics.bounded_page_scan_count, 1);
    assert_eq!(first_metrics.bounded_page_candidate_count, 3);

    let second_plan = plan_with_page_position(compiled.plan, cursor);
    let mut second = MachineInstance::new(second_plan, options).unwrap();
    let (page, second_metrics) = second
        .root_value_current_with_metrics("store.page")
        .unwrap();
    let (second_items, end) = page_parts(page);
    assert_eq!(second_items, vec![number(2)]);
    assert_eq!(end, Value::Text("End".to_owned()));
    assert_eq!(second_metrics.access_index_seek_count, 0);
    assert_eq!(second_metrics.access_candidate_count, 0);
    assert_eq!(second_metrics.access_full_scan_count, 0);
    assert_eq!(second_metrics.bounded_page_scan_count, 1);
    assert_eq!(second_metrics.bounded_page_candidate_count, 3);
}

#[test]
fn take_then_sort_pages_only_the_bounded_source_prefix() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-take-then-sort-page.bn",
        r#"
store: [
    items: LIST {
        [name: TEXT { Gamma }]
        [name: TEXT { Alpha }]
        [name: TEXT { Beta }]
    }
    page:
        items
        |> List/take(count: 2)
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/page(size: 1, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let options = SessionOptions {
        cursor_sealing_key: Some(CursorSealingKey::from_bytes([0x4b; 32])),
        cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x4c; 32])),
        ..SessionOptions::default()
    };
    let mut first = MachineInstance::new(compiled.plan.clone(), options.clone()).unwrap();
    let (page, first_metrics) = first.root_value_current_with_metrics("store.page").unwrap();
    let (first_items, cursor) = page_parts(page);
    assert_eq!(page_names(&first_items), ["Alpha"]);
    assert!(matches!(cursor, Value::Record(_)));
    assert_eq!(first_metrics.access_index_seek_count, 1);
    assert_eq!(first_metrics.access_candidate_count, 2);
    assert_eq!(first_metrics.access_full_scan_count, 0);
    assert_eq!(first_metrics.bounded_page_scan_count, 1);
    assert_eq!(first_metrics.bounded_page_candidate_count, 2);

    let second_plan = plan_with_page_position(compiled.plan, cursor);
    let mut second = MachineInstance::new(second_plan, options).unwrap();
    let (second_items, end) = page_parts(second.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&second_items), ["Gamma"]);
    assert_eq!(end, Value::Text("End".to_owned()));
}

#[test]
fn typed_list_page_cursor_binds_trailing_map_captures() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-page-map-capture.bn",
        r#"
store: [
    suffix_input: SOURCE
    suffix:
        TEXT { one } |> HOLD suffix {
            suffix_input.value |> THEN { suffix_input.value }
        }
    items: LIST {
        [name: TEXT { Alpha }]
        [name: TEXT { Beta }]
    }
    page:
        items
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/map(item, new: [
            name: item.name
            label: item.name |> Text/concat(with: suffix, separator: "-")
        ])
        |> List/page(size: 1, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let suffix_input = source_id(&compiled.plan, "store.suffix_input");
    let options = SessionOptions {
        cursor_sealing_key: Some(CursorSealingKey::from_bytes([0x49; 32])),
        cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x4a; 32])),
        ..SessionOptions::default()
    };
    let mut first = MachineInstance::new(compiled.plan.clone(), options.clone()).unwrap();
    let (first_items, cursor) = page_parts(first.root_value_current("store.page").unwrap());
    assert!(matches!(
        first_items.as_slice(),
        [Value::Record(fields)]
            if fields.get("label") == Some(&Value::Text("Alpha-one".to_owned()))
    ));

    let cursor_plan = plan_with_page_position(compiled.plan, cursor);
    let mut changed = MachineInstance::new(cursor_plan, options).unwrap();
    changed
        .apply(SourceEvent {
            sequence: 1,
            source: suffix_input,
            route: route_token(&changed, suffix_input, None),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([("value".to_owned(), Value::Text("two".to_owned()))]),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    assert_eq!(
        changed.root_value_current("store.page").unwrap(),
        Value::Text("InvalidPageCursor".to_owned())
    );
}

#[test]
fn identical_typed_views_share_one_physical_index() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-index-deduplication.bn",
        r#"
store: [
    items: LIST {
        [name: TEXT { Beta }]
        [name: TEXT { Alpha }]
    }
    first:
        items
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/take(count: 1)
    second:
        items
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/take(count: 2)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert_eq!(compiled.plan.list_indexes.len(), 1);
    assert_eq!(verify_plan(&compiled.plan).unwrap().status, "pass");
    let mut access_indexes = Vec::new();
    for op in compiled.plan.regions.iter().flat_map(|region| &region.ops) {
        if let PlanOpKind::DerivedValue {
            expression: Some(PlanDerivedExpression::MaterializeList { expression, .. }),
            ..
        } = &op.kind
            && let PlanDerivedExpression::RowExpression { expression } = expression.as_ref()
            && let PlanRowExpressionNode::ListAccess { access } =
                compiled.plan.row_expression(*expression).unwrap()
        {
            access_indexes.push(access.index);
        }
    }
    assert_eq!(access_indexes.len(), 2);
    assert!(
        access_indexes
            .iter()
            .all(|index| *index == PlanListIndexId(0))
    );

    let first = list_id(&compiled.plan, "store.first");
    let second = list_id(&compiled.plan, "store.second");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    assert!(
        matches!(session.list_value_current(first).unwrap(), Value::List(items) if items.len() == 1)
    );
    assert!(
        matches!(session.list_value_current(second).unwrap(), Value::List(items) if items.len() == 2)
    );
}

#[test]
fn typed_exact_range_and_boolean_access_use_bounded_structural_selections() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-structural-access-runtime.bn",
        r#"
store: [
    selected_group: TEXT { A }
    items: LIST {
        [name: TEXT { first }, group: TEXT { A }, rank: 3]
        [name: TEXT { second }, group: TEXT { B }, rank: 1]
        [name: TEXT { third }, group: TEXT { A }, rank: 2]
        [name: TEXT { fourth }, group: TEXT { A }, rank: 4]
        [name: TEXT { fifth }, group: TEXT { B }, rank: 5]
    }
    exact:
        items
        |> List/filter(item, if: item.group == selected_group)
        |> List/take(count: 10)
    ranged:
        items
        |> List/filter(item, if:
            Bool/and(
                left: item.rank >= 2
                right: 5 > item.rank
            )
        )
        |> List/sort_by(item, key: item.rank, direction: Ascending)
        |> List/take(count: 10)
    descending:
        items
        |> List/filter(item, if:
            Bool/and(
                left: item.rank >= 2
                right: item.rank < 5
            )
        )
        |> List/sort_by(item, key: item.rank, direction: Descending)
        |> List/take(count: 10)
    either:
        items
        |> List/filter(item, if:
            Bool/or(
                left: item.rank == 1
                right: item.rank >= 4
            )
        )
        |> List/sort_by(item, key: item.rank, direction: Ascending)
        |> List/take(count: 10)
    repeated:
        items
        |> List/filter(item, if:
            Bool/and(
                left: item.group == selected_group
                right: item.name |> Text/contains(needle: TEXT { i })
            )
        )
        |> List/filter(item, if:
            item.name |> Text/contains(needle: TEXT { h })
        )
        |> List/sort_by(item, key: item.rank, direction: Ascending)
        |> List/take(count: 10)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let selections = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::DerivedValue {
                expression: Some(PlanDerivedExpression::MaterializeList { expression, .. }),
                ..
            } => match expression.as_ref() {
                PlanDerivedExpression::RowExpression { expression } => {
                    match compiled.plan.row_expression(*expression).unwrap() {
                        PlanRowExpressionNode::ListAccess { access } => {
                            Some(access.selection.clone())
                        }
                        _ => None,
                    }
                }
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        selections
            .iter()
            .any(|selection| matches!(selection, PlanListAccessSelection::KeyPrefix { .. }))
    );
    assert!(selections.iter().any(|selection| matches!(
        selection,
        PlanListAccessSelection::ComponentRange {
            lower: Some(_),
            upper: Some(_),
            ..
        }
    )));
    assert!(
        selections
            .iter()
            .any(|selection| matches!(selection, PlanListAccessSelection::Union { .. }))
    );

    let exact = list_id(&compiled.plan, "store.exact");
    let ranged = list_id(&compiled.plan, "store.ranged");
    let descending = list_id(&compiled.plan, "store.descending");
    let either = list_id(&compiled.plan, "store.either");
    let repeated = list_id(&compiled.plan, "store.repeated");
    let list_names = |session: &MachineInstance, list: ListId| {
        let name = session
            .plan()
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list)
            .and_then(|slot| slot.row_fields.iter().find(|field| field.name == "name"))
            .map(|field| field.field_id)
            .unwrap();
        session.snapshot().unwrap().lists[&list]
            .iter()
            .map(|row| match &row.fields[&name] {
                Value::Text(name) => name.clone(),
                other => panic!("typed list name is not Text: {other:?}"),
            })
            .collect::<Vec<_>>()
    };
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let (_, exact_metrics) = session.list_value_current_with_metrics(exact).unwrap();
    assert_eq!(list_names(&session, exact), ["first", "third", "fourth"]);
    assert_eq!(exact_metrics.access_full_scan_count, 0);
    assert_eq!(exact_metrics.access_index_seek_count, 1);

    let (_, range_metrics) = session.list_value_current_with_metrics(ranged).unwrap();
    assert_eq!(list_names(&session, ranged), ["third", "first", "fourth"]);
    assert_eq!(range_metrics.access_full_scan_count, 0);
    assert_eq!(range_metrics.access_index_seek_count, 1);

    let (_, descending_metrics) = session.list_value_current_with_metrics(descending).unwrap();
    assert_eq!(
        list_names(&session, descending),
        ["fourth", "first", "third"]
    );
    assert_eq!(descending_metrics.access_full_scan_count, 0);
    assert_eq!(descending_metrics.access_index_seek_count, 1);

    let (_, union_metrics) = session.list_value_current_with_metrics(either).unwrap();
    assert_eq!(list_names(&session, either), ["second", "fourth", "fifth"]);
    assert_eq!(union_metrics.access_full_scan_count, 0);
    let (_, repeated_metrics) = session.list_value_current_with_metrics(repeated).unwrap();
    assert_eq!(list_names(&session, repeated), ["third"]);
    assert_eq!(repeated_metrics.access_full_scan_count, 0);
    assert_eq!(repeated_metrics.access_index_seek_count, 1);
    assert_eq!(union_metrics.access_index_seek_count, 2);
    assert!(union_metrics.access_branch_poll_count >= 2);
}

#[test]
fn typed_list_page_seeks_directly_materializes_rows_and_expires_only_on_source_change() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-page-runtime.bn",
        r#"
store: [
    add: SOURCE
    other_add: SOURCE
    candidate: add |> THEN { [name: add.text] }
    other_candidate: other_add |> THEN { [name: other_add.text] }
    items:
        LIST {
            [name: TEXT { Alpha }]
            [name: TEXT { Beta }]
            [name: TEXT { Charlie }]
            [name: TEXT { Delta }]
            [name: TEXT { Echo }]
        }
        |> List/append(item: candidate)
    others:
        LIST { [name: TEXT { Other }] }
        |> List/append(item: other_candidate)
    page: items |> List/page(size: 2, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let add = source_id(&compiled.plan, "store.add");
    let other_add = source_id(&compiled.plan, "store.other_add");
    let key = CursorSealingKey::from_bytes([0x51; 32]);
    let options = SessionOptions {
        cursor_sealing_key: Some(key.clone()),
        cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x52; 32])),
        ..SessionOptions::default()
    };
    let mut first = MachineInstance::new(compiled.plan.clone(), options.clone()).unwrap();
    let (first_value, first_metrics) = root_field_current_with_metrics(&mut first, "store.page");
    let (first_items, first_next) = page_parts(first_value);
    assert_eq!(page_names(&first_items), ["Alpha", "Beta"]);
    assert_eq!(first_metrics.access_index_seek_count, 1);
    assert_eq!(first_metrics.access_cursor_seek_count, 0);
    assert_eq!(first_metrics.access_full_scan_count, 0);
    let authority = first.durable_restore_image(0, BTreeSet::new()).unwrap();

    let cursor_plan = plan_with_page_position(compiled.plan.clone(), first_next.clone());
    verify_plan(&cursor_plan).unwrap();

    let mut revised_options = options.clone();
    revised_options.program_revision = 87;
    let mut revised = MachineInstanceBuilder::new(cursor_plan.clone(), revised_options)
        .unwrap()
        .restore_durable(authority.clone())
        .unwrap()
        .build()
        .unwrap();
    let (revised_items, _) = page_parts(revised.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&revised_items), ["Charlie", "Delta"]);

    let resized_plan = plan_with_page_size(cursor_plan.clone(), 1);
    verify_plan(&resized_plan).unwrap();
    let mut resized = MachineInstanceBuilder::new(resized_plan, options.clone())
        .unwrap()
        .restore_durable(authority.clone())
        .unwrap()
        .build()
        .unwrap();
    let (resized_items, resized_next) =
        page_parts(resized.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&resized_items), ["Charlie"]);
    assert!(matches!(resized_next, Value::Record(_)));

    let mut wrong_scope = MachineInstanceBuilder::new(
        cursor_plan.clone(),
        SessionOptions {
            cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x53; 32])),
            ..options.clone()
        },
    )
    .unwrap()
    .restore_durable(authority.clone())
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(
        wrong_scope.root_value_current("store.page").unwrap(),
        Value::Text("InvalidPageCursor".to_owned())
    );

    let mut wrong_principal = MachineInstanceBuilder::new(
        cursor_plan.clone(),
        SessionOptions {
            session_context: SessionContext::Available {
                status: SessionConnectionStatus::Current,
                principal: SessionPrincipal::authenticated("another-user", ["reader"]).unwrap(),
            },
            ..options.clone()
        },
    )
    .unwrap()
    .restore_durable(authority.clone())
    .unwrap()
    .build()
    .unwrap();
    assert_eq!(
        wrong_principal.root_value_current("store.page").unwrap(),
        Value::Text("InvalidPageCursor".to_owned())
    );

    let mut second = MachineInstanceBuilder::new(cursor_plan.clone(), options.clone())
        .unwrap()
        .restore_durable(authority.clone())
        .unwrap()
        .build()
        .unwrap();
    let (second_value, second_metrics) = root_field_current_with_metrics(&mut second, "store.page");
    let (second_items, _) = page_parts(second_value);
    assert_eq!(page_names(&second_items), ["Charlie", "Delta"]);
    assert_eq!(second_metrics.access_index_seek_count, 1);
    assert_eq!(second_metrics.access_cursor_seek_count, 1);
    assert_eq!(second_metrics.access_full_scan_count, 0);
    assert_eq!(second_metrics.access_candidate_count, 3);

    let mut unrelated = MachineInstanceBuilder::new(cursor_plan.clone(), options.clone())
        .unwrap()
        .restore_durable(authority.clone())
        .unwrap()
        .build()
        .unwrap();
    unrelated
        .apply(SourceEvent {
            sequence: 1,
            source: other_add,
            route: route_token(&unrelated, other_add, None),
            target: None,
            payload: SourcePayload {
                text: Some("Unrelated".to_owned()),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    let (items, _) = page_parts(unrelated.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&items), ["Charlie", "Delta"]);

    second
        .apply(SourceEvent {
            sequence: 1,
            source: add,
            route: route_token(&second, add, None),
            target: None,
            payload: SourcePayload {
                text: Some("Foxtrot".to_owned()),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    assert_eq!(
        second.root_value_current("store.page").unwrap(),
        Value::Text("PageExpired".to_owned())
    );
    second.rollback_unsettled_turn().unwrap();
    let (rolled_back_items, _) = page_parts(second.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&rolled_back_items), ["Charlie", "Delta"]);

    let mut tampered = first_next;
    let Value::Record(fields) = &mut tampered else {
        panic!("page cursor must be a record");
    };
    let Value::Bytes(bytes) = fields.get_mut("value").expect("cursor bytes") else {
        panic!("cursor value must be BYTES");
    };
    let mut changed = bytes.to_vec();
    let middle = changed.len() / 2;
    changed[middle] ^= 0x80;
    *bytes = changed.into();
    let tampered_plan = plan_with_page_position(compiled.plan, tampered);
    let mut tampered_session = MachineInstanceBuilder::new(tampered_plan, options)
        .unwrap()
        .restore_durable(authority)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(
        tampered_session.root_value_current("store.page").unwrap(),
        Value::Text("InvalidPageCursor".to_owned())
    );
}

#[test]
fn typed_list_page_cursor_survives_physical_list_renumbering() {
    let compile = |unrelated: &str| {
        boon_compiler::compile_source_text_to_machine_plan(
            "typed-page-semantic-list-identity.bn",
            &format!(
                r#"
store: [
    {unrelated}
    items: LIST {{
        [name: TEXT {{ Alpha }}]
        [name: TEXT {{ Beta }}]
        [name: TEXT {{ Charlie }}]
    }}
    page:
        items
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/page(size: 1, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT {{ static }}))
"#
            ),
            TargetProfile::SoftwareDefault,
        )
        .unwrap()
    };
    let original = compile("");
    let shifted = compile("aardvark: LIST { [name: TEXT { Unrelated }] }");
    assert_ne!(
        list_id(&original.plan, "store.items"),
        list_id(&shifted.plan, "store.items"),
        "fixture must physically renumber the semantic source list"
    );

    let options = SessionOptions {
        program_revision: 3,
        cursor_sealing_key: Some(CursorSealingKey::from_bytes([0x75; 32])),
        cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x76; 32])),
        ..SessionOptions::default()
    };
    let mut first = MachineInstance::new(original.plan, options.clone()).unwrap();
    let (first_items, cursor) = page_parts(first.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&first_items), ["Alpha"]);

    let mut shifted_options = options;
    shifted_options.program_revision = 4_003;
    let shifted_plan = plan_with_page_position(shifted.plan, cursor);
    verify_plan(&shifted_plan).unwrap();
    let mut second = MachineInstance::new(shifted_plan, shifted_options).unwrap();
    let (second_items, _) = page_parts(second.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&second_items), ["Beta"]);
}

#[test]
fn typed_list_page_returns_closed_dynamic_size_and_work_limit_variants() {
    let dynamic = boon_compiler::compile_source_text_to_machine_plan(
        "typed-page-dynamic-size.bn",
        r#"
store: [
    size_input: SOURCE
    requested_size:
        2 |> HOLD requested_size {
            size_input.value |> THEN {
                size_input.value |> Text/to_number()
            }
        }
    items: LIST {
        [name: TEXT { Alpha }]
        [name: TEXT { Beta }]
        [name: TEXT { Charlie }]
    }
    page: items |> List/page(size: requested_size, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let size_input = source_id(&dynamic.plan, "store.size_input");
    for invalid in ["0", "1.5", "10001"] {
        let mut session =
            MachineInstance::new(dynamic.plan.clone(), SessionOptions::default()).unwrap();
        session
            .apply(SourceEvent {
                sequence: 1,
                source: size_input,
                route: route_token(&session, size_input, None),
                target: None,
                payload: SourcePayload {
                    fields: BTreeMap::from([("value".to_owned(), Value::Text(invalid.to_owned()))]),
                    ..SourcePayload::default()
                },
            })
            .unwrap();
        assert_eq!(
            session.root_value_current("store.page").unwrap(),
            Value::Text("InvalidPageSize".to_owned()),
            "dynamic size {invalid}"
        );
    }

    let bounded = boon_compiler::compile_source_text_to_machine_plan(
        "typed-page-work-limit.bn",
        r#"
store: [
    items: LIST {
        [name: TEXT { Alpha }, active: False]
        [name: TEXT { Alpine }, active: False]
        [name: TEXT { Amber }, active: False]
    }
    page:
        items
        |> List/filter(item, if:
            Bool/and(
                left: item.name |> Text/starts_with(prefix: TEXT { A })
                right: item.active
            )
        )
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/page(size: 1, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let mut limited = MachineInstance::new(
        bounded.plan,
        SessionOptions {
            list_access_work_limits: ListAccessWorkLimits::new(8, 8, 8, 1, 8, 16, 0),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    let (value, metrics) = root_field_current_with_metrics(&mut limited, "store.page");
    assert_eq!(value, Value::Text("PageWorkLimitExceeded".to_owned()));
    assert_eq!(metrics.access_work_limit_failure_count, 1);
    assert_eq!(metrics.access_full_scan_count, 0);
}

#[test]
fn typed_list_page_honors_upstream_take_and_binds_it_into_cursor_identity() {
    let compile = |take_count: usize| {
        boon_compiler::compile_source_text_to_machine_plan(
            "typed-page-take-identity.bn",
            &format!(
                r#"
store: [
    items: LIST {{
        [name: TEXT {{ Alpha }}]
        [name: TEXT {{ Beta }}]
        [name: TEXT {{ Charlie }}]
        [name: TEXT {{ Delta }}]
    }}
    page:
        items
        |> List/take(count: {take_count})
        |> List/page(size: 2, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT {{ static }}))
"#
            ),
            TargetProfile::SoftwareDefault,
        )
        .unwrap()
    };
    let key = CursorSealingKey::from_bytes([0x61; 32]);
    let options = SessionOptions {
        cursor_sealing_key: Some(key),
        cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x62; 32])),
        ..SessionOptions::default()
    };
    let first_compiled = compile(3);
    let mut first = MachineInstance::new(first_compiled.plan.clone(), options.clone()).unwrap();
    let (first_items, first_next) = page_parts(first.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&first_items), ["Alpha", "Beta"]);
    let authority = first.durable_restore_image(0, BTreeSet::new()).unwrap();

    let second_plan = plan_with_page_position(first_compiled.plan, first_next.clone());
    let mut second = MachineInstanceBuilder::new(second_plan, options.clone())
        .unwrap()
        .restore_durable(authority)
        .unwrap()
        .build()
        .unwrap();
    let (second_items, second_next) = page_parts(second.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&second_items), ["Charlie"]);
    assert_eq!(second_next, Value::Text("End".to_owned()));

    let changed_take_plan = plan_with_page_position(compile(4).plan, first_next);
    verify_plan(&changed_take_plan).unwrap();
    let mut changed_take = MachineInstance::new(changed_take_plan, options).unwrap();
    assert_eq!(
        changed_take.root_value_current("store.page").unwrap(),
        Value::Text("InvalidPageCursor".to_owned())
    );
}

#[test]
fn typed_list_page_binds_evaluated_literal_seek_values() {
    let compile = |prefix: &str| {
        boon_compiler::compile_source_text_to_machine_plan(
            "typed-page-literal-capture.bn",
            &format!(
                r#"
store: [
    items: LIST {{
        [name: TEXT {{ Alpha }}]
        [name: TEXT {{ Alpine }}]
        [name: TEXT {{ Beta }}]
        [name: TEXT {{ Bravo }}]
    }}
    page:
        items
        |> List/filter(item, if:
            item.name |> Text/starts_with(prefix: TEXT {{ {prefix} }})
        )
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/page(size: 1, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT {{ static }}))
"#
            ),
            TargetProfile::SoftwareDefault,
        )
        .unwrap()
    };
    let options = SessionOptions {
        cursor_sealing_key: Some(CursorSealingKey::from_bytes([0x69; 32])),
        cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x6a; 32])),
        ..SessionOptions::default()
    };
    let alpha = compile("A");
    let mut first = MachineInstance::new(alpha.plan, options.clone()).unwrap();
    let (first_items, cursor) = page_parts(first.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&first_items), ["Alpha"]);

    let changed = plan_with_page_position(compile("B").plan, cursor);
    verify_plan(&changed).unwrap();
    let mut second = MachineInstance::new(changed, options).unwrap();
    assert_eq!(
        second.root_value_current("store.page").unwrap(),
        Value::Text("InvalidPageCursor".to_owned())
    );
}

#[test]
fn typed_list_page_capture_identity_uses_normalized_seek_values() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-page-normalized-capture.bn",
        r#"
store: [
    query_input: SOURCE
    query:
        TEXT { a } |> HOLD query {
            query_input.value |> THEN { query_input.value }
        }
    items: LIST {
        [name: TEXT { Alpha }]
        [name: TEXT { Alpine }]
        [name: TEXT { Beta }]
    }
    page:
        items
        |> List/filter(item, if:
            item.name
            |> Text/trim()
            |> Text/to_lowercase()
            |> Text/starts_with(prefix:
                query |> Text/trim() |> Text/to_lowercase()
            )
        )
        |> List/sort_by(item, key:
            item.name |> Text/trim() |> Text/to_lowercase()
            direction: Ascending
        )
        |> List/page(size: 1, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let query_input = source_id(&compiled.plan, "store.query_input");
    let options = SessionOptions {
        cursor_sealing_key: Some(CursorSealingKey::from_bytes([0x6b; 32])),
        cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x6c; 32])),
        ..SessionOptions::default()
    };
    let mut first = MachineInstance::new(compiled.plan.clone(), options.clone()).unwrap();
    let (first_items, cursor) = page_parts(first.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&first_items), ["Alpha"]);

    let cursor_plan = plan_with_page_position(compiled.plan, cursor);
    let mut second = MachineInstance::new(cursor_plan, options).unwrap();
    second
        .apply(SourceEvent {
            sequence: 1,
            source: query_input,
            route: route_token(&second, query_input, None),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([("value".to_owned(), Value::Text(" A ".to_owned()))]),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    let (second_items, _) = page_parts(second.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&second_items), ["Alpine"]);
}

#[test]
fn typed_list_page_binds_captures_and_semantic_view_but_not_physical_index() {
    let compile = |direction: &str| {
        boon_compiler::compile_source_text_to_machine_plan(
            "typed-page-view-identity.bn",
            &format!(
                r#"
store: [
    query_input: SOURCE
    query:
        TEXT {{ A }} |> HOLD query {{
            query_input.value |> THEN {{ query_input.value }}
        }}
    items: LIST {{
        [name: TEXT {{ Alpha }}]
        [name: TEXT {{ Alpine }}]
        [name: TEXT {{ Beta }}]
        [name: TEXT {{ Bravo }}]
    }}
    page:
        items
        |> List/filter(item, if:
            item.name |> Text/starts_with(prefix: query)
        )
        |> List/sort_by(item, key: item.name, direction: {direction})
        |> List/page(size: 1, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT {{ static }}))
"#
            ),
            TargetProfile::SoftwareDefault,
        )
        .unwrap()
    };
    let key = CursorSealingKey::from_bytes([0x71; 32]);
    let options = SessionOptions {
        cursor_sealing_key: Some(key),
        cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x72; 32])),
        ..SessionOptions::default()
    };
    let ascending = compile("Ascending");
    let query_input = source_id(&ascending.plan, "store.query_input");
    let mut first = MachineInstance::new(ascending.plan.clone(), options.clone()).unwrap();
    let (first_items, cursor) = page_parts(first.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&first_items), ["Alpha"]);

    let cursor_plan = plan_with_page_position(ascending.plan.clone(), cursor.clone());
    let shifted_plan = plan_with_prefixed_physical_page_index(cursor_plan.clone());
    verify_plan(&shifted_plan).unwrap();
    let mut shifted = MachineInstance::new(shifted_plan, options.clone()).unwrap();
    let (shifted_items, _) = page_parts(shifted.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&shifted_items), ["Alpine"]);

    let mut changed_capture = MachineInstance::new(cursor_plan.clone(), options.clone()).unwrap();
    changed_capture
        .apply(SourceEvent {
            sequence: 1,
            source: query_input,
            route: route_token(&changed_capture, query_input, None),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([("value".to_owned(), Value::Text("B".to_owned()))]),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    assert_eq!(
        changed_capture.root_value_current("store.page").unwrap(),
        Value::Text("InvalidPageCursor".to_owned())
    );

    let descending_plan = plan_with_page_position(compile("Descending").plan, cursor);
    verify_plan(&descending_plan).unwrap();
    let mut descending = MachineInstance::new(descending_plan, options).unwrap();
    assert_eq!(
        descending.root_value_current("store.page").unwrap(),
        Value::Text("InvalidPageCursor".to_owned())
    );

    let mut noncanonical = cursor_plan;
    let changed = visit_plan_pages_mut(&mut noncanonical, &mut |page| {
        page.view_fingerprint[0] ^= 1;
    });
    assert_eq!(changed, 1);
    let verification = verify_plan(&noncanonical).unwrap();
    assert_eq!(verification.status, "fail");
    assert!(verification.checks.iter().any(|check| {
        check.id == "typed-list-access-plans-resolve"
            && !check.pass
            && check.detail.contains("noncanonical view fingerprint")
    }));
}

#[test]
fn dynamic_order_page_rejects_a_cursor_from_the_other_prebuilt_branch() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "dynamic-order-page-cursor.bn",
        r#"
store: [
    reverse: SOURCE
    direction:
        Ascending |> HOLD direction {
            reverse |> THEN { Descending }
        }
    items: LIST {
        [name: TEXT { Alpha }]
        [name: TEXT { Beta }]
        [name: TEXT { Gamma }]
    }
    page:
        items
        |> List/sort_by(item, key: item.name, direction: direction)
        |> List/page(size: 1, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert_eq!(compiled.plan.list_indexes.len(), 2);
    let reverse = source_id(&compiled.plan, "store.reverse");
    let options = SessionOptions {
        cursor_sealing_key: Some(CursorSealingKey::from_bytes([0x73; 32])),
        cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x74; 32])),
        ..SessionOptions::default()
    };

    let mut first = MachineInstance::new(compiled.plan.clone(), options.clone()).unwrap();
    let (first_items, cursor) = page_parts(first.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&first_items), ["Alpha"]);

    let cursor_plan = plan_with_page_positions(compiled.plan, cursor, 2);
    verify_plan(&cursor_plan).unwrap();
    let mut continued = MachineInstance::new(cursor_plan, options).unwrap();
    let (continued_items, _) = page_parts(continued.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&continued_items), ["Beta"]);

    let turn = continued
        .apply(SourceEvent {
            sequence: 1,
            source: reverse,
            route: route_token(&continued, reverse, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert_eq!(turn.metrics.ordered_index_full_rebuild_count, 0);
    assert_eq!(
        continued.root_value_current("store.page").unwrap(),
        Value::Text("InvalidPageCursor".to_owned())
    );
}

#[test]
fn typed_list_page_cursor_survives_touched_list_persistence_restore() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-page-touched-restore.bn",
        r#"
store: [
    add: SOURCE
    candidate: add |> THEN { [name: add.text] }
    items:
        LIST {
            [name: TEXT { Alpha }]
            [name: TEXT { Beta }]
        }
        |> List/append(item: candidate)
    page: items |> List/page(size: 2, after: Start)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let add = source_id(&compiled.plan, "store.add");
    let options = SessionOptions {
        cursor_sealing_key: Some(CursorSealingKey::from_bytes([0x81; 32])),
        cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x82; 32])),
        ..SessionOptions::default()
    };
    let mut first = MachineInstance::new(compiled.plan.clone(), options.clone()).unwrap();
    first
        .apply(SourceEvent {
            sequence: 1,
            source: add,
            route: route_token(&first, add, None),
            target: None,
            payload: SourcePayload {
                text: Some("Charlie".to_owned()),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    first.settle_turn();
    let (first_items, cursor) = page_parts(first.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&first_items), ["Alpha", "Beta"]);
    let authority = first.durable_restore_image(0, BTreeSet::new()).unwrap();

    let cursor_plan = plan_with_page_position(compiled.plan, cursor);
    let mut restored = MachineInstanceBuilder::new(cursor_plan, options)
        .unwrap()
        .restore_durable(authority)
        .unwrap()
        .build()
        .unwrap();
    let (restored_items, restored_next) =
        page_parts(restored.root_value_current("store.page").unwrap());
    assert_eq!(page_names(&restored_items), ["Charlie"]);
    assert_eq!(restored_next, Value::Text("End".to_owned()));
}

fn root_field_current_with_metrics(
    session: &mut MachineInstance,
    name: &str,
) -> (Value, TurnMetrics) {
    session.root_value_current_with_metrics(name).unwrap()
}

fn page_parts(value: Value) -> (Vec<Value>, Value) {
    let Value::Record(mut page) = value else {
        panic!("page result must be a record");
    };
    assert_eq!(page.remove("$tag"), Some(Value::Text("Page".to_owned())));
    let Some(Value::List(items)) = page.remove("items") else {
        panic!("Page.items must be a typed LIST");
    };
    let next = page.remove("next").expect("Page.next");
    (items, next)
}

fn page_names(items: &[Value]) -> Vec<&str> {
    items
        .iter()
        .map(|item| match item {
            Value::Record(fields) => match fields.get("name") {
                Some(Value::Text(name)) => name.as_str(),
                other => panic!("page item has no typed name: {other:?}"),
            },
            other => panic!("page item leaked a runtime row: {other:?}"),
        })
        .collect()
}

fn plan_with_page_position(plan: MachinePlan, position: Value) -> MachinePlan {
    plan_with_page_positions(plan, position, 1)
}

fn plan_with_page_positions(
    mut plan: MachinePlan,
    position: Value,
    expected_pages: usize,
) -> MachinePlan {
    let mut after_constants = Vec::new();
    for (_, expression) in plan.row_expressions.iter() {
        let after = match expression {
            PlanRowExpressionNode::ListPage { page } => Some(page.after),
            PlanRowExpressionNode::BoundedListPage { page } => Some(page.after),
            _ => None,
        };
        if let Some(after) = after {
            after_constants.push(expression_constant_id(&plan, after));
        }
    }
    assert_eq!(
        after_constants.len(),
        expected_pages,
        "test plan must contain the expected List/page variants"
    );
    let position = PlanConstantValue::Data {
        value: position.to_data().unwrap(),
    };
    for constant_id in after_constants {
        plan.constants
            .iter_mut()
            .find(|constant| constant.id == constant_id)
            .expect("page position constant")
            .value = position.clone();
    }
    plan
}

fn expression_constant_id(plan: &MachinePlan, expression: PlanRowExpressionId) -> PlanConstantId {
    match plan.row_expression(expression).unwrap() {
        PlanRowExpressionNode::Constant { constant_id } => *constant_id,
        other => panic!("test fixture expected a constant expression, got {other:?}"),
    }
}

fn plan_with_page_size(mut plan: MachinePlan, size: i64) -> MachinePlan {
    let limit_constants = plan
        .row_expressions
        .iter()
        .filter_map(|(_, expression)| match expression {
            PlanRowExpressionNode::ListPage { page } => {
                Some(expression_constant_id(&plan, page.access.limit))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        limit_constants.len(),
        1,
        "test plan must contain exactly one List/page"
    );
    for constant_id in limit_constants {
        plan.constants
            .iter_mut()
            .find(|constant| constant.id == constant_id)
            .expect("page size constant")
            .value = number_constant(size);
    }
    plan
}

fn plan_with_prefixed_physical_page_index(mut plan: MachinePlan) -> MachinePlan {
    assert_eq!(plan.list_indexes.len(), 1, "test plan must own one index");
    let prefix = plan
        .row_expressions
        .iter()
        .find_map(|(_, expression)| match expression {
            PlanRowExpressionNode::ListPage { page } => Some(page.access.limit),
            _ => None,
        })
        .expect("test plan must contain one List/page");
    let template = plan.list_indexes[0].keys[0].clone();
    plan.list_indexes[0].keys.insert(
        0,
        PlanListIndexKey {
            owner: template.owner,
            row_local: template.row_local,
            expression: prefix,
            kind: PlanListIndexKeyKind::Number,
            closed_tags: Vec::new(),
            direction: PlanOrderDirection::Ascending,
            multiplicity: PlanListIndexKeyMultiplicity::One,
        },
    );
    fn prepend(selection: &mut PlanListAccessSelection, value: PlanRowExpressionId) {
        match selection {
            PlanListAccessSelection::OrderedStart => {
                *selection = PlanListAccessSelection::KeyPrefix {
                    values: vec![value],
                };
            }
            PlanListAccessSelection::KeyPrefix { values } => values.insert(0, value),
            PlanListAccessSelection::TextPrefix { leading, .. }
            | PlanListAccessSelection::ComponentRange { leading, .. } => {
                leading.insert(0, value);
            }
            PlanListAccessSelection::Union { branches }
            | PlanListAccessSelection::Intersection { branches } => {
                for branch in branches {
                    prepend(branch, value);
                }
            }
        }
    }
    let changed = visit_plan_pages_mut(&mut plan, &mut |page| {
        prepend(&mut page.access.selection, prefix);
    });
    assert_eq!(changed, 1, "test plan must contain exactly one List/page");
    plan
}

fn visit_plan_pages_mut(
    plan: &mut MachinePlan,
    visitor: &mut impl FnMut(&mut PlanListPage),
) -> usize {
    let arena = std::mem::take(&mut plan.row_expressions);
    let mut nodes = arena.into_nodes();
    let mut count = 0;
    for expression in &mut nodes {
        if let PlanRowExpressionNode::ListPage { page } = expression {
            visitor(page);
            count += 1;
        }
    }
    plan.row_expressions = PlanRowExpressionArena::from_nodes(nodes).unwrap();
    count
}

#[test]
fn typed_prefix_guard_skips_seek_and_key_changes_refresh_unseen_rows() {
    let compile = |query: &str| {
        boon_compiler::compile_source_text_to_machine_plan(
            "typed-prefix-currentness.bn",
            &format!(
                r#"
store: [
    query: TEXT {{ {query} }}
    items: LIST {{
        [name: TEXT {{ Beta }}]
        [name: TEXT {{ Charlie }}]
        [name: TEXT {{ Delta }}]
    }}
    ordered:
        items
        |> List/filter(item, if:
            Bool/and(
                left: query |> Text/is_not_empty()
                right:
                    item.name
                    |> Text/starts_with(prefix: query)
            )
        )
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/take(count: 2)
]
document: Document/new(root: Element/label(element: [], label: TEXT {{ static }}))
"#
            ),
            TargetProfile::SoftwareDefault,
        )
        .unwrap()
    };

    let empty = compile("");
    let empty_ordered = list_id(&empty.plan, "store.ordered");
    let mut empty_session = MachineInstance::new(empty.plan, SessionOptions::default()).unwrap();
    let (empty_rows, empty_metrics) = empty_session
        .list_value_current_with_metrics(empty_ordered)
        .unwrap();
    assert!(matches!(empty_rows, Value::List(items) if items.is_empty()));
    assert_eq!(empty_metrics.access_index_seek_count, 0);

    // Compile the active-query case for the unseen-row invalidation assertion.
    let compiled = compile("A");
    let source = list_id(&compiled.plan, "store.items");
    let ordered = list_id(&compiled.plan, "store.ordered");
    let source_name = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == source)
        .and_then(|slot| slot.row_fields.iter().find(|field| field.name == "name"))
        .map(|field| field.field_id)
        .unwrap();
    let ordered_name = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == ordered)
        .and_then(|slot| slot.row_fields.iter().find(|field| field.name == "name"))
        .map(|field| field.field_id)
        .unwrap();
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    assert!(matches!(
        session.list_value_current(ordered).unwrap(),
        Value::List(items) if items.is_empty()
    ));
    let beta = session.list_rows(source)[0];
    session
        .test_set_row_field(beta, source_name, Value::Text("Bravo".to_owned()))
        .unwrap();
    let (unchanged, outside_metrics) = session.list_value_current_with_metrics(ordered).unwrap();
    assert!(matches!(unchanged, Value::List(items) if items.is_empty()));
    assert_eq!(outside_metrics.access_index_seek_count, 0);
    assert_eq!(outside_metrics.recomputed_list_count, 0);
    assert_eq!(outside_metrics.dependency_fanout_count, 0);
    assert_eq!(outside_metrics.ordered_index_incremental_row_count, 1);
    assert_eq!(outside_metrics.ordered_index_update_count, 1);

    let delta = session.list_rows(source)[2];
    session
        .test_set_row_field(delta, source_name, Value::Text("Alpha".to_owned()))
        .unwrap();
    let (_, metrics) = session.list_value_current_with_metrics(ordered).unwrap();
    assert_eq!(metrics.access_index_seek_count, 1);
    assert_eq!(metrics.access_full_scan_count, 0);
    assert_eq!(metrics.ordered_index_full_rebuild_count, 0);
    assert_eq!(metrics.ordered_index_incremental_row_count, 1);
    assert_eq!(metrics.ordered_index_key_evaluation_count, 1);
    assert_eq!(metrics.ordered_index_update_count, 1);
    let snapshot = session.snapshot().unwrap();
    assert_eq!(
        snapshot.lists[&ordered][0].fields[&ordered_name],
        Value::Text("Alpha".to_owned())
    );
}

#[test]
fn source_order_only_relabel_refreshes_a_live_cached_take_view() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-source-order-currentness.bn",
        r#"
store: [
    reverse: SOURCE
    direction:
        Ascending |> HOLD direction {
            reverse |> THEN { Descending }
        }
    items: LIST {
        [name: TEXT { Alpha }]
        [name: TEXT { Beta }]
        [name: TEXT { Gamma }]
    }
    projected:
        items
        |> List/sort_by(item, key: item.name, direction: direction)
        |> List/map(item, new: [name: item.name])
    first_two: projected |> List/take(count: 2)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let reverse = source_id(&compiled.plan, "store.reverse");
    let projected = list_id(&compiled.plan, "store.projected");
    let first_two = list_id(&compiled.plan, "store.first_two");
    let name_field = |list| {
        compiled
            .plan
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list)
            .and_then(|slot| slot.row_fields.iter().find(|field| field.name == "name"))
            .map(|field| field.field_id)
            .unwrap()
    };
    let projected_name = name_field(projected);
    let first_two_name = name_field(first_two);
    let names = |session: &MachineInstance, list, name| {
        session.snapshot().unwrap().lists[&list]
            .iter()
            .map(|row| match &row.fields[&name] {
                Value::Text(name) => name.clone(),
                other => panic!("projected row name is not Text: {other:?}"),
            })
            .collect::<Vec<_>>()
    };
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    session.list_value_current(first_two).unwrap();
    assert_eq!(
        names(&session, first_two, first_two_name),
        ["Alpha", "Beta"]
    );

    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source: reverse,
            route: route_token(&session, reverse, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    session.list_value_current(projected).unwrap();
    assert_eq!(
        names(&session, projected, projected_name),
        ["Gamma", "Beta", "Alpha"]
    );
    let (_, take_metrics) = session.list_value_current_with_metrics(first_two).unwrap();
    assert_eq!(
        names(&session, first_two, first_two_name),
        ["Gamma", "Beta"]
    );
    assert!(take_metrics.recomputed_list_count >= 1);
    assert_eq!(take_metrics.access_index_seek_count, 1);
    assert_eq!(take_metrics.access_full_scan_count, 0);
    assert_eq!(turn.metrics.ordered_index_full_rebuild_count, 0);
    assert!(turn.metrics.ordered_index_incremental_row_count <= 3);
    assert_eq!(turn.metrics.access_full_scan_count, 0);
}

#[test]
fn typed_ordered_index_updates_after_append_remove_and_authority_restore() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-index-restore.bn",
        r#"
store: [
    add: SOURCE
    remove: SOURCE
    candidate:
        add |> THEN { [name: add.text] }
    items:
        LIST { [name: TEXT { Beta }] }
        |> List/append(item: candidate)
        |> List/remove(item, when:
            remove |> THEN { item.name == TEXT { Alpha } }
        )
    ordered:
        items
        |> List/filter(item, if:
            item.name |> Text/starts_with(prefix: TEXT { A })
        )
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/take(count: 2)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let add = source_id(&compiled.plan, "store.add");
    let remove = source_id(&compiled.plan, "store.remove");
    let ordered = list_id(&compiled.plan, "store.ordered");
    let ordered_name = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == ordered)
        .and_then(|slot| slot.row_fields.iter().find(|field| field.name == "name"))
        .map(|field| field.field_id)
        .unwrap();
    let mut session =
        MachineInstance::new(compiled.plan.clone(), SessionOptions::default()).unwrap();
    assert!(matches!(
        session.list_value_current(ordered).unwrap(),
        Value::List(items) if items.is_empty()
    ));
    let outside_turn = session
        .apply(SourceEvent {
            sequence: 1,
            source: add,
            route: route_token(&session, add, None),
            target: None,
            payload: SourcePayload {
                text: Some("Zulu".to_owned()),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    assert_eq!(outside_turn.metrics.ordered_index_incremental_row_count, 1);
    assert_eq!(outside_turn.metrics.ordered_index_insert_count, 1);
    assert_eq!(outside_turn.metrics.ordered_index_full_rebuild_count, 0);
    assert_eq!(outside_turn.metrics.source_order_location_update_count, 1);
    assert_eq!(outside_turn.metrics.source_order_location_update_max, 1);
    assert_eq!(outside_turn.metrics.source_order_relabel_operation_count, 0);
    assert_eq!(outside_turn.metrics.source_order_relabel_row_count, 0);
    let (outside, outside_metrics) = session.list_value_current_with_metrics(ordered).unwrap();
    assert!(matches!(outside, Value::List(items) if items.is_empty()));
    assert_eq!(outside_metrics.access_index_seek_count, 0);
    assert_eq!(outside_metrics.recomputed_list_count, 0);
    assert_eq!(outside_metrics.dependency_fanout_count, 0);
    assert_eq!(outside_metrics.ordered_index_incremental_row_count, 0);
    assert_eq!(outside_metrics.ordered_index_insert_count, 0);

    let alpha_turn = session
        .apply(SourceEvent {
            sequence: 2,
            source: add,
            route: route_token(&session, add, None),
            target: None,
            payload: SourcePayload {
                text: Some("Alpha".to_owned()),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    assert_eq!(alpha_turn.metrics.ordered_index_incremental_row_count, 1);
    assert_eq!(alpha_turn.metrics.ordered_index_key_evaluation_count, 1);
    assert_eq!(alpha_turn.metrics.ordered_index_insert_count, 1);
    assert_eq!(alpha_turn.metrics.ordered_index_full_rebuild_count, 0);
    assert_eq!(alpha_turn.metrics.source_order_location_update_count, 1);
    assert_eq!(alpha_turn.metrics.source_order_relabel_operation_count, 0);
    let (value, metrics) = session.list_value_current_with_metrics(ordered).unwrap();
    assert_eq!(metrics.access_index_seek_count, 1);
    assert_eq!(metrics.access_full_scan_count, 0);
    assert_eq!(metrics.ordered_index_full_rebuild_count, 0);
    assert_eq!(metrics.ordered_index_incremental_row_count, 0);
    assert_eq!(metrics.ordered_index_key_evaluation_count, 0);
    assert_eq!(metrics.ordered_index_insert_count, 0);
    assert!(matches!(&value, Value::List(items) if items.len() == 1));
    assert_eq!(
        session.snapshot().unwrap().lists[&ordered][0].fields[&ordered_name],
        Value::Text("Alpha".to_owned())
    );

    let authority = session.authority_snapshot().unwrap();
    let mut restored = MachineInstanceBuilder::new(compiled.plan, SessionOptions::default())
        .unwrap()
        .restore(authority)
        .build()
        .unwrap();
    assert_eq!(restored.list_value_current(ordered).unwrap(), value);
    let remove_turn = restored
        .apply(SourceEvent {
            sequence: 3,
            source: remove,
            route: route_token(&restored, remove, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert_eq!(remove_turn.metrics.ordered_index_incremental_row_count, 1);
    assert_eq!(remove_turn.metrics.ordered_index_key_evaluation_count, 0);
    assert_eq!(remove_turn.metrics.ordered_index_remove_count, 1);
    assert_eq!(remove_turn.metrics.ordered_index_full_rebuild_count, 0);
    assert!(remove_turn.metrics.source_order_location_update_max <= 256);
    assert_eq!(remove_turn.metrics.source_order_relabel_operation_count, 0);
    let (removed, metrics) = restored.list_value_current_with_metrics(ordered).unwrap();
    assert!(matches!(removed, Value::List(items) if items.is_empty()));
    assert_eq!(metrics.ordered_index_full_rebuild_count, 0);
    assert_eq!(metrics.ordered_index_incremental_row_count, 0);
    assert_eq!(metrics.ordered_index_key_evaluation_count, 0);
    assert_eq!(metrics.ordered_index_remove_count, 0);
}

#[test]
fn typed_ordered_index_rejects_concrete_keys_beyond_target_budget_before_readiness() {
    let oversized = "x".repeat(5_000);
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-index-key-budget.bn",
        &format!(
            r#"
store: [
    items: LIST {{ [name: TEXT {{ {oversized} }}] }}
    ordered:
        items
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/take(count: 1)
]
document: Document/new(root: Element/label(element: [], label: TEXT {{ static }}))
"#
        ),
        TargetProfile::SoftwareBounded,
    )
    .expect("the static index inventory fits the bounded target profile");
    let error = match MachineInstance::new(compiled.plan, SessionOptions::default()) {
        Ok(_) => panic!("oversized concrete index key unexpectedly reached readiness"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("encoded key bytes per entry"));
    assert!(error.to_string().contains("maximum 4096"));
}

#[test]
fn machine_build_task_slices_large_storage_and_ordered_index_without_semantic_drift() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "cooperative-machine-build.bn",
        r#"
store: [
    items:
        List/range(from: 0, to: 999)
        |> List/map(item, new: [value: item])
    ordered:
        items
        |> List/sort_by(item, key: item.value, direction: Descending)
        |> List/take(count: 10)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let synchronous = MachineInstanceBuilder::new(compiled.plan.clone(), SessionOptions::default())
        .unwrap()
        .build()
        .unwrap();

    let mut task = MachineInstanceBuilder::new(compiled.plan, SessionOptions::default())
        .unwrap()
        .into_build_task();
    let mut pending_polls = 0_u64;
    let sliced = loop {
        match task.poll(7).unwrap() {
            MachineBuildPoll::Pending(progress) => {
                pending_polls += 1;
                assert_ne!(progress.phase, MachineBuildPhase::Complete);
            }
            MachineBuildPoll::Ready(session) => break session,
        }
    };

    assert!(
        pending_polls > 100,
        "large build did not yield in bounded slices"
    );
    assert_eq!(sliced.snapshot().unwrap(), synchronous.snapshot().unwrap());
    assert_eq!(sliced.startup_metrics(), synchronous.startup_metrics());
    assert!(sliced.startup_metrics().work_unit_count > 0);
}

#[test]
fn machine_build_task_keeps_one_work_budget_across_polls() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "cooperative-machine-build-budget.bn",
        r#"
store: [
    items:
        List/range(from: 0, to: 99)
        |> List/map(item, new: [value: item])
    ordered:
        items
        |> List/sort_by(item, key: item.value, direction: Ascending)
        |> List/take(count: 5)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let mut task = MachineInstanceBuilder::new(
        compiled.plan,
        SessionOptions {
            max_work_units_per_transaction: Some(20),
            ..SessionOptions::default()
        },
    )
    .unwrap()
    .into_build_task();
    let error = loop {
        match task.poll(3) {
            Ok(MachineBuildPoll::Pending(_)) => {}
            Ok(MachineBuildPoll::Ready(_)) => panic!("bounded startup unexpectedly completed"),
            Err(error) => break error,
        }
    };
    assert!(matches!(error, Error::WorkBudgetExceeded { limit: 20, .. }));
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
fn failed_machine_build_task_cannot_publish_on_a_later_poll() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "failed-cooperative-build.bn",
        "document: Document/new(root: Element/label(element: [], label: TEXT { static }))",
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let authority = AuthoritySnapshot {
        through_turn_sequence: 0,
        states: BTreeMap::new(),
        lists: BTreeMap::from([(
            ListId(999),
            ListAuthority {
                touched: true,
                revision: 1,
                next_key: 1,
                next_order_token: 1,
                rows: Vec::new(),
            },
        )]),
    };
    let mut task = MachineInstanceBuilder::new(compiled.plan, SessionOptions::default())
        .unwrap()
        .restore(authority)
        .into_build_task();
    let first_error = loop {
        match task.poll(1) {
            Ok(MachineBuildPoll::Pending(_)) => {}
            Ok(MachineBuildPoll::Ready(_)) => panic!("invalid restore unexpectedly published"),
            Err(error) => break error,
        }
    };
    assert!(first_error.to_string().contains("unknown list 999"));
    let repoll_error = match task.poll(1) {
        Err(error) => error,
        Ok(_) => panic!("failed build task unexpectedly accepted another poll"),
    };
    assert!(
        repoll_error
            .to_string()
            .contains("failed machine build task was polled again")
    );
}

#[test]
fn typed_ordered_index_ignores_unrelated_row_field_changes() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-index-field-dependencies.bn",
        r#"
store: [
    items: LIST {
        [name: TEXT { Beta }, note: TEXT { first }]
        [name: TEXT { Alpha }, note: TEXT { second }]
    }
    ordered:
        items
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/take(count: 2)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let source = list_id(&compiled.plan, "store.items");
    let slot = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == source)
        .unwrap();
    let name = slot
        .row_fields
        .iter()
        .find(|field| field.name == "name")
        .unwrap()
        .field_id;
    let note = slot
        .row_fields
        .iter()
        .find(|field| field.name == "note")
        .unwrap()
        .field_id;
    let index = compiled.plan.list_indexes[0].id;
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    let row = session.list_rows(source)[0];

    session
        .test_set_row_field(row, note, Value::Text("changed".to_owned()))
        .unwrap();
    let metrics = session.test_ensure_ordered_index_current(index).unwrap();
    assert_eq!(metrics.ordered_index_full_rebuild_count, 0);
    assert_eq!(metrics.ordered_index_incremental_row_count, 0);
    assert_eq!(metrics.ordered_index_key_evaluation_count, 0);

    session
        .test_set_row_field(row, name, Value::Text("Aardvark".to_owned()))
        .unwrap();
    let metrics = session.test_ensure_ordered_index_current(index).unwrap();
    assert_eq!(metrics.ordered_index_full_rebuild_count, 0);
    assert_eq!(metrics.ordered_index_incremental_row_count, 1);
    assert_eq!(metrics.ordered_index_key_evaluation_count, 1);
    assert_eq!(metrics.ordered_index_update_count, 1);
}

#[test]
fn typed_ordered_index_uses_canonical_closed_tag_ordinals() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-tag-index.bn",
        r#"
store: [
    items: LIST {
        [name: TEXT { archived }, status: Archived]
        [name: TEXT { active }, status: Active]
    }
    ordered:
        items
        |> List/sort_by(item, key: item.status, direction: Ascending)
        |> List/take(count: 2)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert_eq!(compiled.plan.list_indexes.len(), 1);
    let key = &compiled.plan.list_indexes[0].keys[0];
    assert_eq!(key.closed_tags, ["Active", "Archived"]);
    assert!(matches!(
        key.kind,
        PlanListIndexKeyKind::ClosedTag { type_id }
            if closed_tag_type_id(&key.closed_tags) == Some(type_id)
    ));
    let ordered = list_id(&compiled.plan, "store.ordered");
    let name = compiled
        .plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == ordered)
        .and_then(|slot| slot.row_fields.iter().find(|field| field.name == "name"))
        .map(|field| field.field_id)
        .unwrap();
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    let (_, metrics) = session.list_value_current_with_metrics(ordered).unwrap();
    assert_eq!(metrics.access_index_seek_count, 1);
    assert_eq!(metrics.access_full_scan_count, 0);
    let snapshot = session.snapshot().unwrap();
    assert_eq!(
        snapshot.lists[&ordered]
            .iter()
            .map(|row| row.fields[&name].clone())
            .collect::<Vec<_>>(),
        [
            Value::Text("active".to_owned()),
            Value::Text("archived".to_owned())
        ]
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
fn identical_literal_rows_keep_distinct_stable_keys_across_filtered_view_reentry() {
    let machine = compile_server_source(
        "literal-row-identity.bn",
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
    rows:
        LIST {
            [value: TEXT { same }]
            [value: TEXT { same }]
        }
        |> List/filter(item, if: visible)
        |> List/map(item, new: [value: item.value])
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan;
    let rows = machine
        .debug_map
        .list_slots
        .iter()
        .find(|entry| entry.label == "store.rows")
        .and_then(|entry| entry.id.strip_prefix("list:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(ListId)
        .expect("rows list");
    let hide = source_id(&machine, "store.hide");
    let show = source_id(&machine, "store.show");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    let initial = session.list_rows_current(rows).unwrap();
    assert_eq!(initial.len(), 2);
    assert_ne!(initial[0], initial[1]);

    session
        .apply(SourceEvent {
            sequence: 1,
            source: hide,
            route: route_token(&session, hide, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert!(session.list_rows_current(rows).unwrap().is_empty());

    session
        .apply(SourceEvent {
            sequence: 2,
            source: show,
            route: route_token(&session, show, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert_eq!(session.list_rows_current(rows).unwrap(), initial);
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

#[test]
fn deep_acyclic_dependency_chain_uses_the_default_test_thread_stack() {
    const DEPTH: usize = 4_096;
    let mut session = MachineInstance::new(
        deep_acyclic_dependency_plan(DEPTH),
        SessionOptions::default(),
    )
    .unwrap();

    assert_eq!(
        session
            .root_value_current(&format!("chain.{}", DEPTH - 1))
            .unwrap(),
        number(7)
    );
}

#[test]
fn deep_work_budget_failure_cleans_currentness_for_later_demands() {
    const DEPTH: usize = 512;
    let mut session = MachineInstance::new(
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
            session.root_value_current(&deepest),
            Err(Error::WorkBudgetExceeded { limit: 32, .. })
        ));
        assert_eq!(session.root_value_current("chain.0").unwrap(), number(7));
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
        crate::machine::runtime_value(boon_persistence::StoredValue::Variant {
            tag: "Done".to_owned(),
            fields: BTreeMap::new(),
        })
        .unwrap(),
        Value::Text("Done".to_owned())
    );

    let runtime = Value::Record(BTreeMap::from([
        ("$tag".to_owned(), Value::Text("Ready".to_owned())),
        ("count".to_owned(), number(4)),
    ]));
    let stored = crate::machine::stored_value(&runtime).unwrap();
    assert!(matches!(
        &stored,
        boon_persistence::StoredValue::Variant { tag, fields }
            if tag == "Ready" && fields["count"] == stored_number(4)
    ));
    assert_eq!(crate::machine::runtime_value(stored).unwrap(), runtime);
}

#[test]
fn host_outputs_are_demand_current_and_reconstructed_without_a_document() {
    let compiled = compile_server_source(
        "server-outputs.bn",
        include_str!("../../../examples/server_outputs.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert!(compiled.plan.document.is_none());
    let response_field = match &compiled.plan.output_root("api_response").unwrap().value {
        OutputValueRef::RuntimeValue {
            value: ValueRef::Field(field),
            ..
        } => *field,
        other => panic!("unexpected response output ref: {other:?}"),
    };
    let source = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.request_received")
        .unwrap()
        .source_id;
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    assert_eq!(
        session.output_value_current("api_response").unwrap(),
        Value::Record(BTreeMap::from([
            ("body".to_owned(), Value::Bytes(b"accepted".to_vec().into()),),
            ("status".to_owned(), number(200)),
        ]))
    );
    assert_eq!(
        session.output_value_current("pending_priorities").unwrap(),
        Value::List(vec![number(1), number(2)])
    );

    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: route_token(&session, source, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert!(
        !turn
            .metrics
            .recomputed_targets
            .contains(&ValueTarget::Field(response_field)),
        "host-demanded output must stay lazy during the source turn"
    );
    let Value::Record(response) = session.output_value_current("api_response").unwrap() else {
        panic!("response output must remain a record");
    };
    assert_eq!(response["body"], Value::Bytes(b"accepted".to_vec().into()));
}

#[test]
fn record_list_host_output_uses_compiler_owned_row_fields() {
    let compiled = compile_server_source(
        "server-record-list-output.bn",
        r#"
store: [
    rows: LIST {
        [name: TEXT { Alpha }, score: 7]
        [name: TEXT { Beta }, score: 9]
    }
]

outputs: [
    rows: store.rows
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let output = compiled.plan.output_root("rows").unwrap();
    let OutputValueRef::RuntimeValue { value, list_fields } = &output.value else {
        panic!("record list output must be a runtime value");
    };
    let ValueRef::List(list_id) = value else {
        panic!("record list output must retain its ListId");
    };
    assert_eq!(
        list_fields
            .iter()
            .map(|field| (field.list_id, field.name.as_str()))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([(*list_id, "name"), (*list_id, "score")])
    );
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    assert_eq!(
        session.output_value_current("rows").unwrap(),
        Value::List(vec![
            Value::Record(BTreeMap::from([
                ("name".to_owned(), Value::Text("Alpha".to_owned())),
                ("score".to_owned(), Value::integer(7).unwrap()),
            ])),
            Value::Record(BTreeMap::from([
                ("name".to_owned(), Value::Text("Beta".to_owned())),
                ("score".to_owned(), Value::integer(9).unwrap()),
            ])),
        ])
    );
}

#[test]
fn recursive_http_source_payload_executes_list_get_and_current_response() {
    let compiled = compile_server_source(
        "server-http-echo.bn",
        include_str!("../../../examples/server_http_echo.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
    let source = source_id(&compiled.plan, "store.request");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: route_token(&session, source, None),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([
                    ("method".to_owned(), Value::Text("GET".to_owned())),
                    (
                        "path_segments".to_owned(),
                        Value::List(vec![
                            Value::Text("health".to_owned()),
                            Value::Text("detail".to_owned()),
                        ]),
                    ),
                ]),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    assert_eq!(
        session.output_value_current("response").unwrap(),
        Value::Record(BTreeMap::from([
            (
                "body".to_owned(),
                Value::Bytes(b"GET:health".to_vec().into()),
            ),
            ("status".to_owned(), number(200)),
        ]))
    );
}

#[test]
fn recursive_http_source_payload_executes_scalar_list_find_variants() {
    let compiled = compile_server_source(
        "server-http-scalar-list-lookups.bn",
        r#"
store: [
    request: SOURCE
    found_value:
        request.method |> THEN {
            request.query
            |> List/find(
                item
                if: item.name == TEXT { q }
            )
            |> WHEN {
                Found[value] => value.value
                NotFound => TEXT { missing }
            }
        }
    found_row_name:
        request.method |> THEN {
            request.query
            |> List/find(
                item
                if: item.name == TEXT { q }
            )
            |> WHEN {
                Found[value] => value.name
                NotFound => TEXT { missing }
            }
        }
]

outputs: [
    response: [
        status: 200
        body: store.found_value
            |> Text/concat(with: store.found_row_name, separator: ":")
            |> Text/to_bytes(encoding: Utf8)
    ]
]

host_ports: [
    http: [
        request: store.request
        response: response
    ]
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
    let source = source_id(&compiled.plan, "store.request");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: route_token(&session, source, None),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([
                    ("method".to_owned(), Value::Text("GET".to_owned())),
                    (
                        "query".to_owned(),
                        Value::List(vec![
                            Value::Record(BTreeMap::from([
                                ("name".to_owned(), Value::Text("q".to_owned())),
                                ("value".to_owned(), Value::Text("answer".to_owned())),
                            ])),
                            Value::Record(BTreeMap::from([
                                ("name".to_owned(), Value::Text("other".to_owned())),
                                ("value".to_owned(), Value::Text("ignored".to_owned())),
                            ])),
                        ]),
                    ),
                ]),
                ..SourcePayload::default()
            },
        })
        .unwrap();

    let Value::Record(response) = session.output_value_current("response").unwrap() else {
        panic!("response output must be a record");
    };
    assert_eq!(response["body"], Value::Bytes(b"answer:q".to_vec().into()));
}

#[test]
fn number_to_text_then_utf8_bytes_executes_for_http_output() {
    let compiled = compile_server_source(
        "server-persistent-counter.bn",
        include_str!("../../../examples/server_persistent_counter.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
    let source = source_id(&compiled.plan, "store.request");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: route_token(&session, source, None),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([("method".to_owned(), Value::Text("POST".to_owned()))]),
                ..SourcePayload::default()
            },
        })
        .unwrap();

    assert_eq!(
        session.output_value_current("response").unwrap(),
        Value::Record(BTreeMap::from([
            ("body".to_owned(), Value::Bytes(b"1".to_vec().into())),
            ("status".to_owned(), number(200)),
        ]))
    );
}

#[test]
fn number_to_text_executes_all_bounded_waveform_formats() {
    let compiled = compile_server_source(
        "number-formats.bn",
        r#"
outputs: [
    grouped: 42 |> Number/to_text(radix: 2, min_width: 8, group_size: 4)
    signed: 255 |> Number/to_text(radix: 10, signed_width: 8)
    hexadecimal: 42 |> Number/to_text(radix: 16, prefix: True)
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    assert_eq!(
        session.output_value_current("grouped").unwrap(),
        Value::Text("0010 1010".to_owned())
    );
    assert_eq!(
        session.output_value_current("signed").unwrap(),
        Value::Text("-1".to_owned())
    );
    assert_eq!(
        session.output_value_current("hexadecimal").unwrap(),
        Value::Text("0x2a".to_owned())
    );
}

#[test]
fn fjordpulse_server_routes_http_and_keeps_search_results_structural() {
    let compiled = compile_server_path(
        std::path::Path::new("examples/fjordpulse/Server/RUN.bn"),
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert!(compiled.plan.capability_summary.cpu_plan_executor_complete);
    let source = source_id(&compiled.plan, "store.http_request");
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: route_token(&session, source, None),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([
                    ("method".to_owned(), Value::Text("GET".to_owned())),
                    ("path".to_owned(), Value::Text("/api/health".to_owned())),
                    (
                        "path_segments".to_owned(),
                        Value::List(vec![
                            Value::Text("api".to_owned()),
                            Value::Text("health".to_owned()),
                        ]),
                    ),
                    ("query".to_owned(), Value::List(Vec::new())),
                ]),
                ..SourcePayload::default()
            },
        })
        .unwrap();

    let response = session.output_value_current("http_response").unwrap();
    let Value::Record(response) = response else {
        panic!("FjordPulse HTTP output must be a record");
    };
    assert_eq!(response["status"], number(200));
    let Value::Bytes(body) = &response["body"] else {
        panic!("FjordPulse HTTP body must be application-owned bytes");
    };
    let body = std::str::from_utf8(body).unwrap();
    assert!(body.starts_with("{\"ok\":true,"));
    assert!(body.contains("\"status\":\"healthy\""));
    assert!(body.contains("\"version\":\"boon-deterministic-v1\""));

    session
        .apply(SourceEvent {
            sequence: 2,
            source,
            route: route_token(&session, source, None),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([
                    ("method".to_owned(), Value::Text("GET".to_owned())),
                    ("path".to_owned(), Value::Text("/api/search".to_owned())),
                    (
                        "path_segments".to_owned(),
                        Value::List(vec![
                            Value::Text("api".to_owned()),
                            Value::Text("search".to_owned()),
                        ]),
                    ),
                    (
                        "query".to_owned(),
                        Value::List(vec![Value::Record(BTreeMap::from([
                            ("name".to_owned(), Value::Text("q".to_owned())),
                            ("value".to_owned(), Value::Text("ber".to_owned())),
                        ]))]),
                    ),
                ]),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    let Value::Record(search) = session.output_value_current("search_contract").unwrap() else {
        panic!("FjordPulse search output must be a record");
    };
    assert_eq!(search["query"], Value::Text("ber".to_owned()));
    assert_eq!(search["indexedResultCount"], number(1));
    assert_eq!(search["limit"], number(20));
}

#[test]
fn decimal_numbers_execute_arithmetic_and_host_output_without_integer_coercion() {
    let compiled = compile_server_source(
        "real-arithmetic.bn",
        r#"
store: [
    tick: SOURCE
    half: 1 / 2
    floor: 1.9 |> Number/floor()
    ceil: -1.9 |> Number/ceil()
    round: -1.5 |> Number/round()
    truncate: -1.9 |> Number/truncate()
    latitude:
        59.91 |> HOLD latitude {
            store.tick |> THEN { latitude + 0.1 }
        }
]

outputs: [
    latitude: store.latitude
    half: store.half
    floor: store.floor
    ceil: store.ceil
    round: store.round
    truncate: store.truncate
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let source = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.tick")
        .unwrap()
        .source_id;
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    assert_eq!(
        session.output_value_current("half").unwrap(),
        Value::Number(FiniteReal::new(0.5).unwrap())
    );
    assert_eq!(session.output_value_current("floor").unwrap(), number(1));
    assert_eq!(session.output_value_current("ceil").unwrap(), number(-1));
    assert_eq!(session.output_value_current("round").unwrap(), number(-2));
    assert_eq!(
        session.output_value_current("truncate").unwrap(),
        number(-1)
    );

    let Value::Number(initial) = session.output_value_current("latitude").unwrap() else {
        panic!("decimal output must remain a real number");
    };
    assert!((initial.get() - 59.91).abs() < 1e-12);
    session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: route_token(&session, source, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    let Value::Number(updated) = session.output_value_current("latitude").unwrap() else {
        panic!("decimal arithmetic must produce a real number");
    };
    assert!((updated.get() - 60.01).abs() < 1e-12);
}

#[test]
fn whole_and_decimal_numbers_share_one_value_identity() {
    let whole = Value::Number(FiniteReal::from_i64_exact(1).unwrap());
    let decimal = Value::Number(FiniteReal::new(1.0).unwrap());
    assert_eq!(whole, decimal);
}

#[test]
fn scalar_list_literals_execute_as_immutable_values() {
    let compiled = compile_server_source(
        "scalar-list-values-executor.bn",
        r#"
store: [
    selected: TEXT { alpha }
    selected_ids: LIST { selected }
    optional_selected_ids:
        False |> WHEN {
            True => LIST {}
            False => LIST { selected }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let machine = compiled.plan;
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let selected = Value::List(vec![Value::Text("alpha".to_owned())]);

    assert_eq!(
        session.root_value_current("store.selected_ids").unwrap(),
        selected
    );
    assert_eq!(
        session
            .root_value_current("store.optional_selected_ids")
            .unwrap(),
        selected
    );
}

#[test]
fn source_payload_text_to_number_executes_the_typed_conversion() {
    let compiled = compile_server_source(
        "source-text-to-number-executor.bn",
        r#"
store: [
    input: SOURCE
    value:
        0 |> HOLD value {
            input.amount |> THEN {
                input.amount |> Text/to_number()
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let machine = compiled.plan;
    let source = source_id(&machine, "store.input");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: route_token(&session, source, None),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([("amount".to_owned(), Value::Text("42".to_owned()))]),
                ..SourcePayload::default()
            },
        })
        .unwrap();

    assert_eq!(
        session.root_value_current("store.value").unwrap(),
        number(42)
    );
}

#[test]
fn text_to_number_propagates_an_input_error_without_stringifying_it() {
    let compiled = compile_server_source(
        "text-to-number-error-propagation-executor.bn",
        r#"
store: [
    value: Error/new(code: TEXT { upstream_failure }) |> Text/to_number()
    error: Error/text(value: value)
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    assert_eq!(
        session.root_value_current("store.value").unwrap(),
        Value::Error {
            code: "upstream_failure".to_owned()
        }
    );
    assert_eq!(
        session.root_value_current("store.error").unwrap(),
        Value::Text("upstream_failure".to_owned())
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
    boon_compiler::compile_source_text_to_machine_plan_for_role(
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
    boon_compiler::compile_source_text_to_machine_plan_for_role(
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
    boon_compiler::compile_source_text_to_machine_plan_for_role(
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
    boon_compiler::compile_source_text_to_machine_plan_for_role(
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

#[test]
fn derived_empty_list_materializes_keyed_rows_without_persisting_reconstructable_structure() {
    let compiled = compile_server_source(
        "derived-list-materialization.bn",
        r#"
store: [
    reset: SOURCE
    seeds: LIST {
        [id: TEXT { first }, initial: TEXT { ready }]
    }
    rows:
        seeds
        |> List/map(item, new:
            wrap_row(row: seed_record(seed: item))
        )
]

FUNCTION seed_record(seed) {
    [id: seed.id, initial: seed.initial]
}

FUNCTION wrap_row(row) {
    stateful_row(seed: row)
}

FUNCTION stateful_row(seed) {
    [
        id: seed.id
        initial: seed.initial
        value:
            seed.initial |> HOLD value {
                store.reset |> THEN { seed.initial }
            }
    ]
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let rows = compiled
        .plan
        .debug_map
        .list_slots
        .iter()
        .find(|entry| entry.label == "store.rows")
        .and_then(|entry| entry.id.strip_prefix("list:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(ListId)
        .expect("rows list");
    let value_field = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|entry| entry.label == "store.rows.value")
        .and_then(|entry| entry.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .expect("row value field");
    assert!(
        compiled
            .plan
            .regions
            .iter()
            .flat_map(|region| &region.ops)
            .all(|op| match (&op.kind, op.output.as_ref()) {
                (
                    PlanOpKind::DerivedValue {
                        expression: Some(PlanDerivedExpression::RowExpression { expression }),
                        ..
                    },
                    Some(ValueRef::Field(output)),
                ) => !matches!(
                    compiled.plan.row_expression(*expression).unwrap(),
                    PlanRowExpressionNode::Field {
                        input: ValueRef::Field(input),
                    } if input == output
                ),
                _ => true,
            })
    );
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let materialized = session.list_value_current(rows).unwrap();
    let Value::List(materialized) = materialized else {
        panic!("derived list did not produce a list facade");
    };
    let [Value::Row { id: row, .. }] = materialized.as_slice() else {
        panic!("derived list did not materialize exactly one keyed row");
    };
    assert_eq!(row.list, rows);
    assert_eq!(
        session
            .project_current(&[ValueTarget::RowField {
                row: *row,
                field: value_field,
            }])
            .unwrap()[&ValueTarget::RowField {
            row: *row,
            field: value_field,
        }],
        Value::Text("ready".to_owned())
    );
    let semantic_image = session.semantic_value_image().unwrap();
    assert_eq!(semantic_image.lists.len(), 1);
    let mut row_field_counts = semantic_image
        .lists
        .values()
        .flat_map(|list| list.rows.iter().map(|row| row.fields.len()))
        .collect::<Vec<_>>();
    row_field_counts.sort_unstable();
    assert_eq!(row_field_counts, vec![2]);
}

fn file_stream_payload() -> SourcePayload {
    let binding = HostValueIssuer::new([3; 32]).mint([7; 32], 1).unwrap();
    SourcePayload {
        fields: BTreeMap::from([(
            "file".to_owned(),
            Value::host_bound(
                Value::Record(BTreeMap::from([(
                    "$tag".to_owned(),
                    Value::Text("FileSelected".to_owned()),
                )])),
                binding,
            ),
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
fn host_bound_projection_preserves_authority_without_exposing_the_facade() {
    let binding = HostValueIssuer::new([1; 32]).mint([2; 32], 1).unwrap();
    let value = Value::host_bound(
        Value::Record(BTreeMap::from([(
            "$tag".to_owned(),
            Value::Text("FileSelected".to_owned()),
        )])),
        binding,
    );
    let enclosing = Value::Record(BTreeMap::from([("file".to_owned(), value.clone())]));
    let tag = vec!["$tag".to_owned()];
    let file = vec!["file".to_owned()];
    let nested_tag = vec!["file".to_owned(), "$tag".to_owned()];

    assert!(value.host_binding().is_some());
    assert!(value.contains_host_binding());
    assert_eq!(crate::machine::project_value(&value, &[]), Some(&value));
    assert_eq!(crate::machine::project_value(&value, &tag), None);
    assert_eq!(
        crate::machine::project_value(&enclosing, &file),
        Some(&value)
    );
    assert_eq!(crate::machine::project_value(&enclosing, &nested_tag), None);
}

#[test]
fn inspection_reports_hide_nested_bindings_while_boundaries_fail_closed() {
    let binding = HostValueIssuer::new([4; 32]).mint([5; 32], 9).unwrap();
    let bound = Value::host_bound(
        Value::Record(BTreeMap::from([(
            "$tag".to_owned(),
            Value::Text("FileSelected".to_owned()),
        )])),
        binding,
    );
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
    let mut record = BTreeMap::from([("$tag".to_owned(), Value::Text(tag.to_owned()))]);
    record.extend(
        fields
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value)),
    );
    Value::Record(record)
}

fn retained_content_outcome(tag: &str, content: Option<Value>) -> Value {
    let mut fields = BTreeMap::from([("$tag".to_owned(), Value::Text(tag.to_owned()))]);
    if let Some(content) = content {
        fields.insert("content".to_owned(), content);
    }
    Value::Record(fields)
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
            ("method".to_owned(), Value::Text("Get".to_owned())),
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
    Value::Record(BTreeMap::from([
        ("$tag".to_owned(), Value::Text("HttpSucceeded".to_owned())),
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
    ]))
}

#[test]
fn filtered_typed_host_result_rows_receive_distinct_hidden_identities() {
    let machine = compile_server_source(
        "filtered-http-result-rows-executor.bn",
        r#"
store: [
    request: SOURCE
    hide: SOURCE
    show: SOURCE
    visible:
        True |> HOLD visible {
            LATEST {
                hide |> THEN { False }
                show |> THEN { True }
            }
        }
    response:
        NotRequested |> HOLD response {
            request |> THEN {
                Http/request(
                    endpoint: request.endpoint
                    method: request.method
                    path_segments: request.path_segments
                    query: request.query
                    headers: request.headers
                    body: request.body
                    connect_timeout_ms: request.connect_timeout_ms
                    overall_timeout_ms: request.overall_timeout_ms
                )
            }
        }
    visible_headers:
        response |> WHEN {
            HttpSucceeded =>
                response.headers
                |> List/filter(item, if: visible)
            __ => LIST {}
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan;
    let request = source_id(&machine, "store.request");
    let hide = source_id(&machine, "store.hide");
    let show = source_id(&machine, "store.show");
    let visible_headers = machine
        .debug_map
        .list_slots
        .iter()
        .find(|slot| slot.label == "store.visible_headers")
        .and_then(|slot| slot.id.strip_prefix("list:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(ListId)
        .expect("visible header list");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let request_call = session
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
    let response = |header_value: &[u8]| {
        let header = || {
            Value::Record(BTreeMap::from([
                ("name".to_owned(), Value::Text("same".to_owned())),
                (
                    "value".to_owned(),
                    Value::Bytes(header_value.to_vec().into()),
                ),
            ]))
        };
        Value::Record(BTreeMap::from([
            ("$tag".to_owned(), Value::Text("HttpSucceeded".to_owned())),
            ("endpoint".to_owned(), Value::Text("catalog".to_owned())),
            ("status".to_owned(), number(200)),
            ("headers".to_owned(), Value::List(vec![header(), header()])),
            ("body".to_owned(), Value::Bytes(Vec::new().into())),
            ("redirects_followed".to_owned(), number(0)),
        ]))
    };
    session
        .complete_transient_effect(request_call.call_id, response(b"v1"))
        .unwrap();

    let initial = session.list_rows_current(visible_headers).unwrap();
    assert_eq!(initial.len(), 2);
    assert_ne!(initial[0], initial[1]);

    session
        .apply(SourceEvent {
            sequence: 2,
            source: hide,
            route: route_token(&session, hide, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert!(
        session
            .list_rows_current(visible_headers)
            .unwrap()
            .is_empty()
    );

    session
        .apply(SourceEvent {
            sequence: 3,
            source: show,
            route: route_token(&session, show, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert_eq!(session.list_rows_current(visible_headers).unwrap(), initial);

    let replacement_call = session
        .apply(SourceEvent {
            sequence: 4,
            source: request,
            route: route_token(&session, request, None),
            target: None,
            payload: outbound_http_payload(),
        })
        .unwrap()
        .transient_effects
        .remove(0);
    session
        .complete_transient_effect(replacement_call.call_id, response(b"v2"))
        .unwrap();
    let replacement = session.list_rows_current(visible_headers).unwrap();
    assert_eq!(replacement.len(), 2);
    assert!(replacement.iter().all(|row| !initial.contains(row)));
    assert!(
        initial
            .iter()
            .all(|row| session.row_snapshot(*row).is_err())
    );
}

#[test]
fn materialized_rows_keep_scoped_sources_out_of_value_storage() {
    let machine = compile_server_source(
        "materialized-row-resource-fields-executor.bn",
        r#"
FUNCTION new_row(input) {
    [
        controls: [remove: SOURCE]
        label: input.label
    ]
}

store: [
    inputs: LIST {
        [label: TEXT { same }]
        [label: TEXT { same }]
    }
    rows:
        inputs
        |> List/map(item, new: new_row(input: item))
    forwarded:
        rows
        |> List/map(item, new: [
            controls: item.controls
            label: item.label
        ])
    filtered:
        forwarded
        |> List/filter(item, if: item.label == TEXT { same })
    consumed:
        filtered
        |> List/map(item, new: [
            controls: item.controls
            label: item.label
        ])
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan;
    let consumed = machine
        .debug_map
        .list_slots
        .iter()
        .find(|slot| slot.label == "store.consumed")
        .and_then(|slot| slot.id.strip_prefix("list:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(ListId)
        .expect("twice-forwarded materialized rows list");
    let remove = machine
        .source_routes
        .iter()
        .find(|route| route.path.ends_with(".controls.remove"))
        .map(|route| route.source_id)
        .expect("row-scoped remove source");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    let materialized = session.list_rows_current(consumed).unwrap();
    assert_eq!(materialized.len(), 2);
    assert_ne!(materialized[0], materialized[1]);
    assert_ne!(
        session
            .source_route_token_for_descendant_row(remove, materialized[0])
            .unwrap(),
        session
            .source_route_token_for_descendant_row(remove, materialized[1])
            .unwrap()
    );
}

#[test]
fn remapped_rows_use_replacement_sources_after_a_row_preserving_filter() {
    let machine = compile_server_source(
        "remapped-row-resource-fields-executor.bn",
        r#"
FUNCTION new_row(input) {
    [
        controls: [remove: SOURCE]
        label: input.label
    ]
}

store: [
    inputs: LIST {
        [label: TEXT { same }]
        [label: TEXT { other }]
    }
    rows:
        inputs
        |> List/map(item, new: new_row(input: item))
    filtered:
        rows
        |> List/filter(item, if: item.label == TEXT { same })
        |> List/map(item, new: new_row(input: item))
    selected:
        filtered
        |> List/map(item, new: item.controls.remove |> THEN { item.label })
        |> List/latest()
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan;
    let route = machine
        .source_routes
        .iter()
        .find(|route| route.path == "store.filtered.controls.remove")
        .unwrap_or_else(|| {
            panic!(
                "missing replacement source route; routes={:?}",
                machine
                    .source_routes
                    .iter()
                    .map(|route| route.path.as_str())
                    .collect::<Vec<_>>()
            )
        });
    let scope = route.scope_id.expect("replacement source is row scoped");
    let list = machine
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.scope_id == Some(scope))
        .expect("replacement source scope has list storage")
        .list_id;
    let source = route.source_id;
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let row = session.list_rows_current(list).unwrap()[0];

    session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: route_token(&session, source, Some(row)),
            target: Some(row),
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert_eq!(
        session.root_value_current("store.selected").unwrap(),
        Value::Text("same".to_owned())
    );
}

#[test]
fn nested_filtered_values_materialize_inside_distinct_outer_rows() {
    let machine = compile_server_source(
        "nested-filtered-list-authority-executor.bn",
        r#"
store: [
    inputs: LIST {
        [
            id: TEXT { one }
            values: LIST {
                [value: 1]
                [value: 2]
                [value: 3]
            }
        ]
        [
            id: TEXT { two }
            values: LIST {
                [value: 3]
            }
        ]
    }
    rows:
        inputs
        |> List/map(item, new: [
            id: item.id
            visible_values:
                item.values
                |> List/filter(item, if: item.value > 1)
                |> List/map(item, new: [label: item.value])
        ])
    segments:
        rows
        |> List/find(item, if: item.id == TEXT { one })
        |> WHEN {
            Found[value] => value.visible_values
            NotFound => LIST {}
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan;
    let rows = machine
        .debug_map
        .list_slots
        .iter()
        .find(|slot| slot.label == "store.rows")
        .and_then(|slot| slot.id.strip_prefix("list:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(ListId)
        .expect("outer rows list");
    let visible_values = machine
        .debug_map
        .fields
        .iter()
        .find(|field| field.label == "store.rows.visible_values")
        .and_then(|field| field.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .expect("nested visible values field");
    let segments = machine
        .debug_map
        .list_slots
        .iter()
        .find(|slot| slot.label == "store.segments")
        .and_then(|slot| slot.id.strip_prefix("list:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(ListId)
        .expect("projected segments list");
    let segment_label = machine
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == segments)
        .and_then(|slot| {
            slot.row_fields
                .iter()
                .find(|field| field.name == "label" && field.role.is_value())
        })
        .map(|field| field.field_id)
        .expect("projected segment label field");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let materialized = session.list_rows_current(rows).unwrap();
    assert_eq!(materialized.len(), 2);
    assert!(materialized.iter().all(|row| row.list == rows));
    let first = ValueTarget::RowField {
        row: materialized[0],
        field: visible_values,
    };
    assert_eq!(
        session.project_current(&[first]).unwrap()[&first],
        Value::List(vec![
            Value::Record(BTreeMap::from([("label".to_owned(), number(2))])),
            Value::Record(BTreeMap::from([("label".to_owned(), number(3))])),
        ])
    );
    let segment_rows = session.list_rows_current(segments).unwrap();
    assert_eq!(segment_rows.len(), 2);
    let segment_targets = segment_rows
        .iter()
        .copied()
        .map(|row| ValueTarget::RowField {
            row,
            field: segment_label,
        })
        .collect::<Vec<_>>();
    let labels = session.project_current(&segment_targets).unwrap();
    assert_eq!(labels[&segment_targets[0]], number(2));
    assert_eq!(labels[&segment_targets[1]], number(3));
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
    let wall_result = Value::Record(BTreeMap::from([
        ("$tag".to_owned(), Value::Text("WallClockRead".to_owned())),
        ("unix_seconds".to_owned(), number(1_700_000_000)),
        ("nanoseconds".to_owned(), number(123)),
    ]));
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
                Value::Record(BTreeMap::from([
                    (
                        "$tag".to_owned(),
                        Value::Text("RandomBytesReady".to_owned()),
                    ),
                    (
                        "bytes".to_owned(),
                        Value::Bytes(vec![sequence as u8; 16].into()),
                    ),
                ])),
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
            Value::Record(BTreeMap::from([
                (
                    "$tag".to_owned(),
                    Value::Text("HostServiceFailed".to_owned()),
                ),
                ("code".to_owned(), Value::Text("clock_failed".to_owned())),
                (
                    "diagnostic".to_owned(),
                    Value::Text("clock unavailable".to_owned()),
                ),
            ])),
        )
        .unwrap();
    assert!(failure.transient_effects.is_empty());
}

#[test]
fn record_equality_piped_into_when_gates_the_effect_on_the_boolean_result() {
    let compiled = compile_server_source(
        "record-equality-effect-gate.bn",
        r#"
store: [
    start: SOURCE
    left: [value: 1]
    right: [value: 1]
    result:
        RandomNotRequested |> HOLD result {
            start |> THEN {
                left
                == right
                |> WHEN {
                    True => Random/bytes(byte_count: 1)
                    False => SKIP
                }
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let machine = compiled.plan;
    let start = source_id(&machine, "store.start");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source: start,
            route: route_token(&session, start, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();

    assert_eq!(
        turn.transient_effects.len(),
        1,
        "equal records must activate the True arm before effect staging"
    );
}

#[test]
fn value_when_over_a_held_effect_result_tracks_continuous_branch_dependencies() {
    let compiled = compile_server_source(
        "held-effect-result-value-when.bn",
        r#"
store: [
    start: SOURCE
    change_suffix: SOURCE
    effect_result:
        NotRequested |> HOLD effect_result {
            start |> THEN { Clock/wall() }
        }
    suffix:
        TEXT { first } |> HOLD suffix {
            change_suffix |> THEN { TEXT { second } }
        }
    label:
        effect_result |> WHEN {
            __ => suffix
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let machine = compiled.plan;
    let start = source_id(&machine, "store.start");
    let change_suffix = source_id(&machine, "store.change_suffix");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    let started = session
        .apply(SourceEvent {
            sequence: 1,
            source: start,
            route: route_token(&session, start, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    let [clock] = started.transient_effects.as_slice() else {
        panic!("start must invoke the clock effect: {started:#?}");
    };
    session
        .complete_transient_effect(
            clock.call_id,
            Value::Record(BTreeMap::from([
                ("$tag".to_owned(), Value::Text("WallClockRead".to_owned())),
                ("unix_seconds".to_owned(), number(1_700_000_000)),
                ("nanoseconds".to_owned(), number(123)),
            ])),
        )
        .unwrap();
    assert_eq!(
        session.root_value_current("store.label").unwrap(),
        Value::Text("first".to_owned())
    );

    session
        .apply(SourceEvent {
            sequence: 2,
            source: change_suffix,
            route: route_token(&session, change_suffix, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert_eq!(
        session.root_value_current("store.label").unwrap(),
        Value::Text("second".to_owned()),
        "value-style WHEN must remain current when its selected branch changes"
    );
}

#[test]
fn derived_when_event_updates_state_despite_later_list_map_branches() {
    let machine = compile_server_source(
        "derived-when-with-list-map-executor.bn",
        r#"
store: [
    start: SOURCE
    reset: SOURCE
    seed_rows: LIST { [key: TEXT { row }] }
    rows:
        seed_rows |> List/map(item, new: selectable_row(seed_row: item))
    clock_result:
        ClockNotRequested |> HOLD clock_result {
            start |> THEN { Clock/wall() }
        }
    projected:
        clock_result |> WHEN {
            WallClockRead => TEXT { canonical }
            __ => TEXT { none }
        }
    direct_active:
        TEXT { fallback } |> HOLD direct_active {
            projected
        }
    active:
        TEXT { fallback } |> HOLD active {
            LATEST {
                projected |> WHEN {
                    TEXT { none } => SKIP
                    __ => projected
                }
                reset |> THEN { TEXT { fallback } }
                rows
                    |> List/map(item, new: item.select |> THEN { item.key })
                    |> List/latest()
            }
        }
]

FUNCTION selectable_row(seed_row) {
    [key: seed_row.key, select: SOURCE]
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan;
    let start = source_id(&machine, "store.start");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let clock = session
        .apply(SourceEvent {
            sequence: 1,
            source: start,
            route: route_token(&session, start, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap()
        .transient_effects
        .remove(0);

    session
        .complete_transient_effect(
            clock.call_id,
            Value::Record(BTreeMap::from([
                ("$tag".to_owned(), Value::Text("WallClockRead".to_owned())),
                ("unix_seconds".to_owned(), number(1_700_000_000)),
                ("nanoseconds".to_owned(), number(123)),
            ])),
        )
        .unwrap();

    assert_eq!(
        session.root_value_current("store.active").unwrap(),
        Value::Text("canonical".to_owned())
    );
    assert_eq!(
        session.root_value_current("store.direct_active").unwrap(),
        Value::Text("canonical".to_owned())
    );

    let reset = source_id(session.plan(), "store.reset");
    let reset_turn = session
        .apply(SourceEvent {
            sequence: 2,
            source: reset,
            route: route_token(&session, reset, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert_eq!(
        session.root_value_current("store.active").unwrap(),
        Value::Text("fallback".to_owned())
    );
    assert_eq!(
        reset_turn.durable_changes.len(),
        1,
        "the generated LATEST event state must not enter durable storage"
    );
}

#[test]
fn effects_sample_state_after_same_trigger_updates_settle() {
    let compiled = compile_server_source(
        "effect-samples-post-update-state.bn",
        r#"
store: [
    start: SOURCE
    reset: SOURCE
    clock_result:
        ClockNotRequested |> HOLD clock_result {
            start |> THEN { Clock/wall() }
        }
    requested_size:
        LATEST {
            clock_result |> WHEN {
                WallClockRead => active_size
                __ => SKIP
            }
            reset |> THEN { 2 }
        }
    random_result:
        RandomNotRead |> HOLD random_result {
            requested_size |> THEN { Random/bytes(byte_count: requested_size) }
        }
    active_size:
        1 |> HOLD active_size {
            clock_result |> WHEN {
                WallClockRead => 7
                __ => SKIP
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let machine = compiled.plan;
    let start = source_id(&machine, "store.start");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let clock = session
        .apply(SourceEvent {
            sequence: 1,
            source: start,
            route: route_token(&session, start, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap()
        .transient_effects
        .remove(0);

    let completed = session
        .complete_transient_effect(
            clock.call_id,
            Value::Record(BTreeMap::from([
                ("$tag".to_owned(), Value::Text("WallClockRead".to_owned())),
                ("unix_seconds".to_owned(), number(1_700_000_000)),
                ("nanoseconds".to_owned(), number(123)),
            ])),
        )
        .unwrap();

    assert_eq!(
        session.root_value_current("store.active_size").unwrap(),
        number(7)
    );
    assert_eq!(completed.transient_effects.len(), 1);
    assert_eq!(
        completed.transient_effects[0].intent,
        Value::Record(BTreeMap::from([("byte_count".to_owned(), number(7))]))
    );
}

#[test]
fn replaced_host_result_list_is_current_before_downstream_effect_reconciliation() {
    let compiled = compile_server_source(
        "replaced-host-result-list-currentness.bn",
        r#"
store: [
    load: SOURCE
    repeat: SOURCE
    hierarchy_result:
        NotStarted |> HOLD hierarchy_result {
            load |> THEN {
                Wellen/hierarchy_page(
                    artifact: load.artifact
                    request_fingerprint: TEXT { hierarchy }
                    offset: 0
                    limit: 8
                )
            }
        }
    first_signal:
        hierarchy_result |> WHEN {
            HierarchyPage =>
                hierarchy_result.rows
                |> List/find(item, if: item.kind == TEXT { Signal })
                |> WHEN {
                    Found[value] => value.signal_id
                    NotFound => TEXT { none }
                }
            __ => TEXT { none }
        }
    first_scope:
        hierarchy_result |> WHEN {
            HierarchyPage =>
                hierarchy_result.rows
                |> List/find(item, if: item.kind == TEXT { Signal })
                |> WHEN {
                    Found[value] => value.parent_id
                    NotFound => TEXT { none }
                }
            __ => TEXT { none }
        }
    signal_ids:
        hierarchy_result |> WHEN {
            HierarchyPage =>
                hierarchy_result.signal_ids
                |> List/find(item, if: item == active_signal)
                |> WHEN {
                    Found[value] => LIST { value }
                    NotFound => first_signal == TEXT { none } |> WHEN {
                        True => LIST {}
                        False => LIST { first_signal }
                    }
                }
            __ => LIST {}
        }
    signal_request_fingerprint:
        hierarchy_result |> WHEN {
            HierarchyPage =>
                active_scope
                |> Text/concat(with: active_signal, separator: "|")
            __ => TEXT { none }
        }
    signal_request:
        LATEST {
            hierarchy_result |> WHEN {
                HierarchyPage => signal_request_fingerprint
                __ => SKIP
            }
            repeat |> THEN { signal_request_fingerprint }
        }
    signal_result:
        NotStarted |> HOLD signal_result {
            signal_request |> THEN {
                hierarchy_result |> WHEN {
                    HierarchyPage => signal_ids |> List/is_not_empty() |> WHEN {
                        True => Wellen/signal_page(
                            artifact: hierarchy_result.artifact
                            request_fingerprint: signal_request_fingerprint
                            signal_ids: signal_ids
                            start_time: 0
                            end_time: 10
                            offset: 0
                            max_transitions: 8
                        )
                        False => SKIP
                    }
                    __ => SKIP
                }
            }
        }
    active_signal:
        TEXT { none } |> HOLD active_signal {
            hierarchy_result |> WHEN {
                HierarchyPage => first_signal |> WHEN {
                    TEXT { none } => SKIP
                    __ => first_signal
                }
                __ => SKIP
            }
        }
    active_scope:
        TEXT { none } |> HOLD active_scope {
            hierarchy_result |> WHEN {
                HierarchyPage => first_scope |> WHEN {
                    TEXT { none } => SKIP
                    __ => first_scope
                }
                __ => SKIP
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let machine = compiled.plan;
    let load = source_id(&machine, "store.load");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    assert_eq!(
        session.root_value_current("store.signal_request").unwrap(),
        Value::Text("SKIP".to_owned()),
        "an inactive LATEST arm must not pre-arm its downstream effect"
    );

    let artifact = |seed: u8, format: &str| {
        Value::Record(BTreeMap::from([
            (
                "content".to_owned(),
                Value::Record(BTreeMap::from([
                    ("digest".to_owned(), Value::Bytes(vec![seed; 32].into())),
                    (
                        "media".to_owned(),
                        Value::Text(format!(
                            "application/vnd.boon.waveform/{}",
                            format.to_ascii_lowercase()
                        )),
                    ),
                    ("size".to_owned(), number(1)),
                ])),
            ),
            ("format".to_owned(), Value::Text(format.to_owned())),
            ("parser_version".to_owned(), Value::Text("test".to_owned())),
            (
                "schema_version".to_owned(),
                Value::Text("wellen.v1".to_owned()),
            ),
        ]))
    };
    let hierarchy_outcome = |artifact: Value, signal_id: &str| {
        let signal_row = Value::Record(BTreeMap::from([
            ("kind".to_owned(), Value::Text("Signal".to_owned())),
            ("id".to_owned(), Value::Text(format!("signal:{signal_id}"))),
            ("parent_id".to_owned(), Value::Text("scope:top".to_owned())),
            ("name".to_owned(), Value::Text(signal_id.to_owned())),
            ("full_name".to_owned(), Value::Text(signal_id.to_owned())),
            ("signal_id".to_owned(), Value::Text(signal_id.to_owned())),
            ("width".to_owned(), number(1)),
            ("encoding".to_owned(), Value::Text("Bits".to_owned())),
        ]));
        file_stream_outcome(
            "HierarchyPage",
            [
                ("artifact", artifact),
                ("request_fingerprint", Value::Text("hierarchy".to_owned())),
                ("start_time", number(0)),
                ("end_time", number(10)),
                ("offset", number(0)),
                ("has_more", Value::Bool(false)),
                ("next_offset", number(1)),
                ("total_rows", number(1)),
                (
                    "signal_ids",
                    Value::List(vec![Value::Text(signal_id.to_owned())]),
                ),
                ("rows", Value::List(vec![signal_row])),
            ],
        )
    };
    let load_event = |session: &MachineInstance, sequence, artifact| SourceEvent {
        sequence,
        source: load,
        route: route_token(session, load, None),
        target: None,
        payload: SourcePayload {
            fields: BTreeMap::from([("artifact".to_owned(), artifact)]),
            ..SourcePayload::default()
        },
    };

    let first_artifact = artifact(1, "VCD");
    let first_hierarchy = session
        .apply(load_event(&session, 1, first_artifact.clone()))
        .unwrap()
        .transient_effects
        .remove(0);
    let first_signal_turn = session
        .complete_transient_effect(
            first_hierarchy.call_id,
            hierarchy_outcome(first_artifact, "top.vcd_signal"),
        )
        .unwrap();
    let [first_signal] = first_signal_turn.transient_effects.as_slice() else {
        panic!("first hierarchy result must request one signal page: {first_signal_turn:#?}");
    };

    let second_artifact = artifact(2, "GHW");
    let second_hierarchy = session
        .apply(load_event(&session, 2, second_artifact.clone()))
        .unwrap()
        .transient_effects
        .remove(0);
    let second_signal_turn = session
        .complete_transient_effect(
            second_hierarchy.call_id,
            hierarchy_outcome(second_artifact.clone(), "top.ghw_signal"),
        )
        .unwrap();
    let [second_signal] = second_signal_turn.transient_effects.as_slice() else {
        panic!(
            "replacement hierarchy must request one current signal page: {second_signal_turn:#?}"
        );
    };
    assert_eq!(
        second_signal_turn.cancelled_transient_effects,
        vec![first_signal.call_id]
    );
    let Value::Record(intent) = &second_signal.intent else {
        panic!(
            "signal page intent is not a record: {:?}",
            second_signal.intent
        );
    };
    assert_eq!(intent.get("artifact"), Some(&second_artifact));
    assert_eq!(
        intent.get("signal_ids"),
        Some(&Value::List(vec![Value::Text("top.ghw_signal".to_owned())]))
    );
    assert_eq!(
        session.root_value_current("store.active_signal").unwrap(),
        Value::Text("top.ghw_signal".to_owned())
    );
}

#[test]
fn trigger_specialized_effect_arms_share_one_result_lane_without_eager_invocation() {
    let compiled = compile_server_source(
        "trigger-specialized-effect-runtime.bn",
        r#"
store: [
    start: SOURCE
    move: SOURCE
    clock_result:
        NotRequested |> HOLD clock_result {
            start |> THEN { Clock/wall() }
        }
    random_result:
        NotRequested |> HOLD random_result {
            LATEST {
                clock_result |> WHEN {
                    WallClockRead => Random/bytes(byte_count: 4)
                    __ => SKIP
                }
                move |> THEN {
                    clock_result |> WHEN {
                        WallClockRead => Random/bytes(byte_count: 8)
                        __ => SKIP
                    }
                }
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let machine = compiled.plan;
    let start = source_id(&machine, "store.start");
    let move_source = source_id(&machine, "store.move");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    assert_eq!(session.pending_transient_effect_count(), 0);

    let started = session
        .apply(SourceEvent {
            sequence: 1,
            source: start,
            route: route_token(&session, start, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    let [clock] = started.transient_effects.as_slice() else {
        panic!("start must emit exactly one clock request: {started:#?}");
    };
    let first = session
        .complete_transient_effect(
            clock.call_id,
            Value::Record(BTreeMap::from([
                ("$tag".to_owned(), Value::Text("WallClockRead".to_owned())),
                ("unix_seconds".to_owned(), number(1_700_000_000)),
                ("nanoseconds".to_owned(), number(123)),
            ])),
        )
        .unwrap();
    let [first_random] = first.transient_effects.as_slice() else {
        panic!("clock completion must emit one four-byte request: {first:#?}");
    };
    assert_eq!(
        first_random.intent,
        Value::Record(BTreeMap::from([("byte_count".to_owned(), number(4))]))
    );
    let first_random_call = first_random.call_id;
    session
        .complete_transient_effect(
            first_random_call,
            Value::Record(BTreeMap::from([
                (
                    "$tag".to_owned(),
                    Value::Text("RandomBytesReady".to_owned()),
                ),
                ("bytes".to_owned(), Value::Bytes(vec![1; 4].into())),
            ])),
        )
        .unwrap();

    let moved = session
        .apply(SourceEvent {
            sequence: 2,
            source: move_source,
            route: route_token(&session, move_source, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    let [second_random] = moved.transient_effects.as_slice() else {
        panic!("move must emit one eight-byte request: {moved:#?}");
    };
    assert_ne!(second_random.call_id, first_random_call);
    assert_eq!(
        second_random.intent,
        Value::Record(BTreeMap::from([("byte_count".to_owned(), number(8))]))
    );
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
        Value::Record(fields)
            if fields.get("$tag") == Some(&Value::Text("Opened".to_owned()))
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
fn inactive_while_branch_cancels_its_owned_transient_effect() {
    let compiled = compile_server_source(
        "while-owned-stream.bn",
        r#"
store: [
    start: SOURCE
    stop: SOURCE
    mode:
        Inactive |> HOLD mode {
            LATEST {
                start |> THEN { Active }
                stop |> THEN { Inactive }
            }
        }
    selected_file: PackageAsset[url: TEXT { asset://fixture/large.vcd }]
    stream_result:
        NotStarted |> HOLD stream_result {
            mode |> WHILE {
                Active => File/read_stream(
                    file: selected_file
                    chunk_bytes: 65536
                    retain_content: True
                )
                Inactive => SKIP
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let machine = compiled.plan;
    let start = source_id(&machine, "store.start");
    let stop = source_id(&machine, "store.stop");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    let started = session
        .apply(SourceEvent {
            sequence: 1,
            source: start,
            route: route_token(&session, start, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    let [invocation] = started.transient_effects.as_slice() else {
        panic!("activating WHILE must start exactly one stream: {started:#?}");
    };
    let call_id = invocation.call_id;

    let stopped = session
        .apply(SourceEvent {
            sequence: 2,
            source: stop,
            route: route_token(&session, stop, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert!(stopped.transient_effects.is_empty());
    assert_eq!(stopped.cancelled_transient_effects, vec![call_id]);
    assert_eq!(session.pending_transient_effect_count(), 0);
}

#[test]
fn live_effect_argument_replacement_restarts_the_same_owned_invocation() {
    let compiled = compile_server_source(
        "reactive-effect-argument-replacement.bn",
        r#"
store: [
    start: SOURCE
    stop: SOURCE
    choose_primary: SOURCE
    choose_secondary: SOURCE
    mode:
        Inactive |> HOLD mode {
            LATEST {
                start |> THEN { Active }
                stop |> THEN { Inactive }
            }
        }
    primary_file: PackageAsset[url: TEXT { asset://files/primary.vcd }]
    secondary_file: PackageAsset[url: TEXT { asset://files/secondary.fst }]
    selected_file:
        primary_file |> HOLD selected_file {
            LATEST {
                choose_primary |> THEN { primary_file }
                choose_secondary |> THEN { secondary_file }
            }
        }
    stream_result:
        NotStarted |> HOLD stream_result {
            mode |> WHILE {
                Active => File/read_stream(
                    file: selected_file
                    chunk_bytes: 65536
                    retain_content: True
                )
                Inactive => SKIP
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let machine = compiled.plan;
    let start = source_id(&machine, "store.start");
    let stop = source_id(&machine, "store.stop");
    let choose_primary = source_id(&machine, "store.choose_primary");
    let choose_secondary = source_id(&machine, "store.choose_secondary");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    let file_url = |intent: &Value| {
        let Value::Record(intent) = intent else {
            panic!("effect intent is not a record: {intent:?}");
        };
        let Some(Value::Record(file)) = intent.get("file") else {
            panic!("effect intent has no file record: {intent:?}");
        };
        let Some(Value::Text(url)) = file.get("url") else {
            panic!("effect file has no URL: {file:?}");
        };
        url.clone()
    };

    let started = session
        .apply(SourceEvent {
            sequence: 1,
            source: start,
            route: route_token(&session, start, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    let [first] = started.transient_effects.as_slice() else {
        panic!("activating the live branch must start one stream: {started:#?}");
    };
    assert_eq!(file_url(&first.intent), "asset://files/primary.vcd");

    let replaced = session
        .apply(SourceEvent {
            sequence: 2,
            source: choose_secondary,
            route: route_token(&session, choose_secondary, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    let [second] = replaced.transient_effects.as_slice() else {
        panic!("changing a live effect argument must start one replacement: {replaced:#?}");
    };
    assert_eq!(replaced.cancelled_transient_effects, vec![first.call_id]);
    assert_eq!(file_url(&second.intent), "asset://files/secondary.fst");
    assert_ne!(second.call_id, first.call_id);
    assert_eq!(session.pending_transient_effect_count(), 1);
    assert!(
        session
            .deliver_transient_effect_result(
                first.call_id,
                0,
                file_stream_outcome("Cancelled", []),
            )
            .is_err(),
        "a replaced stream must reject stale completion"
    );

    session
        .deliver_transient_effect_result(second.call_id, 0, file_stream_outcome("Cancelled", []))
        .unwrap();
    assert_eq!(session.pending_transient_effect_count(), 0);

    let restarted = session
        .apply(SourceEvent {
            sequence: 3,
            source: choose_primary,
            route: route_token(&session, choose_primary, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    let [third] = restarted.transient_effects.as_slice() else {
        panic!(
            "a completed effect remains owned by its live expression and must restart on argument replacement: {restarted:#?}"
        );
    };
    assert_eq!(file_url(&third.intent), "asset://files/primary.vcd");

    let stopped = session
        .apply(SourceEvent {
            sequence: 4,
            source: stop,
            route: route_token(&session, stop, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert!(stopped.transient_effects.is_empty());
    assert_eq!(stopped.cancelled_transient_effects, vec![third.call_id]);
    assert_eq!(session.pending_transient_effect_count(), 0);
}

#[test]
fn source_transform_updates_live_effect_argument_before_reconciliation() {
    let compiled = compile_server_source(
        "source-transform-effect-argument-replacement.bn",
        r#"
store: [
    choose_primary: SOURCE
    choose_secondary: SOURCE
    primary_file: PackageAsset[url: TEXT { asset://files/primary.vcd }]
    secondary_file: PackageAsset[url: TEXT { asset://files/secondary.fst }]
    file_request:
        LATEST {
            choose_primary |> THEN { TEXT { primary } }
            choose_secondary |> THEN { TEXT { secondary } }
        }
    selected_file:
        primary_file |> HOLD selected_file {
            file_request |> WHEN {
                TEXT { primary } => primary_file
                TEXT { secondary } => secondary_file
                __ => SKIP
            }
        }
    mode:
        Inactive |> HOLD mode {
            LATEST {
                choose_primary |> THEN { Active }
                choose_secondary |> THEN { Active }
            }
        }
    stream_result:
        NotStarted |> HOLD stream_result {
            mode |> WHILE {
                Active => File/read_stream(
                    file: selected_file
                    chunk_bytes: 65536
                    retain_content: True
                )
                Inactive => SKIP
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let machine = compiled.plan;
    let choose_primary = source_id(&machine, "store.choose_primary");
    let choose_secondary = source_id(&machine, "store.choose_secondary");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    let file_url = |intent: &Value| {
        let Value::Record(intent) = intent else {
            panic!("effect intent is not a record: {intent:?}");
        };
        let Some(Value::Record(file)) = intent.get("file") else {
            panic!("effect intent has no file record: {intent:?}");
        };
        let Some(Value::Text(url)) = file.get("url") else {
            panic!("effect file has no URL: {file:?}");
        };
        url.clone()
    };

    let primary = session
        .apply(SourceEvent {
            sequence: 1,
            source: choose_primary,
            route: route_token(&session, choose_primary, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    let [first] = primary.transient_effects.as_slice() else {
        panic!("primary selection must start one stream: {primary:#?}");
    };
    assert_eq!(file_url(&first.intent), "asset://files/primary.vcd");

    let secondary = session
        .apply(SourceEvent {
            sequence: 2,
            source: choose_secondary,
            route: route_token(&session, choose_secondary, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    let [second] = secondary.transient_effects.as_slice() else {
        panic!("secondary selection must replace the stream: {secondary:#?}");
    };
    assert_eq!(secondary.cancelled_transient_effects, vec![first.call_id]);
    assert_eq!(file_url(&second.intent), "asset://files/secondary.fst");
    assert_ne!(second.call_id, first.call_id);
}

#[test]
fn mapped_row_selection_enters_derived_while_owned_effect() {
    let compiled = compile_server_source(
        "mapped-derived-while-owned-stream.bn",
        r#"
store: [
    mode:
        Inactive |> HOLD mode {
            selected |> WHEN {
                TEXT { two } => Active
                __ => Inactive
            }
        }
    selected_file: PackageAsset[url: TEXT { asset://fixture/large.vcd }]
    stream_result:
        NotStarted |> HOLD stream_result {
            mode |> WHILE {
                Active => File/read_stream(
                    file: selected_file
                    chunk_bytes: 65536
                    retain_content: True
                )
                Inactive => SKIP
            }
        }
    rows:
        LIST {
            [name: TEXT { one }]
            [name: TEXT { two }]
        }
        |> List/map(item, new: new_row(row: item))
    fallback: SOURCE
    row_selected:
        rows
        |> List/map(item, new:
            item.controls.select.event.press |> THEN { item.name }
        )
        |> List/latest()
    selected:
        LATEST {
            row_selected
            fallback.text
        }
]

FUNCTION new_row(row) {
    [
        controls: [select: SOURCE]
        name: row.name
    ]
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let machine = compiled.plan;
    let route = machine
        .source_routes
        .iter()
        .find(|route| route.path.ends_with(".controls.select"))
        .expect("mapped row exposes select");
    let scope = route.scope_id.expect("mapped row source has a list scope");
    let list = machine
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.scope_id == Some(scope))
        .expect("mapped row source scope has list storage")
        .list_id;
    let source = route.source_id;
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let row = session.list_rows_current(list).unwrap()[1];

    let selected = session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: route_token(&session, source, Some(row)),
            target: Some(row),
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert_eq!(
        session.root_value_current("store.mode").unwrap(),
        Value::Text("Active".to_owned())
    );
    assert_eq!(
        selected.transient_effects.len(),
        1,
        "a row-derived mode change must enter its WHILE-owned effect in the same turn"
    );
}

#[test]
fn field_equality_guard_blocks_stale_host_effect_invocation() {
    let compiled = compile_server_source(
        "field-equality-effect-guard.bn",
        r#"
store: [
    start: SOURCE
    replace_request: SOURCE
    request_fingerprint:
        TEXT { current } |> HOLD request_fingerprint {
            replace_request.text
        }
    response_fingerprint: TEXT { current }
    random_result:
        RandomNotRequested |> HOLD random_result {
            start |> THEN {
                request_fingerprint == response_fingerprint |> WHEN {
                    True => Random/bytes(byte_count: 1)
                    False => SKIP
                }
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let machine = compiled.plan;
    let start = source_id(&machine, "store.start");
    let replace_request = source_id(&machine, "store.replace_request");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    let first = session
        .apply(SourceEvent {
            sequence: 1,
            source: start,
            route: route_token(&session, start, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap()
        .transient_effects
        .remove(0);
    session
        .complete_transient_effect(
            first.call_id,
            Value::Record(BTreeMap::from([
                (
                    "$tag".to_owned(),
                    Value::Text("RandomBytesReady".to_owned()),
                ),
                ("bytes".to_owned(), Value::Bytes(vec![7].into())),
            ])),
        )
        .unwrap();

    session
        .apply(SourceEvent {
            sequence: 2,
            source: replace_request,
            route: route_token(&session, replace_request, None),
            target: None,
            payload: SourcePayload {
                text: Some("stale".to_owned()),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    let stale = session
        .apply(SourceEvent {
            sequence: 3,
            source: start,
            route: route_token(&session, start, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert!(stale.transient_effects.is_empty());
    assert_eq!(session.pending_transient_effect_count(), 0);
}

#[test]
fn scalar_list_nonempty_guard_blocks_empty_host_effect_invocation() {
    let compiled = compile_server_source(
        "scalar-list-nonempty-effect-guard.bn",
        r#"
store: [
    start: SOURCE
    empty_ids:
        False |> WHEN {
            True => LIST { TEXT { top.clk } }
            False => LIST {}
        }
    nonempty_ids: LIST { TEXT { top.clk } }
    empty_result:
        RandomNotRequested |> HOLD empty_result {
            start |> THEN {
                empty_ids |> List/is_not_empty() |> WHEN {
                    True => Random/bytes(byte_count: 1)
                    False => SKIP
                }
            }
        }
    nonempty_result:
        RandomNotRequested |> HOLD nonempty_result {
            start |> THEN {
                nonempty_ids |> List/is_not_empty() |> WHEN {
                    True => Random/bytes(byte_count: 1)
                    False => SKIP
                }
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    let machine = compiled.plan;
    let start = source_id(&machine, "store.start");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source: start,
            route: route_token(&session, start, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();

    assert_eq!(turn.transient_effects.len(), 1);
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
        Value::Record(fields)
            if fields.get("$tag") == Some(&Value::Text("Opened".to_owned()))
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

#[test]
fn root_stateful_function_calls_keep_distinct_runtime_owners() {
    let compiled = compile_server_source(
        "root-stateful-function-owners.bn",
        r#"
FUNCTION local_resource(initial, updated) {
    [
        change: SOURCE
        current:
            initial |> HOLD current {
                change |> THEN { updated }
            }
    ]
}

left: local_resource(initial: TEXT { left }, updated: TEXT { changed-left })
right: local_resource(initial: TEXT { right }, updated: TEXT { changed-right })
"#,
        TargetProfile::SoftwareDefault,
    )
    .expect("stateful function calls must compile as ordinary owned graph instances");

    let left_source = source_id(&compiled.plan, "left.change");
    let right_source = source_id(&compiled.plan, "right.change");
    let left_state = state_id(&compiled.plan, "left.current");
    let right_state = state_id(&compiled.plan, "right.current");
    let left_route = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.source_id == left_source)
        .expect("left source route");
    let right_route = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.source_id == right_source)
        .expect("right source route");
    assert_ne!(
        left_route.owner.static_owner,
        right_route.owner.static_owner
    );
    let left_slot = compiled
        .plan
        .storage_layout
        .scalar_slots
        .iter()
        .find(|slot| slot.state_id == left_state)
        .expect("left state slot");
    let right_slot = compiled
        .plan
        .storage_layout
        .scalar_slots
        .iter()
        .find(|slot| slot.state_id == right_state)
        .expect("right state slot");
    assert_eq!(left_slot.owner.static_owner, left_route.owner.static_owner);
    assert_eq!(
        right_slot.owner.static_owner,
        right_route.owner.static_owner
    );

    let mut machine = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    machine
        .apply(SourceEvent {
            sequence: 1,
            source: left_source,
            route: route_token(&machine, left_source, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    let current = machine
        .project_current(&[
            ValueTarget::State(left_state),
            ValueTarget::State(right_state),
        ])
        .unwrap();
    assert_eq!(
        current[&ValueTarget::State(left_state)],
        Value::Text("changed-left".to_owned())
    );
    assert_eq!(
        current[&ValueTarget::State(right_state)],
        Value::Text("right".to_owned())
    );
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

fn list_id(machine: &MachinePlan, label: &str) -> ListId {
    let id = &machine
        .debug_map
        .list_slots
        .iter()
        .find(|entry| entry.label == label)
        .unwrap_or_else(|| panic!("missing list debug label `{label}`"))
        .id;
    ListId(id.strip_prefix("list:").unwrap().parse().unwrap())
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
    boon_persistence::StoredValue::Variant {
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
    let boon_persistence::StoredValue::Record(intent) = &pending.intent else {
        panic!("effect intent must be a durable record");
    };
    assert_eq!(
        intent["simulation"],
        boon_persistence::StoredValue::Variant {
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
                        boon_persistence::StoredValue::Bool(true),
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
                    ("retryable", boon_persistence::StoredValue::Bool(true)),
                ],
            ),
            "RegistrationFailed",
            Some(("store.failure_retryable", Value::Bool(true))),
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
            Value::Text(expected_tag.to_owned())
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
        Value::Text("RegistrationCancelled".to_owned())
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

#[test]
fn reconciliation_completion_routes_result_after_session_restart() {
    let machine = typed_passkey_effect_machine();
    let (mut session, dispatching) = apply_register_effect(&machine, 1);
    let turn = session.require_effect_reconciliation(&dispatching).unwrap();
    let [
        boon_persistence::DurableOutboxChange::RequireReconciliation {
            item_id,
            expected_revision,
            next_revision,
            attempt,
            turn_sequence,
        },
    ] = turn.outbox_changes.as_slice()
    else {
        panic!("expected one reconciliation change");
    };
    assert_eq!(*item_id, dispatching.item_id);
    assert_eq!(*expected_revision, dispatching.revision);
    let mut reconciling = dispatching;
    reconciling.revision = *next_revision;
    reconciling.updated_turn_sequence = *turn_sequence;
    reconciling.state =
        boon_persistence::DurableOutboxState::ReconciliationRequired { attempt: *attempt };

    let authority = session.authority_snapshot().unwrap();
    let mut restored = MachineInstanceBuilder::new(machine.clone(), SessionOptions::default())
        .unwrap()
        .restore(authority)
        .build()
        .unwrap();
    let outcome = result_variant(
        "RegistrationSucceeded",
        [
            (
                "account_id",
                boon_persistence::StoredValue::Text("restored-account".to_owned()),
            ),
            (
                "credential_id",
                boon_persistence::StoredValue::Text("restored-credential".to_owned()),
            ),
            (
                "label",
                boon_persistence::StoredValue::Text("Restored".to_owned()),
            ),
            (
                "workspace_grant_bound",
                boon_persistence::StoredValue::Bool(true),
            ),
        ],
    );
    let completion = restored.complete_effect(&reconciling, outcome).unwrap();
    assert!(matches!(
        completion.outbox_changes.as_slice(),
        [boon_persistence::DurableOutboxChange::Complete {
            item_id,
            expected_revision: 2,
            next_revision: 3,
            attempt: 1,
            ..
        }] if *item_id == reconciling.item_id
    ));
    let snapshot = restored.snapshot().unwrap();
    assert_eq!(
        snapshot.states[&state_id(&machine, "store.last_result")],
        Value::Text("RegistrationSucceeded".to_owned())
    );
    assert_eq!(
        snapshot.states[&state_id(&machine, "store.result_account_id")],
        Value::Text("restored-account".to_owned())
    );
}

#[test]
fn generated_plan_executes_nested_text_is_empty_match_updates() {
    let machine = compile_server_source(
        "nested-text-empty-update.bn",
        r#"
store: [
    pulse: SOURCE
    ownership: AnonymousGrant |> HOLD ownership
    grant:
        TEXT { } |> HOLD grant {
            pulse |> THEN {
                ownership == AccountOwned |> WHEN {
                    True => TEXT { }
                    False => Text/is_empty(input: grant) |> WHEN {
                        True => TEXT { generated-grant }
                        False => grant
                    }
                }
            }
        }
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan;
    let pulse = source_id(&machine, "store.pulse");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    for sequence in 1..=2 {
        session
            .apply(SourceEvent {
                sequence,
                source: pulse,
                route: route_token(&session, pulse, None),
                target: None,
                payload: SourcePayload::default(),
            })
            .unwrap();
        assert_eq!(
            session.root_value_current("store.grant").unwrap(),
            Value::Text("generated-grant".to_owned())
        );
    }
}

#[test]
fn generated_plan_restores_bare_root_latest_as_its_only_scalar_authority() {
    let machine = compile_server_source(
        "root-latest-memory-executor.bn",
        r#"
store: [
    pulse: SOURCE
    count:
        LATEST {
            0
            pulse |> THEN { count + 1 }
        }
    transient:
        pulse |> THEN { count + 10 }
    derived: count + 20
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan;
    let pulse = source_id(&machine, "store.pulse");
    let count = state_id(&machine, "store.count");
    let mut session = MachineInstance::new(machine.clone(), SessionOptions::default()).unwrap();

    session
        .apply(SourceEvent {
            sequence: 1,
            source: pulse,
            route: route_token(&session, pulse, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    let authority = session.authority_snapshot().unwrap();
    assert_eq!(authority.states.len(), 1);
    assert_eq!(authority.states[&count].value, number(1));
    assert!(authority.states[&count].touched);

    let mut restored = MachineInstanceBuilder::new(machine, SessionOptions::default())
        .unwrap()
        .restore(authority)
        .build()
        .unwrap();
    assert_eq!(
        restored.root_value_current("store.count").unwrap(),
        number(1)
    );
}

#[test]
fn list_append_reads_pre_turn_authority_and_skips_duplicate_candidate() {
    let machine = compile_server_source(
        "unique-append-executor.bn",
        r#"
store: [
    add: SOURCE
    candidate:
        add |> THEN {
            entries
            |> List/any(item, if:
                item.id == add.text
            )
            |> WHEN {
                True => SKIP
                False => [
                    id: add.text
                ]
            }
        }
    entries:
        LIST {}
        |> List/append(item: candidate)
        |> List/map(item, new: entry_view(entry: item))
]

FUNCTION entry_view(entry) {
    [
        id: entry.id
    ]
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan;
    assert!(machine.persistence.memory.is_empty());
    assert_eq!(machine.persistence.lists.len(), 1);
    let add = source_id(&machine, "store.add");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let event = |machine: &MachineInstance, sequence, text: &str| SourceEvent {
        sequence,
        source: add,
        route: route_token(machine, add, None),
        target: None,
        payload: SourcePayload {
            text: Some(text.to_owned()),
            ..SourcePayload::default()
        },
    };

    let first = session.apply(event(&session, 1, "alpha")).unwrap();
    assert!(first.authority_deltas.iter().any(|delta| matches!(
        delta,
        AuthorityDelta::ReplaceList { .. } | AuthorityDelta::InsertRow { .. }
    )));
    let duplicate = session.apply(event(&session, 2, "alpha")).unwrap();
    assert!(duplicate.authority_deltas.iter().all(|delta| !matches!(
        delta,
        AuthorityDelta::ReplaceList { .. } | AuthorityDelta::InsertRow { .. }
    )));
    assert_eq!(
        session
            .authority_snapshot()
            .unwrap()
            .lists
            .values()
            .next()
            .unwrap()
            .rows
            .len(),
        1
    );

    session.apply(event(&session, 3, "beta")).unwrap();
    assert_eq!(
        session
            .authority_snapshot()
            .unwrap()
            .lists
            .values()
            .next()
            .unwrap()
            .rows
            .len(),
        2
    );
}

#[test]
fn false_source_payload_is_present_and_drives_list_append() {
    let machine = compile_server_source(
        "false-state-transition-list-append.bn",
        r#"
store: [
    add: SOURCE
    candidate:
        add.value |> THEN {
            [value: False]
        }
    entries:
        LIST {}
        |> List/append(item: candidate)
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan;
    let add = source_id(&machine, "store.add");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    session
        .apply(SourceEvent {
            sequence: 1,
            source: add,
            route: route_token(&session, add, None),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([("value".to_owned(), Value::Bool(false))]),
                ..SourcePayload::default()
            },
        })
        .unwrap();

    let authority = session.authority_snapshot().unwrap();
    let rows = &authority
        .lists
        .values()
        .next()
        .expect("entries authority")
        .rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].fields.values().next(),
        Some(&Value::Bool(false)),
        "THEN is gated by event presence, not by the transitioned value's truthiness"
    );
}

#[test]
fn identical_list_append_sites_are_not_deduplicated() {
    let machine = compile_server_source(
        "identical-list-append-sites.bn",
        r#"
store: [
    add: SOURCE
    candidate:
        add |> THEN {
            [value: TEXT { alpha }]
        }
    entries:
        LIST {}
        |> List/append(item: candidate)
        |> List/append(item: candidate)
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan;
    let add = source_id(&machine, "store.add");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    session
        .apply(SourceEvent {
            sequence: 1,
            source: add,
            route: route_token(&session, add, None),
            target: None,
            payload: SourcePayload {
                text: Some("alpha".to_owned()),
                ..SourcePayload::default()
            },
        })
        .unwrap();

    let authority = session.authority_snapshot().unwrap();
    let rows = &authority
        .lists
        .values()
        .next()
        .expect("entries authority")
        .rows;
    assert_eq!(rows.len(), 2, "each executable append site must run once");
    assert!(
        rows.iter()
            .all(|row| { row.fields.values().next() == Some(&Value::Text("alpha".to_owned())) })
    );
}

#[test]
fn same_event_list_mutations_evaluate_against_one_pre_turn_snapshot() {
    let machine = compile_server_source(
        "same-event-list-mutation-snapshot.bn",
        r#"
store: [
    replace: SOURCE
    replacement:
        replace |> THEN {
            [value: TEXT { new }]
        }
    entries:
        LIST {
            [value: TEXT { old }]
        }
        |> List/append(item: replacement)
        |> List/remove(item, when:
            replace |> THEN { True }
        )
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan;
    let replace = source_id(&machine, "store.replace");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    session
        .apply(SourceEvent {
            sequence: 1,
            source: replace,
            route: route_token(&session, replace, None),
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();

    let authority = session.authority_snapshot().unwrap();
    let rows = &authority
        .lists
        .values()
        .next()
        .expect("entries authority")
        .rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].fields.values().next(),
        Some(&Value::Text("new".to_owned())),
        "remove must see the old list, not the append committed earlier in the turn"
    );
}

#[test]
fn list_append_record_transform_reads_current_source_payload_fields() {
    let machine = compile_server_source(
        "append-source-payload-fields-executor.bn",
        r#"
store: [
    completed: SOURCE
    append_token:
        completed |> THEN { completed.digest }
    revisions:
        LIST {}
        |> List/append(item: append_token |> THEN {
            [
                digest: append_token
                compiler: completed.compiler
                target: completed.target
            ]
        })
        |> List/map(item, new: revision_view(revision: item))
]

FUNCTION revision_view(revision) {
    [
        digest: revision.digest
        compiler: revision.compiler
        target: revision.target
    ]
}
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan;
    let completed = source_id(&machine, "store.completed");
    let append = machine
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .find_map(|op| match &op.kind {
            PlanOpKind::ListMutation {
                mutation: PlanListMutation::Append(append),
            } => Some(append),
            _ => None,
        })
        .expect("append descriptor");
    let field_id = |name: &str| {
        append
            .fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.field_id)
            .expect("append field id")
    };
    let digest = field_id("digest");
    let compiler = field_id("compiler");
    let target = field_id("target");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    session
        .apply(SourceEvent {
            sequence: 1,
            source: completed,
            route: route_token(&session, completed, None),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([
                    ("digest".to_owned(), Value::Text("sha-123".to_owned())),
                    ("compiler".to_owned(), Value::Text("boon-test".to_owned())),
                    ("target".to_owned(), Value::Text("software".to_owned())),
                ]),
                ..SourcePayload::default()
            },
        })
        .unwrap();

    let authority = session.authority_snapshot().unwrap();
    let row = authority
        .lists
        .values()
        .next()
        .and_then(|list| list.rows.first())
        .expect("persisted revision row");
    assert_eq!(row.fields[&digest], Value::Text("sha-123".to_owned()));
    assert_eq!(row.fields[&compiler], Value::Text("boon-test".to_owned()));
    assert_eq!(row.fields[&target], Value::Text("software".to_owned()));
}

const SESSION_INFO_SOURCE: &str = r#"
outputs: [
    status: SessionInfo/status()
    principal: SessionInfo/principal()
]
"#;

fn session_info_plan() -> MachinePlan {
    boon_compiler::compile_source_text_to_machine_plan_for_role(
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
        Value::Text("Current".to_owned())
    );
    assert_eq!(
        session.output_value_current("principal").unwrap(),
        Value::Text("Anonymous".to_owned())
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
        Value::Text("Connecting".to_owned())
    );
    assert_eq!(
        session.output_value_current("principal").unwrap(),
        Value::Record(BTreeMap::from([
            ("$tag".to_owned(), Value::Text("Authenticated".to_owned()),),
            ("subject".to_owned(), Value::Text("person-42".to_owned()),),
            (
                "roles".to_owned(),
                Value::List(vec![
                    Value::Text("operator".to_owned()),
                    Value::Text("viewer".to_owned()),
                ]),
            ),
        ]))
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
        Value::Record(BTreeMap::from([
            ("$tag".to_owned(), Value::Text("Failed".to_owned())),
            (
                "code".to_owned(),
                Value::Text("transport_timeout".to_owned()),
            ),
        ]))
    );
    assert_eq!(
        session.output_value_current("principal").unwrap(),
        Value::Text("Anonymous".to_owned())
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

fn remote_not_current() -> Value {
    Value::Error {
        code: "remote_not_current".to_owned(),
    }
}

fn session_scope_unavailable() -> Value {
    Value::Error {
        code: "session_scope_unavailable".to_owned(),
    }
}

fn authenticated_principal_value(subject: &str, roles: &[&str]) -> Value {
    Value::Record(BTreeMap::from([
        ("$tag".to_owned(), Value::Text("Authenticated".to_owned())),
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
    ]))
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
    let expected = distributed_context_value(
        Value::Text("Current".to_owned()),
        Value::Record(BTreeMap::from([
            ("$tag".to_owned(), Value::Text("Authenticated".to_owned())),
            (
                "roles".to_owned(),
                Value::List(vec![
                    Value::Text("operator".to_owned()),
                    Value::Text("viewer".to_owned()),
                ]),
            ),
            ("subject".to_owned(), Value::Text("person-42".to_owned())),
        ])),
        number(11),
        number(22),
        remote_not_current(),
    );

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
    assert_eq!(
        turn.deltas
            .iter()
            .filter_map(|delta| match delta {
                Delta::SetValue {
                    target: ValueTarget::Field(FieldId(0)),
                    value,
                } => Some(value.clone()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec![expected.clone()]
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
    assert_eq!(
        session
            .root_value_current("store.distributed_context")
            .unwrap(),
        expected
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
            Value::Text("Stale".to_owned()),
            Value::Text("Anonymous".to_owned()),
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
    assert_eq!(
        session
            .root_value_current("store.distributed_context")
            .unwrap(),
        distributed_context_value(
            Value::Text("Current".to_owned()),
            authenticated_principal_value("origin-a", &["viewer"]),
            number(44),
            number(22),
            remote_not_current(),
        )
    );

    update_atomic_call_result(&mut session, fixture.call_site_id, 1, number(55));
    assert_eq!(
        session
            .root_value_current("store.distributed_context")
            .unwrap(),
        distributed_context_value(
            Value::Text("Current".to_owned()),
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

    let expected = distributed_context_value(
        Value::Text("Current".to_owned()),
        authenticated_principal_value("origin-b", &["operator"]),
        remote_not_current(),
        number(222),
        remote_not_current(),
    );
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
            .filter(|delta| matches!(delta, Delta::SetDistributedImport { .. }))
            .count(),
        2
    );
    assert_eq!(
        session.distributed_import_value_current(fixture.first_import_id),
        Ok(remote_not_current())
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
    assert_eq!(
        session
            .root_value_current("store.distributed_context")
            .unwrap(),
        expected
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
    assert_eq!(
        session
            .root_value_current("store.distributed_context")
            .unwrap(),
        distributed_context_value(
            session_scope_unavailable(),
            session_scope_unavailable(),
            remote_not_current(),
            remote_not_current(),
            remote_not_current(),
        )
    );
    for import_id in [fixture.first_import_id, fixture.second_import_id] {
        assert_eq!(
            session.distributed_import_value_current(import_id),
            Ok(remote_not_current())
        );
        assert_eq!(session.distributed_import_revision(import_id), None);
    }
}

#[test]
fn distributed_context_replacement_rejects_a_batch_without_exposing_its_valid_prefix() {
    let fixture = atomic_distributed_context_fixture();
    let mut session = MachineInstance::new(fixture.plan, SessionOptions::default()).unwrap();
    let initial = distributed_context_value(
        Value::Text("Current".to_owned()),
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
        Value::Text("Current".to_owned()),
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
    assert_eq!(
        session.root_value_current("store.remote_count").unwrap(),
        Value::Error {
            code: "remote_not_current".to_owned(),
        }
    );
    assert_eq!(
        session
            .distributed_export_value_current(fixture.value_export_id)
            .unwrap(),
        Value::Error {
            code: "remote_not_current".to_owned(),
        }
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
    assert!(matches!(
        current_rows[1].fields.get(&result_field),
        Some(Value::Error { code }) if code == "remote_not_current"
    ));

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
        let initial = session
            .evaluate_distributed_function_instance(
                outer_call.call_site_id,
                instance,
                outer_call.function_export_id,
                1,
                BTreeMap::from([(outer_argument, number(value))]),
            )
            .unwrap();
        assert!(matches!(initial, Value::Error { code } if code == "remote_not_current"));
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
    assert!(matches!(
        session
            .distributed_producer_call_result_current(outer_call.call_site_id, second_outer)
            .unwrap(),
        Value::Error { code } if code == "remote_not_current"
    ));

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
            expression: Some(PlanDerivedExpression::MaterializeList {
                target_list: ListId(1),
                authority_source_list: None,
                fields: BTreeMap::from([("seed".to_owned(), FieldId(20))]),
                row_field_copies: Vec::new(),
                value_list_authorities: Vec::new(),
                expression: Box::new(PlanDerivedExpression::RowExpression { expression: map }),
            }),
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
        assert_ne!(captured, &Value::Null);
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
