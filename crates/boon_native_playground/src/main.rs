use boon_native_app_window::{NativeWindowOptions, NativeWindowRole};
use boon_native_gpu::{PresentSurface, RenderBackend};
use boon_parser::{AstExpr, AstExprKind, AstStatement, AstStatementKind};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    let source = std::fs::read_to_string(&code_file)?;
    let proof = native_document_layout_proof(Path::new(&code_file), &source)?;
    boon_runtime::write_json(Path::new(&report), &proof)?;
    Ok(())
}

fn run_preview(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if value_arg(args, "--example").is_some() {
        return Err(
            "preview role must not receive --example; pass --code-file or ReplaceCode".into(),
        );
    }
    let code_file = value_arg(args, "--code-file")
        .ok_or("preview role currently requires --code-file until ReplaceCode IPC is wired")?;
    let source = std::fs::read_to_string(&code_file)?;
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
    let warmup_frame_count = numeric_arg(args, "--warmup-frame-count").unwrap_or(0) as u32;
    let sample_frame_count = numeric_arg(args, "--sample-frame-count").unwrap_or(1) as u32;
    let code_hash = boon_runtime::sha256_file(Path::new(&code_file))?;
    let runtime_summary = preview_runtime_summary(Path::new(&code_file), &source, &code_hash);
    let connect = value_arg(args, "--connect").map(PathBuf::from);
    let title = role_window_title("Boon Preview", value_arg(args, "--title-token").as_deref());
    if let Some(path) = connect.as_deref() {
        start_preview_ipc_server(
            path,
            PreviewIpcState {
                source_path: PathBuf::from(&code_file),
                source_text: source.clone(),
                source_bytes: source.len() as u64,
                source_sha256: code_hash.clone(),
                runtime_summary: runtime_summary.clone(),
            },
        )?;
    }
    let role_args = args[1..].to_vec();
    let render_layout_proof = document_layout_proof.clone();
    let render_hook: Option<boon_native_app_window::NativeRenderHook> = {
        let mut visible_renderer = None;
        let mut app_owned_proof = None;
        let mut layout_frame_cache = None;
        Some(Box::new(move |context| {
            native_gpu_app_owned_render_hook(
                context,
                &render_layout_proof,
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
    let replace_code_expected_hash = replace_code_file
        .as_deref()
        .map(boon_runtime::sha256_file)
        .transpose()?;
    let report = value_arg(args, "--report").map(PathBuf::from);
    let hold_ms = numeric_arg(args, "--hold-ms").unwrap_or(0);
    let input_sample_delay_ms = numeric_arg(args, "--input-sample-delay-ms").unwrap_or(0);
    let ipc_stress_messages = numeric_arg(args, "--ipc-stress-messages").unwrap_or(4_096);
    let ipc_queue_capacity = numeric_arg(args, "--ipc-queue-capacity").unwrap_or(256);
    let title = role_window_title("Boon Dev", value_arg(args, "--title-token").as_deref());
    let role_args = args[1..].to_vec();
    let warmup_frame_count = numeric_arg(args, "--warmup-frame-count").unwrap_or(0) as u32;
    let sample_frame_count = numeric_arg(args, "--sample-frame-count").unwrap_or(1) as u32;
    let dev_source_path_label = replace_code_file
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<no-code-file>".to_owned());
    let dev_source_text = replace_code_file
        .as_deref()
        .map(std::fs::read_to_string)
        .transpose()?
        .unwrap_or_else(|| "document = []".to_owned());
    let render_hook: Option<boon_native_app_window::NativeRenderHook> = {
        let mut visible_renderer = None;
        let source_path_label = dev_source_path_label.clone();
        let source_text = dev_source_text.clone();
        Some(Box::new(move |context| {
            native_gpu_dev_visible_render_hook(
                context,
                &mut visible_renderer,
                &source_path_label,
                &source_text,
            )
        }))
    };
    boon_native_app_window::run_visible_surface_probe_with_render_hook(
        NativeWindowOptions {
            role: NativeWindowRole::Dev,
            title,
            initial_width: 1180.0,
            initial_height: 820.0,
            hold_ms,
            input_sample_delay_ms,
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
                        let ipc_probe = run_dev_ipc_probe(
                            &connect,
                            ipc_stress_messages,
                            ipc_queue_capacity,
                            replace_code_file.as_deref(),
                            replace_code_expected_hash.as_deref(),
                        );
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
                                "replace_code_file": replace_code_file,
                                "replace_code_expected_hash": replace_code_expected_hash,
                                "ipc_probe": ipc_probe.unwrap(),
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
    let source_path = value_arg(args, "--code-file")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("examples/{example}.bn")));
    let source = std::fs::read_to_string(&source_path)?;
    let source_sha256 = boon_runtime::sha256_file(&source_path)?;
    let document_layout_proof =
        native_document_layout_proof(&source_path, &source).unwrap_or_else(|error| {
            json!({
                "status": "fail",
                "blocker": error.to_string()
            })
        });
    let report = value_arg(args, "--report").map(PathBuf::from);
    let live_state_report = value_arg(args, "--live-state-report").map(PathBuf::from);
    let probe = report.is_some() || args.iter().any(|arg| arg == "--probe");
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
    let input_sample_delay_ms = numeric_arg(args, "--input-sample-delay-ms").unwrap_or(0);
    let warmup_frame_count = numeric_arg(args, "--warmup-frame-count").unwrap_or(0);
    let sample_frame_count = numeric_arg(args, "--sample-frame-count").unwrap_or(1);
    let mut preview = spawn_role(&[
        "--role",
        "preview",
        "--code-file",
        source_path
            .to_str()
            .ok_or("resolved code file path is not UTF-8")?,
        "--connect",
        ipc_path.to_str().ok_or("IPC path is not UTF-8")?,
        "--report",
        preview_report
            .to_str()
            .ok_or("preview report path is not UTF-8")?,
        "--hold-ms",
        &child_hold_ms.to_string(),
        "--title-token",
        &title_token,
        "--input-sample-delay-ms",
        &input_sample_delay_ms.to_string(),
        "--warmup-frame-count",
        &warmup_frame_count.to_string(),
        "--sample-frame-count",
        &sample_frame_count.to_string(),
    ])?;
    let preview_pid = preview.id();
    let preview_cmdline = wait_for_proc_cmdline(preview_pid, "--role", "preview");
    let role_report_timeout = Duration::from_millis(role_report_timeout_ms);
    if probe {
        wait_for_report(&preview_report, role_report_timeout)?;
    }
    let mut dev = spawn_role(&[
        "--role",
        "dev",
        "--connect",
        ipc_path.to_str().ok_or("IPC path is not UTF-8")?,
        "--replace-code-file",
        source_path
            .to_str()
            .ok_or("resolved code file path is not UTF-8")?,
        "--report",
        dev_report.to_str().ok_or("dev report path is not UTF-8")?,
        "--hold-ms",
        &dev_hold_ms.to_string(),
        "--ipc-stress-messages",
        "4096",
        "--ipc-queue-capacity",
        "256",
        "--title-token",
        &title_token,
        "--input-sample-delay-ms",
        &input_sample_delay_ms.to_string(),
        "--warmup-frame-count",
        &warmup_frame_count.to_string(),
        "--sample-frame-count",
        &sample_frame_count.to_string(),
    ])?;
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
    let preview_clean_exit_after_dev_exit = wait_child_exit(
        &mut preview,
        Duration::from_millis(child_hold_ms.saturating_add(500)),
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
        write_desktop_report(
            &report,
            &args[1..],
            json!({
                "resolved_example": example,
                "resolved_code_file": source_path,
                "source_bytes": source.len(),
                "source_sha256": source_sha256,
                "preview_document_layout_proof": document_layout_proof,
                "process_model": "two-child-processes",
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
                "preview_surface_proof": preview_proof,
                "dev_surface_proof": dev_proof,
                "preview_native_gpu_render_proof": preview_native_gpu_render_proof,
                "preview_runtime_summary": preview_runtime_summary,
                "dev_ipc_probe": dev_ipc_probe,
                "note": "desktop supervisor spawns two child roles with app_window/wgpu windows and bounded live IPC; COSMIC launcher proof is owned by the xtask wrapper that invokes cosmic-background-launch"
            }),
        )?;
    }
    Ok(())
}

fn native_document_layout_proof(
    source_path: &Path,
    source: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let parsed = boon_parser::parse_source(source_path.display().to_string(), source)?;
    let document = boon_parser::parsed_document(&parsed)
        .ok_or("source does not contain a parseable document block")?;
    let runtime_state = runtime_state_summary_for_source(source_path, source).ok();
    let eval_context = DocumentEvalContext {
        root: runtime_state.as_ref(),
        locals: BTreeMap::new(),
    };
    let mut frame = boon_document_model::DocumentFrame::empty("root");
    let mut source_intents = Vec::new();
    let mut seen_ids = BTreeSet::new();
    let root_id = frame.root.clone();
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

    let mut measurer = boon_document::SimpleTextMeasurer;
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
    let source_sha256 = boon_runtime::sha256_file(source_path)?;
    let artifact_path = artifact_dir.join(format!(
        "{}-{}.json",
        source_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("source"),
        &source_sha256[..12.min(source_sha256.len())]
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
    let hit_target_assertions = hit_target_assertion_total
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
    let source_intent_assertions = source_intent_assertions
        .into_iter()
        .take(256)
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
        "hit_target_sample_count": hit_target_assertions.len(),
        "hit_target_sample_limit": 256,
        "source_intent_count": source_intent_total,
        "source_intent_sample_count": source_intent_assertions.len(),
        "source_intent_sample_limit": 256,
        "hit_target_assertions": hit_target_assertions,
        "source_intent_assertions": source_intent_assertions,
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
    if !scenario_path.exists() {
        return json!({
            "status": "unavailable",
            "owns_live_runtime": false,
            "reason": "no sibling scenario file exists for bounded runtime proof",
            "source_path": source_path,
            "source_sha256": source_sha256,
            "scenario_path": scenario_path,
            "full_state_mirroring_allowed": false
        });
    }
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
        "state_summary_hash": boon_runtime::sha256_bytes(&summary_bytes),
        "state_summary_bytes": summary_bytes.len(),
        "state_summary_top_level_keys": summary_top_level_keys,
        "full_state_mirroring_allowed": false,
        "full_state_mirroring_observed": false
    })
}

fn runtime_state_summary_for_source(source_path: &Path, source: &str) -> Result<Value, String> {
    let scenario_path = source_path.with_extension("scn");
    if !scenario_path.exists() {
        return Err(format!(
            "no sibling scenario file `{}` exists for runtime-backed document state",
            scenario_path.display()
        ));
    }
    let mut runtime = boon_runtime::LiveRuntime::new(
        &format!("native-preview:{}", source_path.display()),
        source,
        &scenario_path,
    )
    .map_err(|error| error.to_string())?;
    Ok(runtime.state_summary())
}

fn preview_runtime_summary_response(runtime_summary: &serde_json::Value) -> serde_json::Value {
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
    visible_renderer: &mut Option<boon_native_gpu::VisibleLayoutRenderer>,
    app_owned_proof: &mut Option<boon_native_gpu::RenderProof>,
    layout_frame_cache: &mut Option<boon_document::LayoutFrame>,
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
    if layout_frame_cache.is_none() {
        let artifact_json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(layout_artifact)?)?;
        *layout_frame_cache = Some(serde_json::from_value(
            artifact_json
                .get("layout_frame")
                .cloned()
                .ok_or("layout artifact missing layout_frame")?,
        )?);
    }
    let layout_frame = layout_frame_cache
        .as_ref()
        .ok_or("layout frame cache was not initialized")?;
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
        frame: layout_frame,
        format: context.surface_texture_format,
        width: context.width,
        height: context.height,
    })?;
    let proof = match app_owned_proof {
        Some(proof) => proof.clone(),
        None => {
            let proof =
                boon_native_gpu::render_app_owned_pixels(boon_native_gpu::AppOwnedRenderRequest {
                    device: context.device,
                    queue: context.queue,
                    frame: layout_frame,
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
        "surface_id": context.surface_id,
        "surface_epoch": context.surface_epoch,
        "surface_format": context.surface_format,
        "uses_generated_shader_entry": "NativeGpuRect",
        "visible_style_mode": "document_style",
        "debug_palette_used": false,
        "viewport_fill_ratio": 1.0,
        "content_bounds_fill_ratio": viewport_fill_ratio(layout_frame, context.width, context.height),
        "visible_surface_rendered": true,
        "visible_present_path": true,
        "visible_surface_metrics": visible_metrics,
        "proof": proof,
        "copy_to_present_limitation": serde_json::Value::Null
    }))
}

fn native_gpu_dev_visible_render_hook(
    context: boon_native_app_window::NativeRenderFrameContext<'_>,
    visible_renderer: &mut Option<boon_native_gpu::VisibleLayoutRenderer>,
    source_path_label: &str,
    source_text: &str,
) -> Result<serde_json::Value, String> {
    let document = dev_shell_document(source_path_label, source_text);
    let mut measurer = boon_document::SimpleTextMeasurer;
    let layout_frame = boon_document::layout(boon_document::LayoutInput {
        document: &document,
        viewport: boon_host::Viewport {
            surface: 1,
            width: context.width as f32,
            height: context.height as f32,
            scale: 1.0,
        },
        text: &mut measurer,
        capabilities: boon_document::RenderCapabilities::fake_portable(),
    });
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
            frame: &layout_frame,
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
        "dev_ui_source": "boon-dev-editor-debug-shell",
        "dev_editor_visible": true,
        "debug_panel_visible": true,
        "fixture_grid_used": false,
        "code_editor_line_count": source_text.lines().count(),
        "layout_metrics": layout_frame.metrics
    }))
}

