// Included by `../bytes.rs`.

// test: bytes_literal_lowers_to_executable_typed_storage_and_constant_payload
// test: repeated_plan_constants_are_interned_by_value
// test: bytes_length_update_lowers_to_typed_executable_plan_op
#[test]
fn bytes_length_update_lowers_to_typed_executable_plan_op() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_length_plan_ops.bn",
        include_str!("../../../../../examples/bytes_length_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "pass");
    assert!(
        plan.capability_summary.cpu_plan_executor_complete,
        "Bytes/length root-scalar fixture should be executable by the CPU PlanExecutor: {:#?}",
        plan.capability_summary
    );
    assert_eq!(
        plan.capability_summary
            .cpu_plan_executor_unsupported_op_count,
        0
    );
    assert_eq!(plan.capability_summary.executable_string_path_count, 0);
    assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

    let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.measure");
    let payload_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "payload");
    let byte_len_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.byte_len");
    let op = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter())
        .find(|op| {
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == byte_len_state_id)
        })
        .expect("store.measure should lower to a byte_len update branch");
    assert_eq!(op.unresolved_executable_ref_count, 0);
    assert!(
        op.inputs
            .iter()
            .any(|input| matches!(input, ValueRef::State(id) if id.0 == payload_state_id)),
        "Bytes/length input must be a typed state ref to top-level payload, not a string path: {op:#?}"
    );
    assert!(matches!(
        &op.kind,
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesLength,
            source_payload_field: None,
            update_constant_id: None,
            ..
        }
    ));

    let mut tampered = plan.clone();
    let tampered_op = tampered
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesLength,
                    ..
                }
            )
        })
        .expect("fixture should contain a Bytes/length update op");
    tampered_op.inputs = vec![
        ValueRef::Source(SourceId(source_id)),
        ValueRef::State(StateId(byte_len_state_id)),
    ];
    let tampered_verification = verify_plan(&tampered).unwrap();
    assert_eq!(tampered_verification.status, "fail");
    assert!(
        tampered_verification
            .checks
            .iter()
            .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "Bytes/length with a non-BYTES input must not satisfy CPU executor support: {tampered_verification:#?}"
    );
}

// test: bytes_is_empty_update_lowers_to_typed_executable_plan_op
#[test]
fn bytes_is_empty_update_lowers_to_typed_executable_plan_op() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_is_empty_plan_ops.bn",
        include_str!("../../../../../examples/bytes_is_empty_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "pass");
    assert!(
        plan.capability_summary.cpu_plan_executor_complete,
        "Bytes/is_empty root-scalar fixture should be executable by the CPU PlanExecutor: {:#?}",
        plan.capability_summary
    );
    assert_eq!(
        plan.capability_summary
            .cpu_plan_executor_unsupported_op_count,
        0
    );
    assert_eq!(plan.capability_summary.executable_string_path_count, 0);
    assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

    let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.measure");
    let empty_payload_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "empty_payload");
    let filled_payload_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "filled_payload");
    let empty_target_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "store.empty_is_empty");
    let filled_target_state_id = debug_entry_id(
        &plan.debug_map.state_slots,
        "state",
        "store.filled_is_empty",
    );

    for (target_state_id, payload_state_id, target_label) in [
        (
            empty_target_state_id,
            empty_payload_state_id,
            "store.empty_is_empty",
        ),
        (
            filled_target_state_id,
            filled_payload_state_id,
            "store.filled_is_empty",
        ),
    ] {
        let op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                    && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
            })
            .unwrap_or_else(|| {
                panic!("store.measure should lower to a bytes_is_empty update for {target_label}")
            });
        assert_eq!(op.unresolved_executable_ref_count, 0);
        assert!(
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::State(id) if id.0 == payload_state_id)),
            "Bytes/is_empty input must be a typed BYTES state ref, not a string path: {op:#?}"
        );
        assert!(matches!(
            &op.kind,
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesIsEmpty,
                source_payload_field: None,
                update_constant_id: None,
                ..
            }
        ));
    }

    let mut tampered_input = plan.clone();
    let tampered_op = tampered_input
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesIsEmpty,
                    ..
                }
            )
        })
        .expect("fixture should contain a Bytes/is_empty update op");
    tampered_op.inputs = vec![
        ValueRef::Source(SourceId(source_id)),
        ValueRef::State(StateId(empty_target_state_id)),
    ];
    let tampered_input_verification = verify_plan(&tampered_input).unwrap();
    assert_eq!(tampered_input_verification.status, "fail");
    assert!(
        tampered_input_verification
            .checks
            .iter()
            .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "Bytes/is_empty with a non-BYTES input must not satisfy CPU executor support: {tampered_input_verification:#?}"
    );

    let mut tampered_output = plan.clone();
    let tampered_op = tampered_output
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesIsEmpty,
                    ..
                }
            )
        })
        .expect("fixture should contain a Bytes/is_empty update op");
    tampered_op.output = Some(ValueRef::State(StateId(empty_payload_state_id)));
    let tampered_output_verification = verify_plan(&tampered_output).unwrap();
    assert_eq!(tampered_output_verification.status, "fail");
    assert!(
        tampered_output_verification
            .checks
            .iter()
            .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "Bytes/is_empty with a non-BOOL output must not satisfy CPU executor support: {tampered_output_verification:#?}"
    );
}

