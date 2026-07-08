// Included by `../tests.rs`; kept in the parent test module for private invariant access.

#[test]
fn schema_accepts_failing_blocker_audits_only_as_blocker_evidence() {
    let path = std::env::temp_dir().join(format!(
        "boon-readiness-schema-{}-{}.json",
        std::process::id(),
        now_string()
    ));
    let mut report = json!({
        "status": "fail",
        "report_version": 1,
        "generated_at_utc": now_string(),
        "command": "audit-goal-readiness",
        "command_argv": ["audit-goal-readiness", "--report", "target/reports/goal-readiness.json"],
        "measurement_mode": "proof",
        "exit_status": 1,
        "git_commit": git_commit(),
        "worktree_fingerprint": worktree_fingerprint(),
        "binary_hash": current_binary_hash(),
        "binary_path": current_binary_path(),
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "budget_hash": "n/a",
        "graph_node_count": 0,
        "per_step_pass_fail": [
            {"id": "human-report-present", "pass": false, "detail": "missing real human report"}
        ],
        "blockers": ["missing fresh real human report"],
        "artifact_sha256s": []
    });

    write_json(&path, &report).unwrap();
    verify_report_schema(&path).unwrap();

    report["command"] = json!("verify-runtime-finality");
    report["command_argv"] = json!([
        "verify-runtime-finality",
        "--report",
        "target/reports/runtime-finality.json"
    ]);
    report["per_step_pass_fail"] = json!([
        {"id": "runtime-finality:parser:real-ast-not-text-lines", "pass": false, "detail": "parser blocker"}
    ]);
    report["blockers"] =
        json!(["parser still depends on line/text/path heuristics instead of a structured AST"]);
    write_json(&path, &report).unwrap();
    verify_report_schema(&path).unwrap();

    report["command"] = json!("verify-example-negative");
    write_json(&path, &report).unwrap();
    assert!(
        verify_report_schema(&path)
            .unwrap_err()
            .to_string()
            .contains("did not pass")
    );

    report["command"] = json!("audit-machine-readiness");
    report["command_argv"] = json!([
        "audit-machine-readiness",
        "--report",
        "target/reports/debug/machine-readiness.json"
    ]);
    write_json(&path, &report).unwrap();
    verify_report_schema(&path).unwrap();

    report["command"] = json!("audit-goal-readiness");
    report["blockers"] = json!([]);
    write_json(&path, &report).unwrap();
    assert!(
        verify_report_schema(&path)
            .unwrap_err()
            .to_string()
            .contains("blockers")
    );

    report["blockers"] = json!(["missing fresh real human report"]);
    report["per_step_pass_fail"] = json!([
        {"id": "schema-only", "pass": true, "detail": "not a readiness blocker"}
    ]);
    write_json(&path, &report).unwrap();
    assert!(
        verify_report_schema(&path)
            .unwrap_err()
            .to_string()
            .contains("no failing per-step check")
    );

    let _ = std::fs::remove_file(path);
}


#[test]
fn bytes_value_summary_hides_inline_payload() {
    let bytes = RuntimeBytes::inline(Bytes::from_static(b"abc"));
    let summary = bytes.report_json();

    assert_eq!(summary["$boon_type"], "BYTES");
    assert_eq!(summary["storage"], "inline");
    assert_eq!(summary["byte_len"], 3);
    assert_eq!(summary["digest"], sha256_bytes(b"abc"));
    assert!(
        summary.get("inline_bytes").is_none(),
        "public summaries must not expose inline bytes: {summary:#?}"
    );

    let artifact = bytes.artifact_json();
    assert_eq!(artifact["inline_bytes"], json!([97, 98, 99]));
    assert_eq!(
        RuntimeBytes::from_artifact(&artifact, "test.bytes")
            .expect("bytes artifact should restore"),
        bytes
    );
}


#[test]
fn large_dynamic_bytes_use_shared_storage_without_public_inline_payload() {
    let small = RuntimeBytes::dynamic(Bytes::from(vec![1; SOURCE_EVENT_INLINE_BYTES_LIMIT]));
    assert_eq!(small.report_json()["storage"], "inline");

    let payload = vec![7; SOURCE_EVENT_INLINE_BYTES_LIMIT + 1];
    let bytes = RuntimeBytes::dynamic(Bytes::from(payload.clone()));
    let summary = bytes.report_json();

    assert_eq!(summary["$boon_type"], "BYTES");
    assert_eq!(summary["storage"], "shared");
    assert_eq!(
        summary["byte_len"],
        (SOURCE_EVENT_INLINE_BYTES_LIMIT + 1) as u64
    );
    assert_eq!(summary["digest"], sha256_bytes(&payload));
    assert!(
        summary.get("inline_bytes").is_none(),
        "public summaries must not expose shared bytes: {summary:#?}"
    );
    assert_eq!(
        bytes
            .inline_bytes()
            .expect("shared runtime bytes should be executable")
            .as_ref(),
        payload.as_slice()
    );

    let artifact = bytes.artifact_json();
    assert_eq!(artifact["storage"], "shared");
    assert_eq!(
        RuntimeBytes::from_artifact(&artifact, "test.shared_bytes")
            .expect("shared bytes artifact should restore"),
        bytes
    );
}


