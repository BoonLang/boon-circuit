use super::client_session::{DecodedClientSessionFrame, OutboundClientSessionQueue, decode_frame};
use super::endpoint::{EndpointRuntime, PreparedEndpointUpdate};
use super::link::{ClientSessionLink, ReceiveOperation, SentControl};
use super::message::{DistributedMessage, DistributedQueueLimits, TypedMessageQueue};
use super::{DistributedRuntimeError, runtime_error};
use crate::program::ProgramArtifact;
use crate::{
    MachineTemplate, RuntimeTurn, SessionConnectionStatus, SessionContext, SessionOptions,
    SessionPrincipal, Value,
};
use boon_plan::{DistributedGraphIdentityPlan, DistributedWireSchemaPlan, ProgramRole};
use boon_wire::{ClientSessionFrameLimits, SessionId};
use std::collections::{BTreeSet, VecDeque};

const SESSION_SERVER_TARGET: &[ProgramRole] = &[ProgramRole::Server];
const SESSION_CONNECTED_TARGETS: &[ProgramRole] = &[ProgramRole::Client, ProgramRole::Server];

pub struct DistributedSessionRuntime {
    endpoint: EndpointRuntime,
    wire_schema: DistributedWireSchemaPlan,
    link: ClientSessionLink,
    connected: bool,
    principal: SessionPrincipal,
    inbound_client_frames: VecDeque<Vec<u8>>,
    inbound_client_bytes: usize,
    inbound_limits: DistributedQueueLimits,
    outbound_client: OutboundClientSessionQueue,
    leased_client_frame: Option<LeasedClientFrame>,
    outbound_server: TypedMessageQueue,
    owned_transient_effects: BTreeSet<crate::TransientEffectCallId>,
}

#[derive(Clone)]
struct LeasedClientFrame {
    bytes: Vec<u8>,
    action: LeasedClientFrameAction,
}

#[derive(Clone, Copy)]
enum LeasedClientFrameAction {
    Data {
        operation_sequence: u64,
        generation: u64,
        ack_through: u64,
    },
    Control(SentControl),
}

#[derive(Clone)]
pub struct DistributedSessionTemplate {
    machine: MachineTemplate,
    contract: boon_plan::DistributedEndpointContractPlan,
    graph: DistributedGraphIdentityPlan,
    wire_schema: DistributedWireSchemaPlan,
    wire_schema_hash: [u8; 32],
}

#[derive(Debug, Default)]
pub struct DistributedSessionUpdate {
    pub turns: Vec<RuntimeTurn>,
}

impl DistributedSessionTemplate {
    pub fn from_artifact(artifact: &ProgramArtifact) -> Result<Self, DistributedRuntimeError> {
        if artifact.role() != ProgramRole::Session {
            return Err(runtime_error(
                "DistributedSessionTemplate requires a Session artifact",
            ));
        }
        let linked = artifact
            .plan()
            .distributed_endpoint
            .as_ref()
            .ok_or_else(|| runtime_error("Session artifact has no distributed endpoint"))?;
        Ok(Self {
            machine: MachineTemplate::new_shared(artifact.plan().clone()).map_err(runtime_error)?,
            contract: linked.endpoint.clone(),
            graph: linked.graph.clone(),
            wire_schema: linked.wire_schema.clone(),
            wire_schema_hash: linked.wire_schema_hash,
        })
    }

    pub fn instantiate(
        &self,
        session_id: SessionId,
        generation: u64,
        principal: SessionPrincipal,
        queue_limits: DistributedQueueLimits,
    ) -> Result<DistributedSessionRuntime, DistributedRuntimeError> {
        if generation == 0 {
            return Err(DistributedRuntimeError::StaleTransportGeneration);
        }
        let endpoint = EndpointRuntime::start(
            &self.machine,
            self.contract.clone(),
            self.wire_schema.clone(),
            SessionOptions {
                session_context: SessionContext::Available {
                    status: SessionConnectionStatus::Connecting,
                    principal: principal.clone(),
                },
                ..SessionOptions::default()
            },
        )?;
        Ok(DistributedSessionRuntime {
            endpoint,
            wire_schema: self.wire_schema.clone(),
            link: ClientSessionLink::new(
                self.graph.graph_id.0,
                self.wire_schema_hash,
                self.graph.revision,
                session_id,
                generation,
            ),
            connected: false,
            principal,
            inbound_client_frames: VecDeque::new(),
            inbound_client_bytes: 0,
            inbound_limits: queue_limits,
            outbound_client: OutboundClientSessionQueue::new(queue_limits)?,
            leased_client_frame: None,
            outbound_server: TypedMessageQueue::new(queue_limits)?,
            owned_transient_effects: BTreeSet::new(),
        })
    }
}

