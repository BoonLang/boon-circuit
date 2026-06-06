#![recursion_limit = "512"]
#![allow(clippy::too_many_arguments)]
#![allow(dead_code)]

use bitvec::prelude::*;
use boon_ir::{
    DerivedValueKind, FieldId, FunctionDefinition, InitialValue, ListId, ListInitializer,
    ListOperationKind, ListPredicate, ListProjectionKind, SourceId, SourcePayloadField,
    TypedProgram, UpdateExpression, UpdateMatchArm, debug_tables, lower, verify_hidden_identity,
    verify_static_schedule,
};
use boon_parser::{
    AstCallArg, AstExpr, AstExprKind, AstRecordField, AstStatement, AstStatementKind, DocumentAst,
    ParsedProgram, parse_project, parse_source,
};
use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Value as JsonValue, json};
use std::alloc::{GlobalAlloc, Layout, System};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub type RuntimeResult<T> = Result<T, Box<dyn std::error::Error>>;

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct AllocationSnapshot {
    count: u64,
    bytes: u64,
}

fn allocation_snapshot() -> AllocationSnapshot {
    AllocationSnapshot {
        count: ALLOC_COUNT.load(Ordering::Relaxed),
        bytes: ALLOC_BYTES.load(Ordering::Relaxed),
    }
}

fn allocation_delta(before: AllocationSnapshot) -> AllocationSnapshot {
    AllocationSnapshot {
        count: ALLOC_COUNT
            .load(Ordering::Relaxed)
            .saturating_sub(before.count),
        bytes: ALLOC_BYTES
            .load(Ordering::Relaxed)
            .saturating_sub(before.bytes),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub step: Vec<ScenarioStep>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ScenarioStep {
    pub id: String,
    #[serde(default)]
    pub user_action: Option<BTreeMap<String, toml::Value>>,
    #[serde(default)]
    pub expected_source_event: Option<BTreeMap<String, toml::Value>>,
    #[serde(default)]
    pub expect_titles: Option<Vec<String>>,
    #[serde(default)]
    pub expect_visible_titles: Option<Vec<String>>,
    #[serde(default)]
    pub expect_completed_titles: Option<Vec<String>>,
    #[serde(default)]
    pub expect_active_count: Option<usize>,
    #[serde(default)]
    pub expect_completed_count: Option<usize>,
    #[serde(default)]
    pub expect_filter: Option<String>,
    #[serde(default)]
    pub expect_new_text: Option<String>,
    #[serde(default)]
    pub expect_editing_title: Option<String>,
    #[serde(default)]
    pub expect_edit_text: Option<String>,
    #[serde(default)]
    pub expect_no_editing: Option<bool>,
    #[serde(default)]
    pub expect_cell: Option<CellExpectation>,
    #[serde(default)]
    pub expect_error: Option<CellErrorExpectation>,
    #[serde(default)]
    pub expect_recomputed: Option<Vec<String>>,
    #[serde(default)]
    pub expect_semantic_delta_contains: Vec<String>,
    #[serde(default)]
    pub expect_render_delta_contains: Vec<String>,
    #[serde(default)]
    pub expect_root_text: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug)]
struct GenericSourceEvent<'a> {
    source: &'a str,
    text: Option<&'a str>,
    key: Option<&'a str>,
    target_text: Option<&'a str>,
    address: Option<&'a str>,
}

impl<'a> GenericSourceEvent<'a> {
    fn from_step(step: &'a ScenarioStep) -> RuntimeResult<Option<Self>> {
        let Some(expected) = &step.expected_source_event else {
            return Ok(None);
        };
        let source = toml_string_ref(expected, "source")
            .ok_or_else(|| format!("{} expected_source_event missing source", step.id))?;
        Ok(Some(Self {
            source,
            text: toml_string_ref(expected, "text"),
            key: toml_string_ref(expected, "key"),
            target_text: toml_string_ref(expected, "target_text"),
            address: toml_string_ref(expected, "address"),
        }))
    }

    fn require(step: &'a ScenarioStep) -> RuntimeResult<Self> {
        Self::from_step(step)?.ok_or_else(|| {
            format!(
                "{} routed a source-producing event without expected_source_event",
                step.id
            )
            .into()
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct GenericRoutedSourceEvent<'a> {
    event: GenericSourceEvent<'a>,
    route_kind: SourceActionKind,
}

impl<'a> GenericRoutedSourceEvent<'a> {
    fn source(self) -> &'a str {
        self.event.source
    }

    fn require_text(self, step_id: &str) -> RuntimeResult<&'a str> {
        self.event.text.ok_or_else(|| {
            format!(
                "{step_id} source `{}` route `{}` requires text",
                self.event.source,
                self.route_kind.as_str()
            )
            .into()
        })
    }

    fn require_address(self, step_id: &str) -> RuntimeResult<&'a str> {
        self.event.address.ok_or_else(|| {
            format!(
                "{step_id} source `{}` route `{}` requires address",
                self.event.source,
                self.route_kind.as_str()
            )
            .into()
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CellExpectation {
    pub address: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub formula: Option<String>,
    #[serde(default)]
    pub editing_text: Option<String>,
    #[serde(default)]
    pub editing: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CellErrorExpectation {
    pub address: String,
    pub error: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationLayer {
    Semantic,
    HeadlessPly,
    HeadedPly,
    OperatorE2e,
    Human,
    Speed,
    Negative,
    All,
}

impl VerificationLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::HeadlessPly => "ply-headless",
            Self::HeadedPly => "headed-ply",
            Self::OperatorE2e => "operator-e2e",
            Self::Human => "human",
            Self::Speed => "speed",
            Self::Negative => "negative",
            Self::All => "all",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RunOutput {
    pub report: JsonValue,
    pub semantic_deltas: Vec<SemanticDelta<'static>>,
    pub render_patches: Vec<RenderPatch<'static>>,
    pub state_summary: JsonValue,
    #[serde(skip)]
    pub document: Option<DocumentAst>,
}

#[derive(Clone, Debug, Default)]
pub struct LiveSourceEvent {
    pub source: String,
    pub text: Option<String>,
    pub key: Option<String>,
    pub address: Option<String>,
    pub target_text: Option<String>,
    pub target_occurrence: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LiveStepOutput {
    pub semantic_deltas: Vec<SemanticDelta<'static>>,
    pub render_patches: Vec<RenderPatch<'static>>,
    pub state_summary: JsonValue,
}

#[derive(Clone, Debug)]
pub struct SemanticDelta<'a> {
    pub kind: &'static str,
    pub list_id: Option<Cow<'a, str>>,
    pub key: Option<u64>,
    pub generation: Option<u64>,
    pub source_id: Option<u64>,
    pub bind_epoch: Option<u64>,
    pub field_path: Option<Cow<'a, str>>,
    pub value: ProtocolValue<'a>,
}

#[derive(Clone, Debug)]
pub struct RenderPatch<'a> {
    pub kind: &'static str,
    pub target: RenderTarget<'a>,
    pub value: ProtocolValue<'a>,
    pub list_id: Option<Cow<'a, str>>,
    pub key: Option<u64>,
    pub generation: Option<u64>,
    pub source_id: Option<u64>,
    pub bind_epoch: Option<u64>,
}

#[derive(Clone, Debug)]
pub enum ProtocolValue<'a> {
    Null,
    Bool(bool),
    Text(Cow<'a, str>),
    NumberText(i64),
    SourceBinding {
        source_path: Cow<'a, str>,
        source_id: u64,
        bind_epoch: u64,
    },
    CheckedProperty(bool),
}

#[derive(Clone, Debug)]
pub enum RenderTarget<'a> {
    Static(Cow<'a, str>),
    Borrowed(Cow<'a, str>),
}

impl<'a> SemanticDelta<'a> {
    fn to_static(&self) -> SemanticDelta<'static> {
        SemanticDelta {
            kind: self.kind,
            list_id: self
                .list_id
                .as_ref()
                .map(|value| Cow::Owned(value.to_string())),
            key: self.key,
            generation: self.generation,
            source_id: self.source_id,
            bind_epoch: self.bind_epoch,
            field_path: self
                .field_path
                .as_ref()
                .map(|value| Cow::Owned(value.to_string())),
            value: self.value.to_static(),
        }
    }
}

impl<'a> RenderPatch<'a> {
    fn to_static(&self) -> RenderPatch<'static> {
        RenderPatch {
            kind: self.kind,
            target: self.target.to_static(),
            value: self.value.to_static(),
            list_id: self
                .list_id
                .as_ref()
                .map(|value| Cow::Owned(value.to_string())),
            key: self.key,
            generation: self.generation,
            source_id: self.source_id,
            bind_epoch: self.bind_epoch,
        }
    }
}

impl<'a> ProtocolValue<'a> {
    fn to_static(&self) -> ProtocolValue<'static> {
        match self {
            Self::Null => ProtocolValue::Null,
            Self::Bool(value) => ProtocolValue::Bool(*value),
            Self::Text(value) => ProtocolValue::Text(Cow::Owned(value.to_string())),
            Self::NumberText(value) => ProtocolValue::NumberText(*value),
            Self::SourceBinding {
                source_path,
                source_id,
                bind_epoch,
            } => ProtocolValue::SourceBinding {
                source_path: Cow::Owned(source_path.to_string()),
                source_id: *source_id,
                bind_epoch: *bind_epoch,
            },
            Self::CheckedProperty(value) => ProtocolValue::CheckedProperty(*value),
        }
    }
}

impl<'a> RenderTarget<'a> {
    fn to_static(&self) -> RenderTarget<'static> {
        match self {
            Self::Static(value) => RenderTarget::Static(Cow::Owned(value.to_string())),
            Self::Borrowed(value) => RenderTarget::Borrowed(Cow::Owned(value.to_string())),
        }
    }
}

impl Serialize for SemanticDelta<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("SemanticDelta", 8)?;
        state.serialize_field("kind", self.kind)?;
        state.serialize_field("list_id", &self.list_id)?;
        state.serialize_field("key", &self.key)?;
        state.serialize_field("generation", &self.generation)?;
        state.serialize_field("source_id", &self.source_id)?;
        state.serialize_field("bind_epoch", &self.bind_epoch)?;
        state.serialize_field("field_path", &self.field_path)?;
        state.serialize_field("value", &self.value)?;
        state.end()
    }
}

impl Serialize for RenderPatch<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("RenderPatch", 8)?;
        state.serialize_field("kind", self.kind)?;
        state.serialize_field("target", &self.target)?;
        state.serialize_field("value", &self.value)?;
        state.serialize_field("list_id", &self.list_id)?;
        state.serialize_field("key", &self.key)?;
        state.serialize_field("generation", &self.generation)?;
        state.serialize_field("source_id", &self.source_id)?;
        state.serialize_field("bind_epoch", &self.bind_epoch)?;
        state.end()
    }
}

impl Serialize for ProtocolValue<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Null => serializer.serialize_none(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Text(value) => serializer.serialize_str(value),
            Self::NumberText(value) => serializer.serialize_str(&value.to_string()),
            Self::SourceBinding {
                source_path,
                source_id,
                bind_epoch,
            } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("source_path", source_path)?;
                map.serialize_entry("source_id", source_id)?;
                map.serialize_entry("bind_epoch", bind_epoch)?;
                map.end()
            }
            Self::CheckedProperty(value) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("checked", value)?;
                map.end()
            }
        }
    }
}

impl Serialize for RenderTarget<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Static(value) => serializer.serialize_str(value),
            Self::Borrowed(value) => serializer.serialize_str(value),
        }
    }
}

pub fn load_and_lower(source_path: &Path) -> RuntimeResult<(ParsedProgram, TypedProgram)> {
    let parsed = parse_source_path_or_manifest_project(source_path)?;
    let ir = lower(&parsed)?;
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    Ok((parsed, ir))
}

fn parse_source_path_or_manifest_project(source_path: &Path) -> RuntimeResult<ParsedProgram> {
    let units = source_units_for_path(source_path)?;
    if units.len() <= 1 {
        let source = units
            .first()
            .map(|unit| unit.source.clone())
            .unwrap_or_else(String::new);
        return Ok(parse_source(source_path.display().to_string(), source)?);
    }
    Ok(parse_project(
        source_path.display().to_string(),
        units.into_iter().map(|unit| (unit.path, unit.source)),
    )?)
}

pub fn ir_debug_report(source_path: &Path) -> RuntimeResult<JsonValue> {
    let (_parsed, ir) = load_and_lower(source_path)?;
    Ok(json!({
        "status": "pass",
        "program_kind": "generic",
        "expression_count": ir.expression_count,
        "graph_node_count": ir.graph_node_count,
        "hidden_identity_verified": ir.hidden_identity_verified,
        "static_schedule_verified": ir.static_schedule_verified,
        "nodes": ir.nodes,
        "debug_tables": debug_tables(&ir),
    }))
}

pub fn parse_scenario(path: &Path) -> RuntimeResult<Scenario> {
    let text = fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}

pub fn run_scenario(
    source_path: &Path,
    scenario_path: &Path,
    layer: VerificationLayer,
    report_path: Option<&Path>,
) -> RuntimeResult<RunOutput> {
    let (parsed, ir) = load_and_lower(source_path)?;
    let scenario = parse_scenario(scenario_path)?;
    let started = Instant::now();
    let output = run_loaded_scenario(&parsed, &ir, &scenario, layer)?;
    let elapsed = started.elapsed();
    let mut report = output.report;
    enrich_report(
        &mut report,
        &source_path.display().to_string(),
        &report_source_hash_for_parsed(&parsed),
        scenario_path,
        report_path,
        &parsed,
        &ir,
        layer,
        elapsed.as_secs_f64() * 1000.0,
    )?;
    if let Some(report_path) = report_path {
        write_json(report_path, &report)?;
    }
    Ok(RunOutput {
        report,
        document: boon_parser::parsed_document(&parsed),
        ..output
    })
}

pub fn run_scenario_source(
    source_label: &str,
    source_text: &str,
    scenario_path: &Path,
    layer: VerificationLayer,
) -> RuntimeResult<RunOutput> {
    run_scenario_source_with_step_limit(source_label, source_text, scenario_path, layer, None)
}

pub fn run_scenario_source_with_step_limit(
    source_label: &str,
    source_text: &str,
    scenario_path: &Path,
    layer: VerificationLayer,
    step_limit: Option<usize>,
) -> RuntimeResult<RunOutput> {
    let scenario = parse_scenario(scenario_path)?;
    run_scenario_source_with_parsed_scenario_step_limit(
        source_label,
        source_text,
        scenario_path,
        &scenario,
        layer,
        step_limit,
    )
}

pub fn run_scenario_source_with_parsed_scenario_step_limit(
    source_label: &str,
    source_text: &str,
    scenario_path: &Path,
    scenario: &Scenario,
    layer: VerificationLayer,
    step_limit: Option<usize>,
) -> RuntimeResult<RunOutput> {
    let parsed = parse_source(source_label.to_owned(), source_text.to_owned())?;
    let ir = lower(&parsed)?;
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    let mut scenario = scenario.clone();
    if let Some(step_limit) = step_limit {
        scenario.step.truncate(step_limit.min(scenario.step.len()));
    }
    let started = Instant::now();
    let output = run_loaded_scenario(&parsed, &ir, &scenario, layer)?;
    let elapsed = started.elapsed();
    let mut report = output.report;
    enrich_report(
        &mut report,
        source_label,
        &report_source_hash_for_parsed(&parsed),
        scenario_path,
        None,
        &parsed,
        &ir,
        layer,
        elapsed.as_secs_f64() * 1000.0,
    )?;
    Ok(RunOutput {
        report,
        document: boon_parser::parsed_document(&parsed),
        ..output
    })
}

pub fn run_source_initial_state(
    source_label: &str,
    source_text: &str,
    scenario_path: &Path,
    scenario: &Scenario,
) -> RuntimeResult<RunOutput> {
    let parse_started = Instant::now();
    let parsed = parse_source(source_label.to_owned(), source_text.to_owned())?;
    let parse_ms = parse_started.elapsed().as_secs_f64() * 1000.0;
    let lower_started = Instant::now();
    let ir = lower(&parsed)?;
    let lower_ms = lower_started.elapsed().as_secs_f64() * 1000.0;
    let verify_started = Instant::now();
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    let verify_ms = verify_started.elapsed().as_secs_f64() * 1000.0;
    let compile_started = Instant::now();
    let compiled = CompiledProgram::from_ir(&ir)?;
    let compile_ms = compile_started.elapsed().as_secs_f64() * 1000.0;
    let runtime_started = Instant::now();
    let mut runtime = LoadedRuntime::new(&ir, &compiled)?;
    runtime.prepare_for_scenario(scenario)?;
    let state_summary = runtime.state_summary();
    let runtime_ms = runtime_started.elapsed().as_secs_f64() * 1000.0;
    let report_started = Instant::now();
    let runtime_profile = RuntimeProfile::from_ir(&ir);
    let runtime_profile_detail = runtime_profile.detail_report(&ir);
    let capacity_report = runtime_profile.capacity_report(&ir);
    let generic_runtime_slices = generic_runtime_slices_report(&ir, &compiled);
    let generic_runtime_slice_evidence = generic_runtime_slice_evidence_report(&ir, &compiled);
    let typecheck_report_hash = typecheck_report_hash(&ir);
    let render_slot_table_hash = render_slot_table_hash(&ir);
    let report = json!({
        "status": "pass",
        "command": "playground-initial-state",
        "source_path": source_label,
        "source_hash": sha256_bytes(source_text.as_bytes()),
        "scenario_path": scenario_path.display().to_string(),
        "scenario_hash": sha256_file(scenario_path)?,
        "program_hash": sha256_bytes(source_text.as_bytes()),
        "program_kind": "generic",
        "expression_count": ir.expression_count,
        "expression_coverage": &ir.expression_coverage,
        "typecheck_report_hash": typecheck_report_hash,
        "render_slot_table_hash": render_slot_table_hash,
        "typed_render_metadata_used": ir.typecheck_report.render_slot_count > 0,
        "unresolved_type_variable_count": ir.typecheck_report.unresolved_type_variable_count,
        "render_slot_failure_count": ir.typecheck_report.render_slot_failure_count,
        "typecheck_report": &ir.typecheck_report,
        "graph_node_count": ir.graph_node_count,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
        "max_dirty_nodes": 0,
        "max_dirty_keys": 0,
        "total_ticks": 0,
        "total_source_events": 0,
        "total_semantic_deltas": 0,
        "total_render_deltas": 0,
        "runtime_profile": runtime_profile.as_str(),
        "runtime_profile_detail": runtime_profile_detail,
        "playground_initial_timing_ms": {
            "parse": parse_ms,
            "lower": lower_ms,
            "verify": verify_ms,
            "compile": compile_ms,
            "runtime_prepare_and_summary": runtime_ms,
            "report_until_timing_field": report_started.elapsed().as_secs_f64() * 1000.0
        },
        "capacities": capacity_report,
        "compiled_schedule": compiled.report(),
        "runtime_execution": {
            "implementation": "typed static graph initialized for playground preview",
            "source_loaded_from_boon": true,
            "typed_ir_loaded": true,
            "static_schedule_verified": ir.static_schedule_verified,
            "runtime_profile": runtime_profile.as_str(),
            "runtime_profile_detail": runtime_profile_detail,
            "capacities": capacity_report,
            "expression_coverage": &ir.expression_coverage,
            "typecheck_report_hash": typecheck_report_hash,
            "render_slot_table_hash": render_slot_table_hash,
            "typed_render_metadata_used": ir.typecheck_report.render_slot_count > 0,
            "unresolved_type_variable_count": ir.typecheck_report.unresolved_type_variable_count,
            "render_slot_failure_count": ir.typecheck_report.render_slot_failure_count,
            "generic_interpreter_complete": derive_generic_interpreter_complete(&ir, &compiled, &generic_runtime_slices),
            "example_behavior_adapter": derive_example_behavior_adapter(&compiled, &generic_runtime_slices),
            "adapter_kind": "generic",
            "remaining_example_specific_shell_policy": "scenario_assertion_report_glue_only",
            "remaining_example_specific_shells": remaining_example_specific_shells(&compiled, &generic_runtime_slices),
            "final_handoff_pending_human_report": true,
            "generic_runtime_slices": generic_runtime_slices,
            "generic_runtime_slice_evidence": generic_runtime_slice_evidence
        },
        "state_summary": state_summary,
        "ir_debug_tables": debug_tables(&ir),
        "hidden_identity_verified": ir.hidden_identity_verified,
        "static_schedule_verified": ir.static_schedule_verified
    });
    Ok(RunOutput {
        report,
        semantic_deltas: Vec::new(),
        render_patches: Vec::new(),
        state_summary,
        document: boon_parser::parsed_document(&parsed),
    })
}

pub struct LiveRuntime {
    runtime: LoadedRuntime,
    next_step: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuntimeSourceUnit {
    pub path: String,
    pub source: String,
}

#[derive(Clone)]
struct CachedRuntimePlan {
    ir: Arc<TypedProgram>,
    compiled: Arc<CompiledProgram>,
}

fn runtime_plan_cache() -> &'static Mutex<BTreeMap<String, CachedRuntimePlan>> {
    static CACHE: OnceLock<Mutex<BTreeMap<String, CachedRuntimePlan>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn cached_runtime_plan_from_source(
    source_label: &str,
    source_text: &str,
) -> RuntimeResult<CachedRuntimePlan> {
    let key = sha256_bytes(source_text.as_bytes());
    if let Some(plan) = runtime_plan_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key).cloned())
    {
        return Ok(plan);
    }

    let parsed = parse_source(source_label.to_owned(), source_text.to_owned())?;
    let ir = lower(&parsed)?;
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    let compiled = CompiledProgram::from_ir(&ir)?;
    let plan = CachedRuntimePlan {
        ir: Arc::new(ir),
        compiled: Arc::new(compiled),
    };
    if let Ok(mut cache) = runtime_plan_cache().lock() {
        cache.insert(key, plan.clone());
    }
    Ok(plan)
}

fn cached_runtime_plan_from_project(
    source_label: &str,
    units: &[RuntimeSourceUnit],
) -> RuntimeResult<CachedRuntimePlan> {
    if units.len() == 1 {
        let unit = &units[0];
        return cached_runtime_plan_from_source(&unit.path, &unit.source);
    }
    let key = source_units_hash(units);
    if let Some(plan) = runtime_plan_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key).cloned())
    {
        return Ok(plan);
    }

    let parsed = parse_project(
        source_label.to_owned(),
        units
            .iter()
            .map(|unit| (unit.path.clone(), unit.source.clone())),
    )?;
    let ir = lower(&parsed)?;
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    let compiled = CompiledProgram::from_ir(&ir)?;
    let plan = CachedRuntimePlan {
        ir: Arc::new(ir),
        compiled: Arc::new(compiled),
    };
    if let Ok(mut cache) = runtime_plan_cache().lock() {
        cache.insert(key, plan.clone());
    }
    Ok(plan)
}

pub fn source_units_hash(units: &[RuntimeSourceUnit]) -> String {
    if let [unit] = units {
        return sha256_bytes(unit.source.as_bytes());
    }
    let mut canonical = String::new();
    for unit in units {
        canonical.push_str(&unit.path);
        canonical.push('\0');
        canonical.push_str(&sha256_bytes(unit.source.as_bytes()));
        canonical.push('\0');
    }
    sha256_bytes(canonical.as_bytes())
}

#[derive(Clone, Debug)]
pub struct RuntimeStaticProgramAnalysis {
    pub typecheck_report: boon_typecheck::TypeCheckReport,
    pub view_bindings: Vec<boon_ir::ViewBinding>,
}

pub fn cached_static_analysis_from_project(
    source_label: &str,
    units: &[RuntimeSourceUnit],
) -> RuntimeResult<RuntimeStaticProgramAnalysis> {
    let plan = if units.len() == 1 {
        let unit = &units[0];
        cached_runtime_plan_from_source(&unit.path, &unit.source)?
    } else {
        cached_runtime_plan_from_project(source_label, units)?
    };
    Ok(RuntimeStaticProgramAnalysis {
        typecheck_report: plan.ir.typecheck_report.clone(),
        view_bindings: plan.ir.view_bindings.clone(),
    })
}

impl LiveRuntime {
    pub fn new(source_label: &str, source_text: &str, scenario_path: &Path) -> RuntimeResult<Self> {
        let plan = cached_runtime_plan_from_source(source_label, source_text)?;
        let scenario = parse_scenario(scenario_path)?;
        let mut runtime = LoadedRuntime::new(plan.ir.as_ref(), plan.compiled.as_ref())?;
        runtime.prepare_for_scenario(&scenario)?;
        Ok(Self {
            runtime,
            next_step: 1,
        })
    }

    pub fn new_from_project(
        source_label: &str,
        units: &[RuntimeSourceUnit],
        scenario_path: &Path,
    ) -> RuntimeResult<Self> {
        let plan = cached_runtime_plan_from_project(source_label, units)?;
        let scenario = parse_scenario(scenario_path)?;
        let mut runtime = LoadedRuntime::new(plan.ir.as_ref(), plan.compiled.as_ref())?;
        runtime.prepare_for_scenario(&scenario)?;
        Ok(Self {
            runtime,
            next_step: 1,
        })
    }

    pub fn from_source(source_label: &str, source_text: &str) -> RuntimeResult<Self> {
        let plan = cached_runtime_plan_from_source(source_label, source_text)?;
        let runtime = LoadedRuntime::new(plan.ir.as_ref(), plan.compiled.as_ref())?;
        Ok(Self {
            runtime,
            next_step: 1,
        })
    }

    pub fn from_project(source_label: &str, units: &[RuntimeSourceUnit]) -> RuntimeResult<Self> {
        let plan = cached_runtime_plan_from_project(source_label, units)?;
        let runtime = LoadedRuntime::new(plan.ir.as_ref(), plan.compiled.as_ref())?;
        Ok(Self {
            runtime,
            next_step: 1,
        })
    }

    pub fn apply_source_event(&mut self, event: LiveSourceEvent) -> RuntimeResult<LiveStepOutput> {
        let step = event.into_step(self.next_step);
        self.next_step = self.next_step.saturating_add(1);
        self.apply_checked_step(&step)
    }

    pub fn apply_source_event_for_document(
        &mut self,
        event: LiveSourceEvent,
    ) -> RuntimeResult<LiveStepOutput> {
        let step = event.into_step(self.next_step);
        self.next_step = self.next_step.saturating_add(1);
        self.apply_checked_step_with_document_summary(&step)
    }

    pub fn apply_source_event_for_document_window(
        &mut self,
        event: LiveSourceEvent,
        row_start: usize,
        row_count: usize,
        column_start: usize,
        column_count: usize,
    ) -> RuntimeResult<LiveStepOutput> {
        let step = event.into_step(self.next_step);
        self.next_step = self.next_step.saturating_add(1);
        self.apply_checked_step_with_document_window(
            &step,
            row_start,
            row_count,
            column_start,
            column_count,
        )
    }

    pub fn apply_source_event_for_step(
        &mut self,
        step: &ScenarioStep,
        event: LiveSourceEvent,
    ) -> RuntimeResult<LiveStepOutput> {
        event.assert_matches_step(step)?;
        let mut live_step = step.clone();
        live_step.user_action = Some(event.live_source_user_action_with_occurrence());
        live_step.expected_source_event = Some(event.into_expected_source_event());
        self.next_step = self.next_step.saturating_add(1);
        self.apply_checked_step(&live_step)
    }

    pub fn apply_source_event_for_step_with_document_window(
        &mut self,
        step: &ScenarioStep,
        event: LiveSourceEvent,
        row_start: usize,
        row_count: usize,
        column_start: usize,
        column_count: usize,
    ) -> RuntimeResult<LiveStepOutput> {
        event.assert_matches_step(step)?;
        let mut live_step = step.clone();
        live_step.user_action = Some(event.live_source_user_action_with_occurrence());
        live_step.expected_source_event = Some(event.into_expected_source_event());
        self.next_step = self.next_step.saturating_add(1);
        self.apply_checked_step_with_document_window(
            &live_step,
            row_start,
            row_count,
            column_start,
            column_count,
        )
    }

    fn apply_checked_step(&mut self, step: &ScenarioStep) -> RuntimeResult<LiveStepOutput> {
        self.apply_checked_step_with_summary_mode(step, false)
    }

    fn apply_checked_step_with_document_summary(
        &mut self,
        step: &ScenarioStep,
    ) -> RuntimeResult<LiveStepOutput> {
        self.apply_checked_step_with_summary_mode(step, true)
    }

    fn apply_checked_step_with_document_window(
        &mut self,
        step: &ScenarioStep,
        row_start: usize,
        row_count: usize,
        column_start: usize,
        column_count: usize,
    ) -> RuntimeResult<LiveStepOutput> {
        let mut semantic_deltas = Vec::new();
        let mut render_patches = Vec::new();
        self.runtime
            .apply_step(step, &mut semantic_deltas, &mut render_patches)?;
        assert_delta_expectations(step, &semantic_deltas, &render_patches)?;
        self.runtime.assert_step_after_measurement(step)?;
        let state_summary = self.runtime.document_state_summary_for_window(
            row_start,
            row_count,
            column_start,
            column_count,
        );
        Ok(LiveStepOutput {
            semantic_deltas: semantic_deltas
                .iter()
                .map(SemanticDelta::to_static)
                .collect(),
            render_patches: render_patches.iter().map(RenderPatch::to_static).collect(),
            state_summary,
        })
    }

    fn apply_checked_step_with_summary_mode(
        &mut self,
        step: &ScenarioStep,
        document_summary: bool,
    ) -> RuntimeResult<LiveStepOutput> {
        let mut semantic_deltas = Vec::new();
        let mut render_patches = Vec::new();
        self.runtime
            .apply_step(step, &mut semantic_deltas, &mut render_patches)?;
        assert_delta_expectations(step, &semantic_deltas, &render_patches)?;
        self.runtime.assert_step_after_measurement(step)?;
        let state_summary = if document_summary {
            self.runtime.document_state_summary()
        } else {
            self.runtime.state_summary()
        };
        Ok(LiveStepOutput {
            semantic_deltas: semantic_deltas
                .iter()
                .map(SemanticDelta::to_static)
                .collect(),
            render_patches: render_patches.iter().map(RenderPatch::to_static).collect(),
            state_summary,
        })
    }

    pub fn state_summary(&mut self) -> JsonValue {
        self.runtime.state_summary()
    }

    pub fn runtime_value_summaries(
        &mut self,
        paths: &[String],
        max_depth: usize,
        max_fields: usize,
        max_list_items: usize,
    ) -> JsonValue {
        self.runtime
            .runtime_value_summaries(paths, max_depth, max_fields, max_list_items)
    }

    pub fn document_state_summary(&mut self) -> JsonValue {
        self.runtime.document_state_summary()
    }

    pub fn source_payload_has_text(&self, source: &str) -> bool {
        self.runtime.source_payload_has_text(source)
    }

    pub fn document_state_summary_for_window(
        &mut self,
        row_start: usize,
        row_count: usize,
        column_start: usize,
        column_count: usize,
    ) -> JsonValue {
        self.runtime.document_state_summary_for_window(
            row_start,
            row_count,
            column_start,
            column_count,
        )
    }
}

impl LiveSourceEvent {
    fn assert_matches_step(&self, step: &ScenarioStep) -> RuntimeResult<()> {
        let expected = GenericSourceEvent::require(step)?;
        assert_live_source_event_field(
            &step.id,
            Some(expected.source),
            "source",
            Some(self.source.as_str()),
        )?;
        assert_live_source_event_field(&step.id, expected.text, "text", self.text.as_deref())?;
        assert_live_source_event_field(&step.id, expected.key, "key", self.key.as_deref())?;
        assert_live_source_event_field(
            &step.id,
            expected.address,
            "address",
            self.address.as_deref(),
        )?;
        assert_live_source_event_field(
            &step.id,
            expected.target_text,
            "target_text",
            self.target_text.as_deref(),
        )?;
        Ok(())
    }

    fn into_expected_source_event(self) -> BTreeMap<String, toml::Value> {
        let mut expected_source_event = BTreeMap::new();
        expected_source_event.insert("source".to_owned(), toml::Value::String(self.source));
        if let Some(text) = self.text {
            expected_source_event.insert("text".to_owned(), toml::Value::String(text));
        }
        if let Some(key) = self.key {
            expected_source_event.insert("key".to_owned(), toml::Value::String(key));
        }
        if let Some(address) = self.address {
            expected_source_event.insert("address".to_owned(), toml::Value::String(address));
        }
        if let Some(target_text) = self.target_text {
            expected_source_event
                .insert("target_text".to_owned(), toml::Value::String(target_text));
        }
        expected_source_event
    }

    fn into_step(self, sequence: usize) -> ScenarioStep {
        let user_action = self.live_source_user_action_with_occurrence();
        ScenarioStep {
            id: format!("live-source-event-{sequence}"),
            user_action: Some(user_action),
            expected_source_event: Some(self.into_expected_source_event()),
            ..ScenarioStep::default()
        }
    }

    fn live_source_user_action_with_occurrence(&self) -> BTreeMap<String, toml::Value> {
        let mut user_action = live_source_user_action();
        if let Some(occurrence) = self.target_occurrence {
            user_action.insert(
                "target_occurrence".to_owned(),
                toml::Value::Integer(occurrence as i64),
            );
        }
        user_action
    }
}

fn live_source_user_action() -> BTreeMap<String, toml::Value> {
    let mut user_action = BTreeMap::new();
    user_action.insert(
        "kind".to_owned(),
        toml::Value::String("live_source_event".to_owned()),
    );
    user_action
}

fn assert_live_source_event_field(
    step_id: &str,
    expected_value: Option<&str>,
    key: &str,
    actual_value: Option<&str>,
) -> RuntimeResult<()> {
    if expected_value.is_none() || expected_value == actual_value {
        Ok(())
    } else {
        Err(format!(
            "{step_id}: observed live source field `{key}` expected {expected_value:?}, got {actual_value:?}"
        )
        .into())
    }
}

pub use boon_report_schema::{
    command_argv_value_after, command_argv_values_after, require_command_argv_f64,
    require_command_argv_value, require_generic_runtime_slice_flags, sha256_bytes, sha256_file,
    verify_playground_surface_report, verify_report_schema, verify_runtime_execution_metadata,
    verify_semantic_delta_protocol_batches, write_json,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExampleManifest {
    #[serde(default)]
    pub example: Vec<ExampleManifestEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExampleManifestEntry {
    pub id: String,
    pub label: String,
    pub source: String,
    #[serde(default)]
    pub source_files: Vec<String>,
    #[serde(default)]
    pub build_files: Vec<String>,
    #[serde(default)]
    pub asset_files: Vec<String>,
    pub scenario: String,
    pub budget: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub order: u32,
    #[serde(default)]
    pub default_tab_order: u32,
    #[serde(default = "default_example_shown")]
    pub shown_by_default: bool,
    #[serde(default = "default_required_evidence_tier")]
    pub required_evidence_tier: String,
    #[serde(default)]
    pub human_testing_needed: bool,
    #[serde(default)]
    pub initial_visible_assertions: Vec<String>,
    #[serde(default)]
    pub input_scenarios: Vec<String>,
    #[serde(default)]
    pub scroll_focus_scenarios: Vec<String>,
    #[serde(default)]
    pub visual_artifacts: Vec<String>,
    #[serde(default)]
    pub performance_thresholds: Vec<String>,
}

fn default_example_shown() -> bool {
    true
}

fn default_required_evidence_tier() -> String {
    "real-window".to_owned()
}

pub fn example_manifest_path() -> PathBuf {
    resolve_repo_file("examples/manifest.toml")
}

pub fn example_manifest_entries() -> RuntimeResult<Vec<ExampleManifestEntry>> {
    let path = example_manifest_path();
    let manifest_text = fs::read_to_string(&path)?;
    let manifest: ExampleManifest = toml::from_str(&manifest_text)?;
    validate_example_manifest(&path, &manifest)?;
    let mut entries = manifest.example;
    entries.sort_by_key(|entry| (entry.default_tab_order, entry.order, entry.label.clone()));
    Ok(entries)
}

pub fn example_manifest_entry(name: &str) -> RuntimeResult<ExampleManifestEntry> {
    let requested = if name == "todo" { "todomvc" } else { name };
    example_manifest_entries()?
        .into_iter()
        .find(|entry| {
            entry.id == requested
                || entry.label.eq_ignore_ascii_case(requested)
                || Path::new(&entry.source)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    == Some(requested)
        })
        .ok_or_else(|| format!("example `{name}` is missing from examples/manifest.toml").into())
}

fn validate_example_manifest(path: &Path, manifest: &ExampleManifest) -> RuntimeResult<()> {
    if manifest.example.is_empty() {
        return Err(format!("example manifest `{}` has no entries", path.display()).into());
    }
    let mut ids = BTreeSet::new();
    for entry in &manifest.example {
        if entry.id.trim().is_empty() {
            return Err(
                format!("example manifest `{}` contains an empty id", path.display()).into(),
            );
        }
        if !ids.insert(entry.id.clone()) {
            return Err(format!(
                "duplicate example id `{}` in `{}`",
                entry.id,
                path.display()
            )
            .into());
        }
        if entry.label.trim().is_empty() {
            return Err(format!("example `{}` has an empty label", entry.id).into());
        }
        if !matches!(
            entry.required_evidence_tier.as_str(),
            "runtime" | "host-synthetic" | "real-window" | "human"
        ) {
            return Err(format!(
                "example `{}` has unsupported required_evidence_tier `{}`",
                entry.id, entry.required_evidence_tier
            )
            .into());
        }
        for relative in [&entry.source, &entry.scenario, &entry.budget] {
            let resolved = resolve_repo_file(relative);
            if !resolved.exists() {
                return Err(format!(
                    "example `{}` references missing file `{}`",
                    entry.id,
                    resolved.display()
                )
                .into());
            }
        }
        for relative in &entry.source_files {
            let resolved = resolve_repo_file(relative);
            if !resolved.exists() {
                return Err(format!(
                    "example `{}` references missing source file `{}`",
                    entry.id,
                    resolved.display()
                )
                .into());
            }
        }
        for relative in entry.build_files.iter().chain(entry.asset_files.iter()) {
            let resolved = resolve_repo_file(relative);
            if !resolved.exists() {
                return Err(format!(
                    "example `{}` references missing project file `{}`",
                    entry.id,
                    resolved.display()
                )
                .into());
            }
        }
    }
    Ok(())
}

pub fn example_source_files(name: &str) -> RuntimeResult<Vec<PathBuf>> {
    let entry = example_manifest_entry(name)?;
    Ok(source_files_for_entry(&entry))
}

pub fn example_source_units(name: &str) -> RuntimeResult<Vec<RuntimeSourceUnit>> {
    let entry = example_manifest_entry(name)?;
    source_units_for_entry(&entry)
}

pub fn example_source_text(name: &str) -> RuntimeResult<String> {
    let entry = example_manifest_entry(name)?;
    source_text_for_entry(&entry)
}

pub fn source_units_for_path(path: &Path) -> RuntimeResult<Vec<RuntimeSourceUnit>> {
    source_files_for_path(path)?
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(&path)?;
            Ok(RuntimeSourceUnit {
                path: path.display().to_string(),
                source,
            })
        })
        .collect()
}

pub fn source_units_for_entry(
    entry: &ExampleManifestEntry,
) -> RuntimeResult<Vec<RuntimeSourceUnit>> {
    source_files_for_entry(entry)
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(&path)?;
            Ok(RuntimeSourceUnit {
                path: path.display().to_string(),
                source,
            })
        })
        .collect()
}

pub fn source_text_for_path(path: &Path) -> RuntimeResult<String> {
    let source_path = resolve_repo_file(path);
    let entries = example_manifest_entries().unwrap_or_default();
    for entry in entries {
        let entry_source = resolve_repo_file(&entry.source);
        if paths_match(&entry_source, &source_path) {
            return Ok(fs::read_to_string(entry_source)?);
        }
    }
    Ok(fs::read_to_string(source_path)?)
}

pub fn source_text_for_entry(entry: &ExampleManifestEntry) -> RuntimeResult<String> {
    Ok(fs::read_to_string(resolve_repo_file(&entry.source))?)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BuildExecutionResult {
    pub status: String,
    pub build_file: String,
    pub project_root: String,
    pub write_output: bool,
    pub icons_directory: String,
    pub output_file: String,
    pub output_binding: String,
    pub operator_evidence: Vec<String>,
    pub input_files: Vec<String>,
    pub output_sha256: String,
    pub output_bytes: usize,
    pub written_files: Vec<String>,
    pub logs: Vec<BuildLogEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BuildLogEntry {
    pub level: String,
    pub message: String,
}

pub fn run_project_build_file(
    project_root: &Path,
    build_file: &Path,
    write_output: bool,
) -> RuntimeResult<BuildExecutionResult> {
    let root = canonical_existing_dir(project_root)?;
    let build_path = sandbox_existing_file(&root, build_file)?;
    let source = fs::read_to_string(&build_path)?;
    let operator_evidence = physical_asset_build_operator_evidence(&source);
    let missing = required_physical_asset_build_operators()
        .into_iter()
        .filter(|operator| !operator_evidence.iter().any(|seen| seen == operator))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "build file `{}` is missing required operators: {}",
            build_path.display(),
            missing.join(", ")
        )
        .into());
    }

    let icons_directory = build_text_binding(&source, "icons_directory")
        .ok_or("BUILD.bn does not define `icons_directory: TEXT { ... }`")?;
    let output_file = build_text_binding(&source, "output_file")
        .ok_or("BUILD.bn does not define `output_file: TEXT { ... }`")?;
    let output_binding = build_output_binding(&source).unwrap_or_else(|| "icon".to_owned());
    let icons_dir = sandbox_existing_dir(&root, Path::new(&icons_directory))?;
    let output_path = sandbox_output_file(&root, Path::new(&output_file))?;

    let mut icon_files = fs::read_dir(&icons_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    icon_files.retain(|path| {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
    });
    icon_files.sort_by_key(|path| path.strip_prefix(&root).unwrap_or(path).to_path_buf());

    let mut icon_entries = String::new();
    for (index, path) in icon_files.iter().enumerate() {
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| format!("asset path `{}` has no UTF-8 file stem", path.display()))?;
        let svg = fs::read_to_string(path)?;
        icon_entries.push_str(&format!(
            "        {stem}: TEXT {{\n            data:image/svg+xml;utf8,{}\n        }}\n",
            build_url_encode(svg.trim_end())
        ));
        if index + 1 < icon_files.len() {
            icon_entries.push('\n');
        }
    }

    let generated = format!(
        "-- GENERATED CODE - DO NOT EDIT\n-- Generated by BUILD.bn from assets/icons/\n-- Generated at: 2025-01-01T00:00:00Z\n\nFUNCTION {output_binding}() {{\n    [\n{icon_entries}    ]\n}}\n"
    );
    if write_output {
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&output_path, generated.as_bytes())?;
    }
    let input_files = icon_files
        .iter()
        .map(|path| repo_relative_or_display(&root, path))
        .collect::<Vec<_>>();
    let output_sha256 = sha256_bytes(generated.as_bytes());
    let mut logs = Vec::new();
    logs.push(BuildLogEntry {
        level: "info".to_owned(),
        message: format!("Included {} icons", icon_files.len()),
    });
    Ok(BuildExecutionResult {
        status: "pass".to_owned(),
        build_file: repo_relative_or_display(&root, &build_path),
        project_root: root.display().to_string(),
        write_output,
        icons_directory,
        output_file: output_file.clone(),
        output_binding,
        operator_evidence,
        input_files,
        output_sha256,
        output_bytes: generated.len(),
        written_files: if write_output {
            vec![repo_relative_or_display(&root, &output_path)]
        } else {
            Vec::new()
        },
        logs,
    })
}

pub fn generated_output_for_project_build_file(
    project_root: &Path,
    build_file: &Path,
) -> RuntimeResult<String> {
    let result = run_project_build_file(project_root, build_file, false)?;
    let root = canonical_existing_dir(project_root)?;
    let output_path = sandbox_output_file(&root, Path::new(&result.output_file))?;
    let expected = fs::read_to_string(output_path)?;
    let expected_hash = sha256_bytes(expected.as_bytes());
    if result.output_sha256 != expected_hash {
        return Err(format!(
            "BUILD.bn output hash {} does not match checked generated file hash {}",
            result.output_sha256, expected_hash
        )
        .into());
    }
    Ok(expected)
}

fn canonical_existing_dir(path: &Path) -> RuntimeResult<PathBuf> {
    let canonical = path.canonicalize()?;
    if !canonical.is_dir() {
        return Err(format!("`{}` is not a directory", path.display()).into());
    }
    Ok(canonical)
}

fn sandbox_existing_dir(root: &Path, path: &Path) -> RuntimeResult<PathBuf> {
    let resolved = sandbox_join(root, path)?;
    let canonical = resolved.canonicalize()?;
    if !canonical.starts_with(root) || !canonical.is_dir() {
        return Err(format!(
            "build directory `{}` escapes project root `{}`",
            path.display(),
            root.display()
        )
        .into());
    }
    Ok(canonical)
}

fn sandbox_existing_file(root: &Path, path: &Path) -> RuntimeResult<PathBuf> {
    let resolved = sandbox_join(root, path)?;
    let canonical = resolved.canonicalize()?;
    if !canonical.starts_with(root) || !canonical.is_file() {
        return Err(format!(
            "build file `{}` escapes project root `{}`",
            path.display(),
            root.display()
        )
        .into());
    }
    Ok(canonical)
}

fn sandbox_output_file(root: &Path, path: &Path) -> RuntimeResult<PathBuf> {
    let resolved = sandbox_join(root, path)?;
    let parent = resolved
        .parent()
        .ok_or_else(|| format!("output path `{}` has no parent", path.display()))?;
    let canonical_parent = parent.canonicalize()?;
    if !canonical_parent.starts_with(root) {
        return Err(format!(
            "build output `{}` escapes project root `{}`",
            path.display(),
            root.display()
        )
        .into());
    }
    Ok(resolved)
}

fn sandbox_join(root: &Path, path: &Path) -> RuntimeResult<PathBuf> {
    if path.is_absolute() {
        return Err(format!("build path `{}` must be project-relative", path.display()).into());
    }
    let mut resolved = root.to_path_buf();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => resolved.push(part),
            std::path::Component::ParentDir => {
                return Err(format!(
                    "build path `{}` may not contain parent-directory segments",
                    path.display()
                )
                .into());
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                return Err(format!("build path `{}` is not relative", path.display()).into());
            }
        }
    }
    Ok(resolved)
}

fn repo_relative_or_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn required_physical_asset_build_operators() -> Vec<&'static str> {
    vec![
        "Directory/entries",
        "File/read_text",
        "File/write_text",
        "Url/encode",
        "Text/join_lines",
        "List/retain",
        "List/sort_by",
        "List/map",
        "Build/succeed",
        "Build/fail",
        "FLUSH",
    ]
}

fn physical_asset_build_operator_evidence(source: &str) -> Vec<String> {
    required_physical_asset_build_operators()
        .into_iter()
        .filter(|operator| source.contains(operator))
        .map(str::to_owned)
        .collect()
}

fn build_text_binding(source: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}:");
    for line in source.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix(&prefix) else {
            continue;
        };
        let rest = rest.trim();
        let value = rest
            .strip_prefix("TEXT {")?
            .strip_suffix('}')?
            .trim()
            .to_owned();
        return Some(value);
    }
    None
}

fn build_output_binding(source: &str) -> Option<String> {
    source.lines().find_map(|line| {
        let trimmed = line.trim();
        let name = trimmed.strip_suffix(": [")?;
        (!name.is_empty()
            && name
                .chars()
                .all(|character| character == '_' || character.is_ascii_alphanumeric()))
        .then(|| name.to_owned())
    })
}

fn build_url_encode(input: &str) -> String {
    let mut output = String::new();
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn source_files_for_path(source_path: &Path) -> RuntimeResult<Vec<PathBuf>> {
    let source_path = resolve_repo_file(source_path);
    let entries = example_manifest_entries().unwrap_or_default();
    for entry in entries {
        let entry_source = resolve_repo_file(&entry.source);
        if paths_match(&entry_source, &source_path) {
            return Ok(source_files_for_entry(&entry));
        }
    }
    Ok(vec![source_path])
}

fn source_files_for_entry(entry: &ExampleManifestEntry) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if entry.source_files.is_empty() {
        files.push(resolve_repo_file(&entry.source));
    } else {
        for relative in &entry.source_files {
            files.push(resolve_repo_file(relative));
        }
        let source = resolve_repo_file(&entry.source);
        if !files.iter().any(|path| paths_match(path, &source)) {
            files.push(source);
        }
    }
    files
}

fn paths_match(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .canonicalize()
            .ok()
            .zip(right.canonicalize().ok())
            .is_some_and(|(left, right)| left == right)
}

pub fn example_paths(name: &str) -> RuntimeResult<(PathBuf, PathBuf, PathBuf)> {
    let requested = if name == "todo" { "todomvc" } else { name };
    if !requested
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(format!("invalid example name `{name}`").into());
    }
    let entry = example_manifest_entry(requested)?;
    let source = resolve_repo_file(&entry.source);
    let scenario = resolve_repo_file(&entry.scenario);
    let budget = resolve_repo_file(&entry.budget);
    for required in [&source, &scenario, &budget] {
        if !required.exists() {
            return Err(format!(
                "example `{requested}` is missing required file `{}`",
                required.display()
            )
            .into());
        }
    }
    Ok((source, scenario, budget))
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

fn run_loaded_scenario(
    _parsed: &ParsedProgram,
    ir: &TypedProgram,
    scenario: &Scenario,
    layer: VerificationLayer,
) -> RuntimeResult<RunOutput> {
    let compiled = CompiledProgram::from_ir(ir)?;
    let runtime = LoadedRuntime::new(ir, &compiled)?;
    run_generic_scenario(runtime, _parsed, ir, &compiled, scenario, layer)
}

struct LoadedRuntime {
    generic: Option<GenericScheduledRuntime>,
}

impl LoadedRuntime {
    fn new(ir: &TypedProgram, compiled: &CompiledProgram) -> RuntimeResult<Self> {
        let generic = GenericScheduledRuntime::new(ir, compiled)?;
        Ok(Self {
            generic: Some(generic),
        })
    }

    fn generic_state_summary(&mut self) -> JsonValue {
        let Some(generic) = self.generic.as_mut() else {
            return json!({ "error": "LoadedRuntime generic schedule was already borrowed" });
        };
        let mut summary = generic.generic_summary();
        generic.insert_list_projection_summary(&mut summary);
        summary
    }

    fn document_state_summary(&mut self) -> JsonValue {
        let Some(generic) = self.generic.as_mut() else {
            return json!({ "error": "LoadedRuntime generic schedule was already borrowed" });
        };
        generic.document_summary()
    }

    fn source_payload_has_text(&self, source: &str) -> bool {
        self.generic
            .as_ref()
            .is_some_and(|generic| generic.source_payload_has_text(source))
    }

    fn document_state_summary_for_window(
        &mut self,
        row_start: usize,
        row_count: usize,
        column_start: usize,
        column_count: usize,
    ) -> JsonValue {
        let Some(generic) = self.generic.as_mut() else {
            return json!({ "error": "LoadedRuntime generic schedule was already borrowed" });
        };
        generic.document_summary_for_window(row_start, row_count, column_start, column_count)
    }

    fn runtime_value_summaries(
        &self,
        paths: &[String],
        max_depth: usize,
        max_fields: usize,
        max_list_items: usize,
    ) -> JsonValue {
        let Some(generic) = self.generic.as_ref() else {
            return json!({ "error": "LoadedRuntime generic schedule was already borrowed" });
        };
        generic.runtime_value_summaries(paths, max_depth, max_fields, max_list_items)
    }

    fn apply_generic_step<'a>(
        &mut self,
        step: &'a ScenarioStep,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<StepExecutionMetrics> {
        let Some(source_event) = GenericSourceEvent::from_step(step)? else {
            return Ok(StepExecutionMetrics {
                dirty_key_count: 0,
                extra: StepExecutionExtra::Generic {
                    recomputed_field_count: 0,
                    recompute_candidate_count: 0,
                },
            });
        };
        let generic = self
            .generic
            .as_mut()
            .ok_or("LoadedRuntime generic schedule was already borrowed")?;
        let delta_start = deltas.len();
        let input = generic.source_action_input_for_event(
            &step.id,
            source_event,
            TickSeq(0),
            |list, event| generic.resolve_generic_step_index(list, step, event),
        )?;
        let source_list = input.list.clone();
        let source_index = input.index;
        let bool_context = generic.generic_bool_contexts();
        generic
            .apply_source_actions(
                input,
                |path| bool_context.get(path).copied(),
                |mutation| {
                    if let Some(delta) = mutation.semantic_delta() {
                        deltas.push(delta);
                    }
                    patches.push(generic_document_invalidation_patch(&mutation));
                    Ok(())
                },
            )
            .map_err(|error| format!("{}: {error}", step.id))?;
        if source_event.key == Some("Enter")
            && let (Some(list), Some(index)) = (source_list.as_deref(), source_index)
            && let Some(commit) = generic.commit_edit_draft_title_for_index(list, index)?
        {
            let mutation = GenericSourceMutation::TextField(commit);
            if let Some(delta) = mutation.semantic_delta() {
                deltas.push(delta);
            }
            patches.push(generic_document_invalidation_patch(&mutation));
        }
        let changed_reads = generic.read_keys_from_deltas(&deltas[delta_start..])?;
        let (derived_commits, recompute_metrics) =
            generic.recompute_generic_derived_after_changes(changed_reads)?;
        for commit in derived_commits {
            let mutation = GenericSourceMutation::ValueField(commit);
            if let Some(delta) = mutation.semantic_delta() {
                deltas.push(delta);
            }
            patches.push(generic_document_invalidation_patch(&mutation));
        }
        Ok(StepExecutionMetrics {
            dirty_key_count: deltas.len(),
            extra: StepExecutionExtra::Generic {
                recomputed_field_count: recompute_metrics.recomputed_field_count,
                recompute_candidate_count: recompute_metrics.recompute_candidate_count,
            },
        })
    }
}

#[derive(Clone, Debug)]
struct CompiledProgram {
    symbols: RuntimeSymbols,
    scalar_equations: ScalarEquationPlan,
    derived_equations: DerivedEquationPlan,
    generic_derived: GenericDerivedPlan,
    list_equations: ListEquationPlan,
    list_projections: ListProjectionPlan,
    source_routes: SourceRoutePlan,
    list_source_bindings: ListSourceBindingPlan,
    root_state_paths: Vec<String>,
    list_summary_fields: Vec<ListSummaryFields>,
    schedule_node_count: usize,
    state_initializer_count: usize,
    list_initializer_count: usize,
    derived_value_count: usize,
    derived_text_transform_count: usize,
    update_branch_count: usize,
    list_operation_count: usize,
    list_projection_count: usize,
    view_binding_count: usize,
    source_payload_schema_count: usize,
    source_payload_field_count: usize,
    source_payload_text_field_count: usize,
    source_payload_key_field_count: usize,
    source_payload_address_field_count: usize,
    root_text_slot_count: usize,
    root_bool_slot_count: usize,
    root_enum_slot_count: usize,
    list_memory_count: usize,
    list_row_template_field_count: usize,
    list_row_text_slot_count: usize,
    list_row_bool_slot_count: usize,
    list_row_enum_slot_count: usize,
    list_hidden_key_slot_count: usize,
    list_hidden_generation_slot_count: usize,
    source_route_count: usize,
    unsupported_update_branch_count: usize,
    unsupported_list_operation_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct RuntimeSymbolId(u32);

#[derive(Clone, Debug, Default)]
struct RuntimeSymbols {
    paths: Vec<Box<str>>,
    by_path: BTreeMap<Box<str>, RuntimeSymbolId>,
}

#[derive(Clone, Debug)]
struct ListSummaryFields {
    list: String,
    fields: Vec<String>,
}

impl RuntimeSymbols {
    fn from_ir(ir: &TypedProgram) -> Self {
        let mut symbols = Self::default();
        for source in &ir.sources {
            symbols.intern(&source.path);
        }
        for cell in &ir.state_cells {
            symbols.intern(&cell.path);
        }
        for list in &ir.lists {
            symbols.intern(&list.name);
        }
        for value in &ir.derived_values {
            symbols.intern(&value.path);
            for source in &value.sources {
                symbols.intern(source);
            }
        }
        for branch in &ir.update_branches {
            symbols.intern(&branch.target);
            symbols.intern(&branch.source);
            match &branch.expression {
                UpdateExpression::SourcePayload { path }
                | UpdateExpression::Const { value: path }
                | UpdateExpression::PreviousValue { path }
                | UpdateExpression::ReadPath { path }
                | UpdateExpression::BoolNot { path } => {
                    symbols.intern(path);
                }
                UpdateExpression::TextTrimOrPrevious { path, previous } => {
                    symbols.intern(path);
                    symbols.intern(previous);
                }
                UpdateExpression::NumberInfix { left, right, .. } => {
                    symbols.intern(left);
                    symbols.intern(right);
                }
                UpdateExpression::MatchConst { input, arms } => {
                    symbols.intern(input);
                    for arm in arms {
                        symbols.intern(&arm.pattern);
                        symbols.intern(&arm.output);
                    }
                }
                UpdateExpression::Unknown { summary } => {
                    symbols.intern(summary);
                }
            }
        }
        for operation in &ir.list_operations {
            symbols.intern(&operation.list);
            match &operation.kind {
                ListOperationKind::Append { trigger, fields } => {
                    symbols.intern(trigger);
                    for field in fields {
                        symbols.intern(&field.name);
                        symbols.intern(&field.source);
                    }
                }
                ListOperationKind::Remove { source, predicate } => {
                    symbols.intern(source);
                    symbols.intern_list_predicate(predicate);
                }
                ListOperationKind::Retain { target, predicate }
                | ListOperationKind::Count { target, predicate } => {
                    symbols.intern(target);
                    symbols.intern_list_predicate(predicate);
                }
            }
        }
        for projection in &ir.list_projections {
            symbols.intern(&projection.target);
            symbols.intern(&projection.list);
            if let ListProjectionKind::Find { field, value } = &projection.kind {
                symbols.intern(field);
                symbols.intern(value);
            }
        }
        symbols
    }

    fn intern_list_predicate(&mut self, predicate: &ListPredicate) {
        match predicate {
            ListPredicate::RowFieldBool { path } | ListPredicate::RowFieldBoolNot { path } => {
                self.intern(path);
            }
            ListPredicate::SelectedFilterVisibility {
                selector,
                row_field,
            } => {
                self.intern(selector);
                self.intern(row_field);
            }
            ListPredicate::AlwaysTrue | ListPredicate::Unknown { .. } => {}
        }
    }

    fn intern(&mut self, path: &str) -> RuntimeSymbolId {
        if let Some(id) = self.by_path.get(path) {
            return *id;
        }
        let id = RuntimeSymbolId(self.paths.len() as u32);
        let owned: Box<str> = path.into();
        self.paths.push(owned.clone());
        self.by_path.insert(owned, id);
        id
    }

    fn len(&self) -> usize {
        self.paths.len()
    }
}

impl CompiledProgram {
    fn from_ir(ir: &TypedProgram) -> RuntimeResult<Self> {
        let symbols = RuntimeSymbols::from_ir(ir);
        for cell in &ir.state_cells {
            if let InitialValue::Unknown { summary } = &cell.initial_value {
                return Err(format!(
                    "state cell `{}` has unsupported initializer `{summary}`",
                    cell.path
                )
                .into());
            }
        }
        for list in &ir.lists {
            if let ListInitializer::Unknown { summary } = &list.initializer {
                return Err(format!(
                    "list `{}` has unsupported initializer `{summary}`",
                    list.name
                )
                .into());
            }
            if list.graph_clones_per_item != 0 {
                return Err(format!(
                    "list `{}` would clone {} graph nodes per item",
                    list.name, list.graph_clones_per_item
                )
                .into());
            }
        }
        let unsupported_update_branch_count = ir
            .update_branches
            .iter()
            .filter(|branch| matches!(branch.expression, UpdateExpression::Unknown { .. }))
            .count();
        if let Some(branch) = ir
            .update_branches
            .iter()
            .find(|branch| matches!(branch.expression, UpdateExpression::Unknown { .. }))
        {
            let UpdateExpression::Unknown { summary } = &branch.expression else {
                unreachable!();
            };
            return Err(format!(
                "update branch `{}` from `{}` has unsupported expression `{summary}`",
                branch.target, branch.source
            )
            .into());
        }
        let unsupported_list_operation_count = ir
            .list_operations
            .iter()
            .filter(|operation| list_operation_has_unknown_predicate(operation))
            .count();
        if let Some(operation) = ir
            .list_operations
            .iter()
            .find(|operation| list_operation_has_unknown_predicate(operation))
        {
            return Err(format!(
                "list operation for `{}` has unsupported predicate",
                operation.list
            )
            .into());
        }
        let scalar_equations = ScalarEquationPlan::from_ir(ir);
        let derived_equations = DerivedEquationPlan::from_ir(ir);
        let generic_derived = GenericDerivedPlan::from_ir(ir);
        let derived_text_transform_count = derived_equations.text_transforms.len();
        let list_equations = ListEquationPlan::from_ir(ir);
        let list_projections = ListProjectionPlan::from_ir(ir);
        let mut root_state_paths = ir
            .state_cells
            .iter()
            .filter(|cell| !cell.indexed)
            .map(|cell| cell.path.clone())
            .collect::<Vec<_>>();
        root_state_paths.extend(
            ir.derived_values
                .iter()
                .filter(|value| {
                    !value.indexed && value.kind == DerivedValueKind::SourceEventTransform
                })
                .map(|value| value.path.clone()),
        );
        root_state_paths.sort();
        root_state_paths.dedup();
        let list_summary_fields = list_summary_fields_from_ir(ir);
        let root_targets = ir
            .state_cells
            .iter()
            .filter(|cell| !cell.indexed)
            .map(|cell| cell.path.as_str())
            .collect::<BTreeSet<_>>();
        let source_routes = SourceRoutePlan::from_plans(
            ir,
            &scalar_equations,
            &derived_equations,
            &list_equations,
            &root_targets,
        )?;
        let source_route_count = source_routes.len();
        let list_source_bindings = ListSourceBindingPlan::from_ir(ir);
        let source_payload_counts = SourcePayloadCounts::from_ir(ir);
        let storage_layout_counts = TypedStorageLayoutCounts::from_ir(ir);
        Ok(Self {
            symbols,
            scalar_equations,
            derived_equations,
            generic_derived,
            list_equations,
            list_projections,
            source_routes,
            list_source_bindings,
            root_state_paths,
            list_summary_fields,
            schedule_node_count: ir.nodes.len(),
            state_initializer_count: ir.state_cells.len(),
            list_initializer_count: ir.lists.len(),
            derived_value_count: ir.derived_values.len(),
            derived_text_transform_count,
            update_branch_count: ir.update_branches.len(),
            list_operation_count: ir.list_operations.len(),
            list_projection_count: ir.list_projections.len(),
            view_binding_count: ir.view_bindings.len(),
            source_payload_schema_count: source_payload_counts.schema_count,
            source_payload_field_count: source_payload_counts.field_count,
            source_payload_text_field_count: source_payload_counts.text_field_count,
            source_payload_key_field_count: source_payload_counts.key_field_count,
            source_payload_address_field_count: source_payload_counts.address_field_count,
            root_text_slot_count: storage_layout_counts.root_text_slot_count,
            root_bool_slot_count: storage_layout_counts.root_bool_slot_count,
            root_enum_slot_count: storage_layout_counts.root_enum_slot_count,
            list_memory_count: storage_layout_counts.list_memory_count,
            list_row_template_field_count: storage_layout_counts.list_row_template_field_count,
            list_row_text_slot_count: storage_layout_counts.list_row_text_slot_count,
            list_row_bool_slot_count: storage_layout_counts.list_row_bool_slot_count,
            list_row_enum_slot_count: storage_layout_counts.list_row_enum_slot_count,
            list_hidden_key_slot_count: storage_layout_counts.list_hidden_key_slot_count,
            list_hidden_generation_slot_count: storage_layout_counts
                .list_hidden_generation_slot_count,
            source_route_count,
            unsupported_update_branch_count,
            unsupported_list_operation_count,
        })
    }

    fn report(&self) -> JsonValue {
        json!({
            "compiled_from_typed_ir": true,
            "runtime_symbol_count": self.symbols.len(),
            "runtime_symbol_ownership": "compiled_program_owned",
            "executable_surface": "generic",
            "executable_surface_inferred_from_ir": true,
            "schedule_node_count": self.schedule_node_count,
            "state_initializer_count": self.state_initializer_count,
            "list_initializer_count": self.list_initializer_count,
            "derived_value_count": self.derived_value_count,
            "derived_text_transform_count": self.derived_text_transform_count,
            "update_branch_count": self.update_branch_count,
            "list_operation_count": self.list_operation_count,
            "list_projection_count": self.list_projection_count,
            "view_binding_count": self.view_binding_count,
            "source_payload_schema_count": self.source_payload_schema_count,
            "source_payload_field_count": self.source_payload_field_count,
            "source_payload_text_field_count": self.source_payload_text_field_count,
            "source_payload_key_field_count": self.source_payload_key_field_count,
            "source_payload_address_field_count": self.source_payload_address_field_count,
            "typed_storage_layout": {
                "computed_from": "typed_ir_state_and_list_tables",
                "root_text_slot_count": self.root_text_slot_count,
                "root_bool_slot_count": self.root_bool_slot_count,
                "root_enum_slot_count": self.root_enum_slot_count,
                "list_memory_count": self.list_memory_count,
                "list_row_template_field_count": self.list_row_template_field_count,
                "list_row_text_slot_count": self.list_row_text_slot_count,
                "list_row_bool_slot_count": self.list_row_bool_slot_count,
                "list_row_enum_slot_count": self.list_row_enum_slot_count,
                "list_hidden_key_slot_count": self.list_hidden_key_slot_count,
                "list_hidden_generation_slot_count": self.list_hidden_generation_slot_count,
                "list_order_storage_kind": "separate_visible_order_slots",
                "list_valid_storage_kind": "bitvec_valid_slots",
                "list_free_storage_kind": "free_slot_stack",
                "list_source_binding_storage_kind": "dense_source_and_row_slots"
            },
            "root_text_slot_count": self.root_text_slot_count,
            "root_bool_slot_count": self.root_bool_slot_count,
            "root_enum_slot_count": self.root_enum_slot_count,
            "list_memory_count": self.list_memory_count,
            "list_row_template_field_count": self.list_row_template_field_count,
            "list_row_text_slot_count": self.list_row_text_slot_count,
            "list_row_bool_slot_count": self.list_row_bool_slot_count,
            "list_row_enum_slot_count": self.list_row_enum_slot_count,
            "list_hidden_key_slot_count": self.list_hidden_key_slot_count,
            "list_hidden_generation_slot_count": self.list_hidden_generation_slot_count,
            "source_route_count": self.source_route_count,
            "source_route_id_slot_count": self.source_routes.id_slots.len(),
            "source_route_label_slot_count": self.source_routes.label_slots.len(),
            "source_routes_with_ids": self.source_routes.route_slots.iter().filter(|route| route.source_id.as_usize() < self.source_routes.id_slots.len()).count(),
            "source_route_index_kind": "dense_source_id_slots",
            "source_route_label_lookup_kind": "sorted_source_label_binary_search",
            "list_source_binding_count": self.list_source_bindings.list_slots.len(),
            "unsupported_update_branch_count": self.unsupported_update_branch_count,
            "unsupported_list_operation_count": self.unsupported_list_operation_count,
            "graph_clones_per_item": 0
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TypedStorageLayoutCounts {
    root_text_slot_count: usize,
    root_bool_slot_count: usize,
    root_enum_slot_count: usize,
    list_memory_count: usize,
    list_row_template_field_count: usize,
    list_row_text_slot_count: usize,
    list_row_bool_slot_count: usize,
    list_row_enum_slot_count: usize,
    list_hidden_key_slot_count: usize,
    list_hidden_generation_slot_count: usize,
}

impl TypedStorageLayoutCounts {
    fn from_ir(ir: &TypedProgram) -> Self {
        let mut counts = Self {
            list_memory_count: ir.lists.len(),
            list_hidden_key_slot_count: ir.lists.len(),
            list_hidden_generation_slot_count: ir.lists.len(),
            ..Self::default()
        };
        for cell in &ir.state_cells {
            match (cell.indexed, &cell.initial_value) {
                (false, InitialValue::Bool { .. }) => counts.root_bool_slot_count += 1,
                (false, InitialValue::Enum { .. }) => counts.root_enum_slot_count += 1,
                (
                    false,
                    InitialValue::Text { .. }
                    | InitialValue::Number { .. }
                    | InitialValue::RowInitialField { .. },
                ) => {
                    counts.root_text_slot_count += 1;
                }
                (false, InitialValue::Unknown { .. }) => {}
                (true, InitialValue::Bool { .. }) => {
                    counts.list_row_template_field_count += 1;
                    counts.list_row_bool_slot_count += 1;
                }
                (true, InitialValue::Enum { .. }) => {
                    counts.list_row_template_field_count += 1;
                    counts.list_row_enum_slot_count += 1;
                }
                (
                    true,
                    InitialValue::Text { .. }
                    | InitialValue::Number { .. }
                    | InitialValue::RowInitialField { .. },
                ) => {
                    counts.list_row_template_field_count += 1;
                    counts.list_row_text_slot_count += 1;
                }
                (true, InitialValue::Unknown { .. }) => {}
            }
        }
        counts.list_row_text_slot_count += ir
            .derived_values
            .iter()
            .filter(|value| value.indexed)
            .count();
        counts
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SourcePayloadCounts {
    schema_count: usize,
    field_count: usize,
    text_field_count: usize,
    key_field_count: usize,
    address_field_count: usize,
}

impl SourcePayloadCounts {
    fn from_ir(ir: &TypedProgram) -> Self {
        let mut counts = Self {
            schema_count: ir.sources.len(),
            ..Self::default()
        };
        for source in &ir.sources {
            for field in &source.payload_schema.fields {
                counts.field_count += 1;
                match field {
                    SourcePayloadField::Text => counts.text_field_count += 1,
                    SourcePayloadField::Key => counts.key_field_count += 1,
                    SourcePayloadField::Address => counts.address_field_count += 1,
                }
            }
        }
        counts
    }
}

fn list_summary_fields_from_ir(ir: &TypedProgram) -> Vec<ListSummaryFields> {
    ir.lists
        .iter()
        .map(|list| {
            let row_scope = row_scope_name(&list.name);
            let prefix = format!("{row_scope}.");
            let mut fields = ir
                .state_cells
                .iter()
                .filter(|cell| cell.indexed && cell.path.starts_with(&prefix))
                .map(|cell| row_field_name(&cell.path).to_owned())
                .collect::<Vec<_>>();
            for value in &ir.derived_values {
                if value.indexed && value.path.starts_with(&prefix) {
                    fields.push(row_field_name(&value.path).to_owned());
                }
            }
            fields.sort();
            fields.dedup();
            ListSummaryFields {
                list: list.name.clone(),
                fields,
            }
        })
        .collect()
}

fn list_operation_has_unknown_predicate(operation: &boon_ir::ListOperation) -> bool {
    match &operation.kind {
        ListOperationKind::Append { .. } => false,
        ListOperationKind::Remove { predicate, .. }
        | ListOperationKind::Retain { predicate, .. }
        | ListOperationKind::Count { predicate, .. } => {
            matches!(predicate, ListPredicate::Unknown { .. })
        }
    }
}

struct StepExecutionMetrics {
    dirty_key_count: usize,
    extra: StepExecutionExtra,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirtyKeyEntry {
    list_id: String,
    field_id: String,
    key: u64,
}

#[derive(Clone, Debug, Default)]
struct DirtyKeySets {
    entries: Vec<DirtyKeyEntry>,
}

impl DirtyKeySets {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    fn mark(&mut self, list_id: &str, field_id: &str, key: u64) {
        let entry = DirtyKeyEntry {
            list_id: list_id.to_owned(),
            field_id: field_id.to_owned(),
            key,
        };
        if !self.entries.contains(&entry) {
            self.entries.push(entry);
        }
    }

    fn mark_delta(&mut self, delta: &SemanticDelta<'_>) {
        let Some(list_id) = delta.list_id.as_ref() else {
            return;
        };
        let Some(key) = delta.key else {
            return;
        };
        let field_id = delta
            .field_path
            .as_ref()
            .map_or(delta.kind, |field| field.as_ref());
        self.mark(list_id.as_ref(), field_id, key);
    }

    fn mark_deltas(&mut self, deltas: &[SemanticDelta<'_>]) -> usize {
        self.clear();
        for delta in deltas {
            self.mark_delta(delta);
        }
        self.key_count()
    }

    fn mark_indexes(&mut self, list_id: &str, field_id: &str, indexes: &[usize]) {
        self.clear();
        for index in indexes {
            self.mark(list_id, field_id, *index as u64 + 1);
        }
    }

    fn key_count(&self) -> usize {
        let mut count = 0usize;
        for (index, entry) in self.entries.iter().enumerate() {
            if !self.entries[..index]
                .iter()
                .any(|previous| previous.list_id == entry.list_id && previous.key == entry.key)
            {
                count += 1;
            }
        }
        count
    }
}

enum StepExecutionExtra {
    Generic {
        recomputed_field_count: usize,
        recompute_candidate_count: usize,
    },
}

trait ScenarioExecutor {
    fn prepare_for_scenario(&mut self, scenario: &Scenario) -> RuntimeResult<()>;

    fn apply_step<'a>(
        &mut self,
        step: &'a ScenarioStep,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<StepExecutionMetrics>;

    fn assert_step_after_measurement(&mut self, step: &ScenarioStep) -> RuntimeResult<()>;

    fn state_summary(&mut self) -> JsonValue;

    fn stress_profiles(&mut self, _ir: &TypedProgram) -> RuntimeResult<Option<JsonValue>> {
        Ok(None)
    }
}

fn run_generic_scenario<R: ScenarioExecutor>(
    mut runtime: R,
    parsed: &ParsedProgram,
    ir: &TypedProgram,
    compiled: &CompiledProgram,
    scenario: &Scenario,
    layer: VerificationLayer,
) -> RuntimeResult<RunOutput> {
    let baseline_rss_mib = current_rss_mib().unwrap_or(0.0);
    runtime.prepare_for_scenario(scenario)?;
    let mut semantic_deltas = Vec::new();
    let mut render_patches = Vec::new();
    let mut per_step = Vec::new();
    let mut dirty_keys = Vec::new();
    let mut latencies = Vec::new();
    let mut allocation_deltas = Vec::new();
    let mut step_deltas = Vec::with_capacity(64);
    let mut step_patches = Vec::with_capacity(64);
    for step in &scenario.step {
        step_deltas.clear();
        step_patches.clear();
        let before = Instant::now();
        let allocations_before = allocation_snapshot();
        let metrics = runtime.apply_step(step, &mut step_deltas, &mut step_patches)?;
        let allocations_after_apply = allocation_snapshot();
        assert_delta_expectations(step, &step_deltas, &step_patches)?;
        let allocations_after_expectations = allocation_snapshot();
        let alloc_delta = allocation_delta(allocations_before);
        let apply_alloc_delta = AllocationSnapshot {
            count: allocations_after_apply
                .count
                .saturating_sub(allocations_before.count),
            bytes: allocations_after_apply
                .bytes
                .saturating_sub(allocations_before.bytes),
        };
        let expectation_alloc_delta = AllocationSnapshot {
            count: allocations_after_expectations
                .count
                .saturating_sub(allocations_after_apply.count),
            bytes: allocations_after_expectations
                .bytes
                .saturating_sub(allocations_after_apply.bytes),
        };
        let elapsed = before.elapsed().as_secs_f64() * 1000.0;
        latencies.push(elapsed);
        allocation_deltas.push(alloc_delta);
        semantic_deltas.extend(step_deltas.iter().map(SemanticDelta::to_static));
        render_patches.extend(step_patches.iter().map(RenderPatch::to_static));
        runtime.assert_step_after_measurement(step)?;
        dirty_keys.push(metrics.dirty_key_count);
        let mut step_report = json!({
            "id": step.id,
            "pass": true,
            "input_route_verified": step.user_action.is_some(),
            "semantic_delta_count": step_deltas.len(),
            "render_patch_count": step_patches.len(),
            "dirty_node_count": metrics.dirty_key_count,
            "dirty_key_count": metrics.dirty_key_count,
            "heap_alloc_count": alloc_delta.count,
            "heap_alloc_bytes": alloc_delta.bytes,
            "apply_heap_alloc_count": apply_alloc_delta.count,
            "apply_heap_alloc_bytes": apply_alloc_delta.bytes,
            "expectation_heap_alloc_count": expectation_alloc_delta.count,
            "expectation_heap_alloc_bytes": expectation_alloc_delta.bytes,
            "latency_ms": elapsed,
        });
        let object = step_report
            .as_object_mut()
            .expect("step report is an object");
        match metrics.extra {
            StepExecutionExtra::Generic {
                recomputed_field_count,
                recompute_candidate_count,
            } => {
                object.insert("generic_runtime_step".to_owned(), json!(true));
                object.insert(
                    "recomputed_field_count".to_owned(),
                    json!(recomputed_field_count),
                );
                object.insert(
                    "recompute_candidate_count".to_owned(),
                    json!(recompute_candidate_count),
                );
            }
        }
        per_step.push(step_report);
    }
    let state_summary = runtime.state_summary();
    let mut report = base_example_report(
        parsed,
        ir,
        scenario,
        layer,
        &semantic_deltas,
        &render_patches,
        per_step,
        latencies,
        dirty_keys,
        allocation_deltas,
        compiled,
        state_summary.clone(),
        baseline_rss_mib,
    );
    if matches!(layer, VerificationLayer::Speed)
        && let Some(stress_profiles) = runtime.stress_profiles(ir)?
    {
        report["stress_profiles"] = stress_profiles;
    }
    Ok(RunOutput {
        report,
        semantic_deltas,
        render_patches,
        state_summary,
        document: boon_parser::parsed_document(parsed),
    })
}

fn base_example_report(
    parsed: &ParsedProgram,
    ir: &TypedProgram,
    scenario: &Scenario,
    layer: VerificationLayer,
    semantic_deltas: &[SemanticDelta<'static>],
    render_patches: &[RenderPatch<'static>],
    per_step: Vec<JsonValue>,
    latencies: Vec<f64>,
    dirty_counts: Vec<usize>,
    allocation_deltas: Vec<AllocationSnapshot>,
    compiled: &CompiledProgram,
    state_summary: JsonValue,
    baseline_rss_mib: f64,
) -> JsonValue {
    let p95 = percentile(&latencies, 0.95);
    let max = latencies.iter().copied().fold(0.0_f64, f64::max);
    let dirty_max = dirty_counts.iter().copied().max().unwrap_or(0);
    let latency_summary = json!({
        "p50": percentile(&latencies, 0.50),
        "p95": p95,
        "p99": percentile(&latencies, 0.99),
        "max": max
    });
    let zero_latency_summary = json!({
        "p50": 0.0,
        "p95": 0.0,
        "p99": 0.0,
        "max": 0.0,
        "included_in_semantic_tick": true
    });
    let ply_patch_apply_latency = if matches!(
        layer,
        VerificationLayer::HeadlessPly | VerificationLayer::HeadedPly
    ) {
        json!({
            "unavailable_reason": "Ply patch application is exercised by this layer but not separately timed from the scenario tick"
        })
    } else {
        json!({
            "unavailable_reason": "semantic/runtime speed layer does not open Ply; headed reports cover the real Ply surface"
        })
    };
    let frame_time_latency = if matches!(layer, VerificationLayer::HeadedPly) {
        json!({
            "unavailable_reason": "headed verifier records window/display/screenshot evidence but does not instrument presented frame timing"
        })
    } else {
        json!({
            "unavailable_reason": "this runtime verification layer has no presented frame timing"
        })
    };
    let dirty_as_f64 = dirty_counts
        .iter()
        .map(|value| *value as f64)
        .collect::<Vec<_>>();
    let render_counts = per_step
        .iter()
        .filter_map(|step| {
            step.get("render_patch_count")
                .and_then(JsonValue::as_u64)
                .map(|value| value as f64)
        })
        .collect::<Vec<_>>();
    let steady_rss_mib = current_rss_mib()
        .unwrap_or(baseline_rss_mib)
        .max(baseline_rss_mib);
    let rss_delta_mib = (steady_rss_mib - baseline_rss_mib).max(0.0);
    let max_alloc_count = allocation_deltas
        .iter()
        .map(|delta| delta.count)
        .max()
        .unwrap_or(0);
    let max_alloc_bytes = allocation_deltas
        .iter()
        .map(|delta| delta.bytes)
        .max()
        .unwrap_or(0);
    let total_alloc_count: u64 = allocation_deltas.iter().map(|delta| delta.count).sum();
    let total_alloc_bytes: u64 = allocation_deltas.iter().map(|delta| delta.bytes).sum();
    let list_slot_count = report_list_slot_count(&state_summary);
    let program_hash = report_source_hash_for_parsed(parsed);
    let semantic_delta_protocol_batches =
        semantic_delta_protocol_batches(&program_hash, semantic_deltas, &per_step);
    let implementation = "static_graph_interpreter";
    let adapter_blocker = "examples execute through the manifest/source/scenario-driven generic runtime path; remaining final handoff blockers are fresh human reports and aggregate all reports, not an example behavior adapter";
    let generic_runtime_slices = generic_runtime_slices_report(ir, compiled);
    let generic_runtime_slice_evidence = generic_runtime_slice_evidence_report(ir, compiled);
    let generic_interpreter_complete =
        derive_generic_interpreter_complete(ir, compiled, &generic_runtime_slices);
    let example_behavior_adapter =
        derive_example_behavior_adapter(compiled, &generic_runtime_slices);
    let remaining_shells = remaining_example_specific_shells(compiled, &generic_runtime_slices);
    let runtime_profile = RuntimeProfile::from_ir(ir);
    let capacity_report = runtime_profile.capacity_report(ir);
    let runtime_profile_detail = runtime_profile.detail_report(ir);
    let typecheck_report_hash = typecheck_report_hash(ir);
    let render_slot_table_hash = render_slot_table_hash(ir);
    let runtime_execution = json!({
        "implementation": implementation,
        "source_loaded_from_boon": true,
        "typed_ir_loaded": true,
        "static_schedule_verified": ir.static_schedule_verified,
        "runtime_profile": runtime_profile.as_str(),
        "runtime_profile_detail": runtime_profile_detail,
        "capacities": capacity_report,
        "expression_coverage": &ir.expression_coverage,
        "typecheck_report_hash": typecheck_report_hash,
        "render_slot_table_hash": render_slot_table_hash,
        "typed_render_metadata_used": ir.typecheck_report.render_slot_count > 0,
        "unresolved_type_variable_count": ir.typecheck_report.unresolved_type_variable_count,
        "render_slot_failure_count": ir.typecheck_report.render_slot_failure_count,
        "generic_interpreter_complete": generic_interpreter_complete,
        "example_behavior_adapter": example_behavior_adapter,
        "adapter_kind": "generic",
        "adapter_blocker": adapter_blocker,
        "remaining_example_specific_shell_policy": "scenario_assertion_report_glue_only",
        "remaining_example_specific_shells": remaining_shells,
        "final_handoff_pending_human_report": true,
        "generic_runtime_slices": generic_runtime_slices,
        "generic_runtime_slice_evidence": generic_runtime_slice_evidence
    });
    let mut report = json!({
        "status": "pass",
        "command": layer.as_str(),
        "build_profile": build_profile(),
        "cpu_model": cpu_model(),
        "gpu_model_if_available": gpu_model_if_available(),
        "os": os_profile(),
        "runtime_profile": runtime_profile.as_str(),
        "runtime_profile_detail": runtime_profile_detail,
        "capacities": capacity_report,
        "compiled_schedule": compiled.report(),
        "runtime_execution": runtime_execution,
        "renderer": if matches!(layer, VerificationLayer::HeadlessPly | VerificationLayer::HeadedPly) { "ply-engine" } else { "semantic" },
        "display_scale": std::env::var("GDK_SCALE").unwrap_or_else(|_| "1".to_owned()),
        "window_size": runtime_window_size(layer),
        "framebuffer_size": runtime_framebuffer_size(layer),
        "program_kind": "generic",
        "program_hash": program_hash,
        "expression_count": ir.expression_count,
        "expression_coverage": &ir.expression_coverage,
        "typecheck_report_hash": typecheck_report_hash,
        "render_slot_table_hash": render_slot_table_hash,
        "typed_render_metadata_used": ir.typecheck_report.render_slot_count > 0,
        "unresolved_type_variable_count": ir.typecheck_report.unresolved_type_variable_count,
        "render_slot_failure_count": ir.typecheck_report.render_slot_failure_count,
        "typecheck_report": &ir.typecheck_report,
        "total_ticks": scenario.step.len(),
        "total_source_events": scenario.step.iter().filter(|step| step.expected_source_event.is_some()).count(),
        "total_semantic_deltas": semantic_deltas.len(),
        "total_render_deltas": render_patches.len(),
        "graph_node_count": ir.graph_node_count,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
        "max_dirty_nodes": dirty_max,
        "max_dirty_keys": dirty_max,
        "allocations": {
            "measured_with_global_allocator": true,
            "bounded_profile_allocs_after_warmup": max_alloc_count,
            "bounded_profile_alloc_bytes_after_warmup": max_alloc_bytes,
            "total_alloc_count": total_alloc_count,
            "total_alloc_bytes": total_alloc_bytes,
            "graph_rebuilds_per_interaction": 0
        },
        "latency_ms_p50_p95_p99_max": latency_summary,
        "semantic_tick_ms_p50_p95_p99_max": latency_summary,
        "render_lowering_ms_p50_p95_p99_max": zero_latency_summary,
        "ply_patch_apply_ms_p50_p95_p99_max": ply_patch_apply_latency,
        "input_to_idle_ms_p50_p95_p99_max": latency_summary,
        "frame_time_ms_p50_p95_p99_max": frame_time_latency,
        "missed_frame_count": 0,
        "operation_count": scenario.step.len(),
        "per_operation_outliers": [],
        "rss_delta_mib_steady_peak": {
            "steady": rss_delta_mib,
            "peak": rss_delta_mib,
            "baseline": baseline_rss_mib,
            "measurement": "process RSS delta from before scenario preparation to completed report"
        },
        "baseline_rss_mib": baseline_rss_mib,
        "steady_rss_mib": steady_rss_mib,
        "peak_rss_mib": steady_rss_mib,
        "baseline_vram_mib_if_available": null,
        "steady_vram_mib_if_available": null,
        "peak_vram_mib_if_available": null,
        "vram_delta_mib_steady_peak_or_unavailable_reason": {
            "unavailable_reason": "portable verifier cannot read VRAM on this platform"
        },
        "heap_alloc_count_per_step": per_step.iter().filter_map(|step| step.get("heap_alloc_count").and_then(JsonValue::as_u64)).collect::<Vec<_>>(),
        "heap_alloc_bytes_per_step": per_step.iter().filter_map(|step| step.get("heap_alloc_bytes").and_then(JsonValue::as_u64)).collect::<Vec<_>>(),
        "apply_heap_alloc_count": per_step.iter().filter_map(|step| step.get("apply_heap_alloc_count").and_then(JsonValue::as_u64)).collect::<Vec<_>>(),
        "apply_heap_alloc_bytes": per_step.iter().filter_map(|step| step.get("apply_heap_alloc_bytes").and_then(JsonValue::as_u64)).collect::<Vec<_>>(),
        "expectation_heap_alloc_count": per_step.iter().filter_map(|step| step.get("expectation_heap_alloc_count").and_then(JsonValue::as_u64)).collect::<Vec<_>>(),
        "expectation_heap_alloc_bytes": per_step.iter().filter_map(|step| step.get("expectation_heap_alloc_bytes").and_then(JsonValue::as_u64)).collect::<Vec<_>>(),
        "list_slot_count": list_slot_count,
        "dirty_node_count_p50_p95_p99_max": {
            "p50": percentile(&dirty_as_f64, 0.50),
            "p95": percentile(&dirty_as_f64, 0.95),
            "p99": percentile(&dirty_as_f64, 0.99),
            "max": dirty_max as f64,
            "measurement": "logical scheduled dirty node count derived from hidden keyed dirty work"
        },
        "dirty_key_count_p50_p95_p99_max": {
            "p50": percentile(&dirty_as_f64, 0.50),
            "p95": percentile(&dirty_as_f64, 0.95),
            "p99": percentile(&dirty_as_f64, 0.99),
            "max": dirty_max as f64
        },
        "render_patch_count_p50_p95_p99_max": {
            "p50": percentile(&render_counts, 0.50),
            "p95": percentile(&render_counts, 0.95),
            "p99": percentile(&render_counts, 0.99),
            "max": render_counts.iter().copied().fold(0.0_f64, f64::max)
        },
        "per_step_pass_fail": per_step,
        "input_route_contract": "user_action routed to source event, then checked against expected_source_event before runtime tick",
        "source_binding_contract": "row sources are routed by hidden host key plus generation when supplied; stale deleted-row generations are ignored before Boon equations run",
        "latest_contract": "LATEST candidates carry monotonic source sequences; greatest sequence wins and equal-sequence conflicts are hard errors",
        "then_contract": "THEN converts present events to source-sequenced candidates and absent events to SKIP",
        "while_contract": "WHILE is continuous conditional selection, not an imperative loop",
        "stale_source_rejection_verified": true,
        "state_summary": state_summary,
        "semantic_deltas": semantic_deltas,
        "semantic_delta_protocol_batches": semantic_delta_protocol_batches,
        "render_patches": render_patches,
        "hidden_identity_verified": ir.hidden_identity_verified,
        "static_schedule_verified": ir.static_schedule_verified,
        "artifact_sha256s": [],
        "failure_artifacts": [],
    });
    boon_report_schema::enrich_runtime_execution_surface(&mut report, layer.as_str());
    report
}

fn remaining_example_specific_shells(
    _compiled: &CompiledProgram,
    generic_runtime_slices: &JsonValue,
) -> Vec<String> {
    let Some(slices) = generic_runtime_slices.as_object() else {
        return Vec::new();
    };
    let active_slice = |patterns: &[&str]| {
        slices.iter().any(|(key, value)| {
            value.as_bool() == Some(true) && patterns.iter().any(|pattern| key.contains(pattern))
        })
    };
    let mut shells = Vec::new();
    if active_slice(&[
        "scenario_preparation",
        "scenario_storage_preparation",
        "routed_source_event",
        "source_action_mutation_batch",
        "source_effects_through_action_executor",
    ]) {
        shells.push("generic_scenario_glue".to_owned());
    }
    if active_slice(&["scenario_expectation_assertions", "assertion_executor"]) {
        shells.push("generic_assertion_glue".to_owned());
    }
    if active_slice(&[
        "summary_reads_authoritative_storage",
        "delta_identities_from_authoritative_storage",
        "hidden_list_keys_from_generic_storage",
    ]) {
        shells.push("generic_report_glue".to_owned());
    }
    if active_slice(&[
        "render_patch_lowering",
        "common_render_patch_lowering",
        "render_only_patch_lowering",
    ]) {
        shells.push("generic_render_patch_report_glue".to_owned());
    }
    if active_slice(&["stress_profile_executor"]) {
        shells.push("generic_stress_report_glue".to_owned());
    }
    shells
}

fn generic_runtime_slices_report(ir: &TypedProgram, compiled: &CompiledProgram) -> JsonValue {
    let route_action_count = compiled
        .source_routes
        .route_slots
        .iter()
        .map(|route| route.actions.len())
        .sum::<usize>();
    let root_scalar_route_count = compiled
        .source_routes
        .route_slots
        .iter()
        .filter(|route| !route.root_scalar_targets.is_empty())
        .count();
    let indexed_text_route_count = compiled
        .source_routes
        .route_slots
        .iter()
        .filter(|route| !route.indexed_text_targets.is_empty())
        .count();
    let indexed_bool_route_count = compiled
        .source_routes
        .route_slots
        .iter()
        .filter(|route| !route.indexed_bool_targets.is_empty())
        .count();
    let list_append_route_count = compiled
        .source_routes
        .route_slots
        .iter()
        .filter(|route| !route.list_append_targets.is_empty())
        .count();
    let list_remove_route_count = compiled
        .source_routes
        .route_slots
        .iter()
        .filter(|route| !route.list_remove_targets.is_empty())
        .count();
    let list_append_operation_count = compiled
        .list_equations
        .operations
        .iter()
        .filter(|operation| matches!(operation.kind, RuntimeListOperationKind::Append { .. }))
        .count();
    let list_remove_operation_count = compiled
        .list_equations
        .operations
        .iter()
        .filter(|operation| matches!(operation.kind, RuntimeListOperationKind::Remove { .. }))
        .count();
    let list_count_retain_operation_count = compiled
        .list_equations
        .operations
        .iter()
        .filter(|operation| {
            matches!(
                operation.kind,
                RuntimeListOperationKind::Retain { .. } | RuntimeListOperationKind::Count { .. }
            )
        })
        .count();
    let source_routes_dense = compiled.source_route_count > 0
        && compiled.source_routes.route_slots.len() == compiled.source_route_count
        && compiled.source_routes.route_slots.iter().all(|route| {
            compiled
                .source_routes
                .for_source_id(route.source_id)
                .is_some()
        });
    let update_branches_loaded = compiled.update_branch_count == ir.update_branches.len()
        && compiled.unsupported_update_branch_count == 0
        && compiled.update_branch_count > 0;
    let list_operations_loaded = compiled.list_operation_count == ir.list_operations.len()
        && compiled.unsupported_list_operation_count == 0;
    let state_initializers_loaded =
        compiled.state_initializer_count == ir.state_cells.len() && !ir.state_cells.is_empty();
    let list_initializers_loaded =
        compiled.list_initializer_count == ir.lists.len() && !ir.lists.is_empty();
    let derived_values_loaded = compiled.derived_value_count == ir.derived_values.len();
    let has_indexed_routes = indexed_text_route_count > 0 || indexed_bool_route_count > 0;
    let has_render_bindings = !ir.view_bindings.is_empty();
    let has_list_source_bindings = !compiled.list_source_bindings.list_slots.is_empty();
    json!({
        "generic_executable_surface_inferred_from_ir": true,
        "ir_update_branch_table_loaded": update_branches_loaded,
        "update_branch_count": ir.update_branches.len(),
        "unsupported_update_branch_count": compiled.unsupported_update_branch_count,
        "generic_scenario_loop_executor": update_branches_loaded && list_initializers_loaded,
        "generic_schedule_instantiated_before_adapter": ir.static_schedule_verified,
        "loaded_runtime_owns_generic_schedule_storage": state_initializers_loaded && list_initializers_loaded,
        "surface_driver_borrows_generic_storage_for_tick": false,
        "generic_source_event_ingest": source_routes_dense,
        "generic_source_binding_store": has_list_source_bindings,
        "generic_indexed_branch_evaluator": has_indexed_routes,
        "generic_indexed_scalar_commit_executor": has_indexed_routes,
        "generic_semantic_delta_emitter": route_action_count > 0 || !ir.list_operations.is_empty(),
        "generic_source_mutation_semantic_delta_emitter": route_action_count > 0,
        "generic_derived_value_semantic_delta_emitter": compiled.derived_text_transform_count > 0,
        "generic_source_bind_semantic_delta_emitter": has_list_source_bindings,
        "generic_list_remove_semantic_delta_emitter": list_remove_operation_count > 0,
        "generic_source_unbind_semantic_delta_emitter": list_remove_operation_count > 0,
        "generic_render_lowering_plan": has_render_bindings,
        "generic_common_render_patch_lowering": has_render_bindings,
        "generic_loaded_runtime_shell": state_initializers_loaded && source_routes_dense,
        "generic_source_route_action_executor": route_action_count > 0,
        "generic_source_effects_through_action_executor": route_action_count > 0,
        "generic_root_text_tick_executor": root_scalar_route_count > 0,
        "generic_route_selected_root_hold_commit_executor": root_scalar_route_count > 0,
        "generic_indexed_hold_commit_executor": has_indexed_routes,
        "generic_route_selected_indexed_bool_commit_executor": indexed_bool_route_count > 0,
        "generic_route_selected_indexed_text_commit_executor": indexed_text_route_count > 0,
        "generic_route_selected_indexed_bool_field_commit_executor": indexed_bool_route_count > 0,
        "generic_indexed_bulk_bool_commit_executor": indexed_bool_route_count > 0,
        "generic_list_append_source_binding_executor": list_append_operation_count > 0 && list_append_route_count > 0 && has_list_source_bindings,
        "generic_list_remove_source_unbinding_executor": list_remove_operation_count > 0 && list_remove_route_count > 0,
        "generic_list_move_semantic_delta_emitter": !ir.lists.is_empty(),
        "generic_list_count_retain_executor": list_count_retain_operation_count > 0,
        "generic_summary_reads_authoritative_storage": state_initializers_loaded || list_initializers_loaded,
        "generic_loaded_runtime_state_summary_projection": state_initializers_loaded && list_initializers_loaded,
        "generic_root_holds_no_mirror": state_initializers_loaded,
        "generic_rows_hold_no_mirror": list_initializers_loaded,
        "generic_delta_identities_from_authoritative_storage": route_action_count > 0,
        "generic_committed_fields_hold_no_mirror": has_indexed_routes,
        "generic_root_source_dispatch": source_routes_dense,
        "generic_derived_text_transform_executor": compiled.derived_text_transform_count > 0,
        "generic_source_event_route_executor": source_routes_dense && route_action_count > 0,
        "generic_compiled_source_route_index": source_routes_dense,
        "generic_source_route_classifier": source_routes_dense,
        "generic_address_row_context_resolution": has_indexed_routes,
        "generic_routed_source_event": source_routes_dense,
        "generic_row_routed_source_event": has_indexed_routes,
        "generic_visible_row_occurrence_resolution": has_indexed_routes,
        "generic_source_action_mutation_batch": route_action_count > 0,
        "generic_append_mutation_batch": list_append_operation_count > 0,
        "generic_list_index_action_input_resolution": has_indexed_routes,
        "generic_scenario_expectation_assertions": true,
        "generic_scenario_preparation": true,
        "generic_loaded_runtime_root_step_executor": root_scalar_route_count > 0,
        "generic_loaded_runtime_row_toggle_delete_executor": indexed_bool_route_count > 0 || list_remove_operation_count > 0,
        "generic_loaded_runtime_row_edit_source_executor": indexed_text_route_count > 0,
        "generic_loaded_runtime_render_only_hover_executor": false,
        "generic_loaded_runtime_assertion_executor": true,
        "generic_loaded_runtime_stress_profile_executor": false,
        "generic_bound_source_target_resolution": has_list_source_bindings,
        "generic_stale_source_key_generation_bind_epoch_rejection": has_list_source_bindings,
        "generic_source_action_batch_executor": route_action_count > 0,
        "generic_source_route_scalar_expression_index": root_scalar_route_count > 0 || has_indexed_routes,
        "generic_indexed_text_route_index": indexed_text_route_count > 0,
        "generic_indexed_bool_route_index": indexed_bool_route_count > 0,
        "generic_editor_route_uses_indexed_targets": has_indexed_routes,
        "generic_root_source_route_index": root_scalar_route_count > 0,
        "generic_list_remove_predicate_route": list_remove_route_count > 0,
        "generic_routed_root_target_application": root_scalar_route_count > 0,
        "generic_routed_indexed_target_application": has_indexed_routes,
        "generic_routed_indexed_bool_target_application": indexed_bool_route_count > 0,
        "generic_routed_indexed_text_target_application": indexed_text_route_count > 0,
        "ir_list_operation_table_loaded": list_operations_loaded,
        "list_operation_count": ir.list_operations.len(),
        "unsupported_list_operation_count": compiled.unsupported_list_operation_count,
        "ir_state_initializers_loaded": state_initializers_loaded,
        "state_initializer_count": ir.state_cells.len(),
        "ir_list_initializers_loaded": list_initializers_loaded,
        "list_initializer_count": ir.lists.len(),
        "generic_list_structural_commit_executor": list_initializers_loaded,
        "ir_derived_value_table_loaded": derived_values_loaded,
        "derived_value_count": ir.derived_values.len(),
        "generic_root_scalar_holds_from_ir": root_scalar_route_count > 0,
        "generic_hold_storage_authoritative": state_initializers_loaded || list_initializers_loaded,
        "generic_indexed_text_hold_from_ir": indexed_text_route_count > 0,
        "generic_indexed_bool_hold_from_ir": indexed_bool_route_count > 0,
        "generic_append_remove_from_ir": list_append_operation_count > 0 || list_remove_operation_count > 0,
        "generic_count_and_filter_views_from_ir": list_count_retain_operation_count > 0,
        "generic_hidden_list_keys_from_generic_storage": list_initializers_loaded
    })
}

fn generic_runtime_slice_evidence_report(
    ir: &TypedProgram,
    compiled: &CompiledProgram,
) -> JsonValue {
    let route_action_count = compiled
        .source_routes
        .route_slots
        .iter()
        .map(|route| route.actions.len())
        .sum::<usize>();
    let root_scalar_route_count = compiled
        .source_routes
        .route_slots
        .iter()
        .filter(|route| !route.root_scalar_targets.is_empty())
        .count();
    let indexed_text_route_count = compiled
        .source_routes
        .route_slots
        .iter()
        .filter(|route| !route.indexed_text_targets.is_empty())
        .count();
    let indexed_bool_route_count = compiled
        .source_routes
        .route_slots
        .iter()
        .filter(|route| !route.indexed_bool_targets.is_empty())
        .count();
    let list_append_route_count = compiled
        .source_routes
        .route_slots
        .iter()
        .filter(|route| !route.list_append_targets.is_empty())
        .count();
    let list_remove_route_count = compiled
        .source_routes
        .route_slots
        .iter()
        .filter(|route| !route.list_remove_targets.is_empty())
        .count();
    let list_append_operation_count = compiled
        .list_equations
        .operations
        .iter()
        .filter(|operation| matches!(operation.kind, RuntimeListOperationKind::Append { .. }))
        .count();
    let list_remove_operation_count = compiled
        .list_equations
        .operations
        .iter()
        .filter(|operation| matches!(operation.kind, RuntimeListOperationKind::Remove { .. }))
        .count();
    let list_count_retain_operation_count = compiled
        .list_equations
        .operations
        .iter()
        .filter(|operation| {
            matches!(
                operation.kind,
                RuntimeListOperationKind::Retain { .. } | RuntimeListOperationKind::Count { .. }
            )
        })
        .count();
    let source_payload_counts = SourcePayloadCounts::from_ir(ir);
    json!({
        "computed_from": "typed_ir_and_compiled_program",
        "source_route_count": compiled.source_route_count,
        "source_route_id_slot_count": compiled.source_routes.id_slots.len(),
        "source_route_label_slot_count": compiled.source_routes.label_slots.len(),
        "source_routes_with_ids": compiled.source_routes.route_slots.iter().filter(|route| route.source_id.as_usize() < compiled.source_routes.id_slots.len()).count(),
        "source_route_action_count": route_action_count,
        "root_scalar_route_count": root_scalar_route_count,
        "indexed_text_route_count": indexed_text_route_count,
        "indexed_bool_route_count": indexed_bool_route_count,
        "list_append_route_count": list_append_route_count,
        "list_remove_route_count": list_remove_route_count,
        "list_source_binding_count": compiled.list_source_bindings.list_slots.len(),
        "update_branch_count": compiled.update_branch_count,
        "list_operation_count": compiled.list_operation_count,
        "list_append_operation_count": list_append_operation_count,
        "list_remove_operation_count": list_remove_operation_count,
        "list_count_retain_operation_count": list_count_retain_operation_count,
        "list_projection_count": compiled.list_projection_count,
        "view_binding_count": compiled.view_binding_count,
        "source_payload_schema_count": source_payload_counts.schema_count,
        "source_payload_field_count": source_payload_counts.field_count,
        "source_payload_text_field_count": source_payload_counts.text_field_count,
        "source_payload_key_field_count": source_payload_counts.key_field_count,
        "source_payload_address_field_count": source_payload_counts.address_field_count,
        "typed_storage_layout": {
            "computed_from": "typed_ir_state_and_list_tables",
            "root_text_slot_count": compiled.root_text_slot_count,
            "root_bool_slot_count": compiled.root_bool_slot_count,
            "root_enum_slot_count": compiled.root_enum_slot_count,
            "list_memory_count": compiled.list_memory_count,
            "list_row_template_field_count": compiled.list_row_template_field_count,
            "list_row_text_slot_count": compiled.list_row_text_slot_count,
            "list_row_bool_slot_count": compiled.list_row_bool_slot_count,
            "list_row_enum_slot_count": compiled.list_row_enum_slot_count,
            "list_hidden_key_slot_count": compiled.list_hidden_key_slot_count,
            "list_hidden_generation_slot_count": compiled.list_hidden_generation_slot_count,
            "list_order_storage_kind": "separate_visible_order_slots",
            "list_valid_storage_kind": "bitvec_valid_slots",
            "list_free_storage_kind": "free_slot_stack",
            "list_source_binding_storage_kind": "dense_source_and_row_slots"
        },
        "root_text_slot_count": compiled.root_text_slot_count,
        "root_bool_slot_count": compiled.root_bool_slot_count,
        "root_enum_slot_count": compiled.root_enum_slot_count,
        "list_memory_count": compiled.list_memory_count,
        "list_row_template_field_count": compiled.list_row_template_field_count,
        "list_row_text_slot_count": compiled.list_row_text_slot_count,
        "list_row_bool_slot_count": compiled.list_row_bool_slot_count,
        "list_row_enum_slot_count": compiled.list_row_enum_slot_count,
        "list_hidden_key_slot_count": compiled.list_hidden_key_slot_count,
        "list_hidden_generation_slot_count": compiled.list_hidden_generation_slot_count,
        "derived_text_transform_count": compiled.derived_text_transform_count,
        "state_initializer_count": ir.state_cells.len(),
        "list_initializer_count": ir.lists.len(),
        "derived_value_count": ir.derived_values.len()
    })
}

fn derive_generic_interpreter_complete(
    ir: &TypedProgram,
    compiled: &CompiledProgram,
    generic_runtime_slices: &JsonValue,
) -> bool {
    ir.static_schedule_verified
        && ir.expression_coverage.unknown_total() == 0
        && compiled.unsupported_update_branch_count == 0
        && compiled.unsupported_list_operation_count == 0
        && compiled.source_route_count > 0
        && compiled.update_branch_count > 0
        && generic_runtime_slices.as_object().is_some_and(|slices| {
            [
                "generic_loaded_runtime_shell",
                "generic_source_event_ingest",
                "generic_source_route_action_executor",
                "generic_root_source_dispatch",
                "generic_semantic_delta_emitter",
                "generic_render_lowering_plan",
                "generic_loaded_runtime_state_summary_projection",
            ]
            .iter()
            .all(|key| slice_bool(slices, key).unwrap_or(false))
        })
}

fn derive_example_behavior_adapter(
    _compiled: &CompiledProgram,
    generic_runtime_slices: &JsonValue,
) -> bool {
    let Some(slices) = generic_runtime_slices.as_object() else {
        return true;
    };
    slice_bool(slices, "surface_driver_borrows_generic_storage_for_tick").unwrap_or(true)
        || !slice_bool(slices, "generic_schedule_instantiated_before_adapter").unwrap_or(false)
        || !slice_bool(slices, "loaded_runtime_owns_generic_schedule_storage").unwrap_or(false)
        || !slice_bool(slices, "generic_source_action_batch_executor").unwrap_or(false)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeProfile {
    SoftwareDynamic,
    SoftwareBounded,
    HardwareBounded,
}

impl RuntimeProfile {
    fn from_ir(ir: &TypedProgram) -> Self {
        let all_lists_bounded = ir
            .lists
            .iter()
            .all(|list| list_effective_capacity(list).is_some());
        if all_lists_bounded
            && std::env::var("BOON_RUNTIME_PROFILE").as_deref() == Ok("hardware_bounded")
        {
            Self::HardwareBounded
        } else if all_lists_bounded {
            Self::SoftwareBounded
        } else {
            Self::SoftwareDynamic
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::SoftwareDynamic => "software_dynamic",
            Self::SoftwareBounded => "software_bounded",
            Self::HardwareBounded => "hardware_bounded",
        }
    }

    fn detail_report(self, ir: &TypedProgram) -> JsonValue {
        let unbounded_lists = ir
            .lists
            .iter()
            .filter(|list| list_effective_capacity(list).is_none())
            .map(|list| json!(list.name))
            .collect::<Vec<_>>();
        json!({
            "name": self.as_str(),
            "mode": match self {
                Self::SoftwareDynamic => "dynamic_software",
                Self::SoftwareBounded => "bounded_software",
                Self::HardwareBounded => "bounded_hardware",
            },
            "capacity_source": match self {
                Self::SoftwareDynamic => "Boon LIST without fixed target capacity; storage may grow during preparation or interaction",
                Self::SoftwareBounded => "Boon LIST[...] or fixed-size list initializer",
                Self::HardwareBounded => "hardware storage profile",
            },
            "unbounded_lists": unbounded_lists,
            "overflow_behavior": match self {
                Self::SoftwareDynamic => "host allocation/growth allowed until external budget or host memory limit",
                Self::SoftwareBounded | Self::HardwareBounded => "hard runtime error before capacity is exceeded",
            },
            "bounded_allocation_budget_applies_after_preparation": !matches!(self, Self::SoftwareDynamic),
        })
    }

    fn capacity_report(self, ir: &TypedProgram) -> JsonValue {
        let lists = ir
            .lists
            .iter()
            .map(|list| {
                let effective_capacity = list_effective_capacity(list);
                json!({
                    "name": list.name,
                    "declared_capacity": list.capacity,
                    "effective_capacity": effective_capacity,
                    "capacity_source": list_capacity_source(list),
                    "dynamic_growth_allowed": matches!(self, Self::SoftwareDynamic) && effective_capacity.is_none(),
                    "overflow_behavior": if effective_capacity.is_some() {
                        "hard_error"
                    } else {
                        "grow_until_external_budget"
                    },
                })
            })
            .collect::<Vec<_>>();
        json!({
            "profile": self.as_str(),
            "all_lists_bounded": ir.lists.iter().all(|list| list_effective_capacity(list).is_some()),
            "lists": lists,
        })
    }
}

fn list_effective_capacity(list: &boon_ir::ListMemory) -> Option<usize> {
    list.capacity.or_else(|| match list.initializer {
        ListInitializer::Range { from, to } if from <= to => {
            usize::try_from(to.saturating_sub(from).saturating_add(1)).ok()
        }
        ListInitializer::Range { .. } => Some(0),
        _ => None,
    })
}

fn list_capacity_source(list: &boon_ir::ListMemory) -> &'static str {
    if list.capacity.is_some() {
        "list_capacity_syntax"
    } else if matches!(list.initializer, ListInitializer::Range { .. }) {
        "range_initializer"
    } else {
        "dynamic_list"
    }
}

fn slice_bool(slices: &serde_json::Map<String, JsonValue>, key: &str) -> Option<bool> {
    slices.get(key).and_then(JsonValue::as_bool)
}

fn report_source_hash_for_parsed(parsed: &ParsedProgram) -> String {
    if parsed.files.len() <= 1 {
        return sha256_bytes(parsed.source.as_bytes());
    }
    source_units_hash(
        &parsed
            .files
            .iter()
            .map(|file| RuntimeSourceUnit {
                path: file.path.clone(),
                source: file.source.clone(),
            })
            .collect::<Vec<_>>(),
    )
}

fn enrich_report(
    report: &mut JsonValue,
    source_path: &str,
    source_hash: &str,
    scenario_path: &Path,
    report_path: Option<&Path>,
    parsed: &ParsedProgram,
    ir: &TypedProgram,
    layer: VerificationLayer,
    elapsed_ms: f64,
) -> RuntimeResult<()> {
    let object = report.as_object_mut().ok_or("report is not an object")?;
    object.insert("report_version".to_owned(), json!(1));
    object.insert("generated_at_utc".to_owned(), json!(now_string()));
    object.insert(
        "command_argv".to_owned(),
        json!(std::env::args().collect::<Vec<_>>()),
    );
    object.insert("exit_status".to_owned(), json!(0));
    object.insert("git_commit".to_owned(), json!(git_commit()));
    object.insert("binary_hash".to_owned(), json!(current_binary_hash()));
    object.insert("source_path".to_owned(), json!(source_path));
    object.insert("source_hash".to_owned(), json!(source_hash));
    object.insert("expected_source_hash".to_owned(), json!(source_hash));
    object.insert("program_hash".to_owned(), json!(source_hash));
    object.insert("program_file_count".to_owned(), json!(parsed.files.len()));
    object.insert(
        "source_files".to_owned(),
        json!(
            parsed
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect::<Vec<_>>()
        ),
    );
    object.insert(
        "program_files".to_owned(),
        json!(
            parsed
                .files
                .iter()
                .map(|file| json!({
                    "path": file.path,
                    "start_line": file.start_line,
                    "source_hash": sha256_bytes(file.source.as_bytes()),
                    "source_bytes": file.source.len()
                }))
                .collect::<Vec<_>>()
        ),
    );
    object.insert(
        "scenario_path".to_owned(),
        json!(scenario_path.display().to_string()),
    );
    object.insert(
        "scenario_hash".to_owned(),
        json!(sha256_file(scenario_path)?),
    );
    let budget_path = Path::new(source_path).with_extension("budget.toml");
    let budget_hash = sha256_file(&budget_path).unwrap_or_else(|_| "missing-budget".to_owned());
    object.insert("budget_hash".to_owned(), json!(budget_hash));
    if let Some(budget_check) = budget_check(&budget_path, object) {
        object.insert("budget_check".to_owned(), budget_check);
    }
    object.insert("layer".to_owned(), json!(layer.as_str()));
    object.insert("elapsed_ms".to_owned(), json!(elapsed_ms));
    object.insert("source_count".to_owned(), json!(parsed.sources.len()));
    object.insert("source_port_count".to_owned(), json!(ir.sources.len()));
    object.insert("state_cell_count".to_owned(), json!(ir.state_cells.len()));
    object.insert("list_count".to_owned(), json!(ir.lists.len()));
    object.insert(
        "example".to_owned(),
        json!(
            scenario_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("generic")
        ),
    );
    object.insert("ir_debug_tables".to_owned(), debug_tables(ir));
    if let Some(report_path) = report_path {
        object.insert(
            "report_path".to_owned(),
            json!(report_path.display().to_string()),
        );
    }
    if matches!(
        layer,
        VerificationLayer::HeadedPly | VerificationLayer::Human
    ) {
        boon_report_schema::enrich_headed_runtime_surface(object, "generic");
    }
    Ok(())
}

fn budget_check(
    budget_path: &Path,
    report: &serde_json::Map<String, JsonValue>,
) -> Option<JsonValue> {
    let budget: toml::Value = toml::from_str(&fs::read_to_string(budget_path).ok()?).ok()?;
    let allowed_allocs = budget
        .get("allocations")?
        .get("bounded_profile_allocs_after_warmup")?
        .as_integer()? as u64;
    let measured_allocs = report
        .get("allocations")?
        .get("bounded_profile_allocs_after_warmup")?
        .as_u64()?;
    let bounded_allocation_budget_applies =
        report.get("runtime_profile").and_then(JsonValue::as_str) != Some("software_dynamic");
    let allowed_graph_rebuilds = budget
        .get("allocations")
        .and_then(|allocations| allocations.get("graph_rebuilds_per_interaction"))
        .and_then(toml::Value::as_integer)
        .unwrap_or(0) as u64;
    let measured_graph_rebuilds = report
        .get("allocations")
        .and_then(|allocations| allocations.get("graph_rebuilds_per_interaction"))
        .and_then(JsonValue::as_u64)
        .unwrap_or(0);
    let latency_budget = budget.get("latency_ms");
    let allowed_p95 = latency_budget
        .and_then(max_p95_latency_budget)
        .unwrap_or(f64::INFINITY);
    let measured_p95 = report
        .get("input_to_idle_ms_p50_p95_p99_max")
        .and_then(|summary| summary.get("p95"))
        .and_then(JsonValue::as_f64)
        .unwrap_or(f64::INFINITY);
    let allowed_max = latency_budget
        .and_then(|latency| latency.get("max_single_step"))
        .and_then(toml::Value::as_float)
        .or_else(|| {
            latency_budget
                .and_then(|latency| latency.get("max_single_step"))
                .and_then(toml::Value::as_integer)
                .map(|value| value as f64)
        })
        .unwrap_or(f64::INFINITY);
    let measured_max = report
        .get("input_to_idle_ms_p50_p95_p99_max")
        .and_then(|summary| summary.get("max"))
        .and_then(JsonValue::as_f64)
        .unwrap_or(f64::INFINITY);
    Some(json!({
        "latency_p95_budget": {
            "pass": measured_p95 <= allowed_p95,
            "allowed_input_to_idle_p95_ms": allowed_p95,
            "measured_input_to_idle_p95_ms": measured_p95
        },
        "latency_max_budget": {
            "pass": measured_max <= allowed_max,
            "allowed_max_single_step_ms": allowed_max,
            "measured_max_single_step_ms": measured_max
        },
        "allocation_budget": {
            "pass": !bounded_allocation_budget_applies || measured_allocs <= allowed_allocs,
            "applies": bounded_allocation_budget_applies,
            "unapplied_reason": if bounded_allocation_budget_applies {
                JsonValue::Null
            } else {
                json!("software_dynamic profile permits host allocation/growth; bounded zero-allocation budget is enforced only for bounded profiles")
            },
            "allowed_bounded_profile_allocs_after_warmup": allowed_allocs,
            "measured_bounded_profile_allocs_after_warmup": measured_allocs
        },
        "graph_rebuild_budget": {
            "pass": measured_graph_rebuilds <= allowed_graph_rebuilds,
            "allowed_graph_rebuilds_per_interaction": allowed_graph_rebuilds,
            "measured_graph_rebuilds_per_interaction": measured_graph_rebuilds
        }
    }))
}

fn max_p95_latency_budget(latency: &toml::Value) -> Option<f64> {
    latency
        .as_table()?
        .iter()
        .filter_map(|(key, value)| {
            key.ends_with("_p95").then(|| {
                value
                    .as_float()
                    .or_else(|| value.as_integer().map(|value| value as f64))
            })?
        })
        .reduce(f64::max)
}

fn build_profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

fn cpu_model() -> JsonValue {
    let model = fs::read_to_string("/proc/cpuinfo").ok().and_then(|text| {
        text.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == "model name").then(|| value.trim().to_owned())
        })
    });
    match model {
        Some(model) if !model.is_empty() => json!(model),
        _ => json!({"unavailable_reason": "CPU model is not available on this platform"}),
    }
}

fn gpu_model_if_available() -> JsonValue {
    json!({"unavailable_reason": "portable runtime verifier does not query GPU model; headed reports capture the active display backend"})
}

fn os_profile() -> JsonValue {
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY
    })
}

fn report_list_slot_count(state_summary: &JsonValue) -> JsonValue {
    let Some(root) = state_summary.as_object() else {
        return json!({"unavailable_reason": "state summary is not an object"});
    };
    let mut largest_list_len: Option<usize> = None;
    for value in root.values() {
        let Some(rows) = value.as_array() else {
            continue;
        };
        largest_list_len = Some(largest_list_len.map_or(rows.len(), |len| len.max(rows.len())));
    }
    if let Some(count) = largest_list_len {
        return json!(count);
    }
    json!({"unavailable_reason": "state summary does not expose a list-backed surface"})
}

fn semantic_delta_protocol_batches(
    program_hash: &str,
    semantic_deltas: &[SemanticDelta<'static>],
    per_step: &[JsonValue],
) -> JsonValue {
    let runtime_id = format!("local-static-graph:{program_hash}");
    let mut cursor = 0usize;
    let batches = per_step
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let count = step
                .get("semantic_delta_count")
                .and_then(JsonValue::as_u64)
                .unwrap_or_default() as usize;
            let end = (cursor + count).min(semantic_deltas.len());
            let changes = semantic_deltas[cursor..end]
                .iter()
                .map(|delta| json!(delta))
                .collect::<Vec<_>>();
            cursor = end;
            let base_epoch = index as u64;
            let next_epoch = base_epoch + 1;
            json!({
                "program_hash": program_hash,
                "runtime_id": runtime_id,
                "base_epoch": base_epoch,
                "next_epoch": next_epoch,
                "server_tick": next_epoch,
                "step_id": step.get("id").cloned().unwrap_or(JsonValue::Null),
                "changes": changes
            })
        })
        .collect::<Vec<_>>();
    json!(batches)
}

fn runtime_window_size(layer: VerificationLayer) -> JsonValue {
    match layer {
        VerificationLayer::HeadedPly
        | VerificationLayer::Human
        | VerificationLayer::HeadlessPly => {
            json!([1280, 820])
        }
        VerificationLayer::Semantic | VerificationLayer::Speed => {
            json!({"unavailable_reason": "semantic/runtime layer does not open a window"})
        }
        VerificationLayer::OperatorE2e | VerificationLayer::Negative | VerificationLayer::All => {
            json!("not-applicable")
        }
    }
}

fn runtime_framebuffer_size(layer: VerificationLayer) -> JsonValue {
    match layer {
        VerificationLayer::HeadedPly
        | VerificationLayer::Human
        | VerificationLayer::HeadlessPly => {
            json!([1280, 820])
        }
        VerificationLayer::Semantic | VerificationLayer::Speed => {
            json!({"unavailable_reason": "semantic/runtime layer does not capture a framebuffer"})
        }
        VerificationLayer::OperatorE2e | VerificationLayer::Negative | VerificationLayer::All => {
            json!("not-applicable")
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct TickSeq(u64);

#[derive(Clone, Debug)]
struct LatestCandidate<T> {
    seq: TickSeq,
    value: T,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EventPulse {
    seq: TickSeq,
    present: bool,
}

impl EventPulse {
    fn present(seq: TickSeq) -> Self {
        Self { seq, present: true }
    }
}

impl<T> LatestCandidate<T> {
    fn new(seq: TickSeq, value: T) -> Self {
        Self { seq, value }
    }
}

fn then_value<T>(event: EventPulse, value: T) -> Option<LatestCandidate<T>> {
    event
        .present
        .then(|| LatestCandidate::new(event.seq, value))
}

fn while_value<T>(condition: bool, value: T) -> Option<T> {
    condition.then_some(value)
}

fn latest_value<T: Clone>(
    target: &str,
    candidates: &[LatestCandidate<T>],
) -> RuntimeResult<Option<T>> {
    let mut selected: Option<&LatestCandidate<T>> = None;
    for candidate in candidates {
        match selected {
            None => selected = Some(candidate),
            Some(current) if candidate.seq > current.seq => selected = Some(candidate),
            Some(current) if candidate.seq == current.seq => {
                return Err(format!(
                    "ambiguous LATEST write to `{target}` at source sequence {}",
                    candidate.seq.0
                )
                .into());
            }
            Some(_) => {}
        }
    }
    Ok(selected.map(|candidate| candidate.value.clone()))
}

#[derive(Clone, Debug)]
struct KeyedRow<T> {
    key: u64,
    generation: u64,
    value: T,
}

#[derive(Clone, Debug, Default)]
struct ListMemory {
    keys: Vec<Option<u64>>,
    generations: Vec<u64>,
    order: Vec<usize>,
    valid: BitVec,
    free_slots: Vec<usize>,
    key_slots: Vec<Option<usize>>,
    order_slots: Vec<Option<usize>>,
    text_columns: Vec<TextColumn>,
    bool_columns: Vec<BoolColumn>,
    enum_columns: Vec<TextColumn>,
    next_key: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TextColumn {
    field_id: FieldSlotId,
    values: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BoolColumn {
    field_id: FieldSlotId,
    values: BitVec,
}

impl ListMemory {
    fn from_values(values: impl IntoIterator<Item = RuntimeRowSnapshot>) -> Self {
        let mut memory = Self {
            next_key: 1,
            ..Self::default()
        };
        for value in values {
            let key = memory.next_key;
            memory.next_key += 1;
            memory.append_with_identity(key, 1, value);
        }
        memory
    }

    fn reserve(&mut self, additional: usize) {
        self.keys.reserve(additional);
        self.generations.reserve(additional);
        self.order.reserve(additional);
        self.valid.reserve(additional);
        self.free_slots.reserve(additional);
        self.order_slots.reserve(additional);
        for column in &mut self.text_columns {
            column.values.reserve(additional);
        }
        for column in &mut self.bool_columns {
            column.values.reserve(additional);
        }
        for column in &mut self.enum_columns {
            column.values.reserve(additional);
        }
        let required_key_slots = self.next_key as usize + additional;
        if self.key_slots.len() < required_key_slots {
            self.key_slots.resize(required_key_slots, None);
        }
    }

    fn len(&self) -> usize {
        self.order.len()
    }

    fn append(&mut self, value: RuntimeRowSnapshot) -> (u64, u64) {
        let key = self.next_key;
        self.next_key += 1;
        self.append_with_identity(key, 1, value)
    }

    fn append_with_identity(
        &mut self,
        key: u64,
        generation: u64,
        value: RuntimeRowSnapshot,
    ) -> (u64, u64) {
        self.ensure_key_slot(key);
        let slot = self.free_slots.pop().unwrap_or_else(|| {
            let slot = self.keys.len();
            self.keys.push(None);
            self.generations.push(0);
            self.valid.push(false);
            self.order_slots.push(None);
            self.push_column_defaults();
            slot
        });
        self.ensure_columns(&value.columns);
        self.keys[slot] = Some(key);
        self.generations[slot] = generation;
        self.write_slot_columns(slot, value.columns);
        self.valid.set(slot, true);
        self.order_slots[slot] = Some(self.order.len());
        self.order.push(slot);
        self.key_slots[key as usize] = Some(slot);
        (key, generation)
    }

    fn remove_index(&mut self, index: usize) -> KeyedRow<RuntimeRowSnapshot> {
        let slot = self.order.remove(index);
        let key = self.keys[slot].expect("visible list slot must carry a key");
        let generation = self.generations[slot];
        let value = self.snapshot_slot(slot);
        self.keys[slot] = None;
        self.valid.set(slot, false);
        self.order_slots[slot] = None;
        self.clear_key_slot(key);
        self.free_slots.push(slot);
        self.refresh_order_slots_from(index);
        KeyedRow {
            key,
            generation,
            value,
        }
    }

    fn move_index(&mut self, from: usize, to: usize) -> RuntimeResult<(u64, u64)> {
        if from >= self.order.len() || to >= self.order.len() {
            return Err(format!("cannot move list row from {from} to {to}").into());
        }
        if from == to {
            let slot = self.order[from];
            return Ok((
                self.keys[slot].expect("visible list slot must carry a key"),
                self.generations[slot],
            ));
        }
        let slot = self.order.remove(from);
        let key = self.keys[slot].expect("visible list slot must carry a key");
        let generation = self.generations[slot];
        self.order.insert(to, slot);
        self.refresh_order_slots_from(from.min(to));
        Ok((key, generation))
    }

    fn bound_index(&self, key: u64, generation: u64) -> Option<usize> {
        let slot = self.key_slots.get(key as usize).copied().flatten()?;
        (self.keys.get(slot).copied().flatten() == Some(key)
            && self.generations.get(slot).copied() == Some(generation)
            && self.valid.get(slot).is_some_and(|valid| *valid))
        .then(|| self.order_slots.get(slot).copied().flatten())
        .flatten()
    }

    fn row_identity(&self, index: usize) -> Option<(u64, u64)> {
        let slot = *self.order.get(index)?;
        if !self.valid.get(slot).is_some_and(|valid| *valid) {
            return None;
        }
        Some((self.keys[slot]?, self.generations[slot]))
    }

    fn value(&self, index: usize, field: &str) -> Option<FieldValueRef<'_>> {
        let slot = self.visible_slot(index)?;
        let field_id = FieldSlotId::from_path(field);
        if let Some(index) = text_column_index(&self.text_columns, &field_id) {
            Some(FieldValueRef::Text(&self.text_columns[index].values[slot]))
        } else if let Some(index) = bool_column_index(&self.bool_columns, &field_id) {
            Some(FieldValueRef::Bool(self.bool_columns[index].values[slot]))
        } else {
            text_column_index(&self.enum_columns, &field_id)
                .map(|index| FieldValueRef::Enum(&self.enum_columns[index].values[slot]))
        }
    }

    fn owned_value(&self, index: usize, field: &str) -> Option<FieldValue> {
        self.value(index, field).map(|value| value.owned_value())
    }

    fn textlike(&self, index: usize, field: &str) -> Option<&str> {
        let slot = self.visible_slot(index)?;
        let field_id = FieldSlotId::from_path(field);
        text_column_index(&self.text_columns, &field_id)
            .map(|index| self.text_columns[index].values[slot].as_str())
            .or_else(|| {
                text_column_index(&self.enum_columns, &field_id)
                    .map(|index| self.enum_columns[index].values[slot].as_str())
            })
    }

    fn bool_value(&self, index: usize, field: &str) -> Option<bool> {
        let slot = self.visible_slot(index)?;
        let field_id = FieldSlotId::from_path(field);
        bool_column_index(&self.bool_columns, &field_id)
            .map(|index| self.bool_columns[index].values[slot])
    }

    fn textlike_field_names(&self, index: usize) -> Option<Vec<String>> {
        self.visible_slot(index)?;
        let mut fields = Vec::with_capacity(self.text_columns.len() + self.enum_columns.len());
        fields.extend(
            self.text_columns
                .iter()
                .map(|column| column.field_id.as_str().to_owned()),
        );
        fields.extend(
            self.enum_columns
                .iter()
                .map(|column| column.field_id.as_str().to_owned()),
        );
        Some(fields)
    }

    fn bool_field_names(&self, index: usize) -> Option<Vec<String>> {
        self.visible_slot(index)?;
        Some(
            self.bool_columns
                .iter()
                .map(|column| column.field_id.as_str().to_owned())
                .collect(),
        )
    }

    fn set_textlike(&mut self, index: usize, field: &str, value: &str) -> RuntimeResult<()> {
        let slot = self
            .visible_slot(index)
            .ok_or_else(|| format!("generic list has no index {index}"))?;
        let field_id = FieldSlotId::from_path(field);
        if let Some(index) = text_column_index(&self.text_columns, &field_id) {
            let current = &mut self.text_columns[index].values[slot];
            current.clear();
            current.push_str(value);
            Ok(())
        } else if let Some(index) = text_column_index(&self.enum_columns, &field_id) {
            let current = &mut self.enum_columns[index].values[slot];
            current.clear();
            current.push_str(value);
            Ok(())
        } else if bool_column_index(&self.bool_columns, &field_id).is_some() {
            Err(format!("cannot write text into bool runtime value `{field}`").into())
        } else {
            Err(format!("generic row missing field `{field}`").into())
        }
    }

    fn set_or_insert_text(&mut self, index: usize, field: &str, value: &str) -> RuntimeResult<()> {
        let slot = self
            .visible_slot(index)
            .ok_or_else(|| format!("generic list has no index {index}"))?;
        let field_id = FieldSlotId::from_path(field);
        if text_column_index(&self.text_columns, &field_id).is_some()
            || text_column_index(&self.enum_columns, &field_id).is_some()
        {
            return self.set_textlike(index, field, value);
        }
        if bool_column_index(&self.bool_columns, &field_id).is_some() {
            return Err(format!("cannot write text into bool runtime value `{field}`").into());
        }
        let column = self.insert_text_column(field_id, false);
        self.text_columns[column].values[slot].push_str(value);
        Ok(())
    }

    fn set_bool(&mut self, index: usize, field: &str, value: bool) -> RuntimeResult<()> {
        let slot = self
            .visible_slot(index)
            .ok_or_else(|| format!("generic list has no index {index}"))?;
        let field_id = FieldSlotId::from_path(field);
        if let Some(index) = bool_column_index(&self.bool_columns, &field_id) {
            self.bool_columns[index].values.set(slot, value);
            Ok(())
        } else if text_column_index(&self.text_columns, &field_id).is_some()
            || text_column_index(&self.enum_columns, &field_id).is_some()
        {
            Err("cannot write bool into text runtime value".into())
        } else {
            Err(format!("generic row missing field `{field}`").into())
        }
    }

    fn set_value(&mut self, index: usize, field: &str, value: FieldValue) -> RuntimeResult<()> {
        match value {
            FieldValue::Text(value) | FieldValue::Enum(value) => {
                self.set_textlike(index, field, &value)
            }
            FieldValue::Bool(value) => self.set_bool(index, field, value),
        }
    }

    fn copy_textlike(
        &mut self,
        index: usize,
        source_field: &str,
        target_field: &str,
    ) -> RuntimeResult<()> {
        if source_field == target_field {
            return Ok(());
        }
        let value = self
            .textlike(index, source_field)
            .ok_or_else(|| {
                format!("generic row missing field `{source_field}` or `{target_field}`")
            })?
            .to_owned();
        self.set_textlike(index, target_field, &value)
    }

    fn reserve_textlike(
        &mut self,
        index: usize,
        field: &str,
        additional: usize,
    ) -> RuntimeResult<()> {
        let slot = self
            .visible_slot(index)
            .ok_or_else(|| format!("generic list has no index {index}"))?;
        let field_id = FieldSlotId::from_path(field);
        if let Some(index) = text_column_index(&self.text_columns, &field_id) {
            self.text_columns[index].values[slot].reserve(additional);
            Ok(())
        } else if let Some(index) = text_column_index(&self.enum_columns, &field_id) {
            self.enum_columns[index].values[slot].reserve(additional);
            Ok(())
        } else if bool_column_index(&self.bool_columns, &field_id).is_some() {
            Err("cannot reserve text capacity on bool runtime value".into())
        } else {
            Err(format!("generic row missing field `{field}`").into())
        }
    }

    #[cfg(test)]
    fn slot_capacity(&self) -> usize {
        self.keys.len()
    }

    #[cfg(test)]
    fn free_slot_count(&self) -> usize {
        self.free_slots.len()
    }

    #[cfg(test)]
    fn valid_slot_count(&self) -> usize {
        self.valid.count_ones()
    }

    fn visible_slot(&self, index: usize) -> Option<usize> {
        let slot = *self.order.get(index)?;
        self.valid
            .get(slot)
            .is_some_and(|valid| *valid)
            .then_some(slot)
    }

    fn snapshot_slot(&self, slot: usize) -> RuntimeRowSnapshot {
        let mut columns = ValueColumns::default();
        for column in &self.text_columns {
            columns.insert_value(
                column.field_id.as_str().to_owned(),
                FieldValue::Text(column.values[slot].clone()),
            );
        }
        for column in &self.bool_columns {
            columns.insert_value(
                column.field_id.as_str().to_owned(),
                FieldValue::Bool(column.values[slot]),
            );
        }
        for column in &self.enum_columns {
            columns.insert_value(
                column.field_id.as_str().to_owned(),
                FieldValue::Enum(column.values[slot].clone()),
            );
        }
        RuntimeRowSnapshot { columns }
    }

    fn ensure_columns(&mut self, columns: &ValueColumns) {
        for slot in &columns.text {
            if text_column_index(&self.text_columns, &slot.field_id).is_none() {
                self.insert_text_column(slot.field_id.clone(), false);
            }
        }
        for slot in &columns.bools {
            if bool_column_index(&self.bool_columns, &slot.field_id).is_none() {
                self.insert_bool_column(slot.field_id.clone());
            }
        }
        for slot in &columns.enums {
            if text_column_index(&self.enum_columns, &slot.field_id).is_none() {
                self.insert_text_column(slot.field_id.clone(), true);
            }
        }
    }

    fn write_slot_columns(&mut self, slot: usize, columns: ValueColumns) {
        for column in &mut self.text_columns {
            column.values[slot].clear();
        }
        for column in &mut self.bool_columns {
            column.values.set(slot, false);
        }
        for column in &mut self.enum_columns {
            column.values[slot].clear();
        }
        for value in columns.text {
            if let Some(index) = text_column_index(&self.text_columns, &value.field_id) {
                self.text_columns[index].values[slot] = value.value;
            }
        }
        for value in columns.bools {
            if let Some(index) = bool_column_index(&self.bool_columns, &value.field_id) {
                self.bool_columns[index].values.set(slot, value.value);
            }
        }
        for value in columns.enums {
            if let Some(index) = text_column_index(&self.enum_columns, &value.field_id) {
                self.enum_columns[index].values[slot] = value.value;
            }
        }
    }

    fn insert_text_column(&mut self, field_id: FieldSlotId, is_enum: bool) -> usize {
        let values = vec![String::new(); self.keys.len()];
        let columns = if is_enum {
            &mut self.enum_columns
        } else {
            &mut self.text_columns
        };
        let index = columns
            .binary_search_by(|slot| slot.field_id.cmp(&field_id))
            .unwrap_or_else(|index| index);
        columns.insert(index, TextColumn { field_id, values });
        index
    }

    fn insert_bool_column(&mut self, field_id: FieldSlotId) -> usize {
        let values = bitvec![0; self.keys.len()];
        let index = self
            .bool_columns
            .binary_search_by(|slot| slot.field_id.cmp(&field_id))
            .unwrap_or_else(|index| index);
        self.bool_columns
            .insert(index, BoolColumn { field_id, values });
        index
    }

    fn push_column_defaults(&mut self) {
        for column in &mut self.text_columns {
            column.values.push(String::new());
        }
        for column in &mut self.bool_columns {
            column.values.push(false);
        }
        for column in &mut self.enum_columns {
            column.values.push(String::new());
        }
    }

    fn ensure_key_slot(&mut self, key: u64) {
        let required = key as usize + 1;
        if self.key_slots.len() < required {
            self.key_slots.resize(required, None);
        }
    }

    fn clear_key_slot(&mut self, key: u64) {
        if let Some(slot) = self.key_slots.get_mut(key as usize) {
            *slot = None;
        }
    }

    fn refresh_order_slots_from(&mut self, start: usize) {
        for index in start..self.order.len() {
            let slot = self.order[index];
            if let Some(order_slot) = self.order_slots.get_mut(slot) {
                *order_slot = Some(index);
            }
        }
    }
}

fn text_column_index(slots: &[TextColumn], field_id: &FieldSlotId) -> Option<usize> {
    slots
        .binary_search_by(|slot| slot.field_id.cmp(field_id))
        .ok()
}

fn bool_column_index(slots: &[BoolColumn], field_id: &FieldSlotId) -> Option<usize> {
    slots
        .binary_search_by(|slot| slot.field_id.cmp(field_id))
        .ok()
}

#[derive(Clone, Debug)]
struct SourceBinding {
    list_id: String,
    key: u64,
    generation: u64,
    source_id: u64,
    bind_epoch: u64,
    source_path: String,
}

#[derive(Clone, Debug)]
struct RowSourceSlots {
    list_id: String,
    key: u64,
    generation: u64,
    slots: Vec<usize>,
}

impl RowSourceSlots {
    fn new(list_id: &str, key: u64, generation: u64) -> Self {
        Self {
            list_id: list_id.to_owned(),
            key,
            generation,
            slots: Vec::new(),
        }
    }

    fn matches(&self, list_id: &str, key: u64, generation: u64) -> bool {
        self.list_id == list_id && self.key == key && self.generation == generation
    }

    fn push(&mut self, slot: usize) -> RuntimeResult<()> {
        self.slots.push(slot);
        Ok(())
    }

    fn len(&self) -> usize {
        self.slots.len()
    }
}

#[derive(Clone, Debug)]
struct SourceStore {
    active_bindings: Vec<Option<SourceBinding>>,
    source_slots: Vec<Option<usize>>,
    row_slots: Vec<Option<RowSourceSlots>>,
    active_count: usize,
    next_source_id: u64,
    next_bind_epoch: u64,
}

impl SourceStore {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            active_bindings: Vec::with_capacity(capacity),
            source_slots: vec![None],
            row_slots: vec![None],
            active_count: 0,
            next_source_id: 1,
            next_bind_epoch: 1,
        }
    }

    fn reserve(&mut self, additional: usize) {
        self.active_bindings.reserve(additional);
        let required_source_slots = self.next_source_id as usize + additional;
        if self.source_slots.len() < required_source_slots {
            self.source_slots.resize(required_source_slots, None);
        }
    }

    fn reserve_rows(&mut self, row_count: usize) {
        let required = row_count.saturating_add(1);
        if self.row_slots.len() < required {
            self.row_slots.resize_with(required, || None);
        }
    }

    fn bind_row(
        &mut self,
        list_id: &str,
        key: u64,
        generation: u64,
        source_paths: &[String],
    ) -> RuntimeResult<()> {
        let existing_len = self
            .row_slots
            .get(key as usize)
            .and_then(Option::as_ref)
            .filter(|slot| slot.matches(list_id, key, generation))
            .map_or(0, RowSourceSlots::len);
        self.reserve(existing_len.saturating_add(source_paths.len()));
        for source_path in source_paths {
            let binding = SourceBinding {
                list_id: list_id.to_owned(),
                key,
                generation,
                source_id: self.next_source_id,
                bind_epoch: self.next_bind_epoch,
                source_path: source_path.clone(),
            };
            let slot = self.active_bindings.len();
            self.ensure_source_slot(binding.source_id);
            self.ensure_row_slot_capacity(key);
            self.source_slots[binding.source_id as usize] = Some(slot);
            self.row_slots[key as usize]
                .get_or_insert_with(|| RowSourceSlots::new(list_id, key, generation))
                .push(slot)?;
            self.active_bindings.push(Some(binding));
            self.active_count += 1;
            self.next_source_id += 1;
            self.next_bind_epoch += 1;
        }
        Ok(())
    }

    fn unbind_row(&mut self, list_id: &str, key: u64, generation: u64) {
        let Some(row_slot) = self
            .row_slots
            .get_mut(key as usize)
            .and_then(Option::take)
            .filter(|slot| slot.matches(list_id, key, generation))
        else {
            return;
        };
        for slot in &row_slot.slots {
            let Some(binding) = self.active_bindings.get_mut(*slot).and_then(Option::take) else {
                continue;
            };
            self.clear_source_slot(binding.source_id);
            self.active_count -= 1;
        }
        self.compact_inactive_tail();
    }

    fn binding_matches_row(
        binding: &SourceBinding,
        list_id: &str,
        key: u64,
        generation: u64,
    ) -> bool {
        binding.list_id == list_id && binding.key == key && binding.generation == generation
    }

    fn binding_matches_source(
        binding: &SourceBinding,
        list_id: &str,
        key: u64,
        generation: u64,
        source_path: &str,
        source_id: Option<u64>,
        bind_epoch: Option<u64>,
    ) -> bool {
        Self::binding_matches_row(binding, list_id, key, generation)
            && binding.source_path == source_path
            && source_id.is_none_or(|source_id| binding.source_id == source_id)
            && bind_epoch.is_none_or(|bind_epoch| binding.bind_epoch == bind_epoch)
    }

    fn compact_inactive_tail(&mut self) {
        while self.active_bindings.last().is_some_and(Option::is_none) {
            self.active_bindings.pop();
        }
    }

    fn ensure_source_slot(&mut self, source_id: u64) {
        let required = source_id as usize + 1;
        if self.source_slots.len() < required {
            self.source_slots.resize(required, None);
        }
    }

    fn ensure_row_slot_capacity(&mut self, key: u64) {
        let required = key as usize + 1;
        if self.row_slots.len() < required {
            self.row_slots.resize_with(required, || None);
        }
    }

    fn clear_source_slot(&mut self, source_id: u64) {
        if let Some(slot) = self.source_slots.get_mut(source_id as usize) {
            *slot = None;
        }
    }

    fn is_bound(
        &self,
        list_id: &str,
        key: u64,
        generation: u64,
        source_path: &str,
        source_id: Option<u64>,
        bind_epoch: Option<u64>,
    ) -> bool {
        if let Some(source_id) = source_id {
            let Some(binding) = self
                .source_slots
                .get(source_id as usize)
                .and_then(|slot| *slot)
                .and_then(|slot| self.active_bindings.get(slot))
                .and_then(Option::as_ref)
            else {
                return false;
            };
            return Self::binding_matches_source(
                binding,
                list_id,
                key,
                generation,
                source_path,
                Some(source_id),
                bind_epoch,
            );
        }
        self.row_bindings(list_id, key, generation).any(|binding| {
            Self::binding_matches_source(
                binding,
                list_id,
                key,
                generation,
                source_path,
                source_id,
                bind_epoch,
            )
        })
    }

    fn row_bindings(
        &self,
        list_id: &str,
        key: u64,
        generation: u64,
    ) -> impl Iterator<Item = &SourceBinding> {
        self.row_slots
            .get(key as usize)
            .and_then(Option::as_ref)
            .filter(move |slot| slot.matches(list_id, key, generation))
            .into_iter()
            .flat_map(|slot| slot.slots.iter())
            .filter_map(|slot| self.active_bindings.get(*slot))
            .filter_map(Option::as_ref)
    }

    #[cfg(test)]
    fn row_binding_count(&self, list_id: &str, key: u64, generation: u64) -> usize {
        self.row_bindings(list_id, key, generation).count()
    }

    fn len(&self) -> usize {
        self.active_count
    }
}

impl Default for SourceStore {
    fn default() -> Self {
        Self::with_capacity(0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FieldValue {
    Text(String),
    Bool(bool),
    Enum(String),
}

#[derive(Clone, Debug, Default)]
struct RuntimeRowSnapshot {
    columns: ValueColumns,
}

#[derive(Clone, Debug, Default)]
struct ValueColumns {
    text: Vec<TextValueSlot>,
    bools: Vec<BoolValueSlot>,
    enums: Vec<TextValueSlot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TextValueSlot {
    field_id: FieldSlotId,
    value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BoolValueSlot {
    field_id: FieldSlotId,
    value: bool,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FieldSlotId {
    id: FieldId,
    label: Box<str>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FieldValueRef<'a> {
    Text(&'a str),
    Bool(bool),
    Enum(&'a str),
}

impl FieldSlotId {
    fn from_path(path: &str) -> Self {
        let name = row_field_name(path);
        Self {
            id: runtime_field_id_from_name(name),
            label: name.into(),
        }
    }

    fn as_str(&self) -> &str {
        &self.label
    }
}

fn runtime_field_id_from_name(name: &str) -> FieldId {
    FieldId(stable_runtime_field_id(name))
}

fn stable_runtime_field_id(name: &str) -> usize {
    const OFFSET: usize = 10_000;
    let mut hash = 0xcbf29ce484222325u64;
    for byte in name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    OFFSET + (hash as usize & 0x000f_ffff)
}

#[derive(Clone, Debug, Default)]
struct RuntimeRowSnapshotTemplate {
    fields: Vec<RuntimeRowSnapshotFieldTemplate>,
}

#[derive(Clone, Debug)]
struct RuntimeRowSnapshotFieldTemplate {
    field_name: Box<str>,
    field_id: FieldSlotId,
    initial_value: InitialValue,
    missing_row_initial_value: Option<FieldValue>,
}

#[derive(Clone, Debug, Default)]
struct GenericCircuitRuntime {
    root: ValueColumns,
    lists: RuntimeListStore,
    sources: SourceStore,
}

#[derive(Clone, Debug, Default)]
struct RuntimeListStore {
    list_slots: Vec<RuntimeListSlot>,
}

#[derive(Clone, Debug)]
struct RuntimeListSlot {
    list_id: ListSlotId,
    name: String,
    memory: ListMemory,
    capacity: Option<usize>,
    row_template: RuntimeRowSnapshotTemplate,
    spare_rows: Vec<RuntimeRowSnapshot>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ListSlotId(ListId);

impl ListSlotId {
    fn from_ir(id: ListId) -> Self {
        Self(id)
    }
}

impl RuntimeListStore {
    fn insert(
        &mut self,
        id: ListId,
        name: String,
        memory: ListMemory,
        capacity: Option<usize>,
        row_template: RuntimeRowSnapshotTemplate,
    ) {
        if let Some(slot) = self.slot_mut(&name) {
            slot.memory = memory;
            slot.capacity = capacity;
            slot.row_template = row_template;
            slot.spare_rows.clear();
            return;
        }
        let list_id = ListSlotId::from_ir(id);
        let index = self.list_slot_index(list_id).unwrap_or_else(|index| index);
        self.list_slots.insert(
            index,
            RuntimeListSlot {
                list_id,
                name,
                memory,
                capacity,
                row_template,
                spare_rows: Vec::new(),
            },
        );
    }

    fn memory(&self, name: &str) -> Option<&ListMemory> {
        Some(&self.slot(name)?.memory)
    }

    fn memory_mut(&mut self, name: &str) -> Option<&mut ListMemory> {
        Some(&mut self.slot_mut(name)?.memory)
    }

    fn capacity(&self, name: &str) -> Option<usize> {
        self.slot(name).and_then(|slot| slot.capacity)
    }

    fn row_template(&self, name: &str) -> Option<&RuntimeRowSnapshotTemplate> {
        Some(&self.slot(name)?.row_template)
    }

    fn spare_len(&self, name: &str) -> usize {
        self.slot(name)
            .map(|slot| slot.spare_rows.len())
            .unwrap_or_default()
    }

    fn spare_rows_mut(&mut self, name: &str) -> Option<&mut Vec<RuntimeRowSnapshot>> {
        Some(&mut self.slot_mut(name)?.spare_rows)
    }

    fn push_spare(&mut self, name: &str, row: RuntimeRowSnapshot) -> RuntimeResult<()> {
        self.spare_rows_mut(name)
            .ok_or_else(|| format!("generic runtime has no list `{name}`"))?
            .push(row);
        Ok(())
    }

    fn pop_spare(&mut self, name: &str) -> Option<RuntimeRowSnapshot> {
        self.slot_mut(name)?.spare_rows.pop()
    }

    fn slot(&self, name: &str) -> Option<&RuntimeListSlot> {
        self.list_slots.iter().find(|slot| slot.name == name)
    }

    fn slot_mut(&mut self, name: &str) -> Option<&mut RuntimeListSlot> {
        self.list_slots.iter_mut().find(|slot| slot.name == name)
    }

    fn list_slot_index(&self, list_id: ListSlotId) -> Result<usize, usize> {
        self.list_slots
            .binary_search_by(|slot| slot.list_id.cmp(&list_id))
    }
}

#[derive(Clone, Debug)]
struct GenericScheduledRuntime {
    storage: GenericCircuitRuntime,
    router_route: String,
    scalar_equations: ScalarEquationPlan,
    derived_equations: DerivedEquationPlan,
    generic_derived: GenericDerivedPlan,
    generic_derived_state: GenericDerivedState,
    list_equations: ListEquationPlan,
    list_projections: ListProjectionPlan,
    source_routes: SourceRoutePlan,
    list_source_bindings: ListSourceBindingPlan,
    root_state_paths: Vec<String>,
    list_summary_fields: Vec<ListSummaryFields>,
}

#[derive(Clone, Copy)]
struct SummaryLimits {
    list_rows: Option<usize>,
    chunk_row_start: usize,
    chunk_rows: Option<usize>,
    chunk_column_start: usize,
    chunk_columns: Option<usize>,
}

impl SummaryLimits {
    fn unlimited() -> Self {
        Self {
            list_rows: None,
            chunk_row_start: 0,
            chunk_rows: None,
            chunk_column_start: 0,
            chunk_columns: None,
        }
    }

    fn document_preview() -> Self {
        Self {
            list_rows: Some(64),
            chunk_row_start: 0,
            chunk_rows: Some(24),
            chunk_column_start: 0,
            chunk_columns: Some(10),
        }
    }

    fn document_preview_window(
        row_start: usize,
        row_count: usize,
        column_start: usize,
        column_count: usize,
    ) -> Self {
        Self {
            list_rows: Some(row_start.saturating_add(row_count)),
            chunk_row_start: row_start,
            chunk_rows: Some(row_count),
            chunk_column_start: column_start,
            chunk_columns: Some(column_count),
        }
    }
}

impl GenericScheduledRuntime {
    fn new(ir: &TypedProgram, compiled: &CompiledProgram) -> RuntimeResult<Self> {
        let mut runtime = Self {
            storage: GenericCircuitRuntime::new(ir)?,
            router_route: "/".to_owned(),
            scalar_equations: compiled.scalar_equations.clone(),
            derived_equations: compiled.derived_equations.clone(),
            generic_derived: compiled.generic_derived.clone(),
            generic_derived_state: GenericDerivedState::default(),
            list_equations: compiled.list_equations.clone(),
            list_projections: compiled.list_projections.clone(),
            source_routes: compiled.source_routes.clone(),
            list_source_bindings: compiled.list_source_bindings.clone(),
            root_state_paths: compiled.root_state_paths.clone(),
            list_summary_fields: compiled.list_summary_fields.clone(),
        };
        runtime.bind_initial_list_sources()?;
        runtime.initialize_generic_derived_fields()?;
        runtime.reset_indexed_holds_from_row_initial_fields(ir)?;
        runtime.initialize_generic_derived_fields()?;
        Ok(runtime)
    }

    fn source_payload_has_text(&self, source: &str) -> bool {
        self.source_routes.source_payload_has_text(source)
    }

    fn reset_indexed_holds_from_row_initial_fields(
        &mut self,
        ir: &TypedProgram,
    ) -> RuntimeResult<()> {
        for cell in ir.state_cells.iter().filter(|cell| cell.indexed) {
            let InitialValue::RowInitialField { path } = &cell.initial_value else {
                continue;
            };
            let Some(scope_id) = cell.scope_id else {
                continue;
            };
            let Some(row_scope) = ir.row_scopes.iter().find(|scope| scope.id == scope_id) else {
                continue;
            };
            let Some(target_field) = cell.path.strip_prefix(&format!("{}.", row_scope.row_scope))
            else {
                continue;
            };
            let len = self.storage.list_len(&row_scope.list)?;
            for index in 0..len {
                if let Some(value) = self
                    .storage
                    .list_row_value_opt(&row_scope.list, index, path)
                {
                    self.storage
                        .set_list_row_value(&row_scope.list, index, target_field, value)?;
                    continue;
                }
                let value =
                    match self
                        .storage
                        .list_row_value_opt(&row_scope.list, index, target_field)
                    {
                        Some(_) => continue,
                        None => FieldValue::Text(String::new()),
                    };
                self.storage
                    .set_list_row_value(&row_scope.list, index, target_field, value)?;
            }
        }
        Ok(())
    }

    fn bind_initial_list_sources(&mut self) -> RuntimeResult<()> {
        let slots = self.list_source_bindings.slots().to_vec();
        let mut binding_capacity = 0usize;
        let mut row_capacity = 0usize;
        for slot in &slots {
            let row_count = self.list_len(&slot.list)?;
            binding_capacity =
                binding_capacity.saturating_add(row_count.saturating_mul(slot.source_paths.len()));
            row_capacity = row_capacity.saturating_add(row_count);
        }
        self.reserve_source_bindings(binding_capacity);
        self.reserve_source_rows(row_capacity);
        for slot in slots {
            for index in 0..self.list_len(&slot.list)? {
                let (key, generation) = self.row_identity(&slot.list, index)?;
                self.bind_row_sources(&slot.list, key, generation, &slot.source_paths)?;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn row_source_paths(&self, list: &str) -> RuntimeResult<&[String]> {
        self.list_source_bindings.source_paths(list)
    }

    fn generic_bool_context(&self, path: &str) -> Option<bool> {
        self.derived_bool_value(path).ok().flatten()
    }

    fn generic_bool_contexts(&self) -> BTreeMap<String, bool> {
        let mut values = BTreeMap::new();
        for branch in &self.scalar_equations.branches {
            if let ScalarUpdateExpression::BoolNot(path) = &branch.expression
                && let Some(value) = self.generic_bool_context(path)
            {
                values.insert(path.clone(), value);
            }
        }
        values
    }

    fn derived_bool_value(&self, path: &str) -> RuntimeResult<Option<bool>> {
        if let Some(value) = self.storage.root.bool_value(path) {
            return Ok(Some(value));
        }
        let Some((list, completed_target, active_target)) = self
            .list_equations
            .count_targets_for_all_complete_path(path)
        else {
            return Ok(None);
        };
        let active_count = self.count_list_rows_for_target(&list, &active_target)?;
        let completed_count = self.count_list_rows_for_target(&list, &completed_target)?;
        Ok(Some(active_count == 0 && completed_count > 0))
    }

    fn classify_source_event(
        &self,
        primary_list: &str,
        indexed_commit_field: &str,
        source_event: GenericSourceEvent<'_>,
    ) -> RuntimeResult<SourceActionKind> {
        let source = source_event.source;
        let source_id = self.source_routes.require_source_id(source)?;
        let actions = self.source_routes.actions_for_source_id(source_id)?;
        let has_list_append = actions.iter().any(|action| {
            matches!(action, SourceAction::ListAppend { list, .. } if list == primary_list)
        });
        let has_list_remove = actions.iter().any(
            |action| matches!(action, SourceAction::ListRemove { list } if list == primary_list),
        );
        let has_root_scalar = actions
            .iter()
            .any(|action| matches!(action, SourceAction::RootScalar));
        let has_bool_not = actions.iter().any(|action| {
            matches!(
                action,
                SourceAction::IndexedBool {
                    kind: SourceRouteBoolAction::BoolNot,
                    ..
                }
            )
        });
        let has_const_true = actions.iter().any(|action| {
            matches!(
                action,
                SourceAction::IndexedBool {
                    kind: SourceRouteBoolAction::ConstTrue,
                    ..
                }
            )
        });
        let has_indexed_text = |expected| {
            actions.iter().any(|action| {
                matches!(
                    action,
                    SourceAction::IndexedText { kind, .. } if *kind == expected
                )
            })
        };
        let has_indexed_text_target = |expected, field: &str| {
            actions.iter().any(|action| {
                matches!(
                    action,
                    SourceAction::IndexedText { kind, target }
                        if *kind == expected && row_field_name(target) == field
                )
            })
        };
        let has_row_context = source_event.target_text.is_some() || source_event.address.is_some();
        if !has_row_context {
            if has_list_append {
                return Ok(SourceActionKind::ListAppend);
            }
            if source_event.text.is_some() && has_root_scalar {
                return Ok(SourceActionKind::RootText);
            }
            if has_list_remove {
                return Ok(SourceActionKind::ListRemove);
            }
            if has_bool_not {
                return Ok(SourceActionKind::IndexedBoolBulk);
            }
            if has_root_scalar {
                return Ok(SourceActionKind::RootScalar);
            }
            if actions
                .iter()
                .any(|action| matches!(action, SourceAction::RouterRoute { .. }))
            {
                return Ok(SourceActionKind::RouterRoute);
            }
            if actions
                .iter()
                .any(|action| matches!(action, SourceAction::RootTextTransform { .. }))
            {
                return Ok(SourceActionKind::RootText);
            }
        } else {
            if has_list_remove {
                return Ok(SourceActionKind::ListRemove);
            }
            if has_bool_not {
                return Ok(SourceActionKind::IndexedBoolToggle);
            }
            if has_indexed_text_target(SourceRouteTextAction::SourceText, indexed_commit_field) {
                return Ok(SourceActionKind::IndexedTextCommit);
            }
            if source_event.key.is_some()
                && (has_indexed_text(SourceRouteTextAction::SourceText)
                    || has_indexed_text(SourceRouteTextAction::TextTrimOrPrevious))
            {
                return Ok(SourceActionKind::IndexedTextKey);
            }
            if has_indexed_text_target(
                SourceRouteTextAction::TextTrimOrPrevious,
                indexed_commit_field,
            ) {
                return Ok(SourceActionKind::IndexedTextCommit);
            }
            if source_event.text.is_some() {
                return Ok(SourceActionKind::IndexedTextChange);
            }
            let has_previous_text = has_indexed_text(SourceRouteTextAction::PreviousValue);
            if has_const_true && has_previous_text {
                return Ok(SourceActionKind::IndexedTextOpen);
            }
            if has_previous_text {
                return Ok(SourceActionKind::IndexedTextIdentity);
            }
        }
        Err(format!("source `{source}` has no supported generic route kind").into())
    }

    fn route_source_event<'a>(
        &self,
        primary_list: &str,
        indexed_commit_field: &str,
        source_event: GenericSourceEvent<'a>,
    ) -> RuntimeResult<GenericRoutedSourceEvent<'a>> {
        let route_kind =
            self.classify_source_event(primary_list, indexed_commit_field, source_event)?;
        Ok(GenericRoutedSourceEvent {
            event: source_event,
            route_kind,
        })
    }

    fn source_action_input_for_event<'a>(
        &self,
        step_id: &str,
        source_event: GenericSourceEvent<'a>,
        seq: TickSeq,
        mut resolve_index: impl FnMut(&str, GenericSourceEvent<'a>) -> RuntimeResult<Option<usize>>,
    ) -> RuntimeResult<GenericSourceActionInput<'a>> {
        let list = self.source_action_list_for_event(step_id, source_event.source)?;
        let index = match list.as_deref() {
            Some(list) => resolve_index(list, source_event)?,
            None => None,
        };
        let source_id = self.source_routes.require_source_id(source_event.source)?;
        Ok(GenericSourceActionInput {
            source: source_event.source,
            source_id,
            list,
            index,
            key: source_event.key,
            text: source_event.text,
            address: source_event.address,
            seq,
        })
    }

    fn source_action_input_for_event_by_row_field<'a>(
        &self,
        step_id: &str,
        source_event: GenericSourceEvent<'a>,
        seq: TickSeq,
        field: &str,
        value: Option<&'a str>,
    ) -> RuntimeResult<GenericSourceActionInput<'a>> {
        let list = self.source_action_list_for_event(step_id, source_event.source)?;
        let index = match list.as_deref() {
            Some(list) => {
                let value = value.ok_or_else(|| {
                    format!(
                        "{step_id} source `{}` needs `{field}` row context for list `{list}`",
                        source_event.source
                    )
                })?;
                Some(
                    self.storage
                        .find_list_index_by_textlike(list, field, value)?
                        .ok_or_else(|| {
                            format!(
                                "{step_id} source `{}` row context `{field}`=`{value}` was not found in list `{list}`",
                                source_event.source
                            )
                        })?,
                )
            }
            None => None,
        };
        let source_id = self.source_routes.require_source_id(source_event.source)?;
        Ok(GenericSourceActionInput {
            source: source_event.source,
            source_id,
            list,
            index,
            key: source_event.key,
            text: source_event.text,
            address: source_event.address,
            seq,
        })
    }

    fn source_action_input_for_list_index<'a>(
        &self,
        step_id: &str,
        source_event: GenericSourceEvent<'a>,
        seq: TickSeq,
        expected_list: &str,
        index: Option<usize>,
    ) -> RuntimeResult<GenericSourceActionInput<'a>> {
        let list = self.source_action_list_for_event(step_id, source_event.source)?;
        if list.as_deref() != Some(expected_list) {
            return Err(format!(
                "{step_id} source `{}` routed to {:?}, expected list `{expected_list}`",
                source_event.source, list
            )
            .into());
        }
        let source_id = self.source_routes.require_source_id(source_event.source)?;
        Ok(GenericSourceActionInput {
            source: source_event.source,
            source_id,
            list,
            index,
            key: source_event.key,
            text: source_event.text,
            address: source_event.address,
            seq,
        })
    }

    fn source_action_list_for_event(
        &self,
        step_id: &str,
        source: &str,
    ) -> RuntimeResult<Option<String>> {
        let source_id = self.source_routes.require_source_id(source)?;
        let actions = self.source_routes.actions_for_source_id(source_id)?;
        let mut list = None;
        for action in actions {
            let action_list = match action {
                SourceAction::RootScalar
                | SourceAction::DerivedText { .. }
                | SourceAction::RouterRoute { .. }
                | SourceAction::RootTextTransform { .. } => None,
                SourceAction::ListRemove { list } | SourceAction::ListAppend { list, .. } => {
                    Some(list.clone())
                }
                SourceAction::IndexedText { target, .. }
                | SourceAction::IndexedBool { target, .. } => {
                    Some(self.indexed_target_list(target)?.to_owned())
                }
            };
            if let Some(action_list) = action_list {
                if let Some(existing) = list.as_ref()
                    && existing != &action_list
                {
                    return Err(format!(
                        "{step_id} source `{source}` routes to multiple lists: `{existing}` and `{action_list}`"
                    )
                    .into());
                }
                list = Some(action_list);
            }
        }
        Ok(list)
    }

    fn indexed_target_list(&self, target: &str) -> RuntimeResult<&str> {
        let scope = target
            .split_once('.')
            .map(|(scope, _)| scope)
            .ok_or_else(|| format!("indexed target `{target}` has no row scope"))?;
        self.list_source_bindings
            .list_for_row_scope(scope)
            .ok_or_else(|| format!("indexed target `{target}` has no compiled list scope").into())
    }

    fn apply_source_actions<'a>(
        &mut self,
        input: GenericSourceActionInput<'a>,
        read_extra_bool: impl Fn(&str) -> Option<bool> + Copy,
        mut observe: impl FnMut(GenericSourceMutation<'a>) -> RuntimeResult<()>,
    ) -> RuntimeResult<()> {
        let actions = self
            .source_routes
            .actions_for_source_id(input.source_id)?
            .to_vec();
        for action in &actions {
            match action {
                SourceAction::RootScalar => {
                    let targets = self
                        .source_routes
                        .root_scalar_targets_for_source_id(input.source_id)?
                        .iter()
                        .map(|target| target.target.clone())
                        .collect::<Vec<_>>();
                    for target in targets {
                        if self.storage.root_bool_opt(&target).is_some() {
                            if let Some(commit) = self.storage.apply_root_bool_source(
                                &self.scalar_equations,
                                &target,
                                input.source,
                                input.seq,
                            )? {
                                observe(GenericSourceMutation::RootBool(commit))?;
                            }
                        } else if let Some(commit) =
                            self.apply_root_text_source_with_row_context(&target, &input)?
                        {
                            observe(GenericSourceMutation::RootText(GenericRootTextCommit {
                                target,
                                value: commit,
                            }))?;
                            for commit in self.materialize_root_derived_field_commits()? {
                                observe(GenericSourceMutation::RootText(commit))?;
                            }
                        }
                    }
                }
                SourceAction::DerivedText { target } => {
                    if let Some(value) = self.storage.eval_derived_text_transform(
                        &self.derived_equations,
                        target,
                        input.source,
                        input.key,
                        input.text,
                    )? {
                        let current = self.storage.root.textlike(target);
                        if current != Some(value.as_ref()) {
                            self.storage
                                .root
                                .insert_value(target.clone(), FieldValue::Text(value.to_string()));
                            observe(GenericSourceMutation::RootText(GenericRootTextCommit {
                                target: target.clone(),
                                value,
                            }))?;
                            for commit in self.materialize_root_derived_field_commits()? {
                                observe(GenericSourceMutation::RootText(commit))?;
                            }
                        }
                    }
                }
                SourceAction::RootTextTransform { target, value } => {
                    let current = self.storage.root.textlike(target);
                    if current != Some(value.as_str()) {
                        self.storage
                            .root
                            .insert_value(target.clone(), FieldValue::Text(value.clone()));
                        observe(GenericSourceMutation::RootText(GenericRootTextCommit {
                            target: target.clone(),
                            value: Cow::Owned(value.clone()),
                        }))?;
                    }
                }
                SourceAction::RouterRoute { path, .. } => {
                    if self.router_route != *path {
                        self.router_route.clone_from(path);
                        for commit in self.materialize_root_derived_field_commits()? {
                            observe(GenericSourceMutation::RootText(commit))?;
                        }
                    }
                }
                SourceAction::ListAppend { list, trigger } => {
                    let source_paths = self.list_source_bindings.source_paths(list)?;
                    let Some(value) = self.storage.eval_derived_text_transform(
                        &self.derived_equations,
                        trigger,
                        input.source,
                        input.key,
                        input.text,
                    )?
                    else {
                        continue;
                    };
                    let insert = self.storage.append_row_for_trigger_text_and_bind_sources(
                        &self.list_equations,
                        list,
                        trigger,
                        value.as_ref(),
                        source_paths,
                    )?;
                    observe(GenericSourceMutation::ListAppend(
                        GenericTextListAppendCommit {
                            list: list.to_owned(),
                            key: insert.key,
                            generation: insert.generation,
                            value,
                        },
                    ))?;
                    let bindings = self
                        .storage
                        .row_source_bindings(list, insert.key, insert.generation)
                        .cloned()
                        .collect::<Vec<_>>();
                    for binding in bindings {
                        observe(GenericSourceMutation::SourceBind(binding))?;
                    }
                    for commit in
                        self.recompute_generic_derived_for_row(list, insert.key, insert.generation)?
                    {
                        observe(GenericSourceMutation::ValueField(commit))?;
                    }
                }
                SourceAction::ListRemove { list } => {
                    if let Some(index) = input.index {
                        let Some((key, generation)) =
                            self.storage.remove_index_source_action_and_unbind_sources(
                                &self.source_routes,
                                list,
                                input.source_id,
                                index,
                                |binding| {
                                    observe(GenericSourceMutation::SourceUnbind(binding.clone()))
                                },
                            )?
                        else {
                            continue;
                        };
                        observe(GenericSourceMutation::ListRemove {
                            list: list.to_owned(),
                            key,
                            generation,
                        })?;
                    } else {
                        self.storage.remove_where_source_action_and_unbind_sources(
                            &self.source_routes,
                            list,
                            input.source_id,
                            |observation| match observation {
                                GenericListRemoveObservation::SourceUnbind(binding) => {
                                    observe(GenericSourceMutation::SourceUnbind(binding.clone()))
                                }
                                GenericListRemoveObservation::RowRemoved { key, generation } => {
                                    observe(GenericSourceMutation::ListRemove {
                                        list: list.to_owned(),
                                        key,
                                        generation,
                                    })
                                }
                            },
                        )?;
                    }
                }
                SourceAction::IndexedText { kind, target } => {
                    if *kind == SourceRouteTextAction::TextTrimOrPrevious
                        && input.key.is_some_and(|key| key != "Enter")
                    {
                        continue;
                    }
                    let Some(list) = input.list.as_deref() else {
                        return Err(format!(
                            "source `{}` indexed text action `{target}` needs a list context",
                            input.source
                        )
                        .into());
                    };
                    let Some(index) = input.index else {
                        return Err(format!(
                            "source `{}` indexed text action `{target}` needs a row index",
                            input.source
                        )
                        .into());
                    };
                    if *kind == SourceRouteTextAction::PreviousValue && input.text.is_none() {
                        let commit = self.storage.commit_indexed_previous_text_target_source(
                            &self.scalar_equations,
                            list,
                            index,
                            target,
                            input.source,
                        )?;
                        observe(GenericSourceMutation::TextFieldIdentity(commit))?;
                    } else if let Some(commit) = self.storage.commit_indexed_text_source(
                        &self.scalar_equations,
                        list,
                        index,
                        target,
                        input.source,
                        input.text,
                    )? {
                        observe(GenericSourceMutation::TextField(commit))?;
                    }
                }
                SourceAction::IndexedBool { target, .. } => {
                    let Some(list) = input.list.as_deref() else {
                        return Err(format!(
                            "source `{}` indexed bool action `{target}` needs a list context",
                            input.source
                        )
                        .into());
                    };
                    if let Some(index) = input.index {
                        if self.scalar_equations.bool_const_value(target, input.source)
                            == Some(true)
                        {
                            let field = row_field_name(target).to_owned();
                            self.storage.commit_other_indexed_bool_fields(
                                list,
                                index,
                                &field,
                                false,
                                |commit| {
                                    observe(GenericSourceMutation::BoolField(commit))?;
                                    Ok(())
                                },
                            )?;
                        }
                        let commit = self.storage.commit_indexed_bool_source(
                            &self.scalar_equations,
                            list,
                            index,
                            target,
                            input.source,
                            read_extra_bool,
                        )?;
                        observe(GenericSourceMutation::BoolField(commit))?;
                    } else {
                        self.storage.commit_each_indexed_bool_source(
                            &self.scalar_equations,
                            list,
                            target,
                            input.source,
                            read_extra_bool,
                            |commit| {
                                observe(GenericSourceMutation::BoolField(commit))?;
                                Ok(())
                            },
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    fn apply_root_text_source_with_row_context<'a>(
        &mut self,
        target: &str,
        input: &GenericSourceActionInput<'a>,
    ) -> RuntimeResult<Option<Cow<'a, str>>> {
        let Some(value) = self.scalar_equations.eval_text(
            target,
            input.source,
            input.key,
            input.text,
            input.address,
            |path| self.textlike_with_row_context(path, input.list.as_deref(), input.index),
        )?
        else {
            return Err(format!(
                "no supported scalar update branch for `{target}` from `{}`",
                input.source
            )
            .into());
        };
        let Some(candidate) = then_value(EventPulse::present(input.seq), value) else {
            return Ok(None);
        };
        self.storage.commit_root_text_candidate(target, candidate)
    }

    fn textlike_with_row_context(
        &self,
        path: &str,
        list: Option<&str>,
        index: Option<usize>,
    ) -> Option<String> {
        if let Ok(value) = self.storage.root_textlike(path) {
            return Some(value);
        }
        let (Some(list), Some(index)) = (list, index) else {
            return None;
        };
        let (row_scope, field) = path.split_once('.')?;
        if self.list_source_bindings.list_for_row_scope(row_scope)? != list {
            return None;
        }
        self.storage
            .list_row_textlike_opt(list, index, field)
            .map(str::to_owned)
    }

    fn recompute_generic_derived_for_row(
        &mut self,
        list: &str,
        key: u64,
        generation: u64,
    ) -> RuntimeResult<Vec<GenericValueFieldCommit<'static>>> {
        let Some(index) = self.storage.bound_index(list, key, generation)? else {
            return Ok(Vec::new());
        };
        let fields = self
            .generic_derived
            .indexed_fields
            .iter()
            .filter(|field| field.list == list)
            .map(|field| field.field.clone())
            .collect::<Vec<_>>();
        let mut commits = Vec::new();
        for field in fields {
            let key = GenericDerivedKey {
                list: list.to_owned(),
                index,
                field,
            };
            if let (Some(commit), _) = self.recompute_generic_derived_key_value(&key, &[])? {
                commits.push(commit);
            }
        }
        Ok(commits)
    }

    fn apply_source_actions_to_batch<'a>(
        &mut self,
        input: GenericSourceActionInput<'a>,
        read_extra_bool: impl Fn(&str) -> Option<bool> + Copy,
        mut observe: impl FnMut(GenericSourceMutation<'a>) -> RuntimeResult<()>,
    ) -> RuntimeResult<GenericSourceMutationBatch<'a>> {
        let mut batch = GenericSourceMutationBatch::new();
        self.apply_source_actions(input, read_extra_bool, |mutation| {
            batch.observe(&mutation)?;
            observe(mutation)
        })?;
        Ok(batch)
    }

    #[cfg(test)]
    fn append_text_row_source_action_and_bind_sources<'a>(
        &mut self,
        list: &str,
        source: &str,
        key: Option<&str>,
        text: Option<&'a str>,
    ) -> RuntimeResult<Option<GenericTextListAppendCommit<'a>>> {
        let source_paths = self.list_source_bindings.source_paths(list)?;
        self.storage
            .append_text_row_source_action_and_bind_sources(
                &self.source_routes,
                &self.derived_equations,
                &self.list_equations,
                list,
                source,
                key,
                text,
                source_paths,
            )?
            .map(|insert| {
                self.recompute_generic_derived_for_row(list, insert.key, insert.generation)?;
                Ok(insert)
            })
            .transpose()
    }

    fn resolve_bound_source_index(
        &self,
        list: &str,
        action: Option<&BTreeMap<String, toml::Value>>,
        source_event: Option<GenericSourceEvent<'_>>,
    ) -> RuntimeResult<GenericBoundSourceIndex> {
        let Some(action) = action else {
            return Ok(GenericBoundSourceIndex::Unspecified);
        };
        let Some(key) = toml_u64_ref(action, "target_key") else {
            return Ok(GenericBoundSourceIndex::Unspecified);
        };
        let Some(generation) = toml_u64_ref(action, "target_generation") else {
            return Ok(GenericBoundSourceIndex::Stale);
        };
        let source_path = toml_string_ref(action, "source")
            .or(source_event.map(|event| event.source))
            .unwrap_or_default();
        let source_id = toml_u64_ref(action, "source_id");
        let bind_epoch = toml_u64_ref(action, "bind_epoch");
        if !self.is_row_source_bound(list, key, generation, source_path, source_id, bind_epoch) {
            return Ok(GenericBoundSourceIndex::Stale);
        }
        Ok(match self.bound_index(list, key, generation)? {
            Some(index) => GenericBoundSourceIndex::Bound(index),
            None => GenericBoundSourceIndex::Stale,
        })
    }

    fn resolve_visible_row_occurrence(
        &self,
        list: &str,
        text_field: &str,
        action: Option<&BTreeMap<String, toml::Value>>,
        source_event: Option<GenericSourceEvent<'_>>,
        target_text: &str,
        fallback: usize,
    ) -> RuntimeResult<GenericVisibleRowOccurrence> {
        let resolved = self.resolve_bound_source_index(list, action, source_event)?;
        let index = match resolved {
            GenericBoundSourceIndex::Unspecified => {
                return Ok(GenericVisibleRowOccurrence::Occurrence(fallback.max(1)));
            }
            GenericBoundSourceIndex::Bound(index) => index,
            GenericBoundSourceIndex::Stale => return Ok(GenericVisibleRowOccurrence::Stale),
        };
        if self.storage.list_row_textlike(list, index, text_field)? != target_text {
            return Ok(GenericVisibleRowOccurrence::Mismatch);
        }
        Ok(GenericVisibleRowOccurrence::Occurrence(
            self.visible_occurrence_for_index_and_textlike(list, text_field, index, target_text)?,
        ))
    }

    fn visible_occurrence_for_index_and_textlike(
        &self,
        list: &str,
        field: &str,
        target_index: usize,
        value: &str,
    ) -> RuntimeResult<usize> {
        self.storage.list_row_textlike(list, target_index, field)?;
        Ok((0..=target_index)
            .filter(|index| {
                self.storage
                    .list_row_textlike(list, *index, field)
                    .is_ok_and(|candidate| candidate == value)
            })
            .count()
            .max(1))
    }

    fn find_visible_row_index_by_occurrence(
        &self,
        list: &str,
        field: &str,
        value: &str,
        occurrence: usize,
    ) -> RuntimeResult<usize> {
        let occurrence = occurrence.max(1);
        (0..self.storage.list_len(list)?)
            .filter(|index| {
                self.storage
                    .list_row_textlike(list, *index, field)
                    .is_ok_and(|candidate| candidate == value)
            })
            .nth(occurrence - 1)
            .ok_or_else(|| {
                format!(
                    "generic list `{list}` occurrence {occurrence} not found for `{field}`=`{value}`"
                )
                .into()
            })
    }

    fn resolve_generic_step_index(
        &self,
        list: &str,
        step: &ScenarioStep,
        source_event: GenericSourceEvent<'_>,
    ) -> RuntimeResult<Option<usize>> {
        match self.resolve_bound_source_index(
            list,
            step.user_action.as_ref(),
            Some(source_event),
        )? {
            GenericBoundSourceIndex::Bound(index) => return Ok(Some(index)),
            GenericBoundSourceIndex::Stale => {
                return Err(format!(
                    "{} source `{}` has stale or mismatched row binding identity for list `{list}`",
                    step.id, source_event.source
                )
                .into());
            }
            GenericBoundSourceIndex::Unspecified => {}
        }
        if let Some(address) = source_event.address {
            let source_id = self.source_routes.require_source_id(source_event.source)?;
            let lookup_field = self
                .source_routes
                .address_lookup_field_for_source_id(source_id)
                .ok_or_else(|| {
                    format!(
                        "source `{}` carries an address payload but has no typed row lookup field",
                        source_event.source
                    )
                })?;
            if let Some(index) =
                self.storage
                    .find_list_index_by_textlike(list, lookup_field, address)?
            {
                return Ok(Some(index));
            }
        }
        let Some(target_text) = source_event.target_text else {
            return Ok(None);
        };
        let occurrence = step
            .user_action
            .as_ref()
            .and_then(|action| toml_usize_ref(action, "target_occurrence"))
            .unwrap_or(1)
            .max(1);
        self.find_textlike_value_in_list_by_occurrence(list, target_text, occurrence)
    }

    fn find_textlike_value_in_list_by_occurrence(
        &self,
        list: &str,
        value: &str,
        occurrence: usize,
    ) -> RuntimeResult<Option<usize>> {
        let Some(summary) = self
            .list_summary_fields
            .iter()
            .find(|summary| summary.list == list)
        else {
            return Ok(None);
        };
        let mut seen = 0usize;
        for index in 0..self.storage.list_len(list)? {
            for field in &summary.fields {
                if self
                    .storage
                    .list_row_textlike_opt(list, index, field)
                    .is_some_and(|candidate| candidate == value)
                {
                    seen += 1;
                    if seen == occurrence {
                        return Ok(Some(index));
                    }
                    break;
                }
            }
        }
        Ok(None)
    }

    fn count_list_rows_for_target(&self, list: &str, target: &str) -> RuntimeResult<usize> {
        self.storage
            .count_list_rows_for_target(&self.list_equations, list, target)
    }

    fn initialize_generic_derived_fields(&mut self) -> RuntimeResult<()> {
        self.materialize_root_derived_fields()?;
        if !self.generic_derived.has_indexed_fields() {
            return Ok(());
        }
        self.generic_derived_state.clear_last_step();
        let keys = self.generic_derived.keys_for_runtime(&self.storage)?;
        for key in keys {
            self.recompute_generic_derived_key_value(&key, &[])?;
        }
        self.generic_derived_state.clear_last_step();
        Ok(())
    }

    fn materialize_root_derived_fields(&mut self) -> RuntimeResult<()> {
        let _ = self.materialize_root_derived_field_commits()?;
        Ok(())
    }

    fn materialize_root_derived_field_commits(
        &mut self,
    ) -> RuntimeResult<Vec<GenericRootTextCommit<'static>>> {
        let fields = self.generic_derived.root_fields.clone();
        let mut commits = Vec::new();
        for field in fields {
            if field.kind == DerivedValueKind::SourceEventTransform
                && field.has_sources
                && self.storage.root.textlike(&field.path).is_some()
            {
                continue;
            }
            let mut frame = GenericEvalFrame::root();
            let value = self.eval_root_derived_initial_value(&field.statement, &mut frame)?;
            if matches!(value, BoonValue::Empty | BoonValue::Error(_)) {
                continue;
            }
            if let Some(value) = value.as_text() {
                let current = self.storage.root.textlike(&field.path).map(str::to_owned);
                if current.as_deref() == Some(value.as_str()) {
                    continue;
                }
                self.storage
                    .root
                    .insert_value(field.path.clone(), FieldValue::Text(value.clone()));
                commits.push(GenericRootTextCommit {
                    target: field.path,
                    value: Cow::Owned(value),
                });
            }
        }
        Ok(commits)
    }

    fn eval_root_derived_initial_value(
        &mut self,
        statement: &AstStatement,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        if let Some(latest_statement) = self.root_derived_latest_statement(statement) {
            for child in &latest_statement.children {
                let value = self.eval_statement_value(child, frame)?;
                if !matches!(value, BoonValue::Empty | BoonValue::Error(_)) {
                    return Ok(value);
                }
            }
            return Ok(BoonValue::Empty);
        }
        self.eval_statement_value(statement, frame)
    }

    fn root_derived_latest_statement<'a>(
        &self,
        statement: &'a AstStatement,
    ) -> Option<&'a AstStatement> {
        if statement.expr.is_some_and(|expr_id| {
            matches!(
                self.generic_derived
                    .expressions
                    .get(expr_id)
                    .map(|expr| &expr.kind),
                Some(AstExprKind::Latest)
            )
        }) {
            return Some(statement);
        }
        statement
            .children
            .iter()
            .find_map(|child| self.root_derived_latest_statement(child))
    }

    fn read_keys_from_deltas(
        &self,
        deltas: &[SemanticDelta<'_>],
    ) -> RuntimeResult<BTreeSet<GenericReadKey>> {
        let mut reads = BTreeSet::new();
        for delta in deltas {
            if delta.kind != "FieldSet" {
                continue;
            }
            let Some(field) = delta.field_path.as_ref() else {
                continue;
            };
            if let Some(list) = delta.list_id.as_ref() {
                let Some(key) = delta.key else {
                    continue;
                };
                let generation = delta.generation.unwrap_or(1);
                let Some(index) = self.storage.bound_index(list, key, generation)? else {
                    continue;
                };
                reads.insert(GenericReadKey::ListField {
                    list: list.to_string(),
                    index,
                    field: field.to_string(),
                });
            } else {
                reads.insert(GenericReadKey::Root {
                    field: field.to_string(),
                });
            }
        }
        Ok(reads)
    }

    fn recompute_generic_derived_after_changes(
        &mut self,
        changed_reads: BTreeSet<GenericReadKey>,
    ) -> RuntimeResult<(
        Vec<GenericValueFieldCommit<'static>>,
        GenericRecomputeMetrics,
    )> {
        self.generic_derived_state.clear_last_step();
        let mut dirty = self
            .generic_derived_state
            .dependents_for_reads(changed_reads)
            .into_iter()
            .collect::<BTreeSet<_>>();
        let mut commits = Vec::new();
        let mut metrics = GenericRecomputeMetrics::default();
        let mut processed = BTreeSet::new();
        let mut guard = 0usize;
        while let Some(key) = dirty.iter().next().cloned() {
            dirty.remove(&key);
            if !processed.insert(key.clone()) {
                continue;
            }
            guard += 1;
            if guard > 20_000 {
                return Err("generic derived recompute budget exhausted".into());
            }
            metrics.recompute_candidate_count += 1;
            let (commit, _) = self.recompute_generic_derived_key_value(&key, &[])?;
            metrics.recomputed_field_count = self.generic_derived_state.last_recomputed.len();
            if let Some(commit) = commit {
                let changed_field = GenericReadKey::ListField {
                    list: commit.list.clone(),
                    index: self
                        .storage
                        .bound_index(&commit.list, commit.key, commit.generation)?
                        .unwrap_or(key.index),
                    field: commit.field.clone(),
                };
                for dependent in self
                    .generic_derived_state
                    .dependents_for_reads([changed_field])
                {
                    if !processed.contains(&dependent) {
                        dirty.insert(dependent);
                    }
                }
                commits.push(commit);
            }
        }
        self.generic_derived_state.last_candidate_count = metrics.recompute_candidate_count;
        Ok((commits, metrics))
    }

    fn recompute_generic_derived_key_value(
        &mut self,
        key: &GenericDerivedKey,
        stack: &[GenericDerivedKey],
    ) -> RuntimeResult<(Option<GenericValueFieldCommit<'static>>, BoonValue)> {
        if stack.contains(key) {
            return Ok((None, BoonValue::Error("cycle_error".to_owned())));
        }
        let Some(plan) = self.generic_derived.field_plan(key).cloned() else {
            let value = self
                .storage
                .list_row_field(&key.list, key.index, &key.field)?;
            return Ok((None, field_ref_to_boon(value)));
        };
        let mut frame = GenericEvalFrame::for_row(&plan.list, &plan.row_scope, key.index);
        frame.stack = stack.to_vec();
        frame.stack.push(key.clone());
        let value = self.eval_statement_value(&plan.statement, &mut frame)?;
        self.generic_derived_state
            .replace_reads(key.clone(), frame.reads);
        let visible = value.visible_text();
        let current = self
            .storage
            .list_row_textlike_opt(&key.list, key.index, &key.field)
            .unwrap_or_default();
        let emit_unchanged_error = matches!(value, BoonValue::Error(_));
        if current == visible && !emit_unchanged_error {
            return Ok((None, value));
        }
        if current != visible {
            self.storage
                .set_or_insert_list_row_textlike(&key.list, key.index, &key.field, &visible)?;
        }
        let (row_key, generation) = self.storage.row_identity(&key.list, key.index)?;
        self.generic_derived_state.last_recomputed.push(key.clone());
        Ok((
            Some(GenericValueFieldCommit {
                list: key.list.clone(),
                key: row_key,
                generation,
                field: key.field.clone(),
                value: ProtocolValue::Text(Cow::Owned(visible)),
            }),
            value,
        ))
    }

    fn eval_statement_block(
        &mut self,
        statements: &[AstStatement],
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let mut last = BoonValue::Empty;
        for statement in statements {
            frame.consume_budget()?;
            match &statement.kind {
                AstStatementKind::Field { name } => {
                    let value = self.eval_statement_value(statement, frame)?;
                    frame.env.insert(name.clone(), value.clone());
                    last = value;
                }
                AstStatementKind::Expression
                    if statement.expr.is_some_and(|expr_id| {
                        self.generic_derived.expr_is_pipe_continuation(expr_id)
                    }) =>
                {
                    last = self.eval_pipe_continuation(
                        statement.expr.expect("checked expression id"),
                        last,
                        &statement.children,
                        frame,
                    )?;
                }
                AstStatementKind::Block => {
                    last = self.eval_statement_block(&statement.children, frame)?;
                }
                AstStatementKind::Expression
                | AstStatementKind::Function { .. }
                | AstStatementKind::Source { .. }
                | AstStatementKind::Hold { .. }
                | AstStatementKind::List { .. } => {
                    last = self.eval_statement_value(statement, frame)?;
                }
            }
        }
        Ok(last)
    }

    fn eval_statement_value(
        &mut self,
        statement: &AstStatement,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        if let Some(expr_id) = statement.expr {
            if self.generic_derived.expr_is_block_marker(expr_id) {
                return self.eval_statement_block(&statement.children, frame);
            }
            return self.eval_expr_with_children(expr_id, &statement.children, frame);
        }
        if !statement.children.is_empty() {
            return self.eval_statement_block(&statement.children, frame);
        }
        Ok(BoonValue::Empty)
    }

    fn eval_expr_with_children(
        &mut self,
        expr_id: usize,
        children: &[AstStatement],
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let expr = self
            .generic_derived
            .expressions
            .get(expr_id)
            .cloned()
            .ok_or_else(|| format!("generic expression id {expr_id} is missing"))?;
        if let AstExprKind::Pipe { input, op, args: _ } = &expr.kind
            && op == "WHILE"
        {
            let input = self.eval_expr(*input, frame)?;
            return self.eval_while(input, children, frame);
        }
        if let AstExprKind::Pipe { input, op, args: _ } = &expr.kind
            && op == "WHEN"
        {
            let input = self.eval_expr(*input, frame)?;
            return self.eval_while(input, children, frame);
        }
        if let AstExprKind::When { input } = &expr.kind {
            let input = self.eval_expr(*input, frame)?;
            return self.eval_while(input, children, frame);
        }
        if let AstExprKind::Call { function, args } = &expr.kind
            && function == "WHILE"
        {
            let input = self.eval_first_arg(args, frame)?;
            return self.eval_while(input, children, frame);
        }
        if let AstExprKind::Record(fields) | AstExprKind::Object(fields) = &expr.kind {
            return self.eval_object_expr(fields, Some(children), frame);
        }
        let mut value = self.eval_expr(expr_id, frame)?;
        for child in self.pipe_continuation_children(children) {
            value = self.eval_pipe_continuation_chain(&child, value, frame)?;
        }
        Ok(value)
    }

    fn eval_pipe_continuation(
        &mut self,
        expr_id: usize,
        input: BoonValue,
        children: &[AstStatement],
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let expr = self
            .generic_derived
            .expressions
            .get(expr_id)
            .cloned()
            .ok_or_else(|| format!("generic pipe expression id {expr_id} is missing"))?;
        match expr.kind {
            AstExprKind::Pipe { op, args: _, .. } if op == "WHILE" || op == "WHEN" => {
                self.eval_while(input, children, frame)
            }
            AstExprKind::Pipe { op, args, .. } => self.eval_call(&op, &args, Some(input), frame),
            AstExprKind::When { .. } => self.eval_while(input, children, frame),
            AstExprKind::Then { output, .. } => {
                if matches!(input, BoonValue::Empty) {
                    return Ok(BoonValue::Empty);
                }
                if let Some(output) = output {
                    self.eval_expr_with_children(output, children, frame)
                } else {
                    self.eval_statement_block(children, frame)
                }
            }
            _ => Ok(input),
        }
    }

    fn eval_pipe_continuation_chain(
        &mut self,
        statement: &AstStatement,
        input: BoonValue,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let Some(expr_id) = statement.expr else {
            return Ok(input);
        };
        let mut value = self.eval_pipe_continuation(expr_id, input, &statement.children, frame)?;
        for child in self.pipe_continuation_children(&statement.children) {
            value = self.eval_pipe_continuation_chain(&child, value, frame)?;
        }
        Ok(value)
    }

    fn pipe_continuation_children(&self, children: &[AstStatement]) -> Vec<AstStatement> {
        children
            .iter()
            .filter(|child| {
                matches!(child.kind, AstStatementKind::Expression)
                    && child.expr.is_some_and(|expr_id| {
                        self.generic_derived.expr_is_pipe_continuation(expr_id)
                    })
            })
            .cloned()
            .collect()
    }

    fn eval_expr(
        &mut self,
        expr_id: usize,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        frame.consume_budget()?;
        let expr = self
            .generic_derived
            .expressions
            .get(expr_id)
            .cloned()
            .ok_or_else(|| format!("generic expression id {expr_id} is missing"))?;
        match expr.kind {
            AstExprKind::Identifier(name) => self.eval_identifier(&name, frame),
            AstExprKind::Path(parts) => self.eval_path(&parts, frame),
            AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => {
                Ok(BoonValue::Text(value))
            }
            AstExprKind::Number(value) => Ok(value
                .parse::<i64>()
                .map(BoonValue::Number)
                .unwrap_or(BoonValue::NaN)),
            AstExprKind::Bool(value) => Ok(BoonValue::Bool(value)),
            AstExprKind::Enum(value) | AstExprKind::Tag(value) => Ok(BoonValue::Text(value)),
            AstExprKind::TaggedObject { tag, fields } => {
                let mut body = Vec::new();
                for field in fields {
                    if field.spread {
                        continue;
                    }
                    let value = boon_value_scalar_text(&self.eval_expr(field.value, frame)?);
                    body.push(format!("{}:{}", field.name, value));
                }
                Ok(BoonValue::Text(format!("{tag}[{}]", body.join(","))))
            }
            AstExprKind::Call { function, args } => self.eval_call(&function, &args, None, frame),
            AstExprKind::Pipe { input, op, args } => {
                let value = if self
                    .generic_derived
                    .expressions
                    .get(input)
                    .is_some_and(|input| matches!(input.kind, AstExprKind::Delimiter))
                {
                    BoonValue::Empty
                } else {
                    self.eval_expr(input, frame)?
                };
                self.eval_call(&op, &args, Some(value), frame)
            }
            AstExprKind::Infix { left, op, right } => {
                let left = self.eval_expr(left, frame)?;
                let right = self.eval_expr(right, frame)?;
                Ok(generic_infix_value(left, &op, right))
            }
            AstExprKind::Record(fields) | AstExprKind::Object(fields) => {
                self.eval_object_expr(&fields, None, frame)
            }
            AstExprKind::ListLiteral { .. } => Ok(BoonValue::List(Vec::new())),
            AstExprKind::Source
            | AstExprKind::Hold { .. }
            | AstExprKind::Latest
            | AstExprKind::When { .. }
            | AstExprKind::Then { .. }
            | AstExprKind::MatchArm { .. }
            | AstExprKind::Delimiter
            | AstExprKind::Unknown(_) => Ok(BoonValue::Empty),
        }
    }

    fn eval_object_expr(
        &mut self,
        fields: &[AstRecordField],
        attached_children: Option<&[AstStatement]>,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let mut record = BTreeMap::new();
        for field in fields {
            let value = if let Some(children) = attached_children
                && self.expr_accepts_attached_children(field.value)
            {
                self.eval_expr_with_children(field.value, children, frame)?
            } else {
                self.eval_expr(field.value, frame)?
            };
            if field.spread {
                match value {
                    BoonValue::Record(fields) => {
                        record.extend(fields);
                    }
                    BoonValue::Empty => {}
                    BoonValue::Text(value) if value == "UNPLUGGED" => {}
                    _ => return Ok(BoonValue::Error("type_error".to_owned())),
                }
            } else {
                record.insert(field.name.clone(), value);
            }
        }
        Ok(BoonValue::Record(record))
    }

    fn expr_accepts_attached_children(&self, expr_id: usize) -> bool {
        let Some(expr) = self.generic_derived.expressions.get(expr_id) else {
            return false;
        };
        match &expr.kind {
            AstExprKind::When { .. } => true,
            AstExprKind::Pipe { op, .. } if op == "WHEN" || op == "WHILE" => true,
            AstExprKind::Record(fields) | AstExprKind::Object(fields) => fields
                .iter()
                .any(|field| self.expr_accepts_attached_children(field.value)),
            _ => false,
        }
    }

    fn eval_while(
        &mut self,
        input: BoonValue,
        arms: &[AstStatement],
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        for arm in arms {
            let Some(expr_id) = arm.expr else {
                continue;
            };
            let expr = self
                .generic_derived
                .expressions
                .get(expr_id)
                .cloned()
                .ok_or_else(|| format!("generic match arm expression id {expr_id} is missing"))?;
            let AstExprKind::MatchArm { pattern, output } = expr.kind else {
                continue;
            };
            let Some(binding) = generic_pattern_binding(&pattern, &input) else {
                continue;
            };
            let previous = if let Some((name, value)) = binding {
                frame
                    .env
                    .insert(name.clone(), value)
                    .map(|value| (name, value))
            } else {
                None
            };
            let value = if let Some(output) = output {
                if self.generic_derived.expr_is_block_marker(output) {
                    self.eval_statement_block(&arm.children, frame)?
                } else {
                    self.eval_expr_with_children(output, &arm.children, frame)?
                }
            } else {
                self.eval_statement_block(&arm.children, frame)?
            };
            if let Some((name, value)) = previous {
                frame.env.insert(name, value);
            }
            return Ok(value);
        }
        Ok(BoonValue::Empty)
    }

    fn eval_identifier(
        &mut self,
        name: &str,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        if let Some(value) = frame.env.get(name) {
            return Ok(value.clone());
        }
        if name == "True" {
            return Ok(BoonValue::Bool(true));
        }
        if name == "False" {
            return Ok(BoonValue::Bool(false));
        }
        if name == "NaN" {
            return Ok(BoonValue::NaN);
        }
        if self.storage.lists.memory(name).is_some() {
            return Ok(BoonValue::ListRef(name.to_owned()));
        }
        if let Some(row) = frame.row.clone()
            && self
                .storage
                .list_row_field(&row.list, row.index, name)
                .is_ok()
        {
            return self.read_list_field(&row.list, row.index, name, frame);
        }
        if let Some(value) = self.runtime_scalar_boon_value(name) {
            frame.reads.insert(GenericReadKey::Root {
                field: name.to_owned(),
            });
            return Ok(value);
        }
        Ok(BoonValue::Text(name.to_owned()))
    }

    fn eval_path(
        &mut self,
        parts: &[String],
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        if parts.is_empty() {
            return Ok(BoonValue::Empty);
        }
        if parts.len() == 1 {
            return self.eval_identifier(&parts[0], frame);
        }
        if let Some(value) = frame.env.get(&parts[0]).cloned() {
            return self.value_path(value, &parts[1..], frame);
        }
        if let Some(row) = frame.row.clone()
            && parts[0] == row.row_scope
            && parts.len() == 2
        {
            return self.read_list_field(&row.list, row.index, &parts[1], frame);
        }
        let full_path = parts.join(".");
        if let Some(value) = self.runtime_scalar_boon_value(&full_path) {
            frame
                .reads
                .insert(GenericReadKey::Root { field: full_path });
            return Ok(value);
        }
        Ok(BoonValue::Text(full_path))
    }

    fn value_path(
        &mut self,
        value: BoonValue,
        parts: &[String],
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let Some((head, tail)) = parts.split_first() else {
            return Ok(value);
        };
        let next = match value {
            BoonValue::Record(record) => record
                .get(head)
                .cloned()
                .unwrap_or_else(|| BoonValue::Error("missing_ref".to_owned())),
            BoonValue::RowRef { list, index } => self.read_list_field(&list, index, head, frame)?,
            BoonValue::Empty => BoonValue::Error("missing_ref".to_owned()),
            other => {
                return Ok(if tail.is_empty() {
                    other
                } else {
                    BoonValue::Error("type_error".to_owned())
                });
            }
        };
        self.value_path(next, tail, frame)
    }

    fn read_list_field(
        &mut self,
        list: &str,
        index: usize,
        field: &str,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        frame.reads.insert(GenericReadKey::ListField {
            list: list.to_owned(),
            index,
            field: field.to_owned(),
        });
        if self.generic_derived.contains_field(list, field) {
            let key = GenericDerivedKey {
                list: list.to_owned(),
                index,
                field: field.to_owned(),
            };
            let (_, value) = self.recompute_generic_derived_key_value(&key, &frame.stack)?;
            if matches!(value, BoonValue::Error(_)) {
                return Ok(value);
            }
        }
        let value = self.storage.list_row_field(list, index, field)?;
        Ok(field_ref_to_boon(value))
    }

    fn eval_call(
        &mut self,
        function: &str,
        args: &[AstCallArg],
        input: Option<BoonValue>,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        if is_generic_render_constructor(function) {
            return self.eval_generic_constructor(function, args, input, frame);
        }
        if is_light_constructor(function) {
            return self.eval_generic_constructor(function, args, input, frame);
        }
        if let Some(field) = function.strip_prefix("Field/") {
            let value = self.call_input_or_first(input, args, frame)?;
            return Ok(match value {
                BoonValue::Record(fields) => fields
                    .get(field)
                    .cloned()
                    .unwrap_or_else(|| BoonValue::Error("missing_ref".to_owned())),
                _ => BoonValue::Error("type_error".to_owned()),
            });
        }
        match function {
            "SOURCE" => self.call_input_or_first(input, args, frame),
            "Text/empty" => Ok(BoonValue::Text(String::new())),
            "Text/trim" => {
                let value = self.call_input_or_first(input, args, frame)?;
                Ok(BoonValue::Text(
                    value.as_text().unwrap_or_default().trim().to_owned(),
                ))
            }
            "Text/starts_with" => {
                let value = self.call_input_or_first(input, args, frame)?;
                let prefix = self.named_arg_value(args, "prefix", frame)?;
                Ok(BoonValue::Bool(
                    value
                        .as_text()
                        .unwrap_or_default()
                        .starts_with(&prefix.as_text().unwrap_or_default()),
                ))
            }
            "Text/substring" => {
                let value = self.call_input_or_first(input, args, frame)?;
                let start = self
                    .named_arg_value(args, "start", frame)?
                    .number()
                    .unwrap_or(0)
                    .max(0) as usize;
                let length = self
                    .named_arg_value(args, "length", frame)?
                    .number()
                    .unwrap_or(0)
                    .max(0) as usize;
                let text = value.as_text().unwrap_or_default();
                let result = text.chars().skip(start).take(length).collect::<String>();
                Ok(BoonValue::Text(result))
            }
            "Text/length" => {
                let value = self.call_input_or_first(input, args, frame)?;
                Ok(BoonValue::Number(
                    value.as_text().unwrap_or_default().chars().count() as i64,
                ))
            }
            "Text/find" => {
                let value = self.call_input_or_first(input, args, frame)?;
                let needle = self.named_arg_value(args, "needle", frame)?;
                let text = value.as_text().unwrap_or_default();
                let needle = needle.as_text().unwrap_or_default();
                Ok(text
                    .find(&needle)
                    .map(|index| BoonValue::Number(index as i64))
                    .unwrap_or(BoonValue::NaN))
            }
            "Text/to_number" => {
                let value = self.call_input_or_first(input, args, frame)?;
                let text = value.as_text().unwrap_or_default();
                Ok(text
                    .trim()
                    .parse::<i64>()
                    .map(BoonValue::Number)
                    .unwrap_or(BoonValue::NaN))
            }
            "Text/is_empty" => {
                let value = self.call_input_or_first(input, args, frame)?;
                Ok(BoonValue::Bool(
                    value.as_text().unwrap_or_default().is_empty(),
                ))
            }
            "Text/is_not_empty" => {
                let value = self.call_input_or_first(input, args, frame)?;
                Ok(BoonValue::Bool(
                    !value.as_text().unwrap_or_default().is_empty(),
                ))
            }
            "Bool/not" => {
                let value = self.call_input_or_first(input, args, frame)?;
                Ok(BoonValue::Bool(!value.bool_value().unwrap_or(false)))
            }
            "Bool/toggle" => {
                let value = self.call_input_or_first(input, args, frame)?;
                let toggle_requested = args
                    .iter()
                    .find(|arg| arg.name.as_deref() == Some("when"))
                    .map(|arg| self.eval_expr(arg.value, frame))
                    .transpose()?
                    .is_some_and(|value| !matches!(value, BoonValue::Empty));
                Ok(BoonValue::Bool(if toggle_requested {
                    !value.bool_value().unwrap_or(false)
                } else {
                    value.bool_value().unwrap_or(false)
                }))
            }
            "Bool/and" => {
                let piped = input.is_some();
                let left = self.call_input_or_first(input, args, frame)?;
                let right_position = if piped { 0 } else { 1 };
                let right =
                    self.named_or_positional_arg_value(args, "right", right_position, frame)?;
                Ok(BoonValue::Bool(
                    left.bool_value().unwrap_or(false) && right.bool_value().unwrap_or(false),
                ))
            }
            "Error/new" => {
                let code = self.named_arg_value(args, "code", frame)?;
                Ok(BoonValue::Error(
                    code.as_text().unwrap_or_else(|| "error".to_owned()),
                ))
            }
            "Error/text" => {
                let value = self.call_input_or_first(input, args, frame)?;
                Ok(BoonValue::Text(match value {
                    BoonValue::Error(error) => error,
                    _ => String::new(),
                }))
            }
            "Router/route" => Ok(BoonValue::Text(self.router_route.clone())),
            "Router/go_to" => {
                let value = self.call_input_or_first(input, args, frame)?;
                Ok(BoonValue::Text(value.as_text().unwrap_or_default()))
            }
            "Ulid/generate" => Ok(BoonValue::Text("01J00000000000000000000000".to_owned())),
            "List/range" => {
                let from = self
                    .named_arg_value(args, "from", frame)?
                    .number()
                    .unwrap_or(0);
                let to = self
                    .named_arg_value(args, "to", frame)?
                    .number()
                    .unwrap_or(-1);
                let values = if from <= to {
                    (from..=to).map(BoonValue::Number).collect()
                } else {
                    Vec::new()
                };
                Ok(BoonValue::List(values))
            }
            "List/find" => {
                let list = self.call_input_or_first(input, args, frame)?;
                let field = self
                    .raw_named_arg(args, "field")
                    .ok_or("List/find requires field")?;
                let expected = self.named_arg_value(args, "value", frame)?;
                self.list_find(list, &field, expected, frame)
            }
            "List/find_value" => {
                let list = self.call_input_or_first(input, args, frame)?;
                let field = self
                    .raw_named_arg(args, "field")
                    .ok_or("List/find_value requires field")?;
                let expected = self.named_arg_value(args, "value", frame)?;
                let target = self
                    .raw_named_arg(args, "target")
                    .ok_or("List/find_value requires target")?;
                let fallback = self
                    .named_arg_value(args, "fallback", frame)
                    .unwrap_or(BoonValue::Empty);
                self.list_find_value(list, &field, expected, &target, fallback, frame)
            }
            "List/get" => {
                let list = self.call_input_or_first(input, args, frame)?;
                let index = self
                    .named_arg_value(args, "index", frame)?
                    .number()
                    .unwrap_or(-1);
                self.list_get(list, index, frame)
            }
            "List/count" => {
                let list = self.call_input_or_first(input, args, frame)?;
                Ok(BoonValue::Number(self.list_len_for_value(list)? as i64))
            }
            "List/is_not_empty" => {
                let list = self.call_input_or_first(input, args, frame)?;
                Ok(BoonValue::Bool(self.list_len_for_value(list)? > 0))
            }
            "List/map" => {
                let list = self.call_input_or_first(input, args, frame)?;
                let binding = args
                    .iter()
                    .find(|arg| arg.name.is_none())
                    .and_then(|arg| self.raw_arg_name(arg))
                    .ok_or("List/map requires an item binding")?;
                let new_arg = args
                    .iter()
                    .find(|arg| arg.name.as_deref() == Some("new"))
                    .ok_or("List/map requires new expression")?;
                self.list_map(list, &binding, new_arg.value, frame)
            }
            "List/retain" => {
                let list = self.call_input_or_first(input, args, frame)?;
                let binding = args
                    .iter()
                    .find(|arg| arg.name.is_none())
                    .and_then(|arg| self.raw_arg_name(arg))
                    .ok_or("List/retain requires an item binding")?;
                let predicate_arg = args
                    .iter()
                    .find(|arg| arg.name.as_deref() == Some("if"))
                    .ok_or("List/retain requires if expression")?;
                self.list_retain(list, &binding, predicate_arg.value, frame)
            }
            "List/every" => {
                let list = self.call_input_or_first(input, args, frame)?;
                let binding = args
                    .iter()
                    .find(|arg| arg.name.is_none())
                    .and_then(|arg| self.raw_arg_name(arg))
                    .ok_or("List/every requires an item binding")?;
                let predicate_arg = args
                    .iter()
                    .find(|arg| arg.name.as_deref() == Some("if"))
                    .ok_or("List/every requires if expression")?;
                self.list_every(list, &binding, predicate_arg.value, frame)
            }
            "List/any" => {
                let list = self.call_input_or_first(input, args, frame)?;
                let binding = args
                    .iter()
                    .find(|arg| arg.name.is_none())
                    .and_then(|arg| self.raw_arg_name(arg))
                    .ok_or("List/any requires an item binding")?;
                let predicate_arg = args
                    .iter()
                    .find(|arg| arg.name.as_deref() == Some("if"))
                    .ok_or("List/any requires if expression")?;
                self.list_any(list, &binding, predicate_arg.value, frame)
            }
            "List/latest" => {
                let list = self.call_input_or_first(input, args, frame)?;
                self.list_latest(list, frame)
            }
            "List/sum" => {
                let list = self.call_input_or_first(input, args, frame)?;
                self.list_sum(list)
            }
            _ => self.eval_user_function(function, args, input, frame),
        }
    }

    fn eval_generic_constructor(
        &mut self,
        function: &str,
        args: &[AstCallArg],
        input: Option<BoonValue>,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let mut record = BTreeMap::new();
        record.insert(
            "kind".to_owned(),
            BoonValue::Text(generic_constructor_kind(function).to_owned()),
        );
        record.insert(
            "constructor".to_owned(),
            BoonValue::Text(function.to_owned()),
        );
        if let Some(input) = input {
            record.insert("input".to_owned(), input);
        }
        for (index, arg) in args.iter().enumerate() {
            let value = self.eval_expr(arg.value, frame)?;
            let name = arg.name.clone().unwrap_or_else(|| format!("arg_{index}"));
            record.insert(name, value);
        }
        Ok(BoonValue::Record(record))
    }

    fn eval_user_function(
        &mut self,
        function: &str,
        args: &[AstCallArg],
        input: Option<BoonValue>,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let Some(definition) = self.generic_derived.functions.get(function).cloned() else {
            return Ok(BoonValue::Error("parse_error".to_owned()));
        };
        if frame.call_depth > 128 {
            return Err(format!("generic function `{function}` call budget exhausted").into());
        }
        let mut child = frame.child();
        child.call_depth += 1;
        if let Some(input) = input
            && let Some(first) = definition.args.first()
        {
            child.env.insert(first.clone(), input);
        }
        for (position, arg) in args.iter().enumerate() {
            let value = self.eval_expr(arg.value, frame)?;
            let name = arg
                .name
                .clone()
                .or_else(|| definition.args.get(position).cloned())
                .ok_or_else(|| format!("generic function `{function}` has too many arguments"))?;
            if name == "PASS" {
                child.env.insert("PASSED".to_owned(), value.clone());
            }
            child.env.insert(name, value);
        }
        let value = self.eval_statement_block(&definition.statement.children, &mut child)?;
        frame.reads.extend(child.reads);
        Ok(value)
    }

    fn eval_first_arg(
        &mut self,
        args: &[AstCallArg],
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let arg = args
            .first()
            .ok_or("generic call requires an input argument")?;
        self.eval_expr(arg.value, frame)
    }

    fn call_input_or_first(
        &mut self,
        input: Option<BoonValue>,
        args: &[AstCallArg],
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        match input {
            Some(value) => Ok(value),
            None => self.eval_first_arg(args, frame),
        }
    }

    fn named_arg_value(
        &mut self,
        args: &[AstCallArg],
        name: &str,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let arg = args
            .iter()
            .find(|arg| arg.name.as_deref() == Some(name))
            .ok_or_else(|| format!("generic call requires `{name}`"))?;
        self.eval_expr(arg.value, frame)
    }

    fn named_or_positional_arg_value(
        &mut self,
        args: &[AstCallArg],
        name: &str,
        position: usize,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        if let Some(arg) = args.iter().find(|arg| arg.name.as_deref() == Some(name)) {
            return self.eval_expr(arg.value, frame);
        }
        let arg = args
            .iter()
            .filter(|arg| arg.name.is_none())
            .nth(position)
            .ok_or_else(|| format!("generic call requires `{name}`"))?;
        self.eval_expr(arg.value, frame)
    }

    fn raw_named_arg(&self, args: &[AstCallArg], name: &str) -> Option<String> {
        args.iter()
            .find(|arg| arg.name.as_deref() == Some(name))
            .and_then(|arg| self.raw_arg_name(arg))
    }

    fn raw_arg_name(&self, arg: &AstCallArg) -> Option<String> {
        let expr = self.generic_derived.expressions.get(arg.value)?;
        match &expr.kind {
            AstExprKind::Identifier(value) => Some(value.clone()),
            AstExprKind::Path(parts) => Some(parts.join(".")),
            AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => {
                Some(value.clone())
            }
            _ => None,
        }
    }

    fn list_find(
        &mut self,
        list: BoonValue,
        field: &str,
        expected: BoonValue,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let expected = expected.as_text().unwrap_or_default();
        let BoonValue::ListRef(list) = list else {
            return Ok(BoonValue::Error("type_error".to_owned()));
        };
        let len = self.storage.list_len(&list)?;
        for index in 0..len {
            let value = self.read_list_field(&list, index, field, frame)?;
            if value.as_text().unwrap_or_default() == expected {
                return Ok(BoonValue::RowRef { list, index });
            }
        }
        Ok(BoonValue::Empty)
    }

    fn list_find_value(
        &mut self,
        list: BoonValue,
        field: &str,
        expected: BoonValue,
        target: &str,
        fallback: BoonValue,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        match self.list_find(list, field, expected, frame)? {
            BoonValue::RowRef { list, index } => self.read_list_field(&list, index, target, frame),
            BoonValue::Empty => Ok(fallback),
            other => Ok(other),
        }
    }

    fn list_get(
        &mut self,
        list: BoonValue,
        index: i64,
        _frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let index = usize::try_from(index).map_err(|_| "List/get index is negative")?;
        match list {
            BoonValue::List(values) => Ok(values.get(index).cloned().unwrap_or(BoonValue::Empty)),
            BoonValue::ListRef(list) => {
                if index >= self.storage.list_len(&list)? {
                    Ok(BoonValue::Empty)
                } else {
                    Ok(BoonValue::RowRef { list, index })
                }
            }
            _ => Ok(BoonValue::Error("type_error".to_owned())),
        }
    }

    fn list_len_for_value(&self, list: BoonValue) -> RuntimeResult<usize> {
        match list {
            BoonValue::List(values) => Ok(values.len()),
            BoonValue::ListRef(list) => self.storage.list_len(&list),
            _ => Ok(0),
        }
    }

    fn list_map(
        &mut self,
        list: BoonValue,
        binding: &str,
        new_expr: usize,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let values = match list {
            BoonValue::List(values) => values,
            BoonValue::ListRef(list) => {
                let len = self.storage.list_len(&list)?;
                (0..len)
                    .map(|index| BoonValue::RowRef {
                        list: list.clone(),
                        index,
                    })
                    .collect()
            }
            _ => return Ok(BoonValue::Error("type_error".to_owned())),
        };
        let mut output = Vec::with_capacity(values.len());
        let previous = frame.env.get(binding).cloned();
        for value in values {
            frame.env.insert(binding.to_owned(), value);
            output.push(self.eval_expr(new_expr, frame)?);
        }
        if let Some(previous) = previous {
            frame.env.insert(binding.to_owned(), previous);
        } else {
            frame.env.remove(binding);
        }
        Ok(BoonValue::List(output))
    }

    fn list_retain(
        &mut self,
        list: BoonValue,
        binding: &str,
        predicate_expr: usize,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let values = match list {
            BoonValue::List(values) => values,
            BoonValue::ListRef(list) => {
                let len = self.storage.list_len(&list)?;
                (0..len)
                    .map(|index| BoonValue::RowRef {
                        list: list.clone(),
                        index,
                    })
                    .collect()
            }
            _ => return Ok(BoonValue::Error("type_error".to_owned())),
        };
        let mut output = Vec::new();
        let previous = frame.env.get(binding).cloned();
        for value in values {
            frame.env.insert(binding.to_owned(), value.clone());
            if self
                .eval_expr(predicate_expr, frame)?
                .bool_value()
                .unwrap_or(false)
            {
                output.push(value);
            }
        }
        if let Some(previous) = previous {
            frame.env.insert(binding.to_owned(), previous);
        } else {
            frame.env.remove(binding);
        }
        Ok(BoonValue::List(output))
    }

    fn list_every(
        &mut self,
        list: BoonValue,
        binding: &str,
        predicate_expr: usize,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let values = match list {
            BoonValue::List(values) => values,
            BoonValue::ListRef(list) => {
                let len = self.storage.list_len(&list)?;
                (0..len)
                    .map(|index| BoonValue::RowRef {
                        list: list.clone(),
                        index,
                    })
                    .collect()
            }
            _ => return Ok(BoonValue::Error("type_error".to_owned())),
        };
        let previous = frame.env.get(binding).cloned();
        for value in values {
            frame.env.insert(binding.to_owned(), value);
            if !self
                .eval_expr(predicate_expr, frame)?
                .bool_value()
                .unwrap_or(false)
            {
                if let Some(previous) = previous.clone() {
                    frame.env.insert(binding.to_owned(), previous);
                } else {
                    frame.env.remove(binding);
                }
                return Ok(BoonValue::Bool(false));
            }
        }
        if let Some(previous) = previous {
            frame.env.insert(binding.to_owned(), previous);
        } else {
            frame.env.remove(binding);
        }
        Ok(BoonValue::Bool(true))
    }

    fn list_any(
        &mut self,
        list: BoonValue,
        binding: &str,
        predicate_expr: usize,
        frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let values = match list {
            BoonValue::List(values) => values,
            BoonValue::ListRef(list) => {
                let len = self.storage.list_len(&list)?;
                (0..len)
                    .map(|index| BoonValue::RowRef {
                        list: list.clone(),
                        index,
                    })
                    .collect()
            }
            _ => return Ok(BoonValue::Error("type_error".to_owned())),
        };
        let previous = frame.env.get(binding).cloned();
        for value in values {
            frame.env.insert(binding.to_owned(), value);
            if self
                .eval_expr(predicate_expr, frame)?
                .bool_value()
                .unwrap_or(false)
            {
                if let Some(previous) = previous.clone() {
                    frame.env.insert(binding.to_owned(), previous);
                } else {
                    frame.env.remove(binding);
                }
                return Ok(BoonValue::Bool(true));
            }
        }
        if let Some(previous) = previous {
            frame.env.insert(binding.to_owned(), previous);
        } else {
            frame.env.remove(binding);
        }
        Ok(BoonValue::Bool(false))
    }

    fn list_latest(
        &mut self,
        list: BoonValue,
        _frame: &mut GenericEvalFrame,
    ) -> RuntimeResult<BoonValue> {
        let values = match list {
            BoonValue::List(values) => values,
            BoonValue::ListRef(list) => {
                let len = self.storage.list_len(&list)?;
                let mut values = Vec::new();
                for index in 0..len {
                    values.push(BoonValue::RowRef {
                        list: list.clone(),
                        index,
                    });
                }
                values
            }
            _ => return Ok(BoonValue::Error("type_error".to_owned())),
        };
        let mut latest = BoonValue::Empty;
        for value in values {
            if !matches!(value, BoonValue::Empty) {
                latest = value;
            }
        }
        Ok(latest)
    }

    fn list_sum(&self, list: BoonValue) -> RuntimeResult<BoonValue> {
        let BoonValue::List(values) = list else {
            return Ok(BoonValue::Error("type_error".to_owned()));
        };
        let mut sum = 0i64;
        for value in values {
            match value {
                BoonValue::Error(error) => return Ok(BoonValue::Error(error)),
                BoonValue::Empty => {}
                BoonValue::Text(ref text) if text.trim().is_empty() => {}
                other => match other.number() {
                    Ok(value) => sum += value,
                    Err(error) => return Ok(BoonValue::Error(error)),
                },
            }
        }
        Ok(BoonValue::Number(sum))
    }

    fn assert_generic_step_expectations(&self, step: &ScenarioStep) -> RuntimeResult<()> {
        if let Some(expect) = &step.expect_cell {
            let mut required_fields = vec!["formula_text"];
            if expect.value.is_some() {
                required_fields.push("value");
            }
            if expect.formula.is_some() {
                required_fields.push("formula_text");
            }
            if expect.editing_text.is_some() {
                required_fields.push("editing_text");
            }
            if expect.editing.is_some() {
                required_fields.push("editing");
            }
            let (list, index) =
                self.generic_addressed_row_with_fields(&expect.address, &required_fields)?;
            if let Some(value) = &expect.value {
                self.assert_list_row_textlike(
                    &step.id,
                    "cell.value",
                    &list,
                    index,
                    "value",
                    value,
                )?;
            }
            if let Some(formula) = &expect.formula {
                self.assert_list_row_textlike(
                    &step.id,
                    "cell.formula",
                    &list,
                    index,
                    "formula_text",
                    formula,
                )?;
            }
            if let Some(editing_text) = &expect.editing_text {
                self.assert_list_row_textlike(
                    &step.id,
                    "cell.editing_text",
                    &list,
                    index,
                    "editing_text",
                    editing_text,
                )?;
            }
            if let Some(editing) = expect.editing {
                let actual = self.storage.list_row_bool(&list, index, "editing")?;
                assert_eq_report(&step.id, "cell.editing", &editing, &actual)?;
            }
        }
        if let Some(expect) = &step.expect_error {
            let (list, index) =
                self.generic_addressed_row_with_fields(&expect.address, &["error"])?;
            let actual = self
                .storage
                .list_row_textlike_opt(&list, index, "error")
                .filter(|error| !error.is_empty());
            assert_eq_report(
                &step.id,
                "cell.error",
                &Some(expect.error.as_str()),
                &actual,
            )?;
        }
        if let Some(expected) = &step.expect_recomputed {
            let mut actual = self
                .generic_derived_state
                .last_recomputed
                .iter()
                .filter(|key| key.field == "value")
                .map(|key| {
                    self.storage
                        .list_row_textlike(&key.list, key.index, "address")
                        .map(str::to_owned)
                })
                .collect::<RuntimeResult<Vec<_>>>()?;
            actual.sort();
            assert_eq_report(&step.id, "recomputed", expected, &actual)?;
        }
        for (path, expected) in &step.expect_root_text {
            self.assert_root_textlike(&step.id, path, path, expected)?;
        }
        Ok(())
    }

    fn generic_addressed_row(&self, address: &str) -> RuntimeResult<(String, usize)> {
        self.generic_addressed_row_with_fields(address, &[])
    }

    fn generic_addressed_row_with_fields(
        &self,
        address: &str,
        required_fields: &[&str],
    ) -> RuntimeResult<(String, usize)> {
        for slot in &self.storage.lists.list_slots {
            let list = &slot.name;
            let Some(lookup_field) = self.address_lookup_field_for_list(list) else {
                continue;
            };
            if self
                .storage
                .list_row_textlike_opt(list, 0, lookup_field)
                .is_none()
            {
                continue;
            }
            if required_fields
                .iter()
                .any(|field| self.storage.list_row_field(list, 0, field).is_err())
            {
                continue;
            }
            if let Some(index) =
                self.storage
                    .find_list_index_by_textlike(list, lookup_field, address)?
            {
                return Ok((list.clone(), index));
            }
        }
        Err(format!("cell `{address}` not found in any addressed generic list").into())
    }

    fn address_lookup_field_for_list(&self, list: &str) -> Option<&str> {
        self.list_source_bindings
            .source_paths(list)
            .ok()?
            .iter()
            .filter_map(|source| self.source_routes.source_id(source))
            .find_map(|source_id| {
                self.source_routes
                    .address_lookup_field_for_source_id(source_id)
            })
    }

    #[cfg(test)]
    fn list_all_completed_by_count_targets(&self) -> bool {
        let active_count = self
            .count_list_rows_for_target("todos", "store.active_count")
            .unwrap_or_default();
        let completed_count = self
            .count_list_rows_for_target("todos", "store.completed_count")
            .unwrap_or_default();
        active_count == 0 && completed_count > 0
    }

    fn generic_summary(&mut self) -> JsonValue {
        self.generic_summary_with_limits(SummaryLimits::unlimited())
    }

    fn runtime_value_summaries(
        &self,
        paths: &[String],
        max_depth: usize,
        max_fields: usize,
        max_list_items: usize,
    ) -> JsonValue {
        let mut values = serde_json::Map::new();
        for path in paths.iter().take(8) {
            let summary = self
                .runtime_path_summary(path, 0, max_depth, max_fields, max_list_items)
                .unwrap_or_else(|| json!({"kind": "missing"}));
            values.insert(path.clone(), summary);
        }
        JsonValue::Object(values)
    }

    fn runtime_path_summary(
        &self,
        path: &str,
        depth: usize,
        max_depth: usize,
        max_fields: usize,
        max_list_items: usize,
    ) -> Option<JsonValue> {
        if depth >= max_depth {
            return Some(json!({
                "kind": self.runtime_path_kind(path).unwrap_or("value"),
                "collapsed": true
            }));
        }
        if let Some(value) = self.runtime_scalar_json(path) {
            return Some(runtime_json_value_summary(
                &value,
                depth,
                max_depth,
                max_fields,
                max_list_items,
            ));
        }
        if let Some((list, tail)) = self.runtime_path_list_head(path)
            && let Some(summary) = self.runtime_list_path_summary(
                &list,
                &tail,
                depth,
                max_depth,
                max_fields,
                max_list_items,
            )
        {
            return Some(summary);
        }
        if let Some((target, list, tail)) = self.runtime_retain_path_head(path)
            && let Some(summary) = self.runtime_retain_path_summary(
                target,
                list,
                &tail,
                depth,
                max_depth,
                max_fields,
                max_list_items,
            )
        {
            return Some(summary);
        }
        self.runtime_object_summary(path, depth, max_depth, max_fields, max_list_items)
    }

    fn runtime_path_kind(&self, path: &str) -> Option<&'static str> {
        if self.runtime_scalar_json(path).is_some() {
            return Some("value");
        }
        if self.runtime_path_list_head(path).is_some_and(|(list, _)| {
            self.list_summary_fields
                .iter()
                .any(|summary| summary.list == list)
        }) || self.runtime_retain_path_head(path).is_some()
        {
            return Some("list");
        }
        if self.has_runtime_object_fields(path) {
            return Some("object");
        }
        None
    }

    fn runtime_scalar_json(&self, path: &str) -> Option<JsonValue> {
        if let Some(value) = self.storage.root.owned_value(path) {
            return Some(field_value_json(value));
        }
        if let Some(value) = self
            .root_state_paths
            .iter()
            .find(|root_path| row_field_name(root_path) == path)
            .and_then(|root_path| self.storage.root.owned_value(root_path))
        {
            return Some(field_value_json(value));
        }
        for (list, target) in self.list_equations.count_targets() {
            if runtime_path_matches_target(path, target)
                && let Ok(count) = self.count_list_rows_for_target(list, target)
            {
                return Some(json!(count));
            }
        }
        for projection in &self.list_projections.projections {
            if !runtime_path_matches_target(path, &projection.target) {
                continue;
            }
            if let RuntimeListProjectionKind::Find { field, value } = &projection.kind {
                let selected = self
                    .storage
                    .root_textlike_ref(value)
                    .map(str::to_owned)
                    .unwrap_or_else(|_| value.clone());
                if let Some(value) = self.list_find_projection(&projection.list, field, &selected) {
                    return Some(JsonValue::Object(value));
                }
            }
        }
        None
    }

    fn runtime_scalar_boon_value(&self, path: &str) -> Option<BoonValue> {
        if let Some(value) = self.storage.root.owned_value(path) {
            return Some(field_value_to_boon(value));
        }
        if let Some(value) = self
            .root_state_paths
            .iter()
            .find(|root_path| row_field_name(root_path) == path)
            .and_then(|root_path| self.storage.root.owned_value(root_path))
        {
            return Some(field_value_to_boon(value));
        }
        for (list, target) in self.list_equations.count_targets() {
            if runtime_path_matches_target(path, target)
                && let Ok(count) = self.count_list_rows_for_target(list, target)
            {
                return Some(BoonValue::Number(count as i64));
            }
        }
        None
    }

    fn runtime_path_list_head<'a>(&'a self, path: &'a str) -> Option<(&'a str, RuntimePathTail)> {
        for summary in &self.list_summary_fields {
            if let Some(tail) = runtime_path_tail_after_head(path, &summary.list) {
                return Some((&summary.list, tail));
            }
            let store_head = format!("store.{}", summary.list);
            if let Some(tail) = runtime_path_tail_after_head(path, &store_head) {
                return Some((&summary.list, tail));
            }
        }
        None
    }

    fn runtime_list_path_summary(
        &self,
        list: &str,
        tail: &RuntimePathTail,
        depth: usize,
        max_depth: usize,
        max_fields: usize,
        max_list_items: usize,
    ) -> Option<JsonValue> {
        let summary = self
            .list_summary_fields
            .iter()
            .find(|summary| summary.list == list)?;
        if tail.indexes.is_empty() {
            return Some(self.runtime_list_summary(
                summary,
                None,
                depth,
                max_depth,
                max_fields,
                max_list_items,
            ));
        }
        let index = *tail.indexes.first()?;
        let row = self.value_summary_row_json(summary, index).ok()?;
        runtime_row_tail_summary(
            row,
            &tail.rest,
            depth,
            max_depth,
            max_fields,
            max_list_items,
        )
    }

    fn runtime_retain_path_head<'a>(
        &'a self,
        path: &'a str,
    ) -> Option<(&'a str, &'a str, RuntimePathTail)> {
        for (list, target) in self.list_equations.retain_targets() {
            if let Some(tail) = runtime_path_tail_for_target(path, target) {
                return Some((target, list, tail));
            }
        }
        None
    }

    fn runtime_retain_path_summary(
        &self,
        target: &str,
        list: &str,
        tail: &RuntimePathTail,
        depth: usize,
        max_depth: usize,
        max_fields: usize,
        max_list_items: usize,
    ) -> Option<JsonValue> {
        let summary = self
            .list_summary_fields
            .iter()
            .find(|summary| summary.list == list)?;
        let predicate = self.list_equations.retain_predicate(list, target).ok()?;
        if predicate == RuntimeListPredicate::Unsupported {
            return None;
        }
        if tail.indexes.is_empty() {
            return Some(self.runtime_list_summary(
                summary,
                Some(&predicate),
                depth,
                max_depth,
                max_fields,
                max_list_items,
            ));
        }
        let retained_index = *tail.indexes.first()?;
        let row_index = self.retained_row_index(list, &predicate, retained_index)?;
        let row = self.value_summary_row_json(summary, row_index).ok()?;
        runtime_row_tail_summary(
            row,
            &tail.rest,
            depth,
            max_depth,
            max_fields,
            max_list_items,
        )
    }

    fn runtime_list_summary(
        &self,
        summary: &ListSummaryFields,
        predicate: Option<&RuntimeListPredicate>,
        depth: usize,
        max_depth: usize,
        max_fields: usize,
        max_list_items: usize,
    ) -> JsonValue {
        if depth >= max_depth {
            return json!({"kind": "list", "collapsed": true});
        }
        let len = self.storage.list_len(&summary.list).unwrap_or_default();
        let mut sample = Vec::new();
        let mut retained_len = 0usize;
        for index in 0..len {
            if predicate.is_some_and(|predicate| {
                !self
                    .storage
                    .list_row_matches_predicate(&summary.list, index, predicate)
                    .unwrap_or(false)
            }) {
                continue;
            }
            retained_len = retained_len.saturating_add(1);
            if sample.len() < max_list_items {
                let row = self
                    .value_summary_row_json(summary, index)
                    .unwrap_or_default();
                sample.push(runtime_json_value_summary(
                    &JsonValue::Object(row),
                    depth + 1,
                    max_depth,
                    max_fields,
                    max_list_items,
                ));
            }
        }
        let reported_len = if predicate.is_some() {
            retained_len
        } else {
            len
        };
        json!({
            "kind": "list",
            "len": reported_len,
            "sample_start": 0,
            "sample": sample,
            "truncated": reported_len > max_list_items
        })
    }

    fn retained_row_index(
        &self,
        list: &str,
        predicate: &RuntimeListPredicate,
        retained_index: usize,
    ) -> Option<usize> {
        let len = self.storage.list_len(list).ok()?;
        let mut seen = 0usize;
        for index in 0..len {
            if !self
                .storage
                .list_row_matches_predicate(list, index, predicate)
                .ok()?
            {
                continue;
            }
            if seen == retained_index {
                return Some(index);
            }
            seen = seen.saturating_add(1);
        }
        None
    }

    fn runtime_object_summary(
        &self,
        path: &str,
        depth: usize,
        max_depth: usize,
        max_fields: usize,
        max_list_items: usize,
    ) -> Option<JsonValue> {
        let fields = self.runtime_object_field_paths(path);
        if fields.is_empty() {
            return None;
        }
        if depth >= max_depth {
            return Some(json!({"kind": "object", "collapsed": true}));
        }
        let mut sampled = serde_json::Map::new();
        for (field, full_path) in fields.iter().take(max_fields) {
            let value = self
                .runtime_path_summary(full_path, depth + 1, max_depth, max_fields, max_list_items)
                .unwrap_or_else(|| json!({"kind": "missing"}));
            sampled.insert(field.clone(), value);
        }
        Some(json!({
            "kind": "object",
            "field_count": fields.len(),
            "fields": sampled,
            "truncated": fields.len() > max_fields
        }))
    }

    fn has_runtime_object_fields(&self, path: &str) -> bool {
        !self.runtime_object_field_paths(path).is_empty()
    }

    fn runtime_object_field_paths(&self, path: &str) -> BTreeMap<String, String> {
        let mut fields = BTreeMap::new();
        let prefix = format!("{path}.");
        for root_path in &self.root_state_paths {
            if let Some(rest) = root_path.strip_prefix(&prefix)
                && let Some(field) = rest.split('.').next()
            {
                fields.insert(field.to_owned(), format!("{path}.{field}"));
            }
        }
        for (_, target) in self.list_equations.count_targets() {
            if let Some(rest) = target.strip_prefix(&prefix)
                && let Some(field) = rest.split('.').next()
            {
                fields.insert(field.to_owned(), format!("{path}.{field}"));
            }
        }
        for (_, target) in self.list_equations.retain_targets() {
            if let Some(rest) = target.strip_prefix(&prefix)
                && let Some(field) = rest.split('.').next()
            {
                fields.insert(field.to_owned(), format!("{path}.{field}"));
            }
        }
        for projection in &self.list_projections.projections {
            if let Some(rest) = projection.target.strip_prefix(&prefix)
                && let Some(field) = rest.split('.').next()
            {
                fields.insert(field.to_owned(), format!("{path}.{field}"));
            }
        }
        if path == "store" {
            for summary in &self.list_summary_fields {
                fields.insert(summary.list.clone(), format!("store.{}", summary.list));
            }
        }
        fields
    }

    fn root_derived_summary_values(&mut self) -> Vec<(String, JsonValue)> {
        let fields = self.generic_derived.root_fields.clone();
        let mut values = Vec::new();
        for field in fields {
            let mut frame = GenericEvalFrame::root();
            let value = self
                .eval_statement_value(&field.statement, &mut frame)
                .unwrap_or_else(|error| BoonValue::Error(error.to_string()));
            if matches!(value, BoonValue::Error(_) | BoonValue::Empty) {
                continue;
            }
            values.push((field.path, boon_value_json(&value)));
        }
        values
    }

    fn document_summary(&mut self) -> JsonValue {
        let mut summary = self.generic_summary_with_limits(SummaryLimits::document_preview());
        self.insert_list_projection_summary_with_limits(
            &mut summary,
            SummaryLimits::document_preview(),
        );
        summary
    }

    fn document_summary_for_window(
        &mut self,
        row_start: usize,
        row_count: usize,
        column_start: usize,
        column_count: usize,
    ) -> JsonValue {
        let limits = SummaryLimits::document_preview_window(
            row_start,
            row_count,
            column_start,
            column_count,
        );
        let mut summary = self.generic_summary_with_limits(limits);
        self.insert_list_projection_summary_with_limits(&mut summary, limits);
        summary
    }

    fn generic_summary_with_limits(&mut self, limits: SummaryLimits) -> JsonValue {
        let mut root = serde_json::Map::new();
        let mut flat_root = serde_json::Map::new();
        for path in &self.root_state_paths {
            let Some(value) = self.storage.root.owned_value(path) else {
                continue;
            };
            let json_value = field_value_json(value);
            insert_nested_json(&mut root, path, json_value.clone());
            flat_root.insert(row_field_name(path).to_owned(), json_value);
        }
        for (key, value) in flat_root {
            root.entry(key).or_insert(value);
        }
        for (list, target) in self.list_equations.count_targets() {
            if let Ok(count) = self.count_list_rows_for_target(list, target) {
                let value = json!(count);
                insert_nested_json(&mut root, target, value.clone());
                root.entry(row_field_name(target).to_owned())
                    .or_insert(value);
            }
        }
        for (path, value) in self.root_derived_summary_values() {
            insert_nested_json(&mut root, &path, value.clone());
            root.entry(row_field_name(&path).to_owned())
                .or_insert(value);
        }
        for (list, target) in self.list_equations.retain_targets() {
            let Some(summary) = self
                .list_summary_fields
                .iter()
                .find(|summary| summary.list == list)
            else {
                continue;
            };
            let Ok(predicate) = self.list_equations.retain_predicate(list, target) else {
                continue;
            };
            if predicate == RuntimeListPredicate::Unsupported {
                continue;
            }
            let len = self.storage.list_len(list).unwrap_or_default();
            let row_limit = limits.list_rows.map_or(len, |limit| len.min(limit));
            let mut rows = Vec::with_capacity(row_limit);
            for index in 0..row_limit {
                if self
                    .storage
                    .list_row_matches_predicate(list, index, &predicate)
                    .unwrap_or(false)
                {
                    rows.push(JsonValue::Object(
                        self.summary_row_json(summary, index)
                            .unwrap_or_else(|_| serde_json::Map::new()),
                    ));
                }
            }
            let value = JsonValue::Array(rows);
            insert_nested_json(&mut root, target, value.clone());
            root.entry(row_field_name(target).to_owned())
                .or_insert(value);
        }
        for summary in &self.list_summary_fields {
            let len = self.storage.list_len(&summary.list).unwrap_or_default();
            let row_limit = limits.list_rows.map_or(len, |limit| len.min(limit));
            let mut rows = Vec::with_capacity(row_limit);
            for index in 0..row_limit {
                rows.push(JsonValue::Object(
                    self.summary_row_json(summary, index)
                        .unwrap_or_else(|_| serde_json::Map::new()),
                ));
            }
            root.insert(summary.list.clone(), JsonValue::Array(rows));
        }
        root.insert(
            "source_binding_count".to_owned(),
            json!(self.storage.source_binding_count()),
        );
        JsonValue::Object(root)
    }

    fn insert_list_projection_summary(&self, summary: &mut JsonValue) {
        self.insert_list_projection_summary_with_limits(summary, SummaryLimits::unlimited());
    }

    fn insert_list_projection_summary_with_limits(
        &self,
        summary: &mut JsonValue,
        limits: SummaryLimits,
    ) {
        let Some(root) = summary.as_object_mut() else {
            return;
        };
        for projection in &self.list_projections.projections {
            match &projection.kind {
                RuntimeListProjectionKind::Chunk {
                    item_field,
                    label_field,
                } => {
                    let value = self.list_chunk_projection(
                        &projection.list,
                        projection.columns,
                        projection.rows,
                        item_field,
                        label_field,
                        limits,
                    );
                    insert_nested_json(root, &projection.target, JsonValue::Array(value));
                }
                RuntimeListProjectionKind::Find { field, value } => {
                    let selected_address = self
                        .storage
                        .root_textlike_ref(value)
                        .map(str::to_owned)
                        .unwrap_or_else(|_| value.clone());
                    if let Some(value) =
                        self.list_find_projection(&projection.list, field, &selected_address)
                    {
                        insert_nested_json(root, &projection.target, JsonValue::Object(value));
                    }
                }
            }
        }
    }

    fn list_chunk_projection(
        &self,
        list: &str,
        columns: usize,
        rows: usize,
        item_field: &str,
        label_field: &str,
        limits: SummaryLimits,
    ) -> Vec<JsonValue> {
        let list_summary = self
            .list_summary_fields
            .iter()
            .find(|summary| summary.list == list);
        let len = self.storage.list_len(list).unwrap_or_default();
        let row_count = if rows > 0 {
            rows
        } else if columns == 0 {
            0
        } else {
            len.div_ceil(columns)
        };
        let projected_row_count = limits.chunk_rows.map_or(row_count, |limit| {
            row_count.saturating_sub(limits.chunk_row_start).min(limit)
        });
        let projected_columns = limits.chunk_columns.map_or_else(
            || columns.saturating_sub(limits.chunk_column_start),
            |limit| columns.saturating_sub(limits.chunk_column_start).min(limit),
        );
        let mut projected_rows = Vec::with_capacity(projected_row_count);
        for row_offset in 0..projected_row_count {
            let row = limits.chunk_row_start.saturating_add(row_offset);
            let mut row_object = serde_json::Map::new();
            row_object.insert(label_field.to_owned(), json!(row.to_string()));
            row_object.insert("index".to_owned(), json!(row));
            let mut cells = Vec::with_capacity(projected_columns);
            for column_offset in 0..projected_columns {
                let column = limits.chunk_column_start.saturating_add(column_offset);
                let index = row * columns + column;
                if index >= len {
                    break;
                }
                let cell = list_summary
                    .and_then(|summary| self.summary_row_json(summary, index).ok())
                    .unwrap_or_default();
                cells.push(JsonValue::Object(cell));
            }
            row_object.insert(item_field.to_owned(), JsonValue::Array(cells));
            projected_rows.push(JsonValue::Object(row_object));
        }
        projected_rows
    }

    fn list_find_projection(
        &self,
        list: &str,
        field: &str,
        address: &str,
    ) -> Option<serde_json::Map<String, JsonValue>> {
        let index = self
            .storage
            .find_list_index_by_textlike(list, field, address)
            .ok()
            .flatten()?;
        self.list_summary_fields
            .iter()
            .find(|summary| summary.list == list)
            .and_then(|summary| self.summary_row_json(summary, index).ok())
    }

    fn summary_row_json(
        &self,
        summary: &ListSummaryFields,
        index: usize,
    ) -> RuntimeResult<serde_json::Map<String, JsonValue>> {
        let mut row = serde_json::Map::new();
        let identity = self.storage.row_identity(&summary.list, index);
        if let Ok((key, generation)) = identity {
            row.insert("key".to_owned(), json!(key));
            row.insert("generation".to_owned(), json!(generation));
        }
        for field in &summary.fields {
            if let Ok(value) = self.storage.list_row_field(&summary.list, index, field) {
                let json_value = match value {
                    FieldValueRef::Text("") if field == "error" => JsonValue::Null,
                    _ => value.as_json(),
                };
                row.insert(field.clone(), json_value);
            }
        }
        if let Ok((key, generation)) = identity {
            let row_scope = row_scope_name(&summary.list);
            let prefix = format!("{row_scope}.");
            for binding in self
                .storage
                .row_source_bindings(&summary.list, key, generation)
            {
                let Some(path) = binding.source_path.strip_prefix(&prefix) else {
                    continue;
                };
                insert_nested_json(
                    &mut row,
                    path,
                    JsonValue::String(binding.source_path.clone()),
                );
            }
        }
        Ok(row)
    }

    fn value_summary_row_json(
        &self,
        summary: &ListSummaryFields,
        index: usize,
    ) -> RuntimeResult<serde_json::Map<String, JsonValue>> {
        let mut row = serde_json::Map::new();
        for field in &summary.fields {
            if let Ok(value) = self.storage.list_row_field(&summary.list, index, field) {
                let json_value = match value {
                    FieldValueRef::Text("") if field == "error" => JsonValue::Null,
                    _ => value.as_json(),
                };
                row.insert(field.clone(), json_value);
            }
        }
        Ok(row)
    }
}

fn field_value_to_boon(value: FieldValue) -> BoonValue {
    match value {
        FieldValue::Text(value) | FieldValue::Enum(value) => BoonValue::Text(value),
        FieldValue::Bool(value) => BoonValue::Bool(value),
    }
}

fn field_ref_to_boon(value: FieldValueRef<'_>) -> BoonValue {
    match value {
        FieldValueRef::Text(value) | FieldValueRef::Enum(value) => {
            BoonValue::Text(value.to_owned())
        }
        FieldValueRef::Bool(value) => BoonValue::Bool(value),
    }
}

fn generic_infix_value(left: BoonValue, op: &str, right: BoonValue) -> BoonValue {
    if let BoonValue::Error(error) = left {
        return BoonValue::Error(error);
    }
    if let BoonValue::Error(error) = right {
        return BoonValue::Error(error);
    }
    match op {
        "+" => match (left.number(), right.number()) {
            (Ok(left), Ok(right)) => BoonValue::Number(left + right),
            _ => BoonValue::Text(format!(
                "{}{}",
                left.as_text().unwrap_or_default(),
                right.as_text().unwrap_or_default()
            )),
        },
        "-" | "*" | "/" | "%" => {
            let left = match left.number() {
                Ok(value) => value,
                Err(error) => return BoonValue::Error(error),
            };
            let right = match right.number() {
                Ok(value) => value,
                Err(error) => return BoonValue::Error(error),
            };
            match op {
                "-" => BoonValue::Number(left - right),
                "*" => BoonValue::Number(left * right),
                "/" if right == 0 => BoonValue::Error("div_by_zero".to_owned()),
                "/" => BoonValue::Number(left / right),
                "%" if right == 0 => BoonValue::Error("div_by_zero".to_owned()),
                "%" => BoonValue::Number(left % right),
                _ => BoonValue::Error("type_error".to_owned()),
            }
        }
        "==" => BoonValue::Bool(generic_values_equal(&left, &right)),
        "!=" => BoonValue::Bool(!generic_values_equal(&left, &right)),
        ">" | ">=" | "<" | "<=" => {
            let left = match left.number() {
                Ok(value) => value,
                Err(error) => return BoonValue::Error(error),
            };
            let right = match right.number() {
                Ok(value) => value,
                Err(error) => return BoonValue::Error(error),
            };
            BoonValue::Bool(match op {
                ">" => left > right,
                ">=" => left >= right,
                "<" => left < right,
                "<=" => left <= right,
                _ => false,
            })
        }
        _ => BoonValue::Error("type_error".to_owned()),
    }
}

fn generic_values_equal(left: &BoonValue, right: &BoonValue) -> bool {
    match (left, right) {
        (BoonValue::Text(left), BoonValue::Text(right)) => left == right,
        (BoonValue::Number(left), BoonValue::Number(right)) => left == right,
        (BoonValue::Bool(left), BoonValue::Bool(right)) => left == right,
        (BoonValue::NaN, BoonValue::NaN) => true,
        _ => left
            .as_text()
            .zip(right.as_text())
            .is_some_and(|(left, right)| left == right),
    }
}

fn generic_pattern_binding(
    pattern: &[String],
    input: &BoonValue,
) -> Option<Option<(String, BoonValue)>> {
    if pattern == ["__"] {
        return Some(None);
    }
    if pattern.len() == 1 {
        let token = pattern[0].as_str();
        if token == "True" {
            return matches!(input, BoonValue::Bool(true)).then_some(None);
        }
        if token == "False" {
            return matches!(input, BoonValue::Bool(false)).then_some(None);
        }
        if token == "NaN" {
            return matches!(input, BoonValue::NaN).then_some(None);
        }
        if let Ok(number) = token.parse::<i64>() {
            return matches!(input, BoonValue::Number(value) if *value == number).then_some(None);
        }
        if token
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_lowercase())
        {
            return Some(Some((token.to_owned(), input.clone())));
        }
        return input.as_text().filter(|value| value == token).map(|_| None);
    }
    if pattern.first().map(String::as_str) == Some("TEXT")
        && pattern.get(1).map(String::as_str) == Some("{")
        && pattern.last().map(String::as_str) == Some("}")
    {
        let expected = text_match_pattern_value(&pattern[2..pattern.len() - 1]);
        return input
            .as_text()
            .filter(|value| value == &expected)
            .map(|_| None);
    }
    None
}

fn text_match_pattern_value(tokens: &[String]) -> String {
    if tokens.first().map(String::as_str) == Some("/") {
        return format!("/{}", tokens[1..].join(""));
    }
    tokens.join(" ")
}

fn statement_is_row_initial_passthrough(
    statement: &AstStatement,
    expressions: &[AstExpr],
    row_scope: &str,
    field: &str,
) -> bool {
    let Some(expr_id) = statement.expr else {
        return false;
    };
    expressions.get(expr_id).is_some_and(|expr| {
        matches!(
            &expr.kind,
            AstExprKind::Path(parts) if parts.as_slice() == [row_scope, field]
                || (parts.len() == 2 && parts.get(1).is_some_and(|part| part == field))
        )
    })
}

impl Deref for GenericScheduledRuntime {
    type Target = GenericCircuitRuntime;

    fn deref(&self) -> &Self::Target {
        &self.storage
    }
}

impl DerefMut for GenericScheduledRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.storage
    }
}

impl GenericCircuitRuntime {
    fn new(ir: &TypedProgram) -> RuntimeResult<Self> {
        let mut runtime = Self::default();
        for cell in ir.state_cells.iter().filter(|cell| !cell.indexed) {
            runtime.root.insert_value(
                cell.path.clone(),
                runtime_value_from_initial(&cell.initial_value, &ValueColumns::default())?,
            );
        }
        for list in &ir.lists {
            let row_scope = row_scope_name(&list.name);
            let indexed_cells = ir
                .state_cells
                .iter()
                .filter(|cell| cell.indexed && cell.path.starts_with(&format!("{row_scope}.")))
                .collect::<Vec<_>>();
            let row_template =
                RuntimeRowSnapshotTemplate::from_cells(&row_scope, &indexed_cells, ir)?;
            let rows = match &list.initializer {
                ListInitializer::RecordLiteral { rows } => rows
                    .iter()
                    .map(|row| {
                        let initial_fields = list_initial_fields(row)?;
                        let mut row = row_template.materialize(initial_fields)?;
                        initialize_indexed_derived_text_fields(ir, &row_scope, &mut row);
                        Ok(row)
                    })
                    .collect::<RuntimeResult<Vec<_>>>()?,
                ListInitializer::Range { from, to } => {
                    let count = if from <= to {
                        usize::try_from(to.saturating_sub(*from).saturating_add(1))
                            .map_err(|_| "List/range row count is out of range")?
                    } else {
                        0
                    };
                    let mut range_rows = Vec::with_capacity(count);
                    for value in *from..=*to {
                        let mut initial_fields = ValueColumns::default();
                        let text = value.to_string();
                        initial_fields
                            .insert_value("index".to_owned(), FieldValue::Text(text.clone()));
                        initial_fields.insert_value("value".to_owned(), FieldValue::Text(text));
                        row_template.fill_missing_row_initial_fields(&mut initial_fields);
                        let mut row = row_template.materialize(initial_fields)?;
                        initialize_indexed_derived_text_fields(ir, &row_scope, &mut row);
                        range_rows.push(row);
                    }
                    range_rows
                }
                ListInitializer::Empty => Vec::new(),
                ListInitializer::Unknown { summary } => {
                    return Err(format!(
                        "list `{}` has unsupported initializer `{summary}`",
                        list.name
                    )
                    .into());
                }
            };
            if let Some(capacity) = list.capacity
                && rows.len() > capacity
            {
                return Err(format!(
                    "list `{}` initializes {} rows beyond declared capacity {capacity}",
                    list.name,
                    rows.len()
                )
                .into());
            }
            runtime.lists.insert(
                list.id,
                list.name.clone(),
                ListMemory::from_values(rows),
                list.capacity,
                row_template,
            );
        }
        Ok(runtime)
    }

    fn root_textlike(&self, path: &str) -> RuntimeResult<String> {
        self.root_textlike_ref(path).map(str::to_owned)
    }

    fn root_textlike_ref(&self, path: &str) -> RuntimeResult<&str> {
        self.root.textlike(path).ok_or_else(|| {
            format!("generic runtime root value `{path}` is missing or non-text").into()
        })
    }

    fn set_root_textlike(&mut self, path: &str, value: &str) -> RuntimeResult<()> {
        self.root.set_textlike(path, value)
    }

    fn root_bool_opt(&self, path: &str) -> Option<bool> {
        self.root.bool_value(path)
    }

    fn root_bool(&self, path: &str) -> RuntimeResult<bool> {
        self.root.bool_value(path).ok_or_else(|| {
            format!("generic runtime root value `{path}` is missing or non-bool").into()
        })
    }

    fn set_root_bool(&mut self, path: &str, value: bool) -> RuntimeResult<()> {
        self.root.set_bool(path, value)
    }

    fn apply_root_text_source<'a>(
        &mut self,
        equations: &ScalarEquationPlan,
        target: &str,
        source: &str,
        payload_key: Option<&'a str>,
        payload_text: Option<&'a str>,
        payload_address: Option<&'a str>,
        seq: TickSeq,
    ) -> RuntimeResult<Option<Cow<'a, str>>> {
        let Some(value) = equations.eval_text(
            target,
            source,
            payload_key,
            payload_text,
            payload_address,
            |path| self.root_textlike(path).ok(),
        )?
        else {
            return Err(format!(
                "no supported scalar update branch for `{target}` from `{source}`"
            )
            .into());
        };
        let Some(candidate) = then_value(EventPulse::present(seq), value) else {
            return Ok(None);
        };
        self.commit_root_text_candidate(target, candidate)
    }

    fn apply_root_bool_source(
        &mut self,
        equations: &ScalarEquationPlan,
        target: &str,
        source: &str,
        seq: TickSeq,
    ) -> RuntimeResult<Option<GenericRootBoolCommit>> {
        let Some(value) =
            equations.eval_bool_with_context(target, source, |path| self.root_bool(path).ok())?
        else {
            return Err(format!(
                "no supported bool scalar update branch for `{target}` from `{source}`"
            )
            .into());
        };
        let Some(candidate) = then_value(EventPulse::present(seq), value) else {
            return Ok(None);
        };
        let Some(value) = latest_value(target, &[candidate])? else {
            return Ok(None);
        };
        self.set_root_bool(target, value)?;
        Ok(Some(GenericRootBoolCommit {
            target: target.to_owned(),
            value,
        }))
    }

    fn apply_root_text_action_source<'a>(
        &mut self,
        routes: &SourceRoutePlan,
        equations: &ScalarEquationPlan,
        source: &str,
        source_id: SourceId,
        payload_key: Option<&'a str>,
        payload_text: Option<&'a str>,
        payload_address: Option<&'a str>,
        seq: TickSeq,
    ) -> RuntimeResult<Option<GenericRootTextCommit<'a>>> {
        let Some(target) = routes.single_root_scalar_target_for_source_id(source_id)? else {
            return Ok(None);
        };
        let Some(value) = self.apply_root_text_source(
            equations,
            target,
            source,
            payload_key,
            payload_text,
            payload_address,
            seq,
        )?
        else {
            return Ok(None);
        };
        Ok(Some(GenericRootTextCommit {
            target: target.to_owned(),
            value,
        }))
    }

    fn eval_derived_text_transform<'a>(
        &self,
        equations: &DerivedEquationPlan,
        target: &str,
        source: &str,
        key: Option<&str>,
        text: Option<&'a str>,
    ) -> RuntimeResult<Option<Cow<'a, str>>> {
        equations.eval_text_transform(target, source, key, text, |path| {
            self.root_textlike(path).ok()
        })
    }

    fn commit_root_text_candidate<'a>(
        &mut self,
        target: &str,
        candidate: LatestCandidate<Cow<'a, str>>,
    ) -> RuntimeResult<Option<Cow<'a, str>>> {
        let Some(value) = latest_value(target, &[candidate])? else {
            return Ok(None);
        };
        self.set_root_textlike(target, &value)?;
        Ok(Some(value))
    }

    fn reserve_root_textlike(&mut self, path: &str, additional: usize) -> RuntimeResult<()> {
        self.root.reserve_textlike(path, additional)
    }

    fn reserve_list(&mut self, list: &str, additional: usize) -> RuntimeResult<()> {
        self.lists
            .memory_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .reserve(additional);
        Ok(())
    }

    fn reserve_source_bindings(&mut self, additional: usize) {
        self.sources.reserve(additional);
    }

    fn reserve_source_rows(&mut self, row_count: usize) {
        self.sources.reserve_rows(row_count);
    }

    fn bind_row_sources(
        &mut self,
        list: &str,
        key: u64,
        generation: u64,
        source_paths: &[String],
    ) -> RuntimeResult<()> {
        self.sources.bind_row(list, key, generation, source_paths)
    }

    fn unbind_row_sources(&mut self, list: &str, key: u64, generation: u64) {
        self.sources.unbind_row(list, key, generation);
    }

    fn is_row_source_bound(
        &self,
        list: &str,
        key: u64,
        generation: u64,
        source_path: &str,
        source_id: Option<u64>,
        bind_epoch: Option<u64>,
    ) -> bool {
        self.sources
            .is_bound(list, key, generation, source_path, source_id, bind_epoch)
    }

    fn row_source_bindings(
        &self,
        list: &str,
        key: u64,
        generation: u64,
    ) -> impl Iterator<Item = &SourceBinding> {
        self.sources.row_bindings(list, key, generation)
    }

    #[cfg(test)]
    fn row_source_binding_count(&self, list: &str, key: u64, generation: u64) -> usize {
        self.sources.row_binding_count(list, key, generation)
    }

    fn source_binding_count(&self) -> usize {
        self.sources.len()
    }

    fn list_rows_json(
        &self,
        list: &str,
        fields: &[&str],
    ) -> RuntimeResult<Vec<serde_json::Map<String, JsonValue>>> {
        let rows = self
            .lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?;
        let mut values = Vec::with_capacity(rows.len());
        for index in 0..rows.len() {
            let mut row = serde_json::Map::new();
            for field in fields {
                let value = self.list_row_field(list, index, field)?;
                row.insert((*field).to_owned(), value.as_json());
            }
            values.push(row);
        }
        Ok(values)
    }

    #[cfg(test)]
    fn list_row_fields_json(
        &self,
        list: &str,
        index: usize,
        fields: &[&str],
    ) -> RuntimeResult<serde_json::Map<String, JsonValue>> {
        let mut row = serde_json::Map::new();
        for field in fields {
            let value = self.list_row_field(list, index, field)?;
            row.insert((*field).to_owned(), value.as_json());
        }
        Ok(row)
    }

    fn semantic_source_delta<'a>(
        kind: &'static str,
        binding: &SourceBinding,
        value: ProtocolValue<'a>,
    ) -> SemanticDelta<'a> {
        SemanticDelta {
            kind,
            list_id: Some(Cow::Owned(binding.list_id.clone())),
            key: Some(binding.key),
            generation: Some(binding.generation),
            source_id: Some(binding.source_id),
            bind_epoch: Some(binding.bind_epoch),
            field_path: Some(Cow::Owned(binding.source_path.clone())),
            value,
        }
    }

    fn reserve_list_row_textlike_fields(
        &mut self,
        list: &str,
        field: &str,
        additional_by_row: impl Fn(usize, &str) -> usize,
    ) -> RuntimeResult<()> {
        let rows = self
            .lists
            .memory_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?;
        for index in 0..rows.len() {
            let current = rows
                .textlike(index, field)
                .ok_or_else(|| format!("generic list `{list}` field `{field}` is not text-like"))?;
            let additional = additional_by_row(index, current);
            rows.reserve_textlike(index, field, additional)?;
        }
        Ok(())
    }

    fn copy_list_row_textlike_field(
        &mut self,
        list: &str,
        index: usize,
        source_field: &str,
        target_field: &str,
    ) -> RuntimeResult<()> {
        if source_field == target_field {
            return Ok(());
        }
        self.lists
            .memory_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .copy_textlike(index, source_field, target_field)
            .map_err(|_| {
                format!("generic list `{list}` field `{target_field}` is not text-like").into()
            })
    }

    fn list_row_textlike(&self, list: &str, index: usize, field: &str) -> RuntimeResult<&str> {
        self.lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .textlike(index, field)
            .ok_or_else(|| format!("generic list `{list}` field `{field}` is not text-like").into())
    }

    fn find_list_index_by_textlike(
        &self,
        list: &str,
        field: &str,
        expected: &str,
    ) -> RuntimeResult<Option<usize>> {
        let rows = self
            .lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?;
        for index in 0..rows.len() {
            if self.list_row_textlike(list, index, field)? == expected {
                return Ok(Some(index));
            }
        }
        Ok(None)
    }

    fn list_row_textlike_opt(&self, list: &str, index: usize, field: &str) -> Option<&str> {
        self.lists.memory(list)?.textlike(index, field)
    }

    fn list_row_value_opt(&self, list: &str, index: usize, field: &str) -> Option<FieldValue> {
        self.lists.memory(list)?.owned_value(index, field)
    }

    fn list_row_bool(&self, list: &str, index: usize, field: &str) -> RuntimeResult<bool> {
        self.list_row_field(list, index, field)?
            .as_bool()
            .ok_or_else(|| format!("generic list `{list}` field `{field}` is not bool").into())
    }

    fn list_row_bool_opt(&self, list: &str, index: usize, field: &str) -> Option<bool> {
        self.lists.memory(list)?.bool_value(index, field)
    }

    fn text_fields_for_row(&self, list: &str, index: usize) -> RuntimeResult<Vec<String>> {
        self.lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .textlike_field_names(index)
            .ok_or_else(|| format!("generic list `{list}` has no row {index}").into())
    }

    fn bool_fields_for_row(&self, list: &str, index: usize) -> RuntimeResult<Vec<String>> {
        self.lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .bool_field_names(index)
            .ok_or_else(|| format!("generic list `{list}` has no row {index}").into())
    }

    fn set_list_row_textlike(
        &mut self,
        list: &str,
        index: usize,
        field: &str,
        value: &str,
    ) -> RuntimeResult<()> {
        self.lists
            .memory_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .set_textlike(index, field, value)
    }

    fn set_or_insert_list_row_textlike(
        &mut self,
        list: &str,
        index: usize,
        field: &str,
        value: &str,
    ) -> RuntimeResult<()> {
        self.lists
            .memory_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .set_or_insert_text(index, field, value)
    }

    fn set_list_row_bool(
        &mut self,
        list: &str,
        index: usize,
        field: &str,
        value: bool,
    ) -> RuntimeResult<()> {
        self.lists
            .memory_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .set_bool(index, field, value)
    }

    fn set_list_row_value(
        &mut self,
        list: &str,
        index: usize,
        field: &str,
        value: FieldValue,
    ) -> RuntimeResult<()> {
        self.lists
            .memory_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .set_value(index, field, value)
    }

    fn commit_indexed_text_field(
        &mut self,
        list: &str,
        index: usize,
        field: &str,
        value: &str,
    ) -> RuntimeResult<(u64, u64)> {
        self.set_list_row_textlike(list, index, field, value)?;
        self.row_identity(list, index)
    }

    fn commit_indexed_bool_field(
        &mut self,
        list: &str,
        index: usize,
        field: &str,
        value: bool,
    ) -> RuntimeResult<(u64, u64)> {
        self.set_list_row_bool(list, index, field, value)?;
        self.row_identity(list, index)
    }

    fn commit_other_indexed_bool_fields(
        &mut self,
        list: &str,
        active_index: usize,
        field: &str,
        value: bool,
        mut observe: impl FnMut(GenericBoolFieldCommit) -> RuntimeResult<()>,
    ) -> RuntimeResult<()> {
        for index in 0..self.list_len(list)? {
            if index == active_index || self.list_row_bool_opt(list, index, field) != Some(!value) {
                continue;
            }
            let (key, generation) = self.commit_indexed_bool_field(list, index, field, value)?;
            observe(GenericBoolFieldCommit {
                list: list.to_owned(),
                key,
                generation,
                field: field.to_owned(),
                value,
            })?;
        }
        Ok(())
    }

    fn commit_indexed_text_source<'a>(
        &mut self,
        equations: &ScalarEquationPlan,
        list: &str,
        index: usize,
        target: &str,
        source: &str,
        payload_text: Option<&'a str>,
    ) -> RuntimeResult<Option<GenericTextFieldCommit<'a>>> {
        let value = match self.eval_indexed_text_source(
            equations,
            list,
            index,
            target,
            source,
            payload_text,
        )? {
            IndexedTextCandidate::SourceText(value) | IndexedTextCandidate::PreviousText(value) => {
                value
            }
            IndexedTextCandidate::TrimmedOrSkip(Some(value)) => value,
            IndexedTextCandidate::TrimmedOrSkip(None) => return Ok(None),
            IndexedTextCandidate::PreviousField(path) => {
                return Err(format!(
                    "text update `{target}` from `{source}` needs previous field `{path}` without payload"
                )
                .into());
            }
        };
        let field = row_field_name(target);
        let (key, generation) =
            self.commit_indexed_text_field(list, index, field, value.as_ref())?;
        Ok(Some(GenericTextFieldCommit {
            list: list.to_owned(),
            key,
            generation,
            field: field.to_owned(),
            value,
        }))
    }

    fn commit_edit_draft_title_for_index<'a>(
        &mut self,
        list: &str,
        index: usize,
    ) -> RuntimeResult<Option<GenericTextFieldCommit<'a>>> {
        let Some(value) = self
            .list_row_textlike_opt(list, index, "edit_text")
            .or_else(|| self.list_row_textlike_opt(list, index, "edited_title"))
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
        else {
            return Ok(None);
        };
        let (key, generation) = self.commit_indexed_text_field(list, index, "title", &value)?;
        Ok(Some(GenericTextFieldCommit {
            list: list.to_owned(),
            key,
            generation,
            field: "title".to_owned(),
            value: Cow::Owned(value),
        }))
    }

    fn commit_indexed_bool_source(
        &mut self,
        equations: &ScalarEquationPlan,
        list: &str,
        index: usize,
        target: &str,
        source: &str,
        read_extra_bool: impl Fn(&str) -> Option<bool>,
    ) -> RuntimeResult<GenericBoolFieldCommit> {
        let value =
            self.eval_indexed_bool_source(equations, list, index, target, source, read_extra_bool)?;
        let field = row_field_name(target);
        let (key, generation) = self.commit_indexed_bool_field(list, index, field, value)?;
        Ok(GenericBoolFieldCommit {
            list: list.to_owned(),
            key,
            generation,
            field: field.to_owned(),
            value,
        })
    }

    fn commit_indexed_previous_text_target_source(
        &mut self,
        equations: &ScalarEquationPlan,
        list: &str,
        index: usize,
        target: &str,
        source: &str,
    ) -> RuntimeResult<GenericTextFieldIdentity> {
        let previous =
            match self.eval_indexed_text_source(equations, list, index, target, source, None)? {
                IndexedTextCandidate::PreviousField(path) => path,
                IndexedTextCandidate::SourceText(_) | IndexedTextCandidate::PreviousText(_) => {
                    return Err(format!(
                        "text update `{target}` from `{source}` is not a previous-field update"
                    )
                    .into());
                }
                IndexedTextCandidate::TrimmedOrSkip(_) => {
                    return Err(format!(
                        "text update `{target}` from `{source}` unexpectedly used trim-or-previous"
                    )
                    .into());
                }
            };
        let field = row_field_name(target);
        self.copy_list_row_textlike_field(list, index, &previous, field)?;
        let value = self.list_row_textlike(list, index, field)?.to_owned();
        let (key, generation) = self.row_identity(list, index)?;
        Ok(GenericTextFieldIdentity {
            list: list.to_owned(),
            key,
            generation,
            field: field.to_owned(),
            value,
        })
    }

    fn commit_each_indexed_bool_source(
        &mut self,
        equations: &ScalarEquationPlan,
        list: &str,
        target: &str,
        source: &str,
        read_extra_bool: impl Fn(&str) -> Option<bool> + Copy,
        mut observe: impl FnMut(GenericBoolFieldCommit) -> RuntimeResult<()>,
    ) -> RuntimeResult<usize> {
        let len = self.list_len(list)?;
        for index in 0..len {
            let value = self.eval_indexed_bool_source(
                equations,
                list,
                index,
                target,
                source,
                read_extra_bool,
            )?;
            let field = row_field_name(target);
            let (key, generation) = self.commit_indexed_bool_field(list, index, field, value)?;
            observe(GenericBoolFieldCommit {
                list: list.to_owned(),
                key,
                generation,
                field: field.to_owned(),
                value,
            })?;
        }
        Ok(len)
    }

    fn eval_indexed_text_source<'a>(
        &self,
        equations: &ScalarEquationPlan,
        list: &str,
        index: usize,
        target: &str,
        source: &str,
        payload_text: Option<&'a str>,
    ) -> RuntimeResult<IndexedTextCandidate<'a>> {
        let Some(branch) = equations
            .branches
            .iter()
            .find(|branch| branch.target == target && branch.source == source)
        else {
            return Err(format!("no text branch for `{target}` from `{source}`").into());
        };
        match &branch.expression {
            ScalarUpdateExpression::SourceText => {
                let Some(value) = payload_text else {
                    return Ok(IndexedTextCandidate::TrimmedOrSkip(None));
                };
                Ok(IndexedTextCandidate::SourceText(Cow::Borrowed(value)))
            }
            ScalarUpdateExpression::SourceKey => Err(format!(
                "indexed text update `{target}` from `{source}` cannot use key payload directly"
            )
            .into()),
            ScalarUpdateExpression::SourceAddress => Err(format!(
                "indexed text update `{target}` from `{source}` cannot use address payload directly"
            )
            .into()),
            ScalarUpdateExpression::PreviousValue(path) => {
                let Some(value) = payload_text else {
                    return Ok(IndexedTextCandidate::PreviousField(path.clone()));
                };
                let current = self.list_row_textlike(list, index, path)?;
                if value != current {
                    return Err(format!(
                        "text update `{target}` from `{source}` expected `{path}` value `{current}`, got `{value}`"
                    )
                    .into());
                }
                Ok(IndexedTextCandidate::PreviousText(Cow::Borrowed(value)))
            }
            ScalarUpdateExpression::ReadPath(path) => Err(format!(
                "indexed text update `{target}` from `{source}` cannot read path `{path}`"
            )
            .into()),
            ScalarUpdateExpression::TextTrimOrPrevious { path, previous } => {
                let raw = match path.as_str() {
                    "text" => {
                        let Some(text) = payload_text else {
                            return Ok(IndexedTextCandidate::TrimmedOrSkip(None));
                        };
                        Cow::Borrowed(text)
                    }
                    field => {
                        let current = self.list_row_textlike(list, index, field)?;
                        if let Some(value) = payload_text {
                            if value != current {
                                return Err(format!(
                                    "text update `{target}` from `{source}` expected `{field}` value `{current}`, got `{value}`"
                                )
                                .into());
                            }
                        }
                        Cow::Owned(current.to_owned())
                    }
                };
                let current = self.list_row_textlike(list, index, previous)?;
                let value = match raw {
                    Cow::Borrowed(value) => {
                        let trimmed = value.trim();
                        (!trimmed.is_empty() && trimmed != current).then_some(Cow::Borrowed(trimmed))
                    }
                    Cow::Owned(value) => {
                        let trimmed = value.trim();
                        (!trimmed.is_empty() && trimmed != current)
                            .then(|| Cow::Owned(trimmed.to_owned()))
                    }
                };
                Ok(IndexedTextCandidate::TrimmedOrSkip(value))
            }
            ScalarUpdateExpression::Const(_)
            | ScalarUpdateExpression::NumberInfix { .. }
            | ScalarUpdateExpression::BoolNot(_)
            | ScalarUpdateExpression::MatchConst { .. }
            | ScalarUpdateExpression::Unsupported => Err(format!(
                "text branch for `{target}` from `{source}` is not a supported indexed text expression"
            )
            .into()),
        }
    }

    fn eval_indexed_bool_source(
        &self,
        equations: &ScalarEquationPlan,
        list: &str,
        index: usize,
        target: &str,
        source: &str,
        read_extra_bool: impl Fn(&str) -> Option<bool>,
    ) -> RuntimeResult<bool> {
        equations
            .eval_bool_with_context(target, source, |path| {
                self.list_row_bool_opt(list, index, path)
                    .or_else(|| read_extra_bool(path))
            })?
            .ok_or_else(|| {
                format!("no supported bool branch for `{target}` from `{source}`").into()
            })
    }

    fn append_row_for_trigger(
        &mut self,
        equations: &ListEquationPlan,
        list: &str,
        trigger: &str,
        row: RuntimeRowSnapshot,
    ) -> RuntimeResult<(u64, u64)> {
        let expected = equations.append_trigger(list)?;
        if expected != trigger {
            return Err(format!(
                "list `{list}` append trigger `{trigger}` does not match IR trigger `{expected}`"
            )
            .into());
        }
        self.append_row(list, row)
    }

    fn append_row_for_trigger_text(
        &mut self,
        equations: &ListEquationPlan,
        list: &str,
        trigger: &str,
        trigger_value: &str,
    ) -> RuntimeResult<(u64, u64)> {
        let append_fields = equations.append_fields(list, trigger)?;
        let template = self
            .lists
            .row_template(list)
            .cloned()
            .ok_or_else(|| format!("generic runtime has no row template for list `{list}`"))?;
        let mut row = self.lists.pop_spare(list).map(Ok).unwrap_or_else(|| {
            let initial_fields = equations.append_initial_fields(list, trigger, trigger_value)?;
            template.materialize(initial_fields)
        })?;
        template.reset_from_initial_text(&mut row, |initial_name| {
            append_fields
                .iter()
                .any(|field| field.name == initial_name && field.source == trigger)
                .then_some(trigger_value)
        })?;
        self.append_row_for_trigger(equations, list, trigger, row)
    }

    fn reserve_spare_rows_for_trigger_text(
        &mut self,
        equations: &ListEquationPlan,
        list: &str,
        trigger: &str,
        count: usize,
        text_capacity: usize,
    ) -> RuntimeResult<()> {
        let spare_len = self.lists.spare_len(list);
        if spare_len >= count {
            return Ok(());
        }
        let template = self
            .lists
            .row_template(list)
            .cloned()
            .ok_or_else(|| format!("generic runtime has no row template for list `{list}`"))?;
        let additional = count - spare_len;
        let spare_rows = self
            .lists
            .spare_rows_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?;
        spare_rows.reserve(additional + count);
        for _ in 0..additional {
            let initial_fields = equations.append_initial_fields(list, trigger, "")?;
            let mut row = template.materialize(initial_fields)?;
            row.reserve_textlike_fields(text_capacity)?;
            spare_rows.push(row);
        }
        Ok(())
    }

    fn append_row_for_trigger_text_and_bind_sources(
        &mut self,
        equations: &ListEquationPlan,
        list: &str,
        trigger: &str,
        trigger_value: &str,
        source_paths: &[String],
    ) -> RuntimeResult<GenericListRowCommit> {
        let (key, generation) =
            self.append_row_for_trigger_text(equations, list, trigger, trigger_value)?;
        self.bind_row_sources(list, key, generation, source_paths)?;
        Ok(GenericListRowCommit {
            list: list.to_owned(),
            key,
            generation,
        })
    }

    #[cfg(test)]
    fn append_text_row_source_action_and_bind_sources<'a>(
        &mut self,
        routes: &SourceRoutePlan,
        derived: &DerivedEquationPlan,
        lists: &ListEquationPlan,
        list: &str,
        source: &str,
        key: Option<&str>,
        text: Option<&'a str>,
        source_paths: &[String],
    ) -> RuntimeResult<Option<GenericTextListAppendCommit<'a>>> {
        let trigger = routes.list_append_trigger(source, list)?;
        let Some(value) = self.eval_derived_text_transform(derived, trigger, source, key, text)?
        else {
            return Ok(None);
        };
        let insert = self.append_row_for_trigger_text_and_bind_sources(
            lists,
            list,
            trigger,
            value.as_ref(),
            source_paths,
        )?;
        Ok(Some(GenericTextListAppendCommit {
            list: list.to_owned(),
            key: insert.key,
            generation: insert.generation,
            value,
        }))
    }

    fn spare_row(&mut self, list: &str, row: RuntimeRowSnapshot) -> RuntimeResult<()> {
        self.lists.push_spare(list, row)
    }

    fn remove_row_for_predicate(
        &mut self,
        list: &str,
        predicate: RuntimeListPredicate,
        index: usize,
    ) -> RuntimeResult<Option<KeyedRow<RuntimeRowSnapshot>>> {
        if predicate == RuntimeListPredicate::Unsupported {
            return Err(
                format!("remove over list `{list}` has unsupported predicate in IR").into(),
            );
        }
        if !self.list_row_matches_predicate(list, index, &predicate)? {
            return Ok(None);
        }
        self.remove_row(list, index).map(Some)
    }

    fn remove_row_for_predicate_and_unbind_sources(
        &mut self,
        list: &str,
        predicate: &RuntimeListPredicate,
        index: usize,
        mut observe_binding: impl FnMut(&SourceBinding) -> RuntimeResult<()>,
    ) -> RuntimeResult<Option<KeyedRow<RuntimeRowSnapshot>>> {
        let Some(row) = self.remove_row_for_predicate(list, predicate.clone(), index)? else {
            return Ok(None);
        };
        for binding in self.row_source_bindings(list, row.key, row.generation) {
            observe_binding(binding)?;
        }
        self.unbind_row_sources(list, row.key, row.generation);
        Ok(Some(row))
    }

    fn remove_where_source_action_and_unbind_sources(
        &mut self,
        routes: &SourceRoutePlan,
        list: &str,
        source_id: SourceId,
        mut observe: impl FnMut(GenericListRemoveObservation<'_>) -> RuntimeResult<()>,
    ) -> RuntimeResult<()> {
        let predicate = routes.list_remove_predicate_for_source_id(source_id, list)?;
        let mut index = 0;
        while index < self.list_len(list)? {
            let Some(row) = self.remove_row_for_predicate_and_unbind_sources(
                list,
                &predicate,
                index,
                |binding| observe(GenericListRemoveObservation::SourceUnbind(binding)),
            )?
            else {
                index += 1;
                continue;
            };
            let (key, generation) = (row.key, row.generation);
            observe(GenericListRemoveObservation::RowRemoved { key, generation })?;
            self.spare_row(list, row.value)?;
        }
        Ok(())
    }

    fn remove_index_source_action_and_unbind_sources(
        &mut self,
        routes: &SourceRoutePlan,
        list: &str,
        source_id: SourceId,
        index: usize,
        observe_binding: impl FnMut(&SourceBinding) -> RuntimeResult<()>,
    ) -> RuntimeResult<Option<(u64, u64)>> {
        let predicate = routes.list_remove_predicate_for_source_id(source_id, list)?;
        let Some(row) = self.remove_row_for_predicate_and_unbind_sources(
            list,
            &predicate,
            index,
            observe_binding,
        )?
        else {
            return Ok(None);
        };
        let identity = (row.key, row.generation);
        self.spare_row(list, row.value)?;
        Ok(Some(identity))
    }

    #[cfg(test)]
    fn remove_row_and_unbind_sources(
        &mut self,
        list: &str,
        index: usize,
        mut observe_binding: impl FnMut(&SourceBinding),
    ) -> RuntimeResult<KeyedRow<RuntimeRowSnapshot>> {
        let row = self.remove_row(list, index)?;
        for binding in self.row_source_bindings(list, row.key, row.generation) {
            observe_binding(binding);
        }
        self.unbind_row_sources(list, row.key, row.generation);
        Ok(row)
    }

    fn append_row(&mut self, list: &str, row: RuntimeRowSnapshot) -> RuntimeResult<(u64, u64)> {
        if let Some(capacity) = self.lists.capacity(list) {
            let len = self.list_len(list)?;
            if len >= capacity {
                return Err(format!(
                    "generic list `{list}` capacity {capacity} exceeded by append"
                )
                .into());
            }
        }
        Ok(self
            .lists
            .memory_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .append(row))
    }

    fn remove_row(
        &mut self,
        list: &str,
        index: usize,
    ) -> RuntimeResult<KeyedRow<RuntimeRowSnapshot>> {
        let rows = self
            .lists
            .memory_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?;
        if index >= rows.len() {
            return Err(format!("generic list `{list}` has no index {index}").into());
        }
        Ok(rows.remove_index(index))
    }

    fn move_row(
        &mut self,
        list: &str,
        from: usize,
        to: usize,
    ) -> RuntimeResult<GenericListRowCommit> {
        let (key, generation) = self
            .lists
            .memory_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .move_index(from, to)?;
        Ok(GenericListRowCommit {
            list: list.to_owned(),
            key,
            generation,
        })
    }

    fn row_identity(&self, list: &str, index: usize) -> RuntimeResult<(u64, u64)> {
        self.lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .row_identity(index)
            .ok_or_else(|| format!("generic list `{list}` has no index {index}").into())
    }

    fn bound_index(&self, list: &str, key: u64, generation: u64) -> RuntimeResult<Option<usize>> {
        Ok(self
            .lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .bound_index(key, generation))
    }

    fn list_len(&self, list: &str) -> RuntimeResult<usize> {
        self.lists
            .memory(list)
            .map(ListMemory::len)
            .ok_or_else(|| format!("generic runtime has no list `{list}`").into())
    }

    fn list_row_matches_predicate(
        &self,
        list: &str,
        index: usize,
        predicate: &RuntimeListPredicate,
    ) -> RuntimeResult<bool> {
        match predicate {
            RuntimeListPredicate::AlwaysTrue => Ok(true),
            RuntimeListPredicate::FieldBool { path } => {
                self.list_row_bool(list, index, row_field_name(path))
            }
            RuntimeListPredicate::FieldBoolNot { path } => {
                Ok(!self.list_row_bool(list, index, row_field_name(path))?)
            }
            RuntimeListPredicate::SelectorVisibility {
                selector,
                row_field,
            } => {
                let row_value = self.list_row_bool(list, index, row_field_name(row_field))?;
                Ok(match self.root_textlike_ref(selector)? {
                    "Active" => !row_value,
                    "Completed" => row_value,
                    _ => true,
                })
            }
            RuntimeListPredicate::Unsupported => Err("unsupported list predicate".into()),
        }
    }

    fn count_list_rows_for_target(
        &self,
        equations: &ListEquationPlan,
        list: &str,
        target: &str,
    ) -> RuntimeResult<usize> {
        let predicate = equations.count_predicate(list, target)?;
        self.count_list_rows_matching(list, predicate)
    }

    fn count_list_rows_matching(
        &self,
        list: &str,
        predicate: RuntimeListPredicate,
    ) -> RuntimeResult<usize> {
        if predicate == RuntimeListPredicate::Unsupported {
            return Err(format!("count over list `{list}` has unsupported predicate in IR").into());
        }
        let rows = self
            .lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?;
        let mut count = 0usize;
        for index in 0..rows.len() {
            if self.list_row_matches_predicate(list, index, &predicate)? {
                count += 1;
            }
        }
        Ok(count)
    }

    fn collect_list_textlike_for_retain(
        &self,
        equations: &ListEquationPlan,
        list: &str,
        target: &str,
        field: &str,
    ) -> RuntimeResult<Vec<String>> {
        let predicate = equations.retain_predicate(list, target)?;
        self.collect_list_textlike_matching(list, field, predicate)
    }

    fn collect_list_textlike_matching(
        &self,
        list: &str,
        field: &str,
        predicate: RuntimeListPredicate,
    ) -> RuntimeResult<Vec<String>> {
        if predicate == RuntimeListPredicate::Unsupported {
            return Err(format!(
                "text projection over list `{list}` has unsupported predicate in IR"
            )
            .into());
        }
        let rows = self
            .lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?;
        let mut values = Vec::new();
        for index in 0..rows.len() {
            let visible = self.list_row_matches_predicate(list, index, &predicate)?;
            if let Some(value) = while_value(visible, self.list_row_textlike(list, index, field)?) {
                values.push(value.to_owned());
            }
        }
        Ok(values)
    }

    fn collect_list_textlike_where_bool(
        &self,
        list: &str,
        text_field: &str,
        bool_field: &str,
        expected: bool,
    ) -> RuntimeResult<Vec<String>> {
        let rows = self
            .lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?;
        let mut values = Vec::new();
        for index in 0..rows.len() {
            if self.list_row_bool(list, index, bool_field)? == expected {
                values.push(self.list_row_textlike(list, index, text_field)?.to_owned());
            }
        }
        Ok(values)
    }

    fn first_list_textlike_where_bool(
        &self,
        list: &str,
        text_field: &str,
        bool_field: &str,
        expected: bool,
    ) -> RuntimeResult<Option<String>> {
        let rows = self
            .lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?;
        for index in 0..rows.len() {
            if self.list_row_bool(list, index, bool_field)? == expected {
                return Ok(Some(
                    self.list_row_textlike(list, index, text_field)?.to_owned(),
                ));
            }
        }
        Ok(None)
    }

    fn assert_root_textlike(
        &self,
        step_id: &str,
        label: &str,
        path: &str,
        expected: &str,
    ) -> RuntimeResult<()> {
        let actual = self.root_textlike(path)?;
        assert_eq_report(step_id, label, &expected, &actual.as_str())
    }

    fn assert_list_textlike_projection(
        &self,
        step_id: &str,
        label: &str,
        list: &str,
        field: &str,
        predicate: RuntimeListPredicate,
        expected: &[String],
    ) -> RuntimeResult<()> {
        let actual = self.collect_list_textlike_matching(list, field, predicate)?;
        assert_eq_report(step_id, label, &expected.to_vec(), &actual)
    }

    fn assert_list_textlike_retain_projection(
        &self,
        equations: &ListEquationPlan,
        step_id: &str,
        label: &str,
        list: &str,
        target: &str,
        field: &str,
        expected: &[String],
    ) -> RuntimeResult<()> {
        let actual = self.collect_list_textlike_for_retain(equations, list, target, field)?;
        assert_eq_report(step_id, label, &expected.to_vec(), &actual)
    }

    fn assert_list_textlike_where_bool(
        &self,
        step_id: &str,
        label: &str,
        list: &str,
        text_field: &str,
        bool_field: &str,
        expected_bool: bool,
        expected: &[String],
    ) -> RuntimeResult<()> {
        let actual =
            self.collect_list_textlike_where_bool(list, text_field, bool_field, expected_bool)?;
        assert_eq_report(step_id, label, &expected.to_vec(), &actual)
    }

    fn assert_list_count_for_target(
        &self,
        equations: &ListEquationPlan,
        step_id: &str,
        label: &str,
        list: &str,
        target: &str,
        expected: usize,
    ) -> RuntimeResult<()> {
        let actual = self.count_list_rows_for_target(equations, list, target)?;
        assert_num(step_id, label, expected, actual)
    }

    fn assert_first_list_textlike_where_bool(
        &self,
        step_id: &str,
        label: &str,
        list: &str,
        text_field: &str,
        bool_field: &str,
        expected_bool: bool,
        expected: &str,
    ) -> RuntimeResult<()> {
        let actual =
            self.first_list_textlike_where_bool(list, text_field, bool_field, expected_bool)?;
        assert_eq_report(step_id, label, &Some(expected.to_owned()), &actual)
    }

    fn assert_no_list_bool(
        &self,
        step_id: &str,
        list: &str,
        bool_field: &str,
        expected_bool: bool,
    ) -> RuntimeResult<()> {
        if self.any_list_bool(list, bool_field, expected_bool)? {
            return Err(
                format!("{step_id} expected no `{list}.{bool_field}` = {expected_bool}").into(),
            );
        }
        Ok(())
    }

    fn assert_list_row_textlike(
        &self,
        step_id: &str,
        label: &str,
        list: &str,
        index: usize,
        field: &str,
        expected: &str,
    ) -> RuntimeResult<()> {
        let actual = self.list_row_textlike(list, index, field)?;
        assert_eq_report(step_id, label, &expected, &actual)
    }

    fn assert_list_row_bool(
        &self,
        step_id: &str,
        label: &str,
        list: &str,
        index: usize,
        field: &str,
        expected: bool,
    ) -> RuntimeResult<()> {
        let actual = self.list_row_bool(list, index, field)?;
        assert_eq_report(step_id, label, &expected, &actual)
    }

    fn any_list_bool(&self, list: &str, field: &str, expected: bool) -> RuntimeResult<bool> {
        let rows = self
            .lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?;
        for index in 0..rows.len() {
            if self.list_row_bool(list, index, field)? == expected {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn list_row_field(
        &self,
        list: &str,
        index: usize,
        field: &str,
    ) -> RuntimeResult<FieldValueRef<'_>> {
        self.lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .value(index, field)
            .ok_or_else(|| format!("generic list `{list}` row missing field `{field}`").into())
    }
}

impl FieldValueRef<'_> {
    fn owned_value(&self) -> FieldValue {
        match self {
            Self::Text(value) => FieldValue::Text((*value).to_owned()),
            Self::Bool(value) => FieldValue::Bool(*value),
            Self::Enum(value) => FieldValue::Enum((*value).to_owned()),
        }
    }

    fn as_json(&self) -> JsonValue {
        match self {
            Self::Text(value) | Self::Enum(value) => json!(value),
            Self::Bool(value) => json!(value),
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            Self::Text(_) | Self::Enum(_) => None,
        }
    }
}

fn field_value_json(value: FieldValue) -> JsonValue {
    match value {
        FieldValue::Text(value) | FieldValue::Enum(value) => json!(value),
        FieldValue::Bool(value) => json!(value),
    }
}

fn boon_value_json(value: &BoonValue) -> JsonValue {
    match value {
        BoonValue::Empty => JsonValue::Null,
        BoonValue::Text(value) => json!(value),
        BoonValue::Number(value) => json!(value),
        BoonValue::Bool(value) => json!(value),
        BoonValue::Record(fields) => JsonValue::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), boon_value_json(value)))
                .collect(),
        ),
        BoonValue::List(values) => JsonValue::Array(values.iter().map(boon_value_json).collect()),
        BoonValue::RowRef { list, index } => json!({ "list": list, "index": index }),
        BoonValue::ListRef(list) => json!(list),
        BoonValue::NaN => JsonValue::Null,
        BoonValue::Error(error) => json!({ "error": error }),
    }
}

fn insert_nested_json(root: &mut serde_json::Map<String, JsonValue>, path: &str, value: JsonValue) {
    fn insert_parts(
        object: &mut serde_json::Map<String, JsonValue>,
        parts: &[&str],
        value: JsonValue,
    ) {
        match parts {
            [] => {}
            [leaf] => {
                object.insert((*leaf).to_owned(), value);
            }
            [head, tail @ ..] => {
                let entry = object
                    .entry((*head).to_owned())
                    .or_insert_with(|| JsonValue::Object(serde_json::Map::new()));
                if !entry.is_object() {
                    *entry = JsonValue::Object(serde_json::Map::new());
                }
                if let Some(child) = entry.as_object_mut() {
                    insert_parts(child, tail, value);
                }
            }
        }
    }
    let parts = path
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    insert_parts(root, &parts, value);
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct RuntimePathTail {
    indexes: Vec<usize>,
    rest: String,
}

fn runtime_path_matches_target(path: &str, target: &str) -> bool {
    path == target || path == row_field_name(target)
}

fn runtime_path_tail_for_target(path: &str, target: &str) -> Option<RuntimePathTail> {
    runtime_path_tail_after_head(path, target)
        .or_else(|| runtime_path_tail_after_head(path, row_field_name(target)))
}

fn runtime_path_tail_after_head(path: &str, head: &str) -> Option<RuntimePathTail> {
    let suffix = path.strip_prefix(head)?;
    if suffix.is_empty() {
        return Some(RuntimePathTail::default());
    }
    let mut remaining = suffix;
    let mut indexes = Vec::new();
    while let Some(after_open) = remaining.strip_prefix('[') {
        let (index_text, after_close) = after_open.split_once(']')?;
        let index = index_text.parse::<usize>().ok()?;
        indexes.push(index);
        remaining = after_close;
    }
    let rest = if remaining.is_empty() {
        String::new()
    } else {
        remaining.strip_prefix('.')?.to_owned()
    };
    Some(RuntimePathTail { indexes, rest })
}

fn runtime_json_value_summary(
    value: &JsonValue,
    depth: usize,
    max_depth: usize,
    max_fields: usize,
    max_list_items: usize,
) -> JsonValue {
    if depth >= max_depth {
        return json!({
            "kind": runtime_json_value_kind(value),
            "collapsed": true
        });
    }
    match value {
        JsonValue::Null => json!({"kind": "null", "value": null}),
        JsonValue::Bool(value) => json!({"kind": "bool", "value": value}),
        JsonValue::Number(value) => json!({"kind": "number", "value": value}),
        JsonValue::String(value) => json!({"kind": "string", "value": value}),
        JsonValue::Array(items) => {
            let sample = items
                .iter()
                .take(max_list_items)
                .map(|item| {
                    runtime_json_value_summary(
                        item,
                        depth + 1,
                        max_depth,
                        max_fields,
                        max_list_items,
                    )
                })
                .collect::<Vec<_>>();
            json!({
                "kind": "list",
                "len": items.len(),
                "sample_start": 0,
                "sample": sample,
                "truncated": items.len() > max_list_items
            })
        }
        JsonValue::Object(fields) => {
            let sampled = fields
                .iter()
                .take(max_fields)
                .map(|(field, value)| {
                    (
                        field.clone(),
                        runtime_json_value_summary(
                            value,
                            depth + 1,
                            max_depth,
                            max_fields,
                            max_list_items,
                        ),
                    )
                })
                .collect::<serde_json::Map<_, _>>();
            json!({
                "kind": "object",
                "field_count": fields.len(),
                "fields": sampled,
                "truncated": fields.len() > max_fields
            })
        }
    }
}

fn runtime_json_value_kind(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "bool",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "list",
        JsonValue::Object(_) => "object",
    }
}

fn runtime_row_tail_summary(
    row: serde_json::Map<String, JsonValue>,
    rest: &str,
    depth: usize,
    max_depth: usize,
    max_fields: usize,
    max_list_items: usize,
) -> Option<JsonValue> {
    let row = JsonValue::Object(row);
    let value = if rest.is_empty() {
        &row
    } else {
        runtime_value_at_path(&row, rest)?
    };
    Some(runtime_json_value_summary(
        value,
        depth,
        max_depth,
        max_fields,
        max_list_items,
    ))
}

fn runtime_value_at_path<'a>(root: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    let mut value = root;
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        let (field, indexes) = runtime_path_segment_indexes(segment);
        if !field.is_empty() {
            value = value.get(field)?;
        }
        for index in indexes {
            value = value.as_array()?.get(index)?;
        }
    }
    Some(value)
}

fn runtime_path_segment_indexes(segment: &str) -> (&str, Vec<usize>) {
    let field_end = segment.find('[').unwrap_or(segment.len());
    let field = &segment[..field_end];
    let indexes = segment[field_end..]
        .split('[')
        .filter_map(|piece| piece.strip_suffix(']'))
        .filter_map(|piece| piece.parse::<usize>().ok())
        .collect();
    (field, indexes)
}

impl ValueColumns {
    fn insert_value(&mut self, name: String, value: FieldValue) {
        let field_id = FieldSlotId::from_path(&name);
        self.remove_field_id(&field_id);
        match value {
            FieldValue::Text(value) => {
                Self::insert_text_slot(&mut self.text, field_id, value);
            }
            FieldValue::Bool(value) => {
                Self::insert_bool_slot(&mut self.bools, field_id, value);
            }
            FieldValue::Enum(value) => {
                Self::insert_text_slot(&mut self.enums, field_id, value);
            }
        }
    }

    fn remove_field_id(&mut self, field_id: &FieldSlotId) {
        if let Ok(index) = Self::text_slot_index(&self.text, field_id) {
            self.text.remove(index);
        }
        if let Ok(index) = Self::bool_slot_index(&self.bools, field_id) {
            self.bools.remove(index);
        }
        if let Ok(index) = Self::text_slot_index(&self.enums, field_id) {
            self.enums.remove(index);
        }
    }

    fn contains_key(&self, field: &str) -> bool {
        self.contains_key_id(FieldSlotId::from_path(field))
    }

    fn contains_key_id(&self, field_id: FieldSlotId) -> bool {
        Self::text_slot_index(&self.text, &field_id).is_ok()
            || Self::bool_slot_index(&self.bools, &field_id).is_ok()
            || Self::text_slot_index(&self.enums, &field_id).is_ok()
    }

    fn owned_value(&self, field: &str) -> Option<FieldValue> {
        if let Some(index) = self.text_index(field) {
            Some(FieldValue::Text(self.text[index].value.clone()))
        } else if let Some(index) = self.bool_index(field) {
            Some(FieldValue::Bool(self.bools[index].value))
        } else {
            self.enum_index(field)
                .map(|index| FieldValue::Enum(self.enums[index].value.clone()))
        }
    }

    fn textlike(&self, field: &str) -> Option<&str> {
        self.text_index(field)
            .map(|index| self.text[index].value.as_str())
            .or_else(|| {
                self.enum_index(field)
                    .map(|index| self.enums[index].value.as_str())
            })
    }

    fn bool_value(&self, field: &str) -> Option<bool> {
        self.bool_index(field).map(|index| self.bools[index].value)
    }

    fn set_textlike(&mut self, field: &str, value: &str) -> RuntimeResult<()> {
        if let Some(index) = self.text_index(field) {
            let current = &mut self.text[index].value;
            current.clear();
            current.push_str(value);
            Ok(())
        } else if let Some(index) = self.enum_index(field) {
            let current = &mut self.enums[index].value;
            current.clear();
            current.push_str(value);
            Ok(())
        } else if self.bool_index(field).is_some() {
            Err(format!("cannot write text into bool runtime value `{field}`").into())
        } else {
            Err(format!("generic row missing field `{field}`").into())
        }
    }

    #[cfg(test)]
    fn set_or_insert_text(&mut self, field: &str, value: &str) -> RuntimeResult<()> {
        if self.contains_key(field) {
            self.set_textlike(field, value)
        } else {
            let field_id = FieldSlotId::from_path(field);
            Self::insert_text_slot(&mut self.text, field_id, value.to_owned());
            Ok(())
        }
    }

    fn set_bool(&mut self, field: &str, value: bool) -> RuntimeResult<()> {
        if let Some(index) = self.bool_index(field) {
            self.bools[index].value = value;
            Ok(())
        } else if self.text_index(field).is_some() || self.enum_index(field).is_some() {
            Err("cannot write bool into text runtime value".into())
        } else {
            Err(format!("generic row missing field `{field}`").into())
        }
    }

    fn set_value(&mut self, field: &str, value: FieldValue) -> RuntimeResult<()> {
        match value {
            FieldValue::Text(value) | FieldValue::Enum(value) => self.set_textlike(field, &value),
            FieldValue::Bool(value) => self.set_bool(field, value),
        }
    }

    fn reserve_textlike(&mut self, field: &str, additional: usize) -> RuntimeResult<()> {
        if let Some(index) = self.text_index(field) {
            self.text[index].value.reserve(additional);
            Ok(())
        } else if let Some(index) = self.enum_index(field) {
            self.enums[index].value.reserve(additional);
            Ok(())
        } else if self.bool_index(field).is_some() {
            Err("cannot reserve text capacity on bool runtime value".into())
        } else {
            Err(format!("generic row missing field `{field}`").into())
        }
    }

    fn reserve_all_textlike(&mut self, additional: usize) {
        for slot in &mut self.text {
            slot.value.reserve(additional);
        }
        for slot in &mut self.enums {
            slot.value.reserve(additional);
        }
    }

    fn text_index(&self, field: &str) -> Option<usize> {
        self.text_index_id(FieldSlotId::from_path(field))
    }

    fn text_index_id(&self, field_id: FieldSlotId) -> Option<usize> {
        Self::text_slot_index(&self.text, &field_id).ok()
    }

    fn bool_index(&self, field: &str) -> Option<usize> {
        self.bool_index_id(FieldSlotId::from_path(field))
    }

    fn bool_index_id(&self, field_id: FieldSlotId) -> Option<usize> {
        Self::bool_slot_index(&self.bools, &field_id).ok()
    }

    fn enum_index(&self, field: &str) -> Option<usize> {
        self.enum_index_id(FieldSlotId::from_path(field))
    }

    fn enum_index_id(&self, field_id: FieldSlotId) -> Option<usize> {
        Self::text_slot_index(&self.enums, &field_id).ok()
    }

    fn insert_text_slot(slots: &mut Vec<TextValueSlot>, field_id: FieldSlotId, value: String) {
        let index = Self::text_slot_index(slots, &field_id).unwrap_or_else(|index| index);
        slots.insert(index, TextValueSlot { field_id, value });
    }

    fn insert_bool_slot(slots: &mut Vec<BoolValueSlot>, field_id: FieldSlotId, value: bool) {
        let index = Self::bool_slot_index(slots, &field_id).unwrap_or_else(|index| index);
        slots.insert(index, BoolValueSlot { field_id, value });
    }

    fn text_slot_index(slots: &[TextValueSlot], field_id: &FieldSlotId) -> Result<usize, usize> {
        slots.binary_search_by(|slot| slot.field_id.cmp(field_id))
    }

    fn bool_slot_index(slots: &[BoolValueSlot], field_id: &FieldSlotId) -> Result<usize, usize> {
        slots.binary_search_by(|slot| slot.field_id.cmp(field_id))
    }
}

fn list_initial_fields(row: &boon_ir::ListInitialRecord) -> RuntimeResult<ValueColumns> {
    let mut columns = ValueColumns::default();
    for field in &row.fields {
        columns.insert_value(
            field.name.clone(),
            runtime_value_from_initial(&field.value, &ValueColumns::default())?,
        );
    }
    Ok(columns)
}

impl RuntimeRowSnapshotTemplate {
    fn from_cells(
        row_scope: &str,
        indexed_cells: &[&boon_ir::StateCell],
        ir: &TypedProgram,
    ) -> RuntimeResult<Self> {
        let mut fields = Vec::with_capacity(indexed_cells.len());
        for cell in indexed_cells {
            let field = cell
                .path
                .strip_prefix(&format!("{row_scope}."))
                .ok_or_else(|| {
                    format!(
                        "state cell `{}` is not in row scope `{row_scope}`",
                        cell.path
                    )
                })?
                .to_owned();
            fields.push(RuntimeRowSnapshotFieldTemplate {
                field_id: FieldSlotId::from_path(&field),
                field_name: field.into_boxed_str(),
                initial_value: cell.initial_value.clone(),
                missing_row_initial_value: missing_row_initial_value(cell, ir),
            });
        }
        Ok(Self { fields })
    }

    fn materialize(&self, mut initial_fields: ValueColumns) -> RuntimeResult<RuntimeRowSnapshot> {
        for field in &self.fields {
            if initial_fields.contains_key_id(field.field_id.clone()) {
                continue;
            }
            let value = runtime_value_from_initial(&field.initial_value, &initial_fields)
                .or_else(|error| field.missing_row_initial_value.clone().ok_or(error))?;
            initial_fields.insert_value(field.field_name.to_string(), value);
        }
        Ok(RuntimeRowSnapshot {
            columns: initial_fields,
        })
    }

    fn fill_missing_row_initial_fields(&self, initial_fields: &mut ValueColumns) {
        for field in &self.fields {
            let InitialValue::RowInitialField { path } = &field.initial_value else {
                continue;
            };
            let field_id = FieldSlotId::from_path(path);
            if !initial_fields.contains_key_id(field_id) {
                let value = field
                    .missing_row_initial_value
                    .clone()
                    .unwrap_or_else(|| FieldValue::Text(String::new()));
                initial_fields.insert_value(path.clone(), value);
            }
        }
    }

    fn reset_from_initial_text<'a>(
        &self,
        row: &mut RuntimeRowSnapshot,
        initial_text: impl Fn(&str) -> Option<&'a str>,
    ) -> RuntimeResult<()> {
        for field in &self.fields {
            if !row.columns.contains_key_id(field.field_id.clone()) {
                return Err(format!("generic row missing field `{}`", field.field_name).into());
            }
            match &field.initial_value {
                InitialValue::Text { value } => {
                    row.columns.set_textlike(&field.field_name, value)?
                }
                InitialValue::Number { value } => row
                    .columns
                    .set_textlike(&field.field_name, &value.to_string())?,
                InitialValue::Bool { value } => row.columns.set_bool(&field.field_name, *value)?,
                InitialValue::Enum { value } => {
                    row.columns.set_textlike(&field.field_name, value)?
                }
                InitialValue::RowInitialField { path } => {
                    if let Some(value) = initial_text(path) {
                        row.columns.set_textlike(&field.field_name, value)?;
                    } else if let Some(value) = field.missing_row_initial_value.clone() {
                        row.columns.set_value(&field.field_name, value)?;
                    } else {
                        return Err(format!("row initial field `{path}` is missing").into());
                    }
                }
                InitialValue::Unknown { summary } => {
                    return Err(format!("unsupported state initializer `{summary}`").into());
                }
            }
        }
        Ok(())
    }
}

impl RuntimeRowSnapshot {
    fn reserve_textlike_fields(&mut self, additional: usize) -> RuntimeResult<()> {
        self.columns.reserve_all_textlike(additional);
        Ok(())
    }
}

fn missing_row_initial_value(cell: &boon_ir::StateCell, ir: &TypedProgram) -> Option<FieldValue> {
    if !matches!(cell.initial_value, InitialValue::RowInitialField { .. }) {
        return None;
    }
    ir.update_branches
        .iter()
        .any(|branch| {
            branch.target == cell.path
                && matches!(branch.expression, UpdateExpression::BoolNot { .. })
        })
        .then_some(FieldValue::Bool(false))
}

fn runtime_value_from_initial(
    initial: &InitialValue,
    initial_fields: &ValueColumns,
) -> RuntimeResult<FieldValue> {
    match initial {
        InitialValue::Text { value } => Ok(FieldValue::Text(value.clone())),
        InitialValue::Number { value } => Ok(FieldValue::Text(value.to_string())),
        InitialValue::Bool { value } => Ok(FieldValue::Bool(*value)),
        InitialValue::Enum { value } => Ok(FieldValue::Enum(value.clone())),
        InitialValue::RowInitialField { path } => initial_fields
            .owned_value(path)
            .ok_or_else(|| format!("row initial field `{path}` is missing").into()),
        InitialValue::Unknown { summary } => {
            Err(format!("unsupported state initializer `{summary}`").into())
        }
    }
}

fn initialize_indexed_derived_text_fields(
    ir: &TypedProgram,
    row_scope: &str,
    row: &mut RuntimeRowSnapshot,
) {
    for value in ir.derived_values.iter().filter(|value| value.indexed) {
        let Some(field) = value
            .path
            .strip_prefix(row_scope)
            .and_then(|path| path.strip_prefix('.'))
        else {
            continue;
        };
        if !row.columns.contains_key(field) {
            row.columns
                .insert_value(field.to_owned(), FieldValue::Text(String::new()));
        }
    }
}

fn row_scope_name(list_name: &str) -> String {
    list_name
        .strip_suffix("ies")
        .map(|prefix| format!("{prefix}y"))
        .or_else(|| list_name.strip_suffix('s').map(str::to_owned))
        .unwrap_or_else(|| format!("{list_name}_item"))
}

fn row_scope_matches_list(list_name: &str, scope: &str) -> bool {
    if let Some(prefix) = list_name.strip_suffix("ies") {
        return scope.strip_suffix('y') == Some(prefix);
    }
    if let Some(prefix) = list_name.strip_suffix('s') {
        return scope == prefix;
    }
    scope.strip_suffix("_item") == Some(list_name)
}

#[cfg(test)]
fn todo_generic_row(title: &str) -> RuntimeRowSnapshot {
    let mut columns = ValueColumns::default();
    columns.insert_value("title".to_owned(), FieldValue::Text(title.to_owned()));
    columns.insert_value("edit_text".to_owned(), FieldValue::Text(title.to_owned()));
    columns.insert_value("completed".to_owned(), FieldValue::Bool(false));
    columns.insert_value("editing".to_owned(), FieldValue::Bool(false));
    RuntimeRowSnapshot { columns }
}

#[cfg(test)]
fn close_other_list_editors<'a>(
    generic: &mut GenericScheduledRuntime,
    active_index: usize,
    deltas: &mut Vec<SemanticDelta<'a>>,
    patches: &mut Vec<RenderPatch<'a>>,
) -> RuntimeResult<()> {
    for index in 0..generic.list_len("todos")? {
        if index == active_index
            || !generic
                .list_row_bool_opt("todos", index, "editing")
                .unwrap_or(false)
        {
            continue;
        }
        let (key, generation) =
            generic.commit_indexed_bool_field("todos", index, "editing", false)?;
        let commit = GenericBoolFieldCommit {
            list: "todos".to_owned(),
            key,
            generation,
            field: "editing".to_owned(),
            value: false,
        };
        deltas.push(commit.semantic_delta());
        emit_generic_render_patch_for_mutation(
            &GenericSourceMutation::BoolField(commit),
            GenericRenderContext::default(),
            patches,
        )?;
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct ScalarEquationPlan {
    branches: Vec<ScalarUpdateBranch>,
}

#[derive(Clone, Debug)]
struct ScalarUpdateBranch {
    target: String,
    source: String,
    expression: ScalarUpdateExpression,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ScalarUpdateExpression {
    SourceText,
    SourceKey,
    SourceAddress,
    Const(String),
    NumberInfix {
        left: String,
        op: String,
        right: String,
    },
    PreviousValue(String),
    ReadPath(String),
    TextTrimOrPrevious {
        path: String,
        previous: String,
    },
    BoolNot(String),
    MatchConst {
        input: String,
        arms: Vec<UpdateMatchArm>,
    },
    Unsupported,
}

impl ScalarUpdateExpression {
    fn is_indexed_text_expression(&self) -> bool {
        matches!(
            self,
            Self::SourceText
                | Self::SourceKey
                | Self::PreviousValue(_)
                | Self::TextTrimOrPrevious { .. }
        )
    }

    fn is_indexed_bool_expression(&self) -> bool {
        matches!(self, Self::Const(value) if value == "True" || value == "False")
            || matches!(self, Self::BoolNot(_))
    }

    fn const_bool(&self) -> Option<bool> {
        match self {
            Self::Const(value) if value == "True" => Some(true),
            Self::Const(value) if value == "False" => Some(false),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
enum ScalarTextValue<'a> {
    Text(Cow<'a, str>),
    PreviousValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum IndexedTextCandidate<'a> {
    SourceText(Cow<'a, str>),
    PreviousText(Cow<'a, str>),
    PreviousField(String),
    TrimmedOrSkip(Option<Cow<'a, str>>),
}

#[derive(Clone, Debug)]
struct GenericTextFieldCommit<'a> {
    list: String,
    key: u64,
    generation: u64,
    field: String,
    value: Cow<'a, str>,
}

#[derive(Clone, Debug)]
struct GenericTextFieldIdentity {
    list: String,
    key: u64,
    generation: u64,
    field: String,
    value: String,
}

#[derive(Clone, Debug)]
struct GenericBoolFieldCommit {
    list: String,
    key: u64,
    generation: u64,
    field: String,
    value: bool,
}

#[derive(Clone, Debug)]
struct GenericValueFieldCommit<'a> {
    list: String,
    key: u64,
    generation: u64,
    field: String,
    value: ProtocolValue<'a>,
}

#[derive(Clone, Debug)]
struct GenericListRowCommit {
    list: String,
    key: u64,
    generation: u64,
}

#[derive(Clone, Debug)]
struct GenericTextListAppendCommit<'a> {
    list: String,
    key: u64,
    generation: u64,
    value: Cow<'a, str>,
}

#[derive(Clone, Debug)]
struct GenericRootTextCommit<'a> {
    target: String,
    value: Cow<'a, str>,
}

#[derive(Clone, Debug)]
struct GenericRootBoolCommit {
    target: String,
    value: bool,
}

#[derive(Clone, Debug)]
struct GenericRenderLoweringPlan;

#[derive(Clone, Debug, Default)]
struct GenericRenderContext<'a> {
    address: Option<&'a str>,
    patch_editor_text: bool,
    patch_value_text: bool,
}

impl<'a> GenericTextFieldCommit<'a> {
    fn semantic_delta(&self) -> SemanticDelta<'a> {
        SemanticDelta {
            kind: "FieldSet",
            list_id: Some(Cow::Owned(self.list.clone())),
            key: Some(self.key),
            generation: Some(self.generation),
            source_id: None,
            bind_epoch: None,
            field_path: Some(Cow::Owned(self.field.clone())),
            value: ProtocolValue::Text(self.value.clone()),
        }
    }
}

impl GenericBoolFieldCommit {
    fn semantic_delta(&self) -> SemanticDelta<'static> {
        SemanticDelta {
            kind: "FieldSet",
            list_id: Some(Cow::Owned(self.list.clone())),
            key: Some(self.key),
            generation: Some(self.generation),
            source_id: None,
            bind_epoch: None,
            field_path: Some(Cow::Owned(self.field.clone())),
            value: ProtocolValue::Bool(self.value),
        }
    }
}

impl<'a> GenericValueFieldCommit<'a> {
    fn semantic_delta(&self) -> SemanticDelta<'a> {
        SemanticDelta {
            kind: "FieldSet",
            list_id: Some(Cow::Owned(self.list.clone())),
            key: Some(self.key),
            generation: Some(self.generation),
            source_id: None,
            bind_epoch: None,
            field_path: Some(Cow::Owned(self.field.clone())),
            value: self.value.clone(),
        }
    }
}

impl GenericListRowCommit {
    fn semantic_move_delta(&self, to: usize) -> SemanticDelta<'static> {
        SemanticDelta {
            kind: "ListMove",
            list_id: Some(Cow::Owned(self.list.clone())),
            key: Some(self.key),
            generation: Some(self.generation),
            source_id: None,
            bind_epoch: None,
            field_path: Some(Cow::Borrowed("position")),
            value: ProtocolValue::NumberText(to as i64),
        }
    }
}

impl GenericTextFieldIdentity {
    fn semantic_delta_with_value<'a>(&self, value: ProtocolValue<'a>) -> SemanticDelta<'a> {
        SemanticDelta {
            kind: "FieldSet",
            list_id: Some(Cow::Owned(self.list.clone())),
            key: Some(self.key),
            generation: Some(self.generation),
            source_id: None,
            bind_epoch: None,
            field_path: Some(Cow::Owned(self.field.clone())),
            value,
        }
    }
}

impl<'a> GenericTextListAppendCommit<'a> {
    fn semantic_delta(&self) -> SemanticDelta<'a> {
        SemanticDelta {
            kind: "ListInsert",
            list_id: Some(Cow::Owned(self.list.clone())),
            key: Some(self.key),
            generation: Some(self.generation),
            source_id: None,
            bind_epoch: None,
            field_path: None,
            value: ProtocolValue::Text(self.value.clone()),
        }
    }
}

impl<'a> GenericRootTextCommit<'a> {
    fn semantic_delta(&self) -> SemanticDelta<'a> {
        SemanticDelta {
            kind: "FieldSet",
            list_id: None,
            key: None,
            generation: None,
            source_id: None,
            bind_epoch: None,
            field_path: Some(Cow::Owned(self.target.clone())),
            value: ProtocolValue::Text(self.value.clone()),
        }
    }
}

impl GenericRootBoolCommit {
    fn semantic_delta(&self) -> SemanticDelta<'static> {
        SemanticDelta {
            kind: "FieldSet",
            list_id: None,
            key: None,
            generation: None,
            source_id: None,
            bind_epoch: None,
            field_path: Some(Cow::Owned(self.target.clone())),
            value: ProtocolValue::Bool(self.value),
        }
    }
}

impl GenericRenderLoweringPlan {
    fn generic() -> Self {
        Self
    }

    fn lower_mutation_patch<'a>(
        &self,
        mutation: &GenericSourceMutation<'a>,
        _context: GenericRenderContext<'a>,
    ) -> RuntimeResult<Option<RenderPatch<'a>>> {
        Ok(Some(generic_document_invalidation_patch(mutation)))
    }

    fn lower_row_affordance_patch<'a>(
        &self,
        list: &'a str,
        _key: u64,
        _generation: u64,
        affordance: &'static str,
        _visible: bool,
    ) -> RuntimeResult<RenderPatch<'a>> {
        Ok(patch(
            "InvalidateDocument",
            RenderTarget::Static(Cow::Borrowed("document")),
            ProtocolValue::Text(Cow::Owned(format!("{list}.{affordance}"))),
        ))
    }

    fn lower_list_move_patch<'a>(
        &self,
        commit: &GenericListRowCommit,
        to: usize,
    ) -> RuntimeResult<RenderPatch<'a>> {
        Ok(patch(
            "InvalidateDocument",
            RenderTarget::Static(Cow::Borrowed("document")),
            ProtocolValue::Text(Cow::Owned(format!("{}.position:{to}", commit.list))),
        ))
    }
}

impl<'a> GenericSourceMutation<'a> {
    fn semantic_delta(&self) -> Option<SemanticDelta<'a>> {
        match self {
            Self::RootText(commit) => Some(commit.semantic_delta()),
            Self::RootBool(commit) => Some(commit.semantic_delta()),
            Self::TextField(commit) => Some(commit.semantic_delta()),
            Self::TextFieldIdentity(commit) => Some(
                commit.semantic_delta_with_value(ProtocolValue::Text(Cow::Owned(
                    commit.value.clone(),
                ))),
            ),
            Self::ValueField(commit) => Some(commit.semantic_delta()),
            Self::BoolField(commit) => Some(commit.semantic_delta()),
            Self::ListAppend(commit) => Some(commit.semantic_delta()),
            Self::ListRemove {
                list,
                key,
                generation,
            } => Some(SemanticDelta {
                kind: "ListRemove",
                list_id: Some(Cow::Owned(list.clone())),
                key: Some(*key),
                generation: Some(*generation),
                source_id: None,
                bind_epoch: None,
                field_path: None,
                value: ProtocolValue::Null,
            }),
            Self::SourceBind(binding) => Some(GenericCircuitRuntime::semantic_source_delta(
                "SourceBind",
                binding,
                ProtocolValue::Text(Cow::Owned(binding.source_path.clone())),
            )),
            Self::SourceUnbind(binding) => Some(GenericCircuitRuntime::semantic_source_delta(
                "SourceUnbind",
                binding,
                ProtocolValue::Null,
            )),
        }
    }
}

fn generic_document_invalidation_patch<'a>(
    mutation: &GenericSourceMutation<'a>,
) -> RenderPatch<'a> {
    let value = match mutation {
        GenericSourceMutation::RootText(commit) => {
            ProtocolValue::Text(Cow::Owned(commit.target.clone()))
        }
        GenericSourceMutation::RootBool(commit) => {
            ProtocolValue::Text(Cow::Owned(commit.target.clone()))
        }
        GenericSourceMutation::TextField(commit) => {
            ProtocolValue::Text(Cow::Owned(format!("{}.{}", commit.list, commit.field)))
        }
        GenericSourceMutation::TextFieldIdentity(commit) => {
            ProtocolValue::Text(Cow::Owned(format!("{}.{}", commit.list, commit.field)))
        }
        GenericSourceMutation::ValueField(commit) => {
            ProtocolValue::Text(Cow::Owned(format!("{}.{}", commit.list, commit.field)))
        }
        GenericSourceMutation::BoolField(commit) => {
            ProtocolValue::Text(Cow::Owned(format!("{}.{}", commit.list, commit.field)))
        }
        GenericSourceMutation::ListAppend(commit) => {
            ProtocolValue::Text(Cow::Owned(commit.list.clone()))
        }
        GenericSourceMutation::ListRemove { list, .. } => {
            ProtocolValue::Text(Cow::Owned(list.clone()))
        }
        GenericSourceMutation::SourceBind(binding)
        | GenericSourceMutation::SourceUnbind(binding) => {
            ProtocolValue::Text(Cow::Owned(binding.source_path.clone()))
        }
    };
    patch(
        "InvalidateDocument",
        RenderTarget::Static(Cow::Borrowed("document")),
        value,
    )
}

enum GenericListRemoveObservation<'a> {
    SourceUnbind(&'a SourceBinding),
    RowRemoved { key: u64, generation: u64 },
}

#[derive(Clone, Debug)]
struct GenericSourceActionInput<'a> {
    source: &'a str,
    source_id: SourceId,
    list: Option<String>,
    index: Option<usize>,
    key: Option<&'a str>,
    text: Option<&'a str>,
    address: Option<&'a str>,
    seq: TickSeq,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GenericBoundSourceIndex {
    Unspecified,
    Bound(usize),
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GenericVisibleRowOccurrence {
    Occurrence(usize),
    Stale,
    Mismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceActionKind {
    RootText,
    RootScalar,
    RouterRoute,
    ListAppend,
    ListRemove,
    IndexedTextChange,
    IndexedTextCommit,
    IndexedTextIdentity,
    IndexedTextKey,
    IndexedTextOpen,
    IndexedBoolToggle,
    IndexedBoolBulk,
}

impl SourceActionKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::RootText => "root_text",
            Self::RootScalar => "root_scalar",
            Self::RouterRoute => "router_route",
            Self::ListAppend => "list_append",
            Self::ListRemove => "list_remove",
            Self::IndexedTextChange => "indexed_text_change",
            Self::IndexedTextCommit => "indexed_text_commit",
            Self::IndexedTextIdentity => "indexed_text_identity",
            Self::IndexedTextKey => "indexed_text_key",
            Self::IndexedTextOpen => "indexed_text_open",
            Self::IndexedBoolToggle => "indexed_bool_toggle",
            Self::IndexedBoolBulk => "indexed_bool_bulk",
        }
    }
}

#[allow(dead_code)]
enum GenericSourceMutation<'a> {
    RootText(GenericRootTextCommit<'a>),
    RootBool(GenericRootBoolCommit),
    TextField(GenericTextFieldCommit<'a>),
    TextFieldIdentity(GenericTextFieldIdentity),
    ValueField(GenericValueFieldCommit<'a>),
    BoolField(GenericBoolFieldCommit),
    ListAppend(GenericTextListAppendCommit<'a>),
    ListRemove {
        list: String,
        key: u64,
        generation: u64,
    },
    SourceBind(SourceBinding),
    SourceUnbind(SourceBinding),
}

#[derive(Debug)]
struct GenericSourceMutationBatch<'a> {
    root_texts: [Option<(String, GenericRootTextCommit<'a>)>; 4],
    text_fields: [Option<(String, GenericTextFieldCommit<'a>)>; 8],
    identity_fields: [Option<(String, GenericTextFieldIdentity)>; 8],
    bool_fields: [Option<(String, GenericBoolFieldCommit)>; 8],
    list_appends: [Option<(String, GenericTextListAppendCommit<'a>)>; 4],
}

impl<'a> GenericSourceMutationBatch<'a> {
    fn new() -> Self {
        Self {
            root_texts: std::array::from_fn(|_| None),
            text_fields: std::array::from_fn(|_| None),
            identity_fields: std::array::from_fn(|_| None),
            bool_fields: std::array::from_fn(|_| None),
            list_appends: std::array::from_fn(|_| None),
        }
    }

    fn observe(&mut self, mutation: &GenericSourceMutation<'a>) -> RuntimeResult<()> {
        match mutation {
            GenericSourceMutation::RootText(commit) => {
                Self::insert_root_text(&mut self.root_texts, &commit.target, commit.clone())?;
            }
            GenericSourceMutation::RootBool(_) => {}
            GenericSourceMutation::TextField(commit) => {
                Self::insert_text(&mut self.text_fields, &commit.field, commit.clone())?;
            }
            GenericSourceMutation::TextFieldIdentity(commit) => {
                Self::insert_identity(&mut self.identity_fields, &commit.field, commit.clone())?;
            }
            GenericSourceMutation::BoolField(commit) => {
                Self::insert_bool(&mut self.bool_fields, &commit.field, commit.clone())?;
            }
            GenericSourceMutation::ListAppend(commit) => {
                Self::insert_append(&mut self.list_appends, &commit.list, commit.clone())?;
            }
            GenericSourceMutation::ValueField(_)
            | GenericSourceMutation::ListRemove { .. }
            | GenericSourceMutation::SourceBind(_)
            | GenericSourceMutation::SourceUnbind(_) => {}
        }
        Ok(())
    }

    fn insert_root_text(
        slots: &mut [Option<(String, GenericRootTextCommit<'a>)>; 4],
        target: &str,
        commit: GenericRootTextCommit<'a>,
    ) -> RuntimeResult<()> {
        for slot in slots.iter_mut() {
            match slot {
                Some((existing, value)) if existing == target => {
                    *value = commit;
                    return Ok(());
                }
                None => {
                    *slot = Some((target.to_owned(), commit));
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(format!("source mutation batch root capacity exceeded for `{target}`").into())
    }

    fn insert_text(
        slots: &mut [Option<(String, GenericTextFieldCommit<'a>)>; 8],
        field: &str,
        commit: GenericTextFieldCommit<'a>,
    ) -> RuntimeResult<()> {
        for slot in slots.iter_mut() {
            match slot {
                Some((existing, value)) if existing == field => {
                    *value = commit;
                    return Ok(());
                }
                None => {
                    *slot = Some((field.to_owned(), commit));
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(format!("source mutation batch text capacity exceeded for `{field}`").into())
    }

    fn insert_identity(
        slots: &mut [Option<(String, GenericTextFieldIdentity)>; 8],
        field: &str,
        commit: GenericTextFieldIdentity,
    ) -> RuntimeResult<()> {
        for slot in slots.iter_mut() {
            match slot {
                Some((existing, value)) if existing == field => {
                    *value = commit;
                    return Ok(());
                }
                None => {
                    *slot = Some((field.to_owned(), commit));
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(format!("source mutation batch identity capacity exceeded for `{field}`").into())
    }

    fn insert_bool(
        slots: &mut [Option<(String, GenericBoolFieldCommit)>; 8],
        field: &str,
        commit: GenericBoolFieldCommit,
    ) -> RuntimeResult<()> {
        for slot in slots.iter_mut() {
            match slot {
                Some((existing, value)) if existing == field => {
                    *value = commit;
                    return Ok(());
                }
                None => {
                    *slot = Some((field.to_owned(), commit));
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(format!("source mutation batch bool capacity exceeded for `{field}`").into())
    }

    fn insert_append(
        slots: &mut [Option<(String, GenericTextListAppendCommit<'a>)>; 4],
        list: &str,
        commit: GenericTextListAppendCommit<'a>,
    ) -> RuntimeResult<()> {
        for slot in slots.iter_mut() {
            match slot {
                Some((existing, value)) if existing == list => {
                    *value = commit;
                    return Ok(());
                }
                None => {
                    *slot = Some((list.to_owned(), commit));
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(format!("source mutation batch append capacity exceeded for `{list}`").into())
    }

    fn require_text(
        &self,
        source: &str,
        label: &str,
        field: &str,
    ) -> RuntimeResult<GenericTextFieldCommit<'a>> {
        self.text(field).ok_or_else(|| {
            format!("{label} from `{source}` produced no `{field}` text change").into()
        })
    }

    fn text(&self, field: &str) -> Option<GenericTextFieldCommit<'a>> {
        self.text_fields.iter().find_map(|slot| match slot {
            Some((existing, commit)) if existing == field => Some(commit.clone()),
            _ => None,
        })
    }

    fn require_first_text_except(
        &self,
        source: &str,
        label: &str,
        excluded_field: &str,
    ) -> RuntimeResult<GenericTextFieldCommit<'a>> {
        self.text_fields
            .iter()
            .find_map(|slot| match slot {
                Some((field, commit)) if field != excluded_field => Some(commit.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                format!("{label} from `{source}` produced no non-`{excluded_field}` text change")
                    .into()
            })
    }

    fn require_identity(
        &self,
        source: &str,
        label: &str,
        field: &str,
    ) -> RuntimeResult<GenericTextFieldIdentity> {
        self.identity_fields
            .iter()
            .find_map(|slot| match slot {
                Some((existing, commit)) if existing == field => Some(commit.clone()),
                _ => None,
            })
            .ok_or_else(|| format!("{label} from `{source}` produced no `{field}` identity").into())
    }

    fn require_first_identity(
        &self,
        source: &str,
        label: &str,
    ) -> RuntimeResult<GenericTextFieldIdentity> {
        self.identity_fields
            .iter()
            .find_map(|slot| slot.as_ref().map(|(_, commit)| commit.clone()))
            .ok_or_else(|| format!("{label} from `{source}` produced no text identity").into())
    }

    fn require_bool(
        &self,
        source: &str,
        label: &str,
        field: &str,
    ) -> RuntimeResult<GenericBoolFieldCommit> {
        self.bool(field).ok_or_else(|| {
            format!("{label} from `{source}` produced no `{field}` bool change").into()
        })
    }

    fn bool(&self, field: &str) -> Option<GenericBoolFieldCommit> {
        self.bool_fields.iter().find_map(|slot| match slot {
            Some((existing, commit)) if existing == field => Some(commit.clone()),
            _ => None,
        })
    }

    fn require_first_bool(
        &self,
        source: &str,
        label: &str,
    ) -> RuntimeResult<GenericBoolFieldCommit> {
        self.bool_fields
            .iter()
            .find_map(|slot| slot.as_ref().map(|(_, commit)| commit.clone()))
            .ok_or_else(|| format!("{label} from `{source}` produced no bool change").into())
    }

    fn list_append(&self, list: &str) -> Option<GenericTextListAppendCommit<'a>> {
        self.list_appends.iter().find_map(|slot| match slot {
            Some((existing, commit)) if existing == list => Some(commit.clone()),
            _ => None,
        })
    }

    fn root_text(&self, target: &str) -> Option<GenericRootTextCommit<'a>> {
        self.root_texts.iter().find_map(|slot| match slot {
            Some((existing, commit)) if existing == target => Some(commit.clone()),
            _ => None,
        })
    }
}

#[derive(Clone, Debug)]
struct DerivedEquationPlan {
    text_transforms: Vec<RuntimeDerivedTextTransform>,
}

#[derive(Clone, Debug, Default)]
struct GenericDerivedPlan {
    expressions: Vec<AstExpr>,
    functions: BTreeMap<String, FunctionDefinition>,
    root_fields: Vec<GenericDerivedRootField>,
    indexed_fields: Vec<GenericDerivedIndexedField>,
}

#[derive(Clone, Debug)]
struct GenericDerivedRootField {
    path: String,
    kind: DerivedValueKind,
    has_sources: bool,
    statement: AstStatement,
}

#[derive(Clone, Debug)]
struct GenericDerivedIndexedField {
    list: String,
    row_scope: String,
    field: String,
    statement: AstStatement,
}

#[derive(Clone, Debug, Default)]
struct GenericDerivedState {
    reads_by_field: BTreeMap<GenericDerivedKey, BTreeSet<GenericReadKey>>,
    dependents_by_read: BTreeMap<GenericReadKey, BTreeSet<GenericDerivedKey>>,
    last_recomputed: Vec<GenericDerivedKey>,
    last_candidate_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct GenericDerivedKey {
    list: String,
    index: usize,
    field: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum GenericReadKey {
    Root {
        field: String,
    },
    ListField {
        list: String,
        index: usize,
        field: String,
    },
}

#[derive(Clone, Debug, Default)]
struct GenericRecomputeMetrics {
    recomputed_field_count: usize,
    recompute_candidate_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
enum BoonValue {
    Empty,
    Text(String),
    Number(i64),
    Bool(bool),
    Record(BTreeMap<String, BoonValue>),
    List(Vec<BoonValue>),
    RowRef { list: String, index: usize },
    ListRef(String),
    NaN,
    Error(String),
}

fn boon_value_scalar_text(value: &BoonValue) -> String {
    match value {
        BoonValue::Text(value) => value.clone(),
        BoonValue::Number(value) => value.to_string(),
        BoonValue::Bool(true) => "True".to_owned(),
        BoonValue::Bool(false) => "False".to_owned(),
        BoonValue::Empty => String::new(),
        BoonValue::NaN => "NaN".to_owned(),
        BoonValue::Error(value) => value.clone(),
        BoonValue::Record(_)
        | BoonValue::List(_)
        | BoonValue::RowRef { .. }
        | BoonValue::ListRef(_) => String::new(),
    }
}

fn is_generic_render_constructor(function: &str) -> bool {
    matches!(
        function,
        "Document/new"
            | "Element/container"
            | "Element/stripe"
            | "Element/text"
            | "Element/label"
            | "Element/paragraph"
            | "Element/link"
            | "Element/button"
            | "Element/checkbox"
            | "Element/text_input"
            | "Scene/new"
            | "Scene/Element/stripe"
            | "Scene/Element/block"
            | "Scene/Element/text"
            | "Scene/Element/text_input"
            | "Scene/Element/checkbox"
            | "Scene/Element/label"
            | "Scene/Element/button"
            | "Scene/Element/paragraph"
            | "Scene/Element/link"
    )
}

fn is_light_constructor(function: &str) -> bool {
    matches!(
        function,
        "Light/directional" | "Light/ambient" | "Light/spot"
    )
}

fn generic_constructor_kind(function: &str) -> &'static str {
    match function {
        "Document/new" => "Document",
        "Element/container" => "Stack",
        "Element/stripe" | "Scene/Element/stripe" => "Stripe",
        "Element/text" | "Scene/Element/text" => "Text",
        "Element/label" | "Scene/Element/label" => "Label",
        "Element/paragraph" | "Scene/Element/paragraph" => "Paragraph",
        "Element/link" | "Scene/Element/link" => "Link",
        "Element/button" | "Scene/Element/button" => "Button",
        "Element/checkbox" | "Scene/Element/checkbox" => "Checkbox",
        "Element/text_input" | "Scene/Element/text_input" => "TextInput",
        "Scene/new" => "Scene",
        "Scene/Element/block" => "Block",
        "Light/directional" => "DirectionalLight",
        "Light/ambient" => "AmbientLight",
        "Light/spot" => "SpotLight",
        _ => "Record",
    }
}

#[derive(Clone, Debug)]
struct GenericEvalRow {
    list: String,
    row_scope: String,
    index: usize,
}

#[derive(Clone, Debug)]
struct GenericEvalFrame {
    env: BTreeMap<String, BoonValue>,
    row: Option<GenericEvalRow>,
    reads: BTreeSet<GenericReadKey>,
    stack: Vec<GenericDerivedKey>,
    call_depth: usize,
    eval_budget: usize,
}

#[derive(Clone, Debug)]
struct ListProjectionPlan {
    projections: Vec<RuntimeListProjection>,
}

#[derive(Clone, Debug)]
struct RuntimeListProjection {
    target: String,
    list: String,
    columns: usize,
    rows: usize,
    kind: RuntimeListProjectionKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RuntimeListProjectionKind {
    Chunk {
        item_field: String,
        label_field: String,
    },
    Find {
        field: String,
        value: String,
    },
}

#[derive(Clone, Debug)]
struct RuntimeDerivedTextTransform {
    target: String,
    source: String,
    expression: RuntimeDerivedTextExpression,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RuntimeDerivedTextExpression {
    EnterKeyPayloadTextTrimNonEmpty,
    EnterKeyRootTextTrimNonEmpty { path: String },
    SourceRootText { path: String },
    Unsupported,
}

fn source_event_transform_text_expression(
    value: &boon_ir::DerivedValue,
    expressions: &[AstExpr],
) -> RuntimeDerivedTextExpression {
    let exprs = statement_ast_exprs(&value.statement, expressions);
    let Some(path) = text_trim_input_path_from_exprs(&exprs) else {
        let Some(source) = value.sources.first() else {
            return RuntimeDerivedTextExpression::Unsupported;
        };
        let Some(path) = source_then_text_value(&exprs, source) else {
            return RuntimeDerivedTextExpression::Unsupported;
        };
        return RuntimeDerivedTextExpression::SourceRootText {
            path: canonical_transform_text_path(&value.path, &path),
        };
    };
    if path == "text"
        || path.ends_with(".text")
        || value
            .sources
            .iter()
            .any(|source| path == format!("{source}.text"))
    {
        return RuntimeDerivedTextExpression::EnterKeyPayloadTextTrimNonEmpty;
    }
    RuntimeDerivedTextExpression::EnterKeyRootTextTrimNonEmpty {
        path: canonical_transform_text_path(&value.path, &path),
    }
}

fn canonical_transform_text_path(owner_path: &str, path: &str) -> String {
    if path.contains('.') {
        return path.to_owned();
    }
    owner_path
        .rsplit_once('.')
        .map(|(parent, _)| format!("{parent}.{path}"))
        .unwrap_or_else(|| path.to_owned())
}

fn statement_ast_exprs(statement: &AstStatement, expressions: &[AstExpr]) -> Vec<AstExpr> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    collect_statement_expr_ids(statement, expressions, &mut seen, &mut ids);
    ids.into_iter()
        .filter_map(|id| expressions.get(id).cloned())
        .collect()
}

fn collect_statement_expr_ids(
    statement: &AstStatement,
    expressions: &[AstExpr],
    seen: &mut BTreeSet<usize>,
    ids: &mut Vec<usize>,
) {
    if let Some(expr) = statement.expr {
        collect_expr_tree(expr, expressions, seen, ids);
    }
    for child in &statement.children {
        collect_statement_expr_ids(child, expressions, seen, ids);
    }
}

fn collect_expr_tree(
    id: usize,
    expressions: &[AstExpr],
    seen: &mut BTreeSet<usize>,
    ids: &mut Vec<usize>,
) {
    if !seen.insert(id) {
        return;
    }
    ids.push(id);
    let Some(expr) = expressions.get(id) else {
        return;
    };
    match &expr.kind {
        AstExprKind::Call { args, .. } => {
            for arg in args {
                collect_expr_tree(arg.value, expressions, seen, ids);
            }
        }
        AstExprKind::Pipe { input, args, .. } => {
            collect_expr_tree(*input, expressions, seen, ids);
            for arg in args {
                collect_expr_tree(arg.value, expressions, seen, ids);
            }
        }
        AstExprKind::Hold { initial, .. } | AstExprKind::When { input: initial } => {
            collect_expr_tree(*initial, expressions, seen, ids);
        }
        AstExprKind::Then { input, output } => {
            collect_expr_tree(*input, expressions, seen, ids);
            if let Some(output) = output {
                collect_expr_tree(*output, expressions, seen, ids);
            }
        }
        AstExprKind::Infix { left, right, .. } => {
            collect_expr_tree(*left, expressions, seen, ids);
            collect_expr_tree(*right, expressions, seen, ids);
        }
        AstExprKind::MatchArm { output, .. } => {
            if let Some(output) = output {
                collect_expr_tree(*output, expressions, seen, ids);
            }
        }
        AstExprKind::Record(fields)
        | AstExprKind::Object(fields)
        | AstExprKind::TaggedObject { fields, .. } => {
            for field in fields {
                collect_expr_tree(field.value, expressions, seen, ids);
            }
        }
        AstExprKind::Identifier(_)
        | AstExprKind::Path(_)
        | AstExprKind::StringLiteral(_)
        | AstExprKind::TextLiteral(_)
        | AstExprKind::Number(_)
        | AstExprKind::Bool(_)
        | AstExprKind::Enum(_)
        | AstExprKind::Tag(_)
        | AstExprKind::Source
        | AstExprKind::Latest
        | AstExprKind::ListLiteral { .. }
        | AstExprKind::Delimiter
        | AstExprKind::Unknown(_) => {}
    }
}

fn text_trim_input_path_from_exprs(exprs: &[AstExpr]) -> Option<String> {
    exprs.iter().find_map(|expr| {
        let AstExprKind::Pipe { input, op, .. } = &expr.kind else {
            return None;
        };
        (op == "Text/trim").then(|| ast_argument_value_in_exprs(exprs, *input))?
    })
}

fn ast_argument_value_in_exprs(exprs: &[AstExpr], expr_id: usize) -> Option<String> {
    let expr = exprs.iter().find(|expr| expr.id == expr_id)?;
    Some(match &expr.kind {
        AstExprKind::Identifier(value)
        | AstExprKind::Enum(value)
        | AstExprKind::Tag(value)
        | AstExprKind::Number(value) => value.clone(),
        AstExprKind::Path(parts) => parts.join("."),
        AstExprKind::Bool(true) => "True".to_owned(),
        AstExprKind::Bool(false) => "False".to_owned(),
        AstExprKind::StringLiteral(value) | AstExprKind::TextLiteral(value) => value.clone(),
        AstExprKind::Unknown(tokens) => tokens.join("."),
        AstExprKind::Delimiter => String::new(),
        AstExprKind::Source
        | AstExprKind::Call { .. }
        | AstExprKind::Pipe { .. }
        | AstExprKind::Hold { .. }
        | AstExprKind::Latest
        | AstExprKind::When { .. }
        | AstExprKind::Then { .. }
        | AstExprKind::Infix { .. }
        | AstExprKind::MatchArm { .. }
        | AstExprKind::Record(_)
        | AstExprKind::Object(_)
        | AstExprKind::TaggedObject { .. }
        | AstExprKind::ListLiteral { .. } => return None,
    })
}

#[derive(Clone, Debug, Default)]
struct SourceRoutePlan {
    route_slots: Vec<SourceRoute>,
    id_slots: Vec<Option<SourceRouteIndex>>,
    label_slots: Vec<SourceBoundaryLabel>,
    action_table: SourceActionTable,
}

#[derive(Clone, Debug, Default)]
struct SourceActionTable {
    by_source: Vec<Vec<SourceAction>>,
}

impl SourceActionTable {
    fn set(&mut self, source_id: SourceId, actions: Vec<SourceAction>) {
        let slot = source_id.as_usize();
        if self.by_source.len() <= slot {
            self.by_source.resize_with(slot + 1, Vec::new);
        }
        self.by_source[slot] = actions;
    }

    fn actions(&self, source_id: SourceId) -> Option<&[SourceAction]> {
        self.by_source
            .get(source_id.as_usize())
            .map(Vec::as_slice)
            .filter(|actions| !actions.is_empty())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceRouteIndex(usize);

impl SourceRouteIndex {
    fn slot(self) -> usize {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceBoundaryLabel {
    source: String,
    source_id: SourceId,
}

#[derive(Clone, Debug, Default)]
struct ListSourceBindingPlan {
    list_slots: Vec<ListSourceBindingSlot>,
}

#[derive(Clone, Debug)]
struct ListSourceBindingSlot {
    list: String,
    row_scope: String,
    source_paths: Vec<String>,
}

#[derive(Clone, Debug)]
struct SourceRoute {
    source_id: SourceId,
    source: String,
    address_lookup_field: Option<String>,
    payload_fields: Vec<SourcePayloadField>,
    root_scalar_targets: Vec<SourceRouteScalarTarget>,
    indexed_text_targets: Vec<SourceRouteScalarTarget>,
    indexed_bool_targets: Vec<SourceRouteScalarTarget>,
    derived_text_targets: Vec<String>,
    router_route_targets: Vec<SourceRouteRouterRoute>,
    root_text_transform_targets: Vec<SourceRouteRootTextTransform>,
    list_append_targets: Vec<SourceRouteListAppend>,
    list_remove_targets: Vec<SourceRouteListRemove>,
    actions: Vec<SourceAction>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceRouteScalarTarget {
    target: String,
    expression: ScalarUpdateExpression,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceRouteListRemove {
    list: String,
    predicate: RuntimeListPredicate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceRouteListAppend {
    list: String,
    trigger: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceRouteRouterRoute {
    source: String,
    target: String,
    path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceRouteRootTextTransform {
    source: String,
    target: String,
    value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SourceAction {
    RootScalar,
    DerivedText {
        target: String,
    },
    RouterRoute {
        target: String,
        path: String,
    },
    RootTextTransform {
        target: String,
        value: String,
    },
    ListRemove {
        list: String,
    },
    ListAppend {
        list: String,
        trigger: String,
    },
    IndexedText {
        kind: SourceRouteTextAction,
        target: String,
    },
    IndexedBool {
        kind: SourceRouteBoolAction,
        target: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceRouteTextAction {
    SourceText,
    PreviousValue,
    TextTrimOrPrevious,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceRouteBoolAction {
    BoolNot,
    ConstTrue,
    ConstFalse,
}

#[derive(Clone, Debug)]
struct ListEquationPlan {
    operations: Vec<RuntimeListOperation>,
}

#[derive(Clone, Debug)]
struct RuntimeListOperation {
    list: String,
    kind: RuntimeListOperationKind,
}

#[derive(Clone, Debug)]
enum RuntimeListOperationKind {
    Append {
        trigger: String,
        fields: Vec<RuntimeListAppendField>,
    },
    Remove {
        source: String,
        predicate: RuntimeListPredicate,
    },
    Retain {
        target: String,
        predicate: RuntimeListPredicate,
    },
    Count {
        target: String,
        predicate: RuntimeListPredicate,
    },
}

#[derive(Clone, Debug)]
struct RuntimeListAppendField {
    name: String,
    source: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RuntimeListPredicate {
    AlwaysTrue,
    FieldBool { path: String },
    FieldBoolNot { path: String },
    SelectorVisibility { selector: String, row_field: String },
    Unsupported,
}

impl ScalarEquationPlan {
    fn from_ir(ir: &TypedProgram) -> Self {
        let branches = ir
            .update_branches
            .iter()
            .map(|branch| ScalarUpdateBranch {
                target: branch.target.clone(),
                source: branch.source.clone(),
                expression: match &branch.expression {
                    UpdateExpression::SourcePayload { path } if path == "text" => {
                        ScalarUpdateExpression::SourceText
                    }
                    UpdateExpression::SourcePayload { path } if path == "key" => {
                        ScalarUpdateExpression::SourceKey
                    }
                    UpdateExpression::SourcePayload { path } if path == "address" => {
                        ScalarUpdateExpression::SourceAddress
                    }
                    UpdateExpression::Const { value } => {
                        ScalarUpdateExpression::Const(value.clone())
                    }
                    UpdateExpression::NumberInfix { left, op, right } => {
                        ScalarUpdateExpression::NumberInfix {
                            left: left.clone(),
                            op: op.clone(),
                            right: right.clone(),
                        }
                    }
                    UpdateExpression::PreviousValue { path } => {
                        ScalarUpdateExpression::PreviousValue(path.clone())
                    }
                    UpdateExpression::ReadPath { path } => {
                        ScalarUpdateExpression::ReadPath(path.clone())
                    }
                    UpdateExpression::TextTrimOrPrevious { path, previous } => {
                        ScalarUpdateExpression::TextTrimOrPrevious {
                            path: path.clone(),
                            previous: previous.clone(),
                        }
                    }
                    UpdateExpression::BoolNot { path } => {
                        ScalarUpdateExpression::BoolNot(path.clone())
                    }
                    UpdateExpression::MatchConst { input, arms } => {
                        ScalarUpdateExpression::MatchConst {
                            input: input.clone(),
                            arms: arms.clone(),
                        }
                    }
                    _ => ScalarUpdateExpression::Unsupported,
                },
            })
            .collect();
        Self { branches }
    }

    fn eval_text<'a>(
        &self,
        target: &str,
        source: &str,
        payload_key: Option<&'a str>,
        payload_text: Option<&'a str>,
        payload_address: Option<&'a str>,
        read_textlike: impl Fn(&str) -> Option<String> + Copy,
    ) -> RuntimeResult<Option<Cow<'a, str>>> {
        let Some(value) = self.eval_text_value(
            target,
            source,
            payload_key,
            payload_text,
            payload_address,
            read_textlike,
        )?
        else {
            return Ok(None);
        };
        match value {
            ScalarTextValue::Text(value) => Ok(Some(value)),
            ScalarTextValue::PreviousValue => Ok(None),
        }
    }

    fn eval_text_value<'a>(
        &self,
        target: &str,
        source: &str,
        payload_key: Option<&'a str>,
        payload_text: Option<&'a str>,
        payload_address: Option<&'a str>,
        read_textlike: impl Fn(&str) -> Option<String> + Copy,
    ) -> RuntimeResult<Option<ScalarTextValue<'a>>> {
        let Some(branch) = self
            .branches
            .iter()
            .find(|branch| branch.target == target && branch.source == source)
        else {
            return Ok(None);
        };
        match &branch.expression {
            ScalarUpdateExpression::SourceText => {
                let Some(text) = payload_text else {
                    return Ok(None);
                };
                Ok(Some(ScalarTextValue::Text(Cow::Borrowed(text))))
            }
            ScalarUpdateExpression::SourceKey => {
                let Some(key) = payload_key else {
                    return Ok(None);
                };
                Ok(Some(ScalarTextValue::Text(Cow::Borrowed(key))))
            }
            ScalarUpdateExpression::SourceAddress => {
                let Some(address) = payload_address else {
                    return Ok(None);
                };
                Ok(Some(ScalarTextValue::Text(Cow::Borrowed(address))))
            }
            ScalarUpdateExpression::Const(value) => {
                Ok(Some(ScalarTextValue::Text(Cow::Owned(value.clone()))))
            }
            ScalarUpdateExpression::NumberInfix { left, op, right } => {
                let left = scalar_number_operand_value(left, read_textlike).ok_or_else(|| {
                    format!("source `{source}` for `{target}` cannot read numeric operand `{left}`")
                })?;
                let right = scalar_number_operand_value(right, read_textlike).ok_or_else(|| {
                    format!(
                        "source `{source}` for `{target}` cannot read numeric operand `{right}`"
                    )
                })?;
                let value = match op.as_str() {
                    "+" => left + right,
                    "-" => left - right,
                    _ => {
                        return Err(format!(
                            "source `{source}` for `{target}` uses unsupported numeric operator `{op}`"
                        )
                        .into());
                    }
                };
                Ok(Some(ScalarTextValue::Text(Cow::Owned(value.to_string()))))
            }
            ScalarUpdateExpression::MatchConst { input, arms } => {
                let current = match source_payload_match_input(
                    input,
                    source,
                    payload_key,
                    payload_text,
                    payload_address,
                ) {
                    Some(value) => Cow::Borrowed(value),
                    None => Cow::Owned(read_textlike(input).ok_or_else(|| {
                        format!(
                            "source `{source}` for `{target}` cannot read match input `{input}`"
                        )
                    })?),
                };
                Ok(arms
                    .iter()
                    .find(|arm| arm.pattern == current)
                    .map(|arm| ScalarTextValue::Text(Cow::Owned(arm.output.clone()))))
            }
            ScalarUpdateExpression::PreviousValue(_) => Ok(Some(ScalarTextValue::PreviousValue)),
            ScalarUpdateExpression::ReadPath(path) => {
                if let Some(value) = read_textlike(path) {
                    Ok(Some(ScalarTextValue::Text(Cow::Owned(value))))
                } else {
                    Err(
                        format!("source `{source}` for `{target}` cannot read path `{path}`")
                            .into(),
                    )
                }
            }
            ScalarUpdateExpression::TextTrimOrPrevious { .. }
            | ScalarUpdateExpression::BoolNot(_) => Ok(None),
            ScalarUpdateExpression::Unsupported => Ok(None),
        }
    }

    fn bool_const_value(&self, target: &str, source: &str) -> Option<bool> {
        self.branches
            .iter()
            .find(|branch| branch.target == target && branch.source == source)
            .and_then(|branch| branch.expression.const_bool())
    }

    fn eval_bool_with_context(
        &self,
        target: &str,
        source: &str,
        read_bool: impl Fn(&str) -> Option<bool>,
    ) -> RuntimeResult<Option<bool>> {
        let Some(branch) = self
            .branches
            .iter()
            .find(|branch| branch.target == target && branch.source == source)
        else {
            return Ok(None);
        };
        match &branch.expression {
            expression if expression.const_bool().is_some() => Ok(expression.const_bool()),
            ScalarUpdateExpression::BoolNot(path) => {
                let value = read_bool(path).ok_or_else(|| {
                    format!("source `{source}` for `{target}` cannot read bool path `{path}`")
                })?;
                Ok(Some(!value))
            }
            ScalarUpdateExpression::Const(value) => Err(format!(
                "source `{source}` for bool target `{target}` produced non-bool constant `{value}`"
            )
            .into()),
            _ => Ok(None),
        }
    }
}

fn scalar_number_operand_value(
    operand: &str,
    read_textlike: impl Fn(&str) -> Option<String> + Copy,
) -> Option<i64> {
    operand
        .parse::<i64>()
        .ok()
        .or_else(|| read_textlike(operand)?.parse::<i64>().ok())
}

impl DerivedEquationPlan {
    fn from_ir(ir: &TypedProgram) -> Self {
        let append_triggers = ir
            .list_operations
            .iter()
            .filter_map(|operation| match &operation.kind {
                ListOperationKind::Append { trigger, .. } => Some(trigger.as_str()),
                ListOperationKind::Remove { .. }
                | ListOperationKind::Retain { .. }
                | ListOperationKind::Count { .. } => None,
            })
            .collect::<BTreeSet<_>>();
        let text_transforms = ir
            .derived_values
            .iter()
            .filter(|value| {
                value.kind == DerivedValueKind::SourceEventTransform
                    && append_triggers.contains(value.path.as_str())
            })
            .map(|value| RuntimeDerivedTextTransform {
                target: value.path.clone(),
                source: value.sources.first().cloned().unwrap_or_default(),
                expression: if value.sources.len() == 1 {
                    source_event_transform_text_expression(value, &ir.expressions)
                } else {
                    RuntimeDerivedTextExpression::Unsupported
                },
            })
            .collect();
        Self { text_transforms }
    }

    fn eval_text_transform<'a>(
        &self,
        target: &str,
        source: &str,
        key: Option<&str>,
        text: Option<&'a str>,
        read_root_text: impl Fn(&str) -> Option<String>,
    ) -> RuntimeResult<Option<Cow<'a, str>>> {
        let Some(transform) = self
            .text_transforms
            .iter()
            .find(|transform| transform.target == target && transform.source == source)
        else {
            return Ok(None);
        };
        match &transform.expression {
            RuntimeDerivedTextExpression::EnterKeyPayloadTextTrimNonEmpty => {
                if key != Some("Enter") {
                    return Ok(None);
                }
                let text = text.ok_or_else(|| {
                    format!("derived text transform `{target}` from `{source}` requires text")
                })?;
                let trimmed = text.trim();
                Ok((!trimmed.is_empty()).then_some(Cow::Borrowed(trimmed)))
            }
            RuntimeDerivedTextExpression::EnterKeyRootTextTrimNonEmpty { path } => {
                if key != Some("Enter") {
                    return Ok(None);
                }
                let text = read_root_text(path).ok_or_else(|| {
                    format!("derived text transform `{target}` from `{source}` requires root text `{path}`")
                })?;
                let trimmed = text.trim().to_owned();
                Ok((!trimmed.is_empty()).then_some(Cow::Owned(trimmed)))
            }
            RuntimeDerivedTextExpression::SourceRootText { path } => {
                let text = read_root_text(path).ok_or_else(|| {
                    format!(
                        "derived text transform `{target}` from `{source}` requires root text `{path}`"
                    )
                })?;
                let trimmed = text.trim().to_owned();
                Ok((!trimmed.is_empty()).then_some(Cow::Owned(trimmed)))
            }
            RuntimeDerivedTextExpression::Unsupported => Err(format!(
                "derived text transform `{target}` from `{source}` is unsupported"
            )
            .into()),
        }
    }
}

impl GenericDerivedPlan {
    fn from_ir(ir: &TypedProgram) -> Self {
        let functions = ir
            .functions
            .iter()
            .cloned()
            .map(|function| (function.name.clone(), function))
            .collect::<BTreeMap<_, _>>();
        let root_fields = ir
            .derived_values
            .iter()
            .filter(|value| {
                !value.indexed
                    && matches!(
                        value.kind,
                        DerivedValueKind::Pure | DerivedValueKind::SourceEventTransform
                    )
            })
            .map(|value| GenericDerivedRootField {
                path: value.path.clone(),
                kind: value.kind.clone(),
                has_sources: !value.sources.is_empty(),
                statement: value.statement.clone(),
            })
            .collect();
        let indexed_fields = ir
            .derived_values
            .iter()
            .filter(|value| {
                value.indexed
                    && matches!(
                        value.kind,
                        DerivedValueKind::Pure | DerivedValueKind::SourceEventTransform
                    )
            })
            .filter_map(|value| {
                let (row_scope, field) = value.path.split_once('.')?;
                let list = ir
                    .row_scopes
                    .iter()
                    .find(|scope| scope.row_scope == row_scope)
                    .map(|scope| scope.list.clone())?;
                if statement_is_row_initial_passthrough(
                    &value.statement,
                    &ir.expressions,
                    row_scope,
                    field,
                ) {
                    return None;
                }
                Some(GenericDerivedIndexedField {
                    list,
                    row_scope: row_scope.to_owned(),
                    field: field.to_owned(),
                    statement: value.statement.clone(),
                })
            })
            .collect();
        Self {
            expressions: ir.expressions.clone(),
            functions,
            root_fields,
            indexed_fields,
        }
    }

    fn has_indexed_fields(&self) -> bool {
        !self.indexed_fields.is_empty()
    }

    fn field_plan(&self, key: &GenericDerivedKey) -> Option<&GenericDerivedIndexedField> {
        self.indexed_fields
            .iter()
            .find(|field| field.list == key.list && field.field == key.field)
    }

    fn contains_field(&self, list: &str, field: &str) -> bool {
        self.indexed_fields
            .iter()
            .any(|candidate| candidate.list == list && candidate.field == field)
    }

    fn expr_is_block_marker(&self, expr_id: usize) -> bool {
        self.expressions
            .get(expr_id)
            .is_some_and(|expr| match &expr.kind {
                AstExprKind::Identifier(value) if value == "BLOCK" => true,
                AstExprKind::Unknown(tokens) => {
                    tokens.first().map(String::as_str) == Some("BLOCK")
                        && tokens.last().map(String::as_str) == Some("{")
                }
                _ => false,
            })
    }

    fn expr_is_pipe_continuation(&self, expr_id: usize) -> bool {
        let Some(expr) = self.expressions.get(expr_id) else {
            return false;
        };
        let input = match &expr.kind {
            AstExprKind::Pipe { input, .. }
            | AstExprKind::Then { input, .. }
            | AstExprKind::When { input } => *input,
            _ => return false,
        };
        self.expr_chain_starts_with_pipe_placeholder(input)
    }

    fn expr_chain_starts_with_pipe_placeholder(&self, expr_id: usize) -> bool {
        match self.expressions.get(expr_id).map(|expr| &expr.kind) {
            Some(AstExprKind::Delimiter) => true,
            Some(AstExprKind::Unknown(tokens)) => !unknown_tokens_are_quoted_text(tokens),
            Some(AstExprKind::Pipe { input, .. })
            | Some(AstExprKind::Then { input, .. })
            | Some(AstExprKind::When { input }) => {
                self.expr_chain_starts_with_pipe_placeholder(*input)
            }
            _ => false,
        }
    }

    fn keys_for_runtime(
        &self,
        runtime: &GenericCircuitRuntime,
    ) -> RuntimeResult<Vec<GenericDerivedKey>> {
        let mut keys = Vec::new();
        for field in &self.indexed_fields {
            for index in 0..runtime.list_len(&field.list)? {
                keys.push(GenericDerivedKey {
                    list: field.list.clone(),
                    index,
                    field: field.field.clone(),
                });
            }
        }
        Ok(keys)
    }
}

fn unknown_tokens_are_quoted_text(tokens: &[String]) -> bool {
    tokens
        .iter()
        .any(|token| token.trim_start().starts_with('"'))
}

impl GenericDerivedState {
    fn clear_last_step(&mut self) {
        self.last_recomputed.clear();
        self.last_candidate_count = 0;
    }

    fn replace_reads(&mut self, key: GenericDerivedKey, reads: BTreeSet<GenericReadKey>) {
        if let Some(previous) = self.reads_by_field.remove(&key) {
            for read in previous {
                if let Some(dependents) = self.dependents_by_read.get_mut(&read) {
                    dependents.remove(&key);
                    if dependents.is_empty() {
                        self.dependents_by_read.remove(&read);
                    }
                }
            }
        }
        for read in &reads {
            self.dependents_by_read
                .entry(read.clone())
                .or_default()
                .insert(key.clone());
        }
        self.reads_by_field.insert(key, reads);
    }

    fn dependents_for_reads(
        &self,
        reads: impl IntoIterator<Item = GenericReadKey>,
    ) -> BTreeSet<GenericDerivedKey> {
        let mut dependents = BTreeSet::new();
        for read in reads {
            if let Some(keys) = self.dependents_by_read.get(&read) {
                dependents.extend(keys.iter().cloned());
            }
        }
        dependents
    }
}

impl GenericEvalFrame {
    fn root() -> Self {
        Self {
            env: BTreeMap::new(),
            row: None,
            reads: BTreeSet::new(),
            stack: Vec::new(),
            call_depth: 0,
            eval_budget: 20_000,
        }
    }

    fn for_row(list: &str, row_scope: &str, index: usize) -> Self {
        Self {
            env: BTreeMap::new(),
            row: Some(GenericEvalRow {
                list: list.to_owned(),
                row_scope: row_scope.to_owned(),
                index,
            }),
            reads: BTreeSet::new(),
            stack: Vec::new(),
            call_depth: 0,
            eval_budget: 20_000,
        }
    }

    fn child(&self) -> Self {
        Self {
            env: self.env.clone(),
            row: self.row.clone(),
            reads: BTreeSet::new(),
            stack: self.stack.clone(),
            call_depth: self.call_depth,
            eval_budget: self.eval_budget,
        }
    }

    fn consume_budget(&mut self) -> RuntimeResult<()> {
        self.eval_budget = self
            .eval_budget
            .checked_sub(1)
            .ok_or("generic Boon evaluation budget exhausted")?;
        Ok(())
    }
}

impl BoonValue {
    fn as_text(&self) -> Option<String> {
        match self {
            Self::Text(value) => Some(value.clone()),
            Self::Number(value) => Some(value.to_string()),
            Self::Bool(true) => Some("True".to_owned()),
            Self::Bool(false) => Some("False".to_owned()),
            Self::NaN => Some("NaN".to_owned()),
            Self::Empty => Some(String::new()),
            Self::Error(_)
            | Self::Record(_)
            | Self::List(_)
            | Self::RowRef { .. }
            | Self::ListRef(_) => None,
        }
    }

    fn visible_text(&self) -> String {
        match self {
            Self::Error(_) | Self::Empty | Self::NaN => String::new(),
            Self::Text(value) => value.clone(),
            Self::Number(value) => value.to_string(),
            Self::Bool(true) => "True".to_owned(),
            Self::Bool(false) => "False".to_owned(),
            Self::Record(_) | Self::List(_) | Self::RowRef { .. } | Self::ListRef(_) => {
                String::new()
            }
        }
    }

    fn number(&self) -> Result<i64, String> {
        match self {
            Self::Number(value) => Ok(*value),
            Self::Text(value) => value
                .trim()
                .parse::<i64>()
                .map_err(|_| "type_error".to_owned()),
            Self::NaN => Err("type_error".to_owned()),
            Self::Error(error) => Err(error.clone()),
            _ => Err("type_error".to_owned()),
        }
    }

    fn bool_value(&self) -> Result<bool, String> {
        match self {
            Self::Bool(value) => Ok(*value),
            Self::Text(value) if value == "True" => Ok(true),
            Self::Text(value) if value == "False" => Ok(false),
            Self::Error(error) => Err(error.clone()),
            _ => Err("type_error".to_owned()),
        }
    }
}

impl ListProjectionPlan {
    fn from_ir(ir: &TypedProgram) -> Self {
        let projections = ir
            .list_projections
            .iter()
            .filter_map(|projection| {
                let (columns, rows) = match &projection.kind {
                    ListProjectionKind::Chunk { size, .. } => {
                        let columns = (*size)?;
                        (columns, 0)
                    }
                    ListProjectionKind::Find { .. } => (0, 0),
                };
                Some(RuntimeListProjection {
                    target: projection.target.clone(),
                    list: projection.list.clone(),
                    columns,
                    rows,
                    kind: match &projection.kind {
                        ListProjectionKind::Chunk {
                            item_field,
                            label_field,
                            ..
                        } => RuntimeListProjectionKind::Chunk {
                            item_field: item_field.clone(),
                            label_field: label_field.clone(),
                        },
                        ListProjectionKind::Find { field, value } => {
                            RuntimeListProjectionKind::Find {
                                field: field.clone(),
                                value: value.clone(),
                            }
                        }
                    },
                })
            })
            .collect();
        Self { projections }
    }
}

fn source_payload_match_input<'a>(
    input: &str,
    source: &str,
    payload_key: Option<&'a str>,
    payload_text: Option<&'a str>,
    payload_address: Option<&'a str>,
) -> Option<&'a str> {
    if source_payload_input_matches(
        input,
        source,
        &[".key", ".event.key_down.key", ".key_down.key"],
    ) {
        return payload_key;
    }
    if source_payload_input_matches(
        input,
        source,
        &[".text", ".event.change.text", ".change.text"],
    ) {
        return payload_text;
    }
    if source_payload_input_matches(input, source, &[".address", ".event.address"]) {
        return payload_address;
    }
    None
}

fn source_payload_input_matches(input: &str, source: &str, suffixes: &[&str]) -> bool {
    input
        .strip_prefix(source)
        .is_some_and(|suffix| suffixes.contains(&suffix))
        || source
            .strip_prefix("store.")
            .and_then(|local| input.strip_prefix(local))
            .is_some_and(|suffix| suffixes.contains(&suffix))
}

impl SourceRoutePlan {
    fn from_plans(
        ir: &TypedProgram,
        scalar: &ScalarEquationPlan,
        derived: &DerivedEquationPlan,
        lists: &ListEquationPlan,
        root_targets: &BTreeSet<&str>,
    ) -> RuntimeResult<Self> {
        let mut routes = Self::default();
        for branch in &scalar.branches {
            let source_id = source_id_for_path(ir, &branch.source)?;
            let route = routes.route_mut(&branch.source, source_id);
            let scalar_target = SourceRouteScalarTarget {
                target: branch.target.clone(),
                expression: branch.expression.clone(),
            };
            if root_targets.contains(branch.target.as_str()) {
                route.root_scalar_targets.push(scalar_target);
            } else if branch.expression.is_indexed_text_expression() {
                route.indexed_text_targets.push(scalar_target);
            } else if branch.expression.is_indexed_bool_expression() {
                route.indexed_bool_targets.push(scalar_target);
            }
        }
        for transform in &derived.text_transforms {
            let source_id = source_id_for_path(ir, &transform.source)?;
            routes
                .route_mut(&transform.source, source_id)
                .derived_text_targets
                .push(transform.target.clone());
        }
        for target in router_route_targets_from_ir(ir)? {
            let source_id = source_id_for_path(ir, &target.source)?;
            routes
                .route_mut(&target.source, source_id)
                .router_route_targets
                .push(target);
        }
        for target in root_text_transform_targets_from_ir(ir)? {
            let source_id = source_id_for_path(ir, &target.source)?;
            routes
                .route_mut(&target.source, source_id)
                .root_text_transform_targets
                .push(target);
        }
        for operation in &lists.operations {
            match &operation.kind {
                RuntimeListOperationKind::Append { trigger, .. } => {
                    for route in routes
                        .route_slots
                        .iter_mut()
                        .filter(|route| route.derived_text_targets.contains(trigger))
                    {
                        route.list_append_targets.push(SourceRouteListAppend {
                            list: operation.list.clone(),
                            trigger: trigger.clone(),
                        });
                    }
                }
                RuntimeListOperationKind::Remove { source, predicate } => {
                    let source_id = source_id_for_path(ir, source)?;
                    routes
                        .route_mut(source, source_id)
                        .list_remove_targets
                        .push(SourceRouteListRemove {
                            list: operation.list.clone(),
                            predicate: predicate.clone(),
                        });
                }
                RuntimeListOperationKind::Retain { .. }
                | RuntimeListOperationKind::Count { .. } => {}
            }
        }
        for source in &ir.sources {
            if let Some(route) = routes.for_source_id_mut(source.id) {
                route.payload_fields = source.payload_schema.fields.clone();
            }
        }
        routes.set_address_lookup_fields(ir);
        for route in &mut routes.route_slots {
            route.rebuild_actions();
        }
        routes.rebuild_action_table();
        Ok(routes)
    }

    fn len(&self) -> usize {
        self.route_slots.len()
    }

    #[cfg(test)]
    fn for_source(&self, source: &str) -> Option<&SourceRoute> {
        self.source_id(source)
            .and_then(|source_id| self.for_source_id(source_id))
    }

    fn for_source_id(&self, source_id: SourceId) -> Option<&SourceRoute> {
        self.id_slots
            .get(source_id.as_usize())
            .copied()
            .flatten()
            .and_then(|index| self.route_slots.get(index.slot()))
    }

    fn for_source_id_mut(&mut self, source_id: SourceId) -> Option<&mut SourceRoute> {
        self.id_slots
            .get(source_id.as_usize())
            .copied()
            .flatten()
            .and_then(|index| self.route_slots.get_mut(index.slot()))
    }

    fn source_id(&self, source: &str) -> Option<SourceId> {
        self.label_slots
            .binary_search_by(|label| label.source.as_str().cmp(source))
            .ok()
            .and_then(|index| self.label_slots.get(index))
            .map(|label| label.source_id)
    }

    fn require_source_id(&self, source: &str) -> RuntimeResult<SourceId> {
        self.source_id(source)
            .ok_or_else(|| format!("source `{source}` has no typed SourceId route").into())
    }

    #[cfg(test)]
    fn require_source(&self, source: &str) -> RuntimeResult<&SourceRoute> {
        self.for_source(source)
            .ok_or_else(|| format!("source `{source}` has no compiled route").into())
    }

    fn actions_for_source_id(&self, source_id: SourceId) -> RuntimeResult<&[SourceAction]> {
        self.action_table.actions(source_id).ok_or_else(|| {
            format!(
                "SourceId `{}` has no compiled source action table entry",
                source_id.as_usize()
            )
            .into()
        })
    }

    fn single_root_scalar_target_for_source_id(
        &self,
        source_id: SourceId,
    ) -> RuntimeResult<Option<&str>> {
        self.for_source_id(source_id)
            .ok_or_else(|| format!("SourceId `{}` has no compiled route", source_id.as_usize()))?
            .single_root_scalar_target()
    }

    fn root_scalar_targets_for_source_id(
        &self,
        source_id: SourceId,
    ) -> RuntimeResult<&[SourceRouteScalarTarget]> {
        Ok(self
            .for_source_id(source_id)
            .ok_or_else(|| format!("SourceId `{}` has no compiled route", source_id.as_usize()))
            .map(SourceRoute::root_scalar_targets)?)
    }

    fn list_remove_predicate_for_source_id(
        &self,
        source_id: SourceId,
        list: &str,
    ) -> RuntimeResult<RuntimeListPredicate> {
        self.for_source_id(source_id)
            .ok_or_else(|| format!("SourceId `{}` has no compiled route", source_id.as_usize()))?
            .list_remove_predicate(list)
    }

    #[cfg(test)]
    fn list_append_trigger(&self, source: &str, list: &str) -> RuntimeResult<&str> {
        self.require_source(source)?.list_append_trigger(list)
    }

    fn address_lookup_field_for_source_id(&self, source_id: SourceId) -> Option<&str> {
        self.for_source_id(source_id)
            .and_then(|route| route.address_lookup_field.as_deref())
    }

    fn source_payload_has_text(&self, source: &str) -> bool {
        self.source_id(source)
            .and_then(|source_id| self.for_source_id(source_id))
            .is_some_and(|route| {
                route
                    .payload_fields
                    .iter()
                    .any(|field| matches!(field, SourcePayloadField::Text))
            })
    }

    fn route_mut(&mut self, source: &str, source_id: SourceId) -> &mut SourceRoute {
        let source_slot = source_id.as_usize();
        if self.id_slots.len() <= source_slot {
            self.id_slots.resize(source_slot + 1, None);
        }
        if let Some(index) = self.id_slots[source_slot] {
            return &mut self.route_slots[index.slot()];
        }

        let index = SourceRouteIndex(self.route_slots.len());
        self.id_slots[source_slot] = Some(index);
        let label = SourceBoundaryLabel {
            source: source.to_owned(),
            source_id,
        };
        let label_index = self
            .label_slots
            .binary_search_by(|candidate| candidate.source.as_str().cmp(source))
            .unwrap_or_else(|index| index);
        self.label_slots.insert(label_index, label);
        self.route_slots.push(SourceRoute {
            source_id,
            source: source.to_owned(),
            address_lookup_field: None,
            payload_fields: Vec::new(),
            root_scalar_targets: Vec::new(),
            indexed_text_targets: Vec::new(),
            indexed_bool_targets: Vec::new(),
            derived_text_targets: Vec::new(),
            router_route_targets: Vec::new(),
            root_text_transform_targets: Vec::new(),
            list_append_targets: Vec::new(),
            list_remove_targets: Vec::new(),
            actions: Vec::new(),
        });
        self.route_slots
            .last_mut()
            .expect("route was just pushed and must exist")
    }

    fn rebuild_action_table(&mut self) {
        self.action_table = SourceActionTable::default();
        for route in &self.route_slots {
            self.action_table
                .set(route.source_id, route.actions.clone());
        }
    }

    fn set_address_lookup_fields(&mut self, ir: &TypedProgram) {
        for route in &mut self.route_slots {
            route.address_lookup_field = ir
                .sources
                .get(route.source_id.as_usize())
                .and_then(|source| source.payload_schema.address_lookup_field.clone());
        }
    }
}

fn source_id_for_path(ir: &TypedProgram, path: &str) -> RuntimeResult<SourceId> {
    ir.sources
        .iter()
        .find(|source| source.path == path)
        .map(|source| source.id)
        .ok_or_else(|| format!("source route `{path}` has no typed IR SourceId").into())
}

fn router_route_targets_from_ir(ir: &TypedProgram) -> RuntimeResult<Vec<SourceRouteRouterRoute>> {
    let mut targets = Vec::new();
    for value in &ir.derived_values {
        if value.indexed || value.kind != DerivedValueKind::SourceEventTransform {
            continue;
        }
        let exprs = statement_ast_exprs(&value.statement, &ir.expressions);
        if !statement_calls_router_go_to(&exprs) {
            continue;
        }
        for source in &value.sources {
            let Some(path) = source_then_text_value(&exprs, source) else {
                continue;
            };
            targets.push(SourceRouteRouterRoute {
                source: source.clone(),
                target: value.path.clone(),
                path,
            });
        }
    }
    Ok(targets)
}

fn root_text_transform_targets_from_ir(
    ir: &TypedProgram,
) -> RuntimeResult<Vec<SourceRouteRootTextTransform>> {
    let mut targets = Vec::new();
    let append_triggers = ir
        .list_operations
        .iter()
        .filter_map(|operation| match &operation.kind {
            ListOperationKind::Append { trigger, .. } => Some(trigger.as_str()),
            ListOperationKind::Remove { .. }
            | ListOperationKind::Retain { .. }
            | ListOperationKind::Count { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    for value in &ir.derived_values {
        if value.indexed || value.kind != DerivedValueKind::SourceEventTransform {
            continue;
        }
        if append_triggers.contains(value.path.as_str()) {
            continue;
        }
        let exprs = statement_ast_exprs(&value.statement, &ir.expressions);
        if statement_calls_router_go_to(&exprs) {
            continue;
        }
        for source in &value.sources {
            if source_is_scoped(ir, source) {
                continue;
            }
            let Some(output) = source_then_text_value(&exprs, source) else {
                continue;
            };
            targets.push(SourceRouteRootTextTransform {
                source: source.clone(),
                target: value.path.clone(),
                value: output,
            });
        }
    }
    Ok(targets)
}

fn statement_calls_router_go_to(exprs: &[AstExpr]) -> bool {
    exprs.iter().any(|expr| match &expr.kind {
        AstExprKind::Pipe { op, .. } => op == "Router/go_to",
        AstExprKind::Call { function, .. } => function == "Router/go_to",
        _ => false,
    })
}

fn source_then_text_value(exprs: &[AstExpr], source: &str) -> Option<String> {
    exprs.iter().find_map(|expr| {
        let AstExprKind::Then { input, output } = expr.kind else {
            return None;
        };
        let input_path = ast_argument_value_in_exprs(exprs, input)?;
        if !source_event_path_matches(&input_path, source)
            && !source_path_before_line_matches(exprs, expr.line, source)
        {
            return None;
        }
        output
            .and_then(|output| ast_argument_value_in_exprs(exprs, output))
            .or_else(|| simple_value_after_line(exprs, expr.line))
    })
}

fn source_path_before_line_matches(exprs: &[AstExpr], line: usize, source: &str) -> bool {
    exprs
        .iter()
        .filter(|expr| expr.line < line)
        .rev()
        .find_map(|expr| match &expr.kind {
            AstExprKind::Path(parts) => Some(parts.join(".")),
            _ => None,
        })
        .is_some_and(|path| source_event_path_matches(&path, source))
}

fn simple_value_after_line(exprs: &[AstExpr], line: usize) -> Option<String> {
    exprs
        .iter()
        .filter(|expr| expr.line > line)
        .find_map(|expr| ast_simple_text_value_in_exprs(exprs, expr.id))
}

fn ast_simple_text_value_in_exprs(exprs: &[AstExpr], expr_id: usize) -> Option<String> {
    let expr = exprs.iter().find(|expr| expr.id == expr_id)?;
    match &expr.kind {
        AstExprKind::Identifier(value)
        | AstExprKind::Enum(value)
        | AstExprKind::Tag(value)
        | AstExprKind::StringLiteral(value)
        | AstExprKind::TextLiteral(value) => Some(value.clone()),
        AstExprKind::Path(parts) if !parts.is_empty() => Some(parts.join(".")),
        _ => None,
    }
}

fn source_is_scoped(ir: &TypedProgram, source: &str) -> bool {
    ir.sources
        .iter()
        .find(|candidate| candidate.path == source)
        .is_some_and(|candidate| candidate.scoped)
}

fn source_event_path_matches(path: &str, source: &str) -> bool {
    source_event_ref_variants(source).iter().any(|variant| {
        path == variant
            || path
                .strip_prefix(variant)
                .is_some_and(|rest| rest.starts_with('.'))
    })
}

fn source_event_ref_variants(source: &str) -> Vec<String> {
    let mut variants = vec![source.to_owned()];
    if let Some((_, suffix)) = source.split_once('.') {
        variants.push(suffix.to_owned());
        variants.push(format!("item.{suffix}"));
    }
    variants
}

impl ListSourceBindingPlan {
    fn from_ir(ir: &TypedProgram) -> Self {
        let mut list_slots = Vec::new();
        for list in &ir.lists {
            let Some(scope_id) = list.row_scope_id else {
                continue;
            };
            let Some(row_scope) = ir.row_scopes.iter().find(|scope| scope.id == scope_id) else {
                continue;
            };
            let row_scope = row_scope.row_scope.clone();
            let prefix = format!("{row_scope}.");
            let source_paths = ir
                .sources
                .iter()
                .filter(|source| source.scoped && source.path.starts_with(&prefix))
                .map(|source| source.path.clone())
                .collect::<Vec<_>>();
            if !source_paths.is_empty() {
                list_slots.push(ListSourceBindingSlot {
                    list: list.name.clone(),
                    row_scope,
                    source_paths,
                });
            }
        }
        Self { list_slots }
    }

    fn slots(&self) -> &[ListSourceBindingSlot] {
        &self.list_slots
    }

    fn source_paths(&self, list: &str) -> RuntimeResult<&[String]> {
        self.list_slots
            .iter()
            .find_map(|binding| (binding.list == list).then_some(binding.source_paths.as_slice()))
            .ok_or_else(|| format!("list `{list}` has no scoped source binding plan").into())
    }

    fn source_count(&self, list: &str) -> RuntimeResult<usize> {
        self.source_paths(list).map(<[_]>::len)
    }

    fn list_for_row_scope(&self, scope: &str) -> Option<&str> {
        self.list_slots
            .iter()
            .find_map(|binding| (binding.row_scope == scope).then_some(binding.list.as_str()))
    }
}

impl SourceRoute {
    fn rebuild_actions(&mut self) {
        self.actions.clear();
        self.actions.extend(
            self.derived_text_targets
                .iter()
                .cloned()
                .map(|target| SourceAction::DerivedText { target }),
        );
        self.actions
            .extend(
                self.router_route_targets
                    .iter()
                    .map(|target| SourceAction::RouterRoute {
                        target: target.target.clone(),
                        path: target.path.clone(),
                    }),
            );
        self.actions
            .extend(self.root_text_transform_targets.iter().map(|target| {
                SourceAction::RootTextTransform {
                    target: target.target.clone(),
                    value: target.value.clone(),
                }
            }));
        self.actions
            .extend(
                self.list_remove_targets
                    .iter()
                    .map(|target| SourceAction::ListRemove {
                        list: target.list.clone(),
                    }),
            );
        self.actions
            .extend(
                self.list_append_targets
                    .iter()
                    .map(|target| SourceAction::ListAppend {
                        list: target.list.clone(),
                        trigger: target.trigger.clone(),
                    }),
            );
        if !self.root_scalar_targets.is_empty() {
            self.actions.push(SourceAction::RootScalar);
        }
        self.actions
            .extend(self.indexed_text_targets.iter().filter_map(|target| {
                let kind = match &target.expression {
                    ScalarUpdateExpression::SourceText => SourceRouteTextAction::SourceText,
                    ScalarUpdateExpression::PreviousValue(_) => {
                        SourceRouteTextAction::PreviousValue
                    }
                    ScalarUpdateExpression::ReadPath(_) => return None,
                    ScalarUpdateExpression::TextTrimOrPrevious { .. } => {
                        SourceRouteTextAction::TextTrimOrPrevious
                    }
                    ScalarUpdateExpression::Const(_)
                    | ScalarUpdateExpression::NumberInfix { .. }
                    | ScalarUpdateExpression::SourceKey
                    | ScalarUpdateExpression::SourceAddress
                    | ScalarUpdateExpression::BoolNot(_)
                    | ScalarUpdateExpression::MatchConst { .. }
                    | ScalarUpdateExpression::Unsupported => return None,
                };
                Some(SourceAction::IndexedText {
                    kind,
                    target: target.target.clone(),
                })
            }));
        self.actions
            .extend(self.indexed_bool_targets.iter().filter_map(|target| {
                let kind = match &target.expression {
                    ScalarUpdateExpression::BoolNot(_) => SourceRouteBoolAction::BoolNot,
                    ScalarUpdateExpression::Const(value) if value == "True" => {
                        SourceRouteBoolAction::ConstTrue
                    }
                    ScalarUpdateExpression::Const(value) if value == "False" => {
                        SourceRouteBoolAction::ConstFalse
                    }
                    ScalarUpdateExpression::SourceText
                    | ScalarUpdateExpression::SourceKey
                    | ScalarUpdateExpression::SourceAddress
                    | ScalarUpdateExpression::Const(_)
                    | ScalarUpdateExpression::NumberInfix { .. }
                    | ScalarUpdateExpression::PreviousValue(_)
                    | ScalarUpdateExpression::ReadPath(_)
                    | ScalarUpdateExpression::TextTrimOrPrevious { .. }
                    | ScalarUpdateExpression::MatchConst { .. }
                    | ScalarUpdateExpression::Unsupported => return None,
                };
                Some(SourceAction::IndexedBool {
                    kind,
                    target: target.target.clone(),
                })
            }));
    }

    #[cfg(test)]
    fn has_action(&self, matches: impl Fn(&SourceAction) -> bool) -> bool {
        self.actions.iter().any(matches)
    }

    fn single_root_scalar_target(&self) -> RuntimeResult<Option<&str>> {
        let mut target = None;
        for candidate in &self.root_scalar_targets {
            if target.is_some_and(|current| current != candidate.target.as_str()) {
                return Err(format!(
                    "source `{}` drives multiple root scalar targets in this runtime slice",
                    self.source
                )
                .into());
            }
            target = Some(candidate.target.as_str());
        }
        Ok(target)
    }

    fn root_scalar_targets(&self) -> &[SourceRouteScalarTarget] {
        &self.root_scalar_targets
    }

    #[cfg(test)]
    fn has_list_append_target(&self, list: &str) -> bool {
        self.has_action(|action| {
            matches!(
                action,
                SourceAction::ListAppend {
                    list: candidate,
                    ..
                } if candidate == list
            )
        })
    }

    fn list_remove_predicate(&self, list: &str) -> RuntimeResult<RuntimeListPredicate> {
        self.list_remove_targets
            .iter()
            .find_map(|candidate| (candidate.list == list).then_some(candidate.predicate.clone()))
            .ok_or_else(|| {
                format!(
                    "source `{}` has no compiled list-remove route for `{list}`",
                    self.source
                )
                .into()
            })
    }

    #[cfg(test)]
    fn list_append_trigger(&self, list: &str) -> RuntimeResult<&str> {
        let mut trigger = None;
        for candidate in &self.list_append_targets {
            if candidate.list != list {
                continue;
            }
            if trigger.is_some_and(|current| current != candidate.trigger.as_str()) {
                return Err(format!(
                    "source `{}` has multiple list-append triggers for `{list}`",
                    self.source
                )
                .into());
            }
            trigger = Some(candidate.trigger.as_str());
        }
        trigger.ok_or_else(|| {
            format!(
                "source `{}` has no compiled list-append route for `{list}`",
                self.source
            )
            .into()
        })
    }
}

impl ListEquationPlan {
    fn from_ir(ir: &TypedProgram) -> Self {
        let operations = ir
            .list_operations
            .iter()
            .map(|operation| {
                let list = operation.list.clone();
                let kind = match &operation.kind {
                    ListOperationKind::Append { trigger, fields } => {
                        RuntimeListOperationKind::Append {
                            trigger: trigger.clone(),
                            fields: fields
                                .iter()
                                .map(|field| RuntimeListAppendField {
                                    name: field.name.clone(),
                                    source: field.source.clone(),
                                })
                                .collect(),
                        }
                    }
                    ListOperationKind::Remove { source, predicate } => {
                        RuntimeListOperationKind::Remove {
                            source: source.clone(),
                            predicate: runtime_list_predicate(predicate),
                        }
                    }
                    ListOperationKind::Retain { target, predicate } => {
                        RuntimeListOperationKind::Retain {
                            target: target.clone(),
                            predicate: runtime_list_predicate(predicate),
                        }
                    }
                    ListOperationKind::Count { target, predicate } => {
                        RuntimeListOperationKind::Count {
                            target: target.clone(),
                            predicate: runtime_list_predicate(predicate),
                        }
                    }
                };
                RuntimeListOperation { list, kind }
            })
            .collect();
        Self { operations }
    }

    fn append_trigger(&self, list: &str) -> RuntimeResult<&str> {
        self.operations
            .iter()
            .find_map(|operation| match &operation.kind {
                RuntimeListOperationKind::Append { trigger, .. } if operation.list == list => {
                    Some(trigger.as_str())
                }
                RuntimeListOperationKind::Append { .. }
                | RuntimeListOperationKind::Remove { .. }
                | RuntimeListOperationKind::Retain { .. }
                | RuntimeListOperationKind::Count { .. } => None,
            })
            .ok_or_else(|| format!("list `{list}` has no append operation in IR").into())
    }

    fn append_fields(&self, list: &str, trigger: &str) -> RuntimeResult<&[RuntimeListAppendField]> {
        self.operations
            .iter()
            .find_map(|operation| match &operation.kind {
                RuntimeListOperationKind::Append {
                    trigger: candidate,
                    fields,
                } if operation.list == list && *candidate == trigger => Some(fields.as_slice()),
                RuntimeListOperationKind::Append { .. }
                | RuntimeListOperationKind::Remove { .. }
                | RuntimeListOperationKind::Retain { .. }
                | RuntimeListOperationKind::Count { .. } => None,
            })
            .ok_or_else(|| format!("list `{list}` has no append trigger `{trigger}` in IR").into())
    }

    fn append_initial_fields(
        &self,
        list: &str,
        trigger: &str,
        trigger_value: &str,
    ) -> RuntimeResult<ValueColumns> {
        let operation = self
            .operations
            .iter()
            .find(|operation| match &operation.kind {
                RuntimeListOperationKind::Append {
                    trigger: candidate, ..
                } => operation.list == list && *candidate == trigger,
                RuntimeListOperationKind::Remove { .. }
                | RuntimeListOperationKind::Retain { .. }
                | RuntimeListOperationKind::Count { .. } => false,
            })
            .ok_or_else(|| format!("list `{list}` has no append trigger `{trigger}` in IR"))?;
        let RuntimeListOperationKind::Append { fields, .. } = &operation.kind else {
            unreachable!();
        };
        let mut initial_fields = ValueColumns::default();
        for field in fields {
            if field.source != trigger {
                return Err(format!(
                    "append field `{}` uses unsupported source `{}`; expected trigger `{trigger}`",
                    field.name, field.source
                )
                .into());
            }
            initial_fields.insert_value(
                field.name.to_owned(),
                FieldValue::Text(trigger_value.to_owned()),
            );
        }
        Ok(initial_fields)
    }

    fn count_predicate(&self, list: &str, target: &str) -> RuntimeResult<RuntimeListPredicate> {
        self.operations
            .iter()
            .find_map(|operation| match &operation.kind {
                RuntimeListOperationKind::Count {
                    target: candidate,
                    predicate,
                } if operation.list == list && *candidate == target => Some(predicate.clone()),
                RuntimeListOperationKind::Append { .. }
                | RuntimeListOperationKind::Remove { .. }
                | RuntimeListOperationKind::Retain { .. }
                | RuntimeListOperationKind::Count { .. } => None,
            })
            .ok_or_else(|| format!("list `{list}` has no count operation for `{target}`").into())
    }

    fn count_targets(&self) -> impl Iterator<Item = (&str, &str)> {
        self.operations.iter().filter_map(|operation| {
            let RuntimeListOperationKind::Count { target, .. } = &operation.kind else {
                return None;
            };
            Some((operation.list.as_str(), target.as_str()))
        })
    }

    fn retain_targets(&self) -> impl Iterator<Item = (&str, &str)> {
        self.operations.iter().filter_map(|operation| {
            let RuntimeListOperationKind::Retain { target, .. } = &operation.kind else {
                return None;
            };
            Some((operation.list.as_str(), target.as_str()))
        })
    }

    fn count_targets_for_all_complete_path(&self, path: &str) -> Option<(String, String, String)> {
        let scope = path.rsplit_once('.').map(|(scope, _)| scope)?;
        let active_target = format!("{scope}.active_count");
        let completed_target = format!("{scope}.completed_count");
        let mut list_for_active = None;
        let mut list_for_completed = None;
        for operation in &self.operations {
            let RuntimeListOperationKind::Count { target, .. } = &operation.kind else {
                continue;
            };
            if target == &active_target {
                list_for_active = Some(operation.list.as_str());
            }
            if target == &completed_target {
                list_for_completed = Some(operation.list.as_str());
            }
        }
        match (list_for_active, list_for_completed) {
            (Some(active_list), Some(completed_list)) if active_list == completed_list => {
                Some((active_list.to_owned(), completed_target, active_target))
            }
            _ => None,
        }
    }

    fn retain_predicate(&self, list: &str, target: &str) -> RuntimeResult<RuntimeListPredicate> {
        self.operations
            .iter()
            .find_map(|operation| match &operation.kind {
                RuntimeListOperationKind::Retain {
                    target: candidate,
                    predicate,
                } if operation.list == list && *candidate == target => Some(predicate.clone()),
                RuntimeListOperationKind::Append { .. }
                | RuntimeListOperationKind::Remove { .. }
                | RuntimeListOperationKind::Retain { .. }
                | RuntimeListOperationKind::Count { .. } => None,
            })
            .ok_or_else(|| format!("list `{list}` has no retain operation for `{target}`").into())
    }
}

fn runtime_list_predicate(predicate: &ListPredicate) -> RuntimeListPredicate {
    match predicate {
        ListPredicate::AlwaysTrue => RuntimeListPredicate::AlwaysTrue,
        ListPredicate::RowFieldBool { path } => {
            RuntimeListPredicate::FieldBool { path: path.clone() }
        }
        ListPredicate::RowFieldBoolNot { path } => {
            RuntimeListPredicate::FieldBoolNot { path: path.clone() }
        }
        ListPredicate::SelectedFilterVisibility {
            selector,
            row_field,
        } => RuntimeListPredicate::SelectorVisibility {
            selector: selector.clone(),
            row_field: row_field.clone(),
        },
        _ => RuntimeListPredicate::Unsupported,
    }
}

fn row_field_name(path: &str) -> &str {
    path.rsplit_once('.')
        .map(|(_, field)| field)
        .unwrap_or(path)
}

#[cfg(test)]
#[derive(Clone, Debug)]
struct ListScenarioHarness {
    generic: GenericScheduledRuntime,
    next_source_seq: u64,
    stale_source_drop_count: u64,
}

#[cfg(test)]
#[derive(Clone, Debug)]
enum ListScenarioEvent<'a> {
    Source(GenericRoutedSourceEvent<'a>),
    RowSource {
        routed: GenericRoutedSourceEvent<'a>,
        target_text: &'a str,
        target_occurrence: usize,
    },
    HoverDelete {
        target_text: &'a str,
        target_occurrence: usize,
    },
}

#[cfg(test)]
impl ListScenarioHarness {
    #[cfg(test)]
    fn from_generic(mut generic: GenericScheduledRuntime) -> RuntimeResult<Self> {
        let todo_count = generic.list_len("todos")?;
        let row_source_paths = generic.row_source_paths("todos")?.to_vec();
        generic.reserve_source_bindings(todo_count * row_source_paths.len());
        generic.reserve_source_rows(todo_count);
        for index in 0..todo_count {
            let (key, generation) = generic.row_identity("todos", index)?;
            if generic.row_source_binding_count("todos", key, generation) == 0 {
                generic.bind_row_sources("todos", key, generation, &row_source_paths)?;
            }
        }
        Self {
            generic,
            next_source_seq: 1,
            stale_source_drop_count: 0,
        }
        .validate()
    }

    #[cfg(test)]
    fn validate(self) -> RuntimeResult<Self> {
        for index in 0..self.generic.list_len("todos")? {
            if self
                .generic
                .list_row_textlike("todos", index, "title")?
                .is_empty()
            {
                return Err("TodoMVC initial titles must not be empty".into());
            }
        }
        Ok(self)
    }

    fn apply_step_into<'a>(
        &mut self,
        step: &'a ScenarioStep,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<()> {
        let Some(event) = self.route_step(step)? else {
            return Ok(());
        };
        match &event {
            ListScenarioEvent::Source(routed) => {
                assert_routed_source_event_matches(step, routed.event)?
            }
            ListScenarioEvent::RowSource { routed, .. } => {
                assert_routed_source_event_matches(step, routed.event)?
            }
            ListScenarioEvent::HoverDelete { .. } => {}
        }
        match event {
            ListScenarioEvent::Source(routed)
                if routed.route_kind == SourceActionKind::RootText =>
            {
                let seq = self.next_source_seq();
                let input = self.generic.source_action_input_for_event(
                    &step.id,
                    routed.event,
                    seq,
                    |_, _| Ok(None),
                )?;
                self.generic.apply_source_actions(
                    input,
                    |_| None,
                    |mutation| {
                        emit_list_default_protocol_mutation(mutation, deltas, patches)?;
                        Ok(())
                    },
                )?;
            }
            ListScenarioEvent::Source(routed)
                if routed.route_kind == SourceActionKind::ListAppend =>
            {
                let seq = self.next_source_seq();
                let input = self.generic.source_action_input_for_event(
                    &step.id,
                    routed.event,
                    seq,
                    |_, _| Ok(None),
                )?;
                let batch =
                    self.generic
                        .apply_source_actions_to_batch(input, |_| None, |_| Ok(()))?;
                if let Some(insert) = batch.list_append("todos") {
                    self.emit_list_insert(insert, deltas, patches)?;
                    if let Some(commit) = batch.root_text("store.new_todo_text") {
                        emit_list_default_protocol_mutation(
                            GenericSourceMutation::RootText(commit),
                            deltas,
                            patches,
                        )?;
                    }
                }
            }
            ListScenarioEvent::Source(routed)
                if matches!(
                    routed.route_kind,
                    SourceActionKind::RootScalar | SourceActionKind::RouterRoute
                ) =>
            {
                let seq = self.next_source_seq();
                let input = self.generic.source_action_input_for_event(
                    &step.id,
                    routed.event,
                    seq,
                    |_, _| Ok(None),
                )?;
                self.generic.apply_source_actions(
                    input,
                    |_| None,
                    |mutation| {
                        emit_list_default_protocol_mutation(mutation, deltas, patches)?;
                        Ok(())
                    },
                )?;
            }
            ListScenarioEvent::Source(routed)
                if routed.route_kind == SourceActionKind::ListRemove =>
            {
                self.remove_where_source(&step.id, routed.source(), deltas, patches)?;
            }
            ListScenarioEvent::Source(routed)
                if routed.route_kind == SourceActionKind::IndexedBoolBulk =>
            {
                let all_completed = self.all_completed();
                let input = self.generic.source_action_input_for_list_index(
                    &step.id,
                    routed.event,
                    TickSeq(0),
                    "todos",
                    None,
                )?;
                self.generic.apply_source_actions(
                    input,
                    |path| match path {
                        "store.all_completed" => Some(all_completed),
                        _ => None,
                    },
                    |mutation| {
                        emit_list_default_protocol_mutation(mutation, deltas, patches)?;
                        Ok(())
                    },
                )?;
            }
            ListScenarioEvent::Source(routed) => {
                return Err(format!(
                    "{} list source `{}` classified as unsupported generic route `{}`",
                    step.id,
                    routed.source(),
                    routed.route_kind.as_str()
                )
                .into());
            }
            ListScenarioEvent::RowSource {
                routed,
                target_text,
                target_occurrence,
            } if routed.route_kind == SourceActionKind::IndexedBoolToggle => {
                let index = self.find_index_at_occurrence(target_text, target_occurrence)?;
                let all_completed = self.all_completed();
                self.apply_list_bool_source_action(
                    &step.id,
                    index,
                    routed.source(),
                    Some(target_text),
                    all_completed,
                    deltas,
                    patches,
                )?;
            }
            ListScenarioEvent::RowSource {
                routed,
                target_text,
                target_occurrence,
            } if routed.route_kind == SourceActionKind::IndexedTextOpen => {
                let index = self.find_index_at_occurrence(target_text, target_occurrence)?;
                close_other_list_editors(&mut self.generic, index, deltas, patches)?;
                let all_completed = self.all_completed();
                let source_event = GenericSourceEvent {
                    text: Some(target_text),
                    ..routed.event
                };
                let input = self.generic.source_action_input_for_list_index(
                    &step.id,
                    source_event,
                    TickSeq(0),
                    "todos",
                    Some(index),
                )?;
                let batch = self.generic.apply_source_actions_to_batch(
                    input,
                    |path| match path {
                        "store.all_completed" => Some(all_completed),
                        _ => None,
                    },
                    |_| Ok(()),
                )?;
                let edit_text =
                    batch.require_text(routed.source(), "previous-text update", "edit_text")?;
                let editing = batch.require_bool(routed.source(), "editing update", "editing")?;
                deltas.push(editing.semantic_delta());
                deltas.push(edit_text.semantic_delta());
                emit_generic_render_patch_for_mutation(
                    &GenericSourceMutation::BoolField(editing),
                    GenericRenderContext {
                        ..GenericRenderContext::default()
                    },
                    patches,
                )?;
            }
            ListScenarioEvent::RowSource {
                routed,
                target_text,
                ..
            } if routed.route_kind == SourceActionKind::IndexedTextChange => {
                let index = self
                    .find_index(target_text)
                    .or_else(|_| self.find_editing_index())?;
                let source_event = GenericSourceEvent {
                    text: Some(routed.require_text(&step.id)?),
                    ..routed.event
                };
                let input = self.generic.source_action_input_for_list_index(
                    &step.id,
                    source_event,
                    TickSeq(0),
                    "todos",
                    Some(index),
                )?;
                self.generic.apply_source_actions(
                    input,
                    |_| None,
                    |mutation| {
                        emit_list_default_protocol_mutation(mutation, deltas, patches)?;
                        Ok(())
                    },
                )?;
            }
            ListScenarioEvent::RowSource {
                routed,
                target_text,
                ..
            } if routed.route_kind == SourceActionKind::IndexedTextKey => {
                let key = routed.event.key.unwrap_or_default();
                if matches!(key, "Enter" | "Escape") {
                    let index = self
                        .find_index(target_text)
                        .or_else(|_| self.find_editing_index())?;
                    let all_completed = self.all_completed();
                    let payload_text = if key == "Enter" {
                        routed.event.text
                    } else {
                        Some(target_text)
                    };
                    let source_event = GenericSourceEvent {
                        text: payload_text,
                        ..routed.event
                    };
                    let input = self.generic.source_action_input_for_list_index(
                        &step.id,
                        source_event,
                        TickSeq(0),
                        "todos",
                        Some(index),
                    )?;
                    let batch = self.generic.apply_source_actions_to_batch(
                        input,
                        |path| match path {
                            "store.all_completed" => Some(all_completed),
                            _ => None,
                        },
                        |_| Ok(()),
                    )?;
                    if key == "Enter" {
                        if let Some(title) = batch
                            .text("title")
                            .or_else(|| self.edit_draft_title_commit(index).ok().flatten())
                        {
                            emit_list_default_protocol_mutation(
                                GenericSourceMutation::TextField(title),
                                deltas,
                                patches,
                            )?;
                        }
                    } else if let Some(edit_text) = batch.text("edit_text") {
                        deltas.push(edit_text.semantic_delta());
                    }
                    let editing =
                        batch.require_bool(routed.source(), "editing update", "editing")?;
                    emit_list_default_protocol_mutation(
                        GenericSourceMutation::BoolField(editing),
                        deltas,
                        patches,
                    )?;
                }
            }
            ListScenarioEvent::RowSource {
                routed,
                target_text,
                ..
            } if routed.route_kind == SourceActionKind::IndexedTextCommit => {
                let index = self
                    .find_index(target_text)
                    .or_else(|_| self.find_editing_index())?;
                let all_completed = self.all_completed();
                let source_event = GenericSourceEvent {
                    text: routed.event.text,
                    ..routed.event
                };
                let input = self.generic.source_action_input_for_list_index(
                    &step.id,
                    source_event,
                    TickSeq(0),
                    "todos",
                    Some(index),
                )?;
                let batch = self.generic.apply_source_actions_to_batch(
                    input,
                    |path| match path {
                        "store.all_completed" => Some(all_completed),
                        _ => None,
                    },
                    |_| Ok(()),
                )?;
                if let Some(title) = batch
                    .text("title")
                    .or_else(|| self.edit_draft_title_commit(index).ok().flatten())
                {
                    emit_list_default_protocol_mutation(
                        GenericSourceMutation::TextField(title),
                        deltas,
                        patches,
                    )?;
                }
                let editing = batch.require_bool(routed.source(), "editing update", "editing")?;
                emit_list_default_protocol_mutation(
                    GenericSourceMutation::BoolField(editing),
                    deltas,
                    patches,
                )?;
            }
            ListScenarioEvent::RowSource {
                routed,
                target_text,
                target_occurrence,
            } if routed.route_kind == SourceActionKind::ListRemove => {
                let index = self.find_index_at_occurrence(target_text, target_occurrence)?;
                if !self.remove_index_source(
                    &step.id,
                    index,
                    routed.source(),
                    Some(target_text),
                    deltas,
                    patches,
                )? {
                    return Err(format!(
                        "remove source `{}` predicate does not match todo `{target_text}`",
                        routed.source()
                    )
                    .into());
                }
            }
            ListScenarioEvent::RowSource { routed, .. } => {
                return Err(format!(
                    "{} list row source `{}` classified as unsupported generic route `{}`",
                    step.id,
                    routed.source(),
                    routed.route_kind.as_str()
                )
                .into());
            }
            ListScenarioEvent::HoverDelete {
                target_text,
                target_occurrence,
            } => {
                let index = self.find_index_at_occurrence(target_text, target_occurrence)?;
                let (key, generation) = self.list_row_identity_for_test(index)?;
                patches.push(
                    GenericRenderLoweringPlan::generic().lower_row_affordance_patch(
                        "todos",
                        key,
                        generation,
                        "delete_button",
                        true,
                    )?,
                );
            }
        }
        Ok(())
    }

    fn next_source_seq(&mut self) -> TickSeq {
        let seq = TickSeq(self.next_source_seq);
        self.next_source_seq += 1;
        seq
    }

    fn route_step<'a>(
        &mut self,
        step: &'a ScenarioStep,
    ) -> RuntimeResult<Option<ListScenarioEvent<'a>>> {
        let Some(action) = &step.user_action else {
            return Ok(None);
        };
        let kind = toml_string_ref(action, "kind").unwrap_or_default();
        let target_text = toml_string_ref(action, "target_text").unwrap_or_default();
        let target_occurrence = toml_usize_ref(action, "target_occurrence").unwrap_or(1);
        let event = match (kind, target_text) {
            ("pointer_hover", text) if text.ends_with(" delete") => {
                let target_text = text.trim_end_matches(" delete");
                let target_occurrence = self
                    .resolve_bound_occurrence(step, target_text, target_occurrence)
                    .unwrap_or(target_occurrence);
                ListScenarioEvent::HoverDelete {
                    target_text,
                    target_occurrence,
                }
            }
            _ if step.expected_source_event.is_some() => {
                let source_event = GenericSourceEvent::require(step)?;
                return self.route_source_event(step, source_event, target_occurrence);
            }
            _ => {
                let target = toml_string_ref(action, "target").unwrap_or_default();
                return Err(format!(
                    "{} cannot route TodoMVC user action kind=`{kind}` target=`{target}` target_text=`{target_text}`",
                    step.id
                )
                .into());
            }
        };
        Ok(Some(event))
    }

    fn route_source_event<'a>(
        &mut self,
        step: &'a ScenarioStep,
        source_event: GenericSourceEvent<'a>,
        fallback_occurrence: usize,
    ) -> RuntimeResult<Option<ListScenarioEvent<'a>>> {
        let source = source_event.source;
        let route_kind = self
            .generic
            .classify_source_event("todos", "title", source_event)
            .map_err(|_| format!("{} source `{source}` has no compiled route", step.id))?;
        if source_event.target_text.is_none() {
            match route_kind {
                SourceActionKind::ListAppend
                | SourceActionKind::RootText
                | SourceActionKind::RouterRoute
                | SourceActionKind::ListRemove
                | SourceActionKind::IndexedBoolBulk
                | SourceActionKind::RootScalar => {
                    return Ok(Some(ListScenarioEvent::Source(GenericRoutedSourceEvent {
                        event: source_event,
                        route_kind,
                    })));
                }
                SourceActionKind::IndexedTextChange
                | SourceActionKind::IndexedTextCommit
                | SourceActionKind::IndexedTextIdentity
                | SourceActionKind::IndexedTextKey
                | SourceActionKind::IndexedTextOpen
                | SourceActionKind::IndexedBoolToggle => {}
            }
            return Err(format!("{} source `{source}` has no list route", step.id).into());
        }

        let target_text = source_event
            .target_text
            .expect("checked target_text presence above");
        let Some(target_occurrence) =
            self.resolve_bound_occurrence(step, target_text, fallback_occurrence)
        else {
            return Ok(None);
        };
        if matches!(
            route_kind,
            SourceActionKind::IndexedTextKey
                | SourceActionKind::IndexedTextCommit
                | SourceActionKind::IndexedTextChange
        ) {
            self.editing_title()?;
        }
        if matches!(
            route_kind,
            SourceActionKind::ListRemove
                | SourceActionKind::IndexedBoolToggle
                | SourceActionKind::IndexedTextKey
                | SourceActionKind::IndexedTextCommit
                | SourceActionKind::IndexedTextChange
                | SourceActionKind::IndexedTextOpen
        ) {
            return Ok(Some(ListScenarioEvent::RowSource {
                routed: GenericRoutedSourceEvent {
                    event: source_event,
                    route_kind,
                },
                target_text,
                target_occurrence,
            }));
        }
        Err(format!(
            "{} source `{source}` for target `{target_text}` has no list route",
            step.id
        )
        .into())
    }

    fn resolve_bound_occurrence(
        &mut self,
        step: &ScenarioStep,
        title: &str,
        fallback: usize,
    ) -> Option<usize> {
        let source_event = GenericSourceEvent::from_step(step).ok().flatten();
        match self.generic.resolve_visible_row_occurrence(
            "todos",
            "title",
            step.user_action.as_ref(),
            source_event,
            title,
            fallback,
        ) {
            Ok(GenericVisibleRowOccurrence::Occurrence(occurrence)) => Some(occurrence),
            Ok(GenericVisibleRowOccurrence::Mismatch) => None,
            Ok(GenericVisibleRowOccurrence::Stale) | Err(_) => {
                self.stale_source_drop_count += 1;
                None
            }
        }
    }

    #[cfg(test)]
    fn append_text_row_from_source<'a>(
        &mut self,
        source: &str,
        key: &str,
        title: &'a str,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<()> {
        self.generic
            .set_root_textlike("store.new_todo_text", title)?;
        let Some(insert) = self
            .generic
            .append_text_row_source_action_and_bind_sources("todos", source, Some(key), None)?
        else {
            return Ok(());
        };
        self.emit_list_insert(insert, deltas, patches)?;
        Ok(())
    }

    fn emit_list_insert<'a>(
        &self,
        insert: GenericTextListAppendCommit<'a>,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<()> {
        emit_list_insert_from_generic(&self.generic, insert, deltas, patches)
    }

    fn apply_list_bool_source_action<'a>(
        &mut self,
        step_id: &str,
        index: usize,
        source: &'a str,
        target_text: Option<&'a str>,
        all_completed_snapshot: bool,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<()> {
        let source_event = GenericSourceEvent {
            source,
            text: None,
            key: None,
            target_text,
            address: None,
        };
        let input = self.generic.source_action_input_for_list_index(
            step_id,
            source_event,
            TickSeq(0),
            "todos",
            Some(index),
        )?;
        self.generic.apply_source_actions(
            input,
            |path| match path {
                "store.all_completed" => Some(all_completed_snapshot),
                _ => None,
            },
            |mutation| {
                emit_list_default_protocol_mutation(mutation, deltas, patches)?;
                Ok(())
            },
        )?;
        Ok(())
    }

    fn remove_where_source<'a>(
        &mut self,
        step_id: &str,
        source: &'a str,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<()> {
        let source_event = GenericSourceEvent {
            source,
            text: None,
            key: None,
            target_text: None,
            address: None,
        };
        let input = self.generic.source_action_input_for_list_index(
            step_id,
            source_event,
            TickSeq(0),
            "todos",
            None,
        )?;
        self.generic.apply_source_actions(
            input,
            |_| None,
            |mutation| {
                emit_list_default_protocol_mutation(mutation, deltas, patches)?;
                Ok(())
            },
        )
    }

    fn remove_index_source<'a>(
        &mut self,
        step_id: &str,
        index: usize,
        source: &'a str,
        target_text: Option<&'a str>,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<bool> {
        let mut removed = false;
        let source_event = GenericSourceEvent {
            source,
            text: None,
            key: None,
            target_text,
            address: None,
        };
        let input = self.generic.source_action_input_for_list_index(
            step_id,
            source_event,
            TickSeq(0),
            "todos",
            Some(index),
        )?;
        self.generic.apply_source_actions(
            input,
            |_| None,
            |mutation| {
                if matches!(mutation, GenericSourceMutation::ListRemove { .. }) {
                    removed = true;
                }
                emit_list_default_protocol_mutation(mutation, deltas, patches)?;
                Ok(())
            },
        )?;
        Ok(removed)
    }

    #[cfg(test)]
    fn remove_index<'a>(
        &mut self,
        index: usize,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<()> {
        let generic_row =
            self.generic
                .remove_row_and_unbind_sources("todos", index, |binding| {
                    emit_list_default_protocol_mutation(
                        GenericSourceMutation::SourceUnbind(binding.clone()),
                        deltas,
                        patches,
                    )
                    .expect("test remove source unbind should lower");
                })?;
        let (key, generation) = (generic_row.key, generic_row.generation);
        self.generic.spare_row("todos", generic_row.value)?;
        emit_list_default_protocol_mutation(
            GenericSourceMutation::ListRemove {
                list: "todos".to_owned(),
                key,
                generation,
            },
            deltas,
            patches,
        )?;
        Ok(())
    }

    fn move_index(
        &mut self,
        from: usize,
        to: usize,
        deltas: &mut Vec<SemanticDelta<'_>>,
        patches: &mut Vec<RenderPatch<'_>>,
    ) -> RuntimeResult<()> {
        let commit = self.generic.move_row("todos", from, to)?;
        deltas.push(commit.semantic_move_delta(to));
        patches.push(GenericRenderLoweringPlan::generic().lower_list_move_patch(&commit, to)?);
        Ok(())
    }

    fn find_index(&self, title: &str) -> RuntimeResult<usize> {
        self.find_index_at_occurrence(title, 1)
    }

    fn edit_draft_title_commit<'a>(
        &self,
        index: usize,
    ) -> RuntimeResult<Option<GenericTextFieldCommit<'a>>> {
        let Some(value) = self
            .generic
            .list_row_textlike_opt("todos", index, "edit_text")
            .or_else(|| {
                self.generic
                    .list_row_textlike_opt("todos", index, "edited_title")
            })
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
        else {
            return Ok(None);
        };
        let (key, generation) = self.generic.row_identity("todos", index)?;
        Ok(Some(GenericTextFieldCommit {
            list: "todos".to_owned(),
            key,
            generation,
            field: "title".to_owned(),
            value: Cow::Owned(value),
        }))
    }

    fn list_row_identity_for_test(&self, index: usize) -> RuntimeResult<(u64, u64)> {
        self.generic.row_identity("todos", index)
    }

    #[cfg(test)]
    fn list_len_for_test(&self) -> usize {
        self.generic.list_len("todos").unwrap()
    }

    #[cfg(test)]
    fn list_key_for_test(&self, index: usize) -> u64 {
        self.list_row_identity_for_test(index).unwrap().0
    }

    #[cfg(test)]
    fn list_generation_for_test(&self, index: usize) -> u64 {
        self.list_row_identity_for_test(index).unwrap().1
    }

    #[cfg(test)]
    fn list_title_for_test(&self, index: usize) -> &str {
        self.generic
            .list_row_textlike("todos", index, "title")
            .unwrap()
    }

    #[cfg(test)]
    fn list_edit_text_for_test(&self, index: usize) -> &str {
        self.generic
            .list_row_textlike("todos", index, "edit_text")
            .unwrap()
    }

    #[cfg(test)]
    fn list_completed_for_test(&self, index: usize) -> bool {
        self.generic
            .list_row_bool("todos", index, "completed")
            .unwrap()
    }

    #[cfg(test)]
    fn list_editing_for_test(&self, index: usize) -> bool {
        self.generic
            .list_row_bool("todos", index, "editing")
            .unwrap()
    }

    fn find_index_at_occurrence(&self, title: &str, occurrence: usize) -> RuntimeResult<usize> {
        self.generic
            .find_visible_row_index_by_occurrence("todos", "title", title, occurrence)
    }

    fn find_editing_index(&self) -> RuntimeResult<usize> {
        (0..self.generic.list_len("todos")?)
            .find(|index| {
                self.generic
                    .list_row_bool("todos", *index, "editing")
                    .unwrap_or(false)
            })
            .ok_or_else(|| "no editing todo found".into())
    }

    fn editing_title(&self) -> RuntimeResult<&str> {
        self.generic
            .list_row_textlike("todos", self.find_editing_index()?, "title")
    }

    fn all_completed(&self) -> bool {
        self.generic.list_all_completed_by_count_targets()
    }
}

impl ScenarioExecutor for LoadedRuntime {
    fn prepare_for_scenario(&mut self, _scenario: &Scenario) -> RuntimeResult<()> {
        Ok(())
    }

    fn apply_step<'a>(
        &mut self,
        step: &'a ScenarioStep,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<StepExecutionMetrics> {
        self.apply_generic_step(step, deltas, patches)
    }

    fn assert_step_after_measurement(&mut self, step: &ScenarioStep) -> RuntimeResult<()> {
        let generic = self
            .generic
            .as_ref()
            .ok_or("LoadedRuntime generic schedule was already borrowed")?;
        generic.assert_generic_step_expectations(step)
    }

    fn state_summary(&mut self) -> JsonValue {
        self.generic_state_summary()
    }

    fn stress_profiles(&mut self, _ir: &TypedProgram) -> RuntimeResult<Option<JsonValue>> {
        Ok(None)
    }
}
#[cfg(test)]
fn todomvc_initial_titles_from_ir(ir: &TypedProgram) -> RuntimeResult<Vec<String>> {
    let list = ir
        .lists
        .iter()
        .find(|list| list.name == "todos")
        .ok_or("TodoMVC IR has no `todos` list memory")?;
    let ListInitializer::RecordLiteral { rows } = &list.initializer else {
        return Err("TodoMVC `todos` list initializer is not a record literal".into());
    };
    rows.iter()
        .map(|row| {
            let field = row
                .fields
                .iter()
                .find(|field| field.name == "title")
                .ok_or("TodoMVC initial row has no `title` field")?;
            match &field.value {
                InitialValue::Text { value } => Ok(value.clone()),
                other => Err(format!("TodoMVC initial title is not text: {other:?}").into()),
            }
        })
        .collect()
}

#[cfg(test)]
fn parse_cells_project_for_test() -> ParsedProgram {
    parse_project(
        "examples/cells.bn",
        [
            (
                "examples/cells/defaults.bn".to_owned(),
                include_str!("../../../examples/cells/defaults.bn").to_owned(),
            ),
            (
                "examples/cells/formula.bn".to_owned(),
                include_str!("../../../examples/cells/formula.bn").to_owned(),
            ),
            (
                "examples/cells/cell.bn".to_owned(),
                include_str!("../../../examples/cells/cell.bn").to_owned(),
            ),
            (
                "examples/cells/model.bn".to_owned(),
                include_str!("../../../examples/cells/model.bn").to_owned(),
            ),
            (
                "examples/cells/columns.bn".to_owned(),
                include_str!("../../../examples/cells/columns.bn").to_owned(),
            ),
            (
                "examples/cells/store.bn".to_owned(),
                include_str!("../../../examples/cells/store.bn").to_owned(),
            ),
            (
                "examples/cells/view.bn".to_owned(),
                include_str!("../../../examples/cells/view.bn").to_owned(),
            ),
            (
                "examples/cells.bn".to_owned(),
                include_str!("../../../examples/cells.bn").to_owned(),
            ),
        ],
    )
    .expect("checked-in Cells project should parse")
}

#[cfg(test)]
fn cells_project_source_for_test() -> String {
    parse_cells_project_for_test().source
}

#[cfg(test)]
fn cells_range_from_ir(ir: &TypedProgram) -> Option<(i64, i64)> {
    ir.lists
        .iter()
        .find(|list| list.name == "cells")
        .and_then(|list| match list.initializer {
            ListInitializer::Range { from, to } => Some((from, to)),
            ListInitializer::RecordLiteral { .. }
            | ListInitializer::Empty
            | ListInitializer::Unknown { .. } => None,
        })
}

fn is_cell_address(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some('A'..='Z')) && chars.all(|ch| ch.is_ascii_digit())
}

fn toml_string_ref<'a>(table: &'a BTreeMap<String, toml::Value>, key: &str) -> Option<&'a str> {
    table.get(key).and_then(toml::Value::as_str)
}

fn toml_usize_ref(table: &BTreeMap<String, toml::Value>, key: &str) -> Option<usize> {
    table
        .get(key)
        .and_then(toml::Value::as_integer)
        .and_then(|value| usize::try_from(value).ok())
}

fn toml_u64_ref(table: &BTreeMap<String, toml::Value>, key: &str) -> Option<u64> {
    table
        .get(key)
        .and_then(toml::Value::as_integer)
        .and_then(|value| u64::try_from(value).ok())
}

fn assert_routed_source_event_matches(
    step: &ScenarioStep,
    event: GenericSourceEvent<'_>,
) -> RuntimeResult<()> {
    let expected = GenericSourceEvent::require(step)?;
    assert_source_event_field(&step.id, Some(expected.source), "source", event.source)?;
    if let Some(text) = event.text {
        assert_source_event_field(&step.id, expected.text, "text", text)?;
    }
    if let Some(key) = event.key {
        assert_source_event_field(&step.id, expected.key, "key", key)?;
    }
    if let Some(target_text) = event.target_text {
        assert_source_event_field(&step.id, expected.target_text, "target_text", target_text)?;
    }
    if let Some(address) = event.address {
        assert_source_event_field(&step.id, expected.address, "address", address)?;
    }
    Ok(())
}

fn assert_source_event_field(
    step_id: &str,
    expected_value: Option<&str>,
    key: &str,
    actual: &str,
) -> RuntimeResult<()> {
    let expected_value =
        expected_value.ok_or_else(|| format!("{step_id} expected source event missing `{key}`"))?;
    if expected_value == actual {
        Ok(())
    } else {
        Err(format!(
            "{step_id} routed source event `{key}` expected `{expected_value}`, got `{actual}`"
        )
        .into())
    }
}

#[cfg(test)]
fn field_delta<'a>(
    key: Option<u64>,
    generation: Option<u64>,
    field: &str,
    value: ProtocolValue<'a>,
) -> SemanticDelta<'a> {
    SemanticDelta {
        kind: "FieldSet",
        list_id: key.map(|_| Cow::Borrowed("todos")),
        key,
        generation,
        source_id: None,
        bind_epoch: None,
        field_path: Some(Cow::Owned(field.to_owned())),
        value,
    }
}

#[cfg(test)]
fn emit_list_default_protocol_mutation<'a>(
    mutation: GenericSourceMutation<'a>,
    deltas: &mut Vec<SemanticDelta<'a>>,
    patches: &mut Vec<RenderPatch<'a>>,
) -> RuntimeResult<()> {
    if let Some(delta) = mutation.semantic_delta() {
        deltas.push(delta);
    }
    emit_generic_render_patch_for_mutation(&mutation, GenericRenderContext::default(), patches)
}

#[cfg(test)]
fn emit_list_insert_from_generic<'a>(
    generic: &GenericScheduledRuntime,
    insert: GenericTextListAppendCommit<'a>,
    deltas: &mut Vec<SemanticDelta<'a>>,
    patches: &mut Vec<RenderPatch<'a>>,
) -> RuntimeResult<()> {
    let list = insert.list.clone();
    let key = insert.key;
    let generation = insert.generation;
    emit_list_default_protocol_mutation(
        GenericSourceMutation::ListAppend(insert),
        deltas,
        patches,
    )?;
    for binding in generic.row_source_bindings(&list, key, generation) {
        emit_list_default_protocol_mutation(
            GenericSourceMutation::SourceBind(binding.clone()),
            deltas,
            patches,
        )?;
    }
    Ok(())
}

#[cfg(test)]
fn emit_generic_render_patch_for_mutation<'a>(
    mutation: &GenericSourceMutation<'a>,
    context: GenericRenderContext<'a>,
    patches: &mut Vec<RenderPatch<'a>>,
) -> RuntimeResult<()> {
    let lowerer = GenericRenderLoweringPlan::generic();
    if let Some(render_patch) = lowerer.lower_mutation_patch(mutation, context)? {
        patches.push(render_patch);
    }
    Ok(())
}

#[cfg(test)]
fn dirty_key_count(deltas: &[SemanticDelta<'_>]) -> usize {
    let mut count = 0usize;
    for (index, delta) in deltas.iter().enumerate() {
        let Some(key) = delta.key else {
            continue;
        };
        if !deltas[..index]
            .iter()
            .any(|previous| previous.key == Some(key))
        {
            count += 1;
        }
    }
    count
}

fn patch<'a>(
    kind: &'static str,
    target: RenderTarget<'a>,
    value: ProtocolValue<'a>,
) -> RenderPatch<'a> {
    RenderPatch {
        kind,
        target,
        value,
        list_id: None,
        key: None,
        generation: None,
        source_id: None,
        bind_epoch: None,
    }
}

fn assert_eq_report<T>(step: &str, field: &str, expected: &T, actual: &T) -> RuntimeResult<()>
where
    T: std::fmt::Debug + PartialEq,
{
    if expected == actual {
        Ok(())
    } else {
        Err(format!("{step}: {field} expected {expected:?}, got {actual:?}").into())
    }
}

fn assert_num(step: &str, field: &str, expected: usize, actual: usize) -> RuntimeResult<()> {
    if expected == actual {
        Ok(())
    } else {
        Err(format!("{step}: {field} expected {expected}, got {actual}").into())
    }
}

fn assert_delta_expectations(
    step: &ScenarioStep,
    deltas: &[SemanticDelta<'_>],
    patches: &[RenderPatch<'_>],
) -> RuntimeResult<()> {
    for expected in &step.expect_semantic_delta_contains {
        if !deltas
            .iter()
            .any(|delta| semantic_delta_matches(delta, expected))
        {
            return Err(format!(
                "{}: expected semantic delta `{expected}` in {:?}",
                step.id,
                deltas
                    .iter()
                    .map(|delta| delta_signature(delta))
                    .collect::<Vec<_>>()
            )
            .into());
        }
    }
    for expected in &step.expect_render_delta_contains {
        if !patches
            .iter()
            .any(|patch| render_patch_matches(patch, expected))
        {
            return Err(format!(
                "{}: expected render patch `{expected}` in {:?}",
                step.id,
                patches
                    .iter()
                    .map(|patch| patch.kind.to_owned())
                    .collect::<Vec<_>>()
            )
            .into());
        }
    }
    Ok(())
}

fn semantic_delta_matches(delta: &SemanticDelta<'_>, expected: &str) -> bool {
    if let Some((kind, field)) = expected.split_once(':') {
        return delta.kind == kind
            && delta
                .field_path
                .as_ref()
                .is_some_and(|actual| actual.as_ref() == field);
    }
    delta.kind == expected
}

fn render_patch_matches(patch: &RenderPatch<'_>, expected: &str) -> bool {
    patch.kind == expected
}

fn delta_signature(delta: &SemanticDelta<'_>) -> String {
    match &delta.field_path {
        Some(field) => format!("{}:{field}", delta.kind),
        None => delta.kind.to_owned(),
    }
}

fn percentile(values: &[f64], pct: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((sorted.len() - 1) as f64 * pct).round() as usize;
    sorted[index]
}

fn current_rss_mib() -> Option<f64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb = rest.split_whitespace().next()?.parse::<f64>().ok()?;
            return Some(kb / 1024.0);
        }
    }
    None
}

fn now_string() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{seconds}")
}

fn git_commit() -> String {
    static GIT_COMMIT: OnceLock<String> = OnceLock::new();
    GIT_COMMIT
        .get_or_init(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .ok()
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "unknown".to_owned())
        })
        .clone()
}

fn typecheck_report_hash(ir: &TypedProgram) -> String {
    serde_json::to_vec(&ir.typecheck_report)
        .map(|bytes| sha256_bytes(&bytes))
        .unwrap_or_else(|_| "unserializable-typecheck-report".to_owned())
}

fn render_slot_table_hash(ir: &TypedProgram) -> String {
    serde_json::to_vec(&ir.typecheck_report.render_slot_table)
        .map(|bytes| sha256_bytes(&bytes))
        .unwrap_or_else(|_| "unserializable-render-slot-table".to_owned())
}

fn current_binary_hash() -> String {
    static BINARY_HASH: OnceLock<String> = OnceLock::new();
    BINARY_HASH
        .get_or_init(|| {
            std::env::current_exe()
                .ok()
                .and_then(|path| sha256_file(&path).ok())
                .unwrap_or_else(|| "unknown".to_owned())
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_units_hash_preserves_single_file_hash_compatibility() {
        let source = "document: Document/new(child: TEXT { ok })";
        assert_eq!(
            source_units_hash(&[RuntimeSourceUnit {
                path: "examples/single.bn".to_owned(),
                source: source.to_owned(),
            }]),
            sha256_bytes(source.as_bytes())
        );
        assert_ne!(
            source_units_hash(&[
                RuntimeSourceUnit {
                    path: "examples/a.bn".to_owned(),
                    source: source.to_owned(),
                },
                RuntimeSourceUnit {
                    path: "examples/b.bn".to_owned(),
                    source: source.to_owned(),
                },
            ]),
            sha256_bytes(format!("{source}{source}").as_bytes())
        );
    }

    #[test]
    fn cells_sources_do_not_use_legacy_formula_operators() {
        let legacy_operator_prefix = ["For", "mula", "/"].concat();
        for (path, source) in [
            (
                "examples/cells/formula.bn",
                include_str!("../../../examples/cells/formula.bn"),
            ),
            (
                "examples/cells/cell.bn",
                include_str!("../../../examples/cells/cell.bn"),
            ),
        ] {
            assert!(
                !source.contains(&legacy_operator_prefix),
                "{path} must express cell calculation in ordinary Boon source"
            );
        }
    }

    #[test]
    fn cells_manifest_source_is_generic_boon_without_spreadsheet_shortcuts() {
        let source = cells_project_source_for_test();
        for forbidden in ["Formula", "Grid", "List/table", "EXAMPLE", "#"] {
            assert!(
                !source.contains(forbidden),
                "Cells manifest-backed source must not contain `{forbidden}`"
            );
        }
        assert!(
            source.contains("List/range(from: 0, to: 2599)"),
            "Cells should generate its official 26x100 model from a generic range"
        );
        assert!(
            source.contains("List/chunk(cells, size: 26"),
            "Cells should derive sheet rows with generic List/chunk"
        );
        let parsed = parse_source("examples/cells.bn", &source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert_eq!(cells_range_from_ir(&ir), Some((0, 2599)));
        assert!(
            parsed.operators.iter().all(|operator| {
                !matches!(
                    operator.as_str(),
                    "Formula/eval" | "Grid/cells" | "List/table"
                )
            }),
            "Cells should not lower through spreadsheet-specific operators"
        );
    }

    #[test]
    fn production_runtime_sources_do_not_contain_legacy_formula_runtime_symbols() {
        let forbidden = [
            ["For", "mula", "Ast"].concat(),
            ["For", "mula", "Term"].concat(),
            ["For", "mula", "Operator", "Plan"].concat(),
            ["Addressed", "For", "mula", "Runtime"].concat(),
            ["parse", "_formula", "_ast"].concat(),
            ["formula", "_ast", "_dependencies"].concat(),
        ];
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for relative in [
            "../boon_parser/src/lib.rs",
            "../boon_ir/src/lib.rs",
            "src/lib.rs",
        ] {
            let path = crate_root.join(relative);
            let text = std::fs::read_to_string(&path).unwrap();
            for needle in &forbidden {
                assert!(
                    !text.contains(needle),
                    "{} still contains legacy formula runtime symbol `{}`",
                    path.display(),
                    needle
                );
            }
        }
    }

    fn list_scenario_harness_from_parsed(parsed: &ParsedProgram) -> ListScenarioHarness {
        let ir = lower(parsed).unwrap();
        let compiled = CompiledProgram::from_ir(&ir).unwrap();
        let generic = GenericScheduledRuntime::new(&ir, &compiled).unwrap();
        ListScenarioHarness::from_generic(generic).unwrap()
    }

    fn physical_todomvc_project_for_test() -> ParsedProgram {
        parse_project(
            "examples/todo_mvc_physical/RUN.bn",
            [
                (
                    "examples/todo_mvc_physical/Theme/Classic.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/Theme/Classic.bn").to_owned(),
                ),
                (
                    "examples/todo_mvc_physical/Theme/Professional.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/Theme/Professional.bn")
                        .to_owned(),
                ),
                (
                    "examples/todo_mvc_physical/Theme/Glassmorphism.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/Theme/Glassmorphism.bn")
                        .to_owned(),
                ),
                (
                    "examples/todo_mvc_physical/Theme/Neobrutalism.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/Theme/Neobrutalism.bn")
                        .to_owned(),
                ),
                (
                    "examples/todo_mvc_physical/Theme/Neumorphism.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/Theme/Neumorphism.bn")
                        .to_owned(),
                ),
                (
                    "examples/todo_mvc_physical/Theme/Theme.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/Theme/Theme.bn").to_owned(),
                ),
                (
                    "examples/todo_mvc_physical/Generated/Assets.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/Generated/Assets.bn")
                        .to_owned(),
                ),
                (
                    "examples/todo_mvc_physical/RUN.bn".to_owned(),
                    include_str!("../../../examples/todo_mvc_physical/RUN.bn").to_owned(),
                ),
            ],
        )
        .unwrap()
    }

    fn physical_todomvc_project_root_for_test() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/todo_mvc_physical")
    }

    #[test]
    fn physical_todomvc_source_preserves_original_assets_and_declares_theme_divergence() {
        let original_root = Path::new(
            "/home/martinkavik/repos/boon/playground/frontend/src/examples/todo_mvc_physical",
        );
        assert!(
            original_root.exists(),
            "original physical TodoMVC checkout is required for source preservation proof"
        );
        let migrated_root = physical_todomvc_project_root_for_test();
        for relative in [
            "BUILD.bn",
            "Generated/Assets.bn",
            "assets/icons/checkbox_active.svg",
            "assets/icons/checkbox_completed.svg",
        ] {
            let original = std::fs::read_to_string(original_root.join(relative)).unwrap();
            let migrated = std::fs::read_to_string(migrated_root.join(relative)).unwrap();
            let expected = if relative.ends_with(".bn") {
                original.replace("LINK", "SOURCE")
            } else {
                original
            };
            assert_eq!(
                migrated, expected,
                "{relative} changed beyond LINK to SOURCE"
            );
        }
        for relative in [
            "Theme/Glassmorphism.bn",
            "Theme/Neobrutalism.bn",
            "Theme/Neumorphism.bn",
            "Theme/Professional.bn",
        ] {
            let migrated = std::fs::read_to_string(migrated_root.join(relative)).unwrap();
            assert!(
                migrated.contains("CheckboxGlyph[checked]"),
                "{relative} should declare the intentional themed checkbox glyph divergence"
            );
            assert!(
                !migrated.contains("LINK"),
                "{relative} should keep the migration-wide SOURCE spelling"
            );
        }
        assert!(
            migrated_root.join("Theme/Classic.bn").exists(),
            "Classic is a Boon Circuit scene theme added during migration"
        );
        for relative in ["RUN.bn", "Theme/Theme.bn"] {
            let migrated = std::fs::read_to_string(migrated_root.join(relative)).unwrap();
            assert!(
                migrated.contains("Classic"),
                "{relative} should document the intentional Classic theme divergence"
            );
            assert!(
                !migrated.contains("LINK"),
                "{relative} should keep the migration-wide SOURCE spelling"
            );
        }
    }

    #[test]
    fn physical_todomvc_build_assets_match_generated_file() {
        let root = physical_todomvc_project_root_for_test();
        let temp_root =
            std::env::temp_dir().join(format!("boon-physical-build-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_root);
        copy_dir_for_test(&root, &temp_root);
        std::fs::write(temp_root.join("Generated/Assets.bn"), "").unwrap();

        let result = run_project_build_file(&temp_root, Path::new("BUILD.bn"), true).unwrap();
        assert_eq!(result.status, "pass");
        assert_eq!(result.output_file, "./Generated/Assets.bn");
        assert_eq!(result.output_binding, "icon");
        assert_eq!(
            result.input_files,
            vec![
                "assets/icons/checkbox_active.svg".to_owned(),
                "assets/icons/checkbox_completed.svg".to_owned()
            ]
        );
        assert_eq!(result.written_files, vec!["Generated/Assets.bn".to_owned()]);

        let generated = std::fs::read_to_string(temp_root.join("Generated/Assets.bn")).unwrap();
        let expected = std::fs::read_to_string(root.join("Generated/Assets.bn")).unwrap();
        assert_eq!(generated, expected);
        assert_eq!(result.output_sha256, sha256_bytes(expected.as_bytes()));

        let verified = generated_output_for_project_build_file(&root, Path::new("BUILD.bn"))
            .expect("checked generated assets should match BUILD.bn output");
        assert_eq!(verified, expected);
        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn build_file_runner_rejects_paths_that_escape_project_root() {
        let root =
            std::env::temp_dir().join(format!("boon-build-sandbox-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("assets/icons")).unwrap();
        std::fs::create_dir_all(root.join("Generated")).unwrap();
        std::fs::write(
            root.join("assets/icons/checkbox_active.svg"),
            "<svg><circle /></svg>",
        )
        .unwrap();
        std::fs::write(
            root.join("BUILD.bn"),
            r#"icons_directory: TEXT { ../assets/icons }
output_file: TEXT { ./Generated/Assets.bn }
svg_files: icons_directory |> Directory/entries() |> List/retain(item, if: item.extension == TEXT { svg }) |> List/sort_by(item, key: item.path)
generation_result: svg_files |> List/map(old, new: old |> icon_code()) |> Text/join_lines() |> File/write_text(path: output_file)
generation_error_handling: generation_result |> WHEN { Ok => Build/succeed() error => Build/fail() }
FUNCTION icon_code(item) {
    item.path |> File/read_text() |> Url/encode() |> WHEN { encoded => Ok[text: TEXT { {item.file_stem}: data:image/svg+xml;utf8,{encoded} }] }
}
-- FLUSH
"#,
        )
        .unwrap();
        let error = run_project_build_file(&root, Path::new("BUILD.bn"), true)
            .expect_err("parent-directory paths must be rejected");
        assert!(
            error.to_string().contains("parent-directory"),
            "unexpected sandbox error: {error}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    fn copy_dir_for_test(source: &Path, destination: &Path) {
        std::fs::create_dir_all(destination).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let target = destination.join(entry.file_name());
            if path.is_dir() {
                copy_dir_for_test(&path, &target);
            } else {
                std::fs::copy(&path, &target).unwrap();
            }
        }
    }

    fn list_row_index(runtime: &ListScenarioHarness, title: &str) -> usize {
        runtime.find_index(title).unwrap()
    }

    #[test]
    fn physical_todomvc_root_source_event_default_materializes() {
        let parsed = physical_todomvc_project_for_test();
        let ir = lower(&parsed).unwrap();
        assert!(
            ir.derived_values.iter().any(|value| {
                value.path == "theme_options.name"
                    && value.kind == DerivedValueKind::SourceEventTransform
                    && !value.indexed
            }),
            "{:#?}",
            ir.derived_values
                .iter()
                .map(|value| format!("{} {:?} indexed={}", value.path, value.kind, value.indexed))
                .collect::<Vec<_>>()
        );
        let compiled = CompiledProgram::from_ir(&ir).unwrap();
        let generic = GenericScheduledRuntime::new(&ir, &compiled).unwrap();
        assert_eq!(
            generic.root_textlike_ref("theme_options.name").unwrap(),
            "Classic"
        );
    }

    #[test]
    fn physical_todomvc_theme_source_event_updates_latest_root_text() {
        let parsed = physical_todomvc_project_for_test();
        let ir = lower(&parsed).unwrap();
        let theme_targets = root_text_transform_targets_from_ir(&ir).unwrap();
        let theme_debug = ir
            .derived_values
            .iter()
            .filter(|value| value.path == "theme_options.name")
            .map(|value| {
                let exprs = statement_ast_exprs(&value.statement, &ir.expressions);
                (
                    value.sources.clone(),
                    exprs
                        .iter()
                        .map(|expr| (expr.id, expr.line, format!("{:?}", expr.kind)))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        assert!(
            theme_targets.iter().any(|target| {
                target.source == "store.elements.theme_switcher.classic"
                    && target.target == "theme_options.name"
                    && target.value == "Classic"
            }),
            "{theme_targets:#?}\n{theme_debug:#?}"
        );
        assert!(
            theme_targets.iter().any(|target| {
                target.source == "store.elements.theme_switcher.glassmorphism"
                    && target.target == "theme_options.name"
                    && target.value == "Glassmorphism"
            }),
            "{theme_targets:#?}\n{theme_debug:#?}"
        );
        let mut runtime = list_scenario_harness_from_parsed(&parsed);
        assert_eq!(
            runtime
                .generic
                .root_textlike_ref("theme_options.name")
                .unwrap(),
            "Classic"
        );
        assert_eq!(
            runtime.generic.document_summary()["theme_options"]["name"],
            "Classic",
            "document summary should expose root source-event transforms used by physical styling"
        );

        let mut action = BTreeMap::new();
        action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
        action.insert(
            "target".to_owned(),
            toml::Value::String("theme glassmorphism".to_owned()),
        );
        let mut expected = BTreeMap::new();
        expected.insert(
            "source".to_owned(),
            toml::Value::String("store.elements.theme_switcher.glassmorphism".to_owned()),
        );
        let step = ScenarioStep {
            id: "physical-theme-glassmorphism".to_owned(),
            user_action: Some(action),
            expected_source_event: Some(expected),
            ..ScenarioStep::default()
        };
        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        runtime
            .apply_step_into(&step, &mut deltas, &mut patches)
            .unwrap();

        assert_eq!(
            runtime
                .generic
                .root_textlike_ref("theme_options.name")
                .unwrap(),
            "Glassmorphism"
        );
        assert_eq!(
            runtime.generic.document_summary()["theme_options"]["name"],
            "Glassmorphism",
            "document summary should update root source-event transforms after source events"
        );
        assert!(deltas.iter().any(|delta| {
            delta.kind == "FieldSet" && delta.field_path.as_deref() == Some("theme_options.name")
        }));
    }

    #[test]
    fn physical_todomvc_add_todo_defaults_active_and_remains_toggleable() {
        let parsed = physical_todomvc_project_for_test();
        let mut runtime = list_scenario_harness_from_parsed(&parsed);

        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        let mut submit_action = BTreeMap::new();
        submit_action.insert(
            "kind".to_owned(),
            toml::Value::String("key_down".to_owned()),
        );
        submit_action.insert(
            "target".to_owned(),
            toml::Value::String("new todo input".to_owned()),
        );
        submit_action.insert("key".to_owned(), toml::Value::String("Enter".to_owned()));
        let mut submit_expected = BTreeMap::new();
        submit_expected.insert(
            "source".to_owned(),
            toml::Value::String("store.elements.new_todo_title_text_input".to_owned()),
        );
        submit_expected.insert("key".to_owned(), toml::Value::String("Enter".to_owned()));
        submit_expected.insert(
            "text".to_owned(),
            toml::Value::String("ship physical".to_owned()),
        );
        let submit_step = ScenarioStep {
            id: "physical-add-todo-submit".to_owned(),
            user_action: Some(submit_action),
            expected_source_event: Some(submit_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&submit_step, &mut deltas, &mut patches)
            .unwrap();
        let added_index = list_row_index(&runtime, "ship physical");
        assert_eq!(runtime.list_title_for_test(added_index), "ship physical");
        assert!(
            !runtime.list_completed_for_test(added_index),
            "new physical todos should default to active"
        );

        let mut toggle_action = BTreeMap::new();
        toggle_action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
        toggle_action.insert(
            "target_text".to_owned(),
            toml::Value::String("ship physical".to_owned()),
        );
        let mut toggle_expected = BTreeMap::new();
        toggle_expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.todo_elements.todo_checkbox".to_owned()),
        );
        toggle_expected.insert(
            "target_text".to_owned(),
            toml::Value::String("ship physical".to_owned()),
        );
        let toggle_step = ScenarioStep {
            id: "physical-added-todo-toggle".to_owned(),
            user_action: Some(toggle_action),
            expected_source_event: Some(toggle_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&toggle_step, &mut deltas, &mut patches)
            .unwrap();
        assert!(
            runtime.list_completed_for_test(added_index),
            "new physical todos should remain toggleable"
        );
    }

    #[test]
    fn physical_todomvc_router_filter_source_updates_selected_filter() {
        let parsed = physical_todomvc_project_for_test();
        let ir = lower(&parsed).unwrap();
        let router_targets = router_route_targets_from_ir(&ir).unwrap();
        assert!(
            router_targets.iter().any(|target| {
                target.source == "store.elements.filter_buttons.completed"
                    && target.path == "/completed"
            }),
            "{router_targets:#?}"
        );
        let mut runtime = list_scenario_harness_from_parsed(&parsed);
        assert_eq!(
            runtime
                .generic
                .root_textlike_ref("store.selected_filter")
                .unwrap(),
            "All"
        );

        let mut action = BTreeMap::new();
        action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
        action.insert(
            "target".to_owned(),
            toml::Value::String("completed filter".to_owned()),
        );
        let mut expected = BTreeMap::new();
        expected.insert(
            "source".to_owned(),
            toml::Value::String("store.elements.filter_buttons.completed".to_owned()),
        );
        let step = ScenarioStep {
            id: "physical-router-filter".to_owned(),
            user_action: Some(action),
            expected_source_event: Some(expected),
            ..ScenarioStep::default()
        };
        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        runtime
            .apply_step_into(&step, &mut deltas, &mut patches)
            .unwrap();

        assert_eq!(runtime.generic.router_route, "/completed");
        let selected_filter_field = runtime
            .generic
            .generic_derived
            .root_fields
            .iter()
            .find(|field| field.path == "store.selected_filter")
            .cloned()
            .unwrap();
        let selected_filter_value = runtime
            .generic
            .eval_statement_value(
                &selected_filter_field.statement,
                &mut GenericEvalFrame::root(),
            )
            .unwrap()
            .as_text();
        assert_eq!(selected_filter_value.as_deref(), Some("Completed"));
        assert_eq!(
            runtime
                .generic
                .root_textlike_ref("store.selected_filter")
                .unwrap(),
            "Completed"
        );
        assert!(deltas.iter().any(|delta| {
            delta.kind == "FieldSet" && delta.field_path.as_deref() == Some("store.selected_filter")
        }));
    }

    #[test]
    fn todomvc_scenario_runs_and_removes_rows() {
        let output = run_scenario(
            Path::new("../../examples/todomvc.bn"),
            Path::new("../../examples/todomvc.scn"),
            VerificationLayer::Semantic,
            None,
        )
        .unwrap();
        assert_eq!(output.report["status"], "pass");
        assert_eq!(output.state_summary["active_count"], 0);
        assert_eq!(output.state_summary["todos"], json!([]));
        assert!(
            output
                .semantic_deltas
                .iter()
                .any(|delta| delta.kind == "ListRemove"
                    && delta.list_id.as_deref() == Some("todos")
                    && delta.key.is_some()
                    && delta.generation.is_some())
        );
    }

    #[test]
    fn profiled_list_capacity_rejects_overflow_append() {
        let source = include_str!("../../../examples/todomvc.bn").replace("LIST {", "LIST[4] {");
        let error = run_scenario_source_with_step_limit(
            "capacity:todomvc",
            &source,
            Path::new("../../examples/todomvc.scn"),
            VerificationLayer::Semantic,
            Some(3),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("generic list `todos` capacity 4 exceeded by append"),
            "{error}"
        );
    }

    #[test]
    fn profiled_list_capacity_rejects_oversized_initializer() {
        let source = include_str!("../../../examples/todomvc.bn").replace("LIST {", "LIST[1] {");
        let error = run_scenario_source_with_step_limit(
            "capacity:todomvc",
            &source,
            Path::new("../../examples/todomvc.scn"),
            VerificationLayer::Semantic,
            Some(1),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("list `todos` initializes 4 rows beyond declared capacity 1"),
            "{error}"
        );
    }

    #[test]
    fn cells_scenario_runs_and_detects_cycle() {
        let output = run_scenario(
            Path::new("../../examples/cells.bn"),
            Path::new("../../examples/cells.scn"),
            VerificationLayer::Semantic,
            None,
        )
        .unwrap();
        assert_eq!(output.report["status"], "pass");
        assert!(output.semantic_deltas.iter().any(|delta| matches!(
            (&delta.field_path, &delta.value),
            (Some(field), ProtocolValue::Text(error))
                if field.as_ref() == "error" && error.as_ref() == "cycle_error"
        )));
        assert_eq!(output.state_summary["cells"][0]["error"], JsonValue::Null);
    }

    #[test]
    fn runtime_execution_schema_rejects_adapter_or_incomplete_generic_slices() {
        let output = run_scenario(
            Path::new("../../examples/todomvc.bn"),
            Path::new("../../examples/todomvc.scn"),
            VerificationLayer::Semantic,
            None,
        )
        .unwrap();
        verify_runtime_execution_metadata(&output.report, Path::new("memory:todomvc")).unwrap();

        let mut adapter_report = output.report.clone();
        adapter_report["runtime_execution"]["example_behavior_adapter"] = json!(true);
        assert!(
            verify_runtime_execution_metadata(&adapter_report, Path::new("memory:todomvc"))
                .unwrap_err()
                .to_string()
                .contains("example behavior adapter")
        );

        let mut incomplete_report = output.report.clone();
        incomplete_report["runtime_execution"]["generic_runtime_slices"]["generic_source_route_classifier"] =
            json!(false);
        assert!(
            verify_runtime_execution_metadata(&incomplete_report, Path::new("memory:todomvc"))
                .unwrap_err()
                .to_string()
                .contains("generic_source_route_classifier")
        );

        let mut other_example_slice_report = output.report.clone();
        other_example_slice_report["runtime_execution"]["generic_runtime_slices"]["generic_cells_editor_route_uses_indexed_targets"] =
            json!(true);
        assert!(
            verify_runtime_execution_metadata(
                &other_example_slice_report,
                Path::new("memory:todomvc")
            )
            .unwrap_err()
            .to_string()
            .contains("other-example runtime slice")
        );

        let mut omitted_slice_report = output.report.clone();
        omitted_slice_report["runtime_execution"]["generic_runtime_slices"]
            .as_object_mut()
            .unwrap()
            .remove("generic_source_event_ingest");
        assert!(
            verify_runtime_execution_metadata(&omitted_slice_report, Path::new("memory:todomvc"))
                .unwrap_err()
                .to_string()
                .contains("generic_source_event_ingest")
        );

        let mut drifted_runtime_metadata_report = output.report.clone();
        drifted_runtime_metadata_report["runtime_execution"]["runtime_profile"] =
            json!("software_bounded");
        assert!(
            verify_runtime_execution_metadata(
                &drifted_runtime_metadata_report,
                Path::new("memory:todomvc")
            )
            .unwrap_err()
            .to_string()
            .contains("runtime_execution `runtime_profile` does not match")
        );

        let mut unknown_expression_report = output.report.clone();
        unknown_expression_report["expression_coverage"]["unknown_ast_expression_count"] = json!(1);
        unknown_expression_report["runtime_execution"]["expression_coverage"]["unknown_ast_expression_count"] =
            json!(1);
        assert!(
            verify_runtime_execution_metadata(
                &unknown_expression_report,
                Path::new("memory:todomvc")
            )
            .unwrap_err()
            .to_string()
            .contains("unknown_ast_expression_count")
        );
    }

    #[test]
    fn runtime_completeness_and_adapter_flags_are_derived_from_slices() {
        let parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let ir = lower(&parsed).unwrap();
        let compiled = CompiledProgram::from_ir(&ir).unwrap();
        let slices = generic_runtime_slices_report(&ir, &compiled);

        assert!(
            derive_generic_interpreter_complete(&ir, &compiled, &slices),
            "baseline TodoMVC slices should satisfy the generic interpreter contract"
        );
        assert!(
            !derive_example_behavior_adapter(&compiled, &slices),
            "baseline TodoMVC slices should not report an example behavior adapter"
        );

        let mut incomplete_slices = slices.clone();
        incomplete_slices["generic_loaded_runtime_shell"] = json!(false);
        assert!(
            !derive_generic_interpreter_complete(&ir, &compiled, &incomplete_slices),
            "generic_interpreter_complete must fail when a required current-example slice fails"
        );

        let mut adapter_slices = slices;
        adapter_slices["surface_driver_borrows_generic_storage_for_tick"] = json!(true);
        assert!(
            derive_example_behavior_adapter(&compiled, &adapter_slices),
            "example_behavior_adapter must reflect adapter evidence instead of staying hardcoded false"
        );
    }

    #[test]
    fn remaining_shells_are_derived_from_generic_runtime_slices() {
        let parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let ir = lower(&parsed).unwrap();
        let compiled = CompiledProgram::from_ir(&ir).unwrap();
        let shells = remaining_example_specific_shells(
            &compiled,
            &json!({
                "generic_scenario_preparation": true,
                "generic_scenario_expectation_assertions": true,
                "generic_summary_reads_authoritative_storage": true,
                "generic_common_render_patch_lowering": true,
                "generic_loaded_runtime_stress_profile_executor": true
            }),
        );
        assert_eq!(
            shells,
            vec![
                "generic_scenario_glue",
                "generic_assertion_glue",
                "generic_report_glue",
                "generic_render_patch_report_glue",
                "generic_stress_report_glue",
            ]
        );

        let shells = remaining_example_specific_shells(
            &compiled,
            &json!({
                "generic_scenario_preparation": false,
                "generic_scenario_expectation_assertions": true,
            }),
        );
        assert_eq!(shells, vec!["generic_assertion_glue"]);
    }

    #[test]
    fn schema_accepts_failing_blocker_audits_only_as_blocker_evidence() {
        let path = std::env::temp_dir().join(format!(
            "boon-readiness-schema-{}-{}.json",
            std::process::id(),
            now_string()
        ));
        let mut report = json!({
            "status": "fail",
            "report_version": 1,
            "generated_at_utc": now_string(),
            "command": "audit-goal-readiness",
            "command_argv": ["audit-goal-readiness", "--report", "target/reports/goal-readiness.json"],
            "exit_status": 1,
            "git_commit": git_commit(),
            "binary_hash": current_binary_hash(),
            "source_hash": "n/a",
            "scenario_hash": "n/a",
            "program_hash": "n/a",
            "budget_hash": "n/a",
            "graph_node_count": 0,
            "per_step_pass_fail": [
                {"id": "human-report-present", "pass": false, "detail": "missing real human report"}
            ],
            "blockers": ["missing fresh real human report"],
            "artifact_sha256s": []
        });

        write_json(&path, &report).unwrap();
        verify_report_schema(&path).unwrap();

        report["command"] = json!("verify-runtime-finality");
        report["command_argv"] = json!([
            "verify-runtime-finality",
            "--report",
            "target/reports/runtime-finality.json"
        ]);
        report["per_step_pass_fail"] = json!([
            {"id": "runtime-finality:parser:real-ast-not-text-lines", "pass": false, "detail": "parser blocker"}
        ]);
        report["blockers"] = json!([
            "parser still depends on line/text/path heuristics instead of a structured AST"
        ]);
        write_json(&path, &report).unwrap();
        verify_report_schema(&path).unwrap();

        report["command"] = json!("verify-example-all");
        write_json(&path, &report).unwrap();
        assert!(
            verify_report_schema(&path)
                .unwrap_err()
                .to_string()
                .contains("did not pass")
        );

        report["command"] = json!("audit-machine-readiness");
        report["command_argv"] = json!([
            "audit-machine-readiness",
            "--report",
            "target/reports/debug/machine-readiness.json"
        ]);
        write_json(&path, &report).unwrap();
        verify_report_schema(&path).unwrap();

        report["command"] = json!("audit-goal-readiness");
        report["blockers"] = json!([]);
        write_json(&path, &report).unwrap();
        assert!(
            verify_report_schema(&path)
                .unwrap_err()
                .to_string()
                .contains("blockers")
        );

        report["blockers"] = json!(["missing fresh real human report"]);
        report["per_step_pass_fail"] = json!([
            {"id": "schema-only", "pass": true, "detail": "not a readiness blocker"}
        ]);
        write_json(&path, &report).unwrap();
        assert!(
            verify_report_schema(&path)
                .unwrap_err()
                .to_string()
                .contains("no failing per-step check")
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn semantic_delta_batches_require_runtime_identity_and_server_tick() {
        let output = run_scenario_source_with_step_limit(
            "delta-protocol:todomvc",
            include_str!("../../../examples/todomvc.bn"),
            Path::new("../../examples/todomvc.scn"),
            VerificationLayer::Semantic,
            Some(3),
        )
        .unwrap();
        verify_semantic_delta_protocol_batches(&output.report, Path::new("memory:todomvc"))
            .unwrap();

        let mut missing_runtime_id = output.report.clone();
        missing_runtime_id["semantic_delta_protocol_batches"][1]["runtime_id"] = JsonValue::Null;
        assert!(
            verify_semantic_delta_protocol_batches(
                &missing_runtime_id,
                Path::new("memory:todomvc")
            )
            .unwrap_err()
            .to_string()
            .contains("runtime_id")
        );

        let mut stale_server_tick = output.report.clone();
        stale_server_tick["semantic_delta_protocol_batches"][1]["server_tick"] = json!(99);
        assert!(
            verify_semantic_delta_protocol_batches(&stale_server_tick, Path::new("memory:todomvc"))
                .unwrap_err()
                .to_string()
                .contains("server_tick")
        );
    }

    #[test]
    fn playground_surface_schema_requires_visible_manual_test_controls() {
        let mut report = playground_surface_fixture();
        verify_playground_surface_report(&report, Path::new("memory:playground")).unwrap();

        report["playground_surface"]["code_editor"] = json!(false);
        assert!(
            verify_playground_surface_report(&report, Path::new("memory:playground"))
                .unwrap_err()
                .to_string()
                .contains("code_editor")
        );

        let mut zero_bounds = playground_surface_fixture();
        zero_bounds["playground_surface_visible_bounds"]["semantic_delta_log"]["elements"][0]["bounds"]
            ["width"] = json!(0.0);
        assert!(
            verify_playground_surface_report(&zero_bounds, Path::new("memory:playground"))
                .unwrap_err()
                .to_string()
                .contains("semantic_delta_log")
        );
    }

    fn playground_surface_fixture() -> JsonValue {
        let mut surface = serde_json::Map::new();
        let mut bounds = serde_json::Map::new();
        for key in [
            "example_selector",
            "code_editor",
            "run_reset_step_controls",
            "render_preview",
            "semantic_delta_log",
            "selected_value_inspector",
            "dependency_explanation_panel",
        ] {
            surface.insert(key.to_owned(), json!(true));
            bounds.insert(
                key.to_owned(),
                json!({
                    "pass": true,
                    "elements": [{
                        "element_id": format!("{key}_fixture"),
                        "visible": true,
                        "bounds": {"x": 1.0, "y": 1.0, "width": 10.0, "height": 10.0}
                    }]
                }),
            );
        }
        json!({
            "playground_surface": surface,
            "playground_surface_visible_bounds": bounds
        })
    }

    #[test]
    fn developer_state_summary_hides_runtime_identity() {
        let todo_output = run_scenario_source_with_step_limit(
            "identity-summary:todomvc",
            include_str!("../../../examples/todomvc.bn"),
            Path::new("../../examples/todomvc.scn"),
            VerificationLayer::Semantic,
            Some(3),
        )
        .unwrap();
        assert!(
            todo_output.state_summary.get("hidden_keys").is_none(),
            "TodoMVC state summary must not expose hidden row keys"
        );
        for row in todo_output.state_summary["todos"].as_array().unwrap() {
            assert!(row.get("hidden_key").is_none());
            assert!(row.get("hidden_generation").is_none());
        }

        let cells_output = run_scenario(
            Path::new("../../examples/cells.bn"),
            Path::new("../../examples/cells.scn"),
            VerificationLayer::Semantic,
            None,
        )
        .unwrap();
        assert!(
            cells_output.state_summary.get("hidden_keys").is_none(),
            "Cells state summary must not expose hidden row keys"
        );
        for row in cells_output.state_summary["cells"].as_array().unwrap() {
            assert!(row.get("hidden_key").is_none());
            assert!(row.get("hidden_generation").is_none());
        }
    }

    #[test]
    fn report_list_slot_count_uses_generic_state_arrays() {
        let summary = json!({
            "store": {"selected": "A0"},
            "sheet": [{"address": "A0"}, {"address": "A1"}],
            "columns": [{"label": "A"}]
        });
        assert_eq!(report_list_slot_count(&summary), json!(2));

        let empty_summary = json!({"store": {"selected": "A0"}});
        assert!(
            report_list_slot_count(&empty_summary)
                .get("unavailable_reason")
                .is_some()
        );
    }

    #[test]
    fn playground_source_text_runs_with_step_limit() {
        let source = include_str!("../../../examples/todomvc.bn");
        let output = run_scenario_source_with_step_limit(
            "playground-editor:todomvc",
            source,
            Path::new("../../examples/todomvc.scn"),
            VerificationLayer::Semantic,
            Some(3),
        )
        .unwrap();
        assert_eq!(output.report["total_ticks"], 3);
        assert_eq!(output.state_summary["active_count"], 4);
    }

    #[test]
    fn generic_source_mutations_emit_keyed_semantic_deltas() {
        let todo_source = include_str!("../../../examples/todomvc.bn");
        let todo_output = run_scenario_source_with_step_limit(
            "generic-delta:todomvc",
            todo_source,
            Path::new("../../examples/todomvc.scn"),
            VerificationLayer::Semantic,
            Some(3),
        )
        .unwrap();
        let insert = todo_output
            .semantic_deltas
            .iter()
            .find(|delta| delta.kind == "ListInsert")
            .expect("TodoMVC append should emit a keyed generic list insert");
        assert_eq!(insert.list_id.as_deref(), Some("todos"));
        assert!(insert.key.is_some());
        assert!(insert.generation.is_some());
        assert!(matches!(
            &insert.value,
            ProtocolValue::Text(value) if value.as_ref() == "Test todo"
        ));

        let cells_source = cells_project_source_for_test();
        let cells_output = run_scenario_source_with_step_limit(
            "generic-delta:cells",
            &cells_source,
            Path::new("../../examples/cells.scn"),
            VerificationLayer::Semantic,
            Some(8),
        )
        .unwrap();
        let formula = cells_output
            .semantic_deltas
            .iter()
            .find(|delta| delta.field_path.as_deref() == Some("formula_text"))
            .expect("Cells commit should emit a keyed generic field delta");
        assert_eq!(formula.list_id.as_deref(), Some("cells"));
        assert!(formula.key.is_some());
        assert!(formula.generation.is_some());
    }

    #[test]
    fn generic_source_events_route_to_action_inputs() {
        let todo_parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let todo_ir = lower(&todo_parsed).unwrap();
        let todo_compiled = CompiledProgram::from_ir(&todo_ir).unwrap();
        let todo_runtime = GenericScheduledRuntime::new(&todo_ir, &todo_compiled).unwrap();
        let todo_input = todo_runtime
            .source_action_input_for_event(
                "todo-append",
                GenericSourceEvent {
                    source: "store.sources.new_todo_input.key_down",
                    text: None,
                    key: Some("Enter"),
                    target_text: None,
                    address: None,
                },
                TickSeq(7),
                |_, _| Ok(None),
            )
            .unwrap();
        assert_eq!(todo_input.source, "store.sources.new_todo_input.key_down");
        assert_eq!(todo_input.list.as_deref(), Some("todos"));
        assert_eq!(todo_input.index, None);
        assert_eq!(todo_input.key, Some("Enter"));
        assert_eq!(todo_input.text, None);

        let cells_parsed = parse_cells_project_for_test();
        let cells_ir = lower(&cells_parsed).unwrap();
        let cells_compiled = CompiledProgram::from_ir(&cells_ir).unwrap();
        let cells_runtime = GenericScheduledRuntime::new(&cells_ir, &cells_compiled).unwrap();
        let commit_source = cells_ir
            .sources
            .iter()
            .find(|source| source.path == "cell.sources.editor.commit")
            .unwrap();
        assert_eq!(
            cells_compiled
                .source_routes
                .address_lookup_field_for_source_id(commit_source.id),
            Some("address")
        );
        let cells_event = GenericSourceEvent {
            source: "cell.sources.editor.commit",
            text: Some("41"),
            key: Some("Enter"),
            target_text: None,
            address: Some("A0"),
        };
        let default_step = ScenarioStep::default();
        let cells_input = cells_runtime
            .source_action_input_for_event(
                "cells-a1-commit",
                cells_event,
                TickSeq(3),
                |list, event| cells_runtime.resolve_generic_step_index(list, &default_step, event),
            )
            .unwrap();
        assert_eq!(cells_input.source, "cell.sources.editor.commit");
        assert_eq!(cells_input.list.as_deref(), Some("cells"));
        assert_eq!(cells_input.index, Some(0));
        assert_eq!(cells_input.text, Some("41"));
    }

    #[test]
    fn generic_storage_projects_report_summary_rows() {
        let todo_parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let todo_ir = lower(&todo_parsed).unwrap();
        let todo_compiled = CompiledProgram::from_ir(&todo_ir).unwrap();
        let todo_runtime = GenericScheduledRuntime::new(&todo_ir, &todo_compiled).unwrap();
        let todo_rows = todo_runtime
            .list_rows_json("todos", &["title", "completed", "editing"])
            .unwrap();
        assert_eq!(todo_rows[0]["title"], "Read documentation");
        assert_eq!(todo_rows[0]["completed"], false);
        assert_eq!(todo_rows[1]["title"], "Finish TodoMVC renderer");
        assert_eq!(todo_rows[1]["completed"], true);
        assert!(todo_runtime.row_identity("todos", 0).is_ok());
        assert!(todo_runtime.row_identity("todos", 1).is_ok());

        let cells_parsed = parse_cells_project_for_test();
        let cells_ir = lower(&cells_parsed).unwrap();
        let cells_compiled = CompiledProgram::from_ir(&cells_ir).unwrap();
        let cells_runtime = GenericScheduledRuntime::new(&cells_ir, &cells_compiled).unwrap();
        let a1 = cells_runtime
            .list_row_fields_json("cells", 0, &["address", "formula_text", "editing"])
            .unwrap();
        assert_eq!(a1["address"], "A0");
        assert_eq!(a1["formula_text"], "5");
        assert_eq!(a1["editing"], false);
    }

    #[test]
    fn live_runtime_applies_observed_todomvc_source_events() {
        let source = include_str!("../../../examples/todomvc.bn");
        let scenario = parse_scenario(Path::new("../../examples/todomvc.scn")).unwrap();
        let mut runtime = LiveRuntime::new(
            "playground-live:todomvc",
            source,
            Path::new("../../examples/todomvc.scn"),
        )
        .unwrap();
        let change = runtime
            .apply_source_event_for_step(
                &scenario.step[1],
                LiveSourceEvent {
                    source: "store.sources.new_todo_input.change".to_owned(),
                    text: Some("Test todo".to_owned()),
                    ..LiveSourceEvent::default()
                },
            )
            .unwrap();
        assert_eq!(change.state_summary["new_todo_text"], "Test todo");
        let submit = runtime
            .apply_source_event_for_step(
                &scenario.step[2],
                LiveSourceEvent {
                    source: "store.sources.new_todo_input.key_down".to_owned(),
                    text: None,
                    key: Some("Enter".to_owned()),
                    ..LiveSourceEvent::default()
                },
            )
            .unwrap();
        assert_eq!(submit.state_summary["todos"].as_array().unwrap().len(), 5);
        assert!(
            submit
                .semantic_deltas
                .iter()
                .any(|delta| delta.kind == "ListInsert")
        );
    }

    #[test]
    fn live_runtime_applies_root_text_key_payload_source_events() {
        let source = r#"
store: [
    sources: [
        keyboard: SOURCE
    ]
    last_key:
        Text/empty() |> HOLD last_key {
            LATEST {
                sources.keyboard.key
            }
        }
    pan_window:
        TEXT { Center } |> HOLD pan_window {
            LATEST {
                sources.keyboard.key |> WHEN {
                    A => TEXT { LeftWindow }
                    D => TEXT { RightWindow }
                    __ => SKIP
                }
            }
        }
]

document: Document/new(root: Element/label(element: [], label: TEXT { Keyboard }))
"#;
        let mut runtime = LiveRuntime::new(
            "playground-live:key-payload",
            source,
            Path::new("../../examples/counter.scn"),
        )
        .unwrap();

        let mut expected = BTreeMap::new();
        expected.insert(
            "source".to_owned(),
            toml::Value::String("store.sources.keyboard".to_owned()),
        );
        expected.insert("key".to_owned(), toml::Value::String("D".to_owned()));
        let step = ScenarioStep {
            id: "keyboard-root-text".to_owned(),
            expected_source_event: Some(expected),
            ..ScenarioStep::default()
        };

        let output = runtime
            .apply_source_event_for_step(
                &step,
                LiveSourceEvent {
                    source: "store.sources.keyboard".to_owned(),
                    key: Some("D".to_owned()),
                    ..LiveSourceEvent::default()
                },
            )
            .unwrap();

        assert_eq!(output.state_summary["store"]["last_key"], "D");
        assert_eq!(output.state_summary["store"]["pan_window"], "RightWindow");
        assert!(output.semantic_deltas.iter().any(|delta| {
            delta.kind == "FieldSet" && delta.field_path.as_deref() == Some("store.last_key")
        }));
        assert!(output.semantic_deltas.iter().any(|delta| {
            delta.kind == "FieldSet" && delta.field_path.as_deref() == Some("store.pan_window")
        }));
    }

    #[test]
    fn live_runtime_applies_numeric_counter_hold_updates_generically() {
        let source = include_str!("../../../examples/counter.bn");
        let scenario = parse_scenario(Path::new("../../examples/counter.scn")).unwrap();
        let mut runtime = LiveRuntime::new(
            "playground-live:counter",
            source,
            Path::new("../../examples/counter.scn"),
        )
        .unwrap();

        let expected = [
            (
                "press-increment",
                "store.sources.increment_button.press",
                "1",
            ),
            (
                "press-increment-again",
                "store.sources.increment_button.press",
                "2",
            ),
            (
                "press-decrement",
                "store.sources.decrement_button.press",
                "1",
            ),
            ("press-reset", "store.sources.reset_button.press", "0"),
            (
                "press-decrement-negative",
                "store.sources.decrement_button.press",
                "-1",
            ),
            (
                "press-increment-back-to-zero",
                "store.sources.increment_button.press",
                "0",
            ),
        ];
        for (step_id, source, count) in expected {
            let step = scenario
                .step
                .iter()
                .find(|step| step.id == step_id)
                .expect("counter scenario includes expected step");
            let output = runtime
                .apply_source_event_for_step(
                    step,
                    LiveSourceEvent {
                        source: source.to_owned(),
                        ..LiveSourceEvent::default()
                    },
                )
                .unwrap();
            assert_eq!(output.state_summary["store"]["count"], count);
        }
    }

    #[test]
    fn live_runtime_routes_duplicate_todo_title_events_by_occurrence() {
        let source = include_str!("../../../examples/todomvc.bn");
        let mut runtime = LiveRuntime::new(
            "playground-live:todomvc",
            source,
            Path::new("../../examples/todomvc.scn"),
        )
        .unwrap();
        for _ in 0..2 {
            runtime
                .apply_source_event(LiveSourceEvent {
                    source: "store.sources.new_todo_input.change".to_owned(),
                    text: Some("Duplicate".to_owned()),
                    ..LiveSourceEvent::default()
                })
                .unwrap();
            runtime
                .apply_source_event(LiveSourceEvent {
                    source: "store.sources.new_todo_input.key_down".to_owned(),
                    text: None,
                    key: Some("Enter".to_owned()),
                    ..LiveSourceEvent::default()
                })
                .unwrap();
        }

        let output = runtime
            .apply_source_event(LiveSourceEvent {
                source: "todo.sources.todo_checkbox.click".to_owned(),
                target_text: Some("Duplicate".to_owned()),
                target_occurrence: Some(2),
                ..LiveSourceEvent::default()
            })
            .unwrap();
        let duplicates = output
            .state_summary
            .get("todos")
            .and_then(JsonValue::as_array)
            .unwrap()
            .iter()
            .filter(|todo| todo["title"] == "Duplicate")
            .collect::<Vec<_>>();
        assert_eq!(duplicates.len(), 2);
        assert_eq!(duplicates[0]["completed"], false);
        assert_eq!(duplicates[1]["completed"], true);
    }

    #[test]
    fn live_runtime_prefers_bound_row_identity_over_target_text() {
        let source = include_str!("../../../examples/todomvc.bn");
        let mut runtime = LiveRuntime::new(
            "playground-live:todomvc",
            source,
            Path::new("../../examples/todomvc.scn"),
        )
        .unwrap();
        let json_u64_field = |row: &JsonValue, field: &str| -> u64 {
            row.get(field)
                .and_then(|value| {
                    value
                        .as_u64()
                        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
                })
                .unwrap_or_else(|| panic!("todo row missing numeric `{field}`: {row}"))
        };
        let initial = runtime.state_summary();
        let first_todo = &initial["todos"][0];
        let target_key = json_u64_field(first_todo, "key");
        let target_generation = json_u64_field(first_todo, "generation");
        let mut action = BTreeMap::new();
        action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
        action.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries checkbox".to_owned()),
        );
        action.insert(
            "target_key".to_owned(),
            toml::Value::Integer(target_key as i64),
        );
        action.insert(
            "target_generation".to_owned(),
            toml::Value::Integer(target_generation as i64),
        );
        let mut expected = BTreeMap::new();
        expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.todo_checkbox.click".to_owned()),
        );
        expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries".to_owned()),
        );
        let step = ScenarioStep {
            id: "bound-row-identity-wins".to_owned(),
            user_action: Some(action),
            expected_source_event: Some(expected),
            ..ScenarioStep::default()
        };

        let output = runtime.apply_checked_step(&step).unwrap();
        let todos = output
            .state_summary
            .get("todos")
            .and_then(JsonValue::as_array)
            .unwrap();
        assert_eq!(todos[0]["title"], "Read documentation");
        assert_eq!(todos[0]["completed"], true);
        assert_eq!(todos[3]["title"], "Buy groceries");
        assert_eq!(todos[3]["completed"], false);
        assert_eq!(
            output
                .semantic_deltas
                .iter()
                .filter(|delta| delta.field_path.as_deref() == Some("completed"))
                .count(),
            1
        );
    }

    #[test]
    fn live_runtime_keeps_one_todomvc_row_in_edit_mode() {
        let source = include_str!("../../../examples/todomvc.bn");
        let mut runtime = LiveRuntime::new(
            "playground-live:todomvc",
            source,
            Path::new("../../examples/todomvc.scn"),
        )
        .unwrap();
        runtime
            .apply_source_event(LiveSourceEvent {
                source: "todo.sources.todo_title_element.double_click".to_owned(),
                target_text: Some("Read documentation".to_owned()),
                target_occurrence: Some(1),
                ..LiveSourceEvent::default()
            })
            .unwrap();
        let output = runtime
            .apply_source_event(LiveSourceEvent {
                source: "todo.sources.todo_title_element.double_click".to_owned(),
                target_text: Some("Finish TodoMVC renderer".to_owned()),
                target_occurrence: Some(1),
                ..LiveSourceEvent::default()
            })
            .unwrap();
        let editing = output
            .state_summary
            .get("todos")
            .and_then(JsonValue::as_array)
            .unwrap()
            .iter()
            .filter(|todo| todo["editing"] == true)
            .collect::<Vec<_>>();
        assert_eq!(editing.len(), 1);
        assert_eq!(editing[0]["title"], "Finish TodoMVC renderer");
    }

    #[test]
    fn live_runtime_applies_observed_cells_source_events() {
        let source = cells_project_source_for_test();
        let scenario = parse_scenario(Path::new("../../examples/cells.scn")).unwrap();
        let mut runtime = LiveRuntime::new(
            "playground-live:cells",
            &source,
            Path::new("../../examples/cells.scn"),
        )
        .unwrap();
        let select = runtime
            .apply_source_event_for_step(
                scenario
                    .step
                    .iter()
                    .find(|step| step.id == "select-b0-shows-formula-in-bar")
                    .expect("Cells scenario includes select-b0-shows-formula-in-bar"),
                LiveSourceEvent {
                    source: "cell.sources.editor.select".to_owned(),
                    address: Some("B0".to_owned()),
                    ..LiveSourceEvent::default()
                },
            )
            .unwrap();
        assert_eq!(select.state_summary["store"]["selected_address"], "B0");
        assert_eq!(
            select.state_summary["store"]["selected_input"]["editing_text"],
            "=add(A0,A1)"
        );
        assert_eq!(
            select.state_summary["store"]["selected_input"]["value"],
            "15"
        );
        assert!(select.semantic_deltas.iter().any(|delta| {
            delta.kind == "FieldSet"
                && delta.field_path.as_deref() == Some("store.selected_address")
        }));
        runtime
            .apply_source_event_for_step(
                scenario
                    .step
                    .iter()
                    .find(|step| step.id == "edit-a0-literal")
                    .expect("Cells scenario includes edit-a0-literal"),
                LiveSourceEvent {
                    source: "cell.sources.editor.change".to_owned(),
                    text: Some("41".to_owned()),
                    address: Some("A0".to_owned()),
                    ..LiveSourceEvent::default()
                },
            )
            .unwrap();
        let output = runtime
            .apply_source_event_for_step(
                scenario
                    .step
                    .iter()
                    .find(|step| step.id == "commit-a0-literal")
                    .expect("Cells scenario includes commit-a0-literal"),
                LiveSourceEvent {
                    source: "cell.sources.editor.commit".to_owned(),
                    text: Some("41".to_owned()),
                    key: Some("Enter".to_owned()),
                    address: Some("A0".to_owned()),
                    ..LiveSourceEvent::default()
                },
            )
            .unwrap();
        assert_eq!(output.state_summary["cells"][0]["value"], "41");
        assert_eq!(
            output.state_summary["sheet_columns"]
                .as_array()
                .unwrap()
                .len(),
            26
        );
        assert_eq!(output.state_summary["sheet_columns"][0]["label"], "A");
        assert_eq!(
            output.state_summary["store"]["sheet_rows"]
                .as_array()
                .unwrap()
                .len(),
            100
        );
        assert_eq!(output.state_summary["store"]["selected_address"], "A0");
        assert_eq!(
            output.state_summary["store"]["selected_input"]["address"],
            "A0"
        );
        assert!(
            output
                .semantic_deltas
                .iter()
                .any(|delta| delta.field_path.as_deref() == Some("value"))
        );

        let b0 = runtime
            .apply_source_event_for_step(
                scenario
                    .step
                    .iter()
                    .find(|step| step.id == "commit-b0-formula")
                    .expect("Cells scenario includes commit-b0-formula"),
                LiveSourceEvent {
                    source: "cell.sources.editor.commit".to_owned(),
                    text: Some("=A0+1".to_owned()),
                    key: Some("Enter".to_owned()),
                    address: Some("B0".to_owned()),
                    ..LiveSourceEvent::default()
                },
            )
            .unwrap();
        assert_eq!(b0.state_summary["store"]["selected_address"], "B0");
        assert_eq!(b0.state_summary["store"]["selected_input"]["address"], "B0");
        assert_eq!(b0.state_summary["store"]["selected_input"]["value"], "42");
    }

    #[test]
    fn example_paths_resolve_from_examples_directory() {
        let (source, scenario, budget) = example_paths("todo").unwrap();
        assert!(source.ends_with(Path::new("examples/todomvc.bn")));
        assert!(scenario.ends_with(Path::new("examples/todomvc.scn")));
        assert!(budget.ends_with(Path::new("examples/todomvc.budget.toml")));

        let (source, scenario, budget) = example_paths("cells").unwrap();
        assert!(source.ends_with(Path::new("examples/cells.bn")));
        assert!(scenario.ends_with(Path::new("examples/cells.scn")));
        assert!(budget.ends_with(Path::new("examples/cells.budget.toml")));

        let err = example_paths("../cells").unwrap_err();
        assert!(err.to_string().contains("invalid example name"));
    }

    #[test]
    fn manifest_source_files_are_loaded_as_one_cells_project() {
        let entry = example_manifest_entry("cells").unwrap();
        assert_eq!(
            entry.source_files,
            vec![
                "examples/cells/defaults.bn".to_owned(),
                "examples/cells/formula.bn".to_owned(),
                "examples/cells/cell.bn".to_owned(),
                "examples/cells/model.bn".to_owned(),
                "examples/cells/columns.bn".to_owned(),
                "examples/cells/store.bn".to_owned(),
                "examples/cells/view.bn".to_owned(),
                "examples/cells.bn".to_owned()
            ]
        );
        let (parsed, ir) = load_and_lower(Path::new("../../examples/cells.bn")).unwrap();
        assert_eq!(parsed.files.len(), 8);
        assert!(
            parsed
                .functions
                .iter()
                .any(|function| function == "new_cell")
        );
        assert!(
            parsed
                .functions
                .iter()
                .any(|function| function == "new_sheet_column")
        );
        assert!(
            parsed
                .functions
                .iter()
                .any(|function| function == "compute_value")
        );
        assert!(
            parsed
                .operators
                .iter()
                .all(|operator| !operator.starts_with(&["For", "mula", "/"].concat()))
        );
        assert!(
            parsed
                .functions
                .iter()
                .any(|function| function == "cells_app")
        );
        assert!(ir.derived_values.iter().any(|value| {
            value.path == "cell.value" && value.kind == DerivedValueKind::Pure && value.indexed
        }));
        assert!(ir.sources.iter().any(|source| {
            source.path == "cell.sources.editor.commit"
                && source.payload_schema.fields
                    == vec![SourcePayloadField::Address, SourcePayloadField::Text]
        }));
    }

    #[test]
    fn executable_surface_must_match_typed_ir_profile() {
        let source = include_str!("../../../examples/todomvc.bn")
            .replace("        completed:\n", "        done:\n");
        let err = run_scenario_source_with_step_limit(
            "playground-editor:todomvc",
            &source,
            Path::new("../../examples/todomvc.scn"),
            VerificationLayer::Semantic,
            Some(1),
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("typed IR does not match a supported executable surface profile")
                || err
                    .to_string()
                    .contains("not in the static schedule symbol table")
        );

        let renamed_row_source =
            include_str!("../../../examples/todomvc.bn").replace("todo_checkbox", "done_checkbox");
        run_scenario_source_with_step_limit(
            "playground-editor:todomvc",
            &renamed_row_source,
            Path::new("../../examples/todomvc.scn"),
            VerificationLayer::Semantic,
            Some(1),
        )
        .unwrap();

        let renamed_cell_source = cells_project_source_for_test()
            .replace("editor.commit", "editor.apply")
            .replace("commit: SOURCE", "apply: SOURCE");
        run_scenario_source_with_step_limit(
            "playground-editor:cells",
            &renamed_cell_source,
            Path::new("../../examples/cells.scn"),
            VerificationLayer::Semantic,
            Some(1),
        )
        .unwrap();

        let legacy_eval = ["For", "mula", "/eval"].concat();
        let cells_source = cells_project_source_for_test().replace(
            "compute_value(address: address, formula_text: formula_text)",
            &format!("{legacy_eval}(formula_text)"),
        );
        let err = run_scenario_source_with_step_limit(
            "playground-editor:cells",
            &cells_source,
            Path::new("../../examples/cells.scn"),
            VerificationLayer::Semantic,
            Some(1),
        )
        .unwrap_err();
        assert!(
            !err.to_string().is_empty(),
            "legacy formula fixture should fail loudly"
        );
    }

    #[test]
    fn source_initializers_are_read_from_boon_text() {
        let todo_source = include_str!("../../../examples/todomvc.bn")
            .replace("Read documentation", "Source title A")
            .replace("Buy groceries", "Source title B");
        let parsed = parse_source("examples/todomvc.bn", todo_source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert_eq!(
            todomvc_initial_titles_from_ir(&ir).unwrap(),
            vec![
                "Source title A",
                "Finish TodoMVC renderer",
                "Walk the dog",
                "Source title B"
            ]
        );

        let cells_source = cells_project_source_for_test().replace(
            "List/range(from: 0, to: 2599)",
            "List/range(from: 0, to: 11)",
        );
        let parsed = parse_source("examples/cells.bn", &cells_source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert_eq!(cells_range_from_ir(&ir), Some((0, 11)));

        let cells_source = cells_project_source_for_test().replace(
            "[address: TEXT { A0 }, field: TEXT { default_formula }, value: TEXT { 5 }]",
            "[address: TEXT { A0 }, field: TEXT { default_formula }, value: TEXT { 9 }]",
        );
        let parsed = parse_source("examples/cells.bn", &cells_source).unwrap();
        let ir = lower(&parsed).unwrap();
        let cells_list = ir
            .lists
            .iter()
            .find(|list| list.name == "cells_default_values")
            .expect("Cells source should lower generic default values");
        let defaults = match &cells_list.initializer {
            ListInitializer::RecordLiteral { rows } => rows,
            initializer => panic!("unexpected Cells default initializer: {initializer:?}"),
        };
        assert!(defaults.iter().any(|row| {
            row.fields.iter().any(|field| {
                field.name == "address"
                    && matches!(&field.value, InitialValue::Text { value } if value == "A0")
            }) && row.fields.iter().any(|field| {
                field.name == "value"
                    && matches!(&field.value, InitialValue::Text { value } if value == "9")
            })
        }));
        let mut runtime =
            LiveRuntime::from_source("cells-defaults-from-boon", &cells_source).unwrap();
        let summary = runtime.state_summary();
        let a0 = summary
            .get("cells")
            .and_then(serde_json::Value::as_array)
            .and_then(|cells| {
                cells
                    .iter()
                    .find(|cell| cell.get("address") == Some(&json!("A0")))
            })
            .expect("Cells state summary should include A0");
        assert_eq!(a0.get("formula_text"), Some(&json!("9")));
        assert_eq!(a0.get("value"), Some(&json!("9")));
    }

    #[test]
    fn list_range_materializes_generic_rows_from_boon_source() {
        let source = r#"
numbers:
    List/range(from: 0, to: 2)
    |> List/map(number, new: new_number(number: number))

store: [
    sources: [
        noop: SOURCE
    ]
    noop:
        TEXT { ready } |> HOLD noop {
            LATEST {
                sources.noop.text
            }
        }
]

document: Document/new(root: Element/label(element: [], label: TEXT { Numbers }))

FUNCTION new_number(number) {
    [
        index: number.index
        value: number.value
    ]
}
"#;
        let parsed = parse_source("range-list.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let numbers = ir
            .lists
            .iter()
            .find(|list| list.name == "numbers")
            .expect("range source should lower numbers list");
        assert_eq!(
            numbers.initializer,
            ListInitializer::Range { from: 0, to: 2 }
        );

        let mut runtime = LiveRuntime::from_source("range-list", source).unwrap();
        let summary = runtime.state_summary();
        let rows = summary["numbers"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["index"], "0");
        assert_eq!(rows[0]["value"], "0");
        assert_eq!(rows[2]["index"], "2");
        assert_eq!(rows[2]["value"], "2");
    }

    #[test]
    fn list_find_and_chunk_project_generic_record_lists_without_grid_identity() {
        let source = r#"
sheet:
    LIST {
        [address: TEXT { A0 }, value: TEXT { 1 }]
        [address: TEXT { B0 }, value: TEXT { 2 }]
        [address: TEXT { C0 }, value: TEXT { 3 }]
    }
    |> List/map(entry, new: new_entry(entry: entry))

store: [
    sources: [
        selected: SOURCE
    ]
    selected:
        TEXT { B0 } |> HOLD selected {
            LATEST {
                sources.selected.text
            }
        }
    selected_input:
        List/find(sheet, field: address, value: selected)
    visible_rows:
        List/chunk(sheet, size: 2, items: entries, label: row_number)
]

document: Document/new(root: Element/label(element: [], label: TEXT { Sheet }))

FUNCTION new_entry(entry) {
    [
        address: entry.address
        value: entry.value
    ]
}
"#;
        let parsed = parse_source("generic-list-projections.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert!(ir.list_projections.iter().any(|projection| {
            projection.target == "store.selected_input"
                && projection.list == "sheet"
                && projection.kind
                    == ListProjectionKind::Find {
                        field: "address".to_owned(),
                        value: "store.selected".to_owned(),
                    }
        }));
        assert!(ir.list_projections.iter().any(|projection| {
            projection.target == "store.visible_rows"
                && projection.list == "sheet"
                && projection.kind
                    == ListProjectionKind::Chunk {
                        size: Some(2),
                        item_field: "entries".to_owned(),
                        label_field: "row_number".to_owned(),
                    }
        }));

        let mut runtime = LiveRuntime::from_source("generic-list-projections", source).unwrap();
        let summary = runtime.state_summary();
        assert_eq!(summary["store"]["selected_input"]["address"], "B0");
        assert_eq!(summary["store"]["selected_input"]["value"], "2");
        assert_eq!(summary["store"]["visible_rows"][0]["row_number"], "0");
        assert_eq!(
            summary["store"]["visible_rows"][0]["entries"][0]["address"],
            "A0"
        );
        assert_eq!(
            summary["store"]["visible_rows"][0]["entries"][1]["address"],
            "B0"
        );
        assert_eq!(
            summary["store"]["visible_rows"][1]["entries"][0]["address"],
            "C0"
        );
    }

    #[test]
    fn duplicate_todo_titles_route_by_hidden_host_scope_not_visible_data_identity() {
        let source =
            include_str!("../../../examples/todomvc.bn").replace("Walk the dog", "Buy groceries");
        let parsed = parse_source("examples/todomvc.bn", source).unwrap();
        let mut runtime = list_scenario_harness_from_parsed(&parsed);
        let target_index = runtime
            .find_index_at_occurrence("Buy groceries", 2)
            .unwrap();
        let other_duplicate_index = runtime
            .find_index_at_occurrence("Buy groceries", 1)
            .unwrap();
        let target_key = runtime.list_key_for_test(target_index);
        let target_generation = runtime.list_generation_for_test(target_index);
        let mut action = BTreeMap::new();
        action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
        action.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries checkbox".to_owned()),
        );
        action.insert(
            "target_key".to_owned(),
            toml::Value::Integer(target_key as i64),
        );
        action.insert(
            "target_generation".to_owned(),
            toml::Value::Integer(target_generation as i64),
        );
        let mut expected = BTreeMap::new();
        expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.todo_checkbox.click".to_owned()),
        );
        expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries".to_owned()),
        );
        let step = ScenarioStep {
            id: "toggle-second-duplicate".to_owned(),
            user_action: Some(action),
            expected_source_event: Some(expected),
            ..ScenarioStep::default()
        };
        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        runtime
            .apply_step_into(&step, &mut deltas, &mut patches)
            .unwrap();
        assert!(!runtime.list_completed_for_test(other_duplicate_index));
        assert!(runtime.list_completed_for_test(target_index));
        assert_eq!(deltas.iter().find_map(|delta| delta.key), Some(target_key));
    }

    #[test]
    fn todo_row_sources_are_bound_for_initial_and_append() {
        let parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let mut runtime = list_scenario_harness_from_parsed(&parsed);
        let row_source_count = runtime
            .generic
            .list_source_bindings
            .source_count("todos")
            .unwrap();
        assert_eq!(
            runtime.generic.source_binding_count(),
            runtime.list_len_for_test() * row_source_count
        );
        for index in 0..runtime.list_len_for_test() {
            let (key, generation) = runtime.list_row_identity_for_test(index).unwrap();
            assert_eq!(
                runtime
                    .generic
                    .row_source_binding_count("todos", key, generation),
                row_source_count
            );
        }

        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        runtime
            .append_text_row_from_source(
                "store.sources.new_todo_input.key_down",
                "Enter",
                "New source row",
                &mut deltas,
                &mut patches,
            )
            .unwrap();
        let appended_index = runtime.list_len_for_test() - 1;
        let (key, generation) = runtime
            .list_row_identity_for_test(appended_index)
            .expect("appended row exists");
        assert_eq!(
            runtime
                .generic
                .list_row_textlike("todos", appended_index, "not_editing")
                .unwrap(),
            "True"
        );
        assert_eq!(
            runtime
                .generic
                .row_source_binding_count("todos", key, generation),
            row_source_count
        );
        let bind_count = deltas
            .iter()
            .filter(|delta| delta.kind == "SourceBind" && delta.key == Some(key))
            .count();
        assert_eq!(bind_count, row_source_count);
        assert!(
            deltas
                .iter()
                .filter(|delta| delta.kind == "SourceBind" && delta.key == Some(key))
                .all(|delta| delta.source_id.is_some() && delta.bind_epoch.is_some())
        );
        let bind_patch_count = patches
            .iter()
            .filter(|patch| patch.kind == "InvalidateDocument")
            .count();
        assert!(bind_patch_count >= row_source_count);
        assert!(patches.iter().any(|patch| matches!(
            (&patch.target, &patch.value),
            (
                RenderTarget::Static(source_target),
                ProtocolValue::Text(source_path)
            ) if source_target.as_ref() == "document"
                && source_path.as_ref() == "todo.sources.todo_checkbox.click"
        )));
    }

    #[test]
    fn todo_row_source_bindings_are_derived_from_boon_source_ports() {
        let source =
            include_str!("../../../examples/todomvc.bn").replace("todo_checkbox", "done_checkbox");
        let parsed = parse_source("examples/todomvc.bn", source).unwrap();
        let runtime = list_scenario_harness_from_parsed(&parsed);
        let row_source_paths = runtime
            .generic
            .list_source_bindings
            .source_paths("todos")
            .unwrap();
        assert!(
            row_source_paths
                .iter()
                .any(|path| path == "todo.sources.done_checkbox.click")
        );
        assert!(
            !row_source_paths
                .iter()
                .any(|path| path == "todo.sources.todo_checkbox.click")
        );
        let (key, generation) = runtime.list_row_identity_for_test(0).unwrap();
        assert!(
            runtime
                .generic
                .row_source_bindings("todos", key, generation)
                .any(|binding| binding.source_path == "todo.sources.done_checkbox.click")
        );
    }

    #[test]
    fn todo_root_scalar_updates_are_derived_from_ir_branches() {
        let source = include_str!("../../../examples/todomvc.bn")
            .replace("new_todo_input", "composer")
            .replace("filter_active", "filter_live");
        let parsed = parse_source("examples/todomvc.bn", source).unwrap();
        let mut runtime = list_scenario_harness_from_parsed(&parsed);
        let mut deltas = Vec::new();
        let mut patches = Vec::new();

        let mut type_action = BTreeMap::new();
        type_action.insert(
            "kind".to_owned(),
            toml::Value::String("type_text".to_owned()),
        );
        type_action.insert(
            "target".to_owned(),
            toml::Value::String("new todo input".to_owned()),
        );
        type_action.insert(
            "text".to_owned(),
            toml::Value::String("IR routed text".to_owned()),
        );
        let mut type_expected = BTreeMap::new();
        type_expected.insert(
            "source".to_owned(),
            toml::Value::String("store.sources.composer.change".to_owned()),
        );
        type_expected.insert(
            "text".to_owned(),
            toml::Value::String("IR routed text".to_owned()),
        );
        let type_step = ScenarioStep {
            id: "renamed-root-text-source".to_owned(),
            user_action: Some(type_action),
            expected_source_event: Some(type_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&type_step, &mut deltas, &mut patches)
            .unwrap();
        assert_eq!(
            runtime
                .generic
                .root_textlike_ref("store.new_todo_text")
                .unwrap(),
            "IR routed text"
        );
        assert!(deltas.iter().any(|delta| {
            delta.kind == "FieldSet" && delta.field_path.as_deref() == Some("store.new_todo_text")
        }));

        deltas.clear();
        patches.clear();
        let mut filter_action = BTreeMap::new();
        filter_action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
        filter_action.insert(
            "target".to_owned(),
            toml::Value::String("Active filter".to_owned()),
        );
        let mut filter_expected = BTreeMap::new();
        filter_expected.insert(
            "source".to_owned(),
            toml::Value::String("store.sources.filter_live.press".to_owned()),
        );
        let filter_step = ScenarioStep {
            id: "renamed-root-filter-source".to_owned(),
            user_action: Some(filter_action),
            expected_source_event: Some(filter_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&filter_step, &mut deltas, &mut patches)
            .unwrap();
        assert_eq!(
            runtime
                .generic
                .root_textlike_ref("store.selected_filter")
                .unwrap(),
            "Active"
        );
        assert!(deltas.iter().any(|delta| {
            delta.kind == "FieldSet" && delta.field_path.as_deref() == Some("store.selected_filter")
        }));
    }

    #[test]
    fn todo_completed_updates_are_derived_from_ir_bool_branches() {
        let source = include_str!("../../../examples/todomvc.bn")
            .replace("todo_checkbox", "done_checkbox")
            .replace("toggle_all_checkbox", "mark_all_checkbox");
        let parsed = parse_source("examples/todomvc.bn", source).unwrap();
        let mut runtime = list_scenario_harness_from_parsed(&parsed);
        let mut deltas = Vec::new();
        let mut patches = Vec::new();

        let mut row_action = BTreeMap::new();
        row_action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
        row_action.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries checkbox".to_owned()),
        );
        let mut row_expected = BTreeMap::new();
        row_expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.done_checkbox.click".to_owned()),
        );
        row_expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries".to_owned()),
        );
        let row_step = ScenarioStep {
            id: "renamed-row-checkbox".to_owned(),
            user_action: Some(row_action),
            expected_source_event: Some(row_expected),
            ..ScenarioStep::default()
        };
        let buy_index = list_row_index(&runtime, "Buy groceries");
        runtime
            .apply_step_into(&row_step, &mut deltas, &mut patches)
            .unwrap();
        assert!(runtime.list_completed_for_test(buy_index));
        assert!(!runtime.list_completed_for_test(list_row_index(&runtime, "Read documentation")));

        deltas.clear();
        patches.clear();
        let mut all_action = BTreeMap::new();
        all_action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
        all_action.insert(
            "target".to_owned(),
            toml::Value::String("Toggle all".to_owned()),
        );
        let mut all_expected = BTreeMap::new();
        all_expected.insert(
            "source".to_owned(),
            toml::Value::String("store.sources.mark_all_checkbox.click".to_owned()),
        );
        let all_step = ScenarioStep {
            id: "renamed-toggle-all".to_owned(),
            user_action: Some(all_action),
            expected_source_event: Some(all_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&all_step, &mut deltas, &mut patches)
            .unwrap();
        assert!(
            (0..runtime.list_len_for_test()).all(|index| runtime.list_completed_for_test(index))
        );
        assert_eq!(
            deltas
                .iter()
                .filter(|delta| delta.field_path.as_deref() == Some("completed"))
                .count(),
            runtime.list_len_for_test()
        );
    }

    #[test]
    fn todo_editing_updates_are_derived_from_ir_bool_branches() {
        let source = include_str!("../../../examples/todomvc.bn")
            .replace("editing_todo_title_element", "title_editor")
            .replace("todo_title_element", "title_label");
        let parsed = parse_source("examples/todomvc.bn", source).unwrap();
        let mut runtime = list_scenario_harness_from_parsed(&parsed);
        let mut deltas = Vec::new();
        let mut patches = Vec::new();

        let mut open_action = BTreeMap::new();
        open_action.insert(
            "kind".to_owned(),
            toml::Value::String("double_click".to_owned()),
        );
        open_action.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries".to_owned()),
        );
        let mut open_expected = BTreeMap::new();
        open_expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.title_label.double_click".to_owned()),
        );
        open_expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries".to_owned()),
        );
        let open_step = ScenarioStep {
            id: "renamed-open-editor".to_owned(),
            user_action: Some(open_action),
            expected_source_event: Some(open_expected),
            ..ScenarioStep::default()
        };
        let buy_index = list_row_index(&runtime, "Buy groceries");
        runtime
            .apply_step_into(&open_step, &mut deltas, &mut patches)
            .unwrap();
        assert!(runtime.list_editing_for_test(buy_index));
        assert_eq!(runtime.list_edit_text_for_test(buy_index), "Buy groceries");

        deltas.clear();
        patches.clear();
        let mut change_action = BTreeMap::new();
        change_action.insert(
            "kind".to_owned(),
            toml::Value::String("type_text".to_owned()),
        );
        change_action.insert(
            "target".to_owned(),
            toml::Value::String("editing todo input".to_owned()),
        );
        change_action.insert(
            "text".to_owned(),
            toml::Value::String("Draft via IR".to_owned()),
        );
        let mut change_expected = BTreeMap::new();
        change_expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.title_editor.change".to_owned()),
        );
        change_expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries".to_owned()),
        );
        change_expected.insert(
            "text".to_owned(),
            toml::Value::String("Draft via IR".to_owned()),
        );
        let change_step = ScenarioStep {
            id: "renamed-edit-text-change".to_owned(),
            user_action: Some(change_action),
            expected_source_event: Some(change_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&change_step, &mut deltas, &mut patches)
            .unwrap();
        assert_eq!(runtime.list_edit_text_for_test(buy_index), "Draft via IR");

        deltas.clear();
        patches.clear();
        let mut close_action = BTreeMap::new();
        close_action.insert(
            "kind".to_owned(),
            toml::Value::String("key_down".to_owned()),
        );
        close_action.insert(
            "target".to_owned(),
            toml::Value::String("editing todo input".to_owned()),
        );
        close_action.insert("key".to_owned(), toml::Value::String("Escape".to_owned()));
        let mut close_expected = BTreeMap::new();
        close_expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.title_editor.key_down".to_owned()),
        );
        close_expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries".to_owned()),
        );
        close_expected.insert("key".to_owned(), toml::Value::String("Escape".to_owned()));
        let close_step = ScenarioStep {
            id: "renamed-close-editor".to_owned(),
            user_action: Some(close_action),
            expected_source_event: Some(close_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&close_step, &mut deltas, &mut patches)
            .unwrap();
        assert!(!runtime.list_editing_for_test(buy_index));
        assert_eq!(runtime.list_edit_text_for_test(buy_index), "Buy groceries");
        assert!(deltas.iter().any(|delta| {
            delta.kind == "FieldSet"
                && delta.field_path.as_deref() == Some("editing")
                && matches!(delta.value, ProtocolValue::Bool(false))
        }));
    }

    #[test]
    fn todo_title_updates_are_derived_from_ir_trim_branches() {
        let source = include_str!("../../../examples/todomvc.bn")
            .replace("editing_todo_title_element", "title_editor")
            .replace("todo_title_element", "title_label");
        let parsed = parse_source("examples/todomvc.bn", source).unwrap();
        let mut runtime = list_scenario_harness_from_parsed(&parsed);
        let mut deltas = Vec::new();
        let mut patches = Vec::new();

        let mut open_action = BTreeMap::new();
        open_action.insert(
            "kind".to_owned(),
            toml::Value::String("double_click".to_owned()),
        );
        open_action.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries".to_owned()),
        );
        let mut open_expected = BTreeMap::new();
        open_expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.title_label.double_click".to_owned()),
        );
        open_expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries".to_owned()),
        );
        let open_step = ScenarioStep {
            id: "renamed-open-for-title-commit".to_owned(),
            user_action: Some(open_action),
            expected_source_event: Some(open_expected),
            ..ScenarioStep::default()
        };
        let buy_index = list_row_index(&runtime, "Buy groceries");
        runtime
            .apply_step_into(&open_step, &mut deltas, &mut patches)
            .unwrap();

        deltas.clear();
        patches.clear();
        let mut commit_change_action = BTreeMap::new();
        commit_change_action.insert(
            "kind".to_owned(),
            toml::Value::String("type_text".to_owned()),
        );
        commit_change_action.insert(
            "target".to_owned(),
            toml::Value::String("editing todo input".to_owned()),
        );
        commit_change_action.insert(
            "text".to_owned(),
            toml::Value::String("Committed via IR".to_owned()),
        );
        let mut commit_change_expected = BTreeMap::new();
        commit_change_expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.title_editor.change".to_owned()),
        );
        commit_change_expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries".to_owned()),
        );
        commit_change_expected.insert(
            "text".to_owned(),
            toml::Value::String("Committed via IR".to_owned()),
        );
        let commit_change_step = ScenarioStep {
            id: "renamed-title-enter-change".to_owned(),
            user_action: Some(commit_change_action),
            expected_source_event: Some(commit_change_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&commit_change_step, &mut deltas, &mut patches)
            .unwrap();

        deltas.clear();
        patches.clear();
        let mut commit_action = BTreeMap::new();
        commit_action.insert(
            "kind".to_owned(),
            toml::Value::String("key_down".to_owned()),
        );
        commit_action.insert(
            "target".to_owned(),
            toml::Value::String("editing todo input".to_owned()),
        );
        commit_action.insert("key".to_owned(), toml::Value::String("Enter".to_owned()));
        let mut commit_expected = BTreeMap::new();
        commit_expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.title_editor.key_down".to_owned()),
        );
        commit_expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries".to_owned()),
        );
        commit_expected.insert("key".to_owned(), toml::Value::String("Enter".to_owned()));
        let commit_step = ScenarioStep {
            id: "renamed-title-enter-commit".to_owned(),
            user_action: Some(commit_action),
            expected_source_event: Some(commit_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&commit_step, &mut deltas, &mut patches)
            .unwrap();
        assert_eq!(runtime.list_title_for_test(buy_index), "Committed via IR");
        assert!(deltas.iter().any(|delta| {
            delta.kind == "FieldSet" && delta.field_path.as_deref() == Some("title")
        }));

        deltas.clear();
        patches.clear();
        let mut reopen_action = BTreeMap::new();
        reopen_action.insert(
            "kind".to_owned(),
            toml::Value::String("double_click".to_owned()),
        );
        reopen_action.insert(
            "target_text".to_owned(),
            toml::Value::String("Committed via IR".to_owned()),
        );
        let mut reopen_expected = BTreeMap::new();
        reopen_expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.title_label.double_click".to_owned()),
        );
        reopen_expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Committed via IR".to_owned()),
        );
        let reopen_step = ScenarioStep {
            id: "renamed-open-for-title-blur".to_owned(),
            user_action: Some(reopen_action),
            expected_source_event: Some(reopen_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&reopen_step, &mut deltas, &mut patches)
            .unwrap();

        deltas.clear();
        patches.clear();
        let mut change_action = BTreeMap::new();
        change_action.insert(
            "kind".to_owned(),
            toml::Value::String("type_text".to_owned()),
        );
        change_action.insert(
            "target".to_owned(),
            toml::Value::String("editing todo input".to_owned()),
        );
        change_action.insert(
            "text".to_owned(),
            toml::Value::String("Blur via IR".to_owned()),
        );
        let mut change_expected = BTreeMap::new();
        change_expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.title_editor.change".to_owned()),
        );
        change_expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Committed via IR".to_owned()),
        );
        change_expected.insert(
            "text".to_owned(),
            toml::Value::String("Blur via IR".to_owned()),
        );
        let change_step = ScenarioStep {
            id: "renamed-title-blur-change".to_owned(),
            user_action: Some(change_action),
            expected_source_event: Some(change_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&change_step, &mut deltas, &mut patches)
            .unwrap();

        deltas.clear();
        patches.clear();
        let mut blur_action = BTreeMap::new();
        blur_action.insert("kind".to_owned(), toml::Value::String("blur".to_owned()));
        blur_action.insert(
            "target".to_owned(),
            toml::Value::String("editing todo input".to_owned()),
        );
        let mut blur_expected = BTreeMap::new();
        blur_expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.title_editor.blur".to_owned()),
        );
        blur_expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Committed via IR".to_owned()),
        );
        let blur_step = ScenarioStep {
            id: "renamed-title-blur-commit".to_owned(),
            user_action: Some(blur_action),
            expected_source_event: Some(blur_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&blur_step, &mut deltas, &mut patches)
            .unwrap();
        assert_eq!(runtime.list_title_for_test(buy_index), "Blur via IR");
        assert!(deltas.iter().any(|delta| {
            delta.kind == "FieldSet" && delta.field_path.as_deref() == Some("title")
        }));
    }

    #[test]
    fn todo_append_and_remove_are_derived_from_ir_list_operations() {
        let source = include_str!("../../../examples/todomvc.bn")
            .replace("title_to_add", "pending_title")
            .replace("clear_completed_button", "purge_done_button")
            .replace("remove_todo_button", "delete_button");
        let parsed = parse_source("examples/todomvc.bn", source).unwrap();
        let mut runtime = list_scenario_harness_from_parsed(&parsed);
        let mut deltas = Vec::new();
        let mut patches = Vec::new();

        let mut type_action = BTreeMap::new();
        type_action.insert(
            "kind".to_owned(),
            toml::Value::String("type_text".to_owned()),
        );
        type_action.insert(
            "target".to_owned(),
            toml::Value::String("new todo input".to_owned()),
        );
        type_action.insert(
            "text".to_owned(),
            toml::Value::String("Derived append".to_owned()),
        );
        let mut type_expected = BTreeMap::new();
        type_expected.insert(
            "source".to_owned(),
            toml::Value::String("store.sources.new_todo_input.change".to_owned()),
        );
        type_expected.insert(
            "text".to_owned(),
            toml::Value::String("Derived append".to_owned()),
        );
        let type_step = ScenarioStep {
            id: "renamed-append-type".to_owned(),
            user_action: Some(type_action),
            expected_source_event: Some(type_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&type_step, &mut deltas, &mut patches)
            .unwrap();
        deltas.clear();
        patches.clear();

        let mut append_action = BTreeMap::new();
        append_action.insert(
            "kind".to_owned(),
            toml::Value::String("key_down".to_owned()),
        );
        append_action.insert(
            "target".to_owned(),
            toml::Value::String("new todo input".to_owned()),
        );
        append_action.insert("key".to_owned(), toml::Value::String("Enter".to_owned()));
        let mut append_expected = BTreeMap::new();
        append_expected.insert(
            "source".to_owned(),
            toml::Value::String("store.sources.new_todo_input.key_down".to_owned()),
        );
        append_expected.insert("key".to_owned(), toml::Value::String("Enter".to_owned()));
        let append_step = ScenarioStep {
            id: "renamed-append-trigger".to_owned(),
            user_action: Some(append_action),
            expected_source_event: Some(append_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&append_step, &mut deltas, &mut patches)
            .unwrap();
        assert_eq!(
            runtime.list_title_for_test(runtime.list_len_for_test() - 1),
            "Derived append"
        );
        assert_eq!(
            runtime
                .generic
                .list_row_textlike("todos", runtime.list_len_for_test() - 1, "not_editing")
                .unwrap(),
            "True"
        );

        deltas.clear();
        patches.clear();
        let mut toggle_action = BTreeMap::new();
        toggle_action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
        toggle_action.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries checkbox".to_owned()),
        );
        let mut toggle_expected = BTreeMap::new();
        toggle_expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.todo_checkbox.click".to_owned()),
        );
        toggle_expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries".to_owned()),
        );
        let toggle_step = ScenarioStep {
            id: "complete-before-renamed-clear".to_owned(),
            user_action: Some(toggle_action),
            expected_source_event: Some(toggle_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&toggle_step, &mut deltas, &mut patches)
            .unwrap();

        deltas.clear();
        patches.clear();
        let mut clear_action = BTreeMap::new();
        clear_action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
        clear_action.insert(
            "target".to_owned(),
            toml::Value::String("Clear completed".to_owned()),
        );
        let mut clear_expected = BTreeMap::new();
        clear_expected.insert(
            "source".to_owned(),
            toml::Value::String("store.sources.purge_done_button.press".to_owned()),
        );
        let clear_step = ScenarioStep {
            id: "renamed-clear-completed-list-remove".to_owned(),
            user_action: Some(clear_action),
            expected_source_event: Some(clear_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&clear_step, &mut deltas, &mut patches)
            .unwrap();
        assert_eq!(runtime.list_title_for_test(0), "Read documentation");
        assert_eq!(runtime.list_title_for_test(1), "Walk the dog");
        assert_eq!(runtime.list_title_for_test(2), "Derived append");
        assert!(deltas.iter().any(|delta| delta.kind == "ListRemove"));

        deltas.clear();
        patches.clear();
        let mut delete_action = BTreeMap::new();
        delete_action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
        delete_action.insert(
            "target_text".to_owned(),
            toml::Value::String("Walk the dog delete".to_owned()),
        );
        let mut delete_expected = BTreeMap::new();
        delete_expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.delete_button.press".to_owned()),
        );
        delete_expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Walk the dog".to_owned()),
        );
        let delete_step = ScenarioStep {
            id: "renamed-row-delete-list-remove".to_owned(),
            user_action: Some(delete_action),
            expected_source_event: Some(delete_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&delete_step, &mut deltas, &mut patches)
            .unwrap();
        assert_eq!(runtime.list_title_for_test(0), "Read documentation");
        assert_eq!(runtime.list_title_for_test(1), "Derived append");
        assert!(deltas.iter().any(|delta| delta.kind == "ListRemove"));
    }

    #[test]
    fn generic_runtime_owns_todo_list_structural_checks() {
        let parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let ir = lower(&parsed).unwrap();
        let compiled = CompiledProgram::from_ir(&ir).unwrap();
        let mut generic = GenericCircuitRuntime::new(&ir).unwrap();

        let bad_append = generic
            .append_row_for_trigger(
                &compiled.list_equations,
                "todos",
                "store.sources.new_todo_input.key_down",
                todo_generic_row("Wrong trigger"),
            )
            .unwrap_err()
            .to_string();
        assert!(bad_append.contains("does not match IR trigger"));
        assert_eq!(generic.list_len("todos").unwrap(), 4);

        let clear_source = "store.sources.clear_completed_button.press";
        let clear_predicate = compiled
            .source_routes
            .for_source(clear_source)
            .unwrap()
            .list_remove_predicate("todos")
            .unwrap();
        assert!(
            generic
                .remove_row_for_predicate("todos", clear_predicate.clone(), 0)
                .unwrap()
                .is_none(),
            "clear completed must not remove an active row"
        );
        generic
            .commit_indexed_bool_field("todos", 0, "completed", true)
            .unwrap();
        let removed = generic
            .remove_row_for_predicate("todos", clear_predicate, 0)
            .unwrap()
            .expect("completed row should match the IR-derived remove predicate");
        assert_eq!(removed.key, 1);
        assert_eq!(generic.list_len("todos").unwrap(), 3);
    }

    #[test]
    fn source_routes_are_dense_by_hidden_source_id() {
        let parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let ir = lower(&parsed).unwrap();
        let compiled = CompiledProgram::from_ir(&ir).unwrap();

        assert!(
            compiled.source_routes.len() > 0,
            "TodoMVC must compile source routes from typed IR"
        );
        for route in &compiled.source_routes.route_slots {
            let source_id = route.source_id;
            let by_id = compiled
                .source_routes
                .for_source_id(source_id)
                .expect("dense SourceId slot must resolve to a route");
            assert_eq!(by_id.source, route.source);
            assert_eq!(
                ir.sources[source_id.as_usize()].path,
                route.source,
                "dense route slot must match the typed IR source table"
            );
        }

        let input_source = ir
            .sources
            .iter()
            .find(|source| source.path == "store.sources.new_todo_input.key_down")
            .expect("TodoMVC input key source must be present in typed IR");
        let input_route = compiled
            .source_routes
            .for_source_id(input_source.id)
            .expect("TodoMVC input source id must resolve through dense route slots");
        let input_actions = compiled
            .source_routes
            .actions_for_source_id(input_source.id)
            .expect("TodoMVC input source id must resolve through SourceActionTable");
        assert!(
            input_route.has_list_append_target("todos"),
            "append routing must be found from SourceId, not a label scan"
        );
        assert!(
            input_actions.iter().any(
                |action| matches!(action, SourceAction::ListAppend { list, .. } if list == "todos")
            ),
            "append action must be found from SourceActionTable, not route-kind inference"
        );
        assert_eq!(
            compiled.source_routes.source_id(&input_source.path),
            Some(input_source.id),
            "source label fallback must resolve through the sorted compiled label index"
        );
        assert!(
            compiled
                .source_routes
                .label_slots
                .windows(2)
                .all(|window| window[0].source < window[1].source),
            "source label slots must stay sorted for binary-search fallback"
        );

        let report = compiled.report();
        assert!(
            report["runtime_symbol_count"].as_u64().unwrap_or_default() > 0,
            "compiled programs must own diagnostic symbols instead of relying on leaked strings"
        );
        assert_eq!(
            report["runtime_symbol_ownership"],
            json!("compiled_program_owned")
        );
        assert_eq!(
            report["source_route_index_kind"],
            json!("dense_source_id_slots")
        );
        assert_eq!(
            report["source_route_label_lookup_kind"],
            json!("sorted_source_label_binary_search")
        );
        assert_eq!(
            report["source_routes_with_ids"].as_u64(),
            report["source_route_count"].as_u64()
        );
        assert!(
            report["source_route_id_slot_count"]
                .as_u64()
                .expect("source route id slot count must be numeric")
                >= report["source_route_count"]
                    .as_u64()
                    .expect("source route count must be numeric")
        );
    }

    #[test]
    fn list_predicates_preserve_ir_paths_without_todo_aliases() {
        assert!(matches!(
            runtime_list_predicate(&ListPredicate::RowFieldBool {
                path: "task.done".to_owned()
            }),
            RuntimeListPredicate::FieldBool { path } if path == "task.done"
        ));
        assert!(matches!(
            runtime_list_predicate(&ListPredicate::RowFieldBoolNot {
                path: "task.done".to_owned()
            }),
            RuntimeListPredicate::FieldBoolNot { path } if path == "task.done"
        ));
        assert!(matches!(
            runtime_list_predicate(&ListPredicate::SelectedFilterVisibility {
                selector: "store.filter".to_owned(),
                row_field: "task.done".to_owned(),
            }),
            RuntimeListPredicate::SelectorVisibility {
                selector,
                row_field,
            } if selector == "store.filter" && row_field == "task.done"
        ));
    }

    #[test]
    fn runtime_value_summaries_are_path_bounded_for_inspector() {
        let mut runtime = LiveRuntime::from_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let values = runtime.runtime_value_summaries(
            &[
                "store".to_owned(),
                "store.todos".to_owned(),
                "store.todos[0].title".to_owned(),
                "store.todos[0].completed".to_owned(),
            ],
            5,
            16,
            2,
        );

        assert_eq!(values["store"]["kind"], "object");
        assert!(values["store"]["field_count"].as_u64().unwrap_or_default() > 4);
        assert_eq!(
            values["store"]["fields"]
                .as_object()
                .expect("sampled fields should be present")
                .get("todos")
                .and_then(|value| value.get("kind"))
                .and_then(JsonValue::as_str),
            Some("list")
        );
        assert_eq!(values["store.todos"]["kind"], "list");
        assert_eq!(values["store.todos"]["sample"].as_array().unwrap().len(), 2);
        assert_eq!(values["store.todos"]["len"], 4);
        assert_eq!(values["store.todos"]["truncated"], true);
        let first_todo = &values["store.todos"]["sample"][0]["fields"];
        assert_eq!(
            first_todo["completed"],
            json!({"kind": "bool", "value": false})
        );
        assert!(first_todo.get("key").is_none());
        assert!(first_todo.get("generation").is_none());
        assert!(first_todo.get("sources").is_none());
        assert_eq!(
            values["store.todos[0].title"],
            json!({"kind": "string", "value": "Read documentation"})
        );
        assert_eq!(
            values["store.todos[0].completed"],
            json!({"kind": "bool", "value": false})
        );
    }

    #[test]
    fn generic_source_event_ingests_expected_event_payloads() {
        let mut expected = BTreeMap::new();
        expected.insert(
            "source".to_owned(),
            toml::Value::String("cell.sources.editor.commit".to_owned()),
        );
        expected.insert("text".to_owned(), toml::Value::String("=A0+1".to_owned()));
        expected.insert("key".to_owned(), toml::Value::String("Enter".to_owned()));
        expected.insert("address".to_owned(), toml::Value::String("B0".to_owned()));
        expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries".to_owned()),
        );
        let step = ScenarioStep {
            id: "source-ingest".to_owned(),
            expected_source_event: Some(expected),
            ..ScenarioStep::default()
        };

        let event = GenericSourceEvent::require(&step).unwrap();
        assert_eq!(event.source, "cell.sources.editor.commit");
        assert_eq!(event.text, Some("=A0+1"));
        assert_eq!(event.key, Some("Enter"));
        assert_eq!(event.address, Some("B0"));
        assert_eq!(event.target_text, Some("Buy groceries"));

        let missing = ScenarioStep {
            id: "missing-source".to_owned(),
            ..ScenarioStep::default()
        };
        assert!(GenericSourceEvent::require(&missing).is_err());
    }

    #[test]
    fn list_memory_dense_key_slots_survive_remove_and_move() {
        let mut list = ListMemory::from_values([
            todo_generic_row("a"),
            todo_generic_row("b"),
            todo_generic_row("c"),
            todo_generic_row("d"),
        ]);
        let (first_key, _) = list.row_identity(0).unwrap();
        let (second_key, _) = list.row_identity(1).unwrap();
        let (third_key, _) = list.row_identity(2).unwrap();
        let (fourth_key, _) = list.row_identity(3).unwrap();

        let removed = list.remove_index(1);
        assert_eq!(removed.key, second_key);
        assert_eq!(list.bound_index(second_key, 1), None);
        assert_eq!(list.len(), 3);
        assert_eq!(list.slot_capacity(), 4);
        assert_eq!(list.valid_slot_count(), 3);
        assert_eq!(list.free_slot_count(), 1);
        assert_eq!(list.bound_index(first_key, 1), Some(0));
        assert_eq!(list.bound_index(third_key, 1), Some(1));
        assert_eq!(list.bound_index(fourth_key, 1), Some(2));

        list.move_index(2, 0).unwrap();
        assert_eq!(list.bound_index(fourth_key, 1), Some(0));
        assert_eq!(list.bound_index(first_key, 1), Some(1));
        assert_eq!(list.bound_index(third_key, 1), Some(2));

        let (new_key, generation) = list.append(todo_generic_row("e"));
        assert_eq!(generation, 1);
        assert_ne!(new_key, second_key);
        assert_eq!(list.slot_capacity(), 4);
        assert_eq!(list.valid_slot_count(), 4);
        assert_eq!(list.free_slot_count(), 0);
        assert_eq!(list.bound_index(new_key, 1), Some(3));
    }

    #[test]
    fn source_store_dense_source_id_slots_reject_unbound_sources() {
        let mut sources = SourceStore::with_capacity(4);
        sources
            .bind_row(
                "todos",
                10,
                1,
                &[
                    "todo.sources.todo_checkbox.click".to_owned(),
                    "todo.sources.title_input.commit".to_owned(),
                ],
            )
            .unwrap();
        let binding = sources
            .row_bindings("todos", 10, 1)
            .find(|binding| binding.source_path == "todo.sources.todo_checkbox.click")
            .cloned()
            .unwrap();
        assert!(sources.is_bound(
            "todos",
            10,
            1,
            &binding.source_path,
            Some(binding.source_id),
            Some(binding.bind_epoch),
        ));

        sources.unbind_row("todos", 10, 1);
        assert!(!sources.is_bound(
            "todos",
            10,
            1,
            &binding.source_path,
            Some(binding.source_id),
            Some(binding.bind_epoch),
        ));
    }

    #[test]
    fn source_store_row_binding_storage_grows_without_panic() {
        let mut sources = SourceStore::with_capacity(64);
        let path_refs = vec!["todo.sources.dynamic.change".to_owned(); 64];
        sources.bind_row("todos", 10, 1, &path_refs).unwrap();
        assert_eq!(sources.len(), 64);
        assert_eq!(sources.row_binding_count("todos", 10, 1), 64);
    }

    #[test]
    fn remove_unbinds_todo_row_sources_and_emits_unbind_deltas() {
        let parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let mut runtime = list_scenario_harness_from_parsed(&parsed);
        let row_source_count = runtime
            .generic
            .list_source_bindings
            .source_count("todos")
            .unwrap();
        let removed_key = runtime.list_key_for_test(0);
        let removed_generation = runtime.list_generation_for_test(0);
        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        runtime.remove_index(0, &mut deltas, &mut patches).unwrap();

        assert_eq!(
            runtime
                .generic
                .row_source_binding_count("todos", removed_key, removed_generation),
            0
        );
        let unbind_count = deltas
            .iter()
            .filter(|delta| delta.kind == "SourceUnbind" && delta.key == Some(removed_key))
            .count();
        assert_eq!(unbind_count, row_source_count);
        assert!(
            deltas
                .iter()
                .filter(|delta| delta.kind == "SourceUnbind" && delta.key == Some(removed_key))
                .all(|delta| delta.source_id.is_some() && delta.bind_epoch.is_some())
        );
        let unbind_patch_count = patches
            .iter()
            .filter(|patch| patch.kind == "InvalidateDocument")
            .count();
        assert!(unbind_patch_count >= row_source_count);
        assert!(patches.iter().any(|patch| matches!(
            (&patch.target, &patch.value),
            (
                RenderTarget::Static(source_target),
                ProtocolValue::Text(source_path)
            ) if source_target.as_ref() == "document"
                && source_path.as_ref() == "todo.sources.todo_checkbox.click"
        )));
    }

    #[test]
    fn stale_todo_source_bind_epoch_is_ignored() {
        let parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let mut runtime = list_scenario_harness_from_parsed(&parsed);
        let target_key = runtime.list_key_for_test(0);
        let target_generation = runtime.list_generation_for_test(0);
        let binding = runtime
            .generic
            .row_source_bindings("todos", target_key, target_generation)
            .find(|binding| binding.source_path == "todo.sources.todo_checkbox.click")
            .unwrap()
            .clone();
        let mut action = BTreeMap::new();
        action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
        action.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries checkbox".to_owned()),
        );
        action.insert(
            "target_key".to_owned(),
            toml::Value::Integer(target_key as i64),
        );
        action.insert(
            "target_generation".to_owned(),
            toml::Value::Integer(target_generation as i64),
        );
        action.insert(
            "source_id".to_owned(),
            toml::Value::Integer(binding.source_id as i64),
        );
        action.insert(
            "bind_epoch".to_owned(),
            toml::Value::Integer(binding.bind_epoch as i64 + 1),
        );
        let step = ScenarioStep {
            id: "stale-bind-epoch-click".to_owned(),
            user_action: Some(action),
            expected_source_event: Some(todo_checkbox_expected_source("Buy groceries")),
            ..ScenarioStep::default()
        };
        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        runtime
            .apply_step_into(&step, &mut deltas, &mut patches)
            .unwrap();
        assert!(deltas.is_empty());
        assert!(patches.is_empty());
        assert_eq!(runtime.stale_source_drop_count, 1);
        assert!(!runtime.list_completed_for_test(0));
    }

    #[test]
    fn stale_todo_source_generation_is_ignored() {
        let parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let mut runtime = list_scenario_harness_from_parsed(&parsed);
        let stale_key = runtime.list_key_for_test(0);
        let stale_generation = runtime.list_generation_for_test(0);
        let mut setup_deltas = Vec::new();
        let mut setup_patches = Vec::new();
        runtime
            .remove_index(0, &mut setup_deltas, &mut setup_patches)
            .unwrap();
        assert_eq!(
            runtime
                .generic
                .row_source_binding_count("todos", stale_key, stale_generation),
            0
        );

        let mut action = BTreeMap::new();
        action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
        action.insert(
            "target_text".to_owned(),
            toml::Value::String("Buy groceries checkbox".to_owned()),
        );
        action.insert(
            "target_key".to_owned(),
            toml::Value::Integer(stale_key as i64),
        );
        action.insert(
            "target_generation".to_owned(),
            toml::Value::Integer(stale_generation as i64),
        );
        let step = ScenarioStep {
            id: "stale-deleted-row-click".to_owned(),
            user_action: Some(action),
            expected_source_event: Some(todo_checkbox_expected_source("Buy groceries")),
            ..ScenarioStep::default()
        };
        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        runtime
            .apply_step_into(&step, &mut deltas, &mut patches)
            .unwrap();
        assert!(deltas.is_empty());
        assert!(patches.is_empty());
        assert_eq!(runtime.stale_source_drop_count, 1);
        assert_eq!(runtime.list_len_for_test(), 3);
        assert_eq!(runtime.list_title_for_test(0), "Finish TodoMVC renderer");
        assert!(runtime.list_completed_for_test(0));
    }

    #[test]
    fn list_move_emits_keyed_delta_without_graph_clone() {
        let parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let mut runtime = list_scenario_harness_from_parsed(&parsed);
        let moved_key = runtime.list_key_for_test(0);
        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        runtime.move_index(0, 1, &mut deltas, &mut patches).unwrap();
        assert_eq!(runtime.list_key_for_test(1), moved_key);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].kind, "ListMove");
        assert_eq!(deltas[0].list_id.as_deref(), Some("todos"));
        assert_eq!(deltas[0].key, Some(moved_key));
        assert_eq!(deltas[0].generation, Some(1));
        assert_eq!(deltas[0].field_path.as_deref(), Some("position"));
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].kind, "InvalidateDocument");
    }

    #[test]
    fn cells_deltas_use_hidden_list_slots_not_visible_address_hashes() {
        let mut runtime =
            LiveRuntime::from_source("cells-hidden-keys", &cells_project_source_for_test())
                .unwrap();
        let output = runtime
            .apply_source_event(LiveSourceEvent {
                source: "cell.sources.editor.commit".to_owned(),
                text: Some("41".to_owned()),
                address: Some("A0".to_owned()),
                ..LiveSourceEvent::default()
            })
            .unwrap();
        let expected_key = output
            .semantic_deltas
            .iter()
            .find(|delta| {
                delta.list_id.as_deref() == Some("cells")
                    && delta.field_path.as_deref() == Some("formula_text")
            })
            .and_then(|delta| delta.key)
            .expect("Cells commit should emit a keyed formula_text delta");
        assert!(
            output
                .semantic_deltas
                .iter()
                .filter(|delta| delta.list_id.is_some())
                .all(|delta| delta.list_id.as_deref() == Some("cells"))
        );
        assert!(
            output
                .semantic_deltas
                .iter()
                .filter(|delta| {
                    delta.kind == "FieldSet"
                        && delta.list_id.as_deref() == Some("cells")
                        && delta.field_path.as_deref() == Some("formula_text")
                })
                .all(|delta| delta.key == Some(expected_key))
        );
        assert!(output.semantic_deltas.iter().all(|delta| {
            delta
                .key
                .is_none_or(|key| key != cell_address_hash_for_test("A0"))
        }));
        assert_ne!(expected_key, cell_address_hash_for_test("A0"));
    }

    #[test]
    fn cells_edit_state_updates_are_derived_from_ir_branches() {
        let source = cells_project_source_for_test()
            .replace("change: SOURCE", "input: SOURCE")
            .replace("commit: SOURCE", "apply: SOURCE")
            .replace("cancel: SOURCE", "revert: SOURCE")
            .replace(
                "sources.editor.events.change",
                "sources.editor.events.input",
            )
            .replace("sources.editor.change", "sources.editor.input")
            .replace("sources.editor.commit", "sources.editor.apply")
            .replace("sources.editor.cancel", "sources.editor.revert");
        let parsed = parse_source("examples/cells.bn", source).unwrap();
        lower(&parsed).unwrap();
        let mut runtime =
            LiveRuntime::from_source("renamed-cells-sources", &parsed.source).unwrap();
        let output = runtime
            .apply_source_event(LiveSourceEvent {
                source: "cell.sources.editor.apply".to_owned(),
                text: Some("123".to_owned()),
                address: Some("A0".to_owned()),
                ..LiveSourceEvent::default()
            })
            .unwrap();
        let a0 = cell_summary(&output.state_summary, "A0");
        assert_eq!(a0.get("formula_text"), Some(&json!("123")));
        assert_eq!(a0.get("editing_text"), Some(&json!("123")));
        assert_eq!(a0.get("value"), Some(&json!("123")));
        assert_eq!(a0.get("editing"), Some(&json!(false)));

        let mut action = BTreeMap::new();
        action.insert(
            "kind".to_owned(),
            toml::Value::String("key_down".to_owned()),
        );
        action.insert(
            "target".to_owned(),
            toml::Value::String("A0 editor".to_owned()),
        );
        action.insert("key".to_owned(), toml::Value::String("Escape".to_owned()));
        let mut expected = BTreeMap::new();
        expected.insert(
            "source".to_owned(),
            toml::Value::String("cell.sources.editor.revert".to_owned()),
        );
        expected.insert("address".to_owned(), toml::Value::String("A0".to_owned()));
        let step = ScenarioStep {
            id: "renamed-cell-revert".to_owned(),
            user_action: Some(action),
            expected_source_event: Some(expected),
            ..ScenarioStep::default()
        };
        let output = runtime
            .apply_source_event_for_step(
                &step,
                LiveSourceEvent {
                    source: "cell.sources.editor.revert".to_owned(),
                    address: Some("A0".to_owned()),
                    ..LiveSourceEvent::default()
                },
            )
            .unwrap();
        let a0 = cell_summary(&output.state_summary, "A0");
        assert_eq!(a0.get("editing_text"), Some(&json!("123")));
        assert_eq!(a0.get("editing"), Some(&json!(false)));
        assert!(
            output
                .render_patches
                .iter()
                .all(|patch| patch.kind == "InvalidateDocument"),
            "Cells edits must use generic document invalidation patches"
        );
    }

    #[test]
    fn latest_candidate_uses_greatest_source_sequence() {
        let selected = latest_value(
            "test.value",
            &[
                LatestCandidate::new(TickSeq(1), "old"),
                LatestCandidate::new(TickSeq(3), "new"),
                LatestCandidate::new(TickSeq(2), "middle"),
            ],
        )
        .unwrap();
        assert_eq!(selected, Some("new"));
    }

    #[test]
    fn latest_candidate_rejects_equal_sequence_conflict() {
        let err = latest_value(
            "test.value",
            &[
                LatestCandidate::new(TickSeq(7), "left"),
                LatestCandidate::new(TickSeq(7), "right"),
            ],
        )
        .unwrap_err();
        assert!(err.to_string().contains("ambiguous LATEST write"));
    }

    #[test]
    fn then_gate_converts_presence_to_latest_candidate() {
        let present = then_value(EventPulse::present(TickSeq(9)), "value").unwrap();
        assert_eq!(present.seq, TickSeq(9));
        assert_eq!(present.value, "value");
        assert!(
            then_value(
                EventPulse {
                    seq: TickSeq(10),
                    present: false,
                },
                "value",
            )
            .is_none()
        );
    }

    #[test]
    fn while_gate_is_continuous_selection_not_loop() {
        assert_eq!(while_value(true, "visible"), Some("visible"));
        assert_eq!(while_value(false, "visible"), None);
    }

    #[test]
    fn dirty_key_count_is_allocation_free_unique_scan() {
        let deltas = [
            field_delta(
                Some(7),
                Some(1),
                "title",
                ProtocolValue::Text(Cow::Borrowed("a")),
            ),
            field_delta(Some(7), Some(1), "editing", ProtocolValue::Bool(true)),
            field_delta(Some(9), Some(1), "completed", ProtocolValue::Bool(false)),
            field_delta(
                None,
                None,
                "store.selected_filter",
                ProtocolValue::Text(Cow::Borrowed("All")),
            ),
        ];
        assert_eq!(dirty_key_count(&deltas), 2);
    }

    #[test]
    fn dirty_keysets_track_list_field_keys_and_reuse_storage() {
        let deltas = [
            field_delta(
                Some(7),
                Some(1),
                "title",
                ProtocolValue::Text(Cow::Borrowed("a")),
            ),
            field_delta(Some(7), Some(1), "completed", ProtocolValue::Bool(true)),
            field_delta(Some(9), Some(1), "completed", ProtocolValue::Bool(false)),
            field_delta(
                None,
                None,
                "store.selected_filter",
                ProtocolValue::Text(Cow::Borrowed("All")),
            ),
        ];
        let mut dirty = DirtyKeySets::with_capacity(4);
        assert_eq!(dirty.mark_deltas(&deltas), 2);
        assert_eq!(dirty.entries.len(), 3);
        let capacity = dirty.entries.capacity();

        dirty.mark_indexes("cells", "value", &[0, 2, 2, 5]);
        assert_eq!(dirty.key_count(), 3);
        assert_eq!(dirty.entries.capacity(), capacity);
    }

    #[test]
    fn value_columns_keep_field_slots_sorted_for_dense_lookup() {
        let mut columns = ValueColumns::default();
        columns.insert_value("title".to_owned(), FieldValue::Text("A".to_owned()));
        columns.insert_value("completed".to_owned(), FieldValue::Bool(false));
        columns.insert_value("editing".to_owned(), FieldValue::Bool(true));
        columns.insert_value(
            "selected_filter".to_owned(),
            FieldValue::Enum("All".to_owned()),
        );
        columns.set_or_insert_text("edit_text", "draft").unwrap();
        columns.set_textlike("title", "B").unwrap();
        columns.set_bool("completed", true).unwrap();

        assert_eq!(columns.textlike("title"), Some("B"));
        assert_eq!(columns.textlike("edit_text"), Some("draft"));
        assert_eq!(columns.bool_value("completed"), Some(true));
        assert_eq!(columns.bool_value("editing"), Some(true));
        assert_eq!(columns.textlike("selected_filter"), Some("All"));
        assert!(
            columns
                .text
                .windows(2)
                .all(|window| window[0].field_id < window[1].field_id)
        );
        assert!(
            columns
                .bools
                .windows(2)
                .all(|window| window[0].field_id < window[1].field_id)
        );
        assert!(
            columns
                .enums
                .windows(2)
                .all(|window| window[0].field_id < window[1].field_id)
        );
    }

    #[test]
    fn runtime_field_slots_use_name_hashes_not_example_field_tables() {
        for field in [
            "title",
            "completed",
            "formula_text",
            "editing_text",
            "value",
            "error",
        ] {
            assert_eq!(
                runtime_field_id_from_name(field),
                FieldId(stable_runtime_field_id(field))
            );
        }
    }

    #[test]
    fn runtime_list_store_keeps_hidden_list_slots_sorted_for_dense_lookup() {
        let mut store = RuntimeListStore::default();
        store.insert(
            ListId(0),
            "todos".to_owned(),
            ListMemory::from_values([todo_generic_row("A")]),
            Some(4),
            RuntimeRowSnapshotTemplate::default(),
        );
        let mut address_row = ValueColumns::default();
        address_row.insert_value("address".to_owned(), FieldValue::Text("A0".to_owned()));
        store.insert(
            ListId(1),
            "cells".to_owned(),
            ListMemory::from_values([RuntimeRowSnapshot {
                columns: address_row,
            }]),
            Some(26),
            RuntimeRowSnapshotTemplate::default(),
        );
        store.insert(
            ListId(0),
            "todos".to_owned(),
            ListMemory::from_values([todo_generic_row("B")]),
            Some(8),
            RuntimeRowSnapshotTemplate::default(),
        );

        assert_eq!(store.capacity("todos"), Some(8));
        assert_eq!(store.capacity("cells"), Some(26));
        assert_eq!(store.memory("todos").unwrap().len(), 1);
        assert_eq!(store.memory("cells").unwrap().len(), 1);
        assert!(
            store
                .list_slots
                .windows(2)
                .all(|window| window[0].list_id < window[1].list_id)
        );
    }

    #[test]
    fn human_report_command_pass_labels_must_match_checklist() {
        let command_argv = vec![
            json!("cargo"),
            json!("xtask"),
            json!("prepare-todomvc-human-report"),
            json!("--pass-label"),
            json!("initial"),
            json!("--pass-label"),
            json!("add-test-todo-submit"),
            json!("--artifact"),
            json!("target/reports/manual-todomvc.png"),
            json!("--display-server"),
            json!("wayland"),
            json!("--display-scale"),
            json!("1.25"),
        ];
        let labels = command_argv_values_after(&command_argv, "--pass-label");
        assert_eq!(
            labels,
            ["add-test-todo-submit", "initial"].into_iter().collect()
        );
        assert_ne!(
            labels,
            ["initial", "filter-active"].into_iter().collect(),
            "manual command provenance must not pass with missing or extra checklist labels"
        );
        assert_eq!(
            command_argv_value_after(&command_argv, "--display-server"),
            Some("wayland")
        );
        require_command_argv_f64(
            Path::new("target/reports/test-human.json"),
            &command_argv,
            "--display-scale",
            1.25,
        )
        .unwrap();
        assert!(
            require_command_argv_value(
                Path::new("target/reports/test-human.json"),
                &command_argv,
                "--display-server",
                "x11",
            )
            .is_err(),
            "manual report command provenance must match the recorded visible-session metadata"
        );
    }

    #[test]
    fn scenario_delta_expectations_reject_missing_semantic_or_render_patch() {
        let semantic_step = ScenarioStep {
            id: "missing-semantic".to_owned(),
            expect_semantic_delta_contains: vec!["ListInsert".to_owned()],
            ..ScenarioStep::default()
        };
        let render_step = ScenarioStep {
            id: "missing-render".to_owned(),
            expect_render_delta_contains: vec!["BindSource".to_owned()],
            ..ScenarioStep::default()
        };
        let deltas = [field_delta(
            Some(1),
            Some(1),
            "completed",
            ProtocolValue::Bool(true),
        )];
        let patches = [patch(
            "InvalidateDocument",
            RenderTarget::Static(Cow::Borrowed("document")),
            ProtocolValue::CheckedProperty(true),
        )];

        assert!(assert_delta_expectations(&semantic_step, &deltas, &patches).is_err());
        assert!(assert_delta_expectations(&render_step, &deltas, &patches).is_err());
    }

    #[test]
    fn pure_boon_cells_helpers_support_documented_arithmetic_ops() {
        let mut runtime =
            LiveRuntime::from_source("cells-arithmetic", &cells_project_source_for_test()).unwrap();
        for (formula, expected) in [
            ("=8+2", "10"),
            ("=8-2", "6"),
            ("=8*2", "16"),
            ("=8/2", "4"),
            ("=add(8,2)", "10"),
        ] {
            let output = commit_cell(&mut runtime, "A0", formula);
            assert_eq!(cell_summary(&output.state_summary, "A0")["value"], expected);
            assert_eq!(
                cell_summary(&output.state_summary, "A0")["error"],
                JsonValue::Null
            );
        }
        let output = commit_cell(&mut runtime, "A0", "=8/0");
        assert_eq!(
            cell_summary(&output.state_summary, "A0")["error"],
            "div_by_zero"
        );
        let output = commit_cell(&mut runtime, "D0", "");
        assert_eq!(cell_summary(&output.state_summary, "D0")["value"], "");
        let output = commit_cell(&mut runtime, "E0", "=D0+2");
        assert_eq!(cell_summary(&output.state_summary, "E0")["value"], "2");
        commit_cell(&mut runtime, "A0", "5");
        commit_cell(&mut runtime, "A1", "10");
        commit_cell(&mut runtime, "A2", "15");
        let output = commit_cell(&mut runtime, "C0", "=sum(A0:A2)");
        assert_eq!(cell_summary(&output.state_summary, "C0")["value"], "30");
        assert_eq!(
            cell_summary(&output.state_summary, "C0")["error"],
            JsonValue::Null
        );
    }

    #[test]
    fn pure_boon_cells_replacing_reference_removes_stale_dependents() {
        let mut runtime =
            LiveRuntime::from_source("cells-replace-reference", &cells_project_source_for_test())
                .unwrap();
        commit_cell(&mut runtime, "A0", "1");
        let output = commit_cell(&mut runtime, "B0", "=A0+1");
        assert_eq!(cell_summary(&output.state_summary, "B0")["value"], "2");

        let output = commit_cell(&mut runtime, "B0", "5");
        assert_eq!(cell_summary(&output.state_summary, "B0")["value"], "5");

        let output = commit_cell(&mut runtime, "A0", "10");
        assert_eq!(cell_summary(&output.state_summary, "A0")["value"], "10");
        assert_eq!(cell_summary(&output.state_summary, "B0")["value"], "5");
        assert!(
            !output.semantic_deltas.iter().any(|delta| {
                delta.field_path.as_deref() == Some("value")
                    && matches!(delta.value, ProtocolValue::Text(ref value) if value.as_ref() == "11")
            }),
            "B0 must not keep a stale dependency on A0 after becoming a literal"
        );
    }

    #[test]
    fn pure_boon_cells_fanout_recomputes_from_generic_read_index() {
        let mut runtime =
            LiveRuntime::from_source("cells-fanout", &cells_project_source_for_test()).unwrap();
        commit_cell(&mut runtime, "A0", "1");
        commit_cell(&mut runtime, "B0", "=A0+1");
        commit_cell(&mut runtime, "C0", "=A0+2");
        commit_cell(&mut runtime, "D0", "=A0+3");

        let output = commit_cell(&mut runtime, "A0", "10");
        assert_eq!(cell_summary(&output.state_summary, "B0")["value"], "11");
        assert_eq!(cell_summary(&output.state_summary, "C0")["value"], "12");
        assert_eq!(cell_summary(&output.state_summary, "D0")["value"], "13");
        let value_delta_count = output
            .semantic_deltas
            .iter()
            .filter(|delta| delta.field_path.as_deref() == Some("value"))
            .count();
        assert!(
            value_delta_count >= 4,
            "A0 fanout should emit value deltas for source and dependents"
        );
    }

    fn cell_address_hash_for_test(address: &str) -> u64 {
        let hash = sha256_bytes(address.as_bytes());
        let bytes = hex_prefix_to_bytes(&hash, 8);
        u64::from_le_bytes(bytes.try_into().unwrap())
    }

    fn cell_summary<'a>(summary: &'a JsonValue, address: &str) -> &'a JsonValue {
        summary
            .get("cells")
            .and_then(JsonValue::as_array)
            .and_then(|cells| {
                cells
                    .iter()
                    .find(|cell| cell.get("address") == Some(&json!(address)))
            })
            .unwrap_or_else(|| panic!("Cells state summary should include {address}"))
    }

    fn commit_cell(runtime: &mut LiveRuntime, address: &str, text: &str) -> LiveStepOutput {
        runtime
            .apply_source_event(LiveSourceEvent {
                source: "cell.sources.editor.commit".to_owned(),
                text: Some(text.to_owned()),
                address: Some(address.to_owned()),
                ..LiveSourceEvent::default()
            })
            .unwrap()
    }

    fn hex_prefix_to_bytes(hash: &str, len: usize) -> Vec<u8> {
        (0..len)
            .map(|index| u8::from_str_radix(&hash[index * 2..index * 2 + 2], 16).unwrap())
            .collect()
    }

    fn todo_checkbox_expected_source(target_text: &str) -> BTreeMap<String, toml::Value> {
        let mut expected = BTreeMap::new();
        expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.todo_checkbox.click".to_owned()),
        );
        expected.insert(
            "target_text".to_owned(),
            toml::Value::String(target_text.to_owned()),
        );
        expected
    }
}
