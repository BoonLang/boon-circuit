use crate::*;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ops::Bound;
use std::sync::Arc;

const TAG_TYPE: TagTypeId = TagTypeId::from_u128(0xabc);
const PLAN_ID: IndexPlanId = IndexPlanId::from_u128(0x123);

fn component(kind: KeyKind, direction: Direction) -> KeyComponent {
    KeyComponent::new(kind, direction)
}

fn schema(components: &[(KeyKind, Direction)]) -> KeySchema {
    KeySchema::new(
        components
            .iter()
            .map(|(kind, direction)| component(*kind, *direction))
            .collect(),
    )
    .unwrap()
}

fn key(parts: Vec<StructuralValue>) -> StructuralKey {
    StructuralKey::new(parts).unwrap()
}

fn number(value: i64) -> StructuralValue {
    StructuralValue::Number(FiniteNumber::new(value as f64).unwrap())
}

fn text(value: &str) -> StructuralValue {
    StructuralValue::text(value)
}

fn tag(ordinal: u32) -> StructuralValue {
    StructuralValue::ClosedTag(ClosedTag::new(TAG_TYPE, ordinal))
}

fn row(value: u128) -> RowId {
    RowId::from_u128(value)
}

fn token(value: u128) -> SourceOrderToken {
    SourceOrderToken::from_u128(value)
}

fn ordered_index(schema: KeySchema) -> OrderedIndex {
    OrderedIndex::new(PLAN_ID, schema)
}

fn collect(mut stream: AccessStream<'_>) -> (Vec<AccessItem>, AccessMetrics) {
    let mut work = WorkTracker::new(WorkLimits::default());
    let mut output = Vec::new();
    while let Some(item) = stream.next(&mut work).unwrap() {
        output.push(item);
    }
    (output, work.metrics())
}

fn value_cmp(left: &StructuralValue, right: &StructuralValue) -> Ordering {
    match (left, right) {
        (StructuralValue::Number(left), StructuralValue::Number(right)) => left.cmp(right),
        (StructuralValue::Text(left), StructuralValue::Text(right)) => left.cmp(right),
        (StructuralValue::Bool(left), StructuralValue::Bool(right)) => left.cmp(right),
        (StructuralValue::ClosedTag(left), StructuralValue::ClosedTag(right)) => left.cmp(right),
        _ => panic!("reference comparator received unlike typed components"),
    }
}

