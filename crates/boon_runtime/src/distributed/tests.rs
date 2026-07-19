use super::*;
#[cfg(not(target_arch = "wasm32"))]
use crate::PersistentProgramSession;
use crate::{
    ApplicationIdentity, DistributedProgramBundle, ProgramCapabilityProfile, ProgramCompileRequest,
    ProgramSession, RuntimeSourceUnit, RuntimeTurn, SessionConnectionStatus, SessionPrincipal,
    compile_distributed_program_bundle,
};
use boon_data::Value as DataValue;
use boon_plan::ProgramRole;
use boon_wire::SessionId;

fn test_session_id() -> SessionId {
    SessionId::from_bytes([0x51; 32])
}

fn take_client_session_frames(
    client: &mut DistributedClientRuntime,
    maximum: usize,
) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    for _ in 0..maximum {
        let Some(frame) = client.next_session_frame().unwrap() else {
            break;
        };
        frames.push(frame);
        assert!(client.acknowledge_session_frame());
    }
    frames
}

fn pump_client_session_transport(
    client: &mut DistributedClientRuntime,
    session: &mut DistributedSessionRuntime,
) {
    for _ in 0..128 {
        let mut progressed = false;
        for frame in take_client_session_frames(client, 64) {
            session.admit_client_frame(&frame).unwrap();
            progressed = true;
        }
        while session.poll_client_frame().unwrap().is_some() {
            progressed = true;
        }
        for _ in 0..64 {
            let Some(frame) = session.next_client_frame().unwrap() else {
                break;
            };
            client.accept_session_frame(&frame).unwrap();
            assert!(session.acknowledge_client_frame());
            progressed = true;
        }
        if !progressed {
            return;
        }
    }
    panic!("Client/Session transport did not settle");
}

const CLIENT_EVENT: &str = r#"
store: [
    increment: SOURCE
]

scene: Scene/Element/text(
    element: [events: [press: store.increment]]
    style: [width: Fill]
    text: TEXT { Distributed event }
)
"#;

const SESSION_EVENT: &str = r#"
store: [
    increment: Client/store.increment
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
]
"#;

const CLIENT_CURRENT: &str = r#"
store: [
    increment: SOURCE
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
    session_value: Session/store.session_value
]

scene: Scene/Element/text(
    element: [events: [press: store.increment]]
    style: [width: Fill]
    text: TEXT { Distributed current }
)
"#;

const SESSION_CURRENT: &str = r#"
store: [
    client_count: Client/store.count
    session_value: 42
]
"#;

const CLIENT_CALL: &str = r#"
store: [
    doubled: Session/double(value: 21)
]

scene: Scene/Element/text(
    element: []
    style: [width: Fill]
    text: TEXT { Distributed call }
)
"#;

const SESSION_CALL: &str = r#"
store: [
    ready: True
]

FUNCTION double(value) {
    value * 2
}
"#;

const CLIENT_ORIGIN: &str = r#"
store: [
    increment: SOURCE
]

scene: Scene/Element/text(
    element: [events: [press: store.increment]]
    style: [width: Fill]
    text: TEXT { Distributed origin }
)
"#;

const SESSION_ORIGIN: &str = r#"
store: [
    increment: Client/store.increment
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
    server_mirrored: Server/store.mirrored_count
    server_shared: Server/store.shared_count
]
"#;

const SERVER_SIMPLE: &str = r#"
store: [
    ready: True
]
"#;

const SERVER_ORIGIN: &str = r#"
store: [
    mirrored_count: Session/store.count + 10
    shared_count: 42
]
"#;

const CLIENT_SESSION_STATUS: &str = r#"
store: [
    session_status: Session/store.status
]

scene: Scene/Element/text(
    element: []
    style: [width: Fill]
    text: store.session_status
)
"#;

const SESSION_STATUS: &str = r#"
store: [
    status: SessionInfo/status()
]
"#;

const SERVER_SESSION_STATUS: &str = r#"
store: [
    session_status: Session/store.status
]
"#;

const CLIENT_SHARED: &str = r#"
store: [
    ready: True
]

scene: Scene/Element/text(
    element: []
    style: [width: Fill]
    text: TEXT { Shared subscription }
)
"#;

const SESSION_SHARED: &str = r#"
store: [
    shared: Server/store.shared
]
"#;

const SERVER_SHARED: &str = r#"
store: [
    shared: 42
]
"#;

