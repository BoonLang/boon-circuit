use boon_host_runtime::{
    ContentRef, ContentStore, ContentStoreLimits, FileCapabilityRegistry, FileEffectAdapter,
    apply_event as apply_file_event,
};
use boon_plan::{ApplicationIdentity, FiniteReal, ProgramRole};
use boon_runtime::{
    ProgramCapabilityProfile, ProgramCompileRequest, ProgramSession, RuntimeSourceUnit,
    SourcePayload, TransientEffectInvocation, Value, compile_program_artifact,
};
use boon_wellen_host::{WaveformEffectAdapter, apply_waveform_completion};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

const FILE_STREAM_PROGRAM: &str = r#"
store: [
    request: SOURCE
    result:
        NotStarted |> HOLD result {
            request |> THEN {
                File/read_stream(
                    file: request.file
                    chunk_bytes: 127
                    retain_content: True
                )
            }
        }
]
outputs: [
    result: store.result
]
"#;

const OPEN_PROGRAM: &str = r#"
store: [
    request: SOURCE
    result:
        NotStarted |> HOLD result {
            request |> THEN { Wellen/open(content: request.content) }
        }
]
outputs: [
    result: store.result
]
"#;

const HIERARCHY_PROGRAM: &str = r#"
store: [
    request: SOURCE
    result:
        NotStarted |> HOLD result {
            request |> THEN {
                Wellen/hierarchy_page(
                    artifact: request.artifact
                    request_fingerprint: request.request_fingerprint
                    offset: request.offset
                    limit: request.limit
                )
            }
        }
]
outputs: [
    result: store.result
]
"#;

const SIGNAL_PROGRAM: &str = r#"
store: [
    request: SOURCE
    result:
        NotStarted |> HOLD result {
            request |> THEN {
                Wellen/signal_page(
                    artifact: request.artifact
                    request_fingerprint: request.request_fingerprint
                    signal_ids: request.signal_ids
                    start_time: request.start_time
                    end_time: request.end_time
                    offset: request.offset
                    max_transitions: request.max_transitions
                )
            }
        }
]
outputs: [
    result: store.result
]
"#;

const CURSOR_PROGRAM: &str = r#"
store: [
    request: SOURCE
    result:
        NotStarted |> HOLD result {
            request |> THEN {
                Wellen/cursor_values(
                    artifact: request.artifact
                    request_fingerprint: request.request_fingerprint
                    cursor_time: request.cursor_time
                    signal_ids: request.signal_ids
                )
            }
        }
]
outputs: [
    result: store.result
]
"#;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/novywave/fixtures")
        .join(name)
}

fn program(source: &str, name: &str) -> ProgramSession {
    let artifact = compile_program_artifact(&ProgramCompileRequest {
        revision: 1,
        entry_path: format!("{name}.bn"),
        units: vec![RuntimeSourceUnit {
            path: format!("{name}.bn"),
            source: source.to_owned(),
        }],
        application: ApplicationIdentity::new(format!("dev.boon.waveform.{name}"), "test", "local"),
        role: ProgramRole::Server,
        capability_profile: ProgramCapabilityProfile::TrustedServer,
    })
    .unwrap();
    ProgramSession::start(artifact).unwrap()
}

fn invoke(
    program: &mut ProgramSession,
    fields: BTreeMap<String, Value>,
) -> TransientEffectInvocation {
    let dispatched = program
        .dispatch(
            "store.request",
            None,
            SourcePayload {
                fields,
                ..SourcePayload::default()
            },
        )
        .unwrap();
    let [invocation] = dispatched.runtime_turn.transient_effects.as_slice() else {
        panic!("request must emit exactly one typed effect");
    };
    invocation.clone()
}

fn submit_waveform(
    adapter: &mut WaveformEffectAdapter,
    program: &mut ProgramSession,
    fields: BTreeMap<String, Value>,
) -> Value {
    let invocation = invoke(program, fields);
    let completion = adapter.submit(invocation).unwrap().completion;
    let outcome = completion.outcome.clone();
    apply_waveform_completion(program, completion).unwrap();
    assert_eq!(program.pending_transient_effect_count(), 0);
    outcome
}

