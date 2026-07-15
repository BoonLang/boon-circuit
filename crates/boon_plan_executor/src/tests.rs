use super::*;
use boon_plan::*;
use std::collections::BTreeMap;

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
        application,
        persistence,
        effects: Vec::new(),
        outputs: Vec::new(),
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
        PlanValueType::Byte => DataTypePlan::Byte,
        PlanValueType::Bool => DataTypePlan::Bool,
        PlanValueType::Bytes { fixed_len } => DataTypePlan::Bytes { fixed_len },
        PlanValueType::Enum => DataTypePlan::Variant {
            variants: Vec::new(),
        },
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
        initial_row_expression: None,
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
    let mut session = Session::new(
        plan(
            RootOutputDemand::All,
            vec![
                constant(0, PlanConstantValue::Number { value: 3 }),
                constant(1, PlanConstantValue::Number { value: 3 }),
                constant(2, PlanConstantValue::Number { value: 4 }),
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
    let mut session = Session::new(
        plan(
            RootOutputDemand::All,
            vec![
                constant(0, PlanConstantValue::Number { value: 1 }),
                constant(1, PlanConstantValue::Number { value: 0 }),
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
        Value::Number(1)
    );
    assert_eq!(
        session.root_value_current("draft_revision").unwrap(),
        Value::Number(1)
    );
    assert_eq!(
        session
            .root_value_current("revision.draft_revision")
            .unwrap(),
        Value::Number(0)
    );
}

#[test]
fn authority_restore_preserves_touched_value_equal_to_old_default() {
    let make_plan = |default: i64| {
        plan(
            RootOutputDemand::Selected(Vec::new()),
            vec![
                constant(0, PlanConstantValue::Number { value: default }),
                constant(1, PlanConstantValue::Number { value: 0 }),
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

    let mut original = Session::new(make_plan(0), SessionOptions::default()).unwrap();
    let turn = original.apply(event(1, 0, None)).unwrap();
    assert!(turn.deltas.is_empty());
    assert_eq!(
        turn.authority_deltas,
        vec![AuthorityDelta::SetRoot {
            state: StateId(0),
            value: Value::Number(0),
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

    let restored = SessionBuilder::new(make_plan(10), SessionOptions::default())
        .unwrap()
        .restore_durable(durable)
        .unwrap()
        .build()
        .unwrap();
    assert_eq!(
        restored.authority_snapshot().unwrap().states[&StateId(0)].value,
        Value::Number(0)
    );
}

#[test]
fn failed_turn_rolls_back_authority_and_touch_provenance() {
    let machine = plan(
        RootOutputDemand::Selected(Vec::new()),
        vec![
            constant(0, PlanConstantValue::Number { value: 1 }),
            constant(1, PlanConstantValue::Number { value: 2 }),
        ],
        vec![route(0, None)],
        vec![number_slot(0, 0), number_slot(1, 0)],
        Vec::new(),
        vec![const_update(0, 0, 0, 1), const_update(1, 0, 1, 99)],
        vec![(StateId(0), "first"), (StateId(1), "second")],
        Vec::new(),
        Vec::new(),
    );
    let mut session = Session::new(machine, SessionOptions::default()).unwrap();
    let before = session.authority_snapshot().unwrap();

    assert!(session.apply(event(1, 0, None)).is_err());
    assert_eq!(session.authority_snapshot().unwrap(), before);
}

#[test]
fn unsettled_turn_can_rollback_authority_sequence_and_durable_delta() {
    let machine = plan(
        RootOutputDemand::Selected(Vec::new()),
        vec![
            constant(0, PlanConstantValue::Number { value: 1 }),
            constant(1, PlanConstantValue::Number { value: 2 }),
        ],
        vec![route(0, None)],
        vec![number_slot(0, 0)],
        Vec::new(),
        vec![const_update(0, 0, 0, 1)],
        vec![(StateId(0), "count")],
        Vec::new(),
        Vec::new(),
    );
    let mut session = Session::new(machine, SessionOptions::default()).unwrap();
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
    let session = Session::new(
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
    let session = Session::new(
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
    let session = Session::new(
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
        initial_row_expression: None,
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
    let mut session = Session::new(
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
    let session = Session::new(
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
        initial_row_expression: None,
    };
    let update = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::UpdateBranch {
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
    let mut session = Session::new(
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
                constant(4, PlanConstantValue::Number { value: 2 }),
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
    let mut session = Session::new(
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
                initial_row_expression: None,
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
    let session = Session::new(
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
    let mut session = Session::new(
        plan(
            RootOutputDemand::Selected(vec![FieldId(0)]),
            vec![
                constant(0, PlanConstantValue::Number { value: 1 }),
                constant(1, PlanConstantValue::Number { value: 2 }),
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
        Value::Number(1)
    );
    assert_eq!(
        session
            .project_current(&[ValueTarget::Field(FieldId(1))])
            .unwrap_err(),
        Error::NotDemanded(FieldId(1))
    );

    let turn = session.apply(event(1, 0, None)).unwrap();
    assert_eq!(turn.metrics.recomputed_field_count, 1);
    assert_eq!(
        session.snapshot().unwrap().fields[&FieldId(0)],
        Value::Number(2)
    );
}

#[test]
fn deterministic_work_budget_bounds_startup_without_affecting_unbounded_sessions() {
    let make_plan = || {
        plan(
            RootOutputDemand::Selected(vec![FieldId(0)]),
            vec![constant(0, PlanConstantValue::Number { value: 1 })],
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

    Session::new(make_plan(), SessionOptions::default())
        .expect("trusted sessions remain unbounded by default");
    let error = Session::new(
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
            expression_kind: PlanExpressionKind::ReadPath,
            ordered_inputs: Vec::new(),
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
    let mut session = Session::new(
        plan(
            RootOutputDemand::Selected(vec![FieldId(1)]),
            vec![constant(0, PlanConstantValue::Number { value: 1 })],
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
    let mut session = Session::new(
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
                    source_id: SourceId(0),
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
    let mut session = Session::new(
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
                    source_id: SourceId(0),
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
        initial_row_expression: None,
    };
    let mut session = Session::new(
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
    let mut session = Session::new(
        plan(
            RootOutputDemand::All,
            vec![
                constant(0, PlanConstantValue::Number { value: 0 }),
                constant(1, PlanConstantValue::Number { value: 1 }),
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
            expression_kind: PlanExpressionKind::ReadPath,
            ordered_inputs: Vec::new(),
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
    let mut session = Session::new(
        plan(
            RootOutputDemand::All,
            vec![
                constant(0, PlanConstantValue::Number { value: 0 }),
                constant(1, PlanConstantValue::Number { value: 1 }),
                constant(2, PlanConstantValue::Number { value: 2 }),
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

    assert_eq!(
        session.snapshot().unwrap().states[&StateId(1)],
        Value::Number(2)
    );
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
    let error = Session::new(
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
    let mut session = Session::new(
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
    let mut session = Session::new(machine.clone(), SessionOptions::default()).unwrap();
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

    let restored = SessionBuilder::new(machine, SessionOptions::default())
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
        initial_row_expression: None,
    };
    let update = PlanOp {
        id: PlanOpId(0),
        kind: PlanOpKind::UpdateBranch {
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
    let mut session = Session::new(machine.clone(), SessionOptions::default()).unwrap();
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

    let restored = SessionBuilder::new(machine, SessionOptions::default())
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
    let mut session = Session::new(
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
        crate::session::runtime_value(boon_persistence::StoredValue::Variant {
            tag: "Done".to_owned(),
            fields: BTreeMap::new(),
        })
        .unwrap(),
        Value::Text("Done".to_owned())
    );

    let runtime = Value::Record(BTreeMap::from([
        ("$tag".to_owned(), Value::Text("Ready".to_owned())),
        ("count".to_owned(), Value::Number(4)),
    ]));
    let stored = crate::session::stored_value(&runtime).unwrap();
    assert!(matches!(
        &stored,
        boon_persistence::StoredValue::Variant { tag, fields }
            if tag == "Ready" && fields["count"] == boon_persistence::StoredValue::Number(4)
    ));
    assert_eq!(crate::session::runtime_value(stored).unwrap(), runtime);
}

#[test]
fn host_outputs_are_demand_current_and_reconstructed_without_a_document() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
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
    let mut session = Session::new(compiled.plan, SessionOptions::default()).unwrap();

    assert_eq!(
        session.output_value_current("api_response").unwrap(),
        Value::Record(BTreeMap::from([
            ("body".to_owned(), Value::Text("accepted".to_owned())),
            ("request_count".to_owned(), Value::Number(0)),
            ("status".to_owned(), Value::Number(200)),
        ]))
    );
    assert_eq!(
        session.output_value_current("pending_priorities").unwrap(),
        Value::List(vec![Value::Number(1), Value::Number(2)])
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
    assert_eq!(response["request_count"], Value::Number(1));
}

#[test]
fn source_payload_text_to_number_executes_the_typed_conversion() {
    let machine = boon_compiler::compile_source_text_to_machine_plan(
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
    let mut session = Session::new(machine, SessionOptions::default()).unwrap();

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
        Value::Number(42)
    );
}

fn typed_passkey_effect_machine() -> MachinePlan {
    boon_compiler::compile_source_text_to_machine_plan(
        "typed-passkey-effects-executor.bn",
        r#"
store: [
    register: SOURCE
    authenticate: SOURCE
    registration_succeeded: SOURCE
    registration_cancelled: SOURCE
    registration_failed: SOURCE
    duplicate_credential: SOURCE
    authentication_succeeded: SOURCE
    authentication_cancelled: SOURCE
    authentication_failed: SOURCE
    simulate_cancel: SOURCE
    simulate_failure: SOURCE
    simulate_duplicate: SOURCE
    workspace_id: TEXT { workspace-1 } |> HOLD workspace_id
    account_id: TEXT { account-1 } |> HOLD account_id
    credential_count: 1 |> HOLD credential_count
    simulation:
        Success |> HOLD simulation {
            LATEST {
                store.simulate_cancel |> THEN { Cancel }
                store.simulate_failure |> THEN { Failure }
                store.simulate_duplicate |> THEN { Duplicate }
            }
        }
    last_result:
        Pending |> HOLD last_result {
            LATEST {
                store.registration_succeeded |> THEN { RegistrationSucceeded }
                store.registration_cancelled |> THEN { RegistrationCancelled }
                store.registration_failed |> THEN { RegistrationFailed }
                store.duplicate_credential |> THEN { DuplicateCredential }
                store.authentication_succeeded |> THEN { AuthenticationSucceeded }
                store.authentication_cancelled |> THEN { AuthenticationCancelled }
                store.authentication_failed |> THEN { AuthenticationFailed }
            }
        }
    result_account_id:
        TEXT {} |> HOLD result_account_id {
            LATEST {
                store.registration_succeeded |> THEN { store.registration_succeeded.account_id }
                store.duplicate_credential |> THEN { store.duplicate_credential.account_id }
                store.authentication_succeeded |> THEN { store.authentication_succeeded.account_id }
            }
        }
    result_credential_id:
        TEXT {} |> HOLD result_credential_id {
            LATEST {
                store.registration_succeeded |> THEN { store.registration_succeeded.credential_id }
                store.duplicate_credential |> THEN { store.duplicate_credential.credential_id }
                store.authentication_succeeded |> THEN { store.authentication_succeeded.credential_id }
            }
        }
    result_label:
        TEXT {} |> HOLD result_label {
            LATEST {
                store.registration_succeeded |> THEN { store.registration_succeeded.label }
            }
        }
    failure_code:
        TEXT {} |> HOLD failure_code {
            LATEST {
                store.registration_failed |> THEN { store.registration_failed.code }
                store.authentication_failed |> THEN { store.authentication_failed.code }
            }
        }
    failure_message:
        TEXT {} |> HOLD failure_message {
            LATEST {
                store.registration_failed |> THEN { store.registration_failed.message }
                store.authentication_failed |> THEN { store.authentication_failed.message }
            }
        }
    failure_retryable:
        False |> HOLD failure_retryable {
            LATEST {
                store.registration_failed |> THEN { store.registration_failed.retryable }
                store.authentication_failed |> THEN { store.authentication_failed.retryable }
            }
        }
]

effects: [
    register_passkey: [
        on: store.register
        perform: DevelopmentPasskey/register(
            workspace_id: store.workspace_id
            account_id: store.account_id
            credential_count: store.credential_count
            simulation: store.simulation
        )
        results: [
            RegistrationSucceeded: store.registration_succeeded
            RegistrationCancelled: store.registration_cancelled
            RegistrationFailed: store.registration_failed
            DuplicateCredential: store.duplicate_credential
        ]
    ]
    authenticate_passkey: [
        on: store.authenticate
        perform: DevelopmentPasskey/authenticate(
            account_id: store.account_id
            credential_count: store.credential_count
            simulation: store.simulation
        )
        results: [
            AuthenticationSucceeded: store.authentication_succeeded
            AuthenticationCancelled: store.authentication_cancelled
            AuthenticationFailed: store.authentication_failed
        ]
    ]
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan
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
    session: &mut Session,
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
) -> (Session, boon_persistence::DurableOutboxItem) {
    let mut session = Session::new(machine.clone(), SessionOptions::default()).unwrap();
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
fn correlated_effect_result_source_rejects_external_dispatch() {
    let machine = typed_passkey_effect_machine();
    let result_source = source_id(&machine, "store.registration_succeeded");
    let mut session = Session::new(machine, SessionOptions::default()).unwrap();
    let error = session
        .apply(SourceEvent {
            sequence: 1,
            source: result_source,
            target: None,
            payload: SourcePayload::default(),
        })
        .unwrap_err();
    assert!(matches!(
        error,
        Error::InvalidEvent(detail)
            if detail.contains("reserved for correlated host-effect completion")
    ));
}

#[test]
fn identical_effect_intents_on_distinct_source_turns_have_distinct_identities() {
    let machine = typed_passkey_effect_machine();
    let register = source_id(&machine, "store.register");
    let mut session = Session::new(machine, SessionOptions::default()).unwrap();
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
    let mut restored = SessionBuilder::new(machine.clone(), SessionOptions::default())
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
fn generated_plan_restores_bare_root_latest_as_its_only_scalar_authority() {
    let machine = boon_compiler::compile_source_text_to_machine_plan(
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
        LATEST {
            pulse |> THEN { count + 10 }
        }
    derived: count + 20
]
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap()
    .plan;
    let pulse = source_id(&machine, "store.pulse");
    let count = state_id(&machine, "store.count");
    let mut session = Session::new(machine.clone(), SessionOptions::default()).unwrap();

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
    assert_eq!(authority.states[&count].value, Value::Number(1));
    assert!(authority.states[&count].touched);

    let mut restored = SessionBuilder::new(machine, SessionOptions::default())
        .unwrap()
        .restore(authority)
        .build()
        .unwrap();
    assert_eq!(
        restored.root_value_current("store.count").unwrap(),
        Value::Number(1)
    );
}

#[test]
fn list_append_reads_pre_turn_authority_and_skips_duplicate_candidate() {
    let machine = boon_compiler::compile_source_text_to_machine_plan(
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
    let mut session = Session::new(machine, SessionOptions::default()).unwrap();
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
    let machine = boon_compiler::compile_source_text_to_machine_plan(
        "append-source-payload-fields-executor.bn",
        r#"
store: [
    completed: SOURCE
    append_token:
        LATEST {
            completed |> THEN { completed.digest }
        }
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
    let mut session = Session::new(machine, SessionOptions::default()).unwrap();

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
