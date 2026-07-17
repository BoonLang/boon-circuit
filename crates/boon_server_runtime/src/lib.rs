//! Generic adapter between a trusted Boon server program and the native server host.
//!
//! A WebSocket actions output is a list of closed records with the fields
//! `kind`, `status`, `body_kind`, `body_text`, `body_bytes`, `frame_kind`,
//! `text`, `bytes`, `room`, `include_current`, `code`, and `reason`. `kind`
//! selects one of `Accept`, `Reject`, `Reply`, `Send`, `JoinRoom`, `LeaveRoom`,
//! `Broadcast`, `RequestResync`, or `Close`. Body and frame discriminators are
//! empty when inactive; their bounded storage fields are ignored until the
//! corresponding discriminator is active. `Reject` uses an empty header list.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use boon_persistence::{
    CommitAck, PersistenceDriver, PersistenceWorkerConfig, PersistenceWorkerStatus,
    TurnEnqueueError, TurnReservationError,
};
use boon_plan::{
    DataTypeFieldPlan, DataTypePlan, DataVariantPlan, EffectReplay, HostPortPlan, MachinePlan,
    OutputContractKind, OutputRootId, OutputRootPlan, ProgramRole, SourceId, SourcePayloadField,
    SourceRoute,
};
use boon_runtime::{
    PersistentDispatchError, PersistentProgramSession, PersistentRuntimeStartupDisposition,
    ProgramArtifact, ProgramCapabilityProfile, ProgramSession, ProgramSessionDispatch, RuntimeTurn,
    SourcePayload, TransientEffectCallId, TransientEffectInvocation, Value,
};
use boon_server_host::{
    CallCancellation, CancellationReason, Header, HttpRequest, HttpResponse, PeerAddress,
    ServerProgram, WebSocketAction, WebSocketClose, WebSocketEvent, WebSocketFrame, WebSocketOpen,
    WebSocketTransportError,
};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const MAX_ADAPTER_DIAGNOSTIC_BYTES: usize = 512;
pub const MAX_WEBSOCKET_ACTIONS: usize = 256;
pub const MAX_WEBSOCKET_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_WEBSOCKET_REJECT_BODY_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_WEBSOCKET_ROOM_BYTES: usize = 512;
pub const MAX_WEBSOCKET_CLOSE_REASON_BYTES: usize = 123;
const MAX_EXACT_INTEGER: u128 = 9_007_199_254_740_992;
const MAX_ACTION_KIND_BYTES: usize = 32;
const ACTION_FIELD_STATUS: u16 = 1 << 0;
const ACTION_FIELD_BODY: u16 = 1 << 1;
const ACTION_FIELD_FRAME: u16 = 1 << 2;
const ACTION_FIELD_ROOM: u16 = 1 << 3;
const ACTION_FIELD_INCLUDE_CURRENT: u16 = 1 << 4;
const ACTION_FIELD_CLOSE: u16 = 1 << 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransientEffectLimits {
    pub max_calls_per_round: usize,
    pub max_calls_per_transaction: usize,
    pub max_chained_rounds: usize,
}

impl Default for TransientEffectLimits {
    fn default() -> Self {
        Self {
            max_calls_per_round: 64,
            max_calls_per_transaction: 256,
            max_chained_rounds: 32,
        }
    }
}

