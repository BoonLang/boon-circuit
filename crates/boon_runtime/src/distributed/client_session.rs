use super::DistributedRuntimeError;
use super::link::ClientSessionLink;
use super::message::{DistributedMessage, DistributedMessagePayload};
use boon_data::Value as DataValue;
use boon_plan::{DistributedWireSchemaPlan, ExportId, ProgramRole, RemoteCallSiteId};
use boon_wire::ClientSessionFrame;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

pub use super::message::DistributedQueueLimits as ClientSessionQueueLimits;

#[derive(Clone)]
struct QueuedClientSessionMessage {
    operation_sequence: u64,
    message: DistributedMessage,
    sent_generation: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct RecoveredClientSessionMessage {
    operation_sequence: u64,
    message: DistributedMessage,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct OutboundClientSessionQueueRecovery {
    messages: Vec<RecoveredClientSessionMessage>,
}

#[derive(Clone)]
pub(super) struct OutboundClientSessionQueue {
    messages: VecDeque<QueuedClientSessionMessage>,
    limits: ClientSessionQueueLimits,
}

pub(super) struct EncodedClientSessionData {
    pub(super) bytes: Vec<u8>,
    pub(super) operation_sequence: u64,
    pub(super) ack_through: u64,
}

impl OutboundClientSessionQueue {
    pub(super) fn new(limits: ClientSessionQueueLimits) -> Result<Self, DistributedRuntimeError> {
        if limits.max_messages == 0 || limits.max_bytes == 0 {
            return Err(DistributedRuntimeError::Runtime(
                "Client/Session queue limits must be positive".to_owned(),
            ));
        }
        Ok(Self {
            messages: VecDeque::new(),
            limits,
        })
    }

    pub(super) fn push(
        &mut self,
        link: &mut ClientSessionLink,
        schema: &DistributedWireSchemaPlan,
        messages: impl IntoIterator<Item = DistributedMessage>,
    ) -> Result<(), DistributedRuntimeError> {
        let mut candidate_messages = self.messages.clone();
        let mut candidate_link = link.clone();
        for message in messages {
            if let Some(queued) = candidate_messages.iter_mut().rev().find(|queued| {
                queued.sent_generation.is_none() && message.replaces_pending(&queued.message)
            }) {
                queued.message = message;
                continue;
            }
            let operation_sequence = candidate_link.allocate_operation_sequence()?;
            candidate_messages.push_back(QueuedClientSessionMessage {
                operation_sequence,
                message,
                sent_generation: None,
            });
        }
        if candidate_messages.len() > self.limits.max_messages {
            return Err(DistributedRuntimeError::QueueFull {
                limit: self.limits.max_messages,
            });
        }
        let mut encoded_bytes = 0usize;
        for queued in &candidate_messages {
            encoded_bytes = encoded_bytes
                .checked_add(
                    encode_message(
                        &candidate_link,
                        schema,
                        queued.operation_sequence,
                        &queued.message,
                    )?
                    .len(),
                )
                .ok_or(DistributedRuntimeError::QueueBytesFull {
                    limit: self.limits.max_bytes,
                })?;
            if encoded_bytes > self.limits.max_bytes {
                return Err(DistributedRuntimeError::QueueBytesFull {
                    limit: self.limits.max_bytes,
                });
            }
        }
        self.messages = candidate_messages;
        *link = candidate_link;
        Ok(())
    }

    pub(super) fn encode_next(
        &self,
        link: &ClientSessionLink,
        schema: &DistributedWireSchemaPlan,
    ) -> Result<Option<EncodedClientSessionData>, DistributedRuntimeError> {
        let Some(queued) = self
            .messages
            .iter()
            .find(|queued| queued.sent_generation != Some(link.generation()))
        else {
            return Ok(None);
        };
        Ok(Some(EncodedClientSessionData {
            bytes: encode_message(link, schema, queued.operation_sequence, &queued.message)?,
            operation_sequence: queued.operation_sequence,
            ack_through: link.applied_receive_through(),
        }))
    }

    pub(super) fn mark_sent(&mut self, operation_sequence: u64, generation: u64) -> bool {
        let Some(queued) = self
            .messages
            .iter_mut()
            .find(|queued| queued.operation_sequence == operation_sequence)
        else {
            return false;
        };
        queued.sent_generation = Some(generation);
        true
    }

    pub(super) fn acknowledge_through(&mut self, ack_through: u64) {
        while self
            .messages
            .front()
            .is_some_and(|queued| queued.operation_sequence <= ack_through)
        {
            self.messages.pop_front();
        }
    }

    pub(super) fn resend_from(
        &mut self,
        expected_next: u64,
        last_operation: u64,
    ) -> Result<(), DistributedRuntimeError> {
        if expected_next == 0 || expected_next > last_operation.saturating_add(1) {
            return Err(DistributedRuntimeError::TransportSequenceMismatch);
        }
        for queued in self
            .messages
            .iter_mut()
            .filter(|queued| queued.operation_sequence >= expected_next)
        {
            queued.sent_generation = None;
        }
        Ok(())
    }

    pub(super) fn len(&self) -> usize {
        self.messages.len()
    }

    pub(super) fn has_sendable(&self, generation: u64) -> bool {
        self.messages
            .iter()
            .any(|queued| queued.sent_generation != Some(generation))
    }

    pub(super) fn max_messages(&self) -> usize {
        self.limits.max_messages
    }

    pub(super) fn clear(&mut self) {
        self.messages.clear();
    }

    pub(super) fn recovery_image(&self) -> OutboundClientSessionQueueRecovery {
        OutboundClientSessionQueueRecovery {
            messages: self
                .messages
                .iter()
                .map(|queued| RecoveredClientSessionMessage {
                    operation_sequence: queued.operation_sequence,
                    message: queued.message.clone(),
                })
                .collect(),
        }
    }

    pub(super) fn from_recovery(
        recovery: OutboundClientSessionQueueRecovery,
        limits: ClientSessionQueueLimits,
        link: &ClientSessionLink,
        schema: &DistributedWireSchemaPlan,
    ) -> Result<Self, DistributedRuntimeError> {
        if recovery.messages.len() > limits.max_messages {
            return Err(DistributedRuntimeError::QueueFull {
                limit: limits.max_messages,
            });
        }
        let first_expected = link
            .peer_acknowledged_through()
            .checked_add(1)
            .ok_or(DistributedRuntimeError::TransportSequenceMismatch)?;
        let last_allocated = link.last_send_operation_sequence();
        if recovery.messages.is_empty() {
            if link.peer_acknowledged_through() != last_allocated {
                return Err(DistributedRuntimeError::TransportSequenceMismatch);
            }
        } else {
            for (index, recovered) in recovery.messages.iter().enumerate() {
                let expected = first_expected
                    .checked_add(index as u64)
                    .ok_or(DistributedRuntimeError::TransportSequenceMismatch)?;
                if recovered.operation_sequence != expected {
                    return Err(DistributedRuntimeError::TransportSequenceMismatch);
                }
            }
            if recovery
                .messages
                .last()
                .is_none_or(|message| message.operation_sequence != last_allocated)
            {
                return Err(DistributedRuntimeError::TransportSequenceMismatch);
            }
        }
        let messages = recovery
            .messages
            .into_iter()
            .map(|recovered| QueuedClientSessionMessage {
                operation_sequence: recovered.operation_sequence,
                message: recovered.message,
                // Writer admission is not remote receipt. Every unacknowledged
                // logical operation is sendable after process recovery.
                sent_generation: None,
            })
            .collect::<VecDeque<_>>();
        let encoded_bytes = messages.iter().try_fold(0usize, |total, queued| {
            total
                .checked_add(
                    encode_message(link, schema, queued.operation_sequence, &queued.message)
                        .map(|bytes| bytes.len())?,
                )
                .ok_or(DistributedRuntimeError::QueueBytesFull {
                    limit: limits.max_bytes,
                })
        });
        if encoded_bytes? > limits.max_bytes {
            return Err(DistributedRuntimeError::QueueBytesFull {
                limit: limits.max_bytes,
            });
        }
        Ok(Self { messages, limits })
    }
}

pub(super) enum DecodedClientSessionFrame {
    Data {
        operation_sequence: u64,
        ack_through: u64,
        message: DistributedMessage,
    },
    Ack {
        ack_through: u64,
    },
    Resync {
        expected_next: u64,
    },
}

fn encode_message(
    link: &ClientSessionLink,
    schema: &DistributedWireSchemaPlan,
    operation_sequence: u64,
    message: &DistributedMessage,
) -> Result<Vec<u8>, DistributedRuntimeError> {
    validate_adjacent_roles(message.producer, message.consumer)?;
    let (semantic_revision, payload) = match &message.payload {
        DistributedMessagePayload::Current {
            export_id,
            revision,
            value,
        } => {
            require_value_edge(schema, *export_id, message.producer, message.consumer)?;
            (*revision, value.clone())
        }
        DistributedMessagePayload::Event {
            export_id,
            sequence,
            value,
        } => {
            require_event_edge(schema, *export_id, message.producer, message.consumer)?;
            (*sequence, value.clone())
        }
        DistributedMessagePayload::CallRequest {
            call_site_id,
            function_export_id,
            revision,
            arguments,
        } => {
            let edge = schema
                .call_edges
                .iter()
                .find(|edge| {
                    edge.call_site_id == *call_site_id
                        && edge.caller_role == message.producer
                        && edge.callee_role == message.consumer
                        && edge.function_export_id == *function_export_id
                })
                .ok_or(DistributedRuntimeError::UnknownTransportEdge)?;
            let values = edge
                .parameters
                .iter()
                .map(|parameter| {
                    arguments
                        .get(&parameter.argument_id)
                        .cloned()
                        .ok_or(DistributedRuntimeError::InvalidTransportFrame)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if values.len() != arguments.len() {
                return Err(DistributedRuntimeError::InvalidTransportFrame);
            }
            (*revision, DataValue::List(values))
        }
        DistributedMessagePayload::CallResult {
            call_site_id,
            revision,
            value,
        } => {
            require_call_result_edge(schema, *call_site_id, message.producer, message.consumer)?;
            (*revision, value.clone())
        }
    };
    link.encode_data(
        operation_sequence,
        message.edge_bytes(),
        semantic_revision,
        payload,
    )
}

pub(super) fn decode_frame(
    link: &ClientSessionLink,
    schema: &DistributedWireSchemaPlan,
    producer: ProgramRole,
    consumer: ProgramRole,
    bytes: &[u8],
) -> Result<DecodedClientSessionFrame, DistributedRuntimeError> {
    validate_adjacent_roles(producer, consumer)?;
    match link.decode_identity(bytes)? {
        ClientSessionFrame::Data {
            operation_sequence,
            ack_through,
            edge_id,
            semantic_revision,
            payload,
            ..
        } => Ok(DecodedClientSessionFrame::Data {
            operation_sequence,
            ack_through,
            message: classify_data(
                schema,
                producer,
                consumer,
                edge_id,
                semantic_revision,
                payload,
            )?,
        }),
        ClientSessionFrame::Ack { ack_through, .. } => {
            Ok(DecodedClientSessionFrame::Ack { ack_through })
        }
        ClientSessionFrame::Resync { expected_next, .. } => {
            Ok(DecodedClientSessionFrame::Resync { expected_next })
        }
    }
}

fn classify_data(
    schema: &DistributedWireSchemaPlan,
    producer: ProgramRole,
    consumer: ProgramRole,
    edge_id: [u8; 32],
    semantic_revision: u64,
    payload: DataValue,
) -> Result<DistributedMessage, DistributedRuntimeError> {
    if let Some(edge) = schema.event_edges.iter().find(|edge| {
        edge.export_id.0 == edge_id
            && edge.producer_role == producer
            && edge.consumer_role == consumer
    }) {
        return Ok(DistributedMessage {
            producer,
            consumer,
            payload: DistributedMessagePayload::Event {
                export_id: edge.export_id,
                sequence: semantic_revision,
                value: payload,
            },
        });
    }
    if let Some(edge) = schema.value_edges.iter().find(|edge| {
        edge.export_id.0 == edge_id
            && edge.producer_role == producer
            && edge.consumer_role == consumer
    }) {
        return Ok(DistributedMessage {
            producer,
            consumer,
            payload: DistributedMessagePayload::Current {
                export_id: edge.export_id,
                revision: semantic_revision,
                value: payload,
            },
        });
    }
    let call_site_id = RemoteCallSiteId(edge_id);
    if let Some(edge) = schema.call_edges.iter().find(|edge| {
        edge.call_site_id == call_site_id
            && edge.caller_role == producer
            && edge.callee_role == consumer
    }) {
        let DataValue::List(values) = &payload else {
            return Err(DistributedRuntimeError::InvalidTransportFrame);
        };
        if values.len() != edge.parameters.len() {
            return Err(DistributedRuntimeError::InvalidTransportFrame);
        }
        let arguments = edge
            .parameters
            .iter()
            .zip(values.iter().cloned())
            .map(|(parameter, value)| (parameter.argument_id, value))
            .collect::<BTreeMap<_, _>>();
        return Ok(DistributedMessage {
            producer,
            consumer,
            payload: DistributedMessagePayload::CallRequest {
                call_site_id,
                function_export_id: edge.function_export_id,
                revision: semantic_revision,
                arguments,
            },
        });
    }
    if schema.call_edges.iter().any(|edge| {
        edge.call_site_id == call_site_id
            && edge.callee_role == producer
            && edge.caller_role == consumer
    }) {
        return Ok(DistributedMessage {
            producer,
            consumer,
            payload: DistributedMessagePayload::CallResult {
                call_site_id,
                revision: semantic_revision,
                value: payload,
            },
        });
    }
    Err(DistributedRuntimeError::UnknownTransportEdge)
}

fn require_value_edge(
    schema: &DistributedWireSchemaPlan,
    export_id: ExportId,
    producer: ProgramRole,
    consumer: ProgramRole,
) -> Result<(), DistributedRuntimeError> {
    schema
        .value_edges
        .iter()
        .any(|edge| {
            edge.export_id == export_id
                && edge.producer_role == producer
                && edge.consumer_role == consumer
        })
        .then_some(())
        .ok_or(DistributedRuntimeError::UnknownTransportEdge)
}

fn require_event_edge(
    schema: &DistributedWireSchemaPlan,
    export_id: ExportId,
    producer: ProgramRole,
    consumer: ProgramRole,
) -> Result<(), DistributedRuntimeError> {
    schema
        .event_edges
        .iter()
        .any(|edge| {
            edge.export_id == export_id
                && edge.producer_role == producer
                && edge.consumer_role == consumer
        })
        .then_some(())
        .ok_or(DistributedRuntimeError::UnknownTransportEdge)
}

fn require_call_result_edge(
    schema: &DistributedWireSchemaPlan,
    call_site_id: RemoteCallSiteId,
    producer: ProgramRole,
    consumer: ProgramRole,
) -> Result<(), DistributedRuntimeError> {
    schema
        .call_edges
        .iter()
        .any(|edge| {
            edge.call_site_id == call_site_id
                && edge.callee_role == producer
                && edge.caller_role == consumer
        })
        .then_some(())
        .ok_or(DistributedRuntimeError::UnknownTransportEdge)
}

fn validate_adjacent_roles(
    producer: ProgramRole,
    consumer: ProgramRole,
) -> Result<(), DistributedRuntimeError> {
    if producer.can_depend_on(consumer) || consumer.can_depend_on(producer) {
        if !matches!(
            (producer, consumer),
            (ProgramRole::Client, ProgramRole::Server) | (ProgramRole::Server, ProgramRole::Client)
        ) {
            return Ok(());
        }
    }
    Err(DistributedRuntimeError::UnknownTransportEdge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_plan::{
        DataTypePlan, DistributedGraphId, DistributedRouteScopePlan, DistributedWireEventEdgePlan,
        ImportId,
    };
    use boon_wire::{ClientSessionFrameLimits, SessionId, decode_client_session_frame};

    fn fixture() -> (
        ExportId,
        DistributedWireSchemaPlan,
        DistributedMessage,
        ClientSessionLink,
    ) {
        let export_id = ExportId([3; 32]);
        let schema = DistributedWireSchemaPlan {
            graph_id: DistributedGraphId([1; 32]),
            endpoints: Vec::new(),
            value_edges: Vec::new(),
            event_edges: vec![DistributedWireEventEdgePlan {
                export_id,
                import_id: ImportId([4; 32]),
                producer_role: ProgramRole::Client,
                consumer_role: ProgramRole::Session,
                scope: DistributedRouteScopePlan::OriginScoped,
                payload_field: None,
                payload_type: DataTypePlan::Null,
            }],
            call_edges: Vec::new(),
        };
        let message = DistributedMessage {
            producer: ProgramRole::Client,
            consumer: ProgramRole::Session,
            payload: DistributedMessagePayload::Event {
                export_id,
                sequence: 7,
                value: DataValue::Null,
            },
        };
        let link = ClientSessionLink::new([1; 32], [2; 32], 1, SessionId::from_bytes([9; 32]), 1);
        (export_id, schema, message, link)
    }

    #[test]
    fn transport_operation_and_semantic_revision_are_independent() {
        let (_, schema, message, mut link) = fixture();
        let mut queue =
            OutboundClientSessionQueue::new(ClientSessionQueueLimits::default()).unwrap();
        queue.push(&mut link, &schema, [message]).unwrap();
        let encoded = queue.encode_next(&link, &schema).unwrap().unwrap();
        let frame =
            decode_client_session_frame(&encoded.bytes, ClientSessionFrameLimits::default())
                .unwrap();
        assert!(matches!(
            frame,
            ClientSessionFrame::Data {
                operation_sequence: 1,
                semantic_revision: 7,
                ..
            }
        ));
    }

    #[test]
    fn rebind_preserves_operation_sequence_and_replays_unacknowledged_data() {
        let (_, schema, message, mut link) = fixture();
        let mut queue =
            OutboundClientSessionQueue::new(ClientSessionQueueLimits::default()).unwrap();
        queue.push(&mut link, &schema, [message.clone()]).unwrap();
        let first = queue.encode_next(&link, &schema).unwrap().unwrap();
        assert!(queue.mark_sent(first.operation_sequence, 1));
        assert!(queue.encode_next(&link, &schema).unwrap().is_none());

        link = link.rebind(link.session_id(), 2);
        let replay = queue.encode_next(&link, &schema).unwrap().unwrap();
        assert_eq!(replay.operation_sequence, 1);
        assert!(queue.mark_sent(replay.operation_sequence, 2));
        queue.push(&mut link, &schema, [message]).unwrap();
        link.accept_peer_ack(1).unwrap();
        queue.acknowledge_through(1);
        let second = queue.encode_next(&link, &schema).unwrap().unwrap();
        assert_eq!(second.operation_sequence, 2);
    }
}
