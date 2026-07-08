// Included by `../tests.rs`; kept in the parent test module for private executor-helper access.

#[test]
fn semantic_delta_signature_uses_kind_and_field_path() {
    assert_eq!(
        semantic_delta_signature(&json!({
            "kind": "FieldSet",
            "field_path": "store.flag"
        }))
        .unwrap(),
        "FieldSet:store.flag"
    );
    assert_eq!(
        semantic_delta_signature(&json!({
            "kind": "ListInsert",
            "field_path": null
        }))
        .unwrap(),
        "ListInsert"
    );
}


#[test]
fn indexed_update_delta_ordering_is_executor_owned() {
    let primary_a = json!({
        "kind": "FieldSet",
        "field_path": "value",
        "value": "A"
    });
    let derived_a = json!({
        "kind": "FieldSet",
        "field_path": "display_text",
        "value": "A"
    });
    let primary_b = json!({
        "kind": "FieldSet",
        "field_path": "value",
        "value": "B"
    });
    let batches = vec![
        IndexedUpdateDeltaBatch {
            semantic_deltas: vec![derived_a.clone(), primary_a.clone()],
            report_rows: vec![json!({ "field_path": "value" })],
        },
        IndexedUpdateDeltaBatch {
            semantic_deltas: vec![primary_b.clone()],
            report_rows: vec![json!({ "field_path": "value" })],
        },
    ];

    let bulk = order_indexed_update_semantic_deltas(true, &batches);
    assert_eq!(
        bulk.semantic_deltas,
        vec![primary_a.clone(), primary_b.clone(), derived_a.clone()]
    );
    assert_eq!(
        bulk.executor_report["executor"],
        "cpu-plan-indexed-update-delta-ordering-v1"
    );
    assert_eq!(bulk.executor_report["bulk_indexed_update"], true);

    let non_bulk = order_indexed_update_semantic_deltas(false, &batches);
    assert_eq!(
        non_bulk.semantic_deltas,
        vec![derived_a, primary_a, primary_b]
    );
    assert_eq!(non_bulk.executor_report["bulk_indexed_update"], false);
}
