use super::message::{DistributedMessage, DistributedMessagePayload};
use super::{
    DistributedRuntimeError, export_runtime_arguments, export_runtime_value, import_data_arguments,
    runtime_error, set_source_payload_value,
};
use crate::program::ProgramArtifact;
use crate::{
    DistributedImportUpdate, RuntimeTurn, SessionConnectionStatus, SessionContext,
    SessionPrincipal, SourceEvent, SourcePayload, TransientEffectCallId, Value,
};
use boon_data::Value as DataValue;
use boon_plan::{
    DistributedArgumentId, DistributedEndpointContractPlan, DistributedRouteScopePlan,
    DistributedWireSchemaPlan, ExportId, ImportId, ProgramRole, RemoteCallSiteId,
    RemoteCallSitePlan, SourceId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};

const GLOBAL_EFFECT_SCOPE: u64 = u64::MAX;
const MAX_EFFECT_CANCELLATION_ROUNDS: usize = 1024;

/// The narrow authority surface required by distributed Server routing.
///
/// Implementations may be ephemeral or persistent, but every mutating source
/// and effect turn must pass through the implementation's normal admission
/// boundary. Context replacement installs transient remote inputs and is not a
/// second durable authority.
pub trait DistributedServerMachine {
    type EvaluationMachine: DistributedServerMachine;

    fn artifact(&self) -> &ProgramArtifact;

    fn fork_prepared_evaluation(
        &self,
        turn: Option<&RuntimeTurn>,
    ) -> Result<Self::EvaluationMachine, DistributedRuntimeError>;

    fn install_evaluation(
        &mut self,
        evaluation: Self::EvaluationMachine,
    ) -> Result<(), DistributedRuntimeError>;

    fn commit_prepared_evaluation(
        &mut self,
        turn: RuntimeTurn,
        evaluation: Self::EvaluationMachine,
    ) -> Result<RuntimeTurn, DistributedRuntimeError>;

    fn commit_prepared_evaluation_with_protocol_state(
        &mut self,
        turn: RuntimeTurn,
        evaluation: Self::EvaluationMachine,
        protocol_state_changes: Vec<boon_persistence::DurableProtocolStateChange>,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        if !protocol_state_changes.is_empty() {
            return Err(runtime_error(
                "this Server authority cannot persist protocol recovery state",
            ));
        }
        self.commit_prepared_evaluation(turn, evaluation)
    }

    fn event_for_path(
        &self,
        path: &str,
        payload: SourcePayload,
    ) -> Result<SourceEvent, DistributedRuntimeError>;

    fn event_for_source(
        &self,
        source: SourceId,
        payload: SourcePayload,
    ) -> Result<SourceEvent, DistributedRuntimeError>;

    fn prepare_dispatch(
        &mut self,
        event: SourceEvent,
    ) -> Result<RuntimeTurn, DistributedRuntimeError>;

    fn prepare_dispatch_with_durability(
        &mut self,
        event: SourceEvent,
        _durable: bool,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        self.prepare_dispatch(event)
    }

    fn export_current(&mut self, export_id: ExportId) -> Result<Value, DistributedRuntimeError>;

    fn call_arguments(
        &mut self,
        call: &RemoteCallSitePlan,
    ) -> Result<BTreeMap<DistributedArgumentId, Value>, DistributedRuntimeError>;

    fn evaluate_function(
        &mut self,
        export_id: ExportId,
        arguments: BTreeMap<DistributedArgumentId, Value>,
    ) -> Result<Value, DistributedRuntimeError>;

    fn replace_distributed_context(
        &mut self,
        session_context: SessionContext,
        imports: Vec<DistributedImportUpdate>,
    ) -> Result<Option<RuntimeTurn>, DistributedRuntimeError>;

    fn prepare_transient_effect_completion(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
    ) -> Result<RuntimeTurn, DistributedRuntimeError>;

    fn prepare_transient_effect_completion_with_durability(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
        _durable: bool,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        self.prepare_transient_effect_completion(call_id, outcome)
    }

    fn prepare_transient_effect_result(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
    ) -> Result<RuntimeTurn, DistributedRuntimeError>;

    fn prepare_transient_effect_result_with_durability(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
        _durable: bool,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        self.prepare_transient_effect_result(call_id, result_sequence, outcome)
    }

    fn prepare_transient_effect_cancellation(
        &mut self,
        call_ids: &[TransientEffectCallId],
    ) -> Result<Option<RuntimeTurn>, DistributedRuntimeError>;

    fn prepare_transient_effect_cancellation_with_durability(
        &mut self,
        call_ids: &[TransientEffectCallId],
        _durable: bool,
    ) -> Result<Option<RuntimeTurn>, DistributedRuntimeError> {
        self.prepare_transient_effect_cancellation(call_ids)
    }

    fn commit_prepared_turn(
        &mut self,
        turn: RuntimeTurn,
    ) -> Result<RuntimeTurn, DistributedRuntimeError>;

    fn commit_prepared_turn_with_protocol_state(
        &mut self,
        turn: RuntimeTurn,
        protocol_state_changes: Vec<boon_persistence::DurableProtocolStateChange>,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        if !protocol_state_changes.is_empty() {
            return Err(runtime_error(
                "this Server authority cannot persist protocol recovery state",
            ));
        }
        self.commit_prepared_turn(turn)
    }

    fn prepare_protocol_checkpoint(&mut self) -> Result<RuntimeTurn, DistributedRuntimeError> {
        Err(runtime_error(
            "this Server authority cannot prepare a protocol recovery checkpoint",
        ))
    }

    fn supports_protocol_state(&self) -> bool {
        false
    }

    fn rollback_prepared_turn(&mut self) -> Result<(), DistributedRuntimeError>;

    fn has_pending_transient_effect(&self, call_id: TransientEffectCallId) -> bool;

    fn set_transient_effect_scope(&mut self, scope: u64);

    fn root_value_current(&mut self, name: &str) -> Result<Value, DistributedRuntimeError>;
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SessionOrigin {
    slot: u32,
    generation: u64,
}

impl SessionOrigin {
    pub fn new(slot: u32, generation: u64) -> Result<Self, DistributedRuntimeError> {
        if generation == 0 {
            return Err(DistributedRuntimeError::StaleTransportGeneration);
        }
        Ok(Self { slot, generation })
    }

    pub const fn slot(self) -> u32 {
        self.slot
    }

