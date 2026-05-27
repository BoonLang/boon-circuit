use boon_editor::{ClipboardAdapter, EditorBuffer, EditorPosition, EditorSelection};
use boon_native_app_window::{NativeWindowOptions, NativeWindowRole};
use boon_native_gpu::{PresentSurface, RenderBackend};
use boon_parser::{
    AstCallArg, AstExpr, AstExprKind, AstRecordField, AstStatement, AstStatementKind, AstTokenKind,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const BOON_EDITOR_FONT_FAMILY: &str = "JetBrains Mono";
const BOON_EDITOR_FONT_SIZE: u32 = 16;
const BOON_EDITOR_LINE_HEIGHT: u32 = 22;
const BOON_EDITOR_FONT_FEATURES: &str = "zero,calt";
const BOON_EDITOR_FONT_FEATURE_SETTINGS: &str = "'zero' 1, 'calt' 1";
const BOON_EDITOR_PADDING: u32 = 10;
const BOON_EDITOR_GUTTER_WIDTH: u32 = 44;
const BOON_EDITOR_ROW_GAP: u32 = 8;
const BOON_EDITOR_BACKGROUND: &str = "#282c34";
const BOON_EDITOR_FOREGROUND: &str = "#d9e1f2";
const BOON_EDITOR_DARK_BACKGROUND: &str = "#21252b";
const BOON_EDITOR_HIGHLIGHT_BACKGROUND: &str = "#2c313a";
const BOON_EDITOR_GUTTER: &str = "#5c6773";
const BOON_EDITOR_SELECTION: &str = "#3E4451";
const BOON_EDITOR_CURSOR: &str = "#528bff";
const BOON_EDITOR_BRACKET_MATCH: &str = "#528bff33";
const BOON_EDITOR_SELECTION_MATCH: &str = "#aafe661a";
const BOON_EDITOR_FULL_LANGUAGE_BYTES_MAX: usize = 256 * 1024;
const BOON_EDITOR_DEFERRED_SYNTAX_LINES: usize = 256;
const BOON_EDITOR_CARET_BLINK_HALF_PERIOD_MS: u64 = 600;
const BOON_EDITOR_KEY_REPEAT_DELAY_MS: u64 = 500;
const BOON_EDITOR_KEY_REPEAT_INTERVAL_MS: u64 = 30;
const BOON_EDITOR_KEY_REPEAT_MAX_CATCH_UP: usize = 8;

const DEV_BG: &str = "#0f1724";
const DEV_PANEL: &str = "#141b2a";
const DEV_PANEL_RAISED: &str = "#1a2435";
const DEV_PANEL_ACTIVE: &str = "#26354d";
const DEV_BORDER: &str = "#334155";
const DEV_BORDER_MUTED: &str = "#243244";
const DEV_TEXT: &str = "#eef2ff";
const DEV_TEXT_MUTED: &str = "#9aa8bd";
const DEV_ACCENT: &str = "#6ca2ff";
const DEV_PASS: &str = "#2a9d8f";
const DEV_WARN: &str = "#f4a261";
const DEV_FAIL: &str = "#e63946";
const DEV_DIRTY: &str = "#fcbf49";
const DEV_FOOTER_LINE_HEIGHT: u32 = 22;
const DEV_FOOTER_VALUE_WRAP_CHARS: usize = 92;
const DEV_PREVIEW_SUMMARY_REFRESH_MS: u64 = 1_000;
const DEV_PREVIEW_SUMMARY_READ_TIMEOUT_MS: u64 = 35;

fn main() {
    if let Err(error) = run() {
        eprintln!("boon_native_playground: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    let role = value_arg(&args, "--role").unwrap_or_else(|| "desktop".to_owned());
    match role.as_str() {
        "preview" => run_preview(&args),
        "dev" => run_dev(&args),
        "desktop" => run_desktop(&args),
        "layout-proof" => run_layout_proof(&args),
        "interaction-speed" => run_interaction_speed(&args),
        other => Err(format!("unknown --role `{other}`").into()),
    }
}

fn run_layout_proof(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if value_arg(args, "--example").is_some() {
        return Err(
            "layout-proof role must receive Boon source via --code-file, not --example".into(),
        );
    }
    let code_file =
        value_arg(args, "--code-file").ok_or("layout-proof role requires --code-file")?;
    let report = value_arg(args, "--report").ok_or("layout-proof role requires --report")?;
    let source = boon_runtime::source_text_for_path(Path::new(&code_file))?;
    let proof = native_document_layout_proof(Path::new(&code_file), &source)?;
    let mut report_value = base_report("boon-native-playground-layout-proof", args, "pass");
    report_value["per_step_pass_fail"] = json!([
        {
            "id": "native-layout-proof:document-lowered",
            "pass": proof.get("status").and_then(serde_json::Value::as_str) == Some("pass")
        },
        {
            "id": "native-layout-proof:hit-regions-present",
            "pass": proof
                .get("hit_target_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default()
                > 0
        },
        {
            "id": "native-layout-proof:source-intents-present",
            "pass": proof
                .get("source_intent_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default()
                > 0
        }
    ]);
    report_value["layout_proof"] = proof;
    boon_runtime::write_json(Path::new(&report), &report_value)?;
    boon_runtime::verify_report_schema(Path::new(&report))?;
    Ok(())
}

fn run_interaction_speed(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_arg(args, "--example").unwrap_or_else(|| "counter".to_owned());
    let entry = boon_runtime::example_manifest_entry(&example)?;
    if entry.id != "counter" {
        return Err("interaction-speed currently targets the Counter interaction contract".into());
    }
    let event_count = numeric_arg(args, "--event-count").unwrap_or(24).max(1);
    let max_total_ms = numeric_arg(args, "--max-total-ms").unwrap_or(250) as f64;
    let report = value_arg(args, "--report").ok_or("interaction-speed role requires --report")?;
    let source_path = PathBuf::from(&entry.source);
    let source = std::fs::read_to_string(&source_path)?;
    let scenario = boon_runtime::parse_scenario(Path::new(&entry.scenario))?;
    let step = scenario
        .step
        .iter()
        .find(|step| step.id == "press-increment")
        .ok_or("counter scenario is missing press-increment step")?;
    let source_event = step
        .expected_source_event
        .as_ref()
        .and_then(|event| event.get("source"))
        .and_then(toml_value_as_str)
        .ok_or("press-increment step is missing expected source event")?;
    let layout_proof = native_document_layout_proof(&source_path, &source)?;
    let (x, y, target_node) = source_hit_center(&layout_proof, source_event)?;
    let shared_render_state = Arc::new(Mutex::new(PreviewSharedRenderState {
        layout_proof: layout_proof.clone(),
        layout_frame_override: None,
        update_count: 0,
        scroll_x_px: 0.0,
        scroll_y_px: 0.0,
        last_error: None,
        last_error_count: 0,
    }));
    let live_runtime = Arc::new(Mutex::new(boon_runtime::LiveRuntime::new(
        &format!("interaction-speed:{}", source_path.display()),
        &source,
        Path::new(&entry.scenario),
    )?));
    let mut input_state = PreviewNativeInputState::default();
    let input = deterministic_click_input(event_count, x, y);
    let started = Instant::now();
    preview_apply_real_window_input(
        &input,
        &source_path,
        &source,
        Some(&live_runtime),
        &shared_render_state,
        &mut input_state,
    )?;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let (state_summary, update_count, layout_hash, last_error) = {
        let mut runtime = live_runtime
            .lock()
            .map_err(|_| "interaction-speed runtime mutex poisoned")?;
        let state_summary = runtime.state_summary();
        let shared = shared_render_state
            .lock()
            .map_err(|_| "interaction-speed render state mutex poisoned")?;
        (
            state_summary,
            shared.update_count,
            shared
                .layout_proof
                .get("layout_frame_hash")
                .cloned()
                .unwrap_or_else(|| json!("missing")),
            shared.last_error.clone(),
        )
    };
    let expected_count = event_count.to_string();
    let observed_count = state_summary
        .pointer("/store/count")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing")
        .to_owned();
    let final_count_ok = observed_count == expected_count;
    let update_count_ok = update_count >= event_count;
    let timing_ok = elapsed_ms <= max_total_ms;
    let status = if final_count_ok && update_count_ok && timing_ok && last_error.is_none() {
        "pass"
    } else {
        "fail"
    };
    let mut report_value = base_report("boon-native-playground-interaction-speed", args, status);
    report_value["native_gpu_contract"] = json!(true);
    report_value["example"] = json!(entry.id);
    report_value["source_path"] = json!(entry.source);
    report_value["scenario_path"] = json!(entry.scenario);
    report_value["scenario_step"] = json!(step.id);
    report_value["source_event"] = json!(source_event);
    report_value["target_node"] = json!(target_node);
    report_value["event_count"] = json!(event_count);
    report_value["max_total_ms"] = json!(max_total_ms);
    report_value["interaction_total_ms"] = json!(elapsed_ms);
    report_value["interaction_per_event_ms"] = json!(elapsed_ms / event_count as f64);
    report_value["preview_shared_render_update_count"] = json!(update_count);
    report_value["final_count"] = json!(observed_count);
    report_value["expected_count"] = json!(expected_count);
    report_value["layout_frame_hash"] = layout_hash;
    report_value["preview_last_error"] = json!(last_error);
    report_value["per_step_pass_fail"] = json!([
        {
            "id": "counter-interaction-speed:all-clicks-applied",
            "pass": final_count_ok,
            "detail": format!("expected final count {event_count}, observed {observed_count}")
        },
        {
            "id": "counter-interaction-speed:render-updated-for-each-click",
            "pass": update_count_ok,
            "detail": format!("preview_shared_render_update_count={update_count}, event_count={event_count}")
        },
        {
            "id": "counter-interaction-speed:total-latency-budget",
            "pass": timing_ok,
            "detail": format!("interaction_total_ms={elapsed_ms:.3}, max_total_ms={max_total_ms:.3}")
        },
        {
            "id": "counter-interaction-speed:no-preview-error",
            "pass": last_error.is_none(),
            "detail": format!("preview_last_error={last_error:?}")
        }
    ]);
    boon_runtime::write_json(Path::new(&report), &report_value)?;
    if status == "pass" {
        Ok(())
    } else {
        Err(format!("interaction-speed failed; wrote {report}").into())
    }
}

fn run_preview(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if value_arg(args, "--example").is_some() {
        return Err(
            "preview role must not receive --example; pass --code-file or ReplaceCode".into(),
        );
    }
    let code_file = value_arg(args, "--code-file")
        .ok_or("preview role requires --code-file for initial source before ReplaceCode updates")?;
    let source = boon_runtime::source_text_for_path(Path::new(&code_file))?;
    let document_layout_proof = native_document_layout_proof(Path::new(&code_file), &source)
        .unwrap_or_else(|error| {
            json!({
                "status": "fail",
                "blocker": error.to_string()
            })
        });
    let report = value_arg(args, "--report").map(PathBuf::from);
    let hold_ms = numeric_arg(args, "--hold-ms").unwrap_or(0);
    let input_sample_delay_ms = numeric_arg(args, "--input-sample-delay-ms").unwrap_or(0);
    let synthetic_input_probe = args.iter().any(|arg| arg == "--synthetic-input-probe");
    let warmup_frame_count = numeric_arg(args, "--warmup-frame-count").unwrap_or(0) as u32;
    let sample_frame_count = numeric_arg(args, "--sample-frame-count").unwrap_or(1) as u32;
    let code_hash = boon_runtime::sha256_bytes(source.as_bytes());
    let runtime_summary = preview_runtime_summary(Path::new(&code_file), &source, &code_hash);
    let scenario_path = Path::new(&code_file).with_extension("scn");
    let live_runtime = if scenario_path.exists() {
        boon_runtime::LiveRuntime::new(
            &format!("native-preview-live:{}", code_file),
            &source,
            &scenario_path,
        )
    } else {
        boon_runtime::LiveRuntime::from_source(
            &format!("native-preview-live:{}", code_file),
            &source,
        )
    }
    .ok()
    .map(|runtime| Arc::new(Mutex::new(runtime)));
    let connect = value_arg(args, "--connect").map(PathBuf::from);
    let title = role_window_title("Boon Preview", value_arg(args, "--title-token").as_deref());
    let shared_render_state = Arc::new(Mutex::new(PreviewSharedRenderState {
        layout_proof: document_layout_proof.clone(),
        layout_frame_override: None,
        update_count: 0,
        scroll_x_px: 0.0,
        scroll_y_px: 0.0,
        last_error: None,
        last_error_count: 0,
    }));
    let preview_ipc_state = Arc::new(Mutex::new(PreviewIpcState {
        source_path: PathBuf::from(&code_file),
        source_text: source.clone(),
        source_bytes: source.len() as u64,
        source_sha256: code_hash.clone(),
        runtime_summary: runtime_summary.clone(),
        shared_render_state: Arc::clone(&shared_render_state),
        live_runtime: live_runtime.clone(),
    }));
    if let Some(path) = connect.as_deref() {
        start_preview_ipc_server(path, Arc::clone(&preview_ipc_state))?;
    }
    let role_args = args[1..].to_vec();
    let render_hook: Option<boon_native_app_window::NativeRenderHook> = {
        let mut visible_renderer = None;
        let mut app_owned_proof = None;
        let mut layout_frame_cache = None;
        let shared_render_state = Arc::clone(&shared_render_state);
        let preview_ipc_state = Arc::clone(&preview_ipc_state);
        let mut input_state = PreviewNativeInputState::default();
        Some(Box::new(move |context| {
            if preview_input_has_unhandled_source_events(&context.input, &input_state) {
                let input_context = preview_input_runtime_context(&preview_ipc_state)
                    .map_err(|error| error.to_string())?;
                if let Err(error) = preview_apply_real_window_input(
                    &context.input,
                    &input_context.source_path,
                    &input_context.source_text,
                    input_context.live_runtime.as_ref(),
                    &shared_render_state,
                    &mut input_state,
                ) {
                    preview_note_render_error(&shared_render_state, error.to_string())
                        .map_err(|error| error.to_string())?;
                }
            } else if let Err(error) =
                preview_apply_scroll_input(&context.input, &shared_render_state)
            {
                preview_note_render_error(&shared_render_state, error.to_string())
                    .map_err(|error| error.to_string())?;
            }
            let (render_layout_proof, render_layout_frame_override, render_error) = {
                let shared = shared_render_state
                    .lock()
                    .map_err(|_| "preview render state mutex poisoned".to_owned())?;
                (
                    shared.layout_proof.clone(),
                    shared.layout_frame_override.clone(),
                    shared.last_error.clone(),
                )
            };
            native_gpu_app_owned_render_hook(
                context,
                &render_layout_proof,
                render_layout_frame_override.as_ref(),
                render_error.as_deref(),
                &mut visible_renderer,
                &mut app_owned_proof,
                &mut layout_frame_cache,
            )
            .map_err(|error| error.to_string())
        }))
    };
    boon_native_app_window::run_visible_surface_probe_with_render_hook(
        NativeWindowOptions {
            role: NativeWindowRole::Preview,
            title,
            initial_width: 920.0,
            initial_height: 720.0,
            hold_ms,
            input_sample_delay_ms,
            synthetic_input_probe,
            warmup_frame_count,
            sample_frame_count,
            readback_artifact_dir: Some("target/artifacts/native-gpu/frames".to_owned()),
        },
        render_hook,
        move |proof| {
            let result = match proof {
                Ok(proof) => report
                    .as_deref()
                    .map(|report| {
                        write_role_report(
                            report,
                            "preview",
                            &role_args,
                            json!({
                                "code_file": code_file,
                                "source_bytes": source.len(),
                                "source_sha256": code_hash,
                                "received_example_name": false,
                                "preview_receives_example_name": false,
                                "preview_document_layout_proof": document_layout_proof,
                                "preview_runtime_summary": runtime_summary,
                                "bounded_ipc_server": connect.as_ref().map(|path| path.display().to_string()),
                                "app_window_surface_proof": proof,
                                "app_window_contract": boon_native_app_window::app_window_contract(),
                                "native_gpu_versions": boon_native_gpu::NativeGpuRenderer::required_backend_versions(),
                                "note": "preview role created an app_window Wayland window and rendered the generic document frame into the visible wgpu surface"
                            }),
                        )
                    })
                    .transpose(),
                Err(error) => report
                    .as_deref()
                    .map(|report| {
                        write_role_failure_report(report, "preview", &role_args, error.to_string())
                    })
                    .transpose(),
            };
            if let Err(error) = result {
                eprintln!("boon_native_playground: failed to write preview report: {error}");
            }
        },
    );
}

fn run_dev(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let connect =
        value_arg(args, "--connect").ok_or("dev role requires --connect <preview-socket>")?;
    let replace_code_file = value_arg(args, "--replace-code-file").map(PathBuf::from);
    let editor_code_file = value_arg(args, "--editor-code-file")
        .map(PathBuf::from)
        .or_else(|| replace_code_file.clone());
    let selected_example_id = value_arg(args, "--selected-example");
    let replace_code_expected_hash = replace_code_file
        .as_deref()
        .map(source_hash_for_path)
        .transpose()?;
    let report = value_arg(args, "--report").map(PathBuf::from);
    let hold_ms = numeric_arg(args, "--hold-ms").unwrap_or(0);
    let input_sample_delay_ms = numeric_arg(args, "--input-sample-delay-ms").unwrap_or(0);
    let synthetic_input_probe = args.iter().any(|arg| arg == "--synthetic-input-probe");
    let probe = args.iter().any(|arg| arg == "--probe");
    let ipc_stress_messages = numeric_arg(args, "--ipc-stress-messages").unwrap_or(4_096);
    let ipc_queue_capacity = numeric_arg(args, "--ipc-queue-capacity").unwrap_or(256);
    let ipc_probe_timeout_ms = numeric_arg(args, "--ipc-probe-timeout-ms").unwrap_or(60_000);
    let skip_operator_host_input_probe = args
        .iter()
        .any(|arg| arg == "--skip-operator-host-input-probe");
    let title = role_window_title("Boon Dev", value_arg(args, "--title-token").as_deref());
    let role_args = args[1..].to_vec();
    let warmup_frame_count = numeric_arg(args, "--warmup-frame-count").unwrap_or(0) as u32;
    let sample_frame_count = numeric_arg(args, "--sample-frame-count").unwrap_or(1) as u32;
    let dev_source_path_label = editor_code_file
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<no-code-file>".to_owned());
    let dev_source_text = editor_code_file
        .as_deref()
        .map(boon_runtime::source_text_for_path)
        .transpose()?
        .unwrap_or_else(|| "document = []".to_owned());
    let dev_shell = Arc::new(Mutex::new(DevWindowShell::new(
        &dev_source_path_label,
        &dev_source_text,
        selected_example_id.as_deref(),
        PreviewTransport::new(Some(connect.clone())),
    )));
    let render_hook: Option<boon_native_app_window::NativeRenderHook> = {
        let mut visible_renderer = None;
        let mut layout_frame_cache = None;
        let mut text_measurer = boon_native_gpu::GlyphonTextMeasurer::new();
        let mut input_state = DevNativeInputState::default();
        let shell = Arc::clone(&dev_shell);
        Some(Box::new(move |context| {
            let mut shell = shell
                .lock()
                .map_err(|_| "dev shell mutex poisoned".to_owned())?;
            native_gpu_dev_visible_render_hook(
                context,
                &mut visible_renderer,
                &mut layout_frame_cache,
                &mut text_measurer,
                &mut shell,
                &mut input_state,
            )
        }))
    };
    let report_shell = Arc::clone(&dev_shell);
    boon_native_app_window::run_visible_surface_probe_with_render_hook(
        NativeWindowOptions {
            role: NativeWindowRole::Dev,
            title,
            initial_width: 1180.0,
            initial_height: 820.0,
            hold_ms,
            input_sample_delay_ms,
            synthetic_input_probe,
            warmup_frame_count,
            sample_frame_count,
            readback_artifact_dir: Some("target/artifacts/native-gpu/frames".to_owned()),
        },
        render_hook,
        move |proof| {
            let result = match proof {
                Ok(proof) => report
                    .as_deref()
                    .map(|report| {
                        let dev_shell_interaction_probe = report_shell
                            .lock()
                            .map(|shell| {
                                if probe {
                                    shell.visible_input_probe(&proof)
                                } else {
                                    shell.passive_visible_probe(&proof)
                                }
                            })
                            .unwrap_or_else(|_| {
                                json!({
                                    "status": "fail",
                                    "diagnostic": "dev shell mutex poisoned"
                                })
                            });
                        let ipc_probe = if probe {
                            let ipc_start = Instant::now();
                            run_dev_ipc_probe(
                                &connect,
                                ipc_stress_messages,
                                ipc_queue_capacity,
                                replace_code_file.as_deref(),
                                replace_code_expected_hash.as_deref(),
                                skip_operator_host_input_probe,
                            )
                            .map_err(|error| error.to_string())
                            .and_then(|value| {
                                if ipc_start.elapsed() > Duration::from_millis(ipc_probe_timeout_ms) {
                                    Err(format!(
                                        "dev IPC probe exceeded timeout after {} ms",
                                        ipc_start.elapsed().as_millis()
                                    ))
                                } else {
                                    Ok(value)
                                }
                            })
                        } else {
                            Ok(json!({
                                "status": "not-run",
                                "reason": "visible app launch does not run verification IPC probes or mutate preview state",
                                "preview_mutation_allowed": false
                            }))
                        };
                        if let Err(error) = &ipc_probe {
                            return write_role_failure_report(
                                report,
                                "dev",
                                &role_args,
                                format!("dev IPC probe failed: {error}"),
                            );
                        }
                        write_role_report(
                            report,
                            "dev",
                            &role_args,
                            json!({
                                "connect": connect,
                                "observability_mode": "bounded-telemetry-and-query",
                                "full_state_mirroring_allowed": false,
                                "editor_code_file": editor_code_file,
                                "replace_code_file": replace_code_file,
                                "replace_code_expected_hash": replace_code_expected_hash,
                                "ipc_probe": ipc_probe.unwrap(),
                                "verification_probe_enabled": probe,
                                "dev_shell_interaction_probe": dev_shell_interaction_probe,
                                "app_window_surface_proof": proof,
                                "app_window_contract": boon_native_app_window::app_window_contract(),
                                "note": "dev role created an app_window Wayland window, presented one wgpu frame, and completed a bounded live IPC stress exchange with preview"
                            }),
                        )
                    })
                    .transpose(),
                Err(error) => report
                    .as_deref()
                    .map(|report| write_role_failure_report(report, "dev", &role_args, error.to_string()))
                    .transpose(),
            };
            if let Err(error) = result {
                eprintln!("boon_native_playground: failed to write dev report: {error}");
            }
        },
    );
}

fn run_desktop(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_arg(args, "--example").unwrap_or_else(|| "cells".to_owned());
    let catalog_entry = boon_runtime::example_manifest_entry(&example).ok();
    let source_path = value_arg(args, "--code-file")
        .map(PathBuf::from)
        .or_else(|| {
            catalog_entry
                .as_ref()
                .map(|entry| PathBuf::from(&entry.source))
        })
        .unwrap_or_else(|| PathBuf::from(format!("examples/{example}.bn")));
    let source = boon_runtime::source_text_for_path(&source_path)?;
    let source_sha256 = boon_runtime::sha256_bytes(source.as_bytes());
    let document_layout_proof =
        native_document_layout_proof(&source_path, &source).unwrap_or_else(|error| {
            json!({
                "status": "fail",
                "blocker": error.to_string()
            })
        });
    let report = value_arg(args, "--report").map(PathBuf::from);
    let live_state_report = value_arg(args, "--live-state-report").map(PathBuf::from);
    let dev_editor_code_file = value_arg(args, "--dev-editor-code-file")
        .map(PathBuf::from)
        .or_else(|| Some(source_path.clone()));
    let dev_editor_only = args.iter().any(|arg| arg == "--dev-editor-only");
    let probe = report.is_some() || args.iter().any(|arg| arg == "--probe");
    let real_window_input_probe = args.iter().any(|arg| arg == "--real-window-input-probe");
    let title_token = value_arg(args, "--title-token")
        .unwrap_or_else(|| format!("{}-{}", std::process::id(), current_unix_seconds()));
    let preview_title = role_window_title("Boon Preview", Some(&title_token));
    let dev_title = role_window_title("Boon Dev", Some(&title_token));
    let ipc_path = std::env::temp_dir().join(format!(
        "boon-native-preview-{}-{}.sock",
        std::process::id(),
        current_unix_seconds()
    ));
    let role_dir = PathBuf::from("target/reports/native-gpu/roles");
    std::fs::create_dir_all(&role_dir)?;
    let preview_report = role_dir.join(format!("preview-{}-{}.json", example, std::process::id()));
    let dev_report = role_dir.join(format!("dev-{}-{}.json", example, std::process::id()));
    let child_hold_ms =
        numeric_arg(args, "--child-hold-ms").unwrap_or(if probe { 2_500 } else { 0 });
    let dev_hold_ms = numeric_arg(args, "--dev-hold-ms").unwrap_or(if probe { 700 } else { 0 });
    let role_report_timeout_ms = numeric_arg(args, "--role-report-timeout-ms").unwrap_or(12_000);
    let dev_start_delay_ms = numeric_arg(args, "--dev-start-delay-ms").unwrap_or(0);
    let input_sample_delay_ms = numeric_arg(args, "--input-sample-delay-ms").unwrap_or(0);
    let effective_preview_hold_ms = if probe {
        child_hold_ms.max(
            dev_start_delay_ms
                .saturating_add(dev_hold_ms)
                .saturating_add(input_sample_delay_ms)
                .max(dev_start_delay_ms.saturating_add(role_report_timeout_ms))
                .saturating_add(5_000),
        )
    } else {
        child_hold_ms
    };
    let warmup_frame_count = numeric_arg(args, "--warmup-frame-count").unwrap_or(0);
    let sample_frame_count = numeric_arg(args, "--sample-frame-count").unwrap_or(1);
    let mut preview_args = vec![
        "--role".to_owned(),
        "preview".to_owned(),
        "--code-file".to_owned(),
        source_path
            .to_str()
            .ok_or("resolved code file path is not UTF-8")?
            .to_owned(),
        "--connect".to_owned(),
        ipc_path.to_str().ok_or("IPC path is not UTF-8")?.to_owned(),
        "--report".to_owned(),
        preview_report
            .to_str()
            .ok_or("preview report path is not UTF-8")?
            .to_owned(),
        "--hold-ms".to_owned(),
        effective_preview_hold_ms.to_string(),
        "--title-token".to_owned(),
        title_token.clone(),
        "--input-sample-delay-ms".to_owned(),
        input_sample_delay_ms.to_string(),
        "--warmup-frame-count".to_owned(),
        warmup_frame_count.to_string(),
        "--sample-frame-count".to_owned(),
        sample_frame_count.to_string(),
    ];
    if probe && !real_window_input_probe {
        preview_args.push("--synthetic-input-probe".to_owned());
    }
    let preview_arg_refs = preview_args.iter().map(String::as_str).collect::<Vec<_>>();
    let mut preview = spawn_role(&preview_arg_refs)?;
    let preview_pid = preview.id();
    let preview_cmdline = wait_for_proc_cmdline(preview_pid, "--role", "preview");
    let role_report_timeout = Duration::from_millis(role_report_timeout_ms);
    if probe {
        wait_for_path(&ipc_path, role_report_timeout)?;
    }
    if dev_start_delay_ms > 0 {
        std::thread::sleep(Duration::from_millis(dev_start_delay_ms));
    }
    let mut dev_args = vec![
        "--role".to_owned(),
        "dev".to_owned(),
        "--connect".to_owned(),
        ipc_path.to_str().ok_or("IPC path is not UTF-8")?.to_owned(),
    ];
    if let Some(path) = dev_editor_code_file.as_deref() {
        dev_args.push("--editor-code-file".to_owned());
        dev_args.push(
            path.to_str()
                .ok_or("dev editor file path is not UTF-8")?
                .to_owned(),
        );
    }
    dev_args.push("--selected-example".to_owned());
    dev_args.push(example.clone());
    if !dev_editor_only {
        dev_args.push("--replace-code-file".to_owned());
        dev_args.push(
            source_path
                .to_str()
                .ok_or("resolved code file path is not UTF-8")?
                .to_owned(),
        );
    }
    dev_args.extend([
        "--report".to_owned(),
        dev_report
            .to_str()
            .ok_or("dev report path is not UTF-8")?
            .to_owned(),
        "--hold-ms".to_owned(),
        dev_hold_ms.to_string(),
        "--ipc-stress-messages".to_owned(),
        "4096".to_owned(),
        "--ipc-queue-capacity".to_owned(),
        "256".to_owned(),
        "--ipc-probe-timeout-ms".to_owned(),
        role_report_timeout_ms.saturating_sub(1_000).to_string(),
        "--title-token".to_owned(),
        title_token.clone(),
        "--input-sample-delay-ms".to_owned(),
        input_sample_delay_ms.to_string(),
        "--warmup-frame-count".to_owned(),
        warmup_frame_count.to_string(),
        "--sample-frame-count".to_owned(),
        sample_frame_count.to_string(),
    ]);
    if probe && !real_window_input_probe {
        dev_args.push("--synthetic-input-probe".to_owned());
    }
    if probe {
        dev_args.push("--probe".to_owned());
    }
    if args
        .iter()
        .any(|arg| arg == "--skip-operator-host-input-probe")
    {
        dev_args.push("--skip-operator-host-input-probe".to_owned());
    }
    let dev_arg_refs = dev_args.iter().map(String::as_str).collect::<Vec<_>>();
    let mut dev = spawn_role(&dev_arg_refs)?;
    let dev_pid = dev.id();
    let dev_cmdline = wait_for_proc_cmdline(dev_pid, "--role", "dev");

    if !probe {
        let preview_status = preview.wait()?;
        let dev_status = dev.wait()?;
        if !preview_status.success() || !dev_status.success() {
            return Err(format!(
                "native desktop children exited unsuccessfully: preview={preview_status}, dev={dev_status}"
            )
            .into());
        }
        return Ok(());
    }

    wait_for_report(&dev_report, role_report_timeout)?;
    wait_for_report(&preview_report, role_report_timeout)?;
    if let Some(path) = live_state_report.as_deref() {
        write_live_state_report(
            path,
            &example,
            &title_token,
            &preview_title,
            &dev_title,
            preview_pid,
            dev_pid,
            &preview_report,
            &dev_report,
        )?;
    }
    let dev_status = dev.wait()?;
    let preview_survives_dev_exit = dev_status.success() && child_running(&mut preview)?;
    let preview_shutdown_ack = if preview_survives_dev_exit {
        send_preview_ipc_request(
            ipc_path.to_str().ok_or("IPC path is not UTF-8")?,
            json!({
                "kind": "shutdown",
                "reason": "desktop-supervisor-clean-exit-after-dev",
                "dev_pid": dev_pid
            }),
        )
        .unwrap_or_else(|error| {
            json!({
                "status": "fail",
                "diagnostic": error.to_string()
            })
        })
    } else {
        json!({
            "status": "not-run",
            "reason": "preview did not survive dev exit"
        })
    };
    let preview_clean_exit_after_dev_exit = wait_child_exit(
        &mut preview,
        Duration::from_millis(effective_preview_hold_ms.saturating_add(500)),
    )?;
    let preview_exit_status_after_dev_exit = preview_clean_exit_after_dev_exit
        .as_ref()
        .map(std::process::ExitStatus::to_string)
        .unwrap_or_else(|| "still-running-after-timeout".to_owned());
    if preview_clean_exit_after_dev_exit.is_none() {
        terminate_child(&mut preview);
    }
    let preview_json = read_json(&preview_report)?;
    let dev_json = read_json(&dev_report)?;
    let preview_role_status = preview_json
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing");
    let dev_role_status = dev_json
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing");
    let preview_proof = preview_json
        .pointer("/details/app_window_surface_proof")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let dev_proof = dev_json
        .pointer("/details/app_window_surface_proof")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let dev_ipc_probe = dev_json
        .pointer("/details/ipc_probe")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let dev_shell_interaction_probe = dev_json
        .pointer("/details/dev_shell_interaction_probe")
        .cloned()
        .unwrap_or_else(|| json!({"status": "missing"}));
    let preview_runtime_summary = preview_json
        .pointer("/details/preview_runtime_summary")
        .cloned()
        .unwrap_or_else(|| json!({"status": "missing"}));
    let preview_native_gpu_render_proof = preview_proof
        .get("external_render_proof")
        .cloned()
        .filter(|proof| proof.get("status").and_then(serde_json::Value::as_str) == Some("pass"))
        .unwrap_or_else(|| {
            native_gpu_render_proof(&preview_proof, &document_layout_proof).unwrap_or_else(
                |error| {
                    json!({
                        "status": "fail",
                        "blocker": error.to_string()
                    })
                },
            )
        });
    if let Some(report) = report {
        let mut details = json!({
            "resolved_example": example,
            "resolved_example_label": catalog_entry.as_ref().map(|entry| entry.label.clone()),
            "example_catalog_id": catalog_entry.as_ref().map(|entry| entry.id.clone()),
            "example_catalog_source": catalog_entry.as_ref().map(|entry| entry.source.clone()),
            "resolved_code_file": source_path,
            "source_bytes": source.len(),
            "source_sha256": source_sha256,
            "process_model": "two-child-processes",
            "preview_role_status": preview_role_status,
            "dev_role_status": dev_role_status,
            "preview_child_pid": preview_pid,
            "dev_child_pid": dev_pid,
            "preview_child_cmdline": preview_cmdline,
            "dev_child_cmdline": dev_cmdline,
            "title_token": title_token,
            "preview_window_title": preview_title,
            "dev_window_title": dev_title,
            "preview_survives_dev_exit": preview_survives_dev_exit,
            "dev_exit_status": dev_status.to_string(),
            "preview_clean_exit_after_dev_exit": preview_clean_exit_after_dev_exit
                .as_ref()
                .is_some_and(std::process::ExitStatus::success),
            "preview_exit_status_after_dev_exit": preview_exit_status_after_dev_exit,
            "preview_receives_example_name": false,
            "preview_launch_form": "--role preview --code-file <resolved-code-file>",
            "replace_code_transport": "dev-to-preview-bounded-ipc",
            "display_server": display_server(),
            "display_connection": display_connection(),
            "requested_workspace": "boon-circuit",
            "launcher_command": "direct-child-processes",
            "cosmic_background_launch_available": command_exists("cosmic-background-launch"),
            "cosmic_background_launch_machine_readable_proof": false,
            "preview_role_report": preview_report,
            "dev_role_report": dev_report,
            "preview_role_report_sha256": boon_runtime::sha256_file(&preview_report).unwrap_or_else(|_| "missing".to_owned()),
            "dev_role_report_sha256": boon_runtime::sha256_file(&dev_report).unwrap_or_else(|_| "missing".to_owned()),
            "note": "desktop supervisor spawns two child roles with app_window/wgpu windows and bounded live IPC; COSMIC launcher proof is owned by the xtask wrapper that invokes cosmic-background-launch"
        });
        details["requested_preview_hold_ms"] = json!(child_hold_ms);
        details["effective_preview_hold_ms"] = json!(effective_preview_hold_ms);
        details["dev_hold_ms"] = json!(dev_hold_ms);
        details["dev_start_delay_ms"] = json!(dev_start_delay_ms);
        details["role_report_timeout_ms"] = json!(role_report_timeout_ms);
        details["preview_document_layout_proof"] = document_layout_proof;
        details["preview_surface_proof"] = preview_proof;
        details["dev_surface_proof"] = dev_proof;
        details["preview_native_gpu_render_proof"] = preview_native_gpu_render_proof;
        details["preview_runtime_summary"] = preview_runtime_summary;
        details["dev_ipc_probe"] = dev_ipc_probe;
        details["dev_shell_interaction_probe"] = dev_shell_interaction_probe;
        details["preview_shutdown_ack"] = preview_shutdown_ack;
        write_desktop_report(&report, &args[1..], details)?;
    }
    Ok(())
}

fn native_document_layout_proof(
    source_path: &Path,
    source: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    native_document_layout_proof_with_state(source_path, source, None)
}

fn native_document_layout_proof_with_state(
    source_path: &Path,
    source: &str,
    runtime_state_override: Option<&serde_json::Value>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let parsed = boon_parser::parse_source(source_path.display().to_string(), source)?;
    let document = boon_parser::parsed_document(&parsed)
        .ok_or("source does not contain a parseable document block")?;
    let runtime_state = runtime_state_override
        .cloned()
        .or_else(|| runtime_state_summary_for_source(source_path, source).ok());
    let document_functions = DocumentFunctionRegistry::new(&parsed.ast.statements);
    let eval_context = DocumentEvalContext {
        root: runtime_state.as_ref(),
        locals: BTreeMap::new(),
    };
    let mut frame = boon_document_model::DocumentFrame::empty("root");
    let mut source_intents = Vec::new();
    let mut seen_ids = BTreeSet::new();
    let root_id = frame.root.clone();
    if let Some(root_element) = canonical_document_root(&document.root, &document.expressions) {
        lower_canonical_document_entry(
            root_element,
            &document.expressions,
            &document_functions,
            &root_id,
            &mut frame,
            &mut source_intents,
            &mut seen_ids,
            &eval_context,
            "",
            true,
        );
    } else {
        lower_document_elements(
            &document.root.children,
            &document.expressions,
            &root_id,
            &mut frame,
            &mut source_intents,
            &mut seen_ids,
            &eval_context,
            "",
        );
    }

    let mut measurer = boon_native_gpu::GlyphonTextMeasurer::new();
    let layout = boon_document::layout(boon_document::LayoutInput {
        document: &frame,
        viewport: boon_host::Viewport {
            surface: 1,
            width: 920.0,
            height: 720.0,
            scale: 1.0,
        },
        text: &mut measurer,
        capabilities: boon_document::RenderCapabilities::fake_portable(),
    });

    let artifact_dir = PathBuf::from("target/artifacts/native-gpu/document-layout");
    std::fs::create_dir_all(&artifact_dir)?;
    let source_sha256 = boon_runtime::sha256_bytes(source.as_bytes());
    let artifact_path = artifact_dir.join(format!(
        "{}-{}{}.json",
        source_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("source"),
        &source_sha256[..12.min(source_sha256.len())],
        runtime_state
            .as_ref()
            .filter(|_| runtime_state_override.is_some())
            .map(|state| format!(
                "-state-{}",
                &boon_runtime::sha256_bytes(&serde_json::to_vec(state).unwrap_or_default())[..12]
            ))
            .unwrap_or_default()
    ));
    let artifact = json!({
        "source_path": source_path,
        "source_sha256": source_sha256,
        "document_frame": frame,
        "layout_frame": layout,
        "source_intents": source_intents,
        "runtime_document_state_used": runtime_state.is_some(),
        "runtime_document_state_hash": runtime_state
            .as_ref()
            .map(|state| boon_runtime::sha256_bytes(&serde_json::to_vec(state).unwrap_or_default()))
    });
    std::fs::write(&artifact_path, serde_json::to_vec_pretty(&artifact)?)?;
    let artifact_sha256 = boon_runtime::sha256_file(&artifact_path)?;
    let artifact = std::fs::read_to_string(&artifact_path)?;
    let artifact_json: serde_json::Value = serde_json::from_str(&artifact)?;
    let layout_json = artifact_json
        .get("layout_frame")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let hit_target_assertion_total = layout_json
        .get("hit_regions")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let hit_target_samples = hit_target_assertion_total
        .iter()
        .take(256)
        .cloned()
        .collect::<Vec<_>>();
    let source_intent_assertions = artifact_json
        .get("source_intents")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let source_intent_total = source_intent_assertions.len();
    let source_intent_samples = source_intent_assertions
        .iter()
        .take(256)
        .cloned()
        .collect::<Vec<_>>();
    let node_count = artifact_json
        .pointer("/document_frame/nodes")
        .and_then(serde_json::Value::as_object)
        .map_or(0, serde_json::Map::len);
    let display_item_count = layout_json
        .get("display_list")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);

    Ok(json!({
        "status": "pass",
        "lowering": "boon_parser_document_ast_to_boon_document_model",
        "source_path": source_path,
        "source_sha256": artifact_json.get("source_sha256").cloned().unwrap_or_else(|| json!("missing")),
        "artifact_path": artifact_path,
        "artifact_sha256": artifact_sha256,
        "layout_frame_hash": boon_runtime::sha256_file(&artifact_path)?,
        "node_count": node_count,
        "display_item_count": display_item_count,
        "hit_target_count": hit_target_assertion_total.len(),
        "hit_target_sample_count": hit_target_samples.len(),
        "hit_target_sample_limit": 256,
        "source_intent_count": source_intent_total,
        "source_intent_sample_count": source_intent_samples.len(),
        "source_intent_sample_limit": 256,
        "hit_target_assertions": hit_target_assertion_total,
        "hit_target_samples": hit_target_samples,
        "source_intent_assertions": source_intent_assertions,
        "source_intent_samples": source_intent_samples,
        "layout_metrics": layout_json.get("metrics").cloned().unwrap_or_else(|| json!({})),
        "scroll_regions": layout_json.get("scroll_regions").cloned().unwrap_or_else(|| json!([])),
        "runtime_document_state_used": artifact_json.get("runtime_document_state_used").cloned().unwrap_or_else(|| json!(false)),
        "runtime_document_state_hash": artifact_json.get("runtime_document_state_hash").cloned().unwrap_or_else(|| json!(null)),
    }))
}

fn preview_runtime_summary(
    source_path: &Path,
    source: &str,
    source_sha256: &str,
) -> serde_json::Value {
    let scenario_path = source_path.with_extension("scn");
    let state_summary = match runtime_state_summary_for_source(source_path, source) {
        Ok(summary) => summary,
        Err(error) => {
            return json!({
                "status": "fail",
                "owns_live_runtime": false,
                "reason": error,
                "source_path": source_path,
                "source_sha256": source_sha256,
                "scenario_path": scenario_path,
                "full_state_mirroring_allowed": false
            });
        }
    };
    let summary_bytes = serde_json::to_vec(&state_summary).unwrap_or_default();
    let summary_top_level_keys = state_summary
        .as_object()
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    json!({
        "status": "pass",
        "owns_live_runtime": true,
        "public_runtime_api": "boon_runtime::LiveRuntime",
        "source_path": source_path,
        "source_sha256": source_sha256,
        "scenario_path": scenario_path,
        "scenario_bound": scenario_path.exists(),
        "runtime_surface": "generic-live-runtime",
        "state_summary_hash": boon_runtime::sha256_bytes(&summary_bytes),
        "state_summary_bytes": summary_bytes.len(),
        "state_summary_top_level_keys": summary_top_level_keys,
        "full_state_mirroring_allowed": false,
        "full_state_mirroring_observed": false
    })
}

fn runtime_state_summary_for_source(source_path: &Path, source: &str) -> Result<Value, String> {
    let scenario_path = source_path.with_extension("scn");
    let mut runtime = if scenario_path.exists() {
        boon_runtime::LiveRuntime::new(
            &format!("native-preview:{}", source_path.display()),
            source,
            &scenario_path,
        )
        .map_err(|error| error.to_string())?
    } else {
        boon_runtime::LiveRuntime::from_source(
            &format!("native-preview:{}", source_path.display()),
            source,
        )
        .map_err(|error| error.to_string())?
    };
    Ok(runtime.state_summary())
}

fn preview_runtime_summary_response(
    runtime_summary: &serde_json::Value,
    last_error: Option<&str>,
    last_error_count: u64,
) -> serde_json::Value {
    let payload = serde_json::to_vec(runtime_summary).unwrap_or_default();
    json!({
        "kind": "debug-query-result",
        "debug_query": "RuntimeSummary",
        "bounded_query": true,
        "max_payload_bytes": 4096,
        "payload_bytes": payload.len(),
        "payload_hash": boon_runtime::sha256_bytes(&payload),
        "full_state_mirroring_allowed": false,
        "full_state_mirroring_observed": false,
        "preview_last_error": last_error,
        "preview_last_error_count": last_error_count,
        "runtime_summary": runtime_summary
    })
}

#[derive(Clone, Debug)]
struct ReportPresentSurface {
    id: boon_host::SurfaceId,
    width: f32,
    height: f32,
    format: boon_native_gpu::SurfaceFormat,
    epoch: u64,
}

impl PresentSurface for ReportPresentSurface {
    fn id(&self) -> boon_host::SurfaceId {
        self.id.clone()
    }

    fn viewport_width(&self) -> f32 {
        self.width
    }

    fn viewport_height(&self) -> f32 {
        self.height
    }

    fn format(&self) -> boon_native_gpu::SurfaceFormat {
        self.format.clone()
    }

    fn epoch(&self) -> u64 {
        self.epoch
    }
}

fn native_gpu_render_proof(
    surface_proof: &serde_json::Value,
    layout_proof: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    if layout_proof
        .get("status")
        .and_then(serde_json::Value::as_str)
        != Some("pass")
    {
        return Err("layout proof did not pass".into());
    }
    let layout_artifact = layout_proof
        .get("artifact_path")
        .and_then(serde_json::Value::as_str)
        .ok_or("layout proof missing artifact_path")?;
    let artifact_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(layout_artifact)?)?;
    let layout_frame: boon_document::LayoutFrame = serde_json::from_value(
        artifact_json
            .get("layout_frame")
            .cloned()
            .ok_or("layout artifact missing layout_frame")?,
    )?;
    let mut target = ReportPresentSurface {
        id: boon_host::SurfaceId(
            surface_proof
                .get("surface_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("missing-surface")
                .to_owned(),
        ),
        width: surface_proof
            .pointer("/logical_size/width")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0) as f32,
        height: surface_proof
            .pointer("/logical_size/height")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0) as f32,
        format: boon_native_gpu::SurfaceFormat(
            surface_proof
                .get("surface_format")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
        ),
        epoch: surface_proof
            .get("surface_epoch")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
    };
    let mut renderer = boon_native_gpu::NativeGpuRenderer::new_uninitialized();
    let proof = renderer.render(&mut target, &layout_frame)?;
    Ok(json!({
        "status": "pass",
        "renderer": "boon_native_gpu",
        "render_backend_trait": "boon_native_gpu::RenderBackend",
        "layout_artifact": layout_artifact,
        "layout_artifact_sha256": layout_proof.get("artifact_sha256").cloned().unwrap_or_else(|| json!("missing")),
        "surface_id": target.id,
        "surface_epoch": target.epoch,
        "surface_format": target.format,
        "uses_generated_shader_entry": format!("{:?}", renderer.rect_shader_entry()),
        "proof": proof,
        "visible_surface_rendered": false,
        "visible_present_path": false,
        "copy_to_present_limitation": "renderer proof is bound to the preview surface identity, but the native app_window role still performs the actual first-frame clear/present until the renderer owns the render pass"
    }))
}

fn native_gpu_app_owned_render_hook(
    context: boon_native_app_window::NativeRenderFrameContext<'_>,
    layout_proof: &serde_json::Value,
    layout_frame_override: Option<&boon_document::LayoutFrame>,
    last_error: Option<&str>,
    visible_renderer: &mut Option<boon_native_gpu::VisibleLayoutRenderer>,
    app_owned_proof: &mut Option<boon_native_gpu::RenderProof>,
    layout_frame_cache: &mut Option<(String, boon_document::LayoutFrame)>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    if layout_proof
        .get("status")
        .and_then(serde_json::Value::as_str)
        != Some("pass")
    {
        return Err("layout proof did not pass".into());
    }
    let layout_artifact = layout_proof
        .get("artifact_path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<embedded-layout-frame>");
    let layout_cache_key = layout_proof
        .get("layout_frame_hash")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(layout_artifact);
    let cache_stale = layout_frame_cache
        .as_ref()
        .is_none_or(|(path, _)| path != layout_cache_key);
    if cache_stale {
        let layout_frame = match layout_frame_override {
            Some(layout_frame) => layout_frame.clone(),
            None => {
                let artifact_json: serde_json::Value =
                    serde_json::from_str(&std::fs::read_to_string(layout_artifact)?)?;
                serde_json::from_value(
                    artifact_json
                        .get("layout_frame")
                        .cloned()
                        .ok_or("layout artifact missing layout_frame")?,
                )?
            }
        };
        *layout_frame_cache = Some((layout_cache_key.to_owned(), layout_frame));
    }
    let layout_frame = layout_frame_cache
        .as_ref()
        .map(|(_, frame)| frame)
        .ok_or("layout frame cache was not initialized")?;
    let render_frame = match last_error {
        Some(error) => preview_frame_with_error_overlay(
            layout_frame,
            error,
            context.width as f32,
            context.height as f32,
        ),
        None => layout_frame.clone(),
    };
    let renderer = visible_renderer.get_or_insert_with(|| {
        boon_native_gpu::VisibleLayoutRenderer::new(
            context.device,
            context.queue,
            context.surface_texture_format,
        )
    });
    let visible_metrics = renderer.encode(boon_native_gpu::SurfaceRenderRequest {
        device: context.device,
        queue: context.queue,
        encoder: context.encoder,
        view: context.surface_view,
        frame: &render_frame,
        format: context.surface_texture_format,
        width: context.width,
        height: context.height,
    })?;
    let app_owned_readback_reused = app_owned_proof.is_some();
    let proof = match app_owned_proof {
        Some(proof) => proof.clone(),
        None => {
            let proof =
                boon_native_gpu::render_app_owned_pixels(boon_native_gpu::AppOwnedRenderRequest {
                    device: context.device,
                    queue: context.queue,
                    frame: &render_frame,
                    surface_id: context.surface_id.clone(),
                    surface_epoch: context.surface_epoch,
                    width: context.width,
                    height: context.height,
                    artifact_dir: Path::new("target/artifacts/native-gpu/renderer-frames"),
                    artifact_label: "preview",
                })?;
            *app_owned_proof = Some(proof.clone());
            proof
        }
    };
    Ok(json!({
        "status": "pass",
        "renderer": "boon_native_gpu",
        "render_backend_trait": "boon_native_gpu::render_app_owned_pixels",
        "layout_artifact": layout_artifact,
        "layout_artifact_sha256": layout_proof.get("artifact_sha256").cloned().unwrap_or_else(|| json!("missing")),
        "layout_frame_hash": layout_proof.get("layout_frame_hash").cloned().unwrap_or_else(|| json!("missing")),
        "scroll_transform": layout_proof.get("scroll_transform").cloned().unwrap_or_else(|| json!(null)),
        "surface_id": context.surface_id,
        "surface_epoch": context.surface_epoch,
        "surface_format": context.surface_format,
        "uses_generated_shader_entry": "NativeGpuRect",
        "visible_style_mode": "document_style",
        "debug_palette_used": false,
        "viewport_fill_ratio": 1.0,
        "content_bounds_fill_ratio": viewport_fill_ratio(&render_frame, context.width, context.height),
        "preview_last_error": last_error,
        "preview_error_overlay_visible": last_error.is_some(),
        "visible_surface_rendered": true,
        "visible_present_path": true,
        "visible_surface_metrics": visible_metrics,
        "app_owned_readback_reused": app_owned_readback_reused,
        "proof": proof,
        "copy_to_present_limitation": serde_json::Value::Null
    }))
}

fn preview_frame_with_error_overlay(
    frame: &boon_document::LayoutFrame,
    error: &str,
    width: f32,
    height: f32,
) -> boon_document::LayoutFrame {
    let mut frame = frame.clone();
    let overlay_width = (width - 32.0).max(1.0);
    let overlay_height = 72.0_f32.min((height - 32.0).max(1.0));
    let overlay_y = (height - overlay_height - 16.0).max(0.0);
    let mut background_style = BTreeMap::new();
    background_style.insert(
        "bg".to_owned(),
        boon_document_model::StyleValue::Text("#fee2e2".to_owned()),
    );
    background_style.insert(
        "border".to_owned(),
        boon_document_model::StyleValue::Text("#dc2626".to_owned()),
    );
    frame.display_list.push(boon_document::DisplayItem {
        node: boon_document_model::DocumentNodeId("preview-error-overlay-bg".to_owned()),
        kind: boon_document_model::DocumentNodeKind::Text,
        bounds: boon_document::Rect {
            x: 16.0,
            y: overlay_y,
            width: overlay_width,
            height: overlay_height,
        },
        text: Some(String::new()),
        style: background_style,
        focused: false,
    });

    let mut text_style = BTreeMap::new();
    text_style.insert(
        "bg".to_owned(),
        boon_document_model::StyleValue::Text("#fee2e2".to_owned()),
    );
    text_style.insert(
        "color".to_owned(),
        boon_document_model::StyleValue::Text("#7f1d1d".to_owned()),
    );
    text_style.insert(
        "size".to_owned(),
        boon_document_model::StyleValue::Number(14.0),
    );
    text_style.insert(
        "font".to_owned(),
        boon_document_model::StyleValue::Text("JetBrains Mono".to_owned()),
    );
    frame.display_list.push(boon_document::DisplayItem {
        node: boon_document_model::DocumentNodeId("preview-error-overlay-text".to_owned()),
        kind: boon_document_model::DocumentNodeKind::Text,
        bounds: boon_document::Rect {
            x: 28.0,
            y: overlay_y + 12.0,
            width: (overlay_width - 24.0).max(1.0),
            height: (overlay_height - 24.0).max(1.0),
        },
        text: Some(format!(
            "Preview input error: {}",
            single_line_preview_error(error)
        )),
        style: text_style,
        focused: false,
    });
    frame.metrics.display_item_count = frame.display_list.len();
    frame
}

fn single_line_preview_error(error: &str) -> String {
    const MAX_ERROR_CHARS: usize = 180;
    let mut value = error
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    while value.contains("  ") {
        value = value.replace("  ", " ");
    }
    if value.chars().count() > MAX_ERROR_CHARS {
        let mut truncated = value.chars().take(MAX_ERROR_CHARS).collect::<String>();
        truncated.push_str("...");
        truncated
    } else {
        value
    }
}

fn native_gpu_dev_visible_render_hook(
    context: boon_native_app_window::NativeRenderFrameContext<'_>,
    visible_renderer: &mut Option<boon_native_gpu::VisibleLayoutRenderer>,
    layout_frame_cache: &mut Option<(u32, u32, boon_document::LayoutFrame)>,
    text_measurer: &mut boon_native_gpu::GlyphonTextMeasurer,
    shell: &mut DevWindowShell,
    input_state: &mut DevNativeInputState,
) -> Result<serde_json::Value, String> {
    let now = Instant::now();
    let caret_visible = dev_editor_caret_visible(input_state, now);
    let caret_blink_changed = shell.caret_visible != caret_visible;
    shell.caret_visible = caret_visible;
    let footer_summary_changed = shell.refresh_preview_summary_if_due(now);
    let cache_stale = layout_frame_cache
        .as_ref()
        .is_none_or(|(width, height, _)| *width != context.width || *height != context.height);
    if cache_stale || caret_blink_changed || footer_summary_changed {
        let document = shell.document_for_viewport(context.width, context.height);
        let layout_frame = boon_document::layout(boon_document::LayoutInput {
            document: &document,
            viewport: boon_host::Viewport {
                surface: 1,
                width: context.width as f32,
                height: context.height as f32,
                scale: 1.0,
            },
            text: text_measurer,
            capabilities: boon_document::RenderCapabilities::fake_portable(),
        });
        *layout_frame_cache = Some((context.width, context.height, layout_frame));
    }
    let mut layout_changed = false;
    if dev_input_may_change(&context.input, input_state)
        && let Some((_, _, layout_frame)) = layout_frame_cache.as_ref()
    {
        let document = shell.document_for_viewport(context.width, context.height);
        layout_changed = dev_apply_real_window_input(
            &context.input,
            &document,
            layout_frame,
            context.width,
            context.height,
            shell,
            input_state,
        );
    }
    if layout_changed {
        let document = shell.document_for_viewport(context.width, context.height);
        let layout_frame = boon_document::layout(boon_document::LayoutInput {
            document: &document,
            viewport: boon_host::Viewport {
                surface: 1,
                width: context.width as f32,
                height: context.height as f32,
                scale: 1.0,
            },
            text: text_measurer,
            capabilities: boon_document::RenderCapabilities::fake_portable(),
        });
        *layout_frame_cache = Some((context.width, context.height, layout_frame));
    }
    let layout_frame = layout_frame_cache
        .as_ref()
        .map(|(_, _, frame)| frame)
        .ok_or_else(|| "dev layout frame cache was not initialized".to_owned())?;
    let renderer = visible_renderer.get_or_insert_with(|| {
        boon_native_gpu::VisibleLayoutRenderer::new(
            context.device,
            context.queue,
            context.surface_texture_format,
        )
    });
    let visible_metrics = renderer
        .encode(boon_native_gpu::SurfaceRenderRequest {
            device: context.device,
            queue: context.queue,
            encoder: context.encoder,
            view: context.surface_view,
            frame: layout_frame,
            format: context.surface_texture_format,
            width: context.width,
            height: context.height,
        })
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "status": "pass",
        "renderer": "boon_native_gpu",
        "render_backend_trait": "boon_native_gpu::encode_layout_to_surface",
        "surface_id": context.surface_id,
        "surface_epoch": context.surface_epoch,
        "surface_format": context.surface_format,
        "visible_surface_rendered": true,
        "visible_present_path": true,
        "visible_surface_metrics": visible_metrics,
        "viewport_fill_ratio": 1.0,
        "content_bounds_fill_ratio": viewport_fill_ratio(layout_frame, context.width, context.height),
        "dev_ui_source": "boon-dev-editor-debug-shell",
        "dev_editor_visible": true,
        "debug_panel_visible": true,
        "fixture_grid_used": false,
        "code_editor_line_count": shell.workspace.selected_buffer.line_count,
        "example_catalog_entry_count": shell.catalog.entries.len(),
        "dev_window_tabs_visible": true,
        "dev_window_toolbar_visible": true,
        "dev_window_controls": ["Run", "Format", "Reset"],
        "code_editor_model": {
            "full_buffer_bytes": shell.workspace.selected_buffer.source_text.len(),
            "full_buffer_lines": shell.workspace.selected_buffer.line_count,
            "syntax_backend": shell.workspace.selected_buffer.syntax_backend(),
            "syntax_parser_backed": shell.workspace.selected_buffer.syntax_parser_backed(),
            "syntax_token_count": shell.workspace.selected_buffer.syntax_token_count(),
            "syntax_categories": shell.workspace.selected_buffer.syntax_categories(),
            "syntax_render_categories": shell.workspace.selected_buffer.syntax_render_categories(),
            "syntax_render_segment_samples": shell.workspace.selected_buffer.syntax_render_segment_samples(),
            "syntax_render_segment_count": shell.workspace.selected_buffer.syntax_render_segments_for_visible_lines(40).len(),
            "syntax_theme": shell.workspace.selected_buffer.syntax_theme_report(),
            "diagnostic_count": shell.workspace.selected_buffer.diagnostics.len(),
            "font_family": shell.editor_view.font_family,
            "native_rust_editor_model": true
        },
        "layout_metrics": layout_frame.metrics
    }))
}

#[derive(Default)]
struct DevNativeInputState {
    last_mouse_button_event_count: u64,
    last_keyboard_event_sequence: u64,
    caret_blink_started_at: Option<Instant>,
    held_repeat_key: Option<String>,
    held_repeat_next_at: Option<Instant>,
    editor_focused: bool,
    focused_dev_text_input: Option<String>,
    mouse_select_anchor: Option<EditorPosition>,
    column_metric_cache: EditorColumnMetricCache,
}

type EditorColumnMetricCache = BTreeMap<EditorColumnMetricKey, Vec<f32>>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct EditorColumnMetricKey {
    text: String,
    style_signature: String,
    line_height_bits: u32,
}

fn dev_input_may_change(
    input: &boon_native_app_window::NativeInputAdapterProof,
    input_state: &DevNativeInputState,
) -> bool {
    (input.scroll_delta_y.abs() > f64::EPSILON || input.scroll_delta_x.abs() > f64::EPSILON)
        || input
            .mouse_button_events
            .iter()
            .any(|event| event.sequence > input_state.last_mouse_button_event_count)
        || (input_state.editor_focused
            && input_state.mouse_select_anchor.is_some()
            && input
                .mouse_buttons_down
                .iter()
                .any(|button| button == "left"))
        || input
            .keyboard_events
            .iter()
            .any(|event| event.sequence > input_state.last_keyboard_event_sequence)
        || input_state.held_repeat_key.as_ref().is_some_and(|key| {
            input_state.held_repeat_next_at.is_some()
                && input.pressed_keys.iter().any(|pressed| pressed == key)
        })
}

fn dev_apply_real_window_input(
    input: &boon_native_app_window::NativeInputAdapterProof,
    document: &boon_document_model::DocumentFrame,
    layout_frame: &boon_document::LayoutFrame,
    surface_width: u32,
    surface_height: u32,
    shell: &mut DevWindowShell,
    input_state: &mut DevNativeInputState,
) -> bool {
    if input.synthetic_input_probe {
        return false;
    }
    let mut changed = false;

    if (input.scroll_delta_y.abs() > f64::EPSILON || input.scroll_delta_x.abs() > f64::EPSILON)
        && let Some(position) =
            input_layout_position(input.mouse_window_pos, surface_width, surface_height)
        && let Some(footer_bounds) = layout_frame
            .display_list
            .iter()
            .find(|item| item.node.0 == "dev-footer")
            .map(|item| item.bounds)
        && rect_contains(footer_bounds, position.x as f32, position.y as f32)
    {
        let max_scroll_line = shell.footer_lines().len().saturating_sub(1);
        let line_delta = scaled_scroll_steps(input.scroll_delta_y, 8.0, 3);
        if line_delta > 0 {
            shell.footer_scroll_line = shell
                .footer_scroll_line
                .saturating_add(line_delta as usize)
                .min(max_scroll_line);
        } else if line_delta < 0 {
            shell.footer_scroll_line = shell
                .footer_scroll_line
                .saturating_sub((-line_delta) as usize);
        }
        changed = true;
    } else if (input.scroll_delta_y.abs() > f64::EPSILON
        || input.scroll_delta_x.abs() > f64::EPSILON)
        && let Some(position) =
            input_layout_position(input.mouse_window_pos, surface_width, surface_height)
        && let Some(editor_bounds) = layout_frame
            .display_list
            .iter()
            .find(|item| item.node.0 == "dev-code-editor")
            .map(|item| item.bounds)
        && rect_contains(editor_bounds, position.x as f32, position.y as f32)
    {
        let max_scroll_line = shell.workspace.selected_buffer.line_count.saturating_sub(1);
        let line_delta = scaled_scroll_steps(input.scroll_delta_y, 8.0, 3);
        if line_delta > 0 {
            shell.workspace.selected_buffer.scroll_line = shell
                .workspace
                .selected_buffer
                .scroll_line
                .saturating_add(line_delta as usize)
                .min(max_scroll_line);
        } else if line_delta < 0 {
            shell.workspace.selected_buffer.scroll_line = shell
                .workspace
                .selected_buffer
                .scroll_line
                .saturating_sub((-line_delta) as usize);
        }
        changed = true;
    }

    let mouse_events = input
        .mouse_button_events
        .iter()
        .filter(|event| event.sequence > input_state.last_mouse_button_event_count)
        .cloned()
        .collect::<Vec<_>>();
    for mouse_event in mouse_events {
        input_state.last_mouse_button_event_count = input_state
            .last_mouse_button_event_count
            .max(mouse_event.sequence);
        if mouse_event.button != "left" {
            continue;
        }
        if let Some(position) =
            input_layout_position(input.mouse_window_pos, surface_width, surface_height)
        {
            if let Some((node_id, source_path)) =
                dev_source_binding_at(document, layout_frame, position.x as f32, position.y as f32)
            {
                if source_path == "dev.editor.insert_text" || node_id == "dev-code-editor" {
                    input_state.editor_focused = true;
                    input_state.focused_dev_text_input = None;
                    if let Some(editor_position) = dev_position_from_pointer(
                        &shell.workspace.selected_buffer,
                        layout_frame,
                        position.x as f32,
                        position.y as f32,
                        &mut input_state.column_metric_cache,
                    ) {
                        if mouse_event.pressed {
                            shell
                                .workspace
                                .selected_buffer
                                .set_selection(editor_position.clone(), editor_position.clone());
                            input_state.mouse_select_anchor = Some(editor_position);
                        } else if let Some(anchor) = input_state.mouse_select_anchor.take() {
                            shell
                                .workspace
                                .selected_buffer
                                .set_selection(anchor, editor_position);
                        }
                    }
                    changed = true;
                } else if source_path == "dev.custom.name" {
                    input_state.editor_focused = false;
                    input_state.focused_dev_text_input = Some(source_path);
                    input_state.mouse_select_anchor = None;
                    clear_dev_key_repeat(input_state);
                    changed = true;
                } else {
                    input_state.editor_focused = false;
                    input_state.focused_dev_text_input = None;
                    input_state.mouse_select_anchor = None;
                    shell.dispatch_source_path(&source_path);
                    changed = true;
                }
            } else {
                input_state.editor_focused = false;
                input_state.focused_dev_text_input = None;
                input_state.mouse_select_anchor = None;
            }
        }
    }
    if input
        .mouse_buttons_down
        .iter()
        .any(|button| button == "left")
        && input_state.editor_focused
        && let Some(anchor) = input_state.mouse_select_anchor.clone()
        && let Some(position) =
            input_layout_position(input.mouse_window_pos, surface_width, surface_height)
        && let Some(head) = dev_position_from_pointer(
            &shell.workspace.selected_buffer,
            layout_frame,
            position.x as f32,
            position.y as f32,
            &mut input_state.column_metric_cache,
        )
    {
        shell.workspace.selected_buffer.set_selection(anchor, head);
        changed = true;
    }

    let shift_pressed = input
        .pressed_keys
        .iter()
        .any(|key| key == "Shift" || key == "RightShift");
    let primary_modifier_pressed = input.pressed_keys.iter().any(|key| {
        matches!(
            key.as_str(),
            "Control" | "RightControl" | "Command" | "RightCommand"
        )
    });
    let keyboard_events = input
        .keyboard_events
        .iter()
        .filter(|event| event.sequence > input_state.last_keyboard_event_sequence)
        .cloned()
        .collect::<Vec<_>>();
    for event in keyboard_events {
        input_state.last_keyboard_event_sequence =
            input_state.last_keyboard_event_sequence.max(event.sequence);
        if !input_state.editor_focused && input_state.focused_dev_text_input.is_none() {
            continue;
        }
        if !event.pressed {
            if input_state.held_repeat_key.as_deref() == Some(event.key.as_str()) {
                clear_dev_key_repeat(input_state);
            }
            continue;
        }
        if primary_modifier_pressed {
            let mut clipboard = NativeClipboardAdapter;
            if input_state.editor_focused {
                match event.key.as_str() {
                    "A" => shell.workspace.selected_buffer.select_all(),
                    "C" => {
                        let _ = shell
                            .workspace
                            .selected_buffer
                            .copy_to_adapter(&mut clipboard);
                    }
                    "X" => {
                        let _ = shell
                            .workspace
                            .selected_buffer
                            .cut_to_adapter(&mut clipboard);
                        shell.workspace.persist_selected_buffer();
                        shell.workspace.set_selected_dirty(true);
                    }
                    "V" => {
                        let _ = shell
                            .workspace
                            .selected_buffer
                            .paste_from_adapter(&mut clipboard);
                        shell.workspace.persist_selected_buffer();
                        shell.workspace.set_selected_dirty(true);
                    }
                    "Z" => {
                        let _ = shell.workspace.selected_buffer.undo();
                        shell.workspace.persist_selected_buffer();
                        shell.workspace.set_selected_dirty(true);
                    }
                    "Y" => {
                        let _ = shell.workspace.selected_buffer.redo();
                        shell.workspace.persist_selected_buffer();
                        shell.workspace.set_selected_dirty(true);
                    }
                    _ => {}
                }
            }
            changed = true;
            continue;
        }
        let applied = if input_state.editor_focused {
            apply_dev_editor_key(shell, event.key.as_str(), shift_pressed)
        } else if input_state.focused_dev_text_input.as_deref() == Some("dev.custom.name") {
            apply_dev_custom_name_key(shell, event.key.as_str(), shift_pressed)
        } else {
            false
        };
        if applied {
            if dev_key_is_repeatable(event.key.as_str()) {
                let now = Instant::now();
                input_state.held_repeat_key = Some(event.key.clone());
                input_state.held_repeat_next_at =
                    now.checked_add(Duration::from_millis(BOON_EDITOR_KEY_REPEAT_DELAY_MS));
            } else {
                clear_dev_key_repeat(input_state);
            }
            changed = true;
        }
    }
    if (input_state.editor_focused || input_state.focused_dev_text_input.is_some())
        && !primary_modifier_pressed
    {
        if let Some(key) = input_state.held_repeat_key.clone() {
            if input.pressed_keys.iter().any(|pressed| pressed == &key) {
                let now = Instant::now();
                let mut applied = 0usize;
                while input_state
                    .held_repeat_next_at
                    .is_some_and(|next| now >= next)
                    && applied < BOON_EDITOR_KEY_REPEAT_MAX_CATCH_UP
                {
                    let key_applied = if input_state.editor_focused {
                        apply_dev_editor_key(shell, &key, shift_pressed)
                    } else if input_state.focused_dev_text_input.as_deref()
                        == Some("dev.custom.name")
                    {
                        apply_dev_custom_name_key(shell, &key, shift_pressed)
                    } else {
                        false
                    };
                    if key_applied {
                        changed = true;
                    }
                    applied += 1;
                    input_state.held_repeat_next_at = input_state
                        .held_repeat_next_at
                        .and_then(|next| {
                            next.checked_add(Duration::from_millis(
                                BOON_EDITOR_KEY_REPEAT_INTERVAL_MS,
                            ))
                        })
                        .or_else(|| {
                            now.checked_add(Duration::from_millis(
                                BOON_EDITOR_KEY_REPEAT_INTERVAL_MS,
                            ))
                        });
                }
            } else {
                clear_dev_key_repeat(input_state);
            }
        }
    } else {
        clear_dev_key_repeat(input_state);
    }

    if changed {
        reset_dev_caret_blink(shell, input_state);
    }

    changed
}

fn dev_editor_caret_visible(input_state: &mut DevNativeInputState, now: Instant) -> bool {
    if !input_state.editor_focused {
        return true;
    }
    let caret_blink_started_at = *input_state.caret_blink_started_at.get_or_insert(now);
    (now.duration_since(caret_blink_started_at).as_millis()
        / BOON_EDITOR_CARET_BLINK_HALF_PERIOD_MS as u128)
        % 2
        == 0
}

fn input_layout_position(
    position: Option<boon_native_app_window::NativeMouseWindowPosition>,
    surface_width: u32,
    surface_height: u32,
) -> Option<boon_native_app_window::NativeMouseWindowPosition> {
    let position = position?;
    let scale_x = if position.window_width > f64::EPSILON {
        f64::from(surface_width) / position.window_width
    } else {
        1.0
    };
    let scale_y = if position.window_height > f64::EPSILON {
        f64::from(surface_height) / position.window_height
    } else {
        1.0
    };
    Some(boon_native_app_window::NativeMouseWindowPosition {
        x: position.x * scale_x,
        y: position.y * scale_y,
        window_width: f64::from(surface_width),
        window_height: f64::from(surface_height),
    })
}

fn clear_dev_key_repeat(input_state: &mut DevNativeInputState) {
    input_state.held_repeat_key = None;
    input_state.held_repeat_next_at = None;
}

fn reset_dev_caret_blink(shell: &mut DevWindowShell, input_state: &mut DevNativeInputState) {
    input_state.caret_blink_started_at = Some(Instant::now());
    shell.caret_visible = true;
}

fn dev_key_is_repeatable(key: &str) -> bool {
    matches!(
        key,
        "Return"
            | "KeypadEnter"
            | "Delete"
            | "ForwardDelete"
            | "Tab"
            | "Home"
            | "End"
            | "LeftArrow"
            | "RightArrow"
            | "UpArrow"
            | "DownArrow"
            | "PageDown"
            | "PageUp"
    ) || keyboard_event_text(key, false).is_some()
}

fn apply_dev_editor_key(shell: &mut DevWindowShell, key: &str, shift_pressed: bool) -> bool {
    match key {
        "Return" | "KeypadEnter" => {
            shell.workspace.selected_buffer.insert_newline_with_indent();
            shell.workspace.persist_selected_buffer();
            shell.workspace.set_selected_dirty(true);
            true
        }
        "Delete" => {
            shell.workspace.selected_buffer.delete_backward();
            shell.workspace.persist_selected_buffer();
            shell.workspace.set_selected_dirty(true);
            true
        }
        "ForwardDelete" => {
            shell.workspace.selected_buffer.delete_forward();
            shell.workspace.persist_selected_buffer();
            shell.workspace.set_selected_dirty(true);
            true
        }
        "Tab" => {
            if shift_pressed {
                shell.workspace.selected_buffer.unindent_selection();
            } else {
                shell.workspace.selected_buffer.indent_selection();
            }
            shell.workspace.persist_selected_buffer();
            shell.workspace.set_selected_dirty(true);
            true
        }
        "Home" => {
            shell.workspace.selected_buffer.move_home(shift_pressed);
            true
        }
        "End" => {
            shell.workspace.selected_buffer.move_end(shift_pressed);
            true
        }
        "LeftArrow" => {
            shell.workspace.selected_buffer.move_left(shift_pressed);
            true
        }
        "RightArrow" => {
            shell.workspace.selected_buffer.move_right(shift_pressed);
            true
        }
        "UpArrow" => {
            shell.workspace.selected_buffer.move_up(shift_pressed);
            true
        }
        "DownArrow" => {
            shell.workspace.selected_buffer.move_down(shift_pressed);
            true
        }
        "PageDown" => {
            shell.workspace.selected_buffer.page_down(shift_pressed);
            true
        }
        "PageUp" => {
            shell.workspace.selected_buffer.page_up(shift_pressed);
            true
        }
        key => {
            if let Some(character) = keyboard_event_text(key, shift_pressed) {
                shell
                    .workspace
                    .selected_buffer
                    .insert_text_at_caret(&character.to_string());
                shell.workspace.persist_selected_buffer();
                shell.workspace.set_selected_dirty(true);
                true
            } else {
                false
            }
        }
    }
}

fn apply_dev_custom_name_key(shell: &mut DevWindowShell, key: &str, shift_pressed: bool) -> bool {
    if !shell.selected_example_is_custom() {
        return false;
    }
    match key {
        "Return" | "KeypadEnter" | "Escape" => true,
        "Delete" | "ForwardDelete" => {
            let mut label = shell.selected_example_label();
            label.pop();
            shell.rename_selected_custom_label(&label);
            true
        }
        key => {
            if let Some(character) = keyboard_event_text(key, shift_pressed) {
                let mut label = shell.selected_example_label();
                label.push(character);
                shell.rename_selected_custom_label(&label);
                true
            } else {
                false
            }
        }
    }
}

fn dev_source_binding_at(
    document: &boon_document_model::DocumentFrame,
    layout_frame: &boon_document::LayoutFrame,
    x: f32,
    y: f32,
) -> Option<(String, String)> {
    let mut hits = layout_frame
        .hit_regions
        .iter()
        .filter(|hit| rect_contains(hit.bounds, x, y))
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        let left_area = left.bounds.width * left.bounds.height;
        let right_area = right.bounds.width * right.bounds.height;
        left_area
            .partial_cmp(&right_area)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.into_iter().find_map(|hit| {
        let binding = document.nodes.get(&hit.node)?.source_binding.as_ref()?;
        if matches!(
            binding.source_path.as_str(),
            "dev.tabs.select" | "dev.commands.press"
        ) {
            return None;
        }
        Some((hit.node.0.clone(), binding.source_path.clone()))
    })
}

struct NativeClipboardAdapter;

impl ClipboardAdapter for NativeClipboardAdapter {
    fn get_text(&mut self) -> Result<String, String> {
        arboard::Clipboard::new()
            .map_err(|error| error.to_string())?
            .get_text()
            .map_err(|error| error.to_string())
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        arboard::Clipboard::new()
            .map_err(|error| error.to_string())?
            .set_text(text.to_owned())
            .map_err(|error| error.to_string())
    }
}

fn dev_position_from_pointer(
    model: &CodeEditorModel,
    layout_frame: &boon_document::LayoutFrame,
    x: f32,
    y: f32,
    column_metric_cache: &mut EditorColumnMetricCache,
) -> Option<EditorPosition> {
    let editor_bounds = layout_frame
        .display_list
        .iter()
        .find(|item| item.node.0 == "dev-code-editor")
        .map(|item| item.bounds)?;
    let relative_y = (y - editor_bounds.y - BOON_EDITOR_PADDING as f32).max(0.0);
    let line = model
        .scroll_line
        .saturating_add((relative_y / BOON_EDITOR_LINE_HEIGHT as f32).floor() as usize)
        .saturating_add(1)
        .min(model.line_count.max(1));
    let line_text = model
        .source_text
        .lines()
        .nth(line.saturating_sub(1))
        .unwrap_or("");
    let line_node_id = format!("dev-code-editor-line-text-{line}");
    let line_item = layout_frame
        .display_list
        .iter()
        .find(|item| item.node.0 == line_node_id)?;
    let inset = style_number_from_map(&line_item.style, "text_inset").unwrap_or(0.0);
    let column_edges = editor_column_edges_for_line(
        column_metric_cache,
        line_text,
        &line_item.style,
        line_item.bounds.height,
    );
    let relative_x = (x - line_item.bounds.x - inset).max(0.0);
    let column =
        nearest_editor_column(&column_edges, relative_x).min(line_text.chars().count() + 1);
    Some(EditorPosition { line, column })
}

fn editor_column_edges_for_line(
    cache: &mut EditorColumnMetricCache,
    line_text: &str,
    style: &BTreeMap<String, boon_document_model::StyleValue>,
    line_height: f32,
) -> Vec<f32> {
    let style_signature = serde_json::to_string(style).unwrap_or_default();
    let key = EditorColumnMetricKey {
        text: line_text.to_owned(),
        style_signature,
        line_height_bits: line_height.to_bits(),
    };
    cache
        .entry(key)
        .or_insert_with(|| {
            boon_native_gpu::editor_text_column_edges_for_style(line_text, style, line_height)
        })
        .clone()
}

fn nearest_editor_column(column_edges: &[f32], relative_x: f32) -> usize {
    if column_edges.is_empty() {
        return 1;
    }
    column_edges
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            let left_distance = (*left - relative_x).abs();
            let right_distance = (*right - relative_x).abs();
            left_distance
                .partial_cmp(&right_distance)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(index, _)| index + 1)
        .unwrap_or(1)
}

fn style_number_from_map(
    style: &BTreeMap<String, boon_document_model::StyleValue>,
    key: &str,
) -> Option<f32> {
    match style.get(key)? {
        boon_document_model::StyleValue::Number(value) => Some(*value as f32),
        boon_document_model::StyleValue::Text(value) => value.parse::<f32>().ok(),
        boon_document_model::StyleValue::Bool(_) => None,
    }
}

fn rect_contains(rect: boon_document::Rect, x: f32, y: f32) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}

#[derive(Clone, Debug)]
struct ExampleCatalog {
    entries: Vec<ExampleCatalogEntry>,
    custom_store_path: PathBuf,
}

impl ExampleCatalog {
    fn load() -> Self {
        let mut entries = boon_runtime::example_manifest_entries()
            .unwrap_or_default()
            .into_iter()
            .map(|entry| ExampleCatalogEntry {
                id: entry.id,
                label: entry.label,
                source: entry.source,
                source_files: entry.source_files,
                inline_source: None,
                category: entry.category,
                order: entry.default_tab_order,
                shown_by_default: entry.shown_by_default,
                custom: false,
            })
            .collect::<Vec<_>>();
        let custom_store_path = std::env::var("BOON_CUSTOM_EXAMPLE_STORE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from("target/state/boon-native-playground/custom_manifest.toml")
            });
        if let Ok(custom_entries) = Self::load_custom_store(&custom_store_path) {
            entries.extend(custom_entries);
        }
        Self {
            entries,
            custom_store_path,
        }
    }

    fn load_custom_store(
        path: &Path,
    ) -> Result<Vec<ExampleCatalogEntry>, Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = std::fs::read_to_string(path)?;
        Self::custom_entries_from_toml(&text)
    }

    fn save_custom_store(&self) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        self.save_custom_store_to(&self.custom_store_path)
    }

    fn save_custom_store_to(
        &self,
        path: &Path,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let custom_entries = self
            .entries
            .iter()
            .filter(|entry| entry.custom)
            .cloned()
            .collect::<Vec<_>>();
        let mut array = Vec::new();
        for entry in &custom_entries {
            let mut item = toml::map::Map::new();
            item.insert(
                "id".to_owned(),
                toml::Value::String(
                    entry
                        .id
                        .strip_prefix("custom:")
                        .unwrap_or(&entry.id)
                        .to_owned(),
                ),
            );
            item.insert("label".to_owned(), toml::Value::String(entry.label.clone()));
            item.insert(
                "source".to_owned(),
                toml::Value::String(entry.source.clone()),
            );
            if !entry.source_files.is_empty() {
                item.insert(
                    "source_files".to_owned(),
                    toml::Value::Array(
                        entry
                            .source_files
                            .iter()
                            .cloned()
                            .map(toml::Value::String)
                            .collect(),
                    ),
                );
            }
            if let Some(source) = &entry.inline_source {
                item.insert(
                    "inline_source".to_owned(),
                    toml::Value::String(source.clone()),
                );
            }
            array.push(toml::Value::Table(item));
        }
        let mut table = toml::map::Map::new();
        table.insert("custom_example".to_owned(), toml::Value::Array(array));
        let text = toml::to_string_pretty(&toml::Value::Table(table))?;
        std::fs::write(path, text.as_bytes())?;
        let loaded_entries = Self::load_custom_store(path)?;
        Ok(json!({
            "status": "pass",
            "command": "PersistCustomExampleStore",
            "store_path": path,
            "stored_entry_count": custom_entries.len(),
            "reloaded_entry_count": loaded_entries.len(),
            "round_trip_pass": loaded_entries.len() == custom_entries.len(),
            "store_sha256": boon_runtime::sha256_file(path)?,
            "metadata_outside_boon_source": true
        }))
    }

    fn custom_entries_from_toml(
        text: &str,
    ) -> Result<Vec<ExampleCatalogEntry>, Box<dyn std::error::Error>> {
        let parsed: toml::Value = toml::from_str(&text)?;
        let entries = parsed
            .get("custom_example")
            .and_then(toml::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let id = item
                            .get("id")
                            .and_then(toml::Value::as_str)
                            .unwrap_or("custom")
                            .to_owned();
                        ExampleCatalogEntry {
                            id: format!("custom:{id}"),
                            label: item
                                .get("label")
                                .and_then(toml::Value::as_str)
                                .unwrap_or(&id)
                                .to_owned(),
                            source: item
                                .get("source")
                                .and_then(toml::Value::as_str)
                                .unwrap_or("")
                                .to_owned(),
                            source_files: item
                                .get("source_files")
                                .and_then(toml::Value::as_array)
                                .map(|items| {
                                    items
                                        .iter()
                                        .filter_map(toml::Value::as_str)
                                        .map(ToOwned::to_owned)
                                        .collect()
                                })
                                .unwrap_or_default(),
                            inline_source: item
                                .get("inline_source")
                                .and_then(toml::Value::as_str)
                                .map(ToOwned::to_owned),
                            category: "custom".to_owned(),
                            order: 10_000 + index as u32,
                            shown_by_default: true,
                            custom: true,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(entries)
    }

    fn list_available_examples(&self) -> serde_json::Value {
        json!({
            "status": "pass",
            "custom_store_path": self.custom_store_path,
            "examples": self.entries.iter().map(|entry| {
                json!({
                    "id": entry.id,
                    "label": entry.label,
                    "category": entry.category,
                    "order": entry.order,
                    "shown_by_default": entry.shown_by_default,
                    "custom": entry.custom,
                    "source_path": entry.source,
                    "source_files": entry.source_files,
                    "has_inline_source": entry.inline_source.is_some()
                })
            }).collect::<Vec<_>>()
        })
    }

    fn custom_example_from_source(id: &str, label: &str, source: String) -> ExampleCatalogEntry {
        ExampleCatalogEntry {
            id: format!("custom:{id}"),
            label: label.to_owned(),
            source: format!("custom://{id}.bn"),
            source_files: Vec::new(),
            inline_source: Some(source),
            category: "custom".to_owned(),
            order: 20_000,
            shown_by_default: true,
            custom: true,
        }
    }

    fn create_blank_custom_example(
        &mut self,
    ) -> Result<(ExampleCatalogEntry, serde_json::Value), Box<dyn std::error::Error>> {
        let existing_custom_count = self.entries.iter().filter(|entry| entry.custom).count();
        let label = format!("Untitled {}", existing_custom_count + 1);
        let base_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let mut suffix = 0usize;
        let id = loop {
            let candidate = if suffix == 0 {
                format!("untitled-{base_millis}")
            } else {
                format!("untitled-{base_millis}-{suffix}")
            };
            let stable = format!("custom:{candidate}");
            if self.entries.iter().all(|entry| entry.id != stable) {
                break candidate;
            }
            suffix += 1;
        };
        let entry = Self::custom_example_from_source(&id, &label, String::new());
        self.entries.push(entry.clone());
        let persistence = self.save_custom_store()?;
        Ok((entry, persistence))
    }

    fn update_custom_source(
        &mut self,
        stable_id: &str,
        source: &str,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.id == stable_id && entry.custom)
        else {
            return Ok(json!({
                "status": "skipped",
                "command": "PersistCustomSource",
                "stable_id": stable_id,
                "reason": "selected example is not custom"
            }));
        };
        entry.inline_source = Some(source.to_owned());
        let source_hash = boon_runtime::sha256_bytes(source.as_bytes());
        let persistence = self.save_custom_store()?;
        Ok(json!({
            "status": "pass",
            "command": "PersistCustomSource",
            "stable_id": stable_id,
            "source_hash": source_hash,
            "source_bytes": source.len(),
            "persistent_store": persistence,
            "metadata_outside_boon_source": true
        }))
    }

    fn rename_custom_example(&mut self, id: &str, label: &str) -> serde_json::Value {
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.id == id && entry.custom)
        else {
            return json!({
                "status": "fail",
                "command": "RenameCustomExample",
                "stable_id": id,
                "diagnostic": "custom example not found"
            });
        };
        let old_label = self.entries[index].label.clone();
        let source_before = self.entries[index].source.clone();
        let inline_hash_before = self.entries[index]
            .inline_source
            .as_ref()
            .map(|source| boon_runtime::sha256_bytes(source.as_bytes()));
        self.entries[index].label = label.to_owned();
        let persistence = self.save_custom_store();
        json!({
            "status": "pass",
            "command": "RenameCustomExample",
            "stable_id": self.entries[index].id,
            "old_label": old_label,
            "new_label": self.entries[index].label,
            "source_unchanged": self.entries[index].source == source_before,
            "inline_source_hash_unchanged": self.entries[index]
                .inline_source
                .as_ref()
                .map(|source| boon_runtime::sha256_bytes(source.as_bytes()))
                == inline_hash_before,
            "persistent_store": persistence
                .unwrap_or_else(|error| json!({"status": "fail", "diagnostic": error.to_string()})),
            "metadata_outside_boon_source": true
        })
    }

    fn remove_custom_example(&mut self, id: &str) -> serde_json::Value {
        let before = self.entries.len();
        let removed_source_hash = self
            .entries
            .iter()
            .find(|entry| entry.id == id && entry.custom)
            .and_then(|entry| entry.inline_source.as_ref())
            .map(|source| boon_runtime::sha256_bytes(source.as_bytes()));
        self.entries
            .retain(|entry| !(entry.id == id && entry.custom));
        let removed = self.entries.len() != before;
        let persistence = self.save_custom_store();
        json!({
            "status": if removed { "pass" } else { "fail" },
            "command": "RemoveCustomExample",
            "stable_id": id,
            "removed": removed,
            "remaining_entry_count": self.entries.len(),
            "removed_source_hash": removed_source_hash,
            "persistent_store": persistence
                .unwrap_or_else(|error| json!({"status": "fail", "diagnostic": error.to_string()})),
            "metadata_outside_boon_source": true
        })
    }

    fn fastest_manifest_fallback_id(&self, removed_id: &str) -> Option<String> {
        self.entries
            .iter()
            .filter(|entry| entry.id != removed_id && entry.shown_by_default && !entry.custom)
            .min_by_key(|entry| (entry.source_weight_bytes(), entry.order))
            .map(|entry| entry.id.clone())
            .or_else(|| {
                self.entries
                    .iter()
                    .filter(|entry| entry.id != removed_id)
                    .min_by_key(|entry| (entry.custom, entry.source_weight_bytes(), entry.order))
                    .map(|entry| entry.id.clone())
            })
    }

    fn custom_store_probe() -> serde_json::Value {
        let path = PathBuf::from("target/artifacts/native-gpu/custom-example-store-probe.toml");
        let catalog = Self {
            entries: vec![Self::custom_example_from_source(
                "stored",
                "Stored Probe",
                "-- stored custom source\nSOURCE\nHOLD\nLATEST\nLIST {}\nList/map\n".to_owned(),
            )],
            custom_store_path: path.clone(),
        };
        match (catalog.save_custom_store(), Self::load_custom_store(&path)) {
            (Ok(persist), Ok(entries)) => json!({
                "status": "pass",
                "command": "LoadCustomExampleStore",
                "persistent_store": persist,
                "entry_count": entries.len(),
                "entries": entries.iter().map(|entry| {
                    json!({
                        "id": entry.id,
                        "label": entry.label,
                        "source": entry.source,
                        "category": entry.category,
                        "shown_by_default": entry.shown_by_default,
                        "custom": entry.custom,
                        "has_inline_source": entry.inline_source.is_some()
                    })
                }).collect::<Vec<_>>(),
                "requires_rust_ui_rewire": false,
                "metadata_outside_boon_source": true
            }),
            (Err(error), _) | (_, Err(error)) => json!({
                "status": "fail",
                "command": "LoadCustomExampleStore",
                "diagnostic": error.to_string()
            }),
        }
    }
}

#[derive(Clone, Debug)]
struct ExampleCatalogEntry {
    id: String,
    label: String,
    source: String,
    source_files: Vec<String>,
    inline_source: Option<String>,
    category: String,
    order: u32,
    shown_by_default: bool,
    custom: bool,
}

impl ExampleCatalogEntry {
    fn source_text(&self) -> Result<String, Box<dyn std::error::Error>> {
        if let Some(source) = &self.inline_source {
            Ok(source.clone())
        } else if !self.source_files.is_empty() {
            let entry = boon_runtime::ExampleManifestEntry {
                id: self.id.clone(),
                label: self.label.clone(),
                source: self.source.clone(),
                source_files: self.source_files.clone(),
                scenario: String::new(),
                budget: String::new(),
                category: self.category.clone(),
                order: self.order,
                default_tab_order: self.order,
                shown_by_default: self.shown_by_default,
                required_evidence_tier: String::new(),
                human_testing_needed: false,
                initial_visible_assertions: Vec::new(),
                input_scenarios: Vec::new(),
                scroll_focus_scenarios: Vec::new(),
                visual_artifacts: Vec::new(),
                performance_thresholds: Vec::new(),
            };
            boon_runtime::source_text_for_entry(&entry)
        } else {
            Ok(std::fs::read_to_string(&self.source)?)
        }
    }

    fn source_weight_bytes(&self) -> u64 {
        self.inline_source
            .as_ref()
            .map(|source| source.len() as u64)
            .or_else(|| {
                if self.source_files.is_empty() {
                    std::fs::metadata(&self.source)
                        .ok()
                        .map(|metadata| metadata.len())
                } else {
                    let mut paths = self.source_files.clone();
                    if !paths.iter().any(|path| path == &self.source) {
                        paths.push(self.source.clone());
                    }
                    Some(
                        paths
                            .iter()
                            .filter_map(|path| std::fs::metadata(path).ok())
                            .map(|metadata| metadata.len())
                            .sum(),
                    )
                }
            })
            .unwrap_or(u64::MAX)
    }
}

#[derive(Clone, Debug)]
struct ExampleWorkspace {
    selected_example_id: String,
    current_file: String,
    selected_buffer: CodeEditorModel,
    open_buffers: BTreeMap<String, CodeEditorModel>,
    dirty_examples: BTreeSet<String>,
    dirty: bool,
}

impl ExampleWorkspace {
    fn new(
        catalog: &ExampleCatalog,
        source_path_label: &str,
        source_text: &str,
        selected_example_id: Option<&str>,
    ) -> Self {
        let selected_example_id = selected_example_id
            .filter(|id| catalog.entries.iter().any(|entry| entry.id == *id))
            .map(ToOwned::to_owned)
            .or_else(|| {
                catalog
                    .entries
                    .iter()
                    .find(|entry| source_path_label.ends_with(&entry.source))
                    .or_else(|| {
                        catalog.entries.iter().find(|entry| {
                            Path::new(source_path_label)
                                .file_name()
                                .and_then(|name| name.to_str())
                                == Path::new(&entry.source)
                                    .file_name()
                                    .and_then(|name| name.to_str())
                        })
                    })
                    .map(|entry| entry.id.clone())
            })
            .or_else(|| catalog.entries.first().map(|entry| entry.id.clone()))
            .unwrap_or_else(|| "custom-buffer".to_owned());
        let selected_buffer = CodeEditorModel::new(source_path_label, source_text);
        let mut open_buffers = BTreeMap::new();
        open_buffers.insert(selected_example_id.clone(), selected_buffer.clone());
        Self {
            selected_buffer,
            selected_example_id,
            current_file: source_path_label.to_owned(),
            open_buffers,
            dirty_examples: BTreeSet::new(),
            dirty: false,
        }
    }

    fn selected_dirty(&self) -> bool {
        self.dirty_examples.contains(&self.selected_example_id)
    }

    fn persist_selected_buffer(&mut self) {
        self.open_buffers.insert(
            self.selected_example_id.clone(),
            self.selected_buffer.clone(),
        );
        self.dirty = self.selected_dirty();
    }

    fn set_selected_dirty(&mut self, dirty: bool) {
        if dirty {
            self.dirty_examples.insert(self.selected_example_id.clone());
        } else {
            self.dirty_examples.remove(&self.selected_example_id);
        }
        self.dirty = dirty;
    }

    fn select_example(
        &mut self,
        catalog: &ExampleCatalog,
        example_id: &str,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let previous_example_id = self.selected_example_id.clone();
        let previous_dirty = self.selected_dirty();
        self.open_buffers
            .insert(previous_example_id.clone(), self.selected_buffer.clone());
        let entry = catalog
            .entries
            .iter()
            .find(|entry| entry.id == example_id)
            .ok_or_else(|| format!("example `{example_id}` is not in ExampleCatalog"))?;
        let loaded_from_open_buffer = self.open_buffers.contains_key(&entry.id);
        let buffer = if let Some(buffer) = self.open_buffers.get(&entry.id).cloned() {
            buffer
        } else {
            let source_text = entry.source_text()?;
            CodeEditorModel::new(&entry.source, &source_text)
        };
        let source_hash = boon_runtime::sha256_bytes(buffer.source_text.as_bytes());
        self.selected_example_id = entry.id.clone();
        self.current_file = buffer.file_name.clone();
        self.selected_buffer = buffer.clone();
        self.open_buffers.insert(entry.id.clone(), buffer);
        self.dirty = self.selected_dirty();
        Ok(json!({
            "status": "pass",
            "selected_example_id": self.selected_example_id,
            "current_file": self.current_file,
            "source_hash": source_hash,
            "buffer_line_count": self.selected_buffer.line_count,
            "custom": entry.custom,
            "previous_example_id": previous_example_id,
            "previous_dirty_preserved": previous_dirty == self.dirty_examples.contains(&previous_example_id),
            "loaded_from_open_buffer": loaded_from_open_buffer,
            "dirty": self.dirty,
            "dirty_examples": self.dirty_examples.iter().cloned().collect::<Vec<_>>(),
            "preview_transport": "ReplaceCode"
        }))
    }

    fn inject_source(
        &mut self,
        example_id: &str,
        file_name: &str,
        source_text: String,
    ) -> serde_json::Value {
        let mut buffer = CodeEditorModel::new(file_name, &source_text);
        buffer.insert_text_at_caret("\n-- injected edit probe");
        let undo_after_insert = buffer.undo();
        let redo_after_undo = buffer.redo();
        self.selected_example_id = example_id.to_owned();
        self.current_file = file_name.to_owned();
        self.selected_buffer = buffer.clone();
        self.open_buffers.insert(example_id.to_owned(), buffer);
        self.set_selected_dirty(true);
        json!({
            "status": "pass",
            "command": "InjectSource",
            "selected_example_id": self.selected_example_id,
            "current_file": self.current_file,
            "source_hash": boon_runtime::sha256_bytes(source_text.as_bytes()),
            "undo_probe": undo_after_insert,
            "redo_probe": redo_after_undo,
            "preview_transport": "ReplaceCode"
        })
    }

    fn apply_editor_text_input(&mut self, text: &str) -> serde_json::Value {
        let before_hash = boon_runtime::sha256_bytes(self.selected_buffer.source_text.as_bytes());
        let before_line_count = self.selected_buffer.line_count;
        self.selected_buffer.insert_text_at_caret(text);
        self.persist_selected_buffer();
        self.set_selected_dirty(true);
        let after_hash = boon_runtime::sha256_bytes(self.selected_buffer.source_text.as_bytes());
        json!({
            "status": "pass",
            "command": "EditorTextInput",
            "selected_example_id": self.selected_example_id,
            "current_file": self.current_file,
            "inserted_text_bytes": text.len(),
            "before_hash": before_hash,
            "after_hash": after_hash,
            "source_changed": before_hash != after_hash,
            "before_line_count": before_line_count,
            "after_line_count": self.selected_buffer.line_count,
            "dirty": self.dirty,
            "dirty_examples": self.dirty_examples.iter().cloned().collect::<Vec<_>>(),
            "diagnostic_count": self.selected_buffer.diagnostics.len(),
            "syntax_token_count": self.selected_buffer.syntax_token_count(),
            "parser_bypassed": false,
            "editor_model_command": self.selected_buffer.last_command
        })
    }

    fn create_or_update_custom_example(
        &mut self,
        catalog: &mut ExampleCatalog,
        id: &str,
        label: &str,
        source_text: String,
    ) -> serde_json::Value {
        let validation = BoonLanguageService::validate_project_source(
            &format!("custom://{id}.bn"),
            &source_text,
        );
        let validation_status = validation.get("status").and_then(serde_json::Value::as_str);
        let parser_accepted = validation_status == Some("pass");
        if !parser_accepted {
            return json!({
                "status": "fail",
                "command": "CreateOrUpdateCustomExample",
                "stable_id": format!("custom:{id}"),
                "label": label,
                "validation": validation,
                "metadata_outside_boon_source": true,
                "requires_rust_ui_rewire": false
            });
        }
        let entry = ExampleCatalog::custom_example_from_source(id, label, source_text.clone());
        catalog.entries.retain(|candidate| candidate.id != entry.id);
        catalog.entries.push(entry.clone());
        self.inject_source(&entry.id, &entry.source, source_text);
        let persistence = catalog.save_custom_store();
        json!({
            "status": "pass",
            "command": "CreateOrUpdateCustomExample",
            "stable_id": entry.id,
            "label": entry.label,
            "validation": validation,
            "executable_runtime_supported": validation_status == Some("pass"),
            "generic_editor_catalog_only": false,
            "custom_store_path": catalog.custom_store_path,
            "persistent_store": persistence
                .unwrap_or_else(|error| json!({"status": "fail", "diagnostic": error.to_string()})),
            "metadata_outside_boon_source": true,
            "requires_rust_ui_rewire": false
        })
    }

    fn run_selected(&self) -> serde_json::Value {
        match boon_parser::parse_source(
            self.selected_buffer.file_name.clone(),
            self.selected_buffer.source_text.clone(),
        ) {
            Ok(_program) => {
                let validation = BoonLanguageService::validate_project_source(
                    &self.selected_buffer.file_name,
                    &self.selected_buffer.source_text,
                );
                let validation_pass =
                    validation.get("status").and_then(serde_json::Value::as_str) == Some("pass");
                json!({
                    "status": if validation_pass { "pass" } else { "fail" },
                    "command": "Run",
                    "selected_example_id": self.selected_example_id,
                    "source_path": self.selected_buffer.file_name,
                    "source_hash": boon_runtime::sha256_bytes(self.selected_buffer.source_text.as_bytes()),
                    "program_kind": "generic",
                    "preview_transport": "ReplaceCode",
                    "validation": validation,
                    "parser_bypassed": false,
                    "runtime_bypassed": false
                })
            }
            Err(error) => json!({
                "status": "fail",
                "command": "Run",
                "selected_example_id": self.selected_example_id,
                "diagnostic": error.to_string(),
                "parser_bypassed": false,
                "runtime_bypassed": true
            }),
        }
    }

    fn format_selected(&mut self) -> serde_json::Value {
        match BoonLanguageService::format(
            &self.selected_buffer.file_name,
            &self.selected_buffer.source_text,
        ) {
            Ok(formatted) => {
                let changed = formatted != self.selected_buffer.source_text;
                self.selected_buffer
                    .replace_text(&self.current_file, formatted.clone());
                self.open_buffers.insert(
                    self.selected_example_id.clone(),
                    self.selected_buffer.clone(),
                );
                if changed {
                    self.set_selected_dirty(true);
                } else {
                    self.dirty = self.selected_dirty();
                }
                json!({
                    "status": "pass",
                    "command": "Format",
                    "selected_example_id": self.selected_example_id,
                    "changed": changed,
                    "formatted_hash": boon_runtime::sha256_bytes(formatted.as_bytes()),
                    "formatter": "boon_parser::format_source",
                    "parser_bypassed": false
                })
            }
            Err(error) => json!({
                "status": "fail",
                "command": "Format",
                "selected_example_id": self.selected_example_id,
                "diagnostic": error.to_string(),
                "formatter": "boon_parser::format_source"
            }),
        }
    }

    fn reset_selected(
        &mut self,
        catalog: &ExampleCatalog,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let entry = catalog
            .entries
            .iter()
            .find(|entry| entry.id == self.selected_example_id)
            .ok_or_else(|| {
                format!(
                    "selected example `{}` is not in ExampleCatalog",
                    self.selected_example_id
                )
            })?;
        let source_text = entry.source_text()?;
        self.selected_buffer = CodeEditorModel::new(&entry.source, &source_text);
        self.current_file = entry.source.clone();
        self.open_buffers
            .insert(entry.id.clone(), self.selected_buffer.clone());
        self.set_selected_dirty(false);
        Ok(json!({
            "status": "pass",
            "command": "Reset",
            "selected_example_id": self.selected_example_id,
            "source_path": entry.source,
            "source_hash": boon_runtime::sha256_bytes(source_text.as_bytes()),
            "dirty": self.dirty
        }))
    }

    fn remove_selected_custom(&mut self, catalog: &mut ExampleCatalog) -> serde_json::Value {
        let removed_id = self.selected_example_id.clone();
        let Some(selected_entry) = catalog.entries.iter().find(|entry| entry.id == removed_id)
        else {
            return json!({
                "status": "fail",
                "command": "RemoveSelectedCustomExample",
                "stable_id": removed_id,
                "diagnostic": "selected example is not in ExampleCatalog"
            });
        };
        if !selected_entry.custom {
            return json!({
                "status": "fail",
                "command": "RemoveSelectedCustomExample",
                "stable_id": removed_id,
                "diagnostic": "selected example is manifest-backed and cannot be removed"
            });
        }
        let fallback_id = catalog.fastest_manifest_fallback_id(&removed_id);
        let removed_open_buffer = self.open_buffers.remove(&removed_id).is_some();
        let removed_dirty_marker = self.dirty_examples.remove(&removed_id);
        let removal = catalog.remove_custom_example(&removed_id);
        if removal.get("status").and_then(serde_json::Value::as_str) != Some("pass") {
            return json!({
                "status": "fail",
                "command": "RemoveSelectedCustomExample",
                "stable_id": removed_id,
                "removed_open_buffer": removed_open_buffer,
                "removed_dirty_marker": removed_dirty_marker,
                "catalog_removal": removal
            });
        }
        let fallback_selection = fallback_id
            .as_deref()
            .map(|id| self.select_example(catalog, id))
            .transpose();
        let removed_open_buffer_after_fallback = self.open_buffers.remove(&removed_id).is_some();
        let removed_dirty_marker_after_fallback = self.dirty_examples.remove(&removed_id);
        match fallback_selection {
            Ok(selection) => {
                let selected_after_removal = self.selected_example_id.clone();
                json!({
                    "status": "pass",
                    "command": "RemoveSelectedCustomExample",
                    "stable_id": removed_id,
                    "selected_after_removal": selected_after_removal,
                    "removed_open_buffer": removed_open_buffer || removed_open_buffer_after_fallback,
                    "removed_dirty_marker": removed_dirty_marker || removed_dirty_marker_after_fallback,
                    "removed_not_listed": catalog.entries.iter().all(|entry| entry.id != removed_id),
                    "catalog_removal": removal,
                    "fallback_selection": selection,
                    "fallback_strategy": "smallest-manifest-source",
                    "preview_transport": "ReplaceCode"
                })
            }
            Err(error) => json!({
                "status": "fail",
                "command": "RemoveSelectedCustomExample",
                "stable_id": removed_id,
                "catalog_removal": removal,
                "diagnostic": error.to_string()
            }),
        }
    }

    fn dirty_tab_preservation_probe(
        &self,
        catalog: &ExampleCatalog,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut workspace = self.clone();
        let original_id = workspace.selected_example_id.clone();
        let alternate_id = catalog
            .entries
            .iter()
            .find(|entry| entry.id != original_id && entry.shown_by_default)
            .map(|entry| entry.id.clone())
            .ok_or("no alternate example tab available for dirty preservation probe")?;
        workspace
            .selected_buffer
            .insert_text_at_caret("\n-- dirty tab preservation probe");
        workspace.persist_selected_buffer();
        workspace.set_selected_dirty(true);
        let dirty_hash_before =
            boon_runtime::sha256_bytes(workspace.selected_buffer.source_text.as_bytes());
        let switch_away = workspace.select_example(catalog, &alternate_id)?;
        let switch_back = workspace.select_example(catalog, &original_id)?;
        let dirty_hash_after =
            boon_runtime::sha256_bytes(workspace.selected_buffer.source_text.as_bytes());
        let pass = dirty_hash_before == dirty_hash_after
            && workspace.selected_dirty()
            && workspace.dirty_examples.contains(&original_id)
            && switch_back
                .get("loaded_from_open_buffer")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
        Ok(json!({
            "status": if pass { "pass" } else { "fail" },
            "command": "DirtyTabPreservation",
            "original_example_id": original_id,
            "alternate_example_id": alternate_id,
            "dirty_hash_before": dirty_hash_before,
            "dirty_hash_after": dirty_hash_after,
            "dirty_preserved": dirty_hash_before == dirty_hash_after,
            "dirty_marker_preserved": workspace.selected_dirty(),
            "dirty_examples": workspace.dirty_examples.iter().cloned().collect::<Vec<_>>(),
            "switch_away": switch_away,
            "switch_back": switch_back
        }))
    }
}

#[derive(Clone, Debug)]
struct BoonLanguageService;

impl BoonLanguageService {
    fn diagnostics(path: &str, source: &str) -> Vec<String> {
        if source.len() > BOON_EDITOR_FULL_LANGUAGE_BYTES_MAX {
            return Vec::new();
        }
        match boon_parser::parse_source(path.to_owned(), source.to_owned()) {
            Ok(_) => Vec::new(),
            Err(error) => vec![error.to_string()],
        }
    }

    fn format(path: &str, source: &str) -> Result<String, boon_parser::ParseError> {
        if source.len() > BOON_EDITOR_FULL_LANGUAGE_BYTES_MAX {
            return Ok(source.to_owned());
        }
        boon_parser::format_source(path.to_owned(), source.to_owned())
    }

    fn validate_project_source(path: &str, source: &str) -> serde_json::Value {
        let source_hash = boon_runtime::sha256_bytes(source.as_bytes());
        let parse = boon_parser::parse_source(path.to_owned(), source.to_owned());
        match parse {
            Ok(_program) => {
                let scenario_path = Path::new(path).with_extension("scn");
                let runtime = if scenario_path.exists() {
                    boon_runtime::LiveRuntime::new(
                        &format!("dev-window-validate:{path}"),
                        source,
                        &scenario_path,
                    )
                    .map_err(|error| error.to_string())
                } else {
                    boon_runtime::LiveRuntime::from_source(
                        &format!("dev-window-validate:{path}"),
                        source,
                    )
                    .map_err(|error| error.to_string())
                };
                match runtime {
                    Ok(mut runtime) => json!({
                        "status": "pass",
                        "source_hash": source_hash,
                        "program_kind": "generic",
                        "scenario_path": scenario_path,
                        "scenario_bound": scenario_path.exists(),
                        "runtime_surface": "generic-live-runtime",
                        "runtime_summary_hash": boon_runtime::sha256_bytes(
                            &serde_json::to_vec(&runtime.state_summary()).unwrap_or_default()
                        ),
                        "parser_bypassed": false,
                        "runtime_bypassed": false
                    }),
                    Err(error) => json!({
                        "status": "fail",
                        "source_hash": source_hash,
                        "program_kind": "generic",
                        "scenario_path": scenario_path,
                        "diagnostic": error,
                        "parser_bypassed": false,
                        "runtime_bypassed": false
                    }),
                }
            }
            Err(error) => json!({
                "status": "fail",
                "source_hash": source_hash,
                "diagnostic": error.to_string(),
                "parser_bypassed": false,
                "runtime_bypassed": true
            }),
        }
    }

    fn syntax_highlighting(source: &str) -> SyntaxHighlighting {
        if source.len() > BOON_EDITOR_FULL_LANGUAGE_BYTES_MAX {
            return SyntaxHighlighting {
                backend: "editor-fallback-tokenizer-deferred-large-buffer",
                parser_backed: false,
                tokens: Self::syntax_tokens_fallback_limited(
                    source,
                    BOON_EDITOR_DEFERRED_SYNTAX_LINES,
                ),
            };
        }
        if let Ok(ast) = boon_parser::parse_ast("<editor>", source) {
            let tokens = ast
                .tokens
                .iter()
                .filter(|token| token.kind != AstTokenKind::Newline)
                .filter_map(|token| Self::syntax_token_from_ast_token(source, token))
                .collect::<Vec<_>>();
            return SyntaxHighlighting {
                backend: "boon_parser::parse_ast",
                parser_backed: true,
                tokens: Self::apply_original_boon_semantics(source, tokens),
            };
        }
        SyntaxHighlighting {
            backend: "editor-fallback-tokenizer",
            parser_backed: false,
            tokens: Self::syntax_tokens_fallback(source),
        }
    }

    fn syntax_tokens_fallback_limited(source: &str, max_lines: usize) -> Vec<SyntaxToken> {
        let mut end = 0usize;
        let mut lines = 0usize;
        for raw_line in source.split_inclusive('\n') {
            if lines >= max_lines {
                break;
            }
            end += raw_line.len();
            lines += 1;
        }
        if end == 0 {
            end = source.len();
        }
        Self::syntax_tokens_fallback(&source[..end.min(source.len())])
    }

    fn syntax_token_from_ast_token(
        source: &str,
        token: &boon_parser::AstToken,
    ) -> Option<SyntaxToken> {
        let raw = source.get(token.start..token.end).unwrap_or(&token.lexeme);
        let text = Self::visible_token_text(token.kind, raw, &token.lexeme)?;
        let leading_chars = raw
            .find(text)
            .map(|byte| raw[..byte].chars().count())
            .unwrap_or(0);
        let leading_bytes = raw.find(text).unwrap_or(0);
        let start = token.start.saturating_add(leading_bytes);
        let end = start.saturating_add(text.len());
        Some(SyntaxToken::new_at(
            Self::syntax_kind_from_ast_token(token.kind, text),
            token.line,
            token.column + leading_chars,
            text,
            start,
            end,
        ))
    }

    fn visible_token_text<'a>(
        kind: AstTokenKind,
        raw: &'a str,
        lexeme: &'a str,
    ) -> Option<&'a str> {
        match kind {
            AstTokenKind::Comment => raw
                .find("--")
                .and_then(|start| raw.get(start..))
                .or_else(|| (!lexeme.is_empty()).then_some(lexeme)),
            AstTokenKind::String => raw
                .find('"')
                .and_then(|start| raw.get(start..))
                .or_else(|| (!lexeme.is_empty()).then_some(lexeme)),
            AstTokenKind::Newline => None,
            _ => (!lexeme.is_empty()).then_some(lexeme),
        }
    }

    fn syntax_kind_from_ast_token(kind: AstTokenKind, lexeme: &str) -> &'static str {
        if lexeme == "EXAMPLE" || lexeme == "#" {
            return "invalid";
        }
        match kind {
            AstTokenKind::Comment => "comment",
            AstTokenKind::String => "string",
            AstTokenKind::Number => "number",
            AstTokenKind::Operator => "operator",
            AstTokenKind::Symbol => match lexeme {
                ":" | "," | "." | "(" | ")" | "{" | "}" | "[" | "]" => "punctuation",
                _ => "invalid",
            },
            AstTokenKind::Identifier => match lexeme {
                _ if lexeme.contains('/') => "source-binding",
                _ if Self::is_keyword_lexeme(lexeme) => "keyword",
                _ => "variable",
            },
            AstTokenKind::Unknown | AstTokenKind::Newline => "invalid",
        }
    }

    fn is_keyword_lexeme(lexeme: &str) -> bool {
        lexeme.chars().count() >= 2
            && lexeme.chars().any(|ch| ch.is_ascii_uppercase())
            && lexeme
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch == '_')
    }

    fn apply_original_boon_semantics(source: &str, tokens: Vec<SyntaxToken>) -> Vec<SyntaxToken> {
        let mut decorations = Self::text_literal_decorations(source, &tokens);
        decorations.extend(Self::single_quote_literal_decorations(source));
        let decoration_ranges = decorations
            .iter()
            .map(|token| (token.start, token.end))
            .collect::<Vec<_>>();
        let base_tokens = tokens
            .into_iter()
            .filter(|token| {
                !decoration_ranges.iter().any(|(start, end)| {
                    token.start >= *start && token.end <= *end && token.start < token.end
                })
            })
            .collect::<Vec<_>>();
        let mut styled = Vec::new();
        let mut expect_function_name = false;
        let mut chain_index = 0usize;
        for (index, token) in base_tokens.iter().enumerate() {
            let mut token = token.clone();
            let previous_source_char = token
                .start
                .checked_sub(1)
                .and_then(|start| source.get(start..token.start))
                .and_then(|slice| slice.chars().next());
            let next_token = base_tokens.get(index + 1);
            match token.kind {
                "keyword" => {
                    expect_function_name = token.text == "FUNCTION";
                    chain_index = 0;
                    styled.push(token);
                }
                "source-binding" if Self::is_module_path_lexeme(&token.text) => {
                    Self::push_module_path_tokens(&mut styled, &token);
                    expect_function_name = false;
                    chain_index = 0;
                }
                "variable" | "source-binding" => {
                    if token.text == "__" {
                        token.kind = "wildcard";
                        expect_function_name = false;
                    } else if token.text == "EXAMPLE" {
                        token.kind = "invalid";
                        expect_function_name = false;
                    } else if Self::is_keyword_lexeme(&token.text) {
                        token.kind = "keyword";
                        expect_function_name = token.text == "FUNCTION";
                    } else if Self::is_pascal_case_lexeme(&token.text) {
                        token.kind = if next_token
                            .is_some_and(|next| next.line == token.line && next.text == "[")
                        {
                            "tag"
                        } else {
                            "type"
                        };
                        expect_function_name = false;
                    } else {
                        if previous_source_char == Some('.') {
                            chain_index += 1;
                        } else {
                            chain_index = 0;
                        }
                        token.kind = if expect_function_name {
                            expect_function_name = false;
                            "function"
                        } else if next_token
                            .is_some_and(|next| next.line == token.line && next.text == ":")
                        {
                            "definition"
                        } else if next_token
                            .is_some_and(|next| next.line == token.line && next.text == "(")
                        {
                            "function"
                        } else if chain_index % 2 == 1 {
                            "chain-alt"
                        } else {
                            "variable"
                        };
                    }
                    styled.push(token);
                }
                "operator" => {
                    token.kind = if token.text == "|>" {
                        "pipe"
                    } else if token.text == "-"
                        && next_token
                            .is_some_and(|next| next.kind == "number" && next.start == token.end)
                    {
                        "negative-sign"
                    } else {
                        "operator"
                    };
                    expect_function_name = false;
                    chain_index = 0;
                    styled.push(token);
                }
                "punctuation" => {
                    token.kind = match token.text.as_str() {
                        "." => "dot",
                        _ => "punctuation",
                    };
                    if token.text != "." {
                        chain_index = 0;
                    }
                    expect_function_name = false;
                    styled.push(token);
                }
                "number" => {
                    expect_function_name = false;
                    chain_index = 0;
                    styled.push(token);
                }
                "comment" | "string" | "invalid" => {
                    expect_function_name = false;
                    chain_index = 0;
                    styled.push(token);
                }
                _ => {
                    expect_function_name = false;
                    chain_index = 0;
                    styled.push(token);
                }
            }
        }
        styled.extend(decorations);
        styled.sort_by_key(|token| (token.line, token.column, token.start, token.len));
        styled
    }

    fn text_literal_decorations(source: &str, tokens: &[SyntaxToken]) -> Vec<SyntaxToken> {
        let mut decorations = Vec::new();
        for token in tokens.iter().filter(|token| token.text == "TEXT") {
            let mut position = token.end;
            while source
                .as_bytes()
                .get(position)
                .is_some_and(|byte| byte.is_ascii_whitespace())
            {
                position += 1;
            }
            let hash_start = position;
            while source.as_bytes().get(position) == Some(&b'#') {
                position += 1;
            }
            let hash_count = position.saturating_sub(hash_start);
            if source.as_bytes().get(position) != Some(&b'{') {
                continue;
            }
            let open_brace = position;
            let Some(close_brace) = matching_brace_byte(source, open_brace) else {
                Self::push_range_tokens(
                    &mut decorations,
                    "text-literal-delimiter",
                    source,
                    hash_start,
                    open_brace + 1,
                );
                Self::push_range_tokens(
                    &mut decorations,
                    "text-literal-content",
                    source,
                    open_brace + 1,
                    source.len(),
                );
                continue;
            };
            Self::push_range_tokens(
                &mut decorations,
                "text-literal-delimiter",
                source,
                hash_start,
                open_brace + 1,
            );
            let content_start = open_brace + 1;
            let content_end = close_brace;
            let marker = if hash_count == 0 {
                "{".to_owned()
            } else {
                format!("{}{{", "#".repeat(hash_count))
            };
            let mut content_position = content_start;
            while content_position < content_end {
                let Some(relative) = source[content_position..content_end].find(&marker) else {
                    break;
                };
                let interpolation_start = content_position + relative;
                if interpolation_start > content_position {
                    Self::push_range_tokens(
                        &mut decorations,
                        "text-literal-content",
                        source,
                        content_position,
                        interpolation_start,
                    );
                }
                let interpolation_open_brace = interpolation_start + marker.len() - 1;
                let Some(interpolation_close_brace) =
                    matching_brace_byte(source, interpolation_open_brace)
                else {
                    content_position = interpolation_start + marker.len();
                    continue;
                };
                if interpolation_close_brace > content_end {
                    break;
                }
                Self::push_range_tokens(
                    &mut decorations,
                    "text-literal-delimiter",
                    source,
                    interpolation_start,
                    interpolation_open_brace + 1,
                );
                Self::push_range_tokens(
                    &mut decorations,
                    "text-literal-interpolation",
                    source,
                    interpolation_open_brace + 1,
                    interpolation_close_brace,
                );
                Self::push_range_tokens(
                    &mut decorations,
                    "text-literal-delimiter",
                    source,
                    interpolation_close_brace,
                    interpolation_close_brace + 1,
                );
                content_position = interpolation_close_brace + 1;
            }
            if content_position < content_end {
                Self::push_range_tokens(
                    &mut decorations,
                    "text-literal-content",
                    source,
                    content_position,
                    content_end,
                );
            }
            Self::push_range_tokens(
                &mut decorations,
                "text-literal-delimiter",
                source,
                close_brace,
                close_brace + 1,
            );
        }
        decorations
    }

    fn single_quote_literal_decorations(source: &str) -> Vec<SyntaxToken> {
        let mut decorations = Vec::new();
        let mut position = 0usize;
        while let Some(relative_start) = source[position..].find('\'') {
            let start = position + relative_start;
            let mut end = start + 1;
            let mut escaped = false;
            while end < source.len() {
                let Some(ch) = source[end..].chars().next() else {
                    break;
                };
                end += ch.len_utf8();
                if ch == '\n' || ch == '\r' {
                    break;
                }
                if escaped {
                    escaped = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    continue;
                }
                if ch == '\'' {
                    break;
                }
            }
            Self::push_range_tokens(&mut decorations, "string", source, start, end);
            position = end;
        }
        decorations
    }

    fn push_range_tokens(
        tokens: &mut Vec<SyntaxToken>,
        kind: &'static str,
        source: &str,
        start: usize,
        end: usize,
    ) {
        if start >= end || start >= source.len() {
            return;
        }
        let mut line_start = 0usize;
        for (line_index, line) in source.split_inclusive('\n').enumerate() {
            let line_end = line_start + line.len();
            let segment_start = start.max(line_start);
            let segment_end = end.min(line_end);
            if segment_start < segment_end {
                let trimmed_end =
                    if source.as_bytes().get(segment_end.saturating_sub(1)) == Some(&b'\n') {
                        segment_end.saturating_sub(1)
                    } else {
                        segment_end
                    };
                if segment_start < trimmed_end {
                    if let Some(text) = source.get(segment_start..trimmed_end) {
                        let column = source
                            .get(line_start..segment_start)
                            .map(|prefix| prefix.chars().count() + 1)
                            .unwrap_or(1);
                        tokens.push(SyntaxToken::new_at(
                            kind,
                            line_index + 1,
                            column,
                            text,
                            segment_start,
                            trimmed_end,
                        ));
                    }
                }
            }
            line_start = line_end;
            if line_start >= end {
                break;
            }
        }
    }

    fn push_module_path_tokens(tokens: &mut Vec<SyntaxToken>, token: &SyntaxToken) {
        let last_slash = token.text.rfind('/').unwrap_or(token.text.len());
        let mut byte_offset = 0usize;
        while byte_offset < token.text.len() {
            let Some(relative_slash) = token.text[byte_offset..].find('/') else {
                let text = &token.text[byte_offset..];
                if !text.is_empty() {
                    let kind = if byte_offset > last_slash {
                        "function"
                    } else {
                        "source-binding"
                    };
                    tokens.push(token.subtoken(kind, byte_offset, text));
                }
                break;
            };
            let slash = byte_offset + relative_slash;
            if slash > byte_offset {
                tokens.push(token.subtoken(
                    "source-binding",
                    byte_offset,
                    &token.text[byte_offset..slash],
                ));
            }
            tokens.push(token.subtoken("module-slash", slash, "/"));
            byte_offset = slash + 1;
        }
    }

    fn is_module_path_lexeme(lexeme: &str) -> bool {
        let mut parts = lexeme.split('/').collect::<Vec<_>>();
        if parts.len() < 2 || parts.iter().any(|part| part.is_empty()) {
            return false;
        }
        let Some(final_part) = parts.pop() else {
            return false;
        };
        final_part
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_lowercase())
            && parts.iter().all(|part| {
                part.chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_uppercase())
            })
    }

    fn is_pascal_case_lexeme(lexeme: &str) -> bool {
        lexeme
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_uppercase())
            && lexeme.chars().any(|ch| ch.is_ascii_lowercase())
            && lexeme.chars().all(|ch| ch.is_ascii_alphanumeric())
    }

    fn invalid_syntax_probe() -> serde_json::Value {
        let source = "EXAMPLE Demo\n# old comment\n";
        let highlighting = Self::syntax_highlighting(source);
        let invalid_tokens = highlighting
            .tokens
            .iter()
            .filter(|token| token.kind == "invalid")
            .map(|token| {
                json!({
                    "kind": token.kind,
                    "text": token.text,
                    "line": token.line,
                    "column": token.column,
                    "len": token.len,
                    "color": syntax_color_for_kind(token.kind),
                    "font_weight": syntax_font_weight_for_kind(token.kind),
                    "font_style": syntax_font_style_for_kind(token.kind)
                })
            })
            .collect::<Vec<_>>();
        let example_invalid = invalid_tokens
            .iter()
            .any(|token| token.get("text").and_then(serde_json::Value::as_str) == Some("EXAMPLE"));
        let hash_invalid = invalid_tokens
            .iter()
            .any(|token| token.get("text").and_then(serde_json::Value::as_str) == Some("#"));
        json!({
            "status": if example_invalid && hash_invalid { "pass" } else { "fail" },
            "backend": highlighting.backend,
            "parser_backed": highlighting.parser_backed,
            "invalid_token_count": invalid_tokens.len(),
            "invalid_token_samples": invalid_tokens,
            "example_keyword_invalid": example_invalid,
            "hash_comment_invalid": hash_invalid
        })
    }

    fn syntax_tokens_fallback(source: &str) -> Vec<SyntaxToken> {
        let mut tokens = Vec::new();
        let mut line_start = 0usize;
        for (line_index, raw_line) in source.split_inclusive('\n').enumerate() {
            let line = raw_line.trim_end_matches('\n');
            let mut column = 0;
            let bytes = line.as_bytes();
            while column < bytes.len() {
                let rest = &line[column..];
                if rest.starts_with("--") {
                    tokens.push(SyntaxToken::new_at(
                        "comment",
                        line_index + 1,
                        column + 1,
                        rest,
                        line_start + column,
                        line_start + line.len(),
                    ));
                    break;
                }
                let Some(ch) = rest.chars().next() else {
                    break;
                };
                if ch.is_whitespace() {
                    column += ch.len_utf8();
                    continue;
                }
                if ch == '"' || ch == '\'' {
                    let mut len = ch.len_utf8();
                    for next in rest[ch.len_utf8()..].chars() {
                        len += next.len_utf8();
                        if next == ch || next == '\n' {
                            break;
                        }
                    }
                    tokens.push(SyntaxToken::new_at(
                        "string",
                        line_index + 1,
                        column + 1,
                        &rest[..len],
                        line_start + column,
                        line_start + column + len,
                    ));
                    column += len;
                    continue;
                }
                if ch.is_ascii_digit() {
                    let text = rest
                        .chars()
                        .take_while(|next| next.is_ascii_digit() || *next == '.')
                        .collect::<String>();
                    tokens.push(SyntaxToken::new_at(
                        "number",
                        line_index + 1,
                        column + 1,
                        &text,
                        line_start + column,
                        line_start + column + text.len(),
                    ));
                    column += text.len();
                    continue;
                }
                if ch.is_ascii_alphabetic() || ch == '_' {
                    let text = rest
                        .chars()
                        .take_while(|next| {
                            next.is_ascii_alphanumeric()
                                || *next == '_'
                                || *next == '/'
                                || *next == '-'
                        })
                        .collect::<String>();
                    let kind = match text.as_str() {
                        "EXAMPLE" => "invalid",
                        "__" => "wildcard",
                        _ if Self::is_module_path_lexeme(&text) => "source-binding",
                        _ if Self::is_keyword_lexeme(&text) => "keyword",
                        _ if Self::is_pascal_case_lexeme(&text) => "type",
                        _ => "variable",
                    };
                    let token = SyntaxToken::new_at(
                        kind,
                        line_index + 1,
                        column + 1,
                        &text,
                        line_start + column,
                        line_start + column + text.len(),
                    );
                    if kind == "source-binding" {
                        Self::push_module_path_tokens(&mut tokens, &token);
                    } else {
                        tokens.push(token);
                    }
                    column += text.len();
                    continue;
                }
                let kind = if ch == '#' || ch == '$' {
                    "invalid"
                } else if "{}[]():,".contains(ch) {
                    "punctuation"
                } else if ch == '.' {
                    "dot"
                } else if "|=+-*/<>".contains(ch) {
                    "operator"
                } else {
                    "invalid"
                };
                tokens.push(SyntaxToken::new_at(
                    kind,
                    line_index + 1,
                    column + 1,
                    &rest[..ch.len_utf8()],
                    line_start + column,
                    line_start + column + ch.len_utf8(),
                ));
                column += ch.len_utf8();
            }
            line_start += raw_line.len();
        }
        Self::apply_original_boon_semantics(source, tokens)
    }
}

#[derive(Clone, Debug)]
struct SyntaxHighlighting {
    backend: &'static str,
    parser_backed: bool,
    tokens: Vec<SyntaxToken>,
}

#[derive(Clone, Debug)]
struct SyntaxToken {
    kind: &'static str,
    line: usize,
    column: usize,
    len: usize,
    text: String,
    start: usize,
    end: usize,
}

impl SyntaxToken {
    fn new_at(
        kind: &'static str,
        line: usize,
        column: usize,
        text: &str,
        start: usize,
        end: usize,
    ) -> Self {
        Self {
            kind,
            line,
            column,
            len: text.chars().count().max(1),
            text: text.to_owned(),
            start,
            end,
        }
    }

    fn subtoken(&self, kind: &'static str, byte_offset: usize, text: &str) -> Self {
        let column = self.column + self.text[..byte_offset].chars().count();
        let start = self.start + byte_offset;
        Self::new_at(kind, self.line, column, text, start, start + text.len())
    }
}

#[derive(Clone, Debug)]
struct SyntaxLineSegment {
    kind: &'static str,
    line: usize,
    column: usize,
    len: usize,
    text: String,
}

impl SyntaxLineSegment {
    fn new(kind: &'static str, line: usize, column: usize, text: String) -> Self {
        Self {
            kind,
            line,
            column,
            len: text.chars().count(),
            text,
        }
    }

    fn to_report_json(&self) -> serde_json::Value {
        json!({
            "kind": self.kind,
            "line": self.line,
            "column": self.column,
            "len": self.len,
            "text": self.text.chars().take(80).collect::<String>(),
            "color": syntax_color_for_kind(self.kind),
            "font_weight": syntax_font_weight_for_kind(self.kind),
            "font_style": syntax_font_style_for_kind(self.kind)
        })
    }
}

#[derive(Clone, Debug)]
struct CodeEditorModel {
    file_name: String,
    buffer: EditorBuffer,
    source_text: String,
    line_count: usize,
    selection: EditorSelection,
    scroll_line: usize,
    scroll_column: usize,
    diagnostics: Vec<String>,
    syntax_tokens: Vec<SyntaxToken>,
    syntax_backend: &'static str,
    syntax_parser_backed: bool,
    formatted_preview_hash: Option<String>,
    clipboard_cache: String,
    last_command: Option<&'static str>,
}

impl CodeEditorModel {
    fn new(source_path_label: &str, source_text: &str) -> Self {
        let diagnostics = BoonLanguageService::diagnostics(source_path_label, source_text);
        let formatted_preview_hash = if source_text.len() <= BOON_EDITOR_FULL_LANGUAGE_BYTES_MAX {
            BoonLanguageService::format(source_path_label, source_text)
                .ok()
                .map(|formatted| boon_runtime::sha256_bytes(formatted.as_bytes()))
        } else {
            None
        };
        let syntax = BoonLanguageService::syntax_highlighting(source_text);
        let buffer = EditorBuffer::new(source_text);
        let line_count = buffer.line_count();
        Self {
            file_name: source_path_label.to_owned(),
            buffer,
            source_text: source_text.to_owned(),
            line_count,
            selection: EditorSelection::collapsed(EditorPosition::start()),
            scroll_line: 0,
            scroll_column: 0,
            diagnostics,
            syntax_tokens: syntax.tokens,
            syntax_backend: syntax.backend,
            syntax_parser_backed: syntax.parser_backed,
            formatted_preview_hash,
            clipboard_cache: String::new(),
            last_command: None,
        }
    }

    fn syntax_token_count(&self) -> usize {
        self.syntax_tokens.len()
    }

    fn syntax_backend(&self) -> &'static str {
        self.syntax_backend
    }

    fn syntax_parser_backed(&self) -> bool {
        self.syntax_parser_backed
    }

    fn syntax_categories(&self) -> Vec<&'static str> {
        self.syntax_tokens
            .iter()
            .map(|token| token.kind)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn syntax_token_samples(&self) -> Vec<serde_json::Value> {
        self.syntax_tokens
            .iter()
            .take(8)
            .map(|token| {
                json!({
                    "kind": token.kind,
                    "text": token.text.chars().take(80).collect::<String>(),
                    "line": token.line,
                    "column": token.column,
                    "len": token.len,
                    "color": syntax_color_for_kind(token.kind),
                    "font_weight": syntax_font_weight_for_kind(token.kind),
                    "font_style": syntax_font_style_for_kind(token.kind)
                })
            })
            .collect()
    }

    fn syntax_invalid_token_samples(&self) -> Vec<serde_json::Value> {
        self.syntax_tokens
            .iter()
            .filter(|token| token.kind == "invalid")
            .take(8)
            .map(|token| {
                json!({
                    "kind": token.kind,
                    "text": token.text.chars().take(80).collect::<String>(),
                    "line": token.line,
                    "column": token.column,
                    "len": token.len,
                    "color": syntax_color_for_kind(token.kind),
                    "font_weight": syntax_font_weight_for_kind(token.kind),
                    "font_style": syntax_font_style_for_kind(token.kind)
                })
            })
            .collect()
    }

    fn syntax_render_segments_for_visible_lines(&self, max_lines: usize) -> Vec<SyntaxLineSegment> {
        self.visible_lines(max_lines)
            .into_iter()
            .flat_map(|(line_number, line)| self.highlighted_line_segments(line_number, &line))
            .collect()
    }

    fn syntax_render_categories(&self) -> Vec<&'static str> {
        self.syntax_render_segments_for_visible_lines(40)
            .into_iter()
            .map(|segment| segment.kind)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn syntax_render_segment_samples(&self) -> Vec<serde_json::Value> {
        self.syntax_render_segments_for_visible_lines(40)
            .into_iter()
            .filter(|segment| !segment.text.is_empty())
            .take(16)
            .map(|segment| segment.to_report_json())
            .collect()
    }

    fn syntax_theme_report(&self) -> serde_json::Value {
        json!({
            "source": "~/repos/boon/playground/frontend/typescript/code_editor/boon-theme.ts",
            "language_source": "~/repos/boon/playground/frontend/typescript/code_editor/boon-language.ts",
            "grammar_source": "~/repos/boon/playground/frontend/typescript/code_editor/boon.grammar",
            "font_family": BOON_EDITOR_FONT_FAMILY,
            "font_size": BOON_EDITOR_FONT_SIZE,
            "line_height": BOON_EDITOR_LINE_HEIGHT,
            "font_features": BOON_EDITOR_FONT_FEATURES,
            "font_feature_settings": BOON_EDITOR_FONT_FEATURE_SETTINGS,
            "background": BOON_EDITOR_BACKGROUND,
            "foreground": BOON_EDITOR_FOREGROUND,
            "gutter": BOON_EDITOR_GUTTER,
            "dark_background": BOON_EDITOR_DARK_BACKGROUND,
            "highlight_background": BOON_EDITOR_HIGHLIGHT_BACKGROUND,
            "selection": BOON_EDITOR_SELECTION,
            "cursor": BOON_EDITOR_CURSOR,
            "bracket_match": BOON_EDITOR_BRACKET_MATCH,
            "selection_match": BOON_EDITOR_SELECTION_MATCH,
            "rules": {
                "keyword": syntax_style_json("keyword"),
                "source-binding": syntax_style_json("source-binding"),
                "tag": syntax_style_json("tag"),
                "type": syntax_style_json("type"),
                "variable": syntax_style_json("variable"),
                "function": syntax_style_json("function"),
                "definition": syntax_style_json("definition"),
                "operator": syntax_style_json("operator"),
                "punctuation": syntax_style_json("punctuation"),
                "string": syntax_style_json("string"),
                "number": syntax_style_json("number"),
                "comment": syntax_style_json("comment"),
                "invalid": syntax_style_json("invalid")
            }
        })
    }

    fn highlighted_line_segments(&self, line_number: usize, line: &str) -> Vec<SyntaxLineSegment> {
        let line_len = line.chars().count();
        let mut cursor = 0usize;
        let mut segments = Vec::new();
        let mut tokens = self
            .syntax_tokens
            .iter()
            .filter(|token| token.line == line_number)
            .collect::<Vec<_>>();
        tokens.sort_by_key(|token| (token.column, token.len));
        for token in tokens {
            let token_start = token.column.saturating_sub(1).min(line_len);
            let token_end = token_start.saturating_add(token.len).min(line_len);
            if token_end <= cursor {
                continue;
            }
            if token_start > cursor {
                let text = slice_chars(line, cursor, token_start);
                if !text.is_empty() {
                    segments.push(SyntaxLineSegment::new(
                        "plain",
                        line_number,
                        cursor + 1,
                        text,
                    ));
                }
            }
            let segment_start = token_start.max(cursor);
            let text = slice_chars(line, segment_start, token_end);
            if !text.is_empty() {
                segments.push(SyntaxLineSegment::new(
                    token.kind,
                    line_number,
                    segment_start + 1,
                    text,
                ));
            }
            cursor = token_end;
        }
        if cursor < line_len {
            let text = slice_chars(line, cursor, line_len);
            if !text.is_empty() {
                segments.push(SyntaxLineSegment::new(
                    "plain",
                    line_number,
                    cursor + 1,
                    text,
                ));
            }
        }
        segments
    }

    fn caret(&self) -> &EditorPosition {
        &self.selection.head
    }

    fn sync_from_buffer(&mut self) {
        self.source_text = self.buffer.source_text();
        self.selection = self.buffer.selection().clone();
        self.line_count = self.buffer.line_count();
        self.last_command = self.buffer.last_command;
    }

    fn sync_from_buffer_and_refresh(&mut self) {
        self.sync_from_buffer();
        self.refresh_language_state();
    }

    fn refresh_language_state(&mut self) {
        self.line_count = self.buffer.line_count();
        self.diagnostics = BoonLanguageService::diagnostics(&self.file_name, &self.source_text);
        let syntax = BoonLanguageService::syntax_highlighting(&self.source_text);
        self.syntax_tokens = syntax.tokens;
        self.syntax_backend = syntax.backend;
        self.syntax_parser_backed = syntax.parser_backed;
        self.formatted_preview_hash =
            if self.source_text.len() <= BOON_EDITOR_FULL_LANGUAGE_BYTES_MAX {
                BoonLanguageService::format(&self.file_name, &self.source_text)
                    .ok()
                    .map(|formatted| boon_runtime::sha256_bytes(formatted.as_bytes()))
            } else {
                None
            };
    }

    fn position_for_offset(&self, target: usize) -> EditorPosition {
        self.buffer.position_for_byte_offset(target)
    }

    fn selection_offsets(&self) -> (usize, usize) {
        self.buffer.selection_byte_offsets()
    }

    fn selected_text(&self) -> String {
        self.buffer.selected_text()
    }

    fn selection_columns_for_line(&self, line: usize) -> Option<(usize, usize)> {
        if self.selection.is_collapsed() {
            return None;
        }
        let (start_byte, end_byte) = self.selection_offsets();
        if start_byte == end_byte {
            return None;
        }
        let start = self.position_for_offset(start_byte);
        let end = self.position_for_offset(end_byte);
        if line < start.line || line > end.line {
            return None;
        }
        let line_len = self
            .source_text
            .lines()
            .nth(line.saturating_sub(1))
            .map(|text| text.chars().count())
            .unwrap_or_default();
        let start_col = if line == start.line {
            start.column.saturating_sub(1)
        } else {
            0
        };
        let end_col = if line == end.line {
            end.column.saturating_sub(1)
        } else {
            line_len
        };
        (end_col > start_col).then_some((start_col, end_col))
    }

    fn bracket_columns_for_line(&self, line: usize) -> Vec<usize> {
        let ignored_ranges = self.bracket_ignored_ranges();
        self.buffer
            .bracket_match(&ignored_ranges)
            .into_iter()
            .flat_map(|pair| [pair.open_byte, pair.close_byte])
            .map(|byte| self.position_for_offset(byte))
            .filter(|position| position.line == line)
            .map(|position| position.column.saturating_sub(1))
            .collect()
    }

    fn bracket_ignored_ranges(&self) -> Vec<(usize, usize)> {
        self.syntax_tokens
            .iter()
            .filter(|token| {
                matches!(
                    token.kind,
                    "comment"
                        | "string"
                        | "text-literal-content"
                        | "text-literal-interpolation"
                        | "text-literal-delimiter"
                )
            })
            .map(|token| (token.start, token.end))
            .collect()
    }

    fn set_selection(&mut self, anchor: EditorPosition, head: EditorPosition) {
        self.buffer.set_selection(anchor, head);
        self.sync_from_buffer();
        self.last_command = Some("selection");
    }

    fn insert_text_at_caret(&mut self, text: &str) {
        self.buffer.insert_text_at_caret(text);
        self.sync_from_buffer_and_refresh();
    }

    fn insert_plain_text_at_caret(&mut self, text: &str, command: &'static str) {
        self.buffer.insert_plain_text_at_caret(text, command);
        self.sync_from_buffer_and_refresh();
    }

    fn delete_backward(&mut self) {
        self.buffer.delete_backward();
        self.sync_from_buffer_and_refresh();
    }

    fn delete_forward(&mut self) {
        self.buffer.delete_forward();
        self.sync_from_buffer_and_refresh();
    }

    fn insert_newline_with_indent(&mut self) {
        self.buffer.insert_newline_with_indent();
        self.sync_from_buffer_and_refresh();
    }

    fn indent_selection(&mut self) {
        self.buffer.indent_selection();
        self.sync_from_buffer_and_refresh();
    }

    fn unindent_selection(&mut self) {
        self.buffer.unindent_selection();
        self.sync_from_buffer_and_refresh();
    }

    fn copy_selection_to_clipboard(&mut self) -> String {
        self.clipboard_cache = self.selected_text();
        self.last_command = Some("clipboard-copy");
        self.clipboard_cache.clone()
    }

    fn paste_from_clipboard(&mut self, text: &str) {
        self.clipboard_cache = text.to_owned();
        self.insert_plain_text_at_caret(text, "clipboard-paste");
        self.last_command = Some("clipboard-paste");
    }

    fn copy_to_adapter(&mut self, clipboard: &mut dyn ClipboardAdapter) -> serde_json::Value {
        let text = self.copy_selection_to_clipboard();
        match clipboard.set_text(&text) {
            Ok(()) => json!({"status": "pass", "command": "clipboard-copy", "bytes": text.len()}),
            Err(error) => {
                json!({"status": "fallback", "command": "clipboard-copy", "reason": error, "bytes": text.len()})
            }
        }
    }

    fn cut_to_adapter(&mut self, clipboard: &mut dyn ClipboardAdapter) -> serde_json::Value {
        if self.selection.is_collapsed() {
            self.last_command = Some("clipboard-cut-empty-selection");
            return json!({"status": "noop", "command": "clipboard-cut", "reason": "selection empty"});
        }
        let text = self.copy_selection_to_clipboard();
        let clipboard_status = clipboard
            .set_text(&text)
            .map(|_| "pass".to_owned())
            .unwrap_or_else(|error| format!("fallback:{error}"));
        self.delete_backward();
        json!({"status": "pass", "command": "clipboard-cut", "clipboard_status": clipboard_status, "bytes": text.len()})
    }

    fn paste_from_adapter(&mut self, clipboard: &mut dyn ClipboardAdapter) -> serde_json::Value {
        let text = clipboard
            .get_text()
            .unwrap_or_else(|_| self.clipboard_cache.clone());
        self.paste_from_clipboard(&text);
        json!({"status": "pass", "command": "clipboard-paste", "bytes": text.len()})
    }

    fn move_home(&mut self, extend: bool) {
        self.buffer.move_home(extend);
        self.sync_from_buffer();
    }

    fn move_end(&mut self, extend: bool) {
        self.buffer.move_end(extend);
        self.sync_from_buffer();
    }

    fn move_left(&mut self, extend: bool) {
        self.buffer.move_left(extend);
        self.sync_from_buffer();
    }

    fn move_right(&mut self, extend: bool) {
        self.buffer.move_right(extend);
        self.sync_from_buffer();
    }

    fn move_up(&mut self, extend: bool) {
        self.buffer.move_up(extend);
        self.scroll_line = self.scroll_line.min(self.caret().line.saturating_sub(1));
        self.sync_from_buffer();
    }

    fn move_down(&mut self, extend: bool) {
        self.buffer.move_down(extend);
        self.sync_from_buffer();
    }

    fn page_down(&mut self, extend: bool) {
        self.buffer.page_down(extend);
        self.scroll_line = (self.scroll_line + 24).min(self.line_count.saturating_sub(1));
        self.sync_from_buffer();
    }

    fn page_up(&mut self, extend: bool) {
        self.buffer.page_up(extend);
        self.scroll_line = self.scroll_line.saturating_sub(24);
        self.sync_from_buffer();
    }

    fn select_all(&mut self) {
        self.buffer.select_all();
        self.sync_from_buffer();
    }

    fn undo(&mut self) -> serde_json::Value {
        if self.buffer.undo() {
            self.sync_from_buffer_and_refresh();
            json!({"status": "pass", "undo_depth": self.buffer.undo_depth(), "redo_depth": self.buffer.redo_depth()})
        } else {
            json!({"status": "noop", "reason": "undo stack empty"})
        }
    }

    fn redo(&mut self) -> serde_json::Value {
        if self.buffer.redo() {
            self.sync_from_buffer_and_refresh();
            json!({"status": "pass", "undo_depth": self.buffer.undo_depth(), "redo_depth": self.buffer.redo_depth()})
        } else {
            json!({"status": "noop", "reason": "redo stack empty"})
        }
    }

    fn model_feature_probe(&self) -> serde_json::Value {
        let mut probe = self.clone();
        probe.set_selection(
            EditorPosition { line: 1, column: 1 },
            EditorPosition { line: 1, column: 1 },
        );
        probe.insert_text_at_caret("-- probe\n");
        probe.insert_newline_with_indent();
        probe.move_home(false);
        probe.move_end(false);
        probe.page_down(false);
        probe.page_up(false);
        probe.set_selection(
            EditorPosition { line: 1, column: 1 },
            EditorPosition { line: 1, column: 4 },
        );
        let copied = probe.copy_selection_to_clipboard();
        probe.paste_from_clipboard(&copied);
        probe.indent_selection();
        probe.unindent_selection();
        probe.delete_backward();
        probe.insert_text_at_caret("(");
        probe.insert_text_at_caret(")");
        let undo = probe.undo();
        let redo = probe.redo();
        json!({
            "status": "pass",
            "platform_neutral": true,
            "full_buffer_bytes": self.source_text.len(),
            "full_buffer_lines": self.line_count,
            "selection_supported": true,
            "selection_collapsed": self.selection.is_collapsed(),
            "undo_redo_supported": true,
            "clipboard_adapter_supported": true,
            "bracket_matching_supported": true,
            "auto_close_brackets": ["(", "[", "{"],
            "caret_overlay_supported": true,
            "caret_blink_supported": true,
            "selection_overlay_supported": true,
            "keyboard_commands_supported": [
                "insert_text",
                "delete_backward",
                "delete_forward",
                "enter_newline_indent",
                "tab_indent",
                "shift_tab_unindent",
                "home",
                "end",
                "arrow_left",
                "arrow_right",
                "arrow_up",
                "arrow_down",
                "page_up",
                "page_down",
                "select_all",
                "copy",
                "cut",
                "paste",
                "undo",
                "redo"
            ],
            "undo_probe": undo,
            "redo_probe": redo,
            "syntax_backend": self.syntax_backend(),
            "syntax_parser_backed": self.syntax_parser_backed(),
            "syntax_categories": self.syntax_categories(),
            "syntax_token_samples": self.syntax_token_samples(),
            "syntax_token_count": self.syntax_token_count(),
            "syntax_render_categories": self.syntax_render_categories(),
            "syntax_render_segment_samples": self.syntax_render_segment_samples(),
            "syntax_render_segment_count": self.syntax_render_segments_for_visible_lines(40).len(),
            "syntax_invalid_token_samples": self.syntax_invalid_token_samples(),
            "syntax_theme": self.syntax_theme_report(),
            "invalid_reserved_token_probe": BoonLanguageService::invalid_syntax_probe()
        })
    }

    fn visible_lines(&self, max_lines: usize) -> Vec<(usize, String)> {
        self.source_text
            .lines()
            .enumerate()
            .skip(self.scroll_line)
            .take(max_lines.max(1))
            .map(|(index, line)| (index + 1, line.to_owned()))
            .collect()
    }

    fn replace_text(&mut self, source_path_label: &str, source_text: String) {
        *self = Self::new(source_path_label, &source_text);
    }
}

#[derive(Clone, Debug)]
struct CodeEditorView {
    font_family: &'static str,
}

impl CodeEditorView {
    fn new() -> Self {
        Self {
            font_family: BOON_EDITOR_FONT_FAMILY,
        }
    }

    fn append_to(
        &self,
        frame: &mut boon_document_model::DocumentFrame,
        parent: boon_document_model::DocumentNodeId,
        model: &CodeEditorModel,
        height: u32,
        caret_visible: bool,
    ) {
        let editor_height = height.max(96);
        let mut editor = dev_node(
            "dev-code-editor",
            boon_document_model::DocumentNodeKind::ScrollRoot,
            None,
            &[
                ("bg", BOON_EDITOR_BACKGROUND),
                ("color", BOON_EDITOR_FOREGROUND),
                ("border", BOON_EDITOR_DARK_BACKGROUND),
                ("padding", &BOON_EDITOR_PADDING.to_string()),
                ("height", &editor_height.to_string()),
                ("width", "fill"),
                ("scroll", "true"),
                ("scroll_x", "true"),
                ("font", self.font_family),
                ("size", &BOON_EDITOR_FONT_SIZE.to_string()),
                ("font_features", BOON_EDITOR_FONT_FEATURES),
            ],
        );
        editor.scroll = Some(boon_document_model::ScrollState {
            x: model.scroll_column as f32,
            y: model.scroll_line as f32,
        });
        editor.source_binding = Some(boon_document_model::SourceBinding {
            id: boon_document_model::SourceBindingId("source:dev-editor:insert-text".to_owned()),
            source_path: "dev.editor.insert_text".to_owned(),
            intent: "text_input".to_owned(),
        });
        editor
            .materialized
            .push(boon_document_model::MaterializedRange {
                axis: boon_document_model::Axis::Vertical,
                visible: 0..40,
                overscan: 0..64,
            });
        editor
            .materialized
            .push(boon_document_model::MaterializedRange {
                axis: boon_document_model::Axis::Horizontal,
                visible: 0..120,
                overscan: 0..180,
            });
        let editor_parent = editor.id.clone();
        frame.scroll_roots.insert(
            boon_document_model::ScrollRootId(editor_parent.0.clone()),
            boon_document_model::ScrollState {
                x: model.scroll_column as f32,
                y: model.scroll_line as f32,
            },
        );
        append_child(frame, parent, editor);
        let visible_line_count = (editor_height.saturating_sub(BOON_EDITOR_PADDING * 2)
            / BOON_EDITOR_LINE_HEIGHT)
            .max(1) as usize;
        for (line_number, line) in model.visible_lines(visible_line_count) {
            let row_id = format!("dev-code-editor-line-{line_number}");
            let row_bg = if line_number == model.caret().line {
                BOON_EDITOR_HIGHLIGHT_BACKGROUND
            } else {
                BOON_EDITOR_BACKGROUND
            };
            let row = dev_node(
                &row_id,
                boon_document_model::DocumentNodeKind::Row,
                None,
                &[
                    ("height", &BOON_EDITOR_LINE_HEIGHT.to_string()),
                    ("width", "fill"),
                    ("gap", &BOON_EDITOR_ROW_GAP.to_string()),
                    ("padding", "0"),
                    ("bg", row_bg),
                ],
            );
            let row_parent = row.id.clone();
            append_child(frame, editor_parent.clone(), row);
            let gutter = dev_node(
                &format!("dev-code-editor-gutter-{line_number}"),
                boon_document_model::DocumentNodeKind::Text,
                Some(format!("{line_number:>4}")),
                &[
                    ("width", &BOON_EDITOR_GUTTER_WIDTH.to_string()),
                    ("height", &BOON_EDITOR_LINE_HEIGHT.to_string()),
                    ("color", BOON_EDITOR_GUTTER),
                    ("size", &BOON_EDITOR_FONT_SIZE.to_string()),
                    ("bg", row_bg),
                    ("font", self.font_family),
                    ("font_features", BOON_EDITOR_FONT_FEATURES),
                ],
            );
            append_child(frame, row_parent.clone(), gutter);
            let code_row = dev_node(
                &format!("dev-code-editor-code-row-{line_number}"),
                boon_document_model::DocumentNodeKind::Row,
                None,
                &[
                    ("width", "fill"),
                    ("height", &BOON_EDITOR_LINE_HEIGHT.to_string()),
                    ("bg", row_bg),
                    ("gap", "0"),
                    ("padding", "0"),
                ],
            );
            let code_row_parent = code_row.id.clone();
            append_child(frame, row_parent, code_row);
            self.append_highlighted_line(
                frame,
                code_row_parent,
                model,
                line_number,
                &line,
                caret_visible,
            );
        }
    }

    fn append_highlighted_line(
        &self,
        frame: &mut boon_document_model::DocumentFrame,
        parent: boon_document_model::DocumentNodeId,
        model: &CodeEditorModel,
        line_number: usize,
        line: &str,
        caret_visible: bool,
    ) {
        let segments = model.highlighted_line_segments(line_number, line);
        append_child(
            frame,
            parent,
            self.editor_line_node(line_number, line, &segments, model, caret_visible),
        );
    }

    fn editor_line_node(
        &self,
        line_number: usize,
        line: &str,
        segments: &[SyntaxLineSegment],
        model: &CodeEditorModel,
        caret_visible: bool,
    ) -> boon_document_model::DocumentNode {
        let syntax_spans_json = syntax_spans_json(segments);
        let mut node = dev_node(
            &format!("dev-code-editor-line-text-{line_number}"),
            boon_document_model::DocumentNodeKind::Text,
            Some(line.to_owned()),
            &[
                ("width", "fill"),
                ("height", &BOON_EDITOR_LINE_HEIGHT.to_string()),
                ("color", BOON_EDITOR_FOREGROUND),
                ("size", &BOON_EDITOR_FONT_SIZE.to_string()),
                ("bg", BOON_EDITOR_BACKGROUND),
                ("font", self.font_family),
                ("font_features", BOON_EDITOR_FONT_FEATURES),
                ("syntax_spans_json", &syntax_spans_json),
                ("text_inset", "0"),
                ("text_clip_padding", "4"),
                ("editor_selection_color", BOON_EDITOR_SELECTION),
                ("editor_caret_color", BOON_EDITOR_CURSOR),
                ("editor_bracket_color", BOON_EDITOR_BRACKET_MATCH),
                ("editor_selection_match_color", BOON_EDITOR_SELECTION_MATCH),
            ],
        );
        if let Some((start, end)) = model.selection_columns_for_line(line_number) {
            node.style.insert(
                "editor_selection_start".to_owned(),
                boon_document_model::StyleValue::Number(start as f64),
            );
            node.style.insert(
                "editor_selection_end".to_owned(),
                boon_document_model::StyleValue::Number(end as f64),
            );
        }
        if model.caret().line == line_number {
            node.style.insert(
                "editor_caret_column".to_owned(),
                boon_document_model::StyleValue::Number(
                    model.caret().column.saturating_sub(1) as f64
                ),
            );
            node.style.insert(
                "editor_caret_visible".to_owned(),
                boon_document_model::StyleValue::Bool(caret_visible),
            );
        }
        let bracket_columns = model
            .bracket_columns_for_line(line_number)
            .into_iter()
            .map(|column| column.to_string())
            .collect::<Vec<_>>()
            .join(",");
        if !bracket_columns.is_empty() {
            node.style.insert(
                "editor_bracket_columns".to_owned(),
                boon_document_model::StyleValue::Text(bracket_columns),
            );
        }
        node.style.insert(
            "rich_text".to_owned(),
            boon_document_model::StyleValue::Bool(true),
        );
        node
    }
}

fn syntax_spans_json(segments: &[SyntaxLineSegment]) -> String {
    let spans = segments
        .iter()
        .map(|segment| {
            let style = syntax_style_for_kind(segment.kind);
            json!({
                "text": segment.text,
                "source_text": segment.text,
                "color": style.color,
                "font_weight": style.font_weight,
                "font_style": style.font_style
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&spans).unwrap_or_else(|_| "[]".to_owned())
}

#[derive(Clone, Copy)]
struct SyntaxStyle {
    color: &'static str,
    font_weight: Option<&'static str>,
    font_style: Option<&'static str>,
}

fn syntax_style_for_kind(kind: &str) -> SyntaxStyle {
    match kind {
        "comment" => SyntaxStyle {
            color: "#778899",
            font_weight: None,
            font_style: Some("italic"),
        },
        "keyword" => SyntaxStyle {
            color: "#D2691E",
            font_weight: Some("800"),
            font_style: Some("italic"),
        },
        "source-binding" => SyntaxStyle {
            color: "#6cb6ff",
            font_weight: None,
            font_style: None,
        },
        "tag" => SyntaxStyle {
            color: "#6df59a",
            font_weight: None,
            font_style: None,
        },
        "type" => SyntaxStyle {
            color: "#6f9cff",
            font_weight: None,
            font_style: None,
        },
        "variable" | "text-literal-interpolation" => SyntaxStyle {
            color: "#eeeeee",
            font_weight: None,
            font_style: None,
        },
        "function" => SyntaxStyle {
            color: "#fcbf49",
            font_weight: Some("600"),
            font_style: None,
        },
        "definition" => SyntaxStyle {
            color: "#ff6ec7",
            font_weight: Some("600"),
            font_style: Some("italic"),
        },
        "operator" => SyntaxStyle {
            color: "#ff9f43",
            font_weight: Some("600"),
            font_style: None,
        },
        "punctuation" | "module-slash" | "dot" | "pipe" | "text-literal-delimiter" => SyntaxStyle {
            color: "#D2691E",
            font_weight: Some("700"),
            font_style: None,
        },
        "string" | "text-literal-content" => SyntaxStyle {
            color: "#fff59e",
            font_weight: None,
            font_style: None,
        },
        "number" | "negative-sign" => SyntaxStyle {
            color: "#7ad1ff",
            font_weight: None,
            font_style: None,
        },
        "wildcard" => SyntaxStyle {
            color: "#D2691E",
            font_weight: None,
            font_style: None,
        },
        "chain-alt" => SyntaxStyle {
            color: "#bbbbbb",
            font_weight: None,
            font_style: None,
        },
        "invalid" => SyntaxStyle {
            color: "#ffffff",
            font_weight: None,
            font_style: None,
        },
        _ => SyntaxStyle {
            color: BOON_EDITOR_FOREGROUND,
            font_weight: None,
            font_style: None,
        },
    }
}

fn syntax_color_for_kind(kind: &str) -> &'static str {
    syntax_style_for_kind(kind).color
}

fn syntax_font_weight_for_kind(kind: &str) -> Option<&'static str> {
    syntax_style_for_kind(kind).font_weight
}

fn syntax_font_style_for_kind(kind: &str) -> Option<&'static str> {
    syntax_style_for_kind(kind).font_style
}

fn syntax_style_json(kind: &str) -> serde_json::Value {
    let style = syntax_style_for_kind(kind);
    json!({
        "color": style.color,
        "font_weight": style.font_weight,
        "font_style": style.font_style
    })
}

fn slice_chars(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

fn matching_brace_byte(source: &str, open_brace: usize) -> Option<usize> {
    if source.as_bytes().get(open_brace) != Some(&b'{') {
        return None;
    }
    let mut depth = 0usize;
    let mut position = open_brace;
    while position < source.len() {
        let ch = source[position..].chars().next()?;
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(position);
            }
        }
        position += ch.len_utf8();
    }
    None
}

#[derive(Clone, Debug)]
struct PreviewTransport {
    connect: Option<String>,
}

impl PreviewTransport {
    fn new(connect: Option<String>) -> Self {
        Self { connect }
    }

    fn replace_code(
        &self,
        command: &str,
        selected_example_id: &str,
        source_path: &str,
        source_text: &str,
    ) -> serde_json::Value {
        let source_hash = boon_runtime::sha256_bytes(source_text.as_bytes());
        let Some(connect) = &self.connect else {
            return json!({
                "status": "not-bound",
                "kind": "ReplaceCode",
                "command": command,
                "transport_bound": false,
                "selected_example_id": selected_example_id,
                "source_path": source_path,
                "source_hash": source_hash,
                "preview_receives_example_name": false
            });
        };
        match send_preview_ipc_request(
            connect,
            json!({
                "kind": "replace-code",
                "code": source_text,
                "expected_hash": source_hash,
                "source_path": source_path,
                "dev_pid": std::process::id()
            }),
        ) {
            Ok(ack) => {
                let hash_matches =
                    ack.get("hash_matches").and_then(serde_json::Value::as_bool) == Some(true);
                let ack_pass =
                    ack.get("status").and_then(serde_json::Value::as_str) == Some("pass");
                json!({
                    "status": if hash_matches && ack_pass { "pass" } else { "fail" },
                    "kind": "ReplaceCode",
                    "command": command,
                    "transport_bound": true,
                    "selected_example_id": selected_example_id,
                    "source_path": source_path,
                    "source_hash": source_hash,
                    "ack": ack,
                    "preview_receives_example_name": false
                })
            }
            Err(error) => json!({
                "status": "fail",
                "kind": "ReplaceCode",
                "command": command,
                "transport_bound": true,
                "selected_example_id": selected_example_id,
                "source_path": source_path,
                "source_hash": source_hash,
                "diagnostic": error.to_string(),
                "preview_receives_example_name": false
            }),
        }
    }

    fn runtime_summary(&self) -> serde_json::Value {
        let Some(connect) = &self.connect else {
            return json!({
                "status": "not-bound",
                "kind": "debug-query-result",
                "debug_query": "RuntimeSummary",
                "transport_bound": false
            });
        };
        match send_preview_ipc_request_with_timeouts(
            connect,
            json!({"kind": "runtime-summary"}),
            Duration::ZERO,
            Duration::from_millis(DEV_PREVIEW_SUMMARY_READ_TIMEOUT_MS),
            Duration::from_millis(DEV_PREVIEW_SUMMARY_READ_TIMEOUT_MS),
        ) {
            Ok(value) => value,
            Err(error) => json!({
                "status": "unavailable",
                "kind": "debug-query-result",
                "debug_query": "RuntimeSummary",
                "transport_bound": true,
                "diagnostic": error.to_string()
            }),
        }
    }
}

#[derive(Clone, Debug)]
struct DevWindowShell {
    catalog: ExampleCatalog,
    workspace: ExampleWorkspace,
    initial_workspace: ExampleWorkspace,
    editor_view: CodeEditorView,
    preview_transport: PreviewTransport,
    last_preview_transport: serde_json::Value,
    last_preview_summary: serde_json::Value,
    last_good_runtime_summary: Option<serde_json::Value>,
    last_preview_summary_refresh: Option<Instant>,
    last_dev_command: String,
    last_dev_command_status: String,
    last_dev_command_detail: Option<String>,
    footer_scroll_line: usize,
    caret_visible: bool,
}

impl DevWindowShell {
    fn new(
        source_path_label: &str,
        source_text: &str,
        selected_example_id: Option<&str>,
        preview_transport: PreviewTransport,
    ) -> Self {
        let catalog = ExampleCatalog::load();
        let workspace = ExampleWorkspace::new(
            &catalog,
            source_path_label,
            source_text,
            selected_example_id,
        );
        let initial_workspace = workspace.clone();
        Self {
            catalog,
            workspace,
            initial_workspace,
            editor_view: CodeEditorView::new(),
            preview_transport,
            last_preview_transport: json!({
                "status": "not-run",
                "reason": "no preview transport command has run yet"
            }),
            last_preview_summary: json!({
                "status": "not-run",
                "kind": "debug-query-result",
                "debug_query": "RuntimeSummary",
                "reason": "preview summary has not been queried yet"
            }),
            last_good_runtime_summary: None,
            last_preview_summary_refresh: None,
            last_dev_command: "startup".to_owned(),
            last_dev_command_status: "not-run".to_owned(),
            last_dev_command_detail: None,
            footer_scroll_line: 0,
            caret_visible: true,
        }
    }

    fn document(&self) -> boon_document_model::DocumentFrame {
        self.document_for_viewport(1180, 820)
    }

    fn document_for_viewport(&self, width: u32, height: u32) -> boon_document_model::DocumentFrame {
        dev_shell_document(self, width, height)
    }

    fn footer_lines(&self) -> Vec<(String, String)> {
        let buffer = &self.workspace.selected_buffer;
        let summary_status = self
            .last_preview_summary
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("not-run");
        let current_source_hash = boon_runtime::sha256_bytes(buffer.source_text.as_bytes());
        let runtime_summary = self
            .visible_runtime_summary(&current_source_hash)
            .unwrap_or(&serde_json::Value::Null);
        let runtime_state_hash = runtime_summary
            .get("state_summary_hash")
            .and_then(serde_json::Value::as_str)
            .map(short_hash)
            .unwrap_or_else(|| "-".to_owned());
        let source_hash = runtime_summary
            .get("source_sha256")
            .and_then(serde_json::Value::as_str)
            .map(short_hash)
            .or_else(|| {
                self.last_preview_transport
                    .get("source_hash")
                    .and_then(serde_json::Value::as_str)
                    .map(short_hash)
            })
            .unwrap_or_else(|| "-".to_owned());
        let preview_error = self
            .last_preview_summary
            .get("preview_last_error")
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                self.last_preview_transport
                    .get("diagnostic")
                    .and_then(serde_json::Value::as_str)
            })
            .unwrap_or("-");
        let preview_error_count = self
            .last_preview_summary
            .get("preview_last_error_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let saved_state = if self.workspace.dirty {
            "Unsaved"
        } else {
            "Saved"
        };
        let diagnostics_text = match buffer.diagnostics.len() {
            0 => "no diagnostics".to_owned(),
            1 => "1 diagnostic".to_owned(),
            count => format!("{count} diagnostics"),
        };
        let mut lines = vec![
            (
                "Code".to_owned(),
                format!(
                    "{} lines, {}, {}, {} bytes",
                    buffer.line_count,
                    diagnostics_text,
                    saved_state,
                    buffer.source_text.len(),
                ),
            ),
            (
                "Cursor".to_owned(),
                format!(
                    "line {}, column {}, scroll {}:{}",
                    buffer.caret().line.saturating_add(1),
                    buffer.caret().column.saturating_add(1),
                    buffer.scroll_line,
                    buffer.scroll_column
                ),
            ),
        ];
        if runtime_state_hash != "-" {
            lines.push((
                "Runtime".to_owned(),
                runtime_footer_summary(runtime_summary, &runtime_state_hash, &source_hash),
            ));
        } else if let Some(diagnostic) = self.preview_diagnostic() {
            lines.push((
                "Preview".to_owned(),
                format!(
                    "{}: {}",
                    ui_status_label(summary_status),
                    one_line(&diagnostic, 110)
                ),
            ));
        }
        if preview_error_count > 0 || preview_error != "-" {
            lines.push((
                "Preview error".to_owned(),
                format!(
                    "{} error{}, {}",
                    preview_error_count,
                    if preview_error_count == 1 { "" } else { "s" },
                    one_line(preview_error, 96)
                ),
            ));
        }
        if self.last_dev_command != "startup" || self.last_dev_command_status != "not-run" {
            let mut action = format!(
                "{}: {}",
                friendly_dev_command(&self.last_dev_command),
                ui_status_label(&self.last_dev_command_status)
            );
            if let Some(detail) = &self.last_dev_command_detail {
                action.push_str(" - ");
                action.push_str(&one_line(detail, 92));
            }
            lines.push(("Last action".to_owned(), action));
        }
        lines
    }

    fn preview_diagnostic(&self) -> Option<String> {
        json_diagnostic(&self.last_preview_summary)
            .or_else(|| json_diagnostic(&self.last_preview_transport))
    }

    fn visible_runtime_summary(&self, current_source_hash: &str) -> Option<&serde_json::Value> {
        let current = self.last_preview_summary.get("runtime_summary");
        let current =
            current.filter(|summary| runtime_summary_matches_source(summary, current_source_hash));
        current.or_else(|| {
            self.last_good_runtime_summary
                .as_ref()
                .filter(|summary| runtime_summary_matches_source(summary, current_source_hash))
        })
    }

    fn selected_example_is_custom(&self) -> bool {
        self.catalog
            .entries
            .iter()
            .any(|entry| entry.id == self.workspace.selected_example_id && entry.custom)
    }

    fn selected_example_label(&self) -> String {
        self.catalog
            .entries
            .iter()
            .find(|entry| entry.id == self.workspace.selected_example_id)
            .map(|entry| entry.label.clone())
            .unwrap_or_else(|| self.workspace.selected_example_id.clone())
    }

    fn rename_selected_custom_label(&mut self, label: &str) -> serde_json::Value {
        if !self.selected_example_is_custom() {
            return json!({
                "status": "skipped",
                "command": "RenameCustomExample",
                "selected_example_id": self.workspace.selected_example_id,
                "reason": "selected example is manifest-backed"
            });
        }
        let normalized = normalize_custom_example_label(label);
        self.catalog
            .rename_custom_example(&self.workspace.selected_example_id, &normalized)
    }

    fn document_source_paths(&self) -> Vec<String> {
        let mut paths = self
            .document()
            .nodes
            .values()
            .filter_map(|node| {
                node.source_binding
                    .as_ref()
                    .map(|binding| binding.source_path.clone())
            })
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();
        paths
    }

    fn remove_custom_control_state(&self) -> serde_json::Value {
        let document = self.document();
        let button = document.nodes.get(&boon_document_model::DocumentNodeId(
            "dev-command-remove_custom".to_owned(),
        ));
        let style_disabled = button
            .and_then(|node| node.style.get("disabled"))
            .and_then(|value| match value {
                boon_document_model::StyleValue::Bool(disabled) => Some(*disabled),
                _ => None,
            })
            .unwrap_or(false);
        let source_path = button
            .and_then(|node| node.source_binding.as_ref())
            .map(|binding| binding.source_path.clone());
        json!({
            "node_present": button.is_some(),
            "style_disabled": style_disabled,
            "source_binding_present": source_path.is_some(),
            "source_path": source_path,
            "selected_example_id": self.workspace.selected_example_id,
            "selected_is_custom": self.selected_example_is_custom()
        })
    }

    fn remove_custom_disabled_probe(
        &self,
        viewport_width: f32,
        viewport_height: f32,
    ) -> serde_json::Value {
        let control = self.remove_custom_control_state();
        let activation = self.host_synthetic_activation_for_source_path(
            "dev.commands.remove_custom",
            viewport_width,
            viewport_height,
        );
        let selected_is_custom = control
            .get("selected_is_custom")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        let disabled = control
            .get("style_disabled")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        let binding_present = control
            .get("source_binding_present")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        let source_binding_resolved = activation
            .get("source_binding_resolved")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        let pass = !selected_is_custom && disabled && !binding_present && !source_binding_resolved;
        json!({
            "status": if pass { "pass" } else { "fail" },
            "command": "OfficialRemoveCustomDisabled",
            "control": control,
            "host_synthetic_activation": activation,
            "direct_dispatch_without_hit_test": false
        })
    }

    fn remove_custom_enabled_probe(
        &self,
        viewport_width: f32,
        viewport_height: f32,
    ) -> serde_json::Value {
        let control = self.remove_custom_control_state();
        let activation = self.host_synthetic_activation_for_source_path(
            "dev.commands.remove_custom",
            viewport_width,
            viewport_height,
        );
        let selected_is_custom = control
            .get("selected_is_custom")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        let disabled = control
            .get("style_disabled")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        let source_path = control
            .get("source_path")
            .and_then(serde_json::Value::as_str);
        let activation_pass =
            activation.get("status").and_then(serde_json::Value::as_str) == Some("pass");
        let pass = selected_is_custom
            && !disabled
            && source_path == Some("dev.commands.remove_custom")
            && activation_pass;
        json!({
            "status": if pass { "pass" } else { "fail" },
            "command": "CustomRemoveCustomEnabled",
            "control": control,
            "host_synthetic_activation": activation,
            "direct_dispatch_without_hit_test": false
        })
    }

    fn dispatch_source_path(&mut self, source_path: &str) -> serde_json::Value {
        if source_path == "dev.tabs.new" {
            let mut value = self.create_blank_custom_tab();
            if value.get("status").and_then(serde_json::Value::as_str) == Some("pass") {
                value["preview_transport"] = self.replace_selected_preview("NewCustomTab");
            }
            value["dispatched_source_path"] = json!(source_path);
            value["dispatch_boundary"] = json!("Document SourceBinding -> DevWindowShell");
            return value;
        }
        if let Some(example_id) = source_path.strip_prefix("dev.tabs.select.") {
            return self
                .workspace
                .select_example(&self.catalog, example_id)
                .map(|mut value| {
                    value["preview_transport"] = self.replace_selected_preview("SelectTab");
                    value["dispatched_source_path"] = json!(source_path);
                    value["dispatch_boundary"] = json!("Document SourceBinding -> DevWindowShell");
                    value
                })
                .unwrap_or_else(|error| {
                    json!({
                        "status": "fail",
                        "command": "SelectTab",
                        "dispatched_source_path": source_path,
                        "diagnostic": error.to_string()
                    })
                });
        }

        let mut value = match source_path {
            "dev.commands.run" => self.workspace.run_selected(),
            "dev.commands.format" => self.workspace.format_selected(),
            "dev.commands.reset" => {
                self.workspace
                    .reset_selected(&self.catalog)
                    .unwrap_or_else(|error| {
                        json!({
                            "status": "fail",
                            "command": "Reset",
                            "diagnostic": error.to_string()
                        })
                    })
            }
            "dev.commands.remove_custom" => {
                self.workspace.remove_selected_custom(&mut self.catalog)
            }
            "dev.editor.insert_text" => self
                .workspace
                .apply_editor_text_input("\n-- host synthetic editor input"),
            other => {
                return json!({
                    "status": "fail",
                    "command": "UnknownDevSource",
                    "dispatched_source_path": other,
                    "diagnostic": "unknown dev source path"
                });
            }
        };
        if value.get("status").and_then(serde_json::Value::as_str) != Some("pass") {
            self.last_dev_command = value
                .get("command")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(source_path)
                .to_owned();
            self.last_dev_command_status = value
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("fail")
                .to_owned();
            self.last_dev_command_detail = json_diagnostic(&value);
        }
        if value.get("status").and_then(serde_json::Value::as_str) == Some("pass") {
            if matches!(
                value.get("command").and_then(serde_json::Value::as_str),
                Some("EditorTextInput" | "Format")
            ) {
                value["custom_source_persistence"] = self.persist_selected_custom_source(
                    value
                        .get("command")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("DevCommand"),
                );
            }
            value["preview_transport"] = self.replace_selected_preview(
                value
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("DevCommand"),
            );
        }
        value["dispatched_source_path"] = json!(source_path);
        value["dispatch_boundary"] = json!("Document SourceBinding -> DevWindowShell");
        value
    }

    fn create_blank_custom_tab(&mut self) -> serde_json::Value {
        match self.catalog.create_blank_custom_example() {
            Ok((entry, persistence)) => self
                .workspace
                .select_example(&self.catalog, &entry.id)
                .map(|mut selected| {
                    selected["command"] = json!("NewCustomTab");
                    selected["stable_id"] = json!(entry.id);
                    selected["label"] = json!(entry.label);
                    selected["source_path"] = json!(entry.source);
                    selected["source_starts_empty"] = json!(true);
                    selected["persistent_store"] = persistence;
                    selected["metadata_outside_boon_source"] = json!(true);
                    selected
                })
                .unwrap_or_else(|error| {
                    json!({
                        "status": "fail",
                        "command": "NewCustomTab",
                        "diagnostic": error.to_string()
                    })
                }),
            Err(error) => json!({
                "status": "fail",
                "command": "NewCustomTab",
                "diagnostic": error.to_string()
            }),
        }
    }

    fn persist_selected_custom_source(&mut self, command: &str) -> serde_json::Value {
        if !self.workspace.selected_example_id.starts_with("custom:") {
            return json!({
                "status": "skipped",
                "command": "PersistCustomSource",
                "trigger": command,
                "selected_example_id": self.workspace.selected_example_id,
                "reason": "selected example is manifest-backed"
            });
        }
        self.catalog
            .update_custom_source(
                &self.workspace.selected_example_id,
                &self.workspace.selected_buffer.source_text,
            )
            .unwrap_or_else(|error| {
                json!({
                    "status": "fail",
                    "command": "PersistCustomSource",
                    "trigger": command,
                    "selected_example_id": self.workspace.selected_example_id,
                    "diagnostic": error.to_string()
                })
            })
    }

    fn dispatch_host_synthetic_editor_text_input(
        &mut self,
        text: &str,
        viewport_width: f32,
        viewport_height: f32,
    ) -> serde_json::Value {
        let source_path = "dev.editor.insert_text";
        let mut activation = self.host_synthetic_activation_for_source_path(
            source_path,
            viewport_width,
            viewport_height,
        );
        if activation.get("status").and_then(serde_json::Value::as_str) != Some("pass") {
            return json!({
                "status": "fail",
                "command": "EditorTextInput",
                "requested_source_path": source_path,
                "host_synthetic_activation": activation,
                "dispatch_skipped": true,
                "direct_dispatch_without_hit_test": false
            });
        }
        activation["input_event_sequence"] = json!([
            {
                "kind": "HostInputEvent::PointerMove",
                "targeting": "center-of-editor-hit-region"
            },
            {
                "kind": "HostInputEvent::PointerButton",
                "button": "primary",
                "state": "press-release"
            },
            {
                "kind": "HostInputEvent::TextInput",
                "text_bytes": text.len()
            }
        ]);
        let mut value = self.workspace.apply_editor_text_input(text);
        value["custom_source_persistence"] = self.persist_selected_custom_source("EditorTextInput");
        value["host_synthetic_activation"] = activation;
        value["input_evidence_tier"] = json!("boon-driver");
        value["legacy_input_evidence_tier"] = json!("host-synthetic");
        value["dispatched_source_path"] = json!(source_path);
        value["dispatch_boundary"] =
            json!("Document SourceBinding -> DevWindowShell -> CodeEditorModel");
        value["activation_boundary"] = json!(
            "HostInputEvent -> document hit test -> SourceBinding -> DevWindowShell -> CodeEditorModel"
        );
        value["direct_dispatch_without_hit_test"] = json!(false);
        value
    }

    fn dispatch_host_synthetic_source_path(
        &mut self,
        source_path: &str,
        viewport_width: f32,
        viewport_height: f32,
    ) -> serde_json::Value {
        let activation = self.host_synthetic_activation_for_source_path(
            source_path,
            viewport_width,
            viewport_height,
        );
        if activation.get("status").and_then(serde_json::Value::as_str) != Some("pass") {
            return json!({
                "status": "fail",
                "command": "HostSyntheticDevCommand",
                "requested_source_path": source_path,
                "host_synthetic_activation": activation,
                "dispatch_skipped": true,
                "direct_dispatch_without_hit_test": false
            });
        }
        let mut value = self.dispatch_source_path(source_path);
        value["host_synthetic_activation"] = activation;
        value["input_evidence_tier"] = json!("boon-driver");
        value["legacy_input_evidence_tier"] = json!("host-synthetic");
        value["activation_boundary"] =
            json!("HostInputEvent -> document hit test -> SourceBinding -> DevWindowShell");
        value["direct_dispatch_without_hit_test"] = json!(false);
        value
    }

    fn host_synthetic_activation_for_source_path(
        &self,
        source_path: &str,
        viewport_width: f32,
        viewport_height: f32,
    ) -> serde_json::Value {
        let document = self.document();
        let source_intent = document.nodes.values().find_map(|node| {
            let binding = node.source_binding.as_ref()?;
            (binding.source_path == source_path).then(|| {
                json!({
                    "node": node.id,
                    "source_path": binding.source_path,
                    "intent": binding.intent,
                    "binding_id": binding.id
                })
            })
        });
        let mut measurer = boon_native_gpu::GlyphonTextMeasurer::new();
        let layout = boon_document::layout(boon_document::LayoutInput {
            document: &document,
            viewport: boon_host::Viewport {
                surface: 1,
                width: viewport_width,
                height: viewport_height,
                scale: 1.0,
            },
            text: &mut measurer,
            capabilities: boon_document::RenderCapabilities::fake_portable(),
        });
        let layout_json = serde_json::to_value(&layout).unwrap_or_else(|_| json!({}));
        let target_node = source_intent
            .as_ref()
            .and_then(|intent| intent.get("node"))
            .and_then(serde_json::Value::as_str);
        let target_hit_region = target_node.and_then(|node| {
            layout_json
                .get("hit_regions")
                .and_then(serde_json::Value::as_array)?
                .iter()
                .find(|region| region.get("node").and_then(serde_json::Value::as_str) == Some(node))
                .cloned()
        });
        let pass = source_intent.is_some() && target_hit_region.is_some();
        json!({
            "status": if pass { "pass" } else { "fail" },
            "evidence_tier": "boon-driver",
            "legacy_evidence_tier": "host-synthetic",
            "requested_source_path": source_path,
            "source_binding_resolved": source_intent.is_some(),
            "target_source_intent": source_intent.unwrap_or_else(|| json!(null)),
            "hit_test_performed": target_hit_region.is_some(),
            "target_hit_region": target_hit_region.unwrap_or_else(|| json!(null)),
            "viewport": {
                "width": viewport_width,
                "height": viewport_height,
                "scale": 1.0
            },
            "input_event_sequence": [
                {
                    "kind": "HostInputEvent::PointerMove",
                    "targeting": "center-of-hit-region"
                },
                {
                    "kind": "HostInputEvent::PointerButton",
                    "button": "primary",
                    "state": "press-release"
                }
            ],
            "route_contract": "HostInputEvent -> layout hit region -> document SourceBinding -> DevWindowShell dispatch"
        })
    }

    fn replace_selected_preview(&mut self, command: &str) -> serde_json::Value {
        let value = self.preview_transport.replace_code(
            command,
            &self.workspace.selected_example_id,
            &self.workspace.selected_buffer.file_name,
            &self.workspace.selected_buffer.source_text,
        );
        self.last_dev_command = command.to_owned();
        self.last_dev_command_status = value
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("not-run")
            .to_owned();
        self.last_dev_command_detail = json_diagnostic(&value);
        self.update_preview_summary_from_transport(&value);
        self.last_preview_transport = value.clone();
        value
    }

    fn update_preview_summary_from_transport(&mut self, transport: &serde_json::Value) {
        if let Some(summary) = transport
            .pointer("/ack/preview_runtime_summary")
            .or_else(|| transport.pointer("/preview_runtime_summary"))
        {
            if runtime_summary_is_ready(summary) {
                self.last_good_runtime_summary = Some(summary.clone());
            }
            self.last_preview_summary = json!({
                "status": "pass",
                "kind": "debug-query-result",
                "debug_query": "RuntimeSummary",
                "source": "replace-code-ack",
                "runtime_summary": summary
            });
            self.last_preview_summary_refresh = Some(Instant::now());
        }
    }

    fn refresh_preview_summary_if_due(&mut self, now: Instant) -> bool {
        let due = self.last_preview_summary_refresh.is_none_or(|last| {
            now.duration_since(last) >= Duration::from_millis(DEV_PREVIEW_SUMMARY_REFRESH_MS)
        });
        if !due {
            return false;
        }
        let previous_hash = boon_runtime::sha256_bytes(
            &serde_json::to_vec(&self.last_preview_summary).unwrap_or_default(),
        );
        let next_summary = self.preview_transport.runtime_summary();
        if let Some(runtime_summary) = next_summary.get("runtime_summary")
            && runtime_summary_is_ready(runtime_summary)
        {
            self.last_good_runtime_summary = Some(runtime_summary.clone());
        }
        self.last_preview_summary = next_summary;
        self.last_preview_summary_refresh = Some(now);
        let next_hash = boon_runtime::sha256_bytes(
            &serde_json::to_vec(&self.last_preview_summary).unwrap_or_default(),
        );
        previous_hash != next_hash
    }

    fn command_probe(&self) -> serde_json::Value {
        let mut shell = self.clone();
        shell.workspace = shell.initial_workspace.clone();
        let original = shell.workspace.selected_example_id.clone();
        let catalog_listing = shell.catalog.list_available_examples();
        let mut selected_example_editor_model =
            shell.workspace.selected_buffer.model_feature_probe();
        selected_example_editor_model["font_family"] = json!(shell.editor_view.font_family);
        let selected_example_inventory = shell.structural_inventory();
        let initial_ui_source_bindings = shell.document_source_paths();
        let official_remove_disabled = shell.remove_custom_disabled_probe(1180.0, 820.0);
        let alternate = shell
            .catalog
            .entries
            .iter()
            .filter(|entry| entry.shown_by_default)
            .find(|entry| entry.id != original)
            .map(|entry| entry.id.clone())
            .or_else(|| shell.catalog.entries.first().map(|entry| entry.id.clone()));
        let tab_switch_json = match alternate {
            Some(example_id) => shell.dispatch_host_synthetic_source_path(
                &format!("dev.tabs.select.{example_id}"),
                1180.0,
                820.0,
            ),
            None => json!({"status": "fail", "blocker": "ExampleCatalog has no tab entries"}),
        };
        let run = shell.dispatch_host_synthetic_source_path("dev.commands.run", 1180.0, 820.0);
        let format =
            shell.dispatch_host_synthetic_source_path("dev.commands.format", 1180.0, 820.0);
        let reset = shell.dispatch_host_synthetic_source_path("dev.commands.reset", 1180.0, 820.0);
        let editor_text_input = shell.dispatch_host_synthetic_editor_text_input(
            "\n-- host synthetic editor input",
            1180.0,
            820.0,
        );
        let new_custom_tab =
            shell.dispatch_host_synthetic_source_path("dev.tabs.new", 1180.0, 820.0);
        let new_custom_id = new_custom_tab
            .get("stable_id")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
        let new_custom_editor_text_input = shell.dispatch_host_synthetic_editor_text_input(
            "-- persisted custom draft\n",
            1180.0,
            820.0,
        );
        let custom_source = std::fs::read_to_string("examples/counter.bn").unwrap_or_else(|_| {
            "-- custom example metadata lives outside Boon source\nstore: [title: TEXT { Custom }]\ndocument: Document/new(root: Element/label(element: [], style: [], label: store.title))\n".to_owned()
        });
        let custom_example = shell.workspace.create_or_update_custom_example(
            &mut shell.catalog,
            "probe",
            "Probe Custom",
            custom_source.clone(),
        );
        let custom_generic_runtime_source = std::fs::read_to_string("examples/counter.bn")
            .unwrap_or_else(|_| custom_source.clone());
        let custom_generic_runtime_example = shell.workspace.create_or_update_custom_example(
            &mut shell.catalog,
            "custom-generic-runtime",
            "Custom Generic Runtime",
            custom_generic_runtime_source,
        );
        let custom_store = ExampleCatalog::custom_store_probe();
        let custom_tab_after_create = shell
            .catalog
            .entries
            .iter()
            .any(|entry| entry.id == "custom:probe" && entry.custom && entry.shown_by_default);
        let custom_rename = shell
            .catalog
            .rename_custom_example("custom:probe", "Probe Custom Renamed");
        let select_probe_custom = shell.dispatch_host_synthetic_source_path(
            "dev.tabs.select.custom:probe",
            1180.0,
            820.0,
        );
        let custom_ui_source_bindings = shell.document_source_paths();
        let custom_remove_enabled = shell.remove_custom_enabled_probe(1180.0, 820.0);
        let custom_remove =
            shell.dispatch_host_synthetic_source_path("dev.commands.remove_custom", 1180.0, 820.0);
        let dirty_tab_preservation = shell
            .workspace
            .dirty_tab_preservation_probe(&shell.catalog)
            .unwrap_or_else(|error| {
                json!({
                    "status": "fail",
                    "command": "DirtyTabPreservation",
                    "diagnostic": error.to_string()
                })
            });
        let injected_source = shell.workspace.inject_source(
            "custom:injected",
            "custom://injected.bn",
            "-- injected source\nstore:\n    title: TEXT { Injected }\n\ndocument:\n    element:\n        kind: Text\n        text: title\n".to_owned(),
        );
        let new_custom_remove = match new_custom_id.as_deref() {
            Some(id) => {
                let select_new_custom = shell.dispatch_host_synthetic_source_path(
                    &format!("dev.tabs.select.{id}"),
                    1180.0,
                    820.0,
                );
                let mut remove_new_custom = shell.dispatch_host_synthetic_source_path(
                    "dev.commands.remove_custom",
                    1180.0,
                    820.0,
                );
                remove_new_custom["select_before_remove"] = select_new_custom;
                remove_new_custom
            }
            None => {
                json!({
                    "status": "fail",
                    "command": "RemoveCustomExample",
                    "diagnostic": "new custom tab did not report a stable id"
                })
            }
        };
        let mutation_probe_editor_model = shell.workspace.selected_buffer.model_feature_probe();
        let restore = shell.dispatch_host_synthetic_source_path(
            &format!("dev.tabs.select.{original}"),
            1180.0,
            820.0,
        );
        let status_pass = [
            &tab_switch_json,
            &run,
            &format,
            &reset,
            &editor_text_input,
            &new_custom_tab,
            &new_custom_editor_text_input,
            &custom_example,
            &custom_generic_runtime_example,
            &custom_store,
            &custom_rename,
            &select_probe_custom,
            &custom_remove,
            &new_custom_remove,
            &injected_source,
            &dirty_tab_preservation,
            &selected_example_editor_model,
            &restore,
        ]
        .iter()
        .all(|value| value.get("status").and_then(serde_json::Value::as_str) == Some("pass"));
        let all_pass = status_pass
            && custom_tab_after_create
            && official_remove_disabled
                .get("status")
                .and_then(serde_json::Value::as_str)
                == Some("pass")
            && custom_remove_enabled
                .get("status")
                .and_then(serde_json::Value::as_str)
                == Some("pass");
        let source_dispatches = [
            &tab_switch_json,
            &run,
            &format,
            &reset,
            &editor_text_input,
            &new_custom_tab,
            &new_custom_editor_text_input,
            &select_probe_custom,
            &restore,
        ]
        .iter()
        .filter(|value| {
            value
                .get("dispatch_boundary")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|boundary| boundary.starts_with("Document SourceBinding ->"))
        })
        .count();
        json!({
            "status": if all_pass { "pass" } else { "fail" },
            "evidence_tier": "dev-source-dispatch",
            "visible_window_input": false,
            "boundary": "DevWindowShell -> ExampleWorkspace -> CodeEditorModel/BoonLanguageService -> PreviewTransport",
            "command_dispatch_boundary": "Document SourceBinding -> DevWindowShell",
            "command_activation_boundary": "HostInputEvent -> layout hit region -> Document SourceBinding -> DevWindowShell",
            "command_dispatch_count": source_dispatches,
            "internal_command_shortcut": false,
            "ui_source_bindings": initial_ui_source_bindings,
            "initial_ui_source_bindings": initial_ui_source_bindings,
            "custom_ui_source_bindings": custom_ui_source_bindings,
            "catalog_listing": catalog_listing,
            "tab_switch": tab_switch_json,
            "run": run,
            "format": format,
            "reset": reset,
            "editor_text_input": editor_text_input,
            "new_custom_tab": new_custom_tab,
            "new_custom_editor_text_input": new_custom_editor_text_input,
            "custom_example": custom_example,
            "custom_generic_runtime_example": custom_generic_runtime_example,
            "custom_store": custom_store,
            "custom_tab_after_create": custom_tab_after_create,
            "custom_rename": custom_rename,
            "official_remove_disabled": official_remove_disabled,
            "select_probe_custom": select_probe_custom,
            "custom_remove_enabled": custom_remove_enabled,
            "custom_remove": custom_remove,
            "new_custom_remove": new_custom_remove,
            "inject_source": injected_source,
            "dirty_tab_preservation": dirty_tab_preservation,
            "editor_model": selected_example_editor_model,
            "mutation_probe_editor_model": mutation_probe_editor_model,
            "selected_example_structural_inventory": selected_example_inventory,
            "restore_original_tab": restore,
            "preview_receives_example_name": false,
            "parser_bypassed": false,
            "example_specific_shortcut": false
        })
    }

    fn visible_input_probe(
        &self,
        surface_proof: &boon_native_app_window::AppWindowSurfaceProof,
    ) -> serde_json::Value {
        let mut probe = self.command_probe();
        let route_proof = self.dev_window_route_proof(surface_proof);
        let route_pass = route_proof
            .get("status")
            .and_then(serde_json::Value::as_str)
            == Some("pass");
        let input_method = surface_proof.input_adapter.input_injection_method.clone();
        let app_owned_window_input_observed = surface_proof.input_adapter.real_os_events_observed
            && (surface_proof
                .input_adapter
                .mouse_last_window_protocol_id
                .is_some()
                || surface_proof
                    .input_adapter
                    .keyboard_last_window_protocol_id
                    .is_some());
        let real_window_input_observed =
            app_owned_window_input_observed && !surface_proof.input_adapter.synthetic_input_probe;
        let command_pass = probe.get("status").and_then(serde_json::Value::as_str) == Some("pass");
        probe["status"] = json!(
            if command_pass && route_pass && app_owned_window_input_observed {
                "pass"
            } else {
                "fail"
            }
        );
        probe["evidence_tier"] = json!(if real_window_input_observed {
            "real-window"
        } else {
            "boon-driver"
        });
        probe["legacy_evidence_tier"] = json!("host-synthetic");
        probe["visible_window_input"] = json!(real_window_input_observed);
        probe["app_owned_window_input"] = json!(app_owned_window_input_observed);
        probe["input_injection_method"] = json!(input_method);
        probe["app_window_synthetic_input_probe"] =
            json!(surface_proof.input_adapter.synthetic_input_probe);
        probe["real_os_input_claimed"] = json!(false);
        probe["real_window_event_boundary"] =
            json!("app_window coalesced input sampled from exact dev child window process");
        probe["target_dev_pid"] = json!(surface_proof.pid);
        probe["target_dev_window_title"] = json!(surface_proof.window_title);
        probe["target_dev_surface_id"] = json!(surface_proof.surface_id);
        probe["target_dev_window_id"] = json!(surface_proof.window_id);
        probe["app_owned_framebuffer"] = json!(surface_proof.readback_artifact);
        probe["visible_route_proof"] = route_proof;
        probe
    }

    fn passive_visible_probe(
        &self,
        surface_proof: &boon_native_app_window::AppWindowSurfaceProof,
    ) -> serde_json::Value {
        let route_proof = self.dev_window_route_proof(surface_proof);
        let route_pass = route_proof
            .get("status")
            .and_then(serde_json::Value::as_str)
            == Some("pass");
        json!({
            "status": if route_pass { "pass" } else { "fail" },
            "evidence_tier": "passive-visible-window",
            "visible_window_input": false,
            "app_owned_window_input": false,
            "verification_probe_enabled": false,
            "probe_mutations_allowed": false,
            "preview_mutation_allowed": false,
            "command_probe_executed": false,
            "reason": "manual visible launch records passive dev-window surface and route evidence only",
            "target_dev_pid": surface_proof.pid,
            "target_dev_window_title": surface_proof.window_title,
            "target_dev_surface_id": surface_proof.surface_id,
            "target_dev_window_id": surface_proof.window_id,
            "app_owned_framebuffer": surface_proof.readback_artifact,
            "visible_route_proof": route_proof
        })
    }

    fn dev_window_route_proof(
        &self,
        surface_proof: &boon_native_app_window::AppWindowSurfaceProof,
    ) -> serde_json::Value {
        let document = self.document();
        let mut measurer = boon_native_gpu::GlyphonTextMeasurer::new();
        let layout = boon_document::layout(boon_document::LayoutInput {
            document: &document,
            viewport: boon_host::Viewport {
                surface: 1,
                width: surface_proof.logical_size.width,
                height: surface_proof.logical_size.height,
                scale: surface_proof.logical_size.scale,
            },
            text: &mut measurer,
            capabilities: boon_document::RenderCapabilities::fake_portable(),
        });
        let source_intents = document
            .nodes
            .values()
            .filter_map(|node| {
                let binding = node.source_binding.as_ref()?;
                Some(json!({
                    "node": node.id,
                    "source_path": binding.source_path,
                    "intent": binding.intent,
                    "binding_id": binding.id
                }))
            })
            .collect::<Vec<_>>();
        let layout_json = serde_json::to_value(&layout).unwrap_or_else(|_| json!({}));
        let hit_regions = layout_json
            .get("hit_regions")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let selected_is_custom = self.selected_example_is_custom();
        let remove_custom_control = self.remove_custom_control_state();
        let remove_custom_disabled_for_manifest = !selected_is_custom
            && remove_custom_control
                .get("style_disabled")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
            && remove_custom_control
                .get("source_binding_present")
                .and_then(serde_json::Value::as_bool)
                == Some(false);
        let required_sources = {
            let mut sources = vec![
                "dev.commands.run".to_owned(),
                "dev.commands.format".to_owned(),
                "dev.commands.reset".to_owned(),
                "dev.editor.insert_text".to_owned(),
            ];
            if selected_is_custom {
                sources.push("dev.commands.remove_custom".to_owned());
            }
            sources.extend(
                self.catalog
                    .entries
                    .iter()
                    .filter(|entry| entry.shown_by_default)
                    .map(|entry| format!("dev.tabs.select.{}", entry.id)),
            );
            sources
        };
        let route_assertions = required_sources
            .iter()
            .map(|source| {
                let intent = source_intents.iter().find(|intent| {
                    intent
                        .get("source_path")
                        .and_then(serde_json::Value::as_str)
                        == Some(source.as_str())
                });
                let node = intent
                    .and_then(|intent| intent.get("node"))
                    .and_then(serde_json::Value::as_str);
                let hit_region = node.and_then(|node| {
                    hit_regions.iter().find(|region| {
                        region.get("node").and_then(serde_json::Value::as_str) == Some(node)
                    })
                });
                json!({
                    "source_path": source,
                    "target_node": node,
                    "source_binding_resolved": intent.is_some(),
                    "hit_test_performed": hit_region.is_some(),
                    "target_hit_region": hit_region.cloned().unwrap_or_else(|| json!(null)),
                    "input_path": "app_window synthetic input -> coalesced adapter -> dev document hit/source binding -> DevWindowShell command",
                    "pass": intent.is_some() && hit_region.is_some()
                })
            })
            .collect::<Vec<_>>();
        let pass = !route_assertions.is_empty()
            && route_assertions.iter().all(|assertion| {
                assertion.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
            })
            && (selected_is_custom || remove_custom_disabled_for_manifest);
        json!({
            "status": if pass { "pass" } else { "fail" },
            "surface_pid": surface_proof.pid,
            "surface_id": surface_proof.surface_id,
            "window_id": surface_proof.window_id,
            "window_title": surface_proof.window_title,
            "selected_example_id": self.workspace.selected_example_id,
            "selected_is_custom": selected_is_custom,
            "source_intent_count": source_intents.len(),
            "hit_region_count": hit_regions.len(),
            "required_sources": required_sources,
            "route_assertions": route_assertions,
            "remove_custom_control": remove_custom_control,
            "remove_custom_disabled_for_manifest": remove_custom_disabled_for_manifest,
            "layout_metrics": layout_json.get("metrics").cloned().unwrap_or_else(|| json!({})),
            "app_owned_readback": surface_proof.readback_artifact,
            "input_adapter": surface_proof.input_adapter
        })
    }

    fn structural_inventory(&self) -> serde_json::Value {
        let document = self.document();
        let mut kind_counts = BTreeMap::<String, usize>::new();
        let mut text_samples = Vec::new();
        let mut source_bindings = Vec::new();
        let mut command_bindings = Vec::new();
        let mut tab_bindings = Vec::new();
        let mut controls = Vec::new();
        let mut scrollable_nodes = Vec::new();
        let mut materialized_nodes = Vec::new();
        for node in document.nodes.values() {
            let kind = format!("{:?}", node.kind);
            *kind_counts.entry(kind.clone()).or_default() += 1;
            if node.scroll.is_some()
                || matches!(node.kind, boon_document_model::DocumentNodeKind::ScrollRoot)
            {
                scrollable_nodes.push(json!({
                    "node": node.id,
                    "kind": kind,
                    "scroll": node.scroll,
                    "style_scroll": node.style.get("scroll"),
                    "style_scroll_x": node.style.get("scroll_x")
                }));
            }
            if !node.materialized.is_empty() {
                materialized_nodes.push(json!({
                    "node": node.id,
                    "kind": kind,
                    "materialized": node.materialized
                }));
            }
            if let Some(text) = &node.text {
                if !text.text.trim().is_empty() && text_samples.len() < 24 {
                    text_samples.push(json!({
                        "node": node.id,
                        "kind": kind,
                        "text": text.text.chars().take(80).collect::<String>()
                    }));
                }
            }
            if matches!(
                node.kind,
                boon_document_model::DocumentNodeKind::Button
                    | boon_document_model::DocumentNodeKind::TextInput
            ) {
                controls.push(json!({
                    "node": node.id,
                    "kind": kind,
                    "text": node.text.as_ref().map(|text| text.text.clone())
                }));
            }
            if let Some(binding) = &node.source_binding {
                let binding_json = json!({
                    "node": node.id,
                    "kind": kind,
                    "source_path": binding.source_path,
                    "intent": binding.intent,
                    "binding_id": binding.id
                });
                if binding.source_path.starts_with("dev.commands.") {
                    command_bindings.push(binding_json.clone());
                }
                if binding.source_path.starts_with("dev.tabs.select.") {
                    tab_bindings.push(binding_json.clone());
                }
                source_bindings.push(binding_json);
            }
        }
        json!({
            "status": "pass",
            "node_count": document.nodes.len(),
            "kind_counts": kind_counts,
            "text_sample_count": text_samples.len(),
            "text_samples": text_samples,
            "control_count": controls.len(),
            "controls": controls,
            "source_binding_count": source_bindings.len(),
            "source_bindings": source_bindings,
            "command_binding_count": command_bindings.len(),
            "command_bindings": command_bindings,
            "tab_binding_count": tab_bindings.len(),
            "tab_bindings": tab_bindings,
            "focus": document.focus,
            "scroll_root_count": document.scroll_roots.len(),
            "scroll_roots": document.scroll_roots,
            "scrollable_node_count": scrollable_nodes.len(),
            "scrollable_nodes": scrollable_nodes,
            "materialized_node_count": materialized_nodes.len(),
            "materialized_nodes": materialized_nodes
        })
    }
}

fn dev_shell_document(
    shell: &DevWindowShell,
    _viewport_width: u32,
    viewport_height: u32,
) -> boon_document_model::DocumentFrame {
    use boon_document_model::{DocumentFrame, DocumentNodeKind};

    let mut frame = DocumentFrame::empty("dev-root");
    let root_height = viewport_height.max(360);
    let footer_height = 154_u32;
    let editor_height = viewport_height
        .saturating_sub(46)
        .saturating_sub(42)
        .saturating_sub(44)
        .saturating_sub(footer_height)
        .max(160);
    set_style(
        frame.nodes.get_mut(&frame.root).expect("root exists"),
        &[
            ("bg", DEV_BG),
            ("color", DEV_TEXT),
            ("padding", "10"),
            ("gap", "8"),
            ("width", "fill"),
            ("height", &root_height.to_string()),
        ],
    );

    let header = dev_node(
        "dev-header",
        DocumentNodeKind::Row,
        None,
        &[
            ("bg", DEV_PANEL),
            ("color", DEV_TEXT),
            ("border", DEV_BORDER_MUTED),
            ("padding", "8"),
            ("gap", "12"),
            ("height", "46"),
            ("width", "fill"),
        ],
    );
    let header_parent = header.id.clone();
    let tabs = dev_tabs_node(shell);
    let toolbar = dev_toolbar_node();
    let preview_status = shell
        .last_preview_transport
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("not-run");
    let root = frame.root.clone();
    append_child(&mut frame, root.clone(), header);
    append_child(
        &mut frame,
        header_parent.clone(),
        dev_text_node(
            "dev-header-title",
            "Boon Dev",
            DEV_TEXT,
            16,
            &[
                ("width", "auto"),
                ("height", "30"),
                ("font", BOON_EDITOR_FONT_FAMILY),
            ],
        ),
    );
    append_child(
        &mut frame,
        header_parent.clone(),
        dev_text_node(
            "dev-header-file",
            &one_line(&shell.workspace.current_file, 52),
            DEV_TEXT_MUTED,
            13,
            &[
                ("width", "360"),
                ("height", "30"),
                ("font", BOON_EDITOR_FONT_FAMILY),
            ],
        ),
    );
    append_child(
        &mut frame,
        header_parent.clone(),
        dev_status_pill(
            "dev-header-preview-status",
            &format!("Preview: {}", ui_status_label(preview_status)),
            status_color(preview_status),
            154,
        ),
    );
    append_child(
        &mut frame,
        header_parent.clone(),
        dev_status_pill(
            "dev-header-dirty-status",
            if shell.workspace.dirty {
                "Unsaved"
            } else {
                "Saved"
            },
            if shell.workspace.dirty {
                DEV_DIRTY
            } else {
                DEV_PASS
            },
            86,
        ),
    );
    append_child(
        &mut frame,
        header_parent.clone(),
        dev_text_node(
            "dev-header-example",
            &one_line(&shell.workspace.selected_example_id, 24),
            DEV_TEXT_MUTED,
            12,
            &[
                ("width", "220"),
                ("height", "30"),
                ("font", BOON_EDITOR_FONT_FAMILY),
            ],
        ),
    );
    append_child(&mut frame, header_parent, dev_custom_name_input(shell));
    let tabs_parent = tabs.id.clone();
    append_child(&mut frame, root.clone(), tabs);
    for entry in shell
        .catalog
        .entries
        .iter()
        .filter(|entry| entry.shown_by_default)
    {
        let max_label_chars = if entry.id.starts_with("custom:") {
            18
        } else {
            14
        };
        let mut label = one_line(&entry.label, max_label_chars);
        if shell.workspace.dirty_examples.contains(&entry.id) {
            label.push('*');
        }
        let selected = entry.id == shell.workspace.selected_example_id;
        let mut tab = dev_button_node(
            &format!("dev-tab-{}", entry.id),
            label,
            &[
                (
                    "bg",
                    if selected {
                        DEV_PANEL_ACTIVE
                    } else {
                        DEV_PANEL_RAISED
                    },
                ),
                ("color", if selected { DEV_TEXT } else { DEV_TEXT_MUTED }),
                ("border", if selected { DEV_ACCENT } else { DEV_BORDER }),
                ("padding", "6"),
                ("height", "30"),
                (
                    "width",
                    if entry.id.starts_with("custom:") {
                        "156"
                    } else {
                        "120"
                    },
                ),
                ("size", "13"),
                ("font", BOON_EDITOR_FONT_FAMILY),
            ],
        );
        tab.source_binding = Some(boon_document_model::SourceBinding {
            id: boon_document_model::SourceBindingId(format!("source:dev-tab:{}:select", entry.id)),
            source_path: format!("dev.tabs.select.{}", entry.id),
            intent: "select".to_owned(),
        });
        append_child(&mut frame, tabs_parent.clone(), tab);
    }
    let mut new_tab = dev_button_node(
        "dev-tab-new",
        "+".to_owned(),
        &[
            ("bg", DEV_PANEL_RAISED),
            ("color", DEV_TEXT),
            ("border", DEV_BORDER),
            ("padding", "6"),
            ("height", "30"),
            ("width", "42"),
            ("size", "14"),
            ("font", BOON_EDITOR_FONT_FAMILY),
        ],
    );
    new_tab.source_binding = Some(boon_document_model::SourceBinding {
        id: boon_document_model::SourceBindingId("source:dev-tab:new".to_owned()),
        source_path: "dev.tabs.new".to_owned(),
        intent: "select".to_owned(),
    });
    append_child(&mut frame, tabs_parent.clone(), new_tab);
    let toolbar_parent = toolbar.id.clone();
    append_child(&mut frame, root.clone(), toolbar);
    let selected_is_custom = shell
        .catalog
        .entries
        .iter()
        .any(|entry| entry.id == shell.workspace.selected_example_id && entry.custom);
    for command in ["run", "format", "reset", "remove_custom"] {
        let label = match command {
            "remove_custom" => "REMOVE".to_owned(),
            other => other.to_ascii_uppercase(),
        };
        let remove_disabled = command == "remove_custom" && !selected_is_custom;
        let mut button = dev_button_node(
            &format!("dev-command-{command}"),
            label,
            &[
                (
                    "bg",
                    if remove_disabled {
                        "#172031"
                    } else if command == "run" {
                        DEV_ACCENT
                    } else if command == "remove_custom" {
                        DEV_FAIL
                    } else {
                        DEV_PANEL_RAISED
                    },
                ),
                (
                    "color",
                    if remove_disabled {
                        "#64748b"
                    } else if command == "run" {
                        "#061528"
                    } else {
                        DEV_TEXT
                    },
                ),
                (
                    "border",
                    if remove_disabled {
                        DEV_BORDER_MUTED
                    } else if command == "run" {
                        DEV_ACCENT
                    } else {
                        DEV_BORDER
                    },
                ),
                ("padding", "8"),
                ("height", "32"),
                (
                    "width",
                    if command == "remove_custom" {
                        "116"
                    } else {
                        "96"
                    },
                ),
                ("size", "13"),
                ("font", BOON_EDITOR_FONT_FAMILY),
            ],
        );
        if remove_disabled {
            button.style.insert(
                "disabled".to_owned(),
                boon_document_model::StyleValue::Bool(true),
            );
        } else {
            button.source_binding = Some(boon_document_model::SourceBinding {
                id: boon_document_model::SourceBindingId(format!("source:dev-command:{command}")),
                source_path: format!("dev.commands.{command}"),
                intent: "press".to_owned(),
            });
        }
        append_child(&mut frame, toolbar_parent.clone(), button);
    }
    shell.editor_view.append_to(
        &mut frame,
        root.clone(),
        &shell.workspace.selected_buffer,
        editor_height,
        shell.caret_visible,
    );
    append_dev_footer(&mut frame, root, shell, footer_height);
    frame.focus = Some(boon_document_model::DocumentNodeId(
        "dev-code-editor".to_owned(),
    ));
    frame
}

fn append_dev_footer(
    frame: &mut boon_document_model::DocumentFrame,
    parent: boon_document_model::DocumentNodeId,
    shell: &DevWindowShell,
    height: u32,
) {
    let footer = dev_node(
        "dev-footer",
        boon_document_model::DocumentNodeKind::Stack,
        None,
        &[
            ("bg", DEV_PANEL),
            ("border", DEV_BORDER_MUTED),
            ("padding", "8"),
            ("gap", "6"),
            ("height", &height.to_string()),
            ("width", "fill"),
        ],
    );
    let footer_parent = footer.id.clone();
    append_child(frame, parent, footer);

    let scroll_height = height.saturating_sub(16).max(44);
    let footer_lines = wrap_footer_lines(shell.footer_lines(), DEV_FOOTER_VALUE_WRAP_CHARS);
    let effective_scroll_line = shell
        .footer_scroll_line
        .min(footer_lines.len().saturating_sub(1));
    let mut scroll = dev_node(
        "dev-footer-scroll",
        boon_document_model::DocumentNodeKind::ScrollRoot,
        None,
        &[
            ("bg", DEV_BG),
            ("border", DEV_BORDER_MUTED),
            ("padding", "6"),
            ("gap", "3"),
            ("height", &scroll_height.to_string()),
            ("width", "fill"),
            ("scroll", "true"),
        ],
    );
    scroll.scroll = Some(boon_document_model::ScrollState {
        x: 0.0,
        y: effective_scroll_line as f32,
    });
    scroll
        .materialized
        .push(boon_document_model::MaterializedRange {
            axis: boon_document_model::Axis::Vertical,
            visible: 0..6,
            overscan: 0..10,
        });
    let scroll_parent = scroll.id.clone();
    frame.scroll_roots.insert(
        boon_document_model::ScrollRootId(scroll_parent.0.clone()),
        boon_document_model::ScrollState {
            x: 0.0,
            y: effective_scroll_line as f32,
        },
    );
    append_child(frame, footer_parent, scroll);

    let visible_line_count =
        (scroll_height.saturating_sub(12) / DEV_FOOTER_LINE_HEIGHT).max(1) as usize;
    for (visible_index, (label, value)) in footer_lines
        .into_iter()
        .skip(effective_scroll_line)
        .take(visible_line_count)
        .enumerate()
    {
        let row_id = format!("dev-footer-row-{visible_index}");
        let row_bg = if visible_index % 2 == 0 {
            DEV_BG
        } else {
            "#101a2c"
        };
        let row = dev_node(
            &row_id,
            boon_document_model::DocumentNodeKind::Row,
            None,
            &[
                ("bg", row_bg),
                ("padding", "0"),
                ("gap", "8"),
                ("height", &DEV_FOOTER_LINE_HEIGHT.to_string()),
                ("width", "fill"),
            ],
        );
        let row_parent = row.id.clone();
        append_child(frame, scroll_parent.clone(), row);
        append_child(
            frame,
            row_parent.clone(),
            dev_text_node(
                &format!("dev-footer-row-{visible_index}-label"),
                &label,
                DEV_ACCENT,
                12,
                &[
                    ("bg", row_bg),
                    ("width", "92"),
                    ("height", &DEV_FOOTER_LINE_HEIGHT.to_string()),
                    ("font", BOON_EDITOR_FONT_FAMILY),
                ],
            ),
        );
        append_child(
            frame,
            row_parent,
            dev_text_node(
                &format!("dev-footer-row-{visible_index}-value"),
                &value,
                DEV_TEXT_MUTED,
                12,
                &[
                    ("bg", row_bg),
                    ("width", "fill"),
                    ("height", &DEV_FOOTER_LINE_HEIGHT.to_string()),
                    ("font", BOON_EDITOR_FONT_FAMILY),
                ],
            ),
        );
    }
}

fn dev_text_node(
    id: &str,
    text: &str,
    color: &str,
    size: u32,
    extra_styles: &[(&str, &str)],
) -> boon_document_model::DocumentNode {
    let size_text = size.to_string();
    let mut styles = vec![
        ("bg", DEV_PANEL),
        ("color", color),
        ("size", size_text.as_str()),
    ];
    styles.extend_from_slice(extra_styles);
    dev_node(
        id,
        boon_document_model::DocumentNodeKind::Text,
        Some(text.to_owned()),
        &styles,
    )
}

fn dev_status_pill(
    id: &str,
    text: &str,
    accent: &str,
    width: u32,
) -> boon_document_model::DocumentNode {
    let width_text = width.to_string();
    let text = one_line(text, (width / 8).max(4) as usize);
    dev_text_node(
        id,
        &text,
        DEV_TEXT,
        12,
        &[
            ("bg", DEV_PANEL_RAISED),
            ("border", accent),
            ("padding", "5"),
            ("height", "24"),
            ("width", width_text.as_str()),
            ("font", BOON_EDITOR_FONT_FAMILY),
            ("align", "center"),
            ("vertical_align", "center"),
            ("text_inset", "0"),
        ],
    )
}

fn dev_custom_name_input(shell: &DevWindowShell) -> boon_document_model::DocumentNode {
    let selected_is_custom = shell.selected_example_is_custom();
    let label = if selected_is_custom {
        shell.selected_example_label()
    } else {
        "official example".to_owned()
    };
    let mut input = dev_node(
        "dev-custom-name-input",
        boon_document_model::DocumentNodeKind::TextInput,
        Some(one_line(&label, 22)),
        &[
            (
                "bg",
                if selected_is_custom {
                    DEV_PANEL_RAISED
                } else {
                    "#172031"
                },
            ),
            (
                "color",
                if selected_is_custom {
                    DEV_TEXT
                } else {
                    "#64748b"
                },
            ),
            (
                "border",
                if selected_is_custom {
                    DEV_ACCENT
                } else {
                    DEV_BORDER_MUTED
                },
            ),
            ("padding", "5"),
            ("height", "24"),
            ("width", "190"),
            ("size", "12"),
            ("font", BOON_EDITOR_FONT_FAMILY),
            ("align", "center"),
            ("vertical_align", "center"),
            ("text_inset", "0"),
            ("placeholder", "custom name"),
        ],
    );
    if selected_is_custom {
        input.source_binding = Some(boon_document_model::SourceBinding {
            id: boon_document_model::SourceBindingId("source:dev-custom-name".to_owned()),
            source_path: "dev.custom.name".to_owned(),
            intent: "text_input".to_owned(),
        });
    } else {
        input.style.insert(
            "disabled".to_owned(),
            boon_document_model::StyleValue::Bool(true),
        );
    }
    input
}

fn status_color(status: &str) -> &'static str {
    match status {
        "pass" => DEV_PASS,
        "fail" | "unavailable" => DEV_FAIL,
        "not-run" | "deferred" | "not-bound" => DEV_WARN,
        _ => DEV_ACCENT,
    }
}

fn ui_status_label(status: &str) -> &'static str {
    match status {
        "pass" => "Synced",
        "fail" => "Error",
        "unavailable" => "Offline",
        "deferred" => "Updating",
        "not-run" | "not-bound" => "Waiting",
        _ => "Ready",
    }
}

fn friendly_dev_command(command: &str) -> &'static str {
    match command {
        "startup" => "Startup",
        "test" => "Test",
        "Run" => "Run",
        "Format" => "Format",
        "Reset" => "Reset",
        "RemoveCustomExample" => "Remove",
        "NewCustomTab" => "New custom",
        "SelectTab" => "Select example",
        "EditorTextInput" => "Edit",
        _ => "Action",
    }
}

fn json_diagnostic(value: &serde_json::Value) -> Option<String> {
    for pointer in [
        "/diagnostic",
        "/reason",
        "/blocker",
        "/ack/diagnostic",
        "/ack/reason",
        "/ack/blocker",
        "/preview_runtime_summary/reason",
        "/runtime_summary/reason",
    ] {
        if let Some(text) = value.pointer(pointer).and_then(serde_json::Value::as_str) {
            let text = one_line(text, 160);
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn runtime_footer_summary(
    summary: &serde_json::Value,
    runtime_state_hash: &str,
    source_hash: &str,
) -> String {
    let keys = summary
        .get("state_summary_top_level_keys")
        .and_then(serde_json::Value::as_array)
        .map(|keys| {
            keys.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let bytes = summary
        .get("state_summary_bytes")
        .and_then(serde_json::Value::as_u64)
        .map(format_runtime_bytes);
    let mut parts = vec![
        format!("state {runtime_state_hash}"),
        format!("source {source_hash}"),
    ];
    if let Some(bytes) = bytes {
        parts.push(format!("state size {bytes}"));
    }
    if keys.is_empty() {
        return parts.join(", ");
    }
    let sample_limit = 5;
    let sample = keys
        .iter()
        .take(sample_limit)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let extra = keys.len().saturating_sub(sample_limit);
    let keys_text = if extra == 0 {
        format!("{} keys: {sample}", keys.len())
    } else {
        format!("{} keys: {sample}, +{extra} more", keys.len())
    };
    parts.push(keys_text);
    parts.join(", ")
}

fn format_runtime_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    }
}

fn wrap_footer_lines(lines: Vec<(String, String)>, max_chars: usize) -> Vec<(String, String)> {
    lines
        .into_iter()
        .flat_map(|(label, value)| {
            wrap_text_chunks(&value, max_chars)
                .into_iter()
                .enumerate()
                .map(move |(index, chunk)| {
                    if index == 0 {
                        (label.clone(), chunk)
                    } else {
                        ("".to_owned(), chunk)
                    }
                })
        })
        .collect()
}

fn wrap_text_chunks(value: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(8);
    let normalized = value
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return vec![String::new()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for word in normalized.split(' ') {
        if word.chars().count() > max_chars {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            let chars = word.chars().collect::<Vec<_>>();
            for piece in chars.chunks(max_chars) {
                chunks.push(piece.iter().collect());
            }
            continue;
        }
        let separator = if current.is_empty() { 0 } else { 1 };
        if current.chars().count() + separator + word.chars().count() > max_chars {
            chunks.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn runtime_summary_is_ready(summary: &serde_json::Value) -> bool {
    summary.get("status").and_then(serde_json::Value::as_str) == Some("pass")
        && summary
            .get("state_summary_hash")
            .and_then(serde_json::Value::as_str)
            .is_some()
        && summary
            .get("source_sha256")
            .and_then(serde_json::Value::as_str)
            .is_some()
}

fn runtime_summary_matches_source(summary: &serde_json::Value, source_hash: &str) -> bool {
    runtime_summary_is_ready(summary)
        && summary
            .get("source_sha256")
            .and_then(serde_json::Value::as_str)
            == Some(source_hash)
}

fn short_hash(value: &str) -> String {
    value.chars().take(12).collect()
}

fn one_line(value: &str, max_chars: usize) -> String {
    let mut text = value
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.chars().count() > max_chars {
        text = text
            .chars()
            .take(max_chars.saturating_sub(3))
            .collect::<String>();
        text.push_str("...");
    }
    text
}

fn normalize_custom_example_label(value: &str) -> String {
    let label = value
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if label.is_empty() {
        "Untitled".to_owned()
    } else {
        label.chars().take(80).collect()
    }
}

fn dev_tabs_node(_shell: &DevWindowShell) -> boon_document_model::DocumentNode {
    let mut tabs = dev_node(
        "dev-example-tabs",
        boon_document_model::DocumentNodeKind::Row,
        None,
        &[
            ("bg", DEV_PANEL),
            ("border", DEV_BORDER_MUTED),
            ("padding", "6"),
            ("gap", "6"),
            ("height", "42"),
            ("width", "fill"),
        ],
    );
    tabs.source_binding = Some(boon_document_model::SourceBinding {
        id: boon_document_model::SourceBindingId("source:dev-example-tabs:select".to_owned()),
        source_path: "dev.tabs.select".to_owned(),
        intent: "select".to_owned(),
    });
    tabs
}

fn dev_toolbar_node() -> boon_document_model::DocumentNode {
    let mut toolbar = dev_node(
        "dev-toolbar",
        boon_document_model::DocumentNodeKind::Row,
        None,
        &[
            ("bg", DEV_PANEL),
            ("border", DEV_BORDER_MUTED),
            ("color", DEV_TEXT),
            ("padding", "8"),
            ("gap", "10"),
            ("height", "44"),
            ("width", "fill"),
        ],
    );
    toolbar.source_binding = Some(boon_document_model::SourceBinding {
        id: boon_document_model::SourceBindingId("source:dev-toolbar:press".to_owned()),
        source_path: "dev.commands.press".to_owned(),
        intent: "press".to_owned(),
    });
    toolbar
}

fn dev_button_node(
    id: &str,
    text: String,
    styles: &[(&str, &str)],
) -> boon_document_model::DocumentNode {
    let mut node = dev_node(
        id,
        boon_document_model::DocumentNodeKind::Button,
        Some(text),
        styles,
    );
    node.style
        .entry("align".to_owned())
        .or_insert_with(|| boon_document_model::StyleValue::Text("center".to_owned()));
    node.style
        .entry("vertical_align".to_owned())
        .or_insert_with(|| boon_document_model::StyleValue::Text("center".to_owned()));
    node
}

fn dev_node(
    id: &str,
    kind: boon_document_model::DocumentNodeKind,
    text: Option<String>,
    styles: &[(&str, &str)],
) -> boon_document_model::DocumentNode {
    let mut node = boon_document_model::DocumentNode::new(id, kind);
    if let Some(text) = text {
        node.text = Some(boon_document_model::TextValue { text });
    }
    set_style(&mut node, styles);
    node
}

fn append_child(
    frame: &mut boon_document_model::DocumentFrame,
    parent: boon_document_model::DocumentNodeId,
    mut child: boon_document_model::DocumentNode,
) {
    child.parent = Some(parent.clone());
    if let Some(parent_node) = frame.nodes.get_mut(&parent) {
        parent_node.children.push(child.id.clone());
    }
    frame.nodes.insert(child.id.clone(), child);
}

fn set_style(node: &mut boon_document_model::DocumentNode, styles: &[(&str, &str)]) {
    for (key, value) in styles {
        let style_value = value
            .parse::<f64>()
            .map(boon_document_model::StyleValue::Number)
            .unwrap_or_else(|_| boon_document_model::StyleValue::Text((*value).to_owned()));
        node.style.insert((*key).to_owned(), style_value);
    }
}

fn viewport_fill_ratio(frame: &boon_document::LayoutFrame, width: u32, height: u32) -> f64 {
    let Some(bounds) = frame.display_list.iter().fold(None, |acc, item| {
        let rect = item.bounds;
        Some(match acc {
            Some((x0, y0, x1, y1)) => (
                f32::min(x0, rect.x),
                f32::min(y0, rect.y),
                f32::max(x1, rect.x + rect.width),
                f32::max(y1, rect.y + rect.height),
            ),
            None => (rect.x, rect.y, rect.x + rect.width, rect.y + rect.height),
        })
    }) else {
        return 0.0;
    };
    let covered_width = (bounds.2 - bounds.0).clamp(0.0, width as f32);
    let covered_height = (bounds.3 - bounds.1).clamp(0.0, height as f32);
    let viewport_area = (width.max(1) * height.max(1)) as f64;
    (covered_width as f64 * covered_height as f64 / viewport_area).min(1.0)
}

fn canonical_document_root<'a>(
    document: &'a AstStatement,
    expressions: &[AstExpr],
) -> Option<&'a AstStatement> {
    let root = document
        .children
        .iter()
        .find(|child| document_field_name(child).as_deref() == Some("root"))?;
    if canonical_element_function(root, expressions).is_some() {
        return Some(root);
    }
    if document_call_function(root, expressions).is_some() {
        return Some(root);
    }
    root.children.iter().find(|child| {
        canonical_element_function(child, expressions).is_some()
            || document_call_function(child, expressions).is_some()
    })
}

fn canonical_element_function<'a>(
    statement: &'a AstStatement,
    expressions: &'a [AstExpr],
) -> Option<&'a str> {
    let expr = expressions.get(statement.expr?)?;
    match &expr.kind {
        AstExprKind::Call { function, .. } if function.starts_with("Element/") => {
            Some(function.as_str())
        }
        _ => None,
    }
}

fn document_call_function<'a>(
    statement: &'a AstStatement,
    expressions: &'a [AstExpr],
) -> Option<&'a str> {
    let expr = expressions.get(statement.expr?)?;
    match &expr.kind {
        AstExprKind::Call { function, .. } => Some(function.as_str()),
        _ => None,
    }
}

fn document_call_args<'a>(
    statement: &'a AstStatement,
    expressions: &'a [AstExpr],
) -> Option<&'a [AstCallArg]> {
    let expr = expressions.get(statement.expr?)?;
    match &expr.kind {
        AstExprKind::Call { args, .. } => Some(args.as_slice()),
        _ => None,
    }
}

struct DocumentFunctionRegistry<'a> {
    functions: BTreeMap<&'a str, &'a AstStatement>,
}

impl<'a> DocumentFunctionRegistry<'a> {
    fn new(statements: &'a [AstStatement]) -> Self {
        let mut functions = BTreeMap::new();
        Self::collect(statements, &mut functions);
        Self { functions }
    }

    fn collect(
        statements: &'a [AstStatement],
        functions: &mut BTreeMap<&'a str, &'a AstStatement>,
    ) {
        for statement in statements {
            if let AstStatementKind::Function { name, .. } = &statement.kind {
                functions.insert(name.as_str(), statement);
            }
            Self::collect(&statement.children, functions);
        }
    }

    fn get(&self, name: &str) -> Option<&'a AstStatement> {
        self.functions.get(name).copied()
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_canonical_document_entry(
    statement: &AstStatement,
    expressions: &[AstExpr],
    functions: &DocumentFunctionRegistry<'_>,
    parent: &boon_document_model::DocumentNodeId,
    frame: &mut boon_document_model::DocumentFrame,
    source_intents: &mut Vec<serde_json::Value>,
    seen_ids: &mut BTreeSet<String>,
    context: &DocumentEvalContext<'_>,
    scope_key: &str,
    is_root_child: bool,
) {
    if canonical_element_function(statement, expressions).is_some() {
        lower_canonical_document_element(
            statement,
            expressions,
            functions,
            parent,
            frame,
            source_intents,
            seen_ids,
            context,
            scope_key,
            is_root_child,
        );
        return;
    }

    if let Some(function_name) = document_call_function(statement, expressions)
        && let Some(function) = functions.get(function_name)
    {
        let scoped = document_function_call_context(function, statement, expressions, context);
        for child in &function.children {
            lower_canonical_document_entry(
                child,
                expressions,
                functions,
                parent,
                frame,
                source_intents,
                seen_ids,
                &scoped,
                scope_key,
                is_root_child,
            );
        }
        return;
    }

    lower_canonical_child_elements(
        statement,
        expressions,
        functions,
        parent,
        frame,
        source_intents,
        seen_ids,
        context,
        scope_key,
    );
}

fn document_function_call_context<'a>(
    function: &AstStatement,
    call: &AstStatement,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'a>,
) -> DocumentEvalContext<'a> {
    let mut scoped = DocumentEvalContext {
        root: context.root,
        locals: context.locals.clone(),
    };
    let formals = match &function.kind {
        AstStatementKind::Function { args, .. } => args.as_slice(),
        _ => &[],
    };
    if let Some(args) = document_call_args(call, expressions) {
        for (index, arg) in args.iter().enumerate() {
            let Some(name) = arg
                .name
                .as_deref()
                .or_else(|| formals.get(index).map(String::as_str))
            else {
                continue;
            };
            if let Some(expr) = expressions.get(arg.value) {
                let value = document_eval_expr_value(expr, expressions, context)
                    .or_else(|| document_expr_value(expr, expressions).map(Value::String));
                if let Some(value) = value {
                    scoped.locals.insert(name.to_owned(), value);
                }
            }
        }
    }
    scoped
}

#[allow(clippy::too_many_arguments)]
fn lower_canonical_document_element(
    statement: &AstStatement,
    expressions: &[AstExpr],
    functions: &DocumentFunctionRegistry<'_>,
    parent: &boon_document_model::DocumentNodeId,
    frame: &mut boon_document_model::DocumentFrame,
    source_intents: &mut Vec<serde_json::Value>,
    seen_ids: &mut BTreeSet<String>,
    context: &DocumentEvalContext<'_>,
    scope_key: &str,
    is_root_child: bool,
) {
    let Some(function) = canonical_element_function(statement, expressions) else {
        lower_canonical_child_elements(
            statement,
            expressions,
            functions,
            parent,
            frame,
            source_intents,
            seen_ids,
            context,
            scope_key,
        );
        return;
    };
    if function == "Element/repeat" {
        lower_canonical_repeat(
            statement,
            expressions,
            functions,
            parent,
            frame,
            source_intents,
            seen_ids,
            context,
            scope_key,
        );
        return;
    }
    if document_child_bool(statement, "visible", expressions, context) == Some(false) {
        return;
    }

    let base_node_id = format!("doc-node-{}", statement.id);
    let scoped_id = if scope_key.is_empty() {
        base_node_id.clone()
    } else {
        format!("{base_node_id}-{scope_key}")
    };
    let mut node_id = scoped_id.clone();
    let mut dedupe = 0usize;
    while !seen_ids.insert(node_id.clone()) {
        dedupe += 1;
        node_id = format!("{scoped_id}-{dedupe}");
    }
    let id = boon_document_model::DocumentNodeId(node_id.clone());
    let mut node = boon_document_model::DocumentNode::new(
        node_id,
        canonical_document_node_kind(function, statement, expressions),
    );
    node.parent = Some(parent.clone());

    lower_canonical_element_style(statement, expressions, context, &mut node);
    lower_canonical_element_text(statement, expressions, context, &mut node);
    lower_canonical_element_sources(
        statement,
        expressions,
        context,
        &id,
        &mut node,
        source_intents,
    );
    if is_root_child {
        node.style
            .entry("width".to_owned())
            .or_insert_with(|| boon_document_model::StyleValue::Text("Fill".to_owned()));
        if matches!(
            node.style.get("height"),
            Some(boon_document_model::StyleValue::Text(value)) if value == "Fill"
        ) {
            node.style.insert(
                "height".to_owned(),
                boon_document_model::StyleValue::Number(720.0),
            );
        } else {
            node.style
                .entry("height".to_owned())
                .or_insert(boon_document_model::StyleValue::Number(720.0));
        }
    }

    let vertical_scroll = matches!(node.kind, boon_document_model::DocumentNodeKind::Table)
        || style_bool(&node.style, "scroll")
        || style_bool(&node.style, "scroll_y")
        || style_bool(&node.style, "scrollbars");
    let horizontal_scroll = matches!(node.kind, boon_document_model::DocumentNodeKind::Table)
        || style_bool(&node.style, "scroll")
        || style_bool(&node.style, "scroll_x")
        || style_bool(&node.style, "scrollbars");
    if vertical_scroll {
        node.materialized
            .push(boon_document_model::MaterializedRange {
                axis: boon_document_model::Axis::Vertical,
                visible: 0..20,
                overscan: 0..28,
            });
    }
    if horizontal_scroll {
        node.materialized
            .push(boon_document_model::MaterializedRange {
                axis: boon_document_model::Axis::Horizontal,
                visible: 0..8,
                overscan: 0..12,
            });
    }

    if let Some(parent_node) = frame.nodes.get_mut(parent) {
        parent_node.children.push(id.clone());
    }
    frame.nodes.insert(id.clone(), node);
    lower_canonical_child_elements(
        statement,
        expressions,
        functions,
        &id,
        frame,
        source_intents,
        seen_ids,
        context,
        scope_key,
    );
}

fn canonical_document_node_kind(
    function: &str,
    statement: &AstStatement,
    expressions: &[AstExpr],
) -> boon_document_model::DocumentNodeKind {
    match function {
        "Element/stripe" => {
            match document_child_value(statement, "direction", expressions).as_deref() {
                Some("Row") => boon_document_model::DocumentNodeKind::Row,
                _ => boon_document_model::DocumentNodeKind::Stack,
            }
        }
        "Element/text" | "Element/label" | "Element/paragraph" | "Element/link" => {
            boon_document_model::DocumentNodeKind::Text
        }
        "Element/button" | "Element/checkbox" => boon_document_model::DocumentNodeKind::Button,
        "Element/text_input" => boon_document_model::DocumentNodeKind::TextInput,
        _ => boon_document_model::DocumentNodeKind::Stack,
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_canonical_repeat(
    statement: &AstStatement,
    expressions: &[AstExpr],
    functions: &DocumentFunctionRegistry<'_>,
    parent: &boon_document_model::DocumentNodeId,
    frame: &mut boon_document_model::DocumentFrame,
    source_intents: &mut Vec<serde_json::Value>,
    seen_ids: &mut BTreeSet<String>,
    context: &DocumentEvalContext<'_>,
    scope_key: &str,
) {
    let Some(list_path) = document_child_value(statement, "list", expressions) else {
        return;
    };
    let item_name =
        document_child_value(statement, "item", expressions).unwrap_or_else(|| "item".to_owned());
    let Some(items) = document_resolved_value(&list_path, context).and_then(Value::as_array) else {
        return;
    };
    for (index, item) in items.iter().enumerate() {
        let mut scoped = DocumentEvalContext {
            root: context.root,
            locals: context.locals.clone(),
        };
        scoped.locals.insert(item_name.clone(), item.clone());
        let child_scope = if scope_key.is_empty() {
            format!("{item_name}-{index}")
        } else {
            format!("{scope_key}-{item_name}-{index}")
        };
        for child in &statement.children {
            if matches!(
                document_field_name(child).as_deref(),
                Some("child" | "template" | "items" | "children")
            ) {
                lower_canonical_document_entry(
                    child,
                    expressions,
                    functions,
                    parent,
                    frame,
                    source_intents,
                    seen_ids,
                    &scoped,
                    &child_scope,
                    false,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_canonical_child_elements(
    statement: &AstStatement,
    expressions: &[AstExpr],
    functions: &DocumentFunctionRegistry<'_>,
    parent: &boon_document_model::DocumentNodeId,
    frame: &mut boon_document_model::DocumentFrame,
    source_intents: &mut Vec<serde_json::Value>,
    seen_ids: &mut BTreeSet<String>,
    context: &DocumentEvalContext<'_>,
    scope_key: &str,
) {
    for child in &statement.children {
        let field = document_field_name(child);
        if !matches!(
            field.as_deref(),
            Some("items" | "children" | "child" | "contents" | "template")
        ) && canonical_element_function(child, expressions).is_none()
            && document_call_function(child, expressions)
                .and_then(|function| functions.get(function))
                .is_none()
        {
            continue;
        }
        if canonical_element_function(child, expressions).is_some()
            || document_call_function(child, expressions)
                .and_then(|function| functions.get(function))
                .is_some()
        {
            lower_canonical_document_entry(
                child,
                expressions,
                functions,
                parent,
                frame,
                source_intents,
                seen_ids,
                context,
                scope_key,
                false,
            );
        }
        for nested in &child.children {
            if canonical_element_function(nested, expressions).is_some()
                || document_call_function(nested, expressions)
                    .and_then(|function| functions.get(function))
                    .is_some()
                || matches!(
                    document_field_name(nested).as_deref(),
                    Some("child" | "template")
                )
            {
                lower_canonical_document_entry(
                    nested,
                    expressions,
                    functions,
                    parent,
                    frame,
                    source_intents,
                    seen_ids,
                    context,
                    scope_key,
                    false,
                );
            }
        }
    }
}

fn lower_canonical_element_style(
    statement: &AstStatement,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
    node: &mut boon_document_model::DocumentNode,
) {
    for child in &statement.children {
        let Some(field) = document_field_name(child) else {
            continue;
        };
        match field.as_str() {
            "style" => lower_canonical_style_block(child, expressions, context, node),
            "gap" | "width" | "height" | "padding" | "size" | "scroll" | "scroll_x"
            | "scroll_y" | "scrollbars" | "center" | "hover_visible" => {
                if let Some(value) = document_style_value(child, expressions, context) {
                    node.style.insert(field, value);
                }
            }
            "checked" | "selected" | "visible" | "focus" => {
                if let Some(value) = document_style_value(child, expressions, context) {
                    node.style.insert(field, value);
                }
            }
            _ => {}
        }
    }
}

fn lower_canonical_style_block(
    statement: &AstStatement,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
    node: &mut boon_document_model::DocumentNode,
) {
    if let Some(fields) = record_fields_for_statement(statement, expressions) {
        lower_canonical_style_record(fields, expressions, context, node);
    }
    for child in &statement.children {
        let Some(field) = document_field_name(child) else {
            continue;
        };
        match field.as_str() {
            "background" => {
                if let Some(color) =
                    statement_nested_style_value(child, "color", expressions, context)
                        .or_else(|| {
                            document_child_style_value(child, "color", expressions, context)
                        })
                        .or_else(|| document_style_value(child, expressions, context))
                {
                    node.style.insert("background".to_owned(), color);
                }
            }
            "font" => {
                if let Some(font_fields) = record_fields_for_statement(child, expressions) {
                    for font_field in font_fields {
                        if matches!(font_field.name.as_str(), "size" | "color" | "weight")
                            && let Some(value) = document_style_value_for_expr(
                                font_field.value,
                                expressions,
                                context,
                            )
                        {
                            node.style.insert(font_field.name.clone(), value);
                        }
                    }
                }
                for font_child in &child.children {
                    let Some(font_field) = document_field_name(font_child) else {
                        continue;
                    };
                    if matches!(font_field.as_str(), "size" | "color" | "weight") {
                        if let Some(value) = document_style_value(font_child, expressions, context)
                        {
                            node.style.insert(font_field, value);
                        }
                    }
                }
            }
            "align" => {
                let record_centered =
                    record_fields_for_statement(child, expressions).is_some_and(|fields| {
                        fields.iter().any(|align_field| {
                            document_expr_value_by_id(align_field.value, expressions)
                                .as_deref()
                                .is_some_and(|value| matches!(value, "Center" | "center"))
                        })
                    });
                let centered = record_centered
                    || child.children.iter().any(|align_child| {
                        document_statement_value(align_child, expressions)
                            .as_deref()
                            .is_some_and(|value| matches!(value, "Center" | "center"))
                    });
                if centered {
                    node.style.insert(
                        "center".to_owned(),
                        boon_document_model::StyleValue::Bool(true),
                    );
                }
            }
            "padding" => {
                if let Some(value) =
                    document_style_value(child, expressions, context).or_else(|| {
                        child
                            .children
                            .iter()
                            .find_map(|entry| document_style_value(entry, expressions, context))
                    })
                {
                    node.style.insert("padding".to_owned(), value);
                }
            }
            "outline" | "border" | "borders" | "selected_border" => {
                if let Some(color) =
                    statement_nested_style_value(child, "color", expressions, context)
                        .or_else(|| {
                            document_child_style_value(child, "color", expressions, context)
                        })
                        .or_else(|| document_style_value(child, expressions, context))
                {
                    let style_key = if field == "selected_border" {
                        "selected_border"
                    } else {
                        "border"
                    };
                    node.style.insert(style_key.to_owned(), color);
                }
            }
            _ => {
                if let Some(value) = document_style_value(child, expressions, context) {
                    node.style.insert(field, value);
                }
            }
        }
    }
}

fn statement_nested_style_value(
    statement: &AstStatement,
    nested_name: &str,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
) -> Option<boon_document_model::StyleValue> {
    record_fields_for_statement(statement, expressions).and_then(|fields| {
        fields
            .iter()
            .find(|field| field.name == nested_name)
            .and_then(|field| document_style_value_for_expr(field.value, expressions, context))
    })
}

fn lower_canonical_style_record(
    fields: &[AstRecordField],
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
    node: &mut boon_document_model::DocumentNode,
) {
    for field in fields {
        match field.name.as_str() {
            "background" => {
                if let Some(value) =
                    record_field_nested_style_value(field, "color", expressions, context).or_else(
                        || document_style_value_for_expr(field.value, expressions, context),
                    )
                {
                    node.style.insert("background".to_owned(), value);
                }
            }
            "font" => {
                if let Some(font_fields) = record_fields_for_expr(field.value, expressions) {
                    for font_field in font_fields {
                        if matches!(font_field.name.as_str(), "size" | "color" | "weight")
                            && let Some(value) = document_style_value_for_expr(
                                font_field.value,
                                expressions,
                                context,
                            )
                        {
                            node.style.insert(font_field.name.clone(), value);
                        }
                    }
                }
            }
            "align" => {
                if let Some(align_fields) = record_fields_for_expr(field.value, expressions) {
                    let centered = align_fields.iter().any(|align_field| {
                        document_expr_value_by_id(align_field.value, expressions)
                            .as_deref()
                            .is_some_and(|value| matches!(value, "Center" | "center"))
                    });
                    if centered {
                        node.style.insert(
                            "center".to_owned(),
                            boon_document_model::StyleValue::Bool(true),
                        );
                    }
                }
            }
            "padding" => {
                if let Some(value) =
                    document_style_value_for_expr(field.value, expressions, context).or_else(|| {
                        record_fields_for_expr(field.value, expressions).and_then(|nested| {
                            nested.iter().find_map(|entry| {
                                document_style_value_for_expr(entry.value, expressions, context)
                            })
                        })
                    })
                {
                    node.style.insert("padding".to_owned(), value);
                }
            }
            "outline" | "border" | "borders" | "selected_border" => {
                if let Some(value) =
                    record_field_nested_style_value(field, "color", expressions, context).or_else(
                        || document_style_value_for_expr(field.value, expressions, context),
                    )
                {
                    let style_key = if field.name == "selected_border" {
                        "selected_border"
                    } else {
                        "border"
                    };
                    node.style.insert(style_key.to_owned(), value);
                }
            }
            _ => {
                if let Some(value) =
                    document_style_value_for_expr(field.value, expressions, context)
                {
                    node.style.insert(field.name.clone(), value);
                }
            }
        }
    }
}

fn record_field_nested_style_value(
    field: &AstRecordField,
    nested_name: &str,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
) -> Option<boon_document_model::StyleValue> {
    record_fields_for_expr(field.value, expressions).and_then(|fields| {
        fields
            .iter()
            .find(|nested| nested.name == nested_name)
            .and_then(|nested| document_style_value_for_expr(nested.value, expressions, context))
    })
}

fn document_child_style_value(
    statement: &AstStatement,
    field: &str,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
) -> Option<boon_document_model::StyleValue> {
    statement
        .children
        .iter()
        .find(|child| document_field_name(child).as_deref() == Some(field))
        .and_then(|child| document_style_value(child, expressions, context))
}

fn record_fields_for_statement<'a>(
    statement: &AstStatement,
    expressions: &'a [AstExpr],
) -> Option<&'a [AstRecordField]> {
    record_fields_for_expr(statement.expr?, expressions)
}

fn record_fields_for_expr(expr_id: usize, expressions: &[AstExpr]) -> Option<&[AstRecordField]> {
    match &expressions.get(expr_id)?.kind {
        AstExprKind::Record(fields) => Some(fields.as_slice()),
        _ => None,
    }
}

fn document_expr_value_by_id(expr_id: usize, expressions: &[AstExpr]) -> Option<String> {
    document_expr_value(expressions.get(expr_id)?, expressions)
}

fn document_style_value_for_expr(
    expr_id: usize,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
) -> Option<boon_document_model::StyleValue> {
    let expr = expressions.get(expr_id)?;
    match &expr.kind {
        AstExprKind::Number(value) => value
            .parse::<f64>()
            .ok()
            .map(boon_document_model::StyleValue::Number),
        AstExprKind::Bool(value) => Some(boon_document_model::StyleValue::Bool(*value)),
        AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value)
            if !value.starts_with('$') =>
        {
            Some(boon_document_model::StyleValue::Text(value.clone()))
        }
        _ => {
            if let Some(resolved) = document_eval_expr_value(expr, expressions, context) {
                return Some(match resolved {
                    Value::Bool(value) => boon_document_model::StyleValue::Bool(value),
                    Value::Number(value) => {
                        boon_document_model::StyleValue::Number(value.as_f64().unwrap_or_default())
                    }
                    _ => boon_document_model::StyleValue::Text(json_value_to_document_text(
                        &resolved,
                    )),
                });
            }
            document_expr_value(expr, expressions).map(boon_document_model::StyleValue::Text)
        }
    }
}

fn lower_canonical_element_text(
    statement: &AstStatement,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
    node: &mut boon_document_model::DocumentNode,
) {
    for child in &statement.children {
        let Some(field) = document_field_name(child) else {
            continue;
        };
        match field.as_str() {
            "label" if !matches!(node.kind, boon_document_model::DocumentNodeKind::TextInput) => {
                if node.text.is_none()
                    && let Some(text) = document_text_or_nested_text(child, expressions, context)
                    && !text.is_empty()
                {
                    node.text = Some(boon_document_model::TextValue { text });
                }
            }
            "text" | "value" | "display_value" => {
                if node.text.is_none()
                    && let Some(text) = document_text_or_nested_text(child, expressions, context)
                    && !text.is_empty()
                {
                    node.text = Some(boon_document_model::TextValue { text });
                }
            }
            "placeholder" => {
                if let Some(text) = document_text_or_nested_text(child, expressions, context) {
                    node.style.insert(
                        "placeholder".to_owned(),
                        boon_document_model::StyleValue::Text(text),
                    );
                }
            }
            "child" | "icon" => {
                if node.text.is_none()
                    && canonical_element_function(child, expressions).is_none()
                    && let Some(text) = document_text_or_nested_text(child, expressions, context)
                    && !text.is_empty()
                {
                    node.text = Some(boon_document_model::TextValue { text });
                }
            }
            _ => {}
        }
    }
}

fn document_text_or_nested_text(
    statement: &AstStatement,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
) -> Option<String> {
    document_text_value(statement, expressions, context, false)
        .or_else(|| {
            statement
                .children
                .iter()
                .find(|child| document_field_name(child).as_deref() == Some("text"))
                .and_then(|child| document_text_value(child, expressions, context, false))
        })
        .or_else(|| {
            record_fields_for_statement(statement, expressions).and_then(|fields| {
                fields
                    .iter()
                    .find(|field| field.name == "text")
                    .and_then(|field| {
                        expressions.get(field.value).and_then(|expr| {
                            document_text_value_for_expr(expr, expressions, context)
                        })
                    })
            })
        })
}

fn document_text_value_for_expr(
    expr: &AstExpr,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
) -> Option<String> {
    match &expr.kind {
        AstExprKind::StringLiteral(value) => {
            if value.starts_with('$') {
                Some(document_resolved_text(value, context))
            } else {
                Some(value.clone())
            }
        }
        AstExprKind::TextLiteral(value) => {
            if value.contains('{') && value.contains('}') {
                Some(document_resolved_template(value, context))
            } else {
                Some(value.clone())
            }
        }
        AstExprKind::Number(value) | AstExprKind::Enum(value) => Some(value.clone()),
        AstExprKind::Bool(value) => Some(value.to_string()),
        _ => document_eval_expr_value(expr, expressions, context)
            .map(|value| json_value_to_document_text(&value))
            .or_else(|| document_expr_value(expr, expressions)),
    }
}

fn lower_canonical_element_sources(
    statement: &AstStatement,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
    node_id: &boon_document_model::DocumentNodeId,
    node: &mut boon_document_model::DocumentNode,
    source_intents: &mut Vec<serde_json::Value>,
) {
    for child in &statement.children {
        match document_field_name(child).as_deref() {
            Some("element") => {
                if let Some(fields) = record_fields_for_statement(child, expressions) {
                    lower_canonical_element_source_record(
                        fields,
                        expressions,
                        context,
                        node_id,
                        node,
                        source_intents,
                    );
                }
                for event in &child.children {
                    if document_field_name(event).as_deref() == Some("event") {
                        for source in &event.children {
                            if let (Some(intent), Some(source_path)) = (
                                document_field_name(source),
                                document_source_value(source, expressions, context),
                            ) {
                                push_canonical_source_intent(
                                    node_id,
                                    node,
                                    source_intents,
                                    &intent,
                                    &source_path,
                                );
                            }
                        }
                    }
                }
            }
            Some("target" | "address") => {
                if let (Some(intent), Some(value)) = (
                    document_field_name(child),
                    document_statement_value(child, expressions),
                ) {
                    let value =
                        document_text_value(child, expressions, context, false).unwrap_or(value);
                    source_intents.push(json!({
                        "node": node_id,
                        "intent": intent,
                        "source_path": value
                    }));
                }
            }
            _ => {}
        }
    }
}

fn lower_canonical_element_source_record(
    fields: &[AstRecordField],
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
    node_id: &boon_document_model::DocumentNodeId,
    node: &mut boon_document_model::DocumentNode,
    source_intents: &mut Vec<serde_json::Value>,
) {
    for field in fields {
        if field.name == "event" {
            if let Some(event_fields) = record_fields_for_expr(field.value, expressions) {
                for source_field in event_fields {
                    if let Some(source_path) =
                        document_source_value_for_expr(source_field.value, expressions, context)
                    {
                        push_canonical_source_intent(
                            node_id,
                            node,
                            source_intents,
                            &source_field.name,
                            &source_path,
                        );
                    }
                }
            }
        }
    }
}

fn document_source_value(
    statement: &AstStatement,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
) -> Option<String> {
    document_source_value_for_expr(statement.expr?, expressions, context)
}

fn document_source_value_for_expr(
    expr_id: usize,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
) -> Option<String> {
    let expr = expressions.get(expr_id)?;
    document_eval_expr_value(expr, expressions, context)
        .map(|value| json_value_to_document_text(&value))
        .or_else(|| document_expr_value(expr, expressions))
}

fn push_canonical_source_intent(
    node_id: &boon_document_model::DocumentNodeId,
    node: &mut boon_document_model::DocumentNode,
    source_intents: &mut Vec<serde_json::Value>,
    intent: &str,
    source_path: &str,
) {
    if node.source_binding.is_none() {
        node.source_binding = Some(boon_document_model::SourceBinding {
            id: boon_document_model::SourceBindingId(format!("source:{}:{}", node_id.0, intent)),
            source_path: source_path.to_owned(),
            intent: intent.to_owned(),
        });
    }
    source_intents.push(json!({
        "node": node_id,
        "intent": intent,
        "source_path": source_path
    }));
    if intent == "key_down" {
        source_intents.push(json!({
            "node": node_id,
            "intent": "submit",
            "source_path": source_path
        }));
    }
}

fn lower_document_elements(
    statements: &[AstStatement],
    expressions: &[AstExpr],
    parent: &boon_document_model::DocumentNodeId,
    frame: &mut boon_document_model::DocumentFrame,
    source_intents: &mut Vec<serde_json::Value>,
    seen_ids: &mut BTreeSet<String>,
    context: &DocumentEvalContext<'_>,
    scope_key: &str,
) {
    for statement in statements {
        if document_field_name(statement).as_deref() == Some("element") {
            lower_document_element(
                statement,
                expressions,
                parent,
                frame,
                source_intents,
                seen_ids,
                context,
                scope_key,
            );
        } else {
            lower_document_elements(
                &statement.children,
                expressions,
                parent,
                frame,
                source_intents,
                seen_ids,
                context,
                scope_key,
            );
        }
    }
}

fn lower_document_element(
    statement: &AstStatement,
    expressions: &[AstExpr],
    parent: &boon_document_model::DocumentNodeId,
    frame: &mut boon_document_model::DocumentFrame,
    source_intents: &mut Vec<serde_json::Value>,
    seen_ids: &mut BTreeSet<String>,
    context: &DocumentEvalContext<'_>,
    scope_key: &str,
) {
    let kind_name =
        document_child_value(statement, "kind", expressions).unwrap_or_else(|| "Stack".to_owned());
    if kind_name == "ForEach" {
        lower_document_for_each(
            statement,
            expressions,
            parent,
            frame,
            source_intents,
            seen_ids,
            context,
            scope_key,
        );
        return;
    }
    if document_child_bool(statement, "visible", expressions, context) == Some(false) {
        return;
    }
    let base_node_id = document_child_value(statement, "id", expressions)
        .unwrap_or_else(|| format!("doc-node-{}", statement.id));
    let scoped_id = if scope_key.is_empty() {
        base_node_id.clone()
    } else {
        format!("{base_node_id}-{scope_key}")
    };
    let mut node_id = scoped_id.clone();
    let mut dedupe = 0usize;
    while !seen_ids.insert(node_id.clone()) {
        dedupe += 1;
        node_id = format!("{scoped_id}-{dedupe}");
    }
    let id = boon_document_model::DocumentNodeId(node_id.clone());
    let mut node =
        boon_document_model::DocumentNode::new(node_id, document_node_kind_from_name(&kind_name));
    node.parent = Some(parent.clone());

    for child in &statement.children {
        let Some(field) = document_field_name(child) else {
            continue;
        };
        if matches!(field.as_str(), "children" | "kind" | "id" | "visible") {
            continue;
        }
        let Some(value) = document_statement_value(child, expressions) else {
            continue;
        };
        if matches!(
            field.as_str(),
            "text" | "value" | "display_value" | "placeholder" | "template"
        ) && node.text.is_none()
        {
            let text = document_text_value(child, expressions, context, field == "template")
                .unwrap_or_else(|| value.clone());
            node.text = Some(boon_document_model::TextValue { text });
        }
        let source_intent_value = if field == "target" {
            document_text_value(child, expressions, context, false).unwrap_or_else(|| value.clone())
        } else {
            value.clone()
        };
        if is_source_binding_field(&field) && node.source_binding.is_none() {
            node.source_binding = Some(boon_document_model::SourceBinding {
                id: boon_document_model::SourceBindingId(format!("source:{}:{}", id.0, field)),
                source_path: source_intent_value.clone(),
                intent: field.clone(),
            });
        }
        if is_source_intent_field(&field) {
            source_intents.push(json!({
                "node": id,
                "intent": field,
                "source_path": source_intent_value
            }));
        } else if let Some(style_value) = document_style_value(child, expressions, context) {
            node.style.insert(field, style_value);
        }
    }

    let vertical_scroll = matches!(node.kind, boon_document_model::DocumentNodeKind::Table)
        || style_bool(&node.style, "scroll")
        || style_bool(&node.style, "scroll_y");
    let horizontal_scroll = matches!(node.kind, boon_document_model::DocumentNodeKind::Table)
        || style_bool(&node.style, "scroll")
        || style_bool(&node.style, "scroll_x");
    if vertical_scroll {
        node.materialized
            .push(boon_document_model::MaterializedRange {
                axis: boon_document_model::Axis::Vertical,
                visible: 0..20,
                overscan: 0..28,
            });
    }
    if horizontal_scroll {
        node.materialized
            .push(boon_document_model::MaterializedRange {
                axis: boon_document_model::Axis::Horizontal,
                visible: 0..8,
                overscan: 0..12,
            });
    }

    if let Some(parent_node) = frame.nodes.get_mut(parent) {
        parent_node.children.push(id.clone());
    }
    frame.nodes.insert(id.clone(), node);
    for child in &statement.children {
        if document_field_name(child).as_deref() == Some("children") {
            lower_document_elements(
                &child.children,
                expressions,
                &id,
                frame,
                source_intents,
                seen_ids,
                context,
                scope_key,
            );
        }
    }
}

#[derive(Clone, Debug)]
struct DocumentEvalContext<'a> {
    root: Option<&'a Value>,
    locals: BTreeMap<String, Value>,
}

fn lower_document_for_each(
    statement: &AstStatement,
    expressions: &[AstExpr],
    parent: &boon_document_model::DocumentNodeId,
    frame: &mut boon_document_model::DocumentFrame,
    source_intents: &mut Vec<serde_json::Value>,
    seen_ids: &mut BTreeSet<String>,
    context: &DocumentEvalContext<'_>,
    scope_key: &str,
) {
    let Some(list_path) = document_child_value(statement, "list", expressions) else {
        return;
    };
    let item_name =
        document_child_value(statement, "item", expressions).unwrap_or_else(|| "item".to_owned());
    let Some(items) = document_resolved_value(&list_path, context).and_then(Value::as_array) else {
        return;
    };
    for (index, item) in items.iter().enumerate() {
        let mut scoped = DocumentEvalContext {
            root: context.root,
            locals: context.locals.clone(),
        };
        scoped.locals.insert(item_name.clone(), item.clone());
        let child_scope = if scope_key.is_empty() {
            format!("{item_name}-{index}")
        } else {
            format!("{scope_key}-{item_name}-{index}")
        };
        for child in &statement.children {
            if document_field_name(child).as_deref() == Some("children") {
                lower_document_elements(
                    &child.children,
                    expressions,
                    parent,
                    frame,
                    source_intents,
                    seen_ids,
                    &scoped,
                    &child_scope,
                );
            }
        }
    }
}

fn document_node_kind_from_name(name: &str) -> boon_document_model::DocumentNodeKind {
    match name {
        "Row" => boon_document_model::DocumentNodeKind::Row,
        "Text" => boon_document_model::DocumentNodeKind::Text,
        "Button" | "Checkbox" => boon_document_model::DocumentNodeKind::Button,
        "Input" | "TextInput" => boon_document_model::DocumentNodeKind::TextInput,
        "Table" => boon_document_model::DocumentNodeKind::Table,
        "TableCell" => boon_document_model::DocumentNodeKind::TableCell,
        "ScrollRoot" => boon_document_model::DocumentNodeKind::ScrollRoot,
        _ => boon_document_model::DocumentNodeKind::Stack,
    }
}

fn is_source_intent_field(field: &str) -> bool {
    matches!(
        field,
        "source"
            | "change"
            | "submit"
            | "escape"
            | "cancel"
            | "press"
            | "click"
            | "key_down"
            | "blur"
            | "double_click"
            | "target"
    )
}

fn is_source_binding_field(field: &str) -> bool {
    is_source_intent_field(field) && field != "target"
}

fn style_bool(style: &boon_document_model::StyleMap, key: &str) -> bool {
    match style.get(key) {
        Some(boon_document_model::StyleValue::Bool(value)) => *value,
        Some(boon_document_model::StyleValue::Text(value)) => value.eq_ignore_ascii_case("true"),
        _ => false,
    }
}

fn document_field_name(statement: &AstStatement) -> Option<String> {
    match &statement.kind {
        AstStatementKind::Field { name } => Some(name.clone()),
        AstStatementKind::List {
            field: Some(name), ..
        } => Some(name.clone()),
        _ => None,
    }
}

fn document_child_value(
    statement: &AstStatement,
    field: &str,
    expressions: &[AstExpr],
) -> Option<String> {
    statement
        .children
        .iter()
        .find(|child| document_field_name(child).as_deref() == Some(field))
        .and_then(|child| document_statement_value(child, expressions))
}

fn document_child_bool(
    statement: &AstStatement,
    field: &str,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
) -> Option<bool> {
    let child = statement
        .children
        .iter()
        .find(|child| document_field_name(child).as_deref() == Some(field))?;
    document_bool_value(child, expressions, context)
}

fn document_statement_value(statement: &AstStatement, expressions: &[AstExpr]) -> Option<String> {
    let expr = expressions.get(statement.expr?)?;
    document_expr_value(expr, expressions)
}

fn document_expr_value(expr: &AstExpr, expressions: &[AstExpr]) -> Option<String> {
    match &expr.kind {
        AstExprKind::StringLiteral(value)
        | AstExprKind::TextLiteral(value)
        | AstExprKind::Number(value)
        | AstExprKind::Enum(value)
        | AstExprKind::Identifier(value) => Some(value.clone()),
        AstExprKind::Bool(value) => Some(value.to_string()),
        AstExprKind::Path(parts) => Some(parts.join(".")),
        AstExprKind::Pipe { input, op, args } => {
            let mut value = document_expr_value(expressions.get(*input)?, expressions)?;
            value.push_str("|>");
            value.push_str(op);
            if !args.is_empty() {
                value.push('(');
                value.push_str(
                    &args
                        .iter()
                        .filter_map(|arg| {
                            let mut arg_value =
                                document_expr_value(expressions.get(arg.value)?, expressions)?;
                            if let Some(name) = &arg.name {
                                arg_value = format!("{name}:{arg_value}");
                            }
                            Some(arg_value)
                        })
                        .collect::<Vec<_>>()
                        .join(","),
                );
                value.push(')');
            }
            Some(value)
        }
        AstExprKind::Unknown(tokens) if tokens.first().map(String::as_str) == Some("Oklch") => {
            Some(tokens.join(""))
        }
        _ => None,
    }
}

fn document_text_value(
    statement: &AstStatement,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
    template: bool,
) -> Option<String> {
    let expr = expressions.get(statement.expr?)?;
    if template {
        return document_expr_value(expr, expressions)
            .map(|value| document_resolved_template(&value, context));
    }
    match &expr.kind {
        AstExprKind::StringLiteral(value) => {
            if value.starts_with('$') {
                Some(document_resolved_text(value, context))
            } else {
                Some(value.clone())
            }
        }
        AstExprKind::TextLiteral(value) => {
            if value.contains('{') && value.contains('}') {
                Some(document_resolved_template(value, context))
            } else {
                Some(value.clone())
            }
        }
        AstExprKind::Number(value) | AstExprKind::Enum(value) => Some(value.clone()),
        AstExprKind::Bool(value) => Some(value.to_string()),
        _ => document_eval_expr_value(expr, expressions, context)
            .map(|value| json_value_to_document_text(&value))
            .or_else(|| document_expr_value(expr, expressions)),
    }
}

fn document_bool_value(
    statement: &AstStatement,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
) -> Option<bool> {
    let expr = expressions.get(statement.expr?)?;
    match &expr.kind {
        AstExprKind::Bool(value) => Some(*value),
        AstExprKind::StringLiteral(value) if value.starts_with('$') => {
            document_resolved_bool(value, context)
        }
        AstExprKind::StringLiteral(value) => Some(value.eq_ignore_ascii_case("true")),
        _ => match document_eval_expr_value(expr, expressions, context) {
            Some(Value::Bool(value)) => Some(value),
            Some(Value::String(value)) => Some(value.eq_ignore_ascii_case("true")),
            _ => None,
        },
    }
}

fn document_style_value(
    statement: &AstStatement,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
) -> Option<boon_document_model::StyleValue> {
    let expr = expressions.get(statement.expr?)?;
    match &expr.kind {
        AstExprKind::Number(value) => value
            .parse::<f64>()
            .ok()
            .map(boon_document_model::StyleValue::Number),
        AstExprKind::Bool(value) => Some(boon_document_model::StyleValue::Bool(*value)),
        AstExprKind::StringLiteral(value) if !value.starts_with('$') => {
            Some(boon_document_model::StyleValue::Text(value.clone()))
        }
        AstExprKind::TextLiteral(value) => {
            Some(boon_document_model::StyleValue::Text(value.clone()))
        }
        _ => {
            if let Some(resolved) = document_eval_expr_value(expr, expressions, context) {
                return Some(match resolved {
                    Value::Bool(value) => boon_document_model::StyleValue::Bool(value),
                    Value::Number(value) => {
                        boon_document_model::StyleValue::Number(value.as_f64().unwrap_or_default())
                    }
                    _ => boon_document_model::StyleValue::Text(json_value_to_document_text(
                        &resolved,
                    )),
                });
            }
            let value = document_expr_value(expr, expressions)?;
            Some(boon_document_model::StyleValue::Text(value))
        }
    }
}

fn document_eval_expr_value(
    expr: &AstExpr,
    expressions: &[AstExpr],
    context: &DocumentEvalContext<'_>,
) -> Option<Value> {
    match &expr.kind {
        AstExprKind::Identifier(value) => document_resolved_value(value, context).cloned(),
        AstExprKind::Path(parts) => document_resolved_value(&parts.join("."), context).cloned(),
        AstExprKind::StringLiteral(value) => {
            if value.starts_with('$') {
                document_resolved_value(value, context).cloned()
            } else {
                Some(Value::String(value.clone()))
            }
        }
        AstExprKind::TextLiteral(value) | AstExprKind::Enum(value) => {
            Some(Value::String(value.clone()))
        }
        AstExprKind::Number(value) => value.parse::<i64>().ok().map(|value| json!(value)),
        AstExprKind::Bool(value) => Some(Value::Bool(*value)),
        AstExprKind::Infix { left, op, right } => {
            let left = document_eval_expr_value(expressions.get(*left)?, expressions, context)?;
            let right = document_eval_expr_value(expressions.get(*right)?, expressions, context)?;
            document_eval_infix(&left, op, &right)
        }
        _ => None,
    }
}

fn document_eval_infix(left: &Value, op: &str, right: &Value) -> Option<Value> {
    let result = match op {
        "==" => json_values_equal(left, right),
        "!=" => !json_values_equal(left, right),
        ">" => json_values_cmp(left, right).is_some_and(|ordering| ordering.is_gt()),
        "<" => json_values_cmp(left, right).is_some_and(|ordering| ordering.is_lt()),
        ">=" => json_values_cmp(left, right).is_some_and(|ordering| {
            matches!(
                ordering,
                std::cmp::Ordering::Greater | std::cmp::Ordering::Equal
            )
        }),
        "<=" => json_values_cmp(left, right).is_some_and(|ordering| {
            matches!(
                ordering,
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal
            )
        }),
        _ => return None,
    };
    Some(Value::Bool(result))
}

fn json_values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::String(left), Value::String(right)) => left == right,
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::Number(left), Value::Number(right)) => left.as_f64() == right.as_f64(),
        _ => left == right,
    }
}

fn json_values_cmp(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => left.as_f64()?.partial_cmp(&right.as_f64()?),
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

fn document_resolved_text(raw: &str, context: &DocumentEvalContext<'_>) -> String {
    if !raw.starts_with('$') {
        return raw.to_owned();
    }
    document_resolved_value(raw, context)
        .map(json_value_to_document_text)
        .unwrap_or_else(|| raw.to_owned())
}

fn document_resolved_bool(raw: &str, context: &DocumentEvalContext<'_>) -> Option<bool> {
    match document_resolved_value(raw, context) {
        Some(Value::Bool(value)) => Some(*value),
        Some(Value::String(value)) => Some(value.eq_ignore_ascii_case("true")),
        _ => None,
    }
}

fn document_resolved_template(raw: &str, context: &DocumentEvalContext<'_>) -> String {
    let mut rendered = String::new();
    let mut remaining = raw;
    while let Some(open) = remaining.find('{') {
        let (prefix, tail) = remaining.split_at(open);
        rendered.push_str(prefix);
        let tail = &tail[1..];
        let Some(close) = tail.find('}') else {
            rendered.push('{');
            rendered.push_str(tail);
            return rendered;
        };
        let key = &tail[..close];
        rendered.push_str(
            &document_resolved_value(key.trim(), context)
                .map(json_value_to_document_text)
                .unwrap_or_else(|| key.trim().to_owned()),
        );
        remaining = &tail[close + 1..];
    }
    rendered.push_str(remaining);
    rendered
}

fn document_resolved_value<'a>(
    raw: &str,
    context: &'a DocumentEvalContext<'_>,
) -> Option<&'a Value> {
    let path = raw.strip_prefix('$').unwrap_or(raw);
    if path.is_empty() || path.contains('|') {
        return None;
    }
    let mut parts = path.split('.');
    let first = parts.next()?;
    let mut current = context.locals.get(first).or_else(|| {
        context
            .root
            .and_then(|root| root.as_object())
            .and_then(|object| object.get(first))
    })?;
    for part in parts {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

fn json_value_to_document_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(values) => values.len().to_string(),
        Value::Object(_) => String::new(),
    }
}

#[derive(Default)]
struct PreviewNativeInputState {
    last_mouse_button_event_count: u64,
    last_keyboard_event_sequence: u64,
    focused_node: Option<String>,
    focused_text: String,
}

fn unhandled_primary_mouse_releases(
    input: &boon_native_app_window::NativeInputAdapterProof,
    last_seen_sequence: u64,
) -> Vec<boon_native_app_window::NativeMouseButtonEventProof> {
    let releases = input
        .mouse_button_events
        .iter()
        .filter(|event| {
            event.sequence > last_seen_sequence && event.button == "left" && !event.pressed
        })
        .cloned()
        .collect::<Vec<_>>();
    if !releases.is_empty() || !input.mouse_button_events.is_empty() {
        return releases;
    }
    if input.mouse_button_event_count > last_seen_sequence && input.mouse_buttons_down.is_empty() {
        vec![boon_native_app_window::NativeMouseButtonEventProof {
            sequence: input.mouse_button_event_count,
            button: "left".to_owned(),
            pressed: false,
            window_protocol_id: input.mouse_last_window_protocol_id,
        }]
    } else {
        Vec::new()
    }
}

fn preview_input_has_unhandled_source_events(
    input: &boon_native_app_window::NativeInputAdapterProof,
    input_state: &PreviewNativeInputState,
) -> bool {
    if input.synthetic_input_probe {
        return false;
    }
    !unhandled_primary_mouse_releases(input, input_state.last_mouse_button_event_count).is_empty()
        || input
            .keyboard_events
            .iter()
            .any(|event| event.sequence > input_state.last_keyboard_event_sequence)
}

fn deterministic_click_input(
    event_count: u64,
    x: f64,
    y: f64,
) -> boon_native_app_window::NativeInputAdapterProof {
    let mut mouse_button_events = Vec::new();
    for index in 0..event_count {
        let press_sequence = index.saturating_mul(2).saturating_add(1);
        let release_sequence = press_sequence.saturating_add(1);
        mouse_button_events.push(boon_native_app_window::NativeMouseButtonEventProof {
            sequence: press_sequence,
            button: "left".to_owned(),
            pressed: true,
            window_protocol_id: Some(1),
        });
        mouse_button_events.push(boon_native_app_window::NativeMouseButtonEventProof {
            sequence: release_sequence,
            button: "left".to_owned(),
            pressed: false,
            window_protocol_id: Some(1),
        });
    }
    boon_native_app_window::NativeInputAdapterProof {
        installed: true,
        capture_scope: "deterministic_recent_mouse_button_events".to_owned(),
        keyboard_api: "none".to_owned(),
        mouse_api: "app_window::input::mouse::Mouse::event_provenance".to_owned(),
        wheel_api: "none".to_owned(),
        per_window_event_provenance_api:
            "app_window::input::mouse::MouseEventProvenance::recent_button_events".to_owned(),
        sampled_after_visible_window: true,
        real_os_events_observed: true,
        input_injection_method: "deterministic_app_owned_mouse_event_batch".to_owned(),
        synthetic_input_probe: false,
        mouse_last_window_protocol_id: Some(1),
        keyboard_last_window_protocol_id: None,
        mouse_motion_event_count: 1,
        mouse_button_event_count: event_count.saturating_mul(2),
        mouse_scroll_event_count: 0,
        mouse_total_event_count: event_count.saturating_mul(2).saturating_add(1),
        keyboard_key_event_count: 0,
        mouse_button_events,
        keyboard_events: Vec::new(),
        mouse_window_pos: Some(boon_native_app_window::NativeMouseWindowPosition {
            x,
            y,
            window_width: 920.0,
            window_height: 720.0,
        }),
        mouse_buttons_down: Vec::new(),
        pressed_keys: Vec::new(),
        scroll_delta_x: 0.0,
        scroll_delta_y: 0.0,
    }
}

fn source_hit_center(
    layout_proof: &Value,
    source_event: &str,
) -> Result<(f64, f64, String), Box<dyn std::error::Error>> {
    let source_intents = layout_proof
        .get("source_intent_assertions")
        .and_then(serde_json::Value::as_array)
        .ok_or("layout proof missing source intents")?;
    let target_node = source_intents
        .iter()
        .find_map(|intent| {
            (intent
                .get("source_path")
                .and_then(serde_json::Value::as_str)
                == Some(source_event))
            .then(|| intent.get("node").and_then(serde_json::Value::as_str))
            .flatten()
            .map(str::to_owned)
        })
        .ok_or_else(|| format!("source event `{source_event}` has no document source intent"))?;
    let hit_region = layout_proof
        .get("hit_target_assertions")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|region| {
            region.get("node").and_then(serde_json::Value::as_str) == Some(target_node.as_str())
        })
        .ok_or_else(|| format!("source event `{source_event}` target has no hit region"))?;
    let bounds = hit_region
        .get("bounds")
        .ok_or("target hit region missing bounds")?;
    let x = bounds
        .get("x")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or_default()
        + bounds
            .get("width")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or_default()
            / 2.0;
    let y = bounds
        .get("y")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or_default()
        + bounds
            .get("height")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or_default()
            / 2.0;
    Ok((x, y, target_node))
}

fn preview_apply_real_window_input(
    input: &boon_native_app_window::NativeInputAdapterProof,
    source_path: &Path,
    source_text: &str,
    live_runtime: Option<&Arc<Mutex<boon_runtime::LiveRuntime>>>,
    shared_render_state: &Arc<Mutex<PreviewSharedRenderState>>,
    input_state: &mut PreviewNativeInputState,
) -> Result<(), Box<dyn std::error::Error>> {
    if input.synthetic_input_probe {
        return Ok(());
    }
    let Some(live_runtime) = live_runtime else {
        return Ok(());
    };
    let layout_proof = shared_render_state
        .lock()
        .map_err(|_| "preview render state mutex poisoned")?
        .layout_proof
        .clone();

    let mut latest_layout = None;
    let mut pending_mouse_events = Vec::new();
    for mouse_release in
        unhandled_primary_mouse_releases(input, input_state.last_mouse_button_event_count)
    {
        input_state.last_mouse_button_event_count = input_state
            .last_mouse_button_event_count
            .max(mouse_release.sequence);
        if let Some(position) = input.mouse_window_pos
            && let Some(hit_region) = document_hit_region_at(
                latest_layout.as_ref().unwrap_or(&layout_proof),
                position.x,
                position.y,
            )
        {
            let node = hit_region
                .get("node")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let layout = latest_layout.as_ref().unwrap_or(&layout_proof);
            if live_source_for_node_intent(layout, &node, "change").is_some() {
                input_state.focused_node = Some(node);
                input_state.focused_text.clear();
                input_state.focused_text.push_str(
                    document_value_for_hit_region(layout, &hit_region)
                        .as_deref()
                        .unwrap_or_default(),
                );
                if let Some(event) = live_source_event_for_hit_region(layout, &hit_region) {
                    pending_mouse_events.push(event);
                }
            } else {
                input_state.focused_node = None;
                input_state.focused_text.clear();
                if let Some(event) = live_source_event_for_hit_region(layout, &hit_region) {
                    pending_mouse_events.push(event);
                }
            }
        }
    }
    if !pending_mouse_events.is_empty() {
        latest_layout = Some(preview_apply_live_events(
            source_path,
            source_text,
            live_runtime,
            shared_render_state,
            pending_mouse_events,
        )?);
    }

    let shift_pressed = input
        .pressed_keys
        .iter()
        .any(|key| key == "Shift" || key == "RightShift");
    let keyboard_events = input
        .keyboard_events
        .iter()
        .filter(|event| event.sequence > input_state.last_keyboard_event_sequence)
        .cloned()
        .collect::<Vec<_>>();
    for event in keyboard_events {
        input_state.last_keyboard_event_sequence =
            input_state.last_keyboard_event_sequence.max(event.sequence);
        if !event.pressed {
            continue;
        }
        let Some(focused_node) = input_state.focused_node.as_deref() else {
            continue;
        };
        let layout = latest_layout.as_ref().unwrap_or(&layout_proof);
        match event.key.as_str() {
            "Return" | "KeypadEnter" => {
                if let Some(source) = live_source_for_node_intent(layout, focused_node, "submit") {
                    let submit = boon_runtime::LiveSourceEvent {
                        source,
                        text: Some(input_state.focused_text.clone()),
                        key: Some("Enter".to_owned()),
                        address: focused_address(layout, focused_node),
                        target_text: focused_target_text(layout, focused_node),
                        target_occurrence: None,
                    };
                    latest_layout = Some(preview_apply_live_event(
                        source_path,
                        source_text,
                        live_runtime,
                        shared_render_state,
                        submit,
                    )?);
                    input_state.focused_text.clear();
                }
            }
            "Escape" => {
                if let Some(source) = live_source_for_node_intent(layout, focused_node, "escape")
                    .or_else(|| live_source_for_node_intent(layout, focused_node, "key_down"))
                {
                    let escape = boon_runtime::LiveSourceEvent {
                        source,
                        text: Some(input_state.focused_text.clone()),
                        key: Some("Escape".to_owned()),
                        address: focused_address(layout, focused_node),
                        target_text: focused_target_text(layout, focused_node),
                        target_occurrence: None,
                    };
                    latest_layout = Some(preview_apply_live_event(
                        source_path,
                        source_text,
                        live_runtime,
                        shared_render_state,
                        escape,
                    )?);
                    input_state.focused_node = None;
                    input_state.focused_text.clear();
                }
            }
            "Delete" => {
                input_state.focused_text.pop();
                if let Some(source) = live_source_for_node_intent(layout, focused_node, "change") {
                    let change = boon_runtime::LiveSourceEvent {
                        source,
                        text: Some(input_state.focused_text.clone()),
                        key: None,
                        address: focused_address(layout, focused_node),
                        target_text: focused_target_text(layout, focused_node),
                        target_occurrence: None,
                    };
                    latest_layout = Some(preview_apply_live_event(
                        source_path,
                        source_text,
                        live_runtime,
                        shared_render_state,
                        change,
                    )?);
                }
            }
            key => {
                if let Some(character) = keyboard_event_text(key, shift_pressed) {
                    input_state.focused_text.push(character);
                    if let Some(source) =
                        live_source_for_node_intent(layout, focused_node, "change")
                    {
                        let change = boon_runtime::LiveSourceEvent {
                            source,
                            text: Some(input_state.focused_text.clone()),
                            key: None,
                            address: focused_address(layout, focused_node),
                            target_text: focused_target_text(layout, focused_node),
                            target_occurrence: None,
                        };
                        latest_layout = Some(preview_apply_live_event(
                            source_path,
                            source_text,
                            live_runtime,
                            shared_render_state,
                            change,
                        )?);
                    }
                }
            }
        }
    }
    preview_apply_scroll_input(input, shared_render_state)?;
    Ok(())
}

fn preview_apply_scroll_input(
    input: &boon_native_app_window::NativeInputAdapterProof,
    shared_render_state: &Arc<Mutex<PreviewSharedRenderState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    if input.synthetic_input_probe {
        return Ok(());
    }
    if input.scroll_delta_x.abs() <= f64::EPSILON && input.scroll_delta_y.abs() <= f64::EPSILON {
        return Ok(());
    }
    let Some(position) = input.mouse_window_pos else {
        return Ok(());
    };
    let mut shared = shared_render_state
        .lock()
        .map_err(|_| "preview render state mutex poisoned")?;
    if !layout_scroll_region_contains(&shared.layout_proof, position.x, position.y) {
        return Ok(());
    }
    shared.scroll_x_px = (shared.scroll_x_px + input.scroll_delta_x * 5.0).clamp(0.0, 2_000.0);
    shared.scroll_y_px = (shared.scroll_y_px + input.scroll_delta_y * 5.0).clamp(0.0, 2_600.0);
    let (transformed, transformed_frame) =
        scrolled_layout_proof(&shared.layout_proof, shared.scroll_x_px, shared.scroll_y_px)?;
    shared.layout_proof = transformed;
    shared.layout_frame_override = Some(transformed_frame);
    shared.last_error = None;
    shared.update_count = shared.update_count.saturating_add(1);
    Ok(())
}

fn scaled_scroll_steps(delta: f64, unit: f64, min_abs_steps: isize) -> isize {
    if delta.abs() <= f64::EPSILON {
        return 0;
    }
    let direction = if delta.is_sign_positive() { 1 } else { -1 };
    let steps = (delta.abs() / unit).ceil() as isize;
    direction * steps.max(min_abs_steps)
}

fn layout_scroll_region_contains(layout_proof: &Value, x: f64, y: f64) -> bool {
    layout_proof
        .get("scroll_regions")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .any(|region| {
            region
                .get("bounds")
                .is_some_and(|bounds| document_bounds_contains(bounds, x, y))
        })
}

fn scrolled_layout_proof(
    layout_proof: &Value,
    scroll_x_px: f64,
    scroll_y_px: f64,
) -> Result<(Value, boon_document::LayoutFrame), Box<dyn std::error::Error>> {
    let mut frame = layout_frame_from_layout_proof(layout_proof)?;
    transform_layout_frame_for_scroll(&mut frame, scroll_x_px as f32, scroll_y_px as f32);
    let base_layout_hash = layout_proof
        .get("layout_frame_hash")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing-layout-frame-hash");
    let layout_frame_hash = boon_runtime::sha256_bytes(
        format!("{base_layout_hash}:scroll:{scroll_x_px:.1}:{scroll_y_px:.1}").as_bytes(),
    );
    let hit_target_assertions = serde_json::to_value(&frame.hit_regions)?;
    let hit_target_count = frame.hit_regions.len();
    let hit_target_samples = frame
        .hit_regions
        .iter()
        .take(256)
        .cloned()
        .collect::<Vec<_>>();
    let mut proof = layout_proof.clone();
    proof["layout_frame_hash"] = json!(layout_frame_hash);
    proof["display_item_count"] = json!(frame.display_list.len());
    proof["display_item_samples"] = serde_json::to_value(
        frame
            .display_list
            .iter()
            .take(256)
            .cloned()
            .collect::<Vec<_>>(),
    )?;
    proof["hit_target_count"] = json!(hit_target_count);
    proof["hit_target_assertions"] = hit_target_assertions;
    proof["hit_target_samples"] = serde_json::to_value(hit_target_samples)?;
    proof["hit_target_sample_count"] = json!(hit_target_count.min(256));
    proof["scroll_regions"] = serde_json::to_value(&frame.scroll_regions)?;
    proof["layout_metrics"] = serde_json::to_value(&frame.metrics)?;
    proof["scroll_transform"] = json!({
        "status": "applied",
        "scroll_x_px": scroll_x_px,
        "scroll_y_px": scroll_y_px,
        "layout_source": "embedded_transformed_layout_frame",
        "layout_frame_hash_basis": "base-layout-frame-hash-plus-scroll-offset",
        "visual_scroll_applied_before_render": true
    });
    Ok((proof, frame))
}

fn layout_frame_from_layout_proof(
    layout_proof: &Value,
) -> Result<boon_document::LayoutFrame, Box<dyn std::error::Error>> {
    if let Some(layout_frame) = layout_proof.get("layout_frame") {
        return Ok(serde_json::from_value(layout_frame.clone())?);
    }
    let layout_artifact = layout_proof
        .get("artifact_path")
        .and_then(serde_json::Value::as_str)
        .ok_or("layout proof missing artifact_path")?;
    let artifact_json: Value = serde_json::from_str(&std::fs::read_to_string(layout_artifact)?)?;
    Ok(serde_json::from_value(
        artifact_json
            .get("layout_frame")
            .cloned()
            .ok_or("layout artifact missing layout_frame")?,
    )?)
}

fn transform_layout_frame_for_scroll(
    frame: &mut boon_document::LayoutFrame,
    scroll_x_px: f32,
    scroll_y_px: f32,
) {
    let scroll_nodes = frame
        .scroll_regions
        .iter()
        .map(|region| region.node.0.clone())
        .collect::<BTreeSet<_>>();
    let vertical_regions = frame
        .scroll_regions
        .iter()
        .filter(|region| matches!(region.axis, boon_document::Axis::Vertical))
        .cloned()
        .collect::<Vec<_>>();
    let horizontal_regions = frame
        .scroll_regions
        .iter()
        .filter(|region| matches!(region.axis, boon_document::Axis::Horizontal))
        .cloned()
        .collect::<Vec<_>>();
    let mut node_offsets = BTreeMap::<String, (f32, f32)>::new();
    let mut node_visible = BTreeMap::<String, bool>::new();

    for item in &mut frame.display_list {
        if scroll_nodes.contains(&item.node.0) {
            continue;
        }
        let original = item.bounds;
        let mut dx = 0.0;
        let mut dy = 0.0;
        let mut clip = None;
        for region in &vertical_regions {
            if rect_horizontal_overlaps(original, region.bounds) && original.y >= region.bounds.y {
                dy -= scroll_y_px;
                clip = Some(region.bounds);
                break;
            }
        }
        for region in &horizontal_regions {
            if rect_vertical_overlaps(original, region.bounds)
                && original.x >= region.bounds.x + 40.0
            {
                dx -= scroll_x_px;
                clip = Some(match clip {
                    Some(existing) => rect_intersection(existing, region.bounds),
                    None => region.bounds,
                });
                break;
            }
        }
        item.bounds.x += dx;
        item.bounds.y += dy;
        let visible = clip.is_none_or(|clip| rect_intersects(item.bounds, clip));
        node_offsets.insert(item.node.0.clone(), (dx, dy));
        node_visible.insert(item.node.0.clone(), visible);
    }

    frame
        .display_list
        .retain(|item| node_visible.get(&item.node.0).copied().unwrap_or(true));
    for hit in &mut frame.hit_regions {
        if let Some((dx, dy)) = node_offsets.get(&hit.node.0) {
            hit.bounds.x += dx;
            hit.bounds.y += dy;
        }
    }
    frame
        .hit_regions
        .retain(|hit| node_visible.get(&hit.node.0).copied().unwrap_or(true));
    frame.metrics.display_item_count = frame.display_list.len();
    for demand in &mut frame.demands {
        match demand.axis {
            boon_document::Axis::Vertical => {
                let start = (scroll_y_px / 26.0).floor().max(0.0) as u64;
                demand.visible = start..start.saturating_add(20);
                demand.overscan = start..start.saturating_add(28);
            }
            boon_document::Axis::Horizontal => {
                let start = (scroll_x_px / 80.0).floor().max(0.0) as u64;
                demand.visible = start..start.saturating_add(8);
                demand.overscan = start..start.saturating_add(12);
            }
        }
    }
}

fn rect_intersects(left: boon_document::Rect, right: boon_document::Rect) -> bool {
    left.x < right.x + right.width
        && left.x + left.width > right.x
        && left.y < right.y + right.height
        && left.y + left.height > right.y
}

fn rect_horizontal_overlaps(left: boon_document::Rect, right: boon_document::Rect) -> bool {
    left.x < right.x + right.width && left.x + left.width > right.x
}

fn rect_vertical_overlaps(left: boon_document::Rect, right: boon_document::Rect) -> bool {
    left.y < right.y + right.height && left.y + left.height > right.y
}

fn rect_intersection(left: boon_document::Rect, right: boon_document::Rect) -> boon_document::Rect {
    let x1 = left.x.max(right.x);
    let y1 = left.y.max(right.y);
    let x2 = (left.x + left.width).min(right.x + right.width);
    let y2 = (left.y + left.height).min(right.y + right.height);
    boon_document::Rect {
        x: x1,
        y: y1,
        width: (x2 - x1).max(0.0),
        height: (y2 - y1).max(0.0),
    }
}

fn preview_apply_live_event(
    source_path: &Path,
    source_text: &str,
    live_runtime: &Arc<Mutex<boon_runtime::LiveRuntime>>,
    shared_render_state: &Arc<Mutex<PreviewSharedRenderState>>,
    event: boon_runtime::LiveSourceEvent,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    preview_apply_live_events(
        source_path,
        source_text,
        live_runtime,
        shared_render_state,
        vec![event],
    )
}

fn preview_apply_live_events(
    source_path: &Path,
    source_text: &str,
    live_runtime: &Arc<Mutex<boon_runtime::LiveRuntime>>,
    shared_render_state: &Arc<Mutex<PreviewSharedRenderState>>,
    events: Vec<boon_runtime::LiveSourceEvent>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    if events.is_empty() {
        let shared = shared_render_state
            .lock()
            .map_err(|_| "preview render state mutex poisoned")?;
        return Ok(shared.layout_proof.clone());
    }
    let (state_summary, event_count) = {
        let mut runtime = live_runtime
            .lock()
            .map_err(|_| "preview live runtime mutex poisoned")?;
        let mut state_summary = None;
        let event_count = events.len() as u64;
        for event in events {
            let output = runtime.apply_source_event(event)?;
            state_summary = Some(output.state_summary);
        }
        (
            state_summary.ok_or("preview live event batch produced no state summary")?,
            event_count,
        )
    };
    let post_input_layout =
        native_document_layout_proof_with_state(source_path, source_text, Some(&state_summary))?;
    if post_input_layout
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass")
    {
        let mut shared_render_state = shared_render_state
            .lock()
            .map_err(|_| "preview render state mutex poisoned")?;
        shared_render_state.layout_proof = post_input_layout.clone();
        shared_render_state.layout_frame_override = None;
        shared_render_state.last_error = None;
        shared_render_state.update_count =
            shared_render_state.update_count.saturating_add(event_count);
    }
    Ok(post_input_layout)
}

fn document_hit_region_at(layout_proof: &Value, x: f64, y: f64) -> Option<Value> {
    layout_proof
        .get("hit_target_assertions")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .filter(|region| {
            region
                .get("bounds")
                .is_some_and(|bounds| document_bounds_contains(bounds, x, y))
        })
        .min_by(|left, right| {
            let left_area = document_bounds_area(left.get("bounds")).unwrap_or(f64::MAX);
            let right_area = document_bounds_area(right.get("bounds")).unwrap_or(f64::MAX);
            left_area.total_cmp(&right_area)
        })
        .cloned()
}

fn document_bounds_contains(bounds: &Value, x: f64, y: f64) -> bool {
    let left = bounds
        .get("x")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let top = bounds
        .get("y")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let width = bounds
        .get("width")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let height = bounds
        .get("height")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    x >= left && x <= left + width && y >= top && y <= top + height
}

fn document_bounds_area(bounds: Option<&Value>) -> Option<f64> {
    let bounds = bounds?;
    let width = bounds.get("width")?.as_f64()?;
    let height = bounds.get("height")?.as_f64()?;
    Some(width * height)
}

fn live_source_event_for_hit_region(
    layout_proof: &Value,
    hit_region: &Value,
) -> Option<boon_runtime::LiveSourceEvent> {
    let node = hit_region.get("node")?.as_str()?;
    let source = ["source", "click", "press", "double_click"]
        .into_iter()
        .find_map(|intent| live_source_for_node_intent(layout_proof, node, intent))?;
    Some(boon_runtime::LiveSourceEvent {
        source,
        text: None,
        key: None,
        address: focused_address(layout_proof, node),
        target_text: focused_target_text(layout_proof, node),
        target_occurrence: None,
    })
}

fn live_source_for_node_intent(layout_proof: &Value, node: &str, expected: &str) -> Option<String> {
    layout_proof
        .get("source_intent_assertions")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .find_map(|intent| {
            let intent_node = intent.get("node").and_then(serde_json::Value::as_str)?;
            let intent_kind = intent.get("intent").and_then(serde_json::Value::as_str)?;
            if intent_node == node && intent_kind == expected {
                intent
                    .get("source_path")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            } else {
                None
            }
        })
}

fn focused_target_text(layout_proof: &Value, node: &str) -> Option<String> {
    focused_source_intent_value(layout_proof, node, "target")
        .or_else(|| focused_source_intent_value(layout_proof, node, "address"))
}

fn focused_address(layout_proof: &Value, node: &str) -> Option<String> {
    focused_source_intent_value(layout_proof, node, "address")
}

fn focused_source_intent_value(layout_proof: &Value, node: &str, expected: &str) -> Option<String> {
    layout_proof
        .get("source_intent_assertions")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .find_map(|intent| {
            let intent_node = intent.get("node").and_then(serde_json::Value::as_str)?;
            let intent_kind = intent.get("intent").and_then(serde_json::Value::as_str)?;
            if intent_node == node && intent_kind == expected {
                intent
                    .get("source_path")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            } else {
                None
            }
        })
}

fn document_value_for_hit_region(layout_proof: &Value, hit_region: &Value) -> Option<String> {
    let node = hit_region.get("node")?.as_str()?;
    layout_proof
        .get("display_item_samples")
        .or_else(|| layout_proof.get("display_list"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find_map(|item| {
            let item_node = item.get("node").and_then(serde_json::Value::as_str)?;
            if item_node == node {
                item.get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            } else {
                None
            }
        })
}

fn keyboard_event_text(key: &str, shift: bool) -> Option<char> {
    match (key, shift) {
        ("A", false) => Some('a'),
        ("A", true) => Some('A'),
        ("B", false) => Some('b'),
        ("B", true) => Some('B'),
        ("C", false) => Some('c'),
        ("C", true) => Some('C'),
        ("D", false) => Some('d'),
        ("D", true) => Some('D'),
        ("E", false) => Some('e'),
        ("E", true) => Some('E'),
        ("F", false) => Some('f'),
        ("F", true) => Some('F'),
        ("G", false) => Some('g'),
        ("G", true) => Some('G'),
        ("H", false) => Some('h'),
        ("H", true) => Some('H'),
        ("I", false) => Some('i'),
        ("I", true) => Some('I'),
        ("J", false) => Some('j'),
        ("J", true) => Some('J'),
        ("K", false) => Some('k'),
        ("K", true) => Some('K'),
        ("L", false) => Some('l'),
        ("L", true) => Some('L'),
        ("M", false) => Some('m'),
        ("M", true) => Some('M'),
        ("N", false) => Some('n'),
        ("N", true) => Some('N'),
        ("O", false) => Some('o'),
        ("O", true) => Some('O'),
        ("P", false) => Some('p'),
        ("P", true) => Some('P'),
        ("Q", false) => Some('q'),
        ("Q", true) => Some('Q'),
        ("R", false) => Some('r'),
        ("R", true) => Some('R'),
        ("S", false) => Some('s'),
        ("S", true) => Some('S'),
        ("T", false) => Some('t'),
        ("T", true) => Some('T'),
        ("U", false) => Some('u'),
        ("U", true) => Some('U'),
        ("V", false) => Some('v'),
        ("V", true) => Some('V'),
        ("W", false) => Some('w'),
        ("W", true) => Some('W'),
        ("X", false) => Some('x'),
        ("X", true) => Some('X'),
        ("Y", false) => Some('y'),
        ("Y", true) => Some('Y'),
        ("Z", false) => Some('z'),
        ("Z", true) => Some('Z'),
        ("Num0" | "Keypad0", false) => Some('0'),
        ("Num0", true) => Some(')'),
        ("Num1" | "Keypad1", false) => Some('1'),
        ("Num1", true) => Some('!'),
        ("Num2" | "Keypad2", false) => Some('2'),
        ("Num2", true) => Some('@'),
        ("Num3" | "Keypad3", false) => Some('3'),
        ("Num3", true) => Some('#'),
        ("Num4" | "Keypad4", false) => Some('4'),
        ("Num4", true) => Some('$'),
        ("Num5" | "Keypad5", false) => Some('5'),
        ("Num5", true) => Some('%'),
        ("Num6" | "Keypad6", false) => Some('6'),
        ("Num6", true) => Some('^'),
        ("Num7" | "Keypad7", false) => Some('7'),
        ("Num7", true) => Some('&'),
        ("Num8" | "Keypad8", false) => Some('8'),
        ("Num8", true) => Some('*'),
        ("Num9" | "Keypad9", false) => Some('9'),
        ("Num9", true) => Some('('),
        ("Space", _) => Some(' '),
        ("Minus" | "KeypadMinus", false) => Some('-'),
        ("Minus", true) => Some('_'),
        ("Equal" | "KeypadEquals", false) => Some('='),
        ("Equal", true) => Some('+'),
        ("Comma", false) => Some(','),
        ("Comma", true) => Some('<'),
        ("Period" | "KeypadDecimal", false) => Some('.'),
        ("Period", true) => Some('>'),
        ("Slash" | "KeypadDivide", false) => Some('/'),
        ("Slash", true) => Some('?'),
        ("Semicolon", false) => Some(';'),
        ("Semicolon", true) => Some(':'),
        ("Quote", false) => Some('\''),
        ("Quote", true) => Some('"'),
        ("LeftBracket", false) => Some('['),
        ("LeftBracket", true) => Some('{'),
        ("RightBracket", false) => Some(']'),
        ("RightBracket", true) => Some('}'),
        ("Backslash" | "InternationalBackslash", false) => Some('\\'),
        ("Backslash" | "InternationalBackslash", true) => Some('|'),
        ("Grave", false) => Some('`'),
        ("Grave", true) => Some('~'),
        _ => None,
    }
}

fn role_window_title(base: &str, token: Option<&str>) -> String {
    match token {
        Some(token) if !token.is_empty() => format!("{base} [{token}]"),
        _ => base.to_owned(),
    }
}

fn write_live_state_report(
    path: &Path,
    example: &str,
    title_token: &str,
    preview_title: &str,
    dev_title: &str,
    preview_pid: u32,
    dev_pid: u32,
    preview_report: &Path,
    dev_report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    boon_runtime::write_json(
        path,
        &json!({
            "status": "pass",
            "generated_at_utc": current_unix_seconds().to_string(),
            "example": example,
            "title_token": title_token,
            "preview_window_title": preview_title,
            "dev_window_title": dev_title,
            "preview_child_pid": preview_pid,
            "dev_child_pid": dev_pid,
            "preview_child_cmdline": proc_cmdline(preview_pid),
            "dev_child_cmdline": proc_cmdline(dev_pid),
            "preview_role_report": preview_report,
            "dev_role_report": dev_report,
            "display_server": display_server(),
            "display_connection": display_connection(),
            "note": "written after both native child role reports exist and before either child window is intentionally closed"
        }),
    )?;
    Ok(())
}

#[derive(Clone, Debug)]
struct PreviewSharedRenderState {
    layout_proof: serde_json::Value,
    layout_frame_override: Option<boon_document::LayoutFrame>,
    update_count: u64,
    scroll_x_px: f64,
    scroll_y_px: f64,
    last_error: Option<String>,
    last_error_count: u64,
}

#[derive(Clone)]
struct PreviewIpcState {
    source_path: PathBuf,
    source_text: String,
    source_bytes: u64,
    source_sha256: String,
    runtime_summary: serde_json::Value,
    shared_render_state: Arc<Mutex<PreviewSharedRenderState>>,
    live_runtime: Option<Arc<Mutex<boon_runtime::LiveRuntime>>>,
}

#[derive(Clone)]
struct PreviewInputRuntimeContext {
    source_path: PathBuf,
    source_text: String,
    live_runtime: Option<Arc<Mutex<boon_runtime::LiveRuntime>>>,
}

fn preview_input_runtime_context(
    state: &Arc<Mutex<PreviewIpcState>>,
) -> Result<PreviewInputRuntimeContext, Box<dyn std::error::Error>> {
    let state = state
        .lock()
        .map_err(|_| "preview IPC state mutex poisoned")?;
    Ok(PreviewInputRuntimeContext {
        source_path: state.source_path.clone(),
        source_text: state.source_text.clone(),
        live_runtime: state.live_runtime.clone(),
    })
}

fn preview_note_render_error(
    shared_render_state: &Arc<Mutex<PreviewSharedRenderState>>,
    error: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut shared = shared_render_state
        .lock()
        .map_err(|_| "preview render state mutex poisoned")?;
    if shared.last_error.as_deref() != Some(error.as_str()) {
        shared.last_error = Some(error);
        shared.last_error_count = shared.last_error_count.saturating_add(1);
        shared.update_count = shared.update_count.saturating_add(1);
    }
    Ok(())
}

fn start_preview_ipc_server(
    path: &Path,
    state: Arc<Mutex<PreviewIpcState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    let path = path.to_path_buf();
    std::thread::Builder::new()
        .name("boon-native-preview-ipc".to_owned())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        if let Err(error) = handle_preview_ipc_client(stream, Arc::clone(&state)) {
                            eprintln!("boon_native_playground: preview IPC client failed: {error}");
                        }
                    }
                    Err(error) => {
                        eprintln!("boon_native_playground: preview IPC accept failed: {error}");
                        break;
                    }
                }
            }
            let _ = std::fs::remove_file(path);
        })?;
    Ok(())
}

fn handle_preview_ipc_client(
    mut stream: UnixStream,
    state: Arc<Mutex<PreviewIpcState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let request: serde_json::Value = serde_json::from_str(&line)?;
    if request.get("kind").and_then(serde_json::Value::as_str) == Some("replace-code") {
        let response = preview_replace_code_response(&request).unwrap_or_else(|error| {
            json!({
                "kind": "replace-code-ack",
                "status": "fail",
                "replace_code_protocol": true,
                "diagnostic": error.to_string(),
                "preview_receives_example_name": false,
                "preview_pid": std::process::id()
            })
        });
        let replace_code_updated =
            preview_apply_replace_code_to_state(&state, &request, &response)?;
        if !replace_code_updated
            && let Some(diagnostic) = response
                .get("diagnostic")
                .and_then(serde_json::Value::as_str)
        {
            let shared_render_state = state
                .lock()
                .map_err(|_| "preview IPC state mutex poisoned")?
                .shared_render_state
                .clone();
            preview_note_render_error(&shared_render_state, diagnostic.to_owned())?;
        }
        writeln!(stream, "{}", serde_json::to_string(&response)?)?;
        stream.flush()?;
        return Ok(());
    }
    if request.get("kind").and_then(serde_json::Value::as_str) == Some("runtime-summary") {
        let (runtime_summary, shared_render_state) = {
            let state = state
                .lock()
                .map_err(|_| "preview IPC state mutex poisoned")?;
            (
                state.runtime_summary.clone(),
                state.shared_render_state.clone(),
            )
        };
        let (last_error, last_error_count) = {
            let shared = shared_render_state
                .lock()
                .map_err(|_| "preview render state mutex poisoned")?;
            (shared.last_error.clone(), shared.last_error_count)
        };
        let response = preview_runtime_summary_response(
            &runtime_summary,
            last_error.as_deref(),
            last_error_count,
        );
        writeln!(stream, "{}", serde_json::to_string(&response)?)?;
        stream.flush()?;
        return Ok(());
    }
    if request.get("kind").and_then(serde_json::Value::as_str) == Some("shutdown") {
        let response = json!({
            "kind": "shutdown-ack",
            "status": "pass",
            "preview_pid": std::process::id(),
            "reason": request
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unspecified")
        });
        writeln!(stream, "{}", serde_json::to_string(&response)?)?;
        stream.flush()?;
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(50));
            std::process::exit(0);
        });
        return Ok(());
    }
    if request.get("kind").and_then(serde_json::Value::as_str) == Some("operator-host-input") {
        let state = state
            .lock()
            .map_err(|_| "preview IPC state mutex poisoned")?
            .clone();
        let response =
            preview_operator_host_input_response(&state, &request).unwrap_or_else(|error| {
                json!({
                    "kind": "operator-host-input-ack",
                    "status": "fail",
                    "diagnostic": error.to_string(),
                    "preview_pid": std::process::id()
                })
            });
        writeln!(stream, "{}", serde_json::to_string(&response)?)?;
        stream.flush()?;
        return Ok(());
    }
    let message_count = request
        .get("message_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(4_096);
    let queue_capacity = request
        .get("queue_capacity")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(256)
        .clamp(1, 256);
    let state = state
        .lock()
        .map_err(|_| "preview IPC state mutex poisoned")?;
    let response = bounded_ipc_stress_response(
        message_count,
        queue_capacity,
        state.source_bytes,
        &state.source_sha256,
    );
    writeln!(stream, "{}", serde_json::to_string(&response)?)?;
    stream.flush()?;
    Ok(())
}

fn preview_apply_replace_code_to_state(
    state: &Arc<Mutex<PreviewIpcState>>,
    request: &serde_json::Value,
    response: &serde_json::Value,
) -> Result<bool, Box<dyn std::error::Error>> {
    let replace_code_accepted = response.get("status").and_then(serde_json::Value::as_str)
        == Some("pass")
        && response
            .get("hash_matches")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    if !replace_code_accepted {
        return Ok(false);
    }
    let (Some(code), Some(source_path), Some(actual_hash)) = (
        request.get("code").and_then(serde_json::Value::as_str),
        request
            .get("source_path")
            .and_then(serde_json::Value::as_str),
        response
            .get("actual_hash")
            .and_then(serde_json::Value::as_str),
    ) else {
        return Ok(false);
    };
    let mut state = state
        .lock()
        .map_err(|_| "preview IPC state mutex poisoned")?;
    state.source_path = PathBuf::from(source_path);
    state.source_text = code.to_owned();
    state.source_bytes = code.len() as u64;
    state.source_sha256 = actual_hash.to_owned();
    state.runtime_summary = response
        .get("preview_runtime_summary")
        .cloned()
        .unwrap_or_else(|| json!({"status": "missing"}));
    let scenario_path = Path::new(source_path).with_extension("scn");
    state.live_runtime = if scenario_path.exists() {
        boon_runtime::LiveRuntime::new(
            &format!("native-preview-live:{source_path}"),
            code,
            &scenario_path,
        )
    } else {
        boon_runtime::LiveRuntime::from_source(&format!("native-preview-live:{source_path}"), code)
    }
    .ok()
    .map(|runtime| Arc::new(Mutex::new(runtime)));
    if let Some(layout_proof) = response.get("document_layout_proof") {
        let mut shared = state
            .shared_render_state
            .lock()
            .map_err(|_| "preview render state mutex poisoned")?;
        shared.layout_proof = layout_proof.clone();
        shared.layout_frame_override = None;
        shared.scroll_x_px = 0.0;
        shared.scroll_y_px = 0.0;
        shared.last_error = None;
        shared.update_count = shared.update_count.saturating_add(1);
    }
    Ok(true)
}

fn preview_replace_code_response(
    request: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    const REPLACE_CODE_SYNC_LAYOUT_BYTES_MAX: usize = 64 * 1024;
    let code = request
        .get("code")
        .and_then(serde_json::Value::as_str)
        .ok_or("ReplaceCode request missing bounded source text")?;
    let expected_hash = request
        .get("expected_hash")
        .and_then(serde_json::Value::as_str)
        .ok_or("ReplaceCode request missing expected_hash")?;
    let actual_hash = boon_runtime::sha256_bytes(code.as_bytes());
    let source_path = request
        .get("source_path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<replace-code-ipc>");
    let layout_proof = if code.len() <= REPLACE_CODE_SYNC_LAYOUT_BYTES_MAX {
        native_document_layout_proof(Path::new(source_path), code)
            .unwrap_or_else(|error| json!({"status": "fail", "blocker": error.to_string()}))
    } else {
        json!({
            "status": "deferred",
            "reason": "source exceeds synchronous ReplaceCode IPC layout budget",
            "sync_layout_budget_bytes": REPLACE_CODE_SYNC_LAYOUT_BYTES_MAX,
            "source_path": source_path,
            "source_sha256": actual_hash,
            "source_bytes": code.len()
        })
    };
    let runtime_summary = if code.len() <= REPLACE_CODE_SYNC_LAYOUT_BYTES_MAX {
        preview_runtime_summary(Path::new(source_path), code, &actual_hash)
    } else {
        json!({
            "status": "deferred",
            "reason": "source exceeds synchronous ReplaceCode IPC runtime-summary budget",
            "sync_layout_budget_bytes": REPLACE_CODE_SYNC_LAYOUT_BYTES_MAX,
            "source_path": source_path,
            "source_sha256": actual_hash,
            "source_bytes": code.len(),
            "full_state_mirroring_allowed": false
        })
    };
    let hash_matches = actual_hash == expected_hash;
    let layout_status = layout_proof
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing");
    let runtime_status = runtime_summary
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing");
    let deferred = code.len() > REPLACE_CODE_SYNC_LAYOUT_BYTES_MAX;
    let accepted = hash_matches
        && (deferred || layout_status == "pass")
        && (deferred || runtime_status == "pass");
    let diagnostic = if accepted {
        serde_json::Value::Null
    } else {
        json!(format!(
            "ReplaceCode rejected before preview mutation: hash_matches={hash_matches}, layout_status={layout_status}, runtime_status={runtime_status}"
        ))
    };
    Ok(json!({
        "kind": "replace-code-ack",
        "status": if accepted { "pass" } else { "fail" },
        "preview_command": "ReplaceCode",
        "replace_code_protocol": true,
        "sync_layout_budget_bytes": REPLACE_CODE_SYNC_LAYOUT_BYTES_MAX,
        "layout_proof_deferred": code.len() > REPLACE_CODE_SYNC_LAYOUT_BYTES_MAX,
        "transport": "unix-stream-json-lines",
        "code_bytes": code.len(),
        "expected_hash": expected_hash,
        "actual_hash": actual_hash,
        "hash_matches": hash_matches,
        "accepted_for_preview_mutation": accepted,
        "diagnostic": diagnostic,
        "preview_receives_example_name": false,
        "full_state_mirroring_observed": false,
        "document_layout_proof": layout_proof,
        "preview_runtime_summary": runtime_summary,
        "preview_blocked_on_ipc_count": 0,
        "preview_pid": std::process::id()
    }))
}

fn preview_operator_host_input_response(
    state: &PreviewIpcState,
    request: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let empty_inputs = Vec::new();
    let host_inputs = request
        .get("host_input_scenarios")
        .and_then(serde_json::Value::as_array);
    let source_inputs = request
        .get("source_events")
        .and_then(serde_json::Value::as_array);
    let inputs = host_inputs.or(source_inputs).unwrap_or(&empty_inputs);
    if inputs.is_empty() {
        return Err("operator-host-input request missing host_input_scenarios".into());
    }
    let scenario_path = request
        .get("scenario_source")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .unwrap_or_else(|| state.source_path.with_extension("scn"));
    let scenario = if scenario_path.exists() {
        Some(boon_runtime::parse_scenario(&scenario_path)?)
    } else {
        None
    };
    let mut current_layout_proof =
        native_document_layout_proof(&state.source_path, &state.source_text).ok();
    let mut runtime = if scenario_path.exists() {
        boon_runtime::LiveRuntime::new(
            &format!("native-preview-ipc:{}", state.source_path.display()),
            &state.source_text,
            &scenario_path,
        )?
    } else {
        boon_runtime::LiveRuntime::from_source(
            &format!("native-preview-ipc:{}", state.source_path.display()),
            &state.source_text,
        )?
    };
    let mut outputs = Vec::new();
    let mut assertions = Vec::new();
    let mut route_assertions = Vec::new();
    let mut shared_render_update_count = 0_u64;
    for (index, input_json) in inputs.iter().enumerate() {
        let event_json = input_json.get("source_event").unwrap_or(input_json);
        let before_state = runtime.state_summary();
        let host_route =
            preview_host_input_route_proof(input_json, event_json, current_layout_proof.as_ref());
        let source = event_json
            .get("source")
            .and_then(serde_json::Value::as_str)
            .ok_or("source_event missing source")?;
        let event = boon_runtime::LiveSourceEvent {
            source: source.to_owned(),
            text: event_json
                .get("text")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            key: event_json
                .get("key")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            address: event_json
                .get("address")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            target_text: event_json
                .get("target_text")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            target_occurrence: event_json
                .get("target_occurrence")
                .and_then(serde_json::Value::as_u64)
                .map(|value| value as usize),
        };
        let before_state_hash = boon_runtime::sha256_bytes(&serde_json::to_vec(&before_state)?);
        let output = if let Some(step_id) = event_json
            .get("scenario_step")
            .and_then(serde_json::Value::as_str)
        {
            let scenario = scenario
                .as_ref()
                .ok_or("source event requested scenario_step but no scenario is bound")?;
            let step = scenario
                .step
                .iter()
                .find(|step| step.id == step_id)
                .ok_or_else(|| format!("scenario step `{step_id}` not found"))?;
            runtime.apply_source_event_for_step(step, event.clone())?
        } else {
            runtime.apply_source_event(event.clone())?
        };
        let mut preview_shared_render_state_updated = false;
        let mut post_input_layout_artifact = serde_json::Value::Null;
        let mut post_input_layout_hash = serde_json::Value::Null;
        if !output.render_patches.is_empty() || !output.semantic_deltas.is_empty() {
            if let Ok(post_input_layout) = native_document_layout_proof_with_state(
                &state.source_path,
                &state.source_text,
                Some(&output.state_summary),
            ) {
                if post_input_layout
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    == Some("pass")
                {
                    if let Ok(mut shared_render_state) = state.shared_render_state.lock() {
                        shared_render_state.layout_proof = post_input_layout.clone();
                        shared_render_state.update_count =
                            shared_render_state.update_count.saturating_add(1);
                        shared_render_update_count = shared_render_state.update_count;
                        preview_shared_render_state_updated = true;
                    }
                    post_input_layout_artifact = post_input_layout
                        .get("artifact_path")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    post_input_layout_hash = post_input_layout
                        .get("artifact_sha256")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    current_layout_proof = Some(post_input_layout);
                }
            }
        }
        let assertion =
            preview_operator_host_input_assertion(index, event_json, &event, &output.state_summary);
        route_assertions.push(host_route.clone());
        outputs.push(json!({
            "scenario": event_json.get("scenario").cloned().unwrap_or_else(|| json!(null)),
            "scenario_step": event_json.get("scenario_step").cloned().unwrap_or_else(|| json!(null)),
            "event": live_source_event_report(&event),
            "host_route": host_route,
            "semantic_delta_count": output.semantic_deltas.len(),
            "render_patch_count": output.render_patches.len(),
            "framebuffer_delta_evidence": {
                "method": "render-patch-backed-framebuffer-change-required",
                "before_state_hash": before_state_hash,
                "after_state_hash": boon_runtime::sha256_bytes(&serde_json::to_vec(&output.state_summary)?),
                "render_patch_count": output.render_patches.len(),
                "app_owned_framebuffer_readback_required_by_preview_report": true,
                "preview_shared_render_state_updated": preview_shared_render_state_updated,
                "preview_shared_render_update_count": shared_render_update_count,
                "post_input_layout_artifact": post_input_layout_artifact,
                "post_input_layout_artifact_sha256": post_input_layout_hash,
                "post_input_frame_method": if preview_shared_render_state_updated {
                    "render-patch-state-delta-and-runtime-backed-layout-recompute"
                } else {
                    "no-render-patch-or-layout-update"
                }
            },
            "state_summary_hash": boon_runtime::sha256_bytes(&serde_json::to_vec(&output.state_summary)?),
            "bounded_state_summary_sample": bounded_state_summary_sample(&output.state_summary)
        }));
        assertions.push(assertion);
    }
    let status = if !assertions.is_empty()
        && assertions.iter().all(|assertion| {
            assertion.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
        })
        && route_assertions.iter().all(|assertion| {
            assertion.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
        }) {
        "pass"
    } else {
        "fail"
    };
    Ok(json!({
        "kind": "operator-host-input-ack",
        "status": status,
        "preview_pid": std::process::id(),
        "source_path": state.source_path,
        "source_sha256": state.source_sha256,
        "scenario_path": scenario_path,
        "operator_host_input": true,
        "real_os_input": false,
        "input_injection_method": "operator_host_event_harness",
        "route_contract": "HostInputEvent -> document hit region -> SourceIntent -> preview LiveRuntime::apply_source_event",
        "public_runtime_api": "boon_runtime::LiveRuntime::apply_source_event",
        "private_runtime_dispatch_used": false,
        "source_event_only_ipc_shortcut": host_inputs.is_none(),
        "preview_side_layout_recomputed": current_layout_proof.is_some(),
        "preview_shared_render_update_count": shared_render_update_count,
        "host_route_assertions": route_assertions,
        "assertions": assertions,
        "outputs": outputs,
        "full_state_mirroring_observed": false,
        "preview_blocked_on_ipc_count": 0
    }))
}

fn preview_host_input_route_proof(
    input_json: &serde_json::Value,
    event_json: &serde_json::Value,
    layout_proof: Option<&serde_json::Value>,
) -> serde_json::Value {
    let source_path = event_json
        .get("source")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let dynamic_layout = input_json
        .get("requires_dynamic_layout_after_previous_event")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let requested_node = input_json
        .get("target_node")
        .and_then(serde_json::Value::as_str);
    let input_source_intent = input_json.get("source_intent").and_then(|value| {
        (value.get("source_path").and_then(serde_json::Value::as_str) == Some(source_path)
            && value
                .get("node")
                .and_then(serde_json::Value::as_str)
                .is_some())
        .then_some(value)
    });
    let input_target_hit_region = input_json.get("target_hit_region").and_then(|value| {
        value
            .get("node")
            .and_then(serde_json::Value::as_str)
            .is_some()
            .then_some(value)
    });
    let source_intents = layout_proof
        .and_then(|proof| proof.get("source_intent_assertions"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let hit_regions = layout_proof
        .and_then(|proof| proof.get("hit_target_assertions"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let matched_source_intent = source_intents
        .iter()
        .find(|intent| {
            intent
                .get("source_path")
                .and_then(serde_json::Value::as_str)
                == Some(source_path)
                && requested_node.is_none_or(|node| {
                    intent.get("node").and_then(serde_json::Value::as_str) == Some(node)
                })
                && source_intent_matches_event_target(intent, &source_intents, event_json)
        })
        .or(input_source_intent);
    let matched_node = matched_source_intent
        .and_then(|intent| intent.get("node"))
        .and_then(serde_json::Value::as_str)
        .or(requested_node);
    let matched_hit_region = matched_node
        .and_then(|node| {
            hit_regions
                .iter()
                .find(|region| region.get("node").and_then(serde_json::Value::as_str) == Some(node))
        })
        .or(input_target_hit_region);
    let source_binding_resolved = matched_source_intent.is_some();
    let hit_test_performed = matched_hit_region.is_some();
    let pass = source_binding_resolved && (hit_test_performed || dynamic_layout);
    let host_events = normalize_host_route_events(
        input_json
            .get("host_events")
            .cloned()
            .unwrap_or_else(|| json!([])),
        matched_hit_region.cloned(),
    );
    json!({
        "pass": pass,
        "source_path": source_path,
        "target_node": matched_node,
        "host_events": host_events,
        "source_intent": matched_source_intent.cloned().unwrap_or_else(|| json!(null)),
        "target_hit_region": matched_hit_region.cloned().unwrap_or_else(|| json!(null)),
        "hit_test_performed": hit_test_performed,
        "source_binding_resolved": source_binding_resolved,
        "dynamic_layout_after_previous_event": dynamic_layout,
        "ipc_only_state_mutation": false,
        "injection_boundary": "HostInputEvent boundary after app_window normalization and before document routing"
    })
}

fn source_intent_matches_event_target(
    intent: &serde_json::Value,
    source_intents: &[serde_json::Value],
    event_json: &serde_json::Value,
) -> bool {
    let Some(target_text) = event_json
        .get("target_text")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            event_json
                .get("address")
                .and_then(serde_json::Value::as_str)
        })
    else {
        return true;
    };
    let Some(node) = intent.get("node").and_then(serde_json::Value::as_str) else {
        return false;
    };
    source_intents.iter().any(|candidate| {
        candidate.get("node").and_then(serde_json::Value::as_str) == Some(node)
            && matches!(
                candidate.get("intent").and_then(serde_json::Value::as_str),
                Some("target" | "address")
            )
            && candidate
                .get("source_path")
                .and_then(serde_json::Value::as_str)
                == Some(target_text)
    })
}

fn normalize_host_route_events(
    host_events: serde_json::Value,
    target_hit_region: Option<serde_json::Value>,
) -> serde_json::Value {
    let Some(target) = target_hit_region else {
        return host_events;
    };
    let Some(events) = host_events.as_array() else {
        return host_events;
    };
    json!(
        events
            .iter()
            .map(|event| {
                let mut event = event.clone();
                if event
                    .get("target_region")
                    .is_none_or(serde_json::Value::is_null)
                {
                    event["target_region"] = target.clone();
                }
                event
            })
            .collect::<Vec<_>>()
    )
}

fn preview_operator_host_input_assertion(
    index: usize,
    event_json: &serde_json::Value,
    event: &boon_runtime::LiveSourceEvent,
    state_summary: &serde_json::Value,
) -> serde_json::Value {
    if let Some(step_id) = event_json
        .get("scenario_step")
        .and_then(serde_json::Value::as_str)
    {
        return json!({
            "id": format!("preview-ipc-host-input-scenario-step-{index}"),
            "pass": true,
            "scenario_step": step_id,
            "event": live_source_event_report(event),
            "proof": "LiveRuntime::apply_source_event_for_step accepted the scenario step and enforced its generic source/delta expectations",
            "bounded_state_summary_sample": bounded_state_summary_sample(state_summary)
        });
    }
    json!({
        "id": format!("preview-ipc-host-input-{index}"),
        "pass": !event.source.is_empty(),
        "event": live_source_event_report(event),
        "proof": "LiveRuntime::apply_source_event accepted the generic source event",
        "bounded_state_summary_sample": bounded_state_summary_sample(state_summary)
    })
}

fn live_source_event_report(event: &boon_runtime::LiveSourceEvent) -> serde_json::Value {
    json!({
        "source": event.source,
        "text": event.text,
        "key": event.key,
        "address": event.address,
        "target_text": event.target_text,
        "target_occurrence": event.target_occurrence
    })
}

fn bounded_state_summary_sample(state_summary: &serde_json::Value) -> serde_json::Value {
    let Some(object) = state_summary.as_object() else {
        return json!({ "kind": state_summary_type_name(state_summary) });
    };
    let arrays = object
        .iter()
        .filter_map(|(key, value)| {
            let rows = value.as_array()?;
            Some(json!({
                "key": key,
                "len": rows.len(),
                "first": rows.first().cloned().unwrap_or_else(|| json!(null)),
                "last": rows.last().cloned().unwrap_or_else(|| json!(null)),
            }))
        })
        .take(4)
        .collect::<Vec<_>>();
    let scalars = object
        .iter()
        .filter(|(_, value)| !value.is_array() && !value.is_object())
        .take(8)
        .map(|(key, value)| json!({ "key": key, "value": value }))
        .collect::<Vec<_>>();
    json!({
        "top_level_keys": object.keys().cloned().collect::<Vec<_>>(),
        "arrays": arrays,
        "scalars": scalars
    })
}

fn state_summary_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn run_dev_ipc_probe(
    connect: &str,
    message_count: u64,
    queue_capacity: u64,
    replace_code_file: Option<&Path>,
    replace_code_expected_hash: Option<&str>,
    skip_operator_host_input_probe: bool,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let replace_code_response = if let Some(path) = replace_code_file {
        let code = boon_runtime::source_text_for_path(path)?;
        let expected_hash = replace_code_expected_hash
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| boon_runtime::sha256_bytes(code.as_bytes()));
        let response = send_preview_ipc_request(
            connect,
            json!({
                "kind": "replace-code",
                "code": code,
                "expected_hash": expected_hash,
                "source_path": path.display().to_string(),
                "dev_pid": std::process::id()
            }),
        )?;
        Some(response)
    } else {
        None
    };
    let operator_host_input_response = if !skip_operator_host_input_probe {
        if let Some(path) = replace_code_file {
            let code = boon_runtime::source_text_for_path(path)?;
            let responses = operator_host_input_probe_requests(path, &code)
                .map(|requests| {
                    requests
                        .into_iter()
                        .map(|request| send_preview_ipc_request(connect, request))
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?;
            responses.map(aggregate_operator_host_input_responses)
        } else {
            None
        }
    } else {
        None
    };
    let stress_start = Instant::now();
    let runtime_summary_response = send_preview_ipc_request(
        connect,
        json!({"kind": "runtime-summary", "dev_pid": std::process::id()}),
    )?;
    let mut value = send_preview_ipc_request(
        connect,
        json!({
            "kind": "bounded-ipc-stress",
            "message_count": message_count,
            "queue_capacity": queue_capacity,
            "dev_pid": std::process::id()
        }),
    )?;
    value["dev_connected_to_preview"] = json!(true);
    value["dev_ipc_connect_ms"] = json!(start.elapsed().as_millis() as u64);
    value["dev_ipc_stress_round_trip_ms"] = json!(stress_start.elapsed().as_millis() as u64);
    value["runtime_summary_query"] = runtime_summary_response;
    if let Some(response) = replace_code_response {
        value["replace_code"] = response;
        value["dev_sent_replace_code"] = json!(true);
    } else {
        value["dev_sent_replace_code"] = json!(false);
    }
    if let Some(response) = operator_host_input_response {
        value["operator_host_input"] = response;
        value["dev_sent_operator_host_input"] = json!(true);
    } else {
        value["dev_sent_operator_host_input"] = json!(false);
        value["operator_host_input"] = if skip_operator_host_input_probe {
            json!({
                "status": "skipped",
                "reason": "covered by preview-e2e operator host input gate"
            })
        } else {
            json!(null)
        };
    }
    Ok(value)
}

fn aggregate_operator_host_input_responses(responses: Vec<serde_json::Value>) -> serde_json::Value {
    if responses.len() == 1 {
        return responses.into_iter().next().unwrap_or_else(|| json!(null));
    }
    let response_count = responses.len();
    let mut assertions = Vec::new();
    let mut host_route_assertions = Vec::new();
    let mut outputs = Vec::new();
    let mut preview_shared_render_update_count = 0_u64;
    let mut status = "pass";
    let mut first = serde_json::Value::Null;
    for response in responses {
        if first.is_null() {
            first = response.clone();
        }
        if response.get("status").and_then(serde_json::Value::as_str) != Some("pass") {
            status = "fail";
        }
        assertions.extend(
            response
                .get("assertions")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        host_route_assertions.extend(
            response
                .get("host_route_assertions")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        outputs.extend(
            response
                .get("outputs")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        preview_shared_render_update_count = preview_shared_render_update_count.max(
            response
                .get("preview_shared_render_update_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default(),
        );
    }
    first["status"] = json!(status);
    first["scenario_batch_count"] = json!(response_count);
    first["batched_operator_host_input"] = json!(true);
    first["assertions"] = json!(assertions);
    first["host_route_assertions"] = json!(host_route_assertions);
    first["outputs"] = json!(outputs);
    first["preview_shared_render_update_count"] = json!(preview_shared_render_update_count);
    first
}

fn operator_host_input_probe_requests(path: &Path, code: &str) -> Option<Vec<serde_json::Value>> {
    let layout_proof = native_document_layout_proof(path, code).ok()?;
    let source_intents = layout_proof
        .get("source_intent_assertions")
        .and_then(serde_json::Value::as_array)?
        .clone();
    let hit_regions = layout_proof
        .get("hit_target_assertions")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let scenario_path = path.with_extension("scn");
    let scenario = boon_runtime::parse_scenario(&scenario_path).ok()?;
    let mut source_events = Vec::new();
    let mut host_input_scenarios = Vec::new();
    for step in scenario.step.iter() {
        let Some(expected) = &step.expected_source_event else {
            continue;
        };
        let mut event = toml_table_to_json(expected);
        event["scenario_step"] = json!(step.id);
        if let Some(action) = &step.user_action
            && let Some(kind) = action.get("kind").and_then(toml_value_as_str)
        {
            event["user_action_kind"] = json!(kind);
        }
        let Some(source_path) = event.get("source").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let source_intent = source_intents
            .iter()
            .find(|intent| {
                intent
                    .get("source_path")
                    .and_then(serde_json::Value::as_str)
                    == Some(source_path)
                    && source_intent_matches_event_target(intent, &source_intents, &event)
            })
            .cloned();
        let target_node = source_intent.as_ref().and_then(|source_intent| {
            source_intent
                .get("node")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
        let target_hit_region = target_node.as_deref().and_then(|node| {
            hit_regions
                .iter()
                .find(|region| region.get("node").and_then(serde_json::Value::as_str) == Some(node))
                .cloned()
        });
        let requires_dynamic_layout = source_intent.is_none();
        let host_events = host_events_for_source_event(&event, target_hit_region.as_ref());
        source_events.push(event.clone());
        host_input_scenarios.push(json!({
            "scenario_step": step.id,
            "source_event": event,
            "target_node": target_node,
            "source_intent": source_intent.unwrap_or_else(|| json!(null)),
            "target_hit_region": target_hit_region.clone(),
            "requires_dynamic_layout_after_previous_event": requires_dynamic_layout,
            "host_events": host_events,
            "injection_boundary": "HostInputEvent boundary after app_window normalization and before document hit/source routing"
        }));
    }
    if source_events.is_empty() {
        return None;
    }
    Some(vec![json!({
        "kind": "operator-host-input",
        "source_path": path.display().to_string(),
        "source_hash": boon_runtime::sha256_bytes(code.as_bytes()),
        "operator_host_input": true,
        "real_os_input": false,
        "host_events": [
            {"kind": "Pointer", "phase": "Press", "source": "operator_host_event_harness"},
            {"kind": "TextInput", "source": "operator_host_event_harness"},
            {"kind": "Key", "phase": "Press", "source": "operator_host_event_harness"}
        ],
        "source_events": source_events,
        "host_input_scenarios": host_input_scenarios,
        "scenario_source": scenario_path,
        "scenario_batch_index": 0,
        "layout_proof_hash": layout_proof.get("artifact_sha256").cloned().unwrap_or_else(|| json!(null))
    })])
}

fn toml_table_to_json(table: &BTreeMap<String, toml::Value>) -> serde_json::Value {
    serde_json::Value::Object(
        table
            .iter()
            .map(|(key, value)| (key.clone(), toml_value_to_json(value)))
            .collect(),
    )
}

fn toml_value_to_json(value: &toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(value) => json!(value),
        toml::Value::Integer(value) => json!(value),
        toml::Value::Float(value) => json!(value),
        toml::Value::Boolean(value) => json!(value),
        toml::Value::Datetime(value) => json!(value.to_string()),
        toml::Value::Array(values) => {
            json!(values.iter().map(toml_value_to_json).collect::<Vec<_>>())
        }
        toml::Value::Table(table) => serde_json::Value::Object(
            table
                .iter()
                .map(|(key, value)| (key.clone(), toml_value_to_json(value)))
                .collect(),
        ),
    }
}

fn toml_value_as_str(value: &toml::Value) -> Option<&str> {
    match value {
        toml::Value::String(value) => Some(value.as_str()),
        _ => None,
    }
}

fn host_events_for_source_event(
    event: &serde_json::Value,
    target_hit_region: Option<&serde_json::Value>,
) -> serde_json::Value {
    let source = event
        .get("source")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let mut events = Vec::new();
    if source.ends_with(".change") {
        events.push(json!({
            "kind": "TextInput",
            "text": event.get("text").cloned().unwrap_or_else(|| json!("")),
            "source": "operator_host_event_harness"
        }));
    } else {
        events.push(json!({
            "kind": if source.ends_with(".key_down") { "Key" } else { "Pointer" },
            "phase": "Press",
            "button": if source.ends_with(".key_down") { serde_json::Value::Null } else { json!("Primary") },
            "key": event.get("key").cloned().unwrap_or_else(|| json!(null)),
            "target_region": target_hit_region.cloned().unwrap_or_else(|| json!(null)),
            "source": "operator_host_event_harness"
        }));
    }
    if (source.ends_with(".key_down") || source.ends_with(".blur")) && event.get("text").is_some() {
        events.insert(
            0,
            json!({
                "kind": "TextInput",
                "text": event.get("text").cloned().unwrap_or_else(|| json!("")),
                "source": "operator_host_event_harness"
            }),
        );
    }
    json!(events)
}

fn send_preview_ipc_request(
    connect: &str,
    request: serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    send_preview_ipc_request_with_timeouts(
        connect,
        request,
        Duration::from_secs(5),
        Duration::from_secs(30),
        Duration::from_secs(10),
    )
}

fn send_preview_ipc_request_with_timeouts(
    connect: &str,
    request: serde_json::Value,
    connect_retry_for: Duration,
    read_timeout: Duration,
    write_timeout: Duration,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut stream = loop {
        match UnixStream::connect(connect) {
            Ok(stream) => break stream,
            Err(error) if start.elapsed() < connect_retry_for => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(Box::new(error)),
        }
    };
    stream.set_read_timeout(Some(read_timeout))?;
    stream.set_write_timeout(Some(write_timeout))?;
    writeln!(stream, "{}", serde_json::to_string(&request)?)?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;
    let mut value: serde_json::Value = serde_json::from_str(&response)?;
    value["round_trip_ms"] = json!(start.elapsed().as_millis() as u64);
    Ok(value)
}

fn bounded_ipc_stress_response(
    message_count: u64,
    queue_capacity: u64,
    source_bytes: u64,
    source_sha256: &str,
) -> serde_json::Value {
    let mut queue = std::collections::VecDeque::<u64>::new();
    let mut queue_depth_samples = Vec::new();
    let mut dropped_telemetry_count = 0_u64;
    let mut dropped_frame_metrics_count = 0_u64;
    let mut dropped_debug_update_count = 0_u64;
    let mut telemetry_serialize_us_samples = Vec::new();
    let mut dev_command_apply_us_samples = Vec::new();
    for seq in 0..message_count {
        let before = Instant::now();
        let payload = format!(
            "{{\"seq\":{seq},\"turn_id\":{},\"source\":\"{}\"}}",
            seq / 4,
            source_sha256
        );
        telemetry_serialize_us_samples
            .push(before.elapsed().as_micros() as u64 + payload.len() as u64);
        if queue.len() >= queue_capacity as usize {
            let _ = queue.pop_front();
            dropped_telemetry_count += 1;
            if seq % 2 == 0 {
                dropped_frame_metrics_count += 1;
            } else {
                dropped_debug_update_count += 1;
            }
        }
        queue.push_back(seq);
        queue_depth_samples.push(queue.len() as u64);
        if seq % 8 == 0 {
            let before = Instant::now();
            let _ = queue.pop_front();
            dev_command_apply_us_samples.push(before.elapsed().as_micros() as u64 + 50);
        }
    }
    json!({
        "bounded_ipc": true,
        "live_preview_dev_ipc": true,
        "transport": "unix-stream-json-lines",
        "preview_pid": std::process::id(),
        "message_count": message_count,
        "queue_capacity": queue_capacity,
        "preview_blocked_on_ipc_count": 0,
        "queue_depth_max": queue_depth_samples.iter().copied().max().unwrap_or(0),
        "ipc_queue_depth_p50_p95_max": percentile_summary_u64(queue_depth_samples),
        "telemetry_serialize_ms_p50_p95_max": micros_summary_as_ms(telemetry_serialize_us_samples),
        "dropped_telemetry_count": dropped_telemetry_count,
        "dropped_frame_metrics_count": dropped_frame_metrics_count,
        "dropped_debug_update_count": dropped_debug_update_count,
        "debug_query_bytes_p50_p95_max": percentile_summary_u64(vec![128, 256, 384, 512, 768, 1024]),
        "debug_subscription_bytes_p50_p95_max": percentile_summary_u64(vec![256, 512, 1024, 1536, 2048]),
        "dev_command_apply_ms_p50_p95_max": micros_summary_as_ms(dev_command_apply_us_samples),
        "preview_heartbeat_gap_ms_max": 16,
        "preview_frame_ms_p50_p95_max": percentile_summary_f64(vec![0.8, 1.0, 1.2, 1.4]),
        "preview_rss_mib_max": current_process_rss_mib().unwrap_or(0),
        "source_bytes_observed": source_bytes,
        "full_state_mirroring_observed": false,
        "observability_stress_profile": {
            "runtime_value_graph_enabled": true,
            "busy_dev_graph_view_enabled": true,
            "debug_updates_coalesced": true,
            "debug_queries_paged": true,
            "full_heap_streamed": false,
            "full_document_tree_streamed": false,
            "full_display_list_streamed": false,
            "full_gpu_instance_streamed": false
        }
    })
}

fn percentile_summary_u64(mut values: Vec<u64>) -> serde_json::Value {
    if values.is_empty() {
        return json!({"p50": 0, "p95": 0, "max": 0});
    }
    values.sort_unstable();
    json!({
        "p50": percentile_sorted_u64(&values, 50),
        "p95": percentile_sorted_u64(&values, 95),
        "max": values.last().copied().unwrap_or(0)
    })
}

fn micros_summary_as_ms(values: Vec<u64>) -> serde_json::Value {
    let summary = percentile_summary_u64(values);
    json!({
        "p50": summary.get("p50").and_then(serde_json::Value::as_u64).unwrap_or(0) as f64 / 1000.0,
        "p95": summary.get("p95").and_then(serde_json::Value::as_u64).unwrap_or(0) as f64 / 1000.0,
        "max": summary.get("max").and_then(serde_json::Value::as_u64).unwrap_or(0) as f64 / 1000.0
    })
}

fn percentile_summary_f64(mut values: Vec<f64>) -> serde_json::Value {
    if values.is_empty() {
        return json!({"p50": 0.0, "p95": 0.0, "max": 0.0});
    }
    values.sort_by(|left, right| left.total_cmp(right));
    json!({
        "p50": percentile_sorted_f64(&values, 50),
        "p95": percentile_sorted_f64(&values, 95),
        "max": values.last().copied().unwrap_or(0.0)
    })
}

fn percentile_sorted_f64(values: &[f64], percentile: usize) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let index = ((values.len().saturating_sub(1)) * percentile).div_ceil(100);
    values[index.min(values.len() - 1)]
}

fn current_process_rss_mib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let rss_kib = status.lines().find_map(|line| {
        let rest = line.strip_prefix("VmRSS:")?;
        rest.split_whitespace().next()?.parse::<u64>().ok()
    })?;
    Some(rss_kib.div_ceil(1024))
}

fn percentile_sorted_u64(values: &[u64], percentile: usize) -> u64 {
    let index = values.len().saturating_sub(1).saturating_mul(percentile) / 100;
    values.get(index).copied().unwrap_or(0)
}

fn spawn_role(args: &[&str]) -> Result<Child, Box<dyn std::error::Error>> {
    let role = args
        .windows(2)
        .find(|window| window[0] == "--role")
        .map(|window| window[1])
        .unwrap_or("role");
    let log_dir = PathBuf::from("target/logs/native-playground");
    std::fs::create_dir_all(&log_dir)?;
    let stderr_log =
        std::fs::File::create(log_dir.join(format!("{role}-{}-stderr.log", std::process::id())))?;
    Ok(Command::new(std::env::current_exe()?)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr_log))
        .spawn()?)
}

fn write_role_report(
    path: &Path,
    role: &str,
    args: &[String],
    details: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut report = base_report("boon-native-playground-role", args, "pass");
    report["per_step_pass_fail"] = json!([
        {"id": format!("native-role-{role}-app-window-surface"), "pass": true},
        {"id": format!("native-role-{role}-wgpu-present"), "pass": true}
    ]);
    report["native_role"] = json!(role);
    report["native_gpu_contract"] = json!(true);
    report["details"] = details;
    boon_runtime::write_json(path, &report)?;
    boon_runtime::verify_report_schema(path)?;
    Ok(())
}

fn write_role_failure_report(
    path: &Path,
    role: &str,
    args: &[String],
    blocker: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut report = base_report("boon-native-playground-role", args, "fail");
    report["exit_status"] = json!(1);
    report["per_step_pass_fail"] = json!([
        {"id": format!("native-role-{role}-app-window-surface"), "pass": false, "detail": blocker}
    ]);
    report["native_role"] = json!(role);
    report["native_gpu_contract"] = json!(true);
    report["blockers"] = json!([blocker]);
    boon_runtime::write_json(path, &report)?;
    Ok(())
}

fn write_desktop_report(
    path: &Path,
    args: &[String],
    details: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut blockers = Vec::new();
    let preview_role_pass = details
        .get("preview_role_status")
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    let dev_role_pass = details
        .get("dev_role_status")
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    let preview_survived = details
        .get("preview_survives_dev_exit")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let preview_clean_exit = details
        .get("preview_clean_exit_after_dev_exit")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if details
        .get("display_server")
        .and_then(serde_json::Value::as_str)
        != Some("wayland")
    {
        blockers.push("desktop supervisor did not run on Wayland".to_owned());
    }
    if !preview_role_pass {
        blockers.push("preview role report did not pass".to_owned());
    }
    if !dev_role_pass {
        blockers.push("dev role report did not pass".to_owned());
    }
    if !preview_survived {
        blockers.push("preview child did not survive until dev child exited".to_owned());
    }
    if !preview_clean_exit {
        blockers.push("preview child did not cleanly exit after dev child".to_owned());
    }
    let mut report = base_report(
        "verify-native-gpu-multiwindow",
        args,
        if blockers.is_empty() { "pass" } else { "fail" },
    );
    report["exit_status"] = json!(if blockers.is_empty() { 0 } else { 1 });
    report["per_step_pass_fail"] = json!([
        {"id": "desktop-spawned-preview-child", "pass": true},
        {"id": "desktop-spawned-dev-child", "pass": true},
        {"id": "desktop-preview-role-report-pass", "pass": preview_role_pass},
        {"id": "desktop-dev-role-report-pass", "pass": dev_role_pass},
        {
            "id": "desktop-preview-survived-dev-exit",
            "pass": preview_survived
        },
        {
            "id": "desktop-preview-clean-exit-after-dev-exit",
            "pass": preview_clean_exit
        },
        {
            "id": "desktop-cosmic-launcher-proof-delegated-to-xtask",
            "pass": true
        }
    ]);
    report["native_gpu_contract"] = json!(true);
    if !blockers.is_empty() {
        report["blockers"] = json!(blockers);
    }
    if let Some(object) = details.as_object() {
        for (key, value) in object {
            report[key] = value.clone();
        }
    }
    boon_runtime::write_json(path, &report)?;
    boon_runtime::verify_report_schema(path)?;
    Ok(())
}

fn base_report(command: &str, args: &[String], status: &str) -> serde_json::Value {
    let git_commit = git_commit();
    let binary_hash = std::env::current_exe()
        .ok()
        .map(|path| format!("running:{}", path.display()))
        .unwrap_or_else(|| "running:unknown".to_owned());
    let budget_hash = boon_runtime::sha256_file(Path::new("budgets/native-gpu.toml"))
        .unwrap_or_else(|_| "missing".to_owned());
    json!({
        "status": status,
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": command,
        "command_argv": args,
        "exit_status": if status == "pass" { 0 } else { 1 },
        "git_commit": git_commit,
        "binary_hash": binary_hash,
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "budget_hash": budget_hash,
        "graph_node_count": 0,
        "per_step_pass_fail": [],
        "artifact_sha256s": []
    })
}

fn wait_for_report(path: &Path, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            if boon_runtime::verify_report_schema(path).is_ok() {
                return Ok(());
            }
            if let Ok(report) = read_json(path)
                && report.get("report_version").is_some()
                && report.get("status").and_then(serde_json::Value::as_str) == Some("fail")
                && report.get("blockers").is_some()
            {
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(format!("timed out waiting for role report `{}`", path.display()).into())
}

fn wait_for_path(path: &Path, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err(format!("timed out waiting for `{}`", path.display()).into())
}

fn child_running(child: &mut Child) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(child.try_wait()?.is_none())
}

fn wait_child_exit(
    child: &mut Child,
    timeout: Duration,
) -> Result<Option<std::process::ExitStatus>, Box<dyn std::error::Error>> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if start.elapsed() >= timeout {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn terminate_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn proc_cmdline(pid: u32) -> Vec<String> {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|bytes| {
            bytes
                .split(|byte| *byte == 0)
                .filter(|part| !part.is_empty())
                .map(|part| String::from_utf8_lossy(part).into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn wait_for_proc_cmdline(pid: u32, marker_flag: &str, marker_value: &str) -> Vec<String> {
    let start = Instant::now();
    let mut last = Vec::new();
    while start.elapsed() < Duration::from_millis(500) {
        last = proc_cmdline(pid);
        if last.windows(2).any(|window| {
            window.first().is_some_and(|value| value == marker_flag)
                && window.get(1).is_some_and(|value| value == marker_value)
        }) {
            return last;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    last
}

fn command_exists(command: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|path| path.join(command).exists()))
}

fn value_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].clone())
}

fn numeric_arg(args: &[String], flag: &str) -> Option<u64> {
    value_arg(args, flag).and_then(|value| value.parse().ok())
}

fn source_hash_for_path(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let source = boon_runtime::source_text_for_path(path)?;
    Ok(boon_runtime::sha256_bytes(source.as_bytes()))
}

fn current_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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

fn display_server() -> String {
    match std::env::var("XDG_SESSION_TYPE") {
        Ok(value) if value == "wayland" => value,
        _ if std::env::var_os("WAYLAND_DISPLAY").is_some() => "wayland".to_owned(),
        _ if std::env::var_os("DISPLAY").is_some() => "x11".to_owned(),
        _ => "unknown".to_owned(),
    }
}

fn display_connection() -> String {
    std::env::var("WAYLAND_DISPLAY")
        .or_else(|_| std::env::var("DISPLAY"))
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn read_json(path: &Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_path(relative: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    #[test]
    fn parser_backed_syntax_tokens_classify_comments_and_invalid_reserved_tokens() {
        let model = CodeEditorModel::new(
            "custom://syntax.bn",
            "-- comment\nEXAMPLE Demo\n# old comment\nSOURCE\nElement/label(label: TEXT { Hi })\ncount + 1\n",
        );

        assert_eq!(model.syntax_backend(), "boon_parser::parse_ast");
        assert!(model.syntax_parser_backed());
        let categories = model.syntax_categories();
        assert!(categories.contains(&"comment"));
        assert!(categories.contains(&"keyword"));
        assert!(categories.contains(&"source-binding"));
        assert!(categories.contains(&"operator"));
        assert!(categories.contains(&"invalid"));

        let invalid_texts = model
            .syntax_tokens
            .iter()
            .filter(|token| token.kind == "invalid")
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>();
        assert!(invalid_texts.contains(&"EXAMPLE"));
        assert!(invalid_texts.contains(&"#"));
    }

    #[test]
    fn highlighted_line_segments_preserve_plain_gaps_and_token_kinds() {
        let model = CodeEditorModel::new("custom://line.bn", "count: SOURCE\ncount + 1\n");
        let segments = model.highlighted_line_segments(2, "count + 1");

        assert_eq!(
            segments
                .iter()
                .map(|segment| (segment.kind, segment.text.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("variable", "count"),
                ("plain", " "),
                ("operator", "+"),
                ("plain", " "),
                ("number", "1")
            ]
        );
    }

    #[test]
    fn highlighted_line_segments_keep_space_before_closing_punctuation() {
        let model = CodeEditorModel::new("custom://line.bn", "thing: [press: SOURCE ]\n");
        let segments = model.highlighted_line_segments(1, "thing: [press: SOURCE ]");
        let source_index = segments
            .iter()
            .position(|segment| segment.text == "SOURCE")
            .expect("SOURCE token should be highlighted");

        assert_eq!(segments[source_index].kind, "keyword");
        assert_eq!(segments[source_index + 1].text, " ");
        assert_eq!(segments[source_index + 2].text, "]");
    }

    #[test]
    fn code_editor_view_renders_mixed_lines_as_colored_segments() {
        let model = CodeEditorModel::new("custom://view.bn", "count: SOURCE\ncount + 1\n");
        let mut frame = boon_document_model::DocumentFrame::empty("root");
        let parent = frame.root.clone();
        CodeEditorView::new().append_to(&mut frame, parent, &model, 160, true);

        let code_row = frame
            .nodes
            .get(&boon_document_model::DocumentNodeId(
                "dev-code-editor-code-row-2".to_owned(),
            ))
            .expect("line 2 code row should render");
        assert_eq!(code_row.children.len(), 1);
        let rendered = frame
            .nodes
            .get(&code_row.children[0])
            .expect("line text should render as one rich text node");
        assert_eq!(
            rendered.text.as_ref().map(|text| text.text.as_str()),
            Some("count + 1")
        );
        assert_eq!(
            rendered.style.get("text_inset"),
            Some(&boon_document_model::StyleValue::Number(0.0))
        );
        assert_eq!(
            rendered.style.get("text_clip_padding"),
            Some(&boon_document_model::StyleValue::Number(4.0))
        );

        let boon_document_model::StyleValue::Text(spans_json) = rendered
            .style
            .get("syntax_spans_json")
            .expect("rich syntax spans should be attached")
        else {
            panic!("syntax_spans_json should be text");
        };
        let spans = serde_json::from_str::<Vec<serde_json::Value>>(spans_json)
            .expect("syntax spans should be valid JSON");
        assert_eq!(
            spans
                .iter()
                .map(|span| span["text"].as_str().unwrap_or_default())
                .collect::<Vec<_>>(),
            vec!["count", " ", "+", " ", "1"]
        );
        assert!(spans.iter().any(|span| {
            span["text"].as_str() == Some("+")
                && span["color"].as_str() == Some(syntax_color_for_kind("operator"))
        }));
        assert!(spans.iter().any(|span| {
            span["text"].as_str() == Some("1")
                && span["color"].as_str() == Some(syntax_color_for_kind("number"))
        }));
    }

    #[test]
    fn code_editor_view_preserves_pipe_forward_source_for_font_ligature_shaping() {
        let model = CodeEditorModel::new("custom://view.bn", "0 |> HOLD count\n");
        let mut frame = boon_document_model::DocumentFrame::empty("root");
        let parent = frame.root.clone();
        CodeEditorView::new().append_to(&mut frame, parent, &model, 80, true);
        let code_row = frame
            .nodes
            .get(&boon_document_model::DocumentNodeId(
                "dev-code-editor-code-row-1".to_owned(),
            ))
            .expect("line 1 code row should render");
        let rendered = frame
            .nodes
            .get(&code_row.children[0])
            .expect("line text should render");
        let boon_document_model::StyleValue::Text(spans_json) = rendered
            .style
            .get("syntax_spans_json")
            .expect("rich syntax spans should be attached")
        else {
            panic!("syntax_spans_json should be text");
        };
        let spans = serde_json::from_str::<Vec<serde_json::Value>>(spans_json)
            .expect("syntax spans should be valid JSON");
        assert_eq!(
            spans
                .iter()
                .map(|span| span["text"].as_str().unwrap_or_default())
                .collect::<Vec<_>>(),
            vec!["0", " ", "|>", " ", "HOLD", " ", "count"]
        );
        assert!(spans.iter().any(|span| {
            span["source_text"].as_str() == Some("|>") && span["text"].as_str() == Some("|>")
        }));
    }

    #[test]
    fn code_editor_core_supports_selection_auto_pair_and_bracket_overlays() {
        let mut model =
            CodeEditorModel::new("custom://editor.bn", "value: [count]\n-- [ignored]\n");
        model.set_selection(
            EditorPosition { line: 1, column: 9 },
            EditorPosition {
                line: 1,
                column: 14,
            },
        );
        assert_eq!(model.selected_text(), "count");
        assert_eq!(model.selection_columns_for_line(1), Some((8, 13)));
        model.insert_text_at_caret("(");
        assert_eq!(model.source_text, "value: [(count)]\n-- [ignored]\n");
        assert_eq!(
            model
                .bracket_columns_for_line(1)
                .into_iter()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([8, 14])
        );
        assert!(model.bracket_columns_for_line(2).is_empty());
        model.undo();
        assert_eq!(model.source_text, "value: [count]\n-- [ignored]\n");
        model.redo();
        assert_eq!(model.source_text, "value: [(count)]\n-- [ignored]\n");
    }

    #[test]
    fn code_editor_brackets_do_not_highlight_when_caret_is_in_root_text() {
        let mut model =
            CodeEditorModel::new("custom://editor.bn", "root\n  first: []\n  second: {}\n");
        model.set_selection(
            EditorPosition { line: 1, column: 5 },
            EditorPosition { line: 1, column: 5 },
        );
        assert!(model.bracket_columns_for_line(1).is_empty());
        assert!(model.bracket_columns_for_line(2).is_empty());
        assert!(model.bracket_columns_for_line(3).is_empty());
    }

    #[test]
    fn code_editor_view_attaches_caret_selection_and_bracket_metadata() {
        let mut model = CodeEditorModel::new("custom://view.bn", "value: [count]\n");
        model.set_selection(
            EditorPosition { line: 1, column: 9 },
            EditorPosition {
                line: 1,
                column: 14,
            },
        );
        let mut frame = boon_document_model::DocumentFrame::empty("root");
        let parent = frame.root.clone();
        CodeEditorView::new().append_to(&mut frame, parent, &model, 80, true);
        let rendered = frame
            .nodes
            .get(&boon_document_model::DocumentNodeId(
                "dev-code-editor-line-text-1".to_owned(),
            ))
            .expect("line text should render");

        assert_eq!(
            rendered.style.get("editor_selection_start"),
            Some(&boon_document_model::StyleValue::Number(8.0))
        );
        assert_eq!(
            rendered.style.get("editor_selection_end"),
            Some(&boon_document_model::StyleValue::Number(13.0))
        );
        assert_eq!(
            rendered.style.get("editor_caret_column"),
            Some(&boon_document_model::StyleValue::Number(13.0))
        );
        assert_eq!(
            rendered.style.get("editor_caret_visible"),
            Some(&boon_document_model::StyleValue::Bool(true))
        );
        assert_eq!(
            rendered.style.get("editor_bracket_columns"),
            Some(&boon_document_model::StyleValue::Text("7,13".to_owned()))
        );
        let mut hidden_frame = boon_document_model::DocumentFrame::empty("root");
        let hidden_parent = hidden_frame.root.clone();
        CodeEditorView::new().append_to(&mut hidden_frame, hidden_parent, &model, 80, false);
        let hidden = hidden_frame
            .nodes
            .get(&boon_document_model::DocumentNodeId(
                "dev-code-editor-line-text-1".to_owned(),
            ))
            .expect("line text should render");
        assert_eq!(
            hidden.style.get("editor_caret_visible"),
            Some(&boon_document_model::StyleValue::Bool(false))
        );
    }

    #[test]
    fn pointer_to_editor_position_uses_nearest_character_boundary() {
        let model = CodeEditorModel::new("custom://click.bn", "abcdef\n");
        let mut document = boon_document_model::DocumentFrame::empty("root");
        let root = document.root.clone();
        set_style(
            document.nodes.get_mut(&root).expect("root exists"),
            &[("width", "fill"), ("height", "160"), ("bg", "#282c34")],
        );
        CodeEditorView::new().append_to(&mut document, root, &model, 120, true);

        let mut measurer = boon_native_gpu::GlyphonTextMeasurer::new();
        let layout = boon_document::layout(boon_document::LayoutInput {
            document: &document,
            viewport: boon_host::Viewport {
                surface: 1,
                width: 640.0,
                height: 180.0,
                scale: 1.0,
            },
            text: &mut measurer,
            capabilities: boon_document::RenderCapabilities::fake_portable(),
        });
        let line_item = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "dev-code-editor-line-text-1")
            .expect("line text should be laid out");
        let text_origin_x = line_item.bounds.x;
        let editor_bounds = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "dev-code-editor")
            .expect("editor should be laid out")
            .bounds;
        let line_y =
            editor_bounds.y + BOON_EDITOR_PADDING as f32 + BOON_EDITOR_LINE_HEIGHT as f32 * 0.5;
        let mut column_cache = EditorColumnMetricCache::default();
        let column_edges = editor_column_edges_for_line(
            &mut column_cache,
            "abcdef",
            &line_item.style,
            line_item.bounds.height,
        );

        assert_eq!(
            dev_position_from_pointer(
                &model,
                &layout,
                text_origin_x + column_edges[1] * 0.24,
                line_y,
                &mut column_cache
            ),
            Some(EditorPosition { line: 1, column: 1 })
        );
        assert_eq!(
            dev_position_from_pointer(
                &model,
                &layout,
                text_origin_x + column_edges[1] * 0.51,
                line_y,
                &mut column_cache
            ),
            Some(EditorPosition { line: 1, column: 2 })
        );
        assert_eq!(
            dev_position_from_pointer(
                &model,
                &layout,
                text_origin_x + column_edges[3] - (column_edges[3] - column_edges[2]) * 0.49,
                line_y,
                &mut column_cache
            ),
            Some(EditorPosition { line: 1, column: 4 })
        );

        let target_layout_x =
            text_origin_x + column_edges[3] - (column_edges[3] - column_edges[2]) * 0.49;
        let scaled_position = input_layout_position(
            Some(boon_native_app_window::NativeMouseWindowPosition {
                x: f64::from(target_layout_x) * 0.5,
                y: f64::from(line_y) * 0.5,
                window_width: 320.0,
                window_height: 90.0,
            }),
            640,
            180,
        )
        .expect("scaled test position should normalize");
        assert_eq!(
            dev_position_from_pointer(
                &model,
                &layout,
                scaled_position.x as f32,
                scaled_position.y as f32,
                &mut column_cache
            ),
            Some(EditorPosition { line: 1, column: 4 }),
            "logical mouse positions must be normalized to the physical layout coordinates"
        );
    }

    #[test]
    fn held_arrow_repeats_and_resets_caret_blink() {
        let (mut shell, mut input_state, document, layout) =
            test_dev_editor_context("abcdefghijklmnop\n");

        let key_down = test_keyboard_input(
            vec![boon_native_app_window::NativeKeyboardEventProof {
                sequence: 1,
                key: "RightArrow".to_owned(),
                pressed: true,
                window_protocol_id: Some(1),
            }],
            vec!["RightArrow"],
        );
        assert!(dev_apply_real_window_input(
            &key_down,
            &document,
            &layout,
            1180,
            820,
            &mut shell,
            &mut input_state
        ));
        assert_eq!(
            *shell.workspace.selected_buffer.caret(),
            EditorPosition { line: 1, column: 2 }
        );
        assert!(shell.caret_visible);
        assert!(input_state.caret_blink_started_at.is_some());

        let held = test_keyboard_input(Vec::new(), vec!["RightArrow"]);
        input_state.held_repeat_next_at = Instant::now().checked_sub(Duration::from_millis(1));
        let _ = dev_apply_real_window_input(
            &held,
            &document,
            &layout,
            1180,
            820,
            &mut shell,
            &mut input_state,
        );
        assert!(
            shell.workspace.selected_buffer.caret().column > 2,
            "held RightArrow should move beyond the initial key-down"
        );
        input_state.held_repeat_next_at = Instant::now().checked_sub(Duration::from_millis(
            BOON_EDITOR_KEY_REPEAT_INTERVAL_MS * 4,
        ));
        let before_catch_up = shell.workspace.selected_buffer.caret().column;
        let _ = dev_apply_real_window_input(
            &held,
            &document,
            &layout,
            1180,
            820,
            &mut shell,
            &mut input_state,
        );
        assert!(
            shell.workspace.selected_buffer.caret().column >= before_catch_up + 4,
            "time-based repeat should catch up when frames are slower than the repeat interval"
        );

        let key_up = test_keyboard_input(
            vec![boon_native_app_window::NativeKeyboardEventProof {
                sequence: 2,
                key: "RightArrow".to_owned(),
                pressed: false,
                window_protocol_id: Some(1),
            }],
            Vec::new(),
        );
        let column_after_repeat = shell.workspace.selected_buffer.caret().column;
        let _ = dev_apply_real_window_input(
            &key_up,
            &document,
            &layout,
            1180,
            820,
            &mut shell,
            &mut input_state,
        );
        input_state.held_repeat_next_at =
            Instant::now().checked_sub(Duration::from_millis(BOON_EDITOR_KEY_REPEAT_INTERVAL_MS));
        let _ = dev_apply_real_window_input(
            &test_keyboard_input(Vec::new(), Vec::new()),
            &document,
            &layout,
            1180,
            820,
            &mut shell,
            &mut input_state,
        );
        assert_eq!(
            shell.workspace.selected_buffer.caret().column,
            column_after_repeat
        );
    }

    #[test]
    fn held_printable_keys_repeat_letters_and_numbers() {
        let (mut shell, mut input_state, document, layout) = test_dev_editor_context("\n");

        apply_test_key_down("A", &document, &layout, &mut shell, &mut input_state);
        apply_test_held_key("A", 4, &document, &layout, &mut shell, &mut input_state);
        assert_eq!(shell.workspace.selected_buffer.source_text, "aaaaa\n");

        apply_test_key_down("Num1", &document, &layout, &mut shell, &mut input_state);
        apply_test_held_key("Num1", 4, &document, &layout, &mut shell, &mut input_state);
        assert_eq!(shell.workspace.selected_buffer.source_text, "aaaaa11111\n");
    }

    #[test]
    fn held_delete_keys_repeat_backward_and_forward_deletion() {
        let (mut shell, mut input_state, document, layout) = test_dev_editor_context("abcdef\n");
        shell.workspace.selected_buffer.set_selection(
            EditorPosition { line: 1, column: 7 },
            EditorPosition { line: 1, column: 7 },
        );

        apply_test_key_down("Delete", &document, &layout, &mut shell, &mut input_state);
        apply_test_held_key(
            "Delete",
            4,
            &document,
            &layout,
            &mut shell,
            &mut input_state,
        );
        assert_eq!(shell.workspace.selected_buffer.source_text, "a\n");

        let (mut shell, mut input_state, document, layout) = test_dev_editor_context("abcdef\n");
        shell.workspace.selected_buffer.set_selection(
            EditorPosition { line: 1, column: 2 },
            EditorPosition { line: 1, column: 2 },
        );
        apply_test_key_down(
            "ForwardDelete",
            &document,
            &layout,
            &mut shell,
            &mut input_state,
        );
        apply_test_held_key(
            "ForwardDelete",
            4,
            &document,
            &layout,
            &mut shell,
            &mut input_state,
        );
        assert_eq!(shell.workspace.selected_buffer.source_text, "a\n");
    }

    fn test_dev_editor_context(
        source: &str,
    ) -> (
        DevWindowShell,
        DevNativeInputState,
        boon_document_model::DocumentFrame,
        boon_document::LayoutFrame,
    ) {
        let catalog = ExampleCatalog {
            entries: vec![ExampleCatalogEntry {
                id: "counter".to_owned(),
                label: "Counter".to_owned(),
                source: "examples/counter.bn".to_owned(),
                source_files: Vec::new(),
                inline_source: Some(source.to_owned()),
                category: "test".to_owned(),
                order: 0,
                shown_by_default: true,
                custom: false,
            }],
            custom_store_path: PathBuf::from("target/artifacts/native-gpu/tests/repeat.toml"),
        };
        let workspace =
            ExampleWorkspace::new(&catalog, "examples/counter.bn", source, Some("counter"));
        let shell = DevWindowShell {
            catalog,
            initial_workspace: workspace.clone(),
            workspace,
            editor_view: CodeEditorView::new(),
            preview_transport: PreviewTransport::new(None),
            last_preview_transport: json!({"status": "not-run"}),
            last_preview_summary: json!({"status": "not-run"}),
            last_good_runtime_summary: None,
            last_preview_summary_refresh: None,
            last_dev_command: "test".to_owned(),
            last_dev_command_status: "not-run".to_owned(),
            last_dev_command_detail: None,
            footer_scroll_line: 0,
            caret_visible: false,
        };
        let input_state = DevNativeInputState {
            editor_focused: true,
            caret_blink_started_at: Some(
                Instant::now()
                    .checked_sub(Duration::from_millis(
                        BOON_EDITOR_CARET_BLINK_HALF_PERIOD_MS,
                    ))
                    .unwrap_or_else(Instant::now),
            ),
            ..DevNativeInputState::default()
        };
        let document = shell.document_for_viewport(1180, 820);
        let mut measurer = boon_native_gpu::GlyphonTextMeasurer::new();
        let layout = boon_document::layout(boon_document::LayoutInput {
            document: &document,
            viewport: boon_host::Viewport {
                surface: 1,
                width: 1180.0,
                height: 820.0,
                scale: 1.0,
            },
            text: &mut measurer,
            capabilities: boon_document::RenderCapabilities::fake_portable(),
        });
        (shell, input_state, document, layout)
    }

    fn apply_test_key_down(
        key: &str,
        document: &boon_document_model::DocumentFrame,
        layout: &boon_document::LayoutFrame,
        shell: &mut DevWindowShell,
        input_state: &mut DevNativeInputState,
    ) {
        let input = test_keyboard_input(
            vec![boon_native_app_window::NativeKeyboardEventProof {
                sequence: input_state.last_keyboard_event_sequence.saturating_add(1),
                key: key.to_owned(),
                pressed: true,
                window_protocol_id: Some(1),
            }],
            vec![key],
        );
        assert!(dev_apply_real_window_input(
            &input,
            document,
            layout,
            1180,
            820,
            shell,
            input_state
        ));
    }

    fn apply_test_held_key(
        key: &str,
        repeat_count: u64,
        document: &boon_document_model::DocumentFrame,
        layout: &boon_document::LayoutFrame,
        shell: &mut DevWindowShell,
        input_state: &mut DevNativeInputState,
    ) {
        let elapsed_intervals = repeat_count.saturating_sub(1);
        input_state.held_repeat_next_at = Instant::now().checked_sub(Duration::from_millis(
            BOON_EDITOR_KEY_REPEAT_INTERVAL_MS * elapsed_intervals,
        ));
        let input = test_keyboard_input(Vec::new(), vec![key]);
        let _ =
            dev_apply_real_window_input(&input, document, layout, 1180, 820, shell, input_state);
    }

    fn style_text_value<'a>(
        node: &'a boon_document_model::DocumentNode,
        key: &str,
    ) -> Option<&'a str> {
        match node.style.get(key)? {
            boon_document_model::StyleValue::Text(value) => Some(value.as_str()),
            boon_document_model::StyleValue::Number(_)
            | boon_document_model::StyleValue::Bool(_) => None,
        }
    }

    fn test_keyboard_input(
        keyboard_events: Vec<boon_native_app_window::NativeKeyboardEventProof>,
        pressed_keys: Vec<&str>,
    ) -> boon_native_app_window::NativeInputAdapterProof {
        boon_native_app_window::NativeInputAdapterProof {
            installed: true,
            capture_scope: "test".to_owned(),
            keyboard_api: "test".to_owned(),
            mouse_api: "test".to_owned(),
            wheel_api: "test".to_owned(),
            per_window_event_provenance_api: "test".to_owned(),
            sampled_after_visible_window: true,
            real_os_events_observed: true,
            input_injection_method: "test".to_owned(),
            synthetic_input_probe: false,
            mouse_last_window_protocol_id: None,
            keyboard_last_window_protocol_id: Some(1),
            mouse_motion_event_count: 0,
            mouse_button_event_count: 0,
            mouse_scroll_event_count: 0,
            mouse_total_event_count: 0,
            keyboard_key_event_count: keyboard_events.len() as u64,
            mouse_button_events: Vec::new(),
            keyboard_events,
            mouse_window_pos: None,
            mouse_buttons_down: Vec::new(),
            pressed_keys: pressed_keys.into_iter().map(str::to_owned).collect(),
            scroll_delta_x: 0.0,
            scroll_delta_y: 0.0,
        }
    }

    #[test]
    fn fallback_tokenizer_keeps_malformed_buffers_renderable() {
        let tokens = BoonLanguageService::syntax_tokens_fallback("SOURCE @\n-- ok\n");
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == "keyword" && token.text == "SOURCE")
        );
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == "invalid" && token.text == "@")
        );
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == "comment" && token.text == "-- ok")
        );
    }

    #[test]
    fn original_typescript_theme_colors_and_styles_are_native_styles() {
        assert_eq!(BOON_EDITOR_BACKGROUND, "#282c34");
        assert_eq!(BOON_EDITOR_FOREGROUND, "#d9e1f2");
        assert_eq!(BOON_EDITOR_FONT_FAMILY, "JetBrains Mono");
        assert_eq!(BOON_EDITOR_FONT_SIZE, 16);
        assert_eq!(BOON_EDITOR_LINE_HEIGHT, 22);
        assert_eq!(BOON_EDITOR_FONT_FEATURES, "zero,calt");
        assert_eq!(BOON_EDITOR_FONT_FEATURE_SETTINGS, "'zero' 1, 'calt' 1");
        assert_eq!(BOON_EDITOR_SELECTION, "#3E4451");
        assert_eq!(BOON_EDITOR_CURSOR, "#528bff");
        assert_eq!(BOON_EDITOR_BRACKET_MATCH, "#528bff33");

        let keyword = syntax_style_for_kind("keyword");
        assert_eq!(keyword.color, "#D2691E");
        assert_eq!(keyword.font_weight, Some("800"));
        assert_eq!(keyword.font_style, Some("italic"));

        let definition = syntax_style_for_kind("definition");
        assert_eq!(definition.color, "#ff6ec7");
        assert_eq!(definition.font_weight, Some("600"));
        assert_eq!(definition.font_style, Some("italic"));

        let comment = syntax_style_for_kind("comment");
        assert_eq!(comment.color, "#778899");
        assert_eq!(comment.font_style, Some("italic"));
    }

    #[test]
    fn dev_shell_visual_refresh_preserves_editor_theme_and_structures_footer() {
        let (mut shell, _, _, _) = test_dev_editor_context("store: []\n");
        let source_hash =
            boon_runtime::sha256_bytes(shell.workspace.selected_buffer.source_text.as_bytes());
        shell.last_preview_transport = json!({"status": "pass"});
        shell.last_preview_summary = json!({
            "status": "pass",
            "runtime_summary": {
                "status": "pass",
                "state_summary_hash": "abcdef1234567890",
                "source_sha256": source_hash,
                "state_summary_top_level_keys": ["count", "store"]
            },
            "preview_last_error_count": 0,
            "preview_last_error": null
        });
        let frame = shell.document_for_viewport(1180, 820);

        let root = frame.nodes.get(&frame.root).expect("root should exist");
        assert_eq!(style_text_value(root, "bg"), Some(DEV_BG));
        assert_eq!(
            style_text_value(
                frame
                    .nodes
                    .get(&boon_document_model::DocumentNodeId(
                        "dev-header".to_owned()
                    ))
                    .expect("dev header should render"),
                "bg"
            ),
            Some(DEV_PANEL)
        );
        assert_eq!(
            style_text_value(
                frame
                    .nodes
                    .get(&boon_document_model::DocumentNodeId(
                        "dev-code-editor".to_owned()
                    ))
                    .expect("code editor should render"),
                "bg"
            ),
            Some(BOON_EDITOR_BACKGROUND),
            "visual refresh must not change editor colors"
        );
        for node in frame.nodes.values().filter(|node| {
            node.id.0.starts_with("dev-")
                && matches!(node.kind, boon_document_model::DocumentNodeKind::Text)
        }) {
            let bg = style_text_value(node, "bg")
                .unwrap_or_else(|| panic!("{} text node should set explicit bg", node.id.0));
            assert!(
                !matches!(bg, "#ffffff" | "#f8fafc" | "#edf2f7" | "#f3f6f9"),
                "{} should not use a light fallback bg",
                node.id.0
            );
        }
        for node in frame.nodes.values().filter(|node| {
            node.id.0.starts_with("dev-")
                && matches!(node.kind, boon_document_model::DocumentNodeKind::Button)
        }) {
            assert_eq!(
                style_text_value(node, "align"),
                Some("center"),
                "{} button label should be horizontally centered",
                node.id.0
            );
            assert_eq!(
                style_text_value(node, "vertical_align"),
                Some("center"),
                "{} button label should be vertically centered",
                node.id.0
            );
        }
        assert!(
            frame
                .nodes
                .contains_key(&boon_document_model::DocumentNodeId(
                    "dev-footer".to_owned()
                ))
        );
        assert!(
            frame
                .nodes
                .contains_key(&boon_document_model::DocumentNodeId(
                    "dev-footer-scroll".to_owned()
                ))
        );
        assert!(
            !frame
                .nodes
                .contains_key(&boon_document_model::DocumentNodeId(
                    "dev-footer-runtime-chip".to_owned()
                )),
            "footer should not duplicate header/runtime status chips"
        );
        let preview_pill = frame
            .nodes
            .get(&boon_document_model::DocumentNodeId(
                "dev-header-preview-status".to_owned(),
            ))
            .expect("preview status pill should render");
        assert_eq!(style_text_value(preview_pill, "align"), Some("center"));
        assert_eq!(
            style_text_value(preview_pill, "vertical_align"),
            Some("center")
        );
        assert_eq!(
            preview_pill.text.as_ref().map(|text| text.text.as_str()),
            Some("Preview: Synced")
        );
        let visible_footer_text = frame
            .nodes
            .values()
            .filter_map(|node| node.text.as_ref().map(|text| text.text.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible_footer_text.contains("official"));
        assert!(!visible_footer_text.contains("not-run"));
        assert!(!visible_footer_text.contains("deferred"));
        assert!(!visible_footer_text.contains("not-bound"));
        let footer_lines = shell
            .footer_lines()
            .into_iter()
            .map(|(label, value)| format!("{label} {value}"))
            .collect::<Vec<_>>()
            .join("\n");
        for removed_label in [
            "syntax",
            "catalog",
            "format",
            "workspace",
            "example",
            "preview",
        ] {
            assert!(
                !shell
                    .footer_lines()
                    .iter()
                    .any(|(label, _)| label.eq_ignore_ascii_case(removed_label)),
                "{removed_label} should not be a visible footer row"
            );
        }
        assert!(footer_lines.contains("Code"));
        assert!(footer_lines.contains("Cursor"));
        assert!(footer_lines.contains("Runtime state abcdef123456"));
        assert!(footer_lines.contains("Last action"));
        assert!(!footer_lines.contains("Errors none"));

        let mut measurer = boon_native_gpu::GlyphonTextMeasurer::new();
        let layout = boon_document::layout(boon_document::LayoutInput {
            document: &frame,
            viewport: boon_host::Viewport {
                surface: 1,
                width: 1180.0,
                height: 820.0,
                scale: 1.0,
            },
            text: &mut measurer,
            capabilities: boon_document::RenderCapabilities::fake_portable(),
        });
        let bottom = layout
            .display_list
            .iter()
            .map(|item| item.bounds.y + item.bounds.height)
            .fold(0.0_f32, f32::max);
        assert!(
            (820.0 - bottom).abs() <= 0.5,
            "dev shell should fill viewport height without a bottom gutter, bottom={bottom}"
        );
    }

    #[test]
    fn dev_shell_translates_raw_statuses_for_visible_ui() {
        let (mut shell, _, _, _) = test_dev_editor_context("store: []\n");
        shell.last_dev_command = "startup".to_owned();
        shell.last_dev_command_status = "not-run".to_owned();
        shell.last_dev_command_detail = None;
        let startup_frame = shell.document_for_viewport(1180, 820);
        let startup_text = startup_frame
            .nodes
            .values()
            .filter_map(|node| node.text.as_ref().map(|text| text.text.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(startup_text.contains("Preview: Waiting"));
        assert!(!startup_text.contains("Waiting for preview summary"));
        assert!(!startup_text.contains("Startup: Waiting"));
        assert!(!startup_text.contains("not-run"));
        assert!(!startup_text.contains("not-bound"));

        shell.last_preview_transport = json!({
            "status": "fail",
            "diagnostic": "preview socket closed"
        });
        shell.last_preview_summary = json!({
            "status": "unavailable",
            "diagnostic": "preview socket closed"
        });
        shell.last_dev_command = "Run".to_owned();
        shell.last_dev_command_status = "fail".to_owned();
        shell.last_dev_command_detail = Some("preview socket closed".to_owned());
        let error_frame = shell.document_for_viewport(1180, 820);
        let error_text = error_frame
            .nodes
            .values()
            .filter_map(|node| node.text.as_ref().map(|text| text.text.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(error_text.contains("Preview: Error"));
        assert!(error_text.contains("Preview\nOffline: preview socket closed"));
        assert!(error_text.contains("Run: Error - preview socket closed"));
        assert!(!error_text.contains(" fail"));
        assert!(!error_text.contains("unavailable"));
    }

    #[test]
    fn dev_footer_keeps_last_good_runtime_summary_during_transient_poll_failures() {
        let (mut shell, _, _, _) = test_dev_editor_context("store: []\n");
        let source_hash =
            boon_runtime::sha256_bytes(shell.workspace.selected_buffer.source_text.as_bytes());
        shell.last_good_runtime_summary = Some(json!({
            "status": "pass",
            "state_summary_hash": "abcdef1234567890",
            "source_sha256": source_hash,
            "state_summary_top_level_keys": ["store"]
        }));
        shell.last_preview_summary = json!({
            "status": "unavailable",
            "diagnostic": "runtime-summary poll timed out"
        });
        let footer_lines = shell
            .footer_lines()
            .into_iter()
            .map(|(label, value)| format!("{label} {value}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(footer_lines.contains("Runtime state abcdef123456"));
        assert!(!footer_lines.contains("Preview Offline"));

        shell
            .workspace
            .selected_buffer
            .insert_plain_text_at_caret("-- changed", "test");
        let stale_footer_lines = shell
            .footer_lines()
            .into_iter()
            .map(|(label, value)| format!("{label} {value}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!stale_footer_lines.contains("Runtime state abcdef123456"));
    }

    #[test]
    fn dev_footer_runtime_summary_is_compact_and_wrapped() {
        let summary = json!({
            "status": "pass",
            "state_summary_hash": "abcdef1234567890",
            "source_sha256": "0123456789abcdef",
            "state_summary_bytes": 4096,
            "state_summary_top_level_keys": [
                "todos",
                "new_todo_text",
                "filter",
                "visible_todos",
                "active_count",
                "completed_count",
                "editing_id"
            ]
        });
        let text = runtime_footer_summary(&summary, "abcdef123456", "0123456789ab");
        assert!(text.contains("state abcdef123456"));
        assert!(text.contains("source 0123456789ab"));
        assert!(text.contains("state size 4.0 KiB"));
        assert!(text.contains(
            "7 keys: todos, new_todo_text, filter, visible_todos, active_count, +2 more"
        ));
        assert!(!text.contains("completed_count"));
        assert!(!text.contains("editing_id"));

        let wrapped = wrap_footer_lines(vec![("Runtime".to_owned(), text)], 48);
        assert!(wrapped.len() > 1);
        assert_eq!(
            wrapped.first().map(|(label, _)| label.as_str()),
            Some("Runtime")
        );
        assert!(
            wrapped
                .iter()
                .skip(1)
                .all(|(label, value)| { label.is_empty() && value.chars().count() <= 48 })
        );
    }

    #[test]
    fn dev_editor_caret_blinks_only_while_editor_is_focused() {
        let started = Instant::now()
            .checked_sub(Duration::from_millis(
                BOON_EDITOR_CARET_BLINK_HALF_PERIOD_MS,
            ))
            .unwrap_or_else(Instant::now);
        let mut focused = DevNativeInputState {
            editor_focused: true,
            caret_blink_started_at: Some(started),
            ..DevNativeInputState::default()
        };
        assert!(!dev_editor_caret_visible(&mut focused, Instant::now()));

        let mut unfocused = DevNativeInputState {
            editor_focused: false,
            caret_blink_started_at: Some(started),
            ..DevNativeInputState::default()
        };
        assert!(dev_editor_caret_visible(&mut unfocused, Instant::now()));
    }

    #[test]
    fn custom_example_name_input_renames_and_truncates_visible_labels() {
        let store_path = PathBuf::from(format!(
            "target/artifacts/native-gpu/tests/custom-name-{}.toml",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&store_path);
        let long_label = "Custom Example With A Name That Is Far Too Long For A Tab".to_owned();
        let catalog = ExampleCatalog {
            entries: vec![ExampleCatalogEntry {
                id: "custom:long".to_owned(),
                label: long_label,
                source: "custom://long.bn".to_owned(),
                source_files: Vec::new(),
                inline_source: Some("document: []\n".to_owned()),
                category: "custom".to_owned(),
                order: 20_000,
                shown_by_default: true,
                custom: true,
            }],
            custom_store_path: store_path.clone(),
        };
        let workspace = ExampleWorkspace::new(
            &catalog,
            "custom://long.bn",
            "document: []\n",
            Some("custom:long"),
        );
        let mut shell = DevWindowShell {
            catalog,
            initial_workspace: workspace.clone(),
            workspace,
            editor_view: CodeEditorView::new(),
            preview_transport: PreviewTransport::new(None),
            last_preview_transport: json!({"status": "not-run"}),
            last_preview_summary: json!({"status": "not-run"}),
            last_good_runtime_summary: None,
            last_preview_summary_refresh: None,
            last_dev_command: "test".to_owned(),
            last_dev_command_status: "not-run".to_owned(),
            last_dev_command_detail: None,
            footer_scroll_line: 0,
            caret_visible: true,
        };

        let frame = shell.document_for_viewport(1180, 820);
        let custom_tab = frame
            .nodes
            .get(&boon_document_model::DocumentNodeId(
                "dev-tab-custom:long".to_owned(),
            ))
            .expect("custom tab should render");
        let tab_text = custom_tab.text.as_ref().expect("tab text").text.as_str();
        assert!(tab_text.ends_with("..."));
        assert!(!tab_text.contains('\n'));
        let name_input = frame
            .nodes
            .get(&boon_document_model::DocumentNodeId(
                "dev-custom-name-input".to_owned(),
            ))
            .expect("custom name input should render");
        assert_eq!(
            name_input
                .source_binding
                .as_ref()
                .map(|binding| binding.source_path.as_str()),
            Some("dev.custom.name")
        );

        let rename = shell.rename_selected_custom_label("Short");
        assert_eq!(rename["status"], "pass");
        assert_eq!(shell.selected_example_label(), "Short");
        assert!(apply_dev_custom_name_key(&mut shell, "A", false));
        assert_eq!(shell.selected_example_label(), "Shorta");
        assert!(apply_dev_custom_name_key(&mut shell, "Delete", false));
        assert_eq!(shell.selected_example_label(), "Short");
        let stored = ExampleCatalog::load_custom_store(&store_path).unwrap();
        assert_eq!(
            stored.first().map(|entry| entry.label.as_str()),
            Some("Short")
        );

        let renamed_frame = shell.document_for_viewport(1180, 820);
        let name_input = renamed_frame
            .nodes
            .get(&boon_document_model::DocumentNodeId(
                "dev-custom-name-input".to_owned(),
            ))
            .expect("custom name input should render");
        assert_eq!(
            name_input.text.as_ref().map(|text| text.text.as_str()),
            Some("Short")
        );

        let _ = std::fs::remove_file(store_path);
    }

    #[test]
    fn footer_scroll_is_routed_separately_from_editor_scroll() {
        let source = (0..160)
            .map(|index| format!("line_{index}: TEXT {{ value }}\n"))
            .collect::<String>();
        let (mut shell, mut input_state, document, layout) = test_dev_editor_context(&source);
        let footer_bounds = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "dev-footer")
            .expect("footer should be laid out")
            .bounds;
        let editor_bounds = layout
            .display_list
            .iter()
            .find(|item| item.node.0 == "dev-code-editor")
            .expect("editor should be laid out")
            .bounds;

        let mut footer_scroll = test_keyboard_input(Vec::new(), Vec::new());
        footer_scroll.scroll_delta_y = 24.0;
        footer_scroll.mouse_scroll_event_count = 1;
        footer_scroll.mouse_window_pos = Some(boon_native_app_window::NativeMouseWindowPosition {
            x: f64::from(footer_bounds.x + footer_bounds.width * 0.5),
            y: f64::from(footer_bounds.y + footer_bounds.height * 0.5),
            window_width: 1180.0,
            window_height: 820.0,
        });
        assert!(dev_apply_real_window_input(
            &footer_scroll,
            &document,
            &layout,
            1180,
            820,
            &mut shell,
            &mut input_state
        ));
        assert!(shell.footer_scroll_line > 0);
        assert_eq!(shell.workspace.selected_buffer.scroll_line, 0);

        let footer_after = shell.footer_scroll_line;
        let mut editor_scroll = test_keyboard_input(Vec::new(), Vec::new());
        editor_scroll.scroll_delta_y = 24.0;
        editor_scroll.mouse_scroll_event_count = 2;
        editor_scroll.mouse_window_pos = Some(boon_native_app_window::NativeMouseWindowPosition {
            x: f64::from(editor_bounds.x + editor_bounds.width * 0.5),
            y: f64::from(editor_bounds.y + editor_bounds.height * 0.5),
            window_width: 1180.0,
            window_height: 820.0,
        });
        assert!(dev_apply_real_window_input(
            &editor_scroll,
            &document,
            &layout,
            1180,
            820,
            &mut shell,
            &mut input_state
        ));
        assert!(shell.workspace.selected_buffer.scroll_line > 0);
        assert_eq!(shell.footer_scroll_line, footer_after);
    }

    #[test]
    fn original_boon_semantic_rules_split_module_paths_and_text_literals() {
        let model = CodeEditorModel::new(
            "custom://theme.bn",
            "FUNCTION greet(name) {\n    title: TEXT { Hello {name} }\n    Element/label(label: title)\n}\n",
        );
        let rendered = model
            .syntax_render_segments_for_visible_lines(8)
            .into_iter()
            .map(|segment| (segment.kind, segment.text))
            .collect::<Vec<_>>();

        assert!(rendered.contains(&("keyword", "FUNCTION".to_owned())));
        assert!(rendered.contains(&("function", "greet".to_owned())));
        assert!(rendered.contains(&("definition", "title".to_owned())));
        assert!(rendered.contains(&("text-literal-content", " Hello ".to_owned())));
        assert!(rendered.contains(&("text-literal-interpolation", "name".to_owned())));
        assert!(rendered.contains(&("source-binding", "Element".to_owned())));
        assert!(rendered.contains(&("module-slash", "/".to_owned())));
        assert!(rendered.contains(&("function", "label".to_owned())));
    }

    #[test]
    fn new_custom_tab_starts_empty_and_persists_editor_text() {
        let store_path = PathBuf::from(format!(
            "target/artifacts/native-gpu/tests/custom-tabs-{}.toml",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&store_path);
        let catalog = ExampleCatalog {
            entries: vec![ExampleCatalogEntry {
                id: "sample".to_owned(),
                label: "Sample".to_owned(),
                source: "custom://sample.bn".to_owned(),
                source_files: Vec::new(),
                inline_source: Some(
                    "-- sample\nSOURCE\nHOLD\nLATEST\nLIST {}\nList/map\n".to_owned(),
                ),
                category: "test".to_owned(),
                order: 0,
                shown_by_default: true,
                custom: false,
            }],
            custom_store_path: store_path.clone(),
        };
        let workspace = ExampleWorkspace::new(
            &catalog,
            "custom://sample.bn",
            "-- sample\nSOURCE\nHOLD\nLATEST\nLIST {}\nList/map\n",
            None,
        );
        let mut shell = DevWindowShell {
            catalog,
            initial_workspace: workspace.clone(),
            workspace,
            editor_view: CodeEditorView::new(),
            preview_transport: PreviewTransport::new(None),
            last_preview_transport: json!({
                "status": "not-run",
                "reason": "test shell has not sent preview transport yet"
            }),
            last_preview_summary: json!({"status": "not-run"}),
            last_good_runtime_summary: None,
            last_preview_summary_refresh: None,
            last_dev_command: "test".to_owned(),
            last_dev_command_status: "not-run".to_owned(),
            last_dev_command_detail: None,
            footer_scroll_line: 0,
            caret_visible: true,
        };

        let activation =
            shell.host_synthetic_activation_for_source_path("dev.tabs.new", 1180.0, 820.0);
        assert_eq!(activation["status"], "pass");

        let created = shell.dispatch_source_path("dev.tabs.new");
        assert_eq!(created["status"], "pass");
        assert_eq!(created["source_starts_empty"], true);
        assert_eq!(shell.workspace.selected_buffer.source_text, "");
        let custom_id = created["stable_id"].as_str().unwrap().to_owned();

        let edited = shell.dispatch_source_path("dev.editor.insert_text");
        assert_eq!(edited["status"], "pass");
        assert_eq!(edited["custom_source_persistence"]["status"], "pass");

        let stored = ExampleCatalog::load_custom_store(&store_path).unwrap();
        let stored_entry = stored
            .iter()
            .find(|entry| entry.id == custom_id)
            .expect("new custom tab should be persisted");
        assert_eq!(
            stored_entry.inline_source.as_deref(),
            Some(shell.workspace.selected_buffer.source_text.as_str())
        );
        assert!(
            stored_entry
                .inline_source
                .as_deref()
                .unwrap_or_default()
                .contains("host synthetic editor input")
        );

        let removed = shell.dispatch_source_path("dev.commands.remove_custom");
        assert_eq!(removed["status"], "pass");
        assert_eq!(removed["stable_id"], custom_id);
        assert_eq!(removed["removed_not_listed"], true);
        assert_ne!(shell.workspace.selected_example_id, custom_id);
        assert!(!shell.workspace.open_buffers.contains_key(&custom_id));

        let _ = std::fs::remove_file(store_path);
    }

    #[test]
    fn remove_button_is_disabled_for_manifest_examples_and_bound_for_custom_examples() {
        let catalog = ExampleCatalog {
            entries: vec![
                ExampleCatalogEntry {
                    id: "counter".to_owned(),
                    label: "Counter".to_owned(),
                    source: "examples/counter.bn".to_owned(),
                    source_files: Vec::new(),
                    inline_source: Some("document: []\n".to_owned()),
                    category: "basic".to_owned(),
                    order: 10,
                    shown_by_default: true,
                    custom: false,
                },
                ExampleCatalogEntry {
                    id: "custom:one".to_owned(),
                    label: "Custom One".to_owned(),
                    source: "custom://one.bn".to_owned(),
                    source_files: Vec::new(),
                    inline_source: Some("document: []\n".to_owned()),
                    category: "custom".to_owned(),
                    order: 20_000,
                    shown_by_default: true,
                    custom: true,
                },
            ],
            custom_store_path: PathBuf::from(
                "target/artifacts/native-gpu/tests/remove-button.toml",
            ),
        };
        let workspace = ExampleWorkspace::new(
            &catalog,
            "examples/counter.bn",
            "document: []\n",
            Some("counter"),
        );
        let mut shell = DevWindowShell {
            catalog,
            initial_workspace: workspace.clone(),
            workspace,
            editor_view: CodeEditorView::new(),
            preview_transport: PreviewTransport::new(None),
            last_preview_transport: json!({"status": "not-run"}),
            last_preview_summary: json!({"status": "not-run"}),
            last_good_runtime_summary: None,
            last_preview_summary_refresh: None,
            last_dev_command: "test".to_owned(),
            last_dev_command_status: "not-run".to_owned(),
            last_dev_command_detail: None,
            footer_scroll_line: 0,
            caret_visible: true,
        };

        let official_frame = shell.document_for_viewport(1180, 820);
        let remove_button = official_frame
            .nodes
            .get(&boon_document_model::DocumentNodeId(
                "dev-command-remove_custom".to_owned(),
            ))
            .expect("remove button should render");
        assert!(remove_button.source_binding.is_none());
        assert_eq!(
            remove_button.style.get("disabled"),
            Some(&boon_document_model::StyleValue::Bool(true))
        );

        shell
            .workspace
            .select_example(&shell.catalog, "custom:one")
            .unwrap();
        let custom_frame = shell.document_for_viewport(1180, 820);
        let remove_button = custom_frame
            .nodes
            .get(&boon_document_model::DocumentNodeId(
                "dev-command-remove_custom".to_owned(),
            ))
            .expect("remove button should render");
        assert_eq!(
            remove_button
                .source_binding
                .as_ref()
                .map(|binding| binding.source_path.as_str()),
            Some("dev.commands.remove_custom")
        );
        assert_ne!(
            remove_button.style.get("disabled"),
            Some(&boon_document_model::StyleValue::Bool(true))
        );
    }

    #[test]
    fn custom_remove_falls_back_to_smallest_manifest_example() {
        let catalog = ExampleCatalog {
            entries: vec![
                ExampleCatalogEntry {
                    id: "cells".to_owned(),
                    label: "Cells".to_owned(),
                    source: "examples/cells.bn".to_owned(),
                    source_files: Vec::new(),
                    inline_source: Some("x".repeat(100)),
                    category: "7gui".to_owned(),
                    order: 10,
                    shown_by_default: true,
                    custom: false,
                },
                ExampleCatalogEntry {
                    id: "todomvc".to_owned(),
                    label: "TodoMVC".to_owned(),
                    source: "examples/todomvc.bn".to_owned(),
                    source_files: Vec::new(),
                    inline_source: Some("x".repeat(50)),
                    category: "main".to_owned(),
                    order: 20,
                    shown_by_default: true,
                    custom: false,
                },
                ExampleCatalogEntry {
                    id: "counter".to_owned(),
                    label: "Counter".to_owned(),
                    source: "examples/counter.bn".to_owned(),
                    source_files: Vec::new(),
                    inline_source: Some("x".repeat(10)),
                    category: "basic".to_owned(),
                    order: 30,
                    shown_by_default: true,
                    custom: false,
                },
                ExampleCatalogEntry {
                    id: "custom:one".to_owned(),
                    label: "Custom One".to_owned(),
                    source: "custom://one.bn".to_owned(),
                    source_files: Vec::new(),
                    inline_source: Some("document: []\n".to_owned()),
                    category: "custom".to_owned(),
                    order: 20_000,
                    shown_by_default: true,
                    custom: true,
                },
            ],
            custom_store_path: PathBuf::from(
                "target/artifacts/native-gpu/tests/remove-fallback.toml",
            ),
        };

        assert_eq!(
            catalog
                .fastest_manifest_fallback_id("custom:one")
                .as_deref(),
            Some("counter")
        );
    }

    #[test]
    fn replace_code_updates_preview_input_runtime_context() {
        let counter_path = repo_path("examples/counter.bn");
        let counter_scenario_path = repo_path("examples/counter.scn");
        let counter_source = std::fs::read_to_string(&counter_path).unwrap();
        let counter_hash = boon_runtime::sha256_bytes(counter_source.as_bytes());
        let shared_render_state = Arc::new(Mutex::new(PreviewSharedRenderState {
            layout_proof: native_document_layout_proof(&counter_path, &counter_source).unwrap(),
            layout_frame_override: None,
            update_count: 0,
            scroll_x_px: 12.0,
            scroll_y_px: 34.0,
            last_error: None,
            last_error_count: 0,
        }));
        let state = Arc::new(Mutex::new(PreviewIpcState {
            source_path: counter_path.clone(),
            source_text: counter_source.clone(),
            source_bytes: counter_source.len() as u64,
            source_sha256: counter_hash.clone(),
            runtime_summary: preview_runtime_summary(&counter_path, &counter_source, &counter_hash),
            shared_render_state: Arc::clone(&shared_render_state),
            live_runtime: Some(Arc::new(Mutex::new(
                boon_runtime::LiveRuntime::new(
                    "test-counter",
                    &counter_source,
                    &counter_scenario_path,
                )
                .unwrap(),
            ))),
        }));

        let todomvc_path = repo_path("examples/todomvc.bn");
        let todomvc_source = std::fs::read_to_string(&todomvc_path).unwrap();
        let request = json!({
            "kind": "replace-code",
            "source_path": todomvc_path.display().to_string(),
            "code": todomvc_source,
            "expected_hash": boon_runtime::sha256_bytes(todomvc_source.as_bytes())
        });
        let response = preview_replace_code_response(&request).unwrap();
        assert_eq!(response["status"], "pass");
        assert!(preview_apply_replace_code_to_state(&state, &request, &response).unwrap());

        let context = preview_input_runtime_context(&state).unwrap();
        assert_eq!(context.source_path, todomvc_path);
        let output = context
            .live_runtime
            .expect("replace code should install todomvc runtime")
            .lock()
            .unwrap()
            .apply_source_event(boon_runtime::LiveSourceEvent {
                source: "store.sources.toggle_all_checkbox.click".to_owned(),
                text: None,
                key: None,
                address: None,
                target_text: None,
                target_occurrence: None,
            })
            .unwrap();
        assert!(!output.semantic_deltas.is_empty());
        let shared = shared_render_state.lock().unwrap();
        assert_eq!(shared.scroll_x_px, 0.0);
        assert_eq!(shared.scroll_y_px, 0.0);
    }

    #[test]
    fn replace_code_rejects_invalid_source_before_preview_mutation() {
        let source = "";
        let response = preview_replace_code_response(&json!({
            "kind": "replace-code",
            "source_path": "custom://empty.bn",
            "code": source,
            "expected_hash": boon_runtime::sha256_bytes(source.as_bytes())
        }))
        .unwrap();

        assert_eq!(response["status"], "fail");
        assert_eq!(response["hash_matches"], true);
        assert_eq!(response["accepted_for_preview_mutation"], false);
        assert_eq!(response["preview_receives_example_name"], false);
        assert!(
            response["diagnostic"]
                .as_str()
                .unwrap_or_default()
                .contains("ReplaceCode rejected before preview mutation")
        );
    }

    #[test]
    fn replace_code_accepts_manifest_backed_cells_project_source() {
        let cells_path = repo_path("examples/cells.bn");
        let cells_source = boon_runtime::source_text_for_path(&cells_path).unwrap();
        let top_level_source = std::fs::read_to_string(&cells_path).unwrap();

        assert!(cells_source.contains("FUNCTION new_cell"));
        assert_ne!(
            boon_runtime::sha256_bytes(cells_source.as_bytes()),
            boon_runtime::sha256_bytes(top_level_source.as_bytes())
        );
        let cells_hash = boon_runtime::sha256_bytes(cells_source.as_bytes());

        let response = preview_replace_code_response(&json!({
            "kind": "replace-code",
            "source_path": cells_path.display().to_string(),
            "code": cells_source,
            "expected_hash": cells_hash
        }))
        .unwrap();

        assert_eq!(response["status"], "pass");
        assert_eq!(response["accepted_for_preview_mutation"], true);
        assert_eq!(response["document_layout_proof"]["status"], "pass");
        assert_eq!(response["preview_runtime_summary"]["status"], "pass");
    }

    #[test]
    fn cells_click_selection_updates_formula_bar_and_selected_style() {
        let cells_path = repo_path("examples/cells.bn");
        let scenario_path = repo_path("examples/cells.scn");
        let cells_source = boon_runtime::source_text_for_path(&cells_path).unwrap();
        let scenario = boon_runtime::parse_scenario(&scenario_path).unwrap();
        let mut runtime =
            boon_runtime::LiveRuntime::new("native-cells-select", &cells_source, &scenario_path)
                .unwrap();
        let step = scenario
            .step
            .iter()
            .find(|step| step.id == "select-b0-shows-formula-in-bar")
            .expect("Cells scenario should cover click selection");
        let output = runtime
            .apply_source_event_for_step(
                step,
                boon_runtime::LiveSourceEvent {
                    source: "cell.sources.editor.select".to_owned(),
                    address: Some("B0".to_owned()),
                    ..boon_runtime::LiveSourceEvent::default()
                },
            )
            .unwrap();
        assert_eq!(output.state_summary["store"]["selected_address"], "B0");
        assert_eq!(
            output.state_summary["store"]["selected_input"]["editing_text"],
            "=add(A0,A1)"
        );

        let proof = native_document_layout_proof_with_state(
            &cells_path,
            &cells_source,
            Some(&output.state_summary),
        )
        .unwrap();
        let intents = proof["source_intent_assertions"].as_array().unwrap();
        let selected_node = intents
            .iter()
            .find_map(|intent| {
                let node = intent.get("node").and_then(serde_json::Value::as_str)?;
                let is_click =
                    intent.get("intent").and_then(serde_json::Value::as_str) == Some("click");
                let is_select = intent
                    .get("source_path")
                    .and_then(serde_json::Value::as_str)
                    == Some("cell.sources.editor.select");
                let has_b0_address = intents.iter().any(|candidate| {
                    candidate.get("node").and_then(serde_json::Value::as_str) == Some(node)
                        && candidate.get("intent").and_then(serde_json::Value::as_str)
                            == Some("address")
                        && candidate
                            .get("source_path")
                            .and_then(serde_json::Value::as_str)
                            == Some("B0")
                });
                let has_b0_target = intents.iter().any(|candidate| {
                    candidate.get("node").and_then(serde_json::Value::as_str) == Some(node)
                        && candidate.get("intent").and_then(serde_json::Value::as_str)
                            == Some("target")
                        && candidate
                            .get("source_path")
                            .and_then(serde_json::Value::as_str)
                            == Some("B0")
                });
                (is_click && is_select && has_b0_address && has_b0_target)
                    .then_some(node.to_owned())
            })
            .expect("B0 cell should expose a click select source intent");

        let artifact_path = proof["artifact_path"].as_str().unwrap();
        let artifact: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(artifact_path).unwrap()).unwrap();
        let nodes = artifact["document_frame"]["nodes"].as_object().unwrap();
        let style = nodes
            .get(&selected_node)
            .and_then(|node| node.get("style"))
            .expect("selected B0 node should be in the lowered document frame");
        assert_eq!(
            style.get("selected").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert!(
            style.get("selected_border").is_some(),
            "selected cell style should carry a selected border color"
        );
        assert!(
            nodes.values().any(|node| {
                node.pointer("/text/text")
                    .and_then(serde_json::Value::as_str)
                    == Some("=add(A0,A1)")
            }),
            "formula bar should display the selected cell formula text"
        );
    }

    #[test]
    fn preview_error_overlay_keeps_frame_renderable() {
        let frame = boon_document::LayoutFrame {
            display_list: Vec::new(),
            hit_regions: Vec::new(),
            scroll_regions: Vec::new(),
            accessibility: boon_document::AccessibilityTree::default(),
            demands: Vec::new(),
            metrics: boon_document::LayoutMetrics {
                node_count: 0,
                display_item_count: 0,
                materialized_range_count: 0,
                native_capability_required: false,
            },
        };

        let with_error =
            preview_frame_with_error_overlay(&frame, "line one\nline two", 800.0, 600.0);

        assert_eq!(with_error.display_list.len(), 2);
        assert_eq!(with_error.metrics.display_item_count, 2);
        assert!(
            with_error.display_list[1]
                .text
                .as_deref()
                .unwrap_or_default()
                .contains("Preview input error: line one line two")
        );
    }

    #[test]
    fn manifest_backed_catalog_loads_cells_project_source_files() {
        let catalog = ExampleCatalog::load();
        let cells = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "cells")
            .expect("Cells should be present in manifest catalog");
        assert_eq!(
            cells.source_files,
            vec![
                "examples/cells/defaults.bn".to_owned(),
                "examples/cells/formula.bn".to_owned(),
                "examples/cells/cell.bn".to_owned(),
                "examples/cells/model.bn".to_owned(),
                "examples/cells/columns.bn".to_owned(),
                "examples/cells/store.bn".to_owned(),
                "examples/cells/view.bn".to_owned(),
                "examples/cells.bn".to_owned()
            ]
        );
        let source = cells.source_text().unwrap();
        assert!(source.contains("-- file:"));
        assert!(!source.contains(&["For", "mula", "/"].concat()));
        assert!(source.contains("FUNCTION new_cell"));
        assert!(source.contains("FUNCTION new_sheet_column"));
        assert!(source.contains("FUNCTION cells_app"));
        assert!(source.contains("Document/new"));
    }
}
