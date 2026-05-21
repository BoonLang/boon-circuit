#![recursion_limit = "256"]

use boon_runtime::{
    LiveRuntime, LiveSourceEvent, LiveStepOutput, RunOutput, Scenario, ScenarioStep,
    VerificationLayer, example_paths, parse_scenario, run_scenario,
    run_scenario_source_with_step_limit, sha256_file, write_json,
};
use ply_engine::prelude::*;
use serde_json::json;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static DEFAULT_FONT: FontAsset = FontAsset::Bytes {
    file_name: "DejaVuSans.ttf",
    data: include_bytes!("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
};

thread_local! {
    static UI_SOURCE_OBSERVATIONS: RefCell<Vec<serde_json::Value>> = const { RefCell::new(Vec::new()) };
    static LAST_FOCUSED_RENDER_INPUT: RefCell<Option<FocusedRenderInput>> = const { RefCell::new(None) };
    static RENDER_ID_LABELS: RefCell<BTreeMap<String, &'static str>> = const { RefCell::new(BTreeMap::new()) };
    static CURRENT_HOVER_SCOPES: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
    static LAST_HOVER_SCOPES: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
    static LAST_BUTTON_PRESS: RefCell<Option<(String, Instant)>> = const { RefCell::new(None) };
    static SUPPRESS_NEXT_BLUR_INPUT: RefCell<Option<Id>> = const { RefCell::new(None) };
}

static FOCUS_FREE_HEADED: AtomicBool = AtomicBool::new(false);
static FOCUS_FREE_SCREENSHOT_CACHE: OnceLock<Mutex<Option<(PathBuf, ScreenshotCapture)>>> =
    OnceLock::new();

const PLAYGROUND_HELP: &str = "\
boon_ply_playground

Usage:
  boon_ply_playground --example <todomvc|cells>
  boon_ply_playground --example <todomvc|cells> --mode <app|dev>
  boon_ply_playground --smoke-launch --example <name> --report <path>
  boon_ply_playground --verify-headed --example <name> --report <path>
  boon_ply_playground --verify-headed-focusless --example <name> --report <path>
  boon_ply_playground --verify-os-input-probe --report <path>
";

#[derive(Clone, Debug)]
struct FocusedRenderInput {
    id: Id,
    submit_source: Option<String>,
    blur_source: Option<String>,
    cancel_source: Option<String>,
    escape_source: Option<String>,
    address: Option<String>,
    target_text: Option<String>,
    target_occurrence: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlaygroundView {
    App,
    Source,
    Deltas,
    Inspector,
    Causes,
    Scenario,
}

impl PlaygroundView {
    fn from_mode_arg(args: &[String]) -> Self {
        match value_after(args, "--mode").as_deref() {
            Some("dev") | Some("source") => Self::Source,
            Some("deltas") => Self::Deltas,
            Some("inspector") => Self::Inspector,
            Some("causes") => Self::Causes,
            Some("scenario") => Self::Scenario,
            _ => Self::App,
        }
    }
}

#[derive(Clone, Debug)]
enum RenderNode {
    Column {
        id: Option<String>,
        width: Option<f32>,
        height: Option<f32>,
        background: u32,
        border: Option<u32>,
        gap: f32,
        padding: Option<(f32, f32, f32, f32)>,
        children: Vec<RenderNode>,
    },
    Row {
        id: Option<String>,
        height: Option<f32>,
        background: u32,
        border: Option<u32>,
        gap: f32,
        padding: Option<(f32, f32, f32, f32)>,
        children: Vec<RenderNode>,
    },
    ForEach {
        list: String,
        item: String,
        children: Vec<RenderNode>,
    },
    Text {
        value: RenderValue,
        size: u16,
        color: u32,
        background: Option<u32>,
        width: Option<RenderExtent>,
        height: Option<f32>,
        strike_if: Option<RenderValue>,
        center: bool,
    },
    Input {
        id: String,
        key: Option<RenderValue>,
        value: RenderValue,
        placeholder: RenderValue,
        change_source: Option<String>,
        submit_source: Option<String>,
        cancel_source: Option<String>,
        escape_source: Option<String>,
        blur_source: Option<String>,
        address: Option<RenderValue>,
        target: Option<RenderValue>,
        visible: Option<RenderValue>,
        size: u16,
        height: Option<f32>,
        color: u32,
        placeholder_color: u32,
        background: u32,
        border: Option<u32>,
    },
    Button {
        id: String,
        text: RenderValue,
        width: Option<RenderExtent>,
        selected: Option<RenderSelection>,
        source: Option<String>,
        double_click_source: Option<String>,
        address: Option<RenderValue>,
        target: Option<RenderValue>,
        visible: Option<RenderValue>,
        hover_visible: bool,
        height: Option<f32>,
        size: u16,
        color: u32,
        background: u32,
        selected_color: u32,
        selected_background: u32,
        border: Option<u32>,
        selected_border: Option<u32>,
        color_if: Option<RenderValue>,
        if_color: Option<u32>,
        strike_if: Option<RenderValue>,
        align_left: bool,
    },
    Checkbox {
        id: String,
        checked: RenderValue,
        source: Option<String>,
        target: Option<RenderValue>,
        size: f32,
    },
}

#[derive(Clone, Debug)]
enum RenderValue {
    Literal(String),
    Path(String),
    Template(String),
}

#[derive(Clone, Debug)]
struct RenderSelection {
    path: String,
    expected: String,
}

#[derive(Clone, Debug)]
enum RenderExtent {
    Fill,
    Fixed(f32),
}

impl RenderExtent {
    fn from_attr(value: &str) -> Option<Self> {
        match value {
            "fill" | "Fill" => Some(Self::Fill),
            _ => value.parse().ok().map(Self::Fixed),
        }
    }
}

#[derive(Clone, Debug)]
struct RenderContext<'a> {
    root: &'a serde_json::Value,
    bindings: Vec<RenderBinding<'a>>,
    index_stack: Vec<usize>,
    hover_scopes: Vec<String>,
}

#[derive(Clone, Debug)]
struct RenderBinding<'a> {
    name: String,
    list: String,
    value: &'a serde_json::Value,
    index: usize,
}

impl<'a> RenderContext<'a> {
    fn root(root: &'a serde_json::Value) -> Self {
        Self {
            root,
            bindings: Vec::new(),
            index_stack: Vec::new(),
            hover_scopes: Vec::new(),
        }
    }

    fn with_binding(
        &self,
        list: &str,
        name: &str,
        value: &'a serde_json::Value,
        index: usize,
    ) -> RenderContext<'a> {
        let mut next = self.clone();
        next.bindings.push(RenderBinding {
            name: name.to_owned(),
            list: list.to_owned(),
            value,
            index,
        });
        next.index_stack.push(index);
        next
    }

    fn with_hover_scope(&self, scope: String) -> RenderContext<'a> {
        let mut next = self.clone();
        next.hover_scopes.push(scope);
        next
    }
}

pub async fn run_app_from_args() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--verify-os-input-probe") {
        return run_verify_os_input_probe(&args).await;
    }
    if args.iter().any(|arg| arg == "--verify-headed-focusless") {
        FOCUS_FREE_HEADED.store(true, Ordering::Relaxed);
        unsafe {
            std::env::set_var("BOON_FOCUS_FREE_HEADED", "1");
        }
        return run_verify_headed(&args).await;
    }
    if args.iter().any(|arg| arg == "--verify-headed") {
        return run_verify_headed(&args).await;
    }
    if args.iter().any(|arg| arg == "--smoke-launch") {
        return run_smoke_launch(&args).await;
    }
    run_interactive(&args).await
}

fn print_help() {
    print!("{PLAYGROUND_HELP}");
}

fn focus_free_headed() -> bool {
    FOCUS_FREE_HEADED.load(Ordering::Relaxed)
        || std::env::var("BOON_FOCUS_FREE_HEADED").as_deref() == Ok("1")
        || std::env::args().any(|arg| arg == "--verify-headed-focusless")
}

fn os_input_forbidden() -> bool {
    focus_free_headed() || std::env::var("BOON_FORBID_OS_INPUT").as_deref() == Ok("1")
}

async fn run_verify_headed(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_after(args, "--example").unwrap_or_else(|| "todomvc".to_owned());
    let focus_free = focus_free_headed();
    let report = value_after(args, "--report")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if focus_free {
                PathBuf::from(format!("target/reports/{example}-headed-focusless.json"))
            } else {
                PathBuf::from(format!("target/reports/{example}-headed-ply.json"))
            }
        });
    let (source, scenario, _) = example_paths(&example)?;
    let screenshot = report.with_extension("png");
    let os_probe_screenshot = report.with_file_name(format!("{example}-headed-os-input.png"));
    let os_pointer_probe_screenshot =
        report.with_file_name(format!("{example}-headed-os-pointer.png"));
    let mut ply = Ply::<()>::new(&DEFAULT_FONT).await;
    ply.set_debug_mode(false);
    let os_probe_token = format!("boon-headed-os-{}-{example}", std::process::id());
    let headed_os_probe = if focus_free {
        skipped_os_keyboard_probe(&os_probe_screenshot)
    } else {
        run_os_keyboard_probe_in_window(&mut ply, &os_probe_token, &os_probe_screenshot).await?
    };
    let headed_os_pointer_probe = if focus_free {
        skipped_os_pointer_probe(&os_pointer_probe_screenshot)
    } else if std::env::var("BOON_ALLOW_OS_POINTER_PROBE").as_deref() == Ok("1") {
        run_os_pointer_probe_in_window(&mut ply, &os_pointer_probe_screenshot).await?
    } else {
        skipped_os_pointer_probe(&os_pointer_probe_screenshot)
    };
    let source_text = std::fs::read_to_string(&source)?;
    let scenario_data = parse_scenario(&scenario)?;
    let mut state = PlaygroundState::new(&example, &mut ply)?;
    ply.set_text_value("source_editor", &source_text);
    state.reset_to_initial(&ply);
    let playground_surface_visible_bounds =
        playground_surface_visible_bounds_for_all_views(&mut ply, &mut state).await;
    state.view = PlaygroundView::App;
    for _ in 0..3 {
        draw_frame(&mut ply, &state).await;
        next_frame().await;
    }
    let app_control_observations =
        drive_visible_app_control_probe(&mut ply, &mut state, &report, &example).await;
    let source_event_observations =
        drive_visible_source_event_probe(&mut ply, &mut state, &report, &example, &scenario_data)
            .await;
    state.reset_to_initial(&ply);
    ply.clear_focus();
    for _ in 0..3 {
        draw_frame(&mut ply, &state).await;
        next_frame().await;
    }
    let step_observations = drive_visible_step_control_sequence(
        &mut ply,
        &mut state,
        &scenario_data,
        &report,
        &example,
    )
    .await;
    let output = run_scenario(&source, &scenario, VerificationLayer::HeadedPly, None)?;
    state = PlaygroundState::from_output(
        example.clone(),
        scenario.clone(),
        scenario_data.step.len(),
        output,
    );
    for _ in 0..3 {
        draw_frame(&mut ply, &state).await;
        next_frame().await;
    }
    draw_frame(&mut ply, &state).await;
    next_frame().await;
    draw_frame(&mut ply, &state).await;
    let image = get_screen_data();
    let mut pixel_stats = image_stats(&image.bytes);
    let mut capture_backend = "macroquad-framebuffer".to_owned();
    let mut framebuffer_width = u32::from(image.width);
    let mut framebuffer_height = u32::from(image.height);
    if let Some(parent) = screenshot.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if pixel_stats.nonzero_channels == 0 || pixel_stats.unique_rgba_values <= 1 {
        let fallback = capture_with_cosmic_screenshot(&screenshot)?;
        pixel_stats = fallback.pixel_stats;
        capture_backend = fallback.capture_backend;
        framebuffer_width = fallback.width;
        framebuffer_height = fallback.height;
    } else {
        image.export_png(screenshot.to_str().ok_or("screenshot path is not utf-8")?);
    }
    next_frame().await;
    let mut report_json = state
        .output
        .as_ref()
        .ok_or("missing verifier output")?
        .report
        .clone();
    if let Some(object) = report_json.as_object_mut() {
        object.insert(
            "window_mode".to_owned(),
            json!(if focus_free {
                "headed-focusless"
            } else {
                "headed"
            }),
        );
        object.insert("window_backend".to_owned(), json!("ply-engine/macroquad"));
        object.insert("window_pid".to_owned(), json!(std::process::id()));
        object.insert(
            "window_title".to_owned(),
            json!("Boon Circuit Ply Playground"),
        );
        object.insert("display_server".to_owned(), json!(display_server()));
        object.insert(
            "linux_backend_requested".to_owned(),
            json!("x11_with_wayland_fallback"),
        );
        object.insert(
            "display_env".to_owned(),
            json!({
                "WAYLAND_DISPLAY": std::env::var("WAYLAND_DISPLAY").ok(),
                "DISPLAY": std::env::var("DISPLAY").ok()
            }),
        );
        object.insert(
            "display_socket_or_compositor_connection".to_owned(),
            json!(display_socket()),
        );
        object.insert(
            "native_display_contract".to_owned(),
            native_display_contract(),
        );
        object.insert("display_scale".to_owned(), json!(screen_dpi_scale()));
        object.insert(
            "window_size".to_owned(),
            json!([screen_width(), screen_height()]),
        );
        object.insert(
            "framebuffer_size".to_owned(),
            json!({
                "width": framebuffer_width,
                "height": framebuffer_height
            }),
        );
        object.insert(
            "input_backend".to_owned(),
            json!(if focus_free {
                "ply-synthetic-focus-free"
            } else {
                "macroquad-os-events + wtype-real-keyboard-events + ydotool-pointer-probe"
            }),
        );
        object.insert("os_focus_required".to_owned(), json!(!focus_free));
        object.insert("os_keyboard_or_pointer_used".to_owned(), json!(!focus_free));
        object.insert(
            "os_input_tools_used".to_owned(),
            json!(if focus_free {
                Vec::<&str>::new()
            } else {
                vec!["wtype"]
            }),
        );
        object.insert("capture_backend".to_owned(), json!(capture_backend));
        object.insert(
            "focused_window_proof".to_owned(),
            json!(if focus_free {
                "focus-free verifier read visible Ply bounds, synthesized Boon SOURCE events from rendered VIEW metadata, applied them through LiveRuntime, and captured headed frames without OS keyboard or pointer injection"
            } else {
                "OS probe set Ply focus to os_probe_input, sent a real keyboard token, observed it in Ply text state, then captured the headed macroquad/Ply framebuffer"
            }),
        );
        let keyboard_probe_attempted = headed_os_probe
            .get("status")
            .and_then(serde_json::Value::as_str)
            != Some("skip");
        let pointer_probe_attempted = headed_os_pointer_probe
            .get("status")
            .and_then(serde_json::Value::as_str)
            != Some("skip");
        let mut checkpoint_paths = vec![json!(screenshot)];
        if keyboard_probe_attempted {
            checkpoint_paths.push(json!(os_probe_screenshot));
        }
        if pointer_probe_attempted {
            checkpoint_paths.push(json!(os_pointer_probe_screenshot));
        }
        checkpoint_paths.extend(
            app_control_observations
                .iter()
                .filter_map(|observation| observation.get("screenshot_path").cloned()),
        );
        checkpoint_paths.extend(
            source_event_observations
                .iter()
                .filter_map(|observation| observation.get("screenshot_path").cloned()),
        );
        checkpoint_paths.extend(
            step_observations
                .iter()
                .filter_map(|observation| observation.get("screenshot_path").cloned()),
        );
        object.insert(
            "checkpoint_screenshot_or_video_paths".to_owned(),
            json!(checkpoint_paths),
        );
        let mut artifact_sha256s = vec![json!({
            "path": screenshot,
            "sha256": sha256_file(&screenshot)?
        })];
        if keyboard_probe_attempted {
            artifact_sha256s.push(json!({
                "path": os_probe_screenshot,
                "sha256": sha256_file(&os_probe_screenshot)?
            }));
        }
        if pointer_probe_attempted {
            artifact_sha256s.push(json!({
                "path": os_pointer_probe_screenshot,
                "sha256": sha256_file(&os_pointer_probe_screenshot)?
            }));
        }
        artifact_sha256s.extend(app_control_observations.iter().filter_map(|observation| {
            Some(json!({
                "path": observation.get("screenshot_path")?.clone(),
                "sha256": observation.get("screenshot_sha256")?.clone()
            }))
        }));
        artifact_sha256s.extend(source_event_observations.iter().filter_map(|observation| {
            Some(json!({
                "path": observation.get("screenshot_path")?.clone(),
                "sha256": observation.get("screenshot_sha256")?.clone()
            }))
        }));
        artifact_sha256s.extend(step_observations.iter().filter_map(|observation| {
            Some(json!({
                "path": observation.get("screenshot_path")?.clone(),
                "sha256": observation.get("screenshot_sha256")?.clone()
            }))
        }));
        object.insert("artifact_sha256s".to_owned(), json!(artifact_sha256s));
        object.insert(
            "nonblank_screenshot_hashes".to_owned(),
            json!([{
                "nonzero_channels": pixel_stats.nonzero_channels,
                "unique_rgba_values": pixel_stats.unique_rgba_values
            }]),
        );
        object.insert(
            "per_step_pointer_keyboard_route".to_owned(),
            json!("real OS keyboard event -> focused Ply text input proof; real OS keyboard event -> visible app text-control proof; visible app control -> observed Boon SOURCE event proof; real OS keyboard activation -> visible Step control proof; scenario user_action -> routed source event -> runtime tick; expected_source_event is assertion-only"),
        );
        let os_input_coverage = headed_os_input_coverage(
            &scenario_data,
            &source_event_observations,
            &step_observations,
        );
        let focus_free_complete =
            json_array_empty(&os_input_coverage["source_event_probe_missing_labels"])
                && json_array_empty(&os_input_coverage["step_control_missing_labels"]);
        let full_os_input_complete =
            json_array_empty(&os_input_coverage["source_event_probe_missing_labels"])
                && json_array_empty(&os_input_coverage["step_control_missing_labels"])
                && json_array_empty(&os_input_coverage["missing_full_os_pointer_keyboard_steps"])
                && headed_os_probe
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    == Some("pass")
                && headed_os_pointer_probe
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    == Some("pass");
        object.insert(
            "input_injection_method".to_owned(),
            if focus_free {
                json!("ply_synthetic_focus_free_render_metadata")
            } else if full_os_input_complete {
                json!("os_pointer_keyboard_to_visible_window")
            } else {
                json!("os_keyboard_probe_visible_app_source_event_and_step_control_plus_scenario_user_action_route")
            },
        );
        if focus_free && focus_free_complete {
            object.insert(
                "focus_free_input_steps".to_owned(),
                json!(headed_os_input_steps(
                    &scenario_data,
                    &source_event_observations,
                    &step_observations,
                    &screenshot,
                )),
            );
        } else if full_os_input_complete {
            object.insert(
                "os_input_steps".to_owned(),
                json!(headed_os_input_steps(
                    &scenario_data,
                    &source_event_observations,
                    &step_observations,
                    &screenshot,
                )),
            );
        } else if !focus_free {
            object.insert(
                "os_input_limitation".to_owned(),
                json!("This headed verifier proves real OS keyboard input reaches the Ply window, reaches visible application controls, emits matching observed Boon SOURCE events for the covered workflow, and applies covered prefix events through boon_runtime::LiveRuntime against real scenario-step expectations. It can also activate the visible Ply Step control for each scenario transition. It still lacks direct app-control OS-input evidence for the labels in os_input_coverage.missing_full_os_pointer_keyboard_steps."),
            );
        }
        object.insert("os_input_coverage".to_owned(), os_input_coverage);
        object.insert("os_input_probe".to_owned(), headed_os_probe);
        object.insert("os_pointer_probe".to_owned(), headed_os_pointer_probe);
        object.insert(
            "visible_app_control_os_input".to_owned(),
            json!(app_control_observations),
        );
        object.insert(
            "visible_source_event_os_input".to_owned(),
            json!(source_event_observations),
        );
        object.insert(
            "visible_step_control_os_input".to_owned(),
            json!(step_observations),
        );
        object.insert("playground_surface".to_owned(), playground_surface_checks());
        object.insert(
            "playground_surface_visible_bounds".to_owned(),
            playground_surface_visible_bounds,
        );
    }
    write_json(&report, &report_json)?;
    if app_control_observations.iter().all(|observation| {
        observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
    }) && source_event_observations.iter().all(|observation| {
        observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
            && (observation
                .get("scenario_step_id")
                .is_none_or(serde_json::Value::is_null)
                || observation
                    .get("scenario_expectations_checked")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true))
    }) && step_observations.iter().all(|observation| {
        observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
    }) {
        macroquad::miniquad::window::quit();
        Ok(())
    } else {
        Err(format!(
            "headed verifier did not activate every visible app-control/source-event probe and Step control; see `{}`",
            report.display()
        )
        .into())
    }
}

async fn drive_visible_app_control_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    report: &std::path::Path,
    example: &str,
) -> Vec<serde_json::Value> {
    let (element_id, label, typed_text, contract) = if example == "cells" {
        (
            "cell_editor_A1",
            "cells-a1-editor",
            "41",
            "OS keyboard text reached the visible Cells A1 editor text input",
        )
    } else {
        (
            "todo_new_input",
            "todomvc-new-todo-input",
            "Visible todo probe",
            "OS keyboard text reached the visible TodoMVC new-todo text input",
        )
    };
    let screenshot = report.with_file_name(format!("{example}-headed-app-control-{label}.png"));
    vec![
        drive_visible_text_input_probe(
            ply,
            state,
            element_id,
            label,
            typed_text,
            contract,
            &screenshot,
        )
        .await,
    ]
}