fn directed_key_cmp(schema: &KeySchema, left: &StructuralKey, right: &StructuralKey) -> Ordering {
    for ((left, right), specification) in left
        .parts()
        .iter()
        .zip(right.parts())
        .zip(schema.components())
    {
        let ordering = value_cmp(left, right);
        let ordering = match specification.direction() {
            Direction::Asc => ordering,
            Direction::Desc => ordering.reverse(),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn reference_rows(
    schema: &KeySchema,
    rows: &BTreeMap<RowId, (SourceOrderToken, StructuralKey)>,
) -> Vec<(RowId, SourceOrderToken, StructuralKey)> {
    let mut output = rows
        .iter()
        .map(|(row_id, (source_order, key))| (*row_id, *source_order, key.clone()))
        .collect::<Vec<_>>();
    output.sort_by(|left, right| {
        directed_key_cmp(schema, &left.2, &right.2)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.0.cmp(&right.0))
    });
    output
}

#[test]
fn finite_numbers_are_canonical_and_strictly_finite() {
    assert_eq!(
        FiniteNumber::new(-0.0).unwrap(),
        FiniteNumber::new(0.0).unwrap()
    );
    assert!(matches!(
        FiniteNumber::new(f64::NAN),
        Err(KeyError::NonFiniteNumber)
    ));
    assert!(matches!(
        FiniteNumber::new(f64::INFINITY),
        Err(KeyError::NonFiniteNumber)
    ));
    assert!(matches!(
        FiniteNumber::new(f64::NEG_INFINITY),
        Err(KeyError::NonFiniteNumber)
    ));

    let ascending = schema(&[(KeyKind::Number, Direction::Asc)]);
    let descending = schema(&[(KeyKind::Number, Direction::Desc)]);
    let values = [
        -f64::MAX,
        -100.5,
        -1.0,
        -f64::MIN_POSITIVE,
        0.0,
        f64::MIN_POSITIVE,
        1.0,
        100.5,
        f64::MAX,
    ];
    for left in values {
        for right in values {
            let left_key = key(vec![StructuralValue::number(left).unwrap()]);
            let right_key = key(vec![StructuralValue::number(right).unwrap()]);
            assert_eq!(
                ascending
                    .encode(&left_key)
                    .unwrap()
                    .cmp(&ascending.encode(&right_key).unwrap()),
                left.total_cmp(&right)
            );
            assert_eq!(
                descending
                    .encode(&left_key)
                    .unwrap()
                    .cmp(&descending.encode(&right_key).unwrap()),
                left.total_cmp(&right).reverse()
            );
        }
    }
}

#[test]
fn codec_has_a_stable_typed_byte_layout() {
    let schema = schema(&[
        (KeyKind::Number, Direction::Asc),
        (KeyKind::Text, Direction::Asc),
        (KeyKind::Bool, Direction::Asc),
        (KeyKind::ClosedTag(TAG_TYPE), Direction::Asc),
    ]);
    let encoded = schema
        .encode(&key(vec![
            number(0),
            text("a\0"),
            StructuralValue::Bool(true),
            tag(7),
        ]))
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
    expected.extend_from_slice(TAG_TYPE.as_bytes());
    expected.extend_from_slice(&7_u32.to_be_bytes());
    assert_eq!(encoded.as_bytes(), expected);
}

#[test]
fn mixed_direction_codec_matches_independent_structural_order() {
    let texts = ["", "a", "a\0", "aa", "z", "é"];
    let numbers = [-2, 0, 3];
    let mut keys = Vec::new();
    for text_value in texts {
        for number_value in numbers {
            for bool_value in [false, true] {
                for tag_value in 0..3 {
                    keys.push(key(vec![
                        text(text_value),
                        number(number_value),
                        StructuralValue::Bool(bool_value),
                        tag(tag_value),
                    ]));
                }
            }
        }
    }
    for direction_mask in 0_u8..16 {
        let direction = |component: u32| {
            if direction_mask & (1_u8 << component) == 0 {
                Direction::Asc
            } else {
                Direction::Desc
            }
        };
        let schema = schema(&[
            (KeyKind::Text, direction(0)),
            (KeyKind::Number, direction(1)),
            (KeyKind::Bool, direction(2)),
            (KeyKind::ClosedTag(TAG_TYPE), direction(3)),
        ]);
        for left in &keys {
            for right in &keys {
                assert_eq!(
                    schema
                        .encode(left)
                        .unwrap()
                        .cmp(&schema.encode(right).unwrap()),
                    directed_key_cmp(&schema, left, right),
                    "direction mask {direction_mask:04b}"
                );
            }
        }
    }
}

#[test]
fn schema_rejects_wrong_kinds_and_closed_tag_types() {
    let expected_tag = TagTypeId::from_u128(1);
    let other_tag = TagTypeId::from_u128(2);
    let schema = schema(&[(KeyKind::ClosedTag(expected_tag), Direction::Asc)]);
    let error = schema
        .encode(&key(vec![StructuralValue::ClosedTag(ClosedTag::new(
            other_tag, 0,
        ))]))
        .unwrap_err();
    assert!(matches!(
        error,
        KeyError::WrongKeyKind {
            component: 0,
            expected: KeyKind::ClosedTag(value),
            actual: KeyKind::ClosedTag(other),
        } if value == expected_tag && other == other_tag
    ));
}

#[test]
fn equal_keys_preserve_source_order_then_row_identity() {
    let schema = schema(&[(KeyKind::Text, Direction::Asc)]);
    let mut index = ordered_index(schema);
    let common = key(vec![text("same")]);
    for (row_id, source_order) in [(9, 20), (2, 10), (1, 10), (7, 30)] {
        index
            .insert(row(row_id), token(source_order), common.clone())
            .unwrap();
    }
    let (items, metrics) = collect(index.exact(&common, None).unwrap());
    assert_eq!(
        items.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        vec![row(1), row(2), row(9), row(7)]
    );
    assert_eq!(metrics.index_seeks, 1);
    assert_eq!(metrics.keys_visited, 1);
    assert_eq!(metrics.candidates_visited, 4);
    assert_eq!(metrics.full_scans, 0);
}

#[test]
fn source_order_only_index_resumes_from_a_direct_cursor_without_scanning() {
    let mut index = ordered_index(schema(&[]));
    let empty = key(Vec::new());
    for (row_id, source_order) in [(7, 30), (2, 10), (9, 40), (3, 20)] {
        index
            .insert(row(row_id), token(source_order), empty.clone())
            .unwrap();
    }

    let mut first_work = WorkTracker::new(WorkLimits::default());
    let mut first = index.ordered_start(None).unwrap();
    let first_item = first.next(&mut first_work).unwrap().unwrap();
    let second_item = first.next(&mut first_work).unwrap().unwrap();
    assert_eq!(
        [first_item.row_id(), second_item.row_id()],
        [row(2), row(3)]
    );

    let mut next_work = WorkTracker::new(WorkLimits::default());
    let mut next = index
        .ordered_start(Some(&second_item.cursor_key()))
        .unwrap();
    let remaining = [
        next.next(&mut next_work).unwrap().unwrap().row_id(),
        next.next(&mut next_work).unwrap().unwrap().row_id(),
    ];
    assert_eq!(remaining, [row(7), row(9)]);
    assert!(next.next(&mut next_work).unwrap().is_none());
    let metrics = next_work.metrics();
    assert_eq!(metrics.index_seeks, 1);
    assert_eq!(metrics.cursor_seeks, 1);
    assert_eq!(metrics.full_scans, 0);
    assert_eq!(metrics.candidates_visited, 2);
}

#[test]
fn exact_range_and_descending_text_prefix_are_seekable() {
    let schema = schema(&[
        (KeyKind::Bool, Direction::Asc),
        (KeyKind::Text, Direction::Desc),
    ]);
    let mut index = ordered_index(schema.clone());
    let names = ["alpha", "alpine", "al\0pha", "alto", "beta", "álpha"];
    for (position, name) in names.iter().enumerate() {
        index
            .insert(
                row(position as u128),
                token(position as u128),
                key(vec![StructuralValue::Bool(true), text(name)]),
            )
            .unwrap();
    }

    let (prefix, prefix_metrics) = collect(
        index
            .text_prefix(&[StructuralValue::Bool(true)], "al", None)
            .unwrap(),
    );
    let prefix_names = prefix
        .iter()
        .map(|item| match &item.key().parts()[1] {
            StructuralValue::Text(value) => value.as_str(),
            _ => unreachable!(),
        })
        .collect::<Vec<_>>();
    assert_eq!(prefix_names, vec!["alto", "alpine", "alpha", "al\0pha"]);
    assert_eq!(prefix_metrics.keys_visited, 4);
    for (position, name) in names.iter().enumerate() {
        let key = index.cursor_for(row(position as u128)).unwrap();
        assert_eq!(
            index
                .key_matches_text_prefix(key.key(), &[StructuralValue::Bool(true)], "al",)
                .unwrap(),
            name.starts_with("al")
        );
    }

    let mut ordered_keys = names
        .iter()
        .map(|name| key(vec![StructuralValue::Bool(true), text(name)]))
        .collect::<Vec<_>>();
    ordered_keys.sort_by(|left, right| directed_key_cmp(&schema, left, right));
    let (middle, _) = collect(
        index
            .range(
                (
                    Bound::Included(ordered_keys[1].clone()),
                    Bound::Excluded(ordered_keys[4].clone()),
                ),
                None,
            )
            .unwrap(),
    );
    assert_eq!(
        middle
            .iter()
            .map(|item| item.key().clone())
            .collect::<Vec<_>>(),
        ordered_keys[1..4]
    );
}

#[test]
fn composite_prefix_and_component_range_preserve_directed_trailing_order() {
    let mut index = ordered_index(schema(&[
        (KeyKind::Text, Direction::Asc),
        (KeyKind::Number, Direction::Desc),
        (KeyKind::Text, Direction::Asc),
    ]));
    for (row_id, source_order, group, score, name) in [
        (1, 20, "A", 8, "zulu"),
        (2, 30, "A", 10, "bravo"),
        (3, 10, "A", 8, "alpha"),
        (4, 40, "A", 5, "charlie"),
        (5, 50, "B", 9, "other"),
    ] {
        index
            .insert(
                row(row_id),
                token(source_order),
                key(vec![text(group), number(score), text(name)]),
            )
            .unwrap();
    }

    let (prefixed, prefix_metrics) = collect(index.key_prefix(&[text("A")], None).unwrap());
    assert_eq!(
        prefixed.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        [row(2), row(3), row(1), row(4)]
    );
    assert_eq!(prefix_metrics.index_seeks, 1);
    assert_eq!(prefix_metrics.full_scans, 0);
    assert!(
        index
            .key_matches_prefix(index.cursor_for(row(1)).unwrap().key(), &[text("A")])
            .unwrap()
    );
    assert!(
        !index
            .key_matches_prefix(index.cursor_for(row(5)).unwrap().key(), &[text("A")])
            .unwrap()
    );

    let lower = number(10);
    let upper = number(8);
    let (ranged, range_metrics) = collect(
        index
            .component_range(
                &[text("A")],
                Bound::Included(&lower),
                Bound::Included(&upper),
                None,
            )
            .unwrap(),
    );
    assert_eq!(
        ranged.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        [row(2), row(3), row(1)]
    );
    assert_eq!(range_metrics.index_seeks, 1);
    assert_eq!(range_metrics.full_scans, 0);
    for (row_id, expected) in [(1, true), (2, true), (3, true), (4, false), (5, false)] {
        assert_eq!(
            index
                .key_matches_component_range(
                    index.cursor_for(row(row_id)).unwrap().key(),
                    &[text("A")],
                    Bound::Included(&lower),
                    Bound::Included(&upper),
                )
                .unwrap(),
            expected
        );
    }

    let cursor = ranged[0].cursor_key();
    let (resumed, resumed_metrics) = collect(
        index
            .component_range(
                &[text("A")],
                Bound::Included(&lower),
                Bound::Included(&upper),
                Some(&cursor),
            )
            .unwrap(),
    );
    assert_eq!(
        resumed.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        [row(3), row(1)]
    );
    assert_eq!(resumed_metrics.cursor_seeks, 1);

    let (exclusive, _) = collect(
        index
            .component_range(
                &[text("A")],
                Bound::Included(&lower),
                Bound::Excluded(&upper),
                None,
            )
            .unwrap(),
    );
    assert_eq!(
        exclusive.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        [row(2)]
    );
}

#[test]
fn direct_cursor_seek_resumes_inside_a_duplicate_key_group() {
    let schema = schema(&[(KeyKind::Number, Direction::Asc)]);
    let mut index = ordered_index(schema);
    let common = key(vec![number(5)]);
    for value in (0_u128..100).rev() {
        index
            .insert(row(value), token(value / 2), common.clone())
            .unwrap();
    }

    let mut first_stream = index.exact(&common, None).unwrap();
    let mut first_work = WorkTracker::new(WorkLimits::default());
    let mut first = Vec::new();
    for _ in 0..37 {
        first.push(first_stream.next(&mut first_work).unwrap().unwrap());
    }
    let cursor = first.last().unwrap().cursor_key();
    drop(first_stream);

    let (remaining, metrics) = collect(index.exact(&common, Some(&cursor)).unwrap());
    assert_eq!(remaining.len(), 63);
    assert_eq!(metrics.cursor_seeks, 1);
    assert_eq!(metrics.candidates_visited, 63);
    assert!(first.iter().all(|left| {
        remaining
            .iter()
            .all(|right| left.row_id() != right.row_id())
    }));

    let mut combined = first
        .iter()
        .chain(&remaining)
        .map(AccessItem::row_id)
        .collect::<Vec<_>>();
    combined.sort();
    assert_eq!(combined, (0_u128..100).map(row).collect::<Vec<_>>());
}

#[test]
fn mutations_are_incremental_atomic_and_integrity_checked() {
    let schema = schema(&[(KeyKind::Number, Direction::Asc)]);
    let mut index = ordered_index(schema);
    assert_eq!(
        index
            .insert(row(1), token(10), key(vec![number(1)]))
            .unwrap(),
        MutationOutcome::Inserted
    );
    assert!(matches!(
        index.insert(row(1), token(11), key(vec![number(2)])),
        Err(AccessError::DuplicateRow(value)) if value == row(1)
    ));

    let invalid = key(vec![text("not a number")]);
    assert!(matches!(
        index.update(row(1), token(20), invalid),
        Err(AccessError::Key(KeyError::WrongKeyKind { .. }))
    ));
    assert_eq!(
        index.cursor_for(row(1)).unwrap().key(),
        &key(vec![number(1)])
    );

    assert_eq!(
        index
            .update(row(1), token(20), key(vec![number(3)]))
            .unwrap(),
        MutationOutcome::Updated
    );
    assert_eq!(
        index
            .update(row(1), token(20), key(vec![number(3)]))
            .unwrap(),
        MutationOutcome::Unchanged
    );
    assert!(matches!(
        index.update(row(2), token(20), key(vec![number(3)])),
        Err(AccessError::UnknownRow(value)) if value == row(2)
    ));
    assert_eq!(index.remove(row(9)).unwrap(), MutationOutcome::NotFound);
    assert_eq!(index.remove(row(1)).unwrap(), MutationOutcome::Removed);
    assert!(index.is_empty());

    let report = index.validate_integrity().unwrap();
    assert_eq!(report.logical_rows, 0);
    let metrics = index.metrics();
    assert_eq!(metrics.insertions, 1);
    assert_eq!(metrics.updates, 1);
    assert_eq!(metrics.unchanged_updates, 1);
    assert_eq!(metrics.removals, 1);
    assert_eq!(metrics.physical_entry_updates, 4);
}

#[test]
fn lazy_union_merges_and_deduplicates_without_materializing_results() {
    let schema = schema(&[(KeyKind::Number, Direction::Asc)]);
    let mut index = ordered_index(schema);
    let keys = (0_i64..8)
        .map(|value| key(vec![number(value)]))
        .collect::<Vec<_>>();
    for (position, key) in keys.iter().enumerate() {
        index
            .insert(row(position as u128), token(position as u128), key.clone())
            .unwrap();
    }
    let left = index
        .range(keys[1].clone()..=keys[5].clone(), None)
        .unwrap();
    let right = index
        .range(keys[3].clone()..=keys[7].clone(), None)
        .unwrap();
    let (items, metrics) = collect(AccessStream::union(vec![left, right]).unwrap());
    assert_eq!(
        items.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        (1_u128..=7).map(row).collect::<Vec<_>>()
    );
    assert_eq!(metrics.candidates_visited, 10);
    assert_eq!(metrics.rows_returned, 7);
    assert_eq!(metrics.union_duplicates_skipped, 3);
    assert!(metrics.branch_polls <= 12);
}

#[test]
fn lazy_intersection_advances_sorted_heads_only() {
    let schema = schema(&[(KeyKind::Number, Direction::Asc)]);
    let mut index = ordered_index(schema);
    let keys = (0_i64..9)
        .map(|value| key(vec![number(value)]))
        .collect::<Vec<_>>();
    for (position, key) in keys.iter().enumerate() {
        index
            .insert(row(position as u128), token(position as u128), key.clone())
            .unwrap();
    }
    let left = index
        .range(keys[1].clone()..=keys[6].clone(), None)
        .unwrap();
    let right = index
        .range(keys[4].clone()..=keys[8].clone(), None)
        .unwrap();
    let (items, metrics) = collect(AccessStream::intersection(vec![left, right]).unwrap());
    assert_eq!(
        items.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        (4_u128..=6).map(row).collect::<Vec<_>>()
    );
    assert_eq!(metrics.rows_returned, 3);
    assert!(metrics.intersection_candidates_skipped >= 3);
    assert!(metrics.candidates_visited < 12);
}

#[test]
fn composite_streams_require_the_same_typed_order() {
    let ascending = ordered_index(schema(&[(KeyKind::Number, Direction::Asc)]));
    let descending = ordered_index(schema(&[(KeyKind::Number, Direction::Desc)]));
    let error = AccessStream::union(vec![
        ascending.scan(None).unwrap(),
        descending.scan(None).unwrap(),
    ])
    .unwrap_err();
    assert!(matches!(
        error,
        AccessError::IncompatibleStreamSchemas { operation: "union" }
    ));

    let first = OrderedIndex::new(
        IndexPlanId::from_u128(1),
        schema(&[(KeyKind::Number, Direction::Asc)]),
    );
    let second = OrderedIndex::new(
        IndexPlanId::from_u128(2),
        schema(&[(KeyKind::Number, Direction::Asc)]),
    );
    assert!(matches!(
        AccessStream::intersection(vec![first.scan(None).unwrap(), second.scan(None).unwrap()]),
        Err(AccessError::IncompatibleStreamSchemas {
            operation: "intersection"
        })
    ));
}

#[test]
fn work_limits_fail_explicitly_without_partial_fallback() {
    let schema = schema(&[(KeyKind::Number, Direction::Asc)]);
    let mut index = ordered_index(schema);
    for value in 0_u128..6 {
        index
            .insert(row(value), token(value), key(vec![number(value as i64)]))
            .unwrap();
    }
    let limits = WorkLimits::new(10, 10, 10, 3, 10, 20, 1);
    let mut work = WorkTracker::new(limits);
    let mut stream = index.scan(None).unwrap();
    for expected in 0_u128..3 {
        assert_eq!(
            stream.next(&mut work).unwrap().unwrap().row_id(),
            row(expected)
        );
    }
    assert!(matches!(
        stream.next(&mut work),
        Err(AccessError::WorkLimitExceeded(WorkLimitExceeded {
            kind: LimitKind::CandidatesVisited,
            limit: 3,
        }))
    ));
    assert!(stream.next(&mut work).unwrap().is_none());
    assert_eq!(work.metrics().candidates_visited, 3);
    assert_eq!(work.metrics().work_limit_failures, 1);

    let mut forbidden_scan = WorkTracker::new(WorkLimits::new(10, 10, 10, 10, 10, 10, 0));
    let mut stream = index.scan(None).unwrap();
    assert!(matches!(
        stream.next(&mut forbidden_scan),
        Err(AccessError::WorkLimitExceeded(WorkLimitExceeded {
            kind: LimitKind::FullScans,
            limit: 0,
        }))
    ));
    assert_eq!(forbidden_scan.metrics().full_scans, 0);

    let exact_key = key(vec![number(2)]);
    let mut exact = index.exact(&exact_key, None).unwrap();
    let mut no_scans = WorkTracker::new(WorkLimits::new(10, 10, 10, 10, 10, 10, 0));
    assert_eq!(exact.next(&mut no_scans).unwrap().unwrap().row_id(), row(2));
    assert_eq!(no_scans.metrics().full_scans, 0);
}

#[test]
fn every_directed_range_boundary_matches_reference_slices() {
    for direction in [Direction::Asc, Direction::Desc] {
        let schema = schema(&[(KeyKind::Number, direction)]);
        let mut index = ordered_index(schema.clone());
        let mut keys = (-5_i64..=5)
            .map(|value| key(vec![number(value)]))
            .collect::<Vec<_>>();
        for (position, key) in keys.iter().enumerate() {
            index
                .insert(row(position as u128), token(position as u128), key.clone())
                .unwrap();
        }
        keys.sort_by(|left, right| directed_key_cmp(&schema, left, right));
        for start in 0..keys.len() {
            for end in start..=keys.len() {
                let upper = if end == keys.len() {
                    Bound::Unbounded
                } else {
                    Bound::Excluded(keys[end].clone())
                };
                let (items, _) = collect(
                    index
                        .range((Bound::Included(keys[start].clone()), upper), None)
                        .unwrap(),
                );
                assert_eq!(
                    items
                        .iter()
                        .map(|item| item.key().clone())
                        .collect::<Vec<_>>(),
                    keys[start..end]
                );
            }
        }
    }
}

#[test]
fn rebuild_and_integrity_report_count_only_index_metadata() {
    let schema = schema(&[(KeyKind::Text, Direction::Asc)]);
    let index = OrderedIndex::rebuild(
        PLAN_ID,
        schema,
        [
            (row(1), token(10), key(vec![text("one")])),
            (row(2), token(20), key(vec![text("two")])),
        ],
    )
    .unwrap();
    let report = index.validate_integrity().unwrap();
    assert_eq!(report.logical_rows, 2);
    assert_eq!(report.index_entries, 2);
    assert!(report.encoded_key_bytes > 0);
    assert_eq!(report.structural_key_bytes, 6);
    assert_eq!(report.source_order_bytes, 32);
    assert_eq!(report.row_identity_bytes, 32);
    assert_eq!(
        report.payload_bytes(),
        report.encoded_key_bytes
            + report.structural_key_bytes
            + report.source_order_bytes
            + report.row_identity_bytes
    );
    assert_eq!(index.metrics().rebuilds, 1);
}

#[test]
fn one_logical_row_can_own_bounded_deduplicated_index_keys() {
    let schema = schema(&[
        (KeyKind::Text, Direction::Asc),
        (KeyKind::Number, Direction::Asc),
    ]);
    let mut index = ordered_index(schema);
    index
        .insert_many(
            row(1),
            token(10),
            [
                key(vec![text("alpha"), number(2)]),
                key(vec![text("beta"), number(2)]),
                key(vec![text("alpha"), number(2)]),
            ],
        )
        .unwrap();
    index.insert_many(row(2), token(20), []).unwrap();
    index
        .insert_many(row(3), token(30), [key(vec![text("beta"), number(1)])])
        .unwrap();

    let report = index.validate_integrity().unwrap();
    assert_eq!(report.logical_rows, 3);
    assert_eq!(report.index_entries, 3);
    assert_eq!(index.metrics().logical_rows, 3);
    assert_eq!(index.metrics().index_entries, 3);
    assert!(index.cursor_for(row(1)).is_none());
    assert_eq!(index.cursor_keys_for(row(1)).len(), 2);
    assert!(index.cursor_keys_for(row(2)).is_empty());

    let (alpha, _) = collect(index.key_prefix(&[text("alpha")], None).unwrap());
    assert_eq!(
        alpha.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        [row(1)]
    );
    let (beta, _) = collect(index.key_prefix(&[text("beta")], None).unwrap());
    assert_eq!(
        beta.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        [row(3), row(1)]
    );

    assert_eq!(
        index
            .update_many(row(1), token(10), [key(vec![text("beta"), number(4)])],)
            .unwrap(),
        MutationOutcome::Updated
    );
    assert_eq!(index.remove(row(2)).unwrap(), MutationOutcome::Removed);
    let report = index.validate_integrity().unwrap();
    assert_eq!(report.logical_rows, 2);
    assert_eq!(report.index_entries, 2);
    let (alpha, _) = collect(index.key_prefix(&[text("alpha")], None).unwrap());
    assert!(alpha.is_empty());
}

#[test]
fn projected_token_streams_union_and_intersect_in_semantic_order() {
    let schema = schema(&[
        (KeyKind::Text, Direction::Asc),
        (KeyKind::Number, Direction::Asc),
    ]);
    let mut index = ordered_index(schema);
    for (row_id, source_order, rank, tokens) in [
        (row(1), token(10), 2, [Some("a"), Some("b")]),
        (row(2), token(20), 1, [Some("a"), None]),
        (row(3), token(30), 1, [Some("b"), None]),
        (row(4), token(40), 3, [Some("a"), Some("b")]),
    ] {
        index
            .insert_many(
                row_id,
                source_order,
                tokens
                    .into_iter()
                    .flatten()
                    .map(|token| key(vec![text(token), number(rank)])),
            )
            .unwrap();
    }

    let projected = |token_value: &str| {
        index
            .key_prefix(&[text(token_value)], None)
            .unwrap()
            .project_key_prefix(1)
            .unwrap()
    };
    let (union, _) = collect(AccessStream::union(vec![projected("a"), projected("b")]).unwrap());
    assert_eq!(
        union.iter().map(AccessItem::row_id).collect::<Vec<_>>(),
        [row(2), row(3), row(1), row(4)]
    );
    assert!(union.iter().all(|item| item.key().parts().len() == 1));

    let (intersection, _) =
        collect(AccessStream::intersection(vec![projected("a"), projected("b")]).unwrap());
    assert_eq!(
        intersection
            .iter()
            .map(AccessItem::row_id)
            .collect::<Vec<_>>(),
        [row(1), row(4)]
    );
}

#[test]
fn owning_integrity_task_matches_synchronous_validation_and_returns_the_index() {
    let schema = schema(&[
        (KeyKind::Text, Direction::Asc),
        (KeyKind::Number, Direction::Desc),
    ]);
    let mut index = ordered_index(schema);
    for value in 0_u128..5 {
        index
            .insert(
                row(value),
                token(value + 10),
                key(vec![text(&format!("row-{value}")), number(value as i64)]),
            )
            .unwrap();
    }
    let expected = index.validate_integrity().unwrap();

    let mut task = index.into_integrity_task();
    let result = loop {
        match task.poll(3).unwrap() {
            OrderedIndexIntegrityPoll::Pending(progress) => {
                assert!(progress.completed_steps <= 20);
            }
            OrderedIndexIntegrityPoll::Ready(result) => break result,
        }
    };

    assert_eq!(result.report(), expected);
    assert_eq!(result.index().len(), 5);
    let (index, report) = result.into_parts();
    assert_eq!(report, expected);
    assert_eq!(index.validate_integrity().unwrap(), expected);
}

#[test]
fn integrity_task_one_step_polls_are_bounded_and_cover_every_phase() {
    let mut index = ordered_index(schema(&[(KeyKind::Number, Direction::Asc)]));
    for value in 0_u128..2 {
        index
            .insert(row(value), token(value), key(vec![number(value as i64)]))
            .unwrap();
    }
    let mut task = index.into_integrity_task();
    assert_eq!(
        task.progress(),
        OrderedIndexIntegrityProgress {
            phase: OrderedIndexIntegrityPhase::Rows,
            completed_steps: 0,
            rows_checked: 0,
            entries_checked: 0,
            accounting_entries_checked: 0,
            accounting_rows_checked: 0,
            total_rows: 2,
            total_entries: 2,
        }
    );
    assert!(matches!(
        task.poll(0),
        Err(AccessError::InvalidIntegrityPollBudget)
    ));

    let mut previous_steps = 0;
    let mut pending_phases = Vec::new();
    let result = loop {
        match task.poll(1).unwrap() {
            OrderedIndexIntegrityPoll::Pending(progress) => {
                assert_eq!(progress.completed_steps, previous_steps + 1);
                previous_steps = progress.completed_steps;
                pending_phases.push(progress.phase);
            }
            OrderedIndexIntegrityPoll::Ready(result) => break result,
        }
    };

    assert_eq!(previous_steps, 8);
    assert!(pending_phases.contains(&OrderedIndexIntegrityPhase::Rows));
    assert!(pending_phases.contains(&OrderedIndexIntegrityPhase::Entries));
    assert!(pending_phases.contains(&OrderedIndexIntegrityPhase::Accounting));
    assert_eq!(task.progress().phase, OrderedIndexIntegrityPhase::Complete);
    assert!(matches!(
        task.poll(1),
        Err(AccessError::CompletedIntegrityTask)
    ));
    assert_eq!(result.report().logical_rows, 2);
}

#[test]
fn dropping_a_pending_integrity_task_cancels_and_drops_its_owned_index() {
    let mut index = ordered_index(schema(&[(KeyKind::Text, Direction::Asc)]));
    index
        .insert(row(1), token(1), key(vec![text("owned")]))
        .unwrap();
    let probe = Arc::new(());
    let weak_probe = Arc::downgrade(&probe);
    index.test_attach_drop_probe(Arc::clone(&probe));
    assert!(index.test_has_drop_probe());
    drop(probe);

    let mut task = index.into_integrity_task();
    assert!(matches!(
        task.poll(1).unwrap(),
        OrderedIndexIntegrityPoll::Pending(_)
    ));
    assert!(weak_probe.upgrade().is_some());
    drop(task);
    assert!(weak_probe.upgrade().is_none());
}

#[test]
fn integrity_task_reports_structural_and_accounting_corruption() {
    let mut missing_entry = ordered_index(schema(&[(KeyKind::Number, Direction::Asc)]));
    missing_entry
        .insert(row(1), token(1), key(vec![number(1)]))
        .unwrap();
    missing_entry.test_remove_first_entry_without_accounting();
    assert!(matches!(
        missing_entry.validate_integrity(),
        Err(AccessError::CorruptIndex(
            "logical row has no corresponding index entry"
        ))
    ));
    let mut task = missing_entry.into_integrity_task();
    assert!(matches!(
        task.poll(1),
        Err(AccessError::CorruptIndex(
            "logical row has no corresponding index entry"
        ))
    ));
    assert!(matches!(
        task.poll(1),
        Err(AccessError::CompletedIntegrityTask)
    ));

    let mut bad_accounting = ordered_index(schema(&[(KeyKind::Text, Direction::Asc)]));
    bad_accounting
        .insert(row(1), token(1), key(vec![text("one")]))
        .unwrap();
    bad_accounting.test_increment_encoded_key_accounting();
    let mut task = bad_accounting.into_integrity_task();
    let error = loop {
        match task.poll(1) {
            Ok(OrderedIndexIntegrityPoll::Pending(_)) => {}
            Ok(OrderedIndexIntegrityPoll::Ready(_)) => panic!("corrupt index validated"),
            Err(error) => break error,
        }
    };
    assert!(matches!(
        error,
        AccessError::CorruptIndex("retained key payload accounting disagrees with index contents")
    ));
    assert_eq!(
        task.progress().phase,
        OrderedIndexIntegrityPhase::Accounting
    );
    assert_eq!(task.progress().completed_steps, 4);
}

#[test]
fn integrity_task_rechecks_entry_key_and_payload_resource_limits() {
    fn populated_index() -> OrderedIndex {
        let mut index = ordered_index(schema(&[(KeyKind::Text, Direction::Asc)]));
        index
            .insert(row(1), token(1), key(vec![text("one")]))
            .unwrap();
        index
    }

    let mut entries = populated_index();
    entries.test_set_resource_limits_unchecked(IndexResourceLimits::new(0, u64::MAX, u64::MAX));
    assert!(matches!(
        entries.validate_integrity(),
        Err(AccessError::ResourceLimitExceeded {
            resource: IndexResource::Entries,
            attempted: 1,
            maximum: 0,
        })
    ));

    let mut encoded_key = populated_index();
    encoded_key.test_set_resource_limits_unchecked(IndexResourceLimits::new(u64::MAX, 1, u64::MAX));
    let mut task = encoded_key.into_integrity_task();
    assert!(matches!(
        task.poll(usize::MAX),
        Err(AccessError::ResourceLimitExceeded {
            resource: IndexResource::EncodedKeyBytes,
            attempted,
            maximum: 1,
        }) if attempted > 1
    ));

    let mut payload = populated_index();
    payload.test_set_resource_limits_unchecked(IndexResourceLimits::new(u64::MAX, u64::MAX, 0));
    let mut task = payload.into_integrity_task();
    assert!(matches!(
        task.poll(usize::MAX),
        Err(AccessError::ResourceLimitExceeded {
            resource: IndexResource::PayloadBytes,
            attempted,
            maximum: 0,
        }) if attempted > 0
    ));
}

#[test]
fn mutation_resource_limits_fail_before_changing_the_index() {
    let schema = schema(&[(KeyKind::Text, Direction::Asc)]);
    let mut index =
        OrderedIndex::new_with_limits(PLAN_ID, schema, IndexResourceLimits::new(2, 16, 90));
    index
        .insert(row(1), token(10), key(vec![text("one")]))
        .unwrap();
    let before = index.validate_integrity().unwrap();

    let error = index
        .update(
            row(1),
            token(10),
            key(vec![text("this-key-is-far-too-long")]),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        AccessError::ResourceLimitExceeded {
            resource: IndexResource::EncodedKeyBytes,
            ..
        }
    ));
    assert_eq!(index.validate_integrity().unwrap(), before);
    let (items, _) = collect(index.exact(&key(vec![text("one")]), None).unwrap());
    assert_eq!(items.len(), 1);

    index
        .insert(row(2), token(20), key(vec![text("two")]))
        .unwrap();
    let error = index
        .insert(row(3), token(30), key(vec![text("three")]))
        .unwrap_err();
    assert!(matches!(
        error,
        AccessError::ResourceLimitExceeded {
            resource: IndexResource::Entries,
            attempted: 3,
            maximum: 2,
        }
    ));
    assert_eq!(index.len(), 2);
}

#[test]
fn sixty_thousand_row_indexes_seek_page_and_mutate_without_scans() {
    const ROW_COUNT: u64 = 60_000;
    let mut names = ordered_index(schema(&[(KeyKind::Text, Direction::Asc)]));
    let mut numbers = OrderedIndex::new(
        IndexPlanId::from_u128(0x124),
        schema(&[(KeyKind::Number, Direction::Asc)]),
    );
    for value in 0..ROW_COUNT {
        let row_id = row(u128::from(value));
        let source_order = token(u128::from(value) + 1);
        names
            .insert(
                row_id,
                source_order,
                key(vec![text(&format!("station-{value:05}"))]),
            )
            .unwrap();
        numbers
            .insert(row_id, source_order, key(vec![number(value as i64)]))
            .unwrap();
    }
    assert_eq!(names.validate_integrity().unwrap().logical_rows, ROW_COUNT);
    assert_eq!(
        numbers.validate_integrity().unwrap().logical_rows,
        ROW_COUNT
    );

    let mut first_work = WorkTracker::new(WorkLimits::new(4, 4, 64, 64, 64, 64, 0));
    let mut first = names.text_prefix(&[], "station-58", None).unwrap();
    let first_page = (0..20)
        .map(|_| first.next(&mut first_work).unwrap().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(first_page[0].row_id(), row(58_000));
    assert_eq!(first_page[19].row_id(), row(58_019));
    assert_eq!(first_work.metrics().candidates_visited, 20);
    assert_eq!(first_work.metrics().full_scans, 0);
    let cursor = first_page.last().unwrap().cursor_key();
    drop(first);

    let mut deep_work = WorkTracker::new(WorkLimits::new(4, 4, 64, 64, 64, 64, 0));
    let mut deep = names.text_prefix(&[], "station-58", Some(&cursor)).unwrap();
    let deep_page = (0..20)
        .map(|_| deep.next(&mut deep_work).unwrap().unwrap().row_id())
        .collect::<Vec<_>>();
    assert_eq!(
        deep_page,
        (58_020_u128..58_040).map(row).collect::<Vec<_>>()
    );
    assert_eq!(deep_work.metrics().cursor_seeks, 1);
    assert_eq!(deep_work.metrics().candidates_visited, 20);
    assert_eq!(deep_work.metrics().full_scans, 0);
    drop(deep);

    let lower = number(58_490);
    let upper = number(58_510);
    let (range, range_metrics) = collect(
        numbers
            .component_range(&[], Bound::Included(&lower), Bound::Included(&upper), None)
            .unwrap(),
    );
    assert_eq!(range.len(), 21);
    assert_eq!(range.first().unwrap().row_id(), row(58_490));
    assert_eq!(range.last().unwrap().row_id(), row(58_510));
    assert_eq!(range_metrics.index_seeks, 1);
    assert_eq!(range_metrics.candidates_visited, 21);
    assert_eq!(range_metrics.full_scans, 0);

    let changed = row(58_500);
    assert_eq!(
        names
            .update(changed, token(58_501), key(vec![text("bergen-stasjon")]),)
            .unwrap(),
        MutationOutcome::Updated
    );
    let (renamed, renamed_metrics) = collect(
        names
            .exact(&key(vec![text("bergen-stasjon")]), None)
            .unwrap(),
    );
    assert_eq!(renamed.len(), 1);
    assert_eq!(renamed[0].row_id(), changed);
    assert_eq!(renamed_metrics.candidates_visited, 1);
    assert_eq!(renamed_metrics.full_scans, 0);
    assert_eq!(names.metrics().updates, 1);
}

#[derive(Clone, Copy)]
struct DeterministicRng(u64);

impl DeterministicRng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
}

fn generated_key(rng: &mut DeterministicRng) -> StructuralKey {
    let texts = ["", "a", "a\0b", "alpha", "z", "é"];
    key(vec![
        text(texts[(rng.next() as usize) % texts.len()]),
        number((rng.next() % 41) as i64 - 20),
        StructuralValue::Bool(rng.next() & 1 == 1),
        tag((rng.next() % 5) as u32),
    ])
}

#[test]
fn deterministic_mutation_trace_matches_reference_after_every_turn() {
    let schema = schema(&[
        (KeyKind::Text, Direction::Asc),
        (KeyKind::Number, Direction::Desc),
        (KeyKind::Bool, Direction::Asc),
        (KeyKind::ClosedTag(TAG_TYPE), Direction::Desc),
    ]);
    let mut index = ordered_index(schema.clone());
    let mut reference = BTreeMap::<RowId, (SourceOrderToken, StructuralKey)>::new();
    let mut rng = DeterministicRng(0x5eed_cafe_f00d_beef);

    for turn in 0..750_u64 {
        let row_id = row((rng.next() % 64) as u128);
        match rng.next() % 3 {
            0 if !reference.contains_key(&row_id) => {
                let source_order = token((rng.next() % 16) as u128);
                let key = generated_key(&mut rng);
                index.insert(row_id, source_order, key.clone()).unwrap();
                reference.insert(row_id, (source_order, key));
            }
            1 if reference.contains_key(&row_id) => {
                let source_order = token((rng.next() % 16) as u128);
                let key = generated_key(&mut rng);
                index.update(row_id, source_order, key.clone()).unwrap();
                reference.insert(row_id, (source_order, key));
            }
            _ => {
                let expected = if reference.remove(&row_id).is_some() {
                    MutationOutcome::Removed
                } else {
                    MutationOutcome::NotFound
                };
                assert_eq!(index.remove(row_id).unwrap(), expected);
            }
        }

        if turn % 7 == 0 || turn == 749 {
            let (actual, _) = collect(index.scan(None).unwrap());
            let expected = reference_rows(&schema, &reference);
            assert_eq!(
                actual
                    .iter()
                    .map(|item| (item.row_id(), item.source_order(), item.key().clone()))
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(
                index.validate_integrity().unwrap().logical_rows,
                expected.len() as u64
            );
        }
    }
}
