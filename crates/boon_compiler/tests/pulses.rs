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
    for function in ["Stream/pulses", "Stream/skip"] {
        let call = checked
            .calls
            .iter()
            .find(|call| call.function == function)
            .unwrap_or_else(|| panic!("missing checked call for {function}"));
        assert_eq!(call.result.mode, boon_typecheck::FlowMode::PresentOrAbsent);
    }
}
