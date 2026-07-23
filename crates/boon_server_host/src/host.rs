use crate::config::valid_application_close_code;
use crate::program::CancellationSource;
use crate::{
    CallCancellation, CancellationReason, CookieMetadata, DISTRIBUTED_SESSION_TRANSPORT_PATH,
    DistributedSessionAction, DistributedSessionConnectionId, DistributedSessionEvent,
    DistributedSessionOpen, Header, HttpRequest, HttpResponse, PeerAddress, RequestScheme,
    ServerConfig, ServerProgram, SlowClientPolicy, WebSocketAction, WebSocketClose, WebSocketEvent,
    WebSocketFrame, WebSocketOpen, WebSocketTransportError,
};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ws::{CloseFrame, Message, WebSocket};
use axum::extract::{ConnectInfo, FromRequestParts, State, WebSocketUpgrade};
use axum::http::header::{CONNECTION, CONTENT_LENGTH, COOKIE, ORIGIN, TRANSFER_ENCODING, UPGRADE};
use axum::http::{HeaderName, HeaderValue, Request, Response, StatusCode};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use percent_encoding::percent_decode_str;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::future::pending;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Interval, MissedTickBehavior};

#[derive(Clone)]
struct AppState {
    owner: mpsc::Sender<OwnerCommand>,
    accepting: Arc<AtomicBool>,
    next_connection: Arc<AtomicU64>,
    has_distributed_session_transport: bool,
    config: Arc<ServerConfig>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct ConnectionId(u64);

enum OwnerCommand {
    Http {
        request: HttpRequest,
        cancellation: CallCancellation,
        cancellation_source: CancellationSource,
        reply: oneshot::Sender<HttpOwnerReply>,
    },
    WebSocketOpen {
        connection: ConnectionId,
        event: WebSocketOpen,
        writer: BoundedWriter,
        close: CloseSender,
        cancellation: CallCancellation,
        cancellation_source: CancellationSource,
        reply: oneshot::Sender<WebSocketOpenReply>,
    },
    WebSocketEvent {
        connection: ConnectionId,
        event: WebSocketEvent,
        cancellation: CallCancellation,
        cancellation_source: CancellationSource,
        reply: oneshot::Sender<WebSocketEventReply>,
    },
    DistributedSessionOpen {
        connection: DistributedSessionConnectionId,
        event: DistributedSessionOpen,
        writer: BoundedWriter,
        close: CloseSender,
        cancellation: CallCancellation,
        cancellation_source: CancellationSource,
        reply: oneshot::Sender<DistributedSessionOpenReply>,
    },
    DistributedSessionEvent {
        connection: DistributedSessionConnectionId,
        event: DistributedSessionEvent,
        cancellation: CallCancellation,
        cancellation_source: CancellationSource,
        reply: oneshot::Sender<DistributedSessionEventReply>,
    },
    BeginShutdown {
        reply: oneshot::Sender<()>,
    },
}

enum OwnerWake {
    InternalWork,
    DistributedSessionTimer(Instant),
    Command,
}

enum HttpOwnerReply {
    Response(HttpResponse),
    TimedOut,
}

enum WebSocketOpenReply {
    Accepted,
    Rejected(HttpResponse),
    TimedOut,
    InvalidProgramOutput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DistributedSessionOpenReply {
    Accepted,
    TimedOut,
    ConnectionGone,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WebSocketEventReply {
    Processed,
    TimedOut,
    ConnectionGone,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DistributedSessionEventReply {
    Processed,
    TimedOut,
    ConnectionGone,
}

#[derive(Clone)]
struct CloseSender(watch::Sender<Option<WebSocketClose>>);

impl CloseSender {
    fn close(&self, close: WebSocketClose) {
        if self.0.borrow().is_none() {
            self.0.send_replace(Some(close));
        }
    }
}

struct QueuedFrame {
    frame: WebSocketFrame,
    bytes: usize,
}

#[derive(Clone)]
struct BoundedWriter {
    sender: mpsc::Sender<QueuedFrame>,
    queued_bytes: Arc<AtomicUsize>,
    max_bytes: usize,
}

impl BoundedWriter {
    fn try_send(&self, frame: WebSocketFrame) -> Result<(), ()> {
        let bytes = frame.byte_len();
        let mut current = self.queued_bytes.load(Ordering::Acquire);
        loop {
            let Some(next) = current.checked_add(bytes) else {
                return Err(());
            };
            if next > self.max_bytes {
                return Err(());
            }
            match self.queued_bytes.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }

        if self.sender.try_send(QueuedFrame { frame, bytes }).is_err() {
            self.queued_bytes.fetch_sub(bytes, Ordering::AcqRel);
            return Err(());
        }
        Ok(())
    }
}

struct ConnectionState {
    writer: BoundedWriter,
    close: CloseSender,
    rooms: BTreeSet<String>,
}

struct DistributedSessionConnectionState {
    writer: BoundedWriter,
    close: CloseSender,
    closing: bool,
}

struct Owner<P> {
    program: P,
    config: Arc<ServerConfig>,
    receiver: mpsc::Receiver<OwnerCommand>,
    connections: HashMap<ConnectionId, ConnectionState>,
    rooms: HashMap<String, BTreeSet<ConnectionId>>,
    distributed_connections:
        HashMap<DistributedSessionConnectionId, DistributedSessionConnectionState>,
    has_distributed_session_transport: bool,
    blocked_distributed_timer_deadline: Option<Instant>,
    shutdown_reply: Option<oneshot::Sender<()>>,
}

impl<P: ServerProgram> Owner<P> {
    async fn run(mut self) {
        let mut yield_to_owner_command = false;
        loop {
            let has_pending_internal_work =
                self.shutdown_reply.is_none() && self.program.has_pending_internal_work();
            let timer_deadline = self.next_distributed_timer_deadline();
            let mut internal_actions = None;
            let mut owner_command = None;
            let claimed_command = if yield_to_owner_command {
                match self.receiver.try_recv() {
                    Ok(command) => Some(Some(command)),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => None,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => Some(None),
                }
            } else {
                None
            };
            let wake = if let Some(command) = claimed_command {
                owner_command = Some(command);
                OwnerWake::Command
            } else {
                let internal_work = async { self.program.on_internal_work().await };
                tokio::pin!(internal_work);
                tokio::select! {
                    biased;
                    actions = &mut internal_work, if has_pending_internal_work => {
                        internal_actions = Some(actions);
                        OwnerWake::InternalWork
                    }
                    _ = wait_for_deadline(timer_deadline), if timer_deadline.is_some() => {
                        OwnerWake::DistributedSessionTimer(
                            timer_deadline.expect("guarded distributed timer deadline"),
                        )
                    }
                    command = self.receiver.recv() => {
                        owner_command = Some(command);
                        OwnerWake::Command
                    }
                }
            };
            let command = match wake {
                OwnerWake::InternalWork => {
                    yield_to_owner_command = true;
                    self.apply_distributed_session_actions(
                        None,
                        internal_actions.expect("internal work branch stores actions"),
                    );
                    tokio::task::yield_now().await;
                    continue;
                }
                OwnerWake::DistributedSessionTimer(scheduled) => {
                    yield_to_owner_command = true;
                    self.handle_distributed_session_timer(scheduled).await;
                    continue;
                }
                OwnerWake::Command => {
                    yield_to_owner_command = false;
                    owner_command.expect("owner command branch stores result")
                }
            };
            let Some(command) = command else {
                break;
            };
            match command {
                OwnerCommand::Http {
                    request,
                    cancellation,
                    cancellation_source,
                    reply,
                } => {
                    self.handle_http(request, cancellation, cancellation_source, reply)
                        .await;
                }
                OwnerCommand::WebSocketOpen {
                    connection,
                    event,
                    writer,
                    close,
                    cancellation,
                    cancellation_source,
                    reply,
                } => {
                    self.handle_websocket_open(
                        connection,
                        event,
                        writer,
                        close,
                        cancellation,
                        cancellation_source,
                        reply,
                    )
                    .await;
                }
                OwnerCommand::WebSocketEvent {
                    connection,
                    event,
                    cancellation,
                    cancellation_source,
                    reply,
                } => {
                    self.handle_websocket_event(
                        connection,
                        event,
                        cancellation,
                        cancellation_source,
                        reply,
                    )
                    .await;
                }
                OwnerCommand::DistributedSessionOpen {
                    connection,
                    event,
                    writer,
                    close,
                    cancellation,
                    cancellation_source,
                    reply,
                } => {
                    self.handle_distributed_session_open(
                        connection,
                        event,
                        writer,
                        close,
                        cancellation,
                        cancellation_source,
                        reply,
                    )
                    .await;
                }
                OwnerCommand::DistributedSessionEvent {
                    connection,
                    event,
                    cancellation,
                    cancellation_source,
                    reply,
                } => {
                    self.handle_distributed_session_event(
                        connection,
                        event,
                        cancellation,
                        cancellation_source,
                        reply,
                    )
                    .await;
                }
                OwnerCommand::BeginShutdown { reply } => {
                    self.receiver.close();
                    self.shutdown_reply = Some(reply);
                }
            }
        }

        for connection in self.connections.values() {
            connection
                .close
                .close(WebSocketClose::new(1001, "server shutdown"));
        }
        self.connections.clear();
        self.rooms.clear();
        for connection in self.distributed_connections.values_mut() {
            connection.closing = true;
            connection
                .close
                .close(WebSocketClose::new(1001, "server shutdown"));
        }
        self.distributed_connections.clear();

        let shutdown = self.program.on_shutdown();
        let _ = tokio::time::timeout(self.config.timeouts.program_call, shutdown).await;
        if let Some(reply) = self.shutdown_reply.take() {
            let _ = reply.send(());
        }
    }

    fn next_distributed_timer_deadline(&mut self) -> Option<Instant> {
        if !self.has_distributed_session_transport || self.shutdown_reply.is_some() {
            return None;
        }
        let Some(deadline) = self.program.distributed_session_next_deadline() else {
            self.blocked_distributed_timer_deadline = None;
            return None;
        };
        if self
            .blocked_distributed_timer_deadline
            .is_some_and(|blocked| deadline <= blocked)
        {
            return None;
        }
        self.blocked_distributed_timer_deadline = None;
        Some(deadline)
    }

    async fn handle_http(
        &mut self,
        request: HttpRequest,
        cancellation: CallCancellation,
        cancellation_source: CancellationSource,
        reply: oneshot::Sender<HttpOwnerReply>,
    ) {
        let deadline = request.deadline;
        let call_cancellation = cancellation.clone();
        let outcome = {
            let call = self.program.on_http(request, call_cancellation);
            tokio::pin!(call);
            tokio::select! {
                biased;
                reason = cancellation.cancelled() => HttpCallOutcome::Cancelled(reason),
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                    HttpCallOutcome::TimedOut
                }
                response = &mut call => HttpCallOutcome::Response(response),
            }
        };

        match outcome {
            HttpCallOutcome::Response(response) => {
                let _ = reply.send(HttpOwnerReply::Response(response));
            }
            HttpCallOutcome::TimedOut => {
                cancellation_source.cancel(CancellationReason::DeadlineExceeded);
                self.notify_http_cancelled(CancellationReason::DeadlineExceeded)
                    .await;
                let _ = reply.send(HttpOwnerReply::TimedOut);
            }
            HttpCallOutcome::Cancelled(reason) => {
                self.notify_http_cancelled(reason).await;
            }
        }
    }

    async fn notify_http_cancelled(&mut self, reason: CancellationReason) {
        let callback = self.program.on_http_cancelled(reason);
        let _ = tokio::time::timeout(self.config.timeouts.program_call, callback).await;
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_websocket_open(
        &mut self,
        connection: ConnectionId,
        event: WebSocketOpen,
        writer: BoundedWriter,
        close: CloseSender,
        cancellation: CallCancellation,
        cancellation_source: CancellationSource,
        reply: oneshot::Sender<WebSocketOpenReply>,
    ) {
        if self.connections.len() + self.distributed_connections.len()
            >= self.config.limits.max_connections
        {
            let _ = reply.send(WebSocketOpenReply::Rejected(host_http_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "connection capacity reached",
            )));
            return;
        }

        let deadline = event.deadline;
        let call_event = WebSocketEvent::Open(event);
        let outcome = self
            .call_websocket(call_event, cancellation, deadline)
            .await;
        let actions = match outcome {
            WebSocketCallOutcome::Actions(actions) => actions,
            WebSocketCallOutcome::TimedOut => {
                cancellation_source.cancel(CancellationReason::DeadlineExceeded);
                self.notify_websocket_cancelled(CancellationReason::DeadlineExceeded)
                    .await;
                let _ = reply.send(WebSocketOpenReply::TimedOut);
                return;
            }
            WebSocketCallOutcome::Cancelled(reason) => {
                self.notify_websocket_cancelled(reason).await;
                return;
            }
        };

        let decision = match validate_open_actions(&actions, &self.config) {
            Ok(decision) => decision,
            Err(()) => {
                let _ = reply.send(WebSocketOpenReply::InvalidProgramOutput);
                return;
            }
        };

        match decision {
            OpenDecision::Reject(response) => {
                let _ = reply.send(WebSocketOpenReply::Rejected(response));
            }
            OpenDecision::Accept => {
                self.connections.insert(
                    connection,
                    ConnectionState {
                        writer,
                        close,
                        rooms: BTreeSet::new(),
                    },
                );
                if reply.send(WebSocketOpenReply::Accepted).is_err() {
                    self.remove_connection(connection);
                    return;
                }
                self.apply_actions(connection, actions, true);
            }
        }
    }

    async fn handle_websocket_event(
        &mut self,
        connection: ConnectionId,
        event: WebSocketEvent,
        cancellation: CallCancellation,
        cancellation_source: CancellationSource,
        reply: oneshot::Sender<WebSocketEventReply>,
    ) {
        if !self.connections.contains_key(&connection) {
            let _ = reply.send(WebSocketEventReply::ConnectionGone);
            return;
        }

        let is_close = matches!(event, WebSocketEvent::Close(_));
        let reply_allowed = matches!(event, WebSocketEvent::Text(_) | WebSocketEvent::Binary(_));
        let deadline = Instant::now() + self.config.timeouts.program_call;
        match self.call_websocket(event, cancellation, deadline).await {
            WebSocketCallOutcome::Actions(actions) => {
                if validate_event_actions(&actions, &self.config, reply_allowed).is_err() {
                    self.close_connection(
                        connection,
                        WebSocketClose::new(1011, "invalid program WebSocket action"),
                    );
                } else {
                    self.apply_actions(connection, actions, false);
                }
                if is_close {
                    self.remove_connection(connection);
                }
                let _ = reply.send(WebSocketEventReply::Processed);
            }
            WebSocketCallOutcome::TimedOut => {
                cancellation_source.cancel(CancellationReason::DeadlineExceeded);
                self.notify_websocket_cancelled(CancellationReason::DeadlineExceeded)
                    .await;
                self.close_connection(connection, WebSocketClose::new(1011, "program timeout"));
                let _ = reply.send(WebSocketEventReply::TimedOut);
            }
            WebSocketCallOutcome::Cancelled(reason) => {
                self.notify_websocket_cancelled(reason).await;
            }
        }
    }

    async fn notify_websocket_cancelled(&mut self, reason: CancellationReason) {
        let callback = self.program.on_websocket_cancelled(reason);
        let _ = tokio::time::timeout(self.config.timeouts.program_call, callback).await;
    }

    async fn call_websocket(
        &mut self,
        event: WebSocketEvent,
        cancellation: CallCancellation,
        deadline: Instant,
    ) -> WebSocketCallOutcome {
        let call_cancellation = cancellation.clone();
        let call = self.program.on_websocket(event, call_cancellation);
        tokio::pin!(call);
        tokio::select! {
            biased;
            reason = cancellation.cancelled() => WebSocketCallOutcome::Cancelled(reason),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                WebSocketCallOutcome::TimedOut
            }
            actions = &mut call => WebSocketCallOutcome::Actions(actions),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_distributed_session_open(
        &mut self,
        connection: DistributedSessionConnectionId,
        event: DistributedSessionOpen,
        writer: BoundedWriter,
        close: CloseSender,
        cancellation: CallCancellation,
        cancellation_source: CancellationSource,
        reply: oneshot::Sender<DistributedSessionOpenReply>,
    ) {
        if !self.has_distributed_session_transport
            || self.connections.len() + self.distributed_connections.len()
                >= self.config.limits.max_connections
            || self.distributed_connections.contains_key(&connection)
        {
            let _ = reply.send(DistributedSessionOpenReply::ConnectionGone);
            return;
        }

        let deadline = event.deadline;
        self.distributed_connections.insert(
            connection,
            DistributedSessionConnectionState {
                writer,
                close,
                closing: false,
            },
        );
        let outcome = self
            .call_distributed_session(
                connection,
                DistributedSessionEvent::Open(event),
                cancellation,
                deadline,
            )
            .await;
        match outcome {
            DistributedSessionCallOutcome::Actions(actions) => {
                if reply.send(DistributedSessionOpenReply::Accepted).is_err() {
                    self.distributed_connections.remove(&connection);
                    self.notify_distributed_session_cancelled(
                        Some(connection),
                        CancellationReason::PeerDisconnected,
                    )
                    .await;
                    return;
                }
                self.apply_distributed_session_actions(Some(connection), actions);
            }
            DistributedSessionCallOutcome::TimedOut => {
                cancellation_source.cancel(CancellationReason::DeadlineExceeded);
                self.distributed_connections.remove(&connection);
                self.notify_distributed_session_cancelled(
                    Some(connection),
                    CancellationReason::DeadlineExceeded,
                )
                .await;
                let _ = reply.send(DistributedSessionOpenReply::TimedOut);
            }
            DistributedSessionCallOutcome::Cancelled(reason) => {
                self.distributed_connections.remove(&connection);
                self.notify_distributed_session_cancelled(Some(connection), reason)
                    .await;
            }
        }
    }

    async fn handle_distributed_session_event(
        &mut self,
        connection: DistributedSessionConnectionId,
        event: DistributedSessionEvent,
        cancellation: CallCancellation,
        cancellation_source: CancellationSource,
        reply: oneshot::Sender<DistributedSessionEventReply>,
    ) {
        let is_close = matches!(event, DistributedSessionEvent::Close(_));
        let connection_is_live = self
            .distributed_connections
            .get(&connection)
            .is_some_and(|state| !state.closing);
        if is_close {
            if self.distributed_connections.remove(&connection).is_none() {
                let _ = reply.send(DistributedSessionEventReply::ConnectionGone);
                return;
            }
        } else if !connection_is_live {
            let _ = reply.send(DistributedSessionEventReply::ConnectionGone);
            return;
        }

        let deadline = Instant::now() + self.config.timeouts.program_call;
        match self
            .call_distributed_session(connection, event, cancellation, deadline)
            .await
        {
            DistributedSessionCallOutcome::Actions(actions) => {
                self.apply_distributed_session_actions(Some(connection), actions);
                let _ = reply.send(DistributedSessionEventReply::Processed);
            }
            DistributedSessionCallOutcome::TimedOut => {
                cancellation_source.cancel(CancellationReason::DeadlineExceeded);
                self.notify_distributed_session_cancelled(
                    Some(connection),
                    CancellationReason::DeadlineExceeded,
                )
                .await;
                if !is_close {
                    self.close_distributed_session_connection(
                        connection,
                        WebSocketClose::new(1011, "program timeout"),
                    );
                }
                let _ = reply.send(DistributedSessionEventReply::TimedOut);
            }
            DistributedSessionCallOutcome::Cancelled(reason) => {
                self.notify_distributed_session_cancelled(Some(connection), reason)
                    .await;
                self.distributed_connections.remove(&connection);
            }
        }
    }

    async fn call_distributed_session(
        &mut self,
        connection: DistributedSessionConnectionId,
        event: DistributedSessionEvent,
        cancellation: CallCancellation,
        deadline: Instant,
    ) -> DistributedSessionCallOutcome {
        let call_cancellation = cancellation.clone();
        let call = self
            .program
            .on_distributed_session(connection, event, call_cancellation);
        tokio::pin!(call);
        tokio::select! {
            biased;
            reason = cancellation.cancelled() => {
                DistributedSessionCallOutcome::Cancelled(reason)
            }
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                DistributedSessionCallOutcome::TimedOut
            }
            actions = &mut call => DistributedSessionCallOutcome::Actions(actions),
        }
    }

    async fn handle_distributed_session_timer(&mut self, scheduled: Instant) {
        let now = Instant::now();
        let (cancellation_source, cancellation) = CallCancellation::channel();
        let call = self.program.on_distributed_session_timer(now, cancellation);
        match tokio::time::timeout(self.config.timeouts.program_call, call).await {
            Ok(actions) => {
                self.apply_distributed_session_actions(None, actions);
                if self
                    .program
                    .distributed_session_next_deadline()
                    .is_some_and(|next| next <= scheduled)
                {
                    self.blocked_distributed_timer_deadline = Some(scheduled);
                    self.close_all_distributed_session_connections(WebSocketClose::new(
                        1011,
                        "invalid distributed Session timer lifecycle",
                    ));
                }
            }
            Err(_) => {
                cancellation_source.cancel(CancellationReason::DeadlineExceeded);
                self.notify_distributed_session_cancelled(
                    None,
                    CancellationReason::DeadlineExceeded,
                )
                .await;
                self.blocked_distributed_timer_deadline = Some(scheduled);
                self.close_all_distributed_session_connections(WebSocketClose::new(
                    1011,
                    "distributed Session timer timeout",
                ));
            }
        }
    }

    async fn notify_distributed_session_cancelled(
        &mut self,
        connection: Option<DistributedSessionConnectionId>,
        reason: CancellationReason,
    ) {
        let callback = self
            .program
            .on_distributed_session_cancelled(connection, reason);
        let _ = tokio::time::timeout(self.config.timeouts.program_call, callback).await;
    }

    fn apply_distributed_session_actions(
        &mut self,
        current: Option<DistributedSessionConnectionId>,
        actions: Vec<DistributedSessionAction>,
    ) {
        if self.validate_distributed_session_actions(&actions).is_err() {
            self.fail_invalid_distributed_session_lifecycle(current);
            return;
        }

        for action in actions {
            match action {
                DistributedSessionAction::Send { connection, bytes } => {
                    let writer = self
                        .distributed_connections
                        .get(&connection)
                        .filter(|state| !state.closing)
                        .map(|state| state.writer.clone());
                    let Some(writer) = writer else {
                        continue;
                    };
                    if writer.try_send(WebSocketFrame::Binary(bytes)).is_ok() {
                        self.program
                            .on_distributed_session_send_accepted(connection);
                    } else {
                        self.close_slow_distributed_session_connection(connection);
                    }
                }
                DistributedSessionAction::Close { connection, close } => {
                    self.close_distributed_session_connection(connection, close);
                }
            }
        }
    }

    fn validate_distributed_session_actions(
        &self,
        actions: &[DistributedSessionAction],
    ) -> Result<(), ()> {
        if actions.len() > self.config.limits.max_actions_per_event {
            return Err(());
        }
        let mut live = self
            .distributed_connections
            .iter()
            .filter_map(|(connection, state)| (!state.closing).then_some(*connection))
            .collect::<BTreeSet<_>>();
        for action in actions {
            match action {
                DistributedSessionAction::Send { connection, bytes } => {
                    if !live.contains(connection)
                        || bytes.len() > self.config.limits.max_websocket_message_bytes
                    {
                        return Err(());
                    }
                }
                DistributedSessionAction::Close { connection, close } => {
                    if !live.remove(connection) || validate_close(close, &self.config).is_err() {
                        return Err(());
                    }
                }
            }
        }
        Ok(())
    }

    fn fail_invalid_distributed_session_lifecycle(
        &mut self,
        current: Option<DistributedSessionConnectionId>,
    ) {
        let close = WebSocketClose::new(1011, "invalid distributed Session lifecycle");
        if let Some(current) = current
            && self.distributed_connections.contains_key(&current)
        {
            self.close_distributed_session_connection(current, close);
        } else {
            self.close_all_distributed_session_connections(close);
        }
    }

    fn close_slow_distributed_session_connection(
        &mut self,
        connection: DistributedSessionConnectionId,
    ) {
        let SlowClientPolicy::Close { code, reason } = &self.config.slow_client_policy;
        self.close_distributed_session_connection(
            connection,
            WebSocketClose::new(*code, reason.clone()),
        );
    }

    fn close_distributed_session_connection(
        &mut self,
        connection: DistributedSessionConnectionId,
        close: WebSocketClose,
    ) {
        if let Some(state) = self.distributed_connections.get_mut(&connection) {
            state.closing = true;
            state.close.close(close);
        }
    }

    fn close_all_distributed_session_connections(&mut self, close: WebSocketClose) {
        for state in self.distributed_connections.values_mut() {
            state.closing = true;
            state.close.close(close.clone());
        }
    }

    fn apply_actions(
        &mut self,
        current: ConnectionId,
        actions: Vec<WebSocketAction>,
        opening: bool,
    ) {
        for action in actions {
            match action {
                WebSocketAction::Accept | WebSocketAction::Reject(_) => {}
                WebSocketAction::Reply(frame) if !opening => {
                    self.send_to_connection(current, frame);
                }
                WebSocketAction::Reply(_) => {
                    self.close_connection(
                        current,
                        WebSocketClose::new(1011, "reply action has no current message"),
                    );
                }
                WebSocketAction::Send(frame) | WebSocketAction::RequestResync { frame } => {
                    self.send_to_connection(current, frame);
                }
                WebSocketAction::JoinRoom { room } => self.join_room(current, room),
                WebSocketAction::LeaveRoom { room } => self.leave_room(current, &room),
                WebSocketAction::Broadcast {
                    room,
                    frame,
                    include_current,
                } => self.broadcast(current, &room, frame, include_current),
                WebSocketAction::Close(close) => self.close_connection(current, close),
            }
        }
    }

    fn join_room(&mut self, connection: ConnectionId, room: String) {
        let Some(state) = self.connections.get_mut(&connection) else {
            return;
        };
        if state.rooms.contains(&room) {
            return;
        }
        if state.rooms.len() >= self.config.limits.max_rooms_per_connection {
            self.close_connection(
                connection,
                WebSocketClose::new(1008, "room membership limit reached"),
            );
            return;
        }
        state.rooms.insert(room.clone());
        self.rooms.entry(room).or_default().insert(connection);
    }

    fn leave_room(&mut self, connection: ConnectionId, room: &str) {
        if let Some(state) = self.connections.get_mut(&connection) {
            state.rooms.remove(room);
        }
        let remove_room = if let Some(members) = self.rooms.get_mut(room) {
            members.remove(&connection);
            members.is_empty()
        } else {
            false
        };
        if remove_room {
            self.rooms.remove(room);
        }
    }

    fn broadcast(
        &mut self,
        current: ConnectionId,
        room: &str,
        frame: WebSocketFrame,
        include_current: bool,
    ) {
        let recipients = self.rooms.get(room).cloned().unwrap_or_default();
        for recipient in recipients {
            if include_current || recipient != current {
                self.send_to_connection(recipient, frame.clone());
            }
        }
    }

    fn send_to_connection(&mut self, connection: ConnectionId, frame: WebSocketFrame) {
        let Some(state) = self.connections.get(&connection) else {
            return;
        };
        if state.writer.try_send(frame).is_err() {
            self.close_slow_client(connection);
        }
    }

    fn close_slow_client(&self, connection: ConnectionId) {
        let SlowClientPolicy::Close { code, reason } = &self.config.slow_client_policy;
        self.close_connection(connection, WebSocketClose::new(*code, reason.clone()));
    }

    fn close_connection(&self, connection: ConnectionId, close: WebSocketClose) {
        if let Some(state) = self.connections.get(&connection) {
            state.close.close(close);
        }
    }

    fn remove_connection(&mut self, connection: ConnectionId) {
        let Some(state) = self.connections.remove(&connection) else {
            return;
        };
        for room in state.rooms {
            let remove_room = if let Some(members) = self.rooms.get_mut(&room) {
                members.remove(&connection);
                members.is_empty()
            } else {
                false
            };
            if remove_room {
                self.rooms.remove(&room);
            }
        }
    }
}

enum HttpCallOutcome {
    Response(HttpResponse),
    TimedOut,
    Cancelled(CancellationReason),
}

enum WebSocketCallOutcome {
    Actions(Vec<WebSocketAction>),
    TimedOut,
    Cancelled(CancellationReason),
}

enum DistributedSessionCallOutcome {
    Actions(Vec<DistributedSessionAction>),
    TimedOut,
    Cancelled(CancellationReason),
}

enum OpenDecision {
    Accept,
    Reject(HttpResponse),
}

pub async fn bind<P: ServerProgram>(
    address: SocketAddr,
    config: ServerConfig,
    program: P,
) -> Result<RunningServer, ServerError> {
    config.validate().map_err(ServerError::Config)?;
    let has_distributed_session_transport = program.has_distributed_session_transport();
    let listener = TcpListener::bind(address)
        .await
        .map_err(ServerError::Bind)?;
    let local_addr = listener.local_addr().map_err(ServerError::Bind)?;
    let config = Arc::new(config);
    let accepting = Arc::new(AtomicBool::new(true));
    let (owner_sender, owner_receiver) = mpsc::channel(config.limits.owner_queue_capacity);
    let owner = Owner {
        program,
        config: Arc::clone(&config),
        receiver: owner_receiver,
        connections: HashMap::new(),
        rooms: HashMap::new(),
        distributed_connections: HashMap::new(),
        has_distributed_session_transport,
        blocked_distributed_timer_deadline: None,
        shutdown_reply: None,
    };
    let owner_task = tokio::spawn(owner.run());

    let state = AppState {
        owner: owner_sender.clone(),
        accepting: Arc::clone(&accepting),
        next_connection: Arc::new(AtomicU64::new(1)),
        has_distributed_session_transport,
        config: Arc::clone(&config),
    };
    let app = Router::new().fallback(dispatch).with_state(state);
    let (server_shutdown_sender, server_shutdown_receiver) = oneshot::channel();
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _ = server_shutdown_receiver.await;
    });
    let server_task = tokio::spawn(server.into_future());

