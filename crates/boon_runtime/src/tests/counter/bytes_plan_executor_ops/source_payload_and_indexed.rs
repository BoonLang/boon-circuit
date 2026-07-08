// Included by `../counter.rs`.

// test: root_scalar_plan_executor_replays_bytes_length_update
#[test]
fn root_scalar_plan_executor_replays_bytes_length_update() {
    let steps = vec!["measure-bytes".to_owned()];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_length_plan_ops.bn"),
        Path::new("../../examples/bytes_length_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("Bytes/length root scalar fixture should execute through PlanExecutor");

    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.state_summary["store.byte_len"], 4);
    assert_eq!(
        output.state_summary["payload"]["digest"],
        "9f64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a"
    );
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
    assert_eq!(
        output.report["plan_executor"]["per_step"][0]["updates"][0]["expression_kind"],
        "bytes_length"
    );
    assert_eq!(
        output.report["plan_executor"]["per_step"][0]["updates"][0]["value"],
        4
    );
    assert_eq!(
        output.report["plan_executor"]["bytes_storage_no_copy"], true,
        "Bytes/length should only read fixed inline bytes during the measured tick"
    );
    assert_eq!(
        output.report["plan_executor"]["bytes_storage_counters"]["copy_from_slice_bytes"],
        0
    );
    assert_eq!(
        output.report["plan_executor"]["bytes_storage_counters"]["vec_clone_bytes"],
        0
    );
    assert_eq!(
        output.report["plan_executor"]["bytes_storage_counters"]["vec_alloc_bytes"],
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

// test: root_scalar_plan_executor_replays_bytes_source_payload_update
#[test]
fn root_scalar_plan_executor_replays_bytes_source_payload_update() {
    let steps = vec!["receive-bytes".to_owned(), "inspect-bytes".to_owned()];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_source_payload_plan_ops.bn"),
        Path::new("../../examples/bytes_source_payload_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("BYTES source payload fixture should execute through PlanExecutor");

    assert_eq!(output.report["status"], "pass");
    assert_eq!(output.state_summary["store.received_len"], 3);
    assert_eq!(output.state_summary["store.received_byte"], 254);
    assert_eq!(
        output.state_summary["store.received"]["digest"],
        "2096dcbc716cabc261901f6c15ce242d3bb589284cf8e97ba196710211d7e99a"
    );
    assert_root_scenario_product_only(&output.report);
    assert_eq!(
        output.report["plan_executor"]["executed_update_branch_count"],
        3
    );
    let receive_update = &output.report["plan_executor"]["per_step"][0]["updates"][0];
    assert_eq!(receive_update["expression_kind"], "source_payload");
    assert_eq!(receive_update["source_payload_field"], "Bytes");
    assert_eq!(
        output.report["semantic_delta_signatures"],
        json!([
            "FieldSet:store.received",
            "FieldSet:store.received_len",
            "FieldSet:store.received_byte"
        ])
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

// test: root_scalar_plan_executor_replays_indexed_bytes_source_payload_update
#[test]
fn root_scalar_plan_executor_replays_indexed_bytes_source_payload_update() {
    let steps = vec![
        "receive-beta-bytes".to_owned(),
        "inspect-beta-bytes".to_owned(),
    ];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_indexed_source_payload_plan_ops.bn"),
        Path::new("../../examples/bytes_indexed_source_payload_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("indexed BYTES source payload fixture should execute through PlanExecutor");

    assert_eq!(output.report["status"], "pass");
    assert_root_scenario_product_only(&output.report);
    assert_eq!(
        output.report["plan_executor"]["executed_indexed_update_count"],
        3
    );
    let indexed_update = &output.report["plan_executor"]["per_step"][0]["indexed_updates"][0];
    assert_eq!(indexed_update["expression_kind"], "source_payload");
    assert_eq!(indexed_update["source_payload_field"], "Bytes");
    assert_eq!(indexed_update["key"], 2);
    assert_eq!(indexed_update["field_path"], "payload");
    assert_eq!(
        indexed_update["value"]["digest"],
        "2096dcbc716cabc261901f6c15ce242d3bb589284cf8e97ba196710211d7e99a"
    );
    assert_eq!(
        indexed_update["bytes_storage"]["storage"],
        "indexed_fixed_byte_bank"
    );
    assert_eq!(indexed_update["bytes_storage"]["byte_bank_used"], true);
    assert_eq!(indexed_update["bytes_storage"]["byte_len"], 3);
    assert_eq!(
        output.report["semantic_delta_signatures"],
        json!([
            "FieldSet:payload",
            "FieldSet:payload_len",
            "FieldSet:payload_second"
        ])
    );
    let inspect_updates = output.report["plan_executor"]["per_step"][1]["indexed_updates"]
        .as_array()
        .expect("inspect step should update indexed row BYTES projections");
    assert_eq!(inspect_updates.len(), 2);
    assert!(inspect_updates.iter().any(|update| {
        update["expression_kind"] == "bytes_length"
            && update["field_path"] == "payload_len"
            && update["value"] == json!(3)
            && update["executor_core"]["executor"] == "cpu-plan-indexed-bytes-read-evaluator-v1"
    }));
    assert!(inspect_updates.iter().any(|update| {
        update["expression_kind"] == "bytes_get"
            && update["field_path"] == "payload_second"
            && update["value"] == json!(254)
            && update["executor_core"]["executor"] == "cpu-plan-indexed-bytes-read-evaluator-v1"
    }));
    let rows = output.report["plan_executor"]["list_summary"]["rows"]["rows"]
        .as_array()
        .expect("indexed BYTES fixture should report rows");
    let beta = rows
        .iter()
        .find(|row| row["key"] == json!(2))
        .expect("beta row should be present in PlanExecutor list summary");
    assert_eq!(beta["fields"]["payload_len"], json!(3));
    assert_eq!(beta["fields"]["payload_second"], json!(254));
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

// test: root_scalar_plan_executor_replays_indexed_bytes_equal_update
#[test]
fn root_scalar_plan_executor_replays_indexed_bytes_equal_update() {
    let steps = vec![
        "receive-beta-bytes".to_owned(),
        "inspect-beta-equal".to_owned(),
    ];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_indexed_equal_plan_ops.bn"),
        Path::new("../../examples/bytes_indexed_equal_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("indexed Bytes/equal fixture should execute through PlanExecutor");

    assert_eq!(output.report["status"], "pass");
    assert_root_scenario_product_only(&output.report);
    assert_eq!(
        output.report["plan_executor"]["executed_indexed_update_count"],
        3
    );

    let inspect_updates = output.report["plan_executor"]["per_step"][1]["indexed_updates"]
        .as_array()
        .expect("inspect step should expose indexed Bytes/equal updates");
    assert_eq!(inspect_updates.len(), 2);
    let same_update = inspect_updates
        .iter()
        .find(|update| update["field_path"] == "payload_matches")
        .expect("payload_matches update should execute");
    assert_eq!(same_update["expression_kind"], "bytes_equal");
    assert_eq!(same_update["value"], true);
    assert_eq!(
        same_update["executor_core"]["executor"],
        "cpu-plan-indexed-bytes-read-evaluator-v1"
    );
    assert_eq!(same_update["bytes_access"]["read_only"], true);
    assert_eq!(
        same_update["bytes_access"]["inputs"][0]["access_source"],
        "indexed_fixed_byte_bank"
    );
    assert_eq!(
        same_update["bytes_access"]["inputs"][1]["access_source"],
        "indexed_row_private_bytes"
    );
    assert_eq!(
        same_update["bytes_access"]["inputs"][0]["byte_bank_used"],
        true
    );
    assert_eq!(
        same_update["bytes_access"]["inputs"][1]["byte_bank_used"],
        false
    );

    let different_update = inspect_updates
        .iter()
        .find(|update| update["field_path"] == "payload_differs")
        .expect("payload_differs update should execute");
    assert_eq!(different_update["expression_kind"], "bytes_equal");
    assert_eq!(different_update["value"], false);
    assert_eq!(
        different_update["executor_core"]["executor"],
        "cpu-plan-indexed-bytes-read-evaluator-v1"
    );

    let rows = output.report["plan_executor"]["list_summary"]["rows"]["rows"]
        .as_array()
        .expect("indexed Bytes/equal fixture should report rows");
    let beta = rows
        .iter()
        .find(|row| row["key"] == json!(2))
        .expect("beta row should be present in PlanExecutor list summary");
    assert_eq!(beta["fields"]["payload_matches"], true);
    assert_eq!(beta["fields"]["payload_differs"], false);
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

// test: root_scalar_plan_executor_replays_indexed_bytes_search_updates
#[test]
fn root_scalar_plan_executor_replays_indexed_bytes_search_updates() {
    let steps = vec![
        "receive-beta-bytes".to_owned(),
        "inspect-beta-search".to_owned(),
    ];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_indexed_search_plan_ops.bn"),
        Path::new("../../examples/bytes_indexed_search_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("indexed Bytes search fixture should execute through PlanExecutor");

    assert_eq!(output.report["status"], "pass");
    assert_root_scenario_product_only(&output.report);
    assert_eq!(
        output.report["plan_executor"]["executed_indexed_update_count"],
        22
    );

    let inspect_updates = output.report["plan_executor"]["per_step"][1]["indexed_updates"]
        .as_array()
        .expect("inspect step should expose indexed Bytes search updates");
    assert_eq!(inspect_updates.len(), 21);
    let expected_text = [
        ("payload_hex", "bytes_to_hex", json!("01fe0405ff")),
        ("payload_base64", "bytes_to_base64", json!("Af4EBf8=")),
        ("decoded_text", "bytes_to_text", json!("ABC")),
    ];
    for (field_path, expression_kind, value) in expected_text {
        let update = inspect_updates
            .iter()
            .find(|update| update["field_path"] == field_path)
            .unwrap_or_else(|| panic!("{field_path} update should execute"));
        assert_eq!(update["expression_kind"], expression_kind);
        assert_eq!(update["value"], value);
        assert_eq!(
            update["executor_core"]["executor"],
            "cpu-plan-indexed-bytes-read-evaluator-v1"
        );
        assert_eq!(update["bytes_access"]["read_only"], true);
    }

    let expected_bytes_digest = "b5d4045c3f466fa91fe2cc6abe79232a1a57cdf104f7a26e716e0a1e2789df78";
    let expected_bytes = [
        ("encoded_text_bytes", "text_to_bytes", "row.text_source"),
        ("decoded_hex_bytes", "bytes_from_hex", "row.hex_source"),
        (
            "decoded_base64_bytes",
            "bytes_from_base64",
            "row.base64_source",
        ),
    ];
    for (field_path, expression_kind, input_field) in expected_bytes {
        let update = inspect_updates
            .iter()
            .find(|update| update["field_path"] == field_path)
            .unwrap_or_else(|| panic!("{field_path} update should execute"));
        assert_eq!(update["expression_kind"], expression_kind);
        assert_eq!(update["value"]["byte_len"], json!(3));
        assert_eq!(update["value"]["digest"], expected_bytes_digest);
        assert_eq!(
            update["executor_core"]["executor"],
            "cpu-plan-indexed-bytes-write-evaluator-v1"
        );
        assert_eq!(update["bytes_access"]["read_only"], true);
        assert_eq!(update["bytes_access"]["input"]["input_field"], input_field);
        assert_eq!(
            update["bytes_storage"]["storage"],
            "indexed_fixed_byte_bank"
        );
        assert_eq!(update["bytes_storage"]["byte_bank_used"], true);
        assert_eq!(update["bytes_storage"]["byte_len"], json!(3));
    }

    let expected_slices = [
        (
            "payload_mid",
            "bytes_slice",
            2,
            "9df02aeff60f85979be4737edbcd69757628a81b9b1201a48c64ab6b2eb18126",
            json!({"offset": 1, "byte_count": 2}),
        ),
        (
            "payload_take",
            "bytes_take",
            2,
            "6077f477043ae8cefee8bd0f88b7db444863c754a0fb128ecec260de45f50b4e",
            json!(2),
        ),
        (
            "payload_drop",
            "bytes_drop",
            3,
            "99c24dfd75d0145b9fe75fa80f6e4fcae7066f24a3f91195c675a849db264541",
            json!(2),
        ),
    ];
    for (field_path, expression_kind, byte_len, digest, update_constant_value) in expected_slices {
        let update = inspect_updates
            .iter()
            .find(|update| update["field_path"] == field_path)
            .unwrap_or_else(|| panic!("{field_path} update should execute"));
        assert_eq!(update["expression_kind"], expression_kind);
        assert_eq!(update["value"]["byte_len"], json!(byte_len));
        assert_eq!(update["value"]["digest"], digest);
        assert_eq!(update["update_constant_value"], update_constant_value);
        assert_eq!(
            update["executor_core"]["executor"],
            "cpu-plan-indexed-bytes-write-evaluator-v1"
        );
        assert_eq!(update["bytes_access"]["read_only"], false);
        assert_eq!(
            update["bytes_access"]["input_access_source"],
            "indexed_fixed_byte_bank"
        );
        assert_eq!(
            update["bytes_access"]["output_storage_kind"],
            "bytes_slice_view"
        );
        assert_eq!(
            update["bytes_storage"]["storage"],
            "indexed_fixed_byte_bank"
        );
        assert_eq!(update["bytes_storage"]["byte_bank_used"], true);
        assert_eq!(update["bytes_storage"]["byte_len"], json!(byte_len));
    }

    let zeros_update = inspect_updates
        .iter()
        .find(|update| update["field_path"] == "zero_fill")
        .expect("zero_fill update should execute");
    assert_eq!(zeros_update["expression_kind"], "bytes_zeros");
    assert_eq!(zeros_update["value"]["byte_len"], json!(3));
    assert_eq!(
        zeros_update["value"]["digest"],
        "709e80c88487a2411e1ee4dfb9f22a861492d20c4765150c0c794abd70f8147c"
    );
    assert!(zeros_update["update_constant_id"].is_number());
    assert_eq!(zeros_update["update_constant_value"], json!(3));
    assert_eq!(
        zeros_update["executor_core"]["executor"],
        "cpu-plan-indexed-bytes-write-evaluator-v1"
    );
    assert_eq!(
        zeros_update["bytes_storage"]["storage"],
        "indexed_fixed_byte_bank"
    );
    assert_eq!(zeros_update["bytes_storage"]["byte_bank_used"], true);
    assert_eq!(zeros_update["bytes_storage"]["byte_len"], json!(3));

    let concat_update = inspect_updates
        .iter()
        .find(|update| update["field_path"] == "payload_joined")
        .expect("payload_joined update should execute");
    assert_eq!(concat_update["expression_kind"], "bytes_concat");
    assert_eq!(concat_update["value"]["byte_len"], json!(7));
    assert_eq!(
        concat_update["value"]["digest"],
        "4825013d9d795bb73ad5e21ce6963c7c858ed984a8fa720a1d393554a04f43ee"
    );
    assert_eq!(
        concat_update["executor_core"]["executor"],
        "cpu-plan-indexed-bytes-write-evaluator-v1"
    );
    assert_eq!(
        concat_update["executor_core"]["bytes_copy_cost"]["reason"],
        "bytes_concat_output_vec"
    );
    assert_eq!(concat_update["bytes_access"]["read_only"], false);
    assert_eq!(concat_update["bytes_access"]["inputs"][0]["role"], "left");
    assert_eq!(concat_update["bytes_access"]["inputs"][1]["role"], "right");
    assert_eq!(
        concat_update["bytes_storage"]["storage"],
        "indexed_fixed_byte_bank"
    );
    assert_eq!(concat_update["bytes_storage"]["byte_bank_used"], true);
    assert_eq!(concat_update["bytes_storage"]["byte_len"], json!(7));

    let read_u16_update = inspect_updates
        .iter()
        .find(|update| update["field_path"] == "read_u16_be")
        .expect("read_u16_be update should execute");
    assert_eq!(read_u16_update["expression_kind"], "bytes_read_unsigned");
    assert_eq!(read_u16_update["value"], json!(65028));
    assert_eq!(
        read_u16_update["update_constant_value"],
        json!({"offset": 1, "byte_count": 2, "endian": "Big"})
    );
    assert_eq!(
        read_u16_update["executor_core"]["executor"],
        "cpu-plan-indexed-bytes-read-evaluator-v1"
    );
    assert_eq!(read_u16_update["bytes_access"]["read_only"], true);
    assert_eq!(
        read_u16_update["bytes_access"]["access_source"],
        "indexed_fixed_byte_bank"
    );

    let read_i8_update = inspect_updates
        .iter()
        .find(|update| update["field_path"] == "read_i8_last")
        .expect("read_i8_last update should execute");
    assert_eq!(read_i8_update["expression_kind"], "bytes_read_signed");
    assert_eq!(read_i8_update["value"], json!(-1));
    assert_eq!(
        read_i8_update["update_constant_value"],
        json!({"offset": 4, "byte_count": 1, "endian": "Little"})
    );
    assert_eq!(
        read_i8_update["executor_core"]["executor"],
        "cpu-plan-indexed-bytes-read-evaluator-v1"
    );
    assert_eq!(read_i8_update["bytes_access"]["read_only"], true);
    assert_eq!(
        read_i8_update["bytes_access"]["access_source"],
        "indexed_fixed_byte_bank"
    );

    let expected_numeric_writes = [
        (
            "write_u16_le",
            "bytes_write_unsigned",
            5,
            "8d220d82abb00ef958463bc77f36fcc1879cf87225bb513b2946fedfdb5d4300",
            json!({"offset": 2, "byte_count": 2, "endian": "Little", "value": 4660}),
            2,
        ),
        (
            "write_i8_first",
            "bytes_write_signed",
            5,
            "b22348517e4e632ac2a247feaa534cb664204a3b5eeb77acb1b0fc306ae63b29",
            json!({"offset": 0, "byte_count": 1, "endian": "Little", "value": -1}),
            1,
        ),
    ];
    for (field_path, expression_kind, byte_len, digest, update_constant_value, patch_count) in
        expected_numeric_writes
    {
        let update = inspect_updates
            .iter()
            .find(|update| update["field_path"] == field_path)
            .unwrap_or_else(|| panic!("{field_path} update should execute"));
        assert_eq!(update["expression_kind"], expression_kind);
        assert_eq!(update["value"]["byte_len"], json!(byte_len));
        assert_eq!(update["value"]["digest"], digest);
        assert_eq!(update["update_constant_value"], update_constant_value);
        assert_eq!(
            update["executor_core"]["executor"],
            "cpu-plan-indexed-bytes-write-evaluator-v1"
        );
        assert_eq!(update["bytes_access"]["read_only"], false);
        assert_eq!(
            update["bytes_access"]["access_source"],
            "indexed_fixed_byte_bank"
        );
        assert_eq!(update["bytes_access"]["byte_bank_used"], true);
        assert_eq!(update["bytes_access"]["mutation_kind"], "inline_bytes_copy");
        assert_eq!(update["bytes_access"]["patch_count"], json!(patch_count));
        assert_eq!(
            update["bytes_storage"]["storage"],
            "indexed_fixed_byte_bank"
        );
        assert_eq!(update["bytes_storage"]["byte_bank_used"], true);
        assert_eq!(update["bytes_storage"]["byte_len"], json!(byte_len));
    }

    let is_empty_update = inspect_updates
        .iter()
        .find(|update| update["field_path"] == "payload_empty")
        .expect("payload_empty update should execute");
    assert_eq!(is_empty_update["expression_kind"], "bytes_is_empty");
    assert_eq!(is_empty_update["value"], false);
    assert_eq!(
        is_empty_update["executor_core"]["executor"],
        "cpu-plan-indexed-bytes-read-evaluator-v1"
    );
    assert_eq!(is_empty_update["bytes_access"]["read_only"], true);
    assert_eq!(
        is_empty_update["bytes_access"]["access_source"],
        "indexed_fixed_byte_bank"
    );

    let expected = [
        ("found_index", "bytes_find", json!(1), "haystack", "needle"),
        (
            "missing_index",
            "bytes_find",
            JsonValue::Null,
            "haystack",
            "needle",
        ),
        (
            "starts",
            "bytes_starts_with",
            json!(true),
            "input",
            "prefix",
        ),
        ("ends", "bytes_ends_with", json!(true), "input", "suffix"),
        (
            "not_ends",
            "bytes_ends_with",
            json!(false),
            "input",
            "suffix",
        ),
    ];
    for (field_path, expression_kind, value, left_role, right_role) in expected {
        let update = inspect_updates
            .iter()
            .find(|update| update["field_path"] == field_path)
            .unwrap_or_else(|| panic!("{field_path} update should execute"));
        assert_eq!(update["expression_kind"], expression_kind);
        assert_eq!(update["value"], value);
        assert_eq!(
            update["executor_core"]["executor"],
            "cpu-plan-indexed-bytes-read-evaluator-v1"
        );
        assert_eq!(update["bytes_access"]["read_only"], true);
        assert_eq!(update["bytes_access"]["inputs"][0]["role"], left_role);
        assert_eq!(update["bytes_access"]["inputs"][1]["role"], right_role);
    }

    let rows = output.report["plan_executor"]["list_summary"]["rows"]["rows"]
        .as_array()
        .expect("indexed Bytes search fixture should report rows");
    let beta = rows
        .iter()
        .find(|row| row["key"] == json!(2))
        .expect("beta row should be present in PlanExecutor list summary");
    assert_eq!(beta["fields"]["payload_hex"], json!("01fe0405ff"));
    assert_eq!(beta["fields"]["payload_base64"], json!("Af4EBf8="));
    assert_eq!(beta["fields"]["decoded_text"], json!("ABC"));
    assert_eq!(beta["fields"]["encoded_text_bytes"]["byte_len"], json!(3));
    assert_eq!(
        beta["fields"]["encoded_text_bytes"]["digest"],
        expected_bytes_digest
    );
    assert_eq!(beta["fields"]["decoded_hex_bytes"]["byte_len"], json!(3));
    assert_eq!(
        beta["fields"]["decoded_hex_bytes"]["digest"],
        expected_bytes_digest
    );
    assert_eq!(beta["fields"]["decoded_base64_bytes"]["byte_len"], json!(3));
    assert_eq!(
        beta["fields"]["decoded_base64_bytes"]["digest"],
        expected_bytes_digest
    );
    assert_eq!(beta["fields"]["payload_mid"]["byte_len"], json!(2));
    assert_eq!(
        beta["fields"]["payload_mid"]["digest"],
        "9df02aeff60f85979be4737edbcd69757628a81b9b1201a48c64ab6b2eb18126"
    );
    assert_eq!(beta["fields"]["payload_take"]["byte_len"], json!(2));
    assert_eq!(
        beta["fields"]["payload_take"]["digest"],
        "6077f477043ae8cefee8bd0f88b7db444863c754a0fb128ecec260de45f50b4e"
    );
    assert_eq!(beta["fields"]["payload_drop"]["byte_len"], json!(3));
    assert_eq!(
        beta["fields"]["payload_drop"]["digest"],
        "99c24dfd75d0145b9fe75fa80f6e4fcae7066f24a3f91195c675a849db264541"
    );
    assert_eq!(beta["fields"]["zero_fill"]["byte_len"], json!(3));
    assert_eq!(
        beta["fields"]["zero_fill"]["digest"],
        "709e80c88487a2411e1ee4dfb9f22a861492d20c4765150c0c794abd70f8147c"
    );
    assert_eq!(beta["fields"]["payload_joined"]["byte_len"], json!(7));
    assert_eq!(
        beta["fields"]["payload_joined"]["digest"],
        "4825013d9d795bb73ad5e21ce6963c7c858ed984a8fa720a1d393554a04f43ee"
    );
    assert_eq!(beta["fields"]["read_u16_be"], json!(65028));
    assert_eq!(beta["fields"]["read_i8_last"], json!(-1));
    assert_eq!(beta["fields"]["write_u16_le"]["byte_len"], json!(5));
    assert_eq!(
        beta["fields"]["write_u16_le"]["digest"],
        "8d220d82abb00ef958463bc77f36fcc1879cf87225bb513b2946fedfdb5d4300"
    );
    assert_eq!(beta["fields"]["write_i8_first"]["byte_len"], json!(5));
    assert_eq!(
        beta["fields"]["write_i8_first"]["digest"],
        "b22348517e4e632ac2a247feaa534cb664204a3b5eeb77acb1b0fc306ae63b29"
    );
    assert_eq!(beta["fields"]["payload_empty"], false);
    assert_eq!(beta["fields"]["found_index"], json!(1));
    assert_eq!(beta["fields"]["missing_index"], JsonValue::Null);
    assert_eq!(beta["fields"]["starts"], true);
    assert_eq!(beta["fields"]["ends"], true);
    assert_eq!(beta["fields"]["not_ends"], false);
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

// test: root_scalar_plan_executor_replays_indexed_same_event_bytes_dependency
#[test]
fn root_scalar_plan_executor_replays_indexed_same_event_bytes_dependency() {
    let steps = vec!["receive-beta-bytes-and-read".to_owned()];
    let output = run_plan_root_scalar_scenario(
        Path::new("../../examples/bytes_indexed_same_event_dependency_plan_ops.bn"),
        Path::new("../../examples/bytes_indexed_same_event_dependency_plan_ops.scn"),
        TargetProfile::SoftwareDefault,
        &steps,
        None,
    )
    .expect("indexed same-event BYTES dependency fixture should execute through PlanExecutor");

    assert_eq!(output.report["status"], "pass");
    assert_root_scenario_product_only(&output.report);
    assert_eq!(
        output.report["plan_executor"]["executed_indexed_update_count"],
        5
    );
    assert_eq!(
        output.report["plan_executor"]["per_step"][0]["bytes_storage_no_copy"],
        true
    );

    let updates = output.report["plan_executor"]["per_step"][0]["indexed_updates"]
        .as_array()
        .expect("same-event step should expose indexed updates");
    assert_eq!(updates.len(), 5);
    assert_eq!(updates[0]["expression_kind"], "source_payload");
    assert_eq!(updates[0]["source_payload_field"], "Bytes");
    assert_eq!(updates[0]["field_path"], "payload");
    assert_eq!(
        updates[0]["value"]["digest"],
        "2096dcbc716cabc261901f6c15ce242d3bb589284cf8e97ba196710211d7e99a"
    );

    let set_update = updates
        .iter()
        .find(|update| update["expression_kind"] == "bytes_set")
        .expect("same-event indexed Bytes/set update should execute");
    assert_eq!(set_update["field_path"], "patched");
    assert_eq!(set_update["value"]["byte_len"], 3);
    assert_eq!(
        set_update["value"]["digest"],
        "9a82810e040052967f7d74664630d0f6f1665873bfffa36fd4ffe40be1241b73"
    );
    assert_eq!(
        set_update["bytes_access"]["access_source"],
        "indexed_fixed_byte_bank"
    );
    assert_eq!(set_update["bytes_access"]["byte_bank_used"], true);
    assert_eq!(
        set_update["bytes_storage"]["storage"],
        "indexed_fixed_byte_bank"
    );
    assert_eq!(set_update["bytes_storage"]["byte_bank_used"], true);
    assert_eq!(
        set_update["executor_core"]["executor"],
        "cpu-plan-indexed-bytes-write-evaluator-v1"
    );

    let length_update = updates
        .iter()
        .find(|update| update["expression_kind"] == "bytes_length")
        .expect("same-event indexed Bytes/length update should execute");
    assert_eq!(length_update["field_path"], "payload_len");
    assert_eq!(length_update["value"], 3);
    assert_eq!(
        length_update["bytes_access"]["access_source"],
        "indexed_fixed_byte_bank"
    );
    assert_eq!(length_update["bytes_access"]["byte_bank_used"], true);

    let get_update = updates
        .iter()
        .find(|update| update["expression_kind"] == "bytes_get")
        .expect("same-event indexed Bytes/get update should execute");
    assert_eq!(get_update["field_path"], "payload_second");
    assert_eq!(get_update["value"], 254);
    assert_eq!(
        get_update["bytes_access"]["access_source"],
        "indexed_fixed_byte_bank"
    );
    assert_eq!(get_update["bytes_access"]["byte_bank_used"], true);
    assert_eq!(get_update["bytes_access"]["index"], 1);

    let patched_get_update = updates
        .iter()
        .find(|update| update["field_path"] == "patched_first")
        .expect("same-event indexed Bytes/get from patched output should execute");
    assert_eq!(patched_get_update["expression_kind"], "bytes_get");
    assert_eq!(patched_get_update["value"], 170);
    assert_eq!(
        patched_get_update["bytes_access"]["access_source"],
        "indexed_fixed_byte_bank"
    );
    assert_eq!(patched_get_update["bytes_access"]["byte_bank_used"], true);
    assert_eq!(patched_get_update["bytes_access"]["index"], 0);

    for key in [
        "copy_from_slice_bytes",
        "vec_clone_bytes",
        "vec_alloc_bytes",
        "zero_fill_bytes",
    ] {
        assert_eq!(
            output.report["plan_executor"]["per_step"][0]["bytes_storage_counters"][key], 0,
            "same-event indexed fixed-bank dependency tick should keep {key}=0"
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

// test: root_scalar_plan_executor_replays_bytes_is_empty_update
