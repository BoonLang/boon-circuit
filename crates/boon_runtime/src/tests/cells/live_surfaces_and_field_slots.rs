#[test]
fn plan_field_set_delta_coalescing_keeps_last_same_target_only() {
    let row_value = |key: u64, value: JsonValue| {
        json!({
            "kind": "FieldSet",
            "list_id": "cells",
            "key": key,
            "generation": 1,
            "source_id": null,
            "bind_epoch": null,
            "field_path": "value",
            "value": value,
        })
    };
    let deltas = vec![
        row_value(4, json!("")),
        json!({
            "kind": "SourceBind",
            "list_id": "cells",
            "key": 4,
            "generation": 1,
            "source_id": 99,
            "bind_epoch": 99,
            "field_path": "cell.sources.editor.commit",
            "value": "cell.sources.editor.commit",
        }),
        row_value(5, json!("separate row")),
        row_value(4, json!(12)),
    ];

    let coalesced = coalesce_plan_field_set_deltas(deltas).expect("coalescing should not fail");

    assert_eq!(coalesced.len(), 3);
    assert_eq!(coalesced[0]["kind"], "SourceBind");
    assert_eq!(coalesced[1]["key"], 5);
    assert_eq!(coalesced[1]["value"], "separate row");
    assert_eq!(coalesced[2]["key"], 4);
    assert_eq!(coalesced[2]["value"], 12);
}


#[test]
fn plan_executor_live_surfaces_match_representative_scenario_events() {
    assert_plan_executor_live_surfaces_match_scenario_events(
        "todomvc",
        "examples/todomvc.bn",
        Path::new("../../examples/todomvc.bn"),
        Path::new("../../examples/todomvc.scn"),
        Some(&[
            "add-test-todo-type",
            "add-test-todo-submit",
            "filter-active",
            "toggle-dynamic-test-todo-under-active-filter",
        ]),
    );
    assert_plan_executor_live_surfaces_match_scenario_events(
        "bytes-source-payload",
        "examples/bytes_source_payload_plan_ops.bn",
        Path::new("../../examples/bytes_source_payload_plan_ops.bn"),
        Path::new("../../examples/bytes_source_payload_plan_ops.scn"),
        None,
    );
    assert_plan_executor_live_surfaces_match_scenario_events(
        "bytes-indexed-source-payload",
        "examples/bytes_indexed_source_payload_plan_ops.bn",
        Path::new("../../examples/bytes_indexed_source_payload_plan_ops.bn"),
        Path::new("../../examples/bytes_indexed_source_payload_plan_ops.scn"),
        None,
    );
    std::thread::Builder::new()
        .name("cells-plan-live-surfaces".to_owned())
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            assert_plan_executor_live_surfaces_match_scenario_events(
                "cells",
                "examples/cells.bn",
                Path::new("../../examples/cells.bn"),
                Path::new("../../examples/cells.scn"),
                None,
            );
        })
        .expect("Cells live-surface parity thread should start")
        .join()
        .expect("Cells live-surface parity should not panic");
}


#[test]
fn live_runtime_plan_executor_cells_window_summary_is_bounded_and_current() {
    let mut runtime = LiveRuntime::from_source(
        "cells-plan-window-summary",
        &cells_project_source_for_test(),
    )
    .expect("Cells should initialize in explicit PlanExecutor mode");
    let output = runtime
        .apply_source_event_for_document_window(
            LiveSourceEvent {
                source: "cell.sources.editor.select".to_owned(),
                address: Some("B0".to_owned()),
                ..LiveSourceEvent::default()
            },
            0,
            24,
            0,
            10,
        )
        .expect("Cells select source should apply through PlanExecutor mode");
    assert_eq!(
        output.state_summary["store"]["selected_input"]["address"],
        "B0"
    );

    let summary = runtime.document_state_summary_for_window(0, 24, 0, 10);
    assert_eq!(summary["store"]["selected_address"], "B0");
    assert_eq!(summary["store"]["selected_input"]["address"], "B0");
    assert_eq!(
        summary["store"]["selected_input"]["editing_text"],
        "=add(A0,A1)"
    );
    let rows = summary["store"]["sheet_rows"]
        .as_array()
        .expect("sheet_rows should be summarized as rows");
    assert_eq!(rows.len(), 24);
    let first_row_cells = rows[0]["cells"]
        .as_array()
        .expect("sheet row should contain cells");
    assert_eq!(first_row_cells.len(), 10);
    assert_eq!(first_row_cells[0]["address"], "A0");
    assert_eq!(first_row_cells[1]["address"], "B0");
    assert!(
        summary["__boon_materialization"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .any(|entry| entry["collection"] == "store.sheet_rows"),
        "window summary should report bounded sheet_rows materialization"
    );

    let values = runtime.document_state_values(&[
        "store.selected_input.address".to_owned(),
        "store.selected_input.editing_text".to_owned(),
    ]);
    assert_eq!(values["store.selected_input.address"], "B0");
    assert_eq!(values["store.selected_input.editing_text"], "=add(A0,A1)");
    let summaries =
        runtime.runtime_value_summaries(&["store.selected_input.editing_text".to_owned()], 3, 8, 4);
    assert_eq!(
        summaries["store.selected_input.editing_text"],
        json!({"kind": "string", "value": "=add(A0,A1)"})
    );
}


#[test]
fn runtime_field_slots_use_name_hashes_not_example_field_tables() {
    for field in [
        "title",
        "completed",
        "formula_text",
        "editing_text",
        "value",
        "error",
    ] {
        assert_eq!(
            runtime_field_id_from_name(field),
            FieldId(stable_runtime_field_id(field))
        );
    }
}


