use super::*;
use boon_plan::*;

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
    MachinePlan {
        version: PlanVersion::default(),
        target_profile: TargetProfile::SoftwareDefault,
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
        payload_schema: SourcePayloadSchema {
            fields: Vec::new(),
            typed_fields: Vec::new(),
            row_lookup_field: None,
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
