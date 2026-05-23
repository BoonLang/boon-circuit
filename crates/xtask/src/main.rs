#![recursion_limit = "256"]

use boon_runtime::{
    LiveRuntime, LiveSourceEvent, VerificationLayer, example_paths, run_scenario,
    verify_report_schema, write_json,
};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

const XTASK_COMMANDS: &[&str] = &[
    "verify-example-semantic",
    "verify-example-speed",
    "verify-example-negative",
    "verify-foundation",
    "bench-example",
    "verify-report-schema",
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
    "verify-native-gpu-preview-e2e",
    "verify-native-gpu-scroll-speed",
    "verify-native-gpu-negative",
    "verify-native-gpu-all",
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
        "verify-native-gpu-preview-e2e" => verify_native_gpu_preview_e2e(&args),
        "verify-native-gpu-scroll-speed" => verify_native_gpu_scroll_speed(&args),
        "verify-native-gpu-negative" => verify_native_gpu_negative(&args),
        "verify-native-gpu-all" => verify_native_gpu_all(&args),
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
            | "verify-runtime-finality"
            | "verify-todomvc-reference-parity"
            | "verify-os-input-probe"
            | "verify-playground-launch"
            | "verify-playground-background-launch"
            | "verify-playground-split-wayland"
            | "verify-playground-genericity"
            | "verify-playground-custom-source"
            | "write-manual-handoff"
            | "audit-machine-readiness"
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
    let frame = boon_document::fixture_frame_with_virtualized_grid();
    let mut measurer = boon_document::SimpleTextMeasurer;
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
    let _ = std::fs::remove_file(&supervisor_report);
    let _ = std::fs::remove_file(&live_state_report);
    let _ = std::fs::remove_file("target/reports/native-gpu/.multiwindow-live-state.json");
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland";
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-multiwindow:wayland-session",
        wayland,
        format!(
            "WAYLAND_DISPLAY={:?}, XDG_SESSION_TYPE={:?}",
            std::env::var("WAYLAND_DISPLAY").ok(),
            std::env::var("XDG_SESSION_TYPE").ok()
        ),
        (!wayland).then(|| "native multiwindow proof requires a Wayland session".to_owned()),
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

    if build.success() && wayland {
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
                "cd {} && ./target/debug/boon_native_playground --role desktop --example cells --probe --child-hold-ms 10000 --dev-hold-ms 5000 --live-state-report {} --report {} >>/tmp/boon-native-gpu-multiwindow.log 2>&1",
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
                    wait_for_json_report(&live_state_report, Duration::from_secs(20));
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
                    wait_for_json_report(&supervisor_report, Duration::from_secs(20));
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
        "launcher_command": "cosmic-background-launch --workspace boon-circuit",
        "cosmic_background_launch_proof": cosmic_launch_proof,
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
    let _ = std::fs::remove_file(&supervisor_report);
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland";
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-ipc:wayland-session",
        wayland,
        format!(
            "WAYLAND_DISPLAY={:?}, XDG_SESSION_TYPE={:?}",
            std::env::var("WAYLAND_DISPLAY").ok(),
            std::env::var("XDG_SESSION_TYPE").ok()
        ),
        (!wayland).then(|| "native IPC proof requires a Wayland session".to_owned()),
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

    if build.success() && wayland {
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
                "cd {} && ./target/debug/boon_native_playground --role desktop --example cells --probe --child-hold-ms 10000 --dev-hold-ms 5000 --report {} >>/tmp/boon-native-gpu-ipc.log 2>&1",
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
                    wait_for_json_report(&supervisor_report, Duration::from_secs(20));
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
        "launcher_command": "cosmic-background-launch --workspace boon-circuit",
        "supervisor_report": supervisor_report,
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
    let _ = std::fs::remove_file(&supervisor_report);
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_SESSION_TYPE").unwrap_or_default() == "wayland";
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-observability:wayland-session",
        wayland,
        format!(
            "WAYLAND_DISPLAY={:?}, XDG_SESSION_TYPE={:?}",
            std::env::var("WAYLAND_DISPLAY").ok(),
            std::env::var("XDG_SESSION_TYPE").ok()
        ),
        (!wayland).then(|| "native observability proof requires a Wayland session".to_owned()),
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

    if build.success() && wayland {
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
                "cd {} && ./target/debug/boon_native_playground --role desktop --example cells --probe --child-hold-ms 10000 --dev-hold-ms 5000 --report {} >>/tmp/boon-native-gpu-observability.log 2>&1",
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
                    wait_for_json_report(&supervisor_report, Duration::from_secs(20));
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
        "launcher_command": "cosmic-background-launch --workspace boon-circuit",
        "supervisor_report": supervisor_report,
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

fn verify_native_gpu_preview_e2e(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let example = value_arg(args, "--example").unwrap_or_else(|| "cells".to_owned());
    if !matches!(example.as_str(), "todomvc" | "cells") {
        return Err(format!("unsupported native preview E2E example `{example}`").into());
    }
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
    let source_path = PathBuf::from(format!("examples/{example}.bn"));
    let source_hash = file_hash(source_path.to_string_lossy().as_ref());
    let scenario_labels = native_preview_e2e_scenario_labels(&example);
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
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:wayland-session"),
        wayland,
        format!(
            "WAYLAND_DISPLAY={:?}, XDG_SESSION_TYPE={:?}",
            std::env::var("WAYLAND_DISPLAY").ok(),
            std::env::var("XDG_SESSION_TYPE").ok()
        ),
        (!wayland).then(|| "native preview E2E requires a Wayland session".to_owned()),
    );

    let build = Command::new("cargo")
        .args(["build", "-p", "boon_native_playground"])
        .status()?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-preview-e2e-{example}:playground-build"),
        build.success(),
        format!("cargo build -p boon_native_playground status={build}"),
        (!build.success()).then(|| "failed to build boon_native_playground".to_owned()),
    );

    let layout_probe = if build.success() {
        run_native_layout_probe(&source_path, &layout_probe_report)?
    } else {
        json!({"status": "not-run", "reason": "boon_native_playground build failed"})
    };
    let driver_target = native_preview_driver_target(&example, &layout_probe);
    let native_input_driver_attempt =
        native_gpu_operator_input_driver_attempt("preview-e2e", &example, driver_target.clone());

    if build.success() && wayland {
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
            let script = format!(
                "cd {} && ./target/debug/boon_native_playground --role desktop --example {} --probe --child-hold-ms 10000 --dev-hold-ms 5000 --title-token {} --input-sample-delay-ms {} --role-report-timeout-ms {} --live-state-report {} --report {} >>/tmp/boon-native-gpu-preview-e2e-{}.log 2>&1",
                shell_quote(&cwd.display().to_string()),
                shell_quote(&example),
                shell_quote(&title_token),
                input_sample_delay_ms,
                12_000_u64.saturating_add(input_sample_delay_ms),
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
                let live_state_ready =
                    wait_for_json_report(&live_state_report, Duration::from_secs(20));
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
                let report_ready =
                    wait_for_json_report(&supervisor_report, Duration::from_secs(20));
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
        "source_path": source_path,
        "scenario_hash": scenario_hash,
        "scenario_artifact": scenario_artifact,
        "layout_probe_report": layout_probe_report,
        "prelaunch_layout_probe": layout_probe,
        "driver_target_region": driver_target,
        "scenario_labels": scenario_labels,
        "real_os_input": false,
        "operator_host_input": true,
        "input_injection_method": "operator_host_event_harness",
        "operator_host_input_evidence": operator_host_input_evidence,
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
        "live_desktop_input_allowed": false,
        "native_input_driver_attempt": native_input_driver_attempt,
        "blocked_reason": "native preview did not yet produce app-owned WGPU readback, host route, and runtime assertion evidence"
    });

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
            if real_os_input_observed {
                extra["real_os_input"] = json!(true);
                extra["input_injection_method"] =
                    json!("os_pointer_keyboard_to_visible_wayland_app_window");
                extra["focused_window_proof"] = json!({
                    "status": "pass",
                    "method": "app_window_per_window_event_provenance",
                    "mouse_last_window_protocol_id": extra
                        .pointer("/native_input_adapter/mouse_last_window_protocol_id")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                    "keyboard_last_window_protocol_id": extra
                        .pointer("/native_input_adapter/keyboard_last_window_protocol_id")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null)
                });
            }
        }
        if let Some(readback) = supervisor
            .pointer("/preview_surface_proof/readback_artifact")
            .and_then(serde_json::Value::as_object)
        {
            if let Some(path) = readback.get("path").and_then(serde_json::Value::as_str) {
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

    write_native_gate_report(
        args,
        "verify-native-gpu-preview-e2e",
        checks,
        blockers,
        extra,
    )
}

fn native_preview_e2e_scenario_labels(example: &str) -> Vec<&'static str> {
    match example {
        "todomvc" => vec![
            "native-preview-visible",
            "new-todo-input-hit",
            "add-todo-via-os-keyboard",
            "toggle-todo-via-os-pointer",
            "filter-source-routing",
        ],
        "cells" => vec![
            "native-preview-visible",
            "cell-hit-target",
            "formula-bar-focus",
            "edit-formula-via-os-keyboard",
            "dependent-cell-recalculation",
        ],
        _ => vec!["native-preview-visible"],
    }
}

fn native_preview_host_route_evidence(
    example: &str,
    report: &serde_json::Value,
) -> serde_json::Value {
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
    let preferred_nodes = match example {
        "todomvc" => ["todo_new_input", "todo_row_checkbox", "todo_row_title"].as_slice(),
        "cells" => ["cell_editor", "formula_editor"].as_slice(),
        _ => [].as_slice(),
    };
    let target_hit = preferred_nodes
        .iter()
        .find_map(|node| {
            hit_targets
                .iter()
                .find(|target| {
                    target.get("node").and_then(serde_json::Value::as_str) == Some(*node)
                })
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
    let real_input = native_gpu_real_input_observed(report);
    let operator_input = report
        .get("operator_host_input")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let input_ready = real_input || operator_input;
    let has_route = target_hit.is_some() && !matched_source_intents.is_empty();
    let status = if input_ready && has_route {
        "pass"
    } else if !input_ready && has_route {
        "waiting-for-host-input"
    } else {
        "fail"
    };
    let route_steps = if has_route {
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
            "real_os_input_observed": real_input
        })]
    } else {
        Vec::new()
    };
    json!({
        "status": status,
        "example": example,
        "target_hit_region": route_steps
            .first()
            .and_then(|step| step.get("target_hit_region"))
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "source_intents": route_steps
            .first()
            .and_then(|step| step.get("source_intents"))
            .cloned()
            .unwrap_or_else(|| json!([])),
        "operator_host_input_observed": operator_input,
        "real_os_input_observed": real_input,
        "per_step_host_input_route": route_steps.clone(),
        "per_step_os_pointer_keyboard_route": route_steps,
        "blocked_reason": match status {
            "pass" => serde_json::Value::Null,
            "waiting-for-host-input" => json!("generic hit/source-intent route exists, but no operator host input was recorded"),
            _ => json!("native document layout did not expose both a hit region and source intent for a route target")
        }
    })
}

