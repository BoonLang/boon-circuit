#[test]
fn map_keys_accept_only_closed_recursive_key_safe_data() {
    let closed_record = Type::Object(ObjectShape::from_ordered_fields(
        [
            ("scope".to_owned(), Type::Text),
            (
                "state".to_owned(),
                Type::VariantSet(vec![Variant::Tag("Ready".to_owned())]),
            ),
        ],
        false,
    ));
    let closed_tagged = Type::VariantSet(vec![Variant::Tagged {
        tag: "Cell".to_owned(),
        fields: ObjectShape::from_ordered_fields(
            [("column".to_owned(), Type::Number)],
            false,
        ),
    }]);

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
        Type::Object(ObjectShape::new(BTreeMap::new(), true)),
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
fn map_and_set_builtins_preserve_typed_collection_shapes() {
    let parsed = boon_parser::parse_source(
        "typed-map-set-builtins.bn",
        r#"
users: MAP {
    TEXT { alice } => [score: 1]
}
updated_users:
    users
    |> Map/upsert(entry: [key: TEXT { bob }, value: [score: 2]])
selected_user:
    updated_users
    |> Map/get(key: TEXT { bob })
roles: SET {
    Admin
    Editor
}
updated_roles:
    roles
    |> Set/add(item: Editor)
has_editor:
    updated_roles
    |> Set/contains(item: Editor)
"#,
    )
    .unwrap();
    let output = check_program(&parsed);
    assert!(
        !output.report.has_errors(),
        "diagnostics: {:#?}",
        output.report.diagnostics
    );

    let named = |path: &str| {
        output
            .report
            .named_value_type_table
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| entry.flow_type.ty.clone())
            .unwrap_or_else(|| panic!("missing named type `{path}`"))
    };
    let Type::Map { key, value } = named("updated_users") else {
        panic!("Map/upsert did not preserve a MAP type");
    };
    assert_eq!(*key, Type::Text);
    assert!(matches!(&*value, Type::Object(_)));
    assert_eq!(
        named("selected_user"),
        found_or_not_found_type((*value).clone())
    );
    assert_eq!(
        named("updated_roles"),
        Type::Set(Box::new(Type::VariantSet(vec![
            Variant::Tag("Admin".to_owned()),
            Variant::Tag("Editor".to_owned()),
        ])))
    );
    assert_eq!(named("has_editor"), true_false_type());
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
