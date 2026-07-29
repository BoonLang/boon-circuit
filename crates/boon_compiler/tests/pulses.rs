const CANONICAL_FIBONACCI: &str = r#"
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
"#;

const SOURCE_FIBONACCI: &str = r#"
store: [
    input: SOURCE
    position:
        1 |> HOLD position {
            input.value |> THEN {
                input.value |> Text/to_number()
                |> WHEN {
                    Parsed[value] => value
                    InvalidNumber[reason, position] => 1
                }
            }
        }
    value: fibonacci(position: position)
]

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
"#;

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
    let parsed = boon_parser::parse_source("fibonacci-pulses.bn", CANONICAL_FIBONACCI)
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

#[test]
fn canonical_fibonacci_pulses_lower_into_a_verified_machine_plan() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan_for_role(
        "fibonacci-pulses.bn",
        CANONICAL_FIBONACCI,
        boon_plan::TargetProfile::SoftwareDefault,
        boon_plan::ProgramRole::Server,
    )
    .expect("compiled Fibonacci MachinePlan");
    let plan = &compiled.plan;
    assert_eq!(plan.version.major, boon_plan::PLAN_MAJOR_VERSION);
    let [activation] = plan.activations.as_slice() else {
        panic!("one activation-local state must lower to one plan activation");
    };
    let [batch] = plan.pulse_batches.as_slice() else {
        panic!("one Stream/pulses call must lower to one plan pulse batch");
    };
    assert_eq!(batch.enclosing_activation, Some(activation.id));
    assert!(matches!(batch.start, boon_plan::PlanPulseStart::Startup));
    assert_eq!(batch.state_update_ops.len(), 1);
    assert_eq!(batch.derived_ops.len(), 1);
    assert_eq!(batch.emission_routes.len(), 1);
    let boon_plan::PlanPulseEmissionFilter::Skip { count: skip_count } =
        batch.emission_routes[0].filter
    else {
        panic!("canonical pulse output must retain Stream/skip");
    };
    assert_eq!(
        skip_count, batch.count,
        "structurally equal count expressions must survive compaction as one root"
    );
    assert!(matches!(
        plan.row_expressions.node(batch.count).expect("pulse count"),
        boon_plan::PlanRowExpressionNode::NumberInfix {
            op: boon_plan::PlanInfixOp::Subtract,
            ..
        }
    ));
    let mut count_inputs = Vec::new();
    plan.row_expressions
        .visit_inputs(batch.count, &mut |input| count_inputs.push(input))
        .expect("pulse count inputs");
    assert!(
        count_inputs
            .iter()
            .all(|input| !matches!(input, boon_plan::ValueRef::State(_))),
        "pulse count compaction must not retarget n - 1 to HOLD state: {count_inputs:?}"
    );
    let [slot] = plan.storage_layout.scalar_slots.as_slice() else {
        panic!("one activation-local state slot");
    };
    assert_eq!(
        slot.lifetime,
        boon_plan::PlanStateLifetime::ActivationLocal {
            activation: activation.id
        }
    );
    assert!(
        plan.persistence
            .memory
            .iter()
            .all(|memory| memory.runtime_slot != slot.id)
    );
    let verification = boon_plan::verify_plan(plan).expect("verified plan report");
    assert_eq!(
        verification.status, "pass",
        "checks: {:#?}",
        verification.checks
    );
    assert!(
        !boon_plan::plan_binary(plan)
            .expect("deterministic binary")
            .is_empty()
    );
}

#[test]
fn source_fibonacci_pulse_activation_has_distinct_start_and_emission_causes() {
    let parsed = boon_parser::parse_source("source-fibonacci-pulses.bn", SOURCE_FIBONACCI).unwrap();
    let checked = boon_typecheck::check_program(&parsed);
    assert!(
        !checked.report.has_errors(),
        "diagnostics: {:#?}",
        checked.report.diagnostics
    );
    let semantic =
        boon_semantic::elaborate(checked.program.expect("checked Fibonacci"), &[]).unwrap();
    let reactive = semantic.reactive_graph();
    let [batch] = reactive.pulse_batches.as_slice() else {
        panic!("one source-triggered pulse batch");
    };
    let boon_semantic::SemanticPulseStartV1::Triggered { arms: start_arms } = &batch.start else {
        panic!("the source-derived position state must start the batch");
    };
    assert!(start_arms.iter().all(|arm| {
        matches!(
            reactive.trigger_arms[arm.as_usize()].cause,
            boon_semantic::SemanticEventCauseV1::State(_)
        )
    }));
    let [derived_id] = batch.derived_values.as_slice() else {
        panic!("the pulse batch must own the Fibonacci output");
    };
    let derived = &reactive.derived_values[derived_id.as_usize()];
    assert_eq!(
        derived.causes,
        vec![boon_semantic::SemanticEventCauseV1::Pulse(batch.id)]
    );
    assert!(derived.trigger_arms.iter().all(|arm| {
        reactive.trigger_arms[arm.as_usize()].cause
            == boon_semantic::SemanticEventCauseV1::Pulse(batch.id)
    }));

    let compiled = boon_compiler::compile_source_text_to_machine_plan_for_role(
        "source-fibonacci-pulses.bn",
        SOURCE_FIBONACCI,
        boon_plan::TargetProfile::SoftwareDefault,
        boon_plan::ProgramRole::Server,
    )
    .expect("compiled source-triggered Fibonacci");
    let [batch] = compiled.plan.pulse_batches.as_slice() else {
        panic!("one lowered pulse batch");
    };
    assert!(matches!(
        batch.start,
        boon_plan::PlanPulseStart::Triggered { .. }
    ));
    assert_eq!(batch.derived_ops.len(), 1, "pulse emissions own the output");
}
