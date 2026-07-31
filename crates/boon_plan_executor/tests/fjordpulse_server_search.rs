const SERVER_PATH: &str = "examples/fjordpulse/Server/RUN.bn";
const SERVER_SOURCE: &str = include_str!("../../../examples/fjordpulse/Server/RUN.bn");
const SHARED_PATH: &str = "examples/fjordpulse/Shared/FjordPulseContract.bn";
const SHARED_SOURCE: &str =
    include_str!("../../../examples/fjordpulse/Shared/FjordPulseContract.bn");

#[test]
fn fjordpulse_server_search_source_uses_a_typed_normalized_pipeline() {
    for legacy_operator in [
        ["List/", "query", "("].concat(),
        ["List/", "query", "_prefix("].concat(),
    ] {
        assert!(!SERVER_SOURCE.contains(&legacy_operator));
    }

    let pipeline_start = SERVER_SOURCE
        .find("    station_matches:")
        .expect("station_matches declaration");
    let pipeline_end = SERVER_SOURCE[pipeline_start..]
        .find("    departures:")
        .map(|offset| pipeline_start + offset)
        .expect("declaration after station_matches");
    let pipeline = &SERVER_SOURCE[pipeline_start..pipeline_end];

    let filter = pipeline.find("|> List/filter(").expect("typed filter");
    let order = pipeline
        .find("|> List/sort_by(")
        .expect("stable primary order");
    let take = pipeline.find("|> List/take(").expect("bounded result");
    assert!(
        filter < order && order < take,
        "pipeline operator order changed"
    );
    assert!(pipeline.contains("left: normalized_search_query |> Text/is_not_empty()"));
    assert!(pipeline.contains(
        "item.name\n                        |> Text/trim()\n                        |> Text/to_lowercase()\n                        |> Text/starts_with(prefix: normalized_search_query)"
    ));
    assert!(pipeline.contains(
        "key:\n                item.name\n                |> Text/trim()\n                |> Text/to_lowercase()"
    ));
    assert!(pipeline.contains("direction: Ascending"));
    assert!(pipeline.contains("|> List/take(count: 20)"));
    assert!(!pipeline.contains("field:"));
    assert!(!pipeline.contains("normalization:"));
}

#[test]
fn fjordpulse_server_search_source_is_parser_valid_during_generic_operator_cutover() {
    let diagnostics = boon_compiler::diagnose_runtime_source_units(
        SERVER_PATH,
        &[
            boon_compiler::CompilerSourceUnit {
                path: SHARED_PATH.to_owned(),
                source: SHARED_SOURCE.to_owned(),
            },
            boon_compiler::CompilerSourceUnit {
                path: SERVER_PATH.to_owned(),
                source: SERVER_SOURCE.to_owned(),
            },
        ],
    );
    let parser_errors = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.start.is_none() && diagnostic.end.is_none())
        .collect::<Vec<_>>();
    assert!(
        parser_errors.is_empty(),
        "FjordPulse source has parser errors: {parser_errors:#?}"
    );
    let unknown_operators = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.message.contains("unknown function"))
        .collect::<Vec<_>>();
    assert!(
        unknown_operators.is_empty(),
        "missing generic operators: {unknown_operators:#?}"
    );
}
