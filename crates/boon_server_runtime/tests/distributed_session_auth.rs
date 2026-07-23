use boon_plan::ProgramRole;
use boon_runtime::{
    ApplicationIdentity, DistributedClientRuntime, DistributedProgramBundle,
    DistributedQueueLimits, ProgramCapabilityProfile, ProgramCompileRequest, RuntimeSourceUnit,
    SessionPrincipal, SourcePayload, Value, compile_distributed_program_bundle,
};
use boon_server_host::{
    DISTRIBUTED_SESSION_TRANSPORT_PATH, DistributedSessionOpen, ServerConfig, bind,
};
use boon_server_runtime::{
    BoonServerProgram, DistributedSessionAuthenticator, DistributedSessionRegistryConfig,
};
use boon_wire::{
    ClientCommit, ClientHello, ClientSessionFrame, ClientSessionFrameLimits, ResumeToken,
    ServerOffer, SessionControlFrame, SessionId, decode_client_session_frame,
    decode_session_control_frame, encode_session_control_frame,
};
use futures::{SinkExt, StreamExt};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

type ClientSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const CLIENT: &str = r#"
store: [
    noop: SOURCE
    principal_label: Session/store.principal_label
]

scene: Scene/Element/text(
    element: [events: [press: store.noop]]
    style: [width: Fill]
    text: store.principal_label
)
"#;

const SESSION: &str = r#"
store: [
    principal: SessionInfo/principal()
    principal_label:
        principal |> WHEN {
            Authenticated => principal.subject
            Anonymous => TEXT { Anonymous }
        }
]
"#;

const SERVER: &str = r#"
store: [ready: True]
"#;

const ISOLATION_CLIENT: &str = r#"
store: [
    increment: SOURCE
    principal_label: Session/store.principal_label
    count: Session/store.count
    shared_count: Session/store.shared_count
    doubled: Session/store.doubled
]

scene: Scene/Element/text(
    element: [events: [press: store.increment]]
    style: [width: Fill]
    text: store.principal_label
)
"#;

const ISOLATION_SESSION: &str = r#"
store: [
    principal: SessionInfo/principal()
    principal_label:
        principal |> WHEN {
            Authenticated => principal.subject
            Anonymous => TEXT { Anonymous }
        }
    increment: Client/store.increment
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
    shared_count: Server/store.shared_count
    doubled: Server/double(value: count)
]
"#;

const ISOLATION_SERVER: &str = r#"
store: [
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

struct HeaderAuthenticator {
    observed: Arc<Mutex<Vec<String>>>,
}

struct CredentialAuthenticator {
    expected: &'static str,
}

impl DistributedSessionAuthenticator for HeaderAuthenticator {
    fn authenticate(&mut self, open: &DistributedSessionOpen) -> Option<SessionPrincipal> {
        let subject = open
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case("x-test-subject"))
            .and_then(|header| std::str::from_utf8(&header.value).ok())?
            .to_owned();
        self.observed.lock().unwrap().push(subject.clone());
        SessionPrincipal::authenticated(subject, ["member"]).ok()
    }
}

impl DistributedSessionAuthenticator for CredentialAuthenticator {
    fn authenticate(&mut self, open: &DistributedSessionOpen) -> Option<SessionPrincipal> {
        let credential = open
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case("x-test-credential"))
            .and_then(|header| std::str::from_utf8(&header.value).ok())?;
        (credential == self.expected)
            .then(|| SessionPrincipal::authenticated("alice", ["member"]).ok())
            .flatten()
    }
}

fn loopback() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
}

fn bundle() -> DistributedProgramBundle {
    compile_distributed_program_bundle(&[
        request(ProgramRole::Client, CLIENT),
        request(ProgramRole::Session, SESSION),
        request(ProgramRole::Server, SERVER),
    ])
    .unwrap()
}

fn isolation_bundle() -> DistributedProgramBundle {
    compile_distributed_program_bundle(&[
        request(ProgramRole::Client, ISOLATION_CLIENT),
        request(ProgramRole::Session, ISOLATION_SESSION),
        request(ProgramRole::Server, ISOLATION_SERVER),
    ])
    .unwrap()
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
            "dev.boon.distributed-auth-test",
            role.as_str(),
            "loopback",
        ),
        capability_profile: match role {
            ProgramRole::Client => ProgramCapabilityProfile::PublicClient,
            ProgramRole::Session => ProgramCapabilityProfile::TrustedSession,
            ProgramRole::Server => ProgramCapabilityProfile::TrustedServer,
        },
    }
}