const CLIENT_SESSION_EFFECT: &str = r#"
store: [
    read_clock: SOURCE
    result: Session/store.clock_result
]

scene: Scene/Element/text(
    element: [events: [press: store.read_clock]]
    style: [width: Fill]
    text: TEXT { Session effect }
)
"#;

const CLIENT_OWN_EFFECT: &str = r#"
store: [
    randomize: SOURCE
    random:
        RandomNotRead |> HOLD random {
            randomize |> THEN { Random/bytes(byte_count: 1) }
        }
]

scene: Scene/Element/text(
    element: [events: [press: store.randomize]]
    style: [width: Fill]
    text: TEXT { Client effect }
)
"#;

const SESSION_EFFECT: &str = r#"
store: [
    read_clock: Client/store.read_clock
    clock_result:
        ClockNotRead |> HOLD clock_result {
            read_clock |> THEN { Clock/wall() }
        }
]
"#;

const CLIENT_SERVER_EFFECT: &str = r#"
store: [
    read_clock: SOURCE
]

scene: Scene/Element/text(
    element: [events: [press: store.read_clock]]
    style: [width: Fill]
    text: Session/store.server_seconds
)
"#;

const SESSION_SERVER_EFFECT: &str = r#"
store: [
    read_clock: Client/store.read_clock
    server_seconds: Server/store.clock_seconds
]
"#;

const SERVER_EFFECT: &str = r#"
store: [
    read_clock: Session/store.read_clock
    clock_result:
        ClockNotRead |> HOLD clock_result {
            read_clock |> THEN { Clock/wall() }
        }
    clock_seconds:
        clock_result |> WHEN {
            WallClockRead => clock_result.unix_seconds
            __ => 0
        }
]
"#;

const CLIENT_ATOMIC: &str = r#"
store: [
    increment: SOURCE
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
]

scene: Scene/Element/text(
    element: [events: [press: store.increment]]
    style: [width: Fill]
    text: TEXT { Atomic publication }
)
"#;

const SESSION_ATOMIC: &str = r#"
store: [
    increment: Client/store.increment
    client_count: Client/store.count
    session_count:
        0 |> HOLD session_count {
            increment |> THEN { session_count + 1 }
        }
]
"#;

const SERVER_ATOMIC: &str = r#"
store: [
    increment: Session/store.increment
    session_count: Session/store.session_count
]
"#;

struct PhysicalLoopback {
    client: DistributedClientRuntime,
    session: DistributedSessionRuntime,
    server: DistributedServerRuntime,
    server_machine: ProgramSession,
    origin: SessionOrigin,
}

impl PhysicalLoopback {
    fn start(bundle: &DistributedProgramBundle) -> Self {
        let limits = DistributedQueueLimits::default();
        let mut client =
            DistributedClientRuntime::start(bundle.artifact(ProgramRole::Client).unwrap(), limits)
                .unwrap();
        let session_template = DistributedSessionTemplate::from_artifact(
            bundle.artifact(ProgramRole::Session).unwrap(),
        )
        .unwrap();
        let mut session = session_template
            .instantiate(test_session_id(), 1, SessionPrincipal::Anonymous, limits)
            .unwrap();
        let server_artifact = bundle.artifact(ProgramRole::Server).unwrap();
        let mut server = DistributedServerRuntime::start(server_artifact).unwrap();
        let mut server_machine = ProgramSession::start(server_artifact.clone()).unwrap();
        let origin = SessionOrigin::new(0, 1).unwrap();
        server
            .bind(&mut server_machine)
            .attach_origin(origin, SessionPrincipal::Anonymous, 1)
            .unwrap();
        client.bind(test_session_id(), 1, 0).unwrap();
        client.mark_current().unwrap();
        session.mark_current().unwrap();
        let mut runtime = Self {
            client,
            session,
            server,
            server_machine,
            origin,
        };
        let update = runtime
            .server
            .bind(&mut runtime.server_machine)
            .set_origin_status(origin, SessionConnectionStatus::Current)
            .unwrap();
        runtime.deliver_server(update);
        runtime.pump();
        runtime
    }

