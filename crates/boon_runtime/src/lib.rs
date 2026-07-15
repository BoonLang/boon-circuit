use boon_compiler::{
    CompileProfile, CompiledMachinePlanFromSource, CompilerSourceUnit,
    compile_runtime_source_text_to_machine_plan_with_identity,
    compile_runtime_source_units_to_machine_plan_with_identity, compiler_source_text_for_path,
    compiler_source_units_for_manifest_source, compiler_source_units_for_path,
};
pub use boon_document_model::{DocumentFrame, DocumentPatch, ProgramCapabilityProfile};
use boon_example_manifest::ExampleManifest;
pub use boon_example_manifest::{
    ExampleEntry as ExampleManifestEntry, MigrationScenario, MigrationSequence,
};
pub use boon_persistence::{DurableChange, RestoreImage};
pub use boon_plan::{ApplicationIdentity, MachinePlan};
use boon_plan::{MigrationEdgeId, OutputContractKind, OutputRootPlan, SourceId, TargetProfile};
pub use boon_plan_executor::{
    AuthorityDelta, Delta, RowId, RowSnapshot, SessionOptions, Snapshot, SourceEvent,
    SourcePayload, TurnMetrics, Value, ValueTarget,
};
use boon_plan_executor::{Session, SessionBuilder, Turn};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

mod document;
mod program;

#[cfg(not(target_arch = "wasm32"))]
mod persistent;

#[cfg(target_arch = "wasm32")]
mod web_persistent;

#[cfg(not(target_arch = "wasm32"))]
mod effects;

#[cfg(not(target_arch = "wasm32"))]
mod migration_scenario;

pub use document::{DocumentMaterializationStats, DocumentWindowDemand};
#[cfg(not(target_arch = "wasm32"))]
pub use effects::*;
#[cfg(not(target_arch = "wasm32"))]
pub use migration_scenario::*;
#[cfg(not(target_arch = "wasm32"))]
pub use persistent::*;
pub use program::*;
#[cfg(target_arch = "wasm32")]
pub use web_persistent::*;

