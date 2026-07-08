// Included by `../machine_plan_backend_tests.rs`; kept in the parent test module for private invariant access.

#[test]
fn source_payload_update_lowers_to_typed_payload_ref() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let source_id = debug_entry_id(
        &plan.debug_map.source_routes,
        "source",
        "store.sources.new_todo_input.change",
    );
    let state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.new_todo_text");
    let op = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter())
        .find(|op| {
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == state_id)
        })
        .expect("new todo text route should lower to one update op");
    assert_eq!(op.unresolved_executable_ref_count, 0);
    assert!(
        op.inputs.iter().any(|input| matches!(
            input,
            ValueRef::SourcePayload {
                source_id: input_source_id,
                field: SourcePayloadField::Text
            } if input_source_id.0 == source_id
        )),
        "source payload should be a typed executable operand: {op:#?}"
    );
    assert!(matches!(
        &op.kind,
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::SourcePayload,
            source_payload_field: Some(SourcePayloadField::Text),
            ..
        }
    ));
}


#[test]
fn const_update_lowers_to_typed_update_constant() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let source_id = debug_entry_id(
        &plan.debug_map.source_routes,
        "source",
        "store.sources.filter_active.press",
    );
    let state_id = debug_entry_id(
        &plan.debug_map.state_slots,
        "state",
        "store.selected_filter",
    );
    let op = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter())
        .find(|op| {
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == state_id)
        })
        .expect("filter active route should lower to one update op");
    assert_eq!(op.unresolved_executable_ref_count, 0);
    let PlanOpKind::UpdateBranch {
        expression_kind: PlanExpressionKind::Const,
        source_payload_field: None,
        update_constant_id: Some(update_constant_id),
        ..
    } = &op.kind
    else {
        panic!("filter active should lower as a typed Const update branch: {op:#?}");
    };
    let constant = plan
        .constants
        .iter()
        .find(|constant| constant.id == *update_constant_id)
        .expect("const update should reference a plan constant");
    assert_eq!(
        constant.value,
        PlanConstantValue::Enum {
            value: "Active".to_owned()
        }
    );
}


#[test]
fn verifier_rejects_tampered_const_update_constant_ref() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let source_id = debug_entry_id(
        &plan.debug_map.source_routes,
        "source",
        "store.sources.filter_active.press",
    );
    let state_id = debug_entry_id(
        &plan.debug_map.state_slots,
        "state",
        "store.selected_filter",
    );
    let op = plan
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == state_id)
        })
        .expect("filter active route should lower to one update op");
    let PlanOpKind::UpdateBranch {
        expression_kind: PlanExpressionKind::Const,
        update_constant_id,
        ..
    } = &mut op.kind
    else {
        panic!("filter active should lower as a typed Const update branch: {op:#?}");
    };
    *update_constant_id = Some(PlanConstantId(usize::MAX));

    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "fail");
    assert!(
        verification.checks.iter().any(|check| {
            check.id == "constant-refs-resolve-and-match-storage-types" && !check.pass
        }),
        "tampered const update ref should fail verifier: {verification:#?}"
    );
}