#[test]
fn runtime_bytes_from_artifact_rejects_malformed_private_state() {
    let cases = [
        (
            "missing-inline-bytes",
            json!({
                "$boon_type": "BYTES",
                "storage": "inline",
                "digest": "abc",
                "byte_len": 1
            }),
            "missing `inline_bytes`",
        ),
        (
            "non-array-inline-bytes",
            json!({
                "$boon_type": "BYTES",
                "storage": "inline",
                "digest": "abc",
                "byte_len": 1,
                "inline_bytes": "not-array"
            }),
            "inline_bytes is not an array",
        ),
        (
            "non-byte-inline-entry",
            json!({
                "$boon_type": "BYTES",
                "storage": "inline",
                "digest": "abc",
                "byte_len": 1,
                "inline_bytes": ["x"]
            }),
            "inline_bytes[0] must be a byte",
        ),
        (
            "out-of-range-inline-entry",
            json!({
                "$boon_type": "BYTES",
                "storage": "inline",
                "digest": "abc",
                "byte_len": 1,
                "inline_bytes": [300]
            }),
            "inline_bytes[0] must be in 0..=255",
        ),
        (
            "byte-len-mismatch",
            json!({
                "$boon_type": "BYTES",
                "storage": "inline",
                "digest": "abc",
                "byte_len": 2,
                "inline_bytes": [1]
            }),
            "declare byte_len 2 but carry 1 byte(s)",
        ),
        (
            "empty-digest",
            json!({
                "$boon_type": "BYTES",
                "storage": "inline",
                "digest": "",
                "byte_len": 1,
                "inline_bytes": [1]
            }),
            "runtime bytes must carry a digest",
        ),
        (
            "unsupported-storage",
            json!({
                "$boon_type": "BYTES",
                "storage": "mystery",
                "digest": "abc",
                "byte_len": 1,
                "inline_bytes": [1]
            }),
            "storage has unsupported value `mystery`",
        ),
    ];

    for (id, artifact, expected) in cases {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            RuntimeBytes::from_artifact(&artifact, id)
        }));
        match result {
            Ok(Err(error)) => {
                let error = error.to_string();
                assert!(
                    error.contains(expected),
                    "case {id} returned wrong structured error: {error}"
                );
            }
            Ok(Ok(value)) => panic!("case {id} accepted malformed artifact: {value:#?}"),
            Err(payload) => {
                let message = payload
                    .downcast_ref::<&str>()
                    .map(|message| (*message).to_owned())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "<non-string panic payload>".to_owned());
                panic!("case {id} panicked while rejecting malformed artifact: {message}");
            }
        }
    }
}


#[test]
fn fixed_source_bytes_constructor_zero_fills_empty_body() {
    let BoonValue::Bytes(bytes) =
        finish_bytes_constructor(&BytesSizeSyntax::Fixed(4), 0, Vec::new())
            .expect("fixed empty BYTES constructor should zero-fill")
    else {
        panic!("fixed bytes constructor should produce BYTES");
    };
    let artifact = bytes.artifact_json();
    assert_eq!(artifact["byte_len"], 4);
    assert_eq!(artifact["inline_bytes"], json!([0, 0, 0, 0]));
}


