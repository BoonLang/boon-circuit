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