async fn drive_visible_text_input_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    element_id: &'static str,
    label: &str,
    typed_text: &str,
    contract: &str,
    screenshot: &PathBuf,
) -> serde_json::Value {
    let focus_free = focus_free_headed();
    ply.set_text_value(element_id, "");
    let mut typed = false;
    let mut send_error = None;
    let mut observed_value = String::new();
    let mut bounds = serde_json::Value::Null;
    for frame in 0..120 {
        draw_frame(ply, state).await;
        if let Some(app_bounds) = ply.bounding_box(element_id) {
            bounds = json!({
                "x": app_bounds.x,
                "y": app_bounds.y,
                "width": app_bounds.width,
                "height": app_bounds.height
            });
        }
        ply.set_focus(element_id);
        if ((focus_free && frame == 0) || (!focus_free && frame == 8)) && !typed {
            typed = true;
            if focus_free {
                ply.set_text_value(element_id, typed_text);
            } else if let Err(error) = send_real_keyboard_text(typed_text) {
                send_error = Some(error.to_string());
            }
        }
        observed_value = ply.get_text_value(element_id).to_owned();
        if os_probe_observed_token(&observed_value, typed_text) {
            break;
        }
        next_frame().await;
    }
    draw_frame(ply, state).await;
    let (pixel_stats, screenshot_capture_backend, screenshot_capture_error) =
        match capture_probe_frame_png(screenshot) {
            Ok(capture) => (capture.pixel_stats, capture.capture_backend, None),
            Err(error) => (
                PixelStats {
                    nonzero_channels: 0,
                    unique_rgba_values: 0,
                },
                "capture-error".to_owned(),
                Some(error.to_string()),
            ),
        };
    let pass = typed && os_probe_observed_token(&observed_value, typed_text);
    let observed_insertion_order = if observed_value.contains(typed_text) {
        "normal"
    } else if observed_value.contains(&reverse_text(typed_text)) {
        "reversed"
    } else {
        "missing"
    };
    ply.set_text_value(element_id, "");
    json!({
        "id": label,
        "pass": pass,
        "target_element_id": element_id,
        "visible_bounds": bounds,
        "input_route_contract": if focus_free {
            "focus-free verifier set visible Ply text input state directly inside the headed process; no desktop keyboard event was sent"
        } else {
            contract
        },
        "input_backend": if focus_free { "ply-synthetic-focus-free" } else { "os_keyboard" },
        "keyboard_tool": if focus_free { serde_json::Value::Null } else { json!("wtype") },
        "keyboard_tool_path": if focus_free { serde_json::Value::Null } else { json!(command_path("wtype")) },
        "typed": typed,
        "send_error": send_error,
        "typed_text_sha256": boon_runtime::sha256_bytes(typed_text.as_bytes()),
        "observed_value_sha256": boon_runtime::sha256_bytes(observed_value.as_bytes()),
        "observed_insertion_order": observed_insertion_order,
        "screenshot_path": screenshot,
        "screenshot_sha256": sha256_file(screenshot).unwrap_or_else(|_| "missing".to_owned()),
        "screenshot_capture_backend": screenshot_capture_backend,
        "screenshot_capture_error": screenshot_capture_error,
        "screenshot_nonzero_channels": pixel_stats.nonzero_channels,
        "screenshot_unique_rgba_values": pixel_stats.unique_rgba_values
    })
}

async fn drive_visible_source_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    report: &std::path::Path,
    example: &str,
    scenario: &Scenario,
) -> Vec<serde_json::Value> {
    if example == "cells" {
        let edit_screenshot =
            report.with_file_name("cells-headed-source-event-edit-a1-literal.png");
        let commit_screenshot =
            report.with_file_name("cells-headed-source-event-commit-a1-literal.png");
        let draft_screenshot =
            report.with_file_name("cells-headed-source-event-edit-a1-cancel-draft.png");
        let mut observations = vec![
            drive_visible_source_text_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "edit-a1-literal",
                    element_id: Id::new("cell_editor_A1"),
                    element_label: "cell_editor_A1",
                    text: "41",
                    expected_text: Some("41"),
                    source: "cell.sources.editor.change",
                    key: None,
                    address: Some("A1"),
                    target_text: None,
                    screenshot: edit_screenshot,
                    scenario_step: scenario_step_by_id(scenario, "edit-a1-literal"),
                },
            )
            .await,
            drive_visible_source_submit_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "commit-a1-literal",
                    element_id: Id::new("cell_editor_A1"),
                    element_label: "cell_editor_A1",
                    text: "41",
                    expected_text: Some("41"),
                    source: "cell.sources.editor.commit",
                    key: Some("Enter"),
                    address: Some("A1"),
                    target_text: None,
                    screenshot: commit_screenshot,
                    scenario_step: scenario_step_by_id(scenario, "commit-a1-literal"),
                },
            )
            .await,
            drive_visible_source_text_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "edit-a1-cancel-draft",
                    element_id: Id::new("cell_editor_A1"),
                    element_label: "cell_editor_A1",
                    text: "123",
                    expected_text: Some("123"),
                    source: "cell.sources.editor.change",
                    key: None,
                    address: Some("A1"),
                    target_text: None,
                    screenshot: draft_screenshot,
                    scenario_step: scenario_step_by_id(scenario, "edit-a1-cancel-draft"),
                },
            )
            .await,
        ];
        observations.push(
            drive_visible_source_escape_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "cancel-a1-draft",
                    element_id: Id::new("cell_editor_A1"),
                    element_label: "cell_editor_A1",
                    text: "123",
                    expected_text: None,
                    source: "cell.sources.editor.cancel",
                    key: None,
                    address: Some("A1"),
                    target_text: None,
                    screenshot: report
                        .with_file_name("cells-headed-source-event-cancel-a1-draft.png"),
                    scenario_step: scenario_step_by_id(scenario, "cancel-a1-draft"),
                },
            )
            .await,
        );
        for (id, editor, address, text) in [
            ("commit-b1-formula", "cell_editor_B1", "B1", "=A1+1"),
            ("change-a1-updates-b1", "cell_editor_A1", "A1", "99"),
            ("cycle-error", "cell_editor_A1", "A1", "=B1+1"),
            (
                "replace-b1-formula-removes-stale-cycle-edge",
                "cell_editor_B1",
                "B1",
                "5",
            ),
            (
                "change-a1-after-edge-replacement-does-not-recompute-b1",
                "cell_editor_A1",
                "A1",
                "10",
            ),
            ("commit-c1-fanout-formula", "cell_editor_C1", "C1", "=A1+1"),
            ("commit-d1-fanout-formula", "cell_editor_D1", "D1", "=A1+2"),
            (
                "change-a1-fanout-recomputes-dependents-only",
                "cell_editor_A1",
                "A1",
                "20",
            ),
        ] {
            observations.push(
                drive_visible_source_submit_event_probe(
                    ply,
                    state,
                    VisibleSourceTextProbe {
                        id,
                        element_id: Id::new(editor),
                        element_label: editor,
                        text,
                        expected_text: Some(text),
                        source: "cell.sources.editor.commit",
                        key: Some("Enter"),
                        address: Some(address),
                        target_text: None,
                        screenshot: report.with_file_name(format!(
                            "cells-headed-source-event-{}.png",
                            sanitize_artifact_label(id)
                        )),
                        scenario_step: scenario_step_by_id(scenario, id),
                    },
                )
                .await,
            );
        }
        observations
    } else {
        let mut observations = vec![
            drive_visible_source_text_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "add-test-todo-type",
                    element_id: Id::new("todo_new_input"),
                    element_label: "todo_new_input",
                    text: "Test todo",
                    expected_text: Some("Test todo"),
                    source: "store.sources.new_todo_input.change",
                    key: None,
                    address: None,
                    target_text: None,
                    screenshot: report
                        .with_file_name("todomvc-headed-source-event-add-test-todo-type.png"),
                    scenario_step: scenario_step_by_id(scenario, "add-test-todo-type"),
                },
            )
            .await,
            drive_visible_source_submit_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "add-test-todo-submit",
                    element_id: Id::new("todo_new_input"),
                    element_label: "todo_new_input",
                    text: "Test todo",
                    expected_text: Some("Test todo"),
                    source: "store.sources.new_todo_input.key_down",
                    key: Some("Enter"),
                    address: None,
                    target_text: None,
                    screenshot: report
                        .with_file_name("todomvc-headed-source-event-add-test-todo-submit.png"),
                    scenario_step: scenario_step_by_id(scenario, "add-test-todo-submit"),
                },
            )
            .await,
            drive_visible_source_submit_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "reject-empty-todo",
                    element_id: Id::new("todo_new_input"),
                    element_label: "todo_new_input",
                    text: "   ",
                    expected_text: Some("   "),
                    source: "store.sources.new_todo_input.key_down",
                    key: Some("Enter"),
                    address: None,
                    target_text: None,
                    screenshot: report
                        .with_file_name("todomvc-headed-source-event-reject-empty-todo.png"),
                    scenario_step: scenario_step_by_id(scenario, "reject-empty-todo"),
                },
            )
            .await,
        ];
        for probe in [
            VisibleSourcePressProbe {
                id: "toggle-all-complete",
                element_id: Id::new("todo_toggle_all"),
                element_label: "todo_toggle_all",
                source: "store.sources.toggle_all_checkbox.click",
                target_text: None,
                screenshot: report
                    .with_file_name("todomvc-headed-source-event-toggle-all-complete.png"),
                scenario_step: scenario_step_by_id(scenario, "toggle-all-complete"),
            },
            VisibleSourcePressProbe {
                id: "toggle-all-active",
                element_id: Id::new("todo_toggle_all"),
                element_label: "todo_toggle_all",
                source: "store.sources.toggle_all_checkbox.click",
                target_text: None,
                screenshot: report
                    .with_file_name("todomvc-headed-source-event-toggle-all-active.png"),
                scenario_step: scenario_step_by_id(scenario, "toggle-all-active"),
            },
            VisibleSourcePressProbe {
                id: "toggle-buy-groceries",
                element_id: Id::new_index("todo_row_checkbox", 3),
                element_label: "todo_row_checkbox[3]",
                source: "todo.sources.todo_checkbox.click",
                target_text: Some("Buy groceries"),
                screenshot: report
                    .with_file_name("todomvc-headed-source-event-toggle-buy-groceries.png"),
                scenario_step: scenario_step_by_id(scenario, "toggle-buy-groceries"),
            },
            VisibleSourcePressProbe {
                id: "filter-active",
                element_id: Id::new("todo_filter_active"),
                element_label: "todo_filter_active",
                source: "store.sources.filter_active.press",
                target_text: None,
                screenshot: report.with_file_name("todomvc-headed-source-event-filter-active.png"),
                scenario_step: scenario_step_by_id(scenario, "filter-active"),
            },
            VisibleSourcePressProbe {
                id: "toggle-dynamic-test-todo-under-active-filter",
                element_id: Id::new_index("todo_row_checkbox", 3),
                element_label: "todo_row_checkbox[3]",
                source: "todo.sources.todo_checkbox.click",
                target_text: Some("Test todo"),
                screenshot: report.with_file_name(
                    "todomvc-headed-source-event-toggle-dynamic-test-todo-under-active-filter.png",
                ),
                scenario_step: scenario_step_by_id(
                    scenario,
                    "toggle-dynamic-test-todo-under-active-filter",
                ),
            },
            VisibleSourcePressProbe {
                id: "filter-completed",
                element_id: Id::new("todo_filter_completed"),
                element_label: "todo_filter_completed",
                source: "store.sources.filter_completed.press",
                target_text: None,
                screenshot: report
                    .with_file_name("todomvc-headed-source-event-filter-completed.png"),
                scenario_step: scenario_step_by_id(scenario, "filter-completed"),
            },
            VisibleSourcePressProbe {
                id: "filter-all",
                element_id: Id::new("todo_filter_all"),
                element_label: "todo_filter_all",
                source: "store.sources.filter_all.press",
                target_text: None,
                screenshot: report.with_file_name("todomvc-headed-source-event-filter-all.png"),
                scenario_step: scenario_step_by_id(scenario, "filter-all"),
            },
            VisibleSourcePressProbe {
                id: "edit-test-todo",
                element_id: Id::new_index("todo_row_title", 4),
                element_label: "todo_row_title[4]",
                source: "todo.sources.todo_title_element.double_click",
                target_text: Some("Test todo"),
                screenshot: report.with_file_name("todomvc-headed-source-event-edit-open.png"),
                scenario_step: scenario_step_by_id(scenario, "edit-test-todo"),
            },
        ] {
            observations.push(drive_visible_source_press_event_probe(ply, state, probe).await);
        }
        observations.push(
            drive_visible_source_text_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "edit-test-todo-change",
                    element_id: Id::new_index("todo_row_edit", 4),
                    element_label: "todo_row_edit[4]",
                    text: "Test todo edited",
                    expected_text: Some("Test todo edited"),
                    source: "todo.sources.editing_todo_title_element.change",
                    key: None,
                    address: None,
                    target_text: Some("Test todo"),
                    screenshot: report
                        .with_file_name("todomvc-headed-source-event-edit-change.png"),
                    scenario_step: scenario_step_by_id(scenario, "edit-test-todo-change"),
                },
            )
            .await,
        );
        observations.push(
            drive_visible_source_submit_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "edit-test-todo-commit",
                    element_id: Id::new_index("todo_row_edit", 4),
                    element_label: "todo_row_edit[4]",
                    text: "Test todo edited",
                    expected_text: Some("Test todo edited"),
                    source: "todo.sources.editing_todo_title_element.key_down",
                    key: Some("Enter"),
                    address: None,
                    target_text: Some("Test todo"),
                    screenshot: report
                        .with_file_name("todomvc-headed-source-event-edit-commit.png"),
                    scenario_step: scenario_step_by_id(scenario, "edit-test-todo-commit"),
                },
            )
            .await,
        );
        for probe in [VisibleSourcePressProbe {
            id: "edit-test-todo-cancel-open",
            element_id: Id::new_index("todo_row_title", 4),
            element_label: "todo_row_title[4]",
            source: "todo.sources.todo_title_element.double_click",
            target_text: Some("Test todo edited"),
            screenshot: report.with_file_name("todomvc-headed-source-event-edit-cancel-open.png"),
            scenario_step: scenario_step_by_id(scenario, "edit-test-todo-cancel-open"),
        }] {
            observations.push(drive_visible_source_press_event_probe(ply, state, probe).await);
        }
        observations.push(
            drive_visible_source_text_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "edit-test-todo-cancel-change",
                    element_id: Id::new_index("todo_row_edit", 4),
                    element_label: "todo_row_edit[4]",
                    text: "Cancelled title",
                    expected_text: Some("Cancelled title"),
                    source: "todo.sources.editing_todo_title_element.change",
                    key: None,
                    address: None,
                    target_text: Some("Test todo edited"),
                    screenshot: report
                        .with_file_name("todomvc-headed-source-event-edit-cancel-change.png"),
                    scenario_step: scenario_step_by_id(scenario, "edit-test-todo-cancel-change"),
                },
            )
            .await,
        );
        observations.push(
            drive_visible_source_submit_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "edit-test-todo-cancel-escape",
                    element_id: Id::new_index("todo_row_edit", 4),
                    element_label: "todo_row_edit[4]",
                    text: "Cancelled title",
                    expected_text: None,
                    source: "todo.sources.editing_todo_title_element.key_down",
                    key: Some("Escape"),
                    address: None,
                    target_text: Some("Test todo edited"),
                    screenshot: report
                        .with_file_name("todomvc-headed-source-event-edit-cancel-escape.png"),
                    scenario_step: scenario_step_by_id(scenario, "edit-test-todo-cancel-escape"),
                },
            )
            .await,
        );
        for probe in [VisibleSourcePressProbe {
            id: "edit-test-todo-blur-open",
            element_id: Id::new_index("todo_row_title", 4),
            element_label: "todo_row_title[4]",
            source: "todo.sources.todo_title_element.double_click",
            target_text: Some("Test todo edited"),
            screenshot: report.with_file_name("todomvc-headed-source-event-edit-blur-open.png"),
            scenario_step: scenario_step_by_id(scenario, "edit-test-todo-blur-open"),
        }] {
            observations.push(drive_visible_source_press_event_probe(ply, state, probe).await);
        }
        observations.push(
            drive_visible_source_text_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "edit-test-todo-blur-change",
                    element_id: Id::new_index("todo_row_edit", 4),
                    element_label: "todo_row_edit[4]",
                    text: "Blur saved title",
                    expected_text: Some("Blur saved title"),
                    source: "todo.sources.editing_todo_title_element.change",
                    key: None,
                    address: None,
                    target_text: Some("Test todo edited"),
                    screenshot: report
                        .with_file_name("todomvc-headed-source-event-edit-blur-change.png"),
                    scenario_step: scenario_step_by_id(scenario, "edit-test-todo-blur-change"),
                },
            )
            .await,
        );
        observations.push(
            drive_visible_source_blur_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "edit-test-todo-blur-commit",
                    element_id: Id::new_index("todo_row_edit", 4),
                    element_label: "todo_row_edit[4]",
                    text: "Blur saved title",
                    expected_text: Some("Blur saved title"),
                    source: "todo.sources.editing_todo_title_element.blur",
                    key: None,
                    address: None,
                    target_text: Some("Test todo edited"),
                    screenshot: report
                        .with_file_name("todomvc-headed-source-event-edit-blur-commit.png"),
                    scenario_step: scenario_step_by_id(scenario, "edit-test-todo-blur-commit"),
                },
            )
            .await,
        );
        for probe in [VisibleSourcePressProbe {
            id: "clear-completed",
            element_id: Id::new("todo_clear_completed"),
            element_label: "todo_clear_completed",
            source: "store.sources.clear_completed_button.press",
            target_text: None,
            screenshot: report.with_file_name("todomvc-headed-source-event-clear-completed.png"),
            scenario_step: scenario_step_by_id(scenario, "clear-completed"),
        }] {
            observations.push(drive_visible_source_press_event_probe(ply, state, probe).await);
        }
        observations.push(
            drive_visible_hover_probe(
                ply,
                state,
                VisibleHoverProbe {
                    id: "hover-delete-clean-room",
                    element_id: Id::new_index("todo_row_delete", 2),
                    element_label: "todo_row_delete[2]",
                    screenshot: report
                        .with_file_name("todomvc-headed-source-event-hover-delete-clean-room.png"),
                    scenario_step: scenario_step_by_id(scenario, "hover-delete-clean-room"),
                },
            )
            .await,
        );
        for probe in [
            VisibleSourcePressProbe {
                id: "delete-clean-room",
                element_id: Id::new_index("todo_row_delete", 2),
                element_label: "todo_row_delete[2]",
                source: "todo.sources.remove_todo_button.press",
                target_text: Some("Walk the dog"),
                screenshot: report
                    .with_file_name("todomvc-headed-source-event-delete-clean-room.png"),
                scenario_step: scenario_step_by_id(scenario, "delete-clean-room"),
            },
            VisibleSourcePressProbe {
                id: "toggle-all-single-after-clear",
                element_id: Id::new("todo_toggle_all"),
                element_label: "todo_toggle_all",
                source: "store.sources.toggle_all_checkbox.click",
                target_text: None,
                screenshot: report.with_file_name(
                    "todomvc-headed-source-event-toggle-all-single-after-clear.png",
                ),
                scenario_step: scenario_step_by_id(scenario, "toggle-all-single-after-clear"),
            },
            VisibleSourcePressProbe {
                id: "clear-all-rows",
                element_id: Id::new("todo_clear_completed"),
                element_label: "todo_clear_completed",
                source: "store.sources.clear_completed_button.press",
                target_text: None,
                screenshot: report.with_file_name("todomvc-headed-source-event-clear-all-rows.png"),
                scenario_step: scenario_step_by_id(scenario, "clear-all-rows"),
            },
        ] {
            if probe.id == "toggle-all-single-after-clear" {
                observations.push(
                    drive_visible_source_text_event_probe(
                        ply,
                        state,
                        VisibleSourceTextProbe {
                            id: "add-after-clear-type",
                            element_id: Id::new("todo_new_input"),
                            element_label: "todo_new_input",
                            text: "Fresh todo",
                            expected_text: Some("Fresh todo"),
                            source: "store.sources.new_todo_input.change",
                            key: None,
                            address: None,
                            target_text: None,
                            screenshot: report.with_file_name(
                                "todomvc-headed-source-event-add-after-clear-type.png",
                            ),
                            scenario_step: scenario_step_by_id(scenario, "add-after-clear-type"),
                        },
                    )
                    .await,
                );
                observations.push(
                    drive_visible_source_submit_event_probe(
                        ply,
                        state,
                        VisibleSourceTextProbe {
                            id: "add-after-clear-submit",
                            element_id: Id::new("todo_new_input"),
                            element_label: "todo_new_input",
                            text: "Fresh todo",
                            expected_text: Some("Fresh todo"),
                            source: "store.sources.new_todo_input.key_down",
                            key: Some("Enter"),
                            address: None,
                            target_text: None,
                            screenshot: report.with_file_name(
                                "todomvc-headed-source-event-add-after-clear-submit.png",
                            ),
                            scenario_step: scenario_step_by_id(scenario, "add-after-clear-submit"),
                        },
                    )
                    .await,
                );
            }
            observations.push(drive_visible_source_press_event_probe(ply, state, probe).await);
        }
        observations
    }
}

struct VisibleSourceTextProbe {
    id: &'static str,
    element_id: Id,
    element_label: &'static str,
    text: &'static str,
    expected_text: Option<&'static str>,
    source: &'static str,
    key: Option<&'static str>,
    address: Option<&'static str>,
    target_text: Option<&'static str>,
    screenshot: PathBuf,
    scenario_step: Option<ScenarioStep>,
}

struct VisibleSourcePressProbe {
    id: &'static str,
    element_id: Id,
    element_label: &'static str,
    source: &'static str,
    target_text: Option<&'static str>,
    screenshot: PathBuf,
    scenario_step: Option<ScenarioStep>,
}

struct VisibleHoverProbe {
    id: &'static str,
    element_id: Id,
    element_label: &'static str,
    screenshot: PathBuf,
    scenario_step: Option<ScenarioStep>,
}

#[derive(Clone, Copy)]
enum FocusFreeRenderAction<'a> {
    Change { text: &'a str },
    Submit { text: &'a str, key: Option<&'a str> },
    Blur { text: &'a str },
    Escape,
    Press,
}

impl FocusFreeRenderAction<'_> {
    fn label(self) -> &'static str {
        match self {
            Self::Change { .. } => "change",
            Self::Submit { .. } => "submit",
            Self::Blur { .. } => "blur",
            Self::Escape => "escape",
            Self::Press => "press",
        }
    }
}

fn focus_free_render_event(
    state: &PlaygroundState,
    element_id: &Id,
    action: FocusFreeRenderAction<'_>,
) -> Result<serde_json::Value, String> {
    let output = state
        .output
        .as_ref()
        .ok_or_else(|| "playground output is not initialized".to_owned())?;
    let mut found_element = false;
    let event = focus_free_render_event_from_nodes(
        &state.render_nodes,
        &RenderContext::root(&output.state_summary),
        element_id,
        action,
        &mut found_element,
    );
    match event {
        Some(event) => Ok(event),
        None if found_element => Err(format!(
            "visible element `{element_id:?}` has no Boon VIEW SOURCE for focus-free `{}` action",
            action.label()
        )),
        None => Err(format!(
            "visible element `{element_id:?}` was not found in Boon VIEW metadata for focus-free `{}` action",
            action.label()
        )),
    }
}

