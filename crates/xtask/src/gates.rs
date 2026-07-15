use crate::architecture::collect_architecture_evidence;
use crate::report_v2::{
    AggregateGateResult, AggregateIdentity, AggregateMode, AggregateReport, AggregateReportKind,
    BoundedId, CheckOutcome, ChildValidation, DetailText, ExpectedIdentity, FORMAT_VERSION,
    GateEvidence, GateName, GateReport, GateRunner, ManifestGate, ManifestIdentity,
    ProducerEvidence, ReportStatus, ToolResult, VerifierProfile, check, current_identity, detail,
    empty_evidence, gate_report, load_manifest, make_report_id, make_run_id, protocol_name,
    read_gate_report, read_producer_envelope, report_file_metadata, unix_time_ms, workspace_path,
    write_aggregate_report, write_gate_report,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const RUN_ID_ENV: &str = "BOON_XTASK_RUN_ID";
const BUILD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const PRODUCER_TIMEOUT: Duration = Duration::from_secs(3 * 60);

pub fn run_gate(
    workspace: &Path,
    gate: GateName,
    output_override: Option<PathBuf>,
) -> ToolResult<ReportStatus> {
    let (manifest, _) = load_manifest(workspace)?;
    let entry = manifest.gate(&gate);
    let output = output_override.unwrap_or_else(|| workspace_path(workspace, &entry.output));
    remove_file_if_present(&output)?;
    let run_id = std::env::var(RUN_ID_ENV)
        .ok()
        .map(BoundedId::new)
        .transpose()?
        .unwrap_or(make_run_id(gate.slug())?);

    let report = match entry.runner {
        GateRunner::Architecture => architecture_report(workspace, entry, run_id)?,
        GateRunner::NativeProduct => product_report(workspace, entry, run_id)?,
    };
    let status = report.status;
    write_gate_report(&output, &report, entry.byte_limit)?;
    println!("wrote {} ({})", output.display(), status_name(status));
    Ok(status)
}

fn architecture_report(
    workspace: &Path,
    entry: &ManifestGate,
    run_id: BoundedId,
) -> ToolResult<GateReport> {
    let expected = current_identity(workspace)?;
    let evidence = collect_architecture_evidence(workspace);
    let (status, blockers) = status_and_blockers(&evidence);
    Ok(gate_report(
        entry, run_id, expected, status, evidence, blockers,
    )?)
}

fn product_report(
    workspace: &Path,
    entry: &ManifestGate,
    run_id: BoundedId,
) -> ToolResult<GateReport> {
    let gate = &entry.gate;
    let before = current_identity(workspace)?;
    let scratch = workspace.join("target/reports/report-v2/.producer");
    fs::create_dir_all(&scratch)?;
    let build_log = scratch.join(format!("{}-{}-build.log", run_id, gate.slug()));

    let mut build = Command::new("cargo");
    build.current_dir(workspace).args([
        "build",
        "--release",
        "-p",
        "boon_native_playground",
        "--bin",
        "boon_native_playground",
    ]);
    let build_result = run_logged(&mut build, &build_log, BUILD_TIMEOUT)?;
    if !build_result.success {
        let current = current_identity(workspace)?;
        let message = format!(
            "native evidence producer build failed{}: {}",
            timeout_suffix(build_result.timed_out),
            build_result.log_tail
        );
        return failing_product_report(
            entry,
            run_id,
            current,
            ProducerEvidence {
                program: crate::report_v2::ShortText::new(
                    "cargo build --release -p boon_native_playground",
                )?,
                protocol: protocol_name(),
                exit_code: build_result.exit_code,
                elapsed_ms: build_result.elapsed_ms,
            },
            message,
        );
    }

    let evidence_relative = format!(
        "target/reports/report-v2/.producer/{}-{}.json",
        run_id,
        gate.slug()
    );
    let artifact_relative = format!(
        "target/reports/report-v2/artifacts/{}/{}",
        run_id,
        gate.slug()
    );
    let evidence_path = workspace.join(&evidence_relative);
    remove_file_if_present(&evidence_path)?;
    fs::create_dir_all(workspace.join(&artifact_relative))?;
    let producer_log = scratch.join(format!("{}-{}.log", run_id, gate.slug()));
    let binary = product_binary(workspace);
    let mut producer = Command::new(&binary);
    producer.current_dir(workspace).args([
        "--role",
        "verify-v2",
        "--gate",
        gate.slug(),
        "--evidence-output",
        &evidence_relative,
        "--artifact-dir",
        &artifact_relative,
        "--run-id",
        run_id.as_str(),
        "--source-digest",
        before.source.workspace_digest.as_str(),
    ]);
    append_profile_arguments(
        &mut producer,
        entry.profile.as_ref().expect("product profile"),
    );
    configure_product_scheduler(&mut producer);
    let process_result = run_logged(&mut producer, &producer_log, PRODUCER_TIMEOUT)?;
    let producer_metadata = ProducerEvidence {
        program: crate::report_v2::ShortText::new(binary.display().to_string())?,
        protocol: protocol_name(),
        exit_code: process_result.exit_code,
        elapsed_ms: process_result.elapsed_ms,
    };
    let after = current_identity(workspace)?;
    if before != after {
        remove_file_if_present(&evidence_path)?;
        return failing_product_report(
            entry,
            run_id,
            after,
            producer_metadata,
            "workspace identity changed while native evidence was being measured".to_owned(),
        );
    }
    if !process_result.success {
        remove_file_if_present(&evidence_path)?;
        let message = format!(
            "native evidence producer failed{}: {}",
            timeout_suffix(process_result.timed_out),
            process_result.log_tail
        );
        return failing_product_report(entry, run_id, after, producer_metadata, message);
    }

    let envelope = match read_producer_envelope(&evidence_path) {
        Ok(envelope) => envelope,
        Err(error) => {
            remove_file_if_present(&evidence_path)?;
            return failing_product_report(
                entry,
                run_id,
                after,
                producer_metadata,
                format!("native producer did not emit bounded v2 evidence: {error}"),
            );
        }
    };
    remove_file_if_present(&evidence_path)?;
    if let Err(error) = envelope.validate_for(entry, &run_id, &after.source) {
        return failing_product_report(
            entry,
            run_id,
            after,
            producer_metadata,
            format!("native producer evidence identity/protocol rejected: {error}"),
        );
    }

    let mut evidence = envelope.evidence;
    evidence.producer = Some(producer_metadata);
    evidence.checks.push(check(
        "producer-process",
        CheckOutcome::Pass,
        "rewritten native producer exited successfully through the v2 process boundary",
    ));
    let (status, blockers) = status_and_blockers(&evidence);
    let candidate = match gate_report(
        entry,
        run_id.clone(),
        after.clone(),
        status,
        evidence,
        blockers,
    ) {
        Ok(report) => report,
        Err(error) => {
            return failing_product_report(
                entry,
                run_id,
                after,
                ProducerEvidence {
                    program: crate::report_v2::ShortText::new(binary.display().to_string())?,
                    protocol: protocol_name(),
                    exit_code: Some(0),
                    elapsed_ms: process_result.elapsed_ms,
                },
                format!("native producer evidence failed v2 validation: {error}"),
            );
        }
    };
    if let Err(error) = candidate.validate_artifacts(workspace, entry.sidecar_byte_limit) {
        return failing_product_report(
            entry,
            run_id,
            after,
            ProducerEvidence {
                program: crate::report_v2::ShortText::new(binary.display().to_string())?,
                protocol: protocol_name(),
                exit_code: Some(0),
                elapsed_ms: process_result.elapsed_ms,
            },
            format!("native proof artifact validation failed: {error}"),
        );
    }
    Ok(candidate)
}

fn append_profile_arguments(command: &mut Command, profile: &VerifierProfile) {
    command
        .arg("--profile")
        .arg(profile.id.as_str())
        .arg("--profile-digest")
        .arg(profile.digest().as_str());
    for argument in &profile.arguments {
        command
            .arg(argument.flag.as_str())
            .arg(argument.value.as_str());
    }
    let requirements = &profile.proof_requirements;
    if let Some(scenario) = &requirements.scenario {
        command
            .arg("--scenario-proof")
            .arg(scenario.path.as_str())
            .arg("--require-semantic-scenario")
            .arg(if scenario.semantic_assertions {
                "true"
            } else {
                "false"
            });
    }
    if let Some(budget) = &requirements.budget {
        command
            .arg("--budget-proof")
            .arg(budget.path.as_str())
            .arg("--required-budget-metrics")
            .arg(
                budget
                    .metrics
                    .iter()
                    .map(BoundedId::as_str)
                    .collect::<Vec<_>>()
                    .join(","),
            );
    }
    if let Some(state_root) = &requirements.state_root {
        command
            .arg("--state-root-policy")
            .arg(state_root.policy.as_str())
            .arg("--restart-required")
            .arg(if state_root.restart_required {
                "true"
            } else {
                "false"
            });
    }
    for checkpoint in &requirements.checkpoints {
        command.arg("--required-checkpoint").arg(
            serde_json::to_string(checkpoint).expect("validated checkpoint requirement serializes"),
        );
    }
}

#[cfg(target_os = "linux")]
fn configure_product_scheduler(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    unsafe {
        command.pre_exec(|| {
            let parameters = libc::sched_param { sched_priority: 0 };
            if libc::setpriority(libc::PRIO_PROCESS, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::sched_setscheduler(0, libc::SCHED_OTHER, &parameters) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_product_scheduler(_command: &mut Command) {}

fn failing_product_report(
    entry: &ManifestGate,
    run_id: BoundedId,
    expected: ExpectedIdentity,
    producer: ProducerEvidence,
    message: String,
) -> ToolResult<GateReport> {
    let blocker = detail(message);
    let mut evidence = empty_evidence(vec![check(
        "producer-v2-evidence",
        CheckOutcome::Fail,
        blocker.as_str(),
    )]);
    evidence.producer = Some(producer);
    Ok(gate_report(
        entry,
        run_id,
        expected,
        ReportStatus::Fail,
        evidence,
        vec![blocker],
    )?)
}

pub fn run_verify_all(
    workspace: &Path,
    check_existing: bool,
    output_override: Option<PathBuf>,
) -> ToolResult<ReportStatus> {
    let (manifest, manifest_digest) = load_manifest(workspace)?;
    let run_id = make_run_id("verify-all")?;
    if !check_existing {
        let executable = std::env::current_exe()?;
        for entry in &manifest.gates {
            let output = workspace_path(workspace, &entry.output);
            remove_file_if_present(&output)?;
            let status = Command::new(&executable)
                .current_dir(workspace)
                .env(RUN_ID_ENV, run_id.as_str())
                .arg(entry.verifier.as_str())
                .arg("--report")
                .arg(entry.output.as_str())
                .status()?;
            if !status.success() {
                eprintln!(
                    "xtask: {} child exited {}; aggregate validation will use its v2 report",
                    entry.verifier.as_str(),
                    status
                );
            }
        }
    }

    let expected = current_identity(workspace)?;
    let mode = if check_existing {
        AggregateMode::CheckExisting
    } else {
        AggregateMode::Fresh
    };
    let mut results = Vec::with_capacity(manifest.gates.len());
    let mut blockers = Vec::new();
    for entry in &manifest.gates {
        let result = validate_child_report(workspace, entry, &expected);
        match (&result.validation, result.outcome) {
            (ChildValidation::Valid, Some(ReportStatus::Pass)) => {}
            (ChildValidation::Valid, Some(ReportStatus::Fail)) => blockers.push(detail(format!(
                "{} produced a valid fail report",
                entry.gate.slug()
            ))),
            _ => blockers.push(
                result
                    .issue
                    .clone()
                    .unwrap_or_else(|| detail(format!("{} report is invalid", entry.gate.slug()))),
            ),
        }
        results.push(result);
    }
    let status = if blockers.is_empty() {
        ReportStatus::Pass
    } else {
        ReportStatus::Fail
    };
    let aggregate = AggregateReport {
        format: FORMAT_VERSION,
        kind: AggregateReportKind::Aggregate,
        identity: AggregateIdentity {
            report_id: make_report_id(&run_id, "verify-all")?,
            run_id,
            source: expected.source.clone(),
            tooling: expected.tooling.clone(),
            generated_unix_ms: unix_time_ms(),
        },
        mode,
        manifest: ManifestIdentity {
            id: manifest.id.clone(),
            digest: manifest_digest.clone(),
        },
        status,
        gates: results,
        blockers,
    };
    aggregate
        .validate(&manifest, &manifest_digest, &expected)
        .map_err(|error| format!("aggregate construction failed: {error}"))?;
    let output =
        output_override.unwrap_or_else(|| workspace_path(workspace, &manifest.aggregate_output));
    if manifest
        .gates
        .iter()
        .any(|entry| output == workspace_path(workspace, &entry.output))
    {
        return Err("aggregate output must not overwrite a gate report".into());
    }
    write_aggregate_report(&output, &aggregate, manifest.aggregate_byte_limit)?;
    println!("wrote {} ({})", output.display(), status_name(status));
    Ok(status)
}

fn validate_child_report(
    workspace: &Path,
    entry: &crate::report_v2::ManifestGate,
    expected: &ExpectedIdentity,
) -> AggregateGateResult {
    let validation = (|| -> ToolResult<(crate::report_v2::ReportFileMetadata, GateReport)> {
        let metadata = report_file_metadata(workspace, &entry.output, entry.byte_limit)?;
        let path = workspace_path(workspace, &entry.output);
        let report = read_gate_report(&path, entry.byte_limit)?;
        report
            .validate_current(entry, expected)
            .map_err(|error| error.to_string())?;
        report
            .validate_artifacts(workspace, entry.sidecar_byte_limit)
            .map_err(|error| error.to_string())?;
        Ok((metadata, report))
    })();
    match validation {
        Ok((metadata, report)) => AggregateGateResult {
            gate: entry.gate.clone(),
            verifier: entry.verifier.clone(),
            report: Some(metadata),
            validation: ChildValidation::Valid,
            outcome: Some(report.status),
            report_id: Some(report.identity.report_id),
            run_id: Some(report.identity.run_id),
            issue: None,
        },
        Err(error) => AggregateGateResult {
            gate: entry.gate.clone(),
            verifier: entry.verifier.clone(),
            report: report_file_metadata(workspace, &entry.output, entry.byte_limit).ok(),
            validation: ChildValidation::Invalid,
            outcome: None,
            report_id: None,
            run_id: None,
            issue: Some(detail(format!(
                "{} report rejected: {error}",
                entry.gate.slug()
            ))),
        },
    }
}

fn status_and_blockers(evidence: &GateEvidence) -> (ReportStatus, Vec<DetailText>) {
    let blockers = evidence
        .checks
        .iter()
        .filter(|check| check.outcome == CheckOutcome::Fail)
        .take(16)
        .map(|check| check.detail.clone())
        .collect::<Vec<_>>();
    let status = if blockers.is_empty() {
        ReportStatus::Pass
    } else {
        ReportStatus::Fail
    };
    (status, blockers)
}

struct ProcessResult {
    success: bool,
    exit_code: Option<i32>,
    elapsed_ms: u64,
    timed_out: bool,
    log_tail: String,
}

fn run_logged(
    command: &mut Command,
    log_path: &Path,
    timeout: Duration,
) -> ToolResult<ProcessResult> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let log = fs::File::create(log_path)?;
    command
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    let started = Instant::now();
    let mut child = command.spawn()?;
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status, false);
        }
        if started.elapsed() >= timeout {
            child.kill()?;
            break (child.wait()?, true);
        }
        thread::sleep(Duration::from_millis(50));
    };
    Ok(ProcessResult {
        success: status.success() && !timed_out,
        exit_code: status.code(),
        elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        timed_out,
        log_tail: read_tail(log_path, 8 * 1024)
            .unwrap_or_else(|error| format!("could not read process log: {error}")),
    })
}

fn read_tail(path: &Path, maximum: usize) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    let start = bytes.len().saturating_sub(maximum);
    Ok(String::from_utf8_lossy(&bytes[start..]).trim().to_owned())
}

fn product_binary(workspace: &Path) -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                workspace.join(path)
            }
        })
        .unwrap_or_else(|| workspace.join("target"));
    target.join("release").join(format!(
        "boon_native_playground{}",
        std::env::consts::EXE_SUFFIX
    ))
}

fn remove_file_if_present(path: &Path) -> ToolResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn timeout_suffix(timed_out: bool) -> &'static str {
    if timed_out { " after timeout" } else { "" }
}

fn status_name(status: ReportStatus) -> &'static str {
    match status {
        ReportStatus::Pass => "pass",
        ReportStatus::Fail => "fail",
    }
}
