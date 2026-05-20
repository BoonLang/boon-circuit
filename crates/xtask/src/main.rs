#![recursion_limit = "256"]

use boon_runtime::{
    VerificationLayer, example_paths, run_scenario, run_scenario_source_with_step_limit,
    verify_report_schema, write_json,
};
use serde_json::json;
use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

const XTASK_COMMANDS: &[&str] = &[
    "verify-example-headed-ply",
    "verify-example-human",
    "prepare-example-human-report",
    "verify-example-semantic",
    "verify-example-ply-headless",
    "verify-example-speed",
    "verify-example-negative",
    "verify-example-all",
    "verify-examples-all",
    "verify-os-input-probe",
    "verify-foundation",
    "verify-playground-launch",
    "verify-playground-background-launch",
    "bench-example",
    "verify-playground-custom-source",
    "write-manual-handoff",
    "verify-report-schema",
    "audit-goal-readiness",
    "audit-manual-readiness",
    "verify-todomvc-headed-ply",
    "verify-todomvc-human",
    "prepare-todomvc-human-report",
    "verify-todomvc-semantic",
    "verify-todomvc-ply-headless",
    "verify-todomvc-speed",
    "verify-todomvc-negative",
    "verify-todomvc-all",
    "bench-todomvc",
    "explain-todomvc-hardware",
    "verify-cells-headed-ply",
    "verify-cells-human",
    "prepare-cells-human-report",
    "verify-cells-semantic",
    "verify-cells-ply-headless",
    "verify-cells-speed",
    "verify-cells-negative",
    "verify-cells-all",
];

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = args.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };
    match command {
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "verify-example-semantic" => verify_named(&args, VerificationLayer::Semantic),
        "verify-example-ply-headless" => verify_named(&args, VerificationLayer::HeadlessPly),
        "verify-example-headed-ply" => verify_named(&args, VerificationLayer::HeadedPly),
        "verify-example-human" => verify_human(named_arg(&args, 1)?, &args),
        "prepare-example-human-report" => prepare_human_report(named_arg(&args, 1)?, &args),
        "verify-example-speed" => verify_named(&args, VerificationLayer::Speed),
        "verify-example-negative" => verify_negative(&args),
        "verify-example-all" => verify_all_with_optional_report(named_arg(&args, 1)?, &args),
        "verify-examples-all" => verify_examples_all(&args),
        "verify-os-input-probe" => verify_os_input_probe(&args),
        "verify-foundation" => verify_foundation(&args),
        "verify-playground-launch" => verify_playground_launch(&args),
        "verify-playground-background-launch" => verify_playground_background_launch(&args),
        "verify-playground-custom-source" => verify_playground_custom_source(&args),
        "write-manual-handoff" => write_manual_handoff(&args),
        "verify-report-schema" => verify_reports_schema(),
        "audit-goal-readiness" | "audit-manual-readiness" => audit_goal_readiness(&args),
        "bench-example" => bench_example(named_arg(&args, 1)?, &args),
        "verify-todomvc-semantic" => verify_specific("todomvc", VerificationLayer::Semantic, &args),
        "verify-todomvc-ply-headless" => {
            verify_specific("todomvc", VerificationLayer::HeadlessPly, &args)
        }
        "verify-todomvc-headed-ply" => {
            verify_specific("todomvc", VerificationLayer::HeadedPly, &args)
        }
        "verify-todomvc-human" => verify_human("todomvc", &args),
        "prepare-todomvc-human-report" => prepare_human_report("todomvc", &args),
        "verify-todomvc-speed" => verify_specific("todomvc", VerificationLayer::Speed, &args),
        "verify-todomvc-negative" => verify_negative_name("todomvc"),
        "verify-todomvc-all" => verify_all_with_optional_report("todomvc", &args),
        "bench-todomvc" => bench_example("todomvc", &args),
        "explain-todomvc-hardware" => explain_hardware("todomvc", &args),
        "verify-cells-semantic" => verify_specific("cells", VerificationLayer::Semantic, &args),
        "verify-cells-ply-headless" => {
            verify_specific("cells", VerificationLayer::HeadlessPly, &args)
        }
        "verify-cells-headed-ply" => verify_specific("cells", VerificationLayer::HeadedPly, &args),
        "verify-cells-human" => verify_human("cells", &args),
        "prepare-cells-human-report" => prepare_human_report("cells", &args),
        "verify-cells-speed" => verify_specific("cells", VerificationLayer::Speed, &args),
        "verify-cells-negative" => verify_negative_name("cells"),
        "verify-cells-all" => verify_all_with_optional_report("cells", &args),
        other => Err(format!("unknown xtask command `{other}`").into()),
    }
}

fn print_help() {
    println!("boon-circuit xtask");
    println!();
    println!("Usage:");
    println!("  cargo xtask <command> [args]");
    println!();
    println!("Commands:");
    for command in XTASK_COMMANDS {
        println!("  {command}");
    }
}

fn verify_named(
    args: &[String],
    layer: VerificationLayer,
) -> Result<(), Box<dyn std::error::Error>> {
    verify_specific(named_arg(args, 1)?, layer, args)
}

fn verify_specific(
    name: &str,
    layer: VerificationLayer,
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    if matches!(layer, VerificationLayer::Speed) && should_reexec_speed_in_release() {
        return reexec_speed_in_release(name, args);
    }
    let (source, scenario, _) = example_paths(name)?;
    let report = report_arg(args).unwrap_or_else(|| report_path(name, layer));
    if matches!(layer, VerificationLayer::Human) {
        return verify_existing_human_report(name, args);
    }
    if matches!(layer, VerificationLayer::HeadedPly) {
        let _headed_lock = HeadedVerifierLock::acquire()?;
        let timeout = headed_verifier_timeout();
        let mut command = Command::new("cargo");
        command.args([
            "run",
            "--release",
            "-p",
            "boon_ply_playground",
            "--",
            "--verify-headed",
            "--example",
            name,
            "--report",
            report.to_str().ok_or("report path is not utf-8")?,
        ]);
        let status = match run_command_with_timeout(&mut command, timeout) {
            Ok(status) => status,
            Err(error) => {
                write_headed_debug_failure(name, &report, timeout, &error.to_string())?;
                return Err(error);
            }
        };
        if !status.success() {
            return Err(format!("headed Ply verifier failed for {name}").into());
        }
        verify_report_schema(&report)?;
        let _ = std::fs::remove_file(headed_debug_failure_path(name));
        return Ok(());
    }
    let output = run_scenario(&source, &scenario, layer, Some(&report))?;
    if matches!(layer, VerificationLayer::Speed) {
        verify_budget_passed(&output.report)?;
    }
    verify_report_schema(&report)?;
    Ok(())
}

fn bench_example(name: &str, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if should_reexec_benchmark_in_release() {
        return reexec_benchmark_in_release(args);
    }
    let iterations = value_arg(args, "--iterations")
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|error| format!("--iterations must be a positive integer: {error}"))
        })
        .transpose()?
        .unwrap_or(100);
    if iterations == 0 {
        return Err("--iterations must be greater than zero".into());
    }
    let (source, scenario, budget) = example_paths(name)?;
    let report = report_arg(args)
        .unwrap_or_else(|| PathBuf::from(format!("target/reports/{name}-bench.json")));
    let speed_report = value_arg(args, "--speed-report")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("target/reports/{name}-bench-speed.json")));

    let speed_output = run_scenario(
        &source,
        &scenario,
        VerificationLayer::Speed,
        Some(&speed_report),
    )?;
    verify_budget_passed(&speed_output.report)?;
    verify_report_schema(&speed_report)?;

    let started = Instant::now();
    for _ in 0..iterations {
        run_scenario(&source, &scenario, VerificationLayer::Speed, None)?;
    }
    let total_ms = started.elapsed().as_secs_f64() * 1000.0;
    let average_ms = total_ms / iterations as f64;
    let source_hash = boon_runtime::sha256_file(&source)?;
    let scenario_hash = boon_runtime::sha256_file(&scenario)?;
    let budget_hash = boon_runtime::sha256_file(&budget)?;
    let speed_report_hash = boon_runtime::sha256_file(&speed_report)?;
    let program_hash = speed_output
        .report
        .get("program_hash")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&source_hash)
        .to_owned();
    let graph_node_count = speed_output
        .report
        .get("graph_node_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let budget_check = speed_output
        .report
        .get("budget_check")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let input_to_idle_latency = speed_output
        .report
        .get("input_to_idle_ms_p50_p95_p99_max")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let semantic_tick_latency = speed_output
        .report
        .get("semantic_tick_ms_p50_p95_p99_max")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let render_lowering_latency = speed_output
        .report
        .get("render_lowering_ms_p50_p95_p99_max")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let ply_patch_apply_latency = speed_output
        .report
        .get("ply_patch_apply_ms_p50_p95_p99_max")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let frame_time_latency = speed_output
        .report
        .get("frame_time_ms_p50_p95_p99_max")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let dirty_key_count = speed_output
        .report
        .get("dirty_key_count_p50_p95_p99_max")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let render_patch_count = speed_output
        .report
        .get("render_patch_count_p50_p95_p99_max")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let graph_rebuild_count = speed_output
        .report
        .get("graph_rebuild_count")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let allocations = speed_output
        .report
        .get("allocations")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let stress_profiles = speed_output
        .report
        .get("stress_profiles")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let bounded_allocs = speed_output
        .report
        .get("allocations")
        .and_then(|allocations| allocations.get("bounded_profile_allocs_after_warmup"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let report_json = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": if name == "todomvc" { "bench-todomvc" } else { "bench-example" },
        "command_argv": args,
        "exit_status": 0,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_path": source.display().to_string(),
        "source_hash": source_hash,
        "scenario_path": scenario.display().to_string(),
        "scenario_hash": scenario_hash,
        "program_hash": program_hash,
        "budget_hash": budget_hash,
        "graph_node_count": graph_node_count,
        "budget_check": budget_check,
        "input_to_idle_ms_p50_p95_p99_max": input_to_idle_latency,
        "semantic_tick_ms_p50_p95_p99_max": semantic_tick_latency,
        "render_lowering_ms_p50_p95_p99_max": render_lowering_latency,
        "ply_patch_apply_ms_p50_p95_p99_max": ply_patch_apply_latency,
        "frame_time_ms_p50_p95_p99_max": frame_time_latency,
        "dirty_key_count_p50_p95_p99_max": dirty_key_count,
        "render_patch_count_p50_p95_p99_max": render_patch_count,
        "graph_rebuild_count": graph_rebuild_count,
        "allocations": allocations,
        "stress_profiles": stress_profiles,
        "per_step_pass_fail": [
            {
                "id": "bench-iterations",
                "pass": true,
                "detail": format!("{iterations} full speed-layer {name} scenario iterations completed")
            },
            {
                "id": "speed-report-schema",
                "pass": true,
                "detail": format!("{} schema-valid", speed_report.display())
            },
            {
                "id": "speed-budget-check",
                "pass": true,
                "detail": "speed report passed budget checks"
            }
        ],
        "artifact_sha256s": [
            {
                "path": speed_report.display().to_string(),
                "sha256": speed_report_hash
            }
        ],
        "benchmark": {
            "example": name,
            "iterations": iterations,
            "total_ms": total_ms,
            "average_ms_per_iteration": average_ms,
            "iteration_scope": "full_speed_layer_scenario_rerun_including_reportless_verifier_overhead",
            "speed_report_path": speed_report.display().to_string(),
            "speed_report_layer": "speed",
            "interaction_latency_source": "input_to_idle_ms_p50_p95_p99_max copied from linked speed report",
            "heap_alloc_count_after_warmup": bounded_allocs
        }
    });
    write_json(&report, &report_json)?;
    verify_report_schema(&report)?;
    println!(
        "{name} static-runtime bench: {iterations} iterations in {:.3}ms ({:.3}ms/iteration)",
        total_ms, average_ms
    );
    println!("wrote {}", report.display());
    Ok(())
}

fn should_reexec_benchmark_in_release() -> bool {
    cfg!(debug_assertions) && std::env::var("BOON_XTASK_BENCH_CHILD").as_deref() != Ok("1")
}

fn reexec_benchmark_in_release(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("cargo")
        .args(["run", "--release", "-p", "xtask", "--"])
        .args(args)
        .env("BOON_XTASK_BENCH_CHILD", "1")
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "release benchmark failed: cargo run --release -p xtask -- {}",
            args.join(" ")
        )
        .into())
    }
}

fn run_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> Result<std::process::ExitStatus, Box<dyn std::error::Error>> {
    let mut child = command.spawn()?;
    let started = SystemTime::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if started.elapsed().unwrap_or_default() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "timed out after {}s waiting for {:?}",
                timeout.as_secs(),
                command
            )
            .into());
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn headed_verifier_timeout() -> Duration {
    std::env::var("BOON_HEADED_VERIFIER_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(120))
}

fn write_headed_debug_failure(
    name: &str,
    report: &Path,
    timeout: Duration,
    error: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let debug_path = headed_debug_failure_path(name);
    let debug_report = json!({
        "status": "fail",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "headed-ply-debug-failure",
        "command_argv": ["cargo", "xtask", format!("verify-{name}-headed-ply")],
        "exit_status": 1,
        "example": name,
        "intended_report": report,
        "timeout_seconds": timeout.as_secs(),
        "error": error,
        "failure_is_blocker": true,
        "note": "debug report only; top-level headed-ply report is intentionally absent or invalid until real headed verification completes"
    });
    write_json(&debug_path, &debug_report)?;
    Ok(())
}

fn headed_debug_failure_path(name: &str) -> PathBuf {
    PathBuf::from(format!(
        "target/reports/debug/{name}-headed-ply-failure.json"
    ))
}

struct HeadedVerifierLock {
    path: PathBuf,
}

impl HeadedVerifierLock {
    fn acquire() -> Result<Self, Box<dyn std::error::Error>> {
        let path = PathBuf::from("target/reports/.headed-ply.lock");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let started = SystemTime::now();
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    writeln!(file, "pid={}", std::process::id())?;
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    remove_stale_headed_lock(&path)?;
                    if started.elapsed().unwrap_or_default() > Duration::from_secs(120) {
                        return Err(format!(
                            "timed out waiting for headed Ply verifier lock `{}`",
                            path.display()
                        )
                        .into());
                    }
                    thread::sleep(Duration::from_millis(250));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl Drop for HeadedVerifierLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn remove_stale_headed_lock(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if let Ok(text) = std::fs::read_to_string(path)
        && let Some(pid) = text
            .lines()
            .find_map(|line| line.strip_prefix("pid=")?.trim().parse::<u32>().ok())
        && !process_is_alive(pid)
    {
        std::fs::remove_file(path)?;
        return Ok(());
    }
    let modified = metadata.modified().unwrap_or(SystemTime::now());
    if modified.elapsed().unwrap_or_default() > Duration::from_secs(300) {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn process_is_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

fn verify_budget_passed(report: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    if report
        .get("build_profile")
        .and_then(serde_json::Value::as_str)
        != Some("release")
    {
        return Err("speed report was not generated by a release binary".into());
    }
    let Some(checks) = report
        .get("budget_check")
        .and_then(serde_json::Value::as_object)
    else {
        return Err("speed report missing budget_check".into());
    };
    let failed = checks
        .iter()
        .filter_map(|(name, value)| {
            (value.get("pass").and_then(serde_json::Value::as_bool) != Some(true))
                .then_some(name.as_str())
        })
        .collect::<Vec<_>>();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("speed budget failed: {}", failed.join(", ")).into())
    }
}

fn should_reexec_speed_in_release() -> bool {
    cfg!(debug_assertions) && std::env::var("BOON_XTASK_RELEASE_CHILD").as_deref() != Ok("1")
}

fn reexec_speed_in_release(name: &str, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let release_args = if args.is_empty() {
        vec![format!("verify-{name}-speed")]
    } else {
        args.to_vec()
    };
    let status = Command::new("cargo")
        .args(["run", "--release", "-p", "xtask", "--"])
        .args(&release_args)
        .env("BOON_XTASK_RELEASE_CHILD", "1")
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "release speed verifier failed: cargo run --release -p xtask -- {}",
            release_args.join(" ")
        )
        .into())
    }
}

fn verify_human(name: &str, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.iter().any(|arg| arg == "--check") {
        verify_existing_human_report(name, args)?;
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--write-template") {
        write_manual_template(
            name,
            report_arg(args).unwrap_or_else(|| manual_template_path(name)),
        )?;
        return Ok(());
    }
    Err(format!(
        "manual verification for `{name}` cannot be generated automatically; run the playground with a human observer, then check a report with `--check --report <path>` or write a checklist template with `--write-template`"
    )
    .into())
}

fn verify_existing_human_report(
    name: &str,
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let report = report_arg(args).unwrap_or_else(|| report_path(name, VerificationLayer::Human));
    if !report.exists() {
        return Err(format!(
            "missing manual human report `{}`; run `cargo xtask verify-{name}-human --write-template`, perform a real headed manual pass, fill artifact hashes/checklist, then rerun with --check",
            report.display()
        )
        .into());
    }
    let max_age_seconds = max_age_seconds(args)?.unwrap_or(24 * 60 * 60);
    verify_human_report(&report, max_age_seconds)
}

fn write_manual_template(name: &str, path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let (source, scenario, _) = example_paths(name)?;
    let scenario_data = boon_runtime::parse_scenario(&scenario)?;
    let headed_report_path = report_path(name, VerificationLayer::HeadedPly);
    let headed_report = headed_report_path
        .exists()
        .then(|| read_json(&headed_report_path))
        .transpose()?;
    let headed_field = |key: &str| {
        headed_report
            .as_ref()
            .and_then(|report| report.get(key))
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    };
    let headed_string = |key: &str, fallback: &str| {
        headed_report
            .as_ref()
            .and_then(|report| report.get(key))
            .and_then(serde_json::Value::as_str)
            .unwrap_or(fallback)
            .to_owned()
    };
    let checkpoint_paths = headed_report
        .as_ref()
        .and_then(|report| report.get("checkpoint_screenshot_or_video_paths"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let artifact_sha256s = headed_report
        .as_ref()
        .and_then(|report| report.get("artifact_sha256s"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let headed_report_sha256 = if headed_report_path.exists() {
        file_hash(&headed_report_path.to_string_lossy())
    } else {
        "missing-headed-report".to_owned()
    };
    let headed_os_input_step_count = headed_report
        .as_ref()
        .and_then(|report| report.get("os_input_steps"))
        .and_then(serde_json::Value::as_array)
        .map(|steps| steps.len())
        .unwrap_or_default();
    let report = json!({
        "status": "needs_manual",
        "report_version": 1,
        "generated_at_utc": "fill-with-unix-seconds",
        "command": "human",
        "command_argv": ["cargo", "xtask", format!("verify-{name}-human"), "--check", "--report", format!("target/reports/{name}-human.json")],
        "layer": "human",
        "exit_status": 0,
        "git_commit": git_commit(),
        "binary_hash": headed_string("binary_hash", "copy-from-headed-report-or-current-verifier"),
        "source_path": source,
        "source_hash": file_hash(&format!("examples/{name}.bn")),
        "scenario_path": scenario,
        "scenario_hash": file_hash(&format!("examples/{name}.scn")),
        "program_hash": file_hash(&format!("examples/{name}.bn")),
        "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
        "graph_node_count": headed_field("graph_node_count"),
        "headed_report_path": headed_report_path,
        "headed_report_sha256": headed_report_sha256,
        "headed_input_injection_method": headed_string("input_injection_method", "missing-headed-report"),
        "headed_os_input_step_count": headed_os_input_step_count,
        "headed_os_input_missing_labels": headed_report
            .as_ref()
            .and_then(|report| report.get("os_input_coverage"))
            .and_then(|coverage| coverage.get("missing_full_os_pointer_keyboard_steps"))
            .cloned()
            .unwrap_or_else(|| json!(["missing-headed-report"])),
        "input_injection_method": "human_visible_window",
        "manual_observer": "fill-real-observer-name",
        "manual_input_route": "human_visible_window",
        "manual_artifact_capture_method": "fill-screenshot-or-video-captured-during-visible-manual-session",
        "manual_started_at_utc": "fill-with-unix-seconds",
        "manual_finished_at_utc": "fill-with-unix-seconds",
        "manual_session_duration_seconds": "fill-with-seconds",
        "display_server": headed_string("display_server", "copy-from-headed-report-or-fill-live-desktop"),
        "display_socket_or_compositor_connection": headed_string("display_socket_or_compositor_connection", "copy-from-headed-report-or-fill-live-desktop"),
        "window_backend": headed_string("window_backend", "ply-engine/macroquad"),
        "display_scale": headed_field("display_scale"),
        "window_pid": "fill-visible-playground-window-pid",
        "window_title": "Boon Circuit Ply Playground",
        "input_backend": "human-visible-window-pointer-keyboard",
        "capture_backend": "fill-manual-capture-backend",
        "focused_window_proof": "fill-visible-window-focus-proof",
        "manual_notes": "fill visual quality notes and any deviations",
        "manual_checklist_pass_fail": scenario_data.step.iter().map(|step| (step.id.clone(), json!(false))).collect::<serde_json::Map<_, _>>(),
        "visual_checkpoint_pass_fail": [],
        "per_step_pass_fail": [],
        "headed_checkpoint_screenshot_or_video_paths": checkpoint_paths,
        "headed_artifact_sha256s": artifact_sha256s,
        "checkpoint_screenshot_or_video_paths": [],
        "artifact_sha256s": []
    });
    write_json(&path, &report)?;
    eprintln!(
        "wrote manual checklist template `{}`; fill it from a real headed session and rerun with --check",
        path.display()
    );
    Ok(())
}

fn prepare_human_report(name: &str, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let template = value_arg(args, "--template")
        .map(PathBuf::from)
        .unwrap_or_else(|| manual_template_path(name));
    let report_path =
        report_arg(args).unwrap_or_else(|| report_path(name, VerificationLayer::Human));
    let observer = required_value_arg(args, "--observer")?;
    let started = required_value_arg(args, "--started")?;
    let finished = required_value_arg(args, "--finished")?;
    let notes = required_value_arg(args, "--notes")?;
    let capture_method = required_value_arg(args, "--capture-method")?;
    let window_pid = required_value_arg(args, "--window-pid")?;
    let focused_window_proof = required_value_arg(args, "--focused-window-proof")?;
    let artifacts = value_args(args, "--artifact");
    let passed_labels = value_args(args, "--pass-label")
        .into_iter()
        .collect::<BTreeSet<_>>();
    if artifacts.is_empty() {
        return Err("prepare human report requires at least one --artifact <path>".into());
    }
    let started_seconds = started.parse::<u64>()?;
    let finished_seconds = finished.parse::<u64>()?;
    let window_pid_value = window_pid.parse::<u64>()?;
    if window_pid_value == 0 {
        return Err("--window-pid must be a positive process id".into());
    }
    if finished_seconds < started_seconds {
        return Err("--finished must be greater than or equal to --started".into());
    }
    let mut report = read_json(&template)?;
    let object = report
        .as_object_mut()
        .ok_or("manual template is not a JSON object")?;
    let command_name = args
        .first()
        .cloned()
        .unwrap_or_else(|| format!("prepare-{name}-human-report"));
    let mut command_argv = vec!["cargo".to_owned(), "xtask".to_owned()];
    command_argv.extend(args.iter().cloned());
    object.insert("status".to_owned(), json!("pass"));
    object.insert(
        "generated_at_utc".to_owned(),
        json!(current_unix_seconds().to_string()),
    );
    object.insert("command".to_owned(), json!(command_name));
    object.insert("command_argv".to_owned(), json!(command_argv));
    object.insert(
        "manual_report_prepared_by".to_owned(),
        json!(
            args.first()
                .cloned()
                .unwrap_or_else(|| format!("prepare-{name}-human-report"))
        ),
    );
    object.insert(
        "manual_report_template_path".to_owned(),
        json!(template.display().to_string()),
    );
    object.insert(
        "manual_report_template_sha256".to_owned(),
        json!(file_hash(&template.to_string_lossy())),
    );
    object.insert("manual_observer".to_owned(), json!(observer));
    object.insert("manual_started_at_utc".to_owned(), json!(started));
    object.insert("manual_finished_at_utc".to_owned(), json!(finished));
    object.insert(
        "manual_session_duration_seconds".to_owned(),
        json!(finished_seconds.saturating_sub(started_seconds).to_string()),
    );
    object.insert("manual_notes".to_owned(), json!(notes));
    object.insert(
        "manual_artifact_capture_method".to_owned(),
        json!(capture_method),
    );
    object.insert(
        "input_injection_method".to_owned(),
        json!("human_visible_window"),
    );
    object.insert("window_pid".to_owned(), json!(window_pid_value));
    object.insert(
        "window_title".to_owned(),
        json!(
            value_arg(args, "--window-title")
                .unwrap_or_else(|| "Boon Circuit Ply Playground".to_owned())
        ),
    );
    if let Some(value) = value_arg(args, "--display-server") {
        object.insert("display_server".to_owned(), json!(value));
    }
    if let Some(value) = value_arg(args, "--display-connection") {
        object.insert(
            "display_socket_or_compositor_connection".to_owned(),
            json!(value),
        );
    }
    if let Some(value) = value_arg(args, "--display-scale") {
        object.insert("display_scale".to_owned(), json!(value.parse::<f64>()?));
    }
    if let Some(value) = value_arg(args, "--window-backend") {
        object.insert("window_backend".to_owned(), json!(value));
    }
    object.insert(
        "input_backend".to_owned(),
        json!(
            value_arg(args, "--input-backend")
                .unwrap_or_else(|| "human-visible-window-pointer-keyboard".to_owned())
        ),
    );
    object.insert(
        "capture_backend".to_owned(),
        json!(value_arg(args, "--capture-backend").unwrap_or_else(|| capture_method.clone())),
    );
    object.insert(
        "focused_window_proof".to_owned(),
        json!(focused_window_proof),
    );
    let checklist = object
        .get_mut("manual_checklist_pass_fail")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("manual template missing manual_checklist_pass_fail object")?;
    let expected_labels = checklist.keys().cloned().collect::<BTreeSet<_>>();
    if expected_labels.is_empty() {
        return Err("manual template has an empty checklist".into());
    }
    let missing_labels = expected_labels
        .difference(&passed_labels)
        .cloned()
        .collect::<Vec<_>>();
    let unknown_labels = passed_labels
        .difference(&expected_labels)
        .cloned()
        .collect::<Vec<_>>();
    if !missing_labels.is_empty() || !unknown_labels.is_empty() {
        return Err(format!(
            "prepare human report requires explicit --pass-label for every scenario label; missing={missing_labels:?}, unknown={unknown_labels:?}"
        )
        .into());
    }
    for (label, value) in checklist {
        *value = json!(passed_labels.contains(label));
    }
    object.insert(
        "checkpoint_screenshot_or_video_paths".to_owned(),
        json!(artifacts),
    );
    object.insert(
        "visual_checkpoint_pass_fail".to_owned(),
        json!(
            artifacts
                .iter()
                .map(|path| json!({
                    "path": path,
                    "pass": true,
                    "checked_by": observer,
                    "note": "manual visible-session checkpoint"
                }))
                .collect::<Vec<_>>()
        ),
    );
    object.insert(
        "artifact_sha256s".to_owned(),
        json!(
            artifacts
                .iter()
                .map(|path| json!({
                    "path": path,
                    "sha256": file_hash(path)
                }))
                .collect::<Vec<_>>()
        ),
    );
    let temp_report_path = report_path.with_extension(format!(
        "{}tmp",
        report_path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| format!("{extension}."))
            .unwrap_or_default()
    ));
    write_json(&temp_report_path, &report)?;
    if let Err(error) = verify_human_report(&temp_report_path, 24 * 60 * 60) {
        let _ = std::fs::remove_file(&temp_report_path);
        return Err(error);
    }
    std::fs::rename(&temp_report_path, &report_path)?;
    eprintln!(
        "wrote checked human report `{}` from `{}`",
        report_path.display(),
        template.display()
    );
    Ok(())
}

fn verify_examples_all(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let check_existing = args.iter().any(|arg| arg == "--check-existing");
    let aggregate_path =
        report_arg(args).unwrap_or_else(|| PathBuf::from("target/reports/examples-all.json"));
    let example_reports = [
        PathBuf::from("target/reports/todomvc-all.json"),
        PathBuf::from("target/reports/cells-all.json"),
    ];
    let todomvc_args = example_all_command_args("todomvc", &example_reports[0], check_existing);
    if let Err(error) = verify_all_with_optional_report("todomvc", &todomvc_args) {
        write_examples_all_blocked_debug_report(args, "todomvc", &error.to_string())?;
        return Err(error);
    }
    let cells_args = example_all_command_args("cells", &example_reports[1], check_existing);
    if let Err(error) = verify_all_with_optional_report("cells", &cells_args) {
        write_examples_all_blocked_debug_report(args, "cells", &error.to_string())?;
        return Err(error);
    }
    for report in &example_reports {
        verify_report_schema(report)?;
    }
    let aggregate = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-examples-all",
        "command_argv": args,
        "exit_status": 0,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_hash": "see example reports",
        "scenario_hash": "see example reports",
        "program_hash": "see example reports",
        "budget_hash": "see example reports",
        "graph_node_count": "see example reports",
        "per_step_pass_fail": [
            {"id": "todomvc-all-report", "pass": true},
            {"id": "cells-all-report", "pass": true}
        ],
        "artifact_sha256s": example_reports.iter().map(|path| json!({
            "path": path,
            "sha256": boon_runtime::sha256_file(path).unwrap_or_else(|_| "missing".to_owned())
        })).collect::<Vec<_>>(),
        "example_all_reports": example_reports,
    });
    write_json(&aggregate_path, &aggregate)?;
    let _ = std::fs::remove_file("target/reports/debug/examples-all-blocked.json");
    verify_report_schema(&aggregate_path)?;
    Ok(())
}

fn example_all_command_args(name: &str, report: &Path, check_existing: bool) -> Vec<String> {
    let mut args = vec![
        format!("verify-{name}-all"),
        "--report".to_owned(),
        report.display().to_string(),
    ];
    if check_existing {
        args.push("--check-existing".to_owned());
    }
    args
}

fn write_examples_all_blocked_debug_report(
    args: &[String],
    blocked_example: &str,
    error: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from("target/reports/debug/examples-all-blocked.json");
    let blocked_debug_report = PathBuf::from(format!(
        "target/reports/debug/{blocked_example}-all-blocked.json"
    ));
    let artifact_sha256s = if blocked_debug_report.exists() {
        vec![artifact_hash(&blocked_debug_report)?]
    } else {
        Vec::new()
    };
    let report = json!({
        "status": "fail",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-examples-all-blocked",
        "command_argv": args,
        "exit_status": 1,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_hash": "see blocked example",
        "scenario_hash": "see blocked example",
        "program_hash": "see blocked example",
        "budget_hash": "see blocked example",
        "graph_node_count": "see blocked example",
        "per_step_pass_fail": [{
            "id": format!("{blocked_example}:all:blocked"),
            "pass": false,
            "detail": error
        }],
        "artifact_sha256s": artifact_sha256s,
        "blocked_example": blocked_example,
        "blocked_example_debug_report": blocked_debug_report,
        "blocker": error,
        "note": "debug-only failure artifact; target/reports/examples-all.json is intentionally not written until every accepted example all report passes"
    });
    write_json(&path, &report)?;
    Ok(())
}

fn verify_os_input_probe(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let report =
        report_arg(args).unwrap_or_else(|| PathBuf::from("target/reports/os-input-probe.json"));
    let status = Command::new("cargo")
        .args([
            "run",
            "-p",
            "boon_ply_playground",
            "--",
            "--verify-os-input-probe",
            "--report",
            report.to_str().ok_or("report path is not utf-8")?,
        ])
        .env("BOON_ALLOW_OS_INPUT_PROBE", "1")
        .status()?;
    if !status.success() {
        return Err("OS input probe failed".into());
    }
    verify_report_schema(&report)?;
    Ok(())
}

fn verify_foundation(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let report_path =
        report_arg(args).unwrap_or_else(|| PathBuf::from("target/reports/foundation.json"));
    let commands: &[(&str, &[&str])] = &[
        ("cargo-test-boon-parser", &["test", "-p", "boon_parser"]),
        ("cargo-test-boon-ir", &["test", "-p", "boon_ir"]),
        ("cargo-test-boon-runtime", &["test", "-p", "boon_runtime"]),
        ("cargo-test-workspace", &["test", "--workspace"]),
    ];

    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let started = Instant::now();
    for (id, cargo_args) in commands {
        let command_started = Instant::now();
        let output = Command::new("cargo").args(*cargo_args).output()?;
        let duration_ms = command_started.elapsed().as_millis() as u64;
        let pass = output.status.success();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        checks.push(json!({
            "id": id,
            "pass": pass,
            "command": format!("cargo {}", cargo_args.join(" ")),
            "exit_status": output.status.code().unwrap_or(-1),
            "duration_ms": duration_ms,
            "stdout_tail": text_tail(&stdout, 4000),
            "stderr_tail": text_tail(&stderr, 4000)
        }));
        if !pass {
            blockers.push(format!(
                "foundation command failed: cargo {}",
                cargo_args.join(" ")
            ));
            break;
        }
    }

    let status = if blockers.is_empty() { "pass" } else { "fail" };
    let report = json!({
        "status": status,
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-foundation",
        "command_argv": args,
        "exit_status": if blockers.is_empty() { 0 } else { 1 },
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "budget_hash": "n/a",
        "graph_node_count": 0,
        "per_step_pass_fail": checks,
        "artifact_sha256s": [],
        "foundation": {
            "duration_ms": started.elapsed().as_millis() as u64,
            "parser_gate": "cargo test -p boon_parser",
            "ir_gate": "cargo test -p boon_ir",
            "runtime_gate": "cargo test -p boon_runtime",
            "workspace_gate": "cargo test --workspace",
            "blockers": blockers.clone()
        }
    });
    write_json(&report_path, &report)?;
    if blockers.is_empty() {
        verify_report_schema(&report_path)?;
        Ok(())
    } else {
        Err(format!(
            "foundation blockers written to `{}`: {}",
            report_path.display(),
            blockers.join("; ")
        )
        .into())
    }
}

fn text_tail(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_owned();
    }
    text.chars().skip(char_count - max_chars).collect()
}

