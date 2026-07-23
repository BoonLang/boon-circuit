use boon_plan::ProgramRole;
use boon_runtime::{
    DistributedMessage, DistributedProgramBundle, DistributedQueueLimits, DistributedRuntimeError,
    DistributedServerAuthority, DistributedServerMachine, DistributedServerUpdate,
    DistributedSessionRuntime, DistributedSessionTemplate, DistributedSessionUpdate,
    PreparedDistributedServerTransaction, RuntimeTurn, ServerDelivery, ServerDeliveryTarget,
    SessionConnectionStatus, SessionOrigin, SessionPrincipal, Value,
};
#[cfg(test)]
use boon_runtime::{DistributedMessagePayload, DistributedServerRuntime};
use boon_wire::{
    ResumeToken, ResumeTokenGenerationError, ServerOffer, ServerReady, ServerReject, ServerRevoked,
    SessionControlFrame, SessionControlFrameError, SessionId, SessionIdGenerationError,
    decode_session_control_frame, encode_session_control_frame,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::ops::Deref;
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const DEFAULT_SESSION_RESUME_WINDOW: Duration = Duration::from_secs(60);
pub const DEFAULT_SESSION_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const QUEUE_LANES_PER_SESSION: usize = 4;
const MAX_SESSION_CLEANUP_ROUNDS: usize = 1024;
const RESUME_DIGEST_DOMAIN: &[u8] = b"boon.session.resume-digest.v1\0";

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DistributedSessionConnectionId(u64);

impl fmt::Debug for DistributedSessionConnectionId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("DistributedSessionConnectionId(..)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DistributedSessionRegistryIdentity {
    pub graph_id: [u8; 32],
    pub graph_revision: u64,
    pub schema_hash: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DistributedSessionRegistryConfig {
    pub max_sessions: usize,
    pub max_pending_handshakes: usize,
    pub max_global_queued_bytes: usize,
    pub session_queue_limits: DistributedQueueLimits,
    pub handshake_timeout: Duration,
    pub resume_window: Duration,
}

impl Default for DistributedSessionRegistryConfig {
    fn default() -> Self {
        Self {
            max_sessions: 64,
            max_pending_handshakes: 64,
            max_global_queued_bytes: 256 * 1024 * 1024,
            session_queue_limits: DistributedQueueLimits::default(),
            handshake_timeout: DEFAULT_SESSION_HANDSHAKE_TIMEOUT,
            resume_window: DEFAULT_SESSION_RESUME_WINDOW,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DistributedSessionHandshakeRejectionReason {
    GraphMismatch,
    SchemaMismatch,
    ResumeUnavailable,
    Capacity,
}

pub struct DistributedSessionHandshakeOffer {
    connection_id: DistributedSessionConnectionId,
    server_frame: Vec<u8>,
}

impl DistributedSessionHandshakeOffer {
    pub fn connection_id(&self) -> DistributedSessionConnectionId {
        self.connection_id
    }

    pub fn server_frame(&self) -> &[u8] {
        &self.server_frame
    }

    pub fn into_parts(self) -> (DistributedSessionConnectionId, Vec<u8>) {
        (self.connection_id, self.server_frame)
    }
}

pub struct DistributedSessionHandshakeRejection {
    reason: DistributedSessionHandshakeRejectionReason,
    server_frame: Vec<u8>,
}

impl DistributedSessionHandshakeRejection {
    pub fn reason(&self) -> DistributedSessionHandshakeRejectionReason {
        self.reason
    }

    pub fn server_frame(&self) -> &[u8] {
        &self.server_frame
    }
}

pub enum DistributedSessionHandshakeStart {
    Offer(DistributedSessionHandshakeOffer),
    Reject(DistributedSessionHandshakeRejection),
}

pub struct DistributedSessionRegistryPoll {
    pub serviced_origins: Vec<SessionOrigin>,
    pub serviced_connections: Vec<DistributedSessionConnectionId>,
    pub backpressured_origins: Vec<SessionOrigin>,
    pub poisoned_sessions: Vec<PoisonedDistributedSession>,
    pub session_turns: Vec<(SessionOrigin, RuntimeTurn)>,
    pub server_turns: Vec<(SessionOrigin, RuntimeTurn)>,
    pub expired_sessions: usize,
}

pub struct PoisonedDistributedSession {
    pub connection_id: Option<DistributedSessionConnectionId>,
    pub diagnostic: String,
}

impl DistributedSessionRegistryPoll {
    fn new(expired_sessions: usize) -> Self {
        Self {
            serviced_origins: Vec::new(),
            serviced_connections: Vec::new(),
            backpressured_origins: Vec::new(),
            poisoned_sessions: Vec::new(),
            session_turns: Vec::new(),
            server_turns: Vec::new(),
            expired_sessions,
        }
    }
}

enum LanePoll {
    Progress,
    Backpressured,
    Poisoned(DistributedSessionRegistryError),
}

#[derive(Debug)]
pub enum DistributedSessionRegistryError {
    InvalidConfig(&'static str),
    InvalidControlFrame(SessionControlFrameError),
    UnexpectedControlFrame,
    UnknownConnection,
    SessionNotConnected,
    SessionExpired,
    TimeRegression,
    TimeOverflow,
    IdentityUnavailable,
    TokenGeneration(ResumeTokenGenerationError),
    SessionIdGeneration(SessionIdGenerationError),
    CleanupFailures { count: usize, first: String },
    Runtime(DistributedRuntimeError),
}

impl Display for DistributedSessionRegistryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => formatter.write_str(message),
            Self::InvalidControlFrame(error) => Display::fmt(error, formatter),
            Self::UnexpectedControlFrame => formatter.write_str("unexpected session control frame"),
            Self::UnknownConnection => formatter.write_str("session connection is unknown"),
            Self::SessionNotConnected => formatter.write_str("session is not connected"),
            Self::SessionExpired => formatter.write_str("session resume window expired"),
            Self::TimeRegression => formatter.write_str("session monotonic time moved backwards"),
            Self::TimeOverflow => formatter.write_str("session monotonic deadline overflowed"),
            Self::IdentityUnavailable => {
                formatter.write_str("distributed Session identity is unavailable")
            }
            Self::TokenGeneration(error) => Display::fmt(error, formatter),
            Self::SessionIdGeneration(error) => Display::fmt(error, formatter),
            Self::CleanupFailures { count, first } => {
                write!(
                    formatter,
                    "{count} distributed Session cleanup operation(s) failed; first: {first}"
                )
            }
            Self::Runtime(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for DistributedSessionRegistryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidControlFrame(error) => Some(error),
            Self::TokenGeneration(error) => Some(error),
            Self::SessionIdGeneration(error) => Some(error),
            Self::Runtime(error) => Some(error),
            _ => None,
        }
    }
}

impl From<SessionControlFrameError> for DistributedSessionRegistryError {
    fn from(error: SessionControlFrameError) -> Self {
        Self::InvalidControlFrame(error)
    }
}

impl From<ResumeTokenGenerationError> for DistributedSessionRegistryError {
    fn from(error: ResumeTokenGenerationError) -> Self {
        Self::TokenGeneration(error)
    }
}

impl From<SessionIdGenerationError> for DistributedSessionRegistryError {
    fn from(error: SessionIdGenerationError) -> Self {
        Self::SessionIdGeneration(error)
    }
}

impl From<DistributedRuntimeError> for DistributedSessionRegistryError {
    fn from(error: DistributedRuntimeError) -> Self {
        Self::Runtime(error)
    }
}

#[derive(Clone)]
struct PendingHandshake {
    connection_id: DistributedSessionConnectionId,
    deadline: Duration,
    kind: PendingHandshakeKind,
    next_resume_digest: [u8; 32],
    session_id: SessionId,
    next_transport_generation: u64,
    applied_server_through: u64,
    applied_client_through: u64,
}

#[derive(Clone)]
enum PendingHandshakeKind {
    Fresh { principal: SessionPrincipal },
    Resume { slot_id: u32 },
}

#[derive(Clone)]
enum SessionSlotState {
    Connected {
        connection_id: DistributedSessionConnectionId,
    },
    Stale {
        deadline: Duration,
        cleanup: Option<SessionCleanupDisposition>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionCleanupDisposition {
    Resume,
    Remove,
}

struct SessionSlot {
    origin: SessionOrigin,
    execution_scope: u64,
    principal: SessionPrincipal,
    runtime: SessionRuntimeSlab,
    transport_generation: u64,
    resume_digest: [u8; 32],
    state: SessionSlotState,
    inbound_frame_sizes: VecDeque<usize>,
    pending_server_messages: VecDeque<DistributedMessage>,
    pending_server_bytes: usize,
    next_lane: u8,
}

/// Copy-on-write state for one row of the compiled Session template.
///
/// Registry transactions clone the indexed slot table frequently. Sharing the
/// settled runtime here keeps that clone proportional to slot metadata; only a
/// row actually mutated by a candidate transaction forks its runtime state.
struct SessionRuntimeSlab {
    settled: Arc<DistributedSessionRuntime>,
}

impl SessionRuntimeSlab {
    fn new(runtime: DistributedSessionRuntime) -> Self {
        Self {
            settled: Arc::new(runtime),
        }
    }

    fn get(&self) -> &DistributedSessionRuntime {
        self.settled.as_ref()
    }

    fn get_mut(
        &mut self,
    ) -> Result<&mut DistributedSessionRuntime, DistributedSessionRegistryError> {
        if Arc::get_mut(&mut self.settled).is_none() {
            self.settled = Arc::new(self.settled.fork_settled()?);
        }
        Ok(Arc::get_mut(&mut self.settled)
            .expect("a freshly forked Session runtime slab has one owner"))
    }

    #[cfg(test)]
    fn shares_settled_state_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.settled, &other.settled)
    }
}

impl Clone for SessionRuntimeSlab {
    fn clone(&self) -> Self {
        Self {
            settled: Arc::clone(&self.settled),
        }
    }
}

impl Deref for SessionRuntimeSlab {
    type Target = DistributedSessionRuntime;

    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

pub struct PreparedDistributedSessionDeliveries {
    candidates: BTreeMap<u32, (VecDeque<DistributedMessage>, usize)>,
    prospective_global: usize,
}

impl SessionSlot {
    fn fork_settled(&self) -> Result<Self, DistributedSessionRegistryError> {
        Ok(Self {
            origin: self.origin,
            execution_scope: self.execution_scope,
            principal: self.principal.clone(),
            runtime: self.runtime.clone(),
            transport_generation: self.transport_generation,
            resume_digest: self.resume_digest,
            state: match self.state {
                SessionSlotState::Connected { connection_id } => {
                    SessionSlotState::Connected { connection_id }
                }
                SessionSlotState::Stale { deadline, cleanup } => {
                    SessionSlotState::Stale { deadline, cleanup }
                }
            },
            inbound_frame_sizes: self.inbound_frame_sizes.clone(),
            pending_server_messages: self.pending_server_messages.clone(),
            pending_server_bytes: self.pending_server_bytes,
            next_lane: self.next_lane,
        })
    }

    fn connection_id(&self) -> Option<DistributedSessionConnectionId> {
        match &self.state {
            SessionSlotState::Connected { connection_id } => Some(*connection_id),
            SessionSlotState::Stale { .. } => None,
        }
    }

    fn connected_id(&self) -> Option<DistributedSessionConnectionId> {
        match &self.state {
            SessionSlotState::Connected { connection_id } => Some(*connection_id),
            SessionSlotState::Stale { .. } => None,
        }
    }

    fn queued_registry_bytes(&self) -> Option<usize> {
        self.inbound_frame_sizes
            .iter()
            .copied()
            .try_fold(self.pending_server_bytes, usize::checked_add)
    }

    fn has_runnable_work(&self, now: Duration) -> bool {
        matches!(
            self.state,
            SessionSlotState::Stale {
                cleanup: Some(_),
                ..
            }
        ) || matches!(
            self.state,
            SessionSlotState::Stale {
                deadline,
                cleanup: None,
            } if now >= deadline
        ) || !self.pending_server_messages.is_empty()
            || (self.connected_id().is_some() && !self.inbound_frame_sizes.is_empty())
            || self.runtime.get().pending_server_messages() > 0
    }
}

pub struct DistributedSessionRegistry {
    config: DistributedSessionRegistryConfig,
    identity: DistributedSessionRegistryIdentity,
    session_template: DistributedSessionTemplate,
    slots: BTreeMap<u32, SessionSlot>,
    connections: BTreeMap<DistributedSessionConnectionId, u32>,
    pending_handshakes: BTreeMap<DistributedSessionConnectionId, PendingHandshake>,
    resume_index: BTreeMap<[u8; 32], u32>,
    revoked_connections: VecDeque<DistributedSessionConnectionId>,
    slot_epochs: BTreeMap<u32, u64>,
    next_connection_id: u64,
    next_execution_scope: u64,
    last_now: Duration,
    round_robin_cursor: Option<u32>,
    global_queued_bytes: usize,
    global_reserved_queue_bytes: usize,
    pending_session_turns: VecDeque<(SessionOrigin, RuntimeTurn)>,
    pending_server_turns: VecDeque<(SessionOrigin, RuntimeTurn)>,
    #[cfg(test)]
    inject_server_publication_queue_pressure: Arc<AtomicBool>,
}

impl DistributedSessionRegistry {
    #[cfg(test)]
    pub(crate) fn set_session_queue_limits_for_test(&mut self, limits: DistributedQueueLimits) {
        self.config.session_queue_limits = limits;
    }

    #[cfg(test)]
    fn inject_server_publication_queue_pressure_for_test(&mut self) {
        self.inject_server_publication_queue_pressure
            .store(true, Ordering::SeqCst);
    }

    pub fn start(
        bundle: &DistributedProgramBundle,
        config: DistributedSessionRegistryConfig,
    ) -> Result<Self, DistributedSessionRegistryError> {
        validate_config(config)?;
        let session_artifact = bundle
            .artifact(ProgramRole::Session)
            .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?
            .clone();
        let endpoint = session_artifact
            .plan()
            .distributed_endpoint
            .as_ref()
            .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?;
        let identity = DistributedSessionRegistryIdentity {
            graph_id: endpoint.graph.graph_id.0,
            graph_revision: endpoint.graph.revision,
            schema_hash: endpoint.wire_schema_hash,
        };
        let session_template = DistributedSessionTemplate::from_artifact(&session_artifact)?;
        Ok(Self {
            config,
            identity,
            session_template,
            slots: BTreeMap::new(),
            connections: BTreeMap::new(),
            pending_handshakes: BTreeMap::new(),
            resume_index: BTreeMap::new(),
            revoked_connections: VecDeque::new(),
            slot_epochs: BTreeMap::new(),
            next_connection_id: 1,
            next_execution_scope: 1,
            last_now: Duration::ZERO,
            round_robin_cursor: None,
            global_queued_bytes: 0,
            global_reserved_queue_bytes: 0,
            pending_session_turns: VecDeque::new(),
            pending_server_turns: VecDeque::new(),
            #[cfg(test)]
            inject_server_publication_queue_pressure: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn identity(&self) -> DistributedSessionRegistryIdentity {
        self.identity
    }

    pub(crate) fn fork_settled(&self) -> Result<Self, DistributedSessionRegistryError> {
        let slots = self
            .slots
            .iter()
            .map(|(slot_id, slot)| Ok((*slot_id, slot.fork_settled()?)))
            .collect::<Result<_, DistributedSessionRegistryError>>()?;
        Ok(Self {
            config: self.config,
            identity: self.identity,
            session_template: self.session_template.clone(),
            slots,
            connections: self.connections.clone(),
            pending_handshakes: self.pending_handshakes.clone(),
            resume_index: self.resume_index.clone(),
            revoked_connections: self.revoked_connections.clone(),
            slot_epochs: self.slot_epochs.clone(),
            next_connection_id: self.next_connection_id,
            next_execution_scope: self.next_execution_scope,
            last_now: self.last_now,
            round_robin_cursor: self.round_robin_cursor,
            global_queued_bytes: self.global_queued_bytes,
            global_reserved_queue_bytes: self.global_reserved_queue_bytes,
            pending_session_turns: self.pending_session_turns.clone(),
            pending_server_turns: self.pending_server_turns.clone(),
            #[cfg(test)]
            inject_server_publication_queue_pressure: Arc::clone(
                &self.inject_server_publication_queue_pressure,
            ),
        })
    }

    fn commit_registry_candidate_checkpoint(
        &mut self,
        candidate: Self,
    ) -> Result<(), DistributedSessionRegistryError> {
        *self = candidate;
        Ok(())
    }

    fn commit_registry_candidate_transaction<M: DistributedServerMachine>(
        &mut self,
        server: &mut DistributedServerAuthority<'_, M>,
        mut candidate: Self,
        origin: SessionOrigin,
        prepared: PreparedDistributedServerTransaction<M::EvaluationMachine>,
    ) -> Result<(), DistributedSessionRegistryError> {
        if let Err(error) = candidate.publish_server_deliveries(prepared.deliveries().to_vec()) {
            server.rollback_prepared_transaction(prepared)?;
            return Err(error);
        }
        let update = server.commit_prepared_transaction(prepared)?;
        candidate
            .pending_server_turns
            .extend(update.turns.into_iter().map(|turn| (origin, turn)));
        *self = candidate;
        Ok(())
    }

    pub fn session_count(&self) -> usize {
        self.slots.len()
    }

    pub(crate) fn take_direct_lifecycle_turns(
        &mut self,
    ) -> (
        VecDeque<(SessionOrigin, RuntimeTurn)>,
        VecDeque<(SessionOrigin, RuntimeTurn)>,
    ) {
        (
            std::mem::take(&mut self.pending_session_turns),
            std::mem::take(&mut self.pending_server_turns),
        )
    }

    pub fn global_queued_bytes(&self) -> usize {
        self.global_queued_bytes
    }

    pub fn global_reserved_queue_bytes(&self) -> usize {
        self.global_reserved_queue_bytes
    }

    pub fn has_runnable_work(&self) -> bool {
        self.slots
            .values()
            .any(|slot| slot.has_runnable_work(self.last_now))
    }

    pub fn pending_client_frames(
        &self,
        connection_id: DistributedSessionConnectionId,
    ) -> Result<usize, DistributedSessionRegistryError> {
        let slot_id = self.connected_slot_id(connection_id)?;
        Ok(self
            .slots
            .get(&slot_id)
            .expect("connection index points to a Session slot")
            .runtime
            .pending_client_frames())
    }

    pub fn has_sendable_client_frame(
        &self,
        connection_id: DistributedSessionConnectionId,
    ) -> Result<bool, DistributedSessionRegistryError> {
        let slot_id = self.connected_slot_id(connection_id)?;
        Ok(self
            .slots
            .get(&slot_id)
            .expect("connection index points to a Session slot")
            .runtime
            .has_sendable_client_frame())
    }

    pub fn next_deadline(&self) -> Option<Duration> {
        self.pending_handshakes
            .values()
            .map(|pending| pending.deadline)
            .chain(self.slots.values().filter_map(|slot| match slot.state {
                SessionSlotState::Connected { .. } => None,
                SessionSlotState::Stale { deadline, .. } => Some(deadline),
            }))
            .min()
    }

    pub fn begin_handshake(
        &mut self,
        now: Duration,
        principal: SessionPrincipal,
        client_frame: &[u8],
    ) -> Result<DistributedSessionHandshakeStart, DistributedSessionRegistryError> {
        self.observe_lifecycle(now)?;
        let SessionControlFrame::ClientHello(hello) = decode_session_control_frame(client_frame)?
        else {
            return Err(DistributedSessionRegistryError::UnexpectedControlFrame);
        };
        let (graph_id, graph_revision, schema_hash, resume_token, applied_server_through) =
            hello.into_parts();
        if graph_id != self.identity.graph_id || graph_revision != self.identity.graph_revision {
            return self.rejection(DistributedSessionHandshakeRejectionReason::GraphMismatch);
        }
        if schema_hash != self.identity.schema_hash {
            return self.rejection(DistributedSessionHandshakeRejectionReason::SchemaMismatch);
        }
        match resume_token {
            Some(token) => self.begin_resume(now, principal, token, applied_server_through),
            None if applied_server_through == 0 => self.begin_fresh(now, principal),
            None => self.rejection(DistributedSessionHandshakeRejectionReason::ResumeUnavailable),
        }
    }

    pub fn commit_handshake(
        &mut self,
        server: &mut DistributedServerAuthority<'_, impl DistributedServerMachine>,
        now: Duration,
        connection_id: DistributedSessionConnectionId,
        client_frame: &[u8],
    ) -> Result<Vec<u8>, DistributedSessionRegistryError> {
        self.observe_now(now)?;
        let SessionControlFrame::ClientCommit(commit) = decode_session_control_frame(client_frame)?
        else {
            return Err(DistributedSessionRegistryError::UnexpectedControlFrame);
        };
        let pending = self
            .pending_handshakes
            .remove(&connection_id)
            .ok_or(DistributedSessionRegistryError::UnknownConnection)?;
        if pending.connection_id != connection_id {
            return Err(DistributedSessionRegistryError::UnknownConnection);
        }
        if now >= pending.deadline {
            return Err(DistributedSessionRegistryError::SessionExpired);
        }
        if commit.session_id() != pending.session_id
            || commit.generation() != pending.next_transport_generation
            || commit.applied_server_through() != pending.applied_server_through
        {
            return Err(DistributedSessionRegistryError::UnexpectedControlFrame);
        }
        let ready =
            encode_session_control_frame(&SessionControlFrame::ServerReady(ServerReady::new(
                pending.session_id,
                pending.next_transport_generation,
                pending.applied_client_through,
            )))?;
        let next_resume_digest = pending.next_resume_digest;
        let next_transport_generation = pending.next_transport_generation;
        match pending.kind {
            PendingHandshakeKind::Fresh { principal } => {
                self.commit_fresh(
                    server,
                    connection_id,
                    principal,
                    next_resume_digest,
                    pending.session_id,
                    next_transport_generation,
                )?;
            }
            PendingHandshakeKind::Resume { slot_id } => {
                self.commit_resume(
                    server,
                    connection_id,
                    slot_id,
                    next_resume_digest,
                    next_transport_generation,
                    pending.applied_server_through,
                )?;
            }
        }
        Ok(ready)
    }

    pub fn disconnect(
        &mut self,
        server: &mut DistributedServerAuthority<'_, impl DistributedServerMachine>,
        now: Duration,
        connection_id: DistributedSessionConnectionId,
    ) -> Result<(), DistributedSessionRegistryError> {
        self.observe_now(now)?;
        if self.pending_handshakes.remove(&connection_id).is_some() {
            return Ok(());
        }
        let slot_id = self.connected_slot_id(connection_id)?;
        self.disconnect_connected(server, now, slot_id, connection_id)
    }

    fn disconnect_connected(
        &mut self,
        server: &mut DistributedServerAuthority<'_, impl DistributedServerMachine>,
        now: Duration,
        slot_id: u32,
        connection_id: DistributedSessionConnectionId,
    ) -> Result<(), DistributedSessionRegistryError> {
        let deadline = checked_deadline(now, self.config.resume_window)?;
        debug_assert!(matches!(
            self.connected_slot_id(connection_id),
            Ok(current) if current == slot_id
        ));
        self.begin_stale_cleanup(server, slot_id, deadline, SessionCleanupDisposition::Resume)?;
        self.drive_cleanup(server, slot_id)
    }

    pub fn revoke(
        &mut self,
        server: &mut DistributedServerAuthority<'_, impl DistributedServerMachine>,
        connection_id: DistributedSessionConnectionId,
        client_frame: &[u8],
    ) -> Result<Vec<u8>, DistributedSessionRegistryError> {
        if !matches!(
            decode_session_control_frame(client_frame)?,
            SessionControlFrame::ClientRevoke(_)
        ) {
            return Err(DistributedSessionRegistryError::UnexpectedControlFrame);
        }
        let acknowledgement = encode_session_control_frame(&SessionControlFrame::ServerRevoked(
            ServerRevoked::new(),
        ))?;
        if self.revoked_connections.contains(&connection_id) {
            return Ok(acknowledgement);
        }
        let slot_id = self.connected_slot_id(connection_id)?;
        self.begin_stale_cleanup(
            server,
            slot_id,
            self.last_now,
            SessionCleanupDisposition::Remove,
        )?;
        self.drive_cleanup(server, slot_id)?;
        self.revoked_connections.push_back(connection_id);
        while self.revoked_connections.len() > self.config.max_pending_handshakes {
            self.revoked_connections.pop_front();
        }
        Ok(acknowledgement)
    }

    pub fn admit_client_frame(
        &mut self,
        connection_id: DistributedSessionConnectionId,
        frame: &[u8],
    ) -> Result<(), DistributedSessionRegistryError> {
        let slot_id = self.connected_slot_id(connection_id)?;
        self.ensure_global_capacity(frame.len())?;
        let slot = self
            .slots
            .get_mut(&slot_id)
            .expect("connection index points to a Session slot");
        slot.runtime.get_mut()?.admit_client_frame(frame)?;
        slot.inbound_frame_sizes.push_back(frame.len());
        self.global_queued_bytes += frame.len();
        Ok(())
    }

    pub fn next_client_frame(
        &mut self,
        connection_id: DistributedSessionConnectionId,
    ) -> Result<Option<Vec<u8>>, DistributedSessionRegistryError> {
        let slot_id = self.connected_slot_id(connection_id)?;
        self.slots
            .get_mut(&slot_id)
            .expect("connection index points to a Session slot")
            .runtime
            .get_mut()?
            .next_client_frame()
            .map_err(Into::into)
    }

    pub fn acknowledge_client_frame(
        &mut self,
        connection_id: DistributedSessionConnectionId,
    ) -> Result<bool, DistributedSessionRegistryError> {
        let slot_id = self.connected_slot_id(connection_id)?;
        Ok(self
            .slots
            .get_mut(&slot_id)
            .expect("connection index points to a Session slot")
            .runtime
            .get_mut()?
            .acknowledge_client_frame())
    }

    pub fn session_root_value_current(
        &mut self,
        connection_id: DistributedSessionConnectionId,
        name: &str,
    ) -> Result<Value, DistributedSessionRegistryError> {
        let slot_id = self.connected_slot_id(connection_id)?;
        self.slots
            .get_mut(&slot_id)
            .expect("connection index points to a Session slot")
            .runtime
            .get_mut()?
            .root_value_current(name)
            .map_err(Into::into)
    }

    pub fn complete_session_transient_effect(
        &mut self,
        origin: SessionOrigin,
        call_id: boon_runtime::TransientEffectCallId,
        outcome: Value,
    ) -> Result<bool, DistributedSessionRegistryError> {
        self.apply_session_transient_effect_update(origin, call_id, |runtime| {
            runtime.complete_transient_effect(call_id, outcome)
        })
    }

    pub fn deliver_session_transient_effect_result(
        &mut self,
        origin: SessionOrigin,
        call_id: boon_runtime::TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
    ) -> Result<bool, DistributedSessionRegistryError> {
        self.apply_session_transient_effect_update(origin, call_id, |runtime| {
            runtime.deliver_transient_effect_result(call_id, result_sequence, outcome)
        })
    }

    pub fn cancel_session_transient_effect(
        &mut self,
        origin: SessionOrigin,
        call_id: boon_runtime::TransientEffectCallId,
    ) -> Result<(), DistributedSessionRegistryError> {
        self.apply_session_transient_effect_update(origin, call_id, |runtime| {
            runtime.cancel_transient_effect(call_id)
        })?;
        Ok(())
    }

    fn apply_session_transient_effect_update(
        &mut self,
        origin: SessionOrigin,
        call_id: boon_runtime::TransientEffectCallId,
        apply: impl FnOnce(
            &mut DistributedSessionRuntime,
        ) -> Result<DistributedSessionUpdate, DistributedRuntimeError>,
    ) -> Result<bool, DistributedSessionRegistryError> {
        let slot_id = self.slot_id_for_origin(origin)?;
        let mut candidate = self
            .slots
            .get(&slot_id)
            .expect("resolved Session origin remains registered")
            .fork_settled()?;
        let update = apply(candidate.runtime.get_mut()?)?;
        let pending = candidate.runtime.has_pending_transient_effect(call_id);
        let candidates = BTreeMap::from([(slot_id, candidate)]);
        self.commit_slot_candidates(candidates);
        self.record_session_update(origin, update);
        Ok(pending)
    }

    fn slot_id_for_origin(
        &self,
        origin: SessionOrigin,
    ) -> Result<u32, DistributedSessionRegistryError> {
        self.slots
            .iter()
            .find_map(|(slot_id, slot)| (slot.origin == origin).then_some(*slot_id))
            .ok_or(DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::InvalidLease,
            ))
    }

    pub fn poll(
        &mut self,
        server: &mut DistributedServerAuthority<'_, impl DistributedServerMachine>,
        now: Duration,
        maximum_steps: usize,
    ) -> Result<DistributedSessionRegistryPoll, DistributedSessionRegistryError> {
        self.observe_lifecycle(now)?;
        let mut poll = DistributedSessionRegistryPoll::new(0);
        for _ in 0..maximum_steps {
            let Some(slot_id) = self.next_runnable_slot() else {
                break;
            };
            let completing_expiry = matches!(
                self.slots.get(&slot_id).map(|slot| &slot.state),
                Some(SessionSlotState::Stale {
                    cleanup: Some(SessionCleanupDisposition::Remove),
                    ..
                })
            );
            let origin = self
                .slots
                .get(&slot_id)
                .expect("selected Session slot remains registered")
                .origin;
            let connection_id = self.slots.get(&slot_id).and_then(SessionSlot::connected_id);
            let outcome = self.poll_slot_once(server, slot_id);
            self.round_robin_cursor = Some(slot_id);
            match outcome {
                LanePoll::Progress => {
                    poll.serviced_origins.push(origin);
                    if let Some(connection_id) = connection_id {
                        poll.serviced_connections.push(connection_id);
                    }
                    if completing_expiry && !self.slots.contains_key(&slot_id) {
                        poll.expired_sessions += 1;
                    }
                }
                LanePoll::Backpressured => poll.backpressured_origins.push(origin),
                LanePoll::Poisoned(error) => {
                    let mut diagnostic = bounded_diagnostic(&error);
                    if let Err(cleanup_error) = self.remove_slot(server, slot_id) {
                        diagnostic = bounded_diagnostic(&format_args!(
                            "{diagnostic}; cleanup failed: {cleanup_error}"
                        ));
                    }
                    poll.poisoned_sessions.push(PoisonedDistributedSession {
                        connection_id,
                        diagnostic,
                    });
                }
            }
        }
        poll.session_turns
            .extend(self.pending_session_turns.drain(..));
        poll.server_turns
            .extend(self.pending_server_turns.drain(..));
        Ok(poll)
    }

    fn commit_fresh(
        &mut self,
        server: &mut DistributedServerAuthority<'_, impl DistributedServerMachine>,
        connection_id: DistributedSessionConnectionId,
        principal: SessionPrincipal,
        next_resume_digest: [u8; 32],
        session_id: SessionId,
        next_transport_generation: u64,
    ) -> Result<(), DistributedSessionRegistryError> {
        if self.slots.len() >= self.config.max_sessions {
            return Err(DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::SessionCapacity {
                    limit: self.config.max_sessions,
                },
            ));
        }
        if self.resume_index.contains_key(&next_resume_digest) {
            return Err(DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::InvalidLease,
            ));
        }
        let queue_reservation = queue_reservation_per_session(self.config)?;
        let next_reserved = self
            .global_reserved_queue_bytes
            .checked_add(queue_reservation)
            .ok_or(DistributedSessionRegistryError::InvalidConfig(
                "distributed Session queue reservation overflowed",
            ))?;
        if next_reserved > self.config.max_global_queued_bytes {
            return Err(DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::QueueBytesFull {
                    limit: self.config.max_global_queued_bytes,
                },
            ));
        }
        let slot_id =
            self.available_slot_id()
                .ok_or(DistributedSessionRegistryError::InvalidConfig(
                    "max_sessions exceeds the Session slot identifier space",
                ))?;
        let slot_epoch = self.next_slot_epoch(slot_id)?;
        let origin = SessionOrigin::new(slot_id, slot_epoch)?;
        let execution_scope = self.take_execution_scope()?;
        let mut runtime = self.session_template.instantiate(
            session_id,
            next_transport_generation,
            principal.clone(),
            self.config.session_queue_limits,
        )?;
        let current_update = runtime.mark_current()?;
        server.attach_origin(origin, principal.clone(), execution_scope)?;

        self.global_reserved_queue_bytes = next_reserved;
        self.slot_epochs.insert(slot_id, slot_epoch);
        self.connections.insert(connection_id, slot_id);
        self.resume_index.insert(next_resume_digest, slot_id);
        self.slots.insert(
            slot_id,
            SessionSlot {
                origin,
                execution_scope,
                principal,
                runtime: SessionRuntimeSlab::new(runtime),
                transport_generation: next_transport_generation,
                resume_digest: next_resume_digest,
                state: SessionSlotState::Connected { connection_id },
                inbound_frame_sizes: VecDeque::new(),
                pending_server_messages: VecDeque::new(),
                pending_server_bytes: 0,
                next_lane: 0,
            },
        );

        let initialization = (|| {
            self.record_session_update(origin, current_update);
            let server_update =
                server.set_origin_status(origin, SessionConnectionStatus::Current)?;
            self.route_server_update(origin, server_update)
        })();
        if let Err(error) = initialization {
            let cleanup = self.remove_slot(server, slot_id);
            self.pending_session_turns
                .retain(|(pending_origin, _)| *pending_origin != origin);
            self.pending_server_turns
                .retain(|(pending_origin, _)| *pending_origin != origin);
            return match cleanup {
                Ok(()) => Err(error),
                Err(cleanup_error) => Err(cleanup_error),
            };
        }
        Ok(())
    }

    fn commit_resume(
        &mut self,
        server: &mut DistributedServerAuthority<'_, impl DistributedServerMachine>,
        connection_id: DistributedSessionConnectionId,
        slot_id: u32,
        next_resume_digest: [u8; 32],
        next_transport_generation: u64,
        applied_server_through: u64,
    ) -> Result<(), DistributedSessionRegistryError> {
        if self
            .resume_index
            .get(&next_resume_digest)
            .is_some_and(|existing| *existing != slot_id)
        {
            return Err(DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::InvalidLease,
            ));
        }
        let (origin, old_resume_digest) = {
            let slot = self
                .slots
                .get(&slot_id)
                .ok_or(DistributedSessionRegistryError::SessionExpired)?;
            if !matches!(slot.state, SessionSlotState::Stale { cleanup: None, .. }) {
                return Err(DistributedSessionRegistryError::SessionNotConnected);
            }
            (slot.origin, slot.resume_digest)
        };

        let mut candidate = self.fork_settled()?;
        let (rebind, current) = {
            let slot = candidate
                .slots
                .get_mut(&slot_id)
                .expect("validated resumable Session remains registered");
            let runtime = slot.runtime.get_mut()?;
            let rebind =
                runtime.rebind_client(next_transport_generation, applied_server_through)?;
            let current = runtime.mark_current()?;
            (rebind, current)
        };
        candidate.record_session_update(origin, rebind);
        candidate.record_session_update(origin, current);

        let prepared =
            server.prepare_origin_status_transaction(origin, SessionConnectionStatus::Current)?;
        self.commit_registry_candidate_transaction(server, candidate, origin, prepared)?;

        self.resume_index.remove(&old_resume_digest);
        self.resume_index.insert(next_resume_digest, slot_id);
        self.connections.insert(connection_id, slot_id);
        let slot = self
            .slots
            .get_mut(&slot_id)
            .expect("committed resumed Session remains registered");
        slot.transport_generation = next_transport_generation;
        slot.resume_digest = next_resume_digest;
        slot.state = SessionSlotState::Connected { connection_id };
        Ok(())
    }

    fn begin_fresh(
        &mut self,
        now: Duration,
        principal: SessionPrincipal,
    ) -> Result<DistributedSessionHandshakeStart, DistributedSessionRegistryError> {
        let pending_fresh = self
            .pending_handshakes
            .values()
            .filter(|pending| matches!(&pending.kind, PendingHandshakeKind::Fresh { .. }))
            .count();
        if self.pending_handshakes.len() >= self.config.max_pending_handshakes
            || self
                .slots
                .len()
                .checked_add(pending_fresh)
                .is_none_or(|total| total >= self.config.max_sessions)
        {
            return self.rejection(DistributedSessionHandshakeRejectionReason::Capacity);
        }
        let connection_id = self.take_connection_id()?;
        let deadline = checked_deadline(now, self.config.handshake_timeout)?;
        let next_token = ResumeToken::generate()?;
        let session_id = SessionId::generate()?;
        let next_resume_digest = resume_digest(&next_token);
        let offer_frame = encode_session_control_frame(&SessionControlFrame::ServerOffer(
            ServerOffer::new(next_token, session_id, 1, 0),
        ))?;
        self.pending_handshakes.insert(
            connection_id,
            PendingHandshake {
                connection_id,
                deadline,
                kind: PendingHandshakeKind::Fresh { principal },
                next_resume_digest,
                session_id,
                next_transport_generation: 1,
                applied_server_through: 0,
                applied_client_through: 0,
            },
        );
        Ok(DistributedSessionHandshakeStart::Offer(
            DistributedSessionHandshakeOffer {
                connection_id,
                server_frame: offer_frame,
            },
        ))
    }

    fn begin_resume(
        &mut self,
        now: Duration,
        principal: SessionPrincipal,
        token: ResumeToken,
        applied_server_through: u64,
    ) -> Result<DistributedSessionHandshakeStart, DistributedSessionRegistryError> {
        if self.pending_handshakes.len() >= self.config.max_pending_handshakes {
            return self.rejection(DistributedSessionHandshakeRejectionReason::Capacity);
        }
        let digest = resume_digest(&token);
        let Some(slot_id) = self.resume_index.get(&digest).copied() else {
            return self.rejection(DistributedSessionHandshakeRejectionReason::ResumeUnavailable);
        };
        let slot = self.slots.get(&slot_id).expect("matched Session slot");
        let SessionSlotState::Stale {
            deadline,
            cleanup: None,
        } = &slot.state
        else {
            return self.rejection(DistributedSessionHandshakeRejectionReason::ResumeUnavailable);
        };
        if slot.principal != principal {
            return self.rejection(DistributedSessionHandshakeRejectionReason::ResumeUnavailable);
        }
        if self.pending_handshakes.values().any(|pending| {
            matches!(&pending.kind, PendingHandshakeKind::Resume { slot_id: pending_slot } if *pending_slot == slot_id)
        }) {
            return self.rejection(DistributedSessionHandshakeRejectionReason::ResumeUnavailable);
        }
        let resume_deadline = *deadline;
        let deadline = checked_deadline(now, self.config.handshake_timeout)?.min(resume_deadline);
        if now >= deadline {
            return self.rejection(DistributedSessionHandshakeRejectionReason::ResumeUnavailable);
        }
        let next_transport_generation = slot.transport_generation.checked_add(1).ok_or(
            DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::StaleTransportGeneration,
            ),
        )?;
        let next_token = ResumeToken::generate()?;
        let next_resume_digest = resume_digest(&next_token);
        let session_id = slot.runtime.session_id();
        let applied_client_through = slot.runtime.applied_client_through();
        let offer_frame =
            encode_session_control_frame(&SessionControlFrame::ServerOffer(ServerOffer::new(
                next_token,
                session_id,
                next_transport_generation,
                applied_client_through,
            )))?;
        let connection_id = self.take_connection_id()?;
        self.pending_handshakes.insert(
            connection_id,
            PendingHandshake {
                connection_id,
                deadline,
                kind: PendingHandshakeKind::Resume { slot_id },
                next_resume_digest,
                session_id,
                next_transport_generation,
                applied_server_through,
                applied_client_through,
            },
        );
        Ok(DistributedSessionHandshakeStart::Offer(
            DistributedSessionHandshakeOffer {
                connection_id,
                server_frame: offer_frame,
            },
        ))
    }

    fn rejection(
        &self,
        reason: DistributedSessionHandshakeRejectionReason,
    ) -> Result<DistributedSessionHandshakeStart, DistributedSessionRegistryError> {
        Ok(DistributedSessionHandshakeStart::Reject(
            DistributedSessionHandshakeRejection {
                reason,
                server_frame: encode_session_control_frame(&SessionControlFrame::ServerReject(
                    ServerReject::new(),
                ))?,
            },
        ))
    }

    fn connected_slot_id(
        &self,
        connection_id: DistributedSessionConnectionId,
    ) -> Result<u32, DistributedSessionRegistryError> {
        let slot_id = self
            .connections
            .get(&connection_id)
            .copied()
            .ok_or(DistributedSessionRegistryError::UnknownConnection)?;
        self.slots
            .get(&slot_id)
            .and_then(SessionSlot::connected_id)
            .filter(|current| *current == connection_id)
            .map(|_| slot_id)
            .ok_or(DistributedSessionRegistryError::SessionNotConnected)
    }

    fn available_slot_id(&self) -> Option<u32> {
        (0..self.config.max_sessions).find_map(|candidate| {
            let candidate = u32::try_from(candidate).ok()?;
            (!self.slots.contains_key(&candidate)).then_some(candidate)
        })
    }

    fn next_slot_epoch(&self, slot_id: u32) -> Result<u64, DistributedSessionRegistryError> {
        self.slot_epochs
            .get(&slot_id)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::StaleTransportGeneration,
            ))
    }

    fn take_connection_id(
        &mut self,
    ) -> Result<DistributedSessionConnectionId, DistributedSessionRegistryError> {
        let id = self.next_connection_id;
        self.next_connection_id = self.next_connection_id.checked_add(1).ok_or(
            DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::StaleTransportGeneration,
            ),
        )?;
        Ok(DistributedSessionConnectionId(id))
    }

    fn take_execution_scope(&mut self) -> Result<u64, DistributedSessionRegistryError> {
        let scope = self.next_execution_scope;
        self.next_execution_scope = self.next_execution_scope.checked_add(1).ok_or(
            DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::StaleTransientEffectOwner,
            ),
        )?;
        Ok(scope)
    }

    fn observe_now(&mut self, now: Duration) -> Result<(), DistributedSessionRegistryError> {
        if now < self.last_now {
            return Err(DistributedSessionRegistryError::TimeRegression);
        }
        self.last_now = now;
        Ok(())
    }

    fn observe_lifecycle(&mut self, now: Duration) -> Result<(), DistributedSessionRegistryError> {
        self.observe_now(now)?;
        self.pending_handshakes
            .retain(|_, pending| now < pending.deadline);
        Ok(())
    }

    fn begin_stale_cleanup<M: DistributedServerMachine>(
        &mut self,
        server: &mut DistributedServerAuthority<'_, M>,
        slot_id: u32,
        deadline: Duration,
        disposition: SessionCleanupDisposition,
    ) -> Result<(), DistributedSessionRegistryError> {
        let mut candidate = self.fork_settled()?;
        let (
            origin,
            connection_id,
            resume_digest,
            released_queue_bytes,
            cancellation,
            stale_update,
        ) = {
            let slot = candidate
                .slots
                .get_mut(&slot_id)
                .ok_or(DistributedSessionRegistryError::UnknownConnection)?;
            let inbound_bytes = slot
                .inbound_frame_sizes
                .iter()
                .copied()
                .try_fold(0usize, usize::checked_add)
                .ok_or(DistributedSessionRegistryError::InvalidConfig(
                    "distributed Session inbound-byte accounting overflowed",
                ))?;
            let pending_before = slot.pending_server_bytes;
            slot.pending_server_messages
                .retain(DistributedMessage::is_session_resume_snapshot);
            slot.pending_server_bytes = slot
                .pending_server_messages
                .iter()
                .try_fold(0usize, |total, message| {
                    total.checked_add(estimated_message_bytes(message)?)
                })
                .ok_or(DistributedSessionRegistryError::InvalidConfig(
                    "distributed Session retained-message accounting overflowed",
                ))?;
            let released_server_bytes = pending_before
                .checked_sub(slot.pending_server_bytes)
                .ok_or(DistributedSessionRegistryError::InvalidConfig(
                    "distributed Session retained-message accounting underflowed",
                ))?;
            let runtime = slot.runtime.get_mut()?;
            let cancellation = runtime.cancel_all_transient_effects()?;
            let stale_update = runtime.mark_stale()?;
            let connection_id = slot.connection_id();
            slot.inbound_frame_sizes.clear();
            slot.state = SessionSlotState::Stale {
                deadline,
                cleanup: Some(disposition),
            };
            (
                slot.origin,
                connection_id,
                slot.resume_digest,
                inbound_bytes.checked_add(released_server_bytes).ok_or(
                    DistributedSessionRegistryError::InvalidConfig(
                        "distributed Session released-byte accounting overflowed",
                    ),
                )?,
                cancellation,
                stale_update,
            )
        };
        candidate.global_queued_bytes = candidate
            .global_queued_bytes
            .checked_sub(released_queue_bytes)
            .ok_or(DistributedSessionRegistryError::InvalidConfig(
                "distributed Session inbound-byte accounting underflowed",
            ))?;
        if let Some(connection_id) = connection_id {
            candidate.connections.remove(&connection_id);
        }
        if disposition == SessionCleanupDisposition::Remove {
            candidate.resume_index.remove(&resume_digest);
            candidate.pending_handshakes.retain(|_, pending| {
                !matches!(
                    pending.kind,
                    PendingHandshakeKind::Resume {
                        slot_id: pending_slot
                    } if pending_slot == slot_id
                )
            });
        }
        candidate.record_session_update(origin, cancellation);
        candidate.record_session_update(origin, stale_update);

        if server.has_origin(origin) {
            let prepared =
                server.prepare_origin_status_transaction(origin, SessionConnectionStatus::Stale)?;
            self.commit_registry_candidate_transaction(server, candidate, origin, prepared)
        } else {
            self.commit_registry_candidate_checkpoint(candidate)
        }
    }

    fn drive_cleanup<M: DistributedServerMachine>(
        &mut self,
        server: &mut DistributedServerAuthority<'_, M>,
        slot_id: u32,
    ) -> Result<(), DistributedSessionRegistryError> {
        for _ in 0..MAX_SESSION_CLEANUP_ROUNDS {
            let Some(slot) = self.slots.get(&slot_id) else {
                return Ok(());
            };
            if !matches!(
                slot.state,
                SessionSlotState::Stale {
                    cleanup: Some(_),
                    ..
                }
            ) {
                return Ok(());
            }
            self.poll_cleanup_step(server, slot_id)?;
        }
        Err(DistributedSessionRegistryError::Runtime(
            DistributedRuntimeError::Runtime(
                "distributed Session cleanup did not reach a fixed point".to_owned(),
            ),
        ))
    }

    fn poll_cleanup_step<M: DistributedServerMachine>(
        &mut self,
        server: &mut DistributedServerAuthority<'_, M>,
        slot_id: u32,
    ) -> Result<(), DistributedSessionRegistryError> {
        let (origin, disposition, has_server_delivery, has_session_effect, has_session_delivery) = {
            let slot = self
                .slots
                .get(&slot_id)
                .ok_or(DistributedSessionRegistryError::UnknownConnection)?;
            let SessionSlotState::Stale {
                cleanup: Some(disposition),
                ..
            } = slot.state
            else {
                return Ok(());
            };
            (
                slot.origin,
                disposition,
                !slot.pending_server_messages.is_empty(),
                slot.runtime.pending_transient_effect_count() != 0,
                slot.runtime.pending_server_messages() != 0,
            )
        };

        if has_server_delivery {
            return self.poll_server_delivery(slot_id);
        }
        if has_session_effect {
            let mut candidate = self.fork_settled()?;
            let cancellation = candidate
                .slots
                .get_mut(&slot_id)
                .expect("cleanup Session remains registered")
                .runtime
                .get_mut()?
                .cancel_all_transient_effects()?;
            candidate.record_session_update(origin, cancellation);
            return self.commit_registry_candidate_checkpoint(candidate);
        }
        if has_session_delivery {
            return self.poll_session_delivery(server, slot_id);
        }
        if let Some(call_id) = server.next_origin_transient_effect(origin) {
            let candidate = self.fork_settled()?;
            let prepared =
                server.prepare_transient_effect_cancellation_transaction(call_id, true)?;
            return self.commit_registry_candidate_transaction(server, candidate, origin, prepared);
        }

        match disposition {
            SessionCleanupDisposition::Resume => {
                let mut candidate = self.fork_settled()?;
                let slot = candidate
                    .slots
                    .get_mut(&slot_id)
                    .expect("resumable cleanup Session remains registered");
                let SessionSlotState::Stale { cleanup, .. } = &mut slot.state else {
                    unreachable!("cleanup only runs for stale Sessions")
                };
                *cleanup = None;
                self.commit_registry_candidate_checkpoint(candidate)
            }
            SessionCleanupDisposition::Remove => {
                let mut candidate = self.fork_settled()?;
                let mut detached = candidate.detach_slots(&[slot_id])?;
                let removed = detached.pop().expect("one cleanup Session was detached");
                debug_assert_eq!(removed.origin, origin);
                if server.has_origin(origin) {
                    let prepared = server.prepare_origin_expiration_transaction(origin)?;
                    self.commit_registry_candidate_transaction(server, candidate, origin, prepared)
                } else {
                    self.commit_registry_candidate_checkpoint(candidate)
                }
            }
        }
    }

    fn ensure_global_capacity(
        &self,
        additional: usize,
    ) -> Result<(), DistributedSessionRegistryError> {
        let next = self.global_queued_bytes.checked_add(additional).ok_or(
            DistributedSessionRegistryError::Runtime(DistributedRuntimeError::QueueBytesFull {
                limit: self.config.max_global_queued_bytes,
            }),
        )?;
        if next > self.config.max_global_queued_bytes {
            return Err(DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::QueueBytesFull {
                    limit: self.config.max_global_queued_bytes,
                },
            ));
        }
        Ok(())
    }

    fn remove_slot(
        &mut self,
        server: &mut DistributedServerAuthority<'_, impl DistributedServerMachine>,
        slot_id: u32,
    ) -> Result<(), DistributedSessionRegistryError> {
        self.begin_stale_cleanup(
            server,
            slot_id,
            self.last_now,
            SessionCleanupDisposition::Remove,
        )?;
        self.drive_cleanup(server, slot_id)
    }

    fn detach_slots(
        &mut self,
        slot_ids: &[u32],
    ) -> Result<Vec<SessionSlot>, DistributedSessionRegistryError> {
        let queued_bytes = slot_ids.iter().try_fold(0usize, |total, slot_id| {
            let slot = self
                .slots
                .get(slot_id)
                .ok_or(DistributedSessionRegistryError::UnknownConnection)?;
            let slot_bytes = slot.queued_registry_bytes().ok_or(
                DistributedSessionRegistryError::InvalidConfig(
                    "distributed Session queued-byte accounting overflowed",
                ),
            )?;
            total
                .checked_add(slot_bytes)
                .ok_or(DistributedSessionRegistryError::InvalidConfig(
                    "distributed Session queued-byte accounting overflowed",
                ))
        })?;
        let reserved_bytes = queue_reservation_per_session(self.config)?
            .checked_mul(slot_ids.len())
            .ok_or(DistributedSessionRegistryError::InvalidConfig(
                "distributed Session reservation accounting overflowed",
            ))?;
        let next_queued = self.global_queued_bytes.checked_sub(queued_bytes).ok_or(
            DistributedSessionRegistryError::InvalidConfig(
                "distributed Session queued-byte accounting underflowed",
            ),
        )?;
        let next_reserved = self
            .global_reserved_queue_bytes
            .checked_sub(reserved_bytes)
            .ok_or(DistributedSessionRegistryError::InvalidConfig(
                "distributed Session reservation accounting underflowed",
            ))?;

        let mut detached = Vec::with_capacity(slot_ids.len());
        for slot_id in slot_ids {
            let slot = self
                .slots
                .remove(slot_id)
                .expect("all Session slots were validated before detachment");
            self.resume_index.remove(&slot.resume_digest);
            self.pending_handshakes.retain(|_, pending| {
                !matches!(
                    &pending.kind,
                    PendingHandshakeKind::Resume { slot_id: pending_slot }
                        if pending_slot == slot_id
                )
            });
            if let Some(connection_id) = slot.connection_id() {
                self.connections.remove(&connection_id);
            }
            if self.round_robin_cursor == Some(*slot_id) {
                self.round_robin_cursor = None;
            }
            detached.push(slot);
        }
        self.global_queued_bytes = next_queued;
        self.global_reserved_queue_bytes = next_reserved;
        Ok(detached)
    }

    fn next_runnable_slot(&self) -> Option<u32> {
        let after_cursor = self.round_robin_cursor.and_then(|cursor| {
            self.slots
                .range((
                    std::ops::Bound::Excluded(cursor),
                    std::ops::Bound::Unbounded,
                ))
                .find_map(|(slot_id, slot)| {
                    slot.has_runnable_work(self.last_now).then_some(*slot_id)
                })
        });
        after_cursor.or_else(|| {
            self.slots.iter().find_map(|(slot_id, slot)| {
                slot.has_runnable_work(self.last_now).then_some(*slot_id)
            })
        })
    }

    fn poll_slot_once(
        &mut self,
        server: &mut DistributedServerAuthority<'_, impl DistributedServerMachine>,
        slot_id: u32,
    ) -> LanePoll {
        if let SessionSlotState::Stale {
            deadline,
            cleanup: None,
        } = self
            .slots
            .get(&slot_id)
            .expect("selected Session slot remains registered")
            .state
            && self.last_now >= deadline
        {
            return match self.begin_stale_cleanup(
                server,
                slot_id,
                deadline,
                SessionCleanupDisposition::Remove,
            ) {
                Ok(()) => LanePoll::Progress,
                Err(error) if is_queue_pressure(&error) => LanePoll::Backpressured,
                Err(error) => LanePoll::Poisoned(error),
            };
        }
        if matches!(
            self.slots
                .get(&slot_id)
                .expect("selected Session slot remains registered")
                .state,
            SessionSlotState::Stale {
                cleanup: Some(_),
                ..
            }
        ) {
            return match self.poll_cleanup_step(server, slot_id) {
                Ok(()) => LanePoll::Progress,
                Err(error) if is_queue_pressure(&error) => LanePoll::Backpressured,
                Err(error) => LanePoll::Poisoned(error),
            };
        }
        let next_lane = self
            .slots
            .get(&slot_id)
            .expect("selected Session slot remains registered")
            .next_lane;
        for offset in 0..3 {
            let lane = (next_lane + offset) % 3;
            let available = {
                let slot = self
                    .slots
                    .get(&slot_id)
                    .expect("selected Session slot remains registered");
                match lane {
                    0 => !slot.pending_server_messages.is_empty(),
                    1 => slot.connected_id().is_some() && !slot.inbound_frame_sizes.is_empty(),
                    2 => slot.runtime.pending_server_messages() > 0,
                    _ => unreachable!(),
                }
            };
            if !available {
                continue;
            }
            let outcome = match lane {
                0 => self.poll_server_delivery(slot_id),
                1 => self.poll_client_admission(slot_id),
                2 => self.poll_session_delivery(server, slot_id),
                _ => unreachable!(),
            };
            self.slots
                .get_mut(&slot_id)
                .expect("selected Session slot remains registered")
                .next_lane = (lane + 1) % 3;
            return match outcome {
                Ok(()) => LanePoll::Progress,
                Err(error) if is_queue_pressure(&error) => LanePoll::Backpressured,
                Err(error) => LanePoll::Poisoned(error),
            };
        }
        LanePoll::Progress
    }

    fn poll_server_delivery(
        &mut self,
        slot_id: u32,
    ) -> Result<(), DistributedSessionRegistryError> {
        let (origin, message, next_pending_bytes, next_global_queued) = {
            let slot = self
                .slots
                .get(&slot_id)
                .expect("selected Session slot remains registered");
            let message = slot
                .pending_server_messages
                .front()
                .expect("selected server delivery lane is non-empty")
                .clone();
            let bytes = estimated_message_bytes(&message).ok_or(
                DistributedSessionRegistryError::Runtime(DistributedRuntimeError::QueueBytesFull {
                    limit: self.config.session_queue_limits.max_bytes,
                }),
            )?;
            let next_pending_bytes = slot.pending_server_bytes.checked_sub(bytes).ok_or(
                DistributedSessionRegistryError::InvalidConfig(
                    "distributed Server-delivery byte accounting underflowed",
                ),
            )?;
            let next_global_queued = self.global_queued_bytes.checked_sub(bytes).ok_or(
                DistributedSessionRegistryError::InvalidConfig(
                    "distributed global queue byte accounting underflowed",
                ),
            )?;
            (slot.origin, message, next_pending_bytes, next_global_queued)
        };
        let mut candidate = self
            .slots
            .get(&slot_id)
            .expect("selected Session slot remains registered")
            .fork_settled()?;
        let update = candidate
            .runtime
            .get_mut()?
            .accept_server_message(message)?;
        candidate.pending_server_messages.pop_front();
        candidate.pending_server_bytes = next_pending_bytes;
        let candidates = BTreeMap::from([(slot_id, candidate)]);
        self.commit_slot_candidates(candidates);
        self.global_queued_bytes = next_global_queued;
        self.record_session_update(origin, update);
        Ok(())
    }

    fn poll_client_admission(
        &mut self,
        slot_id: u32,
    ) -> Result<(), DistributedSessionRegistryError> {
        let (origin, next_global_queued) = {
            let slot = self
                .slots
                .get(&slot_id)
                .expect("selected Session slot remains registered");
            let bytes = *slot
                .inbound_frame_sizes
                .front()
                .expect("selected client admission lane is non-empty");
            let next_global_queued = self.global_queued_bytes.checked_sub(bytes).ok_or(
                DistributedSessionRegistryError::InvalidConfig(
                    "distributed client-frame byte accounting underflowed",
                ),
            )?;
            (slot.origin, next_global_queued)
        };
        let mut candidate = self
            .slots
            .get(&slot_id)
            .expect("selected Session slot remains registered")
            .fork_settled()?;
        let update = candidate.runtime.get_mut()?.poll_client_frame()?;
        candidate.inbound_frame_sizes.pop_front();
        let candidates = BTreeMap::from([(slot_id, candidate)]);
        self.commit_slot_candidates(candidates);
        self.global_queued_bytes = next_global_queued;
        if let Some(update) = update {
            self.record_session_update(origin, update);
        }
        Ok(())
    }

    fn poll_session_delivery(
        &mut self,
        server: &mut DistributedServerAuthority<'_, impl DistributedServerMachine>,
        slot_id: u32,
    ) -> Result<(), DistributedSessionRegistryError> {
        let (origin, message) = {
            let slot = self
                .slots
                .get(&slot_id)
                .expect("selected Session slot remains registered");
            let message = slot
                .runtime
                .next_server_message()
                .expect("selected Session delivery lane is non-empty");
            (slot.origin, message)
        };
        let prepared_transaction = server.prepare_session_message(origin, message)?;
        let prepared_deliveries = match self.prepare_deliveries(prepared_transaction.deliveries()) {
            Ok(prepared) => prepared,
            Err(error) => {
                server.rollback_prepared_transaction(prepared_transaction)?;
                return Err(error);
            }
        };
        let mut candidates = match self.fork_delivery_slots(&prepared_deliveries) {
            Ok(candidates) => candidates,
            Err(error) => {
                server.rollback_prepared_transaction(prepared_transaction)?;
                return Err(error);
            }
        };
        let source = match candidates.get_mut(&slot_id) {
            Some(source) => source,
            None => {
                let source = match self
                    .slots
                    .get(&slot_id)
                    .expect("selected Session slot remains registered")
                    .fork_settled()
                {
                    Ok(source) => source,
                    Err(error) => {
                        server.rollback_prepared_transaction(prepared_transaction)?;
                        return Err(error);
                    }
                };
                candidates.insert(slot_id, source);
                candidates
                    .get_mut(&slot_id)
                    .expect("inserted source Session candidate")
            }
        };
        let acknowledged = source.runtime.get_mut()?.acknowledge_server_message();
        debug_assert!(acknowledged);

        let update = server.commit_prepared_transaction(prepared_transaction)?;
        self.commit_slot_candidates(candidates);
        self.global_queued_bytes = prepared_deliveries.prospective_global;
        self.pending_server_turns
            .extend(update.turns.into_iter().map(|turn| (origin, turn)));
        Ok(())
    }

    fn record_session_update(&mut self, origin: SessionOrigin, update: DistributedSessionUpdate) {
        self.pending_session_turns
            .extend(update.turns.into_iter().map(|turn| (origin, turn)));
    }

    pub fn publish_server_deliveries(
        &mut self,
        deliveries: Vec<ServerDelivery>,
    ) -> Result<(), DistributedSessionRegistryError> {
        #[cfg(test)]
        if self
            .inject_server_publication_queue_pressure
            .swap(false, Ordering::SeqCst)
        {
            return Err(DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::QueueFull {
                    limit: self.config.session_queue_limits.max_messages,
                },
            ));
        }
        if deliveries.is_empty() {
            return Ok(());
        }
        let prepared = self.prepare_deliveries(&deliveries)?;
        self.commit_deliveries(prepared);
        Ok(())
    }

    fn route_server_update(
        &mut self,
        origin: SessionOrigin,
        update: DistributedServerUpdate,
    ) -> Result<(), DistributedSessionRegistryError> {
        let prepared = self.prepare_deliveries(&update.deliveries)?;
        self.commit_deliveries(prepared);
        self.pending_server_turns
            .extend(update.turns.into_iter().map(|turn| (origin, turn)));
        Ok(())
    }

    #[cfg(test)]
    fn route_server_delivery(
        &mut self,
        delivery: ServerDelivery,
    ) -> Result<(), DistributedSessionRegistryError> {
        let prepared = self.prepare_deliveries(std::slice::from_ref(&delivery))?;
        self.commit_deliveries(prepared);
        Ok(())
    }

    pub fn prepare_deliveries(
        &self,
        deliveries: &[ServerDelivery],
    ) -> Result<PreparedDistributedSessionDeliveries, DistributedSessionRegistryError> {
        let mut candidates = BTreeMap::new();
        let mut prospective_global = self.global_queued_bytes;
        for delivery in deliveries {
            let target_slots = match delivery.target {
                ServerDeliveryTarget::Origin(origin) => vec![
                    self.slots
                        .iter()
                        .find_map(|(slot_id, slot)| (slot.origin == origin).then_some(*slot_id))
                        .ok_or(DistributedSessionRegistryError::Runtime(
                            DistributedRuntimeError::InvalidLease,
                        ))?,
                ],
                ServerDeliveryTarget::AllSessions => self.slots.keys().copied().collect(),
            };
            for slot_id in target_slots {
                let slot = self
                    .slots
                    .get(&slot_id)
                    .expect("server delivery target remains registered");
                if matches!(slot.state, SessionSlotState::Stale { .. })
                    && !delivery.message.is_session_resume_snapshot()
                {
                    continue;
                }
                let (current, current_bytes) = candidates
                    .get(&slot_id)
                    .map(|(messages, bytes)| (messages, *bytes))
                    .unwrap_or((&slot.pending_server_messages, slot.pending_server_bytes));
                let (messages, bytes) = candidate_server_queue(
                    current,
                    delivery.message.clone(),
                    self.config.session_queue_limits,
                )?;
                prospective_global = prospective_global
                    .checked_sub(current_bytes)
                    .ok_or(DistributedSessionRegistryError::InvalidConfig(
                        "distributed delivery candidate accounting underflowed",
                    ))?
                    .checked_add(bytes)
                    .ok_or(DistributedSessionRegistryError::Runtime(
                        DistributedRuntimeError::QueueBytesFull {
                            limit: self.config.max_global_queued_bytes,
                        },
                    ))?;
                if prospective_global > self.config.max_global_queued_bytes {
                    return Err(DistributedSessionRegistryError::Runtime(
                        DistributedRuntimeError::QueueBytesFull {
                            limit: self.config.max_global_queued_bytes,
                        },
                    ));
                }
                candidates.insert(slot_id, (messages, bytes));
            }
        }
        Ok(PreparedDistributedSessionDeliveries {
            candidates,
            prospective_global,
        })
    }

    fn fork_delivery_slots(
        &self,
        prepared: &PreparedDistributedSessionDeliveries,
    ) -> Result<BTreeMap<u32, SessionSlot>, DistributedSessionRegistryError> {
        prepared
            .candidates
            .iter()
            .map(|(slot_id, (messages, bytes))| {
                let mut candidate = self
                    .slots
                    .get(slot_id)
                    .expect("prepared delivery target remains registered")
                    .fork_settled()?;
                candidate.pending_server_messages = messages.clone();
                candidate.pending_server_bytes = *bytes;
                Ok((*slot_id, candidate))
            })
            .collect()
    }

    fn commit_slot_candidates(&mut self, candidates: BTreeMap<u32, SessionSlot>) {
        for (slot_id, candidate) in candidates {
            let replaced = self.slots.insert(slot_id, candidate);
            debug_assert!(replaced.is_some());
        }
    }

    pub fn commit_deliveries(&mut self, prepared: PreparedDistributedSessionDeliveries) {
        for (slot_id, (messages, bytes)) in prepared.candidates {
            let slot = self
                .slots
                .get_mut(&slot_id)
                .expect("server delivery target remains registered");
            slot.pending_server_messages = messages;
            slot.pending_server_bytes = bytes;
        }
        self.global_queued_bytes = prepared.prospective_global;
    }
}

