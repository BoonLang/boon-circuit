use boon_compiler::{
    CancellationToken, CheckedCompileRequest, CompileIntent, CompilerCheckRequest, CompilerProject,
    CompilerSession, UnitUpdate, check_runtime_source, compiler_source_project_for_path,
    finish_checked_sealed_machine_plan,
};
use boon_plan::{ApplicationIdentity, ProgramRole, TargetProfile};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{self, Write};
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::{
    CompilerAllocationCounters, compiler_allocation_counters, reset_compiler_allocation_counters,
};

const FORMAT_VERSION: u16 = 5;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum SampleIntent {
    Diagnostics,
    Verified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SampleMode {
    FreshProcess,
    EmptySession,
}

impl SampleMode {
    const fn compiler_state(self) -> &'static str {
        match self {
            Self::FreshProcess => "fresh-process",
            Self::EmptySession => "empty-session",
        }
    }
}

#[derive(Debug, Serialize)]
struct SampleBatch {
    format_version: u16,
    source: String,
    intent: SampleIntent,
    compiler_state: &'static str,
    target_profile: &'static str,
    program_role: &'static str,
    compiler_threads: usize,
    compiler_caches: &'static str,
    peak_rss_unit: &'static str,
    peak_rss_scope: &'static str,
    cache_hit_count: u64,
    samples: Vec<Sample>,
}