fn verify_playground_launch(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let aggregate_path =
        report_arg(args).unwrap_or_else(|| PathBuf::from("target/reports/playground-launch.json"));
    let mut reports = Vec::new();
    for example in ["todomvc", "cells"] {
        let report = PathBuf::from(format!("target/reports/playground-launch-{example}.json"));
        let mut command = Command::new("cargo");
        command.args([
            "run",
            "--release",
            "-p",
            "boon_ply_playground",
            "--",
            "--smoke-launch",
            "--example",
            example,
            "--report",
            report.to_str().ok_or("launch report path is not utf-8")?,
        ]);
        let status = run_command_with_timeout(&mut command, Duration::from_secs(60))?;
        if !status.success() {
            return Err(format!("playground launch smoke failed for {example}").into());
        }
        verify_report_schema(&report)?;
        reports.push(report);
    }
    let checks = reports
        .iter()
        .map(|path| {
            let report = read_json(path).unwrap_or_else(|_| json!({}));
            json!({
                "id": format!(
                    "{}-launch-smoke",
                    report
                        .get("example")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown")
                ),
                "pass": true,
                "report": path,
                "frames_drawn": report.get("frames_drawn").cloned().unwrap_or_else(|| json!(null)),
                "window_backend": report.get("window_backend").cloned().unwrap_or_else(|| json!(null))
            })
        })
        .collect::<Vec<_>>();
    let aggregate = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-playground-launch",
        "command_argv": args,
        "exit_status": 0,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "budget_hash": "n/a",
        "graph_node_count": "see launch smoke reports",
        "per_step_pass_fail": checks,
        "artifact_sha256s": reports.iter().map(|path| json!({
            "path": path,
            "sha256": boon_runtime::sha256_file(path).unwrap_or_else(|_| "missing".to_owned())
        })).collect::<Vec<_>>(),
        "launch_reports": reports,
        "note": "bounded native Ply launch smoke for TodoMVC and Cells; does not replace headed OS-input or human verification"
    });
    write_json(&aggregate_path, &aggregate)?;
    verify_report_schema(&aggregate_path)?;
    Ok(())
}

fn verify_playground_background_launch(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let aggregate_path = report_arg(args)
        .unwrap_or_else(|| PathBuf::from("target/reports/playground-background-launch.json"));
    let mut reports = Vec::new();
    let mut checks = Vec::new();
    let mut launcher_outputs = Vec::new();
    for example in ["todomvc", "cells"] {
        let report = PathBuf::from(format!(
            "target/reports/playground-background-launch-{example}.json"
        ));
        let screenshot = report.with_extension("png");
        let _ = std::fs::remove_file(&report);
        let _ = std::fs::remove_file(&screenshot);
        let launched_after = SystemTime::now();
        let output = Command::new("cosmic-background-launch")
            .args([
                "--workspace",
                "boon-circuit",
                "--",
                "cargo",
                "run",
                "-p",
                "boon_ply_playground",
                "--",
                "--smoke-launch",
                "--example",
                example,
                "--frames",
                "3",
                "--report",
                report
                    .to_str()
                    .ok_or("background report path is not utf-8")?,
            ])
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "cosmic background launch failed for {example}: {}",
                text_tail(&String::from_utf8_lossy(&output.stderr), 1200)
            )
            .into());
        }
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let mut stdout_parts = stdout.split_whitespace();
        let launched_pid = stdout_parts
            .next()
            .and_then(|pid| pid.parse::<u32>().ok())
            .ok_or_else(|| {
                format!(
                    "cosmic background launch for {example} did not print a child pid: `{stdout}`"
                )
            })?;
        let launch_id = stdout_parts.next().ok_or_else(|| {
            format!("cosmic background launch for {example} did not print a launch id: `{stdout}`")
        })?;
        if !launch_id.starts_with("background-launch-") {
            return Err(format!(
                "cosmic background launch for {example} printed unexpected launch id `{launch_id}`"
            )
            .into());
        }
        wait_for_fresh_report(&report, launched_after, Duration::from_secs(60))?;
        verify_report_schema(&report)?;
        wait_for_pid_exit(launched_pid, Duration::from_secs(30))?;
        let child_report = read_json(&report)?;
        let frames_drawn = child_report
            .get("frames_drawn")
            .cloned()
            .unwrap_or_else(|| json!(null));
        launcher_outputs.push(json!({
            "example": example,
            "stdout": stdout,
            "stderr_tail": text_tail(&String::from_utf8_lossy(&output.stderr), 1200),
            "child_pid": launched_pid,
            "launch_id": launch_id,
            "report": report,
            "screenshot": screenshot
        }));
        checks.push(json!({
            "id": format!("{example}-background-launch-smoke"),
            "pass": true,
            "child_pid": launched_pid,
            "launch_id": launch_id,
            "report": report,
            "frames_drawn": frames_drawn,
            "process_exited_after_report": true
        }));
        reports.push(report);
    }
    let aggregate = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-playground-background-launch",
        "command_argv": args,
        "exit_status": 0,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "budget_hash": "n/a",
        "graph_node_count": "see background launch smoke reports",
        "per_step_pass_fail": checks,
        "artifact_sha256s": reports.iter().map(|path| json!({
            "path": path,
            "sha256": boon_runtime::sha256_file(path).unwrap_or_else(|_| "missing".to_owned())
        })).collect::<Vec<_>>(),
        "background_launcher": "cosmic-background-launch",
        "background_workspace": "boon-circuit",
        "launch_outputs": launcher_outputs,
        "child_pids": launcher_outputs.iter().filter_map(|entry| entry.get("child_pid").cloned()).collect::<Vec<_>>(),
        "launch_ids": launcher_outputs.iter().filter_map(|entry| entry.get("launch_id").cloned()).collect::<Vec<_>>(),
        "launch_reports": reports,
        "note": "bounded COSMIC background launch smoke; proves startup/rendering without stealing initial focus, not full headed OS-input or human verification"
    });
    write_json(&aggregate_path, &aggregate)?;
    verify_report_schema(&aggregate_path)?;
    Ok(())
}

fn wait_for_fresh_report(
    report: &Path,
    launched_after: SystemTime,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    loop {
        if let Ok(metadata) = std::fs::metadata(report) {
            if metadata.len() > 0
                && metadata
                    .modified()
                    .map(|modified| modified >= launched_after)
                    .unwrap_or(false)
            {
                return Ok(());
            }
        }
        if started.elapsed() > timeout {
            return Err(format!(
                "timed out after {}s waiting for fresh background report `{}`",
                timeout.as_secs(),
                report.display()
            )
            .into());
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn wait_for_pid_exit(pid: u32, timeout: Duration) -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let proc_path = PathBuf::from(format!("/proc/{pid}"));
    while proc_path.exists() {
        if started.elapsed() > timeout {
            return Err(format!(
                "timed out after {}s waiting for background child pid {pid} to exit",
                timeout.as_secs()
            )
            .into());
        }
        thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}

fn verify_playground_custom_source(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let report_path = report_arg(args)
        .unwrap_or_else(|| PathBuf::from("target/reports/playground-custom-source.json"));
    let artifact_dir = PathBuf::from("target/reports/artifacts/playground-custom-source");
    std::fs::create_dir_all(&artifact_dir)?;

    let source_artifact = artifact_dir.join("custom-todomvc.bn");
    let scenario_artifact = artifact_dir.join("custom-todomvc.scn");
    let cells_source_artifact = artifact_dir.join("custom-cells.bn");
    let cells_scenario_artifact = artifact_dir.join("custom-cells.scn");
    let custom_source = std::fs::read_to_string("examples/todomvc.bn")?
        .replace("Buy groceries", "Custom source item A")
        .replace("Clean room", "Custom source item B");
    let custom_scenario = std::fs::read_to_string("examples/todomvc.scn")?
        .replace(
            "source = \"examples/todomvc.bn\"",
            &format!("source = \"{}\"", source_artifact.display()),
        )
        .replace("Buy groceries", "Custom source item A")
        .replace("Clean room", "Custom source item B");
    std::fs::write(&source_artifact, &custom_source)?;
    std::fs::write(&scenario_artifact, &custom_scenario)?;
    let custom_cells_source = std::fs::read_to_string("examples/cells.bn")?
        .replace("columns: 26, rows: 100", "columns: 3, rows: 4");
    let custom_cells_scenario = std::fs::read_to_string("examples/cells.scn")?.replace(
        "source = \"examples/cells.bn\"",
        &format!("source = \"{}\"", cells_source_artifact.display()),
    );
    std::fs::write(&cells_source_artifact, &custom_cells_source)?;
    std::fs::write(&cells_scenario_artifact, &custom_cells_scenario)?;

    let output = run_scenario_source_with_step_limit(
        source_artifact
            .to_str()
            .ok_or("custom source artifact path is not utf-8")?,
        &custom_source,
        &scenario_artifact,
        VerificationLayer::Semantic,
        Some(3),
    )?;
    let original_scenario_error = run_scenario_source_with_step_limit(
        source_artifact
            .to_str()
            .ok_or("custom source artifact path is not utf-8")?,
        &custom_source,
        Path::new("examples/todomvc.scn"),
        VerificationLayer::Semantic,
        Some(1),
    )
    .err()
    .map(|error| error.to_string())
    .ok_or("custom source unexpectedly passed the original scenario initial-state assertions")?;
    let cells_output = run_scenario_source_with_step_limit(
        cells_source_artifact
            .to_str()
            .ok_or("custom Cells source artifact path is not utf-8")?,
        &custom_cells_source,
        &cells_scenario_artifact,
        VerificationLayer::Semantic,
        Some(3),
    )?;
    let original_cells_scenario_error = run_scenario_source_with_step_limit(
        cells_source_artifact
            .to_str()
            .ok_or("custom Cells source artifact path is not utf-8")?,
        &custom_cells_source,
        Path::new("examples/cells.scn"),
        VerificationLayer::Semantic,
        None,
    )
    .err()
    .map(|error| error.to_string())
    .ok_or("custom Cells source unexpectedly passed the original full scenario")?;

    let mut report = output.report;
    let actual_titles_after_submit = report["state_summary"]["todos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|todo| todo.get("title"))
        .cloned()
        .collect::<Vec<_>>();
    let object = report
        .as_object_mut()
        .ok_or("custom-source report is not an object")?;
    object.insert(
        "command".to_owned(),
        json!("verify-playground-custom-source"),
    );
    object.insert(
        "report_path".to_owned(),
        json!(report_path.display().to_string()),
    );
    object.insert(
        "artifact_sha256s".to_owned(),
        json!([
            {
                "path": source_artifact.display().to_string(),
                "sha256": boon_runtime::sha256_file(&source_artifact)?
            },
            {
                "path": scenario_artifact.display().to_string(),
                "sha256": boon_runtime::sha256_file(&scenario_artifact)?
            },
            {
                "path": cells_source_artifact.display().to_string(),
                "sha256": boon_runtime::sha256_file(&cells_source_artifact)?
            },
            {
                "path": cells_scenario_artifact.display().to_string(),
                "sha256": boon_runtime::sha256_file(&cells_scenario_artifact)?
            }
        ]),
    );
    object.insert(
        "playground_custom_source".to_owned(),
        json!({
            "input_surface": "playground editor source text",
            "custom_source_text_was_interpreted": true,
            "custom_scenario_was_interpreted": true,
            "source_text_artifact_path": source_artifact.display().to_string(),
            "scenario_artifact_path": scenario_artifact.display().to_string(),
            "source_diff_summary": [
                "initial todo title Buy groceries -> Custom source item A",
                "initial todo title Clean room -> Custom source item B"
            ],
            "behavior_probe": {
                "step_limit": 3,
                "expected_titles_after_submit": [
                    "Custom source item A",
                    "Custom source item B",
                    "Test todo"
                ],
                "actual_titles_after_submit": actual_titles_after_submit
            },
            "original_scenario_rejected_custom_initial_state": true,
            "original_scenario_rejection": original_scenario_error,
            "custom_examples": [
                {
                    "example": "todomvc",
                    "custom_source_text_was_interpreted": true,
                    "custom_scenario_was_interpreted": true,
                    "source_text_artifact_path": source_artifact.display().to_string(),
                    "scenario_artifact_path": scenario_artifact.display().to_string(),
                    "source_hash_differs_from_bundled_example": boon_runtime::sha256_file(&source_artifact)? != file_hash("examples/todomvc.bn"),
                    "behavior_probe": {
                        "step_limit": 3,
                        "expected_titles_after_submit": [
                            "Custom source item A",
                            "Custom source item B",
                            "Test todo"
                        ],
                        "actual_titles_after_submit": actual_titles_after_submit
                    },
                    "original_scenario_rejected_custom_initial_state": true,
                    "original_scenario_rejection": original_scenario_error
                },
                {
                    "example": "cells",
                    "custom_source_text_was_interpreted": true,
                    "custom_scenario_was_interpreted": true,
                    "source_text_artifact_path": cells_source_artifact.display().to_string(),
                    "scenario_artifact_path": cells_scenario_artifact.display().to_string(),
                    "source_hash_differs_from_bundled_example": boon_runtime::sha256_file(&cells_source_artifact)? != file_hash("examples/cells.bn"),
                    "behavior_probe": {
                        "step_limit": 3,
                        "expected_grid_dimensions": {"columns": 3, "rows": 4},
                        "actual_grid_initializer": cells_output.report["ir_debug_tables"]["lists"][0]["initializer"].clone(),
                        "a1_value_after_commit": cells_output.state_summary["cells"][0]["value"].clone()
                    },
                    "original_full_scenario_rejected_custom_grid_shape": true,
                    "original_full_scenario_rejection": original_cells_scenario_error
                }
            ]
        }),
    );
    write_json(&report_path, &report)?;
    verify_report_schema(&report_path)?;
    Ok(())
}

fn write_manual_handoff(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let report_path =
        report_arg(args).unwrap_or_else(|| PathBuf::from("target/reports/manual-handoff.json"));
    let blockers = current_handoff_blockers();
    let runbook_path = PathBuf::from("docs/plans/MANUAL_TESTING_RUNBOOK.md");
    let todomvc_template = manual_template_path("todomvc");
    let cells_template = manual_template_path("cells");
    let handoff_artifacts = [
        artifact_hash(&runbook_path)?,
        artifact_hash(&todomvc_template)?,
        artifact_hash(&cells_template)?,
    ];
    let report = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "write-manual-handoff",
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
            {"id": "manual-runbook-present", "pass": Path::new("docs/plans/MANUAL_TESTING_RUNBOOK.md").exists()},
            {"id": "todomvc-template-present", "pass": manual_template_path("todomvc").exists()},
            {"id": "cells-template-present", "pass": manual_template_path("cells").exists()},
            {"id": "prepare-todomvc-human-report-command", "pass": true},
            {"id": "prepare-cells-human-report-command", "pass": true},
            {"id": "benchmark-commands", "pass": true},
            {"id": "background-launch-smoke-commands", "pass": true},
            {"id": "background-launch-smoke-report-command", "pass": true},
            {"id": "aggregate-check-existing-commands", "pass": true}
        ],
        "artifact_sha256s": handoff_artifacts,
        "handoff_status": if blockers.is_empty() { "ready_complete" } else { "blocked_on_real_human_reports" },
        "remaining_blockers": blockers,
        "runbook": "docs/plans/MANUAL_TESTING_RUNBOOK.md",
        "manual_template_paths": [
            todomvc_template.display().to_string(),
            cells_template.display().to_string()
        ],
        "background_launch_policy": {
            "acceptable_for": [
                "opening visible playground surfaces without stealing unrelated focus",
                "bounded smoke launches that prove the native surface can render briefly"
            ],
            "not_acceptable_for": [
                "full headed OS-input verification",
                "claiming human keyboard/mouse interaction reached the playground without a recorded focused-window manual session"
            ],
            "required_full_input_route": "directly controlled headed verifier or real human visible-window session"
        },
        "manual_testing_commands": {
            "refresh_automated_baseline": [
                "cargo xtask verify-foundation",
                "cargo xtask verify-playground-launch",
                "cargo xtask verify-playground-custom-source",
                "cargo xtask verify-os-input-probe --report target/reports/os-input-probe.json",
                "BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-todomvc-headed-ply",
                "BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-cells-headed-ply",
                "cargo xtask verify-todomvc-speed",
                "cargo xtask verify-cells-speed",
                "cargo xtask verify-todomvc-negative",
                "cargo xtask verify-cells-negative",
                "cargo bench -p boon_runtime --bench todomvc -- --report target/reports/todomvc-bench.json --speed-report target/reports/todomvc-bench-speed.json",
                "cargo xtask bench-todomvc",
                "cargo xtask bench-example cells",
                "cargo xtask explain-todomvc-hardware --report target/reports/todomvc-hardware.json",
                "cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn --report target/reports/todomvc-cli-run.json",
                "cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn --report target/reports/cells-cli-run.json",
                "cargo xtask verify-report-schema"
            ],
            "write_templates": [
                "cargo xtask verify-todomvc-human --write-template --report target/reports/manual-templates/todomvc-human.json",
                "cargo xtask verify-cells-human --write-template --report target/reports/manual-templates/cells-human.json"
            ],
            "launch_playgrounds": [
                "cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --example todomvc",
                "cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --example cells"
            ],
            "background_launch_smoke": [
                "cargo xtask verify-playground-background-launch --report target/reports/playground-background-launch.json",
                "cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --smoke-launch --example todomvc --frames 3 --report target/reports/playground-background-launch-todomvc.json",
                "cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --smoke-launch --example cells --frames 3 --report target/reports/playground-background-launch-cells.json"
            ],
            "prepare_human_reports": [
                "cargo xtask prepare-todomvc-human-report --observer <real-name> --started <unix-start> --finished <unix-finish> --window-pid <visible-playground-pid> --focused-window-proof <how-focus-was-confirmed> --notes <visual-notes> --capture-method <tool-used> --artifact <manual-png-or-video> --pass-label <each-todomvc-scenario-label> --report target/reports/todomvc-human.json",
                "cargo xtask prepare-cells-human-report --observer <real-name> --started <unix-start> --finished <unix-finish> --window-pid <visible-playground-pid> --focused-window-proof <how-focus-was-confirmed> --notes <visual-notes> --capture-method <tool-used> --artifact <manual-png-or-video> --pass-label <each-cells-scenario-label> --report target/reports/cells-human.json"
            ],
            "final_aggregate": [
                "cargo xtask verify-todomvc-all --check-existing --report target/reports/todomvc-all.json",
                "cargo xtask verify-cells-all --check-existing --report target/reports/cells-all.json",
                "cargo xtask verify-examples-all --check-existing --report target/reports/examples-all.json",
                "cargo xtask audit-manual-readiness --report target/reports/debug/manual-readiness.json",
                "cargo xtask audit-goal-readiness --report target/reports/debug/goal-readiness.json"
            ]
        }
    });
    write_json(&report_path, &report)?;
    verify_report_schema(&report_path)?;
    Ok(())
}

fn current_handoff_blockers() -> Vec<String> {
    let mut blockers = Vec::new();
    for name in ["todomvc", "cells"] {
        let human = report_path(name, VerificationLayer::Human);
        if verify_human_report(&human, 24 * 60 * 60).is_err() {
            blockers.push(format!(
                "missing fresh real human report `{}`",
                human.display()
            ));
        }
        let all = report_path(name, VerificationLayer::All);
        if !all.exists() {
            blockers.push(format!("missing aggregate report `{}`", all.display()));
        }
    }
    blockers
}

fn audit_goal_readiness(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let report_path = report_arg(args)
        .unwrap_or_else(|| PathBuf::from("target/reports/debug/goal-readiness.json"));
    let command_name = args
        .first()
        .map(String::as_str)
        .unwrap_or("audit-goal-readiness");
    let mut checks = Vec::new();
    let mut blockers = Vec::new();

    audit_top_level_report_schema(&mut checks, &mut blockers)?;
    audit_recursive_report_schema_summary(&mut checks, &mut blockers)?;
    audit_debug_blocked_reports(&mut checks, &mut blockers)?;
    audit_foundation(&mut checks, &mut blockers)?;
    audit_playground_launch(&mut checks, &mut blockers)?;
    audit_playground_background_launch(&mut checks, &mut blockers)?;
    audit_example_source_contracts(&mut checks, &mut blockers)?;
    audit_scenario_coverage(&mut checks, &mut blockers)?;
    audit_cli_scenario_reports(&mut checks, &mut blockers)?;
    for name in ["todomvc", "cells"] {
        audit_example_readiness(name, &mut checks, &mut blockers)?;
    }
    audit_examples_all_report(&mut checks, &mut blockers)?;
    audit_benchmark_reports(&mut checks, &mut blockers)?;
    audit_todomvc_hardware_plan(&mut checks, &mut blockers)?;
    audit_playground_custom_source(&mut checks, &mut blockers)?;
    audit_manual_handoff(&mut checks, &mut blockers)?;
    audit_repo_handoff_docs(&mut checks, &mut blockers)?;
    audit_scope_control(&mut checks, &mut blockers)?;
    audit_xtask_command_surface(&mut checks, &mut blockers);

    let status = if blockers.is_empty() { "pass" } else { "fail" };
    let report = json!({
        "status": status,
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": command_name,
        "command_argv": args,
        "exit_status": if blockers.is_empty() { 0 } else { 1 },
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "budget_hash": "n/a",
        "graph_node_count": 0,
        "per_step_pass_fail": checks,
        "blockers": blockers,
        "artifact_sha256s": []
    });
    write_json(&report_path, &report)?;

    let blockers = report
        .get("blockers")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if blockers.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{command_name} blockers written to `{}`: {}",
            report_path.display(),
            blockers.join("; ")
        )
        .into())
    }
}

fn audit_top_level_report_schema(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new("target/reports");
    if !dir.exists() {
        push_audit_check(
            checks,
            blockers,
            "top-level-report-schema",
            false,
            "target/reports is missing",
            Some("target/reports is missing; run the verification commands first".to_owned()),
        );
        return Ok(());
    }
    let mut checked = 0usize;
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        checked += 1;
        let id = format!("report-schema:{}", path.display());
        match verify_report_schema(&path) {
            Ok(()) => push_audit_check(checks, blockers, &id, true, "schema valid", None),
            Err(error) => push_audit_check(
                checks,
                blockers,
                &id,
                false,
                error.to_string(),
                Some(format!("{} is not schema-valid: {error}", path.display())),
            ),
        }
    }
    push_audit_check(
        checks,
        blockers,
        "top-level-report-schema-count",
        checked > 0,
        format!("checked {checked} top-level JSON reports"),
        (checked == 0).then(|| "no top-level target/reports/*.json reports exist".to_owned()),
    );
    Ok(())
}

fn audit_recursive_report_schema_summary(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new("target/reports");
    let summary_path = dir.join("schema.json");
    if !summary_path.exists() {
        push_audit_check(
            checks,
            blockers,
            "recursive-report-schema-summary:present",
            false,
            format!("missing {}", summary_path.display()),
            Some(format!(
                "missing recursive schema summary `{}`; run `cargo xtask verify-report-schema`",
                summary_path.display()
            )),
        );
        return Ok(());
    }
    match verify_report_schema(&summary_path) {
        Ok(()) => push_audit_check(
            checks,
            blockers,
            "recursive-report-schema-summary:schema",
            true,
            format!("{} schema valid", summary_path.display()),
            None,
        ),
        Err(error) => {
            push_audit_check(
                checks,
                blockers,
                "recursive-report-schema-summary:schema",
                false,
                error.to_string(),
                Some(format!(
                    "recursive schema summary `{}` is not schema-valid: {error}",
                    summary_path.display()
                )),
            );
            return Ok(());
        }
    }

    let summary = read_json(&summary_path)?;
    let readiness_path = dir.join("debug/goal-readiness.json");
    let report_paths = collect_report_json_paths(dir)?;
    let expected_seen = report_paths
        .iter()
        .filter(|path| *path != &summary_path)
        .count() as u64;
    let expected_artifact_paths = report_paths
        .iter()
        .filter(|path| *path != &summary_path && *path != &readiness_path)
        .map(|path| path.display().to_string())
        .collect::<BTreeSet<_>>();
    let seen_count = report_check_count(&summary, "report-json-files-seen-recursively");
    push_audit_check(
        checks,
        blockers,
        "recursive-report-schema-summary:seen-count-current",
        seen_count == Some(expected_seen),
        format!("schema summary seen_count={seen_count:?}, current_json_count={expected_seen}"),
        (seen_count != Some(expected_seen)).then(|| {
            format!(
                "recursive schema summary `{}` is stale; run `cargo xtask verify-report-schema`",
                summary_path.display()
            )
        }),
    );
    let artifact_paths = report_artifact_paths(&summary);
    let artifact_count_matches = artifact_paths.len() == expected_artifact_paths.len();
    push_audit_check(
        checks,
        blockers,
        "recursive-report-schema-summary:artifact-hash-count",
        artifact_count_matches,
        format!(
            "schema artifact hashes={}, expected={}",
            artifact_paths.len(),
            expected_artifact_paths.len()
        ),
        (!artifact_count_matches).then(|| {
            format!(
                "recursive schema summary `{}` does not hash the expected report artifact count",
                summary_path.display()
            )
        }),
    );
    let missing_artifact_paths = expected_artifact_paths
        .difference(&artifact_paths)
        .cloned()
        .collect::<Vec<_>>();
    push_audit_check(
        checks,
        blockers,
        "recursive-report-schema-summary:artifact-hash-path-coverage",
        missing_artifact_paths.is_empty(),
        format!(
            "schema artifact hash path coverage missing={}",
            missing_artifact_paths.len()
        ),
        (!missing_artifact_paths.is_empty()).then(|| {
            format!(
                "recursive schema summary `{}` is missing artifact hashes for: {}",
                summary_path.display(),
                missing_artifact_paths.join(", ")
            )
        }),
    );
    let readiness_hashed = artifact_paths.contains(&readiness_path.display().to_string());
    push_audit_check(
        checks,
        blockers,
        "recursive-report-schema-summary:goal-readiness-excluded-from-artifacts",
        !readiness_hashed,
        "goal-readiness report is intentionally excluded from schema artifact hashes".to_owned(),
        readiness_hashed.then(|| {
            format!(
                "recursive schema summary `{}` hashes self-mutating readiness report `{}`",
                summary_path.display(),
                readiness_path.display()
            )
        }),
    );

    for id in [
        "full-pass-reports-schema-checked",
        "debug-failure-artifacts-accounted",
        "manual-template-artifacts-accounted",
        "debug-dump-artifacts-accounted",
        "debug-auxiliary-artifacts-accounted",
    ] {
        let count = report_check_count(&summary, id);
        push_audit_check(
            checks,
            blockers,
            format!("recursive-report-schema-summary:{id}"),
            count.is_some(),
            format!("{id} count={count:?}"),
            count.is_none().then(|| {
                format!(
                    "recursive schema summary `{}` is missing `{id}` evidence",
                    summary_path.display()
                )
            }),
        );
    }
    Ok(())
}

