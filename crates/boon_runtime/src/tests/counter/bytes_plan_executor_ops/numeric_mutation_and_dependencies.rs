#[test]
fn root_scalar_plan_executor_replays_bytes_numeric_updates() {
    let steps = vec![
        "measure-bytes".to_owned(),
        "write-bytes".to_owned(),
        "inspect-written".to_owned(),
    ];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_numeric_plan_ops.bn"),
        Path::new("../../examples/bytes_numeric_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("Bytes/numeric root scalar fixture should execute through PlanExecutor");

    let written_unsigned_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "c76b0857a5d51757ff556afe25625f9224c305e4960e24bd5dc25451cb460093",
        "byte_len": 8
    });
    let written_signed_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "080aff601f9266cd9cc3f3c86e0add56283d3635a0aa200f3801cf7bda944bec",
        "byte_len": 8
    });
    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.state_summary["store.read_u16_le"], 513);
    assert_eq!(output.state_summary["store.read_u16_be"], 258);
    assert_eq!(output.state_summary["store.read_i16_be"], -2);
    assert_eq!(output.state_summary["store.read_i8"], -128);
    assert_eq!(
        output.state_summary["store.written_unsigned"],
        written_unsigned_summary
    );
    assert_eq!(
        output.state_summary["store.written_signed"],
        written_signed_summary
    );
    assert_eq!(
        output.state_summary["store.written_unsigned_hex"],
        "0102fffe7f801234"
    );
    assert_eq!(
        output.state_summary["store.written_signed_hex"],
        "0102fffe7fff0010"
    );
    assert_eq!(output.state_summary["store.written_unsigned_read"], 4660);
    assert_eq!(output.state_summary["store.written_signed_read"], -129);
    assert_root_scenario_product_only(&output.report);
    assert_eq!(
        output.report["plan_executor"]["executed_update_branch_count"],
        10
    );

    let measure_updates = output.report["plan_executor"]["per_step"][0]["updates"]
        .as_array()
        .expect("measure step should expose updates");
    let measure_update_for = |target: &str| {
        measure_updates
            .iter()
            .find(|update| update["target_state"] == target)
            .unwrap_or_else(|| panic!("missing measure update for {target}: {measure_updates:#?}"))
    };
    assert_eq!(
        measure_update_for("store.read_u16_le")["expression_kind"],
        "bytes_read_unsigned"
    );
    assert_eq!(
        measure_update_for("store.read_u16_le")["executor_core"]["executor"],
        "cpu-plan-root-bytes-read-evaluator-v1"
    );
    assert_eq!(measure_update_for("store.read_u16_le")["value"], 513);
    assert_eq!(
        measure_update_for("store.read_u16_le")["update_constant_value"],
        json!({"offset": 0, "byte_count": 2, "endian": "Little"})
    );
    assert_eq!(
        measure_update_for("store.read_u16_be")["update_constant_value"],
        json!({"offset": 0, "byte_count": 2, "endian": "Big"})
    );
    assert_eq!(
        measure_update_for("store.read_i16_be")["expression_kind"],
        "bytes_read_signed"
    );
    assert_eq!(
        measure_update_for("store.read_i16_be")["executor_core"]["executor"],
        "cpu-plan-root-bytes-read-evaluator-v1"
    );
    assert_eq!(measure_update_for("store.read_i16_be")["value"], -2);
    assert_eq!(
        measure_update_for("store.read_i8")["update_constant_value"],
        json!({"offset": 5, "byte_count": 1, "endian": "Little"})
    );

    let write_updates = output.report["plan_executor"]["per_step"][1]["updates"]
        .as_array()
        .expect("write step should expose updates");
    let write_update_for = |target: &str| {
        write_updates
            .iter()
            .find(|update| update["target_state"] == target)
            .unwrap_or_else(|| panic!("missing write update for {target}: {write_updates:#?}"))
    };
    assert_eq!(
        write_update_for("store.written_unsigned")["expression_kind"],
        "bytes_write_unsigned"
    );
    assert_eq!(
        write_update_for("store.written_unsigned")["executor_core"]["executor"],
        "cpu-plan-root-bytes-write-evaluator-v1"
    );
    assert_eq!(
        write_update_for("store.written_unsigned")["value"],
        written_unsigned_summary
    );
    assert_eq!(
        write_update_for("store.written_unsigned")["update_constant_value"],
        json!({"offset": 6, "byte_count": 2, "endian": "Big", "value": 4660})
    );
    assert_eq!(
        write_update_for("store.written_signed")["expression_kind"],
        "bytes_write_signed"
    );
    assert_eq!(
        write_update_for("store.written_signed")["executor_core"]["executor"],
        "cpu-plan-root-bytes-write-evaluator-v1"
    );
    assert_eq!(
        write_update_for("store.written_signed")["value"],
        written_signed_summary
    );
    assert_eq!(
        write_update_for("store.written_signed")["update_constant_value"],
        json!({"offset": 4, "byte_count": 2, "endian": "Little", "value": -129})
    );
    for update in write_updates {
        assert!(
            update["value"].get("inline_bytes").is_none(),
            "public BYTES summaries must not expose inline bytes"
        );
    }

    let inspect_updates = output.report["plan_executor"]["per_step"][2]["updates"]
        .as_array()
        .expect("inspect step should expose updates");
    let inspect_update_for = |target: &str| {
        inspect_updates
            .iter()
            .find(|update| update["target_state"] == target)
            .unwrap_or_else(|| panic!("missing inspect update for {target}: {inspect_updates:#?}"))
    };
    assert_eq!(
        inspect_update_for("store.written_unsigned_hex")["value"],
        "0102fffe7f801234"
    );
    assert_eq!(
        inspect_update_for("store.written_signed_hex")["value"],
        "0102fffe7fff0010"
    );
    assert_eq!(
        inspect_update_for("store.written_unsigned_read")["expression_kind"],
        "bytes_read_unsigned"
    );
    assert_eq!(
        inspect_update_for("store.written_unsigned_read")["executor_core"]["executor"],
        "cpu-plan-root-bytes-read-evaluator-v1"
    );
    assert_eq!(
        inspect_update_for("store.written_unsigned_read")["value"],
        4660
    );
    assert_eq!(
        inspect_update_for("store.written_signed_read")["expression_kind"],
        "bytes_read_signed"
    );
    assert_eq!(
        inspect_update_for("store.written_signed_read")["executor_core"]["executor"],
        "cpu-plan-root-bytes-read-evaluator-v1"
    );
    assert_eq!(
        inspect_update_for("store.written_signed_read")["value"],
        -129
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

// test: root_scalar_plan_executor_replays_bytes_concat_update
#[test]
fn root_scalar_plan_executor_replays_bytes_concat_update() {
    let steps = vec!["measure-bytes".to_owned()];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_concat_plan_ops.bn"),
        Path::new("../../examples/bytes_concat_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("Bytes/concat root scalar fixture should execute through PlanExecutor");

    let expected_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "4fdcf430d9a09a049ecb6b373b31776fa149c6b4fd54d6229534c8f971c29b89",
        "byte_len": 5
    });
    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.state_summary["store.joined_pipe"], expected_summary);
    assert_eq!(output.state_summary["store.joined_call"], expected_summary);
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

    let updates = output.report["plan_executor"]["per_step"][0]["updates"]
        .as_array()
        .expect("PlanExecutor report should expose update array");
    let update_for = |target: &str| {
        updates
            .iter()
            .find(|update| update["target_state"] == target)
            .unwrap_or_else(|| panic!("missing update for {target}: {updates:#?}"))
    };
    for target in ["store.joined_pipe", "store.joined_call"] {
        let update = update_for(target);
        assert_eq!(update["expression_kind"], "bytes_concat");
        assert_eq!(update["value"], expected_summary);
        assert_eq!(update["source_payload_field"], JsonValue::Null);
        assert_eq!(update["update_constant_id"], JsonValue::Null);
        assert_eq!(update["update_constant_value"], JsonValue::Null);
        assert_eq!(update["selected_op_indexed"], false);
        assert_eq!(update["selected_op_unresolved_executable_ref_count"], 0);
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

// test: root_scalar_plan_executor_replays_chained_bytes_concat_state
#[test]
fn root_scalar_plan_executor_replays_chained_bytes_concat_state() {
    let steps = vec!["join-bytes".to_owned(), "inspect-joined".to_owned()];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_concat_chain_plan_ops.bn"),
        Path::new("../../examples/bytes_concat_chain_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("chained BYTES fixture should execute through PlanExecutor");

    let joined_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "4fdcf430d9a09a049ecb6b373b31776fa149c6b4fd54d6229534c8f971c29b89",
        "byte_len": 5
    });
    let joined_again_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "a9290f6579e270419a97e9c69d022898df14bb2bfb3db13d9729b45d1862443a",
        "byte_len": 7
    });

    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.state_summary["store.joined"], joined_summary);
    assert_eq!(output.state_summary["store.joined_len"], 5);
    assert_eq!(output.state_summary["store.joined_byte"], 254);
    assert_eq!(
        output.state_summary["store.joined_again"],
        joined_again_summary
    );
    assert_root_scenario_product_only(&output.report);
    assert_eq!(
        output.report["plan_executor"]["executed_update_branch_count"],
        4
    );

    let first_step_updates = output.report["plan_executor"]["per_step"][0]["updates"]
        .as_array()
        .expect("first step should expose updates");
    assert_eq!(first_step_updates.len(), 1);
    assert_eq!(first_step_updates[0]["target_state"], "store.joined");
    assert_eq!(first_step_updates[0]["expression_kind"], "bytes_concat");
    assert_eq!(first_step_updates[0]["value"], joined_summary);

    let second_step_updates = output.report["plan_executor"]["per_step"][1]["updates"]
        .as_array()
        .expect("second step should expose updates");
    let update_for = |target: &str| {
        second_step_updates
            .iter()
            .find(|update| update["target_state"] == target)
            .unwrap_or_else(|| panic!("missing update for {target}: {second_step_updates:#?}"))
    };
    assert_eq!(
        update_for("store.joined_len")["expression_kind"],
        "bytes_length"
    );
    assert_eq!(update_for("store.joined_len")["value"], 5);
    assert_eq!(
        update_for("store.joined_byte")["expression_kind"],
        "bytes_get"
    );
    assert_eq!(update_for("store.joined_byte")["value"], 254);
    assert_eq!(
        update_for("store.joined_again")["expression_kind"],
        "bytes_concat"
    );
    assert_eq!(
        update_for("store.joined_again")["value"],
        joined_again_summary
    );

    for value in [
        &output.state_summary["store.joined"],
        &output.state_summary["store.joined_again"],
        &first_step_updates[0]["value"],
        &update_for("store.joined_again")["value"],
    ] {
        assert!(
            value.get("inline_bytes").is_none(),
            "public BYTES summaries must not expose inline bytes: {value:#?}"
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

// test: root_scalar_plan_executor_replays_text_bytes_conversion_chain
#[test]
fn root_scalar_plan_executor_replays_text_bytes_conversion_chain() {
    let steps = vec!["encode-text".to_owned(), "decode-bytes".to_owned()];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_text_conversion_plan_ops.bn"),
        Path::new("../../examples/bytes_text_conversion_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("text/BYTES conversion fixture should execute through PlanExecutor");

    let encoded_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "8f434346648f6b96df89dda901c5176b10a6d83961dd3c1ac88b59b2dc327aa4",
        "byte_len": 2
    });

    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.state_summary["store.encoded"], encoded_summary);
    assert_eq!(output.state_summary["store.encoded_len"], 2);
    assert_eq!(output.state_summary["store.decoded"], "hi");
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
        3
    );

    let first_step_updates = output.report["plan_executor"]["per_step"][0]["updates"]
        .as_array()
        .expect("first step should expose updates");
    assert_eq!(first_step_updates.len(), 1);
    let encode_update = &first_step_updates[0];
    assert_eq!(encode_update["target_state"], "store.encoded");
    assert_eq!(encode_update["expression_kind"], "text_to_bytes");
    assert_eq!(
        encode_update["executor_core"]["executor"],
        "cpu-plan-root-bytes-write-evaluator-v1"
    );
    assert_eq!(encode_update["value"], encoded_summary);
    assert!(encode_update["update_constant_id"].is_number());
    assert_eq!(encode_update["update_constant_value"], "utf8");
    assert_eq!(encode_update["source_payload_field"], JsonValue::Null);
    assert!(
        encode_update["value"].get("inline_bytes").is_none(),
        "public Text/to_bytes update summary must not expose inline bytes"
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
    let len_update = update_for("store.encoded_len");
    assert_eq!(len_update["expression_kind"], "bytes_length");
    assert_eq!(len_update["value"], 2);
    let decode_update = update_for("store.decoded");
    assert_eq!(decode_update["expression_kind"], "bytes_to_text");
    assert_eq!(
        decode_update["executor_core"]["executor"],
        "cpu-plan-root-bytes-read-evaluator-v1"
    );
    assert_eq!(decode_update["value"], "hi");
    assert!(decode_update["update_constant_id"].is_number());
    assert_eq!(decode_update["update_constant_value"], "utf8");
    assert_eq!(decode_update["source_payload_field"], JsonValue::Null);

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

// test: root_scalar_plan_executor_replays_bytes_slice_take_drop_chain
#[test]
fn root_scalar_plan_executor_replays_bytes_slice_take_drop_chain() {
    let steps = vec!["split-bytes".to_owned(), "inspect-splits".to_owned()];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_slice_take_drop_plan_ops.bn"),
        Path::new("../../examples/bytes_slice_take_drop_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("slice/take/drop BYTES fixture should execute through PlanExecutor");

    let sliced_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "56e75135f0ade48aa22e0f12a1b8dbdacf1eacadc82f5af7c1b46757dc4dd697",
        "byte_len": 3
    });
    let taken_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "a00291e8229d191815f2cbd2aa49d3b783585deae8012ca5941f011ceb9eb119",
        "byte_len": 4
    });
    let dropped_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "69694cc0dd6646b54f6934e4d2778f79e94e54a2d9ca44ed87799cc5db400b6a",
        "byte_len": 4
    });
    let joined_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "a499a56927c382e4ada0f391b7f618af78f16063a72fd0f222837e80c14fbde9",
        "byte_len": 7
    });

    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.state_summary["store.sliced"], sliced_summary);
    assert_eq!(output.state_summary["store.taken"], taken_summary);
    assert_eq!(output.state_summary["store.dropped"], dropped_summary);
    assert_eq!(output.state_summary["store.sliced_len"], 3);
    assert_eq!(output.state_summary["store.taken_byte"], 19);
    assert_eq!(output.state_summary["store.dropped_joined"], joined_summary);
    assert_root_scenario_product_only(&output.report);
    assert_eq!(
        output.report["plan_executor"]["executed_update_branch_count"],
        6
    );
    assert_eq!(
        output.report["plan_executor"]["per_step"][0]["bytes_storage_no_copy"], true,
        "slice/take/drop step should reuse Bytes views without measured byte-buffer copies"
    );
    for key in [
        "copy_from_slice_bytes",
        "vec_clone_bytes",
        "vec_alloc_bytes",
        "zero_fill_bytes",
    ] {
        assert_eq!(
            output.report["plan_executor"]["per_step"][0]["bytes_storage_counters"][key], 0,
            "first slice/take/drop step counter {key} must stay zero"
        );
    }
    assert_eq!(
        output.report["plan_executor"]["per_step"][1]["bytes_storage_no_copy"], false,
        "concat step should report measured byte-buffer copy cost"
    );
    assert_eq!(
        output.report["plan_executor"]["bytes_storage_no_copy"], false,
        "overall two-step fixture should report the measured concat copy cost"
    );
    for key in ["copy_from_slice_bytes", "vec_alloc_bytes"] {
        assert!(
            output.report["plan_executor"]["bytes_storage_counters"][key]
                .as_u64()
                .unwrap_or_default()
                > 0,
            "overall counter {key} should include executor-owned concat copy cost"
        );
    }

    let first_step_updates = output.report["plan_executor"]["per_step"][0]["updates"]
        .as_array()
        .expect("first step should expose updates");
    let second_step_updates = output.report["plan_executor"]["per_step"][1]["updates"]
        .as_array()
        .expect("second step should expose updates");
    let update_index_for = |updates: &[JsonValue], target: &str| {
        updates
            .iter()
            .position(|update| update["target_state"] == target)
            .unwrap_or_else(|| panic!("missing update for {target}: {updates:#?}"))
    };
    let slice_index = update_index_for(first_step_updates, "store.sliced");
    let slice_update = &first_step_updates[slice_index];
    assert_eq!(slice_update["expression_kind"], "bytes_slice");
    assert_eq!(slice_update["value"], sliced_summary);
    assert_eq!(
        slice_update["update_constant_value"],
        json!({"offset": 1, "byte_count": 3})
    );
    assert!(slice_update["update_constant_id"].is_array());
    assert_eq!(
        first_step_updates[update_index_for(first_step_updates, "store.taken")]["expression_kind"],
        "bytes_take"
    );
    assert_eq!(
        first_step_updates[update_index_for(first_step_updates, "store.dropped")]["expression_kind"],
        "bytes_drop"
    );
    assert_eq!(
        second_step_updates[update_index_for(second_step_updates, "store.sliced_len")]["expression_kind"],
        "bytes_length"
    );
    assert_eq!(
        second_step_updates[update_index_for(second_step_updates, "store.taken_byte")]["expression_kind"],
        "bytes_get"
    );
    let joined_index = update_index_for(second_step_updates, "store.dropped_joined");
    assert_eq!(
        second_step_updates[joined_index]["expression_kind"],
        "bytes_concat"
    );
    assert_eq!(second_step_updates[joined_index]["value"], joined_summary);

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

