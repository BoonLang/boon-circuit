use boon_persistence::{
    BarrierAck, CommitAck, PersistenceControlError, PersistenceDriver, PersistenceWorkerConfig,
    PersistenceWorkerStatus, ShutdownAck,
};
use boon_program_runtime::{
    ProgramArtifact, ProgramDiagnostic, ProgramSession, ProgramSessionDispatch, ProgramSessionId,
};
use boon_runtime::{RowId, SessionOptions, SourcePayload};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// One trusted program session whose authoritative state is owned by the
/// native persistence coordinator.
///
/// Source sequencing stays host-local and is deliberately absent from the
/// durable image. Authority turns use the runtime's independent contiguous
/// turn sequence.
#[cfg(not(target_arch = "wasm32"))]
pub struct PersistentProgramSession {
    id: ProgramSessionId,
    artifact: ProgramArtifact,
    runtime: crate::PersistentRuntime,
    next_source_sequence: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl PersistentProgramSession {
    pub fn start<D>(
        artifact: ProgramArtifact,
        driver: D,
        config: PersistenceWorkerConfig,
    ) -> Result<(Self, crate::PersistentRuntimeStartup), ProgramDiagnostic>
    where
        D: PersistenceDriver + Send + 'static,
    {
        let max_runtime_work_units_per_transaction =
            artifact.max_runtime_work_units_per_transaction();
        let id = artifact.session_id();
        let (runtime, startup) = crate::PersistentRuntime::from_shared_machine_plan(
            Arc::clone(artifact.plan()),
            SessionOptions {
                program_revision: artifact.revision(),
                max_work_units_per_transaction: Some(max_runtime_work_units_per_transaction),
                ..SessionOptions::default()
            },
            driver,
            config,
        )
        .map_err(|error| ProgramDiagnostic::start(artifact.revision(), error.to_string()))?;
        Ok((
            Self {
                id,
                artifact,
                runtime,
                next_source_sequence: 1,
            },
            startup,
        ))
    }

    pub fn id(&self) -> &ProgramSessionId {
        &self.id
    }

    pub fn artifact(&self) -> &ProgramArtifact {
        &self.artifact
    }

    pub fn runtime(&self) -> &crate::PersistentRuntime {
        &self.runtime
    }

    pub fn next_source_sequence(&self) -> u64 {
        self.next_source_sequence
    }

    pub fn dispatch(
        &mut self,
        source_path: &str,
        target: Option<RowId>,
        payload: SourcePayload,
    ) -> Result<ProgramSessionDispatch, crate::PersistentDispatchError> {
        let (source_sequence, next_source_sequence, event) =
            self.prepare_dispatch(source_path, target, payload)?;
        let runtime_turn = self.runtime.dispatch(event)?;
        self.next_source_sequence = next_source_sequence;
        Ok(ProgramSessionDispatch {
            source_sequence,
            source_path: source_path.to_owned(),
            runtime_turn,
        })
    }

    pub fn dispatch_durably(
        &mut self,
        source_path: &str,
        target: Option<RowId>,
        payload: SourcePayload,
    ) -> Result<(ProgramSessionDispatch, CommitAck), crate::PersistentDispatchError> {
        let (source_sequence, next_source_sequence, event) =
            self.prepare_dispatch(source_path, target, payload)?;
        let acknowledged = self.runtime.dispatch_durably(event)?;
        self.next_source_sequence = next_source_sequence;
        Ok((
            ProgramSessionDispatch {
                source_sequence,
                source_path: source_path.to_owned(),
                runtime_turn: acknowledged.turn,
            },
            acknowledged.acknowledgement,
        ))
    }

    pub fn dispatch_prepared_durably(
        &mut self,
        event: boon_runtime::SourceEvent,
    ) -> Result<crate::DurablyAcknowledgedTurn, crate::PersistentDispatchError> {
        if event.sequence != self.next_source_sequence {
            return Err(crate::PersistentDispatchError::Runtime(format!(
                "prepared source sequence {} does not match next sequence {}",
                event.sequence, self.next_source_sequence
            )));
        }
        let next_source_sequence = self.next_source_sequence.checked_add(1).ok_or_else(|| {
            crate::PersistentDispatchError::Runtime("program source sequence overflow".to_owned())
        })?;
        let acknowledged = self.runtime.dispatch_durably(event)?;
        self.next_source_sequence = next_source_sequence;
        Ok(acknowledged)
    }

    pub fn prepare_distributed_dispatch(
        &mut self,
        event: boon_runtime::SourceEvent,
        immediate: bool,
    ) -> Result<boon_runtime::RuntimeTurn, crate::PersistentDispatchError> {
        if event.sequence != self.next_source_sequence {
            return Err(crate::PersistentDispatchError::Runtime(format!(
                "prepared source sequence {} does not match next sequence {}",
                event.sequence, self.next_source_sequence
            )));
        }
        self.next_source_sequence.checked_add(1).ok_or_else(|| {
            crate::PersistentDispatchError::Runtime("program source sequence overflow".to_owned())
        })?;
        self.runtime.prepare_distributed_dispatch(event, immediate)
    }

    pub fn prepare_distributed_effect_completion(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
        outcome: boon_runtime::Value,
        immediate: bool,
    ) -> Result<boon_runtime::RuntimeTurn, crate::PersistentDispatchError> {
        self.runtime
            .prepare_distributed_effect_completion(call_id, outcome, immediate)
    }

    pub fn prepare_distributed_effect_result(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
        result_sequence: u64,
        outcome: boon_runtime::Value,
        immediate: bool,
    ) -> Result<boon_runtime::RuntimeTurn, crate::PersistentDispatchError> {
        self.runtime
            .prepare_distributed_effect_result(call_id, result_sequence, outcome, immediate)
    }

    pub fn prepare_distributed_effect_cancellation(
        &mut self,
        call_ids: &[boon_runtime::TransientEffectCallId],
        immediate: bool,
    ) -> Result<Option<boon_runtime::RuntimeTurn>, crate::PersistentDispatchError> {
        self.runtime
            .prepare_distributed_effect_cancellation(call_ids, immediate)
    }

    pub fn prepare_distributed_function_instance(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
        export_id: boon_plan::ExportId,
        demand_revision: u64,
        arguments: BTreeMap<boon_plan::DistributedArgumentId, boon_runtime::Value>,
        immediate: bool,
    ) -> Result<
        (boon_runtime::Value, Option<boon_runtime::RuntimeTurn>),
        crate::PersistentDispatchError,
    > {
        self.runtime.prepare_distributed_function_instance(
            call_site_id,
            call_instance_id,
            export_id,
            demand_revision,
            arguments,
            immediate,
        )
    }

    pub fn prepare_distributed_call_result_instance(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
        content_revision: u64,
        value: boon_runtime::Value,
    ) -> Result<Option<boon_runtime::RuntimeTurn>, crate::PersistentDispatchError> {
        self.runtime.prepare_distributed_call_result_instance(
            call_site_id,
            call_instance_id,
            content_revision,
            value,
        )
    }

    pub fn prepare_drop_producer_call_instance(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
        immediate: bool,
    ) -> Result<Option<boon_runtime::RuntimeTurn>, crate::PersistentDispatchError> {
        self.runtime
            .prepare_drop_producer_call_instance(call_site_id, call_instance_id, immediate)
    }

    pub fn commit_prepared_distributed_turn(
        &mut self,
        turn: boon_runtime::RuntimeTurn,
    ) -> Result<crate::PersistentDistributedCommit, crate::PersistentDispatchError> {
        let source_sequence = turn.source_sequence;
        if let Some(source_sequence) = source_sequence {
            if source_sequence != self.next_source_sequence {
                return Err(self.fail_prepared_distributed_commit(
                    "prepared Server source sequence changed before commit",
                ));
            }
            if self.next_source_sequence.checked_add(1).is_none() {
                return Err(
                    self.fail_prepared_distributed_commit("program source sequence overflow")
                );
            }
        }
        let committed = self.runtime.commit_prepared_distributed_turn(turn)?;
        if source_sequence.is_some() {
            self.next_source_sequence += 1;
        }
        Ok(committed)
    }

    pub fn rollback_prepared_distributed_turn(
        &mut self,
    ) -> Result<(), crate::PersistentDispatchError> {
        self.runtime.rollback_prepared_distributed_turn()
    }

    pub fn fork_prepared_distributed_server_evaluation(
        &self,
        turn: Option<&boon_runtime::RuntimeTurn>,
    ) -> Result<ProgramSession, boon_runtime::DistributedRuntimeError> {
        let next_source_sequence = self.evaluation_next_source_sequence(turn)?;
        let runtime = self
            .runtime
            .runtime()
            .fork_distributed_server_evaluation(turn.is_some())
            .map_err(persistent_distributed_machine_error)?;
        Ok(ProgramSession::from_runtime_parts(
            self.id.clone(),
            self.artifact.clone(),
            runtime,
            next_source_sequence,
        ))
    }

    pub fn install_distributed_server_evaluation(
        &mut self,
        evaluation: ProgramSession,
    ) -> Result<(), crate::PersistentDispatchError> {
        self.validate_distributed_server_evaluation(None, &evaluation)
            .map_err(|error| crate::PersistentDispatchError::Runtime(error.to_string()))?;
        self.runtime
            .validate_distributed_server_evaluation(evaluation.runtime(), false)?;
        let next_source_sequence = evaluation.next_source_sequence();
        self.runtime
            .install_distributed_server_evaluation(evaluation.into_runtime());
        self.next_source_sequence = next_source_sequence;
        Ok(())
    }

    pub fn commit_prepared_distributed_server_evaluation(
        &mut self,
        turn: boon_runtime::RuntimeTurn,
        evaluation: ProgramSession,
    ) -> Result<crate::PersistentDistributedCommit, crate::PersistentDispatchError> {
        if let Err(error) = self.validate_distributed_server_evaluation(Some(&turn), &evaluation) {
            return Err(self.fail_prepared_distributed_commit(error));
        }
        if let Err(error) = self
            .runtime
            .validate_distributed_server_evaluation(evaluation.runtime(), true)
        {
            return Err(self.fail_prepared_distributed_commit(error));
        }
        let next_source_sequence = evaluation.next_source_sequence();
        let committed = self.runtime.commit_prepared_distributed_turn(turn)?;
        self.runtime
            .install_distributed_server_evaluation(evaluation.into_runtime());
        self.next_source_sequence = next_source_sequence;
        Ok(committed)
    }

    fn fail_prepared_distributed_commit(
        &mut self,
        error: impl std::fmt::Display,
    ) -> crate::PersistentDispatchError {
        match self.runtime.rollback_prepared_distributed_turn() {
            Ok(()) => crate::PersistentDispatchError::Runtime(error.to_string()),
            Err(rollback) => crate::PersistentDispatchError::Runtime(format!(
                "{error}; rollback failed: {rollback}"
            )),
        }
    }

    fn evaluation_next_source_sequence(
        &self,
        turn: Option<&boon_runtime::RuntimeTurn>,
    ) -> Result<u64, boon_runtime::DistributedRuntimeError> {
        let Some(source_sequence) = turn.and_then(|turn| turn.source_sequence) else {
            return Ok(self.next_source_sequence);
        };
        if source_sequence != self.next_source_sequence {
            return Err(persistent_distributed_machine_error(
                "prepared persistent Server source sequence changed before evaluation",
            ));
        }
        source_sequence
            .checked_add(1)
            .ok_or_else(|| persistent_distributed_machine_error("program source sequence overflow"))
    }

    fn validate_distributed_server_evaluation(
        &self,
        turn: Option<&boon_runtime::RuntimeTurn>,
        evaluation: &ProgramSession,
    ) -> Result<(), boon_runtime::DistributedRuntimeError> {
        if self.runtime.runtime().has_unsettled_turn() != turn.is_some() {
            return Err(persistent_distributed_machine_error(
                "persistent distributed Server authority preparation state changed before commit",
            ));
        }
        if evaluation.runtime().has_unsettled_turn() {
            return Err(persistent_distributed_machine_error(
                "distributed Server evaluation remained unsettled",
            ));
        }
        if &self.id != evaluation.id()
            || self.artifact.id() != evaluation.artifact().id()
            || self.artifact.plan_digest() != evaluation.artifact().plan_digest()
        {
            return Err(persistent_distributed_machine_error(
                "distributed Server evaluation belongs to another persistent authority",
            ));
        }
        if evaluation.next_source_sequence() != self.evaluation_next_source_sequence(turn)? {
            return Err(persistent_distributed_machine_error(
                "persistent distributed Server evaluation source sequence is invalid",
            ));
        }
        Ok(())
    }

    fn prepare_dispatch(
        &self,
        source_path: &str,
        target: Option<RowId>,
        payload: SourcePayload,
    ) -> Result<(u64, u64, boon_runtime::SourceEvent), crate::PersistentDispatchError> {
        let source_sequence = self.next_source_sequence;
        let next_source_sequence = source_sequence.checked_add(1).ok_or_else(|| {
            crate::PersistentDispatchError::Runtime("program source sequence overflow".to_owned())
        })?;
        let event = self
            .runtime
            .runtime()
            .source_event_for_path(source_sequence, source_path, target.as_slice(), payload)
            .map_err(|error| crate::PersistentDispatchError::Runtime(error.to_string()))?;
        Ok((source_sequence, next_source_sequence, event))
    }

    pub fn root_value_current(
        &mut self,
        name: &str,
    ) -> Result<boon_runtime::Value, crate::PersistentDispatchError> {
        self.runtime.root_value_current(name)
    }

    pub fn output_value_current(
        &mut self,
        name: &str,
    ) -> Result<boon_runtime::Value, crate::PersistentDispatchError> {
        self.runtime.output_value_current(name)
    }

    pub fn complete_transient_effect(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
        outcome: boon_runtime::Value,
    ) -> Result<boon_runtime::RuntimeTurn, crate::PersistentDispatchError> {
        self.runtime.complete_transient_effect(call_id, outcome)
    }

    pub fn deliver_transient_effect_result(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
        result_sequence: u64,
        outcome: boon_runtime::Value,
    ) -> Result<boon_runtime::RuntimeTurn, crate::PersistentDispatchError> {
        self.runtime
            .deliver_transient_effect_result(call_id, result_sequence, outcome)
    }

    pub fn deliver_transient_effect_result_durably(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
        result_sequence: u64,
        outcome: boon_runtime::Value,
    ) -> Result<crate::DurablyAcknowledgedTurn, crate::PersistentDispatchError> {
        self.runtime
            .deliver_transient_effect_result_durably(call_id, result_sequence, outcome)
    }

    pub fn complete_transient_effect_durably(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
        outcome: boon_runtime::Value,
    ) -> Result<crate::DurablyAcknowledgedTurn, crate::PersistentDispatchError> {
        self.runtime
            .complete_transient_effect_durably(call_id, outcome)
    }

    pub fn cancel_transient_effect(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
    ) -> Result<bool, crate::PersistentDispatchError> {
        self.runtime.cancel_transient_effect(call_id)
    }

    pub fn pending_transient_effect_count(&self) -> usize {
        self.runtime.pending_transient_effect_count()
    }

    pub fn pending_transient_effect_credits(
        &self,
        call_id: boon_runtime::TransientEffectCallId,
    ) -> Option<u32> {
        self.runtime.pending_transient_effect_credits(call_id)
    }

    pub fn persistence_status(&self) -> PersistenceWorkerStatus {
        self.runtime.status()
    }

    pub fn barrier(&self) -> Result<BarrierAck, PersistenceControlError> {
        self.runtime.barrier()
    }

    pub fn shutdown(&self) -> Result<ShutdownAck, PersistenceControlError> {
        self.runtime.shutdown()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl boon_runtime::DistributedServerMachine for PersistentProgramSession {
    type EvaluationMachine = ProgramSession;

    fn fork_prepared_evaluation(
        &self,
        turn: Option<&boon_runtime::RuntimeTurn>,
    ) -> Result<Self::EvaluationMachine, boon_runtime::DistributedRuntimeError> {
        self.fork_prepared_distributed_server_evaluation(turn)
    }

    fn install_evaluation(
        &mut self,
        evaluation: Self::EvaluationMachine,
    ) -> Result<(), boon_runtime::DistributedRuntimeError> {
        self.install_distributed_server_evaluation(evaluation)
            .map_err(persistent_distributed_machine_error)
    }

    fn commit_prepared_evaluation(
        &mut self,
        turn: boon_runtime::RuntimeTurn,
        evaluation: Self::EvaluationMachine,
    ) -> Result<boon_runtime::RuntimeTurn, boon_runtime::DistributedRuntimeError> {
        self.commit_prepared_distributed_server_evaluation(turn, evaluation)
            .map(|committed| committed.turn)
            .map_err(persistent_distributed_machine_error)
    }

    fn event_for_path(
        &self,
        path: &str,
        payload: SourcePayload,
    ) -> Result<boon_runtime::SourceEvent, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .runtime()
            .source_event_for_path(self.next_source_sequence, path, &[], payload)
            .map_err(persistent_distributed_machine_error)
    }

    fn event_for_source(
        &self,
        source: boon_plan::SourceId,
        payload: SourcePayload,
    ) -> Result<boon_runtime::SourceEvent, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .runtime()
            .source_event_by_id(self.next_source_sequence, source, payload)
            .map_err(persistent_distributed_machine_error)
    }

    fn event_for_route(
        &self,
        route: boon_plan::SourceRouteToken,
        payload: SourcePayload,
    ) -> Result<boon_runtime::SourceEvent, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .runtime()
            .source_event(self.next_source_sequence, route, payload)
            .map_err(persistent_distributed_machine_error)
    }

