use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::compiler_producer::{
    ExpectedProducer, ProducerIdentity, ProducerMetadata, validate_producer_metadata,
};
use crate::compiler_work_sample::require_current_prebuilt_producer;
use crate::report_v2::{
    ExpectedIdentity, ReportStatus, ToolResult, current_identity, sha256_bytes, sha256_file,
    unix_time_ms,
};

const FORMAT_VERSION: u16 = 3;
const PRODUCER_FORMAT_VERSION: u16 = 10;
const CONTRACT: &str = "boon-compiler-allocator-tournament-v3";
const DEFAULT_REPORT: &str = "target/reports/compiler-performance/compiler-allocator.json";
const SYSTEM_PATH: &str = "target/release/boon_cli_system";
const MIMALLOC_PATH: &str = "target/release/boon_cli";
const EVIDENCE_PATH: &str = "target/release/boon_cli_evidence";
const SOURCE: &str = "examples/novywave/RUN.bn";
const MAX_REPORT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Intent {
    Diagnostics,
    Verified,
}

impl Intent {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Diagnostics => "diagnostics",
            Self::Verified => "verified",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum LaunchOrder {
    SystemThenMimalloc,
    MimallocThenSystem,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AllocatorTournamentReport {
    format_version: u16,
    contract: String,
    status: ReportStatus,
    generated_unix_ms: u64,
    identity: ExpectedIdentity,
    source: String,
    seed: u64,
    setup_samples: usize,
    scored_samples: usize,
    schedule: String,
    producers: AllocatorProducerSet,
    workloads: Vec<WorkloadReport>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AllocatorProducerSet {
    system: ProducerIdentity,
    mimalloc: ProducerIdentity,
    evidence: ProducerIdentity,
    system_file_bytes: u64,
    system_text_bytes: u64,
    mimalloc_file_bytes: u64,
    mimalloc_text_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkloadReport {
    intent: Intent,
    status: ReportStatus,
    launch_order: Vec<LaunchOrder>,
    semantic_parity_pass: bool,
    mimalloc_p50_win_pass: bool,
    mimalloc_p95_non_regression_pass: bool,
    rss_bound_pass: bool,
    system: VariantMetric,
    mimalloc: VariantMetric,
    allocation_evidence: AllocationEvidence,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct VariantMetric {
    scored_samples: Vec<TournamentSample>,
    process_to_artifact_ms: MillisSummary,
    compiler_elapsed_ms: MillisSummary,
    compiler_cpu_ms: MillisSummary,
    peak_rss_kib: CountSummary,
    minor_page_faults: CountSummary,
    major_page_faults: CountSummary,
    report_export_ms: MillisSummary,
    source_digests: Vec<String>,
    result_digests: Vec<String>,
    work_digests: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct TournamentSample {
    pair_index: usize,
    process_to_artifact_ms: f64,
    process_exit_ms: f64,
    compiler_elapsed_ms: f64,
    compiler_cpu_ms: f64,
    peak_rss_kib: u64,
    minor_page_faults: u64,
    major_page_faults: u64,
    report_export_ms: f64,
    source_digest: String,
    result_digest: String,
    work_digest: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AllocationEvidence {
    allocation_calls: u64,
    allocated_bytes: u64,
    deallocation_calls: u64,
    deallocated_bytes: u64,
    source_digest: String,
    result_digest: String,
    work_digest: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct MillisSummary {
    sample_count: usize,
    p50: f64,
    p95: f64,
    max: f64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CountSummary {
    sample_count: usize,
    p50: u64,
    p95: u64,
    max: u64,
}

#[derive(Debug, Deserialize)]
struct RawBatch {
    format_version: u16,
    producer: ProducerMetadata,
    source: String,
    intent: Intent,
    compiler_state: String,
    compiler_threads: usize,
    compiler_caches: String,
    samples: Vec<RawSample>,
}

#[derive(Debug, Deserialize)]
struct RawSample {
    compiler_artifact_ready_unix_us: u64,
    elapsed_ms: f64,
    compiler_cpu_ms: f64,
    compiler_minor_page_faults: u64,
    compiler_major_page_faults: u64,
    peak_rss_kib: u64,
    source_bundle_digest_v1: String,
    diagnostics_fingerprint_v1: Option<String>,
    plan_sha256: Option<String>,
    allocations: RawAllocations,
    work: Value,
    export: RawExport,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
struct RawAllocations {
    allocation_calls: u64,
    allocated_bytes: u64,
    deallocation_calls: u64,
    deallocated_bytes: u64,
    allocation_calls_by_ceil_log2_size: [u64; 32],
    allocated_bytes_by_ceil_log2_size: [u64; 32],
    largest_allocation_sizes: [u64; 32],
}

impl RawAllocations {
    fn size_class_totals_match(self) -> bool {
        self.allocation_calls_by_ceil_log2_size
            .iter()
            .try_fold(0_u64, |sum, value| sum.checked_add(*value))
            == Some(self.allocation_calls)
            && self
                .allocated_bytes_by_ceil_log2_size
                .iter()
                .try_fold(0_u64, |sum, value| sum.checked_add(*value))
                == Some(self.allocated_bytes)
    }
}

#[derive(Debug, Deserialize)]
struct RawExport {
    elapsed_ms: f64,
}

struct ExecutedSample {
    metadata: ProducerMetadata,
    sample: TournamentSample,
    allocations: RawAllocations,
}

pub(crate) fn run(
    workspace: &Path,
    check_existing: bool,
    output: Option<PathBuf>,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<ReportStatus> {
    if setup_samples == 0 || scored_samples < 2 || scored_samples > 30 {
        return Err("allocator tournament requires setup >= 1 and scored samples in 2..=30".into());
    }
    let report_path = output.unwrap_or_else(|| workspace.join(DEFAULT_REPORT));
    if !check_existing {
        collect(workspace, &report_path, setup_samples, scored_samples)?;
    }
    let report = validate_existing(workspace, &report_path, setup_samples, scored_samples)?;
    println!(
        "{} compiler allocator tournament {}: diagnostics system/mimalloc p50 {:.3}/{:.3} ms, verified {:.3}/{:.3} ms, status {}",
        if check_existing { "checked" } else { "wrote" },
        report_path.display(),
        report.workloads[0].system.process_to_artifact_ms.p50,
        report.workloads[0].mimalloc.process_to_artifact_ms.p50,
        report.workloads[1].system.process_to_artifact_ms.p50,
        report.workloads[1].mimalloc.process_to_artifact_ms.p50,
        status_name(report.status),
    );
    Ok(report.status)
}

fn collect(
    workspace: &Path,
    report_path: &Path,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<()> {
    let before = current_identity(workspace)?;
    let system_path = workspace.join(SYSTEM_PATH);
    let mimalloc_path = workspace.join(MIMALLOC_PATH);
    let evidence_path = workspace.join(EVIDENCE_PATH);
    for path in [&system_path, &mimalloc_path, &evidence_path] {
        require_current_prebuilt_producer(workspace, path)?;
    }
    let system_digest = sha256_file(&system_path)?.as_str().to_owned();
    let mimalloc_digest = sha256_file(&mimalloc_path)?.as_str().to_owned();
    let evidence_digest = sha256_file(&evidence_path)?.as_str().to_owned();
    let seed = unix_time_ms()
        ^ u64::from_le_bytes(
            mimalloc_digest.as_bytes()[..8]
                .try_into()
                .expect("eight digest bytes"),
        );
    let mut random = XorShift64::new(seed);
    let mut workloads = Vec::with_capacity(2);
    let mut system_metadata = None;
    let mut mimalloc_metadata = None;
    let mut evidence_metadata = None;
    for intent in [Intent::Diagnostics, Intent::Verified] {
        workloads.push(collect_workload(
            workspace,
            &before,
            &system_path,
            &system_digest,
            &mimalloc_path,
            &mimalloc_digest,
            &evidence_path,
            &evidence_digest,
            intent,
            setup_samples,
            scored_samples,
            &mut random,
            &mut system_metadata,
            &mut mimalloc_metadata,
            &mut evidence_metadata,
        )?);
    }
    if sha256_file(&system_path)?.as_str() != system_digest
        || sha256_file(&mimalloc_path)?.as_str() != mimalloc_digest
        || sha256_file(&evidence_path)?.as_str() != evidence_digest
    {
        return Err("an allocator tournament producer changed during collection".into());
    }
    if current_identity(workspace)? != before {
        return Err("workspace identity changed during allocator tournament".into());
    }
    let status = if workloads
        .iter()
        .all(|workload| workload.status == ReportStatus::Pass)
    {
        ReportStatus::Pass
    } else {
        ReportStatus::Fail
    };
    let report = AllocatorTournamentReport {
        format_version: FORMAT_VERSION,
        contract: CONTRACT.to_owned(),
        status,
        generated_unix_ms: unix_time_ms(),
        identity: before,
        source: SOURCE.to_owned(),
        seed,
        setup_samples,
        scored_samples,
        schedule: "seeded-balanced-pairs;one-process-per-variant-per-observation".to_owned(),
        producers: AllocatorProducerSet {
            system: ProducerIdentity {
                path: SYSTEM_PATH.to_owned(),
                sha256: system_digest,
                metadata: system_metadata.ok_or("system producer metadata was not observed")?,
            },
            mimalloc: ProducerIdentity {
                path: MIMALLOC_PATH.to_owned(),
                sha256: mimalloc_digest,
                metadata: mimalloc_metadata.ok_or("mimalloc producer metadata was not observed")?,
            },
            evidence: ProducerIdentity {
                path: EVIDENCE_PATH.to_owned(),
                sha256: evidence_digest,
                metadata: evidence_metadata.ok_or("evidence producer metadata was not observed")?,
            },
            system_file_bytes: fs::metadata(&system_path)?.len(),
            system_text_bytes: elf_text_bytes(&system_path)?,
            mimalloc_file_bytes: fs::metadata(&mimalloc_path)?.len(),
            mimalloc_text_bytes: elf_text_bytes(&mimalloc_path)?,
        },
        workloads,
    };
    validate_report(workspace, &report, setup_samples, scored_samples)?;
    write_report(report_path, &report)
}

#[allow(clippy::too_many_arguments)]
fn collect_workload(
    workspace: &Path,
    identity: &ExpectedIdentity,
    system_path: &Path,
    system_digest: &str,
    mimalloc_path: &Path,
    mimalloc_digest: &str,
    evidence_path: &Path,
    evidence_digest: &str,
    intent: Intent,
    setup_samples: usize,
    scored_samples: usize,
    random: &mut XorShift64,
    system_metadata: &mut Option<ProducerMetadata>,
    mimalloc_metadata: &mut Option<ProducerMetadata>,
    evidence_metadata: &mut Option<ProducerMetadata>,
) -> ToolResult<WorkloadReport> {
    let mut system_samples = Vec::with_capacity(scored_samples);
    let mut mimalloc_samples = Vec::with_capacity(scored_samples);
    let mut launch_order = Vec::with_capacity(setup_samples + scored_samples);
    for pair_index in 0..setup_samples + scored_samples {
        let order = if random.next_bool() {
            LaunchOrder::SystemThenMimalloc
        } else {
            LaunchOrder::MimallocThenSystem
        };
        launch_order.push(order);
        let (system, mimalloc) = match order {
            LaunchOrder::SystemThenMimalloc => (
                execute(workspace, system_path, intent, pair_index)?,
                execute(workspace, mimalloc_path, intent, pair_index)?,
            ),
            LaunchOrder::MimallocThenSystem => {
                let mimalloc = execute(workspace, mimalloc_path, intent, pair_index)?;
                let system = execute(workspace, system_path, intent, pair_index)?;
                (system, mimalloc)
            }
        };
        validate_lane(
            workspace,
            identity,
            &system.metadata,
            SYSTEM_PATH,
            system_digest,
            "linux-system-product",
            "system",
            "none",
            "none",
        )?;
        validate_lane(
            workspace,
            identity,
            &mimalloc.metadata,
            MIMALLOC_PATH,
            mimalloc_digest,
            "linux-mimalloc-product",
            "microsoft-mimalloc",
            "none",
            "none",
        )?;
        observe_metadata(system_metadata, &system.metadata, "system")?;
        observe_metadata(mimalloc_metadata, &mimalloc.metadata, "mimalloc")?;
        if !semantic_sample_eq(&system.sample, &mimalloc.sample) {
            return Err(format!(
                "allocator variants differ for {} pair {pair_index}",
                intent.as_str()
            )
            .into());
        }
        if system.allocations != RawAllocations::default()
            || mimalloc.allocations != RawAllocations::default()
        {
            return Err("uninstrumented allocator tournament lane emitted counters".into());
        }
        if pair_index >= setup_samples {
            system_samples.push(system.sample);
            mimalloc_samples.push(mimalloc.sample);
        }
    }
    let evidence = execute(workspace, evidence_path, intent, usize::MAX)?;
    validate_lane(
        workspace,
        identity,
        &evidence.metadata,
        EVIDENCE_PATH,
        evidence_digest,
        "rust-allocation-evidence",
        "microsoft-mimalloc",
        "thread-local-rust-global-allocator",
        "single-compiler-thread-rust-global-allocator-events",
    )?;
    observe_metadata(evidence_metadata, &evidence.metadata, "evidence")?;
    if evidence.allocations.allocation_calls == 0
        || evidence.allocations.allocated_bytes == 0
        || !semantic_sample_eq(&mimalloc_samples[0], &evidence.sample)
    {
        return Err("allocator evidence lane is empty or semantically different".into());
    }
    let system = variant_metric(system_samples);
    let mimalloc = variant_metric(mimalloc_samples);
    let mimalloc_p50_win_pass =
        mimalloc.process_to_artifact_ms.p50 < system.process_to_artifact_ms.p50;
    let mimalloc_p95_non_regression_pass =
        mimalloc.process_to_artifact_ms.p95 <= system.process_to_artifact_ms.p95;
    let rss_limit_kib = match intent {
        Intent::Diagnostics => 384 * 1024,
        Intent::Verified => 512 * 1024,
    };
    let rss_bound_pass = mimalloc.peak_rss_kib.max <= rss_limit_kib;
    let semantic_parity_pass = true;
    let status = if semantic_parity_pass
        && mimalloc_p50_win_pass
        && mimalloc_p95_non_regression_pass
        && rss_bound_pass
    {
        ReportStatus::Pass
    } else {
        ReportStatus::Fail
    };
    Ok(WorkloadReport {
        intent,
        status,
        launch_order,
        semantic_parity_pass,
        mimalloc_p50_win_pass,
        mimalloc_p95_non_regression_pass,
        rss_bound_pass,
        system,
        mimalloc,
        allocation_evidence: AllocationEvidence {
            allocation_calls: evidence.allocations.allocation_calls,
            allocated_bytes: evidence.allocations.allocated_bytes,
            deallocation_calls: evidence.allocations.deallocation_calls,
            deallocated_bytes: evidence.allocations.deallocated_bytes,
            source_digest: evidence.sample.source_digest,
            result_digest: evidence.sample.result_digest,
            work_digest: evidence.sample.work_digest,
        },
    })
}

fn execute(
    workspace: &Path,
    producer: &Path,
    intent: Intent,
    pair_index: usize,
) -> ToolResult<ExecutedSample> {
    let started_unix_us = unix_time_us()?;
    let started = Instant::now();
    let output = Command::new(producer)
        .current_dir(workspace)
        .env("RAYON_NUM_THREADS", "1")
        .env_remove("BOON_KERNEL_EXPERIMENTAL_PARALLEL")
        .args([
            "compiler-sample",
            SOURCE,
            "--intent",
            intent.as_str(),
            "--mode",
            "fresh-process",
            "--samples",
            "1",
        ])
        .output()?;
    let process_exit_ms = started.elapsed().as_secs_f64() * 1_000.0;
    if !output.status.success() {
        return Err(format!(
            "allocator tournament {} failed: {}",
            producer.display(),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    if output.stdout.is_empty() || output.stdout.len() > MAX_OUTPUT_BYTES {
        return Err("allocator tournament producer output size is invalid".into());
    }
    let batch: RawBatch = serde_json::from_slice(&output.stdout)?;
    if batch.format_version != PRODUCER_FORMAT_VERSION
        || batch.source != SOURCE
        || batch.intent != intent
        || batch.compiler_state != "fresh-process"
        || batch.compiler_threads != 1
        || batch.compiler_caches != "disabled"
        || batch.samples.len() != 1
    {
        return Err("allocator tournament producer contract is invalid".into());
    }
    let raw = batch
        .samples
        .into_iter()
        .next()
        .expect("one sample checked");
    if !raw.allocations.size_class_totals_match() {
        return Err("allocator tournament size-class counters are inconsistent".into());
    }
    let process_to_artifact_ms =
        raw.compiler_artifact_ready_unix_us
            .checked_sub(started_unix_us)
            .ok_or("allocator sample artifact predates process launch")? as f64
            / 1_000.0;
    if process_to_artifact_ms > process_exit_ms + 1.0
        || !finite_nonnegative(raw.elapsed_ms)
        || !finite_nonnegative(raw.compiler_cpu_ms)
        || raw.peak_rss_kib == 0
    {
        return Err("allocator sample timing or RSS evidence is invalid".into());
    }
    let result_digest = match intent {
        Intent::Diagnostics => raw
            .diagnostics_fingerprint_v1
            .ok_or("diagnostics allocator sample omitted its fingerprint")?,
        Intent::Verified => raw
            .plan_sha256
            .ok_or("verified allocator sample omitted its plan hash")?,
    };
    let work_digest = sha256_bytes(&serde_json::to_vec(&raw.work)?)
        .as_str()
        .to_owned();
    Ok(ExecutedSample {
        metadata: batch.producer,
        allocations: raw.allocations,
        sample: TournamentSample {
            pair_index,
            process_to_artifact_ms,
            process_exit_ms,
            compiler_elapsed_ms: raw.elapsed_ms,
            compiler_cpu_ms: raw.compiler_cpu_ms,
            peak_rss_kib: raw.peak_rss_kib,
            minor_page_faults: raw.compiler_minor_page_faults,
            major_page_faults: raw.compiler_major_page_faults,
            report_export_ms: raw.export.elapsed_ms,
            source_digest: raw.source_bundle_digest_v1,
            result_digest,
            work_digest,
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_lane(
    workspace: &Path,
    identity: &ExpectedIdentity,
    metadata: &ProducerMetadata,
    path: &str,
    digest: &str,
    kind: &str,
    allocator: &str,
    instrumentation: &str,
    counter_scope: &str,
) -> ToolResult<()> {
    validate_producer_metadata(
        workspace,
        identity,
        metadata,
        ExpectedProducer {
            path,
            sha256: digest,
            product_kind: kind,
            allocator_id: allocator,
            allocation_instrumentation: instrumentation,
            allocation_counter_scope: counter_scope,
            cargo_profile: "release",
            target_cpu: "generic",
            profile_options: "opt-level=3;lto=off;codegen-units=16;debug-assertions=off;overflow-checks=off",
            rustflags: "",
        },
    )
}

fn observe_metadata(
    observed: &mut Option<ProducerMetadata>,
    candidate: &ProducerMetadata,
    lane: &str,
) -> ToolResult<()> {
    match observed {
        Some(previous) if previous != candidate => {
            Err(format!("allocator tournament {lane} metadata changed").into())
        }
        Some(_) => Ok(()),
        None => {
            *observed = Some(candidate.clone());
            Ok(())
        }
    }
}

fn semantic_sample_eq(left: &TournamentSample, right: &TournamentSample) -> bool {
    left.source_digest == right.source_digest
        && left.result_digest == right.result_digest
        && left.work_digest == right.work_digest
}

fn variant_metric(scored_samples: Vec<TournamentSample>) -> VariantMetric {
    let ms = |field: fn(&TournamentSample) -> f64| {
        summarize_ms(scored_samples.iter().map(field).collect())
    };
    let counts = |field: fn(&TournamentSample) -> u64| {
        summarize_counts(scored_samples.iter().map(field).collect())
    };
    let digests = |field: fn(&TournamentSample) -> &String| {
        scored_samples
            .iter()
            .map(field)
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    VariantMetric {
        process_to_artifact_ms: ms(|sample| sample.process_to_artifact_ms),
        compiler_elapsed_ms: ms(|sample| sample.compiler_elapsed_ms),
        compiler_cpu_ms: ms(|sample| sample.compiler_cpu_ms),
        peak_rss_kib: counts(|sample| sample.peak_rss_kib),
        minor_page_faults: counts(|sample| sample.minor_page_faults),
        major_page_faults: counts(|sample| sample.major_page_faults),
        report_export_ms: ms(|sample| sample.report_export_ms),
        source_digests: digests(|sample| &sample.source_digest),
        result_digests: digests(|sample| &sample.result_digest),
        work_digests: digests(|sample| &sample.work_digest),
        scored_samples,
    }
}

fn validate_existing(
    workspace: &Path,
    path: &Path,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<AllocatorTournamentReport> {
    let bytes = fs::read(path)?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err("allocator tournament report size is invalid".into());
    }
    let report: AllocatorTournamentReport = serde_json::from_slice(&bytes)?;
    validate_report(workspace, &report, setup_samples, scored_samples)?;
    Ok(report)
}

fn validate_report(
    workspace: &Path,
    report: &AllocatorTournamentReport,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<()> {
    if report.format_version != FORMAT_VERSION
        || report.contract != CONTRACT
        || report.identity != current_identity(workspace)?
        || report.source != SOURCE
        || report.setup_samples != setup_samples
        || report.scored_samples != scored_samples
        || report.workloads.len() != 2
        || report.workloads[0].intent != Intent::Diagnostics
        || report.workloads[1].intent != Intent::Verified
    {
        return Err("allocator tournament report identity or sampling shape is stale".into());
    }
    for (producer, path, kind, allocator, instrumentation, counter_scope) in [
        (
            &report.producers.system,
            SYSTEM_PATH,
            "linux-system-product",
            "system",
            "none",
            "none",
        ),
        (
            &report.producers.mimalloc,
            MIMALLOC_PATH,
            "linux-mimalloc-product",
            "microsoft-mimalloc",
            "none",
            "none",
        ),
        (
            &report.producers.evidence,
            EVIDENCE_PATH,
            "rust-allocation-evidence",
            "microsoft-mimalloc",
            "thread-local-rust-global-allocator",
            "single-compiler-thread-rust-global-allocator-events",
        ),
    ] {
        let binary = workspace.join(path);
        require_current_prebuilt_producer(workspace, &binary)?;
        let digest = sha256_file(&binary)?;
        if producer.path != path || producer.sha256 != digest.as_str() {
            return Err(format!("allocator tournament producer {path} is stale").into());
        }
        validate_lane(
            workspace,
            &report.identity,
            &producer.metadata,
            path,
            digest.as_str(),
            kind,
            allocator,
            instrumentation,
            counter_scope,
        )?;
    }
    let expected_sizes = (
        fs::metadata(workspace.join(SYSTEM_PATH))?.len(),
        elf_text_bytes(&workspace.join(SYSTEM_PATH))?,
        fs::metadata(workspace.join(MIMALLOC_PATH))?.len(),
        elf_text_bytes(&workspace.join(MIMALLOC_PATH))?,
    );
    if (
        report.producers.system_file_bytes,
        report.producers.system_text_bytes,
        report.producers.mimalloc_file_bytes,
        report.producers.mimalloc_text_bytes,
    ) != expected_sizes
    {
        return Err("allocator tournament binary size evidence is stale".into());
    }
    let mut random = XorShift64::new(report.seed);
    for workload in &report.workloads {
        let expected_order = (0..setup_samples + scored_samples)
            .map(|_| {
                if random.next_bool() {
                    LaunchOrder::SystemThenMimalloc
                } else {
                    LaunchOrder::MimallocThenSystem
                }
            })
            .collect::<Vec<_>>();
        if workload.launch_order != expected_order
            || workload.system.scored_samples.len() != scored_samples
            || workload.mimalloc.scored_samples.len() != scored_samples
            || !workload.semantic_parity_pass
            || workload.allocation_evidence.allocation_calls == 0
            || workload.allocation_evidence.allocated_bytes == 0
        {
            return Err("allocator tournament workload evidence is inconsistent".into());
        }
        for (system, mimalloc) in workload
            .system
            .scored_samples
            .iter()
            .zip(&workload.mimalloc.scored_samples)
        {
            if !semantic_sample_eq(system, mimalloc) {
                return Err("stored allocator samples lack semantic parity".into());
            }
        }
        if variant_metric(workload.system.scored_samples.clone()) != workload.system
            || variant_metric(workload.mimalloc.scored_samples.clone()) != workload.mimalloc
        {
            return Err("allocator tournament summaries are inconsistent".into());
        }
        let expected_status = workload.semantic_parity_pass
            && workload.mimalloc_p50_win_pass
            && workload.mimalloc_p95_non_regression_pass
            && workload.rss_bound_pass;
        if (workload.status == ReportStatus::Pass) != expected_status {
            return Err("allocator workload status is inconsistent".into());
        }
    }
    let expected_status = if report
        .workloads
        .iter()
        .all(|workload| workload.status == ReportStatus::Pass)
    {
        ReportStatus::Pass
    } else {
        ReportStatus::Fail
    };
    if report.status != expected_status {
        return Err("allocator tournament aggregate status is inconsistent".into());
    }
    Ok(())
}

fn summarize_ms(mut values: Vec<f64>) -> MillisSummary {
    if values.is_empty() {
        return MillisSummary::default();
    }
    values.sort_by(f64::total_cmp);
    MillisSummary {
        sample_count: values.len(),
        p50: nearest_rank(&values, 50),
        p95: nearest_rank(&values, 95),
        max: *values.last().expect("non-empty"),
    }
}

fn summarize_counts(mut values: Vec<u64>) -> CountSummary {
    if values.is_empty() {
        return CountSummary::default();
    }
    values.sort_unstable();
    CountSummary {
        sample_count: values.len(),
        p50: nearest_rank(&values, 50),
        p95: nearest_rank(&values, 95),
        max: *values.last().expect("non-empty"),
    }
}

fn nearest_rank<T: Copy>(sorted: &[T], percentile: usize) -> T {
    let rank = sorted.len().saturating_mul(percentile).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn elf_text_bytes(path: &Path) -> ToolResult<u64> {
    let output = Command::new("size").args(["-A"]).arg(path).output()?;
    if !output.status.success() {
        return Err(format!("read ELF section sizes for {}", path.display()).into());
    }
    let stdout = std::str::from_utf8(&output.stdout)?;
    stdout
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next() == Some(".text"))
                .then(|| fields.next()?.parse::<u64>().ok())
                .flatten()
        })
        .ok_or_else(|| format!("ELF {} has no .text section", path.display()).into())
}

fn unix_time_us() -> ToolResult<u64> {
    Ok(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros(),
    )?)
}

fn finite_nonnegative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

fn write_report(path: &Path, report: &AllocatorTournamentReport) -> ToolResult<()> {
    let parent = path.parent().ok_or("allocator report has no parent")?;
    fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec_pretty(report)?;
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err("allocator report exceeds byte budget".into());
    }
    let temporary = path.with_extension(format!("allocator-tmp-{}", std::process::id()));
    fs::write(&temporary, &bytes)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn status_name(status: ReportStatus) -> &'static str {
    match status {
        ReportStatus::Pass => "pass",
        ReportStatus::Fail => "fail",
    }
}

struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_bool(&mut self) -> bool {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value & 1 == 1
    }
}