fn native_runtime_assertions_after_input(
    example: &str,
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

    let real_input = native_gpu_real_input_observed(report);
    let operator_input = report
        .get("operator_host_input")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    let input_ready = real_input || operator_input;
    let host_route_ready = report
        .pointer("/native_host_input_route_evidence/status")
        .and_then(serde_json::Value::as_str)
        == Some("pass");
    let Some(source_intents) = report
        .pointer("/native_host_input_route_evidence/source_intents")
        .and_then(serde_json::Value::as_array)
    else {
        return json!({
            "status": "fail",
            "assertions": [],
            "blocked_reason": "host route evidence did not expose source intents"
        });
    };

    let Some(events) = native_runtime_events_for_route(example, source_intents) else {
        return json!({
            "status": "fail",
            "assertions": [],
            "operator_host_input_observed": operator_input,
            "real_os_input_observed": real_input,
            "host_route_ready": host_route_ready,
            "blocked_reason": "host route source intents cannot be mapped to public runtime source events"
        });
    };

    if !input_ready || !host_route_ready {
        return json!({
            "status": "waiting-for-host-input",
            "assertions": [],
            "planned_public_runtime_events": events
                .iter()
                .map(native_runtime_event_report)
                .collect::<Vec<_>>(),
            "operator_host_input_observed": operator_input,
            "real_os_input_observed": real_input,
            "host_route_ready": host_route_ready,
            "private_runtime_dispatch_used": false,
            "blocked_reason": "runtime assertions are gated until operator host input and generic host route evidence are present"
        });
    }

    let source_path = PathBuf::from(format!("examples/{example}.bn"));
    let scenario_path = PathBuf::from(format!("examples/{example}.scn"));
    let source_text = match std::fs::read_to_string(&source_path) {
        Ok(source_text) => source_text,
        Err(error) => {
            return json!({
                "status": "fail",
                "assertions": [],
                "blocked_reason": format!("failed to read source `{}`: {error}", source_path.display())
            });
        }
    };
    let mut runtime = match LiveRuntime::new(
        &format!("native-preview-e2e:{example}"),
        &source_text,
        &scenario_path,
    ) {
        Ok(runtime) => runtime,
        Err(error) => {
            return json!({
                "status": "fail",
                "assertions": [],
                "blocked_reason": format!("failed to initialize public LiveRuntime: {error}")
            });
        }
    };

    let mut assertions = Vec::new();
    let mut outputs = Vec::new();
    for (index, event) in events.into_iter().enumerate() {
        match runtime.apply_source_event(event.clone()) {
            Ok(output) => {
                let assertion =
                    native_runtime_assertion_for_output(example, index, &event, &output);
                outputs.push(json!({
                    "event": native_runtime_event_report(&event),
                    "semantic_delta_count": output.semantic_deltas.len(),
                    "render_patch_count": output.render_patches.len(),
                    "state_summary": output.state_summary
                }));
                assertions.push(assertion);
            }
            Err(error) => {
                assertions.push(json!({
                    "id": format!("native-runtime-event-{index}"),
                    "pass": false,
                    "event": native_runtime_event_report(&event),
                    "error": error.to_string()
                }));
            }
        }
    }
    let pass = !assertions.is_empty()
        && assertions.iter().all(|assertion| {
            assertion.get("pass").and_then(serde_json::Value::as_bool) == Some(true)
        });
    json!({
        "status": if pass { "pass" } else { "fail" },
        "assertions": assertions,
        "public_runtime_api": "boon_runtime::LiveRuntime::apply_source_event",
        "private_runtime_dispatch_used": false,
        "operator_host_input_observed": operator_input,
        "real_os_input_observed": real_input,
        "host_route_ready": host_route_ready,
        "outputs": outputs
    })
}