    fn pump(&mut self) {
        for _ in 0..128 {
            let mut progressed = false;
            let client_frames = take_client_session_frames(&mut self.client, 64);
            progressed |= !client_frames.is_empty();
            for frame in client_frames {
                self.session.admit_client_frame(&frame).unwrap();
            }
            while self.session.poll_client_frame().unwrap().is_some() {
                progressed = true;
            }
            let server_messages = self.session.drain_server_messages(64);
            progressed |= !server_messages.is_empty();
            for message in server_messages {
                let update = self
                    .server
                    .bind(&mut self.server_machine)
                    .accept_session_message(self.origin, message)
                    .unwrap();
                progressed |= !update.deliveries.is_empty();
                self.deliver_server(update);
            }
            for _ in 0..64 {
                let Some(frame) = self.session.next_client_frame().unwrap() else {
                    break;
                };
                progressed = true;
                self.client.accept_session_frame(&frame).unwrap();
                assert!(self.session.acknowledge_client_frame());
            }
            if !progressed {
                return;
            }
        }
        panic!("physical distributed loopback did not settle");
    }

    fn deliver_server(&mut self, update: DistributedServerUpdate) {
        for delivery in update.deliveries {
            assert!(
                matches!(
                    delivery.target,
                    ServerDeliveryTarget::Origin(origin) if origin == self.origin
                ) || delivery.target == ServerDeliveryTarget::AllSessions
            );
            self.session
                .accept_server_message(delivery.message)
                .unwrap();
        }
    }

    fn dispatch_client_event_to_server(&mut self, path: &str) -> Vec<RuntimeTurn> {
        self.client
            .dispatch(path, SourcePayload::default())
            .unwrap();
        for frame in take_client_session_frames(&mut self.client, 64) {
            self.session.admit_client_frame(&frame).unwrap();
        }
        while self.session.poll_client_frame().unwrap().is_some() {}
        let mut turns = Vec::new();
        for message in self.session.drain_server_messages(64) {
            let update = self
                .server
                .bind(&mut self.server_machine)
                .accept_session_message(self.origin, message)
                .unwrap();
            turns.extend(update.turns.clone());
            self.deliver_server(update);
        }
        turns
    }
}

#[test]
fn browser_boundary_moves_events_only_as_canonical_cbor() {
    let mut runtime = PhysicalLoopback::start(&bundle(CLIENT_EVENT, SESSION_EVENT, SERVER_SIMPLE));
    runtime
        .client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    runtime.pump();
    assert_eq!(
        runtime.session.root_value_current("store.count").unwrap(),
        Value::integer(1).unwrap()
    );
}

#[test]
fn client_frame_lease_is_idempotent_until_writer_acknowledgement() {
    let bundle = bundle(CLIENT_EVENT, SESSION_EVENT, SERVER_SIMPLE);
    let mut client = DistributedClientRuntime::start(
        bundle.artifact(ProgramRole::Client).unwrap(),
        DistributedQueueLimits::default(),
    )
    .unwrap();
    let template =
        DistributedSessionTemplate::from_artifact(bundle.artifact(ProgramRole::Session).unwrap())
            .unwrap();
    let mut session = template
        .instantiate(
            test_session_id(),
            1,
            SessionPrincipal::Anonymous,
            DistributedQueueLimits::default(),
        )
        .unwrap();
    client.bind(test_session_id(), 1, 0).unwrap();
    client.mark_current().unwrap();
    session.mark_current().unwrap();
    client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();

    let first = client.next_session_frame().unwrap().unwrap();
    let retry = client.next_session_frame().unwrap().unwrap();
    assert_eq!(retry, first);
    assert_eq!(client.pending_session_frames(), 1);
    assert!(client.acknowledge_session_frame());
    assert_eq!(client.pending_session_frames(), 1);
    assert!(client.next_session_frame().unwrap().is_none());
    session.admit_client_frame(&first).unwrap();
    assert!(session.poll_client_frame().unwrap().is_some());
    let acknowledgement = session.next_client_frame().unwrap().unwrap();
    client.accept_session_frame(&acknowledgement).unwrap();
    assert!(session.acknowledge_client_frame());
    assert_eq!(client.pending_session_frames(), 0);
}

#[test]
fn session_frame_lease_is_idempotent_until_writer_acknowledgement() {
    let bundle = bundle(CLIENT_CURRENT, SESSION_CURRENT, SERVER_SIMPLE);
    let template =
        DistributedSessionTemplate::from_artifact(bundle.artifact(ProgramRole::Session).unwrap())
            .unwrap();
    let mut session = template
        .instantiate(
            test_session_id(),
            1,
            SessionPrincipal::Anonymous,
            DistributedQueueLimits::default(),
        )
        .unwrap();
    session.mark_current().unwrap();

    let first = session.next_client_frame().unwrap().unwrap();
    let retry = session.next_client_frame().unwrap().unwrap();
    assert_eq!(retry, first);
    assert_eq!(session.pending_client_frames(), 1);
    assert!(session.acknowledge_client_frame());
    assert!(session.next_client_frame().unwrap().is_none());
    assert_eq!(session.pending_client_frames(), 1);
}

