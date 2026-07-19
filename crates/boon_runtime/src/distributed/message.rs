use boon_data::Value;
use boon_plan::{DistributedArgumentId, ExportId, ProgramRole, RemoteCallSiteId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::VecDeque;

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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
    CallRequest {
        call_site_id: RemoteCallSiteId,
        function_export_id: ExportId,
        revision: u64,
        arguments: BTreeMap<DistributedArgumentId, Value>,
    },
    CallResult {
        call_site_id: RemoteCallSiteId,
        revision: u64,
        value: Value,
    },
}

impl DistributedMessage {
    pub(super) fn edge_bytes(&self) -> [u8; 32] {
        match &self.payload {
            DistributedMessagePayload::Current { export_id, .. }
            | DistributedMessagePayload::Event { export_id, .. } => export_id.0,
            DistributedMessagePayload::CallRequest { call_site_id, .. }
            | DistributedMessagePayload::CallResult { call_site_id, .. } => call_site_id.0,
        }
    }

    /// Returns the bounded queue-accounting estimate used by every transport
    /// owner. `None` indicates arithmetic overflow.
    pub fn estimated_bytes(&self) -> Option<usize> {
        let metadata = 128usize;
        let payload = match &self.payload {
            DistributedMessagePayload::Current { value, .. }
            | DistributedMessagePayload::Event { value, .. }
            | DistributedMessagePayload::CallResult { value, .. } => estimated_value_bytes(value),
            DistributedMessagePayload::CallRequest { arguments, .. } => {
                arguments.values().try_fold(0usize, |total, value| {
                    total.checked_add(estimated_value_bytes(value)?)
                })
            }
        };
        metadata.checked_add(payload?)
    }

    pub fn replaces_pending(&self, queued: &Self) -> bool {
        if self.producer != queued.producer
            || self.consumer != queued.consumer
            || self.edge_bytes() != queued.edge_bytes()
        {
            return false;
        }
        matches!(
            (&self.payload, &queued.payload),
            (
                DistributedMessagePayload::Current { .. },
                DistributedMessagePayload::Current { .. }
            ) | (
                DistributedMessagePayload::CallRequest { .. },
                DistributedMessagePayload::CallRequest { .. }
            ) | (
                DistributedMessagePayload::CallResult { .. },
                DistributedMessagePayload::CallResult { .. }
            )
        )
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

    pub(super) fn drain(&mut self, maximum: usize) -> Vec<DistributedMessage> {
        let count = maximum.min(self.messages.len());
        (0..count).filter_map(|_| self.pop_front()).collect()
    }

    pub(super) fn len(&self) -> usize {
        self.messages.len()
    }

    pub(super) fn recovery_messages(&self) -> Vec<DistributedMessage> {
        self.messages.iter().cloned().collect()
    }

    pub(super) fn from_recovery(
        messages: Vec<DistributedMessage>,
        limits: DistributedQueueLimits,
    ) -> Result<Self, super::DistributedRuntimeError> {
        let mut queue = Self::new(limits)?;
        for message in messages {
            let before = queue.messages.len();
            queue.push([message])?;
            if queue.messages.len() != before.saturating_add(1) {
                return Err(super::DistributedRuntimeError::InvalidTransportFrame);
            }
        }
        Ok(queue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_plan::RemoteCallSiteId;

    fn call(revision: u64) -> DistributedMessage {
        DistributedMessage {
            producer: ProgramRole::Session,
            consumer: ProgramRole::Server,
            payload: DistributedMessagePayload::CallRequest {
                call_site_id: RemoteCallSiteId([7; 32]),
                function_export_id: ExportId([8; 32]),
                revision,
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
    fn pending_pure_calls_are_latest_wins_but_events_remain_fifo() {
        let mut queue = TypedMessageQueue::new(DistributedQueueLimits::default()).unwrap();
        queue.push([call(1), call(2)]).unwrap();
        assert_eq!(queue.len(), 1);
        assert!(matches!(
            queue.pop_front().unwrap().payload,
            DistributedMessagePayload::CallRequest { revision: 2, .. }
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
    fn distributed_payloads_and_recovery_are_canonical_data() {
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
        let recovered = TypedMessageQueue::from_recovery(
            queue.recovery_messages(),
            DistributedQueueLimits::default(),
        )
        .unwrap();
        let DistributedMessagePayload::Event { value, .. } =
            &recovered.front_cloned().unwrap().payload
        else {
            panic!("recovered message changed payload kind");
        };
        let _: &boon_data::Value = value;

        let mut encoded = Vec::new();
        ciborium::into_writer(&message, &mut encoded).unwrap();
        assert!(!encoded.is_empty());
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
