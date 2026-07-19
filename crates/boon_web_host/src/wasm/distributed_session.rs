use crate::{
    BrowserWebSocketCapabilities, BrowserWebSocketRequest, DistributedSessionIdentity,
    DistributedSessionJournalStore, DistributedSessionSocketAdmission,
    DistributedSessionSocketDisconnect, DistributedSessionSocketError,
    DistributedSessionSocketLimits, DistributedSessionSocketOwner, DistributedSessionSocketPhase,
    DistributedSessionStorageError, SocketFrame, WebHostError,
};
use boon_runtime::{
    DistributedClientRuntime, DistributedClientUpdate, DocumentFrame, SourcePayload,
    TransientEffectCallId, Value,
};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::rc::Rc;

use super::{BrowserWebSocketAdapter, BrowserWebSocketConnector, BrowserWebSocketEvent};

pub struct BrowserDistributedSessionJournalStore {
    storage: web_sys::Storage,
}

impl BrowserDistributedSessionJournalStore {
    pub fn open() -> Result<Self, DistributedSessionStorageError> {
        let window = super::window().map_err(|error| {
            DistributedSessionStorageError::platform("open sessionStorage", error.to_string())
        })?;
        let storage = window
            .session_storage()
            .map_err(|error| storage_error("open sessionStorage", error))?
            .ok_or_else(|| {
                DistributedSessionStorageError::platform(
                    "open sessionStorage",
                    "Window did not provide sessionStorage",
                )
            })?;
        Ok(Self { storage })
    }
}

impl DistributedSessionJournalStore for BrowserDistributedSessionJournalStore {
    fn read(&mut self, key: &str) -> Result<Option<String>, DistributedSessionStorageError> {
        self.storage
            .get_item(key)
            .map_err(|error| storage_error("read sessionStorage", error))
    }

    fn write(&mut self, key: &str, value: &str) -> Result<(), DistributedSessionStorageError> {
        self.storage
            .set_item(key, value)
            .map_err(|error| storage_error("write sessionStorage", error))
    }

    fn remove(&mut self, key: &str) -> Result<(), DistributedSessionStorageError> {
        self.storage
            .remove_item(key)
            .map_err(|error| storage_error("remove sessionStorage", error))
    }
}

fn storage_error(
    operation: &'static str,
    error: wasm_bindgen::JsValue,
) -> DistributedSessionStorageError {
    DistributedSessionStorageError::platform(operation, super::js_message(&error))
}

pub type BrowserDistributedSessionSocketOwner =
    DistributedSessionSocketOwner<BrowserDistributedSessionJournalStore, DistributedClientRuntime>;

#[derive(Debug)]
pub enum BrowserDistributedSessionSocketError {
    Session(DistributedSessionSocketError),
    WebSocket(WebHostError),
}

impl Display for BrowserDistributedSessionSocketError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => Display::fmt(error, formatter),
            Self::WebSocket(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for BrowserDistributedSessionSocketError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Session(error) => Some(error),
            Self::WebSocket(error) => Some(error),
        }
    }
}

impl From<DistributedSessionSocketError> for BrowserDistributedSessionSocketError {
    fn from(error: DistributedSessionSocketError) -> Self {
        Self::Session(error)
    }
}

impl From<WebHostError> for BrowserDistributedSessionSocketError {
    fn from(error: WebHostError) -> Self {
        Self::WebSocket(error)
    }
}

