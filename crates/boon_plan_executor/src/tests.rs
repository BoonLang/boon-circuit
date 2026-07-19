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

const INDEXED_PREFIX_QUERY_SOURCE: &str = r#"
store: [
    change: SOURCE
    prefix:
        TEXT { al } |> HOLD prefix {
            change.text |> THEN { change.text }
        }
    catalog: LIST {
        [id: TEXT { 1 }, name: TEXT { Alpha }]
        [id: TEXT { 2 }, name: TEXT { Alpine }]
        [id: TEXT { 3 }, name: TEXT { Beta }]
    }
    results:
        List/query_prefix(
            catalog
            field: name
            prefix: prefix
            limit: 20
            normalization: TrimLowercase
        )
]
"#;

const GENERIC_COMPOUND_QUERY_SOURCE: &str = r#"
store: [
    catalog: LIST {
        [id: TEXT { 1 }, city: TEXT { Oslo }, name: TEXT { Alpha }, score: 10, modes: TEXT { rail bus }]
        [id: TEXT { 2 }, city: TEXT { Oslo }, name: TEXT { Beta }, score: 20, modes: TEXT { rail }]
        [id: TEXT { 3 }, city: TEXT { Bergen }, name: TEXT { Alpha }, score: 30, modes: TEXT { bus }]
    }
    exact_key: [city: TEXT { OSLO }, name: TEXT { alpha }]
    exact_page:
        List/query(
            catalog
            fields: TEXT { city,name }
            normalization: TEXT { TrimLowercase,TrimLowercase }
            select: Exact
            key: exact_key
            limit: 2
            order: Ascending
            residual: None
        )
    mode_keys: [first: TEXT { rail }, second: TEXT { bus }]
    union_page:
        List/query(
            catalog
            fields: TEXT { modes }
            normalization: TEXT { Tokens }
            select: Union
            keys: mode_keys
            limit: 10
            order: Ascending
            residual: None
        )
    intersection_page:
        List/query(
            catalog
            fields: TEXT { modes }
            normalization: TEXT { Tokens }
            select: Intersection
            keys: mode_keys
            limit: 10
            order: Ascending
            residual: None
        )
]
"#;

const GENERIC_QUERY_MUTATION_SOURCE: &str = r#"
store: [
    add: SOURCE
    value_to_add:
        add.text |> THEN { add.text }
    catalog:
        LIST {}
        |> List/append(item: value_to_add |> THEN {
            [id: value_to_add, name: value_to_add]
        })
    prefix: TEXT { al }
    page:
        List/query(
            catalog
            fields: TEXT { name }
            normalization: TEXT { TrimLowercase }
            select: Prefix
            prefix: prefix
            limit: 10
            order: Ascending
            residual: None
        )
]
"#;

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