    Ok(RunningServer {
        local_addr,
        accepting,
        owner_sender,
        server_shutdown_sender: Some(server_shutdown_sender),
        server_task: Some(server_task),
        owner_task: Some(owner_task),
        shutdown_timeout: config.timeouts.graceful_shutdown,
    })
}

async fn dispatch(State(state): State<AppState>, request: Request<Body>) -> Response<Body> {
    if !state.accepting.load(Ordering::Acquire) {
        return plain_response(StatusCode::SERVICE_UNAVAILABLE, "server shutting down");
    }

    let (mut parts, body) = request.into_parts();
    let websocket_attempt = parts
        .headers
        .get(UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    if parts.uri.path() == DISTRIBUTED_SESSION_TRANSPORT_PATH {
        if !state.has_distributed_session_transport {
            return plain_response(StatusCode::NOT_FOUND, "not found");
        }
        if !websocket_attempt {
            return plain_response(
                StatusCode::UPGRADE_REQUIRED,
                "distributed Session transport requires WebSocket upgrade",
            );
        }
        let upgrade = match WebSocketUpgrade::from_request_parts(&mut parts, &state).await {
            Ok(upgrade) => upgrade,
            Err(rejection) => return rejection.into_response(),
        };
        return dispatch_distributed_session(state, parts, upgrade).await;
    }
    if websocket_attempt {
        let upgrade = match WebSocketUpgrade::from_request_parts(&mut parts, &state).await {
            Ok(upgrade) => upgrade,
            Err(rejection) => return rejection.into_response(),
        };
        return dispatch_websocket(state, parts, upgrade).await;
    }

    let request_head = match parse_request_head(&parts, &state.config, false) {
        Ok(head) => head,
        Err(error) => return error.into_response(),
    };
    let body = match to_bytes(body, state.config.limits.max_http_body_bytes).await {
        Ok(body) => body.to_vec(),
        Err(_) => return plain_response(StatusCode::PAYLOAD_TOO_LARGE, "request body too large"),
    };
    let deadline = Instant::now() + state.config.timeouts.program_call;
    let request = HttpRequest {
        method: parts.method.as_str().to_owned(),
        path_segments: request_head.path_segments,
        query: request_head.query,
        headers: request_head.headers,
        cookies: request_head.cookies,
        body,
        peer: request_head.peer,
        scheme: request_head.scheme,
        deadline,
    };
    let (cancellation_source, cancellation) = CallCancellation::channel();
    let mut disconnect_guard = DisconnectGuard::new(cancellation_source.clone());
    let (reply_sender, reply_receiver) = oneshot::channel();
    let command = OwnerCommand::Http {
        request,
        cancellation,
        cancellation_source,
        reply: reply_sender,
    };
    match state.owner.try_send(command) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            disconnect_guard.disarm();
            return plain_response(StatusCode::SERVICE_UNAVAILABLE, "server overloaded");
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            disconnect_guard.disarm();
            return plain_response(StatusCode::SERVICE_UNAVAILABLE, "server unavailable");
        }
    }

