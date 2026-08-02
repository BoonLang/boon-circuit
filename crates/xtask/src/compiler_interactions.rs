use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

use crate::report_v2::{
    ExpectedIdentity, ReportStatus, ToolResult, current_identity, sha256_bytes, sha256_file,
    unix_time_ms,
};

const FORMAT_VERSION: u16 = 1;
const PRODUCER_FORMAT_VERSION: u16 = 2;
const BUDGET_FORMAT_VERSION: u16 = 1;
const REPORT_CONTRACT: &str = "boon-compiler-interactions-v1";
const DEFAULT_BUDGET: &str = "budgets/compiler.toml";
const PRODUCER_PATH: &str = "target/release/boon_cli";
const MAX_BUDGET_BYTES: u64 = 64 * 1024;
const MAX_REPORT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_SAMPLE_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_SAMPLE_COUNT: usize = 128;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetManifest {
    format_version: u16,
    owner_plan: String,
    report: String,
    protocol: BudgetProtocol,
    fixtures: Vec<toml::Value>,
    warm: WarmBudget,
    scaling: ScalingBudget,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetProtocol {
    build_profile: String,
    target_profile: String,
    setup_samples: usize,
    scored_samples: usize,
    compiler_threads: usize,
    compiler_caches: String,
    cold_modes: Vec<String>,
    os_page_cache: String,
    sample_process_isolation: String,
    peak_rss_unit: String,
    peak_rss_scope: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WarmBudget {
    report: String,
    source: String,
    switch_source: String,
    edit_unit: String,
    edit_from: String,
    edit_to: String,
    checked_diagnostics_p95_ms: f64,
    checked_diagnostics_p99_ms: f64,
    checked_diagnostics_max_ms: f64,
    verified_preview_p95_ms: f64,
    verified_preview_max_ms: f64,
    loaded_bundle_lookup_max_ms: f64,
    switch_ack_p95_ms: f64,
    switch_present_p95_ms: f64,
    switch_present_max_ms: f64,
    cancellation_max_ms: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScalingBudget {
    maximum_doubling_ratio: f64,
    dimensions: Vec<String>,
    workloads: Vec<ScalingWorkloadBudget>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScalingWorkloadBudget {
    id: String,
    intent: SampleIntent,
    owning_work_counter: String,
    baseline_size: usize,
    base_size: usize,
    doubled_size: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum RunClassification {
    Acceptance,
    DevelopmentNonAcceptance,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SampleIntent {
    Diagnostics,
    Verified,
}

impl SampleIntent {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Diagnostics => "diagnostics",
            Self::Verified => "verified",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CompilerInteractionsReport {
    format_version: u16,
    contract: String,
    status: ReportStatus,
    run_classification: RunClassification,
    generated_unix_ms: u64,
    identity: ExpectedIdentity,
    budget: BudgetIdentity,
    producer: ProducerIdentity,
    protocol: ProtocolEvidence,
    warm: WarmReport,
    scaling: Vec<ScalingReport>,
    missing_acceptance_evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct BudgetIdentity {
    path: String,
    sha256: String,
    format_version: u16,
    owner_plan: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ProducerIdentity {
    path: String,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ProtocolEvidence {
    default_setup_samples: usize,
    default_scored_samples: usize,
    effective_setup_samples: usize,
    effective_scored_samples: usize,
    compiler_threads: usize,
    compiler_caches: String,
    percentile_method: String,
    scaling_process_isolation: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WarmReport {
    status: ReportStatus,
    raw: WarmSessionBatch,
    diagnostics_edit_to_ready_ms: MillisSummary,
    verified_preview_edit_to_ready_ms: MillisSummary,
    update_ack_ms: MillisSummary,
    loaded_bundle_lookup_ms: MillisSummary,
    switch_ack_ms: MillisSummary,
    cancellation_stop_ms: f64,
    latest_generation_publish_ms: f64,
    evaluation: WarmEvaluation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WarmEvaluation {
    diagnostics_p95_pass: bool,
    diagnostics_p99_pass: bool,
    diagnostics_max_pass: bool,
    verified_preview_p95_pass: bool,
    verified_preview_max_pass: bool,
    loaded_bundle_lookup_pass: bool,
    switch_ack_pass: bool,
    switch_no_compile_pass: bool,
    switch_no_allocation_pass: bool,
    pre_canceled_request_pass: bool,
    latest_generation_pass: bool,
    in_flight_supersession_supported: bool,
    full_cancellation_gate_pass: bool,
    native_present_evidence: String,
    native_present_gate_pass: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ScalingReport {
    id: String,
    intent: SampleIntent,
    owning_work_counter: String,
    maximum_doubling_ratio: f64,
    status: ReportStatus,
    baseline: ScalingPoint,
    base: ScalingPoint,
    doubled: ScalingPoint,
    elapsed_ms_fixed_overhead_ratio: Option<f64>,
    allocation_calls_fixed_overhead_ratio: Option<f64>,
    allocated_bytes_fixed_overhead_ratio: Option<f64>,
    owning_work_fixed_overhead_ratio: Option<f64>,
    evaluation: ScalingEvaluation,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ScalingPoint {
    size: usize,
    setup_producer_pids: Vec<u32>,
    scored_samples: Vec<SyntheticScalingBatch>,
    elapsed_ms: MillisSummary,
    allocation_calls: CountSummary,
    allocated_bytes: CountSummary,
    parsed_expressions: CountSummary,
    checked_calls: CountSummary,
    semantic_graph_nodes: CountSummary,
    dependency_scc: Option<SccTraceEvidence>,
    source_sha256: String,
    source_bundle_digest_v1: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ScalingEvaluation {
    source_identity_pass: bool,
    process_isolation_pass: bool,
    allocation_calls_ratio_pass: bool,
    allocated_bytes_ratio_pass: bool,
    owning_work_ratio_pass: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct MillisSummary {
    sample_count: usize,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CountSummary {
    sample_count: usize,
    p50: u64,
    p95: u64,
    p99: u64,
    max: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WarmSessionBatch {
    format_version: u16,
    workload: String,
    source: String,
    switch_source: String,
    edit_unit: String,
    producer_pid: u32,
    compiler_threads: usize,
    compiler_caches: String,
    setup_samples: usize,
    scored_samples: usize,
    primary_project_id: u64,
    switch_project_id: u64,
    base_revision: u64,
    initial_source_bundle_digest_v1: String,
    initial_plan_sha256: String,
    switch_plan_sha256: String,
    original_unit_sha256: String,
    edited_unit_sha256: String,
    compiler_request_count: u64,
    edits: Vec<WarmEditSample>,
    switches: Vec<LoadedSwitchSample>,
    cancellation: CancellationEvidence,
    latest_generation: LatestGenerationEvidence,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WarmEditSample {
    sequence: usize,
    scored: bool,
    direction: String,
    previous_revision: u64,
    revision: u64,
    last_good_revision_before: u64,
    update_ack_ms: f64,
    diagnostics_request_ms: f64,
    edit_to_diagnostics_ms: f64,
    verified_preview_request_ms: f64,
    edit_to_verified_preview_ms: f64,
    diagnostic_count: usize,
    full_document_typecheck_coverage: bool,
    source_bundle_digest_v1: String,
    plan_sha256: String,
    published_revision: u64,
    diagnostics_allocations: AllocationSample,
    preview_allocations: AllocationSample,
    diagnostics_work: WorkSample,
    preview_work: WorkSample,
    diagnostics_phase: PhaseSample,
    preview_phase: PhaseSample,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LoadedSwitchSample {
    sequence: usize,
    scored: bool,
    from_project_id: u64,
    to_project_id: u64,
    selected_revision: u64,
    acknowledgement_ms: f64,
    loaded_bundle_lookup_ms: f64,
    allocation_calls: u64,
    allocated_bytes: u64,
    compiler_requests_before: u64,
    compiler_requests_after: u64,
    selected_plan_sha256: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CancellationEvidence {
    scope: String,
    revision: u64,
    token_canceled_before_request: bool,
    request_rejected: bool,
    stop_latency_ms: f64,
    last_good_revision_before: u64,
    last_good_revision_after: u64,
    publication_unchanged: bool,
    in_flight_supersession_supported: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LatestGenerationEvidence {
    stale_revision: u64,
    latest_revision: u64,
    stale_request_rejected: bool,
    last_good_revision_after_stale_request: u64,
    published_revision: u64,
    publish_latest_ms: f64,
    no_stale_publication: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SyntheticScalingBatch {
    format_version: u16,
    workload: String,
    generator: String,
    dimension: String,
    size: usize,
    intent: SampleIntent,
    producer_pid: u32,
    compiler_threads: usize,
    compiler_caches: String,
    revision: u64,
    synthetic_source_sha256: String,
    source_bundle_digest_v1: String,
    elapsed_ms: f64,
    peak_rss_kib: u64,
    plan_sha256: Option<String>,
    allocations: AllocationSample,
    work: WorkSample,
    phase: PhaseSample,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AllocationSample {
    allocation_calls: u64,
    allocated_bytes: u64,
    deallocation_calls: u64,
    deallocated_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkSample {
    source_units: usize,
    parsed_expressions: usize,
    checked_expressions: usize,
    checked_calls: usize,
    semantic_graph_nodes: usize,
    cancellation_checkpoints: usize,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PhaseSample {
    parse_ms: f64,
    typecheck_ms: f64,
    semantic_ms: f64,
    contract_verify_ms: f64,
    ir_lower_ms: f64,
    ir_validation_ms: f64,
    backend_ms: f64,
    plan_validation_ms: f64,
    serialization_ms: f64,
}

impl PhaseSample {
    const fn values(self) -> [f64; 9] {
        [
            self.parse_ms,
            self.typecheck_ms,
            self.semantic_ms,
            self.contract_verify_ms,
            self.ir_lower_ms,
            self.ir_validation_ms,
            self.backend_ms,
            self.plan_validation_ms,
            self.serialization_ms,
        ]
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SccTraceEvidence {
    nodes: u64,
    edges: u64,
    components: u64,
    cyclic_components: u64,
    maximum_component_nodes: u64,
    component_edges: u64,
}

pub fn run(
    workspace: &Path,
    check_existing: bool,
    output: Option<PathBuf>,
    setup_override: Option<usize>,
    scored_override: Option<usize>,
) -> ToolResult<ReportStatus> {
    let (budget, budget_digest) = load_budget(workspace)?;
    let setup_samples = setup_override.unwrap_or(budget.protocol.setup_samples);
    let scored_samples = scored_override.unwrap_or(budget.protocol.scored_samples);
    validate_effective_samples(setup_samples, scored_samples)?;
    let report_path = match output {
        Some(path) => path,
        None => workspace.join(safe_relative_path(&budget.warm.report, "warm report path")?),
    };
    if !check_existing {
        collect(
            workspace,
            &report_path,
            &budget,
            &budget_digest,
            setup_samples,
            scored_samples,
        )?;
    }
    let report = validate_existing(
        workspace,
        &report_path,
        &budget,
        &budget_digest,
        setup_samples,
        scored_samples,
    )?;
    println!(
        "{} compiler interactions {}: warm {}, {} scaling dimensions, aggregate {} ({})",
        if check_existing { "checked" } else { "wrote" },
        report_path.display(),
        status_name(report.warm.status),
        report.scaling.len(),
        status_name(report.status),
        if report.missing_acceptance_evidence.is_empty() {
            "acceptance evidence complete"
        } else {
            "missing native-presentation and in-flight-supersession evidence"
        }
    );
    Ok(report.status)
}

fn collect(
    workspace: &Path,
    report_path: &Path,
    budget: &BudgetManifest,
    budget_digest: &str,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<()> {
    build_producer(workspace)?;
    let producer_path = workspace.join(PRODUCER_PATH);
    if !producer_path.is_file() {
        return Err(format!(
            "compiler interaction producer is missing at {}",
            producer_path.display()
        )
        .into());
    }
    let before = current_identity(workspace)?;
    let producer_digest = sha256_file(&producer_path)?.as_str().to_owned();
    let warm_batch = run_warm_batch(
        workspace,
        &producer_path,
        &budget.warm,
        setup_samples,
        scored_samples,
    )?;
    let warm = warm_report(warm_batch, &budget.warm)?;
    let mut scaling = Vec::with_capacity(budget.scaling.workloads.len());
    for workload in &budget.scaling.workloads {
        scaling.push(collect_scaling(
            workspace,
            &producer_path,
            workload,
            budget.scaling.maximum_doubling_ratio,
            setup_samples,
            scored_samples,
        )?);
    }
    if sha256_file(&producer_path)?.as_str() != producer_digest {
        return Err("compiler interaction producer changed during measurement".into());
    }
    let after = current_identity(workspace)?;
    if before != after {
        return Err("workspace identity changed during compiler interaction measurement".into());
    }

    let missing_acceptance_evidence = vec![
        "native-presented-frame: compiler-only harness cannot replace app-owned WGPU readback"
            .to_owned(),
        "in-flight-supersession: synchronous CompilerSession exposes no worker generation handle"
            .to_owned(),
    ];
    let status = aggregate_status(&warm, &scaling, &missing_acceptance_evidence);
    let report = CompilerInteractionsReport {
        format_version: FORMAT_VERSION,
        contract: REPORT_CONTRACT.to_owned(),
        status,
        run_classification: classification(budget, setup_samples, scored_samples),
        generated_unix_ms: unix_time_ms(),
        identity: before,
        budget: BudgetIdentity {
            path: DEFAULT_BUDGET.to_owned(),
            sha256: budget_digest.to_owned(),
            format_version: budget.format_version,
            owner_plan: budget.owner_plan.clone(),
        },
        producer: ProducerIdentity {
            path: PRODUCER_PATH.to_owned(),
            sha256: producer_digest,
        },
        protocol: protocol_evidence(budget, setup_samples, scored_samples),
        warm,
        scaling,
        missing_acceptance_evidence,
    };
    validate_report(
        workspace,
        &report,
        budget,
        budget_digest,
        setup_samples,
        scored_samples,
    )?;
    write_report(report_path, &report)
}

fn build_producer(workspace: &Path) -> ToolResult<()> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let status = Command::new(cargo)
        .current_dir(workspace)
        .args([
            "build",
            "--locked",
            "--release",
            "--jobs",
            "1",
            "-p",
            "boon_cli",
            "--bin",
            "boon_cli",
        ])
        .status()?;
    if !status.success() {
        return Err(
            format!("release compiler interaction producer build failed with {status}").into(),
        );
    }
    Ok(())
}

fn run_warm_batch(
    workspace: &Path,
    producer: &Path,
    budget: &WarmBudget,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<WarmSessionBatch> {
    let setup = setup_samples.to_string();
    let scored = scored_samples.to_string();
    let output = Command::new(producer)
        .current_dir(workspace)
        .env("RAYON_NUM_THREADS", "1")
        .args([
            "compiler-sample",
            "warm-session",
            budget.source.as_str(),
            "--switch-source",
            budget.switch_source.as_str(),
            "--edit-unit",
            budget.edit_unit.as_str(),
            "--edit-from",
            budget.edit_from.as_str(),
            "--edit-to",
            budget.edit_to.as_str(),
            "--setup-samples",
            setup.as_str(),
            "--scored-samples",
            scored.as_str(),
        ])
        .output()?;
    let batch: WarmSessionBatch = parse_output_json(&output, "warm compiler interaction")?;
    validate_warm_batch(workspace, &batch, budget, setup_samples, scored_samples)?;
    Ok(batch)
}

fn warm_report(raw: WarmSessionBatch, budget: &WarmBudget) -> ToolResult<WarmReport> {
    let scored_edits = raw
        .edits
        .iter()
        .filter(|sample| sample.scored)
        .collect::<Vec<_>>();
    let scored_switches = raw
        .switches
        .iter()
        .filter(|sample| sample.scored)
        .collect::<Vec<_>>();
    let diagnostics_edit_to_ready_ms = summarize_ms(
        scored_edits
            .iter()
            .map(|sample| sample.edit_to_diagnostics_ms)
            .collect(),
    );
    let verified_preview_edit_to_ready_ms = summarize_ms(
        scored_edits
            .iter()
            .map(|sample| sample.edit_to_verified_preview_ms)
            .collect(),
    );
    let update_ack_ms = summarize_ms(
        scored_edits
            .iter()
            .map(|sample| sample.update_ack_ms)
            .collect(),
    );
    let loaded_bundle_lookup_ms = summarize_ms(
        scored_switches
            .iter()
            .map(|sample| sample.loaded_bundle_lookup_ms)
            .collect(),
    );
    let switch_ack_ms = summarize_ms(
        scored_switches
            .iter()
            .map(|sample| sample.acknowledgement_ms)
            .collect(),
    );
    let evaluation = warm_evaluation(
        &raw,
        budget,
        diagnostics_edit_to_ready_ms,
        verified_preview_edit_to_ready_ms,
        loaded_bundle_lookup_ms,
        switch_ack_ms,
    );
    let status = warm_status(&evaluation);
    Ok(WarmReport {
        status,
        cancellation_stop_ms: raw.cancellation.stop_latency_ms,
        latest_generation_publish_ms: raw.latest_generation.publish_latest_ms,
        raw,
        diagnostics_edit_to_ready_ms,
        verified_preview_edit_to_ready_ms,
        update_ack_ms,
        loaded_bundle_lookup_ms,
        switch_ack_ms,
        evaluation,
    })
}

fn warm_evaluation(
    raw: &WarmSessionBatch,
    budget: &WarmBudget,
    diagnostics: MillisSummary,
    preview: MillisSummary,
    lookup: MillisSummary,
    switch_ack: MillisSummary,
) -> WarmEvaluation {
    let switch_no_compile_pass = raw
        .switches
        .iter()
        .filter(|sample| sample.scored)
        .all(|sample| sample.compiler_requests_before == sample.compiler_requests_after);
    let switch_no_allocation_pass = raw
        .switches
        .iter()
        .filter(|sample| sample.scored)
        .all(|sample| sample.allocation_calls == 0 && sample.allocated_bytes == 0);
    let pre_canceled_request_pass = raw.cancellation.scope == "pre-canceled-request"
        && raw.cancellation.token_canceled_before_request
        && raw.cancellation.request_rejected
        && raw.cancellation.publication_unchanged
        && raw.cancellation.stop_latency_ms <= budget.cancellation_max_ms;
    WarmEvaluation {
        diagnostics_p95_pass: diagnostics.p95 <= budget.checked_diagnostics_p95_ms,
        diagnostics_p99_pass: diagnostics.p99 <= budget.checked_diagnostics_p99_ms,
        diagnostics_max_pass: diagnostics.max <= budget.checked_diagnostics_max_ms,
        verified_preview_p95_pass: preview.p95 <= budget.verified_preview_p95_ms,
        verified_preview_max_pass: preview.max <= budget.verified_preview_max_ms,
        loaded_bundle_lookup_pass: lookup.max <= budget.loaded_bundle_lookup_max_ms,
        switch_ack_pass: switch_ack.p95 <= budget.switch_ack_p95_ms,
        switch_no_compile_pass,
        switch_no_allocation_pass,
        pre_canceled_request_pass,
        latest_generation_pass: raw.latest_generation.stale_request_rejected
            && raw.latest_generation.no_stale_publication
            && raw.latest_generation.published_revision == raw.latest_generation.latest_revision,
        in_flight_supersession_supported: raw.cancellation.in_flight_supersession_supported,
        // A pre-canceled synchronous call is useful preflight evidence but is
        // not the planned "generation superseded while working" gate.
        full_cancellation_gate_pass: false,
        native_present_evidence: "not-measured-compiler-only".to_owned(),
        native_present_gate_pass: false,
    }
}

fn warm_status(evaluation: &WarmEvaluation) -> ReportStatus {
    let passes = evaluation.diagnostics_p95_pass
        && evaluation.diagnostics_p99_pass
        && evaluation.diagnostics_max_pass
        && evaluation.verified_preview_p95_pass
        && evaluation.verified_preview_max_pass
        && evaluation.loaded_bundle_lookup_pass
        && evaluation.switch_ack_pass
        && evaluation.switch_no_compile_pass
        && evaluation.switch_no_allocation_pass
        && evaluation.pre_canceled_request_pass
        && evaluation.latest_generation_pass
        && evaluation.in_flight_supersession_supported
        && evaluation.full_cancellation_gate_pass
        && evaluation.native_present_gate_pass;
    if passes {
        ReportStatus::Pass
    } else {
        ReportStatus::Fail
    }
}

fn collect_scaling(
    workspace: &Path,
    producer: &Path,
    budget: &ScalingWorkloadBudget,
    maximum_ratio: f64,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<ScalingReport> {
    let baseline = collect_scaling_point(
        workspace,
        producer,
        budget,
        budget.baseline_size,
        setup_samples,
        scored_samples,
    )?;
    let base = collect_scaling_point(
        workspace,
        producer,
        budget,
        budget.base_size,
        setup_samples,
        scored_samples,
    )?;
    let doubled = collect_scaling_point(
        workspace,
        producer,
        budget,
        budget.doubled_size,
        setup_samples,
        scored_samples,
    )?;
    scaling_report(budget, maximum_ratio, baseline, base, doubled)
}

fn collect_scaling_point(
    workspace: &Path,
    producer: &Path,
    budget: &ScalingWorkloadBudget,
    size: usize,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<ScalingPoint> {
    let mut setup_producer_pids = Vec::with_capacity(setup_samples);
    for _ in 0..setup_samples {
        let sample = run_scaling_sample(workspace, producer, budget, size, false)?;
        setup_producer_pids.push(sample.producer_pid);
    }
    let mut scored = Vec::with_capacity(scored_samples);
    for _ in 0..scored_samples {
        scored.push(run_scaling_sample(
            workspace, producer, budget, size, false,
        )?);
    }
    let trace = if budget.owning_work_counter == "dependency-scc-components" {
        let (_, trace) = run_scaling_trace(workspace, producer, budget, size)?;
        Some(trace)
    } else {
        None
    };
    scaling_point(size, setup_producer_pids, scored, trace)
}

fn run_scaling_sample(
    workspace: &Path,
    producer: &Path,
    budget: &ScalingWorkloadBudget,
    size: usize,
    trace: bool,
) -> ToolResult<SyntheticScalingBatch> {
    let size_text = size.to_string();
    let mut command = Command::new(producer);
    command
        .current_dir(workspace)
        .env("RAYON_NUM_THREADS", "1")
        .args([
            "compiler-sample",
            "synthetic-scaling",
            budget.id.as_str(),
            "--size",
            size_text.as_str(),
            "--intent",
            budget.intent.as_str(),
        ]);
    if trace {
        command.env("BOON_SEMANTIC_TRACE", "1");
    } else {
        command.env_remove("BOON_SEMANTIC_TRACE");
    }
    let output = command.output()?;
    let sample: SyntheticScalingBatch = parse_output_json(
        &output,
        &format!("synthetic scaling {}/{}", budget.id, size),
    )?;
    validate_scaling_sample(&sample, budget, size)?;
    Ok(sample)
}

fn run_scaling_trace(
    workspace: &Path,
    producer: &Path,
    budget: &ScalingWorkloadBudget,
    size: usize,
) -> ToolResult<(SyntheticScalingBatch, SccTraceEvidence)> {
    let size_text = size.to_string();
    let output = Command::new(producer)
        .current_dir(workspace)
        .env("RAYON_NUM_THREADS", "1")
        .env("BOON_SEMANTIC_TRACE", "1")
        .args([
            "compiler-sample",
            "synthetic-scaling",
            budget.id.as_str(),
            "--size",
            size_text.as_str(),
            "--intent",
            budget.intent.as_str(),
        ])
        .output()?;
    let sample: SyntheticScalingBatch = parse_output_json(
        &output,
        &format!("synthetic scaling trace {}/{}", budget.id, size),
    )?;
    validate_scaling_sample(&sample, budget, size)?;
    let trace = parse_scc_trace(&output.stderr)?;
    Ok((sample, trace))
}

fn scaling_point(
    size: usize,
    setup_producer_pids: Vec<u32>,
    scored_samples: Vec<SyntheticScalingBatch>,
    trace: Option<SccTraceEvidence>,
) -> ToolResult<ScalingPoint> {
    let source_sha256 = one_digest(
        scored_samples
            .iter()
            .map(|sample| sample.synthetic_source_sha256.as_str()),
        "synthetic source",
    )?;
    let source_bundle_digest_v1 = one_digest(
        scored_samples
            .iter()
            .map(|sample| sample.source_bundle_digest_v1.as_str()),
        "synthetic source bundle",
    )?;
    let elapsed_ms = summarize_ms(
        scored_samples
            .iter()
            .map(|sample| sample.elapsed_ms)
            .collect(),
    );
    let allocation_calls = summarize_counts(
        scored_samples
            .iter()
            .map(|sample| sample.allocations.allocation_calls)
            .collect(),
    );
    let allocated_bytes = summarize_counts(
        scored_samples
            .iter()
            .map(|sample| sample.allocations.allocated_bytes)
            .collect(),
    );
    let checked_calls = summarize_counts(
        scored_samples
            .iter()
            .map(|sample| sample.work.checked_calls as u64)
            .collect(),
    );
    let parsed_expressions = summarize_counts(
        scored_samples
            .iter()
            .map(|sample| sample.work.parsed_expressions as u64)
            .collect(),
    );
    let semantic_graph_nodes = summarize_counts(
        scored_samples
            .iter()
            .map(|sample| sample.work.semantic_graph_nodes as u64)
            .collect(),
    );
    Ok(ScalingPoint {
        size,
        setup_producer_pids,
        scored_samples,
        elapsed_ms,
        allocation_calls,
        allocated_bytes,
        parsed_expressions,
        checked_calls,
        semantic_graph_nodes,
        dependency_scc: trace,
        source_sha256,
        source_bundle_digest_v1,
    })
}

fn scaling_report(
    budget: &ScalingWorkloadBudget,
    maximum_ratio: f64,
    baseline: ScalingPoint,
    base: ScalingPoint,
    doubled: ScalingPoint,
) -> ToolResult<ScalingReport> {
    let elapsed_ms_fixed_overhead_ratio = fixed_overhead_ratio(
        baseline.elapsed_ms.p95,
        base.elapsed_ms.p95,
        doubled.elapsed_ms.p95,
    );
    let allocation_calls_fixed_overhead_ratio = fixed_overhead_ratio(
        baseline.allocation_calls.p95 as f64,
        base.allocation_calls.p95 as f64,
        doubled.allocation_calls.p95 as f64,
    );
    let allocated_bytes_fixed_overhead_ratio = fixed_overhead_ratio(
        baseline.allocated_bytes.p95 as f64,
        base.allocated_bytes.p95 as f64,
        doubled.allocated_bytes.p95 as f64,
    );
    let owning_work_fixed_overhead_ratio = fixed_overhead_ratio(
        owning_work(&baseline, &budget.owning_work_counter)? as f64,
        owning_work(&base, &budget.owning_work_counter)? as f64,
        owning_work(&doubled, &budget.owning_work_counter)? as f64,
    );
    let source_identity_pass = baseline.source_sha256 != base.source_sha256
        && base.source_sha256 != doubled.source_sha256
        && baseline.source_bundle_digest_v1 != base.source_bundle_digest_v1
        && base.source_bundle_digest_v1 != doubled.source_bundle_digest_v1;
    let process_isolation_pass = [&baseline, &base, &doubled].into_iter().all(|point| {
        point.setup_producer_pids.iter().all(|pid| *pid != 0)
            && point
                .scored_samples
                .iter()
                .all(|sample| sample.producer_pid != 0)
    });
    let evaluation = ScalingEvaluation {
        source_identity_pass,
        process_isolation_pass,
        allocation_calls_ratio_pass: ratio_pass(
            allocation_calls_fixed_overhead_ratio,
            maximum_ratio,
        ),
        allocated_bytes_ratio_pass: ratio_pass(allocated_bytes_fixed_overhead_ratio, maximum_ratio),
        owning_work_ratio_pass: ratio_pass(owning_work_fixed_overhead_ratio, maximum_ratio),
    };
    let status = scaling_status(&evaluation);
    Ok(ScalingReport {
        id: budget.id.clone(),
        intent: budget.intent,
        owning_work_counter: budget.owning_work_counter.clone(),
        maximum_doubling_ratio: maximum_ratio,
        status,
        baseline,
        base,
        doubled,
        elapsed_ms_fixed_overhead_ratio,
        allocation_calls_fixed_overhead_ratio,
        allocated_bytes_fixed_overhead_ratio,
        owning_work_fixed_overhead_ratio,
        evaluation,
    })
}

fn owning_work(point: &ScalingPoint, counter: &str) -> ToolResult<u64> {
    match counter {
        "checked-calls" => Ok(point.checked_calls.p95),
        "parsed-expressions" => Ok(point.parsed_expressions.p95),
        "semantic-graph-nodes" => Ok(point.semantic_graph_nodes.p95),
        "dependency-scc-components" => point
            .dependency_scc
            .map(|trace| trace.components)
            .ok_or_else(|| {
                Box::<dyn std::error::Error>::from(
                    "dependency-cone scaling point omitted SCC trace evidence",
                )
            }),
        other => Err(format!("unknown scaling owning work counter `{other}`").into()),
    }
}

fn scaling_status(evaluation: &ScalingEvaluation) -> ReportStatus {
    if evaluation.source_identity_pass
        && evaluation.process_isolation_pass
        && evaluation.allocation_calls_ratio_pass
        && evaluation.allocated_bytes_ratio_pass
        && evaluation.owning_work_ratio_pass
    {
        ReportStatus::Pass
    } else {
        ReportStatus::Fail
    }
}

fn validate_warm_batch(
    workspace: &Path,
    batch: &WarmSessionBatch,
    budget: &WarmBudget,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<()> {
    let total = setup_samples
        .checked_add(scored_samples)
        .ok_or("warm sample count overflow")?;
    if batch.format_version != PRODUCER_FORMAT_VERSION
        || batch.workload != "warm-session-v1"
        || batch.source != budget.source
        || batch.switch_source != budget.switch_source
        || batch.edit_unit != budget.edit_unit
        || batch.producer_pid == 0
        || batch.compiler_threads != 1
        || batch.compiler_caches != "disabled"
        || batch.setup_samples != setup_samples
        || batch.scored_samples != scored_samples
        || batch.primary_project_id == 0
        || batch.switch_project_id == 0
        || batch.primary_project_id == batch.switch_project_id
        || batch.edits.len() != total
        || batch.switches.len() != total
    {
        return Err("warm compiler producer identity or sampling shape is invalid".into());
    }
    for (value, label) in [
        (
            &batch.initial_source_bundle_digest_v1,
            "warm initial source bundle",
        ),
        (&batch.initial_plan_sha256, "warm initial plan"),
        (&batch.switch_plan_sha256, "warm switch plan"),
        (&batch.original_unit_sha256, "warm original edit unit"),
        (&batch.edited_unit_sha256, "warm edited edit unit"),
    ] {
        validate_sha256(value, label)?;
    }
    let original =
        fs::read(workspace.join(safe_relative_path(&budget.edit_unit, "warm edit unit")?))?;
    let original_text = std::str::from_utf8(&original)?;
    let edited_text = original_text.replacen(&budget.edit_from, &budget.edit_to, 1);
    if batch.original_unit_sha256 != sha256_bytes(&original).as_str()
        || batch.edited_unit_sha256 != sha256_bytes(edited_text.as_bytes()).as_str()
    {
        return Err("warm edit-unit hashes differ from the configured source replacement".into());
    }

    let mut forward_plan = None::<&str>;
    let mut forward_source = None::<&str>;
    for (index, sample) in batch.edits.iter().enumerate() {
        let expected_revision = batch
            .base_revision
            .checked_add(index as u64)
            .and_then(|revision| revision.checked_add(1))
            .ok_or("warm revision overflow")?;
        let expected_direction = if index.is_multiple_of(2) {
            "forward"
        } else {
            "reverse"
        };
        if sample.sequence != index
            || sample.scored != (index >= setup_samples)
            || sample.direction != expected_direction
            || sample.previous_revision + 1 != sample.revision
            || sample.revision != expected_revision
            || sample.last_good_revision_before != sample.previous_revision
            || sample.published_revision != sample.revision
            || sample.diagnostic_count != 0
            || !sample.full_document_typecheck_coverage
            || !warm_edit_times_valid(sample)
            || sample.diagnostics_work.source_units == 0
            || sample.diagnostics_work.parsed_expressions == 0
            || sample.diagnostics_work.checked_expressions == 0
            || sample.diagnostics_work.semantic_graph_nodes != 0
            || sample.preview_work.semantic_graph_nodes == 0
            || sample
                .diagnostics_phase
                .values()
                .into_iter()
                .any(|value| !finite_nonnegative(value))
            || sample
                .preview_phase
                .values()
                .into_iter()
                .any(|value| !finite_nonnegative(value))
        {
            return Err(format!("warm edit sample {index} has inconsistent evidence").into());
        }
        validate_sha256(&sample.source_bundle_digest_v1, "warm edit source bundle")?;
        validate_sha256(&sample.plan_sha256, "warm edit plan")?;
        if expected_direction == "reverse" {
            if sample.source_bundle_digest_v1 != batch.initial_source_bundle_digest_v1
                || sample.plan_sha256 != batch.initial_plan_sha256
            {
                return Err(
                    "reverse warm edit did not restore the initial artifact identity".into(),
                );
            }
        } else {
            match (forward_source, forward_plan) {
                (None, None) => {
                    forward_source = Some(&sample.source_bundle_digest_v1);
                    forward_plan = Some(&sample.plan_sha256);
                }
                (Some(source), Some(plan))
                    if source == sample.source_bundle_digest_v1 && plan == sample.plan_sha256 => {}
                _ => {
                    return Err(
                        "forward warm edits produced inconsistent artifact identities".into(),
                    );
                }
            }
        }
    }
    let primary_revision = batch
        .edits
        .last()
        .map(|sample| sample.revision)
        .unwrap_or(batch.base_revision);
    let primary_plan = batch
        .edits
        .last()
        .map(|sample| sample.plan_sha256.as_str())
        .unwrap_or(batch.initial_plan_sha256.as_str());
    for (index, sample) in batch.switches.iter().enumerate() {
        let to_switch = index.is_multiple_of(2);
        let (expected_from, expected_to, expected_revision, expected_plan) = if to_switch {
            (
                batch.primary_project_id,
                batch.switch_project_id,
                batch.base_revision,
                batch.switch_plan_sha256.as_str(),
            )
        } else {
            (
                batch.switch_project_id,
                batch.primary_project_id,
                primary_revision,
                primary_plan,
            )
        };
        if sample.sequence != index
            || sample.scored != (index >= setup_samples)
            || sample.from_project_id != expected_from
            || sample.to_project_id != expected_to
            || sample.selected_revision != expected_revision
            || sample.selected_plan_sha256 != expected_plan
            || sample.compiler_requests_before != sample.compiler_requests_after
            || !finite_nonnegative(sample.acknowledgement_ms)
            || !finite_nonnegative(sample.loaded_bundle_lookup_ms)
            || sample.loaded_bundle_lookup_ms > sample.acknowledgement_ms + 0.05
        {
            return Err(format!("loaded switch sample {index} has inconsistent evidence").into());
        }
        validate_sha256(&sample.selected_plan_sha256, "loaded switch plan")?;
    }
    let expected_cancellation_revision = primary_revision
        .checked_add(1)
        .ok_or("warm cancellation revision overflow")?;
    let cancellation = &batch.cancellation;
    if cancellation.scope != "pre-canceled-request"
        || cancellation.revision != expected_cancellation_revision
        || !cancellation.token_canceled_before_request
        || !cancellation.request_rejected
        || !finite_nonnegative(cancellation.stop_latency_ms)
        || cancellation.last_good_revision_before != primary_revision
        || cancellation.last_good_revision_after != primary_revision
        || !cancellation.publication_unchanged
        || cancellation.in_flight_supersession_supported
    {
        return Err("warm cancellation evidence must describe the sound pre-canceled seam".into());
    }
    let latest = &batch.latest_generation;
    if latest.stale_revision != expected_cancellation_revision
        || latest.latest_revision != expected_cancellation_revision + 1
        || !latest.stale_request_rejected
        || latest.last_good_revision_after_stale_request != primary_revision
        || latest.published_revision != latest.latest_revision
        || !latest.no_stale_publication
        || !finite_nonnegative(latest.publish_latest_ms)
    {
        return Err("warm latest-generation evidence is inconsistent".into());
    }
    let expected_requests = u64::try_from(total)
        .ok()
        .and_then(|count| count.checked_mul(2))
        .and_then(|count| count.checked_add(5))
        .ok_or("warm compiler request count overflow")?;
    if batch.compiler_request_count != expected_requests {
        return Err("warm compiler request count does not match the measured requests".into());
    }
    Ok(())
}

fn warm_edit_times_valid(sample: &WarmEditSample) -> bool {
    [
        sample.update_ack_ms,
        sample.diagnostics_request_ms,
        sample.edit_to_diagnostics_ms,
        sample.verified_preview_request_ms,
        sample.edit_to_verified_preview_ms,
    ]
    .into_iter()
    .all(finite_nonnegative)
        && sample.diagnostics_request_ms <= sample.edit_to_diagnostics_ms + 0.05
        && sample.verified_preview_request_ms <= sample.edit_to_verified_preview_ms + 0.05
        && sample.edit_to_diagnostics_ms <= sample.edit_to_verified_preview_ms + 0.05
}

fn validate_scaling_sample(
    sample: &SyntheticScalingBatch,
    budget: &ScalingWorkloadBudget,
    size: usize,
) -> ToolResult<()> {
    if sample.format_version != PRODUCER_FORMAT_VERSION
        || sample.workload != "synthetic-scaling-v1"
        || sample.generator != "boon-synthetic-scaling-v1"
        || sample.dimension != budget.id
        || sample.size != size
        || sample.intent != budget.intent
        || sample.producer_pid == 0
        || sample.compiler_threads != 1
        || sample.compiler_caches != "disabled"
        || sample.revision != 0
        || !finite_nonnegative(sample.elapsed_ms)
        || sample.peak_rss_kib == 0
        || sample.allocations.allocation_calls == 0
        || sample.allocations.allocated_bytes == 0
        || sample.work.source_units == 0
        || sample.work.parsed_expressions == 0
        || sample.work.checked_expressions == 0
        || sample
            .phase
            .values()
            .into_iter()
            .any(|value| !finite_nonnegative(value))
    {
        return Err(format!("synthetic scaling sample {}/{} is invalid", budget.id, size).into());
    }
    validate_sha256(&sample.synthetic_source_sha256, "synthetic source")?;
    validate_sha256(&sample.source_bundle_digest_v1, "synthetic source bundle")?;
    match (budget.intent, sample.plan_sha256.as_deref()) {
        (SampleIntent::Diagnostics, None) if sample.work.semantic_graph_nodes == 0 => {}
        (SampleIntent::Verified, Some(plan)) if sample.work.semantic_graph_nodes > 0 => {
            validate_sha256(plan, "synthetic MachinePlan")?;
        }
        _ => return Err("synthetic scaling sample phase/artifact ownership is invalid".into()),
    }
    Ok(())
}

fn parse_scc_trace(stderr: &[u8]) -> ToolResult<SccTraceEvidence> {
    const PREFIX: &str = "boon_semantic dependency_manifest graph:counts ";
    let text = std::str::from_utf8(stderr)?;
    let matches = text
        .lines()
        .filter_map(|line| line.strip_prefix(PREFIX))
        .collect::<Vec<_>>();
    let [line] = matches.as_slice() else {
        return Err(format!(
            "expected one dependency SCC trace line, observed {}",
            matches.len()
        )
        .into());
    };
    let value = |name: &str| -> ToolResult<u64> {
        line.split_whitespace()
            .find_map(|field| field.strip_prefix(&format!("{name}=")))
            .ok_or_else(|| format!("dependency SCC trace omitted `{name}`"))?
            .parse::<u64>()
            .map_err(|error| format!("invalid dependency SCC trace `{name}`: {error}").into())
    };
    let evidence = SccTraceEvidence {
        nodes: value("nodes")?,
        edges: value("edges")?,
        components: value("components")?,
        cyclic_components: value("cyclic_components")?,
        maximum_component_nodes: value("maximum_component_nodes")?,
        component_edges: value("component_edges")?,
    };
    if evidence.nodes == 0
        || evidence.components == 0
        || evidence.components > evidence.nodes
        || evidence.maximum_component_nodes == 0
        || evidence.maximum_component_nodes > evidence.nodes
    {
        return Err("dependency SCC trace counts are internally inconsistent".into());
    }
    Ok(evidence)
}

fn load_budget(workspace: &Path) -> ToolResult<(BudgetManifest, String)> {
    let path = workspace.join(DEFAULT_BUDGET);
    let bytes = read_bounded(&path, MAX_BUDGET_BYTES)?;
    let text = std::str::from_utf8(&bytes)?;
    let budget: BudgetManifest =
        toml::from_str(text).map_err(|error| format!("{}: {error}", path.display()))?;
    validate_budget(workspace, &budget)?;
    Ok((budget, sha256_bytes(&bytes).as_str().to_owned()))
}

fn validate_budget(workspace: &Path, budget: &BudgetManifest) -> ToolResult<()> {
    if budget.format_version != BUDGET_FORMAT_VERSION
        || budget.fixtures.is_empty()
        || budget.protocol.build_profile != "release"
        || budget.protocol.target_profile != "software_default"
        || budget.protocol.compiler_threads != 1
        || budget.protocol.compiler_caches != "disabled"
        || budget.protocol.cold_modes != ["fresh-process", "empty-session"]
        || budget.protocol.os_page_cache != "natural"
        || budget.protocol.sample_process_isolation != "one-process-per-observation"
        || budget.protocol.peak_rss_unit != "KiB"
        || budget.protocol.peak_rss_scope != "process-high-water-through-compiler-artifact"
    {
        return Err("compiler interaction budget protocol is invalid".into());
    }
    validate_effective_samples(
        budget.protocol.setup_samples,
        budget.protocol.scored_samples,
    )?;
    let owner_plan = safe_relative_path(&budget.owner_plan, "compiler budget owner plan")?;
    safe_relative_path(&budget.report, "cold compiler report")?;
    if !workspace.join(owner_plan).is_file() {
        return Err("compiler interaction budget owner plan is missing".into());
    }

    let warm = &budget.warm;
    let warm_source = safe_relative_path(&warm.source, "warm source")?;
    let switch_source = safe_relative_path(&warm.switch_source, "warm switch source")?;
    let edit_unit = safe_relative_path(&warm.edit_unit, "warm edit unit")?;
    safe_relative_path(&warm.report, "warm report")?;
    if warm_source == switch_source
        || !workspace.join(&warm_source).is_file()
        || !workspace.join(&switch_source).is_file()
        || !workspace.join(&edit_unit).is_file()
        || warm.edit_from.is_empty()
        || warm.edit_from == warm.edit_to
    {
        return Err("warm compiler workload identity is invalid".into());
    }
    let edit_source = fs::read_to_string(workspace.join(edit_unit))?;
    if edit_source.matches(&warm.edit_from).count() != 1 {
        return Err("warm compiler edit marker must occur exactly once".into());
    }
    let warm_values = [
        warm.checked_diagnostics_p95_ms,
        warm.checked_diagnostics_p99_ms,
        warm.checked_diagnostics_max_ms,
        warm.verified_preview_p95_ms,
        warm.verified_preview_max_ms,
        warm.loaded_bundle_lookup_max_ms,
        warm.switch_ack_p95_ms,
        warm.switch_present_p95_ms,
        warm.switch_present_max_ms,
        warm.cancellation_max_ms,
    ];
    if warm_values.into_iter().any(|value| !finite_positive(value))
        || warm.checked_diagnostics_p95_ms > warm.checked_diagnostics_p99_ms
        || warm.checked_diagnostics_p99_ms > warm.checked_diagnostics_max_ms
        || warm.verified_preview_p95_ms > warm.verified_preview_max_ms
        || warm.switch_present_p95_ms > warm.switch_present_max_ms
    {
        return Err("warm compiler latency budgets are invalid".into());
    }

    let required_dimensions = BTreeSet::from([
        "call-depth",
        "call-site-count",
        "contextual-call-site-count",
        "static-branch-count",
        "source-unit-count",
        "dependency-cone-size",
    ]);
    let dimensions = budget
        .scaling
        .dimensions
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if budget.scaling.dimensions.len() != required_dimensions.len()
        || dimensions != required_dimensions
        || !finite_positive(budget.scaling.maximum_doubling_ratio)
        || budget.scaling.maximum_doubling_ratio > 2.2
        || budget.scaling.workloads.len() != required_dimensions.len()
    {
        return Err("scaling budget must preserve exactly the six planned dimensions".into());
    }
    let mut workloads = BTreeSet::new();
    for workload in &budget.scaling.workloads {
        if !workloads.insert(workload.id.as_str())
            || workload.baseline_size >= workload.base_size
            || workload.base_size == 0
            || workload.base_size.checked_mul(2) != Some(workload.doubled_size)
        {
            return Err(format!(
                "scaling workload `{}` has an invalid size series",
                workload.id
            )
            .into());
        }
        let expected = match workload.id.as_str() {
            "call-depth" | "call-site-count" => (SampleIntent::Diagnostics, "checked-calls"),
            "contextual-call-site-count" | "static-branch-count" => {
                (SampleIntent::Verified, "semantic-graph-nodes")
            }
            "source-unit-count" => (SampleIntent::Diagnostics, "parsed-expressions"),
            "dependency-cone-size" => (SampleIntent::Verified, "dependency-scc-components"),
            other => return Err(format!("unsupported scaling workload `{other}`").into()),
        };
        if workload.intent != expected.0 || workload.owning_work_counter != expected.1 {
            return Err(format!(
                "scaling workload `{}` intent/counter is invalid",
                workload.id
            )
            .into());
        }
    }
    if workloads != required_dimensions {
        return Err("scaling workload definitions do not cover the planned dimensions".into());
    }
    Ok(())
}

fn validate_existing(
    workspace: &Path,
    path: &Path,
    budget: &BudgetManifest,
    budget_digest: &str,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<CompilerInteractionsReport> {
    let bytes = read_bounded(path, MAX_REPORT_BYTES)?;
    let report: CompilerInteractionsReport = serde_json::from_slice(&bytes)?;
    validate_report(
        workspace,
        &report,
        budget,
        budget_digest,
        setup_samples,
        scored_samples,
    )?;
    Ok(report)
}

fn validate_report(
    workspace: &Path,
    report: &CompilerInteractionsReport,
    budget: &BudgetManifest,
    budget_digest: &str,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<()> {
    if report.format_version != FORMAT_VERSION || report.contract != REPORT_CONTRACT {
        return Err("compiler interaction report contract is unsupported".into());
    }
    if report.identity != current_identity(workspace)? {
        return Err("compiler interaction report source/tool identity is stale".into());
    }
    let expected_budget = BudgetIdentity {
        path: DEFAULT_BUDGET.to_owned(),
        sha256: budget_digest.to_owned(),
        format_version: budget.format_version,
        owner_plan: budget.owner_plan.clone(),
    };
    if report.budget != expected_budget {
        return Err("compiler interaction report budget identity is stale".into());
    }
    let producer_path = workspace.join(PRODUCER_PATH);
    let expected_producer = ProducerIdentity {
        path: PRODUCER_PATH.to_owned(),
        sha256: sha256_file(&producer_path)?.as_str().to_owned(),
    };
    if report.producer != expected_producer {
        return Err("compiler interaction report producer identity is stale".into());
    }
    if report.protocol != protocol_evidence(budget, setup_samples, scored_samples)
        || report.run_classification != classification(budget, setup_samples, scored_samples)
    {
        return Err("compiler interaction report sampling protocol is stale".into());
    }
    validate_warm_batch(
        workspace,
        &report.warm.raw,
        &budget.warm,
        setup_samples,
        scored_samples,
    )?;
    let expected_warm = warm_report(report.warm.raw.clone(), &budget.warm)?;
    if report.warm != expected_warm {
        return Err("compiler interaction warm summaries/evaluation are inconsistent".into());
    }
    if report.scaling.len() != budget.scaling.workloads.len() {
        return Err("compiler interaction scaling report count is invalid".into());
    }
    for (scaling, workload) in report.scaling.iter().zip(&budget.scaling.workloads) {
        validate_scaling_point(
            &scaling.baseline,
            workload,
            workload.baseline_size,
            setup_samples,
            scored_samples,
        )?;
        validate_scaling_point(
            &scaling.base,
            workload,
            workload.base_size,
            setup_samples,
            scored_samples,
        )?;
        validate_scaling_point(
            &scaling.doubled,
            workload,
            workload.doubled_size,
            setup_samples,
            scored_samples,
        )?;
        let expected = scaling_report(
            workload,
            budget.scaling.maximum_doubling_ratio,
            scaling.baseline.clone(),
            scaling.base.clone(),
            scaling.doubled.clone(),
        )?;
        if scaling != &expected {
            return Err(format!("scaling report `{}` is inconsistent", workload.id).into());
        }
    }
    let expected_missing = vec![
        "native-presented-frame: compiler-only harness cannot replace app-owned WGPU readback"
            .to_owned(),
        "in-flight-supersession: synchronous CompilerSession exposes no worker generation handle"
            .to_owned(),
    ];
    if report.missing_acceptance_evidence != expected_missing {
        return Err("compiler interaction report omitted a known acceptance-evidence gap".into());
    }
    let expected_status = aggregate_status(&report.warm, &report.scaling, &expected_missing);
    if report.status != expected_status || report.status != ReportStatus::Fail {
        return Err(
            "compiler interaction aggregate must fail closed while acceptance evidence is missing"
                .into(),
        );
    }
    Ok(())
}

fn validate_scaling_point(
    point: &ScalingPoint,
    budget: &ScalingWorkloadBudget,
    size: usize,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<()> {
    if point.size != size
        || point.setup_producer_pids.len() != setup_samples
        || point.scored_samples.len() != scored_samples
        || point.setup_producer_pids.iter().any(|pid| *pid == 0)
    {
        return Err(format!(
            "scaling point {}/{} has invalid sampling shape",
            budget.id, size
        )
        .into());
    }
    for sample in &point.scored_samples {
        validate_scaling_sample(sample, budget, size)?;
    }
    let expected_trace = if budget.owning_work_counter == "dependency-scc-components" {
        point.dependency_scc
    } else {
        None
    };
    if (budget.owning_work_counter == "dependency-scc-components") != point.dependency_scc.is_some()
    {
        return Err(format!(
            "scaling point {}/{} has incorrect SCC evidence ownership",
            budget.id, size
        )
        .into());
    }
    if let Some(trace) = expected_trace
        && (trace.nodes == 0
            || trace.components == 0
            || trace.components > trace.nodes
            || trace.maximum_component_nodes == 0
            || trace.maximum_component_nodes > trace.nodes)
    {
        return Err("stored dependency SCC evidence is inconsistent".into());
    }
    let expected = scaling_point(
        size,
        point.setup_producer_pids.clone(),
        point.scored_samples.clone(),
        expected_trace,
    )?;
    if point != &expected {
        return Err(format!(
            "scaling point {}/{} summaries are inconsistent",
            budget.id, size
        )
        .into());
    }
    let plans = point
        .scored_samples
        .iter()
        .filter_map(|sample| sample.plan_sha256.as_deref())
        .collect::<BTreeSet<_>>();
    if (budget.intent == SampleIntent::Verified && plans.len() != 1)
        || (budget.intent == SampleIntent::Diagnostics && !plans.is_empty())
    {
        return Err(format!(
            "scaling point {}/{} artifact hashes are nondeterministic",
            budget.id, size
        )
        .into());
    }
    Ok(())
}

fn aggregate_status(
    warm: &WarmReport,
    scaling: &[ScalingReport],
    missing_acceptance_evidence: &[String],
) -> ReportStatus {
    if warm.status == ReportStatus::Pass
        && scaling
            .iter()
            .all(|report| report.status == ReportStatus::Pass)
        && missing_acceptance_evidence.is_empty()
    {
        ReportStatus::Pass
    } else {
        ReportStatus::Fail
    }
}

fn protocol_evidence(
    budget: &BudgetManifest,
    setup_samples: usize,
    scored_samples: usize,
) -> ProtocolEvidence {
    ProtocolEvidence {
        default_setup_samples: budget.protocol.setup_samples,
        default_scored_samples: budget.protocol.scored_samples,
        effective_setup_samples: setup_samples,
        effective_scored_samples: scored_samples,
        compiler_threads: budget.protocol.compiler_threads,
        compiler_caches: budget.protocol.compiler_caches.clone(),
        percentile_method: "nearest-rank".to_owned(),
        scaling_process_isolation: budget.protocol.sample_process_isolation.clone(),
    }
}

fn classification(
    budget: &BudgetManifest,
    setup_samples: usize,
    scored_samples: usize,
) -> RunClassification {
    if setup_samples == budget.protocol.setup_samples
        && scored_samples == budget.protocol.scored_samples
    {
        RunClassification::Acceptance
    } else {
        RunClassification::DevelopmentNonAcceptance
    }
}

fn validate_effective_samples(setup_samples: usize, scored_samples: usize) -> ToolResult<()> {
    if scored_samples == 0
        || setup_samples
            .checked_add(scored_samples)
            .is_none_or(|count| count > MAX_SAMPLE_COUNT)
    {
        return Err(format!(
            "compiler interactions require scored samples and setup + scored <= {MAX_SAMPLE_COUNT}"
        )
        .into());
    }
    Ok(())
}

fn safe_relative_path(value: &str, label: &str) -> ToolResult<PathBuf> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("{label} `{value}` must be a normalized relative path").into());
    }
    Ok(path.to_path_buf())
}

fn validate_sha256(value: &str, label: &str) -> ToolResult<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("{label} must be 64 lowercase hexadecimal bytes").into());
    }
    Ok(())
}

fn finite_positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn finite_nonnegative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

fn status_name(status: ReportStatus) -> &'static str {
    match status {
        ReportStatus::Pass => "pass",
        ReportStatus::Fail => "fail",
    }
}

fn read_bounded(path: &Path, byte_limit: u64) -> ToolResult<Vec<u8>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > byte_limit {
        return Err(format!(
            "{} is not a regular file of 1..={byte_limit} bytes",
            path.display()
        )
        .into());
    }
    Ok(fs::read(path)?)
}

fn write_report(path: &Path, report: &CompilerInteractionsReport) -> ToolResult<()> {
    let mut bytes = serde_json::to_vec_pretty(report)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err(format!(
            "compiler interaction report is {} bytes; limit is {MAX_REPORT_BYTES}",
            bytes.len()
        )
        .into());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary =
        path.with_extension(format!("compiler-interactions-tmp-{}", std::process::id()));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn parse_output_json<T: serde::de::DeserializeOwned>(
    output: &Output,
    label: &str,
) -> ToolResult<T> {
    if !output.status.success() {
        return Err(format!(
            "{label} failed with {}: {}",
            output.status,
            bounded_lossy(&output.stderr, 4096)
        )
        .into());
    }
    if output.stdout.is_empty() || output.stdout.len() > MAX_SAMPLE_OUTPUT_BYTES {
        return Err(format!(
            "{label} stdout is {} bytes; expected 1..={MAX_SAMPLE_OUTPUT_BYTES}",
            output.stdout.len()
        )
        .into());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| {
        format!(
            "parse {label} JSON: {error}; stdout: {}",
            bounded_lossy(&output.stdout, 4096)
        )
        .into()
    })
}

fn bounded_lossy(bytes: &[u8], limit: usize) -> String {
    let end = bytes.len().min(limit);
    let mut value = String::from_utf8_lossy(&bytes[..end]).trim().to_owned();
    if bytes.len() > limit {
        value.push_str(" ...<truncated>");
    }
    value
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
        p99: nearest_rank(&values, 99),
        max: *values.last().expect("non-empty values"),
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
        p99: nearest_rank(&values, 99),
        max: *values.last().expect("non-empty values"),
    }
}

fn nearest_rank<T: Copy>(sorted: &[T], percentile: usize) -> T {
    debug_assert!(!sorted.is_empty());
    debug_assert!((1..=100).contains(&percentile));
    let rank = sorted.len().saturating_mul(percentile).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn one_digest<'a>(values: impl IntoIterator<Item = &'a str>, label: &str) -> ToolResult<String> {
    let digests = values.into_iter().collect::<BTreeSet<_>>();
    if digests.len() != 1 {
        return Err(format!(
            "{label} digest observations are missing or nondeterministic; observed {} unique values",
            digests.len()
        )
        .into());
    }
    let digest = digests
        .into_iter()
        .next()
        .expect("one digest after cardinality check");
    validate_sha256(digest, label)?;
    Ok(digest.to_owned())
}

fn fixed_overhead_ratio(baseline: f64, base: f64, doubled: f64) -> Option<f64> {
    if [baseline, base, doubled]
        .into_iter()
        .any(|value| !finite_nonnegative(value))
    {
        return None;
    }
    let base_increment = base - baseline;
    let doubled_increment = doubled - baseline;
    if !finite_positive(base_increment) || !finite_nonnegative(doubled_increment) {
        return None;
    }
    let ratio = doubled_increment / base_increment;
    finite_nonnegative(ratio).then_some(ratio)
}

fn ratio_pass(ratio: Option<f64>, maximum_ratio: f64) -> bool {
    finite_positive(maximum_ratio)
        && ratio.is_some_and(|value| finite_nonnegative(value) && value <= maximum_ratio)
}
