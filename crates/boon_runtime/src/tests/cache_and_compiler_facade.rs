// Included by `../tests.rs`; kept in the parent test module for private invariant access.

#[test]
fn record_columns_cache_fragment_is_stable_across_field_insert_order() {
    let mut left = ValueColumns::default();
    left.insert_value("title".to_owned(), FieldValue::Text("A".to_owned()));
    left.insert_value("completed".to_owned(), FieldValue::Bool(true));
    left.insert_value("status".to_owned(), FieldValue::Enum("Ready".to_owned()));

    let mut right = ValueColumns::default();
    right.insert_value("status".to_owned(), FieldValue::Enum("Ready".to_owned()));
    right.insert_value("completed".to_owned(), FieldValue::Bool(true));
    right.insert_value("title".to_owned(), FieldValue::Text("A".to_owned()));

    assert_eq!(
        boon_value_cache_fragment(&BoonValue::RecordColumns(left)),
        boon_value_cache_fragment(&BoonValue::RecordColumns(right))
    );
}


#[test]
fn record_columns_cache_fragment_separates_field_value_types() {
    let mut text_true = ValueColumns::default();
    text_true.insert_value("flag".to_owned(), FieldValue::Text("true".to_owned()));
    let mut bool_true = ValueColumns::default();
    bool_true.insert_value("flag".to_owned(), FieldValue::Bool(true));
    assert_ne!(
        boon_value_cache_fragment(&BoonValue::RecordColumns(text_true)),
        boon_value_cache_fragment(&BoonValue::RecordColumns(bool_true))
    );

    let mut text_a = ValueColumns::default();
    text_a.insert_value("kind".to_owned(), FieldValue::Text("A".to_owned()));
    let mut enum_a = ValueColumns::default();
    enum_a.insert_value("kind".to_owned(), FieldValue::Enum("A".to_owned()));
    assert_ne!(
        boon_value_cache_fragment(&BoonValue::RecordColumns(text_a)),
        boon_value_cache_fragment(&BoonValue::RecordColumns(enum_a))
    );
}