fn validate_config(
    config: DistributedSessionRegistryConfig,
) -> Result<(), DistributedSessionRegistryError> {
    if config.max_sessions == 0 {
        return Err(DistributedSessionRegistryError::InvalidConfig(
            "distributed Session capacity must be positive",
        ));
    }
    if config.max_pending_handshakes == 0 {
        return Err(DistributedSessionRegistryError::InvalidConfig(
            "distributed pending handshake capacity must be positive",
        ));
    }
    if config.max_global_queued_bytes == 0 {
        return Err(DistributedSessionRegistryError::InvalidConfig(
            "distributed Session global queue byte limit must be positive",
        ));
    }
    if config.session_queue_limits.max_messages == 0 || config.session_queue_limits.max_bytes == 0 {
        return Err(DistributedSessionRegistryError::InvalidConfig(
            "distributed per-Session queue limits must be positive",
        ));
    }
    if config.resume_window.is_zero() {
        return Err(DistributedSessionRegistryError::InvalidConfig(
            "distributed Session resume window must be positive",
        ));
    }
    if config.handshake_timeout.is_zero() || config.handshake_timeout > config.resume_window {
        return Err(DistributedSessionRegistryError::InvalidConfig(
            "distributed Session handshake timeout must be positive and no longer than the resume window",
        ));
    }
    let _ = queue_reservation_per_session(config)?;
    Ok(())
}

