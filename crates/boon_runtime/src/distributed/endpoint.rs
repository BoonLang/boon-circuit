use super::message::{DistributedMessage, DistributedMessagePayload};
use super::{
    DistributedRuntimeError, export_runtime_arguments, export_runtime_value, exported_event_data,
    import_data_arguments, runtime_error, set_source_payload_value,
};
use crate::{
    DistributedCurrentCallInstance, DocumentFrame, LiveRuntime, LiveRuntimeBuildPoll,
    LiveRuntimeBuildTask, MachineBuildProgress, MachineTemplate, RowId, RuntimeTurn,
    SessionConnectionStatus, SessionOptions, SessionPrincipal, SourceEvent, SourcePayload,
    SourceRouteToken, TransientEffectCallId, Value,
};
use boon_data::Value as DataValue;
use boon_plan::{
    DistributedArgumentId, DistributedCallInstanceId, DistributedCallMode,
    DistributedEndpointContractPlan, DistributedWireSchemaPlan, ExportId, ImportId, ProgramRole,
    RemoteCallSiteId, RemoteCallSitePlan, SourceId,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::Range;

const INVOCATION_REPLAY_WINDOW: u64 = 256;
const CURRENT_CALL_TOMBSTONE_LIMIT: usize = 1024;

type CallInstanceKey = (RemoteCallSiteId, DistributedCallInstanceId);

#[derive(Clone)]
struct SentCurrentCall {
    demand_revision: u64,
    accepted_result_revision: u64,
    accepted_content_revision: u64,
    accepted_result: Option<DataValue>,
    arguments: Option<BTreeMap<DistributedArgumentId, DataValue>>,
}

#[derive(Clone)]
struct AcceptedCurrentCall {
    demand_revision: u64,
    result_revision: u64,
    value: DataValue,
}

pub(super) struct EndpointMachine {
    pub(super) runtime: LiveRuntime,
    next_source_sequence: u64,
}

pub(super) struct EndpointRuntime {
    role: ProgramRole,
    contract: DistributedEndpointContractPlan,
    wire_schema: DistributedWireSchemaPlan,
    machine: EndpointMachine,
    protocol: EndpointProtocolState,
}

pub(super) enum EndpointBuildPoll {
    Pending(MachineBuildProgress),
    Ready(EndpointRuntime),
}

pub(super) struct EndpointBuildTask {
    role: ProgramRole,
    contract: DistributedEndpointContractPlan,
    wire_schema: DistributedWireSchemaPlan,
    machine: LiveRuntimeBuildTask,
}

impl EndpointBuildTask {
    pub(super) fn poll(
        &mut self,
        max_steps: usize,
    ) -> Result<EndpointBuildPoll, DistributedRuntimeError> {
        match self.machine.poll(max_steps).map_err(runtime_error)? {
            LiveRuntimeBuildPoll::Pending(progress) => Ok(EndpointBuildPoll::Pending(progress)),
            LiveRuntimeBuildPoll::Ready(runtime) => Ok(EndpointBuildPoll::Ready(EndpointRuntime {
                role: self.role,
                contract: self.contract.clone(),
                wire_schema: self.wire_schema.clone(),
                machine: EndpointMachine {
                    runtime,
                    next_source_sequence: 1,
                },
                protocol: EndpointProtocolState::default(),
            })),
        }
    }
}

#[derive(Clone, Default)]
struct EndpointProtocolState {
    sent_values: BTreeMap<(ExportId, ProgramRole), (u64, DataValue)>,
    sent_event_sequences: BTreeMap<ExportId, u64>,
    sent_current_call_sequence: BTreeMap<RemoteCallSiteId, u64>,
    sent_current_calls: BTreeMap<CallInstanceKey, SentCurrentCall>,
    current_call_tombstones: VecDeque<CallInstanceKey>,
    sent_invocation_sequences: BTreeMap<CallInstanceKey, u64>,
    pending_invocation_results:
        BTreeMap<(RemoteCallSiteId, DistributedCallInstanceId, u64), SourceRouteToken>,
    accepted_current_revisions: BTreeMap<ImportId, u64>,
    accepted_event_sequences: BTreeMap<ExportId, u64>,
    accepted_current_call_requests: BTreeMap<CallInstanceKey, u64>,
    accepted_current_calls: BTreeMap<CallInstanceKey, AcceptedCurrentCall>,
    accepted_current_call_tombstones: VecDeque<CallInstanceKey>,
    accepted_invocation_requests: BTreeMap<CallInstanceKey, u64>,
    accepted_invocation_results: BTreeMap<CallInstanceKey, u64>,
    invocation_request_replays:
        BTreeMap<(RemoteCallSiteId, DistributedCallInstanceId, u64), InvocationRequestReplay>,
    invocation_result_replays:
        BTreeMap<(RemoteCallSiteId, DistributedCallInstanceId, u64), DataValue>,
}

#[derive(Clone)]
struct InvocationRequestReplay {
    function_export_id: ExportId,
    arguments: BTreeMap<DistributedArgumentId, DataValue>,
    result: DataValue,
}

pub(super) struct PreparedEndpointEvent {
    event: SourceEvent,
    event_sequences: Vec<(ExportId, u64)>,
    pub(super) messages: Vec<DistributedMessage>,
}

pub(super) struct PreparedEndpointUpdate {
    pub(super) update: EndpointUpdate,
    protocol: EndpointProtocolState,
    next_source_sequence: u64,
    machine_turn_pending: bool,
}

#[derive(Default)]
pub(super) struct EndpointUpdate {
    pub(super) turns: Vec<RuntimeTurn>,
    pub(super) messages: Vec<DistributedMessage>,
}

impl EndpointMachine {
    pub(super) fn root_route_for_path(
        &self,
        path: &str,
    ) -> Result<SourceRouteToken, DistributedRuntimeError> {
        self.runtime
            .source_route_token_for_path(path, &[])
            .map_err(runtime_error)
    }

    pub(super) fn event_for_route(
        &self,
        route: SourceRouteToken,
        payload: SourcePayload,
    ) -> Result<SourceEvent, DistributedRuntimeError> {
        self.runtime
            .source_event(self.next_source_sequence, route, payload)
            .map_err(runtime_error)
    }

    pub(super) fn event_for_source(
        &self,
        source: SourceId,
        payload: SourcePayload,
    ) -> Result<SourceEvent, DistributedRuntimeError> {
        self.runtime
            .source_event_by_id(self.next_source_sequence, source, payload)
            .map_err(runtime_error)
    }

    pub(super) fn dispatch_unsettled(
        &mut self,
        event: SourceEvent,
    ) -> Result<(RuntimeTurn, u64), DistributedRuntimeError> {
        let next = self
            .next_source_sequence
            .checked_add(1)
            .ok_or_else(|| runtime_error("machine source sequence exhausted"))?;
        let turn = self
            .runtime
            .dispatch_unsettled(event)
            .map_err(runtime_error)?;
        Ok((turn, next))
    }

    pub(super) fn update_import_unsettled(
        &mut self,
        import_id: ImportId,
        revision: u64,
        value: Value,
    ) -> Result<Option<RuntimeTurn>, DistributedRuntimeError> {
        self.runtime
            .update_distributed_import_unsettled(import_id, revision, value)
            .map_err(runtime_error)
    }

    pub(super) fn export_current(
        &mut self,
        export_id: ExportId,
    ) -> Result<Value, DistributedRuntimeError> {
        self.runtime
            .distributed_export_value_current(export_id)
            .map_err(runtime_error)
    }

    pub(super) fn call_instances(
        &mut self,
        call: &RemoteCallSitePlan,
    ) -> Result<Vec<DistributedCurrentCallInstance>, DistributedRuntimeError> {
        self.runtime
            .distributed_call_instances_current(call.call_site_id)
            .map_err(runtime_error)
    }

    pub(super) fn producer_call_result_current(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
    ) -> Result<Value, DistributedRuntimeError> {
        self.runtime
            .distributed_producer_call_result_current(call_site_id, call_instance_id)
            .map_err(runtime_error)
    }

    pub(super) fn evaluate_function_instance(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        export_id: ExportId,
        content_revision: u64,
        arguments: BTreeMap<boon_plan::DistributedArgumentId, Value>,
    ) -> Result<(Value, Option<RuntimeTurn>), DistributedRuntimeError> {
        self.runtime
            .evaluate_distributed_function_instance_unsettled(
                call_site_id,
                call_instance_id,
                export_id,
                content_revision,
                arguments,
            )
            .map_err(runtime_error)
    }

    pub(super) fn update_call_result_instance_unsettled(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        content_revision: u64,
        value: Value,
    ) -> Result<Option<RuntimeTurn>, DistributedRuntimeError> {
        self.runtime
            .update_distributed_call_result_instance_unsettled(
                call_site_id,
                call_instance_id,
                content_revision,
                value,
            )
            .map_err(runtime_error)
    }

    pub(super) fn drop_call_instance_unsettled(
        &mut self,
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
    ) -> Result<Option<RuntimeTurn>, DistributedRuntimeError> {
        self.runtime
            .drop_producer_call_instance_unsettled(call_site_id, call_instance_id)
            .map_err(runtime_error)
    }

    pub(super) fn update_context_unsettled(
        &mut self,
        status: SessionConnectionStatus,
        principal: SessionPrincipal,
    ) -> Result<Option<RuntimeTurn>, DistributedRuntimeError> {
        self.runtime
            .update_session_context_unsettled(status, principal)
            .map_err(runtime_error)
    }

    pub(super) fn complete_transient_effect_unsettled(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        self.runtime
            .complete_transient_effect_unsettled(call_id, outcome)
            .map_err(runtime_error)
    }

    pub(super) fn deliver_transient_effect_result_unsettled(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
    ) -> Result<RuntimeTurn, DistributedRuntimeError> {
        self.runtime
            .deliver_transient_effect_result_unsettled(call_id, result_sequence, outcome)
            .map_err(runtime_error)
    }

    pub(super) fn cancel_transient_effects_unsettled(
        &mut self,
        call_ids: &[TransientEffectCallId],
    ) -> Result<Option<RuntimeTurn>, DistributedRuntimeError> {
        self.runtime
            .cancel_transient_effects_unsettled(call_ids)
            .map_err(runtime_error)
    }

    fn settle(&mut self, next_source_sequence: u64, machine_turn_pending: bool) {
        if machine_turn_pending {
            self.runtime.settle_turn();
        }
        self.next_source_sequence = next_source_sequence;
    }

    fn rollback(&mut self, machine_turn_pending: bool) -> Result<(), DistributedRuntimeError> {
        if machine_turn_pending {
            self.runtime
                .rollback_unsettled_turn()
                .map_err(runtime_error)?;
        }
        Ok(())
    }

    pub(super) fn has_pending_transient_effect(&self, call_id: TransientEffectCallId) -> bool {
        self.runtime
            .pending_transient_effect_credits(call_id)
            .is_some()
    }

    pub(super) fn root_value_current(
        &mut self,
        name: &str,
    ) -> Result<Value, DistributedRuntimeError> {
        self.runtime.root_value_current(name).map_err(runtime_error)
    }

    pub(super) fn document_frame(&self) -> Option<&DocumentFrame> {
        self.runtime.document_frame()
    }
}

impl EndpointRuntime {
    pub(super) fn start(
        template: &MachineTemplate,
        contract: DistributedEndpointContractPlan,
        wire_schema: DistributedWireSchemaPlan,
        options: SessionOptions,
    ) -> Result<Self, DistributedRuntimeError> {
        let mut task = Self::begin_start(template, contract, wire_schema, options)?;
        loop {
            match task.poll(usize::MAX)? {
                EndpointBuildPoll::Pending(_) => {}
                EndpointBuildPoll::Ready(runtime) => return Ok(runtime),
            }
        }
    }

    pub(super) fn begin_start(
        template: &MachineTemplate,
        contract: DistributedEndpointContractPlan,
        wire_schema: DistributedWireSchemaPlan,
        options: SessionOptions,
    ) -> Result<EndpointBuildTask, DistributedRuntimeError> {
        let role = contract.role;
        if !wire_schema
            .endpoints
            .iter()
            .any(|endpoint| endpoint.role == role && endpoint.endpoint_id == contract.endpoint_id)
        {
            return Err(runtime_error(
                "distributed endpoint is absent from its linked wire schema",
            ));
        }
        Ok(EndpointBuildTask {
            role,
            contract,
            wire_schema,
            machine: LiveRuntime::begin_machine_template_build(template, options)
                .map_err(runtime_error)?,
        })
    }

    pub(super) fn fork_settled(&self) -> Result<Self, DistributedRuntimeError> {
        Ok(Self {
            role: self.role,
            contract: self.contract.clone(),
            wire_schema: self.wire_schema.clone(),
            machine: EndpointMachine {
                runtime: self.machine.runtime.fork_settled().map_err(runtime_error)?,
                next_source_sequence: self.machine.next_source_sequence,
            },
            protocol: self.protocol.clone(),
        })
    }

    pub(super) fn prepare_event_for_path(
        &mut self,
        path: &str,
        payload: SourcePayload,
    ) -> Result<PreparedEndpointEvent, DistributedRuntimeError> {
        let route = self.machine.root_route_for_path(path)?;
        self.prepare_event_for_route(route, payload)
    }

    pub(super) fn prepare_event_for_route(
        &mut self,
        route: SourceRouteToken,
        payload: SourcePayload,
    ) -> Result<PreparedEndpointEvent, DistributedRuntimeError> {
        let event = self.machine.event_for_route(route, payload)?;
        self.prepare_event(&self.protocol, event)
    }

    pub(super) fn root_route_for_path(
        &self,
        path: &str,
    ) -> Result<SourceRouteToken, DistributedRuntimeError> {
        self.machine.root_route_for_path(path)
    }

    fn prepare_event(
        &self,
        protocol: &EndpointProtocolState,
        event: SourceEvent,
    ) -> Result<PreparedEndpointEvent, DistributedRuntimeError> {
        let mut messages = Vec::new();
        let mut event_sequences = Vec::new();
        for export in self
            .contract
            .event_exports
            .iter()
            .filter(|export| export.source_id == event.source)
        {
            let sequence = next_revision(
                protocol
                    .sent_event_sequences
                    .get(&export.export_id)
                    .copied(),
            )?;
            event_sequences.push((export.export_id, sequence));
            let value = exported_event_data(export, &event.payload)?;
            for edge in self.wire_schema.event_edges.iter().filter(|edge| {
                edge.export_id == export.export_id && edge.producer_role == self.role
            }) {
                messages.push(DistributedMessage {
                    producer: self.role,
                    consumer: edge.consumer_role,
                    payload: DistributedMessagePayload::Event {
                        export_id: export.export_id,
                        sequence,
                        value: value.clone(),
                    },
                });
            }
        }
        Ok(PreparedEndpointEvent {
            event,
            event_sequences,
            messages,
        })
    }

    pub(super) fn dispatch_prepared(
        &mut self,
        prepared: PreparedEndpointEvent,
        publish_to: &[ProgramRole],
    ) -> Result<PreparedEndpointUpdate, DistributedRuntimeError> {
        let mut protocol = self.protocol.clone();
        for (export_id, sequence) in prepared.event_sequences {
            protocol.sent_event_sequences.insert(export_id, sequence);
        }
        let (turn, next_source_sequence) = self.machine.dispatch_unsettled(prepared.event)?;
        let update = EndpointUpdate {
            turns: vec![turn],
            messages: prepared.messages,
        };
        self.finish_preparation(update, protocol, next_source_sequence, true, publish_to)
    }

    pub(super) fn prepare_settle(
        &mut self,
        publish_to: &[ProgramRole],
    ) -> Result<PreparedEndpointUpdate, DistributedRuntimeError> {
        self.finish_preparation(
            EndpointUpdate::default(),
            self.protocol.clone(),
            self.machine.next_source_sequence,
            false,
            publish_to,
        )
    }

    pub(super) fn prepare_context_update(
        &mut self,
        status: SessionConnectionStatus,
        principal: SessionPrincipal,
        publish_to: &[ProgramRole],
    ) -> Result<PreparedEndpointUpdate, DistributedRuntimeError> {
        let turn = self.machine.update_context_unsettled(status, principal)?;
        let machine_turn_pending = turn.is_some();
        self.finish_preparation(
            EndpointUpdate {
                turns: turn.into_iter().collect(),
                messages: Vec::new(),
            },
            self.protocol.clone(),
            self.machine.next_source_sequence,
            machine_turn_pending,
            publish_to,
        )
    }

    pub(super) fn prepare_context_rebind(
        &mut self,
        status: SessionConnectionStatus,
        principal: SessionPrincipal,
        reconnected_role: ProgramRole,
        publish_to: &[ProgramRole],
    ) -> Result<PreparedEndpointUpdate, DistributedRuntimeError> {
        let turn = self.machine.update_context_unsettled(status, principal)?;
        let machine_turn_pending = turn.is_some();
        let abandoned_call_sites = self
            .contract
            .remote_call_sites
            .iter()
            .filter(|call| call.callee_role == reconnected_role)
            .map(|call| call.call_site_id)
            .collect::<Vec<_>>();
        let mut protocol = self.protocol.clone();
        protocol
            .pending_invocation_results
            .retain(|(call_site_id, _, _), _| !abandoned_call_sites.contains(call_site_id));
        self.finish_preparation_with_forced_current(
            EndpointUpdate {
                turns: turn.into_iter().collect(),
                messages: Vec::new(),
            },
            protocol,
            self.machine.next_source_sequence,
            machine_turn_pending,
            publish_to,
            &[reconnected_role],
        )
    }

    pub(super) fn prepare_transient_effect_completion(
        &mut self,
        call_id: TransientEffectCallId,
        outcome: Value,
        publish_to: &[ProgramRole],
    ) -> Result<PreparedEndpointUpdate, DistributedRuntimeError> {
        let turn = self
            .machine
            .complete_transient_effect_unsettled(call_id, outcome)?;
        self.finish_preparation(
            EndpointUpdate {
                turns: vec![turn],
                messages: Vec::new(),
            },
            self.protocol.clone(),
            self.machine.next_source_sequence,
            true,
            publish_to,
        )
    }

    pub(super) fn prepare_transient_effect_result(
        &mut self,
        call_id: TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
        publish_to: &[ProgramRole],
    ) -> Result<PreparedEndpointUpdate, DistributedRuntimeError> {
        let turn = self.machine.deliver_transient_effect_result_unsettled(
            call_id,
            result_sequence,
            outcome,
        )?;
        self.finish_preparation(
            EndpointUpdate {
                turns: vec![turn],
                messages: Vec::new(),
            },
            self.protocol.clone(),
            self.machine.next_source_sequence,
            true,
            publish_to,
        )
    }

    pub(super) fn prepare_transient_effect_cancellation(
        &mut self,
        call_ids: &[TransientEffectCallId],
        publish_to: &[ProgramRole],
    ) -> Result<PreparedEndpointUpdate, DistributedRuntimeError> {
        let turns = self
            .machine
            .cancel_transient_effects_unsettled(call_ids)?
            .into_iter()
            .collect::<Vec<_>>();
        let machine_turn_pending = !turns.is_empty();
        self.finish_preparation(
            EndpointUpdate {
                turns,
                messages: Vec::new(),
            },
            self.protocol.clone(),
            self.machine.next_source_sequence,
            machine_turn_pending,
            publish_to,
        )
    }

    pub(super) fn has_pending_transient_effect(&self, call_id: TransientEffectCallId) -> bool {
        self.machine.has_pending_transient_effect(call_id)
    }

    pub(super) fn prepare_accept(
        &mut self,
        message: DistributedMessage,
        publish_to: &[ProgramRole],
    ) -> Result<PreparedEndpointUpdate, DistributedRuntimeError> {
        if message.consumer != self.role || message.producer == self.role {
            return Err(DistributedRuntimeError::UnknownTransportEdge);
        }
        let producer = message.producer;
        let mut update = EndpointUpdate::default();
        let mut protocol = self.protocol.clone();
        let mut next_source_sequence = self.machine.next_source_sequence;
        let mut machine_turn_pending = false;
        match message.payload {
            DistributedMessagePayload::Current {
                export_id,
                revision,
                value,
            } => {
                let import = self
                    .contract
                    .value_imports
                    .iter()
                    .find(|import| {
                        import.producer_role == producer && import.source_export_id == export_id
                    })
                    .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
                accept_greater_revision(
                    &mut protocol.accepted_current_revisions,
                    import.import_id,
                    revision,
                )?;
                if let Some(turn) = self.machine.update_import_unsettled(
                    import.import_id,
                    revision,
                    Value::from_data(&value),
                )? {
                    machine_turn_pending = true;
                    update.turns.push(turn);
                }
            }
            DistributedMessagePayload::Event {
                export_id,
                sequence,
                value,
            } => {
                let import = self
                    .contract
                    .event_imports
                    .iter()
                    .find(|import| {
                        import.producer_role == producer && import.source_export_id == export_id
                    })
                    .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
                accept_greater_revision(
                    &mut protocol.accepted_event_sequences,
                    export_id,
                    sequence,
                )?;
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
                let prepared = self.prepare_event(&protocol, event)?;
                for (export_id, sequence) in prepared.event_sequences {
                    protocol.sent_event_sequences.insert(export_id, sequence);
                }
                let (turn, next) = self.machine.dispatch_unsettled(prepared.event)?;
                machine_turn_pending = true;
                next_source_sequence = next;
                update.turns.push(turn);
                update.messages.extend(prepared.messages);
            }
            DistributedMessagePayload::CurrentCallRequest {
                call_site_id,
                call_instance_id,
                function_export_id,
                demand_revision,
                arguments,
            } => {
                let edge = self
                    .wire_schema
                    .call_edges
                    .iter()
                    .find(|edge| {
                        edge.call_site_id == call_site_id
                            && edge.caller_role == producer
                            && edge.callee_role == self.role
                            && edge.function_export_id == function_export_id
                            && edge.mode == DistributedCallMode::Current
                    })
                    .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
                let call_key = (call_site_id, call_instance_id);
                accept_greater_revision(
                    &mut protocol.accepted_current_call_requests,
                    call_key,
                    demand_revision,
                )?;
                let (value, turn) = self.machine.evaluate_function_instance(
                    call_site_id,
                    call_instance_id,
                    edge.function_export_id,
                    demand_revision,
                    import_data_arguments(arguments),
                )?;
                let turn_pending = turn.is_some();
                let value = match export_runtime_value(value) {
                    Ok(value) => value,
                    Err(error) => {
                        if let Err(rollback) = self.machine.rollback(turn_pending) {
                            return Err(runtime_error(format!(
                                "distributed endpoint call preparation failed: {error}; rollback failed: {rollback}"
                            )));
                        }
                        return Err(error);
                    }
                };
                if let Some(turn) = turn {
                    machine_turn_pending = true;
                    update.turns.push(turn);
                }
                let result_revision = 1;
                protocol.accepted_current_calls.insert(
                    call_key,
                    AcceptedCurrentCall {
                        demand_revision,
                        result_revision,
                        value: value.clone(),
                    },
                );
                protocol
                    .accepted_current_call_tombstones
                    .retain(|candidate| *candidate != call_key);
                update.messages.push(DistributedMessage {
                    producer: self.role,
                    consumer: producer,
                    payload: DistributedMessagePayload::CurrentCallResult {
                        call_site_id,
                        call_instance_id,
                        demand_revision,
                        result_revision,
                        value,
                    },
                });
            }
            DistributedMessagePayload::CurrentCallDetach {
                call_site_id,
                call_instance_id,
                demand_revision,
            } => {
                self.wire_schema
                    .call_edges
                    .iter()
                    .any(|edge| {
                        edge.call_site_id == call_site_id
                            && edge.caller_role == producer
                            && edge.callee_role == self.role
                            && edge.mode == DistributedCallMode::Current
                    })
                    .then_some(())
                    .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
                let call_key = (call_site_id, call_instance_id);
                accept_greater_revision(
                    &mut protocol.accepted_current_call_requests,
                    call_key,
                    demand_revision,
                )?;
                protocol.accepted_current_calls.remove(&call_key);
                protocol
                    .accepted_current_call_tombstones
                    .push_back(call_key);
                if let Some(turn) = self
                    .machine
                    .drop_call_instance_unsettled(call_site_id, call_instance_id)?
                {
                    machine_turn_pending = true;
                    update.turns.push(turn);
                }
                prune_current_call_tombstones(&mut protocol);
            }
            DistributedMessagePayload::InvocationRequest {
                call_site_id,
                call_instance_id,
                function_export_id,
                sequence,
                arguments,
            } => {
                let edge = self
                    .wire_schema
                    .call_edges
                    .iter()
                    .find(|edge| {
                        edge.call_site_id == call_site_id
                            && edge.caller_role == producer
                            && edge.callee_role == self.role
                            && edge.function_export_id == function_export_id
                            && edge.mode == DistributedCallMode::Invocation
                    })
                    .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
                let function_export_id = edge.function_export_id;
                let call_key = (call_site_id, call_instance_id);
                let accepted = protocol
                    .accepted_invocation_requests
                    .get(&call_key)
                    .copied()
                    .unwrap_or(0);
                let value = if sequence <= accepted {
                    let replay = protocol
                        .invocation_request_replays
                        .get(&(call_site_id, call_instance_id, sequence))
                        .ok_or(DistributedRuntimeError::TransportSequenceMismatch)?;
                    if replay.function_export_id != function_export_id
                        || replay.arguments != arguments
                    {
                        return Err(DistributedRuntimeError::InvalidTransportFrame);
                    }
                    replay.result.clone()
                } else {
                    accept_greater_revision(
                        &mut protocol.accepted_invocation_requests,
                        call_key,
                        sequence,
                    )?;
                    let (value, turn) = self.machine.evaluate_function_instance(
                        call_site_id,
                        call_instance_id,
                        function_export_id,
                        sequence,
                        import_data_arguments(arguments.clone()),
                    )?;
                    let turn_pending = turn.is_some();
                    let value = match export_runtime_value(value) {
                        Ok(value) => value,
                        Err(error) => {
                            if let Err(rollback) = self.machine.rollback(turn_pending) {
                                return Err(runtime_error(format!(
                                    "distributed endpoint invocation preparation failed: {error}; rollback failed: {rollback}"
                                )));
                            }
                            return Err(error);
                        }
                    };
                    if let Some(turn) = turn {
                        machine_turn_pending = true;
                        update.turns.push(turn);
                    }
                    protocol.invocation_request_replays.insert(
                        (call_site_id, call_instance_id, sequence),
                        InvocationRequestReplay {
                            function_export_id,
                            arguments,
                            result: value.clone(),
                        },
                    );
                    prune_invocation_replays(
                        &mut protocol.invocation_request_replays,
                        call_site_id,
                        call_instance_id,
                        sequence,
                    );
                    value
                };
                update.messages.push(DistributedMessage {
                    producer: self.role,
                    consumer: producer,
                    payload: DistributedMessagePayload::InvocationResult {
                        call_site_id,
                        call_instance_id,
                        sequence,
                        value,
                    },
                });
            }
            DistributedMessagePayload::CurrentCallResult {
                call_site_id,
                call_instance_id,
                demand_revision,
                result_revision,
                value,
            } => {
                let call = self
                    .contract
                    .remote_call_sites
                    .iter()
                    .find(|call| {
                        call.call_site_id == call_site_id
                            && call.callee_role == producer
                            && call.mode == DistributedCallMode::Current
                    })
                    .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
                let call_key = (call_site_id, call_instance_id);
                let sent = self
                    .protocol
                    .sent_current_calls
                    .get(&call_key)
                    .ok_or(DistributedRuntimeError::TransportSequenceMismatch)?;
                if sent.arguments.is_none() {
                    if demand_revision <= sent.demand_revision {
                        return Ok(PreparedEndpointUpdate {
                            update,
                            protocol: self.protocol.clone(),
                            next_source_sequence,
                            machine_turn_pending: false,
                        });
                    }
                    return Err(DistributedRuntimeError::TransportSequenceMismatch);
                }
                let latest_request = sent.demand_revision;
                if demand_revision < latest_request {
                    return Ok(PreparedEndpointUpdate {
                        update,
                        protocol: self.protocol.clone(),
                        next_source_sequence,
                        machine_turn_pending: false,
                    });
                }
                if demand_revision > latest_request {
                    return Err(DistributedRuntimeError::TransportSequenceMismatch);
                }
                if result_revision == 0 {
                    return Err(DistributedRuntimeError::TransportSequenceMismatch);
                }
                let sent = protocol
                    .sent_current_calls
                    .get_mut(&call_key)
                    .ok_or(DistributedRuntimeError::TransportSequenceMismatch)?;
                if result_revision < sent.accepted_result_revision {
                    return Ok(PreparedEndpointUpdate {
                        update,
                        protocol: self.protocol.clone(),
                        next_source_sequence,
                        machine_turn_pending: false,
                    });
                }
                if result_revision == sent.accepted_result_revision {
                    if sent.accepted_result.as_ref() == Some(&value) {
                        return Ok(PreparedEndpointUpdate {
                            update,
                            protocol: self.protocol.clone(),
                            next_source_sequence,
                            machine_turn_pending: false,
                        });
                    }
                    return Err(DistributedRuntimeError::InvalidTransportFrame);
                }
                let content_revision = next_revision(Some(sent.accepted_content_revision))?;
                sent.accepted_result_revision = result_revision;
                sent.accepted_content_revision = content_revision;
                sent.accepted_result = Some(value.clone());
                call.result.current_import_id().ok_or_else(|| {
                    runtime_error("current distributed call has no current result import")
                })?;
                if let Some(turn) = self.machine.update_call_result_instance_unsettled(
                    call_site_id,
                    call_instance_id,
                    content_revision,
                    Value::from_data(&value),
                )? {
                    machine_turn_pending = true;
                    update.turns.push(turn);
                }
            }
            DistributedMessagePayload::InvocationResult {
                call_site_id,
                call_instance_id,
                sequence,
                value,
            } => {
                let call = self
                    .contract
                    .remote_call_sites
                    .iter()
                    .find(|call| {
                        call.call_site_id == call_site_id
                            && call.callee_role == producer
                            && call.mode == DistributedCallMode::Invocation
                    })
                    .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
                let call_key = (call_site_id, call_instance_id);
                let latest_request = self
                    .protocol
                    .sent_invocation_sequences
                    .get(&call_key)
                    .copied()
                    .ok_or(DistributedRuntimeError::TransportSequenceMismatch)?;
                if sequence > latest_request {
                    return Err(DistributedRuntimeError::TransportSequenceMismatch);
                }
                let accepted = protocol
                    .accepted_invocation_results
                    .get(&call_key)
                    .copied()
                    .unwrap_or(0);
                if sequence <= accepted {
                    let replay = protocol
                        .invocation_result_replays
                        .get(&(call_site_id, call_instance_id, sequence))
                        .ok_or(DistributedRuntimeError::TransportSequenceMismatch)?;
                    if replay != &value {
                        return Err(DistributedRuntimeError::InvalidTransportFrame);
                    }
                } else {
                    accept_greater_revision(
                        &mut protocol.accepted_invocation_results,
                        call_key,
                        sequence,
                    )?;
                    let (result_source, result_field) = call
                        .result
                        .invocation_source()
                        .map(|(source, field)| (source, field.clone()))
                        .ok_or_else(|| {
                            runtime_error("distributed invocation has no private result source")
                        })?;
                    let result_route = protocol
                        .pending_invocation_results
                        .remove(&(call_site_id, call_instance_id, sequence))
                        .ok_or(DistributedRuntimeError::TransportSequenceMismatch)?;
                    if result_route.source != result_source {
                        return Err(DistributedRuntimeError::InvalidTransportFrame);
                    }
                    let mut payload = SourcePayload::default();
                    set_source_payload_value(
                        &mut payload,
                        &result_field,
                        Value::from_data(&value),
                    )?;
                    let event = self.machine.event_for_route(result_route, payload)?;
                    let prepared = self.prepare_event(&protocol, event)?;
                    for (export_id, sequence) in prepared.event_sequences {
                        protocol.sent_event_sequences.insert(export_id, sequence);
                    }
                    let (turn, next) = self.machine.dispatch_unsettled(prepared.event)?;
                    machine_turn_pending = true;
                    next_source_sequence = next;
                    update.turns.push(turn);
                    update.messages.extend(prepared.messages);
                    protocol
                        .invocation_result_replays
                        .insert((call_site_id, call_instance_id, sequence), value);
                    prune_invocation_replays(
                        &mut protocol.invocation_result_replays,
                        call_site_id,
                        call_instance_id,
                        sequence,
                    );
                }
            }
        }
        self.finish_preparation(
            update,
            protocol,
            next_source_sequence,
            machine_turn_pending,
            publish_to,
        )
    }

    fn collect_invocation_messages(
        &self,
        turns: &[RuntimeTurn],
        protocol: &mut EndpointProtocolState,
        publish_to: &[ProgramRole],
    ) -> Result<Vec<DistributedMessage>, DistributedRuntimeError> {
        let mut messages = Vec::new();
        for invocation in turns.iter().flat_map(|turn| &turn.distributed_invocations) {
            let call = self
                .contract
                .remote_call_sites
                .iter()
                .find(|call| {
                    call.call_site_id == invocation.call_site_id
                        && call.mode == DistributedCallMode::Invocation
                })
                .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
            if !publish_to.contains(&call.callee_role) {
                return Err(DistributedRuntimeError::UnknownTransportEdge);
            }
            let call_key = (call.call_site_id, invocation.call_instance_id);
            let sequence =
                next_revision(protocol.sent_invocation_sequences.get(&call_key).copied())?;
            protocol
                .sent_invocation_sequences
                .insert(call_key, sequence);
            if protocol
                .pending_invocation_results
                .insert(
                    (call.call_site_id, invocation.call_instance_id, sequence),
                    invocation.result_route.clone(),
                )
                .is_some()
            {
                return Err(runtime_error(
                    "distributed invocation result route was registered twice",
                ));
            }
            messages.push(DistributedMessage {
                producer: self.role,
                consumer: call.callee_role,
                payload: DistributedMessagePayload::InvocationRequest {
                    call_site_id: call.call_site_id,
                    call_instance_id: invocation.call_instance_id,
                    function_export_id: call.function_export_id,
                    sequence,
                    arguments: export_runtime_arguments(invocation.arguments.clone())?,
                },
            });
        }
        Ok(messages)
    }

    fn collect_current_and_calls(
        &mut self,
        protocol: &mut EndpointProtocolState,
        publish_to: &[ProgramRole],
        force_current_to: &[ProgramRole],
    ) -> Result<Vec<DistributedMessage>, DistributedRuntimeError> {
        let mut messages = Vec::new();
        let value_edges = self
            .wire_schema
            .value_edges
            .iter()
            .filter(|edge| {
                edge.producer_role == self.role && publish_to.contains(&edge.consumer_role)
            })
            .cloned()
            .collect::<Vec<_>>();
        for edge in value_edges {
            let value = export_runtime_value(self.machine.export_current(edge.export_id)?)?;
            let route = (edge.export_id, edge.consumer_role);
            if !force_current_to.contains(&edge.consumer_role)
                && protocol
                    .sent_values
                    .get(&route)
                    .is_some_and(|(_, current)| current == &value)
            {
                continue;
            }
            let revision = next_revision(protocol.sent_values.get(&route).map(|entry| entry.0))?;
            protocol
                .sent_values
                .insert(route, (revision, value.clone()));
            messages.push(DistributedMessage {
                producer: self.role,
                consumer: edge.consumer_role,
                payload: DistributedMessagePayload::Current {
                    export_id: edge.export_id,
                    revision,
                    value,
                },
            });
        }

        let accepted_calls = protocol
            .accepted_current_calls
            .iter()
            .map(|(key, call)| (*key, call.clone()))
            .collect::<Vec<_>>();
        for ((call_site_id, call_instance_id), current) in accepted_calls {
            let edge = self
                .wire_schema
                .call_edges
                .iter()
                .find(|edge| {
                    edge.call_site_id == call_site_id
                        && edge.callee_role == self.role
                        && edge.mode == DistributedCallMode::Current
                })
                .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
            if !publish_to.contains(&edge.caller_role) {
                continue;
            }
            let value = export_runtime_value(
                self.machine
                    .producer_call_result_current(call_site_id, call_instance_id)?,
            )?;
            if value == current.value {
                continue;
            }
            let result_revision = next_revision(Some(current.result_revision))?;
            let accepted = protocol
                .accepted_current_calls
                .get_mut(&(call_site_id, call_instance_id))
                .ok_or(DistributedRuntimeError::InvalidLease)?;
            if accepted.demand_revision != current.demand_revision
                || accepted.result_revision != current.result_revision
            {
                return Err(runtime_error(
                    "distributed endpoint producer call changed during result collection",
                ));
            }
            accepted.result_revision = result_revision;
            accepted.value = value.clone();
            messages.push(DistributedMessage {
                producer: self.role,
                consumer: edge.caller_role,
                payload: DistributedMessagePayload::CurrentCallResult {
                    call_site_id,
                    call_instance_id,
                    demand_revision: accepted.demand_revision,
                    result_revision,
                    value,
                },
            });
        }

        let calls = self
            .contract
            .remote_call_sites
            .iter()
            .filter(|call| {
                call.mode == DistributedCallMode::Current && publish_to.contains(&call.callee_role)
            })
            .cloned()
            .collect::<Vec<_>>();
        for call in calls {
            let mut live = BTreeSet::new();
            for instance in self.machine.call_instances(&call)? {
                let call_key = (call.call_site_id, instance.call_instance_id);
                let arguments = export_runtime_arguments(instance.arguments)?;
                live.insert(call_key);
                if !force_current_to.contains(&call.callee_role)
                    && protocol
                        .sent_current_calls
                        .get(&call_key)
                        .and_then(|sent| sent.arguments.as_ref())
                        == Some(&arguments)
                {
                    continue;
                }
                let revision = next_current_call_revision(protocol, call.call_site_id)?;
                let accepted_content_revision = protocol
                    .sent_current_calls
                    .get(&call_key)
                    .map(|sent| sent.accepted_content_revision)
                    .unwrap_or(0);
                protocol.sent_current_calls.insert(
                    call_key,
                    SentCurrentCall {
                        demand_revision: revision,
                        accepted_result_revision: 0,
                        accepted_content_revision,
                        accepted_result: None,
                        arguments: Some(arguments.clone()),
                    },
                );
                protocol
                    .current_call_tombstones
                    .retain(|candidate| *candidate != call_key);
                messages.push(DistributedMessage {
                    producer: self.role,
                    consumer: call.callee_role,
                    payload: DistributedMessagePayload::CurrentCallRequest {
                        call_site_id: call.call_site_id,
                        call_instance_id: instance.call_instance_id,
                        function_export_id: call.function_export_id,
                        demand_revision: revision,
                        arguments,
                    },
                });
            }
            let detached = protocol
                .sent_current_calls
                .iter()
                .filter_map(|(key, sent)| {
                    (key.0 == call.call_site_id && sent.arguments.is_some() && !live.contains(key))
                        .then_some(*key)
                })
                .collect::<Vec<_>>();
            for call_key in detached {
                let revision = next_current_call_revision(protocol, call.call_site_id)?;
                let accepted_content_revision = protocol
                    .sent_current_calls
                    .get(&call_key)
                    .map(|sent| sent.accepted_content_revision)
                    .unwrap_or(0);
                protocol.sent_current_calls.insert(
                    call_key,
                    SentCurrentCall {
                        demand_revision: revision,
                        accepted_result_revision: 0,
                        accepted_content_revision,
                        accepted_result: None,
                        arguments: None,
                    },
                );
                protocol.current_call_tombstones.push_back(call_key);
                messages.push(DistributedMessage {
                    producer: self.role,
                    consumer: call.callee_role,
                    payload: DistributedMessagePayload::CurrentCallDetach {
                        call_site_id: call.call_site_id,
                        call_instance_id: call_key.1,
                        demand_revision: revision,
                    },
                });
            }
            prune_current_call_tombstones(protocol);
        }
        Ok(messages)
    }

    fn finish_preparation(
        &mut self,
        update: EndpointUpdate,
        protocol: EndpointProtocolState,
        next_source_sequence: u64,
        machine_turn_pending: bool,
        publish_to: &[ProgramRole],
    ) -> Result<PreparedEndpointUpdate, DistributedRuntimeError> {
        self.finish_preparation_with_forced_current(
            update,
            protocol,
            next_source_sequence,
            machine_turn_pending,
            publish_to,
            &[],
        )
    }

    fn finish_preparation_with_forced_current(
        &mut self,
        mut update: EndpointUpdate,
        mut protocol: EndpointProtocolState,
        next_source_sequence: u64,
        machine_turn_pending: bool,
        publish_to: &[ProgramRole],
        force_current_to: &[ProgramRole],
    ) -> Result<PreparedEndpointUpdate, DistributedRuntimeError> {
        match self.collect_invocation_messages(&update.turns, &mut protocol, publish_to) {
            Ok(messages) => update.messages.extend(messages),
            Err(error) => {
                if let Err(rollback) = self.machine.rollback(machine_turn_pending) {
                    return Err(runtime_error(format!(
                        "distributed invocation preparation failed: {error}; rollback failed: {rollback}"
                    )));
                }
                return Err(error);
            }
        }
        match self.collect_current_and_calls(&mut protocol, publish_to, force_current_to) {
            Ok(messages) => update.messages.extend(messages),
            Err(error) => {
                if let Err(rollback) = self.machine.rollback(machine_turn_pending) {
                    return Err(runtime_error(format!(
                        "distributed endpoint preparation failed: {error}; rollback failed: {rollback}"
                    )));
                }
                return Err(error);
            }
        }
        Ok(PreparedEndpointUpdate {
            update,
            protocol,
            next_source_sequence,
            machine_turn_pending,
        })
    }

    pub(super) fn commit_prepared(&mut self, prepared: PreparedEndpointUpdate) -> EndpointUpdate {
        self.machine
            .settle(prepared.next_source_sequence, prepared.machine_turn_pending);
        self.protocol = prepared.protocol;
        prepared.update
    }

    pub(super) fn rollback_prepared(
        &mut self,
        prepared: PreparedEndpointUpdate,
    ) -> Result<(), DistributedRuntimeError> {
        self.machine.rollback(prepared.machine_turn_pending)
    }

    pub(super) fn root_value_current(
        &mut self,
        name: &str,
    ) -> Result<Value, DistributedRuntimeError> {
        self.machine.root_value_current(name)
    }

    pub(super) fn document_frame(&self) -> Option<&DocumentFrame> {
        self.machine.document_frame()
    }

    pub(super) fn inspect_value_current(
        &mut self,
        name: &str,
        max_rows: usize,
    ) -> Result<Value, DistributedRuntimeError> {
        self.machine
            .runtime
            .inspect_value_current(name, max_rows)
            .map_err(runtime_error)
    }

    pub(super) fn demand_document_window_by_id(
        &mut self,
        materialization: u64,
        visible: Range<u64>,
        overscan: Range<u64>,
    ) -> Result<Vec<crate::DocumentPatch>, DistributedRuntimeError> {
        self.machine
            .runtime
            .demand_document_window_by_id(materialization, visible, overscan)
            .map_err(runtime_error)
    }

    pub(super) fn row_target_for_source_path(
        &self,
        path: &str,
        key: u64,
        generation: u64,
    ) -> Result<RowId, DistributedRuntimeError> {
        self.machine
            .runtime
            .row_target_for_source_path(path, key, generation)
            .map_err(runtime_error)
    }

    pub(super) fn source_is_row_scoped(&self, path: &str) -> Option<bool> {
        self.machine.runtime.source_is_row_scoped(path)
    }
}

fn next_revision(current: Option<u64>) -> Result<u64, DistributedRuntimeError> {
    current
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| runtime_error("distributed edge revision exhausted"))
}

