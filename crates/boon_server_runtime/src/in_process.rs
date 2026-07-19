use super::{
    AdapterError, BoonServerProgram, DistributedSessionConnectionId,
    DistributedSessionHandshakeRejectionReason, DistributedSessionHandshakeStart,
    DistributedSessionRegistryConfig, DistributedSessionRegistryError, PersistentServerConfig,
    PersistentServerStartup, PersistentServerStatus, ServerTurnClass,
};
use boon_persistence::PersistenceDriver;
use boon_plan::{EffectDeliveryCardinality, ProgramRole};
use boon_runtime::{
    DistributedClientRuntime, DistributedProgramBundle, DistributedQueueLimits,
    DistributedRuntimeError, DocumentFrame, RowId, RuntimeTurn, SessionOrigin, SessionPrincipal,
    SourcePayload, TransientEffectCallId, TransientEffectCreditGrant, TransientEffectInvocation,
    Value,
};
use boon_wire::{
    ClientCommit, ClientHello, ResumeToken, ServerReady, SessionControlFrame,
    SessionControlFrameError, decode_session_control_frame, encode_session_control_frame,
};
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::ops::Range;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_IN_PROCESS_POLL_STEPS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InProcessDistributedRuntimeConfig {
    pub sessions: DistributedSessionRegistryConfig,
    pub client_queue_limits: DistributedQueueLimits,
    pub max_poll_steps: usize,
}

impl Default for InProcessDistributedRuntimeConfig {
    fn default() -> Self {
        Self {
            sessions: DistributedSessionRegistryConfig::default(),
            client_queue_limits: DistributedQueueLimits::default(),
            max_poll_steps: DEFAULT_IN_PROCESS_POLL_STEPS,
        }
    }
}

impl InProcessDistributedRuntimeConfig {
    fn validate(self) -> Result<Self, InProcessDistributedRuntimeError> {
        if self.max_poll_steps == 0
            || self.max_poll_steps > super::MAX_DISTRIBUTED_SESSION_POLL_STEPS
        {
            return Err(InProcessDistributedRuntimeError::InvalidConfig(
                "max_poll_steps must be between 1 and 256",
            ));
        }
        Ok(self)
    }
}

/// Move-only, process-local Client authority needed to resume the same
/// persistent Session after restarting its Server authority.
///
/// The bearer token and transport cursor remain private and are never formatted
/// or serialized by this adapter.
pub struct InProcessResumeState {
    client: DistributedClientRuntime,
    token: ResumeToken,
    applied_server_through: u64,
}

