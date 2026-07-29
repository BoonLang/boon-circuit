#[test]
fn checked_stream_pulses_owns_event_flow_and_exact_count_contracts() {
    let parsed = boon_parser::parse_source(
        "checked-stream-pulses.bn",
        r#"
trigger: SOURCE

state:
    [previous: 0, current: 1]
    |> HOLD state {
        trigger
        |> THEN {
            9
            |> Stream/pulses()
            |> THEN {
                [
                    previous: state.current
                    current: state.previous + state.current
                ]
            }
        }
    }

visible:
    state
    |> Stream/skip(count: 9)
    |> Field/current()
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("valid pulse program");
    let pulses = program
        .calls
        .iter()
        .find(|call| call.function == "Stream/pulses")
        .expect("checked pulse call");
    assert_eq!(pulses.intrinsic, Some(CheckedIntrinsicV1::StreamPulses));
    assert_eq!(pulses.result.mode, FlowMode::PresentOrAbsent);
    assert_eq!(pulses.result.ty, tag_type("Pulse"));
    let skip = program
        .calls
        .iter()
        .find(|call| call.function == "Stream/skip")
        .expect("checked stream skip call");
    assert_eq!(skip.intrinsic, Some(CheckedIntrinsicV1::StreamSkip));
    assert_eq!(skip.result.mode, FlowMode::PresentOrAbsent);
    assert!(matches!(skip.result.ty, Type::Object(_)));
}

#[test]
fn canonical_fibonacci_has_one_checked_hold_state() {
    let parsed = boon_parser::parse_source(
        "checked-fibonacci-pulses.bn",
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
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("checked Fibonacci pulse function");
    assert_eq!(
        program.states.len(),
        1,
        "canonical HOLD must own one checked state: {:#?}",
        program.states
    );
    let state = &program.states[0];
    assert_eq!(
        program
            .declarations
            .iter()
            .find(|declaration| declaration.id == state.declaration)
            .map(|declaration| declaration.name.as_str()),
        Some("state")
    );
    assert!(program.statements.iter().any(|statement| {
        statement.id == state.statement
            && statement
                .resources
                .contains(&CheckedResourceBinding::State { state: state.id })
    }));
}

#[test]
fn fibonacci_called_from_list_map_keeps_scalar_result_in_nested_calls() {
    let parsed = boon_parser::parse_source(
        "checked-list-fibonacci-pulses.bn",
        r#"
positions: LIST {
    1
    2
    3
}

sequence:
    positions
    |> List/map(item, new: [
        position: item
        value: fibonacci(position: item)
    ])

selected:
    fibonacci_result(sequence: sequence)

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

FUNCTION fibonacci_result(sequence) {
    sequence
    |> List/get(position: 3)
}
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );
    let program = output.program.expect("checked list Fibonacci program");
    let fibonacci = program
        .callables
        .iter()
        .find(|callable| callable.name == "fibonacci")
        .expect("Fibonacci callable");
    assert_eq!(fibonacci.result.ty, Type::Number);

    let get = program
        .calls
        .iter()
        .find(|call| call.function == "List/get")
        .expect("nested List/get call");
    let get_expression = program
        .expressions
        .iter()
        .find(|expression| expression.id == get.expression)
        .expect("nested List/get expression");
    assert_eq!(get.result, get_expression.flow_type);
    assert!(matches!(
        found_payload_type(&get.result.ty),
        Some(Type::Var(_))
    ));
    assert!(matches!(
        found_payload_type(
            &program
                .callables
                .iter()
                .find(|callable| callable.name == "fibonacci_result")
                .expect("generic Fibonacci result callable")
                .result
                .ty
        ),
        Some(Type::Var(_))
    ));
    let result_call = program
        .calls
        .iter()
        .find(|call| call.function == "fibonacci_result")
        .expect("root Fibonacci result call");
    assert!(matches!(
        &result_call.result.ty,
        Type::VariantSet(variants)
            if variants.iter().any(|variant| matches!(
                variant,
                Variant::Tagged { tag, fields }
                    if tag == "Found"
                        && matches!(
                            fields.fields.get("value"),
                            Some(Type::Object(value))
                                if value.fields.get("value") == Some(&Type::Number)
                        )
            ))
    ));
}

#[test]
fn ordinary_continuous_values_remain_invalid_then_triggers() {
    let parsed = boon_parser::parse_source(
        "continuous-then.bn",
        r#"
value:
    1
    |> THEN {
        2
    }
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(output.report.has_errors());
    assert!(output.report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("`THEN` requires a tick-present-or-absent value")
    }));
}

#[test]
fn stream_counts_reject_static_fractional_and_negative_numbers() {
    for (name, source, function) in [
        (
            "fractional-pulses.bn",
            "value: 1.5 |> Stream/pulses()\n",
            "Stream/pulses",
        ),
        (
            "negative-pulses.bn",
            "value: 0 - 1 |> Stream/pulses()\n",
            "Stream/pulses",
        ),
        (
            "fractional-skip.bn",
            "value: SOURCE |> Stream/skip(count: 0.5)\n",
            "Stream/skip",
        ),
    ] {
        let parsed = boon_parser::parse_source(name, source).unwrap();
        let output = check_program(&parsed);
        assert!(
            output.report.diagnostics.iter().any(|diagnostic| {
                diagnostic.message
                    == format!("`{function}` count must be a non-negative whole Number")
            }),
            "{name} diagnostics: {:#?}",
            output.report.diagnostics
        );
    }
}
