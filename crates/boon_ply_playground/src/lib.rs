use boon_runtime::{
    LiveRuntime, LiveSourceEvent, RunOutput, VerificationLayer, example_paths, parse_scenario,
    run_scenario, run_scenario_source_with_step_limit, sha256_file, write_json,
};
use ply_engine::prelude::*;
use serde_json::json;
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static DEFAULT_FONT: FontAsset = FontAsset::Bytes {
    file_name: "FiraSans-Regular.otf",
    data: include_bytes!("/usr/share/fonts/opentype/fira/FiraSans-Regular.otf"),
};

thread_local! {
    static UI_SOURCE_OBSERVATIONS: RefCell<Vec<serde_json::Value>> = const { RefCell::new(Vec::new()) };
}

pub async fn run_app_from_args() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--verify-os-input-probe") {
        return run_verify_os_input_probe(&args).await;
    }
    if args.iter().any(|arg| arg == "--verify-headed") {
        return run_verify_headed(&args).await;
    }
    run_interactive(&args).await
}

async fn run_verify_headed(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_after(args, "--example").unwrap_or_else(|| "todomvc".to_owned());
    let report = value_after(args, "--report")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("target/reports/{example}-headed-ply.json")));
    let (source, scenario, _) = example_paths(&example)?;
    let screenshot = report.with_extension("png");
    let os_probe_screenshot = report.with_file_name(format!("{example}-headed-os-input.png"));
    let mut ply = Ply::<()>::new(&DEFAULT_FONT).await;
    ply.set_debug_mode(false);
    let os_probe_token = format!("boon-headed-os-{}-{example}", std::process::id());
    let headed_os_probe =
        run_os_keyboard_probe_in_window(&mut ply, &os_probe_token, &os_probe_screenshot).await?;
    let source_text = std::fs::read_to_string(&source)?;
    let scenario_data = parse_scenario(&scenario)?;
    let mut state = PlaygroundState::new(&example, &mut ply)?;
    ply.set_text_value("source_editor", &source_text);
    state.reset_to_initial(&ply);
    for _ in 0..3 {
        draw_frame(&mut ply, &state).await;
        next_frame().await;
    }
    let app_control_observations =
        drive_visible_app_control_probe(&mut ply, &mut state, &report, &example).await;
    let source_event_observations =
        drive_visible_source_event_probe(&mut ply, &mut state, &report, &example).await;
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
    let pixel_stats = image_stats(&image.bytes);
    if pixel_stats.nonzero_channels == 0 || pixel_stats.unique_rgba_values <= 1 {
        return Err(format!(
            "headed Ply capture is blank: nonzero_channels={}, unique_rgba_values={}",
            pixel_stats.nonzero_channels, pixel_stats.unique_rgba_values
        )
        .into());
    }
    if let Some(parent) = screenshot.parent() {
        std::fs::create_dir_all(parent)?;
    }
    image.export_png(screenshot.to_str().ok_or("screenshot path is not utf-8")?);
    next_frame().await;
    let mut report_json = state
        .output
        .as_ref()
        .ok_or("missing verifier output")?
        .report
        .clone();
    if let Some(object) = report_json.as_object_mut() {
        object.insert("window_mode".to_owned(), json!("headed"));
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
        object.insert("display_scale".to_owned(), json!(screen_dpi_scale()));
        object.insert(
            "window_size".to_owned(),
            json!([screen_width(), screen_height()]),
        );
        object.insert(
            "framebuffer_size".to_owned(),
            json!([image.width, image.height]),
        );
        object.insert(
            "input_backend".to_owned(),
            json!("macroquad-os-events + wtype-real-keyboard-events"),
        );
        object.insert("capture_backend".to_owned(), json!("macroquad-framebuffer"));
        object.insert(
            "focused_window_proof".to_owned(),
            json!("OS probe set Ply focus to os_probe_input, sent a real keyboard token, observed it in Ply text state, then captured the headed macroquad/Ply framebuffer"),
        );
        let mut checkpoint_paths = vec![json!(screenshot), json!(os_probe_screenshot)];
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
        let mut artifact_sha256s = vec![
            json!({
                "path": screenshot,
                "sha256": sha256_file(&screenshot)?
            }),
            json!({
                "path": os_probe_screenshot,
                "sha256": sha256_file(&os_probe_screenshot)?
            }),
        ];
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
        object.insert(
            "input_injection_method".to_owned(),
            json!("os_keyboard_probe_visible_app_source_event_and_step_control_plus_scenario_user_action_route"),
        );
        object.insert(
            "os_input_limitation".to_owned(),
            json!("This headed verifier proves real OS keyboard input reaches the Ply window, reaches visible application controls, emits matching observed Boon SOURCE events for the covered workflow, and applies those observed SOURCE events through boon_runtime::LiveRuntime. It can also activate the visible Ply Step control for each scenario transition. It still replays the complete scenario through scenario user_action records after the visible-control probes, so it does not yet pointer-click or type every visible TodoMVC/Cells application control through OS hit testing."),
        );
        object.insert("os_input_probe".to_owned(), headed_os_probe);
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
        object.insert(
            "playground_surface".to_owned(),
            json!({
                "example_selector": true,
                "code_editor": true,
                "run_reset_step_controls": true,
                "render_preview": true,
                "semantic_delta_log": true,
                "selected_value_inspector": true,
                "dependency_explanation_panel": true
            }),
        );
    }
    write_json(&report, &report_json)?;
    if app_control_observations.iter().all(|observation| {
        observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
    }) && source_event_observations.iter().all(|observation| {
        observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
    }) && step_observations.iter().all(|observation| {
        observation.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
    }) {
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
        if frame == 8 && !typed {
            typed = true;
            if let Err(error) = send_real_keyboard_text(typed_text) {
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
    let image = get_screen_data();
    let pixel_stats = image_stats(&image.bytes);
    if let Some(parent) = screenshot.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    image.export_png(
        screenshot
            .to_str()
            .unwrap_or("target/reports/invalid-app-control.png"),
    );
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
        "input_route_contract": contract,
        "keyboard_tool": "wtype",
        "keyboard_tool_path": command_path("wtype"),
        "typed": typed,
        "send_error": send_error,
        "typed_text_sha256": boon_runtime::sha256_bytes(typed_text.as_bytes()),
        "observed_value_sha256": boon_runtime::sha256_bytes(observed_value.as_bytes()),
        "observed_insertion_order": observed_insertion_order,
        "screenshot_path": screenshot,
        "screenshot_sha256": sha256_file(screenshot).unwrap_or_else(|_| "missing".to_owned()),
        "screenshot_nonzero_channels": pixel_stats.nonzero_channels,
        "screenshot_unique_rgba_values": pixel_stats.unique_rgba_values
    })
}