impl TransientEffectLimits {
    fn validate(self) -> Result<Self, AdapterError> {
        if self.max_calls_per_round == 0
            || self.max_calls_per_transaction == 0
            || self.max_chained_rounds == 0
            || self.max_calls_per_round > self.max_calls_per_transaction
        {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidArtifact,
                "transient effect limits must be positive and the round limit must not exceed the transaction limit",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransientEffectCompletion {
    pub call_id: TransientEffectCallId,
    pub outcome: Value,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TransientEffectBatchResult {
    pub completions: Vec<TransientEffectCompletion>,
    pub cancelled: Vec<TransientEffectCallId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransientEffectHostError {
    diagnostic: String,
}

impl TransientEffectHostError {
    pub fn new(diagnostic: impl Display) -> Self {
        Self {
            diagnostic: bounded_text(diagnostic.to_string(), MAX_ADAPTER_DIAGNOSTIC_BYTES),
        }
    }

    pub fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
}

impl Display for TransientEffectHostError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.diagnostic)
    }
}

impl Error for TransientEffectHostError {}

#[async_trait]
pub trait TransientEffectHost: Send + 'static {
    fn owns(&self, effect_id: boon_plan::EffectId) -> bool;

    /// Executes one bounded set concurrently where the concrete transports
    /// permit it. Every input call must appear exactly once in either
    /// `completions` or `cancelled`.
    async fn execute_batch(
        &mut self,
        calls: Vec<TransientEffectInvocation>,
    ) -> Result<TransientEffectBatchResult, TransientEffectHostError>;

    fn cancel(&mut self, call_id: TransientEffectCallId);

    fn shutdown(&mut self) {}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterErrorKind {
    InvalidArtifact,
    InvalidHostPort,
    InvalidRequest,
    Runtime,
    Backpressure,
    Persistence,
    InvalidOutput,
    Unsupported,
}

impl AdapterErrorKind {
    const fn label(self) -> &'static str {
        match self {
            Self::InvalidArtifact => "invalid artifact",
            Self::InvalidHostPort => "invalid host port",
            Self::InvalidRequest => "invalid request",
            Self::Runtime => "runtime failure",
            Self::Backpressure => "server admission overloaded",
            Self::Persistence => "persistent authority failure",
            Self::InvalidOutput => "invalid host output",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerTurnDurability {
    /// Do not expose the turn's output until that exact authority turn is
    /// durably committed.
    Immediate,
    /// Admit to the bounded persistence queue and expose the output while its
    /// durable tail is still reported as pending.
    Buffered,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerDurabilityPolicy {
    /// Use `Immediate` whenever an HTTP response acknowledges authoritative
    /// mutation. `Buffered` is reserved for explicitly weaker telemetry-style
    /// contracts whose pending tail is visible in lifecycle status.
    pub http: ServerTurnDurability,
    pub websocket: ServerTurnDurability,
    pub disconnect: ServerTurnDurability,
}

impl ServerDurabilityPolicy {
    pub const AUTHORITATIVE: Self = Self {
        http: ServerTurnDurability::Immediate,
        websocket: ServerTurnDurability::Immediate,
        disconnect: ServerTurnDurability::Immediate,
    };

    pub const BUFFERED: Self = Self {
        http: ServerTurnDurability::Buffered,
        websocket: ServerTurnDurability::Buffered,
        disconnect: ServerTurnDurability::Buffered,
    };
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistentServerConfig {
    pub worker: PersistenceWorkerConfig,
    pub durability: ServerDurabilityPolicy,
}

impl PersistentServerConfig {
    pub fn authoritative(worker: PersistenceWorkerConfig) -> Self {
        Self {
            worker,
            durability: ServerDurabilityPolicy::AUTHORITATIVE,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerLifecyclePhase {
    Ready,
    Failed,
    ShuttingDown,
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistentServerStatus {
    pub phase: ServerLifecyclePhase,
    pub accepting_turns: bool,
    pub startup_disposition: PersistentRuntimeStartupDisposition,
    pub durability: ServerDurabilityPolicy,
    /// Conservative worker snapshot refreshed after every admitted/rejected
    /// turn and during shutdown. A buffered tail may be reported longer than
    /// it actually remains pending, but is never reported durable early.
    pub persistence: PersistenceWorkerStatus,
    pub accepted_turns: u64,
    pub durably_acknowledged_turns: u64,
    pub rejected_turns: u64,
    pub last_acknowledged_epoch: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct ServerLifecycleHandle {
    status: Arc<Mutex<PersistentServerStatus>>,
}

impl ServerLifecycleHandle {
    pub fn status(&self) -> PersistentServerStatus {
        self.status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl fmt::Debug for ServerLifecycleHandle {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerLifecycleHandle")
            .field("status", &self.status())
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct PersistentServerStartup {
    pub disposition: PersistentRuntimeStartupDisposition,
    pub restore_epoch: u64,
    pub restore_through_turn_sequence: u64,
    pub lifecycle: ServerLifecycleHandle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterError {
    kind: AdapterErrorKind,
    diagnostic: String,
}

impl AdapterError {
    fn new(kind: AdapterErrorKind, message: impl Display) -> Self {
        Self {
            kind,
            diagnostic: bounded_text(
                format!("{}: {message}", kind.label()),
                MAX_ADAPTER_DIAGNOSTIC_BYTES,
            ),
        }
    }

    pub fn kind(&self) -> AdapterErrorKind {
        self.kind
    }

    pub fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
}

impl Display for AdapterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.diagnostic)
    }
}

impl Error for AdapterError {}

#[derive(Clone, Debug)]
struct ResolvedSource {
    path: String,
}

#[derive(Clone, Debug)]
struct ResolvedOutput {
    name: String,
}

#[derive(Clone, Debug)]
enum HeaderValueType {
    Text,
    Bytes { fixed_len: Option<u64> },
}

#[derive(Clone, Debug)]
struct HttpResponseSchema {
    body_fixed_len: Option<u64>,
    header_value: Option<HeaderValueType>,
}

impl HttpResponseSchema {
    fn field_names(&self) -> BTreeSet<&'static str> {
        let mut fields = BTreeSet::from(["body", "status"]);
        if self.header_value.is_some() {
            fields.insert("headers");
        }
        fields
    }
}

#[derive(Clone, Debug)]
struct HttpPortBinding {
    request: ResolvedSource,
    disconnect: Option<ResolvedSource>,
    response: ResolvedOutput,
    response_schema: HttpResponseSchema,
}

#[derive(Clone, Debug)]
struct WebSocketPortBinding {
    open: ResolvedSource,
    message: ResolvedSource,
    close: ResolvedSource,
    error: ResolvedSource,
    actions: ResolvedOutput,
}

#[derive(Clone, Debug, Default)]
struct ResolvedBindings {
    http: Option<HttpPortBinding>,
    websocket: Option<WebSocketPortBinding>,
}

enum ServerRuntimeSession {
    Ephemeral(Box<ProgramSession>),
    Persistent(Box<PersistentProgramSession>),
}

struct ServerSessionDispatch {
    dispatched: ProgramSessionDispatch,
    acknowledgement: Option<CommitAck>,
}

struct ServerSessionEffectTurn {
    runtime_turn: RuntimeTurn,
    acknowledgement: Option<CommitAck>,
}

impl ServerRuntimeSession {
    fn artifact(&self) -> &ProgramArtifact {
        match self {
            Self::Ephemeral(session) => session.artifact(),
            Self::Persistent(session) => session.artifact(),
        }
    }

    fn dispatch(
        &mut self,
        source_path: &str,
        payload: SourcePayload,
        durability: ServerTurnDurability,
    ) -> Result<ServerSessionDispatch, AdapterError> {
        match self {
            Self::Ephemeral(session) => session
                .dispatch(source_path, None, payload)
                .map(|dispatched| ServerSessionDispatch {
                    dispatched,
                    acknowledgement: None,
                })
                .map_err(|error| AdapterError::new(AdapterErrorKind::Runtime, error)),
            Self::Persistent(session) => match durability {
                ServerTurnDurability::Immediate => session
                    .dispatch_durably(source_path, None, payload)
                    .map(|(dispatched, acknowledgement)| ServerSessionDispatch {
                        dispatched,
                        acknowledgement: Some(acknowledgement),
                    })
                    .map_err(persistent_dispatch_error),
                ServerTurnDurability::Buffered => session
                    .dispatch(source_path, None, payload)
                    .map(|dispatched| ServerSessionDispatch {
                        dispatched,
                        acknowledgement: None,
                    })
                    .map_err(persistent_dispatch_error),
            },
        }
    }

    fn output_value_current(&mut self, name: &str) -> Result<Value, AdapterError> {
        match self {
            Self::Ephemeral(session) => session
                .output_value_current(name)
                .map_err(|error| AdapterError::new(AdapterErrorKind::Runtime, error)),
            Self::Persistent(session) => session
                .output_value_current(name)
                .map_err(persistent_dispatch_error),
        }
    }

    fn complete_transient_effect(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
        durability: ServerTurnDurability,
    ) -> Result<ServerSessionEffectTurn, AdapterError> {
        match self {
            Self::Ephemeral(session) => session
                .complete_transient_effect(call_id, outcome)
                .map(|runtime_turn| ServerSessionEffectTurn {
                    runtime_turn,
                    acknowledgement: None,
                })
                .map_err(|error| AdapterError::new(AdapterErrorKind::Runtime, error)),
            Self::Persistent(session) => match durability {
                ServerTurnDurability::Immediate => session
                    .complete_transient_effect_durably(call_id, outcome)
                    .map(|acknowledged| ServerSessionEffectTurn {
                        runtime_turn: acknowledged.turn,
                        acknowledgement: Some(acknowledged.acknowledgement),
                    })
                    .map_err(persistent_dispatch_error),
                ServerTurnDurability::Buffered => session
                    .complete_transient_effect(call_id, outcome)
                    .map(|runtime_turn| ServerSessionEffectTurn {
                        runtime_turn,
                        acknowledgement: None,
                    })
                    .map_err(persistent_dispatch_error),
            },
        }
    }

    fn cancel_transient_effect(
        &mut self,
        call_id: TransientEffectCallId,
    ) -> Result<bool, AdapterError> {
        match self {
            Self::Ephemeral(session) => session
                .cancel_transient_effect(call_id)
                .map_err(|error| AdapterError::new(AdapterErrorKind::Runtime, error)),
            Self::Persistent(session) => session
                .cancel_transient_effect(call_id)
                .map_err(persistent_dispatch_error),
        }
    }

    fn pending_transient_effect_count(&self) -> usize {
        match self {
            Self::Ephemeral(session) => session.pending_transient_effect_count(),
            Self::Persistent(session) => session.pending_transient_effect_count(),
        }
    }

    fn persistence_status(&self) -> Option<PersistenceWorkerStatus> {
        match self {
            Self::Ephemeral(_) => None,
            Self::Persistent(session) => Some(session.persistence_status()),
        }
    }

    fn barrier(&self) -> Result<(), AdapterError> {
        match self {
            Self::Ephemeral(_) => Ok(()),
            Self::Persistent(session) => session
                .barrier()
                .map(|_| ())
                .map_err(|error| AdapterError::new(AdapterErrorKind::Persistence, error)),
        }
    }

    fn shutdown(&self) -> Result<(), AdapterError> {
        match self {
            Self::Ephemeral(_) => Ok(()),
            Self::Persistent(session) => session
                .shutdown()
                .map(|_| ())
                .map_err(|error| AdapterError::new(AdapterErrorKind::Persistence, error)),
        }
    }
}

fn persistent_dispatch_error(error: PersistentDispatchError) -> AdapterError {
    let kind = match &error {
        PersistentDispatchError::Backpressure(
            TurnReservationError::Backpressure { .. } | TurnReservationError::ControlInProgress,
        ) => AdapterErrorKind::Backpressure,
        PersistentDispatchError::Backpressure(TurnReservationError::Closed) => {
            AdapterErrorKind::Persistence
        }
        PersistentDispatchError::Runtime(_) => AdapterErrorKind::Runtime,
        PersistentDispatchError::PersistenceAdmissionFailed { error, .. }
            if matches!(
                error.as_ref(),
                TurnEnqueueError::Backpressure { .. } | TurnEnqueueError::ControlInProgress { .. }
            ) =>
        {
            AdapterErrorKind::Backpressure
        }
        PersistentDispatchError::PersistenceAdmissionFailed { .. }
        | PersistentDispatchError::ImmediateCommitFailed { .. } => AdapterErrorKind::Persistence,
    };
    AdapterError::new(kind, error)
}

struct PersistentServerState {
    durability: ServerDurabilityPolicy,
    lifecycle: ServerLifecycleHandle,
    admission_open: bool,
    shutdown_complete: bool,
}

#[derive(Clone, Copy)]
enum ServerTurnClass {
    Http,
    WebSocket,
    Disconnect,
}

enum SettledTransientEffect {
    Completed(Value),
    Cancelled,
}

fn validate_effect_batch_result(
    expected: &[TransientEffectCallId],
    result: TransientEffectBatchResult,
) -> Result<BTreeMap<TransientEffectCallId, SettledTransientEffect>, AdapterError> {
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    let mut settled = BTreeMap::new();
    for completion in result.completions {
        if !expected.contains(&completion.call_id)
            || settled
                .insert(
                    completion.call_id,
                    SettledTransientEffect::Completed(completion.outcome),
                )
                .is_some()
        {
            return Err(AdapterError::new(
                AdapterErrorKind::Runtime,
                "transient effect host returned an unknown or duplicate completion",
            ));
        }
    }
    for call_id in result.cancelled {
        if !expected.contains(&call_id)
            || settled
                .insert(call_id, SettledTransientEffect::Cancelled)
                .is_some()
        {
            return Err(AdapterError::new(
                AdapterErrorKind::Runtime,
                "transient effect host returned an unknown or duplicate cancellation",
            ));
        }
    }
    if settled.len() != expected.len() {
        return Err(AdapterError::new(
            AdapterErrorKind::Runtime,
            "transient effect host did not settle every submitted call",
        ));
    }
    Ok(settled)
}

/// One serialized trusted-server session owned by [`boon_server_host`].
///
/// Host-port names are never conventions at this boundary. The constructor
/// resolves the IDs embedded in `HostPortPlan` to runtime handles once, before
/// the native listener can be started.
pub struct BoonServerProgram {
    session: ServerRuntimeSession,
    http: Option<HttpPortBinding>,
    websocket: Option<WebSocketPortBinding>,
    last_diagnostic: Option<AdapterError>,
    persistent: Option<PersistentServerState>,
    transient_effect_host: Option<Box<dyn TransientEffectHost>>,
    transient_effect_limits: TransientEffectLimits,
    active_transient_effects: BTreeSet<TransientEffectCallId>,
}

impl BoonServerProgram {
    pub fn new(artifact: ProgramArtifact) -> Result<Self, AdapterError> {
        validate_server_artifact(&artifact)?;
        let bindings = resolve_bindings(artifact.plan())?;
        let session = ProgramSession::start(artifact)
            .map_err(|error| AdapterError::new(AdapterErrorKind::InvalidArtifact, error))?;
        Ok(Self {
            session: ServerRuntimeSession::Ephemeral(Box::new(session)),
            http: bindings.http,
            websocket: bindings.websocket,
            last_diagnostic: None,
            persistent: None,
            transient_effect_host: None,
            transient_effect_limits: TransientEffectLimits::default(),
            active_transient_effects: BTreeSet::new(),
        })
    }

    pub fn from_artifact(artifact: ProgramArtifact) -> Result<Self, AdapterError> {
        Self::new(artifact)
    }

    pub fn with_persistence<D>(
        artifact: ProgramArtifact,
        driver: D,
        config: PersistentServerConfig,
    ) -> Result<(Self, PersistentServerStartup), AdapterError>
    where
        D: PersistenceDriver + Send + 'static,
    {
        validate_server_artifact(&artifact)?;
        let bindings = resolve_bindings(artifact.plan())?;
        let (session, startup) =
            PersistentProgramSession::start(artifact, driver, config.worker.clone())
                .map_err(|error| AdapterError::new(AdapterErrorKind::Persistence, error))?;
        let session = ServerRuntimeSession::Persistent(Box::new(session));
        let persistence = session
            .persistence_status()
            .expect("persistent session reports persistence status");
        if !persistence.worker_alive || !persistence.accepting_turns {
            let _ = session.shutdown();
            return Err(AdapterError::new(
                AdapterErrorKind::Persistence,
                "persistence worker did not enter ready admission after startup",
            ));
        }

        let startup_disposition = startup.disposition.clone();
        let restore_epoch = startup.restore_image.epoch;
        let lifecycle = ServerLifecycleHandle {
            status: Arc::new(Mutex::new(PersistentServerStatus {
                phase: ServerLifecyclePhase::Ready,
                accepting_turns: true,
                startup_disposition: startup_disposition.clone(),
                durability: config.durability,
                persistence,
                accepted_turns: 0,
                durably_acknowledged_turns: 0,
                rejected_turns: 0,
                last_acknowledged_epoch: Some(restore_epoch),
                last_error: None,
            })),
        };
        let server_startup = PersistentServerStartup {
            disposition: startup_disposition,
            restore_epoch,
            restore_through_turn_sequence: startup.restore_image.through_turn_sequence,
            lifecycle: lifecycle.clone(),
        };
        Ok((
            Self {
                session,
                http: bindings.http,
                websocket: bindings.websocket,
                last_diagnostic: None,
                persistent: Some(PersistentServerState {
                    durability: config.durability,
                    lifecycle,
                    admission_open: true,
                    shutdown_complete: false,
                }),
                transient_effect_host: None,
                transient_effect_limits: TransientEffectLimits::default(),
                active_transient_effects: BTreeSet::new(),
            },
            server_startup,
        ))
    }

    pub fn artifact(&self) -> &ProgramArtifact {
        self.session.artifact()
    }

    pub fn has_http_port(&self) -> bool {
        self.http.is_some()
    }

    pub fn has_websocket_port(&self) -> bool {
        self.websocket.is_some()
    }

    pub fn last_diagnostic(&self) -> Option<&AdapterError> {
        self.last_diagnostic.as_ref()
    }

    pub fn lifecycle_handle(&self) -> Option<ServerLifecycleHandle> {
        self.persistent
            .as_ref()
            .map(|state| state.lifecycle.clone())
    }

    pub fn attach_transient_effect_host(
        &mut self,
        host: Box<dyn TransientEffectHost>,
        limits: TransientEffectLimits,
    ) -> Result<(), AdapterError> {
        if self.transient_effect_host.is_some() {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidArtifact,
                "trusted server already has a transient effect host",
            ));
        }
        let limits = limits.validate()?;
        let missing = self
            .artifact()
            .plan()
            .effects
            .iter()
            .filter(|effect| matches!(effect.replay, EffectReplay::ReadOnly))
            .filter(|effect| !host.owns(effect.effect_id))
            .map(|effect| effect.host_operation.as_str())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidArtifact,
                format_args!(
                    "transient effect host does not own required operations: {}",
                    missing.join(", ")
                ),
            ));
        }
        self.transient_effect_limits = limits;
        self.transient_effect_host = Some(host);
        Ok(())
    }

    fn ensure_admission_open(&mut self) -> Result<(), AdapterError> {
        if self
            .persistent
            .as_ref()
            .is_some_and(|state| !state.admission_open)
        {
            return Err(AdapterError::new(
                AdapterErrorKind::Persistence,
                "persistent server admission is closed",
            ));
        }
        if let Some(persistence) = self.session.persistence_status()
            && (!persistence.worker_alive
                || !persistence.accepting_turns
                || persistence.last_error.is_some())
        {
            let detail = persistence.last_error.as_ref().map_or_else(
                || "persistence worker is unavailable".to_owned(),
                |error| format!("persistence worker recorded `{error}`"),
            );
            let error = AdapterError::new(AdapterErrorKind::Persistence, detail);
            self.fail_persistent(&error);
            return Err(error);
        }
        Ok(())
    }

    fn durability(&self, class: ServerTurnClass) -> ServerTurnDurability {
        let Some(state) = &self.persistent else {
            return ServerTurnDurability::Buffered;
        };
        match class {
            ServerTurnClass::Http => state.durability.http,
            ServerTurnClass::WebSocket => state.durability.websocket,
            ServerTurnClass::Disconnect => state.durability.disconnect,
        }
    }

    fn dispatch_turn(
        &mut self,
        source_path: &str,
        payload: SourcePayload,
        class: ServerTurnClass,
    ) -> Result<ProgramSessionDispatch, AdapterError> {
        self.ensure_admission_open()?;
        let durability = self.durability(class);
        let result = self.session.dispatch(source_path, payload, durability);
        match result {
            Ok(result) => {
                self.record_persistent_accept(result.acknowledgement.as_ref())?;
                Ok(result.dispatched)
            }
            Err(error) => {
                self.record_persistent_rejection(&error);
                Err(error)
            }
        }
    }

    async fn settle_transient_effects(
        &mut self,
        initial: Vec<TransientEffectInvocation>,
        class: ServerTurnClass,
    ) -> Result<(), AdapterError> {
        if initial.is_empty() {
            return Ok(());
        }
        if !self.active_transient_effects.is_empty() {
            return Err(AdapterError::new(
                AdapterErrorKind::Runtime,
                "a server transaction started while transient effects from another transaction remained active",
            ));
        }
        if let Err(error) = self.register_transient_calls(&initial) {
            self.cancel_unregistered_transient_effects(&initial);
            return Err(error);
        }
        let mut calls = initial;
        let mut completed_count = 0usize;
        let mut round_count = 0usize;

        while !calls.is_empty() {
            round_count = round_count.saturating_add(1);
            if round_count > self.transient_effect_limits.max_chained_rounds
                || calls.len() > self.transient_effect_limits.max_calls_per_round
                || completed_count.saturating_add(calls.len())
                    > self.transient_effect_limits.max_calls_per_transaction
            {
                self.cancel_active_transient_effects();
                return Err(AdapterError::new(
                    AdapterErrorKind::Runtime,
                    "transient effect transaction exceeded its bounded call or chained-round limit",
                ));
            }
            let call_order = calls.iter().map(|call| call.call_id).collect::<Vec<_>>();
            let result = match self.transient_effect_host.as_mut() {
                Some(host) => host.execute_batch(calls).await,
                None => {
                    self.cancel_active_transient_effects();
                    return Err(AdapterError::new(
                        AdapterErrorKind::Unsupported,
                        "trusted server emitted a transient effect without an attached host",
                    ));
                }
            };
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    self.cancel_active_transient_effects();
                    return Err(AdapterError::new(AdapterErrorKind::Runtime, error));
                }
            };
            let settled = match validate_effect_batch_result(&call_order, result) {
                Ok(settled) => settled,
                Err(error) => {
                    self.cancel_active_transient_effects();
                    return Err(error);
                }
            };
            let mut chained = Vec::new();
            for call_id in call_order {
                match settled
                    .get(&call_id)
                    .expect("validated effect batch covers every call")
                {
                    SettledTransientEffect::Cancelled => {
                        if let Err(error) = self.session.cancel_transient_effect(call_id) {
                            self.cancel_active_transient_effects();
                            return Err(error);
                        }
                        self.active_transient_effects.remove(&call_id);
                    }
                    SettledTransientEffect::Completed(outcome) => {
                        let durability = self.durability(class);
                        let turn = match self.session.complete_transient_effect(
                            call_id,
                            outcome.clone(),
                            durability,
                        ) {
                            Ok(turn) => turn,
                            Err(error) => {
                                self.cancel_active_transient_effects();
                                return Err(error);
                            }
                        };
                        self.active_transient_effects.remove(&call_id);
                        if let Err(error) =
                            self.record_persistent_accept(turn.acknowledgement.as_ref())
                        {
                            self.cancel_unregistered_transient_effects(
                                &turn.runtime_turn.transient_effects,
                            );
                            self.cancel_active_transient_effects();
                            return Err(error);
                        }
                        if let Err(error) =
                            self.register_transient_calls(&turn.runtime_turn.transient_effects)
                        {
                            self.cancel_unregistered_transient_effects(
                                &turn.runtime_turn.transient_effects,
                            );
                            self.cancel_active_transient_effects();
                            return Err(error);
                        }
                        chained.extend(turn.runtime_turn.transient_effects);
                    }
                }
                completed_count = completed_count.saturating_add(1);
            }
            calls = chained;
        }

        if !self.active_transient_effects.is_empty()
            || self.session.pending_transient_effect_count() != 0
        {
            self.cancel_active_transient_effects();
            return Err(AdapterError::new(
                AdapterErrorKind::Runtime,
                "transient effect host completed a transaction with pending runtime calls",
            ));
        }
        Ok(())
    }

    fn register_transient_calls(
        &mut self,
        calls: &[TransientEffectInvocation],
    ) -> Result<(), AdapterError> {
        let Some(host) = self.transient_effect_host.as_ref() else {
            return Err(AdapterError::new(
                AdapterErrorKind::Unsupported,
                "trusted server emitted a transient effect without an attached host",
            ));
        };
        let mut batch = BTreeSet::new();
        for call in calls {
            if !host.owns(call.effect_id) {
                return Err(AdapterError::new(
                    AdapterErrorKind::Unsupported,
                    format_args!(
                        "transient effect host does not own effect {}",
                        call.effect_id
                    ),
                ));
            }
            if self.active_transient_effects.contains(&call.call_id) || !batch.insert(call.call_id)
            {
                return Err(AdapterError::new(
                    AdapterErrorKind::Runtime,
                    format_args!("duplicate transient effect call {}", call.call_id),
                ));
            }
        }
        self.active_transient_effects.extend(batch);
        Ok(())
    }

    fn cancel_unregistered_transient_effects(&mut self, calls: &[TransientEffectInvocation]) {
        for call in calls {
            if !self.active_transient_effects.contains(&call.call_id) {
                let _ = self.session.cancel_transient_effect(call.call_id);
            }
        }
    }

    fn cancel_active_transient_effects(&mut self) {
        let active = std::mem::take(&mut self.active_transient_effects);
        if let Some(host) = self.transient_effect_host.as_mut() {
            for call_id in &active {
                host.cancel(*call_id);
            }
        }
        for call_id in active {
            let _ = self.session.cancel_transient_effect(call_id);
        }
    }

    fn record_persistent_accept(
        &mut self,
        acknowledgement: Option<&CommitAck>,
    ) -> Result<(), AdapterError> {
        let persistence = self.session.persistence_status();
        let Some(state) = self.persistent.as_mut() else {
            return Ok(());
        };
        let worker_failure = persistence.as_ref().and_then(|persistence| {
            (!persistence.worker_alive || !persistence.accepting_turns).then(|| {
                persistence.last_error.as_ref().map_or_else(
                    || "persistence worker stopped accepting turns".to_owned(),
                    |error| format!("persistence worker stopped after `{error}`"),
                )
            })
        });
        let mut status = state
            .lifecycle
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        status.accepted_turns = status.accepted_turns.saturating_add(1);
        if let Some(acknowledgement) = acknowledgement {
            status.durably_acknowledged_turns = status.durably_acknowledged_turns.saturating_add(1);
            status.last_acknowledged_epoch = Some(acknowledgement.epoch);
        }
        if let Some(persistence) = persistence {
            status.persistence = persistence;
        }
        if let Some(worker_failure) = worker_failure {
            let error = AdapterError::new(AdapterErrorKind::Persistence, worker_failure);
            state.admission_open = false;
            status.phase = ServerLifecyclePhase::Failed;
            status.accepting_turns = false;
            status.last_error = Some(error.diagnostic().to_owned());
            return Err(error);
        }
        Ok(())
    }

    fn record_persistent_rejection(&mut self, error: &AdapterError) {
        let persistence = self.session.persistence_status();
        let Some(state) = self.persistent.as_mut() else {
            return;
        };
        let terminal = matches!(error.kind(), AdapterErrorKind::Persistence);
        if terminal {
            state.admission_open = false;
        }
        let mut status = state
            .lifecycle
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        status.rejected_turns = status.rejected_turns.saturating_add(1);
        status.last_error = Some(error.diagnostic().to_owned());
        if terminal {
            status.phase = ServerLifecyclePhase::Failed;
            status.accepting_turns = false;
        }
        if let Some(persistence) = persistence {
            status.persistence = persistence;
        }
    }

    fn fail_persistent(&mut self, error: &AdapterError) {
        let persistence = self.session.persistence_status();
        let Some(state) = self.persistent.as_mut() else {
            return;
        };
        state.admission_open = false;
        let mut status = state
            .lifecycle
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        status.phase = ServerLifecyclePhase::Failed;
        status.accepting_turns = false;
        status.last_error = Some(error.diagnostic().to_owned());
        if let Some(persistence) = persistence {
            status.persistence = persistence;
        }
    }

    async fn handle_http(&mut self, request: HttpRequest) -> Result<HttpResponse, AdapterError> {
        let (source_path, output_name, response_schema) = {
            let binding = self.http.as_ref().ok_or_else(|| {
                AdapterError::new(
                    AdapterErrorKind::Unsupported,
                    "trusted server artifact declares no HTTP host port",
                )
            })?;
            (
                binding.request.path.clone(),
                binding.response.name.clone(),
                binding.response_schema.clone(),
            )
        };
        let payload = http_request_payload(request)?;

        let dispatched = self.dispatch_turn(&source_path, payload, ServerTurnClass::Http)?;
        if dispatched.runtime_turn.source_sequence != Some(dispatched.source_sequence) {
            let error = AdapterError::new(
                AdapterErrorKind::Runtime,
                "dispatched server turn lost its source-sequence binding",
            );
            self.fail_persistent(&error);
            return Err(error);
        }
        self.settle_transient_effects(
            dispatched.runtime_turn.transient_effects,
            ServerTurnClass::Http,
        )
        .await?;
        let value = match self.session.output_value_current(&output_name) {
            Ok(value) => value,
            Err(error) => {
                self.fail_persistent(&error);
                return Err(error);
            }
        };
        let response = decode_http_response(value, &response_schema);
        if let Err(error) = &response {
            self.fail_persistent(error);
        }
        response
    }

    fn dispatch_http_disconnect(&mut self, reason: CancellationReason) -> Result<(), AdapterError> {
        let Some(source) = self
            .http
            .as_ref()
            .and_then(|binding| binding.disconnect.as_ref())
        else {
            return Ok(());
        };
        let path = source.path.clone();
        let payload = SourcePayload {
            fields: BTreeMap::from([
                ("peer".to_owned(), Value::Text("unavailable".to_owned())),
                (
                    "reason".to_owned(),
                    Value::Text(cancellation_reason(reason).to_owned()),
                ),
            ]),
            ..SourcePayload::default()
        };
        self.dispatch_turn(&path, payload, ServerTurnClass::Disconnect)
            .map(|_| ())
    }

    async fn handle_websocket(
        &mut self,
        event: WebSocketEvent,
    ) -> Result<Vec<WebSocketAction>, AdapterError> {
        let binding = self.websocket.as_ref().ok_or_else(|| {
            AdapterError::new(
                AdapterErrorKind::Unsupported,
                "trusted server artifact declares no WebSocket host port",
            )
        })?;
        let (source_path, payload) = match event {
            WebSocketEvent::Open(open) => {
                (binding.open.path.clone(), websocket_open_payload(open)?)
            }
            WebSocketEvent::Text(text) => (
                binding.message.path.clone(),
                websocket_message_payload(Some(text), None),
            ),
            WebSocketEvent::Binary(bytes) => (
                binding.message.path.clone(),
                websocket_message_payload(None, Some(bytes)),
            ),
            WebSocketEvent::Close(close) => {
                (binding.close.path.clone(), websocket_close_payload(close)?)
            }
            WebSocketEvent::TransportError(error) => (
                binding.error.path.clone(),
                websocket_transport_error_payload(error),
            ),
        };
        let output_name = binding.actions.name.clone();

        let dispatched = self.dispatch_turn(&source_path, payload, ServerTurnClass::WebSocket)?;
        if dispatched.runtime_turn.source_sequence != Some(dispatched.source_sequence) {
            let error = AdapterError::new(
                AdapterErrorKind::Runtime,
                "dispatched WebSocket turn lost its source-sequence binding",
            );
            self.fail_persistent(&error);
            return Err(error);
        }
        self.settle_transient_effects(
            dispatched.runtime_turn.transient_effects,
            ServerTurnClass::WebSocket,
        )
        .await?;
        let value = match self.session.output_value_current(&output_name) {
            Ok(value) => value,
            Err(error) => {
                self.fail_persistent(&error);
                return Err(error);
            }
        };
        let actions = decode_websocket_actions(value);
        if let Err(error) = &actions {
            self.fail_persistent(error);
        }
        actions
    }

    fn record_failure(&mut self, error: AdapterError, status: u16) -> HttpResponse {
        if matches!(
            error.kind(),
            AdapterErrorKind::Runtime
                | AdapterErrorKind::Persistence
                | AdapterErrorKind::InvalidOutput
        ) {
            self.fail_persistent(&error);
        }
        let response = diagnostic_http_response(status, &error);
        self.last_diagnostic = Some(error);
        response
    }

    fn record_websocket_failure(
        &mut self,
        error: AdapterError,
        opening: bool,
    ) -> Vec<WebSocketAction> {
        if matches!(
            error.kind(),
            AdapterErrorKind::Runtime
                | AdapterErrorKind::Persistence
                | AdapterErrorKind::InvalidOutput
        ) {
            self.fail_persistent(&error);
        }
        let status = match error.kind() {
            AdapterErrorKind::Unsupported => 501,
            AdapterErrorKind::Backpressure | AdapterErrorKind::Persistence => 503,
            _ => 500,
        };
        let actions = if opening {
            vec![WebSocketAction::Reject(diagnostic_http_response(
                status, &error,
            ))]
        } else {
            vec![WebSocketAction::Close(WebSocketClose::new(
                1011,
                "invalid Boon WebSocket output",
            ))]
        };
        self.last_diagnostic = Some(error);
        actions
    }

    fn shutdown_persistent(&mut self) -> Result<(), AdapterError> {
        let Some(state) = self.persistent.as_mut() else {
            return Ok(());
        };
        if state.shutdown_complete {
            return Ok(());
        }
        state.admission_open = false;
        let lifecycle = state.lifecycle.clone();
        {
            let mut status = lifecycle
                .status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            status.phase = ServerLifecyclePhase::ShuttingDown;
            status.accepting_turns = false;
        }

        let barrier = self.session.barrier();
        let shutdown = self.session.shutdown();
        let persistence = self.session.persistence_status();
        let result = match (barrier, shutdown) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(barrier), Ok(())) => Err(barrier),
            (Ok(()), Err(shutdown)) => Err(shutdown),
            (Err(barrier), Err(shutdown)) => Err(AdapterError::new(
                AdapterErrorKind::Persistence,
                format_args!(
                    "shutdown barrier failed with `{barrier}` and worker shutdown failed with `{shutdown}`"
                ),
            )),
        };

        let mut status = lifecycle
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(persistence) = persistence {
            status.persistence = persistence;
        }
        match &result {
            Ok(()) => {
                status.phase = ServerLifecyclePhase::Stopped;
                status.last_error = None;
                if let Some(state) = self.persistent.as_mut() {
                    state.shutdown_complete = true;
                }
            }
            Err(error) => {
                status.phase = ServerLifecyclePhase::Failed;
                status.last_error = Some(error.diagnostic().to_owned());
                self.last_diagnostic = Some(error.clone());
            }
        }
        result
    }
}

#[async_trait]
impl ServerProgram for BoonServerProgram {
    async fn on_http(
        &mut self,
        request: HttpRequest,
        cancellation: CallCancellation,
    ) -> HttpResponse {
        if let Some(reason) = cancellation.reason() {
            let error = AdapterError::new(
                AdapterErrorKind::InvalidRequest,
                format_args!(
                    "request was cancelled before dispatch: {}",
                    cancellation_reason(reason)
                ),
            );
            return self.record_failure(error, 408);
        }
        match self.handle_http(request).await {
            Ok(response) => response,
            Err(error) => {
                let status = match error.kind() {
                    AdapterErrorKind::InvalidRequest => 400,
                    AdapterErrorKind::Unsupported => 501,
                    AdapterErrorKind::Backpressure | AdapterErrorKind::Persistence => 503,
                    _ => 500,
                };
                self.record_failure(error, status)
            }
        }
    }

    async fn on_websocket(
        &mut self,
        event: WebSocketEvent,
        cancellation: CallCancellation,
    ) -> Vec<WebSocketAction> {
        let opening = matches!(&event, WebSocketEvent::Open(_));
        if let Some(reason) = cancellation.reason() {
            let error = AdapterError::new(
                AdapterErrorKind::InvalidRequest,
                format_args!(
                    "WebSocket event was cancelled before dispatch: {}",
                    cancellation_reason(reason)
                ),
            );
            return self.record_websocket_failure(error, opening);
        }
        match self.handle_websocket(event).await {
            Ok(actions) => actions,
            Err(error) => self.record_websocket_failure(error, opening),
        }
    }

    async fn on_http_cancelled(&mut self, reason: CancellationReason) {
        self.cancel_active_transient_effects();
        if let Err(error) = self.dispatch_http_disconnect(reason) {
            self.last_diagnostic = Some(error);
        }
    }

    async fn on_websocket_cancelled(&mut self, _reason: CancellationReason) {
        self.cancel_active_transient_effects();
    }

    async fn on_shutdown(&mut self) {
        self.cancel_active_transient_effects();
        if let Some(host) = self.transient_effect_host.as_mut() {
            host.shutdown();
        }
        if let Err(error) = self.shutdown_persistent() {
            self.last_diagnostic = Some(error);
        }
    }
}

fn validate_server_artifact(artifact: &ProgramArtifact) -> Result<(), AdapterError> {
    if artifact.capability_profile() != ProgramCapabilityProfile::TrustedServer {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidArtifact,
            format_args!(
                "expected trusted_server capability, found {}",
                artifact.capability_profile().name()
            ),
        ));
    }
    if artifact.role() != ProgramRole::Server {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidArtifact,
            format_args!("expected server role, found {}", artifact.role().as_str()),
        ));
    }
    Ok(())
}

