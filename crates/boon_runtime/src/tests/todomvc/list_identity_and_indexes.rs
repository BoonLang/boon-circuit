// Included by `../todomvc.rs`.

// test: list_memory_dense_key_slots_survive_remove_and_move
#[test]
fn list_memory_dense_key_slots_survive_remove_and_move() {
    let mut list = ListMemory::from_values([
        todo_generic_row("a"),
        todo_generic_row("b"),
        todo_generic_row("c"),
        todo_generic_row("d"),
    ]);
    let (first_key, _) = list.row_identity(0).unwrap();
    let (second_key, _) = list.row_identity(1).unwrap();
    let (third_key, _) = list.row_identity(2).unwrap();
    let (fourth_key, _) = list.row_identity(3).unwrap();

    let removed = list.remove_index(1);
    assert_eq!(removed.key, second_key);
    assert_eq!(list.bound_index(second_key, 1), None);
    assert_eq!(list.len(), 3);
    assert_eq!(list.slot_capacity(), 4);
    assert_eq!(list.valid_slot_count(), 3);
    assert_eq!(list.free_slot_count(), 1);
    assert_eq!(list.bound_index(first_key, 1), Some(0));
    assert_eq!(list.bound_index(third_key, 1), Some(1));
    assert_eq!(list.bound_index(fourth_key, 1), Some(2));

    list.move_index(2, 0).unwrap();
    assert_eq!(list.bound_index(fourth_key, 1), Some(0));
    assert_eq!(list.bound_index(first_key, 1), Some(1));
    assert_eq!(list.bound_index(third_key, 1), Some(2));

    let (new_key, generation) = list.append(todo_generic_row("e"));
    assert_eq!(generation, 1);
    assert_ne!(new_key, second_key);
    assert_eq!(list.slot_capacity(), 4);
    assert_eq!(list.valid_slot_count(), 4);
    assert_eq!(list.free_slot_count(), 0);
    assert_eq!(list.bound_index(new_key, 1), Some(3));
}

// test: source_store_stale_unbinds_do_not_drop_live_row_slots
#[test]
fn source_store_stale_unbinds_do_not_drop_live_row_slots() {
    let mut sources = SourceStore::with_capacity(4);
    let paths = [
        "todo.sources.todo_checkbox.click".to_owned(),
        "todo.sources.title_input.commit".to_owned(),
    ];
    sources.bind_row("todos", 10, 1, &paths).unwrap();
    let binding = sources
        .row_bindings("todos", 10, 1)
        .find(|binding| binding.source_path == paths[0])
        .cloned()
        .unwrap();

    for (list, key, generation) in [("todos", 11, 1), ("todos", 10, 2), ("projects", 10, 1)] {
        sources.unbind_row(list, key, generation);
        sources.assert_invariants();
        assert_eq!(sources.len(), 2);
        assert_eq!(sources.row_binding_count("todos", 10, 1), 2);
        assert!(sources.is_bound(
            "todos",
            10,
            1,
            &binding.source_path,
            Some(binding.source_id),
            Some(binding.bind_epoch),
        ));
    }

    sources.unbind_row("todos", 10, 1);
    sources.assert_invariants();
    assert_eq!(sources.len(), 0);
    assert_eq!(sources.row_binding_count("todos", 10, 1), 0);
    assert!(!sources.is_bound(
        "todos",
        10,
        1,
        &binding.source_path,
        Some(binding.source_id),
        Some(binding.bind_epoch),
    ));

    sources.unbind_row("todos", 10, 1);
    sources.assert_invariants();
    assert_eq!(sources.len(), 0);
}

// test: source_store_rebinding_same_row_paths_is_idempotent
#[test]
fn source_store_rebinding_same_row_paths_is_idempotent() {
    let mut sources = SourceStore::with_capacity(4);
    let paths = [
        "todo.sources.todo_checkbox.click".to_owned(),
        "todo.sources.title_input.commit".to_owned(),
    ];
    sources.bind_row("todos", 10, 1, &paths).unwrap();
    let first_binding = sources
        .row_bindings("todos", 10, 1)
        .find(|binding| binding.source_path == paths[0])
        .cloned()
        .unwrap();

    sources.bind_row("todos", 10, 1, &paths).unwrap();
    sources.assert_invariants();

    assert_eq!(sources.len(), 2);
    assert_eq!(sources.row_binding_count("todos", 10, 1), 2);
    assert!(sources.is_bound(
        "todos",
        10,
        1,
        &first_binding.source_path,
        Some(first_binding.source_id),
        Some(first_binding.bind_epoch),
    ));
}

