// Included by `../tests.rs`; kept in the parent test module for private executor-helper access.

#[test]
fn root_runtime_branch_update_execution_is_executor_owned() {
    let inline = vec![1, 2, 3, 4];
    let bytes = PlanExecutorBytes::from_inline(
        sha256_bytes(&inline),
        inline.len() as u64,
        inline,
        "root runtime branch update test",
    )
    .expect("test bytes should be valid");
    let executed = assemble_root_runtime_branch_update(RootRuntimeBranchUpdateInput {
        value: json!({
            "$boon_type": "BYTES",
            "storage": "inline",
            "byte_len": 4,
            "digest": bytes.digest()
        }),
        bytes_value: Some(bytes),
        fixed_bytes_mutation: None,
        bytes_access: json!({
            "read_only": false,
            "access_source": "private_bytes"
        }),
        runtime_branch_core: json!({
            "executor": "cpu-plan-root-bytes-write-evaluator-v1"
        }),
        state_write_core: JsonValue::Null,
        bytes_state_core: json!({
            "executor": "cpu-plan-root-bytes-state-transition-v1"
        }),
        expression_kind: "bytes_concat".to_owned(),
        source_payload_field: JsonValue::Null,
        update_constant_id: JsonValue::Null,
        update_constant_value: JsonValue::Null,
        host_effect: JsonValue::Null,
    });

    assert_eq!(executed.expression_kind, "bytes_concat");
    assert_eq!(
        executed.executor_core["executor"],
        "cpu-plan-root-bytes-write-evaluator-v1"
    );
    assert_eq!(
        executed.executor_core["runtime_branch_execution_core"]["executor"],
        "cpu-plan-root-runtime-branch-update-execution-v1"
    );
    assert_eq!(
        executed.executor_core["runtime_branch_execution_core"]["runtime_branch_core"]["executor"],
        "cpu-plan-root-bytes-write-evaluator-v1"
    );
    assert_eq!(
        executed.bytes_state_core["executor"],
        "cpu-plan-root-bytes-state-transition-v1"
    );
    assert_eq!(
        executed.bytes_access["access_source"],
        json!("private_bytes")
    );
}


#[test]
fn root_bytes_update_dispatch_kind_is_executor_owned() {
    fn update_op(expression_kind: PlanExpressionKind) -> PlanOp {
        PlanOp {
            id: PlanOpId(4),
            kind: PlanOpKind::UpdateBranch {
                expression_kind,
                ordered_inputs: Vec::new(),
                source_payload_field: None,
                update_constant_id: None,
                source_guard: None,
            },
            inputs: Vec::new(),
            output: Some(ValueRef::State(StateId(3))),
            indexed: false,
            unresolved_executable_ref_count: 0,
        }
    }

    assert_eq!(
        root_bytes_update_dispatch_kind(&update_op(PlanExpressionKind::BytesLength)),
        Some(RootBytesUpdateDispatchKind::Read)
    );
    assert_eq!(
        root_bytes_update_dispatch_kind(&update_op(PlanExpressionKind::FileReadBytes)),
        Some(RootBytesUpdateDispatchKind::Read)
    );
    assert_eq!(
        root_bytes_update_dispatch_kind(&update_op(PlanExpressionKind::BytesConcat)),
        Some(RootBytesUpdateDispatchKind::Write)
    );
    assert_eq!(
        root_bytes_update_dispatch_kind(&update_op(PlanExpressionKind::FileWriteBytes)),
        Some(RootBytesUpdateDispatchKind::Write)
    );
    assert_eq!(
        root_bytes_update_dispatch_kind(&update_op(PlanExpressionKind::BoolNot)),
        None
    );

    let mut payload_op = update_op(PlanExpressionKind::BytesLength);
    if let PlanOpKind::UpdateBranch {
        source_payload_field,
        ..
    } = &mut payload_op.kind
    {
        *source_payload_field = Some(SourcePayloadField::Bytes);
    }
    assert_eq!(root_bytes_update_dispatch_kind(&payload_op), None);
}