fn queue_reservation_per_session(
    config: DistributedSessionRegistryConfig,
) -> Result<usize, DistributedSessionRegistryError> {
    config
        .session_queue_limits
        .max_bytes
        .checked_mul(QUEUE_LANES_PER_SESSION)
        .ok_or(DistributedSessionRegistryError::InvalidConfig(
            "distributed per-Session queue reservation overflowed",
        ))
}

fn checked_deadline(
    now: Duration,
    window: Duration,
) -> Result<Duration, DistributedSessionRegistryError> {
    now.checked_add(window)
        .ok_or(DistributedSessionRegistryError::TimeOverflow)
}

fn bounded_diagnostic(error: &impl Display) -> String {
    const MAX_DIAGNOSTIC_BYTES: usize = 512;
    let mut diagnostic = error.to_string();
    if diagnostic.len() > MAX_DIAGNOSTIC_BYTES {
        diagnostic.truncate(MAX_DIAGNOSTIC_BYTES);
    }
    diagnostic
}

fn resume_digest(token: &ResumeToken) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(RESUME_DIGEST_DOMAIN);
    hasher.update(token.as_bytes());
    hasher.finalize().into()
}

fn candidate_server_queue(
    current: &VecDeque<DistributedMessage>,
    message: DistributedMessage,
    limits: DistributedQueueLimits,
) -> Result<(VecDeque<DistributedMessage>, usize), DistributedSessionRegistryError> {
    let mut candidate = current.clone();
    candidate.retain(|queued| !message.replaces_pending(queued));
    candidate.push_back(message);
    if candidate.len() > limits.max_messages {
        return Err(DistributedSessionRegistryError::Runtime(
            DistributedRuntimeError::QueueFull {
                limit: limits.max_messages,
            },
        ));
    }
    let bytes = candidate
        .iter()
        .try_fold(0usize, |total, message| {
            total.checked_add(estimated_message_bytes(message)?)
        })
        .ok_or(DistributedSessionRegistryError::Runtime(
            DistributedRuntimeError::QueueBytesFull {
                limit: limits.max_bytes,
            },
        ))?;
    if bytes > limits.max_bytes {
        return Err(DistributedSessionRegistryError::Runtime(
            DistributedRuntimeError::QueueBytesFull {
                limit: limits.max_bytes,
            },
        ));
    }
    Ok((candidate, bytes))
}