// test: bytes_get_update_lowers_to_typed_executable_plan_op
#[test]
fn bytes_get_update_lowers_to_typed_executable_plan_op() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_get_plan_ops.bn",
        include_str!("../../../../../examples/bytes_get_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "pass");
    assert!(
        plan.capability_summary.cpu_plan_executor_complete,
        "Bytes/get root-scalar fixture should be executable by the CPU PlanExecutor: {:#?}",
        plan.capability_summary
    );
    assert_eq!(
        plan.capability_summary
            .cpu_plan_executor_unsupported_op_count,
        0
    );
    assert_eq!(plan.capability_summary.executable_string_path_count, 0);
    assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

    let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.measure");
    let payload_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "payload");
    let target_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "store.selected_byte");
    let target_slot = plan
        .storage_layout
        .scalar_slots
        .iter()
        .find(|slot| slot.state_id.0 == target_state_id)
        .expect("selected_byte storage slot should lower");
    assert_eq!(target_slot.value_type, PlanValueType::Byte);

    let op = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter())
        .find(|op| {
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
        })
        .expect("store.measure should lower to a bytes_get update branch");
    assert_eq!(op.unresolved_executable_ref_count, 0);
    assert!(
        op.inputs
            .iter()
            .any(|input| matches!(input, ValueRef::State(id) if id.0 == payload_state_id)),
        "Bytes/get input must be a typed BYTES state ref, not a string path: {op:#?}"
    );
    let PlanOpKind::UpdateBranch {
        expression_kind: PlanExpressionKind::BytesGet,
        source_payload_field: None,
        update_constant_id: Some(index_constant_id),
        ..
    } = &op.kind
    else {
        panic!("Bytes/get op should carry a typed index constant: {op:#?}");
    };
    assert_eq!(
        plan.constants
            .iter()
            .find(|constant| constant.id == *index_constant_id)
            .map(|constant| &constant.value),
        Some(&PlanConstantValue::Number { value: 2 })
    );

    let mut tampered_input = plan.clone();
    let tampered_op = tampered_input
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesGet,
                    ..
                }
            )
        })
        .expect("fixture should contain a Bytes/get update op");
    tampered_op.inputs = vec![
        ValueRef::Source(SourceId(source_id)),
        ValueRef::State(StateId(target_state_id)),
    ];
    let tampered_input_verification = verify_plan(&tampered_input).unwrap();
    assert_eq!(tampered_input_verification.status, "fail");
    assert!(
        tampered_input_verification
            .checks
            .iter()
            .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "Bytes/get with a non-BYTES input must not satisfy CPU executor support: {tampered_input_verification:#?}"
    );

    let mut tampered_output = plan.clone();
    let tampered_op = tampered_output
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesGet,
                    ..
                }
            )
        })
        .expect("fixture should contain a Bytes/get update op");
    tampered_op.output = Some(ValueRef::State(StateId(payload_state_id)));
    let tampered_output_verification = verify_plan(&tampered_output).unwrap();
    assert_eq!(tampered_output_verification.status, "fail");
    assert!(
        tampered_output_verification
            .checks
            .iter()
            .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "Bytes/get with a non-BYTE output must not satisfy CPU executor support: {tampered_output_verification:#?}"
    );

    let mut tampered_index = plan.clone();
    let tampered_op = tampered_index
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesGet,
                    ..
                }
            )
        })
        .expect("fixture should contain a Bytes/get update op");
    let PlanOpKind::UpdateBranch {
        update_constant_id, ..
    } = &mut tampered_op.kind
    else {
        unreachable!()
    };
    *update_constant_id = None;
    let tampered_index_verification = verify_plan(&tampered_index).unwrap();
    assert_eq!(tampered_index_verification.status, "fail");
    assert!(
        tampered_index_verification
            .checks
            .iter()
            .any(
                |check| check.id == "constant-refs-resolve-and-match-storage-types" && !check.pass
            )
            || tampered_index_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "Bytes/get without a typed index constant must fail verifier support: {tampered_index_verification:#?}"
    );
}