fn resolve_bindings(plan: &MachinePlan) -> Result<ResolvedBindings, AdapterError> {
    let mut bindings = ResolvedBindings::default();
    for port in &plan.host_ports {
        match port {
            HostPortPlan::HttpServer {
                request_source,
                disconnect_source,
                response_output,
            } => {
                if bindings.http.is_some() {
                    return Err(AdapterError::new(
                        AdapterErrorKind::InvalidHostPort,
                        "artifact declares more than one HTTP host port",
                    ));
                }
                let request_route = resolve_source(plan, *request_source, "HTTP request")?;
                validate_source_route(request_route, &http_request_schema(), "HTTP request")?;
                let request = ResolvedSource {
                    path: request_route.path.clone(),
                };
                let disconnect = disconnect_source
                    .map(|id| {
                        let route = resolve_source(plan, id, "HTTP disconnect")?;
                        validate_source_route(
                            route,
                            &BTreeMap::from([
                                (named_payload_field("peer"), DataTypePlan::Text),
                                (named_payload_field("reason"), DataTypePlan::Text),
                            ]),
                            "HTTP disconnect",
                        )?;
                        Ok(ResolvedSource {
                            path: route.path.clone(),
                        })
                    })
                    .transpose()?;
                let response_output = resolve_output(plan, *response_output, "HTTP response")?;
                let response_schema = validate_http_response_type(response_output)?;
                bindings.http = Some(HttpPortBinding {
                    request,
                    disconnect,
                    response: ResolvedOutput {
                        name: response_output.name.clone(),
                    },
                    response_schema,
                });
            }
            HostPortPlan::WebSocketServer {
                open_source,
                message_source,
                close_source,
                error_source,
                actions_output,
            } => {
                if bindings.websocket.is_some() {
                    return Err(AdapterError::new(
                        AdapterErrorKind::InvalidHostPort,
                        "artifact declares more than one WebSocket host port",
                    ));
                }
                let open_route = resolve_source(plan, *open_source, "WebSocket open")?;
                validate_source_route(open_route, &websocket_open_schema(), "WebSocket open")?;
                let message_route = resolve_source(plan, *message_source, "WebSocket message")?;
                validate_source_route(
                    message_route,
                    &websocket_message_schema(),
                    "WebSocket message",
                )?;
                let close_route = resolve_source(plan, *close_source, "WebSocket close")?;
                validate_source_route(close_route, &websocket_close_schema(), "WebSocket close")?;
                let error_route = resolve_source(plan, *error_source, "WebSocket error")?;
                validate_source_route(error_route, &websocket_error_schema(), "WebSocket error")?;
                let actions = resolve_output(plan, *actions_output, "WebSocket actions")?;
                validate_websocket_actions_type(actions)?;
                bindings.websocket = Some(WebSocketPortBinding {
                    open: ResolvedSource {
                        path: open_route.path.clone(),
                    },
                    message: ResolvedSource {
                        path: message_route.path.clone(),
                    },
                    close: ResolvedSource {
                        path: close_route.path.clone(),
                    },
                    error: ResolvedSource {
                        path: error_route.path.clone(),
                    },
                    actions: ResolvedOutput {
                        name: actions.name.clone(),
                    },
                });
            }
        }
    }
    if bindings.http.is_none() && bindings.websocket.is_none() {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidHostPort,
            "trusted server artifact declares no host ports",
        ));
    }
    Ok(bindings)
}