fn report_check_count(report: &serde_json::Value, id: &str) -> Option<u64> {
    report
        .get("per_step_pass_fail")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .find(|check| check.get("id").and_then(serde_json::Value::as_str) == Some(id))?
        .get("count")
        .and_then(serde_json::Value::as_u64)
}

fn report_artifact_paths(report: &serde_json::Value) -> BTreeSet<String> {
    report
        .get("artifact_sha256s")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artifact| artifact.get("path").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect()
}

fn report_artifact_hash_matches(report: &serde_json::Value, path: &Path, hash: &str) -> bool {
    let path = path.display().to_string();
    report
        .get("artifact_sha256s")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|artifacts| {
            artifacts.iter().any(|artifact| {
                artifact.get("path").and_then(serde_json::Value::as_str) == Some(path.as_str())
                    && artifact.get("sha256").and_then(serde_json::Value::as_str) == Some(hash)
            })
        })
}

fn report_artifact_hash_entries_current(
    report: &serde_json::Value,
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(artifacts) = report
        .get("artifact_sha256s")
        .and_then(serde_json::Value::as_array)
    else {
        return Ok(false);
    };
    if artifacts.is_empty() {
        return Ok(false);
    }
    for artifact in artifacts {
        let Some(path) = artifact.get("path").and_then(serde_json::Value::as_str) else {
            return Ok(false);
        };
        let Some(expected) = artifact.get("sha256").and_then(serde_json::Value::as_str) else {
            return Ok(false);
        };
        let actual = boon_runtime::sha256_file(Path::new(path))?;
        if actual != expected {
            return Ok(false);
        }
    }
    Ok(true)
}

fn blocked_report_completed_artifacts_hashed(report: &serde_json::Value, field: &str) -> bool {
    let artifact_paths = report_artifact_paths(report);
    match report.get(field) {
        Some(serde_json::Value::Array(paths)) => {
            !paths.is_empty()
                && paths.iter().all(|path| {
                    path.as_str()
                        .is_some_and(|path| artifact_paths.contains(path))
                })
        }
        Some(serde_json::Value::String(path)) => artifact_paths.contains(path),
        _ => false,
    }
}

fn audit_debug_blocked_reports(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new("target/reports/debug");
    if !dir.exists() {
        push_audit_check(
            checks,
            blockers,
            "debug-blocked-reports:none-present",
            true,
            "no debug blocked reports exist",
            None,
        );
        return Ok(());
    }
    let mut checked = 0usize;
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.extension().and_then(|ext| ext.to_str()) != Some("json")
            || !name.ends_with("-blocked.json")
        {
            continue;
        }
        checked += 1;
        let report = read_json(&path)?;
        let status_fail = report.get("status").and_then(serde_json::Value::as_str) == Some("fail");
        let exit_nonzero = report
            .get("exit_status")
            .and_then(serde_json::Value::as_i64)
            .is_some_and(|status| status != 0);
        let command_blocked = report
            .get("command")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|command| command.ends_with("-blocked"));
        let has_failing_check = report
            .get("per_step_pass_fail")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|checks| {
                checks.iter().any(|check| {
                    check.get("pass").and_then(serde_json::Value::as_bool) == Some(false)
                })
            });
        let has_blocker_text = report
            .get("blocker")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|blocker| !blocker.trim().is_empty());
        let note_marks_debug_only = report
            .get("note")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|note| note.contains("debug-only"));
        let artifact_hashes_current = report_artifact_hash_entries_current(&report)?;
        let completed_reports_hashed =
            blocked_report_completed_artifacts_hashed(&report, "completed_layer_reports");
        let blocked_example_report_hashed =
            blocked_report_completed_artifacts_hashed(&report, "blocked_example_debug_report");
        let pass = status_fail
            && exit_nonzero
            && command_blocked
            && has_failing_check
            && has_blocker_text
            && note_marks_debug_only
            && artifact_hashes_current
            && (completed_reports_hashed || blocked_example_report_hashed);
        push_audit_check(
            checks,
            blockers,
            format!("debug-blocked-report:{}", path.display()),
            pass,
            format!(
                "status_fail={status_fail}, exit_nonzero={exit_nonzero}, command_blocked={command_blocked}, has_failing_check={has_failing_check}, has_blocker_text={has_blocker_text}, note_marks_debug_only={note_marks_debug_only}, artifact_hashes_current={artifact_hashes_current}, completed_reports_hashed={completed_reports_hashed}, blocked_example_report_hashed={blocked_example_report_hashed}"
            ),
            (!pass).then(|| {
                format!(
                    "debug blocked report `{}` is missing fail/nonzero/debug-only blocker shape or artifact hash bindings",
                    path.display()
                )
            }),
        );
    }
    push_audit_check(
        checks,
        blockers,
        "debug-blocked-reports:checked-count",
        true,
        format!("checked {checked} debug blocked reports"),
        None,
    );
    Ok(())
}

fn audit_example_readiness(
    name: &str,
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let required_layers = [
        VerificationLayer::Semantic,
        VerificationLayer::HeadlessPly,
        VerificationLayer::HeadedPly,
        VerificationLayer::Speed,
        VerificationLayer::Negative,
        VerificationLayer::All,
    ];
    for layer in required_layers {
        let report = report_path(name, layer);
        if !report.exists() {
            push_audit_check(
                checks,
                blockers,
                format!("{name}:{}:report-present", layer.as_str()),
                false,
                format!("missing {}", report.display()),
                Some(format!(
                    "{name} missing {} report `{}`",
                    layer.as_str(),
                    report.display()
                )),
            );
            continue;
        }

        match verify_report_schema(&report) {
            Ok(()) => push_audit_check(
                checks,
                blockers,
                format!("{name}:{}:schema", layer.as_str()),
                true,
                format!("{} schema valid", report.display()),
                None,
            ),
            Err(error) => {
                push_audit_check(
                    checks,
                    blockers,
                    format!("{name}:{}:schema", layer.as_str()),
                    false,
                    error.to_string(),
                    Some(format!(
                        "{name} {} report `{}` is not schema-valid: {error}",
                        layer.as_str(),
                        report.display()
                    )),
                );
                continue;
            }
        }

        let report_json = read_json(&report)?;
        if matches!(
            layer,
            VerificationLayer::Semantic
                | VerificationLayer::HeadlessPly
                | VerificationLayer::HeadedPly
                | VerificationLayer::Speed
        ) {
            audit_runtime_execution(name, layer, &report, &report_json, checks, blockers);
        }
        if matches!(layer, VerificationLayer::HeadedPly) {
            audit_headed_input(name, &report, &report_json, checks, blockers)?;
            audit_playground_surface(name, &report, &report_json, checks, blockers);
        }
        if matches!(layer, VerificationLayer::Negative) {
            audit_negative_report_contract(name, &report, &report_json, checks, blockers);
        }
    }

    audit_manual_template_readiness(name, checks, blockers)?;

    let human_report = report_path(name, VerificationLayer::Human);
    match verify_human_report(&human_report, 24 * 60 * 60) {
        Ok(()) => push_audit_check(
            checks,
            blockers,
            format!("{name}:human:fresh-real-report"),
            true,
            format!("{} is a fresh checked human report", human_report.display()),
            None,
        ),
        Err(error) => push_audit_check(
            checks,
            blockers,
            format!("{name}:human:fresh-real-report"),
            false,
            error.to_string(),
            Some(format!(
                "{name} missing fresh real human report `{}`: {error}",
                human_report.display()
            )),
        ),
    }
    Ok(())
}

fn audit_examples_all_report(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let example_reports = [
        PathBuf::from("target/reports/todomvc-all.json"),
        PathBuf::from("target/reports/cells-all.json"),
    ];
    if example_reports.iter().any(|path| !path.exists()) {
        push_audit_check(
            checks,
            blockers,
            "examples-all:deferred-until-example-all-reports",
            true,
            "examples-all aggregate is checked after todomvc-all and cells-all exist",
            None,
        );
        return Ok(());
    }
    let path = PathBuf::from("target/reports/examples-all.json");
    if !path.exists() {
        push_audit_check(
            checks,
            blockers,
            "examples-all:report-present",
            false,
            format!("missing {}", path.display()),
            Some(format!(
                "missing examples aggregate report `{}`; run `cargo xtask verify-examples-all --check-existing --report target/reports/examples-all.json`",
                path.display()
            )),
        );
        return Ok(());
    }
    match verify_report_schema(&path) {
        Ok(()) => push_audit_check(
            checks,
            blockers,
            "examples-all:schema",
            true,
            format!("{} schema valid", path.display()),
            None,
        ),
        Err(error) => {
            push_audit_check(
                checks,
                blockers,
                "examples-all:schema",
                false,
                error.to_string(),
                Some(format!(
                    "examples aggregate report `{}` is not schema-valid: {error}",
                    path.display()
                )),
            );
            return Ok(());
        }
    }
    let report = read_json(&path)?;
    for example in ["todomvc", "cells"] {
        let expected = format!("target/reports/{example}-all.json");
        let linked = report
            .get("example_all_reports")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|reports| {
                reports
                    .iter()
                    .any(|report| report.as_str() == Some(expected.as_str()))
            });
        push_audit_check(
            checks,
            blockers,
            format!("examples-all:links-{example}"),
            linked,
            format!("{} links {expected}", path.display()),
            (!linked).then(|| {
                format!(
                    "examples aggregate report `{}` does not link `{expected}`",
                    path.display()
                )
            }),
        );
    }
    Ok(())
}

fn audit_benchmark_reports(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    for name in ["todomvc", "cells"] {
        let path = PathBuf::from(format!("target/reports/{name}-bench.json"));
        if !path.exists() {
            let command = if name == "todomvc" {
                "cargo xtask bench-todomvc"
            } else {
                "cargo xtask bench-example cells"
            };
            push_audit_check(
                checks,
                blockers,
                format!("{name}:bench:report-present"),
                false,
                format!("missing {}", path.display()),
                Some(format!(
                    "missing {name} benchmark report `{}`; run `{command}`",
                    path.display()
                )),
            );
            continue;
        }

        match verify_report_schema(&path) {
            Ok(()) => push_audit_check(
                checks,
                blockers,
                format!("{name}:bench:schema"),
                true,
                format!("{} schema valid", path.display()),
                None,
            ),
            Err(error) => {
                push_audit_check(
                    checks,
                    blockers,
                    format!("{name}:bench:schema"),
                    false,
                    error.to_string(),
                    Some(format!(
                        "{name} benchmark report `{}` is not schema-valid: {error}",
                        path.display()
                    )),
                );
                continue;
            }
        }

        let report = read_json(&path)?;
        let expected_source = format!("examples/{name}.bn");
        let expected_scenario = format!("examples/{name}.scn");
        let expected_budget = format!("examples/{name}.budget.toml");
        let source_hash = file_hash(&expected_source);
        let scenario_hash = file_hash(&expected_scenario);
        let budget_hash = file_hash(&expected_budget);
        let expected_command = if name == "todomvc" {
            "bench-todomvc"
        } else {
            "bench-example"
        };
        let command_matches =
            report.get("command").and_then(serde_json::Value::as_str) == Some(expected_command);
        push_audit_check(
            checks,
            blockers,
            format!("{name}:bench:command"),
            command_matches,
            format!("benchmark command is {:?}", report.get("command")),
            (!command_matches).then(|| {
                format!(
                    "{name} benchmark report `{}` does not prove `{expected_command}`",
                    path.display()
                )
            }),
        );
        let report_hashes_current = report
            .get("source_path")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|path| path == expected_source)
            && report
                .get("scenario_path")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|path| path == expected_scenario)
            && report
                .get("source_hash")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|hash| hash == source_hash)
            && report
                .get("scenario_hash")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|hash| hash == scenario_hash)
            && report
                .get("budget_hash")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|hash| hash == budget_hash);
        push_audit_check(
            checks,
            blockers,
            format!("{name}:bench:current-source-scenario-budget"),
            report_hashes_current,
            "benchmark source/scenario/budget hashes match current files".to_owned(),
            (!report_hashes_current).then(|| {
                format!(
                    "{name} benchmark report `{}` is stale for current source/scenario/budget files",
                    path.display()
                )
            }),
        );
        let iterations = report
            .get("benchmark")
            .and_then(|benchmark| benchmark.get("iterations"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        push_audit_check(
            checks,
            blockers,
            format!("{name}:bench:iterations"),
            iterations > 0,
            format!("benchmark iterations: {iterations}"),
            (iterations == 0).then(|| format!("{name} benchmark report has zero iterations")),
        );

        let speed_report_path = report
            .get("benchmark")
            .and_then(|benchmark| benchmark.get("speed_report_path"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("target/reports/{name}-bench-speed.json"));
        let speed_report = Path::new(&speed_report_path);
        let speed_report_valid =
            speed_report.exists() && verify_report_schema(speed_report).is_ok();
        push_audit_check(
            checks,
            blockers,
            format!("{name}:bench:speed-artifact-schema"),
            speed_report_valid,
            format!("checked linked speed report {}", speed_report.display()),
            (!speed_report_valid).then(|| {
                format!(
                    "{name} benchmark linked speed report `{}` is missing or invalid",
                    speed_report.display()
                )
            }),
        );
        if speed_report_valid {
            let linked = read_json(speed_report)?;
            let linked_hashes_current = linked
                .get("source_hash")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|hash| hash == source_hash)
                && linked
                    .get("scenario_hash")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|hash| hash == scenario_hash)
                && linked
                    .get("budget_hash")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|hash| hash == budget_hash);
            push_audit_check(
                checks,
                blockers,
                format!("{name}:bench:linked-speed-current"),
                linked_hashes_current,
                format!(
                    "linked speed report {} matches current source/scenario/budget",
                    speed_report.display()
                ),
                (!linked_hashes_current).then(|| {
                    format!(
                        "{name} benchmark linked speed report `{}` is stale for current source/scenario/budget files",
                        speed_report.display()
                    )
                }),
            );
            let linked_hash = boon_runtime::sha256_file(speed_report)?;
            let artifact_hash_matches = report
                .get("artifact_sha256s")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|artifacts| {
                    artifacts.iter().any(|artifact| {
                        artifact.get("path").and_then(serde_json::Value::as_str)
                            == Some(speed_report_path.as_str())
                            && artifact.get("sha256").and_then(serde_json::Value::as_str)
                                == Some(linked_hash.as_str())
                    })
                });
            push_audit_check(
                checks,
                blockers,
                format!("{name}:bench:linked-speed-hash"),
                artifact_hash_matches,
                "benchmark artifact hash matches linked speed report".to_owned(),
                (!artifact_hash_matches).then(|| {
                    format!(
                        "{name} benchmark report `{}` does not hash its linked speed report `{}`",
                        path.display(),
                        speed_report.display()
                    )
                }),
            );
            for field in [
                "budget_check",
                "input_to_idle_ms_p50_p95_p99_max",
                "allocations",
                "graph_rebuild_count",
                "stress_profiles",
            ] {
                let copied_field_matches = report.get(field) == linked.get(field);
                push_audit_check(
                    checks,
                    blockers,
                    format!("{name}:bench:copied-speed-field:{field}"),
                    copied_field_matches,
                    format!("benchmark report copies `{field}` from linked speed report"),
                    (!copied_field_matches).then(|| {
                        format!(
                            "{name} benchmark report `{}` does not copy `{field}` from linked speed report `{}`",
                            path.display(),
                            speed_report.display()
                        )
                    }),
                );
            }
        }
    }
    Ok(())
}

fn audit_manual_template_readiness(
    name: &str,
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let template_path = manual_template_path(name);
    if !template_path.exists() {
        push_audit_check(
            checks,
            blockers,
            format!("{name}:human-template:present"),
            false,
            format!("missing {}", template_path.display()),
            Some(format!(
                "{name} missing manual template `{}`",
                template_path.display()
            )),
        );
        return Ok(());
    }
    let template = read_json(&template_path)?;
    let scenario_path = PathBuf::from(format!("examples/{name}.scn"));
    let scenario = boon_runtime::parse_scenario(&scenario_path)?;
    let expected_labels = scenario
        .step
        .iter()
        .map(|step| step.id.as_str())
        .collect::<Vec<_>>();
    let checklist = template
        .get("manual_checklist_pass_fail")
        .and_then(serde_json::Value::as_object);
    let checklist_ready = checklist.is_some_and(|checklist| {
        checklist.len() == expected_labels.len()
            && expected_labels.iter().all(|label| {
                checklist.get(*label).and_then(serde_json::Value::as_bool) == Some(false)
            })
    });
    push_audit_check(
        checks,
        blockers,
        format!("{name}:human-template:checklist"),
        checklist_ready,
        format!(
            "{} checklist covers scenario labels",
            template_path.display()
        ),
        (!checklist_ready).then(|| {
            format!(
                "{name} manual template `{}` does not cover every scenario label as unchecked",
                template_path.display()
            )
        }),
    );
    let status_ready = template.get("status").and_then(serde_json::Value::as_str)
        == Some("needs_manual")
        && template
            .get("manual_observer")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|observer| observer.contains("fill"))
        && template
            .get("artifact_sha256s")
            .and_then(serde_json::Value::as_array)
            .is_some_and(Vec::is_empty)
        && template
            .get("checkpoint_screenshot_or_video_paths")
            .and_then(serde_json::Value::as_array)
            .is_some_and(Vec::is_empty);
    push_audit_check(
        checks,
        blockers,
        format!("{name}:human-template:needs-manual"),
        status_ready,
        "template is clearly not a passing human report".to_owned(),
        (!status_ready).then(|| {
            format!(
                "{name} manual template `{}` is not an empty needs_manual template",
                template_path.display()
            )
        }),
    );
    let source_hash_ok = template
        .get("source_hash")
        .and_then(serde_json::Value::as_str)
        == Some(file_hash(&format!("examples/{name}.bn")).as_str());
    let scenario_hash_ok = template
        .get("scenario_hash")
        .and_then(serde_json::Value::as_str)
        == Some(file_hash(&format!("examples/{name}.scn")).as_str());
    push_audit_check(
        checks,
        blockers,
        format!("{name}:human-template:source-scenario-hash"),
        source_hash_ok && scenario_hash_ok,
        "template source/scenario hashes match current files".to_owned(),
        (!(source_hash_ok && scenario_hash_ok)).then(|| {
            format!(
                "{name} manual template `{}` has stale source or scenario hash",
                template_path.display()
            )
        }),
    );
    let headed_path = template
        .get("headed_report_path")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let headed_report_path = report_path(name, VerificationLayer::HeadedPly);
    let headed_hash_ok = headed_path == headed_report_path.to_string_lossy()
        && template
            .get("headed_report_sha256")
            .and_then(serde_json::Value::as_str)
            == Some(file_hash(&headed_report_path.to_string_lossy()).as_str())
        && template
            .get("headed_input_injection_method")
            .and_then(serde_json::Value::as_str)
            == Some("os_pointer_keyboard_to_visible_window")
        && template
            .get("headed_os_input_missing_labels")
            .and_then(serde_json::Value::as_array)
            .is_some_and(Vec::is_empty);
    push_audit_check(
        checks,
        blockers,
        format!("{name}:human-template:headed-binding"),
        headed_hash_ok,
        "template is bound to current full-OS-input headed report".to_owned(),
        (!headed_hash_ok).then(|| {
            format!(
                "{name} manual template `{}` is not bound to the current full-OS-input headed report",
                template_path.display()
            )
        }),
    );
    let manual_metadata_template_ok = template
        .get("input_injection_method")
        .and_then(serde_json::Value::as_str)
        == Some("human_visible_window")
        && template
            .get("visual_checkpoint_pass_fail")
            .and_then(serde_json::Value::as_array)
            .is_some_and(Vec::is_empty)
        && template
            .get("window_pid")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|pid| pid.contains("fill-visible"))
        && template
            .get("focused_window_proof")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|proof| proof.contains("fill-visible"))
        && template
            .get("input_backend")
            .and_then(serde_json::Value::as_str)
            == Some("human-visible-window-pointer-keyboard")
        && template
            .get("capture_backend")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|capture| capture.contains("fill-manual"));
    let headed_display_defaults_ok = headed_report_path.exists() && {
        let headed_report = read_json(&headed_report_path)?;
        template.get("display_server") == headed_report.get("display_server")
            && template.get("display_socket_or_compositor_connection")
                == headed_report.get("display_socket_or_compositor_connection")
            && template.get("display_scale") == headed_report.get("display_scale")
            && template.get("window_backend") == headed_report.get("window_backend")
    };
    push_audit_check(
        checks,
        blockers,
        format!("{name}:human-template:manual-visual-metadata"),
        manual_metadata_template_ok && headed_display_defaults_ok,
        "template requires manual visible-window metadata and carries headed display defaults"
            .to_owned(),
        (!(manual_metadata_template_ok && headed_display_defaults_ok)).then(|| {
            format!(
                "{name} manual template `{}` is missing manual visible-window metadata placeholders or headed display defaults",
                template_path.display()
            )
        }),
    );
    Ok(())
}

fn audit_foundation(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from("target/reports/foundation.json");
    if !path.exists() {
        push_audit_check(
            checks,
            blockers,
            "foundation:report-present",
            false,
            format!("missing {}", path.display()),
            Some(format!(
                "missing foundation report `{}`; run `cargo xtask verify-foundation`",
                path.display()
            )),
        );
        return Ok(());
    }

    match verify_report_schema(&path) {
        Ok(()) => push_audit_check(
            checks,
            blockers,
            "foundation:schema",
            true,
            format!("{} schema valid", path.display()),
            None,
        ),
        Err(error) => {
            push_audit_check(
                checks,
                blockers,
                "foundation:schema",
                false,
                error.to_string(),
                Some(format!(
                    "foundation report `{}` is not schema-valid: {error}",
                    path.display()
                )),
            );
            return Ok(());
        }
    }

    let report = read_json(&path)?;
    let check_ids = report
        .get("per_step_pass_fail")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|check| check.get("id").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();
    for expected in [
        "cargo-test-boon-parser",
        "cargo-test-boon-ir",
        "cargo-test-boon-runtime",
        "cargo-test-workspace",
    ] {
        let present = check_ids.contains(&expected);
        push_audit_check(
            checks,
            blockers,
            format!("foundation:{expected}"),
            present,
            format!("{} includes {expected}", path.display()),
            (!present).then(|| {
                format!(
                    "foundation report `{}` does not include required gate `{expected}`",
                    path.display()
                )
            }),
        );
    }
    Ok(())
}

fn audit_playground_launch(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let aggregate = PathBuf::from("target/reports/playground-launch.json");
    if !aggregate.exists() {
        push_audit_check(
            checks,
            blockers,
            "playground-launch:aggregate-present",
            false,
            format!("missing {}", aggregate.display()),
            Some(format!(
                "missing playground launch smoke report `{}`; run `cargo xtask verify-playground-launch`",
                aggregate.display()
            )),
        );
        return Ok(());
    }
    match verify_report_schema(&aggregate) {
        Ok(()) => push_audit_check(
            checks,
            blockers,
            "playground-launch:aggregate-schema",
            true,
            format!("{} schema valid", aggregate.display()),
            None,
        ),
        Err(error) => {
            push_audit_check(
                checks,
                blockers,
                "playground-launch:aggregate-schema",
                false,
                error.to_string(),
                Some(format!(
                    "playground launch report `{}` is not schema-valid: {error}",
                    aggregate.display()
                )),
            );
            return Ok(());
        }
    }
    for example in ["todomvc", "cells"] {
        let path = PathBuf::from(format!("target/reports/playground-launch-{example}.json"));
        if !path.exists() {
            push_audit_check(
                checks,
                blockers,
                format!("playground-launch:{example}:present"),
                false,
                format!("missing {}", path.display()),
                Some(format!(
                    "missing {example} playground launch smoke report `{}`",
                    path.display()
                )),
            );
            continue;
        }
        match verify_report_schema(&path) {
            Ok(()) => push_audit_check(
                checks,
                blockers,
                format!("playground-launch:{example}:schema"),
                true,
                format!("{} schema valid", path.display()),
                None,
            ),
            Err(error) => {
                push_audit_check(
                    checks,
                    blockers,
                    format!("playground-launch:{example}:schema"),
                    false,
                    error.to_string(),
                    Some(format!(
                        "{example} playground launch smoke report `{}` is not schema-valid: {error}",
                        path.display()
                    )),
                );
                continue;
            }
        }
        let report = read_json(&path)?;
        for key in [
            "example_selector",
            "code_editor",
            "run_reset_step_controls",
            "render_preview",
            "semantic_delta_log",
            "selected_value_inspector",
            "dependency_explanation_panel",
        ] {
            let present = playground_surface_key_proven(&report, key);
            push_audit_check(
                checks,
                blockers,
                format!("playground-launch:{example}:surface:{key}"),
                present,
                format!("{} {} visible bounds", path.display(), key),
                (!present).then(|| {
                    format!(
                        "{example} playground launch smoke report `{}` does not prove visible nonzero bounds for surface `{key}`",
                        path.display()
                    )
                }),
            );
        }
    }
    Ok(())
}

