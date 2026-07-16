use async_trait::async_trait;
use boon_server_host::{
    CallCancellation, CancellationReason, Header, HttpRequest, HttpResponse, OriginPolicy,
    ServerConfig, ServerProgram, SlowClientPolicy, TrustedProxyPolicy, WebSocketAction,
    WebSocketClose, WebSocketEvent, WebSocketFrame, WebSocketTransportError, bind,
};
use futures::{SinkExt, StreamExt};
use reqwest::StatusCode;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::{Barrier, Notify, Semaphore};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{Error as ClientWebSocketError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

type ClientSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Clone)]
struct Probe {
    active_calls: Arc<AtomicUsize>,
    overlapping_call: Arc<AtomicBool>,
    http_calls: Arc<AtomicUsize>,
    cancellations: Arc<AtomicUsize>,
    transport_errors: Arc<AtomicUsize>,
    shutdowns: Arc<AtomicUsize>,
    block_started: Arc<Notify>,
    block_gate: Arc<Semaphore>,
}

impl Default for Probe {
    fn default() -> Self {
        Self {
            active_calls: Arc::new(AtomicUsize::new(0)),
            overlapping_call: Arc::new(AtomicBool::new(false)),
            http_calls: Arc::new(AtomicUsize::new(0)),
            cancellations: Arc::new(AtomicUsize::new(0)),
            transport_errors: Arc::new(AtomicUsize::new(0)),
            shutdowns: Arc::new(AtomicUsize::new(0)),
            block_started: Arc::new(Notify::new()),
            block_gate: Arc::new(Semaphore::new(0)),
        }
    }
}

impl Probe {
    fn enter(&self) -> CallGuard {
        if self.active_calls.fetch_add(1, Ordering::SeqCst) != 0 {
            self.overlapping_call.store(true, Ordering::SeqCst);
        }
        CallGuard {
            active_calls: Arc::clone(&self.active_calls),
        }
    }
}

struct CallGuard {
    active_calls: Arc<AtomicUsize>,
}

impl Drop for CallGuard {
    fn drop(&mut self) {
        self.active_calls.fetch_sub(1, Ordering::SeqCst);
    }
}

struct SwitchboardProgram {
    probe: Probe,
}

impl SwitchboardProgram {
    fn new(probe: Probe) -> Self {
        Self { probe }
    }
}

#[async_trait]
impl ServerProgram for SwitchboardProgram {
    async fn on_http(
        &mut self,
        request: HttpRequest,
        _cancellation: CallCancellation,
    ) -> HttpResponse {
        let _call = self.probe.enter();
        self.probe.http_calls.fetch_add(1, Ordering::SeqCst);
        match request.path_segments.first().map(String::as_str) {
            Some("slow") => {
                tokio::time::sleep(Duration::from_millis(250)).await;
                HttpResponse::new(200, "too late")
            }
            Some("block") => {
                self.probe.block_started.notify_waiters();
                let permit = self.probe.block_gate.acquire().await.unwrap();
                permit.forget();
                HttpResponse::new(200, "released")
            }
            Some("echo") => {
                let query = request
                    .query
                    .get("x")
                    .map(|values| values.join(","))
                    .unwrap_or_default();
                let header = request
                    .headers
                    .iter()
                    .find(|header| header.name == "x-switchboard")
                    .map(|header| String::from_utf8_lossy(&header.value).into_owned())
                    .unwrap_or_default();
                let peer = match request.peer {
                    boon_server_host::PeerAddress::Known(address) => address.ip().to_string(),
                    boon_server_host::PeerAddress::Unavailable => "unavailable".to_owned(),
                };
                let cookies = request
                    .cookies
                    .iter()
                    .map(|cookie| format!("{}={}", cookie.name, cookie.value))
                    .collect::<Vec<_>>()
                    .join(",");
                let body = format!(
                    "{}|{}|{}|{}|{}|{}|{}|{}",
                    request.method,
                    request.path_segments.join("/"),
                    query,
                    header,
                    String::from_utf8_lossy(&request.body),
                    peer,
                    request.scheme.as_str(),
                    cookies,
                );
                HttpResponse {
                    status: 201,
                    headers: vec![Header::new("x-switchboard", b"yes".to_vec())],
                    body: body.into_bytes(),
                }
            }
            _ => HttpResponse::new(404, "not found"),
        }
    }