impl DistributedSessionRuntime {
    pub fn fork_settled(&self) -> Result<Self, DistributedRuntimeError> {
        Ok(Self {
            endpoint: self.endpoint.fork_settled()?,
            wire_schema: self.wire_schema.clone(),
            link: self.link.clone(),
            connected: self.connected,
            principal: self.principal.clone(),
            inbound_client_frames: self.inbound_client_frames.clone(),
            inbound_client_bytes: self.inbound_client_bytes,
            inbound_limits: self.inbound_limits,
            outbound_client: self.outbound_client.clone(),
            leased_client_frame: self.leased_client_frame.clone(),
            outbound_server: self.outbound_server.clone(),
            owned_transient_effects: self.owned_transient_effects.clone(),
        })
    }

    pub fn mark_current(&mut self) -> Result<DistributedSessionUpdate, DistributedRuntimeError> {
        let prepared = self.endpoint.prepare_context_update(
            SessionConnectionStatus::Current,
            self.principal.clone(),
            SESSION_CONNECTED_TARGETS,
        )?;
        self.route_prepared_with_transport(
            prepared,
            true,
            self.link.clone(),
            self.outbound_client.clone(),
            self.outbound_server.clone(),
        )
    }

    pub fn rebind_client(
        &mut self,
        generation: u64,
        applied_server_through: u64,
    ) -> Result<DistributedSessionUpdate, DistributedRuntimeError> {
        if generation == 0 {
            return Err(DistributedRuntimeError::StaleTransportGeneration);
        }
        let candidate_link = self.link.rebind(generation, applied_server_through)?;
        let mut candidate_client = self.outbound_client.clone();
        candidate_client.clear();
        let prepared = self.endpoint.prepare_context_rebind(
            SessionConnectionStatus::Connecting,
            self.principal.clone(),
            ProgramRole::Client,
            SESSION_CONNECTED_TARGETS,
        )?;
        self.inbound_client_frames.clear();
        self.inbound_client_bytes = 0;
        self.leased_client_frame = None;
        let update = self.route_prepared_with_transport(
            prepared,
            true,
            candidate_link,
            candidate_client,
            self.outbound_server.clone(),
        )?;
        Ok(update)
    }

    pub fn mark_stale(&mut self) -> Result<DistributedSessionUpdate, DistributedRuntimeError> {
        let candidate_client = self.outbound_client.clone();
        let mut candidate_server = self.outbound_server.clone();
        candidate_server.retain_session_resume_snapshots();
        let prepared = self.endpoint.prepare_context_update(
            SessionConnectionStatus::Stale,
            self.principal.clone(),
            SESSION_SERVER_TARGET,
        )?;
        let update = self.route_prepared_with_transport(
            prepared,
            false,
            self.link.clone(),
            candidate_client,
            candidate_server,
        )?;
        self.inbound_client_frames.clear();
        self.inbound_client_bytes = 0;
        self.leased_client_frame = None;
        Ok(update)
    }

    pub fn settle(&mut self) -> Result<DistributedSessionUpdate, DistributedRuntimeError> {
        let prepared = self.endpoint.prepare_settle(self.publish_targets())?;
        self.route_prepared(prepared)
    }

