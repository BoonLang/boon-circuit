fn exact_state_call_names(program: &ErasedProgram) -> BTreeSet<String> {
    program
        .state_update_arms
        .iter()
        .flat_map(|arm| exact_subtree(program, arm.output_expression_id))
        .filter_map(|expression| match &expression.kind {
            ExecutableExpressionKind::Call { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn bytes_operations_survive_as_exact_typed_calls() {
    let fixtures = [
        (
            "bytes_set_plan_ops.bn",
            include_str!("../../../../examples/bytes_set_plan_ops.bn"),
            &["Bytes/set", "Bytes/get", "Bytes/length", "Bytes/concat"][..],
        ),
        (
            "bytes_encoding_plan_ops.bn",
            include_str!("../../../../examples/bytes_encoding_plan_ops.bn"),
            &[
                "Bytes/concat",
                "Bytes/to_hex",
                "Bytes/to_base64",
                "Bytes/zeros",
                "Bytes/from_hex",
                "Bytes/from_base64",
            ][..],
        ),
        (
            "bytes_text_conversion_plan_ops.bn",
            include_str!("../../../../examples/bytes_text_conversion_plan_ops.bn"),
            &["Text/to_bytes", "Bytes/to_text"][..],
        ),
        (
            "bytes_numeric_plan_ops.bn",
            include_str!("../../../../examples/bytes_numeric_plan_ops.bn"),
            &[
                "Bytes/read_unsigned",
                "Bytes/read_signed",
                "Bytes/write_unsigned",
                "Bytes/write_signed",
            ][..],
        ),
    ];

    for (name, source, expected) in fixtures {
        let parsed = boon_parser::parse_source(name, source).unwrap();
        let program = lower(&parsed).unwrap();
        let calls = exact_state_call_names(&program);
        for function in expected {
            assert!(
                calls.contains(*function),
                "{name} lost exact typed call `{function}`; calls={calls:?}"
            );
        }
    }
}

#[test]
fn bytes_set_preserves_fixed_single_byte_argument() {
    let parsed = boon_parser::parse_source(
        "bytes_set_plan_ops.bn",
        include_str!("../../../../examples/bytes_set_plan_ops.bn"),
    )
    .unwrap();
    let program = lower(&parsed).unwrap();
    let arm = exact_state_arm(
        &program,
        "store.patched",
        exact_source_cause(&program, "store.patch"),
    );
    let call = exact_call(&program, arm.output_expression_id, "Bytes/set");
    let ExecutableExpressionKind::Call { arguments, .. } = &call.kind else {
        unreachable!();
    };
    let value = arguments
        .iter()
        .find(|argument| argument.name == "value")
        .expect("Bytes/set value argument");
    assert!(matches!(
        program.executable.expressions[value.value.as_usize()].kind,
        ExecutableExpressionKind::Bytes {
            fixed_size: Some(1),
            ..
        }
    ));
}

#[test]
fn row_local_sources_keep_exact_indexed_bytes_ownership() {
    let parsed = boon_parser::parse_source(
        "bytes_indexed_source_payload_plan_ops.bn",
        include_str!("../../../../examples/bytes_indexed_source_payload_plan_ops.bn"),
    )
    .unwrap();
    let program = lower(&parsed).unwrap();
    let inspect = exact_source_cause(&program, "store.rows.inspect");
    let receive = exact_source_cause(&program, "store.rows.receive");

    for (target, function) in [
        ("store.rows.payload_len", "Bytes/length"),
        ("store.rows.payload_second", "Bytes/get"),
    ] {
        let state = program
            .state_cells
            .iter()
            .find(|state| state.path == target)
            .expect("indexed BYTES state");
        assert!(state.indexed);
        let arm = exact_state_arm(&program, target, inspect);
        exact_call(&program, arm.output_expression_id, function);
        assert!(
            !program
                .state_update_arms
                .iter()
                .any(|arm| arm.state == state.id && arm.cause == receive),
            "`{target}` inherited an unrelated row source"
        );
    }
}

#[test]
fn lower_rejects_duplicate_direct_latest_source_branches() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/bytes_indexed_duplicate_update_conflict_plan_ops.bn");
    let source = std::fs::read_to_string(&fixture).unwrap();
    let parsed = boon_parser::parse_source(fixture.display().to_string(), &source).unwrap();
    let error = lower(&parsed)
        .expect_err("semantic IR lowering must not silently drop duplicate LATEST branches");
    assert!(
        error.contains("duplicate direct `LATEST` branch"),
        "unexpected duplicate LATEST lowering error: {error}"
    );
    assert!(error.contains("receive.bytes"));
}
