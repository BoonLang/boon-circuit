//! Target-neutral Client/Session transport orchestration around the Boon runtime.

#![forbid(unsafe_code)]

mod client;
mod client_session;
mod endpoint;
mod link;
mod message;
mod session;

pub use boon_runtime::DistributedRuntimeError;
pub use client::{
    DistributedClientRuntime, DistributedClientStartupPoll, DistributedClientStartupTask,
    DistributedClientUpdate,
};
pub use client_session::ClientSessionQueueLimits;
pub use message::{DistributedMessage, DistributedMessagePayload, DistributedQueueLimits};
pub use session::{
    DistributedSessionRuntime, DistributedSessionTemplate, DistributedSessionUpdate,
};

use boon_plan::{DistributedEventExportPlan, SourcePayloadField};
use boon_runtime::{
    SourcePayload, Value, export_runtime_arguments, export_runtime_value, import_data_arguments,
    runtime_error, set_source_payload_value,
};

fn exported_event_data(
    export: &DistributedEventExportPlan,
    source: &SourcePayload,
) -> Result<Option<boon_data::Value>, DistributedRuntimeError> {
    let Some(field) = export.payload_field.as_ref() else {
        return Ok(None);
    };
    export_runtime_value(
        source_payload_value(source, field)
            .ok_or_else(|| runtime_error("distributed event export is missing its payload"))?,
    )
    .map(Some)
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