fn is_queue_pressure(error: &DistributedSessionRegistryError) -> bool {
    matches!(
        error,
        DistributedSessionRegistryError::Runtime(
            DistributedRuntimeError::QueueFull { .. }
                | DistributedRuntimeError::QueueBytesFull { .. }
        )
    )
}

fn estimated_message_bytes(message: &DistributedMessage) -> Option<usize> {
    message.estimated_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_persistence::StoredValue as DataValue;
    use boon_runtime::{
        ApplicationIdentity, DistributedClientRuntime, ProgramCapabilityProfile,
        ProgramCompileRequest, ProgramSession, RuntimeSourceUnit, SourcePayload,
        compile_distributed_program_bundle,
    };
    use boon_wire::{ClientCommit, ClientHello, ClientRevoke, SessionId};
    use std::sync::{Arc, Mutex, OnceLock};

    #[derive(Clone, Default)]
    struct SharedPersistenceDriver {
        inner: Arc<Mutex<boon_persistence::InMemoryDriver>>,
        commits_before_failure: Arc<Mutex<Option<usize>>>,
    }

    impl SharedPersistenceDriver {
        fn fail_after_commits(&self, successful_commits: usize) {
            *self.commits_before_failure.lock().unwrap() = Some(successful_commits);
        }

        fn snapshot(&self) -> boon_persistence::InMemoryDriver {
            self.inner.lock().unwrap().clone()
        }
    }

    impl boon_persistence::PersistenceDriver for SharedPersistenceDriver {
        fn execute(
            &mut self,
            command: boon_persistence::PersistenceCommand,
        ) -> boon_persistence::PersistenceResult {
            if matches!(&command, boon_persistence::PersistenceCommand::Commit(_)) {
                let mut remaining = self.commits_before_failure.lock().unwrap();
                if let Some(count) = remaining.as_mut() {
                    if *count == 0 {
                        *remaining = None;
                        return boon_persistence::PersistenceResult::Committed(Err(
                            boon_persistence::StoreError::Backend(
                                "injected distributed lifecycle commit failure".to_owned(),
                            ),
                        ));
                    }
                    *count -= 1;
                }
            }
            self.inner.lock().unwrap().execute(command)
        }
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

    const CLIENT_SOURCE: &str = r#"
store: [
    increment: SOURCE
]

scene: Scene/Element/text(
    element: [events: [press: store.increment]]
    style: [width: Fill]
    text: TEXT { Distributed registry }
)
"#;

    const SESSION_SOURCE: &str = r#"
store: [
    increment: Client/store.increment
    server_ready: Server/store.ready
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
]

"#;

    const SERVER_SOURCE: &str = r#"
store: [
    ready: True
]

"#;

    const SESSION_SERVER_COUNTER: &str = r#"
store: [
    increment: Client/store.increment
    ready: Server/store.ready
    server_count: Server/store.count
]

"#;

    const SERVER_COUNTER: &str = r#"
store: [
    increment: Session/store.increment
    ready: True
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
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
    text: TEXT { Server effect }
)
"#;

    const SESSION_SERVER_EFFECT: &str = r#"
store: [
    read_clock: Client/store.read_clock
    server_result: Server/store.clock_result
]

"#;

    const SERVER_EFFECT: &str = r#"
store: [
    read_clock: Session/store.read_clock
    clock_result:
        ClockNotRead |> HOLD clock_result {
            read_clock |> THEN { Clock/wall() }
        }
]

"#;

    const CLIENT_GLOBAL_EFFECT: &str = r#"
store: [noop: SOURCE]

scene: Scene/Element/text(
    element: [events: [press: store.noop]]
    style: [width: Fill]
    text: TEXT { Global effect recovery }
)
"#;

    const SESSION_GLOBAL_EFFECT: &str = r#"
store: [result: Server/store.result]
"#;

    const SERVER_GLOBAL_EFFECT: &str = r#"
store: [
    run: SOURCE
    result:
        NotStarted |> HOLD result {
            run |> THEN { Clock/wall() }
        }
]
"#;

    struct TestClient {
        connection_id: DistributedSessionConnectionId,
        resume_token: ResumeToken,
        generation: u64,
        runtime: DistributedClientRuntime,
    }

    struct RegistryHarness {
        registry: DistributedSessionRegistry,
        server: DistributedServerRuntime,
        server_machine: ProgramSession,
    }

    impl RegistryHarness {
        fn start(
            bundle: &DistributedProgramBundle,
            config: DistributedSessionRegistryConfig,
        ) -> Self {
            let server_artifact = bundle.artifact(ProgramRole::Server).unwrap();
            Self {
                registry: DistributedSessionRegistry::start(bundle, config).unwrap(),
                server: DistributedServerRuntime::start(server_artifact).unwrap(),
                server_machine: ProgramSession::start(server_artifact.clone()).unwrap(),
            }
        }

        fn identity(&self) -> DistributedSessionRegistryIdentity {
            self.registry.identity()
        }

        fn session_count(&self) -> usize {
            self.registry.session_count()
        }

        fn global_queued_bytes(&self) -> usize {
            self.registry.global_queued_bytes()
        }

        fn global_reserved_queue_bytes(&self) -> usize {
            self.registry.global_reserved_queue_bytes()
        }

        fn begin_handshake(
            &mut self,
            now: Duration,
            principal: SessionPrincipal,
            client_frame: &[u8],
        ) -> Result<DistributedSessionHandshakeStart, DistributedSessionRegistryError> {
            self.registry.begin_handshake(now, principal, client_frame)
        }

        fn commit_handshake(
            &mut self,
            now: Duration,
            connection_id: DistributedSessionConnectionId,
            client_frame: &[u8],
        ) -> Result<Vec<u8>, DistributedSessionRegistryError> {
            let mut server = self.server.bind(&mut self.server_machine);
            self.registry
                .commit_handshake(&mut server, now, connection_id, client_frame)
        }

        fn disconnect(
            &mut self,
            now: Duration,
            connection_id: DistributedSessionConnectionId,
        ) -> Result<(), DistributedSessionRegistryError> {
            let mut server = self.server.bind(&mut self.server_machine);
            self.registry.disconnect(&mut server, now, connection_id)
        }

        fn revoke(
            &mut self,
            connection_id: DistributedSessionConnectionId,
            client_frame: &[u8],
        ) -> Result<Vec<u8>, DistributedSessionRegistryError> {
            let mut server = self.server.bind(&mut self.server_machine);
            self.registry
                .revoke(&mut server, connection_id, client_frame)
        }

        fn admit_client_frame(
            &mut self,
            connection_id: DistributedSessionConnectionId,
            frame: &[u8],
        ) -> Result<(), DistributedSessionRegistryError> {
            self.registry.admit_client_frame(connection_id, frame)
        }

        fn session_root_value_current(
            &mut self,
            connection_id: DistributedSessionConnectionId,
            name: &str,
        ) -> Result<Value, DistributedSessionRegistryError> {
            self.registry
                .session_root_value_current(connection_id, name)
        }

        fn poll(
            &mut self,
            now: Duration,
            maximum_steps: usize,
        ) -> Result<DistributedSessionRegistryPoll, DistributedSessionRegistryError> {
            let mut server = self.server.bind(&mut self.server_machine);
            self.registry.poll(&mut server, now, maximum_steps)
        }

        fn route_server_delivery(
            &mut self,
            delivery: ServerDelivery,
        ) -> Result<(), DistributedSessionRegistryError> {
            self.registry.route_server_delivery(delivery)
        }
    }

    #[test]
    fn registry_struct_owns_no_server_runtime() {
        let RegistryHarness {
            registry,
            server,
            server_machine,
        } = harness(DistributedSessionRegistryConfig::default());
        let DistributedSessionRegistry {
            config: _,
            identity: _,
            session_template: _,
            slots: _,
            connections: _,
            pending_handshakes: _,
            resume_index: _,
            revoked_connections: _,
            slot_epochs: _,
            next_connection_id: _,
            next_execution_scope: _,
            last_now: _,
            round_robin_cursor: _,
            global_queued_bytes: _,
            global_reserved_queue_bytes: _,
            pending_session_turns: _,
            pending_server_turns: _,
            inject_server_publication_queue_pressure: _,
        } = registry;
        let _: DistributedServerRuntime = server;
        let _: ProgramSession = server_machine;
    }

    #[test]
    fn registry_candidates_fork_only_the_mutated_indexed_session_slab() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let _first = connect(&mut registry, Duration::ZERO);
        let _second = connect(&mut registry, Duration::ZERO);
        let slot_ids = registry.registry.slots.keys().copied().collect::<Vec<_>>();
        let mut candidate = registry.registry.fork_settled().unwrap();

        for slot_id in &slot_ids {
            assert!(
                registry.registry.slots[slot_id]
                    .runtime
                    .shares_settled_state_with(&candidate.slots[slot_id].runtime)
            );
        }

        candidate
            .slots
            .get_mut(&slot_ids[0])
            .unwrap()
            .runtime
            .get_mut()
            .unwrap();
        assert!(
            !registry.registry.slots[&slot_ids[0]]
                .runtime
                .shares_settled_state_with(&candidate.slots[&slot_ids[0]].runtime)
        );
        assert!(
            registry.registry.slots[&slot_ids[1]]
                .runtime
                .shares_settled_state_with(&candidate.slots[&slot_ids[1]].runtime)
        );
    }

    #[test]
    fn boon_server_program_owns_one_machine_for_two_distributed_sessions() {
        let mut program = crate::BoonServerProgram::new_distributed(
            bundle(),
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();
        let identity = program.distributed_identity().unwrap();

        for _ in 0..2 {
            let start = program
                .begin_distributed_handshake(
                    Duration::ZERO,
                    SessionPrincipal::Anonymous,
                    &hello(identity, None),
                )
                .unwrap();
            let offer = offered(start);
            let ready = program
                .commit_distributed_handshake(
                    Duration::ZERO,
                    offer.connection_id,
                    &offer.commit_frame(),
                )
                .unwrap();
            assert_eq!(ready_generation(&ready), 1);
        }

        assert_eq!(program.distributed_session_count(), Some(2));
    }

    #[test]
    fn persistent_boon_server_program_admits_distributed_turns_immediately() {
        let bundle = compile_bundle(
            CLIENT_SERVER_EFFECT,
            SESSION_SERVER_EFFECT,
            SERVER_EFFECT,
            "persistent-authority",
        );
        let (mut program, _) = crate::BoonServerProgram::with_distributed_persistence(
            &bundle,
            boon_persistence::InMemoryDriver::default(),
            crate::PersistentServerConfig {
                worker: boon_persistence::PersistenceWorkerConfig::default(),
                durability: crate::ServerDurabilityPolicy::AUTHORITATIVE,
            },
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();
        let identity = program.distributed_identity().unwrap();
        let start = program
            .begin_distributed_handshake(
                Duration::ZERO,
                SessionPrincipal::Anonymous,
                &hello(identity, None),
            )
            .unwrap();
        let offer = offered(start);
        program
            .commit_distributed_handshake(
                Duration::ZERO,
                offer.connection_id,
                &offer.commit_frame(),
            )
            .unwrap();
        let post_handshake = program.lifecycle_handle().unwrap().status();

        let mut client = DistributedClientRuntime::start(
            bundle.artifact(ProgramRole::Client).unwrap(),
            DistributedQueueLimits::default(),
        )
        .unwrap();
        client
            .bind(
                offer.session_id,
                offer.generation,
                offer.applied_client_through,
            )
            .unwrap();
        client.mark_current().unwrap();
        client
            .dispatch("store.read_clock", SourcePayload::default())
            .unwrap();
        for frame in take_client_session_frames(&mut client, 64) {
            program
                .admit_distributed_client_frame(offer.connection_id, &frame)
                .unwrap();
        }

        let mut observed_server_turn = false;
        for _ in 0..16 {
            let poll = program
                .poll_distributed_sessions(Duration::ZERO, 64)
                .unwrap();
            assert!(
                poll.poisoned_sessions.is_empty(),
                "post-handshake Session was poisoned: {}",
                poll.poisoned_sessions
                    .iter()
                    .map(|poisoned| poisoned.diagnostic.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            );
            observed_server_turn |= poll
                .server_turns
                .iter()
                .any(|(_, turn)| turn.source_sequence.is_some());
            for _ in 0..64 {
                let Some(frame) = program
                    .next_distributed_client_frame(offer.connection_id)
                    .unwrap()
                else {
                    break;
                };
                client.accept_session_frame(&frame).unwrap();
                assert!(
                    program
                        .acknowledge_distributed_client_frame(offer.connection_id)
                        .unwrap()
                );
            }
            if poll.serviced_connections.is_empty() && poll.serviced_origins.is_empty() {
                break;
            }
        }
        assert!(observed_server_turn);
        let lifecycle = program.lifecycle_handle().unwrap().status();
        let expected_turns = post_handshake.accepted_turns + 1;
        assert_eq!(lifecycle.accepted_turns, expected_turns);
        assert_eq!(lifecycle.durably_acknowledged_turns, expected_turns);
        assert!(lifecycle.last_error.is_none());
        program.shutdown_persistent().unwrap();
    }

    #[test]
    fn persistent_sessions_remain_process_local_and_restart_requires_a_fresh_session() {
        const CONNECTION_MARKER: u64 = 0xd1c2_b3a4_9586_7768;
        const EXECUTION_SCOPE_MARKER: u64 = 0xe1d2_c3b4_a596_8778;
        const ORIGIN_EPOCH_MARKER: u64 = 0xf1e2_d3c4_b5a6_9788;
        const TRANSPORT_GENERATION_MARKER: u64 = 0xc1b2_a394_8576_6758;

        let bundle = bundle();
        let shared = SharedPersistenceDriver::default();
        let (mut program, _) = crate::BoonServerProgram::with_distributed_persistence(
            bundle,
            shared.clone(),
            crate::PersistentServerConfig::authoritative(
                boon_persistence::PersistenceWorkerConfig::default(),
            ),
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();
        {
            let registry = program.distributed_sessions.as_mut().unwrap();
            registry.next_connection_id = CONNECTION_MARKER;
            registry.next_execution_scope = EXECUTION_SCOPE_MARKER;
            registry.slot_epochs.insert(0, ORIGIN_EPOCH_MARKER - 1);
        }
        let identity = program.distributed_identity().unwrap();
        let offer = offered(
            program
                .begin_distributed_handshake(
                    Duration::ZERO,
                    SessionPrincipal::Anonymous,
                    &hello(identity, None),
                )
                .unwrap(),
        );
        let token_bytes = *offer.resume_token.as_bytes();
        let session_id = offer.session_id;
        let session_id_bytes = *session_id.as_bytes();
        let resume_digest_bytes = resume_digest(&offer.resume_token);
        let token_storage = offer
            .resume_token
            .to_lookup_key()
            .as_storage_str()
            .to_owned();
        let token_bytes_debug = format!("{token_bytes:?}");
        let session_id_debug = format!("{:?}", session_id.as_bytes());
        program
            .commit_distributed_handshake(
                Duration::ZERO,
                offer.connection_id,
                &offer.commit_frame(),
            )
            .unwrap();
        program
            .distributed_sessions
            .as_mut()
            .unwrap()
            .slots
            .values_mut()
            .next()
            .unwrap()
            .transport_generation = TRANSPORT_GENERATION_MARKER;
        let lifecycle = program.lifecycle_handle().unwrap().status();
        assert_eq!(lifecycle.accepted_turns, 0);
        assert_eq!(lifecycle.durably_acknowledged_turns, 0);

        let persisted = shared.snapshot();
        let durable_image = persisted
            .image(bundle.artifact(ProgramRole::Server).unwrap().application())
            .expect("persistent Server image");
        let durable_bytes = boon_persistence::encode_restore_image(durable_image).unwrap();
        let session_value = {
            let registry = program.distributed_sessions.as_mut().unwrap();
            let slot = registry.slots.values_mut().next().unwrap();
            slot.runtime
                .get_mut()
                .unwrap()
                .root_value_current("store.count")
                .unwrap()
        };
        let observable_surfaces = format!(
            "{session_value:?}\n{lifecycle:?}\n{persisted:?}\n{:?}",
            program.last_diagnostic()
        );
        for secret in [
            token_storage,
            token_bytes_debug,
            session_id_debug,
            format!("{resume_digest_bytes:?}"),
            CONNECTION_MARKER.to_string(),
            EXECUTION_SCOPE_MARKER.to_string(),
            ORIGIN_EPOCH_MARKER.to_string(),
            TRANSPORT_GENERATION_MARKER.to_string(),
        ] {
            assert!(
                !observable_surfaces.contains(&secret),
                "host-private Session identity leaked into Boon state, lifecycle reports, or durable state: {secret}"
            );
        }
        for secret in [
            token_bytes.as_slice(),
            session_id_bytes.as_slice(),
            resume_digest_bytes.as_slice(),
            CONNECTION_MARKER.to_be_bytes().as_slice(),
            EXECUTION_SCOPE_MARKER.to_be_bytes().as_slice(),
            ORIGIN_EPOCH_MARKER.to_be_bytes().as_slice(),
            TRANSPORT_GENERATION_MARKER.to_be_bytes().as_slice(),
        ] {
            assert!(
                !durable_bytes
                    .windows(secret.len())
                    .any(|window| window == secret),
                "host-private Session bytes leaked into canonical durable state"
            );
        }
        program.shutdown_persistent().unwrap();

        let (mut restarted, _) = crate::BoonServerProgram::with_distributed_persistence(
            bundle,
            persisted,
            crate::PersistentServerConfig::authoritative(
                boon_persistence::PersistenceWorkerConfig::default(),
            ),
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();
        assert_eq!(restarted.distributed_session_count(), Some(0));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let rejected = restarted
            .begin_distributed_handshake(
                now,
                SessionPrincipal::Anonymous,
                &hello(
                    restarted.distributed_identity().unwrap(),
                    Some(ResumeToken::from_bytes(token_bytes)),
                ),
            )
            .unwrap();
        assert_rejected(
            rejected,
            DistributedSessionHandshakeRejectionReason::ResumeUnavailable,
        );
        let fresh = offered(
            restarted
                .begin_distributed_handshake(
                    now,
                    SessionPrincipal::Anonymous,
                    &hello(restarted.distributed_identity().unwrap(), None),
                )
                .unwrap(),
        );
        assert!(fresh.session_id != session_id);
        assert_eq!(fresh.generation, 1);
        let ready = restarted
            .commit_distributed_handshake(now, fresh.connection_id, &fresh.commit_frame())
            .unwrap();
        assert_eq!(ready_generation(&ready), 1);
        restarted.shutdown_persistent().unwrap();
    }

    #[test]
    fn process_local_disconnect_does_not_depend_on_persistence() {
        let bundle = bundle();
        let shared = SharedPersistenceDriver::default();
        let (mut program, _) = crate::BoonServerProgram::with_distributed_persistence(
            bundle,
            shared.clone(),
            crate::PersistentServerConfig::authoritative(
                boon_persistence::PersistenceWorkerConfig::default(),
            ),
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let offer = offered(
            program
                .begin_distributed_handshake(
                    now,
                    SessionPrincipal::Anonymous,
                    &hello(program.distributed_identity().unwrap(), None),
                )
                .unwrap(),
        );
        program
            .commit_distributed_handshake(now, offer.connection_id, &offer.commit_frame())
            .unwrap();
        shared.fail_after_commits(0);
        program
            .disconnect_distributed_session(now, offer.connection_id)
            .unwrap();
        assert_eq!(program.distributed_session_count(), Some(1));
    }

    #[test]
    fn restart_drops_process_owned_global_effect_without_resubmission() {
        let bundle = compile_bundle(
            CLIENT_GLOBAL_EFFECT,
            SESSION_GLOBAL_EFFECT,
            SERVER_GLOBAL_EFFECT,
            "global-effect-recovery",
        );
        let shared = SharedPersistenceDriver::default();
        let (mut program, _) = crate::BoonServerProgram::with_distributed_persistence(
            &bundle,
            shared.clone(),
            crate::PersistentServerConfig::authoritative(
                boon_persistence::PersistenceWorkerConfig::default(),
            ),
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();
        let started = program
            .dispatch_turn(
                "store.run",
                SourcePayload::default(),
                crate::ServerTurnClass::Http,
            )
            .unwrap();
        let [invocation] = started.runtime_turn.transient_effects.as_slice() else {
            panic!("global source must emit one transient effect");
        };
        let call_id = invocation.call_id;
        assert!(
            program
                .authority
                .machine
                .has_pending_transient_effect(call_id)
        );
        let persisted = shared.snapshot();
        drop(program);

        let (mut restarted, _) = crate::BoonServerProgram::with_distributed_persistence(
            &bundle,
            persisted,
            crate::PersistentServerConfig::authoritative(
                boon_persistence::PersistenceWorkerConfig::default(),
            ),
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();
        assert!(
            !restarted
                .authority
                .machine
                .has_pending_transient_effect(call_id)
        );
        assert!(
            !restarted
                .distributed_sessions
                .as_ref()
                .unwrap()
                .has_runnable_work()
        );
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let poll = restarted.poll_distributed_sessions(now, 1).unwrap();
        assert!(poll.server_turns.is_empty());
        assert!(poll.session_turns.is_empty());
        restarted.shutdown_persistent().unwrap();
    }

    #[test]
    fn fresh_handshake_uses_canonical_control_frames_and_activates_one_session() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let principal = SessionPrincipal::authenticated("person-42", ["operator"]).unwrap();
        let start = registry
            .begin_handshake(Duration::ZERO, principal, &hello(registry.identity(), None))
            .unwrap();
        let offer = offered(start);
        let ready = registry
            .commit_handshake(Duration::ZERO, offer.connection_id, &offer.commit_frame())
            .unwrap();
        assert_eq!(ready_generation(&ready), 1);
        assert_eq!(registry.session_count(), 1);
        assert_eq!(
            registry
                .session_root_value_current(offer.connection_id, "store.count")
                .unwrap(),
            Value::integer(0).unwrap()
        );
        assert_eq!(
            offer.resume_token.as_bytes().len(),
            boon_wire::RESUME_TOKEN_BYTES
        );
    }

    #[test]
    fn fresh_offer_is_control_only_until_commit_and_expires_on_its_short_deadline() {
        let config = DistributedSessionRegistryConfig {
            handshake_timeout: Duration::from_secs(2),
            ..DistributedSessionRegistryConfig::default()
        };
        let mut registry = harness(config);
        let start = registry
            .begin_handshake(
                Duration::from_secs(10),
                SessionPrincipal::Anonymous,
                &hello(registry.identity(), None),
            )
            .unwrap();
        let offer = offered(start);

        assert_eq!(registry.session_count(), 0);
        assert_eq!(registry.registry.pending_handshakes.len(), 1);
        assert_eq!(registry.global_reserved_queue_bytes(), 0);
        assert!(registry.registry.pending_session_turns.is_empty());
        assert!(registry.registry.pending_server_turns.is_empty());
        assert!(matches!(
            registry
                .server
                .bind(&mut registry.server_machine)
                .set_origin_status(
                    SessionOrigin::new(0, 1).unwrap(),
                    SessionConnectionStatus::Current
                ),
            Err(DistributedRuntimeError::InvalidLease)
        ));

        assert_eq!(
            registry
                .poll(Duration::from_secs(12), 0)
                .unwrap()
                .expired_sessions,
            0
        );
        assert!(registry.registry.pending_handshakes.is_empty());
        assert!(matches!(
            registry.commit_handshake(
                Duration::from_secs(12),
                offer.connection_id,
                &offer.commit_frame(),
            ),
            Err(DistributedSessionRegistryError::UnknownConnection)
        ));
        assert!(matches!(
            registry.begin_handshake(
                Duration::from_secs(11),
                SessionPrincipal::Anonymous,
                &hello(registry.identity(), None)
            ),
            Err(DistributedSessionRegistryError::TimeRegression)
        ));
    }

    #[test]
    fn two_tabs_share_one_server_authority_and_keep_session_state_isolated() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let mut first = connect(&mut registry, Duration::ZERO);
        let second = connect(&mut registry, Duration::ZERO);

        let origins = registry
            .registry
            .slots
            .values()
            .map(|slot| slot.origin)
            .collect::<Vec<_>>();
        assert_eq!(origins.len(), 2);
        for origin in origins {
            registry
                .server
                .bind(&mut registry.server_machine)
                .set_origin_status(origin, SessionConnectionStatus::Current)
                .unwrap();
        }

        dispatch_increment(&mut registry, &mut first);
        poll_until_idle(&mut registry, Duration::ZERO);

        assert_eq!(
            registry
                .session_root_value_current(first.connection_id, "store.count")
                .unwrap(),
            Value::integer(1).unwrap()
        );
        assert_eq!(
            registry
                .session_root_value_current(second.connection_id, "store.count")
                .unwrap(),
            Value::integer(0).unwrap()
        );
    }

    #[test]
    fn detach_and_resume_within_window_rotate_token_and_increment_generation() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let mut client = connect(&mut registry, Duration::from_secs(5));
        dispatch_increment(&mut registry, &mut client);
        poll_until_idle(&mut registry, Duration::from_secs(5));
        let old_token_bytes = *client.resume_token.as_bytes();

        registry
            .disconnect(Duration::from_secs(5), client.connection_id)
            .unwrap();
        let start = registry
            .begin_handshake(
                Duration::from_secs(64),
                SessionPrincipal::Anonymous,
                &hello(registry.identity(), Some(client.resume_token)),
            )
            .unwrap();
        let resumed_offer = offered(start);
        assert_ne!(resumed_offer.resume_token.as_bytes(), &old_token_bytes);
        let ready = registry
            .commit_handshake(
                Duration::from_secs(64),
                resumed_offer.connection_id,
                &resumed_offer.commit_frame(),
            )
            .unwrap();
        assert_eq!(ready_generation(&ready), client.generation + 1);
        assert_eq!(
            registry
                .session_root_value_current(resumed_offer.connection_id, "store.count")
                .unwrap(),
            Value::integer(1).unwrap()
        );

        let replay = registry
            .begin_handshake(
                Duration::from_secs(64),
                SessionPrincipal::Anonymous,
                &hello(
                    registry.identity(),
                    Some(ResumeToken::from_bytes(old_token_bytes)),
                ),
            )
            .unwrap();
        assert_rejected(
            replay,
            DistributedSessionHandshakeRejectionReason::ResumeUnavailable,
        );
    }

    #[test]
    fn failed_resume_publication_preserves_exact_generation_for_identical_retry() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let client = connect(&mut registry, Duration::ZERO);
        let original_token = *client.resume_token.as_bytes();
        registry
            .disconnect(Duration::ZERO, client.connection_id)
            .unwrap();
        let (slot_id, runtime_before) = registry
            .registry
            .slots
            .iter()
            .map(|(slot_id, slot)| (*slot_id, slot.runtime.clone()))
            .next()
            .unwrap();

        let failed_offer = offered(
            registry
                .begin_handshake(
                    Duration::from_secs(1),
                    SessionPrincipal::Anonymous,
                    &hello(
                        registry.identity(),
                        Some(ResumeToken::from_bytes(original_token)),
                    ),
                )
                .unwrap(),
        );
        assert_eq!(failed_offer.generation, client.generation + 1);
        let failed_rotated_token = *failed_offer.resume_token.as_bytes();
        assert_ne!(failed_rotated_token, original_token);

        let queue_limit = registry.registry.config.session_queue_limits.max_messages;
        registry
            .registry
            .inject_server_publication_queue_pressure_for_test();
        assert!(matches!(
            registry.commit_handshake(
                Duration::from_secs(1),
                failed_offer.connection_id,
                &failed_offer.commit_frame(),
            ),
            Err(DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::QueueFull { limit }
            )) if limit == queue_limit
        ));

        let slot = &registry.registry.slots[&slot_id];
        assert!(slot.runtime.shares_settled_state_with(&runtime_before));
        assert_eq!(slot.runtime.transport_generation(), client.generation);
        assert_eq!(slot.transport_generation, client.generation);
        assert!(matches!(
            slot.state,
            SessionSlotState::Stale { cleanup: None, .. }
        ));
        assert_eq!(
            registry
                .registry
                .resume_index
                .get(&resume_digest(&ResumeToken::from_bytes(original_token))),
            Some(&slot_id)
        );
        assert!(!registry.registry.resume_index.contains_key(&resume_digest(
            &ResumeToken::from_bytes(failed_rotated_token)
        )));

        let retry_offer = offered(
            registry
                .begin_handshake(
                    Duration::from_secs(2),
                    SessionPrincipal::Anonymous,
                    &hello(
                        registry.identity(),
                        Some(ResumeToken::from_bytes(original_token)),
                    ),
                )
                .unwrap(),
        );
        assert!(retry_offer.session_id == failed_offer.session_id);
        assert_eq!(retry_offer.generation, failed_offer.generation);
        assert_ne!(retry_offer.resume_token.as_bytes(), &original_token);
        let ready = registry
            .commit_handshake(
                Duration::from_secs(2),
                retry_offer.connection_id,
                &retry_offer.commit_frame(),
            )
            .unwrap();
        assert_eq!(ready_generation(&ready), client.generation + 1);
        assert_eq!(
            registry.registry.slots[&slot_id]
                .runtime
                .transport_generation(),
            client.generation + 1
        );
    }

    #[test]
    fn resume_token_is_bound_to_the_authenticated_principal() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let alice = SessionPrincipal::authenticated("alice", ["member"]).unwrap();
        let bob = SessionPrincipal::authenticated("bob", ["member"]).unwrap();
        let start = registry
            .begin_handshake(Duration::ZERO, alice, &hello(registry.identity(), None))
            .unwrap();
        let offer = offered(start);
        registry
            .commit_handshake(Duration::ZERO, offer.connection_id, &offer.commit_frame())
            .unwrap();
        registry
            .disconnect(Duration::ZERO, offer.connection_id)
            .unwrap();

        let rejected = registry
            .begin_handshake(
                Duration::from_secs(1),
                bob,
                &hello(registry.identity(), Some(offer.resume_token)),
            )
            .unwrap();
        assert_rejected(
            rejected,
            DistributedSessionHandshakeRejectionReason::ResumeUnavailable,
        );
        assert_eq!(registry.session_count(), 1);
    }

    #[test]
    fn resumed_session_rejects_frames_from_the_previous_generation() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let mut client = connect(&mut registry, Duration::ZERO);
        client
            .runtime
            .dispatch("store.increment", SourcePayload::default())
            .unwrap();
        let frames = take_client_session_frames(&mut client.runtime, 64);
        let [stale_frame] = frames.as_slice() else {
            panic!("expected one previous-generation Client frame");
        };
        let stale_frame = stale_frame.clone();

        registry
            .disconnect(Duration::ZERO, client.connection_id)
            .unwrap();
        let start = registry
            .begin_handshake(
                Duration::from_secs(1),
                SessionPrincipal::Anonymous,
                &hello(registry.identity(), Some(client.resume_token)),
            )
            .unwrap();
        let offer = offered(start);
        let ready = registry
            .commit_handshake(
                Duration::from_secs(1),
                offer.connection_id,
                &offer.commit_frame(),
            )
            .unwrap();
        assert_eq!(ready_generation(&ready), client.generation + 1);
        registry
            .admit_client_frame(offer.connection_id, &stale_frame)
            .unwrap();
        let poll = registry.poll(Duration::from_secs(1), 1).unwrap();
        let [poisoned] = poll.poisoned_sessions.as_slice() else {
            panic!("stale generation must poison exactly its resumed Session");
        };
        assert_eq!(poisoned.connection_id, Some(offer.connection_id));
        assert!(poisoned.diagnostic.contains("transport frame is stale"));
        assert_eq!(registry.session_count(), 0);
    }

    #[test]
    fn disconnect_cleans_fresh_handshake_and_rolls_interrupted_resume_back() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let fresh = registry
            .begin_handshake(
                Duration::ZERO,
                SessionPrincipal::Anonymous,
                &hello(registry.identity(), None),
            )
            .unwrap();
        let fresh_offer = offered(fresh);
        registry
            .disconnect(Duration::ZERO, fresh_offer.connection_id)
            .unwrap();
        assert_eq!(registry.session_count(), 0);

        let client = connect(&mut registry, Duration::from_secs(1));
        let old_token = *client.resume_token.as_bytes();
        registry
            .disconnect(Duration::from_secs(1), client.connection_id)
            .unwrap();
        let pending = registry
            .begin_handshake(
                Duration::from_secs(2),
                SessionPrincipal::Anonymous,
                &hello(
                    registry.identity(),
                    Some(ResumeToken::from_bytes(old_token)),
                ),
            )
            .unwrap();
        let pending_offer = offered(pending);
        registry
            .disconnect(Duration::from_secs(2), pending_offer.connection_id)
            .unwrap();

        let retry = registry
            .begin_handshake(
                Duration::from_secs(3),
                SessionPrincipal::Anonymous,
                &hello(
                    registry.identity(),
                    Some(ResumeToken::from_bytes(old_token)),
                ),
            )
            .unwrap();
        let retry_offer = offered(retry);
        let ready = registry
            .commit_handshake(
                Duration::from_secs(3),
                retry_offer.connection_id,
                &retry_offer.commit_frame(),
            )
            .unwrap();
        assert_eq!(ready_generation(&ready), 2);
        assert_eq!(registry.session_count(), 1);
    }

    #[test]
    fn expiration_at_exact_boundary_rejects_resume_and_schedules_bounded_origin_removal() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let client = connect(&mut registry, Duration::ZERO);
        let origin = registry.registry.slots.values().next().unwrap().origin;
        registry
            .disconnect(Duration::ZERO, client.connection_id)
            .unwrap();

        let start = registry
            .begin_handshake(
                DEFAULT_SESSION_RESUME_WINDOW,
                SessionPrincipal::Anonymous,
                &hello(registry.identity(), Some(client.resume_token)),
            )
            .unwrap();
        assert_rejected(
            start,
            DistributedSessionHandshakeRejectionReason::ResumeUnavailable,
        );
        assert_eq!(
            registry.session_count(),
            1,
            "handshake admission must not run Session cleanup outside the scheduler budget"
        );
        let transition = registry.poll(DEFAULT_SESSION_RESUME_WINDOW, 1).unwrap();
        assert_eq!(transition.serviced_origins, vec![origin]);
        assert_eq!(transition.expired_sessions, 0);
        assert_eq!(registry.session_count(), 1);
        let removal = registry.poll(DEFAULT_SESSION_RESUME_WINDOW, 1).unwrap();
        assert_eq!(removal.serviced_origins, vec![origin]);
        assert_eq!(removal.expired_sessions, 1);
        assert_eq!(registry.session_count(), 0);
        assert!(matches!(
            registry
                .server
                .bind(&mut registry.server_machine)
                .set_origin_status(origin, SessionConnectionStatus::Current),
            Err(DistributedRuntimeError::InvalidLease)
        ));
    }

    #[test]
    fn resume_is_allowed_one_millisecond_before_the_fixed_boundary() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let client = connect(&mut registry, Duration::ZERO);
        registry
            .disconnect(Duration::ZERO, client.connection_id)
            .unwrap();
        assert_eq!(
            registry.registry.next_deadline(),
            Some(DEFAULT_SESSION_RESUME_WINDOW)
        );

        let before_boundary = DEFAULT_SESSION_RESUME_WINDOW - Duration::from_millis(1);
        let start = registry
            .begin_handshake(
                before_boundary,
                SessionPrincipal::Anonymous,
                &hello(registry.identity(), Some(client.resume_token)),
            )
            .unwrap();
        let offer = offered(start);
        let ready = registry
            .commit_handshake(before_boundary, offer.connection_id, &offer.commit_frame())
            .unwrap();
        assert_eq!(ready_generation(&ready), client.generation + 1);
        assert_eq!(registry.registry.next_deadline(), None);
    }

    #[test]
    fn expiry_is_bounded_fair_and_tolerates_already_removed_origins() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let clients = (0..3)
            .map(|_| connect(&mut registry, Duration::ZERO))
            .collect::<Vec<_>>();
        for client in &clients {
            registry
                .disconnect(Duration::ZERO, client.connection_id)
                .unwrap();
        }
        let origins = registry
            .registry
            .slots
            .values()
            .map(|slot| slot.origin)
            .collect::<Vec<_>>();

        registry
            .server
            .bind(&mut registry.server_machine)
            .expire_origin(origins[0])
            .unwrap();
        let first = registry.poll(DEFAULT_SESSION_RESUME_WINDOW, 1).unwrap();
        assert_eq!(first.serviced_origins.len(), 1);
        assert_eq!(first.expired_sessions, 0);
        assert_eq!(
            registry.session_count(),
            3,
            "one scheduler step may start only one expiry cleanup"
        );

        let mut expired = 0usize;
        let mut service_order = first.serviced_origins;
        for _ in 0..8 {
            let poll = registry.poll(DEFAULT_SESSION_RESUME_WINDOW, 1).unwrap();
            assert!(poll.serviced_origins.len() <= 1);
            expired += poll.expired_sessions;
            service_order.extend(poll.serviced_origins);
            if registry.session_count() == 0 {
                break;
            }
        }
        assert_eq!(expired, 3);
        assert_eq!(
            &service_order[..3],
            origins.as_slice(),
            "each due Session must receive one cleanup transition before any repeats"
        );
        assert_eq!(registry.session_count(), 0);
        assert!(registry.registry.resume_index.is_empty());
        assert_eq!(registry.registry.next_deadline(), None);
        for origin in origins {
            assert!(matches!(
                registry
                    .server
                    .bind(&mut registry.server_machine)
                    .set_origin_status(origin, SessionConnectionStatus::Current),
                Err(DistributedRuntimeError::InvalidLease)
            ));
        }
        for client in clients {
            let start = registry
                .begin_handshake(
                    DEFAULT_SESSION_RESUME_WINDOW,
                    SessionPrincipal::Anonymous,
                    &hello(registry.identity(), Some(client.resume_token)),
                )
                .unwrap();
            assert_rejected(
                start,
                DistributedSessionHandshakeRejectionReason::ResumeUnavailable,
            );
        }
    }

    #[test]
    fn revoke_drops_session_and_invalidates_resume_authority() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let client = connect(&mut registry, Duration::ZERO);
        let acknowledgement = registry
            .revoke(client.connection_id, &revoke_frame())
            .unwrap();
        assert!(matches!(
            decode_session_control_frame(&acknowledgement).unwrap(),
            SessionControlFrame::ServerRevoked(_)
        ));
        let duplicate = registry
            .revoke(client.connection_id, &revoke_frame())
            .unwrap();
        assert!(matches!(
            decode_session_control_frame(&duplicate).unwrap(),
            SessionControlFrame::ServerRevoked(_)
        ));
        assert_eq!(registry.session_count(), 0);

        let start = registry
            .begin_handshake(
                Duration::ZERO,
                SessionPrincipal::Anonymous,
                &hello(registry.identity(), Some(client.resume_token)),
            )
            .unwrap();
        assert_rejected(
            start,
            DistributedSessionHandshakeRejectionReason::ResumeUnavailable,
        );
    }

    #[test]
    fn session_capacity_and_global_admission_bytes_are_hard_bounds() {
        let queue_limits = DistributedQueueLimits {
            max_messages: 64,
            max_bytes: 1024,
        };
        let queue_reservation = queue_limits.max_bytes * QUEUE_LANES_PER_SESSION;
        let defaults = DistributedSessionRegistryConfig::default();
        assert_eq!(
            defaults.max_sessions * queue_reservation_per_session(defaults).unwrap(),
            defaults.max_global_queued_bytes
        );
        let session_capacity_config = DistributedSessionRegistryConfig {
            max_sessions: 1,
            max_global_queued_bytes: queue_reservation * 2,
            session_queue_limits: queue_limits,
            ..DistributedSessionRegistryConfig::default()
        };
        let mut session_bounded = harness(session_capacity_config);
        let client = connect(&mut session_bounded, Duration::ZERO);

        let second = session_bounded
            .begin_handshake(
                Duration::ZERO,
                SessionPrincipal::Anonymous,
                &hello(session_bounded.identity(), None),
            )
            .unwrap();
        assert_rejected(second, DistributedSessionHandshakeRejectionReason::Capacity);

        let oversized = vec![0; queue_limits.max_bytes + 1];
        assert!(matches!(
            session_bounded.admit_client_frame(client.connection_id, &oversized),
            Err(DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::QueueBytesFull { limit }
            )) if limit == queue_limits.max_bytes
        ));
        assert_eq!(session_bounded.global_queued_bytes(), 0);

        let global_capacity_config = DistributedSessionRegistryConfig {
            max_sessions: 2,
            max_global_queued_bytes: queue_reservation,
            session_queue_limits: queue_limits,
            ..DistributedSessionRegistryConfig::default()
        };
        let mut globally_bounded = harness(global_capacity_config);
        let _first = connect(&mut globally_bounded, Duration::ZERO);
        assert_eq!(
            globally_bounded.global_reserved_queue_bytes(),
            queue_reservation
        );
        let second = globally_bounded
            .begin_handshake(
                Duration::ZERO,
                SessionPrincipal::Anonymous,
                &hello(globally_bounded.identity(), None),
            )
            .unwrap();
        let offer = offered(second);
        assert!(matches!(
            globally_bounded.commit_handshake(
                Duration::ZERO,
                offer.connection_id,
                &offer.commit_frame()
            ),
            Err(DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::QueueBytesFull { limit }
            )) if limit == queue_reservation
        ));
        assert_eq!(
            globally_bounded.global_reserved_queue_bytes(),
            queue_reservation
        );
    }

    #[test]
    fn polling_is_round_robin_across_busy_sessions() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let mut first = connect(&mut registry, Duration::ZERO);
        let mut second = connect(&mut registry, Duration::ZERO);
        for _ in 0..2 {
            queue_increment(&mut registry, &mut first);
            queue_increment(&mut registry, &mut second);
        }

        let poll = registry.poll(Duration::ZERO, 4).unwrap();
        assert_eq!(poll.serviced_connections.len(), 4);
        assert_ne!(poll.serviced_connections[0], poll.serviced_connections[1]);
        assert_eq!(poll.serviced_connections[0], poll.serviced_connections[2]);
        assert_eq!(poll.serviced_connections[1], poll.serviced_connections[3]);
    }

    #[test]
    fn one_saturated_session_cannot_starve_seven_quiet_sessions() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let mut clients = (0..8)
            .map(|_| connect(&mut registry, Duration::ZERO))
            .collect::<Vec<_>>();

        for _ in 0..16 {
            queue_increment(&mut registry, &mut clients[0]);
        }
        for client in &mut clients[1..] {
            queue_increment(&mut registry, client);
        }

        let poll = registry.poll(Duration::ZERO, clients.len()).unwrap();
        assert_eq!(poll.serviced_connections.len(), clients.len());
        let mut unique = poll.serviced_connections.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            clients.len(),
            "every runnable Session must receive one service step before the saturated Session repeats"
        );
    }

    #[test]
    fn session_server_backpressure_retries_exactly_once_after_delivery_lane_progress() {
        let counter_bundle = compile_bundle(
            CLIENT_SOURCE,
            SESSION_SERVER_COUNTER,
            SERVER_COUNTER,
            "prepared-backpressure",
        );
        let mut registry =
            RegistryHarness::start(&counter_bundle, DistributedSessionRegistryConfig::default());
        let mut client = connect_with_bundle(&mut registry, &counter_bundle, Duration::ZERO);
        let (slot_id, origin) = registry
            .registry
            .slots
            .iter()
            .map(|(slot_id, slot)| (*slot_id, slot.origin))
            .next()
            .unwrap();

        queue_increment(&mut registry, &mut client);
        registry.poll(Duration::ZERO, 1).unwrap();
        assert_eq!(
            registry
                .registry
                .slots
                .get(&slot_id)
                .unwrap()
                .runtime
                .pending_server_messages(),
            1
        );

        let shared_edge = counter_bundle
            .artifact(ProgramRole::Server)
            .unwrap()
            .plan()
            .distributed_endpoint
            .as_ref()
            .unwrap()
            .wire_schema
            .value_edges
            .iter()
            .find(|edge| {
                edge.producer_role == ProgramRole::Server
                    && edge.consumer_role == ProgramRole::Session
                    && edge.scope == boon_plan::DistributedRouteScopePlan::SharedSubscription
            })
            .unwrap();
        registry.registry.config.session_queue_limits.max_messages = 1;
        registry
            .route_server_delivery(ServerDelivery {
                target: ServerDeliveryTarget::Origin(origin),
                message: DistributedMessage {
                    producer: ProgramRole::Server,
                    consumer: ProgramRole::Session,
                    payload: DistributedMessagePayload::Current {
                        export_id: shared_edge.export_id,
                        revision: 2,
                        value: DataValue::Bool(true),
                    },
                },
            })
            .unwrap();
        registry.registry.slots.get_mut(&slot_id).unwrap().next_lane = 2;

        let blocked = registry.poll(Duration::ZERO, 1).unwrap();
        assert_eq!(blocked.backpressured_origins, vec![origin]);
        assert_eq!(
            registry
                .server
                .bind(&mut registry.server_machine)
                .root_value_current(origin, "store.count")
                .unwrap(),
            Value::integer(0).unwrap()
        );
        assert_eq!(
            registry
                .registry
                .slots
                .get(&slot_id)
                .unwrap()
                .runtime
                .pending_server_messages(),
            1
        );

        let released = registry.poll(Duration::ZERO, 1).unwrap();
        assert_eq!(released.serviced_origins, vec![origin]);
        let committed = registry.poll(Duration::ZERO, 1).unwrap();
        assert_eq!(committed.serviced_origins, vec![origin]);
        assert!(committed.backpressured_origins.is_empty());
        assert_eq!(
            registry
                .server
                .bind(&mut registry.server_machine)
                .root_value_current(origin, "store.count")
                .unwrap(),
            Value::integer(1).unwrap()
        );
        assert_eq!(
            registry
                .registry
                .slots
                .get(&slot_id)
                .unwrap()
                .runtime
                .pending_server_messages(),
            0
        );
    }

    #[test]
    fn stale_session_keeps_current_server_snapshot_without_a_client_connection() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let client = connect(&mut registry, Duration::ZERO);
        let (slot_id, origin) = registry
            .registry
            .slots
            .iter()
            .map(|(slot_id, slot)| (*slot_id, slot.origin))
            .next()
            .unwrap();
        registry
            .disconnect(Duration::ZERO, client.connection_id)
            .unwrap();
        let edge = bundle()
            .artifact(ProgramRole::Server)
            .unwrap()
            .plan()
            .distributed_endpoint
            .as_ref()
            .unwrap()
            .wire_schema
            .value_edges
            .iter()
            .find(|edge| {
                edge.producer_role == ProgramRole::Server
                    && edge.consumer_role == ProgramRole::Session
            })
            .unwrap();
        registry
            .route_server_delivery(ServerDelivery {
                target: ServerDeliveryTarget::Origin(origin),
                message: DistributedMessage {
                    producer: ProgramRole::Server,
                    consumer: ProgramRole::Session,
                    payload: DistributedMessagePayload::Current {
                        export_id: edge.export_id,
                        revision: 2,
                        value: DataValue::Bool(false),
                    },
                },
            })
            .unwrap();

        let poll = registry.poll(Duration::from_secs(1), 1).unwrap();
        assert_eq!(poll.serviced_origins.len(), 1);
        assert!(poll.serviced_origins[0] == origin);
        assert!(poll.serviced_connections.is_empty());
        assert_eq!(
            registry
                .registry
                .slots
                .get_mut(&slot_id)
                .unwrap()
                .runtime
                .get_mut()
                .unwrap()
                .root_value_current("store.server_ready")
                .unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn stale_session_drops_server_events_instead_of_replaying_them_on_resume() {
        let event_bundle = compile_bundle(
            CLIENT_SOURCE,
            r#"
store: [
    pulse: Server/store.pulse
    pulse_count:
        0 |> HOLD pulse_count {
            pulse |> THEN { pulse_count + 1 }
        }
]
"#,
            r#"
store: [
    pulse: SOURCE
]
"#,
            "stale-event",
        );
        let mut registry =
            RegistryHarness::start(&event_bundle, DistributedSessionRegistryConfig::default());
        let client = connect_with_bundle(&mut registry, &event_bundle, Duration::ZERO);
        let (slot_id, origin) = registry
            .registry
            .slots
            .iter()
            .map(|(slot_id, slot)| (*slot_id, slot.origin))
            .next()
            .unwrap();
        let export_id = event_bundle
            .artifact(ProgramRole::Server)
            .unwrap()
            .plan()
            .distributed_endpoint
            .as_ref()
            .unwrap()
            .wire_schema
            .event_edges
            .iter()
            .find(|edge| {
                edge.producer_role == ProgramRole::Server
                    && edge.consumer_role == ProgramRole::Session
            })
            .unwrap()
            .export_id;

        registry
            .route_server_delivery(ServerDelivery {
                target: ServerDeliveryTarget::Origin(origin),
                message: DistributedMessage {
                    producer: ProgramRole::Server,
                    consumer: ProgramRole::Session,
                    payload: DistributedMessagePayload::Event {
                        export_id,
                        sequence: 1,
                        value: DataValue::Null,
                    },
                },
            })
            .unwrap();
        assert_eq!(
            registry.registry.slots[&slot_id]
                .pending_server_messages
                .len(),
            1
        );

        registry
            .disconnect(Duration::ZERO, client.connection_id)
            .unwrap();
        registry
            .route_server_delivery(ServerDelivery {
                target: ServerDeliveryTarget::Origin(origin),
                message: DistributedMessage {
                    producer: ProgramRole::Server,
                    consumer: ProgramRole::Session,
                    payload: DistributedMessagePayload::Event {
                        export_id,
                        sequence: 2,
                        value: DataValue::Null,
                    },
                },
            })
            .unwrap();

        assert!(
            registry.registry.slots[&slot_id]
                .pending_server_messages
                .is_empty()
        );
        assert_eq!(registry.registry.global_queued_bytes(), 0);
        assert_eq!(
            registry
                .registry
                .slots
                .get_mut(&slot_id)
                .unwrap()
                .runtime
                .get_mut()
                .unwrap()
                .root_value_current("store.pulse_count")
                .unwrap(),
            Value::integer(0).unwrap()
        );
    }

    #[test]
    fn malformed_client_frame_removes_only_its_session() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let first = connect(&mut registry, Duration::ZERO);
        let second = connect(&mut registry, Duration::ZERO);

        registry
            .admit_client_frame(first.connection_id, &[0xff])
            .unwrap();
        let poll = registry.poll(Duration::ZERO, 1).unwrap();

        assert_eq!(poll.poisoned_sessions.len(), 1);
        assert_eq!(
            poll.poisoned_sessions[0].connection_id,
            Some(first.connection_id)
        );
        assert!(!poll.poisoned_sessions[0].diagnostic.is_empty());
        assert_eq!(registry.registry.session_count(), 1);
        assert_eq!(
            registry
                .registry
                .pending_client_frames(second.connection_id)
                .unwrap(),
            0
        );
    }

    #[test]
    fn pending_server_queue_latest_wins_pure_work_and_keeps_events_fifo() {
        use boon_plan::{DistributedCallInstanceId, ExportId, RemoteCallSiteId};

        let limits = DistributedQueueLimits::default();
        let export_id = ExportId([7; 32]);
        let call_site_id = RemoteCallSiteId([9; 32]);
        let call_instance_id = DistributedCallInstanceId::from_rows(call_site_id, &[]).unwrap();
        let current = |revision| DistributedMessage {
            producer: ProgramRole::Server,
            consumer: ProgramRole::Session,
            payload: DistributedMessagePayload::Current {
                export_id,
                revision,
                value: DataValue::integer(revision as i64).unwrap(),
            },
        };
        let call = |revision| DistributedMessage {
            producer: ProgramRole::Server,
            consumer: ProgramRole::Session,
            payload: DistributedMessagePayload::CurrentCallResult {
                call_site_id,
                call_instance_id,
                demand_revision: 1,
                result_revision: revision,
                value: DataValue::integer(revision as i64).unwrap(),
            },
        };
        let call_request = |revision| DistributedMessage {
            producer: ProgramRole::Server,
            consumer: ProgramRole::Session,
            payload: DistributedMessagePayload::CurrentCallRequest {
                call_site_id,
                call_instance_id,
                function_export_id: export_id,
                demand_revision: revision,
                arguments: BTreeMap::new(),
            },
        };
        let event = |sequence| DistributedMessage {
            producer: ProgramRole::Server,
            consumer: ProgramRole::Session,
            payload: DistributedMessagePayload::Event {
                export_id,
                sequence,
                value: DataValue::Null,
            },
        };

        let (queue, _) = candidate_server_queue(&VecDeque::new(), current(1), limits).unwrap();
        let (queue, _) = candidate_server_queue(&queue, current(2), limits).unwrap();
        assert_eq!(queue.len(), 1);
        assert!(matches!(
            queue.front().unwrap().payload,
            DistributedMessagePayload::Current { revision: 2, .. }
        ));

        let (queue, _) = candidate_server_queue(&VecDeque::new(), call(1), limits).unwrap();
        let (queue, _) = candidate_server_queue(&queue, call(2), limits).unwrap();
        assert_eq!(queue.len(), 1);
        assert!(matches!(
            queue.front().unwrap().payload,
            DistributedMessagePayload::CurrentCallResult {
                demand_revision: 1,
                result_revision: 2,
                ..
            }
        ));

        let (queue, _) = candidate_server_queue(&VecDeque::new(), call_request(1), limits).unwrap();
        let (queue, _) = candidate_server_queue(&queue, call_request(2), limits).unwrap();
        assert_eq!(queue.len(), 1);
        assert!(matches!(
            queue.front().unwrap().payload,
            DistributedMessagePayload::CurrentCallRequest {
                demand_revision: 2,
                ..
            }
        ));

        let (queue, _) = candidate_server_queue(&VecDeque::new(), event(1), limits).unwrap();
        let (queue, _) = candidate_server_queue(&queue, event(2), limits).unwrap();
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn server_delivery_preparation_is_atomic_under_queue_pressure() {
        use boon_plan::ExportId;

        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let _client = connect(&mut registry, Duration::ZERO);
        let (slot_id, origin) = registry
            .registry
            .slots
            .iter()
            .map(|(slot_id, slot)| (*slot_id, slot.origin))
            .next()
            .unwrap();
        assert!(
            registry
                .registry
                .slots
                .get(&slot_id)
                .unwrap()
                .pending_server_messages
                .is_empty()
        );
        registry.registry.config.session_queue_limits.max_messages = 1;
        let delivery = |byte, value| ServerDelivery {
            target: ServerDeliveryTarget::Origin(origin),
            message: DistributedMessage {
                producer: ProgramRole::Server,
                consumer: ProgramRole::Session,
                payload: DistributedMessagePayload::Current {
                    export_id: ExportId([byte; 32]),
                    revision: 1,
                    value: DataValue::integer(value).unwrap(),
                },
            },
        };
        let error = registry
            .registry
            .route_server_update(
                origin,
                DistributedServerUpdate {
                    turns: Vec::new(),
                    deliveries: vec![delivery(1, 10), delivery(2, 20)],
                },
            )
            .unwrap_err();

        assert!(matches!(
            error,
            DistributedSessionRegistryError::Runtime(DistributedRuntimeError::QueueFull {
                limit: 1
            })
        ));
        assert!(
            registry
                .registry
                .slots
                .get(&slot_id)
                .unwrap()
                .pending_server_messages
                .is_empty()
        );
        registry.registry.config.session_queue_limits.max_messages = 2;
        registry
            .registry
            .route_server_update(
                origin,
                DistributedServerUpdate {
                    turns: Vec::new(),
                    deliveries: vec![delivery(1, 10), delivery(2, 20)],
                },
            )
            .unwrap();
        assert_eq!(
            registry
                .registry
                .slots
                .get(&slot_id)
                .unwrap()
                .pending_server_messages
                .len(),
            2
        );
    }

    #[test]
    fn detach_cancels_session_and_server_effects_and_preserves_cancellation_turns() {
        let session_effect_bundle = compile_bundle(
            CLIENT_SESSION_EFFECT,
            SESSION_EFFECT,
            SERVER_SOURCE,
            "session-effect",
        );
        let mut session_registry = RegistryHarness::start(
            &session_effect_bundle,
            DistributedSessionRegistryConfig::default(),
        );
        let mut session_client = connect_with_bundle(
            &mut session_registry,
            &session_effect_bundle,
            Duration::ZERO,
        );
        queue_source(
            &mut session_registry,
            &mut session_client,
            "store.read_clock",
        );
        let effect_turn = poll_collect(&mut session_registry, Duration::ZERO);
        let session_call = effect_turn
            .session_turns
            .iter()
            .flat_map(|(_, turn)| &turn.transient_effects)
            .next()
            .expect("Session effect invocation")
            .call_id;
        assert_eq!(
            session_registry
                .registry
                .slots
                .values()
                .next()
                .unwrap()
                .runtime
                .pending_transient_effect_count(),
            1
        );
        session_registry
            .disconnect(Duration::ZERO, session_client.connection_id)
            .unwrap();
        let cancellation = session_registry.poll(Duration::ZERO, 0).unwrap();
        assert!(
            cancellation
                .session_turns
                .iter()
                .any(|(_, turn)| { turn.cancelled_transient_effects.contains(&session_call) })
        );
        assert_eq!(
            session_registry
                .registry
                .slots
                .values()
                .next()
                .unwrap()
                .runtime
                .pending_transient_effect_count(),
            0
        );

        let server_effect_bundle = compile_bundle(
            CLIENT_SERVER_EFFECT,
            SESSION_SERVER_EFFECT,
            SERVER_EFFECT,
            "server-effect",
        );
        let mut server_registry = RegistryHarness::start(
            &server_effect_bundle,
            DistributedSessionRegistryConfig::default(),
        );
        let mut server_client =
            connect_with_bundle(&mut server_registry, &server_effect_bundle, Duration::ZERO);
        queue_source(&mut server_registry, &mut server_client, "store.read_clock");
        let effect_turn = poll_collect(&mut server_registry, Duration::ZERO);
        let server_call = effect_turn
            .server_turns
            .iter()
            .flat_map(|(_, turn)| &turn.transient_effects)
            .next()
            .expect("Server effect invocation")
            .call_id;
        let origin = server_registry
            .registry
            .slots
            .values()
            .next()
            .unwrap()
            .origin;
        assert_eq!(
            server_registry
                .server
                .bind(&mut server_registry.server_machine)
                .pending_transient_effect_count(origin),
            1
        );
        server_registry
            .disconnect(Duration::ZERO, server_client.connection_id)
            .unwrap();
        let cancellation = server_registry.poll(Duration::ZERO, 0).unwrap();
        assert!(
            cancellation
                .server_turns
                .iter()
                .any(|(_, turn)| turn.cancelled_transient_effects.contains(&server_call))
        );
        assert_eq!(
            server_registry
                .server
                .bind(&mut server_registry.server_machine)
                .pending_transient_effect_count(origin),
            0
        );
    }

    #[test]
    fn schema_mismatch_returns_canonical_rejection_without_allocating_session() {
        let mut registry = harness(DistributedSessionRegistryConfig::default());
        let mut identity = registry.identity();
        identity.schema_hash[0] ^= 0xff;
        let start = registry
            .begin_handshake(
                Duration::ZERO,
                SessionPrincipal::Anonymous,
                &hello(identity, None),
            )
            .unwrap();
        assert_rejected(
            start,
            DistributedSessionHandshakeRejectionReason::SchemaMismatch,
        );
        assert_eq!(registry.session_count(), 0);
    }

    fn harness(config: DistributedSessionRegistryConfig) -> RegistryHarness {
        RegistryHarness::start(bundle(), config)
    }

    fn bundle() -> &'static DistributedProgramBundle {
        static BUNDLE: OnceLock<DistributedProgramBundle> = OnceLock::new();
        BUNDLE.get_or_init(|| {
            compile_bundle(CLIENT_SOURCE, SESSION_SOURCE, SERVER_SOURCE, "registry")
        })
    }

    fn compile_bundle(
        client: &str,
        session: &str,
        server: &str,
        label: &str,
    ) -> DistributedProgramBundle {
        compile_distributed_program_bundle(&[
            request(ProgramRole::Client, client, label),
            request(ProgramRole::Session, session, label),
            request(ProgramRole::Server, server, label),
        ])
        .expect("compile distributed Session registry fixture")
    }

    fn request(role: ProgramRole, source: &str, label: &str) -> ProgramCompileRequest {
        ProgramCompileRequest {
            revision: 1,
            role,
            entry_path: "RUN.bn".to_owned(),
            units: vec![RuntimeSourceUnit {
                path: "RUN.bn".to_owned(),
                source: source.to_owned(),
            }],
            application: ApplicationIdentity::new(
                format!("dev.boon.distributed-session-registry-{label}"),
                format!("test-{}", role.as_str()),
                "server-runtime-test",
            ),
            capability_profile: match role {
                ProgramRole::Client => ProgramCapabilityProfile::PublicClient,
                ProgramRole::Session => ProgramCapabilityProfile::TrustedSession,
                ProgramRole::Server => ProgramCapabilityProfile::TrustedServer,
            },
        }
    }

    fn connect(registry: &mut RegistryHarness, now: Duration) -> TestClient {
        connect_with_bundle(registry, bundle(), now)
    }

    fn connect_with_bundle(
        registry: &mut RegistryHarness,
        bundle: &DistributedProgramBundle,
        now: Duration,
    ) -> TestClient {
        let start = registry
            .begin_handshake(
                now,
                SessionPrincipal::Anonymous,
                &hello(registry.identity(), None),
            )
            .unwrap();
        let offer = offered(start);
        let ready = registry
            .commit_handshake(now, offer.connection_id, &offer.commit_frame())
            .unwrap();
        let generation = ready_generation(&ready);
        let mut runtime = DistributedClientRuntime::start(
            bundle.artifact(ProgramRole::Client).unwrap(),
            registry.registry.config.session_queue_limits,
        )
        .unwrap();
        runtime
            .bind(
                offer.session_id,
                offer.generation,
                offer.applied_client_through,
            )
            .unwrap();
        runtime.mark_current().unwrap();
        let client = TestClient {
            connection_id: offer.connection_id,
            resume_token: offer.resume_token,
            generation,
            runtime,
        };
        poll_until_idle(registry, now);
        client
    }

    fn hello(identity: DistributedSessionRegistryIdentity, token: Option<ResumeToken>) -> Vec<u8> {
        encode_session_control_frame(&SessionControlFrame::ClientHello(ClientHello::new(
            identity.graph_id,
            identity.graph_revision,
            identity.schema_hash,
            token,
            0,
        )))
        .unwrap()
    }

    fn revoke_frame() -> Vec<u8> {
        encode_session_control_frame(&SessionControlFrame::ClientRevoke(ClientRevoke::new()))
            .unwrap()
    }

    struct TestOffer {
        connection_id: DistributedSessionConnectionId,
        resume_token: ResumeToken,
        session_id: SessionId,
        generation: u64,
        applied_client_through: u64,
    }

    impl TestOffer {
        fn commit_frame(&self) -> Vec<u8> {
            encode_session_control_frame(&SessionControlFrame::ClientCommit(ClientCommit::new(
                self.session_id,
                self.generation,
                0,
            )))
            .unwrap()
        }
    }

    fn offered(start: DistributedSessionHandshakeStart) -> TestOffer {
        let DistributedSessionHandshakeStart::Offer(offer) = start else {
            panic!("expected Session handshake offer");
        };
        let connection_id = offer.connection_id();
        let SessionControlFrame::ServerOffer(server_offer) =
            decode_session_control_frame(offer.server_frame()).unwrap()
        else {
            panic!("expected canonical ServerOffer");
        };
        let (resume_token, session_id, generation, applied_client_through) =
            server_offer.into_parts();
        TestOffer {
            connection_id,
            resume_token,
            session_id,
            generation,
            applied_client_through,
        }
    }

    fn assert_rejected(
        start: DistributedSessionHandshakeStart,
        reason: DistributedSessionHandshakeRejectionReason,
    ) {
        let DistributedSessionHandshakeStart::Reject(rejection) = start else {
            panic!("expected Session handshake rejection");
        };
        assert_eq!(rejection.reason(), reason);
        assert!(matches!(
            decode_session_control_frame(rejection.server_frame()).unwrap(),
            SessionControlFrame::ServerReject(_)
        ));
    }

    fn ready_generation(frame: &[u8]) -> u64 {
        let SessionControlFrame::ServerReady(ready) = decode_session_control_frame(frame).unwrap()
        else {
            panic!("expected canonical ServerReady");
        };
        ready.generation()
    }

    fn dispatch_increment(registry: &mut RegistryHarness, client: &mut TestClient) {
        queue_increment(registry, client);
    }

    fn queue_increment(registry: &mut RegistryHarness, client: &mut TestClient) {
        queue_source(registry, client, "store.increment");
    }

    fn queue_source(registry: &mut RegistryHarness, client: &mut TestClient, source: &str) {
        client
            .runtime
            .dispatch(source, SourcePayload::default())
            .unwrap();
        for frame in take_client_session_frames(&mut client.runtime, 64) {
            registry
                .admit_client_frame(client.connection_id, &frame)
                .unwrap();
        }
    }

    fn poll_until_idle(registry: &mut RegistryHarness, now: Duration) {
        for _ in 0..64 {
            if registry.poll(now, 64).unwrap().serviced_origins.is_empty() {
                return;
            }
        }
        panic!("distributed Session registry did not settle");
    }

    fn poll_collect(
        registry: &mut RegistryHarness,
        now: Duration,
    ) -> DistributedSessionRegistryPoll {
        let mut collected = DistributedSessionRegistryPoll::new(0);
        for _ in 0..64 {
            let mut poll = registry.poll(now, 64).unwrap();
            let idle = poll.serviced_origins.is_empty();
            collected
                .serviced_origins
                .append(&mut poll.serviced_origins);
            collected
                .serviced_connections
                .append(&mut poll.serviced_connections);
            collected
                .backpressured_origins
                .append(&mut poll.backpressured_origins);
            collected
                .poisoned_sessions
                .append(&mut poll.poisoned_sessions);
            collected.session_turns.append(&mut poll.session_turns);
            collected.server_turns.append(&mut poll.server_turns);
            collected.expired_sessions += poll.expired_sessions;
            if idle {
                return collected;
            }
        }
        panic!("distributed Session registry did not settle");
    }
}