pub type RuntimeResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RuntimeSourceUnit {
    pub path: String,
    pub source: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RuntimeLoadProfile {
    pub cache_hit: bool,
    pub compile: CompileProfile,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceDescriptor {
    pub id: SourceId,
    pub path: String,
    pub scoped: bool,
    pub interval_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceInventory {
    pub sources: Vec<SourceDescriptor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DocumentPatchStatus {
    Complete,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeTurn {
    pub sequence: u64,
    pub source_sequence: Option<u64>,
    pub deltas: Vec<Delta>,
    pub authority_deltas: Vec<AuthorityDelta>,
    pub durable_changes: Vec<DurableChange>,
    pub outbox_changes: Vec<boon_persistence::DurableOutboxChange>,
    pub document_patches: Vec<DocumentPatch>,
    pub document_patch_status: DocumentPatchStatus,
    pub metrics: TurnMetrics,
    pub materialization: DocumentMaterializationStats,
    pub phase_timings: RuntimePhaseTimings,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RuntimePhaseTimings {
    pub executor_us: u64,
    pub document_us: u64,
    pub persistence_enqueue_us: u64,
}

pub struct LiveRuntime {
    session: Session,
    document: Option<document::DocumentRuntime>,
    pending_document_rollback: Option<document::DocumentRollback>,
    source_inventory: SourceInventory,
    source_ids_by_path: BTreeMap<String, SourceId>,
}

#[derive(Clone)]
struct CachedPlan {
    plan: Arc<MachinePlan>,
    compile: CompileProfile,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum RuntimeSourceCacheKey {
    SourceText(String),
    SourceUnits(String),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RuntimePlanCacheKey {
    source: RuntimeSourceCacheKey,
    application: ApplicationIdentity,
}

fn plan_cache() -> &'static Mutex<BTreeMap<RuntimePlanCacheKey, CachedPlan>> {
    static CACHE: OnceLock<Mutex<BTreeMap<RuntimePlanCacheKey, CachedPlan>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn cached_plan(
    key: RuntimePlanCacheKey,
    compile: impl FnOnce() -> RuntimeResult<CompiledMachinePlanFromSource>,
) -> RuntimeResult<(CachedPlan, bool)> {
    if let Ok(cache) = plan_cache().lock()
        && let Some(cached) = cache.get(&key).cloned()
    {
        return Ok((cached, true));
    }

    let compiled = compile()?;
    let cached = CachedPlan {
        plan: Arc::new(compiled.plan),
        compile: compiled.profile,
    };
    if let Ok(mut cache) = plan_cache().lock() {
        cache.insert(key, cached.clone());
    }
    Ok((cached, false))
}

impl LiveRuntime {
    pub fn from_source(source_label: &str, source: &str) -> RuntimeResult<Self> {
        Self::from_source_with_identity(
            source_label,
            source,
            ApplicationIdentity::compiler_default(),
        )
    }

    pub fn from_source_with_identity(
        source_label: &str,
        source: &str,
        application: ApplicationIdentity,
    ) -> RuntimeResult<Self> {
        Ok(Self::from_source_profiled_with_identity(source_label, source, application)?.0)
    }

    pub fn from_source_profiled(
        source_label: &str,
        source: &str,
    ) -> RuntimeResult<(Self, RuntimeLoadProfile)> {
        Self::from_source_profiled_with_identity(
            source_label,
            source,
            ApplicationIdentity::compiler_default(),
        )
    }

    pub fn from_source_profiled_with_identity(
        source_label: &str,
        source: &str,
        application: ApplicationIdentity,
    ) -> RuntimeResult<(Self, RuntimeLoadProfile)> {
        let key = RuntimePlanCacheKey {
            source: RuntimeSourceCacheKey::SourceText(sha256_bytes(source.as_bytes())),
            application: application.clone(),
        };
        let (cached, cache_hit) = cached_plan(key, || {
            compile_runtime_source_text_to_machine_plan_with_identity(
                source_label,
                source,
                TargetProfile::SoftwareDefault,
                application,
            )
        })?;
        let runtime = Self::from_cached_plan(cached.clone())?;
        Ok((
            runtime,
            RuntimeLoadProfile {
                cache_hit,
                compile: cached.compile,
            },
        ))
    }

    pub fn from_project(source_label: &str, units: &[RuntimeSourceUnit]) -> RuntimeResult<Self> {
        Self::from_project_with_identity(
            source_label,
            units,
            ApplicationIdentity::compiler_default(),
        )
    }

    pub fn from_project_with_identity(
        source_label: &str,
        units: &[RuntimeSourceUnit],
        application: ApplicationIdentity,
    ) -> RuntimeResult<Self> {
        Ok(Self::from_project_profiled_with_identity(source_label, units, application)?.0)
    }

    pub fn from_project_profiled(
        source_label: &str,
        units: &[RuntimeSourceUnit],
    ) -> RuntimeResult<(Self, RuntimeLoadProfile)> {
        Self::from_project_profiled_with_identity(
            source_label,
            units,
            ApplicationIdentity::compiler_default(),
        )
    }

    pub fn from_project_profiled_with_identity(
        source_label: &str,
        units: &[RuntimeSourceUnit],
        application: ApplicationIdentity,
    ) -> RuntimeResult<(Self, RuntimeLoadProfile)> {
        let key = RuntimePlanCacheKey {
            source: RuntimeSourceCacheKey::SourceUnits(source_units_hash(units)),
            application: application.clone(),
        };
        let compiler_units = units
            .iter()
            .map(|unit| CompilerSourceUnit {
                path: unit.path.clone(),
                source: unit.source.clone(),
            })
            .collect::<Vec<_>>();
        let (cached, cache_hit) = cached_plan(key, || {
            compile_runtime_source_units_to_machine_plan_with_identity(
                source_label,
                &compiler_units,
                TargetProfile::SoftwareDefault,
                application,
            )
        })?;
        let runtime = Self::from_cached_plan(cached.clone())?;
        Ok((
            runtime,
            RuntimeLoadProfile {
                cache_hit,
                compile: cached.compile,
            },
        ))
    }

    pub fn from_machine_plan(plan: MachinePlan, options: SessionOptions) -> RuntimeResult<Self> {
        Self::from_machine_plan_with_restore(plan, options, None)
    }

    pub fn from_machine_plan_with_restore(
        plan: MachinePlan,
        options: SessionOptions,
        restore: Option<RestoreImage>,
    ) -> RuntimeResult<Self> {
        Self::from_shared_machine_plan_with_restore(Arc::new(plan), options, restore)
    }

    pub fn from_shared_machine_plan(
        plan: Arc<MachinePlan>,
        options: SessionOptions,
    ) -> RuntimeResult<Self> {
        Self::from_shared_machine_plan_with_restore(plan, options, None)
    }

    pub fn from_shared_machine_plan_with_restore(
        plan: Arc<MachinePlan>,
        options: SessionOptions,
        restore: Option<RestoreImage>,
    ) -> RuntimeResult<Self> {
        let source_inventory = source_inventory(&plan);
        let source_ids_by_path = source_inventory
            .sources
            .iter()
            .map(|source| (source.path.clone(), source.id))
            .collect();
        let builder = SessionBuilder::new_shared(plan, options)?;
        let mut session = match restore {
            Some(image) => builder.restore_durable(image)?,
            None => builder,
        }
        .build()?;
        let document = document::DocumentRuntime::new(&mut session)?;
        Ok(Self {
            session,
            document,
            pending_document_rollback: None,
            source_inventory,
            source_ids_by_path,
        })
    }

    fn from_cached_plan(cached: CachedPlan) -> RuntimeResult<Self> {
        Self::from_shared_machine_plan(cached.plan, SessionOptions::default())
    }

    pub fn mount(&self) -> RuntimeTurn {
        RuntimeTurn {
            sequence: 0,
            source_sequence: None,
            deltas: Vec::new(),
            authority_deltas: Vec::new(),
            durable_changes: Vec::new(),
            outbox_changes: Vec::new(),
            document_patches: self
                .document
                .as_ref()
                .map(document::DocumentRuntime::mount_patches)
                .unwrap_or_default(),
            document_patch_status: DocumentPatchStatus::Complete,
            metrics: TurnMetrics::default(),
            materialization: self
                .document
                .as_ref()
                .map(document::DocumentRuntime::stats)
                .unwrap_or_default(),
            phase_timings: RuntimePhaseTimings::default(),
        }
    }

    pub fn dispatch(&mut self, event: SourceEvent) -> RuntimeResult<RuntimeTurn> {
        let turn = self.dispatch_unsettled(event)?;
        self.settle_turn();
        Ok(turn)
    }

    pub fn dispatch_unsettled(&mut self, event: SourceEvent) -> RuntimeResult<RuntimeTurn> {
        if self.pending_document_rollback.is_some() {
            return Err("previous runtime turn has not been settled".into());
        }
        let demanded = self
            .document
            .as_ref()
            .map(document::DocumentRuntime::demanded_targets)
            .unwrap_or_default();
        let started = Instant::now();
        let turn = self.session.apply_with_demand(event, &demanded)?;
        let executor_us = duration_us(started.elapsed());
        self.runtime_turn(turn, executor_us)
    }

    pub fn begin_effect_dispatch_unsettled(
        &mut self,
        item: &boon_persistence::DurableOutboxItem,
    ) -> RuntimeResult<RuntimeTurn> {
        self.effect_turn(|session| session.begin_effect_dispatch(item))
    }

    pub fn require_effect_reconciliation_unsettled(
        &mut self,
        item: &boon_persistence::DurableOutboxItem,
    ) -> RuntimeResult<RuntimeTurn> {
        self.effect_turn(|session| session.require_effect_reconciliation(item))
    }

    pub fn complete_effect_unsettled(
        &mut self,
        item: &boon_persistence::DurableOutboxItem,
        outcome: boon_persistence::StoredValue,
    ) -> RuntimeResult<RuntimeTurn> {
        self.effect_turn(|session| session.complete_effect(item, outcome))
    }

    fn effect_turn(
        &mut self,
        build: impl FnOnce(&mut Session) -> Result<Turn, boon_plan_executor::Error>,
    ) -> RuntimeResult<RuntimeTurn> {
        if self.pending_document_rollback.is_some() {
            return Err("previous runtime turn has not been settled".into());
        }
        let started = Instant::now();
        let turn = build(&mut self.session)?;
        self.runtime_turn(turn, duration_us(started.elapsed()))
    }

    pub fn settle_turn(&mut self) {
        self.session.settle_turn();
        self.pending_document_rollback = None;
    }

    pub fn rollback_unsettled_turn(&mut self) -> RuntimeResult<()> {
        let Some(rollback) = self.pending_document_rollback.take() else {
            return Ok(());
        };
        let demanded = if let Some(document) = self.document.as_mut() {
            document.rollback_turn(rollback);
            document.demanded_targets()
        } else if !rollback.is_unchanged() {
            return Err("document rollback exists without a retained document runtime".into());
        } else {
            Vec::new()
        };
        self.session.rollback_unsettled_turn()?;
        self.session.ensure_current(&demanded)?;
        Ok(())
    }

    pub fn source_event(
        &self,
        sequence: u64,
        path: &str,
        target: Option<RowId>,
        payload: SourcePayload,
    ) -> RuntimeResult<SourceEvent> {
        let source = self
            .source_ids_by_path
            .get(path)
            .copied()
            .ok_or_else(|| format!("MachinePlan has no source route `{path}`"))?;
        Ok(SourceEvent {
            sequence,
            source,
            target,
            payload,
        })
    }

    pub fn snapshot(&self) -> RuntimeResult<Snapshot> {
        Ok(self.session.snapshot()?)
    }

    pub fn machine_plan(&self) -> &MachinePlan {
        self.session.plan()
    }

    pub fn shared_machine_plan(&self) -> Arc<MachinePlan> {
        self.session.shared_plan()
    }

    pub fn durable_restore_image(
        &self,
        epoch: u64,
        completed_migration_edges: BTreeSet<MigrationEdgeId>,
    ) -> RuntimeResult<RestoreImage> {
        Ok(self
            .session
            .durable_restore_image(epoch, completed_migration_edges)?)
    }

    pub fn root_value_current(&mut self, name: &str) -> RuntimeResult<Value> {
        Ok(self.session.root_value_current(name)?)
    }

    pub fn output_value_current(&mut self, name: &str) -> RuntimeResult<Value> {
        Ok(self.session.output_value_current(name)?)
    }

    pub fn inspect_value_current(&mut self, name: &str, max_rows: usize) -> RuntimeResult<Value> {
        Ok(self.session.inspect_value_current(name, max_rows)?)
    }

    pub fn document_frame(&self) -> Option<&DocumentFrame> {
        self.document.as_ref().map(document::DocumentRuntime::frame)
    }

    pub fn output_roots(&self) -> &[OutputRootPlan] {
        &self.session.plan().outputs
    }

    pub fn retained_output_frame(&self, name: &str) -> RuntimeResult<&DocumentFrame> {
        let output = self
            .session
            .plan()
            .outputs
            .iter()
            .find(|output| output.name == name)
            .ok_or_else(|| format!("MachinePlan has no output root `{name}`"))?;
        if !matches!(
            output.contract,
            OutputContractKind::Document | OutputContractKind::Scene
        ) {
            return Err(format!("output root `{name}` is not a retained visual output").into());
        }
        self.document
            .as_ref()
            .map(document::DocumentRuntime::frame)
            .ok_or_else(|| format!("output root `{name}` has no retained frame").into())
    }

    pub fn primary_retained_output_frame(&self) -> RuntimeResult<&DocumentFrame> {
        let output = self
            .session
            .plan()
            .outputs
            .iter()
            .find(|output| {
                matches!(
                    output.contract,
                    OutputContractKind::Document | OutputContractKind::Scene
                )
            })
            .ok_or("MachinePlan has no retained visual output root")?;
        self.retained_output_frame(&output.name)
    }

    pub fn document_materialization_stats(&self) -> DocumentMaterializationStats {
        self.document
            .as_ref()
            .map(document::DocumentRuntime::stats)
            .unwrap_or_default()
    }

    pub fn document_materialization_ids(&self) -> Vec<boon_plan::DocumentMaterializationId> {
        self.session
            .document_plan()
            .map(|plan| {
                plan.materializations
                    .iter()
                    .map(|materialization| materialization.id)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn demand_document_window(
        &mut self,
        demand: DocumentWindowDemand,
    ) -> RuntimeResult<Vec<DocumentPatch>> {
        let document = self
            .document
            .as_mut()
            .ok_or("MachinePlan has no DocumentPlan")?;
        Ok(document.demand_window(&mut self.session, demand)?)
    }

    pub fn demand_document_window_by_id(
        &mut self,
        materialization: u64,
        visible: Range<u64>,
        overscan: Range<u64>,
    ) -> RuntimeResult<Vec<DocumentPatch>> {
        self.demand_document_window(DocumentWindowDemand {
            materialization: boon_plan::DocumentMaterializationId(materialization),
            visible,
            overscan,
        })
    }

    pub fn row_target_for_source_path(
        &self,
        path: &str,
        key: u64,
        generation: u64,
    ) -> RuntimeResult<RowId> {
        Ok(self
            .session
            .row_target_for_source_path(path, key, generation)?)
    }

    pub fn row_target_for_source_text(
        &self,
        path: &str,
        text: &str,
        occurrence: usize,
    ) -> RuntimeResult<Option<RowId>> {
        let source = self
            .session
            .plan()
            .source_routes
            .iter()
            .find(|route| route.path == path)
            .ok_or_else(|| format!("MachinePlan has no source route `{path}`"))?;
        let Some(scope) = source.scope_id else {
            return Ok(None);
        };
        let list = self
            .session
            .plan()
            .storage_layout
            .list_slots
            .iter()
            .find(|list| list.scope_id == Some(scope))
            .map(|list| list.list_id)
            .ok_or_else(|| format!("scoped source `{path}` has no owning list"))?;
        Ok(self.session.find_row_by_text(list, text, occurrence))
    }

    pub fn source_inventory(&self) -> &SourceInventory {
        &self.source_inventory
    }

    pub fn source_row_lookup_field(&self, path: &str) -> Option<&str> {
        self.session
            .plan()
            .source_routes
            .iter()
            .find(|route| route.path == path)?
            .payload_schema
            .row_lookup_field_name()
    }

    pub fn source_is_row_scoped(&self, path: &str) -> Option<bool> {
        self.session
            .plan()
            .source_routes
            .iter()
            .find(|route| route.path == path)
            .map(|route| route.scope_id.is_some())
    }

    pub fn run_scenario(&mut self, scenario: &Scenario) -> RuntimeResult<Vec<RuntimeTurn>> {
        let mut turns = Vec::new();
        let mut sequence = 1u64;
        for step in &scenario.steps {
            let turn = if let Some(event) = &step.source_event {
                let target = self
                    .scenario_target(event)
                    .map_err(|error| format!("scenario step `{}` target: {error}", step.id))?;
                let source_event = self
                    .source_event(sequence, &event.source, target, event.payload.clone())
                    .map_err(|error| format!("scenario step `{}` event: {error}", step.id))?;
                let turn = self
                    .dispatch(source_event)
                    .map_err(|error| format!("scenario step `{}` dispatch: {error}", step.id))?;
                sequence = sequence.saturating_add(1);
                Some(turn)
            } else {
                None
            };
            self.assert_scenario_step(step, turn.as_ref())?;
            if let Some(turn) = turn {
                turns.push(turn);
            }
        }
        Ok(turns)
    }

    pub fn assert_scenario_step(
        &mut self,
        step: &ScenarioStep,
        turn: Option<&RuntimeTurn>,
    ) -> RuntimeResult<()> {
        let mut mismatches = Vec::new();
        for expectation in &step.expectations {
            match self.scenario_expectation_mismatch(expectation, turn) {
                Ok(Some(mismatch)) => mismatches.push(mismatch),
                Ok(None) => {}
                Err(error) => mismatches.push(error.to_string()),
            }
        }
        if let Err(error) = self.session.settle_published() {
            mismatches.push(format!("currentness barrier: {error}"));
        }
        if mismatches.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "scenario step `{}` expectation mismatches: {}",
                step.id,
                mismatches.join("; ")
            )
            .into())
        }
    }

    fn scenario_expectation_mismatch(
        &mut self,
        expectation: &ScenarioExpectation,
        turn: Option<&RuntimeTurn>,
    ) -> RuntimeResult<Option<String>> {
        match expectation {
            ScenarioExpectation::RootText { name, value } => {
                let actual = scenario_value_text(&self.session.root_value_current(name)?)?;
                Ok((actual != *value)
                    .then(|| format!("root `{name}` expected `{value}`, got `{actual}`")))
            }
            ScenarioExpectation::RootNonEmpty { name } => {
                let actual = scenario_value_text(&self.session.root_value_current(name)?)?;
                Ok(actual
                    .is_empty()
                    .then(|| format!("root `{name}` expected a non-empty value")))
            }
            ScenarioExpectation::ListTexts {
                list,
                field,
                filter,
                values,
            } => {
                let actual = self.scenario_list_texts(list, field, filter.as_ref())?;
                Ok((actual != *values)
                    .then(|| format!("list `{list}.{field}` expected {values:?}, got {actual:?}")))
            }
            ScenarioExpectation::RootRowTexts {
                root,
                field,
                values,
            } => {
                let root_value = self.session.root_value_current(root)?;
                let Value::List(rows) = root_value else {
                    return Err(format!("root `{root}` is not a row list").into());
                };
                let rows = rows
                    .into_iter()
                    .map(|value| match value {
                        Value::Row { id, .. } => Ok(id),
                        other => Err(format!("root `{root}` contains non-row value {other:?}")),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let list = rows
                    .first()
                    .map(|row| row.list)
                    .or_else(|| self.scenario_list_id("todos").ok())
                    .ok_or_else(|| format!("root `{root}` has no rows or owning list"))?;
                let field_id = self.scenario_field_id(list, field)?;
                let actual = self.scenario_row_texts(&rows, field_id)?;
                Ok((actual != *values).then(|| {
                    format!("root rows `{root}.{field}` expected {values:?}, got {actual:?}")
                }))
            }
            ScenarioExpectation::ListCount {
                list,
                filter,
                count,
            } => {
                let actual = self
                    .scenario_list_texts(list, &filter.field, None)?
                    .into_iter()
                    .filter(|value| value == &filter.value)
                    .count();
                Ok((actual != *count).then(|| {
                    format!(
                        "list `{list}` count where {}={} expected {count}, got {actual}",
                        filter.field, filter.value
                    )
                }))
            }
            ScenarioExpectation::RowFields {
                list,
                key_field,
                key,
                fields,
            } => {
                let list_id = self.scenario_list_id(list)?;
                let key_field_id = self.scenario_field_id(list_id, key_field)?;
                let rows = self.session.list_rows(list_id);
                let keys = self.scenario_row_texts(&rows, key_field_id)?;
                let matches = rows
                    .into_iter()
                    .zip(keys)
                    .filter_map(|(row, value)| (value == *key).then_some(row))
                    .collect::<Vec<_>>();
                let [row] = matches.as_slice() else {
                    return Ok(Some(format!(
                        "list `{list}` expected one row where {key_field}={key}, found {}",
                        matches.len()
                    )));
                };
                let mut actual = BTreeMap::new();
                for field in fields.keys() {
                    let field_id = self.scenario_field_id(list_id, field)?;
                    actual.insert(
                        field.clone(),
                        self.scenario_row_texts(&[*row], field_id)?[0].clone(),
                    );
                }
                Ok((actual != *fields)
                    .then(|| format!("row `{list}[{key}]` expected {fields:?}, got {actual:?}")))
            }
            ScenarioExpectation::RecomputedRows {
                list,
                key_field,
                field,
                keys,
            } => {
                let turn = turn.ok_or("recomputed-row expectation requires a source event")?;
                let list_id = self.scenario_list_id(list)?;
                let field_id = self.scenario_field_id(list_id, field)?;
                let key_field_id = self.scenario_field_id(list_id, key_field)?;
                let rows = turn
                    .metrics
                    .recomputed_targets
                    .iter()
                    .filter_map(|target| match target {
                        ValueTarget::RowField { row, field }
                            if row.list == list_id && *field == field_id =>
                        {
                            Some(*row)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let actual = self.scenario_row_texts(&rows, key_field_id)?;
                Ok((actual != *keys).then(|| {
                    format!("recomputed `{list}.{field}` rows expected {keys:?}, got {actual:?}")
                }))
            }
            ScenarioExpectation::SemanticDeltaContains(expected) => {
                let turn = turn.ok_or("semantic-delta expectation requires a source event")?;
                Ok((!turn
                    .deltas
                    .iter()
                    .any(|delta| self.scenario_delta_matches(delta, expected)))
                .then(|| {
                    format!(
                        "semantic deltas do not contain `{expected}`: {:?}",
                        turn.deltas
                    )
                }))
            }
            ScenarioExpectation::DocumentChanged => {
                let turn = turn.ok_or("document-change expectation requires a source event")?;
                Ok(turn.document_patches.is_empty().then(|| {
                    format!(
                        "document produced no retained patches after deltas {:?}",
                        turn.deltas
                    )
                }))
            }
        }
    }

    fn scenario_list_texts(
        &mut self,
        list: &str,
        field: &str,
        filter: Option<&ScenarioFieldMatch>,
    ) -> RuntimeResult<Vec<String>> {
        let list_id = self.scenario_list_id(list)?;
        let field_id = self.scenario_field_id(list_id, field)?;
        let rows = self.session.list_rows(list_id);
        let values = self.scenario_row_texts(&rows, field_id)?;
        let Some(filter) = filter else {
            return Ok(values);
        };
        let filter_id = self.scenario_field_id(list_id, &filter.field)?;
        let filters = self.scenario_row_texts(&rows, filter_id)?;
        Ok(values
            .into_iter()
            .zip(filters)
            .filter_map(|(value, actual)| (actual == filter.value).then_some(value))
            .collect())
    }

    fn scenario_row_texts(
        &mut self,
        rows: &[RowId],
        field: boon_plan::FieldId,
    ) -> RuntimeResult<Vec<String>> {
        let targets = rows
            .iter()
            .copied()
            .map(|row| ValueTarget::RowField { row, field })
            .collect::<Vec<_>>();
        let values = self.session.project_current(&targets)?;
        targets
            .iter()
            .map(|target| {
                values
                    .get(target)
                    .ok_or_else(|| {
                        format!("scenario target {target:?} has no current value").into()
                    })
                    .and_then(scenario_value_text)
            })
            .collect()
    }

    fn scenario_list_id(&self, name: &str) -> RuntimeResult<boon_plan::ListId> {
        let candidates = self
            .session
            .plan()
            .debug_map
            .list_slots
            .iter()
            .filter(|entry| scenario_name_matches(&entry.label, name))
            .filter_map(|entry| entry.id.rsplit(':').next()?.parse().ok())
            .map(boon_plan::ListId)
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [list] => Ok(*list),
            _ => Err(format!("MachinePlan list `{name}` resolved to {candidates:?}").into()),
        }
    }

    fn scenario_field_id(
        &self,
        list: boon_plan::ListId,
        name: &str,
    ) -> RuntimeResult<boon_plan::FieldId> {
        let fields = self
            .session
            .plan()
            .storage_layout
            .list_slots
            .iter()
            .find(|slot| slot.list_id == list)
            .ok_or_else(|| format!("MachinePlan has no list {}", list.0))?
            .row_field_ids
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        let candidates = self
            .session
            .plan()
            .debug_map
            .fields
            .iter()
            .filter(|entry| scenario_name_matches(&entry.label, name))
            .filter_map(|entry| {
                let field = boon_plan::FieldId(entry.id.rsplit(':').next()?.parse().ok()?);
                fields
                    .contains(&field)
                    .then_some((field, entry.label.as_str()))
            })
            .collect::<Vec<_>>();
        let canonical = candidates
            .iter()
            .filter(|(_, label)| !label.contains(".$input$"))
            .map(|(field, _)| *field)
            .collect::<Vec<_>>();
        let computed = candidates
            .iter()
            .filter_map(|(field, _)| {
                self.session
                    .plan()
                    .regions
                    .iter()
                    .flat_map(|region| &region.ops)
                    .any(|op| op.indexed && op.output == Some(boon_plan::ValueRef::Field(*field)))
                    .then_some(*field)
            })
            .collect::<Vec<_>>();
        match computed.as_slice() {
            [field] => Ok(*field),
            [] => match canonical.as_slice() {
                [field] => Ok(*field),
                [] if candidates.len() == 1 => Ok(candidates[0].0),
                _ => Err(format!(
                    "MachinePlan list {} field `{name}` resolved to {candidates:?}",
                    list.0
                )
                .into()),
            },
            _ => Err(format!(
                "MachinePlan list {} field `{name}` resolved to {candidates:?}",
                list.0
            )
            .into()),
        }
    }

    fn scenario_delta_matches(&self, delta: &Delta, expected: &str) -> bool {
        match expected {
            "ListInsert" => matches!(delta, Delta::InsertRow { .. }),
            "ListRemove" => matches!(delta, Delta::RemoveRow { .. }),
            "SourceBind" => matches!(delta, Delta::BindSource { .. }),
            "SourceUnbind" => matches!(delta, Delta::UnbindSource { .. }),
            _ => expected.strip_prefix("FieldSet:").is_some_and(|name| {
                let Delta::SetValue { target, .. } = delta else {
                    return false;
                };
                self.scenario_target_label(*target)
                    .is_some_and(|label| scenario_name_matches(label, name))
            }),
        }
    }

    fn scenario_target_label(&self, target: ValueTarget) -> Option<&str> {
        let (entries, prefix, id) = match target {
            ValueTarget::State(id) => (&self.session.plan().debug_map.state_slots, "state:", id.0),
            ValueTarget::Field(id) | ValueTarget::RowField { field: id, .. } => {
                (&self.session.plan().debug_map.fields, "field:", id.0)
            }
        };
        let id = format!("{prefix}{id}");
        entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.label.as_str())
    }

    fn scenario_target(&self, event: &ScenarioSourceEvent) -> RuntimeResult<Option<RowId>> {
        if let Some(list) = event.target_list.as_deref() {
            let list = self
                .session
                .plan()
                .debug_map
                .list_slots
                .iter()
                .find(|entry| entry.label == list)
                .and_then(|entry| entry.id.rsplit(':').next())
                .and_then(|id| id.parse().ok())
                .map(boon_plan::ListId)
                .ok_or_else(|| format!("MachinePlan has no list `{list}`"))?;
            return Ok(Some(RowId {
                list,
                key: event.target_key.ok_or("scenario row target has no key")?,
                generation: event.target_generation.unwrap_or(1),
            }));
        }
        let source = self
            .session
            .plan()
            .source_routes
            .iter()
            .find(|route| route.path == event.source)
            .ok_or_else(|| format!("MachinePlan has no source route `{}`", event.source))?;
        if source.scope_id.is_none() {
            return Ok(None);
        }
        let Some(target_text) = event.target_text.as_deref() else {
            return Ok(None);
        };
        let occurrence = event.target_occurrence.unwrap_or(0);
        let target = self
            .row_target_for_source_text(&event.source, target_text, occurrence)?
            .ok_or_else(|| {
                format!(
                    "scenario source `{}` could not resolve row text `{target_text}` occurrence {occurrence}",
                    event.source
                )
            })?;
        Ok(Some(target))
    }

    fn runtime_turn(&mut self, turn: Turn, executor_us: u64) -> RuntimeResult<RuntimeTurn> {
        let document_started = Instant::now();
        let (document_patches, document_rollback) = match self.document.as_mut() {
            Some(document) => match document.apply_turn(&mut self.session, &turn.deltas) {
                Ok(applied) => applied,
                Err(error) => {
                    if let Err(rollback) = self.session.rollback_unsettled_turn() {
                        return Err(format!(
                            "document settle failed with `{error}` and authority rollback failed with `{rollback}`"
                        )
                        .into());
                    }
                    return Err(error.into());
                }
            },
            None => (Vec::new(), document::DocumentRollback::unchanged()),
        };
        self.pending_document_rollback = Some(document_rollback);
        Ok(RuntimeTurn {
            sequence: turn.sequence,
            source_sequence: turn.source_sequence,
            deltas: turn.deltas,
            authority_deltas: turn.authority_deltas,
            durable_changes: turn.durable_changes,
            outbox_changes: turn.outbox_changes,
            document_patches,
            document_patch_status: DocumentPatchStatus::Complete,
            metrics: turn.metrics,
            materialization: self
                .document
                .as_ref()
                .map(document::DocumentRuntime::stats)
                .unwrap_or_default(),
            phase_timings: RuntimePhaseTimings {
                executor_us,
                document_us: duration_us(document_started.elapsed()),
                persistence_enqueue_us: 0,
            },
        })
    }
}

fn duration_us(duration: std::time::Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

fn source_inventory(plan: &MachinePlan) -> SourceInventory {
    SourceInventory {
        sources: plan
            .source_routes
            .iter()
            .map(|route| SourceDescriptor {
                id: route.source_id,
                path: route.path.clone(),
                scoped: route.scoped,
                interval_ms: route.interval_ms,
            })
            .collect(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scenario {
    pub name: String,
    pub source: String,
    pub steps: Vec<ScenarioStep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioStep {
    pub id: String,
    pub user_action_kind: Option<String>,
    pub user_action_text: Option<String>,
    pub user_action_key: Option<String>,
    pub source_event: Option<ScenarioSourceEvent>,
    pub expectations: Vec<ScenarioExpectation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScenarioExpectation {
    RootText {
        name: String,
        value: String,
    },
    RootNonEmpty {
        name: String,
    },
    ListTexts {
        list: String,
        field: String,
        filter: Option<ScenarioFieldMatch>,
        values: Vec<String>,
    },
    RootRowTexts {
        root: String,
        field: String,
        values: Vec<String>,
    },
    ListCount {
        list: String,
        filter: ScenarioFieldMatch,
        count: usize,
    },
    RowFields {
        list: String,
        key_field: String,
        key: String,
        fields: BTreeMap<String, String>,
    },
    RecomputedRows {
        list: String,
        key_field: String,
        field: String,
        keys: Vec<String>,
    },
    SemanticDeltaContains(String),
    DocumentChanged,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioFieldMatch {
    pub field: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioSourceEvent {
    pub source: String,
    pub target_list: Option<String>,
    pub target_key: Option<u64>,
    pub target_generation: Option<u64>,
    pub target_text: Option<String>,
    pub target_occurrence: Option<usize>,
    pub payload: SourcePayload,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioFile {
    name: String,
    source: String,
    #[serde(default)]
    step: Vec<ScenarioFileStep>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioFileStep {
    id: String,
    expected_source_event: Option<ScenarioFileEvent>,
    #[serde(rename = "user_action")]
    user_action: Option<toml::Value>,
    #[serde(rename = "source_intent_exemption")]
    _source_intent_exemption: Option<String>,
    #[serde(default)]
    expect_root_text: BTreeMap<String, String>,
    #[serde(default)]
    expect_root_nonempty: Vec<String>,
    expect_titles: Option<Vec<String>>,
    expect_completed_titles: Option<Vec<String>>,
    expect_visible_titles: Option<Vec<String>>,
    expect_active_count: Option<usize>,
    expect_completed_count: Option<usize>,
    expect_filter: Option<String>,
    expect_new_text: Option<String>,
    expect_editing_title: Option<String>,
    expect_edit_text: Option<String>,
    expect_no_editing: Option<bool>,
    expect_cell: Option<ScenarioFileCellExpectation>,
    expect_error: Option<ScenarioFileCellErrorExpectation>,
    #[serde(default)]
    expect_recomputed: Vec<String>,
    #[serde(default)]
    expect_semantic_delta_contains: Vec<String>,
    #[serde(default)]
    expect_render_delta_contains: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioFileCellExpectation {
    address: String,
    value: Option<String>,
    formula: Option<String>,
    editing_text: Option<String>,
    editing: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioFileCellErrorExpectation {
    address: String,
    error: String,
}

#[derive(Deserialize)]
struct ScenarioFileEvent {
    source: String,
    text: Option<String>,
    key: Option<String>,
    address: Option<String>,
    list_id: Option<String>,
    target_key: Option<u64>,
    target_generation: Option<u64>,
    target_text: Option<String>,
    target_occurrence: Option<usize>,
    #[serde(default)]
    payload: BTreeMap<String, String>,
    #[serde(flatten)]
    fields: BTreeMap<String, String>,
}

pub fn parse_scenario(path: &Path) -> RuntimeResult<Scenario> {
    let file: ScenarioFile = toml::from_str(&fs::read_to_string(resolve_repo_file(path))?)?;
    Ok(Scenario {
        name: file.name,
        source: file.source,
        steps: file
            .step
            .into_iter()
            .map(|step| {
                let expectations = scenario_expectations(&step)?;
                let user_action_kind = step
                    .user_action
                    .as_ref()
                    .and_then(|action| action.get("kind"))
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned);
                let user_action_text = step
                    .user_action
                    .as_ref()
                    .and_then(|action| action.get("text"))
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned);
                let user_action_key = step
                    .user_action
                    .as_ref()
                    .and_then(|action| action.get("key"))
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned);
                Ok(ScenarioStep {
                    id: step.id,
                    user_action_kind,
                    user_action_text,
                    user_action_key,
                    source_event: step.expected_source_event.map(|event| {
                        let mut fields = event.payload;
                        fields.extend(event.fields);
                        ScenarioSourceEvent {
                            source: event.source,
                            target_list: event.list_id,
                            target_key: event.target_key,
                            target_generation: event.target_generation,
                            target_text: event.target_text,
                            target_occurrence: event.target_occurrence,
                            payload: SourcePayload {
                                text: event.text,
                                key: event.key,
                                address: event.address,
                                fields: fields
                                    .into_iter()
                                    .map(|(name, value)| (name, Value::Text(value)))
                                    .collect(),
                            },
                        }
                    }),
                    expectations,
                })
            })
            .collect::<RuntimeResult<Vec<_>>>()?,
    })
}

fn scenario_expectations(step: &ScenarioFileStep) -> RuntimeResult<Vec<ScenarioExpectation>> {
    let mut expectations = step
        .expect_root_text
        .iter()
        .map(|(name, value)| ScenarioExpectation::RootText {
            name: name.clone(),
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    expectations.extend(
        step.expect_root_nonempty
            .iter()
            .cloned()
            .map(|name| ScenarioExpectation::RootNonEmpty { name }),
    );
    let list_texts =
        |field: &str, filter: Option<(&str, &str)>, values: &[String]| -> ScenarioExpectation {
            ScenarioExpectation::ListTexts {
                list: "todos".to_owned(),
                field: field.to_owned(),
                filter: filter.map(|(field, value)| ScenarioFieldMatch {
                    field: field.to_owned(),
                    value: value.to_owned(),
                }),
                values: values.to_vec(),
            }
        };
    if let Some(values) = &step.expect_titles {
        expectations.push(list_texts("title", None, values));
    }
    if let Some(values) = &step.expect_completed_titles {
        expectations.push(list_texts("title", Some(("completed", "True")), values));
    }
    if let Some(values) = &step.expect_visible_titles {
        expectations.push(ScenarioExpectation::RootRowTexts {
            root: "store.visible_todos".to_owned(),
            field: "title".to_owned(),
            values: values.clone(),
        });
    }
    for (count, value) in [
        (step.expect_active_count, "False"),
        (step.expect_completed_count, "True"),
    ] {
        if let Some(count) = count {
            expectations.push(ScenarioExpectation::ListCount {
                list: "todos".to_owned(),
                filter: ScenarioFieldMatch {
                    field: "completed".to_owned(),
                    value: value.to_owned(),
                },
                count,
            });
        }
    }
    for (name, value) in [
        ("store.selected_filter", step.expect_filter.as_ref()),
        ("store.new_todo_text", step.expect_new_text.as_ref()),
    ] {
        if let Some(value) = value {
            expectations.push(ScenarioExpectation::RootText {
                name: name.to_owned(),
                value: value.clone(),
            });
        }
    }
    if let Some(value) = &step.expect_editing_title {
        expectations.push(list_texts(
            "title",
            Some(("editing", "True")),
            std::slice::from_ref(value),
        ));
    }
    if let Some(value) = &step.expect_edit_text {
        expectations.push(list_texts(
            "edit_text",
            Some(("editing", "True")),
            std::slice::from_ref(value),
        ));
    }
    if step.expect_no_editing == Some(true) {
        expectations.push(ScenarioExpectation::ListCount {
            list: "todos".to_owned(),
            filter: ScenarioFieldMatch {
                field: "editing".to_owned(),
                value: "True".to_owned(),
            },
            count: 0,
        });
    }
    if let Some(cell) = &step.expect_cell {
        let mut fields = BTreeMap::new();
        for (field, value) in [
            ("value", cell.value.as_ref()),
            ("formula_text", cell.formula.as_ref()),
            ("editing_text", cell.editing_text.as_ref()),
        ] {
            if let Some(value) = value {
                fields.insert(field.to_owned(), value.clone());
            }
        }
        if let Some(value) = cell.editing {
            fields.insert(
                "editing".to_owned(),
                if value { "True" } else { "False" }.to_owned(),
            );
        }
        expectations.push(ScenarioExpectation::RowFields {
            list: "cells".to_owned(),
            key_field: "address".to_owned(),
            key: cell.address.clone(),
            fields,
        });
    }
    if let Some(cell) = &step.expect_error {
        expectations.push(ScenarioExpectation::RowFields {
            list: "cells".to_owned(),
            key_field: "address".to_owned(),
            key: cell.address.clone(),
            fields: BTreeMap::from([("error".to_owned(), cell.error.clone())]),
        });
    }
    if !step.expect_recomputed.is_empty() {
        expectations.push(ScenarioExpectation::RecomputedRows {
            list: "cells".to_owned(),
            key_field: "address".to_owned(),
            field: "value".to_owned(),
            keys: step.expect_recomputed.clone(),
        });
    }
    expectations.extend(
        step.expect_semantic_delta_contains
            .iter()
            .cloned()
            .map(ScenarioExpectation::SemanticDeltaContains),
    );
    for expected in &step.expect_render_delta_contains {
        if expected != "InvalidateDocument" {
            return Err(format!(
                "scenario step `{}` has unsupported document expectation `{expected}`",
                step.id
            )
            .into());
        }
        if !expectations.contains(&ScenarioExpectation::DocumentChanged) {
            expectations.push(ScenarioExpectation::DocumentChanged);
        }
    }
    Ok(expectations)
}

fn scenario_name_matches(label: &str, expected: &str) -> bool {
    label == expected || label.rsplit('.').next() == Some(expected)
}

fn scenario_value_text(value: &Value) -> RuntimeResult<String> {
    match value {
        Value::Null => Ok(String::new()),
        Value::Bool(value) => Ok(if *value { "True" } else { "False" }.to_owned()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Text(value) => Ok(value.clone()),
        Value::Bytes(value) => Ok(String::from_utf8(value.clone())?),
        Value::Error { code } => Ok(code.clone()),
        Value::List(_) | Value::Record(_) | Value::MappedRow { .. } | Value::Row { .. } => {
            Err("scenario text expectation targeted a structured value".into())
        }
    }
}

pub fn example_manifest_entries() -> RuntimeResult<Vec<ExampleManifestEntry>> {
    let path = resolve_repo_file("examples/manifest.toml");
    let manifest = ExampleManifest::from_path(path)?;
    Ok(manifest.example)
}

pub fn example_manifest_entry(id: &str) -> RuntimeResult<ExampleManifestEntry> {
    example_manifest_entries()?
        .into_iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| format!("example manifest has no entry `{id}`").into())
}

pub fn source_units_for_path(path: &Path) -> RuntimeResult<Vec<RuntimeSourceUnit>> {
    Ok(compiler_source_units_for_path(path)?
        .into_iter()
        .map(runtime_source_unit)
        .collect())
}

pub fn source_units_for_entry(
    entry: &ExampleManifestEntry,
) -> RuntimeResult<Vec<RuntimeSourceUnit>> {
    Ok(
        compiler_source_units_for_manifest_source(&entry.source, &entry.source_files)?
            .into_iter()
            .map(runtime_source_unit)
            .collect(),
    )
}

pub fn migration_sequence_for_entry(
    entry: &ExampleManifestEntry,
) -> RuntimeResult<Option<(MigrationSequence, MigrationScenario)>> {
    let Some(sequence_path) = entry.migration_sequence.as_deref() else {
        return Ok(None);
    };
    let sequence = MigrationSequence::from_path(
        resolve_repo_file(sequence_path),
        resolve_repo_file("examples"),
    )?;
    let scenario = MigrationScenario::from_path(resolve_repo_file(&sequence.scenario), &sequence)?;
    Ok(Some((sequence, scenario)))
}

pub fn source_text_for_path(path: &Path) -> RuntimeResult<String> {
    compiler_source_text_for_path(path)
}

pub fn source_text_for_entry(entry: &ExampleManifestEntry) -> RuntimeResult<String> {
    source_text_for_path(Path::new(&entry.source))
}

fn runtime_source_unit(unit: CompilerSourceUnit) -> RuntimeSourceUnit {
    RuntimeSourceUnit {
        path: unit.path,
        source: unit.source,
    }
}

pub fn source_units_hash(units: &[RuntimeSourceUnit]) -> String {
    let parts = units
        .iter()
        .map(|unit| (unit.path.as_str(), unit.source.as_str()))
        .collect::<Vec<_>>();
    source_unit_parts_hash(&parts)
}

pub fn source_unit_parts_hash(units: &[(&str, &str)]) -> String {
    if let [(_, source)] = units {
        return sha256_bytes(source.as_bytes());
    }
    let mut hasher = Sha256::new();
    for (path, source) in units {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(source.as_bytes());
        hasher.update([0xff]);
    }
    format!("{:x}", hasher.finalize())
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn resolve_repo_file(relative: impl AsRef<Path>) -> PathBuf {
    let relative = relative.as_ref();
    if relative.exists() {
        return relative.to_path_buf();
    }
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            let candidate = ancestor.join(relative);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    relative.to_path_buf()
}

#[cfg(test)]
mod tests;
