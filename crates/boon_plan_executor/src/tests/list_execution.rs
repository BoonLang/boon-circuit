// Included by `../tests.rs`; kept in the parent test module for private executor-helper access.

#[test]
fn source_event_payload_bytes_report_writes_artifacts_and_inlines_small_payloads() {
    let temp_dir = std::env::temp_dir().join(format!(
        "boon-plan-executor-source-event-bytes-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    let report_path = temp_dir.join("route-report.json");

    let small = vec![1, 2];
    let large = vec![0, 1, 2, 3];
    let report = build_source_event_payload_bytes_report(
        &BTreeMap::from([
            ("small".to_owned(), small.clone()),
            ("weird/name".to_owned(), large.clone()),
        ]),
        Some(&report_path),
        3,
    )
    .unwrap();

    let small_digest = sha256_bytes(&small);
    assert_eq!(report.payload_bytes["small"]["storage"], "inline");
    assert_eq!(report.payload_bytes["small"]["digest"], small_digest);
    assert_eq!(report.payload_bytes["small"]["byte_len"], 2);
    assert_eq!(report.payload_bytes["small"]["inline_bytes"], json!([1, 2]));
    assert_eq!(report.payload_bytes["small"]["inline_byte_limit"], 3);

    let large_digest = sha256_bytes(&large);
    let artifact_path = report.payload_bytes["weird/name"]["artifact_path"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(report.payload_bytes["weird/name"]["storage"], "artifact");
    assert_eq!(report.payload_bytes["weird/name"]["digest"], large_digest);
    assert_eq!(
        report.payload_bytes["weird/name"]["artifact_sha256"],
        large_digest
    );
    assert_eq!(report.payload_bytes["weird/name"]["inline_byte_limit"], 3);
    assert!(
        artifact_path.ends_with(&format!(
            "route-report-artifacts/source-event-weird_name-{large_digest}.bytes"
        )),
        "unexpected artifact path: {artifact_path}"
    );
    assert_eq!(fs::read(&artifact_path).unwrap(), large);
    assert_eq!(report.artifacts.len(), 1);
    assert_eq!(report.artifacts[0]["path"], artifact_path);
    assert_eq!(report.artifacts[0]["sha256"], large_digest);
    assert_eq!(
        report.executor_report["executor"],
        "cpu-plan-source-event-payload-bytes-report-v1"
    );
    assert_eq!(report.executor_report["payload_field_count"], 2);
    assert_eq!(report.executor_report["inline_payload_count"], 1);
    assert_eq!(report.executor_report["artifact_payload_count"], 1);
    assert_eq!(report.executor_report["runtime_ast_eval_count"], 0);

    let _ = fs::remove_dir_all(&temp_dir);
}


#[test]
fn initial_list_row_constant_value_conversion_is_executor_owned() {
    let value = PlanConstantValue::Bytes {
        byte_len: 3,
        sha256: sha256_bytes(&[7, 8, 9]),
        inline_bytes: Some(vec![7, 8, 9]),
    };

    let json_value = plan_constant_value_json_value(&value, "initial row field `payload`")
        .expect("BYTES row value should report JSON");
    assert_eq!(json_value["$boon_type"], "BYTES");
    assert_eq!(json_value["byte_len"], 3);

    let bytes = plan_constant_value_bytes(&value, "initial row field `payload`")
        .expect("BYTES row value should validate")
        .expect("BYTES row value should produce private bytes");
    assert_eq!(bytes.inline_bytes(), &[7, 8, 9]);

    let scalar = PlanConstantValue::Text {
        value: "row title".to_owned(),
    };
    assert_eq!(
        plan_constant_value_json_value(&scalar, "initial row field `title`")
            .expect("TEXT row value should report JSON"),
        json!("row title")
    );
    assert!(
        plan_constant_value_bytes(&scalar, "initial row field `title`")
            .expect("TEXT row value should not fail")
            .is_none()
    );

    let tampered = PlanConstantValue::Bytes {
        byte_len: 3,
        sha256: sha256_bytes(&[7, 8, 9]),
        inline_bytes: Some(vec![7, 8, 10]),
    };
    let error = plan_constant_value_bytes(&tampered, "initial row field `payload`")
        .expect_err("digest mismatch should be rejected by executor conversion");
    assert!(
        error.to_string().contains("digest mismatch"),
        "unexpected error: {error}"
    );
}


#[test]
fn list_row_report_fields_are_executor_owned() {
    let row = PlanExecutorListRow {
        key: 4,
        generation: 2,
        fields: BTreeMap::from([
            ("title".to_owned(), json!("task")),
            ("payload".to_owned(), json!("stale-public-value")),
        ]),
    };
    let private_bytes = BTreeMap::from([(
        "payload".to_owned(),
        PlanExecutorBytes::from_inline(sha256_bytes(&[4, 5, 6]), 3, vec![4, 5, 6], "row payload")
            .expect("valid private row bytes"),
    )]);

    let fields = list_row_report_fields(&row, &private_bytes);
    assert_eq!(fields["title"], json!("task"));
    assert_eq!(fields["payload"]["$boon_type"], "BYTES");
    assert_eq!(fields["payload"]["byte_len"], 3);
    assert_eq!(fields["payload"]["digest"], sha256_bytes(&[4, 5, 6]));
}


#[test]
fn list_row_state_carrier_reports_private_bytes() {
    let row = PlanExecutorListRowState {
        key: 11,
        generation: 3,
        fields: BTreeMap::from([
            ("title".to_owned(), json!("row")),
            ("payload".to_owned(), json!("stale-public-value")),
        ]),
        private_bytes: BTreeMap::from([(
            "payload".to_owned(),
            PlanExecutorBytes::from_inline(
                sha256_bytes(&[9, 8, 7]),
                3,
                vec![9, 8, 7],
                "row state payload",
            )
            .expect("valid row state bytes"),
        )]),
        fixed_bytes_banks: BTreeMap::from([("payload".to_owned(), vec![9, 8, 7])]),
    };
    let public_rows = list_row_state_public_rows(&BTreeMap::from([(5usize, vec![row.clone()])]));
    assert_eq!(public_rows[&5][0].key, 11);
    assert_eq!(public_rows[&5][0].fields["title"], json!("row"));

    let fields = list_row_state_report_fields(&row);
    assert_eq!(fields["title"], json!("row"));
    assert_eq!(fields["payload"]["$boon_type"], "BYTES");
    assert_eq!(fields["payload"]["byte_len"], 3);
    assert_eq!(fields["payload"]["digest"], sha256_bytes(&[9, 8, 7]));
}


#[test]
fn list_row_initial_state_refresh_is_executor_owned() {
    let scope_id = boon_plan::ScopeId(3);
    let title_state_id = StateId(21);
    let payload_state_id = StateId(22);
    let list_slot = boon_plan::ListStorageSlot {
        id: boon_plan::PlanStorageId(1),
        list_id: boon_plan::ListId(7),
        scope_id: Some(scope_id),
        row_field_ids: Vec::new(),
        capacity: None,
        hidden_key_type: "u64".to_owned(),
        has_generation: true,
        initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: Vec::new(),
    };
    let mut plan = empty_executor_test_plan();
    plan.debug_map.state_slots = vec![
        boon_plan::DebugEntry {
            id: "state:21".to_owned(),
            label: "todo.title_state".to_owned(),
        },
        boon_plan::DebugEntry {
            id: "state:22".to_owned(),
            label: "todo.payload_state".to_owned(),
        },
    ];
    plan.storage_layout.list_slots = vec![list_slot.clone()];
    plan.storage_layout.scalar_slots = vec![
        boon_plan::ScalarStorageSlot {
            id: boon_plan::PlanStorageId(2),
            state_id: title_state_id,
            value_type: PlanValueType::Text,
            scope_id: Some(scope_id),
            indexed: true,
            initial_value_kind: InitialValueKind::Text,
            initial_constant_id: None,
            initial_root_field_path: None,
            initial_row_field_path: Some("todo.title".to_owned()),
        },
        boon_plan::ScalarStorageSlot {
            id: boon_plan::PlanStorageId(3),
            state_id: payload_state_id,
            value_type: PlanValueType::Bytes { fixed_len: Some(3) },
            scope_id: Some(scope_id),
            indexed: true,
            initial_value_kind: InitialValueKind::Bytes,
            initial_constant_id: None,
            initial_root_field_path: None,
            initial_row_field_path: Some("todo.payload".to_owned()),
        },
    ];
    plan.storage_layout.byte_banks = vec![boon_plan::ByteStorageBank {
        id: boon_plan::PlanStorageId(4),
        state_storage_id: boon_plan::PlanStorageId(3),
        state_id: payload_state_id,
        scope_id: Some(scope_id),
        indexed: true,
        fixed_len: 3,
        capacity: None,
    }];
    let payload = PlanExecutorBytes::from_inline(
        sha256_bytes(&[1, 2, 3]),
        3,
        vec![1, 2, 3],
        "row initial state refresh payload",
    )
    .expect("valid payload bytes");
    let mut row = PlanExecutorListRowState {
        key: 4,
        generation: 1,
        fields: BTreeMap::from([
            ("title".to_owned(), json!("Buy milk")),
            ("payload".to_owned(), payload.report_json()),
        ]),
        private_bytes: BTreeMap::from([("payload".to_owned(), payload)]),
        fixed_bytes_banks: BTreeMap::new(),
    };

    refresh_list_row_initial_state_fields(&plan, &list_slot, &mut row);

    assert_eq!(row.fields["title_state"], json!("Buy milk"));
    assert_eq!(row.fields["payload_state"]["$boon_type"], "BYTES");
    assert_eq!(
        row.private_bytes["payload_state"].inline_bytes(),
        &[1, 2, 3]
    );
    assert_eq!(row.fixed_bytes_banks["payload_state"], vec![1, 2, 3]);
}


#[test]
fn list_row_bool_not_refresh_is_executor_owned() {
    let scope_id = boon_plan::ScopeId(4);
    let done_state_id = StateId(31);
    let not_done_field_id = FieldId(41);
    let list_slot = boon_plan::ListStorageSlot {
        id: boon_plan::PlanStorageId(1),
        list_id: boon_plan::ListId(9),
        scope_id: Some(scope_id),
        row_field_ids: Vec::new(),
        capacity: None,
        hidden_key_type: "u64".to_owned(),
        has_generation: true,
        initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: Vec::new(),
    };
    let mut plan = empty_executor_test_plan();
    plan.debug_map.state_slots = vec![boon_plan::DebugEntry {
        id: "state:31".to_owned(),
        label: "todo.done".to_owned(),
    }];
    plan.debug_map.fields = vec![boon_plan::DebugEntry {
        id: "field:41".to_owned(),
        label: "todo.not_done".to_owned(),
    }];
    plan.storage_layout.list_slots = vec![list_slot.clone()];
    plan.storage_layout.scalar_slots = vec![boon_plan::ScalarStorageSlot {
        id: boon_plan::PlanStorageId(2),
        state_id: done_state_id,
        value_type: PlanValueType::Bool,
        scope_id: Some(scope_id),
        indexed: true,
        initial_value_kind: InitialValueKind::Bool,
        initial_constant_id: None,
        initial_root_field_path: None,
        initial_row_field_path: None,
    }];
    plan.regions = vec![boon_plan::OperationRegion {
        id: boon_plan::PlanRegionId(1),
        kind: RegionKind::DerivedEvaluation,
        ops: vec![PlanOp {
            id: PlanOpId(12),
            kind: PlanOpKind::DerivedValue {
                derived_kind: boon_plan::PlanDerivedKind::Pure,
                startup_recompute: true,
                expression: Some(PlanDerivedExpression::BoolNot {
                    input: ValueRef::State(done_state_id),
                }),
            },
            inputs: vec![ValueRef::State(done_state_id)],
            output: Some(ValueRef::Field(not_done_field_id)),
            indexed: true,
            unresolved_executable_ref_count: 0,
        }],
    }];

    let mut fields = BTreeMap::from([("done".to_owned(), json!(false))]);
    let deltas = refresh_list_row_bool_not_deltas(&plan, &list_slot, "todos", 7, 1, &mut fields)
        .expect("strict Bool/not refresh should produce a delta");
    assert_eq!(fields["not_done"], json!(true));
    assert_eq!(deltas.len(), 1);
    assert_eq!(deltas[0]["kind"], "FieldSet");
    assert_eq!(deltas[0]["field_path"], "not_done");
    assert_eq!(deltas[0]["value"], json!(true));

    let unchanged = refresh_list_row_bool_not_fields(&plan, &list_slot, "todos", 7, 1, &mut fields)
        .expect("best-effort Bool/not refresh should succeed");
    assert!(
        unchanged.is_empty(),
        "best-effort refresh should not emit a delta when the value is current"
    );
    fields.insert("done".to_owned(), json!(true));
    let changed = refresh_list_row_bool_not_fields(&plan, &list_slot, "todos", 7, 1, &mut fields)
        .expect("best-effort Bool/not refresh should update changed values");
    assert_eq!(fields["not_done"], json!(false));
    assert_eq!(changed[0]["value"], json!(false));
}


#[test]
fn list_row_textlike_field_is_executor_owned() {
    let row = PlanExecutorListRow {
        key: 8,
        generation: 1,
        fields: BTreeMap::from([
            ("title".to_owned(), json!("task")),
            ("count".to_owned(), json!(3)),
            ("done".to_owned(), json!(true)),
            ("metadata".to_owned(), json!({"owner": "plan"})),
        ]),
    };

    assert_eq!(
        list_row_textlike_field(&row, "title").as_deref(),
        Some("task")
    );
    assert_eq!(list_row_textlike_field(&row, "count").as_deref(), Some("3"));
    assert_eq!(
        list_row_textlike_field(&row, "done").as_deref(),
        Some("True")
    );
    assert_eq!(list_row_textlike_field(&row, "metadata"), None);
    assert_eq!(list_row_textlike_field(&row, "missing"), None);
}


#[test]
fn source_payload_press_updates_bool_state_as_event_pulse() {
    let slot = boon_plan::ScalarStorageSlot {
        id: boon_plan::PlanStorageId(0),
        state_id: StateId(1),
        value_type: PlanValueType::Bool,
        scope_id: None,
        indexed: false,
        initial_value_kind: InitialValueKind::Bool,
        initial_constant_id: None,
        initial_root_field_path: None,
        initial_row_field_path: None,
    };
    let event = RootJsonSourceEvent {
        payload: BTreeMap::new(),
        ..RootJsonSourceEvent::default()
    };

    let value = source_payload_value_for_slot(
        &event,
        &SourcePayloadField::Named("press".to_owned()),
        &slot,
        PlanOpId(7),
    )
    .expect("press source payload should be a bool event pulse");
    assert_eq!(value, json!(true));

    let false_event = RootJsonSourceEvent {
        payload: BTreeMap::from([("press".to_owned(), "False".to_owned())]),
        ..RootJsonSourceEvent::default()
    };
    let value = source_payload_value_for_slot(
        &false_event,
        &SourcePayloadField::Named("press".to_owned()),
        &slot,
        PlanOpId(8),
    )
    .expect("explicit false press source payload should decode");
    assert_eq!(value, json!(false));
}


#[test]
fn list_next_key_allocation_is_executor_owned() {
    let list_state = BTreeMap::from([
        (
            4,
            vec![
                PlanExecutorListRow {
                    key: 1,
                    generation: 1,
                    fields: BTreeMap::new(),
                },
                PlanExecutorListRow {
                    key: 7,
                    generation: 1,
                    fields: BTreeMap::new(),
                },
            ],
        ),
        (9, Vec::new()),
    ]);
    let mut next_keys = initial_list_next_keys(&list_state);
    assert_eq!(next_keys.get(&4), Some(&8));
    assert_eq!(next_keys.get(&9), Some(&1));

    assert_eq!(
        reserve_list_row_key(&mut next_keys, &list_state, 4)
            .expect("first reservation should use current next key"),
        8
    );
    assert_eq!(
        reserve_list_row_key(&mut next_keys, &list_state, 4)
            .expect("second reservation should increment"),
        9
    );
    assert_eq!(next_keys.get(&4), Some(&10));

    let error = reserve_list_row_key(&mut next_keys, &list_state, 99)
        .expect_err("unknown list should be rejected");
    assert!(
        error.to_string().contains("list state missing list 99"),
        "unexpected error: {error}"
    );
}


#[test]
fn row_source_binding_ids_and_deltas_are_executor_owned() {
    let route_source_ids = vec![SourceId(2), SourceId(5), SourceId(9)];
    assert_eq!(
        row_source_binding_id(4, &route_source_ids, SourceId(2)),
        Some(10)
    );
    assert_eq!(
        row_source_binding_id(4, &route_source_ids, SourceId(5)),
        Some(11)
    );
    assert_eq!(
        row_source_binding_id(4, &route_source_ids, SourceId(9)),
        Some(12)
    );
    assert_eq!(
        row_source_binding_id(4, &route_source_ids, SourceId(99)),
        None
    );

    let deltas = build_source_bind_deltas(
        "todos",
        4,
        1,
        &[
            "todo.sources.remove.click".to_owned(),
            "todo.sources.title.change".to_owned(),
        ],
    );
    assert_eq!(deltas.len(), 2);
    assert_eq!(deltas[0]["kind"], "SourceBind");
    assert_eq!(deltas[0]["source_id"], 7);
    assert_eq!(deltas[0]["bind_epoch"], 7);
    assert_eq!(deltas[0]["field_path"], "todo.sources.remove.click");
    assert_eq!(deltas[1]["source_id"], 8);
    assert_eq!(deltas[1]["value"], "todo.sources.title.change");

    let unbinds = build_source_unbind_deltas(
        "todos",
        4,
        2,
        &[
            "todo.sources.remove.click".to_owned(),
            "todo.sources.title.change".to_owned(),
        ],
    );
    assert_eq!(unbinds.len(), 2);
    assert_eq!(unbinds[0]["kind"], "SourceUnbind");
    assert_eq!(unbinds[0]["source_id"], 7);
    assert_eq!(unbinds[0]["bind_epoch"], 7);
    assert!(unbinds[0]["value"].is_null());

    let remove = build_list_remove_delta("todos", 4, 2);
    assert_eq!(remove["kind"], "ListRemove");
    assert_eq!(remove["list_id"], "todos");
    assert_eq!(remove["key"], 4);
    assert_eq!(remove["generation"], 2);
    assert!(remove["source_id"].is_null());
}


#[test]
fn list_mutation_records_are_executor_owned() {
    let list_slot = boon_plan::ListStorageSlot {
        id: boon_plan::PlanStorageId(0),
        list_id: boon_plan::ListId(7),
        scope_id: Some(boon_plan::ScopeId(3)),
        row_field_ids: Vec::new(),
        capacity: None,
        hidden_key_type: "u64".to_owned(),
        has_generation: true,
        initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: Vec::new(),
    };
    let plan = empty_executor_test_plan();
    let append = record_list_append_mutation(
        &plan,
        &list_slot,
        ListAppendMutationInput {
            list_id: 7,
            list_label: "todos".to_owned(),
            append_op_id: 10,
            key: 4,
            generation: 1,
            trigger_value: json!("Buy milk"),
            fields_before_refresh: BTreeMap::from([("title".to_owned(), json!("Buy milk"))]),
            fields_after_refresh: BTreeMap::from([
                ("title".to_owned(), json!("Buy milk")),
                ("completed".to_owned(), json!(false)),
            ]),
            source_paths: vec![
                "todo.remove.click".to_owned(),
                "todo.title.change".to_owned(),
            ],
            row_bool_deltas: vec![json!({
                "kind": "FieldSet",
                "list_id": "todos",
                "key": 4,
                "generation": 1,
                "source_id": null,
                "bind_epoch": null,
                "field_path": "active",
                "value": true,
            })],
        },
    );
    assert_eq!(append.source_bind_count, 2);
    assert_eq!(append.semantic_deltas[0]["kind"], "ListInsert");
    assert_eq!(append.semantic_deltas[1]["kind"], "SourceBind");
    assert_eq!(append.report_row["append_op_id"], 10);
    assert_eq!(
        append.executor_report["executor"],
        "cpu-plan-list-append-mutation-record-v1"
    );

    let remove = record_list_remove_mutation(ListRemoveMutationInput {
        list_id: 7,
        list_label: "todos".to_owned(),
        remove_op_id: 11,
        source_id: 2,
        source_label: "todo.remove.click".to_owned(),
        row_index: 3,
        key: 4,
        generation: 1,
        source_binding_id: Some(7),
        bind_epoch: Some(7),
        row_resolution: json!({"method": "source_binding"}),
        source_paths: vec![
            "todo.remove.click".to_owned(),
            "todo.title.change".to_owned(),
        ],
        row_fields: BTreeMap::from([("title".to_owned(), json!("Buy milk"))]),
    });
    assert_eq!(remove.source_unbind_count, 2);
    assert_eq!(remove.semantic_deltas[0]["kind"], "SourceUnbind");
    assert_eq!(remove.semantic_deltas[2]["kind"], "ListRemove");
    assert_eq!(remove.report_row["remove_op_id"], 11);
    assert_eq!(
        remove.executor_report["executor"],
        "cpu-plan-list-remove-mutation-record-v1"
    );
}


#[test]
fn live_source_event_expected_toml_builder_is_executor_owned() {
    let payload = BTreeMap::from([
        (
            "source".to_owned(),
            "payload-should-not-override".to_owned(),
        ),
        ("custom".to_owned(), "value".to_owned()),
    ]);
    let payload_bytes = BTreeMap::from([("bytes".to_owned(), vec![0x01, 0xfe, 0x04])]);

    let expected = build_live_source_event_expected_toml(PlanExecutorLiveSourceEventExpectedToml {
        source: "store.receive",
        text: Some("Typed"),
        key: Some("Enter"),
        list_id: Some("todos"),
        address: Some("A1"),
        payload: &payload,
        payload_bytes: &payload_bytes,
        pointer_x: Some("10"),
        pointer_y: Some("20"),
        pointer_width: Some("30"),
        pointer_height: Some("40"),
        target_text: Some("target"),
        target_occurrence: Some(2),
        target_key: Some(3),
        target_generation: Some(4),
        bind_epoch: Some(5),
        source_epoch: Some(6),
        source_id: Some(7),
    });

    assert_eq!(
        expected.get("source").and_then(toml::Value::as_str),
        Some("store.receive")
    );
    assert_eq!(
        expected.get("custom").and_then(toml::Value::as_str),
        Some("value")
    );
    assert_eq!(
        expected.get("bytes_hex").and_then(toml::Value::as_str),
        Some("01fe04")
    );
    assert_eq!(
        expected
            .get("target_occurrence")
            .and_then(toml::Value::as_integer),
        Some(2)
    );
    assert_eq!(
        expected.get("source_id").and_then(toml::Value::as_integer),
        Some(7)
    );
}


#[test]
fn coalesce_field_set_deltas_keeps_last_write_per_target() {
    let deltas = vec![
        json!({
            "kind": "FieldSet",
            "list_id": "cells",
            "key": 2,
            "generation": 1,
            "source_id": null,
            "bind_epoch": null,
            "field_path": "value",
            "value": "old"
        }),
        json!({
            "kind": "ListInsert",
            "list_id": "cells",
            "key": 3,
            "generation": 1
        }),
        json!({
            "kind": "FieldSet",
            "list_id": "cells",
            "key": 2,
            "generation": 1,
            "source_id": null,
            "bind_epoch": null,
            "field_path": "value",
            "value": "new"
        }),
    ];

    let coalesced = coalesce_field_set_deltas(deltas).unwrap();
    assert_eq!(coalesced.len(), 2);
    assert_eq!(coalesced[0]["kind"], "ListInsert");
    assert_eq!(coalesced[1]["value"], "new");
}


#[test]
fn indexed_update_conflict_guard_rejects_real_same_target_writes() {
    let report_row = |op_id: u64, key: u64, field: &str, value: JsonValue| {
        json!({
            "list_id": 7,
            "list": "items",
            "key": key,
            "generation": 1,
            "field_path": field,
            "update_op_id": op_id,
            "value": value,
        })
    };

    let mut touched = BTreeMap::new();
    track_indexed_update_write_conflicts(
        &mut touched,
        &[
            report_row(10, 4, "value", json!("old")),
            report_row(11, 5, "value", json!("separate row")),
            report_row(12, 4, "error", json!("separate field")),
        ],
    )
    .expect("different indexed targets should be accepted");

    let error = track_indexed_update_write_conflicts(
        &mut touched,
        &[report_row(14, 4, "value", json!("new"))],
    )
    .expect_err("different values for the same indexed target must be rejected");
    assert!(
        error
            .to_string()
            .contains("conflicting indexed update branches"),
        "unexpected error: {error}"
    );

    let mut touched = BTreeMap::new();
    track_indexed_update_write_conflicts(
        &mut touched,
        &[report_row(20, 4, "value", json!("same"))],
    )
    .expect("first indexed write should be accepted");
    let error = track_indexed_update_write_conflicts(
        &mut touched,
        &[report_row(21, 4, "value", json!("same"))],
    )
    .expect_err("same-value duplicate indexed target ownership must be rejected");
    assert!(
        error
            .to_string()
            .contains("duplicate indexed update branches"),
        "unexpected error: {error}"
    );
}


#[test]
fn indexed_update_conflict_guard_ignores_derived_semantic_deltas() {
    let mut touched = BTreeMap::new();
    track_indexed_update_write_conflicts(
        &mut touched,
        &[json!({
            "list_id": 7,
            "list": "cells",
            "key": 4,
            "generation": 1,
            "field_path": "formula_text",
            "update_op_id": 30,
            "value": "=A1+A2",
        })],
    )
    .expect("derived semantic-delta churn must not count as duplicate real writes");
}
