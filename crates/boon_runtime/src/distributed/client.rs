use super::client_session::{
    ClientSessionQueueLimits, DecodedClientSessionFrame, OutboundClientSessionQueue, decode_frame,
};
use super::endpoint::{
    EndpointBuildPoll, EndpointBuildTask, EndpointRuntime, PreparedEndpointUpdate,
};
use super::link::{ClientSessionLink, ReceiveOperation, SentControl};
use super::{DistributedRuntimeError, runtime_error};
use crate::program::ProgramArtifact;
use crate::program::program_limits;
use crate::{
    DocumentFrame, MachineBuildProgress, RowId, RuntimeTurn, SessionConnectionStatus,
    SessionContext, SessionOptions, SessionPrincipal, SourcePayload, SourceRouteToken, Value,
};
use boon_plan::{DistributedGraphIdentityPlan, DistributedWireSchemaPlan, ProgramRole};
use boon_wire::SessionId;
use std::collections::BTreeSet;
use std::ops::Range;

pub struct DistributedClientRuntime {
    endpoint: EndpointRuntime,
    graph: DistributedGraphIdentityPlan,
    wire_schema: DistributedWireSchemaPlan,
    wire_schema_hash: [u8; 32],
    link: Option<ClientSessionLink>,
    connected: bool,
    outbound: OutboundClientSessionQueue,
    leased_session_frame: Option<LeasedSessionFrame>,
    owned_transient_effects: BTreeSet<crate::TransientEffectCallId>,
}

pub enum DistributedClientStartupPoll {
    Pending(MachineBuildProgress),
    Ready(DistributedClientRuntime),
}

pub struct DistributedClientStartupTask {
    endpoint: EndpointBuildTask,
    graph: DistributedGraphIdentityPlan,
    wire_schema: DistributedWireSchemaPlan,
    wire_schema_hash: [u8; 32],
    outbound: OutboundClientSessionQueue,
}

impl DistributedClientStartupTask {
    pub fn poll(
        &mut self,
        max_steps: usize,
    ) -> Result<DistributedClientStartupPoll, DistributedRuntimeError> {
        match self.endpoint.poll(max_steps)? {
            EndpointBuildPoll::Pending(progress) => {
                Ok(DistributedClientStartupPoll::Pending(progress))
            }
            EndpointBuildPoll::Ready(endpoint) => Ok(DistributedClientStartupPoll::Ready(
                DistributedClientRuntime {
                    endpoint,
                    graph: self.graph.clone(),
                    wire_schema: self.wire_schema.clone(),
                    wire_schema_hash: self.wire_schema_hash,
                    link: None,
                    connected: false,
                    outbound: self.outbound.clone(),
                    leased_session_frame: None,
                    owned_transient_effects: BTreeSet::new(),
                },
            )),
        }
    }
}

struct LeasedSessionFrame {
    bytes: Vec<u8>,
    action: LeasedSessionFrameAction,
}

#[derive(Clone, Copy)]
enum LeasedSessionFrameAction {
    Data {
        operation_sequence: u64,
        generation: u64,
        ack_through: u64,
    },
    Control(SentControl),
}

#[derive(Debug, Default)]
pub struct DistributedClientUpdate {
    pub turns: Vec<RuntimeTurn>,
}

impl DistributedClientRuntime {
    pub fn start(
        artifact: &ProgramArtifact,
        queue_limits: ClientSessionQueueLimits,
    ) -> Result<Self, DistributedRuntimeError> {
        let mut task = Self::begin_start(artifact, queue_limits)?;
        loop {
            match task.poll(usize::MAX)? {
                DistributedClientStartupPoll::Pending(_) => {}
                DistributedClientStartupPoll::Ready(runtime) => return Ok(runtime),
            }
        }
    }