// test: bytes_set_update_lowers_to_typed_executable_plan_op
#[test]
fn bytes_set_update_lowers_to_typed_executable_plan_op() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_set_plan_ops.bn",
        include_str!("../../../../../examples/bytes_set_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "pass");
    assert!(
        plan.capability_summary.cpu_plan_executor_complete,
        "Bytes/set root-scalar fixture should be executable by the CPU PlanExecutor: {:#?}",
        plan.capability_summary
    );
    assert_eq!(
        plan.capability_summary
            .cpu_plan_executor_unsupported_op_count,
        0
    );
    assert_eq!(plan.capability_summary.executable_string_path_count, 0);
    assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

    let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.patch");
    let input_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "left_payload");
    let target_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.patched");
    let target_slot = plan
        .storage_layout
        .scalar_slots
        .iter()
        .find(|slot| slot.state_id.0 == target_state_id)
        .expect("patched storage slot should lower");
    assert_eq!(
        target_slot.value_type,
        PlanValueType::Bytes { fixed_len: Some(4) }
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
                && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
        })
        .expect("store.patch should lower to a bytes_set update branch");
    assert_eq!(op.unresolved_executable_ref_count, 0);
    assert!(
        op.inputs
            .iter()
            .any(|input| matches!(input, ValueRef::State(id) if id.0 == input_state_id)),
        "Bytes/set input must be a typed BYTES state ref, not a string path: {op:#?}"
    );
    let PlanOpKind::UpdateBranch {
        expression_kind: PlanExpressionKind::BytesSet,
        ordered_inputs,
        source_payload_field: None,
        update_constant_id: None,
        ..
    } = &op.kind
    else {
        panic!("Bytes/set op should carry typed ordered operands: {op:#?}");
    };
    let [
        ValueRef::State(ordered_input),
        ValueRef::Constant(index_constant_id),
        ValueRef::Constant(value_constant_id),
    ] = ordered_inputs.as_slice()
    else {
        panic!("Bytes/set ordered operands should be state/index/value: {ordered_inputs:#?}");
    };
    assert_eq!(ordered_input.0, input_state_id);
    assert_eq!(
        plan.constants
            .iter()
            .find(|constant| constant.id == *index_constant_id)
            .map(|constant| &constant.value),
        Some(&PlanConstantValue::Number { value: 2 })
    );
    assert_eq!(
        plan.constants
            .iter()
            .find(|constant| constant.id == *value_constant_id)
            .map(|constant| &constant.value),
        Some(&PlanConstantValue::Byte { value: 0xaa })
    );

    let mut tampered_missing_value = plan.clone();
    let tampered_op = tampered_missing_value
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesSet,
                    ..
                }
            )
        })
        .expect("fixture should contain a Bytes/set update op");
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &mut tampered_op.kind else {
        unreachable!()
    };
    ordered_inputs.pop();
    let tampered_missing_value_verification = verify_plan(&tampered_missing_value).unwrap();
    assert_eq!(tampered_missing_value_verification.status, "fail");

    let mut tampered_value_type = plan.clone();
    let value_constant = tampered_value_type
        .constants
        .iter_mut()
        .find(|constant| constant.id == *value_constant_id)
        .expect("value constant should exist");
    value_constant.value = PlanConstantValue::Number { value: 170 };
    let tampered_value_type_verification = verify_plan(&tampered_value_type).unwrap();
    assert_eq!(tampered_value_type_verification.status, "fail");

    let mut tampered_oob_index = plan.clone();
    let index_constant = tampered_oob_index
        .constants
        .iter_mut()
        .find(|constant| constant.id == *index_constant_id)
        .expect("index constant should exist");
    index_constant.value = PlanConstantValue::Number { value: 4 };
    let tampered_oob_index_verification = verify_plan(&tampered_oob_index).unwrap();
    assert_eq!(tampered_oob_index_verification.status, "fail");
}

