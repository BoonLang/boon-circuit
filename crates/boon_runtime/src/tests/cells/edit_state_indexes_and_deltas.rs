#[test]
fn cells_deltas_use_hidden_list_slots_not_visible_address_hashes() {
    let mut runtime =
        LiveRuntime::from_source("cells-hidden-keys", &cells_project_source_for_test()).unwrap();
    let output = runtime
        .apply_source_event(LiveSourceEvent {
            source: "cell.sources.editor.commit".to_owned(),
            text: Some("41".to_owned()),
            address: Some("A0".to_owned()),
            ..LiveSourceEvent::default()
        })
        .unwrap();
    let expected_key = output
        .semantic_deltas
        .iter()
        .find(|delta| {
            delta.list_id.as_deref() == Some("cells")
                && delta.field_path.as_deref() == Some("formula_text")
        })
        .and_then(|delta| delta.key)
        .expect("Cells commit should emit a keyed formula_text delta");
    assert!(
        output
            .semantic_deltas
            .iter()
            .filter(|delta| delta.list_id.is_some())
            .all(|delta| delta.list_id.as_deref() == Some("cells"))
    );
    assert!(
        output
            .semantic_deltas
            .iter()
            .filter(|delta| {
                delta.kind == "FieldSet"
                    && delta.list_id.as_deref() == Some("cells")
                    && delta.field_path.as_deref() == Some("formula_text")
            })
            .all(|delta| delta.key == Some(expected_key))
    );
    assert!(output.semantic_deltas.iter().all(|delta| {
        delta
            .key
            .is_none_or(|key| key != cell_address_hash_for_test("A0"))
    }));
    assert_ne!(expected_key, cell_address_hash_for_test("A0"));
}


#[test]
fn cells_edit_state_updates_are_derived_from_ir_branches() {
    let source = cells_project_source_for_test()
        .replace("change: SOURCE", "input: SOURCE")
        .replace("commit: SOURCE", "apply: SOURCE")
        .replace("cancel: SOURCE", "revert: SOURCE")
        .replace(
            "sources.editor.events.change",
            "sources.editor.events.input",
        )
        .replace("sources.editor.change", "sources.editor.input")
        .replace("sources.editor.commit", "sources.editor.apply")
        .replace("sources.editor.cancel", "sources.editor.revert");
    let parsed = parse_source("examples/cells.bn", source).unwrap();
    lower(&parsed).unwrap();
    let mut runtime = LiveRuntime::from_source("renamed-cells-sources", &parsed.source).unwrap();
    let _output = runtime
        .apply_source_event(LiveSourceEvent {
            source: "cell.sources.editor.apply".to_owned(),
            text: Some("123".to_owned()),
            address: Some("A0".to_owned()),
            ..LiveSourceEvent::default()
        })
        .unwrap();
    let document_summary = runtime.document_state_summary();
    let a0 = cell_summary(&document_summary, "A0");
    assert_eq!(a0.get("formula_text"), Some(&json!("123")));
    assert_eq!(a0.get("editing_text"), Some(&json!("123")));
    assert_eq!(
        a0.get("value").and_then(json_scalar_text).as_deref(),
        Some("123")
    );
    assert_eq!(a0.get("editing"), Some(&json!(false)));

    let mut action = BTreeMap::new();
    action.insert(
        "kind".to_owned(),
        toml::Value::String("key_down".to_owned()),
    );
    action.insert(
        "target".to_owned(),
        toml::Value::String("A0 editor".to_owned()),
    );
    action.insert("key".to_owned(), toml::Value::String("Escape".to_owned()));
    let mut expected = BTreeMap::new();
    expected.insert(
        "source".to_owned(),
        toml::Value::String("cell.sources.editor.revert".to_owned()),
    );
    expected.insert("address".to_owned(), toml::Value::String("A0".to_owned()));
    let step = ScenarioStep {
        id: "renamed-cell-revert".to_owned(),
        user_action: Some(action),
        expected_source_event: Some(expected),
        ..ScenarioStep::default()
    };
    let output = runtime
        .apply_source_event_for_step(
            &step,
            LiveSourceEvent {
                source: "cell.sources.editor.revert".to_owned(),
                address: Some("A0".to_owned()),
                ..LiveSourceEvent::default()
            },
        )
        .unwrap();
    let document_summary = runtime.document_state_summary();
    let a0 = cell_summary(&document_summary, "A0");
    assert_eq!(a0.get("editing_text"), Some(&json!("123")));
    assert_eq!(a0.get("editing"), Some(&json!(false)));
    assert!(output.semantic_deltas.iter().any(|delta| {
        delta.kind == "FieldSet"
            && delta.list_id.as_deref() == Some("cells")
            && delta.field_path.as_deref() == Some("editing_text")
    }));
}


