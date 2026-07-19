use async_trait::async_trait;
use boon_server_host::{
    CallCancellation, DISTRIBUTED_SESSION_TRANSPORT_PATH, DistributedSessionAction,
    DistributedSessionConnectionId, DistributedSessionEvent, HttpRequest, HttpResponse,
    ServerConfig, ServerProgram, WebSocketAction, WebSocketEvent, WebSocketFrame, bind,
};
use futures::{SinkExt, StreamExt};
use reqwest::StatusCode;
use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

type ClientSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

fn loopback() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
}

fn test_config() -> ServerConfig {
    let mut config = ServerConfig::default();
    config.limits.max_websocket_message_bytes = 32;
    config.limits.websocket_write_queue_messages = 8;
    config.limits.websocket_write_queue_bytes = 256;
    config.timeouts.websocket_ping_interval = None;
    config
}

fn websocket_url(address: SocketAddr, path: &str) -> String {
    format!("ws://{address}{path}")
}

async fn connect_distributed(address: SocketAddr) -> ClientSocket {
    connect_async(websocket_url(address, DISTRIBUTED_SESSION_TRANSPORT_PATH))
        .await
        .expect("distributed Session upgrade should succeed")
        .0
}

async fn next_binary(socket: &mut ClientSocket) -> Vec<u8> {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("timed out waiting for distributed frame")
            .expect("distributed socket ended")
            .expect("distributed socket read failed");
        match message {
            Message::Binary(bytes) => return bytes.to_vec(),
            Message::Ping(payload) => socket.send(Message::Pong(payload)).await.unwrap(),
            Message::Pong(_) => {}
            other => panic!("expected binary frame, got {other:?}"),
        }
    }
}

async fn next_close(socket: &mut ClientSocket) -> u16 {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("timed out waiting for distributed close")
            .expect("distributed socket ended before close")
            .expect("distributed socket read failed before close");
        match message {
            Message::Close(Some(close)) => return close.code.into(),
            Message::Ping(payload) => socket.send(Message::Pong(payload)).await.unwrap(),
            Message::Pong(_) | Message::Binary(_) => {}
            other => panic!("expected close frame, got {other:?}"),
        }
    }
}

async fn wait_for(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("timed out waiting for host callback");
}

#[derive(Default)]
struct RoutingState {
    connections: Vec<DistributedSessionConnectionId>,
    ordinary_http_calls: usize,
    ordinary_websocket_calls: usize,
    binary_calls: usize,
    accepted_sends: usize,
}

struct RoutingProgram(Arc<Mutex<RoutingState>>);

#[async_trait]
impl ServerProgram for RoutingProgram {
    async fn on_http(
        &mut self,
        _request: HttpRequest,
        _cancellation: CallCancellation,
    ) -> HttpResponse {
        self.0.lock().unwrap().ordinary_http_calls += 1;
        HttpResponse::new(200, "ordinary")
    }

    async fn on_websocket(
        &mut self,
        event: WebSocketEvent,
        _cancellation: CallCancellation,
    ) -> Vec<WebSocketAction> {
        self.0.lock().unwrap().ordinary_websocket_calls += 1;
        if matches!(event, WebSocketEvent::Open(_)) {
            vec![
                WebSocketAction::Accept,
                WebSocketAction::Send(WebSocketFrame::Text("ordinary".to_owned())),
            ]
        } else {
            Vec::new()
        }
    }

    fn has_distributed_session_transport(&self) -> bool {
        true
    }

    async fn on_distributed_session(
        &mut self,
        connection: DistributedSessionConnectionId,
        event: DistributedSessionEvent,
        _cancellation: CallCancellation,
    ) -> Vec<DistributedSessionAction> {
        let mut state = self.0.lock().unwrap();
        match event {
            DistributedSessionEvent::Open(_) => {
                state.connections.push(connection);
                if state.connections.len() == 2 {
                    vec![
                        DistributedSessionAction::send(state.connections[0], [1]),
                        DistributedSessionAction::send(state.connections[1], [2]),
                    ]
                } else {
                    Vec::new()
                }
            }
            DistributedSessionEvent::Binary(bytes) => {
                state.binary_calls += 1;
                assert_eq!(bytes, vec![9]);
                vec![DistributedSessionAction::send(state.connections[1], [7])]
            }
            DistributedSessionEvent::Close(_) => Vec::new(),
        }
    }