    let response = match reply_receiver.await {
        Ok(HttpOwnerReply::Response(response)) => response_from_program(response, &state.config),
        Ok(HttpOwnerReply::TimedOut) => {
            plain_response(StatusCode::GATEWAY_TIMEOUT, "program timeout")
        }
        Err(_) => plain_response(StatusCode::SERVICE_UNAVAILABLE, "server unavailable"),
    };
    disconnect_guard.disarm();
    response
}

async fn dispatch_websocket(
    state: AppState,
    parts: axum::http::request::Parts,
    upgrade: WebSocketUpgrade,
) -> Response<Body> {
    let request_head = match parse_request_head(&parts, &state.config, true) {
        Ok(head) => head,
        Err(error) => return error.into_response(),
    };
    let deadline = Instant::now() + state.config.timeouts.program_call;
    let event = WebSocketOpen {
        path_segments: request_head.path_segments,
        query: request_head.query,
        headers: request_head.headers,
        cookies: request_head.cookies,
        peer: request_head.peer,
        scheme: request_head.scheme,
        deadline,
    };
    let connection = ConnectionId(state.next_connection.fetch_add(1, Ordering::Relaxed));
    let (write_sender, write_receiver) =
        mpsc::channel(state.config.limits.websocket_write_queue_messages);
    let queued_bytes = Arc::new(AtomicUsize::new(0));
    let writer = BoundedWriter {
        sender: write_sender,
        queued_bytes: Arc::clone(&queued_bytes),
        max_bytes: state.config.limits.websocket_write_queue_bytes,
    };
    let (close_sender, close_receiver) = watch::channel(None);
    let close = CloseSender(close_sender);
    let (cancellation_source, cancellation) = CallCancellation::channel();
    let mut disconnect_guard = DisconnectGuard::new(cancellation_source.clone());
    let (reply_sender, reply_receiver) = oneshot::channel();
    let command = OwnerCommand::WebSocketOpen {
        connection,
        event,
        writer,
        close,
        cancellation,
        cancellation_source,
        reply: reply_sender,
    };
    match state.owner.try_send(command) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            disconnect_guard.disarm();
            return plain_response(StatusCode::SERVICE_UNAVAILABLE, "server overloaded");
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            disconnect_guard.disarm();
            return plain_response(StatusCode::SERVICE_UNAVAILABLE, "server unavailable");
        }
    }

    match reply_receiver.await {
        Ok(WebSocketOpenReply::Accepted) => {
            disconnect_guard.disarm();
            let owner = state.owner.clone();
            let config = Arc::clone(&state.config);
            upgrade
                .max_message_size(state.config.limits.max_websocket_message_bytes)
                .max_frame_size(state.config.limits.max_websocket_message_bytes)
                .on_upgrade(move |socket| {
                    run_websocket(
                        socket,
                        connection,
                        owner,
                        write_receiver,
                        queued_bytes,
                        close_receiver,
                        config,
                    )
                })
        }
        Ok(WebSocketOpenReply::Rejected(response)) => {
            disconnect_guard.disarm();
            response_from_program(response, &state.config)
        }
        Ok(WebSocketOpenReply::TimedOut) => {
            disconnect_guard.disarm();
            plain_response(StatusCode::GATEWAY_TIMEOUT, "program timeout")
        }
        Ok(WebSocketOpenReply::InvalidProgramOutput) => {
            disconnect_guard.disarm();
            plain_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid WebSocket open actions",
            )
        }
        Err(_) => {
            disconnect_guard.disarm();
            plain_response(StatusCode::SERVICE_UNAVAILABLE, "server unavailable")
        }
    }
}