    async fn on_websocket(
        &mut self,
        event: WebSocketEvent,
        _cancellation: CallCancellation,
    ) -> Vec<WebSocketAction> {
        let _call = self.probe.enter();
        match event {
            WebSocketEvent::Open(open) => {
                if open
                    .path_segments
                    .first()
                    .is_some_and(|path| path == "denied")
                {
                    vec![WebSocketAction::Reject(HttpResponse::new(403, "denied"))]
                } else {
                    vec![
                        WebSocketAction::Accept,
                        WebSocketAction::Send(WebSocketFrame::Text("ready".to_owned())),
                    ]
                }
            }
            WebSocketEvent::Text(text) => {
                if let Some(room) = text.strip_prefix("join:") {
                    vec![
                        WebSocketAction::JoinRoom {
                            room: room.to_owned(),
                        },
                        WebSocketAction::Reply(WebSocketFrame::Text(format!("joined:{room}"))),
                    ]
                } else if let Some((room, payload)) = text
                    .strip_prefix("say:")
                    .and_then(|message| message.split_once(':'))
                {
                    vec![
                        WebSocketAction::Broadcast {
                            room: room.to_owned(),
                            frame: WebSocketFrame::Text(format!("room:{payload}")),
                            include_current: false,
                        },
                        WebSocketAction::Reply(WebSocketFrame::Text("sent".to_owned())),
                    ]
                } else if let Some(payload) = text.strip_prefix("reply:") {
                    vec![WebSocketAction::Reply(WebSocketFrame::Text(
                        payload.to_owned(),
                    ))]
                } else if text == "burst" {
                    vec![
                        WebSocketAction::Send(WebSocketFrame::Text("first".to_owned())),
                        WebSocketAction::Send(WebSocketFrame::Text("second".to_owned())),
                    ]
                } else if text == "resync" {
                    vec![WebSocketAction::RequestResync {
                        frame: WebSocketFrame::Text("snapshot".to_owned()),
                    }]
                } else {
                    vec![WebSocketAction::Reply(WebSocketFrame::Text(
                        "error:malformed".to_owned(),
                    ))]
                }
            }
            WebSocketEvent::Binary(bytes) => {
                vec![WebSocketAction::Reply(WebSocketFrame::Binary(bytes))]
            }
            WebSocketEvent::TransportError(error) => {
                self.probe.transport_errors.fetch_add(1, Ordering::SeqCst);
                let close = match error {
                    WebSocketTransportError::MessageTooLarge => {
                        WebSocketClose::new(1009, "message too large")
                    }
                    _ => WebSocketClose::new(1002, "transport error"),
                };
                vec![WebSocketAction::Close(close)]
            }
            WebSocketEvent::Close(_) => Vec::new(),
        }
    }

    async fn on_http_cancelled(&mut self, reason: CancellationReason) {
        let _call = self.probe.enter();
        assert_eq!(reason, CancellationReason::DeadlineExceeded);
        self.probe.cancellations.fetch_add(1, Ordering::SeqCst);
    }

    async fn on_shutdown(&mut self) {
        let _call = self.probe.enter();
        self.probe.shutdowns.fetch_add(1, Ordering::SeqCst);
    }
}

fn loopback() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
}

fn test_config() -> ServerConfig {
    let mut config = ServerConfig::default();
    config
        .request_header_allowlist
        .insert("x-switchboard".to_owned());
    config.limits.max_http_body_bytes = 64;
    config.limits.max_websocket_message_bytes = 64;
    config.limits.websocket_write_queue_messages = 8;
    config.limits.websocket_write_queue_bytes = 512;
    config.timeouts.websocket_ping_interval = None;
    config
}

fn http_url(address: SocketAddr, path: &str) -> String {
    format!("http://{address}{path}")
}

fn websocket_url(address: SocketAddr, path: &str) -> String {
    format!("ws://{address}{path}")
}

async fn connect(address: SocketAddr) -> ClientSocket {
    connect_async(websocket_url(address, "/socket"))
        .await
        .unwrap()
        .0
}

