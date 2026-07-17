use boon_persistence::{PersistenceWorkerConfig, RedbDriver};
use boon_runtime::{
    ApplicationIdentity, ProgramCapabilityProfile, ProgramCompileRequest, RuntimeSourceUnit,
    compile_program_artifact,
};
use boon_server_host::{ServerConfig, bind};
use boon_server_runtime::{BoonServerProgram, PersistentServerConfig, ServerLifecyclePhase};
use futures::{SinkExt, StreamExt};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{Error as ClientWebSocketError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

type ClientSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

fn loopback() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
}

fn compile_artifact(path: &str, source: &str, namespace: &str) -> boon_runtime::ProgramArtifact {
    compile_program_artifact(&ProgramCompileRequest {
        revision: 1,
        entry_path: path.to_owned(),
        units: vec![RuntimeSourceUnit {
            path: path.to_owned(),
            source: source.to_owned(),
        }],
        application: ApplicationIdentity::new(
            "dev.boon.server-runtime-test",
            namespace,
            "loopback",
        ),
        role: boon_plan::ProgramRole::Server,
        capability_profile: ProgramCapabilityProfile::TrustedServer,
    })
    .expect("unrelated server fixture should compile as TrustedServer")
}

fn compile_fixture(path: &str, source: &str, namespace: &str) -> BoonServerProgram {
    BoonServerProgram::new(compile_artifact(path, source, namespace))
        .expect("compiled host-port metadata should resolve by stable IDs")
}

fn websocket_url(address: SocketAddr, path: &str) -> String {
    format!("ws://{address}{path}")
}

async fn connect(address: SocketAddr) -> ClientSocket {
    connect_async(websocket_url(address, "/socket"))
        .await
        .expect("WebSocket upgrade should be accepted")
        .0
}

async fn next_message(socket: &mut ClientSocket) -> Message {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("timed out waiting for WebSocket frame")
            .expect("WebSocket stream ended")
            .expect("WebSocket read failed");
        match message {
            Message::Ping(payload) => socket.send(Message::Pong(payload)).await.unwrap(),
            Message::Pong(_) => {}
            message => return message,
        }
    }
}