fn audit_playground_background_launch(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let aggregate = PathBuf::from("target/reports/playground-background-launch.json");
    if !aggregate.exists() {
        push_audit_check(
            checks,
            blockers,
            "playground-background-launch:aggregate-present",
            false,
            format!("missing {}", aggregate.display()),
            Some(format!(
                "missing background launch smoke report `{}`; run `cargo xtask verify-playground-background-launch --report target/reports/playground-background-launch.json`",
                aggregate.display()
            )),
        );
        return Ok(());
    }
    match verify_report_schema(&aggregate) {
        Ok(()) => push_audit_check(
            checks,
            blockers,
            "playground-background-launch:aggregate-schema",
            true,
            format!("{} schema valid", aggregate.display()),
            None,
        ),
        Err(error) => {
            push_audit_check(
                checks,
                blockers,
                "playground-background-launch:aggregate-schema",
                false,
                error.to_string(),
                Some(format!(
                    "background launch report `{}` is not schema-valid: {error}",
                    aggregate.display()
                )),
            );
            return Ok(());
        }
    }
    let report = read_json(&aggregate)?;
    let launcher_ok = report
        .get("background_launcher")
        .and_then(serde_json::Value::as_str)
        == Some("cosmic-background-launch")
        && report
            .get("background_workspace")
            .and_then(serde_json::Value::as_str)
            == Some("boon-circuit");
    push_audit_check(
        checks,
        blockers,
        "playground-background-launch:cosmic-workspace",
        launcher_ok,
        "background smoke uses cosmic-background-launch on boon-circuit workspace",
        (!launcher_ok).then(|| {
            "background launch smoke report does not prove the COSMIC boon-circuit workspace launcher"
                .to_owned()
        }),
    );
    let launch_outputs = report
        .get("launch_outputs")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for example in ["todomvc", "cells"] {
        let child = launch_outputs.iter().find(|entry| {
            entry.get("example").and_then(serde_json::Value::as_str) == Some(example)
        });
        let stdout_ok = child
            .and_then(|entry| entry.get("stdout"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|stdout| stdout.contains("background-launch"));
        let pid_ok = child
            .and_then(|entry| entry.get("child_pid"))
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|pid| pid > 0);
        let launch_id_ok = child
            .and_then(|entry| entry.get("launch_id"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|launch_id| launch_id.starts_with("background-launch-"));
        let process_exited = report
            .get("per_step_pass_fail")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .find(|check| {
                check.get("id").and_then(serde_json::Value::as_str)
                    == Some(&format!("{example}-background-launch-smoke"))
            })
            .and_then(|check| check.get("process_exited_after_report"))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        let path = PathBuf::from(format!(
            "target/reports/playground-background-launch-{example}.json"
        ));
        let child_report_ok = path.exists() && verify_report_schema(&path).is_ok();
        push_audit_check(
            checks,
            blockers,
            format!("playground-background-launch:{example}:proof"),
            stdout_ok && pid_ok && launch_id_ok && process_exited && child_report_ok,
            format!(
                "stdout_ok={stdout_ok}, pid_ok={pid_ok}, launch_id_ok={launch_id_ok}, process_exited={process_exited}, child_report_ok={child_report_ok}"
            ),
            (!(stdout_ok && pid_ok && launch_id_ok && process_exited && child_report_ok)).then(|| {
                format!(
                    "{example} background launch report does not prove launcher pid, launch id, process exit, and schema-valid child report"
                )
            }),
        );
    }
    Ok(())
}

fn audit_example_source_contracts(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    for name in ["todomvc", "cells"] {
        let source_path = PathBuf::from(format!("examples/{name}.bn"));
        let source = std::fs::read_to_string(&source_path)?;
        let parsed = match boon_parser::parse_source(source_path.display().to_string(), &source) {
            Ok(parsed) => {
                push_audit_check(
                    checks,
                    blockers,
                    format!("source-contract:{name}:parse"),
                    true,
                    format!(
                        "{} parsed as {}",
                        source_path.display(),
                        parsed.kind.as_str()
                    ),
                    None,
                );
                parsed
            }
            Err(error) => {
                push_audit_check(
                    checks,
                    blockers,
                    format!("source-contract:{name}:parse"),
                    false,
                    error.to_string(),
                    Some(format!(
                        "{name} source `{}` does not parse: {error}",
                        source_path.display()
                    )),
                );
                continue;
            }
        };
        let ir = match boon_ir::lower(&parsed) {
            Ok(ir) => {
                push_audit_check(
                    checks,
                    blockers,
                    format!("source-contract:{name}:lower"),
                    true,
                    format!(
                        "{} lowered to {} IR nodes",
                        source_path.display(),
                        ir.graph_node_count
                    ),
                    None,
                );
                ir
            }
            Err(error) => {
                push_audit_check(
                    checks,
                    blockers,
                    format!("source-contract:{name}:lower"),
                    false,
                    error.clone(),
                    Some(format!(
                        "{name} source `{}` does not lower to typed IR: {error}",
                        source_path.display()
                    )),
                );
                continue;
            }
        };
        match boon_ir::verify_hidden_identity(&ir) {
            Ok(()) => push_audit_check(
                checks,
                blockers,
                format!("source-contract:{name}:hidden-identity"),
                true,
                "hidden runtime identity is not represented as Boon values in IR".to_owned(),
                None,
            ),
            Err(error) => push_audit_check(
                checks,
                blockers,
                format!("source-contract:{name}:hidden-identity"),
                false,
                error.clone(),
                Some(format!("{name} IR leaks hidden runtime identity: {error}")),
            ),
        }
        match boon_ir::verify_static_schedule(&ir) {
            Ok(()) => push_audit_check(
                checks,
                blockers,
                format!("source-contract:{name}:static-schedule"),
                true,
                "typed IR static schedule has ordered nodes and valid source/state/list references"
                    .to_owned(),
                None,
            ),
            Err(error) => push_audit_check(
                checks,
                blockers,
                format!("source-contract:{name}:static-schedule"),
                false,
                error.clone(),
                Some(format!("{name} IR static schedule is invalid: {error}")),
            ),
        }
        let no_dynamic_graph_clones = ir.lists.iter().all(|list| list.graph_clones_per_item == 0);
        push_audit_check(
            checks,
            blockers,
            format!("source-contract:{name}:no-graph-clones-per-item"),
            no_dynamic_graph_clones,
            format!(
                "list graph clones: {:?}",
                ir.lists
                    .iter()
                    .map(|list| (&list.name, list.graph_clones_per_item))
                    .collect::<Vec<_>>()
            ),
            (!no_dynamic_graph_clones)
                .then(|| format!("{name} IR still clones runtime graph per list item")),
        );
        let every_source_is_declared = !parsed.source_ports.is_empty()
            && ir.sources.len() == parsed.source_ports.len()
            && ir.sources.iter().all(|source| {
                parsed
                    .source_ports
                    .iter()
                    .any(|port| port.path == source.path)
            });
        push_audit_check(
            checks,
            blockers,
            format!("source-contract:{name}:sources-derived-from-boon"),
            every_source_is_declared,
            format!(
                "parsed source ports={}, IR source ports={}",
                parsed.source_ports.len(),
                ir.sources.len()
            ),
            (!every_source_is_declared).then(|| {
                format!("{name} IR source table is not derived from declared Boon SOURCE ports")
            }),
        );
        if name == "todomvc" {
            audit_todomvc_source_contract(&source, &ir, checks, blockers);
            audit_todomvc_list_capacity_contract(&source, checks, blockers);
        } else {
            audit_cells_source_contract(&source, &ir, checks, blockers);
        }
    }
    Ok(())
}

fn audit_todomvc_source_contract(
    source: &str,
    ir: &boon_ir::TypedProgram,
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) {
    let no_reducer_shape = !source.contains("FUNCTION update(")
        && !source.contains("event.source |> WHEN")
        && !source.contains("state |>");
    push_audit_check(
        checks,
        blockers,
        "source-contract:todomvc:no-global-reducer",
        no_reducer_shape,
        "checked source for reducer-style update(state,event) and event.source/state pipe shapes"
            .to_owned(),
        (!no_reducer_shape)
            .then(|| "TodoMVC source uses a reducer-style global state/event update".to_owned()),
    );
    let no_visible_row_identity = source.lines().all(|line| {
        let trimmed = line.trim_start();
        !trimmed.starts_with("id:")
            && !trimmed.starts_with("alive:")
            && !trimmed.contains("TodoId")
            && !trimmed.contains("ListKey")
            && !trimmed.contains("runtime_key")
            && !trimmed.contains("source_id:")
    });
    push_audit_check(
        checks,
        blockers,
        "source-contract:todomvc:no-visible-identity",
        no_visible_row_identity,
        "checked TodoMVC Boon source for visible id/alive/ListKey/runtime identity fields"
            .to_owned(),
        (!no_visible_row_identity)
            .then(|| "TodoMVC source exposes identity or lifetime fields to Boon code".to_owned()),
    );
    for (id, needle) in [
        ("list-append", "List/append"),
        ("list-remove", "List/remove"),
        ("list-map", "List/map"),
        ("visible-retain", "visible_todos:"),
        ("active-count", "active_count:"),
        ("completed-count", "completed_count:"),
        ("all-completed", "all_completed:"),
    ] {
        let present = source.contains(needle);
        push_audit_check(
            checks,
            blockers,
            format!("source-contract:todomvc:{id}"),
            present,
            format!("checked TodoMVC source for `{needle}`"),
            (!present)
                .then(|| format!("TodoMVC source is missing documented circuit-style `{needle}`")),
        );
    }
    let row_field_holds = [
        "todo.title",
        "todo.edit_text",
        "todo.completed",
        "todo.editing",
    ]
    .iter()
    .all(|path| {
        ir.state_cells
            .iter()
            .any(|cell| cell.path == *path && cell.indexed)
    });
    push_audit_check(
        checks,
        blockers,
        "source-contract:todomvc:row-field-holds",
        row_field_holds,
        "checked indexed title/edit_text/completed/editing HOLD state cells in typed IR".to_owned(),
        (!row_field_holds).then(|| {
            "TodoMVC row fields are not all represented as local HOLD equations".to_owned()
        }),
    );
    let row_local_sources =
        ir.sources
            .iter()
            .any(|source| source.path == "todo.sources.todo_checkbox.click" && source.scoped)
            && ir.sources.iter().any(|source| {
                source.path == "todo.sources.remove_todo_button.press" && source.scoped
            });
    push_audit_check(
        checks,
        blockers,
        "source-contract:todomvc:row-local-sources",
        row_local_sources,
        "checked row-local todo SOURCE ports in typed IR".to_owned(),
        (!row_local_sources)
            .then(|| "TodoMVC row events are not declared as row-local SOURCE ports".to_owned()),
    );
    let local_hold_updates = ir.update_branches.iter().any(|branch| {
        branch.target == "todo.completed"
            && branch.source == "todo.sources.todo_checkbox.click"
            && branch.indexed
    }) && ir.update_branches.iter().any(|branch| {
        branch.target == "store.selected_filter"
            && branch.source == "store.sources.filter_active.press"
            && !branch.indexed
    });
    push_audit_check(
        checks,
        blockers,
        "source-contract:todomvc:local-hold-updates",
        local_hold_updates,
        "checked local HOLD update branches for row and store fields".to_owned(),
        (!local_hold_updates).then(|| {
            "TodoMVC field updates are not expressed as local typed HOLD branches".to_owned()
        }),
    );
}

fn audit_todomvc_list_capacity_contract(
    source: &str,
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) {
    let oversized_initializer_source = source.replacen("LIST {", "LIST[1] {", 1);
    let oversized_initializer_rejected = run_scenario_source_with_step_limit(
        "capacity-audit:todomvc",
        &oversized_initializer_source,
        Path::new("examples/todomvc.scn"),
        VerificationLayer::Semantic,
        Some(1),
    )
    .err()
    .is_some_and(|error| {
        error
            .to_string()
            .contains("list `todos` initializes 2 rows beyond declared capacity 1")
    });
    push_audit_check(
        checks,
        blockers,
        "source-contract:todomvc:list-capacity-initializer-overflow-rejected",
        oversized_initializer_rejected,
        "checked TodoMVC LIST[1] rejects two-row initializer overflow",
        (!oversized_initializer_rejected).then(|| {
            "TodoMVC bounded LIST capacity does not reject oversized initializers".to_owned()
        }),
    );

    let append_overflow_source = source.replacen("LIST {", "LIST[2] {", 1);
    let append_overflow_rejected = run_scenario_source_with_step_limit(
        "capacity-audit:todomvc",
        &append_overflow_source,
        Path::new("examples/todomvc.scn"),
        VerificationLayer::Semantic,
        Some(3),
    )
    .err()
    .is_some_and(|error| {
        error
            .to_string()
            .contains("generic list `todos` capacity 2 exceeded by append")
    });
    push_audit_check(
        checks,
        blockers,
        "source-contract:todomvc:list-capacity-append-overflow-rejected",
        append_overflow_rejected,
        "checked TodoMVC LIST[2] rejects append overflow",
        (!append_overflow_rejected)
            .then(|| "TodoMVC bounded LIST capacity does not reject append overflow".to_owned()),
    );
}

fn audit_cells_source_contract(
    source: &str,
    ir: &boon_ir::TypedProgram,
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) {
    let grid_shape = source.contains("Grid/cells(columns: 26, rows: 100)")
        && source.contains("|> List/map(seed, new: new_cell(seed: seed))");
    push_audit_check(
        checks,
        blockers,
        "source-contract:cells:grid-shape",
        grid_shape,
        "checked 26x100 Grid/cells plus row-template map".to_owned(),
        (!grid_shape).then(|| {
            "Cells source is missing the documented Grid/cells row-template shape".to_owned()
        }),
    );
    let address_is_data = source.contains("address: seed.address")
        && !source.contains("address |> HOLD")
        && !source.contains("AddressKey")
        && !source.contains("ListKey");
    push_audit_check(
        checks,
        blockers,
        "source-contract:cells:address-is-data",
        address_is_data,
        "checked visible spreadsheet address is seed data, not hidden runtime identity".to_owned(),
        (!address_is_data)
            .then(|| "Cells address is not represented as ordinary visible seed data".to_owned()),
    );
    let editor_sources = ["change", "commit", "cancel"].iter().all(|event| {
        ir.sources
            .iter()
            .any(|source| source.path == format!("cell.sources.editor.{event}") && source.scoped)
    });
    push_audit_check(
        checks,
        blockers,
        "source-contract:cells:editor-sources",
        editor_sources,
        "checked row-local editor change/commit/cancel SOURCE ports".to_owned(),
        (!editor_sources).then(|| {
            "Cells editor events are not declared as row-local Boon SOURCE ports".to_owned()
        }),
    );
    let formula_primitives = ir.formula_operations.iter().any(|operation| {
        operation.target == "cell.parsed_formula"
            && matches!(operation.kind, boon_ir::FormulaOperationKind::Parse { .. })
    }) && ir.formula_operations.iter().any(|operation| {
        operation.target == "cell.dependencies"
            && matches!(
                operation.kind,
                boon_ir::FormulaOperationKind::Dependencies { .. }
            )
    }) && ir.formula_operations.iter().any(|operation| {
        operation.target == "cell.value"
            && matches!(operation.kind, boon_ir::FormulaOperationKind::Eval { .. })
    });
    push_audit_check(
        checks,
        blockers,
        "source-contract:cells:formula-primitives",
        formula_primitives,
        "checked generic Formula/parse, Formula/dependencies, and Formula/eval IR operations"
            .to_owned(),
        (!formula_primitives).then(|| {
            "Cells formula behavior is not represented by generic formula primitives in IR"
                .to_owned()
        }),
    );
    let formula_error_primitive = ir.formula_operations.iter().any(|operation| {
        operation.target == "cell.error"
            && matches!(operation.kind, boon_ir::FormulaOperationKind::Error { .. })
    });
    push_audit_check(
        checks,
        blockers,
        "source-contract:cells:formula-error-primitive",
        formula_error_primitive,
        "checked generic Formula/error IR operation".to_owned(),
        (!formula_error_primitive).then(|| {
            "Cells error behavior is not represented by generic Formula/error in IR".to_owned()
        }),
    );
    let edit_state_in_boon = ir.update_branches.iter().any(|branch| {
        branch.target == "cell.editing_text"
            && branch.source == "cell.sources.editor.change"
            && branch.indexed
    }) && ir.update_branches.iter().any(|branch| {
        branch.target == "cell.formula_text"
            && branch.source == "cell.sources.editor.commit"
            && branch.indexed
    }) && ir.update_branches.iter().any(|branch| {
        branch.target == "cell.editing"
            && branch.source == "cell.sources.editor.cancel"
            && branch.indexed
    });
    push_audit_check(
        checks,
        blockers,
        "source-contract:cells:edit-state-in-boon",
        edit_state_in_boon,
        "checked Boon-derived edit/commit/cancel HOLD branches".to_owned(),
        (!edit_state_in_boon).then(|| {
            "Cells edit/commit/cancel state is not represented as Boon HOLD branches".to_owned()
        }),
    );
}

fn audit_scenario_coverage(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    for (name, required_ids) in [
        ("todomvc", REQUIRED_TODOMVC_SCENARIO_IDS),
        ("cells", REQUIRED_CELLS_SCENARIO_IDS),
    ] {
        let scenario_path = PathBuf::from(format!("examples/{name}.scn"));
        let scenario = boon_runtime::parse_scenario(&scenario_path)?;
        let ids = scenario
            .step
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();
        let unique_ids = ids.iter().copied().collect::<BTreeSet<_>>();
        push_audit_check(
            checks,
            blockers,
            format!("scenario-contract:{name}:unique-labels"),
            unique_ids.len() == ids.len(),
            format!("{} labels, {} unique", ids.len(), unique_ids.len()),
            (unique_ids.len() != ids.len()).then(|| {
                format!(
                    "{name} scenario `{}` contains duplicate step ids",
                    scenario_path.display()
                )
            }),
        );
        for required_id in required_ids {
            let present = unique_ids.contains(required_id);
            push_audit_check(
                checks,
                blockers,
                format!("scenario-contract:{name}:label:{required_id}"),
                present,
                format!(
                    "{} contains required scenario label `{required_id}`",
                    scenario_path.display()
                ),
                (!present).then(|| {
                    format!(
                        "{name} scenario `{}` is missing required label `{required_id}`",
                        scenario_path.display()
                    )
                }),
            );
        }
        let scenario_events = scenario
            .step
            .iter()
            .filter(|step| step.expected_source_event.is_some())
            .count();
        push_audit_check(
            checks,
            blockers,
            format!("scenario-contract:{name}:source-events-present"),
            scenario_events > 0,
            format!("{scenario_events} scenario steps expect source events"),
            (scenario_events == 0).then(|| {
                format!(
                    "{name} scenario `{}` does not exercise SOURCE-driven changes",
                    scenario_path.display()
                )
            }),
        );
    }
    Ok(())
}

const REQUIRED_TODOMVC_SCENARIO_IDS: &[&str] = &[
    "initial",
    "add-test-todo-type",
    "add-test-todo-submit",
    "reject-empty-todo",
    "toggle-all-complete",
    "toggle-all-active",
    "toggle-buy-groceries",
    "toggle-dynamic-test-todo-under-active-filter",
    "filter-active",
    "filter-completed",
    "filter-all",
    "edit-test-todo",
    "edit-test-todo-change",
    "edit-test-todo-commit",
    "edit-test-todo-cancel-open",
    "edit-test-todo-cancel-change",
    "edit-test-todo-cancel-escape",
    "edit-test-todo-blur-open",
    "edit-test-todo-blur-change",
    "edit-test-todo-blur-commit",
    "clear-completed",
    "hover-delete-clean-room",
    "delete-clean-room",
    "empty-state",
    "add-after-clear-type",
    "add-after-clear-submit",
    "toggle-all-single-after-clear",
    "clear-all-rows",
];

const REQUIRED_CELLS_SCENARIO_IDS: &[&str] = &[
    "initial",
    "edit-a1-literal",
    "commit-a1-literal",
    "edit-a1-cancel-draft",
    "cancel-a1-draft",
    "commit-b1-formula",
    "change-a1-updates-b1",
    "cycle-error",
    "replace-b1-formula-removes-stale-cycle-edge",
    "a1-recomputes-after-cycle-break",
    "change-a1-after-edge-replacement-does-not-recompute-b1",
    "commit-c1-fanout-formula",
    "commit-d1-fanout-formula",
    "change-a1-fanout-recomputes-dependents-only",
    "d1-updated-by-fanout",
];

fn audit_cli_scenario_reports(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    for name in ["todomvc", "cells"] {
        let path = PathBuf::from(format!("target/reports/{name}-cli-run.json"));
        if !path.exists() {
            push_audit_check(
                checks,
                blockers,
                format!("cli-run:{name}:report-present"),
                false,
                format!("missing {}", path.display()),
                Some(format!(
                    "missing {name} CLI scenario report `{}`; run `cargo run -p boon_cli -- run examples/{name}.bn --scenario examples/{name}.scn --report {}`",
                    path.display(),
                    path.display()
                )),
            );
            continue;
        }
        match verify_report_schema(&path) {
            Ok(()) => push_audit_check(
                checks,
                blockers,
                format!("cli-run:{name}:schema"),
                true,
                format!("{} schema valid", path.display()),
                None,
            ),
            Err(error) => {
                push_audit_check(
                    checks,
                    blockers,
                    format!("cli-run:{name}:schema"),
                    false,
                    error.to_string(),
                    Some(format!(
                        "{name} CLI scenario report `{}` is not schema-valid: {error}",
                        path.display()
                    )),
                );
                continue;
            }
        }
        let report = read_json(&path)?;
        let expected_source = format!("examples/{name}.bn");
        let expected_scenario = format!("examples/{name}.scn");
        let argv = report
            .get("command_argv")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let argv_strings = argv
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        let argv_matches = argv_strings.iter().any(|arg| *arg == "run")
            && argv_strings.iter().any(|arg| *arg == expected_source)
            && argv_strings.iter().any(|arg| *arg == "--scenario")
            && argv_strings.iter().any(|arg| *arg == expected_scenario);
        push_audit_check(
            checks,
            blockers,
            format!("cli-run:{name}:argv"),
            argv_matches,
            format!("command_argv={argv_strings:?}"),
            (!argv_matches).then(|| {
                format!(
                    "{name} CLI report `{}` does not prove the documented boon_cli run command",
                    path.display()
                )
            }),
        );
        let paths_match = report
            .get("source_path")
            .and_then(serde_json::Value::as_str)
            == Some(expected_source.as_str())
            && report
                .get("scenario_path")
                .and_then(serde_json::Value::as_str)
                == Some(expected_scenario.as_str())
            && report
                .get("source_hash")
                .and_then(serde_json::Value::as_str)
                == Some(file_hash(&expected_source).as_str())
            && report
                .get("scenario_hash")
                .and_then(serde_json::Value::as_str)
                == Some(file_hash(&expected_scenario).as_str());
        push_audit_check(
            checks,
            blockers,
            format!("cli-run:{name}:source-scenario-current"),
            paths_match,
            "source/scenario paths and hashes match current examples".to_owned(),
            (!paths_match).then(|| {
                format!(
                    "{name} CLI report `{}` is not bound to current source/scenario files",
                    path.display()
                )
            }),
        );
        let execution = report
            .get("runtime_execution")
            .unwrap_or(&serde_json::Value::Null);
        let generic_runtime = execution
            .get("source_loaded_from_boon")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
            && execution
                .get("typed_ir_loaded")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
            && execution
                .get("static_schedule_verified")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
            && execution
                .get("generic_interpreter_complete")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
            && execution
                .get("example_behavior_adapter")
                .and_then(serde_json::Value::as_bool)
                == Some(false);
        push_audit_check(
            checks,
            blockers,
            format!("cli-run:{name}:generic-runtime"),
            generic_runtime,
            "CLI scenario report proves Boon source -> typed IR -> static schedule -> generic interpreter".to_owned(),
            (!generic_runtime).then(|| {
                format!(
                    "{name} CLI report `{}` does not prove the generic static-graph runtime path",
                    path.display()
                )
            }),
        );
        let developer_summary_hides_identity = report
            .get("state_summary")
            .is_some_and(|summary| !state_summary_exposes_hidden_identity(summary));
        push_audit_check(
            checks,
            blockers,
            format!("cli-run:{name}:developer-summary-hides-identity"),
            developer_summary_hides_identity,
            "CLI state_summary hides hidden keys, generations, source ids, and bind epochs"
                .to_owned(),
            (!developer_summary_hides_identity).then(|| {
                format!(
                    "{name} CLI report `{}` exposes hidden runtime identity in state_summary",
                    path.display()
                )
            }),
        );
        let scenario = boon_runtime::parse_scenario(Path::new(&expected_scenario))?;
        let total_ticks = report
            .get("total_ticks")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default() as usize;
        let total_source_events = report
            .get("total_source_events")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        let scenario_exercised = total_ticks >= scenario.step.len() && total_source_events > 0;
        push_audit_check(
            checks,
            blockers,
            format!("cli-run:{name}:scenario-exercised"),
            scenario_exercised,
            format!(
                "total_ticks={total_ticks}, scenario_steps={}, total_source_events={total_source_events}",
                scenario.step.len()
            ),
            (!scenario_exercised).then(|| {
                format!(
                    "{name} CLI report `{}` did not exercise the scenario source events",
                    path.display()
                )
            }),
        );
    }
    Ok(())
}

fn state_summary_exposes_hidden_identity(summary: &serde_json::Value) -> bool {
    let rendered = summary.to_string();
    [
        "hidden_key",
        "hidden_keys",
        "hidden_generation",
        "source_id",
        "bind_epoch",
    ]
    .iter()
    .any(|needle| rendered.contains(needle))
}

fn audit_runtime_execution(
    name: &str,
    layer: VerificationLayer,
    report_path: &Path,
    report: &serde_json::Value,
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) {
    let Some(execution) = report.get("runtime_execution") else {
        push_audit_check(
            checks,
            blockers,
            format!("{name}:{}:runtime-execution", layer.as_str()),
            false,
            "missing runtime_execution",
            Some(format!(
                "{name} {} report `{}` missing runtime_execution",
                layer.as_str(),
                report_path.display()
            )),
        );
        return;
    };

    let generic = execution
        .get("generic_interpreter_complete")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    push_audit_check(
        checks,
        blockers,
        format!("{name}:{}:generic-interpreter", layer.as_str()),
        generic,
        format!(
            "generic_interpreter_complete={}",
            execution
                .get("generic_interpreter_complete")
                .unwrap_or(&serde_json::Value::Null)
        ),
        (!generic).then_some(format!(
            "{name} {} report is still not executed by the complete generic interpreter",
            layer.as_str()
        )),
    );

    let adapter_free = execution
        .get("example_behavior_adapter")
        .and_then(serde_json::Value::as_bool)
        == Some(false);
    let adapter_blocker = execution
        .get("adapter_blocker")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("no adapter blocker detail");
    push_audit_check(
        checks,
        blockers,
        format!("{name}:{}:adapter-free", layer.as_str()),
        adapter_free,
        format!(
            "example_behavior_adapter={}; {adapter_blocker}",
            execution
                .get("example_behavior_adapter")
                .unwrap_or(&serde_json::Value::Null)
        ),
        (!adapter_free).then_some(format!(
            "{name} {} report is still adapter-backed: {adapter_blocker}",
            layer.as_str()
        )),
    );

    let implementation = execution
        .get("implementation")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let implementation_final = !implementation.contains("adapter");
    push_audit_check(
        checks,
        blockers,
        format!("{name}:{}:implementation-name", layer.as_str()),
        implementation_final,
        format!("implementation={implementation}"),
        (!implementation_final).then_some(format!(
            "{name} {} report implementation still says `{implementation}`",
            layer.as_str()
        )),
    );
}

fn audit_negative_report_contract(
    name: &str,
    report_path: &Path,
    report: &serde_json::Value,
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) {
    let negative_ids = report
        .get("per_step_pass_fail")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|check| check.get("id").and_then(serde_json::Value::as_str))
        .collect::<BTreeSet<_>>();
    for expected in [
        "direct-source-injection-headed-rejected-by-contract",
        "headed-without-os-input-limitation-rejected",
        "fake-full-os-input-report-rejected",
        "fake-full-os-input-without-visible-coverage-rejected",
        "handwritten-human-report-rejected",
        "prepare-human-report-pass-labels-enforced",
        "missing-headed-report-binding-rejected",
        "headed-only-manual-artifacts-rejected",
        "replace-placeholder-manual-report-rejected",
        "fake-manual-image-artifact-rejected",
        "fake-manual-video-artifact-rejected",
        "future-generated-human-report-rejected",
        "future-manual-session-rejected",
        "debug-speed-report-rejected",
        "failed-speed-budget-rejected",
        "missing-speed-stress-profiles-rejected",
        "missing-speed-resource-fields-rejected",
        "adapter-runtime-execution-rejected",
        "incomplete-generic-runtime-slice-rejected",
        "missing-delta-runtime-id-rejected",
        "bad-delta-server-tick-rejected",
        "missing-delta-step-id-rejected",
        "missing-playground-surface-rejected",
    ] {
        let present = negative_ids.contains(expected);
        push_audit_check(
            checks,
            blockers,
            format!("{name}:negative:fixture:{expected}"),
            present,
            format!(
                "{} includes negative fixture `{expected}`",
                report_path.display()
            ),
            (!present).then(|| {
                format!(
                    "{name} negative report `{}` does not include required fixture `{expected}`",
                    report_path.display()
                )
            }),
        );
    }
}

fn audit_headed_input(
    name: &str,
    report_path: &Path,
    report: &serde_json::Value,
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let method = report
        .get("input_injection_method")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let full_os_input = method == "os_pointer_keyboard_to_visible_window";
    push_audit_check(
        checks,
        blockers,
        format!("{name}:headed:full-os-input"),
        full_os_input,
        format!("input_injection_method={method}"),
        (!full_os_input).then_some(format!(
            "{name} headed report `{}` does not drive every step through real OS pointer/keyboard hit testing",
            report_path.display()
        )),
    );

    let limitation = report
        .get("os_input_limitation")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let no_limitation = limitation.is_empty();
    push_audit_check(
        checks,
        blockers,
        format!("{name}:headed:no-os-input-limitation"),
        no_limitation,
        if no_limitation {
            "no os_input_limitation".to_owned()
        } else {
            limitation.to_owned()
        },
        (!no_limitation).then_some(format!(
            "{name} headed report `{}` still records os_input_limitation",
            report_path.display()
        )),
    );

    let window_pid_ok = report
        .get("window_pid")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|pid| pid > 0);
    let window_title_ok = report
        .get("window_title")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|title| !title.trim().is_empty());
    let display_server_ok = report
        .get("display_server")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|server| !server.trim().is_empty());
    let display_scale_ok = report
        .get("display_scale")
        .and_then(serde_json::Value::as_f64)
        .is_some_and(|scale| scale > 0.0);
    let display_connection_ok = report
        .get("display_socket_or_compositor_connection")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|connection| !connection.trim().is_empty());
    let input_backend_ok = report
        .get("input_backend")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|backend| !backend.trim().is_empty());
    let capture_backend_ok = report
        .get("capture_backend")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|backend| !backend.trim().is_empty());
    let focused_window_ok = report
        .get("focused_window_proof")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|proof| !proof.trim().is_empty());
    let metadata_ok = window_pid_ok
        && window_title_ok
        && display_server_ok
        && display_scale_ok
        && display_connection_ok
        && input_backend_ok
        && capture_backend_ok
        && focused_window_ok;
    push_audit_check(
        checks,
        blockers,
        format!("{name}:headed:metadata"),
        metadata_ok,
        format!(
            "window_pid={window_pid_ok}, window_title={window_title_ok}, display_server={display_server_ok}, display_scale={display_scale_ok}, display_connection={display_connection_ok}, input_backend={input_backend_ok}, capture_backend={capture_backend_ok}, focused_window={focused_window_ok}"
        ),
        (!metadata_ok).then_some(format!(
            "{name} headed report `{}` is missing window/display/input/capture/focus metadata",
            report_path.display()
        )),
    );

    let scenario = boon_runtime::parse_scenario(Path::new(&format!("examples/{name}.scn")))?;
    let expected_labels = scenario
        .step
        .iter()
        .map(|step| step.id.as_str())
        .collect::<Vec<_>>();
    let os_steps = report
        .get("os_input_steps")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let step_labels_match = os_steps.len() == expected_labels.len()
        && os_steps
            .iter()
            .zip(expected_labels.iter())
            .all(|(step, label)| {
                step.get("id").and_then(serde_json::Value::as_str) == Some(*label)
            });
    push_audit_check(
        checks,
        blockers,
        format!("{name}:headed:os-step-labels"),
        step_labels_match,
        format!(
            "os_input_steps={}, scenario_steps={}",
            os_steps.len(),
            expected_labels.len()
        ),
        (!step_labels_match).then_some(format!(
            "{name} headed report `{}` does not cover every scenario label in order",
            report_path.display()
        )),
    );

    let artifact_paths = report
        .get("artifact_sha256s")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artifact| artifact.get("path").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let step_artifacts_ok = !os_steps.is_empty()
        && os_steps.iter().all(|step| {
            let target_ok = step
                .get("target_element_id")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|target| !target.trim().is_empty());
            let bounds_ok = step.get("visible_bounds").is_some_and(|bounds| {
                bounds
                    .get("width")
                    .and_then(serde_json::Value::as_f64)
                    .is_some_and(|width| width > 0.0)
                    && bounds
                        .get("height")
                        .and_then(serde_json::Value::as_f64)
                        .is_some_and(|height| height > 0.0)
            });
            let screenshot_path = step
                .get("screenshot_path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let screenshot_ok =
                artifact_paths.contains(screenshot_path) && Path::new(screenshot_path).exists();
            target_ok && bounds_ok && screenshot_ok
        });
    push_audit_check(
        checks,
        blockers,
        format!("{name}:headed:os-step-artifacts"),
        step_artifacts_ok,
        "every OS input step has visible target bounds and a hashed screenshot artifact".to_owned(),
        (!step_artifacts_ok).then_some(format!(
            "{name} headed report `{}` has incomplete per-step OS target/artifact evidence",
            report_path.display()
        )),
    );

    let nonblank_ok = report
        .get("nonblank_screenshot_hashes")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|hashes| {
            hashes.iter().any(|hash| {
                hash.get("nonzero_channels")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|channels| channels > 0)
                    && hash
                        .get("unique_rgba_values")
                        .and_then(serde_json::Value::as_u64)
                        .is_some_and(|values| values > 1)
            })
        });
    push_audit_check(
        checks,
        blockers,
        format!("{name}:headed:nonblank-screenshot"),
        nonblank_ok,
        "headed report includes nonblank screenshot pixel statistics".to_owned(),
        (!nonblank_ok).then_some(format!(
            "{name} headed report `{}` does not prove nonblank screenshots",
            report_path.display()
        )),
    );

    let stale_failure_path = headed_debug_failure_path(name);
    let no_stale_failure = !stale_failure_path.exists();
    push_audit_check(
        checks,
        blockers,
        format!("{name}:headed:no-stale-debug-failure"),
        no_stale_failure,
        if no_stale_failure {
            "no stale headed failure report".to_owned()
        } else {
            format!("stale failure report {}", stale_failure_path.display())
        },
        (!no_stale_failure).then_some(format!(
            "{name} has a stale headed failure report `{}` despite a passing headed report",
            stale_failure_path.display()
        )),
    );
    Ok(())
}

fn audit_playground_surface(
    name: &str,
    report_path: &Path,
    report: &serde_json::Value,
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) {
    let expected = [
        "example_selector",
        "code_editor",
        "run_reset_step_controls",
        "render_preview",
        "semantic_delta_log",
        "selected_value_inspector",
        "dependency_explanation_panel",
    ];
    for key in expected {
        let present = playground_surface_key_proven(report, key);
        push_audit_check(
            checks,
            blockers,
            format!("{name}:playground-surface:{key}"),
            present,
            format!("{} {} visible bounds", report_path.display(), key),
            (!present).then_some(format!(
                "{name} headed report `{}` does not prove visible nonzero bounds for playground surface `{key}`",
                report_path.display()
            )),
        );
    }
}