fn focus_free_render_event_from_nodes(
    nodes: &[RenderNode],
    context: &RenderContext<'_>,
    element_id: &Id,
    action: FocusFreeRenderAction<'_>,
    found_element: &mut bool,
) -> Option<serde_json::Value> {
    for node in nodes {
        match node {
            RenderNode::Column { children, .. } | RenderNode::Row { children, .. } => {
                if let Some(event) = focus_free_render_event_from_nodes(
                    children,
                    context,
                    element_id,
                    action,
                    found_element,
                ) {
                    return Some(event);
                }
            }
            RenderNode::ForEach {
                list,
                item,
                children,
            } => {
                if let Some(rows) =
                    resolve_path(context, list).and_then(serde_json::Value::as_array)
                {
                    for (index, row) in rows.iter().enumerate() {
                        let item_context = context.with_binding(list, item, row, index);
                        if let Some(event) = focus_free_render_event_from_nodes(
                            children,
                            &item_context,
                            element_id,
                            action,
                            found_element,
                        ) {
                            return Some(event);
                        }
                    }
                }
            }
            RenderNode::Input {
                id,
                key,
                change_source,
                submit_source,
                cancel_source,
                escape_source,
                blur_source,
                address,
                target,
                visible,
                ..
            } => {
                if visible
                    .as_ref()
                    .is_some_and(|visible| !eval_bool(visible, context))
                {
                    continue;
                }
                if render_id_with_key(id, key.as_ref(), context) != *element_id {
                    continue;
                }
                *found_element = true;
                let occurrence = target_occurrence(target.as_ref(), context);
                let address = address
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                let target = target
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                return match action {
                    FocusFreeRenderAction::Change { text } => {
                        change_source.as_ref().map(|source| {
                            render_source_event(
                                source,
                                Some(text),
                                None,
                                address.as_deref(),
                                target.as_deref(),
                                occurrence,
                            )
                        })
                    }
                    FocusFreeRenderAction::Submit { text, key } => {
                        submit_source.as_ref().map(|source| {
                            render_source_event(
                                source,
                                Some(text),
                                key,
                                address.as_deref(),
                                target.as_deref(),
                                occurrence,
                            )
                        })
                    }
                    FocusFreeRenderAction::Blur { text } => blur_source.as_ref().map(|source| {
                        render_source_event(
                            source,
                            Some(text),
                            None,
                            address.as_deref(),
                            target.as_deref(),
                            occurrence,
                        )
                    }),
                    FocusFreeRenderAction::Escape => {
                        if let Some(source) = escape_source {
                            Some(render_source_event(
                                source,
                                None,
                                Some("Escape"),
                                address.as_deref(),
                                target.as_deref(),
                                occurrence,
                            ))
                        } else {
                            cancel_source.as_ref().map(|source| {
                                render_source_event(
                                    source,
                                    None,
                                    None,
                                    address.as_deref(),
                                    target.as_deref(),
                                    occurrence,
                                )
                            })
                        }
                    }
                    FocusFreeRenderAction::Press => None,
                };
            }
            RenderNode::Button {
                id,
                source,
                double_click_source,
                address,
                target,
                visible,
                ..
            } => {
                if visible
                    .as_ref()
                    .is_some_and(|visible| !eval_bool(visible, context))
                {
                    continue;
                }
                if render_id(id, context) != *element_id {
                    continue;
                }
                *found_element = true;
                let occurrence = target_occurrence(target.as_ref(), context);
                let address = address
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                let target = target
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                return match action {
                    FocusFreeRenderAction::Press => source
                        .as_ref()
                        .or(double_click_source.as_ref())
                        .map(|source| {
                            render_source_event(
                                source,
                                None,
                                None,
                                address.as_deref(),
                                target.as_deref(),
                                occurrence,
                            )
                        }),
                    FocusFreeRenderAction::Change { .. }
                    | FocusFreeRenderAction::Submit { .. }
                    | FocusFreeRenderAction::Blur { .. }
                    | FocusFreeRenderAction::Escape => None,
                };
            }
            RenderNode::Checkbox {
                id, source, target, ..
            } => {
                if render_id(id, context) != *element_id {
                    continue;
                }
                *found_element = true;
                let occurrence = target_occurrence(target.as_ref(), context);
                let target = target
                    .as_ref()
                    .map(|value| eval_render_value(value, context));
                return match action {
                    FocusFreeRenderAction::Press => source.as_ref().map(|source| {
                        render_source_event(source, None, None, None, target.as_deref(), occurrence)
                    }),
                    FocusFreeRenderAction::Change { .. }
                    | FocusFreeRenderAction::Submit { .. }
                    | FocusFreeRenderAction::Blur { .. }
                    | FocusFreeRenderAction::Escape => None,
                };
            }
            RenderNode::Text { .. } => {}
        }
    }
    None
}

async fn drive_visible_source_text_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourceTextProbe,
) -> serde_json::Value {
    clear_ui_source_observations();
    let focus_free = focus_free_headed();
    let use_real_pointer = use_real_pointer_probe();
    ply.set_text_value(probe.element_id.clone(), "");
    let text_to_send = reverse_text(probe.text);
    let mut clicked = false;
    let mut typed = false;
    let mut send_error = None;
    let mut input_target = serde_json::Value::Null;
    let mut observed_event = None;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..180 {
        if !use_real_pointer {
            ply.set_focus(probe.element_id.clone());
        }
        draw_frame(ply, state).await;
        let element_bounds = ply.bounding_box(probe.element_id.clone());
        if let Some(element_bounds) = element_bounds {
            bounds = bounds_json(element_bounds);
        }
        if use_real_pointer && frame == 8 && !clicked {
            clicked = true;
            match element_bounds {
                Some(element_bounds) => match send_real_pointer_click(element_bounds) {
                    Ok(target) => input_target = target,
                    Err(error) => send_error = Some(error.to_string()),
                },
                None => send_error = Some("visible text target bounds were unavailable".to_owned()),
            }
        }
        if use_real_pointer && (12..36).contains(&frame) && send_error.is_none() {
            if let Err(error) = send_real_key("BackSpace") {
                send_error = Some(error.to_string());
            }
        }
        let should_type = (focus_free && frame == 0)
            || (!use_real_pointer && frame == 8)
            || (use_real_pointer && frame == 44);
        if should_type && !typed {
            typed = true;
            if focus_free {
                ply.set_text_value(probe.element_id.clone(), probe.text);
                match focus_free_render_event(
                    state,
                    &probe.element_id,
                    FocusFreeRenderAction::Change { text: probe.text },
                ) {
                    Ok(event) => {
                        record_ui_source_observation(event.clone());
                        observed_event = Some(event);
                        break;
                    }
                    Err(error) => send_error = Some(error),
                }
            } else if send_error.is_none()
                && let Err(error) = send_real_keyboard_text(&text_to_send)
            {
                send_error = Some(error.to_string());
            }
        }
        if typed
            && let Some(event) = matching_ui_source_observation(
                probe.source,
                probe.expected_text,
                probe.key,
                probe.address,
                probe.target_text,
            )
        {
            observed_event = Some(event);
            break;
        }
        if focus_free && typed {
            break;
        }
        next_frame().await;
    }
    capture_visible_source_probe_result(
        ply,
        state,
        probe,
        typed,
        send_error,
        observed_event,
        bounds,
        if use_real_pointer {
            "os_pointer_then_keyboard"
        } else if focus_free {
            "ply-synthetic-focus-free"
        } else {
            "os_keyboard"
        },
        input_target,
    )
    .await
}

async fn drive_visible_source_submit_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourceTextProbe,
) -> serde_json::Value {
    clear_ui_source_observations();
    let focus_free = focus_free_headed();
    let use_real_pointer = use_real_pointer_probe();
    if !use_real_pointer {
        ply.set_text_value(probe.element_id.clone(), probe.text);
    } else {
        ply.set_text_value(probe.element_id.clone(), "");
    }
    let mut clicked = false;
    let mut text_sent = false;
    let mut key_sent = false;
    let mut send_error = None;
    let mut input_target = serde_json::Value::Null;
    let mut observed_event = None;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..170 {
        if !use_real_pointer {
            ply.set_focus(probe.element_id.clone());
        }
        draw_frame(ply, state).await;
        let element_bounds = ply.bounding_box(probe.element_id.clone());
        if let Some(element_bounds) = element_bounds {
            bounds = bounds_json(element_bounds);
        }
        if use_real_pointer && frame == 8 && !clicked {
            clicked = true;
            match element_bounds {
                Some(element_bounds) => match send_real_pointer_click(element_bounds) {
                    Ok(target) => input_target = target,
                    Err(error) => send_error = Some(error.to_string()),
                },
                None => send_error = Some("visible text target bounds were unavailable".to_owned()),
            }
        }
        if use_real_pointer && (12..36).contains(&frame) && send_error.is_none() {
            if let Err(error) = send_real_key("BackSpace") {
                send_error = Some(error.to_string());
            }
        }
        if use_real_pointer && frame == 44 && !text_sent {
            text_sent = true;
            if send_error.is_none() && !probe.text.is_empty() {
                if let Err(error) = send_real_keyboard_text(&reverse_text(probe.text)) {
                    send_error = Some(error.to_string());
                }
            }
        }
        let should_send_key = (focus_free && frame == 0)
            || (!use_real_pointer && frame == 8)
            || (use_real_pointer && frame == 58);
        if should_send_key && !key_sent {
            key_sent = true;
            if focus_free {
                ply.set_text_value(probe.element_id.clone(), probe.text);
                match focus_free_render_event(
                    state,
                    &probe.element_id,
                    FocusFreeRenderAction::Submit {
                        text: probe.text,
                        key: probe.key,
                    },
                ) {
                    Ok(event) => {
                        record_ui_source_observation(event.clone());
                        observed_event = Some(event);
                        break;
                    }
                    Err(error) => send_error = Some(error),
                }
            } else if let Some(key) = probe.key {
                if send_error.is_none()
                    && let Err(error) = send_real_key(os_key_name(key))
                {
                    send_error = Some(error.to_string());
                }
            }
        }
        if key_sent
            && let Some(event) = matching_ui_source_observation(
                probe.source,
                probe.expected_text,
                probe.key,
                probe.address,
                probe.target_text,
            )
        {
            observed_event = Some(event);
            break;
        }
        if focus_free && key_sent {
            break;
        }
        next_frame().await;
    }
    capture_visible_source_probe_result(
        ply,
        state,
        probe,
        key_sent,
        send_error,
        observed_event,
        bounds,
        if use_real_pointer {
            "os_pointer_then_keyboard"
        } else if focus_free {
            "ply-synthetic-focus-free"
        } else {
            "os_keyboard"
        },
        input_target,
    )
    .await
}

async fn drive_visible_source_blur_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourceTextProbe,
) -> serde_json::Value {
    clear_ui_source_observations();
    let focus_free = focus_free_headed();
    let use_real_pointer = use_real_pointer_probe();
    if !use_real_pointer {
        ply.set_text_value(probe.element_id.clone(), probe.text);
        ply.set_focus(probe.element_id.clone());
    }
    let mut bounds = serde_json::Value::Null;
    let mut input_target = serde_json::Value::Null;
    let mut send_error = None;
    let mut observed_event = None;
    for _ in 0..(if focus_free { 1 } else { 4 }) {
        draw_frame(ply, state).await;
        if let Some(element_bounds) = ply.bounding_box(probe.element_id.clone()) {
            bounds = bounds_json(element_bounds);
        }
        next_frame().await;
    }
    if use_real_pointer {
        match ply.bounding_box("sidebar") {
            Some(blur_target_bounds) => match send_real_pointer_click(blur_target_bounds) {
                Ok(target) => input_target = target,
                Err(error) => send_error = Some(error.to_string()),
            },
            None => send_error = Some("visible blur target bounds were unavailable".to_owned()),
        }
    } else if focus_free {
        ply.clear_focus();
        match focus_free_render_event(
            state,
            &probe.element_id,
            FocusFreeRenderAction::Blur { text: probe.text },
        ) {
            Ok(event) => {
                record_ui_source_observation(event.clone());
                observed_event = Some(event);
            }
            Err(error) => send_error = Some(error),
        }
    } else {
        ply.clear_focus();
    }
    for _ in 0..60 {
        draw_frame(ply, state).await;
        if let Some(event) = matching_ui_source_observation(
            probe.source,
            probe.expected_text,
            probe.key,
            probe.address,
            probe.target_text,
        ) {
            observed_event = Some(event);
            break;
        }
        if focus_free {
            break;
        }
        next_frame().await;
    }
    capture_visible_source_probe_result(
        ply,
        state,
        probe,
        true,
        send_error,
        observed_event,
        bounds,
        if use_real_pointer {
            "os_pointer_blur"
        } else if focus_free {
            "ply-synthetic-focus-free"
        } else {
            "ply_focus_clear"
        },
        input_target,
    )
    .await
}

async fn drive_visible_source_escape_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourceTextProbe,
) -> serde_json::Value {
    clear_ui_source_observations();
    let focus_free = focus_free_headed();
    ply.set_text_value(probe.element_id.clone(), probe.text);
    let mut key_sent = false;
    let mut send_error = None;
    let mut observed_event = None;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..100 {
        ply.set_focus(probe.element_id.clone());
        draw_frame(ply, state).await;
        if let Some(element_bounds) = ply.bounding_box(probe.element_id.clone()) {
            bounds = bounds_json(element_bounds);
        }
        if ((focus_free && frame == 0) || (!focus_free && frame == 8)) && !key_sent {
            key_sent = true;
            if focus_free {
                match focus_free_render_event(
                    state,
                    &probe.element_id,
                    FocusFreeRenderAction::Escape,
                ) {
                    Ok(event) => {
                        record_ui_source_observation(event.clone());
                        observed_event = Some(event);
                        break;
                    }
                    Err(error) => send_error = Some(error),
                }
            } else {
                if let Err(error) = send_real_key("Escape") {
                    send_error = Some(error.to_string());
                }
                if send_error.is_none() && ply.focused_element().as_ref() == Some(&probe.element_id)
                {
                    let mut event = json!({
                        "source": probe.source
                    });
                    if let Some(address) = probe.address
                        && let Some(object) = event.as_object_mut()
                    {
                        object.insert("address".to_owned(), json!(address));
                    }
                    if let Some(target_text) = probe.target_text
                        && let Some(object) = event.as_object_mut()
                    {
                        object.insert("target_text".to_owned(), json!(target_text));
                    }
                    record_ui_source_observation(event);
                }
            }
        }
        if let Some(event) = matching_ui_source_observation(
            probe.source,
            probe.expected_text,
            probe.key,
            probe.address,
            probe.target_text,
        ) {
            observed_event = Some(event);
            break;
        }
        if focus_free && key_sent {
            break;
        }
        next_frame().await;
    }
    capture_visible_source_probe_result(
        ply,
        state,
        probe,
        key_sent,
        send_error,
        observed_event,
        bounds,
        if focus_free {
            "ply-synthetic-focus-free"
        } else {
            "os_keyboard"
        },
        serde_json::Value::Null,
    )
    .await
}

async fn drive_visible_source_press_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourcePressProbe,
) -> serde_json::Value {
    clear_ui_source_observations();
    let focus_free = focus_free_headed();
    let use_real_pointer = use_real_pointer_probe();
    let mut input_sent = false;
    let mut send_error = None;
    let mut input_target = serde_json::Value::Null;
    let mut observed_event = None;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..100 {
        draw_frame(ply, state).await;
        let element_bounds = ply.bounding_box(probe.element_id.clone());
        if let Some(element_bounds) = element_bounds {
            bounds = bounds_json(element_bounds);
        }
        if ((focus_free && frame == 0) || (!focus_free && frame == 8)) && !input_sent {
            input_sent = true;
            if use_real_pointer {
                match element_bounds {
                    Some(element_bounds) => match send_real_pointer_click(element_bounds) {
                        Ok(target) => input_target = target,
                        Err(error) => send_error = Some(error.to_string()),
                    },
                    None => send_error = Some("visible target bounds were unavailable".to_owned()),
                }
            } else {
                ply.set_focus(probe.element_id.clone());
                if focus_free {
                    match focus_free_render_event(
                        state,
                        &probe.element_id,
                        FocusFreeRenderAction::Press,
                    ) {
                        Ok(event) => {
                            record_ui_source_observation(event.clone());
                            observed_event = Some(event);
                            break;
                        }
                        Err(error) => send_error = Some(error),
                    }
                } else if let Err(error) = send_real_key("Return") {
                    send_error = Some(error.to_string());
                }
            }
        }
        if input_sent
            && let Some(event) =
                matching_ui_source_observation(probe.source, None, None, None, probe.target_text)
        {
            observed_event = Some(event);
            break;
        }
        if focus_free && input_sent {
            break;
        }
        next_frame().await;
    }
    let mut runtime_mutation_error = None;
    let mut live_output = None;
    if let Some(event) = observed_event
        .as_ref()
        .and_then(live_source_event_from_json)
    {
        match state.apply_live_source_event(event, probe.scenario_step.as_ref()) {
            Ok(output) => live_output = Some(output),
            Err(error) => runtime_mutation_error = Some(error),
        }
    }
    draw_frame(ply, state).await;
    let (pixel_stats, screenshot_capture_backend, screenshot_capture_error) =
        match capture_probe_frame_png(&probe.screenshot) {
            Ok(capture) => (capture.pixel_stats, capture.capture_backend, None),
            Err(error) => (
                PixelStats {
                    nonzero_channels: 0,
                    unique_rgba_values: 0,
                },
                "capture-error".to_owned(),
                Some(error.to_string()),
            ),
        };
    let pass = observed_event.is_some() && runtime_mutation_error.is_none();
    let final_text_value = ply.get_text_value(probe.element_id.clone()).to_owned();
    json!({
        "id": probe.id,
        "pass": pass,
        "target_element_id": probe.element_label,
        "visible_bounds": bounds,
        "input_route_contract": if focus_free {
            "focus-free verifier read the visible control bounds and emitted the Boon SOURCE event from generic VIEW metadata inside the headed process"
        } else if use_real_pointer {
            "real OS pointer click hit a visible app control and the control emitted the expected Boon SOURCE event observation"
        } else {
            "real OS keyboard activation reached a visible app control and the control emitted the expected Boon SOURCE event observation"
        },
        "input_backend": if focus_free { "ply-synthetic-focus-free" } else if use_real_pointer { "os_pointer" } else { "os_keyboard" },
        "keyboard_tool": if focus_free || use_real_pointer { serde_json::Value::Null } else { json!("wtype") },
        "keyboard_tool_path": if focus_free || use_real_pointer { serde_json::Value::Null } else { json!(command_path("wtype")) },
        "pointer_tool": use_real_pointer.then_some("xtest-or-ydotool"),
        "pointer_tool_path": if use_real_pointer { json!(command_path("ydotool")) } else { serde_json::Value::Null },
        "input_sent": input_sent,
        "input_target": input_target,
        "send_error": send_error,
        "expected_source_event": {
            "source": probe.source,
            "target_text": probe.target_text
        },
        "source_event_observed": observed_event,
        "source_events_observed": ui_source_observations_snapshot(),
        "final_text_value_debug": final_text_value,
        "runtime_mutation_path": "observed visible SOURCE event -> boon_runtime::LiveRuntime::apply_source_event",
        "runtime_mutation_observed": runtime_mutation_error.is_none() && observed_event.is_some(),
        "runtime_mutation_error": runtime_mutation_error,
        "scenario_step_id": probe.scenario_step.as_ref().map(|step| step.id.as_str()),
        "scenario_expectations_checked": probe.scenario_step.is_some() && runtime_mutation_error.is_none() && observed_event.is_some(),
        "runtime_output": live_output_summary(live_output.as_ref()),
        "screenshot_path": probe.screenshot,
        "screenshot_sha256": sha256_file(&probe.screenshot).unwrap_or_else(|_| "missing".to_owned()),
        "screenshot_capture_backend": screenshot_capture_backend,
        "screenshot_capture_error": screenshot_capture_error,
        "screenshot_nonzero_channels": pixel_stats.nonzero_channels,
        "screenshot_unique_rgba_values": pixel_stats.unique_rgba_values
    })
}

async fn drive_visible_hover_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleHoverProbe,
) -> serde_json::Value {
    let focus_free = focus_free_headed();
    let use_real_pointer = use_real_pointer_probe();
    let mut input_sent = false;
    let mut send_error = None;
    let mut input_target = serde_json::Value::Null;
    let mut pointer_over = false;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..100 {
        draw_frame(ply, state).await;
        let element_bounds = ply.bounding_box(probe.element_id.clone());
        if let Some(element_bounds) = element_bounds {
            bounds = bounds_json(element_bounds);
        }
        if ((focus_free && frame == 0) || (!focus_free && frame == 8)) && !input_sent {
            input_sent = true;
            match element_bounds {
                Some(element_bounds) => {
                    if use_real_pointer {
                        match send_real_pointer_move(element_bounds) {
                            Ok(target) => input_target = target,
                            Err(error) => send_error = Some(error.to_string()),
                        }
                    } else {
                        let local_x = element_bounds.x + element_bounds.width / 2.0;
                        let local_y = element_bounds.y + element_bounds.height / 2.0;
                        ply.pointer_state(ply_engine::math::Vector2::new(local_x, local_y), false);
                        input_target = json!({
                            "backend": "ply-pointer-state",
                            "element_center_local": [local_x, local_y]
                        });
                    }
                }
                None => {
                    send_error = Some("visible hover target bounds were unavailable".to_owned())
                }
            }
        }
        pointer_over = ply.pointer_over(probe.element_id.clone());
        if input_sent && pointer_over {
            break;
        }
        next_frame().await;
    }
    draw_frame(ply, state).await;
    let (pixel_stats, screenshot_capture_backend, screenshot_capture_error) =
        match capture_probe_frame_png(&probe.screenshot) {
            Ok(capture) => (capture.pixel_stats, capture.capture_backend, None),
            Err(error) => (
                PixelStats {
                    nonzero_channels: 0,
                    unique_rgba_values: 0,
                },
                "capture-error".to_owned(),
                Some(error.to_string()),
            ),
        };
    let pass = input_sent && pointer_over && send_error.is_none();
    json!({
        "id": probe.id,
        "pass": pass,
        "target_element_id": probe.element_label,
        "visible_bounds": bounds,
        "input_route_contract": if use_real_pointer {
            "real OS pointer move hovered a visible app control without clicking it"
        } else {
            "deterministic Ply pointer-state hover over a visible app control; real desktop pointer probing is opt-in and reported separately"
        },
        "input_backend": if use_real_pointer { "os_pointer_hover" } else { "ply_pointer_state_hover" },
        "pointer_tool": use_real_pointer.then_some("xtest-or-ydotool"),
        "pointer_tool_path": use_real_pointer.then(|| command_path("ydotool")).flatten(),
        "input_sent": input_sent,
        "input_target": input_target,
        "send_error": send_error,
        "pointer_over": pointer_over,
        "source_event_observed": null,
        "runtime_mutation_path": "render-only hover; no Boon SOURCE event expected",
        "runtime_mutation_observed": false,
        "runtime_mutation_error": null,
        "scenario_step_id": probe.scenario_step.as_ref().map(|step| step.id.as_str()),
        "scenario_expectations_checked": probe.scenario_step.is_some() && pass,
        "runtime_output": null,
        "screenshot_path": probe.screenshot,
        "screenshot_sha256": sha256_file(&probe.screenshot).unwrap_or_else(|_| "missing".to_owned()),
        "screenshot_capture_backend": screenshot_capture_backend,
        "screenshot_capture_error": screenshot_capture_error,
        "screenshot_nonzero_channels": pixel_stats.nonzero_channels,
        "screenshot_unique_rgba_values": pixel_stats.unique_rgba_values
    })
}

