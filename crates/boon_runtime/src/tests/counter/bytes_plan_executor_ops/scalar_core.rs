#[test]
fn root_scalar_plan_executor_replays_bytes_is_empty_update() {
    let steps = vec!["measure-bytes".to_owned()];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_is_empty_plan_ops.bn"),
        Path::new("../../examples/bytes_is_empty_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("Bytes/is_empty root scalar fixture should execute through PlanExecutor");

    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.state_summary["store.empty_is_empty"], true);
    assert_eq!(output.state_summary["store.filled_is_empty"], false);
    assert_eq!(
        output.report["capability_summary"]["cpu_plan_executor_complete"],
        true
    );
    assert_eq!(
        output.report["capability_summary"]["cpu_plan_executor_unsupported_op_count"],
        0
    );
    assert_root_scenario_product_only(&output.report);
    assert_eq!(
        output.report["plan_executor"]["executed_update_branch_count"],
        2
    );
    assert_eq!(
        output.report["plan_executor"]["executed_derived_value_count"],
        0
    );
    assert_eq!(
        output.report["plan_executor"]["executed_list_append_count"],
        0
    );
    assert_eq!(
        output.report["plan_executor"]["executed_list_remove_count"],
        0
    );
    assert_eq!(
        output.report["plan_executor"]["executed_indexed_update_count"],
        0
    );

    let updates = output.report["plan_executor"]["per_step"][0]["updates"]
        .as_array()
        .expect("PlanExecutor report should expose update array");
    let update_for = |target: &str| {
        updates
            .iter()
            .find(|update| update["target_state"] == target)
            .unwrap_or_else(|| panic!("missing update for {target}: {updates:#?}"))
    };
    let empty_update = update_for("store.empty_is_empty");
    assert_eq!(empty_update["expression_kind"], "bytes_is_empty");
    assert_eq!(empty_update["value"], true);
    assert_eq!(empty_update["source_payload_field"], JsonValue::Null);
    assert_eq!(empty_update["update_constant_id"], JsonValue::Null);
    assert_eq!(empty_update["update_constant_value"], JsonValue::Null);
    assert_eq!(empty_update["selected_op_indexed"], false);
    assert_eq!(
        empty_update["selected_op_unresolved_executable_ref_count"],
        0
    );

    let filled_update = update_for("store.filled_is_empty");
    assert_eq!(filled_update["expression_kind"], "bytes_is_empty");
    assert_eq!(filled_update["value"], false);
    assert_eq!(filled_update["source_payload_field"], JsonValue::Null);
    assert_eq!(filled_update["update_constant_id"], JsonValue::Null);
    assert_eq!(filled_update["update_constant_value"], JsonValue::Null);
    assert_eq!(filled_update["selected_op_indexed"], false);
    assert_eq!(
        filled_update["selected_op_unresolved_executable_ref_count"],
        0
    );

    for key in [
        "runtime_ast_eval_count",
        "executable_string_path_count",
        "unknown_plan_op_count",
        "graph_rebuild_count",
        "graph_clones_per_item",
    ] {
        assert_eq!(
            output.report["plan_executor"][key], 0,
            "fallback counter {key} must stay zero"
        );
    }
}