impl From<DistributedSessionStorageError> for BrowserDistributedSessionSocketError {
    fn from(error: DistributedSessionStorageError) -> Self {
        Self::Session(DistributedSessionSocketError::Handshake(
            crate::DistributedSessionHandshakeError::Storage(error),
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrowserDistributedSessionDisconnect {
    Close {
        code: u16,
        reason: String,
        clean: bool,
    },
    Error {
        message: String,
    },
    Terminal {
        phase: DistributedSessionSocketPhase,
    },
}

#[derive(Debug, Default)]
pub struct BrowserDistributedSessionPoll {
    pub runtime_updates: Vec<DistributedClientUpdate>,
    pub disconnect: Option<BrowserDistributedSessionDisconnect>,
}

/// Browser WebSocket owner for one distributed Session.
///
/// Reconnection is explicit so the embedding scheduler owns retry timing. A
/// reconnect always restarts the journal-backed handshake and therefore resumes
/// with the pending/current sessionStorage token selected by the neutral owner.
pub struct BrowserDistributedSessionSocket {
    connector: BrowserWebSocketConnector,
    request: BrowserWebSocketRequest,
    owner: BrowserDistributedSessionSocketOwner,
    socket: Option<BrowserWebSocketAdapter>,
    socket_epoch: Option<u64>,
}

impl BrowserDistributedSessionSocket {
    pub fn new(
        capabilities: BrowserWebSocketCapabilities,
        request: BrowserWebSocketRequest,
        identity: DistributedSessionIdentity,
        runtime: DistributedClientRuntime,
        limits: DistributedSessionSocketLimits,
    ) -> Result<Self, BrowserDistributedSessionSocketError> {
        Self::new_with_event_wake(
            capabilities,
            request,
            identity,
            runtime,
            limits,
            Rc::new(|| {}),
        )
    }

    pub fn new_with_event_wake(
        capabilities: BrowserWebSocketCapabilities,
        request: BrowserWebSocketRequest,
        identity: DistributedSessionIdentity,
        runtime: DistributedClientRuntime,
        limits: DistributedSessionSocketLimits,
        event_wake: Rc<dyn Fn()>,
    ) -> Result<Self, BrowserDistributedSessionSocketError> {
        let storage = BrowserDistributedSessionJournalStore::open()?;
        let owner = DistributedSessionSocketOwner::new(identity, storage, runtime, limits)?;
        let connector =
            BrowserWebSocketConnector::new_with_event_wake(capabilities, 1, event_wake)?;
        Ok(Self {
            connector,
            request,
            owner,
            socket: None,
            socket_epoch: None,
        })
    }

    pub fn connect(
        capabilities: BrowserWebSocketCapabilities,
        request: BrowserWebSocketRequest,
        identity: DistributedSessionIdentity,
        runtime: DistributedClientRuntime,
        limits: DistributedSessionSocketLimits,
    ) -> Result<Self, BrowserDistributedSessionSocketError> {
        Self::connect_with_event_wake(
            capabilities,
            request,
            identity,
            runtime,
            limits,
            Rc::new(|| {}),
        )
    }

    pub fn connect_with_event_wake(
        capabilities: BrowserWebSocketCapabilities,
        request: BrowserWebSocketRequest,
        identity: DistributedSessionIdentity,
        runtime: DistributedClientRuntime,
        limits: DistributedSessionSocketLimits,
        event_wake: Rc<dyn Fn()>,
    ) -> Result<Self, BrowserDistributedSessionSocketError> {
        let mut session = Self::new_with_event_wake(
            capabilities,
            request,
            identity,
            runtime,
            limits,
            event_wake,
        )?;
        session.reconnect()?;
        Ok(session)
    }

    pub fn reconnect(&mut self) -> Result<u64, BrowserDistributedSessionSocketError> {
        let socket_epoch = self.owner.begin_connect()?;
        match self.connector.connect(self.request.clone()) {
            Ok(socket) => {
                self.socket = Some(socket);
                self.socket_epoch = Some(socket_epoch);
                Ok(socket_epoch)
            }
            Err(error) => {
                let _ = self.owner.socket_connect_failed(socket_epoch)?;
                Err(error.into())
            }
        }
    }

    pub fn poll(
        &mut self,
    ) -> Result<BrowserDistributedSessionPoll, BrowserDistributedSessionSocketError> {
        let mut poll = BrowserDistributedSessionPoll::default();
        let events = self
            .socket
            .as_ref()
            .map(BrowserWebSocketAdapter::take_events)
            .unwrap_or_default();
        if self
            .socket
            .as_ref()
            .is_some_and(BrowserWebSocketAdapter::overflowed)
        {
            self.abort_active_socket(4009, "receive queue full", &mut poll)?;
            return Err(WebHostError::platform(
                "receive distributed Session WebSocket events",
                "bounded event queue overflowed",
            )
            .into());
        }

        for event in events {
            let Some(socket_epoch) = self.socket_epoch else {
                break;
            };
            match event {
                BrowserWebSocketEvent::Open { .. } => {
                    if let Err(error) = self.owner.socket_opened(socket_epoch) {
                        self.abort_active_socket(4002, "invalid socket state", &mut poll)?;
                        return Err(error.into());
                    }
                }
                BrowserWebSocketEvent::Message {
                    frame: SocketFrame::Binary { bytes },
                } => {
                    if let Err(error) = self.owner.push_inbound_binary(socket_epoch, bytes) {
                        self.abort_active_socket(4009, "receive queue rejected frame", &mut poll)?;
                        return Err(error.into());
                    }
                    loop {
                        match self.owner.poll_inbound() {
                            Ok(Some(processed)) => {
                                poll.runtime_updates.extend(processed.runtime_updates);
                            }
                            Ok(None) => break,
                            Err(error) => {
                                self.abort_active_socket(4002, "invalid Session frame", &mut poll)?;
                                return Err(error.into());
                            }
                        }
                    }
                }
                BrowserWebSocketEvent::Message {
                    frame: SocketFrame::Text { .. },
                } => {
                    if let Err(error) = self.owner.reject_text_frame(socket_epoch) {
                        self.abort_active_socket(4003, "binary frames required", &mut poll)?;
                        return Err(error.into());
                    }
                }
                BrowserWebSocketEvent::Close {
                    code,
                    reason,
                    clean,
                } => {
                    let disconnected = self.owner.socket_disconnected(socket_epoch)?;
                    append_disconnect_update(&mut poll, disconnected);
                    poll.disconnect = Some(BrowserDistributedSessionDisconnect::Close {
                        code,
                        reason,
                        clean,
                    });
                    self.socket = None;
                    self.socket_epoch = None;
                    break;
                }
                BrowserWebSocketEvent::Error { message } => {
                    let disconnected = self.owner.socket_disconnected(socket_epoch)?;
                    append_disconnect_update(&mut poll, disconnected);
                    poll.disconnect = Some(BrowserDistributedSessionDisconnect::Error { message });
                    self.socket = None;
                    self.socket_epoch = None;
                    break;
                }
            }
            self.flush_outbound()?;
            if matches!(
                self.owner.phase(),
                DistributedSessionSocketPhase::Rejected | DistributedSessionSocketPhase::Revoked
            ) {
                let phase = self.owner.phase();
                if let Some(socket) = self.socket.as_ref() {
                    let _ = socket.close(1000, "Session complete");
                }
                self.socket = None;
                self.socket_epoch = None;
                poll.disconnect = Some(BrowserDistributedSessionDisconnect::Terminal { phase });
                break;
            }
        }
        self.flush_outbound()?;
        Ok(poll)
    }

    pub fn retry_outbound(&mut self) -> Result<(), BrowserDistributedSessionSocketError> {
        self.flush_outbound()
    }

    pub fn revoke(&mut self) -> Result<(), BrowserDistributedSessionSocketError> {
        self.owner.revoke()?;
        self.flush_outbound()
    }

    pub fn dispatch(
        &mut self,
        path: &str,
        payload: SourcePayload,
    ) -> Result<DistributedClientUpdate, BrowserDistributedSessionSocketError> {
        let update = self.owner.dispatch(path, payload)?;
        self.flush_outbound()?;
        Ok(update)
    }

    pub fn complete_transient_effect(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
    ) -> Result<DistributedClientUpdate, BrowserDistributedSessionSocketError> {
        let update = self.owner.complete_transient_effect(call_id, outcome)?;
        self.flush_outbound()?;
        Ok(update)
    }

    pub fn deliver_transient_effect_result(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
    ) -> Result<DistributedClientUpdate, BrowserDistributedSessionSocketError> {
        let update =
            self.owner
                .deliver_transient_effect_result(call_id, result_sequence, outcome)?;
        self.flush_outbound()?;
        Ok(update)
    }

    pub fn cancel_all_transient_effects(
        &mut self,
    ) -> Result<DistributedClientUpdate, BrowserDistributedSessionSocketError> {
        let update = self.owner.cancel_all_transient_effects()?;
        self.flush_outbound()?;
        Ok(update)
    }

    pub fn root_value_current(
        &mut self,
        name: &str,
    ) -> Result<Value, BrowserDistributedSessionSocketError> {
        Ok(self.owner.root_value_current(name)?)
    }

    pub fn pending_transient_effect_count(&self) -> usize {
        self.owner.pending_transient_effect_count()
    }

    pub fn document_frame(&self) -> Option<&DocumentFrame> {
        self.owner.document_frame()
    }

    pub fn close(
        &mut self,
    ) -> Result<Option<DistributedClientUpdate>, BrowserDistributedSessionSocketError> {
        let Some(socket_epoch) = self.socket_epoch else {
            return Ok(None);
        };
        if let Some(socket) = self.socket.as_ref() {
            socket.close(1000, "Session socket closed")?;
        }
        let disconnected = self.owner.close(socket_epoch)?;
        self.socket = None;
        self.socket_epoch = None;
        Ok(disconnected.runtime_update)
    }

    pub fn phase(&self) -> DistributedSessionSocketPhase {
        self.owner.phase()
    }

    pub fn socket_epoch(&self) -> Option<u64> {
        self.socket_epoch
    }

    pub fn owner(&self) -> &BrowserDistributedSessionSocketOwner {
        &self.owner
    }

    fn flush_outbound(&mut self) -> Result<(), BrowserDistributedSessionSocketError> {
        if matches!(
            self.owner.phase(),
            DistributedSessionSocketPhase::Idle
                | DistributedSessionSocketPhase::Connecting
                | DistributedSessionSocketPhase::ReconnectRequired
                | DistributedSessionSocketPhase::Rejected
                | DistributedSessionSocketPhase::Revoked
                | DistributedSessionSocketPhase::Closed
                | DistributedSessionSocketPhase::Failed
        ) {
            return Ok(());
        }
        let (Some(socket_epoch), Some(socket)) = (self.socket_epoch, self.socket.as_mut()) else {
            return Ok(());
        };
        loop {
            let Some((lease_id, bytes)) = self
                .owner
                .lease_outbound(socket_epoch)?
                .map(|lease| (lease.lease_id(), lease.bytes().to_vec()))
            else {
                return Ok(());
            };
            socket.send(&SocketFrame::Binary { bytes })?;
            self.owner.acknowledge_outbound(socket_epoch, lease_id)?;
        }
    }

    fn abort_active_socket(
        &mut self,
        close_code: u16,
        close_reason: &str,
        poll: &mut BrowserDistributedSessionPoll,
    ) -> Result<(), BrowserDistributedSessionSocketError> {
        let Some(socket_epoch) = self.socket_epoch else {
            return Ok(());
        };
        if let Some(socket) = self.socket.as_ref() {
            let _ = socket.close(close_code, close_reason);
        }
        let disconnected = self.owner.abort_socket(socket_epoch)?;
        append_disconnect_update(poll, disconnected);
        self.socket = None;
        self.socket_epoch = None;
        Ok(())
    }
}

fn append_disconnect_update(
    poll: &mut BrowserDistributedSessionPoll,
    disconnected: DistributedSessionSocketDisconnect,
) {
    if disconnected.admission == DistributedSessionSocketAdmission::Accepted
        && let Some(update) = disconnected.runtime_update
    {
        poll.runtime_updates.push(update);
    }
}
