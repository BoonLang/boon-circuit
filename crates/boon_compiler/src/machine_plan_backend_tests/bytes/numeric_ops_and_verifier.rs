// Included by `../bytes.rs`.

// test: bytes_numeric_updates_lower_to_ordered_typed_executable_plan_ops
#[test]
fn bytes_numeric_updates_lower_to_ordered_typed_executable_plan_ops() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_numeric_plan_ops.bn",
        include_str!("../../../../../examples/bytes_numeric_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "pass");
    assert!(plan.capability_summary.cpu_plan_executor_complete);
    assert_eq!(
        plan.capability_summary
            .cpu_plan_executor_unsupported_op_count,
        0
    );
    assert_eq!(plan.capability_summary.executable_string_path_count, 0);
    assert_eq!(plan.capability_summary.runtime_ast_dependency_count, 0);
    assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

    let constant_value = |constant_id: PlanConstantId| {
        plan.constants
            .iter()
            .find(|constant| constant.id == constant_id)
            .map(|constant| &constant.value)
            .unwrap_or_else(|| panic!("missing plan constant {constant_id:?}"))
    };
    let number_constant = |value_ref: &ValueRef| {
        let ValueRef::Constant(constant_id) = value_ref else {
            panic!("expected numeric constant ref, got {value_ref:?}");
        };
        match constant_value(*constant_id) {
            PlanConstantValue::Number { value } => *value,
            other => panic!("expected numeric constant, got {other:?}"),
        }
    };
    let text_constant = |value_ref: &ValueRef| {
        let ValueRef::Constant(constant_id) = value_ref else {
            panic!("expected text constant ref, got {value_ref:?}");
        };
        match constant_value(*constant_id) {
            PlanConstantValue::Text { value } => value.as_str(),
            other => panic!("expected text constant, got {other:?}"),
        }
    };
    let op_for = |source_label: &str, target_label: &str| {
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
            .unwrap_or_else(|| panic!("missing numeric op for {source_label} -> {target_label}"))
    };
    let assert_numeric_branch = |source_label: &str,
                                 target_label: &str,
                                 input_label: &str,
                                 expected_kind: PlanExpressionKind,
                                 expected_offset: i64,
                                 expected_byte_count: i64,
                                 expected_endian: &str,
                                 expected_value: Option<i64>| {
        let input_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", input_label);
        let op = op_for(source_label, target_label);
        let PlanOpKind::UpdateBranch {
            expression_kind,
            source_payload_field,
            update_constant_id,
            ordered_inputs,
            ..
        } = &op.kind
        else {
            panic!("expected update branch for {target_label}: {op:#?}");
        };
        assert_eq!(*expression_kind, expected_kind);
        assert_eq!(*source_payload_field, None);
        assert_eq!(*update_constant_id, None);
        match expected_value {
            Some(value) => {
                assert_eq!(ordered_inputs.len(), 5);
                assert_eq!(ordered_inputs[0], ValueRef::State(StateId(input_state_id)));
                assert_eq!(number_constant(&ordered_inputs[1]), expected_offset);
                assert_eq!(number_constant(&ordered_inputs[2]), expected_byte_count);
                assert_eq!(text_constant(&ordered_inputs[3]), expected_endian);
                assert_eq!(number_constant(&ordered_inputs[4]), value);
            }
            None => {
                assert_eq!(ordered_inputs.len(), 4);
                assert_eq!(ordered_inputs[0], ValueRef::State(StateId(input_state_id)));
                assert_eq!(number_constant(&ordered_inputs[1]), expected_offset);
                assert_eq!(number_constant(&ordered_inputs[2]), expected_byte_count);
                assert_eq!(text_constant(&ordered_inputs[3]), expected_endian);
            }
        }
    };

    assert_numeric_branch(
        "store.measure",
        "store.read_u16_le",
        "payload",
        PlanExpressionKind::BytesReadUnsigned,
        0,
        2,
        "Little",
        None,
    );
    assert_numeric_branch(
        "store.measure",
        "store.read_u16_be",
        "payload",
        PlanExpressionKind::BytesReadUnsigned,
        0,
        2,
        "Big",
        None,
    );
    assert_numeric_branch(
        "store.measure",
        "store.read_i16_be",
        "payload",
        PlanExpressionKind::BytesReadSigned,
        2,
        2,
        "Big",
        None,
    );
    assert_numeric_branch(
        "store.measure",
        "store.read_i8",
        "payload",
        PlanExpressionKind::BytesReadSigned,
        5,
        1,
        "Little",
        None,
    );
    assert_numeric_branch(
        "store.write",
        "store.written_unsigned",
        "payload",
        PlanExpressionKind::BytesWriteUnsigned,
        6,
        2,
        "Big",
        Some(4660),
    );
    assert_numeric_branch(
        "store.write",
        "store.written_signed",
        "payload",
        PlanExpressionKind::BytesWriteSigned,
        4,
        2,
        "Little",
        Some(-129),
    );
    assert_numeric_branch(
        "store.inspect",
        "store.written_unsigned_read",
        "store.written_unsigned",
        PlanExpressionKind::BytesReadUnsigned,
        6,
        2,
        "Big",
        None,
    );
    assert_numeric_branch(
        "store.inspect",
        "store.written_signed_read",
        "store.written_signed",
        PlanExpressionKind::BytesReadSigned,
        4,
        2,
        "Little",
        None,
    );

    let write_unsigned_op = op_for("store.write", "store.written_unsigned");
    let endian_constant_id = match &write_unsigned_op.kind {
        PlanOpKind::UpdateBranch { ordered_inputs, .. } => match ordered_inputs.get(3) {
            Some(ValueRef::Constant(constant_id)) => *constant_id,
            other => panic!("missing endian constant for numeric write: {other:?}"),
        },
        other => panic!("expected update branch for numeric write: {other:?}"),
    };
    let mut tampered = plan.clone();
    let constant = tampered
        .constants
        .iter_mut()
        .find(|constant| constant.id == endian_constant_id)
        .expect("tampered plan should contain numeric endian constant");
    constant.value = PlanConstantValue::Text {
        value: "Middle".to_owned(),
    };
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
        "unsupported endian constant must fail plan verification: {tampered_verification:#?}"
    );
}

