#[test]
fn bounded_pulse_stream_contracts_are_public_to_compiler_consumers() {
    let parsed = boon_parser::parse_source(
        "bounded-pulse-stream.bn",
        "pulses: 3 |> Stream/pulses()\nvisible: pulses |> Stream/skip(count: 2)\n",
    )
    .expect("parsed pulse stream");
    let checked = boon_typecheck::check_program(&parsed);
    assert!(
        !checked.report.has_errors(),
        "diagnostics: {:#?}",
        checked.report.diagnostics
    );
    let checked = checked.program.expect("checked pulse stream");
    for (function, intrinsic) in [
        (
            "Stream/pulses",
            boon_typecheck::CheckedIntrinsicV1::StreamPulses,
        ),
        (
            "Stream/skip",
            boon_typecheck::CheckedIntrinsicV1::StreamSkip,
        ),
    ] {
        let call = checked
            .calls
            .iter()
            .find(|call| call.function == function)
            .unwrap_or_else(|| panic!("missing checked call for {function}"));
        assert_eq!(call.intrinsic, Some(intrinsic));
        assert_eq!(call.result.mode, boon_typecheck::FlowMode::PresentOrAbsent);
    }
}

#[test]
fn canonical_fibonacci_pulses_cross_the_verified_ir_spine() {
    let parsed = boon_parser::parse_source(
        "fibonacci-pulses.bn",
        r#"
value: fibonacci(position: 10)

FUNCTION fibonacci(position) {
    position
    |> THEN {
        position |> WHILE {
            1 => 1

            n =>
                [previous: 0, current: 1]
                |> HOLD state {
                    n - 1
                    |> Stream/pulses()
                    |> THEN {
                        [
                            previous: state.current
                            current: state.previous + state.current
                        ]
                    }
                }
                |> Stream/skip(count: n - 1)
                |> .current
        }
    }
}
"#,
    )
    .expect("parsed Fibonacci");
    let checked = boon_typecheck::check_program(&parsed);
    assert!(
        !checked.report.has_errors(),
        "diagnostics: {:#?}",
        checked.report.diagnostics
    );
    let semantic = boon_semantic::elaborate(checked.program.expect("checked Fibonacci"), &[])
        .expect("semantic Fibonacci");
    for intrinsic in [
        boon_typecheck::CheckedIntrinsicV1::StreamPulses,
        boon_typecheck::CheckedIntrinsicV1::StreamSkip,
    ] {
        assert!(
            semantic
                .execution_graph()
                .expressions
                .iter()
                .any(|expression| {
                    matches!(
                        expression.kind,
                        boon_semantic::SemanticExpressionKind::Call {
                            intrinsic: Some(found),
                            ..
                        } if found == intrinsic
                    )
                })
        );
    }
    let verified = boon_verify::verify_explicit_contracts(semantic).expect("verified Fibonacci");
    let ir = boon_ir::erase_and_lower(verified).expect("erased Fibonacci");

    let hold_updates = ir
        .executable
        .expressions
        .iter()
        .find_map(|expression| match &expression.kind {
            boon_ir::ExecutableExpressionKind::Hold { updates, .. } => Some(updates),
            _ => None,
        })
        .expect("canonical HOLD");
    assert_eq!(hold_updates.len(), 1);
    assert!(matches!(
        ir.executable.expressions[hold_updates[0].as_usize()].kind,
        boon_ir::ExecutableExpressionKind::Then { .. }
    ));
    for (function, intrinsic) in [
        (
            "Stream/pulses",
            boon_typecheck::CheckedIntrinsicV1::StreamPulses,
        ),
        (
            "Stream/skip",
            boon_typecheck::CheckedIntrinsicV1::StreamSkip,
        ),
    ] {
        assert!(ir.executable.expressions.iter().any(|expression| {
            matches!(
                &expression.kind,
                boon_ir::ExecutableExpressionKind::Call {
                    name,
                    intrinsic: Some(found),
                    ..
                } if name == function && *found == intrinsic
            )
        }));
    }
    assert_eq!(ir.state_cells.len(), 1);
}