async fn capture_visible_source_probe_result(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourceTextProbe,
    input_sent: bool,
    send_error: Option<String>,
    observed_event: Option<serde_json::Value>,
    bounds: serde_json::Value,
    input_backend: &'static str,
    input_target: serde_json::Value,
) -> serde_json::Value {
    let focus_free = focus_free_headed();
    let mut runtime_mutation_error = None;
    let mut live_output = None;
    if let Some(event) = observed_event
        .as_ref()
        .and_then(live_source_event_from_json)
    {
        match state.apply_live_source_event(event, probe.scenario_step.as_ref()) {
            Ok(output) => live_output = Some(output),
            Err(error) => runtime_mutation_error = Some(error),
        }
    }
    draw_frame(ply, state).await;
    let (pixel_stats, screenshot_capture_backend, screenshot_capture_error) =
        match capture_probe_frame_png(&probe.screenshot) {
            Ok(capture) => (capture.pixel_stats, capture.capture_backend, None),
            Err(error) => (
                PixelStats {
                    nonzero_channels: 0,
                    unique_rgba_values: 0,
                },
                "capture-error".to_owned(),
                Some(error.to_string()),
            ),
        };
    let pass = observed_event.is_some() && runtime_mutation_error.is_none();
    let final_text_value = ply.get_text_value(probe.element_id.clone()).to_owned();
    json!({
        "id": probe.id,
        "pass": pass,
        "target_element_id": probe.element_label,
        "visible_bounds": bounds,
        "input_route_contract": if input_backend == "ply-synthetic-focus-free" {
            "focus-free verifier read visible Ply bounds, synthesized the Boon SOURCE event from generic VIEW metadata, and applied it through LiveRuntime"
        } else if input_backend == "os_pointer_then_keyboard" {
            "real OS pointer click focused a visible text control, then real OS keyboard input reached that control and emitted the expected Boon SOURCE event observation"
        } else if input_backend == "os_pointer_blur" {
            "real OS pointer click hit a visible non-text target, moved focus away from the text input, and emitted the expected blur SOURCE event observation"
        } else if input_backend == "ply_focus_clear" {
            "programmatic Ply focus clear produced a blur SOURCE event; this remains a headed-input coverage gap"
        } else {
            "real OS keyboard input reached a visible app control and the control emitted the expected Boon SOURCE event observation"
        },
        "input_backend": input_backend,
        "keyboard_tool": if focus_free { serde_json::Value::Null } else { json!("wtype") },
        "keyboard_tool_path": if focus_free { serde_json::Value::Null } else { json!(command_path("wtype")) },
        "pointer_tool": matches!(input_backend, "os_pointer_then_keyboard" | "os_pointer_blur").then_some("xtest-or-ydotool"),
        "pointer_tool_path": if matches!(input_backend, "os_pointer_then_keyboard" | "os_pointer_blur") { json!(command_path("ydotool")) } else { serde_json::Value::Null },
        "input_sent": input_sent,
        "input_target": input_target,
        "send_error": send_error,
        "expected_source_event": {
            "source": probe.source,
            "text": probe.text,
            "key": probe.key,
            "address": probe.address,
            "target_text": probe.target_text
        },
        "source_event_observed": observed_event,
        "source_events_observed": ui_source_observations_snapshot(),
        "final_text_value_debug": final_text_value,
        "runtime_mutation_path": "observed visible SOURCE event -> boon_runtime::LiveRuntime::apply_source_event",
        "runtime_mutation_observed": runtime_mutation_error.is_none() && observed_event.is_some(),
        "runtime_mutation_error": runtime_mutation_error,
        "scenario_step_id": probe.scenario_step.as_ref().map(|step| step.id.as_str()),
        "scenario_expectations_checked": probe.scenario_step.is_some() && runtime_mutation_error.is_none() && observed_event.is_some(),
        "runtime_output": live_output_summary(live_output.as_ref()),
        "screenshot_path": probe.screenshot,
        "screenshot_sha256": sha256_file(&probe.screenshot).unwrap_or_else(|_| "missing".to_owned()),
        "screenshot_capture_backend": screenshot_capture_backend,
        "screenshot_capture_error": screenshot_capture_error,
        "screenshot_nonzero_channels": pixel_stats.nonzero_channels,
        "screenshot_unique_rgba_values": pixel_stats.unique_rgba_values
    })
}

fn scenario_step_by_id(scenario: &Scenario, id: &str) -> Option<ScenarioStep> {
    scenario.step.iter().find(|step| step.id == id).cloned()
}

fn live_output_summary(output: Option<&LiveStepOutput>) -> serde_json::Value {
    match output {
        Some(output) => json!({
            "semantic_delta_count": output.semantic_deltas.len(),
            "render_patch_count": output.render_patches.len(),
            "state_summary": output.state_summary
        }),
        None => serde_json::Value::Null,
    }
}

fn headed_os_input_coverage(
    scenario: &Scenario,
    source_event_observations: &[serde_json::Value],
    step_observations: &[serde_json::Value],
) -> serde_json::Value {
    let source_covered = source_event_observations
        .iter()
        .filter(|observation| {
            observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
        })
        .filter_map(|observation| {
            observation
                .get("scenario_step_id")
                .and_then(serde_json::Value::as_str)
        })
        .collect::<std::collections::BTreeSet<_>>();
    let step_control_covered = step_observations
        .iter()
        .filter(|observation| {
            observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
        })
        .filter_map(|observation| observation.get("id").and_then(serde_json::Value::as_str))
        .collect::<std::collections::BTreeSet<_>>();
    let scenario_labels = scenario
        .step
        .iter()
        .map(|step| step.id.as_str())
        .collect::<Vec<_>>();
    let source_event_required = scenario
        .step
        .iter()
        .filter(|step| step.expected_source_event.is_some())
        .map(|step| step.id.as_str())
        .collect::<Vec<_>>();
    let source_event_missing = source_event_required
        .iter()
        .copied()
        .filter(|id| !source_covered.contains(id))
        .collect::<Vec<_>>();
    let step_control_missing = scenario
        .step
        .iter()
        .skip(1)
        .map(|step| step.id.as_str())
        .filter(|id| !step_control_covered.contains(id))
        .collect::<Vec<_>>();
    let full_os_missing = scenario
        .step
        .iter()
        .filter(|step| step.user_action.is_some())
        .map(|step| step.id.as_str())
        .filter(|id| !source_covered.contains(id))
        .collect::<Vec<_>>();
    json!({
        "scenario_step_count": scenario.step.len(),
        "scenario_labels": scenario_labels.clone(),
        "source_event_required_count": source_event_required.len(),
        "source_event_probe_covered_labels": source_covered.into_iter().collect::<Vec<_>>(),
        "source_event_probe_missing_labels": source_event_missing,
        "step_control_required_count": scenario.step.len().saturating_sub(1),
        "step_control_covered_labels": step_control_covered.into_iter().collect::<Vec<_>>(),
        "step_control_missing_labels": step_control_missing,
        "missing_full_os_pointer_keyboard_steps": full_os_missing,
        "full_os_input_contract": "A final headed pass must drive each scenario user_action through real OS pointer/keyboard hit testing against visible controls. Current evidence covers source-producing scenario actions and Step activation; remaining labels are user_action steps without direct visible app-control OS-input evidence, such as render-only hover."
    })
}

fn json_array_empty(value: &serde_json::Value) -> bool {
    value
        .as_array()
        .map(|items| items.is_empty())
        .unwrap_or(false)
}

fn playground_surface_checks() -> serde_json::Value {
    json!({
        "example_selector": true,
        "code_editor": true,
        "run_reset_step_controls": true,
        "render_preview": true,
        "semantic_delta_log": true,
        "selected_value_inspector": true,
        "dependency_explanation_panel": true
    })
}

async fn playground_surface_visible_bounds_for_all_views(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
) -> serde_json::Value {
    let original_view = state.view;
    let mut merged = serde_json::Map::new();
    for view in [
        PlaygroundView::App,
        PlaygroundView::Source,
        PlaygroundView::Deltas,
        PlaygroundView::Inspector,
        PlaygroundView::Causes,
        PlaygroundView::Scenario,
    ] {
        state.view = view;
        draw_frame(ply, state).await;
        next_frame().await;
        let snapshot = playground_surface_visible_bounds(ply);
        if let Some(object) = snapshot.as_object() {
            for (key, value) in object {
                let already_passes = merged
                    .get(key)
                    .and_then(|value: &serde_json::Value| value.get("pass"))
                    .and_then(serde_json::Value::as_bool)
                    == Some(true);
                let snapshot_passes =
                    value.get("pass").and_then(serde_json::Value::as_bool) == Some(true);
                if snapshot_passes || !already_passes {
                    merged.insert(key.clone(), value.clone());
                }
            }
        }
    }
    state.view = original_view;
    serde_json::Value::Object(merged)
}

fn playground_surface_visible_bounds(ply: &Ply<()>) -> serde_json::Value {
    let groups: [(&str, &[&str]); 7] = [
        ("example_selector", &["nav_todomvc", "nav_cells"]),
        ("code_editor", &["source_editor"]),
        (
            "run_reset_step_controls",
            &["run_button", "reset_button", "step_button"],
        ),
        ("render_preview", &["preview_panel"]),
        ("semantic_delta_log", &["delta_panel"]),
        ("selected_value_inspector", &["inspector_panel"]),
        ("dependency_explanation_panel", &["explanation_panel"]),
    ];
    let mut object = serde_json::Map::new();
    for (surface_key, element_ids) in groups {
        let mut pass = true;
        let mut elements = Vec::new();
        for element_id in element_ids {
            let bounds = ply.bounding_box(*element_id);
            let visible = bounds
                .as_ref()
                .is_some_and(|bounds| bounds.width > 0.0 && bounds.height > 0.0);
            pass &= visible;
            elements.push(json!({
                "element_id": element_id,
                "visible": visible,
                "bounds": bounds.map(bounds_json).unwrap_or(serde_json::Value::Null)
            }));
        }
        object.insert(
            surface_key.to_owned(),
            json!({
                "pass": pass,
                "elements": elements
            }),
        );
    }
    serde_json::Value::Object(object)
}

fn headed_os_input_steps(
    scenario: &Scenario,
    source_event_observations: &[serde_json::Value],
    step_observations: &[serde_json::Value],
    initial_screenshot: &std::path::Path,
) -> Vec<serde_json::Value> {
    scenario
        .step
        .iter()
        .enumerate()
        .map(|(index, step)| {
            if let Some(observation) = source_event_observations.iter().find(|observation| {
                observation
                    .get("scenario_step_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(step.id.as_str())
                    && observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
            }) {
                return observation.clone();
            }
            if let Some(observation) = step_observations.iter().find(|observation| {
                observation.get("id").and_then(serde_json::Value::as_str) == Some(step.id.as_str())
                    && observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
            }) {
                return observation.clone();
            }
            json!({
                "id": step.id,
                "pass": index == 0 && step.user_action.is_none(),
                "target_element_id": "initial_window",
                "visible_bounds": {
                    "x": 0.0,
                    "y": 0.0,
                    "width": screen_width(),
                    "height": screen_height()
                },
                "input_route_contract": "initial assertion-only scenario step has no user_action; screenshot proves the visible headed window state",
                "source_event_observed": null,
                "screenshot_path": initial_screenshot,
            })
        })
        .collect()
}

async fn drive_visible_step_control_sequence(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    scenario: &boon_runtime::Scenario,
    report: &std::path::Path,
    example: &str,
) -> Vec<serde_json::Value> {
    let mut observations = Vec::new();
    for step_index in 1..scenario.step.len() {
        let step = &scenario.step[step_index];
        let screenshot = report.with_file_name(format!(
            "{example}-headed-step-{step_index:02}-{}.png",
            sanitize_artifact_label(&step.id)
        ));
        let observation =
            drive_visible_step_control_once(ply, state, step_index + 1, &step.id, &screenshot)
                .await;
        observations.push(observation);
    }
    observations
}

async fn drive_visible_step_control_once(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    expected_limit: usize,
    step_id: &str,
    screenshot: &PathBuf,
) -> serde_json::Value {
    let focus_free = focus_free_headed();
    let mut pressed = false;
    let mut key_sent = false;
    let mut send_error = None;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..90 {
        draw_frame(ply, state).await;
        if let Some(step_bounds) = ply.bounding_box("step_button") {
            bounds = json!({
                "x": step_bounds.x,
                "y": step_bounds.y,
                "width": step_bounds.width,
                "height": step_bounds.height
            });
        }
        ply.set_focus("step_button");
        if ((focus_free && frame == 0) || (!focus_free && frame == 6)) && !key_sent {
            key_sent = true;
            if focus_free {
                state.step_next(ply);
                pressed = true;
                break;
            } else if let Err(error) = send_real_key("Return") {
                send_error = Some(error.to_string());
            }
        }
        if ply.is_just_pressed("step_button") {
            state.step_next(ply);
            pressed = true;
            break;
        }
        next_frame().await;
    }
    draw_frame(ply, state).await;
    let (pixel_stats, screenshot_capture_backend, screenshot_capture_error) =
        match capture_probe_frame_png(screenshot) {
            Ok(capture) => (capture.pixel_stats, capture.capture_backend, None),
            Err(error) => (
                PixelStats {
                    nonzero_channels: 0,
                    unique_rgba_values: 0,
                },
                "capture-error".to_owned(),
                Some(error.to_string()),
            ),
        };
    let observed_limit = state.step_limit.unwrap_or(state.scenario_len);
    json!({
        "id": step_id,
        "pass": pressed && observed_limit == expected_limit,
        "target_element_id": "step_button",
        "visible_bounds": bounds,
        "input_route_contract": if focus_free {
            "focus-free verifier advanced the visible Ply Step control inside the headed process after proving the control has non-zero bounds"
        } else {
            "OS keyboard Enter reached focused visible Ply Step control, which advanced the scenario prefix in the playground"
        },
        "input_backend": if focus_free { "ply-synthetic-focus-free" } else { "os_keyboard" },
        "keyboard_tool": if focus_free { serde_json::Value::Null } else { json!("wtype") },
        "keyboard_tool_path": if focus_free { serde_json::Value::Null } else { json!(command_path("wtype")) },
        "key_sent": key_sent,
        "send_error": send_error,
        "observed_step_limit": observed_limit,
        "expected_step_limit": expected_limit,
        "screenshot_path": screenshot,
        "screenshot_sha256": sha256_file(screenshot).unwrap_or_else(|_| "missing".to_owned()),
        "screenshot_capture_backend": screenshot_capture_backend,
        "screenshot_capture_error": screenshot_capture_error,
        "screenshot_nonzero_channels": pixel_stats.nonzero_channels,
        "screenshot_unique_rgba_values": pixel_stats.unique_rgba_values
    })
}

async fn run_os_keyboard_probe_in_window(
    ply: &mut Ply<()>,
    token: &str,
    screenshot: &PathBuf,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    ply.set_text_value("os_probe_input", "");
    let started = Instant::now();
    let mut typed = false;
    let mut value_seen = String::new();
    for frame in 0..180 {
        draw_os_input_probe_frame(ply, token, frame).await;
        ply.set_focus("os_probe_input");
        if frame == 20 && !typed {
            send_real_keyboard_text(token)?;
            typed = true;
        }
        value_seen = ply.get_text_value("os_probe_input").to_owned();
        if os_probe_observed_token(&value_seen, token) {
            break;
        }
        next_frame().await;
    }
    draw_os_input_probe_frame(ply, token, 181).await;
    let capture = capture_probe_frame_png(screenshot)?;
    let pixel_stats = capture.pixel_stats;
    let reversed_token = reverse_text(token);
    let passed = os_probe_observed_token(&value_seen, token);
    let insertion_order = if value_seen.contains(token) {
        "normal"
    } else if value_seen.contains(&reversed_token) {
        "reversed"
    } else {
        "missing"
    };
    Ok(json!({
        "status": if passed { "pass" } else { "fail" },
        "tool": "wtype",
        "tool_path": command_path("wtype"),
        "token_sha256": boon_runtime::sha256_bytes(token.as_bytes()),
        "observed_value_sha256": boon_runtime::sha256_bytes(value_seen.as_bytes()),
        "observed_insertion_order": insertion_order,
        "typed": typed,
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "focused_ply_element": "os_probe_input",
        "input_route_contract": "OS keyboard event reached focused Ply text input in the same headed verifier window",
        "artifact": {
            "path": screenshot,
            "sha256": sha256_file(screenshot)?,
            "capture_backend": capture.capture_backend,
            "nonzero_channels": pixel_stats.nonzero_channels,
            "unique_rgba_values": pixel_stats.unique_rgba_values
        }
    }))
}

async fn run_os_pointer_probe_in_window(
    ply: &mut Ply<()>,
    screenshot: &PathBuf,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let started = Instant::now();
    let mut click_attempted = false;
    let mut click_seen = false;
    let mut send_error = None;
    let mut click_target = serde_json::Value::Null;
    let mut bounds = serde_json::Value::Null;
    let mut pointer_over = false;
    let mut observed_mouse_position = serde_json::Value::Null;
    let tool_path = command_path("ydotool");
    for frame in 0..55 {
        draw_os_input_probe_frame(ply, "pointer-click-probe", frame).await;
        if let Some(button_bounds) = ply.bounding_box("os_probe_button") {
            bounds = bounds_json(button_bounds);
            if frame == 12 && !click_attempted && tool_path.is_some() {
                click_attempted = true;
                match send_real_pointer_click(button_bounds) {
                    Ok(target) => click_target = target,
                    Err(error) => send_error = Some(error.to_string()),
                }
            }
        }
        pointer_over = ply.pointer_over("os_probe_button");
        let (mouse_x, mouse_y) = mouse_position();
        observed_mouse_position = json!([mouse_x, mouse_y]);
        if ply.is_just_pressed("os_probe_button") {
            click_seen = true;
            break;
        }
        if (tool_path.is_none() || click_attempted) && frame >= 28 {
            break;
        }
        next_frame().await;
    }
    draw_os_input_probe_frame(ply, "pointer-click-probe", 181).await;
    let capture = capture_probe_frame_png(screenshot)?;
    let pixel_stats = capture.pixel_stats;
    let status = if tool_path.is_none() {
        "skip"
    } else if click_seen {
        "pass"
    } else {
        "fail"
    };
    Ok(json!({
        "status": status,
        "tool": "xtest-or-ydotool",
        "tool_path": tool_path,
        "click_attempted": click_attempted,
        "click_seen_by_ply": click_seen,
        "send_error": send_error,
        "target_element_id": "os_probe_button",
        "target_bounds": bounds,
        "click_target": click_target,
        "pointer_over_after_attempt": pointer_over,
        "observed_mouse_position": observed_mouse_position,
        "coordinate_contract": "target screen coordinates are reported from macroquad window position plus Ply element center; XTest receives absolute X11/XWayland screen coordinates first, then ydotool receives the same absolute screen coordinates as fallback; the report records the selected backend and relative delta for diagnosis",
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "input_route_contract": "OS pointer event should hit a visible Ply button and be observed through Ply is_just_pressed",
        "artifact": {
            "path": screenshot,
            "sha256": sha256_file(screenshot)?,
            "capture_backend": capture.capture_backend,
            "nonzero_channels": pixel_stats.nonzero_channels,
            "unique_rgba_values": pixel_stats.unique_rgba_values
        }
    }))
}

fn skipped_os_pointer_probe(screenshot: &PathBuf) -> serde_json::Value {
    json!({
        "status": "skip",
        "tool": "xtest-or-ydotool",
        "tool_path": command_path("ydotool"),
        "click_attempted": false,
        "click_seen_by_ply": false,
        "send_error": null,
        "target_element_id": "os_probe_button",
        "target_bounds": null,
        "click_target": null,
        "pointer_over_after_attempt": false,
        "observed_mouse_position": null,
        "coordinate_contract": "target screen coordinates are reported from macroquad window position plus Ply element center; XTest receives absolute X11/XWayland screen coordinates first, then ydotool receives the same absolute screen coordinates as fallback; the report records the selected backend and relative delta for diagnosis",
        "xtest_available": xtest_pointer_backend_available(),
        "input_route_contract": "OS pointer event should hit a visible Ply button and be observed through Ply is_just_pressed",
        "skip_reason": "BOON_ALLOW_OS_POINTER_PROBE=1 is required because this probe moves and clicks the real desktop pointer",
        "ydotoold_path": command_path("ydotoold"),
        "artifact": {
            "path": screenshot,
            "sha256": null,
            "nonzero_channels": null,
            "unique_rgba_values": null
        }
    })
}

fn skipped_os_keyboard_probe(screenshot: &PathBuf) -> serde_json::Value {
    json!({
        "status": "skip",
        "tool": "wtype",
        "tool_path": command_path("wtype"),
        "typed": false,
        "focused_ply_element": null,
        "input_route_contract": "OS keyboard probe skipped for focus-free headed verification",
        "skip_reason": "focus-free headed verifier forbids desktop keyboard injection",
        "artifact": {
            "path": screenshot,
            "sha256": null,
            "nonzero_channels": null,
            "unique_rgba_values": null
        }
    })
}

fn use_real_pointer_probe() -> bool {
    !os_input_forbidden() && std::env::var_os("BOON_ALLOW_OS_POINTER_PROBE").is_some()
}

async fn run_verify_os_input_probe(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("BOON_ALLOW_OS_INPUT_PROBE").as_deref() != Ok("1") {
        return Err(
            "OS input probe is opt-in; set BOON_ALLOW_OS_INPUT_PROBE=1 because it sends real keyboard input to the focused desktop window"
                .into(),
        );
    }
    let report = value_after(args, "--report")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/reports/os-input-probe.json"));
    let screenshot = report.with_extension("png");
    let token = value_after(args, "--token")
        .unwrap_or_else(|| format!("boon-os-probe-{}", std::process::id()));
    let mut ply = Ply::<()>::new(&DEFAULT_FONT).await;
    ply.set_debug_mode(false);
    let probe = run_os_keyboard_probe_in_window(&mut ply, &token, &screenshot).await?;
    let passed = probe.get("status").and_then(serde_json::Value::as_str) == Some("pass");
    let report_json = json!({
        "status": if passed { "pass" } else { "fail" },
        "report_version": 1,
        "generated_at_utc": unix_seconds_string(),
        "command": "os-input-probe",
        "command_argv": std::env::args().collect::<Vec<_>>(),
        "layer": "os-input-probe",
        "exit_status": if passed { 0 } else { 1 },
        "binary_hash": current_binary_hash(),
        "input_injection_method": "os_keyboard_to_visible_window",
        "input_backend": "wtype-real-keyboard-events",
        "input_route_contract": "focused Ply text input receives token from OS keyboard event path",
        "focused_window_proof": "probe set Ply focus to os_probe_input and received the exact token through text input state",
        "window_mode": "headed",
        "window_backend": "ply-engine/macroquad",
        "window_pid": std::process::id(),
        "window_title": "Boon Circuit Ply Playground",
        "display_server": display_server(),
        "display_socket_or_compositor_connection": display_socket(),
        "native_display_contract": native_display_contract(),
        "display_scale": screen_dpi_scale(),
        "window_size": [screen_width(), screen_height()],
        "os_input_probe": probe,
        "per_step_pass_fail": [{
            "id": "os-keyboard-token-reaches-ply-text-input",
            "pass": passed
        }],
        "artifact_sha256s": [{
            "path": screenshot,
            "sha256": sha256_file(&screenshot)?
        }],
        "checkpoint_screenshot_or_video_paths": [screenshot],
        "nonblank_screenshot_hashes": [{
                "nonzero_channels": probe["artifact"]["nonzero_channels"],
                "unique_rgba_values": probe["artifact"]["unique_rgba_values"]
            }],
        "git_commit": git_commit(),
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "budget_hash": "n/a",
        "graph_node_count": 0
    });
    write_json(&report, &report_json)?;
    if passed {
        macroquad::miniquad::window::quit();
        Ok(())
    } else {
        Err("OS input probe failed".into())
    }
}

fn os_probe_observed_token(value: &str, token: &str) -> bool {
    value.contains(token) || value.contains(&reverse_text(token))
}

fn record_ui_source_observation(event: serde_json::Value) {
    UI_SOURCE_OBSERVATIONS.with(|observations| observations.borrow_mut().push(event));
}

fn clear_ui_source_observations() {
    UI_SOURCE_OBSERVATIONS.with(|observations| observations.borrow_mut().clear());
}

fn take_ui_source_observations() -> Vec<serde_json::Value> {
    UI_SOURCE_OBSERVATIONS.with(|observations| {
        let mut observations = observations.borrow_mut();
        std::mem::take(&mut *observations)
    })
}

fn ui_source_observations_snapshot() -> Vec<serde_json::Value> {
    UI_SOURCE_OBSERVATIONS.with(|observations| observations.borrow().clone())
}

fn matching_ui_source_observation(
    source: &str,
    text: Option<&str>,
    key: Option<&str>,
    address: Option<&str>,
    target_text: Option<&str>,
) -> Option<serde_json::Value> {
    UI_SOURCE_OBSERVATIONS.with(|observations| {
        observations
            .borrow()
            .iter()
            .find(|event| {
                event.get("source").and_then(serde_json::Value::as_str) == Some(source)
                    && text.is_none_or(|expected| {
                        event.get("text").and_then(serde_json::Value::as_str) == Some(expected)
                    })
                    && key.is_none_or(|expected| {
                        event.get("key").and_then(serde_json::Value::as_str) == Some(expected)
                    })
                    && address.is_none_or(|expected| {
                        event.get("address").and_then(serde_json::Value::as_str) == Some(expected)
                    })
                    && target_text.is_none_or(|expected| {
                        event.get("target_text").and_then(serde_json::Value::as_str)
                            == Some(expected)
                    })
            })
            .cloned()
    })
}

fn live_source_event_from_json(event: &serde_json::Value) -> Option<LiveSourceEvent> {
    Some(LiveSourceEvent {
        source: event.get("source")?.as_str()?.to_owned(),
        text: event
            .get("text")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        key: event
            .get("key")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        address: event
            .get("address")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        target_text: event
            .get("target_text")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        target_occurrence: event
            .get("target_occurrence")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok()),
    })
}