fn resolve_source<'a>(
    plan: &'a MachinePlan,
    id: SourceId,
    label: &str,
) -> Result<&'a SourceRoute, AdapterError> {
    let mut matches = plan
        .source_routes
        .iter()
        .filter(|route| route.source_id == id);
    let route = matches.next().ok_or_else(|| {
        AdapterError::new(
            AdapterErrorKind::InvalidHostPort,
            format_args!("{label} references missing source ID {}", id.0),
        )
    })?;
    if matches.next().is_some() {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidHostPort,
            format_args!("{label} source ID {} is ambiguous", id.0),
        ));
    }
    if route.scoped || route.scope_id.is_some() || route.interval_ms.is_some() {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidHostPort,
            format_args!("{label} source ID {} is not an unscoped host source", id.0),
        ));
    }
    Ok(route)
}

fn resolve_output<'a>(
    plan: &'a MachinePlan,
    id: OutputRootId,
    label: &str,
) -> Result<&'a OutputRootPlan, AdapterError> {
    let mut matches = plan.outputs.iter().filter(|output| output.id == id);
    let output = matches.next().ok_or_else(|| {
        AdapterError::new(
            AdapterErrorKind::InvalidHostPort,
            format_args!("{label} references missing output ID {id}"),
        )
    })?;
    if matches.next().is_some() {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidHostPort,
            format_args!("{label} output ID {id} is ambiguous"),
        ));
    }
    if !matches!(output.contract, OutputContractKind::HostValue { .. }) {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidHostPort,
            format_args!("{label} output ID {id} is not a host-value output"),
        ));
    }
    Ok(output)
}

