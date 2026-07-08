// Included by `../bytes.rs`.

// test: text_bytes_conversion_updates_lower_to_typed_executable_plan_ops
#[test]
fn text_bytes_conversion_updates_lower_to_typed_executable_plan_ops() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_text_conversion_plan_ops.bn",
        include_str!("../../../../../examples/bytes_text_conversion_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "pass");
    assert!(
        plan.capability_summary.cpu_plan_executor_complete,
        "text/BYTES conversion fixture should be executable by the CPU PlanExecutor: {:#?}",
        plan.capability_summary
    );
    assert_eq!(
        plan.capability_summary
            .cpu_plan_executor_unsupported_op_count,
        0
    );
    assert_eq!(plan.capability_summary.executable_string_path_count, 0);
    assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

    let encode_source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.encode");
    let decode_source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.decode");
    let text_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "text_payload");
    let encoded_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.encoded");
    let decoded_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.decoded");

    let encode_op = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter())
        .find(|op| {
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::Source(id) if id.0 == encode_source_id))
                && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == encoded_state_id)
        })
        .expect("store.encode should lower to a Text/to_bytes update branch");
    let PlanOpKind::UpdateBranch {
        expression_kind: PlanExpressionKind::TextToBytes,
        ordered_inputs,
        source_payload_field: None,
        update_constant_id: None,
        ..
    } = &encode_op.kind
    else {
        panic!("Text/to_bytes op should carry typed ordered operands: {encode_op:#?}");
    };
    let [
        ValueRef::State(ordered_text),
        ValueRef::Constant(encode_encoding_id),
    ] = ordered_inputs.as_slice()
    else {
        panic!("Text/to_bytes ordered operands should be state/encoding: {ordered_inputs:#?}");
    };
    assert_eq!(ordered_text.0, text_state_id);
    assert_eq!(
        plan.constants
            .iter()
            .find(|constant| constant.id == *encode_encoding_id)
            .map(|constant| &constant.value),
        Some(&PlanConstantValue::Text {
            value: "Utf8".to_owned()
        })
    );

    let decode_op = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter())
        .find(|op| {
            op.inputs
                .iter()
                .any(|input| matches!(input, ValueRef::Source(id) if id.0 == decode_source_id))
                && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == decoded_state_id)
        })
        .expect("store.decode should lower to a Bytes/to_text update branch");
    let PlanOpKind::UpdateBranch {
        expression_kind: PlanExpressionKind::BytesToText,
        ordered_inputs,
        source_payload_field: None,
        update_constant_id: None,
        ..
    } = &decode_op.kind
    else {
        panic!("Bytes/to_text op should carry typed ordered operands: {decode_op:#?}");
    };
    let [
        ValueRef::State(ordered_bytes),
        ValueRef::Constant(decode_encoding_id),
    ] = ordered_inputs.as_slice()
    else {
        panic!("Bytes/to_text ordered operands should be state/encoding: {ordered_inputs:#?}");
    };
    assert_eq!(ordered_bytes.0, encoded_state_id);
    assert_eq!(
        plan.constants
            .iter()
            .find(|constant| constant.id == *decode_encoding_id)
            .map(|constant| &constant.value),
        Some(&PlanConstantValue::Text {
            value: "Utf8".to_owned()
        })
    );

    let mut tampered_encoding = plan.clone();
    let constant = tampered_encoding
        .constants
        .iter_mut()
        .find(|constant| constant.id == *decode_encoding_id)
        .expect("decode encoding constant should exist");
    constant.value = PlanConstantValue::Text {
        value: "Utf16".to_owned(),
    };
    let tampered_verification = verify_plan(&tampered_encoding).unwrap();
    assert_eq!(tampered_verification.status, "fail");
    assert!(
        tampered_verification.checks.iter().any(|check| check.id
            == "constant-refs-resolve-and-match-storage-types"
            && !check.pass)
            || tampered_verification
                .checks
                .iter()
                .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "unsupported conversion constants must fail verification: {tampered_verification:#?}"
    );
}