async fn materialize(
    adapter: &mut FileEffectAdapter,
    program: &mut ProgramSession,
    selected_file: Value,
) -> ContentRef {
    let invocation = invoke(
        program,
        BTreeMap::from([("file".to_owned(), selected_file)]),
    );
    adapter.submit(invocation).unwrap();
    loop {
        let event = adapter.next_event().await.unwrap();
        let terminal = event.is_terminal();
        let content = (event.result_tag() == Some("Finished")).then(|| {
            let Value::Record(fields) = &event.outcome else {
                panic!("finished event must be a record");
            };
            let Value::Record(retained) = &fields["retained"] else {
                panic!("finished retention must be a tagged record");
            };
            assert_eq!(retained["$tag"], Value::Text("Retained".to_owned()));
            let content = ContentRef::from_value(&retained["content"]).unwrap();
            assert_eq!(
                fields["digest"],
                Value::Bytes(content.digest().to_vec().into())
            );
            content
        });
        apply_file_event(program, adapter, event).unwrap();
        if terminal {
            return content.expect("real fixture stream must finish successfully");
        }
    }
}

fn number(value: i64) -> Value {
    Value::Number(FiniteReal::from_i64_exact(value).unwrap())
}

fn fields<'a>(value: &'a Value, expected_tag: &str) -> &'a BTreeMap<String, Value> {
    let Value::Record(fields) = value else {
        panic!("effect outcome must be a tagged record");
    };
    assert_eq!(
        fields.get("$tag"),
        Some(&Value::Text(expected_tag.to_owned()))
    );
    fields
}

fn integer(value: &Value) -> i64 {
    let Value::Number(value) = value else {
        panic!("field must be Number");
    };
    value.to_i64_exact().unwrap()
}

fn content_store(max_entries: usize) -> ContentStore {
    ContentStore::new(
        tempfile::tempdir().unwrap().keep(),
        ContentStoreLimits::new(max_entries, 8 * 1024 * 1024),
    )
    .unwrap()
}

