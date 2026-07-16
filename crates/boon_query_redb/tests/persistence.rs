use boon_data::{FiniteReal, Value};
use boon_query::{
    Collection, CollectionPlan, IndexFieldPlan, IndexKey, IndexPlan, KeyPart, QueryError,
    QueryPlan, QuerySelection, ResidualPredicate, RowId, TextNormalization,
};
use boon_query_redb::{PersistentQueryError, RedbCollection};
use redb::{Database, ReadableTable, TableDefinition};
use std::collections::BTreeMap;
use tempfile::TempDir;

const RAW_ROWS: TableDefinition<&str, &[u8]> = TableDefinition::new("boon_query_redb.rows.v1");
const RAW_INDEX_ENTRIES: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("boon_query_redb.index_entries.v1");

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

#[derive(Clone)]
struct FixturePlans {
    collection: CollectionPlan,
    by_name: IndexPlan,
    by_city_name: IndexPlan,
    by_mode: IndexPlan,
}

impl FixturePlans {
    fn new() -> Self {
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
        Self {
            collection,
            by_name,
            by_city_name,
            by_mode,
        }
    }

    fn indexes(&self) -> Vec<IndexPlan> {
        vec![
            self.by_name.clone(),
            self.by_city_name.clone(),
            self.by_mode.clone(),
        ]
    }

    fn in_memory(&self) -> Collection {
        Collection::new(self.collection.clone(), self.indexes()).unwrap()
    }
}

fn fixture_rows() -> Vec<Value> {
    vec![
        row("1", "Oslo S", "Oslo", &["rail", "metro"], 59.9109, 10.7523),
        row("2", "Oslo bus terminal", "Oslo", &["bus"], 59.9111, 10.7550),
        row(
            "3",
            "Bergen station",
            "Bergen",
            &["rail", "bus"],
            60.3905,
            5.3331,
        ),
        row("4", "Trondheim S", "Trondheim", &["rail"], 63.4366, 10.3988),
        row(
            "5",
            "Oslo harbor",
            "Oslo",
            &["ferry", "bus"],
            59.9020,
            10.7280,
        ),
    ]
}

fn exact(value: &str) -> QuerySelection {
    QuerySelection::Exact {
        key: IndexKey::new(vec![KeyPart::Text(value.to_owned())]).unwrap(),
    }
}