    pub const fn generation(self) -> u64 {
        self.generation
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ServerDeliveryTarget {
    Origin(SessionOrigin),
    AllSessions,
}

#[derive(Clone, Eq, PartialEq)]
pub struct ServerDelivery {
    pub target: ServerDeliveryTarget,
    pub message: DistributedMessage,
}

#[derive(Default)]
pub struct DistributedServerUpdate {
    pub turns: Vec<RuntimeTurn>,
    pub deliveries: Vec<ServerDelivery>,
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
enum ServerContextKey {
    Global,
    Origin(SessionOrigin),
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
enum EffectOwner {
    Global,
    Origin(SessionOrigin),
}

#[derive(Clone, Serialize, Deserialize)]
struct OriginState {
    status: SessionConnectionStatus,
    principal: SessionPrincipal,
    execution_scope: u64,
    imports: BTreeMap<ImportId, (u64, DataValue)>,
    accepted_events: BTreeMap<ExportId, u64>,
    accepted_calls: BTreeMap<RemoteCallSiteId, u64>,
    sent_values: BTreeMap<ExportId, (u64, DataValue)>,
    shared_sent_revisions: BTreeMap<ExportId, u64>,
    sent_calls: BTreeMap<RemoteCallSiteId, (u64, BTreeMap<DistributedArgumentId, DataValue>)>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ServerRouterState {
    machine_context: ServerContextKey,
    origins: BTreeMap<SessionOrigin, OriginState>,
    shared_sent_values: BTreeMap<ExportId, (u64, DataValue)>,
    effect_owners: BTreeMap<TransientEffectCallId, EffectOwner>,
}

/// One authoritative Server machine shared by every Session origin.
///
/// The machine necessarily contains one installed execution snapshot at a
/// time, but no operation trusts that ambient snapshot. Every machine entry
/// installs either the complete Global context or one complete Origin context
/// first. Missing imports are therefore cleared rather than inherited from the
/// previously evaluated Session.
#[derive(Clone)]
pub struct DistributedServerRuntime {
    contract: DistributedEndpointContractPlan,
    wire_schema: DistributedWireSchemaPlan,
    state: ServerRouterState,
}

const SERVER_ROUTER_RECOVERY_FORMAT_VERSION: u16 = 3;

#[derive(Serialize, Deserialize)]
struct ServerRouterRecoveryImage {
    format_version: u16,
    origins: BTreeMap<SessionOrigin, OriginState>,
    shared_sent_values: BTreeMap<ExportId, (u64, DataValue)>,
}

pub struct PreparedDistributedServerUpdate {
    update: DistributedServerUpdate,
    candidate_state: ServerRouterState,
    machine_turn: Option<RuntimeTurn>,
}

pub struct PreparedDistributedServerTransaction<E> {
    update: DistributedServerUpdate,
    candidate_runtime: DistributedServerRuntime,
    evaluation_machine: E,
    machine_turn: Option<(RuntimeTurn, usize)>,
    protocol_checkpoint_turn: Option<RuntimeTurn>,
}

impl<E> PreparedDistributedServerTransaction<E> {
    pub fn deliveries(&self) -> &[ServerDelivery] {
        &self.update.deliveries
    }

    pub fn prepares_machine_turn(&self) -> bool {
        self.machine_turn.is_some()
    }

    pub fn candidate_recovery_payload(&self) -> Result<Vec<u8>, DistributedRuntimeError> {
        encode_server_router_recovery(&self.candidate_runtime.state)
    }
}

impl PreparedDistributedServerUpdate {
    pub fn deliveries(&self) -> &[ServerDelivery] {
        &self.update.deliveries
    }

    pub fn prepares_machine_turn(&self) -> bool {
        self.machine_turn.is_some()
    }

    pub fn candidate_recovery_payload(&self) -> Result<Vec<u8>, DistributedRuntimeError> {
        encode_server_router_recovery(&self.candidate_state)
    }
}

pub struct DistributedServerAuthority<'a, M> {
    runtime: &'a mut DistributedServerRuntime,
    machine: &'a mut M,
}

impl<M> Deref for DistributedServerAuthority<'_, M> {
    type Target = DistributedServerRuntime;

    fn deref(&self) -> &Self::Target {
        self.runtime
    }
}

impl<M> DerefMut for DistributedServerAuthority<'_, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.runtime
    }
}

impl DistributedServerRuntime {
    pub fn start(artifact: &ProgramArtifact) -> Result<Self, DistributedRuntimeError> {
        if artifact.role() != ProgramRole::Server {
            return Err(runtime_error(
                "DistributedServerRuntime requires a Server artifact",
            ));
        }
        let linked = artifact
            .plan()
            .distributed_endpoint
            .as_ref()
            .ok_or_else(|| runtime_error("Server artifact has no distributed endpoint"))?;
        let contract = linked.endpoint.clone();
        let wire_schema = linked.wire_schema.clone();
        Ok(Self {
            contract,
            wire_schema,
            state: ServerRouterState {
                machine_context: ServerContextKey::Global,
                origins: BTreeMap::new(),
                shared_sent_values: BTreeMap::new(),
                effect_owners: BTreeMap::new(),
            },
        })
    }

    pub fn recovery_payload(&self) -> Result<Vec<u8>, DistributedRuntimeError> {
        encode_server_router_recovery(&self.state)
    }

    pub fn start_with_recovery(
        artifact: &ProgramArtifact,
        payload: &[u8],
    ) -> Result<Self, DistributedRuntimeError> {
        if payload.is_empty() || payload.len() > boon_persistence::MAX_PROTOCOL_STATE_RECORD_BYTES {
            return Err(DistributedRuntimeError::InvalidTransportFrame);
        }
        let recovery: ServerRouterRecoveryImage = ciborium::de::from_reader(payload)
            .map_err(|_| DistributedRuntimeError::InvalidTransportFrame)?;
        if recovery.format_version != SERVER_ROUTER_RECOVERY_FORMAT_VERSION {
            return Err(DistributedRuntimeError::ProtocolMismatch);
        }
        let mut canonical = Vec::new();
        ciborium::ser::into_writer(&recovery, &mut canonical)
            .map_err(|_| DistributedRuntimeError::InvalidTransportFrame)?;
        if canonical != payload {
            return Err(DistributedRuntimeError::InvalidTransportFrame);
        }
        let mut runtime = Self::start(artifact)?;
        let state = ServerRouterState {
            machine_context: ServerContextKey::Global,
            origins: recovery.origins,
            shared_sent_values: recovery.shared_sent_values,
            effect_owners: BTreeMap::new(),
        };
        validate_server_router_recovery(runtime.contract.role, &runtime.wire_schema, &state)?;
        runtime.state = state;
        for origin in runtime.state.origins.values_mut() {
            origin.status = SessionConnectionStatus::Stale;
        }
        Ok(runtime)
    }

    pub fn recovery_origin_count(&self) -> usize {
        self.state.origins.len()
    }

    pub fn recovery_origin_matches(
        &self,
        origin: SessionOrigin,
        principal: &SessionPrincipal,
        execution_scope: u64,
    ) -> bool {
        self.state.origins.get(&origin).is_some_and(|state| {
            &state.principal == principal && state.execution_scope == execution_scope
        })
    }