// test: bytes_equal_update_lowers_to_typed_executable_plan_op
#[test]
fn bytes_equal_update_lowers_to_typed_executable_plan_op() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_equal_plan_ops.bn",
        include_str!("../../../../../examples/bytes_equal_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "pass");
    assert!(
        plan.capability_summary.cpu_plan_executor_complete,
        "Bytes/equal root-scalar fixture should be executable by the CPU PlanExecutor: {:#?}",
        plan.capability_summary
    );
    assert_eq!(
        plan.capability_summary
            .cpu_plan_executor_unsupported_op_count,
        0
    );
    assert_eq!(plan.capability_summary.executable_string_path_count, 0);
    assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

    let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.measure");
    let payload_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "payload");
    let same_payload_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "same_payload");
    let different_payload_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "different_payload");
    let same_target_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.same");
    let different_target_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "store.different");

    for (target_state_id, right_state_id, target_label) in [
        (same_target_state_id, same_payload_state_id, "store.same"),
        (
            different_target_state_id,
            different_payload_state_id,
            "store.different",
        ),
    ] {
        let target_slot = plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id.0 == target_state_id)
            .unwrap_or_else(|| panic!("{target_label} storage slot should lower"));
        assert_eq!(target_slot.value_type, PlanValueType::Bool);

        let op = plan
            .regions
            .iter()
            .filter(|region| region.kind == RegionKind::UpdateBranches)
            .flat_map(|region| region.ops.iter())
            .find(|op| {
                op.inputs
                    .iter()
                    .any(|input| matches!(input, ValueRef::Source(id) if id.0 == source_id))
                    && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
            })
            .unwrap_or_else(|| {
                panic!("store.measure should lower to a bytes_equal update for {target_label}")
            });
        assert_eq!(op.unresolved_executable_ref_count, 0);
        assert!(
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::State(id) if id.0 == payload_state_id)),
            "Bytes/equal left input must be a typed BYTES state ref: {op:#?}"
        );
        assert!(
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::State(id) if id.0 == right_state_id)),
            "Bytes/equal right input must be a typed BYTES state ref: {op:#?}"
        );
        assert!(matches!(
            &op.kind,
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesEqual,
                source_payload_field: None,
                update_constant_id: None,
                ..
            }
        ));
    }

    let mut tampered_input = plan.clone();
    let tampered_op = tampered_input
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesEqual,
                    ..
                }
            )
        })
        .expect("fixture should contain a Bytes/equal update op");
    tampered_op.inputs = vec![
        ValueRef::Source(SourceId(source_id)),
        ValueRef::State(StateId(same_target_state_id)),
        ValueRef::State(StateId(same_payload_state_id)),
    ];
    let tampered_input_verification = verify_plan(&tampered_input).unwrap();
    assert_eq!(tampered_input_verification.status, "fail");
    assert!(
        tampered_input_verification
            .checks
            .iter()
            .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "Bytes/equal with a non-BYTES input must not satisfy CPU executor support: {tampered_input_verification:#?}"
    );

    let mut tampered_output = plan.clone();
    let tampered_op = tampered_output
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesEqual,
                    ..
                }
            )
        })
        .expect("fixture should contain a Bytes/equal update op");
    tampered_op.output = Some(ValueRef::State(StateId(payload_state_id)));
    let tampered_output_verification = verify_plan(&tampered_output).unwrap();
    assert_eq!(tampered_output_verification.status, "fail");
    assert!(
        tampered_output_verification
            .checks
            .iter()
            .any(
                |check| check.id == "constant-refs-resolve-and-match-storage-types" && !check.pass
            )
            || tampered_output_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "Bytes/equal with a non-BOOL output must fail verifier support: {tampered_output_verification:#?}"
    );

    let mut tampered_constant = plan.clone();
    let tampered_op = tampered_constant
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesEqual,
                    ..
                }
            )
        })
        .expect("fixture should contain a Bytes/equal update op");
    let PlanOpKind::UpdateBranch {
        update_constant_id, ..
    } = &mut tampered_op.kind
    else {
        unreachable!()
    };
    *update_constant_id = Some(PlanConstantId(0));
    let tampered_constant_verification = verify_plan(&tampered_constant).unwrap();
    assert_eq!(tampered_constant_verification.status, "fail");
    assert!(
        tampered_constant_verification
            .checks
            .iter()
            .any(
                |check| check.id == "constant-refs-resolve-and-match-storage-types" && !check.pass
            )
            || tampered_constant_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "Bytes/equal with an update constant must fail verifier support: {tampered_constant_verification:#?}"
    );
}