#[test]
fn golden_query_results_cursors_and_metrics_match_in_memory() {
    let temp = TempDir::new().unwrap();
    let plans = FixturePlans::new();
    let values = fixture_rows();
    let mut memory = plans.in_memory();
    let mut persistent = RedbCollection::open(
        temp.path().join("stations.redb"),
        plans.collection.clone(),
        plans.indexes(),
    )
    .unwrap();
    for value in &values {
        memory.upsert(value.clone()).unwrap();
    }
    persistent.upsert_batch(values).unwrap();

    let queries = vec![
        QueryPlan {
            index: plans.by_name.id,
            selection: exact("bergen station"),
            residual: vec![],
            limit: 10,
            cursor: None,
        },
        QueryPlan {
            index: plans.by_name.id,
            selection: QuerySelection::TextPrefix {
                leading: vec![],
                prefix: "oslo".to_owned(),
            },
            residual: vec![],
            limit: 10,
            cursor: None,
        },
        QueryPlan {
            index: plans.by_city_name.id,
            selection: QuerySelection::TextPrefix {
                leading: vec![KeyPart::Text("oslo".to_owned())],
                prefix: "oslo b".to_owned(),
            },
            residual: vec![],
            limit: 10,
            cursor: None,
        },
        QueryPlan {
            index: plans.by_mode.id,
            selection: QuerySelection::Union {
                selections: vec![exact("rail"), exact("bus")],
            },
            residual: vec![],
            limit: 10,
            cursor: None,
        },
        QueryPlan {
            index: plans.by_mode.id,
            selection: QuerySelection::Intersection {
                selections: vec![exact("rail"), exact("bus")],
            },
            residual: vec![],
            limit: 10,
            cursor: None,
        },
        QueryPlan {
            index: plans.by_city_name.id,
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
        },
    ];
    for query in queries {
        assert_eq!(
            persistent.query(&query).unwrap(),
            memory.query(&query).unwrap()
        );
    }

    let page_query = QueryPlan {
        index: plans.by_name.id,
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
    let memory_first = memory.query(&page_query).unwrap();
    let persistent_first = persistent.query(&page_query).unwrap();
    assert_eq!(persistent_first, memory_first);
    let second_query = QueryPlan {
        cursor: persistent_first.next_cursor,
        ..page_query
    };
    assert_eq!(
        persistent.query(&second_query).unwrap(),
        memory.query(&second_query).unwrap()
    );
}

fn user_plans() -> (CollectionPlan, IndexPlan) {
    let collection = CollectionPlan::new("users", vec!["id".to_owned()]).unwrap();
    let index = IndexPlan::new(
        collection.id,
        "user_name",
        vec![IndexFieldPlan::field("name")],
        true,
    )
    .unwrap();
    (collection, index)
}

fn user(id: impl Into<String>, name: impl Into<String>) -> Value {
    Value::Record(BTreeMap::from([
        ("id".to_owned(), text(id)),
        ("name".to_owned(), text(name)),
    ]))
}

#[test]
fn mutations_are_atomic_and_cursor_epochs_survive_reopen() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("users.redb");
    let (collection, index) = user_plans();
    let mut memory = Collection::new(collection.clone(), vec![index.clone()]).unwrap();
    let mut persistent =
        RedbCollection::open(&path, collection.clone(), vec![index.clone()]).unwrap();
    for position in 0..5 {
        let value = user(position.to_string(), format!("user-{position}"));
        memory.upsert(value.clone()).unwrap();
        persistent.upsert(value).unwrap();
    }
    let page = QueryPlan {
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
    let first = persistent.query(&page).unwrap();
    let cursor = first.next_cursor.clone().unwrap();
    drop(persistent);

    let mut persistent =
        RedbCollection::open(&path, collection.clone(), vec![index.clone()]).unwrap();
    assert_eq!(persistent.epoch(), memory.epoch());
    let second_page = QueryPlan {
        cursor: Some(cursor.clone()),
        ..page.clone()
    };
    assert_eq!(
        persistent.query(&second_page).unwrap(),
        memory.query(&second_page).unwrap()
    );

    let update = user("2", "renamed-user");
    memory.upsert(update.clone()).unwrap();
    persistent.upsert(update).unwrap();
    assert!(matches!(
        persistent.query(&QueryPlan {
            cursor: Some(cursor),
            ..page.clone()
        }),
        Err(PersistentQueryError::Query(QueryError::StaleCursor))
    ));

    let removed_id = RowId::new("4").unwrap();
    assert_eq!(
        persistent.remove(&removed_id).unwrap(),
        memory.remove(&removed_id).unwrap()
    );
    let epoch_before_conflict = persistent.epoch();
    assert!(matches!(
        persistent.upsert(user("other", "user-0")),
        Err(PersistentQueryError::Query(
            QueryError::UniqueConflict { .. }
        ))
    ));
    assert_eq!(persistent.epoch(), epoch_before_conflict);
    drop(persistent);

    let persistent = RedbCollection::open(&path, collection, vec![index.clone()]).unwrap();
    assert_eq!(persistent.epoch(), memory.epoch());
    assert_eq!(persistent.len(), memory.len());
    assert_eq!(
        persistent.query(&page).unwrap(),
        memory.query(&page).unwrap()
    );
    let status = persistent.authority_status();
    assert_eq!(status.collection_epoch, persistent.epoch());
    assert_eq!(status.row_count, persistent.len() as u64);
    assert_eq!(status.indexes[&index.id].epoch, persistent.epoch());
    assert_ne!(status.collection_plan_hash, [0; 32]);
    assert_ne!(status.indexes[&index.id].plan_hash, [0; 32]);
}

#[test]
fn strict_open_rejects_corrupt_index_entries_and_explicit_rebuild_restores_them() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("rebuild.redb");
    let plans = FixturePlans::new();
    let mut memory = plans.in_memory();
    let values = fixture_rows();
    let mut persistent =
        RedbCollection::open(&path, plans.collection.clone(), plans.indexes()).unwrap();
    for value in &values {
        memory.upsert(value.clone()).unwrap();
    }
    persistent.upsert_batch(values).unwrap();
    drop(persistent);

    corrupt_one_index_entry(&path);
    let error = RedbCollection::open(&path, plans.collection.clone(), plans.indexes())
        .err()
        .expect("strict open must reject a corrupt derived entry");
    assert!(matches!(error, PersistentQueryError::CorruptAuthority(_)));

    let rebuilt =
        RedbCollection::open_rebuilding_indexes(&path, plans.collection.clone(), plans.indexes())
            .unwrap();
    let query = QueryPlan {
        index: plans.by_name.id,
        selection: QuerySelection::TextPrefix {
            leading: vec![],
            prefix: "oslo".to_owned(),
        },
        residual: vec![],
        limit: 20,
        cursor: None,
    };
    assert_eq!(
        rebuilt.query(&query).unwrap(),
        memory.query(&query).unwrap()
    );
    drop(rebuilt);
    RedbCollection::open(&path, plans.collection.clone(), plans.indexes()).unwrap();
}