// test: bytes_numeric_plan_verifier_rejects_invalid_operands_and_output_lengths
#[test]
fn bytes_numeric_plan_verifier_rejects_invalid_operands_and_output_lengths() {
    let plan = bytes_numeric_fixture_plan();

    let read_u16_le_op_id = update_op_id_for(&plan, "store.measure", "store.read_u16_le");
    let read_byte_count_constant_id = ordered_constant_id(&plan, read_u16_le_op_id, 2);
    let mut invalid_byte_count = plan.clone();
    set_number_constant(&mut invalid_byte_count, read_byte_count_constant_id, 3);
    assert_numeric_plan_rejected(&invalid_byte_count, "unsupported numeric byte_count");

    let read_offset_constant_id = ordered_constant_id(&plan, read_u16_le_op_id, 1);
    let mut out_of_bounds = plan.clone();
    set_number_constant(&mut out_of_bounds, read_offset_constant_id, 7);
    assert_numeric_plan_rejected(&out_of_bounds, "fixed input range out of bounds");

    let write_unsigned_op_id = update_op_id_for(&plan, "store.write", "store.written_unsigned");
    let write_unsigned_value_constant_id = ordered_constant_id(&plan, write_unsigned_op_id, 4);
    let mut unsigned_overflow = plan.clone();
    set_number_constant(
        &mut unsigned_overflow,
        write_unsigned_value_constant_id,
        65_536,
    );
    assert_numeric_plan_rejected(&unsigned_overflow, "unsigned numeric write overflow");

    let write_signed_op_id = update_op_id_for(&plan, "store.write", "store.written_signed");
    let write_signed_value_constant_id = ordered_constant_id(&plan, write_signed_op_id, 4);
    let mut signed_overflow = plan.clone();
    set_number_constant(&mut signed_overflow, write_signed_value_constant_id, 32_768);
    assert_numeric_plan_rejected(&signed_overflow, "signed numeric write overflow");

    let write_unsigned_output_state_id = match &op_by_id(&plan, write_unsigned_op_id).output {
        Some(ValueRef::State(state_id)) => *state_id,
        other => panic!("numeric write should target a state, got {other:?}"),
    };
    let mut fixed_length_mismatch = plan.clone();
    let slot = fixed_length_mismatch
        .storage_layout
        .scalar_slots
        .iter_mut()
        .find(|slot| slot.state_id == write_unsigned_output_state_id)
        .expect("numeric write output state should have a storage slot");
    slot.value_type = PlanValueType::Bytes { fixed_len: Some(7) };
    assert_numeric_plan_rejected(
        &fixed_length_mismatch,
        "numeric write output fixed length mismatch",
    );
}

