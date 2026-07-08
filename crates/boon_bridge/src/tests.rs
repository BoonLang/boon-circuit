use super::*;

#[test]
fn bridge_metadata_includes_provider_capability_schema_hashes_and_abi() {
    let registry = bridge_fixture_registry();
    let module = registry.module("wellen.v1").unwrap();
    assert_eq!(module.abi_version, BRIDGE_ABI_VERSION);
    assert_eq!(module.canonical_schema_version, CANONICAL_SCHEMA_VERSION);
    assert_eq!(module.provider.provider, "wellen");
    let open = module.exports.get("open").unwrap();
    assert_eq!(open.kind, BridgeExportKind::Effect);
    assert!(open.input_schema_hash.starts_with("sha256:"));
    assert!(open.output_schema_hash.starts_with("sha256:"));
    assert_eq!(open.required_capabilities[0].grant_id, "grant:wave-traces");
}

#[test]
fn bridge_schema_sidecar_is_registered_but_not_serialized() {
    let registry = bridge_fixture_registry();
    assert!(
        registry
            .export_schemas("wellen.v1", "open")
            .is_some_and(|schemas| schemas.input.hash()
                == registry
                    .export("wellen.v1", "open")
                    .unwrap()
                    .input_schema_hash)
    );

    let serialized = serde_json::to_value(&registry).expect("bridge registry should serialize");
    assert!(serialized.get("export_schemas").is_none());
    let restored: BridgeRegistry =
        serde_json::from_value(serialized).expect("bridge registry should deserialize");
    assert!(restored.export_schemas("wellen.v1", "open").is_none());
    assert!(restored.export("wellen.v1", "open").is_ok());
}

#[test]
fn bridge_golden_vectors_cover_records_variants_lists_refs_pages_blobs_diagnostics_and_completions()
{
    let vectors = bridge_golden_vectors();
    assert!(vectors["record"].starts_with("sha256:"));
    assert!(vectors["schema"].starts_with("sha256:"));
    assert!(vectors["completion"].starts_with("sha256:"));
    assert_eq!(vectors.len(), 3);
}

#[test]
fn bridge_canonical_hash_streams_same_bytes_as_canonical_json() {
    let value = BridgeValue::Record(BTreeMap::from([
        (
            "bytes".to_owned(),
            BridgeValue::inline_bytes("sha256:inline", Vec::from([1, 2, 3, 4])),
        ),
        (
            "tagged".to_owned(),
            BridgeValue::Tagged {
                tag: "Ready".to_owned(),
                value: Box::new(BridgeValue::Bool(true)),
            },
        ),
    ]));
    let canonical = canonical_json(&value);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    assert_eq!(
        canonical_hash(&value),
        format!("sha256:{:x}", hasher.finalize())
    );
}

#[test]
fn bridge_bytes_use_shared_storage_without_changing_json_shape() {
    let value = BridgeValue::inline_bytes("sha256:inline", Vec::from([1, 2, 3, 4]));
    let expected = json!({
        "kind": "bytes",
        "value": {
            "digest": "sha256:inline",
            "byte_len": 4,
            "bytes": [1, 2, 3, 4],
        },
    });
    let serialized = serde_json::to_value(&value).expect("bridge bytes should serialize");
    assert_eq!(serialized, expected);

    let restored: BridgeValue =
        serde_json::from_value(serialized).expect("bridge bytes should deserialize");
    assert_eq!(restored, value);
    assert_eq!(canonical_json(&restored), canonical_json(&value));
    assert_eq!(canonical_hash(&restored), canonical_hash(&value));
    match restored {
        BridgeValue::Bytes { bytes, .. } => assert_eq!(&bytes[..], &[1, 2, 3, 4]),
        other => panic!("expected bridge bytes, got {other:?}"),
    }
}