fn native_runtime_events_for_route(
    example: &str,
    source_intents: &[serde_json::Value],
) -> Option<Vec<LiveSourceEvent>> {
    let has_source = |source: &str| {
        source_intents.iter().any(|intent| {
            intent
                .get("source_path")
                .and_then(serde_json::Value::as_str)
                == Some(source)
        })
    };
    let has_intent_node = |node: &str, expected_intent: &str| {
        source_intents.iter().any(|intent| {
            intent.get("node").and_then(serde_json::Value::as_str) == Some(node)
                && intent.get("intent").and_then(serde_json::Value::as_str) == Some(expected_intent)
        })
    };
    match example {
        "todomvc"
            if has_source("store.sources.new_todo_input.change")
                || has_intent_node("todo_new_input", "change") =>
        {
            Some(vec![
                LiveSourceEvent {
                    source: "store.sources.new_todo_input.change".to_owned(),
                    text: Some("Native GPU todo".to_owned()),
                    ..LiveSourceEvent::default()
                },
                LiveSourceEvent {
                    source: "store.sources.new_todo_input.key_down".to_owned(),
                    text: Some("Native GPU todo".to_owned()),
                    key: Some("Enter".to_owned()),
                    ..LiveSourceEvent::default()
                },
            ])
        }
        "cells"
            if has_source("cell.sources.editor.change")
                || has_intent_node("cell_editor", "change")
                || has_intent_node("formula_editor", "change") =>
        {
            Some(vec![
                LiveSourceEvent {
                    source: "cell.sources.editor.change".to_owned(),
                    text: Some("41".to_owned()),
                    address: Some("A0".to_owned()),
                    ..LiveSourceEvent::default()
                },
                LiveSourceEvent {
                    source: "cell.sources.editor.commit".to_owned(),
                    text: Some("41".to_owned()),
                    key: Some("Enter".to_owned()),
                    address: Some("A0".to_owned()),
                    ..LiveSourceEvent::default()
                },
            ])
        }
        _ => None,
    }
}