// test: source_store_rebinds_removed_keys_but_rejects_active_identity_collision
#[test]
fn source_store_rebinds_removed_keys_but_rejects_active_identity_collision() {
    let mut sources = SourceStore::with_capacity(4);
    let first_paths = ["todo.sources.first.click".to_owned()];
    sources.bind_row("todos", 10, 1, &first_paths).unwrap();
    sources.assert_invariants();

    assert!(
        sources
            .bind_row("todos", 10, 2, &["todo.sources.second.click".to_owned()])
            .is_err()
    );
    sources
        .bind_row(
            "projects",
            10,
            1,
            &["project.sources.name.click".to_owned()],
        )
        .unwrap();
    sources.assert_invariants();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources.row_binding_count("todos", 10, 1), 1);
    assert_eq!(sources.row_binding_count("projects", 10, 1), 1);

    sources.unbind_row("todos", 10, 1);
    sources.assert_invariants();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources.row_binding_count("todos", 10, 1), 0);
    assert_eq!(sources.row_binding_count("projects", 10, 1), 1);

    sources.unbind_row("projects", 10, 1);
    sources.assert_invariants();
    assert_eq!(sources.len(), 0);

    let second_paths = [
        "todo.sources.second.click".to_owned(),
        "todo.sources.second.commit".to_owned(),
    ];
    sources.bind_row("todos", 10, 2, &second_paths).unwrap();
    sources.assert_invariants();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources.row_binding_count("todos", 10, 2), 2);
    assert_eq!(sources.row_binding_count("todos", 10, 1), 0);
}

// test: row_identity_source_binding_resolution_reports_mismatch_reasons
#[test]
fn row_identity_source_binding_resolution_reports_mismatch_reasons() {
    let mut sources = SourceStore::with_capacity(4);
    sources
        .bind_row(
            "todos",
            10,
            1,
            &[
                "todo.sources.todo_checkbox.click".to_owned(),
                "todo.sources.title_input.commit".to_owned(),
            ],
        )
        .unwrap();
    let binding = sources
        .row_bindings("todos", 10, 1)
        .find(|binding| binding.source_path == "todo.sources.todo_checkbox.click")
        .cloned()
        .unwrap();
    let matched = sources.binding_resolution_report(
        "todos",
        10,
        1,
        &binding.source_path,
        Some(binding.source_id),
        Some(binding.bind_epoch),
    );
    assert_eq!(matched["matched"], json!(true));
    assert_eq!(matched["reason"], json!("matched"));
    assert_eq!(matched["requested"]["row_key"], json!(10));
    assert_eq!(matched["requested"]["generation"], json!(1));
    assert_eq!(
        matched["requested"]["bind_epoch"],
        json!(binding.bind_epoch)
    );

    let stale_epoch = sources.binding_resolution_report(
        "todos",
        10,
        1,
        &binding.source_path,
        Some(binding.source_id),
        Some(binding.bind_epoch + 1),
    );
    assert_eq!(stale_epoch["matched"], json!(false));
    assert_eq!(stale_epoch["reason"], json!("bind_epoch_mismatch"));

    let stale_generation = sources.binding_resolution_report(
        "todos",
        10,
        2,
        &binding.source_path,
        Some(binding.source_id),
        Some(binding.bind_epoch),
    );
    assert_eq!(stale_generation["matched"], json!(false));
    assert_eq!(stale_generation["reason"], json!("generation_mismatch"));

    sources.unbind_row("todos", 10, 1);
    let unbound = sources.binding_resolution_report(
        "todos",
        10,
        1,
        &binding.source_path,
        Some(binding.source_id),
        Some(binding.bind_epoch),
    );
    assert_eq!(unbound["matched"], json!(false));
    assert_eq!(unbound["reason"], json!("source_id_unbound"));
}

