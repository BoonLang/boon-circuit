use super::*;
use crate::{
    ApplicationIdentity, DistributedProgramBundle, ProgramCapabilityProfile, ProgramCompileRequest,
    ProgramSession, RuntimeSourceUnit, RuntimeTurn, SessionConnectionStatus, SessionPrincipal,
    compile_distributed_program_bundle,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::{PersistentDistributedCommitOutcome, PersistentProgramSession};
use boon_data::Value as DataValue;
use boon_plan::{DistributedCallInstanceId, ProgramRole};
use boon_wire::SessionId;

fn test_session_id() -> SessionId {
    SessionId::from_bytes([0x51; 32])
}

#[test]
fn session_origin_debug_redacts_hidden_slot_and_generation() {
    let origin = SessionOrigin::new(4_294_967_291, 18_446_744_073_709_551_611).unwrap();

    assert_eq!(format!("{origin:?}"), "SessionOrigin(..)");
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

fn take_session_server_messages(
    session: &mut DistributedSessionRuntime,
    maximum: usize,
) -> Vec<DistributedMessage> {
    let mut messages = Vec::new();
    for _ in 0..maximum {
        let Some(message) = session.next_server_message() else {
            break;
        };
        messages.push(message);
        assert!(session.acknowledge_server_message());
    }
    messages
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
    text: store.doubled |> Number/to_text(radix: 10)
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

const CLIENT_WRAPPED_CALLS: &str = r#"
store: [
    first: remote_add(value: 5)
    second: outer(value: 8)
]

FUNCTION remote_add(value) {
    Session/add(value: value)
}

FUNCTION outer(value) {
    remote_add(value: value)
}

scene: Scene/Element/text(
    element: []
    style: [width: Fill]
    text:
        store.first + store.second
        |> Number/to_text(radix: 10)
)
"#;

const SESSION_ADD: &str = r#"
store: [
    ready: True
]

FUNCTION add(value) {
    value + 1
}
"#;

const CLIENT_NESTED_CURRENT: &str = r#"
store: [
    result: Session/double_after_server(value: 20)
]

scene: Scene/Element/text(
    element: []
    style: [width: Fill]
    text: store.result |> Number/to_text(radix: 10)
)
"#;

const SESSION_NESTED_CURRENT: &str = r#"
store: [ready: True]

FUNCTION double_after_server(value) {
    Server/add_one(value: value) * 2
}
"#;

const SERVER_ADD_ONE: &str = r#"
store: [ready: True]

FUNCTION add_one(value) {
    value + 1
}
"#;

const SESSION_ZERO_ARGUMENT_CURRENT: &str = r#"
store: [
    answer: Server/answer()
]
"#;

const SERVER_ZERO_ARGUMENT_CURRENT: &str = r#"
store: [ready: True]

FUNCTION answer() {
    42
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

const SESSION_SERVER_REMEMBER: &str = r#"
store: [
    invoke: SOURCE
    remembered:
        invoke |> THEN { Server/remember(value: 1) }
]
"#;

const SERVER_REMEMBER: &str = r#"
store: [ready: True]

FUNCTION remember(value) {
    value |> HOLD current { LATEST {} }
}
"#;

#[cfg(not(target_arch = "wasm32"))]
const SESSION_SERVER_CURRENT: &str = r#"
store: [
    client_count: Client/store.count
    session_value: Server/double(value: 21)
]
"#;

#[cfg(not(target_arch = "wasm32"))]
const SERVER_DOUBLE: &str = r#"
store: [ready: True]

FUNCTION double(value) {
    value * 2
}
"#;

const CLIENT_INVOCATION: &str = r#"
store: [
    invoke: SOURCE
]

scene: Scene/Element/text(
    element: [events: [press: store.invoke]]
    style: [width: Fill]
    text: TEXT { Distributed invocation }
)
"#;

const CLIENT_SESSION_INVOCATION: &str = r#"
store: [
    invoke: SOURCE
    result:
        0 |> HOLD result {
            invoke |> THEN { Session/double(value: 21) }
        }
]

scene: Scene/Element/text(
    element: [events: [press: store.invoke]]
    style: [width: Fill]
    text: TEXT { Session invocation }
)
"#;

const SESSION_INVOCATION: &str = r#"
store: [
    invoke: Client/store.invoke
    result:
        invoke |> THEN { Server/identity(value: 7) }
]
"#;

const SESSION_FALSE_INVOCATION: &str = r#"
store: [
    invoke: Client/store.invoke
    result:
        invoke |> THEN {
            False |> WHILE {
                True => Server/identity(value: 7)
                False => 0
            }
        }
]
"#;

const SERVER_IDENTITY: &str = r#"
store: [ready: True]

FUNCTION identity(value) {
    value
}
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

const SESSION_NESTED_SERVER_EFFECT: &str = r#"
store: [
    invoke: Client/store.invoke
    forwarded: Server/store.forwarded
]

FUNCTION echo(value) {
    value
}
"#;

const SERVER_NESTED_EFFECT: &str = r#"
store: [
    invoke: Session/store.invoke
    reading:
        ClockNotRead |> HOLD reading {
            invoke |> THEN { Clock/wall() }
        }
    forwarded:
        0 |> HOLD forwarded {
        reading |> THEN {
            reading |> WHEN {
                WallClockRead => Session/echo(value: reading.unix_seconds)
                __ => 0
            }
        }
    }
]
"#;

const SESSION_GLOBAL_INVOCATION: &str = r#"
store: [ready: True]

FUNCTION echo(value) {
    value
}
"#;

const SERVER_GLOBAL_INVOCATION: &str = r#"
store: [
    invoke: SOURCE
    result:
        invoke |> THEN { Session/echo(value: 1) }
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
    server_messages: Vec<DistributedMessage>,
    server_deliveries: Vec<DistributedMessage>,
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
            server_messages: Vec::new(),
            server_deliveries: Vec::new(),
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
            let server_messages = take_session_server_messages(&mut self.session, 64);
            progressed |= !server_messages.is_empty();
            for message in server_messages {
                self.server_messages.push(message.clone());
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
            self.server_deliveries.push(delivery.message.clone());
            self.session
                .accept_server_message(delivery.message)
                .unwrap();
        }
    }

    fn dispatch_client_event_to_server(&mut self, path: &str) -> Vec<RuntimeTurn> {
        let messages = self.dispatch_client_event_to_session_messages(path);
        let mut turns = Vec::new();
        for message in messages {
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

    fn dispatch_client_event_to_session_messages(&mut self, path: &str) -> Vec<DistributedMessage> {
        self.client
            .dispatch(path, SourcePayload::default())
            .unwrap();
        for frame in take_client_session_frames(&mut self.client, 64) {
            self.session.admit_client_frame(&frame).unwrap();
        }
        while self.session.poll_client_frame().unwrap().is_some() {}
        take_session_server_messages(&mut self.session, 64)
    }
}

#[test]
fn browser_boundary_moves_events_only_as_canonical_cbor() {
    let bundle = bundle(CLIENT_EVENT, SESSION_EVENT, SERVER_SIMPLE);
    let mut runtime = PhysicalLoopback::start(&bundle);
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
fn repeated_equal_physical_invocations_remain_fifo_and_publish_each_result() {
    let mut runtime = PhysicalLoopback::start(&bundle(
        CLIENT_INVOCATION,
        SESSION_INVOCATION,
        SERVER_IDENTITY,
    ));

    for expected_sequence in 1..=2 {
        let messages = runtime.dispatch_client_event_to_session_messages("store.invoke");
        let invocations = messages
            .iter()
            .filter_map(|message| match &message.payload {
                DistributedMessagePayload::InvocationRequest {
                    sequence,
                    arguments,
                    ..
                } => Some((*sequence, arguments)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            invocations.len(),
            1,
            "one physical click must emit exactly one invocation"
        );
        assert_eq!(invocations[0].0, expected_sequence);
        assert_eq!(
            invocations[0].1.values().collect::<Vec<_>>(),
            vec![&DataValue::integer(7).unwrap()]
        );

        let mut results = Vec::new();
        for message in messages {
            let update = runtime
                .server
                .bind(&mut runtime.server_machine)
                .accept_session_message(runtime.origin, message)
                .unwrap();
            results.extend(update.deliveries.iter().filter_map(|delivery| {
                match &delivery.message.payload {
                    DistributedMessagePayload::InvocationResult {
                        sequence, value, ..
                    } => Some((*sequence, value.clone())),
                    _ => None,
                }
            }));
            runtime.deliver_server(update);
        }
        assert_eq!(
            results,
            vec![(expected_sequence, DataValue::integer(7).unwrap())]
        );
        runtime.pump();
        assert_eq!(
            runtime.session.root_value_current("store.result").unwrap(),
            Value::integer(7).unwrap()
        );
    }
}

#[test]
fn inactive_distributed_call_branch_emits_no_invocation() {
    let mut runtime = PhysicalLoopback::start(&bundle(
        CLIENT_INVOCATION,
        SESSION_FALSE_INVOCATION,
        SERVER_IDENTITY,
    ));
    let messages = runtime.dispatch_client_event_to_session_messages("store.invoke");
    assert!(
        messages.iter().all(|message| !matches!(
            message.payload,
            DistributedMessagePayload::InvocationRequest { .. }
        )),
        "a distributed call in an inactive branch must not be invoked"
    );
}

#[test]
fn invocation_retry_replays_result_without_reexecution_or_duplicate_result_event() {
    let mut runtime = PhysicalLoopback::start(&bundle(
        CLIENT_INVOCATION,
        SESSION_INVOCATION,
        SERVER_IDENTITY,
    ));
    let request = runtime
        .dispatch_client_event_to_session_messages("store.invoke")
        .into_iter()
        .find(|message| {
            matches!(
                message.payload,
                DistributedMessagePayload::InvocationRequest { .. }
            )
        })
        .expect("physical click must emit an invocation request");

    let first = runtime
        .server
        .bind(&mut runtime.server_machine)
        .accept_session_message(runtime.origin, request.clone())
        .unwrap();
    assert!(
        !first.turns.is_empty(),
        "the first request must enter the producer machine"
    );
    let first_result = first
        .deliveries
        .iter()
        .find_map(|delivery| {
            matches!(
                delivery.message.payload,
                DistributedMessagePayload::InvocationResult { .. }
            )
            .then(|| delivery.message.clone())
        })
        .expect("first execution must publish its result");

    let replay = runtime
        .server
        .bind(&mut runtime.server_machine)
        .accept_session_message(runtime.origin, request.clone())
        .unwrap();
    assert!(
        replay.turns.is_empty(),
        "an identical retry must not execute the producer again"
    );
    let replay_result = replay
        .deliveries
        .iter()
        .find_map(|delivery| {
            matches!(
                delivery.message.payload,
                DistributedMessagePayload::InvocationResult { .. }
            )
            .then(|| delivery.message.clone())
        })
        .expect("an identical retry must replay the cached result");
    assert_eq!(replay_result, first_result);

    runtime
        .session
        .accept_server_message(first_result.clone())
        .unwrap();
    let duplicate = runtime.session.accept_server_message(first_result).unwrap();
    assert!(duplicate.turns.is_empty());
    assert_eq!(
        runtime.session.root_value_current("store.result").unwrap(),
        Value::integer(7).unwrap()
    );

    let mut conflicting = request;
    let DistributedMessagePayload::InvocationRequest { arguments, .. } = &mut conflicting.payload
    else {
        unreachable!("selected request is an invocation")
    };
    *arguments.values_mut().next().expect("identity argument") = DataValue::integer(8).unwrap();
    assert!(matches!(
        runtime
            .server
            .bind(&mut runtime.server_machine)
            .accept_session_message(runtime.origin, conflicting),
        Err(DistributedRuntimeError::InvalidTransportFrame)
    ));
}

#[test]
fn reusable_wrappers_keep_remote_call_instances_and_results_distinct() {
    let mut runtime =
        PhysicalLoopback::start(&bundle(CLIENT_WRAPPED_CALLS, SESSION_ADD, SERVER_SIMPLE));
    assert_eq!(
        runtime.client.root_value_current("store.first").unwrap(),
        Value::integer(6).unwrap()
    );
    assert_eq!(
        runtime.client.root_value_current("store.second").unwrap(),
        Value::integer(9).unwrap()
    );
}

#[test]
fn nested_current_call_updates_cross_all_three_islands_without_reissuing_demand() {
    let mut runtime = PhysicalLoopback::start(&bundle(
        CLIENT_NESTED_CURRENT,
        SESSION_NESTED_CURRENT,
        SERVER_ADD_ONE,
    ));

    assert_eq!(
        runtime.client.root_value_current("store.result").unwrap(),
        Value::integer(42).unwrap()
    );
    assert_eq!(
        runtime
            .server_messages
            .iter()
            .filter(|message| matches!(
                message.payload,
                DistributedMessagePayload::CurrentCallRequest { .. }
            ))
            .count(),
        1,
        "the nested Server demand remains one live request"
    );
    assert_eq!(
        runtime
            .server_deliveries
            .iter()
            .filter(|message| matches!(
                message.payload,
                DistributedMessagePayload::CurrentCallResult {
                    demand_revision: 1,
                    result_revision: 1,
                    ..
                }
            ))
            .count(),
        1,
        "the Server answers that demand once"
    );
}

#[test]
fn client_publication_backpressure_rolls_back_machine_and_transport_state() {
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
    take_session_server_messages(&mut session, 64);

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
    take_session_server_messages(&mut session, 64);
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
    take_session_server_messages(&mut session, 64);

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

    take_session_server_messages(&mut session, 64);
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
    let source_turn_index = update
        .turns
        .iter()
        .position(|turn| turn.source_sequence.is_some())
        .expect("the Session event has one authoritative source turn");
    assert!(
        update
            .turns
            .iter()
            .any(|turn| { turn.sequence == 1 && turn.source_sequence.is_some() })
    );
    assert!(
        update.turns[source_turn_index + 1..]
            .iter()
            .all(|turn| turn.source_sequence.is_none()),
        "post-turn context updates must follow the authoritative source turn"
    );
    machine.barrier().unwrap();
    let status = machine.persistence_status();
    assert_eq!(status.durable_through_turn_sequence, 1);
    assert!(status.last_error.is_none());
    machine.shutdown().unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn persistent_current_call_is_execution_only() {
    let bundle = bundle(CLIENT_CURRENT, SESSION_SERVER_CURRENT, SERVER_DOUBLE);
    let artifact = bundle.artifact(ProgramRole::Server).unwrap();
    let edge = artifact
        .plan()
        .distributed_endpoint
        .as_ref()
        .unwrap()
        .wire_schema
        .call_edges
        .iter()
        .find(|edge| {
            edge.caller_role == ProgramRole::Session
                && edge.callee_role == ProgramRole::Server
                && edge.mode == boon_plan::DistributedCallMode::Current
        })
        .unwrap();
    let argument = edge.parameters.first().unwrap();
    let call_instance_id = DistributedCallInstanceId::from_rows(edge.call_site_id, &[]).unwrap();
    let (mut machine, _) = PersistentProgramSession::start(
        artifact.clone(),
        boon_persistence::InMemoryDriver::default(),
        boon_persistence::PersistenceWorkerConfig::default(),
    )
    .unwrap();
    machine
        .set_machine_origin(SessionOrigin::new(0, 1).unwrap())
        .unwrap();

    let (value, turn) = machine
        .prepare_distributed_function_instance(
            edge.call_site_id,
            call_instance_id,
            edge.function_export_id,
            1,
            BTreeMap::from([(argument.argument_id, Value::integer(21).unwrap())]),
            true,
        )
        .unwrap();
    assert_eq!(value, Value::integer(42).unwrap());
    let committed = machine
        .commit_prepared_distributed_turn(turn.expect("Current producer lease turn"))
        .unwrap();
    assert_eq!(
        committed.outcome,
        PersistentDistributedCommitOutcome::ExecutionOnly
    );
    machine.barrier().unwrap();
    assert_eq!(
        machine.persistence_status().durable_through_turn_sequence,
        0
    );
    machine.shutdown().unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn persistent_invocation_is_durably_admitted_without_persisting_lease_state() {
    let bundle = bundle(CLIENT_EVENT, SESSION_SERVER_REMEMBER, SERVER_REMEMBER);
    let artifact = bundle.artifact(ProgramRole::Server).unwrap();
    let edge = artifact
        .plan()
        .distributed_endpoint
        .as_ref()
        .unwrap()
        .wire_schema
        .call_edges
        .iter()
        .find(|edge| {
            edge.caller_role == ProgramRole::Session
                && edge.callee_role == ProgramRole::Server
                && edge.mode == boon_plan::DistributedCallMode::Invocation
        })
        .unwrap();
    let argument = edge.parameters.first().unwrap();
    let call_instance_id = DistributedCallInstanceId::from_rows(edge.call_site_id, &[]).unwrap();
    let (mut machine, _) = PersistentProgramSession::start(
        artifact.clone(),
        boon_persistence::InMemoryDriver::default(),
        boon_persistence::PersistenceWorkerConfig::default(),
    )
    .unwrap();
    machine
        .set_machine_origin(SessionOrigin::new(0, 1).unwrap())
        .unwrap();

    let (value, turn) = machine
        .prepare_distributed_function_instance(
            edge.call_site_id,
            call_instance_id,
            edge.function_export_id,
            1,
            BTreeMap::from([(argument.argument_id, Value::integer(7).unwrap())]),
            true,
        )
        .unwrap();
    assert_eq!(value, Value::integer(7).unwrap());
    let turn = turn.expect("Invocation authority turn");
    assert!(
        turn.durable_changes.is_empty(),
        "process-local producer HOLD state must not enter global persistence"
    );
    let committed = machine.commit_prepared_distributed_turn(turn).unwrap();
    assert!(matches!(
        committed.outcome,
        PersistentDistributedCommitOutcome::ImmediateAcknowledged(_)
    ));

    let (value, turn) = machine
        .prepare_distributed_function_instance(
            edge.call_site_id,
            call_instance_id,
            edge.function_export_id,
            2,
            BTreeMap::from([(argument.argument_id, Value::integer(8).unwrap())]),
            true,
        )
        .unwrap();
    assert_eq!(
        value,
        Value::integer(7).unwrap(),
        "the process-local producer HOLD must remain live for the call-site lease"
    );
    let turn = turn.expect("second Invocation authority turn");
    assert!(turn.durable_changes.is_empty());
    assert!(matches!(
        machine
            .commit_prepared_distributed_turn(turn)
            .unwrap()
            .outcome,
        PersistentDistributedCommitOutcome::ImmediateAcknowledged(_)
    ));
    machine.barrier().unwrap();
    assert_eq!(
        machine.persistence_status().durable_through_turn_sequence,
        2
    );
    machine.shutdown().unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn persistent_invocation_can_use_bounded_buffered_admission() {
    let bundle = bundle(CLIENT_EVENT, SESSION_SERVER_REMEMBER, SERVER_REMEMBER);
    let artifact = bundle.artifact(ProgramRole::Server).unwrap();
    let edge = artifact
        .plan()
        .distributed_endpoint
        .as_ref()
        .unwrap()
        .wire_schema
        .call_edges
        .iter()
        .find(|edge| {
            edge.caller_role == ProgramRole::Session
                && edge.callee_role == ProgramRole::Server
                && edge.mode == boon_plan::DistributedCallMode::Invocation
        })
        .unwrap();
    let argument = edge.parameters.first().unwrap();
    let call_instance_id = DistributedCallInstanceId::from_rows(edge.call_site_id, &[]).unwrap();
    let (mut machine, _) = PersistentProgramSession::start(
        artifact.clone(),
        boon_persistence::InMemoryDriver::default(),
        boon_persistence::PersistenceWorkerConfig::default(),
    )
    .unwrap();
    machine
        .set_machine_origin(SessionOrigin::new(0, 1).unwrap())
        .unwrap();

    let (_, turn) = machine
        .prepare_distributed_function_instance(
            edge.call_site_id,
            call_instance_id,
            edge.function_export_id,
            1,
            BTreeMap::from([(argument.argument_id, Value::integer(7).unwrap())]),
            false,
        )
        .unwrap();
    let committed = machine
        .commit_prepared_distributed_turn(turn.expect("Invocation authority turn"))
        .unwrap();
    assert_eq!(
        committed.outcome,
        PersistentDistributedCommitOutcome::BufferedAccepted
    );
    machine.barrier().unwrap();
    assert_eq!(
        machine.persistence_status().durable_through_turn_sequence,
        1
    );
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
    take_session_server_messages(&mut session, 64);
    session.mark_current().unwrap();
    while session.next_client_frame().unwrap().is_some() {
        assert!(session.acknowledge_client_frame());
    }
    take_session_server_messages(&mut session, 64);

    let pending_before_stale = session.pending_client_frames();
    session.mark_stale().unwrap();

    assert_eq!(session.pending_client_frames(), pending_before_stale);
    assert_eq!(session.pending_server_messages(), 1);
}

#[test]
fn stale_session_discards_queued_server_events_but_keeps_current_snapshots() {
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
            DistributedQueueLimits::default(),
        )
        .unwrap();

    client.bind(test_session_id(), 1, 0).unwrap();
    client.mark_current().unwrap();
    session.mark_current().unwrap();
    pump_client_session_transport(&mut client, &mut session);
    take_session_server_messages(&mut session, 64);

    client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    pump_client_session_transport(&mut client, &mut session);
    assert_eq!(session.pending_server_messages(), 2);

    session.mark_stale().unwrap();

    assert_eq!(session.pending_server_messages(), 1);
    let retained = session.next_server_message().unwrap();
    assert!(matches!(
        retained.payload,
        DistributedMessagePayload::Current { .. }
    ));
    assert!(session.acknowledge_server_message());
}

#[test]
fn generation_cutover_discards_client_events_and_republishes_current_state() {
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
            DistributedQueueLimits::default(),
        )
        .unwrap();

    client.bind(test_session_id(), 1, 0).unwrap();
    client.mark_current().unwrap();
    session.mark_current().unwrap();
    pump_client_session_transport(&mut client, &mut session);

    client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    assert!(client.pending_session_frames() > 0);
    let applied_server_through = client.applied_server_through();
    let applied_client_through = session.applied_client_through();

    client.mark_stale().unwrap();
    session.mark_stale().unwrap();
    session.rebind_client(2, applied_server_through).unwrap();
    client
        .bind(test_session_id(), 2, applied_client_through)
        .unwrap();
    client.mark_current().unwrap();
    session.mark_current().unwrap();
    pump_client_session_transport(&mut client, &mut session);

    assert_eq!(
        client.root_value_current("store.count").unwrap(),
        Value::integer(1).unwrap()
    );
    assert_eq!(
        session.root_value_current("store.client_count").unwrap(),
        Value::integer(1).unwrap(),
        "Current state is regenerated for the new generation"
    );
    assert_eq!(
        session.root_value_current("store.session_count").unwrap(),
        Value::integer(0).unwrap(),
        "the stale Event is not replayed"
    );

    client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    pump_client_session_transport(&mut client, &mut session);
    assert_eq!(
        session.root_value_current("store.session_count").unwrap(),
        Value::integer(1).unwrap(),
        "a newer semantic Event may follow an intentionally discarded sequence"
    );
}

#[test]
fn generation_cutover_discards_invocation_results_and_accepts_new_work() {
    let bundle = bundle(CLIENT_SESSION_INVOCATION, SESSION_CALL, SERVER_SIMPLE);
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
            DistributedQueueLimits::default(),
        )
        .unwrap();

    client.bind(test_session_id(), 1, 0).unwrap();
    client.mark_current().unwrap();
    session.mark_current().unwrap();
    pump_client_session_transport(&mut client, &mut session);

    client
        .dispatch("store.invoke", SourcePayload::default())
        .unwrap();
    for frame in take_client_session_frames(&mut client, 64) {
        session.admit_client_frame(&frame).unwrap();
    }
    while session.poll_client_frame().unwrap().is_some() {}
    assert!(session.pending_client_frames() > 0);
    assert_eq!(
        client.root_value_current("store.result").unwrap(),
        Value::integer(0).unwrap()
    );

    let applied_server_through = client.applied_server_through();
    let applied_client_through = session.applied_client_through();
    client.mark_stale().unwrap();
    session.mark_stale().unwrap();
    session.rebind_client(2, applied_server_through).unwrap();
    client
        .bind(test_session_id(), 2, applied_client_through)
        .unwrap();
    client.mark_current().unwrap();
    session.mark_current().unwrap();
    pump_client_session_transport(&mut client, &mut session);
    assert_eq!(
        client.root_value_current("store.result").unwrap(),
        Value::integer(0).unwrap(),
        "the old generation's InvocationResult is not replayed"
    );

    client
        .dispatch("store.invoke", SourcePayload::default())
        .unwrap();
    pump_client_session_transport(&mut client, &mut session);
    assert_eq!(
        client.root_value_current("store.result").unwrap(),
        Value::integer(42).unwrap(),
        "new invocation work remains live after the cutover"
    );
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
fn server_function_leases_are_origin_scoped_transactional_and_generation_bounded() {
    let bundle = bundle(CLIENT_EVENT, SESSION_SERVER_REMEMBER, SERVER_REMEMBER);
    let server_artifact = bundle.artifact(ProgramRole::Server).unwrap();
    let mut server = DistributedServerRuntime::start(server_artifact).unwrap();
    let mut machine = ProgramSession::start(server_artifact.clone()).unwrap();
    let endpoint = server_artifact
        .plan()
        .distributed_endpoint
        .as_ref()
        .expect("Server distributed endpoint");
    let edge = endpoint
        .wire_schema
        .call_edges
        .iter()
        .find(|edge| {
            edge.caller_role == ProgramRole::Session && edge.callee_role == ProgramRole::Server
        })
        .expect("Session-to-Server function edge");
    let argument = edge.parameters.first().expect("remember value parameter");
    assert_eq!(edge.mode, boon_plan::DistributedCallMode::Invocation);
    let call_instance_id = DistributedCallInstanceId::from_rows(edge.call_site_id, &[]).unwrap();
    let first = SessionOrigin::new(30, 1).unwrap();
    let second = SessionOrigin::new(31, 1).unwrap();
    let rolled_back = SessionOrigin::new(32, 1).unwrap();

    for (origin, scope) in [(first, 101), (second, 102), (rolled_back, 103)] {
        server
            .bind(&mut machine)
            .attach_origin(origin, SessionPrincipal::Anonymous, scope)
            .unwrap();
    }
    assert!(
        server
            .bind(&mut machine)
            .attach_origin(
                SessionOrigin::new(first.slot(), first.generation() + 1).unwrap(),
                SessionPrincipal::Anonymous,
                105,
            )
            .is_err(),
        "one Session slot cannot own two live generations"
    );

    let call = |sequence: u64, value: i64| DistributedMessage {
        producer: ProgramRole::Session,
        consumer: ProgramRole::Server,
        payload: DistributedMessagePayload::InvocationRequest {
            call_site_id: edge.call_site_id,
            call_instance_id,
            function_export_id: edge.function_export_id,
            sequence,
            arguments: BTreeMap::from([(argument.argument_id, DataValue::integer(value).unwrap())]),
        },
    };
    let result = |update: &DistributedServerUpdate| {
        update
            .deliveries
            .iter()
            .find_map(|delivery| match &delivery.message.payload {
                DistributedMessagePayload::InvocationResult { value, .. } => Some(value.clone()),
                _ => None,
            })
            .expect("scoped function result")
    };

    let first_initial = server
        .bind(&mut machine)
        .accept_session_message(first, call(1, 7))
        .unwrap();
    assert_eq!(result(&first_initial), DataValue::integer(7).unwrap());
    let first_updated = server
        .bind(&mut machine)
        .accept_session_message(first, call(2, 8))
        .unwrap();
    assert_eq!(
        result(&first_updated),
        DataValue::integer(7).unwrap(),
        "one origin must retain its call-site HOLD authority"
    );
    let second_initial = server
        .bind(&mut machine)
        .accept_session_message(second, call(1, 9))
        .unwrap();
    assert_eq!(
        result(&second_initial),
        DataValue::integer(9).unwrap(),
        "another origin must initialize independent function authority"
    );

    let prepared = server
        .bind(&mut machine)
        .prepare_session_message(rolled_back, call(1, 11))
        .unwrap();
    server
        .bind(&mut machine)
        .rollback_prepared_transaction(prepared)
        .unwrap();
    let retried = server
        .bind(&mut machine)
        .accept_session_message(rolled_back, call(1, 12))
        .unwrap();
    assert_eq!(
        result(&retried),
        DataValue::integer(12).unwrap(),
        "rolling back first publication must discard the candidate lease"
    );

    server.bind(&mut machine).expire_origin(first).unwrap();
    let next_generation = SessionOrigin::new(first.slot(), first.generation() + 1).unwrap();
    server
        .bind(&mut machine)
        .attach_origin(next_generation, SessionPrincipal::Anonymous, 104)
        .unwrap();
    let restarted = server
        .bind(&mut machine)
        .accept_session_message(next_generation, call(1, 13))
        .unwrap();
    assert_eq!(
        result(&restarted),
        DataValue::integer(13).unwrap(),
        "slot reuse with a new generation must not recover expired function state"
    );
    assert!(matches!(
        server
            .bind(&mut machine)
            .accept_session_message(first, call(3, 14)),
        Err(DistributedRuntimeError::InvalidLease)
    ));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn prepared_current_call_rolls_back_and_open_transactions_fail_closed() {
    let bundle = bundle(
        CLIENT_EVENT,
        SESSION_ZERO_ARGUMENT_CURRENT,
        SERVER_ZERO_ARGUMENT_CURRENT,
    );
    let artifact = bundle.artifact(ProgramRole::Server).unwrap();
    let edge = artifact
        .plan()
        .distributed_endpoint
        .as_ref()
        .unwrap()
        .wire_schema
        .call_edges
        .iter()
        .find(|edge| {
            edge.caller_role == ProgramRole::Session
                && edge.callee_role == ProgramRole::Server
                && edge.mode == boon_plan::DistributedCallMode::Current
        })
        .cloned()
        .unwrap();
    assert!(edge.parameters.is_empty());
    let call_instance_id = DistributedCallInstanceId::from_rows(edge.call_site_id, &[]).unwrap();
    let origin = SessionOrigin::new(7, 1).unwrap();
    let mut server = DistributedServerRuntime::start(artifact).unwrap();
    let mut machine =
        ProgramSession::start(bundle.artifact(ProgramRole::Server).unwrap().clone()).unwrap();
    server
        .bind(&mut machine)
        .attach_origin(origin, SessionPrincipal::Anonymous, 77)
        .unwrap();

    let request = DistributedMessage {
        producer: ProgramRole::Session,
        consumer: ProgramRole::Server,
        payload: DistributedMessagePayload::CurrentCallRequest {
            call_site_id: edge.call_site_id,
            call_instance_id,
            function_export_id: edge.function_export_id,
            demand_revision: 1,
            arguments: BTreeMap::new(),
        },
    };
    let prepared = server
        .bind(&mut machine)
        .prepare_session_message(origin, request.clone())
        .unwrap();
    assert!(
        server
            .bind(&mut machine)
            .prepare_global_read_transaction()
            .is_err(),
        "a second transaction cannot fork from a stale base"
    );
    server
        .bind(&mut machine)
        .rollback_prepared_transaction(prepared)
        .unwrap();

    server
        .bind(&mut machine)
        .accept_session_message(origin, request)
        .unwrap();
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
    let mut restarted_server = DistributedServerRuntime::start(artifact).unwrap();
    let mut restarted_machine = ProgramSession::start(artifact.clone()).unwrap();
    assert_eq!(
        restarted_server
            .bind(&mut restarted_machine)
            .pending_transient_effect_count(runtime.origin),
        0,
        "process restart must not restore Session-owned effect calls"
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

#[test]
fn origin_effect_completion_routes_each_nested_invocation_once() {
    let bundle = bundle(
        CLIENT_INVOCATION,
        SESSION_NESTED_SERVER_EFFECT,
        SERVER_NESTED_EFFECT,
    );
    let mut runtime = PhysicalLoopback::start(&bundle);
    let effect = runtime
        .dispatch_client_event_to_server("store.invoke")
        .iter()
        .flat_map(|turn| &turn.transient_effects)
        .next()
        .cloned()
        .expect("Server effect invocation");

    let update = runtime
        .server
        .bind(&mut runtime.server_machine)
        .complete_transient_effect(
            effect.call_id,
            Value::Record(BTreeMap::from([
                ("$tag".to_owned(), Value::Text("WallClockRead".to_owned())),
                ("unix_seconds".to_owned(), Value::integer(789).unwrap()),
                ("nanoseconds".to_owned(), Value::integer(0).unwrap()),
            ])),
        )
        .unwrap();
    let nested = update
        .deliveries
        .iter()
        .filter(|delivery| {
            delivery.target == ServerDeliveryTarget::Origin(runtime.origin)
                && matches!(
                    delivery.message.payload,
                    DistributedMessagePayload::InvocationRequest { .. }
                )
        })
        .count();
    assert_eq!(
        nested, 1,
        "one effect result must route one nested Invocation"
    );
    runtime.deliver_server(update);
    runtime.pump();
    assert!(
        runtime
            .server
            .bind(&mut runtime.server_machine)
            .complete_transient_effect(effect.call_id, Value::Null)
            .is_err(),
        "a completed effect cannot replay its nested Invocation"
    );
}

#[test]
fn global_turn_with_origin_scoped_invocation_fails_and_rolls_back() {
    let bundle = bundle(
        CLIENT_EVENT,
        SESSION_GLOBAL_INVOCATION,
        SERVER_GLOBAL_INVOCATION,
    );
    let artifact = bundle.artifact(ProgramRole::Server).unwrap();
    let mut server = DistributedServerRuntime::start(artifact).unwrap();
    let mut machine = ProgramSession::start(artifact.clone()).unwrap();

    for attempt in 1..=2 {
        let error = match server.bind(&mut machine).prepare_global_source_transaction(
            "store.invoke",
            SourcePayload::default(),
            false,
        ) {
            Ok(_) => panic!("Global turn unexpectedly retained an origin-scoped Invocation"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains(
                "Global-owned Server turn emitted an origin-scoped distributed invocation"
            ),
            "attempt {attempt} failed for the wrong reason: {error}"
        );
        assert!(
            !machine.runtime().has_unsettled_turn(),
            "attempt {attempt} leaked the rejected Global machine turn"
        );
    }
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