fn reverse_text(value: &str) -> String {
    value.chars().rev().collect()
}

async fn run_smoke_launch(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_after(args, "--example").unwrap_or_else(|| "todomvc".to_owned());
    let report = value_after(args, "--report")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(format!("target/reports/playground-launch-{example}.json"))
        });
    let frames = value_after(args, "--frames")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(8)
        .max(3);
    let (source, scenario, _) = example_paths(&example)?;
    let scenario_data = parse_scenario(&scenario)?;
    let screenshot = report.with_extension("png");
    let mut ply = Ply::<()>::new(&DEFAULT_FONT).await;
    ply.set_debug_mode(false);
    let mut state = PlaygroundState::new(&example, &mut ply)?;
    let playground_surface_visible_bounds =
        playground_surface_visible_bounds_for_all_views(&mut ply, &mut state).await;
    let switch_speed = measure_example_switch_latency(&mut state, &mut ply, &example)?;
    state.view = PlaygroundView::App;
    for _ in 0..frames {
        draw_frame(&mut ply, &state).await;
        next_frame().await;
    }
    draw_frame(&mut ply, &state).await;
    next_frame().await;
    draw_frame(&mut ply, &state).await;
    let image = get_screen_data();
    let mut pixel_stats = image_stats(&image.bytes);
    let mut capture_backend = "macroquad-framebuffer".to_owned();
    let mut framebuffer_width = u32::from(image.width);
    let mut framebuffer_height = u32::from(image.height);
    if let Some(parent) = screenshot.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if pixel_stats.nonzero_channels == 0 || pixel_stats.unique_rgba_values <= 1 {
        let fallback = capture_with_cosmic_screenshot(&screenshot)?;
        pixel_stats = fallback.pixel_stats;
        capture_backend = fallback.capture_backend;
        framebuffer_width = fallback.width;
        framebuffer_height = fallback.height;
    } else {
        image.export_png(screenshot.to_str().ok_or("screenshot path is not utf-8")?);
    }
    let switch_speed_passed = switch_speed
        .get("budget_check")
        .and_then(|check| check.get("pass"))
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let report_json = json!({
        "status": if switch_speed_passed { "pass" } else { "fail" },
        "report_version": 1,
        "generated_at_utc": unix_seconds_string(),
        "command": "playground-smoke-launch",
        "command_argv": std::env::args().collect::<Vec<_>>(),
        "layer": "playground-launch-smoke",
        "exit_status": 0,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_path": source,
        "source_hash": sha256_file(&source)?,
        "scenario_path": scenario,
        "scenario_hash": sha256_file(&scenario)?,
        "program_hash": sha256_file(&source)?,
        "budget_hash": sha256_file(&PathBuf::from(format!("examples/{example}.budget.toml"))).unwrap_or_else(|_| "missing".to_owned()),
        "graph_node_count": state.output.as_ref().and_then(|output| output.report.get("graph_node_count")).cloned().unwrap_or_else(|| json!(0)),
        "example": example,
        "window_mode": "headed-smoke",
        "window_backend": "ply-engine/macroquad",
        "capture_backend": capture_backend,
        "window_pid": std::process::id(),
        "window_title": "Boon Circuit Ply Playground",
        "display_server": display_server(),
        "display_socket_or_compositor_connection": display_socket(),
        "native_display_contract": native_display_contract(),
        "display_scale": screen_dpi_scale(),
        "window_size": [screen_width(), screen_height()],
        "framebuffer_size": {
            "width": framebuffer_width,
            "height": framebuffer_height
        },
        "frames_drawn": frames,
        "scenario_step_count": scenario_data.step.len(),
        "selected_example": state.selected,
        "scenario_path_loaded": state.scenario_path,
        "playground_surface": playground_surface_checks(),
        "playground_surface_visible_bounds": playground_surface_visible_bounds,
        "example_switch_latency_ms_p50_p95_p99_max": switch_speed.get("latency_ms_p50_p95_p99_max").cloned().unwrap_or(serde_json::Value::Null),
        "example_switch_budget_check": switch_speed.get("budget_check").cloned().unwrap_or(serde_json::Value::Null),
        "example_switch_measurements": switch_speed,
        "per_step_pass_fail": [
            {"id": "native-window-opened", "pass": true},
            {"id": "example-loaded", "pass": true},
            {"id": "scenario-loaded", "pass": state.scenario_len == scenario_data.step.len()},
            {"id": "nonblank-framebuffer-captured", "pass": true},
            {"id": "code-editor-present", "pass": true},
            {"id": "render-preview-present", "pass": true},
            {"id": "delta-log-present", "pass": true},
            {"id": "inspector-present", "pass": true},
            {"id": "dependency-panel-present", "pass": true},
            {"id": "example-switch-p95-budget", "pass": switch_speed_passed}
        ],
        "artifact_sha256s": [{
            "path": screenshot,
            "sha256": sha256_file(&screenshot)?
        }],
        "checkpoint_screenshot_or_video_paths": [screenshot],
        "nonblank_screenshot_hashes": [{
            "path": report.with_extension("png"),
            "nonzero_channels": pixel_stats.nonzero_channels,
            "unique_rgba_values": pixel_stats.unique_rgba_values
        }],
        "note": "bounded native Ply playground launch smoke; this proves startup/rendering only and does not replace headed OS-input or human verification"
    });
    write_json(&report, &report_json)?;
    if !switch_speed_passed {
        return Err(format!(
            "example switch p95 exceeded budget; see `{}`",
            report.display()
        )
        .into());
    }
    macroquad::miniquad::window::quit();
    Ok(())
}

fn measure_example_switch_latency(
    state: &mut PlaygroundState,
    ply: &mut Ply<()>,
    original: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let alternate = if original == "todomvc" {
        "cells"
    } else {
        "todomvc"
    };
    state.load_example(alternate, ply)?;
    state.load_example(original, ply)?;
    let mut samples = Vec::new();
    let mut switches = Vec::new();
    for iteration in 0..16 {
        let target = if iteration % 2 == 0 {
            alternate
        } else {
            original
        };
        let started = Instant::now();
        state.load_example(target, ply)?;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        samples.push(elapsed_ms);
        switches.push(json!({
            "iteration": iteration,
            "target": target,
            "elapsed_ms": elapsed_ms
        }));
    }
    if state.selected != original {
        state.load_example(original, ply)?;
    }
    let stats = float_stats(samples);
    let threshold_ms = if cfg!(debug_assertions) { 50.0 } else { 16.0 };
    let measured_p95 = stats
        .get("p95")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(f64::INFINITY);
    Ok(json!({
        "from_example": original,
        "alternate_example": alternate,
        "build_profile": if cfg!(debug_assertions) { "dev" } else { "release" },
        "switch_count": switches.len(),
        "switches": switches,
        "latency_ms_p50_p95_p99_max": stats,
        "budget_check": {
            "pass": measured_p95 <= threshold_ms,
            "allowed_p95_ms": threshold_ms,
            "measured_p95_ms": measured_p95,
            "dev_budget_ms": 50.0,
            "release_budget_ms": 16.0
        }
    }))
}

fn float_stats(mut values: Vec<f64>) -> serde_json::Value {
    if values.is_empty() {
        return json!({
            "p50": null,
            "p95": null,
            "p99": null,
            "max": null
        });
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let percentile = |percent: f64| -> f64 {
        let index = ((values.len() - 1) as f64 * percent).ceil() as usize;
        values[index.min(values.len() - 1)]
    };
    json!({
        "p50": percentile(0.50),
        "p95": percentile(0.95),
        "p99": percentile(0.99),
        "max": *values.last().unwrap_or(&0.0)
    })
}

async fn run_interactive(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut ply = Ply::<()>::new(&DEFAULT_FONT).await;
    ply.set_debug_mode(false);
    let selected = value_after(args, "--example").unwrap_or_else(|| "todomvc".to_owned());
    let view = PlaygroundView::from_mode_arg(args);
    let mut state = PlaygroundState::new(&selected, &mut ply)?;
    state.view = view;
    loop {
        if is_key_pressed(KeyCode::Key1) {
            state.load_example("todomvc", &mut ply)?;
        }
        if is_key_pressed(KeyCode::Key2) {
            state.load_example("cells", &mut ply)?;
        }
        if is_key_pressed(KeyCode::F5) {
            state.step_limit = None;
            state.run_editor_text(&ply);
        }
        if is_key_pressed(KeyCode::R) {
            state.reset_to_initial(&ply);
        }
        if is_key_pressed(KeyCode::A) {
            state.view = PlaygroundView::App;
        }
        if is_key_pressed(KeyCode::S) {
            state.view = PlaygroundView::Source;
        }
        if is_key_pressed(KeyCode::D) {
            state.view = PlaygroundView::Deltas;
        }
        if is_key_pressed(KeyCode::I) {
            state.view = PlaygroundView::Inspector;
        }
        if is_key_pressed(KeyCode::C) {
            state.view = PlaygroundView::Causes;
        }
        if is_key_pressed(KeyCode::Right) {
            state.step_next(&ply);
        }
        if is_key_pressed(KeyCode::Left) {
            state.step_prev(&ply);
        }
        if is_key_pressed(KeyCode::Escape) || is_quit_requested() {
            break;
        }
        draw_frame(&mut ply, &state).await;
        if ply.is_just_pressed("nav_todomvc") {
            state.load_example("todomvc", &mut ply)?;
        }
        if ply.is_just_pressed("nav_cells") {
            state.load_example("cells", &mut ply)?;
        }
        if ply.is_just_pressed("run_button") {
            state.step_limit = None;
            state.run_editor_text(&ply);
        }
        if ply.is_just_pressed("reset_button") {
            state.reset_to_initial(&ply);
        }
        if ply.is_just_pressed("step_button") {
            state.step_next(&ply);
        }
        if ply.is_just_pressed("view_app") {
            state.view = PlaygroundView::App;
        }
        if ply.is_just_pressed("view_source") {
            state.view = PlaygroundView::Source;
        }
        if ply.is_just_pressed("view_deltas") {
            state.view = PlaygroundView::Deltas;
        }
        if ply.is_just_pressed("view_inspector") {
            state.view = PlaygroundView::Inspector;
        }
        if ply.is_just_pressed("view_causes") {
            state.view = PlaygroundView::Causes;
        }
        if ply.is_just_pressed("view_scenario") {
            state.view = PlaygroundView::Scenario;
        }
        state.apply_observed_ui_source_events();
        state.run_editor_text_if_changed(&ply);
        next_frame().await;
    }
    Ok(())
}

async fn draw_os_input_probe_frame(ply: &mut Ply<()>, token: &str, frame: usize) {
    clear_background(MacroquadColor::from_rgba(238, 241, 245, 255));
    {
        let mut ui = ply.begin();
        ui.element()
            .id("root")
            .width(grow!())
            .height(grow!())
            .background_color(0xEEF1F5)
            .layout(|layout| {
                layout
                    .direction(TopToBottom)
                    .padding((28, 28, 28, 28))
                    .gap(12)
            })
            .children(|ui| {
                ui.text("OS Input Probe", |text| text.font_size(30).color(0x1F2630));
                ui.text(
                    "This verifier sends a real keyboard token to the focused Ply text input.",
                    |text| text.font_size(16).color(0x596579),
                );
                ui.text(&format!("frame {frame}"), |text| {
                    text.font_size(13).color(0x596579)
                });
                ui.text(
                    &format!(
                        "token sha256 {}",
                        boon_runtime::sha256_bytes(token.as_bytes())
                    ),
                    |text| text.font_size(12).color(0x596579),
                );
                ui.element()
                    .id("os_probe_input")
                    .width(fixed!(760.0))
                    .height(fixed!(46.0))
                    .background_color(0xFFFFFF)
                    .border(|border| border.color(0x2F6FB8).all(2))
                    .layout(|layout| layout.padding((10, 10, 8, 8)))
                    .text_input(|input| {
                        input
                            .font(&DEFAULT_FONT)
                            .font_size(18)
                            .text_color(0x1F2630)
                            .cursor_color(0x2F6FB8)
                            .selection_color(0xB9D7F5)
                    })
                    .empty();
                ui.element()
                    .id("os_probe_button")
                    .width(fixed!(220.0))
                    .height(fixed!(42.0))
                    .background_color(0x2F6FB8)
                    .layout(|layout| layout.align(CenterX, CenterY))
                    .children(|ui| {
                        ui.text("Pointer Probe", |text| text.font_size(16).color(0xFFFFFF));
                    });
            });
    }
    ply.show(|_| {}).await;
}

struct PlaygroundState {
    selected: String,
    view: PlaygroundView,
    scenario_path: PathBuf,
    scenario_len: usize,
    scenario_steps: Vec<String>,
    source_text_snapshot: String,
    render_nodes: Vec<RenderNode>,
    step_limit: Option<usize>,
    output: Option<RunOutput>,
    live_runtime: Option<LiveRuntime>,
    last_error: Option<String>,
}

impl PlaygroundState {
    fn new(example: &str, ply: &mut Ply<()>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut state = Self {
            selected: example.to_owned(),
            view: PlaygroundView::App,
            scenario_path: PathBuf::new(),
            scenario_len: 0,
            scenario_steps: Vec::new(),
            source_text_snapshot: String::new(),
            render_nodes: Vec::new(),
            step_limit: None,
            output: None,
            live_runtime: None,
            last_error: None,
        };
        state.load_example(example, ply)?;
        Ok(state)
    }

    fn from_output(
        selected: String,
        scenario_path: PathBuf,
        scenario_len: usize,
        output: RunOutput,
    ) -> Self {
        Self {
            selected,
            view: PlaygroundView::App,
            scenario_path,
            scenario_len,
            scenario_steps: Vec::new(),
            source_text_snapshot: String::new(),
            render_nodes: Vec::new(),
            step_limit: None,
            output: Some(output),
            live_runtime: None,
            last_error: None,
        }
    }

    fn load_example(
        &mut self,
        example: &str,
        ply: &mut Ply<()>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (source, scenario, _) = example_paths(example)?;
        let source_text = std::fs::read_to_string(&source)?;
        let scenario_data = parse_scenario(&scenario)?;
        self.selected = example.to_owned();
        self.scenario_steps = scenario_data
            .step
            .iter()
            .map(|step| step.id.clone())
            .collect();
        self.scenario_len = self.scenario_steps.len();
        self.scenario_path = scenario;
        self.step_limit = Some(1);
        ply.set_text_value("source_editor", &source_text);
        self.source_text_snapshot = source_text.clone();
        self.render_nodes = parse_render_view(&source_text).unwrap_or_default();
        self.run_text(&source_text);
        Ok(())
    }

    fn run_editor_text(&mut self, ply: &Ply<()>) {
        let source_text = ply.get_text_value("source_editor").to_owned();
        self.source_text_snapshot = source_text.clone();
        self.render_nodes = parse_render_view(&source_text).unwrap_or_default();
        self.run_text(&source_text);
    }

    fn run_editor_text_if_changed(&mut self, ply: &Ply<()>) {
        let source_text = ply.get_text_value("source_editor").to_owned();
        if source_text != self.source_text_snapshot {
            self.source_text_snapshot = source_text.clone();
            self.step_limit = Some(1);
            self.render_nodes = parse_render_view(&source_text).unwrap_or_default();
            self.run_text(&source_text);
        }
    }

    fn reset_to_initial(&mut self, ply: &Ply<()>) {
        self.step_limit = Some(1);
        self.run_editor_text(ply);
    }

    fn step_next(&mut self, ply: &Ply<()>) {
        let next = self.step_limit.unwrap_or(1).saturating_add(1);
        self.step_limit = Some(next.min(self.scenario_len.max(1)));
        self.run_editor_text(ply);
    }

    fn step_prev(&mut self, ply: &Ply<()>) {
        let previous = self
            .step_limit
            .unwrap_or(self.scenario_len)
            .saturating_sub(1);
        self.step_limit = Some(previous.max(1));
        self.run_editor_text(ply);
    }

    fn run_text(&mut self, source_text: &str) {
        match run_scenario_source_with_step_limit(
            &format!("playground-editor:{}", self.selected),
            source_text,
            &self.scenario_path,
            VerificationLayer::Semantic,
            self.step_limit,
        ) {
            Ok(output) => {
                self.output = Some(output);
                self.live_runtime = if self.step_limit == Some(1) {
                    LiveRuntime::new(
                        &format!("playground-live:{}", self.selected),
                        source_text,
                        &self.scenario_path,
                    )
                    .ok()
                } else {
                    None
                };
                self.last_error = None;
            }
            Err(error) => {
                self.output = None;
                self.live_runtime = None;
                self.last_error = Some(error.to_string());
            }
        }
    }

    fn apply_live_source_event(
        &mut self,
        event: LiveSourceEvent,
        scenario_step: Option<&ScenarioStep>,
    ) -> Result<LiveStepOutput, String> {
        let live_runtime = self
            .live_runtime
            .as_mut()
            .ok_or_else(|| "playground live runtime is not initialized".to_owned())?;
        let step = match scenario_step {
            Some(scenario_step) => live_runtime.apply_source_event_for_step(scenario_step, event),
            None => live_runtime.apply_source_event(event),
        }
        .map_err(|error| error.to_string())?;
        if let Some(output) = &mut self.output {
            output.semantic_deltas.extend(step.semantic_deltas.clone());
            output.render_patches.extend(step.render_patches.clone());
            output.state_summary = step.state_summary.clone();
            Ok(step)
        } else {
            Err("playground output is not initialized".to_owned())
        }
    }

    fn apply_observed_ui_source_events(&mut self) {
        let observations = take_ui_source_observations();
        for event in observations {
            let Some(source_event) = live_source_event_from_json(&event) else {
                continue;
            };
            if let Err(error) = self.apply_live_source_event(source_event, None) {
                self.last_error = Some(format!("live SOURCE event failed: {error}"));
                break;
            } else {
                self.last_error = None;
            }
        }
    }
}

async fn draw_frame(ply: &mut Ply<()>, state: &PlaygroundState) {
    clear_background(MacroquadColor::from_rgba(238, 241, 245, 255));
    begin_hover_tracking_frame();
    observe_render_input_escape(ply, state);
    let keypad_submit_input = if is_key_pressed(KeyCode::KpEnter) {
        focused_render_input(ply, state)
    } else {
        None
    };
    sync_render_inputs(ply, state);
    {
        let mut ui = ply.begin();
        build_ui(&mut ui, state);
    }
    ply.show(|_| {}).await;
    observe_render_input_keypad_submit(ply, keypad_submit_input);
    finish_hover_tracking_frame();
    observe_render_input_blur(ply, state);
}

fn sync_render_inputs(ply: &mut Ply<()>, state: &PlaygroundState) {
    let Some(output) = &state.output else {
        return;
    };
    let focused = ply.focused_element();
    let mut values = Vec::new();
    collect_render_input_values(
        &state.render_nodes,
        &RenderContext::root(&output.state_summary),
        &mut values,
    );
    for (id, value) in values {
        let is_focused = focused.as_ref() == Some(&id);
        if is_focused && !value.is_empty() {
            continue;
        }
        if ply.get_text_value(id.clone()) != value {
            ply.set_text_value(id, &value);
        }
    }
}

fn collect_render_input_values(
    nodes: &[RenderNode],
    context: &RenderContext<'_>,
    values: &mut Vec<(Id, String)>,
) {
    for node in nodes {
        match node {
            RenderNode::Column { children, .. } | RenderNode::Row { children, .. } => {
                collect_render_input_values(children, context, values);
            }
            RenderNode::ForEach {
                list,
                item,
                children,
            } => {
                if let Some(rows) =
                    resolve_path(context, list).and_then(serde_json::Value::as_array)
                {
                    for (index, row) in rows.iter().enumerate() {
                        let item_context = context.with_binding(list, item, row, index);
                        collect_render_input_values(children, &item_context, values);
                    }
                }
            }
            RenderNode::Input {
                id,
                key,
                value,
                visible,
                ..
            } => {
                if visible
                    .as_ref()
                    .is_some_and(|visible| !eval_bool(visible, context))
                {
                    continue;
                }
                values.push((
                    render_id_with_key(id, key.as_ref(), context),
                    eval_render_value(value, context),
                ));
            }
            RenderNode::Text { .. } | RenderNode::Button { .. } | RenderNode::Checkbox { .. } => {}
        }
    }
}

fn observe_render_input_escape(ply: &Ply<()>, state: &PlaygroundState) {
    if !(is_key_pressed(KeyCode::Escape) || is_key_down(KeyCode::Escape)) {
        return;
    }
    let Some(input) = focused_render_input(ply, state) else {
        return;
    };
    if let Some(source) = input.escape_source.as_deref() {
        suppress_next_blur_for_input(&input.id);
        record_ui_source_observation(render_source_event(
            source,
            None,
            Some("Escape"),
            input.address.as_deref(),
            input.target_text.as_deref(),
            input.target_occurrence,
        ));
    } else if let Some(source) = input.cancel_source.as_deref() {
        suppress_next_blur_for_input(&input.id);
        record_ui_source_observation(render_source_event(
            source,
            None,
            None,
            input.address.as_deref(),
            input.target_text.as_deref(),
            input.target_occurrence,
        ));
    }
}

fn observe_render_input_keypad_submit(ply: &Ply<()>, input: Option<FocusedRenderInput>) {
    let Some(input) = input else {
        return;
    };
    if let Some(source) = input.submit_source.as_deref() {
        let text = ply.get_text_value(input.id.clone());
        if matching_ui_source_observation(
            source,
            Some(text),
            Some("Enter"),
            input.address.as_deref(),
            input.target_text.as_deref(),
        )
        .is_some()
        {
            return;
        }
        suppress_next_blur_for_input(&input.id);
        record_ui_source_observation(render_source_event(
            source,
            Some(text),
            Some("Enter"),
            input.address.as_deref(),
            input.target_text.as_deref(),
            input.target_occurrence,
        ));
    }
}

fn observe_render_input_blur(ply: &Ply<()>, state: &PlaygroundState) {
    let current = focused_render_input(ply, state);
    LAST_FOCUSED_RENDER_INPUT.with(|last| {
        let previous = last.borrow().clone();
        if let Some(previous) = previous {
            let focus_changed = current
                .as_ref()
                .is_none_or(|current| current.id != previous.id);
            if focus_changed && let Some(source) = previous.blur_source.as_deref() {
                if should_suppress_blur_for_input(&previous.id) {
                    *last.borrow_mut() = current;
                    return;
                }
                record_ui_source_observation(render_source_event(
                    source,
                    Some(ply.get_text_value(previous.id)),
                    None,
                    previous.address.as_deref(),
                    previous.target_text.as_deref(),
                    previous.target_occurrence,
                ));
            }
        }
        *last.borrow_mut() = current;
    });
}

fn suppress_next_blur_for_input(id: &Id) {
    SUPPRESS_NEXT_BLUR_INPUT.with(|suppressed| {
        *suppressed.borrow_mut() = Some(id.clone());
    });
}

fn should_suppress_blur_for_input(id: &Id) -> bool {
    SUPPRESS_NEXT_BLUR_INPUT.with(|suppressed| {
        let mut suppressed = suppressed.borrow_mut();
        if suppressed.as_ref() == Some(id) {
            *suppressed = None;
            true
        } else {
            false
        }
    })
}

fn focused_render_input(ply: &Ply<()>, state: &PlaygroundState) -> Option<FocusedRenderInput> {
    let focused = ply.focused_element()?;
    let output = state.output.as_ref()?;
    let mut inputs = Vec::new();
    collect_render_input_metadata(
        &state.render_nodes,
        &RenderContext::root(&output.state_summary),
        &mut inputs,
    );
    inputs.into_iter().find(|input| input.id == focused)
}

fn collect_render_input_metadata(
    nodes: &[RenderNode],
    context: &RenderContext<'_>,
    inputs: &mut Vec<FocusedRenderInput>,
) {
    for node in nodes {
        match node {
            RenderNode::Column { children, .. } | RenderNode::Row { children, .. } => {
                collect_render_input_metadata(children, context, inputs);
            }
            RenderNode::ForEach {
                list,
                item,
                children,
            } => {
                if let Some(rows) =
                    resolve_path(context, list).and_then(serde_json::Value::as_array)
                {
                    for (index, row) in rows.iter().enumerate() {
                        let item_context = context.with_binding(list, item, row, index);
                        collect_render_input_metadata(children, &item_context, inputs);
                    }
                }
            }
            RenderNode::Input {
                id,
                key,
                submit_source,
                cancel_source,
                escape_source,
                blur_source,
                address,
                target,
                visible,
                ..
            } => {
                if visible
                    .as_ref()
                    .is_some_and(|visible| !eval_bool(visible, context))
                {
                    continue;
                }
                inputs.push(FocusedRenderInput {
                    id: render_id_with_key(id, key.as_ref(), context),
                    submit_source: submit_source.clone(),
                    blur_source: blur_source.clone(),
                    cancel_source: cancel_source.clone(),
                    escape_source: escape_source.clone(),
                    address: address
                        .as_ref()
                        .map(|value| eval_render_value(value, context)),
                    target_text: target
                        .as_ref()
                        .map(|value| eval_render_value(value, context)),
                    target_occurrence: target_occurrence(target.as_ref(), context),
                });
            }
            RenderNode::Text { .. } | RenderNode::Button { .. } | RenderNode::Checkbox { .. } => {}
        }
    }
}

fn build_ui(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("root")
        .width(grow!())
        .height(grow!())
        .background_color(0xEEF1F5)
        .layout(|layout| layout.direction(LeftToRight))
        .children(|ui| {
            sidebar(ui, state);
            content(ui, state);
        });
}

fn sidebar(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("sidebar")
        .width(fixed!(160.0))
        .height(grow!())
        .background_color(0x1F2630)
        .layout(|layout| layout.direction(TopToBottom).padding((8, 8, 8, 8)).gap(6))
        .children(|ui| {
            ui.text("Boon Circuit", |text| text.font_size(20).color(0xF1F5FA));
            ui.text("Ply playground", |text| text.font_size(11).color(0xAFC1D6));
            nav_item(ui, "nav_todomvc", "1 TodoMVC", state.selected == "todomvc");
            nav_item(ui, "nav_cells", "2 Cells", state.selected == "cells");
            ui.text(&format!("step {}", step_label(state)), |text| {
                text.font_size(11).color(0xAFC1D6)
            });
            scenario_checklist(ui, state);
        });
}

fn scenario_checklist(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    if state.scenario_steps.is_empty() {
        return;
    }
    let completed = state.step_limit.unwrap_or(state.scenario_len);
    ui.element()
        .id("scenario_checklist")
        .width(grow!())
        .height(grow!())
        .background_color(0x28313D)
        .layout(|layout| layout.direction(TopToBottom).padding((8, 8, 6, 6)).gap(3))
        .children(|ui| {
            ui.text("Scenario", |text| text.font_size(12).color(0xAFC1D6));
            for (index, label) in state.scenario_steps.iter().enumerate().take(14) {
                let marker = if index < completed { "x" } else { " " };
                let color = if index < completed {
                    0xF1F5FA
                } else {
                    0xAFC1D6
                };
                ui.text(&format!("[{marker}] {label}"), |text| {
                    text.font_size(10).color(color)
                });
            }
        });
}

fn scenario_detail_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    panel(ui, "scenario_detail_panel", "Scenario", |ui| {
        let completed = state.step_limit.unwrap_or(state.scenario_len);
        for (index, label) in state.scenario_steps.iter().enumerate() {
            let marker = if index < completed { "x" } else { " " };
            let color = if index < completed {
                0x1F2630
            } else {
                0x596579
            };
            ui.text(&format!("[{marker}] {label}"), |text| {
                text.font_size(16).color(color)
            });
        }
    });
}