// test: source_store_row_binding_storage_grows_without_panic
#[test]
fn source_store_row_binding_storage_grows_without_panic() {
    let mut sources = SourceStore::with_capacity(64);
    let path_refs = vec!["todo.sources.dynamic.change".to_owned(); 64];
    sources.bind_row("todos", 10, 1, &path_refs).unwrap();
    sources.assert_invariants();
    assert_eq!(sources.len(), 64);
    assert_eq!(sources.row_binding_count("todos", 10, 1), 64);
}

// test: dirty_metrics_report_density_duplicates_and_recommendation
#[test]
fn dirty_metrics_report_density_duplicates_and_recommendation() {
    let deltas = [
        field_delta(
            Some(7),
            Some(1),
            "title",
            ProtocolValue::Text(Cow::Borrowed("a")),
        ),
        field_delta(Some(7), Some(1), "completed", ProtocolValue::Bool(true)),
        field_delta(Some(9), Some(1), "completed", ProtocolValue::Bool(false)),
    ];
    let mut dirty = DirtyKeySets::with_capacity(deltas.len());
    dirty.mark_deltas(&deltas);

    let metrics = dirty.metrics(
        4,
        3,
        vec!["todos[0].visible".to_owned(), "todos[1].visible".to_owned()],
    );
    let report = metrics.to_report();

    assert_eq!(metrics.entry_count, 3);
    assert_eq!(metrics.unique_key_count, 2);
    assert_eq!(metrics.duplicate_attempt_count, 1);
    assert_eq!(metrics.fanout_recompute_candidate_count, 3);
    assert_eq!(metrics.density_estimate, 0.5);
    assert_eq!(metrics.representation_boundary, "DirtyKeySets");
    assert_eq!(metrics.recommended_representation, "fixed_bitset");
    assert_eq!(report["top_recompute_causes"][0], "todos[0].visible");
}

// test: list_index_text_lookup_preserves_visible_order_and_survives_mutation
#[test]
fn list_index_text_lookup_preserves_visible_order_and_survives_mutation() {
    let mut list = ListMemory::from_values([
        todo_generic_row("duplicate"),
        todo_generic_row("middle"),
        todo_generic_row("duplicate"),
    ]);

    let (index, probe) = list.find_textlike_indexed("title", "duplicate");
    assert_eq!(index, Some(0));
    assert!(probe.used_index);
    assert_eq!(probe.candidate_count, 2);

    list.move_index(2, 0).unwrap();
    let (index, probe) = list.find_textlike_indexed("title", "duplicate");
    assert_eq!(index, Some(0));
    assert!(probe.used_index);
    assert_eq!(probe.candidate_count, 2);

    list.set_textlike(0, "title", "changed").unwrap();
    let (index, probe) = list.find_textlike_indexed("title", "duplicate");
    assert_eq!(index, Some(1));
    assert!(probe.used_index);
    assert_eq!(probe.candidate_count, 1);
}

// test: list_existing_text_index_lookup_is_read_only_and_ordered
#[test]
fn list_existing_text_index_lookup_is_read_only_and_ordered() {
    let mut list = ListMemory::from_values([
        todo_generic_row("duplicate"),
        todo_generic_row("middle"),
        todo_generic_row("duplicate"),
    ]);

    assert_eq!(
        list.find_textlike_existing_index("title", "duplicate"),
        None,
        "read-only lookup must not build an index"
    );
    assert!(list.text_lookup_indexes.is_empty());

    let (index, probe) = list.find_textlike_indexed("title", "duplicate");
    assert_eq!(index, Some(0));
    assert!(probe.used_index);

    assert_eq!(
        list.find_textlike_existing_index("title", "duplicate"),
        Some(0)
    );
    list.move_index(2, 0).unwrap();
    assert_eq!(
        list.find_textlike_existing_index("title", "duplicate"),
        Some(0)
    );
}