#[test]
fn todomvc_append_lowers_to_typed_trigger_fields_and_initial_rows() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let title_to_add_id = debug_entry_id(
        &plan.debug_map.fields,
        "field",
        "store.title_to_add",
    );
    let todos_id = debug_entry_id(&plan.debug_map.list_slots, "list", "todos");
    let append_op = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::ListOperations)
        .flat_map(|region| region.ops.iter())
        .find(|op| {
            matches!(&op.output, Some(ValueRef::List(id)) if id.0 == todos_id)
                && matches!(
                    &op.kind,
                    PlanOpKind::ListOperation {
                        operation_kind: PlanListOperationKind::Append,
                        ..
                    }
                )
        })
        .expect("TodoMVC todos append should lower to one list op");
    let PlanOpKind::ListOperation {
        operation_kind: PlanListOperationKind::Append,
        append: Some(append),
        ..
    } = &append_op.kind
    else {
        panic!("append op should carry typed append details: {append_op:#?}");
    };
    assert_eq!(append.trigger, ValueRef::Field(FieldId(title_to_add_id)));
    assert!(append_op.inputs.contains(&append.trigger));
    assert_eq!(append.fields.len(), 1);
    assert_eq!(append.fields[0].name, "title");
    assert_eq!(append.fields[0].field_id, Some(FieldId(11)));
    assert_eq!(
        append.fields[0].value_ref,
        Some(ValueRef::Field(FieldId(title_to_add_id)))
    );
    assert_eq!(append.fields[0].constant_id, None);
    let list_slot = plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id.0 == todos_id)
        .expect("todos list slot should exist");
    assert_eq!(list_slot.initial_rows.len(), 4);
    assert_eq!(
        list_slot.initial_rows[0].fields[0].field_id,
        Some(FieldId(11))
    );
    assert_eq!(
        list_slot.initial_rows[0].fields[1].field_id,
        Some(FieldId(13))
    );
    assert_eq!(
        list_slot.initial_rows[0].fields[0].value,
        PlanConstantValue::Text {
            value: "Read documentation".to_owned()
        }
    );
    assert_eq!(
        list_slot.initial_rows[1].fields[1].value,
        PlanConstantValue::Bool { value: true }
    );
    assert!(
        verify_plan(&plan)
            .unwrap()
            .checks
            .iter()
            .any(|check| check.id == "list-initial-row-fields-resolve" && check.pass),
        "initial row refs should verify"
    );
    assert!(
        verify_plan(&plan)
            .unwrap()
            .checks
            .iter()
            .any(|check| check.id == "list-append-refs-resolve" && check.pass),
        "append refs should verify"
    );
}


#[test]
fn todomvc_remove_lowers_to_typed_source_and_predicate() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let todos_id = debug_entry_id(&plan.debug_map.list_slots, "list", "todos");
    let remove_source_id = debug_entry_id(
        &plan.debug_map.source_routes,
        "source",
        "todo.sources.remove_todo_button.press",
    );
    let clear_source_id = debug_entry_id(
        &plan.debug_map.source_routes,
        "source",
        "store.sources.clear_completed_button.press",
    );
    let completed_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "todo.completed");

    let remove_ops = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::ListOperations)
        .flat_map(|region| region.ops.iter())
        .filter(|op| {
            matches!(&op.output, Some(ValueRef::List(id)) if id.0 == todos_id)
                && matches!(
                    &op.kind,
                    PlanOpKind::ListOperation {
                        operation_kind: PlanListOperationKind::Remove,
                        ..
                    }
                )
        })
        .collect::<Vec<_>>();
    assert_eq!(remove_ops.len(), 2);

    let row_remove = remove_ops
        .iter()
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::ListOperation {
                    remove: Some(PlanListRemove {
                        source: ValueRef::Source(source_id),
                        predicate: PlanListRemovePredicate::AlwaysTrue,
                    }),
                    ..
                } if source_id.0 == remove_source_id
            )
        })
        .expect("row remove source should lower to typed AlwaysTrue remove metadata");
    let PlanOpKind::ListOperation {
        remove: Some(row_remove_plan),
        ..
    } = &row_remove.kind
    else {
        panic!("row remove op should carry typed remove metadata");
    };
    assert!(row_remove.inputs.contains(&row_remove_plan.source));

    let clear_remove = remove_ops
        .iter()
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::ListOperation {
                    remove: Some(PlanListRemove {
                        source: ValueRef::Source(source_id),
                        predicate: PlanListRemovePredicate::RowFieldBool {
                            input: ValueRef::State(state_id),
                        },
                    }),
                    ..
                } if source_id.0 == clear_source_id && state_id.0 == completed_state_id
            )
        })
        .expect("clear-completed should lower to typed row-field bool remove predicate");
    let PlanOpKind::ListOperation {
        remove: Some(clear_remove_plan),
        ..
    } = &clear_remove.kind
    else {
        panic!("clear-completed op should carry typed remove metadata");
    };
    assert!(clear_remove.inputs.contains(&clear_remove_plan.source));
    assert!(matches!(
        &clear_remove_plan.predicate,
        PlanListRemovePredicate::RowFieldBool {
            input: ValueRef::State(state_id),
        } if state_id.0 == completed_state_id
    ));
    assert!(
        clear_remove
            .inputs
            .contains(&ValueRef::State(StateId(completed_state_id)))
    );
    assert!(
        verify_plan(&plan)
            .unwrap()
            .checks
            .iter()
            .any(|check| check.id == "list-remove-refs-resolve" && check.pass),
        "remove refs should verify"
    );
}