#[test]
fn session_recovery_replays_writer_admitted_but_unacknowledged_operation() {
    let bundle = bundle(CLIENT_CURRENT, SESSION_CURRENT, SERVER_SIMPLE);
    let template =
        DistributedSessionTemplate::from_artifact(bundle.artifact(ProgramRole::Session).unwrap())
            .unwrap();
    let limits = DistributedQueueLimits::default();
    let mut session = template
        .instantiate(test_session_id(), 1, SessionPrincipal::Anonymous, limits)
        .unwrap();
    session.mark_current().unwrap();

    let admitted = session.next_client_frame().unwrap().unwrap();
    assert!(session.acknowledge_client_frame());
    assert!(session.next_client_frame().unwrap().is_none());
    let payload = session.recovery_payload().unwrap();

    let mut recovered = template.restore(&payload, limits).unwrap();
    assert!(recovered.session_id() == test_session_id());
    assert_eq!(recovered.pending_client_frames(), 1);
    assert_eq!(recovered.next_client_frame().unwrap().unwrap(), admitted);
}

#[test]
fn session_recovery_rejects_noncanonical_or_wrong_graph_payload() {
    let current_bundle = bundle(CLIENT_CURRENT, SESSION_CURRENT, SERVER_SIMPLE);
    let template = DistributedSessionTemplate::from_artifact(
        current_bundle.artifact(ProgramRole::Session).unwrap(),
    )
    .unwrap();
    let limits = DistributedQueueLimits::default();
    let mut session = template
        .instantiate(test_session_id(), 1, SessionPrincipal::Anonymous, limits)
        .unwrap();
    session.mark_current().unwrap();
    let mut payload = session.recovery_payload().unwrap();
    payload.push(0);
    assert!(matches!(
        template.restore(&payload, limits),
        Err(DistributedRuntimeError::InvalidTransportFrame)
    ));

    let other = bundle(CLIENT_EVENT, SESSION_EVENT, SERVER_SIMPLE);
    let other_template =
        DistributedSessionTemplate::from_artifact(other.artifact(ProgramRole::Session).unwrap())
            .unwrap();
    payload.pop();
    assert!(matches!(
        other_template.restore(&payload, limits),
        Err(DistributedRuntimeError::ProtocolMismatch)
            | Err(DistributedRuntimeError::InvalidTransportFrame)
    ));
}

#[test]
fn browser_boundary_moves_current_values_in_both_directions() {
    let mut runtime =
        PhysicalLoopback::start(&bundle(CLIENT_CURRENT, SESSION_CURRENT, SERVER_SIMPLE));
    assert_eq!(
        runtime
            .client
            .root_value_current("store.session_value")
            .unwrap(),
        Value::integer(42).unwrap()
    );
    runtime
        .client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    runtime.pump();
    assert_eq!(
        runtime
            .session
            .root_value_current("store.client_count")
            .unwrap(),
        Value::integer(1).unwrap()
    );
}

#[test]
fn browser_boundary_moves_pure_calls_and_results_as_cbor() {
    let mut runtime = PhysicalLoopback::start(&bundle(CLIENT_CALL, SESSION_CALL, SERVER_SIMPLE));
    assert_eq!(
        runtime.client.root_value_current("store.doubled").unwrap(),
        Value::integer(42).unwrap()
    );
}