    fn prepare_dispatch(
        &mut self,
        event: boon_runtime::SourceEvent,
    ) -> Result<boon_runtime::RuntimeTurn, boon_runtime::DistributedRuntimeError> {
        self.prepare_distributed_dispatch(event, false)
            .map_err(persistent_distributed_machine_error)
    }

    fn export_if_current(
        &mut self,
        export_id: boon_plan::ExportId,
    ) -> Result<Option<boon_runtime::Value>, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .distributed_export_value_if_current(export_id)
            .map_err(persistent_distributed_machine_error)
    }

    fn current_call_instances(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
    ) -> Result<
        Vec<boon_runtime::DistributedCurrentCallInstance>,
        boon_runtime::DistributedRuntimeError,
    > {
        self.runtime
            .distributed_call_instances_current(call_site_id)
            .map_err(persistent_distributed_machine_error)
    }

    fn producer_call_result_current(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
    ) -> Result<boon_runtime::Value, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .distributed_producer_call_result_current(call_site_id, call_instance_id)
            .map_err(persistent_distributed_machine_error)
    }

    fn evaluate_function_instance(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
        export_id: boon_plan::ExportId,
        demand_revision: u64,
        arguments: BTreeMap<boon_plan::DistributedArgumentId, boon_runtime::Value>,
    ) -> Result<
        (boon_runtime::Value, Option<boon_runtime::RuntimeTurn>),
        boon_runtime::DistributedRuntimeError,
    > {
        self.runtime
            .prepare_distributed_function_instance(
                call_site_id,
                call_instance_id,
                export_id,
                demand_revision,
                arguments,
                true,
            )
            .map_err(persistent_distributed_machine_error)
    }

    fn update_current_call_result_instance(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
        content_revision: u64,
        value: boon_runtime::Value,
    ) -> Result<Option<boon_runtime::RuntimeTurn>, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .prepare_distributed_call_result_instance(
                call_site_id,
                call_instance_id,
                content_revision,
                value,
            )
            .map_err(persistent_distributed_machine_error)
    }

    fn drop_producer_call_instance(
        &mut self,
        call_site_id: boon_plan::RemoteCallSiteId,
        call_instance_id: boon_plan::DistributedCallInstanceId,
    ) -> Result<Option<boon_runtime::RuntimeTurn>, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .prepare_drop_producer_call_instance(call_site_id, call_instance_id, true)
            .map_err(persistent_distributed_machine_error)
    }

    fn replace_distributed_context(
        &mut self,
        session_context: boon_runtime::SessionContext,
        imports: Vec<boon_runtime::DistributedImportUpdate>,
    ) -> Result<Option<boon_runtime::RuntimeTurn>, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .replace_distributed_context(session_context, imports)
            .map_err(persistent_distributed_machine_error)
    }

    fn prepare_transient_effect_completion(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
        outcome: boon_runtime::Value,
    ) -> Result<boon_runtime::RuntimeTurn, boon_runtime::DistributedRuntimeError> {
        self.prepare_distributed_effect_completion(call_id, outcome, false)
            .map_err(persistent_distributed_machine_error)
    }

    fn prepare_transient_effect_result(
        &mut self,
        call_id: boon_runtime::TransientEffectCallId,
        result_sequence: u64,
        outcome: boon_runtime::Value,
    ) -> Result<boon_runtime::RuntimeTurn, boon_runtime::DistributedRuntimeError> {
        self.prepare_distributed_effect_result(call_id, result_sequence, outcome, false)
            .map_err(persistent_distributed_machine_error)
    }

    fn prepare_transient_effect_cancellation(
        &mut self,
        call_ids: &[boon_runtime::TransientEffectCallId],
    ) -> Result<Option<boon_runtime::RuntimeTurn>, boon_runtime::DistributedRuntimeError> {
        self.prepare_distributed_effect_cancellation(call_ids, false)
            .map_err(persistent_distributed_machine_error)
    }

    fn commit_prepared_turn(
        &mut self,
        turn: boon_runtime::RuntimeTurn,
    ) -> Result<boon_runtime::RuntimeTurn, boon_runtime::DistributedRuntimeError> {
        self.commit_prepared_distributed_turn(turn)
            .map(|committed| committed.turn)
            .map_err(persistent_distributed_machine_error)
    }

    fn rollback_prepared_turn(&mut self) -> Result<(), boon_runtime::DistributedRuntimeError> {
        self.rollback_prepared_distributed_turn()
            .map_err(persistent_distributed_machine_error)
    }

    fn has_pending_transient_effect(&self, call_id: boon_runtime::TransientEffectCallId) -> bool {
        self.runtime
            .pending_transient_effect_credits(call_id)
            .is_some()
    }

    fn set_transient_effect_scope(&mut self, scope: u64) {
        self.runtime.set_transient_effect_scope(scope);
    }

    fn set_machine_origin(
        &mut self,
        origin: boon_runtime::SessionOrigin,
    ) -> Result<(), boon_runtime::DistributedRuntimeError> {
        let origin = boon_runtime::MachineOrigin::new(origin.slot(), origin.generation())
            .map_err(persistent_distributed_machine_error)?;
        self.runtime
            .set_machine_origin(origin)
            .map_err(persistent_distributed_machine_error)
    }

    fn reset_machine_origin(&mut self) -> Result<(), boon_runtime::DistributedRuntimeError> {
        self.runtime
            .reset_machine_origin()
            .map_err(persistent_distributed_machine_error)
    }

    fn drop_producer_origin(
        &mut self,
        origin: boon_runtime::SessionOrigin,
    ) -> Result<Vec<boon_runtime::TransientEffectCallId>, boon_runtime::DistributedRuntimeError>
    {
        let origin = boon_runtime::MachineOrigin::new(origin.slot(), origin.generation())
            .map_err(persistent_distributed_machine_error)?;
        self.runtime
            .drop_producer_origin(origin)
            .map_err(persistent_distributed_machine_error)
    }

    fn root_value_current(
        &mut self,
        name: &str,
    ) -> Result<boon_runtime::Value, boon_runtime::DistributedRuntimeError> {
        self.runtime
            .root_value_current(name)
            .map_err(persistent_distributed_machine_error)
    }
}

fn persistent_distributed_machine_error(
    error: impl fmt::Display,
) -> boon_runtime::DistributedRuntimeError {
    boon_runtime::DistributedRuntimeError::Runtime(error.to_string())
}
