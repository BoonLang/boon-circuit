use crate::{SourcePayload, Value};
use boon_plan::{DistributedArgumentId, DistributedEventExportPlan, SourcePayloadField};
use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt;

mod client;
mod client_session;
mod endpoint;
mod link;
mod message;
mod server;
mod session;

pub use client::{DistributedClientRuntime, DistributedClientUpdate};
pub use client_session::ClientSessionQueueLimits;
pub use message::{DistributedMessage, DistributedMessagePayload, DistributedQueueLimits};
pub use server::{
    DistributedServerAuthority, DistributedServerMachine, DistributedServerRuntime,
    DistributedServerUpdate, PreparedDistributedServerTransaction, PreparedDistributedServerUpdate,
    ServerDelivery, ServerDeliveryTarget, SessionOrigin,
};
pub use session::{
    DistributedSessionRuntime, DistributedSessionTemplate, DistributedSessionUpdate,
};

pub const SESSION_RESUME_WINDOW_MS: u64 = 60_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DistributedRuntimeError {
    InvalidLease,
    SessionDisconnected,
    SessionExpired,
    SessionCapacity { limit: usize },
    QueueFull { limit: usize },
    QueueBytesFull { limit: usize },
    InvalidTransportFrame,
    ProtocolMismatch,
    SchemaMismatch,
    StaleTransportGeneration,
    TransportSequenceMismatch,
    UnknownTransportEdge,
    UnknownTransientEffect,
    StaleTransientEffectOwner,
    UnsupportedScope(String),
    Runtime(String),
}

impl fmt::Display for DistributedRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLease => formatter.write_str("session ownership is invalid"),
            Self::SessionDisconnected => formatter.write_str("session is disconnected"),
            Self::SessionExpired => formatter.write_str("session resume window expired"),
            Self::SessionCapacity { limit } => {
                write!(formatter, "session capacity {limit} is exhausted")
            }
            Self::QueueFull { limit } => {
                write!(formatter, "session queue reached its message limit {limit}")
            }
            Self::QueueBytesFull { limit } => {
                write!(formatter, "session queue reached its byte limit {limit}")
            }
            Self::InvalidTransportFrame => {
                formatter.write_str("client/session transport frame is invalid")
            }
            Self::ProtocolMismatch => {
                formatter.write_str("client/session transport protocol does not match")
            }
            Self::SchemaMismatch => {
                formatter.write_str("client/session transport schema does not match")
            }
            Self::StaleTransportGeneration => {
                formatter.write_str("client/session transport frame is stale")
            }
            Self::TransportSequenceMismatch => {
                formatter.write_str("client/session transport sequence is invalid")
            }
            Self::UnknownTransportEdge => {
                formatter.write_str("transport edge is not part of the linked graph")
            }
            Self::UnknownTransientEffect => {
                formatter.write_str("transient effect call is unknown or no longer active")
            }
            Self::StaleTransientEffectOwner => {
                formatter.write_str("transient effect result belongs to a stale Session")
            }
            Self::UnsupportedScope(message) | Self::Runtime(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl StdError for DistributedRuntimeError {}

pub(super) fn runtime_error(error: impl fmt::Display) -> DistributedRuntimeError {
    DistributedRuntimeError::Runtime(error.to_string())
}

pub(super) fn exported_event_data(
    export: &DistributedEventExportPlan,
    source: &SourcePayload,
) -> Result<boon_data::Value, DistributedRuntimeError> {
    let Some(field) = export.payload_field.as_ref() else {
        return Ok(boon_data::Value::Null);
    };
    export_runtime_value(source_payload_value(source, field).ok_or_else(|| {
        runtime_error(format!(
            "distributed event export {} is missing its payload",
            export.export_id
        ))
    })?)
}

pub(super) fn export_runtime_value(
    value: Value,
) -> Result<boon_data::Value, DistributedRuntimeError> {
    value.to_data().map_err(runtime_error)
}

pub(super) fn export_runtime_arguments(
    arguments: BTreeMap<DistributedArgumentId, Value>,
) -> Result<BTreeMap<DistributedArgumentId, boon_data::Value>, DistributedRuntimeError> {
    arguments
        .into_iter()
        .map(|(argument_id, value)| Ok((argument_id, export_runtime_value(value)?)))
        .collect()
}

pub(super) fn import_data_arguments(
    arguments: BTreeMap<DistributedArgumentId, boon_data::Value>,
) -> BTreeMap<DistributedArgumentId, Value> {
    arguments
        .into_iter()
        .map(|(argument_id, value)| (argument_id, Value::from_data(&value)))
        .collect()
}

fn source_payload_value(payload: &SourcePayload, field: &SourcePayloadField) -> Option<Value> {
    match field {
        SourcePayloadField::Address => payload.address.clone().map(Value::Text),
        SourcePayloadField::Key => payload.key.clone().map(Value::Text),
        SourcePayloadField::Text => payload.text.clone().map(Value::Text),
        SourcePayloadField::Named(name) => payload.fields.get(name).cloned(),
        SourcePayloadField::Bytes => payload
            .fields
            .get("bytes")
            .or_else(|| payload.fields.get("Bytes"))
            .cloned(),
    }
}

pub(super) fn set_source_payload_value(
    payload: &mut SourcePayload,
    field: &SourcePayloadField,
    value: Value,
) -> Result<(), DistributedRuntimeError> {
    match (field, value) {
        (SourcePayloadField::Address, Value::Text(value)) => payload.address = Some(value),
        (SourcePayloadField::Key, Value::Text(value)) => payload.key = Some(value),
        (SourcePayloadField::Text, Value::Text(value)) => payload.text = Some(value),
        (SourcePayloadField::Bytes, value @ Value::Bytes(_)) => {
            payload.fields.insert("bytes".to_owned(), value);
        }
        (SourcePayloadField::Named(name), value) => {
            payload.fields.insert(name.clone(), value);
        }
        (field, value) => {
            return Err(runtime_error(format!(
                "distributed source payload field {field:?} cannot contain {value:?}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
