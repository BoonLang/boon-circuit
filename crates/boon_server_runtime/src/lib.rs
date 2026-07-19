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

mod distributed_sessions;
mod exact_call_host;
mod in_process;

pub use distributed_sessions::{
    DEFAULT_SESSION_RESUME_WINDOW, DistributedSessionConnectionId,
    DistributedSessionHandshakeOffer, DistributedSessionHandshakeRejection,
    DistributedSessionHandshakeRejectionReason, DistributedSessionHandshakeStart,
    DistributedSessionRegistry, DistributedSessionRegistryConfig, DistributedSessionRegistryError,
    DistributedSessionRegistryIdentity, DistributedSessionRegistryPoll, PoisonedDistributedSession,
    PreparedDistributedSessionDeliveries,
};
pub use exact_call_host::ExactCallHostCore;
pub use in_process::{
    DEFAULT_IN_PROCESS_POLL_STEPS, InProcessDistributedRuntime, InProcessDistributedRuntimeConfig,
    InProcessDistributedRuntimeError, InProcessFrameProgress, InProcessFrameTransferProgress,
    InProcessPoll, InProcessResumeState, InProcessTransientEffectCancellation,
    InProcessTransientEffectCreditGrant, InProcessTransientEffectInvocation,
    InProcessTransientEffectOwner,
};

use async_trait::async_trait;
use boon_persistence::{
    CommitAck, PersistenceDriver, PersistenceWorkerConfig, PersistenceWorkerStatus,
    TurnEnqueueError, TurnReservationError,
};
use boon_plan::{
    DataTypeFieldPlan, DataTypePlan, DataVariantPlan, HostPortPlan, MachinePlan,
    OutputContractKind, OutputRootId, OutputRootPlan, ProgramRole, SourceId, SourcePayloadField,
    SourceRoute,
};
use boon_runtime::{
    DistributedImportUpdate, DistributedProgramBundle, DistributedRuntimeError,
    DistributedServerMachine, DistributedServerRuntime, DistributedServerUpdate,
    PersistentDispatchError, PersistentProgramSession, PersistentRuntimeStartupDisposition,
    PreparedDistributedServerTransaction, PreparedDistributedServerUpdate, ProgramArtifact,
    ProgramCapabilityProfile, ProgramSession, ProgramSessionDispatch, RuntimeTurn, SessionContext,
    SessionPrincipal, SourceEvent, SourcePayload, TransientEffectCallId, TransientEffectInvocation,
    Value,
};
use boon_server_host::{
    CallCancellation, CancellationReason, DistributedSessionAction,
    DistributedSessionConnectionId as HostDistributedSessionConnectionId, DistributedSessionEvent,
    Header, HttpRequest, HttpResponse, PeerAddress, ServerProgram, WebSocketAction, WebSocketClose,
    WebSocketEvent, WebSocketFrame, WebSocketOpen, WebSocketTransportError,
};
use boon_wire::{SessionControlFrame, decode_session_control_frame};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const MAX_ADAPTER_DIAGNOSTIC_BYTES: usize = 512;
pub const MAX_WEBSOCKET_ACTIONS: usize = 256;
pub const MAX_WEBSOCKET_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_WEBSOCKET_REJECT_BODY_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_WEBSOCKET_ROOM_BYTES: usize = 512;
pub const MAX_WEBSOCKET_CLOSE_REASON_BYTES: usize = 123;
const MAX_DISTRIBUTED_SESSION_POLL_STEPS: usize = 256;
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
    pub max_active_calls: usize,
    pub max_calls_per_transaction: usize,
    pub max_events_per_transaction: usize,
}

impl Default for TransientEffectLimits {
    fn default() -> Self {
        Self {
            max_active_calls: 64,
            max_calls_per_transaction: 256,
            max_events_per_transaction: 65_536,
        }
    }
}

