#[test]
fn cells_generic_derived_runtime_plan_covers_roots_indexes_and_functions() {
    let parsed = parse_cells_project_for_test();
    let ir = lower(&parsed).unwrap();
    let compiled = CompiledProgram::from_ir(&ir).unwrap();
    assert_eq!(
        compiled.generic_derived_runtime.supported_root_count(),
        2,
        "Cells should support selected_input and sheet_rows root list views"
    );
    assert_eq!(
        compiled.generic_derived_runtime.supported_indexed_count(),
        7,
        "Cells should support all indexed derived row fields"
    );
    assert!(
        compiled
            .generic_derived_runtime
            .unsupported_reasons
            .is_empty(),
        "Cells runtime generic-derived plan should have no blockers: {:?}",
        compiled.generic_derived_runtime.unsupported_reasons
    );
    for function in [
        "cell_address",
        "default_formula_for_address",
        "compute_value",
    ] {
        assert!(
            compiled
                .generic_derived_runtime
                .functions
                .contains_key(function),
            "runtime plan should include reachable Cells function `{function}`"
        );
    }
}


#[test]
fn cells_selected_input_list_find_materializes_single_row_storage() {
    let source = cells_project_source_for_test();
    let mut runtime =
        LiveRuntime::from_source("cells-selected-input-list-find-storage", &source).unwrap();
    let initial = runtime.document_state_summary();
    assert_eq!(initial["store"]["selected_input"]["address"], "A0");
    assert_eq!(
        json_scalar_text(&initial["store"]["selected_input"]["value"]).as_deref(),
        Some("5")
    );

    let output = commit_cell(&mut runtime, "B0", "=A0+1");
    assert_eq!(
        output.state_summary["store"]["selected_input"]["address"],
        "B0"
    );
    assert_eq!(
        json_scalar_text(&output.state_summary["store"]["selected_input"]["value"]).as_deref(),
        Some("6")
    );
}


#[test]
fn cells_selected_input_document_state_values_use_indexed_list_find_projection() {
    let source = cells_project_source_for_test();
    let mut runtime =
        LiveRuntime::from_source("cells-selected-input-targeted-values", &source).unwrap();
    runtime
        .apply_source_event_turn(LiveSourceEvent {
            source: "cell.sources.editor.select".to_owned(),
            address: Some("B0".to_owned()),
            ..LiveSourceEvent::default()
        })
        .unwrap();
    let values = runtime.document_state_values(&[
        "store.selected_input.address".to_owned(),
        "store.selected_input.editing_text".to_owned(),
        "store.selected_input.sources.editor.blur".to_owned(),
    ]);
    assert_eq!(values["store.selected_input.address"], "B0");
    assert_eq!(values["store.selected_input.editing_text"], "=add(A0,A1)");
    assert_eq!(
        values["store.selected_input.sources.editor.blur"]["source_path"],
        "cell.sources.editor.blur"
    );
    let summary = runtime.document_state_summary();
    assert_eq!(summary["store"]["selected_input"]["address"], "B0");
}


#[test]
fn cells_window_document_summary_keeps_selected_projection_current() {
    let source = cells_project_source_for_test();
    let mut runtime =
        LiveRuntime::from_source("cells-selected-input-window-summary", &source).unwrap();
    runtime
        .apply_source_event_turn(LiveSourceEvent {
            source: "cell.sources.editor.select".to_owned(),
            address: Some("B0".to_owned()),
            ..LiveSourceEvent::default()
        })
        .unwrap();

    let summary = runtime.document_state_summary_for_window(0, 24, 0, 10);
    assert_eq!(summary["store"]["selected_address"], "B0");
    assert_eq!(summary["store"]["selected_input"]["address"], "B0");
    assert_eq!(
        summary["store"]["selected_input"]["editing_text"],
        "=add(A0,A1)"
    );
    let rows = summary["store"]["sheet_rows"].as_array().unwrap();
    let cells = rows[0]["cells"].as_array().unwrap();
    assert_eq!(cells[0]["address"], "A0");
    assert_eq!(cells[1]["address"], "B0");
}


