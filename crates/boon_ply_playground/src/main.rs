use boon_ply_playground::run_app_from_args;
use ply_engine::prelude::*;

fn window_conf() -> macroquad::conf::Conf {
    let linux_backend = match std::env::var("BOON_PLY_LINUX_BACKEND").as_deref() {
        Ok("x11") | Ok("X11") => miniquad::conf::LinuxBackend::X11Only,
        _ => miniquad::conf::LinuxBackend::WaylandOnly,
    };
    macroquad::conf::Conf {
        miniquad_conf: miniquad::conf::Conf {
            window_title: "Boon Circuit Ply Playground".to_owned(),
            window_width: 1500,
            window_height: 1000,
            high_dpi: false,
            sample_count: 1,
            platform: miniquad::conf::Platform {
                linux_backend,
                linux_wm_class: "boon-circuit-ply-playground",
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
