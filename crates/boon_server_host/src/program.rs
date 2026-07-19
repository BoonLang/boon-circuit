use crate::{
    DistributedSessionAction, DistributedSessionConnectionId, DistributedSessionEvent, HttpRequest,
    HttpResponse, WebSocketAction, WebSocketEvent,
};
use async_trait::async_trait;
use std::fmt::{self, Debug, Formatter};
use std::time::Instant;
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CancellationReason {
    PeerDisconnected,
    DeadlineExceeded,
    ServerShutdown,
}

#[derive(Clone)]
pub struct CallCancellation {
    receiver: watch::Receiver<Option<CancellationReason>>,
}

impl CallCancellation {
    pub fn reason(&self) -> Option<CancellationReason> {
        *self.receiver.borrow()
    }

    pub async fn cancelled(&self) -> CancellationReason {
        let mut receiver = self.receiver.clone();
        loop {
            if let Some(reason) = *receiver.borrow_and_update() {
                return reason;
            }
            if receiver.changed().await.is_err() {
                return CancellationReason::ServerShutdown;
            }
        }
    }

    pub(crate) fn channel() -> (CancellationSource, Self) {
        let (sender, receiver) = watch::channel(None);
        (CancellationSource(sender), Self { receiver })
    }
}

impl Debug for CallCancellation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CallCancellation")
            .field("reason", &self.reason())
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub(crate) struct CancellationSource(watch::Sender<Option<CancellationReason>>);

impl CancellationSource {
    pub(crate) fn cancel(&self, reason: CancellationReason) {
        self.0.send_if_modified(|current| {
            if current.is_some() {
                false
            } else {
                *current = Some(reason);
                true
            }
        });
    }
}

#[async_trait]
/// Adapter boundary between the native transport owner and a server actor.
///
/// A future Boon adapter can translate typed `host_ports` into these structural
/// values and actions without exposing transport correlation to the actor. The
/// host invokes every method on one owner task, so implementations never receive
/// overlapping calls through this trait.
pub trait ServerProgram: Send + 'static {
    /// Dispatches one admitted HTTP request and returns its correlated response.
    async fn on_http(
        &mut self,
        request: HttpRequest,
        cancellation: CallCancellation,
    ) -> HttpResponse;

    /// Dispatches one event for the current socket and returns scoped actions.
    async fn on_websocket(
        &mut self,
        event: WebSocketEvent,
        cancellation: CallCancellation,
    ) -> Vec<WebSocketAction>;

    /// Enables the host-owned distributed-Session transport lane.
    ///
    /// The host reads this capability once during binding. The reserved path is
    /// never dispatched to [`ServerProgram::on_http`] or
    /// [`ServerProgram::on_websocket`], whether this returns true or false.
    fn has_distributed_session_transport(&self) -> bool {
        false
    }

    /// Dispatches one lifecycle event from the dedicated binary transport.
    async fn on_distributed_session(
        &mut self,
        _connection: DistributedSessionConnectionId,
        _event: DistributedSessionEvent,
        _cancellation: CallCancellation,
    ) -> Vec<DistributedSessionAction> {
        Vec::new()
    }

    /// Returns the next monotonic instant at which the host should poll the
    /// distributed-Session program, or `None` when no timer is armed.
    fn distributed_session_next_deadline(&self) -> Option<Instant> {
        None
    }

    /// Runs when [`ServerProgram::distributed_session_next_deadline`] is due.
    async fn on_distributed_session_timer(
        &mut self,
        _now: Instant,
        _cancellation: CallCancellation,
    ) -> Vec<DistributedSessionAction> {
        Vec::new()
    }

    /// Reports whether the program has asynchronous internal work for the
    /// owner to await.
    ///
    /// Keep this true while [`ServerProgram::on_internal_work`] can make
    /// progress, including while its next item is not ready yet. Return false
    /// when there is no work to prevent the owner from polling needlessly.
    fn has_pending_internal_work(&self) -> bool {
        false
    }

    /// Awaits and processes one internal work item, returning any resulting
    /// distributed-Session transport actions.
    ///
    /// The owner may drop this future whenever a timer or owner command wins
    /// the scheduling race. Implementations must therefore be cancellation
    /// safe and leave the item available for a later call until it is ready to
    /// complete.
    async fn on_internal_work(&mut self) -> Vec<DistributedSessionAction> {
        std::future::pending().await
    }

    /// Acknowledges one `Send` after, and only after, its target connection's
    /// bounded writer accepted the bytes.
    ///
    /// Programs that lease an outbound queue head must commit that lease here,
    /// not while constructing the action. Queue rejection closes the connection
    /// and never calls this method.
    fn on_distributed_session_send_accepted(
        &mut self,
        _connection: DistributedSessionConnectionId,
    ) {
    }

    /// Reports cancellation after an in-progress distributed callback future
    /// has been dropped. `connection` is `None` for timer callbacks.
    async fn on_distributed_session_cancelled(
        &mut self,
        _connection: Option<DistributedSessionConnectionId>,
        _reason: CancellationReason,
    ) {
    }

    /// Reports cancellation after the in-progress HTTP future has been dropped.
    async fn on_http_cancelled(&mut self, _reason: CancellationReason) {}

    /// Reports cancellation after an in-progress WebSocket callback future has
    /// been dropped, allowing effect hosts to cancel transport work.
    async fn on_websocket_cancelled(&mut self, _reason: CancellationReason) {}

    /// Runs after admission is closed and admitted owner commands have settled.
    async fn on_shutdown(&mut self) {}
}