    pub fn begin_start(
        artifact: &ProgramArtifact,
        queue_limits: ClientSessionQueueLimits,
    ) -> Result<DistributedClientStartupTask, DistributedRuntimeError> {
        if artifact.role() != ProgramRole::Client {
            return Err(runtime_error(
                "DistributedClientRuntime requires a Client artifact",
            ));
        }
        let linked = artifact
            .plan()
            .distributed_endpoint
            .as_ref()
            .ok_or_else(|| runtime_error("Client artifact has no distributed endpoint"))?;
        let limits = program_limits(artifact.capability_profile());
        let endpoint = EndpointRuntime::begin_start(
            artifact.machine_template(),
            linked.endpoint.clone(),
            linked.wire_schema.clone(),
            SessionOptions {
                session_context: SessionContext::Available {
                    status: SessionConnectionStatus::Connecting,
                    principal: SessionPrincipal::Anonymous,
                },
                program_revision: artifact.revision(),
                max_work_units_per_transaction: Some(limits.max_runtime_work_units_per_transaction),
                ..SessionOptions::default()
            },
        )?;
        Ok(DistributedClientStartupTask {
            endpoint,
            graph: linked.graph.clone(),
            wire_schema: linked.wire_schema.clone(),
            wire_schema_hash: linked.wire_schema_hash,
            outbound: OutboundClientSessionQueue::new(queue_limits)?,
        })
    }