#[derive(Debug, Serialize)]
struct WarmSessionBatch {
    format_version: u16,
    workload: &'static str,
    source: String,
    switch_source: String,
    edit_unit: String,
    producer_pid: u32,
    compiler_threads: usize,
    compiler_caches: &'static str,
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

#[derive(Debug, Serialize)]
struct WarmEditSample {
    sequence: usize,
    scored: bool,
    direction: &'static str,
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

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
struct CancellationEvidence {
    scope: &'static str,
    revision: u64,
    token_canceled_before_request: bool,
    request_rejected: bool,
    stop_latency_ms: f64,
    last_good_revision_before: u64,
    last_good_revision_after: u64,
    publication_unchanged: bool,
    in_flight_supersession_supported: bool,
}

#[derive(Debug, Serialize)]
struct LatestGenerationEvidence {
    stale_revision: u64,
    latest_revision: u64,
    stale_request_rejected: bool,
    last_good_revision_after_stale_request: u64,
    published_revision: u64,
    publish_latest_ms: f64,
    no_stale_publication: bool,
}

#[derive(Debug, Serialize)]
struct SyntheticScalingBatch {
    format_version: u16,
    workload: &'static str,
    generator: &'static str,
    dimension: String,
    size: usize,
    intent: SampleIntent,
    producer_pid: u32,
    compiler_threads: usize,
    compiler_caches: &'static str,
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

#[derive(Debug, Serialize)]
struct Sample {
    producer_pid: u32,
    observation_started_unix_us: u64,
    compiler_artifact_ready_unix_us: u64,
    elapsed_ms: f64,
    peak_rss_kib: u64,
    source_bundle_digest_v1: String,
    diagnostics_fingerprint_v1: Option<String>,
    diagnostic_count: usize,
    full_document_typecheck_coverage: Option<bool>,
    plan_sha256: Option<String>,
    allocations: AllocationSample,
    work: WorkSample,
    phase: PhaseSample,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct AllocationSample {
    allocation_calls: u64,
    allocated_bytes: u64,
    deallocation_calls: u64,
    deallocated_bytes: u64,
}

impl From<CompilerAllocationCounters> for AllocationSample {
    fn from(value: CompilerAllocationCounters) -> Self {
        Self {
            allocation_calls: value.allocation_calls,
            allocated_bytes: value.allocated_bytes,
            deallocation_calls: value.deallocation_calls,
            deallocated_bytes: value.deallocated_bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct ParserWorkSample {
    source_units_attempted: usize,
    source_units_parsed: usize,
    source_units_reused: usize,
    source_bytes_inspected: usize,
    token_inspections: usize,
    symbol_inspections: usize,
    statement_visits: usize,
    expression_visits: usize,
    nodes_rebased: usize,
    validation_visits: usize,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct TypeCheckWorkSample {
    inference_invocations: u64,
    inference_rounds: u64,
    inference_expression_visits: u64,
    inference_declaration_visits: u64,
    inference_callable_visits: u64,
    inference_call_visits: u64,
    inference_call_changed_visits: u64,
    inference_call_noop_visits: u64,
    inference_call_seed_enqueues: u64,
    inference_call_input_enqueues: u64,
    inference_call_output_enqueues: u64,
    inference_call_callee_enqueues: u64,
    inference_call_selector_enqueues: u64,
    inference_call_output_scope_enqueues: u64,
    inference_call_output_origin_skips: u64,
    inference_selector_visits: u64,
    inference_pattern_visits: u64,
    context_scheme_worklist_invocations: u64,
    context_scheme_worklist_visits: u64,
    context_scheme_worklist_changes: u64,
    wrapper_scheme_worklist_invocations: u64,
    wrapper_scheme_worklist_visits: u64,
    wrapper_scheme_changed_owners: u64,
    wrapper_scheme_parameter_changes: u64,
    wrapper_scheme_result_changes: u64,
    checked_flow_cache_hits: u64,
    checked_flow_cache_misses: u64,
    checked_flow_cache_invalidations: u64,
    checked_flow_cache_reverse_invalidation_traversals: u64,
    checked_flow_cache_full_resets: u64,
    checked_flow_cache_rejected_invalid_ids: u64,
    checked_flow_indexed_read_hits: u64,
    checked_flow_indexed_read_missing: u64,
    checked_flow_indexed_read_rejected: u64,
    checked_flow_indexed_out_hits: u64,
    checked_flow_indexed_out_missing: u64,
    diagnostic_flow_install_attempts: u64,
    diagnostic_flow_duplicate_ids: u64,
    diagnostic_flow_out_of_range_ids: u64,
    diagnostic_flow_missing_parser_ids: u64,
    diagnostic_replay_requests: u64,
    diagnostic_replay_hits: u64,
    diagnostic_replay_misses: u64,
    diagnostic_replay_unique_expressions: u64,
    owner_statements: u64,
    owner_expressions: u64,
    owner_local_constraints: u64,
    owner_interface_imports: u64,
    owner_interface_plan_direct_owners: u64,
    owner_interface_plan_required_owners: u64,
    owner_interface_plan_provider_sccs: u64,
    owner_interface_plan_result_transfers: u64,
    owner_interface_plan_transfer_nodes: u64,
    owner_interface_plan_transfer_edges: u64,
    owner_calls: u64,
    owner_unification_steps: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
struct WorkSample {
    source_units: usize,
    parsed_expressions: usize,
    checked_expressions: usize,
    checked_calls: usize,
    semantic_graph_nodes: usize,
    cancellation_checkpoints: usize,
    parse: ParserWorkSample,
    typecheck: TypeCheckWorkSample,
}

macro_rules! parser_work_sample {
    ($work:expr) => {{
        let work = $work;
        ParserWorkSample {
            source_units_attempted: work.source_units_attempted,
            source_units_parsed: work.source_units_parsed,
            source_units_reused: work.source_units_reused,
            source_bytes_inspected: work.source_bytes_inspected,
            token_inspections: work.token_inspections,
            symbol_inspections: work.symbol_inspections,
            statement_visits: work.statement_visits,
            expression_visits: work.expression_visits,
            nodes_rebased: work.nodes_rebased,
            validation_visits: work.validation_visits,
        }
    }};
}

macro_rules! typecheck_work_sample {
    ($work:expr) => {{
        let work = $work;
        TypeCheckWorkSample {
            inference_invocations: work.inference_invocations,
            inference_rounds: work.inference_rounds,
            inference_expression_visits: work.inference_expression_visits,
            inference_declaration_visits: work.inference_declaration_visits,
            inference_callable_visits: work.inference_callable_visits,
            inference_call_visits: work.inference_call_visits,
            inference_call_changed_visits: work.inference_call_changed_visits,
            inference_call_noop_visits: work.inference_call_noop_visits,
            inference_call_seed_enqueues: work.inference_call_seed_enqueues,
            inference_call_input_enqueues: work.inference_call_input_enqueues,
            inference_call_output_enqueues: work.inference_call_output_enqueues,
            inference_call_callee_enqueues: work.inference_call_callee_enqueues,
            inference_call_selector_enqueues: work.inference_call_selector_enqueues,
            inference_call_output_scope_enqueues: work.inference_call_output_scope_enqueues,
            inference_call_output_origin_skips: work.inference_call_output_origin_skips,
            inference_selector_visits: work.inference_selector_visits,
            inference_pattern_visits: work.inference_pattern_visits,
            context_scheme_worklist_invocations: work.context_scheme_worklist_invocations,
            context_scheme_worklist_visits: work.context_scheme_worklist_visits,
            context_scheme_worklist_changes: work.context_scheme_worklist_changes,
            wrapper_scheme_worklist_invocations: work.wrapper_scheme_worklist_invocations,
            wrapper_scheme_worklist_visits: work.wrapper_scheme_worklist_visits,
            wrapper_scheme_changed_owners: work.wrapper_scheme_changed_owners,
            wrapper_scheme_parameter_changes: work.wrapper_scheme_parameter_changes,
            wrapper_scheme_result_changes: work.wrapper_scheme_result_changes,
            checked_flow_cache_hits: work.checked_flow_cache_hits,
            checked_flow_cache_misses: work.checked_flow_cache_misses,
            checked_flow_cache_invalidations: work.checked_flow_cache_invalidations,
            checked_flow_cache_reverse_invalidation_traversals: work
                .checked_flow_cache_reverse_invalidation_traversals,
            checked_flow_cache_full_resets: work.checked_flow_cache_full_resets,
            checked_flow_cache_rejected_invalid_ids: work.checked_flow_cache_rejected_invalid_ids,
            checked_flow_indexed_read_hits: work.checked_flow_indexed_read_hits,
            checked_flow_indexed_read_missing: work.checked_flow_indexed_read_missing,
            checked_flow_indexed_read_rejected: work.checked_flow_indexed_read_rejected,
            checked_flow_indexed_out_hits: work.checked_flow_indexed_out_hits,
            checked_flow_indexed_out_missing: work.checked_flow_indexed_out_missing,
            diagnostic_flow_install_attempts: work.diagnostic_flow_install_attempts,
            diagnostic_flow_duplicate_ids: work.diagnostic_flow_duplicate_ids,
            diagnostic_flow_out_of_range_ids: work.diagnostic_flow_out_of_range_ids,
            diagnostic_flow_missing_parser_ids: work.diagnostic_flow_missing_parser_ids,
            diagnostic_replay_requests: work.diagnostic_replay_requests,
            diagnostic_replay_hits: work.diagnostic_replay_hits,
            diagnostic_replay_misses: work.diagnostic_replay_misses,
            diagnostic_replay_unique_expressions: work.diagnostic_replay_unique_expressions,
            ..TypeCheckWorkSample::default()
        }
    }};
}

macro_rules! merge_owner_work_sample {
    ($sample:expr, $work:expr) => {{
        let mut sample = $sample;
        let work = $work;
        sample.owner_statements = work.statements;
        sample.owner_expressions = work.expressions;
        sample.owner_local_constraints = work.local_constraints;
        sample.owner_interface_imports = work.interface_imports;
        sample.owner_interface_plan_direct_owners = work.interface_plan_direct_owners;
        sample.owner_interface_plan_required_owners = work.interface_plan_required_owners;
        sample.owner_interface_plan_provider_sccs = work.interface_plan_provider_sccs;
        sample.owner_interface_plan_result_transfers = work.interface_plan_result_transfers;
        sample.owner_interface_plan_transfer_nodes = work.interface_plan_transfer_nodes;
        sample.owner_interface_plan_transfer_edges = work.interface_plan_transfer_edges;
        sample.owner_calls = work.calls;
        sample.owner_unification_steps = work.unification_steps;
        sample
    }};
}

macro_rules! owner_work_sample {
    ($work:expr) => {{ merge_owner_work_sample!(TypeCheckWorkSample::default(), $work) }};
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
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

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    match args.first().map(String::as_str) {
        Some("warm-session") => return warm_session_sample(&args[1..]),
        Some("synthetic-scaling") => return synthetic_scaling_sample(&args[1..]),
        _ => {}
    }
    let source = args
        .first()
        .ok_or("compiler-sample requires a source path")?;
    let intent = option_value(args, "--intent")?
        .ok_or("compiler-sample requires --intent <diagnostics|verified>")?;
    let intent = match intent.as_str() {
        "diagnostics" => SampleIntent::Diagnostics,
        "verified" => SampleIntent::Verified,
        other => return Err(format!("unknown compiler sample intent `{other}`").into()),
    };
    let mode = option_value(args, "--mode")?
        .ok_or("compiler-sample requires --mode <fresh-process|empty-session>")?;
    let mode = match mode.as_str() {
        "fresh-process" => SampleMode::FreshProcess,
        "empty-session" => SampleMode::EmptySession,
        other => return Err(format!("unknown compiler sample mode `{other}`").into()),
    };
    let sample_count = option_value(args, "--samples")?
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|error| format!("invalid --samples count: {error}"))?
        .unwrap_or(1);
    if sample_count != 1 {
        return Err(
            "compiler cold observations require exactly one sample per producer process".into(),
        );
    }
    reject_unknown_options(args, &["--intent", "--mode", "--samples"])?;

    let mut samples = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        samples.push(match (mode, intent) {
            (SampleMode::FreshProcess, SampleIntent::Diagnostics) => {
                diagnostics_sample(Path::new(source))?
            }
            (SampleMode::FreshProcess, SampleIntent::Verified) => {
                verified_sample(Path::new(source))?
            }
            (SampleMode::EmptySession, SampleIntent::Diagnostics) => {
                session_diagnostics_sample(Path::new(source))?
            }
            (SampleMode::EmptySession, SampleIntent::Verified) => {
                session_verified_sample(Path::new(source))?
            }
        });
    }
    serde_json::to_writer(
        std::io::stdout().lock(),
        &SampleBatch {
            format_version: FORMAT_VERSION,
            source: source.clone(),
            intent,
            compiler_state: mode.compiler_state(),
            target_profile: TargetProfile::SoftwareDefault.as_str(),
            program_role: ProgramRole::Client.as_str(),
            compiler_threads: 1,
            // The compiler currently has no persistent artifact-cache backend.
            // Keep this producer-owned evidence explicit so adding one cannot
            // silently inherit a passing cold-report schema.
            compiler_caches: "disabled",
            peak_rss_unit: "KiB",
            peak_rss_scope: "process-high-water-through-compiler-artifact",
            cache_hit_count: 0,
            samples,
        },
    )?;
    println!();
    Ok(())
}

fn warm_session_sample(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let source = args
        .first()
        .ok_or("warm-session sample requires a primary source path")?;
    let switch_source = option_value(args, "--switch-source")?
        .ok_or("warm-session sample requires --switch-source <path>")?;
    let edit_unit =
        option_value(args, "--edit-unit")?.ok_or("warm-session requires --edit-unit <path>")?;
    let edit_from =
        option_value(args, "--edit-from")?.ok_or("warm-session requires --edit-from <text>")?;
    let edit_to =
        option_value(args, "--edit-to")?.ok_or("warm-session requires --edit-to <text>")?;
    let setup_samples = required_usize_option(args, "--setup-samples")?;
    let scored_samples = required_usize_option(args, "--scored-samples")?;
    let total_samples = setup_samples
        .checked_add(scored_samples)
        .filter(|count| *count > 0 && *count <= 128)
        .ok_or("warm-session setup + scored samples must be in 1..=128")?;
    if source == &switch_source || edit_from.is_empty() || edit_from == edit_to {
        return Err("warm-session sources and edit replacement must be distinct".into());
    }
    reject_unknown_options(
        args,
        &[
            "--switch-source",
            "--edit-unit",
            "--edit-from",
            "--edit-to",
            "--setup-samples",
            "--scored-samples",
        ],
    )?;

    let (entrypoint, primary_units) = compiler_source_project_for_path(Path::new(source))?;
    let unit = primary_units
        .iter()
        .find(|unit| unit.path == edit_unit)
        .ok_or_else(|| format!("warm-session source bundle has no edit unit `{edit_unit}`"))?;
    if unit.source.matches(&edit_from).count() != 1 {
        return Err(format!(
            "warm-session edit marker occurs {} times in `{edit_unit}`; expected exactly one",
            unit.source.matches(&edit_from).count()
        )
        .into());
    }
    let original_unit = unit.source.clone();
    let edited_unit = original_unit.replacen(&edit_from, &edit_to, 1);
    let original_unit_sha256 = sha256_text(&original_unit);
    let edited_unit_sha256 = sha256_text(&edited_unit);
    let (switch_entrypoint, switch_units) =
        compiler_source_project_for_path(Path::new(&switch_source))?;

    let mut session = CompilerSession::new();
    let primary_project = session.open_project(CompilerProject::new(
        entrypoint,
        primary_units,
        TargetProfile::SoftwareDefault,
        ProgramRole::Client,
        ApplicationIdentity::compiler_default(),
    ))?;
    let switch_project = session.open_project(CompilerProject::new(
        switch_entrypoint,
        switch_units,
        TargetProfile::SoftwareDefault,
        ProgramRole::Client,
        ApplicationIdentity::compiler_default(),
    ))?;
    let base_revision = session.revision(primary_project)?;
    let switch_revision = session.revision(switch_project)?;
    let mut compiler_request_count = 0_u64;
    compiler_request_count += 1;
    let (initial_source_bundle_digest_v1, initial_plan_sha256) = {
        let result = session.request(
            primary_project,
            base_revision,
            CompileIntent::VerifiedPreview,
            &CancellationToken::new(),
        )?;
        compiled_identity(
            result
                .compiled()
                .ok_or("initial warm request returned no compiled plan")?,
        )?
    };
    compiler_request_count += 1;
    let switch_plan_sha256 = {
        let result = session.request(
            switch_project,
            switch_revision,
            CompileIntent::VerifiedPreview,
            &CancellationToken::new(),
        )?;
        plan_sha256(
            result
                .compiled()
                .ok_or("initial switch request returned no compiled plan")?
                .plan
                .plan(),
        )?
    };

    let mut edits = Vec::with_capacity(total_samples);
    let mut current_is_edited = false;
    for sequence in 0..total_samples {
        let previous_revision = session.revision(primary_project)?;
        let last_good_revision_before = session
            .last_verified(primary_project)?
            .ok_or("warm project lost its initial verified plan")?
            .0;
        let (next_source, direction) = if current_is_edited {
            (original_unit.clone(), "reverse")
        } else {
            (edited_unit.clone(), "forward")
        };
        reset_compiler_allocation_counters();
        let edit_started = Instant::now();
        let update_started = Instant::now();
        let revision = session.apply_update(
            primary_project,
            UnitUpdate::new(edit_unit.clone(), next_source),
        )?;
        let update_ack_ms = duration_ms(update_started.elapsed());
        compiler_request_count += 1;
        let diagnostics_started = Instant::now();
        let diagnostics_result = session.request(
            primary_project,
            revision,
            CompileIntent::Diagnostics,
            &CancellationToken::new(),
        )?;
        let diagnostics_request_ms = duration_ms(diagnostics_started.elapsed());
        let edit_to_diagnostics_ms = duration_ms(edit_started.elapsed());
        let diagnostics_allocations = compiler_allocation_counters().into();
        let (
            diagnostic_count,
            full_document_typecheck_coverage,
            diagnostics_work,
            diagnostics_phase,
        ) = {
            let diagnostics = diagnostics_result
                .diagnostics()
                .ok_or("warm diagnostics request returned no diagnostics result")?;
            if diagnostics.has_errors() {
                return Err(format!(
                    "warm edit revision {} produced {} diagnostic(s)",
                    revision.0, diagnostics.profile.diagnostic_count
                )
                .into());
            }
            let (work, phase) = diagnostics_work_and_phase(diagnostics);
            (
                diagnostics.profile.diagnostic_count,
                diagnostics.full_document_typecheck_coverage(),
                work,
                phase,
            )
        };
        drop(diagnostics_result);

        reset_compiler_allocation_counters();
        let preview_started = Instant::now();
        compiler_request_count += 1;
        let preview_result = session.request(
            primary_project,
            revision,
            CompileIntent::VerifiedPreview,
            &CancellationToken::new(),
        )?;
        let verified_preview_request_ms = duration_ms(preview_started.elapsed());
        let edit_to_verified_preview_ms = duration_ms(edit_started.elapsed());
        let preview_allocations = compiler_allocation_counters().into();
        let (source_bundle_digest_v1, plan_sha256, preview_work, preview_phase) = {
            let compiled = preview_result
                .compiled()
                .ok_or("warm preview request returned no compiled plan")?;
            let (work, phase) = compiled_work_and_phase(compiled);
            let source_digest = compiled.source_bundle_digest_v1.to_string();
            (
                source_digest,
                plan_sha256(compiled.plan.plan())?,
                work,
                phase,
            )
        };
        drop(preview_result);
        let published_revision = session
            .last_verified(primary_project)?
            .ok_or("successful warm preview was not published")?
            .0;
        if revision.0 != previous_revision.0.saturating_add(1)
            || published_revision != revision
            || !full_document_typecheck_coverage
        {
            return Err(
                "warm edit revision, coverage, or publication identity is inconsistent".into(),
            );
        }
        current_is_edited = !current_is_edited;
        edits.push(WarmEditSample {
            sequence,
            scored: sequence >= setup_samples,
            direction,
            previous_revision: previous_revision.0,
            revision: revision.0,
            last_good_revision_before: last_good_revision_before.0,
            update_ack_ms,
            diagnostics_request_ms,
            edit_to_diagnostics_ms,
            verified_preview_request_ms,
            edit_to_verified_preview_ms,
            diagnostic_count,
            full_document_typecheck_coverage,
            source_bundle_digest_v1,
            plan_sha256,
            published_revision: published_revision.0,
            diagnostics_allocations,
            preview_allocations,
            diagnostics_work,
            preview_work,
            diagnostics_phase,
            preview_phase,
        });
    }

    let mut switches = Vec::with_capacity(total_samples);
    let mut selected_project = primary_project;
    for sequence in 0..total_samples {
        let next_project = if selected_project == primary_project {
            switch_project
        } else {
            primary_project
        };
        let compiler_requests_before = compiler_request_count;
        reset_compiler_allocation_counters();
        let switch_started = Instant::now();
        selected_project = next_project;
        let lookup_started = Instant::now();
        let (selected_revision, compiled) = session
            .last_verified(selected_project)?
            .ok_or("loaded switch target has no verified bundle")?;
        let loaded_bundle_lookup_ms = duration_ms(lookup_started.elapsed());
        let acknowledgement_ms = duration_ms(switch_started.elapsed());
        let allocations = compiler_allocation_counters();
        let selected_plan_sha256 = plan_sha256(compiled.plan.plan())?;
        switches.push(LoadedSwitchSample {
            sequence,
            scored: sequence >= setup_samples,
            from_project_id: if selected_project == primary_project {
                switch_project.0
            } else {
                primary_project.0
            },
            to_project_id: selected_project.0,
            selected_revision: selected_revision.0,
            acknowledgement_ms,
            loaded_bundle_lookup_ms,
            allocation_calls: allocations.allocation_calls,
            allocated_bytes: allocations.allocated_bytes,
            compiler_requests_before,
            compiler_requests_after: compiler_request_count,
            selected_plan_sha256,
        });
    }

    let last_good_revision_before = session
        .last_verified(primary_project)?
        .ok_or("warm project has no last-good plan before cancellation probe")?
        .0;
    let cancellation_source = if current_is_edited {
        original_unit.clone()
    } else {
        edited_unit.clone()
    };
    let cancellation_revision = session.apply_update(
        primary_project,
        UnitUpdate::new(edit_unit.clone(), cancellation_source),
    )?;
    current_is_edited = !current_is_edited;
    let cancellation = {
        let token = CancellationToken::new();
        token.cancel();
        let started = Instant::now();
        compiler_request_count += 1;
        let request_rejected = session
            .request(
                primary_project,
                cancellation_revision,
                CompileIntent::Diagnostics,
                &token,
            )
            .is_err();
        let stop_latency_ms = duration_ms(started.elapsed());
        let last_good_revision_after = session
            .last_verified(primary_project)?
            .ok_or("canceled request removed the last-good plan")?
            .0;
        CancellationEvidence {
            scope: "pre-canceled-request",
            revision: cancellation_revision.0,
            token_canceled_before_request: true,
            request_rejected,
            stop_latency_ms,
            last_good_revision_before: last_good_revision_before.0,
            last_good_revision_after: last_good_revision_after.0,
            publication_unchanged: last_good_revision_after == last_good_revision_before,
            // CompilerSession is synchronous and exposes no worker queue or
            // generation handle that can soundly prove an in-flight
            // supersession race yet.
            in_flight_supersession_supported: false,
        }
    };

    let latest_source = if current_is_edited {
        original_unit
    } else {
        edited_unit
    };
    let latest_revision = session.apply_update(
        primary_project,
        UnitUpdate::new(edit_unit.clone(), latest_source),
    )?;
    compiler_request_count += 1;
    let stale_request_rejected = session
        .request(
            primary_project,
            cancellation_revision,
            CompileIntent::Diagnostics,
            &CancellationToken::new(),
        )
        .is_err();
    let last_good_revision_after_stale_request = session
        .last_verified(primary_project)?
        .ok_or("stale request removed the last-good plan")?
        .0;
    let publish_started = Instant::now();
    compiler_request_count += 1;
    {
        let result = session.request(
            primary_project,
            latest_revision,
            CompileIntent::VerifiedPreview,
            &CancellationToken::new(),
        )?;
        if result.compiled().is_none() {
            return Err("latest-generation request returned no compiled plan".into());
        }
    }
    let publish_latest_ms = duration_ms(publish_started.elapsed());
    let published_revision = session
        .last_verified(primary_project)?
        .ok_or("latest-generation request was not published")?
        .0;
    let latest_generation = LatestGenerationEvidence {
        stale_revision: cancellation_revision.0,
        latest_revision: latest_revision.0,
        stale_request_rejected,
        last_good_revision_after_stale_request: last_good_revision_after_stale_request.0,
        published_revision: published_revision.0,
        publish_latest_ms,
        no_stale_publication: stale_request_rejected
            && last_good_revision_after_stale_request == last_good_revision_before
            && published_revision == latest_revision,
    };

    serde_json::to_writer(
        std::io::stdout().lock(),
        &WarmSessionBatch {
            format_version: FORMAT_VERSION,
            workload: "warm-session-v1",
            source: source.clone(),
            switch_source,
            edit_unit,
            producer_pid: std::process::id(),
            compiler_threads: 1,
            compiler_caches: "disabled",
            setup_samples,
            scored_samples,
            primary_project_id: primary_project.0,
            switch_project_id: switch_project.0,
            base_revision: base_revision.0,
            initial_source_bundle_digest_v1,
            initial_plan_sha256,
            switch_plan_sha256,
            original_unit_sha256,
            edited_unit_sha256,
            compiler_request_count,
            edits,
            switches,
            cancellation,
            latest_generation,
        },
    )?;
    println!();
    Ok(())
}

fn synthetic_scaling_sample(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let dimension = args
        .first()
        .ok_or("synthetic-scaling requires a dimension")?;
    let size = required_usize_option(args, "--size")?;
    let intent = match option_value(args, "--intent")?
        .ok_or("synthetic-scaling requires --intent <diagnostics|verified>")?
        .as_str()
    {
        "diagnostics" => SampleIntent::Diagnostics,
        "verified" => SampleIntent::Verified,
        other => return Err(format!("unknown synthetic scaling intent `{other}`").into()),
    };
    reject_unknown_options(args, &["--size", "--intent"])?;
    let units = synthetic_units(dimension, size)?;
    let synthetic_source_sha256 = synthetic_units_sha256(&units);

    reset_compiler_allocation_counters();
    let started = Instant::now();
    let mut session = CompilerSession::new();
    let project = session.open_project(CompilerProject::new(
        "SYNTHETIC.bn",
        units,
        TargetProfile::SoftwareDefault,
        ProgramRole::Server,
        ApplicationIdentity::compiler_default(),
    ))?;
    let revision = session.revision(project)?;
    let result = session.request(
        project,
        revision,
        match intent {
            SampleIntent::Diagnostics => CompileIntent::Diagnostics,
            SampleIntent::Verified => CompileIntent::VerifiedCheck,
        },
        &CancellationToken::new(),
    )?;
    let elapsed_ms = duration_ms(started.elapsed());
    let allocations = compiler_allocation_counters().into();
    let peak_rss_kib = peak_rss_kib();
    let (source_bundle_digest_v1, plan_sha256, work, phase) = match intent {
        SampleIntent::Diagnostics => {
            let diagnostics = result
                .diagnostics()
                .ok_or("synthetic diagnostics returned no diagnostics result")?;
            if diagnostics.has_errors() {
                return Err(format!(
                    "synthetic {dimension}/{size} produced {} diagnostic(s)",
                    diagnostics.profile.diagnostic_count
                )
                .into());
            }
            let source_digest = diagnostics.source_bundle_digest_v1().to_string();
            let (work, phase) = diagnostics_work_and_phase(diagnostics);
            (source_digest, None, work, phase)
        }
        SampleIntent::Verified => {
            let compiled = result
                .compiled()
                .ok_or("synthetic verified request returned no plan")?;
            let source_digest = compiled.source_bundle_digest_v1.to_string();
            let plan_hash = plan_sha256(compiled.plan.plan())?;
            let (work, phase) = compiled_work_and_phase(compiled);
            (source_digest, Some(plan_hash), work, phase)
        }
    };

    serde_json::to_writer(
        std::io::stdout().lock(),
        &SyntheticScalingBatch {
            format_version: FORMAT_VERSION,
            workload: "synthetic-scaling-v1",
            generator: "boon-synthetic-scaling-v1",
            dimension: dimension.clone(),
            size,
            intent,
            producer_pid: std::process::id(),
            compiler_threads: 1,
            compiler_caches: "disabled",
            revision: revision.0,
            synthetic_source_sha256,
            source_bundle_digest_v1,
            elapsed_ms,
            peak_rss_kib,
            plan_sha256,
            allocations,
            work,
            phase,
        },
    )?;
    println!();
    Ok(())
}

fn synthetic_units(
    dimension: &str,
    size: usize,
) -> Result<Vec<boon_compiler::CompilerSourceUnit>, Box<dyn std::error::Error>> {
    if dimension == "source-unit-count" {
        let mut units = Vec::with_capacity(size.saturating_add(1));
        units.push(boon_compiler::CompilerSourceUnit {
            path: "SYNTHETIC.bn".to_owned(),
            source: "result: 0\n".to_owned(),
        });
        for index in 0..size {
            units.push(boon_compiler::CompilerSourceUnit {
                path: format!("synthetic/unit_{index:05}.bn"),
                source: format!("FUNCTION unit_{index}() {{\n    {index}\n}}\n"),
            });
        }
        return Ok(units);
    }

    let mut source = String::new();
    match dimension {
        "call-depth" => {
            if size == 0 {
                source.push_str("result: 0\n");
            } else {
                source.push_str("result: depth_0()\n\n");
                for index in 0..size {
                    source.push_str(&format!("FUNCTION depth_{index}() {{\n    "));
                    if index + 1 == size {
                        source.push('0');
                    } else {
                        source.push_str(&format!("depth_{}()", index + 1));
                    }
                    source.push_str("\n}\n\n");
                }
            }
        }
        "call-site-count" => {
            source.push_str("result: LIST {\n");
            for _ in 0..size {
                source.push_str("    leaf()\n");
            }
            source.push_str("}\n\nFUNCTION leaf() {\n    0\n}\n");
        }
        "contextual-call-site-count" => {
            source.push_str("rows: LIST { [value: 1] }\nresult: [\n");
            for index in 0..size {
                source.push_str(&format!(
                    "    mapped_{index}: rows |> List/map(item, new: item.value + {index})\n"
                ));
            }
            source.push_str("]\n");
        }
        "static-branch-count" => {
            for index in 0..=size {
                source.push_str(&format!(
                    "FUNCTION branch_{index}() {{\n    {index}\n}}\n\n"
                ));
            }
            source.push_str("FUNCTION dispatch(choice) {\n    choice |> WHEN {\n");
            for index in 0..=size {
                source.push_str(&format!("        Choice{index} => branch_{index}()\n"));
            }
            source.push_str("    }\n}\n\nresult: dispatch(choice: Choice0)\n");
        }
        "dependency-cone-size" => {
            source.push_str("result: LIST {\n    cone_root()\n");
            for index in 0..size {
                source.push_str(&format!("    cone_{index}()\n"));
            }
            source.push_str("}\n\nFUNCTION cone_root() {\n    0\n}\n\n");
            for index in 0..size {
                source.push_str(&format!("FUNCTION cone_{index}() {{\n    {index}\n}}\n\n"));
            }
        }
        other => return Err(format!("unknown synthetic scaling dimension `{other}`").into()),
    }
    Ok(vec![boon_compiler::CompilerSourceUnit {
        path: "SYNTHETIC.bn".to_owned(),
        source,
    }])
}

fn synthetic_units_sha256(units: &[boon_compiler::CompilerSourceUnit]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"boon-synthetic-source-bundle-v1\0");
    for unit in units {
        hasher.update((unit.path.len() as u64).to_le_bytes());
        hasher.update(unit.path.as_bytes());
        hasher.update((unit.source.len() as u64).to_le_bytes());
        hasher.update(unit.source.as_bytes());
    }
    hex_digest(hasher.finalize())
}

fn diagnostics_work_and_phase(
    diagnostics: &boon_compiler::CompilerDiagnostics,
) -> (WorkSample, PhaseSample) {
    (
        WorkSample {
            source_units: diagnostics.profile.source_unit_count,
            parsed_expressions: diagnostics.profile.expression_count,
            checked_expressions: diagnostics.profile.checked_expression_count,
            checked_calls: diagnostics.profile.call_count,
            parse: parser_work_sample!(diagnostics.profile.parse_work),
            typecheck: owner_work_sample!(diagnostics.profile.owner_work),
            ..WorkSample::default()
        },
        PhaseSample {
            parse_ms: diagnostics.profile.parse_ms,
            typecheck_ms: diagnostics.profile.typecheck_ms,
            ..PhaseSample::default()
        },
    )
}

fn compiled_work_and_phase(
    compiled: &boon_compiler::CompiledSealedMachinePlanFromSource,
) -> (WorkSample, PhaseSample) {
    (
        WorkSample {
            source_units: compiled.profile.source_unit_count,
            parsed_expressions: compiled.profile.expression_count,
            checked_expressions: compiled.profile.checked_expression_count,
            checked_calls: compiled.profile.checked_call_count,
            semantic_graph_nodes: compiled.profile.graph_node_count,
            cancellation_checkpoints: compiled.profile.cancellation_checkpoint_count,
            parse: parser_work_sample!(compiled.profile.parse_work),
            typecheck: merge_owner_work_sample!(
                typecheck_work_sample!(compiled.profile.typecheck_work),
                compiled.profile.owner_work
            ),
        },
        PhaseSample {
            parse_ms: compiled.profile.parse_ms,
            typecheck_ms: compiled.profile.typecheck_ms,
            semantic_ms: compiled.profile.semantic_ms,
            contract_verify_ms: compiled.profile.contract_verify_ms,
            ir_lower_ms: compiled.profile.ir_lower_ms,
            ir_validation_ms: compiled.profile.verify_ms,
            backend_ms: compiled.profile.compile_ms,
            plan_validation_ms: compiled.profile.plan_validation_ms,
            ..PhaseSample::default()
        },
    )
}

fn compiled_identity(
    compiled: &boon_compiler::CompiledSealedMachinePlanFromSource,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    Ok((
        compiled.source_bundle_digest_v1.to_string(),
        plan_sha256(compiled.plan.plan())?,
    ))
}

fn plan_sha256(plan: &boon_plan::MachinePlan) -> Result<String, serde_json::Error> {
    let mut hasher = Sha256Writer::default();
    serde_json::to_writer_pretty(&mut hasher, plan)?;
    Ok(hex_digest(hasher.finish()))
}

fn sha256_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex_digest(hasher.finalize())
}

fn required_usize_option(
    args: &[String],
    option: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    option_value(args, option)?
        .ok_or_else(|| format!("{option} requires a value").into())
        .and_then(|value| {
            value
                .parse::<usize>()
                .map_err(|error| format!("invalid {option}: {error}").into())
        })
}

fn diagnostics_sample(source: &Path) -> Result<Sample, Box<dyn std::error::Error>> {
    session_diagnostics_sample(source)
}

fn session_diagnostics_sample(source: &Path) -> Result<Sample, Box<dyn std::error::Error>> {
    reset_compiler_allocation_counters();
    let observation_started_unix_us = unix_time_us()?;
    let started = Instant::now();
    let (entrypoint, units) = compiler_source_project_for_path(source)?;
    let mut session = CompilerSession::new();
    let project = session.open_project(CompilerProject::new(
        entrypoint,
        units,
        TargetProfile::SoftwareDefault,
        ProgramRole::Client,
        ApplicationIdentity::compiler_default(),
    ))?;
    let revision = session.revision(project)?;
    let result = session.request(
        project,
        revision,
        CompileIntent::Diagnostics,
        &CancellationToken::new(),
    )?;
    let diagnostics = result
        .diagnostics()
        .ok_or("diagnostics session request returned no diagnostics result")?;
    let elapsed_ms = duration_ms(started.elapsed());
    let allocations = compiler_allocation_counters().into();
    let compiler_artifact_ready_unix_us = unix_time_us()?;
    let compiler_peak_rss_kib = peak_rss_kib();
    if diagnostics.has_errors() {
        return Err(format!(
            "performance fixture produced {} diagnostic(s)",
            diagnostics.profile.diagnostic_count
        )
        .into());
    }
    let (work, phase) = diagnostics_work_and_phase(diagnostics);
    Ok(Sample {
        producer_pid: std::process::id(),
        observation_started_unix_us,
        compiler_artifact_ready_unix_us,
        elapsed_ms,
        peak_rss_kib: compiler_peak_rss_kib,
        source_bundle_digest_v1: diagnostics.source_bundle_digest_v1().to_string(),
        diagnostics_fingerprint_v1: Some(hex_digest(diagnostics.fingerprint_v1())),
        diagnostic_count: diagnostics.profile.diagnostic_count,
        full_document_typecheck_coverage: Some(diagnostics.full_document_typecheck_coverage()),
        plan_sha256: None,
        allocations,
        work,
        phase,
    })
}

fn verified_sample(source: &Path) -> Result<Sample, Box<dyn std::error::Error>> {
    reset_compiler_allocation_counters();
    let observation_started_unix_us = unix_time_us()?;
    let started = Instant::now();
    let checked = check_runtime_source(CompilerCheckRequest::source_path(
        source,
        ProgramRole::Client,
    ))?;
    let compiled = finish_checked_sealed_machine_plan(
        checked,
        CheckedCompileRequest::new(
            TargetProfile::SoftwareDefault,
            ProgramRole::Client,
            ApplicationIdentity::compiler_default(),
        ),
    )?;
    let elapsed_ms = duration_ms(started.elapsed());
    let allocations = compiler_allocation_counters().into();
    compiled_sample(
        &compiled,
        elapsed_ms,
        allocations,
        observation_started_unix_us,
    )
}

fn session_verified_sample(source: &Path) -> Result<Sample, Box<dyn std::error::Error>> {
    reset_compiler_allocation_counters();
    let observation_started_unix_us = unix_time_us()?;
    let started = Instant::now();
    let (entrypoint, units) = compiler_source_project_for_path(source)?;
    let mut session = CompilerSession::new();
    let project = session.open_project(CompilerProject::new(
        entrypoint,
        units,
        TargetProfile::SoftwareDefault,
        ProgramRole::Client,
        ApplicationIdentity::compiler_default(),
    ))?;
    let revision = session.revision(project)?;
    let result = session.request(
        project,
        revision,
        CompileIntent::VerifiedPreview,
        &CancellationToken::new(),
    )?;
    let compiled = result
        .compiled()
        .ok_or("verified session request returned no compiled result")?;
    let elapsed_ms = duration_ms(started.elapsed());
    let allocations = compiler_allocation_counters().into();
    compiled_sample(
        compiled,
        elapsed_ms,
        allocations,
        observation_started_unix_us,
    )
}

fn compiled_sample(
    compiled: &boon_compiler::CompiledSealedMachinePlanFromSource,
    elapsed_ms: f64,
    allocations: AllocationSample,
    observation_started_unix_us: u64,
) -> Result<Sample, Box<dyn std::error::Error>> {
    // Capture the compiler high-water mark before verifier/report-only work
    // below can allocate a second representation of the plan.
    let compiler_peak_rss_kib = peak_rss_kib();
    let compiler_artifact_ready_unix_us = unix_time_us()?;
    let plan_validation_ms = compiled.profile.plan_validation_ms;
    let validation = compiled.plan.verification();
    if validation.status != "pass" {
        return Err("compiled performance fixture failed MachinePlan validation".into());
    }
    let serialization_started = Instant::now();
    let mut plan_hasher = Sha256Writer::default();
    serde_json::to_writer_pretty(&mut plan_hasher, compiled.plan.plan())?;
    let serialization_ms = duration_ms(serialization_started.elapsed());
    let plan_sha256 = hex_digest(plan_hasher.finish());
    Ok(Sample {
        producer_pid: std::process::id(),
        observation_started_unix_us,
        compiler_artifact_ready_unix_us,
        elapsed_ms,
        peak_rss_kib: compiler_peak_rss_kib,
        source_bundle_digest_v1: compiled.source_bundle_digest_v1.to_string(),
        diagnostics_fingerprint_v1: None,
        diagnostic_count: 0,
        full_document_typecheck_coverage: None,
        plan_sha256: Some(plan_sha256),
        allocations,
        work: compiled_work_and_phase(compiled).0,
        phase: PhaseSample {
            parse_ms: compiled.profile.parse_ms,
            typecheck_ms: compiled.profile.typecheck_ms,
            semantic_ms: compiled.profile.semantic_ms,
            contract_verify_ms: compiled.profile.contract_verify_ms,
            ir_lower_ms: compiled.profile.ir_lower_ms,
            ir_validation_ms: compiled.profile.verify_ms,
            backend_ms: compiled.profile.compile_ms,
            plan_validation_ms,
            serialization_ms,
        },
    })
}

#[derive(Default)]
struct Sha256Writer(Sha256);

impl Sha256Writer {
    fn finish(self) -> [u8; 32] {
        self.0.finalize().into()
    }
}

impl Write for Sha256Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn option_value(
    args: &[String],
    option: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let Some(index) = args.iter().position(|arg| arg == option) else {
        return Ok(None);
    };
    Ok(Some(
        args.get(index + 1)
            .ok_or_else(|| format!("{option} requires a value"))?
            .clone(),
    ))
}

fn reject_unknown_options(
    args: &[String],
    options_with_values: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut index = 1usize;
    while index < args.len() {
        let option = args[index].as_str();
        if options_with_values.contains(&option) {
            index += 2;
        } else {
            return Err(format!("unknown argument `{option}`").into());
        }
    }
    Ok(())
}

fn duration_ms(duration: std::time::Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn unix_time_us() -> Result<u64, Box<dyn std::error::Error>> {
    let micros = SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros();
    u64::try_from(micros).map_err(|_| "compiler sample wall-clock timestamp exceeds u64".into())
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(target_os = "linux")]
fn peak_rss_kib() -> u64 {
    // `/proc/self/status` exposes this process's resident high-water mark in
    // KiB. Reading the kernel-owned label avoids libc `rusage` ABI/unit drift
    // and agrees with `/usr/bin/time -v` on the reference Linux host.
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return 0;
    };
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
}

#[cfg(not(target_os = "linux"))]
fn peak_rss_kib() -> u64 {
    0
}
