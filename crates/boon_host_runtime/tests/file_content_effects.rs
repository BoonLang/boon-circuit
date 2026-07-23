use boon_host_runtime::{
    ContentRef, ContentStore, ContentStoreLimits, FileCapabilityRegistry, FileEffectAdapter,
    FileEffectEvent, FileEffectLimits, apply_event,
};
use boon_plan::{ApplicationIdentity, ProgramRole};
use boon_runtime::{
    ProgramArtifact, ProgramCapabilityProfile, ProgramCompileRequest, ProgramSession,
    RuntimeSourceUnit, SourcePayload, TransientEffectInvocation, Value, compile_program_artifact,
};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::sync::OnceLock;
use std::time::Duration;
use tempfile::NamedTempFile;

const PROGRAM: &str = r#"
store: [
    read: SOURCE
    write: SOURCE
    import: SOURCE
    save: SOURCE
    bytes: BYTES[5] { 16u68, 16u65, 16u6c, 16u6c, 16u6f }
    read_result:
        NotStarted |> HOLD read_result {
            store.read |> THEN {
                File/read_bytes(file: store.read.file, max_bytes: 1048576)
            }
        }
    write_result:
        NotStarted |> HOLD write_result {
            store.write |> THEN {
                File/write_bytes(file: store.write.file, bytes: store.bytes)
            }
        }
    import_result:
        NotStarted |> HOLD import_result {
            store.import |> THEN {
                Content/import(file: store.import.file)
            }
        }
    save_result:
        NotStarted |> HOLD save_result {
            store.save |> THEN {
                Content/save(content: store.save.content, file: store.save.file)
            }
        }
]

outputs: [
    read_result: store.read_result
    write_result: store.write_result
    import_result: store.import_result
    save_result: store.save_result
]

document: Document/new(
    root: Element/label(
        element: []
        label: TEXT { File content effects }
    )
)
"#;

fn program() -> ProgramSession {
    static ARTIFACT: OnceLock<ProgramArtifact> = OnceLock::new();
    let artifact = ARTIFACT.get_or_init(|| {
        compile_program_artifact(&ProgramCompileRequest {
            revision: 1,
            entry_path: "file-content-effects.bn".to_owned(),
            units: vec![RuntimeSourceUnit {
                path: "file-content-effects.bn".to_owned(),
                source: PROGRAM.to_owned(),
            }],
            application: ApplicationIdentity::new("dev.boon.file-content-effects", "test", "local"),
            role: ProgramRole::Client,
            capability_profile: ProgramCapabilityProfile::PublicClient,
        })
        .unwrap()
    });
    ProgramSession::start(artifact.clone()).unwrap()
}

fn adapter(registry: FileCapabilityRegistry) -> FileEffectAdapter {
    let root = tempfile::tempdir().unwrap().keep();
    let store = ContentStore::new(root, ContentStoreLimits::new(16, 32 * 1024 * 1024)).unwrap();
    FileEffectAdapter::with_limits(registry, store, FileEffectLimits::new(4, 32, 4)).unwrap()
}

fn dispatch(
    program: &mut ProgramSession,
    sequence: u64,
    source: &str,
    fields: BTreeMap<String, Value>,
) -> TransientEffectInvocation {
    let dispatched = program
        .dispatch(
            source,
            None,
            SourcePayload {
                fields,
                ..SourcePayload::default()
            },
        )
        .unwrap();
    let [invocation] = dispatched.runtime_turn.transient_effects.as_slice() else {
        panic!("{source} must emit exactly one effect invocation at sequence {sequence}");
    };
    invocation.clone()
}

fn source_file(bytes: &[u8]) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
    file
}

fn tag(event: &FileEffectEvent) -> &str {
    event.result_tag().expect("effect outcome tag")
}

fn fields(event: &FileEffectEvent) -> &BTreeMap<String, Value> {
    let Value::Record(fields) = &event.outcome else {
        panic!("effect outcome must be a tagged record");
    };
    fields
}

