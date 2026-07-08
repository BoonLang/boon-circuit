// Included by `../bytes.rs`.

// test: bytes_search_updates_lower_to_ordered_typed_executable_plan_ops
#[test]
fn bytes_search_updates_lower_to_ordered_typed_executable_plan_ops() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_search_plan_ops.bn",
        include_str!("../../../../../examples/bytes_search_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "pass");
    assert!(
        plan.capability_summary.cpu_plan_executor_complete,
        "Bytes/search fixture should be executable by the CPU PlanExecutor: {:#?}",
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
    let joined_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.joined");
    let found_needle_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "found_needle");
    let missing_needle_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "missing_needle");
    let empty_needle_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "empty_needle");
    let prefix_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "prefix");
    let wrong_prefix_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "wrong_prefix");
    let suffix_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "suffix");
    let wrong_suffix_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "wrong_suffix");

    let expected = [
        (
            "store.found_index",
            PlanExpressionKind::BytesFind,
            found_needle_state_id,
            PlanValueType::Number,
        ),
        (
            "store.missing_index",
            PlanExpressionKind::BytesFind,
            missing_needle_state_id,
            PlanValueType::Number,
        ),
        (
            "store.empty_index",
            PlanExpressionKind::BytesFind,
            empty_needle_state_id,
            PlanValueType::Number,
        ),
        (
            "store.starts",
            PlanExpressionKind::BytesStartsWith,
            prefix_state_id,
            PlanValueType::Bool,
        ),
        (
            "store.not_starts",
            PlanExpressionKind::BytesStartsWith,
            wrong_prefix_state_id,
            PlanValueType::Bool,
        ),
        (
            "store.ends",
            PlanExpressionKind::BytesEndsWith,
            suffix_state_id,
            PlanValueType::Bool,
        ),
        (
            "store.not_ends",
            PlanExpressionKind::BytesEndsWith,
            wrong_suffix_state_id,
            PlanValueType::Bool,
        ),
    ];

    for (target_label, expression_kind, second_state_id, output_type) in expected {
        let target_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", target_label);
        let target_slot = plan
            .storage_layout
            .scalar_slots
            .iter()
            .find(|slot| slot.state_id.0 == target_state_id)
            .unwrap_or_else(|| panic!("{target_label} storage slot should lower"));
        assert_eq!(target_slot.value_type, output_type);

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
                panic!("store.measure should lower to bytes search update for {target_label}")
            });
        assert_eq!(op.unresolved_executable_ref_count, 0);
        assert!(matches!(
            &op.kind,
            PlanOpKind::UpdateBranch {
                expression_kind: actual_kind,
                source_payload_field: None,
                update_constant_id: None,
                ordered_inputs,
                ..
            } if *actual_kind == expression_kind
                && ordered_inputs == &vec![
                    ValueRef::State(StateId(joined_state_id)),
                    ValueRef::State(StateId(second_state_id)),
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
                    expression_kind: PlanExpressionKind::BytesFind,
                    ..
                }
            )
        })
        .expect("fixture should contain a Bytes/find update op");
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
        "Bytes/find without ordered inputs must fail verifier support: {tampered_missing_order_verification:#?}"
    );

    let mut tampered_output = plan.clone();
    let payload_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "left_payload");
    let tampered_op = tampered_output
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BytesStartsWith,
                    ..
                }
            )
        })
        .expect("fixture should contain a Bytes/starts_with update op");
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
        "Bytes/starts_with with non-BOOL output must fail verifier support: {tampered_output_verification:#?}"
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
                    expression_kind: PlanExpressionKind::BytesEndsWith,
                    ..
                }
            )
        })
        .expect("fixture should contain a Bytes/ends_with update op");
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
        "Bytes/ends_with with an update constant must fail verifier support: {tampered_constant_verification:#?}"
    );
}

// test: bytes_encoding_updates_lower_to_ordered_typed_executable_plan_ops
#[test]
fn bytes_encoding_updates_lower_to_ordered_typed_executable_plan_ops() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_encoding_plan_ops.bn",
        include_str!("../../../../../examples/bytes_encoding_plan_ops.bn").to_owned(),
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
    assert_eq!(plan.capability_summary.unknown_plan_op_count, 0);

    let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.decode");
    let zeros_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.zeros");
    let hex_input_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "hex_input");
    let base64_input_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "base64_input");
    let decoded_hex_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "store.decoded_hex");
    let decoded_base64_state_id =
        debug_entry_id(&plan.debug_map.state_slots, "state", "store.decoded_base64");

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
            .unwrap_or_else(|| panic!("missing decode op for state {target_state_id}"))
    };

    assert!(matches!(
        &op_for(zeros_state_id).kind,
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesZeros,
            source_payload_field: None,
            update_constant_id: None,
            ordered_inputs,
            ..
        } if matches!(ordered_inputs.as_slice(), [ValueRef::Constant(_)])
    ));
    assert!(matches!(
        &op_for(decoded_hex_state_id).kind,
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesFromHex,
            source_payload_field: None,
            update_constant_id: None,
            ordered_inputs,
            ..
        } if ordered_inputs == &vec![ValueRef::State(StateId(hex_input_state_id))]
    ));
    assert!(matches!(
        &op_for(decoded_base64_state_id).kind,
        PlanOpKind::UpdateBranch {
            expression_kind: PlanExpressionKind::BytesFromBase64,
            source_payload_field: None,
            update_constant_id: None,
            ordered_inputs,
            ..
        } if ordered_inputs == &vec![ValueRef::State(StateId(base64_input_state_id))]
    ));

    let encode_source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.encode");
    let joined_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.joined");
    for (target, expression_kind) in [
        ("store.hex", PlanExpressionKind::BytesToHex),
        ("store.base64", PlanExpressionKind::BytesToBase64),
    ] {
        let target_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", target);
        let op =
            plan.regions
                .iter()
                .filter(|region| region.kind == RegionKind::UpdateBranches)
                .flat_map(|region| region.ops.iter())
                .find(|op| {
                    op.inputs.iter().any(
                        |input| matches!(input, ValueRef::Source(id) if id.0 == encode_source_id),
                    ) && matches!(&op.output, Some(ValueRef::State(id)) if id.0 == target_state_id)
                })
                .unwrap_or_else(|| panic!("missing encode op for {target}"));
        assert!(matches!(
            &op.kind,
            PlanOpKind::UpdateBranch {
                expression_kind: actual_kind,
                source_payload_field: None,
                update_constant_id: None,
                ordered_inputs,
                ..
            } if *actual_kind == expression_kind
                && ordered_inputs == &vec![ValueRef::State(StateId(joined_state_id))]
        ));
    }
}

