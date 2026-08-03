use crate::{
    DistributedServerAuthority, DistributedServerUpdate, PreparedDistributedServerTransaction,
    ServerDelivery, ServerDeliveryTarget,
};
use boon_distributed_runtime::{
    DistributedMessage, DistributedQueueLimits, DistributedSessionRuntime,
    DistributedSessionTemplate, DistributedSessionUpdate,
};
use boon_plan::ProgramRole;
use boon_program_runtime::DistributedProgramBundle;
use boon_runtime::{
    DistributedRuntimeError, DistributedServerMachine, RuntimeTurn, SessionConnectionStatus,
    SessionOrigin, SessionPrincipal, Value,
};
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
}

impl DistributedSessionRegistry {
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
        let activation = self.session_template.instantiate(
            session_id,
            next_transport_generation,
            principal.clone(),
            self.config.session_queue_limits,
        )?;
        let (mut runtime, initial_turn) = activation.into_parts();
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
            self.pending_session_turns.push_back((origin, initial_turn));
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