fn text<'a>(fields: &'a BTreeMap<String, Value>, name: &str) -> &'a str {
    let Value::Text(value) = &fields[name] else {
        panic!("{name} must be Text");
    };
    value
}

fn bytes<'a>(fields: &'a BTreeMap<String, Value>, name: &str) -> &'a [u8] {
    let Value::Bytes(value) = &fields[name] else {
        panic!("{name} must be Bytes");
    };
    value
}

async fn next(adapter: &mut FileEffectAdapter) -> FileEffectEvent {
    tokio::time::timeout(Duration::from_secs(5), adapter.next_event())
        .await
        .expect("bounded file operation timed out")
        .unwrap()
}

async fn settle_retired_workers(adapter: &mut FileEffectAdapter) {
    for _ in 0..100 {
        assert!(adapter.try_next_event().unwrap().is_none());
        if adapter.retired_worker_count() == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("retired file worker did not release its resources");
}

#[tokio::test]
async fn bounded_read_and_atomic_write_use_directional_host_targets() {
    let input = source_file(b"source bytes");
    let target = source_file(b"old target");
    let target_path = target.path().to_path_buf();
    let mut registry = FileCapabilityRegistry::new(4).unwrap();
    let source = registry.register_file(input.path()).unwrap();
    let destination = registry.register_target(&target_path).unwrap();
    let mut adapter = adapter(registry);
    let mut program = program();

    let read = dispatch(
        &mut program,
        1,
        "store.read",
        BTreeMap::from([("file".to_owned(), source.file_selected_value())]),
    );
    adapter.submit(read).unwrap();
    let read = next(&mut adapter).await;
    assert_eq!(tag(&read), "BytesRead");
    assert!(!read.is_stream());
    assert_eq!(bytes(fields(&read), "bytes"), b"source bytes");
    apply_event(&mut program, &mut adapter, read).unwrap();

    let write = dispatch(
        &mut program,
        2,
        "store.write",
        BTreeMap::from([("file".to_owned(), destination.file_target_value())]),
    );
    adapter.submit(write).unwrap();
    let write = next(&mut adapter).await;
    assert_eq!(tag(&write), "BytesWritten");
    assert!(!write.is_stream());
    apply_event(&mut program, &mut adapter, write).unwrap();
    assert_eq!(fs::read(target_path).unwrap(), b"hello");
}

#[tokio::test]
async fn same_target_is_busy_until_the_active_atomic_operation_is_observed() {
    let target = source_file(b"old target");
    let mut registry = FileCapabilityRegistry::new(2).unwrap();
    let destination = registry.register_target(target.path()).unwrap();
    let destination_alias = registry.register_target(target.path()).unwrap();
    let mut adapter = adapter(registry);
    let mut program = program();

    let first = dispatch(
        &mut program,
        1,
        "store.write",
        BTreeMap::from([("file".to_owned(), destination.file_target_value())]),
    );
    adapter.submit(first).unwrap();
    let second = dispatch(
        &mut program,
        2,
        "store.write",
        BTreeMap::from([("file".to_owned(), destination_alias.file_target_value())]),
    );
    adapter.submit(second).unwrap();

    let mut observed_busy = false;
    let mut observed_written = false;
    for _ in 0..2 {
        let event = next(&mut adapter).await;
        observed_busy |= tag(&event) == "Busy";
        observed_written |= tag(&event) == "BytesWritten";
    }
    assert!(observed_busy);
    assert!(observed_written);

    let third = dispatch(
        &mut program,
        3,
        "store.write",
        BTreeMap::from([("file".to_owned(), destination.file_target_value())]),
    );
    assert!(!adapter.submit(third).unwrap().queued_terminal);
    assert_eq!(tag(&next(&mut adapter).await), "BytesWritten");
}

#[tokio::test]
async fn credit_starved_operation_times_out_and_releases_pending_content() {
    let payload = vec![0x71; 9 * 64 * 1024];
    let input = source_file(&payload);
    let mut registry = FileCapabilityRegistry::new(1).unwrap();
    let source = registry.register_file(input.path()).unwrap();
    let root = tempfile::tempdir().unwrap().keep();
    let store = ContentStore::new(root, ContentStoreLimits::new(4, 2 * 1024 * 1024)).unwrap();
    let limits = FileEffectLimits::new(1, 8, 4).with_operation_timeout(Duration::from_millis(500));
    let mut adapter = FileEffectAdapter::with_limits(registry, store, limits).unwrap();
    let mut program = program();

    let import = dispatch(
        &mut program,
        1,
        "store.import",
        BTreeMap::from([("file".to_owned(), source.file_selected_value())]),
    );
    adapter.submit(import).unwrap();
    assert_eq!(tag(&next(&mut adapter).await), "Started");
    for _ in 0..4 {
        assert_eq!(tag(&next(&mut adapter).await), "Progress");
    }

    let timed_out = next(&mut adapter).await;
    assert_eq!(tag(&timed_out), "Failed");
    assert_eq!(text(fields(&timed_out), "code"), "timeout");
    assert!(timed_out.is_terminal());
    assert_eq!(adapter.active_count(), 0);
    assert_eq!(adapter.content_store().pending_writer_count(), 0);
    settle_retired_workers(&mut adapter).await;
    assert_eq!(adapter.retired_worker_count(), 0);
}

#[tokio::test]
async fn bounded_file_operations_reject_oversize_and_relabelled_wrong_direction_bindings() {
    let input = source_file(b"too large");
    let target = source_file(b"old target");
    let mut registry = FileCapabilityRegistry::new(4).unwrap();
    let source = registry.register_file(input.path()).unwrap();
    let destination = registry.register_target(target.path()).unwrap();
    let mut adapter = adapter(registry);
    let mut program = program();

    let mut read = dispatch(
        &mut program,
        1,
        "store.read",
        BTreeMap::from([("file".to_owned(), source.file_selected_value())]),
    );
    let Value::Record(read_intent) = &mut read.intent else {
        panic!("read intent must be a record");
    };
    read_intent.insert("max_bytes".to_owned(), Value::integer(4).unwrap());
    adapter.submit(read).unwrap();
    let too_large = next(&mut adapter).await;
    assert_eq!(tag(&too_large), "Failed");
    assert_eq!(text(fields(&too_large), "code"), "file_too_large");

    let wrong_source = Value::host_bound(
        Value::Record(BTreeMap::from([(
            "$tag".to_owned(),
            Value::Text("FileSelected".to_owned()),
        )])),
        destination
            .file_target_value()
            .host_binding()
            .unwrap()
            .clone(),
    );
    let read = dispatch(
        &mut program,
        2,
        "store.read",
        BTreeMap::from([("file".to_owned(), wrong_source)]),
    );
    adapter.submit(read).unwrap();
    let wrong_source = next(&mut adapter).await;
    assert_eq!(tag(&wrong_source), "Failed");
    assert_eq!(
        text(fields(&wrong_source), "code"),
        "wrong_capability_access"
    );
    assert!(
        !text(fields(&wrong_source), "diagnostic").contains(&input.path().display().to_string())
    );

    let wrong_target = Value::host_bound(
        Value::Record(BTreeMap::from([(
            "$tag".to_owned(),
            Value::Text("FileTarget".to_owned()),
        )])),
        source.file_selected_value().host_binding().unwrap().clone(),
    );
    let write = dispatch(
        &mut program,
        3,
        "store.write",
        BTreeMap::from([("file".to_owned(), wrong_target)]),
    );
    adapter.submit(write).unwrap();
    let wrong_target = next(&mut adapter).await;
    assert_eq!(tag(&wrong_target), "Failed");
    assert_eq!(
        text(fields(&wrong_target), "code"),
        "wrong_capability_access"
    );
    assert!(
        !text(fields(&wrong_target), "diagnostic").contains(&target.path().display().to_string())
    );
}

#[tokio::test]
async fn content_import_and_save_obey_credit_backpressure_and_preserve_bytes() {
    let payload = vec![0x5a; 5 * 64 * 1024 + 17];
    let input = source_file(&payload);
    let target = source_file(b"old target");
    let target_path = target.path().to_path_buf();
    let mut registry = FileCapabilityRegistry::new(4).unwrap();
    let source = registry.register_file(input.path()).unwrap();
    let destination = registry.register_target(&target_path).unwrap();
    let mut adapter = adapter(registry);
    let mut program = program();

    let import = dispatch(
        &mut program,
        1,
        "store.import",
        BTreeMap::from([("file".to_owned(), source.file_selected_value())]),
    );
    adapter.submit(import).unwrap();
    let started = next(&mut adapter).await;
    assert_eq!(tag(&started), "Started");
    assert!(started.is_stream());
    apply_event(&mut program, &mut adapter, started).unwrap();

    let mut held_progress = Vec::new();
    for _ in 0..4 {
        let progress = next(&mut adapter).await;
        assert_eq!(tag(&progress), "Progress");
        held_progress.push(progress);
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(40), adapter.next_event())
            .await
            .is_err(),
        "the fifth progress event must wait for returned credit"
    );
    for progress in held_progress {
        apply_event(&mut program, &mut adapter, progress).unwrap();
    }

    let content = loop {
        let event = next(&mut adapter).await;
        if tag(&event) == "Imported" {
            let content = ContentRef::from_value(&fields(&event)["content"]).unwrap();
            apply_event(&mut program, &mut adapter, event).unwrap();
            break content;
        }
        assert_eq!(tag(&event), "Progress");
        apply_event(&mut program, &mut adapter, event).unwrap();
    };

    let save = dispatch(
        &mut program,
        2,
        "store.save",
        BTreeMap::from([
            ("content".to_owned(), content.value().unwrap()),
            ("file".to_owned(), destination.file_target_value()),
        ]),
    );
    adapter.submit(save).unwrap();
    loop {
        let event = next(&mut adapter).await;
        let terminal = event.is_terminal();
        if tag(&event) == "Failed" {
            panic!(
                "content save failed: {}",
                text(fields(&event), "diagnostic")
            );
        }
        apply_event(&mut program, &mut adapter, event).unwrap();
        if terminal {
            break;
        }
    }
    assert_eq!(fs::read(target_path).unwrap(), payload);
}