async fn next_text(socket: &mut ClientSocket) -> String {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("timed out waiting for WebSocket frame")
            .expect("WebSocket stream ended")
            .expect("WebSocket read failed");
        match message {
            Message::Text(text) => return text.to_string(),
            Message::Ping(payload) => socket.send(Message::Pong(payload)).await.unwrap(),
            Message::Pong(_) => {}
            other => panic!("expected text frame, got {other:?}"),
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
async fn loopback_http_request_response_and_body_limit() {
    let probe = Probe::default();
    let server = bind(
        loopback(),
        test_config(),
        SwitchboardProgram::new(probe.clone()),
    )
    .await
    .unwrap();
    let address = server.local_addr();
    let client = reqwest::Client::new();

    let response = client
        .post(http_url(address, "/echo/alpha?x=one&x=two"))
        .header("x-switchboard", "present")
        .body("payload")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()["x-switchboard"], "yes");
    assert_eq!(
        response.text().await.unwrap(),
        "POST|echo/alpha|one,two|present|payload|127.0.0.1|http|"
    );

    let oversized = client
        .post(http_url(address, "/echo"))
        .body(vec![b'x'; 65])
        .send()
        .await
        .unwrap();
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(probe.http_calls.load(Ordering::SeqCst), 1);
    assert!(!probe.overlapping_call.load(Ordering::SeqCst));
    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn proxy_cookie_and_origin_metadata_are_bounded_and_trust_aware() {
    let probe = Probe::default();
    let mut config = test_config();
    config.origin_policy = OriginPolicy::exact(["https://fjordpulse.example"]);
    let server = bind(
        loopback(),
        config.clone(),
        SwitchboardProgram::new(probe.clone()),
    )
    .await
    .unwrap();
    let address = server.local_addr();
    let client = reqwest::Client::new();

    let untrusted = client
        .get(http_url(address, "/echo"))
        .header("origin", "https://fjordpulse.example")
        .header("x-forwarded-for", "203.0.113.9")
        .header("x-forwarded-proto", "https")
        .header("cookie", "session=abc; theme=dark")
        .send()
        .await
        .unwrap();
    assert_eq!(untrusted.status(), StatusCode::CREATED);
    assert_eq!(
        untrusted.text().await.unwrap(),
        "GET|echo||||127.0.0.1|http|session=abc,theme=dark"
    );

    let rejected_origin = client
        .get(http_url(address, "/echo"))
        .header("origin", "https://attacker.example")
        .send()
        .await
        .unwrap();
    assert_eq!(rejected_origin.status(), StatusCode::FORBIDDEN);
    assert_eq!(probe.http_calls.load(Ordering::SeqCst), 1);
    server.shutdown().await.unwrap();

    config.trusted_proxy = TrustedProxyPolicy::from_cidrs(["127.0.0.0/8"])
        .unwrap()
        .with_max_forwarded_hops(2);
    let trusted = bind(loopback(), config, SwitchboardProgram::new(probe.clone()))
        .await
        .unwrap();
    let address = trusted.local_addr();
    let response = client
        .get(http_url(address, "/echo"))
        .header("origin", "https://fjordpulse.example")
        .header("x-forwarded-for", "203.0.113.9, 127.0.0.2")
        .header("x-forwarded-proto", "https")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        response.text().await.unwrap(),
        "GET|echo||||203.0.113.9|https|"
    );

    let too_many_hops = client
        .get(http_url(address, "/echo"))
        .header("origin", "https://fjordpulse.example")
        .header("x-forwarded-for", "203.0.113.9, 127.0.0.2, 127.0.0.3")
        .send()
        .await
        .unwrap();
    assert_eq!(too_many_hops.status(), StatusCode::BAD_REQUEST);
    trusted.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_origin_policy_requires_an_exact_browser_origin() {
    let probe = Probe::default();
    let mut config = test_config();
    config.origin_policy = OriginPolicy::exact(["https://fjordpulse.example"]);
    let server = bind(loopback(), config, SwitchboardProgram::new(probe))
        .await
        .unwrap();
    let address = server.local_addr();

    let missing = connect_async(websocket_url(address, "/socket"))
        .await
        .unwrap_err();
    assert!(matches!(
        missing,
        ClientWebSocketError::Http(ref response)
            if response.status().as_u16() == StatusCode::FORBIDDEN.as_u16()
    ));

    let mut bad_request = websocket_url(address, "/socket")
        .into_client_request()
        .unwrap();
    bad_request.headers_mut().insert(
        "origin",
        HeaderValue::from_static("https://attacker.example"),
    );
    let rejected = connect_async(bad_request).await.unwrap_err();
    assert!(matches!(
        rejected,
        ClientWebSocketError::Http(ref response)
            if response.status().as_u16() == StatusCode::FORBIDDEN.as_u16()
    ));

    let mut good_request = websocket_url(address, "/socket")
        .into_client_request()
        .unwrap();
    good_request.headers_mut().insert(
        "origin",
        HeaderValue::from_static("https://fjordpulse.example"),
    );
    let (mut socket, _) = connect_async(good_request).await.unwrap();
    assert_eq!(next_text(&mut socket).await, "ready");
    socket.close(None).await.unwrap();
    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn loopback_websocket_rooms_malformed_oversized_and_slow_client() {
    let probe = Probe::default();
    let mut config = test_config();
    config.limits.websocket_write_queue_messages = 1;
    config.limits.websocket_write_queue_bytes = 64;
    config.slow_client_policy = SlowClientPolicy::Close {
        code: 4001,
        reason: "slow switchboard client".to_owned(),
    };
    let server = bind(loopback(), config, SwitchboardProgram::new(probe.clone()))
        .await
        .unwrap();
    let address = server.local_addr();

    let denied = connect_async(websocket_url(address, "/denied"))
        .await
        .unwrap_err();
    match denied {
        ClientWebSocketError::Http(response) => {
            assert_eq!(response.status().as_u16(), StatusCode::FORBIDDEN.as_u16());
        }
        error => panic!("expected rejected upgrade, got {error}"),
    }

    let mut first = connect(address).await;
    let mut second = connect(address).await;
    assert_eq!(next_text(&mut first).await, "ready");
    assert_eq!(next_text(&mut second).await, "ready");
    send_text(&mut first, "join:blue").await;
    send_text(&mut second, "join:blue").await;
    assert_eq!(next_text(&mut first).await, "joined:blue");
    assert_eq!(next_text(&mut second).await, "joined:blue");

    send_text(&mut first, "reply:private").await;
    assert_eq!(next_text(&mut first).await, "private");
    first
        .send(Message::Binary(vec![1, 2, 3].into()))
        .await
        .unwrap();
    let binary = tokio::time::timeout(Duration::from_secs(2), first.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(binary, Message::Binary(vec![1, 2, 3].into()));
    send_text(&mut first, "resync").await;
    assert_eq!(next_text(&mut first).await, "snapshot");
    send_text(&mut first, "say:blue:hello").await;
    assert_eq!(next_text(&mut first).await, "sent");
    assert_eq!(next_text(&mut second).await, "room:hello");
    send_text(&mut first, "not-a-command").await;
    assert_eq!(next_text(&mut first).await, "error:malformed");

    let mut slow = connect(address).await;
    assert_eq!(next_text(&mut slow).await, "ready");
    send_text(&mut slow, "burst").await;
    let slow_close = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match slow.next().await {
                Some(Ok(Message::Close(Some(close)))) => break close,
                Some(Ok(_)) => {}
                Some(Err(error)) => panic!("slow-client socket failed before close: {error}"),
                None => panic!("slow-client socket ended before policy close"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(u16::from(slow_close.code), 4001);
    assert_eq!(slow_close.reason, "slow switchboard client");

    let mut oversized = connect(address).await;
    assert_eq!(next_text(&mut oversized).await, "ready");
    send_text(&mut oversized, &"x".repeat(65)).await;
    let ended = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match oversized.next().await {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(
        ended.is_ok(),
        "oversized WebSocket message was not rejected"
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while probe.transport_errors.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    assert!(!probe.overlapping_call.load(Ordering::SeqCst));
    drop(first);
    drop(second);
    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn program_timeout_cancels_call_and_owner_remains_usable() {
    let probe = Probe::default();
    let mut config = test_config();
    config.timeouts.program_call = Duration::from_millis(40);
    let server = bind(loopback(), config, SwitchboardProgram::new(probe.clone()))
        .await
        .unwrap();
    let address = server.local_addr();
    let client = reqwest::Client::new();

    let timed_out = client.get(http_url(address, "/slow")).send().await.unwrap();
    assert_eq!(timed_out.status(), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(probe.cancellations.load(Ordering::SeqCst), 1);

    let usable = client.get(http_url(address, "/echo")).send().await.unwrap();
    assert_eq!(usable.status(), StatusCode::CREATED);
    assert!(!probe.overlapping_call.load(Ordering::SeqCst));
    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bounded_owner_admission_rejects_backpressure() {
    let probe = Probe::default();
    let mut config = test_config();
    config.limits.owner_queue_capacity = 1;
    config.timeouts.program_call = Duration::from_secs(2);
    let server = bind(loopback(), config, SwitchboardProgram::new(probe.clone()))
        .await
        .unwrap();
    let address = server.local_addr();
    let client = reqwest::Client::new();

    let started = probe.block_started.notified();
    let blocking_client = client.clone();
    let blocking = tokio::spawn(async move {
        blocking_client
            .get(http_url(address, "/block"))
            .send()
            .await
            .unwrap()
            .status()
    });
    started.await;

    let callers = 16;
    let barrier = Arc::new(Barrier::new(callers + 1));
    let mut requests = Vec::new();
    for _ in 0..callers {
        let client = client.clone();
        let barrier = Arc::clone(&barrier);
        requests.push(tokio::spawn(async move {
            barrier.wait().await;
            client
                .get(http_url(address, "/echo"))
                .send()
                .await
                .unwrap()
                .status()
        }));
    }
    barrier.wait().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    probe.block_gate.add_permits(1);

    assert_eq!(blocking.await.unwrap(), StatusCode::OK);
    let mut overloaded = 0;
    let mut admitted = 0;
    for request in requests {
        match request.await.unwrap() {
            StatusCode::SERVICE_UNAVAILABLE => overloaded += 1,
            StatusCode::CREATED => admitted += 1,
            status => panic!("unexpected admission status {status}"),
        }
    }
    assert!(overloaded > 0);
    assert!(admitted >= 1);
    assert!(probe.http_calls.load(Ordering::SeqCst) < callers + 1);
    assert!(!probe.overlapping_call.load(Ordering::SeqCst));
    server.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn graceful_shutdown_settles_admitted_http_and_closes_websocket() {
    let probe = Probe::default();
    let mut config = test_config();
    config.timeouts.program_call = Duration::from_secs(2);
    config.timeouts.graceful_shutdown = Duration::from_secs(3);
    let server = bind(loopback(), config, SwitchboardProgram::new(probe.clone()))
        .await
        .unwrap();
    let address = server.local_addr();
    let mut socket = connect(address).await;
    assert_eq!(next_text(&mut socket).await, "ready");

    let started = probe.block_started.notified();
    let request = tokio::spawn(async move {
        reqwest::Client::new()
            .get(http_url(address, "/block"))
            .send()
            .await
            .unwrap()
    });
    started.await;
    let shutdown = tokio::spawn(server.shutdown());
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!shutdown.is_finished());
    probe.block_gate.add_permits(1);

    assert_eq!(request.await.unwrap().status(), StatusCode::OK);
    shutdown.await.unwrap().unwrap();
    assert_eq!(probe.shutdowns.load(Ordering::SeqCst), 1);
    assert!(!probe.overlapping_call.load(Ordering::SeqCst));

    let close = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Close(Some(close)))) => break close,
                Some(Ok(_)) => {}
                Some(Err(ClientWebSocketError::ConnectionClosed)) | None => {
                    panic!("server closed socket without a shutdown close frame")
                }
                Some(Err(error)) => panic!("WebSocket shutdown failed: {error}"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(u16::from(close.code), 1001);

    let retry = reqwest::Client::new()
        .get(http_url(address, "/echo"))
        .send()
        .await;
    assert!(retry.is_err());
}
