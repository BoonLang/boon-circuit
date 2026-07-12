mod catalog;
mod compile;
mod desktop;
mod dev;
mod dev_state;
mod frame;
mod language;
#[cfg(target_os = "linux")]
mod native_input;
mod observer;
mod preview;
mod proof;
mod protocol;
mod runtime_view;
mod ui;
mod verify;
mod view;
mod workspace;
#[cfg(target_os = "linux")]
mod workspace_control;

use std::path::PathBuf;

use boon_host::{LogicalSize, RoleId, SurfaceId, WindowId};
use boon_native_app_window::{
    NativeHostIds, NativeWindowConfig, WindowPosition, run_native_role_process,
};
use protocol::VERIFY_BOUNDED_WINDOWS_ENV;

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let role = argument(&args, "--role").unwrap_or("desktop");
    let result = match role {
        "desktop" => desktop::run(&args).map_err(|error| error.to_string()),
        "preview" => run_preview(&args),
        "dev" => run_dev(&args),
        "verify-v2" => verify::run(&args),
        #[cfg(target_os = "linux")]
        "native-input" => native_input::run_device_process(&args),
        #[cfg(target_os = "linux")]
        "workspace-control" => workspace_control::run_guard_process(&args),
        unknown => Err(format!("unknown native playground role `{unknown}`")),
    };
    if let Err(error) = result {
        eprintln!("boon_native_playground: {error}");
        std::process::exit(1);
    }
}

fn run_preview(args: &[String]) -> Result<(), String> {
    let path = required_path(args, "--connect")?;
    let connection = preview::connect(&path).map_err(|error| error.to_string())?;
    run_native_role_process(
        window_config(
            "preview",
            "Boon Preview",
            WindowPosition { x: 80.0, y: 70.0 },
            LogicalSize {
                width: 980.0,
                height: 760.0,
            },
        ),
        move |host| preview::run(host, connection),
    )
}

fn run_dev(args: &[String]) -> Result<(), String> {
    let path = required_path(args, "--connect")?;
    let connection = dev::connect(&path).map_err(|error| error.to_string())?;
    run_native_role_process(
        window_config(
            "dev",
            "Boon Dev",
            WindowPosition { x: 1100.0, y: 70.0 },
            LogicalSize {
                width: 1000.0,
                height: 720.0,
            },
        ),
        move |host| dev::run(host, connection),
    )
}

fn window_config(
    role: &str,
    title: &str,
    position: WindowPosition,
    initial_logical_size: LogicalSize,
) -> NativeWindowConfig {
    let bounded = std::env::var_os(VERIFY_BOUNDED_WINDOWS_ENV).is_some();
    let position = if bounded {
        WindowPosition { x: 12.0, y: 12.0 }
    } else {
        position
    };
    let initial_logical_size = if bounded {
        LogicalSize {
            width: 440.0,
            height: 680.0,
        }
    } else {
        initial_logical_size
    };
    NativeWindowConfig {
        ids: NativeHostIds {
            role: RoleId(role.to_owned()),
            window: WindowId(format!("{role}-window")),
            surface: SurfaceId(format!("{role}-surface")),
        },
        title: title.to_owned(),
        position,
        initial_logical_size,
    }
}

fn required_path(args: &[String], flag: &str) -> Result<PathBuf, String> {
    argument(args, flag)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{flag} requires a path"))
}

fn argument<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}