#[test]
fn cells_escape_cancel_restores_uncommitted_draft_from_row_formula_text() {
    let mut runtime =
        LiveRuntime::from_source("cells-cancel-draft", &cells_project_source_for_test()).unwrap();
    runtime
        .apply_source_event(LiveSourceEvent {
            source: "cell.sources.editor.change".to_owned(),
            text: Some("42".to_owned()),
            address: Some("A0".to_owned()),
            ..LiveSourceEvent::default()
        })
        .unwrap();
    let changed_summary = runtime.document_state_summary();
    let changed_a0 = cell_summary(&changed_summary, "A0");
    assert_eq!(changed_a0.get("formula_text"), Some(&json!("5")));
    assert_eq!(changed_a0.get("editing_text"), Some(&json!("42")));
    assert_eq!(changed_a0.get("editing"), Some(&json!(true)));

    let output = runtime
        .apply_source_event(LiveSourceEvent {
            source: "cell.sources.editor.cancel".to_owned(),
            key: Some("Escape".to_owned()),
            address: Some("A0".to_owned()),
            ..LiveSourceEvent::default()
        })
        .unwrap();
    let document_summary = runtime.document_state_summary();
    let a0 = cell_summary(&document_summary, "A0");
    assert_eq!(a0.get("formula_text"), Some(&json!("5")));
    assert_eq!(a0.get("editing_text"), Some(&json!("5")));
    assert_eq!(a0.get("editing"), Some(&json!(false)));
    assert!(output.semantic_deltas.iter().any(|delta| {
        delta.kind == "FieldSet"
            && delta.list_id.as_deref() == Some("cells")
            && delta.field_path.as_deref() == Some("editing_text")
    }));
}


#[test]
fn dirty_keysets_track_list_field_keys_and_reuse_storage() {
    let deltas = [
        field_delta(
            Some(7),
            Some(1),
            "title",
            ProtocolValue::Text(Cow::Borrowed("a")),
        ),
        field_delta(Some(7), Some(1), "completed", ProtocolValue::Bool(true)),
        field_delta(Some(9), Some(1), "completed", ProtocolValue::Bool(false)),
        field_delta(
            None,
            None,
            "store.selected_filter",
            ProtocolValue::Text(Cow::Borrowed("All")),
        ),
    ];
    let mut dirty = DirtyKeySets::with_capacity(4);
    assert_eq!(dirty.mark_deltas(&deltas), 2);
    assert_eq!(dirty.entries.len(), 3);
    let capacity = dirty.entries.capacity();

    dirty.mark_indexes("cells", "value", &[0, 2, 2, 5]);
    assert_eq!(dirty.key_count(), 3);
    assert_eq!(dirty.entries.capacity(), capacity);
}


#[test]
fn list_index_text_lookup_survives_unrelated_text_field_mutation() {
    fn cell_row(address: &str, formula: &str) -> RuntimeRowSnapshot {
        let mut columns = ValueColumns::default();
        columns.insert_value("address".to_owned(), FieldValue::Text(address.to_owned()));
        columns.insert_value(
            "formula_text".to_owned(),
            FieldValue::Text(formula.to_owned()),
        );
        RuntimeRowSnapshot { columns }
    }

    let mut list = ListMemory::from_values([
        cell_row("A0", "1"),
        cell_row("B0", "2"),
        cell_row("C0", "3"),
    ]);

    let (indices, probe) = list.find_textlike_indices_indexed("address", "B0");
    assert_eq!(indices, Some(vec![1]));
    assert!(probe.used_index);
    assert_eq!(list.text_lookup_indexes.len(), 1);
    assert_eq!(
        list.text_lookup_indexes[0].field_id,
        FieldSlotId::from_path("address")
    );

    list.set_textlike(1, "formula_text", "=add(A0,C0)").unwrap();

    assert_eq!(
        list.text_lookup_indexes.len(),
        1,
        "updating formula_text should not evict the address lookup index"
    );
    assert_eq!(
        list.text_lookup_indexes[0].field_id,
        FieldSlotId::from_path("address")
    );
    let (indices, probe) = list.find_textlike_indices_indexed("address", "B0");
    assert_eq!(indices, Some(vec![1]));
    assert!(probe.used_index);
    assert_eq!(probe.candidate_count, 1);

    list.set_textlike(1, "address", "B1").unwrap();

    assert!(
        list.text_lookup_indexes.is_empty(),
        "updating address must still evict the address lookup index"
    );
    let (indices, probe) = list.find_textlike_indices_indexed("address", "B0");
    assert_eq!(indices, Some(vec![]));
    assert!(probe.used_index);
    assert_eq!(probe.candidate_count, 0);
}


