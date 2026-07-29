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
    let semantic = boon_semantic::elaborate(checked, &[]).expect("semantic pulse stream");
    let verified = boon_verify::verify_explicit_contracts(semantic).expect("verified pulse stream");
    let ir = boon_ir::erase_and_lower(verified).expect("erased pulse stream");
    assert!(ir.activations().is_empty());
    let [batch] = ir.pulse_batches() else {
        panic!("one typed Stream/pulses call must lower to one baseline pulse batch");
    };
    assert_eq!(batch.id, boon_ir::PulseBatchId(0));
    assert_eq!(batch.state, None);
    assert_eq!(batch.hold_expression, None);
    assert_eq!(batch.enclosing_activation, None);
    assert!(batch.state_update_arms.is_empty());
    assert_ne!(batch.semantic_slice_digest, [0; 32]);
    assert!(matches!(
        ir.executable.expressions[batch.call_expression.as_usize()].kind,
        boon_ir::ExecutableExpressionKind::Call {
            intrinsic: Some(boon_typecheck::CheckedIntrinsicV1::StreamPulses),
            ..
        }
    ));
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
    let boon_ir::StateCellLifetimeV1::ActivationLocal { then_expression } =
        ir.state_cells[0].lifetime
    else {
        panic!("Fibonacci HOLD must be scoped to its enclosing THEN activation");
    };
    assert!(matches!(
        ir.executable.expressions[then_expression.as_usize()].kind,
        boon_ir::ExecutableExpressionKind::Then { .. }
    ));
    assert_ne!(then_expression, hold_updates[0]);

    let [activation] = ir.activations() else {
        panic!("activation-local Fibonacci HOLD must own one activation site");
    };
    assert_eq!(activation.id, boon_ir::ActivationId(0));
    assert_eq!(activation.then_expression, then_expression);
    assert_eq!(activation.states, vec![ir.state_cells[0].id]);

    let [batch] = ir.pulse_batches() else {
        panic!("canonical Fibonacci must own one baseline pulse batch");
    };
    assert_eq!(batch.id, boon_ir::PulseBatchId(0));
    assert_eq!(batch.enclosing_activation, Some(activation.id));
    assert_eq!(batch.state, Some(ir.state_cells[0].id));
    assert_eq!(
        batch.hold_expression,
        Some(
            ir.executable
                .expressions
                .iter()
                .find(|expression| {
                    matches!(
                        expression.kind,
                        boon_ir::ExecutableExpressionKind::Hold { .. }
                    )
                })
                .expect("canonical HOLD expression")
                .id
        )
    );
    assert_eq!(
        batch.schedule,
        boon_ir::PulseSchedule::StageArbitrateCommitPublishBeforeNext
    );
    assert_eq!(
        batch.flush_policy,
        boon_ir::PulseFlushPolicy::DiscardCurrentStopRemainingKeepPriorCommits
    );
    assert!(!batch.trigger_arms.is_empty());
    assert!(
        batch
            .trigger_arms
            .iter()
            .all(|arm| arm.cause == boon_ir::EventCause::Pulse(batch.id))
    );
    assert_eq!(batch.state_update_arms.len(), 1);
    assert_eq!(
        batch.state_update_arms[0].cause,
        boon_ir::EventCause::Pulse(batch.id)
    );
    assert_eq!(batch.state_update_arms[0].state, ir.state_cells[0].id);
    assert!(ir.state_updates().contains(&batch.state_update_arms[0]));
    assert!(
        batch
            .emission_routes
            .iter()
            .any(|route| matches!(route.filter, boon_ir::PulseEmissionFilter::Skip { .. }))
    );
    assert_ne!(batch.semantic_slice_digest, [0; 32]);
}