#[tokio::test]
async fn real_vcd_fst_and_ghw_stream_then_open_without_filename_dispatch() {
    let mut anonymous_vcd = NamedTempFile::new().unwrap();
    anonymous_vcd
        .write_all(&fs::read(fixture("simple.vcd")).unwrap())
        .unwrap();
    anonymous_vcd.flush().unwrap();
    let paths = [
        (anonymous_vcd.path().to_path_buf(), "VCD"),
        (fixture("basic_test.fst"), "FST"),
        (fixture("simple_test.ghw"), "GHW"),
    ];
    let mut registry = FileCapabilityRegistry::new(paths.len()).unwrap();
    let selected = paths
        .iter()
        .map(|(path, expected)| {
            (
                registry.register_file(path).unwrap().file_selected_value(),
                *expected,
                fs::metadata(path).unwrap().len(),
                Sha256::digest(fs::read(path).unwrap()),
            )
        })
        .collect::<Vec<_>>();
    let store = content_store(paths.len());
    let mut stream = FileEffectAdapter::new(registry, store.clone(), 1).unwrap();
    let mut file_program = program(FILE_STREAM_PROGRAM, "stream-real-formats");
    let mut wellen = WaveformEffectAdapter::new(store.clone(), paths.len()).unwrap();
    let mut open = program(OPEN_PROGRAM, "open-real-formats");
    let mut hierarchy = program(HIERARCHY_PROGRAM, "hierarchy-real-formats");
    let mut signal = program(SIGNAL_PROGRAM, "signal-real-formats");

    for (file, expected_format, expected_len, expected_digest) in selected {
        let content = materialize(&mut stream, &mut file_program, file).await;
        assert_eq!(content.size(), expected_len);
        assert_eq!(content.digest().as_slice(), &expected_digest[..]);
        let outcome = submit_waveform(
            &mut wellen,
            &mut open,
            BTreeMap::from([("content".to_owned(), content.value().unwrap())]),
        );
        let opened = fields(&outcome, "WaveformOpened");
        assert_eq!(opened["format"], Value::Text(expected_format.to_owned()));
        assert_eq!(integer(&opened["byte_length"]), expected_len as i64);
        assert!(integer(&opened["signal_count"]) > 0);
        assert!(integer(&opened["hierarchy_bytes"]) > 0);
        let artifact = opened["artifact"].clone();
        let hierarchy_page = submit_waveform(
            &mut wellen,
            &mut hierarchy,
            BTreeMap::from([
                ("artifact".to_owned(), artifact.clone()),
                (
                    "request_fingerprint".to_owned(),
                    Value::Text(format!("{expected_format}:hierarchy")),
                ),
                ("offset".to_owned(), number(0)),
                ("limit".to_owned(), number(256)),
            ]),
        );
        let hierarchy_page = fields(&hierarchy_page, "HierarchyPage");
        let Value::List(rows) = &hierarchy_page["rows"] else {
            panic!("{expected_format} hierarchy rows must be a List");
        };
        let signal_id = rows
            .iter()
            .find_map(|row| match row {
                Value::Record(row)
                    if row.get("kind") == Some(&Value::Text("Signal".to_owned())) =>
                {
                    row.get("signal_id").cloned()
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{expected_format} hierarchy has no signal row"));
        let signal_page = submit_waveform(
            &mut wellen,
            &mut signal,
            BTreeMap::from([
                ("artifact".to_owned(), artifact),
                (
                    "request_fingerprint".to_owned(),
                    Value::Text(format!("{expected_format}:signal")),
                ),
                ("signal_ids".to_owned(), Value::List(vec![signal_id])),
                ("start_time".to_owned(), opened["start_time"].clone()),
                ("end_time".to_owned(), opened["end_time"].clone()),
                ("offset".to_owned(), number(0)),
                ("max_transitions".to_owned(), number(256)),
            ]),
        );
        let signal_page = fields(&signal_page, "SignalPage");
        assert!(matches!(signal_page.get("signals"), Some(Value::List(rows)) if !rows.is_empty()));
    }
    assert_eq!(store.entry_count(), 3);
    assert_eq!(store.pending_writer_count(), 0);
    assert_eq!(wellen.cached_waveform_count(), 3);
}

#[tokio::test]
async fn real_content_drives_bounded_typed_pages_and_survives_parser_cache_eviction() {
    let paths = [fixture("simple.vcd"), fixture("basic_test.fst")];
    let mut registry = FileCapabilityRegistry::new(paths.len()).unwrap();
    let selected = paths
        .iter()
        .map(|path| registry.register_file(path).unwrap().file_selected_value())
        .collect::<Vec<_>>();
    let store = content_store(paths.len());
    let mut stream = FileEffectAdapter::new(registry, store.clone(), 1).unwrap();
    let mut file_program = program(FILE_STREAM_PROGRAM, "stream-page-content");
    let first = materialize(&mut stream, &mut file_program, selected[0].clone()).await;
    let second = materialize(&mut stream, &mut file_program, selected[1].clone()).await;
    let mut wellen = WaveformEffectAdapter::new(store.clone(), 1).unwrap();
    let mut open = program(OPEN_PROGRAM, "open-page-content");
    let first_opened = submit_waveform(
        &mut wellen,
        &mut open,
        BTreeMap::from([("content".to_owned(), first.value().unwrap())]),
    );
    let first_opened = fields(&first_opened, "WaveformOpened").clone();
    let first_artifact = first_opened["artifact"].clone();
    submit_waveform(
        &mut wellen,
        &mut open,
        BTreeMap::from([("content".to_owned(), second.value().unwrap())]),
    );
    assert_eq!(wellen.cached_waveform_count(), 1);

    let mut hierarchy = program(HIERARCHY_PROGRAM, "hierarchy-page");
    let hierarchy_page = submit_waveform(
        &mut wellen,
        &mut hierarchy,
        BTreeMap::from([
            ("artifact".to_owned(), first_artifact.clone()),
            (
                "request_fingerprint".to_owned(),
                Value::Text("hierarchy:0".to_owned()),
            ),
            ("offset".to_owned(), number(0)),
            ("limit".to_owned(), number(256)),
        ]),
    );
    let hierarchy_page = fields(&hierarchy_page, "HierarchyPage");
    assert_eq!(
        hierarchy_page["request_fingerprint"],
        Value::Text("hierarchy:0".to_owned())
    );
    let Value::List(rows) = &hierarchy_page["rows"] else {
        panic!("hierarchy rows must be a list");
    };
    let signal_id = rows
        .iter()
        .find_map(|row| {
            let Value::Record(row) = row else {
                return None;
            };
            (row.get("kind") == Some(&Value::Text("Signal".to_owned())))
                .then(|| row["signal_id"].clone())
        })
        .expect("real VCD hierarchy must expose a signal");
    assert!(rows.iter().all(|row| {
        matches!(row, Value::Record(fields) if matches!(fields.get("full_name"), Some(Value::Text(_))))
    }));
    assert!(rows.iter().any(|row| {
        matches!(row, Value::Record(fields)
            if fields.get("kind") == Some(&Value::Text("Signal".to_owned()))
                && fields.get("full_name") == fields.get("signal_id"))
    }));
    assert_eq!(wellen.cached_waveform_count(), 1);

    let mut signal = program(SIGNAL_PROGRAM, "signal-page");
    let signal_page = submit_waveform(
        &mut wellen,
        &mut signal,
        BTreeMap::from([
            ("artifact".to_owned(), first_artifact.clone()),
            (
                "request_fingerprint".to_owned(),
                Value::Text("signal:0".to_owned()),
            ),
            (
                "signal_ids".to_owned(),
                Value::List(vec![signal_id.clone()]),
            ),
            ("start_time".to_owned(), number(0)),
            ("end_time".to_owned(), first_opened["end_time"].clone()),
            ("offset".to_owned(), number(0)),
            ("max_transitions".to_owned(), number(1)),
        ]),
    );
    let signal_page = fields(&signal_page, "SignalPage");
    let Value::List(signals) = &signal_page["signals"] else {
        panic!("signal page must contain signal rows");
    };
    let Value::Record(first_signal) = &signals[0] else {
        panic!("signal row must be a record");
    };
    let Value::List(transitions) = &first_signal["transitions"] else {
        panic!("signal transitions must be a list");
    };
    assert_eq!(transitions.len(), 1);
    let Value::Record(transition) = &transitions[0] else {
        panic!("transition must be a record");
    };
    let first_transition_time = transition["time"].clone();
    assert!(integer(&transition["end_time"]) >= integer(&transition["time"]));
    let Value::Record(value) = &transition["value"] else {
        panic!("transition value must be a closed variant");
    };
    assert!(matches!(value.get("$tag"), Some(Value::Text(tag)) if tag.ends_with("Value")));
    assert_eq!(signal_page["has_more"], Value::Bool(true));
    assert_eq!(integer(&signal_page["next_offset"]), 1);

    let next_signal_page = submit_waveform(
        &mut wellen,
        &mut signal,
        BTreeMap::from([
            ("artifact".to_owned(), first_artifact.clone()),
            (
                "request_fingerprint".to_owned(),
                Value::Text("signal:1".to_owned()),
            ),
            (
                "signal_ids".to_owned(),
                Value::List(vec![signal_id.clone()]),
            ),
            ("start_time".to_owned(), number(0)),
            ("end_time".to_owned(), first_opened["end_time"].clone()),
            ("offset".to_owned(), signal_page["next_offset"].clone()),
            ("max_transitions".to_owned(), number(1)),
        ]),
    );
    let next_signal_page = fields(&next_signal_page, "SignalPage");
    let Value::List(next_signals) = &next_signal_page["signals"] else {
        panic!("second signal page must contain signal rows");
    };
    let Value::Record(next_signal) = &next_signals[0] else {
        panic!("second signal row must be a record");
    };
    let Value::List(next_transitions) = &next_signal["transitions"] else {
        panic!("second signal page must contain transitions");
    };
    let Value::Record(next_transition) = &next_transitions[0] else {
        panic!("second transition must be a record");
    };
    assert!(integer(&next_transition["time"]) > integer(&first_transition_time));

    let mut cursor = program(CURSOR_PROGRAM, "cursor-values");
    let cursor_values = submit_waveform(
        &mut wellen,
        &mut cursor,
        BTreeMap::from([
            ("artifact".to_owned(), first_artifact),
            (
                "request_fingerprint".to_owned(),
                Value::Text("cursor:end".to_owned()),
            ),
            ("cursor_time".to_owned(), first_opened["end_time"].clone()),
            ("signal_ids".to_owned(), Value::List(vec![signal_id])),
        ]),
    );
    let cursor_values = fields(&cursor_values, "CursorValues");
    let Value::List(cursor_rows) = &cursor_values["rows"] else {
        panic!("cursor rows must be a list");
    };
    assert_eq!(cursor_rows.len(), 1);
}

#[tokio::test]
async fn malformed_content_and_oversized_page_requests_fail_as_typed_values() {
    let mut malformed = NamedTempFile::new().unwrap();
    malformed.write_all(b"not a waveform").unwrap();
    malformed.flush().unwrap();
    let mut registry = FileCapabilityRegistry::new(1).unwrap();
    let selected = registry
        .register_file(malformed.path())
        .unwrap()
        .file_selected_value();
    let store = content_store(2);
    let mut stream = FileEffectAdapter::new(registry, store.clone(), 1).unwrap();
    let mut file_program = program(FILE_STREAM_PROGRAM, "stream-malformed");
    let malformed = materialize(&mut stream, &mut file_program, selected).await;
    let mut wellen = WaveformEffectAdapter::new(store.clone(), 1).unwrap();
    let mut open = program(OPEN_PROGRAM, "malformed-open");
    let failed = submit_waveform(
        &mut wellen,
        &mut open,
        BTreeMap::from([("content".to_owned(), malformed.value().unwrap())]),
    );
    assert_eq!(
        fields(&failed, "WaveformFailed")["code"],
        Value::Text("unsupported_format".to_owned())
    );
    assert_eq!(wellen.cached_waveform_count(), 0);

    let path = fixture("simple.vcd");
    let mut registry = FileCapabilityRegistry::new(1).unwrap();
    let selected = registry.register_file(path).unwrap().file_selected_value();
    let mut stream = FileEffectAdapter::new(registry, store.clone(), 1).unwrap();
    let content = materialize(&mut stream, &mut file_program, selected).await;
    let opened = submit_waveform(
        &mut wellen,
        &mut open,
        BTreeMap::from([("content".to_owned(), content.value().unwrap())]),
    );
    let artifact = fields(&opened, "WaveformOpened")["artifact"].clone();
    let mut hierarchy = program(HIERARCHY_PROGRAM, "oversized-hierarchy-page");
    let failed = submit_waveform(
        &mut wellen,
        &mut hierarchy,
        BTreeMap::from([
            ("artifact".to_owned(), artifact),
            (
                "request_fingerprint".to_owned(),
                Value::Text("too-large".to_owned()),
            ),
            ("offset".to_owned(), number(0)),
            ("limit".to_owned(), number(257)),
        ]),
    );
    assert_eq!(
        fields(&failed, "WaveformFailed")["code"],
        Value::Text("invalid_intent".to_owned())
    );
}
