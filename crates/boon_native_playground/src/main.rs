use serde_json::json;
use std::path::{Path, PathBuf};

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
        other => Err(format!("unknown --role `{other}`").into()),
    }
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
    if let Some(report) = value_arg(args, "--report") {
        write_role_report(
            Path::new(&report),
            "preview",
            &args[1..],
            json!({
                "code_file": code_file,
                "source_bytes": source.len(),
                "received_example_name": false,
                "app_window_contract": boon_native_app_window::app_window_contract(),
                "native_gpu_versions": boon_native_gpu::NativeGpuRenderer::required_backend_versions(),
                "note": "preview role scaffold validates code-source boundary but does not yet create the final app_window/wgpu surface"
            }),
        )?;
    }
    println!("preview-ready source_bytes={}", source.len());
    Ok(())
}

fn run_dev(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if value_arg(args, "--connect").is_none() {
        return Err("dev role requires --connect <preview-socket>".into());
    }
    if let Some(report) = value_arg(args, "--report") {
        write_role_report(
            Path::new(&report),
            "dev",
            &args[1..],
            json!({
                "connect": value_arg(args, "--connect"),
                "observability_mode": "bounded-telemetry-and-query-scaffold",
                "full_state_mirroring_allowed": false,
                "note": "dev role scaffold validates IPC boundary but does not yet render a native dev window"
            }),
        )?;
    }
    println!("dev-ready");
    Ok(())
}

fn run_desktop(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_arg(args, "--example").unwrap_or_else(|| "cells".to_owned());
    let source_path = PathBuf::from(format!("examples/{example}.bn"));
    let source = std::fs::read_to_string(&source_path)?;
    if let Some(report) = value_arg(args, "--report") {
        write_role_report(
            Path::new(&report),
            "desktop",
            &args[1..],
            json!({
                "resolved_example": example,
                "resolved_code_file": source_path,
                "source_bytes": source.len(),
                "preview_receives_example_name": false,
                "preview_launch_form": "--role preview --code-file <resolved-code-file>",
                "note": "desktop scaffold resolves examples to source but does not yet spawn two app_window/wgpu child processes"
            }),
        )?;
    }
    println!("desktop-resolved source_bytes={}", source.len());
    Ok(())
}

fn write_role_report(
    path: &Path,
    role: &str,
    args: &[String],
    details: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let report = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "boon-native-playground-role-scaffold",
        "command_argv": args,
        "exit_status": 0,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "budget_hash": "n/a",
        "graph_node_count": 0,
        "per_step_pass_fail": [
            {"id": format!("native-role-{role}-scaffold"), "pass": true}
        ],
        "artifact_sha256s": [],
        "native_role": role,
        "details": details
    });
    boon_runtime::write_json(path, &report)?;
    boon_runtime::verify_report_schema(path)?;
    Ok(())
}

fn value_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].clone())
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

fn current_binary_hash() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| boon_runtime::sha256_file(&path).ok())
        .unwrap_or_else(|| "unknown".to_owned())
}