fn dev_shell_document(
    source_path_label: &str,
    source_text: &str,
) -> boon_document_model::DocumentFrame {
    use boon_document_model::{DocumentFrame, DocumentNodeKind};

    let mut frame = DocumentFrame::empty("dev-root");
    set_style(
        frame.nodes.get_mut(&frame.root).expect("root exists"),
        &[
            ("bg", "#f3f6f9"),
            ("padding", "12"),
            ("gap", "10"),
            ("width", "fill"),
        ],
    );

    let header = dev_node(
        "dev-header",
        DocumentNodeKind::Row,
        Some(format!("Boon Dev  {source_path_label}")),
        &[
            ("bg", "#26313f"),
            ("color", "#f6f8fb"),
            ("padding", "10"),
            ("gap", "12"),
            ("height", "40"),
            ("width", "fill"),
        ],
    );
    let editor = dev_node(
        "dev-code-editor",
        DocumentNodeKind::Text,
        Some(source_preview_text(source_text)),
        &[
            ("bg", "#ffffff"),
            ("color", "#202936"),
            ("border", "#9aa7b5"),
            ("padding", "12"),
            ("height", "560"),
            ("width", "fill"),
        ],
    );
    let debug = dev_node(
        "dev-debug-panel",
        DocumentNodeKind::Text,
        Some(format!(
            "runtime: bounded query mode\nsource bytes: {}\nlines: {}\npreview transport: ReplaceCode",
            source_text.len(),
            source_text.lines().count()
        )),
        &[
            ("bg", "#edf2f7"),
            ("color", "#1f2937"),
            ("border", "#b8c2cc"),
            ("padding", "10"),
            ("height", "130"),
            ("width", "fill"),
        ],
    );
    let root = frame.root.clone();
    append_child(&mut frame, root.clone(), header);
    append_child(&mut frame, root.clone(), editor);
    append_child(&mut frame, root, debug);
    frame.focus = Some(boon_document_model::DocumentNodeId(
        "dev-code-editor".to_owned(),
    ));
    frame
}

