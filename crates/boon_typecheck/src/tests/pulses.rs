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
    assert_eq!(pulses.result.mode, FlowMode::PresentOrAbsent);
    assert_eq!(pulses.result.ty, tag_type("Pulse"));
    let skip = program
        .calls
        .iter()
        .find(|call| call.function == "Stream/skip")
        .expect("checked stream skip call");
    assert_eq!(skip.result.mode, FlowMode::PresentOrAbsent);
    assert!(matches!(skip.result.ty, Type::Object(_)));
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
