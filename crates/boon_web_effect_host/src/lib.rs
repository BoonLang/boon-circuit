//! Browser-owned adapters for bounded File and Content effects.
//!
//! This crate deliberately excludes document rendering and WGPU so host-effect
//! lifecycle tests do not link or load the browser renderer.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[cfg(target_arch = "wasm32")]
mod content_store;
#[cfg(target_arch = "wasm32")]
mod file_effect_host;

#[cfg(target_arch = "wasm32")]
pub use file_effect_host::{
    BrowserFileEffectHost, BrowserFileEffectLimits, BrowserFileEffectNotification,
    BrowserFileEffectOperation,
};

pub type WebHostResult<T> = Result<T, WebHostError>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebHostError {
    LimitExceeded { resource: String, limit: usize },
    InvalidInput { field: String, reason: String },
    QueueOverflow { queue: String, capacity: usize },
    Platform { operation: String, message: String },
}

impl Display for WebHostError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
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
        }
    }
}

impl Error for WebHostError {}

#[cfg(target_arch = "wasm32")]
fn js_message(value: &wasm_bindgen::JsValue) -> String {
    value
        .as_string()
        .or_else(|| js_sys::JSON::stringify(value).ok()?.as_string())
        .unwrap_or_else(|| "browser rejected the operation".to_owned())
}