async fn dispatch_distributed_session(
    state: AppState,
    parts: axum::http::request::Parts,
    upgrade: WebSocketUpgrade,
) -> Response<Body> {
    let request_head = match parse_request_head(&parts, &state.config, true) {
        Ok(head) => head,
        Err(error) => return error.into_response(),
    };
    let deadline = Instant::now() + state.config.timeouts.program_call;
    let event = DistributedSessionOpen {
        headers: request_head.headers,
        cookies: request_head.cookies,
        peer: request_head.peer,
        scheme: request_head.scheme,
        deadline,
    };
    let connection = DistributedSessionConnectionId::from_raw(
        state.next_connection.fetch_add(1, Ordering::Relaxed),
    );
    let (write_sender, write_receiver) =
        mpsc::channel(state.config.limits.websocket_write_queue_messages);
    let queued_bytes = Arc::new(AtomicUsize::new(0));
    let writer = BoundedWriter {
        sender: write_sender,
        queued_bytes: Arc::clone(&queued_bytes),
        max_bytes: state.config.limits.websocket_write_queue_bytes,
    };
    let (close_sender, close_receiver) = watch::channel(None);
    let close = CloseSender(close_sender);
    let (cancellation_source, cancellation) = CallCancellation::channel();
    let mut disconnect_guard = DisconnectGuard::new(cancellation_source.clone());
    let (reply_sender, reply_receiver) = oneshot::channel();
    let command = OwnerCommand::DistributedSessionOpen {
        connection,
        event,
        writer,
        close,
        cancellation,
        cancellation_source,
        reply: reply_sender,
    };
    match state.owner.try_send(command) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            disconnect_guard.disarm();
            return plain_response(StatusCode::SERVICE_UNAVAILABLE, "server overloaded");
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            disconnect_guard.disarm();
            return plain_response(StatusCode::SERVICE_UNAVAILABLE, "server unavailable");
        }
    }

    match reply_receiver.await {
        Ok(DistributedSessionOpenReply::Accepted) => {
            disconnect_guard.disarm();
            let owner = state.owner.clone();
            let config = Arc::clone(&state.config);
            upgrade
                .max_message_size(state.config.limits.max_websocket_message_bytes)
                .max_frame_size(state.config.limits.max_websocket_message_bytes)
                .on_upgrade(move |socket| {
                    run_distributed_session_websocket(
                        socket,
                        connection,
                        owner,
                        write_receiver,
                        queued_bytes,
                        close_receiver,
                        config,
                    )
                })
        }
        Ok(DistributedSessionOpenReply::TimedOut) => {
            disconnect_guard.disarm();
            plain_response(StatusCode::GATEWAY_TIMEOUT, "program timeout")
        }
        Ok(DistributedSessionOpenReply::ConnectionGone) => {
            disconnect_guard.disarm();
            plain_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "distributed Session transport unavailable",
            )
        }
        Err(_) => {
            disconnect_guard.disarm();
            plain_response(StatusCode::SERVICE_UNAVAILABLE, "server unavailable")
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_websocket(
    socket: WebSocket,
    connection: ConnectionId,
    owner: mpsc::Sender<OwnerCommand>,
    mut write_receiver: mpsc::Receiver<QueuedFrame>,
    queued_bytes: Arc<AtomicUsize>,
    mut close_receiver: watch::Receiver<Option<WebSocketClose>>,
    config: Arc<ServerConfig>,
) {
    let (mut sink, mut stream) = socket.split();
    let mut ping_interval = config.timeouts.websocket_ping_interval.map(|duration| {
        let mut interval =
            tokio::time::interval_at(tokio::time::Instant::now() + duration, duration);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        interval
    });
    let mut pong_timeout = Box::pin(tokio::time::sleep(Duration::from_secs(365 * 24 * 60 * 60)));
    let mut awaiting_pong = false;
    let mut ping_sequence = 0_u64;
    let mut observed_close = None;

    loop {
        tokio::select! {
            biased;
            changed = close_receiver.changed() => {
                if changed.is_err() {
                    break;
                }
                let requested_close = close_receiver.borrow().clone();
                if let Some(close) = requested_close {
                    let mut flush_failed = false;
                    while let Ok(queued) = write_receiver.try_recv() {
                        queued_bytes.fetch_sub(queued.bytes, Ordering::AcqRel);
                        if sink.send(frame_message(queued.frame)).await.is_err() {
                            flush_failed = true;
                            break;
                        }
                    }
                    if flush_failed {
                        break;
                    }
                    let _ = sink.send(close_message(&close)).await;
                    observed_close = Some(close);
                    break;
                }
            }
            _ = &mut pong_timeout, if awaiting_pong => {
                let close = WebSocketClose::new(1002, "pong timeout");
                let _ = sink.send(close_message(&close)).await;
                observed_close = Some(close);
                break;
            }
            queued = write_receiver.recv() => {
                let Some(queued) = queued else {
                    break;
                };
                queued_bytes.fetch_sub(queued.bytes, Ordering::AcqRel);
                if sink.send(frame_message(queued.frame)).await.is_err() {
                    break;
                }
            }
            _ = next_ping(&mut ping_interval), if !awaiting_pong => {
                ping_sequence = ping_sequence.wrapping_add(1);
                if sink.send(Message::Ping(ping_sequence.to_be_bytes().to_vec().into())).await.is_err() {
                    break;
                }
                awaiting_pong = true;
                pong_timeout.as_mut().reset(
                    tokio::time::Instant::now() + config.timeouts.websocket_pong_timeout
                );
            }
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if !process_websocket_event(
                            &owner,
                            connection,
                            WebSocketEvent::Text(text.to_string()),
                            &mut sink,
                        ).await {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        if !process_websocket_event(
                            &owner,
                            connection,
                            WebSocketEvent::Binary(bytes.to_vec()),
                            &mut sink,
                        ).await {
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if sink.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => awaiting_pong = false,
                    Some(Ok(Message::Close(frame))) => {
                        observed_close = frame.as_ref().map(|frame| {
                            WebSocketClose::new(frame.code, frame.reason.to_string())
                        });
                        let _ = sink.send(Message::Close(frame)).await;
                        break;
                    }
                    Some(Err(error)) => {
                        let transport_error = classify_websocket_error(&error);
                        let close = match transport_error {
                            WebSocketTransportError::MessageTooLarge => {
                                WebSocketClose::new(1009, "message too large")
                            }
                            _ => WebSocketClose::new(1002, "invalid WebSocket message"),
                        };
                        let _ = process_websocket_event(
                            &owner,
                            connection,
                            WebSocketEvent::TransportError(transport_error),
                            &mut sink,
                        ).await;
                        let _ = sink.send(close_message(&close)).await;
                        observed_close = Some(close);
                        break;
                    }
                    None => break,
                }
            }
        }
    }

    notify_websocket_close(&owner, connection, observed_close).await;
}

#[allow(clippy::too_many_arguments)]
async fn run_distributed_session_websocket(
    socket: WebSocket,
    connection: DistributedSessionConnectionId,
    owner: mpsc::Sender<OwnerCommand>,
    mut write_receiver: mpsc::Receiver<QueuedFrame>,
    queued_bytes: Arc<AtomicUsize>,
    mut close_receiver: watch::Receiver<Option<WebSocketClose>>,
    config: Arc<ServerConfig>,
) {
    let (mut sink, mut stream) = socket.split();
    let mut ping_interval = config.timeouts.websocket_ping_interval.map(|duration| {
        let mut interval =
            tokio::time::interval_at(tokio::time::Instant::now() + duration, duration);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        interval
    });
    let mut pong_timeout = Box::pin(tokio::time::sleep(Duration::from_secs(365 * 24 * 60 * 60)));
    let mut awaiting_pong = false;
    let mut ping_sequence = 0_u64;
    let mut observed_close = None;

    loop {
        tokio::select! {
            biased;
            changed = close_receiver.changed() => {
                if changed.is_err() {
                    break;
                }
                let requested_close = close_receiver.borrow().clone();
                if let Some(close) = requested_close {
                    let mut flush_failed = false;
                    while let Ok(queued) = write_receiver.try_recv() {
                        queued_bytes.fetch_sub(queued.bytes, Ordering::AcqRel);
                        let WebSocketFrame::Binary(bytes) = queued.frame else {
                            flush_failed = true;
                            break;
                        };
                        if sink.send(Message::Binary(bytes.into())).await.is_err() {
                            flush_failed = true;
                            break;
                        }
                    }
                    if flush_failed {
                        break;
                    }
                    let _ = sink.send(close_message(&close)).await;
                    observed_close = Some(close);
                    break;
                }
            }
            _ = &mut pong_timeout, if awaiting_pong => {
                let close = WebSocketClose::new(1002, "pong timeout");
                let _ = sink.send(close_message(&close)).await;
                observed_close = Some(close);
                break;
            }
            queued = write_receiver.recv() => {
                let Some(queued) = queued else {
                    break;
                };
                queued_bytes.fetch_sub(queued.bytes, Ordering::AcqRel);
                let WebSocketFrame::Binary(bytes) = queued.frame else {
                    let close = WebSocketClose::new(
                        1011,
                        "invalid distributed Session writer frame",
                    );
                    let _ = sink.send(close_message(&close)).await;
                    observed_close = Some(close);
                    break;
                };
                if sink.send(Message::Binary(bytes.into())).await.is_err() {
                    break;
                }
            }
            _ = next_ping(&mut ping_interval), if !awaiting_pong => {
                ping_sequence = ping_sequence.wrapping_add(1);
                if sink.send(Message::Ping(ping_sequence.to_be_bytes().to_vec().into())).await.is_err() {
                    break;
                }
                awaiting_pong = true;
                pong_timeout.as_mut().reset(
                    tokio::time::Instant::now() + config.timeouts.websocket_pong_timeout
                );
            }
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Binary(bytes))) => {
                        if !process_distributed_session_event(
                            &owner,
                            connection,
                            DistributedSessionEvent::Binary(bytes.to_vec()),
                            &mut sink,
                        ).await {
                            break;
                        }
                    }
                    Some(Ok(Message::Text(_))) => {
                        let close = WebSocketClose::new(1003, "binary frames required");
                        let _ = sink.send(close_message(&close)).await;
                        observed_close = Some(close);
                        break;
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if sink.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => awaiting_pong = false,
                    Some(Ok(Message::Close(frame))) => {
                        observed_close = frame.as_ref().map(|frame| {
                            WebSocketClose::new(frame.code, frame.reason.to_string())
                        });
                        let _ = sink.send(Message::Close(frame)).await;
                        break;
                    }
                    Some(Err(error)) => {
                        let transport_error = classify_websocket_error(&error);
                        let close = match transport_error {
                            WebSocketTransportError::MessageTooLarge => {
                                WebSocketClose::new(1009, "message too large")
                            }
                            _ => WebSocketClose::new(1002, "invalid WebSocket message"),
                        };
                        let _ = sink.send(close_message(&close)).await;
                        observed_close = Some(close);
                        break;
                    }
                    None => break,
                }
            }
        }
    }

    notify_distributed_session_close(&owner, connection, observed_close).await;
}

