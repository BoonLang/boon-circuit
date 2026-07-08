#[test]
fn pure_boon_cells_helpers_support_documented_arithmetic_ops() {
    let mut runtime =
        LiveRuntime::from_source("cells-arithmetic", &cells_project_source_for_test()).unwrap();
    for (formula, expected) in [
        ("=8+2", "10"),
        ("=8-2", "6"),
        ("=8*2", "16"),
        ("=8/2", "4"),
        ("=add(8,2)", "10"),
    ] {
        let output = commit_cell(&mut runtime, "A0", formula);
        assert_eq!(
            cell_summary_field_text(&output.state_summary, "A0", "value").as_deref(),
            Some(expected)
        );
        assert!(cell_summary_field_has_no_error(&output.state_summary, "A0"));
    }
    let output = commit_cell(&mut runtime, "A0", "=8/0");
    assert_eq!(
        cell_summary(&output.state_summary, "A0")["error"],
        "div_by_zero"
    );
    let output = commit_cell(&mut runtime, "D0", "");
    assert_eq!(
        cell_summary_field_text(&output.state_summary, "D0", "value").as_deref(),
        Some("")
    );
    let output = commit_cell(&mut runtime, "E0", "=D0+2");
    assert_eq!(
        cell_summary_field_text(&output.state_summary, "E0", "value").as_deref(),
        Some("2")
    );
    let output = commit_cell(&mut runtime, "A0", "caf\u{00e9}");
    assert_eq!(
        cell_summary_field_text(&output.state_summary, "A0", "value").as_deref(),
        Some("caf\u{00e9}")
    );
    assert_eq!(
        cell_summary_field_has_no_error(&output.state_summary, "A0"),
        true
    );
    let output = commit_cell(&mut runtime, "A0", "=caf\u{00e9}+1");
    assert_eq!(
        cell_summary(&output.state_summary, "A0")["error"],
        "parse_error"
    );
    commit_cell(&mut runtime, "A0", "5");
    commit_cell(&mut runtime, "A1", "10");
    commit_cell(&mut runtime, "A2", "15");
    let output = commit_cell(&mut runtime, "C0", "=sum(A0:A2)");
    assert_eq!(
        cell_summary_field_text(&output.state_summary, "C0", "value").as_deref(),
        Some("30")
    );
    assert_eq!(
        cell_summary_field_has_no_error(&output.state_summary, "C0"),
        true
    );
}


#[test]
fn cells_unrelated_row_commit_preserves_default_sum_until_formula_changes() {
    let mut runtime = LiveRuntime::from_source(
        "cells-unrelated-row-commit",
        &cells_project_source_for_test(),
    )
    .unwrap();
    let initial = runtime.document_state_summary();
    assert_eq!(cell_summary(&initial, "C0")["formula_text"], "=sum(A0:A2)");
    assert_eq!(
        cell_summary_field_text(&initial, "C0", "value").as_deref(),
        Some("30")
    );

    let output = commit_cell(&mut runtime, "A3", "20");
    assert_eq!(
        cell_summary_field_text(&output.state_summary, "A3", "value").as_deref(),
        Some("20")
    );
    assert_eq!(
        cell_summary(&output.state_summary, "C0")["formula_text"],
        "=sum(A0:A2)"
    );
    assert_eq!(
        cell_summary_field_text(&output.state_summary, "C0", "value").as_deref(),
        Some("30"),
        "editing A3 must not change the C0 sum until C0 references A3"
    );
}


#[test]
fn pure_boon_cells_replacing_reference_removes_stale_dependents() {
    let mut runtime =
        LiveRuntime::from_source("cells-replace-reference", &cells_project_source_for_test())
            .unwrap();
    commit_cell(&mut runtime, "A0", "1");
    let output = commit_cell(&mut runtime, "B0", "=A0+1");
    assert_eq!(
        cell_summary_field_text(&output.state_summary, "B0", "value").as_deref(),
        Some("2")
    );

    let output = commit_cell(&mut runtime, "B0", "5");
    assert_eq!(
        cell_summary_field_text(&output.state_summary, "B0", "value").as_deref(),
        Some("5")
    );

    let output = commit_cell(&mut runtime, "A0", "10");
    assert_eq!(
        cell_summary_field_text(&output.state_summary, "A0", "value").as_deref(),
        Some("10")
    );
    assert_eq!(
        cell_summary_field_text(&output.state_summary, "B0", "value").as_deref(),
        Some("5")
    );
    assert!(
        !output.semantic_deltas.iter().any(|delta| {
            delta.field_path.as_deref() == Some("value")
                && matches!(delta.value, ProtocolValue::Text(ref value) if value.as_ref() == "11")
        }),
        "B0 must not keep a stale dependency on A0 after becoming a literal"
    );
}