async fn connect(address: SocketAddr, subject: &str) -> ClientSocket {
    connect_with_header(address, "x-test-subject", subject).await
}

async fn connect_with_header(address: SocketAddr, name: &str, value: &str) -> ClientSocket {
    let mut request = format!("ws://{address}{DISTRIBUTED_SESSION_TRANSPORT_PATH}")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        name.parse::<HeaderName>().expect("valid test header name"),
        HeaderValue::from_str(value).unwrap(),
    );
    connect_async(request).await.unwrap().0
}

async fn next_binary(socket: &mut ClientSocket) -> Vec<u8> {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("timed out waiting for distributed Session frame")
            .expect("distributed Session socket ended")
            .expect("distributed Session socket failed");
        match message {
            Message::Binary(bytes) => return bytes.to_vec(),
            Message::Ping(payload) => socket.send(Message::Pong(payload)).await.unwrap(),
            Message::Pong(_) => {}
            other => panic!("expected binary Session frame, got {other:?}"),
        }
    }
}

async fn begin(
    socket: &mut ClientSocket,
    identity: boon_server_runtime::DistributedSessionRegistryIdentity,
    token: Option<ResumeToken>,
) -> SessionControlFrame {
    let hello = encode_session_control_frame(&SessionControlFrame::ClientHello(ClientHello::new(
        identity.graph_id,
        identity.graph_revision,
        identity.schema_hash,
        token,
        0,
    )))
    .unwrap();
    socket.send(Message::Binary(hello.into())).await.unwrap();
    decode_session_control_frame(&next_binary(socket).await).unwrap()
}

async fn commit_with_binding(
    socket: &mut ClientSocket,
    offer: ServerOffer,
) -> (ResumeToken, SessionId, u64, u64) {
    let (token, session_id, generation, applied_client_through) = offer.into_parts();
    let frame = encode_session_control_frame(&SessionControlFrame::ClientCommit(
        ClientCommit::new(session_id, generation, 0),
    ))
    .unwrap();
    socket.send(Message::Binary(frame.into())).await.unwrap();
    let ready = decode_session_control_frame(&next_binary(socket).await).unwrap();
    assert!(matches!(ready, SessionControlFrame::ServerReady(_)));
    assert_eq!(applied_client_through, 0);
    (token, session_id, generation, applied_client_through)
}

async fn commit(socket: &mut ClientSocket, offer: ServerOffer) -> ResumeToken {
    commit_with_binding(socket, offer).await.0
}

async fn expect_client_text(socket: &mut ClientSocket, expected: &str) {
    for _ in 0..8 {
        let frame = decode_client_session_frame(
            &next_binary(socket).await,
            ClientSessionFrameLimits::default(),
        )
        .unwrap();
        if matches!(
            frame,
            ClientSessionFrame::Data {
                payload: boon_data::Value::Text(value),
                ..
            } if value == expected
        ) {
            return;
        }
    }
    panic!("Client did not receive the Session-projected principal label `{expected}`");
}

async fn expect_client_text_without_secret(
    socket: &mut ClientSocket,
    expected: &str,
    secret: &[u8],
) {
    for _ in 0..8 {
        let bytes = next_binary(socket).await;
        assert!(
            !bytes.windows(secret.len()).any(|window| window == secret),
            "raw credential leaked into Client/Session transport"
        );
        let frame =
            decode_client_session_frame(&bytes, ClientSessionFrameLimits::default()).unwrap();
        if matches!(
            frame,
            ClientSessionFrame::Data {
                payload: boon_data::Value::Text(value),
                ..
            } if value == expected
        ) {
            return;
        }
    }
    panic!("Client did not receive the credential-mapped principal label `{expected}`");
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
            "timed out waiting for authenticated distributed roots: {}",
            expected
                .iter()
                .map(|(path, _)| *path)
                .collect::<Vec<_>>()
                .join(", ")
        );
        let next = tokio::time::timeout(remaining, socket.next())
            .await
            .expect("timed out waiting for authenticated distributed Session state");
        let message = next
            .expect("authenticated distributed Session socket ended during synchronization")
            .expect("authenticated distributed Session frame failed during synchronization");
        match message {
            Message::Binary(bytes) => {
                client.accept_session_frame(&bytes).unwrap();
            }
            Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.unwrap(),
            Message::Pong(_) => {}
            other => panic!(
                "unexpected authenticated distributed Session synchronization frame: {other:?}"
            ),
        }
    }
}