fn validate_source_route(
    route: &SourceRoute,
    expected: &BTreeMap<SourcePayloadField, DataTypePlan>,
    label: &str,
) -> Result<(), AdapterError> {
    let mut actual = BTreeMap::new();
    for descriptor in &route.payload_schema.typed_fields {
        if actual
            .insert(descriptor.field.clone(), descriptor.data_type.clone())
            .is_some()
        {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidHostPort,
                format_args!(
                    "{label} source ID {} repeats a payload field",
                    route.source_id.0
                ),
            ));
        }
    }
    let declared = route
        .payload_schema
        .fields
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let expected_names = expected.keys().cloned().collect::<BTreeSet<_>>();
    if actual != *expected || declared != expected_names {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidHostPort,
            format_args!(
                "{label} source ID {} payload schema does not match the generic host contract",
                route.source_id.0
            ),
        ));
    }
    Ok(())
}

fn named_payload_field(name: &str) -> SourcePayloadField {
    SourcePayloadField::Named(name.to_owned())
}

fn http_request_schema() -> BTreeMap<SourcePayloadField, DataTypePlan> {
    let pairs = named_text_pairs_type();
    BTreeMap::from([
        (
            named_payload_field("body"),
            DataTypePlan::Bytes { fixed_len: None },
        ),
        (named_payload_field("cookies"), pairs.clone()),
        (named_payload_field("deadline_ms"), DataTypePlan::Number),
        (named_payload_field("headers"), pairs.clone()),
        (named_payload_field("method"), DataTypePlan::Text),
        (named_payload_field("path"), DataTypePlan::Text),
        (
            named_payload_field("path_segments"),
            DataTypePlan::List {
                item: Box::new(DataTypePlan::Text),
            },
        ),
        (named_payload_field("peer"), DataTypePlan::Text),
        (named_payload_field("query"), pairs),
        (named_payload_field("scheme"), DataTypePlan::Text),
    ])
}

fn named_text_pairs_type() -> DataTypePlan {
    DataTypePlan::List {
        item: Box::new(DataTypePlan::Record {
            fields: vec![
                boon_plan::DataTypeFieldPlan {
                    name: "name".to_owned(),
                    data_type: DataTypePlan::Text,
                },
                boon_plan::DataTypeFieldPlan {
                    name: "value".to_owned(),
                    data_type: DataTypePlan::Text,
                },
            ],
            open: false,
        }),
    }
}