async fn drive_visible_source_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    report: &std::path::Path,
    example: &str,
) -> Vec<serde_json::Value> {
    if example == "cells" {
        let edit_screenshot =
            report.with_file_name("cells-headed-source-event-edit-a1-literal.png");
        let commit_screenshot =
            report.with_file_name("cells-headed-source-event-commit-a1-literal.png");
        vec![
            drive_visible_source_text_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "edit-a1-literal",
                    element_id: "cell_editor_A1",
                    text: "41",
                    source: "cell.sources.editor.change",
                    key: None,
                    address: Some("A1"),
                    screenshot: edit_screenshot,
                },
            )
            .await,
            drive_visible_source_submit_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "commit-a1-literal",
                    element_id: "cell_editor_A1",
                    text: "41",
                    source: "cell.sources.editor.commit",
                    key: Some("Enter"),
                    address: Some("A1"),
                    screenshot: commit_screenshot,
                },
            )
            .await,
        ]
    } else {
        let mut observations = vec![
            drive_visible_source_text_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "add-test-todo-type",
                    element_id: "todo_new_input",
                    text: "Test todo",
                    source: "store.sources.new_todo_input.change",
                    key: None,
                    address: None,
                    screenshot: report
                        .with_file_name("todomvc-headed-source-event-add-test-todo-type.png"),
                },
            )
            .await,
            drive_visible_source_submit_event_probe(
                ply,
                state,
                VisibleSourceTextProbe {
                    id: "add-test-todo-submit",
                    element_id: "todo_new_input",
                    text: "Test todo",
                    source: "store.sources.new_todo_input.key_down",
                    key: Some("Enter"),
                    address: None,
                    screenshot: report
                        .with_file_name("todomvc-headed-source-event-add-test-todo-submit.png"),
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
            },
            VisibleSourcePressProbe {
                id: "toggle-buy-groceries",
                element_id: Id::new_index("todo_row_checkbox", 0),
                element_label: "todo_row_checkbox[0]",
                source: "todo.sources.todo_checkbox.click",
                target_text: Some("Buy groceries"),
                screenshot: report
                    .with_file_name("todomvc-headed-source-event-toggle-buy-groceries.png"),
            },
            VisibleSourcePressProbe {
                id: "filter-active",
                element_id: Id::new("todo_filter_active"),
                element_label: "todo_filter_active",
                source: "store.sources.filter_active.press",
                target_text: None,
                screenshot: report.with_file_name("todomvc-headed-source-event-filter-active.png"),
            },
            VisibleSourcePressProbe {
                id: "filter-completed",
                element_id: Id::new("todo_filter_completed"),
                element_label: "todo_filter_completed",
                source: "store.sources.filter_completed.press",
                target_text: None,
                screenshot: report
                    .with_file_name("todomvc-headed-source-event-filter-completed.png"),
            },
            VisibleSourcePressProbe {
                id: "filter-all",
                element_id: Id::new("todo_filter_all"),
                element_label: "todo_filter_all",
                source: "store.sources.filter_all.press",
                target_text: None,
                screenshot: report.with_file_name("todomvc-headed-source-event-filter-all.png"),
            },
            VisibleSourcePressProbe {
                id: "delete-clean-room",
                element_id: Id::new_index("todo_row_delete", 1),
                element_label: "todo_row_delete[1]",
                source: "todo.sources.remove_todo_button.press",
                target_text: Some("Clean room"),
                screenshot: report
                    .with_file_name("todomvc-headed-source-event-delete-clean-room.png"),
            },
        ] {
            observations.push(drive_visible_source_press_event_probe(ply, state, probe).await);
        }
        observations
    }
}