#[test]
fn cells_visible_value_edit_keeps_window_summary_bounded_and_current() {
    let source = cells_project_source_for_test();
    let mut runtime = LiveRuntime::from_source("cells-chunk-row-field-edit-skip", &source).unwrap();

    let initial = runtime.document_state_summary_for_window(0, 24, 0, 10);
    assert_eq!(initial["store"]["sheet_rows"].as_array().unwrap().len(), 24);

    runtime
        .apply_source_event_turn(LiveSourceEvent {
            source: "cell.sources.editor.commit".to_owned(),
            text: Some("20".to_owned()),
            key: Some("Enter".to_owned()),
            address: Some("A0".to_owned()),
            ..LiveSourceEvent::default()
        })
        .expect("visible cell commit should apply");
    let updated = runtime.document_state_summary_for_window(0, 24, 0, 10);
    assert_eq!(updated["store"]["sheet_rows"].as_array().unwrap().len(), 24);
    assert_eq!(
        json_scalar_text(&updated["store"]["sheet_rows"][0]["cells"][0]["value"]).as_deref(),
        Some("20")
    );
}


#[test]
fn generic_derived_state_skips_unchanged_root_read_replacement() {
    let mut state = GenericDerivedState::default();
    let mut reads = root_read_keys_for_path("store.selected_address")
        .into_iter()
        .collect::<BTreeSet<_>>();
    reads.insert(list_read_key("cells"));
    reads.insert(list_column_read_key("cells", "address"));

    assert!(
        state.replace_root_reads("store.selected_input".to_owned(), reads.clone()),
        "first root read registration should install dependency edges"
    );
    let reads_before = state.root_reads_by_field.clone();
    let dependents_before = state.root_dependents_by_read.clone();

    assert!(
        !state.replace_root_reads("store.selected_input".to_owned(), reads),
        "unchanged root read registration should not churn dependency edges"
    );
    assert_eq!(state.root_reads_by_field, reads_before);
    assert_eq!(state.root_dependents_by_read, dependents_before);

    let shared_read = list_read_key("cells");
    let removed_read = list_column_read_key("cells", "address");
    let added_read = list_column_read_key("cells", "value");
    let mut changed_reads = BTreeSet::new();
    changed_reads.insert(shared_read.clone());
    changed_reads.insert(added_read.clone());
    assert!(
        state.replace_root_reads("store.selected_input".to_owned(), changed_reads),
        "changed root reads should update only the dependency edge diff"
    );
    assert!(
        state
            .root_dependents_by_read
            .get(&shared_read)
            .is_some_and(|dependents| dependents.contains("store.selected_input")),
        "shared read edge should remain registered"
    );
    assert!(
        !state.root_dependents_by_read.contains_key(&removed_read),
        "removed read edge should be deleted"
    );
    assert!(
        state
            .root_dependents_by_read
            .get(&added_read)
            .is_some_and(|dependents| dependents.contains("store.selected_input")),
        "added read edge should be registered"
    );
}


#[test]
fn example_paths_resolve_from_examples_directory() {
    let (source, scenario, budget) = example_paths("todo").unwrap();
    assert!(source.ends_with(Path::new("examples/todomvc.bn")));
    assert!(scenario.ends_with(Path::new("examples/todomvc.scn")));
    assert!(budget.ends_with(Path::new("examples/todomvc.budget.toml")));

    let (source, scenario, budget) = example_paths("cells").unwrap();
    assert!(source.ends_with(Path::new("examples/cells.bn")));
    assert!(scenario.ends_with(Path::new("examples/cells.scn")));
    assert!(budget.ends_with(Path::new("examples/cells.budget.toml")));

    let err = example_paths("../cells").unwrap_err();
    assert!(err.to_string().contains("invalid example name"));
}


