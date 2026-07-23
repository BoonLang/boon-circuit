use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WebHostError {
    Unsupported { feature: String, reason: String },
    CapabilityDenied { capability: String, reason: String },
    LimitExceeded { resource: String, limit: usize },
    InvalidInput { field: String, reason: String },
    QueueOverflow { queue: String, capacity: usize },
    Platform { operation: String, message: String },
    DeviceLost { reason: String },
}

impl WebHostError {
    pub fn unsupported(feature: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Unsupported {
            feature: feature.into(),
            reason: reason.into(),
        }
    }

    pub fn platform(operation: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Platform {
            operation: operation.into(),
            message: message.into(),
        }
    }
}

impl Display for WebHostError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { feature, reason } => {
                write!(formatter, "unsupported browser feature {feature}: {reason}")
            }
            Self::CapabilityDenied { capability, reason } => {
                write!(
                    formatter,
                    "browser capability {capability} denied: {reason}"
                )
            }
            Self::LimitExceeded { resource, limit } => {
                write!(
                    formatter,
                    "browser resource {resource} exceeds limit {limit}"
                )
            }
            Self::InvalidInput { field, reason } => {
                write!(formatter, "invalid browser input {field}: {reason}")
            }
            Self::QueueOverflow { queue, capacity } => {
                write!(
                    formatter,
                    "browser queue {queue} exceeded capacity {capacity}"
                )
            }
            Self::Platform { operation, message } => {
                write!(formatter, "browser operation {operation} failed: {message}")
            }
            Self::DeviceLost { reason } => write!(formatter, "WebGPU device lost: {reason}"),
        }
    }
}

impl Error for WebHostError {}

#[cfg(target_arch = "wasm32")]
impl From<boon_web_effect_host::WebHostError> for WebHostError {
    fn from(error: boon_web_effect_host::WebHostError) -> Self {
        match error {
            boon_web_effect_host::WebHostError::LimitExceeded { resource, limit } => {
                Self::LimitExceeded { resource, limit }
            }
            boon_web_effect_host::WebHostError::InvalidInput { field, reason } => {
                Self::InvalidInput { field, reason }
            }
            boon_web_effect_host::WebHostError::QueueOverflow { queue, capacity } => {
                Self::QueueOverflow { queue, capacity }
            }
            boon_web_effect_host::WebHostError::Platform { operation, message } => {
                Self::Platform { operation, message }
            }
        }
    }
}