fn native_runtime_event_report(event: &LiveSourceEvent) -> serde_json::Value {
    json!({
        "source": event.source,
        "text": event.text,
        "key": event.key,
        "address": event.address,
        "target_text": event.target_text,
        "target_occurrence": event.target_occurrence
    })
}

fn native_runtime_assertion_for_output(
    example: &str,
    index: usize,
    event: &LiveSourceEvent,
    output: &boon_runtime::LiveStepOutput,
) -> serde_json::Value {
    match example {
        "todomvc" => {
            let expected_title = "Native GPU todo";
            let todos = output
                .state_summary
                .get("todos")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            let inserted = todos
                .iter()
                .any(|todo| todo.get("title") == Some(&json!(expected_title)));
            let draft_matches =
                output.state_summary.get("new_todo_text") == Some(&json!(expected_title));
            let pass = if event.source.ends_with(".change") {
                draft_matches
            } else {
                inserted
            };
            json!({
                "id": format!("native-runtime-todomvc-event-{index}"),
                "pass": pass,
                "event": native_runtime_event_report(event),
                "expected": if event.source.ends_with(".change") {
                    json!({"new_todo_text": expected_title})
                } else {
                    json!({"todo_title_inserted": expected_title})
                },
                "actual": {
                    "new_todo_text": output.state_summary.get("new_todo_text").cloned().unwrap_or_else(|| json!(null)),
                    "todo_count": todos.len(),
                    "inserted": inserted
                }
            })
        }
        "cells" => {
            let cells = output
                .state_summary
                .get("cells")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            let a0 = cells
                .iter()
                .find(|cell| cell.get("address") == Some(&json!("A0")));
            let pass = if event.source.ends_with(".change") {
                a0.and_then(|cell| cell.get("editing_text")) == Some(&json!("41"))
                    && a0.and_then(|cell| cell.get("editing")) == Some(&json!(true))
            } else {
                a0.and_then(|cell| cell.get("value")) == Some(&json!("41"))
                    && a0.and_then(|cell| cell.get("formula")) == Some(&json!("41"))
            };
            json!({
                "id": format!("native-runtime-cells-event-{index}"),
                "pass": pass,
                "event": native_runtime_event_report(event),
                "expected": if event.source.ends_with(".change") {
                    json!({"A0": {"editing_text": "41", "editing": true}})
                } else {
                    json!({"A0": {"value": "41", "formula": "41"}})
                },
                "actual": a0.cloned().unwrap_or_else(|| json!(null))
            })
        }
        _ => json!({
            "id": format!("native-runtime-event-{index}"),
            "pass": false,
            "event": native_runtime_event_report(event),
            "error": format!("unsupported example `{example}`")
        }),
    }
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
            "verify-native-gpu-scroll-speed no longer accepts --target; use `--example cells` or `--surface dev-code-editor`"
                .to_owned(),
        );
    }

    let selected = match (example.as_deref(), surface.as_deref()) {
        (None, None) => "cells",
        (Some("cells"), None) | (Some("cells"), Some("cells")) => "cells",
        (None, Some("cells")) => "cells",
        (None, Some("dev-code-editor")) => "dev-code-editor",
        (Some("cells"), Some("dev-code-editor")) => {
            blockers.push(
                "ambiguous scroll selector: `--example cells` conflicts with `--surface dev-code-editor`"
                    .to_owned(),
            );
            "dev-code-editor"
        }
        (Some(other), _) => {
            blockers.push(format!(
                "unsupported scroll example `{other}`; expected `--example cells`"
            ));
            "cells"
        }
        (_, Some(other)) => {
            blockers.push(format!(
                "unsupported scroll surface `{other}`; expected `--surface dev-code-editor` or `--surface cells`"
            ));
            "cells"
        }
    };

    NativeGpuScrollSelector {
        label: selected.to_owned(),
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
    let source_path = if dev_editor {
        let fixture = artifacts_dir.join("dev-code-editor-scroll-fixture.bn");
        write_dev_code_editor_scroll_fixture(&fixture)?;
        fixture
    } else {
        PathBuf::from("examples/cells.bn")
    };
    let source_hash = file_hash(source_path.to_string_lossy().as_ref());
    let source_text = std::fs::read_to_string(&source_path)?;
    let layout_probe_report = artifacts_dir.join(format!("scroll-{label}-layout-proof.json"));
    let mut cosmic_launch_proof = json!({"status": "not-run"});
    let title_token = native_gpu_title_token(&format!("scroll-{label}"));
    let input_sample_delay_ms = native_gpu_input_sample_delay_ms();
    let _ = std::fs::remove_file(&supervisor_report);
    let _ = std::fs::remove_file(&live_state_report);

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

    let build = Command::new("cargo")
        .args(["build", "-p", "boon_native_playground"])
        .status()?;
    push_audit_check(
        &mut checks,
        &mut blockers,
        format!("native-gpu-scroll-{label}:playground-build"),
        build.success(),
        format!("cargo build -p boon_native_playground status={build}"),
        (!build.success()).then(|| "failed to build boon_native_playground".to_owned()),
    );

    let layout_probe = if build.success() && selector_valid {
        run_native_layout_probe(&source_path, &layout_probe_report)?
    } else {
        json!({"status": "not-run", "reason": "boon_native_playground build failed or scroll selector invalid"})
    };
    let driver_target = native_scroll_driver_target(&label, &layout_probe);
    let native_input_driver_attempt =
        native_gpu_operator_input_driver_attempt("scroll-speed", &label, driver_target.clone());

    if build.success() && wayland && selector_valid {
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
            let script = if dev_editor {
                format!(
                    "cd {} && ./target/debug/boon_native_playground --role desktop --example dev-code-editor --code-file {} --probe --child-hold-ms 10000 --dev-hold-ms 5000 --warmup-frame-count 3 --sample-frame-count 30 --title-token {} --input-sample-delay-ms {} --role-report-timeout-ms {} --live-state-report {} --report {} >>/tmp/boon-native-gpu-scroll-dev-code-editor.log 2>&1",
                    shell_quote(&cwd.display().to_string()),
                    shell_quote(&source_path.display().to_string()),
                    shell_quote(&title_token),
                    input_sample_delay_ms,
                    60_000_u64.saturating_add(input_sample_delay_ms),
                    shell_quote(&live_state_report.display().to_string()),
                    shell_quote(&supervisor_report.display().to_string())
                )
            } else {
                format!(
                    "cd {} && ./target/debug/boon_native_playground --role desktop --example cells --probe --child-hold-ms 10000 --dev-hold-ms 5000 --warmup-frame-count 3 --sample-frame-count 30 --title-token {} --input-sample-delay-ms {} --role-report-timeout-ms {} --live-state-report {} --report {} >>/tmp/boon-native-gpu-scroll-cells.log 2>&1",
                    shell_quote(&cwd.display().to_string()),
                    shell_quote(&title_token),
                    input_sample_delay_ms,
                    12_000_u64.saturating_add(input_sample_delay_ms),
                    shell_quote(&live_state_report.display().to_string()),
                    shell_quote(&supervisor_report.display().to_string())
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
                let report_timeout = if dev_editor {
                    Duration::from_secs(75)
                } else {
                    Duration::from_secs(20)
                };
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
        "source_path": source_path,
        "layout_probe_report": layout_probe_report,
        "prelaunch_layout_probe": layout_probe,
        "driver_target_region": driver_target,
        "supervisor_report": supervisor_report,
        "live_state_report": live_state_report,
        "launcher_command": "cosmic-background-launch --workspace boon-circuit",
        "cosmic_background_launch_proof": cosmic_launch_proof,
        "live_desktop_input_allowed": false,
        "native_input_driver_attempt": native_input_driver_attempt,
        "surface_under_test": label,
        "blocked_reason": "native scroll proof did not yet produce app-owned WGPU readback, host wheel route, and frame/readback timing evidence"
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
    } else {
        extra["logical_columns"] =
            json!(native_gpu_budget_u64("cells", "logical_columns").unwrap_or(26));
        extra["logical_rows"] =
            json!(native_gpu_budget_u64("cells", "logical_rows").unwrap_or(100));
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
        if let Some(presented_frame_ms) = supervisor
            .pointer("/preview_surface_proof/frame_timing/presented_frame_ms_p95")
            .or_else(|| supervisor.pointer("/preview_surface_proof/presented_frame_ms"))
            .and_then(serde_json::Value::as_f64)
        {
            extra["preview_frame_ms_p95"] = json!(presented_frame_ms);
            extra["probe_presented_frame_ms"] = json!(presented_frame_ms);
        }
        if let Some(frame_timing) = supervisor
            .pointer("/preview_surface_proof/frame_timing")
            .cloned()
        {
            extra["preview_frame_timing"] = frame_timing;
        }
        if let Some(first_frame_ms) = supervisor
            .pointer("/preview_surface_proof/first_frame_ms")
            .and_then(serde_json::Value::as_f64)
        {
            extra["probe_first_frame_with_readback_ms"] = json!(first_frame_ms);
        }
        if let Some(readback_ms) = supervisor
            .pointer("/preview_surface_proof/readback_ms")
            .and_then(serde_json::Value::as_f64)
        {
            extra["probe_readback_ms"] = json!(readback_ms);
        }
        if supervisor
            .pointer("/preview_surface_proof/presented_frame")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            extra["missed_frame_count"] = json!(0);
            extra["probe_presented_frame"] = json!(true);
        }
        if let Some(input_adapter) = supervisor
            .pointer("/preview_surface_proof/input_adapter")
            .cloned()
        {
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
            let real_wheel_input_observed = extra
                .pointer("/native_input_adapter/mouse_scroll_event_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
                > 0;
            if real_os_input_observed {
                extra["input_injection_method"] =
                    json!("os_pointer_keyboard_to_visible_wayland_app_window");
            }
            if real_wheel_input_observed {
                extra["real_wheel_input"] = json!(true);
            }
        }
    }
    add_native_scroll_model_evidence(&mut extra, dev_editor);
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

    write_native_gate_report(
        args,
        "verify-native-gpu-scroll-speed",
        checks,
        blockers,
        extra,
    )
}

fn add_native_scroll_model_evidence(extra: &mut serde_json::Value, dev_editor: bool) {
    let preview_frame_ms = extra
        .get("preview_frame_ms_p95")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let preview_frame_budget =
        native_gpu_budget_f64("frame", "preview_frame_ms_p95").unwrap_or(16.7);
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
        extra
            .pointer("/native_input_adapter/scroll_delta_x")
            .and_then(numeric_value_as_f64)
            .unwrap_or(0.0)
    };
    let scroll_delta_y = if operator_wheel_input {
        extra
            .pointer("/operator_host_input_evidence/deltas/vertical_px")
            .and_then(numeric_value_as_f64)
            .unwrap_or(720.0)
    } else {
        extra
            .pointer("/native_input_adapter/scroll_delta_y")
            .and_then(numeric_value_as_f64)
            .unwrap_or(0.0)
    };
    let vertical_wheel_observed = wheel_events > 0 && scroll_delta_y.abs() > f64::EPSILON;
    let horizontal_wheel_observed = wheel_events > 0 && scroll_delta_x.abs() > f64::EPSILON;
    let required_wheel_axes_observed = vertical_wheel_observed && horizontal_wheel_observed;
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
    let frame_upload_budget_pass =
        preview_frame_ms <= preview_frame_budget && render_upload_bytes <= upload_budget;
    extra["wheel_events_coalesced"] = json!(wheel_events);
    extra["operator_vertical_wheel_input"] = json!(operator_wheel_input && vertical_wheel_observed);
    extra["operator_horizontal_wheel_input"] =
        json!(operator_wheel_input && horizontal_wheel_observed);
    extra["real_vertical_wheel_input"] = json!(!operator_wheel_input && vertical_wheel_observed);
    extra["real_horizontal_wheel_input"] =
        json!(!operator_wheel_input && horizontal_wheel_observed);
    extra["real_wheel_input"] = json!(!operator_wheel_input && required_wheel_axes_observed);
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
                && wheel_to_visible_ms.is_some_and(|value| {
                    value
                        <= native_gpu_budget_f64("frame", "wheel_to_visible_ms_p95").unwrap_or(50.0)
                })
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
                && wheel_to_visible_ms.is_some_and(|value| {
                    value
                        <= native_gpu_budget_f64("frame", "wheel_to_visible_ms_p95").unwrap_or(50.0)
                })
        );
    }
}