#[test]
fn todomvc_counts_and_has_completed_lower_to_typed_refs() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let todos_id = debug_entry_id(&plan.debug_map.list_slots, "list", "todos");
    let completed_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "todo.completed");
    let active_count_id = debug_entry_id(
        &plan.debug_map.fields,
        "field",
        "store.active_count",
    );
    let completed_count_id = debug_entry_id(
        &plan.debug_map.fields,
        "field",
        "store.completed_count",
    );
    let has_completed_id = debug_entry_id(
        &plan.debug_map.fields,
        "field",
        "store.has_completed",
    );

    let count_ops = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::ListOperations)
        .flat_map(|region| region.ops.iter())
        .filter(|op| {
            matches!(&op.output, Some(ValueRef::List(id)) if id.0 == todos_id)
                && matches!(
                    &op.kind,
                    PlanOpKind::ListOperation {
                        operation_kind: PlanListOperationKind::Count,
                        ..
                    }
                )
        })
        .collect::<Vec<_>>();
    assert_eq!(count_ops.len(), 2);
    assert!(count_ops.iter().any(|op| {
        matches!(
            &op.kind,
            PlanOpKind::ListOperation {
                count: Some(PlanListCount {
                    target: ValueRef::Field(field_id),
                    predicate: PlanListRemovePredicate::RowFieldBoolNot {
                        input: ValueRef::State(state_id),
                    },
                }),
                ..
            } if field_id.0 == active_count_id && state_id.0 == completed_state_id
        )
    }));
    assert!(count_ops.iter().any(|op| {
        matches!(
            &op.kind,
            PlanOpKind::ListOperation {
                count: Some(PlanListCount {
                    target: ValueRef::Field(field_id),
                    predicate: PlanListRemovePredicate::RowFieldBool {
                        input: ValueRef::State(state_id),
                    },
                }),
                ..
            } if field_id.0 == completed_count_id && state_id.0 == completed_state_id
        )
    }));

    let has_completed = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .find(|op| {
            matches!(op.output, Some(ValueRef::Field(field_id)) if field_id.0 == has_completed_id)
        })
        .expect("store.has_completed derived op should lower");
    assert!(matches!(
        &has_completed.kind,
        PlanOpKind::DerivedValue {
            derived_kind: PlanDerivedKind::Pure,
            expression: Some(PlanDerivedExpression::NumberCompareConst {
                left: ValueRef::Field(field_id),
                op,
                right: 0,
            }),
            ..
        } if field_id.0 == completed_count_id && op == ">"
    ));

    let checks = verify_plan(&plan).unwrap().checks;
    assert!(
        checks
            .iter()
            .any(|check| check.id == "list-count-refs-resolve" && check.pass),
        "count refs should verify"
    );
    assert!(
        checks
            .iter()
            .any(|check| check.id == "derived-expression-refs-resolve" && check.pass),
        "derived numeric compare refs should verify"
    );
}