#[allow(clippy::too_many_arguments)]
fn plan(
    demand: RootOutputDemand,
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
                .row_field_ids
                .iter()
                .map(|field| {
                    MemoryLeafPlan::new(
                        memory_id,
                        Some(*field),
                        field_label_map.get(field).copied().unwrap_or("field"),
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
    MachinePlan {
        version: PlanVersion::default(),
        target_profile: TargetProfile::SoftwareDefault,
        program_role: ProgramRole::Client,
        distributed_endpoint: None,
        application,
        persistence,
        effects: Vec::new(),
        outputs: Vec::new(),
        host_ports: Vec::new(),
        query_collections: Vec::new(),
        query_indexes: Vec::new(),
        demand: DemandPlan {
            root_derived_outputs: demand,
        },
        document: None,
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
    }
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
        PlanValueType::RootInitialField
        | PlanValueType::RowInitialField
        | PlanValueType::Unknown => DataTypePlan::Unknown,
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
        value_type: PlanValueType::Number,
        scope_id: None,
        indexed: false,
        initial_value_kind: InitialValueKind::Number,
        initial_constant_id: Some(PlanConstantId(constant)),
        initial_root_field_path: None,
        initial_row_field_path: None,
        initial_expression: None,
    }
}

fn route(source: usize, scope: Option<usize>) -> SourceRoute {
    SourceRoute {
        id: PlanSourceRouteId(source),
        source_id: SourceId(source),
        path: format!("source.{source}"),
        scoped: scope.is_some(),
        scope_id: scope.map(ScopeId),
        interval_ms: None,
        payload_schema: SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
            row_lookup_field: None,
            row_lookup_field_id: None,
        },
    }
}

fn derived(
    id: usize,
    output: usize,
    inputs: Vec<ValueRef>,
    expression: Option<PlanRowExpression>,
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

fn const_update(id: usize, source: usize, state: usize, constant: usize) -> PlanOp {
    PlanOp {
        id: PlanOpId(id),
        kind: PlanOpKind::UpdateBranch {
            trigger: ValueRef::Source(SourceId(source)),
            expression_kind: PlanExpressionKind::Const,
            ordered_inputs: Vec::new(),
            source_payload_field: None,
            update_constant_id: Some(PlanConstantId(constant)),
            source_guard: None,
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(source))],
        output: Some(ValueRef::State(StateId(state))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    }
}

fn event(sequence: u64, source: usize, target: Option<RowId>) -> SourceEvent {
    SourceEvent {
        sequence,
        source: SourceId(source),
        target,
        payload: SourcePayload::default(),
    }
}

#[test]
fn root_value_comparison_tracks_both_state_inputs() {
    let comparison = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::Pure,
            startup_recompute: true,
            expression: Some(PlanDerivedExpression::ValueCompare {
                left: ValueRef::State(StateId(0)),
                op: "==".to_owned(),
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
            vec![
                constant(0, number_constant(3)),
                constant(1, number_constant(3)),
                constant(2, number_constant(4)),
            ],
            vec![route(0, None)],
            vec![number_slot(0, 0), number_slot(1, 1)],
            Vec::new(),
            vec![comparison, const_update(1, 0, 1, 2)],
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
    session.apply(event(1, 0, None)).unwrap();
    assert_eq!(
        session.root_value_current("store.same").unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn fully_qualified_state_lookup_wins_over_an_unrelated_field_local_name() {
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
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
                Some(PlanRowExpression::Constant {
                    constant_id: PlanConstantId(1),
                }),
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
        plan(
            RootOutputDemand::Selected(Vec::new()),
            vec![
                constant(0, number_constant(default)),
                constant(1, number_constant(0)),
            ],
            vec![route(0, None)],
            vec![number_slot(0, 0)],
            Vec::new(),
            vec![const_update(0, 0, 0, 1)],
            vec![(StateId(0), "count")],
            Vec::new(),
            Vec::new(),
        )
    };

    let untouched = MachineInstance::new(make_plan(0), SessionOptions::default()).unwrap();
    let semantic_default = untouched.semantic_value_image().unwrap();
    let mut original = MachineInstance::new(make_plan(0), SessionOptions::default()).unwrap();
    let turn = original.apply(event(1, 0, None)).unwrap();
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
    let machine = plan(
        RootOutputDemand::Selected(Vec::new()),
        vec![
            constant(0, number_constant(1)),
            constant(1, number_constant(2)),
        ],
        vec![route(0, None)],
        vec![number_slot(0, 0), number_slot(1, 0)],
        Vec::new(),
        vec![const_update(0, 0, 0, 1), const_update(1, 0, 1, 99)],
        vec![(StateId(0), "first"), (StateId(1), "second")],
        Vec::new(),
        Vec::new(),
    );
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let before = session.authority_snapshot().unwrap();

    assert!(session.apply(event(1, 0, None)).is_err());
    assert_eq!(session.authority_snapshot().unwrap(), before);
}

#[test]
fn unsettled_turn_can_rollback_authority_sequence_and_durable_delta() {
    let machine = plan(
        RootOutputDemand::Selected(Vec::new()),
        vec![
            constant(0, number_constant(1)),
            constant(1, number_constant(2)),
        ],
        vec![route(0, None)],
        vec![number_slot(0, 0)],
        Vec::new(),
        vec![const_update(0, 0, 0, 1)],
        vec![(StateId(0), "count")],
        Vec::new(),
        Vec::new(),
    );
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let before = session.authority_snapshot().unwrap();

    let turn = session.apply(event(1, 0, None)).unwrap();
    assert_eq!(turn.durable_changes.len(), 1);
    assert_eq!(
        session.authority_snapshot().unwrap().through_turn_sequence,
        1
    );

    session.rollback_unsettled_turn().unwrap();
    assert_eq!(session.authority_snapshot().unwrap(), before);

    let retried = session.apply(event(1, 0, None)).unwrap();
    assert_eq!(retried.durable_changes, turn.durable_changes);
    session.settle_turn();
    assert_eq!(
        session.authority_snapshot().unwrap().through_turn_sequence,
        1
    );
}

#[test]
fn field_ids_keep_same_named_list_fields_distinct() {
    let list = |id, field, value: &str| ListStorageSlot {
        id: PlanStorageId(id),
        list_id: ListId(id),
        scope_id: None,
        row_field_ids: vec![FieldId(field)],
        capacity: None,
        hidden_key_type: "Key".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![PlanInitialListField {
                name: "value".to_owned(),
                field_id: Some(FieldId(field)),
                value: PlanConstantValue::Text {
                    value: value.to_owned(),
                },
            }],
        }],
    };
    let expression = PlanRowExpression::TextConcat {
        parts: vec![
            PlanRowExpression::ListFindValue {
                list_id: ListId(0),
                field: FieldId(10),
                value: Box::new(PlanRowExpression::Constant {
                    constant_id: PlanConstantId(0),
                }),
                target: FieldId(10),
                fallback: None,
            },
            PlanRowExpression::ListFindValue {
                list_id: ListId(1),
                field: FieldId(20),
                value: Box::new(PlanRowExpression::Constant {
                    constant_id: PlanConstantId(1),
                }),
                target: FieldId(20),
                fallback: None,
            },
        ],
    };
    let session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            vec![
                constant(0, PlanConstantValue::Text { value: "A".into() }),
                constant(1, PlanConstantValue::Text { value: "B".into() }),
            ],
            Vec::new(),
            Vec::new(),
            vec![list(0, 10, "A"), list(1, 20, "B")],
            vec![derived(
                0,
                30,
                vec![ValueRef::List(ListId(0)), ValueRef::List(ListId(1))],
                Some(expression),
            )],
            Vec::new(),
            vec![(ListId(0), "left"), (ListId(1), "right")],
            vec![
                (FieldId(10), "left.value"),
                (FieldId(20), "right.value"),
                (FieldId(30), "joined"),
            ],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    assert_eq!(
        session.snapshot().unwrap().fields[&FieldId(30)],
        Value::Text("AB".into())
    );
}

#[test]
fn text_filter_uses_empty_scope_only_for_empty_queries() {
    let row = |name: &str, family: &str, scope: &str| PlanInitialListRow {
        fields: vec![
            PlanInitialListField {
                name: "name".into(),
                field_id: Some(FieldId(10)),
                value: PlanConstantValue::Text { value: name.into() },
            },
            PlanInitialListField {
                name: "family".into(),
                field_id: Some(FieldId(11)),
                value: PlanConstantValue::Text {
                    value: family.into(),
                },
            },
            PlanInitialListField {
                name: "scope".into(),
                field_id: Some(FieldId(12)),
                value: PlanConstantValue::Text {
                    value: scope.into(),
                },
            },
        ],
    };
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: None,
        row_field_ids: vec![FieldId(10), FieldId(11), FieldId(12)],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![
            row("tx_data", "uart", "top.uart"),
            row("rx_data", "uart", "top.uart"),
            row("counter", "ghw", "ghw.simple"),
        ],
    };
    let filter = |needle: usize| PlanRowExpression::BuiltinCall {
        function: "List/filter_text_contains".into(),
        input: Some(Box::new(PlanRowExpression::ListRef { list_id: ListId(0) })),
        args: [
            ("field", 0),
            ("needle", needle),
            ("prefer_field", 1),
            ("empty_field", 2),
            ("empty_value", 4),
        ]
        .into_iter()
        .map(|(name, constant_id)| PlanRowCallArg {
            name: Some(name.into()),
            value: PlanRowExpression::Constant {
                constant_id: PlanConstantId(constant_id),
            },
        })
        .collect(),
    };
    let session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            ["name", "family", "scope", "tx", "top.uart", ""]
                .into_iter()
                .enumerate()
                .map(|(id, value)| {
                    constant(
                        id,
                        PlanConstantValue::Text {
                            value: value.into(),
                        },
                    )
                })
                .collect(),
            Vec::new(),
            Vec::new(),
            vec![list],
            vec![
                derived(0, 20, vec![ValueRef::List(ListId(0))], Some(filter(3))),
                derived(1, 21, vec![ValueRef::List(ListId(0))], Some(filter(5))),
            ],
            Vec::new(),
            vec![(ListId(0), "signals")],
            vec![
                (FieldId(10), "signals.name"),
                (FieldId(11), "signals.family"),
                (FieldId(12), "signals.scope"),
                (FieldId(20), "matching"),
                (FieldId(21), "in_scope"),
            ],
        ),
        SessionOptions::default(),
    )
    .unwrap();
    let snapshot = session.snapshot().unwrap();

    assert!(matches!(snapshot.fields[&FieldId(20)], Value::List(ref rows) if rows.len() == 1));
    assert!(matches!(snapshot.fields[&FieldId(21)], Value::List(ref rows) if rows.len() == 2));
}