async fn send_client_frames(socket: &mut ClientSocket, client: &mut DistributedClientRuntime) {
    let mut sent = 0usize;
    for _ in 0..16 {
        let Some(frame) = client.next_session_frame().unwrap() else {
            break;
        };
        socket.send(Message::Binary(frame.into())).await.unwrap();
        assert!(client.acknowledge_session_frame());
        sent += 1;
    }
    assert!(
        sent > 0,
        "authenticated Client event must produce a Session frame"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn production_transport_isolates_concurrent_authenticated_users() {
    let bundle = isolation_bundle();
    let mut program =
        BoonServerProgram::new_distributed(&bundle, DistributedSessionRegistryConfig::default())
            .unwrap();
    let identity = program.distributed_identity().unwrap();
    let observed = Arc::new(Mutex::new(Vec::new()));
    program.set_distributed_session_authenticator(Box::new(HeaderAuthenticator {
        observed: Arc::clone(&observed),
    }));
    let mut server_config = ServerConfig::default();
    server_config
        .request_header_allowlist
        .insert("x-test-subject".to_owned());
    server_config.timeouts.websocket_ping_interval = None;
    let server = bind(loopback(), server_config, program).await.unwrap();

    let mut alice_socket = connect(server.local_addr(), "alice").await;
    let SessionControlFrame::ServerOffer(alice_offer) =
        begin(&mut alice_socket, identity, None).await
    else {
        panic!("Alice must receive a fresh Session offer");
    };
    let (_alice_token, alice_session_id, alice_generation, alice_applied_client) =
        commit_with_binding(&mut alice_socket, alice_offer).await;

    let mut bob_socket = connect(server.local_addr(), "bob").await;
    let SessionControlFrame::ServerOffer(bob_offer) = begin(&mut bob_socket, identity, None).await
    else {
        panic!("Bob must receive a fresh Session offer");
    };
    let (_bob_token, bob_session_id, bob_generation, bob_applied_client) =
        commit_with_binding(&mut bob_socket, bob_offer).await;

    let mut alice_client = DistributedClientRuntime::start(
        bundle.artifact(ProgramRole::Client).unwrap(),
        DistributedQueueLimits::default(),
    )
    .unwrap();
    alice_client
        .bind(alice_session_id, alice_generation, alice_applied_client)
        .unwrap();
    alice_client.mark_current().unwrap();
    let mut bob_client = DistributedClientRuntime::start(
        bundle.artifact(ProgramRole::Client).unwrap(),
        DistributedQueueLimits::default(),
    )
    .unwrap();
    bob_client
        .bind(bob_session_id, bob_generation, bob_applied_client)
        .unwrap();
    bob_client.mark_current().unwrap();

    let alice_initial = [
        ("store.principal_label", Value::Text("alice".to_owned())),
        ("store.count", Value::integer(0).unwrap()),
        ("store.shared_count", Value::integer(0).unwrap()),
        ("store.doubled", Value::integer(0).unwrap()),
    ];
    let bob_initial = [
        ("store.principal_label", Value::Text("bob".to_owned())),
        ("store.count", Value::integer(0).unwrap()),
        ("store.shared_count", Value::integer(0).unwrap()),
        ("store.doubled", Value::integer(0).unwrap()),
    ];
    tokio::join!(
        accept_until_roots(&mut alice_socket, &mut alice_client, &alice_initial),
        accept_until_roots(&mut bob_socket, &mut bob_client, &bob_initial),
    );

    alice_client
        .dispatch("store.increment", SourcePayload::default())
        .unwrap();
    send_client_frames(&mut alice_socket, &mut alice_client).await;

    let alice_updated = [
        ("store.principal_label", Value::Text("alice".to_owned())),
        ("store.count", Value::integer(1).unwrap()),
        ("store.shared_count", Value::integer(1).unwrap()),
        ("store.doubled", Value::integer(2).unwrap()),
    ];
    let bob_updated = [
        ("store.principal_label", Value::Text("bob".to_owned())),
        ("store.count", Value::integer(0).unwrap()),
        ("store.shared_count", Value::integer(1).unwrap()),
        ("store.doubled", Value::integer(0).unwrap()),
    ];
    tokio::join!(
        accept_until_roots(&mut alice_socket, &mut alice_client, &alice_updated),
        accept_until_roots(&mut bob_socket, &mut bob_client, &bob_updated),
    );

    assert_eq!(
        alice_client
            .root_value_current("store.principal_label")
            .unwrap(),
        Value::Text("alice".to_owned())
    );
    assert_eq!(
        bob_client
            .root_value_current("store.principal_label")
            .unwrap(),
        Value::Text("bob".to_owned())
    );
    assert_eq!(
        alice_client.root_value_current("store.count").unwrap(),
        Value::integer(1).unwrap()
    );
    assert_eq!(
        bob_client.root_value_current("store.count").unwrap(),
        Value::integer(0).unwrap(),
        "Alice's event must not mutate Bob's Session state"
    );
    assert_eq!(
        alice_client.root_value_current("store.doubled").unwrap(),
        Value::integer(2).unwrap()
    );
    assert_eq!(
        bob_client.root_value_current("store.doubled").unwrap(),
        Value::integer(0).unwrap(),
        "Alice's origin-scoped Server call result must not reach Bob"
    );
    assert_eq!(
        alice_client
            .root_value_current("store.shared_count")
            .unwrap(),
        Value::integer(1).unwrap()
    );
    assert_eq!(
        bob_client.root_value_current("store.shared_count").unwrap(),
        Value::integer(1).unwrap(),
        "independent Server Current state must broadcast to both authenticated users"
    );
    assert_eq!(&*observed.lock().unwrap(), &["alice", "bob"]);

    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn production_transport_keeps_raw_credentials_host_private() {
    const CREDENTIAL: &str = "credential-must-never-enter-boon-or-wire-7f3b";

    let bundle = bundle();
    let mut program =
        BoonServerProgram::new_distributed(&bundle, DistributedSessionRegistryConfig::default())
            .unwrap();
    let identity = program.distributed_identity().unwrap();
    program.set_distributed_session_authenticator(Box::new(CredentialAuthenticator {
        expected: CREDENTIAL,
    }));
    let mut server_config = ServerConfig::default();
    server_config
        .request_header_allowlist
        .insert("x-test-credential".to_owned());
    server_config.timeouts.websocket_ping_interval = None;
    let server = bind(loopback(), server_config, program).await.unwrap();

    let mut socket =
        connect_with_header(server.local_addr(), "x-test-credential", CREDENTIAL).await;
    let SessionControlFrame::ServerOffer(offer) = begin(&mut socket, identity, None).await else {
        panic!("valid credential must receive a fresh Session offer");
    };
    let _token = commit(&mut socket, offer).await;
    expect_client_text_without_secret(&mut socket, "alice", CREDENTIAL.as_bytes()).await;

    drop(socket);
    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn production_transport_binds_resume_authority_to_the_authenticated_principal() {
    let bundle = bundle();
    let mut program =
        BoonServerProgram::new_distributed(&bundle, DistributedSessionRegistryConfig::default())
            .unwrap();
    let identity = program.distributed_identity().unwrap();
    let observed = Arc::new(Mutex::new(Vec::new()));
    program.set_distributed_session_authenticator(Box::new(HeaderAuthenticator {
        observed: Arc::clone(&observed),
    }));
    let mut server_config = ServerConfig::default();
    server_config
        .request_header_allowlist
        .insert("x-test-subject".to_owned());
    let server = bind(loopback(), server_config, program).await.unwrap();

    let mut alice = connect(server.local_addr(), "alice").await;
    let SessionControlFrame::ServerOffer(offer) = begin(&mut alice, identity, None).await else {
        panic!("Alice must receive a fresh Session offer");
    };
    let token = commit(&mut alice, offer).await;
    expect_client_text(&mut alice, "alice").await;
    let lookup = token.to_lookup_key();
    alice.close(None).await.unwrap();
    drop(alice);

    let mut bob = connect(server.local_addr(), "bob").await;
    assert!(matches!(
        begin(&mut bob, identity, Some(lookup.to_resume_token())).await,
        SessionControlFrame::ServerReject(_)
    ));
    drop(bob);

    let resume_bytes = *lookup.to_resume_token().as_bytes();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut resumed_alice = loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "Alice must retain resume authority after Bob is rejected"
        );
        let mut candidate = connect(server.local_addr(), "alice").await;
        if let SessionControlFrame::ServerOffer(resumed_offer) = begin(
            &mut candidate,
            identity,
            Some(ResumeToken::from_bytes(resume_bytes)),
        )
        .await
        {
            let _rotated_token = commit(&mut candidate, resumed_offer).await;
            break candidate;
        }
        drop(candidate);
        tokio::time::sleep(Duration::from_millis(5)).await;
    };

    assert_eq!(&*observed.lock().unwrap(), &["alice", "bob", "alice"]);
    resumed_alice.close(None).await.unwrap();
    server.shutdown().await.unwrap();
}