#[test]
fn manifest_source_files_are_loaded_as_one_cells_project() {
    let entry = example_manifest_entry("cells").unwrap();
    assert_eq!(
        entry.source_files,
        vec![
            "examples/cells/defaults.bn".to_owned(),
            "examples/cells/formula.bn".to_owned(),
            "examples/cells/cell.bn".to_owned(),
            "examples/cells/model.bn".to_owned(),
            "examples/cells/columns.bn".to_owned(),
            "examples/cells/store.bn".to_owned(),
            "examples/cells/view.bn".to_owned(),
            "examples/cells.bn".to_owned()
        ]
    );
    let compiled = compile_source_path_to_full_ir(Path::new("../../examples/cells.bn")).unwrap();
    let parsed = compiled.parsed;
    let ir = compiled.ir;
    assert_eq!(parsed.files.len(), 8);
    assert!(
        parsed
            .functions
            .iter()
            .any(|function| function == "new_cell")
    );
    assert!(
        parsed
            .functions
            .iter()
            .any(|function| function == "new_sheet_column")
    );
    assert!(
        parsed
            .functions
            .iter()
            .any(|function| function == "compute_value")
    );
    assert!(
        parsed
            .operators
            .iter()
            .all(|operator| !operator.starts_with(&["For", "mula", "/"].concat()))
    );
    assert!(
        parsed
            .functions
            .iter()
            .any(|function| function == "cells_app")
    );
    let generic_derived_plan = compiler_generic_derived_plan_from_ir(&ir);
    assert!(generic_derived_plan.indexed_fields.iter().any(|value| {
        value.list == "cells" && value.field == "value" && value.kind == DerivedValueKind::Pure
    }));
    let source_routes = compiler_source_route_sources_from_ir(&ir);
    assert!(source_routes.iter().any(|source| {
        source.path == "cell.sources.editor.commit"
            && source.payload_fields
                == vec![
                    CompilerSourcePayloadField::Address,
                    CompilerSourcePayloadField::Text,
                ]
    }));
}


#[test]
fn source_initializers_are_read_from_boon_text() {
    let todo_source = include_str!("../../../../../examples/todomvc.bn")
        .replace("Read documentation", "Source title A")
        .replace("Buy groceries", "Source title B");
    let parsed = parse_source("examples/todomvc.bn", todo_source).unwrap();
    let ir = lower(&parsed).unwrap();
    assert_eq!(
        todomvc_initial_titles_from_ir(&ir).unwrap(),
        vec![
            "Source title A",
            "Finish TodoMVC renderer",
            "Walk the dog",
            "Source title B"
        ]
    );

    let cells_source = cells_project_source_for_test().replace(
        "List/range(from: 0, to: 2599)",
        "List/range(from: 0, to: 11)",
    );
    let parsed = parse_source("examples/cells.bn", &cells_source).unwrap();
    let ir = lower(&parsed).unwrap();
    assert_eq!(cells_range_from_ir(&ir), Some((0, 11)));

    let cells_source = cells_project_source_for_test().replace(
        "[address: TEXT { A0 }, field: TEXT { default_formula }, value: TEXT { 5 }]",
        "[address: TEXT { A0 }, field: TEXT { default_formula }, value: TEXT { 9 }]",
    );
    let parsed = parse_source("examples/cells.bn", &cells_source).unwrap();
    let ir = lower(&parsed).unwrap();
    let defaults = compiler_storage_initial_rows_from_ir(&ir)
        .into_iter()
        .find(|rows| rows.list == "cells_default_values")
        .expect("Cells source should lower generic default values");
    assert!(!defaults.rows.is_empty());
    assert!(defaults.rows.iter().any(|row| {
        row.fields.iter().any(|field| {
            field.name == "address"
                && matches!(&field.value, CompilerInitialValue::Text(value) if value == "A0")
        }) && row.fields.iter().any(|field| {
            field.name == "value"
                && matches!(&field.value, CompilerInitialValue::Text(value) if value == "9")
        })
    }));
    let mut runtime = LiveRuntime::from_source("cells-defaults-from-boon", &cells_source).unwrap();
    let summary = runtime.document_state_summary();
    let a0 = summary
        .get("cells")
        .and_then(serde_json::Value::as_array)
        .and_then(|cells| {
            cells
                .iter()
                .find(|cell| cell.get("address") == Some(&json!("A0")))
        })
        .expect("Cells state summary should include A0");
    assert_eq!(a0.get("formula_text"), Some(&json!("9")));
    assert_eq!(
        a0.get("value").and_then(json_scalar_text).as_deref(),
        Some("9")
    );
}

