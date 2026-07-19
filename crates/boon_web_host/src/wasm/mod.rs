//! Browser implementations. These adapters contain platform mechanics only;
//! application policy remains in the compiled Boon document program.

mod canvas;
mod client_effect_host;
mod clipboard;
mod distributed_session;
mod history;
mod input;
mod map_host;
mod network;
mod raster;
mod semantic_dom;
mod startup;
mod storage;

pub use canvas::*;
pub(crate) use client_effect_host::*;
pub use clipboard::*;
pub use distributed_session::*;
pub use history::*;
pub use input::*;
pub use map_host::*;
pub use network::*;
pub use raster::*;
pub use semantic_dom::*;
pub use startup::*;
pub use storage::*;

use crate::{WebHostError, WebHostResult};
use wasm_bindgen::JsValue;

pub(crate) fn window() -> WebHostResult<web_sys::Window> {
    web_sys::window().ok_or_else(|| WebHostError::unsupported("Window", "global Window is absent"))
}

pub(crate) fn js_error(operation: &str, error: impl Into<JsValue>) -> WebHostError {
    let error = error.into();
    WebHostError::platform(operation, js_message(&error))
}

pub(crate) fn js_message(value: &JsValue) -> String {
    value
        .as_string()
        .or_else(|| js_sys::JSON::stringify(value).ok()?.as_string())
        .unwrap_or_else(|| "browser rejected the operation".to_owned())
}