#[test]
fn root_update_candidate_tracker_is_executor_owned() {
    let mut tracker = RootUpdateCandidateTracker::default();

    let inserted = record_root_update_candidate(
        &mut tracker,
        "store.submit",
        RootUpdateCandidate {
            state_id: 7,
            op_id: 40,
            value: json!("value"),
            bytes_value: None,
            fixed_bytes_mutation: None,
        },
    )
    .expect("first candidate should be inserted");
    assert_eq!(inserted.kind, RootUpdateCandidateRecordKind::Inserted);

    let duplicate = record_root_update_candidate(
        &mut tracker,
        "store.submit",
        RootUpdateCandidate {
            state_id: 7,
            op_id: 41,
            value: json!("value"),
            bytes_value: None,
            fixed_bytes_mutation: None,
        },
    )
    .expect("same-value candidate should coalesce");
    assert_eq!(duplicate.kind, RootUpdateCandidateRecordKind::Duplicate);
    assert_eq!(duplicate.op_ids, vec![40, 41]);

    let ordered = tracker.ordered_candidates();
    assert_eq!(ordered.len(), 1);
    assert_eq!(ordered[0].state_id, 7);
    assert_eq!(ordered[0].op_ids, vec![40, 41]);
    assert_eq!(ordered[0].value, json!("value"));

    let conflict = record_root_update_candidate(
        &mut tracker,
        "store.submit",
        RootUpdateCandidate {
            state_id: 7,
            op_id: 42,
            value: json!("other"),
            bytes_value: None,
            fixed_bytes_mutation: None,
        },
    )
    .expect_err("conflicting candidate should be rejected");
    assert!(
        conflict.to_string().contains("conflicting branches"),
        "unexpected conflict error: {conflict}"
    );
}


#[test]
fn root_update_candidate_tracker_rejects_byte_fingerprint_conflicts() {
    let mut tracker = RootUpdateCandidateTracker::default();
    let first_bytes = Some(json!({
        "$boon_type": "BYTES",
        "byte_len": 3,
        "digest": "aaa",
    }));
    let other_bytes = Some(json!({
        "$boon_type": "BYTES",
        "byte_len": 3,
        "digest": "bbb",
    }));

    record_root_update_candidate(
        &mut tracker,
        "store.bytes",
        RootUpdateCandidate {
            state_id: 9,
            op_id: 50,
            value: json!({"$boon_type": "BYTES", "byte_len": 3}),
            bytes_value: first_bytes,
            fixed_bytes_mutation: None,
        },
    )
    .expect("first byte candidate should be inserted");

    let conflict = record_root_update_candidate(
        &mut tracker,
        "store.bytes",
        RootUpdateCandidate {
            state_id: 9,
            op_id: 51,
            value: json!({"$boon_type": "BYTES", "byte_len": 3}),
            bytes_value: other_bytes,
            fixed_bytes_mutation: None,
        },
    )
    .expect_err("same public value with different private bytes should conflict");
    assert!(
        conflict.to_string().contains("conflicting branches"),
        "unexpected byte conflict error: {conflict}"
    );
}


#[test]
fn root_update_commit_assembly_is_executor_owned() {
    let commit = assemble_root_update_commit(RootUpdateCommitInput {
        source_id: SourceId(3),
        target_state: "store.title".to_owned(),
        target_state_id: 7,
        candidate_update_op_ids: vec![40, 41],
        expression_kind: "source_payload_text".to_owned(),
        source_payload_field: json!("Text"),
        update_constant_id: JsonValue::Null,
        update_constant_value: JsonValue::Null,
        bytes_access: JsonValue::Null,
        host_effect: JsonValue::Null,
        executor_core: json!({"executor": "core"}),
        state_write_core: json!({"changed": true}),
        bytes_state_core: JsonValue::Null,
        value: json!("New title"),
        changed: true,
        semantic_delta: None,
    })
    .expect("changed root update commit should assemble");

    assert_eq!(
        commit.touched_state,
        Some(("store.title".to_owned(), json!("New title")))
    );
    assert_eq!(
        commit.semantic_delta_signature.as_deref(),
        Some("FieldSet:store.title")
    );
    assert_eq!(
        commit
            .semantic_delta
            .as_ref()
            .and_then(|delta| delta.get("field_path"))
            .and_then(JsonValue::as_str),
        Some("store.title")
    );
    assert_eq!(commit.update_report["update_op_id"], 40);
    assert_eq!(
        commit.update_report["candidate_update_op_ids"],
        json!([40, 41])
    );
    assert_eq!(
        commit.executor_report["executor"],
        "cpu-plan-root-update-commit-assembly-v1"
    );
}