    pub fn admit_client_frame(&mut self, bytes: &[u8]) -> Result<(), DistributedRuntimeError> {
        if !self.connected {
            return Err(DistributedRuntimeError::SessionDisconnected);
        }
        if bytes.len() > ClientSessionFrameLimits::default().max_frame_bytes {
            return Err(DistributedRuntimeError::InvalidTransportFrame);
        }
        if self.inbound_client_frames.len() >= self.inbound_limits.max_messages {
            return Err(DistributedRuntimeError::QueueFull {
                limit: self.inbound_limits.max_messages,
            });
        }
        let next_bytes = self.inbound_client_bytes.checked_add(bytes.len()).ok_or(
            DistributedRuntimeError::QueueBytesFull {
                limit: self.inbound_limits.max_bytes,
            },
        )?;
        if next_bytes > self.inbound_limits.max_bytes {
            return Err(DistributedRuntimeError::QueueBytesFull {
                limit: self.inbound_limits.max_bytes,
            });
        }
        self.inbound_client_frames.push_back(bytes.to_vec());
        self.inbound_client_bytes = next_bytes;
        Ok(())
    }

    pub fn poll_client_frame(
        &mut self,
    ) -> Result<Option<DistributedSessionUpdate>, DistributedRuntimeError> {
        let Some(bytes) = self.inbound_client_frames.front() else {
            return Ok(None);
        };
        let frame_len = bytes.len();
        let next_inbound_bytes = self
            .inbound_client_bytes
            .checked_sub(frame_len)
            .ok_or_else(|| {
                DistributedRuntimeError::Runtime(
                    "inbound Client frame byte accounting is inconsistent".to_owned(),
                )
            })?;
        let mut candidate_link = self.link.clone();
        let decoded = decode_frame(
            &candidate_link,
            &self.wire_schema,
            ProgramRole::Client,
            ProgramRole::Session,
            bytes,
        )?;
        let mut candidate_client = self.outbound_client.clone();
        let routed = match decoded {
            DecodedClientSessionFrame::Data {
                operation_sequence,
                ack_through,
                message,
            } => {
                let fingerprint = message.semantic_fingerprint()?;
                let acknowledged = candidate_link.accept_peer_ack(ack_through)?;
                candidate_client.acknowledge_through(acknowledged);
                match candidate_link.classify_receive(operation_sequence, fingerprint)? {
                    ReceiveOperation::Next => {
                        let prepared = self
                            .endpoint
                            .prepare_accept(message, SESSION_CONNECTED_TARGETS)?;
                        candidate_link.accept_receive(operation_sequence, fingerprint)?;
                        self.route_prepared_with_transport(
                            prepared,
                            self.connected,
                            candidate_link,
                            candidate_client,
                            self.outbound_server.clone(),
                        )?
                    }
                    ReceiveOperation::Duplicate => {
                        candidate_link.accept_receive(operation_sequence, fingerprint)?;
                        self.link = candidate_link;
                        self.outbound_client = candidate_client;
                        DistributedSessionUpdate::default()
                    }
                    ReceiveOperation::Gap { expected_next } => {
                        candidate_link.request_resync(expected_next);
                        self.link = candidate_link;
                        self.outbound_client = candidate_client;
                        DistributedSessionUpdate::default()
                    }
                }
            }
            DecodedClientSessionFrame::Ack { ack_through } => {
                let acknowledged = candidate_link.accept_peer_ack(ack_through)?;
                candidate_client.acknowledge_through(acknowledged);
                self.link = candidate_link;
                self.outbound_client = candidate_client;
                DistributedSessionUpdate::default()
            }
            DecodedClientSessionFrame::Resync { expected_next } => {
                candidate_client
                    .resend_from(expected_next, candidate_link.last_send_operation_sequence())?;
                self.link = candidate_link;
                self.outbound_client = candidate_client;
                DistributedSessionUpdate::default()
            }
        };
        self.inbound_client_frames.pop_front();
        self.inbound_client_bytes = next_inbound_bytes;
        Ok(Some(routed))
    }

    pub fn accept_server_message(
        &mut self,
        message: DistributedMessage,
    ) -> Result<DistributedSessionUpdate, DistributedRuntimeError> {
        if message.producer != ProgramRole::Server || message.consumer != ProgramRole::Session {
            return Err(DistributedRuntimeError::UnknownTransportEdge);
        }
        let targets = self.publish_targets();
        let prepared = self.endpoint.prepare_accept(message, targets)?;
        self.route_prepared(prepared)
    }