impl InProcessResumeState {
    fn into_parts(self) -> (DistributedClientRuntime, ResumeToken, u64) {
        (self.client, self.token, self.applied_server_through)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InProcessTransientEffectOwner {
    Client,
    Session,
    Server,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InProcessTransientEffectInvocation {
    pub owner: InProcessTransientEffectOwner,
    pub invocation: TransientEffectInvocation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InProcessTransientEffectCancellation {
    pub owner: InProcessTransientEffectOwner,
    pub call_id: TransientEffectCallId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InProcessTransientEffectCreditGrant {
    pub owner: InProcessTransientEffectOwner,
    pub grant: TransientEffectCreditGrant,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InProcessFrameTransferProgress {
    pub admitted: usize,
    pub acknowledged: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InProcessFrameProgress {
    pub client_to_session: InProcessFrameTransferProgress,
    pub session_to_client: InProcessFrameTransferProgress,
}

#[derive(Debug, Default)]
pub struct InProcessPoll {
    pub steps: usize,
    pub has_more_work: bool,
    pub frame_progress: InProcessFrameProgress,
    pub client_turns: Vec<RuntimeTurn>,
    pub session_turns: Vec<RuntimeTurn>,
    pub server_turns: Vec<RuntimeTurn>,
    pub transient_effects: Vec<InProcessTransientEffectInvocation>,
    pub cancelled_transient_effects: Vec<InProcessTransientEffectCancellation>,
    pub transient_effect_credit_grants: Vec<InProcessTransientEffectCreditGrant>,
    pub serviced_session_steps: usize,
    pub backpressured_session_steps: usize,
    pub durable_protocol_checkpoints: usize,
    pub expired_sessions: usize,
}

#[derive(Debug)]
pub enum InProcessDistributedRuntimeError {
    InvalidConfig(&'static str),
    Adapter(AdapterError),
    Registry(DistributedSessionRegistryError),
    Client(DistributedRuntimeError),
    Wire(SessionControlFrameError),
    HandshakeRejected(DistributedSessionHandshakeRejectionReason),
    UnexpectedHandshakeFrame(&'static str),
    Clock(&'static str),
    TimeRegression,
    FrameLease(&'static str),
    PoisonedSession(String),
    DuplicateTransientEffect(TransientEffectCallId),
    UnknownTransientEffect(TransientEffectCallId),
    TransientEffectOwnerMismatch {
        call_id: TransientEffectCallId,
        expected: InProcessTransientEffectOwner,
        actual: InProcessTransientEffectOwner,
    },
    TransientEffectDeliveryMismatch {
        call_id: TransientEffectCallId,
        expected: &'static str,
    },
    Shutdown,
}

impl Display for InProcessDistributedRuntimeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => {
                write!(formatter, "invalid in-process runtime config: {message}")
            }
            Self::Adapter(error) => Display::fmt(error, formatter),
            Self::Registry(error) => Display::fmt(error, formatter),
            Self::Client(error) => Display::fmt(error, formatter),
            Self::Wire(error) => Display::fmt(error, formatter),
            Self::HandshakeRejected(reason) => {
                write!(
                    formatter,
                    "distributed Session handshake was rejected: {reason:?}"
                )
            }
            Self::UnexpectedHandshakeFrame(phase) => {
                write!(
                    formatter,
                    "unexpected distributed Session frame while {phase}"
                )
            }
            Self::Clock(message) => write!(formatter, "in-process runtime clock failed: {message}"),
            Self::TimeRegression => formatter.write_str("in-process runtime time regressed"),
            Self::FrameLease(message) => {
                write!(formatter, "in-process frame lease failed: {message}")
            }
            Self::PoisonedSession(diagnostic) => {
                write!(formatter, "in-process Session was poisoned: {diagnostic}")
            }
            Self::DuplicateTransientEffect(call_id) => {
                write!(
                    formatter,
                    "transient effect call {call_id} was emitted twice"
                )
            }
            Self::UnknownTransientEffect(call_id) => {
                write!(formatter, "transient effect call {call_id} is not active")
            }
            Self::TransientEffectOwnerMismatch {
                call_id,
                expected,
                actual,
            } => write!(
                formatter,
                "transient effect call {call_id} belongs to {actual:?}, not {expected:?}",
            ),
            Self::TransientEffectDeliveryMismatch { call_id, expected } => write!(
                formatter,
                "transient effect call {call_id} does not use {expected} delivery",
            ),
            Self::Shutdown => formatter.write_str("in-process distributed runtime is shut down"),
        }
    }
}

impl Error for InProcessDistributedRuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Adapter(error) => Some(error),
            Self::Registry(error) => Some(error),
            Self::Client(error) => Some(error),
            Self::Wire(error) => Some(error),
            _ => None,
        }
    }
}

impl From<AdapterError> for InProcessDistributedRuntimeError {
    fn from(error: AdapterError) -> Self {
        Self::Adapter(error)
    }
}

impl From<DistributedSessionRegistryError> for InProcessDistributedRuntimeError {
    fn from(error: DistributedSessionRegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<DistributedRuntimeError> for InProcessDistributedRuntimeError {
    fn from(error: DistributedRuntimeError) -> Self {
        Self::Client(error)
    }
}

impl From<SessionControlFrameError> for InProcessDistributedRuntimeError {
    fn from(error: SessionControlFrameError) -> Self {
        Self::Wire(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TransientEffectOwner {
    Client,
    Session(SessionOrigin),
    Server(SessionOrigin),
}

impl TransientEffectOwner {
    fn public(self) -> InProcessTransientEffectOwner {
        match self {
            Self::Client => InProcessTransientEffectOwner::Client,
            Self::Session(_) => InProcessTransientEffectOwner::Session,
            Self::Server(_) => InProcessTransientEffectOwner::Server,
        }
    }
}

#[derive(Clone)]
struct ActiveTransientEffect {
    owner: TransientEffectOwner,
    delivery: EffectDeliveryCardinality,
}

struct PendingTurn {
    owner: TransientEffectOwner,
    turn: RuntimeTurn,
}

struct InitializedClient {
    client: DistributedClientRuntime,
    connection: DistributedSessionConnectionId,
    resume_token: ResumeToken,
    pending_turns: VecDeque<PendingTurn>,
}

/// One complete distributed product graph connected without a network stack.
///
/// The physical Client/Session wire protocol is still exercised in both
/// directions. This type does not mount Client code through `ProgramSession` or
/// bypass the Session registry's routing, persistence, or frame leases.
pub struct InProcessDistributedRuntime {
    client: Option<DistributedClientRuntime>,
    server: BoonServerProgram,
    connection: DistributedSessionConnectionId,
    resume_token: Option<ResumeToken>,
    clock_origin: Duration,
    last_now: Duration,
    config: InProcessDistributedRuntimeConfig,
    pending_turns: VecDeque<PendingTurn>,
    active_transient_effects: BTreeMap<TransientEffectCallId, ActiveTransientEffect>,
    shutdown: bool,
}

impl InProcessDistributedRuntime {
    pub fn start_ephemeral(
        bundle: &DistributedProgramBundle,
    ) -> Result<Self, InProcessDistributedRuntimeError> {
        Self::start_ephemeral_with_config(bundle, InProcessDistributedRuntimeConfig::default())
    }

    pub fn start_ephemeral_with_config(
        bundle: &DistributedProgramBundle,
        config: InProcessDistributedRuntimeConfig,
    ) -> Result<Self, InProcessDistributedRuntimeError> {
        let config = config.validate()?;
        let mut server = BoonServerProgram::new_distributed(bundle, config.sessions)?;
        let clock_origin = unix_now()?;
        let initialized = initialize_client(bundle, &mut server, config, clock_origin, None)?;
        Ok(Self::from_initialized(
            server,
            initialized,
            config,
            clock_origin,
        ))
    }

    pub fn start_persistent<D>(
        bundle: &DistributedProgramBundle,
        driver: D,
        persistence: PersistentServerConfig,
    ) -> Result<(Self, PersistentServerStartup), InProcessDistributedRuntimeError>
    where
        D: PersistenceDriver + Send + 'static,
    {
        Self::start_persistent_with_config(
            bundle,
            driver,
            persistence,
            InProcessDistributedRuntimeConfig::default(),
        )
    }

    pub fn start_persistent_with_config<D>(
        bundle: &DistributedProgramBundle,
        driver: D,
        persistence: PersistentServerConfig,
        config: InProcessDistributedRuntimeConfig,
    ) -> Result<(Self, PersistentServerStartup), InProcessDistributedRuntimeError>
    where
        D: PersistenceDriver + Send + 'static,
    {
        Self::start_or_resume_persistent(bundle, driver, persistence, config, None)
    }

    pub fn resume_persistent<D>(
        bundle: &DistributedProgramBundle,
        driver: D,
        persistence: PersistentServerConfig,
        resume: InProcessResumeState,
    ) -> Result<(Self, PersistentServerStartup), InProcessDistributedRuntimeError>
    where
        D: PersistenceDriver + Send + 'static,
    {
        Self::resume_persistent_with_config(
            bundle,
            driver,
            persistence,
            InProcessDistributedRuntimeConfig::default(),
            resume,
        )
    }

    pub fn resume_persistent_with_config<D>(
        bundle: &DistributedProgramBundle,
        driver: D,
        persistence: PersistentServerConfig,
        config: InProcessDistributedRuntimeConfig,
        resume: InProcessResumeState,
    ) -> Result<(Self, PersistentServerStartup), InProcessDistributedRuntimeError>
    where
        D: PersistenceDriver + Send + 'static,
    {
        Self::start_or_resume_persistent(bundle, driver, persistence, config, Some(resume))
    }

    fn start_or_resume_persistent<D>(
        bundle: &DistributedProgramBundle,
        driver: D,
        persistence: PersistentServerConfig,
        config: InProcessDistributedRuntimeConfig,
        resume: Option<InProcessResumeState>,
    ) -> Result<(Self, PersistentServerStartup), InProcessDistributedRuntimeError>
    where
        D: PersistenceDriver + Send + 'static,
    {
        let config = config.validate()?;
        let (mut server, startup) = BoonServerProgram::with_distributed_persistence(
            bundle,
            driver,
            persistence,
            config.sessions,
        )?;
        let clock_origin = unix_now()?;
        let initialized = match initialize_client(bundle, &mut server, config, clock_origin, resume)
        {
            Ok(initialized) => initialized,
            Err(error) => {
                let _ = server.shutdown_persistent();
                return Err(error);
            }
        };
        Ok((
            Self::from_initialized(server, initialized, config, clock_origin),
            startup,
        ))
    }

    fn from_initialized(
        server: BoonServerProgram,
        initialized: InitializedClient,
        config: InProcessDistributedRuntimeConfig,
        clock_origin: Duration,
    ) -> Self {
        Self {
            client: Some(initialized.client),
            server,
            connection: initialized.connection,
            resume_token: Some(initialized.resume_token),
            clock_origin,
            last_now: Duration::ZERO,
            config,
            pending_turns: initialized.pending_turns,
            active_transient_effects: BTreeMap::new(),
            shutdown: false,
        }
    }

    pub fn dispatch_client(
        &mut self,
        path: &str,
        payload: SourcePayload,
    ) -> Result<(), InProcessDistributedRuntimeError> {
        self.dispatch_client_scoped(path, None, payload)
    }

    pub fn dispatch_client_scoped(
        &mut self,
        path: &str,
        row: Option<RowId>,
        payload: SourcePayload,
    ) -> Result<(), InProcessDistributedRuntimeError> {
        self.require_running()?;
        let update = self
            .client
            .as_mut()
            .expect("running in-process runtime owns its Client")
            .dispatch_scoped(path, row, payload)?;
        self.queue_turns(TransientEffectOwner::Client, update.turns);
        Ok(())
    }

    /// Performs at most `config.max_poll_steps` fixed-shape transport/runtime rounds.
    pub fn poll(
        &mut self,
        now: Duration,
    ) -> Result<InProcessPoll, InProcessDistributedRuntimeError> {
        self.require_running()?;
        let registry_now = self.registry_now(now)?;
        self.last_now = now;
        let mut output = InProcessPoll::default();

        for _ in 0..self.config.max_poll_steps {
            let mut progressed = false;

            if let Some(pending) = self.pending_turns.pop_front() {
                self.emit_turn(pending, &mut output)?;
                progressed = true;
            }

            if self.transfer_client_frame(&mut output.frame_progress)? {
                progressed = true;
            }

            let registry_poll = self.server.poll_distributed_sessions(registry_now, 1)?;
            if let Some(poisoned) = registry_poll.poisoned_sessions.into_iter().next() {
                return Err(InProcessDistributedRuntimeError::PoisonedSession(
                    poisoned.diagnostic,
                ));
            }
            output.serviced_session_steps += registry_poll.serviced_origins.len();
            output.backpressured_session_steps += registry_poll.backpressured_origins.len();
            output.durable_protocol_checkpoints += registry_poll.durable_protocol_checkpoints;
            output.expired_sessions += registry_poll.expired_sessions;
            progressed |= !registry_poll.serviced_origins.is_empty()
                || !registry_poll.session_turns.is_empty()
                || !registry_poll.server_turns.is_empty()
                || registry_poll.durable_protocol_checkpoints != 0
                || registry_poll.expired_sessions != 0;
            for (origin, turn) in registry_poll.session_turns {
                self.pending_turns.push_back(PendingTurn {
                    owner: TransientEffectOwner::Session(origin),
                    turn,
                });
            }
            for (origin, turn) in registry_poll.server_turns {
                self.pending_turns.push_back(PendingTurn {
                    owner: TransientEffectOwner::Server(origin),
                    turn,
                });
            }

            if self.transfer_session_frame(&mut output.frame_progress)? {
                progressed = true;
            }

            if !progressed {
                break;
            }
            output.steps += 1;
        }

        output.has_more_work = self.has_immediate_work();
        Ok(output)
    }

    pub fn complete_transient_effect(
        &mut self,
        owner: InProcessTransientEffectOwner,
        call_id: TransientEffectCallId,
        outcome: Value,
    ) -> Result<(), InProcessDistributedRuntimeError> {
        self.require_running()?;
        let active = self.active_effect(owner, call_id)?.clone();
        if !matches!(active.delivery, EffectDeliveryCardinality::Single) {
            return Err(
                InProcessDistributedRuntimeError::TransientEffectDeliveryMismatch {
                    call_id,
                    expected: "single-result",
                },
            );
        }

        match active.owner {
            TransientEffectOwner::Client => {
                let update = self
                    .client
                    .as_mut()
                    .expect("running in-process runtime owns its Client")
                    .complete_transient_effect(call_id, outcome)?;
                self.queue_turns(TransientEffectOwner::Client, update.turns);
            }
            TransientEffectOwner::Session(origin) => {
                let pending = self
                    .server
                    .complete_distributed_session_transient_effect(origin, call_id, outcome)?;
                if pending {
                    return Err(InProcessDistributedRuntimeError::Client(
                        DistributedRuntimeError::Runtime(
                            "single-result Session effect remained active after completion"
                                .to_owned(),
                        ),
                    ));
                }
            }
            TransientEffectOwner::Server(origin) => {
                let turn = self.server.complete_server_transient_effect(
                    call_id,
                    outcome,
                    ServerTurnClass::Distributed,
                )?;
                self.pending_turns.push_back(PendingTurn {
                    owner: TransientEffectOwner::Server(origin),
                    turn,
                });
            }
        }
        self.active_transient_effects.remove(&call_id);
        Ok(())
    }

    pub fn deliver_transient_effect_result(
        &mut self,
        owner: InProcessTransientEffectOwner,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
    ) -> Result<(), InProcessDistributedRuntimeError> {
        self.require_running()?;
        let active = self.active_effect(owner, call_id)?.clone();
        if !matches!(active.delivery, EffectDeliveryCardinality::Stream { .. }) {
            return Err(
                InProcessDistributedRuntimeError::TransientEffectDeliveryMismatch {
                    call_id,
                    expected: "stream",
                },
            );
        }
        let terminal = stream_result_is_terminal(&active.delivery, &outcome);

        match active.owner {
            TransientEffectOwner::Client => {
                let update = self
                    .client
                    .as_mut()
                    .expect("running in-process runtime owns its Client")
                    .deliver_transient_effect_result(call_id, result_sequence, outcome)?;
                self.queue_turns(TransientEffectOwner::Client, update.turns);
            }
            TransientEffectOwner::Session(origin) => {
                let pending = self
                    .server
                    .deliver_distributed_session_transient_effect_result(
                        origin,
                        call_id,
                        result_sequence,
                        outcome,
                    )?;
                if pending == terminal {
                    return Err(InProcessDistributedRuntimeError::Client(
                        DistributedRuntimeError::Runtime(
                            "Session stream effect terminal state disagreed with its contract"
                                .to_owned(),
                        ),
                    ));
                }
            }
            TransientEffectOwner::Server(origin) => {
                let turn = self.server.deliver_server_transient_effect_result(
                    call_id,
                    result_sequence,
                    outcome,
                    ServerTurnClass::Distributed,
                )?;
                self.pending_turns.push_back(PendingTurn {
                    owner: TransientEffectOwner::Server(origin),
                    turn,
                });
            }
        }
        if terminal {
            self.active_transient_effects.remove(&call_id);
        }
        Ok(())
    }

    pub fn document_frame(&self) -> Option<&DocumentFrame> {
        self.client
            .as_ref()
            .and_then(DistributedClientRuntime::document_frame)
    }

    pub fn client_root_value_current(
        &mut self,
        name: &str,
    ) -> Result<Value, InProcessDistributedRuntimeError> {
        self.require_running()?;
        self.client
            .as_mut()
            .expect("running in-process runtime owns its Client")
            .root_value_current(name)
            .map_err(Into::into)
    }

    pub fn inspect_client_value_current(
        &mut self,
        name: &str,
        max_rows: usize,
    ) -> Result<Value, InProcessDistributedRuntimeError> {
        self.require_running()?;
        self.client
            .as_mut()
            .expect("running in-process runtime owns its Client")
            .inspect_value_current(name, max_rows)
            .map_err(Into::into)
    }

    pub fn demand_client_document_window_by_id(
        &mut self,
        materialization: u64,
        visible: Range<u64>,
        overscan: Range<u64>,
    ) -> Result<Vec<boon_runtime::DocumentPatch>, InProcessDistributedRuntimeError> {
        self.require_running()?;
        self.client
            .as_mut()
            .expect("running in-process runtime owns its Client")
            .demand_document_window_by_id(materialization, visible, overscan)
            .map_err(Into::into)
    }

    pub fn client_row_target_for_source_path(
        &self,
        path: &str,
        key: u64,
        generation: u64,
    ) -> Result<RowId, InProcessDistributedRuntimeError> {
        self.require_running()?;
        self.client
            .as_ref()
            .expect("running in-process runtime owns its Client")
            .row_target_for_source_path(path, key, generation)
            .map_err(Into::into)
    }

    pub fn client_row_target_for_source_text(
        &self,
        path: &str,
        text: &str,
        occurrence: usize,
    ) -> Result<Option<RowId>, InProcessDistributedRuntimeError> {
        self.require_running()?;
        self.client
            .as_ref()
            .expect("running in-process runtime owns its Client")
            .row_target_for_source_text(path, text, occurrence)
            .map_err(Into::into)
    }

    pub fn client_source_row_lookup_field(&self, path: &str) -> Option<&str> {
        self.client
            .as_ref()
            .and_then(|client| client.source_row_lookup_field(path))
    }

    pub fn client_source_is_row_scoped(&self, path: &str) -> Option<bool> {
        self.client
            .as_ref()
            .and_then(|client| client.source_is_row_scoped(path))
    }

    pub fn pending_transient_effect_count(&self, owner: InProcessTransientEffectOwner) -> usize {
        self.active_transient_effects
            .values()
            .filter(|active| active.owner.public() == owner)
            .count()
    }

    pub fn next_deadline(&self) -> Option<Duration> {
        if self.shutdown {
            return None;
        }
        if self.has_immediate_work() {
            return Some(self.last_now);
        }
        self.server
            .distributed_sessions
            .as_ref()
            .and_then(|sessions| sessions.next_deadline())
            .map(|deadline| deadline.saturating_sub(self.clock_origin))
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown
    }

    pub fn persistent_server_status(&self) -> Option<PersistentServerStatus> {
        self.server
            .lifecycle_handle()
            .map(|lifecycle| lifecycle.status())
    }

    /// Disconnects the real Session, drains persistent authority, and returns
    /// opaque resume authority for a later `resume_persistent` call.
    pub fn shutdown(
        &mut self,
    ) -> Result<Option<InProcessResumeState>, InProcessDistributedRuntimeError> {
        if self.shutdown {
            return Ok(None);
        }
        let client = self
            .client
            .as_mut()
            .expect("running in-process runtime owns its Client");
        let applied_server_through = client.applied_server_through();
        let _ = client.cancel_all_transient_effects()?;

        let active = self
            .active_transient_effects
            .iter()
            .map(|(call_id, active)| (*call_id, active.owner))
            .collect::<Vec<_>>();
        for (call_id, owner) in active {
            match owner {
                TransientEffectOwner::Client => {}
                TransientEffectOwner::Session(origin) => self
                    .server
                    .cancel_distributed_session_transient_effect(origin, call_id)?,
                TransientEffectOwner::Server(_) => {
                    self.server
                        .cancel_server_transient_effect(call_id, ServerTurnClass::Distributed)?;
                }
            }
        }
        self.active_transient_effects.clear();
        let _ = self
            .client
            .as_mut()
            .expect("running in-process runtime owns its Client")
            .mark_stale()?;
        let registry_now = self.registry_now(self.last_now)?;
        self.server
            .disconnect_distributed_session(registry_now, self.connection)?;
        self.server.shutdown_persistent()?;
        self.pending_turns.clear();
        self.shutdown = true;
        let client = self
            .client
            .take()
            .expect("shutting down in-process runtime still owns its Client");
        Ok(self.resume_token.take().map(|token| InProcessResumeState {
            client,
            token,
            applied_server_through,
        }))
    }

    fn registry_now(&self, now: Duration) -> Result<Duration, InProcessDistributedRuntimeError> {
        if now < self.last_now {
            return Err(InProcessDistributedRuntimeError::TimeRegression);
        }
        self.clock_origin
            .checked_add(now)
            .ok_or(InProcessDistributedRuntimeError::Clock(
                "logical time overflowed the registry clock",
            ))
    }

    fn require_running(&self) -> Result<(), InProcessDistributedRuntimeError> {
        if self.shutdown {
            Err(InProcessDistributedRuntimeError::Shutdown)
        } else {
            Ok(())
        }
    }

    fn queue_turns(
        &mut self,
        owner: TransientEffectOwner,
        turns: impl IntoIterator<Item = RuntimeTurn>,
    ) {
        self.pending_turns
            .extend(turns.into_iter().map(|turn| PendingTurn { owner, turn }));
    }

    fn transfer_client_frame(
        &mut self,
        progress: &mut InProcessFrameProgress,
    ) -> Result<bool, InProcessDistributedRuntimeError> {
        let Some(frame) = self
            .client
            .as_mut()
            .expect("running in-process runtime owns its Client")
            .next_session_frame()?
        else {
            return Ok(false);
        };
        self.server
            .admit_distributed_client_frame(self.connection, &frame)?;
        progress.client_to_session.admitted += 1;
        if !self
            .client
            .as_mut()
            .expect("running in-process runtime owns its Client")
            .acknowledge_session_frame()
        {
            return Err(InProcessDistributedRuntimeError::FrameLease(
                "Client writer did not acknowledge its admitted frame",
            ));
        }
        progress.client_to_session.acknowledged += 1;
        Ok(true)
    }

    fn transfer_session_frame(
        &mut self,
        progress: &mut InProcessFrameProgress,
    ) -> Result<bool, InProcessDistributedRuntimeError> {
        let Some(frame) = self.server.next_distributed_client_frame(self.connection)? else {
            return Ok(false);
        };
        let update = self
            .client
            .as_mut()
            .expect("running in-process runtime owns its Client")
            .accept_session_frame(&frame)?;
        progress.session_to_client.admitted += 1;
        if !self
            .server
            .acknowledge_distributed_client_frame(self.connection)?
        {
            return Err(InProcessDistributedRuntimeError::FrameLease(
                "Session writer did not acknowledge its accepted frame",
            ));
        }
        progress.session_to_client.acknowledged += 1;
        self.queue_turns(TransientEffectOwner::Client, update.turns);
        Ok(true)
    }

    fn emit_turn(
        &mut self,
        pending: PendingTurn,
        output: &mut InProcessPoll,
    ) -> Result<(), InProcessDistributedRuntimeError> {
        for call_id in &pending.turn.cancelled_transient_effects {
            let active = self.active_transient_effects.remove(call_id).ok_or(
                InProcessDistributedRuntimeError::UnknownTransientEffect(*call_id),
            )?;
            output
                .cancelled_transient_effects
                .push(InProcessTransientEffectCancellation {
                    owner: active.owner.public(),
                    call_id: *call_id,
                });
        }
        for grant in &pending.turn.transient_effect_credit_grants {
            let active = self.active_transient_effects.get(&grant.call_id).ok_or(
                InProcessDistributedRuntimeError::UnknownTransientEffect(grant.call_id),
            )?;
            output
                .transient_effect_credit_grants
                .push(InProcessTransientEffectCreditGrant {
                    owner: active.owner.public(),
                    grant: *grant,
                });
        }
        for invocation in &pending.turn.transient_effects {
            let active = ActiveTransientEffect {
                owner: pending.owner,
                delivery: invocation.delivery.clone(),
            };
            if self
                .active_transient_effects
                .insert(invocation.call_id, active)
                .is_some()
            {
                return Err(InProcessDistributedRuntimeError::DuplicateTransientEffect(
                    invocation.call_id,
                ));
            }
            output
                .transient_effects
                .push(InProcessTransientEffectInvocation {
                    owner: pending.owner.public(),
                    invocation: invocation.clone(),
                });
        }

        match pending.owner {
            TransientEffectOwner::Client => output.client_turns.push(pending.turn),
            TransientEffectOwner::Session(_) => output.session_turns.push(pending.turn),
            TransientEffectOwner::Server(_) => output.server_turns.push(pending.turn),
        }
        Ok(())
    }

    fn active_effect(
        &self,
        owner: InProcessTransientEffectOwner,
        call_id: TransientEffectCallId,
    ) -> Result<&ActiveTransientEffect, InProcessDistributedRuntimeError> {
        let active = self.active_transient_effects.get(&call_id).ok_or(
            InProcessDistributedRuntimeError::UnknownTransientEffect(call_id),
        )?;
        let actual = active.owner.public();
        if owner != actual {
            return Err(
                InProcessDistributedRuntimeError::TransientEffectOwnerMismatch {
                    call_id,
                    expected: owner,
                    actual,
                },
            );
        }
        Ok(active)
    }

    fn has_immediate_work(&self) -> bool {
        if !self.pending_turns.is_empty()
            || self
                .client
                .as_ref()
                .is_some_and(|client| client.pending_session_frames() != 0)
        {
            return true;
        }
        let Some(sessions) = self.server.distributed_sessions.as_ref() else {
            return false;
        };
        sessions.has_runnable_work()
            || sessions
                .has_sendable_client_frame(self.connection)
                .unwrap_or(true)
    }
}

impl Drop for InProcessDistributedRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn initialize_client(
    bundle: &DistributedProgramBundle,
    server: &mut BoonServerProgram,
    config: InProcessDistributedRuntimeConfig,
    registry_now: Duration,
    resume: Option<InProcessResumeState>,
) -> Result<InitializedClient, InProcessDistributedRuntimeError> {
    let identity = server
        .distributed_identity()
        .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?;
    let (mut client, resume_token, applied_server_through) = match resume {
        Some(resume) => {
            let (client, token, applied) = resume.into_parts();
            (client, Some(token), applied)
        }
        None => {
            let artifact = bundle.artifact(ProgramRole::Client).ok_or_else(|| {
                InProcessDistributedRuntimeError::Client(DistributedRuntimeError::Runtime(
                    "distributed bundle has no Client artifact".to_owned(),
                ))
            })?;
            (
                DistributedClientRuntime::start(artifact, config.client_queue_limits)?,
                None,
                0,
            )
        }
    };
    let hello = encode_session_control_frame(&SessionControlFrame::ClientHello(ClientHello::new(
        identity.graph_id,
        identity.graph_revision,
        identity.schema_hash,
        resume_token,
        applied_server_through,
    )))?;
    let offer = match server.begin_distributed_handshake(
        registry_now,
        SessionPrincipal::Anonymous,
        &hello,
    )? {
        DistributedSessionHandshakeStart::Offer(offer) => offer,
        DistributedSessionHandshakeStart::Reject(rejection) => {
            return Err(InProcessDistributedRuntimeError::HandshakeRejected(
                rejection.reason(),
            ));
        }
    };
    let (connection, offer_frame) = offer.into_parts();
    let SessionControlFrame::ServerOffer(offer) = decode_session_control_frame(&offer_frame)?
    else {
        return Err(InProcessDistributedRuntimeError::UnexpectedHandshakeFrame(
            "awaiting ServerOffer",
        ));
    };
    let (next_resume_token, session_id, generation, applied_client_through) = offer.into_parts();
    let commit = encode_session_control_frame(&SessionControlFrame::ClientCommit(
        ClientCommit::new(session_id, generation, applied_server_through),
    ))?;
    let ready_frame = server.commit_distributed_handshake(registry_now, connection, &commit)?;
    let SessionControlFrame::ServerReady(ready) = decode_session_control_frame(&ready_frame)?
    else {
        return Err(InProcessDistributedRuntimeError::UnexpectedHandshakeFrame(
            "awaiting ServerReady",
        ));
    };
    validate_ready(&ready, session_id, generation, applied_client_through)?;

    let mut pending_turns = VecDeque::new();
    pending_turns.extend(
        client
            .bind(session_id, generation, applied_client_through)?
            .turns
            .into_iter()
            .map(|turn| PendingTurn {
                owner: TransientEffectOwner::Client,
                turn,
            }),
    );
    pending_turns.extend(
        client
            .mark_current()?
            .turns
            .into_iter()
            .map(|turn| PendingTurn {
                owner: TransientEffectOwner::Client,
                turn,
            }),
    );

    Ok(InitializedClient {
        client,
        connection,
        resume_token: next_resume_token,
        pending_turns,
    })
}

fn validate_ready(
    ready: &ServerReady,
    session_id: boon_wire::SessionId,
    generation: u64,
    applied_client_through: u64,
) -> Result<(), InProcessDistributedRuntimeError> {
    if ready.session_id() != session_id
        || ready.generation() != generation
        || ready.applied_client_through() != applied_client_through
    {
        return Err(InProcessDistributedRuntimeError::UnexpectedHandshakeFrame(
            "validating ServerReady",
        ));
    }
    Ok(())
}

fn stream_result_is_terminal(delivery: &EffectDeliveryCardinality, outcome: &Value) -> bool {
    let EffectDeliveryCardinality::Stream {
        terminal_result_tags,
        ..
    } = delivery
    else {
        return false;
    };
    effect_outcome_tag(outcome).is_some_and(|tag| {
        terminal_result_tags
            .binary_search_by(|candidate| candidate.as_str().cmp(tag))
            .is_ok()
    })
}

fn effect_outcome_tag(value: &Value) -> Option<&str> {
    match value.visible() {
        Value::Text(tag) => Some(tag),
        Value::Record(fields) => fields.get("$tag").and_then(|tag| match tag {
            Value::Text(tag) => Some(tag.as_str()),
            _ => None,
        }),
        _ => None,
    }
}

fn unix_now() -> Result<Duration, InProcessDistributedRuntimeError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| InProcessDistributedRuntimeError::Clock("system time is before Unix epoch"))
}
