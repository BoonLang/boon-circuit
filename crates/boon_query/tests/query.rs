use boon_data::{FiniteReal, Value};
use boon_query::*;
use std::collections::{BTreeMap, BTreeSet};

fn text(value: impl Into<String>) -> Value {
    Value::Text(value.into())
}

fn number(value: f64) -> Value {
    Value::Number(FiniteReal::new(value).unwrap())
}

fn row(
    id: impl Into<String>,
    name: impl Into<String>,
    city: impl Into<String>,
    modes: &[&str],
    latitude: f64,
    longitude: f64,
) -> Value {
    Value::Record(BTreeMap::from([
        ("id".to_owned(), text(id)),
        ("name".to_owned(), text(name)),
        ("city".to_owned(), text(city)),
        (
            "modes".to_owned(),
            Value::List(modes.iter().map(|mode| text(*mode)).collect()),
        ),
        ("latitude".to_owned(), number(latitude)),
        ("longitude".to_owned(), number(longitude)),
    ]))
}

fn fixture() -> (Collection, IndexPlan, IndexPlan, IndexPlan) {
    let collection = CollectionPlan::new("stations", vec!["id".to_owned()]).unwrap();
    let by_name = IndexPlan::new(
        collection.id,
        "station_name",
        vec![IndexFieldPlan {
            path: vec!["name".to_owned()],
            text_normalization: TextNormalization::TrimLowercase,
            multi_value: false,
        }],
        false,
    )
    .unwrap();
    let by_city_name = IndexPlan::new(
        collection.id,
        "station_city_name",
        vec![
            IndexFieldPlan {
                path: vec!["city".to_owned()],
                text_normalization: TextNormalization::TrimLowercase,
                multi_value: false,
            },
            IndexFieldPlan {
                path: vec!["name".to_owned()],
                text_normalization: TextNormalization::TrimLowercase,
                multi_value: false,
            },
        ],
        false,
    )
    .unwrap();
    let by_mode = IndexPlan::new(
        collection.id,
        "station_mode",
        vec![IndexFieldPlan {
            path: vec!["modes".to_owned()],
            text_normalization: TextNormalization::TrimLowercase,
            multi_value: true,
        }],
        false,
    )
    .unwrap();
    (
        Collection::new(
            collection,
            vec![by_name.clone(), by_city_name.clone(), by_mode.clone()],
        )
        .unwrap(),
        by_name,
        by_city_name,
        by_mode,
    )
}

#[test]
fn exact_prefix_compound_and_multi_value_queries_are_index_owned() {
    let (mut collection, by_name, by_city_name, by_mode) = fixture();
    for value in [
        row("1", "Oslo S", "Oslo", &["rail", "metro"], 59.9109, 10.7523),
        row("2", "Oslo bussterminal", "Oslo", &["bus"], 59.9111, 10.7550),
        row(
            "3",
            "Bergen stasjon",
            "Bergen",
            &["rail", "bus"],
            60.3905,
            5.3331,
        ),
    ] {
        collection.upsert(value).unwrap();
    }

    let exact = collection
        .query(&QueryPlan {
            index: by_name.id,
            selection: QuerySelection::Exact {
                key: IndexKey::new(vec![KeyPart::Text("bergen stasjon".to_owned())]).unwrap(),
            },
            residual: vec![],
            limit: 10,
            cursor: None,
        })
        .unwrap();
    assert_eq!(exact.rows[0].id.0, "3");
    assert_eq!(exact.metrics.keys_visited, 1);

    let prefix = collection
        .query(&QueryPlan {
            index: by_name.id,
            selection: QuerySelection::TextPrefix {
                leading: vec![],
                prefix: "oslo".to_owned(),
            },
            residual: vec![],
            limit: 10,
            cursor: None,
        })
        .unwrap();
    assert_eq!(
        prefix
            .rows
            .iter()
            .map(|row| row.id.0.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["1", "2"])
    );
    assert_eq!(prefix.metrics.keys_visited, 2);

    let compound = collection
        .query(&QueryPlan {
            index: by_city_name.id,
            selection: QuerySelection::TextPrefix {
                leading: vec![KeyPart::Text("oslo".to_owned())],
                prefix: "oslo b".to_owned(),
            },
            residual: vec![],
            limit: 10,
            cursor: None,
        })
        .unwrap();
    assert_eq!(compound.rows.len(), 1);
    assert_eq!(compound.rows[0].id.0, "2");

    let modes = collection
        .query(&QueryPlan {
            index: by_mode.id,
            selection: QuerySelection::Exact {
                key: IndexKey::new(vec![KeyPart::Text("rail".to_owned())]).unwrap(),
            },
            residual: vec![],
            limit: 10,
            cursor: None,
        })
        .unwrap();
    assert_eq!(modes.rows.len(), 2);
    assert_eq!(modes.metrics.keys_visited, 1);
}

