use boon_host_runtime::{
    ContentRef, ContentStore, ContentStoreLimits, FileCapabilityRegistry, FileEffectAdapter,
    FileEffectEvent, FileEffectLimits, apply_event, package_asset_value,
};
use boon_plan::{ApplicationIdentity, ProgramRole};
use boon_runtime::{
    ProgramCapabilityProfile, ProgramCompileRequest, ProgramSession, RuntimeSourceUnit,
    SourcePayload, TransientEffectInvocation, Value, compile_program_artifact,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::time::Duration;
use tempfile::NamedTempFile;

const FILE_PROGRAM: &str = r#"
store: [
    read: SOURCE
    stream_result:
        NotStarted |> HOLD stream_result {
            read |> THEN {
                File/read_stream(
                    file: read.file
                    chunk_bytes: 3
                    retain_content: True
                )
            }
        }
]

outputs: [
    stream_result: store.stream_result
]
"#;

fn program() -> ProgramSession {
    let artifact = compile_program_artifact(&ProgramCompileRequest {
        revision: 1,
        entry_path: "file_read_stream.bn".to_owned(),
        units: vec![RuntimeSourceUnit {
            path: "file_read_stream.bn".to_owned(),
            source: FILE_PROGRAM.to_owned(),
        }],
        application: ApplicationIdentity::new("dev.boon.file-read-stream", "test", "local"),
        role: ProgramRole::Server,
        capability_profile: ProgramCapabilityProfile::TrustedServer,
    })
    .unwrap();
    ProgramSession::start(artifact).unwrap()
}

#[tokio::test]
async fn package_assets_use_the_same_bounded_stream_without_exposing_paths() {
    let content = b"package waveform";
    let registry = FileCapabilityRegistry::new(1).unwrap();
    let mut adapter = adapter(registry);
    let url = "asset://fixture/simple.vcd";
    let package_ref = adapter
        .register_package_asset(url, "application/vnd.test.waveform", content)
        .unwrap();
    let mut program = program();
    let invocation = invocation(&mut program, package_asset_value(url));
    adapter.submit(invocation).unwrap();

    let opened = adapter.next_event().await.unwrap();
    assert_eq!(opened.result_tag(), Some("Opened"));
    assert_eq!(text(fields(&opened), "display_name"), "simple.vcd");
    apply_event(&mut program, &mut adapter, opened).unwrap();

    let mut received = Vec::new();
    loop {
        let event = adapter.next_event().await.unwrap();
        let terminal = event.is_terminal();
        if event.result_tag() == Some("Chunk") {
            received.extend_from_slice(bytes(fields(&event), "bytes"));
        }
        if event.result_tag() == Some("Finished") {
            let retained_fields = variant(fields(&event), "retained", "Retained");
            assert_eq!(
                ContentRef::from_value(&retained_fields["content"]).unwrap(),
                package_ref
            );
        }
        apply_event(&mut program, &mut adapter, event).unwrap();
        if terminal {
            break;
        }
    }

    assert_eq!(received, content);
    assert_eq!(adapter.package_asset_count(), 1);
    assert_eq!(program.pending_transient_effect_count(), 0);
}

fn file(content: &[u8]) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(content).unwrap();
    file.flush().unwrap();
    file
}

fn invocation(program: &mut ProgramSession, selected_file: Value) -> TransientEffectInvocation {
    let dispatched = program
        .dispatch(
            "store.read",
            None,
            SourcePayload {
                fields: BTreeMap::from([("file".to_owned(), selected_file)]),
                ..SourcePayload::default()
            },
        )
        .unwrap();
    let [invocation] = dispatched.runtime_turn.transient_effects.as_slice() else {
        panic!("file source must emit exactly one stream invocation");
    };
    invocation.clone()
}

fn adapter(registry: FileCapabilityRegistry) -> FileEffectAdapter {
    let root = tempfile::tempdir().unwrap().keep();
    let content_store = ContentStore::new(root, ContentStoreLimits::new(4, 1024 * 1024)).unwrap();
    FileEffectAdapter::with_limits(registry, content_store, FileEffectLimits::new(1, 8, 1)).unwrap()
}

fn fields(event: &FileEffectEvent) -> &BTreeMap<String, Value> {
    let Value::Record(fields) = &event.outcome else {
        panic!("stream outcome must be a tagged record");
    };
    fields
}

