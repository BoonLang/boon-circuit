use super::message::{DistributedMessage, DistributedMessagePayload};
use super::{
    DistributedRuntimeError, export_runtime_arguments, export_runtime_value, import_data_arguments,
    runtime_error, set_source_payload_value,
};
use crate::program::ProgramArtifact;
use crate::{
    DistributedCurrentCallInstance, DistributedImportUpdate, RuntimeTurn, SessionConnectionStatus,
    SessionContext, SessionPrincipal, SourceEvent, SourcePayload, TransientEffectCallId, Value,
};
use boon_data::Value as DataValue;
use boon_plan::{
    DistributedArgumentId, DistributedCallInstanceId, DistributedCallMode,
    DistributedEndpointContractPlan, DistributedRouteScopePlan, DistributedWireSchemaPlan,
    ExportId, ImportId, ProgramRole, RemoteCallSiteId, SourceId,
};
use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::ops::{Deref, DerefMut};

const GLOBAL_EFFECT_SCOPE: u64 = u64::MAX;
const MAX_EFFECT_CANCELLATION_ROUNDS: usize = 1024;
const INVOCATION_REPLAY_WINDOW: u64 = 256;
const CURRENT_CALL_TOMBSTONE_WINDOW: u64 = 256;
const MAX_CURRENT_CALL_INSTANCES_PER_ORIGIN: usize = 4096;
const MAX_INVOCATION_INSTANCES_PER_ORIGIN: usize = 4096;

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

    fn event_for_route(
        &self,
        route: boon_plan::SourceRouteToken,
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

    /// Returns the complete live demand set for this call site in the current
    /// machine origin. This includes root demands and demands retained inside
    /// every active producer lease. Nested demand IDs must incorporate their
    /// outer producer call instance; the router treats them as opaque.
    fn current_call_instances(
        &mut self,
        call_site_id: RemoteCallSiteId,
    ) -> Result<Vec<DistributedCurrentCallInstance>, DistributedRuntimeError>;

    /// Reads the current result of an already-active producer lease without
    /// replaying or advancing its demand revision. Nested call identities in
    /// the lease must remain scoped by the outer `call_instance_id`. The read
    /// is part of output collection and therefore must preserve any prepared
    /// turn and any producer lease currently being evaluated.
    fn producer_call_result_current(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
    ) -> Result<Value, DistributedRuntimeError>;

    fn evaluate_function_instance(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        export_id: ExportId,
        demand_revision: u64,
        arguments: BTreeMap<DistributedArgumentId, Value>,
    ) -> Result<(Value, Option<RuntimeTurn>), DistributedRuntimeError>;

    fn update_current_call_result_instance(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        content_revision: u64,
        value: Value,
    ) -> Result<Option<RuntimeTurn>, DistributedRuntimeError>;

    fn drop_producer_call_instance(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
    ) -> Result<Option<RuntimeTurn>, DistributedRuntimeError>;

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

    fn rollback_prepared_turn(&mut self) -> Result<(), DistributedRuntimeError>;

    fn has_pending_transient_effect(&self, call_id: TransientEffectCallId) -> bool;

    fn set_transient_effect_scope(&mut self, scope: u64);

    fn set_machine_origin(&mut self, origin: SessionOrigin) -> Result<(), DistributedRuntimeError>;

    fn reset_machine_origin(&mut self) -> Result<(), DistributedRuntimeError>;

    fn drop_producer_origin(
        &mut self,
        origin: SessionOrigin,
    ) -> Result<Vec<TransientEffectCallId>, DistributedRuntimeError>;

    fn root_value_current(&mut self, name: &str) -> Result<Value, DistributedRuntimeError>;
}

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SessionOrigin {
    slot: u32,
    generation: u64,
}

impl Debug for SessionOrigin {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionOrigin(..)")
    }
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

#[derive(Clone, Copy, Eq, PartialEq)]
enum ServerContextKey {
    Global,
    Origin(SessionOrigin),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum EffectOwner {
    Global,
    Origin(SessionOrigin),
}

#[derive(Clone)]
struct OriginState {
    status: SessionConnectionStatus,
    principal: SessionPrincipal,
    execution_scope: u64,
    imports: BTreeMap<ImportId, (u64, DataValue)>,
    accepted_events: BTreeMap<ExportId, u64>,
    accepted_current_call_revisions: BTreeMap<RemoteCallSiteId, u64>,
    accepted_current_calls: BTreeMap<CallInstanceKey, AcceptedCurrentCall>,
    accepted_current_call_tombstones: BTreeMap<CallInstanceKey, u64>,
    accepted_invocation_requests: BTreeMap<CallInstanceKey, u64>,
    accepted_invocation_results: BTreeMap<CallInstanceKey, u64>,
    invocation_request_replays: BTreeMap<InvocationKey, InvocationRequestReplay>,
    invocation_result_replays: BTreeMap<InvocationKey, DataValue>,
    sent_values: BTreeMap<ExportId, (u64, DataValue)>,
    shared_sent_revisions: BTreeMap<ExportId, u64>,
    sent_current_call_revisions: BTreeMap<RemoteCallSiteId, u64>,
    sent_current_calls: BTreeMap<CallInstanceKey, SentCurrentCall>,
    sent_current_call_tombstones: BTreeMap<CallInstanceKey, u64>,
    accepted_result_content_revisions: BTreeMap<RemoteCallSiteId, u64>,
    sent_invocation_sequences: BTreeMap<CallInstanceKey, u64>,
    pending_invocation_results: BTreeMap<InvocationKey, boon_plan::SourceRouteToken>,
}

type CallInstanceKey = (RemoteCallSiteId, DistributedCallInstanceId);
type InvocationKey = (RemoteCallSiteId, DistributedCallInstanceId, u64);

#[derive(Clone)]
struct AcceptedCurrentCall {
    demand_revision: u64,
    result_revision: u64,
    value: DataValue,
}

#[derive(Clone)]
struct SentCurrentCall {
    demand_revision: u64,
    accepted_result_revision: u64,
    accepted_result: Option<DataValue>,
    arguments: BTreeMap<DistributedArgumentId, DataValue>,
}

#[derive(Clone)]
struct InvocationRequestReplay {
    function_export_id: ExportId,
    arguments: BTreeMap<DistributedArgumentId, DataValue>,
    result: DataValue,
}

#[derive(Clone)]
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
    next_transaction_id: u64,
    open_transaction_id: Option<u64>,
}