fn playground_surface_key_proven(report: &serde_json::Value, key: &str) -> bool {
    let claimed = report
        .get("playground_surface")
        .and_then(|surface| surface.get(key))
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let Some(elements) = report
        .get("playground_surface_visible_bounds")
        .and_then(|bounds| bounds.get(key))
    else {
        return false;
    };
    let bounds_pass = elements.get("pass").and_then(serde_json::Value::as_bool) == Some(true);
    let Some(elements) = elements
        .get("elements")
        .and_then(serde_json::Value::as_array)
    else {
        return false;
    };
    claimed
        && bounds_pass
        && !elements.is_empty()
        && elements.iter().all(|element| {
            let visible = element.get("visible").and_then(serde_json::Value::as_bool) == Some(true);
            let width = element
                .get("bounds")
                .and_then(|bounds| bounds.get("width"))
                .and_then(serde_json::Value::as_f64)
                .unwrap_or_default();
            let height = element
                .get("bounds")
                .and_then(|bounds| bounds.get("height"))
                .and_then(serde_json::Value::as_f64)
                .unwrap_or_default();
            visible && width > 0.0 && height > 0.0
        })
}

fn audit_playground_custom_source(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from("target/reports/playground-custom-source.json");
    if !path.exists() {
        push_audit_check(
            checks,
            blockers,
            "playground-custom-source:report-present",
            false,
            format!("missing {}", path.display()),
            Some(format!(
                "missing playground custom-source report `{}`; run `cargo xtask verify-playground-custom-source`",
                path.display()
            )),
        );
        return Ok(());
    }

    match verify_report_schema(&path) {
        Ok(()) => push_audit_check(
            checks,
            blockers,
            "playground-custom-source:schema",
            true,
            format!("{} schema valid", path.display()),
            None,
        ),
        Err(error) => {
            push_audit_check(
                checks,
                blockers,
                "playground-custom-source:schema",
                false,
                error.to_string(),
                Some(format!(
                    "playground custom-source report `{}` is not schema-valid: {error}",
                    path.display()
                )),
            );
            return Ok(());
        }
    }

    let report = read_json(&path)?;
    let proof = report.get("playground_custom_source");
    for key in [
        "custom_source_text_was_interpreted",
        "custom_scenario_was_interpreted",
        "original_scenario_rejected_custom_initial_state",
    ] {
        let present = proof
            .and_then(|proof| proof.get(key))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        push_audit_check(
            checks,
            blockers,
            format!("playground-custom-source:{key}"),
            present,
            format!("{} {}", path.display(), key),
            (!present).then_some(format!(
                "playground custom-source report `{}` does not prove `{key}`",
                path.display()
            )),
        );
    }

    let source_hash_changed = report
        .get("source_hash")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|hash| hash != file_hash("examples/todomvc.bn"));
    push_audit_check(
        checks,
        blockers,
        "playground-custom-source:source-hash-differs-from-example",
        source_hash_changed,
        format!(
            "{} source hash differs from bundled TodoMVC",
            path.display()
        ),
        (!source_hash_changed).then_some(format!(
            "playground custom-source report `{}` did not use a modified source hash",
            path.display()
        )),
    );
    for example in ["todomvc", "cells"] {
        let example_proof = proof
            .and_then(|proof| proof.get("custom_examples"))
            .and_then(serde_json::Value::as_array)
            .and_then(|examples| {
                examples.iter().find(|entry| {
                    entry.get("example").and_then(serde_json::Value::as_str) == Some(example)
                })
            });
        let present = example_proof.is_some();
        push_audit_check(
            checks,
            blockers,
            format!("playground-custom-source:{example}:example-proof-present"),
            present,
            format!("{} custom example proof for {example}", path.display()),
            (!present).then_some(format!(
                "playground custom-source report `{}` does not include a custom `{example}` proof",
                path.display()
            )),
        );
        let source_interpreted = example_proof
            .and_then(|proof| proof.get("custom_source_text_was_interpreted"))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        let scenario_interpreted = example_proof
            .and_then(|proof| proof.get("custom_scenario_was_interpreted"))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        let hash_differs = example_proof
            .and_then(|proof| proof.get("source_hash_differs_from_bundled_example"))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        push_audit_check(
            checks,
            blockers,
            format!("playground-custom-source:{example}:source-and-scenario"),
            source_interpreted && scenario_interpreted && hash_differs,
            format!(
                "source_interpreted={source_interpreted}, scenario_interpreted={scenario_interpreted}, hash_differs={hash_differs}"
            ),
            (!(source_interpreted && scenario_interpreted && hash_differs)).then_some(format!(
                "playground custom-source report `{}` does not prove modified {example} source/scenario execution",
                path.display()
            )),
        );
    }
    Ok(())
}

fn audit_scope_control(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_paths = std::iter::once(PathBuf::from("Cargo.toml"))
        .chain(std::fs::read_dir("crates")?.filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path().join("Cargo.toml");
            path.exists().then_some(path)
        }))
        .collect::<Vec<_>>();
    let mut manifest_text = String::new();
    for path in &manifest_paths {
        manifest_text.push_str(&std::fs::read_to_string(path)?);
        manifest_text.push('\n');
    }
    if let Ok(lock) = std::fs::read_to_string("Cargo.lock") {
        manifest_text.push_str(&lock);
    }
    let forbidden_dependency_needles = [
        ("differential-dataflow", "Differential Dataflow core"),
        ("timely", "Timely/DD substrate"),
        ("actix", "actor runtime"),
        ("ractor", "actor runtime"),
        ("xtra", "actor runtime"),
        ("async-channel", "channels-per-value substrate"),
        ("crossbeam-channel", "channels-per-value substrate"),
        ("flume", "channels-per-value substrate"),
        ("kanal", "channels-per-value substrate"),
        ("tokio", "async runtime"),
        ("async-std", "async runtime"),
        ("smol", "async runtime"),
        ("wasmtime", "bytecode/VM substrate"),
        ("cranelift", "codegen/bytecode substrate"),
        ("inkwell", "codegen substrate"),
        ("yew", "virtual DOM/web UI substrate"),
        ("dioxus", "virtual DOM/web UI substrate"),
        ("leptos", "web UI substrate"),
        ("sycamore", "web UI substrate"),
        ("virtual-dom", "virtual DOM substrate"),
    ];
    for (needle, label) in forbidden_dependency_needles {
        let present = manifest_text.contains(needle);
        push_audit_check(
            checks,
            blockers,
            format!("scope:no-forbidden-dependency:{needle}"),
            !present,
            format!(
                "checked {} manifests and Cargo.lock for {label}",
                manifest_paths.len()
            ),
            present.then(|| format!("forbidden first-phase dependency `{needle}` found: {label}")),
        );
    }

    for needle in [
        "codegen-rust",
        "codegen-zig",
        "compile-rust",
        "compile-zig",
        "bytecode",
    ] {
        let present = xtask_command_supported(needle);
        push_audit_check(
            checks,
            blockers,
            format!("scope:no-phase-command:{needle}"),
            !present,
            "checked xtask command dispatcher for out-of-phase commands".to_owned(),
            present.then(|| {
                format!("out-of-phase command `{needle}` is exposed before this interpreter phase")
            }),
        );
    }

    for example in ["todomvc", "cells"] {
        audit_static_graph_speed_scope(example, checks, blockers)?;
        audit_direct_ply_patch_scope(example, checks, blockers)?;
    }
    audit_no_legacy_runtime_fallback(checks, blockers)?;
    Ok(())
}

fn audit_no_legacy_runtime_fallback(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let runtime_path = Path::new("crates/boon_runtime/src/lib.rs");
    let text = std::fs::read_to_string(runtime_path)?;
    for forbidden in [
        "impl ScenarioExecutor for TodoRuntime",
        "impl ScenarioExecutor for CellsRuntime",
        "run_generic_scenario(TodoRuntime",
        "run_generic_scenario(CellsRuntime",
    ] {
        let present = text.contains(forbidden);
        push_audit_check(
            checks,
            blockers,
            format!(
                "scope:no-legacy-runtime-fallback:{}",
                sanitize_audit_id(forbidden)
            ),
            !present,
            format!("checked {} for `{forbidden}`", runtime_path.display()),
            present.then(|| {
                format!(
                    "runtime source `{}` still exposes legacy fallback `{forbidden}`",
                    runtime_path.display()
                )
            }),
        );
    }

    let todomvc_stress_section = text
        .split("fn seeded_todomvc_generic(")
        .nth(1)
        .and_then(|tail| {
            tail.split("#[cfg(test)]\n#[derive(Clone, Debug)]\nstruct TodoRuntime")
                .next()
        })
        .unwrap_or_default();
    let stress_uses_generic = todomvc_stress_section
        .contains("let compiled = CompiledProgram::from_ir(ir)?;")
        && todomvc_stress_section
            .contains("let mut runtime = GenericScheduledRuntime::new(ir, &compiled)?;")
        && todomvc_stress_section.contains("row_template.materialize(seed_fields)?");
    push_audit_check(
        checks,
        blockers,
        "scope:todomvc:stress-uses-generic-scheduled-runtime",
        stress_uses_generic,
        "TodoMVC stress profiles seed GenericScheduledRuntime from typed IR",
        (!stress_uses_generic).then(|| {
            "TodoMVC stress profiles do not prove IR-derived GenericScheduledRuntime execution"
                .to_owned()
        }),
    );

    let todomvc_stress_uses_default_binding_helper =
        todomvc_stress_section.contains("default_todo_list_source_bindings");
    push_audit_check(
        checks,
        blockers,
        "scope:todomvc:stress-no-default-source-binding-helper",
        !todomvc_stress_uses_default_binding_helper,
        "TodoMVC stress profiles derive row source bindings from the compiled program",
        todomvc_stress_uses_default_binding_helper.then(|| {
            "TodoMVC stress profiles still parse bundled source through default_todo_list_source_bindings"
                .to_owned()
        }),
    );

    let stress_uses_legacy = text.contains("let mut runtime = TodoRuntime::seeded(rows);");
    push_audit_check(
        checks,
        blockers,
        "scope:todomvc:stress-no-legacy-runtime-wrapper",
        !stress_uses_legacy,
        "TodoMVC stress profiles do not instantiate TodoRuntime",
        stress_uses_legacy.then(|| {
            "TodoMVC stress profiles still instantiate the legacy TodoRuntime wrapper".to_owned()
        }),
    );

    let cells_stress_section = text
        .split("fn cells_stress_profiles(ir: &TypedProgram) -> RuntimeResult<JsonValue>")
        .nth(1)
        .and_then(|tail| tail.split("fn formula_ast_dependencies_into").next())
        .unwrap_or_default();
    let cells_stress_uses_ir_runtime = cells_stress_section
        .contains("let compiled = CompiledProgram::from_ir(ir)?;")
        && cells_stress_section
            .contains("let generic = GenericScheduledRuntime::new(ir, &compiled)?;")
        && cells_stress_section.contains("initialize_loaded_cells_generic(generic, ir)?");
    push_audit_check(
        checks,
        blockers,
        "scope:cells:stress-uses-loaded-ir-runtime",
        cells_stress_uses_ir_runtime,
        "Cells stress profiles construct GenericScheduledRuntime from typed IR",
        (!cells_stress_uses_ir_runtime).then(|| {
            "Cells stress profiles do not prove CompiledProgram/GenericScheduledRuntime execution"
                .to_owned()
        }),
    );

    let cells_stress_uses_default_tables = cells_stress_section.contains("default_cells")
        || cells_stress_section.contains("generic_cells_runtime(");
    push_audit_check(
        checks,
        blockers,
        "scope:cells:stress-no-default-table-runtime",
        !cells_stress_uses_default_tables,
        "Cells stress profiles do not instantiate default Rust Cells tables",
        cells_stress_uses_default_tables.then(|| {
            "Cells stress profiles still instantiate default Rust Cells tables".to_owned()
        }),
    );

    let public_path_uses_loaded_runtime = text.contains("let output = run_loaded_scenario")
        && text.contains("let runtime = LoadedRuntime::new(parsed, ir, &compiled)?;");
    push_audit_check(
        checks,
        blockers,
        "scope:runtime-public-scenario-path-loaded-runtime",
        public_path_uses_loaded_runtime,
        "public scenario execution enters LoadedRuntime",
        (!public_path_uses_loaded_runtime)
            .then(|| "public scenario execution no longer proves LoadedRuntime entry".to_owned()),
    );
    Ok(())
}

fn audit_direct_ply_patch_scope(
    example: &str,
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    for layer in [VerificationLayer::HeadlessPly, VerificationLayer::HeadedPly] {
        let path = report_path(example, layer);
        if !path.exists() {
            push_audit_check(
                checks,
                blockers,
                format!(
                    "scope:{example}:{}:direct-ply-report-present",
                    layer.as_str()
                ),
                false,
                format!("missing {}", path.display()),
                Some(format!(
                    "missing {example} {} report for direct Ply patch audit `{}`",
                    layer.as_str(),
                    path.display()
                )),
            );
            continue;
        }
        let report = read_json(&path)?;
        let renderer = report
            .get("renderer")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        push_audit_check(
            checks,
            blockers,
            format!("scope:{example}:{}:renderer-ply-engine", layer.as_str()),
            renderer == "ply-engine",
            format!("renderer={renderer}"),
            (renderer != "ply-engine").then(|| {
                format!(
                    "{example} {} report `{}` did not use ply-engine renderer",
                    layer.as_str(),
                    path.display()
                )
            }),
        );
        let patches = report
            .get("render_patches")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let total_render_deltas = report
            .get("total_render_deltas")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default() as usize;
        let patch_trace_direct = total_render_deltas > 0 && patches.len() == total_render_deltas;
        push_audit_check(
            checks,
            blockers,
            format!(
                "scope:{example}:{}:direct-render-patch-trace",
                layer.as_str()
            ),
            patch_trace_direct,
            format!(
                "total_render_deltas={total_render_deltas}, render_patches={}",
                patches.len()
            ),
            (!patch_trace_direct).then(|| {
                format!(
                    "{example} {} report `{}` does not prove a direct render patch trace",
                    layer.as_str(),
                    path.display()
                )
            }),
        );
        let forbidden_diff_fields =
            report.get("virtual_dom_diff").is_some() || report.get("list_diff").is_some();
        push_audit_check(
            checks,
            blockers,
            format!("scope:{example}:{}:no-diff-report-fields", layer.as_str()),
            !forbidden_diff_fields,
            "checked report for virtual_dom_diff/list_diff fields".to_owned(),
            forbidden_diff_fields.then(|| {
                format!(
                    "{example} {} report `{}` contains virtual DOM or list diff fields",
                    layer.as_str(),
                    path.display()
                )
            }),
        );
        let forbidden_patch = patches.iter().find_map(|patch| {
            let kind = patch
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let lowered = kind.to_ascii_lowercase();
            (lowered.contains("diff")
                || lowered.contains("reconcile")
                || lowered.contains("whole")
                || lowered.contains("snapshot"))
            .then(|| kind.to_owned())
        });
        push_audit_check(
            checks,
            blockers,
            format!("scope:{example}:{}:no-diff-patch-kind", layer.as_str()),
            forbidden_patch.is_none(),
            "checked render patch kinds for diff/reconcile/whole/snapshot".to_owned(),
            forbidden_patch.map(|kind| {
                format!(
                    "{example} {} report `{}` contains diff-like render patch kind `{kind}`",
                    layer.as_str(),
                    path.display()
                )
            }),
        );
    }
    Ok(())
}