fn variant<'a>(
    fields: &'a BTreeMap<String, Value>,
    name: &str,
    expected_tag: &str,
) -> &'a BTreeMap<String, Value> {
    let Value::Record(variant) = &fields[name] else {
        panic!("{name} must be a tagged record");
    };
    assert_eq!(
        variant.get("$tag"),
        Some(&Value::Text(expected_tag.to_owned()))
    );
    variant
}

fn number(value: &Value) -> i64 {
    let Value::Number(value) = value else {
        panic!("field must be Number");
    };
    value.to_i64_exact().unwrap()
}

fn text<'a>(fields: &'a BTreeMap<String, Value>, name: &str) -> &'a str {
    let Value::Text(value) = &fields[name] else {
        panic!("field must be Text");
    };
    value
}

fn bytes<'a>(fields: &'a BTreeMap<String, Value>, name: &str) -> &'a [u8] {
    let Value::Bytes(value) = &fields[name] else {
        panic!("field must be Bytes");
    };
    value
}

#[tokio::test]
async fn streams_exact_chunks_only_when_bounded_credits_are_available() {
    let content = b"abcdefghijklmno";
    let selected = file(content);
    let mut registry = FileCapabilityRegistry::new(2).unwrap();
    let capability = registry.register_file(selected.path()).unwrap();
    let mut adapter = adapter(registry);
    let mut program = program();
    let invocation = invocation(&mut program, capability.file_selected_value());
    let call_id = invocation.call_id;

    let submission = adapter.submit(invocation).unwrap();
    assert!(!submission.queued_terminal);

    let opened = tokio::time::timeout(Duration::from_secs(1), adapter.next_event())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(opened.result_sequence, 0);
    assert_eq!(opened.result_tag(), Some("Opened"));
    assert_eq!(number(&fields(&opened)["size"]), content.len() as i64);
    assert_eq!(
        text(fields(&opened), "content_type"),
        "application/octet-stream"
    );

    let mut queued_chunks = Vec::new();
    for expected_sequence in 0..4 {
        let chunk = tokio::time::timeout(Duration::from_secs(1), adapter.next_event())
            .await
            .unwrap()
            .unwrap();
        let offset = expected_sequence * 3;
        assert_eq!(chunk.result_sequence, expected_sequence as u64 + 1);
        assert_eq!(chunk.result_tag(), Some("Chunk"));
        assert_eq!(
            number(&fields(&chunk)["sequence"]),
            expected_sequence as i64
        );
        assert_eq!(number(&fields(&chunk)["offset"]), offset as i64);
        assert_eq!(bytes(fields(&chunk), "bytes"), &content[offset..offset + 3]);
        queued_chunks.push(chunk);
    }
    assert_eq!(adapter.outstanding_credits(call_id), Some(0));
    assert!(
        tokio::time::timeout(Duration::from_millis(40), adapter.next_event())
            .await
            .is_err(),
        "the fifth chunk must wait after four outstanding chunks"
    );

    let opened_turn = apply_event(&mut program, &mut adapter, opened).unwrap();
    assert!(opened_turn.transient_effect_credit_grants.is_empty());
    assert!(
        tokio::time::timeout(Duration::from_millis(40), adapter.next_event())
            .await
            .is_err(),
        "an uncredited Opened event must not release a chunk"
    );

    let first_chunk = queued_chunks.remove(0);
    let first_chunk_turn = apply_event(&mut program, &mut adapter, first_chunk).unwrap();
    assert_eq!(first_chunk_turn.transient_effect_credit_grants.len(), 1);

    let fifth_chunk = adapter.next_event().await.unwrap();
    assert_eq!(fifth_chunk.result_sequence, 5);
    assert_eq!(fifth_chunk.result_tag(), Some("Chunk"));
    assert_eq!(number(&fields(&fifth_chunk)["sequence"]), 4);
    assert_eq!(number(&fields(&fifth_chunk)["offset"]), 12);
    assert_eq!(bytes(fields(&fifth_chunk), "bytes"), b"mno");
    assert_eq!(adapter.outstanding_credits(call_id), Some(0));

    let finished = adapter.next_event().await.unwrap();
    assert_eq!(finished.result_sequence, 6);
    assert_eq!(finished.result_tag(), Some("Finished"));
    assert!(finished.is_terminal());
    assert_eq!(
        number(&fields(&finished)["byte_count"]),
        content.len() as i64
    );
    assert_eq!(
        bytes(fields(&finished), "digest"),
        &Sha256::digest(content)[..]
    );
    let retained = variant(fields(&finished), "retained", "Retained");
    let content_ref = ContentRef::from_value(&retained["content"]).unwrap();
    assert_eq!(content_ref.size(), content.len() as u64);
    assert_eq!(content_ref.media(), "application/octet-stream");
    assert_eq!(
        content_ref.digest().as_slice(),
        &Sha256::digest(content)[..]
    );
    assert!(adapter.content_store().contains(&content_ref));

    for chunk in queued_chunks {
        apply_event(&mut program, &mut adapter, chunk).unwrap();
    }
    apply_event(&mut program, &mut adapter, fifth_chunk).unwrap();
    apply_event(&mut program, &mut adapter, finished).unwrap();

    assert_eq!(program.pending_transient_effect_count(), 0);
    assert_eq!(adapter.active_count(), 0);
    assert_eq!(adapter.owned_call_count(), 0);
    assert!(adapter.try_next_event().unwrap().is_none());
    assert_eq!(adapter.content_store().pending_writer_count(), 0);
}

