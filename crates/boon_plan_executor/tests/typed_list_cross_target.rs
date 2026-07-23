#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

use boon_list_access::{
    ClosedTag, Direction, IndexPlanId, KEY_CODEC_VERSION, KeyComponent, KeyKind, KeySchema,
    MutationOutcome, OrderedIndex, RowId as AccessRowId, SourceOrderToken, StructuralKey,
    StructuralValue, TagTypeId, WorkLimits, WorkTracker,
};
use boon_plan::TargetProfile;
use boon_plan_executor::{
    CursorScopeFingerprint, CursorSealingKey, MachineInstance, SessionOptions, SourceEvent,
    SourcePayload, Value,
};
use std::collections::BTreeMap;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn typed_key_codec_has_the_same_golden_bytes_on_native_and_wasm() {
    let tag_type = TagTypeId::from_u128(0x0102_0304_0506_0708_1112_1314_1516_1718);
    let schema = KeySchema::new(vec![
        KeyComponent::new(KeyKind::Number, Direction::Asc),
        KeyComponent::new(KeyKind::Text, Direction::Asc),
        KeyComponent::new(KeyKind::Bool, Direction::Asc),
        KeyComponent::new(KeyKind::ClosedTag(tag_type), Direction::Asc),
    ])
    .unwrap();
    let key = StructuralKey::new(vec![
        StructuralValue::number(0.0).unwrap(),
        StructuralValue::text("a\0"),
        StructuralValue::Bool(true),
        StructuralValue::ClosedTag(ClosedTag::new(tag_type, 7)),
    ])
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

fn page_parts(value: Value) -> (Vec<Value>, Value) {
    let Value::Record(mut fields) = value else {
        panic!("page must be a closed tagged record")
    };
    assert_eq!(fields.remove("$tag"), Some(Value::Text("Page".to_owned())));
    let Value::List(items) = fields.remove("items").expect("page items") else {
        panic!("page items must stay typed as a list")
    };
    let next = fields.remove("next").expect("page continuation");
    (items, next)
}

fn item_names(items: &[Value]) -> Vec<&str> {
    items
        .iter()
        .map(|item| {
            let Value::Record(fields) = item else {
                panic!("page item must preserve its record type")
            };
            let Some(Value::Text(name)) = fields.get("name") else {
                panic!("page item must preserve its name field")
            };
            name.as_str()
        })
        .collect()
}

fn list_field(
    plan: &boon_plan::MachinePlan,
    list_name: &str,
    field_name: &str,
) -> (boon_plan::ListId, boon_plan::FieldId) {
    let list = plan
        .debug_map
        .list_slots
        .iter()
        .find(|entry| entry.label == list_name)
        .and_then(|entry| entry.id.strip_prefix("list:"))
        .and_then(|id| id.parse().ok())
        .map(boon_plan::ListId)
        .unwrap_or_else(|| panic!("missing list debug label `{list_name}`"));
    let field = plan
        .storage_layout
        .list_slots
        .iter()
        .find(|slot| slot.list_id == list)
        .and_then(|slot| {
            slot.row_fields
                .iter()
                .find(|field| field.name == field_name)
        })
        .map(|field| field.field_id)
        .unwrap_or_else(|| panic!("missing `{field_name}` field on `{list_name}`"));
    (list, field)
}

fn station_ids(
    session: &MachineInstance,
    list: boon_plan::ListId,
    id_field: boon_plan::FieldId,
) -> Vec<i64> {
    session
        .list_row_snapshots(list)
        .expect("station rows must remain readable")
        .into_iter()
        .map(|row| {
            let Some(Value::Number(id)) = row.fields.get(&id_field) else {
                panic!("station access item must preserve its numeric id")
            };
            id.to_i64_exact().expect("station id must remain integral")
        })
        .collect()
}

fn station_prefix_ids(prefix: &str, bergen_only: bool) -> Vec<i64> {
    let mut ids = (0_i64..=58_499)
        .filter(|id| !bergen_only || *id >= 58_400)
        .filter(|id| format!("station-{id}").starts_with(prefix))
        .collect::<Vec<_>>();
    ids.sort_by_key(|id| format!("station-{id}"));
    ids.truncate(20);
    ids
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn typed_page_executes_and_resumes_identically_on_native_and_wasm() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "typed-page-cross-target.bn",
        r#"
store: [
    continue_page: SOURCE
    after:
        Start |> HOLD after {
            continue_page.value |> THEN { continue_page.value }
        }
    items: LIST {
        [name: TEXT { Delta }]
        [name: TEXT { Alpha }]
        [name: TEXT { Charlie }]
        [name: TEXT { Beta }]
    }
    page:
        items
        |> List/sort_by(item, key: item.name, direction: Ascending)
        |> List/page(size: 2, after: after)
]
document: Document/new(root: Element/label(element: [], label: TEXT { static }))
"#,
        TargetProfile::SoftwareDefault,
    )
    .unwrap();
    assert_eq!(compiled.plan.list_indexes.len(), 1);
    let source = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.continue_page")
        .expect("page continuation source")
        .source_id;
    let mut session = MachineInstance::new(
        compiled.plan,
        SessionOptions {
            cursor_sealing_key: Some(CursorSealingKey::from_bytes([0x91; 32])),
            cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x92; 32])),
            ..SessionOptions::default()
        },
    )
    .unwrap();

    let (first, first_metrics) = session
        .root_value_current_with_metrics("store.page")
        .unwrap();
    let (first_items, next) = page_parts(first);
    assert_eq!(item_names(&first_items), ["Alpha", "Beta"]);
    assert!(matches!(
        &next,
        Value::Record(fields)
            if fields.get("$tag") == Some(&Value::Text("Cursor".to_owned()))
                && matches!(fields.get("value"), Some(Value::Bytes(bytes)) if !bytes.is_empty())
    ));
    assert_eq!(first_metrics.access_index_seek_count, 1);
    assert_eq!(first_metrics.access_candidate_count, 3);
    assert_eq!(first_metrics.access_full_scan_count, 0);

    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source,
            route: session.source_route_token(source, &[]).unwrap(),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([("value".to_owned(), next)]),
                ..SourcePayload::default()
            },
        })
        .unwrap();
    let (second, second_metrics) = session
        .root_value_current_with_metrics("store.page")
        .unwrap();
    let (second_items, end) = page_parts(second);
    assert_eq!(item_names(&second_items), ["Charlie", "Delta"]);
    assert_eq!(end, Value::Text("End".to_owned()));
    assert_eq!(
        turn.metrics
            .access_cursor_seek_count
            .saturating_add(second_metrics.access_cursor_seek_count),
        1
    );
    assert_eq!(
        turn.metrics
            .access_full_scan_count
            .saturating_add(second_metrics.access_full_scan_count),
        0
    );
    assert!(
        turn.metrics
            .access_candidate_count
            .saturating_add(second_metrics.access_candidate_count)
            <= 2
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn fjordpulse_scale_access_matrix_is_bounded_on_native_and_wasm() {
    let compiled = boon_compiler::compile_source_text_to_machine_plan(
        "fjordpulse-scale-access.bn",
        include_str!("../../../testdata/fjordpulse_scale_access.bn"),
        TargetProfile::SoftwareDefault,
    )
    .expect("shared 58,500-row access matrix must compile");
    let index_count = compiled.plan.list_indexes.len() as u64;
    assert!(index_count >= 5);

    let access_cases = [
        (
            "store.exact",
            list_field(&compiled.plan, "store.exact", "id"),
            vec![58_499],
            1,
        ),
        (
            "store.compound_exact",
            list_field(&compiled.plan, "store.compound_exact", "id"),
            vec![58_499],
            1,
        ),
        (
            "store.prefix",
            list_field(&compiled.plan, "store.prefix", "id"),
            station_prefix_ids("station-584", false),
            20,
        ),
        (
            "store.compound_prefix",
            list_field(&compiled.plan, "store.compound_prefix", "id"),
            station_prefix_ids("station-584", true),
            20,
        ),
        (
            "store.range",
            list_field(&compiled.plan, "store.range", "id"),
            (58_440..58_460).collect(),
            20,
        ),
        (
            "store.union",
            list_field(&compiled.plan, "store.union", "id"),
            vec![1, 58_499],
            2,
        ),
        (
            "store.token_union",
            list_field(&compiled.plan, "store.token_union", "id"),
            vec![58_499, 58_498],
            2,
        ),
        (
            "store.token_intersection",
            list_field(&compiled.plan, "store.token_intersection", "id"),
            vec![58_499],
            2,
        ),
        (
            "store.spatial",
            list_field(&compiled.plan, "store.spatial", "id"),
            (58_490..=58_499).collect(),
            100,
        ),
    ];
    let page_source = compiled
        .plan
        .source_routes
        .iter()
        .find(|route| route.path == "store.page_after_input")
        .expect("page continuation source")
        .source_id;
    let mut session = MachineInstance::new(
        compiled.plan,
        SessionOptions {
            cursor_sealing_key: Some(CursorSealingKey::from_bytes([0x81; 32])),
            cursor_scope_fingerprint: Some(CursorScopeFingerprint::from_bytes([0x82; 32])),
            ..SessionOptions::default()
        },
    )
    .expect("shared 58,500-row access matrix must initialize");
    let startup = session.startup_metrics();
    assert_eq!(startup.ordered_index_full_rebuild_count, index_count);
    assert!(startup.ordered_index_rebuild_entry_count >= 58_500_u64.saturating_mul(index_count));
    assert_eq!(startup.ordered_index_current_count, index_count);
    assert_eq!(startup.ordered_index_resource_limit_failure_count, 0);

    for (name, (list, id_field), expected_ids, maximum_candidates) in access_cases {
        let (value, metrics) = session
            .list_value_current_with_metrics(list)
            .unwrap_or_else(|error| panic!("{name} failed: {error}"));
        assert!(matches!(value, Value::List(_)));
        assert_eq!(
            station_ids(&session, list, id_field),
            expected_ids,
            "{name} returned wrong rows"
        );
        assert!(metrics.access_index_seek_count >= 1, "{name} did not seek");
        assert_eq!(metrics.access_full_scan_count, 0, "{name} scanned");
        assert!(
            metrics.access_candidate_count <= maximum_candidates,
            "{name} visited {} candidates, limit is {maximum_candidates}",
            metrics.access_candidate_count
        );
        assert_eq!(metrics.ordered_index_full_rebuild_count, 0);
        assert_eq!(metrics.ordered_index_resource_limit_failure_count, 0);
        if matches!(
            name,
            "store.union" | "store.token_union" | "store.token_intersection"
        ) {
            assert!(
                metrics.access_branch_poll_count >= 2,
                "{name} skipped branches"
            );
        }
        if name == "store.spatial" {
            assert!(
                metrics.access_kernel_returned_count > metrics.access_result_count,
                "spatial residual did not prune index candidates"
            );
        }
    }

    let (first, first_metrics) = session
        .root_value_current_with_metrics("store.page")
        .expect("first station page must execute");
    let (first_items, next) = page_parts(first);
    assert_eq!(first_items.len(), 20);
    assert_eq!(first_metrics.access_index_seek_count, 1);
    assert_eq!(first_metrics.access_candidate_count, 21);
    assert_eq!(first_metrics.access_full_scan_count, 0);

    let turn = session
        .apply(SourceEvent {
            sequence: 1,
            source: page_source,
            route: session.source_route_token(page_source, &[]).unwrap(),
            target: None,
            payload: SourcePayload {
                fields: BTreeMap::from([("value".to_owned(), next)]),
                ..SourcePayload::default()
            },
        })
        .expect("page continuation source must apply");
    let (second, second_metrics) = session
        .root_value_current_with_metrics("store.page")
        .expect("deep station page must execute");
    let (second_items, _) = page_parts(second);
    assert_eq!(second_items.len(), 20);
    assert_eq!(
        turn.metrics
            .access_cursor_seek_count
            .saturating_add(second_metrics.access_cursor_seek_count),
        1
    );
    assert_eq!(
        turn.metrics
            .access_full_scan_count
            .saturating_add(second_metrics.access_full_scan_count),
        0
    );
    assert!(
        turn.metrics
            .access_candidate_count
            .saturating_add(second_metrics.access_candidate_count)
            <= 21
    );
}
