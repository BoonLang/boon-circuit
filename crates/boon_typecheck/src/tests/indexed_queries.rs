#[test]
fn indexed_query_symbols_do_not_require_value_bindings() {
    let parsed = boon_parser::parse_source(
        "indexed-query-symbols.bn",
        r#"
store: [
    prefix: TEXT { os }
    catalog: LIST { [name: TEXT { Oslo }] }
    results:
        List/query_prefix(
            catalog
            field: name
            prefix: prefix
            limit: 8
            normalization: TrimLowercase
        )
]
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
}

#[test]
fn generic_compound_query_declaration_is_typed_as_a_page() {
    let parsed = boon_parser::parse_source(
        "generic-compound-query.bn",
        r#"
store: [
    catalog: LIST {
        [id: TEXT { 1 }, city: TEXT { Oslo }, name: TEXT { Alpha }]
        [id: TEXT { 2 }, city: TEXT { Bergen }, name: TEXT { Beta }]
    }
    key: [city: TEXT { OSLO }, name: TEXT { alpha }]
    page:
        List/query(
            catalog
            fields: TEXT { city,name }
            normalization: TEXT { TrimLowercase,TrimLowercase }
            select: Exact
            key: key
            limit: 10
            order: Ascending
            residual: None
        )
]
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
}

#[test]
fn latest_branch_uses_the_terminal_type_of_a_multiline_list_pipeline() {
    let parsed = boon_parser::parse_source(
        "latest-multiline-list-pipeline.bn",
        r#"
store: [
    refresh: SOURCE
    catalog: LIST {
        [name: TEXT { city } value: TEXT { Oslo }]
        [name: TEXT { country } value: TEXT { Norway }]
    }
    joined:
        LATEST {
            Text/empty()
            refresh |> THEN {
                catalog
                    |> List/filter_field_equal(
                        field: "name"
                        value: TEXT { city }
                    )
                    |> List/join_field(
                        field: "value"
                        separator: Text/empty()
                    )
            }
        }
]
"#,
    )
    .unwrap();
    let report = check(&parsed);
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
}