async fn send_text(socket: &mut ClientSocket, text: &str) {
    socket
        .send(Message::Text(text.to_owned().into()))
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unrelated_boon_program_serves_a_real_http_request() {
    let program = compile_fixture(
        "server_http_echo.bn",
        include_str!("../../../examples/server_http_echo.bn"),
        "http-echo-test",
    );
    let server = bind(loopback(), ServerConfig::default(), program)
        .await
        .expect("loopback server should bind");

    let response = reqwest::Client::new()
        .get(format!("http://{}/health/detail", server.local_addr()))
        .send()
        .await
        .expect("loopback GET should complete");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "GET:health");

    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_response_body_preserves_application_owned_bytes() {
    let program = compile_fixture(
        "server-http-bytes.bn",
        r#"
store: [
    request: SOURCE
]
outputs: [
    response: [
        status: 200
        headers: LIST {
            [name: TEXT { content-type }, value: TEXT { application/octet-stream }]
        }
        body: BYTES[4] { 16uff, 16u00, 16u80, 16u41 }
    ]
]
host_ports: [
    http: [
        request: store.request
        response: response
    ]
]
"#,
        "binary-response-test",
    );
    let server = bind(loopback(), ServerConfig::default(), program)
        .await
        .expect("loopback server should bind");

    let response = reqwest::get(format!("http://{}/bytes", server.local_addr()))
        .await
        .expect("loopback GET should complete");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.headers()[reqwest::header::CONTENT_TYPE],
        "application/octet-stream"
    );
    assert_eq!(
        response.bytes().await.unwrap().as_ref(),
        &[0xff, 0x00, 0x80, 0x41]
    );

    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persistent_counter_is_acknowledged_and_restored_through_real_http() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("server.redb");
    let artifact = compile_artifact(
        "server_persistent_counter.bn",
        include_str!("../../../examples/server_persistent_counter.bn"),
        "persistent-counter-loopback",
    );
    let restart_artifact = artifact.clone();
    let (program, startup) = BoonServerProgram::with_persistence(
        artifact,
        RedbDriver::open(&database).unwrap(),
        PersistentServerConfig::authoritative(PersistenceWorkerConfig::default()),
    )
    .unwrap();
    assert_eq!(
        startup.lifecycle.status().phase,
        ServerLifecyclePhase::Ready
    );
    let server = bind(loopback(), ServerConfig::default(), program)
        .await
        .expect("persistent server should bind only after startup is ready");
    let response = reqwest::Client::new()
        .post(format!("http://{}/counter", server.local_addr()))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "1");
    assert_eq!(startup.lifecycle.status().durably_acknowledged_turns, 1);
    assert!(startup.lifecycle.status().persistence.pending.is_none());
    server.shutdown().await.unwrap();
    assert_eq!(
        startup.lifecycle.status().phase,
        ServerLifecyclePhase::Stopped
    );

    let (program, restart) = BoonServerProgram::with_persistence(
        restart_artifact,
        RedbDriver::open(&database).unwrap(),
        PersistentServerConfig::authoritative(PersistenceWorkerConfig::default()),
    )
    .unwrap();
    let server = bind(loopback(), ServerConfig::default(), program)
        .await
        .unwrap();
    let response = reqwest::Client::new()
        .post(format!("http://{}/counter", server.local_addr()))
        .send()
        .await
        .unwrap();
    assert_eq!(response.text().await.unwrap(), "2");
    assert_eq!(restart.lifecycle.status().durably_acknowledged_turns, 1);
    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unrelated_boon_program_drives_real_websocket_actions() {
    let program = compile_fixture(
        "server_websocket_echo.bn",
        include_str!("../../../examples/server_websocket_echo.bn"),
        "websocket-echo-test",
    );
    let mut config = ServerConfig::default();
    config.timeouts.websocket_ping_interval = None;
    let server = bind(loopback(), config, program)
        .await
        .expect("loopback server should bind");
    let address = server.local_addr();

    let denied = connect_async(websocket_url(address, "/reject"))
        .await
        .expect_err("/reject should reject the WebSocket upgrade");
    match denied {
        ClientWebSocketError::Http(response) => assert_eq!(
            response.status().as_u16(),
            403,
            "rejection body: {:?}",
            response.body()
        ),
        error => panic!("expected rejected upgrade, got {error}"),
    }

    let mut first = connect(address).await;
    let mut second = connect(address).await;

    send_text(&mut first, "hello").await;
    assert_eq!(
        next_message(&mut first).await,
        Message::Text("hello".into())
    );
    first
        .send(Message::Binary(vec![1, 2, 3].into()))
        .await
        .unwrap();
    assert_eq!(
        next_message(&mut first).await,
        Message::Binary(vec![1, 2, 3].into())
    );
    send_text(&mut first, "send").await;
    assert_eq!(next_message(&mut first).await, Message::Text("sent".into()));
    send_text(&mut first, "resync").await;
    assert_eq!(
        next_message(&mut first).await,
        Message::Text("snapshot".into())
    );

    send_text(&mut first, "join").await;
    send_text(&mut first, "first-joined").await;
    assert_eq!(
        next_message(&mut first).await,
        Message::Text("first-joined".into())
    );
    send_text(&mut second, "join").await;
    send_text(&mut second, "second-joined").await;
    assert_eq!(
        next_message(&mut second).await,
        Message::Text("second-joined".into())
    );

    send_text(&mut first, "broadcast").await;
    assert_eq!(
        next_message(&mut second).await,
        Message::Text("room:hello".into())
    );
    send_text(&mut first, "after-broadcast").await;
    assert_eq!(
        next_message(&mut first).await,
        Message::Text("after-broadcast".into())
    );

    send_text(&mut first, "close").await;
    let Message::Close(Some(close)) = next_message(&mut first).await else {
        panic!("expected application close frame");
    };
    assert_eq!(u16::from(close.code), 1000);
    assert_eq!(close.reason, "done");

    let mut malformed = connect(address).await;
    send_text(&mut malformed, "malformed").await;
    let Message::Close(Some(close)) = next_message(&mut malformed).await else {
        panic!("expected malformed-output close frame");
    };
    assert_eq!(u16::from(close.code), 1011);
    assert_eq!(close.reason, "invalid Boon WebSocket output");

    drop(second);
    server.shutdown().await.unwrap();
}