    fn on_distributed_session_send_accepted(
        &mut self,
        _connection: DistributedSessionConnectionId,
    ) {
        self.0.lock().unwrap().accepted_sends += 1;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reserved_lane_is_isolated_binary_and_targetable() {
    let state = Arc::new(Mutex::new(RoutingState::default()));
    let server = bind(
        loopback(),
        test_config(),
        RoutingProgram(Arc::clone(&state)),
    )
    .await
    .unwrap();
    let address = server.local_addr();

    let reserved_http = reqwest::get(format!(
        "http://{address}{DISTRIBUTED_SESSION_TRANSPORT_PATH}"
    ))
    .await
    .unwrap();
    assert_eq!(reserved_http.status(), StatusCode::UPGRADE_REQUIRED);

    let mut first = connect_distributed(address).await;
    let mut second = connect_distributed(address).await;
    assert_eq!(next_binary(&mut first).await, vec![1]);
    assert_eq!(next_binary(&mut second).await, vec![2]);

    first.send(Message::Binary(vec![9].into())).await.unwrap();
    assert_eq!(next_binary(&mut second).await, vec![7]);

    let (mut ordinary, _) = connect_async(websocket_url(address, "/ordinary"))
        .await
        .unwrap();
    assert_eq!(
        ordinary.next().await.unwrap().unwrap(),
        Message::Text("ordinary".into())
    );

    {
        let state = state.lock().unwrap();
        assert_eq!(state.ordinary_http_calls, 0);
        assert_eq!(state.ordinary_websocket_calls, 1);
        assert_eq!(state.binary_calls, 1);
        assert_eq!(state.accepted_sends, 3);
    }
    drop(first);
    drop(second);
    drop(ordinary);
    server.shutdown().await.unwrap();
}

#[derive(Default)]
struct LeaseState {
    pending: VecDeque<Vec<u8>>,
    accepted: usize,
    closes: usize,
}

struct LeaseProgram(Arc<Mutex<LeaseState>>);

#[async_trait]
impl ServerProgram for LeaseProgram {
    async fn on_http(
        &mut self,
        _request: HttpRequest,
        _cancellation: CallCancellation,
    ) -> HttpResponse {
        HttpResponse::new(404, Vec::new())
    }

    async fn on_websocket(
        &mut self,
        _event: WebSocketEvent,
        _cancellation: CallCancellation,
    ) -> Vec<WebSocketAction> {
        Vec::new()
    }

    fn has_distributed_session_transport(&self) -> bool {
        true
    }

    async fn on_distributed_session(
        &mut self,
        connection: DistributedSessionConnectionId,
        event: DistributedSessionEvent,
        _cancellation: CallCancellation,
    ) -> Vec<DistributedSessionAction> {
        let mut state = self.0.lock().unwrap();
        match event {
            DistributedSessionEvent::Open(_) => state
                .pending
                .iter()
                .cloned()
                .map(|bytes| DistributedSessionAction::send(connection, bytes))
                .collect(),
            DistributedSessionEvent::Close(_) => {
                state.closes += 1;
                Vec::new()
            }
            DistributedSessionEvent::Binary(_) => Vec::new(),
        }
    }

    fn on_distributed_session_send_accepted(
        &mut self,
        _connection: DistributedSessionConnectionId,
    ) {
        let mut state = self.0.lock().unwrap();
        state.pending.pop_front().expect("accepted lease exists");
        state.accepted += 1;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounded_writer_acknowledges_only_accepted_leases() {
    let state = Arc::new(Mutex::new(LeaseState {
        pending: VecDeque::from([vec![1], vec![2]]),
        ..LeaseState::default()
    }));
    let mut config = test_config();
    config.limits.websocket_write_queue_messages = 1;
    let server = bind(loopback(), config, LeaseProgram(Arc::clone(&state)))
        .await
        .unwrap();
    let mut socket = connect_distributed(server.local_addr()).await;

    assert_eq!(next_close(&mut socket).await, 1008);
    wait_for(|| state.lock().unwrap().closes == 1).await;

    {
        let state = state.lock().unwrap();
        assert_eq!(state.accepted, 1);
        assert_eq!(state.pending, VecDeque::from([vec![2]]));
    }
    server.shutdown().await.unwrap();
}

#[derive(Default)]
struct FailureState {
    stale: Option<DistributedSessionConnectionId>,
    opens: usize,
    binary_calls: usize,
    closes: usize,
    accepted: usize,
}

struct FailureProgram(Arc<Mutex<FailureState>>);

#[async_trait]
impl ServerProgram for FailureProgram {
    async fn on_http(
        &mut self,
        _request: HttpRequest,
        _cancellation: CallCancellation,
    ) -> HttpResponse {
        HttpResponse::new(404, Vec::new())
    }

    async fn on_websocket(
        &mut self,
        _event: WebSocketEvent,
        _cancellation: CallCancellation,
    ) -> Vec<WebSocketAction> {
        panic!("reserved lane reached ordinary WebSocket callback")
    }

    fn has_distributed_session_transport(&self) -> bool {
        true
    }

    async fn on_distributed_session(
        &mut self,
        connection: DistributedSessionConnectionId,
        event: DistributedSessionEvent,
        _cancellation: CallCancellation,
    ) -> Vec<DistributedSessionAction> {
        let mut state = self.0.lock().unwrap();
        match event {
            DistributedSessionEvent::Open(_) => {
                state.opens += 1;
                if state.opens == 1 {
                    state.stale = Some(connection);
                    Vec::new()
                } else if state.opens == 2 {
                    let stale = state.stale.expect("first connection identity");
                    vec![DistributedSessionAction::send(stale, [1])]
                } else {
                    Vec::new()
                }
            }
            DistributedSessionEvent::Binary(_) => {
                state.binary_calls += 1;
                Vec::new()
            }
            DistributedSessionEvent::Close(_) => {
                state.closes += 1;
                Vec::new()
            }
        }
    }

    fn on_distributed_session_send_accepted(
        &mut self,
        _connection: DistributedSessionConnectionId,
    ) {
        self.0.lock().unwrap().accepted += 1;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn text_oversize_and_stale_targets_fail_closed() {
    let state = Arc::new(Mutex::new(FailureState::default()));
    let mut config = test_config();
    config.limits.max_websocket_message_bytes = 8;
    let server = bind(loopback(), config, FailureProgram(Arc::clone(&state)))
        .await
        .unwrap();
    let address = server.local_addr();

    let mut text = connect_distributed(address).await;
    text.send(Message::Text("no".into())).await.unwrap();
    assert_eq!(next_close(&mut text).await, 1003);
    wait_for(|| state.lock().unwrap().closes == 1).await;

    let mut stale = connect_distributed(address).await;
    assert_eq!(next_close(&mut stale).await, 1011);
    wait_for(|| state.lock().unwrap().closes == 2).await;

    let mut oversized = connect_distributed(address).await;
    oversized
        .send(Message::Binary(vec![0; 9].into()))
        .await
        .unwrap();
    assert_eq!(next_close(&mut oversized).await, 1009);
    wait_for(|| state.lock().unwrap().closes == 3).await;

    {
        let state = state.lock().unwrap();
        assert_eq!(state.binary_calls, 0);
        assert_eq!(state.accepted, 0);
    }
    server.shutdown().await.unwrap();
}

#[derive(Default)]
struct TimerState {
    connection: Option<DistributedSessionConnectionId>,
    deadline: Option<Instant>,
    timer_calls: usize,
    accepted: usize,
}

struct TimerProgram(Arc<Mutex<TimerState>>);

#[async_trait]
impl ServerProgram for TimerProgram {
    async fn on_http(
        &mut self,
        _request: HttpRequest,
        _cancellation: CallCancellation,
    ) -> HttpResponse {
        HttpResponse::new(404, Vec::new())
    }

    async fn on_websocket(
        &mut self,
        _event: WebSocketEvent,
        _cancellation: CallCancellation,
    ) -> Vec<WebSocketAction> {
        Vec::new()
    }

    fn has_distributed_session_transport(&self) -> bool {
        true
    }

    async fn on_distributed_session(
        &mut self,
        connection: DistributedSessionConnectionId,
        event: DistributedSessionEvent,
        _cancellation: CallCancellation,
    ) -> Vec<DistributedSessionAction> {
        if matches!(event, DistributedSessionEvent::Open(_)) {
            let mut state = self.0.lock().unwrap();
            state.connection = Some(connection);
            state.deadline = Some(Instant::now() + Duration::from_millis(25));
        }
        Vec::new()
    }

    fn distributed_session_next_deadline(&self) -> Option<Instant> {
        self.0.lock().unwrap().deadline
    }

    async fn on_distributed_session_timer(
        &mut self,
        _now: Instant,
        _cancellation: CallCancellation,
    ) -> Vec<DistributedSessionAction> {
        let mut state = self.0.lock().unwrap();
        state.timer_calls += 1;
        state.deadline = None;
        vec![DistributedSessionAction::send(
            state.connection.expect("timer connection"),
            [42],
        )]
    }

    fn on_distributed_session_send_accepted(
        &mut self,
        _connection: DistributedSessionConnectionId,
    ) {
        self.0.lock().unwrap().accepted += 1;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn program_deadline_drives_host_timer_and_acknowledged_send() {
    let state = Arc::new(Mutex::new(TimerState::default()));
    let server = bind(loopback(), test_config(), TimerProgram(Arc::clone(&state)))
        .await
        .unwrap();
    let mut socket = connect_distributed(server.local_addr()).await;

    assert_eq!(next_binary(&mut socket).await, vec![42]);
    {
        let state = state.lock().unwrap();
        assert_eq!(state.timer_calls, 1);
        assert_eq!(state.accepted, 1);
        assert_eq!(state.deadline, None);
    }
    drop(socket);
    server.shutdown().await.unwrap();
}

#[derive(Default)]
struct InternalWorkState {
    connection: Option<DistributedSessionConnectionId>,
    deadline: Option<Instant>,
    waits_started: usize,
    timer_calls: usize,
    http_calls: usize,
    accepted: usize,
}

struct InternalWorkProgram {
    state: Arc<Mutex<InternalWorkState>>,
    work: mpsc::UnboundedReceiver<Vec<u8>>,
}

#[async_trait]
impl ServerProgram for InternalWorkProgram {
    async fn on_http(
        &mut self,
        _request: HttpRequest,
        _cancellation: CallCancellation,
    ) -> HttpResponse {
        self.state.lock().unwrap().http_calls += 1;
        HttpResponse::new(200, "serviceable")
    }

    async fn on_websocket(
        &mut self,
        _event: WebSocketEvent,
        _cancellation: CallCancellation,
    ) -> Vec<WebSocketAction> {
        Vec::new()
    }

    fn has_distributed_session_transport(&self) -> bool {
        true
    }

    async fn on_distributed_session(
        &mut self,
        connection: DistributedSessionConnectionId,
        event: DistributedSessionEvent,
        _cancellation: CallCancellation,
    ) -> Vec<DistributedSessionAction> {
        let mut state = self.state.lock().unwrap();
        match event {
            DistributedSessionEvent::Open(_) => {
                state.connection = Some(connection);
                state.deadline = Some(Instant::now());
            }
            DistributedSessionEvent::Close(_) => state.connection = None,
            DistributedSessionEvent::Binary(_) => {}
        }
        Vec::new()
    }

    fn distributed_session_next_deadline(&self) -> Option<Instant> {
        self.state.lock().unwrap().deadline
    }

    async fn on_distributed_session_timer(
        &mut self,
        _now: Instant,
        _cancellation: CallCancellation,
    ) -> Vec<DistributedSessionAction> {
        let mut state = self.state.lock().unwrap();
        state.deadline = None;
        state.timer_calls += 1;
        vec![DistributedSessionAction::send(
            state.connection.expect("timer has a live connection"),
            [40],
        )]
    }

    fn has_pending_internal_work(&self) -> bool {
        self.state.lock().unwrap().connection.is_some()
    }

    async fn on_internal_work(&mut self) -> Vec<DistributedSessionAction> {
        self.state.lock().unwrap().waits_started += 1;
        let bytes = self
            .work
            .recv()
            .await
            .expect("internal work sender remains open");
        let connection = self
            .state
            .lock()
            .unwrap()
            .connection
            .expect("internal work has a live connection");
        vec![DistributedSessionAction::send(connection, bytes)]
    }

    fn on_distributed_session_send_accepted(
        &mut self,
        _connection: DistributedSessionConnectionId,
    ) {
        self.state.lock().unwrap().accepted += 1;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn internal_work_wakes_without_socket_input_and_yields_to_owner_commands() {
    let state = Arc::new(Mutex::new(InternalWorkState::default()));
    let (work_sender, work_receiver) = mpsc::unbounded_channel();
    work_sender
        .send(vec![41])
        .expect("internal work receiver is live");
    let server = bind(
        loopback(),
        test_config(),
        InternalWorkProgram {
            state: Arc::clone(&state),
            work: work_receiver,
        },
    )
    .await
    .unwrap();
    let address = server.local_addr();
    let mut socket = connect_distributed(address).await;

    assert_eq!(next_binary(&mut socket).await, vec![41]);
    assert_eq!(next_binary(&mut socket).await, vec![40]);
    wait_for(|| {
        let state = state.lock().unwrap();
        state.waits_started >= 3 && state.timer_calls == 1
    })
    .await;

    let response = reqwest::get(format!("http://{address}/while-internal-work-is-pending"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.text().await.unwrap(), "serviceable");
    wait_for(|| {
        let state = state.lock().unwrap();
        state.http_calls == 1 && state.waits_started >= 4
    })
    .await;

    let waits_started = state.lock().unwrap().waits_started;
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert_eq!(state.lock().unwrap().waits_started, waits_started);

    work_sender
        .send(vec![42])
        .expect("pending internal work receiver is live");
    assert_eq!(next_binary(&mut socket).await, vec![42]);
    wait_for(|| state.lock().unwrap().accepted == 3).await;

    drop(socket);
    server.shutdown().await.unwrap();
}

const READY_INTERNAL_WORK_ITEMS: usize = 1_000_000;

#[derive(Default)]
struct ReadyInternalWorkState {
    remaining: usize,
    completed: usize,
    completed_when_probed: Option<usize>,
}

struct ReadyInternalWorkProgram(Arc<Mutex<ReadyInternalWorkState>>);

#[async_trait]
impl ServerProgram for ReadyInternalWorkProgram {
    async fn on_http(
        &mut self,
        request: HttpRequest,
        _cancellation: CallCancellation,
    ) -> HttpResponse {
        let mut state = self.0.lock().unwrap();
        match request.path_segments.first().map(String::as_str) {
            Some("start") => {
                state.remaining = READY_INTERNAL_WORK_ITEMS;
                HttpResponse::new(200, "started")
            }
            Some("probe") => {
                state.completed_when_probed = Some(state.completed);
                HttpResponse::new(200, "serviceable")
            }
            _ => HttpResponse::new(404, "not found"),
        }
    }

    async fn on_websocket(
        &mut self,
        _event: WebSocketEvent,
        _cancellation: CallCancellation,
    ) -> Vec<WebSocketAction> {
        Vec::new()
    }

    fn has_pending_internal_work(&self) -> bool {
        self.0.lock().unwrap().remaining > 0
    }

    async fn on_internal_work(&mut self) -> Vec<DistributedSessionAction> {
        let mut state = self.0.lock().unwrap();
        state.remaining -= 1;
        state.completed += 1;
        Vec::new()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn continuously_ready_internal_work_cannot_starve_owner_commands() {
    let state = Arc::new(Mutex::new(ReadyInternalWorkState::default()));
    let server = bind(
        loopback(),
        test_config(),
        ReadyInternalWorkProgram(Arc::clone(&state)),
    )
    .await
    .unwrap();
    let address = server.local_addr();
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(format!("http://{address}/start"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let response = tokio::time::timeout(
        Duration::from_secs(1),
        client.get(format!("http://{address}/probe")).send(),
    )
    .await
    .expect("owner command must preempt continuously ready internal work")
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let state = state.lock().unwrap();
    assert!(
        state.completed_when_probed.unwrap() < READY_INTERNAL_WORK_ITEMS,
        "the internal lane drained completely before servicing an already queued owner command"
    );
    drop(state);
    server.shutdown().await.unwrap();
}