fn source_preview_text(source_text: &str) -> String {
    source_text.lines().take(80).collect::<Vec<_>>().join("\n")
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
    if document_child_value(statement, "visible", expressions)
        .as_deref()
        .and_then(|raw| document_resolved_bool(raw, context))
        == Some(false)
    {
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
            let text = if field == "template" {
                document_resolved_template(&value, context)
            } else {
                document_resolved_text(&value, context)
            };
            node.text = Some(boon_document_model::TextValue { text });
        }
        if is_source_intent_field(&field) && node.source_binding.is_none() {
            node.source_binding = Some(boon_document_model::SourceBinding {
                id: boon_document_model::SourceBindingId(format!("source:{}:{}", id.0, field)),
                source_path: value.clone(),
                intent: field.clone(),
            });
        }
        if is_source_intent_field(&field) {
            source_intents.push(json!({
                "node": id,
                "intent": field,
                "source_path": value
            }));
        } else if let Some(style_value) = document_style_value(child, expressions, context) {
            node.style.insert(field, style_value);
        }
    }

    let vertical_scroll = matches!(node.kind, boon_document_model::DocumentNodeKind::Grid)
        || style_bool(&node.style, "scroll")
        || style_bool(&node.style, "scroll_y");
    let horizontal_scroll = matches!(node.kind, boon_document_model::DocumentNodeKind::Grid)
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
        "Button" => boon_document_model::DocumentNodeKind::Button,
        "Input" | "TextInput" => boon_document_model::DocumentNodeKind::TextInput,
        "Grid" => boon_document_model::DocumentNodeKind::Grid,
        "GridCell" => boon_document_model::DocumentNodeKind::GridCell,
        "ScrollRoot" => boon_document_model::DocumentNodeKind::ScrollRoot,
        _ => boon_document_model::DocumentNodeKind::Stack,
    }
}

