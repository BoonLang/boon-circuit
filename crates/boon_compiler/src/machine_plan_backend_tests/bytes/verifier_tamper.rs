// Included by `../bytes.rs`.

// test: verifier_rejects_tampered_inline_bytes_payload
// test: verifier_rejects_tampered_cpu_executor_support_shapes
#[test]
fn verifier_rejects_tampered_cpu_executor_support_shapes() {
    let parsed = boon_parser::parse_source(
        "examples/root_scalar_plan_ops.bn",
        include_str!("../../../../../examples/root_scalar_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    assert!(
        plan.capability_summary.cpu_plan_executor_complete,
        "root scalar fixture should be executable before tampering: {:#?}",
        plan.capability_summary
    );

    let mut missing_payload_ref = plan.clone();
    let payload_read_op = missing_payload_ref
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::SourcePayload,
                    ..
                }
            ) && op
                .inputs
                .iter()
                .any(|input| matches!(input, ValueRef::SourcePayload { .. }))
        })
        .expect("fixture should contain a SourcePayload update branch");
    payload_read_op
        .inputs
        .retain(|input| !matches!(input, ValueRef::SourcePayload { .. }));
    let missing_payload_ref_verification = verify_plan(&missing_payload_ref).unwrap();
    assert_eq!(missing_payload_ref_verification.status, "fail");
    assert!(
        missing_payload_ref_verification
            .checks
            .iter()
            .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "SourcePayload update without its typed payload ref must not satisfy CPU executor support: {missing_payload_ref_verification:#?}"
    );

    let text_state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.source_text");
    let mut wrong_bool_input = plan.clone();
    let bool_not_op = wrong_bool_input
        .regions
        .iter_mut()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter_mut())
        .find(|op| {
            matches!(
                &op.kind,
                PlanOpKind::UpdateBranch {
                    expression_kind: PlanExpressionKind::BoolNot,
                    ..
                }
            )
        })
        .expect("fixture should contain a BoolNot update branch");
    bool_not_op.inputs = bool_not_op
        .inputs
        .iter()
        .map(|input| match input {
            ValueRef::State(_) => ValueRef::State(StateId(text_state_id)),
            other => other.clone(),
        })
        .collect();
    let wrong_bool_input_verification = verify_plan(&wrong_bool_input).unwrap();
    assert_eq!(wrong_bool_input_verification.status, "fail");
    assert!(
        wrong_bool_input_verification
            .checks
            .iter()
            .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "BoolNot update with a non-bool input must not satisfy CPU executor support: {wrong_bool_input_verification:#?}"
    );
}

// test: verify_plan_rejects_tampered_source_payload_field_after_lowering
#[test]
fn verify_plan_rejects_tampered_source_payload_field_after_lowering() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_source_payload_plan_ops.bn",
        include_str!("../../../../../examples/bytes_source_payload_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    assert_eq!(verify_plan(&plan).unwrap().status, "pass");

    let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.receive");
    let state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.received");
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
        .expect("BYTES source payload route should lower to one update op");
    assert!(
        op.inputs.iter().any(|input| matches!(
            input,
            ValueRef::SourcePayload {
                source_id: input_source_id,
                field: SourcePayloadField::Bytes
            } if input_source_id.0 == source_id
        )),
        "source payload should be a typed BYTES executable operand: {op:#?}"
    );
    let PlanOpKind::UpdateBranch {
        expression_kind: PlanExpressionKind::SourcePayload,
        source_payload_field,
        ..
    } = &mut op.kind
    else {
        panic!("BYTES source payload route should be a source-payload update branch");
    };
    assert_eq!(*source_payload_field, Some(SourcePayloadField::Bytes));

    *source_payload_field = Some(SourcePayloadField::Text);

    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "fail");
    assert!(
        verification.checks.iter().any(|check| check.id
            == "constant-refs-resolve-and-match-storage-types"
            && !check.pass),
        "tampered source_payload_field must fail storage type verification: {verification:#?}"
    );
    assert!(
        verification
            .checks
            .iter()
            .any(|check| check.id == "capability-summary-derived-counts" && !check.pass),
        "tampered source_payload_field must fail derived executor support counts: {verification:#?}"
    );
}

// test: verify_plan_rejects_tampered_bytes_source_payload_descriptor_type
#[test]
fn verify_plan_rejects_tampered_bytes_source_payload_descriptor_type() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_source_payload_plan_ops.bn",
        include_str!("../../../../../examples/bytes_source_payload_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    assert_eq!(verify_plan(&plan).unwrap().status, "pass");

    let route = plan
        .source_routes
        .iter_mut()
        .find(|route| route.path == "store.receive")
        .expect("fixture should contain store.receive source route");
    let descriptor = route
        .payload_schema
        .typed_fields
        .iter_mut()
        .find(|descriptor| descriptor.field == SourcePayloadField::Bytes)
        .expect("BYTES source route should declare a typed Bytes payload descriptor");
    assert_eq!(descriptor.value_type, SourcePayloadValueType::Bytes);
    descriptor.value_type = SourcePayloadValueType::Text;

    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "fail");
    assert!(
        verification.checks.iter().any(|check| check.id
            == "constant-refs-resolve-and-match-storage-types"
            && !check.pass),
        "tampered Bytes payload descriptor must fail storage/type verification: {verification:#?}"
    );
}

// test: verify_plan_accepts_bytes_source_payload_guards
#[test]
fn verify_plan_accepts_bytes_source_payload_guards() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_source_payload_plan_ops.bn",
        include_str!("../../../../../examples/bytes_source_payload_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let mut plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();
    let source_id = debug_entry_id(&plan.debug_map.source_routes, "source", "store.receive");
    let state_id = debug_entry_id(&plan.debug_map.state_slots, "state", "store.received");
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
        .expect("BYTES source payload route should lower to one update op");
    let PlanOpKind::UpdateBranch { source_guard, .. } = &mut op.kind else {
        panic!("BYTES source payload route should be an update branch");
    };
    *source_guard = Some(PlanSourceGuard::SourcePayloadOneOf {
        source_id: SourceId(source_id),
        field: SourcePayloadField::Bytes,
        values: vec!["01fe04".to_owned()],
    });

    let verification = verify_plan(&plan).unwrap();
    assert_eq!(verification.status, "pass");
    assert!(
        verification
            .checks
            .iter()
            .any(|check| check.id == "capability-summary-derived-counts" && check.pass),
        "BYTES source payload guard should remain executable in the CPU PlanExecutor capability summary: {verification:#?}"
    );
}