async fn process_distributed_session_event<S>(
    owner: &mpsc::Sender<OwnerCommand>,
    connection: DistributedSessionConnectionId,
    event: DistributedSessionEvent,
    sink: &mut S,
) -> bool
where
    S: futures::Sink<Message> + Unpin,
{
    let (cancellation_source, cancellation) = CallCancellation::channel();
    let mut disconnect_guard = DisconnectGuard::new(cancellation_source.clone());
    let (reply_sender, reply_receiver) = oneshot::channel();
    let command = OwnerCommand::DistributedSessionEvent {
        connection,
        event,
        cancellation,
        cancellation_source,
        reply: reply_sender,
    };
    match owner.try_send(command) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            disconnect_guard.disarm();
            let _ = sink
                .send(close_message(&WebSocketClose::new(
                    1013,
                    "server overloaded",
                )))
                .await;
            return false;
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            disconnect_guard.disarm();
            let _ = sink
                .send(close_message(&WebSocketClose::new(1001, "server shutdown")))
                .await;
            return false;
        }
    }

    let keep_open = match reply_receiver.await {
        Ok(DistributedSessionEventReply::Processed) => true,
        Ok(DistributedSessionEventReply::TimedOut) => {
            let _ = sink
                .send(close_message(&WebSocketClose::new(1011, "program timeout")))
                .await;
            false
        }
        Ok(DistributedSessionEventReply::ConnectionGone) | Err(_) => false,
    };
    disconnect_guard.disarm();
    keep_open
}

