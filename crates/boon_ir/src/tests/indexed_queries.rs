#[test]
fn multiline_indexed_query_arguments_are_metadata_not_derived_fields() {
    let parsed = boon_parser::parse_source(
        "indexed-query-multiline.bn",
        r#"
store: [
    prefix: TEXT { os }
    catalog: LIST {
        [id: TEXT { 1 }, name: TEXT { Oslo }]
        [id: TEXT { 2 }, name: TEXT { Bergen }]
    }
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
    let typed = lower(&parsed).unwrap();

    assert_eq!(typed.list_projections.len(), 1);
    assert!(matches!(
        &typed.list_projections[0].kind,
        ListProjectionKind::TextPrefix {
            field,
            prefix,
            limit: Some(8),
            normalization: ListTextNormalization::TrimLowercase,
        } if field == "name" && prefix == "store.prefix"
    ));
    assert!(
        typed
            .derived_values
            .iter()
            .any(|value| value.path == "store.results")
    );
    assert!(typed.derived_values.iter().all(|value| {
        !matches!(
            value.path.as_str(),
            "store.results.field"
                | "store.results.prefix"
                | "store.results.limit"
                | "store.results.normalization"
        )
    }));
}

#[test]
fn generic_compound_query_lowers_closed_index_and_selection_metadata() {
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
    let typed = lower(&parsed).unwrap();
    let [projection] = typed.list_projections.as_slice() else {
        panic!("expected one indexed query projection");
    };
    assert!(matches!(
        &projection.kind,
        ListProjectionKind::IndexedQuery {
            fields,
            selection: ListQuerySelection::Exact { key },
            limit: Some(10),
            order: ListQueryOrder::Ascending,
            ..
        } if fields.len() == 2
            && fields.iter().all(|field| field.normalization == ListTextNormalization::TrimLowercase)
            && key == "store.key"
    ));
    assert!(typed.derived_values.iter().all(|value| {
        !matches!(
            value.path.as_str(),
            "store.page.fields"
                | "store.page.normalization"
                | "store.page.select"
                | "store.page.limit"
                | "store.page.order"
                | "store.page.residual"
        )
    }));
}

#[test]
fn terminal_list_reducer_inside_latest_is_a_source_transform_not_a_list_view() {
    let parsed = boon_parser::parse_source(
        "latest-list-reducer.bn",
        r#"
store: [
    refresh: SOURCE
    catalog: LIST {
        [name: TEXT { city } value: TEXT { Oslo }]
        [name: TEXT { country } value: TEXT { Norway }]
    }
    joined:
        LATEST {
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
    let typed = lower(&parsed).unwrap();

    assert!(
        !typed
            .state_cells
            .iter()
            .any(|cell| cell.path == "store.joined")
    );
    assert!(typed.derived_values.iter().any(|value| {
        value.path == "store.joined" && value.kind == DerivedValueKind::SourceEventTransform
    }));
}