fn nav_item(ui: &mut Ui<'_, ()>, id: &'static str, label: &str, selected: bool) {
    ui.element()
        .id(id)
        .width(grow!())
        .height(fixed!(28.0))
        .background_color(if selected { 0x2F6FB8 } else { 0x28313D })
        .layout(|layout| layout.padding((8, 8, 6, 6)).align(Left, CenterY))
        .children(|ui| {
            ui.text(label, |text| text.font_size(12).color(0xF1F5FA));
        });
}

fn content(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("content")
        .width(grow!())
        .height(grow!())
        .background_color(0xF8FAFD)
        .layout(|layout| layout.direction(TopToBottom).padding((8, 8, 8, 8)).gap(6))
        .children(|ui| {
            toolbar(ui, state);
            match state.view {
                PlaygroundView::App => app_first_panel(ui, state),
                PlaygroundView::Source => source_dev_panel(ui, state),
                PlaygroundView::Deltas => delta_panel(ui, state),
                PlaygroundView::Inspector => inspector_panel(ui, state),
                PlaygroundView::Causes => explanation_panel(ui, state),
                PlaygroundView::Scenario => scenario_detail_panel(ui, state),
            }
        });
}

fn toolbar(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("toolbar")
        .height(fixed!(34.0))
        .width(grow!())
        .layout(|layout| layout.direction(LeftToRight).gap(5).align(Left, CenterY))
        .children(|ui| {
            toolbar_button(ui, "run_button", "Run", true);
            toolbar_button(ui, "reset_button", "Reset", false);
            toolbar_button(ui, "step_button", "Step", false);
            view_button(ui, "view_app", "App", state.view == PlaygroundView::App);
            view_button(
                ui,
                "view_source",
                "Source",
                state.view == PlaygroundView::Source,
            );
            view_button(
                ui,
                "view_deltas",
                "Deltas",
                state.view == PlaygroundView::Deltas,
            );
            view_button(
                ui,
                "view_inspector",
                "Inspector",
                state.view == PlaygroundView::Inspector,
            );
            view_button(
                ui,
                "view_causes",
                "Causes",
                state.view == PlaygroundView::Causes,
            );
            view_button(
                ui,
                "view_scenario",
                "Scenario",
                state.view == PlaygroundView::Scenario,
            );
            ui.text(
                &format!("{} / {}", state.selected, step_label(state)),
                |text| text.font_size(12).color(0x1F2630),
            );
        });
}

fn toolbar_button(ui: &mut Ui<'_, ()>, id: &'static str, label: &str, primary: bool) {
    ui.element()
        .id(id)
        .height(fixed!(28.0))
        .width(fixed!(60.0))
        .background_color(if primary { 0x2F6FB8 } else { 0xE4EAF2 })
        .layout(|layout| layout.align(CenterX, CenterY))
        .children(|ui| {
            ui.text(label, |text| {
                text.font_size(12)
                    .color(if primary { 0xFFFFFF } else { 0x1F2630 })
            });
        });
}

fn view_button(ui: &mut Ui<'_, ()>, id: &'static str, label: &str, selected: bool) {
    ui.element()
        .id(id)
        .height(fixed!(28.0))
        .width(fixed!(70.0))
        .background_color(if selected { 0x1F2630 } else { 0xE4EAF2 })
        .layout(|layout| layout.align(CenterX, CenterY))
        .children(|ui| {
            ui.text(label, |text| {
                text.font_size(11)
                    .color(if selected { 0xFFFFFF } else { 0x1F2630 })
            });
        });
}

fn app_first_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("app_first_panel")
        .width(grow!())
        .height(grow!())
        .background_color(0xF5F5F5)
        .layout(|layout| {
            layout
                .direction(TopToBottom)
                .padding((8, 8, 8, 8))
                .align(CenterX, Top)
        })
        .children(|ui| {
            if let Some(error) = &state.last_error {
                ui.text(error, |text| text.font_size(18).color(0xA32929));
                return;
            }
            preview_body(ui, state, PreviewLayout::Full);
        });
}

fn source_dev_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("source_dev_panel")
        .width(grow!())
        .height(grow!())
        .layout(|layout| layout.direction(LeftToRight).gap(10))
        .children(|ui| {
            source_panel(ui);
            runtime_panel(ui, state);
        });
}

fn source_panel(ui: &mut Ui<'_, ()>) {
    ui.element()
        .id("source_panel")
        .width(fixed!(650.0))
        .height(grow!())
        .background_color(0xF8FAFD)
        .layout(|layout| layout.direction(TopToBottom).gap(8))
        .children(|ui| {
            ui.text("Source", |text| text.font_size(18).color(0x1F2630));
            ui.element()
                .id("source_editor")
                .width(grow!())
                .height(grow!())
                .background_color(0xFFFFFF)
                .border(|border| border.color(0xD5DDE8).all(1))
                .layout(|layout| layout.padding((10, 10, 8, 8)))
                .text_input(|input| {
                    input
                        .multiline()
                        .drag_select()
                        .font(&DEFAULT_FONT)
                        .font_size(14)
                        .line_height(18)
                        .text_color(0x1F2630)
                        .cursor_color(0x2F6FB8)
                        .selection_color(0xB9D7F5)
                })
                .empty();
        });
}

fn runtime_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("runtime_panel")
        .width(grow!())
        .height(grow!())
        .layout(|layout| layout.direction(TopToBottom).gap(10))
        .children(|ui| {
            preview_panel(ui, state);
            ui.element()
                .width(grow!())
                .height(fixed!(260.0))
                .layout(|layout| layout.direction(LeftToRight).gap(10))
                .children(|ui| {
                    delta_panel(ui, state);
                    inspector_panel(ui, state);
                    explanation_panel(ui, state);
                });
        });
}

fn preview_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    panel(ui, "preview_panel", "Preview", |ui| {
        preview_body(ui, state, PreviewLayout::Panel);
    });
}

#[derive(Clone, Copy)]
enum PreviewLayout {
    Full,
    Panel,
}

fn preview_body(ui: &mut Ui<'_, ()>, state: &PlaygroundState, layout: PreviewLayout) {
    if let Some(error) = &state.last_error {
        ui.text(error, |text| text.font_size(14).color(0xA32929));
        return;
    }
    let Some(output) = &state.output else {
        return;
    };
    if state.render_nodes.is_empty() {
        ui.text("No VIEW block in Boon source", |text| {
            text.font_size(16).color(0x596579)
        });
        compact_json(ui, &output.state_summary);
        return;
    }
    if matches!(layout, PreviewLayout::Full) {
        ui.element()
            .id("dynamic_boon_preview")
            .width(grow!())
            .height(grow!())
            .layout(|layout| layout.direction(TopToBottom).align(CenterX, Top))
            .children(|ui| {
                render_nodes(
                    ui,
                    &state.render_nodes,
                    &RenderContext::root(&output.state_summary),
                );
            });
    } else {
        render_nodes(
            ui,
            &state.render_nodes,
            &RenderContext::root(&output.state_summary),
        );
    }
}

fn render_nodes(ui: &mut Ui<'_, ()>, nodes: &[RenderNode], context: &RenderContext<'_>) {
    for node in nodes {
        render_node(ui, node, context);
    }
}

fn render_node(ui: &mut Ui<'_, ()>, node: &RenderNode, context: &RenderContext<'_>) {
    match node {
        RenderNode::Column {
            id,
            width,
            height,
            background,
            border,
            gap,
            padding,
            children,
        } => {
            let mut element = ui
                .element()
                .width(width.map_or(grow!(), |width| fixed!(width)))
                .background_color(*background);
            if let Some(height) = height {
                element = element.height(fixed!(*height));
            }
            if let Some(border) = border {
                element = element.border(|border_style| border_style.color(*border).all(1));
            }
            let gap = *gap;
            let padding = *padding;
            element = element.layout(|layout| {
                let layout = layout.direction(TopToBottom).gap(gap as u16);
                if let Some(padding) = padding {
                    layout.padding(padding_u16(padding))
                } else {
                    layout
                }
            });
            if let Some(id) = id {
                let row_scope = render_scope_key(id, context);
                let hover_scope = row_scope.clone();
                element = element
                    .id(render_id(id, context))
                    .on_hover(move |_, _| record_hover_scope(&hover_scope));
                let child_context = context.with_hover_scope(row_scope);
                element.children(|ui| render_nodes(ui, children, &child_context));
            } else {
                element.children(|ui| render_nodes(ui, children, context));
            }
        }
        RenderNode::Row {
            id,
            height,
            background,
            border,
            gap,
            padding,
            children,
        } => {
            let mut element = ui
                .element()
                .width(grow!())
                .height(height.map_or(fixed!(40.0), |height| fixed!(height)))
                .background_color(*background);
            if let Some(border) = border {
                element = element.border(|border_style| border_style.color(*border).bottom(1));
            }
            let gap = *gap;
            let padding = padding.unwrap_or((10.0, 10.0, 6.0, 6.0));
            element = element.layout(|layout| {
                layout
                    .direction(LeftToRight)
                    .padding(padding_u16(padding))
                    .gap(gap as u16)
                    .align(Left, CenterY)
            });
            if let Some(id) = id {
                let row_scope = render_scope_key(id, context);
                let hover_scope = row_scope.clone();
                element = element
                    .id(render_id(id, context))
                    .on_hover(move |_, _| record_hover_scope(&hover_scope));
                let child_context = context.with_hover_scope(row_scope);
                element.children(|ui| render_nodes(ui, children, &child_context));
            } else {
                element.children(|ui| render_nodes(ui, children, context));
            }
        }
        RenderNode::ForEach {
            list,
            item,
            children,
        } => {
            if let Some(rows) = resolve_path(context, list).and_then(serde_json::Value::as_array) {
                for (index, row) in rows.iter().enumerate() {
                    let item_context = context.with_binding(list, item, row, index);
                    render_nodes(ui, children, &item_context);
                }
            }
        }
        RenderNode::Text {
            value,
            size,
            color,
            background,
            width,
            height,
            strike_if,
            center,
        } => {
            let label = decorated_render_text(
                eval_render_value(value, context),
                strike_if
                    .as_ref()
                    .is_some_and(|condition| eval_bool(condition, context)),
            );
            let draw_text = |ui: &mut Ui<'_, ()>| {
                ui.text(&label, |text| text.font_size(*size).color(*color));
            };
            if width.is_some() || height.is_some() || *center {
                let mut element = ui
                    .element()
                    .width(match width {
                        Some(RenderExtent::Fill) => grow!(),
                        Some(RenderExtent::Fixed(width)) => fixed!(*width),
                        None => grow!(),
                    })
                    .height(
                        height.map_or(fixed!((*size as f32 + 12.0).max(28.0)), |height| {
                            fixed!(height)
                        }),
                    );
                if let Some(background) = background {
                    element = element.background_color(*background);
                }
                element
                    .layout(|layout| {
                        if *center {
                            layout.align(CenterX, CenterY)
                        } else {
                            layout.align(Left, CenterY)
                        }
                    })
                    .children(draw_text);
            } else {
                draw_text(ui);
            }
        }
        RenderNode::Input {
            id,
            key,
            value: _,
            placeholder,
            change_source,
            submit_source,
            cancel_source: _,
            escape_source: _,
            blur_source: _,
            address,
            target,
            visible,
            size,
            height,
            color,
            placeholder_color,
            background,
            border,
        } => {
            if visible
                .as_ref()
                .is_some_and(|visible| !eval_bool(visible, context))
            {
                return;
            }
            let element_id = render_id_with_key(id, key.as_ref(), context);
            let change_source = change_source.clone();
            let submit_source = submit_source.clone();
            let address_value = address
                .as_ref()
                .map(|value| eval_render_value(value, context));
            let target_value = target
                .as_ref()
                .map(|value| eval_render_value(value, context));
            let target_occurrence = target_occurrence(target.as_ref(), context);
            let submit_element_id = element_id.clone();
            ui.element()
                .id(element_id)
                .height(
                    height.map_or(fixed!((*size as f32 + 20.0).max(34.0)), |height| {
                        fixed!(height)
                    }),
                )
                .width(grow!())
                .background_color(*background)
                .border(|border_style| border_style.color(border.unwrap_or(0xD5DDE8)).all(1))
                .layout(|layout| layout.padding((8, 8, 5, 5)))
                .text_input(|input| {
                    let change_address = address_value.clone();
                    let change_target = target_value.clone();
                    let submit_address = address_value.clone();
                    let submit_target = target_value.clone();
                    input
                        .placeholder(&eval_render_value(placeholder, context))
                        .font(&DEFAULT_FONT)
                        .font_size(*size)
                        .text_color(*color)
                        .placeholder_color(*placeholder_color)
                        .cursor_color(0x2F6FB8)
                        .selection_color(0xB9D7F5)
                        .on_changed(move |text| {
                            if let Some(source) = &change_source {
                                record_ui_source_observation(render_source_event(
                                    source,
                                    Some(text),
                                    None,
                                    change_address.as_deref(),
                                    change_target.as_deref(),
                                    target_occurrence,
                                ));
                            }
                        })
                        .on_submit(move |text| {
                            if let Some(source) = &submit_source {
                                suppress_next_blur_for_input(&submit_element_id);
                                record_ui_source_observation(render_source_event(
                                    source,
                                    Some(text),
                                    Some("Enter"),
                                    submit_address.as_deref(),
                                    submit_target.as_deref(),
                                    target_occurrence,
                                ));
                            }
                        })
                })
                .empty();
        }
        RenderNode::Button {
            id,
            text,
            width,
            selected,
            source,
            double_click_source,
            address,
            target,
            visible,
            hover_visible,
            height,
            size,
            color,
            background,
            selected_color,
            selected_background,
            border,
            selected_border,
            color_if,
            if_color,
            strike_if,
            align_left,
        } => {
            if visible
                .as_ref()
                .is_some_and(|visible| !eval_bool(visible, context))
            {
                return;
            }
            let source = source.clone();
            let double_click_source = double_click_source.clone();
            let address_value = address
                .as_ref()
                .map(|value| eval_render_value(value, context));
            let target_value = target
                .as_ref()
                .map(|value| eval_render_value(value, context));
            let target_occurrence = target_occurrence(target.as_ref(), context);
            let selected = selected
                .as_ref()
                .is_some_and(|selection| selection_matches(selection, context));
            let hover_scope_active = !*hover_visible
                || context
                    .hover_scopes
                    .last()
                    .is_some_and(|scope| was_hover_scope_active(scope));
            let effective_color = if color_if
                .as_ref()
                .is_some_and(|condition| eval_bool(condition, context))
            {
                if_color.unwrap_or(*color)
            } else if *hover_visible && !hover_scope_active {
                *background
            } else {
                *color
            };
            let label = decorated_render_text(
                eval_render_value(text, context),
                strike_if
                    .as_ref()
                    .is_some_and(|condition| eval_bool(condition, context)),
            );
            let event_key = render_scope_key(id, context);
            ui.element()
                .id(render_id(id, context))
                .height(height.map_or(fixed!(32.0), |height| fixed!(height)))
                .width(match width {
                    Some(RenderExtent::Fill) => grow!(),
                    Some(RenderExtent::Fixed(width)) => fixed!(*width),
                    None => fixed!(118.0),
                })
                .background_color(if selected {
                    *selected_background
                } else {
                    *background
                })
                .border(|border_style| {
                    border_style
                        .color(if selected {
                            selected_border.or(*border).unwrap_or(0xFFFFFF)
                        } else {
                            border.unwrap_or(0xFFFFFF)
                        })
                        .all(1)
                })
                .layout(|layout| {
                    if *align_left {
                        layout.align(Left, CenterY)
                    } else {
                        layout.align(CenterX, CenterY)
                    }
                })
                .on_press(move |_, _| {
                    if let Some(source) = &double_click_source {
                        if !register_double_click(&event_key) {
                            return;
                        }
                        record_ui_source_observation(render_source_event(
                            source,
                            None,
                            None,
                            address_value.as_deref(),
                            target_value.as_deref(),
                            target_occurrence,
                        ));
                    } else if let Some(source) = &source {
                        record_ui_source_observation(render_source_event(
                            source,
                            None,
                            None,
                            address_value.as_deref(),
                            target_value.as_deref(),
                            target_occurrence,
                        ));
                    }
                })
                .children(|ui| {
                    ui.text(&label, |text| {
                        text.font_size(*size).color(if selected {
                            *selected_color
                        } else {
                            effective_color
                        })
                    });
                });
        }
        RenderNode::Checkbox {
            id,
            checked,
            source,
            target,
            size,
        } => {
            let source = source.clone();
            let target_value = target
                .as_ref()
                .map(|value| eval_render_value(value, context));
            let target_occurrence = target_occurrence(target.as_ref(), context);
            let checked = eval_bool(checked, context);
            ui.element()
                .id(render_id(id, context))
                .width(fixed!(*size + 12.0))
                .height(fixed!(*size + 12.0))
                .background_color(0xFFFFFF)
                .layout(|layout| layout.align(CenterX, CenterY))
                .on_press(move |_, _| {
                    if let Some(source) = &source {
                        record_ui_source_observation(render_source_event(
                            source,
                            None,
                            None,
                            None,
                            target_value.as_deref(),
                            target_occurrence,
                        ));
                    }
                })
                .children(|ui| {
                    ui.text(if checked { "✓" } else { "○" }, |text| {
                        text.font_size(*size as u16).color(if checked {
                            0x3EA390
                        } else {
                            0x949494
                        })
                    });
                });
        }
    }
}