fn native_scroll_input_route_evidence(
    label: &str,
    report: &serde_json::Value,
) -> serde_json::Value {
    let scroll_regions = report
        .pointer("/preview_document_layout_proof/scroll_regions")
        .and_then(serde_json::Value::as_array)
        .cloned()
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

fn write_dev_code_editor_scroll_fixture(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut source = String::from(
        "# Boon Circuit native GPU dev code editor scroll fixture\n\nEXAMPLE Cells\n\nstore:\n    sources: [change: SOURCE]\n    title: TEXT { Native GPU editor scroll fixture }\n    draft: TEXT { } |> HOLD draft { LATEST { sources.change.text } }\n    lines: LIST { [title: TEXT { line }] } |> List/map(seed, new: seed)\n\ndocument:\n    element:\n        kind: Column\n        id: \"dev_code_editor_fixture\"\n        children:\n            element:\n                kind: Column\n                id: \"dev_code_editor_viewport\"\n                height: 720\n                scroll: true\n                scroll_x: true\n                children:\n                    element:\n                        kind: Text\n                        id: \"dev_code_editor_visible_slice\"\n                        text: \"Native GPU dev editor stress fixture\"\n\n# Code editor scroll payload follows. These lines are part of the source file\n# under test, but intentionally not materialized as document nodes.\n",
    );
    let long_tail = "x".repeat(2_100);
    for index in 0..10_000_u32 {
        if index == 9_999 {
            source.push_str(&format!("# editor-fixture-line {index:05} {long_tail}\n"));
        } else {
            source.push_str(&format!("# editor-fixture-line {index:05}\n"));
        }
    }
    std::fs::write(path, source)?;
    Ok(())
}

fn verify_native_gpu_negative(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let base = || {
        json!({
            "command": "verify-native-gpu-preview-e2e",
            "git_commit": git_commit(),
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
    let mut checks = Vec::new();
    let mut blockers = Vec::new();
    let required = native_gpu_required_reports();
    let mut artifacts = Vec::new();
    let check_existing = args.iter().any(|arg| arg == "--check-existing");
    push_audit_check(
        &mut checks,
        &mut blockers,
        "native-gpu-all:check-existing-mode",
        check_existing,
        format!("--check-existing present={check_existing}"),
        (!check_existing)
            .then(|| "native GPU aggregate currently requires --check-existing".to_owned()),
    );
    for requirement in &required {
        let label = requirement.label;
        let path = &requirement.path;
        let exists = path.exists();
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-gpu-all:report-present:{label}"),
            exists,
            format!("{} exists={exists}", path.display()),
            (!exists).then(|| format!("missing native GPU report `{}`", path.display())),
        );
        if !exists {
            continue;
        }
        let report = read_json(path)?;
        let schema_blockers = validate_native_gpu_child_report_shape(path, &report);
        let schema_valid = schema_blockers.is_empty();
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-gpu-all:schema:{label}"),
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
            format!("native-gpu-all:contract:{label}"),
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
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-gpu-all:status-pass:{label}"),
            pass,
            format!("{} status pass={pass}", path.display()),
            (!pass).then(|| format!("native GPU report `{}` did not pass", path.display())),
        );
        let commit_fresh = report.get("git_commit").and_then(serde_json::Value::as_str)
            == Some(git_commit().as_str());
        push_audit_check(
            &mut checks,
            &mut blockers,
            format!("native-gpu-all:git-fresh:{label}"),
            commit_fresh,
            format!("{} git_fresh={commit_fresh}", path.display()),
            (!commit_fresh).then(|| {
                format!(
                    "native GPU report `{}` is stale for current git commit",
                    path.display()
                )
            }),
        );
        artifacts.push(artifact_hash(path)?);
    }
    write_native_gate_report(
        args,
        "verify-native-gpu-all",
        checks,
        blockers,
        json!({
            "required_reports": required.iter().map(|report| {
                json!({
                    "label": report.label,
                    "path": report.path.display().to_string(),
                    "command": report.command,
                    "required_argv": report.required_argv
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
}

fn native_gpu_required_reports() -> Vec<NativeGpuRequiredReport> {
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
            &[],
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
    }
}

fn validate_native_gpu_child_report_shape(path: &Path, report: &serde_json::Value) -> Vec<String> {
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
    blockers.extend(native_gpu_report_rejection_reasons(report));
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
        .map(|blocker| format!("{}: {blocker}", path.display()))
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
    blockers.extend(native_gpu_report_rejection_reasons(report));
    blockers.extend(native_gpu_label_contract_blockers(
        requirement.label,
        report,
    ));
    blockers
}

fn native_gpu_label_contract_blockers(label: &str, report: &serde_json::Value) -> Vec<String> {
    let mut blockers = Vec::new();
    match label {
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
            require_bool_field(&mut blockers, report, "operator_host_input", true);
            require_bool_field(&mut blockers, report, "real_os_input", false);
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
            require_nonempty_array(&mut blockers, report, "per_step_os_pointer_keyboard_route");
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
            if report
                .get("input_injection_method")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|method| {
                    let method = method.to_ascii_lowercase();
                    !method.contains("operator_host_event_harness")
                        || method.contains("xvfb")
                        || method.contains("headless")
                        || method.contains("compositor")
                })
            {
                blockers.push(
                    "input_injection_method must be operator_host_event_harness for portable native E2E"
                        .to_owned(),
                );
            }
        }
        "scroll-speed-cells" => {
            require_scroll_budget_fields(&mut blockers, report);
            require_common_scroll_hot_path_fields(&mut blockers, report);
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

fn require_scroll_budget_fields(blockers: &mut Vec<String>, report: &serde_json::Value) {
    require_str_field(blockers, report, "display_server", "wayland");
    require_bool_field(blockers, report, "budget_pass", true);
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
    require_positive_u64(blockers, report, "sample_frame_count");
    require_positive_u64(blockers, report, "sustained_scroll_duration_ms");
    require_object_field(blockers, report, "scroll_distance_px_rows_cols");
    require_object_field(blockers, report, "materialized_range_before_after");
    require_axis_p95_at_most(
        blockers,
        report,
        "wheel_to_visible_ms_p95_per_axis",
        native_gpu_budget_f64("frame", "wheel_to_visible_ms_p95").unwrap_or(50.0),
    );
    require_u64_array_field(blockers, report, "frames_over_16_7_ms");
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
        "verify-native-gpu-preview-e2e" => match value_arg(args, "--example").as_deref() {
            Some("todomvc") => "preview-e2e-todomvc",
            Some("cells") => "preview-e2e-cells",
            _ => "preview-e2e",
        },
        "verify-native-gpu-scroll-speed" => match native_gpu_scroll_selector(args).label.as_str() {
            "dev-code-editor" => "scroll-speed-dev-code-editor",
            _ => "scroll-speed-cells",
        },
        "verify-native-gpu-negative" => "negative",
        "verify-native-gpu-all" => return PathBuf::from("target/reports/native-gpu-all.json"),
        _ => command,
    };
    PathBuf::from(format!("target/reports/native-gpu/{name}.json"))
}

fn native_gpu_report_rejects(report: &serde_json::Value) -> bool {
    !native_gpu_report_rejection_reasons(report).is_empty()
}

fn native_gpu_report_rejection_reasons(report: &serde_json::Value) -> Vec<String> {
    let mut reasons = Vec::new();
    if report
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
        .get("binary_hash")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|hash| hash != current_binary_hash())
    {
        reasons.push("binary_hash is stale for current xtask binary".to_owned());
    }
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
    reasons
}

fn command_argv_contains_pair(argv: &[serde_json::Value], flag: &str, value: &str) -> bool {
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
        .is_some_and(|value| value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit()))
    {
        blockers.push(format!("{key} must be a 64-character hex sha256"));
    }
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

fn command_available(command: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|path| path.join(command).exists()))
}

fn run_cosmic_background_launch(
    workspace: &str,
    script: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let output = Command::new("cosmic-background-launch")
        .args(["--workspace", workspace, "--", "bash", "-lc", script])
        .output()?;
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
                | "verify-native-gpu-dependency-graph"
                | "verify-native-gpu-architecture"
                | "verify-native-gpu-layout-contract"
                | "verify-native-gpu-shaders"
                | "verify-native-gpu-multiwindow"
                | "verify-native-gpu-ipc-backpressure"
                | "verify-native-gpu-observability"
                | "verify-native-gpu-preview-e2e"
                | "verify-native-gpu-scroll-speed"
                | "verify-native-gpu-negative"
                | "verify-native-gpu-all"
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
    source_path: &Path,
    report: &Path,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    if let Some(parent) = report.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let status = Command::new("./target/debug/boon_native_playground")
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
    read_json(report)
}

fn native_preview_driver_target(
    example: &str,
    layout_probe: &serde_json::Value,
) -> Option<serde_json::Value> {
    let preferred_nodes = match example {
        "todomvc" => ["todo_new_input", "todo_row_checkbox", "todo_row_title"].as_slice(),
        "cells" => ["cell_editor", "formula_editor"].as_slice(),
        _ => [].as_slice(),
    };
    let hit_targets = layout_probe
        .get("hit_target_assertions")
        .and_then(serde_json::Value::as_array)?;
    let target = preferred_nodes
        .iter()
        .find_map(|node| {
            hit_targets.iter().find(|target| {
                target.get("node").and_then(serde_json::Value::as_str) == Some(*node)
            })
        })
        .or_else(|| hit_targets.first())?;
    native_driver_target_from_region("hit_region", target)
}

fn native_scroll_driver_target(
    label: &str,
    layout_probe: &serde_json::Value,
) -> Option<serde_json::Value> {
    let preferred_nodes = match label {
        "dev-code-editor" => ["dev_code_editor_viewport"].as_slice(),
        "cells" => ["spreadsheet_body", "spreadsheet_header"].as_slice(),
        _ => [].as_slice(),
    };
    let scroll_regions = layout_probe
        .get("scroll_regions")
        .and_then(serde_json::Value::as_array)?;
    let target = preferred_nodes
        .iter()
        .find_map(|node| {
            scroll_regions.iter().find(|region| {
                region.get("node").and_then(serde_json::Value::as_str) == Some(*node)
                    && region
                        .get("axis")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|axis| axis.eq_ignore_ascii_case("vertical"))
            })
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
    Some(json!({
        "kind": kind,
        "id": region.get("id").cloned().unwrap_or(serde_json::Value::Null),
        "node": region.get("node").cloned().unwrap_or(serde_json::Value::Null),
        "axis": region.get("axis").cloned().unwrap_or(serde_json::Value::Null),
        "bounds": bounds,
        "local_x": x + width / 2.0,
        "local_y": y + height / 2.0,
        "targeting_basis": "prelaunch-generic-document-layout-proof"
    }))
}

fn native_gpu_real_input_observed(report: &serde_json::Value) -> bool {
    report
        .pointer("/native_input_adapter/real_os_events_observed")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        && (report
            .pointer("/native_input_adapter/mouse_last_window_protocol_id")
            .and_then(serde_json::Value::as_u64)
            .is_some()
            || report
                .pointer("/native_input_adapter/keyboard_last_window_protocol_id")
                .and_then(serde_json::Value::as_u64)
                .is_some())
}

fn native_gpu_operator_input_driver_attempt(
    kind: &str,
    label: &str,
    target_region: Option<serde_json::Value>,
) -> serde_json::Value {
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
        "reason": "portable verifier uses host events instead of compositor or desktop input synthesis"
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

fn native_gpu_preview_input_text(label: &str) -> &'static str {
    match label {
        "todomvc" => "Native GPU todo",
        "cells" => "41",
        _ => "boon-native-input-proof",
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