// test: bytes_concat_update_lowers_to_ordered_typed_executable_plan_op
#[test]
fn bytes_concat_update_lowers_to_ordered_typed_executable_plan_op() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_concat_plan_ops.bn",
        include_str!("../../../../../examples/bytes_concat_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "pass");
    assert!(
        plan.capability_summary.cpu_plan_executor_complete,
        "Bytes/concat root-scalar fixture should be executable by the CPU PlanExecutor: {:#?}",
        plan.capability_summary
    );
    assert_eq!(
        plan.capability_summary
            .cpu_plan_executor_unsupported_op_count,
        0
    );
    assert_eq!(plan.capability_summary.executable_string_path_count, 0);
    assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

    let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.measure");
    let left_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "left_payload");
    let right_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "right_payload");
    let joined_pipe_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "store.joined_pipe");
    let joined_call_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "store.joined_call");

    for (target_state_id, target_label) in [
        (joined_pipe_state_id, "store.joined_pipe"),
        (joined_call_state_id, "store.joined_call"),
    ] {
        let target_slot = plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id.0 == target_state_id)
            .unwrap_or_else(|| panic!("{target_label} storage slot should lower"));
        assert_eq!(
            target_slot.value_type,
            PlanValueType::Bytes { fixed_len: Some(5) }
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
                    && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
            })
            .unwrap_or_else(|| {
                panic!("store.measure should lower to a bytes_concat update for {target_label}")
            });
        assert_eq!(op.unresolved_executable_ref_count, 0);
        assert!(matches!(
            &op.kind,
            PlanOpKind::UpdateBranch {
                expression_kind: PlanExpressionKind::BytesConcat,
                source_payload_field: None,
                update_constant_id: None,
                ordered_inputs,
                ..
            } if ordered_inputs == &vec![
                ValueRef::State(StateId(left_state_id)),
                ValueRef::State(StateId(right_state_id)),
            ]
        ));
    }

    let mut tampered_missing_order = plan.clone();
    let tampered_op = tampered_missing_order
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesConcat,
                    ..
                }
            )
        })
        .expect("fixture should contain a Bytes/concat update op");
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &mut tampered_op.kind else {
        unreachable!()
    };
    ordered_inputs.clear();
    let tampered_missing_order_verification = verify_plan(&tampered_missing_order).unwrap();
    assert_eq!(tampered_missing_order_verification.status, "fail");
    assert!(
        tampered_missing_order_verification
            .checks
            .iter()
            .any(
                |check| check.id == "constant-refs-resolve-and-match-storage-types" && !check.pass
            )
            || tampered_missing_order_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "Bytes/concat without ordered operands must fail verifier support: {tampered_missing_order_verification:#?}"
    );

    let mut tampered_fixed_len = plan.clone();
    let target_slot = tampered_fixed_len
        .storage_layout
        .scalar_slots
        .iter_mut()
        .find(|slot| slot.state_id.0 == joined_pipe_state_id)
        .expect("joined_pipe storage slot should lower");
    target_slot.value_type = PlanValueType::Bytes { fixed_len: Some(4) };
    let tampered_fixed_len_verification = verify_plan(&tampered_fixed_len).unwrap();
    assert_eq!(tampered_fixed_len_verification.status, "fail");
    assert!(
        tampered_fixed_len_verification
            .checks
            .iter()
            .any(
                |check| check.id == "constant-refs-resolve-and-match-storage-types" && !check.pass
            )
            || tampered_fixed_len_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "Bytes/concat fixed output length mismatch must fail verifier support: {tampered_fixed_len_verification:#?}"
    );
}