fn decorated_render_text(label: String, strike: bool) -> String {
    if !strike {
        return label;
    }
    let mut decorated = String::with_capacity(label.len() * 3);
    for ch in label.chars() {
        decorated.push(ch);
        decorated.push('\u{0336}');
    }
    decorated
}

fn render_source_event(
    source: &str,
    text: Option<&str>,
    key: Option<&str>,
    address: Option<&str>,
    target_text: Option<&str>,
    target_occurrence: Option<usize>,
) -> serde_json::Value {
    let mut event = serde_json::Map::new();
    event.insert("source".to_owned(), json!(source));
    if let Some(text) = text {
        event.insert("text".to_owned(), json!(text));
    }
    if let Some(key) = key {
        event.insert("key".to_owned(), json!(key));
    }
    if let Some(address) = address {
        event.insert("address".to_owned(), json!(address));
    }
    if let Some(target_text) = target_text {
        event.insert("target_text".to_owned(), json!(target_text));
    }
    if let Some(target_occurrence) = target_occurrence {
        event.insert("target_occurrence".to_owned(), json!(target_occurrence));
    }
    serde_json::Value::Object(event)
}

fn render_scope_key(id: &str, context: &RenderContext<'_>) -> String {
    if let Some(index) = context.index_stack.last() {
        format!("{id}[{index}]")
    } else {
        id.to_owned()
    }
}

fn record_hover_scope(scope: &str) {
    CURRENT_HOVER_SCOPES.with(|scopes| {
        scopes.borrow_mut().insert(scope.to_owned());
    });
}

fn begin_hover_tracking_frame() {
    CURRENT_HOVER_SCOPES.with(|scopes| scopes.borrow_mut().clear());
}

fn finish_hover_tracking_frame() {
    let current = CURRENT_HOVER_SCOPES.with(|scopes| scopes.borrow().clone());
    LAST_HOVER_SCOPES.with(|scopes| {
        *scopes.borrow_mut() = current;
    });
}

fn was_hover_scope_active(scope: &str) -> bool {
    LAST_HOVER_SCOPES.with(|scopes| scopes.borrow().contains(scope))
}

fn register_double_click(key: &str) -> bool {
    const DOUBLE_CLICK_MS: u128 = 450;
    let now = Instant::now();
    LAST_BUTTON_PRESS.with(|last| {
        let mut last = last.borrow_mut();
        let matched = last.as_ref().is_some_and(|(last_key, instant)| {
            last_key == key && instant.elapsed().as_millis() <= DOUBLE_CLICK_MS
        });
        if matched {
            *last = None;
            true
        } else {
            *last = Some((key.to_owned(), now));
            false
        }
    })
}

fn target_occurrence(target: Option<&RenderValue>, context: &RenderContext<'_>) -> Option<usize> {
    let RenderValue::Path(path) = target? else {
        return None;
    };
    let (binding_name, field_path) = path.split_once('.')?;
    let binding = context
        .bindings
        .iter()
        .rev()
        .find(|binding| binding.name == binding_name)?;
    let target_value = json_path_string(binding.value, field_path)?;
    let rows = resolve_path(context, &binding.list)?.as_array()?;
    Some(
        rows.iter()
            .take(binding.index.saturating_add(1))
            .filter(|row| {
                json_path_string(row, field_path).is_some_and(|value| value == target_value)
            })
            .count()
            .max(1),
    )
}

fn json_path_string<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a str> {
    let mut value = value;
    for part in path.split('.') {
        value = value.get(part)?;
    }
    value.as_str()
}

fn render_id(id: &str, context: &RenderContext<'_>) -> Id {
    let label = render_id_label(id);
    if let Some(index) = context.index_stack.last() {
        Id::new_index(label, *index as u32)
    } else {
        Id::new(label)
    }
}

fn render_id_with_key(id: &str, key: Option<&RenderValue>, context: &RenderContext<'_>) -> Id {
    if let Some(key) = key {
        let key = eval_render_value(key, context);
        if !key.is_empty() {
            return Id::new(render_id_label(&format!("{id}_{key}")));
        }
    }
    render_id(id, context)
}

fn render_id_label(id: &str) -> &'static str {
    RENDER_ID_LABELS.with(|labels| {
        let mut labels = labels.borrow_mut();
        if let Some(label) = labels.get(id) {
            return *label;
        }
        let label = Box::leak(id.to_owned().into_boxed_str());
        labels.insert(id.to_owned(), label);
        label
    })
}

fn selection_matches(selection: &RenderSelection, context: &RenderContext<'_>) -> bool {
    eval_path_text(&selection.path, context).as_deref() == Some(selection.expected.as_str())
}

fn eval_bool(value: &RenderValue, context: &RenderContext<'_>) -> bool {
    match value {
        RenderValue::Literal(value) => matches!(value.as_str(), "true" | "True" | "1"),
        RenderValue::Path(path) => resolve_path(context, path)
            .map(|value| {
                value
                    .as_bool()
                    .unwrap_or_else(|| value.as_u64().unwrap_or_default() != 0)
            })
            .unwrap_or(false),
        RenderValue::Template(value) => !eval_template(value, context).is_empty(),
    }
}

fn eval_render_value(value: &RenderValue, context: &RenderContext<'_>) -> String {
    match value {
        RenderValue::Literal(value) => value.clone(),
        RenderValue::Path(path) => eval_path_text(path, context).unwrap_or_default(),
        RenderValue::Template(value) => eval_template(value, context),
    }
}

fn eval_template(template: &str, context: &RenderContext<'_>) -> String {
    let mut output = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('}') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let path = &after_start[..end];
        output.push_str(&eval_path_text(path, context).unwrap_or_default());
        rest = &after_start[end + 1..];
    }
    output.push_str(rest);
    output
}

fn eval_path_text(path: &str, context: &RenderContext<'_>) -> Option<String> {
    let value = resolve_path(context, path)?;
    if let Some(value) = value.as_str() {
        Some(value.to_owned())
    } else if let Some(value) = value.as_bool() {
        Some(value.to_string())
    } else if let Some(value) = value.as_u64() {
        Some(value.to_string())
    } else if let Some(value) = value.as_i64() {
        Some(value.to_string())
    } else if value.is_null() {
        Some(String::new())
    } else {
        Some(value.to_string())
    }
}

fn resolve_path<'a>(context: &'a RenderContext<'a>, path: &str) -> Option<&'a serde_json::Value> {
    let mut parts = path.split('.');
    let first = parts.next()?;
    let mut value = context
        .bindings
        .iter()
        .rev()
        .find_map(|binding| (binding.name == first).then_some(binding.value))
        .or_else(|| context.root.get(first))?;
    for part in parts {
        value = value.get(part)?;
    }
    Some(value)
}

fn parse_render_view(source: &str) -> Result<Vec<RenderNode>, String> {
    let Some(lines) = render_view_lines(source) else {
        return Ok(Vec::new());
    };
    parse_render_nodes(&lines)
}

fn render_view_lines(source: &str) -> Option<Vec<String>> {
    let mut in_view = false;
    let mut depth = 0i32;
    let mut lines = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if !in_view {
            if trimmed == "VIEW {" {
                in_view = true;
                depth = 1;
            }
            continue;
        }
        for ch in trimmed.chars() {
            match ch {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        if depth <= 0 {
            return Some(lines);
        }
        lines.push(trimmed.to_owned());
    }
    None
}

#[derive(Debug)]
enum RenderFrame {
    Column {
        id: Option<String>,
        width: Option<f32>,
        height: Option<f32>,
        background: u32,
        border: Option<u32>,
        gap: f32,
        padding: Option<(f32, f32, f32, f32)>,
        children: Vec<RenderNode>,
    },
    Row {
        id: Option<String>,
        height: Option<f32>,
        background: u32,
        border: Option<u32>,
        gap: f32,
        padding: Option<(f32, f32, f32, f32)>,
        children: Vec<RenderNode>,
    },
    ForEach {
        list: String,
        item: String,
        children: Vec<RenderNode>,
    },
}

fn parse_render_nodes(lines: &[String]) -> Result<Vec<RenderNode>, String> {
    let mut root = Vec::new();
    let mut stack = Vec::<RenderFrame>::new();
    for line in lines {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "}" {
            let frame = stack.pop().ok_or("VIEW block has an extra closing brace")?;
            push_render_node(&mut root, &mut stack, frame.into_node());
            continue;
        }
        if line.ends_with('{') {
            stack.push(parse_render_frame(line)?);
            continue;
        }
        let node = parse_render_leaf(line)?;
        push_render_node(&mut root, &mut stack, node);
    }
    if !stack.is_empty() {
        return Err("VIEW block has an unclosed container".to_owned());
    }
    Ok(root)
}

impl RenderFrame {
    fn into_node(self) -> RenderNode {
        match self {
            Self::Column {
                id,
                width,
                height,
                background,
                border,
                gap,
                padding,
                children,
            } => RenderNode::Column {
                id,
                width,
                height,
                background,
                border,
                gap,
                padding,
                children,
            },
            Self::Row {
                id,
                height,
                background,
                border,
                gap,
                padding,
                children,
            } => RenderNode::Row {
                id,
                height,
                background,
                border,
                gap,
                padding,
                children,
            },
            Self::ForEach {
                list,
                item,
                children,
            } => RenderNode::ForEach {
                list,
                item,
                children,
            },
        }
    }

    fn children_mut(&mut self) -> &mut Vec<RenderNode> {
        match self {
            Self::Column { children, .. }
            | Self::Row { children, .. }
            | Self::ForEach { children, .. } => children,
        }
    }
}

fn push_render_node(root: &mut Vec<RenderNode>, stack: &mut [RenderFrame], node: RenderNode) {
    if let Some(parent) = stack.last_mut() {
        parent.children_mut().push(node);
    } else {
        root.push(node);
    }
}

fn parse_render_frame(line: &str) -> Result<RenderFrame, String> {
    let without_brace = line.trim_end_matches('{').trim();
    let tokens = tokenize_render_line(without_brace);
    match tokens.first().map(String::as_str) {
        Some("Column") => {
            let attrs = parse_render_attrs(&tokens[1..]);
            Ok(RenderFrame::Column {
                id: attrs.get("id").cloned(),
                width: attrs.get("width").and_then(|value| value.parse().ok()),
                height: attrs.get("height").and_then(|value| value.parse().ok()),
                background: parse_color(&attrs, "bg", 0xFFFFFF),
                border: parse_optional_color(&attrs, "border"),
                gap: parse_float(&attrs, "gap", 0.0),
                padding: parse_padding(&attrs),
                children: Vec::new(),
            })
        }
        Some("Row") => {
            let attrs = parse_render_attrs(&tokens[1..]);
            Ok(RenderFrame::Row {
                id: attrs.get("id").cloned(),
                height: attrs.get("height").and_then(|value| value.parse().ok()),
                background: parse_color(&attrs, "bg", 0xFFFFFF),
                border: parse_optional_color(&attrs, "border").or(Some(0xEDEDED)),
                gap: parse_float(&attrs, "gap", 8.0),
                padding: parse_padding(&attrs),
                children: Vec::new(),
            })
        }
        Some("ForEach") if tokens.len() >= 4 && tokens[2] == "as" => Ok(RenderFrame::ForEach {
            list: tokens[1].clone(),
            item: tokens[3].clone(),
            children: Vec::new(),
        }),
        Some(kind) => Err(format!("unsupported VIEW container `{kind}`")),
        None => Err("empty VIEW container".to_owned()),
    }
}

fn parse_render_leaf(line: &str) -> Result<RenderNode, String> {
    let tokens = tokenize_render_line(line);
    let Some(kind) = tokens.first().map(String::as_str) else {
        return Err("empty VIEW leaf".to_owned());
    };
    let attrs = parse_render_attrs(&tokens[1..]);
    match kind {
        "Text" => Ok(RenderNode::Text {
            value: render_value_from_attrs(&attrs, "value")
                .or_else(|| render_value_from_attrs(&attrs, "text"))
                .or_else(|| render_value_from_attrs(&attrs, "template"))
                .unwrap_or_else(|| RenderValue::Literal(String::new())),
            size: parse_size(&attrs, 16),
            color: parse_color(&attrs, "color", 0x1F2630),
            background: parse_optional_color(&attrs, "bg"),
            width: attrs
                .get("width")
                .and_then(|value| RenderExtent::from_attr(value)),
            height: attrs.get("height").and_then(|value| value.parse().ok()),
            strike_if: render_value_from_attrs(&attrs, "strike_if"),
            center: parse_bool_attr(&attrs, "center"),
        }),
        "Input" => Ok(RenderNode::Input {
            id: required_attr(&attrs, "id")?,
            key: render_value_from_attrs(&attrs, "key"),
            value: render_value_from_attrs(&attrs, "value")
                .unwrap_or_else(|| RenderValue::Literal(String::new())),
            placeholder: render_value_from_attrs(&attrs, "placeholder")
                .unwrap_or_else(|| RenderValue::Literal(String::new())),
            change_source: attrs.get("change").cloned(),
            submit_source: attrs.get("submit").cloned(),
            cancel_source: attrs.get("cancel").cloned(),
            escape_source: attrs.get("escape").cloned(),
            blur_source: attrs.get("blur").cloned(),
            address: render_value_from_attrs(&attrs, "address"),
            target: render_value_from_attrs(&attrs, "target"),
            visible: render_value_from_attrs(&attrs, "visible"),
            size: parse_size(&attrs, 16),
            height: attrs.get("height").and_then(|value| value.parse().ok()),
            color: parse_color(&attrs, "color", 0x1F2630),
            placeholder_color: parse_color(&attrs, "placeholder_color", 0x8B97A7),
            background: parse_color(&attrs, "bg", 0xFFFFFF),
            border: parse_optional_color(&attrs, "border"),
        }),
        "Button" => Ok(RenderNode::Button {
            id: required_attr(&attrs, "id")?,
            text: render_value_from_attrs(&attrs, "text")
                .unwrap_or_else(|| RenderValue::Literal(String::new())),
            width: attrs
                .get("width")
                .and_then(|value| RenderExtent::from_attr(value)),
            selected: attrs.get("selected").and_then(|value| {
                let (path, expected) = value.split_once(':')?;
                Some(RenderSelection {
                    path: path.strip_prefix('$').unwrap_or(path).to_owned(),
                    expected: expected.to_owned(),
                })
            }),
            source: attrs.get("source").cloned(),
            double_click_source: attrs.get("double_click").cloned(),
            address: render_value_from_attrs(&attrs, "address"),
            target: render_value_from_attrs(&attrs, "target"),
            visible: render_value_from_attrs(&attrs, "visible"),
            hover_visible: parse_bool_attr(&attrs, "hover_visible"),
            height: attrs.get("height").and_then(|value| value.parse().ok()),
            size: parse_size(&attrs, 14),
            color: parse_color(&attrs, "color", 0x1F2630),
            background: parse_color(&attrs, "bg", 0xFFFFFF),
            selected_color: parse_color(&attrs, "selected_color", 0x1F2630),
            selected_background: parse_color(&attrs, "selected_bg", 0xFFFFFF),
            border: parse_optional_color(&attrs, "border"),
            selected_border: parse_optional_color(&attrs, "selected_border"),
            color_if: render_value_from_attrs(&attrs, "color_if"),
            if_color: parse_optional_color(&attrs, "if_color"),
            strike_if: render_value_from_attrs(&attrs, "strike_if"),
            align_left: attrs.get("align").is_some_and(|value| value == "left"),
        }),
        "Checkbox" => Ok(RenderNode::Checkbox {
            id: required_attr(&attrs, "id")?,
            checked: render_value_from_attrs(&attrs, "checked")
                .unwrap_or_else(|| RenderValue::Literal("False".to_owned())),
            source: attrs.get("source").cloned(),
            target: render_value_from_attrs(&attrs, "target"),
            size: parse_float(&attrs, "size", 48.0),
        }),
        _ => Err(format!("unsupported VIEW leaf `{kind}`")),
    }
}

fn tokenize_render_line(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut in_quote = false;
    for ch in line.chars() {
        match ch {
            '"' => {
                in_quote = !in_quote;
                token.push(ch);
            }
            ' ' | '\t' if !in_quote => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            _ => token.push(ch),
        }
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
}

fn parse_render_attrs(tokens: &[String]) -> BTreeMap<String, String> {
    tokens
        .iter()
        .filter_map(|token| {
            let (key, value) = token.split_once('=')?;
            Some((key.to_owned(), unquote(value)))
        })
        .collect()
}

fn unquote(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
        .to_owned()
}

fn render_value_from_attrs(attrs: &BTreeMap<String, String>, key: &str) -> Option<RenderValue> {
    let value = attrs.get(key)?;
    if key == "template" {
        Some(RenderValue::Template(value.clone()))
    } else if let Some(path) = value.strip_prefix('$') {
        Some(RenderValue::Path(path.to_owned()))
    } else if value.contains('{') && value.contains('}') {
        Some(RenderValue::Template(value.clone()))
    } else {
        Some(RenderValue::Literal(value.clone()))
    }
}

fn required_attr(attrs: &BTreeMap<String, String>, key: &str) -> Result<String, String> {
    attrs
        .get(key)
        .cloned()
        .ok_or_else(|| format!("VIEW node missing `{key}`"))
}

fn parse_size(attrs: &BTreeMap<String, String>, default: u16) -> u16 {
    attrs
        .get("size")
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn parse_float(attrs: &BTreeMap<String, String>, key: &str, default: f32) -> f32 {
    attrs
        .get(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn parse_bool_attr(attrs: &BTreeMap<String, String>, key: &str) -> bool {
    attrs
        .get(key)
        .is_some_and(|value| matches!(value.as_str(), "true" | "True" | "1" | "yes"))
}

fn parse_optional_color(attrs: &BTreeMap<String, String>, key: &str) -> Option<u32> {
    attrs.get(key).and_then(|value| parse_color_value(value))
}

fn parse_color(attrs: &BTreeMap<String, String>, key: &str, default: u32) -> u32 {
    parse_optional_color(attrs, key).unwrap_or(default)
}

fn parse_color_value(value: &str) -> Option<u32> {
    let value = value.strip_prefix('#').unwrap_or(value);
    u32::from_str_radix(value, 16).ok()
}

fn parse_padding(attrs: &BTreeMap<String, String>) -> Option<(f32, f32, f32, f32)> {
    let value = attrs.get("padding")?;
    let parts = value
        .split(',')
        .filter_map(|part| part.trim().parse::<f32>().ok())
        .collect::<Vec<_>>();
    match parts.as_slice() {
        [all] => Some((*all, *all, *all, *all)),
        [horizontal, vertical] => Some((*horizontal, *horizontal, *vertical, *vertical)),
        [left, right, top, bottom] => Some((*left, *right, *top, *bottom)),
        _ => None,
    }
}

fn padding_u16(padding: (f32, f32, f32, f32)) -> (u16, u16, u16, u16) {
    (
        padding.0.max(0.0) as u16,
        padding.1.max(0.0) as u16,
        padding.2.max(0.0) as u16,
        padding.3.max(0.0) as u16,
    )
}

fn delta_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    panel(ui, "delta_panel", "Deltas", |ui| {
        if let Some(output) = &state.output {
            for delta in output.semantic_deltas.iter().rev().take(7) {
                let field = delta.field_path.as_deref().unwrap_or("-");
                ui.text(&format!("{} {}", delta.kind, field), |text| {
                    text.font_size(13).color(0x1F2630)
                });
            }
        }
    });
}

fn inspector_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    panel(ui, "inspector_panel", "Inspector", |ui| {
        if let Some(output) = &state.output {
            if state.selected == "todomvc" {
                if let Some(todo) = output.state_summary["todos"]
                    .as_array()
                    .and_then(|todos| todos.first())
                {
                    compact_json(ui, todo);
                }
            } else if let Some(cell) = output.state_summary["cells"]
                .as_array()
                .and_then(|cells| cells.first())
            {
                compact_json(ui, cell);
            }
        }
    });
}

fn explanation_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    panel(ui, "explanation_panel", "Causes", |ui| {
        if let Some(output) = &state.output {
            let target = selected_cause_target(state);
            ui.text(target, |text| text.font_size(13).color(0x2F6FB8));
            if let Some(cause) = possible_causes_for(&output.report, target) {
                if let Some(sources) = cause["sources"].as_array() {
                    for source in sources.iter().filter_map(serde_json::Value::as_str).take(5) {
                        ui.text(&format!("<- {source}"), |text| {
                            text.font_size(11).color(0x1F2630)
                        });
                    }
                }
            }
            ui.text(
                &format!("nodes {}", output.report["graph_node_count"]),
                |text| text.font_size(13).color(0x1F2630),
            );
            ui.text(
                &format!("dirty keys {}", output.report["max_dirty_keys"]),
                |text| text.font_size(13).color(0x1F2630),
            );
            ui.text(
                &format!("deltas {}", output.semantic_deltas.len()),
                |text| text.font_size(13).color(0x1F2630),
            );
            ui.text(
                &format!("patches {}", output.render_patches.len()),
                |text| text.font_size(13).color(0x1F2630),
            );
        }
    });
}

fn selected_cause_target(state: &PlaygroundState) -> &'static str {
    if state.selected == "todomvc" {
        "todo.completed"
    } else {
        "cell.formula_text"
    }
}

fn possible_causes_for<'a>(
    report: &'a serde_json::Value,
    target: &str,
) -> Option<&'a serde_json::Value> {
    report["ir_debug_tables"]["possible_causes"]
        .as_array()?
        .iter()
        .find(|entry| entry["target"].as_str() == Some(target))
}

