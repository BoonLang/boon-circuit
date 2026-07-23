use async_trait::async_trait;
use boon_plan::ProgramRole;
use boon_runtime::{
    ApplicationIdentity, DistributedClientRuntime, DistributedProgramBundle,
    DistributedQueueLimits, ProgramCapabilityProfile, ProgramCompileRequest, RuntimeSourceUnit,
    SourcePayload, TransientEffectCallId, TransientEffectInvocation, Value,
    compile_distributed_program_bundle,
};
use boon_server_host::{DISTRIBUTED_SESSION_TRANSPORT_PATH, ServerConfig, bind};
use boon_server_runtime::{
    BoonServerProgram, DistributedSessionRegistryConfig, TransientEffectHost,
    TransientEffectHostError, TransientEffectHostEvent, TransientEffectLimits,
};
use boon_wire::{
    ClientCommit, ClientHello, ResumeToken, SessionControlFrame, SessionId,
    decode_session_control_frame, encode_session_control_frame,
};
use futures::{SinkExt, StreamExt};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

type ClientSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const CLIENT_SOURCE: &str = r#"
store: [
    increment: SOURCE
    count: Session/store.count
    shared_count: Session/store.shared_count
    doubled: Session/store.doubled
]

scene: Scene/Element/text(
    element: [events: [press: store.increment]]
    style: [width: Fill]
    text: TEXT { Distributed Session }
)
"#;

const CLIENT_EFFECT_SOURCE: &str = r#"
store: [
    increment: SOURCE
    count: Session/store.count
]

scene: Scene/Element/text(
    element: [events: [press: store.increment]]
    style: [width: Fill]
    text: TEXT { Distributed Session Effect }
)
"#;

const SESSION_SOURCE: &str = r#"
store: [
    increment: Client/store.increment
    ready: Server/store.ready
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
    shared_count: Server/store.shared_count
    doubled: Server/double(value: count)
]
"#;

const SERVER_SOURCE: &str = r#"
store: [
    ready: True
    increment: Session/store.increment
    shared_count:
        0 |> HOLD shared_count {
            increment |> THEN { shared_count + 1 }
        }
]

FUNCTION double(value) {
    value * 2
}
"#;

const SESSION_EFFECT_SOURCE: &str = r#"
store: [
    increment: Client/store.increment
    ready: Server/store.ready
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
    random:
        NotRequested |> HOLD random {
            increment |> THEN { Random/bytes(byte_count: 1) }
        }
]
"#;

fn loopback() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
}

fn compile_bundle() -> DistributedProgramBundle {
    compile_distributed_program_bundle(&[
        request(ProgramRole::Client, CLIENT_SOURCE),
        request(ProgramRole::Session, SESSION_SOURCE),
        request(ProgramRole::Server, SERVER_SOURCE),
    ])
    .expect("distributed loopback fixture should compile")
}

fn compile_effect_bundle() -> DistributedProgramBundle {
    compile_distributed_program_bundle(&[
        request(ProgramRole::Client, CLIENT_EFFECT_SOURCE),
        request(ProgramRole::Session, SESSION_EFFECT_SOURCE),
        request(ProgramRole::Server, SERVER_SOURCE),
    ])
    .expect("distributed effect loopback fixture should compile")
}

struct FailingRandomHost;

#[async_trait]
impl TransientEffectHost for FailingRandomHost {
    fn owns(&self, effect_id: boon_plan::EffectId) -> bool {
        effect_id
            == boon_plan::EffectId::from_host_operation(
                boon_effect_schema::SECURE_RANDOM_BYTES_OPERATION,
            )
            .unwrap()
    }

    fn submit(
        &mut self,
        calls: Vec<TransientEffectInvocation>,
    ) -> Result<(), TransientEffectHostError> {
        assert_eq!(calls.len(), 1);
        Ok(())
    }

    async fn next_event(&mut self) -> Result<TransientEffectHostEvent, TransientEffectHostError> {
        Err(TransientEffectHostError::new("injected host failure"))
    }

