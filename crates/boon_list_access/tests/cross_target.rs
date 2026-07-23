#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;

use boon_list_access::{
    AccessError, AccessItem, AccessStream, Direction, FiniteNumber, IndexPlanId, KEY_CODEC_VERSION,
    KeyComponent, KeyKind, KeySchema, LimitKind, OrderedIndex, RowId, SourceOrderToken,
    StructuralKey, StructuralValue, WorkLimitExceeded, WorkLimits, WorkTracker,
};
use std::ops::Bound;

fn row(value: u128) -> RowId {
    RowId::from_u128(value)
}

fn token(value: u128) -> SourceOrderToken {
    SourceOrderToken::from_u128(value)
}

fn key(name: &str, score: f64) -> StructuralKey {
    StructuralKey::new(vec![
        StructuralValue::text(name),
        StructuralValue::Number(FiniteNumber::new(score).unwrap()),
    ])
    .unwrap()
}

fn number(value: i64) -> StructuralValue {
    StructuralValue::Number(FiniteNumber::new(value as f64).unwrap())
}

fn numeric_key(value: i64) -> StructuralKey {
    StructuralKey::new(vec![number(value)]).unwrap()
}

fn collect_rows(
    mut stream: boon_list_access::AccessStream<'_>,
) -> (Vec<AccessItem>, boon_list_access::AccessMetrics) {
    let mut work = WorkTracker::new(WorkLimits::default());
    let mut rows = Vec::new();
    while let Some(item) = stream.next(&mut work).unwrap() {
        rows.push(item);
    }
    (rows, work.metrics())
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn typed_index_has_one_native_and_wasm_golden_behavior() {
    let schema = KeySchema::new(vec![
        KeyComponent::new(KeyKind::Text, Direction::Asc),
        KeyComponent::new(KeyKind::Number, Direction::Desc),
    ])
    .unwrap();
    let mut index = OrderedIndex::new(IndexPlanId::from_u128(0x51), schema);
    for (row_id, source_order, name, score) in [
        (1, 30, "alpha", 3.0),
        (2, 20, "alpha", 5.0),
        (3, 10, "alpha", 5.0),
        (4, 40, "beta", 9.0),
    ] {
        index
            .insert(row(row_id), token(source_order), key(name, score))
            .unwrap();
    }

    let (ordered, ordered_metrics) = collect_rows(index.ordered_start(None).unwrap());
    assert_eq!(
        ordered.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        [row(3), row(2), row(1), row(4)]
    );
    assert_eq!(ordered_metrics.index_seeks, 1);
    assert_eq!(ordered_metrics.full_scans, 0);

    let (alpha, prefix_metrics) = collect_rows(index.text_prefix(&[], "alpha", None).unwrap());
    assert_eq!(
        alpha.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        [row(3), row(2), row(1)]
    );
    assert_eq!(prefix_metrics.candidates_visited, 3);
    assert_eq!(prefix_metrics.full_scans, 0);

    let cursor = alpha[1].cursor_key();
    let (resumed, resumed_metrics) =
        collect_rows(index.text_prefix(&[], "alpha", Some(&cursor)).unwrap());
    assert_eq!(
        resumed.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        [row(1)]
    );
    assert_eq!(resumed_metrics.cursor_seeks, 1);
    assert_eq!(resumed_metrics.candidates_visited, 1);
    assert_eq!(resumed_metrics.full_scans, 0);

    index.update(row(4), token(40), key("alpha", 6.0)).unwrap();
    let (updated, updated_metrics) = collect_rows(index.text_prefix(&[], "alpha", None).unwrap());
    assert_eq!(
        updated.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        [row(4), row(3), row(2), row(1)]
    );
    assert_eq!(updated_metrics.full_scans, 0);

    let integrity = index.validate_integrity().unwrap();
    assert_eq!(integrity.logical_rows, 4);
    assert_eq!(integrity.index_entries, 4);
    assert!(integrity.payload_bytes() > 0);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn typed_key_codec_has_one_native_and_wasm_byte_layout() {
    let tag_type = boon_list_access::TagTypeId::from_u128(0x1020_3040_5060_7080);
    let schema = KeySchema::new(vec![
        KeyComponent::new(KeyKind::Number, Direction::Asc),
        KeyComponent::new(KeyKind::Text, Direction::Asc),
        KeyComponent::new(KeyKind::Bool, Direction::Asc),
        KeyComponent::new(KeyKind::ClosedTag(tag_type), Direction::Asc),
    ])
    .unwrap();
    let encoded = schema
        .encode(
            &StructuralKey::new(vec![
                number(0),
                StructuralValue::text("a\0"),
                StructuralValue::Bool(true),
                StructuralValue::ClosedTag(boon_list_access::ClosedTag::new(tag_type, 7)),
            ])
            .unwrap(),
        )
        .unwrap();
    let mut expected = vec![
        KEY_CODEC_VERSION,
        0x11,
        0x80,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0x22,
        b'a',
        0,
        u8::MAX,
        0,
        0,
        0x33,
        1,
        0x44,
    ];
    expected.extend_from_slice(tag_type.as_bytes());
    expected.extend_from_slice(&7_u32.to_be_bytes());
    assert_eq!(encoded.as_bytes(), expected);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn range_composition_and_work_limits_have_one_native_and_wasm_behavior() {
    let schema = KeySchema::new(vec![KeyComponent::new(KeyKind::Number, Direction::Asc)]).unwrap();
    let mut index = OrderedIndex::new(IndexPlanId::from_u128(0x52), schema);
    for value in 0_i64..9 {
        index
            .insert(row(value as u128), token(value as u128), numeric_key(value))
            .unwrap();
    }

    let left = index.range(numeric_key(1)..=numeric_key(6), None).unwrap();
    let right = index.range(numeric_key(4)..=numeric_key(8), None).unwrap();
    let (union, union_metrics) = collect_rows(AccessStream::union(vec![left, right]).unwrap());
    assert_eq!(
        union.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        (1_u128..=8).map(row).collect::<Vec<_>>()
    );
    assert_eq!(union_metrics.full_scans, 0);
    assert_eq!(union_metrics.union_duplicates_skipped, 3);

    let left = index.range(numeric_key(1)..=numeric_key(6), None).unwrap();
    let right = index.range(numeric_key(4)..=numeric_key(8), None).unwrap();
    let (intersection, intersection_metrics) =
        collect_rows(AccessStream::intersection(vec![left, right]).unwrap());
    assert_eq!(
        intersection
            .iter()
            .map(AccessItem::row_id)
            .collect::<Vec<_>>(),
        (4_u128..=6).map(row).collect::<Vec<_>>()
    );
    assert_eq!(intersection_metrics.full_scans, 0);

    let (middle, range_metrics) = collect_rows(
        index
            .component_range(
                &[],
                Bound::Included(&number(3)),
                Bound::Excluded(&number(6)),
                None,
            )
            .unwrap(),
    );
    assert_eq!(
        middle.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        [row(3), row(4), row(5)]
    );
    assert_eq!(range_metrics.index_seeks, 1);
    assert_eq!(range_metrics.full_scans, 0);

    let mut work = WorkTracker::new(WorkLimits::new(4, 4, 4, 2, 4, 8, 1));
    let mut stream = index.ordered_start(None).unwrap();
    assert_eq!(stream.next(&mut work).unwrap().unwrap().row_id(), row(0));
    assert_eq!(stream.next(&mut work).unwrap().unwrap().row_id(), row(1));
    assert!(matches!(
        stream.next(&mut work),
        Err(AccessError::WorkLimitExceeded(WorkLimitExceeded {
            kind: LimitKind::CandidatesVisited,
            limit: 2,
        }))
    ));
    assert_eq!(work.metrics().work_limit_failures, 1);
}
