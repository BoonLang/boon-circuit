use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

use crate::compiler_producer::{
    ExpectedProducer, ProducerIdentity, ProducerMetadata, ProducerSetIdentity,
    validate_producer_metadata,
};
use crate::compiler_work_sample::{WorkSample, require_current_prebuilt_producer};
use crate::report_v2::{
    ExpectedIdentity, ReportStatus, ToolResult, current_identity, sha256_bytes, sha256_file,
    unix_time_ms,
};

const FORMAT_VERSION: u16 = 7;
const PRODUCER_FORMAT_VERSION: u16 = 8;
const BUDGET_FORMAT_VERSION: u16 = 3;
const REPORT_CONTRACT: &str = "boon-compiler-interactions-v6";
const DEFAULT_BUDGET: &str = "budgets/compiler.toml";
const MAX_BUDGET_BYTES: u64 = 64 * 1024;
const MAX_REPORT_BYTES: u64 = 16 * 1024 * 1024;
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
    product_producer: String,
    evidence_producer: String,
    product_kind: String,
    product_allocator: String,
    evidence_kind: String,
    evidence_allocator: String,
    evidence_instrumentation: String,
    evidence_counter_scope: String,
    target_cpu: String,
    profile_options: String,
    rustflags: String,
    producer_pair_schedule: String,
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
    producers: ProducerSetIdentity,
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
struct ProtocolEvidence {
    product_kind: String,
    product_allocator: String,
    evidence_kind: String,
    evidence_allocator: String,
    evidence_instrumentation: String,
    evidence_counter_scope: String,
    target_cpu: String,
    profile_options: String,
    rustflags: String,
    producer_pair_schedule: String,
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
    evidence_raw: WarmSessionBatch,
    evidence_parity_pass: bool,
    diagnostics_edit_to_ready_ms: MillisSummary,
    verified_preview_edit_to_ready_ms: MillisSummary,
    update_ack_ms: MillisSummary,
    loaded_bundle_lookup_ms: MillisSummary,
    switch_ack_ms: MillisSummary,
    resident_rss_kib: CountSummary,
    peak_rss_kib: u64,
    cancellation_stop_ms: f64,
    latest_generation_publish_ms: f64,
    evaluation: WarmEvaluation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WarmEvaluation {
    evidence_parity_pass: bool,
    diagnostics_p95_pass: bool,
    diagnostics_p99_pass: bool,
    diagnostics_max_pass: bool,
    verified_preview_p95_pass: bool,
    verified_preview_max_pass: bool,
    loaded_bundle_lookup_pass: bool,
    switch_ack_pass: bool,
    switch_no_compile_pass: bool,
    switch_no_allocation_pass: bool,
    session_peak_rss_pass: bool,
    session_tail_growth_pass: bool,
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
    evidence_setup_producer_pids: Vec<u32>,
    scored_samples: Vec<SyntheticScalingBatch>,
    evidence_scored_samples: Vec<SyntheticScalingBatch>,
    evidence_parity_pass: bool,
    elapsed_ms: MillisSummary,
    allocation_calls: CountSummary,
    allocated_bytes: CountSummary,
    parse_source_units_attempted: CountSummary,
    parsed_expressions: CountSummary,
    checked_expressions: CountSummary,
    typecheck_inference_call_visits: CountSummary,
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
    evidence_parity_pass: bool,
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
    producer: ProducerMetadata,
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
    initial_resident_rss_kib: u64,
    final_resident_rss_kib: u64,
    peak_rss_kib: u64,
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
    resident_rss_kib: u64,
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
    producer: ProducerMetadata,
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
}

impl PhaseSample {
    const fn values(self) -> [f64; 8] {
        [
            self.parse_ms,
            self.typecheck_ms,
            self.semantic_ms,
            self.contract_verify_ms,
            self.ir_lower_ms,
            self.ir_validation_ms,
            self.backend_ms,
            self.plan_validation_ms,
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
    let before = current_identity(workspace)?;
    let product_path = workspace.join(safe_relative_path(
        &budget.protocol.product_producer,
        "product producer path",
    )?);
    let evidence_path = workspace.join(safe_relative_path(
        &budget.protocol.evidence_producer,
        "evidence producer path",
    )?);
    require_current_prebuilt_producer(workspace, &product_path)?;
    require_current_prebuilt_producer(workspace, &evidence_path)?;
    let product_digest = sha256_file(&product_path)?.as_str().to_owned();
    let evidence_digest = sha256_file(&evidence_path)?.as_str().to_owned();
    let warm_batch = run_warm_batch(
        workspace,
        &product_path,
        &budget.warm,
        setup_samples,
        scored_samples,
    )?;
    let warm_evidence_batch = run_warm_batch(
        workspace,
        &evidence_path,
        &budget.warm,
        setup_samples,
        scored_samples,
    )?;
    validate_interaction_producer(
        workspace,
        &before,
        &warm_batch.producer,
        &product_digest,
        &budget.protocol.product_producer,
        &budget.protocol.product_kind,
        &budget.protocol.product_allocator,
        "none",
        "none",
        &budget.protocol,
    )?;
    validate_interaction_producer(
        workspace,
        &before,
        &warm_evidence_batch.producer,
        &evidence_digest,
        &budget.protocol.evidence_producer,
        &budget.protocol.evidence_kind,
        &budget.protocol.evidence_allocator,
        &budget.protocol.evidence_instrumentation,
        &budget.protocol.evidence_counter_scope,
        &budget.protocol,
    )?;
    validate_warm_product_evidence_parity(&warm_batch, &warm_evidence_batch)?;
    let product_metadata = warm_batch.producer.clone();
    let evidence_metadata = warm_evidence_batch.producer.clone();
    let warm = warm_report(warm_batch, warm_evidence_batch, &budget.warm)?;
    let mut scaling = Vec::with_capacity(budget.scaling.workloads.len());
    for workload in &budget.scaling.workloads {
        scaling.push(collect_scaling(
            workspace,
            &before,
            &product_path,
            &product_digest,
            &evidence_path,
            &evidence_digest,
            &budget.protocol,
            workload,
            budget.scaling.maximum_doubling_ratio,
            setup_samples,
            scored_samples,
        )?);
    }
    if sha256_file(&product_path)?.as_str() != product_digest
        || sha256_file(&evidence_path)?.as_str() != evidence_digest
    {
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
        producers: ProducerSetIdentity {
            product: ProducerIdentity {
                path: budget.protocol.product_producer.clone(),
                sha256: product_digest,
                metadata: product_metadata,
            },
            evidence: ProducerIdentity {
                path: budget.protocol.evidence_producer.clone(),
                sha256: evidence_digest,
                metadata: evidence_metadata,
            },
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
        .env_remove("BOON_KERNEL_EXPERIMENTAL_PARALLEL")
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

#[allow(clippy::too_many_arguments)]
fn validate_interaction_producer(
    workspace: &Path,
    identity: &ExpectedIdentity,
    metadata: &ProducerMetadata,
    digest: &str,
    path: &str,
    kind: &str,
    allocator: &str,
    instrumentation: &str,
    counter_scope: &str,
    protocol: &BudgetProtocol,
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
            cargo_profile: &protocol.build_profile,
            target_cpu: &protocol.target_cpu,
            profile_options: &protocol.profile_options,
            rustflags: &protocol.rustflags,
        },
    )
}

fn validate_warm_product_evidence_parity(
    product: &WarmSessionBatch,
    evidence: &WarmSessionBatch,
) -> ToolResult<()> {
    if product.workload != evidence.workload
        || product.source != evidence.source
        || product.switch_source != evidence.switch_source
        || product.edit_unit != evidence.edit_unit
        || product.compiler_threads != evidence.compiler_threads
        || product.compiler_caches != evidence.compiler_caches
        || product.setup_samples != evidence.setup_samples
        || product.scored_samples != evidence.scored_samples
        || product.primary_project_id != evidence.primary_project_id
        || product.switch_project_id != evidence.switch_project_id
        || product.base_revision != evidence.base_revision
        || product.initial_source_bundle_digest_v1 != evidence.initial_source_bundle_digest_v1
        || product.initial_plan_sha256 != evidence.initial_plan_sha256
        || product.switch_plan_sha256 != evidence.switch_plan_sha256
        || product.original_unit_sha256 != evidence.original_unit_sha256
        || product.edited_unit_sha256 != evidence.edited_unit_sha256
        || product.compiler_request_count != evidence.compiler_request_count
        || product.edits.len() != evidence.edits.len()
        || product.switches.len() != evidence.switches.len()
    {
        return Err("warm product/evidence batch identities differ".into());
    }
    for (product, evidence) in product.edits.iter().zip(&evidence.edits) {
        if product.sequence != evidence.sequence
            || product.scored != evidence.scored
            || product.direction != evidence.direction
            || product.previous_revision != evidence.previous_revision
            || product.revision != evidence.revision
            || product.last_good_revision_before != evidence.last_good_revision_before
            || product.diagnostic_count != evidence.diagnostic_count
            || product.full_document_typecheck_coverage != evidence.full_document_typecheck_coverage
            || product.source_bundle_digest_v1 != evidence.source_bundle_digest_v1
            || product.plan_sha256 != evidence.plan_sha256
            || product.published_revision != evidence.published_revision
            || product.diagnostics_work != evidence.diagnostics_work
            || product.preview_work != evidence.preview_work
        {
            return Err("warm product/evidence edit results or owned work differ".into());
        }
        if product.diagnostics_allocations != AllocationSample::default()
            || product.preview_allocations != AllocationSample::default()
            || evidence.diagnostics_allocations.allocation_calls == 0
            || evidence.diagnostics_allocations.allocated_bytes == 0
            || evidence.preview_allocations.allocation_calls == 0
            || evidence.preview_allocations.allocated_bytes == 0
        {
            return Err("warm product/evidence allocation lanes are invalid".into());
        }
    }
    for (product, evidence) in product.switches.iter().zip(&evidence.switches) {
        if product.sequence != evidence.sequence
            || product.scored != evidence.scored
            || product.from_project_id != evidence.from_project_id
            || product.to_project_id != evidence.to_project_id
            || product.selected_revision != evidence.selected_revision
            || product.compiler_requests_before != evidence.compiler_requests_before
            || product.compiler_requests_after != evidence.compiler_requests_after
            || product.selected_plan_sha256 != evidence.selected_plan_sha256
            || product.allocation_calls != 0
            || product.allocated_bytes != 0
            || evidence.allocation_calls != 0
            || evidence.allocated_bytes != 0
        {
            return Err("warm product/evidence loaded-switch results differ".into());
        }
    }
    let product_cancellation = &product.cancellation;
    let evidence_cancellation = &evidence.cancellation;
    if product_cancellation.scope != evidence_cancellation.scope
        || product_cancellation.revision != evidence_cancellation.revision
        || product_cancellation.token_canceled_before_request
            != evidence_cancellation.token_canceled_before_request
        || product_cancellation.request_rejected != evidence_cancellation.request_rejected
        || product_cancellation.last_good_revision_before
            != evidence_cancellation.last_good_revision_before
        || product_cancellation.last_good_revision_after
            != evidence_cancellation.last_good_revision_after
        || product_cancellation.publication_unchanged != evidence_cancellation.publication_unchanged
        || product_cancellation.in_flight_supersession_supported
            != evidence_cancellation.in_flight_supersession_supported
    {
        return Err("warm product/evidence cancellation semantics differ".into());
    }
    let product_latest = &product.latest_generation;
    let evidence_latest = &evidence.latest_generation;
    if product_latest.stale_revision != evidence_latest.stale_revision
        || product_latest.latest_revision != evidence_latest.latest_revision
        || product_latest.stale_request_rejected != evidence_latest.stale_request_rejected
        || product_latest.last_good_revision_after_stale_request
            != evidence_latest.last_good_revision_after_stale_request
        || product_latest.published_revision != evidence_latest.published_revision
        || product_latest.no_stale_publication != evidence_latest.no_stale_publication
    {
        return Err("warm product/evidence latest-generation semantics differ".into());
    }
    Ok(())
}

fn validate_scaling_product_evidence_parity(
    product: &SyntheticScalingBatch,
    evidence: &SyntheticScalingBatch,
) -> ToolResult<()> {
    if product.workload != evidence.workload
        || product.generator != evidence.generator
        || product.dimension != evidence.dimension
        || product.size != evidence.size
        || product.intent != evidence.intent
        || product.compiler_threads != evidence.compiler_threads
        || product.compiler_caches != evidence.compiler_caches
        || product.revision != evidence.revision
        || product.synthetic_source_sha256 != evidence.synthetic_source_sha256
        || product.source_bundle_digest_v1 != evidence.source_bundle_digest_v1
        || product.plan_sha256 != evidence.plan_sha256
        || product.work != evidence.work
    {
        return Err("scaling product/evidence results or owned work differ".into());
    }
    if product.allocations != AllocationSample::default()
        || evidence.allocations.allocation_calls == 0
        || evidence.allocations.allocated_bytes == 0
    {
        return Err("scaling product/evidence allocation lanes are invalid".into());
    }
    Ok(())
}

fn warm_report(
    raw: WarmSessionBatch,
    evidence_raw: WarmSessionBatch,
    budget: &WarmBudget,
) -> ToolResult<WarmReport> {
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
    let resident_rss_kib = summarize_counts(
        scored_edits
            .iter()
            .map(|sample| sample.resident_rss_kib)
            .collect(),
    );
    let evaluation = warm_evaluation(
        &raw,
        &evidence_raw,
        budget,
        diagnostics_edit_to_ready_ms,
        verified_preview_edit_to_ready_ms,
        loaded_bundle_lookup_ms,
        switch_ack_ms,
    );
    let status = warm_status(&evaluation);
    let peak_rss_kib = raw.peak_rss_kib;
    Ok(WarmReport {
        status,
        cancellation_stop_ms: raw.cancellation.stop_latency_ms,
        latest_generation_publish_ms: raw.latest_generation.publish_latest_ms,
        raw,
        evidence_raw,
        evidence_parity_pass: true,
        diagnostics_edit_to_ready_ms,
        verified_preview_edit_to_ready_ms,
        update_ack_ms,
        loaded_bundle_lookup_ms,
        switch_ack_ms,
        resident_rss_kib,
        peak_rss_kib,
        evaluation,
    })
}

fn warm_evaluation(
    raw: &WarmSessionBatch,
    evidence_raw: &WarmSessionBatch,
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
    let switch_no_allocation_pass = evidence_raw
        .switches
        .iter()
        .filter(|sample| sample.scored)
        .all(|sample| sample.allocation_calls == 0 && sample.allocated_bytes == 0);
    let pre_canceled_request_pass = raw.cancellation.scope == "pre-canceled-request"
        && raw.cancellation.token_canceled_before_request
        && raw.cancellation.request_rejected
        && raw.cancellation.publication_unchanged
        && raw.cancellation.stop_latency_ms <= budget.cancellation_max_ms;
    let mut tail_rss = raw
        .edits
        .iter()
        .skip(raw.edits.len() / 2)
        .map(|sample| sample.resident_rss_kib)
        .collect::<Vec<_>>();
    tail_rss.push(raw.final_resident_rss_kib);
    let tail_min = tail_rss.iter().copied().min().unwrap_or(0);
    let tail_max = tail_rss.iter().copied().max().unwrap_or(u64::MAX);
    WarmEvaluation {
        evidence_parity_pass: true,
        diagnostics_p95_pass: diagnostics.p95 <= budget.checked_diagnostics_p95_ms,
        diagnostics_p99_pass: diagnostics.p99 <= budget.checked_diagnostics_p99_ms,
        diagnostics_max_pass: diagnostics.max <= budget.checked_diagnostics_max_ms,
        verified_preview_p95_pass: preview.p95 <= budget.verified_preview_p95_ms,
        verified_preview_max_pass: preview.max <= budget.verified_preview_max_ms,
        loaded_bundle_lookup_pass: lookup.max <= budget.loaded_bundle_lookup_max_ms,
        switch_ack_pass: switch_ack.p95 <= budget.switch_ack_p95_ms,
        switch_no_compile_pass,
        switch_no_allocation_pass,
        session_peak_rss_pass: raw.peak_rss_kib <= 512 * 1024,
        session_tail_growth_pass: raw.initial_resident_rss_kib > 0
            && raw.final_resident_rss_kib > 0
            && tail_min > 0
            && tail_max.saturating_sub(tail_min) <= 128 * 1024
            && raw
                .final_resident_rss_kib
                .saturating_sub(raw.initial_resident_rss_kib)
                <= 256 * 1024,
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
    let passes = evaluation.evidence_parity_pass
        && evaluation.diagnostics_p95_pass
        && evaluation.diagnostics_p99_pass
        && evaluation.diagnostics_max_pass
        && evaluation.verified_preview_p95_pass
        && evaluation.verified_preview_max_pass
        && evaluation.loaded_bundle_lookup_pass
        && evaluation.switch_ack_pass
        && evaluation.switch_no_compile_pass
        && evaluation.switch_no_allocation_pass
        && evaluation.session_peak_rss_pass
        && evaluation.session_tail_growth_pass
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
    identity: &ExpectedIdentity,
    product: &Path,
    product_digest: &str,
    evidence: &Path,
    evidence_digest: &str,
    protocol: &BudgetProtocol,
    budget: &ScalingWorkloadBudget,
    maximum_ratio: f64,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<ScalingReport> {
    let baseline = collect_scaling_point(
        workspace,
        identity,
        product,
        product_digest,
        evidence,
        evidence_digest,
        protocol,
        budget,
        budget.baseline_size,
        setup_samples,
        scored_samples,
    )?;
    let base = collect_scaling_point(
        workspace,
        identity,
        product,
        product_digest,
        evidence,
        evidence_digest,
        protocol,
        budget,
        budget.base_size,
        setup_samples,
        scored_samples,
    )?;
    let doubled = collect_scaling_point(
        workspace,
        identity,
        product,
        product_digest,
        evidence,
        evidence_digest,
        protocol,
        budget,
        budget.doubled_size,
        setup_samples,
        scored_samples,
    )?;
    scaling_report(budget, maximum_ratio, baseline, base, doubled)
}

fn collect_scaling_point(
    workspace: &Path,
    identity: &ExpectedIdentity,
    product: &Path,
    product_digest: &str,
    evidence: &Path,
    evidence_digest: &str,
    protocol: &BudgetProtocol,
    budget: &ScalingWorkloadBudget,
    size: usize,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<ScalingPoint> {
    let mut setup_producer_pids = Vec::with_capacity(setup_samples);
    let mut evidence_setup_producer_pids = Vec::with_capacity(setup_samples);
    for sample_index in 0..setup_samples {
        let (product_sample, evidence_sample) = run_scaling_pair(
            workspace,
            identity,
            product,
            product_digest,
            evidence,
            evidence_digest,
            protocol,
            budget,
            size,
            sample_index,
        )?;
        setup_producer_pids.push(product_sample.producer_pid);
        evidence_setup_producer_pids.push(evidence_sample.producer_pid);
    }
    let mut scored = Vec::with_capacity(scored_samples);
    let mut evidence_scored = Vec::with_capacity(scored_samples);
    for sample_index in 0..scored_samples {
        let (product_sample, evidence_sample) = run_scaling_pair(
            workspace,
            identity,
            product,
            product_digest,
            evidence,
            evidence_digest,
            protocol,
            budget,
            size,
            setup_samples + sample_index,
        )?;
        scored.push(product_sample);
        evidence_scored.push(evidence_sample);
    }
    let trace = if budget.owning_work_counter == "dependency-scc-components" {
        let (sample, trace) = run_scaling_trace(workspace, product, budget, size)?;
        validate_interaction_producer(
            workspace,
            identity,
            &sample.producer,
            product_digest,
            &protocol.product_producer,
            &protocol.product_kind,
            &protocol.product_allocator,
            "none",
            "none",
            protocol,
        )?;
        Some(trace)
    } else {
        None
    };
    scaling_point(
        size,
        setup_producer_pids,
        evidence_setup_producer_pids,
        scored,
        evidence_scored,
        trace,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_scaling_pair(
    workspace: &Path,
    identity: &ExpectedIdentity,
    product: &Path,
    product_digest: &str,
    evidence: &Path,
    evidence_digest: &str,
    protocol: &BudgetProtocol,
    budget: &ScalingWorkloadBudget,
    size: usize,
    sample_index: usize,
) -> ToolResult<(SyntheticScalingBatch, SyntheticScalingBatch)> {
    let (product_sample, evidence_sample) = if sample_index.is_multiple_of(2) {
        (
            run_scaling_sample(workspace, product, budget, size, false)?,
            run_scaling_sample(workspace, evidence, budget, size, false)?,
        )
    } else {
        let evidence_sample = run_scaling_sample(workspace, evidence, budget, size, false)?;
        let product_sample = run_scaling_sample(workspace, product, budget, size, false)?;
        (product_sample, evidence_sample)
    };
    validate_interaction_producer(
        workspace,
        identity,
        &product_sample.producer,
        product_digest,
        &protocol.product_producer,
        &protocol.product_kind,
        &protocol.product_allocator,
        "none",
        "none",
        protocol,
    )?;
    validate_interaction_producer(
        workspace,
        identity,
        &evidence_sample.producer,
        evidence_digest,
        &protocol.evidence_producer,
        &protocol.evidence_kind,
        &protocol.evidence_allocator,
        &protocol.evidence_instrumentation,
        &protocol.evidence_counter_scope,
        protocol,
    )?;
    validate_scaling_product_evidence_parity(&product_sample, &evidence_sample)?;
    Ok((product_sample, evidence_sample))
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
        .env_remove("BOON_KERNEL_EXPERIMENTAL_PARALLEL")
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
        .env_remove("BOON_KERNEL_EXPERIMENTAL_PARALLEL")
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
    evidence_setup_producer_pids: Vec<u32>,
    scored_samples: Vec<SyntheticScalingBatch>,
    evidence_scored_samples: Vec<SyntheticScalingBatch>,
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
        evidence_scored_samples
            .iter()
            .map(|sample| sample.allocations.allocation_calls)
            .collect(),
    );
    let allocated_bytes = summarize_counts(
        evidence_scored_samples
            .iter()
            .map(|sample| sample.allocations.allocated_bytes)
            .collect(),
    );
    let checked_calls = summarize_counts(
        evidence_scored_samples
            .iter()
            .map(|sample| sample.work.checked_calls as u64)
            .collect(),
    );
    let typecheck_inference_call_visits = summarize_counts(
        evidence_scored_samples
            .iter()
            .map(|sample| sample.work.typecheck.inference_call_visits)
            .collect(),
    );
    let parse_source_units_attempted = summarize_counts(
        evidence_scored_samples
            .iter()
            .map(|sample| sample.work.parse.source_units_attempted as u64)
            .collect(),
    );
    let parsed_expressions = summarize_counts(
        evidence_scored_samples
            .iter()
            .map(|sample| sample.work.parsed_expressions as u64)
            .collect(),
    );
    let checked_expressions = summarize_counts(
        evidence_scored_samples
            .iter()
            .map(|sample| sample.work.checked_expressions as u64)
            .collect(),
    );
    let semantic_graph_nodes = summarize_counts(
        evidence_scored_samples
            .iter()
            .map(|sample| sample.work.semantic_graph_nodes as u64)
            .collect(),
    );
    Ok(ScalingPoint {
        size,
        setup_producer_pids,
        evidence_setup_producer_pids,
        scored_samples,
        evidence_scored_samples,
        evidence_parity_pass: true,
        elapsed_ms,
        allocation_calls,
        allocated_bytes,
        parse_source_units_attempted,
        parsed_expressions,
        checked_expressions,
        typecheck_inference_call_visits,
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
                .evidence_setup_producer_pids
                .iter()
                .all(|pid| *pid != 0)
            && point
                .scored_samples
                .iter()
                .all(|sample| sample.producer_pid != 0)
            && point
                .evidence_scored_samples
                .iter()
                .all(|sample| sample.producer_pid != 0)
    });
    let evidence_parity_pass = [&baseline, &base, &doubled]
        .into_iter()
        .all(|point| point.evidence_parity_pass);
    let evaluation = ScalingEvaluation {
        source_identity_pass,
        process_isolation_pass,
        evidence_parity_pass,
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
        "typecheck-inference-call-visits" => Ok(point.typecheck_inference_call_visits.p95),
        "checked-calls" => Ok(point.checked_calls.p95),
        "checked-expressions" => Ok(point.checked_expressions.p95),
        "parse-source-units-attempted" => Ok(point.parse_source_units_attempted.p95),
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
        && evaluation.evidence_parity_pass
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
        || batch.initial_resident_rss_kib == 0
        || batch.final_resident_rss_kib == 0
        || batch.peak_rss_kib < batch.initial_resident_rss_kib
        || batch.peak_rss_kib < batch.final_resident_rss_kib
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
        let mut failures = Vec::new();
        let mut require = |pass: bool, label: &'static str| {
            if !pass {
                failures.push(label);
            }
        };
        require(sample.sequence == index, "sequence");
        require(sample.scored == (index >= setup_samples), "scored");
        require(sample.direction == expected_direction, "direction");
        require(
            sample.previous_revision.checked_add(1) == Some(sample.revision),
            "revision-step",
        );
        require(sample.revision == expected_revision, "expected-revision");
        require(
            sample.last_good_revision_before == sample.previous_revision,
            "last-good-before",
        );
        require(
            sample.published_revision == sample.revision,
            "published-revision",
        );
        require(sample.resident_rss_kib > 0, "resident-rss");
        require(
            sample.resident_rss_kib <= batch.peak_rss_kib,
            "resident-rss-within-peak",
        );
        require(sample.diagnostic_count == 0, "diagnostics");
        require(
            sample.full_document_typecheck_coverage,
            "typecheck-coverage",
        );
        require(warm_edit_times_valid(sample), "timing-order");
        require(
            has_single_unit_edit_frontend_work(&sample.diagnostics_work),
            "diagnostics-work",
        );
        require(
            sample.diagnostics_work.semantic_graph_nodes == 0,
            "diagnostics-phase-boundary",
        );
        require(
            has_fully_reused_preview_frontend_work(&sample.preview_work),
            "preview-work",
        );
        require(
            sample.preview_work.semantic_graph_nodes > 0,
            "preview-semantics",
        );
        require(
            sample
                .diagnostics_phase
                .values()
                .into_iter()
                .all(finite_nonnegative),
            "diagnostics-phase-times",
        );
        require(
            sample
                .preview_phase
                .values()
                .into_iter()
                .all(finite_nonnegative),
            "preview-phase-times",
        );
        if !failures.is_empty() {
            return Err(format!(
                "warm edit sample {index} has inconsistent evidence: {}",
                failures.join(", ")
            )
            .into());
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

fn has_single_unit_edit_frontend_work(work: &WorkSample) -> bool {
    work.has_complete_frontend_work()
        && work.parse.source_units_attempted == 1
        && work.parse.source_units_parsed == 1
        && work.parse.source_units_reused.checked_add(1) == Some(work.source_units)
}

fn has_fully_reused_preview_frontend_work(work: &WorkSample) -> bool {
    work.source_units > 0
        && work.parsed_expressions > 0
        && work.checked_expressions > 0
        && work.parse.source_units_attempted == 0
        && work.parse.source_units_parsed == 0
        && work.parse.source_units_reused == work.source_units
        && work.typecheck.owner_statements > 0
        && work.typecheck.owner_expressions > 0
        && work.typecheck.owner_local_constraints > 0
        && work.typecheck.owner_unification_steps > 0
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
        || sample.work.source_units == 0
        || sample.work.parsed_expressions == 0
        || sample.work.checked_expressions == 0
        || !sample.work.has_cold_complete_frontend_work()
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
    const PREFIX: &str = "boon_semantic dependency_manifest_v7 projection_graph:counts ";
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
        || budget.protocol.product_producer.is_empty()
        || budget.protocol.evidence_producer.is_empty()
        || budget.protocol.product_producer == budget.protocol.evidence_producer
        || budget.protocol.product_kind.is_empty()
        || budget.protocol.product_allocator.is_empty()
        || budget.protocol.evidence_kind.is_empty()
        || budget.protocol.evidence_allocator.is_empty()
        || budget.protocol.evidence_instrumentation != "thread-local-rust-global-allocator"
        || budget.protocol.evidence_counter_scope
            != "single-compiler-thread-rust-global-allocator-events"
        || budget.protocol.target_cpu.is_empty()
        || budget.protocol.profile_options.is_empty()
        || budget.protocol.producer_pair_schedule != "alternating-by-observation-index"
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
    safe_relative_path(&budget.protocol.product_producer, "product producer path")?;
    safe_relative_path(&budget.protocol.evidence_producer, "evidence producer path")?;
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
            "contextual-call-site-count" => (SampleIntent::Verified, "semantic-graph-nodes"),
            "static-branch-count" => (SampleIntent::Verified, "checked-expressions"),
            "source-unit-count" => (SampleIntent::Diagnostics, "parse-source-units-attempted"),
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
    let expected_identity = current_identity(workspace)?;
    if report.identity != expected_identity {
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
    validate_interaction_report_producer(
        workspace,
        &expected_identity,
        &report.producers.product,
        &budget.protocol.product_producer,
        &budget.protocol.product_kind,
        &budget.protocol.product_allocator,
        "none",
        "none",
        &budget.protocol,
    )?;
    validate_interaction_report_producer(
        workspace,
        &expected_identity,
        &report.producers.evidence,
        &budget.protocol.evidence_producer,
        &budget.protocol.evidence_kind,
        &budget.protocol.evidence_allocator,
        &budget.protocol.evidence_instrumentation,
        &budget.protocol.evidence_counter_scope,
        &budget.protocol,
    )?;
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
    validate_warm_batch(
        workspace,
        &report.warm.evidence_raw,
        &budget.warm,
        setup_samples,
        scored_samples,
    )?;
    if report.warm.raw.producer != report.producers.product.metadata
        || report.warm.evidence_raw.producer != report.producers.evidence.metadata
    {
        return Err("warm compiler producer metadata differs from report provenance".into());
    }
    validate_warm_product_evidence_parity(&report.warm.raw, &report.warm.evidence_raw)?;
    let expected_warm = warm_report(
        report.warm.raw.clone(),
        report.warm.evidence_raw.clone(),
        &budget.warm,
    )?;
    if report.warm != expected_warm {
        return Err("compiler interaction warm summaries/evaluation are inconsistent".into());
    }
    if report.scaling.len() != budget.scaling.workloads.len() {
        return Err("compiler interaction scaling report count is invalid".into());
    }
    for (scaling, workload) in report.scaling.iter().zip(&budget.scaling.workloads) {
        validate_scaling_point(
            &scaling.baseline,
            &report.producers,
            workload,
            workload.baseline_size,
            setup_samples,
            scored_samples,
        )?;
        validate_scaling_point(
            &scaling.base,
            &report.producers,
            workload,
            workload.base_size,
            setup_samples,
            scored_samples,
        )?;
        validate_scaling_point(
            &scaling.doubled,
            &report.producers,
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
        if !scaling_reports_equivalent(scaling, &expected) {
            return Err(format!(
                "scaling report `{}` is inconsistent: status {:?}/{:?}, elapsed ratio {:?}/{:?}, allocation-call ratio {:?}/{:?}, allocated-byte ratio {:?}/{:?}, owning-work ratio {:?}/{:?}, evaluation {:?}/{:?}",
                workload.id,
                scaling.status,
                expected.status,
                scaling.elapsed_ms_fixed_overhead_ratio,
                expected.elapsed_ms_fixed_overhead_ratio,
                scaling.allocation_calls_fixed_overhead_ratio,
                expected.allocation_calls_fixed_overhead_ratio,
                scaling.allocated_bytes_fixed_overhead_ratio,
                expected.allocated_bytes_fixed_overhead_ratio,
                scaling.owning_work_fixed_overhead_ratio,
                expected.owning_work_fixed_overhead_ratio,
                scaling.evaluation,
                expected.evaluation,
            )
            .into());
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

fn scaling_reports_equivalent(left: &ScalingReport, right: &ScalingReport) -> bool {
    left.id == right.id
        && left.intent == right.intent
        && left.owning_work_counter == right.owning_work_counter
        && close_f64(left.maximum_doubling_ratio, right.maximum_doubling_ratio)
        && left.status == right.status
        && left.baseline == right.baseline
        && left.base == right.base
        && left.doubled == right.doubled
        && close_optional_f64(
            left.elapsed_ms_fixed_overhead_ratio,
            right.elapsed_ms_fixed_overhead_ratio,
        )
        && close_optional_f64(
            left.allocation_calls_fixed_overhead_ratio,
            right.allocation_calls_fixed_overhead_ratio,
        )
        && close_optional_f64(
            left.allocated_bytes_fixed_overhead_ratio,
            right.allocated_bytes_fixed_overhead_ratio,
        )
        && close_optional_f64(
            left.owning_work_fixed_overhead_ratio,
            right.owning_work_fixed_overhead_ratio,
        )
        && left.evaluation == right.evaluation
}

fn close_optional_f64(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => close_f64(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn close_f64(left: f64, right: f64) -> bool {
    finite_nonnegative(left)
        && finite_nonnegative(right)
        && (left - right).abs() <= 1.0e-12 * left.abs().max(right.abs()).max(1.0)
}

#[allow(clippy::too_many_arguments)]
fn validate_interaction_report_producer(
    workspace: &Path,
    identity: &ExpectedIdentity,
    producer: &ProducerIdentity,
    expected_path: &str,
    expected_kind: &str,
    expected_allocator: &str,
    expected_instrumentation: &str,
    expected_counter_scope: &str,
    protocol: &BudgetProtocol,
) -> ToolResult<()> {
    if producer.path != expected_path {
        return Err(format!(
            "compiler interaction producer path is {}; expected {expected_path}",
            producer.path
        )
        .into());
    }
    let path = workspace.join(safe_relative_path(expected_path, "producer path")?);
    require_current_prebuilt_producer(workspace, &path)?;
    let digest = sha256_file(&path)?;
    if producer.sha256 != digest.as_str() {
        return Err(format!("compiler interaction producer {expected_path} is stale").into());
    }
    validate_interaction_producer(
        workspace,
        identity,
        &producer.metadata,
        digest.as_str(),
        expected_path,
        expected_kind,
        expected_allocator,
        expected_instrumentation,
        expected_counter_scope,
        protocol,
    )
}

fn validate_scaling_point(
    point: &ScalingPoint,
    producers: &ProducerSetIdentity,
    budget: &ScalingWorkloadBudget,
    size: usize,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<()> {
    if point.size != size
        || point.setup_producer_pids.len() != setup_samples
        || point.evidence_setup_producer_pids.len() != setup_samples
        || point.scored_samples.len() != scored_samples
        || point.evidence_scored_samples.len() != scored_samples
        || point.setup_producer_pids.iter().any(|pid| *pid == 0)
        || point
            .evidence_setup_producer_pids
            .iter()
            .any(|pid| *pid == 0)
        || !point.evidence_parity_pass
    {
        return Err(format!(
            "scaling point {}/{} has invalid sampling shape",
            budget.id, size
        )
        .into());
    }
    for sample in &point.scored_samples {
        validate_scaling_sample(sample, budget, size)?;
        if sample.producer != producers.product.metadata {
            return Err("scaling product producer metadata changed".into());
        }
    }
    for sample in &point.evidence_scored_samples {
        validate_scaling_sample(sample, budget, size)?;
        if sample.producer != producers.evidence.metadata {
            return Err("scaling evidence producer metadata changed".into());
        }
    }
    for (product, evidence) in point
        .scored_samples
        .iter()
        .zip(&point.evidence_scored_samples)
    {
        validate_scaling_product_evidence_parity(product, evidence)?;
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
        point.evidence_setup_producer_pids.clone(),
        point.scored_samples.clone(),
        point.evidence_scored_samples.clone(),
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
        product_kind: budget.protocol.product_kind.clone(),
        product_allocator: budget.protocol.product_allocator.clone(),
        evidence_kind: budget.protocol.evidence_kind.clone(),
        evidence_allocator: budget.protocol.evidence_allocator.clone(),
        evidence_instrumentation: budget.protocol.evidence_instrumentation.clone(),
        evidence_counter_scope: budget.protocol.evidence_counter_scope.clone(),
        target_cpu: budget.protocol.target_cpu.clone(),
        profile_options: budget.protocol.profile_options.clone(),
        rustflags: budget.protocol.rustflags.clone(),
        producer_pair_schedule: budget.protocol.producer_pair_schedule.clone(),
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