#[test]
fn list_any_evaluates_bound_row_predicates() {
    let row = |selected: bool| PlanInitialListRow {
        fields: vec![PlanInitialListField {
            name: "selected".into(),
            field_id: Some(FieldId(10)),
            value: PlanConstantValue::Bool { value: selected },
        }],
    };
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: None,
        row_field_ids: vec![FieldId(10)],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![row(false), row(true)],
    };
    let expression = PlanRowExpression::BuiltinCall {
        function: "List/any".into(),
        input: Some(Box::new(PlanRowExpression::ListRef { list_id: ListId(0) })),
        args: vec![
            PlanRowCallArg {
                name: Some("binding".into()),
                value: PlanRowExpression::Constant {
                    constant_id: PlanConstantId(0),
                },
            },
            PlanRowCallArg {
                name: Some("if".into()),
                value: PlanRowExpression::ObjectField {
                    object: Box::new(PlanRowExpression::ListMapItem {
                        binding: "item".into(),
                    }),
                    field: "selected".into(),
                },
            },
        ],
    };
    let session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            vec![constant(
                0,
                PlanConstantValue::Text {
                    value: "item".into(),
                },
            )],
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
fn dynamic_row_dependencies_invalidate_consumers_across_lists() {
    let source_rows = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_field_ids: vec![FieldId(10), FieldId(11), FieldId(12)],
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
                    value: PlanConstantValue::Text {
                        value: "candidate".into(),
                    },
                },
                PlanInitialListField {
                    name: "initial".into(),
                    field_id: Some(FieldId(12)),
                    value: PlanConstantValue::Bool { value: false },
                },
            ],
        }],
    };
    let projected_rows = ListStorageSlot {
        id: PlanStorageId(1),
        list_id: ListId(1),
        scope_id: None,
        row_field_ids: vec![FieldId(20), FieldId(21)],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![PlanInitialListField {
                name: "id".into(),
                field_id: Some(FieldId(20)),
                value: PlanConstantValue::Text {
                    value: "projected".into(),
                },
            }],
        }],
    };
    let selected_state = ScalarStorageSlot {
        id: PlanStorageId(2),
        state_id: StateId(0),
        value_type: PlanValueType::Bool,
        scope_id: Some(ScopeId(0)),
        indexed: true,
        initial_value_kind: InitialValueKind::RowInitialField,
        initial_constant_id: None,
        initial_root_field_path: None,
        initial_row_field_path: Some("source.initial".into()),
        initial_expression: None,
    };
    let select_route = SourceRoute {
        id: PlanSourceRouteId(0),
        source_id: SourceId(0),
        path: "source.select".into(),
        scoped: true,
        scope_id: Some(ScopeId(0)),
        interval_ms: None,
        payload_schema: SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
            row_lookup_field: Some("key".into()),
            row_lookup_field_id: Some(FieldId(10)),
        },
    };
    let select_update = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::UpdateBranch {
            trigger: ValueRef::Source(SourceId(0)),
            expression_kind: PlanExpressionKind::Const,
            ordered_inputs: Vec::new(),
            source_payload_field: None,
            update_constant_id: Some(PlanConstantId(0)),
            source_guard: None,
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0))],
        output: Some(ValueRef::State(StateId(0))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let projected_selected = PlanOp {
        id: PlanOpId(1),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::Pure,
            startup_recompute: true,
            expression: Some(PlanDerivedExpression::RowExpression {
                expression: PlanRowExpression::BuiltinCall {
                    function: "List/any".into(),
                    input: Some(Box::new(PlanRowExpression::ListRef { list_id: ListId(0) })),
                    args: vec![
                        PlanRowCallArg {
                            name: Some("binding".into()),
                            value: PlanRowExpression::Constant {
                                constant_id: PlanConstantId(1),
                            },
                        },
                        PlanRowCallArg {
                            name: Some("if".into()),
                            value: PlanRowExpression::ObjectField {
                                object: Box::new(PlanRowExpression::ListMapItem {
                                    binding: "source".into(),
                                }),
                                field: "selected".into(),
                            },
                        },
                    ],
                },
            }),
        },
        inputs: vec![ValueRef::List(ListId(0))],
        output: Some(ValueRef::Field(FieldId(21))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let visible_rows = derived(
        2,
        30,
        vec![ValueRef::List(ListId(1))],
        Some(PlanRowExpression::BuiltinCall {
            function: "List/filter_field_equal".into(),
            input: Some(Box::new(PlanRowExpression::ListRef { list_id: ListId(1) })),
            args: vec![
                PlanRowCallArg {
                    name: Some("field".into()),
                    value: PlanRowExpression::Constant {
                        constant_id: PlanConstantId(2),
                    },
                },
                PlanRowCallArg {
                    name: Some("value".into()),
                    value: PlanRowExpression::Constant {
                        constant_id: PlanConstantId(0),
                    },
                },
            ],
        }),
    );
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            vec![
                constant(0, PlanConstantValue::Bool { value: true }),
                constant(
                    1,
                    PlanConstantValue::Text {
                        value: "source".into(),
                    },
                ),
                constant(
                    2,
                    PlanConstantValue::Text {
                        value: "selected".into(),
                    },
                ),
            ],
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
        ),
        SessionOptions::default(),
    )
    .unwrap();

    assert!(matches!(
        session.snapshot().unwrap().fields[&FieldId(30)],
        Value::List(ref rows) if rows.is_empty()
    ));

    session
        .apply(event(
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
fn mapped_range_initializes_synthetic_input_columns() {
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: None,
        row_field_ids: vec![FieldId(10), FieldId(11)],
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
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![list],
            Vec::new(),
            Vec::new(),
            vec![(ListId(0), "items")],
            vec![
                (FieldId(10), "items.$input$index"),
                (FieldId(11), "items.$input$value"),
            ],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    let rows = &session.snapshot().unwrap().lists[&ListId(0)];
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].fields[&FieldId(10)], Value::Text("3".into()));
    assert_eq!(rows[0].fields[&FieldId(11)], Value::Text("3".into()));
    assert_eq!(rows[1].fields[&FieldId(10)], Value::Text("4".into()));
    assert_eq!(rows[1].fields[&FieldId(11)], Value::Text("4".into()));
}

#[test]
fn unscoped_source_updates_every_row_owned_by_indexed_state() {
    let row = |id: &str| PlanInitialListRow {
        fields: vec![
            PlanInitialListField {
                name: "id".into(),
                field_id: Some(FieldId(10)),
                value: PlanConstantValue::Text { value: id.into() },
            },
            PlanInitialListField {
                name: "initial".into(),
                field_id: Some(FieldId(12)),
                value: PlanConstantValue::Enum {
                    value: "Hexadecimal".into(),
                },
            },
        ],
    };
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_field_ids: vec![FieldId(10), FieldId(11), FieldId(12)],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![row("active"), row("other")],
    };
    let indexed_state = ScalarStorageSlot {
        id: PlanStorageId(1),
        state_id: StateId(0),
        value_type: PlanValueType::Enum,
        scope_id: Some(ScopeId(0)),
        indexed: true,
        initial_value_kind: InitialValueKind::RowInitialField,
        initial_constant_id: None,
        initial_root_field_path: None,
        initial_row_field_path: Some("items.initial".into()),
        initial_expression: None,
    };
    let update = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::UpdateBranch {
            trigger: ValueRef::Source(SourceId(0)),
            expression_kind: PlanExpressionKind::MatchInfixConst,
            ordered_inputs: vec![
                ValueRef::Field(FieldId(10)),
                ValueRef::Constant(PlanConstantId(0)),
                ValueRef::Constant(PlanConstantId(1)),
                ValueRef::Constant(PlanConstantId(2)),
                ValueRef::Constant(PlanConstantId(3)),
                ValueRef::State(StateId(0)),
                ValueRef::Constant(PlanConstantId(4)),
                ValueRef::Constant(PlanConstantId(5)),
                ValueRef::Constant(PlanConstantId(6)),
                ValueRef::Constant(PlanConstantId(7)),
                ValueRef::Constant(PlanConstantId(8)),
                ValueRef::Constant(PlanConstantId(6)),
                ValueRef::State(StateId(0)),
                ValueRef::Constant(PlanConstantId(9)),
                ValueRef::Constant(PlanConstantId(6)),
                ValueRef::State(StateId(0)),
            ],
            source_payload_field: None,
            update_constant_id: None,
            source_guard: None,
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

    session.apply(event(1, 0, None)).unwrap();

    let rows = &session.snapshot().unwrap().lists[&ListId(0)];
    assert_eq!(rows[0].fields[&FieldId(11)], Value::Text("Binary".into()));
    assert_eq!(
        rows[1].fields[&FieldId(11)],
        Value::Text("Hexadecimal".into())
    );
}