    pub fn next_client_frame(&mut self) -> Result<Option<Vec<u8>>, DistributedRuntimeError> {
        if let Some(leased) = &self.leased_client_frame {
            return Ok(Some(leased.bytes.clone()));
        }
        if self.link.has_resync_pending()
            && let Some((bytes, control)) = self.link.encode_pending_control()?
        {
            self.leased_client_frame = Some(LeasedClientFrame {
                bytes: bytes.clone(),
                action: LeasedClientFrameAction::Control(control),
            });
            return Ok(Some(bytes));
        }
        if let Some(data) = self
            .outbound_client
            .encode_next(&self.link, &self.wire_schema)?
        {
            self.leased_client_frame = Some(LeasedClientFrame {
                bytes: data.bytes.clone(),
                action: LeasedClientFrameAction::Data {
                    operation_sequence: data.operation_sequence,
                    generation: self.link.generation(),
                    ack_through: data.ack_through,
                },
            });
            return Ok(Some(data.bytes));
        }
        if let Some((bytes, control)) = self.link.encode_pending_control()? {
            self.leased_client_frame = Some(LeasedClientFrame {
                bytes: bytes.clone(),
                action: LeasedClientFrameAction::Control(control),
            });
            return Ok(Some(bytes));
        }
        Ok(None)
    }

    pub fn acknowledge_client_frame(&mut self) -> bool {
        let Some(leased) = self.leased_client_frame.take() else {
            return false;
        };
        match leased.action {
            LeasedClientFrameAction::Data {
                operation_sequence,
                generation,
                ack_through,
            } => {
                if !self
                    .outbound_client
                    .mark_sent(operation_sequence, generation)
                {
                    self.leased_client_frame = Some(leased);
                    return false;
                }
                self.link.acknowledge_piggybacked_receive(ack_through);
            }
            LeasedClientFrameAction::Control(control) => {
                self.link.acknowledge_sent_control(control);
            }
        }
        true
    }

    pub fn next_server_message(&self) -> Option<DistributedMessage> {
        self.outbound_server.front_cloned()
    }

    pub fn acknowledge_server_message(&mut self) -> bool {
        self.outbound_server.pop_front().is_some()
    }

    pub fn pending_client_frames(&self) -> usize {
        self.outbound_client.len() + self.link.pending_control_count()
    }

    pub fn has_sendable_client_frame(&self) -> bool {
        self.link.pending_control_count() != 0
            || self.outbound_client.has_sendable(self.link.generation())
    }

    pub fn session_id(&self) -> SessionId {
        self.link.session_id()
    }

    pub fn transport_generation(&self) -> u64 {
        self.link.generation()
    }

    pub fn applied_client_through(&self) -> u64 {
        self.link.applied_receive_through()
    }

    pub fn pending_server_messages(&self) -> usize {
        self.outbound_server.len()
    }

    pub fn root_value_current(&mut self, name: &str) -> Result<Value, DistributedRuntimeError> {
        self.endpoint.root_value_current(name)
    }