    pub fn bind(
        &mut self,
        session_id: SessionId,
        generation: u64,
        applied_client_through: u64,
    ) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        if generation == 0 {
            return Err(DistributedRuntimeError::StaleTransportGeneration);
        }
        let mut candidate_outbound = self.outbound.clone();
        candidate_outbound.clear();
        let candidate_link = match self.link.as_ref() {
            Some(link) if link.session_id() == session_id => {
                link.rebind(generation, applied_client_through)?
            }
            Some(_) => {
                let mut link = ClientSessionLink::new(
                    self.graph.graph_id.0,
                    self.wire_schema_hash,
                    self.graph.revision,
                    session_id,
                    generation,
                );
                link.rebase_send_after_handshake(applied_client_through)?;
                link
            }
            None => {
                let mut link = ClientSessionLink::new(
                    self.graph.graph_id.0,
                    self.wire_schema_hash,
                    self.graph.revision,
                    session_id,
                    generation,
                );
                link.rebase_send_after_handshake(applied_client_through)?;
                link
            }
        };
        let candidate_link = Some(candidate_link);
        let prepared = self.endpoint.prepare_context_rebind(
            SessionConnectionStatus::Connecting,
            SessionPrincipal::Anonymous,
            ProgramRole::Session,
            &[ProgramRole::Session],
        )?;
        let update =
            self.route_prepared_with_transport(prepared, candidate_link, candidate_outbound)?;
        self.connected = true;
        self.leased_session_frame = None;
        Ok(update)
    }

    pub fn mark_current(&mut self) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        if !self.connected {
            return Err(DistributedRuntimeError::SessionDisconnected);
        }
        let prepared = self.endpoint.prepare_context_update(
            SessionConnectionStatus::Current,
            SessionPrincipal::Anonymous,
            &[ProgramRole::Session],
        )?;
        self.route_prepared(prepared)
    }

    pub fn mark_stale(&mut self) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        let mut turns = Vec::new();
        if !self.owned_transient_effects.is_empty() {
            let call_ids = self
                .owned_transient_effects
                .iter()
                .copied()
                .collect::<Vec<_>>();
            let prepared = self
                .endpoint
                .prepare_transient_effect_cancellation(&call_ids, &[])?;
            let cancelled = self.route_prepared_with_transport(
                prepared,
                self.link.clone(),
                self.outbound.clone(),
            )?;
            turns.extend(cancelled.turns);
            self.owned_transient_effects.clear();
        }
        let mut candidate_outbound = self.outbound.clone();
        candidate_outbound.clear();
        let prepared = self.endpoint.prepare_context_update(
            SessionConnectionStatus::Stale,
            SessionPrincipal::Anonymous,
            &[],
        )?;
        let update =
            self.route_prepared_with_transport(prepared, self.link.clone(), candidate_outbound)?;
        turns.extend(update.turns);
        self.connected = false;
        self.leased_session_frame = None;
        Ok(DistributedClientUpdate { turns })
    }

    pub fn dispatch(
        &mut self,
        path: &str,
        payload: SourcePayload,
    ) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        if !self.connected {
            return Err(DistributedRuntimeError::SessionDisconnected);
        }
        let prepared = self.endpoint.prepare_event_for_path(path, payload)?;
        let prepared = self
            .endpoint
            .dispatch_prepared(prepared, &[ProgramRole::Session])?;
        self.route_prepared(prepared)
    }

    pub fn dispatch_route(
        &mut self,
        route: SourceRouteToken,
        payload: SourcePayload,
    ) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        if !self.connected {
            return Err(DistributedRuntimeError::SessionDisconnected);
        }
        let prepared = self.endpoint.prepare_event_for_route(route, payload)?;
        let prepared = self
            .endpoint
            .dispatch_prepared(prepared, &[ProgramRole::Session])?;
        self.route_prepared(prepared)
    }

    pub fn source_route_token_for_path(
        &self,
        path: &str,
    ) -> Result<SourceRouteToken, DistributedRuntimeError> {
        self.endpoint.root_route_for_path(path)
    }

    pub fn accept_session_frame(
        &mut self,
        bytes: &[u8],
    ) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        if !self.connected {
            return Err(DistributedRuntimeError::SessionDisconnected);
        }
        let link = self
            .link
            .as_ref()
            .ok_or(DistributedRuntimeError::SessionDisconnected)?;
        let mut candidate_link = link.clone();
        let mut candidate_outbound = self.outbound.clone();
        let decoded = decode_frame(
            &candidate_link,
            &self.wire_schema,
            ProgramRole::Session,
            ProgramRole::Client,
            bytes,
        )?;
        match decoded {
            DecodedClientSessionFrame::Data {
                operation_sequence,
                ack_through,
                message,
            } => {
                let fingerprint = message.semantic_fingerprint()?;
                let acknowledged = candidate_link.accept_peer_ack(ack_through)?;
                candidate_outbound.acknowledge_through(acknowledged);
                match candidate_link.classify_receive(operation_sequence, fingerprint)? {
                    ReceiveOperation::Next => {
                        let prepared = self
                            .endpoint
                            .prepare_accept(message, &[ProgramRole::Session])?;
                        candidate_link.accept_receive(operation_sequence, fingerprint)?;
                        self.route_prepared_with_transport(
                            prepared,
                            Some(candidate_link),
                            candidate_outbound,
                        )
                    }
                    ReceiveOperation::Duplicate => {
                        candidate_link.accept_receive(operation_sequence, fingerprint)?;
                        self.link = Some(candidate_link);
                        self.outbound = candidate_outbound;
                        Ok(DistributedClientUpdate::default())
                    }
                    ReceiveOperation::Gap { expected_next } => {
                        candidate_link.request_resync(expected_next);
                        self.link = Some(candidate_link);
                        self.outbound = candidate_outbound;
                        Ok(DistributedClientUpdate::default())
                    }
                }
            }
            DecodedClientSessionFrame::Ack { ack_through } => {
                let acknowledged = candidate_link.accept_peer_ack(ack_through)?;
                candidate_outbound.acknowledge_through(acknowledged);
                self.link = Some(candidate_link);
                self.outbound = candidate_outbound;
                Ok(DistributedClientUpdate::default())
            }
            DecodedClientSessionFrame::Resync { expected_next } => {
                candidate_outbound
                    .resend_from(expected_next, candidate_link.last_send_operation_sequence())?;
                self.link = Some(candidate_link);
                self.outbound = candidate_outbound;
                Ok(DistributedClientUpdate::default())
            }
        }
    }

    pub fn next_session_frame(&mut self) -> Result<Option<Vec<u8>>, DistributedRuntimeError> {
        if !self.connected {
            return Err(DistributedRuntimeError::SessionDisconnected);
        }
        if let Some(leased) = &self.leased_session_frame {
            return Ok(Some(leased.bytes.clone()));
        }
        let link = self
            .link
            .as_ref()
            .ok_or(DistributedRuntimeError::SessionDisconnected)?;
        if link.has_resync_pending()
            && let Some((bytes, control)) = link.encode_pending_control()?
        {
            self.leased_session_frame = Some(LeasedSessionFrame {
                bytes: bytes.clone(),
                action: LeasedSessionFrameAction::Control(control),
            });
            return Ok(Some(bytes));
        }
        if let Some(data) = self.outbound.encode_next(link, &self.wire_schema)? {
            let generation = link.generation();
            self.leased_session_frame = Some(LeasedSessionFrame {
                bytes: data.bytes.clone(),
                action: LeasedSessionFrameAction::Data {
                    operation_sequence: data.operation_sequence,
                    generation,
                    ack_through: data.ack_through,
                },
            });
            return Ok(Some(data.bytes));
        }
        if let Some((bytes, control)) = link.encode_pending_control()? {
            self.leased_session_frame = Some(LeasedSessionFrame {
                bytes: bytes.clone(),
                action: LeasedSessionFrameAction::Control(control),
            });
            return Ok(Some(bytes));
        }
        Ok(None)
    }

    pub fn acknowledge_session_frame(&mut self) -> bool {
        let Some(leased) = self.leased_session_frame.take() else {
            return false;
        };
        let Some(link) = self.link.as_mut() else {
            self.leased_session_frame = Some(leased);
            return false;
        };
        match leased.action {
            LeasedSessionFrameAction::Data {
                operation_sequence,
                generation,
                ack_through,
            } => {
                if !self.outbound.mark_sent(operation_sequence, generation) {
                    self.leased_session_frame = Some(leased);
                    return false;
                }
                link.acknowledge_piggybacked_receive(ack_through);
            }
            LeasedSessionFrameAction::Control(control) => {
                link.acknowledge_sent_control(control);
            }
        }
        true
    }

    pub fn pending_session_frames(&self) -> usize {
        self.outbound.len()
            + self
                .link
                .as_ref()
                .map(ClientSessionLink::pending_control_count)
                .unwrap_or(0)
    }

    pub fn applied_server_through(&self) -> u64 {
        self.link
            .as_ref()
            .map(ClientSessionLink::applied_receive_through)
            .unwrap_or(0)
    }

    pub fn root_value_current(&mut self, name: &str) -> Result<Value, DistributedRuntimeError> {
        self.endpoint.root_value_current(name)
    }

    pub fn complete_transient_effect(
        &mut self,
        call_id: crate::TransientEffectCallId,
        outcome: Value,
    ) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        if !self.owned_transient_effects.contains(&call_id) {
            return Err(DistributedRuntimeError::InvalidLease);
        }
        let prepared = self.endpoint.prepare_transient_effect_completion(
            call_id,
            outcome,
            &[ProgramRole::Session],
        )?;
        let routed = self.route_prepared(prepared)?;
        let completed = !self.endpoint.has_pending_transient_effect(call_id);
        if completed {
            self.owned_transient_effects.remove(&call_id);
        }
        Ok(routed)
    }

    pub fn deliver_transient_effect_result(
        &mut self,
        call_id: crate::TransientEffectCallId,
        result_sequence: u64,
        outcome: Value,
    ) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        if !self.owned_transient_effects.contains(&call_id) {
            return Err(DistributedRuntimeError::InvalidLease);
        }
        let prepared = self.endpoint.prepare_transient_effect_result(
            call_id,
            result_sequence,
            outcome,
            &[ProgramRole::Session],
        )?;
        let routed = self.route_prepared(prepared)?;
        let completed = !self.endpoint.has_pending_transient_effect(call_id);
        if completed {
            self.owned_transient_effects.remove(&call_id);
        }
        Ok(routed)
    }

    pub fn cancel_all_transient_effects(
        &mut self,
    ) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        let call_ids = self
            .owned_transient_effects
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let targets = self
            .connected
            .then_some(&[ProgramRole::Session][..])
            .unwrap_or(&[]);
        let prepared = self
            .endpoint
            .prepare_transient_effect_cancellation(&call_ids, targets)?;
        let routed = self.route_prepared(prepared)?;
        self.owned_transient_effects.clear();
        Ok(routed)
    }

    pub fn pending_transient_effect_count(&self) -> usize {
        self.owned_transient_effects.len()
    }

    pub fn document_frame(&self) -> Option<&DocumentFrame> {
        self.endpoint.document_frame()
    }

    pub fn inspect_value_current(
        &mut self,
        name: &str,
        max_rows: usize,
    ) -> Result<Value, DistributedRuntimeError> {
        self.endpoint.inspect_value_current(name, max_rows)
    }

    pub fn demand_document_window_by_id(
        &mut self,
        materialization: u64,
        visible: Range<u64>,
        overscan: Range<u64>,
    ) -> Result<Vec<crate::DocumentPatch>, DistributedRuntimeError> {
        self.endpoint
            .demand_document_window_by_id(materialization, visible, overscan)
    }

    pub fn row_target_for_source_path(
        &self,
        path: &str,
        key: u64,
        generation: u64,
    ) -> Result<RowId, DistributedRuntimeError> {
        self.endpoint
            .row_target_for_source_path(path, key, generation)
    }

    pub fn source_is_row_scoped(&self, path: &str) -> Option<bool> {
        self.endpoint.source_is_row_scoped(path)
    }

    fn route_prepared(
        &mut self,
        prepared: PreparedEndpointUpdate,
    ) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        self.route_prepared_with_transport(prepared, self.link.clone(), self.outbound.clone())
    }

    fn route_prepared_with_transport(
        &mut self,
        prepared: PreparedEndpointUpdate,
        mut candidate_link: Option<ClientSessionLink>,
        mut candidate_outbound: OutboundClientSessionQueue,
    ) -> Result<DistributedClientUpdate, DistributedRuntimeError> {
        let stage = if prepared.update.messages.is_empty() {
            Ok(())
        } else {
            let link = candidate_link
                .as_mut()
                .ok_or(DistributedRuntimeError::SessionDisconnected);
            match link {
                Ok(link) => {
                    if prepared.update.messages.iter().any(|message| {
                        message.producer != ProgramRole::Client
                            || message.consumer != ProgramRole::Session
                    }) {
                        Err(DistributedRuntimeError::UnknownTransportEdge)
                    } else {
                        candidate_outbound.push(
                            link,
                            &self.wire_schema,
                            prepared.update.messages.clone(),
                        )
                    }
                }
                Err(error) => Err(error),
            }
        };
        if let Err(error) = stage {
            if let Err(rollback) = self.endpoint.rollback_prepared(prepared) {
                return Err(runtime_error(format!(
                    "distributed Client publication failed: {error}; rollback failed: {rollback}"
                )));
            }
            return Err(error);
        }

        let update = self.endpoint.commit_prepared(prepared);
        self.link = candidate_link;
        self.outbound = candidate_outbound;
        self.record_transient_effects(&update.turns);
        Ok(DistributedClientUpdate {
            turns: update.turns,
        })
    }

    fn record_transient_effects(&mut self, turns: &[RuntimeTurn]) {
        for turn in turns {
            self.owned_transient_effects.extend(
                turn.transient_effects
                    .iter()
                    .map(|invocation| invocation.call_id),
            );
            for call_id in &turn.cancelled_transient_effects {
                self.owned_transient_effects.remove(call_id);
            }
        }
    }
}