// test: list_index_text_lookup_returns_all_matches_in_visible_order
#[test]
fn list_index_text_lookup_returns_all_matches_in_visible_order() {
    let mut list = ListMemory::from_values([
        todo_generic_row("duplicate"),
        todo_generic_row("middle"),
        todo_generic_row("duplicate"),
    ]);

    let (indices, probe) = list.find_textlike_indices_indexed("title", "duplicate");
    assert_eq!(indices, Some(vec![0, 2]));
    assert!(probe.used_index);
    assert_eq!(probe.candidate_count, 2);

    list.move_index(2, 0).unwrap();
    let (indices, probe) = list.find_textlike_indices_indexed("title", "duplicate");
    assert_eq!(indices, Some(vec![0, 1]));
    assert!(probe.used_index);
    assert_eq!(probe.candidate_count, 2);

    list.set_textlike(0, "title", "changed").unwrap();
    let (indices, probe) = list.find_textlike_indices_indexed("title", "duplicate");
    assert_eq!(indices, Some(vec![1]));
    assert!(probe.used_index);
    assert_eq!(probe.candidate_count, 1);
}

// test: list_index_text_lookup_intersects_selection_without_reordering
#[test]
fn list_index_text_lookup_intersects_selection_without_reordering() {
    let mut list = ListMemory::from_values([
        todo_generic_row("duplicate"),
        todo_generic_row("middle"),
        todo_generic_row("duplicate"),
        todo_generic_row("other"),
        todo_generic_row("duplicate"),
    ]);

    let selection = [4, 1, 0, 3];
    let (indices, probe) =
        list.find_textlike_indices_in_selection_indexed("title", "duplicate", &selection, true);
    assert_eq!(indices, Some(vec![4, 0]));
    assert!(probe.used_index);
    assert_eq!(probe.candidate_count, selection.len());

    let (indices, probe) =
        list.find_textlike_indices_in_selection_indexed("title", "duplicate", &selection, false);
    assert_eq!(indices, Some(vec![1, 3]));
    assert!(probe.used_index);
    assert_eq!(probe.candidate_count, selection.len());
}