    pub fn bind<'a, M>(&'a mut self, machine: &'a mut M) -> DistributedServerAuthority<'a, M>
    where
        M: DistributedServerMachine,
    {
        let scope = match self.state.machine_context {
            ServerContextKey::Global => GLOBAL_EFFECT_SCOPE,
            ServerContextKey::Origin(origin) => self
                .state
                .origins
                .get(&origin)
                .map(|state| state.execution_scope)
                .unwrap_or(GLOBAL_EFFECT_SCOPE),
        };
        machine.set_transient_effect_scope(scope);
        DistributedServerAuthority {
            runtime: self,
            machine,
        }
    }
}

fn validate_server_router_recovery(
    role: ProgramRole,
    schema: &DistributedWireSchemaPlan,
    state: &ServerRouterState,
) -> Result<(), DistributedRuntimeError> {
    if role != ProgramRole::Server {
        return Err(DistributedRuntimeError::ProtocolMismatch);
    }
    if let ServerContextKey::Origin(origin) = state.machine_context
        && !state.origins.contains_key(&origin)
    {
        return Err(DistributedRuntimeError::InvalidLease);
    }
    let mut scopes = std::collections::BTreeSet::new();
    for origin_state in state.origins.values() {
        if origin_state.execution_scope == 0
            || origin_state.execution_scope == GLOBAL_EFFECT_SCOPE
            || !scopes.insert(origin_state.execution_scope)
        {
            return Err(DistributedRuntimeError::InvalidLease);
        }
        SessionContext::Available {
            status: origin_state.status.clone(),
            principal: origin_state.principal.clone(),
        }
        .validate()
        .map_err(runtime_error)?;
        if origin_state
            .imports
            .iter()
            .any(|(import_id, (revision, _))| {
                *revision == 0
                    || !schema.value_edges.iter().any(|edge| {
                        edge.import_id == *import_id && edge.consumer_role == ProgramRole::Server
                    })
            })
            || origin_state
                .accepted_events
                .iter()
                .any(|(export_id, sequence)| {
                    *sequence == 0
                        || !schema.event_edges.iter().any(|edge| {
                            edge.export_id == *export_id
                                && edge.consumer_role == ProgramRole::Server
                        })
                })
            || origin_state
                .accepted_calls
                .iter()
                .any(|(call_site_id, revision)| {
                    *revision == 0
                        || !schema.call_edges.iter().any(|edge| {
                            edge.call_site_id == *call_site_id
                                && edge.callee_role == ProgramRole::Server
                        })
                })
            || origin_state
                .sent_values
                .iter()
                .any(|(export_id, (revision, _))| {
                    *revision == 0
                        || !schema.value_edges.iter().any(|edge| {
                            edge.export_id == *export_id
                                && edge.producer_role == ProgramRole::Server
                        })
                })
            || origin_state
                .sent_calls
                .iter()
                .any(|(call_site_id, (revision, _))| {
                    *revision == 0
                        || !schema.call_edges.iter().any(|edge| {
                            edge.call_site_id == *call_site_id
                                && edge.caller_role == ProgramRole::Server
                        })
                })
        {
            return Err(DistributedRuntimeError::UnknownTransportEdge);
        }
    }
    if state
        .shared_sent_values
        .values()
        .any(|(revision, _)| *revision == 0)
        || !state.effect_owners.is_empty()
    {
        return Err(DistributedRuntimeError::InvalidLease);
    }
    Ok(())
}

fn encode_server_router_recovery(
    state: &ServerRouterState,
) -> Result<Vec<u8>, DistributedRuntimeError> {
    let recovery = ServerRouterRecoveryImage {
        format_version: SERVER_ROUTER_RECOVERY_FORMAT_VERSION,
        origins: state.origins.clone(),
        shared_sent_values: state.shared_sent_values.clone(),
    };
    let mut payload = Vec::new();
    ciborium::ser::into_writer(&recovery, &mut payload)
        .map_err(|_| runtime_error("failed to encode Server router recovery checkpoint"))?;
    if payload.len() > boon_persistence::MAX_PROTOCOL_STATE_RECORD_BYTES {
        return Err(DistributedRuntimeError::QueueBytesFull {
            limit: boon_persistence::MAX_PROTOCOL_STATE_RECORD_BYTES,
        });
    }
    Ok(payload)
}

impl<M> DistributedServerAuthority<'_, M>
where
    M: DistributedServerMachine,
{
    pub fn supports_protocol_state(&self) -> bool {
        self.machine.supports_protocol_state()
    }

    pub fn commit_protocol_checkpoint(
        &mut self,
        changes: impl FnOnce(
            u64,
        ) -> Result<
            Vec<boon_persistence::DurableProtocolStateChange>,
            DistributedRuntimeError,
        >,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        let turn = self.machine.prepare_protocol_checkpoint()?;
        let protocol_state_changes = match changes(turn.sequence) {
            Ok(changes) => changes,
            Err(error) => {
                self.machine.rollback_prepared_turn()?;
                return Err(error);
            }
        };
        match self
            .machine
            .commit_prepared_turn_with_protocol_state(turn, protocol_state_changes)
        {
            Ok(turn) => Ok(turn),
            Err(error) => {
                let _ = self.machine.rollback_prepared_turn();
                Err(error)
            }
        }
    }

    pub fn attach_origin(
        &mut self,
        origin: SessionOrigin,
        principal: SessionPrincipal,
        execution_scope: u64,
    ) -> Result<(), DistributedRuntimeError> {
        if execution_scope == 0
            || execution_scope == GLOBAL_EFFECT_SCOPE
            || self.state.origins.contains_key(&origin)
            || self
                .state
                .origins
                .values()
                .any(|state| state.execution_scope == execution_scope)
        {
            return Err(runtime_error(
                "Server Session origin or execution scope is already attached or invalid",
            ));
        }
        self.state.origins.insert(
            origin,
            OriginState {
                status: SessionConnectionStatus::Connecting,
                principal,
                execution_scope,
                imports: BTreeMap::new(),
                accepted_events: BTreeMap::new(),
                accepted_calls: BTreeMap::new(),
                sent_values: BTreeMap::new(),
                shared_sent_revisions: BTreeMap::new(),
                sent_calls: BTreeMap::new(),
            },
        );
        Ok(())
    }

    pub fn set_origin_status(
        &mut self,
        origin: SessionOrigin,
        status: SessionConnectionStatus,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        self.state
            .origins
            .get_mut(&origin)
            .ok_or(DistributedRuntimeError::InvalidLease)?
            .status = status;
        self.settle_origin(origin)
    }

    pub fn prepare_origin_status_transaction(
        &mut self,
        origin: SessionOrigin,
        status: SessionConnectionStatus,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        self.require_origin(origin)?;
        self.prepare_evaluated_transaction(
            DistributedServerUpdate::default(),
            None,
            move |authority, update, _| {
                let status_update = authority.set_origin_status(origin, status)?;
                update.turns.extend(status_update.turns);
                update.deliveries.extend(status_update.deliveries);
                Ok(())
            },
        )
    }

    pub fn settle_origin(
        &mut self,
        origin: SessionOrigin,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        self.require_origin(origin)?;
        let mut update = DistributedServerUpdate::default();
        self.enter_origin(origin, &mut update)?;
        if self.origin_inputs_are_current(origin) {
            update
                .deliveries
                .extend(self.collect_origin_outputs(origin)?);
        }
        self.enter_global(&mut update)?;
        update
            .deliveries
            .extend(self.collect_shared_outputs(Some(origin))?);
        Ok(update)
    }

    pub fn expire_origin(
        &mut self,
        origin: SessionOrigin,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        self.require_origin(origin)?;
        let mut update = self.cancel_origin_transient_effects(origin)?;
        self.state
            .origins
            .remove(&origin)
            .ok_or(DistributedRuntimeError::InvalidLease)?;
        self.enter_global(&mut update)?;
        update
            .deliveries
            .retain(|delivery| delivery.target != ServerDeliveryTarget::Origin(origin));
        Ok(update)
    }

    pub fn prepare_origin_expiration_transaction(
        &mut self,
        origin: SessionOrigin,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        self.require_origin(origin)?;
        if self.pending_transient_effect_count(origin) != 0 {
            return Err(runtime_error(
                "Server Session origin cannot expire while it still owns transient effects",
            ));
        }
        self.prepare_evaluated_transaction(
            DistributedServerUpdate::default(),
            None,
            move |authority, update, _| {
                authority
                    .state
                    .origins
                    .remove(&origin)
                    .ok_or(DistributedRuntimeError::InvalidLease)?;
                authority.enter_global(update)?;
                update
                    .deliveries
                    .retain(|delivery| delivery.target != ServerDeliveryTarget::Origin(origin));
                Ok(())
            },
        )
    }

    pub fn accept_session_message(
        &mut self,
        origin: SessionOrigin,
        message: DistributedMessage,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let prepared = self.prepare_session_message(origin, message)?;
        self.commit_prepared_update(prepared)
    }

    pub fn prepare_session_message(
        &mut self,
        origin: SessionOrigin,
        message: DistributedMessage,
    ) -> Result<PreparedDistributedServerUpdate, DistributedRuntimeError> {
        if message.producer != ProgramRole::Session || message.consumer != ProgramRole::Server {
            return Err(DistributedRuntimeError::UnknownTransportEdge);
        }
        self.require_origin(origin)?;
        let original_state = self.runtime.state.clone();
        let result = match message.payload {
            DistributedMessagePayload::Current {
                export_id,
                revision,
                value,
            } => self
                .accept_current(origin, export_id, revision, value)
                .map(|update| (update, None)),
            DistributedMessagePayload::Event {
                export_id,
                sequence,
                value,
            } => self.prepare_event(origin, export_id, sequence, value),
            DistributedMessagePayload::CallRequest {
                call_site_id,
                function_export_id,
                revision,
                arguments,
            } => self
                .accept_call_request(
                    origin,
                    call_site_id,
                    function_export_id,
                    revision,
                    arguments,
                )
                .map(|update| (update, None)),
            DistributedMessagePayload::CallResult {
                call_site_id,
                revision,
                value,
            } => self
                .accept_call_result(origin, call_site_id, revision, value)
                .map(|update| (update, None)),
        };
        match result {
            Ok((update, machine_turn)) => {
                let candidate_state = std::mem::replace(&mut self.runtime.state, original_state);
                self.runtime.state.machine_context = candidate_state.machine_context;
                self.restore_effect_scope();
                Ok(PreparedDistributedServerUpdate {
                    update,
                    candidate_state,
                    machine_turn,
                })
            }
            Err(error) => {
                let actual_context = self.runtime.state.machine_context;
                self.runtime.state = original_state;
                self.runtime.state.machine_context = actual_context;
                self.restore_effect_scope();
                Err(error)
            }
        }
    }

    pub fn commit_prepared_update(
        &mut self,
        mut prepared: PreparedDistributedServerUpdate,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        if let Some(turn) = prepared.machine_turn.take() {
            match self.machine.commit_prepared_turn(turn) {
                Ok(turn) => prepared.update.turns.push(turn),
                Err(error) => {
                    let _ = self.machine.rollback_prepared_turn();
                    self.restore_effect_scope();
                    return Err(error);
                }
            }
        }
        self.runtime.state = prepared.candidate_state;
        self.restore_effect_scope();
        Ok(prepared.update)
    }

    pub fn commit_prepared_update_with_protocol_state(
        &mut self,
        mut prepared: PreparedDistributedServerUpdate,
        changes: impl FnOnce(
            u64,
        ) -> Result<
            Vec<boon_persistence::DurableProtocolStateChange>,
            DistributedRuntimeError,
        >,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let synthesized_checkpoint = prepared.machine_turn.is_none();
        let turn = match prepared.machine_turn.take() {
            Some(turn) => turn,
            None => self.machine.prepare_protocol_checkpoint()?,
        };
        let protocol_state_changes = match changes(turn.sequence) {
            Ok(changes) => changes,
            Err(error) => {
                let _ = self.machine.rollback_prepared_turn();
                self.restore_effect_scope();
                return Err(error);
            }
        };
        let committed = match self
            .machine
            .commit_prepared_turn_with_protocol_state(turn, protocol_state_changes)
        {
            Ok(turn) => turn,
            Err(error) => {
                let _ = self.machine.rollback_prepared_turn();
                self.restore_effect_scope();
                return Err(error);
            }
        };
        if !synthesized_checkpoint {
            prepared.update.turns.push(committed);
        }
        self.runtime.state = prepared.candidate_state;
        self.restore_effect_scope();
        Ok(prepared.update)
    }

    pub fn rollback_prepared_update(
        &mut self,
        prepared: PreparedDistributedServerUpdate,
    ) -> Result<(), DistributedRuntimeError> {
        if prepared.machine_turn.is_some() {
            self.machine.rollback_prepared_turn()?;
        }
        self.restore_effect_scope();
        Ok(())
    }

    fn finish_prepared_router_update(
        &mut self,
        original_state: ServerRouterState,
        result: Result<(DistributedServerUpdate, Option<RuntimeTurn>), DistributedRuntimeError>,
    ) -> Result<PreparedDistributedServerUpdate, DistributedRuntimeError> {
        match result {
            Ok((update, machine_turn)) => {
                let candidate_state = std::mem::replace(&mut self.runtime.state, original_state);
                self.runtime.state.machine_context = candidate_state.machine_context;
                self.restore_effect_scope();
                Ok(PreparedDistributedServerUpdate {
                    update,
                    candidate_state,
                    machine_turn,
                })
            }
            Err(error) => {
                let actual_context = self.runtime.state.machine_context;
                self.runtime.state = original_state;
                self.runtime.state.machine_context = actual_context;
                self.restore_effect_scope();
                Err(error)
            }
        }
    }

    pub fn prepare_global_source_transaction(
        &mut self,
        source_path: &str,
        payload: SourcePayload,
        durable: bool,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        let mut update = DistributedServerUpdate::default();
        self.enter_global(&mut update)?;
        let event = self.machine.event_for_path(source_path, payload)?;
        self.reject_imported_event_source(event.source)?;
        let turn = self
            .machine
            .prepare_dispatch_with_durability(event, durable)?;
        self.prepare_evaluated_transaction(update, Some(turn), |authority, update, turn| {
            let turn = turn.expect("Global source transaction has a machine turn");
            authority.record_transient_effects(EffectOwner::Global, std::slice::from_ref(turn))?;
            authority.collect_after_global_turn(update)
        })
    }

    pub fn prepare_global_read_update(
        &mut self,
    ) -> Result<PreparedDistributedServerUpdate, DistributedRuntimeError> {
        let original_state = self.runtime.state.clone();
        let result = (|| {
            let mut update = DistributedServerUpdate::default();
            self.enter_global(&mut update)?;
            Ok((update, None))
        })();
        self.finish_prepared_router_update(original_state, result)
    }

    fn prepare_transient_effect_transaction(
        &mut self,
        call_id: TransientEffectCallId,
        prepare: impl FnOnce(&mut M) -> Result<RuntimeTurn, DistributedRuntimeError>,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        let owner = self
            .state
            .effect_owners
            .get(&call_id)
            .copied()
            .ok_or(DistributedRuntimeError::InvalidLease)?;
        let mut update = DistributedServerUpdate::default();
        self.enter_effect_owner(owner, &mut update)?;
        let turn = prepare(self.machine)?;
        self.prepare_evaluated_transaction(update, Some(turn), move |authority, update, turn| {
            let turn = turn.expect("transient-effect transaction has a machine turn");
            authority.record_transient_effects(owner, std::slice::from_ref(turn))?;
            authority.finish_effect_owner(owner, update)?;
            if !authority.machine.has_pending_transient_effect(call_id) {
                authority.state.effect_owners.remove(&call_id);
            }
            Ok(())
        })
    }

    pub fn prepare_transient_effect_completion_transaction(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
        durable: bool,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        self.prepare_transient_effect_transaction(call_id, |machine| {
            machine.prepare_transient_effect_completion_with_durability(call_id, outcome, durable)
        })
    }

    pub fn prepare_transient_effect_result_transaction(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
        durable: bool,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        self.prepare_transient_effect_transaction(call_id, |machine| {
            machine.prepare_transient_effect_result_with_durability(
                call_id,
                result_sequence,
                outcome,
                durable,
            )
        })
    }

    pub fn prepare_transient_effect_cancellation_transaction(
        &mut self,
        call_id: TransientEffectCallId,
        durable: bool,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        let owner = self
            .state
            .effect_owners
            .get(&call_id)
            .copied()
            .ok_or(DistributedRuntimeError::InvalidLease)?;
        let mut update = DistributedServerUpdate::default();
        self.enter_effect_owner(owner, &mut update)?;
        let turn = self
            .machine
            .prepare_transient_effect_cancellation_with_durability(&[call_id], durable)?;
        self.prepare_evaluated_transaction(update, turn, move |authority, update, turn| {
            if let Some(turn) = turn {
                authority.record_transient_effects(owner, std::slice::from_ref(turn))?;
            }
            authority.state.effect_owners.remove(&call_id);
            authority.finish_effect_owner(owner, update)
        })
    }

    fn prepare_evaluated_transaction(
        &mut self,
        mut update: DistributedServerUpdate,
        machine_turn: Option<RuntimeTurn>,
        evaluate: impl FnOnce(
            &mut DistributedServerAuthority<'_, M::EvaluationMachine>,
            &mut DistributedServerUpdate,
            Option<&RuntimeTurn>,
        ) -> Result<(), DistributedRuntimeError>,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        let protocol_checkpoint_turn =
            if machine_turn.is_none() && self.machine.supports_protocol_state() {
                Some(self.machine.prepare_protocol_checkpoint()?)
            } else {
                None
            };
        let prepared_turn = machine_turn.as_ref().or(protocol_checkpoint_turn.as_ref());
        let evaluation_machine = match self.machine.fork_prepared_evaluation(prepared_turn) {
            Ok(machine) => machine,
            Err(error) => {
                return self.fail_evaluated_transaction(prepared_turn.is_some(), error);
            }
        };
        let mut candidate_runtime = self.runtime.clone();
        let mut evaluation_machine = evaluation_machine;
        let result = {
            let mut authority = candidate_runtime.bind(&mut evaluation_machine);
            evaluate(&mut authority, &mut update, machine_turn.as_ref())
        };
        if let Err(error) = result {
            return self.fail_evaluated_transaction(prepared_turn.is_some(), error);
        }
        let machine_turn = machine_turn.map(|turn| {
            let index = update.turns.len();
            update.turns.push(turn.clone());
            (turn, index)
        });
        Ok(PreparedDistributedServerTransaction {
            update,
            candidate_runtime,
            evaluation_machine,
            machine_turn,
            protocol_checkpoint_turn,
        })
    }

    fn fail_evaluated_transaction<T>(
        &mut self,
        has_machine_turn: bool,
        error: DistributedRuntimeError,
    ) -> Result<T, DistributedRuntimeError> {
        if has_machine_turn && let Err(rollback) = self.machine.rollback_prepared_turn() {
            return Err(runtime_error(format!(
                "distributed Server evaluation failed: {error}; rollback failed: {rollback}"
            )));
        }
        self.restore_effect_scope();
        Err(error)
    }

    pub fn commit_prepared_transaction(
        &mut self,
        mut prepared: PreparedDistributedServerTransaction<M::EvaluationMachine>,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        match prepared.machine_turn.take() {
            Some((turn, index)) => {
                prepared.update.turns[index] = self
                    .machine
                    .commit_prepared_evaluation(turn, prepared.evaluation_machine)?;
            }
            None => match prepared.protocol_checkpoint_turn.take() {
                Some(turn) => {
                    self.machine
                        .commit_prepared_evaluation(turn, prepared.evaluation_machine)?;
                }
                None => {
                    self.machine
                        .install_evaluation(prepared.evaluation_machine)?;
                }
            },
        }
        *self.runtime = prepared.candidate_runtime;
        self.restore_effect_scope();
        Ok(prepared.update)
    }

    pub fn commit_prepared_transaction_with_protocol_state(
        &mut self,
        mut prepared: PreparedDistributedServerTransaction<M::EvaluationMachine>,
        changes: impl FnOnce(
            u64,
        ) -> Result<
            Vec<boon_persistence::DurableProtocolStateChange>,
            DistributedRuntimeError,
        >,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let (turn, update_index) = match prepared.machine_turn.take() {
            Some((turn, index)) => (turn, Some(index)),
            None => (
                prepared.protocol_checkpoint_turn.take().ok_or_else(|| {
                    runtime_error(
                        "protocol-state transaction was not prepared against a protocol checkpoint",
                    )
                })?,
                None,
            ),
        };
        let protocol_state_changes = match changes(turn.sequence) {
            Ok(changes) => changes,
            Err(error) => {
                let _ = self.machine.rollback_prepared_turn();
                self.restore_effect_scope();
                return Err(error);
            }
        };
        let turn = match self.machine.commit_prepared_evaluation_with_protocol_state(
            turn,
            prepared.evaluation_machine,
            protocol_state_changes,
        ) {
            Ok(turn) => turn,
            Err(error) => {
                let _ = self.machine.rollback_prepared_turn();
                self.restore_effect_scope();
                return Err(error);
            }
        };
        if let Some(index) = update_index {
            prepared.update.turns[index] = turn;
        }
        *self.runtime = prepared.candidate_runtime;
        self.restore_effect_scope();
        Ok(prepared.update)
    }

    pub fn rollback_prepared_transaction(
        &mut self,
        prepared: PreparedDistributedServerTransaction<M::EvaluationMachine>,
    ) -> Result<(), DistributedRuntimeError> {
        if prepared.machine_turn.is_some() || prepared.protocol_checkpoint_turn.is_some() {
            self.machine.rollback_prepared_turn()?;
        }
        self.restore_effect_scope();
        Ok(())
    }

    /// Dispatches one Server-owned host or timer source under the Global
    /// context. Session-imported event sources are not host-dispatchable.
    pub fn dispatch_global(
        &mut self,
        source_path: &str,
        payload: SourcePayload,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let mut update = DistributedServerUpdate::default();
        self.enter_global(&mut update)?;
        let event = self.machine.event_for_path(source_path, payload)?;
        self.reject_imported_event_source(event.source)?;
        let prepared_turn = self.machine.prepare_dispatch(event)?;
        let turn = match self.machine.commit_prepared_turn(prepared_turn) {
            Ok(turn) => turn,
            Err(error) => {
                let _ = self.machine.rollback_prepared_turn();
                return Err(error);
            }
        };
        self.record_transient_effects(EffectOwner::Global, std::slice::from_ref(&turn))?;
        update.turns.push(turn);
        self.collect_after_global_turn(&mut update)?;
        Ok(update)
    }

    /// Installs the complete Global context and validates that a host source is
    /// not owned by a Session transport edge. The caller may then execute the
    /// source through its normal ephemeral or persistent admission policy.
    pub fn prepare_global_source(
        &mut self,
        source_path: &str,
        payload: &SourcePayload,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let mut update = DistributedServerUpdate::default();
        self.enter_global(&mut update)?;
        let event = self.machine.event_for_path(source_path, payload.clone())?;
        self.reject_imported_event_source(event.source)?;
        Ok(update)
    }

    pub fn prepare_global_read(
        &mut self,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let mut update = DistributedServerUpdate::default();
        self.enter_global(&mut update)?;
        Ok(update)
    }

    /// Publishes a Global source turn that was already admitted by the owning
    /// Server machine. This keeps host-specific durability outside routing
    /// while retaining one authority and one distributed publication stream.
    pub fn publish_global_turn(
        &mut self,
        turn: &RuntimeTurn,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        if self.state.machine_context != ServerContextKey::Global {
            return Err(runtime_error(
                "global Server turn was published outside the Global context",
            ));
        }
        let mut update = DistributedServerUpdate::default();
        self.record_transient_effects(EffectOwner::Global, std::slice::from_ref(turn))?;
        update.turns.push(turn.clone());
        self.collect_after_global_turn(&mut update)?;
        Ok(update)
    }

    pub fn prepare_transient_effect(
        &mut self,
        call_id: TransientEffectCallId,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let owner = self
            .state
            .effect_owners
            .get(&call_id)
            .copied()
            .ok_or(DistributedRuntimeError::InvalidLease)?;
        let mut update = DistributedServerUpdate::default();
        self.enter_effect_owner(owner, &mut update)?;
        Ok(update)
    }

    pub fn publish_transient_effect_turn(
        &mut self,
        call_id: TransientEffectCallId,
        turn: &RuntimeTurn,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let owner = self
            .state
            .effect_owners
            .get(&call_id)
            .copied()
            .ok_or(DistributedRuntimeError::InvalidLease)?;
        self.record_transient_effects(owner, std::slice::from_ref(turn))?;
        let mut update = DistributedServerUpdate::default();
        update.turns.push(turn.clone());
        self.finish_effect_owner(owner, &mut update)?;
        if !self.machine.has_pending_transient_effect(call_id) {
            self.state.effect_owners.remove(&call_id);
        }
        Ok(update)
    }

    pub fn publish_transient_effect_cancellation(
        &mut self,
        call_id: TransientEffectCallId,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let owner = self
            .state
            .effect_owners
            .get(&call_id)
            .copied()
            .ok_or(DistributedRuntimeError::InvalidLease)?;
        if self.machine.has_pending_transient_effect(call_id) {
            return Err(runtime_error(
                "transient effect cancellation was published while the call remained active",
            ));
        }
        self.state.effect_owners.remove(&call_id);
        let mut update = DistributedServerUpdate::default();
        self.finish_effect_owner(owner, &mut update)?;
        Ok(update)
    }

    pub fn root_value_current(
        &mut self,
        origin: SessionOrigin,
        name: &str,
    ) -> Result<Value, DistributedRuntimeError> {
        let mut ignored = DistributedServerUpdate::default();
        self.enter_origin(origin, &mut ignored)?;
        self.machine.root_value_current(name)
    }

    pub fn root_value_current_global(
        &mut self,
        name: &str,
    ) -> Result<Value, DistributedRuntimeError> {
        let mut ignored = DistributedServerUpdate::default();
        self.enter_global(&mut ignored)?;
        self.machine.root_value_current(name)
    }

    pub fn complete_transient_effect(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let owner = self
            .state
            .effect_owners
            .get(&call_id)
            .copied()
            .ok_or(DistributedRuntimeError::InvalidLease)?;
        let mut update = DistributedServerUpdate::default();
        self.enter_effect_owner(owner, &mut update)?;
        let prepared_turn = self
            .machine
            .prepare_transient_effect_completion(call_id, outcome)?;
        let turn = match self.machine.commit_prepared_turn(prepared_turn) {
            Ok(turn) => turn,
            Err(error) => {
                let _ = self.machine.rollback_prepared_turn();
                return Err(error);
            }
        };
        self.record_transient_effects(owner, std::slice::from_ref(&turn))?;
        update.turns.push(turn);
        self.finish_effect_owner(owner, &mut update)?;
        if !self.machine.has_pending_transient_effect(call_id) {
            self.state.effect_owners.remove(&call_id);
        }
        Ok(update)
    }

    pub fn deliver_transient_effect_result(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let owner = self
            .state
            .effect_owners
            .get(&call_id)
            .copied()
            .ok_or(DistributedRuntimeError::InvalidLease)?;
        let mut update = DistributedServerUpdate::default();
        self.enter_effect_owner(owner, &mut update)?;
        let prepared_turn =
            self.machine
                .prepare_transient_effect_result(call_id, result_sequence, outcome)?;
        let turn = match self.machine.commit_prepared_turn(prepared_turn) {
            Ok(turn) => turn,
            Err(error) => {
                let _ = self.machine.rollback_prepared_turn();
                return Err(error);
            }
        };
        self.record_transient_effects(owner, std::slice::from_ref(&turn))?;
        update.turns.push(turn);
        self.finish_effect_owner(owner, &mut update)?;
        if !self.machine.has_pending_transient_effect(call_id) {
            self.state.effect_owners.remove(&call_id);
        }
        Ok(update)
    }

    pub fn cancel_origin_transient_effects(
        &mut self,
        origin: SessionOrigin,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        self.require_origin(origin)?;
        let mut update = DistributedServerUpdate::default();
        for round in 0..=MAX_EFFECT_CANCELLATION_ROUNDS {
            if self.pending_transient_effect_count(origin) == 0 {
                return Ok(update);
            }
            if round == MAX_EFFECT_CANCELLATION_ROUNDS {
                return Err(runtime_error(
                    "Server Session-origin effect cancellation did not reach a fixed point",
                ));
            }
            let cancellation = self.cancel_origin_transient_effect_round(origin)?;
            update.turns.extend(cancellation.turns);
            update.deliveries.extend(cancellation.deliveries);
        }
        unreachable!("bounded effect cancellation loop always returns")
    }

    fn cancel_origin_transient_effect_round(
        &mut self,
        origin: SessionOrigin,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let call_ids = self
            .state
            .effect_owners
            .iter()
            .filter_map(|(call_id, owner)| {
                (*owner == EffectOwner::Origin(origin)).then_some(*call_id)
            })
            .collect::<Vec<_>>();
        if call_ids.is_empty() {
            return Ok(DistributedServerUpdate::default());
        }
        let mut update = DistributedServerUpdate::default();
        self.enter_origin(origin, &mut update)?;
        if let Some(prepared_turn) = self
            .machine
            .prepare_transient_effect_cancellation(&call_ids)?
        {
            let turn = match self.machine.commit_prepared_turn(prepared_turn) {
                Ok(turn) => turn,
                Err(error) => {
                    let _ = self.machine.rollback_prepared_turn();
                    return Err(error);
                }
            };
            self.record_transient_effects(
                EffectOwner::Origin(origin),
                std::slice::from_ref(&turn),
            )?;
            update.turns.push(turn);
            if self.origin_inputs_are_current(origin) {
                update
                    .deliveries
                    .extend(self.collect_origin_outputs(origin)?);
            }
        }
        for call_id in call_ids {
            self.state.effect_owners.remove(&call_id);
        }
        self.enter_global(&mut update)?;
        update
            .deliveries
            .extend(self.collect_shared_outputs(Some(origin))?);
        Ok(update)
    }

    pub fn pending_transient_effect_count(&self, origin: SessionOrigin) -> usize {
        self.state
            .effect_owners
            .values()
            .filter(|owner| **owner == EffectOwner::Origin(origin))
            .count()
    }

    pub fn has_origin(&self, origin: SessionOrigin) -> bool {
        self.state.origins.contains_key(&origin)
    }

    pub fn next_origin_transient_effect(
        &self,
        origin: SessionOrigin,
    ) -> Option<TransientEffectCallId> {
        self.state
            .effect_owners
            .iter()
            .find_map(|(call_id, owner)| {
                (*owner == EffectOwner::Origin(origin)).then_some(*call_id)
            })
    }

    fn accept_current(
        &mut self,
        origin: SessionOrigin,
        export_id: ExportId,
        revision: u64,
        value: DataValue,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let import_id = self
            .contract
            .value_imports
            .iter()
            .find(|import| {
                import.producer_role == ProgramRole::Session
                    && import.source_export_id == export_id
                    && import.scope == DistributedRouteScopePlan::OriginScoped
            })
            .map(|import| import.import_id)
            .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
        let state = self
            .state
            .origins
            .get_mut(&origin)
            .expect("validated origin");
        accept_greater(state.imports.get(&import_id).map(|entry| entry.0), revision)?;
        state.imports.insert(import_id, (revision, value));
        if self.origin_inputs_are_current(origin) {
            self.settle_origin(origin)
        } else {
            Ok(DistributedServerUpdate::default())
        }
    }

    fn prepare_event(
        &mut self,
        origin: SessionOrigin,
        export_id: ExportId,
        sequence: u64,
        value: DataValue,
    ) -> Result<(DistributedServerUpdate, Option<RuntimeTurn>), DistributedRuntimeError> {
        let import = self
            .contract
            .event_imports
            .iter()
            .find(|import| {
                import.producer_role == ProgramRole::Session && import.source_export_id == export_id
            })
            .cloned()
            .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
        expect_next(
            &self
                .state
                .origins
                .get(&origin)
                .expect("validated origin")
                .accepted_events,
            export_id,
            sequence,
        )?;
        let mut update = DistributedServerUpdate::default();
        self.enter_origin(origin, &mut update)?;
        let mut payload = SourcePayload::default();
        let value = Value::from_data(&value);
        match &import.payload_field {
            Some(field) => set_source_payload_value(&mut payload, field, value)?,
            None if value == Value::Null => {}
            None => return Err(DistributedRuntimeError::InvalidTransportFrame),
        }
        let event = self
            .machine
            .event_for_source(import.local_source_id, payload)?;
        let turn = self.machine.prepare_dispatch(event)?;
        let result = (|| {
            self.record_transient_effects(
                EffectOwner::Origin(origin),
                std::slice::from_ref(&turn),
            )?;
            self.state
                .origins
                .get_mut(&origin)
                .expect("validated origin")
                .accepted_events
                .insert(export_id, sequence);
            if self.origin_inputs_are_current(origin) {
                update
                    .deliveries
                    .extend(self.collect_origin_outputs(origin)?);
            }
            update
                .deliveries
                .extend(self.collect_shared_outputs(Some(origin))?);
            Ok::<(), DistributedRuntimeError>(())
        })();
        match result {
            Ok(()) => Ok((update, Some(turn))),
            Err(error) => {
                if let Err(rollback) = self.machine.rollback_prepared_turn() {
                    return Err(runtime_error(format!(
                        "distributed Server event preparation failed: {error}; rollback failed: {rollback}"
                    )));
                }
                Err(error)
            }
        }
    }

    fn accept_call_request(
        &mut self,
        origin: SessionOrigin,
        call_site_id: RemoteCallSiteId,
        function_export_id: ExportId,
        revision: u64,
        arguments: BTreeMap<DistributedArgumentId, DataValue>,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let edge = self
            .wire_schema
            .call_edges
            .iter()
            .find(|edge| {
                edge.call_site_id == call_site_id
                    && edge.caller_role == ProgramRole::Session
                    && edge.callee_role == ProgramRole::Server
                    && edge.function_export_id == function_export_id
            })
            .cloned()
            .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
        let accepted = &self
            .state
            .origins
            .get(&origin)
            .expect("validated origin")
            .accepted_calls;
        accept_greater(accepted.get(&call_site_id).copied(), revision)?;
        let mut update = DistributedServerUpdate::default();
        self.enter_origin(origin, &mut update)?;
        let value = self
            .machine
            .evaluate_function(edge.function_export_id, import_data_arguments(arguments))?;
        let value = export_runtime_value(value)?;
        self.state
            .origins
            .get_mut(&origin)
            .expect("validated origin")
            .accepted_calls
            .insert(call_site_id, revision);
        update.deliveries.push(ServerDelivery {
            target: ServerDeliveryTarget::Origin(origin),
            message: DistributedMessage {
                producer: ProgramRole::Server,
                consumer: ProgramRole::Session,
                payload: DistributedMessagePayload::CallResult {
                    call_site_id,
                    revision,
                    value,
                },
            },
        });
        self.enter_global(&mut update)?;
        Ok(update)
    }

    fn accept_call_result(
        &mut self,
        origin: SessionOrigin,
        call_site_id: RemoteCallSiteId,
        revision: u64,
        value: DataValue,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let call = self
            .contract
            .remote_call_sites
            .iter()
            .find(|call| {
                call.call_site_id == call_site_id && call.callee_role == ProgramRole::Session
            })
            .cloned()
            .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
        let latest = self
            .state
            .origins
            .get(&origin)
            .and_then(|state| state.sent_calls.get(&call_site_id))
            .map(|entry| entry.0)
            .ok_or(DistributedRuntimeError::TransportSequenceMismatch)?;
        if revision < latest {
            return Ok(DistributedServerUpdate::default());
        }
        if revision > latest {
            return Err(DistributedRuntimeError::TransportSequenceMismatch);
        }
        let state = self
            .state
            .origins
            .get_mut(&origin)
            .expect("validated origin");
        accept_greater(
            state
                .imports
                .get(&call.result_import_id)
                .map(|entry| entry.0),
            revision,
        )?;
        state
            .imports
            .insert(call.result_import_id, (revision, value));
        self.settle_origin(origin)
    }

    fn enter_effect_owner(
        &mut self,
        owner: EffectOwner,
        update: &mut DistributedServerUpdate,
    ) -> Result<(), DistributedRuntimeError> {
        match owner {
            EffectOwner::Global => self.enter_global(update),
            EffectOwner::Origin(origin) => self.enter_origin(origin, update),
        }
    }

    fn finish_effect_owner(
        &mut self,
        owner: EffectOwner,
        update: &mut DistributedServerUpdate,
    ) -> Result<(), DistributedRuntimeError> {
        match owner {
            EffectOwner::Global => self.collect_after_global_turn(update),
            EffectOwner::Origin(origin) => self.finish_origin_turn(origin, update),
        }
    }

    fn enter_origin(
        &mut self,
        origin: SessionOrigin,
        update: &mut DistributedServerUpdate,
    ) -> Result<(), DistributedRuntimeError> {
        let state = self
            .state
            .origins
            .get(&origin)
            .ok_or(DistributedRuntimeError::InvalidLease)?;
        let context = SessionContext::Available {
            status: state.status.clone(),
            principal: state.principal.clone(),
        };
        let imports = state
            .imports
            .iter()
            .map(|(import_id, (revision, value))| {
                DistributedImportUpdate::new(*import_id, *revision, Value::from_data(value))
            })
            .collect();
        let scope = state.execution_scope;
        self.machine.set_transient_effect_scope(scope);
        match self.machine.replace_distributed_context(context, imports) {
            Ok(Some(turn)) => Self::record_context_turn(update, turn)?,
            Ok(None) => {}
            Err(error) => {
                self.restore_effect_scope();
                return Err(error);
            }
        }
        self.state.machine_context = ServerContextKey::Origin(origin);
        Ok(())
    }

    fn enter_global(
        &mut self,
        update: &mut DistributedServerUpdate,
    ) -> Result<(), DistributedRuntimeError> {
        self.machine.set_transient_effect_scope(GLOBAL_EFFECT_SCOPE);
        match self
            .machine
            .replace_distributed_context(SessionContext::Unavailable, Vec::new())
        {
            Ok(Some(turn)) => Self::record_context_turn(update, turn)?,
            Ok(None) => {}
            Err(error) => {
                self.restore_effect_scope();
                return Err(error);
            }
        }
        self.state.machine_context = ServerContextKey::Global;
        Ok(())
    }

    fn record_context_turn(
        update: &mut DistributedServerUpdate,
        turn: RuntimeTurn,
    ) -> Result<(), DistributedRuntimeError> {
        if turn.source_sequence.is_some()
            || !turn.authority_deltas.is_empty()
            || !turn.durable_changes.is_empty()
            || !turn.outbox_changes.is_empty()
            || !turn.transient_effects.is_empty()
            || !turn.cancelled_transient_effects.is_empty()
            || !turn.transient_effect_credit_grants.is_empty()
            || !turn.document_patches.is_empty()
        {
            return Err(runtime_error(
                "distributed Server context replacement attempted authoritative or effect work",
            ));
        }
        update.turns.push(turn);
        Ok(())
    }

    fn restore_effect_scope(&mut self) {
        let scope = match self.state.machine_context {
            ServerContextKey::Global => GLOBAL_EFFECT_SCOPE,
            ServerContextKey::Origin(origin) => self
                .state
                .origins
                .get(&origin)
                .map(|state| state.execution_scope)
                .unwrap_or(GLOBAL_EFFECT_SCOPE),
        };
        self.machine.set_transient_effect_scope(scope);
    }

    fn finish_origin_turn(
        &mut self,
        origin: SessionOrigin,
        update: &mut DistributedServerUpdate,
    ) -> Result<(), DistributedRuntimeError> {
        if self.origin_inputs_are_current(origin) {
            update
                .deliveries
                .extend(self.collect_origin_outputs(origin)?);
        }
        self.enter_global(update)?;
        update
            .deliveries
            .extend(self.collect_shared_outputs(Some(origin))?);
        Ok(())
    }

    fn collect_after_global_turn(
        &mut self,
        update: &mut DistributedServerUpdate,
    ) -> Result<(), DistributedRuntimeError> {
        update.deliveries.extend(self.collect_shared_outputs(None)?);
        let origins = self.state.origins.keys().copied().collect::<Vec<_>>();
        for origin in origins {
            if !self.origin_inputs_are_current(origin) {
                continue;
            }
            self.enter_origin(origin, update)?;
            update
                .deliveries
                .extend(self.collect_origin_outputs(origin)?);
        }
        self.enter_global(update)
    }

    fn origin_inputs_are_current(&self, origin: SessionOrigin) -> bool {
        let Some(state) = self.state.origins.get(&origin) else {
            return false;
        };
        self.contract
            .value_imports
            .iter()
            .filter(|import| import.scope == DistributedRouteScopePlan::OriginScoped)
            .all(|import| state.imports.contains_key(&import.import_id))
    }

    fn collect_origin_outputs(
        &mut self,
        origin: SessionOrigin,
    ) -> Result<Vec<ServerDelivery>, DistributedRuntimeError> {
        let mut deliveries = Vec::new();
        let edges = self
            .wire_schema
            .value_edges
            .iter()
            .filter(|edge| {
                edge.producer_role == ProgramRole::Server
                    && edge.scope == DistributedRouteScopePlan::OriginScoped
            })
            .cloned()
            .collect::<Vec<_>>();
        for edge in edges {
            let value = export_runtime_value(self.machine.export_current(edge.export_id)?)?;
            let state = self.state.origins.get_mut(&origin).expect("active origin");
            if state
                .sent_values
                .get(&edge.export_id)
                .is_some_and(|(_, current)| current == &value)
            {
                continue;
            }
            let revision =
                next_revision(state.sent_values.get(&edge.export_id).map(|entry| entry.0))?;
            state
                .sent_values
                .insert(edge.export_id, (revision, value.clone()));
            deliveries.push(ServerDelivery {
                target: ServerDeliveryTarget::Origin(origin),
                message: DistributedMessage {
                    producer: ProgramRole::Server,
                    consumer: edge.consumer_role,
                    payload: DistributedMessagePayload::Current {
                        export_id: edge.export_id,
                        revision,
                        value,
                    },
                },
            });
        }

        let calls = self.contract.remote_call_sites.clone();
        for call in calls {
            if call.callee_role != ProgramRole::Session {
                return Err(DistributedRuntimeError::UnknownTransportEdge);
            }
            let arguments = export_runtime_arguments(self.machine.call_arguments(&call)?)?;
            let state = self.state.origins.get_mut(&origin).expect("active origin");
            if state
                .sent_calls
                .get(&call.call_site_id)
                .is_some_and(|(_, current)| current == &arguments)
            {
                continue;
            }
            let revision = next_revision(
                state
                    .sent_calls
                    .get(&call.call_site_id)
                    .map(|entry| entry.0),
            )?;
            state
                .sent_calls
                .insert(call.call_site_id, (revision, arguments.clone()));
            deliveries.push(ServerDelivery {
                target: ServerDeliveryTarget::Origin(origin),
                message: DistributedMessage {
                    producer: ProgramRole::Server,
                    consumer: ProgramRole::Session,
                    payload: DistributedMessagePayload::CallRequest {
                        call_site_id: call.call_site_id,
                        function_export_id: call.function_export_id,
                        revision,
                        arguments,
                    },
                },
            });
        }
        Ok(deliveries)
    }

    fn collect_shared_outputs(
        &mut self,
        target_origin: Option<SessionOrigin>,
    ) -> Result<Vec<ServerDelivery>, DistributedRuntimeError> {
        let mut deliveries = Vec::new();
        let edges = self
            .wire_schema
            .value_edges
            .iter()
            .filter(|edge| {
                edge.producer_role == ProgramRole::Server
                    && edge.scope == DistributedRouteScopePlan::SharedSubscription
            })
            .cloned()
            .collect::<Vec<_>>();
        for edge in edges {
            let value = export_runtime_value(self.machine.export_current(edge.export_id)?)?;
            let changed = !self
                .state
                .shared_sent_values
                .get(&edge.export_id)
                .is_some_and(|(_, current)| current == &value);
            let revision = if changed {
                let revision = next_revision(
                    self.state
                        .shared_sent_values
                        .get(&edge.export_id)
                        .map(|entry| entry.0),
                )?;
                self.state
                    .shared_sent_values
                    .insert(edge.export_id, (revision, value.clone()));
                for state in self.state.origins.values_mut() {
                    state.shared_sent_revisions.insert(edge.export_id, revision);
                }
                revision
            } else {
                self.state
                    .shared_sent_values
                    .get(&edge.export_id)
                    .expect("unchanged shared value is initialized")
                    .0
            };
            let target = if changed {
                ServerDeliveryTarget::AllSessions
            } else {
                let Some(origin) = target_origin else {
                    continue;
                };
                let state = self
                    .state
                    .origins
                    .get_mut(&origin)
                    .ok_or(DistributedRuntimeError::InvalidLease)?;
                if state.shared_sent_revisions.get(&edge.export_id) == Some(&revision) {
                    continue;
                }
                state.shared_sent_revisions.insert(edge.export_id, revision);
                ServerDeliveryTarget::Origin(origin)
            };
            deliveries.push(ServerDelivery {
                target,
                message: DistributedMessage {
                    producer: ProgramRole::Server,
                    consumer: edge.consumer_role,
                    payload: DistributedMessagePayload::Current {
                        export_id: edge.export_id,
                        revision,
                        value,
                    },
                },
            });
        }
        Ok(deliveries)
    }

    fn record_transient_effects(
        &mut self,
        owner: EffectOwner,
        turns: &[RuntimeTurn],
    ) -> Result<(), DistributedRuntimeError> {
        for turn in turns {
            for invocation in &turn.transient_effects {
                if self
                    .state
                    .effect_owners
                    .insert(invocation.call_id, owner)
                    .is_some()
                {
                    return Err(runtime_error(
                        "Server transient effect call changed ownership",
                    ));
                }
            }
            for call_id in &turn.cancelled_transient_effects {
                self.state.effect_owners.remove(call_id);
            }
        }
        Ok(())
    }

    fn reject_imported_event_source(
        &self,
        source: SourceId,
    ) -> Result<(), DistributedRuntimeError> {
        if self
            .contract
            .event_imports
            .iter()
            .any(|import| import.local_source_id == source)
        {
            return Err(runtime_error(
                "Session-imported Server event sources cannot be host-dispatched",
            ));
        }
        Ok(())
    }

    fn require_origin(&self, origin: SessionOrigin) -> Result<(), DistributedRuntimeError> {
        if self.state.origins.contains_key(&origin) {
            Ok(())
        } else {
            Err(DistributedRuntimeError::InvalidLease)
        }
    }
}

fn next_revision(current: Option<u64>) -> Result<u64, DistributedRuntimeError> {
    current
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| runtime_error("distributed Server revision exhausted"))
}

fn accept_greater(current: Option<u64>, revision: u64) -> Result<(), DistributedRuntimeError> {
    if revision == 0 || current.is_some_and(|current| revision <= current) {
        return Err(DistributedRuntimeError::TransportSequenceMismatch);
    }
    Ok(())
}

fn expect_next<K: Ord + Copy>(
    sequences: &BTreeMap<K, u64>,
    key: K,
    sequence: u64,
) -> Result<(), DistributedRuntimeError> {
    let expected = sequences
        .get(&key)
        .copied()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| runtime_error("distributed Server sequence exhausted"))?;
    if sequence != expected {
        return Err(DistributedRuntimeError::TransportSequenceMismatch);
    }
    Ok(())
}