// test: root_scalar_plan_executor_replays_bytes_get_update
#[test]
fn root_scalar_plan_executor_replays_bytes_get_update() {
    let steps = vec!["measure-bytes".to_owned()];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_get_plan_ops.bn"),
        Path::new("../../examples/bytes_get_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("Bytes/get root scalar fixture should execute through PlanExecutor");

    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.state_summary["store.selected_byte"], 254);
    assert_eq!(
        output.report["capability_summary"]["cpu_plan_executor_complete"],
        true
    );
    assert_eq!(
        output.report["capability_summary"]["cpu_plan_executor_unsupported_op_count"],
        0
    );
    assert_root_scenario_product_only(&output.report);
    assert_eq!(
        output.report["plan_executor"]["executed_update_branch_count"],
        1
    );

    let update = &output.report["plan_executor"]["per_step"][0]["updates"][0];
    assert_eq!(update["target_state"], "store.selected_byte");
    assert_eq!(update["expression_kind"], "bytes_get");
    assert_eq!(update["value"], 254);
    assert!(update["update_constant_id"].is_number());
    assert_eq!(update["update_constant_value"], 2);
    assert_eq!(update["source_payload_field"], JsonValue::Null);
    assert_eq!(update["selected_op_indexed"], false);
    assert_eq!(update["selected_op_unresolved_executable_ref_count"], 0);

    for key in [
        "runtime_ast_eval_count",
        "executable_string_path_count",
        "unknown_plan_op_count",
        "graph_rebuild_count",
        "graph_clones_per_item",
    ] {
        assert_eq!(
            output.report["plan_executor"][key], 0,
            "fallback counter {key} must stay zero"
        );
    }
}

// test: root_scalar_plan_executor_replays_bytes_equal_update
#[test]
fn root_scalar_plan_executor_replays_bytes_equal_update() {
    let steps = vec!["measure-bytes".to_owned()];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_equal_plan_ops.bn"),
        Path::new("../../examples/bytes_equal_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("Bytes/equal root scalar fixture should execute through PlanExecutor");

    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.state_summary["store.same"], true);
    assert_eq!(output.state_summary["store.different"], false);
    assert_eq!(
        output.report["capability_summary"]["cpu_plan_executor_complete"],
        true
    );
    assert_eq!(
        output.report["capability_summary"]["cpu_plan_executor_unsupported_op_count"],
        0
    );
    assert_root_scenario_product_only(&output.report);
    assert_eq!(
        output.report["plan_executor"]["executed_update_branch_count"],
        2
    );
    assert_eq!(
        output.report["plan_executor"]["bytes_storage_no_copy"], true,
        "Bytes/equal should read inputs without allocating byte buffers"
    );
    assert_eq!(
        output.report["plan_executor"]["bytes_storage_counters"]["vec_alloc_bytes"], 0,
        "Bytes/equal should not report output buffer allocation"
    );

    let updates = output.report["plan_executor"]["per_step"][0]["updates"]
        .as_array()
        .expect("PlanExecutor report should expose update array");
    let update_for = |target: &str| {
        updates
            .iter()
            .find(|update| update["target_state"] == target)
            .unwrap_or_else(|| panic!("missing update for {target}: {updates:#?}"))
    };
    let same_update = update_for("store.same");
    assert_eq!(same_update["expression_kind"], "bytes_equal");
    assert_eq!(
        same_update["executor_core"]["executor"],
        "cpu-plan-root-bytes-read-evaluator-v1"
    );
    assert_eq!(same_update["value"], true);
    assert_eq!(same_update["source_payload_field"], JsonValue::Null);
    assert_eq!(same_update["update_constant_id"], JsonValue::Null);
    assert_eq!(same_update["update_constant_value"], JsonValue::Null);
    assert_eq!(same_update["selected_op_indexed"], false);
    assert_eq!(
        same_update["selected_op_unresolved_executable_ref_count"],
        0
    );

    let different_update = update_for("store.different");
    assert_eq!(different_update["expression_kind"], "bytes_equal");
    assert_eq!(different_update["value"], false);
    assert_eq!(different_update["source_payload_field"], JsonValue::Null);
    assert_eq!(different_update["update_constant_id"], JsonValue::Null);
    assert_eq!(different_update["update_constant_value"], JsonValue::Null);
    assert_eq!(different_update["selected_op_indexed"], false);
    assert_eq!(
        different_update["selected_op_unresolved_executable_ref_count"],
        0
    );

    for key in [
        "runtime_ast_eval_count",
        "executable_string_path_count",
        "unknown_plan_op_count",
        "graph_rebuild_count",
        "graph_clones_per_item",
    ] {
        assert_eq!(
            output.report["plan_executor"][key], 0,
            "fallback counter {key} must stay zero"
        );
    }
}

