use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::compiler_producer::{
    ExpectedProducer, ProducerIdentity, ProducerMetadata, ProducerSetIdentity,
    validate_producer_metadata,
};
use crate::compiler_work_sample::{WorkSample, require_current_prebuilt_producer};
use crate::report_v2::{
    ExpectedIdentity, ReportStatus, ToolResult, current_identity, sha256_bytes, sha256_file,
    unix_time_ms,
};

const REPORT_FORMAT_VERSION: u16 = 9;
const PRODUCER_FORMAT_VERSION: u16 = 8;
const BUDGET_FORMAT_VERSION: u16 = 3;
const REPORT_CONTRACT: &str = "boon-compiler-performance-v8";
const DEFAULT_BUDGET: &str = "budgets/compiler.toml";
const MAX_BUDGET_BYTES: u64 = 64 * 1024;
const MAX_REPORT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SAMPLE_OUTPUT_BYTES: usize = 512 * 1024;
const MAX_SAMPLE_COUNT: usize = 128;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetManifest {
    format_version: u16,
    owner_plan: String,
    report: String,
    protocol: BudgetProtocol,
    fixtures: Vec<FixtureBudget>,
    warm: WarmBudget,
    scaling: ScalingBudget,
}

#[derive(Clone, Debug, Deserialize)]
struct ExampleLineManifest {
    example: Vec<ExampleLineEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct ExampleLineEntry {
    source: String,
    #[serde(default)]
    source_files: Vec<String>,
    #[serde(default)]
    build_files: Vec<String>,
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
struct FixtureBudget {
    id: String,
    source: String,
    package_source_lines: u64,
    compiler_input_source_lines: u64,
    checked_diagnostics_p95_ms: f64,
    verified_machine_plan_p95_ms: f64,
    peak_rss_mib_max: u64,
    machine_plan_sha256: String,
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
    intent: String,
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
enum ColdMode {
    FreshProcess,
    EmptySession,
}

impl ColdMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::FreshProcess => "fresh-process",
            Self::EmptySession => "empty-session",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SampleIntent {
    Diagnostics,
    Verified,
}

impl SampleIntent {
    fn as_str(self) -> &'static str {
        match self {
            Self::Diagnostics => "diagnostics",
            Self::Verified => "verified",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CompilerPerformanceReport {
    format_version: u16,
    contract: String,
    status: ReportStatus,
    run_classification: RunClassification,
    generated_unix_ms: u64,
    identity: ExpectedIdentity,
    budget: BudgetIdentity,
    producers: ProducerSetIdentity,
    protocol: ProtocolEvidence,
    phase_acceptance: PhaseAcceptanceProjection,
    fixtures: Vec<FixtureReport>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PhaseAcceptanceProjection {
    phase_1_cold_diagnostics: ColdPhaseAcceptance,
    phase_3_verified_runnable: ColdPhaseAcceptance,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ColdPhaseAcceptance {
    status: ReportStatus,
    fixture_count: usize,
    evaluated_mode_count: usize,
    fresh_process_pass: bool,
    empty_session_pass: bool,
    p95_budget_pass: bool,
    peak_rss_budget_pass: bool,
    cache_disabled_pass: bool,
    within_mode_source_determinism_pass: bool,
    within_mode_result_determinism_pass: bool,
    cross_mode_source_parity_pass: bool,
    cross_mode_result_parity_pass: bool,
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
    build_profile: String,
    target_profile: String,
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
    cold_modes: Vec<String>,
    os_page_cache: String,
    sample_process_isolation: String,
    peak_rss_unit: String,
    peak_rss_scope: String,
    percentile_method: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct FixtureReport {
    id: String,
    source: String,
    package_source_lines: u64,
    compiler_input_source_lines: u64,
    checked_diagnostics_p95_budget_ms: f64,
    verified_machine_plan_p95_budget_ms: f64,
    peak_rss_mib_max: u64,
    expected_machine_plan_sha256: String,
    observed_source_bundle_digests: Vec<String>,
    observed_diagnostics_fingerprint_v1: Vec<String>,
    status: ReportStatus,
    modes: Vec<ModeReport>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ModeReport {
    mode: ColdMode,
    status: ReportStatus,
    diagnostics: MetricReport,
    verified: MetricReport,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct MetricReport {
    intent: SampleIntent,
    setup_samples: usize,
    scored_samples: Vec<Sample>,
    evidence_scored_samples: Vec<Sample>,
    evidence_parity_pass: bool,
    cache_hit_count: u64,
    observed_source_bundle_digests: Vec<String>,
    observed_diagnostics_fingerprint_v1: Vec<String>,
    observed_plan_sha256: Vec<String>,
    elapsed_ms: MillisSummary,
    compiler_cpu_ms: MillisSummary,
    compiler_minor_page_faults: CountSummary,
    compiler_major_page_faults: CountSummary,
    peak_rss_kib: RssSummary,
    allocations: AllocationSummary,
    work: WorkSummary,
    phase_ms: PhaseSummary,
    export: ExportSummary,
    evaluation: MetricEvaluation,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    producer_pid: u32,
    observation_started_unix_us: u64,
    compiler_artifact_ready_unix_us: u64,
    elapsed_ms: f64,
    #[serde(default)]
    normative_elapsed_ms: f64,
    #[serde(default)]
    process_to_artifact_ms: f64,
    #[serde(default)]
    process_exit_ms: f64,
    #[serde(default)]
    startup_and_cli_ms: f64,
    #[serde(default)]
    post_artifact_ms: f64,
    compiler_cpu_ms: f64,
    compiler_minor_page_faults: u64,
    compiler_major_page_faults: u64,
    peak_rss_kib: u64,
    source_bundle_digest_v1: String,
    diagnostics_fingerprint_v1: Option<String>,
    diagnostic_count: usize,
    full_document_typecheck_coverage: Option<bool>,
    plan_sha256: Option<String>,
    allocations: AllocationSample,
    work: WorkSample,
    phase: PhaseSample,
    export: ExportSample,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ExportSample {
    kind: String,
    elapsed_ms: f64,
    output_bytes: u64,
    sha256: Option<String>,
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
    fn values(self) -> [f64; 8] {
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct SampleBatch {
    format_version: u16,
    producer: ProducerMetadata,
    source: String,
    intent: SampleIntent,
    compiler_state: String,
    target_profile: String,
    program_role: String,
    compiler_threads: usize,
    compiler_caches: String,
    peak_rss_unit: String,
    peak_rss_scope: String,
    cache_hit_count: u64,
    samples: Vec<Sample>,
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
struct RssSummary {
    sample_count: usize,
    p50: u64,
    p95: u64,
    p99: u64,
    max: u64,
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct AllocationSummary {
    allocation_calls: CountSummary,
    allocated_bytes: CountSummary,
    deallocation_calls: CountSummary,
    deallocated_bytes: CountSummary,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkSummary {
    source_units: CountSummary,
    parsed_expressions: CountSummary,
    checked_expressions: CountSummary,
    checked_calls: CountSummary,
    semantic_graph_nodes: CountSummary,
    cancellation_checkpoints: CountSummary,
    parse_source_units_attempted: CountSummary,
    parse_source_units_parsed: CountSummary,
    parse_source_units_reused: CountSummary,
    parse_expression_visits: CountSummary,
    parse_validation_visits: CountSummary,
    typecheck_inference_expression_visits: CountSummary,
    typecheck_inference_call_visits: CountSummary,
    typecheck_diagnostic_replay_requests: CountSummary,
    typecheck_diagnostic_replay_misses: CountSummary,
    kernel_definition_modules: CountSummary,
    kernel_linked_operations: CountSummary,
    kernel_scheduled_work_items: CountSummary,
    kernel_variables: CountSummary,
    kernel_operations: CountSummary,
    kernel_activations: CountSummary,
    kernel_rich_output_flow_exports: CountSummary,
    kernel_term_intern_requests: CountSummary,
    kernel_term_intern_hits: CountSummary,
    kernel_dynamic_dependency_edges: CountSummary,
    kernel_frozen_type_store_term_rows: CountSummary,
    kernel_frozen_type_store_name_rows: CountSummary,
    kernel_frozen_type_store_child_rows: CountSummary,
    kernel_frozen_type_store_variant_rows: CountSummary,
    kernel_frozen_type_store_object_shape_rows: CountSummary,
    kernel_frozen_type_store_object_field_rows: CountSummary,
    kernel_frozen_type_store_semantic_order_rows: CountSummary,
    kernel_frozen_type_store_payload_len_bytes: CountSummary,
    kernel_frozen_type_store_payload_capacity_bytes: CountSummary,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PhaseSummary {
    parse: MillisSummary,
    typecheck: MillisSummary,
    semantic: MillisSummary,
    contract_verify: MillisSummary,
    ir_lower: MillisSummary,
    ir_validation: MillisSummary,
    backend: MillisSummary,
    plan_validation: MillisSummary,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ExportSummary {
    kinds: Vec<String>,
    elapsed_ms: MillisSummary,
    output_bytes: CountSummary,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct MetricEvaluation {
    status: ReportStatus,
    evidence_parity_pass: bool,
    p95_budget_pass: bool,
    peak_rss_budget_pass: bool,
    cache_disabled_pass: bool,
    source_digest_pass: bool,
    diagnostics_fingerprint_pass: bool,
    machine_plan_hash_pass: bool,
}

#[derive(Default)]
struct CollectedSamples {
    scored: Vec<Sample>,
    evidence_scored: Vec<Sample>,
    evidence_parity_pass: bool,
    cache_hit_count: u64,
    source_digests: BTreeSet<String>,
    diagnostics_fingerprints: BTreeSet<String>,
    plan_hashes: BTreeSet<String>,
}

#[derive(Default)]
struct ObservedProducerMetadata {
    product: Option<ProducerMetadata>,
    evidence: Option<ProducerMetadata>,
}

struct ExecutedSampleBatch {
    batch: SampleBatch,
    process_started_unix_us: u64,
    process_exit_ms: f64,
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
        None => workspace.join(safe_relative_path(&budget.report, "budget report path")?),
    };
    if !check_existing {
        collect(
            workspace,
            &report_path,
            &budget,
            budget_digest.as_str(),
            setup_samples,
            scored_samples,
        )?;
    }
    let report = validate_existing(
        workspace,
        &report_path,
        &budget,
        budget_digest.as_str(),
        setup_samples,
        scored_samples,
    )?;
    println!(
        "{} compiler performance {}: {} fixtures, status {}, phase 1 cold diagnostics {}, phase 3 verified runnable {}, {}",
        if check_existing { "checked" } else { "wrote" },
        report_path.display(),
        report.fixtures.len(),
        status_name(report.status),
        status_name(report.phase_acceptance.phase_1_cold_diagnostics.status),
        status_name(report.phase_acceptance.phase_3_verified_runnable.status),
        match report.run_classification {
            RunClassification::Acceptance => "acceptance protocol",
            RunClassification::DevelopmentNonAcceptance => {
                "development-only non-acceptance protocol"
            }
        }
    );
    for fixture in &report.fixtures {
        for mode in &fixture.modes {
            let diagnostics = &mode.diagnostics;
            let verified = &mode.verified;
            println!(
                "  {} {}: diagnostics_ms[p50={:.3},p95={:.3},cpu_p50={:.3}] verified_ms[p50={:.3},p95={:.3},cpu_p50={:.3}] phases_p50_ms[semantic={:.3},where_verify={:.3},ir_lower={:.3},ir_verify={:.3},backend={:.3},plan_verify={:.3}] phases_p95_ms[semantic={:.3},where_verify={:.3},ir_lower={:.3},ir_verify={:.3},backend={:.3},plan_verify={:.3}] report_export_ms[p50={:.3},p95={:.3}]",
                fixture.id,
                mode.mode.as_str(),
                diagnostics.elapsed_ms.p50,
                diagnostics.elapsed_ms.p95,
                diagnostics.compiler_cpu_ms.p50,
                verified.elapsed_ms.p50,
                verified.elapsed_ms.p95,
                verified.compiler_cpu_ms.p50,
                verified.phase_ms.semantic.p50,
                verified.phase_ms.contract_verify.p50,
                verified.phase_ms.ir_lower.p50,
                verified.phase_ms.ir_validation.p50,
                verified.phase_ms.backend.p50,
                verified.phase_ms.plan_validation.p50,
                verified.phase_ms.semantic.p95,
                verified.phase_ms.contract_verify.p95,
                verified.phase_ms.ir_lower.p95,
                verified.phase_ms.ir_validation.p95,
                verified.phase_ms.backend.p95,
                verified.phase_ms.plan_validation.p95,
                verified.export.elapsed_ms.p50,
                verified.export.elapsed_ms.p95,
            );
        }
    }
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
    let product_digest = sha256_file(&product_path)?;
    let evidence_digest = sha256_file(&evidence_path)?;
    let mut observed_producers = ObservedProducerMetadata::default();
    let mut fixtures = Vec::with_capacity(budget.fixtures.len());
    for fixture in &budget.fixtures {
        fixtures.push(collect_fixture(
            workspace,
            &before,
            &product_path,
            product_digest.as_str(),
            &evidence_path,
            evidence_digest.as_str(),
            &budget.protocol,
            &mut observed_producers,
            fixture,
            setup_samples,
            scored_samples,
        )?);
    }
    let after_product_digest = sha256_file(&product_path)?;
    let after_evidence_digest = sha256_file(&evidence_path)?;
    if after_product_digest != product_digest || after_evidence_digest != evidence_digest {
        return Err(
            "a release compiler sample producer changed while performance was being measured"
                .into(),
        );
    }
    let after = current_identity(workspace)?;
    if before != after {
        return Err(
            "workspace identity changed while compiler performance was being measured; no current report can be written"
                .into(),
        );
    }

    let phase_acceptance = phase_acceptance_projection(&fixtures);
    let status = if fixtures
        .iter()
        .all(|fixture| fixture.status == ReportStatus::Pass)
    {
        ReportStatus::Pass
    } else {
        ReportStatus::Fail
    };
    let run_classification = classification(budget, setup_samples, scored_samples);
    let report = CompilerPerformanceReport {
        format_version: REPORT_FORMAT_VERSION,
        contract: REPORT_CONTRACT.to_owned(),
        status,
        run_classification,
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
                sha256: product_digest.as_str().to_owned(),
                metadata: observed_producers
                    .product
                    .ok_or("no product producer metadata was observed")?,
            },
            evidence: ProducerIdentity {
                path: budget.protocol.evidence_producer.clone(),
                sha256: evidence_digest.as_str().to_owned(),
                metadata: observed_producers
                    .evidence
                    .ok_or("no evidence producer metadata was observed")?,
            },
        },
        protocol: protocol_evidence(budget, setup_samples, scored_samples),
        phase_acceptance,
        fixtures,
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

fn collect_fixture(
    workspace: &Path,
    identity: &ExpectedIdentity,
    product: &Path,
    product_digest: &str,
    evidence: &Path,
    evidence_digest: &str,
    protocol: &BudgetProtocol,
    observed_producers: &mut ObservedProducerMetadata,
    budget: &FixtureBudget,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<FixtureReport> {
    let source = safe_relative_path(&budget.source, "fixture source")?;
    let source_path = workspace.join(&source);
    if !source_path.is_file() {
        return Err(format!(
            "compiler performance fixture {} is missing at {}",
            budget.id,
            source_path.display()
        )
        .into());
    }
    let mut modes = Vec::with_capacity(2);
    for mode in [ColdMode::FreshProcess, ColdMode::EmptySession] {
        let diagnostics = collect_metric(
            workspace,
            identity,
            product,
            product_digest,
            evidence,
            evidence_digest,
            protocol,
            observed_producers,
            &budget.source,
            mode,
            SampleIntent::Diagnostics,
            setup_samples,
            scored_samples,
            budget,
        )?;
        let verified = collect_metric(
            workspace,
            identity,
            product,
            product_digest,
            evidence,
            evidence_digest,
            protocol,
            observed_producers,
            &budget.source,
            mode,
            SampleIntent::Verified,
            setup_samples,
            scored_samples,
            budget,
        )?;
        let status = if diagnostics.evaluation.status == ReportStatus::Pass
            && verified.evaluation.status == ReportStatus::Pass
        {
            ReportStatus::Pass
        } else {
            ReportStatus::Fail
        };
        modes.push(ModeReport {
            mode,
            status,
            diagnostics,
            verified,
        });
    }
    let observed_source_bundle_digests = modes
        .iter()
        .flat_map(|mode| {
            mode.diagnostics
                .observed_source_bundle_digests
                .iter()
                .chain(mode.verified.observed_source_bundle_digests.iter())
        })
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let observed_diagnostics_fingerprint_v1 = modes
        .iter()
        .flat_map(|mode| mode.diagnostics.observed_diagnostics_fingerprint_v1.iter())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let status = if modes.iter().all(|mode| mode.status == ReportStatus::Pass)
        && observed_source_bundle_digests.len() == 1
        && observed_diagnostics_fingerprint_v1.len() == 1
    {
        ReportStatus::Pass
    } else {
        ReportStatus::Fail
    };
    Ok(FixtureReport {
        id: budget.id.clone(),
        source: budget.source.clone(),
        package_source_lines: budget.package_source_lines,
        compiler_input_source_lines: budget.compiler_input_source_lines,
        checked_diagnostics_p95_budget_ms: budget.checked_diagnostics_p95_ms,
        verified_machine_plan_p95_budget_ms: budget.verified_machine_plan_p95_ms,
        peak_rss_mib_max: budget.peak_rss_mib_max,
        expected_machine_plan_sha256: budget.machine_plan_sha256.clone(),
        observed_source_bundle_digests,
        observed_diagnostics_fingerprint_v1,
        status,
        modes,
    })
}

#[allow(clippy::too_many_arguments)]
fn collect_metric(
    workspace: &Path,
    identity: &ExpectedIdentity,
    product: &Path,
    product_digest: &str,
    evidence: &Path,
    evidence_digest: &str,
    protocol: &BudgetProtocol,
    observed_producers: &mut ObservedProducerMetadata,
    source: &str,
    mode: ColdMode,
    intent: SampleIntent,
    setup_samples: usize,
    scored_samples: usize,
    fixture_budget: &FixtureBudget,
) -> ToolResult<MetricReport> {
    let mut collected = CollectedSamples::default();
    // Every cold observation gets a new OS process. `fresh-process` exercises
    // the direct compiler entrypoint; `empty-session` exercises a newly
    // constructed in-process CompilerSession. Keeping one process per sample
    // prevents allocator high-water marks or future process-local caches from
    // leaking setup/warm state into the empty-session RSS and timing samples.
    for sample_index in 0..setup_samples + scored_samples {
        let product_first = sample_index % 2 == 0;
        let (product_batch, evidence_batch) = if product_first {
            let product_batch = run_sample_batch(workspace, product, source, mode, intent, 1)?;
            let evidence_batch = run_sample_batch(workspace, evidence, source, mode, intent, 1)?;
            (product_batch, evidence_batch)
        } else {
            let evidence_batch = run_sample_batch(workspace, evidence, source, mode, intent, 1)?;
            let product_batch = run_sample_batch(workspace, product, source, mode, intent, 1)?;
            (product_batch, evidence_batch)
        };
        let product_batch = enrich_and_validate_batch(
            workspace,
            identity,
            product_digest,
            &protocol.product_producer,
            &protocol.product_kind,
            &protocol.product_allocator,
            "none",
            "none",
            protocol,
            product_batch,
            source,
            mode,
            intent,
            1,
        )?;
        let evidence_batch = enrich_and_validate_batch(
            workspace,
            identity,
            evidence_digest,
            &protocol.evidence_producer,
            &protocol.evidence_kind,
            &protocol.evidence_allocator,
            &protocol.evidence_instrumentation,
            &protocol.evidence_counter_scope,
            protocol,
            evidence_batch,
            source,
            mode,
            intent,
            1,
        )?;
        observe_producer_metadata(
            &mut observed_producers.product,
            &product_batch.producer,
            "product",
        )?;
        observe_producer_metadata(
            &mut observed_producers.evidence,
            &evidence_batch.producer,
            "evidence",
        )?;
        validate_product_evidence_parity(&product_batch, &evidence_batch)?;
        absorb_batch(
            &mut collected,
            product_batch,
            source,
            intent,
            sample_index >= setup_samples,
        )?;
        absorb_evidence_batch(
            &mut collected,
            evidence_batch,
            source,
            intent,
            sample_index >= setup_samples,
        )?;
    }
    Ok(metric_report(
        intent,
        setup_samples,
        collected,
        fixture_budget,
    ))
}

fn run_sample_batch(
    workspace: &Path,
    producer: &Path,
    source: &str,
    mode: ColdMode,
    intent: SampleIntent,
    samples: usize,
) -> ToolResult<ExecutedSampleBatch> {
    let sample_count = samples.to_string();
    let process_started_unix_us = unix_time_us()?;
    let process_started = Instant::now();
    let output = Command::new(producer)
        .current_dir(workspace)
        .env("RAYON_NUM_THREADS", "1")
        .env_remove("BOON_KERNEL_EXPERIMENTAL_PARALLEL")
        .args([
            "compiler-sample",
            source,
            "--intent",
            intent.as_str(),
            "--mode",
            mode.as_str(),
            "--samples",
            sample_count.as_str(),
        ])
        .output()?;
    let process_exit_ms = duration_ms(process_started.elapsed());
    if !output.status.success() {
        return Err(format!(
            "compiler sample {} {} failed with {}: {}",
            source,
            intent.as_str(),
            output.status,
            bounded_lossy(&output.stderr, 4096)
        )
        .into());
    }
    if output.stdout.is_empty() || output.stdout.len() > MAX_SAMPLE_OUTPUT_BYTES {
        return Err(format!(
            "compiler sample {} {} stdout is {} bytes; expected 1..={MAX_SAMPLE_OUTPUT_BYTES}",
            source,
            intent.as_str(),
            output.stdout.len()
        )
        .into());
    }
    let batch: SampleBatch = serde_json::from_slice(&output.stdout).map_err(|error| {
        format!(
            "parse compiler sample {} {} JSON: {error}; stdout: {}",
            source,
            intent.as_str(),
            bounded_lossy(&output.stdout, 4096)
        )
    })?;
    Ok(ExecutedSampleBatch {
        batch,
        process_started_unix_us,
        process_exit_ms,
    })
}

#[allow(clippy::too_many_arguments)]
fn enrich_and_validate_batch(
    workspace: &Path,
    identity: &ExpectedIdentity,
    producer_digest: &str,
    producer_path: &str,
    product_kind: &str,
    allocator_id: &str,
    allocation_instrumentation: &str,
    allocation_counter_scope: &str,
    protocol: &BudgetProtocol,
    execution: ExecutedSampleBatch,
    source: &str,
    mode: ColdMode,
    intent: SampleIntent,
    samples: usize,
) -> ToolResult<SampleBatch> {
    let ExecutedSampleBatch {
        mut batch,
        process_started_unix_us,
        process_exit_ms,
    } = execution;
    validate_producer_metadata(
        workspace,
        identity,
        &batch.producer,
        ExpectedProducer {
            path: producer_path,
            sha256: producer_digest,
            product_kind,
            allocator_id,
            allocation_instrumentation,
            allocation_counter_scope,
            cargo_profile: &protocol.build_profile,
            target_cpu: &protocol.target_cpu,
            profile_options: &protocol.profile_options,
            rustflags: &protocol.rustflags,
        },
    )?;
    if batch.samples.len() != 1 || samples != 1 {
        return Err(
            "external process endpoint accounting requires one observation per process".into(),
        );
    }
    let sample = batch.samples.first_mut().expect("one sample checked");
    let artifact_delta_us = sample
        .compiler_artifact_ready_unix_us
        .checked_sub(process_started_unix_us)
        .ok_or("compiler artifact timestamp predates the collector process start")?;
    let process_to_artifact_ms = artifact_delta_us as f64 / 1_000.0;
    if process_to_artifact_ms > process_exit_ms + 1.0 {
        return Err(format!(
            "compiler artifact endpoint {process_to_artifact_ms:.3}ms exceeds process-exit endpoint {process_exit_ms:.3}ms"
        )
        .into());
    }
    sample.process_to_artifact_ms = process_to_artifact_ms;
    sample.process_exit_ms = process_exit_ms;
    sample.startup_and_cli_ms = (process_to_artifact_ms - sample.elapsed_ms).max(0.0);
    sample.post_artifact_ms = (process_exit_ms - process_to_artifact_ms).max(0.0);
    sample.normative_elapsed_ms = match mode {
        ColdMode::FreshProcess => process_to_artifact_ms,
        ColdMode::EmptySession => sample.elapsed_ms,
    };
    validate_sample_batch(
        &batch,
        source,
        mode,
        intent,
        samples,
        allocation_instrumentation != "none",
    )?;
    Ok(batch)
}

fn observe_producer_metadata(
    observed: &mut Option<ProducerMetadata>,
    candidate: &ProducerMetadata,
    lane: &str,
) -> ToolResult<()> {
    match observed {
        Some(previous) if previous != candidate => {
            Err(format!("{lane} producer metadata changed during collection").into())
        }
        Some(_) => Ok(()),
        None => {
            *observed = Some(candidate.clone());
            Ok(())
        }
    }
}

fn validate_product_evidence_parity(
    product: &SampleBatch,
    evidence: &SampleBatch,
) -> ToolResult<()> {
    if product.source != evidence.source
        || product.intent != evidence.intent
        || product.compiler_state != evidence.compiler_state
        || product.target_profile != evidence.target_profile
        || product.program_role != evidence.program_role
        || product.compiler_threads != evidence.compiler_threads
        || product.compiler_caches != evidence.compiler_caches
        || product.cache_hit_count != evidence.cache_hit_count
        || product.samples.len() != evidence.samples.len()
    {
        return Err("product/evidence compiler sample batch contracts differ".into());
    }
    for (product, evidence) in product.samples.iter().zip(&evidence.samples) {
        validate_product_evidence_sample_parity(product, evidence)?;
    }
    Ok(())
}

fn validate_product_evidence_sample_parity(product: &Sample, evidence: &Sample) -> ToolResult<()> {
    if product.source_bundle_digest_v1 != evidence.source_bundle_digest_v1
        || product.diagnostics_fingerprint_v1 != evidence.diagnostics_fingerprint_v1
        || product.diagnostic_count != evidence.diagnostic_count
        || product.full_document_typecheck_coverage != evidence.full_document_typecheck_coverage
        || product.plan_sha256 != evidence.plan_sha256
        || product.work != evidence.work
    {
        return Err(
            "product/evidence compiler observations differ semantically or in owned work".into(),
        );
    }
    Ok(())
}

fn validate_sample_batch(
    batch: &SampleBatch,
    source: &str,
    mode: ColdMode,
    intent: SampleIntent,
    samples: usize,
    allocation_instrumented: bool,
) -> ToolResult<()> {
    if batch.format_version != PRODUCER_FORMAT_VERSION {
        return Err(format!(
            "compiler sample format is {}; expected {PRODUCER_FORMAT_VERSION}",
            batch.format_version
        )
        .into());
    }
    if batch.source != source || batch.intent != intent {
        return Err("compiler sample source or intent differs from the request".into());
    }
    if batch.compiler_state != mode.as_str() {
        return Err(format!(
            "compiler sample state is `{}`; expected `{}`",
            batch.compiler_state,
            mode.as_str(),
        )
        .into());
    }
    if batch.target_profile != "software_default"
        || batch.program_role != "client"
        || batch.compiler_threads != 1
        || batch.compiler_caches != "disabled"
        || batch.peak_rss_unit != "KiB"
        || batch.peak_rss_scope != "process-high-water-through-compiler-artifact"
    {
        return Err(format!(
            "compiler sample execution contract differs from the cold protocol: target={}, role={}, threads={}, caches={}, rss={}/{}",
            batch.target_profile,
            batch.program_role,
            batch.compiler_threads,
            batch.compiler_caches,
            batch.peak_rss_unit,
            batch.peak_rss_scope,
        )
        .into());
    }
    if batch.samples.len() != samples {
        return Err(format!(
            "compiler sample returned {} samples; expected {samples}",
            batch.samples.len()
        )
        .into());
    }
    for sample in &batch.samples {
        validate_sample_shape(sample, intent, allocation_instrumented)?;
    }
    Ok(())
}

fn validate_sample_shape(
    sample: &Sample,
    intent: SampleIntent,
    allocation_instrumented: bool,
) -> ToolResult<()> {
    if sample.producer_pid == 0
        || sample.observation_started_unix_us == 0
        || sample.compiler_artifact_ready_unix_us < sample.observation_started_unix_us
    {
        return Err("compiler sample process/time evidence is invalid".into());
    }
    if !finite_nonnegative(sample.elapsed_ms)
        || !finite_positive(sample.normative_elapsed_ms)
        || !finite_positive(sample.process_to_artifact_ms)
        || !finite_positive(sample.process_exit_ms)
        || !finite_nonnegative(sample.startup_and_cli_ms)
        || !finite_nonnegative(sample.post_artifact_ms)
        || !finite_nonnegative(sample.compiler_cpu_ms)
        || sample
            .phase
            .values()
            .into_iter()
            .any(|value| !finite_nonnegative(value))
    {
        return Err("compiler sample contains a non-finite or negative duration".into());
    }
    if sample.peak_rss_kib == 0 {
        return Err("compiler sample peak RSS is unavailable or zero".into());
    }
    if allocation_instrumented {
        if sample.allocations.allocation_calls == 0 || sample.allocations.allocated_bytes == 0 {
            return Err("instrumented compiler allocation counters are unavailable or zero".into());
        }
    } else if sample.allocations != AllocationSample::default() {
        return Err("uninstrumented product compiler emitted allocation counters".into());
    }
    if !sample.work.has_cold_complete_frontend_work() {
        return Err("compiler sample frontend work counters are incomplete or inconsistent".into());
    }
    validate_sha256(
        &sample.source_bundle_digest_v1,
        "sample SourceBundleDigestV1",
    )?;
    match (intent, sample.diagnostics_fingerprint_v1.as_deref()) {
        (SampleIntent::Diagnostics, Some(digest)) => {
            validate_sha256(digest, "sample diagnostics-fingerprint SHA-256")?;
        }
        (SampleIntent::Diagnostics, None) => {
            return Err("diagnostics sample omitted its diagnostics-fingerprint hash".into());
        }
        (SampleIntent::Verified, None) => {}
        (SampleIntent::Verified, Some(_)) => {
            return Err(
                "verified sample unexpectedly emitted a diagnostics-fingerprint hash".into(),
            );
        }
    }
    match (intent, &sample.plan_sha256) {
        (SampleIntent::Diagnostics, None) => {
            if sample.full_document_typecheck_coverage != Some(true) {
                return Err(
                    "diagnostics sample did not prove full-document typecheck coverage".into(),
                );
            }
            if sample.work.source_units == 0
                || sample.work.parsed_expressions == 0
                || sample.work.checked_expressions == 0
                || sample.work.semantic_graph_nodes != 0
            {
                return Err(
                    "diagnostics sample work counters are incomplete or cross phase boundaries"
                        .into(),
                );
            }
            if !finite_positive(sample.phase.parse_ms)
                || !finite_positive(sample.phase.typecheck_ms)
                || sample.phase.semantic_ms != 0.0
                || sample.phase.contract_verify_ms != 0.0
                || sample.phase.ir_lower_ms != 0.0
                || sample.phase.ir_validation_ms != 0.0
                || sample.phase.backend_ms != 0.0
                || sample.phase.plan_validation_ms != 0.0
            {
                return Err(
                    "diagnostics sample phase ownership is incomplete or overlapping".into(),
                );
            }
            if sample.export.kind != "none"
                || sample.export.elapsed_ms != 0.0
                || sample.export.output_bytes != 0
                || sample.export.sha256.is_some()
            {
                return Err("diagnostics sample unexpectedly performed report export".into());
            }
        }
        (SampleIntent::Verified, Some(digest)) => {
            validate_sha256(digest, "sample MachinePlan SHA-256")?;
            if sample.work.source_units == 0
                || sample.work.parsed_expressions == 0
                || sample.work.checked_expressions == 0
                || sample.work.semantic_graph_nodes == 0
            {
                return Err("verified sample omitted required compiler work counters".into());
            }
            if sample.full_document_typecheck_coverage.is_some()
                || !finite_positive(sample.phase.parse_ms)
                || !finite_positive(sample.phase.typecheck_ms)
                || !finite_positive(sample.phase.semantic_ms)
                || !finite_positive(sample.phase.contract_verify_ms)
                || !finite_positive(sample.phase.ir_lower_ms)
                || !finite_positive(sample.phase.ir_validation_ms)
                || !finite_positive(sample.phase.backend_ms)
                || !finite_positive(sample.phase.plan_validation_ms)
            {
                return Err("verified sample omitted a required compiler/report phase".into());
            }
            if sample.export.kind != "pretty-json-machine-plan-sha256"
                || !finite_positive(sample.export.elapsed_ms)
                || sample.export.output_bytes == 0
                || sample.export.sha256.as_deref() != Some(digest.as_str())
            {
                return Err(
                    "verified sample report export is missing, malformed, or differs from its plan oracle"
                        .into(),
                );
            }
        }
        (SampleIntent::Diagnostics, Some(_)) => {
            return Err("diagnostics sample unexpectedly emitted a MachinePlan hash".into());
        }
        (SampleIntent::Verified, None) => {
            return Err("verified sample omitted its MachinePlan hash".into());
        }
    }
    let compiler_phase_ms = sample.phase.parse_ms
        + sample.phase.typecheck_ms
        + sample.phase.semantic_ms
        + sample.phase.contract_verify_ms
        + sample.phase.ir_lower_ms
        + sample.phase.ir_validation_ms
        + sample.phase.backend_ms;
    let rounding_tolerance_ms = 0.05_f64.max(sample.elapsed_ms * 0.01);
    if compiler_phase_ms > sample.elapsed_ms + rounding_tolerance_ms {
        return Err(format!(
            "compiler phase time {compiler_phase_ms:.3}ms exceeds total {:.3}ms",
            sample.elapsed_ms
        )
        .into());
    }
    Ok(())
}

fn absorb_batch(
    collected: &mut CollectedSamples,
    batch: SampleBatch,
    source: &str,
    intent: SampleIntent,
    scored: bool,
) -> ToolResult<()> {
    collected.cache_hit_count = collected
        .cache_hit_count
        .checked_add(batch.cache_hit_count)
        .ok_or("compiler sample cache-hit count overflow")?;
    for sample in batch.samples {
        absorb_sample(collected, &sample);
        if scored {
            collected.scored.push(sample);
        }
    }
    if batch.source != source || batch.intent != intent {
        return Err("compiler sample batch identity changed after validation".into());
    }
    Ok(())
}

fn absorb_evidence_batch(
    collected: &mut CollectedSamples,
    batch: SampleBatch,
    source: &str,
    intent: SampleIntent,
    scored: bool,
) -> ToolResult<()> {
    if batch.source != source || batch.intent != intent {
        return Err("compiler evidence batch identity changed after validation".into());
    }
    collected.evidence_parity_pass = true;
    if scored {
        collected.evidence_scored.extend(batch.samples);
    }
    Ok(())
}

fn absorb_sample(collected: &mut CollectedSamples, sample: &Sample) {
    collected
        .source_digests
        .insert(sample.source_bundle_digest_v1.clone());
    if let Some(diagnostics_fingerprint_v1) = &sample.diagnostics_fingerprint_v1 {
        collected
            .diagnostics_fingerprints
            .insert(diagnostics_fingerprint_v1.clone());
    }
    if let Some(plan_sha256) = &sample.plan_sha256 {
        collected.plan_hashes.insert(plan_sha256.clone());
    }
}

fn metric_report(
    intent: SampleIntent,
    setup_samples: usize,
    collected: CollectedSamples,
    budget: &FixtureBudget,
) -> MetricReport {
    let elapsed_ms = summarize_ms(
        collected
            .scored
            .iter()
            .map(|sample| sample.normative_elapsed_ms)
            .collect(),
    );
    let peak_rss_kib = summarize_rss(
        collected
            .scored
            .iter()
            .map(|sample| sample.peak_rss_kib)
            .collect(),
    );
    let compiler_cpu_ms = summarize_ms(
        collected
            .scored
            .iter()
            .map(|sample| sample.compiler_cpu_ms)
            .collect(),
    );
    let compiler_minor_page_faults = summarize_counts(
        collected
            .scored
            .iter()
            .map(|sample| sample.compiler_minor_page_faults)
            .collect(),
    );
    let compiler_major_page_faults = summarize_counts(
        collected
            .scored
            .iter()
            .map(|sample| sample.compiler_major_page_faults)
            .collect(),
    );
    let allocations = summarize_allocations(&collected.evidence_scored);
    let work = summarize_work(&collected.evidence_scored);
    let phase_ms = summarize_phases(&collected.scored);
    let export = summarize_exports(&collected.scored);
    let observed_source_bundle_digests = collected.source_digests.into_iter().collect::<Vec<_>>();
    let observed_diagnostics_fingerprint_v1 = collected
        .diagnostics_fingerprints
        .into_iter()
        .collect::<Vec<_>>();
    let observed_plan_sha256 = collected.plan_hashes.into_iter().collect::<Vec<_>>();
    let evaluation = evaluate_metric(
        intent,
        elapsed_ms,
        peak_rss_kib,
        collected.evidence_parity_pass,
        collected.cache_hit_count,
        &observed_source_bundle_digests,
        &observed_diagnostics_fingerprint_v1,
        &observed_plan_sha256,
        budget,
    );
    MetricReport {
        intent,
        setup_samples,
        scored_samples: collected.scored,
        evidence_scored_samples: collected.evidence_scored,
        evidence_parity_pass: collected.evidence_parity_pass,
        cache_hit_count: collected.cache_hit_count,
        observed_source_bundle_digests,
        observed_diagnostics_fingerprint_v1,
        observed_plan_sha256,
        elapsed_ms,
        compiler_cpu_ms,
        compiler_minor_page_faults,
        compiler_major_page_faults,
        peak_rss_kib,
        allocations,
        work,
        phase_ms,
        export,
        evaluation,
    }
}

fn evaluate_metric(
    intent: SampleIntent,
    elapsed_ms: MillisSummary,
    peak_rss_kib: RssSummary,
    evidence_parity_pass: bool,
    cache_hit_count: u64,
    source_digests: &[String],
    diagnostics_fingerprints: &[String],
    plan_hashes: &[String],
    budget: &FixtureBudget,
) -> MetricEvaluation {
    let p95_budget = match intent {
        SampleIntent::Diagnostics => budget.checked_diagnostics_p95_ms,
        SampleIntent::Verified => budget.verified_machine_plan_p95_ms,
    };
    let p95_budget_pass = elapsed_ms.p95 <= p95_budget;
    let peak_rss_budget_pass = peak_rss_kib.max <= budget.peak_rss_mib_max.saturating_mul(1024);
    let cache_disabled_pass = cache_hit_count == 0;
    let source_digest_pass = source_digests.len() == 1
        && source_digests
            .first()
            .is_some_and(|digest| validate_sha256(digest, "SourceBundleDigestV1").is_ok());
    let diagnostics_fingerprint_pass = match intent {
        SampleIntent::Diagnostics => {
            diagnostics_fingerprints.len() == 1
                && diagnostics_fingerprints.first().is_some_and(|digest| {
                    validate_sha256(digest, "diagnostics-fingerprint SHA-256").is_ok()
                })
        }
        SampleIntent::Verified => diagnostics_fingerprints.is_empty(),
    };
    let machine_plan_hash_pass = match intent {
        SampleIntent::Diagnostics => plan_hashes.is_empty(),
        SampleIntent::Verified => {
            plan_hashes.len() == 1
                && plan_hashes.first().map(String::as_str)
                    == Some(budget.machine_plan_sha256.as_str())
        }
    };
    let pass = evidence_parity_pass
        && p95_budget_pass
        && peak_rss_budget_pass
        && cache_disabled_pass
        && source_digest_pass
        && diagnostics_fingerprint_pass
        && machine_plan_hash_pass;
    MetricEvaluation {
        status: if pass {
            ReportStatus::Pass
        } else {
            ReportStatus::Fail
        },
        evidence_parity_pass,
        p95_budget_pass,
        peak_rss_budget_pass,
        cache_disabled_pass,
        source_digest_pass,
        diagnostics_fingerprint_pass,
        machine_plan_hash_pass,
    }
}

fn phase_acceptance_projection(fixtures: &[FixtureReport]) -> PhaseAcceptanceProjection {
    PhaseAcceptanceProjection {
        phase_1_cold_diagnostics: cold_phase_acceptance(fixtures, SampleIntent::Diagnostics),
        phase_3_verified_runnable: cold_phase_acceptance(fixtures, SampleIntent::Verified),
    }
}

fn cold_phase_acceptance(fixtures: &[FixtureReport], intent: SampleIntent) -> ColdPhaseAcceptance {
    let fixture_count = fixtures.len();
    let evaluated_mode_count = fixtures
        .iter()
        .map(|fixture| fixture.modes.len())
        .sum::<usize>();
    let fresh_process_pass = cold_mode_pass(fixtures, intent, ColdMode::FreshProcess);
    let empty_session_pass = cold_mode_pass(fixtures, intent, ColdMode::EmptySession);
    let p95_budget_pass = fixtures.iter().all(|fixture| {
        fixture
            .modes
            .iter()
            .all(|mode| metric_for_intent(mode, intent).evaluation.p95_budget_pass)
    });
    let peak_rss_budget_pass = fixtures.iter().all(|fixture| {
        fixture.modes.iter().all(|mode| {
            metric_for_intent(mode, intent)
                .evaluation
                .peak_rss_budget_pass
        })
    });
    let cache_disabled_pass = fixtures.iter().all(|fixture| {
        fixture.modes.iter().all(|mode| {
            metric_for_intent(mode, intent)
                .evaluation
                .cache_disabled_pass
        })
    });
    let within_mode_source_determinism_pass = fixtures.iter().all(|fixture| {
        fixture.modes.iter().all(|mode| {
            metric_for_intent(mode, intent)
                .evaluation
                .source_digest_pass
        })
    });
    let within_mode_result_determinism_pass = fixtures.iter().all(|fixture| {
        fixture.modes.iter().all(|mode| {
            let evaluation = &metric_for_intent(mode, intent).evaluation;
            match intent {
                SampleIntent::Diagnostics => evaluation.diagnostics_fingerprint_pass,
                SampleIntent::Verified => evaluation.machine_plan_hash_pass,
            }
        })
    });
    let cross_mode_source_parity_pass = fixtures
        .iter()
        .all(|fixture| cross_mode_source_parity(fixture, intent));
    let cross_mode_result_parity_pass = fixtures
        .iter()
        .all(|fixture| cross_mode_result_parity(fixture, intent));
    let complete_mode_shape =
        fixture_count > 0 && fixture_count.checked_mul(2) == Some(evaluated_mode_count);
    let pass = complete_mode_shape
        && fresh_process_pass
        && empty_session_pass
        && p95_budget_pass
        && peak_rss_budget_pass
        && cache_disabled_pass
        && within_mode_source_determinism_pass
        && within_mode_result_determinism_pass
        && cross_mode_source_parity_pass
        && cross_mode_result_parity_pass;
    ColdPhaseAcceptance {
        status: if pass {
            ReportStatus::Pass
        } else {
            ReportStatus::Fail
        },
        fixture_count,
        evaluated_mode_count,
        fresh_process_pass,
        empty_session_pass,
        p95_budget_pass,
        peak_rss_budget_pass,
        cache_disabled_pass,
        within_mode_source_determinism_pass,
        within_mode_result_determinism_pass,
        cross_mode_source_parity_pass,
        cross_mode_result_parity_pass,
    }
}

fn cold_mode_pass(fixtures: &[FixtureReport], intent: SampleIntent, mode: ColdMode) -> bool {
    fixtures.iter().all(|fixture| {
        fixture
            .modes
            .iter()
            .find(|candidate| candidate.mode == mode)
            .is_some_and(|candidate| {
                metric_for_intent(candidate, intent).evaluation.status == ReportStatus::Pass
            })
    })
}

fn cross_mode_source_parity(fixture: &FixtureReport, intent: SampleIntent) -> bool {
    fixture
        .modes
        .iter()
        .flat_map(|mode| {
            metric_for_intent(mode, intent)
                .observed_source_bundle_digests
                .iter()
        })
        .collect::<BTreeSet<_>>()
        .len()
        == 1
}

fn cross_mode_result_parity(fixture: &FixtureReport, intent: SampleIntent) -> bool {
    fixture
        .modes
        .iter()
        .flat_map(|mode| {
            let metric = metric_for_intent(mode, intent);
            match intent {
                SampleIntent::Diagnostics => metric.observed_diagnostics_fingerprint_v1.iter(),
                SampleIntent::Verified => metric.observed_plan_sha256.iter(),
            }
        })
        .collect::<BTreeSet<_>>()
        .len()
        == 1
}

fn metric_for_intent(mode: &ModeReport, intent: SampleIntent) -> &MetricReport {
    match intent {
        SampleIntent::Diagnostics => &mode.diagnostics,
        SampleIntent::Verified => &mode.verified,
    }
}

fn validate_existing(
    workspace: &Path,
    path: &Path,
    budget: &BudgetManifest,
    budget_digest: &str,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<CompilerPerformanceReport> {
    let bytes = read_bounded(path, MAX_REPORT_BYTES)?;
    let report: CompilerPerformanceReport = serde_json::from_slice(&bytes)?;
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
    report: &CompilerPerformanceReport,
    budget: &BudgetManifest,
    budget_digest: &str,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<()> {
    if report.format_version != REPORT_FORMAT_VERSION || report.contract != REPORT_CONTRACT {
        return Err("compiler performance report contract or format is unsupported".into());
    }
    let expected_identity = current_identity(workspace)?;
    if report.identity != expected_identity {
        return Err("compiler performance report identity is stale for this workspace/tool".into());
    }
    let expected_budget = BudgetIdentity {
        path: DEFAULT_BUDGET.to_owned(),
        sha256: budget_digest.to_owned(),
        format_version: budget.format_version,
        owner_plan: budget.owner_plan.clone(),
    };
    if report.budget != expected_budget {
        return Err("compiler performance report budget identity is stale".into());
    }
    validate_report_producer(
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
    validate_report_producer(
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
    let expected_protocol = protocol_evidence(budget, setup_samples, scored_samples);
    if report.protocol != expected_protocol
        || report.run_classification != classification(budget, setup_samples, scored_samples)
    {
        return Err(
            "compiler performance report sampling protocol differs from the request".into(),
        );
    }
    if report.fixtures.len() != budget.fixtures.len() {
        return Err(format!(
            "compiler performance report has {} fixtures; expected {}",
            report.fixtures.len(),
            budget.fixtures.len()
        )
        .into());
    }

    for (fixture, fixture_budget) in report.fixtures.iter().zip(&budget.fixtures) {
        validate_fixture_report(fixture, fixture_budget, setup_samples, scored_samples)?;
    }
    let expected_phase_acceptance = phase_acceptance_projection(&report.fixtures);
    if report.phase_acceptance != expected_phase_acceptance {
        return Err(
            "compiler performance phase-acceptance projection is inconsistent with fixture evidence"
                .into(),
        );
    }
    let expected_status = if report
        .fixtures
        .iter()
        .all(|fixture| fixture.status == ReportStatus::Pass)
    {
        ReportStatus::Pass
    } else {
        ReportStatus::Fail
    };
    if report.status != expected_status {
        return Err("compiler performance report aggregate status is inconsistent".into());
    }
    if report.status == ReportStatus::Pass
        && (report.phase_acceptance.phase_1_cold_diagnostics.status != ReportStatus::Pass
            || report.phase_acceptance.phase_3_verified_runnable.status != ReportStatus::Pass)
    {
        return Err(
            "compiler performance aggregate passed without both cold phase projections".into(),
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_report_producer(
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
            "compiler report producer path is {}; expected {expected_path}",
            producer.path
        )
        .into());
    }
    let path = workspace.join(safe_relative_path(expected_path, "producer path")?);
    require_current_prebuilt_producer(workspace, &path)?;
    let digest = sha256_file(&path)?;
    if producer.sha256 != digest.as_str() {
        return Err(
            format!("compiler performance producer {expected_path} identity is stale").into(),
        );
    }
    validate_producer_metadata(
        workspace,
        identity,
        &producer.metadata,
        ExpectedProducer {
            path: expected_path,
            sha256: digest.as_str(),
            product_kind: expected_kind,
            allocator_id: expected_allocator,
            allocation_instrumentation: expected_instrumentation,
            allocation_counter_scope: expected_counter_scope,
            cargo_profile: &protocol.build_profile,
            target_cpu: &protocol.target_cpu,
            profile_options: &protocol.profile_options,
            rustflags: &protocol.rustflags,
        },
    )
}

fn validate_fixture_report(
    report: &FixtureReport,
    budget: &FixtureBudget,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<()> {
    if report.id != budget.id
        || report.source != budget.source
        || report.package_source_lines != budget.package_source_lines
        || report.compiler_input_source_lines != budget.compiler_input_source_lines
        || report.checked_diagnostics_p95_budget_ms != budget.checked_diagnostics_p95_ms
        || report.verified_machine_plan_p95_budget_ms != budget.verified_machine_plan_p95_ms
        || report.peak_rss_mib_max != budget.peak_rss_mib_max
        || report.expected_machine_plan_sha256 != budget.machine_plan_sha256
    {
        return Err(format!(
            "compiler performance fixture {} differs from the budget manifest",
            budget.id
        )
        .into());
    }
    if report.modes.len() != 2
        || report.modes[0].mode != ColdMode::FreshProcess
        || report.modes[1].mode != ColdMode::EmptySession
    {
        return Err(format!(
            "compiler performance fixture {} must contain fresh-process then empty-session evidence",
            budget.id
        )
        .into());
    }
    for mode in &report.modes {
        validate_metric_report(
            &mode.diagnostics,
            SampleIntent::Diagnostics,
            budget,
            setup_samples,
            scored_samples,
        )?;
        validate_metric_report(
            &mode.verified,
            SampleIntent::Verified,
            budget,
            setup_samples,
            scored_samples,
        )?;
        let expected = if mode.diagnostics.evaluation.status == ReportStatus::Pass
            && mode.verified.evaluation.status == ReportStatus::Pass
        {
            ReportStatus::Pass
        } else {
            ReportStatus::Fail
        };
        if mode.status != expected {
            return Err(format!(
                "compiler performance fixture {} mode {} status is inconsistent",
                budget.id,
                mode.mode.as_str()
            )
            .into());
        }
    }
    let observed = report
        .modes
        .iter()
        .flat_map(|mode| {
            mode.diagnostics
                .observed_source_bundle_digests
                .iter()
                .chain(mode.verified.observed_source_bundle_digests.iter())
        })
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if report.observed_source_bundle_digests != observed {
        return Err(format!(
            "compiler performance fixture {} source-digest summary is inconsistent",
            budget.id
        )
        .into());
    }
    let observed_diagnostics_fingerprints = report
        .modes
        .iter()
        .flat_map(|mode| mode.diagnostics.observed_diagnostics_fingerprint_v1.iter())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if report.observed_diagnostics_fingerprint_v1 != observed_diagnostics_fingerprints {
        return Err(format!(
            "compiler performance fixture {} diagnostics-fingerprint summary is inconsistent",
            budget.id
        )
        .into());
    }
    let expected_status = if report
        .modes
        .iter()
        .all(|mode| mode.status == ReportStatus::Pass)
        && observed.len() == 1
        && observed_diagnostics_fingerprints.len() == 1
    {
        ReportStatus::Pass
    } else {
        ReportStatus::Fail
    };
    if report.status != expected_status {
        return Err(format!(
            "compiler performance fixture {} status is inconsistent",
            budget.id
        )
        .into());
    }
    Ok(())
}

fn validate_metric_report(
    report: &MetricReport,
    intent: SampleIntent,
    budget: &FixtureBudget,
    setup_samples: usize,
    scored_samples: usize,
) -> ToolResult<()> {
    if report.intent != intent
        || report.setup_samples != setup_samples
        || report.scored_samples.len() != scored_samples
        || report.evidence_scored_samples.len() != scored_samples
        || !report.evidence_parity_pass
    {
        return Err(format!(
            "{} metric sampling shape differs from the requested protocol",
            intent.as_str()
        )
        .into());
    }
    for sample in &report.scored_samples {
        validate_sample_shape(sample, intent, false)?;
    }
    for sample in &report.evidence_scored_samples {
        validate_sample_shape(sample, intent, true)?;
    }
    for (product, evidence) in report
        .scored_samples
        .iter()
        .zip(&report.evidence_scored_samples)
    {
        validate_product_evidence_sample_parity(product, evidence)?;
    }
    if report
        .scored_samples
        .windows(2)
        .any(|pair| pair[0].compiler_artifact_ready_unix_us > pair[1].observation_started_unix_us)
    {
        return Err(format!(
            "{} metric contains overlapping compiler observations",
            intent.as_str()
        )
        .into());
    }
    let elapsed = summarize_ms(
        report
            .scored_samples
            .iter()
            .map(|sample| sample.normative_elapsed_ms)
            .collect(),
    );
    let rss = summarize_rss(
        report
            .scored_samples
            .iter()
            .map(|sample| sample.peak_rss_kib)
            .collect(),
    );
    let cpu = summarize_ms(
        report
            .scored_samples
            .iter()
            .map(|sample| sample.compiler_cpu_ms)
            .collect(),
    );
    let minor_faults = summarize_counts(
        report
            .scored_samples
            .iter()
            .map(|sample| sample.compiler_minor_page_faults)
            .collect(),
    );
    let major_faults = summarize_counts(
        report
            .scored_samples
            .iter()
            .map(|sample| sample.compiler_major_page_faults)
            .collect(),
    );
    let allocations = summarize_allocations(&report.evidence_scored_samples);
    let work = summarize_work(&report.evidence_scored_samples);
    let phases = summarize_phases(&report.scored_samples);
    let export = summarize_exports(&report.scored_samples);
    if report.elapsed_ms != elapsed
        || report.compiler_cpu_ms != cpu
        || report.compiler_minor_page_faults != minor_faults
        || report.compiler_major_page_faults != major_faults
        || report.peak_rss_kib != rss
        || report.allocations != allocations
        || report.work != work
        || report.phase_ms != phases
        || report.export != export
    {
        return Err(format!(
            "{} metric nearest-rank summaries do not match its scored samples",
            intent.as_str()
        )
        .into());
    }
    if !strict_sorted_unique_sha256(&report.observed_source_bundle_digests)
        || !strict_sorted_unique_sha256(&report.observed_diagnostics_fingerprint_v1)
        || !strict_sorted_unique_sha256(&report.observed_plan_sha256)
    {
        return Err(format!(
            "{} metric digest observations are invalid or non-canonical",
            intent.as_str()
        )
        .into());
    }
    let expected_evaluation = evaluate_metric(
        intent,
        elapsed,
        rss,
        report.evidence_parity_pass,
        report.cache_hit_count,
        &report.observed_source_bundle_digests,
        &report.observed_diagnostics_fingerprint_v1,
        &report.observed_plan_sha256,
        budget,
    );
    if report.evaluation != expected_evaluation {
        return Err(format!(
            "{} metric evaluation does not match its evidence and budgets",
            intent.as_str()
        )
        .into());
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
        p99: nearest_rank(&values, 99),
        max: *values.last().expect("non-empty values"),
    }
}

fn summarize_rss(mut values: Vec<u64>) -> RssSummary {
    if values.is_empty() {
        return RssSummary::default();
    }
    values.sort_unstable();
    RssSummary {
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

fn summarize_allocations(samples: &[Sample]) -> AllocationSummary {
    let values = |field: fn(&AllocationSample) -> u64| {
        summarize_counts(
            samples
                .iter()
                .map(|sample| field(&sample.allocations))
                .collect(),
        )
    };
    AllocationSummary {
        allocation_calls: values(|allocation| allocation.allocation_calls),
        allocated_bytes: values(|allocation| allocation.allocated_bytes),
        deallocation_calls: values(|allocation| allocation.deallocation_calls),
        deallocated_bytes: values(|allocation| allocation.deallocated_bytes),
    }
}

fn summarize_work(samples: &[Sample]) -> WorkSummary {
    let values = |field: fn(&WorkSample) -> usize| {
        summarize_counts(
            samples
                .iter()
                .map(|sample| {
                    u64::try_from(field(&sample.work)).expect("work count exceeds report u64")
                })
                .collect(),
        )
    };
    let u64_values = |field: fn(&WorkSample) -> u64| {
        summarize_counts(samples.iter().map(|sample| field(&sample.work)).collect())
    };
    WorkSummary {
        source_units: values(|work| work.source_units),
        parsed_expressions: values(|work| work.parsed_expressions),
        checked_expressions: values(|work| work.checked_expressions),
        checked_calls: values(|work| work.checked_calls),
        semantic_graph_nodes: values(|work| work.semantic_graph_nodes),
        cancellation_checkpoints: values(|work| work.cancellation_checkpoints),
        parse_source_units_attempted: values(|work| work.parse.source_units_attempted),
        parse_source_units_parsed: values(|work| work.parse.source_units_parsed),
        parse_source_units_reused: values(|work| work.parse.source_units_reused),
        parse_expression_visits: values(|work| work.parse.expression_visits),
        parse_validation_visits: values(|work| work.parse.validation_visits),
        typecheck_inference_expression_visits: u64_values(|work| {
            work.typecheck.inference_expression_visits
        }),
        typecheck_inference_call_visits: u64_values(|work| work.typecheck.inference_call_visits),
        typecheck_diagnostic_replay_requests: u64_values(|work| {
            work.typecheck.diagnostic_replay_requests
        }),
        typecheck_diagnostic_replay_misses: u64_values(|work| {
            work.typecheck.diagnostic_replay_misses
        }),
        kernel_definition_modules: u64_values(|work| work.kernel_compile.definition_modules),
        kernel_linked_operations: u64_values(|work| work.kernel_compile.linked_operations),
        kernel_scheduled_work_items: u64_values(|work| work.kernel_compile.scheduled_work_items),
        kernel_variables: u64_values(|work| work.kernel_solve.variables),
        kernel_operations: u64_values(|work| work.kernel_solve.operations),
        kernel_activations: u64_values(|work| work.kernel_solve.activations),
        kernel_rich_output_flow_exports: u64_values(|work| {
            work.kernel_solve.rich_output_flow_exports
        }),
        kernel_term_intern_requests: u64_values(|work| work.kernel_solve.term_intern_requests),
        kernel_term_intern_hits: u64_values(|work| work.kernel_solve.term_intern_hits),
        kernel_dynamic_dependency_edges: u64_values(|work| {
            work.kernel_solve.dynamic_dependency_edges
        }),
        kernel_frozen_type_store_term_rows: u64_values(|work| {
            work.kernel_solve.frozen_type_store.term_rows
        }),
        kernel_frozen_type_store_name_rows: u64_values(|work| {
            work.kernel_solve.frozen_type_store.name_rows
        }),
        kernel_frozen_type_store_child_rows: u64_values(|work| {
            work.kernel_solve.frozen_type_store.child_rows
        }),
        kernel_frozen_type_store_variant_rows: u64_values(|work| {
            work.kernel_solve.frozen_type_store.variant_rows
        }),
        kernel_frozen_type_store_object_shape_rows: u64_values(|work| {
            work.kernel_solve.frozen_type_store.object_shape_rows
        }),
        kernel_frozen_type_store_object_field_rows: u64_values(|work| {
            work.kernel_solve.frozen_type_store.object_field_rows
        }),
        kernel_frozen_type_store_semantic_order_rows: u64_values(|work| {
            work.kernel_solve.frozen_type_store.semantic_order_rows
        }),
        kernel_frozen_type_store_payload_len_bytes: u64_values(|work| {
            work.kernel_solve.frozen_type_store.payload_len_bytes
        }),
        kernel_frozen_type_store_payload_capacity_bytes: u64_values(|work| {
            work.kernel_solve.frozen_type_store.payload_capacity_bytes
        }),
    }
}

fn summarize_phases(samples: &[Sample]) -> PhaseSummary {
    let values = |field: fn(&PhaseSample) -> f64| {
        summarize_ms(samples.iter().map(|sample| field(&sample.phase)).collect())
    };
    PhaseSummary {
        parse: values(|phase| phase.parse_ms),
        typecheck: values(|phase| phase.typecheck_ms),
        semantic: values(|phase| phase.semantic_ms),
        contract_verify: values(|phase| phase.contract_verify_ms),
        ir_lower: values(|phase| phase.ir_lower_ms),
        ir_validation: values(|phase| phase.ir_validation_ms),
        backend: values(|phase| phase.backend_ms),
        plan_validation: values(|phase| phase.plan_validation_ms),
    }
}

fn summarize_exports(samples: &[Sample]) -> ExportSummary {
    ExportSummary {
        kinds: samples
            .iter()
            .map(|sample| sample.export.kind.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        elapsed_ms: summarize_ms(
            samples
                .iter()
                .map(|sample| sample.export.elapsed_ms)
                .collect(),
        ),
        output_bytes: summarize_counts(
            samples
                .iter()
                .map(|sample| sample.export.output_bytes)
                .collect(),
        ),
    }
}

fn nearest_rank<T: Copy>(sorted: &[T], percentile: usize) -> T {
    debug_assert!(!sorted.is_empty());
    debug_assert!((1..=100).contains(&percentile));
    let rank = sorted.len().saturating_mul(percentile).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn protocol_evidence(
    budget: &BudgetManifest,
    setup_samples: usize,
    scored_samples: usize,
) -> ProtocolEvidence {
    ProtocolEvidence {
        build_profile: budget.protocol.build_profile.clone(),
        target_profile: budget.protocol.target_profile.clone(),
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
        cold_modes: budget.protocol.cold_modes.clone(),
        os_page_cache: budget.protocol.os_page_cache.clone(),
        sample_process_isolation: budget.protocol.sample_process_isolation.clone(),
        peak_rss_unit: budget.protocol.peak_rss_unit.clone(),
        peak_rss_scope: budget.protocol.peak_rss_scope.clone(),
        percentile_method: "nearest-rank".to_owned(),
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

fn load_budget(workspace: &Path) -> ToolResult<(BudgetManifest, String)> {
    let path = workspace.join(DEFAULT_BUDGET);
    let bytes = read_bounded(&path, MAX_BUDGET_BYTES)?;
    let budget_text = std::str::from_utf8(&bytes)
        .map_err(|error| format!("{} is not UTF-8: {error}", path.display()))?;
    let budget: BudgetManifest =
        toml::from_str(budget_text).map_err(|error| format!("{}: {error}", path.display()))?;
    validate_budget(workspace, &budget)?;
    Ok((budget, sha256_bytes(&bytes).as_str().to_owned()))
}

fn validate_budget(workspace: &Path, budget: &BudgetManifest) -> ToolResult<()> {
    if budget.format_version != BUDGET_FORMAT_VERSION {
        return Err(format!(
            "compiler budget format is {}; expected {BUDGET_FORMAT_VERSION}",
            budget.format_version
        )
        .into());
    }
    let owner = safe_relative_path(&budget.owner_plan, "budget owner plan")?;
    if !workspace.join(owner).is_file() {
        return Err("compiler budget owner plan does not exist".into());
    }
    safe_relative_path(&budget.report, "budget report path")?;
    if budget.protocol.build_profile != "release"
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
        || budget.protocol.cold_modes.len() != 2
        || budget.protocol.cold_modes[0] != "fresh-process"
        || budget.protocol.cold_modes[1] != "empty-session"
        || budget.protocol.os_page_cache != "natural"
        || budget.protocol.sample_process_isolation != "one-process-per-observation"
        || budget.protocol.peak_rss_unit != "KiB"
        || budget.protocol.peak_rss_scope != "process-high-water-through-compiler-artifact"
    {
        return Err("compiler budget protocol differs from the cold acceptance contract".into());
    }
    safe_relative_path(&budget.protocol.product_producer, "product producer path")?;
    safe_relative_path(&budget.protocol.evidence_producer, "evidence producer path")?;
    validate_effective_samples(
        budget.protocol.setup_samples,
        budget.protocol.scored_samples,
    )?;
    if budget.fixtures.is_empty() {
        return Err("compiler budget must define at least one fixture".into());
    }
    let example_manifest = load_example_line_manifest(workspace)?;
    let mut ids = BTreeSet::new();
    for fixture in &budget.fixtures {
        if fixture.id.is_empty() || !ids.insert(fixture.id.as_str()) {
            return Err(
                format!("invalid or duplicate compiler fixture id `{}`", fixture.id).into(),
            );
        }
        let source = safe_relative_path(&fixture.source, "fixture source")?;
        if !workspace.join(source).is_file() {
            return Err(format!("compiler fixture {} source is missing", fixture.id).into());
        }
        if fixture.package_source_lines == 0
            || fixture.compiler_input_source_lines == 0
            || fixture.compiler_input_source_lines > fixture.package_source_lines
            || !finite_positive(fixture.checked_diagnostics_p95_ms)
            || !finite_positive(fixture.verified_machine_plan_p95_ms)
            || fixture.peak_rss_mib_max == 0
        {
            return Err(format!("compiler fixture {} has invalid budgets", fixture.id).into());
        }
        validate_sha256(&fixture.machine_plan_sha256, "fixture MachinePlan SHA-256")?;
        validate_fixture_line_counts(workspace, &example_manifest, fixture)?;
    }
    validate_warm_budget(workspace, &budget.warm)?;
    validate_scaling_budget(&budget.scaling)?;
    Ok(())
}

fn load_example_line_manifest(workspace: &Path) -> ToolResult<ExampleLineManifest> {
    let path = workspace.join("examples/manifest.toml");
    let bytes = read_bounded(&path, MAX_BUDGET_BYTES.saturating_mul(8))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| format!("{} is not UTF-8: {error}", path.display()))?;
    toml::from_str(text).map_err(|error| format!("{}: {error}", path.display()).into())
}

fn validate_fixture_line_counts(
    workspace: &Path,
    manifest: &ExampleLineManifest,
    fixture: &FixtureBudget,
) -> ToolResult<()> {
    let matching = manifest
        .example
        .iter()
        .filter(|entry| entry.source == fixture.source)
        .collect::<Vec<_>>();
    let [entry] = matching.as_slice() else {
        return Err(format!(
            "compiler fixture {} source `{}` resolves to {} example-manifest entries; expected one",
            fixture.id,
            fixture.source,
            matching.len()
        )
        .into());
    };

    let source = safe_relative_path(&entry.source, "example source")?;
    let mut compiler_files = entry
        .source_files
        .iter()
        .map(|value| safe_relative_path(value, "example source file"))
        .collect::<ToolResult<Vec<_>>>()?;
    compiler_files.retain(|path| path != &source);
    compiler_files.push(source);
    let compiler_files = compiler_files.into_iter().collect::<BTreeSet<_>>();

    let mut package_files = compiler_files.clone();
    for build_file in &entry.build_files {
        package_files.insert(safe_relative_path(build_file, "example build file")?);
    }
    let compiler_input_source_lines = count_source_lines(workspace, &compiler_files)?;
    let package_source_lines = count_source_lines(workspace, &package_files)?;
    if compiler_input_source_lines != fixture.compiler_input_source_lines
        || package_source_lines != fixture.package_source_lines
    {
        return Err(format!(
            "compiler fixture {} line counts are stale: budget package/input={}/{}, observed={}/{}",
            fixture.id,
            fixture.package_source_lines,
            fixture.compiler_input_source_lines,
            package_source_lines,
            compiler_input_source_lines,
        )
        .into());
    }
    Ok(())
}

fn count_source_lines(workspace: &Path, files: &BTreeSet<PathBuf>) -> ToolResult<u64> {
    let mut total = 0_u64;
    for relative in files {
        let path = workspace.join(relative);
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("read compiler source {}: {error}", path.display()))?;
        let lines = u64::try_from(source.lines().count())
            .map_err(|_| format!("source line count overflow for {}", path.display()))?;
        total = total
            .checked_add(lines)
            .ok_or("compiler fixture source-line count overflow")?;
    }
    Ok(total)
}

fn validate_warm_budget(workspace: &Path, warm: &WarmBudget) -> ToolResult<()> {
    safe_relative_path(&warm.report, "warm report path")?;
    let source = safe_relative_path(&warm.source, "warm source")?;
    let switch_source = safe_relative_path(&warm.switch_source, "warm switch source")?;
    let edit_unit = safe_relative_path(&warm.edit_unit, "warm edit unit")?;
    if source == switch_source
        || !workspace.join(&source).is_file()
        || !workspace.join(&switch_source).is_file()
        || !workspace.join(&edit_unit).is_file()
        || warm.edit_from.is_empty()
        || warm.edit_from == warm.edit_to
    {
        return Err("compiler warm workload paths or edit replacement are invalid".into());
    }
    let edit_source = fs::read_to_string(workspace.join(&edit_unit))?;
    if edit_source.matches(&warm.edit_from).count() != 1 {
        return Err(format!(
            "compiler warm edit marker must occur exactly once in `{}`",
            warm.edit_unit
        )
        .into());
    }
    let values = [
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
    if values.into_iter().any(|value| !finite_positive(value))
        || warm.checked_diagnostics_p95_ms > warm.checked_diagnostics_p99_ms
        || warm.checked_diagnostics_p99_ms > warm.checked_diagnostics_max_ms
        || warm.verified_preview_p95_ms > warm.verified_preview_max_ms
        || warm.switch_present_p95_ms > warm.switch_present_max_ms
    {
        return Err("compiler warm budget is invalid".into());
    }
    Ok(())
}

fn validate_scaling_budget(scaling: &ScalingBudget) -> ToolResult<()> {
    if !finite_positive(scaling.maximum_doubling_ratio)
        || scaling.maximum_doubling_ratio > 2.2
        || scaling.dimensions.is_empty()
        || scaling.workloads.is_empty()
    {
        return Err("compiler scaling budget is invalid".into());
    }
    let mut dimensions = BTreeSet::new();
    for dimension in &scaling.dimensions {
        if dimension.is_empty() || !dimensions.insert(dimension.as_str()) {
            return Err(format!("invalid or duplicate scaling dimension `{dimension}`").into());
        }
    }
    let required_dimensions = BTreeSet::from([
        "call-depth",
        "call-site-count",
        "contextual-call-site-count",
        "static-branch-count",
        "source-unit-count",
        "dependency-cone-size",
    ]);
    if dimensions != required_dimensions {
        return Err(
            "compiler scaling budget must define exactly the six planned dimensions".into(),
        );
    }
    let mut workloads = BTreeSet::new();
    for workload in &scaling.workloads {
        if !workloads.insert(workload.id.as_str())
            || !dimensions.contains(workload.id.as_str())
            || workload.base_size == 0
            || workload.baseline_size >= workload.base_size
            || workload.base_size.checked_mul(2) != Some(workload.doubled_size)
        {
            return Err(format!("invalid scaling workload `{}`", workload.id).into());
        }
        let expected = match workload.id.as_str() {
            "call-depth" | "call-site-count" => ("diagnostics", "checked-calls"),
            "contextual-call-site-count" => ("verified", "semantic-graph-nodes"),
            "static-branch-count" => ("verified", "checked-expressions"),
            "source-unit-count" => ("diagnostics", "parse-source-units-attempted"),
            "dependency-cone-size" => ("verified", "dependency-scc-components"),
            other => return Err(format!("unsupported scaling workload `{other}`").into()),
        };
        if workload.intent != expected.0 || workload.owning_work_counter != expected.1 {
            return Err(format!(
                "scaling workload `{}` has intent/counter `{}`/`{}`; expected `{}`/`{}`",
                workload.id, workload.intent, workload.owning_work_counter, expected.0, expected.1
            )
            .into());
        }
    }
    if workloads != required_dimensions {
        return Err(
            "compiler scaling workloads must cover every planned dimension exactly once".into(),
        );
    }
    Ok(())
}

fn validate_effective_samples(setup_samples: usize, scored_samples: usize) -> ToolResult<()> {
    if scored_samples == 0
        || setup_samples
            .checked_add(scored_samples)
            .is_none_or(|count| count > MAX_SAMPLE_COUNT)
    {
        return Err(format!(
            "compiler performance requires scored samples and setup + scored <= {MAX_SAMPLE_COUNT}"
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

fn strict_sorted_unique_sha256(values: &[String]) -> bool {
    values
        .iter()
        .all(|value| validate_sha256(value, "digest").is_ok())
        && values.windows(2).all(|pair| pair[0] < pair[1])
}

fn finite_positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn finite_nonnegative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

fn duration_ms(duration: std::time::Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn unix_time_us() -> ToolResult<u64> {
    Ok(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros(),
    )?)
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

fn write_report(path: &Path, report: &CompilerPerformanceReport) -> ToolResult<()> {
    let mut bytes = serde_json::to_vec_pretty(report)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err(format!(
            "compiler performance report is {} bytes; limit is {MAX_REPORT_BYTES}",
            bytes.len()
        )
        .into());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("compiler-tmp-{}", std::process::id()));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn bounded_lossy(bytes: &[u8], limit: usize) -> String {
    let end = bytes.len().min(limit);
    let mut value = String::from_utf8_lossy(&bytes[..end]).trim().to_owned();
    if bytes.len() > limit {
        value.push_str(" ...<truncated>");
    }
    value
}