#[must_use = "prepared Server transactions must be committed or rolled back"]
pub struct PreparedDistributedServerTransaction<E> {
    transaction_id: u64,
    update: DistributedServerUpdate,
    candidate_runtime: DistributedServerRuntime,
    evaluation_machine: E,
    rollback_machine: Option<E>,
    machine_turn: Option<(RuntimeTurn, usize)>,
}

#[derive(Clone, Copy)]
enum PreparedMessageCompletion {
    None,
    FinishOrigin(SessionOrigin),
}

struct PreparedSessionMessage {
    update: DistributedServerUpdate,
    machine_turn: Option<RuntimeTurn>,
    completion: PreparedMessageCompletion,
}

impl PreparedSessionMessage {
    fn complete(update: DistributedServerUpdate) -> Self {
        Self {
            update,
            machine_turn: None,
            completion: PreparedMessageCompletion::None,
        }
    }

    fn finish_origin(
        origin: SessionOrigin,
        update: DistributedServerUpdate,
        machine_turn: Option<RuntimeTurn>,
    ) -> Self {
        Self {
            update,
            machine_turn,
            completion: PreparedMessageCompletion::FinishOrigin(origin),
        }
    }
}

impl<E> PreparedDistributedServerTransaction<E> {
    pub fn deliveries(&self) -> &[ServerDelivery] {
        &self.update.deliveries
    }