// test: root_scalar_plan_executor_replays_chained_bytes_set_state
#[test]
fn root_scalar_plan_executor_replays_chained_bytes_set_state() {
    let steps = vec!["patch-bytes".to_owned(), "inspect-patched".to_owned()];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_set_plan_ops.bn"),
        Path::new("../../examples/bytes_set_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("chained Bytes/set fixture should execute through PlanExecutor");

    let patched_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "9c5ec9570d8bf71994dfc97037803d9a280ba40f6dab32591c949310e0d9fdd8",
        "byte_len": 4
    });
    let joined_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "de71c0407a4cf40db6a195c3d88787156c590b5643bbf50c758660c71189c91e",
        "byte_len": 6
    });

    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.state_summary["store.patched"], patched_summary);
    assert_eq!(output.state_summary["store.patched_byte"], 170);
    assert_eq!(output.state_summary["store.patched_len"], 4);
    assert_eq!(output.state_summary["store.patched_joined"], joined_summary);
    assert_root_scenario_product_only(&output.report);
    assert_eq!(
        output.report["plan_executor"]["executed_update_branch_count"],
        4
    );

    let first_step_updates = output.report["plan_executor"]["per_step"][0]["updates"]
        .as_array()
        .expect("first step should expose updates");
    assert_eq!(first_step_updates.len(), 1);
    let set_update = &first_step_updates[0];
    assert_eq!(set_update["target_state"], "store.patched");
    assert_eq!(set_update["expression_kind"], "bytes_set");
    assert_eq!(set_update["value"], patched_summary);
    assert!(set_update["update_constant_id"].is_array());
    assert_eq!(
        set_update["update_constant_value"],
        json!({"index": 2, "value": 170})
    );
    assert_eq!(
        output.report["plan_executor"]["per_step"][0]["bytes_storage_no_copy"], true,
        "fixed Bytes/set should mutate the preallocated root byte bank without measured byte-buffer allocation"
    );
    for key in [
        "copy_from_slice_bytes",
        "vec_clone_bytes",
        "vec_alloc_bytes",
        "zero_fill_bytes",
    ] {
        assert_eq!(
            output.report["plan_executor"]["per_step"][0]["bytes_storage_counters"][key], 0,
            "fixed Bytes/set measured tick should keep {key}=0"
        );
    }

    let second_step_updates = output.report["plan_executor"]["per_step"][1]["updates"]
        .as_array()
        .expect("second step should expose updates");
    let update_for = |target: &str| {
        second_step_updates
            .iter()
            .find(|update| update["target_state"] == target)
            .unwrap_or_else(|| panic!("missing update for {target}: {second_step_updates:#?}"))
    };
    assert_eq!(
        update_for("store.patched_byte")["expression_kind"],
        "bytes_get"
    );
    assert_eq!(update_for("store.patched_byte")["value"], 170);
    assert_eq!(
        update_for("store.patched_len")["expression_kind"],
        "bytes_length"
    );
    assert_eq!(update_for("store.patched_len")["value"], 4);
    assert_eq!(
        update_for("store.patched_joined")["expression_kind"],
        "bytes_concat"
    );
    assert_eq!(update_for("store.patched_joined")["value"], joined_summary);

    for value in [
        &output.state_summary["store.patched"],
        &output.state_summary["store.patched_joined"],
        &set_update["value"],
        &update_for("store.patched_joined")["value"],
    ] {
        assert!(
            value.get("inline_bytes").is_none(),
            "public BYTES summaries must not expose inline bytes: {value:#?}"
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

// test: root_scalar_plan_executor_replays_same_event_bytes_dependency
#[test]
fn root_scalar_plan_executor_replays_same_event_bytes_dependency() {
    let steps = vec!["patch-and-read-bytes".to_owned()];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_same_event_dependency_plan_ops.bn"),
        Path::new("../../examples/bytes_same_event_dependency_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("same-event Bytes/set dependency fixture should execute through PlanExecutor");

    let patched_summary = json!({
        "$boon_type": "BYTES",
        "storage": "inline",
        "digest": "9c5ec9570d8bf71994dfc97037803d9a280ba40f6dab32591c949310e0d9fdd8",
        "byte_len": 4
    });

    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.state_summary["store.patched"], patched_summary);
    assert_eq!(output.state_summary["store.patched_byte"], 170);
    assert_eq!(output.state_summary["store.patched_len"], 4);
    assert_root_scenario_product_only(&output.report);

    let updates = output.report["plan_executor"]["per_step"][0]["updates"]
        .as_array()
        .expect("same-event step should expose updates");
    let update_for = |target: &str| {
        updates
            .iter()
            .find(|update| update["target_state"] == target)
            .unwrap_or_else(|| panic!("missing update for {target}: {updates:#?}"))
    };
    assert_eq!(updates.len(), 3);
    assert_eq!(update_for("store.patched")["expression_kind"], "bytes_set");
    assert_eq!(update_for("store.patched")["value"], patched_summary);
    assert_eq!(update_for("store.patched_byte")["value"], 170);
    assert_eq!(update_for("store.patched_len")["value"], 4);
    assert_eq!(
        output.report["plan_executor"]["per_step"][0]["bytes_storage_no_copy"], true,
        "same-event fixed-bank Bytes/set/read path should avoid measured byte-buffer copies"
    );
    for key in [
        "copy_from_slice_bytes",
        "vec_clone_bytes",
        "vec_alloc_bytes",
        "zero_fill_bytes",
    ] {
        assert_eq!(
            output.report["plan_executor"]["per_step"][0]["bytes_storage_counters"][key], 0,
            "same-event fixed-bank dependency tick should keep {key}=0"
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
    for value in [
        &output.state_summary["store.patched"],
        &update_for("store.patched")["value"],
    ] {
        assert!(
            value.get("inline_bytes").is_none(),
            "public BYTES summaries must not expose inline bytes: {value:#?}"
        );
    }
}