async fn notify_distributed_session_close(
    owner: &mpsc::Sender<OwnerCommand>,
    connection: DistributedSessionConnectionId,
    close: Option<WebSocketClose>,
) {
    let (cancellation_source, cancellation) = CallCancellation::channel();
    let (reply_sender, reply_receiver) = oneshot::channel();
    let command = OwnerCommand::DistributedSessionEvent {
        connection,
        event: DistributedSessionEvent::Close(close),
        cancellation,
        cancellation_source,
        reply: reply_sender,
    };
    if owner.send(command).await.is_ok() {
        let _ = reply_receiver.await;
    }
}

async fn process_websocket_event<S>(
    owner: &mpsc::Sender<OwnerCommand>,
    connection: ConnectionId,
    event: WebSocketEvent,
    sink: &mut S,
) -> bool
where
    S: futures::Sink<Message> + Unpin,
{
    let (cancellation_source, cancellation) = CallCancellation::channel();
    let mut disconnect_guard = DisconnectGuard::new(cancellation_source.clone());
    let (reply_sender, reply_receiver) = oneshot::channel();
    let command = OwnerCommand::WebSocketEvent {
        connection,
        event,
        cancellation,
        cancellation_source,
        reply: reply_sender,
    };
    match owner.try_send(command) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            disconnect_guard.disarm();
            let _ = sink
                .send(close_message(&WebSocketClose::new(
                    1013,
                    "server overloaded",
                )))
                .await;
            return false;
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            disconnect_guard.disarm();
            let _ = sink
                .send(close_message(&WebSocketClose::new(1001, "server shutdown")))
                .await;
            return false;
        }
    }

    let keep_open = match reply_receiver.await {
        Ok(WebSocketEventReply::Processed) => true,
        Ok(WebSocketEventReply::TimedOut) => {
            let _ = sink
                .send(close_message(&WebSocketClose::new(1011, "program timeout")))
                .await;
            false
        }
        Ok(WebSocketEventReply::ConnectionGone) | Err(_) => false,
    };
    disconnect_guard.disarm();
    keep_open
}

async fn notify_websocket_close(
    owner: &mpsc::Sender<OwnerCommand>,
    connection: ConnectionId,
    close: Option<WebSocketClose>,
) {
    let (cancellation_source, cancellation) = CallCancellation::channel();
    let (reply_sender, reply_receiver) = oneshot::channel();
    let command = OwnerCommand::WebSocketEvent {
        connection,
        event: WebSocketEvent::Close(close),
        cancellation,
        cancellation_source,
        reply: reply_sender,
    };
    if owner.send(command).await.is_ok() {
        let _ = reply_receiver.await;
    }
}

async fn next_ping(interval: &mut Option<Interval>) {
    match interval {
        Some(interval) => {
            interval.tick().await;
        }
        None => pending::<()>().await,
    }
}

async fn wait_for_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => {
            tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
        }
        None => pending::<()>().await,
    }
}

fn frame_message(frame: WebSocketFrame) -> Message {
    match frame {
        WebSocketFrame::Text(text) => Message::Text(text.into()),
        WebSocketFrame::Binary(bytes) => Message::Binary(bytes.into()),
    }
}

fn close_message(close: &WebSocketClose) -> Message {
    Message::Close(Some(CloseFrame {
        code: close.code,
        reason: close.reason.clone().into(),
    }))
}

fn classify_websocket_error(error: &axum::Error) -> WebSocketTransportError {
    let message = error.to_string().to_ascii_lowercase();
    if message.contains("capacity")
        || message.contains("too large")
        || message.contains("too big")
        || message.contains("too long")
    {
        WebSocketTransportError::MessageTooLarge
    } else if message.contains("protocol") || message.contains("utf-8") || message.contains("utf8")
    {
        WebSocketTransportError::InvalidMessage
    } else {
        WebSocketTransportError::Io
    }
}

struct ParsedRequestHead {
    path_segments: Vec<String>,
    query: BTreeMap<String, Vec<String>>,
    headers: Vec<Header>,
    cookies: Vec<CookieMetadata>,
    peer: PeerAddress,
    scheme: RequestScheme,
}

struct RequestHeadError {
    status: StatusCode,
    message: &'static str,
}

impl RequestHeadError {
    fn new(status: StatusCode, message: &'static str) -> Self {
        Self { status, message }
    }

    fn into_response(self) -> Response<Body> {
        plain_response(self.status, self.message)
    }
}

