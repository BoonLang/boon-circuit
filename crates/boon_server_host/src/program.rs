use crate::{HttpRequest, HttpResponse, WebSocketAction, WebSocketEvent};
use async_trait::async_trait;
use std::fmt::{self, Debug, Formatter};
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

    /// Reports cancellation after the in-progress HTTP future has been dropped.
    async fn on_http_cancelled(&mut self, _reason: CancellationReason) {}

    /// Reports cancellation after an in-progress WebSocket callback future has
    /// been dropped, allowing effect hosts to cancel transport work.
    async fn on_websocket_cancelled(&mut self, _reason: CancellationReason) {}

    /// Runs after admission is closed and admitted owner commands have settled.
    async fn on_shutdown(&mut self) {}
}