impl TransientEffectLimits {
    fn validate(self) -> Result<Self, AdapterError> {
        if self.max_active_calls == 0
            || self.max_calls_per_transaction == 0
            || self.max_events_per_transaction == 0
            || self.max_active_calls > self.max_calls_per_transaction
        {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidArtifact,
                "transient effect limits must be positive and the active-call limit must not exceed the transaction call limit",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransientEffectHostDelivery {
    Single,
    Stream { result_sequence: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransientEffectHostEvent {
    Result {
        call_id: TransientEffectCallId,
        delivery: TransientEffectHostDelivery,
        outcome: Value,
    },
    Cancelled {
        call_id: TransientEffectCallId,
    },
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

    /// Accepts exact runtime-owned calls without inferring replacement from
    /// invocation IDs or application values.
    fn submit(
        &mut self,
        calls: Vec<TransientEffectInvocation>,
    ) -> Result<(), TransientEffectHostError>;

    /// Returns one bounded single-result or stream-result event.
    async fn next_event(&mut self) -> Result<TransientEffectHostEvent, TransientEffectHostError>;

    fn grant_credits(
        &mut self,
        grants: &[boon_runtime::TransientEffectCreditGrant],
    ) -> Result<(), TransientEffectHostError> {
        if grants.is_empty() {
            Ok(())
        } else {
            Err(TransientEffectHostError::new(
                "transient effect host does not accept stream credits",
            ))
        }
    }

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
    pub distributed: ServerTurnDurability,
}

impl ServerDurabilityPolicy {
    pub const AUTHORITATIVE: Self = Self {
        http: ServerTurnDurability::Immediate,
        websocket: ServerTurnDurability::Immediate,
        disconnect: ServerTurnDurability::Immediate,
        distributed: ServerTurnDurability::Immediate,
    };

    pub const BUFFERED: Self = Self {
        http: ServerTurnDurability::Buffered,
        websocket: ServerTurnDurability::Buffered,
        disconnect: ServerTurnDurability::Buffered,
        distributed: ServerTurnDurability::Buffered,
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
    Persistent {
        session: Box<PersistentProgramSession>,
        distributed_durability: ServerTurnDurability,
        distributed_acknowledgements: Vec<CommitAck>,
    },
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
            Self::Persistent { session, .. } => session.artifact(),
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
            Self::Persistent { session, .. } => match durability {
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
            Self::Persistent { session, .. } => session
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
            Self::Persistent { session, .. } => match durability {
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

    fn deliver_transient_effect_result(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
        durability: ServerTurnDurability,
    ) -> Result<ServerSessionEffectTurn, AdapterError> {
        match self {
            Self::Ephemeral(session) => session
                .deliver_transient_effect_result(call_id, result_sequence, outcome)
                .map(|runtime_turn| ServerSessionEffectTurn {
                    runtime_turn,
                    acknowledgement: None,
                })
                .map_err(|error| AdapterError::new(AdapterErrorKind::Runtime, error)),
            Self::Persistent { session, .. } => match durability {
                ServerTurnDurability::Immediate => session
                    .deliver_transient_effect_result_durably(call_id, result_sequence, outcome)
                    .map(|acknowledged| ServerSessionEffectTurn {
                        runtime_turn: acknowledged.turn,
                        acknowledgement: Some(acknowledged.acknowledgement),
                    })
                    .map_err(persistent_dispatch_error),
                ServerTurnDurability::Buffered => session
                    .deliver_transient_effect_result(call_id, result_sequence, outcome)
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
            Self::Persistent { session, .. } => session
                .cancel_transient_effect(call_id)
                .map_err(persistent_dispatch_error),
        }
    }

    #[cfg(test)]
    fn pending_transient_effect_count(&self) -> usize {
        match self {
            Self::Ephemeral(session) => session.pending_transient_effect_count(),
            Self::Persistent { session, .. } => session.pending_transient_effect_count(),
        }
    }

    fn persistence_status(&self) -> Option<PersistenceWorkerStatus> {
        match self {
            Self::Ephemeral(_) => None,
            Self::Persistent { session, .. } => Some(session.persistence_status()),
        }
    }

    fn barrier(&self) -> Result<(), AdapterError> {
        match self {
            Self::Ephemeral(_) => Ok(()),
            Self::Persistent { session, .. } => session
                .barrier()
                .map(|_| ())
                .map_err(|error| AdapterError::new(AdapterErrorKind::Persistence, error)),
        }
    }

    fn shutdown(&self) -> Result<(), AdapterError> {
        match self {
            Self::Ephemeral(_) => Ok(()),
            Self::Persistent { session, .. } => session
                .shutdown()
                .map(|_| ())
                .map_err(|error| AdapterError::new(AdapterErrorKind::Persistence, error)),
        }
    }

    fn take_distributed_acknowledgements(&mut self) -> Vec<CommitAck> {
        match self {
            Self::Ephemeral(_) => Vec::new(),
            Self::Persistent {
                distributed_acknowledgements,
                ..
            } => std::mem::take(distributed_acknowledgements),
        }
    }
}

impl DistributedServerMachine for ServerRuntimeSession {
    type EvaluationMachine = ProgramSession;

    fn artifact(&self) -> &ProgramArtifact {
        ServerRuntimeSession::artifact(self)
    }

    fn fork_prepared_evaluation(
        &self,
        turn: Option<&RuntimeTurn>,
    ) -> Result<Self::EvaluationMachine, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => session.fork_prepared_evaluation(turn),
            Self::Persistent { session, .. } => {
                session.fork_prepared_distributed_server_evaluation(turn)
            }
        }
    }

    fn install_evaluation(
        &mut self,
        evaluation: Self::EvaluationMachine,
    ) -> Result<(), DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => session.install_evaluation(evaluation),
            Self::Persistent { session, .. } => session
                .install_distributed_server_evaluation(evaluation)
                .map_err(|error| DistributedRuntimeError::Runtime(error.to_string())),
        }
    }

    fn commit_prepared_evaluation(
        &mut self,
        turn: RuntimeTurn,
        evaluation: Self::EvaluationMachine,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => session.commit_prepared_evaluation(turn, evaluation),
            Self::Persistent {
                session,
                distributed_acknowledgements,
                ..
            } => {
                let (turn, acknowledgement) = session
                    .commit_prepared_distributed_server_evaluation(turn, evaluation, Vec::new())
                    .map_err(|error| DistributedRuntimeError::Runtime(error.to_string()))?;
                if let Some(acknowledgement) = acknowledgement {
                    distributed_acknowledgements.push(acknowledgement);
                }
                Ok(turn)
            }
        }
    }

    fn commit_prepared_evaluation_with_protocol_state(
        &mut self,
        turn: RuntimeTurn,
        evaluation: Self::EvaluationMachine,
        protocol_state_changes: Vec<boon_persistence::DurableProtocolStateChange>,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => {
                if !protocol_state_changes.is_empty() {
                    return Err(DistributedRuntimeError::Runtime(
                        "ephemeral Server authority cannot persist protocol recovery state"
                            .to_owned(),
                    ));
                }
                session.commit_prepared_evaluation(turn, evaluation)
            }
            Self::Persistent {
                session,
                distributed_acknowledgements,
                ..
            } => {
                let (turn, acknowledgement) = session
                    .commit_prepared_distributed_server_evaluation(
                        turn,
                        evaluation,
                        protocol_state_changes,
                    )
                    .map_err(|error| DistributedRuntimeError::Runtime(error.to_string()))?;
                if let Some(acknowledgement) = acknowledgement {
                    distributed_acknowledgements.push(acknowledgement);
                }
                Ok(turn)
            }
        }
    }

    fn event_for_path(
        &self,
        path: &str,
        payload: SourcePayload,
    ) -> Result<SourceEvent, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => session.event_for_path(path, payload),
            Self::Persistent { session, .. } => session.event_for_path(path, payload),
        }
    }

    fn event_for_source(
        &self,
        source: SourceId,
        payload: SourcePayload,
    ) -> Result<SourceEvent, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => session.event_for_source(source, payload),
            Self::Persistent { session, .. } => session.event_for_source(source, payload),
        }
    }

    fn prepare_dispatch(
        &mut self,
        event: SourceEvent,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => {
                DistributedServerMachine::prepare_dispatch(&mut **session, event)
            }
            Self::Persistent {
                session,
                distributed_durability,
                ..
            } => session
                .prepare_distributed_dispatch(
                    event,
                    *distributed_durability == ServerTurnDurability::Immediate,
                )
                .map_err(|error| DistributedRuntimeError::Runtime(error.to_string())),
        }
    }

    fn prepare_dispatch_with_durability(
        &mut self,
        event: SourceEvent,
        durable: bool,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => {
                DistributedServerMachine::prepare_dispatch(&mut **session, event)
            }
            Self::Persistent { session, .. } => session
                .prepare_distributed_dispatch(event, durable)
                .map_err(|error| DistributedRuntimeError::Runtime(error.to_string())),
        }
    }

    fn export_current(
        &mut self,
        export_id: boon_plan::ExportId,
    ) -> Result<Value, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => session.export_current(export_id),
            Self::Persistent { session, .. } => session.export_current(export_id),
        }
    }

    fn call_arguments(
        &mut self,
        call: &boon_plan::RemoteCallSitePlan,
    ) -> Result<BTreeMap<boon_plan::DistributedArgumentId, Value>, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => session.call_arguments(call),
            Self::Persistent { session, .. } => session.call_arguments(call),
        }
    }

    fn evaluate_function(
        &mut self,
        export_id: boon_plan::ExportId,
        arguments: BTreeMap<boon_plan::DistributedArgumentId, Value>,
    ) -> Result<Value, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => session.evaluate_function(export_id, arguments),
            Self::Persistent { session, .. } => session.evaluate_function(export_id, arguments),
        }
    }

    fn replace_distributed_context(
        &mut self,
        session_context: SessionContext,
        imports: Vec<DistributedImportUpdate>,
    ) -> Result<Option<RuntimeTurn>, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => {
                session.replace_distributed_context(session_context, imports)
            }
            Self::Persistent { session, .. } => {
                session.replace_distributed_context(session_context, imports)
            }
        }
    }

    fn prepare_transient_effect_completion(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => {
                DistributedServerMachine::prepare_transient_effect_completion(
                    &mut **session,
                    call_id,
                    outcome,
                )
            }
            Self::Persistent {
                session,
                distributed_durability,
                ..
            } => session
                .prepare_distributed_effect_completion(
                    call_id,
                    outcome,
                    *distributed_durability == ServerTurnDurability::Immediate,
                )
                .map_err(|error| DistributedRuntimeError::Runtime(error.to_string())),
        }
    }

    fn prepare_transient_effect_completion_with_durability(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
        durable: bool,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => {
                DistributedServerMachine::prepare_transient_effect_completion(
                    &mut **session,
                    call_id,
                    outcome,
                )
            }
            Self::Persistent { session, .. } => session
                .prepare_distributed_effect_completion(call_id, outcome, durable)
                .map_err(|error| DistributedRuntimeError::Runtime(error.to_string())),
        }
    }

    fn prepare_transient_effect_result(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => DistributedServerMachine::prepare_transient_effect_result(
                &mut **session,
                call_id,
                result_sequence,
                outcome,
            ),
            Self::Persistent {
                session,
                distributed_durability,
                ..
            } => session
                .prepare_distributed_effect_result(
                    call_id,
                    result_sequence,
                    outcome,
                    *distributed_durability == ServerTurnDurability::Immediate,
                )
                .map_err(|error| DistributedRuntimeError::Runtime(error.to_string())),
        }
    }

    fn prepare_transient_effect_result_with_durability(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
        durable: bool,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => DistributedServerMachine::prepare_transient_effect_result(
                &mut **session,
                call_id,
                result_sequence,
                outcome,
            ),
            Self::Persistent { session, .. } => session
                .prepare_distributed_effect_result(call_id, result_sequence, outcome, durable)
                .map_err(|error| DistributedRuntimeError::Runtime(error.to_string())),
        }
    }

    fn prepare_transient_effect_cancellation(
        &mut self,
        call_ids: &[TransientEffectCallId],
    ) -> Result<Option<RuntimeTurn>, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => session.prepare_transient_effect_cancellation(call_ids),
            Self::Persistent {
                session,
                distributed_durability,
                ..
            } => session
                .prepare_distributed_effect_cancellation(
                    call_ids,
                    *distributed_durability == ServerTurnDurability::Immediate,
                )
                .map_err(|error| DistributedRuntimeError::Runtime(error.to_string())),
        }
    }

    fn prepare_transient_effect_cancellation_with_durability(
        &mut self,
        call_ids: &[TransientEffectCallId],
        durable: bool,
    ) -> Result<Option<RuntimeTurn>, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => session.prepare_transient_effect_cancellation(call_ids),
            Self::Persistent { session, .. } => session
                .prepare_distributed_effect_cancellation(call_ids, durable)
                .map_err(|error| DistributedRuntimeError::Runtime(error.to_string())),
        }
    }

    fn commit_prepared_turn(
        &mut self,
        turn: RuntimeTurn,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => session.commit_prepared_turn(turn),
            Self::Persistent {
                session,
                distributed_acknowledgements,
                ..
            } => {
                let (turn, acknowledgement) = session
                    .commit_prepared_distributed_turn(turn)
                    .map_err(|error| DistributedRuntimeError::Runtime(error.to_string()))?;
                if let Some(acknowledgement) = acknowledgement {
                    distributed_acknowledgements.push(acknowledgement);
                }
                Ok(turn)
            }
        }
    }

    fn commit_prepared_turn_with_protocol_state(
        &mut self,
        turn: RuntimeTurn,
        protocol_state_changes: Vec<boon_persistence::DurableProtocolStateChange>,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => {
                if !protocol_state_changes.is_empty() {
                    return Err(DistributedRuntimeError::Runtime(
                        "ephemeral Server authority cannot persist protocol recovery state"
                            .to_owned(),
                    ));
                }
                session.commit_prepared_turn(turn)
            }
            Self::Persistent {
                session,
                distributed_acknowledgements,
                ..
            } => {
                let (turn, acknowledgement) = session
                    .commit_prepared_distributed_turn_with_protocol_state(
                        turn,
                        protocol_state_changes,
                    )
                    .map_err(|error| DistributedRuntimeError::Runtime(error.to_string()))?;
                if let Some(acknowledgement) = acknowledgement {
                    distributed_acknowledgements.push(acknowledgement);
                }
                Ok(turn)
            }
        }
    }

    fn prepare_protocol_checkpoint(&mut self) -> Result<RuntimeTurn, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(_) => Err(DistributedRuntimeError::Runtime(
                "ephemeral Server authority cannot persist protocol recovery state".to_owned(),
            )),
            Self::Persistent { session, .. } => session
                .prepare_protocol_checkpoint()
                .map_err(|error| DistributedRuntimeError::Runtime(error.to_string())),
        }
    }

    fn supports_protocol_state(&self) -> bool {
        matches!(self, Self::Persistent { .. })
    }

    fn rollback_prepared_turn(&mut self) -> Result<(), DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => session.rollback_prepared_turn(),
            Self::Persistent { session, .. } => session
                .rollback_prepared_distributed_turn()
                .map_err(|error| DistributedRuntimeError::Runtime(error.to_string())),
        }
    }

    fn has_pending_transient_effect(&self, call_id: TransientEffectCallId) -> bool {
        match self {
            Self::Ephemeral(session) => session.has_pending_transient_effect(call_id),
            Self::Persistent { session, .. } => session.has_pending_transient_effect(call_id),
        }
    }

    fn set_transient_effect_scope(&mut self, scope: u64) {
        match self {
            Self::Ephemeral(session) => session.set_transient_effect_scope(scope),
            Self::Persistent { session, .. } => session.set_transient_effect_scope(scope),
        }
    }

    fn root_value_current(&mut self, name: &str) -> Result<Value, DistributedRuntimeError> {
        match self {
            Self::Ephemeral(session) => {
                DistributedServerMachine::root_value_current(&mut **session, name)
            }
            Self::Persistent { session, .. } => {
                DistributedServerMachine::root_value_current(&mut **session, name)
            }
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

fn distributed_runtime_adapter_error(error: DistributedRuntimeError) -> AdapterError {
    let kind = match error {
        DistributedRuntimeError::QueueFull { .. }
        | DistributedRuntimeError::QueueBytesFull { .. }
        | DistributedRuntimeError::SessionCapacity { .. } => AdapterErrorKind::Backpressure,
        _ => AdapterErrorKind::Runtime,
    };
    AdapterError::new(kind, error)
}

fn distributed_registry_adapter_error(error: DistributedSessionRegistryError) -> AdapterError {
    let kind = match &error {
        DistributedSessionRegistryError::Runtime(
            DistributedRuntimeError::QueueFull { .. }
            | DistributedRuntimeError::QueueBytesFull { .. }
            | DistributedRuntimeError::SessionCapacity { .. },
        ) => AdapterErrorKind::Backpressure,
        _ => AdapterErrorKind::Runtime,
    };
    AdapterError::new(kind, error)
}

struct PersistentServerState {
    durability: ServerDurabilityPolicy,
    lifecycle: ServerLifecycleHandle,
    admission_open: bool,
    shutdown_complete: bool,
}

struct ServerAuthority {
    machine: ServerRuntimeSession,
    routing: Option<DistributedServerRuntime>,
    persistence: Option<PersistentServerState>,
}

#[derive(Clone)]
struct ActiveTransientEffect {
    delivery: boon_plan::EffectDeliveryCardinality,
    owner: TransientRuntimeOwner,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ServerTurnClass {
    Http,
    WebSocket,
    Distributed,
    Disconnect,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum TransientRuntimeOwner {
    DirectServer(ServerTurnClass),
    DistributedServer(boon_runtime::SessionOrigin),
    DistributedSession(boon_runtime::SessionOrigin),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TransientEffectBudget {
    submitted_calls: usize,
    delivered_events: usize,
}

/// One serialized trusted-server session owned by [`boon_server_host`].
///
/// Host-port names are never conventions at this boundary. The constructor
/// resolves the IDs embedded in `HostPortPlan` to runtime handles once, before
/// the native listener can be started.
#[derive(Clone, Copy)]
enum DistributedTransportPhase {
    AwaitingHello,
    AwaitingCommit(DistributedSessionConnectionId),
    Current(DistributedSessionConnectionId),
}

impl DistributedTransportPhase {
    fn registry_connection(self) -> Option<DistributedSessionConnectionId> {
        match self {
            Self::AwaitingHello => None,
            Self::AwaitingCommit(connection) | Self::Current(connection) => Some(connection),
        }
    }
}

#[derive(Clone, Copy)]
enum DistributedTransportSend {
    Control,
    Data,
}

struct DistributedTransportConnection {
    phase: DistributedTransportPhase,
    pending_send: Option<DistributedTransportSend>,
}

pub struct BoonServerProgram {
    authority: ServerAuthority,
    restored_protocol_state: boon_persistence::ProtocolStateSnapshot,
    restored_authority_turn_sequence: u64,
    distributed_sessions: Option<DistributedSessionRegistry>,
    distributed_clock_origin: Option<Instant>,
    distributed_wall_clock_origin: Option<Duration>,
    distributed_transport_connections:
        BTreeMap<HostDistributedSessionConnectionId, DistributedTransportConnection>,
    http: Option<HttpPortBinding>,
    websocket: Option<WebSocketPortBinding>,
    last_diagnostic: Option<AdapterError>,
    transient_effect_host: Option<Box<dyn TransientEffectHost>>,
    transient_effect_limits: TransientEffectLimits,
    required_transient_effects: BTreeMap<boon_plan::EffectId, String>,
    active_transient_effects: BTreeMap<TransientEffectCallId, ActiveTransientEffect>,
    transient_effect_budgets: BTreeMap<TransientRuntimeOwner, TransientEffectBudget>,
    pending_distributed_actions: VecDeque<DistributedSessionAction>,
}

impl BoonServerProgram {
    pub fn new(artifact: ProgramArtifact) -> Result<Self, AdapterError> {
        validate_server_artifact(&artifact)?;
        let bindings = resolve_bindings(artifact.plan())?;
        let required_transient_effects = collect_required_transient_effects([&artifact])?;
        let session = ProgramSession::start(artifact)
            .map_err(|error| AdapterError::new(AdapterErrorKind::InvalidArtifact, error))?;
        Ok(Self {
            authority: ServerAuthority {
                machine: ServerRuntimeSession::Ephemeral(Box::new(session)),
                routing: None,
                persistence: None,
            },
            restored_protocol_state: boon_persistence::ProtocolStateSnapshot::default(),
            restored_authority_turn_sequence: 0,
            distributed_sessions: None,
            distributed_clock_origin: None,
            distributed_wall_clock_origin: None,
            distributed_transport_connections: BTreeMap::new(),
            http: bindings.http,
            websocket: bindings.websocket,
            last_diagnostic: None,
            transient_effect_host: None,
            transient_effect_limits: TransientEffectLimits::default(),
            required_transient_effects,
            active_transient_effects: BTreeMap::new(),
            transient_effect_budgets: BTreeMap::new(),
            pending_distributed_actions: VecDeque::new(),
        })
    }

    pub fn from_artifact(artifact: ProgramArtifact) -> Result<Self, AdapterError> {
        Self::new(artifact)
    }

    pub fn new_distributed(
        bundle: &DistributedProgramBundle,
        config: DistributedSessionRegistryConfig,
    ) -> Result<Self, AdapterError> {
        let artifact = bundle
            .artifact(ProgramRole::Server)
            .ok_or_else(|| {
                AdapterError::new(
                    AdapterErrorKind::InvalidArtifact,
                    "distributed bundle has no Server artifact",
                )
            })?
            .clone();
        let mut program = Self::new(artifact)?;
        program.attach_distributed_sessions(bundle, config)?;
        Ok(program)
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
        let required_transient_effects = collect_required_transient_effects([&artifact])?;
        let (session, startup) =
            PersistentProgramSession::start(artifact, driver, config.worker.clone())
                .map_err(|error| AdapterError::new(AdapterErrorKind::Persistence, error))?;
        let session = ServerRuntimeSession::Persistent {
            session: Box::new(session),
            distributed_durability: config.durability.distributed,
            distributed_acknowledgements: Vec::new(),
        };
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
        let restored_protocol_state = startup.protocol_state.clone();
        let restored_authority_turn_sequence = startup.restore_image.through_turn_sequence;
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
                authority: ServerAuthority {
                    machine: session,
                    routing: None,
                    persistence: Some(PersistentServerState {
                        durability: config.durability,
                        lifecycle,
                        admission_open: true,
                        shutdown_complete: false,
                    }),
                },
                restored_protocol_state,
                restored_authority_turn_sequence,
                distributed_sessions: None,
                distributed_clock_origin: None,
                distributed_wall_clock_origin: None,
                distributed_transport_connections: BTreeMap::new(),
                http: bindings.http,
                websocket: bindings.websocket,
                last_diagnostic: None,
                transient_effect_host: None,
                transient_effect_limits: TransientEffectLimits::default(),
                required_transient_effects,
                active_transient_effects: BTreeMap::new(),
                transient_effect_budgets: BTreeMap::new(),
                pending_distributed_actions: VecDeque::new(),
            },
            server_startup,
        ))
    }

    pub fn with_distributed_persistence<D>(
        bundle: &DistributedProgramBundle,
        driver: D,
        persistence: PersistentServerConfig,
        sessions: DistributedSessionRegistryConfig,
    ) -> Result<(Self, PersistentServerStartup), AdapterError>
    where
        D: PersistenceDriver + Send + 'static,
    {
        let artifact = bundle
            .artifact(ProgramRole::Server)
            .ok_or_else(|| {
                AdapterError::new(
                    AdapterErrorKind::InvalidArtifact,
                    "distributed bundle has no Server artifact",
                )
            })?
            .clone();
        let (mut program, startup) = Self::with_persistence(artifact, driver, persistence)?;
        program.attach_distributed_sessions(bundle, sessions)?;
        Ok((program, startup))
    }

    pub fn attach_distributed_sessions(
        &mut self,
        bundle: &DistributedProgramBundle,
        config: DistributedSessionRegistryConfig,
    ) -> Result<(), AdapterError> {
        if self.authority.routing.is_some() || self.distributed_sessions.is_some() {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidArtifact,
                "trusted Server already has a distributed Session registry",
            ));
        }
        let required_transient_effects =
            collect_required_transient_effects(bundle.artifacts().iter())?;
        if let Some(host) = self.transient_effect_host.as_ref() {
            validate_transient_effect_host(host.as_ref(), &required_transient_effects)?;
        }
        let artifact = bundle.artifact(ProgramRole::Server).ok_or_else(|| {
            AdapterError::new(
                AdapterErrorKind::InvalidArtifact,
                "distributed bundle has no Server artifact",
            )
        })?;
        if artifact.id() != self.artifact().id()
            || artifact.revision() != self.artifact().revision()
            || artifact.plan_digest() != self.artifact().plan_digest()
        {
            return Err(AdapterError::new(
                AdapterErrorKind::InvalidArtifact,
                "distributed bundle Server artifact is not the program authority artifact",
            ));
        }
        let clock_origin = Instant::now();
        let wall_clock_origin = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| {
            AdapterError::new(
                AdapterErrorKind::Runtime,
                "system clock is before the Unix epoch",
            )
        })?;
        let (mut sessions, router_recovery) = DistributedSessionRegistry::start_with_recovery(
            bundle,
            config,
            &self.restored_protocol_state,
            self.restored_authority_turn_sequence,
            wall_clock_origin,
        )
        .map_err(|error| AdapterError::new(AdapterErrorKind::InvalidArtifact, error))?;
        let mut server = match router_recovery.as_deref() {
            Some(payload) => DistributedServerRuntime::start_with_recovery(artifact, payload),
            None => DistributedServerRuntime::start(artifact),
        }
        .map_err(|error| AdapterError::new(AdapterErrorKind::InvalidArtifact, error))?;
        sessions
            .validate_router_recovery(&server)
            .map_err(|error| AdapterError::new(AdapterErrorKind::InvalidArtifact, error))?;
        if router_recovery.is_some() {
            let prepared = sessions
                .prepare_recovery_checkpoint(
                    server
                        .recovery_payload()
                        .map_err(|error| AdapterError::new(AdapterErrorKind::Runtime, error))?,
                )
                .map_err(|error| AdapterError::new(AdapterErrorKind::Runtime, error))?;
            {
                let mut authority = server.bind(&mut self.authority.machine);
                authority
                    .commit_protocol_checkpoint(|turn_sequence| {
                        prepared
                            .changes(turn_sequence)
                            .map_err(|error| DistributedRuntimeError::Runtime(error.to_string()))
                    })
                    .map_err(|error| AdapterError::new(AdapterErrorKind::Persistence, error))?;
            }
            sessions.commit_recovery_checkpoint(prepared);
            let acknowledgements = self.authority.machine.take_distributed_acknowledgements();
            if acknowledgements.len() != 1 {
                return Err(AdapterError::new(
                    AdapterErrorKind::Persistence,
                    "restored distributed recovery checkpoint did not produce one durable acknowledgement",
                ));
            }
            self.record_persistent_accept(acknowledgements.first())?;
        }
        self.authority.routing = Some(server);
        self.distributed_sessions = Some(sessions);
        self.distributed_clock_origin = Some(clock_origin);
        self.distributed_wall_clock_origin = Some(wall_clock_origin);
        self.restored_protocol_state = boon_persistence::ProtocolStateSnapshot::default();
        self.restored_authority_turn_sequence = 0;
        self.distributed_transport_connections.clear();
        self.required_transient_effects = required_transient_effects;
        Ok(())
    }

    pub fn distributed_identity(&self) -> Option<DistributedSessionRegistryIdentity> {
        self.distributed_sessions
            .as_ref()
            .map(DistributedSessionRegistry::identity)
    }

    pub fn distributed_session_count(&self) -> Option<usize> {
        self.distributed_sessions
            .as_ref()
            .map(DistributedSessionRegistry::session_count)
    }

    pub fn begin_distributed_handshake(
        &mut self,
        now: Duration,
        principal: SessionPrincipal,
        client_frame: &[u8],
    ) -> Result<DistributedSessionHandshakeStart, DistributedSessionRegistryError> {
        let server = self
            .authority
            .routing
            .as_mut()
            .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?;
        let sessions = self
            .distributed_sessions
            .as_mut()
            .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?;
        let mut authority = server.bind(&mut self.authority.machine);
        sessions.begin_handshake(&mut authority, now, principal, client_frame)
    }

    pub fn commit_distributed_handshake(
        &mut self,
        now: Duration,
        connection_id: DistributedSessionConnectionId,
        client_frame: &[u8],
    ) -> Result<Vec<u8>, DistributedSessionRegistryError> {
        if self.authority.machine.supports_protocol_state() {
            let mut sessions = self
                .distributed_sessions
                .as_ref()
                .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?
                .fork_settled()?;
            let mut server = self
                .authority
                .routing
                .as_ref()
                .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?
                .clone();
            let turn = self.authority.machine.prepare_protocol_checkpoint()?;
            let mut evaluation = match self.authority.machine.fork_prepared_evaluation(Some(&turn))
            {
                Ok(evaluation) => evaluation,
                Err(error) => {
                    let _ = self.authority.machine.rollback_prepared_turn();
                    return Err(error.into());
                }
            };
            let ready = match {
                let mut authority = server.bind(&mut evaluation);
                sessions.commit_handshake(&mut authority, now, connection_id, client_frame)
            } {
                Ok(ready) => ready,
                Err(error) => {
                    let _ = self.authority.machine.rollback_prepared_turn();
                    return Err(error);
                }
            };
            self.commit_distributed_recovery_candidate(sessions, server, evaluation, turn)?;
            return Ok(ready);
        }
        let server = self
            .authority
            .routing
            .as_mut()
            .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?;
        let sessions = self
            .distributed_sessions
            .as_mut()
            .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?;
        let mut authority = server.bind(&mut self.authority.machine);
        sessions.commit_handshake(&mut authority, now, connection_id, client_frame)
    }

    fn commit_distributed_recovery_candidate(
        &mut self,
        mut sessions: DistributedSessionRegistry,
        server: DistributedServerRuntime,
        evaluation: ProgramSession,
        turn: RuntimeTurn,
    ) -> Result<(), DistributedSessionRegistryError> {
        let prepared = match server
            .recovery_payload()
            .map_err(DistributedSessionRegistryError::from)
            .and_then(|payload| sessions.prepare_recovery_checkpoint(payload))
        {
            Ok(prepared) => prepared,
            Err(error) => {
                let _ = self.authority.machine.rollback_prepared_turn();
                return Err(error);
            }
        };
        let changes = match prepared.changes(turn.sequence) {
            Ok(changes) => changes,
            Err(error) => {
                let _ = self.authority.machine.rollback_prepared_turn();
                return Err(error);
            }
        };
        if let Err(error) = self
            .authority
            .machine
            .commit_prepared_evaluation_with_protocol_state(turn, evaluation, changes)
        {
            let _ = self.authority.machine.rollback_prepared_turn();
            return Err(error.into());
        }
        sessions.commit_recovery_checkpoint(prepared);
        self.authority.routing = Some(server);
        self.distributed_sessions = Some(sessions);

        let acknowledgements = self.authority.machine.take_distributed_acknowledgements();
        if acknowledgements.len() != 1 {
            return Err(DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::Runtime(
                    "distributed recovery transaction did not produce one durable acknowledgement"
                        .to_owned(),
                ),
            ));
        }
        self.record_persistent_accept(acknowledgements.first())
            .map_err(|error| {
                DistributedSessionRegistryError::Runtime(DistributedRuntimeError::Runtime(
                    error.to_string(),
                ))
            })?;
        Ok(())
    }

    pub fn disconnect_distributed_session(
        &mut self,
        now: Duration,
        connection_id: DistributedSessionConnectionId,
    ) -> Result<(), DistributedSessionRegistryError> {
        let result = {
            let server = self
                .authority
                .routing
                .as_mut()
                .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?;
            let sessions = self
                .distributed_sessions
                .as_mut()
                .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?;
            let mut authority = server.bind(&mut self.authority.machine);
            sessions.disconnect(&mut authority, now, connection_id)
        };
        self.finish_distributed_lifecycle_operation(result)
    }

    pub fn revoke_distributed_session(
        &mut self,
        connection_id: DistributedSessionConnectionId,
        client_frame: &[u8],
    ) -> Result<Vec<u8>, DistributedSessionRegistryError> {
        let result = {
            let server = self
                .authority
                .routing
                .as_mut()
                .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?;
            let sessions = self
                .distributed_sessions
                .as_mut()
                .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?;
            let mut authority = server.bind(&mut self.authority.machine);
            sessions.revoke(&mut authority, connection_id, client_frame)
        };
        self.finish_distributed_lifecycle_operation(result)
    }

    fn finish_distributed_lifecycle_operation<T>(
        &mut self,
        result: Result<T, DistributedSessionRegistryError>,
    ) -> Result<T, DistributedSessionRegistryError> {
        let durable_admissions = self
            .distributed_sessions
            .as_mut()
            .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?
            .take_direct_lifecycle_durable_admissions()?;
        let acknowledgements = self.authority.machine.take_distributed_acknowledgements();
        let acknowledgement_result = if acknowledgements.len() > durable_admissions {
            Err(DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::Runtime(
                    "distributed lifecycle acknowledgements exceed durable admissions".to_owned(),
                ),
            ))
        } else {
            let mut acknowledgements = acknowledgements.iter();
            (0..durable_admissions).try_for_each(|_| {
                self.record_persistent_accept(acknowledgements.next())
                    .map_err(|error| {
                        DistributedSessionRegistryError::Runtime(DistributedRuntimeError::Runtime(
                            error.to_string(),
                        ))
                    })
            })
        };
        match (result, acknowledgement_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Err(operation), Err(acknowledgement)) => {
                Err(DistributedSessionRegistryError::Runtime(
                    DistributedRuntimeError::Runtime(format!(
                        "distributed lifecycle failed: {operation}; acknowledgement accounting failed: {acknowledgement}"
                    )),
                ))
            }
        }
    }

    pub fn admit_distributed_client_frame(
        &mut self,
        connection_id: DistributedSessionConnectionId,
        frame: &[u8],
    ) -> Result<(), DistributedSessionRegistryError> {
        self.distributed_sessions
            .as_mut()
            .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?
            .admit_client_frame(connection_id, frame)
    }

    pub fn next_distributed_client_frame(
        &mut self,
        connection_id: DistributedSessionConnectionId,
    ) -> Result<Option<Vec<u8>>, DistributedSessionRegistryError> {
        self.distributed_sessions
            .as_mut()
            .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?
            .next_client_frame(connection_id)
    }

    pub fn acknowledge_distributed_client_frame(
        &mut self,
        connection_id: DistributedSessionConnectionId,
    ) -> Result<bool, DistributedSessionRegistryError> {
        self.distributed_sessions
            .as_mut()
            .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?
            .acknowledge_client_frame(connection_id)
    }

    pub fn poll_distributed_sessions(
        &mut self,
        now: Duration,
        maximum_steps: usize,
    ) -> Result<DistributedSessionRegistryPoll, DistributedSessionRegistryError> {
        let server = self
            .authority
            .routing
            .as_mut()
            .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?;
        let sessions = self
            .distributed_sessions
            .as_mut()
            .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?;
        let poll = {
            let mut authority = server.bind(&mut self.authority.machine);
            sessions.poll(&mut authority, now, maximum_steps)?
        };
        let mutation_count = poll
            .server_turns
            .iter()
            .filter(|(_, turn)| turn.source_sequence.is_some())
            .count();
        let durable_admission_count = mutation_count
            .checked_add(poll.durable_protocol_checkpoints)
            .ok_or_else(|| {
                DistributedSessionRegistryError::Runtime(DistributedRuntimeError::Runtime(
                    "distributed durability acknowledgement count overflowed".to_owned(),
                ))
            })?;
        let acknowledgements = self.authority.machine.take_distributed_acknowledgements();
        if acknowledgements.len() > durable_admission_count {
            return Err(DistributedSessionRegistryError::Runtime(
                DistributedRuntimeError::Runtime(
                    "distributed persistence acknowledgements exceed admitted Server turns"
                        .to_owned(),
                ),
            ));
        }
        let mut acknowledgements = acknowledgements.into_iter();
        for _ in 0..durable_admission_count {
            self.record_persistent_accept(acknowledgements.next().as_ref())
                .map_err(|error| {
                    DistributedSessionRegistryError::Runtime(DistributedRuntimeError::Runtime(
                        error.to_string(),
                    ))
                })?;
        }
        Ok(poll)
    }

    pub fn distributed_next_deadline(&self) -> Option<Instant> {
        let sessions = self.distributed_sessions.as_ref()?;
        self.distributed_clock_origin?;
        let instant_now = Instant::now();
        let wall_now = self.distributed_now(instant_now).ok()?;
        let lifecycle_deadline = sessions.next_deadline().and_then(|deadline| {
            if deadline <= wall_now {
                Some(instant_now)
            } else {
                instant_now.checked_add(deadline - wall_now)
            }
        });
        let writer_admission_pending = self
            .distributed_transport_connections
            .values()
            .any(|connection| connection.pending_send.is_some());
        if writer_admission_pending {
            return lifecycle_deadline;
        }
        let sendable_output = self
            .distributed_transport_connections
            .values()
            .any(|connection| {
                let DistributedTransportPhase::Current(registry_connection) = connection.phase
                else {
                    return false;
                };
                sessions
                    .has_sendable_client_frame(registry_connection)
                    .is_ok_and(|sendable| sendable)
            });
        if sessions.has_runnable_work() || sendable_output {
            return Some(Instant::now());
        }
        lifecycle_deadline
    }

    fn distributed_now(&self, now: Instant) -> Result<Duration, DistributedSessionRegistryError> {
        let origin = self
            .distributed_clock_origin
            .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?;
        let elapsed = now
            .checked_duration_since(origin)
            .ok_or(DistributedSessionRegistryError::TimeRegression)?;
        self.distributed_wall_clock_origin
            .ok_or(DistributedSessionRegistryError::IdentityUnavailable)?
            .checked_add(elapsed)
            .ok_or(DistributedSessionRegistryError::TimeOverflow)
    }

    fn record_distributed_transport_failure(&mut self, error: impl Display) {
        self.last_diagnostic = Some(AdapterError::new(AdapterErrorKind::Runtime, error));
    }

    fn queue_distributed_control_send(
        &mut self,
        connection: HostDistributedSessionConnectionId,
        bytes: Vec<u8>,
    ) -> DistributedSessionAction {
        self.distributed_transport_connections
            .get_mut(&connection)
            .expect("distributed transport connection remains registered")
            .pending_send = Some(DistributedTransportSend::Control);
        DistributedSessionAction::send(connection, bytes)
    }

    fn fail_distributed_transport_connection(
        &mut self,
        connection: HostDistributedSessionConnectionId,
        now: Duration,
        error: impl Display,
    ) -> Vec<DistributedSessionAction> {
        let registry_connection = self
            .distributed_transport_connections
            .remove(&connection)
            .and_then(|state| state.phase.registry_connection());
        self.record_distributed_transport_failure(error);
        if let Some(registry_connection) = registry_connection
            && let Err(cleanup_error) =
                self.disconnect_distributed_session(now, registry_connection)
        {
            self.record_distributed_transport_failure(format_args!(
                "distributed transport cleanup failed: {cleanup_error}"
            ));
        }
        vec![DistributedSessionAction::close(
            connection,
            WebSocketClose::new(1002, "invalid distributed Session transport"),
        )]
    }

    fn fail_all_distributed_transport_connections(
        &mut self,
        now: Duration,
        error: impl Display,
    ) -> Vec<DistributedSessionAction> {
        self.record_distributed_transport_failure(error);
        let connections = self
            .distributed_transport_connections
            .keys()
            .copied()
            .collect::<Vec<_>>();
        let mut actions = Vec::with_capacity(connections.len());
        for connection in connections {
            let registry_connection = self
                .distributed_transport_connections
                .remove(&connection)
                .and_then(|state| state.phase.registry_connection());
            if let Some(registry_connection) = registry_connection
                && let Err(cleanup_error) =
                    self.disconnect_distributed_session(now, registry_connection)
            {
                self.record_distributed_transport_failure(format_args!(
                    "distributed transport cleanup failed: {cleanup_error}"
                ));
            }
            actions.push(DistributedSessionAction::close(
                connection,
                WebSocketClose::new(1011, "distributed Session runtime failed"),
            ));
        }
        actions
    }

    fn collect_distributed_client_frames(
        &mut self,
        now: Duration,
    ) -> Vec<DistributedSessionAction> {
        let candidates = self
            .distributed_transport_connections
            .iter()
            .filter_map(|(host_connection, state)| {
                let DistributedTransportPhase::Current(registry_connection) = state.phase else {
                    return None;
                };
                state
                    .pending_send
                    .is_none()
                    .then_some((*host_connection, registry_connection))
            })
            .collect::<Vec<_>>();
        let mut actions = Vec::new();
        for (host_connection, registry_connection) in candidates {
            match self.next_distributed_client_frame(registry_connection) {
                Ok(Some(bytes)) => {
                    self.distributed_transport_connections
                        .get_mut(&host_connection)
                        .expect("collected distributed transport connection remains registered")
                        .pending_send = Some(DistributedTransportSend::Data);
                    actions.push(DistributedSessionAction::send(host_connection, bytes));
                }
                Ok(None) => {}
                Err(error) => actions.extend(self.fail_distributed_transport_connection(
                    host_connection,
                    now,
                    error,
                )),
            }
        }
        actions
    }

    fn poll_distributed_transport(&mut self, now: Duration) -> Vec<DistributedSessionAction> {
        let poll = match self.poll_distributed_sessions(now, MAX_DISTRIBUTED_SESSION_POLL_STEPS) {
            Ok(poll) => poll,
            Err(error) => return self.fail_all_distributed_transport_connections(now, error),
        };
        let DistributedSessionRegistryPoll {
            poisoned_sessions,
            session_turns,
            server_turns,
            ..
        } = poll;
        let mut actions = Vec::new();
        for poisoned in poisoned_sessions {
            self.record_distributed_transport_failure(&poisoned.diagnostic);
            let Some(registry_connection) = poisoned.connection_id else {
                continue;
            };
            let host_connection = self.distributed_transport_connections.iter().find_map(
                |(host_connection, state)| {
                    (state.phase.registry_connection() == Some(registry_connection))
                        .then_some(*host_connection)
                },
            );
            if let Some(host_connection) = host_connection {
                self.distributed_transport_connections
                    .remove(&host_connection);
                actions.push(DistributedSessionAction::close(
                    host_connection,
                    WebSocketClose::new(1002, "invalid distributed Session frame"),
                ));
            }
        }
        for (origin, turn) in session_turns {
            if let Err(error) = self.route_transient_runtime_turn(
                &turn,
                TransientRuntimeOwner::DistributedSession(origin),
            ) {
                actions.extend(self.fail_all_distributed_transport_connections(now, error));
                return actions;
            }
        }
        for (origin, turn) in server_turns {
            if let Err(error) = self.route_transient_runtime_turn(
                &turn,
                TransientRuntimeOwner::DistributedServer(origin),
            ) {
                actions.extend(self.fail_all_distributed_transport_connections(now, error));
                return actions;
            }
        }
        actions.extend(self.collect_distributed_client_frames(now));
        actions
    }

    fn handle_distributed_transport_event(
        &mut self,
        connection: HostDistributedSessionConnectionId,
        event: DistributedSessionEvent,
    ) -> Vec<DistributedSessionAction> {
        match event {
            DistributedSessionEvent::Open(_) => {
                if self
                    .distributed_transport_connections
                    .contains_key(&connection)
                {
                    return vec![DistributedSessionAction::close(
                        connection,
                        WebSocketClose::new(1002, "duplicate distributed Session connection"),
                    )];
                }
                self.distributed_transport_connections.insert(
                    connection,
                    DistributedTransportConnection {
                        phase: DistributedTransportPhase::AwaitingHello,
                        pending_send: None,
                    },
                );
                Vec::new()
            }
            DistributedSessionEvent::Close(_) => {
                let Some(state) = self.distributed_transport_connections.remove(&connection) else {
                    return Vec::new();
                };
                let Ok(now) = self.distributed_now(Instant::now()) else {
                    return Vec::new();
                };
                if let Some(registry_connection) = state.phase.registry_connection()
                    && let Err(error) =
                        self.disconnect_distributed_session(now, registry_connection)
                {
                    self.record_distributed_transport_failure(error);
                }
                self.poll_distributed_transport(now)
            }
            DistributedSessionEvent::Binary(bytes) => {
                let now = match self.distributed_now(Instant::now()) {
                    Ok(now) => now,
                    Err(error) => {
                        self.record_distributed_transport_failure(error);
                        return vec![DistributedSessionAction::close(
                            connection,
                            WebSocketClose::new(1011, "distributed Session clock failed"),
                        )];
                    }
                };
                let Some(phase) = self
                    .distributed_transport_connections
                    .get(&connection)
                    .map(|state| state.phase)
                else {
                    return vec![DistributedSessionAction::close(
                        connection,
                        WebSocketClose::new(1002, "unknown distributed Session connection"),
                    )];
                };
                match phase {
                    DistributedTransportPhase::AwaitingHello => {
                        match self.begin_distributed_handshake(
                            now,
                            SessionPrincipal::Anonymous,
                            &bytes,
                        ) {
                            Ok(DistributedSessionHandshakeStart::Offer(offer)) => {
                                let (registry_connection, server_frame) = offer.into_parts();
                                self.distributed_transport_connections
                                    .get_mut(&connection)
                                    .expect("distributed transport connection remains registered")
                                    .phase =
                                    DistributedTransportPhase::AwaitingCommit(registry_connection);
                                vec![self.queue_distributed_control_send(connection, server_frame)]
                            }
                            Ok(DistributedSessionHandshakeStart::Reject(rejection)) => {
                                let server_frame = rejection.server_frame().to_vec();
                                self.distributed_transport_connections.remove(&connection);
                                vec![
                                    DistributedSessionAction::send(connection, server_frame),
                                    DistributedSessionAction::close(
                                        connection,
                                        WebSocketClose::new(1008, "distributed Session rejected"),
                                    ),
                                ]
                            }
                            Err(error) => {
                                self.fail_distributed_transport_connection(connection, now, error)
                            }
                        }
                    }
                    DistributedTransportPhase::AwaitingCommit(registry_connection) => {
                        match self.commit_distributed_handshake(now, registry_connection, &bytes) {
                            Ok(server_frame) => {
                                self.distributed_transport_connections
                                    .get_mut(&connection)
                                    .expect("distributed transport connection remains registered")
                                    .phase =
                                    DistributedTransportPhase::Current(registry_connection);
                                vec![self.queue_distributed_control_send(connection, server_frame)]
                            }
                            Err(error) => {
                                self.fail_distributed_transport_connection(connection, now, error)
                            }
                        }
                    }
                    DistributedTransportPhase::Current(registry_connection) => {
                        match decode_session_control_frame(&bytes) {
                            Ok(SessionControlFrame::ClientRevoke(_)) => {
                                match self.revoke_distributed_session(registry_connection, &bytes) {
                                    Ok(server_frame) => {
                                        self.distributed_transport_connections.remove(&connection);
                                        vec![
                                            DistributedSessionAction::send(
                                                connection,
                                                server_frame,
                                            ),
                                            DistributedSessionAction::close(
                                                connection,
                                                WebSocketClose::new(
                                                    1000,
                                                    "distributed Session revoked",
                                                ),
                                            ),
                                        ]
                                    }
                                    Err(error) => self.fail_distributed_transport_connection(
                                        connection, now, error,
                                    ),
                                }
                            }
                            Ok(_) => self.fail_distributed_transport_connection(
                                connection,
                                now,
                                DistributedSessionRegistryError::UnexpectedControlFrame,
                            ),
                            Err(_) => {
                                if let Err(error) =
                                    self.admit_distributed_client_frame(registry_connection, &bytes)
                                {
                                    return self.fail_distributed_transport_connection(
                                        connection, now, error,
                                    );
                                }
                                self.poll_distributed_transport(now)
                            }
                        }
                    }
                }
            }
        }
    }

    fn acknowledge_distributed_transport_send(
        &mut self,
        connection: HostDistributedSessionConnectionId,
    ) {
        let pending = self
            .distributed_transport_connections
            .get_mut(&connection)
            .and_then(|state| state.pending_send.take().map(|send| (send, state.phase)));
        let Some((DistributedTransportSend::Data, phase)) = pending else {
            return;
        };
        let DistributedTransportPhase::Current(registry_connection) = phase else {
            self.record_distributed_transport_failure(
                "distributed data send completed outside Current phase",
            );
            return;
        };
        match self.acknowledge_distributed_client_frame(registry_connection) {
            Ok(true) => {}
            Ok(false) => self.record_distributed_transport_failure(
                "distributed data writer acknowledged a missing frame lease",
            ),
            Err(error) => self.record_distributed_transport_failure(error),
        }
    }

    fn cancel_distributed_transport_connection(
        &mut self,
        connection: HostDistributedSessionConnectionId,
    ) {
        let Some(state) = self.distributed_transport_connections.remove(&connection) else {
            return;
        };
        let Some(registry_connection) = state.phase.registry_connection() else {
            return;
        };
        let Ok(now) = self.distributed_now(Instant::now()) else {
            return;
        };
        if let Err(error) = self.disconnect_distributed_session(now, registry_connection) {
            self.record_distributed_transport_failure(error);
        }
    }

    fn cancel_all_distributed_transport_connections(&mut self) {
        let connections = self
            .distributed_transport_connections
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for connection in connections {
            self.cancel_distributed_transport_connection(connection);
        }
    }

    pub fn artifact(&self) -> &ProgramArtifact {
        self.authority.machine.artifact()
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
        self.authority
            .persistence
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
        validate_transient_effect_host(host.as_ref(), &self.required_transient_effects)?;
        self.transient_effect_limits = limits;
        self.transient_effect_host = Some(host);
        Ok(())
    }

    fn ensure_admission_open(&mut self) -> Result<(), AdapterError> {
        if self
            .authority
            .persistence
            .as_ref()
            .is_some_and(|state| !state.admission_open)
        {
            return Err(AdapterError::new(
                AdapterErrorKind::Persistence,
                "persistent server admission is closed",
            ));
        }
        if let Some(persistence) = self.authority.machine.persistence_status()
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
        let Some(state) = &self.authority.persistence else {
            return ServerTurnDurability::Buffered;
        };
        match class {
            ServerTurnClass::Http => state.durability.http,
            ServerTurnClass::WebSocket => state.durability.websocket,
            ServerTurnClass::Distributed => state.durability.distributed,
            ServerTurnClass::Disconnect => state.durability.disconnect,
        }
    }

    fn commit_prepared_distributed_update(
        &mut self,
        prepared: PreparedDistributedServerUpdate,
    ) -> Result<DistributedServerUpdate, AdapterError> {
        let prepares_machine_turn = prepared.prepares_machine_turn();
        let prepared_deliveries = match self
            .distributed_sessions
            .as_ref()
            .ok_or_else(|| {
                AdapterError::new(
                    AdapterErrorKind::Runtime,
                    "distributed Server routing has no Session registry",
                )
            })?
            .prepare_deliveries(prepared.deliveries())
        {
            Ok(prepared_deliveries) => prepared_deliveries,
            Err(error) => {
                let rollback = self
                    .authority
                    .routing
                    .as_mut()
                    .expect("distributed preparation requires routing")
                    .bind(&mut self.authority.machine)
                    .rollback_prepared_update(prepared);
                return match rollback {
                    Ok(()) => Err(distributed_registry_adapter_error(error)),
                    Err(rollback) => Err(AdapterError::new(
                        AdapterErrorKind::Runtime,
                        format_args!(
                            "distributed delivery reservation failed: {error}; rollback failed: {rollback}"
                        ),
                    )),
                };
            }
        };
        let prepared_recovery = if self.authority.machine.supports_protocol_state() {
            let router_payload = match prepared.candidate_recovery_payload() {
                Ok(payload) => payload,
                Err(error) => {
                    self.authority
                        .routing
                        .as_mut()
                        .expect("distributed preparation requires routing")
                        .bind(&mut self.authority.machine)
                        .rollback_prepared_update(prepared)
                        .map_err(distributed_runtime_adapter_error)?;
                    return Err(distributed_runtime_adapter_error(error));
                }
            };
            match self
                .distributed_sessions
                .as_ref()
                .expect("distributed preparation requires a Session registry")
                .prepare_recovery_checkpoint_with_deliveries(router_payload, &prepared_deliveries)
            {
                Ok(checkpoint) => Some(checkpoint),
                Err(error) => {
                    self.authority
                        .routing
                        .as_mut()
                        .expect("distributed preparation requires routing")
                        .bind(&mut self.authority.machine)
                        .rollback_prepared_update(prepared)
                        .map_err(distributed_runtime_adapter_error)?;
                    return Err(distributed_registry_adapter_error(error));
                }
            }
        } else {
            None
        };
        let update = {
            let mut authority = self
                .authority
                .routing
                .as_mut()
                .expect("distributed preparation requires routing")
                .bind(&mut self.authority.machine);
            match prepared_recovery.as_ref() {
                Some(recovery) => authority.commit_prepared_update_with_protocol_state(
                    prepared,
                    |turn_sequence| {
                        recovery
                            .changes(turn_sequence)
                            .map_err(|error| DistributedRuntimeError::Runtime(error.to_string()))
                    },
                ),
                None => authority.commit_prepared_update(prepared),
            }
            .map_err(distributed_runtime_adapter_error)?
        };
        self.distributed_sessions
            .as_mut()
            .expect("distributed preparation requires a Session registry")
            .commit_deliveries(prepared_deliveries);
        if let Some(recovery) = prepared_recovery {
            self.distributed_sessions
                .as_mut()
                .expect("distributed preparation requires a Session registry")
                .commit_recovery_checkpoint(recovery);
        }

        let mut acknowledgements = self.authority.machine.take_distributed_acknowledgements();
        let durable_admission =
            prepares_machine_turn || self.authority.machine.supports_protocol_state();
        if acknowledgements.len() > usize::from(durable_admission) {
            return Err(AdapterError::new(
                AdapterErrorKind::Persistence,
                "distributed persistence acknowledgements exceeded the committed machine turn",
            ));
        }
        if durable_admission {
            self.record_persistent_accept(acknowledgements.pop().as_ref())?;
        }
        Ok(update)
    }

    fn commit_prepared_distributed_transaction(
        &mut self,
        prepared: PreparedDistributedServerTransaction<ProgramSession>,
    ) -> Result<DistributedServerUpdate, AdapterError> {
        let prepares_machine_turn = prepared.prepares_machine_turn();
        let prepared_deliveries = match self
            .distributed_sessions
            .as_ref()
            .ok_or_else(|| {
                AdapterError::new(
                    AdapterErrorKind::Runtime,
                    "distributed Server routing has no Session registry",
                )
            })?
            .prepare_deliveries(prepared.deliveries())
        {
            Ok(prepared_deliveries) => prepared_deliveries,
            Err(error) => {
                let rollback = self
                    .authority
                    .routing
                    .as_mut()
                    .expect("distributed transaction requires routing")
                    .bind(&mut self.authority.machine)
                    .rollback_prepared_transaction(prepared);
                return match rollback {
                    Ok(()) => Err(distributed_registry_adapter_error(error)),
                    Err(rollback) => Err(AdapterError::new(
                        AdapterErrorKind::Runtime,
                        format_args!(
                            "distributed delivery reservation failed: {error}; rollback failed: {rollback}"
                        ),
                    )),
                };
            }
        };
        let prepared_recovery = if self.authority.machine.supports_protocol_state() {
            let router_payload = match prepared.candidate_recovery_payload() {
                Ok(payload) => payload,
                Err(error) => {
                    self.authority
                        .routing
                        .as_mut()
                        .expect("distributed transaction requires routing")
                        .bind(&mut self.authority.machine)
                        .rollback_prepared_transaction(prepared)
                        .map_err(distributed_runtime_adapter_error)?;
                    return Err(distributed_runtime_adapter_error(error));
                }
            };
            match self
                .distributed_sessions
                .as_ref()
                .expect("distributed transaction requires a Session registry")
                .prepare_recovery_checkpoint_with_deliveries(router_payload, &prepared_deliveries)
            {
                Ok(checkpoint) => Some(checkpoint),
                Err(error) => {
                    self.authority
                        .routing
                        .as_mut()
                        .expect("distributed transaction requires routing")
                        .bind(&mut self.authority.machine)
                        .rollback_prepared_transaction(prepared)
                        .map_err(distributed_runtime_adapter_error)?;
                    return Err(distributed_registry_adapter_error(error));
                }
            }
        } else {
            None
        };
        let update = {
            let mut authority = self
                .authority
                .routing
                .as_mut()
                .expect("distributed transaction requires routing")
                .bind(&mut self.authority.machine);
            match prepared_recovery.as_ref() {
                Some(recovery) => authority.commit_prepared_transaction_with_protocol_state(
                    prepared,
                    |turn_sequence| {
                        recovery
                            .changes(turn_sequence)
                            .map_err(|error| DistributedRuntimeError::Runtime(error.to_string()))
                    },
                ),
                None => authority.commit_prepared_transaction(prepared),
            }
            .map_err(distributed_runtime_adapter_error)?
        };
        self.distributed_sessions
            .as_mut()
            .expect("distributed transaction requires a Session registry")
            .commit_deliveries(prepared_deliveries);
        if let Some(recovery) = prepared_recovery {
            self.distributed_sessions
                .as_mut()
                .expect("distributed transaction requires a Session registry")
                .commit_recovery_checkpoint(recovery);
        }

        let mut acknowledgements = self.authority.machine.take_distributed_acknowledgements();
        let durable_admission =
            prepares_machine_turn || self.authority.machine.supports_protocol_state();
        if acknowledgements.len() > usize::from(durable_admission) {
            return Err(AdapterError::new(
                AdapterErrorKind::Persistence,
                "distributed persistence acknowledgements exceeded the committed machine turn",
            ));
        }
        if durable_admission {
            self.record_persistent_accept(acknowledgements.pop().as_ref())?;
        }
        Ok(update)
    }

    fn prepare_distributed_global_read(&mut self) -> Result<(), AdapterError> {
        let Some(routing) = self.authority.routing.as_mut() else {
            return Ok(());
        };
        let prepared = routing
            .bind(&mut self.authority.machine)
            .prepare_global_read_update()
            .map_err(distributed_runtime_adapter_error)?;
        self.commit_prepared_distributed_update(prepared)?;
        Ok(())
    }

    fn dispatch_turn(
        &mut self,
        source_path: &str,
        payload: SourcePayload,
        class: ServerTurnClass,
    ) -> Result<ProgramSessionDispatch, AdapterError> {
        self.ensure_admission_open()?;
        let durability = self.durability(class);
        if self.authority.routing.is_some() {
            let result = (|| {
                let prepared = self
                    .authority
                    .routing
                    .as_mut()
                    .expect("checked distributed routing")
                    .bind(&mut self.authority.machine)
                    .prepare_global_source_transaction(
                        source_path,
                        payload,
                        durability == ServerTurnDurability::Immediate,
                    )
                    .map_err(distributed_runtime_adapter_error)?;
                let update = self.commit_prepared_distributed_transaction(prepared)?;
                let mut source_turns = update
                    .turns
                    .into_iter()
                    .filter(|turn| turn.source_sequence.is_some());
                let runtime_turn = source_turns.next().ok_or_else(|| {
                    AdapterError::new(
                        AdapterErrorKind::Runtime,
                        "distributed Global source committed without a source turn",
                    )
                })?;
                if source_turns.next().is_some() {
                    return Err(AdapterError::new(
                        AdapterErrorKind::Runtime,
                        "distributed Global source committed multiple source turns",
                    ));
                }
                let source_sequence = runtime_turn
                    .source_sequence
                    .expect("filtered distributed source turn has a sequence");
                Ok(ProgramSessionDispatch {
                    source_sequence,
                    source_path: source_path.to_owned(),
                    runtime_turn,
                })
            })();
            if let Err(error) = &result {
                self.record_persistent_rejection(error);
            }
            return result;
        }
        let result = self
            .authority
            .machine
            .dispatch(source_path, payload, durability);
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

    fn complete_server_transient_effect(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
        class: ServerTurnClass,
    ) -> Result<RuntimeTurn, AdapterError> {
        let durability = self.durability(class);
        let result = if self.authority.routing.is_some() {
            (|| {
                let prepared = self
                    .authority
                    .routing
                    .as_mut()
                    .expect("checked distributed routing")
                    .bind(&mut self.authority.machine)
                    .prepare_transient_effect_completion_transaction(
                        call_id,
                        outcome,
                        durability == ServerTurnDurability::Immediate,
                    )
                    .map_err(distributed_runtime_adapter_error)?;
                let mut update = self.commit_prepared_distributed_transaction(prepared)?;
                update.turns.pop().ok_or_else(|| {
                    AdapterError::new(
                        AdapterErrorKind::Runtime,
                        "distributed transient effect committed without a runtime turn",
                    )
                })
            })()
        } else {
            self.authority
                .machine
                .complete_transient_effect(call_id, outcome, durability)
                .and_then(|turn| {
                    self.record_persistent_accept(turn.acknowledgement.as_ref())?;
                    Ok(turn.runtime_turn)
                })
        };
        if let Err(error) = &result {
            self.record_persistent_rejection(error);
        }
        result
    }

    fn deliver_server_transient_effect_result(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
        class: ServerTurnClass,
    ) -> Result<RuntimeTurn, AdapterError> {
        let durability = self.durability(class);
        let result = if self.authority.routing.is_some() {
            (|| {
                let prepared = self
                    .authority
                    .routing
                    .as_mut()
                    .expect("checked distributed routing")
                    .bind(&mut self.authority.machine)
                    .prepare_transient_effect_result_transaction(
                        call_id,
                        result_sequence,
                        outcome,
                        durability == ServerTurnDurability::Immediate,
                    )
                    .map_err(distributed_runtime_adapter_error)?;
                let mut update = self.commit_prepared_distributed_transaction(prepared)?;
                update.turns.pop().ok_or_else(|| {
                    AdapterError::new(
                        AdapterErrorKind::Runtime,
                        "distributed stream effect committed without a runtime turn",
                    )
                })
            })()
        } else {
            self.authority
                .machine
                .deliver_transient_effect_result(call_id, result_sequence, outcome, durability)
                .and_then(|turn| {
                    self.record_persistent_accept(turn.acknowledgement.as_ref())?;
                    Ok(turn.runtime_turn)
                })
        };
        if let Err(error) = &result {
            self.record_persistent_rejection(error);
        }
        result
    }

    fn cancel_server_transient_effect(
        &mut self,
        call_id: TransientEffectCallId,
        class: ServerTurnClass,
    ) -> Result<bool, AdapterError> {
        if !self.authority.machine.has_pending_transient_effect(call_id) {
            return Ok(false);
        }
        let durability = self.durability(class);
        if let Some(routing) = self.authority.routing.as_mut() {
            let prepared = routing
                .bind(&mut self.authority.machine)
                .prepare_transient_effect_cancellation_transaction(
                    call_id,
                    durability == ServerTurnDurability::Immediate,
                )
                .map_err(distributed_runtime_adapter_error)?;
            self.commit_prepared_distributed_transaction(prepared)?;
            return Ok(true);
        }
        self.authority.machine.cancel_transient_effect(call_id)
    }

    async fn settle_transient_effects(
        &mut self,
        initial: RuntimeTurn,
        class: ServerTurnClass,
    ) -> Result<(), AdapterError> {
        let owner = TransientRuntimeOwner::DirectServer(class);
        if self.owner_has_active_transient_effects(owner) {
            return Err(AdapterError::new(
                AdapterErrorKind::Runtime,
                "a direct server transaction started while its previous transient effects remained active",
            ));
        }
        if let Err(error) = self.route_transient_runtime_turn(&initial, owner) {
            self.cancel_unregistered_transient_effects(&initial.transient_effects, owner);
            self.cancel_transient_effect_owner(owner);
            return Err(error);
        }

        while self.owner_has_active_transient_effects(owner) {
            let event = match self.transient_effect_host.as_mut() {
                Some(host) => host.next_event().await,
                None => {
                    self.cancel_transient_effect_owner(owner);
                    return Err(AdapterError::new(
                        AdapterErrorKind::Unsupported,
                        "trusted server emitted a transient effect without an attached host",
                    ));
                }
            };
            let event = match event {
                Ok(event) => event,
                Err(error) => {
                    self.cancel_transient_effect_owner(owner);
                    return Err(AdapterError::new(AdapterErrorKind::Runtime, error));
                }
            };
            if let Err(error) = self.apply_transient_host_event(event) {
                self.cancel_transient_effect_owner(owner);
                return Err(error);
            }
        }
        self.transient_effect_budgets.remove(&owner);
        Ok(())
    }

    fn apply_transient_host_event(
        &mut self,
        event: TransientEffectHostEvent,
    ) -> Result<(), AdapterError> {
        let call_id = match &event {
            TransientEffectHostEvent::Result { call_id, .. }
            | TransientEffectHostEvent::Cancelled { call_id } => *call_id,
        };
        let active = self
            .active_transient_effects
            .get(&call_id)
            .cloned()
            .ok_or_else(|| {
                AdapterError::new(
                    AdapterErrorKind::Runtime,
                    "transient effect host returned an event for an unknown call",
                )
            })?;
        let budget = self
            .transient_effect_budgets
            .get_mut(&active.owner)
            .expect("an active transient effect has an owner budget");
        budget.delivered_events = budget.delivered_events.saturating_add(1);
        if budget.delivered_events > self.transient_effect_limits.max_events_per_transaction {
            return Err(AdapterError::new(
                AdapterErrorKind::Runtime,
                "transient effect owner exceeded its bounded event limit",
            ));
        }

        match event {
            TransientEffectHostEvent::Cancelled { call_id } => {
                self.active_transient_effects.remove(&call_id);
                self.cancel_runtime_transient_effect(call_id, active.owner)?;
            }
            TransientEffectHostEvent::Result {
                call_id,
                delivery,
                outcome,
            } => {
                let (turn, pending) = match (&active.delivery, delivery) {
                    (
                        boon_plan::EffectDeliveryCardinality::Single,
                        TransientEffectHostDelivery::Single,
                    ) => self.complete_runtime_transient_effect(call_id, outcome, active.owner)?,
                    (
                        boon_plan::EffectDeliveryCardinality::Stream { .. },
                        TransientEffectHostDelivery::Stream { result_sequence },
                    ) => self.deliver_runtime_transient_effect_result(
                        call_id,
                        result_sequence,
                        outcome,
                        active.owner,
                    )?,
                    _ => {
                        return Err(AdapterError::new(
                            AdapterErrorKind::Runtime,
                            "transient effect host used the wrong delivery shape for a call",
                        ));
                    }
                };
                if !pending {
                    self.active_transient_effects.remove(&call_id);
                }
                if let Some(turn) = turn {
                    self.route_transient_runtime_turn(&turn, active.owner)?;
                }
            }
        }
        if matches!(
            active.owner,
            TransientRuntimeOwner::DistributedServer(_)
                | TransientRuntimeOwner::DistributedSession(_)
        ) {
            self.queue_distributed_follow_up();
        }
        self.remove_idle_transient_effect_budget(active.owner);
        Ok(())
    }

    fn route_transient_runtime_turn(
        &mut self,
        turn: &RuntimeTurn,
        owner: TransientRuntimeOwner,
    ) -> Result<(), AdapterError> {
        if turn.transient_effects.is_empty()
            && turn.cancelled_transient_effects.is_empty()
            && turn.transient_effect_credit_grants.is_empty()
        {
            return Ok(());
        }
        if self.transient_effect_host.is_none() {
            return Err(AdapterError::new(
                AdapterErrorKind::Unsupported,
                "trusted server emitted a transient effect without an attached host",
            ));
        }

        let mut cancelled_owners = BTreeSet::new();
        for call_id in &turn.cancelled_transient_effects {
            let Some(cancelled) = self.active_transient_effects.remove(call_id) else {
                return Err(AdapterError::new(
                    AdapterErrorKind::Runtime,
                    "runtime cancelled a transient effect not owned by the host",
                ));
            };
            cancelled_owners.insert(cancelled.owner);
        }
        if let Some(host) = self.transient_effect_host.as_mut() {
            for call_id in &turn.cancelled_transient_effects {
                host.cancel(*call_id);
            }
            host.grant_credits(&turn.transient_effect_credit_grants)
                .map_err(|error| AdapterError::new(AdapterErrorKind::Runtime, error))?;
        }

        let calls = &turn.transient_effects;
        if calls.is_empty() {
            for cancelled_owner in cancelled_owners {
                self.remove_idle_transient_effect_budget(cancelled_owner);
            }
            return Ok(());
        }
        let submitted_calls = self
            .transient_effect_budgets
            .get(&owner)
            .map_or(0, |budget| budget.submitted_calls);
        if submitted_calls.saturating_add(calls.len())
            > self.transient_effect_limits.max_calls_per_transaction
            || self
                .active_transient_effects
                .len()
                .saturating_add(calls.len())
                > self.transient_effect_limits.max_active_calls
        {
            return Err(AdapterError::new(
                AdapterErrorKind::Runtime,
                "transient effect transaction exceeded its bounded call limit",
            ));
        }
        let mut batch = BTreeSet::new();
        for call in calls {
            if !self
                .transient_effect_host
                .as_ref()
                .expect("host checked above")
                .owns(call.effect_id)
            {
                return Err(AdapterError::new(
                    AdapterErrorKind::Unsupported,
                    format_args!(
                        "transient effect host does not own effect {}",
                        call.effect_id
                    ),
                ));
            }
            if self.active_transient_effects.contains_key(&call.call_id)
                || !batch.insert(call.call_id)
            {
                return Err(AdapterError::new(
                    AdapterErrorKind::Runtime,
                    format_args!("duplicate transient effect call {}", call.call_id),
                ));
            }
        }
        for call in calls {
            self.active_transient_effects.insert(
                call.call_id,
                ActiveTransientEffect {
                    delivery: call.delivery.clone(),
                    owner,
                },
            );
        }
        self.transient_effect_budgets
            .entry(owner)
            .or_default()
            .submitted_calls = submitted_calls.saturating_add(calls.len());
        self.transient_effect_host
            .as_mut()
            .expect("host checked above")
            .submit(calls.clone())
            .map_err(|error| AdapterError::new(AdapterErrorKind::Runtime, error))?;
        for cancelled_owner in cancelled_owners {
            self.remove_idle_transient_effect_budget(cancelled_owner);
        }
        Ok(())
    }

    fn cancel_unregistered_transient_effects(
        &mut self,
        calls: &[TransientEffectInvocation],
        owner: TransientRuntimeOwner,
    ) {
        for call in calls {
            if !self.active_transient_effects.contains_key(&call.call_id) {
                self.cancel_runtime_transient_effect_best_effort(call.call_id, owner);
            }
        }
    }

    fn cancel_active_transient_effects(&mut self) {
        let active = std::mem::take(&mut self.active_transient_effects);
        if let Some(host) = self.transient_effect_host.as_mut() {
            for call_id in active.keys() {
                host.cancel(*call_id);
            }
        }
        for (call_id, active) in active {
            self.cancel_runtime_transient_effect_best_effort(call_id, active.owner);
        }
        self.transient_effect_budgets.clear();
    }

    fn cancel_transient_effect_owner(&mut self, owner: TransientRuntimeOwner) {
        let calls = self
            .active_transient_effects
            .iter()
            .filter_map(|(call_id, active)| (active.owner == owner).then_some(*call_id))
            .collect::<Vec<_>>();
        if let Some(host) = self.transient_effect_host.as_mut() {
            for call_id in &calls {
                host.cancel(*call_id);
            }
        }
        for call_id in calls {
            self.active_transient_effects.remove(&call_id);
            self.cancel_runtime_transient_effect_best_effort(call_id, owner);
        }
        self.transient_effect_budgets.remove(&owner);
    }

    fn owner_has_active_transient_effects(&self, owner: TransientRuntimeOwner) -> bool {
        self.active_transient_effects
            .values()
            .any(|active| active.owner == owner)
    }

    fn remove_idle_transient_effect_budget(&mut self, owner: TransientRuntimeOwner) {
        if !self.owner_has_active_transient_effects(owner) {
            self.transient_effect_budgets.remove(&owner);
        }
    }

    fn complete_runtime_transient_effect(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
        owner: TransientRuntimeOwner,
    ) -> Result<(Option<RuntimeTurn>, bool), AdapterError> {
        match owner {
            TransientRuntimeOwner::DirectServer(class) => {
                let turn = self.complete_server_transient_effect(call_id, outcome, class)?;
                let pending = self.authority.machine.has_pending_transient_effect(call_id);
                Ok((Some(turn), pending))
            }
            TransientRuntimeOwner::DistributedServer(_) => {
                let turn = self.complete_server_transient_effect(
                    call_id,
                    outcome,
                    ServerTurnClass::Distributed,
                )?;
                let pending = self.authority.machine.has_pending_transient_effect(call_id);
                Ok((Some(turn), pending))
            }
            TransientRuntimeOwner::DistributedSession(origin) => {
                let pending =
                    self.complete_distributed_session_transient_effect(origin, call_id, outcome)?;
                Ok((None, pending))
            }
        }
    }

    fn deliver_runtime_transient_effect_result(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
        owner: TransientRuntimeOwner,
    ) -> Result<(Option<RuntimeTurn>, bool), AdapterError> {
        match owner {
            TransientRuntimeOwner::DirectServer(class) => {
                let turn = self.deliver_server_transient_effect_result(
                    call_id,
                    result_sequence,
                    outcome,
                    class,
                )?;
                let pending = self.authority.machine.has_pending_transient_effect(call_id);
                Ok((Some(turn), pending))
            }
            TransientRuntimeOwner::DistributedServer(_) => {
                let turn = self.deliver_server_transient_effect_result(
                    call_id,
                    result_sequence,
                    outcome,
                    ServerTurnClass::Distributed,
                )?;
                let pending = self.authority.machine.has_pending_transient_effect(call_id);
                Ok((Some(turn), pending))
            }
            TransientRuntimeOwner::DistributedSession(origin) => {
                let pending = self.deliver_distributed_session_transient_effect_result(
                    origin,
                    call_id,
                    result_sequence,
                    outcome,
                )?;
                Ok((None, pending))
            }
        }
    }

    fn cancel_runtime_transient_effect(
        &mut self,
        call_id: TransientEffectCallId,
        owner: TransientRuntimeOwner,
    ) -> Result<(), AdapterError> {
        match owner {
            TransientRuntimeOwner::DirectServer(class) => {
                self.cancel_server_transient_effect(call_id, class)?;
            }
            TransientRuntimeOwner::DistributedServer(_) => {
                self.cancel_server_transient_effect(call_id, ServerTurnClass::Distributed)?;
            }
            TransientRuntimeOwner::DistributedSession(origin) => {
                self.cancel_distributed_session_transient_effect(origin, call_id)?;
            }
        }
        Ok(())
    }

    fn complete_distributed_session_transient_effect(
        &mut self,
        origin: boon_runtime::SessionOrigin,
        call_id: TransientEffectCallId,
        outcome: Value,
    ) -> Result<bool, AdapterError> {
        let routing = self.authority.routing.as_mut().ok_or_else(|| {
            AdapterError::new(
                AdapterErrorKind::Runtime,
                "distributed Session effect completion has no Server router",
            )
        })?;
        let sessions = self.distributed_sessions.as_mut().ok_or_else(|| {
            AdapterError::new(
                AdapterErrorKind::Runtime,
                "distributed Session effect completion has no Session registry",
            )
        })?;
        let mut authority = routing.bind(&mut self.authority.machine);
        sessions
            .complete_session_transient_effect(&mut authority, origin, call_id, outcome)
            .map_err(distributed_registry_adapter_error)
    }

    fn deliver_distributed_session_transient_effect_result(
        &mut self,
        origin: boon_runtime::SessionOrigin,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
    ) -> Result<bool, AdapterError> {
        let routing = self.authority.routing.as_mut().ok_or_else(|| {
            AdapterError::new(
                AdapterErrorKind::Runtime,
                "distributed Session stream completion has no Server router",
            )
        })?;
        let sessions = self.distributed_sessions.as_mut().ok_or_else(|| {
            AdapterError::new(
                AdapterErrorKind::Runtime,
                "distributed Session stream completion has no Session registry",
            )
        })?;
        let mut authority = routing.bind(&mut self.authority.machine);
        sessions
            .deliver_session_transient_effect_result(
                &mut authority,
                origin,
                call_id,
                result_sequence,
                outcome,
            )
            .map_err(distributed_registry_adapter_error)
    }

    fn cancel_distributed_session_transient_effect(
        &mut self,
        origin: boon_runtime::SessionOrigin,
        call_id: TransientEffectCallId,
    ) -> Result<(), AdapterError> {
        let routing = self.authority.routing.as_mut().ok_or_else(|| {
            AdapterError::new(
                AdapterErrorKind::Runtime,
                "distributed Session effect cancellation has no Server router",
            )
        })?;
        let sessions = self.distributed_sessions.as_mut().ok_or_else(|| {
            AdapterError::new(
                AdapterErrorKind::Runtime,
                "distributed Session effect cancellation has no Session registry",
            )
        })?;
        let mut authority = routing.bind(&mut self.authority.machine);
        sessions
            .cancel_session_transient_effect(&mut authority, origin, call_id)
            .map_err(distributed_registry_adapter_error)
    }

    fn cancel_runtime_transient_effect_best_effort(
        &mut self,
        call_id: TransientEffectCallId,
        owner: TransientRuntimeOwner,
    ) {
        let _ = self.cancel_runtime_transient_effect(call_id, owner);
    }

    fn has_distributed_transient_effect_work(&self) -> bool {
        self.active_transient_effects.values().any(|active| {
            matches!(
                active.owner,
                TransientRuntimeOwner::DistributedServer(_)
                    | TransientRuntimeOwner::DistributedSession(_)
            )
        })
    }

    fn queue_distributed_follow_up(&mut self) {
        let now = match self.distributed_now(Instant::now()) {
            Ok(now) => now,
            Err(error) => {
                self.fail_distributed_transient_work(error);
                return;
            }
        };
        let actions = self.poll_distributed_transport(now);
        self.pending_distributed_actions.extend(actions);
    }

    fn take_pending_distributed_actions(&mut self) -> Vec<DistributedSessionAction> {
        self.pending_distributed_actions.drain(..).collect()
    }

    fn cancel_distributed_transient_effects(&mut self) {
        let owners = self
            .active_transient_effects
            .values()
            .filter_map(|active| {
                matches!(
                    active.owner,
                    TransientRuntimeOwner::DistributedServer(_)
                        | TransientRuntimeOwner::DistributedSession(_)
                )
                .then_some(active.owner)
            })
            .collect::<BTreeSet<_>>();
        for owner in owners {
            self.cancel_transient_effect_owner(owner);
        }
    }

    fn fail_distributed_transient_work(&mut self, error: impl Display) {
        let diagnostic = error.to_string();
        self.cancel_distributed_transient_effects();
        self.pending_distributed_actions.clear();
        let actions = match self.distributed_now(Instant::now()) {
            Ok(now) => self.fail_all_distributed_transport_connections(now, &diagnostic),
            Err(clock_error) => {
                self.record_distributed_transport_failure(format_args!(
                    "{diagnostic}; distributed cleanup clock failed: {clock_error}"
                ));
                std::mem::take(&mut self.distributed_transport_connections)
                    .into_keys()
                    .map(|connection| {
                        DistributedSessionAction::close(
                            connection,
                            WebSocketClose::new(1011, "distributed Session runtime failed"),
                        )
                    })
                    .collect()
            }
        };
        self.pending_distributed_actions.extend(actions);
    }

    async fn service_distributed_transient_effect_work(&mut self) -> Vec<DistributedSessionAction> {
        if !self.pending_distributed_actions.is_empty() {
            return self.take_pending_distributed_actions();
        }
        if !self.has_distributed_transient_effect_work() {
            return Vec::new();
        }
        let event = match self.transient_effect_host.as_mut() {
            Some(host) => host.next_event().await,
            None => Err(TransientEffectHostError::new(
                "distributed runtime emitted a transient effect without an attached host",
            )),
        };
        match event {
            Ok(event) => {
                if let Err(error) = self.apply_transient_host_event(event) {
                    self.fail_distributed_transient_work(error);
                }
            }
            Err(error) => {
                self.fail_distributed_transient_work(error);
            }
        }
        self.take_pending_distributed_actions()
    }

    fn record_persistent_accept(
        &mut self,
        acknowledgement: Option<&CommitAck>,
    ) -> Result<(), AdapterError> {
        let persistence = self.authority.machine.persistence_status();
        let Some(state) = self.authority.persistence.as_mut() else {
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
        let persistence = self.authority.machine.persistence_status();
        let Some(state) = self.authority.persistence.as_mut() else {
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
        let persistence = self.authority.machine.persistence_status();
        let Some(state) = self.authority.persistence.as_mut() else {
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
        self.settle_transient_effects(dispatched.runtime_turn, ServerTurnClass::Http)
            .await?;
        self.prepare_distributed_global_read()?;
        let value = match self.authority.machine.output_value_current(&output_name) {
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
        self.settle_transient_effects(dispatched.runtime_turn, ServerTurnClass::WebSocket)
            .await?;
        self.prepare_distributed_global_read()?;
        let value = match self.authority.machine.output_value_current(&output_name) {
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
        let Some(state) = self.authority.persistence.as_mut() else {
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

        let barrier = self.authority.machine.barrier();
        let shutdown = self.authority.machine.shutdown();
        let persistence = self.authority.machine.persistence_status();
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
                if let Some(state) = self.authority.persistence.as_mut() {
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
    fn has_distributed_session_transport(&self) -> bool {
        self.distributed_sessions.is_some()
    }

    async fn on_distributed_session(
        &mut self,
        connection: HostDistributedSessionConnectionId,
        event: DistributedSessionEvent,
        cancellation: CallCancellation,
    ) -> Vec<DistributedSessionAction> {
        if cancellation.reason().is_some() {
            self.cancel_distributed_transport_connection(connection);
            return Vec::new();
        }
        self.handle_distributed_transport_event(connection, event)
    }

    fn distributed_session_next_deadline(&self) -> Option<Instant> {
        self.distributed_next_deadline()
    }

    async fn on_distributed_session_timer(
        &mut self,
        now: Instant,
        cancellation: CallCancellation,
    ) -> Vec<DistributedSessionAction> {
        if cancellation.reason().is_some() {
            return Vec::new();
        }
        match self.distributed_now(now) {
            Ok(now) => self.poll_distributed_transport(now),
            Err(error) => {
                self.record_distributed_transport_failure(error);
                Vec::new()
            }
        }
    }

    fn has_pending_internal_work(&self) -> bool {
        !self.pending_distributed_actions.is_empty() || self.has_distributed_transient_effect_work()
    }

    async fn on_internal_work(&mut self) -> Vec<DistributedSessionAction> {
        self.service_distributed_transient_effect_work().await
    }

    fn on_distributed_session_send_accepted(
        &mut self,
        connection: HostDistributedSessionConnectionId,
    ) {
        self.acknowledge_distributed_transport_send(connection);
    }

    async fn on_distributed_session_cancelled(
        &mut self,
        connection: Option<HostDistributedSessionConnectionId>,
        _reason: CancellationReason,
    ) {
        if let Some(connection) = connection {
            self.cancel_distributed_transport_connection(connection);
        } else {
            self.cancel_all_distributed_transport_connections();
        }
    }

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
        self.cancel_all_distributed_transport_connections();
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

fn collect_required_transient_effects<'a>(
    artifacts: impl IntoIterator<Item = &'a ProgramArtifact>,
) -> Result<BTreeMap<boon_plan::EffectId, String>, AdapterError> {
    let mut required = BTreeMap::new();
    for artifact in artifacts {
        for effect in &artifact.plan().effects {
            let operation = effect.host_operation.to_string();
            if let Some(previous) = required.insert(effect.effect_id, operation.clone())
                && previous != operation
            {
                return Err(AdapterError::new(
                    AdapterErrorKind::InvalidArtifact,
                    format_args!(
                        "effect {} maps to both `{previous}` and `{operation}` across distributed roles",
                        effect.effect_id
                    ),
                ));
            }
        }
    }
    Ok(required)
}

fn validate_transient_effect_host(
    host: &dyn TransientEffectHost,
    required: &BTreeMap<boon_plan::EffectId, String>,
) -> Result<(), AdapterError> {
    let missing = required
        .iter()
        .filter_map(|(effect_id, operation)| (!host.owns(*effect_id)).then_some(operation.as_str()))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    Err(AdapterError::new(
        AdapterErrorKind::InvalidArtifact,
        format_args!(
            "transient effect host does not own required operations: {}",
            missing.join(", ")
        ),
    ))
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
    if bindings.http.is_none()
        && bindings.websocket.is_none()
        && plan.distributed_endpoint.is_none()
    {
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
            ("body".to_owned(), Value::Bytes(request.body.into())),
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
            ("bytes".to_owned(), Value::Bytes(bytes.into())),
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
        Some(Value::Bytes(value)) => Ok(value.to_vec()),
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
                    validate_fixed_bytes(value.to_vec(), *fixed_len, "HTTP response header value")?
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
        Value::Bytes(value) => {
            validate_fixed_bytes(value.to_vec(), fixed_len, "HTTP response body")
        }
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

#[cfg(test)]
mod transient_host_tests {
    use super::*;
    use async_trait::async_trait;
    use boon_runtime::{
        ApplicationIdentity, ProgramCapabilityProfile, ProgramCompileRequest, RuntimeSourceUnit,
        compile_program_artifact,
    };
    use std::collections::VecDeque;

    const PROGRAM: &str = r#"
store: [
    http_request: SOURCE
    read: SOURCE
    stream_result:
        NotStarted |> HOLD stream_result {
            read |> THEN {
                File/read_stream(
                    file: read.file
                    chunk_bytes: 4
                    retain_content: False
                )
            }
        }
]

outputs: [
    stream_result: store.stream_result
    response: [
        status: 200
        body: BYTES {}
    ]
]

host_ports: [
    http: [
        request: store.http_request
        response: response
    ]
]
"#;

    #[derive(Default)]
    struct HostState {
        submitted: Vec<TransientEffectCallId>,
        credits: Vec<boon_runtime::TransientEffectCreditGrant>,
        cancelled: Vec<TransientEffectCallId>,
    }

    struct ScriptedStreamHost {
        effect_id: boon_plan::EffectId,
        state: Arc<Mutex<HostState>>,
        events: VecDeque<TransientEffectHostEvent>,
    }

    impl ScriptedStreamHost {
        fn new(state: Arc<Mutex<HostState>>) -> Self {
            Self {
                effect_id: boon_plan::EffectId::from_host_operation(
                    boon_effect_schema::FILE_READ_STREAM_OPERATION,
                )
                .unwrap(),
                state,
                events: VecDeque::new(),
            }
        }
    }

    #[async_trait]
    impl TransientEffectHost for ScriptedStreamHost {
        fn owns(&self, effect_id: boon_plan::EffectId) -> bool {
            effect_id == self.effect_id
        }

        fn submit(
            &mut self,
            calls: Vec<TransientEffectInvocation>,
        ) -> Result<(), TransientEffectHostError> {
            if calls.len() != 1 {
                return Err(TransientEffectHostError::new(
                    "scripted stream host expects one call",
                ));
            }
            let call_id = calls[0].call_id;
            self.state.lock().unwrap().submitted.push(call_id);
            self.events.extend([
                TransientEffectHostEvent::Result {
                    call_id,
                    delivery: TransientEffectHostDelivery::Stream { result_sequence: 0 },
                    outcome: tagged_value(
                        "Opened",
                        [
                            ("size", Value::integer(3).unwrap()),
                            (
                                "content_type",
                                Value::Text("application/octet-stream".to_owned()),
                            ),
                            ("display_name", Value::Text("fixture.bin".to_owned())),
                        ],
                    ),
                },
                TransientEffectHostEvent::Result {
                    call_id,
                    delivery: TransientEffectHostDelivery::Stream { result_sequence: 1 },
                    outcome: tagged_value(
                        "Chunk",
                        [
                            ("sequence", Value::integer(0).unwrap()),
                            ("offset", Value::integer(0).unwrap()),
                            ("bytes", Value::Bytes(vec![1, 2, 3].into())),
                        ],
                    ),
                },
                TransientEffectHostEvent::Result {
                    call_id,
                    delivery: TransientEffectHostDelivery::Stream { result_sequence: 2 },
                    outcome: tagged_value(
                        "Finished",
                        [
                            ("byte_count", Value::integer(3).unwrap()),
                            ("digest", Value::Bytes(vec![9; 32].into())),
                            ("retained", tagged_value("NotRetained", [])),
                        ],
                    ),
                },
            ]);
            Ok(())
        }

        async fn next_event(
            &mut self,
        ) -> Result<TransientEffectHostEvent, TransientEffectHostError> {
            self.events
                .pop_front()
                .ok_or_else(|| TransientEffectHostError::new("scripted event queue is empty"))
        }

        fn grant_credits(
            &mut self,
            grants: &[boon_runtime::TransientEffectCreditGrant],
        ) -> Result<(), TransientEffectHostError> {
            self.state.lock().unwrap().credits.extend_from_slice(grants);
            Ok(())
        }

        fn cancel(&mut self, call_id: TransientEffectCallId) {
            self.state.lock().unwrap().cancelled.push(call_id);
        }
    }

    #[tokio::test]
    async fn server_host_lane_delivers_a_bounded_multishot_stream_with_credit() {
        let artifact = compile_program_artifact(&ProgramCompileRequest {
            revision: 1,
            entry_path: "stream-server.bn".to_owned(),
            units: vec![RuntimeSourceUnit {
                path: "stream-server.bn".to_owned(),
                source: PROGRAM.to_owned(),
            }],
            application: ApplicationIdentity::new("dev.boon.server-stream-test", "test", "local"),
            role: boon_plan::ProgramRole::Server,
            capability_profile: ProgramCapabilityProfile::TrustedServer,
        })
        .unwrap();
        let mut program = BoonServerProgram::new(artifact).unwrap();
        let state = Arc::new(Mutex::new(HostState::default()));
        program
            .attach_transient_effect_host(
                Box::new(ScriptedStreamHost::new(Arc::clone(&state))),
                TransientEffectLimits::default(),
            )
            .unwrap();
        let dispatched = program
            .dispatch_turn(
                "store.read",
                SourcePayload {
                    fields: BTreeMap::from([("file".to_owned(), selected_file_value())]),
                    ..SourcePayload::default()
                },
                ServerTurnClass::Http,
            )
            .unwrap();
        let call_id = dispatched.runtime_turn.transient_effects[0].call_id;

        program
            .settle_transient_effects(dispatched.runtime_turn, ServerTurnClass::Http)
            .await
            .unwrap();

        assert_eq!(
            program.authority.machine.pending_transient_effect_count(),
            0
        );
        assert!(matches!(
            program
                .authority
                .machine
                .output_value_current("stream_result")
                .unwrap(),
            Value::Record(fields)
                if fields.get("$tag") == Some(&Value::Text("Finished".to_owned()))
        ));
        let state = state.lock().unwrap();
        assert_eq!(state.submitted, [call_id]);
        assert_eq!(state.credits.len(), 1);
        assert_eq!(state.credits[0].call_id, call_id);
        assert_eq!(state.credits[0].credits, 1);
        assert!(state.cancelled.is_empty());
    }

    fn selected_file_value() -> Value {
        let mut registry = boon_host_runtime::FileCapabilityRegistry::new(1).unwrap();
        registry
            .register_file("/scripted/fixture.bin")
            .unwrap()
            .file_selected_value()
    }

    fn tagged_value(tag: &str, fields: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
        let mut record = BTreeMap::from([("$tag".to_owned(), Value::Text(tag.to_owned()))]);
        record.extend(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value)),
        );
        Value::Record(record)
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::*;
    use async_trait::async_trait;
    use boon_persistence::InMemoryDriver;
    use boon_runtime::{
        ApplicationIdentity, DistributedClientRuntime, DistributedQueueLimits,
        ProgramCompileRequest, RuntimeSourceUnit, compile_distributed_program_bundle,
    };
    use boon_wire::{
        ClientCommit, ClientHello, ServerReady, SessionControlFrame, SessionId,
        decode_session_control_frame, encode_session_control_frame,
    };

    const CLIENT: &str = r#"
store: [
    increment: SOURCE
    count: Session/store.count
]

scene: Scene/Element/text(
    element: [events: [press: store.increment]]
    style: [width: Fill]
    text: TEXT { Client }
)
"#;

    fn connect_and_limit_delivery_queue(
        program: &mut BoonServerProgram,
    ) -> (DistributedSessionConnectionId, SessionId, u64, u64) {
        let identity = program.distributed_identity().unwrap();
        let hello =
            encode_session_control_frame(&SessionControlFrame::ClientHello(ClientHello::new(
                identity.graph_id,
                identity.graph_revision,
                identity.schema_hash,
                None,
                0,
            )))
            .unwrap();
        let DistributedSessionHandshakeStart::Offer(offer) = program
            .begin_distributed_handshake(Duration::ZERO, SessionPrincipal::Anonymous, &hello)
            .unwrap()
        else {
            panic!("fresh Session should be offered");
        };
        let connection = offer.connection_id();
        let SessionControlFrame::ServerOffer(server_offer) =
            decode_session_control_frame(offer.server_frame()).unwrap()
        else {
            panic!("fresh Session should provide an offer");
        };
        let (_, session_id, offered_generation, applied_client_through) = server_offer.into_parts();
        let commit = encode_session_control_frame(&SessionControlFrame::ClientCommit(
            ClientCommit::new(session_id, offered_generation, 0),
        ))
        .unwrap();
        let ready = program
            .commit_distributed_handshake(Duration::ZERO, connection, &commit)
            .unwrap();
        let SessionControlFrame::ServerReady(ready) = decode_session_control_frame(&ready).unwrap()
        else {
            panic!("fresh Session should become current");
        };
        let generation = ServerReady::generation(&ready);
        let initial = program
            .poll_distributed_sessions(Duration::ZERO, 64)
            .unwrap();
        assert!(
            initial.poisoned_sessions.is_empty(),
            "post-handshake Session was poisoned: {}",
            initial
                .poisoned_sessions
                .iter()
                .map(|poisoned| poisoned.diagnostic.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        );
        assert!(!initial.serviced_connections.is_empty());
        assert!(
            initial
                .serviced_connections
                .iter()
                .all(|serviced| *serviced == connection)
        );
        program
            .distributed_sessions
            .as_mut()
            .unwrap()
            .set_session_queue_limits_for_test(boon_runtime::DistributedQueueLimits {
                max_messages: 1,
                max_bytes: 1024 * 1024,
            });
        assert_eq!(generation, offered_generation);
        (connection, session_id, generation, applied_client_through)
    }

    const SESSION: &str = r#"
store: [
    increment: Client/store.increment
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
    first: Server/store.first
    second: Server/store.second
    random: Server/store.random
]
"#;

    const SERVER: &str = r#"
store: [
    bump: SOURCE
    global_randomize: SOURCE
    session_randomize: Session/store.increment
    count:
        0 |> HOLD count {
            bump |> THEN { count + 1 }
        }
    first: count > 0
    second: count > 1
    random:
        NotRequested |> HOLD random {
            LATEST {
                global_randomize |> THEN { Random/bytes(byte_count: 1) }
                session_randomize |> THEN { Random/bytes(byte_count: 1) }
            }
        }
]
"#;

    const SESSION_WITH_EFFECT: &str = r#"
store: [
    increment: Client/store.increment
    count:
        0 |> HOLD count {
            increment |> THEN { count + 1 }
        }
    random:
        NotRequested |> HOLD random {
            increment |> THEN { Random/bytes(byte_count: 1) }
        }
    first: Server/store.first
    second: Server/store.second
    server_random: Server/store.random
]
"#;

    const SERVER_WITHOUT_EFFECTS: &str = r#"
store: [
    first: False
    second: False
    random: NotRequested
]
"#;

    #[derive(Default)]
    struct RandomHostState {
        submitted: Vec<TransientEffectCallId>,
        cancelled: Vec<TransientEffectCallId>,
    }

    struct ScriptedRandomHost {
        effect_id: boon_plan::EffectId,
        state: Arc<Mutex<RandomHostState>>,
        events: VecDeque<TransientEffectHostEvent>,
        next_byte: u8,
    }

    impl ScriptedRandomHost {
        fn new(state: Arc<Mutex<RandomHostState>>) -> Self {
            Self {
                effect_id: boon_plan::EffectId::from_host_operation(
                    boon_effect_schema::SECURE_RANDOM_BYTES_OPERATION,
                )
                .unwrap(),
                state,
                events: VecDeque::new(),
                next_byte: 17,
            }
        }
    }

    #[async_trait]
    impl TransientEffectHost for ScriptedRandomHost {
        fn owns(&self, effect_id: boon_plan::EffectId) -> bool {
            effect_id == self.effect_id
        }

        fn submit(
            &mut self,
            calls: Vec<TransientEffectInvocation>,
        ) -> Result<(), TransientEffectHostError> {
            for call in calls {
                self.state.lock().unwrap().submitted.push(call.call_id);
                let outcome = Value::Record(BTreeMap::from([
                    (
                        "$tag".to_owned(),
                        Value::Text("RandomBytesReady".to_owned()),
                    ),
                    (
                        "bytes".to_owned(),
                        Value::Bytes(vec![self.next_byte].into()),
                    ),
                ]));
                self.next_byte = self.next_byte.saturating_add(12);
                self.events.push_back(TransientEffectHostEvent::Result {
                    call_id: call.call_id,
                    delivery: TransientEffectHostDelivery::Single,
                    outcome,
                });
            }
            Ok(())
        }

        async fn next_event(
            &mut self,
        ) -> Result<TransientEffectHostEvent, TransientEffectHostError> {
            self.events
                .pop_front()
                .ok_or_else(|| TransientEffectHostError::new("scripted random queue is empty"))
        }

        fn cancel(&mut self, call_id: TransientEffectCallId) {
            self.state.lock().unwrap().cancelled.push(call_id);
        }
    }

    struct NoEffectsHost;

    #[async_trait]
    impl TransientEffectHost for NoEffectsHost {
        fn owns(&self, _effect_id: boon_plan::EffectId) -> bool {
            false
        }

        fn submit(
            &mut self,
            _calls: Vec<TransientEffectInvocation>,
        ) -> Result<(), TransientEffectHostError> {
            Err(TransientEffectHostError::new(
                "host without effects cannot accept calls",
            ))
        }

        async fn next_event(
            &mut self,
        ) -> Result<TransientEffectHostEvent, TransientEffectHostError> {
            Err(TransientEffectHostError::new(
                "host without effects cannot produce events",
            ))
        }

        fn cancel(&mut self, _call_id: TransientEffectCallId) {}
    }

    #[test]
    fn distributed_host_admission_includes_session_only_effect_requirements() {
        let bundle = compile_distributed_program_bundle(&[
            request(ProgramRole::Client, CLIENT),
            request(ProgramRole::Session, SESSION_WITH_EFFECT),
            request(ProgramRole::Server, SERVER_WITHOUT_EFFECTS),
        ])
        .unwrap();
        let mut program = BoonServerProgram::new_distributed(
            &bundle,
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();

        let error = program
            .attach_transient_effect_host(Box::new(NoEffectsHost), TransientEffectLimits::default())
            .unwrap_err();
        assert_eq!(error.kind(), AdapterErrorKind::InvalidArtifact);
        assert!(error.diagnostic().contains("Random/bytes"));
    }

    #[tokio::test]
    async fn distributed_session_and_server_effects_settle_on_their_exact_owners() {
        let bundle = compile_distributed_program_bundle(&[
            request(ProgramRole::Client, CLIENT),
            request(ProgramRole::Session, SESSION_WITH_EFFECT),
            request(ProgramRole::Server, SERVER),
        ])
        .unwrap();
        let mut program = BoonServerProgram::new_distributed(
            &bundle,
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();
        let host_state = Arc::new(Mutex::new(RandomHostState::default()));
        program
            .attach_transient_effect_host(
                Box::new(ScriptedRandomHost::new(Arc::clone(&host_state))),
                TransientEffectLimits::default(),
            )
            .unwrap();
        let (connection, session_id, generation, applied_client_through) =
            connect_and_limit_delivery_queue(&mut program);
        program
            .distributed_sessions
            .as_mut()
            .unwrap()
            .set_session_queue_limits_for_test(DistributedQueueLimits::default());

        let mut client = DistributedClientRuntime::start(
            bundle.artifact(ProgramRole::Client).unwrap(),
            DistributedQueueLimits::default(),
        )
        .unwrap();
        client
            .bind(session_id, generation, applied_client_through)
            .unwrap();
        client.mark_current().unwrap();
        client
            .dispatch("store.increment", SourcePayload::default())
            .unwrap();
        let frame = client.next_session_frame().unwrap().unwrap();
        program
            .admit_distributed_client_frame(connection, &frame)
            .unwrap();
        assert!(client.acknowledge_session_frame());

        assert!(
            program
                .poll_distributed_transport(Duration::ZERO)
                .is_empty()
        );
        assert_eq!(program.active_transient_effects.len(), 2);
        assert!(program.active_transient_effects.values().any(|active| {
            matches!(active.owner, TransientRuntimeOwner::DistributedSession(_))
        }));
        assert!(
            program.active_transient_effects.values().any(|active| {
                matches!(active.owner, TransientRuntimeOwner::DistributedServer(_))
            })
        );
        assert!(program.has_pending_internal_work());

        assert!(program.on_internal_work().await.is_empty());
        let session_random = program
            .distributed_sessions
            .as_mut()
            .unwrap()
            .session_root_value_current(connection, "store.random")
            .unwrap();
        assert_eq!(random_bytes(&session_random), &[17]);
        assert!(random_bytes(&global_root(&mut program, "store.random")).is_empty());

        assert!(program.on_internal_work().await.is_empty());
        assert_eq!(
            random_bytes(&global_root(&mut program, "store.random")),
            &[29]
        );
        assert!(program.active_transient_effects.is_empty());
        assert!(!program.has_pending_internal_work());
        let host_state = host_state.lock().unwrap();
        assert_eq!(host_state.submitted.len(), 2);
        assert!(host_state.cancelled.is_empty());
    }

    #[tokio::test]
    async fn distributed_effect_completions_are_durably_acknowledged_per_owner() {
        let bundle = compile_distributed_program_bundle(&[
            request(ProgramRole::Client, CLIENT),
            request(ProgramRole::Session, SESSION_WITH_EFFECT),
            request(ProgramRole::Server, SERVER),
        ])
        .unwrap();
        let (mut program, startup) = BoonServerProgram::with_distributed_persistence(
            &bundle,
            InMemoryDriver::default(),
            PersistentServerConfig::authoritative(PersistenceWorkerConfig::default()),
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();
        let host_state = Arc::new(Mutex::new(RandomHostState::default()));
        program
            .attach_transient_effect_host(
                Box::new(ScriptedRandomHost::new(host_state)),
                TransientEffectLimits::default(),
            )
            .unwrap();
        let (connection, session_id, generation, applied_client_through) =
            connect_and_limit_delivery_queue(&mut program);
        program
            .distributed_sessions
            .as_mut()
            .unwrap()
            .set_session_queue_limits_for_test(DistributedQueueLimits::default());
        let mut client = DistributedClientRuntime::start(
            bundle.artifact(ProgramRole::Client).unwrap(),
            DistributedQueueLimits::default(),
        )
        .unwrap();
        client
            .bind(session_id, generation, applied_client_through)
            .unwrap();
        client.mark_current().unwrap();
        client
            .dispatch("store.increment", SourcePayload::default())
            .unwrap();
        let frame = client.next_session_frame().unwrap().unwrap();
        program
            .admit_distributed_client_frame(connection, &frame)
            .unwrap();
        assert!(client.acknowledge_session_frame());
        assert!(
            program
                .poll_distributed_transport(Duration::ZERO)
                .is_empty()
        );
        let before = startup.lifecycle.status();

        assert!(program.on_internal_work().await.is_empty());
        assert!(program.on_internal_work().await.is_empty());
        let after = startup.lifecycle.status();
        let accepted = after.accepted_turns - before.accepted_turns;
        let acknowledged = after.durably_acknowledged_turns - before.durably_acknowledged_turns;
        assert!(accepted >= 2);
        assert_eq!(acknowledged, accepted);
        assert!(program.active_transient_effects.is_empty());
        program.shutdown_persistent().unwrap();
    }

    fn random_bytes(value: &Value) -> &[u8] {
        let Value::Record(fields) = value else {
            return &[];
        };
        let Some(Value::Bytes(bytes)) = fields.get("bytes") else {
            return &[];
        };
        bytes.as_ref()
    }

    #[test]
    fn global_source_rolls_back_before_bounded_delivery_pressure() {
        let bundle = compile_distributed_program_bundle(&[
            request(ProgramRole::Client, CLIENT),
            request(ProgramRole::Session, SESSION),
            request(ProgramRole::Server, SERVER),
        ])
        .unwrap();
        let mut program = BoonServerProgram::new_distributed(
            &bundle,
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();
        let (connection, _, _, _) = connect_and_limit_delivery_queue(&mut program);

        program
            .dispatch_turn(
                "store.bump",
                SourcePayload::default(),
                ServerTurnClass::Http,
            )
            .unwrap();
        assert_eq!(global_count(&mut program), Value::integer(1).unwrap());

        let blocked = program
            .dispatch_turn(
                "store.bump",
                SourcePayload::default(),
                ServerTurnClass::Http,
            )
            .unwrap_err();
        assert_eq!(blocked.kind(), AdapterErrorKind::Backpressure);
        assert_eq!(global_count(&mut program), Value::integer(1).unwrap());

        let released = program
            .poll_distributed_sessions(Duration::ZERO, 1)
            .unwrap();
        assert_eq!(released.serviced_connections, vec![connection]);
        program
            .dispatch_turn(
                "store.bump",
                SourcePayload::default(),
                ServerTurnClass::Http,
            )
            .unwrap();
        assert_eq!(global_count(&mut program), Value::integer(2).unwrap());
    }

    #[test]
    fn persistent_global_source_has_no_accept_or_ack_before_delivery_capacity() {
        let bundle = compile_distributed_program_bundle(&[
            request(ProgramRole::Client, CLIENT),
            request(ProgramRole::Session, SESSION),
            request(ProgramRole::Server, SERVER),
        ])
        .unwrap();
        let (mut program, startup) = BoonServerProgram::with_distributed_persistence(
            &bundle,
            InMemoryDriver::default(),
            PersistentServerConfig::authoritative(PersistenceWorkerConfig::default()),
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();
        let (connection, _, _, _) = connect_and_limit_delivery_queue(&mut program);
        let baseline = startup.lifecycle.status();

        program
            .dispatch_turn(
                "store.bump",
                SourcePayload::default(),
                ServerTurnClass::Http,
            )
            .unwrap();
        let accepted = startup.lifecycle.status();
        assert_eq!(accepted.accepted_turns, baseline.accepted_turns + 1);
        assert_eq!(
            accepted.durably_acknowledged_turns,
            baseline.durably_acknowledged_turns + 1
        );
        assert_eq!(global_count(&mut program), Value::integer(1).unwrap());

        let blocked = program
            .dispatch_turn(
                "store.bump",
                SourcePayload::default(),
                ServerTurnClass::Http,
            )
            .unwrap_err();
        assert_eq!(blocked.kind(), AdapterErrorKind::Backpressure);
        let rejected = startup.lifecycle.status();
        assert_eq!(rejected.accepted_turns, accepted.accepted_turns);
        assert_eq!(
            rejected.durably_acknowledged_turns,
            accepted.durably_acknowledged_turns
        );
        assert_eq!(rejected.rejected_turns, 1);
        assert!(
            rejected
                .last_error
                .as_deref()
                .is_some_and(|error| error.contains("overloaded"))
        );
        assert_eq!(global_count(&mut program), Value::integer(1).unwrap());

        let released = program
            .poll_distributed_sessions(Duration::ZERO, 1)
            .unwrap();
        assert_eq!(released.serviced_connections, vec![connection]);
        let after_release = startup.lifecycle.status();
        program
            .dispatch_turn(
                "store.bump",
                SourcePayload::default(),
                ServerTurnClass::Http,
            )
            .unwrap();
        let retried = startup.lifecycle.status();
        assert_eq!(retried.accepted_turns, after_release.accepted_turns + 1);
        assert_eq!(
            retried.durably_acknowledged_turns,
            after_release.durably_acknowledged_turns + 1
        );
        assert_eq!(global_count(&mut program), Value::integer(2).unwrap());
        program.shutdown_persistent().unwrap();
    }

    #[test]
    fn global_effect_completion_rolls_back_before_bounded_delivery_pressure() {
        let bundle = compile_distributed_program_bundle(&[
            request(ProgramRole::Client, CLIENT),
            request(ProgramRole::Session, SESSION),
            request(ProgramRole::Server, SERVER),
        ])
        .unwrap();
        let mut program = BoonServerProgram::new_distributed(
            &bundle,
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();
        let (connection, _, _, _) = connect_and_limit_delivery_queue(&mut program);
        program
            .dispatch_turn(
                "store.bump",
                SourcePayload::default(),
                ServerTurnClass::Http,
            )
            .unwrap();
        let started = program
            .dispatch_turn(
                "store.global_randomize",
                SourcePayload::default(),
                ServerTurnClass::Http,
            )
            .unwrap();
        let [invocation] = started.runtime_turn.transient_effects.as_slice() else {
            panic!("randomize should emit exactly one transient effect");
        };
        let call_id = invocation.call_id;
        let outcome = Value::Record(BTreeMap::from([
            (
                "$tag".to_owned(),
                Value::Text("RandomBytesReady".to_owned()),
            ),
            ("bytes".to_owned(), Value::Bytes(vec![7].into())),
        ]));

        let blocked = program
            .complete_server_transient_effect(call_id, outcome.clone(), ServerTurnClass::Http)
            .unwrap_err();
        assert_eq!(blocked.kind(), AdapterErrorKind::Backpressure);
        assert!(
            program
                .authority
                .machine
                .has_pending_transient_effect(call_id)
        );

        let released = program
            .poll_distributed_sessions(Duration::ZERO, 1)
            .unwrap();
        assert_eq!(released.serviced_connections, vec![connection]);
        program
            .complete_server_transient_effect(call_id, outcome.clone(), ServerTurnClass::Http)
            .unwrap();
        assert!(
            !program
                .authority
                .machine
                .has_pending_transient_effect(call_id)
        );
        assert_eq!(global_root(&mut program, "store.random"), outcome);
    }

    #[test]
    fn persistent_effect_completion_has_no_ack_before_delivery_capacity() {
        let bundle = compile_distributed_program_bundle(&[
            request(ProgramRole::Client, CLIENT),
            request(ProgramRole::Session, SESSION),
            request(ProgramRole::Server, SERVER),
        ])
        .unwrap();
        let (mut program, startup) = BoonServerProgram::with_distributed_persistence(
            &bundle,
            InMemoryDriver::default(),
            PersistentServerConfig::authoritative(PersistenceWorkerConfig::default()),
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();
        let (connection, _, _, _) = connect_and_limit_delivery_queue(&mut program);
        let baseline = startup.lifecycle.status();
        program
            .dispatch_turn(
                "store.bump",
                SourcePayload::default(),
                ServerTurnClass::Http,
            )
            .unwrap();
        let started = program
            .dispatch_turn(
                "store.global_randomize",
                SourcePayload::default(),
                ServerTurnClass::Http,
            )
            .unwrap();
        let [invocation] = started.runtime_turn.transient_effects.as_slice() else {
            panic!("randomize should emit exactly one transient effect");
        };
        let call_id = invocation.call_id;
        let outcome = Value::Record(BTreeMap::from([
            (
                "$tag".to_owned(),
                Value::Text("RandomBytesReady".to_owned()),
            ),
            ("bytes".to_owned(), Value::Bytes(vec![9].into())),
        ]));
        let before_completion = startup.lifecycle.status();
        assert_eq!(
            before_completion.accepted_turns,
            baseline.accepted_turns + 2
        );
        assert_eq!(
            before_completion.durably_acknowledged_turns,
            baseline.durably_acknowledged_turns + 2
        );

        let blocked = program
            .complete_server_transient_effect(call_id, outcome.clone(), ServerTurnClass::Http)
            .unwrap_err();
        assert_eq!(blocked.kind(), AdapterErrorKind::Backpressure);
        let after_rejection = startup.lifecycle.status();
        assert_eq!(
            after_rejection.accepted_turns,
            before_completion.accepted_turns
        );
        assert_eq!(
            after_rejection.durably_acknowledged_turns,
            before_completion.durably_acknowledged_turns
        );
        assert_eq!(after_rejection.rejected_turns, 1);
        assert!(
            program
                .authority
                .machine
                .has_pending_transient_effect(call_id)
        );

        let released = program
            .poll_distributed_sessions(Duration::ZERO, 1)
            .unwrap();
        assert_eq!(released.serviced_connections, vec![connection]);
        let after_release = startup.lifecycle.status();
        program
            .complete_server_transient_effect(call_id, outcome.clone(), ServerTurnClass::Http)
            .unwrap();
        let committed = startup.lifecycle.status();
        assert_eq!(committed.accepted_turns, after_release.accepted_turns + 1);
        assert_eq!(
            committed.durably_acknowledged_turns,
            after_release.durably_acknowledged_turns + 1
        );
        assert!(
            !program
                .authority
                .machine
                .has_pending_transient_effect(call_id)
        );
        assert_eq!(global_root(&mut program, "store.random"), outcome);
        program.shutdown_persistent().unwrap();
    }

    #[test]
    fn origin_effect_completion_rolls_back_before_bounded_delivery_pressure() {
        let bundle = compile_distributed_program_bundle(&[
            request(ProgramRole::Client, CLIENT),
            request(ProgramRole::Session, SESSION),
            request(ProgramRole::Server, SERVER),
        ])
        .unwrap();
        let mut program = BoonServerProgram::new_distributed(
            &bundle,
            DistributedSessionRegistryConfig::default(),
        )
        .unwrap();
        let (connection, session_id, generation, applied_client_through) =
            connect_and_limit_delivery_queue(&mut program);
        let mut client = DistributedClientRuntime::start(
            bundle.artifact(ProgramRole::Client).unwrap(),
            DistributedQueueLimits::default(),
        )
        .unwrap();
        client
            .bind(session_id, generation, applied_client_through)
            .unwrap();
        client.mark_current().unwrap();
        client
            .dispatch("store.increment", SourcePayload::default())
            .unwrap();
        let frame = client.next_session_frame().unwrap().unwrap();
        program
            .admit_distributed_client_frame(connection, &frame)
            .unwrap();
        assert!(client.acknowledge_session_frame());
        let origin_turn = program
            .poll_distributed_sessions(Duration::ZERO, 64)
            .unwrap();
        let invocations = origin_turn
            .server_turns
            .iter()
            .flat_map(|(_, turn)| turn.transient_effects.iter())
            .collect::<Vec<_>>();
        let [invocation] = invocations.as_slice() else {
            panic!("Session event should emit exactly one Server transient effect");
        };
        let call_id = invocation.call_id;

        program
            .dispatch_turn(
                "store.bump",
                SourcePayload::default(),
                ServerTurnClass::Http,
            )
            .unwrap();
        let outcome = Value::Record(BTreeMap::from([
            (
                "$tag".to_owned(),
                Value::Text("RandomBytesReady".to_owned()),
            ),
            ("bytes".to_owned(), Value::Bytes(vec![11].into())),
        ]));
        let blocked = program
            .complete_server_transient_effect(call_id, outcome.clone(), ServerTurnClass::Http)
            .unwrap_err();
        assert_eq!(blocked.kind(), AdapterErrorKind::Backpressure);
        assert!(
            program
                .authority
                .machine
                .has_pending_transient_effect(call_id)
        );

        let released = program
            .poll_distributed_sessions(Duration::ZERO, 1)
            .unwrap();
        assert_eq!(released.serviced_connections, vec![connection]);
        program
            .complete_server_transient_effect(call_id, outcome, ServerTurnClass::Http)
            .unwrap();
        assert!(
            !program
                .authority
                .machine
                .has_pending_transient_effect(call_id)
        );
    }

    fn global_count(program: &mut BoonServerProgram) -> Value {
        global_root(program, "store.count")
    }

    fn global_root(program: &mut BoonServerProgram, name: &str) -> Value {
        let ServerAuthority {
            machine, routing, ..
        } = &mut program.authority;
        routing
            .as_mut()
            .unwrap()
            .bind(machine)
            .root_value_current_global(name)
            .unwrap()
    }

    fn request(role: ProgramRole, source: &str) -> ProgramCompileRequest {
        ProgramCompileRequest {
            revision: 1,
            role,
            entry_path: "RUN.bn".to_owned(),
            units: vec![RuntimeSourceUnit {
                path: "RUN.bn".to_owned(),
                source: source.to_owned(),
            }],
            application: ApplicationIdentity::new(
                "dev.boon.distributed-transaction-test",
                format!("test-{}", role.as_str()),
                "distributed-transaction-test",
            ),
            capability_profile: match role {
                ProgramRole::Client => ProgramCapabilityProfile::PublicClient,
                ProgramRole::Session => ProgramCapabilityProfile::TrustedSession,
                ProgramRole::Server => ProgramCapabilityProfile::TrustedServer,
            },
        }
    }
}
