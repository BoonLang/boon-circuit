use std::error::Error;
use std::future::Future;

use crate::{NativeSurfaceHost, NativeWindowConfig};

pub type NativeRoleError = Box<dyn Error + Send + Sync + 'static>;
pub type NativeRoleResult = Result<(), NativeRoleError>;

/// Runs one native role process. The role future owns the only window/surface.
///
/// The pinned app_window revision does not expose its internal event-loop stop
/// operation. Once the role has dropped all of its state, this process-level
/// runner exits explicitly instead of leaving the app_window main loop parked.
pub fn run_native_role_process<F, Fut>(config: NativeWindowConfig, role: F) -> !
where
    F: FnOnce(NativeSurfaceHost) -> Fut + Send + 'static,
    Fut: Future<Output = NativeRoleResult> + Send + 'static,
{
    app_window::application::main(move || {
        let result: NativeRoleResult = futures::executor::block_on(async move {
            let host = NativeSurfaceHost::open(config)
                .await
                .map_err(|error| Box::new(error) as NativeRoleError)?;
            role(host).await
        });
        match result {
            Ok(()) => std::process::exit(0),
            Err(error) => {
                eprintln!("native role failed: {error}");
                std::process::exit(1);
            }
        }
    });
    panic!("app_window main loop returned before the native role exited")
}