#[test]
fn root_update_commit_assembly_suppresses_unchanged_delta() {
    let commit = assemble_root_update_commit(RootUpdateCommitInput {
        source_id: SourceId(3),
        target_state: "store.title".to_owned(),
        target_state_id: 7,
        candidate_update_op_ids: vec![40],
        expression_kind: "source_payload_text".to_owned(),
        source_payload_field: json!("Text"),
        update_constant_id: JsonValue::Null,
        update_constant_value: JsonValue::Null,
        bytes_access: JsonValue::Null,
        host_effect: JsonValue::Null,
        executor_core: json!({"executor": "core"}),
        state_write_core: json!({"changed": false}),
        bytes_state_core: JsonValue::Null,
        value: json!("Same title"),
        changed: false,
        semantic_delta: Some(json!({"kind": "FieldSet"})),
    })
    .expect("unchanged root update commit should still report");

    assert_eq!(commit.touched_state, None);
    assert_eq!(commit.semantic_delta_signature, None);
    assert_eq!(commit.semantic_delta, None);
    assert_eq!(commit.update_report["changed"], false);
    assert_eq!(commit.executor_report["emitted_semantic_delta"], false);
}


#[test]
fn root_update_commit_batch_applies_candidates_to_root_state() {
    let (plan, source_id, state_id, update_op_id) = simple_text_source_payload_plan();
    let mut root_state = initialize_root_state(&plan).expect("root state should initialize");
    assert_eq!(root_state.root_state["store.input"], "");

    let executed = RootExecutedUpdate {
        value: json!("Typed text"),
        bytes_value: None,
        fixed_bytes_mutation: None,
        bytes_access: JsonValue::Null,
        executor_core: json!({"executor": "test-root-update"}),
        state_write_core: JsonValue::Null,
        bytes_state_core: JsonValue::Null,
        expression_kind: "source_payload_text".to_owned(),
        source_payload_field: json!("Text"),
        update_constant_id: JsonValue::Null,
        update_constant_value: JsonValue::Null,
        host_effect: JsonValue::Null,
    };
    let mut tracker = RootUpdateCandidateTracker::default();
    record_root_update_candidate(
        &mut tracker,
        "store.input.change",
        root_update_candidate_from_executed(state_id.0, update_op_id.0, &executed),
    )
    .expect("candidate should be recorded");

    let batch = commit_ordered_root_update_candidates(
        &mut root_state,
        &plan,
        source_id,
        &tracker,
        BTreeMap::from([(state_id.0, executed)]),
    )
    .expect("PlanExecutor should commit ordered root update candidates");

    assert_eq!(root_state.root_state["store.input"], "Typed text");
    assert_eq!(batch.executed_update_branch_count, 1);
    assert_eq!(batch.touched_states["store.input"], "Typed text");
    assert_eq!(
        batch.semantic_delta_signatures,
        vec!["FieldSet:store.input".to_owned()]
    );
    assert_eq!(batch.semantic_deltas[0]["field_path"], "store.input");
    assert_eq!(batch.update_reports[0]["update_op_id"], update_op_id.0);
    assert_eq!(
        batch.executor_report["executor"],
        "cpu-plan-root-update-commit-batch-v1"
    );
    assert_eq!(batch.executor_report["committed_update_count"], 1);
}