#[tokio::test]
async fn stale_and_revoked_capabilities_end_as_typed_failures() {
    let selected = file(b"content");
    let mut registry = FileCapabilityRegistry::new(1).unwrap();
    let stale = registry.register_file(selected.path()).unwrap();
    let current = registry.replace_file(&stale, selected.path()).unwrap();
    assert!(!registry.contains(&stale));
    assert!(registry.contains(&current));
    let mut adapter = adapter(registry);
    let mut program = program();

    let stale_invocation = invocation(&mut program, stale.file_selected_value());
    assert!(adapter.submit(stale_invocation).unwrap().queued_terminal);
    let stale_failure = adapter.next_event().await.unwrap();
    assert_eq!(stale_failure.result_tag(), Some("Failed"));
    assert_eq!(text(fields(&stale_failure), "code"), "stale_capability");
    let raw_path = selected.path().display().to_string();
    assert!(!text(fields(&stale_failure), "diagnostic").contains(raw_path.as_str()));
    apply_event(&mut program, &mut adapter, stale_failure).unwrap();

    assert!(adapter.capabilities_mut().revoke(&current));
    assert!(!adapter.capabilities().contains(&current));
    let revoked_invocation = invocation(&mut program, current.file_selected_value());
    assert!(adapter.submit(revoked_invocation).unwrap().queued_terminal);
    let revoked_failure = adapter.next_event().await.unwrap();
    assert_eq!(revoked_failure.result_tag(), Some("Failed"));
    assert_eq!(text(fields(&revoked_failure), "code"), "unknown_capability");
    apply_event(&mut program, &mut adapter, revoked_failure).unwrap();
    assert_eq!(program.pending_transient_effect_count(), 0);
    assert!(adapter.try_next_event().unwrap().is_none());
}

#[tokio::test]
async fn forged_and_foreign_file_selections_fail_closed() {
    let selected = file(b"content");
    let mut first_registry = FileCapabilityRegistry::new(1).unwrap();
    let foreign = first_registry.register_file(selected.path()).unwrap();
    let second_registry = FileCapabilityRegistry::new(1).unwrap();
    let mut adapter = adapter(second_registry);
    let mut program = program();

    let foreign_invocation = invocation(&mut program, foreign.file_selected_value());
    assert!(adapter.submit(foreign_invocation).unwrap().queued_terminal);
    let foreign_failure = adapter.next_event().await.unwrap();
    assert_eq!(foreign_failure.result_tag(), Some("Failed"));
    assert_eq!(text(fields(&foreign_failure), "code"), "unknown_capability");
    apply_event(&mut program, &mut adapter, foreign_failure).unwrap();

    let forged = Value::Record(BTreeMap::from([(
        "$tag".to_owned(),
        Value::Text("FileSelected".to_owned()),
    )]));
    let forged_invocation = invocation(&mut program, forged);
    assert!(adapter.submit(forged_invocation).unwrap().queued_terminal);
    let forged_failure = adapter.next_event().await.unwrap();
    assert_eq!(forged_failure.result_tag(), Some("Failed"));
    assert_eq!(text(fields(&forged_failure), "code"), "invalid_intent");
    apply_event(&mut program, &mut adapter, forged_failure).unwrap();

    assert_eq!(program.pending_transient_effect_count(), 0);
    assert!(adapter.try_next_event().unwrap().is_none());
}

