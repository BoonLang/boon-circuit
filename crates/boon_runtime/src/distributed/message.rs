use boon_data::Value;
use boon_plan::{
    DistributedArgumentId, DistributedCallInstanceId, ExportId, ProgramRole, RemoteCallSiteId,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedQueueLimits {
    pub max_messages: usize,
    pub max_bytes: usize,
}

impl Default for DistributedQueueLimits {
    fn default() -> Self {
        Self {
            max_messages: 64,
            max_bytes: 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DistributedMessage {
    pub producer: ProgramRole,
    pub consumer: ProgramRole,
    pub payload: DistributedMessagePayload,
}

/// Canonical data carried between runtime islands.
///
/// Runtime-only values, including host-bound authority, are a different Rust
/// type and cannot populate a distributed payload.
///
/// ```compile_fail
/// use boon_runtime::{DistributedMessagePayload, ExportId, Value};
///
/// let _ = DistributedMessagePayload::Event {
///     export_id: ExportId([0; 32]),
///     sequence: 1,
///     value: Value::Null,
/// };
/// ```
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum DistributedMessagePayload {
    Current {
        export_id: ExportId,
        revision: u64,
        value: Value,
    },
    Event {
        export_id: ExportId,
        sequence: u64,
        value: Value,
    },
    CurrentCallRequest {
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        function_export_id: ExportId,
        demand_revision: u64,
        arguments: BTreeMap<DistributedArgumentId, Value>,
    },
    CurrentCallResult {
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        demand_revision: u64,
        result_revision: u64,
        value: Value,
    },
    CurrentCallDetach {
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        demand_revision: u64,
    },
    InvocationRequest {
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        function_export_id: ExportId,
        sequence: u64,
        arguments: BTreeMap<DistributedArgumentId, Value>,
    },
    InvocationResult {
        call_site_id: RemoteCallSiteId,
        call_instance_id: DistributedCallInstanceId,
        sequence: u64,
        value: Value,
    },
}

impl fmt::Debug for DistributedMessagePayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let variant = match self {
            Self::Current { .. } => "Current",
            Self::Event { .. } => "Event",
            Self::CurrentCallRequest { .. } => "CurrentCallRequest",
            Self::CurrentCallResult { .. } => "CurrentCallResult",
            Self::CurrentCallDetach { .. } => "CurrentCallDetach",
            Self::InvocationRequest { .. } => "InvocationRequest",
            Self::InvocationResult { .. } => "InvocationResult",
        };
        write!(formatter, "DistributedMessagePayload::{variant}(..)")
    }
}

impl DistributedMessage {
    pub(super) fn semantic_fingerprint(&self) -> Result<[u8; 32], super::DistributedRuntimeError> {
        let mut encoded = Vec::new();
        ciborium::ser::into_writer(self, &mut encoded)
            .map_err(|_| super::DistributedRuntimeError::InvalidTransportFrame)?;
        Ok(Sha256::digest(encoded).into())
    }

    pub(super) fn edge_bytes(&self) -> [u8; 32] {
        match &self.payload {
            DistributedMessagePayload::Current { export_id, .. }
            | DistributedMessagePayload::Event { export_id, .. } => export_id.0,
            DistributedMessagePayload::CurrentCallRequest { call_site_id, .. }
            | DistributedMessagePayload::CurrentCallResult { call_site_id, .. }
            | DistributedMessagePayload::CurrentCallDetach { call_site_id, .. }
            | DistributedMessagePayload::InvocationRequest { call_site_id, .. }
            | DistributedMessagePayload::InvocationResult { call_site_id, .. } => call_site_id.0,
        }
    }

    /// Returns the bounded queue-accounting estimate used by every transport
    /// owner. `None` indicates arithmetic overflow.
    pub fn estimated_bytes(&self) -> Option<usize> {
        let metadata = 128usize;
        let payload = match &self.payload {
            DistributedMessagePayload::Current { value, .. }
            | DistributedMessagePayload::Event { value, .. }
            | DistributedMessagePayload::CurrentCallResult { value, .. }
            | DistributedMessagePayload::InvocationResult { value, .. } => {
                estimated_value_bytes(value)
            }
            DistributedMessagePayload::CurrentCallRequest { arguments, .. }
            | DistributedMessagePayload::InvocationRequest { arguments, .. } => {
                arguments.values().try_fold(0usize, |total, value| {
                    total.checked_add(estimated_value_bytes(value)?)
                })
            }
            DistributedMessagePayload::CurrentCallDetach { .. } => Some(0),
        };
        metadata.checked_add(payload?)
    }

    pub fn replaces_pending(&self, queued: &Self) -> bool {
        if self.producer != queued.producer
            || self.consumer != queued.consumer
            || self.edge_bytes() != queued.edge_bytes()
            || self.call_instance_id() != queued.call_instance_id()
        {
            return false;
        }
        matches!(
            (&self.payload, &queued.payload),
            (
                DistributedMessagePayload::Current { .. },
                DistributedMessagePayload::Current { .. }
            ) | (
                DistributedMessagePayload::CurrentCallResult { .. },
                DistributedMessagePayload::CurrentCallResult { .. }
            ) | (
                DistributedMessagePayload::CurrentCallRequest { .. }
                    | DistributedMessagePayload::CurrentCallDetach { .. },
                DistributedMessagePayload::CurrentCallRequest { .. }
                    | DistributedMessagePayload::CurrentCallDetach { .. }
            )
        )
    }

    pub fn call_instance_id(&self) -> Option<DistributedCallInstanceId> {
        match &self.payload {
            DistributedMessagePayload::Current { .. } | DistributedMessagePayload::Event { .. } => {
                None
            }
            DistributedMessagePayload::CurrentCallRequest {
                call_instance_id, ..
            }
            | DistributedMessagePayload::CurrentCallResult {
                call_instance_id, ..
            }
            | DistributedMessagePayload::CurrentCallDetach {
                call_instance_id, ..
            }
            | DistributedMessagePayload::InvocationRequest {
                call_instance_id, ..
            }
            | DistributedMessagePayload::InvocationResult {
                call_instance_id, ..
            } => Some(*call_instance_id),
        }
    }

    /// Only state snapshots may cross a disconnected Session interval.
    ///
    /// Events and call replies belong to the transport generation that
    /// produced them. Retaining those messages for a resumed tab would replay
    /// transient work after its owner had already been cancelled.
    pub fn is_session_resume_snapshot(&self) -> bool {
        matches!(self.payload, DistributedMessagePayload::Current { .. })
    }
}

fn estimated_value_bytes(value: &Value) -> Option<usize> {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => Some(16),
        Value::Text(value) => 16usize.checked_add(value.len()),
        Value::Bytes(value) => 16usize.checked_add(value.len()),
        Value::List(values) => values.iter().try_fold(16usize, |total, value| {
            total.checked_add(estimated_value_bytes(value)?)
        }),
        Value::Record(fields) => estimated_fields_bytes(fields, 16),
        Value::Variant { tag, fields } | Value::Error { code: tag, fields } => {
            estimated_fields_bytes(fields, 32usize.checked_add(tag.len())?)
        }
    }
}

fn estimated_fields_bytes(fields: &BTreeMap<String, Value>, initial: usize) -> Option<usize> {
    fields.iter().try_fold(initial, |total, (name, value)| {
        total
            .checked_add(name.len())?
            .checked_add(estimated_value_bytes(value)?)
    })
}

#[derive(Clone)]
pub(super) struct TypedMessageQueue {
    messages: VecDeque<DistributedMessage>,
    estimated_bytes: usize,
    limits: DistributedQueueLimits,
}

impl TypedMessageQueue {
    pub(super) fn new(
        limits: DistributedQueueLimits,
    ) -> Result<Self, super::DistributedRuntimeError> {
        if limits.max_messages == 0 || limits.max_bytes == 0 {
            return Err(super::DistributedRuntimeError::Runtime(
                "distributed queue limits must be positive".to_owned(),
            ));
        }
        Ok(Self {
            messages: VecDeque::new(),
            estimated_bytes: 0,
            limits,
        })
    }

    pub(super) fn push(
        &mut self,
        messages: impl IntoIterator<Item = DistributedMessage>,
    ) -> Result<(), super::DistributedRuntimeError> {
        let mut candidate = self.messages.clone();
        for message in messages {
            candidate.retain(|queued| !message.replaces_pending(queued));
            candidate.push_back(message);
        }
        if candidate.len() > self.limits.max_messages {
            return Err(super::DistributedRuntimeError::QueueFull {
                limit: self.limits.max_messages,
            });
        }
        let bytes = candidate
            .iter()
            .try_fold(0usize, |total, message| {
                total.checked_add(message.estimated_bytes()?)
            })
            .ok_or(super::DistributedRuntimeError::QueueBytesFull {
                limit: self.limits.max_bytes,
            })?;
        if bytes > self.limits.max_bytes {
            return Err(super::DistributedRuntimeError::QueueBytesFull {
                limit: self.limits.max_bytes,
            });
        }
        self.messages = candidate;
        self.estimated_bytes = bytes;
        Ok(())
    }

    pub(super) fn pop_front(&mut self) -> Option<DistributedMessage> {
        let message = self.messages.pop_front()?;
        let message_bytes = message
            .estimated_bytes()
            .expect("admitted distributed message size must remain representable");
        self.estimated_bytes = self
            .estimated_bytes
            .checked_sub(message_bytes)
            .expect("distributed message queue byte accounting must balance");
        Some(message)
    }

    pub(super) fn front_cloned(&self) -> Option<DistributedMessage> {
        self.messages.front().cloned()
    }

    pub(super) fn retain_session_resume_snapshots(&mut self) {
        self.messages
            .retain(DistributedMessage::is_session_resume_snapshot);
        self.estimated_bytes = self
            .messages
            .iter()
            .map(|message| {
                message
                    .estimated_bytes()
                    .expect("admitted distributed message size remains representable")
            })
            .sum();
    }

    pub(super) fn len(&self) -> usize {
        self.messages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_plan::RemoteCallSiteId;

    fn current_call(demand_revision: u64) -> DistributedMessage {
        DistributedMessage {
            producer: ProgramRole::Session,
            consumer: ProgramRole::Server,
            payload: DistributedMessagePayload::CurrentCallRequest {
                call_site_id: RemoteCallSiteId([7; 32]),
                call_instance_id: DistributedCallInstanceId([6; 32]),
                function_export_id: ExportId([8; 32]),
                demand_revision,
                arguments: BTreeMap::new(),
            },
        }
    }

    fn invocation(sequence: u64) -> DistributedMessage {
        DistributedMessage {
            producer: ProgramRole::Session,
            consumer: ProgramRole::Server,
            payload: DistributedMessagePayload::InvocationRequest {
                call_site_id: RemoteCallSiteId([7; 32]),
                call_instance_id: DistributedCallInstanceId([6; 32]),
                function_export_id: ExportId([8; 32]),
                sequence,
                arguments: BTreeMap::new(),
            },
        }
    }

    fn event(sequence: u64) -> DistributedMessage {
        DistributedMessage {
            producer: ProgramRole::Session,
            consumer: ProgramRole::Server,
            payload: DistributedMessagePayload::Event {
                export_id: ExportId([9; 32]),
                sequence,
                value: Value::Null,
            },
        }
    }

    #[test]
    fn current_calls_are_latest_wins_but_invocations_and_events_remain_fifo() {
        let mut queue = TypedMessageQueue::new(DistributedQueueLimits::default()).unwrap();
        queue.push([current_call(1), current_call(2)]).unwrap();
        assert_eq!(queue.len(), 1);
        assert!(matches!(
            queue.pop_front().unwrap().payload,
            DistributedMessagePayload::CurrentCallRequest {
                demand_revision: 2,
                ..
            }
        ));

        queue.push([invocation(1), invocation(2)]).unwrap();
        assert_eq!(queue.len(), 2);
        assert!(matches!(
            queue.pop_front().unwrap().payload,
            DistributedMessagePayload::InvocationRequest { sequence: 1, .. }
        ));
        assert!(matches!(
            queue.pop_front().unwrap().payload,
            DistributedMessagePayload::InvocationRequest { sequence: 2, .. }
        ));

        queue.push([event(1), event(2)]).unwrap();
        assert_eq!(queue.len(), 2);
        assert!(matches!(
            queue.pop_front().unwrap().payload,
            DistributedMessagePayload::Event { sequence: 1, .. }
        ));
        assert!(matches!(
            queue.pop_front().unwrap().payload,
            DistributedMessagePayload::Event { sequence: 2, .. }
        ));
    }

    #[test]
    fn distributed_payloads_are_canonical_data() {
        let message = DistributedMessage {
            producer: ProgramRole::Client,
            consumer: ProgramRole::Session,
            payload: DistributedMessagePayload::Event {
                export_id: ExportId([4; 32]),
                sequence: 1,
                value: Value::Text("ordinary data".to_owned()),
            },
        };
        let mut queue = TypedMessageQueue::new(DistributedQueueLimits::default()).unwrap();
        queue.push([message.clone()]).unwrap();
        let DistributedMessagePayload::Event { value, .. } = &queue.front_cloned().unwrap().payload
        else {
            panic!("queued message changed payload kind");
        };
        let _: &boon_data::Value = value;

        let mut encoded = Vec::new();
        ciborium::into_writer(&message, &mut encoded).unwrap();
        assert!(!encoded.is_empty());
    }

    #[test]
    fn distributed_message_debug_is_structural_and_redacted() {
        const SENTINEL: &str = "wire-secret-82be7a";
        let message = DistributedMessage {
            producer: ProgramRole::Session,
            consumer: ProgramRole::Server,
            payload: DistributedMessagePayload::CurrentCallRequest {
                call_site_id: RemoteCallSiteId([0xa1; 32]),
                call_instance_id: DistributedCallInstanceId([0xa2; 32]),
                function_export_id: ExportId([0xa3; 32]),
                demand_revision: 0xa4,
                arguments: BTreeMap::from([(
                    DistributedArgumentId([0xa5; 32]),
                    Value::Text(SENTINEL.to_owned()),
                )]),
            },
        };

        let diagnostic = format!("{message:?}");
        assert!(diagnostic.contains("CurrentCallRequest"));
        for hidden in [SENTINEL, "a1a1", "a2a2", "a3a3", "a5a5"] {
            assert!(!diagnostic.contains(hidden), "leaked `{hidden}`");
        }
    }

    #[test]
    fn canonical_nested_values_retain_bounded_queue_accounting() {
        let message = DistributedMessage {
            producer: ProgramRole::Session,
            consumer: ProgramRole::Server,
            payload: DistributedMessagePayload::Event {
                export_id: ExportId([5; 32]),
                sequence: 1,
                value: Value::Error {
                    code: "read_failed".to_owned(),
                    fields: BTreeMap::from([(
                        "detail".to_owned(),
                        Value::Variant {
                            tag: "Chunk".to_owned(),
                            fields: BTreeMap::from([(
                                "bytes".to_owned(),
                                Value::Bytes(boon_data::Bytes::from_static(b"bounded")),
                            )]),
                        },
                    )]),
                },
            },
        };
        let estimated_bytes = message.estimated_bytes().unwrap();
        let mut exact = TypedMessageQueue::new(DistributedQueueLimits {
            max_messages: 1,
            max_bytes: estimated_bytes,
        })
        .unwrap();
        exact.push([message.clone()]).unwrap();

        let mut too_small = TypedMessageQueue::new(DistributedQueueLimits {
            max_messages: 1,
            max_bytes: estimated_bytes - 1,
        })
        .unwrap();
        assert!(matches!(
            too_small.push([message]),
            Err(super::super::DistributedRuntimeError::QueueBytesFull { limit })
                if limit == estimated_bytes - 1
        ));
    }
}
