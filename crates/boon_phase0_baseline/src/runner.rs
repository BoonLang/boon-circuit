use crate::allocator::{AllocationInterval, live_requested_bytes};
use crate::manifest::{ActionDefinition, ActionKind, FixtureDefinition, FixtureManifest};
use crate::report::{
    ActionClass, ActionReport, BaselineReport, FORMAT_VERSION, FactEvidence, FactStatus,
    FixtureReport, LatencySummary, MAX_REPORT_BYTES, MetricEvidence, MetricScope, MetricUnit,
    PROTOCOL, REPORT_KIND, ReportStatus, SemanticClaim, SourceIdentity, StartupEvidence,
    WorkEvidence,
};
use boon_compiler::{
    CompileRequest, CompilerSourceUnit, compile_machine_plan, compiler_source_units_for_path,
};
use boon_contract::{CanonicalSourceBundleV1, SourceBundleUnit};
use boon_plan::{ApplicationIdentity, ProgramRole, SourceId, TargetProfile};
use boon_plan_executor::{
    MachineInstance, RowId, SessionOptions, SourceEvent, SourcePayload, TurnMetrics, Value,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct ProducerOptions {
    pub workspace: PathBuf,
    pub manifest_path: PathBuf,
    pub budget_path: PathBuf,
    pub report_path: PathBuf,
    pub source: SourceIdentity,
}

pub fn produce(options: &ProducerOptions) -> Result<BaselineReport, String> {
    let (manifest, manifest_bytes) = FixtureManifest::load(&options.manifest_path)?;
    let manifest_digest = sha256_bytes(&manifest_bytes);
    let budget_bytes = fs::read(&options.budget_path)
        .map_err(|error| format!("{}: {error}", options.budget_path.display()))?;
    if budget_bytes.is_empty() {
        return Err(format!(
            "{} must not be empty",
            options.budget_path.display()
        ));
    }
    let budget_digest = sha256_bytes(&budget_bytes);
    let mut fixtures = Vec::with_capacity(manifest.fixtures.len());
    for definition in &manifest.fixtures {
        fixtures.push(measure_fixture(&options.workspace, definition)?);
    }
    fixtures.sort_by(|left, right| left.id.cmp(&right.id));

    let report = BaselineReport {
        format: FORMAT_VERSION,
        kind: REPORT_KIND.to_owned(),
        protocol: PROTOCOL.to_owned(),
        generated_unix_ms: unix_time_ms(),
        status: ReportStatus::Pass,
        source: options.source.clone(),
        binary_sha256: sha256_file(&std::env::current_exe().map_err(|error| error.to_string())?)?,
        fixture_manifest_path: relative_display(&options.workspace, &options.manifest_path)?,
        fixture_manifest_sha256: manifest_digest,
        budget_manifest_path: relative_display(&options.workspace, &options.budget_path)?,
        budget_manifest_sha256: budget_digest,
        target_profile: manifest.target_profile,
        build_profile: manifest.build_profile,
        allocator_scope: manifest.allocator_scope,
        host_arch: std::env::consts::ARCH.to_owned(),
        host_os: std::env::consts::OS.to_owned(),
        packed_bytes_by_store_layout: Vec::new(),
        metrics: aggregate_metrics(&fixtures),
        fixtures,
    };
    report.validate()?;
    write_report(&options.report_path, &report)?;
    Ok(report)
}

fn measure_fixture(
    workspace: &Path,
    definition: &FixtureDefinition,
) -> Result<FixtureReport, String> {
    let source_path = workspace.join(&definition.source);
    let discovered_units = compiler_source_units_for_path(&source_path)
        .map_err(|error| format!("fixture {} source units: {error}", definition.id))?;
    let canonical_input = canonical_compiler_units(workspace, &definition.source, discovered_units)
        .map_err(|error| format!("fixture {} source bundle: {error}", definition.id))?;

    let compile_interval = AllocationInterval::begin().map_err(str::to_owned)?;
    let compile_started = Instant::now();
    let compiled = compile_machine_plan(CompileRequest::source_units(
        &canonical_input.entrypoint,
        &canonical_input.units,
        TargetProfile::SoftwareDefault,
        ProgramRole::Client,
        ApplicationIdentity::compiler_default(),
    ))
    .map_err(|error| format!("fixture {} compilation: {error}", definition.id))?;
    let compilation_elapsed_ns = duration_ns(compile_started.elapsed());
    let compilation_allocator = compile_interval.finish();
    let plan = Arc::new(compiled.plan);

    let startup_live_start = live_requested_bytes();
    let startup_interval = AllocationInterval::begin().map_err(str::to_owned)?;
    let startup_started = Instant::now();
    let mut session = MachineInstance::new_shared(Arc::clone(&plan), SessionOptions::default())
        .map_err(|error| format!("fixture {} startup: {error}", definition.id))?;
    let startup_elapsed_ns = duration_ns(startup_started.elapsed());
    let startup_work = WorkEvidence::from(session.startup_metrics());
    let startup_allocator = startup_interval.finish();
    let retained_requested_bytes = live_requested_bytes().saturating_sub(startup_live_start);
    let observed_logical_rows = to_u64(session.logical_row_count());
    if observed_logical_rows < definition.expected_authoritative_rows {
        return Err(format!(
            "fixture {} observed {observed_logical_rows} logical rows below expected {}",
            definition.id, definition.expected_authoritative_rows
        ));
    }

    let mut next_sequence = 1_u64;
    let mut actions = Vec::with_capacity(definition.actions.len());
    for action in &definition.actions {
        actions.push(measure_action(
            &definition.id,
            &mut session,
            action,
            &mut next_sequence,
        )?);
    }

    let mut target_facts = BTreeMap::new();
    target_facts.insert(
        "native-headless".to_owned(),
        FactEvidence {
            status: FactStatus::Measured,
            detail: "release MachineInstance startup and declared deterministic actions completed"
                .to_owned(),
        },
    );
    target_facts.insert(
        "wasm-browser".to_owned(),
        FactEvidence {
            status: FactStatus::Unavailable,
            detail:
                "this Phase 0 producer is native-only; no browser allocator or report transport is installed"
                    .to_owned(),
        },
    );
    target_facts.insert(
        "native-product".to_owned(),
        native_product_fact(&definition.id),
    );

    let mut metrics = BTreeMap::new();
    metrics.insert(
        "retained-runtime-requested-bytes".to_owned(),
        measured(
            retained_requested_bytes,
            MetricUnit::Bytes,
            MetricScope::RetainedRuntime,
            "whole MachineInstance requested-live delta above the retained plan",
        ),
    );
    metrics.insert(
        "logical-index-payload-bytes".to_owned(),
        measured(
            startup_work.ordered_index_current_payload_bytes,
            MetricUnit::Bytes,
            MetricScope::RuntimeStartup,
            "deterministic ordered-index payload; allocator and tree-node overhead excluded",
        ),
    );
    metrics.insert(
        "bytes-per-row".to_owned(),
        match retained_requested_bytes.checked_div(definition.expected_authoritative_rows) {
            None => MetricEvidence::NotApplicable {
                unit: MetricUnit::BytesPerRow,
                reason: "fixture has no authoritative rows".to_owned(),
            },
            Some(bytes_per_row) => measured(
                bytes_per_row,
                MetricUnit::BytesPerRow,
                MetricScope::RetainedRuntime,
                "whole-runtime requested-live delta divided by authoritative rows; not packed store bytes",
            ),
        },
    );

    Ok(FixtureReport {
        id: definition.id.clone(),
        source_path: definition.source.clone(),
        source_bundle_digest_v1: canonical_input.source_bundle_digest_v1,
        expected_authoritative_rows: definition.expected_authoritative_rows,
        observed_logical_rows,
        semantic_claim: SemanticClaim::default(),
        compilation_elapsed_ns,
        compilation_allocator,
        startup: StartupEvidence {
            elapsed_ns: startup_elapsed_ns,
            allocator: startup_allocator,
            work: startup_work,
            retained_requested_bytes,
        },
        actions,
        target_facts,
        metrics,
    })
}

fn measure_action(
    fixture: &str,
    session: &mut MachineInstance,
    definition: &ActionDefinition,
    next_sequence: &mut u64,
) -> Result<ActionReport, String> {
    let prepared = PreparedAction::new(session, definition)
        .map_err(|error| format!("fixture {fixture} action {}: {error}", definition.id))?;
    for ordinal in 0..definition.warmup_turns {
        execute_action(session, definition, &prepared, next_sequence, ordinal).map_err(
            |error| {
                format!(
                    "fixture {fixture} action {} warmup {ordinal}: {error}",
                    definition.id
                )
            },
        )?;
    }

    let mut samples = Vec::with_capacity(definition.measured_turns as usize);
    let mut work_total = WorkEvidence::default();
    let mut work_max = WorkEvidence::default();
    let allocator_interval = AllocationInterval::begin().map_err(str::to_owned)?;
    let measured_started = Instant::now();
    for ordinal in 0..definition.measured_turns {
        let sample_started = Instant::now();
        let work = execute_action(
            session,
            definition,
            &prepared,
            next_sequence,
            ordinal.saturating_add(definition.warmup_turns),
        )
        .map_err(|error| {
            format!(
                "fixture {fixture} action {} measured turn {ordinal}: {error}",
                definition.id
            )
        })?;
        samples.push(duration_ns(sample_started.elapsed()));
        work_total = work_total.saturating_add(work);
        work_max = work_max.component_max(work);
    }
    let total_ns = duration_ns(measured_started.elapsed());
    let allocator = allocator_interval.finish();
    if definition.require_no_full_scan && work_total.access_full_scan_count != 0 {
        return Err(format!(
            "fixture {fixture} action {} performed {} full scans",
            definition.id, work_total.access_full_scan_count
        ));
    }
    if definition.require_no_interaction_index_rebuild
        && work_total.ordered_index_full_rebuild_count != 0
    {
        return Err(format!(
            "fixture {fixture} action {} rebuilt {} indexes during interaction",
            definition.id, work_total.ordered_index_full_rebuild_count
        ));
    }
    let latency = latency_summary(&mut samples, total_ns);
    let turns_per_second_milli = rate_per_second_milli(definition.measured_turns, total_ns);
    let rows_per_second_milli = rate_per_second_milli(
        definition
            .measured_turns
            .saturating_mul(definition.semantic_rows_per_turn),
        total_ns,
    );
    let mut metrics = BTreeMap::new();
    metrics.insert(
        "allocation-count".to_owned(),
        measured(
            allocator
                .allocation_calls
                .saturating_add(allocator.zeroed_allocation_calls)
                .saturating_add(allocator.reallocation_calls),
            MetricUnit::Allocations,
            MetricScope::MeasuredTurns,
            "process-global Rust allocation events in the declared measured interval",
        ),
    );
    metrics.insert(
        "allocated-bytes".to_owned(),
        measured(
            allocator
                .requested_allocation_bytes
                .saturating_add(allocator.requested_reallocation_bytes),
            MetricUnit::Bytes,
            MetricScope::MeasuredTurns,
            "requested allocation plus reallocation bytes in the declared measured interval",
        ),
    );
    metrics.insert(
        "latency-p95".to_owned(),
        measured(
            latency.p95_ns,
            MetricUnit::Nanoseconds,
            MetricScope::MeasuredTurns,
            "nearest-rank p95 with all measured samples retained",
        ),
    );
    metrics.insert(
        "turns-per-second".to_owned(),
        measured(
            turns_per_second_milli,
            MetricUnit::TurnsPerSecondMilli,
            MetricScope::MeasuredTurns,
            "milli-turns per second over the complete measured interval",
        ),
    );
    metrics.insert(
        "rows-per-second".to_owned(),
        if definition.semantic_rows_per_turn == 0 {
            MetricEvidence::NotApplicable {
                unit: MetricUnit::RowsPerSecondMilli,
                reason: "action does not declare a semantic changed-row population".to_owned(),
            }
        } else {
            measured(
                rows_per_second_milli,
                MetricUnit::RowsPerSecondMilli,
                MetricScope::MeasuredTurns,
                "milli-rows per second using the manifest-declared semantic population",
            )
        },
    );
    metrics.insert(
        "charged-work".to_owned(),
        measured(
            work_total.work_unit_count,
            MetricUnit::WorkUnits,
            MetricScope::MeasuredTurns,
            "executor-charged work units; not CPU instructions",
        ),
    );
    metrics.insert(
        "index-work".to_owned(),
        measured(
            work_total
                .access_key_count
                .saturating_add(work_total.access_candidate_count),
            MetricUnit::WorkUnits,
            MetricScope::MeasuredTurns,
            "typed-list keys plus candidates visited",
        ),
    );

    Ok(ActionReport {
        id: definition.id.clone(),
        class: definition.class,
        target: definition.target.clone(),
        warmup_turns: definition.warmup_turns,
        measured_turns: definition.measured_turns,
        allocator,
        latency,
        work_total,
        work_max,
        semantic_rows_per_turn: definition.semantic_rows_per_turn,
        metrics,
    })
}

struct PreparedAction {
    source: Option<SourceId>,
    row: Option<RowId>,
}

impl PreparedAction {
    fn new(session: &MachineInstance, definition: &ActionDefinition) -> Result<Self, String> {
        match definition.kind {
            ActionKind::RootRead => Ok(Self {
                source: None,
                row: None,
            }),
            ActionKind::RootSource | ActionKind::CursorPage => {
                let source = source_id(session, &definition.target)?;
                Ok(Self {
                    source: Some(source),
                    row: None,
                })
            }
            ActionKind::RowSource => {
                let source = source_id(session, &definition.target)?;
                let row = session
                    .row_target_for_source_path(
                        &definition.target,
                        definition.row_key.unwrap_or_default(),
                        definition.row_generation.unwrap_or_default(),
                    )
                    .map_err(|error| error.to_string())?;
                Ok(Self {
                    source: Some(source),
                    row: Some(row),
                })
            }
        }
    }
}

fn execute_action(
    session: &mut MachineInstance,
    definition: &ActionDefinition,
    prepared: &PreparedAction,
    next_sequence: &mut u64,
    ordinal: u64,
) -> Result<WorkEvidence, String> {
    match definition.kind {
        ActionKind::RootRead => {
            let (value, metrics) = session
                .root_value_current_with_metrics(&definition.target)
                .map_err(|error| error.to_string())?;
            drop(value);
            Ok(WorkEvidence::from(&metrics))
        }
        ActionKind::RootSource | ActionKind::RowSource => {
            let source = prepared
                .source
                .ok_or_else(|| "prepared source action has no source ID".to_owned())?;
            let ancestors = prepared.row.as_slice();
            let route = session
                .source_route_token(source, ancestors)
                .map_err(|error| error.to_string())?;
            let payload_text = if definition.payload_text.is_empty() {
                None
            } else {
                let index = (ordinal as usize) % definition.payload_text.len();
                Some(definition.payload_text[index].clone())
            };
            let event = SourceEvent {
                sequence: *next_sequence,
                route,
                source,
                target: prepared.row,
                payload: SourcePayload {
                    text: payload_text,
                    address: definition.payload_address.clone(),
                    ..SourcePayload::default()
                },
            };
            *next_sequence = next_sequence
                .checked_add(1)
                .ok_or_else(|| "source sequence exhausted".to_owned())?;
            let turn = session.apply(event).map_err(|error| error.to_string())?;
            let mut work = WorkEvidence::from(&turn.metrics);
            drop(turn);
            if let Some(target) = &definition.read_after {
                let (value, read_metrics) = session
                    .root_value_current_with_metrics(target)
                    .map_err(|error| error.to_string())?;
                work = work.saturating_add(WorkEvidence::from(&read_metrics));
                drop(value);
            }
            Ok(work)
        }
        ActionKind::CursorPage => {
            let page_target = definition
                .read_after
                .as_deref()
                .ok_or_else(|| "cursor-page action has no read_after target".to_owned())?;
            let (page, first_read_metrics) = session
                .root_value_current_with_metrics(page_target)
                .map_err(|error| error.to_string())?;
            let next = page_next(page)?;
            let source = prepared
                .source
                .ok_or_else(|| "prepared cursor-page action has no source ID".to_owned())?;
            let route = session
                .source_route_token(source, &[])
                .map_err(|error| error.to_string())?;
            let turn = session
                .apply(SourceEvent {
                    sequence: *next_sequence,
                    route,
                    source,
                    target: None,
                    payload: SourcePayload {
                        fields: BTreeMap::from([("value".to_owned(), next)]),
                        ..SourcePayload::default()
                    },
                })
                .map_err(|error| error.to_string())?;
            *next_sequence = next_sequence
                .checked_add(1)
                .ok_or_else(|| "source sequence exhausted".to_owned())?;
            let mut work = WorkEvidence::from(&first_read_metrics)
                .saturating_add(WorkEvidence::from(&turn.metrics));
            drop(turn);
            let (page, second_read_metrics) = session
                .root_value_current_with_metrics(page_target)
                .map_err(|error| error.to_string())?;
            work = work.saturating_add(WorkEvidence::from(&second_read_metrics));
            drop(page);
            Ok(work)
        }
    }
}

fn page_next(value: Value) -> Result<Value, String> {
    let Value::Tag { tag, mut fields } = value else {
        return Err("cursor-page target is not a Page tag".to_owned());
    };
    if tag != "Page" {
        return Err(format!("cursor-page target carries the `{tag}` tag"));
    }
    fields
        .remove("next")
        .ok_or_else(|| "cursor-page target has no next value".to_owned())
}

fn source_id(session: &MachineInstance, path: &str) -> Result<SourceId, String> {
    session
        .plan()
        .source_routes
        .iter()
        .find(|route| route.path == path)
        .map(|route| route.source_id)
        .ok_or_else(|| format!("MachinePlan has no source route `{path}`"))
}

fn aggregate_metrics(fixtures: &[FixtureReport]) -> BTreeMap<String, MetricEvidence> {
    let actions = fixtures
        .iter()
        .flat_map(|fixture| fixture.actions.iter())
        .collect::<Vec<_>>();
    let allocation_count = actions.iter().fold(0_u64, |total, action| {
        total
            .saturating_add(action.allocator.allocation_calls)
            .saturating_add(action.allocator.zeroed_allocation_calls)
            .saturating_add(action.allocator.reallocation_calls)
    });
    let allocated_bytes = actions.iter().fold(0_u64, |total, action| {
        total
            .saturating_add(action.allocator.requested_allocation_bytes)
            .saturating_add(action.allocator.requested_reallocation_bytes)
    });
    let bytes_per_row = fixtures
        .iter()
        .filter(|fixture| fixture.expected_authoritative_rows > 0)
        .map(|fixture| {
            fixture.startup.retained_requested_bytes / fixture.expected_authoritative_rows
        })
        .max()
        .unwrap_or_default();
    let bytes_per_store_upper_bound = fixtures
        .iter()
        .map(|fixture| fixture.startup.retained_requested_bytes)
        .max()
        .unwrap_or_default();
    let index_work = actions.iter().fold(0_u64, |total, action| {
        total
            .saturating_add(action.work_total.access_key_count)
            .saturating_add(action.work_total.access_candidate_count)
    });
    let dirty_population = actions
        .iter()
        .map(|action| {
            action
                .work_max
                .dirty_state_count
                .saturating_add(action.work_max.dirty_field_count)
        })
        .max()
        .unwrap_or_default();
    let touched_population = actions
        .iter()
        .map(|action| action.work_max.touched_population_count)
        .max()
        .unwrap_or_default();
    let dependency_edge_visits = actions.iter().fold(0_u64, |total, action| {
        total.saturating_add(action.work_total.dependency_fanout_count)
    });
    let recursive_boundary_materializations = actions.iter().fold(0_u64, |total, action| {
        total.saturating_add(action.work_total.recursive_boundary_materialization_count)
    });
    let boundary_materialization_bytes = actions.iter().fold(0_u64, |total, action| {
        total.saturating_add(action.work_total.boundary_materialized_payload_bytes)
    });
    let recursive_clones = actions.iter().fold(0_u64, |total, action| {
        total
            .saturating_add(action.work_total.recursive_value_clone_count)
            .saturating_add(action.work_total.whole_list_snapshot_clone_count)
    });
    let whole_list_snapshot_clones = actions.iter().fold(0_u64, |total, action| {
        total.saturating_add(action.work_total.whole_list_snapshot_clone_count)
    });
    let whole_list_snapshot_comparisons = actions.iter().fold(0_u64, |total, action| {
        total.saturating_add(action.work_total.whole_list_snapshot_comparison_count)
    });
    let runtime_string_lookups = actions.iter().fold(0_u64, |total, action| {
        total.saturating_add(action.work_total.runtime_string_field_lookup_count)
    });
    let tree_container_lookups = actions.iter().fold(0_u64, |total, action| {
        total.saturating_add(action.work_total.tree_container_lookup_count)
    });
    let queue_high_water = actions
        .iter()
        .map(|action| action.work_max.queue_high_water)
        .max()
        .unwrap_or_default();
    let queue_capacity_growth = actions.iter().fold(0_u64, |total, action| {
        total
            .saturating_add(action.work_total.evaluator_task_queue_capacity_growth_count)
            .saturating_add(
                action
                    .work_total
                    .evaluator_value_queue_capacity_growth_count,
            )
            .saturating_add(
                action
                    .work_total
                    .dirty_propagation_queue_capacity_growth_count,
            )
            .saturating_add(
                action
                    .work_total
                    .pending_mutation_queue_capacity_growth_count,
            )
    });
    let delta_items = actions.iter().fold(0_u64, |total, action| {
        total
            .saturating_add(action.work_total.delta_item_count)
            .saturating_add(action.work_total.authority_delta_item_count)
    });
    let delta_bytes = actions.iter().fold(0_u64, |total, action| {
        total
            .saturating_add(action.work_total.delta_logical_byte_count)
            .saturating_add(action.work_total.authority_delta_logical_byte_count)
    });
    let elapsed_ingest_ns = actions.iter().fold(0_u64, |total, action| {
        total.saturating_add(action.work_total.elapsed_ingest_ns)
    });
    let elapsed_evaluate_ns = actions.iter().fold(0_u64, |total, action| {
        total.saturating_add(action.work_total.elapsed_evaluate_ns)
    });
    let elapsed_commit_ns = actions.iter().fold(0_u64, |total, action| {
        total.saturating_add(action.work_total.elapsed_commit_ns)
    });
    let elapsed_delta_ns = actions.iter().fold(0_u64, |total, action| {
        total.saturating_add(action.work_total.elapsed_delta_ns)
    });
    let elapsed_boundary_ns = actions.iter().fold(0_u64, |total, action| {
        total.saturating_add(action.work_total.elapsed_boundary_ns)
    });
    let sparse = actions
        .iter()
        .filter(|action| action.class == ActionClass::Sparse)
        .collect::<Vec<_>>();
    let dense = actions
        .iter()
        .filter(|action| action.class == ActionClass::Dense)
        .collect::<Vec<_>>();
    let sparse_latency = sparse
        .iter()
        .map(|action| action.latency.p95_ns)
        .max()
        .unwrap_or_default();
    let sparse_throughput = sparse
        .iter()
        .map(|action| rate_per_second_milli(action.measured_turns, action.latency.total_ns))
        .min()
        .unwrap_or_default();
    let dense_latency = dense
        .iter()
        .map(|action| action.latency.p95_ns)
        .max()
        .unwrap_or_default();
    let dense_throughput = dense
        .iter()
        .map(|action| {
            rate_per_second_milli(
                action
                    .measured_turns
                    .saturating_mul(action.semantic_rows_per_turn),
                action.latency.total_ns,
            )
        })
        .min()
        .unwrap_or_default();

    BTreeMap::from([
        (
            "allocation-count".to_owned(),
            measured(
                allocation_count,
                MetricUnit::Allocations,
                MetricScope::MeasuredTurns,
                "sum of process-global Rust allocation events across declared actions",
            ),
        ),
        (
            "allocated-bytes".to_owned(),
            measured(
                allocated_bytes,
                MetricUnit::Bytes,
                MetricScope::MeasuredTurns,
                "sum of requested allocation and reallocation bytes across declared actions",
            ),
        ),
        (
            "bytes-per-row".to_owned(),
            measured(
                bytes_per_row,
                MetricUnit::BytesPerRow,
                MetricScope::RetainedRuntime,
                "maximum whole-runtime requested-live delta per authoritative row; not packed store bytes",
            ),
        ),
        (
            "bytes-per-store".to_owned(),
            measured(
                bytes_per_store_upper_bound,
                MetricUnit::Bytes,
                MetricScope::RetainedRuntime,
                "maximum whole-MachineInstance requested-live delta, used as a conservative upper bound for any one legacy store until store-owned allocation domains exist",
            ),
        ),
        (
            "packed-bytes".to_owned(),
            unavailable(
                MetricUnit::Bytes,
                "the current runtime has no packed store/layout ownership",
                "emit deterministic bytes keyed by packed store kind and layout kind",
            ),
        ),
        (
            "arena-live-bytes".to_owned(),
            unavailable(
                MetricUnit::Bytes,
                "the current runtime has no packed live arena",
                "land packed arena ownership and expose live-byte accounting",
            ),
        ),
        (
            "arena-staged-bytes".to_owned(),
            unavailable(
                MetricUnit::Bytes,
                "the current runtime has no packed staged arena",
                "land staged packed handles and expose staged-byte accounting",
            ),
        ),
        (
            "arena-leased-bytes".to_owned(),
            unavailable(
                MetricUnit::Bytes,
                "the current runtime has no packed leased arena",
                "land generational leases and expose leased-byte accounting",
            ),
        ),
        (
            "arena-retired-bytes".to_owned(),
            unavailable(
                MetricUnit::Bytes,
                "the current runtime has no packed retired arena",
                "land retirement and quiescence accounting",
            ),
        ),
        (
            "boundary-materialization-bytes".to_owned(),
            measured(
                boundary_materialization_bytes,
                MetricUnit::Bytes,
                MetricScope::MeasuredTurns,
                "sum of deterministic logical payload bytes exposed by instrumented evaluator and report boundary materializations",
            ),
        ),
        (
            "recursive-boundary-materialization-count".to_owned(),
            measured(
                recursive_boundary_materializations,
                MetricUnit::Items,
                MetricScope::MeasuredTurns,
                "sum of instrumented boundary conversion roots whose value carrier owns recursive children",
            ),
        ),
        (
            "recursive-clone-count".to_owned(),
            measured(
                recursive_clones,
                MetricUnit::Items,
                MetricScope::MeasuredTurns,
                "sum of Value clone roots with recursive carriers plus explicitly instrumented derived-list snapshot clone roots",
            ),
        ),
        (
            "whole-list-snapshot-clone-count".to_owned(),
            measured(
                whole_list_snapshot_clones,
                MetricUnit::Items,
                MetricScope::MeasuredTurns,
                "sum of explicitly instrumented full derived-list snapshot clone roots",
            ),
        ),
        (
            "whole-list-snapshot-comparison-count".to_owned(),
            measured(
                whole_list_snapshot_comparisons,
                MetricUnit::Items,
                MetricScope::MeasuredTurns,
                "sum of explicitly instrumented full derived-list old/new equality comparisons",
            ),
        ),
        (
            "runtime-string-lookup-count".to_owned(),
            measured(
                runtime_string_lookups,
                MetricUnit::Items,
                MetricScope::MeasuredTurns,
                "sum of instrumented get, remove, and contains operations on runtime string-keyed value maps",
            ),
        ),
        (
            "tree-container-lookup-count".to_owned(),
            measured(
                tree_container_lookups,
                MetricUnit::Items,
                MetricScope::MeasuredTurns,
                "exact lower-bound count for the instrumented string-key BTreeMap operations; the exhaustive static site ledger owns remaining tree-container coverage",
            ),
        ),
        (
            "dense-slot-read-count".to_owned(),
            unavailable(
                MetricUnit::Items,
                "the current runtime has no final packed dense slots",
                "instrument packed slot reads when dense stores land",
            ),
        ),
        (
            "dense-slot-write-count".to_owned(),
            unavailable(
                MetricUnit::Items,
                "the current runtime has no final packed dense slots",
                "instrument packed slot writes when dense stores land",
            ),
        ),
        (
            "dirty-population-count".to_owned(),
            measured(
                dirty_population,
                MetricUnit::Items,
                MetricScope::MeasuredTurns,
                "maximum dirty root-state plus dirty row-field population in one measured turn",
            ),
        ),
        (
            "touched-population-count".to_owned(),
            measured(
                touched_population,
                MetricUnit::Items,
                MetricScope::MeasuredTurns,
                "maximum distinct authority root, row-field, and list population touched in one measured work interval",
            ),
        ),
        (
            "dependency-edge-visit-count".to_owned(),
            measured(
                dependency_edge_visits,
                MetricUnit::Items,
                MetricScope::MeasuredTurns,
                "sum of dependency fanout edges charged across declared actions",
            ),
        ),
        (
            "queue-high-water".to_owned(),
            measured(
                queue_high_water,
                MetricUnit::Items,
                MetricScope::MeasuredTurns,
                "maximum pending population across evaluator task/value, dirty-propagation, and mutation queues in one measured turn",
            ),
        ),
        (
            "queue-capacity-growth-count".to_owned(),
            measured(
                queue_capacity_growth,
                MetricUnit::Items,
                MetricScope::MeasuredTurns,
                "sum of observed capacity-growth events across evaluator task/value, dirty-propagation, and mutation queues",
            ),
        ),
        (
            "index-work".to_owned(),
            measured(
                index_work,
                MetricUnit::WorkUnits,
                MetricScope::MeasuredTurns,
                "typed-list keys plus candidates visited",
            ),
        ),
        (
            "index-page-touch-count".to_owned(),
            unavailable(
                MetricUnit::Items,
                "the current ordered indexes do not expose physical page touches",
                "instrument page-oriented index implementations when selected",
            ),
        ),
        (
            "index-segment-touch-count".to_owned(),
            unavailable(
                MetricUnit::Items,
                "the current ordered indexes do not expose packed segment touches",
                "instrument packed-flat index segments when selected",
            ),
        ),
        (
            "delta-item-count".to_owned(),
            measured(
                delta_items,
                MetricUnit::Items,
                MetricScope::MeasuredTurns,
                "sum of final coalesced public and authority delta items across measured turns",
            ),
        ),
        (
            "delta-byte-count".to_owned(),
            measured(
                delta_bytes,
                MetricUnit::Bytes,
                MetricScope::MeasuredTurns,
                "sum of deterministic logical payload bytes in final public and authority delta streams",
            ),
        ),
        (
            "elapsed-ingest-ns".to_owned(),
            measured(
                elapsed_ingest_ns,
                MetricUnit::Nanoseconds,
                MetricScope::MeasuredTurns,
                "sum of executor source-event validation time across measured turns",
            ),
        ),
        (
            "elapsed-evaluate-ns".to_owned(),
            measured(
                elapsed_evaluate_ns,
                MetricUnit::Nanoseconds,
                MetricScope::MeasuredTurns,
                "sum of route, evaluate, currentness, mutation, and reconciliation time across measured turns",
            ),
        ),
        (
            "elapsed-commit-ns".to_owned(),
            measured(
                elapsed_commit_ns,
                MetricUnit::Nanoseconds,
                MetricScope::MeasuredTurns,
                "sum of durable-change, sequence, and transient-effect commit time across measured turns",
            ),
        ),
        (
            "elapsed-delta-ns".to_owned(),
            measured(
                elapsed_delta_ns,
                MetricUnit::Nanoseconds,
                MetricScope::MeasuredTurns,
                "sum of final public and authority delta coalescing/accounting time across measured turns",
            ),
        ),
        (
            "elapsed-boundary-ns".to_owned(),
            measured(
                elapsed_boundary_ns,
                MetricUnit::Nanoseconds,
                MetricScope::MeasuredTurns,
                "sum of complete public root/list read and materialization calls captured by measured actions",
            ),
        ),
        (
            "sparse-latency-p95".to_owned(),
            measured(
                sparse_latency,
                MetricUnit::Nanoseconds,
                MetricScope::MeasuredTurns,
                "maximum nearest-rank p95 across sparse actions",
            ),
        ),
        (
            "sparse-throughput".to_owned(),
            measured(
                sparse_throughput,
                MetricUnit::TurnsPerSecondMilli,
                MetricScope::MeasuredTurns,
                "minimum milli-turns per second across sparse actions",
            ),
        ),
        (
            "dense-latency-p95".to_owned(),
            if dense.is_empty() {
                unavailable(
                    MetricUnit::Nanoseconds,
                    "no dense current-runtime action completed",
                    "add and run a deterministic all-row action",
                )
            } else {
                measured(
                    dense_latency,
                    MetricUnit::Nanoseconds,
                    MetricScope::MeasuredTurns,
                    "maximum nearest-rank p95 across dense actions",
                )
            },
        ),
        (
            "dense-throughput".to_owned(),
            if dense.is_empty() {
                unavailable(
                    MetricUnit::RowsPerSecondMilli,
                    "no dense current-runtime action completed",
                    "add and run a deterministic all-row action",
                )
            } else {
                measured(
                    dense_throughput,
                    MetricUnit::RowsPerSecondMilli,
                    MetricScope::MeasuredTurns,
                    "minimum semantic milli-rows per second across dense actions",
                )
            },
        ),
    ])
}

fn native_product_fact(fixture: &str) -> FactEvidence {
    match fixture {
        "counter" | "cells" => FactEvidence {
            status: FactStatus::Stale,
            detail:
                "an exact report-v2 native product gate exists, but the Phase 0 baseline manifest marks current artifacts stale"
                    .to_owned(),
        },
        "todomvc" => FactEvidence {
            status: FactStatus::NotApplicable,
            detail:
                "the native handoff gate measures todo_mvc_physical, not examples/todomvc.bn"
                    .to_owned(),
        },
        _ => FactEvidence {
            status: FactStatus::NotApplicable,
            detail: "the native handoff manifest has no exact product gate for this fixture".to_owned(),
        },
    }
}

fn latency_summary(samples: &mut [u64], total_ns: u64) -> LatencySummary {
    samples.sort_unstable();
    LatencySummary {
        sample_count: samples.len() as u64,
        total_ns,
        p50_ns: nearest_rank(samples, 50),
        p95_ns: nearest_rank(samples, 95),
        p99_ns: nearest_rank(samples, 99),
        max_ns: samples.last().copied().unwrap_or_default(),
    }
}

fn nearest_rank(samples: &[u64], percentile: usize) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let rank = percentile.saturating_mul(samples.len()).saturating_add(99) / 100;
    samples[rank.saturating_sub(1).min(samples.len() - 1)]
}