#[test]
fn canonical_corruption_and_incompatible_plans_are_rejected() {
    let temp = TempDir::new().unwrap();
    let corrupt_path = temp.path().join("corrupt.redb");
    let plans = FixturePlans::new();
    let mut persistent =
        RedbCollection::open(&corrupt_path, plans.collection.clone(), plans.indexes()).unwrap();
    persistent.upsert_batch(fixture_rows()).unwrap();
    drop(persistent);
    corrupt_one_row(&corrupt_path);
    let strict_error =
        RedbCollection::open(&corrupt_path, plans.collection.clone(), plans.indexes())
            .err()
            .expect("strict open must reject canonical corruption");
    assert!(matches!(
        strict_error,
        PersistentQueryError::CorruptAuthority(_)
    ));
    let rebuild_error = RedbCollection::open_rebuilding_indexes(
        &corrupt_path,
        plans.collection.clone(),
        plans.indexes(),
    )
    .err()
    .expect("index rebuilding must not mask canonical corruption");
    assert!(matches!(
        rebuild_error,
        PersistentQueryError::CorruptAuthority(_)
    ));

    let incompatible_path = temp.path().join("incompatible.redb");
    let (collection, index) = user_plans();
    let persistent =
        RedbCollection::open(&incompatible_path, collection.clone(), vec![index.clone()]).unwrap();
    drop(persistent);

    let other_collection = CollectionPlan::new("other_users", vec!["id".to_owned()]).unwrap();
    let other_index = IndexPlan::new(
        other_collection.id,
        "user_name",
        vec![IndexFieldPlan::field("name")],
        true,
    )
    .unwrap();
    let collection_error =
        RedbCollection::open(&incompatible_path, other_collection, vec![other_index])
            .err()
            .expect("collection plan changes must be rejected");
    assert!(matches!(
        collection_error,
        PersistentQueryError::IncompatibleCollectionPlan { .. }
    ));

    let replacement_index = IndexPlan::new(
        collection.id,
        "user_name_v2",
        vec![IndexFieldPlan::field("name")],
        true,
    )
    .unwrap();
    let index_error = RedbCollection::open(
        &incompatible_path,
        collection.clone(),
        vec![replacement_index.clone()],
    )
    .err()
    .expect("index plan changes require explicit rebuilding");
    assert_eq!(index_error, PersistentQueryError::IncompatibleIndexPlans);

    let rebuilt = RedbCollection::open_rebuilding_indexes(
        &incompatible_path,
        collection,
        vec![replacement_index],
    )
    .unwrap();
    assert!(rebuilt.is_empty());
}