#[test]
fn mutation_unique_index_and_epoch_bound_cursor_are_atomic() {
    let collection_plan = CollectionPlan::new("users", vec!["id".to_owned()]).unwrap();
    let unique_name = IndexPlan::new(
        collection_plan.id,
        "user_name",
        vec![IndexFieldPlan::field("name")],
        true,
    )
    .unwrap();
    let mut collection = Collection::new(collection_plan, vec![unique_name.clone()]).unwrap();
    for index in 0..4 {
        collection
            .upsert(Value::Record(BTreeMap::from([
                ("id".to_owned(), text(index.to_string())),
                ("name".to_owned(), text(format!("user-{index}"))),
            ])))
            .unwrap();
    }
    let query = QueryPlan {
        index: unique_name.id,
        selection: QuerySelection::Range {
            lower: None,
            lower_inclusive: true,
            upper: None,
            upper_inclusive: true,
        },
        residual: vec![],
        limit: 2,
        cursor: None,
    };
    let first = collection.query(&query).unwrap();
    assert_eq!(first.rows.len(), 2);
    let cursor = first.next_cursor.clone().unwrap();
    let second = collection
        .query(&QueryPlan {
            cursor: Some(cursor.clone()),
            ..query.clone()
        })
        .unwrap();
    assert_eq!(second.rows.len(), 2);
    assert!(second.next_cursor.is_none());

    let conflict = collection.upsert(Value::Record(BTreeMap::from([
        ("id".to_owned(), text("other")),
        ("name".to_owned(), text("user-0")),
    ])));
    assert!(matches!(conflict, Err(QueryError::UniqueConflict { .. })));
    assert_eq!(collection.len(), 4);

    collection.remove(&RowId::new("3").unwrap()).unwrap();
    assert!(matches!(
        collection.query(&QueryPlan {
            cursor: Some(cursor),
            ..query
        }),
        Err(QueryError::StaleCursor)
    ));
}

#[test]
fn residual_geo_filter_and_text_helpers_are_deterministic() {
    let (mut collection, _by_name, by_city_name, _) = fixture();
    collection
        .upsert(row("1", "Oslo S", "Oslo", &["rail"], 59.9109, 10.7523))
        .unwrap();
    collection
        .upsert(row("2", "Bergen", "Bergen", &["rail"], 60.3905, 5.3331))
        .unwrap();
    let result = collection
        .query(&QueryPlan {
            index: by_city_name.id,
            selection: QuerySelection::Range {
                lower: None,
                lower_inclusive: true,
                upper: None,
                upper_inclusive: true,
            },
            residual: vec![ResidualPredicate::Wgs84Radius {
                latitude_path: vec!["latitude".to_owned()],
                longitude_path: vec!["longitude".to_owned()],
                center_latitude: FiniteReal::new(59.91).unwrap(),
                center_longitude: FiniteReal::new(10.75).unwrap(),
                radius_meters: FiniteReal::new(5_000.0).unwrap(),
            }],
            limit: 10,
            cursor: None,
        })
        .unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0].id.0, "1");
    assert!(wgs84_distance_meters(59.9109, 10.7523, 60.3905, 5.3331) > 300_000.0);
    assert_eq!(normalize_text("  FØRDE   Sentrum "), "førde sentrum");
    assert_eq!(
        normalize_tokens("Rail, bus + RAIL"),
        BTreeSet::from(["bus".to_owned(), "rail".to_owned()])
    );
    assert!(damerau_levenshtein_at_most_one("førde", "frøde"));
    assert!(damerau_levenshtein_at_most_one("oslo", "oslo"));
    assert!(!damerau_levenshtein_at_most_one("oslo", "bergen"));
}

#[test]
fn union_and_intersection_deduplicate_rows_across_multi_value_keys() {
    let (mut collection, _, _, by_mode) = fixture();
    for value in [
        row("1", "One", "Oslo", &["rail", "metro"], 59.91, 10.75),
        row("2", "Two", "Oslo", &["rail", "bus"], 59.92, 10.76),
        row("3", "Three", "Oslo", &["bus"], 59.93, 10.77),
    ] {
        collection.upsert(value).unwrap();
    }
    let exact = |value: &str| QuerySelection::Exact {
        key: IndexKey::new(vec![KeyPart::Text(value.to_owned())]).unwrap(),
    };
    let union = collection
        .query(&QueryPlan {
            index: by_mode.id,
            selection: QuerySelection::Union {
                selections: vec![exact("rail"), exact("bus")],
            },
            residual: vec![],
            limit: 10,
            cursor: None,
        })
        .unwrap();
    assert_eq!(union.rows.len(), 3);
    assert_eq!(union.metrics.keys_visited, 2);
    let reversed_union = collection
        .query(&QueryPlan {
            index: by_mode.id,
            selection: QuerySelection::Union {
                selections: vec![exact("bus"), exact("rail")],
            },
            residual: vec![],
            limit: 10,
            cursor: None,
        })
        .unwrap();
    assert_eq!(
        union.rows.iter().map(|row| &row.id).collect::<Vec<_>>(),
        reversed_union
            .rows
            .iter()
            .map(|row| &row.id)
            .collect::<Vec<_>>()
    );

    let intersection = collection
        .query(&QueryPlan {
            index: by_mode.id,
            selection: QuerySelection::Intersection {
                selections: vec![exact("rail"), exact("bus")],
            },
            residual: vec![],
            limit: 10,
            cursor: None,
        })
        .unwrap();
    assert_eq!(intersection.rows.len(), 1);
    assert_eq!(intersection.rows[0].id.0, "2");
}

