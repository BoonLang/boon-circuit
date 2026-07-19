use boon_persistence::{PersistenceWorkerConfig, RedbDriver};
use boon_plan::ProgramRole;
use boon_runtime::{
    ApplicationIdentity, DistributedProgramBundle, ProgramCapabilityProfile, ProgramCompileRequest,
    RuntimeSourceUnit, SourcePayload, Value, compile_distributed_program_bundle,
};
use boon_server_runtime::{
    InProcessDistributedRuntime, InProcessPoll, InProcessTransientEffectOwner,
    PersistentServerConfig,
};
use std::collections::BTreeMap;
use std::time::Duration;

const CLIENT_SOURCE: &str = r#"
store: [
    advance: SOURCE
    result: Session/store.result
]

scene: Scene/Element/text(
    element: [events: [press: store.advance]]
    style: [width: Fill]
    text: store.result
)
"#;

const SESSION_SOURCE: &str = r#"
store: [
    advance: Client/store.advance
    count:
        0 |> HOLD count {
            advance |> THEN { count + 1 }
        }
    result: Server/store.result
]
"#;

const SERVER_SOURCE: &str = r#"
store: [
    count: Session/store.count
    result: count + 100
]
"#;

const CLIENT_EFFECT_SOURCE: &str = r#"
store: [
    fire: SOURCE
    random:
        NotRequested |> HOLD random {
            fire |> THEN { Random/bytes(byte_count: 1) }
        }
]

document: Document/new(
    root: Element/label(
        element: [events: [press: store.fire]]
        label: TEXT { Distributed effects }
    )
)
"#;

const SESSION_EFFECT_SOURCE: &str = r#"
store: [
    fire: Client/store.fire
    random:
        NotRequested |> HOLD random {
            fire |> THEN { Random/bytes(byte_count: 1) }
        }
]
"#;

const SERVER_EFFECT_SOURCE: &str = r#"
store: [
    fire: Session/store.fire
    random:
        NotRequested |> HOLD random {
            fire |> THEN { Random/bytes(byte_count: 1) }
        }
]
"#;

#[derive(Default)]
struct SettleSummary {
    client_turns: usize,
    session_turns: usize,
    server_turns: usize,
    client_frames_admitted: usize,
    client_frames_acknowledged: usize,
    session_frames_admitted: usize,
    session_frames_acknowledged: usize,
}

impl SettleSummary {
    fn record(&mut self, poll: &InProcessPoll) {
        self.client_turns += poll.client_turns.len();
        self.session_turns += poll.session_turns.len();
        self.server_turns += poll.server_turns.len();
        self.client_frames_admitted += poll.frame_progress.client_to_session.admitted;
        self.client_frames_acknowledged += poll.frame_progress.client_to_session.acknowledged;
        self.session_frames_admitted += poll.frame_progress.session_to_client.admitted;
        self.session_frames_acknowledged += poll.frame_progress.session_to_client.acknowledged;
    }
}

fn bundle() -> DistributedProgramBundle {
    compile_distributed_program_bundle(&[
        request(ProgramRole::Client, CLIENT_SOURCE),
        request(ProgramRole::Session, SESSION_SOURCE),
        request(ProgramRole::Server, SERVER_SOURCE),
    ])
    .expect("in-process distributed fixture should compile")
}

fn effect_bundle() -> DistributedProgramBundle {
    compile_distributed_program_bundle(&[
        request(ProgramRole::Client, CLIENT_EFFECT_SOURCE),
        request(ProgramRole::Session, SESSION_EFFECT_SOURCE),
        request(ProgramRole::Server, SERVER_EFFECT_SOURCE),
    ])
    .expect("in-process distributed effect fixture should compile")
}