// test: ascii_text_bytes_conversion_lowers_to_typed_executable_plan_ops
// test: bytes_set_conversion_bank_updates_lower_to_typed_executable_plan_ops
#[test]
fn bytes_set_conversion_bank_updates_lower_to_typed_executable_plan_ops() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_set_conversion_bank_plan_ops.bn",
        include_str!("../../../../../examples/bytes_set_conversion_bank_plan_ops.bn").to_owned(),
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
    let patch_source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.patch");
    let inspect_source_id =
        debug_entry_id(&plan.debug_map.source_routes, "source", "store.inspect");
    let left_payload_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "left_payload");
    let patched_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.patched");

    let op_for = |source_id: usize, target: &str| {
        let target_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", target);
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
            .unwrap_or_else(|| panic!("missing update op for {target}"))
    };

    let patch_op = op_for(patch_source_id, "store.patched");
    let PlanOpKind::UpdateBranch {
        expression_kind: PlanExpressionKind::BytesSet,
        ordered_inputs,
        source_payload_field: None,
        update_constant_id: None,
        ..
    } = &patch_op.kind
    else {
        panic!("Bytes/set op should carry typed ordered operands: {patch_op:#?}");
    };
    let [
        ValueRef::State(input_bytes),
        ValueRef::Constant(index_id),
        ValueRef::Constant(value_id),
    ] = ordered_inputs.as_slice()
    else {
        panic!("Bytes/set ordered operands should be state/index/value: {ordered_inputs:#?}");
    };
    assert_eq!(input_bytes.0, left_payload_state_id);
    assert_eq!(
        constant_value(*index_id),
        &PlanConstantValue::Number { value: 1 }
    );
    assert_eq!(
        constant_value(*value_id),
        &PlanConstantValue::Byte { value: 0x5A }
    );

    let text_op = op_for(inspect_source_id, "store.text");
    let PlanOpKind::UpdateBranch {
        expression_kind: PlanExpressionKind::BytesToText,
        ordered_inputs,
        source_payload_field: None,
        update_constant_id: None,
        ..
    } = &text_op.kind
    else {
        panic!("Bytes/to_text op should carry typed ordered operands: {text_op:#?}");
    };
    let [ValueRef::State(text_input), ValueRef::Constant(encoding_id)] = ordered_inputs.as_slice()
    else {
        panic!(
            "Bytes/to_text ordered operands should be patched state plus encoding: {ordered_inputs:#?}"
        );
    };
    assert_eq!(text_input.0, patched_state_id);
    assert_eq!(
        constant_value(*encoding_id),
        &PlanConstantValue::Text {
            value: "Utf8".to_owned()
        }
    );

    for (target, expression_kind) in [
        ("store.hex", PlanExpressionKind::BytesToHex),
        ("store.base64", PlanExpressionKind::BytesToBase64),
    ] {
        let op = op_for(inspect_source_id, target);
        assert!(matches!(
            &op.kind,
            PlanOpKind::UpdateBranch {
                expression_kind: actual_kind,
                source_payload_field: None,
                update_constant_id: None,
                ordered_inputs,
                ..
            } if *actual_kind == expression_kind
                && ordered_inputs == &vec![ValueRef::State(StateId(patched_state_id))]
        ));
    }
}

// test: verify_plan_rejects_tampered_text_source_payload_field_after_lowering
#[test]
fn verify_plan_rejects_tampered_text_source_payload_field_after_lowering() {
    let parsed = boon_parser::parse_source(
        "examples/root_scalar_plan_ops.bn",
        include_str!("../../../../../examples/root_scalar_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    assert_eq!(verify_plan(&plan).unwrap().status, "pass");

    let source_id = debug_entry_id(
        &plan.debug_map.source_routes,
        "source",
        "store.input.change",
    );
    let state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.copied");
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
        .expect("TEXT source payload route should lower to one update op");
    assert!(
        op.inputs.iter().any(|input| matches!(
            input,
            ValueRef::SourcePayload {
                source_id: input_source_id,
                field: SourcePayloadField::Text
            } if input_source_id.0 == source_id
        )),
        "source payload should be a typed TEXT executable operand: {op:#?}"
    );
    let PlanOpKind::UpdateBranch {
        expression_kind: PlanExpressionKind::SourcePayload,
        source_payload_field,
        ..
    } = &mut op.kind
    else {
        panic!("TEXT source payload route should be a source-payload update branch");
    };
    assert_eq!(*source_payload_field, Some(SourcePayloadField::Text));

    *source_payload_field = Some(SourcePayloadField::Key);

    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "fail");
    assert!(
        verification.checks.iter().any(|check| check.id
            == "constant-refs-resolve-and-match-storage-types"
            && !check.pass),
        "TEXT-to-key tamper must fail typed payload declaration verification: {verification:#?}"
    );
    assert!(
        verification
            .checks
            .iter()
            .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "tampered source_payload_field must fail typed payload operand/executor support counts: {verification:#?}"
    );
}