fn next_current_call_revision(
    protocol: &mut EndpointProtocolState,
    call_site_id: RemoteCallSiteId,
) -> Result<u64, DistributedRuntimeError> {
    let revision = next_revision(
        protocol
            .sent_current_call_sequence
            .get(&call_site_id)
            .copied(),
    )?;
    protocol
        .sent_current_call_sequence
        .insert(call_site_id, revision);
    Ok(revision)
}

fn prune_current_call_tombstones(protocol: &mut EndpointProtocolState) {
    while protocol.current_call_tombstones.len() > CURRENT_CALL_TOMBSTONE_LIMIT {
        let Some(key) = protocol.current_call_tombstones.pop_front() else {
            break;
        };
        if protocol
            .sent_current_calls
            .get(&key)
            .is_some_and(|sent| sent.arguments.is_none())
        {
            protocol.sent_current_calls.remove(&key);
        }
    }
    while protocol.accepted_current_call_tombstones.len() > CURRENT_CALL_TOMBSTONE_LIMIT {
        let Some(key) = protocol.accepted_current_call_tombstones.pop_front() else {
            break;
        };
        if !protocol.accepted_current_calls.contains_key(&key) {
            protocol.accepted_current_call_requests.remove(&key);
        }
    }
}

fn prune_invocation_replays<T>(
    replays: &mut BTreeMap<(RemoteCallSiteId, DistributedCallInstanceId, u64), T>,
    call_site_id: RemoteCallSiteId,
    call_instance_id: DistributedCallInstanceId,
    newest_sequence: u64,
) {
    let oldest_retained = newest_sequence.saturating_sub(INVOCATION_REPLAY_WINDOW - 1);
    replays.retain(|(candidate_site, candidate_instance, sequence), _| {
        *candidate_site != call_site_id
            || *candidate_instance != call_instance_id
            || *sequence >= oldest_retained
    });
}

fn accept_greater_revision<K: Ord + Copy>(
    revisions: &mut BTreeMap<K, u64>,
    key: K,
    revision: u64,
) -> Result<(), DistributedRuntimeError> {
    if revision == 0
        || revisions
            .get(&key)
            .is_some_and(|current| revision <= *current)
    {
        return Err(DistributedRuntimeError::TransportSequenceMismatch);
    }
    revisions.insert(key, revision);
    Ok(())
}