// test: root_scalar_plan_executor_replays_bytes_search_updates
#[test]
fn root_scalar_plan_executor_replays_bytes_search_updates() {
    let steps = vec!["build-bytes".to_owned(), "measure-bytes".to_owned()];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_search_plan_ops.bn"),
        Path::new("../../examples/bytes_search_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("Bytes/search root scalar fixture should execute through PlanExecutor");

    let joined_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "74f81fe167d99b4cb41d6d0ccda82278caee9f3e2f25d5e5a3936ff3dcec60d0",
        "byte_len": 5
    });
    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.state_summary["store.joined"], joined_summary);
    assert_eq!(output.state_summary["store.found_index"], 2);
    assert_eq!(output.state_summary["store.missing_index"], JsonValue::Null);
    assert_eq!(output.state_summary["store.empty_index"], 0);
    assert_eq!(output.state_summary["store.starts"], true);
    assert_eq!(output.state_summary["store.not_starts"], false);
    assert_eq!(output.state_summary["store.ends"], true);
    assert_eq!(output.state_summary["store.not_ends"], false);
    assert_eq!(
        output.report["capability_summary"]["cpu_plan_executor_complete"],
        true
    );
    assert_eq!(
        output.report["capability_summary"]["cpu_plan_executor_unsupported_op_count"],
        0
    );
    assert_root_scenario_product_only(&output.report);
    assert_eq!(
        output.report["plan_executor"]["executed_update_branch_count"],
        8
    );

    let first_step_updates = output.report["plan_executor"]["per_step"][0]["updates"]
        .as_array()
        .expect("first step should expose updates");
    assert_eq!(first_step_updates.len(), 1);
    assert_eq!(first_step_updates[0]["target_state"], "store.joined");
    assert_eq!(first_step_updates[0]["expression_kind"], "bytes_concat");
    assert_eq!(first_step_updates[0]["value"], joined_summary);
    assert!(
        first_step_updates[0]["value"].get("inline_bytes").is_none(),
        "public BYTES summaries must not expose inline bytes"
    );

    let second_step_updates = output.report["plan_executor"]["per_step"][1]["updates"]
        .as_array()
        .expect("second step should expose updates");
    let update_for = |target: &str| {
        second_step_updates
            .iter()
            .find(|update| update["target_state"] == target)
            .unwrap_or_else(|| panic!("missing update for {target}: {second_step_updates:#?}"))
    };
    let found_update = update_for("store.found_index");
    assert_eq!(found_update["expression_kind"], "bytes_find");
    assert_eq!(
        found_update["executor_core"]["executor"],
        "cpu-plan-root-bytes-read-evaluator-v1"
    );
    assert_eq!(found_update["value"], 2);
    assert_eq!(found_update["source_payload_field"], JsonValue::Null);
    assert_eq!(found_update["update_constant_id"], JsonValue::Null);
    assert_eq!(found_update["update_constant_value"], JsonValue::Null);

    let missing_update = update_for("store.missing_index");
    assert_eq!(missing_update["expression_kind"], "bytes_find");
    assert_eq!(missing_update["value"], JsonValue::Null);

    let empty_update = update_for("store.empty_index");
    assert_eq!(empty_update["expression_kind"], "bytes_find");
    assert_eq!(empty_update["value"], 0);

    assert_eq!(
        update_for("store.starts")["expression_kind"],
        "bytes_starts_with"
    );
    assert_eq!(
        update_for("store.starts")["executor_core"]["executor"],
        "cpu-plan-root-bytes-read-evaluator-v1"
    );
    assert_eq!(update_for("store.starts")["value"], true);
    assert_eq!(
        update_for("store.not_starts")["expression_kind"],
        "bytes_starts_with"
    );
    assert_eq!(update_for("store.not_starts")["value"], false);
    assert_eq!(
        update_for("store.ends")["expression_kind"],
        "bytes_ends_with"
    );
    assert_eq!(
        update_for("store.ends")["executor_core"]["executor"],
        "cpu-plan-root-bytes-read-evaluator-v1"
    );
    assert_eq!(update_for("store.ends")["value"], true);
    assert_eq!(
        update_for("store.not_ends")["expression_kind"],
        "bytes_ends_with"
    );
    assert_eq!(update_for("store.not_ends")["value"], false);

    for key in [
        "runtime_ast_eval_count",
        "executable_string_path_count",
        "unknown_plan_op_count",
        "graph_rebuild_count",
        "graph_clones_per_item",
    ] {
        assert_eq!(
            output.report["plan_executor"][key], 0,
            "fallback counter {key} must stay zero"
        );
    }
}