#[test]
fn todomvc_typed_remove_and_count_list_ops_are_cpu_supported() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

    let unsupported_list_ops = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::ListOperations)
        .flat_map(|region| region.ops.iter())
        .filter(|op| {
            !cpu_plan_executor_supports_whole_plan_op(
                &plan.storage_layout.scalar_slots,
                &plan.storage_layout.list_slots,
                &plan.constants,
                op,
                &BTreeSet::new(),
                &BTreeSet::new(),
                &BTreeSet::new(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(unsupported_list_ops.len(), 0);
    assert!(
        plan.capability_summary
            .cpu_plan_executor_unsupported_op_count
            <= 2,
        "TodoMVC list remove/count/retain ops should no longer be counted unsupported once typed retain execution exists"
    );
}


#[test]
fn todomvc_title_to_add_lowers_to_typed_source_key_trim_expression() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let title_to_add_id = debug_entry_id(
        &plan.debug_map.fields,
        "field",
        "store.title_to_add",
    );
    let key_down_source_id = debug_entry_id(
        &plan.debug_map.source_routes,
        "source",
        "store.sources.new_todo_input.key_down",
    );
    let new_text_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "store.new_todo_text");
    let op = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter())
        .find(|op| matches!(&op.output, Some(ValueRef::Field(id)) if id.0 == title_to_add_id))
        .expect("title_to_add derived op should exist");
    let PlanOpKind::DerivedValue {
        derived_kind: PlanDerivedKind::SourceEventTransform,
        expression:
            Some(PlanDerivedExpression::SourceKeyTextTrimNonEmpty {
                source_id,
                key_field,
                required_key,
                state,
                skip_empty,
            }),
        ..
    } = &op.kind
    else {
        panic!("title_to_add should lower to a typed source-key trim expression: {op:#?}");
    };
    assert_eq!(source_id.0, key_down_source_id);
    assert_eq!(key_field, &SourcePayloadField::Key);
    assert_eq!(required_key, "Enter");
    assert_eq!(state, &ValueRef::State(StateId(new_text_state_id)));
    assert!(*skip_empty);
    assert!(
        op.inputs
            .contains(&ValueRef::Source(SourceId(key_down_source_id)))
    );
    assert!(op.inputs.contains(&ValueRef::SourcePayload {
        source_id: SourceId(key_down_source_id),
        field: SourcePayloadField::Key,
    }));
    assert!(
        op.inputs
            .contains(&ValueRef::State(StateId(new_text_state_id)))
    );
    assert!(
        verify_plan(&plan)
            .unwrap()
            .checks
            .iter()
            .any(|check| check.id == "derived-expression-refs-resolve" && check.pass),
        "derived expression refs should verify"
    );
}


#[test]
fn todomvc_row_bool_not_derived_values_lower_to_typed_inputs() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let not_editing_id = debug_entry_id(&plan.debug_map.fields, "field", "todo.not_editing");
    let not_completed_id = debug_entry_id(&plan.debug_map.fields, "field", "todo.not_completed");
    let editing_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "todo.editing");
    let completed_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "todo.completed");

    for (output_id, input_state_id) in [
        (not_editing_id, editing_state_id),
        (not_completed_id, completed_state_id),
    ] {
        let op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::DerivedEvaluation)
            .flat_map(|region| region.ops.iter())
            .find(|op| matches!(&op.output, Some(ValueRef::Field(id)) if id.0 == output_id))
            .expect("typed Bool/not derived op should exist");
        assert!(op.indexed);
        assert!(matches!(
            &op.kind,
            PlanOpKind::DerivedValue {
                derived_kind: PlanDerivedKind::Pure,
                expression: Some(PlanDerivedExpression::BoolNot {
                    input: ValueRef::State(state_id)
                }),
                ..
            } if state_id.0 == input_state_id
        ));
        assert!(
            op.inputs
                .contains(&ValueRef::State(StateId(input_state_id)))
        );
    }

    assert!(
        verify_plan(&plan)
            .unwrap()
            .checks
            .iter()
            .any(|check| check.id == "derived-expression-refs-resolve" && check.pass),
        "Bool/not derived expression refs should verify"
    );
}


#[test]
fn verifier_rejects_tampered_derived_expression_ref() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let op = plan
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::DerivedEvaluation)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::DerivedValue {
                    expression: Some(PlanDerivedExpression::SourceKeyTextTrimNonEmpty { .. }),
                    ..
                }
            )
        })
        .expect("typed derived expression should exist");
    op.inputs
        .retain(|input| !matches!(input, ValueRef::State(StateId(0))));

    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "fail");
    assert!(
        verification
            .checks
            .iter()
            .any(|check| check.id == "derived-expression-refs-resolve" && !check.pass),
        "tampered derived expression refs should fail verifier: {verification:#?}"
    );
}


#[test]
fn verifier_rejects_tampered_append_field_ref() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let append_op = plan
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::ListOperations)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::ListOperation {
                    operation_kind: PlanListOperationKind::Append,
                    ..
                }
            )
        })
        .expect("append op should exist");
    let PlanOpKind::ListOperation {
        append: Some(append),
        ..
    } = &mut append_op.kind
    else {
        panic!("append op should carry append details");
    };
    append.fields[0].field_id = None;

    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "fail");
    assert!(
        verification
            .checks
            .iter()
            .any(|check| check.id == "list-append-refs-resolve" && !check.pass),
        "tampered append refs should fail verifier: {verification:#?}"
    );
}