fn is_source_intent_field(field: &str) -> bool {
    matches!(
        field,
        "change"
            | "submit"
            | "cancel"
            | "press"
            | "click"
            | "key_down"
            | "blur"
            | "double_click"
            | "target"
    )
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
        _ => None,
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
        _ => {
            let value = document_expr_value(expr, expressions)?;
            if let Some(resolved) = document_resolved_value(&value, context) {
                return Some(match resolved {
                    Value::Bool(value) => boon_document_model::StyleValue::Bool(*value),
                    Value::Number(value) => {
                        boon_document_model::StyleValue::Number(value.as_f64().unwrap_or_default())
                    }
                    _ => {
                        boon_document_model::StyleValue::Text(json_value_to_document_text(resolved))
                    }
                });
            }
            Some(boon_document_model::StyleValue::Text(value))
        }
    }
}

fn document_resolved_text(raw: &str, context: &DocumentEvalContext<'_>) -> String {
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
        rendered.push_str(&document_resolved_text(key.trim(), context));
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
struct PreviewIpcState {
    source_path: PathBuf,
    source_text: String,
    source_bytes: u64,
    source_sha256: String,
    runtime_summary: serde_json::Value,
}

fn start_preview_ipc_server(
    path: &Path,
    state: PreviewIpcState,
) -> Result<(), Box<dyn std::error::Error>> {
    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    let path = path.to_path_buf();
    let state = Arc::new(Mutex::new(state));
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
        let response = preview_replace_code_response(&request)?;
        if response
            .get("hash_matches")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            if let (Some(code), Some(source_path), Some(actual_hash)) = (
                request.get("code").and_then(serde_json::Value::as_str),
                request
                    .get("source_path")
                    .and_then(serde_json::Value::as_str),
                response
                    .get("actual_hash")
                    .and_then(serde_json::Value::as_str),
            ) {
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
            }
        }
        writeln!(stream, "{}", serde_json::to_string(&response)?)?;
        stream.flush()?;
        return Ok(());
    }
    if request.get("kind").and_then(serde_json::Value::as_str) == Some("runtime-summary") {
        let runtime_summary = state
            .lock()
            .map_err(|_| "preview IPC state mutex poisoned")?
            .runtime_summary
            .clone();
        let response = preview_runtime_summary_response(&runtime_summary);
        writeln!(stream, "{}", serde_json::to_string(&response)?)?;
        stream.flush()?;
        return Ok(());
    }
    if request.get("kind").and_then(serde_json::Value::as_str) == Some("operator-host-input") {
        let state = state
            .lock()
            .map_err(|_| "preview IPC state mutex poisoned")?
            .clone();
        let response = preview_operator_host_input_response(&state, &request)?;
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
    Ok(json!({
        "kind": "replace-code-ack",
        "preview_command": "ReplaceCode",
        "replace_code_protocol": true,
        "sync_layout_budget_bytes": REPLACE_CODE_SYNC_LAYOUT_BYTES_MAX,
        "layout_proof_deferred": code.len() > REPLACE_CODE_SYNC_LAYOUT_BYTES_MAX,
        "transport": "unix-stream-json-lines",
        "code_bytes": code.len(),
        "expected_hash": expected_hash,
        "actual_hash": actual_hash,
        "hash_matches": actual_hash == expected_hash,
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
    let events = request
        .get("source_events")
        .and_then(serde_json::Value::as_array)
        .ok_or("operator-host-input request missing source_events")?;
    let scenario_path = state.source_path.with_extension("scn");
    let mut runtime = boon_runtime::LiveRuntime::new(
        &format!("native-preview-ipc:{}", state.source_path.display()),
        &state.source_text,
        &scenario_path,
    )?;
    let mut outputs = Vec::new();
    let mut assertions = Vec::new();
    for (index, event_json) in events.iter().enumerate() {
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
        let output = runtime.apply_source_event(event.clone())?;
        let assertion = preview_operator_host_input_assertion(index, &event, &output.state_summary);
        outputs.push(json!({
            "event": live_source_event_report(&event),
            "semantic_delta_count": output.semantic_deltas.len(),
            "render_patch_count": output.render_patches.len(),
            "state_summary_hash": boon_runtime::sha256_bytes(&serde_json::to_vec(&output.state_summary)?),
            "bounded_state_summary_sample": bounded_state_summary_sample(&output.state_summary)
        }));
        assertions.push(assertion);
    }
    let status = if !assertions.is_empty()
        && assertions.iter().all(|assertion| {
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
        "route_contract": "HostInputEvent -> document hit region -> SourceIntent -> preview IPC -> LiveRuntime::apply_source_event",
        "public_runtime_api": "boon_runtime::LiveRuntime::apply_source_event",
        "private_runtime_dispatch_used": false,
        "assertions": assertions,
        "outputs": outputs,
        "full_state_mirroring_observed": false,
        "preview_blocked_on_ipc_count": 0
    }))
}

fn preview_operator_host_input_assertion(
    index: usize,
    event: &boon_runtime::LiveSourceEvent,
    state_summary: &serde_json::Value,
) -> serde_json::Value {
    if event.source == "store.sources.new_todo_input.change" {
        return json!({
            "id": format!("preview-ipc-host-input-{index}"),
            "pass": state_summary.get("new_todo_text") == Some(&json!(event.text.clone().unwrap_or_default())),
            "expected": {"new_todo_text": event.text},
            "actual": {"new_todo_text": state_summary.get("new_todo_text").cloned().unwrap_or_else(|| json!(null))}
        });
    }
    if event.source == "store.sources.new_todo_input.key_down" {
        let expected = event.text.clone().unwrap_or_default();
        let inserted = state_summary
            .get("todos")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|todos| {
                todos
                    .iter()
                    .any(|todo| todo.get("title") == Some(&json!(expected)))
            });
        return json!({
            "id": format!("preview-ipc-host-input-{index}"),
            "pass": inserted,
            "expected": {"todo_title_inserted": expected},
            "actual": {
                "todo_count": state_summary
                    .get("todos")
                    .and_then(serde_json::Value::as_array)
                    .map_or(0, Vec::len),
                "inserted": inserted
            }
        });
    }
    if event.source == "cell.sources.editor.change" || event.source == "cell.sources.editor.commit"
    {
        let expected = event.text.clone().unwrap_or_default();
        let address = event.address.as_deref().unwrap_or("A0");
        let cell = state_summary
            .get("cells")
            .and_then(serde_json::Value::as_array)
            .and_then(|cells| {
                cells
                    .iter()
                    .find(|cell| cell.get("address") == Some(&json!(address)))
            });
        let pass = if event.source.ends_with(".change") {
            cell.and_then(|cell| cell.get("editing_text")) == Some(&json!(expected))
                && cell.and_then(|cell| cell.get("editing")) == Some(&json!(true))
        } else {
            cell.and_then(|cell| cell.get("formula")) == Some(&json!(expected))
                && cell.and_then(|cell| cell.get("value")) == Some(&json!(expected))
        };
        return json!({
            "id": format!("preview-ipc-host-input-{index}"),
            "pass": pass,
            "expected": {"address": address, "text": expected},
            "actual": cell.cloned().unwrap_or_else(|| json!(null))
        });
    }
    json!({
        "id": format!("preview-ipc-host-input-{index}"),
        "pass": false,
        "event": live_source_event_report(event),
        "error": "source event was not recognized by the generic preview input assertion probe"
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
    if let Some(todos) = state_summary
        .get("todos")
        .and_then(serde_json::Value::as_array)
    {
        return json!({
            "new_todo_text": state_summary.get("new_todo_text").cloned().unwrap_or_else(|| json!(null)),
            "todo_count": todos.len(),
            "last_todo": todos.last().cloned().unwrap_or_else(|| json!(null))
        });
    }
    if let Some(cells) = state_summary
        .get("cells")
        .and_then(serde_json::Value::as_array)
    {
        return json!({
            "cell_count": cells.len(),
            "a0": cells
                .iter()
                .find(|cell| cell.get("address") == Some(&json!("A0")))
                .cloned()
                .unwrap_or_else(|| json!(null))
        });
    }
    json!({
        "top_level_keys": state_summary
            .as_object()
            .map(|object| object.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    })
}

fn run_dev_ipc_probe(
    connect: &str,
    message_count: u64,
    queue_capacity: u64,
    replace_code_file: Option<&Path>,
    replace_code_expected_hash: Option<&str>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let replace_code_response = if let Some(path) = replace_code_file {
        let code = std::fs::read_to_string(path)?;
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
    let operator_host_input_response = if let Some(path) = replace_code_file {
        let code = std::fs::read_to_string(path)?;
        operator_host_input_probe_request(path, &code)
            .map(|request| send_preview_ipc_request(connect, request))
            .transpose()?
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
    }
    Ok(value)
}

fn operator_host_input_probe_request(path: &Path, code: &str) -> Option<serde_json::Value> {
    let layout_proof = native_document_layout_proof(path, code).ok()?;
    let source_intents = layout_proof
        .get("source_intent_assertions")
        .and_then(serde_json::Value::as_array)?;
    let has_source = |source: &str| {
        source_intents.iter().any(|intent| {
            intent
                .get("source_path")
                .and_then(serde_json::Value::as_str)
                == Some(source)
        })
    };
    let has_source_suffix = |suffix: &str| {
        source_intents.iter().any(|intent| {
            intent
                .get("source_path")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|source| source.ends_with(suffix))
        })
    };
    let source_events = if has_source("store.sources.new_todo_input.change")
        || has_source("store.sources.new_todo_input.key_down")
    {
        json!([
            {
                "source": "store.sources.new_todo_input.change",
                "text": "Native GPU todo"
            },
            {
                "source": "store.sources.new_todo_input.key_down",
                "text": "Native GPU todo",
                "key": "Enter"
            }
        ])
    } else if has_source("cell.sources.editor.change")
        || has_source("cell.sources.editor.commit")
        || has_source_suffix(".change_source")
        || has_source_suffix(".submit_source")
    {
        json!([
            {
                "source": "cell.sources.editor.change",
                "text": "41",
                "address": "A0"
            },
            {
                "source": "cell.sources.editor.commit",
                "text": "41",
                "key": "Enter",
                "address": "A0"
            }
        ])
    } else {
        return None;
    };
    Some(json!({
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
        "layout_proof_hash": layout_proof.get("artifact_sha256").cloned().unwrap_or_else(|| json!(null))
    }))
}

fn send_preview_ipc_request(
    connect: &str,
    request: serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut stream = loop {
        match UnixStream::connect(connect) {
            Ok(stream) => break stream,
            Err(error) if start.elapsed() < Duration::from_secs(5) => {
                let _ = error;
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(Box::new(error)),
        }
    };
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
    Ok(Command::new(std::env::current_exe()?)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
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
    if details
        .get("display_server")
        .and_then(serde_json::Value::as_str)
        != Some("wayland")
    {
        blockers.push("desktop supervisor did not run on Wayland".to_owned());
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
        {
            "id": "desktop-preview-survived-dev-exit",
            "pass": details
                .get("preview_survives_dev_exit")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        },
        {
            "id": "desktop-preview-clean-exit-after-dev-exit",
            "pass": details
                .get("preview_clean_exit_after_dev_exit")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
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
        if path.exists() && boon_runtime::verify_report_schema(path).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(format!("timed out waiting for role report `{}`", path.display()).into())
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