fn request(role: ProgramRole, source: &str) -> ProgramCompileRequest {
    ProgramCompileRequest {
        revision: 1,
        role,
        entry_path: "RUN.bn".to_owned(),
        units: vec![RuntimeSourceUnit {
            path: "RUN.bn".to_owned(),
            source: source.to_owned(),
        }],
        application: ApplicationIdentity::new(
            "dev.boon.in-process-runtime-test",
            format!("in-process-{}", role.as_str()),
            "in-process-runtime-test",
        ),
        capability_profile: match role {
            ProgramRole::Client => ProgramCapabilityProfile::PublicClient,
            ProgramRole::Session => ProgramCapabilityProfile::TrustedSession,
            ProgramRole::Server => ProgramCapabilityProfile::TrustedServer,
        },
    }
}

fn settle(runtime: &mut InProcessDistributedRuntime, now: &mut Duration) -> SettleSummary {
    let mut summary = SettleSummary::default();
    for _ in 0..64 {
        let poll = runtime.poll(*now).expect("in-process poll should succeed");
        let has_more_work = poll.has_more_work;
        summary.record(&poll);
        if !has_more_work {
            return summary;
        }
        *now += Duration::from_millis(1);
    }
    panic!("in-process distributed runtime did not settle within its test bound");
}

fn random_ready(byte: u8) -> Value {
    Value::Record(BTreeMap::from([
        (
            "$tag".to_owned(),
            Value::Text("RandomBytesReady".to_owned()),
        ),
        ("bytes".to_owned(), Value::Bytes(vec![byte].into())),
    ]))
}

#[test]
fn client_session_server_session_client_chain_is_canonical_and_bounded() {
    let bundle = bundle();
    let mut runtime = InProcessDistributedRuntime::start_ephemeral(&bundle).unwrap();
    let mut now = Duration::ZERO;
    settle(&mut runtime, &mut now);
    assert_eq!(
        runtime.client_root_value_current("store.result").unwrap(),
        Value::integer(100).unwrap()
    );

    runtime
        .dispatch_client("store.advance", SourcePayload::default())
        .unwrap();
    assert_eq!(runtime.next_deadline(), Some(now));
    let summary = settle(&mut runtime, &mut now);

    assert!(summary.client_turns > 0);
    assert!(summary.session_turns > 0);
    assert!(summary.server_turns > 0);
    assert_eq!(
        runtime.client_root_value_current("store.result").unwrap(),
        Value::integer(101).unwrap()
    );
    assert!(runtime.document_frame().is_some());
    runtime.shutdown().unwrap();
}

#[test]
fn every_admitted_frame_receives_one_writer_ack_and_is_not_replayed() {
    let bundle = bundle();
    let mut runtime = InProcessDistributedRuntime::start_ephemeral(&bundle).unwrap();
    let mut now = Duration::ZERO;
    settle(&mut runtime, &mut now);

    runtime
        .dispatch_client("store.advance", SourcePayload::default())
        .unwrap();
    let summary = settle(&mut runtime, &mut now);
    assert!(summary.client_frames_admitted > 0);
    assert!(summary.session_frames_admitted > 0);
    assert_eq!(
        summary.client_frames_acknowledged,
        summary.client_frames_admitted
    );
    assert_eq!(
        summary.session_frames_acknowledged,
        summary.session_frames_admitted
    );

    let quiet = runtime.poll(now).unwrap();
    assert_eq!(quiet.steps, 0);
    assert_eq!(quiet.frame_progress, Default::default());
    assert_eq!(
        runtime.client_root_value_current("store.result").unwrap(),
        Value::integer(101).unwrap(),
        "an acknowledged operation must not be replayed"
    );
    runtime.shutdown().unwrap();
}

