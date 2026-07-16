// Full multi-unit application regressions for type propagation across modules.

#[test]
fn fjordpulse_client_keeps_locale_helpers_and_render_slots_typed() {
    let units = [
        (
            "examples/fjordpulse/Theme/FjordPulseTheme.bn",
            include_str!("../../../../examples/fjordpulse/Theme/FjordPulseTheme.bn"),
        ),
        (
            "examples/fjordpulse/Model/FjordPulseModel.bn",
            include_str!("../../../../examples/fjordpulse/Model/FjordPulseModel.bn"),
        ),
        (
            "examples/fjordpulse/View/FjordPulseComponents.bn",
            include_str!("../../../../examples/fjordpulse/View/FjordPulseComponents.bn"),
        ),
        (
            "examples/fjordpulse/View/FjordPulseMap.bn",
            include_str!("../../../../examples/fjordpulse/View/FjordPulseMap.bn"),
        ),
        (
            "examples/fjordpulse/View/FjordPulseView.bn",
            include_str!("../../../../examples/fjordpulse/View/FjordPulseView.bn"),
        ),
        (
            "examples/fjordpulse/Client/RUN.bn",
            include_str!("../../../../examples/fjordpulse/Client/RUN.bn"),
        ),
    ]
    .into_iter()
    .map(|(path, source)| (path.to_owned(), source.to_owned()))
    .collect::<Vec<_>>();
    let parsed = boon_parser::parse_project("examples/fjordpulse/Client/RUN.bn", units).unwrap();
    let report = check(&parsed);

    assert!(
        !report.has_errors(),
        "unexpected diagnostics: {:?}; locale function: {:?}",
        report.diagnostics,
        report
            .function_type_table
            .entries
            .iter()
            .find(|entry| entry.name == "FjordPulseModel/locale_text")
    );
}
