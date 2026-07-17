use boon_host_runtime::{
    ContentRef, ContentStore, ContentStoreLimits, FileCapabilityRegistry,
    FileReadStreamEffectAdapter, FileReadStreamEvent, FileReadStreamLimits, apply_event,
    package_asset_value,
};
use boon_plan::{ApplicationIdentity, FiniteReal, ProgramRole};
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
    let retained = adapter.register_package_asset(url, content).unwrap();
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
            assert_eq!(
                ContentRef::from_value(&fields(&event)["content"]).unwrap(),
                retained
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

fn adapter(registry: FileCapabilityRegistry) -> FileReadStreamEffectAdapter {
    let root = tempfile::tempdir().unwrap().keep();
    let content_store = ContentStore::new(root, ContentStoreLimits::new(4, 1024 * 1024)).unwrap();
    FileReadStreamEffectAdapter::with_limits(
        registry,
        content_store,
        FileReadStreamLimits::new(1, 2, 1),
    )
    .unwrap()
}

fn fields(event: &FileReadStreamEvent) -> &BTreeMap<String, Value> {
    let Value::Record(fields) = &event.outcome else {
        panic!("stream outcome must be a tagged record");
    };
    fields
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
    let content = b"abcdefgh";
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

    let first_chunk = tokio::time::timeout(Duration::from_secs(1), adapter.next_event())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first_chunk.result_sequence, 1);
    assert_eq!(first_chunk.result_tag(), Some("Chunk"));
    assert_eq!(number(&fields(&first_chunk)["sequence"]), 0);
    assert_eq!(number(&fields(&first_chunk)["offset"]), 0);
    assert_eq!(bytes(fields(&first_chunk), "bytes"), b"abc");
    assert_eq!(adapter.outstanding_credits(call_id), Some(0));
    assert!(
        tokio::time::timeout(Duration::from_millis(40), adapter.next_event())
            .await
            .is_err(),
        "the worker must stop after consuming its two initial credits"
    );

    apply_event(&mut program, &mut adapter, opened).unwrap();
    apply_event(&mut program, &mut adapter, first_chunk).unwrap();

    let second_chunk = adapter.next_event().await.unwrap();
    assert_eq!(second_chunk.result_sequence, 2);
    assert_eq!(number(&fields(&second_chunk)["sequence"]), 1);
    assert_eq!(number(&fields(&second_chunk)["offset"]), 3);
    assert_eq!(bytes(fields(&second_chunk), "bytes"), b"def");
    apply_event(&mut program, &mut adapter, second_chunk).unwrap();

    let third_chunk = adapter.next_event().await.unwrap();
    assert_eq!(third_chunk.result_sequence, 3);
    assert_eq!(number(&fields(&third_chunk)["sequence"]), 2);
    assert_eq!(number(&fields(&third_chunk)["offset"]), 6);
    assert_eq!(bytes(fields(&third_chunk), "bytes"), b"gh");
    apply_event(&mut program, &mut adapter, third_chunk).unwrap();

    let finished = adapter.next_event().await.unwrap();
    assert_eq!(finished.result_sequence, 4);
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
    let content_ref = ContentRef::from_value(&fields(&finished)["content"]).unwrap();
    assert_eq!(content_ref.byte_count(), content.len() as u64);
    assert_eq!(
        content_ref.digest().as_slice(),
        &Sha256::digest(content)[..]
    );
    assert!(adapter.content_store().contains(content_ref));
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
async fn cancellation_closes_with_one_terminal_and_no_later_event() {
    let selected = file(b"abcdefghijkl");
    let mut registry = FileCapabilityRegistry::new(1).unwrap();
    let capability = registry.register_file(selected.path()).unwrap();
    let mut adapter = adapter(registry);
    let mut program = program();
    let invocation = invocation(&mut program, capability.file_selected_value());
    let call_id = invocation.call_id;
    adapter.submit(invocation).unwrap();

    let opened = adapter.next_event().await.unwrap();
    let first_chunk = adapter.next_event().await.unwrap();
    assert!(adapter.request_cancel(call_id));
    apply_event(&mut program, &mut adapter, opened).unwrap();
    apply_event(&mut program, &mut adapter, first_chunk).unwrap();

    let cancelled = tokio::time::timeout(Duration::from_secs(1), adapter.next_event())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cancelled.result_sequence, 2);
    assert_eq!(cancelled.result_tag(), Some("Cancelled"));
    assert!(cancelled.is_terminal());
    apply_event(&mut program, &mut adapter, cancelled).unwrap();

    assert_eq!(program.pending_transient_effect_count(), 0);
    assert_eq!(adapter.active_count(), 0);
    assert!(adapter.try_next_event().unwrap().is_none());
    assert_eq!(adapter.content_store().entry_count(), 0);
    assert_eq!(adapter.content_store().pending_writer_count(), 0);
}

#[test]
fn capability_generation_is_a_positive_exact_boon_number() {
    let selected = file(b"x");
    let mut registry = FileCapabilityRegistry::new(1).unwrap();
    let capability = registry.register_file(selected.path()).unwrap();
    let Value::Record(selected) = capability.file_selected_value() else {
        panic!("selection must be a variant record");
    };
    let Value::Record(capability) = &selected["capability"] else {
        panic!("capability must be a record");
    };
    assert_eq!(number(&capability["generation"]), 1);
    assert_eq!(bytes(capability, "token").len(), 32);
    assert_ne!(capability["generation"], Value::Number(FiniteReal::ZERO),);
}