#[test]
fn transient_effect_completions_return_to_the_exact_role_owner() {
    let bundle = effect_bundle();
    let mut runtime = InProcessDistributedRuntime::start_ephemeral(&bundle).unwrap();
    let mut now = Duration::ZERO;
    settle(&mut runtime, &mut now);
    runtime
        .dispatch_client("store.fire", SourcePayload::default())
        .unwrap();

    let mut invocations = Vec::new();
    for _ in 0..64 {
        let poll = runtime.poll(now).unwrap();
        invocations.extend(poll.transient_effects);
        if !poll.has_more_work {
            break;
        }
        now += Duration::from_millis(1);
    }
    assert_eq!(invocations.len(), 3);
    for owner in [
        InProcessTransientEffectOwner::Client,
        InProcessTransientEffectOwner::Session,
        InProcessTransientEffectOwner::Server,
    ] {
        assert_eq!(
            invocations
                .iter()
                .filter(|invocation| invocation.owner == owner)
                .count(),
            1
        );
        assert_eq!(runtime.pending_transient_effect_count(owner), 1);
    }

    let client = invocations
        .iter()
        .find(|invocation| invocation.owner == InProcessTransientEffectOwner::Client)
        .unwrap();
    assert!(
        runtime
            .complete_transient_effect(
                InProcessTransientEffectOwner::Session,
                client.invocation.call_id,
                random_ready(0),
            )
            .is_err(),
        "a completion from the wrong role must fail before mutation"
    );
    for (index, invocation) in invocations.into_iter().enumerate() {
        runtime
            .complete_transient_effect(
                invocation.owner,
                invocation.invocation.call_id,
                random_ready(index as u8),
            )
            .unwrap();
    }
    let summary = settle(&mut runtime, &mut now);
    assert!(summary.client_turns > 0);
    assert!(summary.session_turns > 0);
    assert!(summary.server_turns > 0);
    for owner in [
        InProcessTransientEffectOwner::Client,
        InProcessTransientEffectOwner::Session,
        InProcessTransientEffectOwner::Server,
    ] {
        assert_eq!(runtime.pending_transient_effect_count(owner), 0);
    }
    runtime.shutdown().unwrap();
}

#[test]
fn persistent_authority_and_session_resume_without_exposing_session_ids() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("in-process.redb");
    let bundle = bundle();
    let persistence = || PersistentServerConfig::authoritative(PersistenceWorkerConfig::default());

    let (mut runtime, startup) = InProcessDistributedRuntime::start_persistent(
        &bundle,
        RedbDriver::open(&database).unwrap(),
        persistence(),
    )
    .unwrap();
    let status = runtime
        .persistent_server_status()
        .expect("persistent aggregate should expose its Server lifecycle");
    assert_eq!(
        status.phase,
        boon_server_runtime::ServerLifecyclePhase::Ready
    );
    assert!(status.persistence.worker_alive);
    assert!(status.persistence.accepting_turns);
    assert_eq!(
        startup.disposition,
        boon_runtime::PersistentRuntimeStartupDisposition::Fresh
    );
    let mut now = Duration::ZERO;
    settle(&mut runtime, &mut now);
    runtime
        .dispatch_client("store.advance", SourcePayload::default())
        .unwrap();
    settle(&mut runtime, &mut now);
    assert_eq!(
        runtime.client_root_value_current("store.result").unwrap(),
        Value::integer(101).unwrap()
    );
    let resume = runtime
        .shutdown()
        .unwrap()
        .expect("first shutdown should return opaque resume authority");
    assert_eq!(
        runtime.persistent_server_status().unwrap().phase,
        boon_server_runtime::ServerLifecyclePhase::Stopped
    );

    let (mut recovered, startup) = InProcessDistributedRuntime::resume_persistent(
        &bundle,
        RedbDriver::open(&database).unwrap(),
        persistence(),
        resume,
    )
    .unwrap();
    assert_eq!(
        startup.disposition,
        boon_runtime::PersistentRuntimeStartupDisposition::Restored
    );
    let mut recovered_now = Duration::ZERO;
    settle(&mut recovered, &mut recovered_now);
    assert_eq!(
        recovered.client_root_value_current("store.result").unwrap(),
        Value::integer(101).unwrap()
    );
    recovered.shutdown().unwrap();
    assert_eq!(
        recovered.persistent_server_status().unwrap().phase,
        boon_server_runtime::ServerLifecyclePhase::Stopped
    );
}
