#[test]
fn map_keys_accept_only_closed_recursive_key_safe_data() {
    let closed_record = Type::object(ObjectShape::from_ordered_fields(
        [
            ("scope".to_owned(), Type::Text),
            (
                "state".to_owned(),
                Type::VariantSet(vec![Variant::Tag("Ready".to_owned())].into()),
            ),
        ],
        false,
    ));
    let closed_tagged = Type::VariantSet(vec![Variant::Tagged {
        tag: "Cell".to_owned(),
        fields: ObjectShape::from_ordered_fields([("column".to_owned(), Type::Number)], false),
    }]
    .into());

    for key in [
        Type::Number,
        Type::Text,
        Type::Bytes(BytesType::Dynamic),
        closed_record,
        closed_tagged,
    ] {
        assert!(type_is_map_key_safe(&key), "{key:?} should be key-safe");
    }

    for key in [
        Type::List(Box::new(Type::Text)),
        Type::Set(Box::new(Type::Text)),
        Type::Map {
            key: Box::new(Type::Text),
            value: Box::new(Type::Number),
        },
        Type::object(ObjectShape::new(BTreeMap::new(), true)),
        Type::Union(vec![Type::Text, Type::Number]),
        Type::Unknown,
    ] {
        assert!(
            !type_is_map_key_safe(&key),
            "{key:?} must not be accepted as a MAP key or SET item"
        );
    }
}

#[test]
fn map_literal_rejects_static_duplicates_and_hold_rejects_nested_collections() {
    let duplicate = boon_parser::parse_source(
        "duplicate-map-key.bn",
        r#"
users: MAP {
    1 => TEXT { first }
    1.0 => TEXT { second }
}
"#,
    )
    .unwrap();
    let duplicate = check_program(&duplicate);
    assert!(duplicate.program.is_none());
    assert!(duplicate.report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("MAP literal contains a statically duplicate key")
    }));

    let held = boon_parser::parse_source(
        "held-map.bn",
        r#"
state:
    [users: MAP {}]
    |> HOLD state {}
"#,
    )
    .unwrap();
    let held = check_program(&held);
    assert!(held.program.is_none());
    assert!(held.report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("`HOLD` state cannot contain LIST, SET, or MAP authority")
    }));
}

#[test]
fn nested_collection_authorities_require_fresh_single_parent_ownership() {
    let second_parent = boon_parser::parse_source(
        "nested-authority-second-parent.bn",
        r#"
child: MAP {
    TEXT { sku } => 1
}
parents: LIST {
    [name: A, child: child]
    [name: B, child: child]
}
"#,
    )
    .unwrap();
    let second_parent = check_program(&second_parent);
    assert!(second_parent.program.is_none());
    assert!(second_parent.report.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("second parent")
            || diagnostic
                .message
                .contains("more than one structural parent")
    }));

    let cycle = boon_parser::parse_source(
        "nested-authority-cycle.bn",
        r#"
nodes: MAP {}
cyclic:
    nodes
    |> Map/upsert(
        entry: [
            key: TEXT { node }
            value: nodes
        ]
    )
"#,
    )
    .unwrap();
    let cycle = check_program(&cycle);
    assert!(cycle.program.is_none());
    assert!(cycle.report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("attachment forms an ownership cycle")
    }));

    let escaped = boon_parser::parse_source(
        "nested-authority-lifetime-escape.bn",
        r#"
rows: LIST {
    [
        id: TEXT { row }
        child: MAP { TEXT { sku } => 1 }
    ]
}
escaped:
    rows
    |> List/map(item, new:
        MAP {
            TEXT { copy } => item.child
        }
    )
"#,
    )
    .unwrap();
    let escaped = check_program(&escaped);
    assert!(escaped.program.is_none());
    assert!(
        escaped.report.diagnostics.iter().any(|diagnostic| {
            diagnostic.message.contains("escapes its owner")
                || diagnostic.message.contains("beyond its owner lifetime")
        }),
        "diagnostics: {:#?}",
        escaped.report.diagnostics
    );
}

#[test]
fn nested_collection_authorities_accept_fresh_parent_local_construction() {
    let parsed = boon_parser::parse_source(
        "nested-authority-fresh-parent-local.bn",
        r#"
rows: LIST {
    [
        id: TEXT { row }
        child: MAP { TEXT { sku } => 1 }
    ]
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
    assert!(output.program.is_some());
}