#[test]
fn bridge_payload_store_keeps_blob_and_page_bytes_outside_bridge_values() {
    let blob_bytes = Bytes::from_static(b"raw waveform payload");
    let page_bytes = Bytes::from_static(b"decoded page payload");
    let blob_ref = fixture_blob_ref_for_bytes(&blob_bytes);
    let page_ref = fixture_page_ref_for_bytes(&page_bytes);
    let blob_value = BridgeValue::BlobRef(blob_ref.clone());
    let page_value = BridgeValue::PageRef(page_ref.clone());
    let blob_json = canonical_json(&blob_value);
    let page_json = canonical_json(&page_value);
    let mut store = BridgePayloadStore::new();

    store
        .insert_blob(&blob_ref, blob_bytes.clone())
        .expect("blob bytes should satisfy blob ref digest and length");
    store
        .insert_page(&page_ref, page_bytes.clone())
        .expect("page bytes should satisfy page ref digest and length");

    assert_eq!(store.blob(&blob_ref.digest), Some(&blob_bytes));
    assert_eq!(store.page(&page_ref.page_digest), Some(&page_bytes));
    assert_eq!(canonical_json(&blob_value), blob_json);
    assert_eq!(canonical_json(&page_value), page_json);
}

#[test]
fn bridge_payload_store_rejects_digest_and_length_drift() {
    let bytes = Bytes::from_static(b"raw waveform payload");
    let blob_ref = fixture_blob_ref_for_bytes(&bytes);
    let mut wrong_digest = blob_ref.clone();
    wrong_digest.digest = "sha256:wrong".to_owned();
    let digest_drift = BridgePayloadStore::new()
        .insert_blob(&wrong_digest, bytes.clone())
        .expect_err("blob store should reject digest drift");
    assert_eq!(digest_drift.code, BridgeErrorCode::SchemaMismatch);

    let mut wrong_len = fixture_page_ref_for_bytes(&bytes);
    wrong_len.byte_length = wrong_len.byte_length.saturating_add(1);
    wrong_len.byte_len = wrong_len.byte_len.saturating_add(1);
    let len_drift = BridgePayloadStore::new()
        .insert_page(&wrong_len, bytes)
        .expect_err("page store should reject byte length drift");
    assert_eq!(len_drift.code, BridgeErrorCode::SchemaMismatch);
}

#[test]
fn bridge_scheduler_completion_payload_sidecars_validate_blob_and_page_refs() {
    let (pass, detail) = bridge_completion_payloads_contract_check();
    assert!(pass, "completion payload sidecar check failed: {detail}");
}

#[test]
fn bridge_schema_validation_accepts_bytes_blob_artifact_and_page_refs() {
    let value = BridgeValue::Record(BTreeMap::from([
        (
            "bytes".to_owned(),
            BridgeValue::inline_bytes("sha256:inline", Vec::from([1, 2, 3, 4])),
        ),
        (
            "blob".to_owned(),
            BridgeValue::BlobRef(BridgeBlobRef {
                digest: "sha256:blob".to_owned(),
                byte_len: 4,
                media_type: "application/octet-stream".to_owned(),
                storage: "bridge-cache".to_owned(),
                encoding: "raw".to_owned(),
            }),
        ),
        (
            "artifact".to_owned(),
            BridgeValue::ArtifactRef(fixture_artifact_ref()),
        ),
        ("page".to_owned(), BridgeValue::PageRef(fixture_page_ref())),
    ]));
    let shape = BridgeSchemaShape::Record {
        fields: BTreeMap::from([
            ("bytes".to_owned(), BridgeSchemaShape::Bytes),
            ("blob".to_owned(), BridgeSchemaShape::BlobRef),
            ("artifact".to_owned(), BridgeSchemaShape::ArtifactRef),
            ("page".to_owned(), BridgeSchemaShape::PageRef),
        ]),
    };
    let before_json = canonical_json(&value);
    let before_hash = canonical_hash(&value);

    validate_bridge_value_shape(&value, &shape)
        .expect("bytes/blob/artifact/page refs should satisfy their schema shapes");

    assert_eq!(canonical_json(&value), before_json);
    assert_eq!(canonical_hash(&value), before_hash);
}