    fn cancel(&mut self, _call_id: TransientEffectCallId) {}
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
            "dev.boon.distributed-loopback",
            format!("test-{}", role.as_str()),
            "distributed-loopback",
        ),
        capability_profile: match role {
            ProgramRole::Client => ProgramCapabilityProfile::PublicClient,
            ProgramRole::Session => ProgramCapabilityProfile::TrustedSession,
            ProgramRole::Server => ProgramCapabilityProfile::TrustedServer,
        },
    }
}

async fn connect(address: SocketAddr) -> ClientSocket {
    connect_async(format!(
        "ws://{address}{DISTRIBUTED_SESSION_TRANSPORT_PATH}"
    ))
    .await
    .expect("distributed Session WebSocket should connect")
    .0
}

async fn next_binary(socket: &mut ClientSocket) -> Vec<u8> {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("timed out waiting for distributed Session frame")
            .expect("distributed Session socket ended")
            .expect("distributed Session socket read failed");
        match message {
            Message::Binary(bytes) => return bytes.to_vec(),
            Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.unwrap(),
            Message::Pong(_) => {}
            other => panic!("expected binary distributed Session frame, got {other:?}"),
        }
    }
}

fn identity(bundle: &DistributedProgramBundle) -> ([u8; 32], u64, [u8; 32]) {
    let endpoint = bundle
        .artifact(ProgramRole::Client)
        .unwrap()
        .plan()
        .distributed_endpoint
        .as_ref()
        .unwrap();
    (
        endpoint.graph.graph_id.0,
        endpoint.graph.revision,
        endpoint.wire_schema_hash,
    )
}

async fn handshake(
    socket: &mut ClientSocket,
    identity: ([u8; 32], u64, [u8; 32]),
    resume_token: Option<ResumeToken>,
    applied_server_through: u64,
) -> Option<(ResumeToken, SessionId, u64, u64)> {
    let hello = encode_session_control_frame(&SessionControlFrame::ClientHello(ClientHello::new(
        identity.0,
        identity.1,
        identity.2,
        resume_token,
        applied_server_through,
    )))
    .unwrap();
    socket.send(Message::Binary(hello.into())).await.unwrap();
    let offer = next_binary(socket).await;
    let SessionControlFrame::ServerOffer(offer) = decode_session_control_frame(&offer).unwrap()
    else {
        return None;
    };
    let (token, session_id, generation, applied_client_through) = offer.into_parts();
    let commit = encode_session_control_frame(&SessionControlFrame::ClientCommit(
        ClientCommit::new(session_id, generation, applied_server_through),
    ))
    .unwrap();
    socket.send(Message::Binary(commit.into())).await.unwrap();
    let ready = next_binary(socket).await;
    let SessionControlFrame::ServerReady(ready) = decode_session_control_frame(&ready).unwrap()
    else {
        panic!("distributed Session handshake did not finish with ServerReady");
    };
    assert!(ready.session_id() == session_id);
    assert_eq!(ready.generation(), generation);
    assert_eq!(ready.applied_client_through(), applied_client_through);
    Some((token, session_id, generation, applied_client_through))
}

async fn accept_until_roots(
    socket: &mut ClientSocket,
    client: &mut DistributedClientRuntime,
    expected: &[(&str, Value)],
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if expected.iter().all(|(path, expected)| {
            client
                .root_value_current(path)
                .is_ok_and(|actual| actual == *expected)
        }) {
            return;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(
            !remaining.is_zero(),
            "timed out waiting for distributed roots: {}",
            expected
                .iter()
                .map(|(path, _)| *path)
                .collect::<Vec<_>>()
                .join(", ")
        );
        let next = tokio::time::timeout(remaining, socket.next())
            .await
            .expect("timed out waiting for distributed Session state");
        let message = next
            .expect("distributed Session socket ended during synchronization")
            .expect("distributed Session frame failed during synchronization");
        match message {
            Message::Binary(bytes) => {
                client.accept_session_frame(&bytes).unwrap();
            }
            Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.unwrap(),
            Message::Pong(_) => {}
            other => panic!("unexpected distributed Session synchronization frame: {other:?}"),
        }
    }
}