#[test]
fn root_update_storage_transition_commits_bytes_and_public_state() {
    let mut root_state = JsonMap::new();
    let mut private_bytes = BTreeMap::new();
    let mut fixed_byte_banks = BTreeMap::new();
    let inline = vec![1, 2, 3];
    let bytes = PlanExecutorBytes::from_inline(
        sha256_bytes(&inline),
        inline.len() as u64,
        inline.clone(),
        "root update storage transition test",
    )
    .expect("test bytes should be valid");

    let mut state_owner =
        RootUpdateStateMaps::new(&mut root_state, &mut private_bytes, &mut fixed_byte_banks);
    let transition = apply_root_update_storage_transition(
        &mut state_owner,
        StateId(7),
        "store.payload",
        json!({"$boon_type": "BYTES", "byte_len": 3}),
        Some(bytes),
        None,
        PlanOpId(55),
    )
    .expect("root update storage transition should commit bytes");
    drop(state_owner);

    assert_eq!(root_state["store.payload"]["byte_len"], 3);
    assert_eq!(
        private_bytes
            .get(&7)
            .expect("private BYTES state should be committed")
            .inline_bytes,
        inline
    );
    assert!(!fixed_byte_banks.contains_key(&7));
    assert_eq!(transition.target_state_id, StateId(7));
    assert_eq!(transition.target_state_label, "store.payload");
    assert_eq!(transition.bytes_transition_mode, "bytes_commit");
    assert_eq!(
        transition.executor_report["executor"],
        "cpu-plan-root-update-storage-transition-v1"
    );
    assert_eq!(
        transition.executor_report["bytes_transition_core"]["executor"],
        "cpu-plan-root-bytes-state-transition-v1"
    );
}


#[test]
fn root_update_storage_transition_applies_fixed_patch() {
    let mut root_state = JsonMap::new();
    let mut private_bytes = BTreeMap::new();
    let mut fixed_byte_banks = BTreeMap::new();
    let inline = vec![4, 5, 6];
    let bytes = PlanExecutorBytes::from_inline(
        sha256_bytes(&inline),
        inline.len() as u64,
        inline,
        "root update fixed patch transition seed",
    )
    .expect("test bytes should be valid");
    private_bytes.insert(7, bytes);
    fixed_byte_banks.insert(7, vec![4, 5, 6]);

    let mut state_owner =
        RootUpdateStateMaps::new(&mut root_state, &mut private_bytes, &mut fixed_byte_banks);
    let transition = apply_root_update_storage_transition(
        &mut state_owner,
        StateId(7),
        "store.payload",
        json!({"$boon_type": "BYTES", "byte_len": 3}),
        None,
        Some(RootBytesFixedMutation {
            input_state_id: StateId(7),
            output_state_id: StateId(7),
            patches: vec![(1, 9)],
        }),
        PlanOpId(56),
    )
    .expect("root update storage transition should apply fixed patch");
    drop(state_owner);

    assert_eq!(root_state["store.payload"]["byte_len"], 3);
    assert!(!private_bytes.contains_key(&7));
    assert_eq!(fixed_byte_banks.get(&7), Some(&vec![4, 9, 6]));
    assert_eq!(transition.bytes_transition_mode, "fixed_byte_patch");
    assert_eq!(
        transition.executor_report["bytes_transition_mode"],
        "fixed_byte_patch"
    );
}


#[test]
fn root_row_expression_finds_initial_list_value_and_converts_number() {
    let mut plan = empty_executor_test_plan();
    plan.constants = vec![boon_plan::PlanConstant {
        id: PlanConstantId(0),
        value: PlanConstantValue::Text {
            value: "A".to_owned(),
        },
    }];
    plan.storage_layout.list_slots = vec![boon_plan::ListStorageSlot {
        id: boon_plan::PlanStorageId(0),
        list_id: boon_plan::ListId(0),
        scope_id: None,
        row_field_ids: vec![FieldId(1), FieldId(2)],
        capacity: None,
        hidden_key_type: "none".to_owned(),
        has_generation: false,
        initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![boon_plan::PlanInitialListRow {
            fields: vec![
                boon_plan::PlanInitialListField {
                    name: "key".to_owned(),
                    field_id: Some(FieldId(1)),
                    value: PlanConstantValue::Text {
                        value: "A".to_owned(),
                    },
                },
                boon_plan::PlanInitialListField {
                    name: "width".to_owned(),
                    field_id: Some(FieldId(2)),
                    value: PlanConstantValue::Text {
                        value: "120".to_owned(),
                    },
                },
            ],
        }],
    }];

    let expression = PlanRowExpression::TextToNumber {
        input: Box::new(PlanRowExpression::ListFindValue {
            list_id: boon_plan::ListId(0),
            field: FieldId(1),
            value: Box::new(PlanRowExpression::Constant {
                constant_id: PlanConstantId(0),
            }),
            target: FieldId(2),
            fallback: None,
        }),
    };

    let value = eval_root_source_transform_row_expression(&plan, &JsonMap::new(), &expression)
        .expect("root evaluator should read initial list rows");
    assert_eq!(value, json!(120));
}