fn audit_static_graph_speed_scope(
    example: &str,
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = report_path(example, VerificationLayer::Speed);
    if !path.exists() {
        push_audit_check(
            checks,
            blockers,
            format!("scope:{example}:speed-report-present"),
            false,
            format!("missing {}", path.display()),
            Some(format!(
                "missing {example} speed report for static graph scope audit `{}`",
                path.display()
            )),
        );
        return Ok(());
    }
    let report = read_json(&path)?;
    let graph_rebuild_count = report
        .get("graph_rebuild_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX);
    push_audit_check(
        checks,
        blockers,
        format!("scope:{example}:zero-graph-rebuilds"),
        graph_rebuild_count == 0,
        format!(
            "{} graph_rebuild_count={graph_rebuild_count}",
            path.display()
        ),
        (graph_rebuild_count != 0).then(|| {
            format!(
                "{example} speed report `{}` rebuilt the graph",
                path.display()
            )
        }),
    );
    let graph_clones = report
        .get("graph_clones_per_item")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX);
    push_audit_check(
        checks,
        blockers,
        format!("scope:{example}:zero-graph-clones-per-item"),
        graph_clones == 0,
        format!("{} graph_clones_per_item={graph_clones}", path.display()),
        (graph_clones != 0).then(|| {
            format!(
                "{example} speed report `{}` has per-item graph clones",
                path.display()
            )
        }),
    );
    let graph_node_count = report
        .get("graph_node_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX);
    let stress_profiles = report
        .get("stress_profiles")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    push_audit_check(
        checks,
        blockers,
        format!("scope:{example}:stress-profiles-present"),
        !stress_profiles.is_empty(),
        format!(
            "{} stress profile count {}",
            path.display(),
            stress_profiles.len()
        ),
        stress_profiles.is_empty().then(|| {
            format!(
                "{example} speed report `{}` has no large static-graph stress profiles",
                path.display()
            )
        }),
    );
    for profile in stress_profiles {
        let name = profile
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unnamed");
        let profile_graph_nodes = profile
            .get("graph_node_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(u64::MAX);
        let profile_graph_clones = profile
            .get("graph_clones_per_item")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(u64::MAX);
        let static_topology = profile_graph_nodes == graph_node_count && profile_graph_clones == 0;
        push_audit_check(
            checks,
            blockers,
            format!("scope:{example}:stress:{name}:static-topology"),
            static_topology,
            format!(
                "profile graph_node_count={profile_graph_nodes}, report graph_node_count={graph_node_count}, graph_clones_per_item={profile_graph_clones}"
            ),
            (!static_topology).then(|| {
                format!("{example} stress profile `{name}` changes topology or clones per item")
            }),
        );
        let render_patch_count = profile
            .get("render_patch_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(u64::MAX);
        let dirty_count = profile
            .get("dirty_key_count")
            .or_else(|| profile.get("dirty_cell_count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(u64::MAX);
        let heap_alloc_count = profile
            .get("heap_alloc_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(u64::MAX);
        let heap_alloc_bytes = profile
            .get("heap_alloc_bytes")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(u64::MAX);
        let allocation_free = heap_alloc_count == 0 && heap_alloc_bytes == 0;
        push_audit_check(
            checks,
            blockers,
            format!("scope:{example}:stress:{name}:allocation-free"),
            allocation_free,
            format!("heap_alloc_count={heap_alloc_count}, heap_alloc_bytes={heap_alloc_bytes}"),
            (!allocation_free)
                .then(|| format!("{example} stress profile `{name}` allocated after warmup")),
        );
        let expected_fanout = profile
            .get("expected_fanout")
            .and_then(serde_json::Value::as_u64);
        let expected_dirty_count = profile
            .get("expected_dirty_cell_count")
            .and_then(serde_json::Value::as_u64);
        let proportional_dirty_count = if let Some(expected_fanout) = expected_fanout {
            let allowed_dirty_count = expected_fanout.saturating_add(1);
            dirty_count == expected_dirty_count.unwrap_or(allowed_dirty_count)
                && dirty_count <= allowed_dirty_count
        } else {
            (1..=8).contains(&dirty_count)
        };
        push_audit_check(
            checks,
            blockers,
            format!("scope:{example}:stress:{name}:bounded-dirty-work"),
            proportional_dirty_count,
            format!("dirty key/cell count={dirty_count}"),
            (!proportional_dirty_count).then(|| {
                format!(
                    "{example} stress profile `{name}` touches too many dirty keys/cells for a local interaction"
                )
            }),
        );
        let proportional_patch_count = render_patch_count <= 8;
        push_audit_check(
            checks,
            blockers,
            format!("scope:{example}:stress:{name}:bounded-render-patches"),
            proportional_patch_count,
            format!("render_patch_count={render_patch_count}"),
            (!proportional_patch_count).then(|| {
                format!(
                    "{example} stress profile `{name}` emits too many render patches for a local interaction"
                )
            }),
        );
    }
    audit_documented_stress_profile_coverage(example, &path, &report, checks, blockers);
    Ok(())
}

fn audit_documented_stress_profile_coverage(
    example: &str,
    report_path: &Path,
    report: &serde_json::Value,
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) {
    let profiles = report
        .get("stress_profiles")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if example == "todomvc" {
        for required_rows in [1_000_u64, 10_000] {
            let present = profiles.iter().any(|profile| {
                profile.get("rows").and_then(serde_json::Value::as_u64) == Some(required_rows)
            });
            push_audit_check(
                checks,
                blockers,
                format!("scope:todomvc:stress:{required_rows}-rows-present"),
                present,
                format!("{} TodoMVC stress rows {required_rows}", report_path.display()),
                (!present).then(|| {
                    format!(
                        "TodoMVC speed report `{}` missing documented {required_rows}-row stress profile",
                        report_path.display()
                    )
                }),
            );
        }
        let move_present = profiles.iter().any(|profile| {
            profile
                .get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| name.contains("10000") && name.contains("move"))
        });
        push_audit_check(
            checks,
            blockers,
            "scope:todomvc:stress:10000-row-list-move-present",
            move_present,
            format!("{} TodoMVC 10k move stress", report_path.display()),
            (!move_present).then(|| {
                format!(
                    "TodoMVC speed report `{}` missing documented 10,000-row LIST move stress profile",
                    report_path.display()
                )
            }),
        );
        let all_todomvc_stress_profiles_are_ir_derived = profiles
            .iter()
            .filter(|profile| {
                profile
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|name| name.starts_with("todomvc-"))
            })
            .all(|profile| {
                profile
                    .get("ir_runtime_proof")
                    .and_then(|proof| proof.get("runtime_constructed_from_ir"))
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
                    && profile
                        .get("ir_runtime_proof")
                        .and_then(|proof| proof.get("compiled_surface"))
                        .and_then(serde_json::Value::as_str)
                        == Some("todomvc")
                    && profile
                        .get("ir_runtime_proof")
                        .and_then(|proof| proof.get("list_operation_count"))
                        .and_then(serde_json::Value::as_u64)
                        .is_some_and(|count| count > 0)
                    && profile
                        .get("ir_runtime_proof")
                        .and_then(|proof| proof.get("source_route_count"))
                        .and_then(serde_json::Value::as_u64)
                        .is_some_and(|count| count > 0)
                    && profile
                        .get("ir_runtime_proof")
                        .and_then(|proof| proof.get("row_source_binding_count"))
                        .and_then(serde_json::Value::as_u64)
                        .is_some_and(|count| count > 0)
            });
        push_audit_check(
            checks,
            blockers,
            "scope:todomvc:stress:ir-derived-runtime",
            all_todomvc_stress_profiles_are_ir_derived,
            format!(
                "{} TodoMVC stress profiles carry IR-derived runtime proof",
                report_path.display()
            ),
            (!all_todomvc_stress_profiles_are_ir_derived).then(|| {
                format!(
                    "TodoMVC speed report `{}` does not prove stress profiles were constructed from typed IR",
                    report_path.display()
                )
            }),
        );
    }
    if example == "cells" {
        for required in [
            "cells-26x100-unrelated-edit",
            "cells-26x100-dependent-edit",
            "cells-26x100-fanout-100-update",
        ] {
            let present = profiles.iter().any(|profile| {
                profile.get("name").and_then(serde_json::Value::as_str) == Some(required)
            });
            push_audit_check(
                checks,
                blockers,
                format!("scope:cells:stress:{required}:present"),
                present,
                format!("{} Cells stress profile {required}", report_path.display()),
                (!present).then(|| {
                    format!(
                        "Cells speed report `{}` missing documented stress profile `{required}`",
                        report_path.display()
                    )
                }),
            );
        }
        let fanout = profiles.iter().find(|profile| {
            profile.get("name").and_then(serde_json::Value::as_str)
                == Some("cells-26x100-fanout-100-update")
        });
        let all_cells_stress_profiles_are_ir_derived = profiles
            .iter()
            .filter(|profile| {
                profile
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|name| name.starts_with("cells-26x100-"))
            })
            .all(|profile| {
                profile
                    .get("ir_runtime_proof")
                    .and_then(|proof| proof.get("runtime_constructed_from_ir"))
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
                    && profile
                        .get("ir_runtime_proof")
                        .and_then(|proof| proof.get("compiled_surface"))
                        .and_then(serde_json::Value::as_str)
                        == Some("cells")
                    && profile
                        .get("ir_runtime_proof")
                        .and_then(|proof| proof.get("formula_operation_count"))
                        .and_then(serde_json::Value::as_u64)
                        .is_some_and(|count| count >= 4)
                    && profile
                        .get("ir_runtime_proof")
                        .and_then(|proof| proof.get("source_route_count"))
                        .and_then(serde_json::Value::as_u64)
                        .is_some_and(|count| count > 0)
            });
        push_audit_check(
            checks,
            blockers,
            "scope:cells:stress:ir-derived-runtime",
            all_cells_stress_profiles_are_ir_derived,
            format!("{} Cells stress profiles carry IR-derived runtime proof", report_path.display()),
            (!all_cells_stress_profiles_are_ir_derived).then(|| {
                format!(
                    "Cells speed report `{}` does not prove stress profiles were constructed from typed IR",
                    report_path.display()
                )
            }),
        );
        let fanout_proved = fanout.is_some_and(|profile| {
            let expected_fanout = profile
                .get("expected_fanout")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            let expected_dirty = expected_fanout.saturating_add(1);
            expected_fanout == 100
                && profile
                    .get("dirty_cell_count")
                    .and_then(serde_json::Value::as_u64)
                    == Some(expected_dirty)
                && profile
                    .get("recompute_candidate_count")
                    .and_then(serde_json::Value::as_u64)
                    == Some(expected_dirty)
                && profile
                    .get("dependency_edge_walk_count")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|walks| walks >= expected_fanout)
        });
        push_audit_check(
            checks,
            blockers,
            "scope:cells:stress:fanout-100-proved",
            fanout_proved,
            format!("{} Cells fanout 100 stress proof", report_path.display()),
            (!fanout_proved).then(|| {
                format!(
                    "Cells speed report `{}` does not prove the documented fanout_100 stress profile",
                    report_path.display()
                )
            }),
        );
    }
}

fn audit_todomvc_hardware_plan(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let candidates = [
        PathBuf::from("target/reports/todomvc-hardware.json"),
        PathBuf::from("target/reports/todomvc-fpga-plan.json"),
    ];
    let Some(path) = candidates.iter().find(|path| path.exists()) else {
        push_audit_check(
            checks,
            blockers,
            "todomvc:hardware-plan:present",
            false,
            "missing TodoMVC hardware explanation report",
            Some("TodoMVC hardware explanation report is missing".to_owned()),
        );
        return Ok(());
    };
    match verify_report_schema(path) {
        Ok(()) => push_audit_check(
            checks,
            blockers,
            "todomvc:hardware-plan:schema",
            true,
            format!("{} schema valid", path.display()),
            None,
        ),
        Err(error) => {
            push_audit_check(
                checks,
                blockers,
                "todomvc:hardware-plan:schema",
                false,
                error.to_string(),
                Some(format!(
                    "TodoMVC hardware explanation report `{}` is not schema-valid: {error}",
                    path.display()
                )),
            );
            return Ok(());
        }
    }
    let report = read_json(path)?;
    let plan = report
        .get("hardware_plan")
        .unwrap_or(&serde_json::Value::Null);
    for (id, pass, blocker) in [
        (
            "no-app-visible-ids-required",
            plan.get("app_visible_ids_required")
                .and_then(serde_json::Value::as_bool)
                == Some(false),
            "TodoMVC FPGA plan must not require app-visible ids",
        ),
        (
            "hidden-slot-generation-storage",
            plan.get("hidden_slot_generation_storage")
                .and_then(serde_json::Value::as_bool)
                == Some(true),
            "TodoMVC FPGA plan must include hidden slot/generation storage",
        ),
        (
            "delta-output-fifo",
            plan.get("delta_output_fifo")
                .and_then(serde_json::Value::as_bool)
                == Some(true),
            "TodoMVC FPGA plan must emit deltas, not whole snapshots",
        ),
        (
            "register-file-fields-source-derived",
            plan.get("register_file_fields_source_derived")
                .and_then(serde_json::Value::as_bool)
                == Some(true),
            "TodoMVC FPGA register-file fields must be source-derived from IR",
        ),
    ] {
        push_audit_check(
            checks,
            blockers,
            format!("todomvc:hardware-plan:{id}"),
            pass,
            format!("checked {}", path.display()),
            (!pass).then(|| blocker.to_owned()),
        );
    }
    let internal_identity = plan
        .get("internal_list_identity")
        .unwrap_or(&serde_json::Value::Null);
    let hidden_identity_kept_internal = internal_identity
        .get("visible_to_boon")
        .and_then(serde_json::Value::as_bool)
        == Some(false)
        && internal_identity
            .get("boon_equality")
            .and_then(serde_json::Value::as_str)
            == Some("data_only");
    push_audit_check(
        checks,
        blockers,
        "todomvc:hardware-plan:hidden-identity-not-boon-data",
        hidden_identity_kept_internal,
        format!("internal_list_identity={internal_identity}"),
        (!hidden_identity_kept_internal).then(|| {
            "TodoMVC FPGA internal slot/generation identity must stay hidden from Boon equality/data".to_owned()
        }),
    );
    let source_bus = plan
        .get("source_event_bus")
        .unwrap_or(&serde_json::Value::Null);
    let source_bus_hidden_and_checked = source_bus
        .get("decoded_from_source_bindings")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && source_bus
            .get("generation_checked_before_pulse")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && source_bus
            .get("source_ids_visible_to_boon")
            .and_then(serde_json::Value::as_bool)
            == Some(false);
    push_audit_check(
        checks,
        blockers,
        "todomvc:hardware-plan:source-bus-hidden-and-generation-checked",
        source_bus_hidden_and_checked,
        format!("source_event_bus={source_bus}"),
        (!source_bus_hidden_and_checked).then(|| {
            "TodoMVC FPGA source-event bus must decode hidden bindings, check generation, and hide source ids from Boon".to_owned()
        }),
    );
    let unsupported_values = plan
        .get("unsupported_as_boon_values")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect::<BTreeSet<_>>();
    let hidden_runtime_values_blocked =
        ["slot", "generation", "source_id", "bind_epoch", "ListKey"]
            .into_iter()
            .all(|value| unsupported_values.contains(value));
    push_audit_check(
        checks,
        blockers,
        "todomvc:hardware-plan:hidden-runtime-values-not-boon-values",
        hidden_runtime_values_blocked,
        format!("unsupported_as_boon_values={unsupported_values:?}"),
        (!hidden_runtime_values_blocked).then(|| {
            "TodoMVC FPGA plan must mark slot/generation/source_id/bind_epoch/ListKey as unsupported Boon-visible values".to_owned()
        }),
    );
    let bounded_profile = plan
        .get("bounded_storage_profile")
        .unwrap_or(&serde_json::Value::Null);
    let fpga_profile_selected = bounded_profile
        .get("name")
        .and_then(serde_json::Value::as_str)
        == Some("fpga_todomvc");
    push_audit_check(
        checks,
        blockers,
        "todomvc:hardware-plan:fpga-profile-selected",
        fpga_profile_selected,
        format!(
            "bounded_storage_profile.name={:?}",
            bounded_profile.get("name")
        ),
        (!fpga_profile_selected).then(|| {
            "TodoMVC hardware plan must be generated with `fpga_todomvc` profile".to_owned()
        }),
    );
    let todos_capacity = bounded_profile
        .get("todos_capacity")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    push_audit_check(
        checks,
        blockers,
        "todomvc:hardware-plan:todos-capacity",
        todos_capacity > 0,
        format!("todos_capacity={todos_capacity}"),
        (todos_capacity == 0)
            .then(|| "TodoMVC FPGA profile must provide a positive todos capacity".to_owned()),
    );
    let title_width = bounded_profile
        .get("todo_title_width")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let edit_width = bounded_profile
        .get("todo_edit_text_width")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    push_audit_check(
        checks,
        blockers,
        "todomvc:hardware-plan:fixed-text-widths",
        title_width > 0 && edit_width > 0,
        format!("todo_title_width={title_width}, todo_edit_text_width={edit_width}"),
        (title_width == 0 || edit_width == 0)
            .then(|| "TodoMVC FPGA profile must provide fixed title/edit text widths".to_owned()),
    );
    let bounded_text = bounded_profile
        .get("unbounded_text_allowed")
        .and_then(serde_json::Value::as_bool)
        == Some(false);
    push_audit_check(
        checks,
        blockers,
        "todomvc:hardware-plan:unbounded-text-rejected",
        bounded_text,
        format!(
            "unbounded_text_allowed={:?}",
            bounded_profile.get("unbounded_text_allowed")
        ),
        (!bounded_text)
            .then(|| "TodoMVC FPGA profile must reject unbounded text storage".to_owned()),
    );
    let fixed_text = plan
        .get("fixed_text_storage")
        .unwrap_or(&serde_json::Value::Null);
    let fixed_text_matches_profile = [("todo.title", title_width), ("todo.edit_text", edit_width)]
        .into_iter()
        .all(|(field, width)| {
            fixed_text
                .get(field)
                .and_then(|entry| entry.get("width"))
                .and_then(serde_json::Value::as_u64)
                == Some(width)
                && fixed_text
                    .get(field)
                    .and_then(|entry| entry.get("encoding"))
                    .and_then(serde_json::Value::as_str)
                    == Some("ascii")
        });
    push_audit_check(
        checks,
        blockers,
        "todomvc:hardware-plan:fixed-text-storage-matches-profile",
        fixed_text_matches_profile,
        format!("fixed_text_storage={fixed_text}"),
        (!fixed_text_matches_profile).then(|| {
            "TodoMVC FPGA fixed text storage must match profile widths and ASCII encoding"
                .to_owned()
        }),
    );
    let fifo_caps = [
        (
            "input_event_fifo",
            plan.get("input_event_fifo")
                .and_then(|fifo| fifo.get("capacity"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default(),
        ),
        (
            "output_delta_fifo",
            plan.get("output_delta_fifo")
                .and_then(|fifo| fifo.get("capacity"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default(),
        ),
    ];
    let fifo_caps_present = fifo_caps.iter().all(|(_, capacity)| *capacity > 0);
    push_audit_check(
        checks,
        blockers,
        "todomvc:hardware-plan:fifo-capacities",
        fifo_caps_present,
        format!("fifo capacities={fifo_caps:?}"),
        (!fifo_caps_present)
            .then(|| "TodoMVC FPGA plan must include input and output FIFO capacities".to_owned()),
    );
    let effective_list_capacity = plan
        .get("list_storage")
        .and_then(|storage| storage.get("list_memories"))
        .and_then(serde_json::Value::as_array)
        .and_then(|lists| {
            lists
                .iter()
                .find(|list| list.get("name").and_then(serde_json::Value::as_str) == Some("todos"))
        })
        .and_then(|todos| todos.get("effective_capacity"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    push_audit_check(
        checks,
        blockers,
        "todomvc:hardware-plan:effective-list-capacity",
        effective_list_capacity == todos_capacity && effective_list_capacity > 0,
        format!(
            "effective_list_capacity={effective_list_capacity}, profile todos_capacity={todos_capacity}"
        ),
        (effective_list_capacity != todos_capacity || effective_list_capacity == 0).then(|| {
            "TodoMVC FPGA list memory must use the profile's effective bounded capacity".to_owned()
        }),
    );
    let row_sources = plan
        .get("row_source_ports")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let row_sources_are_local = !row_sources.is_empty()
        && row_sources.iter().all(|source| {
            source
                .as_str()
                .is_some_and(|source| source.starts_with("todo.sources."))
        });
    push_audit_check(
        checks,
        blockers,
        "todomvc:hardware-plan:row-source-ports-local",
        row_sources_are_local,
        format!("checked {} row source ports", row_sources.len()),
        (!row_sources_are_local).then(|| {
            "TodoMVC FPGA row source ports must only contain row-local todo sources".to_owned()
        }),
    );
    Ok(())
}

fn audit_xtask_command_surface(checks: &mut Vec<serde_json::Value>, blockers: &mut Vec<String>) {
    for command in XTASK_COMMANDS.iter().copied() {
        push_audit_check(
            checks,
            blockers,
            format!("xtask-command:{command}"),
            xtask_command_supported(command),
            "documented command is supported by xtask".to_owned(),
            (!xtask_command_supported(command))
                .then(|| format!("documented xtask command `{command}` is not implemented")),
        );
    }
}

#[allow(dead_code)]
fn documented_xtask_commands() -> &'static [&'static str] {
    &[
        "verify-example-headed-ply",
        "verify-example-human",
        "prepare-example-human-report",
        "verify-example-semantic",
        "verify-example-ply-headless",
        "verify-example-speed",
        "verify-example-negative",
        "verify-example-all",
        "verify-examples-all",
        "verify-os-input-probe",
        "verify-foundation",
        "verify-playground-launch",
        "verify-playground-background-launch",
        "bench-example",
        "verify-playground-custom-source",
        "write-manual-handoff",
        "verify-report-schema",
        "audit-goal-readiness",
        "audit-manual-readiness",
        "verify-todomvc-headed-ply",
        "verify-todomvc-human",
        "prepare-todomvc-human-report",
        "verify-todomvc-semantic",
        "verify-todomvc-ply-headless",
        "verify-todomvc-speed",
        "verify-todomvc-negative",
        "verify-todomvc-all",
        "bench-todomvc",
        "explain-todomvc-hardware",
        "verify-cells-headed-ply",
        "verify-cells-human",
        "prepare-cells-human-report",
        "verify-cells-semantic",
        "verify-cells-ply-headless",
        "verify-cells-speed",
        "verify-cells-negative",
        "verify-cells-all",
    ]
}

fn audit_manual_handoff(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from("target/reports/manual-handoff.json");
    if !path.exists() {
        push_audit_check(
            checks,
            blockers,
            "manual-handoff:report-present",
            false,
            format!("missing {}", path.display()),
            Some(format!(
                "missing manual handoff report `{}`; run `cargo xtask write-manual-handoff`",
                path.display()
            )),
        );
        return Ok(());
    }
    match verify_report_schema(&path) {
        Ok(()) => push_audit_check(
            checks,
            blockers,
            "manual-handoff:schema",
            true,
            format!("{} schema valid", path.display()),
            None,
        ),
        Err(error) => {
            push_audit_check(
                checks,
                blockers,
                "manual-handoff:schema",
                false,
                error.to_string(),
                Some(format!(
                    "manual handoff report `{}` is not schema-valid: {error}",
                    path.display()
                )),
            );
            return Ok(());
        }
    }
    let report = read_json(&path)?;
    for artifact in [
        PathBuf::from("docs/plans/MANUAL_TESTING_RUNBOOK.md"),
        manual_template_path("todomvc"),
        manual_template_path("cells"),
    ] {
        let current_hash = boon_runtime::sha256_file(&artifact)?;
        let artifact_hash_matches = report_artifact_hash_matches(&report, &artifact, &current_hash);
        push_audit_check(
            checks,
            blockers,
            format!(
                "manual-handoff:artifact-hash:{}",
                sanitize_audit_id(&artifact.display().to_string())
            ),
            artifact_hash_matches,
            format!(
                "manual handoff hashes current artifact {}",
                artifact.display()
            ),
            (!artifact_hash_matches).then(|| {
                format!(
                    "manual handoff report `{}` does not hash current artifact `{}`",
                    path.display(),
                    artifact.display()
                )
            }),
        );
    }
    let template_paths = report
        .get("manual_template_paths")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for template in [
        manual_template_path("todomvc"),
        manual_template_path("cells"),
    ] {
        let template_string = template.display().to_string();
        let present = template_paths
            .iter()
            .any(|path| path.as_str() == Some(template_string.as_str()));
        push_audit_check(
            checks,
            blockers,
            format!(
                "manual-handoff:template-path:{}",
                sanitize_audit_id(&template_string)
            ),
            present,
            format!("manual handoff names template {template_string}"),
            (!present).then(|| {
                format!(
                    "manual handoff report `{}` does not name template `{template_string}`",
                    path.display()
                )
            }),
        );
    }
    for command in [
        "cargo xtask prepare-todomvc-human-report",
        "cargo xtask prepare-cells-human-report",
        "cargo xtask verify-todomvc-human --write-template --report target/reports/manual-templates/todomvc-human.json",
        "cargo xtask verify-cells-human --write-template --report target/reports/manual-templates/cells-human.json",
        "--window-pid <visible-playground-pid>",
        "--focused-window-proof <how-focus-was-confirmed>",
        "--pass-label <each-todomvc-scenario-label>",
        "--pass-label <each-cells-scenario-label>",
        "cargo xtask bench-todomvc",
        "cargo xtask bench-example cells",
        "cargo xtask verify-playground-custom-source",
        "cargo xtask verify-os-input-probe --report target/reports/os-input-probe.json",
        "BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-todomvc-headed-ply",
        "BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-cells-headed-ply",
        "cargo xtask verify-todomvc-negative",
        "cargo xtask verify-cells-negative",
        "cargo xtask verify-report-schema",
        "cargo bench -p boon_runtime --bench todomvc -- --report target/reports/todomvc-bench.json --speed-report target/reports/todomvc-bench-speed.json",
        "cargo xtask explain-todomvc-hardware --report target/reports/todomvc-hardware.json",
        "cargo run -p boon_cli -- run examples/todomvc.bn --scenario examples/todomvc.scn --report target/reports/todomvc-cli-run.json",
        "cargo run -p boon_cli -- run examples/cells.bn --scenario examples/cells.scn --report target/reports/cells-cli-run.json",
        "cargo xtask verify-playground-background-launch",
        "cosmic-background-launch --workspace boon-circuit",
        "cargo xtask verify-todomvc-all --check-existing",
        "cargo xtask verify-cells-all --check-existing",
        "cargo xtask verify-examples-all --check-existing --report target/reports/examples-all.json",
        "cargo xtask audit-manual-readiness --report target/reports/debug/manual-readiness.json",
        "cargo xtask audit-goal-readiness --report target/reports/debug/goal-readiness.json",
    ] {
        let present = report
            .get("manual_testing_commands")
            .is_some_and(|commands| commands.to_string().contains(command));
        push_audit_check(
            checks,
            blockers,
            format!("manual-handoff:command:{}", command.replace(' ', "-")),
            present,
            format!("manual handoff includes `{command}`"),
            (!present).then(|| {
                format!(
                    "manual handoff report `{}` does not include `{command}`",
                    path.display()
                )
            }),
        );
    }
    Ok(())
}

fn audit_repo_handoff_docs(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let docs = [
        (
            "README.md",
            vec![
                "docs/plans/MANUAL_TESTING_RUNBOOK.md",
                "manual_report_prepared_by",
                "BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-todomvc-headed-ply",
                "cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --example todomvc",
                "cargo bench -p boon_runtime --bench todomvc",
                "cargo xtask verify-examples-all --check-existing --report target/reports/examples-all.json",
            ],
        ),
        (
            "AGENTS.md",
            vec![
                "Do not commit or push unless the user explicitly asks.",
                "Do not fabricate `target/reports/todomvc-human.json`",
                "manual_report_prepared_by",
                "cosmic-background-launch --workspace boon-circuit -- cargo run -p boon_ply_playground -- --example todomvc",
                "BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-cells-headed-ply",
                "cargo xtask audit-goal-readiness --report target/reports/debug/goal-readiness.json",
            ],
        ),
    ];
    for (path, required_texts) in docs {
        let path_ref = Path::new(path);
        if !path_ref.exists() {
            push_audit_check(
                checks,
                blockers,
                format!("repo-handoff-doc:{path}:present"),
                false,
                format!("{path} is missing"),
                Some(format!("repo handoff guidance file `{path}` is missing")),
            );
            continue;
        }
        let text = std::fs::read_to_string(path_ref)?;
        push_audit_check(
            checks,
            blockers,
            format!("repo-handoff-doc:{path}:present"),
            true,
            format!("{path} exists"),
            None,
        );
        for needle in required_texts {
            let present = text.contains(needle);
            push_audit_check(
                checks,
                blockers,
                format!("repo-handoff-doc:{path}:{}", sanitize_audit_id(needle)),
                present,
                format!("{path} contains `{needle}`"),
                (!present).then(|| format!("repo handoff guidance `{path}` is missing `{needle}`")),
            );
        }
    }
    Ok(())
}

fn sanitize_audit_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect()
}

fn xtask_command_supported(command: &str) -> bool {
    matches!(
        command,
        "verify-example-semantic"
            | "verify-example-ply-headless"
            | "verify-example-headed-ply"
            | "verify-example-human"
            | "prepare-example-human-report"
            | "verify-example-speed"
            | "verify-example-negative"
            | "verify-example-all"
            | "verify-examples-all"
            | "verify-os-input-probe"
            | "verify-foundation"
            | "verify-playground-launch"
            | "verify-playground-background-launch"
            | "verify-playground-custom-source"
            | "write-manual-handoff"
            | "verify-report-schema"
            | "audit-goal-readiness"
            | "audit-manual-readiness"
            | "bench-example"
            | "verify-todomvc-semantic"
            | "verify-todomvc-ply-headless"
            | "verify-todomvc-headed-ply"
            | "verify-todomvc-human"
            | "prepare-todomvc-human-report"
            | "verify-todomvc-speed"
            | "verify-todomvc-negative"
            | "verify-todomvc-all"
            | "bench-todomvc"
            | "explain-todomvc-hardware"
            | "verify-cells-semantic"
            | "verify-cells-ply-headless"
            | "verify-cells-headed-ply"
            | "verify-cells-human"
            | "prepare-cells-human-report"
            | "verify-cells-speed"
            | "verify-cells-negative"
            | "verify-cells-all"
    )
}

fn read_json(path: &Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn push_audit_check(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
    id: impl Into<String>,
    pass: bool,
    detail: impl Into<String>,
    blocker: Option<String>,
) {
    let id = id.into();
    checks.push(json!({
        "id": id,
        "pass": pass,
        "detail": detail.into()
    }));
    if !pass {
        if let Some(blocker) = blocker {
            blockers.push(blocker);
        } else {
            blockers.push(id);
        }
    }
}

fn verify_all_with_optional_report(
    name: &str,
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let check_existing = args.iter().any(|arg| arg == "--check-existing");
    let mut reports = Vec::new();
    for layer in [
        VerificationLayer::Semantic,
        VerificationLayer::HeadlessPly,
        VerificationLayer::HeadedPly,
        VerificationLayer::Speed,
        VerificationLayer::Negative,
        VerificationLayer::Human,
    ] {
        let report = report_path(name, layer);
        let result = if check_existing {
            verify_existing_layer_report(name, layer, &report)
        } else if matches!(layer, VerificationLayer::Human) {
            verify_existing_layer_report(name, layer, &report)
        } else if matches!(layer, VerificationLayer::HeadedPly)
            && std::env::var("BOON_ALLOW_OS_POINTER_PROBE").as_deref() != Ok("1")
        {
            verify_existing_full_headed_report(name, &report)
        } else if matches!(layer, VerificationLayer::Negative) {
            verify_negative_name(name)
        } else {
            verify_specific(name, layer, &[])
        };
        if let Err(error) = result {
            write_all_blocked_debug_report(
                name,
                args,
                &reports,
                layer,
                &report,
                &error.to_string(),
            )?;
            return Err(error);
        }
        reports.push(report);
    }
    for report in &reports {
        verify_report_schema(report)?;
    }
    let aggregate = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-all",
        "command_argv": args,
        "exit_status": 0,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_hash": file_hash(&format!("examples/{name}.bn")),
        "scenario_hash": file_hash(&format!("examples/{name}.scn")),
        "program_hash": file_hash(&format!("examples/{name}.bn")),
        "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
        "graph_node_count": "see layer reports",
        "per_step_pass_fail": [],
        "artifact_sha256s": reports.iter().map(|path| json!({
            "path": path,
            "sha256": boon_runtime::sha256_file(path).unwrap_or_else(|_| "missing".to_owned())
        })).collect::<Vec<_>>(),
        "layer_reports": reports,
    });
    let aggregate_path =
        report_arg(args).unwrap_or_else(|| report_path(name, VerificationLayer::All));
    write_json(&aggregate_path, &aggregate)?;
    let _ = std::fs::remove_file(format!("target/reports/debug/{name}-all-blocked.json"));
    verify_report_schema(&aggregate_path)?;
    Ok(())
}

fn verify_existing_layer_report(
    name: &str,
    layer: VerificationLayer,
    report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if !report.exists() {
        return Err(format!(
            "missing existing {} report `{}` for `{name}`",
            layer.as_str(),
            report.display()
        )
        .into());
    }
    if matches!(layer, VerificationLayer::Human) {
        verify_human_report(report, 24 * 60 * 60)
    } else if matches!(layer, VerificationLayer::HeadedPly) {
        verify_existing_full_headed_report(name, report)
    } else {
        verify_report_schema(report)
    }
}

fn verify_existing_full_headed_report(
    name: &str,
    report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if !report.exists() {
        return Err(format!(
            "missing existing full headed report `{}` for `{name}`; run `BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-{name}-headed-ply` before the aggregate gate",
            report.display()
        )
        .into());
    }
    verify_report_schema(report)?;
    let report_json = read_json(report)?;
    if report_json
        .get("input_injection_method")
        .and_then(serde_json::Value::as_str)
        != Some("os_pointer_keyboard_to_visible_window")
    {
        return Err(format!(
            "{name} headed report `{}` is not full OS pointer/keyboard evidence; run `BOON_ALLOW_OS_POINTER_PROBE=1 cargo xtask verify-{name}-headed-ply`",
            report.display()
        )
        .into());
    }
    if report_json.get("os_input_limitation").is_some() {
        return Err(format!(
            "{name} headed report `{}` still carries os_input_limitation",
            report.display()
        )
        .into());
    }
    let missing = report_json
        .get("os_input_coverage")
        .and_then(|coverage| coverage.get("missing_full_os_pointer_keyboard_steps"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(Vec::is_empty);
    if !missing {
        return Err(format!(
            "{name} headed report `{}` has missing full OS input labels",
            report.display()
        )
        .into());
    }
    let scenario = boon_runtime::parse_scenario(Path::new(&format!("examples/{name}.scn")))?;
    let os_input_step_count = report_json
        .get("os_input_steps")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();
    if os_input_step_count < scenario.step.len() {
        return Err(format!(
            "{name} headed report `{}` has {os_input_step_count} OS input steps for {} scenario labels",
            report.display(),
            scenario.step.len()
        )
        .into());
    }
    Ok(())
}

fn write_all_blocked_debug_report(
    name: &str,
    args: &[String],
    completed_reports: &[PathBuf],
    blocked_layer: VerificationLayer,
    blocked_report: &Path,
    error: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(format!("target/reports/debug/{name}-all-blocked.json"));
    let artifact_sha256s = completed_reports
        .iter()
        .map(|path| artifact_hash(path))
        .collect::<Result<Vec<_>, _>>()?;
    let report = json!({
        "status": "fail",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-all-blocked",
        "command_argv": args,
        "exit_status": 1,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_hash": file_hash(&format!("examples/{name}.bn")),
        "scenario_hash": file_hash(&format!("examples/{name}.scn")),
        "program_hash": file_hash(&format!("examples/{name}.bn")),
        "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
        "graph_node_count": "see completed layer reports",
        "per_step_pass_fail": [{
            "id": format!("{name}:{}:blocked", blocked_layer.as_str()),
            "pass": false,
            "detail": error
        }],
        "artifact_sha256s": artifact_sha256s,
        "blocked_layer": blocked_layer.as_str(),
        "blocked_report": blocked_report,
        "completed_layer_reports": completed_reports,
        "blocker": error,
        "note": "debug-only failure artifact; the top-level all report is intentionally not written until every required layer, including real human verification, passes"
    });
    write_json(&path, &report)?;
    Ok(())
}

fn verify_negative(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    verify_negative_name(named_arg(args, 1)?)
}

fn verify_negative_name(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (source, scenario, _) = example_paths(name)?;
    let scenario_data = boon_runtime::parse_scenario(&scenario)?;
    let all_true_checklist = scenario_data
        .step
        .iter()
        .map(|step| (step.id.clone(), json!(true)))
        .collect::<serde_json::Map<_, _>>();
    let text = std::fs::read_to_string(&source)?;
    let bad_source = format!("{text}\nruntime_key: TEXT {{ leak }}\n");
    let rejected = boon_parser::parse_source(source.display().to_string(), bad_source).is_err();
    if !rejected {
        return Err(format!("{name} negative hidden-identity fixture did not fail").into());
    }
    let app_visible_identity_rejected = if name == "todomvc" {
        let bad_visible_identity_source = format!("{text}\nid: TEXT {{ exposed-id }}\n");
        boon_parser::parse_source(source.display().to_string(), bad_visible_identity_source)
            .is_err()
    } else {
        true
    };
    if !app_visible_identity_rejected {
        return Err(format!("{name} negative app-visible identity fixture did not fail").into());
    }
    let stale_hash_rejected = schema_rejects(&negative_fixture(
        name,
        "stale-source-hash",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "semantic",
            "git_commit": git_commit(),
            "source_path": source,
            "source_hash": "bad-source-hash",
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "per_step_pass_fail": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let stale_scenario_hash_rejected = schema_rejects(&negative_fixture(
        name,
        "stale-scenario-hash",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "semantic",
            "git_commit": git_commit(),
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": "bad-scenario-hash",
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "per_step_pass_fail": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let missing_screenshot_rejected = schema_rejects(&negative_fixture(
        name,
        "missing-headed-screenshot",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "headed-ply",
            "layer": "headed-ply",
            "git_commit": git_commit(),
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "per_step_pass_fail": [],
            "artifact_sha256s": [],
            "nonblank_screenshot_hashes": []
        }),
    )?)?;
    let direct_injection_rejected = schema_rejects(&negative_fixture(
        name,
        "direct-source-injection",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "headed-ply",
            "layer": "headed-ply",
            "input_injection_method": "direct_source_event",
            "git_commit": git_commit(),
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "per_step_pass_fail": [],
            "artifact_sha256s": [{
                "path": format!("examples/{name}.bn"),
                "sha256": file_hash(&format!("examples/{name}.bn"))
            }],
            "nonblank_screenshot_hashes": [{
                "nonzero_channels": 1,
                "unique_rgba_values": 2
            }]
        }),
    )?)?;
    let missing_os_limitation_rejected = schema_rejects(&negative_fixture(
        name,
        "missing-os-input-limitation",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "headed-ply",
            "layer": "headed-ply",
            "input_injection_method": "scenario_user_action_route_then_headed_render_no_os_input",
            "input_route_contract": "scenario route only",
            "git_commit": git_commit(),
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "per_step_pass_fail": [],
            "artifact_sha256s": [{
                "path": format!("examples/{name}.bn"),
                "sha256": file_hash(&format!("examples/{name}.bn"))
            }],
            "nonblank_screenshot_hashes": [{
                "nonzero_channels": 1,
                "unique_rgba_values": 2
            }]
        }),
    )?)?;
    let fake_full_os_input_rejected = schema_rejects(&negative_fixture(
        name,
        "fake-full-os-input",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "headed-ply",
            "layer": "headed-ply",
            "input_injection_method": "os_pointer_keyboard_to_visible_window",
            "input_route_contract": "claims full OS route without step evidence",
            "os_input_probe": {"status": "pass"},
            "git_commit": git_commit(),
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "per_step_pass_fail": [],
            "artifact_sha256s": [{
                "path": format!("examples/{name}.bn"),
                "sha256": file_hash(&format!("examples/{name}.bn"))
            }],
            "nonblank_screenshot_hashes": [{
                "nonzero_channels": 1,
                "unique_rgba_values": 2
            }]
        }),
    )?)?;
    let fake_full_os_steps_without_visible_coverage = scenario_data
        .step
        .iter()
        .map(|step| {
            let source_event_observed = step
                .expected_source_event
                .as_ref()
                .and_then(|expected| expected.get("source"))
                .and_then(|source| source.as_str())
                .map(|source| json!({"source": source}))
                .unwrap_or_else(|| json!(null));
            json!({
                "id": step.id,
                "pass": true,
                "target_element_id": "fixture-visible-control",
                "visible_bounds": {"x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0},
                "screenshot_path": format!("examples/{name}.bn"),
                "source_event_observed": source_event_observed
            })
        })
        .collect::<Vec<_>>();
    let fake_full_os_without_visible_coverage_rejected = schema_rejects(&negative_fixture(
        name,
        "fake-full-os-input-without-visible-coverage",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "headed-ply",
            "command_argv": ["cargo", "xtask", format!("verify-{name}-headed-ply")],
            "exit_status": 0,
            "layer": "headed-ply",
            "input_injection_method": "os_pointer_keyboard_to_visible_window",
            "input_route_contract": "claims full OS route with os_input_steps but no visible source-event or Step-control coverage",
            "os_input_probe": {"status": "pass"},
            "git_commit": git_commit(),
            "binary_hash": current_binary_hash(),
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
            "graph_node_count": 0,
            "per_step_pass_fail": [],
            "artifact_sha256s": [{
                "path": format!("examples/{name}.bn"),
                "sha256": file_hash(&format!("examples/{name}.bn"))
            }],
            "nonblank_screenshot_hashes": [{
                "nonzero_channels": 1,
                "unique_rgba_values": 2
            }],
            "window_pid": std::process::id(),
            "window_title": "Boon Circuit Ply Playground",
            "display_server": "wayland",
            "display_socket_or_compositor_connection": "wayland-1",
            "display_scale": 1.0,
            "window_size": [800.0, 600.0],
            "input_backend": "negative-fixture-os-input",
            "capture_backend": "negative-fixture-capture",
            "focused_window_proof": "negative fixture claims focus",
            "checkpoint_screenshot_or_video_paths": [format!("examples/{name}.bn")],
            "os_input_steps": fake_full_os_steps_without_visible_coverage
        }),
    )?)?;
    let missing_headed_metadata_rejected = schema_rejects(&negative_fixture(
        name,
        "missing-headed-metadata",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "headed-ply",
            "command_argv": ["cargo", "xtask", format!("verify-{name}-headed-ply")],
            "exit_status": 0,
            "layer": "headed-ply",
            "input_injection_method": "scenario_user_action_route_then_headed_render_no_os_input",
            "input_route_contract": "scenario route only",
            "os_input_limitation": "negative fixture omits headed window/display metadata",
            "git_commit": git_commit(),
            "binary_hash": current_binary_hash(),
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
            "graph_node_count": 0,
            "per_step_pass_fail": [],
            "artifact_sha256s": [{
                "path": format!("examples/{name}.bn"),
                "sha256": file_hash(&format!("examples/{name}.bn"))
            }],
            "nonblank_screenshot_hashes": [{
                "nonzero_channels": 1,
                "unique_rgba_values": 2
            }]
        }),
    )?)?;
    let stale_manual_rejected = human_report_rejects(&negative_fixture(
        name,
        "stale-human-report",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": "1",
            "command": "human",
            "layer": "human",
            "git_commit": git_commit(),
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "manual_observer": "fixture",
            "manual_checklist_pass_fail": {"initial": true},
            "per_step_pass_fail": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let handwritten_manual_rejected = human_report_rejects(&negative_fixture(
        name,
        "handwritten-human-report",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "human",
            "command_argv": ["cargo", "xtask", format!("verify-{name}-human"), "--check"],
            "layer": "human",
            "exit_status": 0,
            "git_commit": git_commit(),
            "binary_hash": "fixture-binary",
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
            "graph_node_count": 0,
            "manual_observer": "Manual Tester",
            "manual_input_route": "human_visible_window",
            "input_injection_method": "human_visible_window",
            "manual_artifact_capture_method": "desktop screenshot tool",
            "manual_started_at_utc": current_unix_seconds().saturating_sub(10).to_string(),
            "manual_finished_at_utc": current_unix_seconds().to_string(),
            "manual_session_duration_seconds": "10",
            "display_server": "wayland",
            "display_socket_or_compositor_connection": "wayland-1",
            "window_backend": "ply-engine/macroquad",
            "display_scale": 1.0,
            "window_pid": std::process::id(),
            "window_title": "Boon Circuit Ply Playground",
            "input_backend": "human-visible-window-pointer-keyboard",
            "capture_backend": "desktop screenshot tool",
            "focused_window_proof": "negative fixture focus proof",
            "manual_notes": "handwritten fixture with no helper provenance",
            "manual_checklist_pass_fail": all_true_checklist.clone(),
            "visual_checkpoint_pass_fail": [],
            "per_step_pass_fail": [],
            "checkpoint_screenshot_or_video_paths": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let scripted_manual_rejected = human_report_rejects(&negative_fixture(
        name,
        "scripted-human-placeholder",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "human",
            "layer": "human",
            "git_commit": git_commit(),
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "manual_observer": std::env::var("USER").unwrap_or_else(|_| "unknown".to_owned()),
            "manual_checklist_pass_fail": {"all_scripted_labels": true},
            "per_step_pass_fail": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let template_placeholder_rejected = human_report_rejects(&negative_fixture(
        name,
        "template-human-placeholder",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "human",
            "command_argv": ["cargo", "xtask", format!("verify-{name}-human"), "--check"],
            "layer": "human",
            "exit_status": 0,
            "git_commit": git_commit(),
            "binary_hash": "copy-from-headed-report-or-current-verifier",
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
            "graph_node_count": 0,
            "manual_observer": "fill-real-observer-name",
            "manual_input_route": "human_visible_window",
            "display_server": "copy-from-headed-report-or-fill-live-desktop",
            "window_backend": "ply-engine/macroquad",
            "display_scale": "copy-from-headed-report-or-fill-live-desktop",
            "window_title": "Boon Circuit Ply Playground",
            "manual_notes": "fill visual quality notes and any deviations",
            "manual_checklist_pass_fail": {"initial": true},
            "per_step_pass_fail": [],
            "checkpoint_screenshot_or_video_paths": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let scenario_labels = scenario_data
        .step
        .iter()
        .map(|step| step.id.clone())
        .collect::<Vec<_>>();
    let prepare_pass_labels_enforced =
        prepare_human_report_rejects_bad_pass_labels(name, &scenario_labels)?;
    let manual_fixture_finished_at = current_unix_seconds();
    let manual_fixture_started_at = manual_fixture_finished_at.saturating_sub(10);
    let missing_headed_binding_rejected = human_report_rejects(&negative_fixture(
        name,
        "missing-headed-report-binding",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "human",
            "command_argv": ["cargo", "xtask", format!("verify-{name}-human"), "--check"],
            "layer": "human",
            "exit_status": 0,
            "git_commit": git_commit(),
            "binary_hash": "fixture-binary",
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
            "graph_node_count": 0,
            "headed_report_path": format!("examples/{name}.bn"),
            "headed_report_sha256": file_hash(&format!("examples/{name}.bn")),
            "headed_input_injection_method": "os_pointer_keyboard_to_visible_window",
            "headed_os_input_step_count": scenario_data.step.len(),
            "headed_os_input_missing_labels": [],
            "manual_observer": "Manual Tester",
            "manual_input_route": "human_visible_window",
            "manual_started_at_utc": manual_fixture_started_at.to_string(),
            "manual_finished_at_utc": manual_fixture_finished_at.to_string(),
            "manual_session_duration_seconds": "10",
            "display_server": "wayland",
            "window_backend": "ply-engine/macroquad",
            "display_scale": 1.0,
            "window_title": "Boon Circuit Ply Playground",
            "manual_notes": "fixture notes",
            "manual_checklist_pass_fail": all_true_checklist.clone(),
            "per_step_pass_fail": [],
            "checkpoint_screenshot_or_video_paths": [format!("target/reports/{name}-human-fixture.png")],
            "artifact_sha256s": [{
                "path": format!("examples/{name}.bn"),
                "sha256": file_hash(&format!("examples/{name}.bn"))
            }]
        }),
    )?)?;
    let headed_only_manual_artifacts_rejected = if report_path(name, VerificationLayer::HeadedPly)
        .exists()
    {
        let headed_report_path = report_path(name, VerificationLayer::HeadedPly);
        let headed_report = read_json(&headed_report_path)?;
        let headed_artifact = headed_report
            .get("artifact_sha256s")
            .and_then(serde_json::Value::as_array)
            .and_then(|artifacts| {
                artifacts.iter().find(|artifact| {
                    artifact
                        .get("path")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|path| path.ends_with(".png"))
                })
            })
            .cloned()
            .ok_or("headed report has no png artifact for negative fixture")?;
        let headed_artifact_path = headed_artifact
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or("headed artifact missing path")?
            .to_owned();
        let manual_fixture_finished_at = current_unix_seconds();
        let manual_fixture_started_at = manual_fixture_finished_at.saturating_sub(10);
        human_report_rejects(&negative_fixture(
            name,
            "headed-only-human-artifacts",
            json!({
                "status": "pass",
                "report_version": 1,
                "generated_at_utc": current_unix_seconds().to_string(),
                "command": "human",
                "command_argv": ["cargo", "xtask", format!("verify-{name}-human"), "--check"],
                "layer": "human",
                "exit_status": 0,
                "git_commit": git_commit(),
                "binary_hash": headed_report.get("binary_hash").cloned().unwrap_or_else(|| json!("fixture-binary")),
                "source_path": source,
                "source_hash": file_hash(&format!("examples/{name}.bn")),
                "scenario_path": scenario,
                "scenario_hash": file_hash(&format!("examples/{name}.scn")),
                "program_hash": file_hash(&format!("examples/{name}.bn")),
                "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
                "graph_node_count": headed_report.get("graph_node_count").cloned().unwrap_or_else(|| json!(0)),
                "headed_report_path": headed_report_path,
                "headed_report_sha256": file_hash(&format!("target/reports/{name}-headed-ply.json")),
                "headed_input_injection_method": "os_pointer_keyboard_to_visible_window",
                "headed_os_input_step_count": headed_report.get("os_input_steps").and_then(serde_json::Value::as_array).map(Vec::len).unwrap_or_default(),
                "headed_os_input_missing_labels": [],
                "manual_observer": "Manual Tester",
                "manual_input_route": "human_visible_window",
                "manual_started_at_utc": manual_fixture_started_at.to_string(),
                "manual_finished_at_utc": manual_fixture_finished_at.to_string(),
                "manual_session_duration_seconds": "10",
                "display_server": headed_report.get("display_server").cloned().unwrap_or_else(|| json!("wayland")),
                "window_backend": headed_report.get("window_backend").cloned().unwrap_or_else(|| json!("ply-engine/macroquad")),
                "display_scale": headed_report.get("display_scale").cloned().unwrap_or_else(|| json!(1.0)),
                "window_title": headed_report.get("window_title").cloned().unwrap_or_else(|| json!("Boon Circuit Ply Playground")),
                "manual_notes": "fixture notes",
                "manual_checklist_pass_fail": all_true_checklist.clone(),
                "per_step_pass_fail": [],
                "checkpoint_screenshot_or_video_paths": [headed_artifact_path],
                "artifact_sha256s": [headed_artifact]
            }),
        )?)?
    } else {
        true
    };
    let replace_placeholder_manual_rejected = if report_path(name, VerificationLayer::HeadedPly)
        .exists()
    {
        let headed_report_path = report_path(name, VerificationLayer::HeadedPly);
        let headed_report = read_json(&headed_report_path)?;
        let manual_fixture_finished_at = current_unix_seconds();
        let manual_fixture_started_at = manual_fixture_finished_at.saturating_sub(10);
        human_report_rejects(&negative_fixture(
            name,
            "replace-placeholder-human-report",
            json!({
                "status": "pass",
                "report_version": 1,
                "generated_at_utc": current_unix_seconds().to_string(),
                "command": "human",
                "command_argv": ["cargo", "xtask", format!("verify-{name}-human"), "--check"],
                "layer": "human",
                "exit_status": 0,
                "git_commit": git_commit(),
                "binary_hash": headed_report.get("binary_hash").cloned().unwrap_or_else(|| json!("fixture-binary")),
                "source_path": source,
                "source_hash": file_hash(&format!("examples/{name}.bn")),
                "scenario_path": scenario,
                "scenario_hash": file_hash(&format!("examples/{name}.scn")),
                "program_hash": file_hash(&format!("examples/{name}.bn")),
                "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
                "graph_node_count": headed_report.get("graph_node_count").cloned().unwrap_or_else(|| json!(0)),
                "headed_report_path": headed_report_path,
                "headed_report_sha256": file_hash(&format!("target/reports/{name}-headed-ply.json")),
                "headed_input_injection_method": "os_pointer_keyboard_to_visible_window",
                "headed_os_input_step_count": headed_report.get("os_input_steps").and_then(serde_json::Value::as_array).map(Vec::len).unwrap_or_default(),
                "headed_os_input_missing_labels": [],
                "manual_observer": "replace-with-real-tester-name",
                "manual_input_route": "human_visible_window",
                "manual_artifact_capture_method": "replace-with-real-capture-tool",
                "manual_started_at_utc": manual_fixture_started_at.to_string(),
                "manual_finished_at_utc": manual_fixture_finished_at.to_string(),
                "manual_session_duration_seconds": "10",
                "display_server": headed_report.get("display_server").cloned().unwrap_or_else(|| json!("wayland")),
                "window_backend": headed_report.get("window_backend").cloned().unwrap_or_else(|| json!("ply-engine/macroquad")),
                "display_scale": headed_report.get("display_scale").cloned().unwrap_or_else(|| json!(1.0)),
                "window_title": headed_report.get("window_title").cloned().unwrap_or_else(|| json!("Boon Circuit Ply Playground")),
                "manual_notes": "replace-with-visual-quality-notes-and-deviations",
                "manual_checklist_pass_fail": all_true_checklist.clone(),
                "per_step_pass_fail": [],
                "checkpoint_screenshot_or_video_paths": [],
                "artifact_sha256s": []
            }),
        )?)?
    } else {
        true
    };
    let fake_manual_image_rejected = if report_path(name, VerificationLayer::HeadedPly).exists() {
        let headed_report_path = report_path(name, VerificationLayer::HeadedPly);
        let headed_report = read_json(&headed_report_path)?;
        let artifact_path = format!("target/reports/{name}-human-fake-image-fixture.png");
        std::fs::write(&artifact_path, b"not a png")?;
        let manual_fixture_finished_at = current_unix_seconds();
        let manual_fixture_started_at = manual_fixture_finished_at.saturating_sub(10);
        let rejected = human_report_rejects(&negative_fixture(
            name,
            "fake-image-human-report",
            json!({
                "status": "pass",
                "report_version": 1,
                "generated_at_utc": current_unix_seconds().to_string(),
                "command": "human",
                "command_argv": ["cargo", "xtask", format!("verify-{name}-human"), "--check"],
                "layer": "human",
                "exit_status": 0,
                "git_commit": git_commit(),
                "binary_hash": headed_report.get("binary_hash").cloned().unwrap_or_else(|| json!("fixture-binary")),
                "source_path": source,
                "source_hash": file_hash(&format!("examples/{name}.bn")),
                "scenario_path": scenario,
                "scenario_hash": file_hash(&format!("examples/{name}.scn")),
                "program_hash": file_hash(&format!("examples/{name}.bn")),
                "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
                "graph_node_count": headed_report.get("graph_node_count").cloned().unwrap_or_else(|| json!(0)),
                "headed_report_path": headed_report_path,
                "headed_report_sha256": file_hash(&format!("target/reports/{name}-headed-ply.json")),
                "headed_input_injection_method": "os_pointer_keyboard_to_visible_window",
                "headed_os_input_step_count": headed_report.get("os_input_steps").and_then(serde_json::Value::as_array).map(Vec::len).unwrap_or_default(),
                "headed_os_input_missing_labels": [],
                "manual_observer": "Human Reviewer",
                "manual_input_route": "human_visible_window",
                "manual_artifact_capture_method": "desktop screenshot tool",
                "manual_started_at_utc": manual_fixture_started_at.to_string(),
                "manual_finished_at_utc": manual_fixture_finished_at.to_string(),
                "manual_session_duration_seconds": "10",
                "display_server": headed_report.get("display_server").cloned().unwrap_or_else(|| json!("wayland")),
                "window_backend": headed_report.get("window_backend").cloned().unwrap_or_else(|| json!("ply-engine/macroquad")),
                "display_scale": headed_report.get("display_scale").cloned().unwrap_or_else(|| json!(1.0)),
                "window_title": headed_report.get("window_title").cloned().unwrap_or_else(|| json!("Boon Circuit Ply Playground")),
                "manual_notes": "visual pass notes",
                "manual_checklist_pass_fail": all_true_checklist.clone(),
                "per_step_pass_fail": [],
                "checkpoint_screenshot_or_video_paths": [artifact_path.clone()],
                "artifact_sha256s": [{
                    "path": artifact_path,
                    "sha256": file_hash(&format!("target/reports/{name}-human-fake-image-fixture.png"))
                }]
            }),
        )?)?;
        let _ = std::fs::remove_file(&artifact_path);
        rejected
    } else {
        true
    };
    let fake_manual_video_rejected = if report_path(name, VerificationLayer::HeadedPly).exists() {
        let headed_report_path = report_path(name, VerificationLayer::HeadedPly);
        let headed_report = read_json(&headed_report_path)?;
        let artifact_path = format!("target/reports/{name}-human-fake-video-fixture.mp4");
        std::fs::write(&artifact_path, vec![b'x'; 2048])?;
        let manual_fixture_finished_at = current_unix_seconds();
        let manual_fixture_started_at = manual_fixture_finished_at.saturating_sub(10);
        let rejected = human_report_rejects(&negative_fixture(
            name,
            "fake-video-human-report",
            json!({
                "status": "pass",
                "report_version": 1,
                "generated_at_utc": current_unix_seconds().to_string(),
                "command": "human",
                "command_argv": ["cargo", "xtask", format!("verify-{name}-human"), "--check"],
                "layer": "human",
                "exit_status": 0,
                "git_commit": git_commit(),
                "binary_hash": headed_report.get("binary_hash").cloned().unwrap_or_else(|| json!("fixture-binary")),
                "source_path": source,
                "source_hash": file_hash(&format!("examples/{name}.bn")),
                "scenario_path": scenario,
                "scenario_hash": file_hash(&format!("examples/{name}.scn")),
                "program_hash": file_hash(&format!("examples/{name}.bn")),
                "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
                "graph_node_count": headed_report.get("graph_node_count").cloned().unwrap_or_else(|| json!(0)),
                "headed_report_path": headed_report_path,
                "headed_report_sha256": file_hash(&format!("target/reports/{name}-headed-ply.json")),
                "headed_input_injection_method": "os_pointer_keyboard_to_visible_window",
                "headed_os_input_step_count": headed_report.get("os_input_steps").and_then(serde_json::Value::as_array).map(Vec::len).unwrap_or_default(),
                "headed_os_input_missing_labels": [],
                "manual_observer": "Human Reviewer",
                "manual_input_route": "human_visible_window",
                "manual_artifact_capture_method": "desktop video recorder",
                "manual_started_at_utc": manual_fixture_started_at.to_string(),
                "manual_finished_at_utc": manual_fixture_finished_at.to_string(),
                "manual_session_duration_seconds": "10",
                "display_server": headed_report.get("display_server").cloned().unwrap_or_else(|| json!("wayland")),
                "window_backend": headed_report.get("window_backend").cloned().unwrap_or_else(|| json!("ply-engine/macroquad")),
                "display_scale": headed_report.get("display_scale").cloned().unwrap_or_else(|| json!(1.0)),
                "window_title": headed_report.get("window_title").cloned().unwrap_or_else(|| json!("Boon Circuit Ply Playground")),
                "manual_notes": "visual pass notes",
                "manual_checklist_pass_fail": all_true_checklist.clone(),
                "per_step_pass_fail": [],
                "checkpoint_screenshot_or_video_paths": [artifact_path.clone()],
                "artifact_sha256s": [{
                    "path": artifact_path,
                    "sha256": file_hash(&format!("target/reports/{name}-human-fake-video-fixture.mp4"))
                }]
            }),
        )?)?;
        let _ = std::fs::remove_file(&artifact_path);
        rejected
    } else {
        true
    };
    let future_generated_manual_rejected = if report_path(name, VerificationLayer::HeadedPly)
        .exists()
    {
        let headed_report_path = report_path(name, VerificationLayer::HeadedPly);
        let headed_report = read_json(&headed_report_path)?;
        let artifact_path = format!("target/reports/{name}-human-future-generated-fixture.png");
        std::fs::write(&artifact_path, b"negative future-generated manual fixture")?;
        let now = current_unix_seconds();
        let rejected = human_report_rejects(&negative_fixture(
            name,
            "future-generated-human-report",
            json!({
                "status": "pass",
                "report_version": 1,
                "generated_at_utc": now.saturating_add(3600).to_string(),
                "command": "human",
                "command_argv": ["cargo", "xtask", format!("verify-{name}-human"), "--check"],
                "layer": "human",
                "exit_status": 0,
                "git_commit": git_commit(),
                "binary_hash": headed_report.get("binary_hash").cloned().unwrap_or_else(|| json!("fixture-binary")),
                "source_path": source,
                "source_hash": file_hash(&format!("examples/{name}.bn")),
                "scenario_path": scenario,
                "scenario_hash": file_hash(&format!("examples/{name}.scn")),
                "program_hash": file_hash(&format!("examples/{name}.bn")),
                "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
                "graph_node_count": headed_report.get("graph_node_count").cloned().unwrap_or_else(|| json!(0)),
                "headed_report_path": headed_report_path,
                "headed_report_sha256": file_hash(&format!("target/reports/{name}-headed-ply.json")),
                "headed_input_injection_method": "os_pointer_keyboard_to_visible_window",
                "headed_os_input_step_count": headed_report.get("os_input_steps").and_then(serde_json::Value::as_array).map(Vec::len).unwrap_or_default(),
                "headed_os_input_missing_labels": [],
                "manual_observer": "Manual Tester",
                "manual_input_route": "human_visible_window",
                "manual_started_at_utc": now.saturating_sub(10).to_string(),
                "manual_finished_at_utc": now.to_string(),
                "manual_session_duration_seconds": "10",
                "display_server": headed_report.get("display_server").cloned().unwrap_or_else(|| json!("wayland")),
                "window_backend": headed_report.get("window_backend").cloned().unwrap_or_else(|| json!("ply-engine/macroquad")),
                "display_scale": headed_report.get("display_scale").cloned().unwrap_or_else(|| json!(1.0)),
                "window_title": headed_report.get("window_title").cloned().unwrap_or_else(|| json!("Boon Circuit Ply Playground")),
                "manual_notes": "fixture notes",
                "manual_checklist_pass_fail": all_true_checklist.clone(),
                "per_step_pass_fail": [],
                "checkpoint_screenshot_or_video_paths": [artifact_path.clone()],
                "artifact_sha256s": [{
                    "path": artifact_path,
                    "sha256": file_hash(&format!("target/reports/{name}-human-future-generated-fixture.png"))
                }]
            }),
        )?)?;
        let _ = std::fs::remove_file(&artifact_path);
        rejected
    } else {
        true
    };
    let future_manual_session_rejected = if report_path(name, VerificationLayer::HeadedPly).exists()
    {
        let headed_report_path = report_path(name, VerificationLayer::HeadedPly);
        let headed_report = read_json(&headed_report_path)?;
        let artifact_path = format!("target/reports/{name}-human-future-session-fixture.png");
        std::fs::write(&artifact_path, b"negative future-session manual fixture")?;
        let now = current_unix_seconds();
        let rejected = human_report_rejects(&negative_fixture(
            name,
            "future-session-human-report",
            json!({
                "status": "pass",
                "report_version": 1,
                "generated_at_utc": now.to_string(),
                "command": "human",
                "command_argv": ["cargo", "xtask", format!("verify-{name}-human"), "--check"],
                "layer": "human",
                "exit_status": 0,
                "git_commit": git_commit(),
                "binary_hash": headed_report.get("binary_hash").cloned().unwrap_or_else(|| json!("fixture-binary")),
                "source_path": source,
                "source_hash": file_hash(&format!("examples/{name}.bn")),
                "scenario_path": scenario,
                "scenario_hash": file_hash(&format!("examples/{name}.scn")),
                "program_hash": file_hash(&format!("examples/{name}.bn")),
                "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
                "graph_node_count": headed_report.get("graph_node_count").cloned().unwrap_or_else(|| json!(0)),
                "headed_report_path": headed_report_path,
                "headed_report_sha256": file_hash(&format!("target/reports/{name}-headed-ply.json")),
                "headed_input_injection_method": "os_pointer_keyboard_to_visible_window",
                "headed_os_input_step_count": headed_report.get("os_input_steps").and_then(serde_json::Value::as_array).map(Vec::len).unwrap_or_default(),
                "headed_os_input_missing_labels": [],
                "manual_observer": "Manual Tester",
                "manual_input_route": "human_visible_window",
                "manual_started_at_utc": now.saturating_add(3600).to_string(),
                "manual_finished_at_utc": now.saturating_add(3610).to_string(),
                "manual_session_duration_seconds": "10",
                "display_server": headed_report.get("display_server").cloned().unwrap_or_else(|| json!("wayland")),
                "window_backend": headed_report.get("window_backend").cloned().unwrap_or_else(|| json!("ply-engine/macroquad")),
                "display_scale": headed_report.get("display_scale").cloned().unwrap_or_else(|| json!(1.0)),
                "window_title": headed_report.get("window_title").cloned().unwrap_or_else(|| json!("Boon Circuit Ply Playground")),
                "manual_notes": "fixture notes",
                "manual_checklist_pass_fail": all_true_checklist.clone(),
                "per_step_pass_fail": [],
                "checkpoint_screenshot_or_video_paths": [artifact_path.clone()],
                "artifact_sha256s": [{
                    "path": artifact_path,
                    "sha256": file_hash(&format!("target/reports/{name}-human-future-session-fixture.png"))
                }]
            }),
        )?)?;
        let _ = std::fs::remove_file(&artifact_path);
        rejected
    } else {
        true
    };
    let debug_speed_report_rejected = schema_rejects(&negative_fixture(
        name,
        "debug-speed-report",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "speed",
            "layer": "speed",
            "build_profile": "debug",
            "git_commit": git_commit(),
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "budget_check": {
                "latency_p95_budget": {"pass": true},
                "latency_max_budget": {"pass": true},
                "allocation_budget": {"pass": true},
                "graph_rebuild_budget": {"pass": true}
            },
            "per_step_pass_fail": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let failed_speed_budget_rejected = schema_rejects(&negative_fixture(
        name,
        "failed-speed-budget",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "speed",
            "layer": "speed",
            "build_profile": "release",
            "git_commit": git_commit(),
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "budget_check": {
                "latency_p95_budget": {"pass": false},
                "latency_max_budget": {"pass": true},
                "allocation_budget": {"pass": true},
                "graph_rebuild_budget": {"pass": true}
            },
            "per_step_pass_fail": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let missing_speed_stress_rejected = schema_rejects(&negative_fixture(
        name,
        "missing-speed-stress",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "speed",
            "command_argv": ["cargo", "xtask", format!("verify-{name}-speed")],
            "exit_status": 0,
            "layer": "speed",
            "build_profile": "release",
            "git_commit": git_commit(),
            "binary_hash": current_binary_hash(),
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
            "graph_node_count": 1,
            "budget_check": {
                "latency_p95_budget": {"pass": true},
                "latency_max_budget": {"pass": true},
                "allocation_budget": {"pass": true},
                "graph_rebuild_budget": {"pass": true}
            },
            "per_step_pass_fail": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let missing_speed_resource_fields_rejected = schema_rejects(&negative_fixture(
        name,
        "missing-speed-resource-fields",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "speed",
            "command_argv": ["cargo", "xtask", format!("verify-{name}-speed")],
            "exit_status": 0,
            "layer": "speed",
            "build_profile": "release",
            "git_commit": git_commit(),
            "binary_hash": current_binary_hash(),
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
            "graph_node_count": 1,
            "runtime_execution": {
                "implementation": "static_graph_interpreter",
                "source_loaded_from_boon": true,
                "typed_ir_loaded": true,
                "static_schedule_verified": true,
                "generic_interpreter_complete": true,
                "example_behavior_adapter": false
            },
            "budget_check": {
                "latency_p95_budget": {"pass": true},
                "latency_max_budget": {"pass": true},
                "allocation_budget": {"pass": true},
                "graph_rebuild_budget": {"pass": true}
            },
            "stress_profiles": [{
                "name": "negative",
                "graph_node_count": 1,
                "graph_clones_per_item": 0,
                "dirty_key_count": 1,
                "render_patch_count": 1
            }],
            "per_step_pass_fail": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let missing_runtime_execution_rejected = schema_rejects(&negative_fixture(
        name,
        "missing-runtime-execution",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "semantic",
            "layer": "semantic",
            "git_commit": git_commit(),
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "per_step_pass_fail": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let missing_runtime_contract_rejected = schema_rejects(&negative_fixture(
        name,
        "missing-runtime-report-contract",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "semantic",
            "layer": "semantic",
            "git_commit": git_commit(),
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "runtime_execution": {
                "implementation": "static_graph_interpreter",
                "source_loaded_from_boon": true,
                "typed_ir_loaded": true,
                "static_schedule_verified": true,
                "generic_interpreter_complete": true,
                "example_behavior_adapter": false
            },
            "per_step_pass_fail": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let mut adapter_runtime_execution = generic_runtime_execution_fixture(name);
    adapter_runtime_execution["example_behavior_adapter"] = json!(true);
    let adapter_runtime_rejected = schema_rejects(&negative_fixture(
        name,
        "adapter-runtime-execution",
        runtime_schema_fixture(
            name,
            &source,
            &scenario,
            adapter_runtime_execution,
            json!([valid_delta_batch_fixture(name, 0, 1)]),
        )?,
    )?)?;
    let mut incomplete_generic_slice_execution = generic_runtime_execution_fixture(name);
    incomplete_generic_slice_execution["generic_runtime_slices"]["generic_source_event_ingest"] =
        json!(false);
    let incomplete_generic_slice_rejected = schema_rejects(&negative_fixture(
        name,
        "incomplete-generic-runtime-slice",
        runtime_schema_fixture(
            name,
            &source,
            &scenario,
            incomplete_generic_slice_execution,
            json!([valid_delta_batch_fixture(name, 0, 1)]),
        )?,
    )?)?;
    let mut missing_runtime_id_batch = valid_delta_batch_fixture(name, 0, 1);
    missing_runtime_id_batch["runtime_id"] = serde_json::Value::Null;
    let missing_delta_runtime_id_rejected = schema_rejects(&negative_fixture(
        name,
        "missing-delta-runtime-id",
        runtime_schema_fixture(
            name,
            &source,
            &scenario,
            generic_runtime_execution_fixture(name),
            json!([missing_runtime_id_batch]),
        )?,
    )?)?;
    let mut bad_server_tick_batch = valid_delta_batch_fixture(name, 0, 1);
    bad_server_tick_batch["server_tick"] = json!(99);
    let bad_delta_server_tick_rejected = schema_rejects(&negative_fixture(
        name,
        "bad-delta-server-tick",
        runtime_schema_fixture(
            name,
            &source,
            &scenario,
            generic_runtime_execution_fixture(name),
            json!([bad_server_tick_batch]),
        )?,
    )?)?;
    let mut missing_step_id_batch = valid_delta_batch_fixture(name, 0, 1);
    missing_step_id_batch["step_id"] = serde_json::Value::Null;
    let missing_delta_step_id_rejected = schema_rejects(&negative_fixture(
        name,
        "missing-delta-step-id",
        runtime_schema_fixture(
            name,
            &source,
            &scenario,
            generic_runtime_execution_fixture(name),
            json!([missing_step_id_batch]),
        )?,
    )?)?;
    let missing_playground_surface_rejected = schema_rejects(&negative_fixture(
        name,
        "missing-playground-surface",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "smoke-launch",
            "command_argv": ["cargo", "run", "--release", "-p", "boon_ply_playground", "--", "--smoke-launch", "--example", name],
            "exit_status": 0,
            "layer": "headed-smoke",
            "git_commit": git_commit(),
            "binary_hash": current_binary_hash(),
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
            "graph_node_count": 1,
            "per_step_pass_fail": [{"id": "negative-fixture-shape", "pass": true}],
            "artifact_sha256s": []
        }),
    )?)?;
    let missing_benchmark_evidence_rejected = schema_rejects(&negative_fixture(
        name,
        "missing-benchmark-evidence",
        json!({
            "status": "pass",
            "command": if name == "todomvc" { "bench-todomvc" } else { "bench-example" },
            "per_step_pass_fail": [{"id": "negative-fixture-shape", "pass": true}],
            "artifact_sha256s": []
        }),
    )?)?;
    let bad_delta_epoch_rejected = schema_rejects(&negative_fixture(
        name,
        "bad-delta-epoch",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "semantic",
            "layer": "semantic",
            "git_commit": git_commit(),
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 1,
            "runtime_execution": {
                "implementation": "static_graph_interpreter",
                "source_loaded_from_boon": true,
                "typed_ir_loaded": true,
                "static_schedule_verified": true,
                "generic_interpreter_complete": true,
                "example_behavior_adapter": false
            },
            "runtime_profile": "software_bounded",
            "renderer": "semantic",
            "window_mode": "none",
            "window_backend": {"unavailable_reason": "negative fixture"},
            "display_server": "negative",
            "display_scale": "1",
            "window_size": {"unavailable_reason": "negative fixture"},
            "framebuffer_size": {"unavailable_reason": "negative fixture"},
            "total_ticks": 1,
            "total_source_events": 0,
            "total_semantic_deltas": 1,
            "total_render_deltas": 0,
            "max_dirty_nodes": 0,
            "max_dirty_keys": 1,
            "allocations": {},
            "latency_ms_p50_p95_p99_max": {"p50": 0, "p95": 0, "p99": 0, "max": 0},
            "rss_delta_mib_steady_peak": {"steady": 0, "peak": 0, "baseline": 1, "measurement": "negative fixture"},
            "baseline_rss_mib": 1,
            "steady_rss_mib": 1,
            "vram_delta_mib_steady_peak_or_unavailable_reason": {"unavailable_reason": "negative fixture"},
            "semantic_delta_protocol_batches": [{
                "program_hash": file_hash(&format!("examples/{name}.bn")),
                "runtime_id": "negative",
                "base_epoch": 0,
                "next_epoch": 2,
                "changes": [{"kind": "FieldSet", "list_id": null, "key": null, "generation": null, "source_id": null, "bind_epoch": null, "field_path": "store.new_todo_text", "value": "x"}]
            }],
            "failure_artifacts": [],
            "per_step_pass_fail": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let missing_render_identity_rejected = schema_rejects(&negative_fixture(
        name,
        "missing-render-identity",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "semantic",
            "layer": "semantic",
            "git_commit": git_commit(),
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 1,
            "runtime_execution": {
                "implementation": "static_graph_interpreter",
                "source_loaded_from_boon": true,
                "typed_ir_loaded": true,
                "static_schedule_verified": true,
                "generic_interpreter_complete": true,
                "example_behavior_adapter": false
            },
            "runtime_profile": "software_bounded",
            "renderer": "semantic",
            "window_mode": "none",
            "window_backend": {"unavailable_reason": "negative fixture"},
            "display_server": "negative",
            "display_scale": "1",
            "window_size": {"unavailable_reason": "negative fixture"},
            "framebuffer_size": {"unavailable_reason": "negative fixture"},
            "total_ticks": 1,
            "total_source_events": 0,
            "total_semantic_deltas": 0,
            "total_render_deltas": 1,
            "max_dirty_nodes": 0,
            "max_dirty_keys": 1,
            "allocations": {},
            "latency_ms_p50_p95_p99_max": {"p50": 0, "p95": 0, "p99": 0, "max": 0},
            "rss_delta_mib_steady_peak": {"steady": 0, "peak": 0, "baseline": 1, "measurement": "negative fixture"},
            "baseline_rss_mib": 1,
            "steady_rss_mib": 1,
            "vram_delta_mib_steady_peak_or_unavailable_reason": {"unavailable_reason": "negative fixture"},
            "semantic_delta_protocol_batches": [{
                "program_hash": file_hash(&format!("examples/{name}.bn")),
                "runtime_id": "negative",
                "base_epoch": 0,
                "next_epoch": 1,
                "changes": []
            }],
            "render_patches": [{
                "kind": "InsertElement",
                "target": "todos:1:row",
                "value": "bad",
                "list_id": null,
                "key": null,
                "generation": null,
                "source_id": null,
                "bind_epoch": null
            }],
            "failure_artifacts": [],
            "per_step_pass_fail": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let malformed_per_step_rejected = schema_rejects(&negative_fixture(
        name,
        "malformed-per-step",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "semantic",
            "git_commit": git_commit(),
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "per_step_pass_fail": ["not-a-check-object"],
            "artifact_sha256s": []
        }),
    )?)?;
    let failed_common_check_rejected = schema_rejects(&negative_fixture(
        name,
        "failed-common-check",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "semantic",
            "git_commit": git_commit(),
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "per_step_pass_fail": [{"id": "must-not-fail-in-pass-report", "pass": false}],
            "artifact_sha256s": []
        }),
    )?)?;
    let nonzero_exit_status_rejected = schema_rejects(&negative_fixture(
        name,
        "nonzero-exit-status",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "semantic",
            "exit_status": 1,
            "git_commit": git_commit(),
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "per_step_pass_fail": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let future_report_rejected = schema_rejects(&negative_fixture(
        name,
        "future-report",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().saturating_add(3600).to_string(),
            "command": "semantic",
            "git_commit": git_commit(),
            "source_path": source,
            "source_hash": file_hash(&format!("examples/{name}.bn")),
            "scenario_path": scenario,
            "scenario_hash": file_hash(&format!("examples/{name}.scn")),
            "program_hash": file_hash(&format!("examples/{name}.bn")),
            "graph_node_count": 0,
            "per_step_pass_fail": [],
            "artifact_sha256s": []
        }),
    )?)?;
    let report = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-negative",
        "command_argv": ["cargo", "xtask", format!("verify-{name}-negative")],
        "exit_status": 0,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_hash": file_hash(&format!("examples/{name}.bn")),
        "scenario_hash": file_hash(&format!("examples/{name}.scn")),
        "program_hash": file_hash(&format!("examples/{name}.bn")),
        "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
        "graph_node_count": 0,
        "per_step_pass_fail": [
            {"id": "hidden-runtime-identity-rejected", "pass": true},
            {"id": "app-visible-identity-routing-rejected", "pass": app_visible_identity_rejected},
            {"id": "stale-source-hash-rejected", "pass": stale_hash_rejected},
            {"id": "stale-scenario-hash-rejected", "pass": stale_scenario_hash_rejected},
            {"id": "missing-headed-screenshot-rejected", "pass": missing_screenshot_rejected},
            {"id": "direct-source-injection-headed-rejected-by-contract", "pass": direct_injection_rejected},
            {"id": "headed-without-os-input-limitation-rejected", "pass": missing_os_limitation_rejected},
            {"id": "fake-full-os-input-report-rejected", "pass": fake_full_os_input_rejected},
            {"id": "fake-full-os-input-without-visible-coverage-rejected", "pass": fake_full_os_without_visible_coverage_rejected},
            {"id": "missing-headed-metadata-rejected", "pass": missing_headed_metadata_rejected},
            {"id": "stale-human-report-rejected", "pass": stale_manual_rejected},
            {"id": "handwritten-human-report-rejected", "pass": handwritten_manual_rejected},
            {"id": "scripted-human-placeholder-rejected", "pass": scripted_manual_rejected},
            {"id": "template-human-placeholder-rejected", "pass": template_placeholder_rejected},
            {"id": "prepare-human-report-pass-labels-enforced", "pass": prepare_pass_labels_enforced},
            {"id": "missing-headed-report-binding-rejected", "pass": missing_headed_binding_rejected},
            {"id": "headed-only-manual-artifacts-rejected", "pass": headed_only_manual_artifacts_rejected},
            {"id": "replace-placeholder-manual-report-rejected", "pass": replace_placeholder_manual_rejected},
            {"id": "fake-manual-image-artifact-rejected", "pass": fake_manual_image_rejected},
            {"id": "fake-manual-video-artifact-rejected", "pass": fake_manual_video_rejected},
            {"id": "future-generated-human-report-rejected", "pass": future_generated_manual_rejected},
            {"id": "future-manual-session-rejected", "pass": future_manual_session_rejected},
            {"id": "debug-speed-report-rejected", "pass": debug_speed_report_rejected},
            {"id": "failed-speed-budget-rejected", "pass": failed_speed_budget_rejected},
            {"id": "missing-speed-stress-profiles-rejected", "pass": missing_speed_stress_rejected},
            {"id": "missing-speed-resource-fields-rejected", "pass": missing_speed_resource_fields_rejected},
            {"id": "missing-runtime-execution-metadata-rejected", "pass": missing_runtime_execution_rejected},
            {"id": "missing-runtime-report-contract-rejected", "pass": missing_runtime_contract_rejected},
            {"id": "adapter-runtime-execution-rejected", "pass": adapter_runtime_rejected},
            {"id": "incomplete-generic-runtime-slice-rejected", "pass": incomplete_generic_slice_rejected},
            {"id": "missing-delta-runtime-id-rejected", "pass": missing_delta_runtime_id_rejected},
            {"id": "bad-delta-epoch-rejected", "pass": bad_delta_epoch_rejected},
            {"id": "bad-delta-server-tick-rejected", "pass": bad_delta_server_tick_rejected},
            {"id": "missing-delta-step-id-rejected", "pass": missing_delta_step_id_rejected},
            {"id": "missing-playground-surface-rejected", "pass": missing_playground_surface_rejected},
            {"id": "missing-benchmark-evidence-rejected", "pass": missing_benchmark_evidence_rejected},
            {"id": "missing-render-patch-identity-rejected", "pass": missing_render_identity_rejected},
            {"id": "malformed-per-step-report-rejected", "pass": malformed_per_step_rejected},
            {"id": "failed-common-check-report-rejected", "pass": failed_common_check_rejected},
            {"id": "nonzero-exit-status-report-rejected", "pass": nonzero_exit_status_rejected},
            {"id": "future-dated-report-rejected", "pass": future_report_rejected}
        ],
        "artifact_sha256s": []
    });
    let path = report_path(name, VerificationLayer::Negative);
    write_json(&path, &report)?;
    verify_report_schema(&path)?;
    Ok(())
}

fn generic_runtime_execution_fixture(name: &str) -> serde_json::Value {
    let mut slices = serde_json::Map::new();
    for key in [
        "generic_executable_surface_inferred_from_ir",
        "ir_update_branch_table_loaded",
        "generic_scenario_loop_executor",
        "generic_schedule_instantiated_before_adapter",
        "loaded_runtime_owns_generic_schedule_storage",
        "generic_source_event_ingest",
        "generic_source_binding_store",
        "generic_indexed_branch_evaluator",
        "generic_semantic_delta_emitter",
        "generic_source_mutation_semantic_delta_emitter",
        "generic_derived_value_semantic_delta_emitter",
        "generic_source_bind_semantic_delta_emitter",
        "generic_list_remove_semantic_delta_emitter",
        "generic_source_unbind_semantic_delta_emitter",
        "generic_render_lowering_plan",
        "generic_loaded_runtime_shell",
        "generic_source_route_action_executor",
        "generic_root_text_tick_executor",
        "generic_indexed_hold_commit_executor",
        "generic_list_append_source_binding_executor",
        "generic_list_remove_source_unbinding_executor",
        "generic_list_move_semantic_delta_emitter",
        "generic_list_count_retain_executor",
        "generic_loaded_runtime_state_summary_projection",
        "generic_root_source_dispatch",
        "generic_source_event_route_executor",
        "generic_compiled_source_route_index",
        "generic_source_route_classifier",
        "generic_bound_source_target_resolution",
        "generic_stale_source_key_generation_bind_epoch_rejection",
        "generic_source_action_batch_executor",
        "generic_source_route_scalar_expression_index",
        "generic_indexed_text_route_index",
        "generic_indexed_bool_route_index",
        "generic_root_source_route_index",
        "generic_list_remove_predicate_route",
        "generic_routed_root_target_application",
        "generic_routed_indexed_target_application",
        "ir_list_operation_table_loaded",
        "ir_formula_operation_table_loaded",
        "ir_state_initializers_loaded",
        "ir_list_initializers_loaded",
        "ir_derived_value_table_loaded",
        "generic_list_structural_commit_executor",
    ] {
        slices.insert(key.to_owned(), json!(true));
    }
    slices.insert(
        "surface_driver_borrows_generic_storage_for_tick".to_owned(),
        json!(false),
    );
    let example_specific: &[&str] = match name {
        "todomvc" => &[
            "generic_todomvc_common_render_patch_lowering",
            "generic_todomvc_append_source_bind_render_lowering",
            "generic_todomvc_edit_open_close_render_lowering",
            "generic_todomvc_render_only_patch_lowering",
            "generic_todomvc_source_effects_through_action_executor",
            "generic_route_selected_todo_edit_text_commit_executor",
            "generic_route_selected_todo_title_commit_executor",
            "generic_route_selected_todo_editing_commit_executor",
            "generic_todomvc_summary_reads_authoritative_storage",
            "generic_todomvc_root_holds_no_mirror",
            "generic_todomvc_rows_hold_no_mirror",
            "generic_todomvc_delta_identities_from_authoritative_storage",
            "generic_todomvc_source_route_classifier",
            "generic_todomvc_routed_source_event",
            "generic_todomvc_row_routed_source_event",
            "generic_todomvc_visible_row_occurrence_resolution",
            "generic_todomvc_source_action_mutation_batch",
            "generic_todomvc_append_mutation_batch",
            "generic_todomvc_list_index_action_input_resolution",
            "generic_todomvc_scenario_expectation_assertions",
            "generic_todomvc_scenario_preparation",
            "generic_loaded_runtime_todomvc_root_step_executor",
            "generic_loaded_runtime_todomvc_row_toggle_delete_executor",
            "generic_loaded_runtime_todomvc_row_edit_source_executor",
            "generic_loaded_runtime_todomvc_render_only_hover_executor",
            "generic_loaded_runtime_todomvc_assertion_executor",
            "generic_loaded_runtime_todomvc_stress_profile_executor",
            "generic_routed_todo_bool_target_application",
            "generic_routed_todo_edit_text_target_application",
            "todomvc_root_scalar_holds_from_ir",
            "todomvc_generic_hold_storage_authoritative",
            "todomvc_title_hold_from_ir",
            "todomvc_completed_bool_hold_from_ir",
            "todomvc_editing_bool_hold_from_ir",
            "todomvc_edit_text_hold_from_ir",
            "todomvc_append_remove_from_ir",
            "todomvc_count_and_filter_views_from_ir",
        ],
        "cells" => &[
            "generic_cells_common_render_patch_lowering",
            "generic_cells_source_effects_through_action_executor",
            "generic_cells_source_route_classifier",
            "generic_cells_address_row_context_resolution",
            "generic_cells_routed_source_event",
            "generic_cells_scenario_expectation_assertions",
            "generic_cells_scenario_storage_preparation",
            "generic_cells_formula_dependency_cache",
            "generic_cells_formula_evaluation_cache",
            "generic_cells_formula_derived_storage_sync",
            "generic_cells_formula_display_mutation_emitter",
            "generic_cells_formula_display_protocol_lowering",
            "generic_cells_source_action_mutation_batch",
            "generic_cells_editor_route_uses_indexed_targets",
            "generic_cells_committed_fields_hold_no_mirror",
            "cells_edit_state_holds_from_ir",
            "cells_generic_hold_storage_authoritative",
            "cells_summary_reads_authoritative_storage",
            "cells_hidden_grid_keys_from_generic_storage",
            "cells_formula_pipeline_from_ir",
        ],
        _ => &[],
    };
    for key in example_specific {
        slices.insert((*key).to_owned(), json!(true));
    }
    json!({
        "implementation": "static_graph_interpreter",
        "source_loaded_from_boon": true,
        "typed_ir_loaded": true,
        "static_schedule_verified": true,
        "generic_interpreter_complete": true,
        "example_behavior_adapter": false,
        "adapter_kind": name,
        "generic_runtime_slices": slices
    })
}

fn valid_delta_batch_fixture(name: &str, base_epoch: u64, next_epoch: u64) -> serde_json::Value {
    json!({
        "program_hash": file_hash(&format!("examples/{name}.bn")),
        "runtime_id": format!("negative:{name}"),
        "base_epoch": base_epoch,
        "next_epoch": next_epoch,
        "server_tick": next_epoch,
        "step_id": "negative-fixture",
        "changes": []
    })
}

fn runtime_schema_fixture(
    name: &str,
    source: &Path,
    scenario: &Path,
    runtime_execution: serde_json::Value,
    semantic_delta_protocol_batches: serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "semantic",
        "command_argv": ["cargo", "xtask", format!("verify-{name}-semantic")],
        "exit_status": 0,
        "layer": "semantic",
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_path": source,
        "source_hash": file_hash(&format!("examples/{name}.bn")),
        "scenario_path": scenario,
        "scenario_hash": file_hash(&format!("examples/{name}.scn")),
        "program_hash": file_hash(&format!("examples/{name}.bn")),
        "budget_hash": file_hash(&format!("examples/{name}.budget.toml")),
        "graph_node_count": 1,
        "runtime_execution": runtime_execution,
        "runtime_profile": "software_bounded",
        "renderer": "semantic",
        "window_mode": "none",
        "window_backend": {"unavailable_reason": "negative fixture"},
        "display_server": "negative",
        "display_scale": "1",
        "window_size": {"unavailable_reason": "negative fixture"},
        "framebuffer_size": {"unavailable_reason": "negative fixture"},
        "total_ticks": 1,
        "total_source_events": 0,
        "total_semantic_deltas": 0,
        "total_render_deltas": 0,
        "max_dirty_nodes": 0,
        "max_dirty_keys": 0,
        "allocations": {},
        "latency_ms_p50_p95_p99_max": {"p50": 0, "p95": 0, "p99": 0, "max": 0},
        "rss_delta_mib_steady_peak": {"steady": 0, "peak": 0, "baseline": 1, "measurement": "negative fixture"},
        "baseline_rss_mib": 1,
        "steady_rss_mib": 1,
        "vram_delta_mib_steady_peak_or_unavailable_reason": {"unavailable_reason": "negative fixture"},
        "semantic_delta_protocol_batches": semantic_delta_protocol_batches,
        "render_patches": [],
        "failure_artifacts": [],
        "per_step_pass_fail": [{"id": "negative-fixture-shape", "pass": true}],
        "artifact_sha256s": []
    }))
}

fn negative_fixture(
    name: &str,
    case: &str,
    mut report: serde_json::Value,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    enrich_negative_fixture(name, case, &mut report);
    let path = PathBuf::from(format!("target/reports/_negative-{name}-{case}.json"));
    write_json(&path, &report)?;
    Ok(path)
}

fn enrich_negative_fixture(name: &str, case: &str, report: &mut serde_json::Value) {
    let Some(object) = report.as_object_mut() else {
        return;
    };
    object.entry("report_version").or_insert_with(|| json!(1));
    object
        .entry("generated_at_utc")
        .or_insert_with(|| json!(current_unix_seconds().to_string()));
    object.entry("command").or_insert_with(|| json!("negative"));
    object.entry("command_argv").or_insert_with(|| {
        json!([
            "cargo",
            "xtask",
            format!("verify-{name}-negative"),
            "--fixture",
            case
        ])
    });
    object.entry("exit_status").or_insert_with(|| json!(0));
    object
        .entry("git_commit")
        .or_insert_with(|| json!(git_commit()));
    object
        .entry("binary_hash")
        .or_insert_with(|| json!(current_binary_hash()));
    object
        .entry("source_path")
        .or_insert_with(|| json!(format!("examples/{name}.bn")));
    object
        .entry("source_hash")
        .or_insert_with(|| json!(file_hash(&format!("examples/{name}.bn"))));
    object
        .entry("scenario_path")
        .or_insert_with(|| json!(format!("examples/{name}.scn")));
    object
        .entry("scenario_hash")
        .or_insert_with(|| json!(file_hash(&format!("examples/{name}.scn"))));
    object
        .entry("program_hash")
        .or_insert_with(|| json!(file_hash(&format!("examples/{name}.bn"))));
    object
        .entry("budget_hash")
        .or_insert_with(|| json!(file_hash(&format!("examples/{name}.budget.toml"))));
    object.entry("graph_node_count").or_insert_with(|| json!(0));
    object
        .entry("per_step_pass_fail")
        .or_insert_with(|| json!([]));
    object
        .entry("artifact_sha256s")
        .or_insert_with(|| json!([]));

    let is_headed = object.get("layer").and_then(serde_json::Value::as_str) == Some("headed-ply");
    if is_headed && case != "missing-headed-metadata" {
        object.entry("window_pid").or_insert_with(|| json!(0));
        object
            .entry("window_title")
            .or_insert_with(|| json!("negative headed fixture"));
        object
            .entry("display_server")
            .or_insert_with(|| json!("negative-display"));
        object
            .entry("display_socket_or_compositor_connection")
            .or_insert_with(|| json!("negative-display-socket"));
        object.entry("display_scale").or_insert_with(|| json!(1.0));
        object
            .entry("window_size")
            .or_insert_with(|| json!([100, 100]));
        object
            .entry("input_backend")
            .or_insert_with(|| json!("negative-input-backend"));
        object
            .entry("capture_backend")
            .or_insert_with(|| json!("negative-capture-backend"));
        object
            .entry("focused_window_proof")
            .or_insert_with(|| json!("negative focused window proof"));
        object
            .entry("checkpoint_screenshot_or_video_paths")
            .or_insert_with(|| json!([]));
        object
            .entry("input_route_contract")
            .or_insert_with(|| json!("negative input route contract"));
    }
}

fn schema_rejects(path: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let rejected = verify_report_schema(path).is_err();
    let _ = std::fs::remove_file(path);
    Ok(rejected)
}

fn human_report_rejects(path: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let rejected = verify_human_report(path, 1).is_err();
    let _ = std::fs::remove_file(path);
    Ok(rejected)
}

fn prepare_human_report_rejects_bad_pass_labels(
    name: &str,
    labels: &[String],
) -> Result<bool, Box<dyn std::error::Error>> {
    if labels.is_empty() {
        return Ok(false);
    }
    let template_path = PathBuf::from(format!(
        "target/reports/_negative-{name}-pass-labels-template.json"
    ));
    let missing_report_path = PathBuf::from(format!(
        "target/reports/_negative-{name}-missing-pass-label-report.json"
    ));
    let unknown_report_path = PathBuf::from(format!(
        "target/reports/_negative-{name}-unknown-pass-label-report.json"
    ));
    let checklist = labels
        .iter()
        .map(|label| (label.clone(), json!(false)))
        .collect::<serde_json::Map<_, _>>();
    write_json(
        &template_path,
        &json!({
            "status": "needs_manual",
            "manual_checklist_pass_fail": checklist
        }),
    )?;

    let base_args = vec![
        format!("prepare-{name}-human-report"),
        "--template".to_owned(),
        template_path.display().to_string(),
        "--observer".to_owned(),
        "Manual Tester".to_owned(),
        "--started".to_owned(),
        current_unix_seconds().saturating_sub(10).to_string(),
        "--finished".to_owned(),
        current_unix_seconds().to_string(),
        "--window-pid".to_owned(),
        std::process::id().to_string(),
        "--focused-window-proof".to_owned(),
        "negative fixture supplied focus proof so pass-label validation is reached".to_owned(),
        "--notes".to_owned(),
        "manual label negative fixture".to_owned(),
        "--capture-method".to_owned(),
        "manual capture".to_owned(),
        "--artifact".to_owned(),
        format!("target/reports/{name}-human-label-negative-fixture.png"),
    ];

    let mut missing_args = base_args.clone();
    missing_args.extend([
        "--report".to_owned(),
        missing_report_path.display().to_string(),
    ]);
    for label in labels.iter().skip(1) {
        missing_args.extend(["--pass-label".to_owned(), label.clone()]);
    }
    let missing_error = prepare_human_report(name, &missing_args)
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    let missing_rejected = missing_error.contains("missing=") && !missing_report_path.exists();

    let mut unknown_args = base_args;
    unknown_args.extend([
        "--report".to_owned(),
        unknown_report_path.display().to_string(),
    ]);
    for label in labels {
        unknown_args.extend(["--pass-label".to_owned(), label.clone()]);
    }
    unknown_args.extend([
        "--pass-label".to_owned(),
        "__unknown_manual_label__".to_owned(),
    ]);
    let unknown_error = prepare_human_report(name, &unknown_args)
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    let unknown_rejected = unknown_error.contains("unknown=") && !unknown_report_path.exists();

    let _ = std::fs::remove_file(&template_path);
    let _ = std::fs::remove_file(&missing_report_path);
    let _ = std::fs::remove_file(&unknown_report_path);
    Ok(missing_rejected && unknown_rejected)
}

fn verify_human_report(
    path: &Path,
    max_age_seconds: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    verify_report_schema(path)?;
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let generated = report
        .get("generated_at_utc")
        .and_then(serde_json::Value::as_str)
        .ok_or("manual report missing generated_at_utc")?
        .parse::<u64>()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    if generated > now {
        return Err(format!("manual report `{}` is future-dated", path.display()).into());
    }
    if now.saturating_sub(generated) > max_age_seconds {
        return Err(format!("manual report `{}` is stale", path.display()).into());
    }
    Ok(())
}

fn max_age_seconds(args: &[String]) -> Result<Option<u64>, Box<dyn std::error::Error>> {
    let Some(raw) = args
        .windows(2)
        .find(|window| window[0] == "--max-age")
        .map(|window| window[1].as_str())
    else {
        return Ok(None);
    };
    let (number, multiplier) = if let Some(hours) = raw.strip_suffix('h') {
        (hours, 60 * 60)
    } else if let Some(minutes) = raw.strip_suffix('m') {
        (minutes, 60)
    } else if let Some(seconds) = raw.strip_suffix('s') {
        (seconds, 1)
    } else {
        (raw, 1)
    };
    Ok(Some(number.parse::<u64>()? * multiplier))
}

fn current_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn verify_reports_schema() -> Result<(), Box<dyn std::error::Error>> {
    let dir = Path::new("target/reports");
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }
    let mut checked = 0usize;
    let mut seen = 0usize;
    let mut debug_failures = 0usize;
    let mut manual_templates = 0usize;
    let mut debug_dumps = 0usize;
    let mut debug_auxiliary = 0usize;
    let summary_path = dir.join("schema.json");
    let readiness_path = dir.join("debug/goal-readiness.json");
    let mut artifact_hashes = Vec::new();
    for path in collect_report_json_paths(dir)? {
        if path == summary_path {
            continue;
        }
        seen += 1;
        let report = read_json(&path)?;
        let status = report
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if path != readiness_path {
            artifact_hashes.push(artifact_hash(&path)?);
        }
        let full_pass_report = status == "pass"
            && report.get("report_version").is_some()
            && report.get("command").is_some();
        if full_pass_report {
            match verify_report_schema(&path) {
                Ok(()) => checked += 1,
                Err(error)
                    if is_debug_auxiliary_report(&path, &report, &error.to_string(), dir) =>
                {
                    debug_auxiliary += 1;
                }
                Err(error) => return Err(error),
            }
        } else if status == "fail" && path.starts_with(dir.join("debug")) {
            debug_failures += 1;
        } else if status == "needs_manual" && path.starts_with(dir.join("manual-templates")) {
            manual_templates += 1;
        } else if is_debug_dump_report(&path, &report, dir) {
            debug_dumps += 1;
        } else {
            return Err(format!(
                "unrecognized report JSON shape `{}` with status `{status}`",
                path.display()
            )
            .into());
        }
    }
    let summary = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-report-schema",
        "command_argv": ["cargo", "xtask", "verify-report-schema"],
        "exit_status": 0,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "budget_hash": "n/a",
        "graph_node_count": 0,
        "per_step_pass_fail": [
            {"id": "report-json-files-seen-recursively", "pass": true, "count": seen},
            {"id": "full-pass-reports-schema-checked", "pass": true, "count": checked},
            {"id": "debug-failure-artifacts-accounted", "pass": true, "count": debug_failures},
            {"id": "manual-template-artifacts-accounted", "pass": true, "count": manual_templates},
            {"id": "debug-dump-artifacts-accounted", "pass": true, "count": debug_dumps},
            {"id": "debug-auxiliary-artifacts-accounted", "pass": true, "count": debug_auxiliary}
        ],
        "artifact_sha256s": artifact_hashes
    });
    write_json(&summary_path, &summary)?;
    verify_report_schema(&summary_path)?;
    Ok(())
}

fn is_debug_auxiliary_report(
    path: &Path,
    report: &serde_json::Value,
    schema_error: &str,
    reports_dir: &Path,
) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if !path.starts_with(reports_dir.join("debug"))
        || !(name.ends_with("-pointer-calibration.json")
            || name.ends_with("-headed-edit-probe.json"))
    {
        return false;
    }
    let headed_probe = report.get("status").and_then(serde_json::Value::as_str) == Some("pass")
        && report.get("command").and_then(serde_json::Value::as_str) == Some("headed-ply")
        && report.get("layer").and_then(serde_json::Value::as_str) == Some("headed-ply")
        && report
            .get("window_pid")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|pid| pid > 0)
        && report
            .get("checkpoint_screenshot_or_video_paths")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|paths| !paths.is_empty())
        && report
            .get("artifact_sha256s")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|artifacts| !artifacts.is_empty())
        && report
            .get("input_injection_method")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|method| method.contains("probe"));
    headed_probe && schema_error.contains("semantic_delta_protocol_batches")
}

fn is_debug_dump_report(path: &Path, report: &serde_json::Value, reports_dir: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    path.starts_with(reports_dir.join("debug"))
        && name.ends_with("-ir.json")
        && report.get("status").and_then(serde_json::Value::as_str) == Some("pass")
        && report
            .get("static_schedule_verified")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && report
            .get("hidden_identity_verified")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && report.get("debug_tables").is_some()
        && report
            .get("nodes")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|nodes| !nodes.is_empty())
}

fn collect_report_json_paths(dir: &Path) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut paths = Vec::new();
    collect_report_json_paths_into(dir, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_report_json_paths_into(
    dir: &Path,
    paths: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_report_json_paths_into(&path, paths)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            paths.push(path);
        }
    }
    Ok(())
}

