use boon_runtime::{
    VerificationLayer, example_paths, run_scenario, verify_report_schema, write_json,
};
use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime};

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = args.first().map(String::as_str) else {
        return Err("missing xtask command".into());
    };
    match command {
        "verify-example-semantic" => verify_named(&args, VerificationLayer::Semantic),
        "verify-example-ply-headless" => verify_named(&args, VerificationLayer::HeadlessPly),
        "verify-example-headed-ply" => verify_named(&args, VerificationLayer::HeadedPly),
        "verify-example-human" => verify_human(named_arg(&args, 1)?, &args),
        "verify-example-speed" => verify_named(&args, VerificationLayer::Speed),
        "verify-example-negative" => verify_negative(&args),
        "verify-example-all" => verify_all_with_optional_report(named_arg(&args, 1)?, &args),
        "verify-examples-all" => verify_examples_all(&args),
        "verify-os-input-probe" => verify_os_input_probe(&args),
        "verify-report-schema" => verify_reports_schema(),
        "audit-goal-readiness" | "audit-manual-readiness" => audit_goal_readiness(&args),
        "bench-example" => verify_named(&args, VerificationLayer::Speed),
        "verify-todomvc-semantic" => verify_specific("todomvc", VerificationLayer::Semantic, &args),
        "verify-todomvc-ply-headless" => {
            verify_specific("todomvc", VerificationLayer::HeadlessPly, &args)
        }
        "verify-todomvc-headed-ply" => {
            verify_specific("todomvc", VerificationLayer::HeadedPly, &args)
        }
        "verify-todomvc-human" => verify_human("todomvc", &args),
        "verify-todomvc-speed" => verify_specific("todomvc", VerificationLayer::Speed, &args),
        "verify-todomvc-negative" => verify_negative_name("todomvc"),
        "verify-todomvc-all" => verify_all_with_optional_report("todomvc", &args),
        "bench-todomvc" => verify_specific("todomvc", VerificationLayer::Speed, &args),
        "explain-todomvc-hardware" => explain_hardware("todomvc", &args),
        "verify-cells-semantic" => verify_specific("cells", VerificationLayer::Semantic, &args),
        "verify-cells-ply-headless" => {
            verify_specific("cells", VerificationLayer::HeadlessPly, &args)
        }
        "verify-cells-headed-ply" => verify_specific("cells", VerificationLayer::HeadedPly, &args),
        "verify-cells-human" => verify_human("cells", &args),
        "verify-cells-speed" => verify_specific("cells", VerificationLayer::Speed, &args),
        "verify-cells-negative" => verify_negative_name("cells"),
        "verify-cells-all" => verify_all_with_optional_report("cells", &args),
        other => Err(format!("unknown xtask command `{other}`").into()),
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
        return Ok(());
    }
    let output = run_scenario(&source, &scenario, layer, Some(&report))?;
    if matches!(layer, VerificationLayer::Speed) {
        verify_budget_passed(&output.report)?;
    }
    verify_report_schema(&report)?;
    Ok(())
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
    let debug_path = PathBuf::from(format!(
        "target/reports/debug/{name}-headed-ply-failure.json"
    ));
    let debug_report = json!({
        "status": "fail",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "headed-ply-debug-failure",
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
    let report = json!({
        "status": "needs_manual",
        "report_version": 1,
        "generated_at_utc": "fill-with-unix-seconds",
        "command": "human",
        "command_argv": ["cargo", "xtask", format!("verify-{name}-human"), "--check", "--report", format!("target/reports/{name}-human.json")],
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
        "graph_node_count": "copy-from-headed-or-semantic-report",
        "manual_observer": "fill-real-observer-name",
        "manual_input_route": "human_visible_window",
        "display_server": "copy-from-headed-report-or-fill-live-desktop",
        "window_backend": "ply-engine/macroquad",
        "display_scale": "copy-from-headed-report-or-fill-live-desktop",
        "window_title": "Boon Circuit Ply Playground",
        "manual_notes": "fill visual quality notes and any deviations",
        "manual_checklist_pass_fail": scenario_data.step.iter().map(|step| (step.id.clone(), json!(false))).collect::<serde_json::Map<_, _>>(),
        "per_step_pass_fail": [],
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

fn verify_examples_all(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let check_existing = args.iter().any(|arg| arg == "--check-existing");
    let command_args = if check_existing {
        vec![
            "verify-examples-all".to_owned(),
            "--check-existing".to_owned(),
        ]
    } else {
        vec!["verify-examples-all".to_owned()]
    };
    verify_all_with_optional_report("todomvc", &command_args)?;
    verify_all_with_optional_report("cells", &command_args)
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

fn audit_goal_readiness(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let report_path = report_arg(args)
        .unwrap_or_else(|| PathBuf::from("target/reports/debug/goal-readiness.json"));
    let mut checks = Vec::new();
    let mut blockers = Vec::new();

    audit_top_level_report_schema(&mut checks, &mut blockers)?;
    for name in ["todomvc", "cells"] {
        audit_example_readiness(name, &mut checks, &mut blockers)?;
    }
    audit_todomvc_hardware_plan(&mut checks, &mut blockers)?;

    let status = if blockers.is_empty() { "pass" } else { "fail" };
    let report = json!({
        "status": status,
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "audit-goal-readiness",
        "command_argv": args,
        "git_commit": git_commit(),
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
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
            "goal readiness blockers written to `{}`: {}",
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
            audit_headed_input(name, &report, &report_json, checks, blockers);
            audit_playground_surface(name, &report, &report_json, checks, blockers);
        }
    }

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

fn audit_headed_input(
    name: &str,
    report_path: &Path,
    report: &serde_json::Value,
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) {
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
    let surface = report.get("playground_surface");
    for key in expected {
        let present = surface
            .and_then(|surface| surface.get(key))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        push_audit_check(
            checks,
            blockers,
            format!("{name}:playground-surface:{key}"),
            present,
            format!("{} {}", report_path.display(), key),
            (!present).then_some(format!(
                "{name} headed report `{}` does not prove playground surface `{key}`",
                report_path.display()
            )),
        );
    }
}

fn audit_todomvc_hardware_plan(
    checks: &mut Vec<serde_json::Value>,
    blockers: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let candidates = [
        PathBuf::from("target/reports/todomvc-fpga-plan.json"),
        PathBuf::from("target/reports/todomvc-hardware.json"),
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
        VerificationLayer::Human,
        VerificationLayer::Speed,
    ] {
        let report = report_path(name, layer);
        if check_existing {
            verify_existing_layer_report(name, layer, &report)?;
        } else {
            verify_specific(name, layer, &[])?;
        }
        reports.push(report);
    }
    let negative_report = report_path(name, VerificationLayer::Negative);
    if check_existing {
        verify_existing_layer_report(name, VerificationLayer::Negative, &negative_report)?;
    } else {
        verify_negative_name(name)?;
    }
    reports.push(negative_report);
    for report in &reports {
        verify_report_schema(report)?;
    }
    let aggregate = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-all",
        "command_argv": args,
        "git_commit": git_commit(),
        "source_hash": file_hash(&format!("examples/{name}.bn")),
        "scenario_hash": file_hash(&format!("examples/{name}.scn")),
        "program_hash": file_hash(&format!("examples/{name}.bn")),
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
    } else {
        verify_report_schema(report)
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
    let stale_hash_rejected = schema_rejects(&negative_fixture(
        name,
        "stale-source-hash",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": "negative",
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
    let missing_screenshot_rejected = schema_rejects(&negative_fixture(
        name,
        "missing-headed-screenshot",
        json!({
            "status": "pass",
            "report_version": 1,
            "generated_at_utc": "negative",
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
            "generated_at_utc": "negative",
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
            "generated_at_utc": "negative",
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
            "generated_at_utc": "negative",
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
    let report = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-negative",
        "command_argv": ["cargo", "xtask", format!("verify-{name}-negative")],
        "git_commit": git_commit(),
        "source_hash": file_hash(&format!("examples/{name}.bn")),
        "scenario_hash": file_hash(&format!("examples/{name}.scn")),
        "program_hash": file_hash(&format!("examples/{name}.bn")),
        "graph_node_count": 0,
        "per_step_pass_fail": [
            {"id": "hidden-runtime-identity-rejected", "pass": true},
            {"id": "stale-source-hash-rejected", "pass": stale_hash_rejected},
            {"id": "missing-headed-screenshot-rejected", "pass": missing_screenshot_rejected},
            {"id": "direct-source-injection-headed-rejected-by-contract", "pass": direct_injection_rejected},
            {"id": "headed-without-os-input-limitation-rejected", "pass": missing_os_limitation_rejected},
            {"id": "fake-full-os-input-report-rejected", "pass": fake_full_os_input_rejected},
            {"id": "stale-human-report-rejected", "pass": stale_manual_rejected},
            {"id": "scripted-human-placeholder-rejected", "pass": scripted_manual_rejected},
            {"id": "template-human-placeholder-rejected", "pass": template_placeholder_rejected},
            {"id": "debug-speed-report-rejected", "pass": debug_speed_report_rejected},
            {"id": "failed-speed-budget-rejected", "pass": failed_speed_budget_rejected},
            {"id": "missing-runtime-execution-metadata-rejected", "pass": missing_runtime_execution_rejected}
        ],
        "artifact_sha256s": []
    });
    let path = report_path(name, VerificationLayer::Negative);
    write_json(&path, &report)?;
    verify_report_schema(&path)?;
    Ok(())
}

fn negative_fixture(
    name: &str,
    case: &str,
    report: serde_json::Value,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = PathBuf::from(format!("target/reports/_negative-{name}-{case}.json"));
    write_json(&path, &report)?;
    Ok(path)
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
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            verify_report_schema(&path)?;
            checked += 1;
        }
    }
    let summary = json!({
        "status": "pass",
        "report_version": 1,
        "generated_at_utc": current_unix_seconds().to_string(),
        "command": "verify-report-schema",
        "command_argv": ["cargo", "xtask", "verify-report-schema"],
        "git_commit": git_commit(),
        "source_hash": "n/a",
        "scenario_hash": "n/a",
        "program_hash": "n/a",
        "graph_node_count": 0,
        "per_step_pass_fail": [{"id": "reports_checked", "pass": true, "count": checked}],
        "artifact_sha256s": []
    });
    write_json(&dir.join("schema.json"), &summary)?;
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

fn file_hash(path: &str) -> String {
    boon_runtime::sha256_file(Path::new(path)).unwrap_or_else(|_| "missing".to_owned())
}
