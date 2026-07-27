use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const FORMAT_VERSION: u16 = 2;
pub const REPORT_KIND: &str = "phase0-current-runtime-baseline";
pub const PROTOCOL: &str = "boon-phase0-current-runtime-v1";
pub const DEFAULT_MANIFEST: &str = "docs/architecture/phase0/packed_baseline_fixtures.toml";
pub const DEFAULT_BUDGET: &str = "budgets/packed-data.toml";
pub const DEFAULT_REPORT: &str = "target/reports/phase0-v1/packed-baseline.json";
pub const MAX_REPORT_BYTES: usize = 4 * 1024 * 1024;
pub const REQUIRED_FIXTURES: &[&str] = &[
    "counter",
    "todomvc",
    "cells",
    "fjordpulse-shaped",
    "million-row",
];
pub const REQUIRED_METRICS: &[&str] = &[
    "allocation-count",
    "allocated-bytes",
    "bytes-per-row",
    "bytes-per-store",
    "packed-bytes",
    "arena-live-bytes",
    "arena-staged-bytes",
    "arena-leased-bytes",
    "arena-retired-bytes",
    "recursive-boundary-materialization-count",
    "boundary-materialization-bytes",
    "recursive-clone-count",
    "whole-list-snapshot-clone-count",
    "whole-list-snapshot-comparison-count",
    "runtime-string-lookup-count",
    "tree-container-lookup-count",
    "dense-slot-read-count",
    "dense-slot-write-count",
    "dirty-population-count",
    "touched-population-count",
    "dependency-edge-visit-count",
    "queue-high-water",
    "queue-capacity-growth-count",
    "index-work",
    "index-page-touch-count",
    "index-segment-touch-count",
    "delta-item-count",
    "delta-byte-count",
    "elapsed-ingest-ns",
    "elapsed-evaluate-ns",
    "elapsed-commit-ns",
    "elapsed-delta-ns",
    "elapsed-boundary-ns",
    "sparse-latency-p95",
    "sparse-throughput",
    "dense-latency-p95",
    "dense-throughput",
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportStatus {
    Pass,
    Fail,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionClass {
    Sparse,
    Dense,
    Query,
    Currentness,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetricUnit {
    Allocations,
    Bytes,
    BytesPerRow,
    Items,
    Nanoseconds,
    RowsPerSecondMilli,
    TurnsPerSecondMilli,
    WorkUnits,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetricScope {
    CurrentRuntime,
    RetainedRuntime,
    MeasuredTurns,
    RuntimeStartup,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, tag = "status", rename_all = "kebab-case")]
pub enum MetricEvidence {
    Measured {
        value: u64,
        unit: MetricUnit,
        scope: MetricScope,
        coverage: String,
    },
    Unavailable {
        unit: MetricUnit,
        reason: String,
        required_change: String,
    },
    NotApplicable {
        unit: MetricUnit,
        reason: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FactStatus {
    Measured,
    Unavailable,
    NotApplicable,
    Stale,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FactEvidence {
    pub status: FactStatus,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    pub head: String,
    pub workspace_digest: String,
    pub dirty: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticClaim {
    pub representation: String,
    pub packed_semantics: bool,
    pub exact_number: bool,
    pub bounded_bits: bool,
    pub final_closed_tags: bool,
}

impl Default for SemanticClaim {
    fn default() -> Self {
        Self {
            representation: "legacy-current-runtime".to_owned(),
            packed_semantics: false,
            exact_number: false,
            bounded_bits: false,
            final_closed_tags: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AllocatorEvidence {
    pub allocation_calls: u64,
    pub zeroed_allocation_calls: u64,
    pub reallocation_calls: u64,
    pub deallocation_calls: u64,
    pub requested_allocation_bytes: u64,
    pub requested_reallocation_bytes: u64,
    pub requested_freed_bytes: u64,
    pub live_requested_bytes_start: u64,
    pub live_requested_bytes_end: u64,
    pub peak_live_requested_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkEvidence {
    pub dirty_state_count: u64,
    pub dirty_field_count: u64,
    pub recomputed_field_count: u64,
    pub recomputed_list_count: u64,
    pub changed_row_count: u64,
    pub touched_root_state_count: u64,
    pub touched_list_count: u64,
    pub touched_row_field_count: u64,
    pub touched_population_count: u64,
    pub dependency_fanout_count: u64,
    pub indexed_access_count: u64,
    pub index_candidate_count: u64,
    pub list_find_scan_count: u64,
    pub access_index_seek_count: u64,
    pub access_cursor_seek_count: u64,
    pub access_key_count: u64,
    pub access_candidate_count: u64,
    pub access_result_count: u64,
    pub access_full_scan_count: u64,
    pub ordered_index_full_rebuild_count: u64,
    pub ordered_index_rebuild_entry_count: u64,
    pub ordered_index_current_entry_count: u64,
    pub ordered_index_current_payload_bytes: u64,
    pub value_clone_count: u64,
    pub recursive_value_clone_count: u64,
    pub recursive_value_clone_value_count: u64,
    pub boundary_materialization_count: u64,
    pub recursive_boundary_materialization_count: u64,
    pub boundary_materialized_value_count: u64,
    pub boundary_materialized_payload_bytes: u64,
    pub runtime_string_field_lookup_count: u64,
    pub tree_container_lookup_count: u64,
    pub whole_list_snapshot_clone_count: u64,
    pub whole_list_snapshot_cloned_item_count: u64,
    pub whole_list_snapshot_comparison_count: u64,
    pub whole_list_snapshot_comparison_input_item_count: u64,
    pub evaluator_task_queue_high_water: u64,
    pub evaluator_task_queue_push_count: u64,
    pub evaluator_task_queue_capacity_growth_count: u64,
    pub evaluator_task_queue_capacity_growth_items: u64,
    pub evaluator_value_queue_high_water: u64,
    pub evaluator_value_queue_push_count: u64,
    pub evaluator_value_queue_capacity_growth_count: u64,
    pub evaluator_value_queue_capacity_growth_items: u64,
    pub dirty_propagation_queue_high_water: u64,
    pub dirty_propagation_queue_push_count: u64,
    pub dirty_propagation_queue_capacity_growth_count: u64,
    pub dirty_propagation_queue_capacity_growth_items: u64,
    pub pending_mutation_queue_high_water: u64,
    pub pending_mutation_queue_push_count: u64,
    pub pending_mutation_queue_capacity_growth_count: u64,
    pub pending_mutation_queue_capacity_growth_items: u64,
    pub queue_high_water: u64,
    pub delta_item_count: u64,
    pub delta_logical_byte_count: u64,
    pub authority_delta_item_count: u64,
    pub authority_delta_logical_byte_count: u64,
    pub elapsed_ingest_ns: u64,
    pub elapsed_evaluate_ns: u64,
    pub elapsed_commit_ns: u64,
    pub elapsed_delta_ns: u64,
    pub elapsed_boundary_ns: u64,
    pub work_unit_count: u64,
}

impl WorkEvidence {
    pub fn saturating_add(self, other: Self) -> Self {
        self.combine(other, u64::saturating_add)
    }

    pub fn component_max(self, other: Self) -> Self {
        self.combine(other, u64::max)
    }

    fn combine(self, other: Self, combine: impl Fn(u64, u64) -> u64) -> Self {
        Self {
            dirty_state_count: combine(self.dirty_state_count, other.dirty_state_count),
            dirty_field_count: combine(self.dirty_field_count, other.dirty_field_count),
            recomputed_field_count: combine(
                self.recomputed_field_count,
                other.recomputed_field_count,
            ),
            recomputed_list_count: combine(self.recomputed_list_count, other.recomputed_list_count),
            changed_row_count: combine(self.changed_row_count, other.changed_row_count),
            touched_root_state_count: combine(
                self.touched_root_state_count,
                other.touched_root_state_count,
            ),
            touched_list_count: combine(self.touched_list_count, other.touched_list_count),
            touched_row_field_count: combine(
                self.touched_row_field_count,
                other.touched_row_field_count,
            ),
            touched_population_count: combine(
                self.touched_population_count,
                other.touched_population_count,
            ),
            dependency_fanout_count: combine(
                self.dependency_fanout_count,
                other.dependency_fanout_count,
            ),
            indexed_access_count: combine(self.indexed_access_count, other.indexed_access_count),
            index_candidate_count: combine(self.index_candidate_count, other.index_candidate_count),
            list_find_scan_count: combine(self.list_find_scan_count, other.list_find_scan_count),
            access_index_seek_count: combine(
                self.access_index_seek_count,
                other.access_index_seek_count,
            ),
            access_cursor_seek_count: combine(
                self.access_cursor_seek_count,
                other.access_cursor_seek_count,
            ),
            access_key_count: combine(self.access_key_count, other.access_key_count),
            access_candidate_count: combine(
                self.access_candidate_count,
                other.access_candidate_count,
            ),
            access_result_count: combine(self.access_result_count, other.access_result_count),
            access_full_scan_count: combine(
                self.access_full_scan_count,
                other.access_full_scan_count,
            ),
            ordered_index_full_rebuild_count: combine(
                self.ordered_index_full_rebuild_count,
                other.ordered_index_full_rebuild_count,
            ),
            ordered_index_rebuild_entry_count: combine(
                self.ordered_index_rebuild_entry_count,
                other.ordered_index_rebuild_entry_count,
            ),
            ordered_index_current_entry_count: combine(
                self.ordered_index_current_entry_count,
                other.ordered_index_current_entry_count,
            ),
            ordered_index_current_payload_bytes: combine(
                self.ordered_index_current_payload_bytes,
                other.ordered_index_current_payload_bytes,
            ),
            value_clone_count: combine(self.value_clone_count, other.value_clone_count),
            recursive_value_clone_count: combine(
                self.recursive_value_clone_count,
                other.recursive_value_clone_count,
            ),
            recursive_value_clone_value_count: combine(
                self.recursive_value_clone_value_count,
                other.recursive_value_clone_value_count,
            ),
            boundary_materialization_count: combine(
                self.boundary_materialization_count,
                other.boundary_materialization_count,
            ),
            recursive_boundary_materialization_count: combine(
                self.recursive_boundary_materialization_count,
                other.recursive_boundary_materialization_count,
            ),
            boundary_materialized_value_count: combine(
                self.boundary_materialized_value_count,
                other.boundary_materialized_value_count,
            ),
            boundary_materialized_payload_bytes: combine(
                self.boundary_materialized_payload_bytes,
                other.boundary_materialized_payload_bytes,
            ),
            runtime_string_field_lookup_count: combine(
                self.runtime_string_field_lookup_count,
                other.runtime_string_field_lookup_count,
            ),
            tree_container_lookup_count: combine(
                self.tree_container_lookup_count,
                other.tree_container_lookup_count,
            ),
            whole_list_snapshot_clone_count: combine(
                self.whole_list_snapshot_clone_count,
                other.whole_list_snapshot_clone_count,
            ),
            whole_list_snapshot_cloned_item_count: combine(
                self.whole_list_snapshot_cloned_item_count,
                other.whole_list_snapshot_cloned_item_count,
            ),
            whole_list_snapshot_comparison_count: combine(
                self.whole_list_snapshot_comparison_count,
                other.whole_list_snapshot_comparison_count,
            ),
            whole_list_snapshot_comparison_input_item_count: combine(
                self.whole_list_snapshot_comparison_input_item_count,
                other.whole_list_snapshot_comparison_input_item_count,
            ),
            evaluator_task_queue_high_water: combine(
                self.evaluator_task_queue_high_water,
                other.evaluator_task_queue_high_water,
            ),
            evaluator_task_queue_push_count: combine(
                self.evaluator_task_queue_push_count,
                other.evaluator_task_queue_push_count,
            ),
            evaluator_task_queue_capacity_growth_count: combine(
                self.evaluator_task_queue_capacity_growth_count,
                other.evaluator_task_queue_capacity_growth_count,
            ),
            evaluator_task_queue_capacity_growth_items: combine(
                self.evaluator_task_queue_capacity_growth_items,
                other.evaluator_task_queue_capacity_growth_items,
            ),
            evaluator_value_queue_high_water: combine(
                self.evaluator_value_queue_high_water,
                other.evaluator_value_queue_high_water,
            ),
            evaluator_value_queue_push_count: combine(
                self.evaluator_value_queue_push_count,
                other.evaluator_value_queue_push_count,
            ),
            evaluator_value_queue_capacity_growth_count: combine(
                self.evaluator_value_queue_capacity_growth_count,
                other.evaluator_value_queue_capacity_growth_count,
            ),
            evaluator_value_queue_capacity_growth_items: combine(
                self.evaluator_value_queue_capacity_growth_items,
                other.evaluator_value_queue_capacity_growth_items,
            ),
            dirty_propagation_queue_high_water: combine(
                self.dirty_propagation_queue_high_water,
                other.dirty_propagation_queue_high_water,
            ),
            dirty_propagation_queue_push_count: combine(
                self.dirty_propagation_queue_push_count,
                other.dirty_propagation_queue_push_count,
            ),
            dirty_propagation_queue_capacity_growth_count: combine(
                self.dirty_propagation_queue_capacity_growth_count,
                other.dirty_propagation_queue_capacity_growth_count,
            ),
            dirty_propagation_queue_capacity_growth_items: combine(
                self.dirty_propagation_queue_capacity_growth_items,
                other.dirty_propagation_queue_capacity_growth_items,
            ),
            pending_mutation_queue_high_water: combine(
                self.pending_mutation_queue_high_water,
                other.pending_mutation_queue_high_water,
            ),
            pending_mutation_queue_push_count: combine(
                self.pending_mutation_queue_push_count,
                other.pending_mutation_queue_push_count,
            ),
            pending_mutation_queue_capacity_growth_count: combine(
                self.pending_mutation_queue_capacity_growth_count,
                other.pending_mutation_queue_capacity_growth_count,
            ),
            pending_mutation_queue_capacity_growth_items: combine(
                self.pending_mutation_queue_capacity_growth_items,
                other.pending_mutation_queue_capacity_growth_items,
            ),
            queue_high_water: combine(self.queue_high_water, other.queue_high_water),
            delta_item_count: combine(self.delta_item_count, other.delta_item_count),
            delta_logical_byte_count: combine(
                self.delta_logical_byte_count,
                other.delta_logical_byte_count,
            ),
            authority_delta_item_count: combine(
                self.authority_delta_item_count,
                other.authority_delta_item_count,
            ),
            authority_delta_logical_byte_count: combine(
                self.authority_delta_logical_byte_count,
                other.authority_delta_logical_byte_count,
            ),
            elapsed_ingest_ns: combine(self.elapsed_ingest_ns, other.elapsed_ingest_ns),
            elapsed_evaluate_ns: combine(self.elapsed_evaluate_ns, other.elapsed_evaluate_ns),
            elapsed_commit_ns: combine(self.elapsed_commit_ns, other.elapsed_commit_ns),
            elapsed_delta_ns: combine(self.elapsed_delta_ns, other.elapsed_delta_ns),
            elapsed_boundary_ns: combine(self.elapsed_boundary_ns, other.elapsed_boundary_ns),
            work_unit_count: combine(self.work_unit_count, other.work_unit_count),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LatencySummary {
    pub sample_count: u64,
    pub total_ns: u64,
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub max_ns: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StartupEvidence {
    pub elapsed_ns: u64,
    pub allocator: AllocatorEvidence,
    pub work: WorkEvidence,
    pub retained_requested_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActionReport {
    pub id: String,
    pub class: ActionClass,
    pub target: String,
    pub warmup_turns: u64,
    pub measured_turns: u64,
    pub allocator: AllocatorEvidence,
    pub latency: LatencySummary,
    pub work_total: WorkEvidence,
    pub work_max: WorkEvidence,
    pub semantic_rows_per_turn: u64,
    pub metrics: BTreeMap<String, MetricEvidence>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureReport {
    pub id: String,
    pub source_path: String,
    pub source_bundle_digest_v1: String,
    pub expected_authoritative_rows: u64,
    pub observed_logical_rows: u64,
    pub semantic_claim: SemanticClaim,
    pub compilation_elapsed_ns: u64,
    pub compilation_allocator: AllocatorEvidence,
    pub startup: StartupEvidence,
    pub actions: Vec<ActionReport>,
    pub target_facts: BTreeMap<String, FactEvidence>,
    pub metrics: BTreeMap<String, MetricEvidence>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackedStoreLayoutBytes {
    pub store_kind: String,
    pub layout_kind: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineReport {
    pub format: u16,
    pub kind: String,
    pub protocol: String,
    pub generated_unix_ms: u64,
    pub status: ReportStatus,
    pub source: SourceIdentity,
    pub binary_sha256: String,
    pub fixture_manifest_path: String,
    pub fixture_manifest_sha256: String,
    pub budget_manifest_path: String,
    pub budget_manifest_sha256: String,
    pub target_profile: String,
    pub build_profile: String,
    pub allocator_scope: String,
    pub host_arch: String,
    pub host_os: String,
    pub packed_bytes_by_store_layout: Vec<PackedStoreLayoutBytes>,
    pub fixtures: Vec<FixtureReport>,
    pub metrics: BTreeMap<String, MetricEvidence>,
}

impl BaselineReport {
    pub fn validate(&self) -> Result<(), String> {
        if self.format != FORMAT_VERSION {
            return Err(format!(
                "packed baseline report format {} is not {FORMAT_VERSION}",
                self.format
            ));
        }
        require_exact(&self.kind, REPORT_KIND, "report kind")?;
        require_exact(&self.protocol, PROTOCOL, "report protocol")?;
        if self.status != ReportStatus::Pass {
            return Err("packed baseline report status is not pass".to_owned());
        }
        require_commit(&self.source.head, "source head")?;
        require_digest(&self.source.workspace_digest, "source workspace digest")?;
        require_digest(&self.binary_sha256, "producer binary digest")?;
        require_digest(&self.fixture_manifest_sha256, "fixture manifest digest")?;
        require_bounded_nonempty(&self.budget_manifest_path, 240, "budget manifest path")?;
        require_digest(&self.budget_manifest_sha256, "budget manifest digest")?;
        require_exact(&self.target_profile, "software_default", "target profile")?;
        require_exact(&self.build_profile, "release", "build profile")?;
        require_bounded_nonempty(&self.fixture_manifest_path, 240, "fixture manifest path")?;
        require_bounded_nonempty(&self.allocator_scope, 256, "allocator scope")?;
        require_bounded_nonempty(&self.host_arch, 64, "host architecture")?;
        require_bounded_nonempty(&self.host_os, 64, "host operating system")?;
        if !self.packed_bytes_by_store_layout.is_empty() {
            return Err(
                "current-runtime baseline must not claim packed store/layout byte ownership"
                    .to_owned(),
            );
        }

        validate_exact_ids(
            self.fixtures.iter().map(|fixture| fixture.id.as_str()),
            REQUIRED_FIXTURES,
            "fixture",
        )?;
        validate_exact_ids(
            self.metrics.keys().map(String::as_str),
            REQUIRED_METRICS,
            "aggregate metric",
        )?;
        for metric in self.metrics.values() {
            validate_metric(metric)?;
        }
        for fixture in &self.fixtures {
            fixture.validate()?;
        }
        Ok(())
    }
}

impl FixtureReport {
    fn validate(&self) -> Result<(), String> {
        require_bounded_nonempty(&self.id, 96, "fixture id")?;
        require_bounded_nonempty(&self.source_path, 240, "fixture source path")?;
        require_digest(
            &self.source_bundle_digest_v1,
            "fixture SourceBundleDigestV1",
        )?;
        if self.observed_logical_rows < self.expected_authoritative_rows {
            return Err(format!(
                "fixture {} observed {} logical rows below authoritative minimum {}",
                self.id, self.observed_logical_rows, self.expected_authoritative_rows
            ));
        }
        if self.semantic_claim.packed_semantics
            || self.semantic_claim.exact_number
            || self.semantic_claim.bounded_bits
            || self.semantic_claim.final_closed_tags
        {
            return Err(format!(
                "fixture {} falsely claims final packed/foundation semantics",
                self.id
            ));
        }
        require_exact(
            &self.semantic_claim.representation,
            "legacy-current-runtime",
            "semantic representation",
        )?;
        if self.actions.is_empty() || self.actions.len() > 16 {
            return Err(format!(
                "fixture {} must contain 1..=16 measured actions",
                self.id
            ));
        }
        let mut action_ids = BTreeSet::new();
        for action in &self.actions {
            if !action_ids.insert(action.id.as_str()) {
                return Err(format!("fixture {} repeats action {}", self.id, action.id));
            }
            action.validate(&self.id)?;
        }
        validate_exact_ids(
            self.target_facts.keys().map(String::as_str),
            &["native-headless", "native-product", "wasm-browser"],
            "target fact",
        )?;
        for (id, fact) in &self.target_facts {
            require_bounded_nonempty(id, 96, "target fact id")?;
            require_bounded_nonempty(&fact.detail, 1024, "target fact detail")?;
        }
        validate_exact_ids(
            self.metrics.keys().map(String::as_str),
            &[
                "bytes-per-row",
                "logical-index-payload-bytes",
                "retained-runtime-requested-bytes",
            ],
            "fixture metric",
        )?;
        for (id, metric) in &self.metrics {
            require_bounded_nonempty(id, 96, "fixture metric id")?;
            validate_metric(metric)?;
        }
        Ok(())
    }
}

impl ActionReport {
    fn validate(&self, fixture: &str) -> Result<(), String> {
        require_bounded_nonempty(&self.id, 96, "action id")?;
        require_bounded_nonempty(&self.target, 240, "action target")?;
        if self.measured_turns == 0 || self.measured_turns > 10_000 {
            return Err(format!(
                "fixture {fixture} action {} has invalid measured turn count {}",
                self.id, self.measured_turns
            ));
        }
        if self.latency.sample_count != self.measured_turns {
            return Err(format!(
                "fixture {fixture} action {} has {} latency samples for {} measured turns",
                self.id, self.latency.sample_count, self.measured_turns
            ));
        }
        if self.latency.p50_ns > self.latency.p95_ns
            || self.latency.p95_ns > self.latency.p99_ns
            || self.latency.p99_ns > self.latency.max_ns
        {
            return Err(format!(
                "fixture {fixture} action {} has non-monotonic latency percentiles",
                self.id
            ));
        }
        validate_exact_ids(
            self.metrics.keys().map(String::as_str),
            &[
                "allocation-count",
                "allocated-bytes",
                "charged-work",
                "index-work",
                "latency-p95",
                "rows-per-second",
                "turns-per-second",
            ],
            "action metric",
        )?;
        for (id, metric) in &self.metrics {
            require_bounded_nonempty(id, 96, "action metric id")?;
            validate_metric(metric)?;
        }
        Ok(())
    }
}

fn validate_metric(metric: &MetricEvidence) -> Result<(), String> {
    match metric {
        MetricEvidence::Measured { coverage, .. } => {
            require_bounded_nonempty(coverage, 256, "metric coverage")
        }
        MetricEvidence::Unavailable {
            reason,
            required_change,
            ..
        } => {
            require_bounded_nonempty(reason, 1024, "unavailable metric reason")?;
            require_bounded_nonempty(required_change, 1024, "unavailable metric required change")
        }
        MetricEvidence::NotApplicable { reason, .. } => {
            require_bounded_nonempty(reason, 1024, "not-applicable metric reason")
        }
    }
}

fn validate_exact_ids<'a>(
    actual: impl Iterator<Item = &'a str>,
    expected: &[&str],
    label: &str,
) -> Result<(), String> {
    let actual = actual.collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "{label} IDs differ: actual={actual:?}, expected={expected:?}"
        ))
    }
}

fn require_exact(actual: &str, expected: &str, label: &str) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{label} is `{actual}`; expected `{expected}`"))
    }
}

fn require_commit(value: &str, label: &str) -> Result<(), String> {
    if value.len() == 40 && value.bytes().all(is_lower_hex) {
        Ok(())
    } else {
        Err(format!("{label} is not a lowercase SHA-1 commit"))
    }
}

fn require_digest(value: &str, label: &str) -> Result<(), String> {
    if value.len() == 64 && value.bytes().all(is_lower_hex) {
        Ok(())
    } else {
        Err(format!("{label} is not a lowercase SHA-256 digest"))
    }
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn require_bounded_nonempty(value: &str, maximum: usize, label: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > maximum {
        Err(format!(
            "{label} length {} is outside 1..={maximum}",
            value.len()
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn report_identity_rejects_unknown_fields() {
        let error = serde_json::from_value::<SourceIdentity>(json!({
            "head": "4a820727d339038826a9d589c207ef5f973dad83",
            "workspace_digest": "00".repeat(32),
            "dirty": true,
            "legacy_digest": "not accepted"
        }))
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }
}