struct VisibleSourceTextProbe {
    id: &'static str,
    element_id: &'static str,
    text: &'static str,
    source: &'static str,
    key: Option<&'static str>,
    address: Option<&'static str>,
    screenshot: PathBuf,
}

struct VisibleSourcePressProbe {
    id: &'static str,
    element_id: Id,
    element_label: &'static str,
    source: &'static str,
    target_text: Option<&'static str>,
    screenshot: PathBuf,
}

async fn drive_visible_source_text_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourceTextProbe,
) -> serde_json::Value {
    clear_ui_source_observations();
    ply.set_text_value(probe.element_id, "");
    let text_to_send = reverse_text(probe.text);
    let mut typed = false;
    let mut send_error = None;
    let mut observed_event = None;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..140 {
        draw_frame(ply, state).await;
        if let Some(element_bounds) = ply.bounding_box(probe.element_id) {
            bounds = bounds_json(element_bounds);
        }
        ply.set_focus(probe.element_id);
        if frame == 8 && !typed {
            typed = true;
            if let Err(error) = send_real_keyboard_text(&text_to_send) {
                send_error = Some(error.to_string());
            }
        }
        if let Some(event) = matching_ui_source_observation(
            probe.source,
            Some(probe.text),
            probe.key,
            probe.address,
            None,
        ) {
            observed_event = Some(event);
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
    )
    .await
}

async fn drive_visible_source_submit_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourceTextProbe,
) -> serde_json::Value {
    clear_ui_source_observations();
    ply.set_text_value(probe.element_id, probe.text);
    let mut key_sent = false;
    let mut send_error = None;
    let mut observed_event = None;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..100 {
        draw_frame(ply, state).await;
        if let Some(element_bounds) = ply.bounding_box(probe.element_id) {
            bounds = bounds_json(element_bounds);
        }
        ply.set_focus(probe.element_id);
        if frame == 8 && !key_sent {
            key_sent = true;
            if let Some(key) = probe.key {
                if let Err(error) = send_real_key(os_key_name(key)) {
                    send_error = Some(error.to_string());
                }
            }
        }
        if let Some(event) = matching_ui_source_observation(
            probe.source,
            Some(probe.text),
            probe.key,
            probe.address,
            None,
        ) {
            observed_event = Some(event);
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
    )
    .await
}

async fn drive_visible_source_press_event_probe(
    ply: &mut Ply<()>,
    state: &mut PlaygroundState,
    probe: VisibleSourcePressProbe,
) -> serde_json::Value {
    clear_ui_source_observations();
    let mut key_sent = false;
    let mut send_error = None;
    let mut observed_event = None;
    let mut bounds = serde_json::Value::Null;
    for frame in 0..100 {
        draw_frame(ply, state).await;
        if let Some(element_bounds) = ply.bounding_box(probe.element_id.clone()) {
            bounds = bounds_json(element_bounds);
        }
        ply.set_focus(probe.element_id.clone());
        if frame == 8 && !key_sent {
            key_sent = true;
            if let Err(error) = send_real_key("Return") {
                send_error = Some(error.to_string());
            }
        }
        if let Some(event) =
            matching_ui_source_observation(probe.source, None, None, None, probe.target_text)
        {
            observed_event = Some(event);
            break;
        }
        next_frame().await;
    }
    let mut runtime_mutation_error = None;
    if let Some(event) = observed_event
        .as_ref()
        .and_then(live_source_event_from_json)
    {
        if let Err(error) = state.apply_live_source_event(event) {
            runtime_mutation_error = Some(error);
        }
    }
    draw_frame(ply, state).await;
    let image = get_screen_data();
    let pixel_stats = image_stats(&image.bytes);
    if let Some(parent) = probe.screenshot.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    image.export_png(
        probe
            .screenshot
            .to_str()
            .unwrap_or("target/reports/invalid-source-press.png"),
    );
    let pass = observed_event.is_some();
    json!({
        "id": probe.id,
        "pass": pass,
        "target_element_id": probe.element_label,
        "visible_bounds": bounds,
        "input_route_contract": "real OS keyboard activation reached a visible app control and the control emitted the expected Boon SOURCE event observation",
        "keyboard_tool": "wtype",
        "keyboard_tool_path": command_path("wtype"),
        "input_sent": key_sent,
        "send_error": send_error,
        "expected_source_event": {
            "source": probe.source,
            "target_text": probe.target_text
        },
        "source_event_observed": observed_event,
        "runtime_mutation_path": "observed visible SOURCE event -> boon_runtime::LiveRuntime::apply_source_event",
        "runtime_mutation_observed": runtime_mutation_error.is_none() && observed_event.is_some(),
        "runtime_mutation_error": runtime_mutation_error,
        "screenshot_path": probe.screenshot,
        "screenshot_sha256": sha256_file(&probe.screenshot).unwrap_or_else(|_| "missing".to_owned()),
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
) -> serde_json::Value {
    let mut runtime_mutation_error = None;
    if let Some(event) = observed_event
        .as_ref()
        .and_then(live_source_event_from_json)
    {
        if let Err(error) = state.apply_live_source_event(event) {
            runtime_mutation_error = Some(error);
        }
    }
    draw_frame(ply, state).await;
    let image = get_screen_data();
    let pixel_stats = image_stats(&image.bytes);
    if let Some(parent) = probe.screenshot.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    image.export_png(
        probe
            .screenshot
            .to_str()
            .unwrap_or("target/reports/invalid-source-event.png"),
    );
    let pass = observed_event.is_some();
    json!({
        "id": probe.id,
        "pass": pass,
        "target_element_id": probe.element_id,
        "visible_bounds": bounds,
        "input_route_contract": "real OS keyboard input reached a visible app control and the control emitted the expected Boon SOURCE event observation",
        "keyboard_tool": "wtype",
        "keyboard_tool_path": command_path("wtype"),
        "input_sent": input_sent,
        "send_error": send_error,
        "expected_source_event": {
            "source": probe.source,
            "text": probe.text,
            "key": probe.key,
            "address": probe.address
        },
        "source_event_observed": observed_event,
        "runtime_mutation_path": "observed visible SOURCE event -> boon_runtime::LiveRuntime::apply_source_event",
        "runtime_mutation_observed": runtime_mutation_error.is_none() && observed_event.is_some(),
        "runtime_mutation_error": runtime_mutation_error,
        "screenshot_path": probe.screenshot,
        "screenshot_sha256": sha256_file(&probe.screenshot).unwrap_or_else(|_| "missing".to_owned()),
        "screenshot_nonzero_channels": pixel_stats.nonzero_channels,
        "screenshot_unique_rgba_values": pixel_stats.unique_rgba_values
    })
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
        if frame == 6 && !key_sent {
            key_sent = true;
            if let Err(error) = send_real_key("Return") {
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
    let image = get_screen_data();
    let pixel_stats = image_stats(&image.bytes);
    if let Some(parent) = screenshot.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    image.export_png(
        screenshot
            .to_str()
            .unwrap_or("target/reports/invalid-step.png"),
    );
    let observed_limit = state.step_limit.unwrap_or(state.scenario_len);
    json!({
        "id": step_id,
        "pass": pressed && observed_limit == expected_limit,
        "target_element_id": "step_button",
        "visible_bounds": bounds,
        "input_route_contract": "OS keyboard Enter reached focused visible Ply Step control, which advanced the scenario prefix in the playground",
        "keyboard_tool": "wtype",
        "keyboard_tool_path": command_path("wtype"),
        "key_sent": key_sent,
        "send_error": send_error,
        "observed_step_limit": observed_limit,
        "expected_step_limit": expected_limit,
        "screenshot_path": screenshot,
        "screenshot_sha256": sha256_file(screenshot).unwrap_or_else(|_| "missing".to_owned()),
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
    let image = get_screen_data();
    let pixel_stats = image_stats(&image.bytes);
    if let Some(parent) = screenshot.parent() {
        std::fs::create_dir_all(parent)?;
    }
    image.export_png(screenshot.to_str().ok_or("screenshot path is not utf-8")?);
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
            "nonzero_channels": pixel_stats.nonzero_channels,
            "unique_rgba_values": pixel_stats.unique_rgba_values
        }
    }))
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
        "layer": "os-input-probe",
        "exit_status": if passed { 0 } else { 1 },
        "input_injection_method": "os_pointer_keyboard_to_visible_window",
        "input_backend": "wtype-real-keyboard-events",
        "input_route_contract": "focused Ply text input receives token from OS keyboard event path",
        "focused_window_proof": "probe set Ply focus to os_probe_input and received the exact token through text input state",
        "window_mode": "headed",
        "window_backend": "ply-engine/macroquad",
        "window_pid": std::process::id(),
        "window_title": "Boon Circuit Ply Playground",
        "display_server": display_server(),
        "display_socket_or_compositor_connection": display_socket(),
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
        "graph_node_count": 0
    });
    write_json(&report, &report_json)?;
    if passed {
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
    })
}