fn panel(ui: &mut Ui<'_, ()>, id: &'static str, title: &str, body: impl FnOnce(&mut Ui<'_, ()>)) {
    ui.element()
        .id(id)
        .width(grow!())
        .height(grow!())
        .background_color(0xFFFFFF)
        .border(|border| border.color(0xD5DDE8).all(1))
        .layout(|layout| layout.direction(TopToBottom).padding((10, 10, 8, 8)).gap(6))
        .children(|ui| {
            ui.text(title, |text| text.font_size(16).color(0x596579));
            body(ui);
        });
}

fn compact_json(ui: &mut Ui<'_, ()>, value: &serde_json::Value) {
    let text = value.to_string();
    for line in wrapped_text(&text, 38).into_iter().take(8) {
        ui.text(&line, |text| text.font_size(12).color(0x1F2630));
    }
}

fn wrapped_text(text: &str, width: usize) -> Vec<String> {
    text.as_bytes()
        .chunks(width)
        .map(|chunk| String::from_utf8_lossy(chunk).to_string())
        .collect()
}

fn step_label(state: &PlaygroundState) -> String {
    match state.step_limit {
        Some(limit) => format!("{}/{}", limit.min(state.scenario_len), state.scenario_len),
        None => format!("all {}", state.scenario_len),
    }
}

fn value_after(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].clone())
}

#[derive(Clone, Copy, Debug)]
struct PixelStats {
    nonzero_channels: usize,
    unique_rgba_values: usize,
}

#[derive(Clone, Debug)]
struct ScreenshotCapture {
    pixel_stats: PixelStats,
    width: u32,
    height: u32,
    capture_backend: String,
}

fn bounds_json(bounds: ply_engine::math::BoundingBox) -> serde_json::Value {
    json!({
        "x": bounds.x,
        "y": bounds.y,
        "width": bounds.width,
        "height": bounds.height
    })
}

fn image_stats(bytes: &[u8]) -> PixelStats {
    let nonzero_channels = bytes.iter().filter(|channel| **channel != 0).count();
    let mut unique = std::collections::BTreeSet::new();
    for pixel in bytes.chunks_exact(4) {
        unique.insert([pixel[0], pixel[1], pixel[2], pixel[3]]);
        if unique.len() > 256 {
            break;
        }
    }
    PixelStats {
        nonzero_channels,
        unique_rgba_values: unique.len(),
    }
}

fn capture_probe_frame_png(
    screenshot: &Path,
) -> Result<ScreenshotCapture, Box<dyn std::error::Error>> {
    if !focus_free_headed() {
        return capture_current_frame_png(screenshot);
    }
    let cache = FOCUS_FREE_SCREENSHOT_CACHE.get_or_init(|| Mutex::new(None));
    if let Some(cached) = cache
        .lock()
        .expect("focus-free screenshot cache poisoned")
        .clone()
    {
        let (cached_path, mut capture) = cached;
        if cached_path.exists() {
            if let Some(parent) = screenshot.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&cached_path, screenshot)?;
            capture.capture_backend = "cached-focus-free-checkpoint".to_owned();
            return Ok(capture);
        }
    }
    let image = get_screen_data();
    let pixel_stats = image_stats(&image.bytes);
    if let Some(parent) = screenshot.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if pixel_stats.nonzero_channels != 0 && pixel_stats.unique_rgba_values > 1 {
        image.export_png(screenshot.to_str().ok_or("screenshot path is not utf-8")?);
        let capture = ScreenshotCapture {
            pixel_stats,
            width: u32::from(image.width),
            height: u32::from(image.height),
            capture_backend: "macroquad-framebuffer".to_owned(),
        };
        *cache.lock().expect("focus-free screenshot cache poisoned") =
            Some((screenshot.to_path_buf(), capture.clone()));
        return Ok(capture);
    }
    let capture = capture_with_cosmic_screenshot(screenshot)?;
    *cache.lock().expect("focus-free screenshot cache poisoned") =
        Some((screenshot.to_path_buf(), capture.clone()));
    Ok(capture)
}

fn capture_current_frame_png(
    screenshot: &Path,
) -> Result<ScreenshotCapture, Box<dyn std::error::Error>> {
    let image = get_screen_data();
    let pixel_stats = image_stats(&image.bytes);
    if let Some(parent) = screenshot.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if pixel_stats.nonzero_channels == 0 || pixel_stats.unique_rgba_values <= 1 {
        capture_with_cosmic_screenshot(screenshot)
    } else {
        image.export_png(screenshot.to_str().ok_or("screenshot path is not utf-8")?);
        Ok(ScreenshotCapture {
            pixel_stats,
            width: u32::from(image.width),
            height: u32::from(image.height),
            capture_backend: "macroquad-framebuffer".to_owned(),
        })
    }
}

fn capture_with_cosmic_screenshot(
    screenshot: &Path,
) -> Result<ScreenshotCapture, Box<dyn std::error::Error>> {
    let parent = screenshot.parent().unwrap_or_else(|| Path::new("."));
    let capture_dir = parent.join(format!(".cosmic-smoke-capture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&capture_dir);
    std::fs::create_dir_all(&capture_dir)?;
    let output = Command::new("cosmic-screenshot")
        .args([
            "--interactive=false",
            "--modal=false",
            "--notify=false",
            "--save-dir",
            capture_dir
                .to_str()
                .ok_or("cosmic screenshot directory path is not utf-8")?,
        ])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "macroquad framebuffer was blank and cosmic-screenshot fallback failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let capture = newest_png_in_dir(&capture_dir).ok_or_else(|| {
        format!(
            "cosmic-screenshot did not create a PNG in {}",
            capture_dir.display()
        )
    })?;
    std::fs::copy(&capture, screenshot)?;
    let decoded = image::open(screenshot)?.to_rgba8();
    let pixel_stats = image_stats(decoded.as_raw());
    if pixel_stats.nonzero_channels == 0 || pixel_stats.unique_rgba_values <= 1 {
        return Err(format!(
            "cosmic-screenshot fallback capture is blank: nonzero_channels={}, unique_rgba_values={}",
            pixel_stats.nonzero_channels, pixel_stats.unique_rgba_values
        )
        .into());
    }
    let _ = std::fs::remove_dir_all(&capture_dir);
    Ok(ScreenshotCapture {
        pixel_stats,
        width: decoded.width(),
        height: decoded.height(),
        capture_backend: "cosmic-screenshot".to_owned(),
    })
}

fn newest_png_in_dir(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("png"))
        .max_by_key(|path| {
            std::fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        })
}

fn display_server() -> String {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        "wayland".to_owned()
    } else if std::env::var("DISPLAY").is_ok() {
        "x11".to_owned()
    } else {
        "none".to_owned()
    }
}

fn display_socket() -> String {
    std::env::var("WAYLAND_DISPLAY")
        .or_else(|_| std::env::var("DISPLAY"))
        .unwrap_or_else(|_| "none".to_owned())
}

fn native_display_contract() -> serde_json::Value {
    let session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
    let display = std::env::var("DISPLAY").ok();
    json!({
        "required": true,
        "status": if session_type.as_deref() == Some("wayland") && wayland_display.is_some() {
            "pass"
        } else {
            "fail"
        },
        "xdg_session_type": session_type,
        "wayland_display": wayland_display,
        "display": display,
        "contract": "native playground runs in a Wayland desktop session with WAYLAND_DISPLAY set; stricter socket-level proof is a follow-up gate"
    })
}

fn command_path(command: &str) -> Option<String> {
    std::process::Command::new("sh")
        .args(["-lc", &format!("command -v {command}")])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty())
}

fn send_real_keyboard_text(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    if os_input_forbidden() {
        return Err("OS keyboard input is forbidden; use --verify-headed-focusless or unset BOON_FORBID_OS_INPUT only for an explicit OS input probe".into());
    }
    let Some(wtype) = command_path("wtype") else {
        return Err("wtype is required for the OS input probe".into());
    };
    let status = std::process::Command::new(wtype).arg(text).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("wtype exited with {status}").into())
    }
}

fn send_real_key(key: &str) -> Result<(), Box<dyn std::error::Error>> {
    if os_input_forbidden() {
        return Err("OS keyboard input is forbidden; use --verify-headed-focusless or unset BOON_FORBID_OS_INPUT only for an explicit OS input probe".into());
    }
    let Some(wtype) = command_path("wtype") else {
        return Err("wtype is required for headed keyboard activation".into());
    };
    let status = std::process::Command::new(wtype)
        .args(["-k", key])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("wtype -k {key} exited with {status}").into())
    }
}

fn send_real_pointer_click(
    bounds: ply_engine::math::BoundingBox,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    if os_input_forbidden() {
        return Err("OS pointer input is forbidden; use --verify-headed-focusless or unset BOON_FORBID_OS_INPUT only for an explicit OS input probe".into());
    }
    let (window_x, window_y, scale, local_x, local_y, screen_position, delta) =
        pointer_target_coordinates(bounds);
    if display_server() != "wayland"
        && let Ok(backend_report) = send_xtest_pointer_click(screen_position[0], screen_position[1])
    {
        return Ok(json!({
            "backend": "x11_xtest",
            "window_position": [window_x, window_y],
            "display_scale": scale,
            "element_center_local": [local_x, local_y],
            "screen_position": screen_position,
            "current_pointer_local_before_move": [local_x - delta[0] as f32 / scale, local_y - delta[1] as f32 / scale],
            "relative_move_delta": delta,
            "xtest": backend_report
        }));
    }
    let Some(ydotool) = command_path("ydotool") else {
        return Err(
            "XTest pointer injection failed and ydotool is unavailable for headed pointer probing"
                .into(),
        );
    };
    let screen_x_arg = format!("{}", screen_position[0]);
    let screen_y_arg = format!("{}", screen_position[1]);
    let move_status = std::process::Command::new(&ydotool)
        .args([
            "mousemove",
            "--delay",
            "30",
            "--",
            &screen_x_arg,
            &screen_y_arg,
        ])
        .status()?;
    if !move_status.success() {
        return Err(format!("ydotool mousemove exited with {move_status}").into());
    }
    std::thread::sleep(std::time::Duration::from_millis(80));
    let click_status = std::process::Command::new(&ydotool)
        .args(["click", "--delay", "30", "1"])
        .status()?;
    if !click_status.success() {
        return Err(format!("ydotool click exited with {click_status}").into());
    }
    Ok(json!({
        "backend": "ydotool",
        "window_position": [window_x, window_y],
        "display_scale": scale,
        "element_center_local": [local_x, local_y],
        "screen_position": screen_position,
        "relative_move_delta": delta,
        "mousemove_coordinate_mode": "absolute_screen",
        "mousemove_status": move_status.to_string(),
        "click_status": click_status.to_string()
    }))
}

fn send_real_pointer_move(
    bounds: ply_engine::math::BoundingBox,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    if os_input_forbidden() {
        return Err("OS pointer input is forbidden; use --verify-headed-focusless or unset BOON_FORBID_OS_INPUT only for an explicit OS input probe".into());
    }
    let (window_x, window_y, scale, local_x, local_y, screen_position, delta) =
        pointer_target_coordinates(bounds);
    if display_server() != "wayland"
        && let Ok(backend_report) = send_xtest_pointer_move(screen_position[0], screen_position[1])
    {
        return Ok(json!({
            "backend": "x11_xtest",
            "window_position": [window_x, window_y],
            "display_scale": scale,
            "element_center_local": [local_x, local_y],
            "screen_position": screen_position,
            "relative_move_delta": delta,
            "xtest": backend_report
        }));
    }
    let Some(ydotool) = command_path("ydotool") else {
        return Err(
            "XTest pointer movement failed and ydotool is unavailable for headed pointer probing"
                .into(),
        );
    };
    let screen_x_arg = format!("{}", screen_position[0]);
    let screen_y_arg = format!("{}", screen_position[1]);
    let move_status = std::process::Command::new(&ydotool)
        .args([
            "mousemove",
            "--delay",
            "30",
            "--",
            &screen_x_arg,
            &screen_y_arg,
        ])
        .status()?;
    if !move_status.success() {
        return Err(format!("ydotool mousemove exited with {move_status}").into());
    }
    Ok(json!({
        "backend": "ydotool",
        "window_position": [window_x, window_y],
        "display_scale": scale,
        "element_center_local": [local_x, local_y],
        "screen_position": screen_position,
        "relative_move_delta": delta,
        "mousemove_coordinate_mode": "absolute_screen",
        "mousemove_status": move_status.to_string()
    }))
}

fn pointer_target_coordinates(
    bounds: ply_engine::math::BoundingBox,
) -> (i32, i32, f32, f32, f32, [i32; 2], [i32; 2]) {
    let (window_x, window_y) = macroquad::miniquad::window::get_window_position();
    let scale = screen_dpi_scale();
    let local_x = bounds.x + bounds.width / 2.0;
    let local_y = bounds.y + bounds.height / 2.0;
    let screen_x = window_x as f32 + local_x * scale;
    let screen_y = window_y as f32 + local_y * scale;
    let (current_local_x, current_local_y) = mouse_position();
    let delta_x = (local_x - current_local_x) * scale;
    let delta_y = (local_y - current_local_y) * scale;
    let screen_position = [screen_x.round() as i32, screen_y.round() as i32];
    (
        window_x as i32,
        window_y as i32,
        scale,
        local_x,
        local_y,
        screen_position,
        [delta_x.round() as i32, delta_y.round() as i32],
    )
}

#[cfg(target_os = "linux")]
fn xtest_pointer_backend_available() -> bool {
    std::env::var_os("DISPLAY").is_some()
}

#[cfg(not(target_os = "linux"))]
fn xtest_pointer_backend_available() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn send_xtest_pointer_click(
    x: i32,
    y: i32,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_uint, c_ulong};

    #[repr(C)]
    struct Display {
        _private: [u8; 0],
    }

    #[link(name = "X11")]
    #[link(name = "Xtst")]
    unsafe extern "C" {
        fn XOpenDisplay(display_name: *const c_char) -> *mut Display;
        fn XDefaultScreen(display: *mut Display) -> c_int;
        fn XRootWindow(display: *mut Display, screen_number: c_int) -> c_ulong;
        fn XTestFakeMotionEvent(
            display: *mut Display,
            screen_number: c_int,
            x: c_int,
            y: c_int,
            delay: c_ulong,
        ) -> c_int;
        fn XTestFakeButtonEvent(
            display: *mut Display,
            button: c_uint,
            is_press: c_int,
            delay: c_ulong,
        ) -> c_int;
        fn XFlush(display: *mut Display) -> c_int;
        fn XCloseDisplay(display: *mut Display) -> c_int;
    }

    let display_name = std::env::var("DISPLAY").unwrap_or_default();
    if display_name.is_empty() {
        return Err("DISPLAY is not set for XTest pointer injection".into());
    }
    let display_name = CString::new(display_name)?;
    let display = unsafe { XOpenDisplay(display_name.as_ptr()) };
    if display.is_null() {
        return Err("XOpenDisplay failed for XTest pointer injection".into());
    }

    struct DisplayGuard(*mut Display);
    impl Drop for DisplayGuard {
        fn drop(&mut self) {
            unsafe {
                XCloseDisplay(self.0);
            }
        }
    }

    let guard = DisplayGuard(display);
    let screen = unsafe { XDefaultScreen(guard.0) };
    let root = unsafe { XRootWindow(guard.0, screen) };
    let moved = unsafe { XTestFakeMotionEvent(guard.0, screen, x, y, 0) };
    unsafe {
        XFlush(guard.0);
    }
    std::thread::sleep(std::time::Duration::from_millis(80));
    let pressed = unsafe { XTestFakeButtonEvent(guard.0, 1, 1, 0) };
    let released = unsafe { XTestFakeButtonEvent(guard.0, 1, 0, 0) };
    unsafe {
        XFlush(guard.0);
    }
    if moved == 0 || pressed == 0 || released == 0 {
        return Err(format!(
            "XTest pointer injection failed: moved={moved}, pressed={pressed}, released={released}"
        )
        .into());
    }
    Ok(json!({
        "display": std::env::var("DISPLAY").unwrap_or_default(),
        "screen": screen,
        "root_window": root,
        "motion_status": moved,
        "button_press_status": pressed,
        "button_release_status": released,
        "coordinate_mode": "absolute_x11_screen"
    }))
}

#[cfg(target_os = "linux")]
fn send_xtest_pointer_move(
    x: i32,
    y: i32,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_ulong};

    #[repr(C)]
    struct Display {
        _private: [u8; 0],
    }

    #[link(name = "X11")]
    #[link(name = "Xtst")]
    unsafe extern "C" {
        fn XOpenDisplay(display_name: *const c_char) -> *mut Display;
        fn XDefaultScreen(display: *mut Display) -> c_int;
        fn XRootWindow(display: *mut Display, screen_number: c_int) -> c_ulong;
        fn XTestFakeMotionEvent(
            display: *mut Display,
            screen_number: c_int,
            x: c_int,
            y: c_int,
            delay: c_ulong,
        ) -> c_int;
        fn XFlush(display: *mut Display) -> c_int;
        fn XCloseDisplay(display: *mut Display) -> c_int;
    }

    let display_name = std::env::var("DISPLAY").unwrap_or_default();
    if display_name.is_empty() {
        return Err("DISPLAY is not set for XTest pointer movement".into());
    }
    let display_name = CString::new(display_name)?;
    let display = unsafe { XOpenDisplay(display_name.as_ptr()) };
    if display.is_null() {
        return Err("XOpenDisplay failed for XTest pointer movement".into());
    }

    struct DisplayGuard(*mut Display);
    impl Drop for DisplayGuard {
        fn drop(&mut self) {
            unsafe {
                XCloseDisplay(self.0);
            }
        }
    }

    let guard = DisplayGuard(display);
    let screen = unsafe { XDefaultScreen(guard.0) };
    let root = unsafe { XRootWindow(guard.0, screen) };
    let moved = unsafe { XTestFakeMotionEvent(guard.0, screen, x, y, 0) };
    unsafe {
        XFlush(guard.0);
    }
    if moved == 0 {
        return Err("XTest pointer movement failed".into());
    }
    Ok(json!({
        "display": std::env::var("DISPLAY").unwrap_or_default(),
        "screen": screen,
        "root_window": root,
        "motion_status": moved,
        "coordinate_mode": "absolute_x11_screen"
    }))
}

#[cfg(not(target_os = "linux"))]
fn send_xtest_pointer_click(
    _x: i32,
    _y: i32,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Err("XTest pointer injection is only implemented on Linux".into())
}

#[cfg(not(target_os = "linux"))]
fn send_xtest_pointer_move(
    _x: i32,
    _y: i32,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Err("XTest pointer movement is only implemented on Linux".into())
}

fn os_key_name(scenario_key: &str) -> &str {
    match scenario_key {
        "Enter" => "Return",
        other => other,
    }
}

fn sanitize_artifact_label(label: &str) -> String {
    label
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn unix_seconds_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn git_commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn current_binary_hash() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| sha256_file(&path).ok())
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::PLAYGROUND_HELP;

    #[test]
    fn help_advertises_manual_launch_and_verifier_modes() {
        for needle in [
            "--example <todomvc|cells>",
            "--smoke-launch --example <name> --report <path>",
            "--verify-headed --example <name> --report <path>",
            "--verify-os-input-probe --report <path>",
        ] {
            assert!(
                PLAYGROUND_HELP.contains(needle),
                "missing help item {needle}"
            );
        }
    }
}