// test: root_list_plan_executor_replays_todomvc_submit
#[test]
fn root_list_plan_executor_replays_todomvc_submit() {
    let steps = vec![
        "add-test-todo-type".to_owned(),
        "add-test-todo-submit".to_owned(),
    ];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/todomvc.bn"),
        Path::new("../../examples/todomvc.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("TodoMVC submit should execute through root-list PlanExecutor slice");

    let report = &output.report;
    assert_eq!(report["status"], "pass");
    assert_eq!(
        report["plan_executor"]["executor"],
        "cpu-plan-root-list-scenario-v1"
    );
    assert_eq!(output.state_summary["store.new_todo_text"], "");
    assert_root_scenario_product_only(report);
    assert_eq!(report["plan_executor"]["executed_derived_value_count"], 2);
    assert_eq!(report["plan_executor"]["executed_list_append_count"], 1);
    assert_eq!(report["plan_executor"]["emitted_source_bind_count"], 6);

    let expected_signatures = json!([
        "FieldSet:store.new_todo_text",
        "FieldSet:store.title_to_add",
        "ListInsert",
        "SourceBind:todo.sources.remove_todo_button.press",
        "SourceBind:todo.sources.editing_todo_title_element.change",
        "SourceBind:todo.sources.editing_todo_title_element.key_down",
        "SourceBind:todo.sources.editing_todo_title_element.blur",
        "SourceBind:todo.sources.todo_title_element.double_click",
        "SourceBind:todo.sources.todo_checkbox.click",
        "FieldSet:not_editing",
        "FieldSet:not_completed",
        "FieldSet:store.new_todo_text",
        "FieldSet:store.active_count"
    ]);
    assert_eq!(report["semantic_delta_signatures"], expected_signatures);

    let todos = &report["plan_executor"]["list_summary"]["todos"];
    assert_eq!(todos["row_count"], 5);
    assert_eq!(todos["active_count"], 4);
    assert_eq!(todos["completed_count"], 1);
    assert_eq!(
        todos["titles"],
        json!([
            "Read documentation",
            "Finish TodoMVC renderer",
            "Walk the dog",
            "Buy groceries",
            "Test todo"
        ])
    );
    assert_eq!(todos["rows"][4]["key"], 5);
    assert_eq!(todos["rows"][4]["generation"], 1);
    assert_eq!(todos["rows"][4]["fields"]["title"], "Test todo");
    assert_eq!(todos["rows"][4]["fields"]["completed"], false);
    assert_eq!(todos["rows"][4]["fields"]["not_editing"], true);
    assert_eq!(todos["rows"][4]["fields"]["not_completed"], true);

    assert_eq!(report["semantic_deltas"][2]["kind"], "ListInsert");
    assert_eq!(report["semantic_deltas"][2]["list_id"], "todos");
    assert_eq!(report["semantic_deltas"][2]["key"], 5);
    assert_eq!(report["semantic_deltas"][2]["generation"], 1);
    assert_eq!(report["semantic_deltas"][2]["value"], "Test todo");
    let expected_source_paths = [
        "todo.sources.remove_todo_button.press",
        "todo.sources.editing_todo_title_element.change",
        "todo.sources.editing_todo_title_element.key_down",
        "todo.sources.editing_todo_title_element.blur",
        "todo.sources.todo_title_element.double_click",
        "todo.sources.todo_checkbox.click",
    ];
    for (offset, expected_path) in expected_source_paths.iter().enumerate() {
        let delta = &report["semantic_deltas"][3 + offset];
        let expected_id = 25 + offset as u64;
        assert_eq!(delta["kind"], "SourceBind");
        assert_eq!(delta["list_id"], "todos");
        assert_eq!(delta["key"], 5);
        assert_eq!(delta["generation"], 1);
        assert_eq!(delta["source_id"], expected_id);
        assert_eq!(delta["bind_epoch"], expected_id);
        assert_eq!(delta["field_path"], *expected_path);
        assert_eq!(delta["value"], *expected_path);
    }
    assert_eq!(report["semantic_deltas"][9]["field_path"], "not_editing");
    assert_eq!(report["semantic_deltas"][9]["value"], true);
    assert_eq!(report["semantic_deltas"][10]["field_path"], "not_completed");
    assert_eq!(report["semantic_deltas"][10]["value"], true);
    assert_eq!(
        report["semantic_deltas"][11]["field_path"],
        "store.new_todo_text"
    );
    assert_eq!(report["semantic_deltas"][11]["value"], "");
    assert_eq!(
        report["semantic_deltas"][12]["field_path"],
        "store.active_count"
    );
    assert_eq!(report["semantic_deltas"][12]["value"], 4);
    assert_eq!(
        report["plan_executor"]["per_step"][1]["updates"][0]["candidate_update_op_ids"],
        json!([32, 33])
    );
    assert_eq!(
        report["plan_executor"]["per_step"][1]["updates"][0]["update_constant_value"],
        ""
    );
}

// test: root_list_plan_executor_replays_todomvc_dynamic_checkbox_toggle
#[test]
fn root_list_plan_executor_replays_todomvc_dynamic_checkbox_toggle() {
    let steps = vec![
        "add-test-todo-type".to_owned(),
        "add-test-todo-submit".to_owned(),
        "filter-active".to_owned(),
        "toggle-dynamic-test-todo-under-active-filter".to_owned(),
    ];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/todomvc.bn"),
        Path::new("../../examples/todomvc.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("TodoMVC dynamic checkbox toggle should execute through indexed PlanExecutor slice");

    let report = &output.report;
    assert_eq!(report["status"], "pass");
    assert_root_scenario_product_only(report);
    assert_eq!(report["plan_executor"]["executed_indexed_update_count"], 1);
    assert_eq!(report["plan_executor"]["executed_list_append_count"], 1);
    assert_eq!(report["plan_executor"]["emitted_source_bind_count"], 6);
    for key in [
        "runtime_ast_eval_count",
        "executable_string_path_count",
        "unknown_plan_op_count",
        "graph_rebuild_count",
        "graph_clones_per_item",
    ] {
        assert_eq!(
            report["plan_executor"][key], 0,
            "fallback counter {key} must stay zero"
        );
    }

    let toggle_step = &report["plan_executor"]["per_step"][3];
    assert_eq!(
        toggle_step["semantic_delta_signatures"],
        json!([
            "FieldSet:completed",
            "FieldSet:not_completed",
            "FieldSet:store.active_count",
            "FieldSet:store.completed_count"
        ])
    );
    assert_eq!(toggle_step["executed_indexed_update_count"], 1);
    let indexed = &toggle_step["indexed_updates"][0];
    assert_eq!(indexed["source"], "todo.sources.todo_checkbox.click");
    assert_eq!(indexed["source_id"], 14);
    assert_eq!(indexed["source_binding_id"], 30);
    assert_eq!(indexed["bind_epoch"], 30);
    assert_eq!(indexed["update_op_id"], 45);
    assert_eq!(indexed["candidate_update_op_ids"], json!([45]));
    assert_eq!(indexed["selected_op_indexed"], true);
    assert_eq!(indexed["selected_op_unresolved_executable_ref_count"], 0);
    assert_eq!(indexed["expression_kind"], "bool_not");
    assert_eq!(indexed["list"], "todos");
    assert_eq!(indexed["key"], 5);
    assert_eq!(indexed["generation"], 1);
    assert_eq!(indexed["row_resolution"]["method"], "target_text");
    assert_eq!(indexed["row_resolution"]["target_text"], "Test todo");
    assert_eq!(indexed["row_resolution"]["source_binding_id"], 30);
    assert_eq!(indexed["field_path"], "completed");
    assert_eq!(indexed["value"], true);
    assert_eq!(indexed["row_fields"]["title"], "Test todo");
    assert_eq!(indexed["row_fields"]["completed"], true);
    assert_eq!(indexed["row_fields"]["not_completed"], false);
    assert_eq!(indexed["row_fields"]["not_editing"], true);

    let todos = &report["plan_executor"]["list_summary"]["todos"];
    assert_eq!(todos["row_count"], 5);
    assert_eq!(todos["active_count"], 3);
    assert_eq!(todos["completed_count"], 2);
    assert_eq!(todos["rows"][4]["key"], 5);
    assert_eq!(todos["rows"][4]["generation"], 1);
    assert_eq!(todos["rows"][4]["fields"]["title"], "Test todo");
    assert_eq!(todos["rows"][4]["fields"]["completed"], true);
    assert_eq!(todos["rows"][4]["fields"]["not_completed"], false);

    let visible = &report["plan_executor"]["list_view_summary"]["store.visible_todos"];
    assert_eq!(report["plan_executor"]["executed_list_retain_count"], 3);
    assert_eq!(report["plan_executor"]["executed_list_view_count"], 3);
    assert_eq!(report["plan_executor"]["retained_list_row_count"], 12);
    assert_eq!(visible["row_count"], 3);
    assert_eq!(
        visible["titles"],
        json!(["Read documentation", "Walk the dog", "Buy groceries"])
    );
    assert_eq!(visible["rows"][0]["key"], 1);
    assert_eq!(visible["rows"][0]["generation"], 1);
    assert_eq!(visible["rows"][1]["key"], 3);
    assert_eq!(visible["rows"][1]["source_row_index"], 2);
    assert_eq!(visible["rows"][2]["key"], 4);
    assert_eq!(
        report["plan_executor"]["list_retains"][0]["predicate"]["selector_value"],
        "Active"
    );
    assert_eq!(
        report["plan_executor"]["list_retains"][0]["executor"],
        "cpu-plan-list-retain-materializer-v1"
    );
    assert_eq!(
        toggle_step["list_view_summary"]["store.visible_todos"]["row_count"],
        3
    );
}

// test: root_list_plan_executor_replays_todomvc_delete_row
#[test]
fn root_list_plan_executor_replays_todomvc_delete_row() {
    let steps = vec![
        "add-test-todo-type".to_owned(),
        "add-test-todo-submit".to_owned(),
        "delete-dynamic-test-todo".to_owned(),
    ];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/todomvc.bn"),
        Path::new("../../examples/todomvc_plan_slices.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("TodoMVC row delete should execute through scoped list-remove PlanExecutor slice");

    let report = &output.report;
    assert_eq!(report["status"], "pass");
    assert_root_scenario_product_only(report);
    assert_eq!(report["plan_executor"]["executed_list_remove_count"], 1);
    assert_eq!(report["plan_executor"]["emitted_source_unbind_count"], 6);
    assert_eq!(report["plan_executor"]["executed_list_append_count"], 1);
    assert_eq!(report["plan_executor"]["emitted_source_bind_count"], 6);
    assert_eq!(report["plan_executor"]["executed_indexed_update_count"], 0);
    for key in [
        "runtime_ast_eval_count",
        "executable_string_path_count",
        "unknown_plan_op_count",
        "graph_rebuild_count",
        "graph_clones_per_item",
    ] {
        assert_eq!(
            report["plan_executor"][key], 0,
            "fallback counter {key} must stay zero"
        );
    }

    let delete_step = &report["plan_executor"]["per_step"][2];
    assert_eq!(
        delete_step["semantic_delta_signatures"],
        json!([
            "SourceUnbind:todo.sources.remove_todo_button.press",
            "SourceUnbind:todo.sources.editing_todo_title_element.change",
            "SourceUnbind:todo.sources.editing_todo_title_element.key_down",
            "SourceUnbind:todo.sources.editing_todo_title_element.blur",
            "SourceUnbind:todo.sources.todo_title_element.double_click",
            "SourceUnbind:todo.sources.todo_checkbox.click",
            "ListRemove",
            "FieldSet:store.active_count"
        ])
    );
    let removed = &delete_step["list_removes"][0];
    assert_eq!(removed["source"], "todo.sources.remove_todo_button.press");
    assert_eq!(removed["source_id"], 9);
    assert_eq!(removed["source_binding_id"], 25);
    assert_eq!(removed["bind_epoch"], 25);
    assert_eq!(removed["remove_op_id"], 51);
    assert_eq!(removed["list"], "todos");
    assert_eq!(removed["row_index"], 4);
    assert_eq!(removed["key"], 5);
    assert_eq!(removed["generation"], 1);
    assert_eq!(removed["row_resolution"]["method"], "key_generation");
    assert_eq!(removed["row_resolution"]["target_key"], 5);
    assert_eq!(removed["row_resolution"]["target_generation"], 1);
    assert_eq!(removed["row_resolution"]["target_text"], "Test todo");
    assert_eq!(removed["row_fields"]["title"], "Test todo");
    assert_eq!(removed["row_fields"]["completed"], false);
    assert_eq!(removed["source_unbinds"].as_array().unwrap().len(), 6);
    assert_eq!(removed["source_unbinds"][0]["source_id"], 25);
    assert_eq!(
        removed["source_unbinds"][0]["field_path"],
        "todo.sources.remove_todo_button.press"
    );
    assert_eq!(removed["source_unbinds"][5]["source_id"], 30);
    assert_eq!(
        removed["source_unbinds"][5]["field_path"],
        "todo.sources.todo_checkbox.click"
    );

    let todos = &report["plan_executor"]["list_summary"]["todos"];
    assert_eq!(todos["row_count"], 4);
    assert_eq!(todos["active_count"], 3);
    assert_eq!(todos["completed_count"], 1);
    assert_eq!(
        todos["titles"],
        json!([
            "Read documentation",
            "Finish TodoMVC renderer",
            "Walk the dog",
            "Buy groceries"
        ])
    );
    assert_eq!(todos["rows"][3]["key"], 4);
    assert_eq!(todos["rows"][3]["fields"]["title"], "Buy groceries");
}

// test: root_list_plan_executor_replays_todomvc_clear_completed
#[test]
fn root_list_plan_executor_replays_todomvc_clear_completed() {
    let steps = vec!["clear-completed".to_owned()];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/todomvc.bn"),
        Path::new("../../examples/todomvc.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("TodoMVC clear-completed should execute through row-field list-remove slice");

    let report = &output.report;
    assert_eq!(report["status"], "pass");
    assert_root_scenario_product_only(report);
    assert_eq!(report["plan_executor"]["executed_list_remove_count"], 1);
    assert_eq!(report["plan_executor"]["emitted_source_unbind_count"], 6);
    assert_eq!(report["plan_executor"]["executed_derived_value_count"], 2);
    for key in [
        "runtime_ast_eval_count",
        "executable_string_path_count",
        "unknown_plan_op_count",
        "graph_rebuild_count",
        "graph_clones_per_item",
    ] {
        assert_eq!(
            report["plan_executor"][key], 0,
            "fallback counter {key} must stay zero"
        );
    }

    let clear_step = &report["plan_executor"]["per_step"][0];
    assert_eq!(
        clear_step["semantic_delta_signatures"],
        json!([
            "SourceUnbind:todo.sources.remove_todo_button.press",
            "SourceUnbind:todo.sources.editing_todo_title_element.change",
            "SourceUnbind:todo.sources.editing_todo_title_element.key_down",
            "SourceUnbind:todo.sources.editing_todo_title_element.blur",
            "SourceUnbind:todo.sources.todo_title_element.double_click",
            "SourceUnbind:todo.sources.todo_checkbox.click",
            "ListRemove",
            "FieldSet:store.completed_count",
            "FieldSet:store.has_completed"
        ])
    );
    let removed = &clear_step["list_removes"][0];
    assert_eq!(
        removed["source"],
        "store.sources.clear_completed_button.press"
    );
    assert_eq!(removed["source_id"], 5);
    assert!(removed["source_binding_id"].is_null());
    assert!(removed["bind_epoch"].is_null());
    assert_eq!(removed["remove_op_id"], 50);
    assert_eq!(removed["key"], 2);
    assert_eq!(removed["generation"], 1);
    assert_eq!(removed["row_resolution"]["method"], "predicate");
    assert_eq!(removed["row_resolution"]["predicate"], "row_field_bool");
    assert_eq!(removed["row_resolution"]["predicate_field"], "completed");
    assert_eq!(removed["row_resolution"]["predicate_value"], true);
    assert_eq!(removed["row_fields"]["title"], "Finish TodoMVC renderer");
    assert_eq!(removed["row_fields"]["completed"], true);
    assert_eq!(removed["source_unbinds"].as_array().unwrap().len(), 6);
    assert_eq!(removed["source_unbinds"][0]["source_id"], 7);
    assert_eq!(removed["source_unbinds"][5]["source_id"], 12);
    assert_eq!(
        clear_step["derived"][0]["field_path"],
        "store.completed_count"
    );
    assert_eq!(clear_step["derived"][0]["value"], 0);
    assert_eq!(
        clear_step["derived"][1]["field_path"],
        "store.has_completed"
    );
    assert_eq!(
        clear_step["derived"][1]["expression_kind"],
        "number_compare_const"
    );
    assert_eq!(clear_step["derived"][1]["value"], false);

    let todos = &report["plan_executor"]["list_summary"]["todos"];
    assert_eq!(todos["row_count"], 3);
    assert_eq!(todos["active_count"], 3);
    assert_eq!(todos["completed_count"], 0);
    assert_eq!(
        todos["titles"],
        json!(["Read documentation", "Walk the dog", "Buy groceries"])
    );
}

// test: root_list_plan_executor_enter_whitespace_clears_without_append
#[test]
fn root_list_plan_executor_enter_whitespace_clears_without_append() {
    let steps = vec![
        "reject-empty-todo-type".to_owned(),
        "reject-empty-todo-submit".to_owned(),
    ];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/todomvc.bn"),
        Path::new("../../examples/todomvc.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("Whitespace Enter should execute without appending a row");

    let report = &output.report;
    assert_eq!(report["status"], "pass");
    assert_eq!(output.state_summary["store.new_todo_text"], "");
    assert_root_scenario_product_only(report);
    assert_eq!(report["plan_executor"]["executed_derived_value_count"], 0);
    assert_eq!(report["plan_executor"]["executed_list_append_count"], 0);
    assert_eq!(report["plan_executor"]["emitted_source_bind_count"], 0);
    assert_eq!(
        report["plan_executor"]["list_summary"]["todos"]["row_count"],
        4
    );
    assert_eq!(
        report["semantic_delta_signatures"],
        json!([
            "FieldSet:store.new_todo_text",
            "FieldSet:store.new_todo_text"
        ])
    );
    assert!(
        report["semantic_deltas"]
            .as_array()
            .unwrap()
            .iter()
            .all(|delta| delta["kind"] != "ListInsert")
    );
}