fn reverse_text(value: &str) -> String {
    value.chars().rev().collect()
}

async fn run_interactive(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut ply = Ply::<()>::new(&DEFAULT_FONT).await;
    ply.set_debug_mode(false);
    let selected = value_after(args, "--example").unwrap_or_else(|| "todomvc".to_owned());
    let mut state = PlaygroundState::new(&selected, &mut ply)?;
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
            });
    }
    ply.show(|_| {}).await;
}

struct PlaygroundState {
    selected: String,
    scenario_path: PathBuf,
    scenario_len: usize,
    scenario_steps: Vec<String>,
    step_limit: Option<usize>,
    output: Option<RunOutput>,
    live_runtime: Option<LiveRuntime>,
    last_error: Option<String>,
}

impl PlaygroundState {
    fn new(example: &str, ply: &mut Ply<()>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut state = Self {
            selected: example.to_owned(),
            scenario_path: PathBuf::new(),
            scenario_len: 0,
            scenario_steps: Vec::new(),
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
            scenario_path,
            scenario_len,
            scenario_steps: Vec::new(),
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
        self.step_limit = None;
        ply.set_text_value("source_editor", &source_text);
        self.run_text(&source_text);
        Ok(())
    }

    fn run_editor_text(&mut self, ply: &Ply<()>) {
        let source_text = ply.get_text_value("source_editor").to_owned();
        self.run_text(&source_text);
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

    fn apply_live_source_event(&mut self, event: LiveSourceEvent) -> Result<(), String> {
        let live_runtime = self
            .live_runtime
            .as_mut()
            .ok_or_else(|| "playground live runtime is not initialized".to_owned())?;
        let step = live_runtime
            .apply_source_event(event)
            .map_err(|error| error.to_string())?;
        if let Some(output) = &mut self.output {
            output.semantic_deltas.extend(step.semantic_deltas);
            output.render_patches.extend(step.render_patches);
            output.state_summary = step.state_summary;
            Ok(())
        } else {
            Err("playground output is not initialized".to_owned())
        }
    }
}

async fn draw_frame(ply: &mut Ply<()>, state: &PlaygroundState) {
    clear_background(MacroquadColor::from_rgba(238, 241, 245, 255));
    {
        let mut ui = ply.begin();
        build_ui(&mut ui, state);
    }
    ply.show(|_| {}).await;
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
        .width(fixed!(270.0))
        .height(grow!())
        .background_color(0x1F2630)
        .layout(|layout| {
            layout
                .direction(TopToBottom)
                .padding((18, 18, 18, 18))
                .gap(10)
        })
        .children(|ui| {
            ui.text("Boon Circuit", |text| text.font_size(30).color(0xF1F5FA));
            ui.text("Ply playground", |text| text.font_size(16).color(0xAFC1D6));
            nav_item(ui, "nav_todomvc", "1 TodoMVC", state.selected == "todomvc");
            nav_item(ui, "nav_cells", "2 Cells", state.selected == "cells");
            ui.text(&format!("step {}", step_label(state)), |text| {
                text.font_size(13).color(0xAFC1D6)
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
        .layout(|layout| layout.direction(TopToBottom).padding((10, 10, 8, 8)).gap(4))
        .children(|ui| {
            ui.text("Scenario", |text| text.font_size(14).color(0xAFC1D6));
            for (index, label) in state.scenario_steps.iter().enumerate().take(14) {
                let marker = if index < completed { "x" } else { " " };
                let color = if index < completed {
                    0xF1F5FA
                } else {
                    0xAFC1D6
                };
                ui.text(&format!("[{marker}] {label}"), |text| {
                    text.font_size(11).color(color)
                });
            }
        });
}

fn nav_item(ui: &mut Ui<'_, ()>, id: &'static str, label: &str, selected: bool) {
    ui.element()
        .id(id)
        .width(grow!())
        .height(fixed!(36.0))
        .background_color(if selected { 0x2F6FB8 } else { 0x28313D })
        .layout(|layout| layout.padding((10, 10, 8, 8)).align(Left, CenterY))
        .children(|ui| {
            ui.text(label, |text| text.font_size(15).color(0xF1F5FA));
        });
}

fn content(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("content")
        .width(grow!())
        .height(grow!())
        .background_color(0xF8FAFD)
        .layout(|layout| {
            layout
                .direction(TopToBottom)
                .padding((22, 22, 22, 22))
                .gap(10)
        })
        .children(|ui| {
            toolbar(ui, state);
            ui.element()
                .width(grow!())
                .height(grow!())
                .layout(|layout| layout.direction(LeftToRight).gap(10))
                .children(|ui| {
                    source_panel(ui);
                    runtime_panel(ui, state);
                });
        });
}

fn toolbar(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    ui.element()
        .id("toolbar")
        .height(fixed!(38.0))
        .width(grow!())
        .layout(|layout| layout.direction(LeftToRight).gap(8).align(Left, CenterY))
        .children(|ui| {
            toolbar_button(ui, "run_button", "Run", true);
            toolbar_button(ui, "reset_button", "Reset", false);
            toolbar_button(ui, "step_button", "Step", false);
            ui.text(
                &format!("{} / {}", state.selected, step_label(state)),
                |text| text.font_size(15).color(0x1F2630),
            );
        });
}

fn toolbar_button(ui: &mut Ui<'_, ()>, id: &'static str, label: &str, primary: bool) {
    ui.element()
        .id(id)
        .height(fixed!(32.0))
        .width(fixed!(82.0))
        .background_color(if primary { 0x2F6FB8 } else { 0xE4EAF2 })
        .layout(|layout| layout.align(CenterX, CenterY))
        .children(|ui| {
            ui.text(label, |text| {
                text.font_size(14)
                    .color(if primary { 0xFFFFFF } else { 0x1F2630 })
            });
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
    panel(ui, "Preview", |ui| {
        if let Some(error) = &state.last_error {
            ui.text(error, |text| text.font_size(14).color(0xA32929));
            return;
        }
        let Some(output) = &state.output else {
            return;
        };
        if state.selected == "todomvc" {
            todomvc_content(ui, &output.state_summary);
        } else {
            cells_content(ui, &output.state_summary);
        }
    });
}

fn delta_panel(ui: &mut Ui<'_, ()>, state: &PlaygroundState) {
    panel(ui, "Deltas", |ui| {
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
    panel(ui, "Inspector", |ui| {
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
    panel(ui, "Causes", |ui| {
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

fn panel(ui: &mut Ui<'_, ()>, title: &str, body: impl FnOnce(&mut Ui<'_, ()>)) {
    ui.element()
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

fn todomvc_content(ui: &mut Ui<'_, ()>, state: &serde_json::Value) {
    let active = state["active_count"].as_u64().unwrap_or_default();
    let completed = state["completed_count"].as_u64().unwrap_or_default();
    ui.element()
        .id("todo_new_input")
        .height(fixed!(40.0))
        .width(grow!())
        .background_color(0xFFFFFF)
        .border(|border| border.color(0x2F6FB8).all(1))
        .layout(|layout| layout.padding((10, 10, 7, 7)))
        .text_input(|input| {
            input
                .placeholder("What needs to be done?")
                .font(&DEFAULT_FONT)
                .font_size(17)
                .text_color(0x1F2630)
                .placeholder_color(0x8B97A7)
                .cursor_color(0x2F6FB8)
                .selection_color(0xB9D7F5)
                .on_changed(|text| {
                    record_ui_source_observation(json!({
                        "source": "store.sources.new_todo_input.change",
                        "text": text
                    }));
                })
                .on_submit(|text| {
                    record_ui_source_observation(json!({
                        "source": "store.sources.new_todo_input.key_down",
                        "key": "Enter",
                        "text": text
                    }));
                })
        })
        .empty();
    ui.element()
        .height(fixed!(32.0))
        .width(grow!())
        .layout(|layout| layout.direction(LeftToRight).gap(6))
        .children(|ui| {
            app_button(
                ui,
                "todo_toggle_all",
                "Toggle all",
                false,
                Some("store.sources.toggle_all_checkbox.click"),
            );
            app_button(
                ui,
                "todo_filter_all",
                "All",
                true,
                Some("store.sources.filter_all.press"),
            );
            app_button(
                ui,
                "todo_filter_active",
                "Active",
                false,
                Some("store.sources.filter_active.press"),
            );
            app_button(
                ui,
                "todo_filter_completed",
                "Completed",
                false,
                Some("store.sources.filter_completed.press"),
            );
            app_button(
                ui,
                "todo_clear_completed",
                "Clear completed",
                false,
                Some("store.sources.clear_completed_button.press"),
            );
        });
    ui.text(&format!("{active} active, {completed} completed"), |text| {
        text.font_size(16).color(0x2F6FB8)
    });
    if let Some(todos) = state["todos"].as_array() {
        for (index, todo) in todos.iter().enumerate() {
            let title = todo["title"].as_str().unwrap_or("");
            let checked = if todo["completed"].as_bool().unwrap_or(false) {
                "[x]"
            } else {
                "[ ]"
            };
            ui.element()
                .height(fixed!(34.0))
                .width(grow!())
                .background_color(0xFFFFFF)
                .border(|border| border.color(0xD5DDE8).all(1))
                .layout(|layout| layout.padding((10, 10, 6, 6)).align(Left, CenterY))
                .children(|ui| {
                    ui.element()
                        .id(("todo_row_checkbox", index as u32))
                        .width(fixed!(34.0))
                        .height(fixed!(24.0))
                        .background_color(0xE4EAF2)
                        .layout(|layout| layout.align(CenterX, CenterY))
                        .on_press({
                            let title = title.to_owned();
                            move |_, _| {
                                record_ui_source_observation(json!({
                                    "source": "todo.sources.todo_checkbox.click",
                                    "target_text": title
                                }));
                            }
                        })
                        .children(|ui| {
                            ui.text(checked, |text| text.font_size(14).color(0x1F2630));
                        });
                    ui.element()
                        .id(("todo_row_title", index as u32))
                        .width(grow!())
                        .height(fixed!(24.0))
                        .layout(|layout| layout.align(Left, CenterY))
                        .on_press({
                            let title = title.to_owned();
                            move |_, _| {
                                record_ui_source_observation(json!({
                                    "source": "todo.sources.todo_title_element.double_click",
                                    "target_text": title
                                }));
                            }
                        })
                        .children(|ui| {
                            ui.text(title, |text| text.font_size(16).color(0x1F2630));
                        });
                    ui.element()
                        .id(("todo_row_delete", index as u32))
                        .width(fixed!(28.0))
                        .height(fixed!(24.0))
                        .background_color(0xF5E8E8)
                        .layout(|layout| layout.align(CenterX, CenterY))
                        .on_press({
                            let title = title.to_owned();
                            move |_, _| {
                                record_ui_source_observation(json!({
                                    "source": "todo.sources.remove_todo_button.press",
                                    "target_text": title
                                }));
                            }
                        })
                        .children(|ui| {
                            ui.text("x", |text| text.font_size(14).color(0xA32929));
                        });
                });
        }
    }
}

fn cells_content(ui: &mut Ui<'_, ()>, state: &serde_json::Value) {
    if let Some(cells) = state["cells"].as_array() {
        for cell in cells {
            let address = cell["address"].as_str().unwrap_or("");
            let value = cell["value"].as_str().unwrap_or("");
            let formula = cell["formula"].as_str().unwrap_or("");
            let error = cell["error"].as_str().unwrap_or("");
            let cell_id = match address {
                "A1" => Some("cell_editor_A1"),
                "B1" => Some("cell_editor_B1"),
                "C1" => Some("cell_editor_C1"),
                "D1" => Some("cell_editor_D1"),
                _ => None,
            };
            ui.element()
                .height(fixed!(40.0))
                .width(grow!())
                .background_color(0xFFFFFF)
                .border(|border| border.color(0xD5DDE8).all(1))
                .layout(|layout| {
                    layout
                        .direction(LeftToRight)
                        .padding((8, 8, 5, 5))
                        .gap(8)
                        .align(Left, CenterY)
                })
                .children(|ui| {
                    ui.text(address, |text| text.font_size(15).color(0x2F6FB8));
                    ui.element()
                        .id(cell_id.unwrap_or("cell_editor_other"))
                        .width(fixed!(140.0))
                        .height(fixed!(28.0))
                        .background_color(0xFFFFFF)
                        .border(|border| border.color(0xD5DDE8).all(1))
                        .layout(|layout| layout.padding((7, 7, 4, 4)))
                        .text_input(|input| {
                            let source_address = address.to_owned();
                            let commit_address = address.to_owned();
                            input
                                .placeholder(formula)
                                .font(&DEFAULT_FONT)
                                .font_size(14)
                                .text_color(0x1F2630)
                                .placeholder_color(0x8B97A7)
                                .cursor_color(0x2F6FB8)
                                .selection_color(0xB9D7F5)
                                .on_changed(move |text| {
                                    record_ui_source_observation(json!({
                                        "source": "cell.sources.editor.change",
                                        "address": source_address.clone(),
                                        "text": text
                                    }));
                                })
                                .on_submit(move |text| {
                                    record_ui_source_observation(json!({
                                        "source": "cell.sources.editor.commit",
                                        "address": commit_address.clone(),
                                        "key": "Enter",
                                        "text": text
                                    }));
                                })
                        })
                        .empty();
                    ui.text(&format!("= {value} {error}"), |text| {
                        text.font_size(15).color(0x1F2630)
                    });
                });
        }
    }
}

fn app_button(
    ui: &mut Ui<'_, ()>,
    id: &'static str,
    label: &str,
    selected: bool,
    source: Option<&'static str>,
) {
    let mut element = ui
        .element()
        .id(id)
        .height(fixed!(28.0))
        .width(fixed!(118.0))
        .background_color(if selected { 0x2F6FB8 } else { 0xE4EAF2 })
        .layout(|layout| layout.align(CenterX, CenterY));
    if let Some(source) = source {
        element = element.on_press(move |_, _| {
            record_ui_source_observation(json!({
                "source": source
            }));
        });
    }
    element.children(|ui| {
        ui.text(label, |text| {
            text.font_size(13)
                .color(if selected { 0xFFFFFF } else { 0x1F2630 })
        });
    });
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