fn websocket_open_schema() -> BTreeMap<SourcePayloadField, DataTypePlan> {
    let pairs = named_text_pairs_type();
    BTreeMap::from([
        (named_payload_field("cookies"), pairs.clone()),
        (named_payload_field("headers"), pairs.clone()),
        (named_payload_field("path"), DataTypePlan::Text),
        (
            named_payload_field("path_segments"),
            DataTypePlan::List {
                item: Box::new(DataTypePlan::Text),
            },
        ),
        (named_payload_field("peer"), DataTypePlan::Text),
        (
            named_payload_field("protocols"),
            DataTypePlan::List {
                item: Box::new(DataTypePlan::Text),
            },
        ),
        (named_payload_field("query"), pairs),
    ])
}

fn websocket_message_schema() -> BTreeMap<SourcePayloadField, DataTypePlan> {
    BTreeMap::from([
        (
            SourcePayloadField::Bytes,
            DataTypePlan::Bytes { fixed_len: None },
        ),
        (
            named_payload_field("kind"),
            DataTypePlan::Variant {
                variants: vec![
                    DataVariantPlan {
                        tag: "BinaryMessage".to_owned(),
                        fields: Vec::new(),
                        open: false,
                    },
                    DataVariantPlan {
                        tag: "TextMessage".to_owned(),
                        fields: Vec::new(),
                        open: false,
                    },
                ],
            },
        ),
        (SourcePayloadField::Text, DataTypePlan::Text),
    ])
}

fn websocket_close_schema() -> BTreeMap<SourcePayloadField, DataTypePlan> {
    BTreeMap::from([
        (named_payload_field("clean"), DataTypePlan::Bool),
        (named_payload_field("code"), DataTypePlan::Number),
        (named_payload_field("reason"), DataTypePlan::Text),
    ])
}

fn websocket_error_schema() -> BTreeMap<SourcePayloadField, DataTypePlan> {
    BTreeMap::from([
        (named_payload_field("code"), DataTypePlan::Text),
        (named_payload_field("message"), DataTypePlan::Text),
        (named_payload_field("retryable"), DataTypePlan::Bool),
    ])
}

fn websocket_action_type() -> DataTypePlan {
    let fields = [
        ("body_bytes", DataTypePlan::Bytes { fixed_len: None }),
        ("body_kind", DataTypePlan::Text),
        ("body_text", DataTypePlan::Text),
        ("bytes", DataTypePlan::Bytes { fixed_len: None }),
        ("code", DataTypePlan::Number),
        ("frame_kind", DataTypePlan::Text),
        ("include_current", DataTypePlan::Bool),
        ("kind", DataTypePlan::Text),
        ("reason", DataTypePlan::Text),
        ("room", DataTypePlan::Text),
        ("status", DataTypePlan::Number),
        ("text", DataTypePlan::Text),
    ]
    .into_iter()
    .map(|(name, data_type)| DataTypeFieldPlan {
        name: name.to_owned(),
        data_type,
    })
    .collect();
    DataTypePlan::List {
        item: Box::new(DataTypePlan::Record {
            fields,
            open: false,
        }),
    }
    .canonicalized()
}

fn validate_websocket_actions_type(output: &OutputRootPlan) -> Result<(), AdapterError> {
    let OutputContractKind::HostValue { data_type } = &output.contract else {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidHostPort,
            "WebSocket actions binding is not a host-value output",
        ));
    };
    if data_type != &websocket_action_type() {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidHostPort,
            "WebSocket actions output does not match the closed generic action envelope",
        ));
    }
    Ok(())
}

fn validate_http_response_type(
    output: &OutputRootPlan,
) -> Result<HttpResponseSchema, AdapterError> {
    let OutputContractKind::HostValue { data_type } = &output.contract else {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidHostPort,
            "HTTP response binding is not a host-value output",
        ));
    };
    let DataTypePlan::Record {
        fields,
        open: false,
    } = data_type
    else {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidHostPort,
            "HTTP response output must be a closed record",
        ));
    };
    let mut types = BTreeMap::new();
    for field in fields {
        if types
            .insert(field.name.as_str(), &field.data_type)
            .is_some()
        {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidHostPort,
                format_args!("HTTP response schema repeats field `{}`", field.name),
            ));
        }
    }
    if !matches!(types.len(), 2 | 3)
        || types
            .keys()
            .any(|name| !matches!(*name, "status" | "headers" | "body"))
    {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidHostPort,
            "HTTP response schema must contain exactly `status`, `body`, and optional `headers`",
        ));
    }
    if types.get("status") != Some(&&DataTypePlan::Number) {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidHostPort,
            "HTTP response `status` must be Number",
        ));
    }
    let body_fixed_len = match types.get("body") {
        Some(DataTypePlan::Bytes { fixed_len }) => *fixed_len,
        _ => {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidHostPort,
                "HTTP response `body` must be Bytes",
            ));
        }
    };
    let header_value = match (types.len(), types.get("headers")) {
        (2, None) => None,
        (3, Some(data_type)) => Some(header_value_type(data_type)?),
        _ => {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidHostPort,
                "HTTP response schema must contain exactly `status`, `body`, and optional `headers`",
            ));
        }
    };
    Ok(HttpResponseSchema {
        body_fixed_len,
        header_value,
    })
}

fn header_value_type(data_type: &DataTypePlan) -> Result<HeaderValueType, AdapterError> {
    let DataTypePlan::List { item } = data_type else {
        return Err(invalid_headers_schema());
    };
    let DataTypePlan::Record {
        fields,
        open: false,
    } = item.as_ref()
    else {
        return Err(invalid_headers_schema());
    };
    let fields = fields
        .iter()
        .map(|field| (field.name.as_str(), &field.data_type))
        .collect::<BTreeMap<_, _>>();
    if fields.len() != 2 || fields.get("name") != Some(&&DataTypePlan::Text) {
        return Err(invalid_headers_schema());
    }
    match fields.get("value") {
        Some(DataTypePlan::Text) => Ok(HeaderValueType::Text),
        Some(DataTypePlan::Bytes { fixed_len }) => Ok(HeaderValueType::Bytes {
            fixed_len: *fixed_len,
        }),
        _ => Err(invalid_headers_schema()),
    }
}

fn invalid_headers_schema() -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::InvalidHostPort,
        "HTTP response `headers` must be a closed list of `{ name: Text, value: Text|Bytes }` records",
    )
}

fn http_request_payload(request: HttpRequest) -> Result<SourcePayload, AdapterError> {
    validate_normalized_segments(&request.path_segments)?;
    let path = if request.path_segments.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}", request.path_segments.join("/"))
    };
    let path_segments = request
        .path_segments
        .into_iter()
        .map(Value::Text)
        .collect::<Vec<_>>();
    let query = request
        .query
        .into_iter()
        .flat_map(|(name, values)| {
            values
                .into_iter()
                .map(move |value| named_text_pair(name.clone(), value))
        })
        .collect::<Vec<_>>();
    let headers = request
        .headers
        .into_iter()
        .map(|header| {
            let value = String::from_utf8(header.value).map_err(|_| {
                AdapterError::new(
                    AdapterErrorKind::InvalidRequest,
                    format_args!("allowlisted header `{}` is not UTF-8", header.name),
                )
            })?;
            Ok(named_text_pair(header.name, value))
        })
        .collect::<Result<Vec<_>, AdapterError>>()?;
    let peer = match request.peer {
        PeerAddress::Known(address) => address.to_string(),
        PeerAddress::Unavailable => "unavailable".to_owned(),
    };
    let deadline_ms = request
        .deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .min(MAX_EXACT_INTEGER);
    let deadline_ms = Value::integer(deadline_ms as i64)
        .map_err(|error| AdapterError::new(AdapterErrorKind::InvalidRequest, error))?;

    Ok(SourcePayload {
        fields: BTreeMap::from([
            ("body".to_owned(), Value::Bytes(request.body)),
            ("cookies".to_owned(), Value::List(Vec::new())),
            ("deadline_ms".to_owned(), deadline_ms),
            ("headers".to_owned(), Value::List(headers)),
            ("method".to_owned(), Value::Text(request.method)),
            ("path".to_owned(), Value::Text(path)),
            ("path_segments".to_owned(), Value::List(path_segments)),
            ("peer".to_owned(), Value::Text(peer)),
            ("query".to_owned(), Value::List(query)),
            ("scheme".to_owned(), Value::Text("http".to_owned())),
        ]),
        ..SourcePayload::default()
    })
}

fn websocket_open_payload(open: WebSocketOpen) -> Result<SourcePayload, AdapterError> {
    validate_normalized_segments(&open.path_segments)?;
    let path = normalized_path(&open.path_segments);
    let path_segments = open
        .path_segments
        .into_iter()
        .map(Value::Text)
        .collect::<Vec<_>>();
    let query = named_query_pairs(open.query);
    let mut headers = Vec::with_capacity(open.headers.len());
    let mut protocols = Vec::new();
    for header in open.headers {
        let value = String::from_utf8(header.value).map_err(|_| {
            AdapterError::new(
                AdapterErrorKind::InvalidRequest,
                format_args!(
                    "allowlisted WebSocket header `{}` is not UTF-8",
                    header.name
                ),
            )
        })?;
        if header.name.eq_ignore_ascii_case("sec-websocket-protocol") {
            protocols.extend(
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|protocol| !protocol.is_empty())
                    .map(|protocol| Value::Text(protocol.to_owned())),
            );
        }
        headers.push(named_text_pair(header.name, value));
    }
    let peer = peer_text(open.peer);
    Ok(SourcePayload {
        fields: BTreeMap::from([
            ("cookies".to_owned(), Value::List(Vec::new())),
            ("headers".to_owned(), Value::List(headers)),
            ("path".to_owned(), Value::Text(path)),
            ("path_segments".to_owned(), Value::List(path_segments)),
            ("peer".to_owned(), Value::Text(peer)),
            ("protocols".to_owned(), Value::List(protocols)),
            ("query".to_owned(), Value::List(query)),
        ]),
        ..SourcePayload::default()
    })
}