async fn send_one_client_frame(socket: &mut ClientSocket, client: &mut DistributedClientRuntime) {
    let mut sent = 0usize;
    for _ in 0..16 {
        let Some(frame) = client.next_session_frame().unwrap() else {
            break;
        };
        socket.send(Message::Binary(frame.into())).await.unwrap();
        assert!(client.acknowledge_session_frame());
        sent += 1;
    }
    assert!(sent > 0, "Client event should produce a Session frame");
}

async fn expect_transport_close(socket: &mut ClientSocket) -> u16 {
    let close = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let message = socket
                .next()
                .await
                .expect("invalid Session transport should send a close frame")
                .expect("invalid Session transport close should be readable");
            match message {
                Message::Close(close) => break close,
                Message::Binary(_) | Message::Pong(_) => {}
                Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.unwrap(),
                other => panic!("unexpected invalid Session transport frame: {other:?}"),
            }
        }
    })
    .await
    .expect("invalid Session transport must close promptly")
    .expect("invalid Session transport close must carry a reason");
    u16::from(close.code)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_reserved_transport_is_scoped_lossless_and_resumable() {
    let bundle = compile_bundle();
    let identity = identity(&bundle);
    let program =
        BoonServerProgram::new_distributed(&bundle, DistributedSessionRegistryConfig::default())
            .unwrap();
    let mut config = ServerConfig::default();
    config.timeouts.websocket_ping_interval = None;
    let server = bind(loopback(), config, program).await.unwrap();
    let address = server.local_addr();

    let mut first_socket = connect(address).await;
    let (first_token, first_session_id, first_generation, first_applied_client) =
        handshake(&mut first_socket, identity, None, 0)
            .await
            .expect("fresh first Session should be accepted");
    let mut second_socket = connect(address).await;
    let (_second_token, second_session_id, second_generation, second_applied_client) =
        handshake(&mut second_socket, identity, None, 0)
            .await
            .expect("fresh second Session should be accepted");

    let mut first_client = DistributedClientRuntime::start(
        bundle.artifact(ProgramRole::Client).unwrap(),
        DistributedQueueLimits::default(),
    )
    .unwrap();
    first_client
        .bind(first_session_id, first_generation, first_applied_client)
        .unwrap();
    first_client.mark_current().unwrap();
    let mut second_client = DistributedClientRuntime::start(
        bundle.artifact(ProgramRole::Client).unwrap(),
        DistributedQueueLimits::default(),
    )
    .unwrap();
    second_client
        .bind(second_session_id, second_generation, second_applied_client)
        .unwrap();
    second_client.mark_current().unwrap();
    let initial = [
        ("store.count", Value::integer(0).unwrap()),
        ("store.shared_count", Value::integer(0).unwrap()),
        ("store.doubled", Value::integer(0).unwrap()),
    ];
    tokio::join!(
        accept_until_roots(&mut first_socket, &mut first_client, &initial),
        accept_until_roots(&mut second_socket, &mut second_client, &initial),
    );

    first_client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    send_one_client_frame(&mut first_socket, &mut first_client).await;
    let first_updated = [
        ("store.count", Value::integer(1).unwrap()),
        ("store.shared_count", Value::integer(1).unwrap()),
        ("store.doubled", Value::integer(2).unwrap()),
    ];
    let second_updated = [
        ("store.count", Value::integer(0).unwrap()),
        ("store.shared_count", Value::integer(1).unwrap()),
        ("store.doubled", Value::integer(0).unwrap()),
    ];
    tokio::join!(
        accept_until_roots(&mut first_socket, &mut first_client, &first_updated),
        accept_until_roots(&mut second_socket, &mut second_client, &second_updated),
    );
    assert_eq!(
        first_client.root_value_current("store.count").unwrap(),
        Value::integer(1).unwrap()
    );
    assert_eq!(
        second_client.root_value_current("store.count").unwrap(),
        Value::integer(0).unwrap()
    );
    assert_eq!(
        first_client
            .root_value_current("store.shared_count")
            .unwrap(),
        Value::integer(1).unwrap()
    );
    assert_eq!(
        second_client
            .root_value_current("store.shared_count")
            .unwrap(),
        Value::integer(1).unwrap(),
        "independent Server Current state must broadcast to every subscribed Session"
    );
    assert_eq!(
        first_client.root_value_current("store.doubled").unwrap(),
        Value::integer(2).unwrap()
    );
    assert_eq!(
        second_client.root_value_current("store.doubled").unwrap(),
        Value::integer(0).unwrap(),
        "a current-call reply derived from one Session must remain origin-scoped"
    );

    let resume_bytes = *first_token.as_bytes();
    first_client.mark_stale().unwrap();
    first_socket.close(None).await.unwrap();
    drop(first_socket);

    let (mut resumed_socket, resumed_session_id, resumed_generation, resumed_applied_client) = {
        let mut resumed = None;
        for _ in 0..100 {
            let mut socket = connect(address).await;
            if let Some((_next_token, session_id, generation, applied_client_through)) = handshake(
                &mut socket,
                identity,
                Some(ResumeToken::from_bytes(resume_bytes)),
                first_client.applied_server_through(),
            )
            .await
            {
                resumed = Some((socket, session_id, generation, applied_client_through));
                break;
            }
            drop(socket);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        resumed.expect("Session should resume after its old socket closes")
    };
    first_client
        .bind(
            resumed_session_id,
            resumed_generation,
            resumed_applied_client,
        )
        .unwrap();
    first_client.mark_current().unwrap();
    accept_until_roots(
        &mut resumed_socket,
        &mut first_client,
        &[("store.count", Value::integer(1).unwrap())],
    )
    .await;
    assert_eq!(
        first_client.root_value_current("store.count").unwrap(),
        Value::integer(1).unwrap(),
        "resumed Session must retain its isolated state"
    );

    drop(resumed_socket);
    drop(second_socket);
    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hostile_transport_lifecycle_is_fail_closed_and_connection_local() {
    let bundle = compile_bundle();
    let identity = identity(&bundle);
    let registry_config = DistributedSessionRegistryConfig {
        handshake_timeout: Duration::from_millis(100),
        resume_window: Duration::from_secs(1),
        ..DistributedSessionRegistryConfig::default()
    };
    let program = BoonServerProgram::new_distributed(&bundle, registry_config).unwrap();
    let mut config = ServerConfig::default();
    config.timeouts.websocket_ping_interval = None;
    let server = bind(loopback(), config, program).await.unwrap();
    let address = server.local_addr();

    let mut wrong_schema = identity;
    wrong_schema.2[0] ^= 0xff;
    let mut rejected_socket = connect(address).await;
    assert!(
        handshake(&mut rejected_socket, wrong_schema, None, 0)
            .await
            .is_none()
    );
    drop(rejected_socket);

    let mut stale_socket = connect(address).await;
    let (resume_token, stale_session_id, stale_generation, stale_applied_client) =
        handshake(&mut stale_socket, identity, None, 0)
            .await
            .expect("first healthy Session should be accepted");
    let mut healthy_socket = connect(address).await;
    let (_healthy_token, healthy_session_id, healthy_generation, healthy_applied_client) =
        handshake(&mut healthy_socket, identity, None, 0)
            .await
            .expect("second healthy Session should be accepted");

    let mut stale_client = DistributedClientRuntime::start(
        bundle.artifact(ProgramRole::Client).unwrap(),
        DistributedQueueLimits::default(),
    )
    .unwrap();
    stale_client
        .bind(stale_session_id, stale_generation, stale_applied_client)
        .unwrap();
    stale_client.mark_current().unwrap();
    let mut healthy_client = DistributedClientRuntime::start(
        bundle.artifact(ProgramRole::Client).unwrap(),
        DistributedQueueLimits::default(),
    )
    .unwrap();
    healthy_client
        .bind(
            healthy_session_id,
            healthy_generation,
            healthy_applied_client,
        )
        .unwrap();
    healthy_client.mark_current().unwrap();
    let initial = [
        ("store.count", Value::integer(0).unwrap()),
        ("store.shared_count", Value::integer(0).unwrap()),
        ("store.doubled", Value::integer(0).unwrap()),
    ];
    tokio::join!(
        accept_until_roots(&mut stale_socket, &mut stale_client, &initial),
        accept_until_roots(&mut healthy_socket, &mut healthy_client, &initial),
    );

    stale_client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    let stale_generation_frame = stale_client
        .next_session_frame()
        .unwrap()
        .expect("generation-one event frame");
    assert!(stale_client.acknowledge_session_frame());
    stale_client.mark_stale().unwrap();
    stale_socket.close(None).await.unwrap();
    drop(stale_socket);

    let resume_bytes = *resume_token.as_bytes();
    let (mut resumed_socket, resumed_token) = {
        let mut resumed = None;
        for _ in 0..100 {
            let mut socket = connect(address).await;
            if let Some((token, _session_id, generation, _applied_client)) = handshake(
                &mut socket,
                identity,
                Some(ResumeToken::from_bytes(resume_bytes)),
                stale_client.applied_server_through(),
            )
            .await
            {
                assert_eq!(generation, stale_generation + 1);
                resumed = Some((socket, token));
                break;
            }
            drop(socket);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        resumed.expect("first Session should resume before its test deadline")
    };

    resumed_socket
        .send(Message::Binary(stale_generation_frame.into()))
        .await
        .unwrap();
    assert_eq!(expect_transport_close(&mut resumed_socket).await, 1002);
    drop(resumed_socket);

    healthy_client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    send_one_client_frame(&mut healthy_socket, &mut healthy_client).await;
    accept_until_roots(
        &mut healthy_socket,
        &mut healthy_client,
        &[
            ("store.count", Value::integer(1).unwrap()),
            ("store.shared_count", Value::integer(1).unwrap()),
            ("store.doubled", Value::integer(2).unwrap()),
        ],
    )
    .await;

    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let mut expired_socket = connect(address).await;
    assert!(
        handshake(&mut expired_socket, identity, Some(resumed_token), 0)
            .await
            .is_none()
    );
    drop(expired_socket);

    healthy_client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    send_one_client_frame(&mut healthy_socket, &mut healthy_client).await;
    accept_until_roots(
        &mut healthy_socket,
        &mut healthy_client,
        &[
            ("store.count", Value::integer(2).unwrap()),
            ("store.shared_count", Value::integer(2).unwrap()),
            ("store.doubled", Value::integer(4).unwrap()),
        ],
    )
    .await;

    drop(healthy_socket);
    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn distributed_host_failure_closes_the_stalled_session_transport() {
    let bundle = compile_effect_bundle();
    let identity = identity(&bundle);
    let mut program =
        BoonServerProgram::new_distributed(&bundle, DistributedSessionRegistryConfig::default())
            .unwrap();
    program
        .attach_transient_effect_host(
            Box::new(FailingRandomHost),
            TransientEffectLimits::default(),
        )
        .unwrap();
    let mut config = ServerConfig::default();
    config.timeouts.websocket_ping_interval = None;
    let server = bind(loopback(), config, program).await.unwrap();
    let mut socket = connect(server.local_addr()).await;
    let (_token, session_id, generation, applied_client_through) =
        handshake(&mut socket, identity, None, 0)
            .await
            .expect("effect Session should be accepted");
    let mut client = DistributedClientRuntime::start(
        bundle.artifact(ProgramRole::Client).unwrap(),
        DistributedQueueLimits::default(),
    )
    .unwrap();
    client
        .bind(session_id, generation, applied_client_through)
        .unwrap();
    client.mark_current().unwrap();
    accept_until_roots(
        &mut socket,
        &mut client,
        &[("store.count", Value::integer(0).unwrap())],
    )
    .await;
    client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    send_one_client_frame(&mut socket, &mut client).await;

    let close = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let message = socket
                .next()
                .await
                .expect("failed Session transport should send a close frame")
                .expect("failed Session transport close should be readable");
            match message {
                Message::Close(close) => break close,
                Message::Binary(bytes) => {
                    let _ = client.accept_session_frame(&bytes);
                }
                Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.unwrap(),
                Message::Pong(_) => {}
                other => panic!("unexpected failed Session transport frame: {other:?}"),
            }
        }
    })
    .await
    .expect("host failure must not leave the Session transport stalled")
    .expect("host failure close frame must carry a reason");
    assert_eq!(u16::from(close.code), 1011);
    server.shutdown().await.unwrap();
}