#[tokio::test]
async fn credit_starved_content_streams_cancel_once_and_release_owned_resources() {
    let payload = vec![0x3c; 9 * 64 * 1024];
    let input = source_file(&payload);
    let target = source_file(b"old target");
    let target_path = target.path().to_path_buf();
    let mut registry = FileCapabilityRegistry::new(4).unwrap();
    let source = registry.register_file(input.path()).unwrap();
    let destination = registry.register_target(&target_path).unwrap();
    let mut adapter = adapter(registry);
    let mut program = program();

    let import = dispatch(
        &mut program,
        1,
        "store.import",
        BTreeMap::from([("file".to_owned(), source.file_selected_value())]),
    );
    let import_call = import.call_id;
    adapter.submit(import).unwrap();
    let mut import_events = vec![next(&mut adapter).await];
    for _ in 0..4 {
        import_events.push(next(&mut adapter).await);
    }
    assert_eq!(tag(&import_events[0]), "Started");
    assert!(
        import_events[1..]
            .iter()
            .all(|event| tag(event) == "Progress")
    );
    assert!(adapter.request_cancel(import_call));
    let cancelled = next(&mut adapter).await;
    assert_eq!(tag(&cancelled), "Cancelled");
    assert!(cancelled.is_terminal());
    for event in import_events {
        apply_event(&mut program, &mut adapter, event).unwrap();
    }
    apply_event(&mut program, &mut adapter, cancelled).unwrap();
    assert_eq!(adapter.content_store().pending_writer_count(), 0);
    settle_retired_workers(&mut adapter).await;
    assert!(adapter.try_next_event().unwrap().is_none());

    let content = adapter
        .content_store()
        .insert_bytes(&payload, "application/octet-stream")
        .unwrap();
    let save = dispatch(
        &mut program,
        2,
        "store.save",
        BTreeMap::from([
            ("content".to_owned(), content.value().unwrap()),
            ("file".to_owned(), destination.file_target_value()),
        ]),
    );
    let save_call = save.call_id;
    adapter.submit(save).unwrap();
    let mut save_events = vec![next(&mut adapter).await];
    for _ in 0..4 {
        save_events.push(next(&mut adapter).await);
    }
    assert_eq!(tag(&save_events[0]), "Started");
    assert!(
        save_events[1..]
            .iter()
            .all(|event| tag(event) == "Progress")
    );
    assert!(adapter.request_cancel(save_call));
    let cancelled = next(&mut adapter).await;
    assert_eq!(tag(&cancelled), "Cancelled");
    assert!(cancelled.is_terminal());
    for event in save_events {
        apply_event(&mut program, &mut adapter, event).unwrap();
    }
    apply_event(&mut program, &mut adapter, cancelled).unwrap();
    assert_eq!(fs::read(&target_path).unwrap(), b"old target");
    settle_retired_workers(&mut adapter).await;
    assert!(adapter.try_next_event().unwrap().is_none());

    let retry = dispatch(
        &mut program,
        3,
        "store.save",
        BTreeMap::from([
            ("content".to_owned(), content.value().unwrap()),
            ("file".to_owned(), destination.file_target_value()),
        ]),
    );
    assert!(!adapter.submit(retry).unwrap().queued_terminal);
    loop {
        let event = next(&mut adapter).await;
        let terminal = event.is_terminal();
        let result_tag = tag(&event).to_owned();
        apply_event(&mut program, &mut adapter, event).unwrap();
        if terminal {
            assert_eq!(result_tag, "Saved");
            break;
        }
    }
    assert_eq!(fs::read(target_path).unwrap(), payload);
}