fn parse_request_head(
    parts: &axum::http::request::Parts,
    config: &ServerConfig,
    websocket: bool,
) -> Result<ParsedRequestHead, RequestHeadError> {
    validate_request_header_envelope(parts, config)?;
    validate_request_origin(parts, config, websocket)?;
    let path_segments = normalize_path(parts.uri.path(), config)?;
    let raw_query = parts.uri.query().unwrap_or_default();
    if raw_query.len() > config.limits.max_query_bytes {
        return Err(RequestHeadError::new(
            StatusCode::URI_TOO_LONG,
            "query string too large",
        ));
    }
    let mut query = BTreeMap::<String, Vec<String>>::new();
    let mut query_pairs = 0_usize;
    for (key, value) in form_urlencoded::parse(raw_query.as_bytes()) {
        query_pairs += 1;
        if query_pairs > config.limits.max_query_pairs {
            return Err(RequestHeadError::new(
                StatusCode::URI_TOO_LONG,
                "too many query parameters",
            ));
        }
        query
            .entry(key.into_owned())
            .or_default()
            .push(value.into_owned());
    }

    let mut headers = Vec::new();
    for (name, value) in &parts.headers {
        if name == COOKIE || forwarding_header(name.as_str()) {
            continue;
        }
        if !config.request_header_allowlist.contains(name.as_str()) {
            continue;
        }
        headers.push(Header::new(name.as_str(), value.as_bytes()));
    }

    let direct_peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map_or(PeerAddress::Unavailable, |ConnectInfo(address)| {
            PeerAddress::Known(*address)
        });
    let (peer, scheme) = resolve_peer_and_scheme(parts, config, direct_peer)?;
    let cookies = parse_cookies(parts, config)?;
    Ok(ParsedRequestHead {
        path_segments,
        query,
        headers,
        cookies,
        peer,
        scheme,
    })
}

fn validate_request_header_envelope(
    parts: &axum::http::request::Parts,
    config: &ServerConfig,
) -> Result<(), RequestHeadError> {
    if parts.headers.len() > config.limits.max_request_headers {
        return Err(RequestHeadError::new(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "too many request headers",
        ));
    }
    let bytes = parts.headers.iter().fold(0_usize, |total, (name, value)| {
        total
            .saturating_add(name.as_str().len())
            .saturating_add(value.as_bytes().len())
    });
    if bytes > config.limits.max_request_header_bytes {
        return Err(RequestHeadError::new(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "request headers too large",
        ));
    }
    Ok(())
}

fn validate_request_origin(
    parts: &axum::http::request::Parts,
    config: &ServerConfig,
    websocket: bool,
) -> Result<(), RequestHeadError> {
    let origins = parts.headers.get_all(ORIGIN).iter().collect::<Vec<_>>();
    if origins.len() > 1 {
        return Err(RequestHeadError::new(
            StatusCode::FORBIDDEN,
            "multiple Origin headers are not allowed",
        ));
    }
    let origin = origins
        .first()
        .map(|value| {
            value.to_str().map_err(|_| {
                RequestHeadError::new(StatusCode::FORBIDDEN, "Origin is not valid ASCII")
            })
        })
        .transpose()?;
    if websocket && config.origin_policy.require_websocket_origin && origin.is_none() {
        return Err(RequestHeadError::new(
            StatusCode::FORBIDDEN,
            "WebSocket Origin is required",
        ));
    }
    let enforce = if websocket {
        config.origin_policy.require_websocket_origin
            || !config.origin_policy.allowed_origins.is_empty()
    } else {
        config.origin_policy.enforce_http_origin
    };
    if enforce
        && let Some(origin) = origin
        && !config.origin_policy.allowed_origins.contains(origin)
    {
        return Err(RequestHeadError::new(
            StatusCode::FORBIDDEN,
            "Origin is not allowed",
        ));
    }
    Ok(())
}

fn forwarding_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("forwarded") || name.to_ascii_lowercase().starts_with("x-forwarded-")
}

fn resolve_peer_and_scheme(
    parts: &axum::http::request::Parts,
    config: &ServerConfig,
    direct_peer: PeerAddress,
) -> Result<(PeerAddress, RequestScheme), RequestHeadError> {
    let PeerAddress::Known(direct_address) = direct_peer else {
        return Ok((PeerAddress::Unavailable, RequestScheme::Http));
    };
    if !config.trusted_proxy.trusts(direct_address.ip()) {
        return Ok((PeerAddress::Known(direct_address), RequestScheme::Http));
    }

    let mut forwarded = Vec::new();
    for value in parts.headers.get_all("x-forwarded-for") {
        let value = value.to_str().map_err(|_| {
            RequestHeadError::new(StatusCode::BAD_REQUEST, "invalid X-Forwarded-For")
        })?;
        for address in value.split(',') {
            if forwarded.len() >= config.trusted_proxy.max_forwarded_hops() {
                return Err(RequestHeadError::new(
                    StatusCode::BAD_REQUEST,
                    "too many forwarded proxy hops",
                ));
            }
            let address = address.trim().parse::<std::net::IpAddr>().map_err(|_| {
                RequestHeadError::new(StatusCode::BAD_REQUEST, "invalid X-Forwarded-For")
            })?;
            forwarded.push(address);
        }
    }
    let peer = forwarded
        .iter()
        .rev()
        .copied()
        .find(|address| !config.trusted_proxy.trusts(*address))
        .or_else(|| forwarded.first().copied())
        .map_or(PeerAddress::Known(direct_address), |address| {
            PeerAddress::Known(SocketAddr::new(address, 0))
        });

    let mut schemes = Vec::new();
    for value in parts.headers.get_all("x-forwarded-proto") {
        let value = value.to_str().map_err(|_| {
            RequestHeadError::new(StatusCode::BAD_REQUEST, "invalid X-Forwarded-Proto")
        })?;
        schemes.extend(value.split(',').map(str::trim));
    }
    if schemes.len() > config.trusted_proxy.max_forwarded_hops() {
        return Err(RequestHeadError::new(
            StatusCode::BAD_REQUEST,
            "too many forwarded protocol hops",
        ));
    }
    let scheme = match schemes.first().copied() {
        None | Some("http") => RequestScheme::Http,
        Some("https") => RequestScheme::Https,
        Some(_) => {
            return Err(RequestHeadError::new(
                StatusCode::BAD_REQUEST,
                "invalid X-Forwarded-Proto",
            ));
        }
    };
    if schemes
        .iter()
        .any(|candidate| !matches!(*candidate, "http" | "https"))
    {
        return Err(RequestHeadError::new(
            StatusCode::BAD_REQUEST,
            "invalid X-Forwarded-Proto",
        ));
    }
    Ok((peer, scheme))
}

fn parse_cookies(
    parts: &axum::http::request::Parts,
    config: &ServerConfig,
) -> Result<Vec<CookieMetadata>, RequestHeadError> {
    let mut cookies = Vec::new();
    let mut bytes = 0usize;
    for header in parts.headers.get_all(COOKIE) {
        bytes = bytes.saturating_add(header.as_bytes().len());
        if bytes > config.limits.max_cookie_bytes {
            return Err(RequestHeadError::new(
                StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
                "cookie metadata too large",
            ));
        }
        let header = header.to_str().map_err(|_| {
            RequestHeadError::new(StatusCode::BAD_REQUEST, "Cookie is not valid ASCII")
        })?;
        for pair in header.split(';') {
            let (name, value) = pair.trim().split_once('=').ok_or_else(|| {
                RequestHeadError::new(StatusCode::BAD_REQUEST, "invalid Cookie metadata")
            })?;
            if !valid_cookie_name(name) || value.chars().any(char::is_control) {
                return Err(RequestHeadError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid Cookie metadata",
                ));
            }
            if cookies.len() >= config.limits.max_cookies {
                return Err(RequestHeadError::new(
                    StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
                    "too many cookies",
                ));
            }
            cookies.push(CookieMetadata {
                name: name.to_owned(),
                value: value.to_owned(),
            });
        }
    }
    Ok(cookies)
}

fn valid_cookie_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn normalize_path(path: &str, config: &ServerConfig) -> Result<Vec<String>, RequestHeadError> {
    let mut normalized = Vec::new();
    for encoded in path.split('/').filter(|segment| !segment.is_empty()) {
        if !valid_percent_encoding(encoded.as_bytes()) {
            return Err(RequestHeadError::new(
                StatusCode::BAD_REQUEST,
                "invalid path encoding",
            ));
        }
        let decoded = percent_decode_str(encoded).decode_utf8().map_err(|_| {
            RequestHeadError::new(StatusCode::BAD_REQUEST, "path is not valid UTF-8")
        })?;
        if decoded.len() > config.limits.max_path_segment_bytes {
            return Err(RequestHeadError::new(
                StatusCode::URI_TOO_LONG,
                "path segment too large",
            ));
        }
        match decoded.as_ref() {
            "." => {}
            ".." => {
                normalized.pop();
            }
            segment if segment.contains(['/', '\\', '\0']) => {
                return Err(RequestHeadError::new(
                    StatusCode::BAD_REQUEST,
                    "encoded path separator is not allowed",
                ));
            }
            segment => normalized.push(segment.to_owned()),
        }
        if normalized.len() > config.limits.max_path_segments {
            return Err(RequestHeadError::new(
                StatusCode::URI_TOO_LONG,
                "too many path segments",
            ));
        }
    }
    Ok(normalized)
}