// test: bytes_slice_take_drop_updates_lower_to_typed_executable_plan_ops
#[test]
fn bytes_slice_take_drop_updates_lower_to_typed_executable_plan_ops() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_slice_take_drop_plan_ops.bn",
        include_str!("../../../../../examples/bytes_slice_take_drop_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "pass");
    assert!(
        plan.capability_summary.cpu_plan_executor_complete,
        "Bytes/slice/take/drop fixture should be executable by the CPU PlanExecutor: {:#?}",
        plan.capability_summary
    );
    assert_eq!(
        plan.capability_summary
            .cpu_plan_executor_unsupported_op_count,
        0
    );
    assert_eq!(plan.capability_summary.executable_string_path_count, 0);
    assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

    let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.split");
    let payload_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "payload");
    let sliced_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.sliced");
    let taken_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.taken");
    let dropped_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.dropped");

    let op_for = |target_state_id: usize| {
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
            .unwrap_or_else(|| {
                panic!("store.split should lower update for state {target_state_id}")
            })
    };

    let sliced_op = op_for(sliced_state_id);
    assert!(matches!(
        &sliced_op.kind,
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesSlice,
            source_payload_field: None,
            update_constant_id: None,
            ordered_inputs,
            ..
        } if matches!(
            ordered_inputs.as_slice(),
            [
                ValueRef::State(state_id),
                ValueRef::Constant(_),
                ValueRef::Constant(_)
            ] if state_id.0 == payload_state_id
        )
    ));
    let PlanOpKind::UpdateBranch { ordered_inputs, .. } = &sliced_op.kind else {
        unreachable!()
    };
    let [
        _,
        ValueRef::Constant(offset_id),
        ValueRef::Constant(slice_count_id),
    ] = ordered_inputs.as_slice()
    else {
        panic!("slice op should carry ordered constants: {sliced_op:#?}");
    };
    assert_eq!(
        plan.constants
            .iter()
            .find(|constant| constant.id == *offset_id)
            .map(|constant| &constant.value),
        Some(&PlanConstantValue::Number { value: 1 })
    );
    assert_eq!(
        plan.constants
            .iter()
            .find(|constant| constant.id == *slice_count_id)
            .map(|constant| &constant.value),
        Some(&PlanConstantValue::Number { value: 3 })
    );

    let taken_op = op_for(taken_state_id);
    assert!(matches!(
        &taken_op.kind,
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesTake,
            ordered_inputs,
            ..
        } if matches!(
            ordered_inputs.as_slice(),
            [ValueRef::State(state_id), ValueRef::Constant(_)] if state_id.0 == payload_state_id
        )
    ));
    let dropped_op = op_for(dropped_state_id);
    assert!(matches!(
        &dropped_op.kind,
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesDrop,
            ordered_inputs,
            ..
        } if matches!(
            ordered_inputs.as_slice(),
            [ValueRef::State(state_id), ValueRef::Constant(_)] if state_id.0 == payload_state_id
        )
    ));

    let mut tampered_fixed_len = plan.clone();
    let target_slot = tampered_fixed_len
        .storage_layout
        .scalar_slots
        .iter_mut()
        .find(|slot| slot.state_id.0 == sliced_state_id)
        .expect("sliced storage slot should lower");
    target_slot.value_type = PlanValueType::Bytes { fixed_len: Some(2) };
    let tampered_fixed_len_verification = verify_plan(&tampered_fixed_len).unwrap();
    assert_eq!(tampered_fixed_len_verification.status, "fail");
    assert!(
        tampered_fixed_len_verification
            .checks
            .iter()
            .any(
                |check| check.id == "constant-refs-resolve-and-match-storage-types" && !check.pass
            )
            || tampered_fixed_len_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "Bytes/slice fixed output length mismatch must fail verifier support: {tampered_fixed_len_verification:#?}"
    );
}

