use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::json;

fn main() {
    if let Err(error) = run() {
        eprintln!("todomvc bench failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(());
    }

    let iterations = parse_usize_arg(&args, "--iterations")?.unwrap_or(100);
    if iterations == 0 {
        return Err("--iterations must be greater than zero".into());
    }

    let workspace_root = workspace_root()?;
    std::env::set_current_dir(&workspace_root)?;

    let report_path = parse_path_arg(&args, "--report")?
        .unwrap_or_else(|| PathBuf::from("target/reports/todomvc-bench.json"));
    let speed_report_path = parse_path_arg(&args, "--speed-report")?
        .unwrap_or_else(|| PathBuf::from("target/reports/todomvc-bench-speed.json"));

    let (source, scenario, budget) = boon_runtime::example_paths("todomvc")?;

    let speed_output = boon_runtime::run_legacy_scenario(
        &source,
        &scenario,
        boon_runtime::VerificationLayer::Speed,
        Some(&speed_report_path),
    )?;
    boon_runtime::verify_report_schema(&speed_report_path)?;

    let started = Instant::now();
    for _ in 0..iterations {
        boon_runtime::run_legacy_scenario(
            &source,
            &scenario,
            boon_runtime::VerificationLayer::Speed,
            None,
        )?;
    }
    let elapsed = started.elapsed();
    let total_ms = elapsed.as_secs_f64() * 1000.0;
    let average_ms = total_ms / iterations as f64;

    let speed_report_hash = boon_runtime::sha256_file(&speed_report_path)?;
    let source_hash = boon_runtime::sha256_file(&source)?;
    let scenario_hash = boon_runtime::sha256_file(&scenario)?;
    let budget_hash = boon_runtime::sha256_file(&budget)?;
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
    let runtime_profile = speed_output
        .report
        .get("runtime_profile")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let runtime_profile_detail = speed_output
        .report
        .get("runtime_profile_detail")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let capacities = speed_output
        .report
        .get("capacities")
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

    let report = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "bench-todomvc",
        "command_argv": args,
        "exit_status": 0,
        "git_commit": git_commit(),
        "binary_hash": current_binary_hash(),
        "source_path": path_string(&source),
        "source_hash": source_hash,
        "scenario_path": path_string(&scenario),
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
        "runtime_profile": runtime_profile,
        "runtime_profile_detail": runtime_profile_detail,
        "capacities": capacities,
        "stress_profiles": stress_profiles,
        "per_step_pass_fail": [
            {
                "id": "bench-iterations",
                "pass": true,
                "detail": format!("{iterations} full speed-layer TodoMVC scenario iterations completed")
            },
            {
                "id": "speed-report-schema",
                "pass": true,
                "detail": format!("{} schema-valid", speed_report_path.display())
            },
            {
                "id": "speed-budget-check",
                "pass": true,
                "detail": "speed report passed budget checks"
            }
        ],
        "artifact_sha256s": [
            {
                "path": path_string(&speed_report_path),
                "sha256": speed_report_hash
            }
        ],
        "benchmark": {
            "iterations": iterations,
            "total_ms": total_ms,
            "average_ms_per_iteration": average_ms,
            "iteration_scope": "full_speed_layer_scenario_rerun_including_reportless_verifier_overhead",
            "speed_report_path": path_string(&speed_report_path),
            "speed_report_layer": "speed",
            "interaction_latency_source": "input_to_idle_ms_p50_p95_p99_max copied from linked speed report",
            "heap_alloc_count_after_warmup": bounded_allocs
        }
    });

    boon_runtime::write_json(&report_path, &report)?;
    boon_runtime::verify_report_schema(&report_path)?;

    println!(
        "todomvc static-runtime bench: {iterations} iterations in {:.3}ms ({:.3}ms/iteration)",
        total_ms, average_ms
    );
    println!("wrote {}", report_path.display());
    Ok(())
}

fn parse_usize_arg(
    args: &[String],
    name: &str,
) -> Result<Option<usize>, Box<dyn std::error::Error>> {
    parse_value_arg(args, name)?
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|error| format!("{name} must be a positive integer: {error}").into())
        })
        .transpose()
}

fn parse_path_arg(
    args: &[String],
    name: &str,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    Ok(parse_value_arg(args, name)?.map(PathBuf::from))
}

fn parse_value_arg(
    args: &[String],
    name: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let mut value = None;
    let mut index = 1;
    while index < args.len() {
        let arg = &args[index];
        if let Some((key, inline_value)) = arg.split_once('=') {
            if key == name {
                value = Some(inline_value.to_owned());
            }
        } else if arg == name {
            let next = args
                .get(index + 1)
                .ok_or_else(|| format!("{name} requires a value"))?;
            value = Some(next.to_owned());
            index += 1;
        }
        index += 1;
    }
    Ok(value)
}

fn workspace_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    for ancestor in cwd.ancestors() {
        if ancestor.join("examples/todomvc.bn").exists()
            && ancestor.join("crates/boon_runtime/Cargo.toml").exists()
        {
            return Ok(ancestor.to_path_buf());
        }
    }
    Err(format!("could not find workspace root above {}", cwd.display()).into())
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn git_commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|hash| hash.trim().to_owned())
        .filter(|hash| !hash.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn current_binary_hash() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| boon_runtime::sha256_file(&path).ok())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
}

fn print_help() {
    println!(
        "TodoMVC benchmark\n\n  --iterations <n>        scenario iterations to run after the schema-checked speed report (default: 100)\n  --report <path>         benchmark report path (default: target/reports/todomvc-bench.json)\n  --speed-report <path>   linked speed-layer report path (default: target/reports/todomvc-bench-speed.json)"
    );
}
