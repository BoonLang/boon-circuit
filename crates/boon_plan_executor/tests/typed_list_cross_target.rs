#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

use boon_list_access::{
    ClosedTag, Direction, IndexPlanId, KEY_CODEC_VERSION, KeyComponent, KeyKind, KeySchema,
    MutationOutcome, OrderedIndex, RowId as AccessRowId, SourceOrderToken, StructuralKey,
    StructuralValue, TagTypeId, WorkLimits, WorkTracker,
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn typed_key_codec_has_the_same_golden_bytes_on_native_and_wasm() {
    let tag_type = TagTypeId::from_u128(0x0102_0304_0506_0708_1112_1314_1516_1718);
    let truth_type = TagTypeId::from_u128(0x2122_2324_2526_2728_3132_3334_3536_3738);
    let schema = KeySchema::new(vec![
        KeyComponent::new(KeyKind::Number, Direction::Asc),
        KeyComponent::new(KeyKind::Text, Direction::Asc),
        KeyComponent::new(KeyKind::ClosedTag(truth_type), Direction::Asc),
        KeyComponent::new(KeyKind::ClosedTag(tag_type), Direction::Asc),
    ])
    .unwrap();
    let key = StructuralKey::new(vec![
        StructuralValue::number(boon_data::ExactNumber::zero()),
        StructuralValue::text("a\0"),
        StructuralValue::ClosedTag(ClosedTag::new(truth_type, 1)),
        StructuralValue::ClosedTag(ClosedTag::new(tag_type, 7)),
    ])
    .unwrap();
    let mut expected = vec![KEY_CODEC_VERSION, 0x11, 0x01, 0x22, b'a', 0, u8::MAX, 0, 0];
    expected.push(0x44);
    expected.extend_from_slice(truth_type.as_bytes());
    expected.extend_from_slice(&1_u32.to_be_bytes());
    expected.push(0x44);
    expected.extend_from_slice(tag_type.as_bytes());
    expected.extend_from_slice(&7_u32.to_be_bytes());
    assert_eq!(schema.encode(&key).unwrap().as_bytes(), expected);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn sixty_thousand_row_kernel_seek_and_mutation_match_native_and_wasm() {
    const ROW_COUNT: u64 = 60_000;
    let schema = KeySchema::new(vec![KeyComponent::new(KeyKind::Text, Direction::Asc)]).unwrap();
    let mut index = OrderedIndex::new(IndexPlanId::from_u128(0x6000), schema);
    for value in 0..ROW_COUNT {
        index
            .insert(
                AccessRowId::from_u128(u128::from(value)),
                SourceOrderToken::from_u128(u128::from(value) + 1),
                StructuralKey::new(vec![StructuralValue::text(format!("station-{value:05}"))])
                    .unwrap(),
            )
            .unwrap();
    }
    let integrity = index.validate_integrity().unwrap();
    assert_eq!(integrity.logical_rows, ROW_COUNT);
    assert_eq!(integrity.index_entries, ROW_COUNT);

    let mut first_work = WorkTracker::new(WorkLimits::new(4, 4, 64, 64, 64, 64, 0));
    let mut first = index.text_prefix(&[], "station-59", None).unwrap();
    let first_page = (0..20)
        .map(|_| first.next(&mut first_work).unwrap().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(first_page[0].row_id(), AccessRowId::from_u128(59_000));
    assert_eq!(first_page[19].row_id(), AccessRowId::from_u128(59_019));
    assert_eq!(first_work.metrics().candidates_visited, 20);
    assert_eq!(first_work.metrics().full_scans, 0);
    let cursor = first_page.last().unwrap().cursor_key();
    drop(first);

    let mut deep_work = WorkTracker::new(WorkLimits::new(4, 4, 64, 64, 64, 64, 0));
    let mut deep = index.text_prefix(&[], "station-59", Some(&cursor)).unwrap();
    let deep_page = (0..20)
        .map(|_| deep.next(&mut deep_work).unwrap().unwrap().row_id())
        .collect::<Vec<_>>();
    assert_eq!(
        deep_page,
        (59_020_u128..59_040)
            .map(AccessRowId::from_u128)
            .collect::<Vec<_>>()
    );
    assert_eq!(deep_work.metrics().cursor_seeks, 1);
    assert_eq!(deep_work.metrics().candidates_visited, 20);
    assert_eq!(deep_work.metrics().full_scans, 0);
    drop(deep);

    let changed = AccessRowId::from_u128(58_500);
    assert_eq!(
        index
            .update(
                changed,
                SourceOrderToken::from_u128(58_501),
                StructuralKey::new(vec![StructuralValue::text("bergen-stasjon")]).unwrap(),
            )
            .unwrap(),
        MutationOutcome::Updated
    );
    let mut exact_work = WorkTracker::new(WorkLimits::default());
    let exact_key = StructuralKey::new(vec![StructuralValue::text("bergen-stasjon")]).unwrap();
    let mut exact = index.exact(&exact_key, None).unwrap();
    assert_eq!(
        exact.next(&mut exact_work).unwrap().unwrap().row_id(),
        changed
    );
    assert!(exact.next(&mut exact_work).unwrap().is_none());
    assert_eq!(exact_work.metrics().candidates_visited, 1);
    assert_eq!(exact_work.metrics().full_scans, 0);
}