// test: root_scalar_plan_executor_replays_bytes_encoding_updates
#[test]
fn root_scalar_plan_executor_replays_bytes_encoding_updates() {
    let steps = vec![
        "build-bytes".to_owned(),
        "encode-bytes".to_owned(),
        "decode-text".to_owned(),
        "inspect-decoded".to_owned(),
    ];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_encoding_plan_ops.bn"),
        Path::new("../../examples/bytes_encoding_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("Bytes/encoding root scalar fixture should execute through PlanExecutor");

    let joined_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a",
        "byte_len": 4
    });
    let zeros_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119",
        "byte_len": 4
    });
    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.state_summary["store.joined"], joined_summary);
    assert_eq!(output.state_summary["store.hex"], "01020304");
    assert_eq!(output.state_summary["store.base64"], "AQIDBA==");
    assert_eq!(output.state_summary["store.zeros"], zeros_summary);
    assert_eq!(output.state_summary["store.decoded_hex"], joined_summary);
    assert_eq!(output.state_summary["store.decoded_base64"], joined_summary);
    assert_eq!(output.state_summary["store.zeros_len"], 4);
    assert_eq!(output.state_summary["store.decoded_hex_byte"], 3);
    assert_eq!(output.state_summary["store.decoded_base64_hex"], "01020304");
    assert_root_scenario_product_only(&output.report);
    assert_eq!(
        output.report["plan_executor"]["executed_update_branch_count"],
        9
    );

    let decode_updates = output.report["plan_executor"]["per_step"][2]["updates"]
        .as_array()
        .expect("decode step should expose updates");
    let update_for = |target: &str| {
        decode_updates
            .iter()
            .find(|update| update["target_state"] == target)
            .unwrap_or_else(|| panic!("missing update for {target}: {decode_updates:#?}"))
    };
    assert_eq!(update_for("store.zeros")["expression_kind"], "bytes_zeros");
    assert_eq!(update_for("store.zeros")["update_constant_value"], 4);
    assert_eq!(
        update_for("store.decoded_hex")["expression_kind"],
        "bytes_from_hex"
    );
    assert_eq!(
        update_for("store.decoded_hex")["executor_core"]["executor"],
        "cpu-plan-root-bytes-write-evaluator-v1"
    );
    assert_eq!(
        update_for("store.decoded_base64")["expression_kind"],
        "bytes_from_base64"
    );
    assert_eq!(
        update_for("store.decoded_base64")["executor_core"]["executor"],
        "cpu-plan-root-bytes-write-evaluator-v1"
    );
    for update in decode_updates {
        assert!(
            update["value"].get("inline_bytes").is_none(),
            "public BYTES summaries must not expose inline bytes"
        );
    }

    for key in [
        "runtime_ast_eval_count",
        "executable_string_path_count",
        "unknown_plan_op_count",
        "graph_rebuild_count",
        "graph_clones_per_item",
    ] {
        assert_eq!(
            output.report["plan_executor"][key], 0,
            "fallback counter {key} must stay zero"
        );
    }
}

// test: root_scalar_plan_executor_replays_bytes_numeric_updates