#[tokio::test]
async fn corrupt_content_never_replaces_the_existing_target() {
    let payload = b"durable content";
    let target = source_file(b"keep me");
    let target_path = target.path().to_path_buf();
    let root = tempfile::tempdir().unwrap().keep();
    let store = ContentStore::new(&root, ContentStoreLimits::new(4, 1024 * 1024)).unwrap();
    let content = store
        .insert_bytes(payload, "application/octet-stream")
        .unwrap();
    let lease = store.resolve(&content).unwrap();
    fs::write(lease.path(), b"corrupt").unwrap();

    let mut registry = FileCapabilityRegistry::new(2).unwrap();
    let destination = registry.register_target(&target_path).unwrap();
    let mut adapter =
        FileEffectAdapter::with_limits(registry, store, FileEffectLimits::new(2, 8, 4)).unwrap();
    let mut program = program();
    let save = dispatch(
        &mut program,
        1,
        "store.save",
        BTreeMap::from([
            ("content".to_owned(), content.value().unwrap()),
            ("file".to_owned(), destination.file_target_value()),
        ]),
    );
    adapter.submit(save).unwrap();
    let failed = loop {
        let event = next(&mut adapter).await;
        if event.is_terminal() {
            break event;
        }
        apply_event(&mut program, &mut adapter, event).unwrap();
    };
    assert_eq!(tag(&failed), "Failed");
    assert_eq!(text(fields(&failed), "code"), "content_corrupt");
    assert_eq!(fs::read(target_path).unwrap(), b"keep me");
}

#[cfg(unix)]
#[tokio::test]
async fn failed_atomic_write_leaves_the_previous_target_unchanged() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let target_path = directory.path().join("target.bin");
    fs::write(&target_path, b"keep me").unwrap();
    let mut registry = FileCapabilityRegistry::new(1).unwrap();
    let destination = registry.register_target(&target_path).unwrap();
    let mut adapter = adapter(registry);
    let mut program = program();

    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let write = dispatch(
        &mut program,
        1,
        "store.write",
        BTreeMap::from([("file".to_owned(), destination.file_target_value())]),
    );
    adapter.submit(write).unwrap();
    let failed = next(&mut adapter).await;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();

    assert_eq!(tag(&failed), "Failed");
    assert_eq!(fs::read(target_path).unwrap(), b"keep me");
}