#[test]
fn verifier_rejects_tampered_remove_source_ref() {
    let parsed = boon_parser::parse_source(
        "examples/todomvc.bn",
        include_str!("../../../../examples/todomvc.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let remove_source_id = debug_entry_id(
        &plan.debug_map.source_routes,
        "source",
        "todo.sources.remove_todo_button.press",
    );
    let remove_op = plan
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::ListOperations)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::ListOperation {
                    operation_kind: PlanListOperationKind::Remove,
                    remove: Some(PlanListRemove {
                        source: ValueRef::Source(source_id),
                        ..
                    }),
                    ..
                } if source_id.0 == remove_source_id
            )
        })
        .expect("row remove op should exist");
    let PlanOpKind::ListOperation {
        remove: Some(remove),
        ..
    } = &mut remove_op.kind
    else {
        panic!("row remove op should carry remove metadata");
    };
    remove.source = ValueRef::Source(SourceId(usize::MAX));

    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "fail");
    assert!(
        verification
            .checks
            .iter()
            .any(|check| check.id == "list-remove-refs-resolve" && !check.pass),
        "tampered remove refs should fail verifier: {verification:#?}"
    );
}

fn debug_entry_id(entries: &[DebugEntry], prefix: &str, label: &str) -> usize {
    entries
        .iter()
        .find(|entry| entry.label == label)
        .and_then(|entry| {
            entry
                .id
                .strip_prefix(prefix)
                .and_then(|suffix| suffix.strip_prefix(':'))
                .and_then(|suffix| suffix.parse::<usize>().ok())
        })
        .unwrap_or_else(|| panic!("missing debug entry `{prefix}:{label}`"))
}

fn bytes_numeric_fixture_plan() -> MachinePlan {
    let parsed = boon_parser::parse_source(
        "examples/bytes_numeric_plan_ops.bn",
        include_str!("../../../../examples/bytes_numeric_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    assert_eq!(verify_plan(&plan).unwrap().status, "pass");
    plan
}

fn update_op_id_for(plan: &MachinePlan, source_label: &str, target_label: &str) -> PlanOpId {
    let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", source_label);
    let target_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", target_label);
    plan.regions
        .iter()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter())
        .find(|op| {
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
        })
        .map(|op| op.id)
        .unwrap_or_else(|| panic!("missing update op for {source_label} -> {target_label}"))
}

fn op_by_id(plan: &MachinePlan, op_id: PlanOpId) -> &PlanOp {
    plan.regions
        .iter()
        .flat_map(|region| region.ops.iter())
        .find(|op| op.id == op_id)
        .unwrap_or_else(|| panic!("missing op {op_id:?}"))
}

fn ordered_constant_id(plan: &MachinePlan, op_id: PlanOpId, index: usize) -> PlanConstantId {
    let op = op_by_id(plan, op_id);
    let ordered_inputs = update_branch_ordered_inputs(op);
    match ordered_inputs.get(index) {
        Some(ValueRef::Constant(constant_id)) => *constant_id,
        other => panic!("expected ordered constant input {index} for {op_id:?}, got {other:?}"),
    }
}

fn set_number_constant(plan: &mut MachinePlan, constant_id: PlanConstantId, value: i64) {
    let constant = plan
        .constants
        .iter_mut()
        .find(|constant| constant.id == constant_id)
        .unwrap_or_else(|| panic!("missing constant {constant_id:?}"));
    constant.value = PlanConstantValue::Number { value };
}

fn assert_numeric_plan_rejected(plan: &MachinePlan, reason: &str) {
    let verification = verify_plan(plan).unwrap();
    assert_eq!(
        verification.status, "fail",
        "{reason} must reject the MachinePlan: {verification:#?}"
    );
    assert!(
        verification.checks.iter().any(|check| matches!(
            check.id.as_str(),
            "constant-refs-resolve-and-match-storage-types" | "capability-summary-derived-counts"
        ) && !check.pass),
        "{reason} should fail a typed constant/capability verifier check: {verification:#?}"
    );
}