fn explain_hardware(name: &str, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let (source, _, _) = example_paths(name)?;
    let profile = args
        .windows(2)
        .find(|window| window[0] == "--profile")
        .map(|window| window[1].as_str())
        .unwrap_or("fpga_todomvc");
    let report = report_arg(args)
        .unwrap_or_else(|| PathBuf::from(format!("target/reports/{name}-hardware.json")));
    let status = Command::new("cargo")
        .args([
            "run",
            "-p",
            "boon_cli",
            "--",
            "explain-hardware",
            source.to_str().ok_or("source path is not utf-8")?,
            "--profile",
            profile,
            "--report",
            report.to_str().ok_or("report path is not utf-8")?,
        ])
        .status()?;
    if !status.success() {
        return Err("hardware explanation command failed".into());
    }
    verify_report_schema(&report)?;
    Ok(())
}

fn named_arg(args: &[String], index: usize) -> Result<&str, Box<dyn std::error::Error>> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| "missing example name".into())
}

fn report_arg(args: &[String]) -> Option<PathBuf> {
    args.windows(2)
        .find(|window| window[0] == "--report")
        .map(|window| PathBuf::from(&window[1]))
}

fn value_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].clone())
}

fn value_args(args: &[String], flag: &str) -> Vec<String> {
    args.windows(2)
        .filter(|window| window[0] == flag)
        .map(|window| window[1].clone())
        .collect()
}

