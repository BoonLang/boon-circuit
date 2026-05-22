use boon_ply_playground::run_app_from_args;
use ply_engine::prelude::*;

fn window_conf() -> macroquad::conf::Conf {
    let args = std::env::args().collect::<Vec<_>>();
    let role = args
        .windows(2)
        .find_map(|window| (window[0] == "--window-role").then(|| window[1].as_str()))
        .unwrap_or("preview");
    let legacy_single_window = args.iter().any(|arg| arg == "--single-window")
        || args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "--smoke-launch"
                    | "--verify-headed"
                    | "--verify-headed-focusless"
                    | "--verify-os-input-probe"
            )
        });
    let linux_backend = if legacy_single_window {
        match std::env::var("BOON_PLY_LINUX_BACKEND").as_deref() {
            Ok("x11") | Ok("X11") => miniquad::conf::LinuxBackend::X11Only,
            _ => miniquad::conf::LinuxBackend::WaylandOnly,
        }
    } else {
        miniquad::conf::LinuxBackend::WaylandOnly
    };
    let (title, wm_class, width, height) = match role {
        "dev" => (
            "Boon Circuit Dev Console",
            "boon-circuit-ply-dev-console",
            1500,
            1000,
        ),
        _ if legacy_single_window => (
            "Boon Circuit Ply Playground",
            "boon-circuit-ply-playground",
            1500,
            1000,
        ),
        _ => (
            "Boon Circuit Preview",
            "boon-circuit-ply-preview",
            1280,
            900,
        ),
    };
    macroquad::conf::Conf {
        miniquad_conf: miniquad::conf::Conf {
            window_title: title.to_owned(),
            window_width: width,
            window_height: height,
            high_dpi: false,
            sample_count: 1,
            platform: miniquad::conf::Platform {
                linux_backend,
                linux_wm_class: wm_class,
                ..Default::default()
            },
            ..Default::default()
        },
        draw_call_vertex_capacity: 200_000,
        draw_call_index_capacity: 200_000,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    if let Err(error) = run_app_from_args().await {
        eprintln!("boon_ply_playground: {error}");
        std::process::exit(1);
    }
}