    pub fn complete_transient_effect(
        &mut self,
        call_id: crate::TransientEffectCallId,
        outcome: Value,
    ) -> Result<DistributedSessionUpdate, DistributedRuntimeError> {
        if !self.owned_transient_effects.contains(&call_id) {
            return Err(DistributedRuntimeError::InvalidLease);
        }
        let targets = self.publish_targets();
        let prepared = self
            .endpoint
            .prepare_transient_effect_completion(call_id, outcome, targets)?;
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
    ) -> Result<DistributedSessionUpdate, DistributedRuntimeError> {
        if !self.owned_transient_effects.contains(&call_id) {
            return Err(DistributedRuntimeError::InvalidLease);
        }
        let targets = self.publish_targets();
        let prepared = self.endpoint.prepare_transient_effect_result(
            call_id,
            result_sequence,
            outcome,
            targets,
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
    ) -> Result<DistributedSessionUpdate, DistributedRuntimeError> {
        let call_ids = self
            .owned_transient_effects
            .iter()
            .copied()
            .collect::<Vec<_>>();
        let targets = self.publish_targets();
        let prepared = self
            .endpoint
            .prepare_transient_effect_cancellation(&call_ids, targets)?;
        let routed = self.route_prepared(prepared)?;
        self.owned_transient_effects.clear();
        Ok(routed)
    }

    pub fn cancel_transient_effect(
        &mut self,
        call_id: crate::TransientEffectCallId,
    ) -> Result<DistributedSessionUpdate, DistributedRuntimeError> {
        if !self.owned_transient_effects.contains(&call_id) {
            return Err(DistributedRuntimeError::InvalidLease);
        }
        let targets = self.publish_targets();
        let prepared = self
            .endpoint
            .prepare_transient_effect_cancellation(&[call_id], targets)?;
        let routed = self.route_prepared(prepared)?;
        self.owned_transient_effects.remove(&call_id);
        Ok(routed)
    }

    pub fn has_pending_transient_effect(&self, call_id: crate::TransientEffectCallId) -> bool {
        self.owned_transient_effects.contains(&call_id)
            && self.endpoint.has_pending_transient_effect(call_id)
    }

    pub fn pending_transient_effect_count(&self) -> usize {
        self.owned_transient_effects.len()
    }

    fn publish_targets(&self) -> &'static [ProgramRole] {
        if self.connected {
            SESSION_CONNECTED_TARGETS
        } else {
            SESSION_SERVER_TARGET
        }
    }

    fn route_prepared(
        &mut self,
        prepared: PreparedEndpointUpdate,
    ) -> Result<DistributedSessionUpdate, DistributedRuntimeError> {
        self.route_prepared_with_transport(
            prepared,
            self.connected,
            self.link.clone(),
            self.outbound_client.clone(),
            self.outbound_server.clone(),
        )
    }

    fn route_prepared_with_transport(
        &mut self,
        prepared: PreparedEndpointUpdate,
        candidate_connected: bool,
        mut candidate_link: ClientSessionLink,
        mut candidate_client: OutboundClientSessionQueue,
        mut candidate_server: TypedMessageQueue,
    ) -> Result<DistributedSessionUpdate, DistributedRuntimeError> {
        let mut client_messages = Vec::new();
        let mut server_messages = Vec::new();
        let messages = prepared.update.messages.clone();
        for message in messages {
            if message.producer != ProgramRole::Session {
                return self
                    .rollback_publication(prepared, DistributedRuntimeError::UnknownTransportEdge);
            }
            match message.consumer {
                ProgramRole::Client => client_messages.push(message),
                ProgramRole::Server => server_messages.push(message),
                ProgramRole::Session => {
                    return self.rollback_publication(
                        prepared,
                        DistributedRuntimeError::UnknownTransportEdge,
                    );
                }
            }
        }

        let stage = (|| {
            if !candidate_connected && !client_messages.is_empty() {
                return Err(DistributedRuntimeError::SessionDisconnected);
            }
            if candidate_connected {
                if self.leased_client_frame.is_some() && !client_messages.is_empty() {
                    return Err(DistributedRuntimeError::QueueFull {
                        limit: candidate_client.max_messages(),
                    });
                }
                candidate_client.push(&mut candidate_link, &self.wire_schema, client_messages)?;
            }
            candidate_server.push(server_messages)
        })();
        if let Err(error) = stage {
            return self.rollback_publication(prepared, error);
        }

        let update = self.endpoint.commit_prepared(prepared);
        self.connected = candidate_connected;
        self.link = candidate_link;
        self.outbound_client = candidate_client;
        self.outbound_server = candidate_server;
        self.record_transient_effects(&update.turns);
        Ok(DistributedSessionUpdate {
            turns: update.turns,
        })
    }

    fn rollback_publication<T>(
        &mut self,
        prepared: PreparedEndpointUpdate,
        error: DistributedRuntimeError,
    ) -> Result<T, DistributedRuntimeError> {
        if let Err(rollback) = self.endpoint.rollback_prepared(prepared) {
            return Err(runtime_error(format!(
                "distributed Session publication failed: {error}; rollback failed: {rollback}"
            )));
        }
        Err(error)
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