#[tokio::test]
async fn host_rejects_an_incomplete_intent_after_compiler_defaulting() {
    let selected = file(b"content");
    let mut registry = FileCapabilityRegistry::new(1).unwrap();
    let capability = registry.register_file(selected.path()).unwrap();
    let mut adapter = adapter(registry);
    let mut program = program();
    let mut invocation = invocation(&mut program, capability.file_selected_value());
    let Value::Record(intent_fields) = &mut invocation.intent else {
        panic!("stream intent must be a record");
    };
    intent_fields.remove("chunk_bytes");

    assert!(adapter.submit(invocation).unwrap().queued_terminal);
    let failed = adapter.next_event().await.unwrap();
    assert_eq!(failed.result_tag(), Some("Failed"));
    assert_eq!(text(fields(&failed), "code"), "invalid_intent");
    apply_event(&mut program, &mut adapter, failed).unwrap();
    assert_eq!(program.pending_transient_effect_count(), 0);
}

#[tokio::test]
async fn nonretained_stream_finishes_without_advertising_unresolvable_content() {
    let selected = file(b"unretained");
    let mut registry = FileCapabilityRegistry::new(1).unwrap();
    let capability = registry.register_file(selected.path()).unwrap();
    let mut adapter = adapter(registry);
    let mut program = program();
    let mut invocation = invocation(&mut program, capability.file_selected_value());
    let Value::Record(intent) = &mut invocation.intent else {
        panic!("stream intent must be a record");
    };
    intent.insert("retain_content".to_owned(), Value::Bool(false));
    adapter.submit(invocation).unwrap();

    loop {
        let event = adapter.next_event().await.unwrap();
        let terminal = event.is_terminal();
        if event.result_tag() == Some("Finished") {
            let retained = variant(fields(&event), "retained", "NotRetained");
            assert_eq!(retained.len(), 1);
        }
        apply_event(&mut program, &mut adapter, event).unwrap();
        if terminal {
            break;
        }
    }

    assert_eq!(adapter.content_store().entry_count(), 0);
    assert_eq!(adapter.content_store().pending_writer_count(), 0);
    assert_eq!(program.pending_transient_effect_count(), 0);
}

#[tokio::test]
async fn cancellation_closes_with_one_terminal_and_no_later_event() {
    let selected = file(b"abcdefghijklmnopqrstuvwxyz012345");
    let mut registry = FileCapabilityRegistry::new(1).unwrap();
    let capability = registry.register_file(selected.path()).unwrap();
    let mut adapter = adapter(registry);
    let mut program = program();
    let invocation = invocation(&mut program, capability.file_selected_value());
    let call_id = invocation.call_id;
    adapter.submit(invocation).unwrap();

    let opened = adapter.next_event().await.unwrap();
    let mut chunks = Vec::new();
    for expected_sequence in 0..4 {
        let chunk = adapter.next_event().await.unwrap();
        assert_eq!(chunk.result_tag(), Some("Chunk"));
        assert_eq!(number(&fields(&chunk)["sequence"]), expected_sequence);
        chunks.push(chunk);
    }
    assert_eq!(adapter.outstanding_credits(call_id), Some(0));
    assert!(adapter.request_cancel(call_id));

    let cancelled = tokio::time::timeout(Duration::from_secs(1), adapter.next_event())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cancelled.result_tag(), Some("Cancelled"));
    assert!(cancelled.is_terminal());
    apply_event(&mut program, &mut adapter, opened).unwrap();
    for chunk in chunks {
        apply_event(&mut program, &mut adapter, chunk).unwrap();
    }
    apply_event(&mut program, &mut adapter, cancelled).unwrap();

    assert_eq!(program.pending_transient_effect_count(), 0);
    assert_eq!(adapter.active_count(), 0);
    assert!(adapter.try_next_event().unwrap().is_none());
    assert_eq!(adapter.content_store().entry_count(), 0);
    assert_eq!(adapter.content_store().pending_writer_count(), 0);
}

#[test]
fn file_capability_is_hidden_from_ordinary_boon_data() {
    let selected_file = file(b"x");
    let selected_path = selected_file.path().display().to_string();
    let mut registry = FileCapabilityRegistry::new(1).unwrap();
    let capability = registry.register_file(selected_file.path()).unwrap();
    let selected = capability.file_selected_value();
    assert!(selected.host_binding().is_some());
    let Value::Record(visible) = selected.visible() else {
        panic!("visible selection must remain a structural tag");
    };
    assert_eq!(visible.len(), 1);
    assert_eq!(visible["$tag"], Value::Text("FileSelected".to_owned()));
    assert!(selected.to_data().is_err());
    assert_eq!(
        format!("{:?}", selected.host_binding().unwrap()),
        "HostValueBinding(<opaque>)"
    );
    assert!(!format!("{capability:?}").contains("generation"));
    assert!(!format!("{selected:?}").contains(&selected_path));
}