#[test]
fn descending_pages_normalization_and_composite_operand_order_are_canonical() {
    let collection_plan =
        CollectionPlan::new_with_schema_hash("ordered_users", vec!["id".to_owned()], [7; 32])
            .unwrap();
    let index = IndexPlan::new_with_order(
        collection_plan.id,
        "ordered_user_name",
        vec![IndexFieldPlan {
            path: vec!["name".to_owned()],
            text_normalization: TextNormalization::TrimLowercase,
            multi_value: false,
        }],
        false,
        IndexOrder::Descending,
    )
    .unwrap();
    let mut collection = Collection::new(collection_plan, vec![index.clone()]).unwrap();
    for position in 0..5 {
        collection
            .upsert(Value::Record(BTreeMap::from([
                ("id".to_owned(), text(format!("row-{position}"))),
                ("name".to_owned(), text(format!("User {position}"))),
            ])))
            .unwrap();
    }
    let query = QueryPlan {
        index: index.id,
        selection: QuerySelection::Range {
            lower: None,
            lower_inclusive: true,
            upper: None,
            upper_inclusive: true,
        },
        residual: vec![],
        limit: 2,
        cursor: None,
    };
    let first = collection.query(&query).unwrap();
    assert_eq!(
        first
            .rows
            .iter()
            .map(|row| row.id.0.as_str())
            .collect::<Vec<_>>(),
        ["row-4", "row-3"]
    );
    let second = collection
        .query(&QueryPlan {
            cursor: first.next_cursor,
            ..query.clone()
        })
        .unwrap();
    assert_eq!(
        second
            .rows
            .iter()
            .map(|row| row.id.0.as_str())
            .collect::<Vec<_>>(),
        ["row-2", "row-1"]
    );

    let normalized = collection
        .query(&QueryPlan {
            index: index.id,
            selection: QuerySelection::Exact {
                key: IndexKey::new(vec![KeyPart::Text("  USER   3 ".to_owned())]).unwrap(),
            },
            residual: vec![],
            limit: 2,
            cursor: None,
        })
        .unwrap();
    assert_eq!(normalized.rows[0].id.0, "row-3");
    assert_eq!(normalized.metrics.full_scans, 0);
    assert!(normalized.metrics.elapsed_nanos > 0);
}

#[test]
fn sixty_thousand_row_prefix_query_visits_only_the_matching_index_range() {
    let collection_plan = CollectionPlan::new("catalog", vec!["id".to_owned()]).unwrap();
    let by_name = IndexPlan::new(
        collection_plan.id,
        "catalog_name",
        vec![IndexFieldPlan {
            path: vec!["name".to_owned()],
            text_normalization: TextNormalization::TrimLowercase,
            multi_value: false,
        }],
        true,
    )
    .unwrap();
    let mut collection = Collection::new(collection_plan, vec![by_name.clone()]).unwrap();
    for index in 0..60_000 {
        let name = if (31_000..31_024).contains(&index) {
            format!("førde-{index:05}")
        } else {
            format!("station-{index:05}")
        };
        collection
            .upsert(Value::Record(BTreeMap::from([
                ("id".to_owned(), text(format!("NSR:{index}"))),
                ("name".to_owned(), text(name)),
            ])))
            .unwrap();
    }

    let result = collection
        .query(&QueryPlan {
            index: by_name.id,
            selection: QuerySelection::TextPrefix {
                leading: vec![],
                prefix: "førde".to_owned(),
            },
            residual: vec![],
            limit: 100,
            cursor: None,
        })
        .unwrap();
    assert_eq!(collection.len(), 60_000);
    assert_eq!(result.rows.len(), 24);
    assert_eq!(result.metrics.keys_visited, 24);
    assert_eq!(result.metrics.rows_examined, 24);
    assert!(result.next_cursor.is_none());
}