#[test]
fn bridge_value_bytes_and_refs_convert_to_runtime_bytes() {
    let inline = BridgeValue::inline_bytes(
        sha256_bytes(b"bridge-inline"),
        Bytes::from_static(b"bridge-inline"),
    );
    let inline_field = bridge_value_to_field_value(&inline)
        .expect("inline bridge bytes should convert to runtime bytes");
    let FieldValue::Bytes(inline_bytes) = inline_field else {
        panic!("inline bridge bytes should become FieldValue::Bytes");
    };
    assert_eq!(inline_bytes.byte_len, b"bridge-inline".len() as u64);
    assert_eq!(inline_bytes.report_json()["storage"], "inline");

    let blob = BridgeBlobRef {
        digest: "sha256:blob".to_owned(),
        byte_len: 4096,
        media_type: "application/octet-stream".to_owned(),
        storage: "fixture-bounded-pages".to_owned(),
        encoding: "fst".to_owned(),
    };
    let blob_field = bridge_value_to_field_value(&BridgeValue::BlobRef(blob.clone()))
        .expect("blob ref should convert to bytes metadata");
    let FieldValue::Bytes(blob_bytes) = blob_field else {
        panic!("blob ref should become FieldValue::Bytes");
    };
    assert_eq!(blob_bytes.report_json()["storage"], "blob_ref");
    assert_eq!(blob_bytes.report_json()["digest"], blob.digest);
    assert_eq!(blob_bytes.report_json()["byte_len"], blob.byte_len);

    let page = BridgePageRef {
        artifact_digest: "sha256:artifact".to_owned(),
        schema_version: 1,
        schema_hash: "sha256:schema".to_owned(),
        request_fingerprint: "request".to_owned(),
        response_fingerprint: "response".to_owned(),
        input_digest: "sha256:input".to_owned(),
        page_digest: "sha256:page".to_owned(),
        generation: 7,
        offset: 128,
        limit: 256,
        row_count: 32,
        sample_count: 64,
        transition_count: 16,
        byte_length: 2048,
        byte_len: 2048,
        status: "ready".to_owned(),
    };
    let page_field = bridge_value_to_field_value(&BridgeValue::PageRef(page.clone()))
        .expect("page ref should convert to bytes metadata");
    let FieldValue::Bytes(page_bytes) = page_field else {
        panic!("page ref should become FieldValue::Bytes");
    };
    let page_summary = page_bytes.report_json();
    assert_eq!(page_summary["storage"], "page_ref");
    assert_eq!(page_summary["digest"], page.page_digest);
    assert_eq!(page_summary["artifact_digest"], page.artifact_digest);
    assert_eq!(page_summary["byte_len"], page.byte_len);
}


