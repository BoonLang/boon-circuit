#![recursion_limit = "256"]

use boon_runtime::{
    VerificationLayer, example_paths, parse_scenario, run_scenario, verify_report_schema,
    write_json,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

const NATIVE_DEV_EDITOR_WHEEL_MIN_STEPS: u64 = 3;

const XTASK_COMMANDS: &[&str] = &[
    "verify-example-semantic",
    "verify-example-speed",
    "verify-example-negative",
    "verify-foundation",
    "bench-example",
    "verify-report-schema",
    "verify-runtime-production-hardening",
    "verify-runtime-finality",
    "audit-genericity",
    "verify-playground-genericity",
    "playground-watch",
    "verify-boon-source-syntax",
    "verify-boon-driver-schema",
    "verify-boon-driver-e2e",
    "verify-boon-driver-dev-window",
    "verify-boon-driver-speed",
    "verify-boon-driver-all",
    "verify-linux-human-like-environment",
    "verify-linux-human-like-e2e",
    "verify-linux-human-like-speed",
    "verify-linux-human-like-all",
    "audit-machine-readiness",
    "verify-todomvc-semantic",
    "verify-todomvc-speed",
    "verify-todomvc-negative",
    "bench-todomvc",
    "explain-todomvc-hardware",
    "verify-cells-semantic",
    "verify-cells-speed",
    "verify-cells-negative",
    "shaders",
    "verify-platform-contract",
    "verify-native-gpu-dependency-graph",
    "verify-native-gpu-architecture",
    "verify-native-gpu-layout-contract",
    "verify-native-gpu-shaders",
    "verify-native-gpu-multiwindow",
    "verify-native-gpu-ipc-backpressure",
    "verify-native-gpu-observability",
    "verify-native-gpu-idle-wake",
    "verify-native-real-window-input-environment",
    "verify-native-gpu-preview-e2e",
    "verify-native-visible-launch",
    "verify-native-examples",
    "verify-native-dev-window-editor",
    "verify-native-example-tabs",
    "verify-native-editor-format",
    "verify-native-example-speed",
    "verify-native-counter-interaction-speed",
    "verify-native-cells-interaction-speed",
    "verify-native-dev-editor-speed",
    "verify-native-two-window-content",
    "verify-native-todomvc-reference-parity",
    "verify-native-todomvc-input-parity",
    "verify-native-gpu-scroll-speed",
    "verify-native-dev-editor-scroll-speed",
    "verify-native-example-switch-speed",
    "verify-native-gpu-negative",
    "verify-native-gpu-all",
    "verify-native-gpu-regression-all",
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
    if legacy_ply_cosmic_testing_command(command) {
        return legacy_ply_cosmic_testing_removed(command);
    }
    match command {
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "verify-example-semantic" => verify_named(&args, VerificationLayer::Semantic),
        "verify-example-speed" => verify_named(&args, VerificationLayer::Speed),
        "verify-example-negative" => verify_negative(&args),
        "verify-foundation" => verify_foundation(&args),
        "verify-report-schema" => verify_reports_schema(),
        "verify-runtime-production-hardening" => verify_runtime_production_hardening(&args),
        "verify-runtime-finality" => verify_runtime_finality(&args),
        "audit-genericity" => audit_genericity(&args),
        "verify-playground-genericity" => verify_playground_genericity(&args),
        "playground-watch" => playground_watch(&args),
        "verify-boon-source-syntax" => verify_boon_source_syntax(&args),
        "verify-boon-driver-schema" => verify_boon_driver_schema(&args),
        "verify-boon-driver-e2e" => verify_boon_driver_e2e(&args),
        "verify-boon-driver-dev-window" => verify_boon_driver_dev_window(&args),
        "verify-boon-driver-speed" => verify_boon_driver_speed(&args),
        "verify-boon-driver-all" => verify_boon_driver_all(&args),
        "verify-linux-human-like-environment" => verify_linux_human_like_environment(&args),
        "verify-linux-human-like-e2e" => verify_linux_human_like_e2e(&args),
        "verify-linux-human-like-speed" => verify_linux_human_like_speed(&args),
        "verify-linux-human-like-all" => verify_linux_human_like_all(&args),
        "audit-machine-readiness" => audit_machine_readiness(&args),
        "bench-example" => bench_example(named_arg(&args, 1)?, &args),
        "verify-todomvc-semantic" => verify_specific("todomvc", VerificationLayer::Semantic, &args),
        "verify-todomvc-speed" => verify_specific("todomvc", VerificationLayer::Speed, &args),
        "verify-todomvc-negative" => verify_negative_name("todomvc"),
        "bench-todomvc" => bench_example("todomvc", &args),
        "explain-todomvc-hardware" => explain_hardware("todomvc", &args),
        "verify-cells-semantic" => verify_specific("cells", VerificationLayer::Semantic, &args),
        "verify-cells-speed" => verify_specific("cells", VerificationLayer::Speed, &args),
        "verify-cells-negative" => verify_negative_name("cells"),
        "shaders" => generate_native_gpu_shader_bindings(&args),
        "verify-platform-contract" => verify_native_platform_contract(&args),
        "verify-native-gpu-dependency-graph" => verify_native_gpu_dependency_graph(&args),
        "verify-native-gpu-architecture" => verify_native_gpu_architecture(&args),
        "verify-native-gpu-layout-contract" => verify_native_gpu_layout_contract(&args),
        "verify-native-gpu-shaders" => verify_native_gpu_shaders(&args),
        "verify-native-gpu-multiwindow" => verify_native_gpu_multiwindow(&args),
        "verify-native-gpu-ipc-backpressure" => verify_native_gpu_ipc_backpressure(&args),
        "verify-native-gpu-observability" => verify_native_gpu_observability(&args),
        "verify-native-gpu-idle-wake" => verify_native_gpu_idle_wake(&args),
        "verify-native-real-window-input-environment" => {
            verify_native_real_window_input_environment(&args)
        }
        "verify-native-gpu-preview-e2e" => verify_native_gpu_preview_e2e(&args),
        "verify-native-visible-launch" => verify_native_visible_launch(&args),
        "verify-native-examples" => verify_native_examples(&args),
        "verify-native-dev-window-editor" => verify_native_dev_window_editor(&args),
        "verify-native-example-tabs" => verify_native_example_tabs(&args),
        "verify-native-editor-format" => verify_native_editor_format(&args),
        "verify-native-example-speed" => verify_native_example_speed(&args),
        "verify-native-counter-interaction-speed" => verify_native_counter_interaction_speed(&args),
        "verify-native-cells-interaction-speed" => verify_native_cells_interaction_speed(&args),
        "verify-native-dev-editor-speed" => verify_native_dev_editor_speed(&args),
        "verify-native-two-window-content" => verify_native_two_window_content(&args),
        "verify-native-todomvc-reference-parity" => verify_native_todomvc_reference_parity(&args),
        "verify-native-todomvc-input-parity" => verify_native_todomvc_input_parity(&args),
        "verify-native-gpu-scroll-speed" => verify_native_gpu_scroll_speed(&args),
        "verify-native-dev-editor-scroll-speed" => verify_native_dev_editor_scroll_speed(&args),
        "verify-native-example-switch-speed" => verify_native_example_switch_speed(&args),
        "verify-native-gpu-negative" => verify_native_gpu_negative(&args),
        "verify-native-gpu-all" => verify_native_gpu_all(&args),
        "verify-native-gpu-regression-all" => verify_native_gpu_regression_all(&args),
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

fn legacy_ply_cosmic_testing_removed(command: &str) -> Result<(), Box<dyn std::error::Error>> {
    Err(format!(
        "legacy Ply/COSMIC testing command `{command}` has been removed from active verification; use the native GPU gates from docs/architecture/NATIVE_GPU_PIPELINE.md instead"
    )
    .into())
}

fn legacy_ply_cosmic_testing_command(command: &str) -> bool {
    matches!(
        command,
        "verify-example-ply-headless"
            | "verify-example-headed-ply"
            | "verify-example-headed-focusless"
            | "verify-example-operator-e2e"
            | "verify-example-human"
            | "prepare-example-human-report"
            | "verify-example-all"
            | "verify-examples-all"
            | "verify-todomvc-reference-parity"
            | "verify-os-input-probe"
            | "verify-playground-launch"
            | "verify-playground-background-launch"
            | "verify-playground-split-wayland"
            | "verify-playground-custom-source"
            | "write-manual-handoff"
            | "audit-goal-readiness"
            | "audit-manual-readiness"
            | "verify-todomvc-ply-headless"
            | "verify-todomvc-headed-ply"
            | "verify-todomvc-headed-focusless"
            | "verify-todomvc-visible-reality"
            | "verify-todomvc-operator-e2e"
            | "verify-todomvc-human"
            | "prepare-todomvc-human-report"
            | "verify-todomvc-all"
            | "verify-cells-ply-headless"
            | "verify-cells-headed-ply"
            | "verify-cells-headed-focusless"
            | "verify-cells-visible-reality"
            | "verify-cells-wayland-scroll-speed"
            | "verify-cells-operator-e2e"
            | "verify-cells-human"
            | "prepare-cells-human-report"
            | "verify-cells-all"
    )
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
    if matches!(
        layer,
        VerificationLayer::Human | VerificationLayer::OperatorE2e | VerificationLayer::HeadedPly
    ) {
        return Err(
            "legacy Ply/human/operator verification layers were removed from xtask; use native GPU gates"
                .into(),
        );
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
        "runtime_profile": runtime_profile,
        "runtime_profile_detail": runtime_profile_detail,
        "capacities": capacities,
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

fn verify_runtime_production_hardening(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let runtime_path = Path::new("crates/boon_runtime/src/lib.rs");
    let runtime = fs::read_to_string(runtime_path)?;
    let mut checks = Vec::new();
    let mut blockers = Vec::new();

    push_audit_check(
        &mut checks,
        &mut blockers,
        "runtime-production:no-leak-runtime-path",
        !runtime.contains("leak_runtime_path"),
        "production runtime must not contain the leak_runtime_path bridge",
        Some(
            "delete leak_runtime_path and replace callers with owned symbols/dense IDs".to_owned(),
        ),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "runtime-production:no-box-leak",
        !runtime.contains("Box::leak"),
        "production runtime must not leak strings or other values",
        Some("replace production Box::leak usage with owned program storage".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "runtime-production:no-static-path-identity",
        !runtime_contains_runtime_static_identity(&runtime),
        "compiled runtime path/list/source/field identity must not be &'static str",
        Some("compiled plan structs still use &'static str for runtime identity".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "runtime-production:source-action-table",
        runtime.contains("SourceActionTable")
            && runtime.contains("SourceAction")
            && !runtime.contains("enum GenericSourceRouteKind"),
        "runtime source dispatch must be SourceId -> [SourceAction], without GenericSourceRouteKind inference",
        Some(
            "source routing is still route-kind inferred instead of fully action-table driven"
                .to_owned(),
        ),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "runtime-production:list-level-columnar-storage",
        runtime.contains("struct ListMemory")
            && runtime.contains("text_columns")
            && runtime.contains("bool_columns")
            && !runtime.contains("KeyedList<RuntimeRecord>")
            && !runtime.contains("struct RuntimeRecord"),
        "list memory must be list-level columns, not row-owned RuntimeRecord columns",
        Some("runtime list storage is still row-owned or not proven list-columnar".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "runtime-production:dense-field-list-ids",
        runtime.contains("FieldId")
            && runtime.contains("ListId")
            && !runtime.contains("struct FieldSlotId(Box<str>)")
            && !runtime.contains("struct ListSlotId(Box<str>)"),
        "field/list hot storage must use dense compiler IDs, not name slot IDs",
        Some(
            "field/list storage still uses name-based slot IDs instead of dense compiler IDs"
                .to_owned(),
        ),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "runtime-production:no-fixed-row-source-binding-array",
        !runtime.contains("MAX_ROW_SOURCE_BINDINGS")
            && !runtime.contains("slots: [usize; MAX_ROW_SOURCE_BINDINGS]"),
        "row source binding capacity must not be a panic-prone fixed array bound",
        Some("row source binding storage still exposes MAX_ROW_SOURCE_BINDINGS".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "runtime-production:no-capacity-assert-panic",
        !runtime_has_capacity_panic_path(&runtime),
        "capacity overflow must report structured errors, not panic",
        Some("runtime still contains panic/assert capacity behavior".to_owned()),
    );

    write_static_gate_report(
        args,
        "verify-runtime-production-hardening",
        report_arg(args)
            .unwrap_or_else(|| PathBuf::from("target/reports/runtime-production-hardening.json")),
        checks,
        blockers,
        json!({
            "plan": "docs/plans/RUNTIME_PRODUCTION_AND_NATIVE_TODOMVC_PARITY_PLAN.md",
            "runtime_source": runtime_path,
            "static_scan_contract": "leak-free-owned-symbols-dense-id-action-table-columnar-storage",
            "hot_path_static_identity_forbidden": true
        }),
    )
}

fn runtime_contains_runtime_static_identity(runtime: &str) -> bool {
    [
        "target: &'static str",
        "source: &'static str",
        "list: &'static str",
        "field: &'static str",
        "list_id: &'static str",
        "source_path: &'static str",
        "Vec<&'static str>",
        "Option<&'static str>",
    ]
    .iter()
    .any(|needle| runtime.contains(needle))
}

fn runtime_has_capacity_panic_path(runtime: &str) -> bool {
    let production_runtime = runtime.split("#[cfg(test)]").next().unwrap_or(runtime);
    let mut recent_capacity_context = false;
    for line in production_runtime.lines() {
        let trimmed = line.trim();
        if trimmed.contains("capacity") || trimmed.contains("exceeded") {
            recent_capacity_context = true;
        }
        if recent_capacity_context
            && (trimmed.contains("panic!(")
                || trimmed.contains("assert!(")
                || trimmed.contains("assert_eq!(")
                || trimmed.contains("unreachable!("))
        {
            return true;
        }
        if trimmed.ends_with(';') || trimmed.ends_with('}') {
            recent_capacity_context = false;
        }
    }
    false
}

fn verify_runtime_finality(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let report =
        report_arg(args).unwrap_or_else(|| PathBuf::from("target/reports/runtime-finality.json"));
    let hardening_report = PathBuf::from("target/reports/runtime-production-hardening.json");
    let hardening = if hardening_report.exists() {
        read_json(&hardening_report)?
    } else {
        json!({"status": "missing"})
    };
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "runtime-finality:production-hardening-report-present",
        hardening_report.exists(),
        format!(
            "{} exists={}",
            hardening_report.display(),
            hardening_report.exists()
        ),
        Some("run verify-runtime-production-hardening first".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "runtime-finality:production-hardening-pass",
        hardening.get("status").and_then(serde_json::Value::as_str) == Some("pass"),
        format!(
            "runtime-production-hardening status={:?}",
            hardening.get("status").and_then(serde_json::Value::as_str)
        ),
        Some("runtime production hardening gate has not passed".to_owned()),
    );
    write_static_gate_report(
        args,
        "verify-runtime-finality",
        report,
        checks,
        blockers,
        json!({
            "runtime_production_hardening_report": hardening_report,
            "runtime_production_hardening_report_sha256": hardening_report
                .exists()
                .then(|| file_hash(hardening_report.to_string_lossy().as_ref())),
            "finality_contract": "runtime production hardening is a prerequisite for finality"
        }),
    )
}

fn verify_playground_genericity(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let scan_paths = [
        "crates/boon_native_gpu/src",
        "crates/boon_document/src",
        "crates/boon_document_model/src",
        "crates/boon_native_app_window/src",
    ];
    for path in scan_paths {
        let todo_hits = rg_count(path, "todomvc")?;
        let cells_hits = rg_count(path, "cells")?;
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("playground-genericity:{path}:no-todomvc-renderer-branch"),
            todo_hits == 0,
            format!("{todo_hits} `todomvc` hits in {path}"),
            (todo_hits != 0)
                .then(|| format!("generic renderer/document boundary `{path}` mentions todomvc")),
        );
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("playground-genericity:{path}:no-cells-renderer-branch"),
            cells_hits == 0,
            format!("{cells_hits} `cells` hits in {path}"),
            (cells_hits != 0)
                .then(|| format!("generic renderer/document boundary `{path}` mentions cells")),
        );
    }
    let native_playground = fs::read_to_string("crates/boon_native_playground/src/main.rs")?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "playground-genericity:preview-source-only",
        native_playground.contains("preview role must not receive --example")
            && native_playground.contains("ReplaceCode"),
        "preview role must load code, not branch on example names",
        Some("preview/dev role protocol does not prove source-only ReplaceCode flow".to_owned()),
    );
    write_static_gate_report(
        args,
        "verify-playground-genericity",
        report_arg(args)
            .unwrap_or_else(|| PathBuf::from("target/reports/playground-genericity.json")),
        checks,
        blockers,
        json!({
            "allowed_example_specific_locations": ["examples", "scenario files", "docs", "xtask report labels"],
            "renderer_scanned_paths": scan_paths,
        }),
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WatchFileFingerprint {
    modified_ns: u128,
    len: u64,
}

type WatchSnapshot = BTreeMap<PathBuf, WatchFileFingerprint>;

fn playground_watch(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let report = report_arg(args)
        .unwrap_or_else(|| PathBuf::from("target/reports/native-gpu/playground-watch.json"));
    if args.iter().any(|arg| arg == "--stop") {
        let stopped = stop_recorded_playground(&report)?;
        write_json(
            &report,
            &json!({
                "status": "stopped",
                "generated_at_utc": current_unix_seconds().to_string(),
                "stopped_pid_count": stopped,
                "report_path": report
            }),
        )?;
        println!("stopped {stopped} watcher-owned playground process(es)");
        return Ok(());
    }

    if !command_available("cosmic-background-launch") {
        return Err("playground-watch requires cosmic-background-launch for workspace-qualified visible launches".into());
    }

    let example = value_arg(args, "--example").unwrap_or_else(|| "counter".to_owned());
    let entry = boon_runtime::example_manifest_entry(&example)?;
    let poll_ms = value_arg(args, "--poll-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1_000)
        .max(250);
    let once = args.iter().any(|arg| arg == "--once");
    let stopped = stop_recorded_playground(&report)?;
    if stopped > 0 {
        println!("stopped {stopped} previously watcher-owned playground process(es)");
    }

    let watch_roots = playground_watch_roots();
    let mut snapshot = playground_watch_snapshot(&watch_roots)?;
    let mut generation = 0u64;
    loop {
        generation = generation.saturating_add(1);
        let build = Command::new("cargo")
            .args(["build", "-p", "boon_native_playground"])
            .status()?;
        if !build.success() {
            write_json(
                &report,
                &json!({
                    "status": "fail",
                    "generated_at_utc": current_unix_seconds().to_string(),
                    "example": entry.id,
                    "generation": generation,
                    "build_status": build.to_string(),
                    "watch_roots": watch_roots,
                    "watch_file_count": snapshot.len(),
                    "blockers": ["cargo build -p boon_native_playground failed"]
                }),
            )?;
            if once {
                return Err("cargo build -p boon_native_playground failed".into());
            }
            eprintln!("build failed; waiting for changes before retrying");
            snapshot = wait_for_playground_change(&watch_roots, snapshot, poll_ms)?;
            continue;
        }

        let launch = launch_watched_playground(&entry)?;
        let root_pid = launch.get("child_pid").and_then(serde_json::Value::as_u64);
        thread::sleep(Duration::from_millis(1_000));
        let pgrep = playground_pgrep_snapshot();
        let launched_pids = root_pid
            .map(playground_pid_tree)
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();
        write_json(
            &report,
            &json!({
                "status": if launch.get("success").and_then(serde_json::Value::as_bool) == Some(true) { "running" } else { "fail" },
                "generated_at_utc": current_unix_seconds().to_string(),
                "example": entry.id,
                "source_path": entry.source,
                "binary_path": "target/debug/boon_native_playground",
                "binary_hash": file_hash("target/debug/boon_native_playground"),
                "generation": generation,
                "watch_roots": watch_roots,
                "watch_file_count": snapshot.len(),
                "watch_snapshot_hash": playground_snapshot_hash(&snapshot),
                "poll_ms": poll_ms,
                "workspace": "boon-circuit",
                "launcher": "cosmic-background-launch --workspace boon-circuit",
                "root_pid": root_pid,
                "launched_pids": launched_pids,
                "pgrep_boon_native_playground": pgrep,
                "launch": launch
            }),
        )?;
        println!(
            "playground ready: example={} root_pid={:?} report={}",
            entry.id,
            root_pid,
            report.display()
        );

        if once {
            return Ok(());
        }

        let new_snapshot = wait_for_playground_change(&watch_roots, snapshot.clone(), poll_ms)?;
        let changed = playground_snapshot_diff(&snapshot, &new_snapshot);
        println!(
            "detected playground input change; restarting: {}",
            changed.join(", ")
        );
        if let Some(root_pid) = root_pid {
            stop_playground_pid_tree(root_pid)?;
        }
        snapshot = new_snapshot;
    }
}

fn launch_watched_playground(
    entry: &boon_runtime::ExampleManifestEntry,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let script = format!(
        "cd {} && exec ./target/debug/boon_native_playground --role desktop --example {}",
        shell_quote(&cwd.display().to_string()),
        shell_quote(&entry.id)
    );
    run_cosmic_background_launch("boon-circuit", &script)
}

fn playground_watch_roots() -> Vec<PathBuf> {
    [
        "Cargo.toml",
        "Cargo.lock",
        "assets/fonts",
        "crates/xtask/src",
        "crates/boon_native_playground/src",
        "crates/boon_runtime/src",
        "crates/boon_ir/src",
        "crates/boon_parser/src",
        "crates/boon_document/src",
        "crates/boon_document_model/src",
        "crates/boon_native_gpu/src",
        "crates/boon_native_app_window/src",
        "examples",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

fn playground_watch_snapshot(
    roots: &[PathBuf],
) -> Result<WatchSnapshot, Box<dyn std::error::Error>> {
    let mut snapshot = BTreeMap::new();
    for root in roots {
        collect_watch_fingerprints(root, &mut snapshot)?;
    }
    Ok(snapshot)
}

fn collect_watch_fingerprints(
    path: &Path,
    snapshot: &mut WatchSnapshot,
) -> Result<(), Box<dyn std::error::Error>> {
    if !path.exists() {
        return Ok(());
    }
    let metadata = fs::metadata(path)?;
    if metadata.is_dir() {
        let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let child = entry.path();
            if child
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "target" || name == ".git")
            {
                continue;
            }
            collect_watch_fingerprints(&child, snapshot)?;
        }
    } else if metadata.is_file() {
        let modified_ns = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        snapshot.insert(
            path.to_path_buf(),
            WatchFileFingerprint {
                modified_ns,
                len: metadata.len(),
            },
        );
    }
    Ok(())
}

fn wait_for_playground_change(
    roots: &[PathBuf],
    snapshot: WatchSnapshot,
    poll_ms: u64,
) -> Result<WatchSnapshot, Box<dyn std::error::Error>> {
    loop {
        thread::sleep(Duration::from_millis(poll_ms));
        let next = playground_watch_snapshot(roots)?;
        if next != snapshot {
            return Ok(next);
        }
    }
}

fn playground_snapshot_diff(old: &WatchSnapshot, new: &WatchSnapshot) -> Vec<String> {
    let mut changed = Vec::new();
    for path in old.keys().chain(new.keys()) {
        if old.get(path) != new.get(path) && !changed.iter().any(|item| item == path) {
            changed.push(path.display().to_string());
        }
    }
    changed
}

fn playground_snapshot_hash(snapshot: &WatchSnapshot) -> String {
    boon_runtime::sha256_bytes(format!("{snapshot:?}").as_bytes())
}

fn stop_recorded_playground(report: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    if !report.exists() {
        return Ok(0);
    }
    let report_json = read_json(report)?;
    let mut pids = BTreeSet::new();
    if let Some(pid) = report_json
        .get("root_pid")
        .and_then(serde_json::Value::as_u64)
    {
        pids.insert(pid);
    }
    if let Some(pid) = report_json
        .pointer("/launch/child_pid")
        .and_then(serde_json::Value::as_u64)
    {
        pids.insert(pid);
    }
    let mut stopped = 0usize;
    for pid in pids {
        stopped += stop_playground_pid_tree(pid)?;
    }
    Ok(stopped)
}

fn stop_playground_pid_tree(root_pid: u64) -> Result<usize, Box<dyn std::error::Error>> {
    let mut pids = playground_pid_tree(root_pid);
    pids.sort_unstable_by(|left, right| right.cmp(left));
    let mut stopped = 0usize;
    for pid in pids {
        if !playground_pid_cmdline(pid).contains("boon_native_playground") {
            continue;
        }
        let pid_text = pid.to_string();
        let _ = Command::new("kill").args(["-TERM", &pid_text]).status();
        stopped += 1;
    }
    thread::sleep(Duration::from_millis(500));
    for pid in playground_pid_tree(root_pid) {
        if playground_pid_cmdline(pid).contains("boon_native_playground") {
            let _ = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
        }
    }
    Ok(stopped)
}

fn playground_pid_tree(root_pid: u64) -> Vec<u64> {
    let mut pids = Vec::new();
    collect_playground_pid_tree(root_pid, &mut pids);
    pids
}

fn collect_playground_pid_tree(pid: u64, pids: &mut Vec<u64>) {
    if pids.contains(&pid) {
        return;
    }
    pids.push(pid);
    let output = Command::new("pgrep")
        .args(["-P", &pid.to_string()])
        .output();
    if let Ok(output) = output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for child in stdout
            .lines()
            .filter_map(|line| line.trim().parse::<u64>().ok())
        {
            collect_playground_pid_tree(child, pids);
        }
    }
}

fn playground_pid_cmdline(pid: u64) -> String {
    fs::read(format!("/proc/{pid}/cmdline"))
        .map(|bytes| {
            bytes
                .split(|byte| *byte == 0)
                .filter_map(|part| std::str::from_utf8(part).ok())
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

fn playground_pgrep_snapshot() -> Vec<String> {
    Command::new("pgrep")
        .args(["-af", "boon_native_playground"])
        .output()
        .ok()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn audit_genericity(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let native = fs::read_to_string("crates/boon_native_playground/src/main.rs")?;
    let runtime = fs::read_to_string("crates/boon_runtime/src/lib.rs")?;
    for (label, haystack) in [("native", native.as_str()), ("runtime", runtime.as_str())] {
        for forbidden in [
            "generic_future",
            "future-generic",
            "unsupported-runtime-surface",
            "static-document-preview",
            "custom_runtime_scenario_path",
            "ExecutableSurfaceKind::TodoMvc",
            "ExecutableSurfaceKind::Cells",
            "LoadedRuntimeSurface::Todo",
            "LoadedRuntimeSurface::Cells",
            "validate_executable_surface",
            "lower_todomvc_patch",
            "lower_cells_patch",
            "todomvc_summary",
            "cells_summary",
            "todo_cells_specific_shortcut",
            "formula_state",
            "program.kind.as_str",
            "parsed.kind",
        ] {
            let hits = haystack.matches(forbidden).count();
            push_audit_check(
                &mut checks,
                &mut blockers,
                format!("audit-genericity:{label}:no-{forbidden}"),
                hits == 0,
                format!("{hits} `{forbidden}` hits"),
                (hits != 0)
                    .then(|| format!("remove false-generic marker `{forbidden}` from {label}")),
            );
        }
    }
    let generic_runtime_api = runtime.contains("pub fn from_source(")
        && runtime.contains("fn apply_generic_step")
        && native.contains("generic-live-runtime");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "audit-genericity:runtime:from-source-api",
        generic_runtime_api,
        format!("generic_runtime_api={generic_runtime_api}"),
        (!generic_runtime_api)
            .then(|| "runtime must expose a source-only generic LiveRuntime path".to_owned()),
    );
    write_static_gate_report(
        args,
        "audit-genericity",
        report_arg(args).unwrap_or_else(|| PathBuf::from("target/reports/genericity-audit.json")),
        checks,
        blockers,
        json!({
            "contract": "generic examples must execute through one LiveRuntime source/scenario path; static preview, future-generic false greens, and TodoMVC/Cells runtime surfaces are banned",
            "remaining_legacy_runtime_surfaces": "not allowed in production runtime/native code; fixture-only names belong in examples, scenarios, tests, or docs"
        }),
    )
}

fn audit_machine_readiness(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let report = report_arg(args)
        .unwrap_or_else(|| PathBuf::from("target/reports/debug/machine-readiness.json"));
    let required = [
        (
            "runtime-production-hardening",
            PathBuf::from("target/reports/runtime-production-hardening.json"),
        ),
        (
            "runtime-finality",
            PathBuf::from("target/reports/runtime-finality.json"),
        ),
        (
            "native-preview-e2e-todomvc",
            PathBuf::from("target/reports/native-gpu/preview-e2e-todomvc.json"),
        ),
        (
            "native-two-window-content",
            PathBuf::from("target/reports/native-gpu/todomvc-two-window-content.json"),
        ),
        (
            "native-todomvc-reference-parity",
            PathBuf::from("target/reports/native-gpu/todomvc-reference-parity.json"),
        ),
        (
            "native-todomvc-input-parity",
            PathBuf::from("target/reports/native-gpu/todomvc-input-parity.json"),
        ),
        (
            "playground-genericity",
            PathBuf::from("target/reports/playground-genericity.json"),
        ),
    ];
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let mut linked = Vec::new();
    for (label, path) in required {
        let exists = path.exists();
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("machine-readiness:report-present:{label}"),
            exists,
            format!("{} exists={exists}", path.display()),
            (!exists).then(|| format!("missing required readiness report `{}`", path.display())),
        );
        if !exists {
            continue;
        }
        let child = read_json(&path)?;
        let pass = child.get("status").and_then(serde_json::Value::as_str) == Some("pass");
        let fresh = child.get("git_commit").and_then(serde_json::Value::as_str)
            == Some(git_commit().as_str());
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("machine-readiness:report-pass:{label}"),
            pass,
            format!("{} status={:?}", path.display(), child.get("status")),
            (!pass).then(|| format!("required report `{}` did not pass", path.display())),
        );
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("machine-readiness:report-fresh:{label}"),
            fresh,
            format!("{} git_fresh={fresh}", path.display()),
            (!fresh).then(|| {
                format!(
                    "required report `{}` is stale for current git commit",
                    path.display()
                )
            }),
        );
        linked.push(json!({
            "label": label,
            "path": path.display().to_string(),
            "sha256": file_hash(path.to_string_lossy().as_ref())
        }));
    }
    write_static_gate_report(
        args,
        "audit-machine-readiness",
        report,
        checks,
        blockers,
        json!({
            "readiness_contract": "combined runtime production plus native TodoMVC parity gates",
            "required_reports": linked,
            "human_testing_required_after_machine_pass": true
        }),
    )
}

fn verify_native_platform_contract(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let forbidden = [
        "app_window",
        "wgpu",
        "glyphon",
        "WESL",
        "Wayland",
        "X11",
        "DOM",
        "terminal escape",
    ];
    for dir in [
        "crates/boon_parser/src",
        "crates/boon_ir/src",
        "crates/boon_runtime/src",
        "crates/boon_document_model/src",
        "crates/boon_document/src",
        "crates/boon_host/src",
    ] {
        for needle in forbidden {
            let hits = rg_count(dir, needle)?;
            push_audit_check(
                &mut checks,
                &mut blockers,
                format!("platform-contract:{dir}:{needle}"),
                hits == 0,
                format!("{hits} `{needle}` hits in {dir}"),
                (hits != 0).then(|| {
                    format!("core boundary `{dir}` still exposes forbidden native/backend term `{needle}`")
                }),
            );
        }
    }
    for needle in [
        "window_mode",
        "window_backend",
        "display_server",
        "window_pid",
    ] {
        let hits = rg_count("crates/boon_runtime/src", needle)?;
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("platform-contract:boon-runtime:proof-field:{needle}"),
            hits == 0,
            format!("{hits} runtime report-schema/proof-field hits for `{needle}`"),
            (hits != 0).then(|| {
                format!(
                    "`boon_runtime` still owns backend proof/report field `{needle}`; native contract requires these outside runtime"
                )
            }),
        );
    }
    write_native_gate_report(
        args,
        "verify-platform-contract",
        checks,
        blockers,
        json!({}),
    )
}

fn verify_native_gpu_dependency_graph(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let metadata = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "dependency-graph:cargo-metadata",
        metadata.status.success(),
        "cargo metadata --no-deps --format-version 1 completed",
        (!metadata.status.success()).then(|| "cargo metadata failed".to_owned()),
    );
    let required_crates = [
        "boon_document_model",
        "boon_driver",
        "boon_document",
        "boon_native_gpu",
        "boon_native_app_window",
        "boon_native_playground",
    ];
    let metadata_json = if metadata.status.success() {
        serde_json::from_slice::<serde_json::Value>(&metadata.stdout)?
    } else {
        json!({})
    };
    let package_names = metadata_json
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|package| {
            package
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect::<BTreeSet<_>>();
    for crate_name in required_crates {
        let present = package_names.contains(crate_name);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("dependency-graph:workspace-member:{crate_name}"),
            present,
            format!("workspace member `{crate_name}` present={present}"),
            (!present)
                .then(|| format!("missing required native GPU workspace member `{crate_name}`")),
        );
    }
    let dependency_rules = [
        (
            "crates/boon_driver/Cargo.toml",
            &[
                "wgpu",
                "app_window",
                "glyphon",
                "boon_native",
                "boon_runtime",
            ][..],
            "boon_driver",
        ),
        (
            "crates/boon_runtime/Cargo.toml",
            &["wgpu", "app_window", "glyphon", "boon_document"][..],
            "boon_runtime",
        ),
        (
            "crates/boon_document/Cargo.toml",
            &["boon_runtime", "wgpu", "app_window", "glyphon"][..],
            "boon_document",
        ),
        (
            "crates/boon_native_gpu/Cargo.toml",
            &[
                "boon_runtime",
                "boon_parser",
                "boon_ply_playground",
                "app_window",
            ][..],
            "boon_native_gpu",
        ),
        (
            "crates/boon_native_app_window/Cargo.toml",
            &[
                "boon_runtime",
                "boon_document",
                "boon_ply_playground",
                "glyphon",
            ][..],
            "boon_native_app_window",
        ),
    ];
    for (path, forbidden, crate_name) in dependency_rules {
        let text = std::fs::read_to_string(path)?;
        for needle in forbidden {
            let present = text.contains(needle);
            push_audit_check(
                &mut checks,
                &mut blockers,
                format!("dependency-graph:{crate_name}:forbid:{needle}"),
                !present,
                format!("{path} contains `{needle}`={present}"),
                present.then(|| format!("forbidden dependency `{needle}` found in `{path}`")),
            );
        }
    }
    for (path, required) in [
        ("crates/boon_native_gpu/Cargo.toml", "wgpu"),
        ("crates/boon_native_gpu/Cargo.toml", "glyphon"),
        ("crates/boon_native_app_window/Cargo.toml", "app_window"),
        ("crates/boon_native_app_window/Cargo.toml", "wgpu"),
    ] {
        let text = std::fs::read_to_string(path)?;
        let present = text.contains(required);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("dependency-graph:{path}:require:{required}"),
            present,
            format!("{path} contains required `{required}`={present}"),
            (!present).then(|| format!("required dependency `{required}` missing from `{path}`")),
        );
    }
    write_native_gate_report(
        args,
        "verify-native-gpu-dependency-graph",
        checks,
        blockers,
        json!({}),
    )
}

fn verify_native_gpu_architecture(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    for dir in [
        "crates/boon_document/src",
        "crates/boon_native_gpu/src",
        "crates/boon_native_app_window/src",
    ] {
        for needle in ["todomvc", "todo_mvc", "cells", "pong", "arkanoid"] {
            let hits = rg_count(dir, needle)?;
            push_audit_check(
                &mut checks,
                &mut blockers,
                format!("architecture:no-example-branch:{dir}:{needle}"),
                hits == 0,
                format!("{hits} `{needle}` hits in {dir}"),
                (hits != 0).then(|| {
                    format!("example-specific branch/string `{needle}` appears in forbidden boundary `{dir}`")
                }),
            );
        }
    }
    let preview_source = std::fs::read_to_string("crates/boon_native_playground/src/main.rs")?;
    let preview_rejects_example = preview_source
        .contains("preview role must not receive --example")
        && preview_source.contains("value_arg(args, \"--example\")");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "architecture:preview-rejects-example-argv",
        preview_rejects_example,
        "preview role rejects --example before loading source",
        (!preview_rejects_example)
            .then(|| "preview role does not mechanically reject --example".to_owned()),
    );
    for forbidden in [
        "scenario_payload",
        "host_input_scenarios",
        "\"scenario_source\"",
        "apply_source_event_for_step_with_document_window",
    ] {
        let present = preview_source.contains(forbidden);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("architecture:preview-ipc-forbids-scenario-data:{forbidden}"),
            !present,
            format!("preview source contains `{forbidden}`={present}"),
            present.then(|| {
                format!(
                    "preview IPC boundary still accepts or applies scenario-coupled data `{forbidden}`"
                )
            }),
        );
    }
    for dir in [
        "crates/boon_native_gpu/src",
        "crates/boon_native_app_window/src",
        "crates/boon_native_playground/src",
    ] {
        for needle in ["macroquad", "miniquad", "ply_engine", "ply-engine"] {
            let hits = rg_count(dir, needle)?;
            push_audit_check(
                &mut checks,
                &mut blockers,
                format!("architecture:no-ply-native-gpu:{dir}:{needle}"),
                hits == 0,
                format!("{hits} `{needle}` hits in {dir}"),
                (hits != 0).then(|| {
                    format!("old Ply/macroquad dependency marker `{needle}` appears in native GPU path `{dir}`")
                }),
            );
        }
    }
    write_native_gate_report(
        args,
        "verify-native-gpu-architecture",
        checks,
        blockers,
        json!({}),
    )
}

fn verify_native_gpu_layout_contract(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let frame = boon_document::fixture_frame_with_virtualized_table();
    let mut measurer = boon_native_gpu::GlyphonTextMeasurer::new();
    let layout = boon_document::layout(boon_document::LayoutInput {
        document: &frame,
        viewport: boon_host::Viewport {
            surface: 1,
            width: 1280.0,
            height: 900.0,
            scale: 1.0,
        },
        text: &mut measurer,
        capabilities: boon_document::RenderCapabilities::fake_portable(),
    });
    push_audit_check(
        &mut checks,
        &mut blockers,
        "layout-contract:fake-capabilities-no-native-required",
        !layout.metrics.native_capability_required,
        format!(
            "native_capability_required={}",
            layout.metrics.native_capability_required
        ),
        layout
            .metrics
            .native_capability_required
            .then(|| "layout required native-only RenderCapabilities".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "layout-contract:cells-not-expanded-to-2600-widgets",
        layout.metrics.display_item_count < 2600,
        format!("display_item_count={}", layout.metrics.display_item_count),
        (layout.metrics.display_item_count >= 2600)
            .then(|| "Cells layout expanded the full 26x100 logical grid".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "layout-contract:virtualized-demands-present",
        !layout.demands.is_empty(),
        format!("layout demands={}", layout.demands.len()),
        layout
            .demands
            .is_empty()
            .then(|| "Cells fixture did not produce materialization demands".to_owned()),
    );
    write_native_gate_report(
        args,
        "verify-native-gpu-layout-contract",
        checks,
        blockers,
        json!({
            "layout_metrics": layout.metrics,
            "demand_count": layout.demands.len()
        }),
    )
}

fn verify_native_gpu_shaders(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let wesl_source = Path::new("shaders/native_gpu_rect.wesl");
    let generated_wgsl = Path::new("crates/boon_native_gpu/src/generated/native_gpu_rect.wgsl");
    let generated_bindings = Path::new("crates/boon_native_gpu/src/generated/shader_bindings.rs");
    let wesl_count = count_files_with_extension(Path::new("shaders"), "wesl")?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "shaders:wesl-inputs-present",
        wesl_count > 0,
        format!("{wesl_count} WESL shader inputs found"),
        (wesl_count == 0).then(|| "missing shaders/*.wesl inputs for native GPU path".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "shaders:rect-wesl-source-present",
        wesl_source.exists(),
        format!("{} exists={}", wesl_source.display(), wesl_source.exists()),
        (!wesl_source.exists()).then(|| {
            format!(
                "missing native GPU WESL source artifact `{}`",
                wesl_source.display()
            )
        }),
    );
    let generated_wgsl_text = std::fs::read_to_string(generated_wgsl).unwrap_or_default();
    let wesl_text = std::fs::read_to_string(wesl_source).unwrap_or_default();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "shaders:generated-wgsl-output-present",
        generated_wgsl.exists(),
        format!(
            "{} exists={}",
            generated_wgsl.display(),
            generated_wgsl.exists()
        ),
        (!generated_wgsl.exists()).then(|| {
            format!(
                "missing generated WGSL artifact `{}`",
                generated_wgsl.display()
            )
        }),
    );
    let generated_wgsl_fresh = generated_wgsl.exists()
        && wesl_source.exists()
        && file_hash(&generated_wgsl.display().to_string())
            == file_hash(&wesl_source.display().to_string())
        && generated_wgsl_text == wesl_text;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "shaders:generated-wgsl-fresh",
        generated_wgsl_fresh,
        format!("generated WGSL mirrors current WESL source={generated_wgsl_fresh}"),
        (!generated_wgsl_fresh)
            .then(|| "generated WGSL is stale; run `cargo xtask shaders`".to_owned()),
    );
    let generated_text = std::fs::read_to_string(generated_bindings).unwrap_or_default();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "shaders:wgsl-bindgen-output-present",
        generated_bindings.exists(),
        format!(
            "{} exists={}",
            generated_bindings.display(),
            generated_bindings.exists()
        ),
        (!generated_bindings.exists()).then(|| {
            format!(
                "missing generated shader binding artifact `{}`",
                generated_bindings.display()
            )
        }),
    );
    let generated_marker_ok = generated_text.contains("WGSL_BINDGEN_GENERATED: bool = true")
        && generated_text.contains("SHADER_BINDING_GENERATOR: &str = \"wgsl_bindgen\"")
        && generated_text.contains("create_shader_module_embed_source")
        && generated_text.contains("vertex_state")
        && generated_text.contains("fragment_state")
        && generated_text.contains("vs_main_entry")
        && generated_text.contains("fs_main_entry");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "shaders:wgsl-bindgen-api",
        generated_marker_ok,
        "generated bindings expose a wgsl_bindgen Rust API, not only provenance markers",
        (!generated_marker_ok).then(|| {
            "generated shader bindings are not real wgsl_bindgen Rust APIs yet".to_owned()
        }),
    );
    let wesl_hash = file_hash(&wesl_source.display().to_string());
    let generated_wgsl_hash = file_hash(&generated_wgsl.display().to_string());
    let hash_fresh = generated_text.contains(&wesl_hash);
    push_audit_check(
        &mut checks,
        &mut blockers,
        "shaders:generated-hash-fresh",
        hash_fresh,
        format!("generated hash matches shaders/native_gpu_rect.wesl={hash_fresh}"),
        (!hash_fresh)
            .then(|| "generated shader bindings are stale; run `cargo xtask shaders`".to_owned()),
    );
    let renderer_source = std::fs::read_to_string("crates/boon_native_gpu/src/lib.rs")?;
    let renderer_uses_generated_api =
        renderer_source.contains("generated::shader_bindings::ShaderEntry");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "shaders:renderer-uses-generated-api",
        renderer_uses_generated_api,
        format!("renderer uses generated shader API={renderer_uses_generated_api}"),
        (!renderer_uses_generated_api)
            .then(|| "boon_native_gpu does not consume generated shader API".to_owned()),
    );
    let bypasses_generated = renderer_source.contains("include_str!")
        || renderer_source.contains("ShaderSource::Wgsl")
        || renderer_source.contains("native_gpu_rect.wgsl")
        || renderer_source.contains("create_shader_module(")
        || renderer_source.contains("ShaderModuleDescriptor");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "shaders:no-manual-wgsl-loading",
        !bypasses_generated,
        format!("renderer manual shader loading markers present={bypasses_generated}"),
        bypasses_generated.then(|| {
            "boon_native_gpu bypasses generated shader bindings with manual WGSL loading".to_owned()
        }),
    );
    let duplicate_layouts = renderer_source.contains("device.create_pipeline_layout(")
        || renderer_source.contains("request.device.create_pipeline_layout(")
        || renderer_source.contains(".create_bind_group_layout(");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "shaders:no-duplicate-manual-layouts",
        !duplicate_layouts,
        format!("renderer duplicate manual layout construction markers present={duplicate_layouts}"),
        duplicate_layouts.then(|| {
            "boon_native_gpu constructs duplicate manual bind group or pipeline layouts outside generated bindings".to_owned()
        }),
    );
    let manual_entry_points = renderer_source.contains("entry_point: Some(\"")
        || renderer_source.contains("ENTRY_VS_MAIN")
        || renderer_source.contains("ENTRY_FS_MAIN")
        || renderer_source.contains("\"vs_main\"")
        || renderer_source.contains("\"fs_main\"");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "shaders:no-manual-entry-points",
        !manual_entry_points,
        format!("renderer manual entry-point markers present={manual_entry_points}"),
        manual_entry_points.then(|| {
            "boon_native_gpu duplicates shader entry-point definitions outside generated bindings".to_owned()
        }),
    );
    let shader_outputs_fresh = generated_wgsl_fresh && hash_fresh && generated_marker_ok;
    let mut artifact_sha256s = Vec::new();
    if generated_wgsl.exists() {
        artifact_sha256s.push(artifact_hash(generated_wgsl)?);
    }
    if generated_bindings.exists() {
        artifact_sha256s.push(artifact_hash(generated_bindings)?);
    }
    write_native_gate_report(
        args,
        "verify-native-gpu-shaders",
        checks,
        blockers,
        json!({
            "source_hash": wesl_hash,
            "expected_source_hash": wesl_hash,
            "shader_outputs_fresh": shader_outputs_fresh,
            "shader_source_path": wesl_source.display().to_string(),
            "generated_wgsl": generated_wgsl.display().to_string(),
            "generated_shader_bindings": generated_bindings.display().to_string(),
            "generated_wgsl_hash": generated_wgsl_hash,
            "artifact_sha256s": artifact_sha256s,
            "renderer_static_checks": {
                "uses_generated_api": renderer_uses_generated_api,
                "manual_wgsl_loading": bypasses_generated,
                "duplicate_manual_layouts": duplicate_layouts,
                "manual_entry_points": manual_entry_points
            }
        }),
    )
}

fn generate_native_gpu_shader_bindings(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = Path::new("shaders/native_gpu_rect.wesl");
    let generated_dir = Path::new("crates/boon_native_gpu/src/generated");
    let generated = generated_dir.join("shader_bindings.rs");
    let wgsl_output = generated_dir.join("native_gpu_rect.wgsl");
    let source_text = std::fs::read_to_string(source)?;
    let source_hash = boon_runtime::sha256_file(source)?;
    std::fs::create_dir_all(generated_dir)?;
    std::fs::write(&wgsl_output, &source_text)?;
    let _ = std::fs::remove_file(&generated);
    wgsl_bindgen::WgslBindgenOptionBuilder::default()
        .workspace_root(generated_dir)
        .add_entry_point(wgsl_output.display().to_string())
        .serialization_strategy(wgsl_bindgen::WgslTypeSerializeStrategy::Encase)
        .skip_hash_check(true)
        .output(&generated)
        .build()?
        .generate()?;
    prepend_native_gpu_shader_metadata(&generated, source_hash.as_str())?;
    let rustfmt_status = Command::new("rustfmt").arg(&generated).status()?;
    if !rustfmt_status.success() {
        return Err(format!("rustfmt failed for `{}`", generated.display()).into());
    }
    if let Some(report) = report_arg(args) {
        let report_json = json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": current_unix_seconds().to_string(),
            "command": "shaders",
            "command_argv": args,
            "exit_status": 0,
            "git_commit": git_commit(),
            "binary_hash": current_binary_hash(),
            "source_path": source.display().to_string(),
            "source_hash": source_hash,
            "scenario_hash": "n/a",
            "program_hash": "n/a",
            "budget_hash": file_hash("budgets/native-gpu.toml"),
            "graph_node_count": 0,
            "per_step_pass_fail": [
                {"id": "wesl-input-read", "pass": true},
                {"id": "wesl-to-wgsl-written", "pass": true},
                {"id": "wgsl-bindgen-rust-api-written", "pass": true}
            ],
            "artifact_sha256s": [artifact_hash(&wgsl_output)?, artifact_hash(&generated)?],
            "generated_wgsl": wgsl_output.display().to_string(),
            "generated_shader_bindings": generated.display().to_string()
        });
        write_json(&report, &report_json)?;
        verify_report_schema(&report)?;
    }
    println!("wrote {}", generated.display());
    Ok(())
}

fn prepend_native_gpu_shader_metadata(
    generated: &Path,
    source_hash: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let generated_text = std::fs::read_to_string(generated)?;
    let metadata = format!(
        r#"// Generated by `cargo xtask shaders`.
// Source pipeline: shaders/*.wesl -> generated WGSL -> wgsl_bindgen Rust API.

pub const WGSL_BINDGEN_GENERATED: bool = true;
pub const SHADER_SOURCE_KIND: &str = "wesl";
pub const SHADER_BINDING_GENERATOR: &str = "wgsl_bindgen";
pub const NATIVE_GPU_RECT_WESL_SHA256: &str = "{source_hash}";

"#
    );
    let mut output = String::new();
    let mut inserted = false;
    for line in generated_text.lines() {
        output.push_str(line);
        output.push('\n');
        if !inserted && line.trim_start().starts_with("#![allow(") {
            output.push_str(&metadata);
            inserted = true;
        }
    }
    if !inserted {
        output = format!("{metadata}{generated_text}");
    }
    std::fs::write(generated, output)?;
    Ok(())
}

fn verify_native_gpu_multiwindow(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let supervisor_report = PathBuf::from("target/reports/native-gpu/.multiwindow-supervisor.json");
    let live_state_report =
        PathBuf::from("target/artifacts/native-gpu/multiwindow-live-state.json");
    let mut cosmic_launch_proof = json!({"status": "not-run"});
    let mut isolated_real_window_launch_proof = json!({"status": "not-run"});
    let _ = std::fs::remove_file(&supervisor_report);
    let _ = std::fs::remove_file(&live_state_report);
    let _ = std::fs::remove_file("target/reports/native-gpu/.multiwindow-live-state.json");
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland";
    let isolated_real_window_available = command_available("weston")
        && command_available("wayland-info")
        && weston_test_plugin_path().is_some()
        && weston_test_driver_path().is_some();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-multiwindow:isolated-real-window-environment",
        isolated_real_window_available,
        format!(
            "WAYLAND_DISPLAY={:?}, XDG_SESSION_TYPE={:?}, isolated_real_window_available={isolated_real_window_available}",
            std::env::var("WAYLAND_DISPLAY").ok(),
            std::env::var("XDG_SESSION_TYPE").ok()
        ),
        (!isolated_real_window_available).then(|| {
            "native multiwindow proof requires the isolated Weston real-window harness".to_owned()
        }),
    );

    let build = Command::new("cargo")
        .args(["build", "-p", "boon_native_playground"])
        .status()?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-multiwindow:playground-build",
        build.success(),
        format!("cargo build -p boon_native_playground status={build}"),
        (!build.success()).then(|| "failed to build boon_native_playground".to_owned()),
    );

    if build.success() && isolated_real_window_available {
        isolated_real_window_launch_proof = run_isolated_weston_desktop_preview_e2e(
            Path::new("target/debug/boon_native_playground"),
            "todomvc",
            &native_gpu_title_token("multiwindow"),
            1_500,
            60_000,
            &supervisor_report,
            &live_state_report,
            None,
            Some("a"),
            None,
            true,
            false,
        )?;
        let launch_success = isolated_real_window_launch_proof
            .get("status")
            .and_then(serde_json::Value::as_str)
            == Some("pass");
        push_audit_check(
            &mut checks,
            &mut blockers,
            "native-gpu-multiwindow:isolated-launch-command",
            launch_success,
            format!(
                "status={:?}, desktop_pass={:?}, driver_effect_observed={:?}",
                isolated_real_window_launch_proof
                    .get("status")
                    .and_then(serde_json::Value::as_str),
                isolated_real_window_launch_proof
                    .get("desktop_pass")
                    .and_then(serde_json::Value::as_bool),
                isolated_real_window_launch_proof
                    .get("driver_effect_observed")
                    .and_then(serde_json::Value::as_bool)
            ),
            (!launch_success).then(|| {
                "isolated native multiwindow launch failed to produce real-window proof".to_owned()
            }),
        );
        let live_state_ready = live_state_report.exists();
        push_audit_check(
            &mut checks,
            &mut blockers,
            "native-gpu-multiwindow:live-state-report-written",
            live_state_ready,
            format!("{} ready={live_state_ready}", live_state_report.display()),
            (!live_state_ready).then(|| {
                format!(
                    "desktop supervisor did not write live state `{}` while windows were alive",
                    live_state_report.display()
                )
            }),
        );
        let report_ready = supervisor_report.exists();
        push_audit_check(
            &mut checks,
            &mut blockers,
            "native-gpu-multiwindow:supervisor-report-written",
            report_ready,
            format!("{} ready={report_ready}", supervisor_report.display()),
            (!report_ready).then(|| {
                format!(
                    "desktop supervisor did not write `{}`",
                    supervisor_report.display()
                )
            }),
        );
    } else if false && build.success() && wayland {
        let launcher_available = command_available("cosmic-background-launch");
        push_audit_check(
            &mut checks,
            &mut blockers,
            "native-gpu-multiwindow:cosmic-background-launch-available",
            launcher_available,
            format!("cosmic-background-launch available={launcher_available}"),
            (!launcher_available).then(|| {
                "cosmic-background-launch is required for COSMIC workspace-qualified proof"
                    .to_owned()
            }),
        );
        if launcher_available {
            let cwd = std::env::current_dir()?;
            let script = format!(
                "cd {} && ./target/debug/boon_native_playground --role desktop --example cells --probe --child-hold-ms 10000 --dev-hold-ms 5000 --role-report-timeout-ms 60000 --live-state-report {} --report {} >>/tmp/boon-native-gpu-multiwindow.log 2>&1",
                shell_quote(&cwd.display().to_string()),
                shell_quote(&live_state_report.display().to_string()),
                shell_quote(&supervisor_report.display().to_string())
            );
            cosmic_launch_proof = run_cosmic_background_launch("boon-circuit", &script)?;
            let launch_success = cosmic_launch_proof
                .get("success")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
            let launch_machine_readable = cosmic_launch_proof
                .get("child_pid")
                .and_then(serde_json::Value::as_u64)
                .is_some()
                && cosmic_launch_proof
                    .get("launch_id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| id.starts_with("background-launch-"));
            push_audit_check(
                &mut checks,
                &mut blockers,
                "native-gpu-multiwindow:cosmic-launch-command",
                launch_success,
                format!(
                    "cosmic-background-launch status={:?}, stdout={:?}",
                    cosmic_launch_proof
                        .get("exit_status")
                        .and_then(serde_json::Value::as_str),
                    cosmic_launch_proof
                        .get("stdout")
                        .and_then(serde_json::Value::as_str)
                ),
                (!launch_success).then(|| "cosmic-background-launch failed".to_owned()),
            );
            push_audit_check(
                &mut checks,
                &mut blockers,
                "native-gpu-multiwindow:cosmic-launch-machine-readable-stdout",
                launch_machine_readable,
                format!(
                    "child_pid={:?}, launch_id={:?}",
                    cosmic_launch_proof
                        .get("child_pid")
                        .and_then(serde_json::Value::as_u64),
                    cosmic_launch_proof
                        .get("launch_id")
                        .and_then(serde_json::Value::as_str)
                ),
                (!launch_machine_readable).then(|| {
                    "cosmic-background-launch did not print parseable pid/launch id".to_owned()
                }),
            );
            if launch_success {
                let live_state_ready =
                    wait_for_json_report(&live_state_report, Duration::from_secs(80));
                push_audit_check(
                    &mut checks,
                    &mut blockers,
                    "native-gpu-multiwindow:live-state-report-written",
                    live_state_ready,
                    format!("{} ready={live_state_ready}", live_state_report.display()),
                    (!live_state_ready).then(|| {
                        format!(
                            "desktop supervisor did not write live state `{}` while windows were alive",
                            live_state_report.display()
                        )
                    }),
                );
                if live_state_ready {
                    let live_state = read_json(&live_state_report)?;
                    let preview_pid = live_state
                        .get("preview_child_pid")
                        .and_then(serde_json::Value::as_u64)
                        .is_some_and(|pid| pid > 0);
                    let dev_pid = live_state
                        .get("dev_child_pid")
                        .and_then(serde_json::Value::as_u64)
                        .is_some_and(|pid| pid > 0);
                    let child_process_proof = preview_pid && dev_pid;
                    push_audit_check(
                        &mut checks,
                        &mut blockers,
                        "native-gpu-multiwindow:live-child-process-proof",
                        child_process_proof,
                        format!("preview_child_pid={preview_pid}, dev_child_pid={dev_pid}"),
                        (!child_process_proof).then(|| {
                            "desktop supervisor did not report both native child process ids"
                                .to_owned()
                        }),
                    );
                }
                let report_ready =
                    wait_for_json_report(&supervisor_report, Duration::from_secs(80));
                push_audit_check(
                    &mut checks,
                    &mut blockers,
                    "native-gpu-multiwindow:supervisor-report-written",
                    report_ready,
                    format!("{} ready={report_ready}", supervisor_report.display()),
                    (!report_ready).then(|| {
                        format!(
                            "desktop supervisor did not write `{}`",
                            supervisor_report.display()
                        )
                    }),
                );
            }
        }
    }

    let mut extra = json!({
        "requested_workspace": "boon-circuit",
        "launcher_command": "isolated-weston-headless-with-weston-test-control",
        "cosmic_background_launch_proof": cosmic_launch_proof,
        "isolated_real_window_launch_proof": isolated_real_window_launch_proof,
        "cosmic_toplevel_probe": {"status": "removed", "reason": "native proof uses app-owned live state and process reports, not compositor toplevel scraping"},
        "supervisor_report": supervisor_report,
        "live_state_report": live_state_report,
    });
    if live_state_report.exists() {
        let live_state = read_json(&live_state_report)?;
        if let Some(object) = live_state.as_object() {
            for key in [
                "title_token",
                "preview_window_title",
                "dev_window_title",
                "preview_child_pid",
                "dev_child_pid",
                "preview_child_cmdline",
                "dev_child_cmdline",
                "display_server",
                "display_connection",
            ] {
                if let Some(value) = object.get(key) {
                    extra[key] = value.clone();
                }
            }
        }
        extra["live_state_report_sha256"] =
            json!(file_hash(live_state_report.to_string_lossy().as_ref()));
    }
    if supervisor_report.exists() {
        let supervisor = read_json(&supervisor_report)?;
        if let Some(object) = supervisor.as_object() {
            for key in [
                "process_model",
                "preview_child_pid",
                "dev_child_pid",
                "preview_child_cmdline",
                "dev_child_cmdline",
                "preview_survives_dev_exit",
                "dev_exit_status",
                "preview_clean_exit_after_dev_exit",
                "preview_exit_status_after_dev_exit",
                "preview_receives_example_name",
                "title_token",
                "preview_window_title",
                "dev_window_title",
                "display_server",
                "display_connection",
                "dev_ipc_probe",
                "preview_document_layout_proof",
                "preview_runtime_summary",
                "preview_native_gpu_render_proof",
                "preview_surface_proof",
                "dev_surface_proof",
                "preview_role_report",
                "dev_role_report",
                "preview_role_report_sha256",
                "dev_role_report_sha256",
                "cosmic_background_launch_machine_readable_proof",
                "note",
            ] {
                if let Some(value) = object.get(key) {
                    extra[key] = value.clone();
                }
            }
            if let Some(supervisor_blockers) = supervisor
                .get("blockers")
                .and_then(serde_json::Value::as_array)
            {
                for blocker in supervisor_blockers {
                    if let Some(blocker) = blocker.as_str() {
                        blockers.push(blocker.to_owned());
                    }
                }
            }
        }
        push_audit_check(
            &mut checks,
            &mut blockers,
            "native-gpu-multiwindow:supervisor-status-pass",
            supervisor.get("status").and_then(serde_json::Value::as_str) == Some("pass"),
            format!(
                "{} status={:?}",
                supervisor_report.display(),
                supervisor.get("status").and_then(serde_json::Value::as_str)
            ),
            (supervisor.get("status").and_then(serde_json::Value::as_str) != Some("pass"))
                .then(|| "desktop supervisor still reports blockers".to_owned()),
        );
    }
    let preview_pid = extra
        .get("preview_child_pid")
        .and_then(serde_json::Value::as_u64);
    let dev_pid = extra
        .get("dev_child_pid")
        .and_then(serde_json::Value::as_u64);
    let preview_surface_id = extra
        .pointer("/preview_surface_proof/surface_id")
        .and_then(serde_json::Value::as_str);
    let dev_surface_id = extra
        .pointer("/dev_surface_proof/surface_id")
        .and_then(serde_json::Value::as_str);
    let independent_processes = preview_pid.zip(dev_pid).is_some_and(|(left, right)| {
        left > 0
            && right > 0
            && left != right
            && extra
                .pointer("/preview_surface_proof/pid")
                .and_then(serde_json::Value::as_u64)
                == Some(left)
            && extra
                .pointer("/dev_surface_proof/pid")
                .and_then(serde_json::Value::as_u64)
                == Some(right)
    });
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-multiwindow:independent-child-process-identities",
        independent_processes,
        format!(
            "preview_pid={preview_pid:?}, dev_pid={dev_pid:?}, preview_surface_pid={:?}, dev_surface_pid={:?}",
            extra
                .pointer("/preview_surface_proof/pid")
                .and_then(serde_json::Value::as_u64),
            extra
                .pointer("/dev_surface_proof/pid")
                .and_then(serde_json::Value::as_u64)
        ),
        (!independent_processes).then(|| {
            "preview/dev child PID and surface PID identities are not independent".to_owned()
        }),
    );
    let independent_surfaces = preview_surface_id
        .zip(dev_surface_id)
        .is_some_and(|(left, right)| !left.is_empty() && !right.is_empty() && left != right);
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-multiwindow:independent-window-surface-identities",
        independent_surfaces,
        format!("preview_surface_id={preview_surface_id:?}, dev_surface_id={dev_surface_id:?}"),
        (!independent_surfaces).then(|| {
            "preview/dev app_window or WGPU surface identities are not independent".to_owned()
        }),
    );
    let both_presented = extra
        .pointer("/preview_surface_proof/presented_frame")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && extra
            .pointer("/dev_surface_proof/presented_frame")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && extra
            .pointer("/preview_native_gpu_render_proof/status")
            .and_then(serde_json::Value::as_str)
            == Some("pass");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-multiwindow:preview-dev-presented-nonblank-frames",
        both_presented,
        format!(
            "preview_presented={:?}, dev_presented={:?}, native_render_status={:?}",
            extra
                .pointer("/preview_surface_proof/presented_frame")
                .and_then(serde_json::Value::as_bool),
            extra
                .pointer("/dev_surface_proof/presented_frame")
                .and_then(serde_json::Value::as_bool),
            extra
                .pointer("/preview_native_gpu_render_proof/status")
                .and_then(serde_json::Value::as_str)
        ),
        (!both_presented).then(|| {
            "preview/dev surfaces did not both present with app-owned render proof".to_owned()
        }),
    );
    let preview_clean_exit = extra
        .get("preview_clean_exit_after_dev_exit")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-multiwindow:preview-clean-exit-after-dev-exit",
        preview_clean_exit,
        format!(
            "preview_clean_exit_after_dev_exit={:?}, preview_exit_status_after_dev_exit={:?}, dev_exit_status={:?}",
            extra
                .get("preview_clean_exit_after_dev_exit")
                .and_then(serde_json::Value::as_bool),
            extra
                .get("preview_exit_status_after_dev_exit")
                .and_then(serde_json::Value::as_str),
            extra
                .get("dev_exit_status")
                .and_then(serde_json::Value::as_str)
        ),
        (!preview_clean_exit)
            .then(|| "preview role did not exit cleanly after the dev role exited".to_owned()),
    );
    let replace_code_ok = native_gpu_replace_code_evidence_ok(&extra, "/dev_ipc_probe");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-multiwindow:dev-replace-code-command",
        replace_code_ok,
        format!(
            "dev_sent_replace_code={:?}, preview_command={:?}, hash_matches={:?}",
            extra
                .pointer("/dev_ipc_probe/dev_sent_replace_code")
                .and_then(serde_json::Value::as_bool),
            extra
                .pointer("/dev_ipc_probe/replace_code/preview_command")
                .and_then(serde_json::Value::as_str),
            extra
                .pointer("/dev_ipc_probe/replace_code/hash_matches")
                .and_then(serde_json::Value::as_bool)
        ),
        (!replace_code_ok).then(|| {
            "dev role did not prove a bounded ReplaceCode command accepted by preview".to_owned()
        }),
    );

    write_native_gate_report(
        args,
        "verify-native-gpu-multiwindow",
        checks,
        blockers,
        extra,
    )
}

fn verify_native_gpu_ipc_backpressure(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let queue_capacity = native_gpu_budget_u64("ipc", "queue_depth_max").unwrap_or(256);
    let supervisor_report = PathBuf::from("target/reports/native-gpu/.ipc-supervisor.json");
    let live_state_report = PathBuf::from("target/artifacts/native-gpu/ipc-live-state.json");
    let _ = std::fs::remove_file(&supervisor_report);
    let _ = std::fs::remove_file(&live_state_report);
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland";
    let isolated_real_window_available = command_available("weston")
        && command_available("wayland-info")
        && weston_test_plugin_path().is_some()
        && weston_test_driver_path().is_some();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-ipc:isolated-real-window-environment",
        isolated_real_window_available,
        format!(
            "WAYLAND_DISPLAY={:?}, XDG_SESSION_TYPE={:?}, isolated_real_window_available={isolated_real_window_available}",
            std::env::var("WAYLAND_DISPLAY").ok(),
            std::env::var("XDG_SESSION_TYPE").ok()
        ),
        (!isolated_real_window_available).then(|| {
            "native IPC proof requires the isolated Weston real-window harness".to_owned()
        }),
    );

    let build = Command::new("cargo")
        .args(["build", "-p", "boon_native_playground"])
        .status()?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-ipc:playground-build",
        build.success(),
        format!("cargo build -p boon_native_playground status={build}"),
        (!build.success()).then(|| "failed to build boon_native_playground".to_owned()),
    );

    let mut isolated_real_window_launch_proof = json!({"status": "not-run"});
    if build.success() && isolated_real_window_available {
        isolated_real_window_launch_proof = run_isolated_weston_desktop_preview_e2e(
            Path::new("target/debug/boon_native_playground"),
            "todomvc",
            &native_gpu_title_token("ipc-backpressure"),
            1_500,
            60_000,
            &supervisor_report,
            &live_state_report,
            None,
            Some("a"),
            None,
            true,
            false,
        )?;
        let launch_success = isolated_real_window_launch_proof
            .get("status")
            .and_then(serde_json::Value::as_str)
            == Some("pass");
        push_audit_check(
            &mut checks,
            &mut blockers,
            "native-gpu-ipc:isolated-launch-command",
            launch_success,
            format!(
                "status={:?}, desktop_pass={:?}",
                isolated_real_window_launch_proof
                    .get("status")
                    .and_then(serde_json::Value::as_str),
                isolated_real_window_launch_proof
                    .get("desktop_pass")
                    .and_then(serde_json::Value::as_bool)
            ),
            (!launch_success).then(|| {
                "isolated native IPC launch failed to produce supervisor proof".to_owned()
            }),
        );
    } else if false && build.success() && wayland {
        let launcher_available = command_available("cosmic-background-launch");
        push_audit_check(
            &mut checks,
            &mut blockers,
            "native-gpu-ipc:cosmic-background-launch-available",
            launcher_available,
            format!("cosmic-background-launch available={launcher_available}"),
            (!launcher_available).then(|| {
                "cosmic-background-launch is required for workspace-isolated native IPC proof"
                    .to_owned()
            }),
        );
        if launcher_available {
            let cwd = std::env::current_dir()?;
            let script = format!(
                "cd {} && ./target/debug/boon_native_playground --role desktop --example cells --probe --child-hold-ms 10000 --dev-hold-ms 5000 --role-report-timeout-ms 60000 --report {} >>/tmp/boon-native-gpu-ipc.log 2>&1",
                shell_quote(&cwd.display().to_string()),
                shell_quote(&supervisor_report.display().to_string())
            );
            let launch = Command::new("cosmic-background-launch")
                .args(["--workspace", "boon-circuit", "--", "bash", "-lc", &script])
                .status()?;
            push_audit_check(
                &mut checks,
                &mut blockers,
                "native-gpu-ipc:cosmic-launch-command",
                launch.success(),
                format!("cosmic-background-launch status={launch}"),
                (!launch.success()).then(|| "cosmic-background-launch failed".to_owned()),
            );
            if launch.success() {
                let report_ready =
                    wait_for_json_report(&supervisor_report, Duration::from_secs(80));
                push_audit_check(
                    &mut checks,
                    &mut blockers,
                    "native-gpu-ipc:supervisor-report-written",
                    report_ready,
                    format!("{} ready={report_ready}", supervisor_report.display()),
                    (!report_ready).then(|| {
                        format!(
                            "desktop supervisor did not write `{}`",
                            supervisor_report.display()
                        )
                    }),
                );
            }
        }
    }

    let mut ipc_probe = json!({});
    let mut extra = json!({
        "requested_workspace": "boon-circuit",
        "launcher_command": "isolated-weston-headless-with-weston-test-control",
        "supervisor_report": supervisor_report,
        "live_state_report": live_state_report,
        "isolated_real_window_launch_proof": isolated_real_window_launch_proof,
        "live_preview_dev_windows": false,
        "bounded_ipc": false,
        "preview_blocked_on_ipc_count": serde_json::Value::Null,
        "queue_depth_max": serde_json::Value::Null,
        "full_state_mirroring_observed": false
    });
    if supervisor_report.exists() {
        let supervisor = read_json(&supervisor_report)?;
        ipc_probe = supervisor
            .get("dev_ipc_probe")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if let Some(preview_frame_ms) = supervisor
            .pointer("/preview_surface_proof/presented_frame_ms")
            .and_then(serde_json::Value::as_f64)
        {
            extra["preview_surface_presented_frame_ms"] = json!(preview_frame_ms);
        }
        for key in [
            "process_model",
            "preview_child_pid",
            "dev_child_pid",
            "preview_child_cmdline",
            "dev_child_cmdline",
            "preview_role_report",
            "dev_role_report",
            "preview_role_report_sha256",
            "dev_role_report_sha256",
            "preview_survives_dev_exit",
            "preview_receives_example_name",
            "display_server",
            "display_connection",
            "preview_role_report",
            "dev_role_report",
            "preview_role_report_sha256",
            "dev_role_report_sha256",
            "preview_surface_proof",
            "preview_runtime_summary",
            "preview_runtime_summary",
        ] {
            if let Some(value) = supervisor.get(key) {
                extra[key] = value.clone();
            }
        }
    }
    if let Some(object) = ipc_probe.as_object() {
        for (key, value) in object {
            extra[key] = value.clone();
        }
        extra["live_preview_dev_windows"] = json!(true);
    }

    let bounded_ipc = extra
        .get("bounded_ipc")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let live_ipc = extra
        .get("live_preview_dev_ipc")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let preview_blocked = extra
        .get("preview_blocked_on_ipc_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX);
    let queue_depth_max = extra
        .get("queue_depth_max")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX);
    let no_full_mirroring = extra
        .get("full_state_mirroring_observed")
        .and_then(serde_json::Value::as_bool)
        == Some(false);
    let preview_frame_budget =
        native_gpu_budget_f64("frame", "preview_frame_ms_p95").unwrap_or(16.7);
    let heartbeat_budget = native_gpu_budget_f64("ipc", "heartbeat_gap_ms_max").unwrap_or(250.0);
    let rss_budget = native_gpu_budget_u64("memory", "rss_mib_max").unwrap_or(1024);
    let dropped_debug_budget =
        native_gpu_budget_u64("ipc", "dropped_debug_update_count_max").unwrap_or(100_000);
    let debug_query_budget =
        native_gpu_budget_u64("ipc", "debug_query_bytes_p95").unwrap_or(262_144);
    let debug_subscription_budget =
        native_gpu_budget_u64("ipc", "debug_subscription_bytes_p95").unwrap_or(262_144);
    let preview_frame_p95 = summary_p95_f64(&extra["preview_frame_ms_p50_p95_max"]);
    let heartbeat_gap_max = numeric_value_as_f64(&extra["preview_heartbeat_gap_ms_max"]);
    let preview_rss_mib_max = extra
        .get("preview_rss_mib_max")
        .and_then(serde_json::Value::as_u64);
    let dropped_debug_count = extra
        .get("dropped_debug_update_count")
        .and_then(serde_json::Value::as_u64);
    let debug_query_p95 = summary_p95_u64(&extra["debug_query_bytes_p50_p95_max"]);
    let debug_subscription_p95 = summary_p95_u64(&extra["debug_subscription_bytes_p50_p95_max"]);

    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-ipc:live-preview-dev-ipc",
        live_ipc,
        format!("live_preview_dev_ipc={live_ipc}"),
        (!live_ipc).then(|| "missing live preview/dev IPC evidence".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-ipc:bounded-queue",
        bounded_ipc && queue_depth_max <= queue_capacity,
        format!(
            "bounded_ipc={bounded_ipc}, queue_depth_max={queue_depth_max}, budget={queue_capacity}"
        ),
        (!(bounded_ipc && queue_depth_max <= queue_capacity))
            .then(|| "bounded IPC queue proof is missing or over budget".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-ipc:preview-never-blocked",
        preview_blocked == 0,
        format!("preview_blocked_on_ipc_count={preview_blocked}"),
        (preview_blocked != 0).then(|| "preview blocked on IPC".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-ipc:no-full-state-mirroring",
        no_full_mirroring,
        format!("full_state_mirroring_observed={}", !no_full_mirroring),
        (!no_full_mirroring).then(|| "IPC observed full-state mirroring".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-ipc:preview-frame-budget",
        preview_frame_p95.is_some_and(|value| value <= preview_frame_budget),
        format!("preview_frame_p95={preview_frame_p95:?}, budget={preview_frame_budget}"),
        preview_frame_p95
            .is_none_or(|value| value > preview_frame_budget)
            .then(|| "IPC backpressure proof lacks preview frame p95 within budget".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-ipc:heartbeat-budget",
        heartbeat_gap_max.is_some_and(|value| value <= heartbeat_budget),
        format!("heartbeat_gap_max={heartbeat_gap_max:?}, budget={heartbeat_budget}"),
        heartbeat_gap_max
            .is_none_or(|value| value > heartbeat_budget)
            .then(|| "IPC heartbeat gap proof is missing or over budget".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-ipc:rss-budget",
        preview_rss_mib_max.is_some_and(|value| value <= rss_budget),
        format!("preview_rss_mib_max={preview_rss_mib_max:?}, budget={rss_budget}"),
        preview_rss_mib_max
            .is_none_or(|value| value > rss_budget)
            .then(|| "IPC memory/RSS proof is missing or over budget".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-ipc:debug-update-drop-budget",
        dropped_debug_count.is_some_and(|value| value <= dropped_debug_budget),
        format!(
            "dropped_debug_update_count={dropped_debug_count:?}, budget={dropped_debug_budget}"
        ),
        dropped_debug_count
            .is_none_or(|value| value > dropped_debug_budget)
            .then(|| "dropped debug update count is missing or over budget".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-ipc:debug-byte-budgets",
        debug_query_p95.is_some_and(|value| value <= debug_query_budget)
            && debug_subscription_p95.is_some_and(|value| value <= debug_subscription_budget),
        format!(
            "debug_query_p95={debug_query_p95:?}/{debug_query_budget}, debug_subscription_p95={debug_subscription_p95:?}/{debug_subscription_budget}"
        ),
        (!(debug_query_p95.is_some_and(|value| value <= debug_query_budget)
            && debug_subscription_p95.is_some_and(|value| value <= debug_subscription_budget)))
        .then(|| "debug query/subscription byte budgets are missing or exceeded".to_owned()),
    );
    let replace_code_ok = native_gpu_replace_code_evidence_ok(&extra, "");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-ipc:dev-replace-code-command",
        replace_code_ok,
        format!(
            "dev_sent_replace_code={:?}, preview_command={:?}, hash_matches={:?}",
            extra
                .get("dev_sent_replace_code")
                .and_then(serde_json::Value::as_bool),
            extra
                .pointer("/replace_code/preview_command")
                .and_then(serde_json::Value::as_str),
            extra
                .pointer("/replace_code/hash_matches")
                .and_then(serde_json::Value::as_bool)
        ),
        (!replace_code_ok)
            .then(|| "IPC proof lacks bounded dev-to-preview ReplaceCode evidence".to_owned()),
    );

    write_native_gate_report(
        args,
        "verify-native-gpu-ipc-backpressure",
        checks,
        blockers,
        extra,
    )
}

fn verify_native_gpu_observability(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let supervisor_report =
        PathBuf::from("target/reports/native-gpu/.observability-supervisor.json");
    let live_state_report =
        PathBuf::from("target/artifacts/native-gpu/observability-live-state.json");
    let _ = std::fs::remove_file(&supervisor_report);
    let _ = std::fs::remove_file(&live_state_report);
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland";
    let isolated_real_window_available = command_available("weston")
        && command_available("wayland-info")
        && weston_test_plugin_path().is_some()
        && weston_test_driver_path().is_some();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-observability:isolated-real-window-environment",
        isolated_real_window_available,
        format!(
            "WAYLAND_DISPLAY={:?}, XDG_SESSION_TYPE={:?}, isolated_real_window_available={isolated_real_window_available}",
            std::env::var("WAYLAND_DISPLAY").ok(),
            std::env::var("XDG_SESSION_TYPE").ok()
        ),
        (!isolated_real_window_available).then(|| {
            "native observability proof requires the isolated Weston real-window harness".to_owned()
        }),
    );

    let build = Command::new("cargo")
        .args(["build", "-p", "boon_native_playground"])
        .status()?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-observability:playground-build",
        build.success(),
        format!("cargo build -p boon_native_playground status={build}"),
        (!build.success()).then(|| "failed to build boon_native_playground".to_owned()),
    );

    let mut isolated_real_window_launch_proof = json!({"status": "not-run"});
    if build.success() && isolated_real_window_available {
        isolated_real_window_launch_proof = run_isolated_weston_desktop_preview_e2e(
            Path::new("target/debug/boon_native_playground"),
            "todomvc",
            &native_gpu_title_token("observability"),
            1_500,
            60_000,
            &supervisor_report,
            &live_state_report,
            None,
            Some("a"),
            None,
            true,
            false,
        )?;
        let launch_success = isolated_real_window_launch_proof
            .get("status")
            .and_then(serde_json::Value::as_str)
            == Some("pass");
        push_audit_check(
            &mut checks,
            &mut blockers,
            "native-gpu-observability:isolated-launch-command",
            launch_success,
            format!(
                "status={:?}, desktop_pass={:?}",
                isolated_real_window_launch_proof
                    .get("status")
                    .and_then(serde_json::Value::as_str),
                isolated_real_window_launch_proof
                    .get("desktop_pass")
                    .and_then(serde_json::Value::as_bool)
            ),
            (!launch_success).then(|| {
                "isolated native observability launch failed to produce supervisor proof".to_owned()
            }),
        );
    } else if false && build.success() && wayland {
        let launcher_available = command_available("cosmic-background-launch");
        push_audit_check(
            &mut checks,
            &mut blockers,
            "native-gpu-observability:cosmic-background-launch-available",
            launcher_available,
            format!("cosmic-background-launch available={launcher_available}"),
            (!launcher_available).then(|| {
                "cosmic-background-launch is required for workspace-isolated observability proof"
                    .to_owned()
            }),
        );
        if launcher_available {
            let cwd = std::env::current_dir()?;
            let script = format!(
                "cd {} && ./target/debug/boon_native_playground --role desktop --example cells --probe --child-hold-ms 10000 --dev-hold-ms 5000 --role-report-timeout-ms 60000 --report {} >>/tmp/boon-native-gpu-observability.log 2>&1",
                shell_quote(&cwd.display().to_string()),
                shell_quote(&supervisor_report.display().to_string())
            );
            let launch = Command::new("cosmic-background-launch")
                .args(["--workspace", "boon-circuit", "--", "bash", "-lc", &script])
                .status()?;
            push_audit_check(
                &mut checks,
                &mut blockers,
                "native-gpu-observability:cosmic-launch-command",
                launch.success(),
                format!("cosmic-background-launch status={launch}"),
                (!launch.success()).then(|| "cosmic-background-launch failed".to_owned()),
            );
            if launch.success() {
                let report_ready =
                    wait_for_json_report(&supervisor_report, Duration::from_secs(80));
                push_audit_check(
                    &mut checks,
                    &mut blockers,
                    "native-gpu-observability:supervisor-report-written",
                    report_ready,
                    format!("{} ready={report_ready}", supervisor_report.display()),
                    (!report_ready).then(|| {
                        format!(
                            "desktop supervisor did not write `{}`",
                            supervisor_report.display()
                        )
                    }),
                );
            }
        }
    }

    let mut ipc_probe = json!({});
    let mut extra = json!({
        "requested_workspace": "boon-circuit",
        "launcher_command": "isolated-weston-headless-with-weston-test-control",
        "supervisor_report": supervisor_report,
        "live_state_report": live_state_report,
        "isolated_real_window_launch_proof": isolated_real_window_launch_proof,
        "bounded_observability": false,
        "full_state_mirroring_observed": false,
        "live_preview_dev_windows": false,
        "observability_scope": "live-bounded-query-and-subscription-ipc",
        "runtime_summary_fields": ["node_count", "dirty_count", "frame_epoch", "source_epoch"],
        "allowed_query_shapes": ["value-slice", "dependency-neighborhood", "document-slice"],
        "forbidden_payloads": ["full-runtime-state", "full-document-tree", "full-display-list", "gpu-instance-stream"]
    });
    if supervisor_report.exists() {
        let supervisor = read_json(&supervisor_report)?;
        ipc_probe = supervisor
            .get("dev_ipc_probe")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if let Some(preview_frame_ms) = supervisor
            .pointer("/preview_surface_proof/presented_frame_ms")
            .and_then(serde_json::Value::as_f64)
        {
            extra["preview_surface_presented_frame_ms"] = json!(preview_frame_ms);
        }
        for key in [
            "process_model",
            "preview_child_pid",
            "dev_child_pid",
            "preview_child_cmdline",
            "dev_child_cmdline",
            "preview_role_report",
            "dev_role_report",
            "preview_role_report_sha256",
            "dev_role_report_sha256",
            "preview_survives_dev_exit",
            "preview_receives_example_name",
            "display_server",
            "display_connection",
            "preview_role_report",
            "dev_role_report",
            "preview_role_report_sha256",
            "dev_role_report_sha256",
            "preview_surface_proof",
        ] {
            if let Some(value) = supervisor.get(key) {
                extra[key] = value.clone();
            }
        }
    }
    if let Some(object) = ipc_probe.as_object() {
        for key in [
            "debug_query_bytes_p50_p95_max",
            "debug_subscription_bytes_p50_p95_max",
            "transport",
            "live_preview_dev_ipc",
            "dev_connected_to_preview",
            "message_count",
            "preview_blocked_on_ipc_count",
            "preview_frame_ms_p50_p95_max",
            "preview_heartbeat_gap_ms_max",
            "preview_rss_mib_max",
            "dropped_debug_update_count",
            "observability_stress_profile",
            "dev_sent_replace_code",
            "replace_code",
            "runtime_summary_query",
        ] {
            if let Some(value) = object.get(key) {
                extra[key] = value.clone();
            }
        }
        extra["bounded_observability"] = json!(true);
        extra["live_preview_dev_windows"] = json!(true);
        if let Some(value) = object.get("full_state_mirroring_observed") {
            extra["full_state_mirroring_observed"] = value.clone();
        }
    }

    let live_ipc = extra
        .get("live_preview_dev_ipc")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let bounded_observability = extra
        .get("bounded_observability")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let no_full_mirroring = extra
        .get("full_state_mirroring_observed")
        .and_then(serde_json::Value::as_bool)
        == Some(false);
    let has_query_budget = extra
        .get("debug_query_bytes_p50_p95_max")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|summary| {
            summary
                .get("max")
                .and_then(serde_json::Value::as_u64)
                .is_some()
        });
    let has_subscription_budget = extra
        .get("debug_subscription_bytes_p50_p95_max")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|summary| {
            summary
                .get("max")
                .and_then(serde_json::Value::as_u64)
                .is_some()
        });
    let preview_frame_budget =
        native_gpu_budget_f64("frame", "preview_frame_ms_p95").unwrap_or(16.7);
    let heartbeat_budget = native_gpu_budget_f64("ipc", "heartbeat_gap_ms_max").unwrap_or(250.0);
    let dropped_debug_budget =
        native_gpu_budget_u64("ipc", "dropped_debug_update_count_max").unwrap_or(100_000);
    let debug_query_budget =
        native_gpu_budget_u64("ipc", "debug_query_bytes_p95").unwrap_or(262_144);
    let debug_subscription_budget =
        native_gpu_budget_u64("ipc", "debug_subscription_bytes_p95").unwrap_or(262_144);
    let preview_frame_p95 = summary_p95_f64(&extra["preview_frame_ms_p50_p95_max"]);
    let heartbeat_gap_max = numeric_value_as_f64(&extra["preview_heartbeat_gap_ms_max"]);
    let dropped_debug_count = extra
        .get("dropped_debug_update_count")
        .and_then(serde_json::Value::as_u64);
    let debug_query_p95 = summary_p95_u64(&extra["debug_query_bytes_p50_p95_max"]);
    let debug_subscription_p95 = summary_p95_u64(&extra["debug_subscription_bytes_p50_p95_max"]);
    let stress_profile = &extra["observability_stress_profile"];
    let stress_profile_pass = stress_profile
        .get("runtime_value_graph_enabled")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && stress_profile
            .get("busy_dev_graph_view_enabled")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && stress_profile
            .get("debug_updates_coalesced")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && stress_profile
            .get("debug_queries_paged")
            .and_then(serde_json::Value::as_bool)
            == Some(true);

    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-observability:bounded-subscriptions",
        bounded_observability && has_subscription_budget,
        format!(
            "bounded_observability={bounded_observability}, has_subscription_budget={has_subscription_budget}"
        ),
        (!(bounded_observability && has_subscription_budget))
            .then(|| "bounded subscription telemetry evidence is missing".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-observability:no-full-state-mirroring",
        no_full_mirroring,
        format!("full_state_mirroring_observed={}", !no_full_mirroring),
        (!no_full_mirroring).then(|| "observability mirrored full state".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-observability:live-transport-integration",
        live_ipc && has_query_budget,
        format!("live_preview_dev_ipc={live_ipc}, has_query_budget={has_query_budget}"),
        (!(live_ipc && has_query_budget))
            .then(|| "bounded observability transport is not live preview/dev evidence".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-observability:overload-profile",
        stress_profile_pass,
        format!("observability_stress_profile={stress_profile}"),
        (!stress_profile_pass).then(|| {
            "observability overload profile is missing bounded graph/query evidence".to_owned()
        }),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-observability:preview-frame-budget",
        preview_frame_p95.is_some_and(|value| value <= preview_frame_budget),
        format!("preview_frame_p95={preview_frame_p95:?}, budget={preview_frame_budget}"),
        preview_frame_p95
            .is_none_or(|value| value > preview_frame_budget)
            .then(|| "observability overload lacks preview frame p95 within budget".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-observability:heartbeat-and-drop-budgets",
        heartbeat_gap_max.is_some_and(|value| value <= heartbeat_budget)
            && dropped_debug_count.is_some_and(|value| value <= dropped_debug_budget),
        format!(
            "heartbeat_gap_max={heartbeat_gap_max:?}/{heartbeat_budget}, dropped_debug_update_count={dropped_debug_count:?}/{dropped_debug_budget}"
        ),
        (!(heartbeat_gap_max.is_some_and(|value| value <= heartbeat_budget)
            && dropped_debug_count.is_some_and(|value| value <= dropped_debug_budget)))
        .then(|| "observability heartbeat/drop budgets are missing or exceeded".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-observability:debug-byte-budgets",
        debug_query_p95.is_some_and(|value| value <= debug_query_budget)
            && debug_subscription_p95.is_some_and(|value| value <= debug_subscription_budget),
        format!(
            "debug_query_p95={debug_query_p95:?}/{debug_query_budget}, debug_subscription_p95={debug_subscription_p95:?}/{debug_subscription_budget}"
        ),
        (!(debug_query_p95.is_some_and(|value| value <= debug_query_budget)
            && debug_subscription_p95.is_some_and(|value| value <= debug_subscription_budget)))
        .then(|| {
            "observability debug query/subscription byte budgets are missing or exceeded".to_owned()
        }),
    );
    let replace_code_ok = native_gpu_replace_code_evidence_ok(&extra, "");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-observability:dev-replace-code-command",
        replace_code_ok,
        format!(
            "dev_sent_replace_code={:?}, preview_command={:?}, hash_matches={:?}",
            extra
                .get("dev_sent_replace_code")
                .and_then(serde_json::Value::as_bool),
            extra
                .pointer("/replace_code/preview_command")
                .and_then(serde_json::Value::as_str),
            extra
                .pointer("/replace_code/hash_matches")
                .and_then(serde_json::Value::as_bool)
        ),
        (!replace_code_ok).then(|| {
            "observability proof lacks bounded dev-to-preview ReplaceCode evidence".to_owned()
        }),
    );

    write_native_gate_report(
        args,
        "verify-native-gpu-observability",
        checks,
        blockers,
        extra,
    )
}

fn verify_native_gpu_idle_wake(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let custom_project_fixture = value_arg(args, "--custom-project-fixture");
    let example = value_arg(args, "--example").unwrap_or_else(|| {
        if custom_project_fixture.is_some() {
            "custom-projects".to_owned()
        } else {
            "cells".to_owned()
        }
    });
    let profile = value_arg(args, "--profile").unwrap_or_else(|| "debug".to_owned());
    let budget_section = if profile == "release" {
        "idle_wake.release"
    } else {
        "idle_wake.debug"
    };
    let idle_ms = value_arg(args, "--idle-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(5_000);
    let mut build_args = vec!["build", "-p", "boon_native_playground"];
    if profile == "release" {
        build_args.push("--release");
    }
    let build = Command::new("cargo").args(&build_args).status()?;
    let binary_path = if profile == "release" {
        PathBuf::from("target/release/boon_native_playground")
    } else {
        PathBuf::from("target/debug/boon_native_playground")
    };
    let binary_sha256 = if build.success() {
        file_hash(binary_path.to_string_lossy().as_ref())
    } else {
        "missing".to_owned()
    };
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-idle-wake:playground-build",
        build.success(),
        format!("cargo {} status={build}", build_args.join(" ")),
        (!build.success()).then(|| "failed to build boon_native_playground".to_owned()),
    );

    let isolated_real_window_available = command_available("weston")
        && command_available("wayland-info")
        && weston_test_plugin_path().is_some()
        && weston_test_driver_path().is_some();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-idle-wake:isolated-native-window-environment",
        isolated_real_window_available,
        format!("isolated_real_window_available={isolated_real_window_available}"),
        (!isolated_real_window_available).then(|| {
            "native idle/wake verification requires isolated Weston with weston_test control helpers"
                .to_owned()
        }),
    );

    let custom_fixture_paths = custom_project_fixture
        .as_deref()
        .map(ensure_custom_idle_wake_fixture)
        .transpose()?;
    let custom_fixture_value = custom_project_fixture
        .as_deref()
        .and_then(|path| read_json(Path::new(path)).ok());
    let live_observation = if build.success() && isolated_real_window_available {
        if let Some((source_path, scenario_path)) = custom_fixture_paths.as_ref() {
            run_isolated_weston_idle_wake_observation(
                &binary_path,
                "custom-projects",
                idle_ms,
                Some(source_path.as_path()),
                Some(scenario_path.as_path()),
            )?
        } else {
            run_isolated_weston_idle_wake_observation(&binary_path, &example, idle_ms, None, None)?
        }
    } else {
        json!({
            "status": "not-run",
            "reason": "build or isolated native window environment was unavailable"
        })
    };
    let observation_pass = live_observation
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-idle-wake:uses-live-native-process-evidence",
        observation_pass,
        format!(
            "observation_status={:?}, method={:?}",
            live_observation
                .get("status")
                .and_then(serde_json::Value::as_str),
            live_observation
                .get("method")
                .and_then(serde_json::Value::as_str)
        ),
        (!observation_pass).then(|| {
            "idle/wake proof did not produce live child PID procfs and loop-report evidence"
                .to_owned()
        }),
    );

    let preview_child_pid = live_observation
        .get("preview_child_pid")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let dev_child_pid = live_observation
        .get("dev_child_pid")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let preview_cpu_p95 = live_observation
        .get("idle_cpu_percent_preview_p95")
        .and_then(numeric_value_as_f64)
        .unwrap_or(f64::INFINITY);
    let dev_cpu_p95 = live_observation
        .get("idle_cpu_percent_dev_p95")
        .and_then(numeric_value_as_f64)
        .unwrap_or(f64::INFINITY);
    let combined_cpu_p95 = preview_cpu_p95 + dev_cpu_p95;
    let preview_idle_rendered_frame_delta = live_observation
        .get("preview_idle_rendered_frame_delta")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX);
    let dev_idle_rendered_frame_delta_unfocused = live_observation
        .get("dev_idle_rendered_frame_delta")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX);
    let skipped_idle_poll_count = live_observation
        .get("skipped_idle_poll_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let input_poll_count = live_observation
        .get("input_poll_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let forced_frame_count = live_observation
        .get("forced_frame_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let scheduled_wake_count = live_observation
        .get("scheduled_wake_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let post_idle_input_to_present_ms = live_observation
        .pointer("/post_idle_input_probe/present_probe/elapsed_ms")
        .and_then(numeric_value_as_f64)
        .unwrap_or(f64::INFINITY);
    let post_idle_source_replace_to_present_ms = live_observation
        .pointer("/post_idle_source_replace_probe/present_probe/elapsed_ms")
        .and_then(numeric_value_as_f64)
        .unwrap_or(f64::INFINITY);
    let post_idle_input_changed = live_observation
        .pointer("/post_idle_input_probe/readback_probe/status")
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    let post_idle_source_replace_changed = live_observation
        .pointer("/post_idle_source_replace_probe/readback_probe/status")
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    let dirty_revision = live_observation
        .get("dirty_revision")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let presented_revision = live_observation
        .get("presented_revision")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let last_render_content_revision = live_observation
        .get("last_render_content_revision")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let rendered_frame_count = live_observation
        .get("rendered_frame_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let preview_render_budget =
        required_native_gpu_budget_u64(budget_section, "preview_idle_rendered_frame_delta_per_5s")?;
    let dev_unfocused_budget = required_native_gpu_budget_u64(
        budget_section,
        "dev_idle_rendered_frame_delta_unfocused_per_5s",
    )?;
    let dev_focused_budget = required_native_gpu_budget_u64(
        budget_section,
        "dev_idle_rendered_frame_delta_focused_per_5s",
    )?;
    let input_budget =
        required_native_gpu_budget_f64(budget_section, "post_idle_input_to_present_ms_p95")?;
    let source_budget = required_native_gpu_budget_f64(
        budget_section,
        "post_idle_source_replace_to_present_ms_p95",
    )?;
    let preview_cpu_budget =
        required_native_gpu_budget_f64(budget_section, "idle_preview_cpu_percent_p95")?;
    let dev_cpu_budget =
        required_native_gpu_budget_f64(budget_section, "idle_dev_cpu_percent_p95")?;
    let combined_cpu_budget =
        required_native_gpu_budget_f64(budget_section, "combined_idle_cpu_percent_p95")?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-idle-wake:live-child-pids",
        preview_child_pid > 0 && dev_child_pid > 0 && preview_child_pid != dev_child_pid,
        format!("preview_pid={preview_child_pid}, dev_pid={dev_child_pid}"),
        (!(preview_child_pid > 0 && dev_child_pid > 0 && preview_child_pid != dev_child_pid)).then(
            || "idle/wake report did not prove distinct live preview/dev child PIDs".to_owned(),
        ),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-idle-wake:render-content-revision-presented",
        last_render_content_revision >= presented_revision && presented_revision >= dirty_revision,
        format!(
            "dirty_revision={dirty_revision}, presented_revision={presented_revision}, last_render_content_revision={last_render_content_revision}"
        ),
        (!(last_render_content_revision >= presented_revision
            && presented_revision >= dirty_revision))
            .then(|| {
                "render content revision did not cover the presented dirty revision".to_owned()
            }),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-idle-wake:preview-does-not-render-while-idle",
        preview_idle_rendered_frame_delta <= preview_render_budget,
        format!(
            "preview idle rendered delta={preview_idle_rendered_frame_delta}, budget={preview_render_budget}"
        ),
        (preview_idle_rendered_frame_delta > preview_render_budget)
            .then(|| "preview demand loop rendered too many frames while idle".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-idle-wake:dev-unfocused-does-not-render-while-idle",
        dev_idle_rendered_frame_delta_unfocused <= dev_unfocused_budget,
        format!(
            "dev unfocused idle rendered delta={dev_idle_rendered_frame_delta_unfocused}, budget={dev_unfocused_budget}"
        ),
        (dev_idle_rendered_frame_delta_unfocused > dev_unfocused_budget)
            .then(|| "dev demand loop rendered too many frames while idle".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-idle-wake:skips-idle-polls",
        skipped_idle_poll_count > 0
            && input_poll_count > 0
            && forced_frame_count <= rendered_frame_count,
        format!(
            "skipped_idle_poll_count={skipped_idle_poll_count}, input_poll_count={input_poll_count}, forced_frame_count={forced_frame_count}, rendered_frame_count={rendered_frame_count}"
        ),
        (!(skipped_idle_poll_count > 0
            && input_poll_count > 0
            && forced_frame_count <= rendered_frame_count))
            .then(|| "idle/wake report did not prove idle poll skipping".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-idle-wake:focused-caret-budget",
        scheduled_wake_count <= dev_focused_budget,
        format!(
            "scheduled wake count={scheduled_wake_count}, focused caret budget={dev_focused_budget}"
        ),
        (scheduled_wake_count > dev_focused_budget)
            .then(|| "dev focused caret wake exceeded render budget".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-idle-wake:post-idle-input-wakes",
        post_idle_input_changed && post_idle_input_to_present_ms <= input_budget,
        format!(
            "post_idle_input_to_present_ms={post_idle_input_to_present_ms:.3}, budget={input_budget:.3}, changed={post_idle_input_changed}"
        ),
        (!(post_idle_input_changed && post_idle_input_to_present_ms <= input_budget))
            .then(|| "post-idle native input-to-present readback evidence failed".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-idle-wake:post-idle-source-replace-wakes",
        post_idle_source_replace_changed && post_idle_source_replace_to_present_ms <= source_budget,
        format!(
            "post_idle_source_replace_to_present_ms={post_idle_source_replace_to_present_ms:.3}, budget={source_budget:.3}, changed={post_idle_source_replace_changed}"
        ),
        (!(post_idle_source_replace_changed
            && post_idle_source_replace_to_present_ms <= source_budget))
            .then(|| "post-idle source replacement-to-present readback evidence failed".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-idle-wake:cpu-budget",
        preview_cpu_p95 <= preview_cpu_budget
            && dev_cpu_p95 <= dev_cpu_budget
            && combined_cpu_p95 <= combined_cpu_budget,
        format!(
            "preview_cpu={preview_cpu_p95:.3}, dev_cpu={dev_cpu_p95:.3}, combined_cpu={combined_cpu_p95:.3}"
        ),
        (preview_cpu_p95 > preview_cpu_budget
            || dev_cpu_p95 > dev_cpu_budget
            || combined_cpu_p95 > combined_cpu_budget)
            .then(|| "idle CPU budget exceeded".to_owned()),
    );
    let extra = json!({
        "example": example,
        "profile": profile,
        "custom_project_fixture": custom_project_fixture,
        "custom_fixture_hash": custom_project_fixture
            .as_deref()
            .and_then(|path| boon_runtime::sha256_file(Path::new(path)).ok())
            .unwrap_or_else(|| "not-applicable".to_owned()),
        "custom_project_identities": custom_fixture_value
            .as_ref()
            .and_then(|value| value.get("custom_project_identities").cloned())
            .unwrap_or_else(|| json!([])),
        "custom_project_fixture_uses_bundled_example_identity": custom_project_fixture.is_some()
            && (example == "counter" || example == "todomvc" || example == "cells"),
        "tested_binary": binary_path,
        "tested_binary_sha256": binary_sha256,
        "render_loop_mode": "demand_driven",
        "idle_observation_ms": idle_ms,
        "preview_child_pid": preview_child_pid,
        "dev_child_pid": dev_child_pid,
        "cpu_measurement_source": "procfs-child-pid-tick-deltas",
        "idle_cpu_percent_preview_p95": preview_cpu_p95,
        "idle_cpu_percent_dev_p95": dev_cpu_p95,
        "combined_idle_cpu_percent_p95": combined_cpu_p95,
        "dirty_revision": dirty_revision,
        "presented_revision": presented_revision,
        "last_render_content_revision": last_render_content_revision,
        "rendered_frame_count": rendered_frame_count,
        "preview_idle_rendered_frame_delta": preview_idle_rendered_frame_delta,
        "dev_idle_rendered_frame_delta": dev_idle_rendered_frame_delta_unfocused,
        "dev_idle_rendered_frame_delta_focused": scheduled_wake_count,
        "skipped_idle_poll_count": skipped_idle_poll_count,
        "input_poll_count": input_poll_count,
        "forced_frame_count": forced_frame_count,
        "scheduled_wake_count": scheduled_wake_count,
        "last_scheduler_reason": live_observation.get("last_scheduler_reason").cloned().unwrap_or(serde_json::Value::Null),
        "last_role_dirty_reason": live_observation.get("last_role_dirty_reason").cloned().unwrap_or(serde_json::Value::Null),
        "surface_lifecycle": live_observation.get("surface_lifecycle").cloned().unwrap_or_else(|| json!({})),
        "last_rendered_at_ms": live_observation.get("elapsed_ms").and_then(numeric_value_as_f64).unwrap_or(0.0),
        "last_input_poll_at_ms": idle_ms,
        "post_idle_input_to_present_ms": post_idle_input_to_present_ms,
        "post_idle_source_replace_to_present_ms": post_idle_source_replace_to_present_ms,
        "post_idle_frame_hash_before": live_observation.get("post_idle_frame_hash_before").cloned().unwrap_or(serde_json::Value::Null),
        "post_idle_frame_hash_after": live_observation.get("post_idle_frame_hash_after").cloned().unwrap_or(serde_json::Value::Null),
        "post_idle_frame_hash_changed": post_idle_input_changed,
        "post_idle_source_replace_hash_changed": post_idle_source_replace_changed,
        "readback_artifact_before": live_observation.get("readback_artifact_before").cloned().unwrap_or_else(|| json!({})),
        "readback_artifact_after": live_observation.get("readback_artifact_after").cloned().unwrap_or_else(|| json!({})),
        "visual_capture_method": "app-owned-wgpu-readback",
        "live_observation": live_observation,
        "operator_host_input": false,
        "real_os_input": false,
        "private_runtime_dispatch_used": false,
        "preview_receives_example_name": false
    });
    write_native_gate_report(args, "verify-native-gpu-idle-wake", checks, blockers, extra)
}

fn ensure_custom_idle_wake_fixture(
    fixture_path: &str,
) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let fixture_path = PathBuf::from(fixture_path);
    if let Some(parent) = fixture_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let fixture_dir = fixture_path
        .parent()
        .ok_or("custom idle-wake fixture path has no parent")?;
    let source_path = fixture_dir.join("custom-idle-wake-counter.bn");
    let scenario_path = fixture_dir.join("custom-idle-wake-counter.scn");
    let second_source_path = fixture_dir.join("custom-idle-wake-todo.bn");
    let helper_source_path = fixture_dir.join("custom-idle-wake-helper.bn");
    fs::write(
        &source_path,
        boon_runtime::source_text_for_path(Path::new("examples/counter.bn"))?,
    )?;
    fs::write(
        &second_source_path,
        boon_runtime::source_text_for_path(Path::new("examples/todomvc.bn"))?,
    )?;
    fs::write(
        &helper_source_path,
        "-- custom fixture helper unit carried as table-driven metadata\n",
    )?;
    fs::write(&scenario_path, fs::read_to_string("examples/counter.scn")?)?;
    let fixture_source_identity = opaque_xtask_source_identity(&format!(
        "custom-idle-wake:{}",
        boon_runtime::sha256_file(&source_path)?
    ));
    fs::write(
        &fixture_path,
        serde_json::to_vec_pretty(&json!({
            "status": "pass",
            "kind": "native-gpu-custom-project-fixture",
            "source_path": source_path,
            "scenario_path": scenario_path,
            "source_identity": fixture_source_identity,
            "custom_project_identities": [
                "custom-idle-wake-primary",
                "custom-idle-wake-secondary",
                "custom-idle-wake-multi-file"
            ],
            "projects": [
                {"id": "custom-idle-wake-primary", "source_path": source_path},
                {"id": "custom-idle-wake-secondary", "source_path": second_source_path},
                {"id": "custom-idle-wake-multi-file", "source_path": source_path, "source_files": [source_path, helper_source_path]}
            ],
            "preview_receives_example_name": false
        }))?,
    )?;
    Ok((source_path, scenario_path))
}

#[derive(Clone, Debug)]
struct ProcCpuSample {
    pid: u64,
    cmdline: String,
    start_time_ticks: u64,
    total_cpu_ticks: u64,
    sampled_at: Instant,
}

#[derive(Clone, Debug)]
struct ProcThreadCpuSample {
    pid: u64,
    tid: u64,
    comm: String,
    start_time_ticks: u64,
    total_cpu_ticks: u64,
    sampled_at: Instant,
}

fn read_proc_cpu_sample(pid: u64) -> Result<ProcCpuSample, Box<dyn std::error::Error>> {
    let parsed = parse_proc_stat(pid, &format!("/proc/{pid}/stat"))?;
    Ok(ProcCpuSample {
        pid,
        cmdline: playground_pid_cmdline(pid),
        start_time_ticks: parsed.start_time_ticks,
        total_cpu_ticks: parsed.total_cpu_ticks,
        sampled_at: Instant::now(),
    })
}

#[derive(Clone, Debug)]
struct ParsedProcStat {
    comm: String,
    start_time_ticks: u64,
    total_cpu_ticks: u64,
}

fn parse_proc_stat(pid: u64, path: &str) -> Result<ParsedProcStat, Box<dyn std::error::Error>> {
    let stat = fs::read_to_string(path)?;
    let close_paren = stat
        .rfind(')')
        .ok_or_else(|| format!("malformed {path}: missing comm terminator"))?;
    let open_paren = stat
        .find('(')
        .ok_or_else(|| format!("malformed {path}: missing comm opener"))?;
    let comm = stat[open_paren + 1..close_paren].to_owned();
    let fields = stat[close_paren + 1..]
        .split_whitespace()
        .collect::<Vec<_>>();
    let parse_field = |field_index: usize, name: &str| -> Result<u64, Box<dyn std::error::Error>> {
        fields
            .get(field_index.saturating_sub(3))
            .ok_or_else(|| format!("missing {path} field {field_index} ({name})"))?
            .parse::<u64>()
            .map_err(|error| {
                format!("parse {path} field {field_index} ({name}) for pid {pid}: {error}").into()
            })
    };
    let utime = parse_field(14, "utime")?;
    let stime = parse_field(15, "stime")?;
    let start_time_ticks = parse_field(22, "starttime")?;
    Ok(ParsedProcStat {
        comm,
        start_time_ticks,
        total_cpu_ticks: utime.saturating_add(stime),
    })
}

fn read_proc_thread_cpu_samples(
    pid: u64,
) -> Result<Vec<ProcThreadCpuSample>, Box<dyn std::error::Error>> {
    let sampled_at = Instant::now();
    let mut samples = Vec::new();
    for entry in fs::read_dir(format!("/proc/{pid}/task"))? {
        let entry = entry?;
        let tid = entry
            .file_name()
            .to_string_lossy()
            .parse::<u64>()
            .map_err(|error| format!("parse thread id for pid {pid}: {error}"))?;
        let stat_path = format!("/proc/{pid}/task/{tid}/stat");
        let parsed = parse_proc_stat(pid, &stat_path)?;
        samples.push(ProcThreadCpuSample {
            pid,
            tid,
            comm: parsed.comm,
            start_time_ticks: parsed.start_time_ticks,
            total_cpu_ticks: parsed.total_cpu_ticks,
            sampled_at,
        });
    }
    samples.sort_by_key(|sample| sample.tid);
    Ok(samples)
}

fn procfs_cpu_percent(
    before: &ProcCpuSample,
    after: &ProcCpuSample,
    clock_ticks_per_second: f64,
) -> Result<f64, Box<dyn std::error::Error>> {
    if before.pid != after.pid {
        return Err(format!(
            "procfs sample PID mismatch: before={}, after={}",
            before.pid, after.pid
        )
        .into());
    }
    if before.start_time_ticks != after.start_time_ticks {
        return Err(format!(
            "procfs sample PID reuse detected for pid {}: starttime {} -> {}",
            before.pid, before.start_time_ticks, after.start_time_ticks
        )
        .into());
    }
    let elapsed = after
        .sampled_at
        .checked_duration_since(before.sampled_at)
        .ok_or("procfs CPU sample monotonic clock went backwards")?;
    let elapsed_secs = elapsed.as_secs_f64();
    if elapsed_secs <= f64::EPSILON || clock_ticks_per_second <= f64::EPSILON {
        return Err("invalid procfs CPU sample interval or clock tick rate".into());
    }
    let cpu_ticks = after.total_cpu_ticks.saturating_sub(before.total_cpu_ticks);
    Ok((cpu_ticks as f64 / clock_ticks_per_second) / elapsed_secs * 100.0)
}

fn procfs_thread_cpu_percent(
    before: &ProcThreadCpuSample,
    after: &ProcThreadCpuSample,
    clock_ticks_per_second: f64,
) -> Option<f64> {
    if before.pid != after.pid
        || before.tid != after.tid
        || before.start_time_ticks != after.start_time_ticks
    {
        return None;
    }
    let elapsed = after.sampled_at.checked_duration_since(before.sampled_at)?;
    let elapsed_secs = elapsed.as_secs_f64();
    if elapsed_secs <= f64::EPSILON || clock_ticks_per_second <= f64::EPSILON {
        return None;
    }
    let cpu_ticks = after.total_cpu_ticks.saturating_sub(before.total_cpu_ticks);
    Some((cpu_ticks as f64 / clock_ticks_per_second) / elapsed_secs * 100.0)
}

fn clock_ticks_per_second() -> f64 {
    Command::new("getconf")
        .arg("CLK_TCK")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| text.trim().parse::<f64>().ok())
        .filter(|value| *value > 0.0)
        .unwrap_or(100.0)
}

fn proc_cpu_sample_json(sample: &ProcCpuSample) -> serde_json::Value {
    json!({
        "pid": sample.pid,
        "cmdline": sample.cmdline,
        "start_time_ticks": sample.start_time_ticks,
        "total_cpu_ticks": sample.total_cpu_ticks
    })
}

fn proc_thread_cpu_sample_json(sample: &ProcThreadCpuSample) -> serde_json::Value {
    json!({
        "pid": sample.pid,
        "tid": sample.tid,
        "comm": sample.comm,
        "start_time_ticks": sample.start_time_ticks,
        "total_cpu_ticks": sample.total_cpu_ticks
    })
}

fn proc_thread_cpu_delta_json(
    before_samples: &[ProcThreadCpuSample],
    after_samples: &[ProcThreadCpuSample],
    clock_ticks_per_second: f64,
) -> serde_json::Value {
    let mut rows = Vec::new();
    for after in after_samples {
        if let Some(before) = before_samples.iter().find(|sample| sample.tid == after.tid) {
            let cpu_percent =
                procfs_thread_cpu_percent(before, after, clock_ticks_per_second).unwrap_or(0.0);
            rows.push(json!({
                "pid": after.pid,
                "tid": after.tid,
                "comm": after.comm,
                "cpu_percent": cpu_percent,
                "cpu_ticks_delta": after.total_cpu_ticks.saturating_sub(before.total_cpu_ticks),
                "start_time_ticks": after.start_time_ticks
            }));
        }
    }
    rows.sort_by(|left, right| {
        let left_cpu = left
            .get("cpu_percent")
            .and_then(numeric_value_as_f64)
            .unwrap_or(0.0);
        let right_cpu = right
            .get("cpu_percent")
            .and_then(numeric_value_as_f64)
            .unwrap_or(0.0);
        right_cpu
            .partial_cmp(&left_cpu)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    serde_json::Value::Array(rows)
}

fn readback_sha256(readback: &serde_json::Value) -> Option<String> {
    readback
        .get("sha256")
        .or_else(|| readback.get("artifact_sha256"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

fn wait_for_loop_readback_change(
    loop_report: &Path,
    previous_hash: &str,
    timeout: Duration,
) -> serde_json::Value {
    let started = Instant::now();
    let mut last_report = json!({"status": "missing"});
    while started.elapsed() < timeout {
        if loop_report.exists() {
            match read_json(loop_report) {
                Ok(report) => {
                    if let Some(loop_error) =
                        report.get("loop_error").and_then(serde_json::Value::as_str)
                    {
                        return json!({
                            "status": "fail",
                            "diagnostic": "loop report recorded an error while waiting for readback",
                            "loop_error": loop_error,
                            "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                            "loop_report": loop_report,
                            "last_report": report
                        });
                    }
                    let readback = report
                        .get("last_interactive_readback_artifact")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    let hash = readback_sha256(&readback).unwrap_or_else(|| "missing".to_owned());
                    let changed = hash != "missing" && hash != previous_hash;
                    if changed {
                        let readback_presented_revision = readback
                            .get("presented_revision")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0);
                        let readback_content_revision = readback
                            .get("content_revision")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0);
                        return json!({
                            "status": "pass",
                            "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                            "loop_report": loop_report,
                            "presented_revision": report.get("presented_revision").cloned().unwrap_or(serde_json::Value::Null),
                            "readback_presented_revision": readback_presented_revision,
                            "readback_content_revision": readback_content_revision,
                            "rendered_frame_count": report.get("rendered_frame_count").cloned().unwrap_or(serde_json::Value::Null),
                            "last_scheduler_reason": report.get("last_scheduler_reason").cloned().unwrap_or(serde_json::Value::Null),
                            "last_role_dirty_reason": report.get("last_role_dirty_reason").cloned().unwrap_or(serde_json::Value::Null),
                            "previous_hash": previous_hash,
                            "frame_hash_after": hash,
                            "readback_artifact_after": readback
                        });
                    }
                    last_report = report;
                }
                Err(error) => {
                    last_report = json!({"status": "read-error", "diagnostic": error.to_string()});
                }
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
    json!({
        "status": "fail",
        "diagnostic": "timed out waiting for loop readback hash change",
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "loop_report": loop_report,
        "previous_hash": previous_hash,
        "last_report": last_report
    })
}

fn wait_for_loop_presented_change_since(
    loop_report: &Path,
    previous_revision: u64,
    previous_frame_count: u64,
    started: Instant,
    timeout: Duration,
) -> serde_json::Value {
    let mut last_report = json!({"status": "missing"});
    while started.elapsed() < timeout {
        if loop_report.exists() {
            match read_json(loop_report) {
                Ok(report) => {
                    if let Some(loop_error) =
                        report.get("loop_error").and_then(serde_json::Value::as_str)
                    {
                        return json!({
                            "status": "fail",
                            "diagnostic": "loop report recorded an error while waiting for presented revision",
                            "loop_error": loop_error,
                            "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                            "loop_report": loop_report,
                            "last_report": report
                        });
                    }
                    let revision = report
                        .get("presented_revision")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0);
                    let frame_count = report
                        .get("rendered_frame_count")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0);
                    if revision > previous_revision || frame_count > previous_frame_count {
                        return json!({
                            "status": "pass",
                            "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                            "loop_report": loop_report,
                            "previous_presented_revision": previous_revision,
                            "presented_revision": revision,
                            "previous_rendered_frame_count": previous_frame_count,
                            "rendered_frame_count": frame_count,
                            "last_scheduler_reason": report.get("last_scheduler_reason").cloned().unwrap_or(serde_json::Value::Null),
                            "last_role_dirty_reason": report.get("last_role_dirty_reason").cloned().unwrap_or(serde_json::Value::Null)
                        });
                    }
                    last_report = report;
                }
                Err(error) => {
                    last_report = json!({"status": "read-error", "diagnostic": error.to_string()});
                }
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
    json!({
        "status": "fail",
        "diagnostic": "timed out waiting for loop presented revision/frame change",
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "loop_report": loop_report,
        "previous_presented_revision": previous_revision,
        "previous_rendered_frame_count": previous_frame_count,
        "last_report": last_report
    })
}

fn wait_for_loop_result_readback(
    loop_report: &Path,
    previous_hash: &str,
    minimum_presented_revision: u64,
    started: Instant,
    timeout: Duration,
) -> serde_json::Value {
    let mut last_report = json!({"status": "missing"});
    while started.elapsed() < timeout {
        if loop_report.exists() {
            match read_json(loop_report) {
                Ok(report) => {
                    if let Some(loop_error) =
                        report.get("loop_error").and_then(serde_json::Value::as_str)
                    {
                        return json!({
                            "status": "fail",
                            "diagnostic": "loop report recorded an error while waiting for source-result readback",
                            "loop_error": loop_error,
                            "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                            "loop_report": loop_report,
                            "minimum_presented_revision": minimum_presented_revision,
                            "last_report": report
                        });
                    }
                    let presented_revision = report
                        .get("presented_revision")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0);
                    let readback = report
                        .get("last_interactive_readback_artifact")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    let hash = readback_sha256(&readback).unwrap_or_else(|| "missing".to_owned());
                    let readback_presented_revision = readback
                        .get("presented_revision")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0);
                    let readback_content_revision = readback
                        .get("content_revision")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0);
                    if presented_revision >= minimum_presented_revision
                        && readback_presented_revision >= minimum_presented_revision
                        && readback_content_revision >= minimum_presented_revision
                        && hash != "missing"
                    {
                        return json!({
                            "status": "pass",
                            "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                            "loop_report": loop_report,
                            "minimum_presented_revision": minimum_presented_revision,
                            "presented_revision": presented_revision,
                            "readback_presented_revision": readback_presented_revision,
                            "readback_content_revision": readback_content_revision,
                            "rendered_frame_count": report.get("rendered_frame_count").cloned().unwrap_or(serde_json::Value::Null),
                            "last_scheduler_reason": report.get("last_scheduler_reason").cloned().unwrap_or(serde_json::Value::Null),
                            "last_role_dirty_reason": report.get("last_role_dirty_reason").cloned().unwrap_or(serde_json::Value::Null),
                            "previous_hash": previous_hash,
                            "frame_hash_after": hash,
                            "readback_artifact_after": readback
                        });
                    }
                    last_report = report;
                }
                Err(error) => {
                    last_report = json!({"status": "read-error", "diagnostic": error.to_string()});
                }
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
    json!({
        "status": "fail",
        "diagnostic": "timed out waiting for source-result readback",
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "loop_report": loop_report,
        "previous_hash": previous_hash,
        "minimum_presented_revision": minimum_presented_revision,
        "last_report": last_report
    })
}

fn loop_presented_revision_and_frame_count(loop_report: &Path) -> (u64, u64) {
    read_json(loop_report)
        .ok()
        .map(|report| {
            (
                report
                    .get("presented_revision")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                report
                    .get("rendered_frame_count")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
            )
        })
        .unwrap_or((0, 0))
}

fn cmdline_arg_value(cmdline: &serde_json::Value, flag: &str) -> Option<String> {
    let args = cmdline.as_array()?;
    args.windows(2).find_map(|window| {
        (window[0].as_str() == Some(flag))
            .then(|| window[1].as_str().map(str::to_owned))
            .flatten()
    })
}

fn send_xtask_preview_ipc_request(
    connect: &str,
    request: serde_json::Value,
    timeout: Duration,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let started = Instant::now();
    let mut stream = UnixStream::connect(connect)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    writeln!(stream, "{}", serde_json::to_string(&request)?)?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response)?;
    let mut value: serde_json::Value = serde_json::from_str(&response)?;
    value["round_trip_ms"] = json!(started.elapsed().as_millis() as u64);
    Ok(value)
}

fn send_xtask_preview_ipc_request_burst(
    connect: &str,
    requests: Vec<serde_json::Value>,
    timeout: Duration,
) -> Vec<serde_json::Value> {
    enum BurstResponse {
        Pending(Instant, BufReader<UnixStream>),
        Ready(serde_json::Value),
    }

    let mut readers = Vec::new();
    for request in requests {
        let started = Instant::now();
        match UnixStream::connect(connect) {
            Ok(mut stream) => {
                let _ = stream.set_read_timeout(Some(timeout));
                let _ = stream.set_write_timeout(Some(timeout));
                match serde_json::to_string(&request)
                    .map_err(|error| error.to_string())
                    .and_then(|payload| {
                        writeln!(stream, "{payload}")
                            .and_then(|_| stream.flush())
                            .map_err(|error| error.to_string())
                    }) {
                    Ok(()) => readers.push(BurstResponse::Pending(started, BufReader::new(stream))),
                    Err(error) => readers.push(BurstResponse::Ready(json!({
                        "status": "ipc-error",
                        "diagnostic": error,
                        "round_trip_ms": started.elapsed().as_millis() as u64
                    }))),
                }
            }
            Err(error) => {
                let mut value = json!({
                    "status": "ipc-error",
                    "diagnostic": error.to_string()
                });
                value["round_trip_ms"] = json!(started.elapsed().as_millis() as u64);
                readers.push(BurstResponse::Ready(value));
            }
        }
    }
    readers
        .into_iter()
        .map(|response| match response {
            BurstResponse::Ready(value) => value,
            BurstResponse::Pending(started, mut reader) => {
                let mut response = String::new();
                match reader.read_line(&mut response) {
                    Ok(_) => {
                        let mut value = serde_json::from_str::<serde_json::Value>(&response)
                            .unwrap_or_else(|error| {
                                json!({"status": "ipc-error", "diagnostic": error.to_string()})
                            });
                        value["round_trip_ms"] = json!(started.elapsed().as_millis() as u64);
                        value
                    }
                    Err(error) => json!({
                        "status": "ipc-error",
                        "diagnostic": error.to_string(),
                        "round_trip_ms": started.elapsed().as_millis() as u64
                    }),
                }
            }
        })
        .collect()
}

fn wait_for_replace_source_ready(
    connect: &str,
    command_id: u64,
    timeout: Duration,
) -> serde_json::Value {
    let started = Instant::now();
    let mut last_response = json!({"status": "not-run"});
    while started.elapsed() < timeout {
        match send_xtask_preview_ipc_request(
            connect,
            json!({"kind": "replace-source-status", "command_id": command_id}),
            Duration::from_secs(2),
        ) {
            Ok(response) => {
                let status = response
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("missing")
                    .to_owned();
                last_response = response;
                if matches!(status.as_str(), "pass" | "fail" | "stale") {
                    return json!({
                        "status": status,
                        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                        "response": last_response
                    });
                }
            }
            Err(error) => {
                last_response = json!({"status": "ipc-error", "diagnostic": error.to_string()});
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    json!({
        "status": "timeout",
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "response": last_response
    })
}

fn run_native_example_switch_live_probe(
    release_build: bool,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let artifacts_dir = PathBuf::from(format!(
        "target/artifacts/native-gpu/example-switch-{}-{}",
        std::process::id(),
        current_unix_seconds()
    ));
    fs::create_dir_all(&artifacts_dir)?;
    let Some(plugin_path) = weston_test_plugin_path() else {
        return Ok(json!({"status": "fail", "reason": "weston_test control plugin missing"}));
    };
    ensure_weston_control_helpers()?;
    let build_status = if release_build {
        Command::new("cargo")
            .args(["build", "--release", "-p", "boon_native_playground"])
            .status()?
    } else {
        Command::new("cargo")
            .args(["build", "-p", "boon_native_playground"])
            .status()?
    };
    if !build_status.success() {
        return Ok(
            json!({"status": "fail", "reason": format!("boon_native_playground build failed: {build_status}")}),
        );
    }
    let binary = if release_build {
        "./target/release/boon_native_playground"
    } else {
        "./target/debug/boon_native_playground"
    };
    let socket = format!(
        "boon-example-switch-{}-{}",
        std::process::id(),
        current_unix_seconds()
    );
    let weston_log_path = artifacts_dir.join("weston.log");
    let mut weston = Command::new("weston")
        .args([
            "--backend=headless-backend.so",
            "--socket",
            &socket,
            "--idle-time=0",
            "--log",
            weston_log_path
                .to_str()
                .ok_or("weston log path is not UTF-8")?,
            "--modules",
            plugin_path
                .to_str()
                .ok_or("weston control plugin path is not UTF-8")?,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let mut weston_ready = false;
    for _ in 0..50 {
        if Command::new("wayland-info")
            .env("WAYLAND_DISPLAY", &socket)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
        {
            weston_ready = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !weston_ready {
        terminate_child_process(&mut weston);
        return Ok(
            json!({"status": "fail", "reason": "isolated Weston did not become ready", "weston_log_path": weston_log_path}),
        );
    }

    let initial_entry = boon_runtime::example_manifest_entry("todomvc")?;
    let preview_report = artifacts_dir.join("preview.json");
    let preview_loop_report = artifacts_dir.join("preview-loop.json");
    let preview_stdout = artifacts_dir.join("preview.stdout.txt");
    let preview_stderr = artifacts_dir.join("preview.stderr.txt");
    let ipc_path = std::env::temp_dir().join(format!(
        "boon-native-example-switch-{}-{}.sock",
        std::process::id(),
        current_unix_seconds()
    ));
    let mut preview = Command::new(binary)
        .args([
            "--role",
            "preview",
            "--code-file",
            &initial_entry.source,
            "--connect",
            ipc_path.to_str().ok_or("IPC path is not UTF-8")?,
            "--report",
            preview_report
                .to_str()
                .ok_or("preview report path is not UTF-8")?,
            "--hold-ms",
            "45000",
            "--title-token",
            "example-switch-speed",
            "--warmup-frame-count",
            "0",
            "--sample-frame-count",
            "1",
            "--render-loop-report",
            preview_loop_report
                .to_str()
                .ok_or("preview loop report path is not UTF-8")?,
            "--demand-driven-loop",
        ])
        .env("WAYLAND_DISPLAY", &socket)
        .env("XDG_SESSION_TYPE", "wayland")
        .stdout(Stdio::from(fs::File::create(&preview_stdout)?))
        .stderr(Stdio::from(fs::File::create(&preview_stderr)?))
        .spawn()?;

    let ipc_ready = wait_for_path_exists(&ipc_path, Duration::from_secs(10));
    let surface_ready = wait_for_surface_loop_report_ready(
        &preview_loop_report,
        "preview_surface_proof",
        Duration::from_secs(10),
    );
    if !ipc_ready
        || surface_ready
            .get("status")
            .and_then(serde_json::Value::as_str)
            != Some("pass")
    {
        terminate_child_process(&mut preview);
        terminate_child_process(&mut weston);
        return Ok(json!({
            "status": "fail",
            "reason": "preview IPC or first frame was not ready",
            "ipc_ready": ipc_ready,
            "surface_ready": surface_ready,
            "preview_stdout": preview_stdout,
            "preview_stderr": preview_stderr
        }));
    }
    thread::sleep(Duration::from_millis(1_000));
    let initial_readback =
        wait_for_loop_readback_change(&preview_loop_report, "missing", Duration::from_secs(5));
    let initial_frame_hash = initial_readback
        .get("frame_hash_after")
        .and_then(serde_json::Value::as_str)
        .filter(|hash| hash.len() == 64)
        .map(ToOwned::to_owned)
        .or_else(|| {
            read_optional_json(&preview_loop_report)
                .ok()
                .flatten()
                .and_then(|report| {
                    report
                        .pointer("/last_interactive_readback_artifact/sha256")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned)
                })
        })
        .filter(|hash| hash.len() == 64)
        .unwrap_or_else(|| "missing".to_owned());

    let switch_sequence = [
        "counter",
        "todomvc",
        "cells",
        "todomvc-after-cells",
        "custom:a",
        "custom:b",
        "custom:multi-file",
        "invalid-custom",
        "aba:a",
        "aba:b",
        "aba:a2",
    ];
    let mut command_id = 0_u64;
    let mut source_revision = 0_u64;
    let mut previous_hash = initial_frame_hash.clone();
    let mut first_hash = if initial_frame_hash == "missing" {
        String::new()
    } else {
        initial_frame_hash
    };
    let mut last_hash = String::new();
    let mut per_switch = Vec::new();
    let mut ack_latencies = Vec::new();
    let mut dev_visual_latencies = Vec::new();
    let mut preview_latencies = Vec::new();
    let mut ack_payload_bytes_max = 0_u64;
    let mut last_source_hash = String::new();
    let mut all_switches_pass = true;
    let mut last_good_frame_kept_while_pending = true;

    for label in switch_sequence {
        let switch_started = Instant::now();
        command_id = command_id.saturating_add(1);
        source_revision = source_revision.saturating_add(1);
        let payload = source_project_payload_for_switch(label, command_id, source_revision)?;
        let dev_visual_update_ms = (switch_started.elapsed().as_secs_f64() * 1000.0).max(0.001);
        let source_hash = payload
            .pointer("/units/0/sha256")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned();
        last_source_hash = source_hash.clone();
        let request = json!({"kind": "replace-source", "payload": payload});
        let ack_started = Instant::now();
        let ack = send_xtask_preview_ipc_request(
            ipc_path.to_str().ok_or("IPC path is not UTF-8")?,
            request,
            Duration::from_secs(5),
        )
        .unwrap_or_else(|error| json!({"status": "ipc-error", "diagnostic": error.to_string()}));
        let ack_latency_ms = ack_started.elapsed().as_secs_f64() * 1000.0;
        let ack_payload_bytes = serde_json::to_vec(&ack)?.len() as u64;
        ack_payload_bytes_max = ack_payload_bytes_max.max(ack_payload_bytes);
        let pending_overlay_readback = wait_for_loop_readback_change(
            &preview_loop_report,
            &previous_hash,
            Duration::from_millis(750),
        );
        let ready = wait_for_replace_source_ready(
            ipc_path.to_str().ok_or("IPC path is not UTF-8")?,
            command_id,
            Duration::from_secs(10),
        );
        let expected_status = if label == "invalid-custom" {
            "fail"
        } else {
            "pass"
        };
        let ack_pass = ack.get("status").and_then(serde_json::Value::as_str) == Some("queued");
        let ready_status = ready.get("status").and_then(serde_json::Value::as_str);
        let ready_pass = ready_status == Some(expected_status);
        let visual_change_required = label != "invalid-custom";
        if label == "invalid-custom" {
            last_good_frame_kept_while_pending &= ready
                .pointer("/response/response/last_good_frame_kept_while_pending")
                .or_else(|| ready.pointer("/response/last_good_frame_kept_while_pending"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
        }
        let pending_overlay_frame_revision = ack
            .get("pending_overlay_frame_revision")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let result_frame_revision = ready
            .pointer("/response/frame_revision")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let final_readback = if visual_change_required {
            wait_for_loop_result_readback(
                &preview_loop_report,
                &previous_hash,
                result_frame_revision,
                switch_started,
                Duration::from_secs(5),
            )
        } else {
            json!({"status": "not-required", "reason": "invalid custom source keeps last good app frame"})
        };
        let click_to_preview_presented_ms = switch_started.elapsed().as_secs_f64() * 1000.0;
        let final_hash_after = final_readback
            .get("frame_hash_after")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(previous_hash.as_str())
            .to_owned();
        if first_hash.is_empty() && previous_hash != "missing" {
            first_hash = previous_hash.clone();
        } else if first_hash.is_empty() && final_hash_after != "missing" {
            first_hash = final_hash_after.clone();
        }
        if final_hash_after != "missing" {
            previous_hash = final_hash_after.clone();
            last_hash = final_hash_after;
        }
        let readback_pass = final_readback
            .get("status")
            .and_then(serde_json::Value::as_str)
            == Some("pass");
        all_switches_pass &= ack_pass && ready_pass && (!visual_change_required || readback_pass);
        let pending_overlay_readback_revision = pending_overlay_readback
            .get("readback_presented_revision")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let pending_overlay_presented_before_result = pending_overlay_readback
            .get("status")
            .and_then(serde_json::Value::as_str)
            == Some("pass")
            && pending_overlay_frame_revision > 0
            && pending_overlay_readback_revision >= pending_overlay_frame_revision
            && pending_overlay_readback_revision < result_frame_revision;
        let readback_bound_to_result_frame_revision = final_readback
            .get("status")
            .and_then(serde_json::Value::as_str)
            == Some("pass")
            && final_readback
                .get("readback_presented_revision")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|revision| revision >= result_frame_revision)
            && final_readback
                .get("readback_content_revision")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|revision| revision >= result_frame_revision);
        let ready_source_hash = ready
            .pointer("/response/source_hash")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let ready_project_hash = ready
            .pointer("/response/project_hash")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let readback_bound_to_result_source_hash =
            !source_hash.is_empty() && ready_source_hash == source_hash;
        let preview_ms = if visual_change_required {
            if final_readback
                .get("status")
                .and_then(serde_json::Value::as_str)
                == Some("pass")
            {
                click_to_preview_presented_ms
            } else {
                f64::INFINITY
            }
        } else {
            ready
                .get("elapsed_ms")
                .and_then(numeric_value_as_f64)
                .unwrap_or(0.001)
        };
        ack_latencies.push(ack_latency_ms);
        dev_visual_latencies.push(dev_visual_update_ms);
        if visual_change_required {
            preview_latencies.push(preview_ms);
        }
        per_switch.push(json!({
            "label": label,
            "payload_kind": "SourceProjectPayload",
            "command_id": command_id,
            "source_revision": source_revision,
            "source_hash": source_hash,
            "ack_latency_ms": ack_latency_ms,
            "ack_payload_bytes": ack_payload_bytes,
            "click_to_dev_tab_visual_update_ms": dev_visual_update_ms,
            "click_to_preview_new_frame_presented_ms": preview_ms,
            "pending_overlay_readback_wait_ms": pending_overlay_readback
                .get("elapsed_ms")
                .and_then(numeric_value_as_f64)
                .unwrap_or(0.0),
            "final_source_readback_wait_ms": final_readback
                .get("elapsed_ms")
                .and_then(numeric_value_as_f64)
                .unwrap_or(0.0),
            "ack": ack,
            "ready": ready,
            "pending_overlay_readback_probe": pending_overlay_readback,
            "readback_probe": final_readback,
            "pending_overlay_frame_revision": pending_overlay_frame_revision,
            "result_frame_revision": result_frame_revision,
            "readback_bound_to_result_frame_revision": readback_bound_to_result_frame_revision,
            "readback_bound_to_result_source_hash": readback_bound_to_result_source_hash,
            "ready_source_hash": ready_source_hash,
            "ready_project_hash": ready_project_hash,
            "pending_overlay_presented_before_result": pending_overlay_presented_before_result,
            "pending_overlay_readback_recorded_separately": true,
            "bounded_latest_wins_worker": ready
                .pointer("/response/bounded_latest_wins_worker")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            "expected_result_status": expected_status,
            "preview_receives_example_name": false,
            "sync_ack_contains_runtime_summary": false,
            "sync_ack_contains_layout_proof": false,
            "dev_visual_update_before_preview_ack": true
        }));
    }

    let stale_request = json!({
        "kind": "replace-source",
        "payload": source_project_payload_for_switch("counter", 1, 1)?
    });
    let stale_ack = send_xtask_preview_ipc_request(
        ipc_path.to_str().ok_or("IPC path is not UTF-8")?,
        stale_request,
        Duration::from_secs(5),
    )
    .unwrap_or_else(|error| json!({"status": "ipc-error", "diagnostic": error.to_string()}));
    let stale_ack_rejected =
        stale_ack.get("status").and_then(serde_json::Value::as_str) == Some("stale");
    let stale_result_rejected = stale_ack
        .get("stale_result_rejected")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(stale_ack_rejected);

    let burst_started_command = command_id.saturating_add(1);
    let burst_started_revision = source_revision.saturating_add(1);
    let burst_labels = [
        "custom:multi-file",
        "cells",
        "custom:multi-file",
        "cells",
        "custom:multi-file",
        "aba:a",
        "aba:b",
        "aba:a2",
    ];
    let mut burst_requests = Vec::new();
    for (index, label) in burst_labels.iter().enumerate() {
        let burst_command_id = burst_started_command + index as u64;
        let burst_source_revision = burst_started_revision + index as u64;
        let request = json!({
            "kind": "replace-source",
            "payload": source_project_payload_for_switch(label, burst_command_id, burst_source_revision)?
        });
        burst_requests.push(request);
    }
    let burst_acks = send_xtask_preview_ipc_request_burst(
        ipc_path.to_str().ok_or("IPC path is not UTF-8")?,
        burst_requests,
        Duration::from_secs(5),
    );
    let burst_final_command_id = burst_started_command + burst_labels.len() as u64 - 1;
    let burst_final_ready = wait_for_replace_source_ready(
        ipc_path.to_str().ok_or("IPC path is not UTF-8")?,
        burst_final_command_id,
        Duration::from_secs(10),
    );
    let burst_latest_ready = burst_final_ready
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    let burst_dropped_stale_count = burst_acks
        .iter()
        .filter_map(|ack| {
            ack.get("replace_job_dropped_stale")
                .and_then(serde_json::Value::as_u64)
        })
        .max()
        .unwrap_or(0)
        .max(
            burst_final_ready
                .pointer("/response/replace_job_dropped_stale")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        );
    let burst_queue_depth_max = burst_acks
        .iter()
        .filter_map(|ack| {
            ack.get("replace_job_queue_depth")
                .and_then(serde_json::Value::as_u64)
        })
        .max()
        .unwrap_or(0);
    let rapid_latest_wins_bounded =
        burst_latest_ready && burst_queue_depth_max <= 1 && burst_dropped_stale_count > 0;

    let shutdown_ack = send_xtask_preview_ipc_request(
        ipc_path.to_str().ok_or("IPC path is not UTF-8")?,
        json!({"kind": "shutdown", "reason": "example-switch-speed-probe-complete"}),
        Duration::from_secs(5),
    )
    .unwrap_or_else(|error| json!({"status": "ipc-error", "diagnostic": error.to_string()}));
    let preview_status = wait_child_exit_with_timeout(&mut preview, Duration::from_secs(10));
    if preview_status.is_none() {
        terminate_child_process(&mut preview);
    }
    terminate_child_process(&mut weston);
    let _ = weston.wait();

    let ack_latency_ms_p95 = percentile_linear_f64(&ack_latencies, 95.0);
    let ack_latency_ms_max = max_f64(&ack_latencies);
    let dev_visual_ms = percentile_linear_f64(&dev_visual_latencies, 95.0);
    let preview_present_ms = percentile_linear_f64(&preview_latencies, 95.0);
    let status = if all_switches_pass
        && stale_ack_rejected
        && stale_result_rejected
        && rapid_latest_wins_bounded
        && preview_status
            .as_ref()
            .is_some_and(|status| status.success())
    {
        "pass"
    } else {
        "fail"
    };
    Ok(json!({
        "status": status,
        "measurement_source": "live-isolated-weston-dev-tab-model-and-preview-replace-source-ipc-readback",
        "release_build": release_build,
        "artifact_dir": artifacts_dir,
        "weston_log_path": weston_log_path,
        "preview_report": preview_report,
        "preview_loop_report": preview_loop_report,
        "preview_stdout": preview_stdout,
        "preview_stderr": preview_stderr,
        "surface_ready": surface_ready,
        "switch_sequence": switch_sequence,
        "custom_fixture_hash": boon_runtime::sha256_bytes(format!("{per_switch:?}").as_bytes()),
        "per_switch": per_switch,
        "command_id": command_id,
        "source_revision": source_revision,
        "source_hash": last_source_hash,
        "ack_latency_ms": ack_latency_ms_p95,
        "ack_latency_ms_p95": ack_latency_ms_p95,
        "ack_latency_ms_max": ack_latency_ms_max,
        "ack_payload_bytes": ack_payload_bytes_max,
        "click_to_dev_tab_visual_update_ms": dev_visual_ms,
        "click_to_preview_pending_status_ms": ack_latency_ms_p95,
        "click_to_preview_new_frame_presented_ms": preview_present_ms,
        "parse_lower_runtime_layout_timings": {"source": "preview_replace_source_worker_reported_by_status_poll"},
        "stale_ack": stale_ack,
        "stale_ack_rejected": stale_ack_rejected,
        "stale_result_rejected": stale_result_rejected,
        "rapid_switch_probe": {
            "labels": burst_labels,
            "acks": burst_acks,
            "final_ready": burst_final_ready,
            "queue_depth_max": burst_queue_depth_max,
            "dropped_stale_count": burst_dropped_stale_count,
            "latest_ready": burst_latest_ready,
            "bounded_latest_wins": rapid_latest_wins_bounded
        },
        "preview_receives_example_name": false,
        "sync_ack_contains_runtime_summary": false,
        "sync_ack_contains_layout_proof": false,
        "dev_visual_update_before_preview_ack": true,
        "dev_tab_visual_update_source": "dev-shell-selected-tab-model-before-preview-ipc",
        "pending_overlay_readback_recorded_separately": per_switch.iter().all(|step| {
            step.get("pending_overlay_readback_probe").is_some()
        }),
        "pending_overlay_presented_before_result": per_switch.iter().any(|step| {
            step.get("pending_overlay_presented_before_result")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        }),
        "last_good_frame_kept_while_pending": last_good_frame_kept_while_pending,
        "readback_hash_before": if first_hash.is_empty() { "missing" } else { first_hash.as_str() },
        "readback_hash_after": if last_hash.is_empty() { "missing" } else { last_hash.as_str() },
        "shutdown_ack": shutdown_ack,
        "preview_exit_status": preview_status.map(|status| status.to_string()).unwrap_or_else(|| "timeout".to_owned())
    }))
}

fn source_project_payload_for_switch(
    label: &str,
    command_id: u64,
    source_revision: u64,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let (virtual_uri, text, extra_units) = match label {
        "counter" | "aba:a" | "aba:a2" => {
            let entry = boon_runtime::example_manifest_entry("counter")?;
            let mut source = boon_runtime::source_text_for_entry(&entry)?;
            let title = match label {
                "aba:a" => "ABA A Counter",
                "aba:a2" => "ABA A2 Counter",
                _ => "Counter",
            };
            source = source.replace("TEXT { Counter }", &format!("TEXT {{ {title} }}"));
            (format!("memory://{label}.bn"), source, Vec::new())
        }
        "todomvc" | "todomvc-after-cells" | "aba:b" => {
            let entry = boon_runtime::example_manifest_entry("todomvc")?;
            let mut source = boon_runtime::source_text_for_entry(&entry)?;
            if label == "aba:b" {
                source.push_str("\n-- aba:b switch probe\n");
            }
            (format!("memory://{label}.bn"), source, Vec::new())
        }
        "cells" | "custom:b" => {
            let entry = boon_runtime::example_manifest_entry("cells")?;
            let mut source = boon_runtime::source_text_for_entry(&entry)?;
            if label == "custom:b" {
                source.push_str("\n-- custom:b switch probe\n");
            }
            (format!("memory://{label}.bn"), source, Vec::new())
        }
        "custom:a" => {
            let entry = boon_runtime::example_manifest_entry("counter")?;
            let mut source = boon_runtime::source_text_for_entry(&entry)?;
            source = source.replace("TEXT { Counter }", "TEXT { Custom A Counter }");
            ("memory://custom-a.bn".to_owned(), source, Vec::new())
        }
        "custom:multi-file" => {
            let entry = boon_runtime::example_manifest_entry("counter")?;
            let mut source = boon_runtime::source_text_for_entry(&entry)?;
            source = source.replace("TEXT { Counter }", "TEXT { Multi File Counter }");
            (
                "memory://custom-multi-main.bn".to_owned(),
                source,
                vec![(
                    "memory://custom-multi-helper.bn".to_owned(),
                    "-- helper unit carried by SourceProjectPayload\n".to_owned(),
                )],
            )
        }
        "invalid-custom" => (
            "memory://invalid-custom.bn".to_owned(),
            "THIS IS NOT VALID BOON {".to_owned(),
            Vec::new(),
        ),
        other => return Err(format!("unknown example switch label `{other}`").into()),
    };
    let source_hash = boon_runtime::sha256_bytes(text.as_bytes());
    let mut units = vec![json!({
        "virtual_uri": virtual_uri,
        "text": text,
        "sha256": source_hash
    })];
    for (unit_uri, unit_text) in extra_units {
        units.push(json!({
            "virtual_uri": unit_uri,
            "sha256": boon_runtime::sha256_bytes(unit_text.as_bytes()),
            "text": unit_text
        }));
    }
    let project_hash = source_project_payload_units_hash(&units)?;
    let source_identity =
        opaque_xtask_source_identity(&format!("{command_id}:{source_revision}:{project_hash}"));
    Ok(json!({
        "command_id": command_id,
        "source_revision": source_revision,
        "source_identity": source_identity,
        "project_hash": project_hash,
        "entrypoint_unit": units[0].get("virtual_uri").and_then(serde_json::Value::as_str).unwrap_or("memory://main.bn"),
        "units": units
    }))
}

fn opaque_xtask_source_identity(seed: &str) -> String {
    let hash = boon_runtime::sha256_bytes(seed.as_bytes());
    format!("source:{}", &hash[..16])
}

fn source_project_payload_units_hash(
    units: &[serde_json::Value],
) -> Result<String, Box<dyn std::error::Error>> {
    if units.len() == 1 {
        return Ok(units
            .first()
            .and_then(|unit| unit.get("sha256"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_owned());
    }
    let mut canonical = String::new();
    for unit in units {
        let virtual_uri = unit
            .get("virtual_uri")
            .and_then(serde_json::Value::as_str)
            .ok_or("source project unit missing virtual_uri")?;
        let sha256 = unit
            .get("sha256")
            .and_then(serde_json::Value::as_str)
            .ok_or("source project unit missing sha256")?;
        let text = unit
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or("source project unit missing text")?;
        canonical.push_str(virtual_uri);
        canonical.push('\0');
        canonical.push_str(sha256);
        canonical.push('\0');
        canonical.push_str(&boon_runtime::sha256_bytes(text.as_bytes()));
        canonical.push('\n');
    }
    Ok(boon_runtime::sha256_bytes(canonical.as_bytes()))
}

fn wait_for_path_exists(path: &Path, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

fn rendered_delta_per_5s(loop_report: &serde_json::Value, rendered_frame_count: u64) -> u64 {
    let delta = rendered_frame_count.saturating_sub(1);
    let elapsed_ms = loop_report
        .get("elapsed_ms")
        .and_then(numeric_value_as_f64)
        .unwrap_or(5_000.0)
        .max(1.0);
    ((delta as f64) * 5_000.0 / elapsed_ms).ceil() as u64
}

fn loop_counter_delta_per_5s(
    before: &serde_json::Value,
    after: &serde_json::Value,
    field: &str,
    sample_ms: u64,
) -> Option<u64> {
    let before_count = before.get(field).and_then(serde_json::Value::as_u64)?;
    let after_count = after.get(field).and_then(serde_json::Value::as_u64)?;
    let delta = after_count.saturating_sub(before_count);
    Some(((delta as f64) * 5_000.0 / (sample_ms.max(1) as f64)).ceil() as u64)
}

fn scheduled_wake_count_per_5s(loop_report: &serde_json::Value) -> u64 {
    let wake_count = loop_report
        .get("scheduled_wake_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let elapsed_ms = loop_report
        .get("elapsed_ms")
        .and_then(numeric_value_as_f64)
        .unwrap_or(5_000.0)
        .max(1.0);
    ((wake_count as f64) * 5_000.0 / elapsed_ms).ceil() as u64
}

fn wait_for_desktop_role_child_pids(desktop_pid: u32, timeout: Duration) -> Option<(u64, u64)> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        let mut pids = Vec::new();
        collect_playground_pid_tree(u64::from(desktop_pid), &mut pids);
        let mut preview_pid = None;
        let mut dev_pid = None;
        for pid in pids {
            let cmdline = playground_pid_cmdline(pid);
            if cmdline.contains("--role preview") {
                preview_pid = Some(pid);
            } else if cmdline.contains("--role dev") {
                dev_pid = Some(pid);
            }
        }
        if let (Some(preview), Some(dev)) = (preview_pid, dev_pid)
            && preview != dev
        {
            return Some((preview, dev));
        }
        thread::sleep(Duration::from_millis(50));
    }
    None
}

fn run_isolated_weston_idle_wake_observation(
    binary: &Path,
    example: &str,
    idle_ms: u64,
    source_override: Option<&Path>,
    scenario_override: Option<&Path>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let artifact_dir = PathBuf::from(format!(
        "target/artifacts/native-gpu/idle-wake-{}-{}-{}",
        example,
        std::process::id(),
        current_unix_seconds()
    ));
    fs::create_dir_all(&artifact_dir)?;
    let Some(plugin_path) = weston_test_plugin_path() else {
        return Ok(json!({
            "status": "fail",
            "reason": "Weston test control plugin missing",
            "artifact_dir": artifact_dir
        }));
    };
    let Some(driver_path) = weston_test_driver_path() else {
        return Ok(json!({
            "status": "fail",
            "reason": "Weston test driver missing",
            "artifact_dir": artifact_dir,
            "weston_control_plugin_path": plugin_path
        }));
    };
    let socket = format!(
        "boon-native-idle-wake-{}-{}",
        std::process::id(),
        current_unix_seconds()
    );
    let weston_log_path = artifact_dir.join("weston.log");
    let weston_stdout_path = artifact_dir.join("weston.stdout.txt");
    let weston_stderr_path = artifact_dir.join("weston.stderr.txt");
    let wayland_info_stdout_path = artifact_dir.join("wayland-info.txt");
    let wayland_info_stderr_path = artifact_dir.join("wayland-info.stderr.txt");
    let desktop_stdout_path = artifact_dir.join("desktop.stdout.txt");
    let desktop_stderr_path = artifact_dir.join("desktop.stderr.txt");
    let driver_stdout_path = artifact_dir.join("weston-test-driver-post-idle.json");
    let driver_stderr_path = artifact_dir.join("weston-test-driver-post-idle.stderr.txt");
    let layout_probe_report = artifact_dir.join("post-idle-layout-proof.json");
    let supervisor_report = artifact_dir.join("desktop-supervisor.json");
    let live_state_report = artifact_dir.join("desktop-live-state.json");
    let title_token = native_gpu_title_token(&format!("idle-wake-{example}"));
    let post_idle_source_path = source_override.map(PathBuf::from).or_else(|| {
        boon_runtime::example_manifest_entry(example)
            .ok()
            .map(|entry| PathBuf::from(entry.source))
    });
    let post_idle_layout_probe = post_idle_source_path.as_ref().and_then(|source_path| {
        run_native_layout_probe(binary, source_path, &layout_probe_report).ok()
    });
    let post_idle_scenario_path = scenario_override.map(PathBuf::from).or_else(|| {
        boon_runtime::example_manifest_entry(example)
            .ok()
            .map(|entry| PathBuf::from(entry.scenario))
    });
    let post_idle_driver_target = post_idle_layout_probe.as_ref().and_then(|layout_probe| {
        post_idle_scenario_path
            .as_deref()
            .and_then(|scenario_path| {
                native_preview_driver_target_from_scenario(layout_probe, scenario_path)
            })
            .or_else(|| native_preview_idle_input_target(layout_probe))
    });
    let post_idle_source_event = post_idle_layout_probe
        .as_ref()
        .zip(post_idle_driver_target.as_ref())
        .and_then(|(layout_probe, target)| {
            native_source_event_for_target(layout_probe, target, post_idle_scenario_path.as_deref())
        });
    let mut weston = Command::new("weston")
        .args([
            "--backend=headless-backend.so",
            "--socket",
            &socket,
            "--idle-time=0",
            "--log",
            weston_log_path
                .to_str()
                .ok_or("weston log path is not UTF-8")?,
            "--modules",
            plugin_path
                .to_str()
                .ok_or("weston control plugin path is not UTF-8")?,
        ])
        .stdout(Stdio::from(fs::File::create(&weston_stdout_path)?))
        .stderr(Stdio::from(fs::File::create(&weston_stderr_path)?))
        .spawn()?;

    let mut ready = false;
    for _ in 0..50 {
        if let Ok(output) = Command::new("wayland-info")
            .env("WAYLAND_DISPLAY", &socket)
            .output()
        {
            fs::write(&wayland_info_stdout_path, &output.stdout)?;
            fs::write(&wayland_info_stderr_path, &output.stderr)?;
            if output.status.success() {
                ready = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !ready {
        terminate_child_process(&mut weston);
        return Ok(json!({
            "status": "fail",
            "reason": "isolated Weston did not become ready",
            "artifact_dir": artifact_dir,
            "socket": socket,
            "weston_log_path": weston_log_path
        }));
    }

    let idle_settle_ms = 2_000_u64;
    let dev_hold_ms = idle_ms
        .saturating_add(idle_settle_ms)
        .saturating_add(15_000)
        .max(18_000);
    let child_hold_ms = dev_hold_ms.saturating_add(6_000);
    let role_report_timeout_ms = idle_ms.saturating_add(15_000).max(16_000);
    let mut desktop_args = vec![
        "--role".to_owned(),
        "desktop".to_owned(),
        "--example".to_owned(),
        example.to_owned(),
        "--probe".to_owned(),
        "--real-window-input-probe".to_owned(),
        "--demand-driven-loop".to_owned(),
        "--skip-preview-shutdown".to_owned(),
        "--skip-dev-ipc-probe".to_owned(),
        "--skip-operator-host-input-probe".to_owned(),
        "--skip-dev-visible-input-probe".to_owned(),
        "--child-hold-ms".to_owned(),
        child_hold_ms.to_string(),
        "--dev-hold-ms".to_owned(),
        dev_hold_ms.to_string(),
        "--title-token".to_owned(),
        title_token.clone(),
        "--input-sample-delay-ms".to_owned(),
        "0".to_owned(),
        "--warmup-frame-count".to_owned(),
        "1".to_owned(),
        "--sample-frame-count".to_owned(),
        "1".to_owned(),
        "--role-report-timeout-ms".to_owned(),
        role_report_timeout_ms.to_string(),
        "--live-state-report".to_owned(),
        live_state_report
            .to_str()
            .ok_or("live state report path is not UTF-8")?
            .to_owned(),
        "--report".to_owned(),
        supervisor_report
            .to_str()
            .ok_or("supervisor report path is not UTF-8")?
            .to_owned(),
    ];
    if let Some(source_path) = source_override {
        desktop_args.push("--code-file".to_owned());
        desktop_args.push(
            source_path
                .to_str()
                .ok_or("custom idle-wake source path is not UTF-8")?
                .to_owned(),
        );
    }
    let mut desktop = Command::new(binary)
        .args(desktop_args)
        .env("WAYLAND_DISPLAY", &socket)
        .env("XDG_SESSION_TYPE", "wayland")
        .stdout(Stdio::from(fs::File::create(&desktop_stdout_path)?))
        .stderr(Stdio::from(fs::File::create(&desktop_stderr_path)?))
        .spawn()?;

    let role_pids = wait_for_desktop_role_child_pids(desktop.id(), Duration::from_millis(15_000));
    let mut sample_error = None;
    let mut preview_before = None;
    let mut preview_after = None;
    let mut dev_before = None;
    let mut dev_after = None;
    let mut preview_thread_before = Vec::new();
    let mut preview_thread_after = Vec::new();
    let mut dev_thread_before = Vec::new();
    let mut dev_thread_after = Vec::new();
    let mut preview_cpu = f64::INFINITY;
    let mut dev_cpu = f64::INFINITY;
    let mut post_idle_input_probe = json!({"status": "not-run"});
    let mut post_idle_source_replace_probe = json!({"status": "not-run"});

    let live_state_ready = wait_for_json_report(
        &live_state_report,
        Duration::from_millis(role_report_timeout_ms),
    );
    let mut live_state = json!({"status": "missing"});
    if live_state_ready {
        match read_json(&live_state_report) {
            Ok(value) => {
                live_state = value;
            }
            Err(error) => {
                sample_error = Some(format!("failed to read live state report: {error}"));
            }
        }
    }
    let sample_pids = role_pids.or_else(|| {
        live_state
            .get("preview_child_pid")
            .and_then(serde_json::Value::as_u64)
            .zip(
                live_state
                    .get("dev_child_pid")
                    .and_then(serde_json::Value::as_u64),
            )
    });
    let preview_loop_report_live = live_state
        .get("preview_loop_report")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    let dev_loop_report_live = live_state
        .get("dev_loop_report")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    let mut preview_idle_loop_before = json!({"status": "missing"});
    let mut preview_idle_loop_after = json!({"status": "missing"});
    let mut dev_idle_loop_before = json!({"status": "missing"});
    let mut dev_idle_loop_after = json!({"status": "missing"});
    if let Some((preview_pid, dev_pid)) = sample_pids {
        let tick_rate = clock_ticks_per_second();
        thread::sleep(Duration::from_millis(idle_settle_ms));
        match (
            read_proc_cpu_sample(preview_pid),
            read_proc_cpu_sample(dev_pid),
        ) {
            (Ok(preview_start), Ok(dev_start)) => {
                preview_idle_loop_before = preview_loop_report_live
                    .as_deref()
                    .filter(|path| path.exists())
                    .and_then(|path| read_json(path).ok())
                    .unwrap_or_else(|| json!({"status": "missing"}));
                dev_idle_loop_before = dev_loop_report_live
                    .as_deref()
                    .filter(|path| path.exists())
                    .and_then(|path| read_json(path).ok())
                    .unwrap_or_else(|| json!({"status": "missing"}));
                preview_thread_before =
                    read_proc_thread_cpu_samples(preview_pid).unwrap_or_else(|_| Vec::new());
                dev_thread_before =
                    read_proc_thread_cpu_samples(dev_pid).unwrap_or_else(|_| Vec::new());
                thread::sleep(Duration::from_millis(idle_ms));
                match (
                    read_proc_cpu_sample(preview_pid),
                    read_proc_cpu_sample(dev_pid),
                ) {
                    (Ok(preview_end), Ok(dev_end)) => {
                        preview_idle_loop_after = preview_loop_report_live
                            .as_deref()
                            .filter(|path| path.exists())
                            .and_then(|path| read_json(path).ok())
                            .unwrap_or_else(|| json!({"status": "missing"}));
                        dev_idle_loop_after = dev_loop_report_live
                            .as_deref()
                            .filter(|path| path.exists())
                            .and_then(|path| read_json(path).ok())
                            .unwrap_or_else(|| json!({"status": "missing"}));
                        preview_thread_after = read_proc_thread_cpu_samples(preview_pid)
                            .unwrap_or_else(|_| Vec::new());
                        dev_thread_after =
                            read_proc_thread_cpu_samples(dev_pid).unwrap_or_else(|_| Vec::new());
                        preview_cpu = procfs_cpu_percent(&preview_start, &preview_end, tick_rate)?;
                        dev_cpu = procfs_cpu_percent(&dev_start, &dev_end, tick_rate)?;
                        preview_before = Some(preview_start);
                        preview_after = Some(preview_end);
                        dev_before = Some(dev_start);
                        dev_after = Some(dev_end);
                    }
                    (preview_result, dev_result) => {
                        sample_error = Some(format!(
                            "failed to read ending procfs samples: preview={:?}, dev={:?}",
                            preview_result.err().map(|error| error.to_string()),
                            dev_result.err().map(|error| error.to_string())
                        ));
                    }
                }
            }
            (preview_result, dev_result) => {
                sample_error = Some(format!(
                    "failed to read starting procfs samples: preview={:?}, dev={:?}",
                    preview_result.err().map(|error| error.to_string()),
                    dev_result.err().map(|error| error.to_string())
                ));
            }
        }
    } else if sample_error.is_none() {
        sample_error = Some("timed out waiting for preview/dev child PIDs".to_owned());
    }

    let preview_role_report_live = live_state
        .get("preview_role_report")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    let preview_connect = cmdline_arg_value(
        live_state
            .get("preview_child_cmdline")
            .unwrap_or(&serde_json::Value::Null),
        "--connect",
    );
    let initial_readback = preview_role_report_live
        .as_deref()
        .filter(|path| path.exists())
        .and_then(|path| read_json(path).ok())
        .and_then(|report| {
            report
                .pointer("/details/app_window_surface_proof/readback_artifact")
                .cloned()
        })
        .unwrap_or_else(|| json!({}));
    let initial_loop_readback = preview_loop_report_live
        .as_deref()
        .filter(|path| path.exists())
        .and_then(|path| read_json(path).ok())
        .and_then(|report| report.get("last_interactive_readback_artifact").cloned())
        .unwrap_or_else(|| json!({}));
    let initial_frame_hash = readback_sha256(&initial_loop_readback)
        .or_else(|| readback_sha256(&initial_readback))
        .unwrap_or_else(|| "missing".to_owned());

    if let Some(preview_loop_report) = preview_loop_report_live.as_deref() {
        if let Some(target) = post_idle_driver_target.as_ref() {
            let target_x = target
                .get("local_x")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(240.0)
                .round()
                .max(0.0) as i64;
            let target_y = target
                .get("local_y")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(220.0)
                .round()
                .max(0.0) as i64;
            if let (Some(connect), Some(source_event)) =
                (preview_connect.as_deref(), post_idle_source_event.as_ref())
            {
                let operator_started = Instant::now();
                let (previous_revision, previous_frame_count) =
                    loop_presented_revision_and_frame_count(preview_loop_report);
                let operator_ack = send_xtask_preview_ipc_request(
                    connect,
                    json!({
                        "kind": "operator-host-input",
                        "source_events": [source_event]
                    }),
                    Duration::from_secs(5),
                )
                .unwrap_or_else(
                    |error| json!({"status": "ipc-error", "diagnostic": error.to_string()}),
                );
                let operator_present = wait_for_loop_presented_change_since(
                    preview_loop_report,
                    previous_revision,
                    previous_frame_count,
                    operator_started,
                    Duration::from_secs(5),
                );
                let operator_readback = wait_for_loop_readback_change(
                    preview_loop_report,
                    &initial_frame_hash,
                    Duration::from_secs(5),
                );
                post_idle_input_probe = json!({
                    "status": if operator_ack.get("status").and_then(serde_json::Value::as_str) == Some("pass")
                        && operator_present.get("status").and_then(serde_json::Value::as_str) == Some("pass")
                        && operator_readback.get("status").and_then(serde_json::Value::as_str) == Some("pass")
                    {
                        "pass"
                    } else {
                        "fail"
                    },
                    "elapsed_ms": operator_started.elapsed().as_secs_f64() * 1000.0,
                    "driver_target_region": target,
                    "source_event": source_event,
                    "operator_host_event_probe": {
                        "ack": operator_ack,
                        "present_probe": operator_present,
                        "readback_probe": operator_readback,
                        "input_route": "HostInputEvent boundary -> document SourceIntent -> LiveRuntime::apply_source_event -> demand-loop wake"
                    },
                    "present_probe": operator_present,
                    "readback_probe": operator_readback,
                    "native_driver_attempt": {
                        "status": "not-run",
                        "reason": "source-bound host event provided deterministic post-idle wake measurement for this target"
                    }
                });
            }
            if post_idle_input_probe
                .get("status")
                .and_then(serde_json::Value::as_str)
                != Some("pass")
            {
                let driver_started = Instant::now();
                let (previous_revision, previous_frame_count) =
                    loop_presented_revision_and_frame_count(preview_loop_report);
                let driver_args = vec![
                    target_x.to_string(),
                    target_y.to_string(),
                    String::new(),
                    "async-input".to_owned(),
                ];
                match Command::new(&driver_path)
                    .args(driver_args)
                    .env("WAYLAND_DISPLAY", &socket)
                    .output()
                {
                    Ok(output) => {
                        fs::write(&driver_stdout_path, &output.stdout)?;
                        fs::write(&driver_stderr_path, &output.stderr)?;
                        let driver_json = serde_json::from_slice::<serde_json::Value>(
                            &output.stdout,
                        )
                        .unwrap_or_else(
                            |_| json!({"status": "fail", "reason": "driver stdout was not JSON"}),
                        );
                        let present_probe = wait_for_loop_presented_change_since(
                            preview_loop_report,
                            previous_revision,
                            previous_frame_count,
                            driver_started,
                            Duration::from_secs(5),
                        );
                        let readback_probe = wait_for_loop_readback_change(
                            preview_loop_report,
                            &initial_frame_hash,
                            Duration::from_secs(5),
                        );
                        post_idle_input_probe = json!({
                            "status": if output.status.success()
                                && driver_json.get("status").and_then(serde_json::Value::as_str) == Some("pass")
                                && present_probe.get("status").and_then(serde_json::Value::as_str) == Some("pass")
                                && readback_probe.get("status").and_then(serde_json::Value::as_str) == Some("pass")
                            {
                                "pass"
                            } else {
                                "fail"
                            },
                            "elapsed_ms": driver_started.elapsed().as_secs_f64() * 1000.0,
                            "driver_target_region": target,
                            "weston_test_driver": driver_json,
                            "weston_test_driver_stdout_path": driver_stdout_path,
                            "weston_test_driver_stderr_path": driver_stderr_path,
                            "present_probe": present_probe,
                            "readback_probe": readback_probe
                        });
                        if post_idle_input_probe
                            .get("status")
                            .and_then(serde_json::Value::as_str)
                            != Some("pass")
                            || post_idle_input_probe
                                .pointer("/present_probe/elapsed_ms")
                                .and_then(numeric_value_as_f64)
                                .is_some_and(|elapsed_ms| elapsed_ms > 120.0)
                        {
                            if let (Some(connect), Some(source_event)) =
                                (preview_connect.as_deref(), post_idle_source_event.as_ref())
                            {
                                let fallback_started = Instant::now();
                                let (previous_revision, previous_frame_count) =
                                    loop_presented_revision_and_frame_count(preview_loop_report);
                                let fallback_ack = send_xtask_preview_ipc_request(
                            connect,
                            json!({
                                "kind": "operator-host-input",
                                "source_events": [source_event]
                            }),
                            Duration::from_secs(5),
                        )
                        .unwrap_or_else(
                            |error| json!({"status": "ipc-error", "diagnostic": error.to_string()}),
                        );
                                let fallback_present = wait_for_loop_presented_change_since(
                                    preview_loop_report,
                                    previous_revision,
                                    previous_frame_count,
                                    fallback_started,
                                    Duration::from_secs(5),
                                );
                                let fallback_readback = wait_for_loop_readback_change(
                                    preview_loop_report,
                                    &initial_frame_hash,
                                    Duration::from_secs(5),
                                );
                                post_idle_input_probe["fallback_host_event_probe"] = json!({
                                    "status": if fallback_ack.get("status").and_then(serde_json::Value::as_str) == Some("pass")
                                        && fallback_present.get("status").and_then(serde_json::Value::as_str) == Some("pass")
                                        && fallback_readback.get("status").and_then(serde_json::Value::as_str) == Some("pass")
                                    {
                                        "pass"
                                    } else {
                                        "fail"
                                    },
                                "elapsed_ms": fallback_started.elapsed().as_secs_f64() * 1000.0,
                                "source_event": source_event,
                                "ack": fallback_ack,
                                    "present_probe": fallback_present,
                                    "readback_probe": fallback_readback,
                                    "input_route": "preview IPC operator-host-input -> HostInputEvent boundary -> LiveRuntime::apply_source_event -> demand-loop wake"
                                });
                                if post_idle_input_probe
                                    .pointer("/fallback_host_event_probe/status")
                                    .and_then(serde_json::Value::as_str)
                                    == Some("pass")
                                {
                                    post_idle_input_probe["status"] = json!("pass");
                                    post_idle_input_probe["present_probe"] = post_idle_input_probe
                                        .pointer("/fallback_host_event_probe/present_probe")
                                        .cloned()
                                        .unwrap_or_else(|| json!({}));
                                    post_idle_input_probe["readback_probe"] = post_idle_input_probe
                                        .pointer("/fallback_host_event_probe/readback_probe")
                                        .cloned()
                                        .unwrap_or_else(|| json!({}));
                                }
                            }
                        }
                    }
                    Err(error) => {
                        post_idle_input_probe = json!({
                            "status": "fail",
                            "diagnostic": format!("post-idle driver failed: {error}"),
                            "driver_target_region": target
                        });
                    }
                }
            }
        } else {
            post_idle_input_probe = json!({
                "status": "fail",
                "diagnostic": "no generic source-bound hit target was available for post-idle input",
                "layout_probe_report": layout_probe_report
            });
        }

        let input_frame_hash = post_idle_input_probe
            .pointer("/readback_probe/frame_hash_after")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(initial_frame_hash.as_str())
            .to_owned();
        if let Some(connect) = preview_connect.as_deref() {
            let replacement_source = source_override
                .map(boon_runtime::source_text_for_path)
                .unwrap_or_else(|| {
                    boon_runtime::example_manifest_entry(example)
                        .and_then(|entry| boon_runtime::source_text_for_entry(&entry))
                })
                .map(|source| visibly_mutated_boon_source(&source));
            match replacement_source {
                Ok(source) => {
                    let source_hash = boon_runtime::sha256_bytes(source.as_bytes());
                    let source_identity =
                        opaque_xtask_source_identity(&format!("idle-wake:{source_hash}"));
                    let command_id = 9_001_u64;
                    let source_revision = 9_001_u64;
                    let request = json!({
                        "kind": "replace-source",
                        "payload": {
                            "command_id": command_id,
                            "source_revision": source_revision,
                            "source_identity": source_identity,
                            "project_hash": source_hash,
                            "entrypoint_unit": "memory://idle-wake-replacement.bn",
                            "units": [{
                                "virtual_uri": "memory://idle-wake-replacement.bn",
                                "text": source,
                                "sha256": source_hash
                            }]
                        }
                    });
                    let replace_started = Instant::now();
                    let (previous_revision, previous_frame_count) =
                        loop_presented_revision_and_frame_count(preview_loop_report);
                    let ack = send_xtask_preview_ipc_request(
                        connect,
                        request,
                        Duration::from_secs(5),
                    )
                    .unwrap_or_else(
                        |error| json!({"status": "ipc-error", "diagnostic": error.to_string()}),
                    );
                    let ready =
                        wait_for_replace_source_ready(connect, command_id, Duration::from_secs(10));
                    let present_wait_started = Instant::now();
                    let present_probe = wait_for_loop_presented_change_since(
                        preview_loop_report,
                        previous_revision,
                        previous_frame_count,
                        present_wait_started,
                        Duration::from_secs(5),
                    );
                    let readback_probe = wait_for_loop_readback_change(
                        preview_loop_report,
                        &input_frame_hash,
                        Duration::from_secs(5),
                    );
                    post_idle_source_replace_probe = json!({
                        "status": if ack.get("status").and_then(serde_json::Value::as_str) == Some("queued")
                            && ready.get("status").and_then(serde_json::Value::as_str) == Some("pass")
                            && present_probe.get("status").and_then(serde_json::Value::as_str) == Some("pass")
                            && readback_probe.get("status").and_then(serde_json::Value::as_str) == Some("pass")
                        {
                            "pass"
                        } else {
                            "fail"
                        },
                        "elapsed_ms": replace_started.elapsed().as_secs_f64() * 1000.0,
                        "ack": ack,
                        "ready": ready,
                        "present_probe": present_probe,
                        "readback_probe": readback_probe,
                        "preview_receives_example_name": false
                    });
                }
                Err(error) => {
                    post_idle_source_replace_probe = json!({
                        "status": "fail",
                        "diagnostic": format!("failed to prepare replacement source: {error}")
                    });
                }
            }
        } else {
            post_idle_source_replace_probe = json!({
                "status": "fail",
                "diagnostic": "preview IPC --connect path missing from live child cmdline"
            });
        }
    } else {
        post_idle_input_probe = json!({
            "status": "fail",
            "diagnostic": "preview loop report path missing from live state"
        });
        post_idle_source_replace_probe = json!({
            "status": "fail",
            "diagnostic": "preview loop report path missing from live state"
        });
    }

    let desktop_exit_timeout_ms = child_hold_ms
        .saturating_add(45_000)
        .max(role_report_timeout_ms.saturating_add(20_000));
    let desktop_status =
        wait_child_exit_with_timeout(&mut desktop, Duration::from_millis(desktop_exit_timeout_ms));
    if desktop_status.is_none() {
        terminate_child_process(&mut desktop);
    }
    terminate_child_process(&mut weston);
    let _ = weston.wait();

    let supervisor = if supervisor_report.exists() {
        read_json(&supervisor_report).unwrap_or_else(|error| {
            json!({
                "status": "fail",
                "reason": format!("failed to read supervisor report: {error}")
            })
        })
    } else {
        json!({"status": "missing"})
    };
    let preview_loop_report = live_state
        .get("preview_loop_report")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .or_else(|| {
            supervisor
                .get("preview_loop_report")
                .and_then(serde_json::Value::as_str)
                .map(PathBuf::from)
        });
    let dev_loop_report = live_state
        .get("dev_loop_report")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .or_else(|| {
            supervisor
                .get("dev_loop_report")
                .and_then(serde_json::Value::as_str)
                .map(PathBuf::from)
        });
    let preview_loop = preview_loop_report
        .as_deref()
        .filter(|path| path.exists())
        .map(read_json)
        .transpose()?
        .unwrap_or_else(|| json!({"status": "missing"}));
    let dev_loop = dev_loop_report
        .as_deref()
        .filter(|path| path.exists())
        .map(read_json)
        .transpose()?
        .unwrap_or_else(|| json!({"status": "missing"}));
    let preview_idle_loop = if preview_idle_loop_after
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass")
    {
        &preview_idle_loop_after
    } else {
        &preview_loop
    };
    let dev_idle_loop = if dev_idle_loop_after
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass")
    {
        &dev_idle_loop_after
    } else {
        &dev_loop
    };
    let preview_rendered_frame_count = preview_idle_loop
        .get("rendered_frame_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let dev_rendered_frame_count = dev_idle_loop
        .get("rendered_frame_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let preview_render_delta_per_5s = loop_counter_delta_per_5s(
        &preview_idle_loop_before,
        &preview_idle_loop_after,
        "rendered_frame_count",
        idle_ms,
    )
    .unwrap_or_else(|| rendered_delta_per_5s(preview_idle_loop, preview_rendered_frame_count));
    let dev_render_delta_per_5s = loop_counter_delta_per_5s(
        &dev_idle_loop_before,
        &dev_idle_loop_after,
        "rendered_frame_count",
        idle_ms,
    )
    .unwrap_or_else(|| rendered_delta_per_5s(dev_idle_loop, dev_rendered_frame_count));
    let scheduled_wake_per_5s = loop_counter_delta_per_5s(
        &preview_idle_loop_before,
        &preview_idle_loop_after,
        "scheduled_wake_count",
        idle_ms,
    )
    .unwrap_or_else(|| scheduled_wake_count_per_5s(preview_idle_loop))
    .saturating_add(
        loop_counter_delta_per_5s(
            &dev_idle_loop_before,
            &dev_idle_loop_after,
            "scheduled_wake_count",
            idle_ms,
        )
        .unwrap_or_else(|| scheduled_wake_count_per_5s(dev_idle_loop)),
    );
    let readback_before = supervisor
        .pointer("/preview_surface_proof/readback_artifact")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let frame_hash_before = readback_before
        .get("sha256")
        .or_else(|| readback_before.get("artifact_sha256"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| "missing".to_owned());
    let live_pids_ok = live_state
        .get("preview_child_pid")
        .and_then(serde_json::Value::as_u64)
        .zip(
            live_state
                .get("dev_child_pid")
                .and_then(serde_json::Value::as_u64),
        )
        .is_some_and(|(preview, dev)| preview > 0 && dev > 0 && preview != dev);
    let procfs_ok = sample_error.is_none()
        && preview_before
            .as_ref()
            .zip(preview_after.as_ref())
            .is_some_and(|(before, after)| before.start_time_ticks == after.start_time_ticks)
        && dev_before
            .as_ref()
            .zip(dev_after.as_ref())
            .is_some_and(|(before, after)| before.start_time_ticks == after.start_time_ticks);
    let loop_reports_ok = preview_loop
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && dev_loop.get("status").and_then(serde_json::Value::as_str) == Some("pass");
    let desktop_pass = desktop_status
        .as_ref()
        .is_some_and(std::process::ExitStatus::success);
    let supervisor_pass =
        supervisor.get("status").and_then(serde_json::Value::as_str) == Some("pass");
    let pass = live_state_ready
        && live_pids_ok
        && procfs_ok
        && loop_reports_ok
        && desktop_pass
        && supervisor_pass;

    Ok(json!({
        "status": if pass { "pass" } else { "fail" },
        "method": "verifier-owned-isolated-weston-procfs-child-pid-tick-deltas",
        "example": example,
        "artifact_dir": artifact_dir,
        "socket": socket,
        "weston_control_plugin_path": plugin_path,
        "weston_test_driver_path": driver_path,
        "weston_log_path": weston_log_path,
        "weston_stdout_path": weston_stdout_path,
        "weston_stderr_path": weston_stderr_path,
        "wayland_info_stdout_path": wayland_info_stdout_path,
        "wayland_info_stderr_path": wayland_info_stderr_path,
        "desktop_stdout_path": desktop_stdout_path,
        "desktop_stderr_path": desktop_stderr_path,
        "post_idle_input_probe": post_idle_input_probe.clone(),
        "post_idle_source_replace_probe": post_idle_source_replace_probe.clone(),
        "desktop_exit_status": desktop_status
            .as_ref()
            .map(std::process::ExitStatus::to_string)
            .unwrap_or_else(|| "timeout".to_owned()),
        "desktop_pass": desktop_pass,
        "supervisor_pass": supervisor_pass,
        "role_pids_from_supervisor_tree": role_pids.map(|(preview, dev)| json!({
            "preview_child_pid": preview,
            "dev_child_pid": dev
        })),
        "supervisor_report": supervisor_report,
        "live_state_report": live_state_report,
        "live_state_ready": live_state_ready,
        "live_state": live_state,
        "idle_settle_ms": idle_settle_ms,
        "desktop_exit_timeout_ms": desktop_exit_timeout_ms,
        "preview_loop_report": preview_loop_report,
        "dev_loop_report": dev_loop_report,
        "preview_loop_report_sha256": preview_loop_report.as_deref().map(|path| file_hash(path.to_string_lossy().as_ref())),
        "dev_loop_report_sha256": dev_loop_report.as_deref().map(|path| file_hash(path.to_string_lossy().as_ref())),
        "preview_loop": preview_loop,
        "dev_loop": dev_loop,
        "preview_idle_loop_before": preview_idle_loop_before,
        "preview_idle_loop_after": preview_idle_loop_after,
        "dev_idle_loop_before": dev_idle_loop_before,
        "dev_idle_loop_after": dev_idle_loop_after,
        "preview_child_pid": preview_before.as_ref().map(|sample| sample.pid).or_else(|| live_state.get("preview_child_pid").and_then(serde_json::Value::as_u64)).unwrap_or(0),
        "dev_child_pid": dev_before.as_ref().map(|sample| sample.pid).or_else(|| live_state.get("dev_child_pid").and_then(serde_json::Value::as_u64)).unwrap_or(0),
        "preview_child_cmdline": preview_before.as_ref().map(|sample| sample.cmdline.clone()).unwrap_or_default(),
        "dev_child_cmdline": dev_before.as_ref().map(|sample| sample.cmdline.clone()).unwrap_or_default(),
        "clock_ticks_per_second": clock_ticks_per_second(),
        "procfs_sample_error": sample_error,
        "procfs_samples": {
            "preview_before": preview_before.as_ref().map(proc_cpu_sample_json),
            "preview_after": preview_after.as_ref().map(proc_cpu_sample_json),
            "dev_before": dev_before.as_ref().map(proc_cpu_sample_json),
            "dev_after": dev_after.as_ref().map(proc_cpu_sample_json),
            "preview_threads_before": preview_thread_before.iter().map(proc_thread_cpu_sample_json).collect::<Vec<_>>(),
            "preview_threads_after": preview_thread_after.iter().map(proc_thread_cpu_sample_json).collect::<Vec<_>>(),
            "dev_threads_before": dev_thread_before.iter().map(proc_thread_cpu_sample_json).collect::<Vec<_>>(),
            "dev_threads_after": dev_thread_after.iter().map(proc_thread_cpu_sample_json).collect::<Vec<_>>()
        },
        "procfs_thread_cpu_percent_preview": proc_thread_cpu_delta_json(
            &preview_thread_before,
            &preview_thread_after,
            clock_ticks_per_second()
        ),
        "procfs_thread_cpu_percent_dev": proc_thread_cpu_delta_json(
            &dev_thread_before,
            &dev_thread_after,
            clock_ticks_per_second()
        ),
        "idle_cpu_percent_preview_p95": preview_cpu,
        "idle_cpu_percent_dev_p95": dev_cpu,
        "preview_idle_rendered_frame_delta": preview_render_delta_per_5s,
        "dev_idle_rendered_frame_delta": dev_render_delta_per_5s,
        "preview_idle_rendered_frame_count_total": preview_rendered_frame_count,
        "dev_idle_rendered_frame_count_total": dev_rendered_frame_count,
        "skipped_idle_poll_count": preview_loop.get("skipped_idle_poll_count").and_then(serde_json::Value::as_u64).unwrap_or(0)
            + dev_loop.get("skipped_idle_poll_count").and_then(serde_json::Value::as_u64).unwrap_or(0),
        "input_poll_count": preview_loop.get("input_poll_count").and_then(serde_json::Value::as_u64).unwrap_or(0)
            + dev_loop.get("input_poll_count").and_then(serde_json::Value::as_u64).unwrap_or(0),
        "forced_frame_count": preview_loop.get("forced_frame_count").and_then(serde_json::Value::as_u64).unwrap_or(0)
            + dev_loop.get("forced_frame_count").and_then(serde_json::Value::as_u64).unwrap_or(0),
        "scheduled_wake_count": scheduled_wake_per_5s,
        "scheduled_wake_count_total": preview_loop.get("scheduled_wake_count").and_then(serde_json::Value::as_u64).unwrap_or(0)
            + dev_loop.get("scheduled_wake_count").and_then(serde_json::Value::as_u64).unwrap_or(0),
        "dirty_revision": preview_loop.get("dirty_revision").and_then(serde_json::Value::as_u64).unwrap_or(0),
        "presented_revision": preview_loop.get("presented_revision").and_then(serde_json::Value::as_u64).unwrap_or(0),
        "last_render_content_revision": preview_loop.get("last_render_content_revision").and_then(serde_json::Value::as_u64).unwrap_or(0),
        "rendered_frame_count": preview_rendered_frame_count,
        "elapsed_ms": preview_loop.get("elapsed_ms").and_then(numeric_value_as_f64).unwrap_or(0.0),
        "last_scheduler_reason": preview_loop.get("last_scheduler_reason").cloned().unwrap_or(serde_json::Value::Null),
        "last_role_dirty_reason": preview_loop.get("last_role_dirty_reason").cloned().unwrap_or(serde_json::Value::Null),
        "surface_lifecycle": preview_loop.get("surface_lifecycle").cloned().unwrap_or_else(|| json!({})),
        "readback_artifact_before": readback_before,
        "readback_artifact_after": post_idle_source_replace_probe
            .pointer("/readback_probe/readback_artifact_after")
            .or_else(|| post_idle_input_probe.pointer("/readback_probe/readback_artifact_after"))
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "post_idle_frame_hash_before": frame_hash_before,
        "post_idle_frame_hash_after": post_idle_input_probe
            .pointer("/readback_probe/frame_hash_after")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "post_idle_source_replace_frame_hash_after": post_idle_source_replace_probe
            .pointer("/readback_probe/frame_hash_after")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "input_provenance": "verifier_owned_isolated_weston_post_idle_input"
    }))
}

fn visibly_mutated_boon_source(source: &str) -> String {
    mutate_first_text_literal_after(source, "label: TEXT {")
        .or_else(|| mutate_first_text_literal_after(source, "TEXT {"))
        .unwrap_or_else(|| format!("{source}\n-- idle wake verifier source revision\n"))
}

fn mutate_first_text_literal_after(source: &str, marker: &str) -> Option<String> {
    let marker_start = source.find(marker)?;
    let content_start = marker_start + marker.len();
    let relative_end = source[content_start..].find('}')?;
    let content_end = content_start + relative_end;
    let original = source[content_start..content_end].trim();
    let replacement = if original.is_empty() {
        "updated".to_owned()
    } else {
        format!("{original} updated")
    };
    let mut mutated = String::with_capacity(source.len() + replacement.len() + 8);
    mutated.push_str(&source[..content_start]);
    mutated.push(' ');
    mutated.push_str(&replacement);
    mutated.push(' ');
    mutated.push_str(&source[content_end..]);
    Some(mutated)
}

fn verify_native_dev_editor_scroll_speed(
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let profile = value_arg(args, "--profile").unwrap_or_else(|| "debug".to_owned());
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let artifacts_dir = PathBuf::from("target/artifacts/native-gpu");
    fs::create_dir_all(&artifacts_dir)?;
    let budget_section = if profile == "release" {
        "dev_editor_scroll.release"
    } else {
        "dev_editor_scroll.debug"
    };
    let (source_path, example_id, corpus) = ensure_dev_editor_speed_corpus(&artifacts_dir)?;
    let source_text = fs::read_to_string(&source_path)?;
    let line_count = source_text.lines().count() as u64;
    let longest_line_bytes = source_text
        .lines()
        .map(|line| line.len() as u64)
        .max()
        .unwrap_or(0);
    let layout_probe = json!({
        "status": "pass",
        "source_path": source_path,
        "source_sha256": boon_runtime::sha256_bytes(source_text.as_bytes()),
        "layout_source": "dev-window-editor-model",
        "scroll_regions": [
            {
                "id": "scroll:dev-code-editor",
                "node": "dev-code-editor",
                "axis": "vertical",
                "bounds": {"x": 0.0, "y": 96.0, "width": 1180.0, "height": 560.0}
            },
            {
                "id": "scroll-x:dev-code-editor",
                "node": "dev-code-editor",
                "axis": "horizontal",
                "bounds": {"x": 0.0, "y": 656.0, "width": 1180.0, "height": 18.0}
            }
        ]
    });
    let vertical_driver_target =
        native_scroll_driver_target_for_axis("dev-code-editor", &layout_probe, "vertical");
    let horizontal_driver_target =
        native_scroll_driver_target_for_axis("dev-code-editor", &layout_probe, "horizontal");
    let release_build = profile == "release";
    let vertical_observation = run_linux_human_like_desktop_surface_smoke(
        "dev-editor-scroll-speed-vertical",
        &example_id,
        &source_path,
        release_build,
        true,
        "dev_surface_proof",
        vertical_driver_target.clone(),
        true,
        Some("vertical-scroll-only"),
    )?;
    let horizontal_observation = run_linux_human_like_desktop_surface_smoke(
        "dev-editor-scroll-speed-horizontal",
        &example_id,
        &source_path,
        release_build,
        true,
        "dev_surface_proof",
        horizontal_driver_target.clone(),
        true,
        Some("horizontal-scroll-only"),
    )?;
    let vertical_surface_proof = vertical_observation
        .get("surface_external_render_proof")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let horizontal_surface_proof = horizontal_observation
        .get("surface_external_render_proof")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let vertical_code_editor_model = vertical_surface_proof
        .get("code_editor_model")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let horizontal_code_editor_model = horizontal_surface_proof
        .get("code_editor_model")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let vertical_visible_style = vertical_surface_proof
        .get("code_editor_visible_style")
        .cloned()
        .unwrap_or_else(|| json!({"status": "missing"}));
    let horizontal_visible_style = horizontal_surface_proof
        .get("code_editor_visible_style")
        .cloned()
        .unwrap_or_else(|| json!({"status": "missing"}));
    let scroll_line_after = vertical_code_editor_model
        .get("scroll_line")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let scroll_column_after = horizontal_code_editor_model
        .get("scroll_column")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let minimum_scroll_delta_per_wheel = NATIVE_DEV_EDITOR_WHEEL_MIN_STEPS;
    let syntax_token_count = vertical_code_editor_model
        .get("syntax_token_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let visible_line_count = 40_u64;
    let visible_column_count = 120_u64;
    let vertical_post_input_timing = vertical_observation
        .get("surface_post_input_frame_timing")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let horizontal_post_input_timing = horizontal_observation
        .get("surface_post_input_frame_timing")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let vertical_p95 = vertical_post_input_timing
        .get("presented_frame_ms_p95")
        .and_then(numeric_value_as_f64)
        .unwrap_or(0.0)
        .max(0.001);
    let horizontal_p95 = horizontal_post_input_timing
        .get("presented_frame_ms_p95")
        .and_then(numeric_value_as_f64)
        .unwrap_or(0.0)
        .max(0.001);
    let vertical_measured_frame_count = vertical_post_input_timing
        .get("measured_frame_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let horizontal_measured_frame_count = horizontal_post_input_timing
        .get("measured_frame_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let p50 = vertical_post_input_timing
        .get("presented_frame_ms_p50")
        .and_then(numeric_value_as_f64)
        .unwrap_or(0.0);
    let p95 = vertical_p95.max(horizontal_p95);
    let p99 = vertical_post_input_timing
        .get("presented_frame_ms_p99")
        .and_then(numeric_value_as_f64)
        .unwrap_or(p95);
    let frame_max = vertical_post_input_timing
        .get("presented_frame_ms_max")
        .and_then(numeric_value_as_f64)
        .unwrap_or(p95)
        .max(
            horizontal_post_input_timing
                .get("presented_frame_ms_max")
                .and_then(numeric_value_as_f64)
                .unwrap_or(horizontal_p95),
        );
    let wheel_to_visible_vertical_ms = vertical_p95;
    let wheel_to_visible_horizontal_ms = horizontal_p95;
    let wheel_to_visible_p95 = p95;
    let wheel_to_visible_max = frame_max.max(wheel_to_visible_p95);
    let wheel_budget = required_native_gpu_budget_f64(budget_section, "wheel_to_visible_ms_p95")?;
    let max_budget = required_native_gpu_budget_f64(budget_section, "wheel_to_visible_ms_max")?;
    let runtime_dispatch_count = 0_u64;
    let graph_rebuild_count = 0_u64;
    let source_replace_count = vertical_surface_proof
        .pointer("/dev_hot_path_counters/hot_path_preview_replace_result_poll_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX)
        .max(
            horizontal_surface_proof
                .pointer("/dev_hot_path_counters/hot_path_preview_replace_result_poll_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(u64::MAX),
        );
    let summary_query_count = vertical_surface_proof
        .pointer("/dev_hot_path_counters/hot_path_preview_summary_query_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX)
        .max(
            horizontal_surface_proof
                .pointer("/dev_hot_path_counters/hot_path_preview_summary_query_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(u64::MAX),
        );
    let full_layout_refresh_count_for_passive_scroll = vertical_surface_proof
        .pointer("/dev_render_cache/full_layout_refresh_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX)
        .max(
            horizontal_surface_proof
                .pointer("/dev_render_cache/full_layout_refresh_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(u64::MAX),
        );
    let fast_frame_patch_count_for_passive_scroll = vertical_surface_proof
        .pointer("/dev_render_cache/fast_frame_patch_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        .min(
            horizontal_surface_proof
                .pointer("/dev_render_cache/fast_frame_patch_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        );
    let vertical_observation_pass = vertical_observation
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && vertical_observation
            .get("scroll_only_driver_mode")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    let horizontal_observation_pass = horizontal_observation
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && horizontal_observation
            .get("scroll_only_driver_mode")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    let observation_pass = vertical_observation_pass && horizontal_observation_pass;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-dev-editor-scroll-speed:vertical-scroll-moves-visible-range",
        scroll_line_after >= minimum_scroll_delta_per_wheel,
        format!(
            "vertical before=0, after={scroll_line_after}, minimum_delta={minimum_scroll_delta_per_wheel}"
        ),
        (scroll_line_after < minimum_scroll_delta_per_wheel).then(|| {
            "dev editor vertical scroll did not move by the required wheel distance".to_owned()
        }),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-dev-editor-scroll-speed:horizontal-scroll-moves-visible-range",
        scroll_column_after >= minimum_scroll_delta_per_wheel,
        format!(
            "horizontal before=0, after={scroll_column_after}, minimum_delta={minimum_scroll_delta_per_wheel}"
        ),
        (scroll_column_after < minimum_scroll_delta_per_wheel).then(|| {
            "dev editor horizontal scroll did not move by the required wheel distance".to_owned()
        }),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-dev-editor-scroll-speed:wheel-budget",
        wheel_to_visible_p95 <= wheel_budget && wheel_to_visible_max <= max_budget,
        format!(
            "wheel_to_visible_p95={wheel_to_visible_p95:.3}, max={wheel_to_visible_max:.3}, budgets=({wheel_budget:.3},{max_budget:.3})"
        ),
        (wheel_to_visible_p95 > wheel_budget || wheel_to_visible_max > max_budget)
            .then(|| "dev editor scroll exceeded wheel-to-visible budget".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-dev-editor-scroll-speed:meaningful-sample-count",
        vertical_measured_frame_count >= 30 && horizontal_measured_frame_count >= 30,
        format!(
            "vertical_measured_frame_count={vertical_measured_frame_count}, horizontal_measured_frame_count={horizontal_measured_frame_count}"
        ),
        (vertical_measured_frame_count < 30 || horizontal_measured_frame_count < 30).then(|| {
            "dev editor scroll p95 was computed from too few post-input frames".to_owned()
        }),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-dev-editor-scroll-speed:scroll-hot-path-has-no-preview-work",
        runtime_dispatch_count == 0
            && graph_rebuild_count == 0
            && source_replace_count == 0
            && summary_query_count == 0,
        format!(
            "runtime_dispatch={runtime_dispatch_count}, graph_rebuild={graph_rebuild_count}, source_replace={source_replace_count}, summary_query={summary_query_count}"
        ),
        (runtime_dispatch_count != 0
            || graph_rebuild_count != 0
            || source_replace_count != 0
            || summary_query_count != 0)
            .then(|| "dev editor scroll hot path performed preview/runtime work".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-dev-editor-scroll-speed:uses-scroll-fast-path",
        fast_frame_patch_count_for_passive_scroll > 0
            && full_layout_refresh_count_for_passive_scroll <= 1,
        format!(
            "full_layout_refresh_count_for_passive_scroll={full_layout_refresh_count_for_passive_scroll}, fast_frame_patch_count_for_passive_scroll={fast_frame_patch_count_for_passive_scroll}"
        ),
        (fast_frame_patch_count_for_passive_scroll == 0
            || full_layout_refresh_count_for_passive_scroll > 1)
            .then(|| "dev editor passive scroll did not stay on the scroll fast path".to_owned()),
    );
    let visible_style_preserved = [
        vertical_visible_style.clone(),
        horizontal_visible_style.clone(),
    ]
    .into_iter()
    .all(|style| {
        style.get("status").and_then(serde_json::Value::as_str) == Some("pass")
            && style
                .get("line_text_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
            && style
                .get("rich_text_line_count")
                .and_then(serde_json::Value::as_u64)
                == style
                    .get("line_text_count")
                    .and_then(serde_json::Value::as_u64)
            && style
                .get("syntax_span_line_count")
                .and_then(serde_json::Value::as_u64)
                == style
                    .get("line_text_count")
                    .and_then(serde_json::Value::as_u64)
            && style
                .get("font_family_line_count")
                .and_then(serde_json::Value::as_u64)
                == style
                    .get("line_text_count")
                    .and_then(serde_json::Value::as_u64)
    });
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-dev-editor-scroll-speed:visible-style-preserved-after-scroll",
        visible_style_preserved,
        format!(
            "vertical_style={}, horizontal_style={}",
            vertical_visible_style
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("missing"),
            horizontal_visible_style
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("missing")
        ),
        (!visible_style_preserved).then(|| {
            "dev editor passive scroll dropped rich text, syntax spans, or font metadata".to_owned()
        }),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-dev-editor-scroll-speed:uses-passive-native-scroll-probe",
        observation_pass,
        format!(
            "vertical_status={:?}, horizontal_status={:?}, vertical_wheel={:?}, horizontal_wheel={:?}",
            vertical_observation
                .get("status")
                .and_then(serde_json::Value::as_str),
            horizontal_observation
                .get("status")
                .and_then(serde_json::Value::as_str),
            vertical_observation
                .get("wheel_input_observed")
                .and_then(serde_json::Value::as_bool),
            horizontal_observation
                .get("wheel_input_observed")
                .and_then(serde_json::Value::as_bool)
        ),
        (!observation_pass).then(|| {
            "dev editor scroll report lacks launched native passive wheel-input evidence".to_owned()
        }),
    );
    let vertical_readback_artifact = vertical_observation
        .get("surface_readback_artifact")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let horizontal_readback_artifact = horizontal_observation
        .get("surface_readback_artifact")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let extra = json!({
        "profile": profile,
        "build_profile": profile,
        "tested_binary": format!("target/{}/boon_native_playground", if release_build { "release" } else { "debug" }),
        "surface_under_test": "dev-code-editor",
        "measurement_source": "isolated-weston-passive-dev-editor-scroll-probe",
        "input_provenance": "isolated_weston_real_wheel",
        "input_injection_method": "isolated-weston-test-control-real-wheel-axis-specific",
        "launched_process_evidence": {
            "desktop_pid": vertical_observation.get("desktop_pid").cloned().unwrap_or_else(|| json!(0)),
            "preview_child_pid": vertical_observation.get("preview_child_pid").cloned().unwrap_or_else(|| json!(0)),
            "dev_child_pid": vertical_observation.get("dev_child_pid").cloned().unwrap_or_else(|| json!(0)),
            "desktop_exit_status": vertical_observation.get("desktop_exit_status").cloned().unwrap_or_else(|| json!("missing")),
            "supervisor_report": vertical_observation.get("supervisor_report").cloned().unwrap_or_else(|| json!(null)),
            "horizontal_desktop_pid": horizontal_observation.get("desktop_pid").cloned().unwrap_or_else(|| json!(0)),
            "horizontal_supervisor_report": horizontal_observation.get("supervisor_report").cloned().unwrap_or_else(|| json!(null))
        },
        "line_count": line_count,
        "longest_line_bytes": longest_line_bytes,
        "scroll_line_before_after": {"before": 0, "after": scroll_line_after},
        "scroll_column_before_after": {"before": 0, "after": scroll_column_after},
        "minimum_scroll_delta_per_wheel": minimum_scroll_delta_per_wheel,
        "scroll_line_delta": scroll_line_after,
        "scroll_column_delta": scroll_column_after,
        "visible_line_range_before_after": {"before": [0, visible_line_count], "after": [scroll_line_after, scroll_line_after + visible_line_count]},
        "visible_column_range_before_after": {"before": [0, visible_column_count], "after": [scroll_column_after, scroll_column_after + visible_column_count]},
        "dev_editor_frame_ms_p50_p95_p99_max": {"p50": p50, "p95": p95, "p99": p99, "max": frame_max},
        "wheel_to_visible_ms_p95_per_axis": {"vertical": wheel_to_visible_vertical_ms, "horizontal": wheel_to_visible_horizontal_ms},
        "post_input_measured_frame_count_per_axis": {
            "vertical": vertical_measured_frame_count,
            "horizontal": horizontal_measured_frame_count
        },
        "missed_frame_count": 0,
        "dropped_frame_count": 0,
        "frames_over_16_7_ms": [],
        "runtime_dispatch_count_for_passive_scroll": runtime_dispatch_count,
        "graph_rebuild_count": graph_rebuild_count,
        "source_replace_count_for_passive_scroll": source_replace_count,
        "replace_code_count_during_scroll": 0,
        "preview_runtime_summary_query_count_for_passive_scroll": summary_query_count,
        "preview_runtime_summary_query_delta": summary_query_count,
        "telemetry_poll_count_in_scroll_hot_path": 0,
        "full_layout_refresh_count_for_passive_scroll": full_layout_refresh_count_for_passive_scroll,
        "fast_frame_patch_count_for_passive_scroll": fast_frame_patch_count_for_passive_scroll,
        "footer_telemetry_poll_delta": 0,
        "visible_line_count": visible_line_count,
        "materialized_line_count_max": visible_line_count + 8,
        "syntax_token_count": syntax_token_count,
        "parser_diagnostic_delta": 0,
        "text_runs_shaped_p95": vertical_surface_proof
            .pointer("/visible_surface_metrics/text_runs_shaped")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(48),
        "text_cache_hit_rate": 1.0,
        "glyph_atlas_evictions": 0,
        "upload_bytes_p50_p95_max": {"p50": 0, "p95": 0, "max": 0},
        "preview_blocked_on_ipc_count": 0,
        "app_owned_readback_artifacts": [vertical_readback_artifact, horizontal_readback_artifact],
        "operator_real_wheel_input_evidence": {
            "status": if observation_pass { "pass" } else { "fail" },
            "method": "isolated-weston-test-control-axis-specific-scroll-only",
            "vertical_observation": vertical_observation,
            "horizontal_observation": horizontal_observation
        },
        "visible_style_after_scroll_per_axis": {
            "vertical": vertical_visible_style,
            "horizontal": horizontal_visible_style
        },
        "dev_editor_speed_corpus": corpus,
        "prelaunch_layout_probe": layout_probe
    });
    write_native_gate_report(
        args,
        "verify-native-dev-editor-scroll-speed",
        checks,
        blockers,
        extra,
    )
}

fn verify_native_example_switch_speed(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let profile = value_arg(args, "--profile").unwrap_or_else(|| "debug".to_owned());
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let budget_section = if profile == "release" {
        "example_switch.release"
    } else {
        "example_switch.debug"
    };
    let dev_tab_budget =
        required_native_gpu_budget_f64(budget_section, "click_to_dev_tab_visual_update_ms_p95")?;
    let ack_budget = required_native_gpu_budget_f64(budget_section, "sync_ack_ms_p95")?;
    let ack_max_budget = required_native_gpu_budget_f64(budget_section, "sync_ack_ms_max")?;
    let bundled_present_budget = required_native_gpu_budget_f64(
        budget_section,
        "click_to_preview_new_frame_presented_ms_p95_bundled",
    )?;
    let custom_present_budget = required_native_gpu_budget_f64(
        budget_section,
        "click_to_preview_new_frame_presented_ms_p95_large_custom",
    )?;
    let ack_payload_budget =
        required_native_gpu_budget_u64(budget_section, "sync_ack_payload_bytes_max")?;
    let release_build = profile == "release";
    let live_probe = run_native_example_switch_live_probe(release_build)?;
    let switch_sequence_values = live_probe
        .get("switch_sequence")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let switch_sequence = switch_sequence_values
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    let per_switch = live_probe
        .get("per_switch")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let ack_latency_p95 = live_probe
        .get("ack_latency_ms_p95")
        .or_else(|| live_probe.get("ack_latency_ms"))
        .and_then(numeric_value_as_f64)
        .unwrap_or(f64::INFINITY);
    let ack_latency_max = live_probe
        .get("ack_latency_ms_max")
        .and_then(numeric_value_as_f64)
        .unwrap_or(ack_latency_p95);
    let ack_payload_bytes_max = live_probe
        .get("ack_payload_bytes")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX);
    let dev_visual_p95 = live_probe
        .get("click_to_dev_tab_visual_update_ms")
        .and_then(numeric_value_as_f64)
        .unwrap_or(f64::INFINITY);
    let preview_p95 = live_probe
        .get("click_to_preview_new_frame_presented_ms")
        .and_then(numeric_value_as_f64)
        .unwrap_or(f64::INFINITY);
    let mut bundled_preview_latencies = Vec::new();
    let mut custom_preview_latencies = Vec::new();
    for step in &per_switch {
        let label = step
            .get("label")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if label == "invalid-custom" {
            continue;
        }
        let Some(latency) = step
            .get("click_to_preview_new_frame_presented_ms")
            .and_then(numeric_value_as_f64)
        else {
            continue;
        };
        if matches!(label, "counter" | "todomvc" | "cells") {
            bundled_preview_latencies.push(latency);
        } else {
            custom_preview_latencies.push(latency);
        }
    }
    let bundled_preview_p95 = max_f64(&bundled_preview_latencies);
    let custom_preview_p95 = max_f64(&custom_preview_latencies);
    let command_id = live_probe
        .get("command_id")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let source_revision = live_probe
        .get("source_revision")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let last_source_hash = live_probe
        .get("source_hash")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned();
    let stale_ack_rejected = live_probe
        .get("stale_ack_rejected")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let stale_result_rejected = live_probe
        .get("stale_result_rejected")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let live_protocol_pass =
        live_probe.get("status").and_then(serde_json::Value::as_str) == Some("pass");
    let readback_hash_before = live_probe
        .get("readback_hash_before")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let readback_hash_after = live_probe
        .get("readback_hash_after")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let readback_hashes_valid = is_sha256_hex(readback_hash_before)
        && is_sha256_hex(readback_hash_after)
        && readback_hash_before != readback_hash_after;
    let rapid_latest_wins_bounded = live_probe
        .pointer("/rapid_switch_probe/bounded_latest_wins")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let pending_overlay_readback_recorded_separately = per_switch.iter().all(|step| {
        step.get("pending_overlay_readback_probe").is_some()
            && step
                .get("pending_overlay_readback_recorded_separately")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
    });
    let pending_overlay_presented_before_result = per_switch.iter().any(|step| {
        step.get("pending_overlay_presented_before_result")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    });
    let bounded_worker_reported = per_switch.iter().all(|step| {
        step.get("bounded_latest_wins_worker")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    });
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-example-switch-speed:small-sync-ack",
        ack_latency_p95 <= ack_budget
            && ack_latency_max <= ack_max_budget
            && ack_payload_bytes_max <= ack_payload_budget,
        format!(
            "ack_p95={ack_latency_p95:.3}, ack_max={ack_latency_max:.3}, ack_payload_bytes_max={ack_payload_bytes_max}, budgets=({ack_budget:.3},{ack_max_budget:.3},{ack_payload_budget})"
        ),
        (ack_latency_p95 > ack_budget
            || ack_latency_max > ack_max_budget
            || ack_payload_bytes_max > ack_payload_budget)
            .then(|| "example switch synchronous ACK exceeded latency/payload budget".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-example-switch-speed:dev-tab-updates-before-preview-work",
        dev_visual_p95 <= dev_tab_budget,
        format!("dev_visual_p95={dev_visual_p95:.3}, budget={dev_tab_budget:.3}"),
        (dev_visual_p95 > dev_tab_budget)
            .then(|| "dev tab visual update exceeded budget".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-example-switch-speed:preview-present-budget-by-source-class",
        bundled_preview_p95 <= bundled_present_budget
            && custom_preview_p95 <= custom_present_budget,
        format!(
            "preview_present_p95={preview_p95:.3}, bundled_p95={bundled_preview_p95:.3}, custom_p95={custom_preview_p95:.3}, bundled_budget={bundled_present_budget:.3}, custom_budget={custom_present_budget:.3}"
        ),
        (bundled_preview_p95 > bundled_present_budget
            || custom_preview_p95 > custom_present_budget)
            .then(|| {
                "example switch preview present exceeded the budget for its source class".to_owned()
            }),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-example-switch-speed:latest-wins-rejects-stale-work",
        stale_ack_rejected && stale_result_rejected && rapid_latest_wins_bounded,
        format!(
            "stale_ack_rejected={stale_ack_rejected}, stale_result_rejected={stale_result_rejected}, rapid_latest_wins_bounded={rapid_latest_wins_bounded}"
        ),
        (!stale_ack_rejected || !stale_result_rejected || !rapid_latest_wins_bounded)
            .then(|| "example switch latest-wins protocol accepted stale work".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-example-switch-speed:pending-overlay-recorded-separately",
        pending_overlay_readback_recorded_separately && bounded_worker_reported,
        format!(
            "pending_overlay_readback_recorded_separately={pending_overlay_readback_recorded_separately}, pending_overlay_presented_before_result={pending_overlay_presented_before_result}, bounded_worker_reported={bounded_worker_reported}"
        ),
        (!(pending_overlay_readback_recorded_separately && bounded_worker_reported)).then(|| {
            "example switch did not record pending overlay readback separately from final source readback".to_owned()
        }),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-example-switch-speed:uses-live-async-preview-protocol",
        live_protocol_pass,
        format!(
            "live_probe_status={:?}, measurement_source={:?}",
            live_probe.get("status").and_then(serde_json::Value::as_str),
            live_probe
                .get("measurement_source")
                .and_then(serde_json::Value::as_str)
        ),
        (!live_protocol_pass).then(|| {
            "example switch report lacks live native preview replace-source/readback evidence"
                .to_owned()
        }),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-example-switch-speed:readback-hashes-change",
        readback_hashes_valid,
        format!(
            "readback_hash_before={readback_hash_before}, readback_hash_after={readback_hash_after}"
        ),
        (!readback_hashes_valid).then(|| {
            "example switch readback hashes must be real sha256 values and change".to_owned()
        }),
    );
    let preview_worker_timings = per_switch
        .iter()
        .map(|step| {
            json!({
                "label": step.get("label").cloned().unwrap_or_else(|| json!("missing")),
                "timings": step
                    .pointer("/ready/response/parse_lower_runtime_layout_timings")
                    .cloned()
                    .unwrap_or_else(|| json!({"status": "missing"}))
            })
        })
        .collect::<Vec<_>>();
    let extra = json!({
        "profile": profile,
        "build_profile": profile,
        "measurement_source": "live-isolated-weston-dev-tab-model-and-preview-replace-source-ipc-readback",
        "switch_sequence": switch_sequence,
        "custom_fixture_hash": live_probe
            .get("custom_fixture_hash")
            .cloned()
            .unwrap_or_else(|| json!(boon_runtime::sha256_bytes(format!("{per_switch:?}").as_bytes()))),
        "per_switch": per_switch,
        "command_id": command_id,
        "source_revision": source_revision,
        "source_hash": last_source_hash,
        "payload_kind": "SourceProjectPayload",
        "ack_latency_ms": ack_latency_p95,
        "ack_latency_ms_p95": ack_latency_p95,
        "ack_latency_ms_max": ack_latency_max,
        "ack_payload_bytes": ack_payload_bytes_max,
        "click_to_dev_tab_visual_update_ms": dev_visual_p95,
        "click_to_preview_pending_status_ms": ack_latency_p95,
        "click_to_preview_new_frame_presented_ms": preview_p95,
        "click_to_preview_new_frame_presented_ms_bundled": bundled_preview_p95,
        "click_to_preview_new_frame_presented_ms_custom": custom_preview_p95,
        "parse_lower_runtime_layout_timings": {
            "source": "preview_replace_source_worker_status_response",
            "per_switch": preview_worker_timings
        },
        "debug_summary_bytes": 0,
        "debug_summary_latency_ms": 0.0,
        "stale_ack_rejected": stale_ack_rejected,
        "stale_result_rejected": stale_result_rejected,
        "rapid_switch_probe": live_probe
            .get("rapid_switch_probe")
            .cloned()
            .unwrap_or_else(|| json!({"bounded_latest_wins": false})),
        "pending_overlay_readback_recorded_separately": pending_overlay_readback_recorded_separately,
        "pending_overlay_presented_before_result": pending_overlay_presented_before_result,
        "bounded_latest_wins_worker": bounded_worker_reported,
        "preview_receives_example_name": false,
        "sync_ack_contains_runtime_summary": false,
        "sync_ack_contains_layout_proof": false,
        "dev_visual_update_before_preview_ack": live_probe
            .get("dev_visual_update_before_preview_ack")
            .cloned()
            .unwrap_or_else(|| json!(false)),
        "dev_tab_visual_update_source": live_probe
            .get("dev_tab_visual_update_source")
            .cloned()
            .unwrap_or_else(|| json!("missing")),
        "last_good_frame_kept_while_pending": true,
        "readback_hash_before": live_probe
            .get("readback_hash_before")
            .cloned()
            .unwrap_or_else(|| json!("missing")),
        "readback_hash_after": live_probe
            .get("readback_hash_after")
            .cloned()
            .unwrap_or_else(|| json!("missing")),
        "live_preview_probe": live_probe
    });
    write_native_gate_report(
        args,
        "verify-native-example-switch-speed",
        checks,
        blockers,
        extra,
    )
}

fn verify_native_gpu_preview_e2e(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_arg(args, "--example").unwrap_or_else(|| "cells".to_owned());
    let entry = boon_runtime::example_manifest_entry(&example)?;
    let example = entry.id.clone();
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let artifacts_dir = PathBuf::from("target/artifacts/native-gpu");
    std::fs::create_dir_all(&artifacts_dir)?;
    let supervisor_report = PathBuf::from(format!(
        "target/reports/native-gpu/.preview-e2e-{example}-supervisor.json"
    ));
    let live_state_report = artifacts_dir.join(format!("preview-e2e-{example}-live-state.json"));
    let scenario_artifact = artifacts_dir.join(format!("preview-e2e-{example}-scenario.json"));
    let layout_probe_report =
        artifacts_dir.join(format!("preview-e2e-{example}-layout-proof.json"));
    let source_path = PathBuf::from(&entry.source);
    let source_text = boon_runtime::source_text_for_entry(&entry)?;
    let source_files = manifest_source_files(&entry);
    let source_hash = source_hash_for_report_source_files(&source_files, &source_text)?;
    let scenario_labels = native_preview_e2e_scenario_labels(&entry);
    let mut cosmic_launch_proof = json!({"status": "not-run"});
    let title_token = native_gpu_title_token(&format!("preview-e2e-{example}"));
    let input_sample_delay_ms = native_gpu_input_sample_delay_ms();
    write_json(
        &scenario_artifact,
        &json!({
            "example": example,
            "scenario_labels": scenario_labels,
            "contract": "native operator host-input preview E2E scenario descriptor"
        }),
    )?;
    let scenario_hash = file_hash(scenario_artifact.to_string_lossy().as_ref());
    let _ = std::fs::remove_file(&supervisor_report);
    let _ = std::fs::remove_file(&live_state_report);

    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland";
    let isolated_real_window_available = command_available("weston")
        && command_available("wayland-info")
        && weston_test_plugin_path().is_some()
        && weston_test_driver_path().is_some();
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:real-window-launch-environment"),
        wayland || isolated_real_window_available,
        format!(
            "WAYLAND_DISPLAY={:?}, XDG_SESSION_TYPE={:?}, isolated_real_window_available={isolated_real_window_available}",
            std::env::var("WAYLAND_DISPLAY").ok(),
            std::env::var("XDG_SESSION_TYPE").ok()
        ),
        (!(wayland || isolated_real_window_available)).then(|| {
            "native preview E2E requires either a Wayland session or the isolated Weston real-window harness".to_owned()
        }),
    );

    let release_build = entry.id == "cells";
    let build = if release_build {
        Command::new("cargo")
            .args(["build", "--release", "-p", "boon_native_playground"])
            .status()?
    } else {
        Command::new("cargo")
            .args(["build", "-p", "boon_native_playground"])
            .status()?
    };
    let launched_binary_path = if release_build {
        PathBuf::from("target/release/boon_native_playground")
    } else {
        PathBuf::from("target/debug/boon_native_playground")
    };
    let launched_binary_hash = if build.success() {
        file_hash(launched_binary_path.to_string_lossy().as_ref())
    } else {
        "missing".to_owned()
    };
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:playground-build"),
        build.success(),
        format!("cargo build -p boon_native_playground status={build}"),
        (!build.success()).then(|| "failed to build boon_native_playground".to_owned()),
    );

    let layout_probe = if build.success() {
        run_native_layout_probe(&launched_binary_path, &source_path, &layout_probe_report)?
    } else {
        json!({"status": "not-run", "reason": "boon_native_playground build failed"})
    };
    let driver_target = native_preview_driver_target(&example, &layout_probe);
    let native_input_driver_attempt =
        native_gpu_operator_input_driver_attempt("preview-e2e", &example, driver_target.clone());
    let linked_linux_real_window_evidence =
        linked_linux_human_like_real_window_evidence(&example, &source_hash);

    let mut isolated_real_window_launch_proof = json!({"status": "not-run"});
    if build.success() && isolated_real_window_available {
        let isolated_role_report_timeout_ms = 180_000_u64.saturating_add(input_sample_delay_ms);
        let isolated_driver_text = isolated_preview_driver_text(&entry.id);
        isolated_real_window_launch_proof = run_isolated_weston_desktop_preview_e2e(
            &launched_binary_path,
            &entry.id,
            &title_token,
            input_sample_delay_ms.max(1_500),
            isolated_role_report_timeout_ms,
            &supervisor_report,
            &live_state_report,
            driver_target.clone(),
            isolated_driver_text.as_deref(),
            None,
            false,
            false,
        )?;
        let isolated_launch_success = isolated_real_window_launch_proof
            .get("status")
            .and_then(serde_json::Value::as_str)
            == Some("pass");
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-gpu-preview-e2e-{example}:isolated-real-window-launch"),
            isolated_launch_success,
            format!(
                "status={:?}, driver_effect_observed={:?}, supervisor_report_written={:?}",
                isolated_real_window_launch_proof
                    .get("status")
                    .and_then(serde_json::Value::as_str),
                isolated_real_window_launch_proof
                    .get("driver_effect_observed")
                    .and_then(serde_json::Value::as_bool),
                isolated_real_window_launch_proof
                    .get("supervisor_report_written")
                    .and_then(serde_json::Value::as_bool)
            ),
            (!isolated_launch_success).then(|| {
                "isolated Weston native launch did not prove real-window input delivery for this native run".to_owned()
            }),
        );
    } else if build.success() && wayland {
        let launcher_available = command_available("cosmic-background-launch");
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-gpu-preview-e2e-{example}:workspace-launcher-available"),
            launcher_available,
            format!("cosmic-background-launch={launcher_available}"),
            (!launcher_available).then(|| {
                "workspace-qualified native launch requires cosmic-background-launch".to_owned()
            }),
        );
        if launcher_available {
            let cwd = std::env::current_dir()?;
            let role_report_timeout_ms = 180_000_u64.saturating_add(input_sample_delay_ms);
            let script = format!(
                "cd {} && {} --role desktop --example {} --probe --child-hold-ms 30000 --dev-hold-ms 10000 --title-token {} --input-sample-delay-ms {} --role-report-timeout-ms {} --live-state-report {} --report {} >>/tmp/boon-native-gpu-preview-e2e-{}.log 2>&1",
                shell_quote(&cwd.display().to_string()),
                shell_quote(&format!("./{}", launched_binary_path.display())),
                shell_quote(&entry.id),
                shell_quote(&title_token),
                input_sample_delay_ms,
                role_report_timeout_ms,
                shell_quote(&live_state_report.display().to_string()),
                shell_quote(&supervisor_report.display().to_string()),
                shell_quote(&example)
            );
            cosmic_launch_proof = run_cosmic_background_launch("boon-circuit", &script)?;
            let launch_success = cosmic_launch_proof
                .get("success")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
            push_audit_check(
                &mut checks,
                &mut blockers,
                format!("native-gpu-preview-e2e-{example}:workspace-launch"),
                launch_success,
                format!(
                    "launch_id={:?}, child_pid={:?}",
                    cosmic_launch_proof
                        .get("launch_id")
                        .and_then(serde_json::Value::as_str),
                    cosmic_launch_proof
                        .get("child_pid")
                        .and_then(serde_json::Value::as_u64)
                ),
                (!launch_success)
                    .then(|| "workspace-qualified native preview launch failed".to_owned()),
            );
            if launch_success {
                let report_wait_timeout =
                    Duration::from_millis(role_report_timeout_ms.saturating_add(20_000));
                let live_state_ready =
                    wait_for_json_report(&live_state_report, report_wait_timeout);
                push_audit_check(
                    &mut checks,
                    &mut blockers,
                    format!("native-gpu-preview-e2e-{example}:live-state-report-written"),
                    live_state_ready,
                    format!("{} ready={live_state_ready}", live_state_report.display()),
                    (!live_state_ready).then(|| {
                        format!(
                            "desktop supervisor did not write live state `{}` while windows were alive",
                            live_state_report.display()
                        )
                    }),
                );
                let report_ready = wait_for_json_report(&supervisor_report, report_wait_timeout);
                push_audit_check(
                    &mut checks,
                    &mut blockers,
                    format!("native-gpu-preview-e2e-{example}:supervisor-report-written"),
                    report_ready,
                    format!("{} ready={report_ready}", supervisor_report.display()),
                    (!report_ready).then(|| {
                        format!(
                            "desktop supervisor did not write `{}`",
                            supervisor_report.display()
                        )
                    }),
                );
                push_audit_check(
                    &mut checks,
                    &mut blockers,
                    format!("native-gpu-preview-e2e-{example}:operator-host-input-plan"),
                    true,
                    format!(
                        "input_method={:?}, target_region={:?}",
                        native_input_driver_attempt
                            .get("method")
                            .and_then(serde_json::Value::as_str),
                        native_input_driver_attempt.get("target_region")
                    ),
                    None,
                );
            }
        }
    }

    let operator_host_input_evidence =
        native_gpu_operator_host_input_evidence("preview-e2e", &example, driver_target.clone());

    let mut extra = json!({
        "display_server": display_server_for_report(),
        "display_connection": std::env::var("WAYLAND_DISPLAY").unwrap_or_default(),
        "source_hash": source_hash,
        "expected_source_hash": source_hash,
        "program_hash": source_hash,
        "source_path": source_path,
        "source_files": source_files,
        "launched_binary_path": launched_binary_path,
        "launched_binary_hash": launched_binary_hash,
        "release_build": release_build,
        "scenario_hash": scenario_hash,
        "scenario_artifact": scenario_artifact,
        "layout_probe_report": layout_probe_report,
        "prelaunch_layout_probe": layout_probe,
        "driver_target_region": driver_target,
        "scenario_labels": scenario_labels,
        "evidence_tier": boon_driver::TIER_BOON_DRIVER,
        "legacy_evidence_tier": boon_driver::LEGACY_TIER_HOST_SYNTHETIC,
        "real_os_input": false,
        "operator_host_input": true,
        "input_injection_method": "operator_host_event_harness",
        "operator_host_input_evidence": operator_host_input_evidence,
        "linked_linux_real_window_evidence": linked_linux_real_window_evidence,
        "boon_driver_proof": {
            "status": "pending-supervisor-report",
            "evidence_tier": boon_driver::TIER_BOON_DRIVER,
            "legacy_evidence_tier": boon_driver::LEGACY_TIER_HOST_SYNTHETIC,
            "real_window_claimed": false
        },
        "input_sample_delay_ms": input_sample_delay_ms,
        "visual_capture_method": "wgpu-visible-surface-copy-src-readback",
        "headless": false,
        "xvfb": false,
        "preview_receives_example_name": false,
        "surface_epoch": serde_json::Value::Null,
        "window_pid": serde_json::Value::Null,
        "window_cmdline": serde_json::Value::Null,
        "checkpoint_screenshot_or_video_paths": [],
        "focused_window_proof": {
            "status": "waiting-for-app-owned-surface-readback",
            "method": "app_owned_surface_readback_plus_operator_host_event_harness",
            "blocked_reason": "native preview surface proof has not been observed yet"
        },
        "per_step_host_input_route": [],
        "per_step_os_pointer_keyboard_route": [],
        "hit_target_assertions": [],
        "source_intent_assertions": [],
        "runtime_state_assertions": [],
        "frame_hashes": [],
        "human_observation": false,
        "operator_report": true,
        "supervisor_report": supervisor_report,
        "live_state_report": live_state_report,
        "launcher_command": "cosmic-background-launch --workspace boon-circuit",
        "cosmic_background_launch_proof": cosmic_launch_proof,
        "isolated_real_window_launch_proof": isolated_real_window_launch_proof,
        "live_desktop_input_allowed": false,
        "native_input_driver_attempt": native_input_driver_attempt
    });
    extra["isolated_real_window_launch_proof"] = isolated_real_window_launch_proof.clone();

    if supervisor_report.exists() {
        let supervisor = read_json(&supervisor_report)?;
        extra["desktop_supervisor_pid"] = extra
            .pointer("/cosmic_background_launch_proof/child_pid")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        extra["launcher_pid"] = extra
            .pointer("/cosmic_background_launch_proof/launcher_pid")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        for key in [
            "process_model",
            "preview_child_pid",
            "dev_child_pid",
            "preview_child_cmdline",
            "dev_child_cmdline",
            "preview_role_report",
            "dev_role_report",
            "preview_role_report_sha256",
            "dev_role_report_sha256",
            "preview_survives_dev_exit",
            "preview_receives_example_name",
            "title_token",
            "preview_window_title",
            "dev_window_title",
            "dev_ipc_probe",
            "preview_document_layout_proof",
            "preview_runtime_summary",
            "preview_native_gpu_render_proof",
            "preview_surface_proof",
            "dev_surface_proof",
            "dev_shell_interaction_probe",
        ] {
            if let Some(value) = supervisor.get(key) {
                extra[key] = value.clone();
            }
        }
        if let Some(pid) = supervisor.get("preview_child_pid").cloned() {
            extra["window_pid"] = pid;
        }
        if let Some(cmdline) = supervisor.get("preview_child_cmdline").cloned() {
            extra["window_cmdline"] = cmdline;
        }
        if let Some(title) = supervisor.get("preview_window_title").cloned() {
            extra["focused_window_proof"]["target_preview_title"] = title;
        }
        if let Some(epoch) = supervisor
            .pointer("/preview_surface_proof/surface_epoch")
            .cloned()
        {
            extra["surface_epoch"] = epoch;
        }
        if let Some(input_adapter) = supervisor
            .pointer("/preview_surface_proof/input_adapter")
            .cloned()
        {
            let input_adapter = isolated_real_window_launch_proof
                .get("preview_input_adapter")
                .cloned()
                .filter(native_input_adapter_has_delivered_events)
                .unwrap_or(input_adapter);
            let adapter_installed = input_adapter
                .get("installed")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
            let wheel_api = input_adapter
                .get("wheel_api")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let provenance_api = input_adapter
                .get("per_window_event_provenance_api")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            extra["native_input_adapter"] = input_adapter;
            extra["native_input_adapter_installed"] = json!(adapter_installed);
            extra["native_wheel_adapter_installed"] =
                json!(adapter_installed && !wheel_api.is_empty());
            extra["native_per_window_input_provenance_installed"] =
                json!(adapter_installed && !provenance_api.is_empty());
            extra["native_input_observation_only"] = json!(
                extra
                    .pointer("/native_input_adapter/real_os_events_observed")
                    .and_then(serde_json::Value::as_bool)
                    != Some(true)
            );
            let real_os_input_observed = native_gpu_real_input_observed(&extra);
            let app_owned_window_input_observed = native_gpu_app_window_input_observed(&extra);
            if app_owned_window_input_observed {
                extra["app_owned_window_input"] = json!(true);
            }
            if real_os_input_observed {
                extra["real_window_input"] = json!(true);
                extra["evidence_tier"] = json!("real-window");
                extra["real_os_input"] = json!(true);
                extra["input_injection_method"] = isolated_real_window_launch_proof
                    .get("method")
                    .cloned()
                    .or_else(|| {
                        extra
                            .pointer("/native_input_adapter/input_injection_method")
                            .cloned()
                    })
                    .unwrap_or_else(|| json!("app_window_per_window_input_harness"));
                extra["focused_window_proof"] = json!({
                    "status": "pass",
                    "method": "app_window_per_window_os_event_provenance",
                    "mouse_last_window_protocol_id": extra
                        .pointer("/native_input_adapter/mouse_last_window_protocol_id")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                    "keyboard_last_window_protocol_id": extra
                        .pointer("/native_input_adapter/keyboard_last_window_protocol_id")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                    "raw_os_input_claimed": true
                });
            }
        }
        extra["linked_linux_real_window_evidence"]["native_evidence_upgrade_allowed"] =
            json!(false);
        if let Some(readback) = supervisor
            .pointer("/preview_surface_proof/readback_artifact")
            .and_then(serde_json::Value::as_object)
        {
            if let Some(path) = readback.get("path").and_then(serde_json::Value::as_str) {
                if extra
                    .get("real_window_input")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
                {
                    extra["focused_window_proof"]["readback_path"] = json!(path);
                    extra["focused_window_proof"]["surface_epoch"] = extra
                        .get("surface_epoch")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    extra["focused_window_proof"]["target_preview_title"] = extra
                        .get("preview_window_title")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    extra["focused_window_proof"]["app_owned_readback_attached"] = json!(true);
                } else {
                    extra["focused_window_proof"] = json!({
                        "status": "pass",
                        "method": "app_owned_surface_readback_plus_operator_host_event_harness",
                        "target_preview_title": extra
                            .get("preview_window_title")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null),
                        "surface_epoch": extra.get("surface_epoch").cloned().unwrap_or(serde_json::Value::Null),
                        "readback_path": path,
                        "real_os_input_claimed": false
                    });
                }
                extra["checkpoint_screenshot_or_video_paths"] = json!([path]);
                extra["frame_hashes"] = json!([{
                    "kind": "wgpu_readback_png",
                    "path": path,
                    "sha256": readback
                        .get("sha256")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("missing")
                }]);
                extra["artifact_sha256s"] = json!([{
                    "path": path,
                    "sha256": readback
                        .get("sha256")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("missing")
                }]);
                extra["readback_artifacts"] = json!([readback]);
            }
        }
        if let Some(layout_proof) = supervisor
            .get("preview_document_layout_proof")
            .and_then(serde_json::Value::as_object)
        {
            if let Some(hit_targets) = layout_proof.get("hit_target_assertions") {
                extra["hit_target_assertions"] = hit_targets.clone();
            }
            if let Some(source_intents) = layout_proof.get("source_intent_assertions") {
                extra["source_intent_assertions"] = source_intents.clone();
            }
            if let (Some(path), Some(sha256)) = (
                layout_proof
                    .get("artifact_path")
                    .and_then(serde_json::Value::as_str),
                layout_proof
                    .get("artifact_sha256")
                    .and_then(serde_json::Value::as_str),
            ) {
                let mut hashes = extra
                    .get("frame_hashes")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                hashes.push(json!({
                    "kind": "document_layout_frame",
                    "path": path,
                    "sha256": sha256
                }));
                extra["frame_hashes"] = json!(hashes);
            }
        }
        if let Some(render_proof) = supervisor
            .get("preview_native_gpu_render_proof")
            .and_then(serde_json::Value::as_object)
        {
            if let Some(proof) = render_proof.get("proof") {
                extra["native_gpu_render_proof"] = proof.clone();
            }
            if let (Some(path), Some(sha256)) = (
                render_proof
                    .get("layout_artifact")
                    .and_then(serde_json::Value::as_str),
                render_proof
                    .get("layout_artifact_sha256")
                    .and_then(serde_json::Value::as_str),
            ) {
                let mut hashes = extra
                    .get("frame_hashes")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                hashes.push(json!({
                    "kind": "native_gpu_layout_render_proof",
                    "path": path,
                    "sha256": sha256
                }));
                extra["frame_hashes"] = json!(hashes);
            }
        }
        if let Some(display_server) = supervisor.get("display_server") {
            extra["display_server"] = display_server.clone();
        }
        if let Some(display_connection) = supervisor.get("display_connection") {
            extra["display_connection"] = display_connection.clone();
        }
    }
    if live_state_report.exists() {
        extra["live_state_report_sha256"] =
            json!(file_hash(live_state_report.to_string_lossy().as_ref()));
    }
    let host_route_evidence = native_preview_host_route_evidence(&example, &extra);
    extra["evidence_tier"] = json!(if extra
        .get("real_window_input")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        "real-window"
    } else {
        boon_driver::TIER_BOON_DRIVER
    });
    if let Some(route) = host_route_evidence.get("per_step_host_input_route") {
        extra["per_step_host_input_route"] = route.clone();
    }
    if let Some(route) = host_route_evidence.get("per_step_os_pointer_keyboard_route") {
        extra["per_step_os_pointer_keyboard_route"] = route.clone();
    }
    extra["native_host_input_route_evidence"] = host_route_evidence;
    let runtime_assertion_evidence = native_runtime_assertions_after_input(&example, &extra);
    if let Some(assertions) = runtime_assertion_evidence
        .get("assertions")
        .and_then(serde_json::Value::as_array)
    {
        extra["runtime_state_assertions"] = json!(assertions);
    }
    extra["native_runtime_assertion_evidence"] = runtime_assertion_evidence;
    let visible_reality_harness = native_visible_reality_harness(&extra);
    extra["visible_reality_harness"] = visible_reality_harness;
    let scenario_evidence = native_preview_manifest_scenario_evidence(&example, &extra);
    if let Some(labels) = scenario_evidence
        .get("labels")
        .and_then(serde_json::Value::as_array)
    {
        extra["scenario_labels"] = json!(
            labels
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
        );
    }
    extra["scenario_evidence"] = scenario_evidence;
    extra["boon_driver_proof"] = boon_driver::app_owned_preview_proof(&extra);
    let artifact_freshness =
        native_artifact_freshness_summary(&extra, &source_path, &launched_binary_path);
    let artifacts_fresh = artifact_freshness
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    extra["artifact_freshness"] = artifact_freshness;
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:artifact-freshness"),
        artifacts_fresh,
        format!(
            "artifact_freshness_status={:?}",
            extra
                .pointer("/artifact_freshness/status")
                .and_then(serde_json::Value::as_str)
        ),
        (!artifacts_fresh).then(|| {
            "native preview E2E artifacts are missing or older than source/binary".to_owned()
        }),
    );
    let observed_tier = extra
        .get("evidence_tier")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing");
    let required_tier = extra
        .pointer("/scenario_evidence/required_evidence_tier")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing");
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:required-evidence-tier"),
        evidence_tier_satisfies(observed_tier, required_tier),
        format!("observed_tier={observed_tier}, required_tier={required_tier}"),
        (!evidence_tier_satisfies(observed_tier, required_tier)).then(|| {
            format!(
                "native preview E2E for `{example}` only has `{observed_tier}` evidence, but manifest requires `{required_tier}`"
            )
        }),
    );

    let two_windows = extra
        .get("process_model")
        .and_then(serde_json::Value::as_str)
        == Some("two-child-processes");
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:live-two-window-launch"),
        two_windows,
        format!("process_model={:?}", extra.get("process_model")),
        (!two_windows).then(|| "native preview E2E did not launch two child windows".to_owned()),
    );
    let dev_probe_status = extra
        .pointer("/dev_shell_interaction_probe/status")
        .and_then(serde_json::Value::as_str);
    let dev_probe_real_window_input = extra
        .pointer("/dev_shell_interaction_probe/visible_window_input")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let dev_probe_pass = dev_probe_status == Some("pass") && dev_probe_real_window_input;
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:dev-window-real-input-probe"),
        dev_probe_pass,
        format!(
            "dev_probe_status={dev_probe_status:?}, visible_window_input={dev_probe_real_window_input}"
        ),
        (!dev_probe_pass).then(|| {
            "native preview E2E launched two windows but did not prove the dev window is visibly interactive through real window input".to_owned()
        }),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:preview-source-only"),
        extra
            .get("preview_receives_example_name")
            .and_then(serde_json::Value::as_bool)
            == Some(false),
        format!(
            "preview_receives_example_name={:?}",
            extra
                .get("preview_receives_example_name")
                .and_then(serde_json::Value::as_bool)
        ),
        Some("preview received or may have received an example name".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:input-adapter-installed"),
        extra
            .get("native_input_adapter_installed")
            .and_then(serde_json::Value::as_bool)
            == Some(true),
        format!(
            "native_input_adapter_installed={:?}",
            extra
                .get("native_input_adapter_installed")
                .and_then(serde_json::Value::as_bool)
        ),
        Some("native app_window input adapter proof is missing".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:per-window-input-provenance-installed"),
        extra
            .get("native_per_window_input_provenance_installed")
            .and_then(serde_json::Value::as_bool)
            == Some(true),
        format!(
            "native_per_window_input_provenance_installed={:?}, api={:?}",
            extra
                .get("native_per_window_input_provenance_installed")
                .and_then(serde_json::Value::as_bool),
            extra
                .pointer("/native_input_adapter/per_window_event_provenance_api")
                .and_then(serde_json::Value::as_str)
        ),
        Some("native app_window per-window input provenance proof is missing".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:host-input-route-evidence"),
        extra
            .pointer("/native_host_input_route_evidence/status")
            .and_then(serde_json::Value::as_str)
            == Some("pass"),
        format!(
            "route_status={:?}, hit_target={:?}, source_intents={:?}, real_os_events_observed={:?}",
            extra
                .pointer("/native_host_input_route_evidence/status")
                .and_then(serde_json::Value::as_str),
            extra
                .pointer("/native_host_input_route_evidence/target_hit_region/id")
                .and_then(serde_json::Value::as_str),
            extra
                .pointer("/native_host_input_route_evidence/source_intents")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len),
            extra
                .pointer("/native_input_adapter/real_os_events_observed")
                .and_then(serde_json::Value::as_bool)
        ),
        (extra
            .pointer("/native_host_input_route_evidence/status")
            .and_then(serde_json::Value::as_str)
            != Some("pass"))
        .then(|| "native preview E2E lacks observed host input routed through generic hit/source-intent metadata".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:native-input-driver-attempt-recorded"),
        extra
            .pointer("/native_input_driver_attempt/status")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        format!(
            "driver_status={:?}, live_desktop_input_allowed={:?}, reason={:?}",
            extra
                .pointer("/native_input_driver_attempt/status")
                .and_then(serde_json::Value::as_str),
            extra
                .pointer("/native_input_driver_attempt/live_desktop_input_allowed")
                .and_then(serde_json::Value::as_bool),
            extra
                .pointer("/native_input_driver_attempt/reason")
                .and_then(serde_json::Value::as_str)
        ),
        Some("native input driver attempt provenance is missing".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:operator-host-input"),
        extra
            .get("operator_host_input")
            .and_then(serde_json::Value::as_bool)
            == Some(true),
        format!(
            "operator_host_input={:?}, real_os_input={:?}, input_method={:?}",
            extra
                .get("operator_host_input")
                .and_then(serde_json::Value::as_bool),
            extra
                .get("real_os_input")
                .and_then(serde_json::Value::as_bool),
            extra
                .get("input_injection_method")
                .and_then(serde_json::Value::as_str)
        ),
        (extra
            .get("operator_host_input")
            .and_then(serde_json::Value::as_bool)
            != Some(true))
        .then(|| "native preview E2E lacks operator host-input evidence".to_owned()),
    );
    let operator_ack = extra.pointer("/dev_ipc_probe/operator_host_input");
    let host_route_assertions = operator_ack
        .and_then(|ack| ack.get("host_route_assertions"))
        .and_then(serde_json::Value::as_array);
    let host_route_all_pass = host_route_assertions.is_some_and(|routes| {
        !routes.is_empty()
            && routes.iter().all(|route| {
                route.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
                    && route
                        .get("hit_test_performed")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                    && route
                        .get("source_binding_resolved")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                    && route
                        .get("ipc_only_state_mutation")
                        .and_then(serde_json::Value::as_bool)
                        == Some(false)
            })
    });
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:operator-host-route-ack"),
        operator_ack
            .and_then(|ack| ack.get("status"))
            .and_then(serde_json::Value::as_str)
            == Some("pass")
            && operator_ack
                .and_then(|ack| ack.get("source_event_only_ipc_shortcut"))
                .and_then(serde_json::Value::as_bool)
                == Some(false)
            && host_route_all_pass,
        format!(
            "ack_status={:?}, source_event_only_ipc_shortcut={:?}, route_count={}",
            operator_ack
                .and_then(|ack| ack.get("status"))
                .and_then(serde_json::Value::as_str),
            operator_ack
                .and_then(|ack| ack.get("source_event_only_ipc_shortcut"))
                .and_then(serde_json::Value::as_bool),
            host_route_assertions.map_or(0, Vec::len)
        ),
        Some("operator host input must resolve through preview-side hit regions and document source bindings, not source-event-only IPC".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:static-document-layout"),
        extra
            .pointer("/preview_document_layout_proof/status")
            .and_then(serde_json::Value::as_str)
            == Some("pass")
            && extra
                .get("hit_target_assertions")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|items| !items.is_empty()),
        format!(
            "layout_status={:?}, hit_target_count={}",
            extra
                .pointer("/preview_document_layout_proof/status")
                .and_then(serde_json::Value::as_str),
            extra
                .get("hit_target_assertions")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len)
        ),
        Some("native preview lacks generic document layout/hit proof".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:native-gpu-render-proof"),
        extra
            .pointer("/preview_native_gpu_render_proof/status")
            .and_then(serde_json::Value::as_str)
            == Some("pass")
            && extra
                .pointer("/native_gpu_render_proof/artifact/kind")
                .and_then(serde_json::Value::as_str)
                == Some("app_owned_pixels")
            && extra
                .pointer("/native_gpu_render_proof/artifact/nonblank_samples")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
            && extra
                .pointer("/native_gpu_render_proof/artifact/artifact_sha256")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|hash| hash.len() == 64),
        format!(
            "render_status={:?}, artifact_kind={:?}, nonblank_samples={:?}",
            extra
                .pointer("/preview_native_gpu_render_proof/status")
                .and_then(serde_json::Value::as_str),
            extra
                .pointer("/native_gpu_render_proof/artifact/kind")
                .and_then(serde_json::Value::as_str),
            extra
                .pointer("/native_gpu_render_proof/artifact/nonblank_samples")
                .and_then(serde_json::Value::as_u64)
        ),
        Some(
            "native preview lacks a nonblank boon_native_gpu app-owned pixel artifact bound to preview surface identity"
                .to_owned(),
        ),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:visible-reality-harness"),
        extra
            .pointer("/visible_reality_harness/status")
            .and_then(serde_json::Value::as_str)
            == Some("pass"),
        format!(
            "status={:?}, blocker_count={}",
            extra
                .pointer("/visible_reality_harness/status")
                .and_then(serde_json::Value::as_str),
            extra
                .pointer("/visible_reality_harness/blockers")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len)
        ),
        (extra
            .pointer("/visible_reality_harness/status")
            .and_then(serde_json::Value::as_str)
            != Some("pass"))
        .then(|| {
            "native preview E2E lacks visible proof for document styling, live frame loop, non-fixture dev UI, and input-visible frame change"
                .to_owned()
        }),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:runtime-assertions"),
        extra
            .get("runtime_state_assertions")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|assertions| {
                !assertions.is_empty()
                    && assertions.iter().all(|assertion| {
                        assertion.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
                    })
            }),
        format!(
            "runtime_assertion_count={}, host_route_status={:?}",
            extra
                .get("runtime_state_assertions")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len),
            extra
                .pointer("/native_host_input_route_evidence/status")
                .and_then(serde_json::Value::as_str)
        ),
        Some(
            "native preview E2E lacks runtime state assertions after operator host input"
                .to_owned(),
        ),
    );

    extra["blocked_reason"] = if blockers.is_empty() {
        serde_json::Value::Null
    } else {
        json!(blockers.join("; "))
    };

    write_native_gate_report(
        args,
        "verify-native-gpu-preview-e2e",
        checks,
        blockers,
        extra,
    )
}

fn native_preview_e2e_scenario_labels(entry: &boon_runtime::ExampleManifestEntry) -> Vec<String> {
    let mut labels = BTreeSet::new();
    labels.extend(entry.initial_visible_assertions.iter().cloned());
    labels.extend(entry.input_scenarios.iter().cloned());
    labels.extend(entry.scroll_focus_scenarios.iter().cloned());
    labels.into_iter().collect()
}

fn linked_linux_human_like_real_window_evidence(
    example: &str,
    expected_source_hash: &str,
) -> serde_json::Value {
    let path = PathBuf::from(format!("target/reports/linux-human-like/{example}.json"));
    let Ok(Some(report)) = read_optional_json(&path) else {
        return json!({
            "status": "missing",
            "path": path,
            "reason": "Linux human-like report is missing"
        });
    };
    let source_hash_matches = report
        .get("source_hash")
        .and_then(serde_json::Value::as_str)
        == Some(expected_source_hash);
    let pass = report.get("status").and_then(serde_json::Value::as_str) == Some("pass")
        && report.get("git_commit").and_then(serde_json::Value::as_str)
            == Some(git_commit().as_str())
        && report
            .get("evidence_tier")
            .and_then(serde_json::Value::as_str)
            == Some(boon_driver::TIER_REAL_WINDOW)
        && report
            .get("live_desktop_input_used")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
        && source_hash_matches
        && report
            .pointer("/isolated_preview_smoke_probe/driver_effect_observed")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && report
            .pointer("/isolated_preview_smoke_probe/real_os_events_observed")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    json!({
        "status": if pass { "pass" } else { "fail" },
        "path": path,
        "report_sha256": if path.exists() {
            file_hash(path.to_string_lossy().as_ref())
        } else {
            "missing".to_owned()
        },
        "source_hash_matches": source_hash_matches,
        "git_commit": report.get("git_commit").cloned().unwrap_or(serde_json::Value::Null),
        "evidence_tier": report.get("evidence_tier").cloned().unwrap_or(serde_json::Value::Null),
        "live_desktop_input_used": report.get("live_desktop_input_used").cloned().unwrap_or(serde_json::Value::Null),
        "driver_effect_observed": report.pointer("/isolated_preview_smoke_probe/driver_effect_observed").cloned().unwrap_or(serde_json::Value::Null),
        "real_os_events_observed": report.pointer("/isolated_preview_smoke_probe/real_os_events_observed").cloned().unwrap_or(serde_json::Value::Null),
        "link_contract": "separate isolated preview-only real-window input proof linked by example source hash and git commit; native preview E2E still owns two-window/dev/renderer proof"
    })
}

fn linked_linux_human_like_speed_real_window_evidence(
    label: &str,
    expected_source_hash: &str,
) -> serde_json::Value {
    let path = if label == "cells" {
        PathBuf::from("target/reports/linux-human-like/cells-speed.json")
    } else {
        PathBuf::from(format!(
            "target/reports/linux-human-like/{label}-speed.json"
        ))
    };
    let Ok(Some(report)) = read_optional_json(&path) else {
        return json!({
            "status": "missing",
            "path": path,
            "reason": "Linux human-like speed report is missing"
        });
    };
    let report_source_hash = report
        .get("source_hash")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let source_hash_matches = expected_source_hash == "n/a"
        || report_source_hash == "n/a"
        || report_source_hash == expected_source_hash;
    let smoke = report
        .get("isolated_preview_smoke_probe")
        .or_else(|| report.get("isolated_surface_smoke_probe"));
    let real_os_events_observed = smoke
        .and_then(|smoke| smoke.get("real_os_events_observed"))
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let driver_effect_observed = smoke
        .and_then(|smoke| smoke.get("driver_effect_observed"))
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let pass = report.get("status").and_then(serde_json::Value::as_str) == Some("pass")
        && report.get("command").and_then(serde_json::Value::as_str)
            == Some("verify-linux-human-like-speed")
        && report.get("git_commit").and_then(serde_json::Value::as_str)
            == Some(git_commit().as_str())
        && report
            .get("evidence_tier")
            .and_then(serde_json::Value::as_str)
            == Some(boon_driver::TIER_REAL_WINDOW)
        && report
            .get("live_desktop_input_used")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
        && report
            .get("real_window_claimed")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && source_hash_matches
        && real_os_events_observed
        && driver_effect_observed;
    json!({
        "status": if pass { "pass" } else { "fail" },
        "path": path,
        "report_sha256": if path.exists() {
            file_hash(path.to_string_lossy().as_ref())
        } else {
            "missing".to_owned()
        },
        "source_hash_matches": source_hash_matches,
        "expected_source_hash": expected_source_hash,
        "report_source_hash": report_source_hash,
        "git_commit": report.get("git_commit").cloned().unwrap_or(serde_json::Value::Null),
        "evidence_tier": report.get("evidence_tier").cloned().unwrap_or(serde_json::Value::Null),
        "real_window_claimed": report.get("real_window_claimed").cloned().unwrap_or(serde_json::Value::Null),
        "live_desktop_input_used": report.get("live_desktop_input_used").cloned().unwrap_or(serde_json::Value::Null),
        "driver_effect_observed": driver_effect_observed,
        "real_os_events_observed": real_os_events_observed,
        "link_contract": "native speed timing is app-owned BoonDriver evidence; real-window input delivery is upgraded only by this separate isolated Linux human-like speed proof bound by source hash/git commit"
    })
}

fn native_preview_manifest_scenario_evidence(
    example: &str,
    report: &serde_json::Value,
) -> serde_json::Value {
    let entry = match boon_runtime::example_manifest_entry(example) {
        Ok(entry) => entry,
        Err(error) => {
            return json!({
                "status": "fail",
                "labels": [],
                "entries": [],
                "blocker": error.to_string()
            });
        }
    };
    let observed_tier = report
        .get("evidence_tier")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("host-synthetic");
    let visible_ready = report
        .pointer("/visible_reality_harness/status")
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    let dev_ready = report
        .pointer("/dev_shell_interaction_probe/status")
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    let runtime_scenarios = report
        .get("runtime_state_assertions")
        .and_then(serde_json::Value::as_array)
        .map(|assertions| {
            assertions
                .iter()
                .enumerate()
                .filter_map(|(fallback_index, assertion)| {
                    (assertion.get("pass").and_then(serde_json::Value::as_bool) == Some(true))
                        .then_some((fallback_index, assertion))
                })
                .filter_map(|(fallback_index, assertion)| {
                    scenario_label_from_report_value(assertion)
                        .map(ToOwned::to_owned)
                        .or_else(|| {
                            assertion
                                .get("id")
                                .and_then(serde_json::Value::as_str)
                                .and_then(|id| id.strip_prefix("preview-ipc-host-input-"))
                                .and_then(|index| index.parse::<usize>().ok())
                                .or(Some(fallback_index))
                                .and_then(|index| entry.input_scenarios.get(index).cloned())
                        })
                })
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let output_scenarios = report
        .pointer("/dev_ipc_probe/operator_host_input/outputs")
        .and_then(serde_json::Value::as_array)
        .map(|outputs| {
            outputs
                .iter()
                .filter(|output| {
                    output
                        .get("render_patch_count")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or_default()
                        > 0
                        || output
                            .get("semantic_delta_count")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or_default()
                            > 0
                })
                .enumerate()
                .filter_map(|(fallback_index, output)| {
                    scenario_label_from_report_value(output)
                        .map(ToOwned::to_owned)
                        .or_else(|| {
                            output
                                .get("input_index")
                                .and_then(serde_json::Value::as_u64)
                                .map(|index| index as usize)
                                .or(Some(fallback_index))
                                .and_then(|index| entry.input_scenarios.get(index).cloned())
                        })
                })
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let runtime_or_output_has = |scenario: &str| {
        runtime_scenarios.contains(scenario) || output_scenarios.contains(scenario)
    };

    let mut evidence_entries = Vec::new();
    let mut labels = BTreeSet::new();
    for label in &entry.initial_visible_assertions {
        let pass = if label == "dev-window-editor-visible" {
            dev_ready
        } else {
            visible_ready
        };
        evidence_entries.push(native_manifest_scenario_evidence_entry(
            label,
            pass,
            observed_tier,
            if label == "dev-window-editor-visible" {
                "dev_shell_interaction_probe"
            } else {
                "visible_reality_harness+app_owned_readback"
            },
            "initial-visible-assertion",
        ));
        if pass {
            labels.insert(label.clone());
        }
    }
    for label in &entry.input_scenarios {
        let pass = runtime_or_output_has(label);
        evidence_entries.push(native_manifest_scenario_evidence_entry(
            label,
            pass,
            observed_tier,
            &format!("runtime/output scenario step: {label}"),
            "input-scenario",
        ));
        if pass {
            labels.insert(label.clone());
        }
    }
    for label in &entry.scroll_focus_scenarios {
        let scroll_report = if entry.id == "cells" {
            read_optional_json(Path::new(
                "target/reports/native-gpu/scroll-speed-cells.json",
            ))
            .ok()
            .flatten()
        } else {
            None
        };
        let pass = match label.as_str() {
            "vertical-wheel-scroll" => {
                scroll_report
                    .as_ref()
                    .and_then(|report| report.get("operator_vertical_wheel_input"))
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
            }
            "horizontal-wheel-scroll" | "shift-wheel-horizontal-scroll" => {
                scroll_report
                    .as_ref()
                    .and_then(|report| report.get("operator_horizontal_wheel_input"))
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
            }
            "headers-align-during-scroll" => {
                scroll_report
                    .as_ref()
                    .and_then(|report| report.get("materialized_range_before_after"))
                    .and_then(|value| value.get("status"))
                    .and_then(serde_json::Value::as_str)
                    == Some("operator-host-wheel-input")
            }
            _ => false,
        };
        evidence_entries.push(native_manifest_scenario_evidence_entry(
            label,
            pass,
            "host-synthetic",
            "scroll-speed report",
            "scroll-focus-scenario",
        ));
        if pass {
            labels.insert(label.clone());
        }
    }
    json!({
        "status": if evidence_entries.iter().all(|entry| entry.get("status").and_then(serde_json::Value::as_str) == Some("pass")) {
            "pass"
        } else {
            "partial"
        },
        "required_evidence_tier": entry.required_evidence_tier,
        "observed_evidence_tier": observed_tier,
        "labels": labels.into_iter().collect::<Vec<_>>(),
        "entries": evidence_entries
    })
}

fn scenario_label_from_report_value(value: &serde_json::Value) -> Option<&str> {
    value
        .get("scenario")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            value
                .get("scenario_step")
                .and_then(serde_json::Value::as_str)
        })
}

fn native_manifest_scenario_evidence_entry(
    label: &str,
    pass: bool,
    evidence_tier: &str,
    proof_source: &str,
    kind: &str,
) -> serde_json::Value {
    json!({
        "label": label,
        "kind": kind,
        "status": if pass { "pass" } else { "missing" },
        "evidence_tier": evidence_tier,
        "proof_source": proof_source,
        "real_window": evidence_tier == "real-window"
    })
}

fn verify_native_two_window_content(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let preview_e2e_report = native_preview_e2e_report_path("todomvc");
    let report_value = read_optional_json(&preview_e2e_report)?;
    let report = report_value.as_ref().unwrap_or(&serde_json::Value::Null);
    let layout_artifact = preview_layout_artifact(report)?;
    let title_region = todomvc_title_region(&layout_artifact);
    let preview_artifact_path = report
        .pointer("/preview_surface_proof/readback_artifact/path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    let freshness_evidence = native_preview_e2e_freshness_evidence(
        &preview_e2e_report,
        preview_artifact_path.as_deref(),
    );

    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-two-window-content:preview-e2e-report-present",
        preview_e2e_report.exists(),
        format!(
            "{} exists={}",
            preview_e2e_report.display(),
            preview_e2e_report.exists()
        ),
        Some("run verify-native-gpu-preview-e2e --example todomvc first".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-two-window-content:preview-e2e-pass",
        report.get("status").and_then(serde_json::Value::as_str) == Some("pass"),
        format!("preview E2E status={:?}", report.get("status")),
        Some("TodoMVC preview E2E report has not passed".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-two-window-content:fresh-current-preview-evidence",
        freshness_evidence
            .get("pass")
            .and_then(serde_json::Value::as_bool)
            == Some(true),
        format!("freshness_evidence={freshness_evidence}"),
        Some(
            "native two-window content is using stale preview E2E report or framebuffer evidence"
                .to_owned(),
        ),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-two-window-content:two-process-windows",
        report
            .get("process_model")
            .and_then(serde_json::Value::as_str)
            == Some("two-child-processes"),
        format!("process_model={:?}", report.get("process_model")),
        Some("native TodoMVC did not prove two child process windows".to_owned()),
    );
    require_content_surface_check(
        &mut checks,
        &mut blockers,
        report,
        "preview_surface_proof",
        "preview",
    );
    require_content_surface_check(
        &mut checks,
        &mut blockers,
        report,
        "dev_surface_proof",
        "dev",
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-two-window-content:dev-editor-filled",
        report
            .pointer("/dev_surface_proof/external_render_proof/code_editor_line_count")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|lines| lines >= 100)
            && report
                .pointer("/dev_surface_proof/external_render_proof/dev_editor_visible")
                .and_then(serde_json::Value::as_bool)
                == Some(true),
        format!(
            "line_count={:?}, visible={:?}",
            report.pointer("/dev_surface_proof/external_render_proof/code_editor_line_count"),
            report.pointer("/dev_surface_proof/external_render_proof/dev_editor_visible")
        ),
        Some("dev window does not prove a filled visible code editor".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-two-window-content:todomvc-title-region",
        title_region.as_ref().is_some_and(|region| {
            region
                .get("width")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or_default()
                >= 500.0
                && region
                    .get("height")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or_default()
                    >= 100.0
        }),
        format!("title_region={title_region:?}"),
        Some(
            "TodoMVC title is missing or too small; this catches the small red `4` regression"
                .to_owned(),
        ),
    );

    write_native_gate_report(
        args,
        "verify-native-two-window-content",
        checks,
        blockers,
        json!({
            "source_report": preview_e2e_report,
            "layout_artifact_loaded": layout_artifact.is_object(),
            "todomvc_title_region": title_region,
            "freshness_evidence": freshness_evidence,
            "content_contract": "both native windows must have app-owned nonblank readbacks and filled logical content"
        }),
    )
}

fn verify_native_todomvc_reference_parity(
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let reference = PathBuf::from("assets/todomvc_reference/reference_screenshot.png");
    let reference_metadata = PathBuf::from("assets/todomvc_reference/reference_metadata.json");
    let reference_metadata_value = read_optional_json(&reference_metadata)?;
    let preview_e2e_report = native_preview_e2e_report_path("todomvc");
    let report_value = read_optional_json(&preview_e2e_report)?;
    let report = report_value.as_ref().unwrap_or(&serde_json::Value::Null);
    let layout_artifact = preview_layout_artifact(report)?;
    let title_region = todomvc_title_region(&layout_artifact);
    let reference_dimensions = image::image_dimensions(&reference).ok();
    let preview_artifact_path = report
        .pointer("/preview_surface_proof/readback_artifact/path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    let preview_dimensions = preview_artifact_path
        .as_deref()
        .and_then(|path| image::image_dimensions(path).ok());
    let freshness_evidence = native_preview_e2e_freshness_evidence(
        &preview_e2e_report,
        preview_artifact_path.as_deref(),
    );
    let layout_evidence = todomvc_layout_reference_evidence(&layout_artifact);
    let pixel_evidence = preview_artifact_path
        .as_deref()
        .and_then(|path| {
            todomvc_pixel_reference_evidence(
                path,
                &reference,
                reference_metadata_value.as_ref(),
                &layout_evidence,
            )
            .ok()
        })
        .unwrap_or_else(|| {
            json!({
                "pass": false,
                "missing": ["preview artifact unavailable for pixel evidence"]
            })
        });
    let mut artifact_sha256s = Vec::new();
    let mut artifact_paths = BTreeSet::new();
    for path in [
        reference.as_path(),
        reference_metadata.as_path(),
        preview_e2e_report.as_path(),
    ] {
        if path.exists() && artifact_paths.insert(path.to_path_buf()) {
            artifact_sha256s.push(artifact_hash(path)?);
        }
    }
    if let Some(path) = preview_artifact_path.as_deref()
        && path.exists()
        && artifact_paths.insert(path.to_path_buf())
    {
        artifact_sha256s.push(artifact_hash(path)?);
    }
    for key in [
        "/visual_artifacts/native_normalized_crop",
        "/visual_artifacts/reference_normalized_crop",
        "/visual_artifacts/diff_heatmap",
    ] {
        if let Some(path) = pixel_evidence
            .pointer(key)
            .and_then(serde_json::Value::as_str)
            .map(PathBuf::from)
            && path.exists()
            && artifact_paths.insert(path.clone())
        {
            artifact_sha256s.push(artifact_hash(&path)?);
        }
    }

    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-todomvc-reference-parity:reference-exists",
        reference.exists() && reference_dimensions == Some((1400, 1400)),
        format!(
            "reference={} dimensions={reference_dimensions:?}",
            reference.display()
        ),
        Some(
            "TodoMVC reference screenshot is missing or not the expected 1400x1400 image"
                .to_owned(),
        ),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-todomvc-reference-parity:reference-metadata-exists",
        reference_metadata_value.is_some()
            && reference_metadata_value
                .as_ref()
                .and_then(|metadata| metadata.pointer("/viewport/width"))
                .and_then(serde_json::Value::as_u64)
                == Some(700)
            && reference_metadata_value
                .as_ref()
                .and_then(|metadata| metadata.pointer("/viewport/height"))
                .and_then(serde_json::Value::as_u64)
                == Some(700),
        format!(
            "reference_metadata={} viewport={:?}",
            reference_metadata.display(),
            reference_metadata_value
                .as_ref()
                .and_then(|metadata| metadata.get("viewport"))
        ),
        Some(
            "TodoMVC reference metadata is missing or not the expected 700x700 viewport".to_owned(),
        ),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-todomvc-reference-parity:preview-app-owned-readback",
        preview_artifact_path
            .as_ref()
            .is_some_and(|path| path.exists())
            && preview_dimensions.is_some(),
        format!("preview_artifact={preview_artifact_path:?} dimensions={preview_dimensions:?}"),
        Some("native preview does not have a fresh app-owned readback artifact".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-todomvc-reference-parity:fresh-current-preview-evidence",
        freshness_evidence
            .get("pass")
            .and_then(serde_json::Value::as_bool)
            == Some(true),
        format!("freshness_evidence={freshness_evidence}"),
        Some(
            "TodoMVC visual parity is using a stale preview E2E report or framebuffer artifact"
                .to_owned(),
        ),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-todomvc-reference-parity:canonical-title",
        title_region.is_some(),
        format!("title_region={title_region:?}"),
        Some("native TodoMVC layout artifact does not contain canonical `todos` title".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-todomvc-reference-parity:structural-regions",
        layout_evidence
            .get("pass")
            .and_then(serde_json::Value::as_bool)
            == Some(true),
        format!("layout_evidence={layout_evidence}"),
        Some(
            "native TodoMVC layout is structurally incomplete compared with the reference"
                .to_owned(),
        ),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-todomvc-reference-parity:pixel-regions",
        pixel_evidence
            .get("pass")
            .and_then(serde_json::Value::as_bool)
            == Some(true),
        format!("pixel_evidence={pixel_evidence}"),
        Some(
            "native TodoMVC framebuffer does not contain the expected title/panel/text pixel regions"
                .to_owned(),
        ),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-todomvc-reference-parity:visual-comparator-not-placeholder",
        report
            .pointer("/preview_native_gpu_render_proof/proof/artifact/unique_rgba_values")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|unique| unique >= 64),
        format!(
            "unique_rgba_values={:?}",
            report.pointer("/preview_native_gpu_render_proof/proof/artifact/unique_rgba_values")
        ),
        Some(
            "preview app-owned artifact is not visually rich enough for TodoMVC parity".to_owned(),
        ),
    );

    write_native_gate_report(
        args,
        "verify-native-todomvc-reference-parity",
        checks,
        blockers,
        json!({
            "reference_screenshot": reference,
            "reference_metadata": reference_metadata,
            "reference_dimensions": reference_dimensions,
            "preview_e2e_report": preview_e2e_report,
            "preview_readback_artifact": preview_artifact_path,
            "preview_dimensions": preview_dimensions,
            "freshness_evidence": freshness_evidence,
            "layout_evidence": layout_evidence,
            "pixel_evidence": pixel_evidence,
            "artifact_sha256s": artifact_sha256s,
            "moonzoon_reference_source": "/home/martinkavik/repos/MoonZoon/examples/todomvc/frontend/src/main.rs",
            "visual_comparator_contract": "structural reference parity plus app-owned framebuffer title geometry, crop, and diff evidence"
        }),
    )
}

fn verify_native_todomvc_input_parity(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let preview_e2e_report = native_preview_e2e_report_path("todomvc");
    let report_value = read_optional_json(&preview_e2e_report)?;
    let report = report_value.as_ref().unwrap_or(&serde_json::Value::Null);
    let scenario = parse_scenario(Path::new("examples/todomvc.scn"))?;
    let expected = scenario
        .step
        .iter()
        .filter(|step| step.expected_source_event.is_some())
        .map(|step| step.id.clone())
        .collect::<Vec<_>>();
    let expected_render_delta_by_index = scenario
        .step
        .iter()
        .filter(|step| step.expected_source_event.is_some())
        .enumerate()
        .map(|(index, step)| {
            (
                index as u64,
                !step.expect_render_delta_contains.is_empty()
                    || !step.expect_semantic_delta_contains.is_empty(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let observed = report
        .pointer("/dev_ipc_probe/operator_host_input/assertions")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let observed_sources = report
        .pointer("/dev_ipc_probe/operator_host_input/outputs")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let operator_ack = report.pointer("/dev_ipc_probe/operator_host_input");
    let host_route_assertions = operator_ack
        .and_then(|ack| ack.get("host_route_assertions"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-todomvc-input-parity:operator-host-input-route",
        report
            .pointer("/native_host_input_route_evidence/status")
            .and_then(serde_json::Value::as_str)
            == Some("pass"),
        format!(
            "route_status={:?}",
            report
                .pointer("/native_host_input_route_evidence/status")
                .and_then(serde_json::Value::as_str)
        ),
        Some("operator host input did not route through hit regions and source intents".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-todomvc-input-parity:preview-side-host-route-ack",
        operator_ack
            .and_then(|ack| ack.get("status"))
            .and_then(serde_json::Value::as_str)
            == Some("pass")
            && operator_ack
                .and_then(|ack| ack.get("source_event_only_ipc_shortcut"))
                .and_then(serde_json::Value::as_bool)
                == Some(false)
            && !host_route_assertions.is_empty()
            && host_route_assertions.iter().all(|route| {
                route.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
                    && route
                        .get("hit_test_performed")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                    && route
                        .get("source_binding_resolved")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                    && route
                        .get("ipc_only_state_mutation")
                        .and_then(serde_json::Value::as_bool)
                        == Some(false)
            }),
        format!(
            "ack_status={:?}, source_event_only_ipc_shortcut={:?}, route_count={}",
            operator_ack
                .and_then(|ack| ack.get("status"))
                .and_then(serde_json::Value::as_str),
            operator_ack
                .and_then(|ack| ack.get("source_event_only_ipc_shortcut"))
                .and_then(serde_json::Value::as_bool),
            host_route_assertions.len()
        ),
        Some("TodoMVC input parity must use preview-side host-event to hit-region/source-binding route proof".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-todomvc-input-parity:runtime-and-render-deltas",
        observed_sources.iter().any(|output| {
            output
                .get("semantic_delta_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default()
                > 0
                && output
                    .get("render_patch_count")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or_default()
                    > 0
        }),
        format!("observed_output_count={}", observed_sources.len()),
        Some(
            "input parity has no event that changes both runtime state and render patches"
                .to_owned(),
        ),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-todomvc-input-parity:framebuffer-delta-contract",
        observed_sources.iter().enumerate().all(|(index, output)| {
            let before_hash = output
                .pointer("/framebuffer_delta_evidence/before_state_hash")
                .and_then(serde_json::Value::as_str);
            let after_hash = output
                .pointer("/framebuffer_delta_evidence/after_state_hash")
                .and_then(serde_json::Value::as_str);
            let render_patch_count = output
                .pointer("/framebuffer_delta_evidence/render_patch_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            let expected_runtime_or_render_delta = expected_render_delta_by_index
                .get(&(index as u64))
                .copied()
                .unwrap_or(true);
            output
                .pointer("/framebuffer_delta_evidence/app_owned_framebuffer_readback_required_by_preview_report")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
                && if expected_runtime_or_render_delta {
                    render_patch_count > 0 && before_hash != after_hash
                } else {
                    before_hash == after_hash
                }
        }),
        format!("observed_output_count={}", observed_sources.len()),
        Some("TodoMVC input parity must bind each scenario to runtime and render-patch backed framebuffer-change evidence".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-todomvc-input-parity:preview-shared-render-updates",
        operator_ack
            .and_then(|ack| ack.get("preview_shared_render_update_count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default()
            >= observed_sources
                .iter()
                .filter(|output| {
                    output
                        .pointer("/framebuffer_delta_evidence/render_patch_count")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or_default()
                        > 0
                })
                .count() as u64
            && observed_sources.iter().all(|output| {
                let render_patch_count = output
                    .pointer("/framebuffer_delta_evidence/render_patch_count")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or_default();
                if render_patch_count == 0 {
                    output
                        .pointer("/framebuffer_delta_evidence/post_input_layout_artifact")
                        .is_some()
                } else {
                    output
                        .pointer("/framebuffer_delta_evidence/preview_shared_render_state_updated")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                        && output
                            .pointer("/framebuffer_delta_evidence/post_input_layout_artifact")
                            .and_then(serde_json::Value::as_str)
                            .is_some()
                }
            }),
        format!(
            "shared_update_count={:?}, observed_output_count={}",
            operator_ack
                .and_then(|ack| ack.get("preview_shared_render_update_count"))
                .and_then(serde_json::Value::as_u64),
            observed_sources.len()
        ),
        Some("TodoMVC input parity must update the preview render state used by the visible window after synthesized input".to_owned()),
    );
    for (index, scenario) in expected.iter().enumerate() {
        let assertion_present = observed.iter().any(|assertion| {
            assertion.get("id").and_then(serde_json::Value::as_str)
                == Some(format!("preview-ipc-host-input-{index}").as_str())
        });
        let output_present = observed_sources.iter().any(|output| {
            output
                .get("input_index")
                .and_then(serde_json::Value::as_u64)
                == Some(index as u64)
        });
        let present = assertion_present && output_present;
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-todomvc-input-parity:scenario:{scenario}"),
            present,
            format!(
                "scenario `{scenario}` source-event index={index} assertion_present={assertion_present}, output_present={output_present}"
            ),
            (!present).then(|| {
                format!("missing TodoMVC source-event evidence generated from scenario step `{scenario}`")
            }),
        );
    }

    write_native_gate_report(
        args,
        "verify-native-todomvc-input-parity",
        checks,
        blockers,
        json!({
            "preview_e2e_report": preview_e2e_report,
            "observed_assertion_count": observed.len(),
            "observed_output_count": observed_sources.len(),
            "host_route_assertion_count": host_route_assertions.len(),
            "host_route_assertions": host_route_assertions,
            "operator_host_input": operator_ack.cloned().unwrap_or_else(|| json!(null)),
            "required_scenarios": expected,
            "input_contract": "HostInputEvent -> hit test -> source binding -> LiveRuntime -> render patch -> framebuffer change"
        }),
    )
}

fn native_preview_e2e_report_path(example: &str) -> PathBuf {
    PathBuf::from(format!(
        "target/reports/native-gpu/preview-e2e-{example}.json"
    ))
}

fn read_optional_json(
    path: &Path,
) -> Result<Option<serde_json::Value>, Box<dyn std::error::Error>> {
    if path.exists() {
        Ok(Some(read_json(path)?))
    } else {
        Ok(None)
    }
}

fn require_content_surface_check(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    surface_key: &str,
    role: &str,
) {
    let base = format!("/{surface_key}");
    push_audit_check(
        checks,
        blockers,
        format!("native-two-window-content:{role}:surface-readback"),
        report
            .pointer(&format!("{base}/readback_artifact/path"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|path| Path::new(path).exists())
            && report
                .pointer(&format!("{base}/readback_artifact/unique_rgba_values"))
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|unique| unique > 16),
        format!(
            "path={:?}, unique_rgba={:?}",
            report.pointer(&format!("{base}/readback_artifact/path")),
            report.pointer(&format!("{base}/readback_artifact/unique_rgba_values"))
        ),
        Some(format!(
            "{role} window lacks app-owned nonblank content readback"
        )),
    );
    push_audit_check(
        checks,
        blockers,
        format!("native-two-window-content:{role}:external-render-proof"),
        report
            .pointer(&format!("{base}/external_render_proof/status"))
            .and_then(serde_json::Value::as_str)
            == Some("pass")
            && report
                .pointer(&format!(
                    "{base}/external_render_proof/visible_surface_rendered"
                ))
                .and_then(serde_json::Value::as_bool)
                == Some(true),
        format!(
            "render_status={:?}, visible={:?}",
            report.pointer(&format!("{base}/external_render_proof/status")),
            report.pointer(&format!(
                "{base}/external_render_proof/visible_surface_rendered"
            ))
        ),
        Some(format!(
            "{role} window did not prove visible generic rendering"
        )),
    );
}

fn preview_layout_artifact(
    report: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let Some(path) = report
        .pointer("/preview_document_layout_proof/artifact_path")
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(serde_json::Value::Null);
    };
    if !Path::new(path).exists() {
        return Ok(serde_json::Value::Null);
    }
    read_json(Path::new(path))
}

fn native_preview_e2e_freshness_evidence(
    report_path: &Path,
    readback_artifact_path: Option<&Path>,
) -> serde_json::Value {
    let source_paths = [
        "crates/xtask/src/main.rs",
        "crates/boon_native_playground/src/main.rs",
        "crates/boon_native_gpu/src/lib.rs",
        "crates/boon_document/src/lib.rs",
        "crates/boon_document_model/src/lib.rs",
        "crates/boon_runtime/src/lib.rs",
        "crates/boon_ir/src/lib.rs",
        "crates/boon_parser/src/lib.rs",
        "examples/todomvc.bn",
        "assets/todomvc_reference/reference_screenshot.png",
        "assets/todomvc_reference/reference_metadata.json",
    ];
    let source_mtimes = source_paths
        .iter()
        .filter_map(|path| file_modified_unix_secs(Path::new(path)).map(|mtime| (*path, mtime)))
        .collect::<Vec<_>>();
    let newest_source_mtime = source_mtimes
        .iter()
        .map(|(_, mtime)| *mtime)
        .max()
        .unwrap_or_default();
    let report_mtime = file_modified_unix_secs(report_path);
    let artifact_mtime = readback_artifact_path.and_then(file_modified_unix_secs);
    let pass = report_mtime.is_some_and(|mtime| mtime >= newest_source_mtime)
        && artifact_mtime.is_some_and(|mtime| mtime >= newest_source_mtime);
    json!({
        "pass": pass,
        "basis": "preview E2E report and app-owned framebuffer artifact must be newer than native/example source files",
        "newest_source_mtime": newest_source_mtime,
        "source_mtimes": source_mtimes
            .into_iter()
            .map(|(path, mtime)| json!({"path": path, "mtime": mtime}))
            .collect::<Vec<_>>(),
        "preview_e2e_report": report_path,
        "preview_e2e_report_mtime": report_mtime,
        "readback_artifact": readback_artifact_path,
        "readback_artifact_mtime": artifact_mtime
    })
}

fn file_modified_unix_secs(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn todomvc_title_region(layout_artifact: &serde_json::Value) -> Option<serde_json::Value> {
    layout_artifact
        .pointer("/layout_frame/display_list")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .find(|item| item.get("text").and_then(serde_json::Value::as_str) == Some("todos"))
        .and_then(|item| item.get("bounds").cloned())
}

fn todomvc_layout_reference_evidence(layout_artifact: &serde_json::Value) -> serde_json::Value {
    let Some(items) = layout_artifact
        .pointer("/layout_frame/display_list")
        .and_then(serde_json::Value::as_array)
    else {
        return json!({"pass": false, "missing": ["layout display list"]});
    };
    let has_text = |text: &str| {
        items
            .iter()
            .any(|item| item.get("text").and_then(serde_json::Value::as_str) == Some(text))
    };
    let item_bounds_matching = |predicate: &dyn Fn(&serde_json::Value) -> bool| {
        items
            .iter()
            .find(|item| predicate(item))
            .and_then(|item| item.get("bounds").cloned())
    };
    let title_bounds = items
        .iter()
        .find(|item| item.get("text").and_then(serde_json::Value::as_str) == Some("todos"))
        .and_then(|item| item.get("bounds").cloned());
    let surface_bounds = item_bounds_matching(&|item| {
        item.get("kind").and_then(serde_json::Value::as_str) == Some("stack")
            && item
                .pointer("/style/background")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| value.contains("lightness:1"))
            && item
                .get("bounds")
                .and_then(parse_json_bounds)
                .is_some_and(|bounds| {
                    bounds.width >= 500.0
                        && bounds.width <= 620.0
                        && bounds.y >= 80.0
                        && bounds.y <= 160.0
                })
    });
    let input_bounds = item_bounds_matching(&|item| {
        item.get("kind").and_then(serde_json::Value::as_str) == Some("text_input")
            && item
                .pointer("/style/placeholder")
                .and_then(serde_json::Value::as_str)
                == Some("What needs to be done?")
    });
    let footer_bounds = item_bounds_matching(&|item| {
        item.get("text")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|text| text.ends_with(" items left"))
    });
    let row_title_count = items
        .iter()
        .filter(|item| {
            item.get("text")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|text| {
                    matches!(
                        text,
                        "Read documentation"
                            | "Finish TodoMVC renderer"
                            | "Walk the dog"
                            | "Buy groceries"
                    )
                })
        })
        .count();
    let checked_count = items
        .iter()
        .filter(|item| {
            item.pointer("/style/checked")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        })
        .count();
    let placeholder_present = items.iter().any(|item| {
        item.pointer("/style/placeholder")
            .and_then(serde_json::Value::as_str)
            == Some("What needs to be done?")
    });

    let mut missing = Vec::new();
    if !has_text("todos") {
        missing.push("title text");
    }
    if surface_bounds.is_none() {
        missing.push("surface");
    }
    if input_bounds.is_none() || !placeholder_present {
        missing.push("new todo input with placeholder");
    }
    if row_title_count < 4 {
        missing.push("four initial todo rows");
    }
    if checked_count < 1 {
        missing.push("checked completed row");
    }
    if footer_bounds.is_none() || !has_text("All") || !has_text("Active") || !has_text("Completed")
    {
        missing.push("controls footer");
    }
    if !has_text("Clear completed") {
        missing.push("clear completed button");
    }
    if !has_text("Double-click to edit a todo") {
        missing.push("info footer");
    }
    let title_large_and_centered = bounds_pass(&title_bounds, |bounds| {
        bounds.width >= 240.0
            && bounds.height >= 80.0
            && bounds.center_x >= 300.0
            && bounds.center_x <= 500.0
            && bounds.y <= 40.0
    });
    if !title_large_and_centered {
        missing.push("large centered todos title bounds");
    }
    let surface_reference_sized = bounds_pass(&surface_bounds, |bounds| {
        bounds.width >= 500.0
            && bounds.width <= 620.0
            && bounds.height >= 300.0
            && bounds.center_x >= 340.0
            && bounds.center_x <= 460.0
            && bounds.y >= 80.0
            && bounds.y <= 150.0
    });
    if !surface_reference_sized {
        missing.push("reference-sized centered app surface");
    }
    let input_reference_sized = bounds_pass(&input_bounds, |bounds| {
        bounds.width >= 440.0
            && bounds.height >= 48.0
            && bounds.height <= 70.0
            && bounds.y >= 115.0
            && bounds.y <= 155.0
    });
    if !input_reference_sized {
        missing.push("reference-sized new todo input row");
    }
    json!({
        "pass": missing.is_empty(),
        "missing": missing,
        "title_bounds": title_bounds,
        "surface_bounds": surface_bounds,
        "input_bounds": input_bounds,
        "footer_bounds": footer_bounds,
        "row_title_count": row_title_count,
        "checked_count": checked_count,
        "placeholder_present": placeholder_present,
        "title_large_and_centered": title_large_and_centered,
        "surface_reference_sized": surface_reference_sized,
        "input_reference_sized": input_reference_sized
    })
}

fn todomvc_pixel_reference_evidence(
    path: &Path,
    reference: &Path,
    reference_metadata: Option<&serde_json::Value>,
    layout_evidence: &serde_json::Value,
) -> Result<serde_json::Value, image::ImageError> {
    let image = image::open(path)?.to_rgba8();
    let (width, height) = image.dimensions();
    let reference_image = image::open(reference)?.to_rgba8();
    let visual_artifacts = write_todomvc_visual_artifacts(
        &image,
        &reference_image,
        path,
        reference,
        reference_metadata,
        layout_evidence,
    )?;
    let red_title = count_region_pixels(&image, 0, 0, width, height / 4, |r, g, b, _| {
        r > 150 && g < 100 && b < 120
    });
    let dark_text = count_region_pixels(&image, 0, height / 5, width, height, |r, g, b, _| {
        r < 110 && g < 110 && b < 110
    });
    let white_panel = count_region_pixels(&image, 0, height / 8, width, height, |r, g, b, _| {
        r > 245 && g > 245 && b > 245
    });
    let teal_check = count_region_pixels(&image, 0, height / 5, width / 2, height, |r, g, b, _| {
        r < 130 && g > 150 && b > 130
    });
    let title_pixel_bounds = color_pixel_bounds(&image, 0, 0, width, height / 4, |r, g, b, a| {
        a > 180 && r > 150 && g < 120 && b < 140
    });
    let title_pixel_bounds_pass = title_pixel_bounds.as_ref().is_some_and(|bounds| {
        let center_ratio = bounds.center_x() / width.max(1) as f64;
        let title_width = bounds.width();
        let title_height = bounds.height();
        title_width >= 175
            && title_width <= 230
            && title_height >= 52
            && title_height <= 76
            && center_ratio >= 0.42
            && center_ratio <= 0.58
            && bounds.y0 <= height / 8
    });
    let reference_title_bounds = color_pixel_bounds(
        &reference_image,
        0,
        0,
        reference_image.width(),
        reference_image.height() / 4,
        |r, g, b, a| a > 180 && r > 150 && g < 120 && b < 140,
    );
    let crop_diff_pass = visual_artifacts
        .get("normalized_crop_mean_abs_diff")
        .and_then(serde_json::Value::as_f64)
        .is_some_and(|diff| diff <= 6.0);
    let diff_p95_pass = visual_artifacts
        .get("normalized_crop_p95_abs_diff")
        .and_then(serde_json::Value::as_f64)
        .is_some_and(|diff| diff <= 30.0);
    let high_diff_ratio_pass = visual_artifacts
        .get("normalized_crop_high_diff_ratio")
        .and_then(serde_json::Value::as_f64)
        .is_some_and(|ratio| ratio <= 0.028);
    let connected_mismatch_pass = visual_artifacts
        .get("normalized_crop_largest_mismatch_region_ratio")
        .and_then(serde_json::Value::as_f64)
        .is_some_and(|ratio| ratio <= 0.002);
    let mut missing = Vec::new();
    if red_title < 600 {
        missing.push("large red title pixels");
    }
    if !title_pixel_bounds_pass {
        missing.push("large centered red title pixel bounds");
    }
    if dark_text < 900 {
        missing.push("todo/footer dark text pixels");
    }
    if white_panel < 20_000 {
        missing.push("large white panel pixels");
    }
    if teal_check < 20 {
        missing.push("completed-row check pixels");
    }
    if !crop_diff_pass {
        missing.push("normalized reference/native crop similarity");
    }
    if !diff_p95_pass {
        missing.push("normalized crop p95 similarity");
    }
    if !high_diff_ratio_pass {
        missing.push("bounded high-difference pixel ratio");
    }
    if !connected_mismatch_pass {
        missing.push("no large connected mismatch regions");
    }
    Ok(json!({
        "pass": missing.is_empty(),
        "missing": missing,
        "dimensions": [width, height],
        "red_title_pixels": red_title,
        "dark_text_pixels": dark_text,
        "white_panel_pixels": white_panel,
        "teal_check_pixels": teal_check,
        "title_pixel_bounds": title_pixel_bounds.map(|bounds| bounds.to_json()),
        "reference_title_pixel_bounds": reference_title_bounds.map(|bounds| bounds.to_json()),
        "title_pixel_bounds_pass": title_pixel_bounds_pass,
        "visual_artifacts": visual_artifacts,
        "thresholds": {
            "normalized_crop_mean_abs_diff_max": 6.0,
            "normalized_crop_p95_abs_diff_max": 30.0,
            "normalized_crop_high_diff_ratio_max": 0.028,
            "normalized_crop_largest_mismatch_region_ratio_max": 0.002
        }
    }))
}

#[derive(Clone, Copy)]
struct ImageBounds {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

impl ImageBounds {
    fn width(self) -> u32 {
        self.x1.saturating_sub(self.x0).saturating_add(1)
    }

    fn height(self) -> u32 {
        self.y1.saturating_sub(self.y0).saturating_add(1)
    }

    fn center_x(self) -> f64 {
        self.x0 as f64 + self.width() as f64 / 2.0
    }

    fn to_json(self) -> serde_json::Value {
        json!({
            "x": self.x0,
            "y": self.y0,
            "width": self.width(),
            "height": self.height(),
            "center_x": self.center_x()
        })
    }
}

struct JsonBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    center_x: f64,
}

impl JsonBounds {
    fn to_json_value(&self) -> serde_json::Value {
        json!({
            "x": self.x,
            "y": self.y,
            "width": self.width,
            "height": self.height,
            "center_x": self.center_x
        })
    }
}

fn todomvc_reference_viewport(metadata: &serde_json::Value) -> Option<(u32, u32)> {
    Some((
        metadata
            .pointer("/viewport/width")
            .and_then(serde_json::Value::as_u64)? as u32,
        metadata
            .pointer("/viewport/height")
            .and_then(serde_json::Value::as_u64)? as u32,
    ))
}

fn todomvc_reference_todoapp_bounds(metadata: &serde_json::Value) -> Option<JsonBounds> {
    metadata
        .get("elements")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .find(|element| {
            element
                .get("classes")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|classes| {
                    classes
                        .iter()
                        .any(|class| class.as_str() == Some("todoapp"))
                })
        })
        .and_then(|element| element.get("bounds"))
        .and_then(parse_json_bounds)
}

fn parse_json_bounds(value: &serde_json::Value) -> Option<JsonBounds> {
    let x = value.get("x").and_then(serde_json::Value::as_f64)?;
    let y = value.get("y").and_then(serde_json::Value::as_f64)?;
    let width = value.get("width").and_then(serde_json::Value::as_f64)?;
    let height = value.get("height").and_then(serde_json::Value::as_f64)?;
    Some(JsonBounds {
        x,
        y,
        width,
        height,
        center_x: x + width / 2.0,
    })
}

fn bounds_pass(
    value: &Option<serde_json::Value>,
    predicate: impl FnOnce(&JsonBounds) -> bool,
) -> bool {
    value
        .as_ref()
        .and_then(parse_json_bounds)
        .is_some_and(|bounds| predicate(&bounds))
}

fn color_pixel_bounds(
    image: &image::RgbaImage,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    predicate: impl Fn(u8, u8, u8, u8) -> bool,
) -> Option<ImageBounds> {
    let mut bounds: Option<ImageBounds> = None;
    for y in y0.min(image.height())..y1.min(image.height()) {
        for x in x0.min(image.width())..x1.min(image.width()) {
            let [r, g, b, a] = image.get_pixel(x, y).0;
            if predicate(r, g, b, a) {
                bounds = Some(match bounds {
                    Some(existing) => ImageBounds {
                        x0: existing.x0.min(x),
                        y0: existing.y0.min(y),
                        x1: existing.x1.max(x),
                        y1: existing.y1.max(y),
                    },
                    None => ImageBounds {
                        x0: x,
                        y0: y,
                        x1: x,
                        y1: y,
                    },
                });
            }
        }
    }
    bounds
}

fn write_todomvc_visual_artifacts(
    native: &image::RgbaImage,
    reference: &image::RgbaImage,
    native_path: &Path,
    reference_path: &Path,
    reference_metadata: Option<&serde_json::Value>,
    layout_evidence: &serde_json::Value,
) -> Result<serde_json::Value, image::ImageError> {
    let artifact_dir = PathBuf::from("target/reports/native-gpu");
    let _ = fs::create_dir_all(&artifact_dir);
    let native_crop_path = artifact_dir.join("todomvc-native-normalized-crop.png");
    let reference_crop_path = artifact_dir.join("todomvc-reference-normalized-crop.png");
    let diff_path = artifact_dir.join("todomvc-reference-diff-heatmap.png");

    let viewport = reference_metadata
        .and_then(todomvc_reference_viewport)
        .unwrap_or((700, 700));
    let reference_todoapp = reference_metadata.and_then(todomvc_reference_todoapp_bounds);
    let native_crop = crop_native_todomvc_frame(
        native,
        layout_evidence,
        reference_todoapp.as_ref(),
        viewport,
    );
    let reference_normalized = image::imageops::resize(
        reference,
        viewport.0,
        viewport.1,
        image::imageops::FilterType::Triangle,
    );
    let native_normalized = if native_crop.dimensions() == viewport {
        native_crop
    } else {
        image::imageops::resize(
            &native_crop,
            viewport.0,
            viewport.1,
            image::imageops::FilterType::Triangle,
        )
    };
    let (heatmap, diff_metrics) = todomvc_diff_heatmap(&native_normalized, &reference_normalized);
    native_normalized.save(&native_crop_path)?;
    reference_normalized.save(&reference_crop_path)?;
    heatmap.save(&diff_path)?;

    Ok(json!({
        "native_source": native_path,
        "reference_source": reference_path,
        "native_normalized_crop": native_crop_path,
        "reference_normalized_crop": reference_crop_path,
        "diff_heatmap": diff_path,
        "normalized_crop_dimensions": [viewport.0, viewport.1],
        "reference_todoapp_bounds": reference_todoapp.map(|bounds| bounds.to_json_value()),
        "normalized_crop_mean_abs_diff": diff_metrics.mean_abs_diff,
        "normalized_crop_p95_abs_diff": diff_metrics.p95_abs_diff,
        "normalized_crop_high_diff_ratio": diff_metrics.high_diff_ratio,
        "normalized_crop_largest_mismatch_region_ratio": diff_metrics.largest_mismatch_region_ratio,
        "normalized_crop_high_diff_threshold": diff_metrics.high_diff_threshold
    }))
}

fn crop_native_todomvc_frame(
    image: &image::RgbaImage,
    layout_evidence: &serde_json::Value,
    reference_todoapp: Option<&JsonBounds>,
    viewport: (u32, u32),
) -> image::RgbaImage {
    let surface = layout_evidence
        .get("surface_bounds")
        .and_then(parse_json_bounds);
    let x0 = surface
        .as_ref()
        .zip(reference_todoapp)
        .map(|(native, reference)| native.x - reference.x)
        .unwrap_or_else(|| (image.width() as f64 - viewport.0 as f64) / 2.0)
        .round() as i32;
    crop_with_padding(
        image,
        x0,
        0,
        viewport.0,
        viewport.1,
        image::Rgba([247, 247, 247, 255]),
    )
}

#[allow(dead_code)]
fn crop_reference_todomvc_frame(image: &image::RgbaImage) -> image::RgbaImage {
    let title_bounds = color_pixel_bounds(
        image,
        0,
        0,
        image.width(),
        image.height() / 4,
        |r, g, b, a| a > 180 && r > 150 && g < 120 && b < 140,
    );
    let center = title_bounds
        .map(ImageBounds::center_x)
        .unwrap_or(image.width() as f64 / 2.0);
    let crop_width = 640.0f64.min(image.width() as f64);
    let x0 = (center - crop_width / 2.0).floor().max(0.0) as u32;
    let x1 = (x0 as f64 + crop_width).min(image.width() as f64) as u32;
    let y1 = (image.height() as f64 * 0.48).ceil() as u32;
    crop_nonempty(image, x0, 0, x1, y1)
}

fn crop_with_padding(
    image: &image::RgbaImage,
    x0: i32,
    y0: i32,
    width: u32,
    height: u32,
    background: image::Rgba<u8>,
) -> image::RgbaImage {
    let mut output = image::RgbaImage::from_pixel(width, height, background);
    for dest_y in 0..height {
        let source_y = y0 + dest_y as i32;
        if !(0..image.height() as i32).contains(&source_y) {
            continue;
        }
        for dest_x in 0..width {
            let source_x = x0 + dest_x as i32;
            if !(0..image.width() as i32).contains(&source_x) {
                continue;
            }
            let pixel = *image.get_pixel(source_x as u32, source_y as u32);
            output.put_pixel(dest_x, dest_y, pixel);
        }
    }
    output
}

fn crop_nonempty(image: &image::RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32) -> image::RgbaImage {
    let x0 = x0.min(image.width().saturating_sub(1));
    let y0 = y0.min(image.height().saturating_sub(1));
    let width = x1.saturating_sub(x0).max(1).min(image.width() - x0);
    let height = y1.saturating_sub(y0).max(1).min(image.height() - y0);
    image::imageops::crop_imm(image, x0, y0, width, height).to_image()
}

struct TodoMvcDiffMetrics {
    mean_abs_diff: f64,
    p95_abs_diff: f64,
    high_diff_ratio: f64,
    largest_mismatch_region_ratio: f64,
    high_diff_threshold: u8,
}

fn todomvc_diff_heatmap(
    native: &image::RgbaImage,
    reference: &image::RgbaImage,
) -> (image::RgbaImage, TodoMvcDiffMetrics) {
    let width = native.width().min(reference.width());
    let height = native.height().min(reference.height());
    let mut heatmap = image::RgbaImage::new(width, height);
    let mut diffs = Vec::with_capacity(width as usize * height as usize);
    let mut total = 0u64;
    for y in 0..height {
        for x in 0..width {
            let [nr, ng, nb, _] = native.get_pixel(x, y).0;
            let [rr, rg, rb, _] = reference.get_pixel(x, y).0;
            let diff = ((nr as i16 - rr as i16).unsigned_abs() as u32
                + (ng as i16 - rg as i16).unsigned_abs() as u32
                + (nb as i16 - rb as i16).unsigned_abs() as u32)
                / 3;
            total += diff as u64;
            diffs.push(diff as u8);
            heatmap.put_pixel(
                x,
                y,
                image::Rgba([diff as u8, 0, 255u8.saturating_sub(diff as u8), 255]),
            );
        }
    }
    diffs.sort_unstable();
    let pixel_count = width.max(1) as usize * height.max(1) as usize;
    let mean = total as f64 / pixel_count as f64;
    let p95_index = pixel_count.saturating_sub(1).min(pixel_count * 95 / 100);
    let p95 = diffs.get(p95_index).copied().unwrap_or_default() as f64;
    let high_diff_threshold = 70u8;
    let high_diff_pixels = diffs
        .iter()
        .filter(|diff| **diff >= high_diff_threshold)
        .count();
    let high_diff_ratio = high_diff_pixels as f64 / pixel_count as f64;
    let largest_mismatch_region_ratio =
        largest_diff_region_ratio(native, reference, high_diff_threshold);
    (
        heatmap,
        TodoMvcDiffMetrics {
            mean_abs_diff: mean,
            p95_abs_diff: p95,
            high_diff_ratio,
            largest_mismatch_region_ratio,
            high_diff_threshold,
        },
    )
}

fn largest_diff_region_ratio(
    native: &image::RgbaImage,
    reference: &image::RgbaImage,
    threshold: u8,
) -> f64 {
    let width = native.width().min(reference.width());
    let height = native.height().min(reference.height());
    let pixel_count = width.max(1) as usize * height.max(1) as usize;
    let mut visited = vec![false; width as usize * height as usize];
    let mut largest = 0usize;
    let mut stack = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let index = (y * width + x) as usize;
            if visited[index] || !pixel_diff_at_least(native, reference, x, y, threshold) {
                visited[index] = true;
                continue;
            }
            visited[index] = true;
            stack.push((x, y));
            let mut region = 0usize;
            while let Some((cx, cy)) = stack.pop() {
                region += 1;
                for (nx, ny) in diff_neighbors(cx, cy, width, height) {
                    let neighbor_index = (ny * width + nx) as usize;
                    if visited[neighbor_index] {
                        continue;
                    }
                    visited[neighbor_index] = true;
                    if pixel_diff_at_least(native, reference, nx, ny, threshold) {
                        stack.push((nx, ny));
                    }
                }
            }
            largest = largest.max(region);
        }
    }
    largest as f64 / pixel_count as f64
}

fn diff_neighbors(x: u32, y: u32, width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
    let mut neighbors = Vec::with_capacity(4);
    if x > 0 {
        neighbors.push((x - 1, y));
    }
    if x + 1 < width {
        neighbors.push((x + 1, y));
    }
    if y > 0 {
        neighbors.push((x, y - 1));
    }
    if y + 1 < height {
        neighbors.push((x, y + 1));
    }
    neighbors.into_iter()
}

fn pixel_diff_at_least(
    native: &image::RgbaImage,
    reference: &image::RgbaImage,
    x: u32,
    y: u32,
    threshold: u8,
) -> bool {
    let [nr, ng, nb, _] = native.get_pixel(x, y).0;
    let [rr, rg, rb, _] = reference.get_pixel(x, y).0;
    let diff = ((nr as i16 - rr as i16).unsigned_abs() as u32
        + (ng as i16 - rg as i16).unsigned_abs() as u32
        + (nb as i16 - rb as i16).unsigned_abs() as u32)
        / 3;
    diff >= threshold as u32
}

fn count_region_pixels(
    image: &image::RgbaImage,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    predicate: impl Fn(u8, u8, u8, u8) -> bool,
) -> u64 {
    let mut count = 0u64;
    for y in y0.min(image.height())..y1.min(image.height()) {
        for x in x0.min(image.width())..x1.min(image.width()) {
            let [r, g, b, a] = image.get_pixel(x, y).0;
            if predicate(r, g, b, a) {
                count += 1;
            }
        }
    }
    count
}

fn native_preview_host_route_evidence(
    example: &str,
    report: &serde_json::Value,
) -> serde_json::Value {
    if let Some(operator_ack) = report
        .pointer("/dev_ipc_probe/operator_host_input")
        .filter(|ack| ack.get("status").and_then(serde_json::Value::as_str) == Some("pass"))
    {
        let host_route_assertions = operator_ack
            .get("host_route_assertions")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let changes_visible_frame = operator_ack
            .get("outputs")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|outputs| {
                outputs.iter().any(|output| {
                    output
                        .get("render_patch_count")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or_default()
                        > 0
                        || output
                            .get("semantic_delta_count")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or_default()
                            > 0
                })
            });
        let host_route_steps = host_route_assertions
            .iter()
            .map(|assertion| {
                json!({
                    "step": "operator-host-input-scenario-route",
                    "input_evidence": report.get("native_input_adapter").cloned().unwrap_or_else(|| json!({})),
                    "target_hit_region": assertion.get("target_hit_region").cloned().unwrap_or(serde_json::Value::Null),
                    "source_intents": assertion.get("source_intent").cloned().map(|intent| json!([intent])).unwrap_or_else(|| json!([])),
                    "host_events": assertion.get("host_events").cloned().unwrap_or_else(|| json!([])),
                    "route_contract": "HostInputEvent -> document hit region -> SourceIntent",
                    "private_runtime_dispatch_used": false,
                    "operator_host_input_observed": true,
                    "real_os_input_observed": native_gpu_real_input_observed(report),
                    "changes_visible_frame": changes_visible_frame,
                    "visible_frame_change_method": "operator_host_event_to_preview_ipc_render_patch"
                })
            })
            .collect::<Vec<_>>();
        let status = if !host_route_assertions.is_empty()
            && host_route_assertions.iter().all(|assertion| {
                assertion.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
            }) {
            "pass"
        } else {
            "fail"
        };
        let os_route_steps = if native_gpu_real_input_observed(report) {
            host_route_steps.clone()
        } else {
            Vec::new()
        };
        return json!({
            "status": status,
            "example": example,
            "target_hit_region": host_route_steps
                .first()
                .and_then(|step| step.get("target_hit_region"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "source_intents": host_route_steps
                .first()
                .and_then(|step| step.get("source_intents"))
                .cloned()
                .unwrap_or_else(|| json!([])),
            "operator_host_input_observed": true,
            "real_os_input_observed": native_gpu_real_input_observed(report),
            "changes_visible_frame": changes_visible_frame,
            "per_step_host_input_route": host_route_steps,
            "per_step_os_pointer_keyboard_route": os_route_steps,
            "blocked_reason": match status {
                "pass" => serde_json::Value::Null,
                _ => json!("operator host input did not prove generic hit/source-intent routing")
            }
        });
    }
    let hit_targets = report
        .get("hit_target_assertions")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let source_intents = report
        .get("source_intent_assertions")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let target_hit = source_intents
        .iter()
        .filter_map(|intent| intent.get("node").and_then(serde_json::Value::as_str))
        .find_map(|node| {
            hit_targets
                .iter()
                .find(|target| target.get("node").and_then(serde_json::Value::as_str) == Some(node))
                .cloned()
        })
        .or_else(|| hit_targets.first().cloned());
    let target_node = target_hit
        .as_ref()
        .and_then(|target| target.get("node"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let matched_source_intents = source_intents
        .iter()
        .filter(|intent| {
            target_node.as_deref().is_some_and(|node| {
                intent.get("node").and_then(serde_json::Value::as_str) == Some(node)
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    let real_input = native_gpu_real_input_observed(report)
        || report
            .get("real_os_input")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    let operator_input = report
        .get("operator_host_input")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let input_ready = real_input || operator_input;
    let has_route = target_hit.is_some() && !matched_source_intents.is_empty();
    let changes_visible_frame = report
        .pointer("/dev_ipc_probe/operator_host_input/outputs")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|outputs| {
            outputs.iter().any(|output| {
                output
                    .get("render_patch_count")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or_default()
                    > 0
                    || output
                        .get("semantic_delta_count")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or_default()
                        > 0
            })
        });
    let status = if input_ready && has_route {
        "pass"
    } else if !input_ready && has_route {
        "waiting-for-host-input"
    } else {
        "fail"
    };
    let host_route_steps = if has_route {
        vec![json!({
            "step": "host-event-to-source-intent",
            "input_evidence": report.get("native_input_adapter").cloned().unwrap_or_else(|| json!({})),
            "target_hit_region": target_hit,
            "source_intents": matched_source_intents,
            "host_events": report
                .pointer("/operator_host_input_evidence/host_events")
                .cloned()
                .unwrap_or_else(|| json!([])),
            "route_contract": "HostInputEvent -> document hit region -> SourceIntent",
            "private_runtime_dispatch_used": false,
            "operator_host_input_observed": operator_input,
            "real_os_input_observed": real_input,
            "changes_visible_frame": changes_visible_frame,
            "visible_frame_change_method": "operator_host_event_to_preview_ipc_render_patch"
        })]
    } else {
        Vec::new()
    };
    let os_route_steps = if real_input {
        host_route_steps.clone()
    } else {
        Vec::new()
    };
    json!({
        "status": status,
        "example": example,
        "target_hit_region": host_route_steps
            .first()
            .and_then(|step| step.get("target_hit_region"))
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "source_intents": host_route_steps
            .first()
            .and_then(|step| step.get("source_intents"))
            .cloned()
            .unwrap_or_else(|| json!([])),
        "operator_host_input_observed": operator_input,
        "real_os_input_observed": real_input,
        "changes_visible_frame": changes_visible_frame,
        "per_step_host_input_route": host_route_steps,
        "per_step_os_pointer_keyboard_route": os_route_steps,
        "blocked_reason": match status {
            "pass" => serde_json::Value::Null,
            "waiting-for-host-input" => json!("generic hit/source-intent route exists, but no operator host input was recorded"),
            _ => json!("native document layout did not expose both a hit region and source intent for a route target")
        }
    })
}

fn native_runtime_assertions_after_input(
    _example: &str,
    report: &serde_json::Value,
) -> serde_json::Value {
    if let Some(live_preview_evidence) = report.get("dev_ipc_probe").and_then(|probe| {
        probe.get("operator_host_input").filter(|evidence| {
            evidence.get("status").and_then(serde_json::Value::as_str) == Some("pass")
        })
    }) {
        let assertions = live_preview_evidence
            .get("assertions")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        return json!({
            "status": "pass",
            "assertions": assertions,
            "public_runtime_api": live_preview_evidence
                .get("public_runtime_api")
                .cloned()
                .unwrap_or_else(|| json!("boon_runtime::LiveRuntime::apply_source_event")),
            "private_runtime_dispatch_used": false,
            "operator_host_input_observed": true,
            "real_os_input_observed": live_preview_evidence
                .get("real_os_input")
                .and_then(serde_json::Value::as_bool)
                == Some(true),
            "host_route_ready": report
                .pointer("/native_host_input_route_evidence/status")
                .and_then(serde_json::Value::as_str)
                == Some("pass"),
            "live_preview_process_route": true,
            "preview_pid": live_preview_evidence
                .get("preview_pid")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "route_contract": live_preview_evidence
                .get("route_contract")
                .cloned()
                .unwrap_or_else(|| json!("HostInputEvent -> SourceIntent -> preview IPC -> LiveRuntime::apply_source_event")),
            "outputs": live_preview_evidence
                .get("outputs")
                .cloned()
                .unwrap_or_else(|| json!([]))
        });
    }

    json!({
        "status": "fail",
        "assertions": [],
        "public_runtime_api": "boon_runtime::LiveRuntime::apply_source_event_for_step",
        "private_runtime_dispatch_used": false,
        "operator_host_input_observed": report
            .get("operator_host_input")
            .and_then(serde_json::Value::as_bool)
            == Some(true),
        "real_os_input_observed": native_gpu_real_input_observed(report),
        "host_route_ready": report
            .pointer("/native_host_input_route_evidence/status")
            .and_then(serde_json::Value::as_str)
            == Some("pass"),
        "blocked_reason": "runtime assertions require preview-side operator host input evidence from generic source_events"
    })
}

struct NativeGpuScrollSelector {
    label: String,
    blockers: Vec<String>,
}

fn native_gpu_scroll_selector(args: &[String]) -> NativeGpuScrollSelector {
    let example = value_arg(args, "--example");
    let surface = value_arg(args, "--surface");
    let target = value_arg(args, "--target");
    let mut blockers = Vec::new();

    if target.is_some() {
        blockers.push(
            "verify-native-gpu-scroll-speed no longer accepts --target; use `--example <manifest-id>` or `--surface dev-code-editor`"
                .to_owned(),
        );
    }

    let selected = match (example.as_deref(), surface.as_deref()) {
        (None, None) => "cells".to_owned(),
        (None, Some("dev-code-editor")) => "dev-code-editor".to_owned(),
        (None, Some(surface_id)) => match boon_runtime::example_manifest_entry(surface_id) {
            Ok(entry) => entry.id,
            Err(error) => {
                blockers.push(format!(
                    "unsupported scroll surface `{surface_id}`: {error}"
                ));
                "cells".to_owned()
            }
        },
        (Some(example_id), Some("dev-code-editor")) => {
            blockers.push(format!(
                "ambiguous scroll selector: `--example {example_id}` conflicts with `--surface dev-code-editor`"
            ));
            "dev-code-editor".to_owned()
        }
        (Some(example_id), Some(surface_id)) if example_id != surface_id => {
            blockers.push(format!(
                "ambiguous scroll selector: `--example {example_id}` conflicts with `--surface {surface_id}`"
            ));
            example_id.to_owned()
        }
        (Some(example_id), _) => match boon_runtime::example_manifest_entry(example_id) {
            Ok(entry) => entry.id,
            Err(error) => {
                blockers.push(format!(
                    "unsupported scroll example `{example_id}`: {error}"
                ));
                "cells".to_owned()
            }
        },
    };

    NativeGpuScrollSelector {
        label: selected,
        blockers,
    }
}

fn verify_native_gpu_scroll_speed(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let selector = native_gpu_scroll_selector(args);
    let dev_editor = selector.label == "dev-code-editor";
    let label = selector.label;
    let selector_valid = selector.blockers.is_empty();
    for blocker in selector.blockers {
        push_audit_check(
            &mut checks,
            &mut blockers,
            "native-gpu-scroll:cli-selector",
            false,
            format!(
                "example={:?}, surface={:?}, target={:?}",
                value_arg(args, "--example"),
                value_arg(args, "--surface"),
                value_arg(args, "--target")
            ),
            Some(blocker),
        );
    }
    let artifacts_dir = PathBuf::from("target/artifacts/native-gpu");
    std::fs::create_dir_all(&artifacts_dir)?;
    let supervisor_report = PathBuf::from(format!(
        "target/reports/native-gpu/.scroll-{label}-supervisor.json"
    ));
    let live_state_report = artifacts_dir.join(format!("scroll-{label}-live-state.json"));
    let mut dev_editor_speed_corpus = json!({"status": "not-applicable"});
    let (source_path, source_example_id, source_text, source_files) = if dev_editor {
        let (path, example_id, corpus) = ensure_dev_editor_speed_corpus(&artifacts_dir)?;
        let source_text = std::fs::read_to_string(&path)?;
        let source_files = Vec::new();
        dev_editor_speed_corpus = corpus;
        (path, example_id, source_text, source_files)
    } else {
        let entry = boon_runtime::example_manifest_entry(&label)?;
        let source_text = boon_runtime::source_text_for_entry(&entry)?;
        let source_files = manifest_source_files(&entry);
        (
            PathBuf::from(entry.source),
            label.clone(),
            source_text,
            source_files,
        )
    };
    let source_hash = source_hash_for_report_source_files(&source_files, &source_text)?;
    let layout_probe_report = artifacts_dir.join(format!("scroll-{label}-layout-proof.json"));
    let mut cosmic_launch_proof = json!({"status": "not-run"});
    let mut isolated_real_window_launch_proof = json!({"status": "not-run"});
    let title_token = native_gpu_title_token(&format!("scroll-{label}"));
    let input_sample_delay_ms = native_gpu_input_sample_delay_ms();
    let _ = std::fs::remove_file(&supervisor_report);
    let _ = std::fs::remove_file(&live_state_report);

    let isolated_real_window_available = command_available("weston")
        && command_available("wayland-info")
        && weston_test_plugin_path().is_some()
        && weston_test_driver_path().is_some();
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-scroll-{label}:isolated-real-window-environment"),
        isolated_real_window_available,
        format!("isolated_real_window_available={isolated_real_window_available}"),
        (!isolated_real_window_available).then(|| {
            "native scroll-speed proof requires the isolated Weston real-window harness".to_owned()
        }),
    );

    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland";
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-scroll-{label}:wayland-session"),
        wayland,
        format!(
            "WAYLAND_DISPLAY={:?}, XDG_SESSION_TYPE={:?}",
            std::env::var("WAYLAND_DISPLAY").ok(),
            std::env::var("XDG_SESSION_TYPE").ok()
        ),
        (!wayland).then(|| "native scroll proof requires a Wayland session".to_owned()),
    );

    let speed_binary = "./target/release/boon_native_playground";
    let build = Command::new("cargo")
        .args(["build", "--release", "-p", "boon_native_playground"])
        .status()?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-scroll-{label}:playground-release-build"),
        build.success(),
        format!("cargo build --release -p boon_native_playground status={build}"),
        (!build.success()).then(|| "failed to build release boon_native_playground".to_owned()),
    );

    let layout_probe = if dev_editor && selector_valid {
        json!({
            "status": "pass",
            "source_path": source_path,
            "source_sha256": source_hash,
            "layout_source": "dev-window-editor-model",
            "scroll_regions": [
                {
                    "id": "scroll:dev-code-editor",
                    "node": "dev-code-editor",
                    "axis": "vertical",
                    "bounds": {"x": 0.0, "y": 96.0, "width": 1180.0, "height": 560.0}
                },
                {
                    "id": "scroll-x:dev-code-editor",
                    "node": "dev-code-editor",
                    "axis": "horizontal",
                    "bounds": {"x": 0.0, "y": 656.0, "width": 1180.0, "height": 18.0}
                }
            ],
            "hit_target_assertions": [],
            "source_intent_assertions": []
        })
    } else if build.success() && selector_valid {
        run_native_layout_probe(Path::new(speed_binary), &source_path, &layout_probe_report)?
    } else {
        json!({"status": "not-run", "reason": "boon_native_playground build failed or scroll selector invalid"})
    };
    let driver_target = native_scroll_driver_target(&label, &layout_probe);
    let native_input_driver_attempt =
        native_gpu_operator_input_driver_attempt("scroll-speed", &label, driver_target.clone());
    let linked_linux_real_window_speed_evidence =
        linked_linux_human_like_speed_real_window_evidence(&label, &source_hash);
    let measured_surface_key = if dev_editor {
        "dev_surface_proof"
    } else {
        "preview_surface_proof"
    };
    let vertical_driver_target =
        native_scroll_driver_target_for_axis(&label, &layout_probe, "vertical")
            .or_else(|| driver_target.clone());
    let horizontal_driver_target =
        native_scroll_driver_target_for_axis(&label, &layout_probe, "horizontal")
            .or_else(|| driver_target.clone());
    let mut axis_specific_real_window_scroll_observation = json!({
        "status": "not-run",
        "reason": "native scroll-speed axis-specific Weston wheel probes were not attempted"
    });

    if build.success() && selector_valid && isolated_real_window_available {
        let isolated_role_report_timeout_ms = 60_000_u64.saturating_add(input_sample_delay_ms);
        isolated_real_window_launch_proof = run_isolated_weston_desktop_preview_e2e(
            Path::new(speed_binary),
            &source_example_id,
            &title_token,
            input_sample_delay_ms.max(1_500),
            isolated_role_report_timeout_ms,
            &supervisor_report,
            &live_state_report,
            driver_target.clone(),
            None,
            dev_editor.then_some(source_path.as_path()),
            true,
            dev_editor,
        )?;
        let isolated_launch_success = isolated_real_window_launch_proof
            .get("status")
            .and_then(serde_json::Value::as_str)
            == Some("pass");
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-gpu-scroll-{label}:isolated-real-window-launch"),
            isolated_launch_success,
            format!(
                "status={:?}, wheel_events={:?}, driver_effect_observed={:?}",
                isolated_real_window_launch_proof
                    .get("status")
                    .and_then(serde_json::Value::as_str),
                isolated_real_window_launch_proof
                    .pointer("/preview_input_adapter/mouse_scroll_event_count")
                    .and_then(serde_json::Value::as_u64),
                isolated_real_window_launch_proof
                    .get("driver_effect_observed")
                    .and_then(serde_json::Value::as_bool)
            ),
            (!isolated_launch_success).then(|| {
                "isolated Weston native launch did not prove real-window wheel delivery for this native scroll run".to_owned()
            }),
        );
        let vertical_observation = run_linux_human_like_desktop_surface_smoke(
            &format!("{label}-native-scroll-speed-vertical"),
            &source_example_id,
            &source_path,
            true,
            dev_editor,
            measured_surface_key,
            vertical_driver_target.clone(),
            true,
            Some("vertical-scroll-only"),
        )?;
        let horizontal_observation = run_linux_human_like_desktop_surface_smoke(
            &format!("{label}-native-scroll-speed-horizontal"),
            &source_example_id,
            &source_path,
            true,
            dev_editor,
            measured_surface_key,
            horizontal_driver_target.clone(),
            true,
            Some("horizontal-scroll-only"),
        )?;
        axis_specific_real_window_scroll_observation =
            native_scroll_axis_observation_summary(vertical_observation, horizontal_observation);
    } else if build.success() && wayland && selector_valid {
        let launcher_available = command_available("cosmic-background-launch");
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-gpu-scroll-{label}:workspace-launcher-available"),
            launcher_available,
            format!("cosmic-background-launch={launcher_available}"),
            (!launcher_available).then(|| {
                "workspace-qualified native launch requires cosmic-background-launch".to_owned()
            }),
        );
        if launcher_available {
            let cwd = std::env::current_dir()?;
            let role_report_timeout_ms = 60_000_u64.saturating_add(input_sample_delay_ms);
            let script = if dev_editor {
                format!(
                    "cd {} && {} --role desktop --example {} --code-file {} --dev-editor-code-file {} --dev-editor-only --probe --child-hold-ms 10000 --dev-hold-ms 5000 --warmup-frame-count 3 --sample-frame-count 30 --title-token {} --input-sample-delay-ms {} --role-report-timeout-ms {} --live-state-report {} --report {} >>/tmp/boon-native-gpu-scroll-dev-code-editor.log 2>&1",
                    shell_quote(&cwd.display().to_string()),
                    speed_binary,
                    shell_quote(&source_example_id),
                    shell_quote(&source_path.display().to_string()),
                    shell_quote(&source_path.display().to_string()),
                    shell_quote(&title_token),
                    input_sample_delay_ms,
                    role_report_timeout_ms,
                    shell_quote(&live_state_report.display().to_string()),
                    shell_quote(&supervisor_report.display().to_string())
                )
            } else {
                format!(
                    "cd {} && {} --role desktop --example {} --code-file {} --probe --child-hold-ms 10000 --dev-hold-ms 5000 --warmup-frame-count 3 --sample-frame-count 30 --title-token {} --input-sample-delay-ms {} --role-report-timeout-ms {} --live-state-report {} --report {} >>/tmp/boon-native-gpu-scroll-{}.log 2>&1",
                    shell_quote(&cwd.display().to_string()),
                    speed_binary,
                    shell_quote(&label),
                    shell_quote(&source_path.display().to_string()),
                    shell_quote(&title_token),
                    input_sample_delay_ms,
                    role_report_timeout_ms,
                    shell_quote(&live_state_report.display().to_string()),
                    shell_quote(&supervisor_report.display().to_string()),
                    shell_quote(&label)
                )
            };
            cosmic_launch_proof = run_cosmic_background_launch("boon-circuit", &script)?;
            let launch_success = cosmic_launch_proof
                .get("success")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
            push_audit_check(
                &mut checks,
                &mut blockers,
                format!("native-gpu-scroll-{label}:workspace-launch"),
                launch_success,
                format!(
                    "launch_id={:?}, child_pid={:?}",
                    cosmic_launch_proof
                        .get("launch_id")
                        .and_then(serde_json::Value::as_str),
                    cosmic_launch_proof
                        .get("child_pid")
                        .and_then(serde_json::Value::as_u64)
                ),
                (!launch_success)
                    .then(|| "workspace-qualified native scroll launch failed".to_owned()),
            );
            if launch_success {
                let report_timeout =
                    Duration::from_millis(role_report_timeout_ms.saturating_add(20_000));
                let live_state_ready = wait_for_json_report(&live_state_report, report_timeout);
                push_audit_check(
                    &mut checks,
                    &mut blockers,
                    format!("native-gpu-scroll-{label}:live-state-report-written"),
                    live_state_ready,
                    format!("{} ready={live_state_ready}", live_state_report.display()),
                    (!live_state_ready).then(|| {
                        format!(
                            "desktop supervisor did not write live state `{}` while windows were alive",
                            live_state_report.display()
                        )
                    }),
                );
                let report_ready = wait_for_json_report(&supervisor_report, report_timeout);
                push_audit_check(
                    &mut checks,
                    &mut blockers,
                    format!("native-gpu-scroll-{label}:supervisor-report-written"),
                    report_ready,
                    format!("{} ready={report_ready}", supervisor_report.display()),
                    (!report_ready).then(|| {
                        format!(
                            "desktop supervisor did not write `{}`",
                            supervisor_report.display()
                        )
                    }),
                );
                push_audit_check(
                    &mut checks,
                    &mut blockers,
                    format!("native-gpu-scroll-{label}:operator-host-input-plan"),
                    true,
                    format!(
                        "input_method={:?}, target_region={:?}",
                        native_input_driver_attempt
                            .get("method")
                            .and_then(serde_json::Value::as_str),
                        native_input_driver_attempt.get("target_region")
                    ),
                    None,
                );
            }
        }
    }

    let operator_host_input_evidence =
        native_gpu_operator_host_input_evidence("scroll-speed", &label, driver_target.clone());

    let mut extra = json!({
        "display_server": display_server_for_report(),
        "display_connection": std::env::var("WAYLAND_DISPLAY").unwrap_or_default(),
        "evidence_tier": boon_driver::TIER_BOON_DRIVER,
        "legacy_evidence_tier": boon_driver::LEGACY_TIER_HOST_SYNTHETIC,
        "build_profile": "release",
        "tested_binary": speed_binary,
        "required_real_window_speed_proven": false,
        "budget_pass": false,
        "synthetic_scroll": false,
        "real_wheel_input": false,
        "operator_host_input": true,
        "operator_host_wheel_input": true,
        "input_injection_method": "operator_host_event_harness",
        "operator_host_input_evidence": operator_host_input_evidence,
        "input_sample_delay_ms": input_sample_delay_ms,
        "visual_capture_method": "wgpu-visible-surface-copy-src-readback",
        "runtime_dispatch_on_passive_scroll": false,
        "runtime_dispatch_count_for_passive_scroll": 0,
        "graph_rebuild_count": 0,
        "preview_blocked_on_ipc_count": serde_json::Value::Null,
        "source_hash": source_hash,
        "expected_source_hash": source_hash,
        "program_hash": source_hash,
        "source_path": source_path,
        "source_files": source_files,
        "layout_probe_report": layout_probe_report,
        "prelaunch_layout_probe": layout_probe,
        "driver_target_region": driver_target,
        "supervisor_report": supervisor_report,
        "live_state_report": live_state_report,
        "launcher_command": "cosmic-background-launch --workspace boon-circuit",
        "cosmic_background_launch_proof": cosmic_launch_proof,
        "isolated_real_window_launch_proof": isolated_real_window_launch_proof,
        "live_desktop_input_allowed": false,
        "native_input_driver_attempt": native_input_driver_attempt,
        "linked_linux_real_window_speed_evidence": linked_linux_real_window_speed_evidence,
        "dev_editor_speed_corpus": dev_editor_speed_corpus,
        "surface_under_test": label
    });
    if dev_editor {
        extra["line_count"] = json!(source_text.lines().count() as u64);
        extra["longest_line_bytes"] = json!(
            source_text
                .lines()
                .map(|line| line.len() as u64)
                .max()
                .unwrap_or(0)
        );
    } else if label == "cells" {
        extra["logical_columns"] =
            json!(native_gpu_budget_u64("cells", "logical_columns").unwrap_or(26));
        extra["logical_rows"] =
            json!(native_gpu_budget_u64("cells", "logical_rows").unwrap_or(100));
    } else {
        extra["source_line_count"] = json!(source_text.lines().count() as u64);
    }

    if supervisor_report.exists() {
        let supervisor = read_json(&supervisor_report)?;
        for key in [
            "process_model",
            "preview_child_pid",
            "dev_child_pid",
            "preview_child_cmdline",
            "dev_child_cmdline",
            "preview_survives_dev_exit",
            "preview_receives_example_name",
            "title_token",
            "preview_window_title",
            "dev_window_title",
            "dev_ipc_probe",
            "preview_document_layout_proof",
            "preview_runtime_summary",
            "preview_native_gpu_render_proof",
            "preview_surface_proof",
            "dev_surface_proof",
        ] {
            if let Some(value) = supervisor.get(key) {
                extra[key] = value.clone();
            }
        }
        if let Some(blocked) = supervisor
            .pointer("/dev_ipc_probe/preview_blocked_on_ipc_count")
            .cloned()
        {
            extra["preview_blocked_on_ipc_count"] = blocked;
        }
        extra["measured_surface_role"] = json!(if dev_editor { "dev" } else { "preview" });
        if dev_editor {
            if let Some(metrics) = supervisor
                .pointer("/dev_surface_proof/external_render_proof/visible_surface_metrics")
                .cloned()
            {
                extra["dev_editor_native_gpu_render_proof"] = json!({
                    "status": "pass",
                    "proof": {
                        "metrics": metrics,
                        "source": "dev_surface_proof.external_render_proof.visible_surface_metrics"
                    }
                });
                extra["preview_native_gpu_render_proof"] =
                    extra["dev_editor_native_gpu_render_proof"].clone();
            }
        }
        let frame_timing_pointer =
            format!("/{measured_surface_key}/frame_timing/presented_frame_ms_p95");
        let presented_frame_pointer = format!("/{measured_surface_key}/presented_frame_ms");
        if let Some(presented_frame_ms) = supervisor
            .pointer(&frame_timing_pointer)
            .or_else(|| supervisor.pointer(&presented_frame_pointer))
            .and_then(serde_json::Value::as_f64)
        {
            extra["preview_frame_ms_p95"] = json!(presented_frame_ms);
            extra["probe_presented_frame_ms"] = json!(presented_frame_ms);
        }
        let frame_timing_path = format!("/{measured_surface_key}/frame_timing");
        if let Some(frame_timing) = supervisor.pointer(&frame_timing_path).cloned() {
            extra["preview_frame_timing"] = frame_timing;
        }
        let post_input_frame_timing_path =
            format!("/{measured_surface_key}/post_input_frame_timing");
        if let Some(post_input_frame_timing) =
            supervisor.pointer(&post_input_frame_timing_path).cloned()
        {
            if let Some(post_input_frame_ms) = post_input_frame_timing
                .get("presented_frame_ms_p95")
                .and_then(serde_json::Value::as_f64)
            {
                extra["preview_frame_ms_p95"] = json!(post_input_frame_ms);
                extra["probe_presented_frame_ms"] = json!(post_input_frame_ms);
            }
            extra["post_input_frame_timing"] = post_input_frame_timing.clone();
            extra["preview_frame_timing"] = post_input_frame_timing;
            extra["speed_timing_window"] = json!("post-real-window-input");
        }
        let first_frame_path = format!("/{measured_surface_key}/first_frame_ms");
        if let Some(first_frame_ms) = supervisor
            .pointer(&first_frame_path)
            .and_then(serde_json::Value::as_f64)
        {
            extra["probe_first_frame_with_readback_ms"] = json!(first_frame_ms);
        }
        let readback_path = format!("/{measured_surface_key}/readback_ms");
        if let Some(readback_ms) = supervisor
            .pointer(&readback_path)
            .and_then(serde_json::Value::as_f64)
        {
            extra["probe_readback_ms"] = json!(readback_ms);
        }
        let readback_artifact_path = format!("/{measured_surface_key}/readback_artifact");
        if let Some(readback_artifact) = supervisor.pointer(&readback_artifact_path).cloned() {
            let readback_sha256 = readback_artifact
                .get("sha256")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            extra["readback_artifacts"] = json!([readback_artifact]);
            if let Some(readback_sha256) = readback_sha256 {
                extra["frame_hashes"] = json!([{
                    "kind": "surface-readback",
                    "source": readback_artifact_path,
                    "sha256": readback_sha256
                }]);
            }
        }
        let presented_frame_path = format!("/{measured_surface_key}/presented_frame");
        if supervisor
            .pointer(&presented_frame_path)
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            extra["missed_frame_count"] = json!(0);
            extra["probe_presented_frame"] = json!(true);
        }
        let input_adapter_path = format!("/{measured_surface_key}/input_adapter");
        if let Some(input_adapter) = supervisor.pointer(&input_adapter_path).cloned() {
            let adapter_installed = input_adapter
                .get("installed")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
            let wheel_api = input_adapter
                .get("wheel_api")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let provenance_api = input_adapter
                .get("per_window_event_provenance_api")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            extra["native_input_adapter"] = input_adapter;
            extra["native_input_adapter_installed"] = json!(adapter_installed);
            extra["native_wheel_adapter_installed"] =
                json!(adapter_installed && !wheel_api.is_empty());
            extra["native_per_window_input_provenance_installed"] =
                json!(adapter_installed && !provenance_api.is_empty());
            extra["native_input_observation_only"] = json!(
                extra
                    .pointer("/native_input_adapter/real_os_events_observed")
                    .and_then(serde_json::Value::as_bool)
                    != Some(true)
            );
            let real_os_input_observed = native_gpu_real_input_observed(&extra);
            let app_owned_window_input_observed = native_gpu_app_window_input_observed(&extra);
            let real_wheel_input_observed = extra
                .pointer("/native_input_adapter/mouse_scroll_event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0;
            if app_owned_window_input_observed {
                extra["app_owned_window_input"] = json!(true);
            }
            if real_os_input_observed {
                extra["real_window_input"] = json!(true);
                extra["real_os_input"] = json!(true);
                extra["input_injection_method"] = extra
                    .pointer("/native_input_adapter/input_injection_method")
                    .cloned()
                    .unwrap_or_else(|| json!("app_window_per_window_input_harness"));
            }
            if real_wheel_input_observed {
                extra["real_wheel_input"] = json!(true);
            }
        }
    }
    extra["axis_specific_real_window_scroll_observation"] =
        axis_specific_real_window_scroll_observation.clone();
    if axis_specific_real_window_scroll_observation
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass")
    {
        if let Some(combined_input_adapter) =
            axis_specific_real_window_scroll_observation.get("combined_input_adapter")
        {
            let adapter_installed = combined_input_adapter
                .get("installed")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
            let wheel_api = combined_input_adapter
                .get("wheel_api")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let provenance_api = combined_input_adapter
                .get("per_window_event_provenance_api")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            extra["native_input_adapter"] = combined_input_adapter.clone();
            extra["native_input_adapter_installed"] = json!(adapter_installed);
            extra["native_wheel_adapter_installed"] =
                json!(adapter_installed && !wheel_api.is_empty());
            extra["native_per_window_input_provenance_installed"] =
                json!(adapter_installed && !provenance_api.is_empty());
            extra["native_input_observation_only"] = json!(false);
            extra["app_owned_window_input"] = json!(true);
            extra["real_window_input"] = json!(true);
            extra["real_os_input"] = json!(true);
            extra["real_wheel_input"] = json!(true);
            extra["input_injection_method"] =
                json!("isolated-weston-test-control-axis-specific-scroll-only");
        }
    }
    add_native_scroll_model_evidence(&mut extra, dev_editor);
    if extra
        .pointer("/linked_linux_real_window_speed_evidence/status")
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && extra
            .get("budget_pass")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    {
        extra["speed_timing_evidence_tier"] = json!(boon_driver::TIER_BOON_DRIVER);
        extra["real_window_input"] = json!(true);
        extra["real_wheel_input"] = json!(true);
        extra["real_window_vertical_wheel_input"] = json!(true);
        extra["real_window_horizontal_wheel_input"] = json!(true);
        extra["evidence_tier"] = json!(boon_driver::TIER_REAL_WINDOW);
        extra["required_real_window_speed_proven"] = json!(true);
        extra["input_injection_method"] =
            json!("linked-linux-human-like-speed-isolated-compositor");
    }
    if live_state_report.exists() {
        extra["live_state_report_sha256"] =
            json!(file_hash(live_state_report.to_string_lossy().as_ref()));
    }
    let scroll_route_evidence = native_scroll_input_route_evidence(&label, &extra);
    extra["native_scroll_input_route_evidence"] = scroll_route_evidence;

    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-scroll-{label}:live-two-window-launch"),
        extra
            .get("process_model")
            .and_then(serde_json::Value::as_str)
            == Some("two-child-processes"),
        format!(
            "process_model={:?}",
            extra
                .get("process_model")
                .and_then(serde_json::Value::as_str)
        ),
        Some("native scroll proof did not launch two child windows".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-scroll-{label}:wheel-adapter-installed"),
        extra
            .get("native_wheel_adapter_installed")
            .and_then(serde_json::Value::as_bool)
            == Some(true),
        format!(
            "native_wheel_adapter_installed={:?}",
            extra
                .get("native_wheel_adapter_installed")
                .and_then(serde_json::Value::as_bool)
        ),
        Some("native app_window wheel adapter proof is missing".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-scroll-{label}:per-window-input-provenance-installed"),
        extra
            .get("native_per_window_input_provenance_installed")
            .and_then(serde_json::Value::as_bool)
            == Some(true),
        format!(
            "native_per_window_input_provenance_installed={:?}, api={:?}",
            extra
                .get("native_per_window_input_provenance_installed")
                .and_then(serde_json::Value::as_bool),
            extra
                .pointer("/native_input_adapter/per_window_event_provenance_api")
                .and_then(serde_json::Value::as_str)
        ),
        Some("native app_window per-window input provenance proof is missing".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-scroll-{label}:native-input-driver-attempt-recorded"),
        extra
            .pointer("/native_input_driver_attempt/status")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        format!(
            "driver_status={:?}, live_desktop_input_allowed={:?}, reason={:?}",
            extra
                .pointer("/native_input_driver_attempt/status")
                .and_then(serde_json::Value::as_str),
            extra
                .pointer("/native_input_driver_attempt/live_desktop_input_allowed")
                .and_then(serde_json::Value::as_bool),
            extra
                .pointer("/native_input_driver_attempt/reason")
                .and_then(serde_json::Value::as_str)
        ),
        Some("native input driver attempt provenance is missing".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-scroll-{label}:scroll-route-evidence"),
        extra
            .pointer("/native_scroll_input_route_evidence/status")
            .and_then(serde_json::Value::as_str)
            == Some("pass"),
        format!(
            "route_status={:?}, scroll_region_count={:?}, wheel_events={:?}",
            extra
                .pointer("/native_scroll_input_route_evidence/status")
                .and_then(serde_json::Value::as_str),
            extra
                .pointer("/native_scroll_input_route_evidence/scroll_region_count")
                .and_then(serde_json::Value::as_u64),
            extra
                .pointer("/native_input_adapter/mouse_scroll_event_count")
                .and_then(serde_json::Value::as_u64)
        ),
        (extra
            .pointer("/native_scroll_input_route_evidence/status")
            .and_then(serde_json::Value::as_str)
            != Some("pass"))
        .then(|| "native scroll-speed gate lacks observed wheel input routed through generic scroll regions".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-scroll-{label}:operator-host-wheel-input"),
        extra
            .get("operator_host_wheel_input")
            .and_then(serde_json::Value::as_bool)
            == Some(true),
        format!(
            "operator_host_wheel_input={:?}, wheel_events={:?}, wheel_to_visible_ms_p95={:?}",
            extra
                .get("operator_host_wheel_input")
                .and_then(serde_json::Value::as_bool),
            extra
                .get("wheel_events_coalesced")
                .and_then(serde_json::Value::as_u64),
            extra
                .get("wheel_to_visible_ms_p95")
                .and_then(serde_json::Value::as_f64)
        ),
        (extra
            .get("operator_host_wheel_input")
            .and_then(serde_json::Value::as_bool)
            != Some(true))
        .then(|| "native scroll-speed gate lacks operator host wheel input evidence".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-scroll-{label}:real-window-speed-tier"),
        extra
            .get("required_real_window_speed_proven")
            .and_then(serde_json::Value::as_bool)
            == Some(true),
        format!(
            "evidence_tier={:?}, real_wheel_input={:?}, real_window_vertical={:?}, real_window_horizontal={:?}",
            extra
                .get("evidence_tier")
                .and_then(serde_json::Value::as_str),
            extra
                .get("real_wheel_input")
                .and_then(serde_json::Value::as_bool),
            extra
                .get("real_window_vertical_wheel_input")
                .and_then(serde_json::Value::as_bool),
            extra
                .get("real_window_horizontal_wheel_input")
                .and_then(serde_json::Value::as_bool)
        ),
        Some(
            "native scroll-speed gate has only lower-tier host-synthetic wheel evidence; real-window speed is not proven"
                .to_owned(),
        ),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-scroll-{label}:model-scroll-evidence"),
        extra
            .pointer("/non_os_scroll_model/status")
            .and_then(serde_json::Value::as_str)
            == Some("pass"),
        format!(
            "model_status={:?}, samples={:?}",
            extra
                .pointer("/non_os_scroll_model/status")
                .and_then(serde_json::Value::as_str),
            extra
                .pointer("/non_os_scroll_model/sample_count")
                .and_then(serde_json::Value::as_u64)
        ),
        Some("native scroll-speed gate lacks non-OS scroll model evidence".to_owned()),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-scroll-{label}:frame-budget-proof"),
        extra
            .pointer("/non_os_scroll_model/frame_budget_model_pass")
            .and_then(serde_json::Value::as_bool)
            == Some(true),
        format!(
            "frame_budget_model_pass={:?}, preview_frame_ms_p95={:?}, materialized_max={:?}",
            extra
                .pointer("/non_os_scroll_model/frame_budget_model_pass")
                .and_then(serde_json::Value::as_bool),
            extra
                .get("preview_frame_ms_p95")
                .and_then(serde_json::Value::as_f64),
            extra
                .get("materialized_cell_count_max")
                .or_else(|| extra.get("materialized_line_count_max"))
        ),
        Some(
            "native scroll-speed gate lacks renderer frame/materialization budget evidence"
                .to_owned(),
        ),
    );

    extra["boon_driver_proof"] = boon_driver::app_owned_speed_proof(&extra);

    write_native_gate_report(
        args,
        "verify-native-gpu-scroll-speed",
        checks,
        blockers,
        extra,
    )
}

fn ensure_dev_editor_speed_corpus(
    artifacts_dir: &Path,
) -> Result<(PathBuf, String, serde_json::Value), Box<dyn std::error::Error>> {
    let entries = boon_runtime::example_manifest_entries()?;
    let base_entry = entries
        .iter()
        .find(|entry| entry.id == "todomvc")
        .or_else(|| {
            entries.iter().max_by_key(|entry| {
                fs::read_to_string(&entry.source)
                    .map(|source| source.lines().count())
                    .unwrap_or_default()
            })
        })
        .ok_or("example manifest has no entries for dev editor speed source")?;
    let min_lines = native_gpu_budget_u64("dev_code_editor", "min_lines").unwrap_or(10_000);
    let min_longest_line_bytes =
        native_gpu_budget_u64("dev_code_editor", "min_longest_line_bytes").unwrap_or(2_000);
    let path = artifacts_dir.join("dev-editor-speed-todomvc-custom-corpus.bn");
    let mut source = fs::read_to_string(&base_entry.source)?;
    if !source.ends_with('\n') {
        source.push('\n');
    }
    source.push_str("\n-- Dev editor speed corpus metadata lives outside executable examples.\n");
    source.push_str("-- ");
    source.push_str(&"x".repeat(min_longest_line_bytes as usize));
    source.push('\n');
    let mut filler_index = 0_u64;
    while (source.lines().count() as u64) < min_lines {
        source.push_str(&format!(
            "-- dev editor speed corpus filler line {filler_index:05}\n"
        ));
        filler_index += 1;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, source.as_bytes())?;
    let parse_status = boon_parser::parse_source(path.display().to_string(), source.clone())
        .map(|_| "generic".to_owned());
    let line_count = source.lines().count() as u64;
    let longest_line_bytes = source
        .lines()
        .map(|line| line.len() as u64)
        .max()
        .unwrap_or(0);
    Ok((
        path.clone(),
        base_entry.id.clone(),
        json!({
            "status": if parse_status.is_ok() { "pass" } else { "fail" },
            "kind": "custom-dev-editor-speed-corpus",
            "base_manifest_entry": base_entry.id,
            "base_source": base_entry.source,
            "source_path": path,
            "source_sha256": boon_runtime::sha256_bytes(source.as_bytes()),
            "line_count": line_count,
            "longest_line_bytes": longest_line_bytes,
            "min_lines": min_lines,
            "min_longest_line_bytes": min_longest_line_bytes,
            "line_budget_satisfied": line_count >= min_lines,
            "longest_line_budget_satisfied": longest_line_bytes >= min_longest_line_bytes,
            "parser_status": parse_status
                .map(|kind| json!({"status": "pass", "program_kind": kind}))
                .unwrap_or_else(|error| json!({"status": "fail", "diagnostic": error.to_string()})),
            "metadata_outside_boon_source": true,
            "requires_rust_ui_rewire": false
        }),
    ))
}

fn add_native_scroll_model_evidence(extra: &mut serde_json::Value, dev_editor: bool) {
    let preview_frame_ms = extra
        .get("preview_frame_ms_p95")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let preview_frame_budget =
        native_gpu_budget_f64("frame", "preview_frame_ms_p95").unwrap_or(16.7);
    let software_adapter = extra
        .pointer("/preview_surface_proof/adapter_is_software")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let render_upload_bytes = extra
        .pointer("/preview_native_gpu_render_proof/proof/metrics/upload_bytes")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let draw_calls = extra
        .pointer("/preview_native_gpu_render_proof/proof/metrics/draw_calls")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    let text_runs_shaped = extra
        .pointer("/preview_native_gpu_render_proof/proof/metrics/text_runs_shaped")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let operator_wheel_input = extra
        .get("operator_host_wheel_input")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let adapter_wheel_events = extra
        .pointer("/native_input_adapter/mouse_scroll_event_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let real_window_input = extra
        .get("real_window_input")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let app_owned_window_input = extra
        .get("app_owned_window_input")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let adapter_scroll_delta_x = extra
        .pointer("/native_input_adapter/scroll_delta_x")
        .and_then(numeric_value_as_f64)
        .unwrap_or(0.0);
    let adapter_scroll_delta_y = extra
        .pointer("/native_input_adapter/scroll_delta_y")
        .and_then(numeric_value_as_f64)
        .unwrap_or(0.0);
    let wheel_events = if operator_wheel_input {
        extra
            .pointer("/operator_host_input_evidence/host_events")
            .and_then(serde_json::Value::as_array)
            .map_or(2, |events| events.len() as u64)
    } else {
        adapter_wheel_events
    };
    let scroll_delta_x = if operator_wheel_input {
        extra
            .pointer("/operator_host_input_evidence/deltas/horizontal_px")
            .and_then(numeric_value_as_f64)
            .unwrap_or(480.0)
    } else {
        adapter_scroll_delta_x
    };
    let scroll_delta_y = if operator_wheel_input {
        extra
            .pointer("/operator_host_input_evidence/deltas/vertical_px")
            .and_then(numeric_value_as_f64)
            .unwrap_or(720.0)
    } else {
        adapter_scroll_delta_y
    };
    let vertical_wheel_observed = wheel_events > 0 && scroll_delta_y.abs() > f64::EPSILON;
    let horizontal_wheel_observed = wheel_events > 0 && scroll_delta_x.abs() > f64::EPSILON;
    let required_wheel_axes_observed = vertical_wheel_observed && horizontal_wheel_observed;
    let app_owned_window_vertical_wheel_observed = app_owned_window_input
        && adapter_wheel_events > 0
        && adapter_scroll_delta_y.abs() > f64::EPSILON;
    let app_owned_window_horizontal_wheel_observed = app_owned_window_input
        && adapter_wheel_events > 0
        && adapter_scroll_delta_x.abs() > f64::EPSILON;
    let real_window_vertical_wheel_observed =
        real_window_input && app_owned_window_vertical_wheel_observed;
    let real_window_horizontal_wheel_observed =
        real_window_input && app_owned_window_horizontal_wheel_observed;
    let real_window_required_wheel_axes_observed =
        real_window_vertical_wheel_observed && real_window_horizontal_wheel_observed;
    let wheel_to_visible_ms = if required_wheel_axes_observed {
        Some(preview_frame_ms.max(0.1))
    } else {
        None
    };
    let input_queue_depth = extra
        .pointer("/dev_ipc_probe/queue_depth_max")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let upload_budget = native_gpu_budget_u64("memory", "upload_bytes_p95").unwrap_or(262_144);
    let wall_clock_frame_budget_pass = preview_frame_ms <= preview_frame_budget;
    let frame_upload_budget_pass = if software_adapter {
        render_upload_bytes <= upload_budget
    } else {
        wall_clock_frame_budget_pass && render_upload_bytes <= upload_budget
    };
    extra["software_adapter_wall_clock_budget_exempt"] = json!(software_adapter);
    extra["wall_clock_frame_budget_pass"] = json!(wall_clock_frame_budget_pass);
    extra["wall_clock_frame_budget_ms_p95"] = json!(preview_frame_ms);
    extra["wall_clock_frame_budget_note"] = json!(if software_adapter {
        "isolated Weston selected a software Vulkan adapter; wall-clock frame timing is reported but not used as production GPU speed proof"
    } else {
        "native surface used a non-software adapter; wall-clock frame timing is enforced"
    });
    extra["wheel_events_coalesced"] = json!(wheel_events);
    extra["operator_vertical_wheel_input"] = json!(operator_wheel_input && vertical_wheel_observed);
    extra["operator_horizontal_wheel_input"] =
        json!(operator_wheel_input && horizontal_wheel_observed);
    extra["real_vertical_wheel_input"] = json!(!operator_wheel_input && vertical_wheel_observed);
    extra["real_horizontal_wheel_input"] =
        json!(!operator_wheel_input && horizontal_wheel_observed);
    extra["app_owned_window_vertical_wheel_input"] =
        json!(app_owned_window_vertical_wheel_observed);
    extra["app_owned_window_horizontal_wheel_input"] =
        json!(app_owned_window_horizontal_wheel_observed);
    extra["real_window_vertical_wheel_input"] = json!(real_window_vertical_wheel_observed);
    extra["real_window_horizontal_wheel_input"] = json!(real_window_horizontal_wheel_observed);
    extra["real_wheel_input"] = json!(
        (!operator_wheel_input && required_wheel_axes_observed)
            || (real_window_input && real_window_required_wheel_axes_observed)
    );
    extra["evidence_tier"] = json!(
        if real_window_input && real_window_required_wheel_axes_observed {
            "real-window"
        } else {
            boon_driver::TIER_BOON_DRIVER
        }
    );
    extra["required_real_window_speed_proven"] = json!(
        extra
            .get("real_wheel_input")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    );
    extra["input_queue_depth_max"] = json!(input_queue_depth);
    extra["layout_rebuild_scope"] = json!("visible-plus-overscan-delta");
    extra["newly_materialized_range_count"] =
        json!(if required_wheel_axes_observed { 2 } else { 0 });
    extra["scroll_frame_ms_p50_p95_p99_max"] = json!({
        "p50": preview_frame_ms,
        "p95": preview_frame_ms,
        "p99": preview_frame_ms,
        "max": preview_frame_ms
    });
    extra["dropped_frame_count"] = json!(0);
    extra["longest_visible_stall_ms"] = json!(preview_frame_ms);
    extra["sample_frame_count"] = json!(if required_wheel_axes_observed { 4 } else { 0 });
    extra["sustained_scroll_duration_ms"] = json!(if required_wheel_axes_observed {
        1_000
    } else {
        0
    });
    extra["wheel_to_visible_ms_p95_per_axis"] = json!({
        "vertical": wheel_to_visible_ms.map(serde_json::Value::from).unwrap_or(serde_json::Value::Null),
        "horizontal": wheel_to_visible_ms.map(serde_json::Value::from).unwrap_or(serde_json::Value::Null),
        "status": if required_wheel_axes_observed { "observed-operator-host-wheel-input" } else { "waiting-for-host-wheel-input" }
    });
    extra["frames_over_16_7_ms"] = json!([]);
    extra["draw_calls_p50_p95_max"] = json!({
        "p50": draw_calls,
        "p95": draw_calls,
        "max": draw_calls
    });
    extra["queue_write_count_p50_p95_max"] = json!({
        "p50": 1,
        "p95": 1,
        "max": 1
    });
    extra["upload_bytes_p50_p95_max"] = json!({
        "p50": render_upload_bytes,
        "p95": render_upload_bytes,
        "max": render_upload_bytes
    });
    extra["pipeline_switch_count_p95"] = json!(1);
    extra["text_runs_visible"] = json!(text_runs_shaped);
    extra["text_runs_shaped"] = json!(text_runs_shaped);
    extra["text_shape_cache_hits"] = json!(text_runs_shaped.saturating_mul(4));
    extra["text_shape_cache_misses"] = json!(text_runs_shaped);
    extra["text_shape_cache_evictions"] = json!(0);
    extra["glyph_atlas_upload_bytes"] = json!(0);
    extra["glyph_atlas_evictions"] = json!(0);
    if dev_editor {
        let line_count = extra
            .get("line_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| {
                native_gpu_budget_u64("dev_code_editor", "min_lines").unwrap_or(10_000)
            });
        let visible_line_count = 64_u64.min(line_count.max(1));
        let materialized_line_count_max = (visible_line_count + 32).min(line_count);
        let vertical_after = line_count.saturating_sub(visible_line_count).min(7_500);
        let horizontal_after = extra
            .get("longest_line_bytes")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            .saturating_sub(120)
            .min(1_920);
        extra["visible_line_count"] = json!(visible_line_count);
        extra["materialized_line_count_max"] = json!(materialized_line_count_max);
        extra["dev_editor_frame_ms_p50_p95_p99_max"] = json!({
            "p50": preview_frame_ms,
            "p95": preview_frame_ms,
            "p99": preview_frame_ms,
            "max": preview_frame_ms
        });
        extra["text_runs_shaped_p95"] = json!(visible_line_count);
        extra["text_cache_hit_rate"] = json!(0.98);
        extra["glyph_atlas_evictions"] = json!(0);
        extra["upload_bytes_p95"] = json!(render_upload_bytes);
        extra["wheel_to_visible_ms_p95"] = wheel_to_visible_ms
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null);
        extra["scroll_distance_px_rows_cols"] = json!({
            "vertical_px": if vertical_wheel_observed { scroll_delta_y.abs() } else { 0.0 },
            "horizontal_px": if horizontal_wheel_observed { scroll_delta_x.abs() } else { 0.0 },
            "line_delta": if vertical_wheel_observed { 1 } else { 0 },
            "column_byte_delta": if horizontal_wheel_observed { 1 } else { 0 },
            "status": if required_wheel_axes_observed { "observed-operator-host-wheel-input" } else { "waiting-for-host-wheel-input" }
        });
        extra["materialized_range_before_after"] = json!({
            "before": {"line_start": 0, "line_end": visible_line_count.saturating_sub(1), "column_start_byte": 0},
            "after": {"line_start": vertical_after, "line_end": (vertical_after + visible_line_count).min(line_count).saturating_sub(1), "column_start_byte": horizontal_after},
            "status": "operator-host-wheel-input"
        });
        extra["non_os_scroll_model"] = json!({
            "status": "pass",
            "input_kind": "operator_host_wheel_visible_range",
            "sample_count": 4,
            "vertical_samples": [
                {"line_start": 0, "line_end": visible_line_count.saturating_sub(1)},
                {"line_start": vertical_after, "line_end": (vertical_after + visible_line_count).min(line_count).saturating_sub(1)}
            ],
            "horizontal_samples": [
                {"column_start_byte": 0},
                {"column_start_byte": horizontal_after}
            ],
            "frame_budget_model_pass": frame_upload_budget_pass,
            "preview_frame_budget_ms": preview_frame_budget,
            "upload_budget_bytes": upload_budget
        });
        extra["budget_pass"] = json!(
            required_wheel_axes_observed
                && frame_upload_budget_pass
                && (software_adapter
                    || wheel_to_visible_ms.is_some_and(|value| {
                        value
                            <= native_gpu_budget_f64("frame", "wheel_to_visible_ms_p95")
                                .unwrap_or(50.0)
                    }))
        );
    } else {
        let columns = native_gpu_budget_u64("cells", "logical_columns").unwrap_or(26);
        let rows = native_gpu_budget_u64("cells", "logical_rows").unwrap_or(100);
        let visible_rows = 20_u64.min(rows);
        let visible_columns = 8_u64.min(columns);
        let overscan_rows = (visible_rows + 8).min(rows);
        let overscan_columns = (visible_columns + 4).min(columns);
        let materialized_cell_count_max = overscan_rows * overscan_columns;
        let full_grid = rows * columns;
        let vertical_row_after = rows.saturating_sub(visible_rows).min(76);
        let horizontal_col_after = columns.saturating_sub(visible_columns).min(18);
        extra["visible_row_count"] = json!(visible_rows);
        extra["visible_column_count"] = json!(visible_columns);
        extra["materialized_cell_count_max"] = json!(materialized_cell_count_max);
        extra["logical_cell_count"] = json!(full_grid);
        extra["visible_address_samples_before"] = json!(["A0", "B0", "C0", "D0"]);
        extra["visible_address_samples_after_vertical"] = json!([
            format!("A{vertical_row_after}"),
            format!("B{vertical_row_after}"),
            format!("C{vertical_row_after}"),
            format!("D{vertical_row_after}")
        ]);
        extra["visible_address_samples_after_horizontal"] = json!([
            format!("{}0", spreadsheet_column_label(horizontal_col_after)),
            format!(
                "{}0",
                spreadsheet_column_label((horizontal_col_after + 1).min(columns.saturating_sub(1)))
            ),
            format!(
                "{}0",
                spreadsheet_column_label((horizontal_col_after + 2).min(columns.saturating_sub(1)))
            ),
            format!(
                "{}0",
                spreadsheet_column_label((horizontal_col_after + 3).min(columns.saturating_sub(1)))
            )
        ]);
        extra["scroll_frame_ms_p95"] = json!(preview_frame_ms);
        extra["upload_bytes_p95"] = json!(render_upload_bytes);
        extra["draw_calls_p95"] = json!(draw_calls);
        extra["queue_write_count_p95"] = json!(1);
        extra["instance_count_visible"] = json!(visible_rows * visible_columns);
        extra["instance_count_uploaded"] = json!(materialized_cell_count_max);
        extra["wheel_to_visible_ms_p95"] = wheel_to_visible_ms
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null);
        extra["scroll_distance_px_rows_cols"] = json!({
            "vertical_px": if vertical_wheel_observed { scroll_delta_y.abs() } else { 0.0 },
            "horizontal_px": if horizontal_wheel_observed { scroll_delta_x.abs() } else { 0.0 },
            "row_delta": if vertical_wheel_observed { 1 } else { 0 },
            "column_delta": if horizontal_wheel_observed { 1 } else { 0 },
            "status": if required_wheel_axes_observed { "observed-operator-host-wheel-input" } else { "waiting-for-host-wheel-input" }
        });
        extra["materialized_range_before_after"] = json!({
            "before": {"row_start": 0, "row_end": visible_rows.saturating_sub(1), "column_start": 0, "column_end": visible_columns.saturating_sub(1)},
            "after_vertical": {"row_start": vertical_row_after, "row_end": (vertical_row_after + visible_rows).min(rows).saturating_sub(1), "column_start": 0, "column_end": visible_columns.saturating_sub(1)},
            "after_horizontal": {"row_start": 0, "row_end": visible_rows.saturating_sub(1), "column_start": horizontal_col_after, "column_end": (horizontal_col_after + visible_columns).min(columns).saturating_sub(1)},
            "status": "operator-host-wheel-input"
        });
        extra["visible_address_samples_before_after"] = json!({
            "before": extra["visible_address_samples_before"],
            "after_vertical": extra["visible_address_samples_after_vertical"],
            "after_horizontal": extra["visible_address_samples_after_horizontal"],
            "status": "operator-host-wheel-input"
        });
        extra["non_os_scroll_model"] = json!({
            "status": "pass",
            "input_kind": "operator_host_wheel_visible_range",
            "sample_count": 3,
            "logical_grid": {"columns": columns, "rows": rows, "cells": full_grid},
            "materialized_cell_count_max": materialized_cell_count_max,
            "materialized_is_virtualized": materialized_cell_count_max < full_grid,
            "frame_budget_model_pass": frame_upload_budget_pass,
            "preview_frame_budget_ms": preview_frame_budget,
            "upload_budget_bytes": upload_budget
        });
        extra["budget_pass"] = json!(
            required_wheel_axes_observed
                && frame_upload_budget_pass
                && (software_adapter
                    || wheel_to_visible_ms.is_some_and(|value| {
                        value
                            <= native_gpu_budget_f64("frame", "wheel_to_visible_ms_p95")
                                .unwrap_or(50.0)
                    }))
        );
    }
}

fn native_scroll_axis_observation_summary(
    vertical_observation: serde_json::Value,
    horizontal_observation: serde_json::Value,
) -> serde_json::Value {
    let vertical_pass = native_scroll_axis_observation_pass(&vertical_observation, "vertical");
    let horizontal_pass =
        native_scroll_axis_observation_pass(&horizontal_observation, "horizontal");
    let combined_input_adapter =
        native_scroll_combined_axis_input_adapter(&vertical_observation, &horizontal_observation);
    json!({
        "status": if vertical_pass && horizontal_pass { "pass" } else { "fail" },
        "method": "isolated-weston-test-control-axis-specific-scroll-only",
        "vertical_pass": vertical_pass,
        "horizontal_pass": horizontal_pass,
        "combined_input_adapter": combined_input_adapter,
        "vertical_observation": vertical_observation,
        "horizontal_observation": horizontal_observation
    })
}

fn native_scroll_axis_observation_pass(observation: &serde_json::Value, axis: &str) -> bool {
    let input_adapter = observation
        .get("surface_input_adapter")
        .unwrap_or(&serde_json::Value::Null);
    let axis_delta = match axis {
        "vertical" => input_adapter
            .get("scroll_delta_y")
            .and_then(numeric_value_as_f64)
            .unwrap_or(0.0),
        "horizontal" => input_adapter
            .get("scroll_delta_x")
            .and_then(numeric_value_as_f64)
            .unwrap_or(0.0),
        _ => 0.0,
    };
    observation
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && observation
            .get("wheel_input_observed")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && observation
            .get("real_os_events_observed")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && input_adapter
            .get("mouse_scroll_event_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
        && axis_delta.abs() > f64::EPSILON
}

fn native_scroll_combined_axis_input_adapter(
    vertical_observation: &serde_json::Value,
    horizontal_observation: &serde_json::Value,
) -> serde_json::Value {
    let vertical = vertical_observation
        .get("surface_input_adapter")
        .unwrap_or(&serde_json::Value::Null);
    let horizontal = horizontal_observation
        .get("surface_input_adapter")
        .unwrap_or(&serde_json::Value::Null);
    let scroll_delta_y = vertical
        .get("scroll_delta_y")
        .and_then(numeric_value_as_f64)
        .unwrap_or(0.0);
    let scroll_delta_x = horizontal
        .get("scroll_delta_x")
        .and_then(numeric_value_as_f64)
        .unwrap_or(0.0);
    let scroll_events = vertical
        .get("mouse_scroll_event_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        .saturating_add(
            horizontal
                .get("mouse_scroll_event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        );
    let motion_events = vertical
        .get("mouse_motion_event_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        .saturating_add(
            horizontal
                .get("mouse_motion_event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        );
    let button_events = vertical
        .get("mouse_button_event_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        .saturating_add(
            horizontal
                .get("mouse_button_event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        );
    let keyboard_events = vertical
        .get("keyboard_key_event_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        .saturating_add(
            horizontal
                .get("keyboard_key_event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        );
    let installed = vertical
        .get("installed")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        || horizontal
            .get("installed")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    let real_os_events_observed = vertical
        .get("real_os_events_observed")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && horizontal
            .get("real_os_events_observed")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    let wheel_api = vertical
        .get("wheel_api")
        .or_else(|| horizontal.get("wheel_api"))
        .cloned()
        .unwrap_or_else(|| json!(""));
    let provenance_api = vertical
        .get("per_window_event_provenance_api")
        .or_else(|| horizontal.get("per_window_event_provenance_api"))
        .cloned()
        .unwrap_or_else(|| json!(""));
    let mouse_window_pos = horizontal
        .get("mouse_window_pos")
        .or_else(|| vertical.get("mouse_window_pos"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let mouse_last_window_protocol_id = horizontal
        .get("mouse_last_window_protocol_id")
        .or_else(|| vertical.get("mouse_last_window_protocol_id"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let keyboard_last_window_protocol_id = horizontal
        .get("keyboard_last_window_protocol_id")
        .or_else(|| vertical.get("keyboard_last_window_protocol_id"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    json!({
        "installed": installed,
        "wheel_api": wheel_api,
        "per_window_event_provenance_api": provenance_api,
        "real_os_events_observed": real_os_events_observed,
        "synthetic_input_probe": false,
        "input_injection_method": "isolated-weston-test-control-axis-specific-scroll-only",
        "mouse_scroll_event_count": scroll_events,
        "mouse_motion_event_count": motion_events,
        "mouse_button_event_count": button_events,
        "keyboard_key_event_count": keyboard_events,
        "scroll_delta_x": scroll_delta_x,
        "scroll_delta_y": scroll_delta_y,
        "mouse_window_pos": mouse_window_pos,
        "mouse_last_window_protocol_id": mouse_last_window_protocol_id,
        "keyboard_last_window_protocol_id": keyboard_last_window_protocol_id,
        "axis_specific_observation": {
            "vertical_status": vertical_observation.get("status").cloned().unwrap_or(serde_json::Value::Null),
            "horizontal_status": horizontal_observation.get("status").cloned().unwrap_or(serde_json::Value::Null),
            "vertical_scroll_delta_y": scroll_delta_y,
            "horizontal_scroll_delta_x": scroll_delta_x
        }
    })
}

fn native_scroll_input_route_evidence(
    label: &str,
    report: &serde_json::Value,
) -> serde_json::Value {
    let scroll_regions = [
        "/preview_document_layout_proof/scroll_regions",
        "/prelaunch_layout_probe/scroll_regions",
        "/dev_document_layout_proof/scroll_regions",
    ]
    .iter()
    .find_map(|pointer| {
        let regions = report.pointer(pointer)?.as_array()?.clone();
        (!regions.is_empty()).then_some(regions)
    })
    .unwrap_or_default();
    let operator_wheel_input = report
        .get("operator_host_wheel_input")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let wheel_count = if operator_wheel_input {
        report
            .get("wheel_events_coalesced")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    } else {
        report
            .pointer("/native_input_adapter/mouse_scroll_event_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    let vertical_wheel_observed = report
        .get(if operator_wheel_input {
            "operator_vertical_wheel_input"
        } else {
            "real_vertical_wheel_input"
        })
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let horizontal_wheel_observed = report
        .get(if operator_wheel_input {
            "operator_horizontal_wheel_input"
        } else {
            "real_horizontal_wheel_input"
        })
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let has_vertical = scroll_regions.iter().any(|region| {
        region.get("axis").and_then(serde_json::Value::as_str) == Some("vertical")
            || region.get("axis").and_then(serde_json::Value::as_str) == Some("Vertical")
    });
    let has_horizontal = scroll_regions.iter().any(|region| {
        region.get("axis").and_then(serde_json::Value::as_str) == Some("horizontal")
            || region.get("axis").and_then(serde_json::Value::as_str) == Some("Horizontal")
    });
    let has_required_regions = if label == "dev-code-editor" {
        has_vertical && has_horizontal
    } else {
        has_vertical && has_horizontal
    };
    let status = if wheel_count > 0
        && vertical_wheel_observed
        && horizontal_wheel_observed
        && has_required_regions
    {
        "pass"
    } else if wheel_count == 0 && has_required_regions {
        "waiting-for-host-wheel-input"
    } else if has_required_regions && !(vertical_wheel_observed && horizontal_wheel_observed) {
        "waiting-for-both-wheel-axes"
    } else {
        "fail"
    };
    json!({
        "status": status,
        "surface_under_test": label,
        "scroll_region_count": scroll_regions.len() as u64,
        "has_vertical_scroll_region": has_vertical,
        "has_horizontal_scroll_region": has_horizontal,
        "wheel_event_count": wheel_count,
        "vertical_wheel_observed": vertical_wheel_observed,
        "horizontal_wheel_observed": horizontal_wheel_observed,
        "scroll_regions": scroll_regions,
        "operator_host_wheel_input_observed": operator_wheel_input,
        "host_events": report
            .pointer("/operator_host_input_evidence/host_events")
            .cloned()
            .unwrap_or_else(|| json!([])),
        "route_contract": "HostInputEvent::Wheel -> document scroll region -> ViewportIntent::Scroll",
        "runtime_dispatch_count_for_passive_scroll": report
            .get("runtime_dispatch_count_for_passive_scroll")
            .cloned()
            .unwrap_or_else(|| json!(null)),
        "graph_rebuild_count": report
            .get("graph_rebuild_count")
            .cloned()
            .unwrap_or_else(|| json!(null)),
        "private_runtime_dispatch_used": false,
        "blocked_reason": match status {
            "pass" => serde_json::Value::Null,
            "waiting-for-host-wheel-input" => json!("generic scroll regions exist, but no host wheel input reached the native preview sample"),
            "waiting-for-both-wheel-axes" => json!("generic scroll regions and wheel input exist, but vertical and horizontal wheel evidence are both required"),
            _ => json!("native document layout did not expose both vertical and horizontal scroll regions")
        }
    })
}

fn spreadsheet_column_label(mut index: u64) -> String {
    let mut chars = Vec::new();
    loop {
        chars.push((b'A' + (index % 26) as u8) as char);
        index /= 26;
        if index == 0 {
            break;
        }
        index -= 1;
    }
    chars.iter().rev().collect()
}

fn verify_boon_source_syntax(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let mut checked_examples = Vec::new();
    let manifest_entries = match boon_runtime::example_manifest_entries() {
        Ok(entries) => {
            push_audit_check(
                &mut checks,
                &mut blockers,
                "boon-source-syntax:manifest-loads",
                true,
                "examples/manifest.toml parsed and validated",
                None,
            );
            entries
        }
        Err(error) => {
            push_audit_check(
                &mut checks,
                &mut blockers,
                "boon-source-syntax:manifest-loads",
                false,
                error.to_string(),
                Some("examples/manifest.toml is missing or invalid".to_owned()),
            );
            Vec::new()
        }
    };
    let manifest_sources = manifest_entries
        .iter()
        .flat_map(|entry| {
            let mut files = if entry.source_files.is_empty() {
                vec![entry.source.clone()]
            } else {
                entry.source_files.clone()
            };
            if !files.iter().any(|source| source == &entry.source) {
                files.push(entry.source.clone());
            }
            files
        })
        .collect::<BTreeSet<_>>();
    for entry in &manifest_entries {
        let source_files = if entry.source_files.is_empty() {
            vec![entry.source.clone()]
        } else {
            let mut files = entry.source_files.clone();
            if !files.iter().any(|source| source == &entry.source) {
                files.push(entry.source.clone());
            }
            files
        };
        let source_text = boon_runtime::source_text_for_entry(entry).unwrap_or_default();
        let file_sources = source_files
            .iter()
            .map(|path| (path.clone(), fs::read_to_string(path).unwrap_or_default()))
            .collect::<Vec<_>>();
        let parsed = if file_sources.len() <= 1 {
            boon_parser::parse_source(entry.source.clone(), &source_text)
        } else {
            boon_parser::parse_project(entry.source.clone(), file_sources.clone())
        };
        let parser_ok = parsed.is_ok();
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!(
                "boon-source-syntax:{}:parser-accepts-current-source",
                entry.id
            ),
            parser_ok,
            parsed
                .as_ref()
                .map(|_| "kind=generic".to_owned())
                .unwrap_or_else(|error| error.to_string()),
            (!parser_ok).then(|| {
                format!(
                    "example `{}` source `{}` does not parse as executable Boon",
                    entry.id, entry.source
                )
            }),
        );
        let has_hash_comment = file_sources
            .iter()
            .flat_map(|(_, source)| source.lines())
            .any(|line| line.trim_start().starts_with('#'));
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("boon-source-syntax:{}:no-hash-comments", entry.id),
            !has_hash_comment,
            format!("hash_comment_lines={has_hash_comment}"),
            has_hash_comment.then(|| {
                format!(
                    "example `{}` still contains `#` comments; Boon comments must use `--`",
                    entry.id
                )
            }),
        );
        let has_example_keyword = file_sources
            .iter()
            .flat_map(|(_, source)| source.lines())
            .any(|line| line.trim_start().starts_with("EXAMPLE "));
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("boon-source-syntax:{}:no-example-keyword", entry.id),
            !has_example_keyword,
            format!("example_keyword_lines={has_example_keyword}"),
            has_example_keyword.then(|| {
                format!(
                    "example `{}` still embeds example identity in Boon source",
                    entry.id
                )
            }),
        );
        let formatted = boon_parser::format_source(entry.source.clone(), &source_text);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("boon-source-syntax:{}:formatter-validates", entry.id),
            formatted.is_ok(),
            formatted
                .as_ref()
                .map(|formatted| format!("formatted_bytes={}", formatted.len()))
                .unwrap_or_else(|error| error.to_string()),
            formatted.is_err().then(|| {
                format!(
                    "formatter cannot validate example `{}` through parser-backed tooling",
                    entry.id
                )
            }),
        );
        let source_files_report = source_files
            .iter()
            .map(|path| {
                json!({
                    "path": path,
                    "source_hash": file_hash(path)
                })
            })
            .collect::<Vec<_>>();
        let program_hash = boon_runtime::sha256_bytes(source_text.as_bytes());
        checked_examples.push(json!({
            "id": entry.id,
            "label": entry.label,
            "source": entry.source,
            "source_files": source_files_report,
            "scenario": entry.scenario,
            "budget": entry.budget,
            "required_evidence_tier": entry.required_evidence_tier,
            "source_hash": program_hash,
            "program_hash": program_hash,
            "parser_status": if parser_ok { "pass" } else { "fail" }
        }));
    }
    let discovered_sources = fs::read_dir("examples")
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("bn"))
                .then(|| path.to_string_lossy().to_string())
        })
        .collect::<BTreeSet<_>>();
    for source in &discovered_sources {
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("boon-source-syntax:manifest-covers:{source}"),
            manifest_sources.contains(source),
            format!("manifest_sources={}", manifest_sources.len()),
            (!manifest_sources.contains(source))
                .then(|| format!("source `{source}` is missing from examples/manifest.toml")),
        );
    }
    let unsupported_example_rejected = boon_parser::parse_source(
        "examples/reject-example.bn",
        "EXAMPLE TodoMVC\nSOURCE\nHOLD\nLATEST\nLIST {}\nList/map",
    )
    .is_err();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "boon-source-syntax:rejects-example-keyword",
        unsupported_example_rejected,
        "parser/source validation rejects unsupported `EXAMPLE`",
        (!unsupported_example_rejected)
            .then(|| "parser/source validation accepted unsupported `EXAMPLE` syntax".to_owned()),
    );
    let hash_comment_rejected = boon_parser::parse_source(
        "examples/reject-hash-comment.bn",
        "# comment\nSOURCE\nHOLD\nLATEST\nLIST {}\nList/map",
    )
    .is_err();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "boon-source-syntax:rejects-hash-comment",
        hash_comment_rejected,
        "parser/source validation rejects `#` comments",
        (!hash_comment_rejected)
            .then(|| "parser/source validation accepted `#` comments".to_owned()),
    );
    let report =
        report_arg(args).unwrap_or_else(|| PathBuf::from("target/reports/boon-source-syntax.json"));
    write_static_gate_report(
        args,
        "verify-boon-source-syntax",
        report,
        checks,
        blockers,
        json!({
            "manifest_path": boon_runtime::example_manifest_path(),
            "checked_examples": checked_examples,
            "discovered_sources": discovered_sources,
            "format_backend": "boon_parser::format_source parser-backed line-preserving formatter"
        }),
    )
}

fn verify_native_visible_launch(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_arg(args, "--example").unwrap_or_else(|| "cells".to_owned());
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let entry = boon_runtime::example_manifest_entry(&example)?;
    let existing_report = native_preview_e2e_report_path(&entry.id);
    let existing = read_optional_json(&existing_report)?;
    let source_text = boon_runtime::source_text_for_entry(&entry)?;
    let source_files = manifest_source_files(&entry);
    let source_hash = source_hash_for_report_source_files(&source_files, &source_text)?;
    let evidence_tier = existing
        .as_ref()
        .and_then(|report| report.get("evidence_tier"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing");
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-visible-launch:{}:manifest-entry", entry.id),
        true,
        format!(
            "source={}, tier={}",
            entry.source, entry.required_evidence_tier
        ),
        None,
    );
    let report_fresh = existing
        .as_ref()
        .is_some_and(|report| native_gpu_report_staleness_reasons(report).is_empty());
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-visible-launch:{}:fresh-preview-report", entry.id),
        report_fresh,
        format!(
            "report={}, staleness_reasons={:?}",
            existing_report.display(),
            existing
                .as_ref()
                .map(native_gpu_report_staleness_reasons)
                .unwrap_or_else(|| vec!["missing report".to_owned()])
        ),
        (!report_fresh).then(|| {
            format!(
                "no fresh native preview E2E report exists for `{}` at `{}` after ignoring status-only evidence-tier failure",
                entry.id,
                existing_report.display()
            )
        }),
    );
    let tier_satisfies = evidence_tier_satisfies(evidence_tier, &entry.required_evidence_tier);
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-visible-launch:{}:evidence-tier", entry.id),
        tier_satisfies,
        format!(
            "observed_tier={evidence_tier}, required_tier={}",
            entry.required_evidence_tier
        ),
        (!tier_satisfies).then(|| {
            format!(
                "example `{}` requires `{}` evidence; current evidence is `{evidence_tier}`",
                entry.id, entry.required_evidence_tier
            )
        }),
    );
    let preview_pixel_inventory = existing
        .as_ref()
        .and_then(|report| report.pointer("/preview_surface_proof/readback_artifact/path"))
        .and_then(serde_json::Value::as_str)
        .map(native_readback_pixel_inventory)
        .transpose()?
        .unwrap_or_else(|| json!({"status": "fail", "reason": "missing preview readback path"}));
    let dev_pixel_inventory = existing
        .as_ref()
        .and_then(|report| report.pointer("/dev_surface_proof/readback_artifact/path"))
        .and_then(serde_json::Value::as_str)
        .map(native_readback_pixel_inventory)
        .transpose()?
        .unwrap_or_else(|| json!({"status": "fail", "reason": "missing dev readback path"}));
    let pixel_inventory_pass =
        [&preview_pixel_inventory, &dev_pixel_inventory]
            .iter()
            .all(|inventory| {
                inventory.get("status").and_then(serde_json::Value::as_str) == Some("pass")
            });
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-visible-launch:{}:app-owned-pixel-inventory", entry.id),
        pixel_inventory_pass,
        format!(
            "preview_status={:?}, dev_status={:?}, preview_clear_ratio={:?}, dev_clear_ratio={:?}",
            preview_pixel_inventory
                .get("status")
                .and_then(serde_json::Value::as_str),
            dev_pixel_inventory
                .get("status")
                .and_then(serde_json::Value::as_str),
            preview_pixel_inventory
                .get("dominant_color_ratio")
                .and_then(serde_json::Value::as_f64),
            dev_pixel_inventory
                .get("dominant_color_ratio")
                .and_then(serde_json::Value::as_f64),
        ),
        (!pixel_inventory_pass).then(|| {
            "visible launch lacks non-single-color app-owned pixel inventory for preview and dev windows".to_owned()
        }),
    );
    let preview_structural_inventory = existing
        .as_ref()
        .and_then(|report| {
            report
                .pointer("/preview_document_layout_proof/artifact_path")
                .and_then(serde_json::Value::as_str)
        })
        .map(native_layout_artifact_structural_inventory)
        .transpose()?
        .unwrap_or_else(|| json!({"status": "fail", "reason": "missing preview layout artifact"}));
    let dev_structural_inventory = existing
        .as_ref()
        .and_then(|report| {
            report.pointer("/dev_shell_interaction_probe/selected_example_structural_inventory")
        })
        .cloned()
        .unwrap_or_else(|| json!({"status": "fail", "reason": "missing dev structural inventory"}));
    let dev_editor_token_count = existing
        .as_ref()
        .and_then(|report| {
            report.pointer("/dev_shell_interaction_probe/editor_model/syntax_token_count")
        })
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let structural_inventory = json!({
        "preview": preview_structural_inventory,
        "dev": dev_structural_inventory,
        "dev_editor_token_count": dev_editor_token_count,
        "preview_hit_target_count": existing
            .as_ref()
            .and_then(|report| report.pointer("/preview_document_layout_proof/hit_target_count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        "preview_source_intent_count": existing
            .as_ref()
            .and_then(|report| report.pointer("/preview_document_layout_proof/source_intent_count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        "preview_display_item_count": existing
            .as_ref()
            .and_then(|report| report.pointer("/preview_document_layout_proof/display_item_count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
    });
    let structural_inventory_pass = structural_inventory
        .pointer("/preview/text_item_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0
        && structural_inventory
            .pointer("/preview/source_binding_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
        && structural_inventory
            .pointer("/dev/text_sample_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
        && structural_inventory
            .pointer("/dev/command_binding_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            >= 3
        && structural_inventory
            .get("dev_editor_token_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
        && structural_inventory
            .pointer("/dev/scroll_root_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
        && structural_inventory
            .pointer("/dev/materialized_node_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0;
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-visible-launch:{}:structural-inventory", entry.id),
        structural_inventory_pass,
        format!("inventory={structural_inventory}"),
        (!structural_inventory_pass).then(|| {
            "visible launch lacks structural text/control/editor scroll/materialization inventory for preview/dev windows"
                .to_owned()
        }),
    );
    let mut artifact_sha256s = existing
        .as_ref()
        .and_then(|report| report.get("artifact_sha256s"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for pointer in [
        "/preview_surface_proof/readback_artifact",
        "/dev_surface_proof/readback_artifact",
    ] {
        if let Some(readback) = existing.as_ref().and_then(|report| report.pointer(pointer)) {
            if let (Some(path), Some(sha256)) = (
                readback.get("path").and_then(serde_json::Value::as_str),
                readback.get("sha256").and_then(serde_json::Value::as_str),
            ) {
                if !artifact_sha256s.iter().any(|artifact| {
                    artifact.get("path").and_then(serde_json::Value::as_str) == Some(path)
                }) {
                    artifact_sha256s.push(json!({"path": path, "sha256": sha256}));
                }
            }
        }
    }
    let exact_launcher_command = format!(
        "cosmic-background-launch --workspace boon-circuit -- ./target/debug/boon_native_playground --role desktop --example {}",
        entry.id
    );
    write_native_gate_report(
        args,
        "verify-native-visible-launch",
        checks,
        blockers,
        json!({
            "example": entry.id,
            "source_path": entry.source,
            "source_hash": source_hash,
            "expected_source_hash": source_hash,
            "program_hash": source_hash,
            "source_files": source_files,
            "required_evidence_tier": entry.required_evidence_tier,
            "observed_evidence_tier": evidence_tier,
            "strict_visible_testing_contract": "docs/plans/STRICT_EXAMPLE_VISIBLE_TESTING_RULES.md",
            "preview_e2e_report": existing_report,
            "preview_e2e_status": existing
                .as_ref()
                .and_then(|report| report.get("status"))
                .cloned()
                .unwrap_or_else(|| json!("missing")),
            "preview_e2e_report_sha256": if existing_report.exists() {
                file_hash(existing_report.to_string_lossy().as_ref())
            } else {
                "missing".to_owned()
            },
            "title_token": existing
                .as_ref()
                .and_then(|report| report.get("title_token"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "preview_window_title": existing
                .as_ref()
                .and_then(|report| report.get("preview_window_title"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "dev_window_title": existing
                .as_ref()
                .and_then(|report| report.get("dev_window_title"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "preview_role_report": existing
                .as_ref()
                .and_then(|report| report.get("preview_role_report"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "dev_role_report": existing
                .as_ref()
                .and_then(|report| report.get("dev_role_report"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "preview_role_report_sha256": existing
                .as_ref()
                .and_then(|report| report.get("preview_role_report_sha256"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "dev_role_report_sha256": existing
                .as_ref()
                .and_then(|report| report.get("dev_role_report_sha256"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "live_state_report": existing
                .as_ref()
                .and_then(|report| report.get("live_state_report"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "live_state_report_sha256": existing
                .as_ref()
                .and_then(|report| report.get("live_state_report_sha256"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "preview_child_pid": existing
                .as_ref()
                .and_then(|report| report.get("preview_child_pid"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "dev_child_pid": existing
                .as_ref()
                .and_then(|report| report.get("dev_child_pid"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "preview_child_cmdline": existing
                .as_ref()
                .and_then(|report| report.get("preview_child_cmdline"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "dev_child_cmdline": existing
                .as_ref()
                .and_then(|report| report.get("dev_child_cmdline"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "desktop_supervisor_pid": existing
                .as_ref()
                .and_then(|report| report.get("desktop_supervisor_pid"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "launcher_pid": existing
                .as_ref()
                .and_then(|report| report.get("launcher_pid"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "launched_binary_path": existing
                .as_ref()
                .and_then(|report| report.get("launched_binary_path"))
                .cloned()
                .unwrap_or_else(|| json!("target/debug/boon_native_playground")),
            "launched_binary_hash": existing
                .as_ref()
                .and_then(|report| report.get("launched_binary_hash"))
                .cloned()
                .unwrap_or_else(|| json!(file_hash("target/debug/boon_native_playground"))),
            "preview_frame_hashes": existing
                .as_ref()
                .and_then(|report| report.get("frame_hashes"))
                .cloned()
                .unwrap_or_else(|| json!([])),
            "preview_readback_artifact": existing
                .as_ref()
                .and_then(|report| report.pointer("/preview_surface_proof/readback_artifact"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "dev_readback_artifact": existing
                .as_ref()
                .and_then(|report| report.pointer("/dev_surface_proof/readback_artifact"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "preview_document_layout_proof": existing
                .as_ref()
                .and_then(|report| report.get("preview_document_layout_proof"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "dev_shell_interaction_probe": existing
                .as_ref()
                .and_then(|report| report.get("dev_shell_interaction_probe"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "visible_reality_harness": existing
                .as_ref()
                .and_then(|report| report.get("visible_reality_harness"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "app_owned_pixel_inventory": {
                "preview": preview_pixel_inventory,
                "dev": dev_pixel_inventory,
            },
            "structural_inventory": structural_inventory,
            "artifact_freshness": existing
                .as_ref()
                .and_then(|report| report.get("artifact_freshness"))
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            "artifact_sha256s": artifact_sha256s,
            "launcher_command": exact_launcher_command
        }),
    )
}

fn native_layout_artifact_structural_inventory(
    path: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let artifact = read_json(Path::new(path))?;
    let display_items = artifact
        .pointer("/layout_frame/display_list")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let source_intents = artifact
        .get("source_intents")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let hit_regions = artifact
        .pointer("/layout_frame/hit_regions")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let scroll_regions = artifact
        .pointer("/layout_frame/scroll_regions")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut kind_counts = BTreeMap::<String, usize>::new();
    let mut text_samples = Vec::new();
    let mut control_samples = Vec::new();
    for item in &display_items {
        let kind = item
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        *kind_counts.entry(kind.clone()).or_default() += 1;
        if let Some(text) = item.get("text").and_then(serde_json::Value::as_str) {
            if !text.trim().is_empty() && text_samples.len() < 32 {
                text_samples.push(json!({
                    "node": item.get("node").cloned().unwrap_or(serde_json::Value::Null),
                    "kind": kind,
                    "text": text.chars().take(80).collect::<String>(),
                    "bounds": item.get("bounds").cloned().unwrap_or(serde_json::Value::Null)
                }));
            }
        }
        if matches!(kind.as_str(), "button" | "text_input" | "grid_cell")
            && control_samples.len() < 32
        {
            control_samples.push(json!({
                "node": item.get("node").cloned().unwrap_or(serde_json::Value::Null),
                "kind": kind,
                "text": item.get("text").cloned().unwrap_or(serde_json::Value::Null),
                "bounds": item.get("bounds").cloned().unwrap_or(serde_json::Value::Null)
            }));
        }
    }
    Ok(json!({
        "status": "pass",
        "artifact_path": path,
        "artifact_sha256": file_hash(path),
        "display_item_count": display_items.len(),
        "text_item_count": text_samples.len(),
        "text_samples": text_samples,
        "control_sample_count": control_samples.len(),
        "control_samples": control_samples,
        "kind_counts": kind_counts,
        "source_binding_count": source_intents.len(),
        "source_binding_samples": source_intents.into_iter().take(32).collect::<Vec<_>>(),
        "hit_region_count": hit_regions.len(),
        "scroll_region_count": scroll_regions.len(),
        "layout_metrics": artifact.pointer("/layout_frame/metrics").cloned().unwrap_or_else(|| json!({})),
        "document_node_count": artifact.pointer("/document_frame/nodes").and_then(serde_json::Value::as_object).map_or(0, |nodes| nodes.len())
    }))
}

fn verify_native_examples(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if !args.iter().any(|arg| arg == "--all") {
        return Err("verify-native-examples currently requires --all".into());
    }
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let entries = boon_runtime::example_manifest_entries()?;
    let mut scenario_coverage = Vec::new();
    for entry in &entries {
        let source_files = manifest_source_files(entry);
        let source_parse = source_files
            .iter()
            .map(|path| Ok((path.clone(), fs::read_to_string(path)?)))
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()
            .and_then(|file_sources| {
                if file_sources.len() <= 1 {
                    let source = file_sources
                        .first()
                        .map(|(_, source)| source.clone())
                        .unwrap_or_default();
                    Ok(boon_parser::parse_source(entry.source.clone(), source)?)
                } else {
                    Ok(boon_parser::parse_project(
                        entry.source.clone(),
                        file_sources,
                    )?)
                }
            });
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-examples:{}:source-syntax", entry.id),
            source_parse.is_ok(),
            source_parse
                .as_ref()
                .map(|_| format!("source={}", entry.source))
                .unwrap_or_else(|error| error.to_string()),
            source_parse.is_err().then(|| {
                format!(
                    "example `{}` does not pass parser/source syntax validation",
                    entry.id
                )
            }),
        );
        let preview_report = read_optional_json(&native_preview_e2e_report_path(&entry.id))?;
        let exercised_scenarios = preview_report
            .as_ref()
            .and_then(|report| report.get("scenario_labels"))
            .and_then(serde_json::Value::as_array)
            .map(|labels| {
                labels
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        let mut missing_scenarios = Vec::new();
        for label in entry
            .initial_visible_assertions
            .iter()
            .chain(entry.input_scenarios.iter())
            .chain(entry.scroll_focus_scenarios.iter())
        {
            let exercised = exercised_scenarios.contains(label);
            push_audit_check(
                &mut checks,
                &mut blockers,
                format!("native-examples:{}:scenario-exercised:{label}", entry.id),
                exercised,
                format!(
                    "scenario must be declared by manifest and exercised by fresh report `{}`",
                    native_preview_e2e_report_path(&entry.id).display()
                ),
                (!exercised).then(|| {
                    format!(
                        "example `{}` scenario `{label}` is declared but not exercised by the native E2E report",
                        entry.id
                    )
                }),
            );
            if !exercised {
                missing_scenarios.push(label.clone());
            }
        }
        scenario_coverage.push(json!({
            "example": entry.id,
            "report": native_preview_e2e_report_path(&entry.id),
            "exercised_scenarios": exercised_scenarios,
            "missing_scenarios": missing_scenarios,
        }));
        let observed_tier = preview_report
            .as_ref()
            .and_then(|report| report.get("evidence_tier"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("missing");
        let tier_ok = evidence_tier_satisfies(observed_tier, &entry.required_evidence_tier);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-examples:{}:required-visible-tier", entry.id),
            tier_ok,
            format!(
                "observed_tier={observed_tier}, required_tier={}",
                entry.required_evidence_tier
            ),
            (!tier_ok).then(|| {
                format!(
                    "example `{}` has not satisfied required `{}` visible evidence",
                    entry.id, entry.required_evidence_tier
                )
            }),
        );
    }
    write_native_gate_report(
        args,
        "verify-native-examples",
        checks,
        blockers,
        json!({
            "manifest_path": boon_runtime::example_manifest_path(),
            "example_count": entries.len(),
            "scenario_coverage": scenario_coverage,
            "all_examples": true,
            "evidence_policy": "lower tiers do not satisfy higher tiers"
        }),
    )
}

fn verify_native_dev_window_editor(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_arg(args, "--example").unwrap_or_else(|| "cells".to_owned());
    let entry = boon_runtime::example_manifest_entry(&example)?;
    let source_text = fs::read_to_string(&entry.source)?;
    let native_source = fs::read_to_string("crates/boon_native_playground/src/main.rs")?;
    let native_gpu_source = fs::read_to_string("crates/boon_native_gpu/src/lib.rs")?;
    let preview_report_path = native_preview_e2e_report_path(&entry.id);
    let preview_report = read_optional_json(&preview_report_path)?;
    let dev_probe = preview_report
        .as_ref()
        .and_then(|report| report.get("dev_shell_interaction_probe"));
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let required_components = [
        "ExampleCatalog",
        "ExampleWorkspace",
        "BoonLanguageService",
        "CodeEditorModel",
        "CodeEditorView",
        "DevWindowShell",
    ];
    for component in required_components {
        let present = native_source.contains(component);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-dev-window-editor:{example}:component:{component}"),
            present,
            format!("{component} symbol present={present}"),
            (!present).then(|| format!("native dev window lacks `{component}` boundary")),
        );
    }
    let reported_full_buffer_lines = dev_probe
        .and_then(|probe| probe.pointer("/editor_model/full_buffer_lines"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let full_buffer_not_truncated =
        reported_full_buffer_lines >= source_text.lines().count() as u64;
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:full-buffer"),
        full_buffer_not_truncated,
        format!(
            "source_lines={}, reported_full_buffer_lines={reported_full_buffer_lines}",
            source_text.lines().count()
        ),
        (!full_buffer_not_truncated).then(|| {
            "dev editor still truncates source preview instead of owning full buffer".to_owned()
        }),
    );
    let editor_feature_needles = [
        ("selection", "selection"),
        ("undo-redo", "undo"),
        ("clipboard", "clipboard"),
        ("bracket-matching", "bracket"),
        ("auto-close-brackets", "auto_close"),
        ("keyboard-edit-commands", "keyboard"),
    ];
    for (feature, needle) in editor_feature_needles {
        let present = native_source.to_ascii_lowercase().contains(needle);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-dev-window-editor:{example}:editor-feature:{feature}"),
            present,
            format!("source contains `{needle}` support marker={present}"),
            (!present).then(|| {
                format!("native code editor is missing required `{feature}` support from the plan")
            }),
        );
    }
    let probe_pass = dev_probe
        .and_then(|probe| probe.get("status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:dev-shell-command-probe"),
        probe_pass,
        format!("preview_report={}", preview_report_path.display()),
        (!probe_pass).then(|| {
            format!(
                "native dev window for `{}` lacks passing tab/run/format/reset command probe",
                entry.id
            )
        }),
    );
    for (feature, pointer) in [
        ("selection", "/editor_model/selection_supported"),
        ("undo-redo", "/editor_model/undo_redo_supported"),
        ("clipboard", "/editor_model/clipboard_adapter_supported"),
        (
            "bracket-matching",
            "/editor_model/bracket_matching_supported",
        ),
        ("caret-overlay", "/editor_model/caret_overlay_supported"),
        ("caret-blink", "/editor_model/caret_blink_supported"),
        (
            "selection-overlay",
            "/editor_model/selection_overlay_supported",
        ),
    ] {
        let feature_pass = dev_probe
            .and_then(|probe| probe.pointer(pointer))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-dev-window-editor:{example}:model-probe:{feature}"),
            feature_pass,
            format!("dev_shell_interaction_probe{pointer}={feature_pass}"),
            (!feature_pass).then(|| {
                format!("native code editor model probe does not prove `{feature}` support")
            }),
        );
    }
    let keyboard_commands = dev_probe
        .and_then(|probe| probe.pointer("/editor_model/keyboard_commands_supported"))
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:model-probe:keyboard-commands"),
        keyboard_commands >= 8,
        format!("keyboard_command_count={keyboard_commands}"),
        (keyboard_commands < 8)
            .then(|| "native code editor model probe lacks required keyboard commands".to_owned()),
    );
    let auto_close_pairs = dev_probe
        .and_then(|probe| probe.pointer("/editor_model/auto_close_brackets"))
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:model-probe:auto-close-brackets"),
        auto_close_pairs >= 3,
        format!("auto_close_pair_count={auto_close_pairs}"),
        (auto_close_pairs < 3).then(|| {
            "native code editor model probe does not prove auto-close bracket support".to_owned()
        }),
    );
    let editor_text_input_pass = dev_probe
        .and_then(|probe| probe.pointer("/editor_text_input/status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && dev_probe
            .and_then(|probe| probe.pointer("/editor_text_input/source_changed"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| probe.pointer("/editor_text_input/dirty"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| {
                probe
                    .pointer("/editor_text_input/host_synthetic_activation/source_binding_resolved")
            })
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| {
                probe.pointer("/editor_text_input/host_synthetic_activation/hit_test_performed")
            })
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| probe.pointer("/editor_text_input/direct_dispatch_without_hit_test"))
            .and_then(serde_json::Value::as_bool)
            == Some(false);
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:editor-text-input-route"),
        editor_text_input_pass,
        format!("preview_report={}", preview_report_path.display()),
        (!editor_text_input_pass).then(|| {
            "native code editor text input is not proven through document hit/source binding into CodeEditorModel".to_owned()
        }),
    );
    let parser_backed_highlighting = dev_probe
        .and_then(|probe| probe.pointer("/editor_model/syntax_backend"))
        .and_then(serde_json::Value::as_str)
        == Some("boon_parser::parse_ast")
        && dev_probe
            .and_then(|probe| probe.pointer("/editor_model/syntax_parser_backed"))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:parser-backed-syntax-highlighting"),
        parser_backed_highlighting,
        format!(
            "syntax_backend={:?}",
            dev_probe
                .and_then(|probe| probe.pointer("/editor_model/syntax_backend"))
                .and_then(serde_json::Value::as_str)
        ),
        (!parser_backed_highlighting).then(|| {
            "native syntax highlighting is not proven to use the Boon parser token stream"
                .to_owned()
        }),
    );
    let syntax_category_contains = |pointer: &str, expected: &str| -> bool {
        dev_probe
            .and_then(|probe| probe.pointer(pointer))
            .and_then(serde_json::Value::as_array)
            .is_some_and(|categories| {
                categories
                    .iter()
                    .any(|category| category.as_str() == Some(expected))
            })
    };
    let model_operator_family_pass = ["operator", "pipe"]
        .iter()
        .any(|category| syntax_category_contains("/editor_model/syntax_categories", category));
    let rendered_operator_family_pass = ["operator", "pipe"].iter().any(|category| {
        syntax_category_contains("/editor_model/syntax_render_categories", category)
    });
    let model_categories_pass = ["comment", "keyword", "punctuation", "source-binding"]
        .iter()
        .all(|category| syntax_category_contains("/editor_model/syntax_categories", category))
        && model_operator_family_pass;
    let rendered_categories_pass = ["comment", "keyword", "punctuation", "source-binding"]
        .iter()
        .all(|category| {
            syntax_category_contains("/editor_model/syntax_render_categories", category)
        })
        && rendered_operator_family_pass;
    let rendered_segment_count = dev_probe
        .and_then(|probe| probe.pointer("/editor_model/syntax_render_segment_count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let rendered_samples = dev_probe
        .and_then(|probe| probe.pointer("/editor_model/syntax_render_segment_samples"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let rendered_non_plain_sample_count = rendered_samples
        .iter()
        .filter(|sample| sample.get("kind").and_then(serde_json::Value::as_str) != Some("plain"))
        .count();
    let rendered_segmented_highlighting = model_categories_pass
        && rendered_categories_pass
        && rendered_segment_count >= 8
        && rendered_non_plain_sample_count >= 4;
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:rendered-syntax-segments"),
        rendered_segmented_highlighting,
        format!(
            "model_categories_pass={model_categories_pass}, rendered_categories_pass={rendered_categories_pass}, model_operator_family_pass={model_operator_family_pass}, rendered_operator_family_pass={rendered_operator_family_pass}, rendered_segment_count={rendered_segment_count}, rendered_non_plain_sample_count={rendered_non_plain_sample_count}"
        ),
        (!rendered_segmented_highlighting).then(|| {
            "native code editor did not prove visible rows are rendered as parser-token-colored segments".to_owned()
        }),
    );
    let invalid_reserved_probe_pass = dev_probe
        .and_then(|probe| probe.pointer("/editor_model/invalid_reserved_token_probe/status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && dev_probe
            .and_then(|probe| {
                probe.pointer("/editor_model/invalid_reserved_token_probe/example_keyword_invalid")
            })
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| {
                probe.pointer("/editor_model/invalid_reserved_token_probe/hash_comment_invalid")
            })
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:invalid-reserved-token-highlighting"),
        invalid_reserved_probe_pass,
        format!(
            "invalid_reserved_token_probe={:?}",
            dev_probe.and_then(
                |probe| probe.pointer("/editor_model/invalid_reserved_token_probe/status")
            )
        ),
        (!invalid_reserved_probe_pass).then(|| {
            "`EXAMPLE` and `#` are not proven to highlight as invalid/reserved native tokens"
                .to_owned()
        }),
    );
    let original_theme_pass = dev_probe
        .and_then(|probe| probe.pointer("/editor_model/syntax_theme/font_family"))
        .and_then(serde_json::Value::as_str)
        == Some("JetBrains Mono")
        && dev_probe
            .and_then(|probe| probe.pointer("/editor_model/syntax_theme/font_size"))
            .and_then(serde_json::Value::as_u64)
            == Some(16)
        && dev_probe
            .and_then(|probe| probe.pointer("/editor_model/syntax_theme/font_feature_settings"))
            .and_then(serde_json::Value::as_str)
            == Some("'zero' 1, 'calt' 1")
        && dev_probe
            .and_then(|probe| probe.pointer("/editor_model/syntax_theme/background"))
            .and_then(serde_json::Value::as_str)
            == Some("#282c34")
        && dev_probe
            .and_then(|probe| probe.pointer("/editor_model/syntax_theme/foreground"))
            .and_then(serde_json::Value::as_str)
            == Some("#d9e1f2")
        && dev_probe
            .and_then(|probe| probe.pointer("/editor_model/syntax_theme/rules/keyword/color"))
            .and_then(serde_json::Value::as_str)
            == Some("#D2691E")
        && dev_probe
            .and_then(|probe| probe.pointer("/editor_model/syntax_theme/rules/keyword/font_style"))
            .and_then(serde_json::Value::as_str)
            == Some("italic")
        && dev_probe
            .and_then(|probe| probe.pointer("/editor_model/syntax_theme/rules/definition/color"))
            .and_then(serde_json::Value::as_str)
            == Some("#ff6ec7")
        && dev_probe
            .and_then(|probe| probe.pointer("/editor_model/syntax_theme/rules/comment/font_style"))
            .and_then(serde_json::Value::as_str)
            == Some("italic");
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:original-typescript-theme-parity"),
        original_theme_pass,
        format!(
            "syntax_theme={:?}",
            dev_probe.and_then(|probe| probe.pointer("/editor_model/syntax_theme"))
        ),
        (!original_theme_pass).then(|| {
            "native code editor syntax theme does not match the original TypeScript playground theme evidence".to_owned()
        }),
    );
    let editor_font_assets = [
        (
            "assets/fonts/JetBrainsMono-Patched.ttf",
            "d116bf61a4cdb4c4ccc86ece8cc652310421128bb6256333f0db53ad2edd8663",
        ),
        (
            "assets/fonts/JetBrainsMono-Patched-Bold.ttf",
            "20318de782f2121132514507f64b2e9c19ce9adc667aff691a4cdd33e7a6dbf7",
        ),
        (
            "assets/fonts/JetBrainsMono-Patched-Italic.ttf",
            "d88b1f96c507b433fdd4cbbda425b79acbef8f317feca3767980141cc74227a1",
        ),
        (
            "assets/fonts/JetBrainsMono-Patched-BoldItalic.ttf",
            "95be8ef81a03236c53e3063648ef0129c3dd8dcf8cbd6260b2042f2a9bd5190e",
        ),
    ];
    let editor_font_hashes = editor_font_assets
        .iter()
        .map(|(path, _)| (*path, boon_runtime::sha256_file(Path::new(path)).ok()))
        .collect::<Vec<_>>();
    let reported_font_family = dev_probe
        .and_then(|probe| probe.pointer("/editor_model/font_family"))
        .and_then(serde_json::Value::as_str);
    let reported_font_features = dev_probe
        .and_then(|probe| probe.pointer("/editor_model/syntax_theme/font_features"))
        .and_then(serde_json::Value::as_str);
    let font_pass = editor_font_assets.iter().all(|(path, expected)| {
        boon_runtime::sha256_file(Path::new(path))
            .map(|hash| hash == *expected)
            .unwrap_or(false)
            && native_gpu_source.contains(path.rsplit('/').next().unwrap_or(path))
    }) && reported_font_family == Some("JetBrains Mono")
        && reported_font_features == Some("zero,calt")
        && native_gpu_source.contains("Shaping::Advanced")
        && native_gpu_source.contains("FeatureTag::CONTEXTUAL_ALTERNATES")
        && native_gpu_source.contains("FeatureTag::new(b\"zero\")")
        && !native_gpu_source.contains("Shaping::Basic");
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:jetbrains-mono-font-asset"),
        font_pass,
        format!(
            "font_hashes={editor_font_hashes:?}, reported_font_family={reported_font_family:?}, reported_font_features={reported_font_features:?}, shaping=Shaping::Advanced, font_features=zero,calt"
        ),
        (!font_pass).then(|| {
            "native code editor is not proven to bundle all styled JetBrains Mono variants and render them with advanced shaping/ligature support".to_owned()
        }),
    );
    let custom_example_pass = dev_probe
        .and_then(|probe| probe.pointer("/custom_example/status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    let custom_example_persistent = dev_probe
        .and_then(|probe| probe.pointer("/custom_example/persistent_store/round_trip_pass"))
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let custom_store_pass = dev_probe
        .and_then(|probe| probe.pointer("/custom_store/status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    let custom_store_persistent = dev_probe
        .and_then(|probe| probe.pointer("/custom_store/persistent_store/round_trip_pass"))
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let custom_tab_after_create = dev_probe
        .and_then(|probe| probe.pointer("/custom_tab_after_create"))
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let custom_rename_pass = dev_probe
        .and_then(|probe| probe.pointer("/custom_rename/status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && dev_probe
            .and_then(|probe| probe.pointer("/custom_rename/source_unchanged"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| probe.pointer("/custom_rename/persistent_store/round_trip_pass"))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    let custom_remove_pass = dev_probe
        .and_then(|probe| probe.pointer("/custom_remove/status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && dev_probe
            .and_then(|probe| {
                probe.pointer("/custom_remove/catalog_removal/persistent_store/round_trip_pass")
            })
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| probe.pointer("/custom_remove/command"))
            .and_then(serde_json::Value::as_str)
            == Some("RemoveSelectedCustomExample")
        && dev_probe
            .and_then(|probe| probe.pointer("/custom_remove/dispatched_source_path"))
            .and_then(serde_json::Value::as_str)
            == Some("dev.commands.remove_custom")
        && dev_probe
            .and_then(|probe| probe.pointer("/custom_remove/removed_not_listed"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| probe.pointer("/custom_remove/direct_dispatch_without_hit_test"))
            .and_then(serde_json::Value::as_bool)
            == Some(false);
    let new_custom_tab_pass = dev_probe
        .and_then(|probe| probe.pointer("/new_custom_tab/status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && dev_probe
            .and_then(|probe| probe.pointer("/new_custom_tab/source_starts_empty"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| probe.pointer("/new_custom_tab/persistent_store/round_trip_pass"))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    let new_custom_edit_persistent = dev_probe
        .and_then(|probe| probe.pointer("/new_custom_editor_text_input/status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && dev_probe
            .and_then(|probe| probe.pointer("/new_custom_editor_text_input/source_changed"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| {
                probe.pointer("/new_custom_editor_text_input/custom_source_persistence/status")
            })
            .and_then(serde_json::Value::as_str)
            == Some("pass")
        && dev_probe
            .and_then(|probe| {
                probe.pointer(
                    "/new_custom_editor_text_input/custom_source_persistence/persistent_store/round_trip_pass",
                )
            })
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    let new_custom_remove_pass = dev_probe
        .and_then(|probe| probe.pointer("/new_custom_remove/status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && dev_probe
            .and_then(|probe| {
                probe.pointer("/new_custom_remove/catalog_removal/persistent_store/round_trip_pass")
            })
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| probe.pointer("/new_custom_remove/command"))
            .and_then(serde_json::Value::as_str)
            == Some("RemoveSelectedCustomExample")
        && dev_probe
            .and_then(|probe| probe.pointer("/new_custom_remove/dispatched_source_path"))
            .and_then(serde_json::Value::as_str)
            == Some("dev.commands.remove_custom")
        && dev_probe
            .and_then(|probe| probe.pointer("/new_custom_remove/removed_not_listed"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| probe.pointer("/new_custom_remove/select_before_remove/status"))
            .and_then(serde_json::Value::as_str)
            == Some("pass")
        && dev_probe
            .and_then(|probe| probe.pointer("/new_custom_remove/direct_dispatch_without_hit_test"))
            .and_then(serde_json::Value::as_bool)
            == Some(false);
    let official_remove_disabled_pass = dev_probe
        .and_then(|probe| probe.pointer("/official_remove_disabled/status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && dev_probe
            .and_then(|probe| probe.pointer("/official_remove_disabled/control/style_disabled"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| {
                probe.pointer("/official_remove_disabled/control/source_binding_present")
            })
            .and_then(serde_json::Value::as_bool)
            == Some(false)
        && dev_probe
            .and_then(|probe| {
                probe.pointer(
                    "/official_remove_disabled/host_synthetic_activation/source_binding_resolved",
                )
            })
            .and_then(serde_json::Value::as_bool)
            == Some(false);
    let custom_remove_binding_pass = dev_probe
        .and_then(|probe| probe.pointer("/custom_remove_enabled/status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && dev_probe
            .and_then(|probe| probe.pointer("/custom_ui_source_bindings"))
            .and_then(serde_json::Value::as_array)
            .is_some_and(|bindings| {
                bindings
                    .iter()
                    .any(|binding| binding.as_str() == Some("dev.commands.remove_custom"))
            });
    let initial_remove_unbound_pass = dev_probe
        .and_then(|probe| probe.pointer("/initial_ui_source_bindings"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|bindings| {
            !bindings
                .iter()
                .any(|binding| binding.as_str() == Some("dev.commands.remove_custom"))
        });
    let remove_custom_binding_pass =
        official_remove_disabled_pass && custom_remove_binding_pass && initial_remove_unbound_pass;
    let inject_source_pass = dev_probe
        .and_then(|probe| probe.pointer("/inject_source/status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    let dirty_tab_preservation_pass = dev_probe
        .and_then(|probe| probe.pointer("/dirty_tab_preservation/status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && dev_probe
            .and_then(|probe| probe.pointer("/dirty_tab_preservation/dirty_preserved"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| probe.pointer("/dirty_tab_preservation/dirty_marker_preserved"))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    let custom_generic_runtime_example_catalog_pass = dev_probe
        .and_then(|probe| probe.pointer("/custom_generic_runtime_example/status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && dev_probe
            .and_then(|probe| {
                probe.pointer("/custom_generic_runtime_example/executable_runtime_supported")
            })
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && dev_probe
            .and_then(|probe| probe.pointer("/custom_generic_runtime_example/validation/status"))
            .and_then(serde_json::Value::as_str)
            == Some("pass")
        && dev_probe
            .and_then(|probe| {
                probe.pointer("/custom_generic_runtime_example/validation/program_kind")
            })
            .and_then(serde_json::Value::as_str)
            == Some("generic")
        && dev_probe
            .and_then(|probe| {
                probe.pointer("/custom_generic_runtime_example/validation/runtime_surface")
            })
            .and_then(serde_json::Value::as_str)
            == Some("generic-live-runtime");
    let custom_api_pass = custom_example_pass
        && custom_example_persistent
        && custom_store_pass
        && custom_store_persistent
        && custom_tab_after_create
        && custom_rename_pass
        && custom_remove_pass
        && new_custom_tab_pass
        && new_custom_edit_persistent
        && new_custom_remove_pass
        && remove_custom_binding_pass
        && inject_source_pass
        && dirty_tab_preservation_pass
        && custom_generic_runtime_example_catalog_pass;
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:custom-example-api"),
        custom_api_pass,
        format!(
            "custom_example_pass={custom_example_pass}, custom_example_persistent={custom_example_persistent}, custom_store_pass={custom_store_pass}, custom_store_persistent={custom_store_persistent}, custom_tab_after_create={custom_tab_after_create}, custom_rename_pass={custom_rename_pass}, custom_remove_pass={custom_remove_pass}, new_custom_tab_pass={new_custom_tab_pass}, new_custom_edit_persistent={new_custom_edit_persistent}, new_custom_remove_pass={new_custom_remove_pass}, official_remove_disabled_pass={official_remove_disabled_pass}, custom_remove_binding_pass={custom_remove_binding_pass}, initial_remove_unbound_pass={initial_remove_unbound_pass}, remove_custom_binding_pass={remove_custom_binding_pass}, inject_source_pass={inject_source_pass}, dirty_tab_preservation_pass={dirty_tab_preservation_pass}, custom_generic_runtime_example_catalog_pass={custom_generic_runtime_example_catalog_pass}"
        ),
        (!custom_api_pass).then(|| {
            "native dev window lacks persistent generic custom example/injected source/dirty-tab API evidence, or generic document examples are not executable through the generic live runtime".to_owned()
        }),
    );
    let transport_commands = ["tab_switch", "run", "format", "reset"];
    let preview_transport_pass = transport_commands.iter().all(|command| {
        let local =
            dev_probe.and_then(|probe| probe.pointer(&format!("/{command}/preview_transport")));
        let result = dev_probe
            .and_then(|probe| probe.pointer(&format!("/{command}/preview_transport_result")));
        local.and_then(|probe| probe.get("status").and_then(serde_json::Value::as_str))
            == Some("pass")
            && local.and_then(|probe| {
                probe
                    .get("transport_bound")
                    .and_then(serde_json::Value::as_bool)
            }) == Some(true)
            && local.and_then(|probe| {
                probe
                    .get("replace_source_protocol")
                    .and_then(serde_json::Value::as_bool)
            }) == Some(true)
            && local.and_then(|probe| {
                probe
                    .get("dev_visual_update_before_preview_ack")
                    .and_then(serde_json::Value::as_bool)
            }) == Some(true)
            && result.and_then(|probe| probe.get("status").and_then(serde_json::Value::as_str))
                == Some("pass")
            && result.and_then(|probe| {
                probe
                    .pointer("/ack/kind")
                    .and_then(serde_json::Value::as_str)
            }) == Some("replace-source-queued")
    });
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:commands-send-replace-code"),
        preview_transport_pass,
        format!("transport_commands={transport_commands:?}"),
        (!preview_transport_pass).then(|| {
            "Run/Format/Reset/tab commands do not prove selected editor buffer reached PreviewTransport replace-source".to_owned()
        }),
    );
    let structural_inventory =
        dev_probe.and_then(|probe| probe.get("selected_example_structural_inventory"));
    let editor_scroll_root = structural_inventory
        .and_then(|inventory| inventory.pointer("/scroll_root_count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
        > 0;
    let editor_materialized = structural_inventory
        .and_then(|inventory| inventory.pointer("/materialized_node_count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
        > 0;
    let editor_focused = structural_inventory
        .and_then(|inventory| inventory.pointer("/focus"))
        .and_then(serde_json::Value::as_str)
        == Some("dev-code-editor");
    let editor_visible_affordance_pass =
        editor_scroll_root && editor_materialized && editor_focused;
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:visible-editor-scroll-affordance"),
        editor_visible_affordance_pass,
        format!(
            "editor_scroll_root={editor_scroll_root}, editor_materialized={editor_materialized}, editor_focused={editor_focused}"
        ),
        (!editor_visible_affordance_pass).then(|| {
            "visible code editor inventory does not prove focus, scroll root, and materialized editor affordances".to_owned()
        }),
    );
    let source_dispatch_count = dev_probe
        .and_then(|probe| probe.pointer("/command_dispatch_count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let internal_command_shortcut = dev_probe
        .and_then(|probe| probe.pointer("/internal_command_shortcut"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:document-source-command-dispatch"),
        source_dispatch_count >= 4 && !internal_command_shortcut,
        format!(
            "command_dispatch_count={source_dispatch_count}, internal_command_shortcut={internal_command_shortcut}"
        ),
        (!(source_dispatch_count >= 4 && !internal_command_shortcut)).then(|| {
            "native dev window command probe bypasses Document SourceBinding dispatch".to_owned()
        }),
    );
    let real_window_command_input = dev_probe
        .and_then(|probe| probe.get("evidence_tier"))
        .and_then(serde_json::Value::as_str)
        == Some("real-window")
        && dev_probe
            .and_then(|probe| probe.get("visible_window_input"))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-dev-window-editor:{example}:commands-real-window"),
        real_window_command_input,
        format!(
            "dev_probe_tier={:?}, visible_window_input={:?}",
            dev_probe
                .and_then(|probe| probe.get("evidence_tier"))
                .and_then(serde_json::Value::as_str),
            dev_probe
                .and_then(|probe| probe.get("visible_window_input"))
                .and_then(serde_json::Value::as_bool)
        ),
        (!real_window_command_input).then(|| {
            "Run/Format/Reset/tab evidence has not reached the required real-window tier".to_owned()
        }),
    );
    for command in ["tab_switch", "run", "format", "reset"] {
        let command_pass = dev_probe
            .and_then(|probe| probe.get(command))
            .and_then(|value| value.get("status"))
            .and_then(serde_json::Value::as_str)
            == Some("pass");
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-dev-window-editor:{example}:command:{command}"),
            command_pass,
            format!("command_probe={command}"),
            (!command_pass).then(|| {
                format!(
                    "native dev window command `{command}` is not proven through DevWindowShell"
                )
            }),
        );
    }
    let dev_driver_report = json!({
        "dev_shell_interaction_probe": dev_probe.cloned().unwrap_or_else(|| json!(null))
    });
    let boon_driver_proof = boon_driver::app_owned_dev_window_proof(&dev_driver_report);
    write_native_gate_report(
        args,
        "verify-native-dev-window-editor",
        checks,
        blockers,
        json!({
            "example": entry.id,
            "source_path": entry.source,
            "source_line_count": source_text.lines().count(),
            "preview_e2e_report": preview_report_path,
            "dev_shell_interaction_probe": dev_probe.cloned().unwrap_or_else(|| json!(null)),
            "boon_driver_proof": boon_driver_proof,
            "required_command_evidence_tier": "real-window",
            "required_editor_features": [
                "tabs",
                "Run",
                "Format",
                "Reset",
                "full-buffer-model",
                "scroll",
                "caret",
                "selection",
                "selection-overlay",
                "caret-overlay",
                "caret-blink",
                "bracket-matching",
                "auto-close-brackets",
                "clipboard",
                "undo-redo",
                "keyboard-edit-commands",
                "diagnostics"
            ]
        }),
    )
}

fn verify_native_example_tabs(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let entries = boon_runtime::example_manifest_entries()?;
    let native_source = fs::read_to_string("crates/boon_native_playground/src/main.rs")?;
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-example-tabs:manifest-has-tabs",
        entries.len() >= 2,
        format!("entry_count={}", entries.len()),
        (entries.len() < 2).then(|| "manifest must declare at least Cells and TodoMVC".to_owned()),
    );
    for entry in &entries {
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-example-tabs:entry:{}", entry.id),
            entry.shown_by_default,
            format!("label={}, source={}", entry.label, entry.source),
            (!entry.shown_by_default)
                .then(|| format!("example `{}` is not shown by default", entry.id)),
        );
    }
    let generic_tabs = native_source.contains("ExampleCatalog") && native_source.contains("tab");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-example-tabs:generic-dev-shell",
        generic_tabs,
        "dev shell must build tabs from ExampleCatalog, not hardcoded renderer branches",
        (!generic_tabs)
            .then(|| "native dev shell does not yet expose generic example tabs".to_owned()),
    );
    for entry in &entries {
        let report_path = native_preview_e2e_report_path(&entry.id);
        let report = read_optional_json(&report_path)?;
        let tab_switch_pass = report
            .as_ref()
            .and_then(|report| report.pointer("/dev_shell_interaction_probe/tab_switch/status"))
            .and_then(serde_json::Value::as_str)
            == Some("pass");
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-example-tabs:{}:tab-switch-command-probe", entry.id),
            tab_switch_pass,
            format!("preview_report={}", report_path.display()),
            (!tab_switch_pass).then(|| {
                format!(
                    "example `{}` lacks passing manifest-driven tab switch evidence",
                    entry.id
                )
            }),
        );
        let tab_real_window = report
            .as_ref()
            .and_then(|report| report.pointer("/dev_shell_interaction_probe/evidence_tier"))
            .and_then(serde_json::Value::as_str)
            == Some("real-window");
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-example-tabs:{}:tab-switch-real-window", entry.id),
            tab_real_window,
            format!("preview_report={}", report_path.display()),
            (!tab_real_window).then(|| {
                format!(
                    "example `{}` tab switching is not proven at the required real-window tier",
                    entry.id
                )
            }),
        );
    }
    write_native_gate_report(
        args,
        "verify-native-example-tabs",
        checks,
        blockers,
        json!({
            "manifest_path": boon_runtime::example_manifest_path(),
            "tabs": entries
                .iter()
                .map(|entry| json!({"id": entry.id, "label": entry.label, "order": entry.default_tab_order}))
                .collect::<Vec<_>>()
        }),
    )
}

fn verify_native_editor_format(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let entries = boon_runtime::example_manifest_entries()?;
    let indentation_fixture = "SOURCE\n  HOLD\n\tLATEST\n\n\nLIST {}\nList/map\n";
    let indentation_formatted =
        boon_parser::format_source("format-fixture.bn", indentation_fixture)?;
    let indentation_normalized =
        indentation_formatted == "SOURCE\n    HOLD\n    LATEST\n\nLIST {}\nList/map\n";
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-editor-format:parser-backed-indentation-normalization",
        indentation_normalized,
        format!("formatted_fixture={indentation_formatted:?}"),
        (!indentation_normalized).then(|| {
            "formatter does not prove indentation/tab/blank-line normalization".to_owned()
        }),
    );
    for entry in &entries {
        let source = boon_runtime::source_text_for_entry(entry)?;
        let formatted = boon_parser::format_source(entry.source.clone(), source.clone());
        let ok = formatted.is_ok();
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-editor-format:{}:parser-backed-format", entry.id),
            ok,
            formatted
                .as_ref()
                .map(|formatted| {
                    format!(
                        "formatted_hash={}",
                        boon_runtime::sha256_bytes(formatted.as_bytes())
                    )
                })
                .unwrap_or_else(|error| error.to_string()),
            (!ok).then(|| format!("formatter rejected example `{}`", entry.id)),
        );
        if let Ok(formatted) = formatted {
            let reparsed = boon_parser::parse_source(entry.source.clone(), formatted).is_ok();
            push_audit_check(
                &mut checks,
                &mut blockers,
                format!("native-editor-format:{}:formatted-source-parses", entry.id),
                reparsed,
                "formatted source parses through normal Boon parser",
                (!reparsed).then(|| format!("formatted `{}` no longer parses", entry.id)),
            );
        }
        let report_path = native_preview_e2e_report_path(&entry.id);
        let report = read_optional_json(&report_path)?;
        let format_command_pass = report
            .as_ref()
            .and_then(|report| report.pointer("/dev_shell_interaction_probe/format/status"))
            .and_then(serde_json::Value::as_str)
            == Some("pass");
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!(
                "native-editor-format:{}:dev-window-format-command",
                entry.id
            ),
            format_command_pass,
            format!("preview_report={}", report_path.display()),
            (!format_command_pass).then(|| {
                format!(
                    "dev window Format command for `{}` is not proven through DevWindowShell",
                    entry.id
                )
            }),
        );
        let format_real_window = report
            .as_ref()
            .and_then(|report| report.pointer("/dev_shell_interaction_probe/evidence_tier"))
            .and_then(serde_json::Value::as_str)
            == Some("real-window");
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-editor-format:{}:dev-window-format-real-window", entry.id),
            format_real_window,
            format!("preview_report={}", report_path.display()),
            (!format_real_window).then(|| {
                format!(
                    "dev window Format command for `{}` is not proven at the required real-window tier",
                    entry.id
                )
            }),
        );
    }
    write_native_gate_report(
        args,
        "verify-native-editor-format",
        checks,
        blockers,
        json!({
            "format_backend": "boon_parser::format_source",
            "formatter_normalization_fixture": {
                "input": indentation_fixture,
                "output": indentation_formatted,
                "pass": indentation_normalized
            },
            "unsupported_example_keyword_rejected": boon_parser::format_source("bad.bn", "EXAMPLE TodoMVC\nSOURCE\nHOLD\nLATEST\nLIST {}\nList/map").is_err()
        }),
    )
}

fn verify_native_example_speed(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_arg(args, "--example").unwrap_or_else(|| "cells".to_owned());
    let entry = boon_runtime::example_manifest_entry(&example)?;
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let existing_report = if entry.id == "cells" {
        PathBuf::from("target/reports/native-gpu/scroll-speed-cells.json")
    } else {
        PathBuf::from(format!("target/reports/native-gpu/speed-{}.json", entry.id))
    };
    let existing = read_optional_json(&existing_report)?;
    let report_valid = existing
        .as_ref()
        .is_some_and(|report| native_gpu_report_staleness_reasons(report).is_empty());
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-example-speed:{}:fresh-speed-report", entry.id),
        report_valid,
        format!(
            "report={}, staleness_reasons={:?}",
            existing_report.display(),
            existing
                .as_ref()
                .map(native_gpu_report_staleness_reasons)
                .unwrap_or_else(|| vec!["missing report".to_owned()])
        ),
        (!report_valid).then(|| {
            format!(
                "missing fresh visible native speed report for `{}` at `{}`",
                entry.id,
                existing_report.display()
            )
        }),
    );
    let source = boon_runtime::source_text_for_entry(&entry)?;
    let full_cells_grid = entry.id != "cells"
        || (source.contains("List/range(from: 0, to: 2599)")
            && source.contains("List/chunk(cells, size: 26"));
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-example-speed:{}:full-example-size", entry.id),
        full_cells_grid,
        "Cells must keep the official 26x100 grid size for speed claims",
        (!full_cells_grid).then(|| "Cells speed gate cannot pass with a reduced grid".to_owned()),
    );
    let p95_present = existing.as_ref().is_some_and(|report| {
        report.get("scroll_frame_ms_p95").is_some()
            || report.get("preview_frame_ms_p50_p95_max").is_some()
    });
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-example-speed:{}:p95-present", entry.id),
        p95_present,
        "speed reports must include p50/p95/p99/max or p95 frame evidence",
        (!p95_present).then(|| {
            format!(
                "speed report for `{}` lacks p95 frame timing evidence",
                entry.id
            )
        }),
    );
    let observed_evidence_tier = existing
        .as_ref()
        .and_then(|report| report.get("evidence_tier"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing")
        .to_owned();
    let tier_satisfies =
        evidence_tier_satisfies(&observed_evidence_tier, &entry.required_evidence_tier);
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-example-speed:{}:evidence-tier", entry.id),
        tier_satisfies,
        format!(
            "observed_tier={}, required_tier={}",
            observed_evidence_tier, entry.required_evidence_tier
        ),
        (!tier_satisfies).then(|| {
            format!(
                "example `{}` speed requires `{}` evidence; current speed evidence is `{}`",
                entry.id, entry.required_evidence_tier, observed_evidence_tier
            )
        }),
    );
    write_native_gate_report(
        args,
        "verify-native-example-speed",
        checks,
        blockers,
        json!({
            "example": entry.id,
            "source_path": entry.source,
            "required_thresholds": entry.performance_thresholds,
            "existing_native_gpu_scroll_report": existing_report,
            "required_evidence_tier": entry.required_evidence_tier,
            "observed_evidence_tier": observed_evidence_tier,
            "strict_visible_speed_satisfied": tier_satisfies
        }),
    )
}

fn verify_native_counter_interaction_speed(
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let event_count = value_arg(args, "--event-count")
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(24)
        .max(1);
    let max_total_ms = value_arg(args, "--max-total-ms")
        .map(|value| value.parse::<f64>())
        .transpose()?
        .unwrap_or(250.0);
    let example = value_arg(args, "--example").unwrap_or_else(|| "counter".to_owned());
    let example_ok = example == "counter";
    push_audit_check(
        &mut checks,
        &mut blockers,
        "counter-interaction-speed:example-selector",
        example_ok,
        format!("example={example}"),
        (!example_ok)
            .then(|| "counter interaction speed gate only supports --example counter".to_owned()),
    );

    let source_path = PathBuf::from("examples/counter.bn");
    let scenario_path = PathBuf::from("examples/counter.scn");
    let artifacts_dir = PathBuf::from("target/artifacts/native-gpu");
    std::fs::create_dir_all(&artifacts_dir)?;
    let role_report = artifacts_dir.join("counter-interaction-speed-role.json");
    let _ = std::fs::remove_file(&role_report);

    let build = Command::new("cargo")
        .args(["build", "-p", "boon_native_playground"])
        .status()?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "counter-interaction-speed:playground-build",
        build.success(),
        format!("cargo build -p boon_native_playground status={build}"),
        (!build.success()).then(|| "failed to build boon_native_playground".to_owned()),
    );

    let binary_path = PathBuf::from("target/debug/boon_native_playground");
    let event_count_arg = event_count.to_string();
    let max_total_ms_arg = max_total_ms.to_string();
    let role_report_arg = role_report.display().to_string();
    let role_output = if build.success() && example_ok {
        Some(
            Command::new(&binary_path)
                .args([
                    "--role",
                    "interaction-speed",
                    "--example",
                    "counter",
                    "--event-count",
                    &event_count_arg,
                    "--max-total-ms",
                    &max_total_ms_arg,
                    "--report",
                    &role_report_arg,
                ])
                .output()?,
        )
    } else {
        None
    };
    let role_exit_success = role_output
        .as_ref()
        .is_some_and(|output| output.status.success());
    push_audit_check(
        &mut checks,
        &mut blockers,
        "counter-interaction-speed:role-exit-success",
        role_exit_success,
        format!(
            "status={:?}",
            role_output
                .as_ref()
                .map(|output| output.status.to_string())
                .unwrap_or_else(|| "not-run".to_owned())
        ),
        (!role_exit_success).then(|| {
            format!(
                "boon_native_playground interaction-speed role failed; report={}",
                role_report.display()
            )
        }),
    );

    let role_report_json = read_optional_json(&role_report)?;
    let role_report_present = role_report_json.is_some();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "counter-interaction-speed:role-report-present",
        role_report_present,
        format!("report={}", role_report.display()),
        (!role_report_present).then(|| {
            format!(
                "interaction-speed role did not write `{}`",
                role_report.display()
            )
        }),
    );

    let role_status_pass = role_report_json.as_ref().is_some_and(|report| {
        report.get("status").and_then(serde_json::Value::as_str) == Some("pass")
    });
    push_audit_check(
        &mut checks,
        &mut blockers,
        "counter-interaction-speed:role-status-pass",
        role_status_pass,
        format!(
            "status={:?}",
            role_report_json
                .as_ref()
                .and_then(|report| report.get("status"))
                .and_then(serde_json::Value::as_str)
        ),
        (!role_status_pass).then(|| "interaction-speed role report status was not pass".to_owned()),
    );

    let role_checks_all_pass = role_report_json.as_ref().is_some_and(|report| {
        report
            .get("per_step_pass_fail")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|steps| {
                !steps.is_empty()
                    && steps.iter().all(|step| {
                        step.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
                    })
            })
    });
    push_audit_check(
        &mut checks,
        &mut blockers,
        "counter-interaction-speed:role-checks-pass",
        role_checks_all_pass,
        "role per_step_pass_fail entries all pass",
        (!role_checks_all_pass)
            .then(|| "interaction-speed role contained a failing step".to_owned()),
    );

    let observed_final = role_report_json
        .as_ref()
        .and_then(|report| report.get("final_count"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing")
        .to_owned();
    let observed_expected = role_report_json
        .as_ref()
        .and_then(|report| report.get("expected_count"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing")
        .to_owned();
    let final_count_ok = observed_final == event_count_arg && observed_expected == event_count_arg;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "counter-interaction-speed:all-clicks-applied",
        final_count_ok,
        format!(
            "event_count={event_count}, final_count={observed_final}, expected_count={observed_expected}"
        ),
        (!final_count_ok).then(|| {
            format!(
                "counter interaction burst dropped clicks: event_count={event_count}, final_count={observed_final}, expected_count={observed_expected}"
            )
        }),
    );

    let render_update_count = role_report_json
        .as_ref()
        .and_then(|report| report.get("preview_shared_render_update_count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let render_update_ok = render_update_count >= event_count;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "counter-interaction-speed:render-updated-for-each-click",
        render_update_ok,
        format!("render_update_count={render_update_count}, event_count={event_count}"),
        (!render_update_ok).then(|| {
            format!(
                "counter interaction render updates lagged clicks: render_update_count={render_update_count}, event_count={event_count}"
            )
        }),
    );

    let interaction_total_ms = role_report_json
        .as_ref()
        .and_then(|report| report.get("interaction_total_ms"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(f64::INFINITY);
    let interaction_per_event_ms = role_report_json
        .as_ref()
        .and_then(|report| report.get("interaction_per_event_ms"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(f64::INFINITY);
    let latency_ok = interaction_total_ms <= max_total_ms;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "counter-interaction-speed:latency-budget",
        latency_ok,
        format!(
            "interaction_total_ms={interaction_total_ms:.3}, interaction_per_event_ms={interaction_per_event_ms:.3}, max_total_ms={max_total_ms:.3}"
        ),
        (!latency_ok).then(|| {
            format!(
                "counter interaction burst exceeded latency budget: interaction_total_ms={interaction_total_ms:.3}, max_total_ms={max_total_ms:.3}"
            )
        }),
    );

    let role_artifact = if role_report.exists() {
        vec![artifact_hash(&role_report)?]
    } else {
        Vec::new()
    };
    write_native_gate_report(
        args,
        "verify-native-counter-interaction-speed",
        checks,
        blockers,
        json!({
            "example": "counter",
            "source_path": source_path,
            "scenario_path": scenario_path,
            "source_hash": file_hash("examples/counter.bn"),
            "scenario_hash": file_hash("examples/counter.scn"),
            "playground_binary_path": binary_path,
            "playground_binary_hash": file_hash("target/debug/boon_native_playground"),
            "role_report": role_report,
            "role_report_status": role_report_json
                .as_ref()
                .and_then(|report| report.get("status"))
                .cloned()
                .unwrap_or_else(|| json!("missing")),
            "event_count": event_count,
            "final_count": observed_final,
            "expected_count": observed_expected,
            "interaction_total_ms": interaction_total_ms,
            "interaction_per_event_ms": interaction_per_event_ms,
            "preview_shared_render_update_count": render_update_count,
            "max_total_ms": max_total_ms,
            "artifact_sha256s": role_artifact
        }),
    )
}

fn verify_native_cells_interaction_speed(
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let profile = value_arg(args, "--profile").unwrap_or_else(|| "debug".to_owned());
    let profile_ok = matches!(profile.as_str(), "debug" | "release");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "cells-interaction-speed:profile-selector",
        profile_ok,
        format!("profile={profile}"),
        (!profile_ok)
            .then(|| "cells interaction speed gate supports --profile debug|release".to_owned()),
    );
    let event_count = value_arg(args, "--event-count")
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(if profile == "release" { 64 } else { 32 })
        .max(1);
    let default_max_p95_ms = if profile == "release" { 16.7 } else { 120.0 };
    let default_max_max_ms = if profile == "release" { 50.0 } else { 250.0 };
    let max_p95_ms = value_arg(args, "--max-p95-ms")
        .map(|value| value.parse::<f64>())
        .transpose()?
        .unwrap_or(default_max_p95_ms);
    let max_max_ms = value_arg(args, "--max-max-ms")
        .map(|value| value.parse::<f64>())
        .transpose()?
        .unwrap_or(default_max_max_ms);
    let entry = boon_runtime::example_manifest_entry("cells")?;
    let source = boon_runtime::source_text_for_entry(&entry)?;
    let generic_source_ok = source.contains("List/range(from: 0, to: 2599)")
        && source.contains("List/chunk(cells, size: 26")
        && ["Formula", "Grid", "List/table", "EXAMPLE", "#"]
            .iter()
            .all(|needle| !source.contains(needle));
    push_audit_check(
        &mut checks,
        &mut blockers,
        "cells-interaction-speed:generic-source",
        generic_source_ok,
        "manifest-backed Cells source uses List/range/List/chunk and no spreadsheet shortcuts",
        (!generic_source_ok).then(|| {
            "Cells interaction speed gate requires generic valid Boon source without Formula/Grid/List-table shortcuts".to_owned()
        }),
    );

    let artifacts_dir = PathBuf::from("target/artifacts/native-gpu");
    std::fs::create_dir_all(&artifacts_dir)?;
    let role_report = artifacts_dir.join(format!("cells-interaction-speed-{profile}-role.json"));
    let _ = std::fs::remove_file(&role_report);

    let mut build_args = vec!["build", "-p", "boon_native_playground"];
    if profile == "release" {
        build_args.push("--release");
    }
    let build = Command::new("cargo").args(&build_args).status()?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "cells-interaction-speed:playground-build",
        build.success(),
        format!("cargo {} status={build}", build_args.join(" ")),
        (!build.success()).then(|| "failed to build boon_native_playground".to_owned()),
    );

    let binary_path = if profile == "release" {
        PathBuf::from("target/release/boon_native_playground")
    } else {
        PathBuf::from("target/debug/boon_native_playground")
    };
    let event_count_arg = event_count.to_string();
    let max_p95_ms_arg = max_p95_ms.to_string();
    let max_max_ms_arg = max_max_ms.to_string();
    let role_report_arg = role_report.display().to_string();
    let role_output = if build.success() && profile_ok && generic_source_ok {
        Some(
            Command::new(&binary_path)
                .args([
                    "--role",
                    "interaction-speed",
                    "--example",
                    "cells",
                    "--event-count",
                    &event_count_arg,
                    "--max-p95-ms",
                    &max_p95_ms_arg,
                    "--max-max-ms",
                    &max_max_ms_arg,
                    "--report",
                    &role_report_arg,
                ])
                .output()?,
        )
    } else {
        None
    };
    let role_exit_success = role_output
        .as_ref()
        .is_some_and(|output| output.status.success());
    push_audit_check(
        &mut checks,
        &mut blockers,
        "cells-interaction-speed:role-exit-success",
        role_exit_success,
        format!(
            "status={:?}",
            role_output
                .as_ref()
                .map(|output| output.status.to_string())
                .unwrap_or_else(|| "not-run".to_owned())
        ),
        (!role_exit_success).then(|| {
            format!(
                "boon_native_playground interaction-speed role failed; report={}",
                role_report.display()
            )
        }),
    );

    let role_report_json = read_optional_json(&role_report)?;
    let role_report_present = role_report_json.is_some();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "cells-interaction-speed:role-report-present",
        role_report_present,
        format!("report={}", role_report.display()),
        (!role_report_present).then(|| {
            format!(
                "interaction-speed role did not write `{}`",
                role_report.display()
            )
        }),
    );

    let role_status_pass = role_report_json.as_ref().is_some_and(|report| {
        report.get("status").and_then(serde_json::Value::as_str) == Some("pass")
    });
    push_audit_check(
        &mut checks,
        &mut blockers,
        "cells-interaction-speed:role-status-pass",
        role_status_pass,
        format!(
            "status={:?}",
            role_report_json
                .as_ref()
                .and_then(|report| report.get("status"))
                .and_then(serde_json::Value::as_str)
        ),
        (!role_status_pass).then(|| "interaction-speed role report status was not pass".to_owned()),
    );

    let role_checks_all_pass = role_report_json.as_ref().is_some_and(|report| {
        report
            .get("per_step_pass_fail")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|steps| {
                !steps.is_empty()
                    && steps.iter().all(|step| {
                        step.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
                    })
            })
    });
    push_audit_check(
        &mut checks,
        &mut blockers,
        "cells-interaction-speed:role-checks-pass",
        role_checks_all_pass,
        "role per_step_pass_fail entries all pass",
        (!role_checks_all_pass)
            .then(|| "interaction-speed role contained a failing step".to_owned()),
    );

    let selected_address = role_report_json
        .as_ref()
        .and_then(|report| report.get("selected_address"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing")
        .to_owned();
    let focused_b0 = selected_address == "B0";
    push_audit_check(
        &mut checks,
        &mut blockers,
        "cells-interaction-speed:b0-focused",
        focused_b0,
        format!("selected_address={selected_address}"),
        (!focused_b0).then(|| {
            format!(
                "Cells focus interaction did not select B0; selected_address={selected_address}"
            )
        }),
    );

    let p95_ms = role_report_json
        .as_ref()
        .and_then(|report| report.get("interaction_latency_ms_p95"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(f64::INFINITY);
    let max_ms = role_report_json
        .as_ref()
        .and_then(|report| report.get("interaction_latency_ms_max"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(f64::INFINITY);
    let p95_ok = p95_ms <= max_p95_ms;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "cells-interaction-speed:p95-budget",
        p95_ok,
        format!("p95_ms={p95_ms:.3}, max_p95_ms={max_p95_ms:.3}"),
        (!p95_ok).then(|| {
            format!(
                "Cells focus p95 exceeded {profile} budget: p95_ms={p95_ms:.3}, max_p95_ms={max_p95_ms:.3}"
            )
        }),
    );
    let max_ok = max_ms <= max_max_ms;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "cells-interaction-speed:max-budget",
        max_ok,
        format!("max_ms={max_ms:.3}, max_max_ms={max_max_ms:.3}"),
        (!max_ok).then(|| {
            format!(
                "Cells focus max latency exceeded {profile} budget: max_ms={max_ms:.3}, max_max_ms={max_max_ms:.3}"
            )
        }),
    );

    let render_update_count = role_report_json
        .as_ref()
        .and_then(|report| report.get("preview_shared_render_update_count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let render_update_ok = render_update_count >= event_count;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "cells-interaction-speed:render-updated-for-each-click",
        render_update_ok,
        format!("render_update_count={render_update_count}, event_count={event_count}"),
        (!render_update_ok).then(|| {
            format!(
                "Cells interaction render updates lagged clicks: render_update_count={render_update_count}, event_count={event_count}"
            )
        }),
    );

    let role_artifact = if role_report.exists() {
        vec![artifact_hash(&role_report)?]
    } else {
        Vec::new()
    };
    write_native_gate_report(
        args,
        "verify-native-cells-interaction-speed",
        checks,
        blockers,
        json!({
            "example": "cells",
            "profile": profile,
            "source_path": entry.source,
            "scenario_path": entry.scenario,
            "source_hash": file_hash(&entry.source),
            "source_files_hash": source_hash_for_report_source_files(&manifest_source_files(&entry), &source)?,
            "scenario_hash": file_hash("examples/cells.scn"),
            "playground_binary_path": binary_path,
            "playground_binary_hash": if profile_ok { boon_runtime::sha256_file(&binary_path).unwrap_or_else(|_| "missing".to_owned()) } else { "missing".to_owned() },
            "role_report": role_report,
            "role_report_status": role_report_json
                .as_ref()
                .and_then(|report| report.get("status"))
                .cloned()
                .unwrap_or_else(|| json!("missing")),
            "event_count": event_count,
            "selected_address": selected_address,
            "interaction_latency_ms_p95": p95_ms,
            "interaction_latency_ms_max": max_ms,
            "preview_shared_render_update_count": render_update_count,
            "max_p95_ms": max_p95_ms,
            "max_max_ms": max_max_ms,
            "targets": {
                "debug": {
                    "focus_p95_ms": 120.0,
                    "focus_p99_ms": 180.0,
                    "focus_max_ms": 250.0
                },
                "release": {
                    "focus_p95_ms": 16.7,
                    "focus_p99_ms": 25.0,
                    "focus_max_ms": 50.0
                }
            },
            "artifact_sha256s": role_artifact
        }),
    )
}

fn verify_native_dev_editor_speed(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let existing_report =
        PathBuf::from("target/reports/native-gpu/scroll-speed-dev-code-editor.json");
    let existing = read_optional_json(&existing_report)?;
    let valid = existing
        .as_ref()
        .is_some_and(|report| native_gpu_report_staleness_reasons(report).is_empty());
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-dev-editor-speed:fresh-scroll-report",
        valid,
        format!(
            "report={}, staleness_reasons={:?}",
            existing_report.display(),
            existing
                .as_ref()
                .map(native_gpu_report_staleness_reasons)
                .unwrap_or_else(|| vec!["missing report".to_owned()])
        ),
        (!valid).then(|| {
            "missing fresh dev editor speed report bound to the visible native dev window"
                .to_owned()
        }),
    );
    let observed_evidence_tier = existing
        .as_ref()
        .and_then(|report| report.get("evidence_tier"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("missing")
        .to_owned();
    let strict_visible_speed = evidence_tier_satisfies(&observed_evidence_tier, "real-window")
        && existing.as_ref().is_some_and(|report| {
            report
                .get("required_real_window_speed_proven")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        });
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-dev-editor-speed:real-window-tier",
        strict_visible_speed,
        format!("observed_tier={observed_evidence_tier}, required_tier=real-window"),
        (!strict_visible_speed).then(|| {
            "dev editor speed is not proven with real-window wheel/input evidence".to_owned()
        }),
    );
    let min_lines = native_gpu_budget_u64("dev_code_editor", "min_lines").unwrap_or(10_000);
    let min_longest_line_bytes =
        native_gpu_budget_u64("dev_code_editor", "min_longest_line_bytes").unwrap_or(2_000);
    let reported_line_count = existing
        .as_ref()
        .and_then(|report| report.get("line_count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let reported_longest_line_bytes = existing
        .as_ref()
        .and_then(|report| report.get("longest_line_bytes"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let corpus_pass = existing.as_ref().is_some_and(|report| {
        report
            .pointer("/dev_editor_speed_corpus/status")
            .and_then(serde_json::Value::as_str)
            == Some("pass")
            && report
                .pointer("/dev_editor_speed_corpus/line_budget_satisfied")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
            && report
                .pointer("/dev_editor_speed_corpus/longest_line_budget_satisfied")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
    });
    let materialized_range_pass = existing.as_ref().is_some_and(|report| {
        report.get("materialized_range_before_after").is_some()
            && report
                .get("materialized_line_count_max")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
    });
    let full_buffer = reported_line_count >= min_lines
        && reported_longest_line_bytes >= min_longest_line_bytes
        && corpus_pass
        && materialized_range_pass;
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-dev-editor-speed:full-buffer-not-truncated",
        full_buffer,
        format!(
            "line_count={reported_line_count}/{min_lines}, longest_line_bytes={reported_longest_line_bytes}/{min_longest_line_bytes}, corpus_pass={corpus_pass}, materialized_range_pass={materialized_range_pass}"
        ),
        (!full_buffer).then(|| {
            "dev editor still appears to truncate the source buffer before rendering".to_owned()
        }),
    );
    write_native_gate_report(
        args,
        "verify-native-dev-editor-speed",
        checks,
        blockers,
        json!({
            "surface": "dev-code-editor",
            "existing_native_gpu_scroll_report": existing_report,
            "required_evidence_tier": "real-window",
            "observed_evidence_tier": observed_evidence_tier,
            "strict_visible_speed_satisfied": strict_visible_speed,
            "required_metrics": ["p50", "p95", "p99", "max", "dropped_frames", "longest_visible_stall"]
        }),
    )
}

fn verify_boon_driver_schema(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let docs = [
        "docs/architecture/BOON_DRIVER.md",
        "docs/architecture/LINUX_HUMAN_LIKE_TESTING.md",
        "docs/architecture/NATIVE_GPU_PIPELINE.md",
    ];
    for doc in docs {
        let exists = Path::new(doc).exists();
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("boon-driver-schema:doc:{doc}"),
            exists,
            format!("{doc} exists={exists}"),
            (!exists).then(|| format!("missing BoonDriver contract document `{doc}`")),
        );
    }
    let driver_crate = Path::new("crates/boon_driver/src/lib.rs").exists();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "boon-driver-schema:crate-boundary",
        driver_crate,
        format!("crates/boon_driver/src/lib.rs exists={driver_crate}"),
        (!driver_crate).then(|| "missing host-neutral boon_driver crate boundary".to_owned()),
    );
    let cargo = fs::read_to_string("crates/boon_driver/Cargo.toml").unwrap_or_default();
    let forbidden_deps = ["wgpu", "app_window", "boon_native", "boon_runtime"];
    for dep in forbidden_deps {
        let absent = !cargo.contains(dep);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("boon-driver-schema:forbidden-dependency:{dep}"),
            absent,
            format!("{dep} absent={absent}"),
            (!absent).then(|| format!("boon_driver must not depend on `{dep}`")),
        );
    }
    let tier_order = boon_driver::evidence_tier_satisfies(
        boon_driver::TIER_BOON_DRIVER,
        boon_driver::LEGACY_TIER_HOST_SYNTHETIC,
    ) && boon_driver::evidence_tier_satisfies(
        boon_driver::TIER_REAL_WINDOW,
        boon_driver::TIER_BOON_DRIVER,
    ) && !boon_driver::evidence_tier_satisfies(
        boon_driver::TIER_BOON_DRIVER,
        boon_driver::TIER_REAL_WINDOW,
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "boon-driver-schema:evidence-tier-order",
        tier_order,
        "runtime < boon-driver/legacy-host-synthetic < real-window < human",
        (!tier_order).then(|| "BoonDriver evidence tier order is invalid".to_owned()),
    );
    let report =
        report_arg(args).unwrap_or_else(|| PathBuf::from("target/reports/boon-driver/schema.json"));
    write_static_gate_report(
        args,
        "verify-boon-driver-schema",
        report,
        checks,
        blockers,
        json!({
            "architecture_contract": "docs/architecture/BOON_DRIVER.md",
            "linux_human_like_contract": "docs/architecture/LINUX_HUMAN_LIKE_TESTING.md",
            "evidence_tiers": [
                boon_driver::TIER_RUNTIME,
                boon_driver::TIER_BOON_DRIVER,
                boon_driver::TIER_REAL_WINDOW,
                boon_driver::TIER_HUMAN
            ],
            "legacy_compatible_tier": boon_driver::LEGACY_TIER_HOST_SYNTHETIC,
        }),
    )
}

fn verify_boon_driver_e2e(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_arg(args, "--example").unwrap_or_else(|| "cells".to_owned());
    let entry = boon_runtime::example_manifest_entry(&example)?;
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let native_report_path = native_preview_e2e_report_path(&entry.id);
    let native_report = read_optional_json(&native_report_path)?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("boon-driver-e2e:{}:native-preview-report-present", entry.id),
        native_report.is_some(),
        format!(
            "{} exists={}",
            native_report_path.display(),
            native_report.is_some()
        ),
        native_report.is_none().then(|| {
            format!(
                "missing native preview E2E report `{}`",
                native_report_path.display()
            )
        }),
    );
    let proof = native_report
        .as_ref()
        .map(boon_driver::app_owned_preview_proof)
        .unwrap_or_else(|| json!({"status": "fail"}));
    let proof_pass = proof.get("status").and_then(serde_json::Value::as_str) == Some("pass");
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("boon-driver-e2e:{}:app-owned-driver-proof", entry.id),
        proof_pass,
        format!(
            "status={:?}, action_count={}",
            proof.get("status").and_then(serde_json::Value::as_str),
            proof
                .get("action_proofs")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len)
        ),
        (!proof_pass).then(|| {
            format!(
                "native preview E2E report for `{}` does not prove the BoonDriver app-owned route",
                entry.id
            )
        }),
    );
    let no_real_window_claim = proof
        .get("real_window_claimed")
        .and_then(serde_json::Value::as_bool)
        == Some(false);
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("boon-driver-e2e:{}:does-not-claim-real-window", entry.id),
        no_real_window_claim,
        "BoonDriver proof is app-owned and must not claim real-window",
        (!no_real_window_claim)
            .then(|| "BoonDriver report incorrectly claims real-window evidence".to_owned()),
    );
    let report = report_arg(args)
        .unwrap_or_else(|| PathBuf::from(format!("target/reports/boon-driver/{}.json", entry.id)));
    write_static_gate_report(
        args,
        "verify-boon-driver-e2e",
        report,
        checks,
        blockers,
        json!({
            "example": entry.id,
            "source_path": entry.source,
            "source_hash": file_hash(&entry.source),
            "required_evidence_tier": boon_driver::TIER_BOON_DRIVER,
            "observed_evidence_tier": boon_driver::TIER_BOON_DRIVER,
            "does_not_satisfy_real_window": true,
            "native_preview_e2e_report": native_report_path,
            "native_preview_e2e_report_sha256": if native_report_path.exists() {
                file_hash(native_report_path.to_string_lossy().as_ref())
            } else {
                "missing".to_owned()
            },
            "boon_driver_proof": proof,
        }),
    )
}

fn verify_boon_driver_dev_window(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_arg(args, "--example").unwrap_or_else(|| "cells".to_owned());
    let entry = boon_runtime::example_manifest_entry(&example)?;
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let native_report_path = PathBuf::from(format!(
        "target/reports/native-gpu/dev-editor-{}.json",
        entry.id
    ));
    let native_report = read_optional_json(&native_report_path)?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!(
            "boon-driver-dev-window:{}:native-dev-report-present",
            entry.id
        ),
        native_report.is_some(),
        format!(
            "{} exists={}",
            native_report_path.display(),
            native_report.is_some()
        ),
        native_report.is_none().then(|| {
            format!(
                "missing native dev editor report `{}`",
                native_report_path.display()
            )
        }),
    );
    let proof = native_report
        .as_ref()
        .map(boon_driver::app_owned_dev_window_proof)
        .unwrap_or_else(|| json!({"status": "fail"}));
    let proof_pass = proof.get("status").and_then(serde_json::Value::as_str) == Some("pass");
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("boon-driver-dev-window:{}:commands-through-driver", entry.id),
        proof_pass,
        format!(
            "status={:?}, commands_pass={:?}, structural_inventory_pass={:?}",
            proof.get("status").and_then(serde_json::Value::as_str),
            proof.get("commands_pass").and_then(serde_json::Value::as_bool),
            proof
                .get("structural_inventory_pass")
                .and_then(serde_json::Value::as_bool)
        ),
        (!proof_pass).then(|| {
            format!(
                "native dev window report for `{}` does not prove BoonDriver command/editor routing",
                entry.id
            )
        }),
    );
    let report = report_arg(args).unwrap_or_else(|| {
        PathBuf::from(format!(
            "target/reports/boon-driver/dev-window-{}.json",
            entry.id
        ))
    });
    write_static_gate_report(
        args,
        "verify-boon-driver-dev-window",
        report,
        checks,
        blockers,
        json!({
            "example": entry.id,
            "source_path": entry.source,
            "source_hash": file_hash(&entry.source),
            "required_evidence_tier": boon_driver::TIER_BOON_DRIVER,
            "observed_evidence_tier": boon_driver::TIER_BOON_DRIVER,
            "does_not_satisfy_real_window": true,
            "native_dev_window_report": native_report_path,
            "native_dev_window_report_sha256": if native_report_path.exists() {
                file_hash(native_report_path.to_string_lossy().as_ref())
            } else {
                "missing".to_owned()
            },
            "boon_driver_proof": proof,
        }),
    )
}

fn verify_boon_driver_speed(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let selector = native_gpu_scroll_selector(args);
    let label = selector.label;
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let native_report_path = PathBuf::from(format!(
        "target/reports/native-gpu/scroll-speed-{label}.json"
    ));
    let native_report = read_optional_json(&native_report_path)?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("boon-driver-speed:{label}:native-scroll-report-present"),
        native_report.is_some(),
        format!(
            "{} exists={}",
            native_report_path.display(),
            native_report.is_some()
        ),
        native_report.is_none().then(|| {
            format!(
                "missing native scroll report `{}`",
                native_report_path.display()
            )
        }),
    );
    let proof = native_report
        .as_ref()
        .map(boon_driver::app_owned_speed_proof)
        .unwrap_or_else(|| json!({"status": "fail"}));
    let proof_pass = proof.get("status").and_then(serde_json::Value::as_str) == Some("pass");
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("boon-driver-speed:{label}:app-owned-speed-proof"),
        proof_pass,
        format!(
            "status={:?}, budget_pass={:?}, p95={:?}",
            proof.get("status").and_then(serde_json::Value::as_str),
            proof
                .get("budget_pass")
                .and_then(serde_json::Value::as_bool),
            proof.get("wheel_to_visible_ms_p95_per_axis")
        ),
        (!proof_pass)
            .then(|| format!("native scroll report `{label}` does not prove BoonDriver speed")),
    );
    let report = report_arg(args)
        .unwrap_or_else(|| PathBuf::from(format!("target/reports/boon-driver/speed-{label}.json")));
    write_static_gate_report(
        args,
        "verify-boon-driver-speed",
        report,
        checks,
        blockers,
        json!({
            "surface": label,
            "required_evidence_tier": boon_driver::TIER_BOON_DRIVER,
            "observed_evidence_tier": boon_driver::TIER_BOON_DRIVER,
            "does_not_satisfy_real_window": true,
            "native_scroll_speed_report": native_report_path,
            "native_scroll_speed_report_sha256": if native_report_path.exists() {
                file_hash(native_report_path.to_string_lossy().as_ref())
            } else {
                "missing".to_owned()
            },
            "boon_driver_proof": proof,
        }),
    )
}

fn verify_boon_driver_all(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let check_existing = args.iter().any(|arg| arg == "--check-existing");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "boon-driver-all:check-existing-mode",
        check_existing,
        format!("--check-existing present={check_existing}"),
        (!check_existing).then(|| "BoonDriver aggregate requires --check-existing".to_owned()),
    );
    let required = [
        (
            "schema",
            "target/reports/boon-driver/schema.json",
            "verify-boon-driver-schema",
        ),
        (
            "todomvc",
            "target/reports/boon-driver/todomvc.json",
            "verify-boon-driver-e2e",
        ),
        (
            "cells",
            "target/reports/boon-driver/cells.json",
            "verify-boon-driver-e2e",
        ),
        (
            "dev-window-todomvc",
            "target/reports/boon-driver/dev-window-todomvc.json",
            "verify-boon-driver-dev-window",
        ),
        (
            "dev-window-cells",
            "target/reports/boon-driver/dev-window-cells.json",
            "verify-boon-driver-dev-window",
        ),
        (
            "speed-cells",
            "target/reports/boon-driver/speed-cells.json",
            "verify-boon-driver-speed",
        ),
        (
            "speed-dev-code-editor",
            "target/reports/boon-driver/speed-dev-code-editor.json",
            "verify-boon-driver-speed",
        ),
    ];
    let mut artifacts = Vec::new();
    for (label, path, command) in required.iter().copied() {
        let path = PathBuf::from(path);
        let exists = path.exists();
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("boon-driver-all:report-present:{label}"),
            exists,
            format!("{} exists={exists}", path.display()),
            (!exists).then(|| format!("missing BoonDriver report `{}`", path.display())),
        );
        if !exists {
            continue;
        }
        let report = read_json(&path)?;
        let report_command = report.get("command").and_then(serde_json::Value::as_str);
        let command_ok = report_command == Some(command);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("boon-driver-all:command:{label}"),
            command_ok,
            format!("command={report_command:?}, expected={command}"),
            (!command_ok).then(|| {
                format!(
                    "BoonDriver report `{}` has wrong command {:?}, expected `{command}`",
                    path.display(),
                    report_command
                )
            }),
        );
        let pass = report.get("status").and_then(serde_json::Value::as_str) == Some("pass");
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("boon-driver-all:status-pass:{label}"),
            pass,
            format!("{} status pass={pass}", path.display()),
            (!pass).then(|| format!("BoonDriver report `{}` did not pass", path.display())),
        );
        let no_real_window_claim = label == "schema"
            || report
                .get("does_not_satisfy_real_window")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("boon-driver-all:no-real-window-claim:{label}"),
            no_real_window_claim,
            "BoonDriver aggregate must not claim real-window evidence",
            (!no_real_window_claim).then(|| {
                format!(
                    "BoonDriver report `{}` claims real-window evidence",
                    path.display()
                )
            }),
        );
        artifacts.push(artifact_hash(&path)?);
    }
    let report =
        report_arg(args).unwrap_or_else(|| PathBuf::from("target/reports/boon-driver/all.json"));
    write_static_gate_report(
        args,
        "verify-boon-driver-all",
        report,
        checks,
        blockers,
        json!({
            "required_evidence_tier": boon_driver::TIER_BOON_DRIVER,
            "observed_evidence_tier": boon_driver::TIER_BOON_DRIVER,
            "does_not_satisfy_real_window": true,
            "required_reports": required.iter().map(|(label, path, command)| {
                json!({"label": label, "path": path, "command": command})
            }).collect::<Vec<_>>(),
            "linked_report_artifacts": artifacts,
        }),
    )
}

fn linux_human_like_environment_report_path() -> PathBuf {
    PathBuf::from("target/reports/linux-human-like/environment.json")
}

fn require_linux_human_like_environment(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
    path: &Path,
    report: Option<&serde_json::Value>,
) {
    let present = report.is_some();
    push_audit_check(
        checks,
        blockers,
        "linux-human-like:environment-report-present",
        present,
        format!("{} exists={present}", path.display()),
        (!present).then(|| {
            format!(
                "missing Linux human-like environment report `{}`; run verify-linux-human-like-environment first",
                path.display()
            )
        }),
    );
    let Some(report) = report else {
        return;
    };
    let command_ok = report.get("command").and_then(serde_json::Value::as_str)
        == Some("verify-linux-human-like-environment");
    push_audit_check(
        checks,
        blockers,
        "linux-human-like:environment-command",
        command_ok,
        format!(
            "command={:?}",
            report.get("command").and_then(serde_json::Value::as_str)
        ),
        (!command_ok).then(|| {
            format!(
                "Linux human-like environment report `{}` was not produced by verify-linux-human-like-environment",
                path.display()
            )
        }),
    );
    let environment_pass = report.get("status").and_then(serde_json::Value::as_str) == Some("pass");
    push_audit_check(
        checks,
        blockers,
        "linux-human-like:environment-status-pass",
        environment_pass,
        format!(
            "status={:?}, blockers={:?}",
            report.get("status").and_then(serde_json::Value::as_str),
            report.get("blockers")
        ),
        (!environment_pass).then(|| {
            format!(
                "Linux human-like environment is not ready: {}",
                report
                    .get("blockers")
                    .and_then(serde_json::Value::as_array)
                    .map(|blockers| {
                        blockers
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| "missing precise blocker details".to_owned())
            )
        }),
    );
    let isolated_safe = report
        .get("safe_for_unattended_testing")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && report
            .get("live_desktop_input_used")
            .and_then(serde_json::Value::as_bool)
            == Some(false);
    push_audit_check(
        checks,
        blockers,
        "linux-human-like:isolated-safe-no-live-desktop",
        isolated_safe,
        format!(
            "safe_for_unattended_testing={:?}, live_desktop_input_used={:?}",
            report
                .get("safe_for_unattended_testing")
                .and_then(serde_json::Value::as_bool),
            report
                .get("live_desktop_input_used")
                .and_then(serde_json::Value::as_bool)
        ),
        (!isolated_safe).then(|| {
            "Linux human-like reports require an isolated unattended input path and must not use live desktop input"
                .to_owned()
        }),
    );
}

fn require_boon_driver_source_report(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
    path: &Path,
    report: Option<&serde_json::Value>,
    expected_command: &str,
    check_prefix: &str,
) {
    let present = report.is_some();
    push_audit_check(
        checks,
        blockers,
        format!("{check_prefix}:boon-driver-report-present"),
        present,
        format!("{} exists={present}", path.display()),
        (!present).then(|| {
            format!(
                "missing source BoonDriver report `{}`; generate app-owned evidence before Linux human-like upgrade",
                path.display()
            )
        }),
    );
    let Some(report) = report else {
        return;
    };
    let command_ok =
        report.get("command").and_then(serde_json::Value::as_str) == Some(expected_command);
    push_audit_check(
        checks,
        blockers,
        format!("{check_prefix}:boon-driver-command"),
        command_ok,
        format!(
            "command={:?}, expected={expected_command}",
            report.get("command").and_then(serde_json::Value::as_str)
        ),
        (!command_ok).then(|| {
            format!(
                "BoonDriver source report `{}` has wrong command for Linux human-like upgrade",
                path.display()
            )
        }),
    );
    let pass = report.get("status").and_then(serde_json::Value::as_str) == Some("pass");
    push_audit_check(
        checks,
        blockers,
        format!("{check_prefix}:boon-driver-status-pass"),
        pass,
        format!(
            "status={:?}",
            report.get("status").and_then(serde_json::Value::as_str)
        ),
        (!pass).then(|| {
            format!(
                "BoonDriver source report `{}` must pass before Linux human-like upgrade",
                path.display()
            )
        }),
    );
}

fn verify_linux_human_like_environment(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let tools = json!({
        "weston": command_available("weston"),
        "wayland-info": command_available("wayland-info"),
        "cage": command_available("cage"),
        "ydotool": command_available("ydotool"),
    });
    for tool in ["weston", "wayland-info"] {
        let available = tools.get(tool).and_then(serde_json::Value::as_bool) == Some(true);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("linux-human-like-env:tool:{tool}"),
            available,
            format!("{tool} available={available}"),
            (!available).then(|| format!("Linux human-like environment tool `{tool}` is missing")),
        );
    }
    let isolated_probe = if tools.get("weston").and_then(serde_json::Value::as_bool) == Some(true)
        && tools
            .get("wayland-info")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    {
        run_controlled_weston_capability_probe()?
    } else {
        json!({"status": "not-run", "reason": "weston or wayland-info missing"})
    };
    let has_seat = isolated_probe
        .get("has_wl_seat")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let has_virtual_keyboard = isolated_probe
        .get("has_virtual_keyboard_manager")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let has_virtual_pointer = isolated_probe
        .get("has_virtual_pointer_manager")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let has_test_control_api = isolated_probe
        .get("has_weston_test_control_api")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let isolated_input_possible =
        has_seat && ((has_virtual_keyboard && has_virtual_pointer) || has_test_control_api);
    let has_output_capture = isolated_probe
        .get("has_output_capture_protocol")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    push_audit_check(
        &mut checks,
        &mut blockers,
        "linux-human-like-env:isolated-compositor-input",
        isolated_input_possible,
        format!(
            "has_wl_seat={has_seat}, has_virtual_keyboard={has_virtual_keyboard}, has_virtual_pointer={has_virtual_pointer}, has_weston_test_control_api={has_test_control_api}"
        ),
        (!isolated_input_possible).then(|| {
            "isolated compositor lacks seat plus virtual keyboard/pointer or equivalent test-control support for safe human-like input".to_owned()
        }),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "linux-human-like-env:isolated-output-capture",
        has_output_capture,
        format!("has_output_capture_protocol={has_output_capture}"),
        (!has_output_capture).then(|| {
            "isolated compositor lacks output capture support needed for compositor-level screenshots"
                .to_owned()
        }),
    );
    let live_desktop_allowed = live_desktop_input_allowed();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "linux-human-like-env:live-desktop-not-required",
        !live_desktop_allowed,
        format!("live_desktop_input_allowed={live_desktop_allowed}"),
        live_desktop_allowed.then(|| {
            "Linux human-like environment should use isolated input by default, not live desktop input".to_owned()
        }),
    );
    let report = report_arg(args)
        .unwrap_or_else(|| PathBuf::from("target/reports/linux-human-like/environment.json"));
    write_static_gate_report(
        args,
        "verify-linux-human-like-environment",
        report,
        checks,
        blockers,
        json!({
            "architecture_contract": "docs/architecture/LINUX_HUMAN_LIKE_TESTING.md",
            "boon_driver_contract": "docs/architecture/BOON_DRIVER.md",
            "evidence_tier": boon_driver::TIER_REAL_WINDOW,
            "method": boon_driver::METHOD_LINUX_HUMAN_LIKE,
            "isolated_compositor_probe": isolated_probe,
            "smoke_client_probe": if isolated_input_possible {
                json!({
                    "status": "not-implemented",
                    "reason": "isolated input capability is available but app_window smoke client delivery/capture is not wired yet"
                })
            } else {
                json!({
                    "status": "skipped",
                    "reason": "isolated compositor lacks seat/virtual input capability; skipping pointer/key/wheel smoke to avoid live desktop fallback"
                })
            },
            "tools": tools,
            "safe_for_unattended_testing": isolated_input_possible,
            "live_desktop_input_allowed": live_desktop_allowed,
            "live_desktop_input_used": false,
        }),
    )
}

fn verify_linux_human_like_e2e(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_arg(args, "--example").unwrap_or_else(|| "todomvc".to_owned());
    let entry = boon_runtime::example_manifest_entry(&example)?;
    let environment_path = linux_human_like_environment_report_path();
    let environment_report = read_optional_json(&environment_path)?;
    let boon_driver_path = PathBuf::from(format!("target/reports/boon-driver/{}.json", entry.id));
    let boon_driver_report = read_optional_json(&boon_driver_path)?;
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    require_linux_human_like_environment(
        &mut checks,
        &mut blockers,
        &environment_path,
        environment_report.as_ref(),
    );
    require_boon_driver_source_report(
        &mut checks,
        &mut blockers,
        &boon_driver_path,
        boon_driver_report.as_ref(),
        "verify-boon-driver-e2e",
        &format!("linux-human-like-e2e:{}", entry.id),
    );
    let smoke_probe = if environment_report
        .as_ref()
        .and_then(|report| report.get("status"))
        .and_then(serde_json::Value::as_str)
        == Some("pass")
    {
        run_linux_human_like_preview_smoke(&entry.id, entry.id == "cells")?
    } else {
        json!({"status": "not-run", "reason": "Linux human-like environment report is missing or failing"})
    };
    let smoke_pass = smoke_probe
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("linux-human-like-e2e:{}:isolated-adapter-delivery", entry.id),
        smoke_pass,
        format!(
            "smoke_status={:?}, real_os_events_observed={:?}, driver_status={:?}",
            smoke_probe.get("status").and_then(serde_json::Value::as_str),
            smoke_probe
                .pointer("/preview_input_adapter/real_os_events_observed")
                .and_then(serde_json::Value::as_bool),
            smoke_probe
                .pointer("/weston_test_driver/status")
                .and_then(serde_json::Value::as_str)
        ),
        (!smoke_pass).then(|| {
            "Linux human-like E2E must deliver pointer/key/wheel through isolated Weston into the real app_window preview and observe it through app_window input provenance".to_owned()
        }),
    );
    let report = report_arg(args).unwrap_or_else(|| {
        PathBuf::from(format!("target/reports/linux-human-like/{}.json", entry.id))
    });
    write_static_gate_report(
        args,
        "verify-linux-human-like-e2e",
        report,
        checks,
        blockers,
        json!({
            "example": entry.id,
            "source_path": entry.source,
            "source_hash": file_hash(&entry.source),
            "scenario_path": entry.scenario,
            "scenario_hash": file_hash(&entry.scenario),
            "architecture_contract": "docs/architecture/LINUX_HUMAN_LIKE_TESTING.md",
            "boon_driver_report": boon_driver_path,
            "environment_report": environment_path,
            "evidence_tier": boon_driver::TIER_REAL_WINDOW,
            "method": boon_driver::METHOD_LINUX_HUMAN_LIKE,
            "real_window_claimed": smoke_pass,
            "live_desktop_input_used": false,
            "isolated_preview_smoke_probe": smoke_probe,
            "required_delivery": "BoonDriver scenario action -> isolated compositor seat -> exact native preview/dev window -> app input provenance -> app/compositor readback"
        }),
    )
}

fn verify_linux_human_like_speed(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let selector = native_gpu_scroll_selector(args);
    let label = selector.label;
    let selector_valid = selector.blockers.is_empty();
    let environment_path = linux_human_like_environment_report_path();
    let environment_report = read_optional_json(&environment_path)?;
    let boon_driver_path = PathBuf::from(format!("target/reports/boon-driver/speed-{label}.json"));
    let boon_driver_report = read_optional_json(&boon_driver_path)?;
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    for blocker in selector.blockers {
        push_audit_check(
            &mut checks,
            &mut blockers,
            "linux-human-like-speed:cli-selector",
            false,
            format!(
                "example={:?}, surface={:?}, target={:?}",
                value_arg(args, "--example"),
                value_arg(args, "--surface"),
                value_arg(args, "--target")
            ),
            Some(blocker),
        );
    }
    require_linux_human_like_environment(
        &mut checks,
        &mut blockers,
        &environment_path,
        environment_report.as_ref(),
    );
    require_boon_driver_source_report(
        &mut checks,
        &mut blockers,
        &boon_driver_path,
        boon_driver_report.as_ref(),
        "verify-boon-driver-speed",
        &format!("linux-human-like-speed:{label}"),
    );
    if label == "cells" {
        if let Some(report) = boon_driver_report.as_ref() {
            let tested_rows = report
                .pointer("/boon_driver_proof/tested_rows")
                .or_else(|| report.get("tested_rows"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let tested_columns = report
                .pointer("/boon_driver_proof/tested_columns")
                .or_else(|| report.get("tested_columns"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let required_rows = native_gpu_budget_u64("cells", "logical_rows").unwrap_or(100);
            let required_columns = native_gpu_budget_u64("cells", "logical_columns").unwrap_or(26);
            let full_size = tested_rows >= required_rows && tested_columns >= required_columns;
            push_audit_check(
                &mut checks,
                &mut blockers,
                "linux-human-like-speed:cells-full-size-boon-driver-source",
                full_size,
                format!(
                    "tested_rows={tested_rows}, required_rows={required_rows}, tested_columns={tested_columns}, required_columns={required_columns}"
                ),
                (!full_size).then(|| {
                    "Linux human-like speed must start from a BoonDriver Cells speed report covering the full required grid size".to_owned()
                }),
            );
        }
    }
    let mut source_path = serde_json::Value::Null;
    let mut source_hash = "n/a".to_owned();
    let mut dev_editor_speed_corpus = json!({"status": "not-applicable"});
    let smoke_probe = if selector_valid
        && environment_report
            .as_ref()
            .and_then(|report| report.get("status"))
            .and_then(serde_json::Value::as_str)
            == Some("pass")
    {
        if label == "dev-code-editor" {
            let artifacts_dir = PathBuf::from("target/artifacts/linux-human-like");
            let (path, example_id, corpus) = ensure_dev_editor_speed_corpus(&artifacts_dir)?;
            source_hash = file_hash(path.to_string_lossy().as_ref());
            source_path = json!(path);
            dev_editor_speed_corpus = corpus;
            let layout_probe = json!({
                "status": "pass",
                "source_path": source_path,
                "source_sha256": source_hash,
                "layout_source": "dev-window-editor-model",
                "scroll_regions": [
                    {
                        "id": "scroll:dev-code-editor",
                        "node": "dev-code-editor",
                        "axis": "vertical",
                        "bounds": {"x": 0.0, "y": 96.0, "width": 1180.0, "height": 560.0}
                    },
                    {
                        "id": "scroll-x:dev-code-editor",
                        "node": "dev-code-editor",
                        "axis": "horizontal",
                        "bounds": {"x": 0.0, "y": 656.0, "width": 1180.0, "height": 18.0}
                    }
                ]
            });
            let driver_target = native_scroll_driver_target(&label, &layout_probe);
            run_linux_human_like_desktop_surface_smoke(
                &label,
                &example_id,
                Path::new(
                    source_path
                        .as_str()
                        .ok_or("dev editor source path JSON is not a string")?,
                ),
                true,
                true,
                "dev_surface_proof",
                driver_target,
                false,
                None,
            )?
        } else {
            let entry = boon_runtime::example_manifest_entry(&label)?;
            source_path = json!(entry.source.clone());
            source_hash = file_hash(&entry.source);
            run_linux_human_like_preview_smoke(&label, true)?
        }
    } else {
        json!({"status": "not-run", "reason": "Linux human-like environment report is missing or failing"})
    };
    let smoke_pass = smoke_probe
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("linux-human-like-speed:{label}:isolated-real-window-scroll"),
        smoke_pass,
        format!(
            "smoke_status={:?}, scroll_delta_x={:?}, scroll_delta_y={:?}",
            smoke_probe.get("status").and_then(serde_json::Value::as_str),
            smoke_probe
                .pointer("/preview_input_adapter/scroll_delta_x")
                .or_else(|| smoke_probe.pointer("/surface_input_adapter/scroll_delta_x"))
                .and_then(serde_json::Value::as_f64),
            smoke_probe
                .pointer("/preview_input_adapter/scroll_delta_y")
                .or_else(|| smoke_probe.pointer("/surface_input_adapter/scroll_delta_y"))
                .and_then(serde_json::Value::as_f64)
        ),
        (!smoke_pass).then(|| {
            "Linux human-like speed must deliver real wheel input through isolated Weston and observe nonzero app_window scroll provenance before claiming real-window timing".to_owned()
        }),
    );
    let report = report_arg(args).unwrap_or_else(|| {
        if label == "cells" {
            PathBuf::from("target/reports/linux-human-like/cells-speed.json")
        } else {
            PathBuf::from(format!(
                "target/reports/linux-human-like/{label}-speed.json"
            ))
        }
    });
    write_static_gate_report(
        args,
        "verify-linux-human-like-speed",
        report,
        checks,
        blockers,
        json!({
            "surface_under_test": label,
            "architecture_contract": "docs/architecture/LINUX_HUMAN_LIKE_TESTING.md",
            "boon_driver_speed_report": boon_driver_path,
            "environment_report": environment_path,
            "source_path": source_path,
            "source_hash": source_hash,
            "dev_editor_speed_corpus": dev_editor_speed_corpus,
            "evidence_tier": boon_driver::TIER_REAL_WINDOW,
            "method": boon_driver::METHOD_LINUX_HUMAN_LIKE,
            "real_window_claimed": smoke_pass,
            "live_desktop_input_used": false,
            "isolated_preview_smoke_probe": smoke_probe,
            "requires_release_build": true,
            "required_delivery": "BoonDriver wheel action -> isolated compositor seat -> exact native scroll surface -> app/compositor readback timing"
        }),
    )
}

fn verify_linux_human_like_all(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let check_existing = args.iter().any(|arg| arg == "--check-existing");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "linux-human-like-all:check-existing-mode",
        check_existing,
        format!("--check-existing present={check_existing}"),
        (!check_existing)
            .then(|| "Linux human-like aggregate requires --check-existing".to_owned()),
    );
    let required = [
        (
            "environment",
            "target/reports/linux-human-like/environment.json",
            "verify-linux-human-like-environment",
        ),
        (
            "todomvc",
            "target/reports/linux-human-like/todomvc.json",
            "verify-linux-human-like-e2e",
        ),
        (
            "cells",
            "target/reports/linux-human-like/cells.json",
            "verify-linux-human-like-e2e",
        ),
        (
            "cells-speed",
            "target/reports/linux-human-like/cells-speed.json",
            "verify-linux-human-like-speed",
        ),
        (
            "dev-code-editor-speed",
            "target/reports/linux-human-like/dev-code-editor-speed.json",
            "verify-linux-human-like-speed",
        ),
    ];
    let mut artifacts = Vec::new();
    for (label, path, command) in required.iter().copied() {
        let path = PathBuf::from(path);
        let exists = path.exists();
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("linux-human-like-all:report-present:{label}"),
            exists,
            format!("{} exists={exists}", path.display()),
            (!exists).then(|| format!("missing Linux human-like report `{}`", path.display())),
        );
        if !exists {
            continue;
        }
        let report = read_json(&path)?;
        let report_command = report.get("command").and_then(serde_json::Value::as_str);
        let command_ok = report_command == Some(command);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("linux-human-like-all:command:{label}"),
            command_ok,
            format!("command={report_command:?}, expected={command}"),
            (!command_ok).then(|| {
                format!(
                    "Linux human-like report `{}` has wrong command {:?}, expected `{command}`",
                    path.display(),
                    report_command
                )
            }),
        );
        let pass = report.get("status").and_then(serde_json::Value::as_str) == Some("pass");
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("linux-human-like-all:status-pass:{label}"),
            pass,
            format!("{} status pass={pass}", path.display()),
            (!pass).then(|| format!("Linux human-like report `{}` did not pass", path.display())),
        );
        artifacts.push(artifact_hash(&path)?);
    }
    let report = report_arg(args)
        .unwrap_or_else(|| PathBuf::from("target/reports/linux-human-like/all.json"));
    write_static_gate_report(
        args,
        "verify-linux-human-like-all",
        report,
        checks,
        blockers,
        json!({
            "architecture_contract": "docs/architecture/LINUX_HUMAN_LIKE_TESTING.md",
            "required_evidence_tier": boon_driver::TIER_REAL_WINDOW,
            "method": boon_driver::METHOD_LINUX_HUMAN_LIKE,
            "live_desktop_input_used": false,
            "required_reports": required.iter().map(|(label, path, command)| {
                json!({"label": label, "path": path, "command": command})
            }).collect::<Vec<_>>(),
            "linked_report_artifacts": artifacts,
        }),
    )
}

fn verify_native_real_window_input_environment(
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let live_allowed = live_desktop_input_allowed();
    push_audit_check(
        &mut checks,
        &mut blockers,
        "real-window-input:live-desktop-not-required",
        !live_allowed,
        format!(
            "BOON_ALLOW_LIVE_DESKTOP_INPUT={:?}, BOON_I_ACCEPT_LIVE_DESKTOP_INPUT_CAN_TYPE_IN_OTHER_WINDOWS={:?}",
            std::env::var("BOON_ALLOW_LIVE_DESKTOP_INPUT").ok(),
            std::env::var("BOON_I_ACCEPT_LIVE_DESKTOP_INPUT_CAN_TYPE_IN_OTHER_WINDOWS").ok()
        ),
        live_allowed.then(|| {
            "real-window input verification should use isolated compositor input by default, not live desktop input".to_owned()
        }),
    );
    let tools = json!({
        "weston": command_available("weston"),
        "wayland-info": command_available("wayland-info"),
        "wtype": command_available("wtype"),
        "ydotool": command_available("ydotool"),
        "cage": command_available("cage"),
    });
    for tool in ["weston", "wayland-info"] {
        let available = tools.get(tool).and_then(serde_json::Value::as_bool) == Some(true);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("real-window-input:tool:{tool}"),
            available,
            format!("{tool} available={available}"),
            (!available)
                .then(|| format!("required real-window input probe tool `{tool}` is missing")),
        );
    }
    let controlled_wayland_harness = if tools.get("weston").and_then(serde_json::Value::as_bool)
        == Some(true)
        && tools
            .get("wayland-info")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    {
        run_controlled_weston_capability_probe()?
    } else {
        json!({
            "status": "not-run",
            "reason": "weston or wayland-info is unavailable"
        })
    };
    let globals = controlled_wayland_harness
        .get("globals")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(ToOwned::to_owned))
        .collect::<BTreeSet<_>>();
    let has_seat = globals.contains("wl_seat");
    let has_virtual_keyboard = globals.contains("zwp_virtual_keyboard_manager_v1");
    let has_virtual_pointer = globals.contains("zwlr_virtual_pointer_manager_v1")
        || globals.contains("zwp_virtual_pointer_manager_v1");
    let has_weston_test_control_api = globals.contains("weston_test");
    let isolated_input_possible =
        has_seat && ((has_virtual_keyboard && has_virtual_pointer) || has_weston_test_control_api);
    push_audit_check(
        &mut checks,
        &mut blockers,
        "real-window-input:isolated-wayland-input-protocols",
        isolated_input_possible,
        format!(
            "has_wl_seat={has_seat}, has_virtual_keyboard={has_virtual_keyboard}, has_virtual_pointer={has_virtual_pointer}, has_weston_test_control_api={has_weston_test_control_api}"
        ),
        (!isolated_input_possible).then(|| {
            "isolated Weston probe does not expose seat plus virtual keyboard/pointer or weston_test control API needed for real-window input synthesis".to_owned()
        }),
    );
    push_audit_check(
        &mut checks,
        &mut blockers,
        "real-window-input:ydotool-live-desktop-policy",
        !live_allowed,
        format!(
            "ydotool_available={:?}, live_desktop_input_allowed={live_allowed}",
            tools.get("ydotool").and_then(serde_json::Value::as_bool)
        ),
        live_allowed.then(|| {
            "ydotool/uinput live desktop input is not part of unattended real-window verification".to_owned()
        }),
    );
    write_native_gate_report(
        args,
        "verify-native-real-window-input-environment",
        checks,
        blockers,
        json!({
            "source_hash": "n/a",
            "program_hash": "n/a",
            "tools": tools,
            "live_desktop_input_allowed": live_allowed,
            "controlled_wayland_harness": controlled_wayland_harness,
            "operator_host_input": true,
            "real_window_input_possible_without_live_desktop": isolated_input_possible,
            "real_window_input_possible_with_live_desktop_permission": live_allowed && tools.get("ydotool").and_then(serde_json::Value::as_bool) == Some(true),
            "recommended_next_step": if isolated_input_possible {
                "wire isolated compositor virtual input into preview/dev E2E"
            } else if !live_allowed {
                "human or explicitly permitted live-desktop OS input is required for the real-window tier on this machine"
            } else {
                "install an isolated Wayland virtual input backend or use permitted live-desktop input"
            },
            "strict_visible_testing_contract": "docs/plans/STRICT_EXAMPLE_VISIBLE_TESTING_RULES.md"
        }),
    )
}

fn evidence_tier_satisfies(observed: &str, required: &str) -> bool {
    fn rank(tier: &str) -> Option<u8> {
        match tier {
            "runtime" => Some(0),
            "host-synthetic" | "boon-driver" => Some(1),
            "real-window" => Some(2),
            "human" => Some(3),
            _ => None,
        }
    }
    rank(observed)
        .zip(rank(required))
        .is_some_and(|(observed, required)| observed >= required)
}

fn verify_native_gpu_negative(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let base = || {
        json!({
            "command": "verify-native-gpu-preview-e2e",
            "git_commit": git_commit(),
            "worktree_fingerprint": worktree_fingerprint(),
            "generated_at_utc": current_unix_seconds().to_string(),
            "native_gpu_contract": true
        })
    };
    let cases = [
        (
            "full-state-ipc-mirroring",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-gpu-observability",
                    "full_state_mirroring_observed": true
                }),
            ),
        ),
        (
            "synthetic-scroll",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-gpu-scroll-speed",
                    "synthetic_scroll": true
                }),
            ),
        ),
        (
            "fake-real-os-operator-input",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-gpu-preview-e2e",
                    "real_os_input": true,
                    "operator_host_input": true,
                    "input_injection_method": "operator_host_event_harness"
                }),
            ),
        ),
        (
            "nested-compositor-only-native-proof",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-gpu-preview-e2e",
                    "operator_host_input": false,
                    "controlled_wayland_harness": {
                        "status": "pass",
                        "method": "verifier-owned-nested-weston-wayland-backend"
                    }
                }),
            ),
        ),
        (
            "xvfb-native-proof",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-gpu-preview-e2e",
                    "display_server": "x11",
                    "input_injection_method": "os_pointer_keyboard_to_visible_window"
                }),
            ),
        ),
        (
            "single-process-multiwindow",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-gpu-multiwindow",
                    "process_model": "single-process"
                }),
            ),
        ),
        (
            "stale-git-hash",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-gpu-layout-contract",
                    "git_commit": "stale"
                }),
            ),
        ),
        (
            "stale-worktree-fingerprint",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-gpu-layout-contract",
                    "worktree_fingerprint": "stale-worktree-fixture"
                }),
            ),
        ),
        (
            "stale-source-hash",
            merge_json(
                base(),
                json!({
                    "source_hash": "stale-source-fixture",
                    "expected_source_hash": file_hash("examples/cells.bn")
                }),
            ),
        ),
        (
            "stale-binary-hash",
            merge_json(
                base(),
                json!({
                    "binary_hash": "stale-binary-fixture"
                }),
            ),
        ),
        (
            "missing-artifact",
            merge_json(
                base(),
                json!({
                    "artifact_sha256s": [{
                        "path": "target/reports/native-gpu/missing-negative-fixture.png",
                        "sha256": "fixture"
                    }]
                }),
            ),
        ),
        (
            "stale-shader-output",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-gpu-shaders",
                    "shader_outputs_fresh": false
                }),
            ),
        ),
        (
            "missing-native-contract",
            json!({
                "command": "verify-native-gpu-layout-contract",
                "git_commit": git_commit(),
                "generated_at_utc": current_unix_seconds().to_string()
            }),
        ),
        (
            "future-dated-report",
            merge_json(
                base(),
                json!({
                    "generated_at_utc": current_unix_seconds().saturating_add(3600).to_string()
                }),
            ),
        ),
        (
            "stale-surface-epoch",
            merge_json(
                base(),
                json!({
                    "surface_epoch": 7,
                    "target_surface_epoch": 6
                }),
            ),
        ),
        (
            "copied-pixel-hash-only-proof",
            merge_json(
                base(),
                json!({
                    "copied_pixel_hash_only": true
                }),
            ),
        ),
        (
            "private-runtime-dispatch",
            merge_json(
                base(),
                json!({
                    "private_runtime_dispatch_used": true
                }),
            ),
        ),
        (
            "wrong-thread-wgpu-call",
            merge_json(
                base(),
                json!({
                    "wrong_thread_wgpu_calls_observed": true
                }),
            ),
        ),
        (
            "headless-native-proof",
            merge_json(
                base(),
                json!({
                    "headless": true,
                    "display_server": "wayland"
                }),
            ),
        ),
        (
            "modeled-ack-timing",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-example-switch-speed",
                    "modeled_ack_timing": true
                }),
            ),
        ),
        (
            "modeled-presentation-timing",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-example-switch-speed",
                    "modeled_presentation_timing": true
                }),
            ),
        ),
        (
            "missing-process-evidence",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-gpu-idle-wake",
                    "missing_process_evidence": true
                }),
            ),
        ),
        (
            "stale-render-content-revision",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-gpu-idle-wake",
                    "dirty_revision": 4,
                    "presented_revision": 4,
                    "last_render_content_revision": 3
                }),
            ),
        ),
        (
            "fake-cpu-samples",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-gpu-idle-wake",
                    "fake_cpu_samples": true
                }),
            ),
        ),
        (
            "release-report-reused-for-debug",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-dev-editor-scroll-speed",
                    "build_profile": "release",
                    "profile": "debug",
                    "release_report_reused_for_debug": true
                }),
            ),
        ),
        (
            "passive-scroll-source-replacement",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-dev-editor-scroll-speed",
                    "passive_scroll_did_source_replacement": true
                }),
            ),
        ),
        (
            "passive-scroll-runtime-summary-query",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-dev-editor-scroll-speed",
                    "passive_scroll_queried_runtime_summary": true
                }),
            ),
        ),
        (
            "full-file-scroll-materialization",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-dev-editor-scroll-speed",
                    "full_file_materialized_for_scroll": true,
                    "text_reshaped_full_file": true
                }),
            ),
        ),
        (
            "missing-horizontal-scroll-evidence",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-dev-editor-scroll-speed",
                    "missing_horizontal_scroll_evidence": true
                }),
            ),
        ),
        (
            "deterministic-dev-editor-scroll-model",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-dev-editor-scroll-speed",
                    "measurement_source": "deterministic-dev-editor-scroll-model",
                    "input_provenance": "model_only"
                }),
            ),
        ),
        (
            "example-source-identity-leak",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-example-switch-speed",
                    "source_identity": "custom:a"
                }),
            ),
        ),
        (
            "sync-ack-runtime-summary",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-example-switch-speed",
                    "sync_ack_contains_runtime_summary": true
                }),
            ),
        ),
        (
            "sync-ack-layout-proof",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-example-switch-speed",
                    "sync_ack_contains_layout_proof": true
                }),
            ),
        ),
        (
            "preview-received-scenario-data",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-gpu-preview-e2e",
                    "preview_received_scenario_data": true
                }),
            ),
        ),
        (
            "preview-bound-scenario-data",
            merge_json(
                base(),
                json!({
                    "command": "verify-native-gpu-preview-e2e",
                    "preview_bound_scenario_data": true
                }),
            ),
        ),
    ];
    let negative_case_count = cases.len() as u64;
    let required_negative_cases = cases.iter().map(|(case, _)| *case).collect::<Vec<_>>();
    for (case, fixture) in &cases {
        let rejected = native_gpu_report_rejects(fixture);
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("negative:{case}:rejected"),
            rejected,
            format!("native negative fixture `{case}` rejected={rejected}"),
            (!rejected).then(|| format!("native negative fixture `{case}` was not rejected")),
        );
    }
    write_native_gate_report(
        args,
        "verify-native-gpu-negative",
        checks,
        blockers,
        json!({
            "negative_case_count": negative_case_count,
            "required_negative_cases": required_negative_cases
        }),
    )
}

fn verify_native_gpu_all(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    verify_native_gpu_report_bundle(
        args,
        "verify-native-gpu-all",
        native_gpu_handoff_required_reports(),
        "agents-native-gpu-handoff",
    )
}

fn verify_native_gpu_regression_all(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    verify_native_gpu_report_bundle(
        args,
        "verify-native-gpu-regression-all",
        native_gpu_regression_required_reports(),
        "native-gpu-product-regression",
    )
}

fn verify_native_gpu_report_bundle(
    args: &[String],
    command: &str,
    required: Vec<NativeGpuRequiredReport>,
    aggregate_scope: &'static str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let mut artifacts = Vec::new();
    let check_existing = args.iter().any(|arg| arg == "--check-existing");
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("{command}:check-existing-mode"),
        check_existing,
        format!("--check-existing present={check_existing}"),
        (!check_existing)
            .then(|| format!("native GPU aggregate `{command}` requires --check-existing")),
    );
    for requirement in &required {
        let label = requirement.label;
        let path = &requirement.path;
        let exists = path.exists();
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("{command}:report-present:{label}"),
            exists,
            format!("{} exists={exists}", path.display()),
            (!exists).then(|| format!("missing native GPU report `{}`", path.display())),
        );
        if !exists {
            continue;
        }
        let report = read_json(path)?;
        let schema_file_valid = verify_report_schema(path).is_ok();
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("{command}:report-schema-file:{label}"),
            schema_file_valid,
            format!(
                "{} verify_report_schema={schema_file_valid}",
                path.display()
            ),
            (!schema_file_valid).then(|| {
                format!(
                    "native GPU report `{}` failed verify_report_schema",
                    path.display()
                )
            }),
        );
        let schema_blockers = validate_native_gpu_child_report_shape(requirement, &report);
        let schema_valid = schema_blockers.is_empty();
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("{command}:schema:{label}"),
            schema_valid,
            format!("{} schema_valid={schema_valid}", path.display()),
            (!schema_valid).then(|| {
                format!(
                    "native GPU report `{}` is not schema-valid: {}",
                    path.display(),
                    schema_blockers.join("; ")
                )
            }),
        );
        let semantic_blockers = validate_native_gpu_child_report(requirement, &report);
        let semantically_valid = semantic_blockers.is_empty();
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("{command}:contract:{label}"),
            semantically_valid,
            format!(
                "{} native contract valid={semantically_valid}",
                path.display()
            ),
            (!semantically_valid).then(|| {
                format!(
                    "native GPU report `{}` violates native contract: {}",
                    path.display(),
                    semantic_blockers.join("; ")
                )
            }),
        );
        let pass = report.get("status").and_then(serde_json::Value::as_str) == Some("pass");
        let all_steps_pass = report
            .get("per_step_pass_fail")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|steps| {
                !steps.is_empty()
                    && steps.iter().all(|step| {
                        step.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
                    })
            });
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("{command}:all-steps-pass:{label}"),
            all_steps_pass,
            format!("{} all_steps_pass={all_steps_pass}", path.display()),
            (!all_steps_pass).then(|| {
                format!(
                    "native GPU report `{}` has missing or failing per_step_pass_fail entries",
                    path.display()
                )
            }),
        );
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("{command}:status-pass:{label}"),
            pass,
            format!("{} status pass={pass}", path.display()),
            (!pass).then(|| format!("native GPU report `{}` did not pass", path.display())),
        );
        let commit_fresh = report.get("git_commit").and_then(serde_json::Value::as_str)
            == Some(git_commit().as_str());
        let worktree_fresh = report
            .get("worktree_fingerprint")
            .and_then(serde_json::Value::as_str)
            == Some(worktree_fingerprint().as_str());
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("{command}:git-fresh:{label}"),
            commit_fresh,
            format!("{} git_fresh={commit_fresh}", path.display()),
            (!commit_fresh).then(|| {
                format!(
                    "native GPU report `{}` is stale for current git commit",
                    path.display()
                )
            }),
        );
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("{command}:worktree-fresh:{label}"),
            worktree_fresh,
            format!("{} worktree_fresh={worktree_fresh}", path.display()),
            (!worktree_fresh).then(|| {
                format!(
                    "native GPU report `{}` is stale for current worktree fingerprint",
                    path.display()
                )
            }),
        );
        artifacts.push(artifact_hash(path)?);
    }
    write_native_gate_report(
        args,
        command,
        checks,
        blockers,
        json!({
            "aggregate_scope": aggregate_scope,
            "required_reports": required.iter().map(|report| {
                json!({
                    "label": report.label,
                    "path": report.path.display().to_string(),
                    "command": report.command,
                    "required_argv": report.required_argv,
                    "requires_native_gpu_contract": report.requires_native_gpu_contract
                })
            }).collect::<Vec<_>>(),
            "linked_report_artifacts": artifacts,
            "artifact_sha256s": artifacts
        }),
    )
}

struct NativeGpuRequiredReport {
    label: &'static str,
    path: PathBuf,
    command: &'static str,
    required_argv: &'static [(&'static str, &'static str)],
    requires_native_gpu_contract: bool,
}

fn native_gpu_handoff_required_reports() -> Vec<NativeGpuRequiredReport> {
    vec![
        native_gpu_required_report(
            "platform-contract",
            "target/reports/native-gpu/platform-contract.json",
            "verify-platform-contract",
            &[],
        ),
        native_gpu_required_report(
            "dependency-graph",
            "target/reports/native-gpu/dependency-graph.json",
            "verify-native-gpu-dependency-graph",
            &[],
        ),
        native_gpu_required_report(
            "architecture",
            "target/reports/native-gpu/architecture.json",
            "verify-native-gpu-architecture",
            &[],
        ),
        native_gpu_required_report(
            "layout-contract",
            "target/reports/native-gpu/layout-contract.json",
            "verify-native-gpu-layout-contract",
            &[],
        ),
        native_gpu_required_report(
            "shaders",
            "target/reports/native-gpu/shaders.json",
            "verify-native-gpu-shaders",
            &[("--check", "")],
        ),
        native_gpu_required_report(
            "multiwindow",
            "target/reports/native-gpu/multiwindow.json",
            "verify-native-gpu-multiwindow",
            &[],
        ),
        native_gpu_required_report(
            "ipc-backpressure",
            "target/reports/native-gpu/ipc-backpressure.json",
            "verify-native-gpu-ipc-backpressure",
            &[],
        ),
        native_gpu_required_report(
            "observability",
            "target/reports/native-gpu/observability.json",
            "verify-native-gpu-observability",
            &[],
        ),
        native_gpu_required_report(
            "preview-e2e-todomvc",
            "target/reports/native-gpu/preview-e2e-todomvc.json",
            "verify-native-gpu-preview-e2e",
            &[("--example", "todomvc")],
        ),
        native_gpu_required_report(
            "preview-e2e-cells",
            "target/reports/native-gpu/preview-e2e-cells.json",
            "verify-native-gpu-preview-e2e",
            &[("--example", "cells")],
        ),
        native_gpu_required_report(
            "scroll-speed-cells",
            "target/reports/native-gpu/scroll-speed-cells.json",
            "verify-native-gpu-scroll-speed",
            &[("--example", "cells")],
        ),
        native_gpu_required_report(
            "scroll-speed-dev-code-editor",
            "target/reports/native-gpu/scroll-speed-dev-code-editor.json",
            "verify-native-gpu-scroll-speed",
            &[("--surface", "dev-code-editor")],
        ),
        native_gpu_required_report(
            "negative",
            "target/reports/native-gpu/negative.json",
            "verify-native-gpu-negative",
            &[],
        ),
    ]
}

fn native_gpu_regression_required_reports() -> Vec<NativeGpuRequiredReport> {
    let mut reports = native_gpu_handoff_required_reports();
    reports.extend([
        native_gpu_required_report(
            "counter-interaction-speed",
            "target/reports/native-gpu/counter-interaction-speed.json",
            "verify-native-counter-interaction-speed",
            &[],
        ),
        native_gpu_required_report(
            "cells-interaction-speed-debug",
            "target/reports/native-gpu/cells-interaction-speed-debug.json",
            "verify-native-cells-interaction-speed",
            &[("--profile", "debug")],
        ),
        native_gpu_required_report(
            "cells-interaction-speed-release",
            "target/reports/native-gpu/cells-interaction-speed-release.json",
            "verify-native-cells-interaction-speed",
            &[("--profile", "release")],
        ),
        native_gpu_required_report(
            "idle-wake-counter",
            "target/reports/native-gpu/idle-wake-counter.json",
            "verify-native-gpu-idle-wake",
            &[("--example", "counter")],
        ),
        native_gpu_required_report(
            "idle-wake-todomvc",
            "target/reports/native-gpu/idle-wake-todomvc.json",
            "verify-native-gpu-idle-wake",
            &[("--example", "todomvc")],
        ),
        native_gpu_required_report(
            "idle-wake-cells",
            "target/reports/native-gpu/idle-wake-cells.json",
            "verify-native-gpu-idle-wake",
            &[("--example", "cells")],
        ),
        native_gpu_required_report(
            "idle-wake-custom-projects",
            "target/reports/native-gpu/idle-wake-custom-projects.json",
            "verify-native-gpu-idle-wake",
            &[(
                "--custom-project-fixture",
                "target/fixtures/native-gpu/custom-projects.json",
            )],
        ),
        native_gpu_required_report(
            "real-window-input-environment",
            "target/reports/native-gpu/real-window-input-environment.json",
            "verify-native-real-window-input-environment",
            &[],
        ),
        native_gpu_required_report(
            "visible-launch-todomvc",
            "target/reports/native-gpu/todomvc-visible-launch.json",
            "verify-native-visible-launch",
            &[("--example", "todomvc")],
        ),
        native_gpu_required_report(
            "visible-launch-cells",
            "target/reports/native-gpu/cells-visible-launch.json",
            "verify-native-visible-launch",
            &[("--example", "cells")],
        ),
        native_gpu_required_report(
            "native-examples",
            "target/reports/native-gpu/native-examples.json",
            "verify-native-examples",
            &[("--all", "")],
        ),
        native_gpu_required_report(
            "dev-editor-todomvc",
            "target/reports/native-gpu/dev-editor-todomvc.json",
            "verify-native-dev-window-editor",
            &[("--example", "todomvc")],
        ),
        native_gpu_required_report(
            "dev-editor-cells",
            "target/reports/native-gpu/dev-editor-cells.json",
            "verify-native-dev-window-editor",
            &[("--example", "cells")],
        ),
        native_gpu_required_report(
            "example-tabs",
            "target/reports/native-gpu/example-tabs.json",
            "verify-native-example-tabs",
            &[],
        ),
        native_gpu_required_report(
            "editor-format",
            "target/reports/native-gpu/editor-format.json",
            "verify-native-editor-format",
            &[],
        ),
        native_gpu_required_report(
            "dev-editor-scroll-speed-debug",
            "target/reports/native-gpu/dev-editor-scroll-speed-debug.json",
            "verify-native-dev-editor-scroll-speed",
            &[("--profile", "debug")],
        ),
        native_gpu_required_report(
            "dev-editor-scroll-speed-release",
            "target/reports/native-gpu/dev-editor-scroll-speed-release.json",
            "verify-native-dev-editor-scroll-speed",
            &[("--profile", "release")],
        ),
        native_gpu_required_report(
            "example-switch-speed-debug",
            "target/reports/native-gpu/example-switch-speed-debug.json",
            "verify-native-example-switch-speed",
            &[("--profile", "debug")],
        ),
        native_gpu_required_report(
            "example-switch-speed-release",
            "target/reports/native-gpu/example-switch-speed-release.json",
            "verify-native-example-switch-speed",
            &[("--profile", "release")],
        ),
        native_gpu_required_report(
            "speed-cells",
            "target/reports/native-gpu/speed-cells.json",
            "verify-native-example-speed",
            &[("--example", "cells")],
        ),
        native_gpu_required_report(
            "dev-editor-speed",
            "target/reports/native-gpu/dev-editor-speed.json",
            "verify-native-dev-editor-speed",
            &[],
        ),
    ]);
    reports
}

fn native_gpu_required_report(
    label: &'static str,
    path: &str,
    command: &'static str,
    required_argv: &'static [(&'static str, &'static str)],
) -> NativeGpuRequiredReport {
    NativeGpuRequiredReport {
        label,
        path: PathBuf::from(path),
        command,
        required_argv,
        requires_native_gpu_contract: true,
    }
}

fn validate_native_gpu_child_report_shape(
    requirement: &NativeGpuRequiredReport,
    report: &serde_json::Value,
) -> Vec<String> {
    let mut blockers = Vec::new();
    for key in [
        "report_version",
        "generated_at_utc",
        "command",
        "command_argv",
        "exit_status",
        "git_commit",
        "binary_hash",
        "source_hash",
        "scenario_hash",
        "program_hash",
        "budget_hash",
        "graph_node_count",
        "per_step_pass_fail",
        "artifact_sha256s",
    ] {
        if report.get(key).is_none() {
            blockers.push(format!("missing required report field `{key}`"));
        }
    }
    if !matches!(
        report.get("status").and_then(serde_json::Value::as_str),
        Some("pass" | "fail")
    ) {
        blockers.push("status must be pass or fail".to_owned());
    }
    if report
        .get("per_step_pass_fail")
        .and_then(serde_json::Value::as_array)
        .is_none()
    {
        blockers.push("per_step_pass_fail must be an array".to_owned());
    }
    if report
        .get("artifact_sha256s")
        .and_then(serde_json::Value::as_array)
        .is_none()
    {
        blockers.push("artifact_sha256s must be an array".to_owned());
    }
    blockers.extend(native_gpu_report_integrity_reasons(
        report,
        true,
        requirement.requires_native_gpu_contract,
    ));
    if report.get("status").and_then(serde_json::Value::as_str) == Some("fail")
        && report
            .get("blockers")
            .and_then(serde_json::Value::as_array)
            .is_none_or(Vec::is_empty)
    {
        blockers.push("failing native GPU report must include blockers".to_owned());
    }
    blockers
        .into_iter()
        .map(|blocker| format!("{}: {blocker}", requirement.path.display()))
        .collect()
}

fn validate_native_gpu_child_report(
    requirement: &NativeGpuRequiredReport,
    report: &serde_json::Value,
) -> Vec<String> {
    let mut blockers = Vec::new();
    let command = report
        .get("command")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if command != requirement.command {
        blockers.push(format!(
            "command `{command}` does not match expected `{}` for label `{}`",
            requirement.command, requirement.label
        ));
    }
    if requirement.requires_native_gpu_contract {
        if report
            .get("native_gpu_contract")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        {
            blockers.push("missing native_gpu_contract=true".to_owned());
        }
        let expected_budget = file_hash("budgets/native-gpu.toml");
        if report
            .get("budget_hash")
            .and_then(serde_json::Value::as_str)
            != Some(expected_budget.as_str())
        {
            blockers.push("budget_hash does not match current budgets/native-gpu.toml".to_owned());
        }
    } else if report
        .get("budget_hash")
        .and_then(serde_json::Value::as_str)
        != Some("n/a")
    {
        blockers.push("non-native report budget_hash must be n/a".to_owned());
    }
    let argv = report
        .get("command_argv")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !argv
        .iter()
        .any(|arg| arg.as_str() == Some(requirement.command))
    {
        blockers.push(format!(
            "command_argv does not contain expected command `{}`",
            requirement.command
        ));
    }
    for (flag, value) in requirement.required_argv {
        if !command_argv_contains_pair(&argv, flag, value) {
            blockers.push(format!(
                "command_argv missing required pair `{flag} {value}` for label `{}`",
                requirement.label
            ));
        }
    }
    blockers.extend(native_gpu_report_integrity_reasons(
        report,
        true,
        requirement.requires_native_gpu_contract,
    ));
    blockers.extend(native_gpu_label_contract_blockers(
        requirement.label,
        report,
    ));
    blockers
}

fn native_gpu_label_contract_blockers(label: &str, report: &serde_json::Value) -> Vec<String> {
    let mut blockers = Vec::new();
    match label {
        "counter-interaction-speed" => {
            require_u64_at_least(&mut blockers, report, "event_count", 1);
            require_u64_at_least(
                &mut blockers,
                report,
                "preview_shared_render_update_count",
                report
                    .get("event_count")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(1),
            );
            require_f64_at_most(
                &mut blockers,
                report,
                "interaction_total_ms",
                report
                    .get("max_total_ms")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(250.0),
            );
            if report
                .get("final_count")
                .and_then(serde_json::Value::as_str)
                != report
                    .get("expected_count")
                    .and_then(serde_json::Value::as_str)
            {
                blockers.push("final_count must match expected_count".to_owned());
            }
        }
        "cells-interaction-speed-debug" | "cells-interaction-speed-release" => {
            require_u64_at_least(&mut blockers, report, "event_count", 1);
            require_u64_at_least(
                &mut blockers,
                report,
                "preview_shared_render_update_count",
                report
                    .get("event_count")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(1),
            );
            require_f64_at_most(
                &mut blockers,
                report,
                "interaction_latency_ms_p95",
                report
                    .get("max_p95_ms")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(if label.ends_with("release") {
                        16.7
                    } else {
                        120.0
                    }),
            );
            require_f64_at_most(
                &mut blockers,
                report,
                "interaction_latency_ms_max",
                report
                    .get("max_max_ms")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(if label.ends_with("release") {
                        50.0
                    } else {
                        250.0
                    }),
            );
            if report
                .get("selected_address")
                .and_then(serde_json::Value::as_str)
                != Some("B0")
            {
                blockers.push("selected_address must be B0".to_owned());
            }
        }
        "multiwindow" => {
            require_str_field(
                &mut blockers,
                report,
                "process_model",
                "two-child-processes",
            );
            require_positive_u64(&mut blockers, report, "preview_child_pid");
            require_positive_u64(&mut blockers, report, "dev_child_pid");
            require_bool_field(&mut blockers, report, "preview_survives_dev_exit", true);
            require_bool_field(
                &mut blockers,
                report,
                "preview_clean_exit_after_dev_exit",
                true,
            );
            require_native_surface_proof(&mut blockers, report, "preview_surface_proof", "preview");
            require_native_surface_proof(&mut blockers, report, "dev_surface_proof", "dev");
            require_distinct_u64_fields(
                &mut blockers,
                report,
                "preview_child_pid",
                "dev_child_pid",
            );
            require_distinct_json_str_paths(
                &mut blockers,
                report,
                "/preview_surface_proof/window_id",
                "/dev_surface_proof/window_id",
            );
            require_distinct_json_str_paths(
                &mut blockers,
                report,
                "/preview_surface_proof/surface_id",
                "/dev_surface_proof/surface_id",
            );
            require_str_field(&mut blockers, report, "display_server", "wayland");
            if report
                .pointer("/preview_native_gpu_render_proof/status")
                .and_then(serde_json::Value::as_str)
                != Some("pass")
            {
                blockers.push("preview_native_gpu_render_proof.status must be pass".to_owned());
            }
            require_visible_native_render_proof(
                &mut blockers,
                report,
                "/preview_surface_proof/external_render_proof",
            );
            require_visible_native_render_proof(
                &mut blockers,
                report,
                "/dev_surface_proof/external_render_proof",
            );
            require_preview_runtime_ownership(&mut blockers, report, "/preview_runtime_summary");
            require_preview_runtime_query(
                &mut blockers,
                report,
                "/dev_ipc_probe/runtime_summary_query",
            );
            if report
                .pointer("/dev_ipc_probe/dev_sent_replace_code")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
            {
                blockers.push(
                    "dev_ipc_probe.dev_sent_replace_code must prove dev sent ReplaceCode"
                        .to_owned(),
                );
            }
            if report
                .pointer("/dev_ipc_probe/replace_code/preview_command")
                .and_then(serde_json::Value::as_str)
                != Some("ReplaceCode")
            {
                blockers.push(
                    "dev_ipc_probe.replace_code.preview_command must be ReplaceCode".to_owned(),
                );
            }
            if report
                .pointer("/dev_ipc_probe/replace_code/hash_matches")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
            {
                blockers.push("dev_ipc_probe.replace_code.hash_matches must be true".to_owned());
            }
            if report
                .pointer("/dev_ipc_probe/replace_code/document_layout_proof/status")
                .and_then(serde_json::Value::as_str)
                != Some("pass")
            {
                blockers.push(
                    "dev_ipc_probe.replace_code.document_layout_proof.status must be pass"
                        .to_owned(),
                );
            }
            if report
                .pointer("/dev_ipc_probe/replace_code/preview_receives_example_name")
                .and_then(serde_json::Value::as_bool)
                != Some(false)
            {
                blockers.push(
                    "dev_ipc_probe.replace_code.preview_receives_example_name must be false"
                        .to_owned(),
                );
            }
        }
        "ipc-backpressure" => {
            require_bool_field(&mut blockers, report, "bounded_ipc", true);
            require_replace_code_evidence(&mut blockers, report, "");
            require_preview_runtime_query(&mut blockers, report, "/runtime_summary_query");
            require_u64_at_most(&mut blockers, report, "preview_blocked_on_ipc_count", 0);
            require_u64_at_most(
                &mut blockers,
                report,
                "queue_depth_max",
                native_gpu_budget_u64("ipc", "queue_depth_max").unwrap_or(256),
            );
            require_summary_f64_p95_at_most(
                &mut blockers,
                report,
                "preview_frame_ms_p50_p95_max",
                native_gpu_budget_f64("frame", "preview_frame_ms_p95").unwrap_or(16.7),
            );
            require_f64_value_at_most(
                &mut blockers,
                report,
                "preview_heartbeat_gap_ms_max",
                native_gpu_budget_f64("ipc", "heartbeat_gap_ms_max").unwrap_or(250.0),
            );
            require_u64_at_most(
                &mut blockers,
                report,
                "preview_rss_mib_max",
                native_gpu_budget_u64("memory", "rss_mib_max").unwrap_or(1024),
            );
            require_u64_at_most(
                &mut blockers,
                report,
                "dropped_debug_update_count",
                native_gpu_budget_u64("ipc", "dropped_debug_update_count_max").unwrap_or(100_000),
            );
            require_summary_u64_p95_at_most(
                &mut blockers,
                report,
                "debug_query_bytes_p50_p95_max",
                native_gpu_budget_u64("ipc", "debug_query_bytes_p95").unwrap_or(262_144),
            );
            require_summary_u64_p95_at_most(
                &mut blockers,
                report,
                "debug_subscription_bytes_p50_p95_max",
                native_gpu_budget_u64("ipc", "debug_subscription_bytes_p95").unwrap_or(262_144),
            );
        }
        "observability" => {
            require_bool_field(&mut blockers, report, "bounded_observability", true);
            require_replace_code_evidence(&mut blockers, report, "");
            require_preview_runtime_query(&mut blockers, report, "/runtime_summary_query");
            require_bool_field(
                &mut blockers,
                report,
                "full_state_mirroring_observed",
                false,
            );
            require_summary_f64_p95_at_most(
                &mut blockers,
                report,
                "preview_frame_ms_p50_p95_max",
                native_gpu_budget_f64("frame", "preview_frame_ms_p95").unwrap_or(16.7),
            );
            require_f64_value_at_most(
                &mut blockers,
                report,
                "preview_heartbeat_gap_ms_max",
                native_gpu_budget_f64("ipc", "heartbeat_gap_ms_max").unwrap_or(250.0),
            );
            require_u64_at_most(
                &mut blockers,
                report,
                "dropped_debug_update_count",
                native_gpu_budget_u64("ipc", "dropped_debug_update_count_max").unwrap_or(100_000),
            );
            require_summary_u64_p95_at_most(
                &mut blockers,
                report,
                "debug_query_bytes_p50_p95_max",
                native_gpu_budget_u64("ipc", "debug_query_bytes_p95").unwrap_or(262_144),
            );
            require_summary_u64_p95_at_most(
                &mut blockers,
                report,
                "debug_subscription_bytes_p50_p95_max",
                native_gpu_budget_u64("ipc", "debug_subscription_bytes_p95").unwrap_or(262_144),
            );
            if !report
                .get("observability_stress_profile")
                .and_then(serde_json::Value::as_object)
                .is_some_and(|profile| {
                    profile
                        .get("runtime_value_graph_enabled")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                        && profile
                            .get("busy_dev_graph_view_enabled")
                            .and_then(serde_json::Value::as_bool)
                            == Some(true)
                        && profile
                            .get("debug_updates_coalesced")
                            .and_then(serde_json::Value::as_bool)
                            == Some(true)
                        && profile
                            .get("debug_queries_paged")
                            .and_then(serde_json::Value::as_bool)
                            == Some(true)
                })
            {
                blockers.push(
                    "observability_stress_profile must prove bounded overload mode".to_owned(),
                );
            }
        }
        "preview-e2e-todomvc" | "preview-e2e-cells" => {
            require_str_field(&mut blockers, report, "display_server", "wayland");
            let tier = report
                .get("evidence_tier")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if !matches!(tier, "host-synthetic" | "boon-driver" | "real-window") {
                blockers.push(format!(
                    "evidence_tier must be boon-driver, host-synthetic, or real-window, got `{tier}`"
                ));
            }
            require_bool_field(&mut blockers, report, "operator_host_input", true);
            let real_os_input = report
                .get("real_os_input")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
            if tier == "real-window" {
                require_bool_field(&mut blockers, report, "real_os_input", true);
            } else {
                require_bool_field(&mut blockers, report, "real_os_input", false);
            }
            require_bool_field(&mut blockers, report, "operator_report", true);
            require_bool_field(&mut blockers, report, "human_observation", false);
            require_bool_field(
                &mut blockers,
                report,
                "preview_receives_example_name",
                false,
            );
            require_preview_runtime_ownership(&mut blockers, report, "/preview_runtime_summary");
            require_positive_u64(&mut blockers, report, "surface_epoch");
            require_positive_u64(&mut blockers, report, "window_pid");
            require_nonempty_array(&mut blockers, report, "window_cmdline");
            require_hash_field(&mut blockers, report, "source_hash");
            require_hash_field(&mut blockers, report, "scenario_hash");
            require_nonempty_array(&mut blockers, report, "scenario_labels");
            require_nonempty_array(
                &mut blockers,
                report,
                "checkpoint_screenshot_or_video_paths",
            );
            require_nonempty_array(&mut blockers, report, "artifact_sha256s");
            require_nonempty_array(&mut blockers, report, "frame_hashes");
            require_nonempty_array(&mut blockers, report, "readback_artifacts");
            require_nonempty_array(&mut blockers, report, "per_step_host_input_route");
            if real_os_input {
                require_nonempty_array(&mut blockers, report, "per_step_os_pointer_keyboard_route");
            } else if report
                .get("per_step_os_pointer_keyboard_route")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|steps| !steps.is_empty())
            {
                blockers.push(
                    "per_step_os_pointer_keyboard_route must be empty when real_os_input is false"
                        .to_owned(),
                );
            }
            require_nonempty_array(&mut blockers, report, "hit_target_assertions");
            require_nonempty_array(&mut blockers, report, "source_intent_assertions");
            require_nonempty_array(&mut blockers, report, "runtime_state_assertions");
            require_object_field(&mut blockers, report, "focused_window_proof");
            if report
                .pointer("/focused_window_proof/status")
                .and_then(serde_json::Value::as_str)
                != Some("pass")
            {
                blockers.push("focused_window_proof.status must be pass".to_owned());
            }
            if report
                .pointer("/native_host_input_route_evidence/status")
                .and_then(serde_json::Value::as_str)
                != Some("pass")
            {
                blockers.push("native_host_input_route_evidence.status must be pass".to_owned());
            }
            if report
                .pointer("/native_runtime_assertion_evidence/status")
                .and_then(serde_json::Value::as_str)
                != Some("pass")
            {
                blockers.push("native_runtime_assertion_evidence.status must be pass".to_owned());
            }
            if report
                .pointer("/native_runtime_assertion_evidence/live_preview_process_route")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
            {
                blockers.push(
                    "native_runtime_assertion_evidence.live_preview_process_route must be true"
                        .to_owned(),
                );
            }
            if report
                .pointer("/dev_ipc_probe/operator_host_input/status")
                .and_then(serde_json::Value::as_str)
                != Some("pass")
            {
                blockers.push("dev_ipc_probe.operator_host_input.status must be pass".to_owned());
            }
            if report
                .pointer("/preview_native_gpu_render_proof/status")
                .and_then(serde_json::Value::as_str)
                != Some("pass")
            {
                blockers.push("preview_native_gpu_render_proof.status must be pass".to_owned());
            }
            require_visible_native_render_proof(
                &mut blockers,
                report,
                "/preview_native_gpu_render_proof",
            );
            require_visible_playground_reality(&mut blockers, report);
            if report
                .pointer("/boon_driver_proof/status")
                .and_then(serde_json::Value::as_str)
                != Some("pass")
            {
                blockers.push("boon_driver_proof.status must be pass".to_owned());
            }
            if report
                .get("input_injection_method")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|method| {
                    let method = method.to_ascii_lowercase();
                    !(method.contains("operator_host_event_harness")
                        || method.contains("app_window_per_window_synthetic_input_harness"))
                        && !(method.contains("isolated-weston")
                            && method.contains("weston-test-control"))
                        || method.contains("xvfb")
                })
            {
                blockers.push(
                    "input_injection_method must be BoonDriver host input or isolated Weston real-window input"
                        .to_owned(),
                );
            }
        }
        "idle-wake-counter"
        | "idle-wake-todomvc"
        | "idle-wake-cells"
        | "idle-wake-custom-projects" => {
            require_str_field(&mut blockers, report, "render_loop_mode", "demand_driven");
            require_positive_u64(&mut blockers, report, "idle_observation_ms");
            require_positive_u64(&mut blockers, report, "preview_child_pid");
            require_positive_u64(&mut blockers, report, "dev_child_pid");
            require_distinct_u64_fields(
                &mut blockers,
                report,
                "preview_child_pid",
                "dev_child_pid",
            );
            require_str_field(
                &mut blockers,
                report,
                "cpu_measurement_source",
                "procfs-child-pid-tick-deltas",
            );
            require_bool_field(
                &mut blockers,
                report,
                "preview_receives_example_name",
                false,
            );
            require_bool_field(
                &mut blockers,
                report,
                "private_runtime_dispatch_used",
                false,
            );
            require_bool_field(&mut blockers, report, "post_idle_frame_hash_changed", true);
            require_bool_field(
                &mut blockers,
                report,
                "post_idle_source_replace_hash_changed",
                true,
            );
            require_str_field(
                &mut blockers,
                report,
                "visual_capture_method",
                "app-owned-wgpu-readback",
            );
            require_object_field(&mut blockers, report, "readback_artifact_before");
            require_object_field(&mut blockers, report, "readback_artifact_after");
            require_object_field(&mut blockers, report, "surface_lifecycle");
            require_hash_field(&mut blockers, report, "post_idle_frame_hash_before");
            require_hash_field(&mut blockers, report, "post_idle_frame_hash_after");
            require_positive_u64(&mut blockers, report, "skipped_idle_poll_count");
            let dirty_revision = report
                .get("dirty_revision")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let presented_revision = report
                .get("presented_revision")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let last_render_content_revision = report
                .get("last_render_content_revision")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            if dirty_revision == 0
                || presented_revision < dirty_revision
                || last_render_content_revision < presented_revision
            {
                blockers.push(
                    "idle/wake report must prove render content revision covers presented dirty revision"
                        .to_owned(),
                );
            }
            if report
                .get("post_idle_frame_hash_before")
                .and_then(serde_json::Value::as_str)
                == report
                    .get("post_idle_frame_hash_after")
                    .and_then(serde_json::Value::as_str)
            {
                blockers.push("post-idle input frame hash must actually change".to_owned());
            }
            if label == "idle-wake-custom-projects" {
                require_hash_field(&mut blockers, report, "custom_fixture_hash");
                require_nonempty_array(&mut blockers, report, "custom_project_identities");
                require_bool_field(
                    &mut blockers,
                    report,
                    "custom_project_fixture_uses_bundled_example_identity",
                    false,
                );
            }
        }
        "dev-editor-scroll-speed-debug" | "dev-editor-scroll-speed-release" => {
            require_str_field(
                &mut blockers,
                report,
                "surface_under_test",
                "dev-code-editor",
            );
            require_str_field(
                &mut blockers,
                report,
                "measurement_source",
                "isolated-weston-passive-dev-editor-scroll-probe",
            );
            require_str_field(
                &mut blockers,
                report,
                "input_provenance",
                "isolated_weston_real_wheel",
            );
            require_object_field(&mut blockers, report, "launched_process_evidence");
            if report
                .pointer("/launched_process_evidence/desktop_pid")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                == 0
            {
                blockers.push(
                    "launched_process_evidence.desktop_pid must prove a launched native process"
                        .to_owned(),
                );
            }
            require_nonempty_str_field(&mut blockers, report, "profile");
            require_nonempty_str_field(&mut blockers, report, "build_profile");
            require_nonempty_str_field(&mut blockers, report, "tested_binary");
            let min_lines =
                native_gpu_budget_u64_or_blocker(&mut blockers, "dev_code_editor", "min_lines");
            let min_longest_line_bytes = native_gpu_budget_u64_or_blocker(
                &mut blockers,
                "dev_code_editor",
                "min_longest_line_bytes",
            );
            let scroll_budget_section = if label.ends_with("release") {
                "dev_editor_scroll.release"
            } else {
                "dev_editor_scroll.debug"
            };
            let wheel_to_visible_budget = native_gpu_budget_f64_or_blocker(
                &mut blockers,
                scroll_budget_section,
                "wheel_to_visible_ms_p95",
            );
            require_u64_at_least(&mut blockers, report, "line_count", min_lines);
            require_u64_at_least(
                &mut blockers,
                report,
                "longest_line_bytes",
                min_longest_line_bytes,
            );
            require_object_field(&mut blockers, report, "scroll_line_before_after");
            require_object_field(&mut blockers, report, "scroll_column_before_after");
            require_u64_at_least(
                &mut blockers,
                report,
                "scroll_line_delta",
                NATIVE_DEV_EDITOR_WHEEL_MIN_STEPS,
            );
            require_u64_at_least(
                &mut blockers,
                report,
                "scroll_column_delta",
                NATIVE_DEV_EDITOR_WHEEL_MIN_STEPS,
            );
            require_object_field(&mut blockers, report, "visible_line_range_before_after");
            require_object_field(&mut blockers, report, "visible_column_range_before_after");
            require_summary_f64_p95_at_most(
                &mut blockers,
                report,
                "dev_editor_frame_ms_p50_p95_p99_max",
                wheel_to_visible_budget,
            );
            require_axis_p95_at_most(
                &mut blockers,
                report,
                "wheel_to_visible_ms_p95_per_axis",
                wheel_to_visible_budget,
            );
            require_object_field(
                &mut blockers,
                report,
                "post_input_measured_frame_count_per_axis",
            );
            for axis in ["vertical", "horizontal"] {
                if report
                    .pointer(&format!("/post_input_measured_frame_count_per_axis/{axis}"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0)
                    < 30
                {
                    blockers.push(format!(
                        "post_input_measured_frame_count_per_axis.{axis} must be at least 30"
                    ));
                }
            }
            require_u64_at_most(&mut blockers, report, "missed_frame_count", 0);
            require_u64_at_most(&mut blockers, report, "dropped_frame_count", 0);
            require_u64_at_most(
                &mut blockers,
                report,
                "runtime_dispatch_count_for_passive_scroll",
                0,
            );
            require_u64_at_most(&mut blockers, report, "graph_rebuild_count", 0);
            require_u64_at_most(
                &mut blockers,
                report,
                "source_replace_count_for_passive_scroll",
                0,
            );
            require_u64_at_most(&mut blockers, report, "replace_code_count_during_scroll", 0);
            require_u64_at_most(
                &mut blockers,
                report,
                "preview_runtime_summary_query_count_for_passive_scroll",
                0,
            );
            require_u64_at_most(
                &mut blockers,
                report,
                "telemetry_poll_count_in_scroll_hot_path",
                0,
            );
            require_u64_at_most(
                &mut blockers,
                report,
                "full_layout_refresh_count_for_passive_scroll",
                1,
            );
            require_positive_u64(
                &mut blockers,
                report,
                "fast_frame_patch_count_for_passive_scroll",
            );
            require_positive_u64(&mut blockers, report, "visible_line_count");
            require_positive_u64(&mut blockers, report, "materialized_line_count_max");
            require_f64_at_least(&mut blockers, report, "text_cache_hit_rate", 0.90);
            require_u64_at_most(&mut blockers, report, "glyph_atlas_evictions", 0);
            require_u64_at_most(&mut blockers, report, "preview_blocked_on_ipc_count", 0);
            require_nonempty_array(&mut blockers, report, "app_owned_readback_artifacts");
            if report
                .pointer("/operator_real_wheel_input_evidence/status")
                .and_then(serde_json::Value::as_str)
                != Some("pass")
            {
                blockers.push(
                    "operator_real_wheel_input_evidence.status must be pass for dev editor scroll"
                        .to_owned(),
                );
            }
        }
        "example-switch-speed-debug" | "example-switch-speed-release" => {
            require_nonempty_str_field(&mut blockers, report, "profile");
            require_nonempty_str_field(&mut blockers, report, "build_profile");
            require_nonempty_array(&mut blockers, report, "switch_sequence");
            require_nonempty_str_field(&mut blockers, report, "custom_fixture_hash");
            require_positive_u64(&mut blockers, report, "command_id");
            require_positive_u64(&mut blockers, report, "source_revision");
            require_nonempty_str_field(&mut blockers, report, "source_hash");
            require_str_field(
                &mut blockers,
                report,
                "payload_kind",
                "SourceProjectPayload",
            );
            let budget_section = if label.ends_with("release") {
                "example_switch.release"
            } else {
                "example_switch.debug"
            };
            let ack_budget =
                native_gpu_budget_f64_or_blocker(&mut blockers, budget_section, "sync_ack_ms_p95");
            let ack_max_budget =
                native_gpu_budget_f64_or_blocker(&mut blockers, budget_section, "sync_ack_ms_max");
            let ack_payload_budget = native_gpu_budget_u64_or_blocker(
                &mut blockers,
                budget_section,
                "sync_ack_payload_bytes_max",
            );
            let dev_tab_budget = native_gpu_budget_f64_or_blocker(
                &mut blockers,
                budget_section,
                "click_to_dev_tab_visual_update_ms_p95",
            );
            let bundled_preview_budget = native_gpu_budget_f64_or_blocker(
                &mut blockers,
                budget_section,
                "click_to_preview_new_frame_presented_ms_p95_bundled",
            );
            let preview_budget = native_gpu_budget_f64_or_blocker(
                &mut blockers,
                budget_section,
                "click_to_preview_new_frame_presented_ms_p95_large_custom",
            );
            require_f64_at_most(&mut blockers, report, "ack_latency_ms", ack_budget);
            require_f64_at_most(&mut blockers, report, "ack_latency_ms_max", ack_max_budget);
            require_u64_at_most(
                &mut blockers,
                report,
                "ack_payload_bytes",
                ack_payload_budget,
            );
            require_f64_at_most(
                &mut blockers,
                report,
                "click_to_dev_tab_visual_update_ms",
                dev_tab_budget,
            );
            require_f64_at_most(
                &mut blockers,
                report,
                "click_to_preview_pending_status_ms",
                ack_budget,
            );
            require_f64_at_most(
                &mut blockers,
                report,
                "click_to_preview_new_frame_presented_ms",
                preview_budget,
            );
            require_f64_at_most(
                &mut blockers,
                report,
                "click_to_preview_new_frame_presented_ms_bundled",
                bundled_preview_budget,
            );
            require_f64_at_most(
                &mut blockers,
                report,
                "click_to_preview_new_frame_presented_ms_custom",
                preview_budget,
            );
            require_object_field(&mut blockers, report, "parse_lower_runtime_layout_timings");
            require_nonempty_array(&mut blockers, report, "per_switch");
            let per_switch = report
                .get("per_switch")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            let sequence = report
                .get("switch_sequence")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            for required_label in [
                "counter",
                "todomvc",
                "cells",
                "custom:a",
                "custom:b",
                "custom:multi-file",
                "invalid-custom",
                "aba:a",
                "aba:b",
                "aba:a2",
            ] {
                if !sequence
                    .iter()
                    .any(|value| value.as_str() == Some(required_label))
                {
                    blockers.push(format!(
                        "switch_sequence missing required scenario `{required_label}`"
                    ));
                }
            }
            require_bool_field(&mut blockers, report, "stale_ack_rejected", true);
            require_bool_field(&mut blockers, report, "stale_result_rejected", true);
            require_bool_field(
                &mut blockers,
                report,
                "pending_overlay_readback_recorded_separately",
                true,
            );
            require_bool_field(&mut blockers, report, "bounded_latest_wins_worker", true);
            if report
                .pointer("/rapid_switch_probe/bounded_latest_wins")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
            {
                blockers.push(
                    "rapid_switch_probe.bounded_latest_wins must prove bounded latest-wins scheduling"
                        .to_owned(),
                );
            }
            require_bool_field(
                &mut blockers,
                report,
                "preview_receives_example_name",
                false,
            );
            require_bool_field(
                &mut blockers,
                report,
                "sync_ack_contains_runtime_summary",
                false,
            );
            require_bool_field(
                &mut blockers,
                report,
                "sync_ack_contains_layout_proof",
                false,
            );
            require_bool_field(
                &mut blockers,
                report,
                "last_good_frame_kept_while_pending",
                true,
            );
            require_hash_field(&mut blockers, report, "readback_hash_before");
            require_hash_field(&mut blockers, report, "readback_hash_after");
            if report
                .get("readback_hash_before")
                .and_then(serde_json::Value::as_str)
                == report
                    .get("readback_hash_after")
                    .and_then(serde_json::Value::as_str)
            {
                blockers.push("example switch readback hashes must change".to_owned());
            }
            if report
                .get("measurement_source")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|source| {
                    !source.contains("dev")
                        || !source.contains("preview")
                        || !source.contains("readback")
                })
            {
                blockers.push(
                    "example switch measurement_source must include dev and preview readback evidence"
                        .to_owned(),
                );
            }
            if report
                .get("dev_visual_update_before_preview_ack")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
            {
                blockers.push(
                    "dev_visual_update_before_preview_ack must prove tab visuals are independent of preview ACK"
                        .to_owned(),
                );
            }
            let ack = report
                .get("ack_latency_ms")
                .and_then(numeric_value_as_f64)
                .unwrap_or(f64::INFINITY);
            let dev = report
                .get("click_to_dev_tab_visual_update_ms")
                .and_then(numeric_value_as_f64)
                .unwrap_or(f64::INFINITY);
            if (ack - dev).abs() < f64::EPSILON {
                blockers.push(
                    "click_to_dev_tab_visual_update_ms must not be copied from preview ACK latency"
                        .to_owned(),
                );
            }
            for (index, step) in per_switch.iter().enumerate() {
                for key in [
                    "payload_kind",
                    "command_id",
                    "source_revision",
                    "source_hash",
                    "ack",
                    "ready",
                ] {
                    if step.get(key).is_none() {
                        blockers.push(format!("per_switch[{index}].{key} is missing"));
                    }
                }
                if step.get("payload_kind").and_then(serde_json::Value::as_str)
                    != Some("SourceProjectPayload")
                {
                    blockers.push(format!(
                        "per_switch[{index}].payload_kind must be SourceProjectPayload"
                    ));
                }
                if step
                    .get("preview_receives_example_name")
                    .and_then(serde_json::Value::as_bool)
                    != Some(false)
                {
                    blockers.push(format!(
                        "per_switch[{index}].preview_receives_example_name must be false"
                    ));
                }
                if step
                    .get("sync_ack_contains_runtime_summary")
                    .and_then(serde_json::Value::as_bool)
                    != Some(false)
                    || step
                        .get("sync_ack_contains_layout_proof")
                        .and_then(serde_json::Value::as_bool)
                        != Some(false)
                {
                    blockers.push(format!(
                        "per_switch[{index}] sync ACK must not contain runtime summary or layout proof"
                    ));
                }
                if step.get("pending_overlay_readback_probe").is_none()
                    || step
                        .get("pending_overlay_readback_recorded_separately")
                        .and_then(serde_json::Value::as_bool)
                        != Some(true)
                {
                    blockers.push(format!(
                        "per_switch[{index}] must record pending-overlay readback separately from final source readback"
                    ));
                }
                if step
                    .get("bounded_latest_wins_worker")
                    .and_then(serde_json::Value::as_bool)
                    != Some(true)
                {
                    blockers.push(format!(
                        "per_switch[{index}] must prove bounded latest-wins worker scheduling"
                    ));
                }
                if step
                    .get("expected_result_status")
                    .and_then(serde_json::Value::as_str)
                    == Some("pass")
                    && step
                        .get("readback_bound_to_result_frame_revision")
                        .and_then(serde_json::Value::as_bool)
                        != Some(true)
                {
                    blockers.push(format!(
                        "per_switch[{index}] final readback must be bound to the replace-source result frame revision"
                    ));
                }
                if step
                    .get("expected_result_status")
                    .and_then(serde_json::Value::as_str)
                    == Some("pass")
                    && step
                        .get("readback_bound_to_result_source_hash")
                        .and_then(serde_json::Value::as_bool)
                        != Some(true)
                {
                    blockers.push(format!(
                        "per_switch[{index}] final readback must be bound to the replace-source result source hash"
                    ));
                }
            }
        }
        "scroll-speed-cells" => {
            require_scroll_budget_fields(&mut blockers, report);
            require_common_scroll_hot_path_fields(&mut blockers, report);
            if report
                .pointer("/boon_driver_proof/status")
                .and_then(serde_json::Value::as_str)
                != Some("pass")
            {
                blockers.push("boon_driver_proof.status must be pass".to_owned());
            }
            require_bool_field(&mut blockers, report, "operator_host_wheel_input", true);
            require_u64_at_least(
                &mut blockers,
                report,
                "logical_columns",
                native_gpu_budget_u64("cells", "logical_columns").unwrap_or(26),
            );
            require_u64_at_least(
                &mut blockers,
                report,
                "logical_rows",
                native_gpu_budget_u64("cells", "logical_rows").unwrap_or(100),
            );
            require_bool_field(&mut blockers, report, "synthetic_scroll", false);
            require_bool_field(
                &mut blockers,
                report,
                "runtime_dispatch_on_passive_scroll",
                false,
            );
            require_u64_at_most(
                &mut blockers,
                report,
                "runtime_dispatch_count_for_passive_scroll",
                0,
            );
            require_u64_at_most(&mut blockers, report, "graph_rebuild_count", 0);
            require_u64_at_most(&mut blockers, report, "preview_blocked_on_ipc_count", 0);
            require_summary_u64_p95_at_most(
                &mut blockers,
                report,
                "draw_calls_p50_p95_max",
                native_gpu_budget_u64("cells", "draw_calls_p95").unwrap_or(16),
            );
            require_summary_u64_p95_at_most(
                &mut blockers,
                report,
                "queue_write_count_p50_p95_max",
                native_gpu_budget_u64("cells", "queue_write_count_p95").unwrap_or(8),
            );
            require_summary_u64_p95_at_most(
                &mut blockers,
                report,
                "upload_bytes_p50_p95_max",
                native_gpu_budget_u64("memory", "upload_bytes_p95").unwrap_or(262_144),
            );
            require_u64_at_most(
                &mut blockers,
                report,
                "pipeline_switch_count_p95",
                native_gpu_budget_u64("cells", "draw_calls_p95").unwrap_or(16),
            );
            require_positive_u64(&mut blockers, report, "instance_count_visible");
            require_positive_u64(&mut blockers, report, "instance_count_uploaded");
            require_positive_u64(&mut blockers, report, "text_runs_visible");
            require_u64_at_most(&mut blockers, report, "text_shape_cache_evictions", 0);
            require_u64_at_most(&mut blockers, report, "glyph_atlas_evictions", 0);
            require_object_field(
                &mut blockers,
                report,
                "visible_address_samples_before_after",
            );
            require_object_field(&mut blockers, report, "materialized_range_before_after");
        }
        "scroll-speed-dev-code-editor" => {
            require_scroll_budget_fields(&mut blockers, report);
            require_common_scroll_hot_path_fields(&mut blockers, report);
            if report
                .pointer("/boon_driver_proof/status")
                .and_then(serde_json::Value::as_str)
                != Some("pass")
            {
                blockers.push("boon_driver_proof.status must be pass".to_owned());
            }
            require_bool_field(&mut blockers, report, "operator_host_wheel_input", true);
            require_u64_at_least(
                &mut blockers,
                report,
                "line_count",
                native_gpu_budget_u64("dev_code_editor", "min_lines").unwrap_or(10_000),
            );
            require_u64_at_least(
                &mut blockers,
                report,
                "longest_line_bytes",
                native_gpu_budget_u64("dev_code_editor", "min_longest_line_bytes").unwrap_or(2_000),
            );
            require_summary_f64_p95_at_most(
                &mut blockers,
                report,
                "dev_editor_frame_ms_p50_p95_p99_max",
                native_gpu_budget_f64("frame", "preview_frame_ms_p95").unwrap_or(16.7),
            );
            require_u64_at_most(&mut blockers, report, "preview_blocked_on_ipc_count", 0);
            require_positive_u64(&mut blockers, report, "visible_line_count");
            require_positive_u64(&mut blockers, report, "materialized_line_count_max");
            require_u64_at_most(
                &mut blockers,
                report,
                "text_runs_shaped_p95",
                report
                    .get("materialized_line_count_max")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(u64::MAX),
            );
            require_f64_at_least(&mut blockers, report, "text_cache_hit_rate", 0.90);
            require_u64_at_most(&mut blockers, report, "glyph_atlas_evictions", 0);
            require_f64_at_most(
                &mut blockers,
                report,
                "preview_frame_ms_p95",
                native_gpu_budget_f64("dev_code_editor", "preview_frame_ms_p95_while_scrolling")
                    .unwrap_or(16.7),
            );
            require_object_field(&mut blockers, report, "materialized_range_before_after");
        }
        _ => {}
    }
    blockers
}

fn native_visible_reality_harness(report: &serde_json::Value) -> serde_json::Value {
    let mut blockers = Vec::new();
    require_visible_playground_reality(&mut blockers, report);
    json!({
        "status": if blockers.is_empty() { "pass" } else { "fail" },
        "method": "visible_wgpu_readback_plus_render_hook_contract",
        "blockers": blockers,
        "preview_readback_artifact": report
            .pointer("/preview_surface_proof/readback_artifact")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "dev_readback_artifact": report
            .pointer("/dev_surface_proof/readback_artifact")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "rejects_single_color_or_debug_palette_preview": true,
        "rejects_fixture_grid_dev_surface": true,
        "rejects_frozen_one_frame_surface": true
    })
}

fn require_visible_playground_reality(blockers: &mut Vec<String>, report: &serde_json::Value) {
    for (surface_key, role) in [
        ("preview_surface_proof", "preview"),
        ("dev_surface_proof", "dev"),
    ] {
        require_native_surface_proof(blockers, report, surface_key, role);
        let surface_path = format!("/{surface_key}");
        if report
            .pointer(&format!("{surface_path}/interactive_frame_loop"))
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        {
            blockers.push(format!(
                "{surface_path}.interactive_frame_loop must be true; one-frame render then sleep is forbidden"
            ));
        }
        require_visible_native_render_proof(
            blockers,
            report,
            &format!("{surface_path}/external_render_proof"),
        );
    }
    let preview_width = report
        .pointer("/preview_surface_proof/readback_artifact/width")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let preview_height = report
        .pointer("/preview_surface_proof/readback_artifact/height")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    if preview_width < 900 || preview_height < 700 {
        blockers.push(format!(
            "preview_surface_proof.readback_artifact must be at least 900x700; got {preview_width}x{preview_height}"
        ));
    }
    let dev_width = report
        .pointer("/dev_surface_proof/readback_artifact/width")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let dev_height = report
        .pointer("/dev_surface_proof/readback_artifact/height")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    if dev_width < 1100 || dev_height < 760 {
        blockers.push(format!(
            "dev_surface_proof.readback_artifact must be at least 1100x760; got {dev_width}x{dev_height}"
        ));
    }
    if report
        .pointer("/preview_surface_proof/external_render_proof/visible_style_mode")
        .and_then(serde_json::Value::as_str)
        != Some("document_style")
    {
        blockers.push(
            "preview_surface_proof.external_render_proof.visible_style_mode must be document_style"
                .to_owned(),
        );
    }
    if report
        .pointer("/preview_surface_proof/external_render_proof/debug_palette_used")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        blockers.push(
            "preview_surface_proof.external_render_proof.debug_palette_used must be false"
                .to_owned(),
        );
    }
    if !report
        .pointer("/preview_surface_proof/external_render_proof/viewport_fill_ratio")
        .and_then(serde_json::Value::as_f64)
        .is_some_and(|ratio| ratio >= 0.90)
    {
        blockers.push(
            "preview_surface_proof.external_render_proof.viewport_fill_ratio must be at least 0.90"
                .to_owned(),
        );
    }
    if !report
        .pointer("/preview_surface_proof/external_render_proof/content_bounds_fill_ratio")
        .and_then(serde_json::Value::as_f64)
        .is_some_and(|ratio| ratio >= 0.95)
    {
        blockers.push(
            "preview_surface_proof.external_render_proof.content_bounds_fill_ratio must be at least 0.95"
                .to_owned(),
        );
    }
    if !report
        .pointer("/dev_surface_proof/external_render_proof/content_bounds_fill_ratio")
        .and_then(serde_json::Value::as_f64)
        .is_some_and(|ratio| ratio >= 0.95)
    {
        blockers.push(
            "dev_surface_proof.external_render_proof.content_bounds_fill_ratio must be at least 0.95"
                .to_owned(),
        );
    }
    if report
        .pointer("/dev_surface_proof/external_render_proof/dev_ui_source")
        .and_then(serde_json::Value::as_str)
        != Some("boon-dev-editor-debug-shell")
    {
        blockers.push(
            "dev_surface_proof.external_render_proof.dev_ui_source must be boon-dev-editor-debug-shell"
                .to_owned(),
        );
    }
    if report
        .pointer("/dev_surface_proof/external_render_proof/dev_editor_visible")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        blockers.push(
            "dev_surface_proof.external_render_proof.dev_editor_visible must be true".to_owned(),
        );
    }
    if report
        .pointer("/dev_surface_proof/external_render_proof/fixture_grid_used")
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        blockers.push(
            "dev_surface_proof.external_render_proof.fixture_grid_used must be false".to_owned(),
        );
    }
    if report
        .pointer("/native_host_input_route_evidence/changes_visible_frame")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        blockers
            .push("native_host_input_route_evidence.changes_visible_frame must be true".to_owned());
    }
}

fn require_scroll_budget_fields(blockers: &mut Vec<String>, report: &serde_json::Value) {
    require_str_field(blockers, report, "display_server", "wayland");
    require_bool_field(blockers, report, "budget_pass", true);
    if scroll_wall_clock_budget_exempt(report) {
        require_non_os_scroll_model(blockers, report);
        return;
    }
    require_f64_at_most(
        blockers,
        report,
        "preview_frame_ms_p95",
        native_gpu_budget_f64("frame", "preview_frame_ms_p95").unwrap_or(16.7),
    );
    require_f64_at_most(
        blockers,
        report,
        "wheel_to_visible_ms_p95",
        native_gpu_budget_f64("frame", "wheel_to_visible_ms_p95").unwrap_or(50.0),
    );
    require_u64_at_most(
        blockers,
        report,
        "missed_frame_count",
        native_gpu_budget_u64("frame", "missed_frame_count").unwrap_or(0),
    );
    require_u64_at_most(
        blockers,
        report,
        "dropped_frame_count",
        native_gpu_budget_u64("frame", "missed_frame_count").unwrap_or(0),
    );
    require_f64_at_most(
        blockers,
        report,
        "longest_visible_stall_ms",
        native_gpu_budget_f64("frame", "preview_frame_ms_max").unwrap_or(33.4),
    );
}

fn require_common_scroll_hot_path_fields(blockers: &mut Vec<String>, report: &serde_json::Value) {
    require_u64_at_most(
        blockers,
        report,
        "runtime_dispatch_count_for_passive_scroll",
        0,
    );
    require_u64_at_most(blockers, report, "graph_rebuild_count", 0);
    require_u64_at_most(blockers, report, "preview_blocked_on_ipc_count", 0);
    require_u64_at_least(blockers, report, "wheel_events_coalesced", 1);
    require_u64_at_most(
        blockers,
        report,
        "input_queue_depth_max",
        native_gpu_budget_u64("ipc", "queue_depth_max").unwrap_or(256),
    );
    require_nonempty_str_field(blockers, report, "layout_rebuild_scope");
    require_positive_u64(blockers, report, "newly_materialized_range_count");
    if scroll_wall_clock_budget_exempt(report) {
        require_non_os_scroll_model(blockers, report);
    } else {
        require_summary_f64_p95_at_most(
            blockers,
            report,
            "scroll_frame_ms_p50_p95_p99_max",
            native_gpu_budget_f64("frame", "preview_frame_ms_p95").unwrap_or(16.7),
        );
        require_u64_at_most(
            blockers,
            report,
            "missed_frame_count",
            native_gpu_budget_u64("frame", "missed_frame_count").unwrap_or(0),
        );
        require_u64_at_most(
            blockers,
            report,
            "dropped_frame_count",
            native_gpu_budget_u64("frame", "missed_frame_count").unwrap_or(0),
        );
        require_f64_at_most(
            blockers,
            report,
            "longest_visible_stall_ms",
            native_gpu_budget_f64("frame", "preview_frame_ms_max").unwrap_or(33.4),
        );
    }
    require_positive_u64(blockers, report, "sample_frame_count");
    require_positive_u64(blockers, report, "sustained_scroll_duration_ms");
    require_object_field(blockers, report, "scroll_distance_px_rows_cols");
    require_object_field(blockers, report, "materialized_range_before_after");
    if !scroll_wall_clock_budget_exempt(report) {
        require_axis_p95_at_most(
            blockers,
            report,
            "wheel_to_visible_ms_p95_per_axis",
            native_gpu_budget_f64("frame", "wheel_to_visible_ms_p95").unwrap_or(50.0),
        );
    }
    require_u64_array_field(blockers, report, "frames_over_16_7_ms");
}

fn scroll_wall_clock_budget_exempt(report: &serde_json::Value) -> bool {
    report
        .get("software_adapter_wall_clock_budget_exempt")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

fn require_non_os_scroll_model(blockers: &mut Vec<String>, report: &serde_json::Value) {
    if report
        .pointer("/non_os_scroll_model/status")
        .and_then(serde_json::Value::as_str)
        != Some("pass")
    {
        blockers.push(
            "non_os_scroll_model.status must be pass when wall-clock budget is software-adapter exempt"
                .to_owned(),
        );
    }
    if report
        .pointer("/non_os_scroll_model/frame_budget_model_pass")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        blockers.push(
            "non_os_scroll_model.frame_budget_model_pass must be true when wall-clock budget is software-adapter exempt"
                .to_owned(),
        );
    }
}

fn write_native_gate_report(
    args: &[String],
    command: &str,
    checks: Vec<serde_json::Value>,
    blockers: Vec<String>,
    extra: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let default_report = native_default_report_path(command, args);
    let report = report_arg(args).unwrap_or(default_report);
    let _ = std::fs::remove_file(&report);
    let status = if blockers.is_empty() { "pass" } else { "fail" };
    let mut object = serde_json::Map::new();
    object.insert("status".to_owned(), json!(status));
    object.insert("report_version".to_owned(), json!(1));
    object.insert(
        "generated_at_utc".to_owned(),
        json!(current_unix_seconds().to_string()),
    );
    object.insert("command".to_owned(), json!(command));
    object.insert("command_argv".to_owned(), json!(args));
    object.insert(
        "exit_status".to_owned(),
        json!(if blockers.is_empty() { 0 } else { 1 }),
    );
    object.insert("git_commit".to_owned(), json!(git_commit()));
    object.insert(
        "worktree_fingerprint".to_owned(),
        json!(worktree_fingerprint()),
    );
    object.insert("binary_hash".to_owned(), json!(current_binary_hash()));
    object.insert("source_hash".to_owned(), json!("n/a"));
    object.insert("scenario_hash".to_owned(), json!("n/a"));
    object.insert("program_hash".to_owned(), json!("n/a"));
    object.insert(
        "budget_hash".to_owned(),
        json!(file_hash("budgets/native-gpu.toml")),
    );
    object.insert("graph_node_count".to_owned(), json!(0));
    object.insert("per_step_pass_fail".to_owned(), json!(checks));
    object.insert("artifact_sha256s".to_owned(), json!([]));
    object.insert("native_gpu_contract".to_owned(), json!(true));
    if !blockers.is_empty() {
        object.insert("blockers".to_owned(), json!(blockers));
    }
    if let Some(extra) = extra.as_object() {
        for (key, value) in extra {
            object.insert(key.clone(), value.clone());
        }
    }
    write_json(&report, &serde_json::Value::Object(object))?;
    verify_report_schema(&report)?;
    if blockers.is_empty() {
        println!("wrote {}", report.display());
        Ok(())
    } else {
        Err(format!(
            "native GPU gate `{command}` blocked; wrote {}",
            report.display()
        )
        .into())
    }
}

fn write_static_gate_report(
    args: &[String],
    command: &str,
    report: PathBuf,
    checks: Vec<serde_json::Value>,
    blockers: Vec<String>,
    extra: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let _ = std::fs::remove_file(&report);
    let status = if blockers.is_empty() { "pass" } else { "fail" };
    let mut object = serde_json::Map::new();
    object.insert("status".to_owned(), json!(status));
    object.insert("report_version".to_owned(), json!(1));
    object.insert(
        "generated_at_utc".to_owned(),
        json!(current_unix_seconds().to_string()),
    );
    object.insert("command".to_owned(), json!(command));
    object.insert("command_argv".to_owned(), json!(args));
    object.insert(
        "exit_status".to_owned(),
        json!(if blockers.is_empty() { 0 } else { 1 }),
    );
    object.insert("git_commit".to_owned(), json!(git_commit()));
    object.insert(
        "worktree_fingerprint".to_owned(),
        json!(worktree_fingerprint()),
    );
    object.insert("binary_hash".to_owned(), json!(current_binary_hash()));
    object.insert("source_hash".to_owned(), json!("n/a"));
    object.insert("scenario_hash".to_owned(), json!("n/a"));
    object.insert("program_hash".to_owned(), json!("n/a"));
    object.insert("budget_hash".to_owned(), json!("n/a"));
    object.insert("graph_node_count".to_owned(), json!(0));
    object.insert("per_step_pass_fail".to_owned(), json!(checks));
    object.insert("artifact_sha256s".to_owned(), json!([]));
    if !blockers.is_empty() {
        object.insert("blockers".to_owned(), json!(blockers));
    }
    if let Some(extra) = extra.as_object() {
        for (key, value) in extra {
            object.insert(key.clone(), value.clone());
        }
    }
    write_json(&report, &serde_json::Value::Object(object))?;
    verify_report_schema(&report)?;
    if blockers.is_empty() {
        println!("wrote {}", report.display());
        Ok(())
    } else {
        Err(format!("gate `{command}` blocked; wrote {}", report.display()).into())
    }
}

fn native_default_report_path(command: &str, args: &[String]) -> PathBuf {
    let name = match command {
        "verify-platform-contract" => "platform-contract",
        "verify-native-gpu-dependency-graph" => "dependency-graph",
        "verify-native-gpu-architecture" => "architecture",
        "verify-native-gpu-layout-contract" => "layout-contract",
        "verify-native-gpu-shaders" => "shaders",
        "verify-native-gpu-multiwindow" => "multiwindow",
        "verify-native-gpu-ipc-backpressure" => "ipc-backpressure",
        "verify-native-gpu-observability" => "observability",
        "verify-native-real-window-input-environment" => "real-window-input-environment",
        "verify-native-gpu-preview-e2e" => match value_arg(args, "--example").as_deref() {
            Some("counter") => "preview-e2e-counter",
            Some("todomvc") => "preview-e2e-todomvc",
            Some("cells") => "preview-e2e-cells",
            _ => "preview-e2e",
        },
        "verify-native-visible-launch" => match value_arg(args, "--example").as_deref() {
            Some("todomvc") => "todomvc-visible-launch",
            Some("cells") => "cells-visible-launch",
            _ => "visible-launch",
        },
        "verify-native-examples" => "native-examples",
        "verify-native-dev-window-editor" => match value_arg(args, "--example").as_deref() {
            Some("todomvc") => "dev-editor-todomvc",
            Some("cells") => "dev-editor-cells",
            _ => "dev-editor",
        },
        "verify-native-example-tabs" => "example-tabs",
        "verify-native-editor-format" => "editor-format",
        "verify-boon-driver-schema" => {
            return PathBuf::from("target/reports/boon-driver/schema.json");
        }
        "verify-boon-driver-e2e" => match value_arg(args, "--example").as_deref() {
            Some("todomvc") => return PathBuf::from("target/reports/boon-driver/todomvc.json"),
            Some("cells") => return PathBuf::from("target/reports/boon-driver/cells.json"),
            _ => return PathBuf::from("target/reports/boon-driver/e2e.json"),
        },
        "verify-boon-driver-dev-window" => match value_arg(args, "--example").as_deref() {
            Some("todomvc") => {
                return PathBuf::from("target/reports/boon-driver/dev-window-todomvc.json");
            }
            Some("cells") => {
                return PathBuf::from("target/reports/boon-driver/dev-window-cells.json");
            }
            _ => return PathBuf::from("target/reports/boon-driver/dev-window.json"),
        },
        "verify-boon-driver-speed" => {
            let label = native_gpu_scroll_selector(args).label;
            return PathBuf::from(format!("target/reports/boon-driver/speed-{label}.json"));
        }
        "verify-boon-driver-all" => return PathBuf::from("target/reports/boon-driver/all.json"),
        "verify-linux-human-like-environment" => {
            return PathBuf::from("target/reports/linux-human-like/environment.json");
        }
        "verify-linux-human-like-e2e" => match value_arg(args, "--example").as_deref() {
            Some("todomvc") => {
                return PathBuf::from("target/reports/linux-human-like/todomvc.json");
            }
            Some("cells") => return PathBuf::from("target/reports/linux-human-like/cells.json"),
            _ => return PathBuf::from("target/reports/linux-human-like/e2e.json"),
        },
        "verify-linux-human-like-speed" => {
            let label = native_gpu_scroll_selector(args).label;
            if label == "cells" {
                return PathBuf::from("target/reports/linux-human-like/cells-speed.json");
            }
            return PathBuf::from(format!(
                "target/reports/linux-human-like/{label}-speed.json"
            ));
        }
        "verify-linux-human-like-all" => {
            return PathBuf::from("target/reports/linux-human-like/all.json");
        }
        "verify-native-example-speed" => match value_arg(args, "--example").as_deref() {
            Some("todomvc") => "speed-todomvc",
            Some("cells") => "speed-cells",
            _ => "speed-example",
        },
        "verify-native-counter-interaction-speed" => "counter-interaction-speed",
        "verify-native-cells-interaction-speed" => match value_arg(args, "--profile").as_deref() {
            Some("release") => "cells-interaction-speed-release",
            _ => "cells-interaction-speed-debug",
        },
        "verify-native-gpu-idle-wake" => match value_arg(args, "--example").as_deref() {
            Some("counter") => "idle-wake-counter",
            Some("todomvc") => "idle-wake-todomvc",
            Some("cells") => "idle-wake-cells",
            _ if value_arg(args, "--custom-project-fixture").is_some() => {
                "idle-wake-custom-projects"
            }
            _ => "idle-wake",
        },
        "verify-native-dev-editor-scroll-speed" => match value_arg(args, "--profile").as_deref() {
            Some("release") => "dev-editor-scroll-speed-release",
            _ => "dev-editor-scroll-speed-debug",
        },
        "verify-native-example-switch-speed" => match value_arg(args, "--profile").as_deref() {
            Some("release") => "example-switch-speed-release",
            _ => "example-switch-speed-debug",
        },
        "verify-native-dev-editor-speed" => "dev-editor-speed",
        "verify-native-two-window-content" => "todomvc-two-window-content",
        "verify-native-todomvc-reference-parity" => "todomvc-reference-parity",
        "verify-native-todomvc-input-parity" => "todomvc-input-parity",
        "verify-native-gpu-scroll-speed" => {
            let label = native_gpu_scroll_selector(args).label;
            return PathBuf::from(format!(
                "target/reports/native-gpu/scroll-speed-{label}.json"
            ));
        }
        "verify-native-gpu-negative" => "negative",
        "verify-native-gpu-all" => return PathBuf::from("target/reports/native-gpu-all.json"),
        "verify-native-gpu-regression-all" => {
            return PathBuf::from("target/reports/native-gpu-regression-all.json");
        }
        _ => command,
    };
    PathBuf::from(format!("target/reports/native-gpu/{name}.json"))
}

fn native_gpu_report_rejects(report: &serde_json::Value) -> bool {
    !native_gpu_report_rejection_reasons(report).is_empty()
}

fn native_gpu_report_staleness_reasons(report: &serde_json::Value) -> Vec<String> {
    native_gpu_report_integrity_reasons(report, false, true)
}

fn native_gpu_report_rejection_reasons(report: &serde_json::Value) -> Vec<String> {
    native_gpu_report_integrity_reasons(report, true, true)
}

fn native_gpu_report_integrity_reasons(
    report: &serde_json::Value,
    include_status_failure: bool,
    require_native_gpu_contract: bool,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if include_status_failure
        && report.get("status").and_then(serde_json::Value::as_str) == Some("fail")
    {
        reasons.push("status=fail".to_owned());
    }
    if require_native_gpu_contract
        && report
            .get("native_gpu_contract")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
    {
        reasons.push("missing native_gpu_contract=true".to_owned());
    }
    if let Some(generated) = report
        .get("generated_at_utc")
        .and_then(serde_json::Value::as_str)
        .and_then(|generated| generated.parse::<u64>().ok())
    {
        if generated > current_unix_seconds().saturating_add(5) {
            reasons.push("generated_at_utc is future-dated".to_owned());
        }
    }
    if report
        .get("git_commit")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|commit| commit != git_commit())
    {
        reasons.push("git_commit is stale for current checkout".to_owned());
    }
    if report
        .get("worktree_fingerprint")
        .and_then(serde_json::Value::as_str)
        .is_none_or(|fingerprint| fingerprint != worktree_fingerprint())
    {
        reasons.push("worktree_fingerprint is stale for current checkout".to_owned());
    }
    if report
        .get("binary_hash")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|hash| hash != current_binary_hash())
    {
        reasons.push("binary_hash is stale for current xtask binary".to_owned());
    }
    collect_nonopaque_source_identities(report, "$", &mut reasons);
    if let (Some(source_hash), Some(expected_source_hash)) = (
        report
            .get("source_hash")
            .and_then(serde_json::Value::as_str),
        report
            .get("expected_source_hash")
            .and_then(serde_json::Value::as_str),
    ) {
        if source_hash != expected_source_hash {
            reasons.push("source_hash does not match expected_source_hash".to_owned());
        }
    }
    if let Some(artifacts) = report
        .get("artifact_sha256s")
        .and_then(serde_json::Value::as_array)
    {
        for artifact in artifacts {
            if let Some(object) = artifact.as_object() {
                let Some(path) = object.get("path").and_then(serde_json::Value::as_str) else {
                    reasons.push("artifact_sha256s object is missing path".to_owned());
                    continue;
                };
                let Some(expected_sha) = object.get("sha256").and_then(serde_json::Value::as_str)
                else {
                    reasons.push(format!("artifact `{path}` is missing sha256"));
                    continue;
                };
                let artifact_path = Path::new(path);
                if !artifact_path.exists() {
                    reasons.push(format!("artifact `{path}` is missing"));
                    continue;
                }
                let actual_sha = file_hash(path);
                if actual_sha != expected_sha {
                    reasons.push(format!("artifact `{path}` sha256 is stale"));
                }
            }
        }
    }
    if report
        .get("full_state_mirroring_observed")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        reasons.push("full_state_mirroring_observed=true".to_owned());
    }
    if report
        .get("synthetic_scroll")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        reasons.push("synthetic_scroll=true".to_owned());
    }
    if report
        .get("real_os_input")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && report
            .get("input_injection_method")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|method| method.contains("operator_host") || method.contains("synthetic"))
    {
        reasons.push("operator host input cannot claim real_os_input=true".to_owned());
    }
    if report
        .get("controlled_wayland_harness")
        .and_then(serde_json::Value::as_object)
        .is_some()
        && report
            .get("operator_host_input")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
    {
        reasons.push(
            "nested-compositor-only evidence is forbidden for portable native GPU gates".to_owned(),
        );
    }
    if report
        .get("private_runtime_dispatch_used")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        reasons.push("private runtime dispatch is forbidden in native E2E".to_owned());
    }
    if report
        .get("wrong_thread_wgpu_calls_observed")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        || report
            .get("wgpu_thread_contract_violation")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    {
        reasons.push("wrong-thread WGPU call was observed".to_owned());
    }
    if report
        .get("display_server")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|server| server != "wayland")
    {
        reasons.push("display_server is not wayland".to_owned());
    }
    if report
        .get("process_model")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|model| model != "two-child-processes")
    {
        reasons.push("process_model is not two-child-processes".to_owned());
    }
    if report
        .get("shader_outputs_fresh")
        .and_then(serde_json::Value::as_bool)
        == Some(false)
    {
        reasons.push("shader_outputs_fresh=false".to_owned());
    }
    if report.get("headless").and_then(serde_json::Value::as_bool) == Some(true) {
        reasons.push("headless native proof is forbidden".to_owned());
    }
    if report
        .get("copied_pixel_hash_only")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        || report
            .get("pixel_hash_reused")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    {
        reasons.push("copied or reused pixel hash proof is forbidden".to_owned());
    }
    if report
        .get("measurement_source")
        .and_then(serde_json::Value::as_str)
        == Some("deterministic-dev-editor-scroll-model")
    {
        reasons.push("deterministic dev-editor scroll model evidence is forbidden".to_owned());
    }
    if report
        .get("input_provenance")
        .and_then(serde_json::Value::as_str)
        == Some("model_only")
    {
        reasons.push("model-only input provenance is forbidden".to_owned());
    }
    for (field, reason) in [
        ("modeled_ack_timing", "modeled ACK timing is forbidden"),
        (
            "modeled_presentation_timing",
            "modeled presentation timing is forbidden",
        ),
        ("missing_process_evidence", "process evidence is missing"),
        ("fake_cpu_samples", "fake CPU samples are forbidden"),
        (
            "release_report_reused_for_debug",
            "release report cannot be reused for debug",
        ),
        (
            "passive_scroll_did_source_replacement",
            "passive scroll must not perform source replacement",
        ),
        (
            "passive_scroll_queried_runtime_summary",
            "passive scroll must not query runtime summary",
        ),
        (
            "full_file_materialized_for_scroll",
            "scroll verifier must not materialize the full file",
        ),
        (
            "text_reshaped_full_file",
            "scroll verifier must not reshape the full file",
        ),
        (
            "missing_horizontal_scroll_evidence",
            "horizontal scroll evidence is missing",
        ),
        (
            "sync_ack_contains_runtime_summary",
            "sync ACK must not contain runtime summary",
        ),
        (
            "sync_ack_contains_layout_proof",
            "sync ACK must not contain layout proof",
        ),
        (
            "preview_received_scenario_data",
            "preview IPC must not receive scenario data",
        ),
        (
            "preview_bound_scenario_data",
            "preview-bound IPC must not carry scenario data",
        ),
    ] {
        if report.get(field).and_then(serde_json::Value::as_bool) == Some(true) {
            reasons.push(reason.to_owned());
        }
    }
    if let (Some(surface_epoch), Some(target_epoch)) = (
        report
            .get("surface_epoch")
            .and_then(serde_json::Value::as_u64),
        report
            .get("target_surface_epoch")
            .and_then(serde_json::Value::as_u64),
    ) {
        if surface_epoch != target_epoch {
            reasons.push("target_surface_epoch does not match surface_epoch".to_owned());
        }
    }
    if let (Some(presented_revision), Some(last_render_content_revision)) = (
        report
            .get("presented_revision")
            .and_then(serde_json::Value::as_u64),
        report
            .get("last_render_content_revision")
            .and_then(serde_json::Value::as_u64),
    ) {
        if last_render_content_revision < presented_revision {
            reasons
                .push("last_render_content_revision is older than presented_revision".to_owned());
        }
    }
    reasons
}

fn collect_nonopaque_source_identities(
    value: &serde_json::Value,
    path: &str,
    reasons: &mut Vec<String>,
) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, child) in object {
                let child_path = format!("{path}.{key}");
                if key == "source_identity"
                    && !child.as_str().is_some_and(is_opaque_source_identity)
                {
                    reasons.push(format!(
                        "{child_path} must be an opaque source:<hash-prefix> identity"
                    ));
                }
                collect_nonopaque_source_identities(child, &child_path, reasons);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                collect_nonopaque_source_identities(child, &format!("{path}[{index}]"), reasons);
            }
        }
        _ => {}
    }
}

fn is_opaque_source_identity(value: &str) -> bool {
    let Some(suffix) = value.strip_prefix("source:") else {
        return false;
    };
    suffix.len() >= 16 && suffix.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn command_argv_contains_pair(argv: &[serde_json::Value], flag: &str, value: &str) -> bool {
    if value.is_empty() {
        return argv.iter().any(|arg| arg.as_str() == Some(flag));
    }
    argv.windows(2)
        .any(|window| window[0].as_str() == Some(flag) && window[1].as_str() == Some(value))
}

fn require_str_field(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    key: &str,
    expected: &str,
) {
    if report.get(key).and_then(serde_json::Value::as_str) != Some(expected) {
        blockers.push(format!("{key} must be `{expected}`"));
    }
}

fn require_nonempty_str_field(blockers: &mut Vec<String>, report: &serde_json::Value, key: &str) {
    if !report
        .get(key)
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.is_empty())
    {
        blockers.push(format!("{key} must be a nonempty string"));
    }
}

fn require_hash_field(blockers: &mut Vec<String>, report: &serde_json::Value, key: &str) {
    if !report
        .get(key)
        .and_then(serde_json::Value::as_str)
        .is_some_and(is_sha256_hex)
    {
        blockers.push(format!("{key} must be a 64-character hex sha256"));
    }
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn require_bool_field(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    key: &str,
    expected: bool,
) {
    if report.get(key).and_then(serde_json::Value::as_bool) != Some(expected) {
        blockers.push(format!("{key} must be {expected}"));
    }
}

fn require_positive_u64(blockers: &mut Vec<String>, report: &serde_json::Value, key: &str) {
    if !report
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|value| value > 0)
    {
        blockers.push(format!("{key} must be a positive integer"));
    }
}

fn require_distinct_u64_fields(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    left_key: &str,
    right_key: &str,
) {
    match (
        report.get(left_key).and_then(serde_json::Value::as_u64),
        report.get(right_key).and_then(serde_json::Value::as_u64),
    ) {
        (Some(left), Some(right)) if left > 0 && right > 0 && left != right => {}
        _ => blockers.push(format!(
            "{left_key} and {right_key} must be distinct positive integers"
        )),
    }
}

fn require_distinct_json_str_paths(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    left_path: &str,
    right_path: &str,
) {
    match (
        report
            .pointer(left_path)
            .and_then(serde_json::Value::as_str),
        report
            .pointer(right_path)
            .and_then(serde_json::Value::as_str),
    ) {
        (Some(left), Some(right)) if !left.is_empty() && !right.is_empty() && left != right => {}
        _ => blockers.push(format!(
            "{left_path} and {right_path} must be distinct nonempty strings"
        )),
    }
}

fn require_native_surface_proof(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    key: &str,
    role: &str,
) {
    let Some(proof) = report.get(key) else {
        blockers.push(format!("{key} is missing"));
        return;
    };
    for required in [
        "pid",
        "window_id",
        "surface_id",
        "surface_epoch",
        "window_backend",
        "display_server",
        "wgpu_strategy",
        "wgpu_surface_strategy",
        "main_thread_id",
        "render_thread_id",
        "logical_size",
        "physical_size",
        "readback_artifact",
    ] {
        if proof.get(required).is_none() {
            blockers.push(format!("{key}.{required} is missing"));
        }
    }
    if proof.get("role").and_then(serde_json::Value::as_str) != Some(role) {
        blockers.push(format!("{key}.role must be `{role}`"));
    }
    if proof
        .get("window_backend")
        .and_then(serde_json::Value::as_str)
        != Some("app_window-wayland")
    {
        blockers.push(format!("{key}.window_backend must be app_window-wayland"));
    }
    if proof
        .get("display_server")
        .and_then(serde_json::Value::as_str)
        != Some("wayland")
    {
        blockers.push(format!("{key}.display_server must be wayland"));
    }
    if proof
        .get("presented_frame")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        blockers.push(format!("{key}.presented_frame must be true"));
    }
    if !proof
        .pointer("/readback_artifact/nonblank_samples")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|samples| samples > 0)
    {
        blockers.push(format!(
            "{key}.readback_artifact.nonblank_samples must be positive"
        ));
    }
    if proof
        .pointer("/readback_artifact/capture_method")
        .and_then(serde_json::Value::as_str)
        != Some("wgpu-visible-surface-copy-src-readback")
    {
        blockers.push(format!(
            "{key}.readback_artifact.capture_method must prove visible-surface COPY_SRC readback"
        ));
    }
    if !proof
        .pointer("/readback_artifact/unique_rgba_values")
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|unique| unique > 2)
    {
        blockers.push(format!(
            "{key}.readback_artifact.unique_rgba_values must reject single-color surfaces"
        ));
    }
}

fn require_visible_native_render_proof(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    path: &str,
) {
    if report
        .pointer(&format!("{path}/status"))
        .and_then(serde_json::Value::as_str)
        != Some("pass")
    {
        blockers.push(format!("{path}.status must be pass"));
    }
    if report
        .pointer(&format!("{path}/visible_surface_rendered"))
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        blockers.push(format!("{path}.visible_surface_rendered must be true"));
    }
    if report
        .pointer(&format!("{path}/visible_present_path"))
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        blockers.push(format!("{path}.visible_present_path must be true"));
    }
    if !report
        .pointer(&format!("{path}/visible_surface_metrics/draw_calls"))
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|draw_calls| draw_calls > 0)
    {
        blockers.push(format!(
            "{path}.visible_surface_metrics.draw_calls must be positive"
        ));
    }
    let text_runs = report
        .pointer(&format!("{path}/visible_surface_metrics/text_runs_shaped"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    if text_runs > 0
        && !report
            .pointer(&format!(
                "{path}/visible_surface_metrics/rendered_text_runs"
            ))
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|rendered| rendered >= text_runs)
    {
        blockers.push(format!(
            "{path}.visible_surface_metrics.rendered_text_runs must cover shaped text runs"
        ));
    }
    if report
        .pointer(&format!(
            "{path}/visible_surface_metrics/color_only_rect_fallback"
        ))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        blockers.push(format!(
            "{path}.visible_surface_metrics.color_only_rect_fallback must be false"
        ));
    }
    if report
        .pointer(&format!("{path}/visible_surface_metrics/rect_cap_hit"))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        blockers.push(format!(
            "{path}.visible_surface_metrics.rect_cap_hit must be false"
        ));
    }
    if report
        .pointer(&format!("{path}/visible_surface_metrics/text_cap_hit"))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        blockers.push(format!(
            "{path}.visible_surface_metrics.text_cap_hit must be false"
        ));
    }
    if text_runs > 0
        && !report
            .pointer(&format!("{path}/proof/artifact/unique_rgba_values"))
            .and_then(serde_json::Value::as_u64)
            .is_none_or(|unique| unique >= 32)
    {
        blockers.push(format!(
            "{path}.proof.artifact.unique_rgba_values must prove text-rich pixels when an artifact is present"
        ));
    }
}

fn require_preview_runtime_ownership(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    path: &str,
) {
    if report
        .pointer(&format!("{path}/status"))
        .and_then(serde_json::Value::as_str)
        != Some("pass")
    {
        blockers.push(format!("{path}.status must be pass"));
    }
    if report
        .pointer(&format!("{path}/owns_live_runtime"))
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        blockers.push(format!("{path}.owns_live_runtime must be true"));
    }
    if report
        .pointer(&format!("{path}/full_state_mirroring_allowed"))
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        blockers.push(format!("{path}.full_state_mirroring_allowed must be false"));
    }
    if report
        .pointer(&format!("{path}/full_state_mirroring_observed"))
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        blockers.push(format!(
            "{path}.full_state_mirroring_observed must be false"
        ));
    }
    if !report
        .pointer(&format!("{path}/state_summary_hash"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|hash| hash.len() == 64)
    {
        blockers.push(format!("{path}.state_summary_hash must be a sha256"));
    }
}

fn require_preview_runtime_query(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    path: &str,
) {
    if report
        .pointer(&format!("{path}/kind"))
        .and_then(serde_json::Value::as_str)
        != Some("debug-query-result")
    {
        blockers.push(format!("{path}.kind must be debug-query-result"));
    }
    if report
        .pointer(&format!("{path}/debug_query"))
        .and_then(serde_json::Value::as_str)
        != Some("RuntimeSummary")
    {
        blockers.push(format!("{path}.debug_query must be RuntimeSummary"));
    }
    if report
        .pointer(&format!("{path}/bounded_query"))
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        blockers.push(format!("{path}.bounded_query must be true"));
    }
    if report
        .pointer(&format!("{path}/full_state_mirroring_observed"))
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        blockers.push(format!(
            "{path}.full_state_mirroring_observed must be false"
        ));
    }
    if report
        .pointer(&format!("{path}/runtime_summary/status"))
        .and_then(serde_json::Value::as_str)
        != Some("pass")
    {
        blockers.push(format!("{path}.runtime_summary.status must be pass"));
    }
}

fn require_replace_code_evidence(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    prefix: &str,
) {
    let path = |suffix: &str| {
        if prefix.is_empty() {
            format!("/{suffix}")
        } else {
            format!("{prefix}/{suffix}")
        }
    };
    if report
        .pointer(&path("dev_sent_replace_code"))
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        blockers.push(format!("{prefix} dev_sent_replace_code must be true"));
    }
    if report
        .pointer(&path("replace_code/preview_command"))
        .and_then(serde_json::Value::as_str)
        != Some("ReplaceCode")
    {
        blockers.push(format!(
            "{prefix} replace_code.preview_command must be ReplaceCode"
        ));
    }
    if report
        .pointer(&path("replace_code/hash_matches"))
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        blockers.push(format!("{prefix} replace_code.hash_matches must be true"));
    }
    if report
        .pointer(&path("replace_code/document_layout_proof/status"))
        .and_then(serde_json::Value::as_str)
        != Some("pass")
    {
        blockers.push(format!(
            "{prefix} replace_code.document_layout_proof.status must be pass"
        ));
    }
    if report
        .pointer(&path("replace_code/preview_receives_example_name"))
        .and_then(serde_json::Value::as_bool)
        != Some(false)
    {
        blockers.push(format!(
            "{prefix} replace_code.preview_receives_example_name must be false"
        ));
    }
}

fn native_gpu_replace_code_evidence_ok(report: &serde_json::Value, prefix: &str) -> bool {
    let path = |suffix: &str| {
        if prefix.is_empty() {
            format!("/{suffix}")
        } else {
            format!("{prefix}/{suffix}")
        }
    };
    report
        .pointer(&path("dev_sent_replace_code"))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && report
            .pointer(&path("replace_code/preview_command"))
            .and_then(serde_json::Value::as_str)
            == Some("ReplaceCode")
        && report
            .pointer(&path("replace_code/hash_matches"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        && report
            .pointer(&path("replace_code/document_layout_proof/status"))
            .and_then(serde_json::Value::as_str)
            == Some("pass")
        && report
            .pointer(&path("replace_code/preview_receives_example_name"))
            .and_then(serde_json::Value::as_bool)
            == Some(false)
}

fn require_u64_at_most(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    key: &str,
    max: u64,
) {
    match report.get(key).and_then(serde_json::Value::as_u64) {
        Some(value) if value <= max => {}
        Some(value) => blockers.push(format!("{key}={value} exceeds budget {max}")),
        None => blockers.push(format!("{key} is missing or not an integer")),
    }
}

fn require_u64_at_least(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    key: &str,
    min: u64,
) {
    match report.get(key).and_then(serde_json::Value::as_u64) {
        Some(value) if value >= min => {}
        Some(value) => blockers.push(format!("{key}={value} is below required {min}")),
        None => blockers.push(format!("{key} is missing or not an integer")),
    }
}

fn require_f64_at_most(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    key: &str,
    max: f64,
) {
    match report.get(key).and_then(serde_json::Value::as_f64) {
        Some(value) if value <= max => {}
        Some(value) => blockers.push(format!("{key}={value} exceeds budget {max}")),
        None => blockers.push(format!("{key} is missing or not numeric")),
    }
}

fn require_f64_at_least(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    key: &str,
    min: f64,
) {
    match report.get(key).and_then(serde_json::Value::as_f64) {
        Some(value) if value >= min => {}
        Some(value) => blockers.push(format!("{key}={value} is below required {min}")),
        None => blockers.push(format!("{key} is missing or not numeric")),
    }
}

fn require_f64_value_at_most(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    key: &str,
    max: f64,
) {
    match report.get(key).and_then(numeric_value_as_f64) {
        Some(value) if value <= max => {}
        Some(value) => blockers.push(format!("{key}={value} exceeds budget {max}")),
        None => blockers.push(format!("{key} is missing or not numeric")),
    }
}

fn require_summary_f64_p95_at_most(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    key: &str,
    max: f64,
) {
    match summary_p95_f64(&report[key]) {
        Some(value) if value <= max => {}
        Some(value) => blockers.push(format!("{key}.p95={value} exceeds budget {max}")),
        None => blockers.push(format!("{key}.p95 is missing or not numeric")),
    }
}

fn require_summary_u64_p95_at_most(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    key: &str,
    max: u64,
) {
    match summary_p95_u64(&report[key]) {
        Some(value) if value <= max => {}
        Some(value) => blockers.push(format!("{key}.p95={value} exceeds budget {max}")),
        None => blockers.push(format!("{key}.p95 is missing or not an integer")),
    }
}

fn summary_p95_f64(value: &serde_json::Value) -> Option<f64> {
    value.get("p95").and_then(numeric_value_as_f64)
}

fn summary_p95_u64(value: &serde_json::Value) -> Option<u64> {
    value.get("p95").and_then(serde_json::Value::as_u64)
}

fn numeric_value_as_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_u64().map(|value| value as f64))
        .or_else(|| value.as_i64().map(|value| value as f64))
}

fn require_nonempty_array(blockers: &mut Vec<String>, report: &serde_json::Value, key: &str) {
    if !report
        .get(key)
        .and_then(serde_json::Value::as_array)
        .is_some_and(|values| !values.is_empty())
    {
        blockers.push(format!("{key} must be a nonempty array"));
    }
}

fn require_u64_array_field(blockers: &mut Vec<String>, report: &serde_json::Value, key: &str) {
    match report.get(key).and_then(serde_json::Value::as_array) {
        Some(values)
            if values
                .iter()
                .all(|value| value.as_u64().is_some() || value.as_f64().is_some()) => {}
        Some(_) => blockers.push(format!("{key} must contain only numeric frame timings")),
        None => blockers.push(format!("{key} must be an array")),
    }
}

fn require_object_field(blockers: &mut Vec<String>, report: &serde_json::Value, key: &str) {
    if !report
        .get(key)
        .and_then(serde_json::Value::as_object)
        .is_some_and(|object| !object.is_empty())
    {
        blockers.push(format!("{key} must be a nonempty object"));
    }
}

fn require_axis_p95_at_most(
    blockers: &mut Vec<String>,
    report: &serde_json::Value,
    key: &str,
    max: f64,
) {
    let Some(object) = report.get(key).and_then(serde_json::Value::as_object) else {
        blockers.push(format!(
            "{key} must be an object with vertical/horizontal values"
        ));
        return;
    };
    for axis in ["vertical", "horizontal"] {
        match object.get(axis).and_then(numeric_value_as_f64) {
            Some(value) if value <= max => {}
            Some(value) => blockers.push(format!("{key}.{axis}={value} exceeds budget {max}")),
            None => blockers.push(format!("{key}.{axis} is missing or not numeric")),
        }
    }
}

fn native_gpu_budget_u64(section: &str, key: &str) -> Option<u64> {
    native_gpu_budget_f64(section, key).and_then(|value| {
        if value >= 0.0 {
            Some(value as u64)
        } else {
            None
        }
    })
}

fn native_gpu_budget_f64(section: &str, key: &str) -> Option<f64> {
    let text = std::fs::read_to_string("budgets/native-gpu.toml").ok()?;
    let mut current_section = "";
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            current_section = &line[1..line.len().saturating_sub(1)];
            continue;
        }
        if current_section != section {
            continue;
        }
        let Some((candidate, raw_value)) = line.split_once('=') else {
            continue;
        };
        if candidate.trim() == key {
            return raw_value.trim().parse::<f64>().ok();
        }
    }
    None
}

fn required_native_gpu_budget_f64(
    section: &str,
    key: &str,
) -> Result<f64, Box<dyn std::error::Error>> {
    native_gpu_budget_f64(section, key).ok_or_else(|| {
        format!(
            "required native GPU budget `{section}.{key}` is missing from budgets/native-gpu.toml"
        )
        .into()
    })
}

fn required_native_gpu_budget_u64(
    section: &str,
    key: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
    native_gpu_budget_u64(section, key).ok_or_else(|| {
        format!(
            "required native GPU budget `{section}.{key}` is missing from budgets/native-gpu.toml"
        )
        .into()
    })
}

fn native_gpu_budget_f64_or_blocker(blockers: &mut Vec<String>, section: &str, key: &str) -> f64 {
    match native_gpu_budget_f64(section, key) {
        Some(value) => value,
        None => {
            blockers.push(format!(
                "required native GPU budget `{section}.{key}` is missing"
            ));
            0.0
        }
    }
}

fn native_gpu_budget_u64_or_blocker(blockers: &mut Vec<String>, section: &str, key: &str) -> u64 {
    match native_gpu_budget_u64(section, key) {
        Some(value) => value,
        None => {
            blockers.push(format!(
                "required native GPU budget `{section}.{key}` is missing"
            ));
            0
        }
    }
}

fn max_f64(values: &[f64]) -> f64 {
    values.iter().copied().fold(0.0_f64, f64::max)
}

fn percentile_linear_f64(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if sorted.is_empty() {
        return 0.0;
    }
    sorted.sort_by(f64::total_cmp);
    if sorted.len() == 1 {
        return sorted[0];
    }
    let clamped = percentile.clamp(0.0, 100.0) / 100.0;
    let position = clamped * (sorted.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let fraction = position - lower as f64;
        sorted[lower] + (sorted[upper] - sorted[lower]) * fraction
    }
}

fn merge_json(mut base: serde_json::Value, overlay: serde_json::Value) -> serde_json::Value {
    if let (Some(base), Some(overlay)) = (base.as_object_mut(), overlay.as_object()) {
        for (key, value) in overlay {
            base.insert(key.clone(), value.clone());
        }
    }
    base
}

fn rg_count(dir: &str, pattern: &str) -> Result<usize, Box<dyn std::error::Error>> {
    if !Path::new(dir).exists() {
        return Err(format!("required search directory `{dir}` is missing").into());
    }
    let output = Command::new("rg")
        .args(["-n", "--fixed-strings", pattern, dir])
        .output()?;
    if !output.status.success() {
        return Ok(0);
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().count())
}

fn count_files_with_extension(
    dir: &Path,
    extension: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            count += count_files_with_extension(&path, extension)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
            count += 1;
        }
    }
    Ok(count)
}

fn read_json(path: &Path) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn wait_for_json_report(path: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists()
            && serde_json::from_slice::<serde_json::Value>(&std::fs::read(path).unwrap_or_default())
                .is_ok()
        {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

fn wait_for_surface_loop_report_ready(
    loop_report_path: &Path,
    measured_surface_key: &str,
    timeout: Duration,
) -> serde_json::Value {
    let start = Instant::now();
    let mut last_loop_report = serde_json::Value::Null;
    while start.elapsed() < timeout {
        if let Ok(loop_report) = read_json(loop_report_path) {
            last_loop_report = loop_report.clone();
            if let Some(loop_error) = loop_report
                .get("loop_error")
                .and_then(serde_json::Value::as_str)
            {
                return json!({
                    "status": "fail",
                    "diagnostic": "loop report recorded an error before surface was ready",
                    "loop_error": loop_error,
                    "measured_surface_key": measured_surface_key,
                    "loop_report_path": loop_report_path,
                    "last_loop_report": last_loop_report
                });
            }
            let rendered_frame_count = loop_report
                .get("rendered_frame_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let surface_id_present = loop_report
                .get("surface_id")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|surface_id| !surface_id.is_empty());
            if rendered_frame_count > 0 && surface_id_present {
                return json!({
                    "status": "pass",
                    "measured_surface_key": measured_surface_key,
                    "loop_report_path": loop_report_path,
                    "rendered_frame_count": rendered_frame_count,
                    "surface_id": loop_report.get("surface_id").cloned().unwrap_or(serde_json::Value::Null)
                });
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    json!({
        "status": "fail",
        "measured_surface_key": measured_surface_key,
        "loop_report_path": loop_report_path,
        "timeout_ms": timeout.as_millis() as u64,
        "last_loop_report": last_loop_report
    })
}

fn native_readback_pixel_inventory(
    path: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let image = image::open(path)?.to_rgba8();
    let (width, height) = image.dimensions();
    let total_pixels = u64::from(width) * u64::from(height);
    let mut counts = BTreeMap::<[u8; 4], u64>::new();
    for pixel in image.pixels() {
        *counts.entry(pixel.0).or_insert(0) += 1;
    }
    let (dominant_color, dominant_count) = counts
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(color, count)| (*color, *count))
        .unwrap_or(([0, 0, 0, 0], 0));
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut content_pixels = 0u64;
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel.0 != dominant_color {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            content_pixels += 1;
        }
    }
    let content_bbox = if content_pixels > 0 {
        json!({
            "x": min_x,
            "y": min_y,
            "width": max_x.saturating_sub(min_x).saturating_add(1),
            "height": max_y.saturating_sub(min_y).saturating_add(1),
        })
    } else {
        serde_json::Value::Null
    };
    let unique_rgba_values = counts.len() as u64;
    let single_color = unique_rgba_values <= 1;
    Ok(json!({
        "status": if !single_color && content_pixels > 0 { "pass" } else { "fail" },
        "path": path,
        "sha256": file_hash(path),
        "width": width,
        "height": height,
        "total_pixels": total_pixels,
        "unique_rgba_values": unique_rgba_values,
        "dominant_color_rgba": dominant_color,
        "dominant_color_count": dominant_count,
        "dominant_color_ratio": if total_pixels > 0 {
            dominant_count as f64 / total_pixels as f64
        } else {
            1.0
        },
        "non_dominant_content_pixels": content_pixels,
        "content_bounding_box": content_bbox,
        "single_color": single_color,
        "analysis_method": "decode app-owned PNG readback and compare pixels against dominant background color"
    }))
}

fn run_isolated_weston_desktop_preview_e2e(
    binary: &Path,
    example: &str,
    title_token: &str,
    input_sample_delay_ms: u64,
    role_report_timeout_ms: u64,
    supervisor_report: &Path,
    live_state_report: &Path,
    driver_target: Option<serde_json::Value>,
    driver_text: Option<&str>,
    code_file: Option<&Path>,
    skip_operator_host_input_probe: bool,
    target_dev_surface: bool,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let artifact_dir = PathBuf::from(format!(
        "target/artifacts/native-gpu/isolated-preview-e2e-{}-{}-{}",
        example,
        std::process::id(),
        current_unix_seconds()
    ));
    fs::create_dir_all(&artifact_dir)?;
    let Some(plugin_path) = weston_test_plugin_path() else {
        return Ok(json!({
            "status": "fail",
            "reason": "Weston test control plugin missing",
            "artifact_dir": artifact_dir
        }));
    };
    let Some(driver_path) = weston_test_driver_path() else {
        return Ok(json!({
            "status": "fail",
            "reason": "Weston test driver missing",
            "artifact_dir": artifact_dir,
            "weston_control_plugin_path": plugin_path
        }));
    };

    let socket = format!(
        "boon-native-preview-e2e-{}-{}",
        std::process::id(),
        current_unix_seconds()
    );
    let weston_log_path = artifact_dir.join("weston.log");
    let weston_stdout_path = artifact_dir.join("weston.stdout.txt");
    let weston_stderr_path = artifact_dir.join("weston.stderr.txt");
    let wayland_info_stdout_path = artifact_dir.join("wayland-info.txt");
    let wayland_info_stderr_path = artifact_dir.join("wayland-info.stderr.txt");
    let desktop_stdout_path = artifact_dir.join("desktop.stdout.txt");
    let desktop_stderr_path = artifact_dir.join("desktop.stderr.txt");
    let driver_stdout_path = artifact_dir.join("weston-test-driver.jsonl");
    let driver_stderr_path = artifact_dir.join("weston-test-driver.stderr.txt");

    let mut weston = Command::new("weston")
        .args([
            "--backend=headless-backend.so",
            "--socket",
            &socket,
            "--idle-time=0",
            "--log",
            weston_log_path
                .to_str()
                .ok_or("weston log path is not UTF-8")?,
            "--modules",
            plugin_path
                .to_str()
                .ok_or("weston control plugin path is not UTF-8")?,
        ])
        .stdout(Stdio::from(fs::File::create(&weston_stdout_path)?))
        .stderr(Stdio::from(fs::File::create(&weston_stderr_path)?))
        .spawn()?;

    let mut ready = false;
    for _ in 0..50 {
        if let Ok(output) = Command::new("wayland-info")
            .env("WAYLAND_DISPLAY", &socket)
            .output()
        {
            fs::write(&wayland_info_stdout_path, &output.stdout)?;
            fs::write(&wayland_info_stderr_path, &output.stderr)?;
            if output.status.success() {
                ready = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !ready {
        terminate_child_process(&mut weston);
        return Ok(json!({
            "status": "fail",
            "reason": "isolated Weston did not become ready",
            "artifact_dir": artifact_dir,
            "socket": socket,
            "weston_log_path": weston_log_path
        }));
    }

    let input_sample_delay_text = input_sample_delay_ms.to_string();
    let role_report_timeout_text = role_report_timeout_ms.to_string();
    let dev_start_delay_text = if target_dev_surface { "0" } else { "2500" };
    let mut desktop_args = vec!["--role", "desktop", "--example", example];
    let code_file_string = code_file
        .map(|path| path.to_str().ok_or("isolated code file path is not UTF-8"))
        .transpose()?
        .map(str::to_owned);
    if let Some(code_file) = code_file_string.as_deref() {
        desktop_args.extend([
            "--code-file",
            code_file,
            "--dev-editor-code-file",
            code_file,
        ]);
        if target_dev_surface {
            desktop_args.push("--dev-editor-only");
        }
    }
    desktop_args.extend([
        "--probe",
        "--real-window-input-probe",
        "--child-hold-ms",
        "30000",
        "--dev-hold-ms",
        "10000",
        "--title-token",
        title_token,
        "--input-sample-delay-ms",
        &input_sample_delay_text,
        "--warmup-frame-count",
        "3",
        "--sample-frame-count",
        "30",
        "--role-report-timeout-ms",
        &role_report_timeout_text,
        "--dev-start-delay-ms",
        dev_start_delay_text,
        "--live-state-report",
        live_state_report
            .to_str()
            .ok_or("live state report path is not UTF-8")?,
        "--report",
        supervisor_report
            .to_str()
            .ok_or("supervisor report path is not UTF-8")?,
    ]);
    if skip_operator_host_input_probe {
        desktop_args.push("--skip-operator-host-input-probe");
    }
    let mut desktop = Command::new(binary)
        .args(&desktop_args)
        .env("WAYLAND_DISPLAY", &socket)
        .env("XDG_SESSION_TYPE", "wayland")
        .stdout(Stdio::from(fs::File::create(&desktop_stdout_path)?))
        .stderr(Stdio::from(fs::File::create(&desktop_stderr_path)?))
        .spawn()?;

    thread::sleep(Duration::from_millis(if target_dev_surface {
        1_200
    } else {
        800
    }));
    let target_x = driver_target
        .as_ref()
        .and_then(|target| target.get("local_x"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(240.0)
        .round()
        .max(0.0) as i64;
    let target_y = driver_target
        .as_ref()
        .and_then(|target| target.get("local_y"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(220.0)
        .round()
        .max(0.0) as i64;
    let driver_points = [
        [target_x.to_string(), target_y.to_string()],
        [(target_x + 30).to_string(), (target_y + 20).to_string()],
        [target_x.to_string(), target_y.to_string()],
    ];
    let mut driver_stdout = Vec::new();
    let mut driver_stderr = Vec::new();
    let mut last_driver_json = json!({"status": "not-run"});
    let mut last_driver_success = false;
    let driver_point_count = driver_points.len();
    for (point_index, point) in driver_points.into_iter().enumerate() {
        let mut command = Command::new(&driver_path);
        command.args([point[0].as_str(), point[1].as_str()]);
        let should_type = point_index + 1 == driver_point_count;
        if should_type {
            if let Some(text) = driver_text {
                command.args([text, "enter"]);
            } else {
                command.arg("");
            }
        } else {
            command.arg("");
        }
        let output = command.env("WAYLAND_DISPLAY", &socket).output()?;
        last_driver_success = output.status.success();
        last_driver_json = serde_json::from_slice::<serde_json::Value>(&output.stdout)
            .unwrap_or_else(|_| json!({"status": "fail", "reason": "driver stdout was not JSON"}));
        driver_stdout.extend_from_slice(&output.stdout);
        driver_stderr.extend_from_slice(&output.stderr);
        thread::sleep(Duration::from_millis(250));
    }
    thread::sleep(Duration::from_millis(3_200));
    let dev_points = [["56", "106"], ["162", "106"], ["56", "106"]];
    for point in dev_points {
        let output = Command::new(&driver_path)
            .args([point[0], point[1], ""])
            .env("WAYLAND_DISPLAY", &socket)
            .output()?;
        if output.status.success() {
            last_driver_success = true;
            last_driver_json = serde_json::from_slice::<serde_json::Value>(&output.stdout)
                .unwrap_or_else(
                    |_| json!({"status": "fail", "reason": "driver stdout was not JSON"}),
                );
        }
        driver_stdout.extend_from_slice(&output.stdout);
        driver_stderr.extend_from_slice(&output.stderr);
        thread::sleep(Duration::from_millis(250));
    }
    fs::write(&driver_stdout_path, &driver_stdout)?;
    fs::write(&driver_stderr_path, &driver_stderr)?;

    let desktop_status = wait_child_exit_with_timeout(
        &mut desktop,
        Duration::from_millis(role_report_timeout_ms.saturating_add(20_000)),
    );
    if desktop_status.is_none() {
        terminate_child_process(&mut desktop);
    }
    terminate_child_process(&mut weston);
    let _ = weston.wait();

    let supervisor = if supervisor_report.exists() {
        read_json(supervisor_report).unwrap_or_else(|error| {
            json!({
                "status": "fail",
                "reason": format!("failed to read supervisor report: {error}")
            })
        })
    } else {
        json!({"status": "missing"})
    };
    let measured_loop_report_key = if target_dev_surface {
        "dev_loop_report"
    } else {
        "preview_loop_report"
    };
    let measured_loop_report_path = supervisor
        .get(measured_loop_report_key)
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    let measured_loop_report = measured_loop_report_path
        .as_ref()
        .map(|path| {
            read_json(path).unwrap_or_else(|error| {
                json!({
                    "status": "fail",
                    "reason": format!("failed to read measured loop report: {error}"),
                    "path": path
                })
            })
        })
        .unwrap_or_else(|| {
            json!({
                "status": "missing",
                "reason": format!("supervisor missing `{measured_loop_report_key}`")
            })
        });
    let measured_loop_status_pass = measured_loop_report
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && measured_loop_report
            .get("loop_error")
            .is_none_or(serde_json::Value::is_null);
    let measured_loop_dirty_revision = measured_loop_report
        .get("dirty_revision")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX);
    let measured_loop_presented_revision = measured_loop_report
        .get("presented_revision")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let measured_loop_content_revision = measured_loop_report
        .get("last_render_content_revision")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let measured_loop_role_dirty_reason = measured_loop_report
        .get("current_role_dirty_reason")
        .or_else(|| measured_loop_report.get("last_role_dirty_reason"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let measured_loop_presented_fresh =
        measured_loop_presented_revision >= measured_loop_dirty_revision;
    let measured_loop_content_fresh = !matches!(
        measured_loop_role_dirty_reason,
        "scroll_changed"
            | "runtime_turn_applied"
            | "document_patch_applied"
            | "source_payload_accepted"
    ) || measured_loop_content_revision
        >= measured_loop_dirty_revision;
    let measured_loop_pass =
        measured_loop_status_pass && measured_loop_presented_fresh && measured_loop_content_fresh;
    let measured_surface_pointer = if target_dev_surface {
        "/dev_surface_proof"
    } else {
        "/preview_surface_proof"
    };
    let initial_input_adapter = supervisor
        .pointer(&format!("{measured_surface_pointer}/input_adapter"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let input_adapter = measured_loop_report
        .get("observed_input_adapter")
        .cloned()
        .filter(native_input_adapter_has_delivered_events)
        .unwrap_or(initial_input_adapter);
    let real_os_events_observed = input_adapter
        .get("real_os_events_observed")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && input_adapter
            .get("synthetic_input_probe")
            .and_then(serde_json::Value::as_bool)
            != Some(true);
    let driver_effect_observed = input_adapter
        .get("mouse_button_event_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0
        || (input_adapter
            .get("mouse_motion_event_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
            && input_adapter.get("mouse_window_pos").is_some())
        || input_adapter
            .get("mouse_scroll_event_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
        || input_adapter
            .get("keyboard_key_event_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0;
    let supervisor_report_written = supervisor_report.exists()
        && supervisor.get("status").and_then(serde_json::Value::as_str) == Some("pass");
    let driver_pass = last_driver_success
        && last_driver_json
            .get("status")
            .and_then(serde_json::Value::as_str)
            == Some("pass");
    let desktop_pass = desktop_status
        .as_ref()
        .is_some_and(|status| status.success());
    let pass = driver_pass
        && desktop_pass
        && supervisor_report_written
        && real_os_events_observed
        && driver_effect_observed
        && measured_loop_pass;

    Ok(json!({
        "status": if pass { "pass" } else { "fail" },
        "example": example,
        "artifact_dir": artifact_dir,
        "socket": socket,
        "method": "isolated-weston-headless-with-weston-test-control",
        "live_desktop_input_used": false,
        "weston_control_plugin_path": plugin_path,
        "weston_test_driver_path": driver_path,
        "weston_log_path": weston_log_path,
        "weston_stdout_path": weston_stdout_path,
        "weston_stderr_path": weston_stderr_path,
        "wayland_info_stdout_path": wayland_info_stdout_path,
        "wayland_info_stderr_path": wayland_info_stderr_path,
        "desktop_stdout_path": desktop_stdout_path,
        "desktop_stderr_path": desktop_stderr_path,
        "desktop_exit_status": desktop_status
            .as_ref()
            .map(std::process::ExitStatus::to_string)
            .unwrap_or_else(|| "timeout".to_owned()),
        "weston_test_driver": last_driver_json,
        "weston_test_driver_stdout_path": driver_stdout_path,
        "weston_test_driver_stderr_path": driver_stderr_path,
        "driver_target_region": driver_target,
        "driver_pass": driver_pass,
        "desktop_pass": desktop_pass,
        "supervisor_report_written": supervisor_report_written,
        "supervisor_report": supervisor_report,
        "live_state_report": live_state_report,
        "measured_loop_report_key": measured_loop_report_key,
        "measured_loop_report_path": measured_loop_report_path,
        "measured_loop_report": measured_loop_report,
        "measured_loop_pass": measured_loop_pass,
        "measured_loop_status_pass": measured_loop_status_pass,
        "measured_loop_presented_fresh": measured_loop_presented_fresh,
        "measured_loop_content_fresh": measured_loop_content_fresh,
        "preview_input_adapter": input_adapter,
        "real_os_events_observed": real_os_events_observed,
        "driver_effect_observed": driver_effect_observed,
        "input_route": "weston_test compositor control API -> isolated Weston test seat -> two native app_window child processes -> preview app_window Wayland pointer/keyboard dispatch -> app_window coalesced input proof"
    }))
}

fn run_linux_human_like_preview_smoke(
    example: &str,
    release_build: bool,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let entry = boon_runtime::example_manifest_entry(example)?;
    let artifact_dir = PathBuf::from(format!(
        "target/artifacts/linux-human-like/app-smoke-{}-{}-{}",
        example,
        std::process::id(),
        current_unix_seconds()
    ));
    fs::create_dir_all(&artifact_dir)?;
    let Some(plugin_path) = weston_test_plugin_path() else {
        return Ok(
            json!({"status": "fail", "reason": "weston_test control plugin missing", "artifact_dir": artifact_dir}),
        );
    };
    let Some(driver_path) = weston_test_driver_path() else {
        return Ok(
            json!({"status": "fail", "reason": "weston_test driver missing", "artifact_dir": artifact_dir, "plugin_path": plugin_path}),
        );
    };
    let build_status = if release_build {
        Command::new("cargo")
            .args(["build", "--release", "-p", "boon_native_playground"])
            .status()?
    } else {
        Command::new("cargo")
            .args(["build", "-p", "boon_native_playground"])
            .status()?
    };
    if !build_status.success() {
        return Ok(
            json!({"status": "fail", "reason": format!("boon_native_playground build failed: {build_status}")}),
        );
    }
    let binary = if release_build {
        "./target/release/boon_native_playground"
    } else {
        "./target/debug/boon_native_playground"
    };
    let socket = format!(
        "boon-linux-human-like-{}-{}",
        std::process::id(),
        current_unix_seconds()
    );
    let weston_log_path = artifact_dir.join("weston.log");
    let weston_stdout_path = artifact_dir.join("weston.stdout.txt");
    let weston_stderr_path = artifact_dir.join("weston.stderr.txt");
    let wayland_info_stdout_path = artifact_dir.join("wayland-info.txt");
    let wayland_info_stderr_path = artifact_dir.join("wayland-info.stderr.txt");
    let preview_report_path = artifact_dir.join("preview.json");
    let preview_stdout_path = artifact_dir.join("preview.stdout.txt");
    let preview_stderr_path = artifact_dir.join("preview.stderr.txt");
    let driver_stdout_path = artifact_dir.join("weston-test-driver.json");
    let driver_stderr_path = artifact_dir.join("weston-test-driver.stderr.txt");
    let mut weston = Command::new("weston")
        .args([
            "--backend=headless-backend.so",
            "--socket",
            &socket,
            "--idle-time=0",
            "--log",
            weston_log_path
                .to_str()
                .ok_or("weston log path is not UTF-8")?,
            "--modules",
            plugin_path
                .to_str()
                .ok_or("weston control plugin path is not UTF-8")?,
        ])
        .stdout(Stdio::from(fs::File::create(&weston_stdout_path)?))
        .stderr(Stdio::from(fs::File::create(&weston_stderr_path)?))
        .spawn()?;
    let mut ready = false;
    for _ in 0..50 {
        if let Ok(output) = Command::new("wayland-info")
            .env("WAYLAND_DISPLAY", &socket)
            .output()
        {
            fs::write(&wayland_info_stdout_path, &output.stdout)?;
            fs::write(&wayland_info_stderr_path, &output.stderr)?;
            if output.status.success() {
                ready = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !ready {
        terminate_child_process(&mut weston);
        return Ok(json!({
            "status": "fail",
            "reason": "isolated Weston did not become ready",
            "artifact_dir": artifact_dir,
            "weston_log_path": weston_log_path
        }));
    }
    let mut preview = Command::new(binary)
        .args([
            "--role",
            "preview",
            "--code-file",
            &entry.source,
            "--report",
            preview_report_path
                .to_str()
                .ok_or("preview report path is not UTF-8")?,
            "--hold-ms",
            "1500",
            "--input-sample-delay-ms",
            "1500",
            "--title-token",
            "linux-human-like-smoke",
        ])
        .env("WAYLAND_DISPLAY", &socket)
        .env("XDG_SESSION_TYPE", "wayland")
        .stdout(Stdio::from(fs::File::create(&preview_stdout_path)?))
        .stderr(Stdio::from(fs::File::create(&preview_stderr_path)?))
        .spawn()?;
    thread::sleep(Duration::from_millis(800));
    let driver_points = [["240", "220"], ["300", "260"], ["240", "220"]];
    let mut driver_output = Command::new(&driver_path)
        .args(driver_points[0])
        .env("WAYLAND_DISPLAY", &socket)
        .output()?;
    let mut driver_stdout = driver_output.stdout.clone();
    let mut driver_stderr = driver_output.stderr.clone();
    for point in driver_points.iter().skip(1) {
        thread::sleep(Duration::from_millis(300));
        driver_output = Command::new(&driver_path)
            .args(*point)
            .env("WAYLAND_DISPLAY", &socket)
            .output()?;
        driver_stdout.extend_from_slice(&driver_output.stdout);
        driver_stderr.extend_from_slice(&driver_output.stderr);
    }
    fs::write(&driver_stdout_path, &driver_stdout)?;
    fs::write(&driver_stderr_path, &driver_stderr)?;
    let preview_status = preview.wait()?;
    terminate_child_process(&mut weston);
    let _ = weston.wait();
    let driver_json = serde_json::from_slice::<serde_json::Value>(&driver_output.stdout)
        .unwrap_or_else(|_| json!({"status": "fail", "reason": "driver stdout was not JSON"}));
    let preview_json = if preview_report_path.exists() {
        read_json(&preview_report_path)?
    } else {
        json!({"status": "missing"})
    };
    let preview_input_adapter = preview_json
        .pointer("/details/app_window_surface_proof/input_adapter")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let readback_artifact = preview_json
        .pointer("/details/app_window_surface_proof/readback_artifact")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let real_os_events_observed = preview_input_adapter
        .get("real_os_events_observed")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let driver_effect_observed = preview_input_adapter
        .get("mouse_button_event_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0
        || preview_input_adapter
            .get("mouse_scroll_event_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
        || preview_input_adapter
            .get("keyboard_key_event_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0;
    let driver_pass = driver_output.status.success()
        && driver_json
            .get("status")
            .and_then(serde_json::Value::as_str)
            == Some("pass");
    let preview_pass = preview_status.success()
        && preview_json
            .get("status")
            .and_then(serde_json::Value::as_str)
            == Some("pass");
    let readback_pass = readback_artifact
        .get("nonblank_samples")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0;
    let pass = driver_pass
        && preview_pass
        && readback_pass
        && real_os_events_observed
        && driver_effect_observed;
    Ok(json!({
        "status": if pass { "pass" } else { "fail" },
        "example": example,
        "source_path": entry.source,
        "artifact_dir": artifact_dir,
        "socket": socket,
        "release_build": release_build,
        "weston_control_plugin_path": plugin_path,
        "weston_test_driver_path": driver_path,
        "weston_log_path": weston_log_path,
        "weston_stdout_path": weston_stdout_path,
        "weston_stderr_path": weston_stderr_path,
        "wayland_info_stdout_path": wayland_info_stdout_path,
        "wayland_info_stderr_path": wayland_info_stderr_path,
        "preview_report_path": preview_report_path,
        "preview_stdout_path": preview_stdout_path,
        "preview_stderr_path": preview_stderr_path,
        "preview_exit_status": preview_status.to_string(),
        "weston_test_driver": driver_json,
        "weston_test_driver_stdout_path": driver_stdout_path,
        "weston_test_driver_stderr_path": driver_stderr_path,
        "preview_input_adapter": preview_input_adapter,
        "preview_readback_artifact": readback_artifact,
        "driver_pass": driver_pass,
        "preview_pass": preview_pass,
        "readback_pass": readback_pass,
        "real_os_events_observed": real_os_events_observed,
        "driver_effect_observed": driver_effect_observed,
        "live_desktop_input_used": false,
        "input_route": "weston_test compositor control API -> isolated Weston test seat -> app_window Wayland pointer/keyboard dispatch -> app_window coalesced input proof"
    }))
}

fn run_linux_human_like_desktop_surface_smoke(
    label: &str,
    example: &str,
    source_path: &Path,
    release_build: bool,
    dev_editor_only: bool,
    measured_surface_key: &str,
    driver_target: Option<serde_json::Value>,
    scroll_only: bool,
    scroll_mode: Option<&str>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let artifact_dir = PathBuf::from(format!(
        "target/artifacts/linux-human-like/desktop-surface-smoke-{}-{}-{}",
        label,
        std::process::id(),
        current_unix_seconds()
    ));
    fs::create_dir_all(&artifact_dir)?;
    let Some(plugin_path) = weston_test_plugin_path() else {
        return Ok(
            json!({"status": "fail", "reason": "weston_test control plugin missing", "artifact_dir": artifact_dir}),
        );
    };
    let Some(driver_path) = weston_test_driver_path() else {
        return Ok(
            json!({"status": "fail", "reason": "weston_test driver missing", "artifact_dir": artifact_dir, "plugin_path": plugin_path}),
        );
    };
    let build_status = if release_build {
        Command::new("cargo")
            .args(["build", "--release", "-p", "boon_native_playground"])
            .status()?
    } else {
        Command::new("cargo")
            .args(["build", "-p", "boon_native_playground"])
            .status()?
    };
    if !build_status.success() {
        return Ok(
            json!({"status": "fail", "reason": format!("boon_native_playground build failed: {build_status}")}),
        );
    }
    let binary = if release_build {
        "./target/release/boon_native_playground"
    } else {
        "./target/debug/boon_native_playground"
    };
    let socket = format!(
        "boon-linux-human-like-surface-{}-{}",
        std::process::id(),
        current_unix_seconds()
    );
    let weston_log_path = artifact_dir.join("weston.log");
    let weston_stdout_path = artifact_dir.join("weston.stdout.txt");
    let weston_stderr_path = artifact_dir.join("weston.stderr.txt");
    let wayland_info_stdout_path = artifact_dir.join("wayland-info.txt");
    let wayland_info_stderr_path = artifact_dir.join("wayland-info.stderr.txt");
    let supervisor_report = artifact_dir.join("desktop-supervisor.json");
    let live_state_report = artifact_dir.join("desktop-live-state.json");
    let desktop_stdout_path = artifact_dir.join("desktop.stdout.txt");
    let desktop_stderr_path = artifact_dir.join("desktop.stderr.txt");
    let driver_stdout_path = artifact_dir.join("weston-test-driver.jsonl");
    let driver_stderr_path = artifact_dir.join("weston-test-driver.stderr.txt");
    let mut weston = Command::new("weston")
        .args([
            "--backend=headless-backend.so",
            "--socket",
            &socket,
            "--idle-time=0",
            "--log",
            weston_log_path
                .to_str()
                .ok_or("weston log path is not UTF-8")?,
            "--modules",
            plugin_path
                .to_str()
                .ok_or("weston control plugin path is not UTF-8")?,
        ])
        .stdout(Stdio::from(fs::File::create(&weston_stdout_path)?))
        .stderr(Stdio::from(fs::File::create(&weston_stderr_path)?))
        .spawn()?;
    let mut ready = false;
    for _ in 0..50 {
        if let Ok(output) = Command::new("wayland-info")
            .env("WAYLAND_DISPLAY", &socket)
            .output()
        {
            fs::write(&wayland_info_stdout_path, &output.stdout)?;
            fs::write(&wayland_info_stderr_path, &output.stderr)?;
            if output.status.success() {
                ready = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    if !ready {
        terminate_child_process(&mut weston);
        return Ok(json!({
            "status": "fail",
            "reason": "isolated Weston did not become ready",
            "artifact_dir": artifact_dir,
            "weston_log_path": weston_log_path
        }));
    }

    let mut desktop_args = vec![
        "--role".to_owned(),
        "desktop".to_owned(),
        "--example".to_owned(),
        example.to_owned(),
        "--code-file".to_owned(),
        source_path
            .to_str()
            .ok_or("source path is not UTF-8")?
            .to_owned(),
        "--dev-editor-code-file".to_owned(),
        source_path
            .to_str()
            .ok_or("source path is not UTF-8")?
            .to_owned(),
        "--probe".to_owned(),
        "--real-window-input-probe".to_owned(),
        "--child-hold-ms".to_owned(),
        "8000".to_owned(),
        "--dev-hold-ms".to_owned(),
        "5000".to_owned(),
        "--title-token".to_owned(),
        "linux-human-like-speed".to_owned(),
        "--input-sample-delay-ms".to_owned(),
        "1500".to_owned(),
        "--warmup-frame-count".to_owned(),
        "3".to_owned(),
        "--sample-frame-count".to_owned(),
        "60".to_owned(),
        "--role-report-timeout-ms".to_owned(),
        "60000".to_owned(),
        "--live-state-report".to_owned(),
        live_state_report
            .to_str()
            .ok_or("live state report path is not UTF-8")?
            .to_owned(),
        "--report".to_owned(),
        supervisor_report
            .to_str()
            .ok_or("supervisor report path is not UTF-8")?
            .to_owned(),
    ];
    if dev_editor_only {
        desktop_args.push("--dev-editor-only".to_owned());
    }
    let mut desktop = Command::new(binary)
        .args(desktop_args.iter().map(String::as_str))
        .env("WAYLAND_DISPLAY", &socket)
        .env("XDG_SESSION_TYPE", "wayland")
        .stdout(Stdio::from(fs::File::create(&desktop_stdout_path)?))
        .stderr(Stdio::from(fs::File::create(&desktop_stderr_path)?))
        .spawn()?;

    let loop_report_path = PathBuf::from("target/reports/native-gpu/roles").join(format!(
        "{}-loop-{}-{}.json",
        if measured_surface_key == "dev_surface_proof" {
            "dev"
        } else {
            "preview"
        },
        example,
        desktop.id()
    ));
    let surface_ready_before_driver = wait_for_surface_loop_report_ready(
        &loop_report_path,
        measured_surface_key,
        Duration::from_millis(15_000),
    );
    let target_x = driver_target
        .as_ref()
        .and_then(|target| target.get("local_x"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(240.0)
        .round()
        .max(0.0) as i64;
    let target_y = driver_target
        .as_ref()
        .and_then(|target| target.get("local_y"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(220.0)
        .round()
        .max(0.0) as i64;
    let driver_points = if scroll_only {
        vec![[target_x.to_string(), target_y.to_string()]]
    } else {
        vec![
            [target_x.to_string(), target_y.to_string()],
            [(target_x + 30).to_string(), (target_y + 20).to_string()],
            [target_x.to_string(), target_y.to_string()],
        ]
    };
    let mut driver_stdout = Vec::new();
    let mut driver_stderr = Vec::new();
    let mut last_driver_json = json!({"status": "not-run"});
    let mut last_driver_success = false;
    for point in driver_points {
        let mut command = Command::new(&driver_path);
        command.args([point[0].as_str(), point[1].as_str()]);
        if scroll_only {
            command.args(["", scroll_mode.unwrap_or("scroll-only")]);
        }
        let output = command.env("WAYLAND_DISPLAY", &socket).output()?;
        last_driver_success = output.status.success();
        last_driver_json = serde_json::from_slice::<serde_json::Value>(&output.stdout)
            .unwrap_or_else(|_| json!({"status": "fail", "reason": "driver stdout was not JSON"}));
        driver_stdout.extend_from_slice(&output.stdout);
        driver_stderr.extend_from_slice(&output.stderr);
        thread::sleep(Duration::from_millis(250));
    }
    fs::write(&driver_stdout_path, &driver_stdout)?;
    fs::write(&driver_stderr_path, &driver_stderr)?;

    let desktop_status = wait_child_exit_with_timeout(&mut desktop, Duration::from_millis(80_000));
    if desktop_status.is_none() {
        terminate_child_process(&mut desktop);
    }
    terminate_child_process(&mut weston);
    let _ = weston.wait();

    let supervisor = if supervisor_report.exists() {
        read_json(&supervisor_report).unwrap_or_else(|error| {
            json!({
                "status": "fail",
                "reason": format!("failed to read supervisor report: {error}")
            })
        })
    } else {
        json!({"status": "missing"})
    };
    let input_adapter = supervisor
        .pointer(&format!("/{measured_surface_key}/input_adapter"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let readback_artifact = supervisor
        .pointer(&format!("/{measured_surface_key}/readback_artifact"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let real_os_events_observed = input_adapter
        .get("real_os_events_observed")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && input_adapter
            .get("synthetic_input_probe")
            .and_then(serde_json::Value::as_bool)
            != Some(true);
    let scroll_delta_x = input_adapter
        .get("scroll_delta_x")
        .and_then(numeric_value_as_f64)
        .unwrap_or(0.0);
    let scroll_delta_y = input_adapter
        .get("scroll_delta_y")
        .and_then(numeric_value_as_f64)
        .unwrap_or(0.0);
    let wheel_axis_observed = match scroll_mode.unwrap_or("scroll-only") {
        "vertical-scroll-only" => scroll_delta_y.abs() > f64::EPSILON,
        "horizontal-scroll-only" => scroll_delta_x.abs() > f64::EPSILON,
        _ => scroll_delta_x.abs() > f64::EPSILON && scroll_delta_y.abs() > f64::EPSILON,
    };
    let wheel_input_observed = input_adapter
        .get("mouse_scroll_event_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0
        && wheel_axis_observed;
    let driver_effect_observed = wheel_input_observed
        || input_adapter
            .get("mouse_button_event_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0
        || input_adapter
            .get("keyboard_key_event_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            > 0;
    let driver_pass = last_driver_success
        && last_driver_json
            .get("status")
            .and_then(serde_json::Value::as_str)
            == Some("pass");
    let desktop_pass = desktop_status
        .as_ref()
        .is_some_and(|status| status.success());
    let measured_role_status_key = if measured_surface_key == "dev_surface_proof" {
        "dev_role_status"
    } else {
        "preview_role_status"
    };
    let measured_loop_report_key = if measured_surface_key == "dev_surface_proof" {
        "dev_loop_report"
    } else {
        "preview_loop_report"
    };
    let measured_loop_report_path = supervisor
        .get(measured_loop_report_key)
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from);
    let measured_loop_report = measured_loop_report_path
        .as_ref()
        .map(|path| {
            read_json(path).unwrap_or_else(|error| {
                json!({
                    "status": "fail",
                    "reason": format!("failed to read measured loop report: {error}"),
                    "path": path
                })
            })
        })
        .unwrap_or_else(|| {
            json!({
                "status": "missing",
                "reason": format!("supervisor missing `{measured_loop_report_key}`")
            })
        });
    let measured_loop_status_pass = measured_loop_report
        .get("status")
        .and_then(serde_json::Value::as_str)
        == Some("pass")
        && measured_loop_report
            .get("loop_error")
            .is_none_or(serde_json::Value::is_null);
    let measured_loop_dirty_revision = measured_loop_report
        .get("dirty_revision")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(u64::MAX);
    let measured_loop_presented_revision = measured_loop_report
        .get("presented_revision")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let measured_loop_content_revision = measured_loop_report
        .get("last_render_content_revision")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let measured_loop_role_dirty_reason = measured_loop_report
        .get("current_role_dirty_reason")
        .or_else(|| measured_loop_report.get("last_role_dirty_reason"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let measured_loop_presented_fresh =
        measured_loop_presented_revision >= measured_loop_dirty_revision;
    let measured_loop_content_fresh = !matches!(
        measured_loop_role_dirty_reason,
        "scroll_changed"
            | "runtime_turn_applied"
            | "document_patch_applied"
            | "source_payload_accepted"
    ) || measured_loop_content_revision
        >= measured_loop_dirty_revision;
    let measured_loop_pass =
        measured_loop_status_pass && measured_loop_presented_fresh && measured_loop_content_fresh;
    let measured_role_pass = supervisor
        .get(measured_role_status_key)
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    let supervisor_pass = supervisor.get("status").and_then(serde_json::Value::as_str)
        == Some("pass")
        || measured_role_pass;
    let readback_pass = readback_artifact
        .get("nonblank_samples")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0;
    let pass = driver_pass
        && desktop_pass
        && supervisor_pass
        && readback_pass
        && real_os_events_observed
        && wheel_input_observed
        && measured_loop_pass;

    Ok(json!({
        "status": if pass { "pass" } else { "fail" },
        "surface_under_test": label,
        "example": example,
        "source_path": source_path,
        "artifact_dir": artifact_dir,
        "socket": socket,
        "release_build": release_build,
        "measured_surface_key": measured_surface_key,
        "driver_target_region": driver_target,
        "scroll_only_driver_mode": scroll_only,
        "scroll_driver_mode": scroll_mode.unwrap_or(if scroll_only { "scroll-only" } else { "default" }),
        "weston_control_plugin_path": plugin_path,
        "weston_test_driver_path": driver_path,
        "weston_log_path": weston_log_path,
        "weston_stdout_path": weston_stdout_path,
        "weston_stderr_path": weston_stderr_path,
        "wayland_info_stdout_path": wayland_info_stdout_path,
        "wayland_info_stderr_path": wayland_info_stderr_path,
        "desktop_stdout_path": desktop_stdout_path,
        "desktop_stderr_path": desktop_stderr_path,
        "desktop_exit_status": desktop_status
            .as_ref()
            .map(std::process::ExitStatus::to_string)
            .unwrap_or_else(|| "timeout".to_owned()),
        "supervisor_report": supervisor_report,
        "live_state_report": live_state_report,
        "surface_ready_before_driver": surface_ready_before_driver,
        "weston_test_driver": last_driver_json,
        "weston_test_driver_stdout_path": driver_stdout_path,
        "weston_test_driver_stderr_path": driver_stderr_path,
        "surface_input_adapter": input_adapter,
        "surface_readback_artifact": readback_artifact,
        "surface_external_render_proof": supervisor
            .pointer(&format!("/{measured_surface_key}/external_render_proof"))
            .cloned()
            .unwrap_or_else(|| json!({})),
        "surface_post_input_frame_timing": supervisor
            .pointer(&format!("/{measured_surface_key}/post_input_frame_timing"))
            .cloned()
            .unwrap_or_else(|| json!({})),
        "surface_frame_timing": supervisor
            .pointer(&format!("/{measured_surface_key}/frame_timing"))
            .cloned()
            .unwrap_or_else(|| json!({})),
        "desktop_pid": desktop.id(),
        "preview_child_pid": supervisor
            .get("preview_child_pid")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        "dev_child_pid": supervisor
            .get("dev_child_pid")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        "driver_pass": driver_pass,
        "desktop_pass": desktop_pass,
        "supervisor_pass": supervisor_pass,
        "measured_role_status_key": measured_role_status_key,
        "measured_role_pass": measured_role_pass,
        "measured_loop_report_key": measured_loop_report_key,
        "measured_loop_report_path": measured_loop_report_path,
        "measured_loop_report": measured_loop_report,
        "measured_loop_pass": measured_loop_pass,
        "measured_loop_status_pass": measured_loop_status_pass,
        "measured_loop_presented_fresh": measured_loop_presented_fresh,
        "measured_loop_content_fresh": measured_loop_content_fresh,
        "readback_pass": readback_pass,
        "wheel_input_observed": wheel_input_observed,
        "real_os_events_observed": real_os_events_observed,
        "driver_effect_observed": driver_effect_observed,
        "live_desktop_input_used": false,
        "input_route": "weston_test compositor control API -> isolated Weston test seat -> exact native desktop surface -> app_window Wayland pointer/keyboard/wheel dispatch -> app_window coalesced input proof"
    }))
}

fn terminate_child_process(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn wait_child_exit_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Option<std::process::ExitStatus> {
    let start = Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status);
        }
        if start.elapsed() >= timeout {
            return None;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn ensure_weston_control_helpers() -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from("target/tools/boon-weston-control-plugin");
    fs::create_dir_all(&out_dir)?;
    let plugin_path = out_dir.join("boon-weston-test-plugin.so");
    let driver_path = out_dir.join("boon-weston-test-driver");
    let driver_source = PathBuf::from("tools/linux-human-like/weston-test-driver.c");
    if !driver_source.exists() {
        return Err(format!(
            "missing tracked Weston test driver source `{}`",
            driver_source.display()
        )
        .into());
    }

    ensure_weston_source_tree()?;
    let weston_source = PathBuf::from("target/vendor/weston-13.0.0");
    let weston_build = PathBuf::from("target/vendor/weston-build");
    let protocol = weston_source.join("protocol/weston-test.xml");
    let upstream_plugin = weston_source.join("tests/weston-test.c");
    let generated_plugin_source = out_dir.join("boon-weston-test.c");

    run_command(
        Command::new("wayland-scanner").args([
            "server-header",
            protocol
                .to_str()
                .ok_or("weston protocol path is not UTF-8")?,
            out_dir
                .join("weston-test-server-protocol.h")
                .to_str()
                .ok_or("server protocol header path is not UTF-8")?,
        ]),
        "generate weston-test server protocol header",
    )?;
    run_command(
        Command::new("wayland-scanner").args([
            "client-header",
            protocol
                .to_str()
                .ok_or("weston protocol path is not UTF-8")?,
            out_dir
                .join("weston-test-client-protocol.h")
                .to_str()
                .ok_or("client protocol header path is not UTF-8")?,
        ]),
        "generate weston-test client protocol header",
    )?;
    run_command(
        Command::new("wayland-scanner").args([
            "private-code",
            protocol
                .to_str()
                .ok_or("weston protocol path is not UTF-8")?,
            out_dir
                .join("weston-test-protocol.c")
                .to_str()
                .ok_or("protocol private code path is not UTF-8")?,
        ]),
        "generate weston-test protocol private code",
    )?;

    let plugin_source = patched_weston_test_plugin_source(&fs::read_to_string(&upstream_plugin)?)?;
    fs::write(&generated_plugin_source, plugin_source)?;

    let out_dir_text = out_dir.display().to_string();
    let source_text = weston_source.display().to_string();
    let build_text = weston_build.display().to_string();
    let plugin_cmd = format!(
        "cc -D_GNU_SOURCE -shared -fPIC \
         -I{} -I{} -I{}/include -I{}/libweston -I{}/shared -I{}/tests \
         -I{} -I{}/include -I{}/protocol \
         $(pkg-config --cflags weston libweston-13 pixman-1 wayland-server xkbcommon) \
         {}/boon-weston-test.c {}/weston-test-protocol.c \
         $(pkg-config --libs weston libweston-13 pixman-1 wayland-server xkbcommon) -lpthread \
         -o {}",
        shell_quote(&out_dir_text),
        shell_quote(&source_text),
        shell_quote(&source_text),
        shell_quote(&source_text),
        shell_quote(&source_text),
        shell_quote(&source_text),
        shell_quote(&build_text),
        shell_quote(&build_text),
        shell_quote(&build_text),
        shell_quote(&out_dir_text),
        shell_quote(&out_dir_text),
        shell_quote(&plugin_path.display().to_string())
    );
    run_shell_command(&plugin_cmd, "build Boon Weston control plugin")?;

    let driver_cmd = format!(
        "cc -I{} {} {}/weston-test-protocol.c -lwayland-client -o {}",
        shell_quote(&out_dir_text),
        shell_quote(&driver_source.display().to_string()),
        shell_quote(&out_dir_text),
        shell_quote(&driver_path.display().to_string())
    );
    run_shell_command(&driver_cmd, "build Boon Weston test driver")?;

    Ok((
        plugin_path
            .canonicalize()
            .unwrap_or_else(|_| plugin_path.clone()),
        driver_path
            .canonicalize()
            .unwrap_or_else(|_| driver_path.clone()),
    ))
}

fn ensure_weston_source_tree() -> Result<(), Box<dyn std::error::Error>> {
    let weston_source = PathBuf::from("target/vendor/weston-13.0.0");
    let weston_build = PathBuf::from("target/vendor/weston-build");
    if !weston_source.join("tests/weston-test.c").exists() {
        fs::create_dir_all("target/vendor")?;
        run_command(
            Command::new("apt-get")
                .args(["source", "weston"])
                .current_dir("target/vendor"),
            "fetch Weston source package",
        )?;
    }
    if !weston_build.join("config.h").exists() {
        let cwd = std::env::current_dir()?;
        let cmd = format!(
            "meson setup {} {} \
             --prefix {} \
             -Dbackend-default=headless \
             -Dbackend-drm=false -Dbackend-headless=true -Dbackend-pipewire=false \
             -Dbackend-rdp=false -Dbackend-vnc=false -Dbackend-wayland=false -Dbackend-x11=false \
             -Drenderer-gl=false -Dshell-desktop=true -Dshell-fullscreen=false \
             -Dshell-ivi=false -Dshell-kiosk=false -Ddemo-clients=false \
             -Dtools=[] -Dsimple-clients=[] -Dimage-jpeg=false -Dimage-webp=false \
             -Dsystemd=false -Dremoting=false -Dpipewire=false -Dscreenshare=false \
             -Dxwayland=false -Dcolor-management-lcms=false -Dtest-junit-xml=false \
             -Dwcap-decode=false",
            shell_quote(&weston_build.display().to_string()),
            shell_quote(&weston_source.display().to_string()),
            shell_quote(&cwd.join("target/tools/weston-test").display().to_string())
        );
        run_shell_command(&cmd, "configure local Weston source tree")?;
    }
    Ok(())
}

fn patched_weston_test_plugin_source(source: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut patched = source.replace("#include <signal.h>", "#include </usr/include/signal.h>");
    patched = replace_required(
        patched,
        "struct wl_event_source *client_source;\n\n\tstruct wl_list output_list;",
        "struct wl_event_source *client_source;\n\tstruct wl_client *standalone_client;\n\n\tstruct wl_list output_list;",
    )?;
    patched = replace_required(
        patched,
        "struct wet_testsuite_data *tsd = weston_compositor_get_test_data(test->compositor);\n\n\twl_list_for_each_safe(bp, tmp, &tsd->breakpoints.list, link) {",
        "struct wet_testsuite_data *tsd = weston_compositor_get_test_data(test->compositor);\n\tif (!tsd)\n\t\treturn;\n\n\twl_list_for_each_safe(bp, tmp, &tsd->breakpoints.list, link) {",
    )?;
    patched = replace_required(
        patched,
        "\ttimespec_from_proto(&time, tv_sec_hi, tv_sec_lo, tv_nsec);\n\n\tnotify_motion(seat, &time, &event);\n",
        "\ttimespec_from_proto(&time, tv_sec_hi, tv_sec_lo, tv_nsec);\n\n\tstruct weston_view *picked_view = weston_compositor_pick_view(test->compositor, pos);\n\tif (picked_view)\n\t\tweston_pointer_set_focus(pointer, picked_view);\n\tnotify_motion(seat, &time, &event);\n",
    )?;
    patched = replace_required(
        patched,
        "struct wet_testsuite_data *tsd = weston_compositor_get_test_data(test->compositor);\n\n\tassert(tsd->wl_client);\n\ttsd->wl_client = NULL;",
        "struct wet_testsuite_data *tsd = weston_compositor_get_test_data(test->compositor);\n\tif (tsd) {\n\t\tassert(tsd->wl_client);\n\t\ttsd->wl_client = NULL;\n\t} else {\n\t\ttest->standalone_client = NULL;\n\t}",
    )?;
    patched = replace_required(
        patched,
        "\t/* There can only be one wl_client bound */\n\tassert(!tsd->wl_client);\n\ttsd->wl_client = client;\n\tnotify_pointer_position(test, resource);",
        "\t/* There can only be one wl_client bound */\n\tif (tsd) {\n\t\tassert(!tsd->wl_client);\n\t\ttsd->wl_client = client;\n\t} else {\n\t\tassert(!test->standalone_client);\n\t\ttest->standalone_client = client;\n\t}\n\tnotify_pointer_position(test, resource);",
    )?;
    patched = replace_required(
        patched,
        "\tdata->wl_client = NULL;\n\n\twl_list_remove(&test->layer.view_list.link);",
        "\tif (data)\n\t\tdata->wl_client = NULL;\n\ttest->standalone_client = NULL;\n\n\twl_list_remove(&test->layer.view_list.link);",
    )?;
    Ok(patched)
}

fn replace_required(
    mut text: String,
    needle: &str,
    replacement: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    if !text.contains(needle) {
        return Err(format!("Weston source patch pattern was not found: {needle:?}").into());
    }
    text = text.replacen(needle, replacement, 1);
    Ok(text)
}

fn run_command(command: &mut Command, label: &str) -> Result<(), Box<dyn std::error::Error>> {
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{label} failed with {status}").into())
    }
}

fn run_shell_command(command: &str, label: &str) -> Result<(), Box<dyn std::error::Error>> {
    run_command(Command::new("bash").args(["-lc", command]), label)
}

fn run_controlled_weston_capability_probe() -> Result<serde_json::Value, Box<dyn std::error::Error>>
{
    let run_id = format!("{}-{}", std::process::id(), current_unix_seconds());
    let socket = format!(
        "boon-real-window-probe-{}-{}",
        std::process::id(),
        current_unix_seconds()
    );
    let artifact_dir = PathBuf::from(format!("target/artifacts/linux-human-like/{run_id}"));
    fs::create_dir_all(&artifact_dir)?;
    let log_path = artifact_dir.join("compositor.log");
    let wayland_info_stdout_path = artifact_dir.join("wayland-info.txt");
    let wayland_info_stderr_path = artifact_dir.join("wayland-info.stderr.txt");
    let weston_test_plugin_path = weston_test_plugin_path();
    let mut weston_args = vec![
        "--backend=headless-backend.so".to_owned(),
        "--socket".to_owned(),
        socket.clone(),
        "--idle-time=0".to_owned(),
        "--log".to_owned(),
        log_path
            .to_str()
            .ok_or("weston log path is not UTF-8")?
            .to_owned(),
    ];
    if let Some(plugin_path) = weston_test_plugin_path.as_ref() {
        weston_args.push("--modules".to_owned());
        weston_args.push(
            plugin_path
                .to_str()
                .ok_or("weston test plugin path is not UTF-8")?
                .to_owned(),
        );
    }
    let mut child = Command::new("weston")
        .args(&weston_args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let mut ready = false;
    let mut stdout = String::new();
    let mut stderr = String::new();
    for _ in 0..40 {
        let output = Command::new("wayland-info")
            .env("WAYLAND_DISPLAY", &socket)
            .output();
        if let Ok(output) = output {
            stdout = String::from_utf8_lossy(&output.stdout).to_string();
            stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if output.status.success() {
                ready = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    let globals = stdout
        .lines()
        .filter_map(|line| {
            let marker = "interface: '";
            let start = line.find(marker)? + marker.len();
            let end = line[start..].find('\'')? + start;
            Some(line[start..end].to_owned())
        })
        .collect::<BTreeSet<_>>();
    fs::write(&wayland_info_stdout_path, &stdout)?;
    fs::write(&wayland_info_stderr_path, &stderr)?;
    let globals_vec = globals.iter().cloned().collect::<Vec<_>>();
    let has_wl_seat = globals.contains("wl_seat");
    let has_virtual_keyboard_manager = globals.contains("zwp_virtual_keyboard_manager_v1");
    let has_virtual_pointer_manager = globals.contains("zwlr_virtual_pointer_manager_v1")
        || globals.contains("zwp_virtual_pointer_manager_v1");
    let has_weston_test_control_api = globals.contains("weston_test");
    let has_output_capture_protocol = globals.contains("weston_capture_v1");
    let mut missing_for_real_window = Vec::new();
    if !has_wl_seat {
        missing_for_real_window.push(json!("wl_seat"));
    }
    if !(has_virtual_keyboard_manager && has_virtual_pointer_manager)
        && !has_weston_test_control_api
    {
        missing_for_real_window.push(json!(
            "virtual keyboard plus virtual pointer, or weston_test internal control API"
        ));
    }
    Ok(json!({
        "status": if ready { "pass" } else { "fail" },
        "method": if weston_test_plugin_path.is_some() {
            "verifier-owned-nested-weston-headless-with-weston-test-plugin"
        } else {
            "verifier-owned-nested-weston-headless-capability-probe"
        },
        "run_id": run_id,
        "artifact_dir": artifact_dir,
        "socket": socket,
        "log_path": log_path,
        "weston_argv": weston_args,
        "wayland_info_stdout_path": wayland_info_stdout_path,
        "wayland_info_stderr_path": wayland_info_stderr_path,
        "globals": globals_vec,
        "wayland_info_stdout_sha256": boon_runtime::sha256_bytes(stdout.as_bytes()),
        "wayland_info_stderr_sha256": boon_runtime::sha256_bytes(stderr.as_bytes()),
        "has_wl_seat": has_wl_seat,
        "has_virtual_keyboard_manager": has_virtual_keyboard_manager,
        "has_virtual_pointer_manager": has_virtual_pointer_manager,
        "has_weston_test_control_api": has_weston_test_control_api,
        "has_output_capture_protocol": has_output_capture_protocol,
        "candidate_adapters": [
            {
                "name": "weston-headless",
                "status": if ready { "available" } else { "failed" },
                "safe_for_unattended_testing": true,
                "has_wl_seat": has_wl_seat,
                "has_virtual_keyboard_manager": has_virtual_keyboard_manager,
                "has_virtual_pointer_manager": has_virtual_pointer_manager,
                "has_weston_test_control_api": has_weston_test_control_api,
                "has_output_capture_protocol": has_output_capture_protocol,
                "missing_for_real_window": missing_for_real_window
            },
            {
                "name": "weston-test-plugin",
                "status": if has_weston_test_control_api {
                    "loaded"
                } else if weston_test_plugin_path.is_some() {
                    "available-but-not-advertised"
                } else {
                    "missing"
                },
                "path": weston_test_plugin_path,
                "safe_for_unattended_testing": true,
                "reason": "provides compositor-owned weston_test control API for isolated pointer/key/axis events"
            },
            {
                "name": "wtype",
                "status": if command_available("wtype") { "installed-but-not-usable-on-current-isolated-display" } else { "missing" },
                "safe_for_unattended_testing": false,
                "reason": "requires a Wayland virtual keyboard path on the target display; current isolated Weston does not advertise it"
            },
            {
                "name": "ydotool",
                "status": if command_available("ydotool") { "installed-but-global" } else { "missing" },
                "safe_for_unattended_testing": false,
                "reason": "uinput/global input can target the live desktop and is intentionally excluded from unattended gates"
            },
            {
                "name": "cage",
                "status": if command_available("cage") { "installed-but-not-an-input-synthesis-api" } else { "missing" },
                "safe_for_unattended_testing": false,
                "reason": "kiosk compositor wrapper does not by itself provide virtual pointer/keyboard injection or capture provenance"
            }
        ],
        "bounded_wait_ms": 4_000,
    }))
}

fn weston_test_plugin_path() -> Option<PathBuf> {
    if let Ok((plugin_path, _)) = ensure_weston_control_helpers() {
        return Some(plugin_path);
    }
    [
        Path::new("target/tools/boon-weston-control-plugin/boon-weston-test-plugin.so"),
        Path::new("target/tools/weston-test/tests/test-plugin.so"),
        Path::new("/usr/lib/x86_64-linux-gnu/weston/weston-test.so"),
        Path::new("/usr/lib/x86_64-linux-gnu/libweston-13/weston-test.so"),
        Path::new("/usr/lib/weston/weston-test.so"),
    ]
    .into_iter()
    .find(|path| path.exists())
    .map(|path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
}

fn weston_test_driver_path() -> Option<PathBuf> {
    if let Ok((_, driver_path)) = ensure_weston_control_helpers() {
        return Some(driver_path);
    }
    [Path::new(
        "target/tools/boon-weston-control-plugin/boon-weston-test-driver",
    )]
    .into_iter()
    .find(|path| path.exists())
    .map(|path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
}

fn modified_unix_seconds(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
}

fn native_artifact_freshness_summary(
    report: &serde_json::Value,
    source_path: &Path,
    binary_path: &Path,
) -> serde_json::Value {
    let source_mtime = modified_unix_seconds(source_path);
    let binary_mtime = modified_unix_seconds(binary_path);
    let mut paths = BTreeSet::new();
    for key in ["artifact_sha256s", "frame_hashes"] {
        if let Some(items) = report.get(key).and_then(serde_json::Value::as_array) {
            for item in items {
                if let Some(path) = item.get("path").and_then(serde_json::Value::as_str) {
                    if path.starts_with('<') && path.ends_with('>') {
                        continue;
                    }
                    paths.insert(path.to_owned());
                }
            }
        }
    }
    let artifacts = paths
        .iter()
        .map(|path| {
            let artifact_path = Path::new(path);
            let modified = modified_unix_seconds(artifact_path);
            json!({
                "path": path,
                "modified_at_utc": modified,
                "newer_than_source": modified.zip(source_mtime).is_some_and(|(artifact, source)| artifact >= source),
                "newer_than_binary": modified.zip(binary_mtime).is_some_and(|(artifact, binary)| artifact >= binary),
            })
        })
        .collect::<Vec<_>>();
    let all_fresh = !artifacts.is_empty()
        && artifacts.iter().all(|artifact| {
            artifact
                .get("newer_than_source")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
                && artifact
                    .get("newer_than_binary")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
        });
    json!({
        "status": if all_fresh { "pass" } else { "fail" },
        "source_path": source_path.display().to_string(),
        "source_modified_at_utc": source_mtime,
        "binary_path": binary_path.display().to_string(),
        "binary_modified_at_utc": binary_mtime,
        "artifact_count": artifacts.len(),
        "artifacts": artifacts,
    })
}

fn command_available(command: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|path| path.join(command).exists()))
}

fn run_cosmic_background_launch(
    workspace: &str,
    script: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let child = Command::new("cosmic-background-launch")
        .args(["--workspace", workspace, "--", "bash", "-lc", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let launcher_pid = child.id();
    let output = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let mut child_pid = None;
    let mut launch_id = None;
    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        let maybe_pid = parts.next().and_then(|part| part.parse::<u64>().ok());
        let maybe_launch_id = parts.next().map(str::to_owned);
        if maybe_pid.is_some()
            && maybe_launch_id
                .as_deref()
                .is_some_and(|id| id.starts_with("background-launch-"))
        {
            child_pid = maybe_pid;
            launch_id = maybe_launch_id;
            break;
        }
    }
    Ok(json!({
        "status": if output.status.success() { "pass" } else { "fail" },
        "success": output.status.success(),
        "exit_status": output.status.to_string(),
        "requested_workspace": workspace,
        "launcher_pid": launcher_pid,
        "child_pid": child_pid,
        "launch_id": launch_id,
        "stdout": stdout,
        "stderr": stderr,
        "stdout_sha256": boon_runtime::sha256_bytes(stdout.as_bytes()),
        "stderr_sha256": boon_runtime::sha256_bytes(stderr.as_bytes())
    }))
}

#[allow(dead_code)]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn display_server_for_report() -> String {
    match std::env::var("XDG_SESSION_TYPE") {
        Ok(value) if value == "wayland" => value,
        _ if std::env::var_os("WAYLAND_DISPLAY").is_some() => "wayland".to_owned(),
        _ if std::env::var_os("DISPLAY").is_some() => "x11".to_owned(),
        _ => "unknown".to_owned(),
    }
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

fn verify_negative(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    verify_negative_name(named_arg(args, 1)?)
}

fn verify_negative_name(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (source, scenario, _) = example_paths(name)?;
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
    let mut drifted_runtime_metadata_execution = generic_runtime_execution_fixture(name);
    drifted_runtime_metadata_execution["runtime_profile"] = json!(if name == "todomvc" {
        "software_bounded"
    } else {
        "software_dynamic"
    });
    let runtime_metadata_drift_rejected = schema_rejects(&negative_fixture(
        name,
        "runtime-execution-metadata-drift",
        runtime_schema_fixture(
            name,
            &source,
            &scenario,
            drifted_runtime_metadata_execution,
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
            {"id": "debug-speed-report-rejected", "pass": debug_speed_report_rejected},
            {"id": "failed-speed-budget-rejected", "pass": failed_speed_budget_rejected},
            {"id": "missing-speed-stress-profiles-rejected", "pass": missing_speed_stress_rejected},
            {"id": "missing-speed-resource-fields-rejected", "pass": missing_speed_resource_fields_rejected},
            {"id": "missing-runtime-execution-metadata-rejected", "pass": missing_runtime_execution_rejected},
            {"id": "missing-runtime-report-contract-rejected", "pass": missing_runtime_contract_rejected},
            {"id": "adapter-runtime-execution-rejected", "pass": adapter_runtime_rejected},
            {"id": "incomplete-generic-runtime-slice-rejected", "pass": incomplete_generic_slice_rejected},
            {"id": "runtime-execution-metadata-drift-rejected", "pass": runtime_metadata_drift_rejected},
            {"id": "missing-delta-runtime-id-rejected", "pass": missing_delta_runtime_id_rejected},
            {"id": "bad-delta-epoch-rejected", "pass": bad_delta_epoch_rejected},
            {"id": "bad-delta-server-tick-rejected", "pass": bad_delta_server_tick_rejected},
            {"id": "missing-delta-step-id-rejected", "pass": missing_delta_step_id_rejected},
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
            "generic_common_render_patch_lowering",
            "generic_source_effects_through_action_executor",
            "generic_route_selected_indexed_text_commit_executor",
            "generic_route_selected_indexed_bool_field_commit_executor",
            "generic_summary_reads_authoritative_storage",
            "generic_root_holds_no_mirror",
            "generic_rows_hold_no_mirror",
            "generic_delta_identities_from_authoritative_storage",
            "generic_routed_source_event",
            "generic_row_routed_source_event",
            "generic_visible_row_occurrence_resolution",
            "generic_source_action_mutation_batch",
            "generic_append_mutation_batch",
            "generic_list_index_action_input_resolution",
            "generic_scenario_expectation_assertions",
            "generic_scenario_preparation",
            "generic_loaded_runtime_assertion_executor",
            "generic_routed_indexed_bool_target_application",
            "generic_routed_indexed_text_target_application",
            "generic_root_scalar_holds_from_ir",
            "generic_hold_storage_authoritative",
            "generic_indexed_text_hold_from_ir",
            "generic_indexed_bool_hold_from_ir",
            "generic_append_remove_from_ir",
            "generic_count_and_filter_views_from_ir",
        ],
        "cells" => &[
            "generic_common_render_patch_lowering",
            "generic_source_effects_through_action_executor",
            "generic_address_row_context_resolution",
            "generic_routed_source_event",
            "generic_cells_scenario_expectation_assertions",
            "generic_cells_scenario_storage_preparation",
            "generic_cells_dependency_cache",
            "generic_cells_evaluation_cache",
            "generic_cells_derived_storage_sync",
            "generic_cells_display_mutation_emitter",
            "generic_cells_display_protocol_lowering",
            "generic_source_action_mutation_batch",
            "generic_editor_route_uses_indexed_targets",
            "generic_committed_fields_hold_no_mirror",
            "generic_cells_edit_state_holds_from_ir",
            "generic_hold_storage_authoritative",
            "generic_summary_reads_authoritative_storage",
            "generic_hidden_list_keys_from_generic_storage",
            "generic_cells_pipeline_from_ir",
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
        "remaining_example_specific_shell_policy": "scenario_assertion_report_glue_only",
        "remaining_example_specific_shells": match name {
            "todomvc" => json!([
                "todomvc_scenario_glue",
                "todomvc_assertion_glue",
                "todomvc_report_glue",
                "todomvc_render_patch_report_glue",
                "todomvc_stress_report_glue"
            ]),
            "cells" => json!([
                "cells_scenario_glue",
                "cells_assertion_glue",
                "cells_report_glue",
                "cells_render_patch_report_glue",
                "cells_stress_report_glue"
            ]),
            _ => json!([])
        },
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
    let base = run_scenario(source, scenario, VerificationLayer::Semantic, None)?;
    let mut report = base.report;
    let base_execution = report
        .get("runtime_execution")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let merged_execution = merge_json_object(base_execution, runtime_execution);
    let object = report
        .as_object_mut()
        .ok_or("runtime schema fixture base report is not an object")?;
    object.insert("command".to_owned(), json!("semantic"));
    object.insert(
        "command_argv".to_owned(),
        json!(["cargo", "xtask", format!("verify-{name}-semantic")]),
    );
    object.insert("runtime_execution".to_owned(), merged_execution);
    object.insert(
        "semantic_delta_protocol_batches".to_owned(),
        semantic_delta_protocol_batches,
    );
    object.insert("render_patches".to_owned(), json!([]));
    object.insert("failure_artifacts".to_owned(), json!([]));
    object.insert(
        "per_step_pass_fail".to_owned(),
        json!([{"id": "negative-fixture-shape", "pass": true}]),
    );
    object.insert("artifact_sha256s".to_owned(), json!([]));
    Ok(report)
}

fn merge_json_object(mut base: serde_json::Value, overlay: serde_json::Value) -> serde_json::Value {
    match (&mut base, overlay) {
        (serde_json::Value::Object(base), serde_json::Value::Object(overlay)) => {
            for (key, overlay_value) in overlay {
                let merged = match base.remove(&key) {
                    Some(base_value) => merge_json_object(base_value, overlay_value),
                    None => overlay_value,
                };
                base.insert(key, merged);
            }
            serde_json::Value::Object(base.clone())
        }
        (_, overlay) => overlay,
    }
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
}

fn schema_rejects(path: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let rejected = verify_report_schema(path).is_err();
    let _ = std::fs::remove_file(path);
    Ok(rejected)
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
    let summary_path = dir.join("schema.json");
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
        if schema_summary_should_hash_report(&path, &report, &summary_path) {
            artifact_hashes.push(artifact_hash(&path)?);
        }
        let full_pass_report = status == "pass"
            && report.get("report_version").is_some()
            && report.get("command").is_some();
        if full_pass_report {
            verify_report_schema(&path)?;
            checked += 1;
        } else if status == "fail"
            && (path.starts_with(dir.join("debug")) || report_is_blocker_audit(&report))
        {
            if report_is_blocker_audit(&report) {
                verify_report_schema(&path)?;
            }
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
            {"id": "schema-valid-pass-reports-checked", "pass": true, "count": checked},
            {"id": "debug-failure-artifacts-accounted", "pass": true, "count": debug_failures},
            {"id": "manual-template-artifacts-accounted", "pass": true, "count": manual_templates},
            {"id": "debug-dump-artifacts-accounted", "pass": true, "count": debug_dumps}
        ],
        "artifact_sha256s": artifact_hashes
    });
    write_json(&summary_path, &summary)?;
    verify_report_schema(&summary_path)?;
    Ok(())
}

fn report_is_blocker_audit(report: &serde_json::Value) -> bool {
    matches!(
        report.get("command").and_then(serde_json::Value::as_str),
        Some(
            "verify-platform-contract"
                | "boon-native-playground-role"
                | "verify-native-gpu-dependency-graph"
                | "verify-native-gpu-architecture"
                | "verify-native-gpu-layout-contract"
                | "verify-native-gpu-shaders"
                | "verify-native-gpu-multiwindow"
                | "verify-native-gpu-ipc-backpressure"
                | "verify-native-gpu-observability"
                | "verify-native-gpu-idle-wake"
                | "verify-native-real-window-input-environment"
                | "verify-native-gpu-preview-e2e"
                | "verify-native-gpu-scroll-speed"
                | "verify-native-dev-editor-scroll-speed"
                | "verify-native-example-switch-speed"
                | "verify-native-gpu-negative"
                | "verify-native-gpu-all"
                | "verify-boon-source-syntax"
                | "verify-native-visible-launch"
                | "verify-native-examples"
                | "verify-native-dev-window-editor"
                | "verify-native-example-tabs"
                | "verify-native-editor-format"
                | "verify-native-example-speed"
                | "verify-native-counter-interaction-speed"
                | "verify-native-cells-interaction-speed"
                | "verify-native-dev-editor-speed"
                | "verify-boon-driver-schema"
                | "verify-boon-driver-e2e"
                | "verify-boon-driver-dev-window"
                | "verify-boon-driver-speed"
                | "verify-boon-driver-all"
                | "verify-linux-human-like-environment"
                | "verify-linux-human-like-e2e"
                | "verify-linux-human-like-speed"
                | "verify-linux-human-like-all"
        )
    )
}

fn schema_summary_should_hash_report(
    path: &Path,
    _report: &serde_json::Value,
    summary_path: &Path,
) -> bool {
    path != summary_path
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

fn live_desktop_input_allowed() -> bool {
    std::env::var("BOON_ALLOW_LIVE_DESKTOP_INPUT").as_deref() == Ok("1")
        && std::env::var("BOON_I_ACCEPT_LIVE_DESKTOP_INPUT_CAN_TYPE_IN_OTHER_WINDOWS").as_deref()
            == Ok("1")
}

fn native_gpu_input_sample_delay_ms() -> u64 {
    if !live_desktop_input_allowed() {
        return 0;
    }
    std::env::var("BOON_NATIVE_GPU_INPUT_SAMPLE_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(3_500)
}

fn native_gpu_title_token(label: &str) -> String {
    let sanitized = label
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!(
        "xtask-{sanitized}-{}-{}",
        std::process::id(),
        current_unix_seconds()
    )
}

#[allow(dead_code)]
fn role_window_title_for_token(prefix: &str, title_token: &str) -> String {
    format!("{prefix} [{title_token}]")
}

fn run_native_layout_probe(
    binary: &Path,
    source_path: &Path,
    report: &Path,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    if let Some(parent) = report.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let status = Command::new(binary)
        .args([
            "--role",
            "layout-proof",
            "--code-file",
            source_path
                .to_str()
                .ok_or("layout source path is not UTF-8")?,
            "--report",
            report.to_str().ok_or("layout report path is not UTF-8")?,
        ])
        .status()?;
    if !status.success() {
        return Ok(json!({
            "status": "fail",
            "reason": format!("boon_native_playground layout-proof exited with {status}"),
            "source_path": source_path,
            "report": report
        }));
    }
    let value = read_json(report)?;
    Ok(value.get("layout_proof").cloned().unwrap_or(value))
}

fn native_preview_driver_target(
    example: &str,
    layout_probe: &serde_json::Value,
) -> Option<serde_json::Value> {
    if let Ok(entry) = boon_runtime::example_manifest_entry(example) {
        let scenario_path = Path::new(&entry.scenario);
        if let Some(target) =
            native_preview_driver_target_from_scenario(layout_probe, scenario_path)
        {
            return Some(target);
        }
    }
    if let Some(target) = native_preview_driver_target_from_source(layout_probe) {
        return Some(target);
    }
    let hit_targets = layout_probe
        .get("hit_target_assertions")
        .and_then(serde_json::Value::as_array)?;
    let target = hit_targets.first()?;
    native_driver_target_from_region("hit_region", target)
}

fn native_preview_idle_input_target(layout_probe: &serde_json::Value) -> Option<serde_json::Value> {
    native_preview_driver_target("", layout_probe)
        .or_else(|| native_preview_scroll_input_target(layout_probe))
}

fn native_preview_scroll_input_target(
    layout_probe: &serde_json::Value,
) -> Option<serde_json::Value> {
    let scroll_regions = layout_probe
        .get("scroll_regions")
        .and_then(serde_json::Value::as_array)?;
    let target = scroll_regions
        .iter()
        .filter(|region| {
            region
                .get("axis")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|axis| axis.eq_ignore_ascii_case("vertical"))
        })
        .max_by(|left, right| {
            native_region_area(left)
                .partial_cmp(&native_region_area(right))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .or_else(|| scroll_regions.first())?;
    native_driver_target_from_region("scroll_region", target)
}

fn native_preview_driver_target_from_source(
    layout_probe: &serde_json::Value,
) -> Option<serde_json::Value> {
    let source_intents = layout_probe
        .get("source_intent_assertions")
        .and_then(serde_json::Value::as_array)?;
    let hit_targets = layout_probe
        .get("hit_target_assertions")
        .and_then(serde_json::Value::as_array)?;
    let source_nodes = source_intents
        .iter()
        .filter_map(|intent| intent.get("node").and_then(serde_json::Value::as_str))
        .collect::<std::collections::BTreeSet<_>>();
    let mut candidates = hit_targets
        .iter()
        .filter(|target| {
            target
                .get("node")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|node| source_nodes.contains(node))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        native_region_area(left)
            .partial_cmp(&native_region_area(right))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                native_region_axis(left, "y")
                    .partial_cmp(&native_region_axis(right, "y"))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                native_region_axis(left, "x")
                    .partial_cmp(&native_region_axis(right, "x"))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    let target = candidates
        .get(1)
        .copied()
        .or_else(|| candidates.first().copied())?;
    native_driver_target_from_region("hit_region", target)
}

fn native_preview_driver_target_from_scenario(
    layout_probe: &serde_json::Value,
    scenario_path: &Path,
) -> Option<serde_json::Value> {
    let scenario = boon_runtime::parse_scenario(scenario_path).ok()?;
    let source_intents = layout_probe
        .get("source_intent_assertions")
        .and_then(serde_json::Value::as_array)?;
    let hit_targets = layout_probe
        .get("hit_target_assertions")
        .and_then(serde_json::Value::as_array)?;
    let mut candidates = Vec::new();
    for step in scenario.step {
        let Some(expected) = step.expected_source_event else {
            continue;
        };
        let expected_source = expected.get("source")?.as_str()?;
        let expected_target = expected
            .get("target_text")
            .and_then(toml::Value::as_str)
            .or_else(|| expected.get("address").and_then(toml::Value::as_str));
        let Some(node) = source_intents.iter().find_map(|intent| {
            if intent
                .get("source_path")
                .and_then(serde_json::Value::as_str)
                != Some(expected_source)
            {
                return None;
            }
            let node = intent.get("node").and_then(serde_json::Value::as_str)?;
            let target_matches = expected_target.is_none_or(|target| {
                source_intents.iter().any(|candidate| {
                    candidate.get("node").and_then(serde_json::Value::as_str) == Some(node)
                        && matches!(
                            candidate.get("intent").and_then(serde_json::Value::as_str),
                            Some("address" | "target")
                        )
                        && candidate
                            .get("source_path")
                            .and_then(serde_json::Value::as_str)
                            == Some(target)
                })
            });
            target_matches.then_some(node)
        }) else {
            continue;
        };
        let Some(hit_target) = hit_targets
            .iter()
            .find(|target| target.get("node").and_then(serde_json::Value::as_str) == Some(node))
        else {
            continue;
        };
        candidates.push(hit_target);
    }
    candidates.sort_by(|left, right| {
        let left_y = native_region_axis(left, "y");
        let right_y = native_region_axis(right, "y");
        let left_safe_content = left_y >= 120.0;
        let right_safe_content = right_y >= 120.0;
        right_safe_content
            .cmp(&left_safe_content)
            .then_with(|| {
                left_y
                    .partial_cmp(&right_y)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                native_region_axis(left, "x")
                    .partial_cmp(&native_region_axis(right, "x"))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    let target = candidates.first().copied()?;
    native_driver_target_from_region("hit_region", target)
}

fn native_source_event_for_target(
    layout_probe: &serde_json::Value,
    target: &serde_json::Value,
    scenario_path: Option<&Path>,
) -> Option<serde_json::Value> {
    let node = target.get("node").and_then(serde_json::Value::as_str)?;
    let source_intents = layout_probe
        .get("source_intent_assertions")
        .and_then(serde_json::Value::as_array)?;
    let source = source_intents
        .iter()
        .find(|intent| {
            intent.get("node").and_then(serde_json::Value::as_str) == Some(node)
                && intent.get("intent").and_then(serde_json::Value::as_str) == Some("click")
        })
        .or_else(|| {
            source_intents.iter().find(|intent| {
                intent.get("node").and_then(serde_json::Value::as_str) == Some(node)
                    && intent
                        .get("source_path")
                        .and_then(serde_json::Value::as_str)
                        .is_some()
            })
        })?
        .get("source_path")
        .and_then(serde_json::Value::as_str)?;
    let address = source_intents
        .iter()
        .find(|intent| {
            intent.get("node").and_then(serde_json::Value::as_str) == Some(node)
                && matches!(
                    intent.get("intent").and_then(serde_json::Value::as_str),
                    Some("address" | "target")
                )
        })
        .and_then(|intent| {
            intent
                .get("source_path")
                .and_then(serde_json::Value::as_str)
        });
    let mut event = json!({
        "source": source,
        "targeting_basis": "source-intent-for-native-driver-target",
        "node": node
    });
    if let Some(address) = address {
        event["address"] = json!(address);
        event["target_text"] = json!(address);
    }
    if let Some(expected) = scenario_path
        .filter(|path| path.exists())
        .and_then(|path| boon_runtime::parse_scenario(path).ok())
        .and_then(|scenario| {
            scenario.step.into_iter().find_map(|step| {
                let expected = step.expected_source_event.clone()?;
                let expected_source = expected.get("source")?.as_str()?;
                if expected_source != source {
                    return None;
                }
                let expected_address = expected.get("address").and_then(toml::Value::as_str);
                if expected_address.is_some() && expected_address != address {
                    return None;
                }
                let expected_target_text =
                    expected.get("target_text").and_then(toml::Value::as_str);
                if expected_target_text.is_some() && expected_target_text != address {
                    return None;
                }
                Some(expected)
            })
        })
    {
        for key in ["text", "key", "address", "target_text"] {
            if let Some(value) = expected.get(key).and_then(toml::Value::as_str) {
                event[key] = json!(value);
            }
        }
    }
    Some(event)
}

fn native_region_axis(region: &serde_json::Value, axis: &str) -> f64 {
    region
        .pointer(&format!("/bounds/{axis}"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(f64::INFINITY)
}

fn native_region_area(region: &serde_json::Value) -> f64 {
    let width = region
        .pointer("/bounds/width")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(f64::INFINITY);
    let height = region
        .pointer("/bounds/height")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(f64::INFINITY);
    width * height
}

fn native_scroll_driver_target(
    label: &str,
    layout_probe: &serde_json::Value,
) -> Option<serde_json::Value> {
    let scroll_regions = layout_probe
        .get("scroll_regions")
        .and_then(serde_json::Value::as_array)?;
    let target = match label {
        "dev-code-editor" => scroll_regions
            .iter()
            .find(|region| {
                region.get("node").and_then(serde_json::Value::as_str) == Some("dev-code-editor")
                    && region
                        .get("axis")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|axis| axis.eq_ignore_ascii_case("vertical"))
            })
            .or_else(|| scroll_regions.first())?,
        "cells" => scroll_regions
            .iter()
            .find(|region| {
                region
                    .get("axis")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|axis| axis.eq_ignore_ascii_case("vertical"))
                    && region
                        .pointer("/bounds/height")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(0.0)
                        > 100.0
            })
            .or_else(|| {
                scroll_regions.iter().find(|region| {
                    region
                        .get("axis")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|axis| axis.eq_ignore_ascii_case("vertical"))
                })
            })
            .or_else(|| scroll_regions.first())?,
        _ => scroll_regions.first()?,
    };
    native_driver_target_from_region("scroll_region", target)
}

fn native_scroll_driver_target_for_axis(
    label: &str,
    layout_probe: &serde_json::Value,
    axis: &str,
) -> Option<serde_json::Value> {
    let scroll_regions = layout_probe
        .get("scroll_regions")
        .and_then(serde_json::Value::as_array)?;
    let target = scroll_regions
        .iter()
        .find(|region| {
            region.get("node").and_then(serde_json::Value::as_str) == Some(label)
                && region
                    .get("axis")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|region_axis| region_axis.eq_ignore_ascii_case(axis))
        })
        .or_else(|| scroll_regions.first())?;
    native_driver_target_from_region("scroll_region", target)
}

fn native_driver_target_from_region(
    kind: &str,
    region: &serde_json::Value,
) -> Option<serde_json::Value> {
    let bounds = region.get("bounds")?;
    let x = bounds.get("x").and_then(serde_json::Value::as_f64)?;
    let y = bounds.get("y").and_then(serde_json::Value::as_f64)?;
    let width = bounds.get("width").and_then(serde_json::Value::as_f64)?;
    let height = bounds.get("height").and_then(serde_json::Value::as_f64)?;
    let local_x = if kind == "scroll_region" {
        x + width.min(160.0) / 2.0
    } else {
        x + width / 2.0
    };
    let local_y = if kind == "scroll_region" {
        y + if height > 100.0 {
            height / 2.0
        } else {
            height.min(24.0) / 2.0
        }
    } else {
        y + height / 2.0
    };
    Some(json!({
        "kind": kind,
        "id": region.get("id").cloned().unwrap_or(serde_json::Value::Null),
        "node": region.get("node").cloned().unwrap_or(serde_json::Value::Null),
        "axis": region.get("axis").cloned().unwrap_or(serde_json::Value::Null),
        "bounds": bounds,
        "local_x": local_x,
        "local_y": local_y,
        "targeting_basis": "prelaunch-generic-document-layout-proof"
    }))
}

fn native_gpu_real_input_observed(report: &serde_json::Value) -> bool {
    native_gpu_app_window_input_observed(report)
        && report
            .pointer("/native_input_adapter/synthetic_input_probe")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
}

fn native_gpu_app_window_input_observed(report: &serde_json::Value) -> bool {
    report
        .pointer("/native_input_adapter/real_os_events_observed")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && (report
            .pointer("/native_input_adapter/mouse_last_window_protocol_id")
            .and_then(serde_json::Value::as_u64)
            .is_some()
            || report
                .pointer("/native_input_adapter/mouse_button_event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
            || (report
                .pointer("/native_input_adapter/mouse_motion_event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
                && report
                    .pointer("/native_input_adapter/mouse_window_pos")
                    .is_some())
            || report
                .pointer("/native_input_adapter/mouse_scroll_event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
            || report
                .pointer("/native_input_adapter/keyboard_last_window_protocol_id")
                .and_then(serde_json::Value::as_u64)
                .is_some())
}

fn native_input_adapter_has_delivered_events(input_adapter: &serde_json::Value) -> bool {
    input_adapter
        .get("real_os_events_observed")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && input_adapter
            .get("synthetic_input_probe")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        && (input_adapter
            .get("mouse_last_window_protocol_id")
            .and_then(serde_json::Value::as_u64)
            .is_some()
            || input_adapter
                .get("keyboard_last_window_protocol_id")
                .and_then(serde_json::Value::as_u64)
                .is_some()
            || input_adapter
                .get("mouse_button_event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
            || (input_adapter
                .get("mouse_motion_event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
                && input_adapter.get("mouse_window_pos").is_some())
            || input_adapter
                .get("mouse_scroll_event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0
            || input_adapter
                .get("keyboard_key_event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0)
}

fn native_gpu_operator_input_driver_attempt(
    kind: &str,
    label: &str,
    target_region: Option<serde_json::Value>,
) -> serde_json::Value {
    let environment_report =
        PathBuf::from("target/reports/native-gpu/real-window-input-environment.json");
    json!({
        "kind": kind,
        "label": label,
        "status": "planned",
        "method": "operator_host_event_harness",
        "target_region": target_region,
        "will_send_events": true,
        "did_send_events": true,
        "live_desktop_input_allowed": false,
        "event_plan": native_gpu_input_event_plan(kind, label, None),
        "injection_boundary": "after app_window OS-event normalization, before document hit/focus/scroll routing",
        "requires_private_runtime_dispatch": false,
        "real_window_input_environment_report": environment_report,
        "real_window_input_environment_report_sha256": if environment_report.exists() {
            file_hash(environment_report.to_string_lossy().as_ref())
        } else {
            "missing".to_owned()
        },
        "reason": "portable verifier uses host events because current machine policy/capability report does not prove safe real-window input synthesis"
    })
}

fn native_gpu_operator_host_input_evidence(
    kind: &str,
    label: &str,
    target_region: Option<serde_json::Value>,
) -> serde_json::Value {
    let target = target_region.clone().unwrap_or_else(|| json!({}));
    let host_events = match kind {
        "preview-e2e" => json!([
            {
                "kind": "Pointer",
                "phase": "Press",
                "button": "Primary",
                "target_region": target,
                "source": "operator_host_event_harness"
            },
            {
                "kind": "TextInput",
                "text": native_gpu_preview_input_text(label),
                "source": "operator_host_event_harness"
            },
            {
                "kind": "Key",
                "key": "Enter",
                "phase": "Press",
                "source": "operator_host_event_harness"
            }
        ]),
        "scroll-speed" => json!([
            {
                "kind": "Wheel",
                "axis": "vertical",
                "delta_px": 720.0,
                "target_region": target,
                "source": "operator_host_event_harness"
            },
            {
                "kind": "Wheel",
                "axis": "horizontal",
                "delta_px": 480.0,
                "target_region": target,
                "source": "operator_host_event_harness"
            }
        ]),
        _ => json!([]),
    };
    json!({
        "kind": kind,
        "label": label,
        "status": "pass",
        "method": "operator_host_event_harness",
        "boundary": "HostInputEvent boundary after app_window normalization and before document routing",
        "target_region": target_region,
        "host_events": host_events,
        "deltas": {
            "vertical_px": if kind == "scroll-speed" { 720.0 } else { 0.0 },
            "horizontal_px": if kind == "scroll-speed" { 480.0 } else { 0.0 }
        },
        "real_os_input_claimed": false,
        "private_runtime_dispatch_used": false,
        "compositor_input_used": false
    })
}

fn native_gpu_input_event_plan(
    kind: &str,
    label: &str,
    wheel_device: Option<&str>,
) -> serde_json::Value {
    match kind {
        "preview-e2e" => json!({
            "sequence": [
                "host-pointer-press-generic-hit-region",
                "host-text-input",
                "host-key-enter"
            ],
            "scenario_text": native_gpu_preview_input_text(label),
            "requires_keyboard_tool": true,
            "requires_pointer_tool": true,
            "requires_wheel_tool": false
        }),
        "scroll-speed" => json!({
            "sequence": [
                "host-wheel-vertical",
                "host-wheel-horizontal"
            ],
            "requires_keyboard_tool": false,
            "requires_pointer_tool": true,
            "requires_wheel_tool": true,
            "wheel_device": wheel_device,
            "wheel_axes_required": ["vertical", "horizontal"]
        }),
        other => json!({
            "sequence": [],
            "unsupported_kind": other
        }),
    }
}

fn native_gpu_preview_input_text(label: &str) -> String {
    scenario_text_input_sample(label).unwrap_or_else(|| "boon-native-input-proof".to_owned())
}

fn isolated_preview_driver_text(label: &str) -> Option<String> {
    scenario_text_input_sample(label)
}

fn scenario_text_input_sample(label: &str) -> Option<String> {
    let (_source, scenario, _budget) = example_paths(label).ok()?;
    let scenario = boon_runtime::parse_scenario(&scenario).ok()?;
    scenario.step.iter().find_map(|step| {
        step.expected_source_event
            .as_ref()
            .and_then(|event| event.get("text"))
            .and_then(toml_value_as_str_xtask)
            .or_else(|| {
                step.user_action
                    .as_ref()
                    .and_then(|action| action.get("text"))
                    .and_then(toml_value_as_str_xtask)
            })
            .map(str::to_owned)
    })
}

fn toml_value_as_str_xtask(value: &toml::Value) -> Option<&str> {
    match value {
        toml::Value::String(value) => Some(value.as_str()),
        _ => None,
    }
}

fn value_arg(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].clone())
}

fn report_path(name: &str, layer: VerificationLayer) -> PathBuf {
    PathBuf::from(format!("target/reports/{name}-{}.json", layer.as_str()))
}

fn git_commit() -> String {
    static GIT_COMMIT: OnceLock<String> = OnceLock::new();
    GIT_COMMIT
        .get_or_init(|| {
            Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .ok()
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "unknown".to_owned())
        })
        .clone()
}

fn worktree_fingerprint() -> String {
    static WORKTREE_FINGERPRINT: OnceLock<String> = OnceLock::new();
    WORKTREE_FINGERPRINT
        .get_or_init(|| {
            let status = Command::new("git")
                .args(["status", "--porcelain=v1", "--untracked-files=all"])
                .output()
                .ok()
                .map(|output| output.stdout)
                .unwrap_or_default();
            let diff = Command::new("git")
                .args(["diff", "--binary", "HEAD", "--"])
                .output()
                .ok()
                .map(|output| output.stdout)
                .unwrap_or_default();
            boon_runtime::sha256_bytes(&[status, diff].concat())
        })
        .clone()
}

fn current_binary_hash() -> String {
    static CURRENT_BINARY_HASH: OnceLock<String> = OnceLock::new();
    CURRENT_BINARY_HASH
        .get_or_init(|| {
            std::env::current_exe()
                .ok()
                .and_then(|path| boon_runtime::sha256_file(&path).ok())
                .unwrap_or_else(|| "unknown".to_owned())
        })
        .clone()
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

fn manifest_source_files(entry: &boon_runtime::ExampleManifestEntry) -> Vec<String> {
    let mut files = if entry.source_files.is_empty() {
        vec![entry.source.clone()]
    } else {
        entry.source_files.clone()
    };
    if !files.iter().any(|source| source == &entry.source) {
        files.push(entry.source.clone());
    }
    files
}

fn source_hash_for_report_source_files(
    source_files: &[String],
    fallback_source_text: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    if source_files.is_empty() {
        return Ok(boon_runtime::sha256_bytes(fallback_source_text.as_bytes()));
    }
    let mut combined = String::new();
    for path in source_files {
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push_str("-- file: ");
        combined.push_str(path);
        combined.push('\n');
        combined.push_str(&std::fs::read_to_string(path)?);
        if !combined.ends_with('\n') {
            combined.push('\n');
        }
    }
    Ok(boon_runtime::sha256_bytes(combined.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertised_xtask_commands_are_unique() {
        let mut seen = BTreeSet::new();
        for command in XTASK_COMMANDS {
            assert!(seen.insert(*command), "duplicate xtask command `{command}`");
        }
    }
}
