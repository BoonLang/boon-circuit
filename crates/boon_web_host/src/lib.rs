//! Generic browser host contracts for Boon document programs.
//!
//! Product pixels are rendered by [`boon_native_gpu`]'s retained WGPU renderer,
//! which also targets browser WebGPU. The DOM boundary in this crate is limited
//! to a canvas, semantic accessibility nodes, and host-owned unsupported state.

mod capability;
#[cfg(any(target_arch = "wasm32", test))]
mod client_effect_host;
mod core;
mod distributed_session;
mod document_runtime;
mod error;
mod gpu;
mod input;
mod map_interaction;
mod map_tile;
mod scheduler;
mod semantic;
#[cfg(any(target_arch = "wasm32", test))]
mod sensitive_input;
mod startup;
mod storage;
mod support;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use capability::*;
pub use core::*;
pub use distributed_session::*;
pub use document_runtime::*;
pub use error::*;
pub use gpu::*;
pub use input::*;
pub use map_interaction::*;
pub use map_tile::*;
pub use scheduler::*;
pub use semantic::*;
pub use startup::*;
pub use storage::*;
pub use support::*;
