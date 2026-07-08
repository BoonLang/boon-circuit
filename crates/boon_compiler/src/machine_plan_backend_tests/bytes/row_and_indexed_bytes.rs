// Included by `../bytes.rs`.

// test: row_bytes_predicates_lower_to_typed_expressions
// test: indexed_bytes_row_initial_fields_get_concrete_storage_types
#[test]
fn indexed_bytes_row_initial_fields_get_concrete_storage_types() {
    let parsed = boon_parser::parse_source(
        "examples/bytes_indexed_source_payload_plan_ops.bn",
        include_str!("../../../../../examples/bytes_indexed_source_payload_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let ir = boon_ir::lower(&parsed).unwrap();
    let plan = compile_typed_program(&ir, TargetProfile::SoftwareDefault).unwrap();

    let payload_id = StateId(debug_entry_id(
        &plan.debug_map.state_slots,
        "state",
        "row.payload",
    ));
    let payload_slot = plan
        .storage_layout
        .scalar_slots
        .iter()
        .find(|slot| slot.state_id == payload_id)
        .expect("row.payload slot should exist");
    assert!(payload_slot.indexed);
    assert_eq!(
        payload_slot.value_type,
        PlanValueType::Bytes { fixed_len: Some(3) }
    );
    assert_eq!(
        payload_slot.initial_value_kind,
        InitialValueKind::RowInitialField
    );
    let payload_bank = plan
        .storage_layout
        .byte_banks
        .iter()
        .find(|bank| bank.state_id == payload_id)
        .expect("fixed indexed row.payload should declare a byte bank");
    assert_eq!(payload_bank.state_storage_id, payload_slot.id);
    assert!(payload_bank.indexed);
    assert_eq!(payload_bank.scope_id, payload_slot.scope_id);
    assert_eq!(payload_bank.fixed_len, 3);

    let rows_id = debug_entry_id(&plan.debug_map.list_slots, "list", "rows");
    let list_slot = plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id.0 == rows_id)
        .expect("rows list slot should exist");
    let payload_field = list_slot.initial_rows[0]
        .fields
        .iter()
        .find(|field| field.name == "payload")
        .expect("payload initial field should exist");
    let PlanConstantValue::Bytes {
        byte_len,
        inline_bytes,
        ..
    } = &payload_field.value
    else {
        panic!("payload initial field should be a typed BYTES constant: {payload_field:#?}");
    };
    assert_eq!(*byte_len, 3);
    assert_eq!(inline_bytes.as_deref(), Some(&[0, 0, 0][..]));

    let receive_id = SourceId(debug_entry_id(
        &plan.debug_map.source_routes,
        "source",
        "row.receive",
    ));
    let receive_update = plan
        .regions
        .iter()
        .filter(|region| region.kind == RegionKind::UpdateBranches)
        .flat_map(|region| region.ops.iter())
        .find(|op| {
            op.indexed
                && matches!(&op.output, Some(ValueRef::State(id)) if *id == payload_id)
                && op.inputs.contains(&ValueRef::Source(receive_id))
        })
        .expect("row.receive should update row.payload through an indexed op");
    let PlanOpKind::UpdateBranch {
        expression_kind,
        source_payload_field,
        ..
    } = &receive_update.kind
    else {
        panic!("row.receive payload op should be an update branch: {receive_update:#?}");
    };
    assert_eq!(expression_kind, &PlanExpressionKind::SourcePayload);
    assert_eq!(source_payload_field, &Some(SourcePayloadField::Bytes));
    assert!(receive_update.inputs.contains(&ValueRef::SourcePayload {
        source_id: receive_id,
        field: SourcePayloadField::Bytes,
    }));

    let verification = verify_plan(&plan).unwrap();
    assert!(
        verification
            .checks
            .iter()
            .any(|check| check.id == "byte-bank-slots-match-fixed-bytes" && check.pass)
    );
    assert_eq!(
        verification.status, "pass",
        "indexed row BYTES initial field plan should verify: {verification:#?}"
    );
}

// test: fixed_bytes_scalars_declare_byte_banks_but_dynamic_bytes_do_not
#[test]
fn fixed_bytes_scalars_declare_byte_banks_but_dynamic_bytes_do_not() {
    let fixed_parsed = boon_parser::parse_source(
        "examples/bytes_set_plan_ops.bn",
        include_str!("../../../../../examples/bytes_set_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let fixed_ir = boon_ir::lower(&fixed_parsed).unwrap();
    let fixed_plan = compile_typed_program(&fixed_ir, TargetProfile::SoftwareDefault).unwrap();

    let fixed_id = StateId(debug_entry_id(
        &fixed_plan.debug_map.state_slots,
        "state",
        "store.patched",
    ));
    let fixed_slot = fixed_plan
        .storage_layout
        .scalar_slots
        .iter()
        .find(|slot| slot.state_id == fixed_id)
        .expect("fixed BYTES slot should exist");
    assert_eq!(
        fixed_slot.value_type,
        PlanValueType::Bytes { fixed_len: Some(4) }
    );
    let fixed_bank = fixed_plan
        .storage_layout
        .byte_banks
        .iter()
        .find(|bank| bank.state_id == fixed_id)
        .expect("fixed BYTES slot should declare a byte bank");
    assert_eq!(fixed_bank.state_storage_id, fixed_slot.id);
    assert_eq!(fixed_bank.fixed_len, 4);
    assert_eq!(fixed_bank.capacity, Some(1));
    assert!(!fixed_bank.indexed);
    let fixed_verification = verify_plan(&fixed_plan).unwrap();
    assert!(
        fixed_verification
            .checks
            .iter()
            .any(|check| check.id == "byte-bank-slots-match-fixed-bytes" && check.pass)
    );
    assert_eq!(fixed_verification.status, "pass");

    let dynamic_parsed = boon_parser::parse_source(
        "examples/bytes_source_payload_plan_ops.bn",
        include_str!("../../../../../examples/bytes_source_payload_plan_ops.bn").to_owned(),
    )
    .unwrap();
    let dynamic_ir = boon_ir::lower(&dynamic_parsed).unwrap();
    let dynamic_plan = compile_typed_program(&dynamic_ir, TargetProfile::SoftwareDefault).unwrap();
    let dynamic_id = StateId(debug_entry_id(
        &dynamic_plan.debug_map.state_slots,
        "state",
        "store.received",
    ));
    let dynamic_slot = dynamic_plan
        .storage_layout
        .scalar_slots
        .iter()
        .find(|slot| slot.state_id == dynamic_id)
        .expect("dynamic BYTES slot should exist");
    assert_eq!(
        dynamic_slot.value_type,
        PlanValueType::Bytes { fixed_len: None }
    );
    assert!(
        !dynamic_plan
            .storage_layout
            .byte_banks
            .iter()
            .any(|bank| bank.state_id == dynamic_id),
        "dynamic BYTES state should not declare a fixed-size byte bank"
    );

    let dynamic_verification = verify_plan(&dynamic_plan).unwrap();
    assert!(
        dynamic_verification
            .checks
            .iter()
            .any(|check| check.id == "byte-bank-slots-match-fixed-bytes" && check.pass)
    );
    assert_eq!(dynamic_verification.status, "pass");
}