#[test]
fn client_publication_backpressure_rolls_back_machine_and_protocol_state() {
    let bundle = bundle(CLIENT_ATOMIC, SESSION_ATOMIC, SERVER_ATOMIC);
    let client_limits = DistributedQueueLimits {
        max_messages: 2,
        max_bytes: 1024 * 1024,
    };
    let mut client = DistributedClientRuntime::start(
        bundle.artifact(ProgramRole::Client).unwrap(),
        client_limits,
    )
    .unwrap();
    let session_template =
        DistributedSessionTemplate::from_artifact(bundle.artifact(ProgramRole::Session).unwrap())
            .unwrap();
    let mut session = session_template
        .instantiate(
            test_session_id(),
            1,
            SessionPrincipal::Anonymous,
            DistributedQueueLimits::default(),
        )
        .unwrap();
    client.bind(test_session_id(), 1, 0).unwrap();
    client.mark_current().unwrap();
    session.mark_current().unwrap();
    pump_client_session_transport(&mut client, &mut session);
    session.drain_server_messages(64);

    client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    assert!(matches!(
        client.dispatch("store.increment", SourcePayload::default()),
        Err(DistributedRuntimeError::QueueFull { limit: 2 })
    ));
    assert_eq!(
        client.root_value_current("store.count").unwrap(),
        Value::integer(1).unwrap()
    );
    assert_eq!(client.pending_session_frames(), 2);

    pump_client_session_transport(&mut client, &mut session);
    session.drain_server_messages(64);
    client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    pump_client_session_transport(&mut client, &mut session);

    assert_eq!(
        client.root_value_current("store.count").unwrap(),
        Value::integer(2).unwrap()
    );
    assert_eq!(
        session.root_value_current("store.client_count").unwrap(),
        Value::integer(2).unwrap()
    );
    assert_eq!(
        session.root_value_current("store.session_count").unwrap(),
        Value::integer(2).unwrap()
    );
}

#[test]
fn session_publication_backpressure_preserves_inbound_frame_for_exact_retry() {
    let bundle = bundle(CLIENT_ATOMIC, SESSION_ATOMIC, SERVER_ATOMIC);
    let mut client = DistributedClientRuntime::start(
        bundle.artifact(ProgramRole::Client).unwrap(),
        DistributedQueueLimits::default(),
    )
    .unwrap();
    let session_template =
        DistributedSessionTemplate::from_artifact(bundle.artifact(ProgramRole::Session).unwrap())
            .unwrap();
    let mut session = session_template
        .instantiate(
            test_session_id(),
            1,
            SessionPrincipal::Anonymous,
            DistributedQueueLimits {
                max_messages: 2,
                max_bytes: 1024 * 1024,
            },
        )
        .unwrap();
    client.bind(test_session_id(), 1, 0).unwrap();
    client.mark_current().unwrap();
    session.mark_current().unwrap();
    for frame in take_client_session_frames(&mut client, 64) {
        session.admit_client_frame(&frame).unwrap();
    }
    while session.poll_client_frame().unwrap().is_some() {}
    session.drain_server_messages(64);

    client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    for frame in take_client_session_frames(&mut client, 64) {
        session.admit_client_frame(&frame).unwrap();
    }
    while session.poll_client_frame().unwrap().is_some() {}
    assert_eq!(session.pending_server_messages(), 2);

    client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    for frame in take_client_session_frames(&mut client, 64) {
        session.admit_client_frame(&frame).unwrap();
    }
    assert!(matches!(
        session.poll_client_frame(),
        Err(DistributedRuntimeError::QueueFull { limit: 2 })
    ));
    assert_eq!(
        session.root_value_current("store.session_count").unwrap(),
        Value::integer(1).unwrap()
    );
    assert_eq!(session.pending_server_messages(), 2);

    session.drain_server_messages(64);
    while session.poll_client_frame().unwrap().is_some() {}
    assert_eq!(
        session.root_value_current("store.client_count").unwrap(),
        Value::integer(2).unwrap()
    );
    assert_eq!(
        session.root_value_current("store.session_count").unwrap(),
        Value::integer(2).unwrap()
    );
    assert_eq!(session.pending_server_messages(), 2);
}

#[test]
fn session_server_origin_inputs_install_atomically_in_process() {
    let mut runtime =
        PhysicalLoopback::start(&bundle(CLIENT_ORIGIN, SESSION_ORIGIN, SERVER_ORIGIN));
    assert_eq!(
        runtime
            .session
            .root_value_current("store.server_mirrored")
            .unwrap(),
        Value::integer(10).unwrap()
    );
    runtime
        .client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    runtime.pump();
    assert_eq!(
        runtime
            .session
            .root_value_current("store.server_mirrored")
            .unwrap(),
        Value::integer(11).unwrap()
    );
}