fn rate_per_second_milli(items: u64, elapsed_ns: u64) -> u64 {
    if elapsed_ns == 0 {
        return 0;
    }
    let scaled = u128::from(items).saturating_mul(1_000_000_000_000);
    (scaled / u128::from(elapsed_ns))
        .try_into()
        .unwrap_or(u64::MAX)
}

fn measured(value: u64, unit: MetricUnit, scope: MetricScope, coverage: &str) -> MetricEvidence {
    MetricEvidence::Measured {
        value,
        unit,
        scope,
        coverage: coverage.to_owned(),
    }
}

fn unavailable(unit: MetricUnit, reason: &str, required_change: &str) -> MetricEvidence {
    MetricEvidence::Unavailable {
        unit,
        reason: reason.to_owned(),
        required_change: required_change.to_owned(),
    }
}

struct CanonicalCompilerInput {
    entrypoint: String,
    units: Vec<CompilerSourceUnit>,
    source_bundle_digest_v1: String,
}

fn canonical_compiler_units(
    workspace: &Path,
    entrypoint: &str,
    discovered_units: Vec<CompilerSourceUnit>,
) -> Result<CanonicalCompilerInput, String> {
    let relative_units = discovered_units
        .into_iter()
        .map(|unit| {
            let path = Path::new(&unit.path);
            let relative = if path.is_absolute() {
                path.strip_prefix(workspace).map_err(|_| {
                    format!(
                        "discovered compiler unit {} is outside workspace {}",
                        path.display(),
                        workspace.display()
                    )
                })?
            } else {
                path
            };
            Ok(CompilerSourceUnit {
                path: relative.display().to_string(),
                source: unit.source,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let canonical = CanonicalSourceBundleV1::new(
        entrypoint,
        relative_units
            .iter()
            .map(|unit| SourceBundleUnit::new(&unit.path, &unit.source)),
    )
    .map_err(|error| error.to_string())?;
    let source_bundle_digest_v1 = canonical.digest().to_string();
    let entrypoint = canonical.entrypoint().to_owned();
    let units = canonical
        .units()
        .iter()
        .map(|unit| CompilerSourceUnit {
            path: unit.path().to_owned(),
            source: unit.source().to_owned(),
        })
        .collect();
    Ok(CanonicalCompilerInput {
        entrypoint,
        units,
        source_bundle_digest_v1,
    })
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(sha256_bytes(&bytes))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_digest(hasher.finalize().as_slice())
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn write_report(path: &Path, report: &BaselineReport) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
    if bytes.is_empty() || bytes.len() > MAX_REPORT_BYTES {
        return Err(format!(
            "packed baseline report is {} bytes; expected 1..={MAX_REPORT_BYTES}",
            bytes.len()
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| format!("report path {} has no parent", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn relative_display(workspace: &Path, path: &Path) -> Result<String, String> {
    path.strip_prefix(workspace)
        .map(|relative| relative.display().to_string())
        .map_err(|_| {
            format!(
                "{} is outside workspace {}",
                path.display(),
                workspace.display()
            )
        })
}

fn duration_ns(duration: std::time::Duration) -> u64 {
    duration.as_nanos().try_into().unwrap_or(u64::MAX)
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn to_u64(value: usize) -> u64 {
    value.try_into().unwrap_or(u64::MAX)
}

impl From<&TurnMetrics> for WorkEvidence {
    fn from(metrics: &TurnMetrics) -> Self {
        Self {
            dirty_state_count: to_u64(metrics.dirty_state_count),
            dirty_field_count: to_u64(metrics.dirty_field_count),
            recomputed_field_count: to_u64(metrics.recomputed_field_count),
            recomputed_list_count: to_u64(metrics.recomputed_list_count),
            changed_row_count: to_u64(metrics.changed_row_count),
            touched_root_state_count: metrics.touched_root_state_count,
            touched_list_count: metrics.touched_list_count,
            touched_row_field_count: metrics.touched_row_field_count,
            touched_population_count: metrics.touched_population_count,
            dependency_fanout_count: to_u64(metrics.dependency_fanout_count),
            indexed_access_count: to_u64(metrics.indexed_access_count),
            index_candidate_count: to_u64(metrics.index_candidate_count),
            list_find_scan_count: to_u64(metrics.list_find_scan_count),
            access_index_seek_count: metrics.access_index_seek_count,
            access_cursor_seek_count: metrics.access_cursor_seek_count,
            access_key_count: metrics.access_key_count,
            access_candidate_count: metrics.access_candidate_count,
            access_result_count: metrics.access_result_count,
            access_full_scan_count: metrics.access_full_scan_count,
            ordered_index_full_rebuild_count: metrics.ordered_index_full_rebuild_count,
            ordered_index_rebuild_entry_count: metrics.ordered_index_rebuild_entry_count,
            ordered_index_current_entry_count: metrics.ordered_index_current_entry_count,
            ordered_index_current_payload_bytes: metrics.ordered_index_current_payload_bytes,
            value_clone_count: metrics.value_clone_count,
            recursive_value_clone_count: metrics.recursive_value_clone_count,
            recursive_value_clone_value_count: metrics.recursive_value_clone_value_count,
            boundary_materialization_count: metrics.boundary_materialization_count,
            recursive_boundary_materialization_count: metrics
                .recursive_boundary_materialization_count,
            boundary_materialized_value_count: metrics.boundary_materialized_value_count,
            boundary_materialized_payload_bytes: metrics.boundary_materialized_payload_bytes,
            runtime_string_field_lookup_count: metrics.runtime_string_field_lookup_count,
            tree_container_lookup_count: metrics.tree_container_lookup_count,
            whole_list_snapshot_clone_count: metrics.whole_list_snapshot_clone_count,
            whole_list_snapshot_cloned_item_count: metrics.whole_list_snapshot_cloned_item_count,
            whole_list_snapshot_comparison_count: metrics.whole_list_snapshot_comparison_count,
            whole_list_snapshot_comparison_input_item_count: metrics
                .whole_list_snapshot_comparison_input_item_count,
            evaluator_task_queue_high_water: metrics.evaluator_task_queue_high_water,
            evaluator_task_queue_push_count: metrics.evaluator_task_queue_push_count,
            evaluator_task_queue_capacity_growth_count: metrics
                .evaluator_task_queue_capacity_growth_count,
            evaluator_task_queue_capacity_growth_items: metrics
                .evaluator_task_queue_capacity_growth_items,
            evaluator_value_queue_high_water: metrics.evaluator_value_queue_high_water,
            evaluator_value_queue_push_count: metrics.evaluator_value_queue_push_count,
            evaluator_value_queue_capacity_growth_count: metrics
                .evaluator_value_queue_capacity_growth_count,
            evaluator_value_queue_capacity_growth_items: metrics
                .evaluator_value_queue_capacity_growth_items,
            dirty_propagation_queue_high_water: metrics.dirty_propagation_queue_high_water,
            dirty_propagation_queue_push_count: metrics.dirty_propagation_queue_push_count,
            dirty_propagation_queue_capacity_growth_count: metrics
                .dirty_propagation_queue_capacity_growth_count,
            dirty_propagation_queue_capacity_growth_items: metrics
                .dirty_propagation_queue_capacity_growth_items,
            pending_mutation_queue_high_water: metrics.pending_mutation_queue_high_water,
            pending_mutation_queue_push_count: metrics.pending_mutation_queue_push_count,
            pending_mutation_queue_capacity_growth_count: metrics
                .pending_mutation_queue_capacity_growth_count,
            pending_mutation_queue_capacity_growth_items: metrics
                .pending_mutation_queue_capacity_growth_items,
            queue_high_water: metrics.queue_high_water,
            delta_item_count: metrics.delta_item_count,
            delta_logical_byte_count: metrics.delta_logical_byte_count,
            authority_delta_item_count: metrics.authority_delta_item_count,
            authority_delta_logical_byte_count: metrics.authority_delta_logical_byte_count,
            elapsed_ingest_ns: metrics.elapsed_ingest_ns,
            elapsed_evaluate_ns: metrics.elapsed_evaluate_ns,
            elapsed_commit_ns: metrics.elapsed_commit_ns,
            elapsed_delta_ns: metrics.elapsed_delta_ns,
            elapsed_boundary_ns: metrics.elapsed_boundary_ns,
            work_unit_count: metrics.work_unit_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct GoldenFixture {
        schema: String,
        entrypoint: String,
        units: Vec<GoldenUnit>,
        canonical_entrypoint: String,
        canonical_paths: Vec<String>,
        digest: String,
    }

    #[derive(Deserialize)]
    struct GoldenUnit {
        path: String,
        source: String,
    }

    #[test]
    fn baseline_uses_shared_source_bundle_digest_v1_golden() {
        let fixture: GoldenFixture = serde_json::from_str(include_str!(
            "../../../fixtures/contracts/source_bundle_digest_v1.json"
        ))
        .unwrap();
        assert_eq!(fixture.schema, "boon.source-bundle-golden.v1");
        let canonical = CanonicalSourceBundleV1::new(
            &fixture.entrypoint,
            fixture
                .units
                .iter()
                .map(|unit| SourceBundleUnit::new(&unit.path, &unit.source)),
        )
        .unwrap();
        assert_eq!(canonical.entrypoint(), fixture.canonical_entrypoint);
        assert_eq!(
            canonical
                .units()
                .iter()
                .map(|unit| unit.path())
                .collect::<Vec<_>>(),
            fixture
                .canonical_paths
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        );
        assert_eq!(canonical.digest().to_string(), fixture.digest);
    }

    #[test]
    fn baseline_source_bundle_identity_normalizes_slashes_and_unit_order() {
        let forward = canonical_compiler_units(
            Path::new("."),
            "app/main.bn",
            vec![
                CompilerSourceUnit {
                    path: "app/main.bn".to_owned(),
                    source: "value: helper.value\n".to_owned(),
                },
                CompilerSourceUnit {
                    path: "app/helper.bn".to_owned(),
                    source: "helper: [value: 42]\n".to_owned(),
                },
            ],
        )
        .unwrap();
        let reversed_windows = canonical_compiler_units(
            Path::new("."),
            "app\\main.bn",
            vec![
                CompilerSourceUnit {
                    path: "app\\helper.bn".to_owned(),
                    source: "helper: [value: 42]\n".to_owned(),
                },
                CompilerSourceUnit {
                    path: "app\\main.bn".to_owned(),
                    source: "value: helper.value\n".to_owned(),
                },
            ],
        )
        .unwrap();
        assert_eq!(forward.entrypoint, "app/main.bn");
        assert_eq!(reversed_windows.entrypoint, "app/main.bn");
        assert_eq!(
            forward.source_bundle_digest_v1,
            reversed_windows.source_bundle_digest_v1
        );
        assert_eq!(
            forward
                .units
                .iter()
                .map(|unit| unit.path.as_str())
                .collect::<Vec<_>>(),
            ["app/helper.bn", "app/main.bn"]
        );
    }
}
