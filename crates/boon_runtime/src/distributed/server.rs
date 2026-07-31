use super::DistributedRuntimeError;
use crate::{
    DistributedCurrentCallInstance, DistributedImportUpdate, RuntimeTurn, SessionContext,
    SourceEvent, SourcePayload, TransientEffectCallId, Value,
};
use boon_plan::{
    DistributedArgumentId, DistributedCallInstanceId, ExportId, RemoteCallSiteId, SourceId,
};
use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};

/// The narrow authority surface required by distributed Server routing.
///
/// Implementations may be ephemeral or persistent, but every mutating source
/// and effect turn must pass through the implementation's normal admission
/// boundary. Context replacement installs transient remote inputs and is not a
/// second durable authority.
pub trait DistributedServerMachine {
    type EvaluationMachine: DistributedServerMachine;

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

    fn export_if_current(
        &mut self,
        export_id: ExportId,
    ) -> Result<Option<Value>, DistributedRuntimeError>;

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