#[test]
fn server_context_switch_clears_absent_origin_imports_without_leakage() {
    let bundle = bundle(CLIENT_ORIGIN, SESSION_ORIGIN, SERVER_ORIGIN);
    let artifact = bundle.artifact(ProgramRole::Server).unwrap();
    let export_id = artifact
        .plan()
        .distributed_endpoint
        .as_ref()
        .unwrap()
        .wire_schema
        .value_edges
        .iter()
        .find(|edge| {
            edge.producer_role == ProgramRole::Session && edge.consumer_role == ProgramRole::Server
        })
        .unwrap()
        .export_id;
    let mut server = DistributedServerRuntime::start(artifact).unwrap();
    let mut server_machine = ProgramSession::start(artifact.clone()).unwrap();
    let first = SessionOrigin::new(0, 1).unwrap();
    let second = SessionOrigin::new(1, 1).unwrap();
    {
        let mut authority = server.bind(&mut server_machine);
        authority
            .attach_origin(first, SessionPrincipal::Anonymous, 1)
            .unwrap();
        authority
            .attach_origin(second, SessionPrincipal::Anonymous, 2)
            .unwrap();
    }

    server
        .bind(&mut server_machine)
        .accept_session_message(
            first,
            DistributedMessage {
                producer: ProgramRole::Session,
                consumer: ProgramRole::Server,
                payload: DistributedMessagePayload::Current {
                    export_id,
                    revision: 5,
                    value: DataValue::integer(7).unwrap(),
                },
            },
        )
        .unwrap();
    assert_eq!(
        server
            .bind(&mut server_machine)
            .root_value_current(first, "store.mirrored_count")
            .unwrap(),
        Value::integer(17).unwrap()
    );
    assert!(matches!(
        server
            .bind(&mut server_machine)
            .root_value_current(second, "store.mirrored_count")
            .unwrap(),
        Value::Error { code } if code == "remote_not_current"
    ));
    assert_eq!(
        server
            .bind(&mut server_machine)
            .root_value_current(first, "store.mirrored_count")
            .unwrap(),
        Value::integer(17).unwrap()
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn persistent_server_context_switches_preserve_contiguous_authority_turns() {
    let bundle = bundle(CLIENT_SERVER_EFFECT, SESSION_SERVER_EFFECT, SERVER_EFFECT);
    let artifact = bundle.artifact(ProgramRole::Server).unwrap();
    let event_export = artifact
        .plan()
        .distributed_endpoint
        .as_ref()
        .unwrap()
        .wire_schema
        .event_edges
        .iter()
        .find(|edge| {
            edge.producer_role == ProgramRole::Session && edge.consumer_role == ProgramRole::Server
        })
        .unwrap()
        .export_id;
    let (mut machine, _) = PersistentProgramSession::start(
        artifact.clone(),
        boon_persistence::InMemoryDriver::default(),
        boon_persistence::PersistenceWorkerConfig::default(),
    )
    .unwrap();
    let mut server = DistributedServerRuntime::start(artifact).unwrap();
    let origin = SessionOrigin::new(0, 1).unwrap();
    {
        let mut authority = server.bind(&mut machine);
        authority
            .attach_origin(origin, SessionPrincipal::Anonymous, 1)
            .unwrap();
        authority
            .set_origin_status(origin, SessionConnectionStatus::Current)
            .unwrap();
    }

    let update = server
        .bind(&mut machine)
        .accept_session_message(
            origin,
            DistributedMessage {
                producer: ProgramRole::Session,
                consumer: ProgramRole::Server,
                payload: DistributedMessagePayload::Event {
                    export_id: event_export,
                    sequence: 1,
                    value: DataValue::Null,
                },
            },
        )
        .unwrap();
    assert!(
        update
            .turns
            .iter()
            .any(|turn| { turn.sequence == 1 && turn.source_sequence.is_some() })
    );
    machine.barrier().unwrap();
    let status = machine.persistence_status();
    assert_eq!(status.durable_through_turn_sequence, 1);
    assert!(status.last_error.is_none());
    machine.shutdown().unwrap();
}

#[test]
fn stale_session_publishes_server_status_without_buffering_client_output() {
    let bundle = bundle(CLIENT_SESSION_STATUS, SESSION_STATUS, SERVER_SESSION_STATUS);
    let session_template =
        DistributedSessionTemplate::from_artifact(bundle.artifact(ProgramRole::Session).unwrap())
            .unwrap();
    let mut session = session_template
        .instantiate(
            test_session_id(),
            1,
            SessionPrincipal::Anonymous,
            DistributedQueueLimits::default(),
        )
        .unwrap();
    session.settle().unwrap();
    while session.next_client_frame().unwrap().is_some() {
        assert!(session.acknowledge_client_frame());
    }
    session.drain_server_messages(64);
    session.mark_current().unwrap();
    while session.next_client_frame().unwrap().is_some() {
        assert!(session.acknowledge_client_frame());
    }
    session.drain_server_messages(64);

    let pending_before_stale = session.pending_client_frames();
    session.mark_stale().unwrap();

    assert_eq!(session.pending_client_frames(), pending_before_stale);
    assert_eq!(session.pending_server_messages(), 1);
}

#[test]
fn each_new_origin_receives_the_current_shared_server_snapshot() {
    let bundle = bundle(CLIENT_SHARED, SESSION_SHARED, SERVER_SHARED);
    let mut server =
        DistributedServerRuntime::start(bundle.artifact(ProgramRole::Server).unwrap()).unwrap();
    let mut server_machine =
        ProgramSession::start(bundle.artifact(ProgramRole::Server).unwrap().clone()).unwrap();
    let first = SessionOrigin::new(0, 1).unwrap();
    let second = SessionOrigin::new(1, 1).unwrap();
    server
        .bind(&mut server_machine)
        .attach_origin(first, SessionPrincipal::Anonymous, 1)
        .unwrap();
    let first_update = server
        .bind(&mut server_machine)
        .settle_origin(first)
        .unwrap();
    assert_eq!(first_update.deliveries.len(), 1);
    assert!(matches!(
        first_update.deliveries[0].target,
        ServerDeliveryTarget::AllSessions
    ));

    server
        .bind(&mut server_machine)
        .attach_origin(second, SessionPrincipal::Anonymous, 2)
        .unwrap();
    let second_update = server
        .bind(&mut server_machine)
        .settle_origin(second)
        .unwrap();
    assert_eq!(second_update.deliveries.len(), 1);
    assert!(matches!(
        second_update.deliveries[0].target,
        ServerDeliveryTarget::Origin(origin) if origin == second
    ));
}

#[test]
fn session_owns_and_completes_its_transient_effects() {
    let bundle = bundle(CLIENT_SESSION_EFFECT, SESSION_EFFECT, SERVER_SIMPLE);
    let limits = DistributedQueueLimits::default();
    let mut client =
        DistributedClientRuntime::start(bundle.artifact(ProgramRole::Client).unwrap(), limits)
            .unwrap();
    let session_template =
        DistributedSessionTemplate::from_artifact(bundle.artifact(ProgramRole::Session).unwrap())
            .unwrap();
    let mut session = session_template
        .instantiate(test_session_id(), 1, SessionPrincipal::Anonymous, limits)
        .unwrap();
    client.bind(test_session_id(), 1, 0).unwrap();
    session.settle().unwrap();
    client.mark_current().unwrap();
    session.mark_current().unwrap();

    client
        .dispatch("store.read_clock", SourcePayload::default())
        .unwrap();
    for frame in take_client_session_frames(&mut client, 64) {
        session.admit_client_frame(&frame).unwrap();
    }
    let update = session.poll_client_frame().unwrap().unwrap();
    let invocation = update
        .turns
        .iter()
        .flat_map(|turn| &turn.transient_effects)
        .next()
        .cloned()
        .expect("Session effect invocation");
    assert_eq!(session.pending_transient_effect_count(), 1);

    session
        .complete_transient_effect(
            invocation.call_id,
            Value::Record(std::collections::BTreeMap::from([
                ("$tag".to_owned(), Value::Text("WallClockRead".to_owned())),
                ("unix_seconds".to_owned(), Value::integer(123).unwrap()),
                ("nanoseconds".to_owned(), Value::integer(0).unwrap()),
            ])),
        )
        .unwrap();
    assert_eq!(session.pending_transient_effect_count(), 0);
    assert!(matches!(
        session.root_value_current("store.clock_result").unwrap(),
        Value::Record(fields)
            if fields.get("$tag") == Some(&Value::Text("WallClockRead".to_owned()))
                && fields.get("unix_seconds") == Some(&Value::integer(123).unwrap())
    ));
}

#[test]
fn client_disconnect_cancels_exact_owned_effects_before_marking_stale() {
    let bundle = bundle(CLIENT_OWN_EFFECT, SERVER_SIMPLE, SERVER_SIMPLE);
    let limits = DistributedQueueLimits::default();
    let mut client =
        DistributedClientRuntime::start(bundle.artifact(ProgramRole::Client).unwrap(), limits)
            .unwrap();
    client.bind(test_session_id(), 1, 0).unwrap();
    client.mark_current().unwrap();

    let dispatched = client
        .dispatch("store.randomize", SourcePayload::default())
        .unwrap();
    let invocation = dispatched
        .turns
        .iter()
        .flat_map(|turn| &turn.transient_effects)
        .next()
        .cloned()
        .expect("Client-owned random effect");
    assert_eq!(client.pending_transient_effect_count(), 1);

    let stale = client.mark_stale().unwrap();
    assert!(stale.turns.iter().any(|turn| {
        turn.cancelled_transient_effects
            .contains(&invocation.call_id)
    }));
    assert_eq!(client.pending_transient_effect_count(), 0);
    assert!(
        client
            .complete_transient_effect(invocation.call_id, Value::Null)
            .is_err(),
        "a completion from the disconnected generation must be rejected"
    );
}

#[test]
fn server_effect_completion_returns_to_its_origin_session() {
    let bundle = bundle(CLIENT_SERVER_EFFECT, SESSION_SERVER_EFFECT, SERVER_EFFECT);
    let mut runtime = PhysicalLoopback::start(&bundle);
    let invocation = runtime
        .dispatch_client_event_to_server("store.read_clock")
        .iter()
        .flat_map(|turn| &turn.transient_effects)
        .next()
        .cloned()
        .expect("Server effect invocation");
    assert_eq!(
        runtime
            .server
            .bind(&mut runtime.server_machine)
            .pending_transient_effect_count(runtime.origin),
        1
    );

    let update = runtime
        .server
        .bind(&mut runtime.server_machine)
        .complete_transient_effect(
            invocation.call_id,
            Value::Record(std::collections::BTreeMap::from([
                ("$tag".to_owned(), Value::Text("WallClockRead".to_owned())),
                ("unix_seconds".to_owned(), Value::integer(456).unwrap()),
                ("nanoseconds".to_owned(), Value::integer(0).unwrap()),
            ])),
        )
        .unwrap();
    runtime.deliver_server(update);
    runtime.pump();

    assert_eq!(
        runtime
            .server
            .bind(&mut runtime.server_machine)
            .pending_transient_effect_count(runtime.origin),
        0
    );
    assert!(
        runtime
            .client
            .document_frame()
            .unwrap()
            .nodes
            .values()
            .any(|node| node.text.as_ref().is_some_and(|text| text.text == "456")),
        "Server effect result did not reach the visible Client document"
    );

    let second = runtime
        .dispatch_client_event_to_server("store.read_clock")
        .iter()
        .flat_map(|turn| &turn.transient_effects)
        .next()
        .cloned()
        .expect("second Server effect invocation");
    assert!(second.call_id != invocation.call_id);
    assert_eq!(
        runtime
            .server
            .bind(&mut runtime.server_machine)
            .pending_transient_effect_count(runtime.origin),
        1
    );
    let artifact = bundle.artifact(ProgramRole::Server).unwrap();
    let recovery = runtime.server.recovery_payload().unwrap();
    let mut recovered_server = DistributedServerRuntime::start_with_recovery(artifact, &recovery)
        .expect("Server router recovery excludes process-owned effect calls");
    let mut recovered_machine = ProgramSession::start(artifact.clone()).unwrap();
    assert_eq!(
        recovered_server
            .bind(&mut recovered_machine)
            .pending_transient_effect_count(runtime.origin),
        0
    );
    let expiry = runtime
        .server
        .bind(&mut runtime.server_machine)
        .expire_origin(runtime.origin)
        .unwrap();
    assert!(
        expiry
            .turns
            .iter()
            .any(|turn| turn.cancelled_transient_effects.contains(&second.call_id))
    );
    assert_eq!(
        runtime
            .server
            .bind(&mut runtime.server_machine)
            .pending_transient_effect_count(runtime.origin),
        0
    );
}

fn bundle(client: &str, session: &str, server: &str) -> DistributedProgramBundle {
    compile_distributed_program_bundle(&[
        request(ProgramRole::Client, client),
        request(ProgramRole::Session, session),
        request(ProgramRole::Server, server),
    ])
    .expect("compile physical distributed fixture")
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
            "dev.boon.distributed-physical-test",
            format!("test-{}", role.as_str()),
            "runtime-test",
        ),
        capability_profile: match role {
            ProgramRole::Client => ProgramCapabilityProfile::PublicClient,
            ProgramRole::Session => ProgramCapabilityProfile::TrustedSession,
            ProgramRole::Server => ProgramCapabilityProfile::TrustedServer,
        },
    }
}