#[test]
fn root_row_expression_list_find_uses_fallback_derived_field() {
    let mut plan = empty_executor_test_plan();
    plan.constants = vec![boon_plan::PlanConstant {
        id: PlanConstantId(0),
        value: PlanConstantValue::Text {
            value: "missing".to_owned(),
        },
    }];
    plan.storage_layout.list_slots = vec![boon_plan::ListStorageSlot {
        id: boon_plan::PlanStorageId(0),
        list_id: boon_plan::ListId(0),
        scope_id: None,
        row_field_ids: vec![FieldId(1), FieldId(2)],
        capacity: None,
        hidden_key_type: "none".to_owned(),
        has_generation: false,
        initializer_kind: boon_plan::ListInitializerKind::RecordLiteral,
        range: None,
        initial_rows: vec![boon_plan::PlanInitialListRow {
            fields: vec![
                boon_plan::PlanInitialListField {
                    name: "key".to_owned(),
                    field_id: Some(FieldId(1)),
                    value: PlanConstantValue::Text {
                        value: "present".to_owned(),
                    },
                },
                boon_plan::PlanInitialListField {
                    name: "width".to_owned(),
                    field_id: Some(FieldId(2)),
                    value: PlanConstantValue::Text {
                        value: "120".to_owned(),
                    },
                },
            ],
        }],
    }];
    plan.debug_map.derived_values = vec![boon_plan::DebugEntry {
        id: "field:4".to_owned(),
        label: "store.default_width".to_owned(),
    }];
    let root_state = JsonMap::from_iter([(
        "store.default_width".to_owned(),
        JsonValue::String("88".to_owned()),
    )]);

    let expression = PlanRowExpression::ListFindValue {
        list_id: boon_plan::ListId(0),
        field: FieldId(1),
        value: Box::new(PlanRowExpression::Constant {
            constant_id: PlanConstantId(0),
        }),
        target: FieldId(2),
        fallback: Some(Box::new(PlanRowExpression::Field {
            input: ValueRef::Field(FieldId(4)),
        })),
    };

    let value = eval_root_source_transform_row_expression(&plan, &root_state, &expression)
        .expect("root evaluator should use fallback when no initial row matches");
    assert_eq!(value, json!("88"));
}


#[test]
fn root_bytes_state_transition_applies_fixed_mutation_in_executor() {
    let mut private_bytes = BTreeMap::from([(
        2,
        PlanExecutorBytes::from_inline(sha256_bytes(&[10, 20, 30]), 3, vec![10, 20, 30], "input")
            .expect("input bytes should be valid"),
    )]);
    let mut fixed_banks = BTreeMap::from([(4, vec![0, 0, 0])]);
    let mut bytes_owner = RootBytesStateMaps::new(&mut private_bytes, &mut fixed_banks);
    let transition = apply_root_bytes_state_transition(
        &mut bytes_owner,
        StateId(4),
        None,
        Some(RootBytesFixedMutation {
            input_state_id: StateId(2),
            output_state_id: StateId(4),
            patches: vec![(1, 99)],
        }),
        PlanOpId(40),
    )
    .expect("fixed-byte mutation should be applied by PlanExecutor");
    drop(bytes_owner);

    assert_eq!(fixed_banks[&4], vec![10, 99, 30]);
    assert!(!private_bytes.contains_key(&4));
    assert_eq!(transition.mode, "fixed_byte_patch");
    assert_eq!(
        transition.executor_report["executor"],
        "cpu-plan-root-bytes-state-transition-v1"
    );
    assert_eq!(transition.executor_report["mode"], "fixed_byte_patch");
}