fn websocket_message_payload(text: Option<String>, bytes: Option<Vec<u8>>) -> SourcePayload {
    let (kind, text, bytes) = match (text, bytes) {
        (Some(text), None) => ("TextMessage", text, Vec::new()),
        (None, Some(bytes)) => ("BinaryMessage", String::new(), bytes),
        _ => unreachable!("WebSocket message payload has exactly one frame representation"),
    };
    SourcePayload {
        text: Some(text),
        fields: BTreeMap::from([
            ("bytes".to_owned(), Value::Bytes(bytes)),
            ("kind".to_owned(), Value::Text(kind.to_owned())),
        ]),
        ..SourcePayload::default()
    }
}

fn websocket_close_payload(close: Option<WebSocketClose>) -> Result<SourcePayload, AdapterError> {
    let (code, reason, clean) = close.map_or((1005, String::new(), false), |close| {
        (close.code, close.reason, true)
    });
    let code = Value::integer(i64::from(code))
        .map_err(|error| AdapterError::new(AdapterErrorKind::InvalidRequest, error))?;
    Ok(SourcePayload {
        fields: BTreeMap::from([
            ("clean".to_owned(), Value::Bool(clean)),
            ("code".to_owned(), code),
            ("reason".to_owned(), Value::Text(reason)),
        ]),
        ..SourcePayload::default()
    })
}

fn websocket_transport_error_payload(error: WebSocketTransportError) -> SourcePayload {
    let (code, message, retryable) = match error {
        WebSocketTransportError::MessageTooLarge => (
            "message_too_large",
            "WebSocket message exceeded its limit",
            false,
        ),
        WebSocketTransportError::InvalidMessage => {
            ("invalid_message", "WebSocket message was invalid", false)
        }
        WebSocketTransportError::Io => ("io", "WebSocket transport I/O failed", true),
        WebSocketTransportError::ProgramTimeout => {
            ("program_timeout", "WebSocket program call timed out", true)
        }
        WebSocketTransportError::AdmissionOverloaded => (
            "admission_overloaded",
            "WebSocket event admission was overloaded",
            true,
        ),
    };
    SourcePayload {
        fields: BTreeMap::from([
            ("code".to_owned(), Value::Text(code.to_owned())),
            ("message".to_owned(), Value::Text(message.to_owned())),
            ("retryable".to_owned(), Value::Bool(retryable)),
        ]),
        ..SourcePayload::default()
    }
}

fn normalized_path(segments: &[String]) -> String {
    if segments.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}", segments.join("/"))
    }
}

fn named_query_pairs(query: BTreeMap<String, Vec<String>>) -> Vec<Value> {
    query
        .into_iter()
        .flat_map(|(name, values)| {
            values
                .into_iter()
                .map(move |value| named_text_pair(name.clone(), value))
        })
        .collect()
}

fn peer_text(peer: PeerAddress) -> String {
    match peer {
        PeerAddress::Known(address) => address.to_string(),
        PeerAddress::Unavailable => "unavailable".to_owned(),
    }
}

fn validate_normalized_segments(segments: &[String]) -> Result<(), AdapterError> {
    if let Some(segment) = segments.iter().find(|segment| {
        segment.is_empty()
            || matches!(segment.as_str(), "." | "..")
            || segment.contains(['/', '\\', '\0'])
    }) {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidRequest,
            format_args!(
                "path contains a non-normalized segment of {} bytes",
                segment.len()
            ),
        ));
    }
    Ok(())
}

fn named_text_pair(name: String, value: String) -> Value {
    Value::Record(BTreeMap::from([
        ("name".to_owned(), Value::Text(name)),
        ("value".to_owned(), Value::Text(value)),
    ]))
}

fn decode_http_response(
    value: Value,
    schema: &HttpResponseSchema,
) -> Result<HttpResponse, AdapterError> {
    let Value::Record(mut fields) = value else {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidOutput,
            "HTTP response output is not a record",
        ));
    };
    let actual = fields.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if actual != schema.field_names() {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidOutput,
            "HTTP response fields differ from the declared closed schema",
        ));
    }
    let status = decode_status(
        fields
            .remove("status")
            .expect("closed response field set contains status"),
    )?;
    let headers = match (&schema.header_value, fields.remove("headers")) {
        (Some(value_type), Some(value)) => decode_headers(value, value_type)?,
        (None, None) => Vec::new(),
        _ => {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidOutput,
                "HTTP response headers differ from the declared schema",
            ));
        }
    };
    let body = decode_body(
        fields
            .remove("body")
            .expect("closed response field set contains body"),
        schema.body_fixed_len,
    )?;
    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

#[derive(Clone, Debug)]
struct WebSocketActionEnvelope {
    kind: String,
    status: i64,
    body_kind: String,
    body_text: String,
    body_bytes: Vec<u8>,
    frame_kind: String,
    text: String,
    bytes: Vec<u8>,
    room: String,
    include_current: bool,
    code: i64,
    reason: String,
}

fn decode_websocket_actions(value: Value) -> Result<Vec<WebSocketAction>, AdapterError> {
    let Value::List(values) = value else {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidOutput,
            "WebSocket actions output is not a list",
        ));
    };
    if values.len() > MAX_WEBSOCKET_ACTIONS {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidOutput,
            format_args!("WebSocket action count exceeds the {MAX_WEBSOCKET_ACTIONS} action limit"),
        ));
    }
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| decode_websocket_action(value, index))
        .collect()
}

fn decode_websocket_action(value: Value, index: usize) -> Result<WebSocketAction, AdapterError> {
    let Value::Record(mut fields) = value else {
        return Err(invalid_action(index, "action is not a record"));
    };
    let expected = BTreeSet::from([
        "body_bytes",
        "body_kind",
        "body_text",
        "bytes",
        "code",
        "frame_kind",
        "include_current",
        "kind",
        "reason",
        "room",
        "status",
        "text",
    ]);
    if fields.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected {
        return Err(invalid_action(
            index,
            "fields differ from the closed action envelope",
        ));
    }
    let envelope = WebSocketActionEnvelope {
        kind: take_action_text(&mut fields, "kind", index)?,
        status: take_action_integer(&mut fields, "status", index)?,
        body_kind: take_action_text(&mut fields, "body_kind", index)?,
        body_text: take_action_text(&mut fields, "body_text", index)?,
        body_bytes: take_action_bytes(&mut fields, "body_bytes", index)?,
        frame_kind: take_action_text(&mut fields, "frame_kind", index)?,
        text: take_action_text(&mut fields, "text", index)?,
        bytes: take_action_bytes(&mut fields, "bytes", index)?,
        room: take_action_text(&mut fields, "room", index)?,
        include_current: take_action_bool(&mut fields, "include_current", index)?,
        code: take_action_integer(&mut fields, "code", index)?,
        reason: take_action_text(&mut fields, "reason", index)?,
    };
    validate_action_envelope_bounds(&envelope, index)?;

    match envelope.kind.as_str() {
        "Accept" => {
            validate_inactive_action_fields(&envelope, index, 0)?;
            Ok(WebSocketAction::Accept)
        }
        "Reject" => {
            validate_inactive_action_fields(
                &envelope,
                index,
                ACTION_FIELD_STATUS | ACTION_FIELD_BODY,
            )?;
            Ok(WebSocketAction::Reject(HttpResponse {
                status: http_status(envelope.status, index)?,
                headers: Vec::new(),
                body: action_body(&envelope, index)?,
            }))
        }
        "Reply" => {
            validate_inactive_action_fields(&envelope, index, ACTION_FIELD_FRAME)?;
            Ok(WebSocketAction::Reply(action_frame(&envelope, index)?))
        }
        "Send" => {
            validate_inactive_action_fields(&envelope, index, ACTION_FIELD_FRAME)?;
            Ok(WebSocketAction::Send(action_frame(&envelope, index)?))
        }
        "JoinRoom" => {
            validate_inactive_action_fields(&envelope, index, ACTION_FIELD_ROOM)?;
            Ok(WebSocketAction::JoinRoom {
                room: action_room(&envelope, index)?.to_owned(),
            })
        }
        "LeaveRoom" => {
            validate_inactive_action_fields(&envelope, index, ACTION_FIELD_ROOM)?;
            Ok(WebSocketAction::LeaveRoom {
                room: action_room(&envelope, index)?.to_owned(),
            })
        }
        "Broadcast" => {
            validate_inactive_action_fields(
                &envelope,
                index,
                ACTION_FIELD_FRAME | ACTION_FIELD_ROOM | ACTION_FIELD_INCLUDE_CURRENT,
            )?;
            Ok(WebSocketAction::Broadcast {
                room: action_room(&envelope, index)?.to_owned(),
                frame: action_frame(&envelope, index)?,
                include_current: envelope.include_current,
            })
        }
        "RequestResync" => {
            validate_inactive_action_fields(&envelope, index, ACTION_FIELD_FRAME)?;
            Ok(WebSocketAction::RequestResync {
                frame: action_frame(&envelope, index)?,
            })
        }
        "Close" => {
            validate_inactive_action_fields(&envelope, index, ACTION_FIELD_CLOSE)?;
            Ok(WebSocketAction::Close(WebSocketClose::new(
                websocket_close_code(envelope.code, index)?,
                envelope.reason,
            )))
        }
        _ => Err(invalid_action(index, "kind is not a supported action")),
    }
}

