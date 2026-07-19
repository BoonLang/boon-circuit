use super::message::{DistributedMessage, DistributedMessagePayload};
use super::{
    DistributedRuntimeError, export_runtime_arguments, export_runtime_value, exported_event_data,
    import_data_arguments, runtime_error, set_source_payload_value,
};
use crate::{
    DocumentFrame, LiveRuntime, MachineTemplate, RowId, RuntimeTurn, SessionConnectionStatus,
    SessionOptions, SessionPrincipal, SourceEvent, SourcePayload, TransientEffectCallId, Value,
};
use boon_data::Value as DataValue;
use boon_plan::{
    DistributedArgumentId, DistributedEndpointContractPlan, DistributedWireSchemaPlan, ExportId,
    ImportId, ProgramRole, RemoteCallSiteId, RemoteCallSitePlan, SourceId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Range;

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

#[derive(Clone, Default, Serialize, Deserialize)]
struct EndpointProtocolState {
    sent_values: BTreeMap<(ExportId, ProgramRole), (u64, DataValue)>,
    sent_event_sequences: BTreeMap<ExportId, u64>,
    sent_calls: BTreeMap<RemoteCallSiteId, (u64, BTreeMap<DistributedArgumentId, DataValue>)>,
    accepted_current_revisions: BTreeMap<ImportId, u64>,
    accepted_event_sequences: BTreeMap<ExportId, u64>,
    accepted_call_requests: BTreeMap<RemoteCallSiteId, u64>,
    accepted_call_results: BTreeMap<RemoteCallSiteId, u64>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct EndpointRecoveryImage {
    machine: crate::MachineRecoveryImage,
    next_source_sequence: u64,
    protocol: EndpointProtocolState,
}

impl EndpointRecoveryImage {
    pub(super) fn principal(&self) -> Option<&SessionPrincipal> {
        match &self.machine.session_context {
            crate::SessionContext::Available { principal, .. } => Some(principal),
            crate::SessionContext::Unavailable => None,
        }
    }
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
    pub(super) fn start(
        template: &MachineTemplate,
        options: SessionOptions,
    ) -> Result<Self, DistributedRuntimeError> {
        Ok(Self {
            runtime: LiveRuntime::from_machine_template(template, options)
                .map_err(runtime_error)?,
            next_source_sequence: 1,
        })
    }

    pub(super) fn event_for_path(
        &self,
        path: &str,
        row: Option<RowId>,
        payload: SourcePayload,
    ) -> Result<SourceEvent, DistributedRuntimeError> {
        self.runtime
            .source_event(self.next_source_sequence, path, row, payload)
            .map_err(runtime_error)
    }

    pub(super) fn event_for_source(
        &self,
        source: SourceId,
        payload: SourcePayload,
    ) -> Result<SourceEvent, DistributedRuntimeError> {
        self.runtime
            .source_event_by_id(self.next_source_sequence, source, None, payload)
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

    pub(super) fn call_arguments(
        &mut self,
        call: &RemoteCallSitePlan,
    ) -> Result<BTreeMap<boon_plan::DistributedArgumentId, Value>, DistributedRuntimeError> {
        self.runtime
            .distributed_call_arguments_current(call.call_site_id)
            .map_err(runtime_error)
    }

    pub(super) fn evaluate_function(
        &mut self,
        export_id: ExportId,
        arguments: BTreeMap<boon_plan::DistributedArgumentId, Value>,
    ) -> Result<Value, DistributedRuntimeError> {
        self.runtime
            .evaluate_distributed_function(export_id, arguments)
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
        Ok(Self {
            role,
            contract,
            wire_schema,
            machine: EndpointMachine::start(template, options)?,
            protocol: EndpointProtocolState::default(),
        })
    }

    pub(super) fn start_with_recovery(
        template: &MachineTemplate,
        contract: DistributedEndpointContractPlan,
        wire_schema: DistributedWireSchemaPlan,
        options: SessionOptions,
        recovery: EndpointRecoveryImage,
    ) -> Result<Self, DistributedRuntimeError> {
        if recovery.next_source_sequence == 0 {
            return Err(runtime_error(
                "distributed endpoint recovery source sequence must be positive",
            ));
        }
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
        validate_recovered_protocol(
            role,
            &wire_schema,
            &recovery.protocol,
            &recovery.machine.distributed_imports,
        )?;
        let runtime =
            LiveRuntime::from_machine_template_with_recovery(template, options, recovery.machine)
                .map_err(runtime_error)?;
        Ok(Self {
            role,
            contract,
            wire_schema,
            machine: EndpointMachine {
                runtime,
                next_source_sequence: recovery.next_source_sequence,
            },
            protocol: recovery.protocol,
        })
    }

    pub(super) fn recovery_image(&self) -> Result<EndpointRecoveryImage, DistributedRuntimeError> {
        Ok(EndpointRecoveryImage {
            machine: self
                .machine
                .runtime
                .recovery_image()
                .map_err(runtime_error)?,
            next_source_sequence: self.machine.next_source_sequence,
            protocol: self.protocol.clone(),
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
        row: Option<RowId>,
        payload: SourcePayload,
    ) -> Result<PreparedEndpointEvent, DistributedRuntimeError> {
        let event = self.machine.event_for_path(path, row, payload)?;
        self.prepare_event(&self.protocol, event)
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
                accept_next_sequence(&mut protocol.accepted_event_sequences, export_id, sequence)?;
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
            DistributedMessagePayload::CallRequest {
                call_site_id,
                function_export_id,
                revision,
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
                    })
                    .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
                accept_greater_revision(
                    &mut protocol.accepted_call_requests,
                    call_site_id,
                    revision,
                )?;
                let value = self
                    .machine
                    .evaluate_function(edge.function_export_id, import_data_arguments(arguments))?;
                let value = export_runtime_value(value)?;
                update.messages.push(DistributedMessage {
                    producer: self.role,
                    consumer: producer,
                    payload: DistributedMessagePayload::CallResult {
                        call_site_id,
                        revision,
                        value,
                    },
                });
            }
            DistributedMessagePayload::CallResult {
                call_site_id,
                revision,
                value,
            } => {
                let call = self
                    .contract
                    .remote_call_sites
                    .iter()
                    .find(|call| call.call_site_id == call_site_id && call.callee_role == producer)
                    .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
                let latest_request = self
                    .protocol
                    .sent_calls
                    .get(&call_site_id)
                    .map(|(revision, _)| *revision)
                    .ok_or(DistributedRuntimeError::TransportSequenceMismatch)?;
                if revision < latest_request {
                    return Ok(PreparedEndpointUpdate {
                        update,
                        protocol: self.protocol.clone(),
                        next_source_sequence,
                        machine_turn_pending: false,
                    });
                }
                if revision > latest_request {
                    return Err(DistributedRuntimeError::TransportSequenceMismatch);
                }
                accept_greater_revision(
                    &mut protocol.accepted_call_results,
                    call_site_id,
                    revision,
                )?;
                if let Some(turn) = self.machine.update_import_unsettled(
                    call.result_import_id,
                    revision,
                    Value::from_data(&value),
                )? {
                    machine_turn_pending = true;
                    update.turns.push(turn);
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

    fn collect_current_and_calls(
        &mut self,
        protocol: &mut EndpointProtocolState,
        publish_to: &[ProgramRole],
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
            if protocol
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

        let calls = self
            .contract
            .remote_call_sites
            .iter()
            .filter(|call| publish_to.contains(&call.callee_role))
            .cloned()
            .collect::<Vec<_>>();
        for call in calls {
            let arguments = export_runtime_arguments(self.machine.call_arguments(&call)?)?;
            if protocol
                .sent_calls
                .get(&call.call_site_id)
                .is_some_and(|(_, current)| current == &arguments)
            {
                continue;
            }
            let revision = next_revision(
                protocol
                    .sent_calls
                    .get(&call.call_site_id)
                    .map(|entry| entry.0),
            )?;
            protocol
                .sent_calls
                .insert(call.call_site_id, (revision, arguments.clone()));
            messages.push(DistributedMessage {
                producer: self.role,
                consumer: call.callee_role,
                payload: DistributedMessagePayload::CallRequest {
                    call_site_id: call.call_site_id,
                    function_export_id: call.function_export_id,
                    revision,
                    arguments,
                },
            });
        }
        Ok(messages)
    }

    fn finish_preparation(
        &mut self,
        mut update: EndpointUpdate,
        mut protocol: EndpointProtocolState,
        next_source_sequence: u64,
        machine_turn_pending: bool,
        publish_to: &[ProgramRole],
    ) -> Result<PreparedEndpointUpdate, DistributedRuntimeError> {
        match self.collect_current_and_calls(&mut protocol, publish_to) {
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

    pub(super) fn row_target_for_source_text(
        &self,
        path: &str,
        text: &str,
        occurrence: usize,
    ) -> Result<Option<RowId>, DistributedRuntimeError> {
        self.machine
            .runtime
            .row_target_for_source_text(path, text, occurrence)
            .map_err(runtime_error)
    }

    pub(super) fn source_row_lookup_field(&self, path: &str) -> Option<&str> {
        self.machine.runtime.source_row_lookup_field(path)
    }

    pub(super) fn source_is_row_scoped(&self, path: &str) -> Option<bool> {
        self.machine.runtime.source_is_row_scoped(path)
    }
}

fn validate_recovered_protocol(
    role: ProgramRole,
    schema: &DistributedWireSchemaPlan,
    protocol: &EndpointProtocolState,
    imports: &BTreeMap<ImportId, boon_plan_executor::RecoveryDistributedImport>,
) -> Result<(), DistributedRuntimeError> {
    let positive = |revision: &u64| *revision != 0;
    if protocol
        .sent_values
        .iter()
        .any(|((export_id, consumer), (revision, _))| {
            !positive(revision)
                || !schema.value_edges.iter().any(|edge| {
                    edge.export_id == *export_id
                        && edge.producer_role == role
                        && edge.consumer_role == *consumer
                })
        })
        || protocol
            .sent_event_sequences
            .iter()
            .any(|(export_id, sequence)| {
                !positive(sequence)
                    || !schema
                        .event_edges
                        .iter()
                        .any(|edge| edge.export_id == *export_id && edge.producer_role == role)
            })
        || protocol
            .sent_calls
            .iter()
            .any(|(call_site_id, (revision, _))| {
                !positive(revision)
                    || !schema
                        .call_edges
                        .iter()
                        .any(|edge| edge.call_site_id == *call_site_id && edge.caller_role == role)
            })
        || protocol
            .accepted_current_revisions
            .iter()
            .any(|(import_id, revision)| {
                !positive(revision)
                    || !schema
                        .value_edges
                        .iter()
                        .any(|edge| edge.import_id == *import_id && edge.consumer_role == role)
            })
        || protocol
            .accepted_event_sequences
            .iter()
            .any(|(export_id, sequence)| {
                !positive(sequence)
                    || !schema
                        .event_edges
                        .iter()
                        .any(|edge| edge.export_id == *export_id && edge.consumer_role == role)
            })
        || protocol
            .accepted_call_requests
            .iter()
            .any(|(call_site_id, revision)| {
                !positive(revision)
                    || !schema
                        .call_edges
                        .iter()
                        .any(|edge| edge.call_site_id == *call_site_id && edge.callee_role == role)
            })
        || protocol
            .accepted_call_results
            .iter()
            .any(|(call_site_id, revision)| {
                !positive(revision)
                    || !schema
                        .call_edges
                        .iter()
                        .any(|edge| edge.call_site_id == *call_site_id && edge.caller_role == role)
            })
    {
        return Err(runtime_error(
            "distributed endpoint recovery protocol does not match its wire schema",
        ));
    }
    for (import_id, recovered) in imports {
        if schema
            .value_edges
            .iter()
            .any(|edge| edge.import_id == *import_id && edge.consumer_role == role)
        {
            if protocol.accepted_current_revisions.get(import_id).copied() != recovered.revision {
                return Err(runtime_error(
                    "distributed endpoint recovery Current cursor disagrees with machine import state",
                ));
            }
            continue;
        }
        if let Some(edge) = schema
            .call_edges
            .iter()
            .find(|edge| edge.result_import_id == *import_id && edge.caller_role == role)
        {
            if protocol
                .accepted_call_results
                .get(&edge.call_site_id)
                .copied()
                != recovered.revision
            {
                return Err(runtime_error(
                    "distributed endpoint recovery call-result cursor disagrees with machine import state",
                ));
            }
            continue;
        }
        if recovered.revision.is_some() {
            return Err(runtime_error(
                "distributed endpoint recovery contains a current import without a wire value producer",
            ));
        }
    }
    if protocol
        .accepted_call_results
        .iter()
        .any(|(call_site_id, accepted)| {
            protocol
                .sent_calls
                .get(call_site_id)
                .is_none_or(|(sent, _)| accepted > sent)
        })
    {
        return Err(runtime_error(
            "distributed endpoint recovery accepted a call result beyond its sent call revision",
        ));
    }
    Ok(())
}

fn next_revision(current: Option<u64>) -> Result<u64, DistributedRuntimeError> {
    current
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| runtime_error("distributed edge revision exhausted"))
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

fn accept_next_sequence<K: Ord + Copy>(
    sequences: &mut BTreeMap<K, u64>,
    key: K,
    sequence: u64,
) -> Result<(), DistributedRuntimeError> {
    let expected = sequences
        .get(&key)
        .copied()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| runtime_error("distributed edge sequence exhausted"))?;
    if sequence != expected {
        return Err(DistributedRuntimeError::TransportSequenceMismatch);
    }
    sequences.insert(key, sequence);
    Ok(())
}