#[test]
fn reopened_sixty_thousand_row_fixture_keeps_index_visits_bounded() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("large.redb");
    let collection = CollectionPlan::new("catalog", vec!["id".to_owned()]).unwrap();
    let by_name = IndexPlan::new(
        collection.id,
        "catalog_name",
        vec![IndexFieldPlan {
            path: vec!["name".to_owned()],
            text_normalization: TextNormalization::TrimLowercase,
            multi_value: false,
        }],
        true,
    )
    .unwrap();
    let by_region_time = IndexPlan::new(
        collection.id,
        "catalog_region_time",
        vec![
            IndexFieldPlan {
                path: vec!["region".to_owned()],
                text_normalization: TextNormalization::Exact,
                multi_value: false,
            },
            IndexFieldPlan::field("timestamp"),
        ],
        true,
    )
    .unwrap();
    let by_mode = IndexPlan::new(
        collection.id,
        "catalog_mode",
        vec![IndexFieldPlan {
            path: vec!["modes".to_owned()],
            text_normalization: TextNormalization::Exact,
            multi_value: true,
        }],
        false,
    )
    .unwrap();
    let by_spatial_cell = IndexPlan::new(
        collection.id,
        "catalog_spatial_cell",
        vec![IndexFieldPlan::field("spatial_cell")],
        false,
    )
    .unwrap();
    let by_kind = IndexPlan::new(
        collection.id,
        "catalog_kind",
        vec![IndexFieldPlan::field("kind")],
        false,
    )
    .unwrap();
    let indexes = vec![
        by_name.clone(),
        by_region_time.clone(),
        by_mode.clone(),
        by_spatial_cell.clone(),
        by_kind.clone(),
    ];
    let mut values = Vec::with_capacity(60_000);
    for position in 0..60_000 {
        let selected = (31_000..31_024).contains(&position);
        let name = if selected {
            format!("needle-{position:05}")
        } else {
            format!("station-{position:05}")
        };
        values.push(Value::Record(BTreeMap::from([
            ("id".to_owned(), text(format!("row:{position}"))),
            ("name".to_owned(), text(name)),
            (
                "region".to_owned(),
                text(if selected {
                    "selected".to_owned()
                } else {
                    format!("region-{:04}", position % 1_000)
                }),
            ),
            ("timestamp".to_owned(), number(position as f64)),
            (
                "modes".to_owned(),
                Value::List(if selected {
                    vec![text("featured"), text("rail")]
                } else {
                    vec![text("bus")]
                }),
            ),
            (
                "spatial_cell".to_owned(),
                text(if selected {
                    "cell:selected".to_owned()
                } else {
                    format!("cell:{:05}", position)
                }),
            ),
            (
                "latitude".to_owned(),
                number(if selected && position % 2 == 0 {
                    59.91
                } else {
                    61.0
                }),
            ),
            (
                "longitude".to_owned(),
                number(if selected && position % 2 == 0 {
                    10.75
                } else {
                    12.0
                }),
            ),
            (
                "kind".to_owned(),
                Value::Variant {
                    tag: if selected { "Featured" } else { "Ordinary" }.to_owned(),
                    fields: BTreeMap::new(),
                },
            ),
        ])));
    }

    let mut persistent = RedbCollection::open(&path, collection.clone(), indexes.clone()).unwrap();
    persistent.upsert_batch(values).unwrap();
    assert_eq!(persistent.len(), 60_000);
    let prefix_plan = QueryPlan {
        index: by_name.id,
        selection: QuerySelection::TextPrefix {
            leading: vec![],
            prefix: "NEEDLE-".to_owned(),
        },
        residual: vec![],
        limit: 10,
        cursor: None,
    };
    let first_page = persistent.query(&prefix_plan).unwrap();
    assert_eq!(first_page.rows.len(), 10);
    assert_eq!(first_page.metrics.keys_visited, 24);
    assert_eq!(first_page.metrics.rows_examined, 11);
    assert_eq!(first_page.metrics.full_scans, 0);
    let second_page = persistent
        .query(&QueryPlan {
            cursor: first_page.next_cursor.clone(),
            ..prefix_plan.clone()
        })
        .unwrap();
    assert_eq!(second_page.rows.len(), 10);
    assert!(second_page.metrics.rows_examined <= 21);

    let compound = persistent
        .query(&QueryPlan {
            index: by_region_time.id,
            selection: QuerySelection::Range {
                lower: Some(
                    IndexKey::new(vec![
                        KeyPart::Text("selected".to_owned()),
                        KeyPart::Number(FiniteReal::new(31_005.0).unwrap()),
                    ])
                    .unwrap(),
                ),
                lower_inclusive: true,
                upper: Some(
                    IndexKey::new(vec![
                        KeyPart::Text("selected".to_owned()),
                        KeyPart::Number(FiniteReal::new(31_012.0).unwrap()),
                    ])
                    .unwrap(),
                ),
                upper_inclusive: true,
            },
            residual: vec![],
            limit: 20,
            cursor: None,
        })
        .unwrap();
    assert_eq!(compound.rows.len(), 8);
    assert_eq!(compound.metrics.keys_visited, 8);
    assert_eq!(compound.metrics.rows_examined, 8);

    let exact = |value: &str| QuerySelection::Exact {
        key: IndexKey::new(vec![KeyPart::Text(value.to_owned())]).unwrap(),
    };
    let union = persistent
        .query(&QueryPlan {
            index: by_mode.id,
            selection: QuerySelection::Union {
                selections: vec![exact("featured"), exact("rail")],
            },
            residual: vec![],
            limit: 30,
            cursor: None,
        })
        .unwrap();
    let intersection = persistent
        .query(&QueryPlan {
            index: by_mode.id,
            selection: QuerySelection::Intersection {
                selections: vec![exact("featured"), exact("rail")],
            },
            residual: vec![],
            limit: 30,
            cursor: None,
        })
        .unwrap();
    assert_eq!(union.rows.len(), 24);
    assert_eq!(intersection.rows.len(), 24);
    assert_eq!(union.metrics.keys_visited, 2);
    assert_eq!(intersection.metrics.keys_visited, 2);

    let nearby = persistent
        .query(&QueryPlan {
            index: by_spatial_cell.id,
            selection: exact("cell:selected"),
            residual: vec![ResidualPredicate::Wgs84Radius {
                latitude_path: vec!["latitude".to_owned()],
                longitude_path: vec!["longitude".to_owned()],
                center_latitude: FiniteReal::new(59.91).unwrap(),
                center_longitude: FiniteReal::new(10.75).unwrap(),
                radius_meters: FiniteReal::new(2_000.0).unwrap(),
            }],
            limit: 20,
            cursor: None,
        })
        .unwrap();
    assert_eq!(nearby.rows.len(), 12);
    assert_eq!(nearby.metrics.rows_examined, 24);
    assert_eq!(nearby.metrics.residual_evaluations, 24);

    let featured = persistent
        .query(&QueryPlan {
            index: by_kind.id,
            selection: QuerySelection::Exact {
                key: IndexKey::new(vec![KeyPart::Tag("Featured".to_owned())]).unwrap(),
            },
            residual: vec![],
            limit: 30,
            cursor: None,
        })
        .unwrap();
    assert_eq!(featured.rows.len(), 24);
    assert_eq!(featured.metrics.keys_visited, 1);

    let stale_cursor = first_page.next_cursor.unwrap();
    persistent
        .upsert(Value::Record(BTreeMap::from([
            ("id".to_owned(), text("row:31000")),
            ("name".to_owned(), text("renamed-31000")),
            ("region".to_owned(), text("selected")),
            ("timestamp".to_owned(), number(31_000.0)),
            (
                "modes".to_owned(),
                Value::List(vec![text("featured"), text("rail")]),
            ),
            ("spatial_cell".to_owned(), text("cell:selected")),
            ("latitude".to_owned(), number(59.91)),
            ("longitude".to_owned(), number(10.75)),
            (
                "kind".to_owned(),
                Value::Variant {
                    tag: "Featured".to_owned(),
                    fields: BTreeMap::new(),
                },
            ),
        ])))
        .unwrap();
    assert!(matches!(
        persistent.query(&QueryPlan {
            cursor: Some(stale_cursor),
            ..prefix_plan.clone()
        }),
        Err(PersistentQueryError::Query(QueryError::StaleCursor))
    ));
    let before_restart = persistent
        .query(&QueryPlan {
            limit: 100,
            ..prefix_plan
        })
        .unwrap();
    assert_eq!(before_restart.rows.len(), 23);
    let expected_ids = before_restart
        .rows
        .iter()
        .map(|row| row.id.clone())
        .collect::<Vec<_>>();
    drop(persistent);

    let persistent = RedbCollection::open(&path, collection, indexes).unwrap();
    let after_restart = persistent
        .query(&QueryPlan {
            index: by_name.id,
            selection: QuerySelection::TextPrefix {
                leading: vec![],
                prefix: "needle-".to_owned(),
            },
            residual: vec![],
            limit: 100,
            cursor: None,
        })
        .unwrap();
    assert_eq!(
        after_restart
            .rows
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>(),
        expected_ids
    );
    assert_eq!(after_restart.metrics.keys_visited, 23);
    assert_eq!(after_restart.metrics.rows_examined, 23);
    assert_eq!(after_restart.metrics.full_scans, 0);
}

fn corrupt_one_index_entry(path: &std::path::Path) {
    let database = Database::create(path).unwrap();
    let transaction = database.begin_write().unwrap();
    {
        let mut table = transaction.open_table(RAW_INDEX_ENTRIES).unwrap();
        let first = table.first().unwrap().expect("fixture has index entries");
        let key = first.0.value().to_vec();
        drop(first);
        table.remove(key.as_slice()).unwrap();
        table
            .insert(b"not-cbor".as_slice(), b"bad-marker".as_slice())
            .unwrap();
    }
    transaction.commit().unwrap();
}

fn corrupt_one_row(path: &std::path::Path) {
    let database = Database::create(path).unwrap();
    let transaction = database.begin_write().unwrap();
    {
        let mut table = transaction.open_table(RAW_ROWS).unwrap();
        let first = table.first().unwrap().expect("fixture has canonical rows");
        let key = first.0.value().to_owned();
        drop(first);
        table.insert(key.as_str(), b"corrupt".as_slice()).unwrap();
    }
    transaction.commit().unwrap();
}