fn validate_inactive_action_fields(
    envelope: &WebSocketActionEnvelope,
    index: usize,
    active: u16,
) -> Result<(), AdapterError> {
    let non_neutral = [
        ("status", ACTION_FIELD_STATUS, envelope.status != 0),
        ("body", ACTION_FIELD_BODY, !envelope.body_kind.is_empty()),
        ("frame", ACTION_FIELD_FRAME, !envelope.frame_kind.is_empty()),
        ("room", ACTION_FIELD_ROOM, !envelope.room.is_empty()),
        (
            "include_current",
            ACTION_FIELD_INCLUDE_CURRENT,
            envelope.include_current,
        ),
        (
            "close",
            ACTION_FIELD_CLOSE,
            envelope.code != 0 || !envelope.reason.is_empty(),
        ),
    ]
    .into_iter()
    .find(|(_, field, non_neutral)| active & field == 0 && *non_neutral);
    if let Some((name, _, _)) = non_neutral {
        return Err(invalid_action(
            index,
            format_args!("inactive `{name}` fields are not neutral"),
        ));
    }
    Ok(())
}

fn take_action_text(
    fields: &mut BTreeMap<String, Value>,
    name: &str,
    index: usize,
) -> Result<String, AdapterError> {
    match fields.remove(name) {
        Some(Value::Text(value)) => Ok(value),
        _ => Err(invalid_action(index, format_args!("`{name}` is not Text"))),
    }
}

fn take_action_bytes(
    fields: &mut BTreeMap<String, Value>,
    name: &str,
    index: usize,
) -> Result<Vec<u8>, AdapterError> {
    match fields.remove(name) {
        Some(Value::Bytes(value)) => Ok(value),
        _ => Err(invalid_action(index, format_args!("`{name}` is not Bytes"))),
    }
}

fn take_action_bool(
    fields: &mut BTreeMap<String, Value>,
    name: &str,
    index: usize,
) -> Result<bool, AdapterError> {
    match fields.remove(name) {
        Some(Value::Bool(value)) => Ok(value),
        _ => Err(invalid_action(index, format_args!("`{name}` is not Bool"))),
    }
}

fn take_action_integer(
    fields: &mut BTreeMap<String, Value>,
    name: &str,
    index: usize,
) -> Result<i64, AdapterError> {
    let Some(Value::Number(value)) = fields.remove(name) else {
        return Err(invalid_action(
            index,
            format_args!("`{name}` is not Number"),
        ));
    };
    value
        .to_i64_exact()
        .map_err(|_| invalid_action(index, format_args!("`{name}` is not a whole i64")))
}

fn validate_action_envelope_bounds(
    envelope: &WebSocketActionEnvelope,
    index: usize,
) -> Result<(), AdapterError> {
    for (name, actual, limit) in [
        ("kind", envelope.kind.len(), MAX_ACTION_KIND_BYTES),
        ("body_kind", envelope.body_kind.len(), MAX_ACTION_KIND_BYTES),
        (
            "body_text",
            envelope.body_text.len(),
            MAX_WEBSOCKET_REJECT_BODY_BYTES,
        ),
        (
            "body_bytes",
            envelope.body_bytes.len(),
            MAX_WEBSOCKET_REJECT_BODY_BYTES,
        ),
        (
            "frame_kind",
            envelope.frame_kind.len(),
            MAX_ACTION_KIND_BYTES,
        ),
        ("text", envelope.text.len(), MAX_WEBSOCKET_FRAME_BYTES),
        ("bytes", envelope.bytes.len(), MAX_WEBSOCKET_FRAME_BYTES),
        ("room", envelope.room.len(), MAX_WEBSOCKET_ROOM_BYTES),
        (
            "reason",
            envelope.reason.len(),
            MAX_WEBSOCKET_CLOSE_REASON_BYTES,
        ),
    ] {
        if actual > limit {
            return Err(invalid_action(
                index,
                format_args!("`{name}` exceeds its {limit} byte limit"),
            ));
        }
    }
    Ok(())
}

fn action_body(envelope: &WebSocketActionEnvelope, index: usize) -> Result<Vec<u8>, AdapterError> {
    match envelope.body_kind.as_str() {
        "Text" if envelope.body_bytes.is_empty() => Ok(envelope.body_text.as_bytes().to_vec()),
        "Binary" if envelope.body_text.is_empty() => Ok(envelope.body_bytes.clone()),
        _ => Err(invalid_action(
            index,
            "reject body kind or inactive body field is invalid",
        )),
    }
}

fn action_frame(
    envelope: &WebSocketActionEnvelope,
    index: usize,
) -> Result<WebSocketFrame, AdapterError> {
    match envelope.frame_kind.as_str() {
        "Text" if envelope.bytes.is_empty() => Ok(WebSocketFrame::Text(envelope.text.clone())),
        "Binary" if envelope.text.is_empty() => Ok(WebSocketFrame::Binary(envelope.bytes.clone())),
        _ => Err(invalid_action(
            index,
            "frame kind or inactive frame field is invalid",
        )),
    }
}

fn action_room(envelope: &WebSocketActionEnvelope, index: usize) -> Result<&str, AdapterError> {
    if envelope.room.is_empty() {
        Err(invalid_action(index, "room is empty"))
    } else {
        Ok(&envelope.room)
    }
}

fn http_status(status: i64, index: usize) -> Result<u16, AdapterError> {
    let status = u16::try_from(status)
        .map_err(|_| invalid_action(index, "reject status is outside the u16 range"))?;
    if !(100..=999).contains(&status) {
        return Err(invalid_action(
            index,
            "reject status is outside the HTTP status range",
        ));
    }
    Ok(status)
}

fn websocket_close_code(code: i64, index: usize) -> Result<u16, AdapterError> {
    let code = u16::try_from(code)
        .map_err(|_| invalid_action(index, "close code is outside the u16 range"))?;
    if !valid_websocket_close_code(code) {
        return Err(invalid_action(index, "close code is not valid on the wire"));
    }
    Ok(code)
}

fn valid_websocket_close_code(code: u16) -> bool {
    matches!(code, 1000..=1003 | 1007..=1014 | 3000..=4999) && !matches!(code, 1004..=1006 | 1015)
}

fn invalid_action(index: usize, detail: impl Display) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::InvalidOutput,
        format_args!("WebSocket action {index}: {detail}"),
    )
}

fn decode_status(value: Value) -> Result<u16, AdapterError> {
    let Value::Number(number) = value else {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidOutput,
            "HTTP response status is not Number",
        ));
    };
    let status = number
        .to_i64_exact()
        .map_err(|error| AdapterError::new(AdapterErrorKind::InvalidOutput, error))?;
    let status = u16::try_from(status).map_err(|_| {
        AdapterError::new(
            AdapterErrorKind::InvalidOutput,
            "HTTP response status is outside the u16 range",
        )
    })?;
    if !(100..=999).contains(&status) {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidOutput,
            "HTTP response status is outside the HTTP status range",
        ));
    }
    Ok(status)
}

fn decode_headers(value: Value, value_type: &HeaderValueType) -> Result<Vec<Header>, AdapterError> {
    let Value::List(values) = value else {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidOutput,
            "HTTP response headers are not a list",
        ));
    };
    values
        .into_iter()
        .map(|value| {
            let Value::Record(mut fields) = value else {
                return Err(AdapterError::new(
                    AdapterErrorKind::InvalidOutput,
                    "HTTP response header is not a record",
                ));
            };
            if fields.keys().map(String::as_str).collect::<BTreeSet<_>>()
                != BTreeSet::from(["name", "value"])
            {
                return Err(AdapterError::new(
                    AdapterErrorKind::InvalidOutput,
                    "HTTP response header fields differ from its closed schema",
                ));
            }
            let Value::Text(name) = fields
                .remove("name")
                .expect("closed header field set contains name")
            else {
                return Err(AdapterError::new(
                    AdapterErrorKind::InvalidOutput,
                    "HTTP response header name is not Text",
                ));
            };
            let value = fields
                .remove("value")
                .expect("closed header field set contains value");
            let value = match (value_type, value) {
                (HeaderValueType::Text, Value::Text(value)) => value.into_bytes(),
                (HeaderValueType::Bytes { fixed_len }, Value::Bytes(value)) => {
                    validate_fixed_bytes(value, *fixed_len, "HTTP response header value")?
                }
                _ => {
                    return Err(AdapterError::new(
                        AdapterErrorKind::InvalidOutput,
                        "HTTP response header value differs from its declared type",
                    ));
                }
            };
            Ok(Header::new(name, value))
        })
        .collect()
}

fn decode_body(value: Value, fixed_len: Option<u64>) -> Result<Vec<u8>, AdapterError> {
    match value {
        Value::Bytes(value) => validate_fixed_bytes(value, fixed_len, "HTTP response body"),
        _ => Err(AdapterError::new(
            AdapterErrorKind::InvalidOutput,
            "HTTP response body differs from its declared Bytes type",
        )),
    }
}

fn validate_fixed_bytes(
    value: Vec<u8>,
    fixed_len: Option<u64>,
    label: &str,
) -> Result<Vec<u8>, AdapterError> {
    if fixed_len.is_some_and(|expected| expected != value.len() as u64) {
        return Err(AdapterError::new(
            AdapterErrorKind::InvalidOutput,
            format_args!("{label} does not match its fixed byte length"),
        ));
    }
    Ok(value)
}

fn diagnostic_http_response(status: u16, error: &AdapterError) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![
            Header::new("content-type", b"text/plain; charset=utf-8".to_vec()),
            Header::new("cache-control", b"no-store".to_vec()),
        ],
        body: error.diagnostic().as_bytes().to_vec(),
    }
}

fn cancellation_reason(reason: CancellationReason) -> &'static str {
    match reason {
        CancellationReason::PeerDisconnected => "peer_disconnected",
        CancellationReason::DeadlineExceeded => "deadline_exceeded",
        CancellationReason::ServerShutdown => "server_shutdown",
    }
}

fn bounded_text(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let suffix = if max_bytes >= 3 { "..." } else { "" };
    let mut end = max_bytes.saturating_sub(suffix.len()).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str(suffix);
    text
}