fn required_value_arg(args: &[String], flag: &str) -> Result<String, Box<dyn std::error::Error>> {
    value_arg(args, flag).ok_or_else(|| format!("missing required `{flag}` argument").into())
}

fn report_path(name: &str, layer: VerificationLayer) -> PathBuf {
    PathBuf::from(format!("target/reports/{name}-{}.json", layer.as_str()))
}

fn manual_template_path(name: &str) -> PathBuf {
    PathBuf::from(format!("target/reports/manual-templates/{name}-human.json"))
}

fn git_commit() -> String {
    Command::new("git")
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

fn artifact_hash(path: &Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(json!({
        "path": path.display().to_string(),
        "sha256": boon_runtime::sha256_file(path)?
    }))
}

fn file_hash(path: &str) -> String {
    boon_runtime::sha256_file(Path::new(path)).unwrap_or_else(|_| "missing".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertised_xtask_commands_are_unique_and_supported() {
        let mut seen = BTreeSet::new();
        for command in XTASK_COMMANDS {
            assert!(seen.insert(*command), "duplicate xtask command `{command}`");
            assert!(
                xtask_command_supported(command),
                "advertised xtask command `{command}` is not supported"
            );
        }
    }

    #[test]
    fn documented_xtask_commands_are_advertised() {
        for command in documented_xtask_commands() {
            assert!(
                XTASK_COMMANDS.contains(command),
                "documented xtask command `{command}` is missing from help"
            );
        }
    }
}