    pub fn prepares_machine_turn(&self) -> bool {
        self.machine_turn.is_some()
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
            next_transaction_id: 1,
            open_transaction_id: None,
        })
    }

    fn ensure_no_open_transaction(&self) -> Result<(), DistributedRuntimeError> {
        if self.open_transaction_id.is_some() {
            return Err(runtime_error(
                "distributed Server already has an open prepared transaction",
            ));
        }
        Ok(())
    }

    fn begin_transaction(&mut self) -> Result<u64, DistributedRuntimeError> {
        self.ensure_no_open_transaction()?;
        let transaction_id = self.next_transaction_id;
        self.next_transaction_id = self
            .next_transaction_id
            .checked_add(1)
            .ok_or_else(|| runtime_error("distributed Server transaction ID exhausted"))?;
        self.open_transaction_id = Some(transaction_id);
        Ok(transaction_id)
    }

    fn require_transaction(&self, transaction_id: u64) -> Result<(), DistributedRuntimeError> {
        if self.open_transaction_id != Some(transaction_id) {
            return Err(DistributedRuntimeError::InvalidLease);
        }
        Ok(())
    }

    fn clear_transaction(&mut self, transaction_id: u64) -> Result<(), DistributedRuntimeError> {
        self.require_transaction(transaction_id)?;
        self.open_transaction_id = None;
        Ok(())
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

impl<M> DistributedServerAuthority<'_, M>
where
    M: DistributedServerMachine,
{
    pub fn attach_origin(
        &mut self,
        origin: SessionOrigin,
        principal: SessionPrincipal,
        execution_scope: u64,
    ) -> Result<(), DistributedRuntimeError> {
        self.ensure_no_open_transaction()?;
        if execution_scope == 0
            || execution_scope == GLOBAL_EFFECT_SCOPE
            || self.state.origins.contains_key(&origin)
            || self
                .state
                .origins
                .keys()
                .any(|attached| attached.slot() == origin.slot())
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
                accepted_current_call_revisions: BTreeMap::new(),
                accepted_current_calls: BTreeMap::new(),
                accepted_current_call_tombstones: BTreeMap::new(),
                accepted_invocation_requests: BTreeMap::new(),
                accepted_invocation_results: BTreeMap::new(),
                invocation_request_replays: BTreeMap::new(),
                invocation_result_replays: BTreeMap::new(),
                sent_values: BTreeMap::new(),
                shared_sent_revisions: BTreeMap::new(),
                sent_current_call_revisions: BTreeMap::new(),
                sent_current_calls: BTreeMap::new(),
                sent_current_call_tombstones: BTreeMap::new(),
                accepted_result_content_revisions: BTreeMap::new(),
                sent_invocation_sequences: BTreeMap::new(),
                pending_invocation_results: BTreeMap::new(),
            },
        );
        Ok(())
    }

    pub fn set_origin_status(
        &mut self,
        origin: SessionOrigin,
        status: SessionConnectionStatus,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        self.ensure_no_open_transaction()?;
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
        self.ensure_no_open_transaction()?;
        self.require_origin(origin)?;
        self.prepare_evaluated_transaction(
            DistributedServerUpdate::default(),
            None,
            None,
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
        self.ensure_no_open_transaction()?;
        self.require_origin(origin)?;
        let mut update = DistributedServerUpdate::default();
        self.enter_origin(origin, &mut update)?;
        if self.origin_inputs_are_current(origin) {
            update
                .deliveries
                .extend(self.collect_producer_call_results(origin)?);
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
        self.ensure_no_open_transaction()?;
        self.require_origin(origin)?;
        let mut update = self.cancel_origin_transient_effects(origin)?;
        let prepared = self.prepare_origin_expiration_transaction(origin)?;
        let expired = self.commit_prepared_transaction(prepared)?;
        update.turns.extend(expired.turns);
        update.deliveries.extend(expired.deliveries);
        Ok(update)
    }

    pub fn prepare_origin_expiration_transaction(
        &mut self,
        origin: SessionOrigin,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        self.ensure_no_open_transaction()?;
        self.require_origin(origin)?;
        if self.pending_transient_effect_count(origin) != 0 {
            return Err(runtime_error(
                "Server Session origin cannot expire while it still owns transient effects",
            ));
        }
        self.prepare_evaluated_transaction(
            DistributedServerUpdate::default(),
            None,
            None,
            None,
            move |authority, update, _| {
                let untracked_effects = authority.machine.drop_producer_origin(origin)?;
                if !untracked_effects.is_empty() {
                    return Err(runtime_error(format!(
                        "Server Session origin retained {} untracked producer effect(s) during expiration",
                        untracked_effects.len()
                    )));
                }
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
        self.commit_prepared_transaction(prepared)
    }

    pub fn prepare_session_message(
        &mut self,
        origin: SessionOrigin,
        message: DistributedMessage,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        self.ensure_no_open_transaction()?;
        if message.producer != ProgramRole::Session || message.consumer != ProgramRole::Server {
            return Err(DistributedRuntimeError::UnknownTransportEdge);
        }
        self.require_origin(origin)?;
        let rollback_machine = self.machine.fork_prepared_evaluation(None)?;
        let original_state = self.runtime.state.clone();
        let result = match message.payload {
            DistributedMessagePayload::Current {
                export_id,
                revision,
                value,
            } => self
                .accept_current(origin, export_id, revision, value)
                .map(PreparedSessionMessage::complete),
            DistributedMessagePayload::Event {
                export_id,
                sequence,
                value,
            } => self.prepare_event(origin, export_id, sequence, value),
            DistributedMessagePayload::CurrentCallRequest {
                call_site_id,
                call_instance_id,
                function_export_id,
                demand_revision,
                arguments,
            } => self.accept_call_request(
                origin,
                call_site_id,
                call_instance_id,
                function_export_id,
                demand_revision,
                arguments,
            ),
            DistributedMessagePayload::CurrentCallResult {
                call_site_id,
                call_instance_id,
                demand_revision,
                result_revision,
                value,
            } => self.accept_call_result(
                origin,
                call_site_id,
                call_instance_id,
                demand_revision,
                result_revision,
                value,
            ),
            DistributedMessagePayload::CurrentCallDetach {
                call_site_id,
                call_instance_id,
                demand_revision,
            } => self.accept_call_detach(origin, call_site_id, call_instance_id, demand_revision),
            DistributedMessagePayload::InvocationRequest {
                call_site_id,
                call_instance_id,
                function_export_id,
                sequence,
                arguments,
            } => self.accept_invocation_request(
                origin,
                call_site_id,
                call_instance_id,
                function_export_id,
                sequence,
                arguments,
            ),
            DistributedMessagePayload::InvocationResult {
                call_site_id,
                call_instance_id,
                sequence,
                value,
            } => self.accept_invocation_result(
                origin,
                call_site_id,
                call_instance_id,
                sequence,
                value,
            ),
        };
        let prepared = match result {
            Ok(prepared) => {
                let rollback_machine = prepared.machine_turn.is_none().then_some(rollback_machine);
                self.prepare_evaluated_transaction(
                    prepared.update,
                    prepared.machine_turn,
                    rollback_machine,
                    Some(original_state),
                    move |authority, update, turn| match prepared.completion {
                        PreparedMessageCompletion::None => Ok(()),
                        PreparedMessageCompletion::FinishOrigin(origin) => {
                            authority.finish_turn(EffectOwner::Origin(origin), turn, update)
                        }
                    },
                )
            }
            Err(error) => {
                let rollback = self.machine.install_evaluation(rollback_machine);
                self.runtime.state = original_state;
                self.restore_effect_scope();
                match rollback {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(runtime_error(format!(
                        "distributed Server message preparation failed: {error}; rollback failed: {rollback}"
                    ))),
                }
            }
        };
        self.restore_effect_scope();
        prepared
    }

    pub fn prepare_global_source_transaction(
        &mut self,
        source_path: &str,
        payload: SourcePayload,
        durable: bool,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        self.ensure_no_open_transaction()?;
        let mut update = DistributedServerUpdate::default();
        self.enter_global(&mut update)?;
        let event = self.machine.event_for_path(source_path, payload)?;
        self.reject_imported_event_source(event.source)?;
        let turn = self
            .machine
            .prepare_dispatch_with_durability(event, durable)?;
        self.prepare_evaluated_transaction(
            update,
            Some(turn),
            None,
            None,
            |authority, update, turn| authority.finish_turn(EffectOwner::Global, turn, update),
        )
    }

    pub fn prepare_global_read_transaction(
        &mut self,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        self.ensure_no_open_transaction()?;
        let mut update = DistributedServerUpdate::default();
        self.enter_global(&mut update)?;
        self.prepare_evaluated_transaction(update, None, None, None, |_, _, _| Ok(()))
    }

    fn prepare_transient_effect_transaction(
        &mut self,
        call_id: TransientEffectCallId,
        prepare: impl FnOnce(&mut M) -> Result<RuntimeTurn, DistributedRuntimeError>,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        self.ensure_no_open_transaction()?;
        let owner = self
            .state
            .effect_owners
            .get(&call_id)
            .copied()
            .ok_or(DistributedRuntimeError::InvalidLease)?;
        let mut update = DistributedServerUpdate::default();
        self.enter_effect_owner(owner, &mut update)?;
        let turn = prepare(self.machine)?;
        self.prepare_evaluated_transaction(
            update,
            Some(turn),
            None,
            None,
            move |authority, update, turn| {
                authority.finish_turn(owner, turn, update)?;
                if !authority.machine.has_pending_transient_effect(call_id) {
                    authority.state.effect_owners.remove(&call_id);
                }
                Ok(())
            },
        )
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
        self.prepare_transient_effect_cancellations_transaction(&[call_id], durable)
    }

    fn prepare_transient_effect_cancellations_transaction(
        &mut self,
        call_ids: &[TransientEffectCallId],
        durable: bool,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        self.ensure_no_open_transaction()?;
        let Some(first) = call_ids.first() else {
            return Err(runtime_error(
                "transient effect cancellation requires at least one call",
            ));
        };
        let owner = self
            .state
            .effect_owners
            .get(first)
            .copied()
            .ok_or(DistributedRuntimeError::InvalidLease)?;
        if call_ids
            .iter()
            .any(|call_id| self.state.effect_owners.get(call_id).copied() != Some(owner))
        {
            return Err(DistributedRuntimeError::InvalidLease);
        }
        let mut update = DistributedServerUpdate::default();
        self.enter_effect_owner(owner, &mut update)?;
        let turn = self
            .machine
            .prepare_transient_effect_cancellation_with_durability(call_ids, durable)?
            .ok_or_else(|| {
                runtime_error(
                    "distributed Server effect ownership diverged from machine effect state",
                )
            })?;
        let call_ids = call_ids.to_vec();
        self.prepare_evaluated_transaction(
            update,
            Some(turn),
            None,
            None,
            move |authority, update, turn| {
                authority.finish_turn(owner, turn, update)?;
                for call_id in call_ids {
                    authority.state.effect_owners.remove(&call_id);
                }
                Ok(())
            },
        )
    }

    fn prepare_evaluated_transaction(
        &mut self,
        mut update: DistributedServerUpdate,
        machine_turn: Option<RuntimeTurn>,
        rollback_machine: Option<M::EvaluationMachine>,
        rollback_state: Option<ServerRouterState>,
        evaluate: impl FnOnce(
            &mut DistributedServerAuthority<'_, M::EvaluationMachine>,
            &mut DistributedServerUpdate,
            Option<&RuntimeTurn>,
        ) -> Result<(), DistributedRuntimeError>,
    ) -> Result<PreparedDistributedServerTransaction<M::EvaluationMachine>, DistributedRuntimeError>
    {
        let mut candidate_runtime = match rollback_state {
            Some(rollback_state) => {
                let candidate_state = std::mem::replace(&mut self.runtime.state, rollback_state);
                self.runtime.state.machine_context = candidate_state.machine_context;
                DistributedServerRuntime {
                    contract: self.runtime.contract.clone(),
                    wire_schema: self.runtime.wire_schema.clone(),
                    state: candidate_state,
                    next_transaction_id: self.runtime.next_transaction_id,
                    open_transaction_id: None,
                }
            }
            None => self.runtime.clone(),
        };
        let transaction_id = match self.runtime.begin_transaction() {
            Ok(transaction_id) => transaction_id,
            Err(error) => {
                let rollback = if machine_turn.is_some() {
                    self.machine.rollback_prepared_turn()
                } else if let Some(rollback_machine) = rollback_machine {
                    self.machine.install_evaluation(rollback_machine)
                } else {
                    Ok(())
                };
                self.restore_effect_scope();
                return match rollback {
                    Ok(()) => Err(error),
                    Err(rollback) => Err(runtime_error(format!(
                        "distributed Server transaction could not begin: {error}; rollback failed: {rollback}"
                    ))),
                };
            }
        };
        candidate_runtime.next_transaction_id = self.runtime.next_transaction_id;
        candidate_runtime.open_transaction_id = None;
        let machine_turn = machine_turn.map(|turn| {
            let index = update.turns.len();
            update.turns.push(turn.clone());
            (turn, index)
        });
        let prepared_turn = machine_turn.as_ref().map(|(turn, _)| turn);
        let evaluation_machine = match self.machine.fork_prepared_evaluation(prepared_turn) {
            Ok(machine) => machine,
            Err(error) => {
                return self.fail_evaluated_transaction(
                    transaction_id,
                    prepared_turn.is_some(),
                    rollback_machine,
                    error,
                );
            }
        };
        let mut evaluation_machine = evaluation_machine;
        let result = {
            let mut authority = candidate_runtime.bind(&mut evaluation_machine);
            evaluate(&mut authority, &mut update, prepared_turn)
        };
        if let Err(error) = result {
            return self.fail_evaluated_transaction(
                transaction_id,
                prepared_turn.is_some(),
                rollback_machine,
                error,
            );
        }
        Ok(PreparedDistributedServerTransaction {
            transaction_id,
            update,
            candidate_runtime,
            evaluation_machine,
            rollback_machine,
            machine_turn,
        })
    }

    fn fail_evaluated_transaction<T>(
        &mut self,
        transaction_id: u64,
        has_machine_turn: bool,
        rollback_machine: Option<M::EvaluationMachine>,
        error: DistributedRuntimeError,
    ) -> Result<T, DistributedRuntimeError> {
        self.runtime.clear_transaction(transaction_id)?;
        let rollback = if has_machine_turn {
            self.machine.rollback_prepared_turn()
        } else if let Some(rollback_machine) = rollback_machine {
            self.machine.install_evaluation(rollback_machine)
        } else {
            Ok(())
        };
        if let Err(rollback) = rollback {
            return Err(runtime_error(format!(
                "distributed Server evaluation failed: {error}; rollback failed: {rollback}"
            )));
        }
        self.restore_effect_scope();
        Err(error)
    }

    fn guard_function_evaluation<T>(
        &mut self,
        operation: &str,
        turn: Option<&RuntimeTurn>,
        finish: impl FnOnce(
            &mut DistributedServerAuthority<'_, M>,
            Option<&RuntimeTurn>,
        ) -> Result<T, DistributedRuntimeError>,
    ) -> Result<T, DistributedRuntimeError> {
        match finish(self, turn) {
            Ok(value) => Ok(value),
            Err(error) => {
                if turn.is_some()
                    && let Err(rollback) = self.machine.rollback_prepared_turn()
                {
                    return Err(runtime_error(format!(
                        "distributed Server {operation} preparation failed: {error}; rollback failed: {rollback}"
                    )));
                }
                Err(error)
            }
        }
    }

    fn fail_prepared_commit<T>(
        &mut self,
        transaction_id: u64,
        error: DistributedRuntimeError,
    ) -> Result<T, DistributedRuntimeError> {
        self.runtime.clear_transaction(transaction_id)?;
        self.restore_effect_scope();
        Err(error)
    }

    pub fn commit_prepared_transaction(
        &mut self,
        mut prepared: PreparedDistributedServerTransaction<M::EvaluationMachine>,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        self.runtime.require_transaction(prepared.transaction_id)?;
        match prepared.machine_turn.take() {
            Some((turn, index)) => {
                match self
                    .machine
                    .commit_prepared_evaluation(turn, prepared.evaluation_machine)
                {
                    Ok(turn) => prepared.update.turns[index] = turn,
                    Err(error) => {
                        return self.fail_prepared_commit(prepared.transaction_id, error);
                    }
                }
            }
            None => {
                if let Err(error) = self.machine.install_evaluation(prepared.evaluation_machine) {
                    let rollback = match prepared.rollback_machine {
                        Some(rollback_machine) => self.machine.install_evaluation(rollback_machine),
                        None => Ok(()),
                    };
                    self.runtime.clear_transaction(prepared.transaction_id)?;
                    self.restore_effect_scope();
                    return match rollback {
                        Ok(()) => Err(error),
                        Err(rollback) => Err(runtime_error(format!(
                            "distributed Server transaction commit failed: {error}; rollback failed: {rollback}"
                        ))),
                    };
                }
            }
        }
        *self.runtime = prepared.candidate_runtime;
        self.restore_effect_scope();
        Ok(prepared.update)
    }

    pub fn rollback_prepared_transaction(
        &mut self,
        prepared: PreparedDistributedServerTransaction<M::EvaluationMachine>,
    ) -> Result<(), DistributedRuntimeError> {
        self.runtime.require_transaction(prepared.transaction_id)?;
        if prepared.machine_turn.is_some() {
            self.machine.rollback_prepared_turn()?;
        } else if let Some(rollback_machine) = prepared.rollback_machine {
            self.machine.install_evaluation(rollback_machine)?;
        }
        self.runtime.clear_transaction(prepared.transaction_id)?;
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
        let prepared = self.prepare_global_source_transaction(source_path, payload, false)?;
        self.commit_prepared_transaction(prepared)
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
        let prepared =
            self.prepare_transient_effect_completion_transaction(call_id, outcome, false)?;
        self.commit_prepared_transaction(prepared)
    }

    pub fn deliver_transient_effect_result(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
    ) -> Result<DistributedServerUpdate, DistributedRuntimeError> {
        let prepared = self.prepare_transient_effect_result_transaction(
            call_id,
            result_sequence,
            outcome,
            false,
        )?;
        self.commit_prepared_transaction(prepared)
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
        let prepared = self.prepare_transient_effect_cancellations_transaction(&call_ids, false)?;
        self.commit_prepared_transaction(prepared)
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
    ) -> Result<PreparedSessionMessage, DistributedRuntimeError> {
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
        self.state
            .origins
            .get_mut(&origin)
            .expect("validated origin")
            .accepted_events
            .insert(export_id, sequence);
        Ok(PreparedSessionMessage::finish_origin(
            origin,
            update,
            Some(turn),
        ))
    }

    fn accept_call_request(
        &mut self,
        origin: SessionOrigin,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        function_export_id: ExportId,
        demand_revision: u64,
        arguments: BTreeMap<DistributedArgumentId, DataValue>,
    ) -> Result<PreparedSessionMessage, DistributedRuntimeError> {
        let edge = self
            .wire_schema
            .call_edges
            .iter()
            .find(|edge| {
                edge.call_site_id == call_site_id
                    && edge.caller_role == ProgramRole::Session
                    && edge.callee_role == ProgramRole::Server
                    && edge.function_export_id == function_export_id
                    && edge.mode == DistributedCallMode::Current
            })
            .cloned()
            .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
        let key = (call_site_id, call_instance_id);
        let state = self.state.origins.get(&origin).expect("validated origin");
        accept_greater(
            state
                .accepted_current_call_revisions
                .get(&call_site_id)
                .copied(),
            demand_revision,
        )?;
        if !state.accepted_current_calls.contains_key(&key)
            && state.accepted_current_calls.len() >= MAX_CURRENT_CALL_INSTANCES_PER_ORIGIN
        {
            return Err(runtime_error(
                "distributed Server active current-call instance limit was exceeded",
            ));
        }
        let mut update = DistributedServerUpdate::default();
        self.enter_origin(origin, &mut update)?;
        let (value, turn) = self.machine.evaluate_function_instance(
            call_site_id,
            call_instance_id,
            edge.function_export_id,
            demand_revision,
            import_data_arguments(arguments),
        )?;
        let update =
            self.guard_function_evaluation("Current call", turn.as_ref(), move |authority, _| {
                let value = export_runtime_value(value)?;
                let result_revision = 1;
                let state = authority
                    .state
                    .origins
                    .get_mut(&origin)
                    .expect("validated origin");
                state
                    .accepted_current_call_revisions
                    .insert(call_site_id, demand_revision);
                state.accepted_current_calls.insert(
                    key,
                    AcceptedCurrentCall {
                        demand_revision,
                        result_revision,
                        value: value.clone(),
                    },
                );
                state.accepted_current_call_tombstones.remove(&key);
                prune_current_call_tombstones(
                    &mut state.accepted_current_call_tombstones,
                    call_site_id,
                    demand_revision,
                );
                update.deliveries.push(ServerDelivery {
                    target: ServerDeliveryTarget::Origin(origin),
                    message: DistributedMessage {
                        producer: ProgramRole::Server,
                        consumer: ProgramRole::Session,
                        payload: DistributedMessagePayload::CurrentCallResult {
                            call_site_id,
                            call_instance_id,
                            demand_revision,
                            result_revision,
                            value,
                        },
                    },
                });
                Ok(update)
            })?;
        Ok(PreparedSessionMessage::finish_origin(origin, update, turn))
    }

    fn accept_call_detach(
        &mut self,
        origin: SessionOrigin,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        demand_revision: u64,
    ) -> Result<PreparedSessionMessage, DistributedRuntimeError> {
        self.wire_schema
            .call_edges
            .iter()
            .find(|edge| {
                edge.call_site_id == call_site_id
                    && edge.caller_role == ProgramRole::Session
                    && edge.callee_role == ProgramRole::Server
                    && edge.mode == DistributedCallMode::Current
            })
            .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
        let key = (call_site_id, call_instance_id);
        let state = self.state.origins.get(&origin).expect("validated origin");
        accept_greater(
            state
                .accepted_current_call_revisions
                .get(&call_site_id)
                .copied(),
            demand_revision,
        )?;
        if !state.accepted_current_calls.contains_key(&key) {
            return Err(DistributedRuntimeError::InvalidLease);
        }

        let mut update = DistributedServerUpdate::default();
        self.enter_origin(origin, &mut update)?;
        let turn = self
            .machine
            .drop_producer_call_instance(call_site_id, call_instance_id)?;
        let update = self.guard_function_evaluation(
            "Current call detach",
            turn.as_ref(),
            move |authority, _| {
                let state = authority
                    .state
                    .origins
                    .get_mut(&origin)
                    .expect("validated origin");
                state
                    .accepted_current_call_revisions
                    .insert(call_site_id, demand_revision);
                state.accepted_current_calls.remove(&key);
                state
                    .accepted_current_call_tombstones
                    .insert(key, demand_revision);
                prune_current_call_tombstones(
                    &mut state.accepted_current_call_tombstones,
                    call_site_id,
                    demand_revision,
                );
                Ok(update)
            },
        )?;
        Ok(PreparedSessionMessage::finish_origin(origin, update, turn))
    }

    fn accept_invocation_request(
        &mut self,
        origin: SessionOrigin,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        function_export_id: ExportId,
        sequence: u64,
        arguments: BTreeMap<DistributedArgumentId, DataValue>,
    ) -> Result<PreparedSessionMessage, DistributedRuntimeError> {
        let edge = self
            .wire_schema
            .call_edges
            .iter()
            .find(|edge| {
                edge.call_site_id == call_site_id
                    && edge.caller_role == ProgramRole::Session
                    && edge.callee_role == ProgramRole::Server
                    && edge.function_export_id == function_export_id
                    && edge.mode == DistributedCallMode::Invocation
            })
            .cloned()
            .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
        let key = (call_site_id, call_instance_id);
        let origin_state = self.state.origins.get(&origin).expect("validated origin");
        if !origin_state.accepted_invocation_requests.contains_key(&key)
            && origin_state.accepted_invocation_requests.len()
                >= MAX_INVOCATION_INSTANCES_PER_ORIGIN
        {
            return Err(runtime_error(
                "distributed Server invocation instance limit was exceeded",
            ));
        }
        let accepted = self
            .state
            .origins
            .get(&origin)
            .expect("validated origin")
            .accepted_invocation_requests
            .get(&key)
            .copied()
            .unwrap_or(0);
        if sequence <= accepted {
            let replay = self
                .state
                .origins
                .get(&origin)
                .and_then(|state| {
                    state.invocation_request_replays.get(&(
                        call_site_id,
                        call_instance_id,
                        sequence,
                    ))
                })
                .ok_or(DistributedRuntimeError::TransportSequenceMismatch)?;
            if replay.function_export_id != function_export_id || replay.arguments != arguments {
                return Err(DistributedRuntimeError::InvalidTransportFrame);
            }
            return Ok(PreparedSessionMessage::complete(DistributedServerUpdate {
                turns: Vec::new(),
                deliveries: vec![ServerDelivery {
                    target: ServerDeliveryTarget::Origin(origin),
                    message: DistributedMessage {
                        producer: ProgramRole::Server,
                        consumer: ProgramRole::Session,
                        payload: DistributedMessagePayload::InvocationResult {
                            call_site_id,
                            call_instance_id,
                            sequence,
                            value: replay.result.clone(),
                        },
                    },
                }],
            }));
        }
        expect_next(
            &self
                .state
                .origins
                .get(&origin)
                .expect("validated origin")
                .accepted_invocation_requests,
            key,
            sequence,
        )?;
        let mut update = DistributedServerUpdate::default();
        self.enter_origin(origin, &mut update)?;
        let (value, turn) = self.machine.evaluate_function_instance(
            call_site_id,
            call_instance_id,
            edge.function_export_id,
            sequence,
            import_data_arguments(arguments.clone()),
        )?;
        let update =
            self.guard_function_evaluation("invocation", turn.as_ref(), move |authority, _| {
                let value = export_runtime_value(value)?;
                let state = authority
                    .state
                    .origins
                    .get_mut(&origin)
                    .expect("validated origin");
                state.accepted_invocation_requests.insert(key, sequence);
                state.invocation_request_replays.insert(
                    (call_site_id, call_instance_id, sequence),
                    InvocationRequestReplay {
                        function_export_id,
                        arguments,
                        result: value.clone(),
                    },
                );
                prune_invocation_replays(&mut state.invocation_request_replays, key, sequence);
                update.deliveries.push(ServerDelivery {
                    target: ServerDeliveryTarget::Origin(origin),
                    message: DistributedMessage {
                        producer: ProgramRole::Server,
                        consumer: ProgramRole::Session,
                        payload: DistributedMessagePayload::InvocationResult {
                            call_site_id,
                            call_instance_id,
                            sequence,
                            value,
                        },
                    },
                });
                Ok(update)
            })?;
        Ok(PreparedSessionMessage::finish_origin(origin, update, turn))
    }

    fn accept_call_result(
        &mut self,
        origin: SessionOrigin,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        demand_revision: u64,
        result_revision: u64,
        value: DataValue,
    ) -> Result<PreparedSessionMessage, DistributedRuntimeError> {
        self.contract
            .remote_call_sites
            .iter()
            .any(|call| {
                call.call_site_id == call_site_id
                    && call.caller_role == ProgramRole::Server
                    && call.callee_role == ProgramRole::Session
                    && call.mode == DistributedCallMode::Current
            })
            .then_some(())
            .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
        if result_revision == 0 {
            return Err(DistributedRuntimeError::TransportSequenceMismatch);
        }
        let key = (call_site_id, call_instance_id);
        let state = self.state.origins.get(&origin).expect("validated origin");
        let Some(sent) = state.sent_current_calls.get(&key) else {
            return match state.sent_current_call_tombstones.get(&key).copied() {
                Some(detach_revision) if demand_revision <= detach_revision => Ok(
                    PreparedSessionMessage::complete(DistributedServerUpdate::default()),
                ),
                _ => Err(DistributedRuntimeError::InvalidLease),
            };
        };
        if demand_revision < sent.demand_revision {
            return Ok(PreparedSessionMessage::complete(
                DistributedServerUpdate::default(),
            ));
        }
        if demand_revision > sent.demand_revision {
            return Err(DistributedRuntimeError::TransportSequenceMismatch);
        }
        if result_revision < sent.accepted_result_revision {
            return Ok(PreparedSessionMessage::complete(
                DistributedServerUpdate::default(),
            ));
        }
        if result_revision == sent.accepted_result_revision {
            return if sent.accepted_result.as_ref() == Some(&value) {
                Ok(PreparedSessionMessage::complete(
                    DistributedServerUpdate::default(),
                ))
            } else {
                Err(DistributedRuntimeError::InvalidTransportFrame)
            };
        }
        let content_revision = next_revision(
            state
                .accepted_result_content_revisions
                .get(&call_site_id)
                .copied(),
        )?;

        let mut update = DistributedServerUpdate::default();
        self.enter_origin(origin, &mut update)?;
        let turn = self.machine.update_current_call_result_instance(
            call_site_id,
            call_instance_id,
            content_revision,
            Value::from_data(&value),
        )?;
        let update = self.guard_function_evaluation(
            "Current call result",
            turn.as_ref(),
            move |authority, _| {
                let state = authority
                    .state
                    .origins
                    .get_mut(&origin)
                    .expect("validated origin");
                state
                    .accepted_result_content_revisions
                    .insert(call_site_id, content_revision);
                let sent = state
                    .sent_current_calls
                    .get_mut(&key)
                    .expect("active call was validated");
                sent.accepted_result_revision = result_revision;
                sent.accepted_result = Some(value);
                Ok(update)
            },
        )?;
        Ok(PreparedSessionMessage::finish_origin(origin, update, turn))
    }

    fn accept_invocation_result(
        &mut self,
        origin: SessionOrigin,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        sequence: u64,
        value: DataValue,
    ) -> Result<PreparedSessionMessage, DistributedRuntimeError> {
        let call = self
            .contract
            .remote_call_sites
            .iter()
            .find(|call| {
                call.call_site_id == call_site_id
                    && call.caller_role == ProgramRole::Server
                    && call.callee_role == ProgramRole::Session
                    && call.mode == DistributedCallMode::Invocation
            })
            .cloned()
            .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
        let key = (call_site_id, call_instance_id);
        let invocation_key = (call_site_id, call_instance_id, sequence);
        let state = self.state.origins.get(&origin).expect("validated origin");
        let latest = state
            .sent_invocation_sequences
            .get(&key)
            .copied()
            .ok_or(DistributedRuntimeError::TransportSequenceMismatch)?;
        if sequence > latest {
            return Err(DistributedRuntimeError::TransportSequenceMismatch);
        }
        let accepted = state
            .accepted_invocation_results
            .get(&key)
            .copied()
            .unwrap_or(0);
        if sequence <= accepted {
            let replay = state
                .invocation_result_replays
                .get(&invocation_key)
                .ok_or(DistributedRuntimeError::TransportSequenceMismatch)?;
            if replay != &value {
                return Err(DistributedRuntimeError::InvalidTransportFrame);
            }
            return Ok(PreparedSessionMessage::complete(
                DistributedServerUpdate::default(),
            ));
        }
        expect_next(&state.accepted_invocation_results, key, sequence)?;
        let result_route = state
            .pending_invocation_results
            .get(&invocation_key)
            .cloned()
            .ok_or(DistributedRuntimeError::TransportSequenceMismatch)?;
        let (result_source, result_field) = call
            .result
            .invocation_source()
            .map(|(source, field)| (source, field.clone()))
            .ok_or_else(|| runtime_error("distributed invocation has no private result source"))?;
        if result_route.source != result_source {
            return Err(DistributedRuntimeError::InvalidTransportFrame);
        }

        let mut update = DistributedServerUpdate::default();
        self.enter_origin(origin, &mut update)?;
        let mut payload = SourcePayload::default();
        set_source_payload_value(&mut payload, &result_field, Value::from_data(&value))?;
        let event = self.machine.event_for_route(result_route, payload)?;
        let turn = self.machine.prepare_dispatch(event)?;
        let state = self
            .state
            .origins
            .get_mut(&origin)
            .expect("validated origin");
        state.accepted_invocation_results.insert(key, sequence);
        state.pending_invocation_results.remove(&invocation_key);
        state
            .invocation_result_replays
            .insert(invocation_key, value);
        prune_invocation_replays(&mut state.invocation_result_replays, key, sequence);
        Ok(PreparedSessionMessage::finish_origin(
            origin,
            update,
            Some(turn),
        ))
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

    fn finish_turn(
        &mut self,
        owner: EffectOwner,
        turn: Option<&RuntimeTurn>,
        update: &mut DistributedServerUpdate,
    ) -> Result<(), DistributedRuntimeError> {
        if let Some(turn) = turn {
            if owner == EffectOwner::Global && !turn.distributed_invocations.is_empty() {
                return Err(runtime_error(
                    "Global-owned Server turn emitted an origin-scoped distributed invocation",
                ));
            }
            self.record_transient_effects(owner, std::slice::from_ref(turn))?;
            if let EffectOwner::Origin(origin) = owner {
                update
                    .deliveries
                    .extend(self.collect_turn_invocation_deliveries(origin, turn)?);
            }
        }
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
        self.ensure_no_open_transaction()?;
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
        let previous_context = self.state.machine_context;
        self.machine.set_machine_origin(origin)?;
        self.machine.set_transient_effect_scope(scope);
        let replaced = self
            .machine
            .replace_distributed_context(context, imports)
            .and_then(|turn| {
                if let Some(turn) = turn {
                    Self::record_context_turn(update, turn)?;
                }
                Ok(())
            });
        if let Err(error) = replaced {
            if let Err(restore) = self.restore_machine_origin(previous_context) {
                self.restore_effect_scope();
                return Err(runtime_error(format!(
                    "distributed Server failed to enter Session origin: {error}; origin restore failed: {restore}"
                )));
            }
            self.restore_effect_scope();
            return Err(error);
        }
        self.state.machine_context = ServerContextKey::Origin(origin);
        Ok(())
    }

    fn enter_global(
        &mut self,
        update: &mut DistributedServerUpdate,
    ) -> Result<(), DistributedRuntimeError> {
        self.ensure_no_open_transaction()?;
        let previous_context = self.state.machine_context;
        self.machine.reset_machine_origin()?;
        self.machine.set_transient_effect_scope(GLOBAL_EFFECT_SCOPE);
        let replaced = self
            .machine
            .replace_distributed_context(SessionContext::Unavailable, Vec::new())
            .and_then(|turn| {
                if let Some(turn) = turn {
                    Self::record_context_turn(update, turn)?;
                }
                Ok(())
            });
        if let Err(error) = replaced {
            if let Err(restore) = self.restore_machine_origin(previous_context) {
                self.restore_effect_scope();
                return Err(runtime_error(format!(
                    "distributed Server failed to enter Global context: {error}; origin restore failed: {restore}"
                )));
            }
            self.restore_effect_scope();
            return Err(error);
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
            || !turn.distributed_invocations.is_empty()
            || !turn.document_patches.is_empty()
        {
            return Err(runtime_error(
                "distributed Server context replacement attempted authoritative or effect work",
            ));
        }
        update.turns.push(turn);
        Ok(())
    }

    fn restore_machine_origin(
        &mut self,
        context: ServerContextKey,
    ) -> Result<(), DistributedRuntimeError> {
        match context {
            ServerContextKey::Global => self.machine.reset_machine_origin(),
            ServerContextKey::Origin(origin) => self.machine.set_machine_origin(origin),
        }
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
                .extend(self.collect_producer_call_results(origin)?);
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
                .extend(self.collect_producer_call_results(origin)?);
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

    fn collect_producer_call_results(
        &mut self,
        origin: SessionOrigin,
    ) -> Result<Vec<ServerDelivery>, DistributedRuntimeError> {
        let active = self
            .state
            .origins
            .get(&origin)
            .ok_or(DistributedRuntimeError::InvalidLease)?
            .accepted_current_calls
            .iter()
            .map(|(key, call)| (*key, call.clone()))
            .collect::<Vec<_>>();
        let mut deliveries = Vec::new();
        for ((call_site_id, call_instance_id), current) in active {
            let value = export_runtime_value(
                self.machine
                    .producer_call_result_current(call_site_id, call_instance_id)?,
            )?;
            if value == current.value {
                continue;
            }
            let result_revision = next_revision(Some(current.result_revision))?;
            let state = self.state.origins.get_mut(&origin).expect("active origin");
            let Some(call) = state
                .accepted_current_calls
                .get_mut(&(call_site_id, call_instance_id))
            else {
                return Err(DistributedRuntimeError::InvalidLease);
            };
            if call.demand_revision != current.demand_revision
                || call.result_revision != current.result_revision
            {
                return Err(runtime_error(
                    "distributed Server producer call changed during result collection",
                ));
            }
            call.result_revision = result_revision;
            call.value = value.clone();
            deliveries.push(ServerDelivery {
                target: ServerDeliveryTarget::Origin(origin),
                message: DistributedMessage {
                    producer: ProgramRole::Server,
                    consumer: ProgramRole::Session,
                    payload: DistributedMessagePayload::CurrentCallResult {
                        call_site_id,
                        call_instance_id,
                        demand_revision: call.demand_revision,
                        result_revision,
                        value,
                    },
                },
            });
        }
        Ok(deliveries)
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
            if call.caller_role != ProgramRole::Server || call.callee_role != ProgramRole::Session {
                return Err(DistributedRuntimeError::UnknownTransportEdge);
            }
            if call.mode != DistributedCallMode::Current {
                continue;
            }
            let mut demanded = BTreeMap::new();
            for instance in self.machine.current_call_instances(call.call_site_id)? {
                let arguments = export_runtime_arguments(instance.arguments)?;
                if demanded
                    .insert(instance.call_instance_id, arguments)
                    .is_some()
                {
                    return Err(runtime_error(
                        "distributed Server machine returned a duplicate current-call instance",
                    ));
                }
            }
            let state = self.state.origins.get_mut(&origin).expect("active origin");
            let active_other_sites = state
                .sent_current_calls
                .keys()
                .filter(|(site, _)| *site != call.call_site_id)
                .count();
            if active_other_sites.saturating_add(demanded.len())
                > MAX_CURRENT_CALL_INSTANCES_PER_ORIGIN
            {
                return Err(runtime_error(
                    "distributed Server active current-call demand limit was exceeded",
                ));
            }
            let detached = state
                .sent_current_calls
                .keys()
                .filter_map(|(site, instance)| {
                    (*site == call.call_site_id && !demanded.contains_key(instance))
                        .then_some(*instance)
                })
                .collect::<Vec<_>>();
            for call_instance_id in detached {
                let demand_revision = next_revision(
                    state
                        .sent_current_call_revisions
                        .get(&call.call_site_id)
                        .copied(),
                )?;
                state
                    .sent_current_call_revisions
                    .insert(call.call_site_id, demand_revision);
                let key = (call.call_site_id, call_instance_id);
                state.sent_current_calls.remove(&key);
                state
                    .sent_current_call_tombstones
                    .insert(key, demand_revision);
                prune_current_call_tombstones(
                    &mut state.sent_current_call_tombstones,
                    call.call_site_id,
                    demand_revision,
                );
                deliveries.push(ServerDelivery {
                    target: ServerDeliveryTarget::Origin(origin),
                    message: DistributedMessage {
                        producer: ProgramRole::Server,
                        consumer: ProgramRole::Session,
                        payload: DistributedMessagePayload::CurrentCallDetach {
                            call_site_id: call.call_site_id,
                            call_instance_id,
                            demand_revision,
                        },
                    },
                });
            }

            for (call_instance_id, arguments) in demanded {
                let key = (call.call_site_id, call_instance_id);
                if state
                    .sent_current_calls
                    .get(&key)
                    .is_some_and(|current| current.arguments == arguments)
                {
                    continue;
                }
                let demand_revision = next_revision(
                    state
                        .sent_current_call_revisions
                        .get(&call.call_site_id)
                        .copied(),
                )?;
                state
                    .sent_current_call_revisions
                    .insert(call.call_site_id, demand_revision);
                state.sent_current_calls.insert(
                    key,
                    SentCurrentCall {
                        demand_revision,
                        accepted_result_revision: 0,
                        accepted_result: None,
                        arguments: arguments.clone(),
                    },
                );
                state.sent_current_call_tombstones.remove(&key);
                prune_current_call_tombstones(
                    &mut state.sent_current_call_tombstones,
                    call.call_site_id,
                    demand_revision,
                );
                deliveries.push(ServerDelivery {
                    target: ServerDeliveryTarget::Origin(origin),
                    message: DistributedMessage {
                        producer: ProgramRole::Server,
                        consumer: ProgramRole::Session,
                        payload: DistributedMessagePayload::CurrentCallRequest {
                            call_site_id: call.call_site_id,
                            call_instance_id,
                            function_export_id: call.function_export_id,
                            demand_revision,
                            arguments,
                        },
                    },
                });
            }
        }
        Ok(deliveries)
    }

    fn collect_turn_invocation_deliveries(
        &mut self,
        origin: SessionOrigin,
        turn: &RuntimeTurn,
    ) -> Result<Vec<ServerDelivery>, DistributedRuntimeError> {
        let mut deliveries = Vec::new();
        for invocation in &turn.distributed_invocations {
            let call = self
                .contract
                .remote_call_sites
                .iter()
                .find(|call| {
                    call.call_site_id == invocation.call_site_id
                        && call.mode == DistributedCallMode::Invocation
                })
                .cloned()
                .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
            if call.caller_role != ProgramRole::Server || call.callee_role != ProgramRole::Session {
                return Err(DistributedRuntimeError::UnknownTransportEdge);
            }
            let state = self.state.origins.get_mut(&origin).expect("active origin");
            let key = (call.call_site_id, invocation.call_instance_id);
            if !state.sent_invocation_sequences.contains_key(&key)
                && state.sent_invocation_sequences.len() >= MAX_INVOCATION_INSTANCES_PER_ORIGIN
            {
                return Err(runtime_error(
                    "distributed Server outbound invocation instance limit was exceeded",
                ));
            }
            let sequence = next_revision(state.sent_invocation_sequences.get(&key).copied())?;
            state.sent_invocation_sequences.insert(key, sequence);
            if state
                .pending_invocation_results
                .insert(
                    (call.call_site_id, invocation.call_instance_id, sequence),
                    invocation.result_route.clone(),
                )
                .is_some()
            {
                return Err(runtime_error(
                    "distributed Server invocation result route was registered twice",
                ));
            }
            deliveries.push(ServerDelivery {
                target: ServerDeliveryTarget::Origin(origin),
                message: DistributedMessage {
                    producer: ProgramRole::Server,
                    consumer: ProgramRole::Session,
                    payload: DistributedMessagePayload::InvocationRequest {
                        call_site_id: call.call_site_id,
                        call_instance_id: invocation.call_instance_id,
                        function_export_id: call.function_export_id,
                        sequence,
                        arguments: export_runtime_arguments(invocation.arguments.clone())?,
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

fn prune_invocation_replays<T>(
    replays: &mut BTreeMap<InvocationKey, T>,
    key: CallInstanceKey,
    newest_sequence: u64,
) {
    let oldest_retained = newest_sequence.saturating_sub(INVOCATION_REPLAY_WINDOW - 1);
    replays.retain(|(call_site_id, call_instance_id, sequence), _| {
        (*call_site_id, *call_instance_id) != key || *sequence >= oldest_retained
    });
}

fn prune_current_call_tombstones(
    tombstones: &mut BTreeMap<CallInstanceKey, u64>,
    call_site_id: RemoteCallSiteId,
    newest_demand_revision: u64,
) {
    let oldest_retained = newest_demand_revision.saturating_sub(CURRENT_CALL_TOMBSTONE_WINDOW - 1);
    tombstones.retain(|(candidate, _), demand_revision| {
        *candidate != call_site_id || *demand_revision >= oldest_retained
    });
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