#[test]
fn bridge_schema_validation_rejects_wrong_ref_kinds_and_byte_len_drift() {
    let wrong_kind = validate_bridge_value_shape(
        &BridgeValue::Text("not bytes".to_owned()),
        &BridgeSchemaShape::Bytes,
    )
    .expect_err("TEXT must not satisfy BYTES");
    assert_eq!(wrong_kind.code, BridgeErrorCode::SchemaMismatch);

    let byte_len_drift = validate_bridge_value_shape(
        &BridgeValue::Bytes {
            digest: "sha256:inline".to_owned(),
            byte_len: 5,
            bytes: Bytes::from_static(&[1, 2, 3, 4]),
        },
        &BridgeSchemaShape::Bytes,
    )
    .expect_err("declared byte_len must match actual bytes");
    assert_eq!(byte_len_drift.code, BridgeErrorCode::SchemaMismatch);

    let wrong_ref = validate_bridge_value_shape(
        &BridgeValue::BlobRef(BridgeBlobRef {
            digest: "sha256:blob".to_owned(),
            byte_len: 4,
            media_type: "application/octet-stream".to_owned(),
            storage: "bridge-cache".to_owned(),
            encoding: "raw".to_owned(),
        }),
        &BridgeSchemaShape::PageRef,
    )
    .expect_err("BlobRef must not satisfy PageRef");
    assert_eq!(wrong_ref.code, BridgeErrorCode::SchemaMismatch);

    let mut page = fixture_page_ref();
    page.byte_len = page.byte_len.saturating_add(1);
    let page_drift =
        validate_bridge_value_shape(&BridgeValue::PageRef(page), &BridgeSchemaShape::PageRef)
            .expect_err("PageRef byte_length and byte_len must agree");
    assert_eq!(page_drift.code, BridgeErrorCode::SchemaMismatch);
}

#[test]
fn bridge_scheduler_enforces_registered_input_output_and_replay_shapes() {
    let registry = bridge_fixture_registry();
    let open = registry.export("wellen.v1", "open").unwrap();
    let missing_options = fixture_open_request(
        open,
        "wave-open:test-missing-options",
        1,
        BridgeValue::Record(BTreeMap::from([(
            "file".to_owned(),
            BridgeValue::ArtifactRef(fixture_artifact_ref()),
        )])),
    );
    let input_mismatch = BridgeEffectScheduler::new(16)
        .schedule(&registry, missing_options)
        .expect_err("registered input shape must reject missing options");
    assert_eq!(input_mismatch.code, BridgeErrorCode::SchemaMismatch);

    let mut scheduler = BridgeEffectScheduler::new(16);
    let request = fixture_open_request(
        open,
        "wave-open:test-output-shape",
        1,
        fixture_open_request_value(),
    );
    scheduler
        .schedule(&registry, request.clone())
        .expect("valid request should schedule");
    let output_mismatch = scheduler
        .complete(BridgeTaskCompletion::for_request(
            &request,
            BridgeCompletionStatus::Ok,
            Some(BridgeValue::PageRef(fixture_page_ref())),
            Vec::new(),
        ))
        .expect_err("registered output shape must reject bare page refs");
    assert_eq!(output_mismatch.code, BridgeErrorCode::SchemaMismatch);

    let replay_request = fixture_open_request(
        open,
        "wave-open:test-replay-shape",
        1,
        fixture_open_request_value(),
    );
    let replay_completion = BridgeTaskCompletion::for_request(
        &replay_request,
        BridgeCompletionStatus::Ok,
        Some(BridgeValue::PageRef(fixture_page_ref())),
        Vec::new(),
    );
    let replay_mismatch = BridgeEffectScheduler::with_replay(16, vec![replay_completion])
        .schedule(&registry, replay_request)
        .expect_err("replayed completions must satisfy registered output shape");
    assert_eq!(replay_mismatch.code, BridgeErrorCode::SchemaMismatch);
}

#[test]
fn bridge_scheduler_deduplicates_and_rejects_stale_duplicate_canceled_or_drifted_completion() {
    let checks = bridge_contract_checks();
    for (id, pass, detail) in checks {
        assert!(pass, "{id} failed: {detail}");
    }
}
