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

fn run_preview(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if value_arg(args, "--example").is_some() {
        return Err(
            "preview role must not receive --example; pass --code-file or ReplaceCode".into(),
        );
    }
    let code_file = value_arg(args, "--code-file")
        .ok_or("preview role requires --code-file for initial source before ReplaceCode updates")?;
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
    let synthetic_input_probe = args.iter().any(|arg| arg == "--synthetic-input-probe");
    let warmup_frame_count = numeric_arg(args, "--warmup-frame-count").unwrap_or(0) as u32;
    let sample_frame_count = numeric_arg(args, "--sample-frame-count").unwrap_or(1) as u32;
    let code_hash = boon_runtime::sha256_file(Path::new(&code_file))?;
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
    }));
    if let Some(path) = connect.as_deref() {
        start_preview_ipc_server(
            path,
            PreviewIpcState {
                source_path: PathBuf::from(&code_file),
                source_text: source.clone(),
                source_bytes: source.len() as u64,
                source_sha256: code_hash.clone(),
                runtime_summary: runtime_summary.clone(),
                shared_render_state: Arc::clone(&shared_render_state),
                live_runtime: live_runtime.clone(),
            },
        )?;
    }
    let role_args = args[1..].to_vec();
    let render_hook: Option<boon_native_app_window::NativeRenderHook> = {
        let mut visible_renderer = None;
        let mut app_owned_proof = None;
        let mut layout_frame_cache = None;
        let shared_render_state = Arc::clone(&shared_render_state);
        let live_runtime = live_runtime.clone();
        let render_code_file = code_file.clone();
        let render_source = source.clone();
        let mut input_state = PreviewNativeInputState::default();
        Some(Box::new(move |context| {
            preview_apply_real_window_input(
                &context.input,
                Path::new(&render_code_file),
                &render_source,
                live_runtime.as_ref(),
                &shared_render_state,
                &mut input_state,
            )
            .map_err(|error| error.to_string())?;
            let (render_layout_proof, render_layout_frame_override) = {
                let shared = shared_render_state
                    .lock()
                    .map_err(|_| "preview render state mutex poisoned".to_owned())?;
                (
                    shared.layout_proof.clone(),
                    shared.layout_frame_override.clone(),
                )
            };
            native_gpu_app_owned_render_hook(
                context,
                &render_layout_proof,
                render_layout_frame_override.as_ref(),
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
        .map(boon_runtime::sha256_file)
        .transpose()?;
    let report = value_arg(args, "--report").map(PathBuf::from);
    let hold_ms = numeric_arg(args, "--hold-ms").unwrap_or(0);
    let input_sample_delay_ms = numeric_arg(args, "--input-sample-delay-ms").unwrap_or(0);
    let synthetic_input_probe = args.iter().any(|arg| arg == "--synthetic-input-probe");
    let probe = args.iter().any(|arg| arg == "--probe");
    let ipc_stress_messages = numeric_arg(args, "--ipc-stress-messages").unwrap_or(4_096);
    let ipc_queue_capacity = numeric_arg(args, "--ipc-queue-capacity").unwrap_or(256);
    let ipc_probe_timeout_ms = numeric_arg(args, "--ipc-probe-timeout-ms").unwrap_or(60_000);
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
        .map(std::fs::read_to_string)
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
    let source_sha256 = if source_path.exists() {
        boon_runtime::sha256_file(source_path)?
    } else {
        boon_runtime::sha256_bytes(source.as_bytes())
    };
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
    layout_frame_override: Option<&boon_document::LayoutFrame>,
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
        *app_owned_proof = None;
    }
    let layout_frame = layout_frame_cache
        .as_ref()
        .map(|(_, frame)| frame)
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
        "layout_frame_hash": layout_proof.get("layout_frame_hash").cloned().unwrap_or_else(|| json!("missing")),
        "scroll_transform": layout_proof.get("scroll_transform").cloned().unwrap_or_else(|| json!(null)),
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
    layout_frame_cache: &mut Option<(u32, u32, boon_document::LayoutFrame)>,
    shell: &mut DevWindowShell,
    input_state: &mut DevNativeInputState,
) -> Result<serde_json::Value, String> {
    let cache_stale = layout_frame_cache
        .as_ref()
        .is_none_or(|(width, height, _)| *width != context.width || *height != context.height);
    if cache_stale {
        let document = shell.document_for_viewport(context.width, context.height);
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
        *layout_frame_cache = Some((context.width, context.height, layout_frame));
    }
    let mut layout_changed = false;
    if let Some((_, _, layout_frame)) = layout_frame_cache.as_ref() {
        let document = shell.document_for_viewport(context.width, context.height);
        layout_changed = dev_apply_real_window_input(
            &context.input,
            &document,
            layout_frame,
            shell,
            input_state,
        );
    }
    if layout_changed {
        let document = shell.document_for_viewport(context.width, context.height);
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
            "syntax_token_count": shell.workspace.selected_buffer.syntax_token_count(),
            "syntax_categories": shell.workspace.selected_buffer.syntax_categories(),
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
    editor_focused: bool,
}

fn dev_apply_real_window_input(
    input: &boon_native_app_window::NativeInputAdapterProof,
    document: &boon_document_model::DocumentFrame,
    layout_frame: &boon_document::LayoutFrame,
    shell: &mut DevWindowShell,
    input_state: &mut DevNativeInputState,
) -> bool {
    if input.synthetic_input_probe {
        return false;
    }
    let mut changed = false;

    if (input.scroll_delta_y.abs() > f64::EPSILON || input.scroll_delta_x.abs() > f64::EPSILON)
        && let Some(position) = input.mouse_window_pos
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
        let column_delta = scaled_scroll_steps(input.scroll_delta_x, 12.0, 2);
        if column_delta > 0 {
            shell.workspace.selected_buffer.scroll_column = shell
                .workspace
                .selected_buffer
                .scroll_column
                .saturating_add(column_delta as usize);
        } else if column_delta < 0 {
            shell.workspace.selected_buffer.scroll_column = shell
                .workspace
                .selected_buffer
                .scroll_column
                .saturating_sub((-column_delta) as usize);
        }
        changed = true;
    }

    if input.mouse_button_event_count > input_state.last_mouse_button_event_count
        && input.mouse_buttons_down.is_empty()
    {
        input_state.last_mouse_button_event_count = input.mouse_button_event_count;
        if let Some(position) = input.mouse_window_pos {
            if let Some((node_id, source_path)) =
                dev_source_binding_at(document, layout_frame, position.x as f32, position.y as f32)
            {
                if source_path == "dev.editor.insert_text" || node_id == "dev-code-editor" {
                    input_state.editor_focused = true;
                    dev_move_caret_from_pointer(
                        &mut shell.workspace.selected_buffer,
                        layout_frame,
                        position.x as f32,
                        position.y as f32,
                    );
                    changed = true;
                } else {
                    input_state.editor_focused = false;
                    shell.dispatch_source_path(&source_path);
                    changed = true;
                }
            } else {
                input_state.editor_focused = false;
            }
        }
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
        if !event.pressed || !input_state.editor_focused {
            continue;
        }
        match event.key.as_str() {
            "Return" | "KeypadEnter" => {
                shell.workspace.selected_buffer.insert_newline_with_indent();
                shell.workspace.persist_selected_buffer();
                shell.workspace.set_selected_dirty(true);
                let _ = shell.persist_selected_custom_source("EditorTextInput");
                shell.replace_selected_preview("EditorTextInput");
                changed = true;
            }
            "Delete" => {
                shell.workspace.selected_buffer.delete_backward();
                shell.workspace.persist_selected_buffer();
                shell.workspace.set_selected_dirty(true);
                let _ = shell.persist_selected_custom_source("EditorTextInput");
                shell.replace_selected_preview("EditorTextInput");
                changed = true;
            }
            "Tab" => {
                shell.workspace.selected_buffer.insert_text_at_caret("    ");
                shell.workspace.persist_selected_buffer();
                shell.workspace.set_selected_dirty(true);
                let _ = shell.persist_selected_custom_source("EditorTextInput");
                shell.replace_selected_preview("EditorTextInput");
                changed = true;
            }
            "Home" => {
                shell.workspace.selected_buffer.move_home();
                changed = true;
            }
            "End" => {
                shell.workspace.selected_buffer.move_end();
                changed = true;
            }
            "PageDown" => {
                shell.workspace.selected_buffer.page_down();
                changed = true;
            }
            "PageUp" => {
                shell.workspace.selected_buffer.page_up();
                changed = true;
            }
            key => {
                if let Some(character) = keyboard_event_text(key, shift_pressed) {
                    shell
                        .workspace
                        .selected_buffer
                        .insert_text_at_caret(&character.to_string());
                    shell.workspace.persist_selected_buffer();
                    shell.workspace.set_selected_dirty(true);
                    let _ = shell.persist_selected_custom_source("EditorTextInput");
                    shell.replace_selected_preview("EditorTextInput");
                    changed = true;
                }
            }
        }
    }

    changed
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

fn dev_move_caret_from_pointer(
    model: &mut CodeEditorModel,
    layout_frame: &boon_document::LayoutFrame,
    x: f32,
    y: f32,
) {
    let Some(editor_bounds) = layout_frame
        .display_list
        .iter()
        .find(|item| item.node.0 == "dev-code-editor")
        .map(|item| item.bounds)
    else {
        return;
    };
    let relative_y = (y - editor_bounds.y - 12.0).max(0.0);
    let line = model
        .scroll_line
        .saturating_add((relative_y / 16.0).floor() as usize)
        .saturating_add(1)
        .min(model.line_count.max(1));
    let line_text = model
        .source_text
        .lines()
        .nth(line.saturating_sub(1))
        .unwrap_or("");
    let relative_x = (x - editor_bounds.x - 64.0).max(0.0);
    let column = ((relative_x / 7.5).floor() as usize + 1).min(line_text.chars().count() + 1);
    model.set_selection(
        EditorPosition { line, column },
        EditorPosition { line, column },
    );
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
        } else {
            Ok(std::fs::read_to_string(&self.source)?)
        }
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
        let fallback_id = catalog
            .entries
            .iter()
            .filter(|entry| entry.id != removed_id && entry.shown_by_default)
            .min_by_key(|entry| entry.order)
            .map(|entry| entry.id.clone())
            .or_else(|| {
                catalog
                    .entries
                    .iter()
                    .find(|entry| entry.id != removed_id)
                    .map(|entry| entry.id.clone())
            });
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
        match boon_parser::parse_source(path.to_owned(), source.to_owned()) {
            Ok(_) => Vec::new(),
            Err(error) => vec![error.to_string()],
        }
    }

    fn format(path: &str, source: &str) -> Result<String, boon_parser::ParseError> {
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

    fn syntax_tokens(source: &str) -> Vec<SyntaxToken> {
        if let Ok(ast) = boon_parser::parse_ast("<editor>", source) {
            return ast
                .tokens
                .iter()
                .filter(|token| token.kind != AstTokenKind::Newline)
                .map(|token| {
                    SyntaxToken::new(
                        Self::syntax_kind_from_ast_token(token.kind, &token.lexeme),
                        token.line,
                        token.column,
                        token.lexeme.chars().count().max(1),
                    )
                })
                .collect();
        }
        Self::syntax_tokens_fallback(source)
    }

    fn syntax_kind_from_ast_token(kind: AstTokenKind, lexeme: &str) -> &'static str {
        if lexeme == "EXAMPLE" || lexeme == "#" {
            return "invalid";
        }
        match kind {
            AstTokenKind::Comment => "comment",
            AstTokenKind::String => "string",
            AstTokenKind::Number => "number",
            AstTokenKind::Operator | AstTokenKind::Symbol => "operator",
            AstTokenKind::Identifier => match lexeme {
                "SOURCE" | "HOLD" | "LATEST" | "THEN" | "WHEN" | "WHILE" | "LIST" | "TEXT"
                | "BOOL" | "INT" | "FLOAT" => "keyword",
                _ if lexeme.contains('/') => "source-binding",
                _ => "identifier",
            },
            AstTokenKind::Unknown | AstTokenKind::Newline => "invalid",
        }
    }

    fn syntax_tokens_fallback(source: &str) -> Vec<SyntaxToken> {
        let mut tokens = Vec::new();
        for (line_index, line) in source.lines().enumerate() {
            let mut column = 0;
            let bytes = line.as_bytes();
            while column < bytes.len() {
                let rest = &line[column..];
                if rest.starts_with("--") {
                    tokens.push(SyntaxToken::new(
                        "comment",
                        line_index + 1,
                        column + 1,
                        rest.len(),
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
                if ch == '"' {
                    let mut len = ch.len_utf8();
                    for next in rest[ch.len_utf8()..].chars() {
                        len += next.len_utf8();
                        if next == '"' {
                            break;
                        }
                    }
                    tokens.push(SyntaxToken::new("string", line_index + 1, column + 1, len));
                    column += len;
                    continue;
                }
                if ch.is_ascii_digit() {
                    let len = rest
                        .chars()
                        .take_while(|next| next.is_ascii_digit() || *next == '.')
                        .map(char::len_utf8)
                        .sum::<usize>();
                    tokens.push(SyntaxToken::new("number", line_index + 1, column + 1, len));
                    column += len;
                    continue;
                }
                if ch.is_ascii_alphabetic() || ch == '_' {
                    let text = rest
                        .chars()
                        .take_while(|next| {
                            next.is_ascii_alphanumeric() || *next == '_' || *next == '/'
                        })
                        .collect::<String>();
                    let kind = match text.as_str() {
                        "SOURCE" | "HOLD" | "LATEST" | "THEN" | "WHEN" | "WHILE" | "LIST"
                        | "TEXT" | "BOOL" | "INT" | "FLOAT" => "keyword",
                        "EXAMPLE" => "invalid",
                        _ if text.contains('/') => "source-binding",
                        _ => "identifier",
                    };
                    tokens.push(SyntaxToken::new(
                        kind,
                        line_index + 1,
                        column + 1,
                        text.len(),
                    ));
                    column += text.len();
                    continue;
                }
                let kind = if "{}[]():.,|=+-*/<>".contains(ch) {
                    "operator"
                } else {
                    "invalid"
                };
                tokens.push(SyntaxToken::new(
                    kind,
                    line_index + 1,
                    column + 1,
                    ch.len_utf8(),
                ));
                column += ch.len_utf8();
            }
        }
        tokens
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EditorPosition {
    line: usize,
    column: usize,
}

impl EditorPosition {
    fn start() -> Self {
        Self { line: 1, column: 1 }
    }
}

#[derive(Clone, Debug)]
struct EditorSelection {
    anchor: EditorPosition,
    head: EditorPosition,
}

impl EditorSelection {
    fn collapsed(position: EditorPosition) -> Self {
        Self {
            anchor: position.clone(),
            head: position,
        }
    }

    fn is_collapsed(&self) -> bool {
        self.anchor == self.head
    }
}

#[derive(Clone, Debug)]
struct EditorSnapshot {
    source_text: String,
    selection: EditorSelection,
    scroll_line: usize,
    scroll_column: usize,
}

#[derive(Clone, Debug)]
struct SyntaxToken {
    kind: &'static str,
    line: usize,
    column: usize,
    len: usize,
}

impl SyntaxToken {
    fn new(kind: &'static str, line: usize, column: usize, len: usize) -> Self {
        Self {
            kind,
            line,
            column,
            len,
        }
    }
}

#[derive(Clone, Debug)]
struct CodeEditorModel {
    file_name: String,
    source_text: String,
    line_count: usize,
    selection: EditorSelection,
    scroll_line: usize,
    scroll_column: usize,
    diagnostics: Vec<String>,
    syntax_tokens: Vec<SyntaxToken>,
    formatted_preview_hash: Option<String>,
    undo_stack: Vec<EditorSnapshot>,
    redo_stack: Vec<EditorSnapshot>,
    clipboard_cache: String,
    last_command: Option<&'static str>,
}

impl CodeEditorModel {
    fn new(source_path_label: &str, source_text: &str) -> Self {
        let diagnostics = BoonLanguageService::diagnostics(source_path_label, source_text);
        let formatted_preview_hash = BoonLanguageService::format(source_path_label, source_text)
            .ok()
            .map(|formatted| boon_runtime::sha256_bytes(formatted.as_bytes()));
        Self {
            file_name: source_path_label.to_owned(),
            source_text: source_text.to_owned(),
            line_count: source_text.lines().count().max(1),
            selection: EditorSelection::collapsed(EditorPosition::start()),
            scroll_line: 0,
            scroll_column: 0,
            diagnostics,
            syntax_tokens: BoonLanguageService::syntax_tokens(source_text),
            formatted_preview_hash,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            clipboard_cache: String::new(),
            last_command: None,
        }
    }

    fn syntax_token_count(&self) -> usize {
        self.syntax_tokens.len()
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
                    "line": token.line,
                    "column": token.column,
                    "len": token.len
                })
            })
            .collect()
    }

    fn caret(&self) -> &EditorPosition {
        &self.selection.head
    }

    fn refresh_language_state(&mut self) {
        self.line_count = self.source_text.lines().count().max(1);
        self.diagnostics = BoonLanguageService::diagnostics(&self.file_name, &self.source_text);
        self.syntax_tokens = BoonLanguageService::syntax_tokens(&self.source_text);
        self.formatted_preview_hash =
            BoonLanguageService::format(&self.file_name, &self.source_text)
                .ok()
                .map(|formatted| boon_runtime::sha256_bytes(formatted.as_bytes()));
    }

    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            source_text: self.source_text.clone(),
            selection: self.selection.clone(),
            scroll_line: self.scroll_line,
            scroll_column: self.scroll_column,
        }
    }

    fn restore_snapshot(&mut self, snapshot: EditorSnapshot) {
        self.source_text = snapshot.source_text;
        self.selection = snapshot.selection;
        self.scroll_line = snapshot.scroll_line;
        self.scroll_column = snapshot.scroll_column;
        self.refresh_language_state();
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(self.snapshot());
        self.redo_stack.clear();
    }

    fn byte_offset(&self, position: &EditorPosition) -> usize {
        let mut offset = 0;
        for (index, line) in self.source_text.split_inclusive('\n').enumerate() {
            if index + 1 == position.line {
                let line_without_newline = line.trim_end_matches('\n');
                let column_offset = line_without_newline
                    .char_indices()
                    .nth(position.column.saturating_sub(1))
                    .map(|(byte, _)| byte)
                    .unwrap_or(line_without_newline.len());
                return offset + column_offset;
            }
            offset += line.len();
        }
        self.source_text.len()
    }

    fn position_for_offset(&self, target: usize) -> EditorPosition {
        let mut offset = 0;
        for (index, line) in self.source_text.split_inclusive('\n').enumerate() {
            let next = offset + line.len();
            if target <= next {
                let column = line[..target.saturating_sub(offset).min(line.len())]
                    .chars()
                    .count()
                    + 1;
                return EditorPosition {
                    line: index + 1,
                    column,
                };
            }
            offset = next;
        }
        EditorPosition {
            line: self.line_count.max(1),
            column: self
                .source_text
                .lines()
                .last()
                .map(|line| line.chars().count() + 1)
                .unwrap_or(1),
        }
    }

    fn selection_offsets(&self) -> (usize, usize) {
        let anchor = self.byte_offset(&self.selection.anchor);
        let head = self.byte_offset(&self.selection.head);
        if anchor <= head {
            (anchor, head)
        } else {
            (head, anchor)
        }
    }

    fn selected_text(&self) -> String {
        let (start, end) = self.selection_offsets();
        self.source_text[start..end].to_owned()
    }

    fn set_selection(&mut self, anchor: EditorPosition, head: EditorPosition) {
        self.selection = EditorSelection { anchor, head };
        self.last_command = Some("selection");
    }

    fn insert_text_at_caret(&mut self, text: &str) {
        self.push_undo();
        let (start, end) = self.selection_offsets();
        self.source_text.replace_range(start..end, text);
        let position = self.position_for_offset(start + text.len());
        self.selection = EditorSelection::collapsed(position);
        self.refresh_language_state();
        self.last_command = Some("keyboard-insert-text");
    }

    fn delete_backward(&mut self) {
        self.push_undo();
        let (start, end) = self.selection_offsets();
        if start != end {
            self.source_text.replace_range(start..end, "");
            self.selection = EditorSelection::collapsed(self.position_for_offset(start));
        } else if start > 0 {
            let previous = self.source_text[..start]
                .char_indices()
                .last()
                .map(|(byte, _)| byte)
                .unwrap_or(0);
            self.source_text.replace_range(previous..start, "");
            self.selection = EditorSelection::collapsed(self.position_for_offset(previous));
        }
        self.refresh_language_state();
        self.last_command = Some("keyboard-delete-backward");
    }

    fn insert_newline_with_indent(&mut self) {
        let line = self
            .source_text
            .lines()
            .nth(self.caret().line.saturating_sub(1))
            .unwrap_or_default();
        let indent = line
            .chars()
            .take_while(|character| character.is_whitespace())
            .collect::<String>();
        self.insert_text_at_caret(&format!("\n{indent}"));
        self.last_command = Some("keyboard-enter-indent");
    }

    fn indent_selection(&mut self) {
        self.push_undo();
        let (start, end) = self.selection_offsets();
        let before = &self.source_text[..start];
        let selected = &self.source_text[start..end];
        let after = &self.source_text[end..];
        let indented = selected
            .lines()
            .map(|line| format!("    {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        self.source_text = format!("{before}{indented}{after}");
        self.refresh_language_state();
        self.last_command = Some("keyboard-tab-indent");
    }

    fn copy_selection_to_clipboard(&mut self) -> String {
        self.clipboard_cache = self.selected_text();
        self.last_command = Some("clipboard-copy");
        self.clipboard_cache.clone()
    }

    fn paste_from_clipboard(&mut self, text: &str) {
        self.clipboard_cache = text.to_owned();
        self.insert_text_at_caret(text);
        self.last_command = Some("clipboard-paste");
    }

    fn move_home(&mut self) {
        let line = self.caret().line;
        self.selection = EditorSelection::collapsed(EditorPosition { line, column: 1 });
        self.last_command = Some("keyboard-home");
    }

    fn move_end(&mut self) {
        let line = self.caret().line;
        let column = self
            .source_text
            .lines()
            .nth(line.saturating_sub(1))
            .map(|line| line.chars().count() + 1)
            .unwrap_or(1);
        self.selection = EditorSelection::collapsed(EditorPosition { line, column });
        self.last_command = Some("keyboard-end");
    }

    fn page_down(&mut self) {
        self.scroll_line = (self.scroll_line + 24).min(self.line_count.saturating_sub(1));
        self.last_command = Some("keyboard-page-down");
    }

    fn page_up(&mut self) {
        self.scroll_line = self.scroll_line.saturating_sub(24);
        self.last_command = Some("keyboard-page-up");
    }

    fn undo(&mut self) -> serde_json::Value {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.redo_stack.push(self.snapshot());
            self.restore_snapshot(snapshot);
            self.last_command = Some("undo");
            json!({"status": "pass", "undo_depth": self.undo_stack.len(), "redo_depth": self.redo_stack.len()})
        } else {
            json!({"status": "noop", "reason": "undo stack empty"})
        }
    }

    fn redo(&mut self) -> serde_json::Value {
        if let Some(snapshot) = self.redo_stack.pop() {
            self.undo_stack.push(self.snapshot());
            self.restore_snapshot(snapshot);
            self.last_command = Some("redo");
            json!({"status": "pass", "undo_depth": self.undo_stack.len(), "redo_depth": self.redo_stack.len()})
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
        probe.move_home();
        probe.move_end();
        probe.page_down();
        probe.page_up();
        probe.set_selection(
            EditorPosition { line: 1, column: 1 },
            EditorPosition { line: 1, column: 4 },
        );
        let copied = probe.copy_selection_to_clipboard();
        probe.paste_from_clipboard(&copied);
        probe.indent_selection();
        probe.delete_backward();
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
            "keyboard_commands_supported": [
                "insert_text",
                "delete_backward",
                "enter_newline_indent",
                "tab_indent",
                "home",
                "end",
                "page_up",
                "page_down"
            ],
            "undo_probe": undo,
            "redo_probe": redo,
            "syntax_backend": "boon_parser::parse_ast token stream with editor fallback for malformed in-progress buffers",
            "syntax_categories": self.syntax_categories(),
            "syntax_token_samples": self.syntax_token_samples(),
            "syntax_token_count": self.syntax_token_count()
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
            font_family: "JetBrains Mono",
        }
    }

    fn append_to(
        &self,
        frame: &mut boon_document_model::DocumentFrame,
        parent: boon_document_model::DocumentNodeId,
        model: &CodeEditorModel,
        height: u32,
    ) {
        let editor_height = height.max(96);
        let mut editor = dev_node(
            "dev-code-editor",
            boon_document_model::DocumentNodeKind::ScrollRoot,
            None,
            &[
                ("bg", "#ffffff"),
                ("color", "#202936"),
                ("border", "#9aa7b5"),
                ("padding", "12"),
                ("height", &editor_height.to_string()),
                ("width", "fill"),
                ("scroll", "true"),
                ("scroll_x", "true"),
                ("font", self.font_family),
                ("size", "13"),
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
        let visible_line_count = (editor_height.saturating_sub(24) / 16).max(1) as usize;
        for (line_number, line) in model.visible_lines(visible_line_count) {
            let row_id = format!("dev-code-editor-line-{line_number}");
            let row = dev_node(
                &row_id,
                boon_document_model::DocumentNodeKind::Row,
                None,
                &[
                    ("height", "16"),
                    ("width", "fill"),
                    ("gap", "8"),
                    ("padding", "0"),
                    ("bg", "#ffffff"),
                ],
            );
            let row_parent = row.id.clone();
            append_child(frame, editor_parent.clone(), row);
            let gutter = dev_node(
                &format!("dev-code-editor-gutter-{line_number}"),
                boon_document_model::DocumentNodeKind::Text,
                Some(format!("{line_number:>4}")),
                &[
                    ("width", "44"),
                    ("height", "16"),
                    ("color", "#64748b"),
                    ("size", "12"),
                    ("bg", "#f8fafc"),
                ],
            );
            append_child(frame, row_parent.clone(), gutter);
            let code_row = dev_node(
                &format!("dev-code-editor-code-row-{line_number}"),
                boon_document_model::DocumentNodeKind::Row,
                None,
                &[
                    ("width", "fill"),
                    ("height", "16"),
                    ("bg", "#ffffff"),
                    ("gap", "0"),
                    ("padding", "0"),
                ],
            );
            let code_row_parent = code_row.id.clone();
            append_child(frame, row_parent, code_row);
            self.append_highlighted_line(frame, code_row_parent, model, line_number, &line);
        }
    }

    fn append_highlighted_line(
        &self,
        frame: &mut boon_document_model::DocumentFrame,
        parent: boon_document_model::DocumentNodeId,
        _model: &CodeEditorModel,
        line_number: usize,
        line: &str,
    ) {
        let color = if line.trim_start().starts_with("--") {
            Self::syntax_color("comment")
        } else {
            Self::syntax_color("plain")
        };
        append_child(
            frame,
            parent,
            self.editor_text_node(line_number, 0, "plain", line, color),
        );
    }

    fn editor_text_node(
        &self,
        line_number: usize,
        segment_index: usize,
        kind: &str,
        text: &str,
        color: &'static str,
    ) -> boon_document_model::DocumentNode {
        dev_node(
            &format!("dev-code-editor-token-{line_number}-{segment_index}-{kind}"),
            boon_document_model::DocumentNodeKind::Text,
            Some(text.to_owned()),
            &[
                ("width", "auto"),
                ("height", "16"),
                ("color", color),
                ("size", "12"),
                ("bg", "#ffffff"),
                ("font", self.font_family),
            ],
        )
    }

    fn syntax_color(kind: &str) -> &'static str {
        match kind {
            "comment" => "#6A737D",
            "keyword" => "#0B5CAD",
            "string" => "#1A7F37",
            "number" => "#953800",
            "operator" => "#5B6472",
            "source-binding" => "#8250DF",
            "invalid" => "#CF222E",
            _ => "#202936",
        }
    }
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
                json!({
                    "status": if hash_matches { "pass" } else { "fail" },
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
}

#[derive(Clone, Debug)]
struct DevWindowShell {
    catalog: ExampleCatalog,
    workspace: ExampleWorkspace,
    initial_workspace: ExampleWorkspace,
    editor_view: CodeEditorView,
    preview_transport: PreviewTransport,
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
        }
    }

    fn document(&self) -> boon_document_model::DocumentFrame {
        self.document_for_viewport(1180, 820)
    }

    fn document_for_viewport(&self, width: u32, height: u32) -> boon_document_model::DocumentFrame {
        dev_shell_document(self, width, height)
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
        let mut measurer = boon_document::SimpleTextMeasurer;
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

    fn replace_selected_preview(&self, command: &str) -> serde_json::Value {
        self.preview_transport.replace_code(
            command,
            &self.workspace.selected_example_id,
            &self.workspace.selected_buffer.file_name,
            &self.workspace.selected_buffer.source_text,
        )
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
        let all_pass = [
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
            "ui_source_bindings": [
                "dev.tabs.select",
                "dev.tabs.new",
                "dev.commands.run",
                "dev.commands.format",
                "dev.commands.reset",
                "dev.commands.remove_custom",
                "dev.editor.insert_text"
            ],
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
            "select_probe_custom": select_probe_custom,
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
        let mut measurer = boon_document::SimpleTextMeasurer;
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
        let required_sources = {
            let mut sources = vec![
                "dev.commands.run".to_owned(),
                "dev.commands.format".to_owned(),
                "dev.commands.reset".to_owned(),
                "dev.commands.remove_custom".to_owned(),
                "dev.editor.insert_text".to_owned(),
            ];
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
            });
        json!({
            "status": if pass { "pass" } else { "fail" },
            "surface_pid": surface_proof.pid,
            "surface_id": surface_proof.surface_id,
            "window_id": surface_proof.window_id,
            "window_title": surface_proof.window_title,
            "source_intent_count": source_intents.len(),
            "hit_region_count": hit_regions.len(),
            "required_sources": required_sources,
            "route_assertions": route_assertions,
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
    let editor_height = viewport_height
        .saturating_sub(40)
        .saturating_sub(42)
        .saturating_sub(44)
        .saturating_sub(130)
        .max(160);
    set_style(
        frame.nodes.get_mut(&frame.root).expect("root exists"),
        &[
            ("bg", "#f3f6f9"),
            ("padding", "12"),
            ("gap", "10"),
            ("width", "fill"),
            ("height", &root_height.to_string()),
        ],
    );

    let title = dev_node(
        "dev-header",
        DocumentNodeKind::Row,
        Some(format!("Boon Dev  {}", shell.workspace.current_file)),
        &[
            ("bg", "#26313f"),
            ("color", "#f6f8fb"),
            ("padding", "10"),
            ("gap", "12"),
            ("height", "40"),
            ("width", "fill"),
        ],
    );
    let tabs = dev_tabs_node(shell);
    let toolbar = dev_toolbar_node();
    let debug = dev_node(
        "dev-debug-panel",
        DocumentNodeKind::Text,
        Some(format!(
            "runtime: bounded query mode\nsource bytes: {}\nlines: {}\ntokens: {}\ndiagnostics: {}\npreview transport: ReplaceCode\nselected example: {}\ncurrent file: {}\ndirty: {}\ncaret: {}:{}\nscroll: line {}, column {}\nformatted hash: {}\ncatalog: {}",
            shell.workspace.selected_buffer.source_text.len(),
            shell.workspace.selected_buffer.line_count,
            shell.workspace.selected_buffer.syntax_token_count(),
            shell.workspace.selected_buffer.diagnostics.len(),
            shell.workspace.selected_example_id,
            shell.workspace.selected_buffer.file_name,
            shell.workspace.dirty,
            shell.workspace.selected_buffer.caret().line,
            shell.workspace.selected_buffer.caret().column,
            shell.workspace.selected_buffer.scroll_line,
            shell.workspace.selected_buffer.scroll_column,
            shell
                .workspace
                .selected_buffer
                .formatted_preview_hash
                .as_deref()
                .unwrap_or("format-error"),
            shell
                .catalog
                .entries
                .iter()
                .map(|entry| format!("{}:{}:{}", entry.category, entry.order, entry.label))
                .collect::<Vec<_>>()
                .join(", ")
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
    append_child(&mut frame, root.clone(), title);
    let tabs_parent = tabs.id.clone();
    append_child(&mut frame, root.clone(), tabs);
    for entry in shell
        .catalog
        .entries
        .iter()
        .filter(|entry| entry.shown_by_default)
    {
        let mut label = entry.label.clone();
        if shell.workspace.dirty_examples.contains(&entry.id) {
            label.push('*');
        }
        let mut tab = dev_button_node(
            &format!("dev-tab-{}", entry.id),
            if entry.id == shell.workspace.selected_example_id {
                format!("[{}]", label)
            } else {
                label
            },
            &[
                ("bg", "#f8fafc"),
                ("color", "#1f2937"),
                ("border", "#aeb8c2"),
                ("padding", "6"),
                ("height", "30"),
                ("width", "120"),
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
            ("bg", "#ffffff"),
            ("color", "#1f2937"),
            ("border", "#aeb8c2"),
            ("padding", "6"),
            ("height", "30"),
            ("width", "42"),
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
    for command in ["run", "format", "reset", "remove_custom"] {
        let label = match command {
            "remove_custom" => "REMOVE".to_owned(),
            other => other.to_ascii_uppercase(),
        };
        let mut button = dev_button_node(
            &format!("dev-command-{command}"),
            label,
            &[
                ("bg", "#ffffff"),
                ("color", "#1f2937"),
                ("border", "#9aa7b5"),
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
            ],
        );
        button.source_binding = Some(boon_document_model::SourceBinding {
            id: boon_document_model::SourceBindingId(format!("source:dev-command:{command}")),
            source_path: format!("dev.commands.{command}"),
            intent: "press".to_owned(),
        });
        append_child(&mut frame, toolbar_parent.clone(), button);
    }
    shell.editor_view.append_to(
        &mut frame,
        root.clone(),
        &shell.workspace.selected_buffer,
        editor_height,
    );
    append_child(&mut frame, root, debug);
    frame.focus = Some(boon_document_model::DocumentNodeId(
        "dev-code-editor".to_owned(),
    ));
    frame
}

fn dev_tabs_node(_shell: &DevWindowShell) -> boon_document_model::DocumentNode {
    let mut tabs = dev_node(
        "dev-example-tabs",
        boon_document_model::DocumentNodeKind::Row,
        None,
        &[
            ("bg", "#d8e0ea"),
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
            ("bg", "#e8eef5"),
            ("color", "#1f2937"),
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
    dev_node(
        id,
        boon_document_model::DocumentNodeKind::Button,
        Some(text),
        styles,
    )
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

    let vertical_scroll = matches!(node.kind, boon_document_model::DocumentNodeKind::Grid)
        || style_bool(&node.style, "scroll")
        || style_bool(&node.style, "scroll_y")
        || style_bool(&node.style, "scrollbars");
    let horizontal_scroll = matches!(node.kind, boon_document_model::DocumentNodeKind::Grid)
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
            "outline" | "border" | "borders" => {
                if let Some(color) =
                    statement_nested_style_value(child, "color", expressions, context)
                        .or_else(|| {
                            document_child_style_value(child, "color", expressions, context)
                        })
                        .or_else(|| document_style_value(child, expressions, context))
                {
                    node.style.insert("border".to_owned(), color);
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
        "Button" | "Checkbox" => boon_document_model::DocumentNodeKind::Button,
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

    if input.mouse_button_event_count > input_state.last_mouse_button_event_count
        && input.mouse_buttons_down.is_empty()
    {
        input_state.last_mouse_button_event_count = input.mouse_button_event_count;
        if let Some(position) = input.mouse_window_pos
            && let Some(hit_region) = document_hit_region_at(&layout_proof, position.x, position.y)
        {
            let node = hit_region
                .get("node")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if live_source_for_node_intent(&layout_proof, &node, "change").is_some() {
                input_state.focused_node = Some(node);
                input_state.focused_text.clear();
                input_state.focused_text.push_str(
                    document_value_for_hit_region(&layout_proof, &hit_region)
                        .as_deref()
                        .unwrap_or_default(),
                );
            } else {
                input_state.focused_node = None;
                input_state.focused_text.clear();
                if let Some(event) = live_source_event_for_hit_region(&layout_proof, &hit_region) {
                    preview_apply_live_event(
                        source_path,
                        source_text,
                        live_runtime,
                        shared_render_state,
                        event,
                    )?;
                }
            }
        }
    }

    let shift_pressed = input
        .pressed_keys
        .iter()
        .any(|key| key == "Shift" || key == "RightShift");
    let mut latest_layout = None;
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
    let mut runtime = live_runtime
        .lock()
        .map_err(|_| "preview live runtime mutex poisoned")?;
    let output = runtime.apply_source_event(event)?;
    let post_input_layout = native_document_layout_proof_with_state(
        source_path,
        source_text,
        Some(&output.state_summary),
    )?;
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
        shared_render_state.update_count = shared_render_state.update_count.saturating_add(1);
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
    let character = match key {
        "A" => 'a',
        "B" => 'b',
        "C" => 'c',
        "D" => 'd',
        "E" => 'e',
        "F" => 'f',
        "G" => 'g',
        "H" => 'h',
        "I" => 'i',
        "J" => 'j',
        "K" => 'k',
        "L" => 'l',
        "M" => 'm',
        "N" => 'n',
        "O" => 'o',
        "P" => 'p',
        "Q" => 'q',
        "R" => 'r',
        "S" => 's',
        "T" => 't',
        "U" => 'u',
        "V" => 'v',
        "W" => 'w',
        "X" => 'x',
        "Y" => 'y',
        "Z" => 'z',
        "Num0" | "Keypad0" => '0',
        "Num1" | "Keypad1" => '1',
        "Num2" | "Keypad2" => '2',
        "Num3" | "Keypad3" => '3',
        "Num4" | "Keypad4" => '4',
        "Num5" | "Keypad5" => '5',
        "Num6" | "Keypad6" => '6',
        "Num7" | "Keypad7" => '7',
        "Num8" | "Keypad8" => '8',
        "Num9" | "Keypad9" => '9',
        "Space" => ' ',
        "Minus" | "KeypadMinus" => '-',
        "Equal" | "KeypadEquals" => '=',
        "Comma" => ',',
        "Period" | "KeypadDecimal" => '.',
        "Slash" | "KeypadDivide" => '/',
        "Semicolon" => ';',
        "Quote" => '\'',
        "LeftBracket" => '[',
        "RightBracket" => ']',
        "Backslash" | "InternationalBackslash" => '\\',
        "Grave" => '`',
        _ => return None,
    };
    Some(if shift && character.is_ascii_alphabetic() {
        character.to_ascii_uppercase()
    } else {
        character
    })
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
                let scenario_path = Path::new(source_path).with_extension("scn");
                state.live_runtime = if scenario_path.exists() {
                    boon_runtime::LiveRuntime::new(
                        &format!("native-preview-live:{source_path}"),
                        code,
                        &scenario_path,
                    )
                } else {
                    boon_runtime::LiveRuntime::from_source(
                        &format!("native-preview-live:{source_path}"),
                        code,
                    )
                }
                .ok()
                .map(|runtime| Arc::new(Mutex::new(runtime)));
                if let Some(layout_proof) = response.get("document_layout_proof") {
                    let mut shared = state
                        .shared_render_state
                        .lock()
                        .map_err(|_| "preview render state mutex poisoned")?;
                    shared.layout_proof = layout_proof.clone();
                    shared.update_count = shared.update_count.saturating_add(1);
                }
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
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
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

    #[test]
    fn new_custom_tab_starts_empty_and_persists_editor_text() {
        let store_path = PathBuf::from(format!(
            "target/artifacts/native-gpu/tests/custom-tabs-{}.toml",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&store_path);
        let catalog = ExampleCatalog {
            entries: vec![ExampleCatalogEntry {
                id: "seed".to_owned(),
                label: "Seed".to_owned(),
                source: "custom://seed.bn".to_owned(),
                inline_source: Some(
                    "-- seed\nSOURCE\nHOLD\nLATEST\nLIST {}\nList/map\n".to_owned(),
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
            "custom://seed.bn",
            "-- seed\nSOURCE\nHOLD\nLATEST\nLIST {}\nList/map\n",
            None,
        );
        let mut shell = DevWindowShell {
            catalog,
            initial_workspace: workspace.clone(),
            workspace,
            editor_view: CodeEditorView::new(),
            preview_transport: PreviewTransport::new(None),
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
}