#[test]
fn pure_boon_cells_fanout_recomputes_from_generic_read_index() {
    let mut runtime =
        LiveRuntime::from_source("cells-fanout", &cells_project_source_for_test()).unwrap();
    commit_cell(&mut runtime, "A0", "1");
    commit_cell(&mut runtime, "B0", "=A0+1");
    commit_cell(&mut runtime, "C0", "=A0+2");
    commit_cell(&mut runtime, "D0", "=A0+3");

    let output = commit_cell(&mut runtime, "A0", "10");
    assert_eq!(
        cell_summary_field_text(&output.state_summary, "B0", "value").as_deref(),
        Some("11")
    );
    assert_eq!(
        cell_summary_field_text(&output.state_summary, "C0", "value").as_deref(),
        Some("12")
    );
    assert_eq!(
        cell_summary_field_text(&output.state_summary, "D0", "value").as_deref(),
        Some("13")
    );
    let value_delta_count = output
        .semantic_deltas
        .iter()
        .filter(|delta| delta.field_path.as_deref() == Some("value"))
        .count();
    assert!(
        value_delta_count < 16,
        "A0 fanout must stay sparse instead of emitting full-grid value deltas"
    );
}


#[test]
fn pure_boon_cells_range_formula_updates_from_member_change() {
    let mut runtime =
        LiveRuntime::from_source("cells-range-fanout", &cells_project_source_for_test()).unwrap();
    commit_cell(&mut runtime, "A3", "20");
    let output = commit_cell(&mut runtime, "C0", "=sum(A0:A3)");
    assert_eq!(
        cell_summary_field_text(&output.state_summary, "C0", "value").as_deref(),
        Some("50")
    );

    let output = runtime
        .apply_source_event_turn(LiveSourceEvent {
            source: "cell.sources.editor.commit".to_owned(),
            text: Some("30".to_owned()),
            key: Some("Enter".to_owned()),
            address: Some("A3".to_owned()),
            ..LiveSourceEvent::default()
        })
        .unwrap();
    let summary = runtime.document_state_summary();
    assert_eq!(
        cell_summary_field_text(&summary, "A3", "value").as_deref(),
        Some("30")
    );
    assert_eq!(
        cell_summary_field_text(&summary, "C0", "value").as_deref(),
        Some("60"),
        "range dependencies should invalidate formulas that read the changed member"
    );
    assert!(
        output.recomputed_field_samples.len() < 16,
        "range fanout accounting should stay sparse instead of reporting a full Cells grid recompute, got {:?}",
        output.recomputed_field_samples
    );
}

fn cell_address_hash_for_test(address: &str) -> u64 {
    let hash = sha256_bytes(address.as_bytes());
    let bytes = hex_prefix_to_bytes(&hash, 8);
    u64::from_le_bytes(bytes.try_into().unwrap())
}

fn cell_summary<'a>(summary: &'a JsonValue, address: &str) -> &'a JsonValue {
    summary
        .get("cells")
        .and_then(JsonValue::as_array)
        .and_then(|cells| {
            cells
                .iter()
                .find(|cell| cell.get("address") == Some(&json!(address)))
        })
        .unwrap_or_else(|| panic!("Cells state summary should include {address}"))
}

fn cell_summary_field_text(summary: &JsonValue, address: &str, field: &str) -> Option<String> {
    cell_summary(summary, address)
        .get(field)
        .and_then(json_scalar_text)
}

fn cell_summary_field_has_no_error(summary: &JsonValue, address: &str) -> bool {
    let error = &cell_summary(summary, address)["error"];
    error.is_null() || error.as_str() == Some("")
}

fn commit_cell(runtime: &mut LiveRuntime, address: &str, text: &str) -> LiveStepOutput {
    let mut output = runtime
        .apply_source_event(LiveSourceEvent {
            source: "cell.sources.editor.commit".to_owned(),
            text: Some(text.to_owned()),
            address: Some(address.to_owned()),
            ..LiveSourceEvent::default()
        })
        .unwrap();
    if output.state_summary.get("cells").is_none() {
        output.state_summary = runtime.document_state_summary();
    }
    output
}

fn hex_prefix_to_bytes(hash: &str, len: usize) -> Vec<u8> {
    (0..len)
        .map(|index| u8::from_str_radix(&hash[index * 2..index * 2 + 2], 16).unwrap())
        .collect()
}

fn todo_checkbox_expected_source(target_text: &str) -> BTreeMap<String, toml::Value> {
    let mut expected = BTreeMap::new();
    expected.insert(
        "source".to_owned(),
        toml::Value::String("todo.sources.todo_checkbox.click".to_owned()),
    );
    expected.insert(
        "target_text".to_owned(),
        toml::Value::String(target_text.to_owned()),
    );
    expected
}