fn valid_percent_encoding(bytes: &[u8]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

fn validate_open_actions(
    actions: &[WebSocketAction],
    config: &ServerConfig,
) -> Result<OpenDecision, ()> {
    validate_action_limits(actions, config)?;
    let mut decision = None;
    for action in actions {
        match action {
            WebSocketAction::Accept => {
                if decision.replace(OpenDecision::Accept).is_some() {
                    return Err(());
                }
            }
            WebSocketAction::Reject(response) => {
                if decision
                    .replace(OpenDecision::Reject(response.clone()))
                    .is_some()
                {
                    return Err(());
                }
            }
            WebSocketAction::Reply(_) => return Err(()),
            _ => {}
        }
    }
    let decision = decision.ok_or(())?;
    if matches!(decision, OpenDecision::Reject(_)) && actions.len() != 1 {
        return Err(());
    }
    Ok(decision)
}

fn validate_event_actions(
    actions: &[WebSocketAction],
    config: &ServerConfig,
    reply_allowed: bool,
) -> Result<(), ()> {
    validate_action_limits(actions, config)?;
    if actions
        .iter()
        .any(|action| matches!(action, WebSocketAction::Accept | WebSocketAction::Reject(_)))
    {
        return Err(());
    }
    if !reply_allowed
        && actions
            .iter()
            .any(|action| matches!(action, WebSocketAction::Reply(_)))
    {
        return Err(());
    }
    Ok(())
}

fn validate_action_limits(actions: &[WebSocketAction], config: &ServerConfig) -> Result<(), ()> {
    if actions.len() > config.limits.max_actions_per_event {
        return Err(());
    }
    for action in actions {
        match action {
            WebSocketAction::Reject(response) => validate_http_response(response, config)?,
            WebSocketAction::Reply(frame)
            | WebSocketAction::Send(frame)
            | WebSocketAction::RequestResync { frame } => validate_frame(frame, config)?,
            WebSocketAction::JoinRoom { room } | WebSocketAction::LeaveRoom { room } => {
                validate_room(room, config)?;
            }
            WebSocketAction::Broadcast { room, frame, .. } => {
                validate_room(room, config)?;
                validate_frame(frame, config)?;
            }
            WebSocketAction::Close(close) => validate_close(close, config)?,
            WebSocketAction::Accept => {}
        }
    }
    Ok(())
}

fn validate_frame(frame: &WebSocketFrame, config: &ServerConfig) -> Result<(), ()> {
    (frame.byte_len() <= config.limits.max_websocket_message_bytes)
        .then_some(())
        .ok_or(())
}

fn validate_room(room: &str, config: &ServerConfig) -> Result<(), ()> {
    (!room.is_empty() && room.len() <= config.limits.max_room_name_bytes)
        .then_some(())
        .ok_or(())
}

fn validate_close(close: &WebSocketClose, config: &ServerConfig) -> Result<(), ()> {
    (valid_application_close_code(close.code)
        && close.reason.len() <= config.limits.max_close_reason_bytes)
        .then_some(())
        .ok_or(())
}

fn validate_http_response(response: &HttpResponse, config: &ServerConfig) -> Result<(), ()> {
    StatusCode::from_u16(response.status).map_err(|_| ())?;
    if response.body.len() > config.limits.max_http_response_body_bytes
        || response.headers.len() > config.limits.max_response_headers
    {
        return Err(());
    }
    let mut bytes = 0_usize;
    for header in &response.headers {
        let name = HeaderName::from_bytes(header.name.as_bytes()).map_err(|_| ())?;
        HeaderValue::from_bytes(&header.value).map_err(|_| ())?;
        if matches!(
            name,
            CONNECTION | CONTENT_LENGTH | TRANSFER_ENCODING | UPGRADE
        ) {
            return Err(());
        }
        bytes = bytes
            .saturating_add(header.name.len())
            .saturating_add(header.value.len());
        if bytes > config.limits.max_response_header_bytes {
            return Err(());
        }
    }
    Ok(())
}

fn response_from_program(response: HttpResponse, config: &ServerConfig) -> Response<Body> {
    if validate_http_response(&response, config).is_err() {
        return plain_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid program HTTP response",
        );
    }
    let mut builder = Response::builder().status(response.status);
    for header in response.headers {
        let name = HeaderName::from_bytes(header.name.as_bytes()).expect("validated header name");
        let value = HeaderValue::from_bytes(&header.value).expect("validated header value");
        builder = builder.header(name, value);
    }
    builder
        .body(Body::from(response.body))
        .expect("validated HTTP response")
}

fn plain_response(status: StatusCode, body: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .header("cache-control", "no-store")
        .body(Body::from(body))
        .expect("static response is valid")
}

fn host_http_response(status: StatusCode, body: &'static str) -> HttpResponse {
    HttpResponse {
        status: status.as_u16(),
        headers: vec![Header::new(
            "content-type",
            b"text/plain; charset=utf-8".to_vec(),
        )],
        body: body.as_bytes().to_vec(),
    }
}

struct DisconnectGuard {
    source: CancellationSource,
    armed: bool,
}

impl DisconnectGuard {
    fn new(source: CancellationSource) -> Self {
        Self {
            source,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for DisconnectGuard {
    fn drop(&mut self) {
        if self.armed {
            self.source.cancel(CancellationReason::PeerDisconnected);
        }
    }
}

pub struct RunningServer {
    local_addr: SocketAddr,
    accepting: Arc<AtomicBool>,
    owner_sender: mpsc::Sender<OwnerCommand>,
    server_shutdown_sender: Option<oneshot::Sender<()>>,
    server_task: Option<JoinHandle<Result<(), std::io::Error>>>,
    owner_task: Option<JoinHandle<()>>,
    shutdown_timeout: Duration,
}

impl RunningServer {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub async fn shutdown(mut self) -> Result<(), ShutdownError> {
        self.accepting.store(false, Ordering::Release);
        if let Some(sender) = self.server_shutdown_sender.take() {
            let _ = sender.send(());
        }
        let deadline = tokio::time::Instant::now() + self.shutdown_timeout;
        let (reply_sender, reply_receiver) = oneshot::channel();
        tokio::time::timeout_at(
            deadline,
            self.owner_sender.send(OwnerCommand::BeginShutdown {
                reply: reply_sender,
            }),
        )
        .await
        .map_err(|_| ShutdownError::TimedOut)?
        .map_err(|_| ShutdownError::OwnerStopped)?;
        tokio::time::timeout_at(deadline, reply_receiver)
            .await
            .map_err(|_| ShutdownError::TimedOut)?
            .map_err(|_| ShutdownError::OwnerStopped)?;

        if let Some(mut task) = self.server_task.take() {
            let result = tokio::time::timeout_at(deadline, &mut task)
                .await
                .map_err(|_| {
                    task.abort();
                    ShutdownError::TimedOut
                })?
                .map_err(ShutdownError::Join)?;
            result.map_err(ShutdownError::Server)?;
        }
        if let Some(mut task) = self.owner_task.take() {
            tokio::time::timeout_at(deadline, &mut task)
                .await
                .map_err(|_| {
                    task.abort();
                    ShutdownError::TimedOut
                })?
                .map_err(ShutdownError::Join)?;
        }
        Ok(())
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.accepting.store(false, Ordering::Release);
        if let Some(sender) = self.server_shutdown_sender.take() {
            let _ = sender.send(());
        }
        if let Some(task) = self.server_task.take() {
            task.abort();
        }
        if let Some(task) = self.owner_task.take() {
            task.abort();
        }
    }
}

#[derive(Debug)]
pub enum ServerError {
    Config(crate::ConfigError),
    Bind(std::io::Error),
}

impl Display for ServerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => write!(formatter, "invalid server config: {error}"),
            Self::Bind(error) => write!(formatter, "failed to bind server: {error}"),
        }
    }
}

impl Error for ServerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Config(error) => Some(error),
            Self::Bind(error) => Some(error),
        }
    }
}

#[derive(Debug)]
pub enum ShutdownError {
    TimedOut,
    OwnerStopped,
    Server(std::io::Error),
    Join(tokio::task::JoinError),
}

impl Display for ShutdownError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimedOut => formatter.write_str("graceful shutdown timed out"),
            Self::OwnerStopped => formatter.write_str("server owner task stopped unexpectedly"),
            Self::Server(error) => write!(formatter, "server stopped with an error: {error}"),
            Self::Join(error) => write!(formatter, "server task failed: {error}"),
        }
    }
}

impl Error for ShutdownError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Server(error) => Some(error),
            Self::Join(error) => Some(error),
            Self::TimedOut | Self::OwnerStopped => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_envelope_limits_include_unexposed_and_forwarding_headers() {
        let request = Request::builder()
            .uri("/")
            .header("x-not-exposed", "ignored")
            .header("x-forwarded-for", "203.0.113.9")
            .body(Body::empty())
            .unwrap();
        let (parts, _) = request.into_parts();
        let mut config = ServerConfig::default();
        config.limits.max_request_headers = 1;

        let error = validate_request_header_envelope(&parts, &config).unwrap_err();
        assert_eq!(error.status, StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE);

        config.limits.max_request_headers = 2;
        config.limits.max_request_header_bytes = 1;
        let error = validate_request_header_envelope(&parts, &config).unwrap_err();
        assert_eq!(error.status, StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE);
    }
}