// test: bytes_dynamic_slice_count_lowers_to_typed_number_state_operand
#[test]
fn bytes_dynamic_slice_count_lowers_to_typed_number_state_operand() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_dynamic_slice_plan_ops.bn",
        include_str!("../../../../../examples/bytes_dynamic_slice_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "pass");
    assert!(
        plan.capability_summary.cpu_plan_executor_complete,
        "dynamic Bytes/slice fixture should be executable by the CPU PlanExecutor: {:#?}",
        plan.capability_summary
    );
    assert_eq!(plan.capability_summary.executable_string_path_count, 0);
    assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

    let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.split");
    let payload_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "payload");
    let count_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.slice_count");
    let output_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "store.dynamic_sliced");
    let output_slot = plan
        .storage_layout
        .scalar_slots
        .iter()
        .find(|slot| slot.state_id.0 == output_state_id)
        .expect("dynamic_sliced storage slot should lower");
    assert_eq!(
        output_slot.value_type,
        PlanValueType::Bytes { fixed_len: None }
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
                && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == output_state_id)
        })
        .expect("store.split should lower dynamic Bytes/slice update");
    let PlanOpKind::UpdateBranch {
        expression_kind,
        source_payload_field,
        update_constant_id,
        ordered_inputs,
        ..
    } = &op.kind
    else {
        panic!("dynamic slice should lower as an update branch: {op:#?}");
    };
    assert_eq!(*expression_kind, PlanExpressionKind::BytesSlice);
    assert_eq!(*source_payload_field, None);
    assert_eq!(*update_constant_id, None);
    assert_eq!(ordered_inputs.len(), 3);
    assert_eq!(
        ordered_inputs[0],
        ValueRef::State(StateId(payload_state_id))
    );
    assert!(matches!(ordered_inputs[1], ValueRef::Constant(_)));
    assert_eq!(ordered_inputs[2], ValueRef::State(StateId(count_state_id)));
    assert!(
        op.inputs
            .contains(&ValueRef::State(StateId(count_state_id)))
    );

    let mut tampered = plan.clone();
    let count_slot = tampered
        .storage_layout
        .scalar_slots
        .iter_mut()
        .find(|slot| slot.state_id.0 == count_state_id)
        .expect("count storage slot should lower");
    count_slot.value_type = PlanValueType::Text;
    let tampered_verification = verify_plan(&tampered).unwrap();
    assert_eq!(tampered_verification.status, "fail");
    assert!(
        tampered_verification.checks.iter().any(|check| check.id
            == "constant-refs-resolve-and-match-storage-types"
            && !check.pass)
            || tampered_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "dynamic Bytes/slice count must be a NUMBER state: {tampered_verification:#?}"
    );
}