#[test]
fn list_find_uses_typed_index_without_scanning() {
    let list = ListStorageSlot {
        id: PlanStorageId(1),
        list_id: ListId(0),
        scope_id: None,
        row_field_ids: vec![FieldId(10), FieldId(11)],
        capacity: None,
        hidden_key_type: "Key".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: ["a", "b"]
            .into_iter()
            .map(|key| PlanInitialListRow {
                fields: vec![
                    PlanInitialListField {
                        name: "key".into(),
                        field_id: Some(FieldId(10)),
                        value: PlanConstantValue::Text { value: key.into() },
                    },
                    PlanInitialListField {
                        name: "value".into(),
                        field_id: Some(FieldId(11)),
                        value: PlanConstantValue::Text {
                            value: key.to_uppercase(),
                        },
                    },
                ],
            })
            .collect(),
    };
    let projection = PlanOp {
        id: PlanOpId(1),
        kind: PlanOpKind::ListProjection {
            projection: PlanListProjection::Find {
                source_list: ListId(0),
                field: "key".into(),
                value: ValueRef::State(StateId(0)),
            },
        },
        inputs: vec![ValueRef::List(ListId(0)), ValueRef::State(StateId(0))],
        output: Some(ValueRef::Field(FieldId(0))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            vec![
                constant(0, PlanConstantValue::Text { value: "a".into() }),
                constant(1, PlanConstantValue::Text { value: "b".into() }),
            ],
            vec![route(0, None)],
            vec![ScalarStorageSlot {
                id: PlanStorageId(0),
                state_id: StateId(0),
                value_type: PlanValueType::Text,
                scope_id: None,
                indexed: false,
                initial_value_kind: InitialValueKind::Text,
                initial_constant_id: Some(PlanConstantId(0)),
                initial_root_field_path: None,
                initial_row_field_path: None,
                initial_expression: None,
            }],
            vec![list],
            vec![projection, const_update(2, 0, 0, 1)],
            vec![(StateId(0), "selector")],
            vec![(ListId(0), "items")],
            vec![
                (FieldId(0), "selected"),
                (FieldId(10), "items.key"),
                (FieldId(11), "items.value"),
            ],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    let turn = session.apply(event(1, 0, None)).unwrap();
    assert!(turn.metrics.index_lookup_count >= 1);
    assert_eq!(turn.metrics.list_find_scan_count, 0);
    let selected = session.snapshot().unwrap().fields[&FieldId(0)].clone();
    let Value::Row { id, fields } = selected else {
        panic!("List/find did not return a stable row identity");
    };
    assert!(fields.is_empty());
    assert_eq!(
        session
            .project_current(&[ValueTarget::RowField {
                row: id,
                field: FieldId(11),
            }])
            .unwrap()[&ValueTarget::RowField {
            row: id,
            field: FieldId(11),
        }],
        Value::Text("B".into())
    );
}

#[test]
fn compiled_prefix_query_uses_bounded_index_and_tracks_currentness() {
    let mut compiled = compile_server_source(
        "indexed-prefix-query.bn",
        INDEXED_PREFIX_QUERY_SOURCE,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    compiled.plan.demand.root_derived_outputs = RootOutputDemand::All;
    let source = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.change")
        .unwrap()
        .source_id;
    let results = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|entry| entry.label == "store.results")
        .and_then(|entry| entry.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .unwrap();
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let initial = session
        .project_current(&[ValueTarget::Field(results)])
        .unwrap()
        .remove(&ValueTarget::Field(results))
        .unwrap();
    assert!(matches!(initial, Value::List(rows) if rows.len() == 2));

    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source,
            target: None,
            payload: SourcePayload {
                text: Some("be".into()),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    assert_eq!(turn.metrics.query_full_scan_count, 0);
    assert!(turn.metrics.query_index_range_count >= 1);
    assert_eq!(turn.metrics.query_rows_examined_count, 1);
    assert_eq!(turn.metrics.query_result_count, 1);
    let updated = session
        .project_current(&[ValueTarget::Field(results)])
        .unwrap()
        .remove(&ValueTarget::Field(results))
        .unwrap();
    assert!(matches!(updated, Value::List(rows) if rows.len() == 1));
}

#[test]
fn compiled_compound_query_executes_through_canonical_query_collection() {
    let mut compiled = compile_server_source(
        "generic-compound-query.bn",
        GENERIC_COMPOUND_QUERY_SOURCE,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    compiled.plan.demand.root_derived_outputs = RootOutputDemand::All;
    let selections = compiled
        .plan
        .regions
        .iter()
        .flat_map(|region| &region.ops)
        .filter_map(|op| match &op.kind {
            PlanOpKind::ListProjection {
                projection: PlanListProjection::IndexedQuery { selection, .. },
            } => Some(selection),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        selections
            .iter()
            .filter(|selection| matches!(selection, PlanQuerySelection::Union { .. }))
            .count(),
        1,
        "query selections: {selections:#?}"
    );
    assert_eq!(
        selections
            .iter()
            .filter(|selection| matches!(selection, PlanQuerySelection::Intersection { .. }))
            .count(),
        1,
        "query selections: {selections:#?}"
    );
    let page = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|entry| entry.label == "store.exact_page")
        .and_then(|entry| entry.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .unwrap();
    let union_page = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|entry| entry.label == "store.union_page")
        .and_then(|entry| entry.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .unwrap();
    let intersection_page = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|entry| entry.label == "store.intersection_page")
        .and_then(|entry| entry.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .unwrap();
    let mode_keys = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|entry| entry.label == "store.mode_keys")
        .and_then(|entry| entry.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .unwrap();
    let mode_index = compiled
        .plan
        .query_indexes
        .iter()
        .find(|index| {
            index
                .fields
                .iter()
                .any(|field| field.normalization == boon_plan::QueryTextNormalization::Tokens)
        })
        .unwrap()
        .clone();
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    let mode_values = session
        .list_row_snapshots(mode_index.source_list)
        .unwrap()
        .into_iter()
        .map(|row| row.fields[&mode_index.fields[0].field].clone())
        .collect::<Vec<_>>();
    assert!(
        mode_values
            .iter()
            .any(|value| value == &Value::Text("rail bus".to_owned())),
        "multi-value row authority: {mode_values:#?}"
    );
    let mut projected = session
        .project_current(&[
            ValueTarget::Field(page),
            ValueTarget::Field(union_page),
            ValueTarget::Field(intersection_page),
            ValueTarget::Field(mode_keys),
        ])
        .unwrap();
    assert_eq!(
        projected.remove(&ValueTarget::Field(mode_keys)),
        Some(Value::Record(BTreeMap::from([
            ("first".to_owned(), Value::Text("rail".to_owned())),
            ("second".to_owned(), Value::Text("bus".to_owned())),
        ])))
    );
    let value = projected.remove(&ValueTarget::Field(page)).unwrap();
    let Value::Record(page) = value else {
        panic!("indexed query did not return a page record");
    };
    assert!(matches!(page.get("rows"), Some(Value::List(rows)) if rows.len() == 1));
    assert_eq!(page.get("cursor"), Some(&Value::Bytes(Vec::new().into())));
    assert!(matches!(
        projected.remove(&ValueTarget::Field(union_page)),
        Some(Value::Record(page)) if matches!(page.get("rows"), Some(Value::List(rows)) if rows.len() == 3)
    ));
    let intersection = projected.remove(&ValueTarget::Field(intersection_page));
    assert!(
        matches!(
            &intersection,
            Some(Value::Record(page)) if matches!(page.get("rows"), Some(Value::List(rows)) if rows.len() == 1)
        ),
        "unexpected intersection page: {intersection:#?}"
    );
}

#[test]
fn indexed_query_mutation_is_atomic_current_and_never_scans() {
    let mut compiled = compile_server_source(
        "generic-query-mutation.bn",
        GENERIC_QUERY_MUTATION_SOURCE,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    compiled.plan.demand.root_derived_outputs = RootOutputDemand::All;
    assert!(
        compiled
            .plan
            .debug_map
            .unresolved_executable_refs
            .is_empty(),
        "mutation query has unresolved refs: {:?}",
        compiled.plan.debug_map.unresolved_executable_refs
    );
    let source = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.add")
        .unwrap()
        .source_id;
    let page = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|entry| entry.label == "store.page")
        .and_then(|entry| entry.id.strip_prefix("field:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(FieldId)
        .unwrap();
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();
    let initial = session
        .project_current(&[ValueTarget::Field(page)])
        .unwrap()
        .remove(&ValueTarget::Field(page))
        .unwrap();
    assert!(matches!(
        initial,
        Value::Record(page) if matches!(page.get("rows"), Some(Value::List(rows)) if rows.is_empty())
    ));
    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source,
            target: None,
            payload: SourcePayload {
                text: Some("Alpine".to_owned()),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    assert_eq!(turn.metrics.query_full_scan_count, 0);
    assert_eq!(turn.metrics.query_selected_indexes.len(), 1);
    assert!(turn.metrics.query_index_key_count <= 1);
    assert!(turn.metrics.query_rows_examined_count <= 1);
    let updated = session
        .project_current(&[ValueTarget::Field(page)])
        .unwrap()
        .remove(&ValueTarget::Field(page))
        .unwrap();
    assert!(matches!(
        updated,
        Value::Record(page) if matches!(page.get("rows"), Some(Value::List(rows)) if rows.len() == 1)
    ));
}

#[test]
fn list_map_records_preserve_source_row_identity() {
    let list = ListStorageSlot {
        id: PlanStorageId(1),
        list_id: ListId(0),
        scope_id: None,
        row_field_ids: vec![FieldId(10)],
        capacity: None,
        hidden_key_type: "Key".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![PlanInitialListField {
                name: "label".into(),
                field_id: Some(FieldId(10)),
                value: PlanConstantValue::Text {
                    value: "first".into(),
                },
            }],
        }],
    };
    let map = derived(
        0,
        0,
        vec![ValueRef::List(ListId(0))],
        Some(PlanRowExpression::ListMap {
            input: Box::new(PlanRowExpression::ListRef { list_id: ListId(0) }),
            binding: "item".into(),
            value: Box::new(PlanRowExpression::Object {
                fields: vec![PlanRowObjectField {
                    name: "title".into(),
                    value: PlanRowExpression::ObjectField {
                        object: Box::new(PlanRowExpression::ListMapItem {
                            binding: "item".into(),
                        }),
                        field: "label".into(),
                    },
                }],
            }),
        }),
    );
    let session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
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
    assert_eq!(fields["title"], Value::Text("first".into()));
}

#[test]
fn selected_demand_stays_current_without_eager_unrequested_work() {
    let demanded = derived(
        0,
        0,
        vec![ValueRef::State(StateId(0))],
        Some(PlanRowExpression::Field {
            input: ValueRef::State(StateId(0)),
        }),
    );
    let unsupported_unrequested = derived(1, 1, Vec::new(), None);
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::Selected(vec![FieldId(0)]),
            vec![
                constant(0, number_constant(1)),
                constant(1, number_constant(2)),
            ],
            vec![route(0, None)],
            vec![number_slot(0, 0)],
            Vec::new(),
            vec![demanded, unsupported_unrequested, const_update(2, 0, 0, 1)],
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

    let turn = session.apply(event(1, 0, None)).unwrap();
    assert_eq!(turn.metrics.recomputed_field_count, 1);
    assert_eq!(session.snapshot().unwrap().fields[&FieldId(0)], number(2));
}

#[test]
fn deterministic_work_budget_bounds_startup_without_affecting_unbounded_sessions() {
    let make_plan = || {
        plan(
            RootOutputDemand::Selected(vec![FieldId(0)]),
            vec![constant(0, number_constant(1))],
            Vec::new(),
            vec![number_slot(0, 0)],
            Vec::new(),
            vec![derived(
                0,
                0,
                vec![ValueRef::State(StateId(0))],
                Some(PlanRowExpression::Field {
                    input: ValueRef::State(StateId(0)),
                }),
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
    let read_update = PlanOp {
        id: PlanOpId(2),
        kind: PlanOpKind::UpdateBranch {
            trigger: ValueRef::Source(SourceId(0)),
            expression_kind: PlanExpressionKind::ReadPath,
            ordered_inputs: vec![ValueRef::Field(FieldId(0))],
            source_payload_field: None,
            update_constant_id: None,
            source_guard: None,
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0)), ValueRef::Field(FieldId(0))],
        output: Some(ValueRef::State(StateId(1))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::Selected(vec![FieldId(1)]),
            vec![constant(0, number_constant(1))],
            vec![route(0, None)],
            vec![number_slot(0, 0), number_slot(1, 0)],
            Vec::new(),
            vec![
                derived(
                    0,
                    0,
                    vec![ValueRef::State(StateId(0))],
                    Some(PlanRowExpression::Field {
                        input: ValueRef::State(StateId(0)),
                    }),
                ),
                derived(
                    1,
                    1,
                    vec![ValueRef::State(StateId(1))],
                    Some(PlanRowExpression::Field {
                        input: ValueRef::State(StateId(1)),
                    }),
                ),
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
        .apply(event(1, 0, None))
        .expect_err("the update plus currentness barrier must exceed four units");
    assert!(matches!(error, Error::WorkBudgetExceeded { limit: 4, .. }));
    assert_eq!(session.snapshot().unwrap(), before);
}

#[test]
fn materializing_a_row_field_does_not_invalidate_list_structure_consumers() {
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_field_ids: vec![FieldId(10), FieldId(11)],
        capacity: None,
        hidden_key_type: "Key".to_owned(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![PlanInitialListField {
                name: "raw".to_owned(),
                field_id: Some(FieldId(10)),
                value: PlanConstantValue::Text {
                    value: "value".to_owned(),
                },
            }],
        }],
    };
    let list_view = derived(
        0,
        0,
        vec![ValueRef::List(ListId(0))],
        Some(PlanRowExpression::ListRef { list_id: ListId(0) }),
    );
    let row_copy = PlanOp {
        id: PlanOpId(1),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::Pure,
            startup_recompute: false,
            expression: Some(PlanDerivedExpression::RowExpression {
                expression: PlanRowExpression::Field {
                    input: ValueRef::Field(FieldId(10)),
                },
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
    let source_transform = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::SourceEventTransform,
            startup_recompute: false,
            expression: Some(PlanDerivedExpression::SourceEventTransform {
                default: Box::new(PlanRowExpression::Constant {
                    constant_id: PlanConstantId(0),
                }),
                arms: vec![PlanSourceEventTransformArm {
                    trigger: ValueRef::Source(SourceId(0)),
                    value: PlanRowExpression::Constant {
                        constant_id: PlanConstantId(1),
                    },
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

    session.apply(event(1, 0, None)).unwrap();

    assert_eq!(
        session.root_value_current("event_value").unwrap(),
        Value::Text("captured".into())
    );
}

#[test]
fn source_transform_keeps_precommit_state_for_the_event_turn() {
    let source_transform = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::SourceEventTransform,
            startup_recompute: true,
            expression: Some(PlanDerivedExpression::SourceEventTransform {
                default: Box::new(PlanRowExpression::Constant {
                    constant_id: PlanConstantId(1),
                }),
                arms: vec![PlanSourceEventTransformArm {
                    trigger: ValueRef::Source(SourceId(0)),
                    value: PlanRowExpression::Field {
                        input: ValueRef::State(StateId(0)),
                    },
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
    let clear_state = PlanOp {
        id: PlanOpId(1),
        kind: PlanOpKind::UpdateBranch {
            trigger: ValueRef::Source(SourceId(0)),
            expression_kind: PlanExpressionKind::Const,
            ordered_inputs: Vec::new(),
            source_payload_field: None,
            update_constant_id: Some(PlanConstantId(1)),
            source_guard: None,
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
        value_type: PlanValueType::Text,
        scope_id: None,
        indexed: false,
        initial_value_kind: InitialValueKind::Text,
        initial_constant_id: Some(PlanConstantId(0)),
        initial_root_field_path: None,
        initial_row_field_path: None,
        initial_expression: None,
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
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

    session.apply(event(1, 0, None)).unwrap();

    assert_eq!(
        session.root_value_current("captured").unwrap(),
        Value::Text("before".into())
    );
}

#[test]
fn reverse_dependencies_recompute_every_dependent_once() {
    let copy = |id, field| {
        derived(
            id,
            field,
            vec![ValueRef::State(StateId(0))],
            Some(PlanRowExpression::Field {
                input: ValueRef::State(StateId(0)),
            }),
        )
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            vec![
                constant(0, number_constant(0)),
                constant(1, number_constant(1)),
            ],
            vec![route(0, None)],
            vec![number_slot(0, 0)],
            Vec::new(),
            vec![copy(0, 0), copy(1, 1), const_update(2, 0, 0, 1)],
            vec![(StateId(0), "source")],
            Vec::new(),
            vec![(FieldId(0), "left"), (FieldId(1), "right")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    let turn = session.apply(event(1, 0, None)).unwrap();
    assert_eq!(turn.metrics.recomputed_field_count, 2);
    assert_eq!(turn.metrics.dirty_field_count, 2);
    assert_eq!(session.snapshot().unwrap().fields.len(), 2);
}

#[test]
fn same_turn_recompute_does_not_suppress_later_invalidation() {
    let read_update = |id, state| PlanOp {
        id: PlanOpId(id),
        kind: PlanOpKind::UpdateBranch {
            trigger: ValueRef::Source(SourceId(0)),
            expression_kind: PlanExpressionKind::ReadPath,
            ordered_inputs: vec![ValueRef::Field(FieldId(1))],
            source_payload_field: None,
            update_constant_id: None,
            source_guard: None,
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0)), ValueRef::Field(FieldId(1))],
        output: Some(ValueRef::State(StateId(state))),
        indexed: false,
        unresolved_executable_ref_count: 0,
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::All,
            vec![
                constant(0, number_constant(0)),
                constant(1, number_constant(1)),
                constant(2, number_constant(2)),
            ],
            vec![route(0, None)],
            vec![number_slot(0, 0), number_slot(1, 0)],
            Vec::new(),
            vec![
                derived(
                    0,
                    0,
                    vec![ValueRef::State(StateId(0))],
                    Some(PlanRowExpression::Field {
                        input: ValueRef::State(StateId(0)),
                    }),
                ),
                derived(
                    1,
                    1,
                    vec![ValueRef::Field(FieldId(0))],
                    Some(PlanRowExpression::Field {
                        input: ValueRef::Field(FieldId(0)),
                    }),
                ),
                const_update(2, 0, 0, 1),
                read_update(3, 1),
                const_update(4, 0, 0, 2),
                read_update(5, 1),
            ],
            vec![(StateId(0), "source"), (StateId(1), "captured")],
            Vec::new(),
            vec![(FieldId(0), "middle"), (FieldId(1), "leaf")],
        ),
        SessionOptions::default(),
    )
    .unwrap();

    session.apply(event(1, 0, None)).unwrap();

    assert_eq!(session.snapshot().unwrap().states[&StateId(1)], number(2));
}

#[test]
fn recursive_derived_reentry_returns_typed_cycle_error() {
    let left = derived(
        0,
        0,
        vec![ValueRef::Field(FieldId(1))],
        Some(PlanRowExpression::Field {
            input: ValueRef::Field(FieldId(1)),
        }),
    );
    let right = derived(
        1,
        1,
        vec![ValueRef::Field(FieldId(0))],
        Some(PlanRowExpression::Field {
            input: ValueRef::Field(FieldId(0)),
        }),
    );
    let error = MachineInstance::new(
        plan(
            RootOutputDemand::Selected(vec![FieldId(0)]),
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
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_field_ids: vec![FieldId(0)],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![PlanInitialListField {
                name: "value".into(),
                field_id: Some(FieldId(0)),
                value: PlanConstantValue::Text {
                    value: "old".into(),
                },
            }],
        }],
    };
    let remove = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::ListOperation {
            operation_kind: PlanListOperationKind::Remove,
            append: None,
            remove: Some(PlanListRemove {
                source: ValueRef::Source(SourceId(0)),
                predicate: PlanListRemovePredicate::AlwaysTrue,
            }),
            retain: None,
            count: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0))],
        output: Some(ValueRef::List(ListId(0))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let append = PlanOp {
        id: PlanOpId(1),
        kind: PlanOpKind::ListOperation {
            operation_kind: PlanListOperationKind::Append,
            append: Some(PlanListAppend {
                trigger: ValueRef::Source(SourceId(1)),
                fields: vec![PlanListAppendField {
                    name: "value".into(),
                    field_id: Some(FieldId(0)),
                    value_ref: None,
                    constant_id: Some(PlanConstantId(0)),
                }],
            }),
            remove: None,
            retain: None,
            count: None,
        },
        inputs: vec![ValueRef::Source(SourceId(1))],
        output: Some(ValueRef::List(ListId(0))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let mut session = MachineInstance::new(
        plan(
            RootOutputDemand::Selected(Vec::new()),
            vec![constant(
                0,
                PlanConstantValue::Text {
                    value: "new".into(),
                },
            )],
            vec![route(0, Some(0)), route(1, Some(0))],
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
    session.apply(event(1, 0, Some(original))).unwrap();
    let turn = session.apply(event(2, 1, None)).unwrap();
    let inserted = turn
        .deltas
        .iter()
        .find_map(|delta| match delta {
            Delta::InsertRow { row } => Some(row.id),
            _ => None,
        })
        .unwrap();
    assert_ne!(inserted, original);
    assert_eq!(
        session
            .row_target_for_source(SourceId(1), inserted.key, inserted.generation)
            .unwrap(),
        inserted
    );
    assert_eq!(
        session
            .row_target_for_source_path("source.1", inserted.key, inserted.generation)
            .unwrap(),
        inserted
    );
}

#[test]
fn authority_restore_preserves_an_explicitly_emptied_list_and_allocator() {
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_field_ids: vec![FieldId(0)],
        capacity: None,
        hidden_key_type: "Key".into(),
        has_generation: true,
        initializer_kind: ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![PlanInitialListRow {
            fields: vec![PlanInitialListField {
                name: "value".into(),
                field_id: Some(FieldId(0)),
                value: PlanConstantValue::Text {
                    value: "default".into(),
                },
            }],
        }],
    };
    let remove = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::ListOperation {
            operation_kind: PlanListOperationKind::Remove,
            append: None,
            remove: Some(PlanListRemove {
                source: ValueRef::Source(SourceId(0)),
                predicate: PlanListRemovePredicate::AlwaysTrue,
            }),
            retain: None,
            count: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0))],
        output: Some(ValueRef::List(ListId(0))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let machine = plan(
        RootOutputDemand::Selected(Vec::new()),
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
    let original = session.list_row_at(ListId(0), 0).unwrap();
    session.apply(event(1, 0, Some(original))).unwrap();
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
    let list = ListStorageSlot {
        id: PlanStorageId(0),
        list_id: ListId(0),
        scope_id: Some(ScopeId(0)),
        row_field_ids: vec![FieldId(0)],
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
                    value: PlanConstantValue::Text {
                        value: "default".into(),
                    },
                }],
            })
            .collect(),
    };
    let indexed = ScalarStorageSlot {
        id: PlanStorageId(1),
        state_id: StateId(0),
        value_type: PlanValueType::Text,
        scope_id: Some(ScopeId(0)),
        indexed: true,
        initial_value_kind: InitialValueKind::Text,
        initial_constant_id: Some(PlanConstantId(0)),
        initial_root_field_path: None,
        initial_row_field_path: None,
        initial_expression: None,
    };
    let update = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::UpdateBranch {
            trigger: ValueRef::Source(SourceId(0)),
            expression_kind: PlanExpressionKind::Const,
            ordered_inputs: Vec::new(),
            source_payload_field: None,
            update_constant_id: Some(PlanConstantId(1)),
            source_guard: None,
            effect: None,
        },
        inputs: vec![ValueRef::Source(SourceId(0))],
        output: Some(ValueRef::State(StateId(0))),
        indexed: true,
        unresolved_executable_ref_count: 0,
    };
    let machine = plan(
        RootOutputDemand::Selected(Vec::new()),
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
    let selected = session.list_row_at(ListId(0), 1).unwrap();
    let turn = session.apply(event(1, 0, Some(selected))).unwrap();
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
    session.apply(event(1, 0, None)).unwrap();
    assert!(matches!(
        session.apply(event(1, 0, None)),
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
            List/find_value(
                request.query
                field: "name"
                value: TEXT { q }
                target: "value"
                fallback: TEXT { missing }
            )
        }
    found_row_name:
        request.method |> THEN {
            List/find_value(
                request.query
                field: "name"
                value: TEXT { q }
                target: "name"
                fallback: TEXT { missing }
            )
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

    let Value::Record(response) = session.output_value_current("http_response").unwrap() else {
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
    let machine = compile_server_source(
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
    .unwrap()
    .plan;
    assert!(
        machine
            .debug_map
            .list_slots
            .iter()
            .all(|entry| !entry.label.contains("selected_ids"))
    );
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
    let machine = compile_server_source(
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
    .unwrap()
    .plan;
    let source = source_id(&machine, "store.input");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    session
        .apply(SourceEvent {
            sequence: 1,
            source,
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
        |> List/map(row, new: stream_row(row: row, asset: asset))
        |> List/remove(row, when:
            row.remove |> THEN { True }
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
        |> List/map(row, new: mapped_effect_row(row: row))
    mapped_request:
        rows
        |> List/map(row, new: LATEST {
            row.open |> THEN { Primary }
        })
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
fn derived_empty_list_materializes_keyed_rows_and_initializes_row_state() {
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
        |> List/map(seed_row, new:
            wrap_row(row: seed_record(seed: seed_row))
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
        .find(|entry| entry.label == "rows")
        .and_then(|entry| entry.id.strip_prefix("list:"))
        .and_then(|id| id.parse::<usize>().ok())
        .map(ListId)
        .expect("rows list");
    let value_field = compiled
        .plan
        .debug_map
        .fields
        .iter()
        .find(|entry| entry.label.ends_with(".value"))
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
            .all(|op| !matches!(
                (&op.kind, op.output.as_ref()),
                (
                    PlanOpKind::DerivedValue {
                        expression: Some(PlanDerivedExpression::RowExpression {
                            expression: PlanRowExpression::Field {
                                input: ValueRef::Field(input),
                            },
                        }),
                        ..
                    },
                    Some(ValueRef::Field(output)),
                ) if input == output
            ))
    );
    let mut session = MachineInstance::new(compiled.plan, SessionOptions::default()).unwrap();

    let materialized = session.root_value_current("store.rows").unwrap();
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
    assert_eq!(semantic_image.lists.len(), 2);
    assert!(
        semantic_image
            .lists
            .values()
            .all(|list| list.rows.iter().all(|row| row.fields.len() <= 2))
    );
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
fn host_bound_projection_preserves_authority_and_tag_matching_uses_the_facade() {
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
    assert_eq!(
        crate::machine::value_to_match_label(&value).unwrap(),
        "FileSelected"
    );
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
    let update = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::UpdateBranch {
            trigger: ValueRef::Source(SourceId(0)),
            expression_kind: PlanExpressionKind::SourcePayload,
            ordered_inputs: Vec::new(),
            source_payload_field: Some(payload_field.clone()),
            update_constant_id: None,
            source_guard: None,
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
fn read_only_http_effect_is_transient_typed_correlated_and_cycle_safe() {
    let machine = outbound_http_effect_machine();
    let last_status = match &machine.output_root("last_status").unwrap().value {
        OutputValueRef::RuntimeValue {
            value: ValueRef::Field(field),
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
            target: None,
            payload: outbound_http_payload(),
        })
        .unwrap();
    assert!(turn.outbox_changes.is_empty());
    let [invocation] = turn.transient_effects.as_slice() else {
        panic!("HTTP request must emit exactly one transient effect");
    };
    assert_eq!(invocation.effect_id, contract.effect_id);
    assert_eq!(invocation.trigger_source_sequence, 1);
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
fn derived_when_event_updates_state_despite_later_list_map_branches() {
    let machine = compile_server_source(
        "derived-when-with-list-map-executor.bn",
        r#"
store: [
    start: SOURCE
    reset: SOURCE
    seed_rows: LIST { [key: TEXT { row }] }
    rows:
        seed_rows |> List/map(seed_row, new: selectable_row(seed_row: seed_row))
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
                    |> List/map(row, new: LATEST {
                        row.select |> THEN { row.key }
                    })
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
}

#[test]
fn effects_sample_state_after_same_trigger_updates_settle() {
    let machine = compile_server_source(
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
    .unwrap()
    .plan;
    let start = source_id(&machine, "store.start");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let clock = session
        .apply(SourceEvent {
            sequence: 1,
            source: start,
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
fn transient_http_cancel_and_rollback_preserve_one_shot_ownership() {
    let machine = outbound_http_effect_machine();
    let request = source_id(&machine, "store.request");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    let first = session
        .apply(SourceEvent {
            sequence: 1,
            source: request,
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
fn stream_credit_accounting_follows_contract_tags_not_result_names() {
    let mut machine = file_stream_effect_machine();
    let contract = machine
        .effects
        .iter_mut()
        .find(|contract| contract.host_operation == "File/read_stream")
        .unwrap();
    contract.delivery = EffectDeliveryCardinality::Stream {
        initial_credits: 1,
        max_in_flight: 1,
        credit_result_tags: vec!["Opened".to_owned()],
        terminal_result_tags: vec![
            "Cancelled".to_owned(),
            "Failed".to_owned(),
            "Finished".to_owned(),
        ],
    };
    contract.validate().unwrap();

    let read = source_id(&machine, "store.read");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let invocation = session
        .apply(SourceEvent {
            sequence: 1,
            source: read,
            target: None,
            payload: file_stream_payload(),
        })
        .unwrap()
        .transient_effects
        .remove(0);

    let opened = session
        .deliver_transient_effect_result(
            invocation.call_id,
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
    assert_eq!(
        opened.transient_effect_credit_grants,
        vec![TransientEffectCreditGrant {
            call_id: invocation.call_id,
            credits: 1,
        }]
    );

    let chunk = session
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
    assert!(chunk.transient_effect_credit_grants.is_empty());

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
fn stream_effect_delivery_is_ordered_bounded_terminal_and_replaced_by_owner() {
    let machine = file_stream_effect_machine();
    let read = source_id(&machine, "store.read");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let first_turn = session
        .apply(SourceEvent {
            sequence: 1,
            source: read,
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
            target: None,
            payload: file_stream_payload(),
        })
        .unwrap();
    let replacement = replacement_source.transient_effects[0].clone();
    let replaced_again = session
        .apply(SourceEvent {
            sequence: 3,
            source: read,
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
fn nested_effect_guards_ignore_partial_variants_and_invoke_only_the_retained_branch() {
    let machine = nested_stream_effect_chain_machine();
    let read = source_id(&machine, "store.read");
    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();

    let first_file = session
        .apply(SourceEvent {
            sequence: 1,
            source: read,
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
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap()
        .transient_effects
        .remove(0);
    let content = content_ref_value();
    let turn = session
        .deliver_transient_effect_result(
            second_file.call_id,
            0,
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
    let effect_op = machine
        .regions
        .iter_mut()
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    effect: Some(_),
                    ..
                }
            )
        })
        .expect("file stream plan has an effect update");
    let PlanOpKind::UpdateBranch { source_guard, .. } = &mut effect_op.kind else {
        unreachable!();
    };
    *source_guard = Some(PlanSourceGuard::ValueOneOf {
        input: ValueRef::SourcePayload {
            source_id: read,
            field: SourcePayloadField::Text,
        },
        values: vec!["start".to_owned()],
    });

    let mut session = MachineInstance::new(machine, SessionOptions::default()).unwrap();
    let mut start_payload = file_stream_payload();
    start_payload.text = Some("start".to_owned());
    let first = session
        .apply(SourceEvent {
            sequence: 1,
            source: read,
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
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap();
    assert!(stopped.transient_effects.is_empty());
    assert_eq!(stopped.cancelled_transient_effects, vec![call_id]);
    assert_eq!(session.pending_transient_effect_count(), 0);
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
        |> List/map(row, new: new_row(row: row))
    fallback: SOURCE
    row_selected:
        rows
        |> List/map(row, new:
            row.controls.select.event.press |> THEN { row.name }
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
    let row = session.list_row_at(list, 1).unwrap();

    let selected = session
        .apply(SourceEvent {
            sequence: 1,
            source,
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
    let row = session.list_row_at(list, 0).unwrap();
    let invocation = session
        .apply(SourceEvent {
            sequence: 1,
            source: open,
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
            target: Some(row),
            payload: SourcePayload::default(),
        })
        .unwrap();

    assert!(session.list_rows(list).is_empty());
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
    let row = session.snapshot().unwrap().lists[&list][0].id;
    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source,
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
    let turn = session
        .apply(SourceEvent {
            sequence,
            source: source_id(machine, "store.register"),
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
                    False => Text/is_empty(grant) |> WHEN {
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
            |> List/any(entry, if:
                entry.id == add.text
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
        |> List/map(entry, new: entry_view(entry: entry))
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
    let event = |sequence, text: &str| SourceEvent {
        sequence,
        source: add,
        target: None,
        payload: SourcePayload {
            text: Some(text.to_owned()),
            ..SourcePayload::default()
        },
    };

    let first = session.apply(event(1, "alpha")).unwrap();
    assert!(first.authority_deltas.iter().any(|delta| matches!(
        delta,
        AuthorityDelta::ReplaceList { .. } | AuthorityDelta::InsertRow { .. }
    )));
    let duplicate = session.apply(event(2, "alpha")).unwrap();
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

    session.apply(event(3, "beta")).unwrap();
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
        |> List/map(revision, new: revision_view(revision: revision))
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
            PlanOpKind::ListOperation {
                operation_kind: PlanListOperationKind::Append,
                append,
                ..
            } => append.as_ref(),
            _ => None,
        })
        .expect("append descriptor");
    let field_id = |name: &str| {
        append
            .fields
            .iter()
            .find(|field| field.name == name)
            .and_then(|field| field.field_id)
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
    function_export_id: ExportId,
    function_argument_id: DistributedArgumentId,
    undeclared_import_id: ImportId,
    undeclared_export_id: ExportId,
}

fn executor_distributed_declaration(semantic_path: &str) -> DistributedDeclarationId {
    DistributedDeclarationId::from_semantic_path("PlanExecutorDistributedFixture", semantic_path)
        .unwrap()
}

fn distributed_session_fixture() -> DistributedSessionFixture {
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
    let function_export_id = ExportId::from_identity(
        graph.graph_id,
        session_endpoint_id,
        DistributedExportKind::PureFunction,
        function_declaration,
    )
    .unwrap();
    let function_argument_id =
        DistributedArgumentId::from_parameter_name(function_export_id, "value").unwrap();
    let function_argument = || PlanRowExpression::Field {
        input: ValueRef::DistributedFunctionArgument {
            export_id: function_export_id,
            argument_id: function_argument_id,
        },
    };
    let function_export = DistributedPureFunctionExportPlan::new(
        graph.graph_id,
        session_endpoint_id,
        function_declaration,
        1,
        ProgramRole::Session,
        vec![("value".to_owned(), DataTypePlan::Number)],
        DataTypePlan::Number,
        PlanRowExpression::NumberInfix {
            op: "+".to_owned(),
            left: Box::new(function_argument()),
            right: Box::new(function_argument()),
        },
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
        vec![function_export],
        Vec::new(),
    )
    .unwrap();
    let client_endpoint = DistributedEndpointContractPlan::new(
        &graph,
        executor_distributed_declaration("endpoint.client"),
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

    let remote_count = PlanRowExpression::Field {
        input: ValueRef::DistributedImport(value_import.import_id),
    };
    let mut machine = plan(
        RootOutputDemand::All,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![derived(
            0,
            0,
            vec![ValueRef::DistributedImport(value_import.import_id)],
            Some(remote_count),
        )],
        Vec::new(),
        Vec::new(),
        vec![(FieldId(0), "store.remote_count")],
    );
    assert_eq!(machine.application.identity, application_identity);
    machine.program_role = ProgramRole::Session;
    machine.distributed_endpoint = Some(
        DistributedEndpointPlan::new(&application_identity, &linked_graph, ProgramRole::Session)
            .unwrap(),
    );

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
        function_export_id,
        function_argument_id,
        undeclared_import_id,
        undeclared_export_id: server_value.export_id,
    }
}

struct AtomicDistributedContextFixture {
    plan: MachinePlan,
    first_import_id: ImportId,
    second_import_id: ImportId,
    call_result_import_id: ImportId,
    undeclared_import_id: ImportId,
}

fn atomic_distributed_context_fixture() -> AtomicDistributedContextFixture {
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
    let function_export_id = ExportId::from_identity(
        graph.graph_id,
        server_endpoint_id,
        DistributedExportKind::PureFunction,
        function_declaration,
    )
    .unwrap();
    let function_argument_id =
        DistributedArgumentId::from_parameter_name(function_export_id, "value").unwrap();
    let server_function = DistributedPureFunctionExportPlan::new(
        graph.graph_id,
        server_endpoint_id,
        function_declaration,
        1,
        ProgramRole::Server,
        vec![("value".to_owned(), DataTypePlan::Number)],
        DataTypePlan::Number,
        PlanRowExpression::Field {
            input: ValueRef::DistributedFunctionArgument {
                export_id: function_export_id,
                argument_id: function_argument_id,
            },
        },
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
    let remote_call = RemoteCallSitePlan::new(
        graph.graph_id,
        session_endpoint_id,
        executor_distributed_declaration("atomic.session.call.identity"),
        1,
        ProgramRole::Session,
        &server_function,
        vec![(
            "value".to_owned(),
            ValueRef::DistributedImport(first_import.import_id),
        )],
    )
    .unwrap();
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
    let context_expression = PlanRowExpression::Object {
        fields: vec![
            PlanRowObjectField {
                name: "status".to_owned(),
                value: PlanRowExpression::Intrinsic {
                    intrinsic: PlanIntrinsic::SessionInfoStatus,
                },
            },
            PlanRowObjectField {
                name: "principal".to_owned(),
                value: PlanRowExpression::Intrinsic {
                    intrinsic: PlanIntrinsic::SessionInfoPrincipal,
                },
            },
            PlanRowObjectField {
                name: "first".to_owned(),
                value: PlanRowExpression::Field {
                    input: ValueRef::DistributedImport(first_import.import_id),
                },
            },
            PlanRowObjectField {
                name: "second".to_owned(),
                value: PlanRowExpression::Field {
                    input: ValueRef::DistributedImport(second_import.import_id),
                },
            },
            PlanRowObjectField {
                name: "call_result".to_owned(),
                value: PlanRowExpression::Field {
                    input: ValueRef::DistributedImport(remote_call.result_import_id),
                },
            },
        ],
    };
    let mut machine = plan(
        RootOutputDemand::All,
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
                ValueRef::DistributedImport(remote_call.result_import_id),
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
        call_result_import_id: remote_call.result_import_id,
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
                DistributedImportUpdate::new(fixture.call_result_import_id, 5, number(33)),
            ],
        )
        .unwrap()
        .expect("the patch API must install an available Session context");

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
    for import_id in [
        fixture.first_import_id,
        fixture.second_import_id,
        fixture.call_result_import_id,
    ] {
        assert_eq!(session.distributed_import_revision(import_id), Some(5));
    }
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
                DistributedImportUpdate::new(fixture.call_result_import_id, 5, number(33)),
            ],
        )
        .unwrap()
        .expect("origin A must install a complete context");

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
        3
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
        session.distributed_import_value_current(fixture.call_result_import_id),
        Ok(remote_not_current())
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
                DistributedImportUpdate::new(fixture.call_result_import_id, 5, number(33)),
            ],
        )
        .unwrap()
        .expect("origin A must install a complete context");

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
    for import_id in [
        fixture.first_import_id,
        fixture.second_import_id,
        fixture.call_result_import_id,
    ] {
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
                DistributedImportUpdate::new(fixture.call_result_import_id, 5, number(33)),
            ],
        )
        .unwrap()
        .expect("origin A must install a complete context");

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
        Some(5)
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
                DistributedImportUpdate::new(fixture.call_result_import_id, 5, number(33)),
            ],
        )
        .unwrap()
        .expect("origin A must install a complete context");
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
        Some(5)
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
fn distributed_pure_functions_use_typed_argument_ids_and_fail_closed() {
    let fixture = distributed_session_fixture();
    let mut session = MachineInstance::new(fixture.plan, SessionOptions::default()).unwrap();

    assert_eq!(
        session
            .evaluate_distributed_function(
                fixture.function_export_id,
                BTreeMap::from([(fixture.function_argument_id, number(7))]),
            )
            .unwrap(),
        number(14)
    );

    assert!(matches!(
        session.evaluate_distributed_function(
            fixture.function_export_id,
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
        session.evaluate_distributed_function(
            fixture.function_export_id,
            BTreeMap::from([(wrong_argument_id, number(7))]),
        ),
        Err(Error::InvalidEvent(detail)) if detail.contains("missing argument `value`")
    ));
    assert!(matches!(
        session.evaluate_distributed_function(fixture.function_export_id, BTreeMap::new()),
        Err(Error::InvalidEvent(detail)) if detail.contains("expected 1")
    ));
    assert!(matches!(
        session.evaluate_distributed_function(fixture.undeclared_export_id, BTreeMap::new()),
        Err(Error::InvalidEvent(detail)) if detail.contains("not declared")
    ));
    assert!(matches!(
        session.distributed_export_value_current(fixture.function_export_id),
        Err(Error::InvalidEvent(detail)) if detail.contains("not declared")
    ));
}
