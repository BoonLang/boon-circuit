//! Compact native window and WGPU surface host.
//!
//! This crate owns platform window/surface lifetime and ordered adaptation into
//! `boon_host`. Rendering, documents, runtime state, readback, and reports belong
//! to higher layers.

mod error;
mod event;
mod runner;
mod surface;

pub use error::{
    NativeHostError, SurfaceAcquireError, SurfacePresentError, SurfaceReconfigureReason,
};
pub use event::NativeEventCapabilities;
pub use runner::{NativeRoleError, NativeRoleResult, run_native_role_process};
pub use surface::{
    NativeHostIds, NativeSurfaceBinding, NativeSurfaceFrame, NativeSurfaceHost,
    NativeSurfaceLifecycle, NativeThreadContract, NativeThreadStrategy, NativeViewport,
    NativeWindowConfig, SurfacePreferences, SurfacePresentReceipt, WindowPosition,
    native_thread_contract,
};