#[test]
fn bridge_completion_payload_sidecars_reach_runtime_bytes_boundary() {
    use boon_bridge::{
        BRIDGE_ABI_VERSION, BridgeCompletionPayloads, BridgeCompletionStatus,
        BridgeEffectScheduler, BridgeExportKind, BridgeExportMetadata, BridgeModuleMetadata,
        BridgeProviderMetadata, BridgeRegistry, BridgeSchema, BridgeSchemaShape,
        BridgeTaskCompletion, BridgeTaskRequest, CANONICAL_SCHEMA_VERSION, bridge_bytes_digest,
    };

    let input = BridgeSchema {
        name: "PayloadRuntimeRequest".to_owned(),
        version: CANONICAL_SCHEMA_VERSION,
        shape: BridgeSchemaShape::Record {
            fields: BTreeMap::new(),
        },
    };
    let output = BridgeSchema {
        name: "PayloadRuntimeOutput".to_owned(),
        version: CANONICAL_SCHEMA_VERSION,
        shape: BridgeSchemaShape::Record {
            fields: BTreeMap::from([
                ("blob".to_owned(), BridgeSchemaShape::BlobRef),
                ("inline".to_owned(), BridgeSchemaShape::Bytes),
                ("page".to_owned(), BridgeSchemaShape::PageRef),
                ("status".to_owned(), BridgeSchemaShape::Text),
            ]),
        },
    };
    let export = BridgeExportMetadata {
        name: "load_payloads".to_owned(),
        kind: BridgeExportKind::Effect,
        input_schema_version: input.version,
        input_schema_hash: input.hash(),
        output_schema_version: output.version,
        output_schema_hash: output.hash(),
        required_capabilities: Vec::new(),
    };
    let mut registry = BridgeRegistry::new();
    registry
        .register_module(BridgeModuleMetadata {
            module: "payloads.v1".to_owned(),
            abi_version: BRIDGE_ABI_VERSION.to_owned(),
            canonical_schema_version: CANONICAL_SCHEMA_VERSION,
            provider: BridgeProviderMetadata {
                provider: "payload-runtime-test".to_owned(),
                provider_version: "0.1.fixture".to_owned(),
                bridge_crate: "boon_payload_runtime_test_bridge".to_owned(),
                bridge_crate_version: "0.1.0".to_owned(),
                features: vec!["bytes-boundary".to_owned()],
            },
            exports: BTreeMap::from([("load_payloads".to_owned(), export.clone())]),
        })
        .expect("payload runtime test module should register once");
    registry
        .register_export_schemas("payloads.v1", "load_payloads", input, output)
        .expect("payload runtime schemas should match metadata");

    let blob_bytes = Bytes::from_static(b"raw waveform blob bytes");
    let page_bytes = Bytes::from_static(b"decoded waveform page bytes");
    let inline_bytes = Bytes::from_static(b"inline header");
    let blob_ref = BridgeBlobRef {
        digest: bridge_bytes_digest(blob_bytes.as_ref()),
        byte_len: blob_bytes.len() as u64,
        media_type: "application/vnd.boon.wave-page".to_owned(),
        storage: "bridge-payload-store".to_owned(),
        encoding: "packed-wave-page".to_owned(),
    };
    let page_ref = BridgePageRef {
        artifact_digest: "sha256:artifact".to_owned(),
        schema_version: CANONICAL_SCHEMA_VERSION,
        schema_hash: "sha256:schema".to_owned(),
        request_fingerprint: "request:fingerprint".to_owned(),
        response_fingerprint: "response:fingerprint".to_owned(),
        input_digest: "sha256:input".to_owned(),
        page_digest: bridge_bytes_digest(page_bytes.as_ref()),
        generation: 1,
        offset: 0,
        limit: page_bytes.len() as u64,
        row_count: 2,
        sample_count: 8,
        transition_count: 4,
        byte_length: page_bytes.len() as u64,
        byte_len: page_bytes.len() as u64,
        status: "ready".to_owned(),
    };
    let inline_digest = bridge_bytes_digest(inline_bytes.as_ref());

    let request = BridgeTaskRequest::new(
        &export,
        "payloads.v1",
        "payload-runtime",
        1,
        BridgeValue::Record(BTreeMap::new()),
        Vec::new(),
        "cancel:payload-runtime",
        0,
    );
    let mut scheduler = BridgeEffectScheduler::new(128);
    scheduler
        .schedule(&registry, request.clone())
        .expect("payload runtime request should schedule");
    let output = BridgeValue::Record(BTreeMap::from([
        ("blob".to_owned(), BridgeValue::BlobRef(blob_ref.clone())),
        (
            "inline".to_owned(),
            BridgeValue::inline_bytes(inline_digest.clone(), inline_bytes.clone()),
        ),
        ("page".to_owned(), BridgeValue::PageRef(page_ref.clone())),
        ("status".to_owned(), BridgeValue::Text("ready".to_owned())),
    ]));
    let mut payloads = BridgeCompletionPayloads::new();
    payloads
        .insert_blob(&blob_ref, blob_bytes)
        .expect("blob sidecar should match blob ref");
    payloads
        .insert_page(&page_ref, page_bytes)
        .expect("page sidecar should match page ref");
    let accepted = scheduler
        .complete_with_payloads(
            BridgeTaskCompletion::for_request(
                &request,
                BridgeCompletionStatus::Ok,
                Some(output),
                Vec::new(),
            ),
            &payloads,
        )
        .expect("completion with matching payload sidecars should be accepted");

    let summary = bridge_completion_output_runtime_summary(&accepted)
        .expect("accepted bridge completion should convert to runtime summary")
        .expect("OK completion should carry output");
    assert_eq!(summary["status"], "ready");
    assert_eq!(summary["blob"]["$boon_type"], "BYTES");
    assert_eq!(summary["blob"]["storage"], "blob_ref");
    assert_eq!(summary["blob"]["digest"], blob_ref.digest);
    assert_eq!(summary["blob"]["byte_len"], blob_ref.byte_len);
    assert!(summary["blob"].get("inline_bytes").is_none());
    assert_eq!(summary["page"]["$boon_type"], "BYTES");
    assert_eq!(summary["page"]["storage"], "page_ref");
    assert_eq!(summary["page"]["digest"], page_ref.page_digest);
    assert_eq!(summary["page"]["artifact_digest"], page_ref.artifact_digest);
    assert_eq!(summary["page"]["byte_len"], page_ref.byte_len);
    assert!(summary["page"].get("inline_bytes").is_none());
    assert_eq!(summary["inline"]["$boon_type"], "BYTES");
    assert_eq!(summary["inline"]["storage"], "inline");
    assert_eq!(summary["inline"]["digest"], inline_digest);
    assert_eq!(summary["inline"]["byte_len"], inline_bytes.len() as u64);
    assert!(summary["inline"].get("inline_bytes").is_none());

    let restored: BridgeTaskCompletion = serde_json::from_value(
        serde_json::to_value(&accepted).expect("completion should serialize"),
    )
    .expect("completion should deserialize");
    assert_eq!(
        bridge_completion_output_runtime_summary(&restored)
            .expect("restored completion should convert"),
        Some(summary),
        "runtime BYTES summaries must be deterministic across completion replay metadata"
    );
}