#[test]
fn list_batch_text_write_preserves_unrelated_text_lookup_indexes() {
    fn cell_row(address: &str, formula: &str) -> RuntimeRowSnapshot {
        let mut columns = ValueColumns::default();
        columns.insert_value("address".to_owned(), FieldValue::Text(address.to_owned()));
        columns.insert_value(
            "formula_text".to_owned(),
            FieldValue::Text(formula.to_owned()),
        );
        RuntimeRowSnapshot { columns }
    }

    let mut list = ListMemory::from_values([
        cell_row("A0", "1"),
        cell_row("B0", "2"),
        cell_row("C0", "3"),
    ]);

    let (indices, probe) = list.find_textlike_indices_indexed("address", "B0");
    assert_eq!(indices, Some(vec![1]));
    assert!(probe.used_index);
    let changed = list
        .set_or_replace_text_values(
            "formula_text",
            vec!["1".to_owned(), "=add(A0,C0)".to_owned(), "3".to_owned()],
        )
        .unwrap();
    assert_eq!(changed, vec![1]);
    assert_eq!(
        list.text_lookup_indexes.len(),
        1,
        "batch-updating formula_text should not evict the address lookup index"
    );
    assert_eq!(
        list.text_lookup_indexes[0].field_id,
        FieldSlotId::from_path("address")
    );
    let (indices, probe) = list.find_textlike_indices_indexed("address", "B0");
    assert_eq!(indices, Some(vec![1]));
    assert!(probe.used_index);
    assert_eq!(probe.candidate_count, 1);

    let changed = list
        .set_or_replace_text_values(
            "address",
            vec!["A0".to_owned(), "B1".to_owned(), "C0".to_owned()],
        )
        .unwrap();
    assert_eq!(changed, vec![1]);
    assert!(
        list.text_lookup_indexes.is_empty(),
        "batch-updating address must evict the address lookup index"
    );
}


#[test]
fn cells_plan_executor_scenario_events_are_product_only() {
    std::thread::Builder::new()
        .name("cells-plan-product-events".to_owned())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let output = run_plan_scenario_events(
                Path::new("../../examples/cells.bn"),
                Path::new("../../examples/cells.scn"),
                TargetProfile::SoftwareDefault,
                None,
            )
            .expect("Cells expected-source-event scenario should run through PlanExecutor");

            assert!(
                output.report.get("comparison_status").is_none(),
                "Cells product scenario events must not emit comparison status"
            );
            assert_eq!(
                output.report["status"], "pass",
                "Cells event replay report should pass through the product PlanExecutor path"
            );
            assert_eq!(
                output.report["command_report_assembly_core"]["executor"],
                "cpu-plan-scenario-events-command-report-assembly-v1"
            );
            assert_eq!(
                output.report["plan_executor"]["command_output_core"]["executor"],
                "cpu-plan-scenario-events-command-output-v1"
            );
            assert_eq!(
                output.report["plan_executor_coverage"]["executor"],
                "cpu-plan-root-scenario-coverage-report-v1"
            );
            assert_eq!(
                output.report["plan_executor_coverage"]["covers_assertion_only_steps"],
                true
            );
            assert_eq!(
                output.report["plan_executor_coverage"]["full_scenario_parity"],
                true
            );
            assert_eq!(
                output.report["plan_executor_coverage"]["assertion_checkpoint_count"],
                6
            );
            assert_eq!(
                output.report["plan_executor"]["list_projections"][0]["executor"],
                "cpu-plan-list-projection-materializer-v1"
            );
            assert_eq!(
                output.report["plan_executor"]["assertion_checkpoints"]
                    .as_array()
                    .expect("Cells product report should expose assertion checkpoints")
                    .iter()
                    .map(|checkpoint| checkpoint["step_id"].as_str().unwrap().to_owned())
                    .collect::<Vec<_>>(),
                vec![
                    "initial",
                    "initial-add-function",
                    "initial-sum-function",
                    "initial-empty-cell-is-blank",
                    "a0-recomputes-after-cycle-break",
                    "d0-updated-by-fanout"
                ]
            );
        })
        .expect("Cells PlanExecutor product scenario thread should start")
        .join()
        .expect("Cells PlanExecutor product scenario should not panic");
}


