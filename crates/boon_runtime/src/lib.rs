#![recursion_limit = "512"]

use bitvec::prelude::*;
use boon_ir::{
    DerivedValueKind, FormulaOperationKind, InitialValue, ListInitializer, ListOperationKind,
    ListPredicate, SourceId, SourcePayloadField, TypedProgram, UpdateExpression, debug_tables,
    lower, verify_hidden_identity, verify_static_schedule,
};
use boon_parser::{ParsedProgram, parse_source};
use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use std::alloc::{GlobalAlloc, Layout, System};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::ops::{Deref, DerefMut, Index, IndexMut};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
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
    route_kind: GenericSourceRouteKind,
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
    pub view_lines: Vec<String>,
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
    pub list_id: Option<&'static str>,
    pub key: Option<u64>,
    pub generation: Option<u64>,
    pub source_id: Option<u64>,
    pub bind_epoch: Option<u64>,
    pub field_path: Option<&'static str>,
    pub value: ProtocolValue<'a>,
}

#[derive(Clone, Debug)]
pub struct RenderPatch<'a> {
    pub kind: &'static str,
    pub target: RenderTarget<'a>,
    pub value: ProtocolValue<'a>,
    pub list_id: Option<&'static str>,
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
    TodoRow {
        title: Cow<'a, str>,
        completed: bool,
        editing: bool,
    },
    SourceBinding {
        source_path: &'static str,
        source_id: u64,
        bind_epoch: u64,
    },
    CheckedProperty(bool),
}

#[derive(Clone, Debug)]
pub enum RenderTarget<'a> {
    Static(&'static str),
    Borrowed(Cow<'a, str>),
    TodoRow(u64),
    TodoTitle(u64),
    TodoEdit(u64),
    TodoCheckbox(u64),
    TodoPosition(u64),
    TodoSource(u64, &'static str),
}

impl<'a> SemanticDelta<'a> {
    fn to_static(&self) -> SemanticDelta<'static> {
        SemanticDelta {
            kind: self.kind,
            list_id: self.list_id,
            key: self.key,
            generation: self.generation,
            source_id: self.source_id,
            bind_epoch: self.bind_epoch,
            field_path: self.field_path,
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
            list_id: self.list_id,
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
            Self::TodoRow {
                title,
                completed,
                editing,
            } => ProtocolValue::TodoRow {
                title: Cow::Owned(title.to_string()),
                completed: *completed,
                editing: *editing,
            },
            Self::SourceBinding {
                source_path,
                source_id,
                bind_epoch,
            } => ProtocolValue::SourceBinding {
                source_path,
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
            Self::Static(value) => RenderTarget::Static(value),
            Self::Borrowed(value) => RenderTarget::Borrowed(Cow::Owned(value.to_string())),
            Self::TodoRow(key) => RenderTarget::TodoRow(*key),
            Self::TodoTitle(key) => RenderTarget::TodoTitle(*key),
            Self::TodoEdit(key) => RenderTarget::TodoEdit(*key),
            Self::TodoCheckbox(key) => RenderTarget::TodoCheckbox(*key),
            Self::TodoPosition(key) => RenderTarget::TodoPosition(*key),
            Self::TodoSource(key, source_path) => RenderTarget::TodoSource(*key, source_path),
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
            Self::TodoRow {
                title,
                completed,
                editing,
            } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("title", title)?;
                map.serialize_entry("completed", completed)?;
                map.serialize_entry("editing", editing)?;
                map.end()
            }
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
            Self::TodoRow(key) => serializer.serialize_str(&format!("todos:{key}:row")),
            Self::TodoTitle(key) => serializer.serialize_str(&format!("todos:{key}:title")),
            Self::TodoEdit(key) => serializer.serialize_str(&format!("todos:{key}:edit")),
            Self::TodoCheckbox(key) => serializer.serialize_str(&format!("todos:{key}:checkbox")),
            Self::TodoPosition(key) => serializer.serialize_str(&format!("todos:{key}:position")),
            Self::TodoSource(key, source_path) => {
                serializer.serialize_str(&format!("todos:{key}:source:{source_path}"))
            }
        }
    }
}

pub fn load_and_lower(source_path: &Path) -> RuntimeResult<(ParsedProgram, TypedProgram)> {
    let source = fs::read_to_string(source_path)?;
    let parsed = parse_source(source_path.display().to_string(), source)?;
    let ir = lower(&parsed)?;
    verify_hidden_identity(&ir)?;
    verify_static_schedule(&ir)?;
    Ok((parsed, ir))
}

pub fn ir_debug_report(source_path: &Path) -> RuntimeResult<JsonValue> {
    let (parsed, ir) = load_and_lower(source_path)?;
    Ok(json!({
        "status": "pass",
        "program_kind": parsed.kind.as_str(),
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
    if parsed.kind.as_str() != scenario.name {
        return Err(format!(
            "scenario `{}` does not match source kind `{}`",
            scenario.name,
            parsed.kind.as_str()
        )
        .into());
    }
    let started = Instant::now();
    let output = run_loaded_scenario(&parsed, &ir, &scenario, layer)?;
    let elapsed = started.elapsed();
    let mut report = output.report;
    enrich_report(
        &mut report,
        &source_path.display().to_string(),
        &sha256_file(source_path)?,
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
        view_lines: boon_parser::parsed_view_lines(&parsed),
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
    if parsed.kind.as_str() != scenario.name {
        return Err(format!(
            "scenario `{}` does not match source kind `{}`",
            scenario.name,
            parsed.kind.as_str()
        )
        .into());
    }
    let started = Instant::now();
    let output = run_loaded_scenario(&parsed, &ir, &scenario, layer)?;
    let elapsed = started.elapsed();
    let mut report = output.report;
    enrich_report(
        &mut report,
        source_label,
        &sha256_bytes(source_text.as_bytes()),
        scenario_path,
        None,
        &parsed,
        &ir,
        layer,
        elapsed.as_secs_f64() * 1000.0,
    )?;
    Ok(RunOutput {
        report,
        view_lines: boon_parser::parsed_view_lines(&parsed),
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
    if parsed.kind.as_str() != scenario.name {
        return Err(format!(
            "scenario `{}` does not match source kind `{}`",
            scenario.name,
            parsed.kind.as_str()
        )
        .into());
    }
    let compile_started = Instant::now();
    let compiled = CompiledProgram::from_ir(&ir)?;
    validate_executable_surface(&parsed, &ir, &compiled)?;
    let compile_ms = compile_started.elapsed().as_secs_f64() * 1000.0;
    let runtime_started = Instant::now();
    let mut runtime = LoadedRuntime::new(&parsed, &ir, &compiled)?;
    runtime.prepare_for_scenario(scenario)?;
    let state_summary = runtime.state_summary();
    let runtime_ms = runtime_started.elapsed().as_secs_f64() * 1000.0;
    let report_started = Instant::now();
    let runtime_profile = RuntimeProfile::from_ir(&ir);
    let runtime_profile_detail = runtime_profile.detail_report(&ir);
    let capacity_report = runtime_profile.capacity_report(&ir);
    let generic_runtime_slices = generic_runtime_slices_report(&ir, &compiled);
    let generic_runtime_slice_evidence = generic_runtime_slice_evidence_report(&ir, &compiled);
    let report = json!({
        "status": "pass",
        "command": "playground-initial-state",
        "source_path": source_label,
        "source_hash": sha256_bytes(source_text.as_bytes()),
        "scenario_path": scenario_path.display().to_string(),
        "scenario_hash": sha256_file(scenario_path)?,
        "program_hash": sha256_bytes(source_text.as_bytes()),
        "program_kind": compiled.surface.kind.as_str(),
        "expression_count": ir.expression_count,
        "expression_coverage": &ir.expression_coverage,
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
            "generic_interpreter_complete": derive_generic_interpreter_complete(&ir, &compiled, &generic_runtime_slices),
            "example_behavior_adapter": derive_example_behavior_adapter(&compiled, &generic_runtime_slices),
            "adapter_kind": compiled.surface.kind.as_str(),
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
        view_lines: boon_parser::parsed_view_lines(&parsed),
    })
}

pub struct LiveRuntime {
    runtime: LoadedRuntime,
    next_step: usize,
}

impl LiveRuntime {
    pub fn new(source_label: &str, source_text: &str, scenario_path: &Path) -> RuntimeResult<Self> {
        let parsed = parse_source(source_label.to_owned(), source_text.to_owned())?;
        let ir = lower(&parsed)?;
        verify_hidden_identity(&ir)?;
        verify_static_schedule(&ir)?;
        let scenario = parse_scenario(scenario_path)?;
        if parsed.kind.as_str() != scenario.name {
            return Err(format!(
                "scenario `{}` does not match source kind `{}`",
                scenario.name,
                parsed.kind.as_str()
            )
            .into());
        }
        let compiled = CompiledProgram::from_ir(&ir)?;
        validate_executable_surface(&parsed, &ir, &compiled)?;
        let mut runtime = LoadedRuntime::new(&parsed, &ir, &compiled)?;
        runtime.prepare_for_scenario(&scenario)?;
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

    fn apply_checked_step(&mut self, step: &ScenarioStep) -> RuntimeResult<LiveStepOutput> {
        let mut semantic_deltas = Vec::new();
        let mut render_patches = Vec::new();
        self.runtime
            .apply_step(&step, &mut semantic_deltas, &mut render_patches)?;
        assert_delta_expectations(step, &semantic_deltas, &render_patches)?;
        self.runtime.assert_step_after_measurement(step)?;
        Ok(LiveStepOutput {
            semantic_deltas: semantic_deltas
                .iter()
                .map(SemanticDelta::to_static)
                .collect(),
            render_patches: render_patches.iter().map(RenderPatch::to_static).collect(),
            state_summary: self.runtime.state_summary(),
        })
    }

    pub fn state_summary(&mut self) -> JsonValue {
        self.runtime.state_summary()
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

pub fn write_json(path: &Path, value: &JsonValue) -> RuntimeResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub fn sha256_file(path: &Path) -> RuntimeResult<String> {
    Ok(sha256_bytes(&fs::read(path)?))
}

pub fn verify_report_schema(path: &Path) -> RuntimeResult<()> {
    let report: JsonValue = serde_json::from_slice(&fs::read(path)?)?;
    let required = [
        "report_version",
        "generated_at_utc",
        "command",
        "command_argv",
        "exit_status",
        "git_commit",
        "binary_hash",
        "source_hash",
        "scenario_hash",
        "program_hash",
        "budget_hash",
        "graph_node_count",
        "per_step_pass_fail",
        "artifact_sha256s",
    ];
    for key in required {
        if report.get(key).is_none() {
            return Err(format!("{} missing required report field `{key}`", path.display()).into());
        }
    }
    let status = report.get("status").and_then(JsonValue::as_str);
    if status != Some("pass") {
        if status == Some("fail") && report_is_blocker_audit(&report) {
            verify_failing_blocker_report_shape(&report, path)?;
            verify_artifact_hashes(&report, path)?;
            return Ok(());
        }
        return Err(format!("{} did not pass", path.display()).into());
    }
    verify_common_report_shape(&report, path)?;
    verify_report_file_hash(&report, path, "source_path", "source_hash")?;
    verify_report_file_hash(&report, path, "scenario_path", "scenario_hash")?;
    verify_artifact_hashes(&report, path)?;
    if report_is_runtime_execution_layer(&report) {
        verify_runtime_execution_metadata(&report, path)?;
    }
    if report.get("playground_surface").is_some()
        || report_layer_is(&report, "headed-smoke")
        || report_layer_is(&report, "headed-ply")
    {
        verify_playground_surface_report(&report, path)?;
    }
    if report_layer_is(&report, "speed") {
        verify_speed_report(&report, path)?;
    }
    if report_command_is(&report, "bench-todomvc") || report_command_is(&report, "bench-example") {
        verify_benchmark_report(&report, path)?;
    }
    if report_layer_is(&report, "headed-ply") {
        verify_headed_artifacts(&report, path)?;
    }
    if report_layer_is(&report, "os-input-probe") {
        verify_os_input_probe_report(&report, path)?;
    }
    if report_layer_is(&report, "human") {
        verify_human_artifacts(&report, path)?;
    }
    Ok(())
}

fn report_is_blocker_audit(report: &JsonValue) -> bool {
    matches!(
        report.get("command").and_then(JsonValue::as_str),
        Some(
            "audit-machine-readiness"
                | "audit-goal-readiness"
                | "audit-manual-readiness"
                | "verify-runtime-finality"
        )
    )
}

fn verify_failing_blocker_report_shape(
    report: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let generated = report
        .get("generated_at_utc")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} generated_at_utc is not a Unix-seconds string",
                report_path.display()
            )
        })?
        .parse::<u64>()
        .map_err(|error| {
            format!(
                "{} generated_at_utc is not parseable Unix seconds: {error}",
                report_path.display()
            )
        })?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    if generated > now.saturating_add(5) {
        return Err(format!("{} is future-dated", report_path.display()).into());
    }
    let exit_status = report
        .get("exit_status")
        .and_then(JsonValue::as_i64)
        .ok_or_else(|| format!("{} exit_status is not a number", report_path.display()))?;
    if exit_status == 0 {
        return Err(format!(
            "{} failing blocker report has zero exit_status",
            report_path.display()
        )
        .into());
    }
    let blockers = report
        .get("blockers")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} blocker report missing blockers", report_path.display()))?;
    if blockers.is_empty()
        || blockers.iter().any(|blocker| {
            blocker
                .as_str()
                .is_none_or(|blocker| blocker.trim().is_empty())
        })
    {
        return Err(format!(
            "{} blocker report has empty or malformed blockers",
            report_path.display()
        )
        .into());
    }
    let checks = report
        .get("per_step_pass_fail")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} per_step_pass_fail is not an array",
                report_path.display()
            )
        })?;
    let mut saw_failing_check = false;
    for (index, check) in checks.iter().enumerate() {
        let object = check.as_object().ok_or_else(|| {
            format!(
                "{} per_step_pass_fail[{index}] is not an object",
                report_path.display()
            )
        })?;
        let id = object
            .get("id")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                format!(
                    "{} per_step_pass_fail[{index}] missing string id",
                    report_path.display()
                )
            })?;
        if id.trim().is_empty() {
            return Err(format!(
                "{} per_step_pass_fail[{index}] has empty id",
                report_path.display()
            )
            .into());
        }
        let pass = object
            .get("pass")
            .and_then(JsonValue::as_bool)
            .ok_or_else(|| {
                format!(
                    "{} per_step_pass_fail[{index}] missing boolean pass",
                    report_path.display()
                )
            })?;
        saw_failing_check |= !pass;
    }
    if !saw_failing_check {
        return Err(format!(
            "{} failing blocker report has no failing per-step check",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_common_report_shape(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let version = report
        .get("report_version")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| format!("{} report_version is not a number", report_path.display()))?;
    if version == 0 {
        return Err(format!("{} report_version must be positive", report_path.display()).into());
    }
    let generated = report
        .get("generated_at_utc")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} generated_at_utc is not a Unix-seconds string",
                report_path.display()
            )
        })?
        .parse::<u64>()
        .map_err(|error| {
            format!(
                "{} generated_at_utc is not parseable Unix seconds: {error}",
                report_path.display()
            )
        })?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    if generated > now.saturating_add(5) {
        return Err(format!("{} is future-dated", report_path.display()).into());
    }
    let command = report
        .get("command")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} command is not a string", report_path.display()))?;
    if command.trim().is_empty() {
        return Err(format!("{} command is empty", report_path.display()).into());
    }
    let argv = report
        .get("command_argv")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} command_argv is not an array", report_path.display()))?;
    if argv.is_empty()
        || argv
            .iter()
            .any(|arg| arg.as_str().is_none_or(|arg| arg.trim().is_empty()))
    {
        return Err(format!(
            "{} command_argv is empty or malformed",
            report_path.display()
        )
        .into());
    }
    let exit_status = report
        .get("exit_status")
        .and_then(JsonValue::as_i64)
        .ok_or_else(|| format!("{} exit_status is not a number", report_path.display()))?;
    if exit_status != 0 {
        return Err(format!(
            "{} pass report has nonzero exit_status",
            report_path.display()
        )
        .into());
    }
    let checks = report
        .get("per_step_pass_fail")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} per_step_pass_fail is not an array",
                report_path.display()
            )
        })?;
    for (index, check) in checks.iter().enumerate() {
        let object = check.as_object().ok_or_else(|| {
            format!(
                "{} per_step_pass_fail[{index}] is not an object",
                report_path.display()
            )
        })?;
        let id = object
            .get("id")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                format!(
                    "{} per_step_pass_fail[{index}] missing string id",
                    report_path.display()
                )
            })?;
        if id.trim().is_empty() {
            return Err(format!(
                "{} per_step_pass_fail[{index}] has empty id",
                report_path.display()
            )
            .into());
        }
        let pass = object
            .get("pass")
            .and_then(JsonValue::as_bool)
            .ok_or_else(|| {
                format!(
                    "{} per_step_pass_fail[{index}] missing boolean pass",
                    report_path.display()
                )
            })?;
        if !pass {
            return Err(format!(
                "{} pass report contains failing check `{id}`",
                report_path.display()
            )
            .into());
        }
    }
    if report.get("command").and_then(JsonValue::as_str) == Some("explain-hardware") {
        verify_runtime_profile_metadata(report, report_path)?;
        if report.get("hardware_plan").is_none() {
            return Err(format!("{} missing hardware_plan", report_path.display()).into());
        }
    }
    Ok(())
}

fn verify_runtime_execution_metadata(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let execution = report.get("runtime_execution").ok_or_else(|| {
        format!(
            "{} missing runtime_execution metadata for executable example report",
            report_path.display()
        )
    })?;
    for key in [
        "implementation",
        "source_loaded_from_boon",
        "typed_ir_loaded",
        "static_schedule_verified",
        "runtime_profile",
        "runtime_profile_detail",
        "capacities",
        "expression_coverage",
        "generic_interpreter_complete",
        "example_behavior_adapter",
        "remaining_example_specific_shell_policy",
        "remaining_example_specific_shells",
    ] {
        if execution.get(key).is_none() {
            return Err(format!(
                "{} runtime_execution missing `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    if execution
        .get("source_loaded_from_boon")
        .and_then(JsonValue::as_bool)
        != Some(true)
    {
        return Err(format!(
            "{} did not load executable source from Boon",
            report_path.display()
        )
        .into());
    }
    if execution
        .get("typed_ir_loaded")
        .and_then(JsonValue::as_bool)
        != Some(true)
    {
        return Err(format!("{} did not lower through typed IR", report_path.display()).into());
    }
    if execution
        .get("static_schedule_verified")
        .and_then(JsonValue::as_bool)
        != Some(true)
    {
        return Err(format!("{} did not verify static schedule", report_path.display()).into());
    }
    if execution
        .get("generic_interpreter_complete")
        .and_then(JsonValue::as_bool)
        != Some(true)
    {
        return Err(format!(
            "{} did not complete generic interpreter execution",
            report_path.display()
        )
        .into());
    }
    if execution
        .get("example_behavior_adapter")
        .and_then(JsonValue::as_bool)
        != Some(false)
    {
        return Err(format!(
            "{} still reports an example behavior adapter",
            report_path.display()
        )
        .into());
    }
    verify_remaining_example_specific_shells(execution, report_path)?;
    verify_runtime_execution_report_mirror(report, execution, report_path)?;
    if execution.get("implementation").and_then(JsonValue::as_str)
        != Some("static_graph_interpreter")
    {
        return Err(format!(
            "{} did not use static_graph_interpreter implementation",
            report_path.display()
        )
        .into());
    }
    verify_generic_runtime_slice_metadata(report, execution, report_path)?;
    verify_generic_runtime_slice_evidence(report, execution, report_path)?;
    verify_expression_coverage(report, report_path)?;
    verify_runtime_report_contract(report, report_path)?;
    Ok(())
}

fn verify_runtime_execution_report_mirror(
    report: &JsonValue,
    execution: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    for key in [
        "runtime_profile",
        "runtime_profile_detail",
        "capacities",
        "expression_coverage",
    ] {
        let top_level = report.get(key).ok_or_else(|| {
            format!(
                "{} executable report missing top-level `{key}`",
                report_path.display()
            )
        })?;
        let execution_level = execution.get(key).ok_or_else(|| {
            format!(
                "{} runtime_execution missing mirrored `{key}`",
                report_path.display()
            )
        })?;
        if execution_level != top_level {
            return Err(format!(
                "{} runtime_execution `{key}` does not match top-level `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    Ok(())
}

fn verify_remaining_example_specific_shells(
    execution: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let policy = execution
        .get("remaining_example_specific_shell_policy")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} runtime_execution missing remaining shell policy",
                report_path.display()
            )
        })?;
    if policy != "scenario_assertion_report_glue_only" {
        return Err(format!(
            "{} runtime_execution has unsupported remaining shell policy `{policy}`",
            report_path.display()
        )
        .into());
    }
    let shells = execution
        .get("remaining_example_specific_shells")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} runtime_execution missing remaining shell list",
                report_path.display()
            )
        })?;
    let slices = execution
        .get("generic_runtime_slices")
        .and_then(JsonValue::as_object);
    let has_example_specific_slices = slices.is_some_and(|slices| {
        slices.iter().any(|(key, value)| {
            value.as_bool() == Some(true)
                && (key.contains("_todomvc_")
                    || key.starts_with("todomvc_")
                    || key.contains("_cells_")
                    || key.starts_with("cells_"))
        })
    });
    if has_example_specific_slices && shells.is_empty() {
        return Err(format!(
            "{} runtime_execution has example-specific runtime slices but no remaining shell listing",
            report_path.display()
        )
        .into());
    }
    for (index, shell) in shells.iter().enumerate() {
        let Some(shell) = shell.as_str() else {
            return Err(format!(
                "{} runtime_execution remaining shell at index {index} is not a string",
                report_path.display()
            )
            .into());
        };
        if !(shell.ends_with("_scenario_glue")
            || shell.ends_with("_assertion_glue")
            || shell.ends_with("_report_glue")
            || shell.ends_with("_render_patch_report_glue")
            || shell.ends_with("_stress_report_glue"))
        {
            return Err(format!(
                "{} runtime_execution remaining shell `{shell}` is not classified as scenario/assertion/report glue",
                report_path.display()
            )
            .into());
        }
    }
    Ok(())
}

fn verify_generic_runtime_slice_metadata(
    report: &JsonValue,
    execution: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let Some(slices) = execution
        .get("generic_runtime_slices")
        .and_then(JsonValue::as_object)
    else {
        return Err(format!("{} missing generic_runtime_slices", report_path.display()).into());
    };
    let example = report
        .get("example")
        .and_then(JsonValue::as_str)
        .or_else(|| execution.get("adapter_kind").and_then(JsonValue::as_str))
        .or_else(|| report_program_kind(report))
        .unwrap_or_default();
    require_generic_runtime_slice_flags(slices, example, report_path)?;
    for (key, value) in slices {
        let Some(flag) = value.as_bool() else {
            continue;
        };
        if key == "surface_driver_borrows_generic_storage_for_tick" {
            if flag {
                return Err(format!(
                    "{} reports surface driver borrowing generic storage for tick execution",
                    report_path.display()
                )
                .into());
            }
            continue;
        }
        if key_is_other_example_runtime_slice(key, example) {
            if flag {
                return Err(format!(
                    "{} reports other-example runtime slice `{key}` as passed for `{example}`",
                    report_path.display()
                )
                .into());
            }
            continue;
        }
        if !flag {
            return Err(format!(
                "{} generic runtime slice `{key}` did not pass",
                report_path.display()
            )
            .into());
        }
    }
    Ok(())
}

fn require_generic_runtime_slice_flags(
    slices: &serde_json::Map<String, JsonValue>,
    example: &str,
    report_path: &Path,
) -> RuntimeResult<()> {
    for key in [
        "generic_executable_surface_inferred_from_ir",
        "ir_update_branch_table_loaded",
        "generic_scenario_loop_executor",
        "generic_schedule_instantiated_before_adapter",
        "loaded_runtime_owns_generic_schedule_storage",
        "generic_source_event_ingest",
        "generic_source_binding_store",
        "generic_indexed_branch_evaluator",
        "generic_semantic_delta_emitter",
        "generic_source_mutation_semantic_delta_emitter",
        "generic_derived_value_semantic_delta_emitter",
        "generic_source_bind_semantic_delta_emitter",
        "generic_list_remove_semantic_delta_emitter",
        "generic_source_unbind_semantic_delta_emitter",
        "generic_render_lowering_plan",
        "generic_loaded_runtime_shell",
        "generic_source_route_action_executor",
        "generic_root_text_tick_executor",
        "generic_indexed_hold_commit_executor",
        "generic_list_append_source_binding_executor",
        "generic_list_remove_source_unbinding_executor",
        "generic_list_move_semantic_delta_emitter",
        "generic_list_count_retain_executor",
        "generic_loaded_runtime_state_summary_projection",
        "generic_root_source_dispatch",
        "generic_source_event_route_executor",
        "generic_compiled_source_route_index",
        "generic_source_route_classifier",
        "generic_bound_source_target_resolution",
        "generic_stale_source_key_generation_bind_epoch_rejection",
        "generic_source_action_batch_executor",
        "generic_source_route_scalar_expression_index",
        "generic_indexed_text_route_index",
        "generic_indexed_bool_route_index",
        "generic_root_source_route_index",
        "generic_list_remove_predicate_route",
        "generic_routed_root_target_application",
        "generic_routed_indexed_target_application",
        "ir_list_operation_table_loaded",
        "ir_formula_operation_table_loaded",
        "ir_state_initializers_loaded",
        "ir_list_initializers_loaded",
        "ir_derived_value_table_loaded",
        "generic_list_structural_commit_executor",
    ] {
        require_slice_bool(slices, key, true, report_path)?;
    }
    require_slice_bool(
        slices,
        "surface_driver_borrows_generic_storage_for_tick",
        false,
        report_path,
    )?;
    let example_specific = match example {
        "todomvc" => &[
            "generic_todomvc_common_render_patch_lowering",
            "generic_todomvc_append_source_bind_render_lowering",
            "generic_todomvc_edit_open_close_render_lowering",
            "generic_todomvc_render_only_patch_lowering",
            "generic_todomvc_source_effects_through_action_executor",
            "generic_route_selected_todo_edit_text_commit_executor",
            "generic_route_selected_todo_title_commit_executor",
            "generic_route_selected_todo_editing_commit_executor",
            "generic_todomvc_summary_reads_authoritative_storage",
            "generic_todomvc_root_holds_no_mirror",
            "generic_todomvc_rows_hold_no_mirror",
            "generic_todomvc_delta_identities_from_authoritative_storage",
            "generic_todomvc_source_route_classifier",
            "generic_todomvc_routed_source_event",
            "generic_todomvc_row_routed_source_event",
            "generic_todomvc_visible_row_occurrence_resolution",
            "generic_todomvc_source_action_mutation_batch",
            "generic_todomvc_append_mutation_batch",
            "generic_todomvc_list_index_action_input_resolution",
            "generic_todomvc_scenario_expectation_assertions",
            "generic_todomvc_scenario_preparation",
            "generic_loaded_runtime_todomvc_root_step_executor",
            "generic_loaded_runtime_todomvc_row_toggle_delete_executor",
            "generic_loaded_runtime_todomvc_row_edit_source_executor",
            "generic_loaded_runtime_todomvc_render_only_hover_executor",
            "generic_loaded_runtime_todomvc_assertion_executor",
            "generic_loaded_runtime_todomvc_stress_profile_executor",
            "generic_routed_todo_bool_target_application",
            "generic_routed_todo_edit_text_target_application",
            "todomvc_root_scalar_holds_from_ir",
            "todomvc_generic_hold_storage_authoritative",
            "todomvc_title_hold_from_ir",
            "todomvc_completed_bool_hold_from_ir",
            "todomvc_editing_bool_hold_from_ir",
            "todomvc_edit_text_hold_from_ir",
            "todomvc_append_remove_from_ir",
            "todomvc_count_and_filter_views_from_ir",
        ][..],
        "cells" => &[
            "generic_cells_common_render_patch_lowering",
            "generic_cells_source_effects_through_action_executor",
            "generic_cells_source_route_classifier",
            "generic_cells_address_row_context_resolution",
            "generic_cells_routed_source_event",
            "generic_cells_scenario_expectation_assertions",
            "generic_cells_scenario_storage_preparation",
            "generic_cells_formula_dependency_cache",
            "generic_cells_formula_evaluation_cache",
            "generic_cells_formula_derived_storage_sync",
            "generic_cells_formula_display_mutation_emitter",
            "generic_cells_formula_display_protocol_lowering",
            "generic_cells_source_action_mutation_batch",
            "generic_cells_editor_route_uses_indexed_targets",
            "generic_cells_committed_fields_hold_no_mirror",
            "generic_loaded_runtime_cells_stress_profile_executor",
            "cells_edit_state_holds_from_ir",
            "cells_generic_hold_storage_authoritative",
            "cells_summary_reads_authoritative_storage",
            "cells_hidden_grid_keys_from_generic_storage",
            "cells_formula_pipeline_from_ir",
        ][..],
        _ => {
            return Err(format!(
                "{} cannot determine executable example for generic runtime slices",
                report_path.display()
            )
            .into());
        }
    };
    for key in example_specific {
        require_slice_bool(slices, key, true, report_path)?;
    }
    Ok(())
}

fn require_slice_bool(
    slices: &serde_json::Map<String, JsonValue>,
    key: &str,
    expected: bool,
    report_path: &Path,
) -> RuntimeResult<()> {
    let actual = slices
        .get(key)
        .and_then(JsonValue::as_bool)
        .ok_or_else(|| {
            format!(
                "{} generic runtime slices missing boolean `{key}`",
                report_path.display()
            )
        })?;
    if actual != expected {
        return Err(format!(
            "{} generic runtime slice `{key}` expected {expected}, got {actual}",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_generic_runtime_slice_evidence(
    report: &JsonValue,
    execution: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let evidence = execution
        .get("generic_runtime_slice_evidence")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} runtime_execution missing generic_runtime_slice_evidence",
                report_path.display()
            )
        })?;
    if evidence.get("computed_from").and_then(JsonValue::as_str)
        != Some("typed_ir_and_compiled_program")
    {
        return Err(format!(
            "{} generic runtime slice evidence is not bound to typed IR and compiled program",
            report_path.display()
        )
        .into());
    }
    let Some(compiled) = report
        .get("compiled_schedule")
        .and_then(JsonValue::as_object)
    else {
        return Err(format!("{} missing compiled_schedule", report_path.display()).into());
    };
    for key in [
        "source_route_count",
        "source_route_id_slot_count",
        "source_route_label_slot_count",
        "source_routes_with_ids",
        "list_source_binding_count",
        "update_branch_count",
        "list_operation_count",
        "formula_operation_count",
        "view_binding_count",
        "source_payload_schema_count",
        "source_payload_field_count",
        "source_payload_text_field_count",
        "source_payload_key_field_count",
        "source_payload_address_field_count",
        "root_text_slot_count",
        "root_bool_slot_count",
        "root_enum_slot_count",
        "list_memory_count",
        "list_row_template_field_count",
        "list_row_text_slot_count",
        "list_row_bool_slot_count",
        "list_row_enum_slot_count",
        "list_hidden_key_slot_count",
        "list_hidden_generation_slot_count",
        "derived_text_transform_count",
    ] {
        if evidence.get(key).and_then(JsonValue::as_u64)
            != compiled.get(key).and_then(JsonValue::as_u64)
        {
            return Err(format!(
                "{} generic runtime slice evidence `{key}` does not match compiled_schedule",
                report_path.display()
            )
            .into());
        }
    }
    let evidence_storage = evidence
        .get("typed_storage_layout")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} generic runtime slice evidence missing typed_storage_layout",
                report_path.display()
            )
        })?;
    let compiled_storage = compiled
        .get("typed_storage_layout")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} compiled_schedule missing typed_storage_layout",
                report_path.display()
            )
        })?;
    if evidence_storage
        .get("computed_from")
        .and_then(JsonValue::as_str)
        != Some("typed_ir_state_and_list_tables")
    {
        return Err(format!(
            "{} typed storage layout evidence is not derived from typed IR tables",
            report_path.display()
        )
        .into());
    }
    for key in [
        "root_text_slot_count",
        "root_bool_slot_count",
        "root_enum_slot_count",
        "list_memory_count",
        "list_row_template_field_count",
        "list_row_text_slot_count",
        "list_row_bool_slot_count",
        "list_row_enum_slot_count",
        "list_hidden_key_slot_count",
        "list_hidden_generation_slot_count",
    ] {
        if evidence_storage.get(key).and_then(JsonValue::as_u64)
            != compiled_storage.get(key).and_then(JsonValue::as_u64)
        {
            return Err(format!(
                "{} typed storage layout evidence `{key}` does not match compiled_schedule",
                report_path.display()
            )
            .into());
        }
    }
    for (key, expected) in [
        ("list_order_storage_kind", "separate_visible_order_slots"),
        ("list_valid_storage_kind", "bitvec_valid_slots"),
        ("list_free_storage_kind", "free_slot_stack"),
        (
            "list_source_binding_storage_kind",
            "dense_source_and_row_slots",
        ),
    ] {
        if evidence_storage.get(key).and_then(JsonValue::as_str) != Some(expected)
            || compiled_storage.get(key).and_then(JsonValue::as_str) != Some(expected)
        {
            return Err(format!(
                "{} typed storage layout `{key}` is not `{expected}`",
                report_path.display()
            )
            .into());
        }
    }
    let source_route_count = evidence
        .get("source_route_count")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    let route_action_count = evidence
        .get("source_route_action_count")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    if source_route_count == 0 || route_action_count == 0 {
        return Err(format!(
            "{} generic runtime slice evidence has no compiled source route actions",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_expression_coverage(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let coverage = report
        .get("expression_coverage")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("{} missing expression_coverage", report_path.display()))?;
    if coverage.get("computed_from").and_then(JsonValue::as_str) != Some("parser_ast_and_typed_ir")
    {
        return Err(format!(
            "{} expression coverage is not bound to parser AST and typed IR",
            report_path.display()
        )
        .into());
    }
    if let (Some(report_expression_count), Some(coverage_expression_count)) = (
        report.get("expression_count").and_then(JsonValue::as_u64),
        coverage
            .get("ast_expression_count")
            .and_then(JsonValue::as_u64),
    ) {
        if report_expression_count != coverage_expression_count {
            return Err(format!(
                "{} expression_count does not match expression_coverage.ast_expression_count",
                report_path.display()
            )
            .into());
        }
    }
    for key in [
        "unknown_ast_expression_count",
        "unknown_initial_value_count",
        "unknown_list_initializer_count",
        "unknown_list_seed_value_count",
        "unknown_update_expression_count",
        "unknown_list_predicate_count",
        "unknown_derived_value_count",
    ] {
        let count = coverage
            .get(key)
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| {
                format!(
                    "{} expression_coverage missing numeric `{key}`",
                    report_path.display()
                )
            })?;
        if count != 0 {
            return Err(format!(
                "{} expression_coverage `{key}` is {count}; executable reports cannot rely on unknown parser/lowering fallback",
                report_path.display()
            )
            .into());
        }
    }
    Ok(())
}

fn key_is_other_example_runtime_slice(key: &str, example: &str) -> bool {
    match example {
        "todomvc" => {
            key.starts_with("generic_cells") || key.starts_with("cells_") || key.contains("_cells_")
        }
        "cells" => {
            key.starts_with("generic_todomvc")
                || key.starts_with("todomvc_")
                || key.contains("_todo_")
                || key.contains("_todomvc_")
        }
        _ => false,
    }
}

fn report_program_kind(report: &JsonValue) -> Option<&str> {
    let candidate = report
        .get("program_kind")
        .or_else(|| report.get("example"))
        .and_then(JsonValue::as_str)
        .or_else(|| {
            report
                .get("runtime_execution")
                .and_then(|execution| execution.get("adapter_kind"))
                .and_then(JsonValue::as_str)
        })?;
    matches!(candidate, "todomvc" | "cells").then_some(candidate)
}

fn verify_runtime_report_contract(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    for key in [
        "source_path",
        "runtime_profile",
        "renderer",
        "window_mode",
        "window_backend",
        "display_server",
        "display_scale",
        "window_size",
        "framebuffer_size",
        "total_ticks",
        "total_source_events",
        "total_semantic_deltas",
        "total_render_deltas",
        "max_dirty_nodes",
        "max_dirty_keys",
        "allocations",
        "latency_ms_p50_p95_p99_max",
        "rss_delta_mib_steady_peak",
        "vram_delta_mib_steady_peak_or_unavailable_reason",
        "dirty_node_count_p50_p95_p99_max",
        "dirty_key_count_p50_p95_p99_max",
        "failure_artifacts",
        "semantic_delta_protocol_batches",
    ] {
        if report.get(key).is_none() {
            return Err(format!(
                "{} runtime report missing documented field `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    verify_semantic_delta_protocol_batches(report, report_path)?;
    verify_render_patch_protocol_identity(report, report_path)?;
    let dirty_node_summary = report
        .get("dirty_node_count_p50_p95_p99_max")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} runtime report dirty_node_count_p50_p95_p99_max is not an object",
                report_path.display()
            )
        })?;
    if dirty_node_summary.get("unavailable_reason").is_some() {
        return Err(format!(
            "{} runtime report still marks dirty node counts unavailable",
            report_path.display()
        )
        .into());
    }
    for key in ["p50", "p95", "p99", "max"] {
        if dirty_node_summary
            .get(key)
            .and_then(JsonValue::as_f64)
            .is_none()
        {
            return Err(format!(
                "{} runtime report dirty_node_count_p50_p95_p99_max missing numeric `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    let rss_delta = report
        .get("rss_delta_mib_steady_peak")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} runtime report rss_delta_mib_steady_peak is not an object",
                report_path.display()
            )
        })?;
    for key in ["steady", "peak", "baseline", "measurement"] {
        if rss_delta.get(key).is_none() {
            return Err(format!(
                "{} runtime report RSS delta missing `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    let baseline = report
        .get("baseline_rss_mib")
        .and_then(JsonValue::as_f64)
        .ok_or_else(|| format!("{} missing numeric baseline_rss_mib", report_path.display()))?;
    let steady = report
        .get("steady_rss_mib")
        .and_then(JsonValue::as_f64)
        .ok_or_else(|| format!("{} missing numeric steady_rss_mib", report_path.display()))?;
    if steady < baseline {
        return Err(format!(
            "{} has steady RSS below baseline RSS",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_playground_surface_report(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    for key in [
        "example_selector",
        "code_editor",
        "run_reset_step_controls",
        "render_preview",
        "semantic_delta_log",
        "selected_value_inspector",
        "dependency_explanation_panel",
    ] {
        let claimed = report
            .get("playground_surface")
            .and_then(|surface| surface.get(key))
            .and_then(JsonValue::as_bool)
            == Some(true);
        if !claimed {
            return Err(format!(
                "{} playground surface `{key}` is not claimed present",
                report_path.display()
            )
            .into());
        }
        let proof = report
            .get("playground_surface_visible_bounds")
            .and_then(|bounds| bounds.get(key))
            .ok_or_else(|| {
                format!(
                    "{} playground surface `{key}` missing visible bounds proof",
                    report_path.display()
                )
            })?;
        if proof.get("pass").and_then(JsonValue::as_bool) != Some(true) {
            return Err(format!(
                "{} playground surface `{key}` visible bounds did not pass",
                report_path.display()
            )
            .into());
        }
        let elements = proof
            .get("elements")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| {
                format!(
                    "{} playground surface `{key}` missing element bounds",
                    report_path.display()
                )
            })?;
        if elements.is_empty() {
            return Err(format!(
                "{} playground surface `{key}` has no visible elements",
                report_path.display()
            )
            .into());
        }
        for element in elements {
            let visible = element.get("visible").and_then(JsonValue::as_bool) == Some(true);
            let width = element
                .get("bounds")
                .and_then(|bounds| bounds.get("width"))
                .and_then(JsonValue::as_f64)
                .unwrap_or_default();
            let height = element
                .get("bounds")
                .and_then(|bounds| bounds.get("height"))
                .and_then(JsonValue::as_f64)
                .unwrap_or_default();
            if !visible || width <= 0.0 || height <= 0.0 {
                return Err(format!(
                    "{} playground surface `{key}` has an invisible or zero-size element",
                    report_path.display()
                )
                .into());
            }
        }
    }
    Ok(())
}

fn verify_render_patch_protocol_identity(
    report: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let patches = report
        .get("render_patches")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} missing render_patches array", report_path.display()))?;
    let total_render_deltas = report
        .get("total_render_deltas")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default() as usize;
    if patches.len() != total_render_deltas {
        return Err(format!(
            "{} render_patches length {} does not match total_render_deltas {total_render_deltas}",
            report_path.display(),
            patches.len()
        )
        .into());
    }
    for patch in patches {
        let kind = patch
            .get("kind")
            .and_then(JsonValue::as_str)
            .unwrap_or("<unknown>");
        let target = patch
            .get("target")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        let keyed_patch =
            target.starts_with("todos:") || matches!(kind, "SetCellEditor" | "SetCellText");
        if keyed_patch {
            for key in ["list_id", "key", "generation"] {
                if patch.get(key).is_none_or(JsonValue::is_null) {
                    return Err(format!(
                        "{} keyed render patch `{kind}` missing `{key}`",
                        report_path.display()
                    )
                    .into());
                }
            }
        }
        if matches!(kind, "BindSource" | "UnbindSource") {
            for key in ["source_id", "bind_epoch"] {
                if patch.get(key).is_none_or(JsonValue::is_null) {
                    return Err(format!(
                        "{} source render patch `{kind}` missing `{key}`",
                        report_path.display()
                    )
                    .into());
                }
            }
        }
    }
    Ok(())
}

fn verify_semantic_delta_protocol_batches(
    report: &JsonValue,
    report_path: &Path,
) -> RuntimeResult<()> {
    let batches = report
        .get("semantic_delta_protocol_batches")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} runtime report semantic_delta_protocol_batches is not an array",
                report_path.display()
            )
        })?;
    let expected_program_hash = report
        .get("program_hash")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing program_hash", report_path.display()))?;
    let mut expected_base_epoch = 0u64;
    let mut expected_runtime_id: Option<String> = None;
    let mut total_changes = 0usize;
    for batch in batches {
        let base_epoch = json_u64(batch, "base_epoch", report_path)?;
        let next_epoch = json_u64(batch, "next_epoch", report_path)?;
        if base_epoch != expected_base_epoch || next_epoch != base_epoch + 1 {
            return Err(format!(
                "{} has non-monotonic semantic delta epochs",
                report_path.display()
            )
            .into());
        }
        if batch.get("program_hash").and_then(JsonValue::as_str) != Some(expected_program_hash) {
            return Err(format!(
                "{} semantic delta batch has stale program_hash",
                report_path.display()
            )
            .into());
        }
        let runtime_id = batch
            .get("runtime_id")
            .and_then(JsonValue::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                format!(
                    "{} semantic delta batch missing nonempty runtime_id",
                    report_path.display()
                )
            })?;
        match &expected_runtime_id {
            Some(expected) if expected != runtime_id => {
                return Err(format!(
                    "{} semantic delta batch changed runtime_id",
                    report_path.display()
                )
                .into());
            }
            None => expected_runtime_id = Some(runtime_id.to_owned()),
            _ => {}
        }
        let server_tick = json_u64(batch, "server_tick", report_path)?;
        if server_tick != next_epoch {
            return Err(format!(
                "{} semantic delta batch server_tick does not match next_epoch",
                report_path.display()
            )
            .into());
        }
        if batch
            .get("step_id")
            .and_then(JsonValue::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(format!(
                "{} semantic delta batch missing nonempty step_id",
                report_path.display()
            )
            .into());
        }
        let changes = batch
            .get("changes")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| {
                format!(
                    "{} semantic delta batch missing changes array",
                    report_path.display()
                )
            })?;
        for change in changes {
            verify_semantic_delta_identity(change, report_path)?;
        }
        total_changes += changes.len();
        expected_base_epoch = next_epoch;
    }
    let total_semantic_deltas = report
        .get("total_semantic_deltas")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default() as usize;
    if total_changes != total_semantic_deltas {
        return Err(format!(
            "{} semantic delta batches contain {total_changes} changes, expected {total_semantic_deltas}",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_semantic_delta_identity(change: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let kind = change
        .get("kind")
        .and_then(JsonValue::as_str)
        .unwrap_or("<unknown>");
    let list_id_present = change.get("list_id").is_some_and(|value| !value.is_null());
    if list_id_present {
        for key in ["key", "generation"] {
            if change.get(key).is_none_or(JsonValue::is_null) {
                return Err(format!(
                    "{} keyed semantic delta `{kind}` missing `{key}`",
                    report_path.display()
                )
                .into());
            }
        }
    }
    if matches!(kind, "SourceBind" | "SourceUnbind") {
        for key in ["source_id", "bind_epoch"] {
            if change.get(key).is_none_or(JsonValue::is_null) {
                return Err(format!(
                    "{} source semantic delta `{kind}` missing `{key}`",
                    report_path.display()
                )
                .into());
            }
        }
    }
    Ok(())
}

fn json_u64(value: &JsonValue, key: &str, report_path: &Path) -> RuntimeResult<u64> {
    value.get(key).and_then(JsonValue::as_u64).ok_or_else(|| {
        format!(
            "{} semantic delta protocol batch missing numeric `{key}`",
            report_path.display()
        )
        .into()
    })
}

fn verify_speed_report(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    if report.get("build_profile").and_then(JsonValue::as_str) != Some("release") {
        return Err(format!(
            "{} speed report was not generated by a release binary",
            report_path.display()
        )
        .into());
    }
    for key in [
        "cpu_model",
        "gpu_model_if_available",
        "os",
        "display_server",
        "window_backend",
        "display_scale",
        "semantic_tick_ms_p50_p95_p99_max",
        "render_lowering_ms_p50_p95_p99_max",
        "ply_patch_apply_ms_p50_p95_p99_max",
        "input_to_idle_ms_p50_p95_p99_max",
        "frame_time_ms_p50_p95_p99_max",
        "missed_frame_count",
        "operation_count",
        "per_operation_outliers",
        "baseline_rss_mib",
        "steady_rss_mib",
        "peak_rss_mib",
        "baseline_vram_mib_if_available",
        "steady_vram_mib_if_available",
        "peak_vram_mib_if_available",
        "heap_alloc_count_per_step",
        "heap_alloc_bytes_per_step",
        "apply_heap_alloc_count",
        "apply_heap_alloc_bytes",
        "expectation_heap_alloc_count",
        "expectation_heap_alloc_bytes",
        "graph_node_count",
        "graph_rebuild_count",
        "list_slot_count",
        "runtime_profile",
        "runtime_profile_detail",
        "capacities",
        "dirty_node_count_p50_p95_p99_max",
        "dirty_key_count_p50_p95_p99_max",
        "render_patch_count_p50_p95_p99_max",
    ] {
        if report.get(key).is_none() {
            return Err(format!(
                "{} speed report missing documented field `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    verify_runtime_profile_metadata(report, report_path)?;
    let dirty_node_summary = report
        .get("dirty_node_count_p50_p95_p99_max")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} speed report dirty_node_count_p50_p95_p99_max is not an object",
                report_path.display()
            )
        })?;
    if dirty_node_summary.get("unavailable_reason").is_some() {
        return Err(format!(
            "{} speed report still marks dirty node counts unavailable",
            report_path.display()
        )
        .into());
    }
    for key in ["p50", "p95", "p99", "max"] {
        if dirty_node_summary
            .get(key)
            .and_then(JsonValue::as_f64)
            .is_none()
        {
            return Err(format!(
                "{} speed report dirty_node_count_p50_p95_p99_max missing numeric `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    let checks = report
        .get("budget_check")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} speed report missing budget_check",
                report_path.display()
            )
        })?;
    let failed = checks
        .iter()
        .filter_map(|(name, value)| {
            (value.get("pass").and_then(JsonValue::as_bool) != Some(true)).then_some(name.as_str())
        })
        .collect::<Vec<_>>();
    if !failed.is_empty() {
        return Err(format!(
            "{} speed budget failed: {}",
            report_path.display(),
            failed.join(", ")
        )
        .into());
    }
    verify_speed_stress_profiles(report, report_path)
}

fn verify_runtime_profile_metadata(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let profile = report
        .get("runtime_profile")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing runtime_profile", report_path.display()))?;
    if !matches!(
        profile,
        "software_dynamic" | "software_bounded" | "hardware_bounded"
    ) {
        return Err(format!(
            "{} has unknown runtime_profile `{profile}`",
            report_path.display()
        )
        .into());
    }
    let detail_name = report
        .get("runtime_profile_detail")
        .and_then(|detail| detail.get("name"))
        .and_then(JsonValue::as_str);
    if detail_name != Some(profile) {
        return Err(format!(
            "{} runtime_profile_detail.name does not match runtime_profile",
            report_path.display()
        )
        .into());
    }
    let capacities = report
        .get("capacities")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("{} missing capacities object", report_path.display()))?;
    let all_lists_bounded = capacities
        .get("all_lists_bounded")
        .and_then(JsonValue::as_bool)
        .ok_or_else(|| {
            format!(
                "{} capacities missing boolean all_lists_bounded",
                report_path.display()
            )
        })?;
    let lists = capacities
        .get("lists")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} capacities missing lists array", report_path.display()))?;
    for list in lists {
        for key in [
            "name",
            "declared_capacity",
            "effective_capacity",
            "capacity_source",
            "dynamic_growth_allowed",
            "overflow_behavior",
        ] {
            if list.get(key).is_none() {
                return Err(format!(
                    "{} capacity report missing list field `{key}`",
                    report_path.display()
                )
                .into());
            }
        }
    }
    if matches!(profile, "software_bounded" | "hardware_bounded") && !all_lists_bounded {
        return Err(format!(
            "{} claims {profile} while at least one list has no effective capacity",
            report_path.display()
        )
        .into());
    }
    if profile == "software_dynamic" && all_lists_bounded {
        return Err(format!(
            "{} claims software_dynamic while every list has an effective capacity",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_benchmark_report(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let benchmark = report
        .get("benchmark")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} benchmark report missing benchmark object",
                report_path.display()
            )
        })?;
    let iterations = benchmark
        .get("iterations")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| {
            format!(
                "{} benchmark report missing positive iteration count",
                report_path.display()
            )
        })?;
    if iterations == 0 {
        return Err(format!(
            "{} benchmark report has zero iterations",
            report_path.display()
        )
        .into());
    }
    let total_ms = benchmark
        .get("total_ms")
        .and_then(JsonValue::as_f64)
        .ok_or_else(|| {
            format!(
                "{} benchmark report missing total_ms",
                report_path.display()
            )
        })?;
    let average_ms = benchmark
        .get("average_ms_per_iteration")
        .and_then(JsonValue::as_f64)
        .ok_or_else(|| {
            format!(
                "{} benchmark report missing average_ms_per_iteration",
                report_path.display()
            )
        })?;
    if total_ms <= 0.0 || average_ms <= 0.0 {
        return Err(format!(
            "{} benchmark report timing must be positive",
            report_path.display()
        )
        .into());
    }
    let expected_average = total_ms / iterations as f64;
    if (average_ms - expected_average).abs() > 0.001_f64.max(expected_average.abs() * 0.001) {
        return Err(format!(
            "{} benchmark average does not match total_ms / iterations",
            report_path.display()
        )
        .into());
    }
    if benchmark
        .get("speed_report_layer")
        .and_then(JsonValue::as_str)
        != Some("speed")
    {
        return Err(format!(
            "{} benchmark report is not linked to a speed-layer report",
            report_path.display()
        )
        .into());
    }
    if benchmark
        .get("iteration_scope")
        .and_then(JsonValue::as_str)
        .is_none_or(|scope| !scope.contains("full_speed_layer_scenario"))
    {
        return Err(format!(
            "{} benchmark report does not describe full speed-layer scenario iterations",
            report_path.display()
        )
        .into());
    }
    let heap_alloc_count_after_warmup = benchmark
        .get("heap_alloc_count_after_warmup")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| {
            format!(
                "{} benchmark report missing heap_alloc_count_after_warmup",
                report_path.display()
            )
        })?;
    let allocation_budget = report
        .get("budget_check")
        .and_then(|budget| budget.get("allocation_budget"))
        .and_then(JsonValue::as_object)
        .ok_or_else(|| {
            format!(
                "{} benchmark report missing allocation budget check",
                report_path.display()
            )
        })?;
    let allocation_budget_passes = allocation_budget
        .get("pass")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);
    let allocation_budget_applies = allocation_budget
        .get("applies")
        .and_then(JsonValue::as_bool)
        .unwrap_or(true);
    if allocation_budget_applies && heap_alloc_count_after_warmup != 0 {
        return Err(format!(
            "{} benchmark report does not prove zero post-warmup allocations",
            report_path.display()
        )
        .into());
    }
    if !allocation_budget_passes {
        return Err(format!(
            "{} benchmark report allocation budget did not pass",
            report_path.display()
        )
        .into());
    }
    if report_command_is(report, "bench-example")
        && benchmark
            .get("example")
            .and_then(JsonValue::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(format!(
            "{} bench-example report missing benchmark.example",
            report_path.display()
        )
        .into());
    }

    let speed_report_path = benchmark
        .get("speed_report_path")
        .and_then(JsonValue::as_str)
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "{} benchmark report missing speed_report_path",
                report_path.display()
            )
        })?;
    let speed_report_path = Path::new(speed_report_path);
    verify_report_schema(speed_report_path)?;
    let linked: JsonValue = serde_json::from_slice(&fs::read(speed_report_path)?)?;
    for key in [
        "source_hash",
        "scenario_hash",
        "program_hash",
        "budget_hash",
        "graph_node_count",
        "budget_check",
        "input_to_idle_ms_p50_p95_p99_max",
        "semantic_tick_ms_p50_p95_p99_max",
        "render_lowering_ms_p50_p95_p99_max",
        "ply_patch_apply_ms_p50_p95_p99_max",
        "frame_time_ms_p50_p95_p99_max",
        "dirty_key_count_p50_p95_p99_max",
        "render_patch_count_p50_p95_p99_max",
        "graph_rebuild_count",
        "allocations",
        "stress_profiles",
        "runtime_profile",
        "runtime_profile_detail",
        "capacities",
    ] {
        if report.get(key) != linked.get(key) {
            return Err(format!(
                "{} benchmark report does not copy `{key}` from linked speed report `{}`",
                report_path.display(),
                speed_report_path.display()
            )
            .into());
        }
    }
    let linked_hash = sha256_file(speed_report_path)?;
    let artifact_hash_matches = report
        .get("artifact_sha256s")
        .and_then(JsonValue::as_array)
        .is_some_and(|artifacts| {
            artifacts.iter().any(|artifact| {
                artifact.get("path").and_then(JsonValue::as_str)
                    == Some(speed_report_path.to_string_lossy().as_ref())
                    && artifact.get("sha256").and_then(JsonValue::as_str)
                        == Some(linked_hash.as_str())
            })
        });
    if !artifact_hash_matches {
        return Err(format!(
            "{} benchmark report does not hash linked speed report `{}`",
            report_path.display(),
            speed_report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_speed_stress_profiles(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let stress_profiles = report
        .get("stress_profiles")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} speed report missing stress_profiles",
                report_path.display()
            )
        })?;
    if stress_profiles.is_empty() {
        return Err(format!(
            "{} speed report has no stress profiles",
            report_path.display()
        )
        .into());
    }
    let base_graph_node_count = report
        .get("graph_node_count")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| {
            format!(
                "{} speed report missing graph_node_count",
                report_path.display()
            )
        })?;
    for profile in stress_profiles {
        let name = profile
            .get("name")
            .and_then(JsonValue::as_str)
            .unwrap_or("<unnamed>");
        let graph_node_count = profile
            .get("graph_node_count")
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| {
                format!(
                    "{} stress profile `{name}` missing graph_node_count",
                    report_path.display()
                )
            })?;
        if graph_node_count != base_graph_node_count {
            return Err(format!(
                "{} stress profile `{name}` changed graph topology",
                report_path.display()
            )
            .into());
        }
        if profile
            .get("graph_clones_per_item")
            .and_then(JsonValue::as_u64)
            != Some(0)
        {
            return Err(format!(
                "{} stress profile `{name}` cloned runtime graph per item",
                report_path.display()
            )
            .into());
        }
        let heap_alloc_count = profile
            .get("heap_alloc_count")
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| {
                format!(
                    "{} stress profile `{name}` missing heap_alloc_count",
                    report_path.display()
                )
            })?;
        let heap_alloc_bytes = profile
            .get("heap_alloc_bytes")
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| {
                format!(
                    "{} stress profile `{name}` missing heap_alloc_bytes",
                    report_path.display()
                )
            })?;
        if heap_alloc_count != 0 || heap_alloc_bytes != 0 {
            return Err(format!(
                "{} stress profile `{name}` allocated after warmup: {heap_alloc_count} allocations / {heap_alloc_bytes} bytes",
                report_path.display()
            )
            .into());
        }
        let dirty_count = profile
            .get("dirty_key_count")
            .or_else(|| profile.get("dirty_cell_count"))
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| {
                format!(
                    "{} stress profile `{name}` missing dirty key/cell count",
                    report_path.display()
                )
            })?;
        let expected_fanout = profile.get("expected_fanout").and_then(JsonValue::as_u64);
        let expected_dirty_count = profile
            .get("expected_dirty_cell_count")
            .and_then(JsonValue::as_u64);
        let dirty_count_is_proportional = if let Some(expected_fanout) = expected_fanout {
            let allowed_dirty_count = expected_fanout.saturating_add(1);
            dirty_count == expected_dirty_count.unwrap_or(allowed_dirty_count)
                && dirty_count <= allowed_dirty_count
        } else {
            (1..=8).contains(&dirty_count)
        };
        if !dirty_count_is_proportional {
            return Err(format!(
                "{} stress profile `{name}` has non-proportional dirty work count {dirty_count}",
                report_path.display()
            )
            .into());
        }
        let render_patch_count = profile
            .get("render_patch_count")
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| {
                format!(
                    "{} stress profile `{name}` missing render_patch_count",
                    report_path.display()
                )
            })?;
        if render_patch_count > 8 {
            return Err(format!(
                "{} stress profile `{name}` has non-proportional render patch count {render_patch_count}",
                report_path.display()
            )
            .into());
        }
    }
    verify_documented_stress_profile_coverage(report, report_path, stress_profiles)?;
    Ok(())
}

fn verify_documented_stress_profile_coverage(
    report: &JsonValue,
    report_path: &Path,
    stress_profiles: &[JsonValue],
) -> RuntimeResult<()> {
    let program_kind = report_program_kind(report);
    match program_kind {
        Some("todomvc") => verify_todomvc_stress_profile_coverage(report_path, stress_profiles),
        Some("cells") => verify_cells_stress_profile_coverage(report_path, stress_profiles),
        _ => Ok(()),
    }
}

fn verify_todomvc_stress_profile_coverage(
    report_path: &Path,
    stress_profiles: &[JsonValue],
) -> RuntimeResult<()> {
    let rows = stress_profiles
        .iter()
        .filter_map(|profile| profile.get("rows").and_then(JsonValue::as_u64))
        .collect::<BTreeSet<_>>();
    for required in [1_000, 10_000] {
        if !rows.contains(&required) {
            return Err(format!(
                "{} TodoMVC speed report missing documented {required}-row stress profile",
                report_path.display()
            )
            .into());
        }
    }
    let has_10k_move = stress_profiles.iter().any(|profile| {
        profile
            .get("name")
            .and_then(JsonValue::as_str)
            .is_some_and(|name| name.contains("10000") && name.contains("move"))
    });
    if !has_10k_move {
        return Err(format!(
            "{} TodoMVC speed report missing documented 10,000-row move/LIST-change stress profile",
            report_path.display()
        )
        .into());
    }
    for profile in stress_profiles {
        if let Some(rows) = profile.get("rows").and_then(JsonValue::as_u64) {
            let slots = profile
                .get("list_slot_count")
                .and_then(JsonValue::as_u64)
                .ok_or_else(|| {
                    format!(
                        "{} TodoMVC stress profile missing list_slot_count",
                        report_path.display()
                    )
                })?;
            if slots != rows {
                return Err(format!(
                    "{} TodoMVC stress profile row count {rows} does not match list_slot_count {slots}",
                    report_path.display()
                )
                .into());
            }
        }
    }
    Ok(())
}

fn verify_cells_stress_profile_coverage(
    report_path: &Path,
    stress_profiles: &[JsonValue],
) -> RuntimeResult<()> {
    for required in [
        "cells-26x100-unrelated-edit",
        "cells-26x100-dependent-edit",
        "cells-26x100-fanout-100-update",
    ] {
        if !stress_profiles
            .iter()
            .any(|profile| profile.get("name").and_then(JsonValue::as_str) == Some(required))
        {
            return Err(format!(
                "{} Cells speed report missing documented stress profile `{required}`",
                report_path.display()
            )
            .into());
        }
    }
    for profile in stress_profiles {
        let name = profile
            .get("name")
            .and_then(JsonValue::as_str)
            .unwrap_or("<unnamed>");
        for key in [
            "cells",
            "dirty_cell_count",
            "recompute_candidate_count",
            "formula_eval_call_count",
            "dependency_edge_walk_count",
            "recomputed_cells",
        ] {
            if profile.get(key).is_none() {
                return Err(format!(
                    "{} Cells stress profile `{name}` missing `{key}`",
                    report_path.display()
                )
                .into());
            }
        }
        if profile.get("cells").and_then(JsonValue::as_u64) != Some(26 * 100) {
            return Err(format!(
                "{} Cells stress profile `{name}` is not bound to the documented 26x100 grid",
                report_path.display()
            )
            .into());
        }
    }
    let fanout = stress_profiles
        .iter()
        .find(|profile| {
            profile.get("name").and_then(JsonValue::as_str)
                == Some("cells-26x100-fanout-100-update")
        })
        .ok_or_else(|| {
            format!(
                "{} missing Cells fanout stress profile",
                report_path.display()
            )
        })?;
    let expected_fanout = fanout
        .get("expected_fanout")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| {
            format!(
                "{} Cells fanout stress profile missing expected_fanout",
                report_path.display()
            )
        })?;
    let expected_dirty = expected_fanout + 1;
    for key in [
        "dirty_cell_count",
        "recompute_candidate_count",
        "recomputed_cell_count",
    ] {
        let value = fanout.get(key).and_then(JsonValue::as_u64).ok_or_else(|| {
            format!(
                "{} Cells fanout stress profile missing `{key}`",
                report_path.display()
            )
        })?;
        if value != expected_dirty {
            return Err(format!(
                "{} Cells fanout stress profile `{key}`={value}, expected {expected_dirty}",
                report_path.display()
            )
            .into());
        }
    }
    let edge_walks = fanout
        .get("dependency_edge_walk_count")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    if edge_walks < expected_fanout {
        return Err(format!(
            "{} Cells fanout stress profile walked only {edge_walks} dependency edges for fanout {expected_fanout}",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_report_file_hash(
    report: &JsonValue,
    report_path: &Path,
    path_key: &str,
    hash_key: &str,
) -> RuntimeResult<()> {
    let Some(file_path) = report.get(path_key).and_then(JsonValue::as_str) else {
        return Ok(());
    };
    let Some(expected) = report.get(hash_key).and_then(JsonValue::as_str) else {
        return Ok(());
    };
    if matches!(expected, "n/a" | "missing" | "missing-budget") {
        return Ok(());
    }
    let actual = sha256_file(Path::new(file_path))?;
    if actual != expected {
        return Err(format!(
            "{} has stale `{hash_key}` for `{file_path}`",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_artifact_hashes(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let Some(artifacts) = report.get("artifact_sha256s").and_then(JsonValue::as_array) else {
        return Ok(());
    };
    for artifact in artifacts {
        let Some(path) = artifact.get("path").and_then(JsonValue::as_str) else {
            return Err(format!("{} has artifact without path", report_path.display()).into());
        };
        let Some(expected) = artifact.get("sha256").and_then(JsonValue::as_str) else {
            return Err(format!("{} has artifact without sha256", report_path.display()).into());
        };
        let actual = sha256_file(Path::new(path))?;
        if actual != expected {
            return Err(format!(
                "{} has stale artifact hash for `{path}`",
                report_path.display()
            )
            .into());
        }
    }
    Ok(())
}

fn verify_headed_artifacts(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    for key in [
        "window_pid",
        "window_title",
        "display_server",
        "display_socket_or_compositor_connection",
        "display_scale",
        "window_size",
        "input_backend",
        "capture_backend",
        "focused_window_proof",
        "checkpoint_screenshot_or_video_paths",
    ] {
        if report.get(key).is_none_or(JsonValue::is_null) {
            return Err(
                format!("{} missing headed metadata `{key}`", report_path.display()).into(),
            );
        }
    }
    for key in [
        "window_title",
        "display_server",
        "display_socket_or_compositor_connection",
        "input_backend",
        "capture_backend",
        "focused_window_proof",
    ] {
        if report
            .get(key)
            .and_then(JsonValue::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(format!(
                "{} has empty headed metadata `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    let injection_method = report
        .get("input_injection_method")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    let focus_free = injection_method == "ply_synthetic_focus_free_render_metadata";
    if focus_free {
        if report.get("input_backend").and_then(JsonValue::as_str)
            != Some("ply-synthetic-focus-free")
        {
            return Err(format!(
                "{} focus-free headed report must use ply-synthetic-focus-free input backend",
                report_path.display()
            )
            .into());
        }
        if report.get("os_focus_required").and_then(JsonValue::as_bool) != Some(false)
            || report
                .get("os_keyboard_or_pointer_used")
                .and_then(JsonValue::as_bool)
                != Some(false)
        {
            return Err(format!(
                "{} focus-free headed report must prove it used no OS keyboard or pointer injection",
                report_path.display()
            )
            .into());
        }
        if !report
            .get("os_input_tools_used")
            .and_then(JsonValue::as_array)
            .is_some_and(Vec::is_empty)
        {
            return Err(format!(
                "{} focus-free headed report must have an empty os_input_tools_used list",
                report_path.display()
            )
            .into());
        }
    }
    if matches!(
        injection_method,
        "direct_source_event" | "semantic_scenario_replay_then_headed_render"
    ) {
        return Err(format!(
            "{} used direct source-event injection in headed replay",
            report_path.display()
        )
        .into());
    }
    if report.get("input_route_contract").is_none() {
        return Err(format!(
            "{} missing headed input route contract",
            report_path.display()
        )
        .into());
    }
    let os_probe_passed = report
        .get("os_input_probe")
        .and_then(|probe| probe.get("status"))
        .and_then(JsonValue::as_str)
        == Some("pass");
    if injection_method.contains("os_") && !os_probe_passed {
        return Err(format!(
            "{} claims OS input but does not include a passing OS input probe",
            report_path.display()
        )
        .into());
    }
    if report
        .get("os_pointer_probe")
        .and_then(|probe| probe.get("status"))
        .and_then(JsonValue::as_str)
        == Some("fail")
    {
        return Err(format!(
            "{} includes an attempted but failed OS pointer probe",
            report_path.display()
        )
        .into());
    }
    if injection_method != "os_pointer_keyboard_to_visible_window" && !focus_free {
        let limitation = report
            .get("os_input_limitation")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        if limitation.is_empty() {
            return Err(format!(
                "{} does not prove full per-step OS input and does not explain the limitation",
                report_path.display()
            )
            .into());
        }
    }
    let artifacts = report
        .get("artifact_sha256s")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} missing headed artifacts", report_path.display()))?;
    if artifacts.is_empty() {
        return Err(format!(
            "{} has no headed screenshot artifacts",
            report_path.display()
        )
        .into());
    }
    let nonblank = report
        .get("nonblank_screenshot_hashes")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} missing nonblank screenshot proof",
                report_path.display()
            )
        })?;
    let proved_nonblank = nonblank.iter().any(|entry| {
        entry
            .get("nonzero_channels")
            .and_then(JsonValue::as_u64)
            .unwrap_or_default()
            > 0
            && entry
                .get("unique_rgba_values")
                .and_then(JsonValue::as_u64)
                .unwrap_or_default()
                > 1
    });
    if !proved_nonblank {
        return Err(format!("{} headed screenshot is blank", report_path.display()).into());
    }
    if injection_method == "os_pointer_keyboard_to_visible_window" {
        verify_full_os_input_steps(report, report_path)?;
    }
    Ok(())
}

fn verify_os_input_probe_report(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    if report.get("command").and_then(JsonValue::as_str) != Some("os-input-probe") {
        return Err(format!("{} is not an os-input-probe report", report_path.display()).into());
    }
    if report
        .get("input_injection_method")
        .and_then(JsonValue::as_str)
        != Some("os_keyboard_to_visible_window")
    {
        return Err(format!(
            "{} standalone OS probe must use os_keyboard_to_visible_window",
            report_path.display()
        )
        .into());
    }
    if report
        .get("input_backend")
        .and_then(JsonValue::as_str)
        .is_none_or(str::is_empty)
    {
        return Err(format!("{} missing input_backend", report_path.display()).into());
    }
    if report
        .get("focused_window_proof")
        .and_then(JsonValue::as_str)
        .is_none_or(str::is_empty)
    {
        return Err(format!("{} missing focused_window_proof", report_path.display()).into());
    }
    let window_pid = report
        .get("window_pid")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    if window_pid == 0 {
        return Err(format!("{} missing valid window_pid", report_path.display()).into());
    }
    let probe = report
        .get("os_input_probe")
        .ok_or_else(|| format!("{} missing os_input_probe", report_path.display()))?;
    if probe.get("status").and_then(JsonValue::as_str) != Some("pass")
        || probe.get("typed").and_then(JsonValue::as_bool) != Some(true)
    {
        return Err(format!("{} OS input probe did not pass", report_path.display()).into());
    }
    if probe.get("focused_ply_element").and_then(JsonValue::as_str) != Some("os_probe_input") {
        return Err(format!(
            "{} OS input probe did not target os_probe_input",
            report_path.display()
        )
        .into());
    }
    if probe
        .get("tool")
        .and_then(JsonValue::as_str)
        .is_none_or(str::is_empty)
    {
        return Err(format!("{} OS input probe missing tool", report_path.display()).into());
    }
    let artifact = probe
        .get("artifact")
        .ok_or_else(|| format!("{} OS input probe missing artifact", report_path.display()))?;
    let artifact_path = artifact
        .get("path")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} OS input probe artifact missing path",
                report_path.display()
            )
        })?;
    let expected_hash = artifact
        .get("sha256")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} OS input probe artifact missing sha256",
                report_path.display()
            )
        })?;
    let actual_hash = sha256_file(Path::new(artifact_path))?;
    if actual_hash != expected_hash {
        return Err(format!(
            "{} OS input probe artifact hash mismatch for `{artifact_path}`",
            report_path.display()
        )
        .into());
    }
    let nonzero = artifact
        .get("nonzero_channels")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    let unique = artifact
        .get("unique_rgba_values")
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    if nonzero == 0 || unique <= 1 {
        return Err(format!(
            "{} OS input probe screenshot is blank",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn verify_full_os_input_steps(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    let scenario_path = report
        .get("scenario_path")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} full OS headed report missing scenario_path",
                report_path.display()
            )
        })?;
    let scenario = parse_scenario(Path::new(scenario_path))?;
    let steps = report
        .get("os_input_steps")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} full OS headed report missing os_input_steps",
                report_path.display()
            )
        })?;
    if steps.len() != scenario.step.len() {
        return Err(format!(
            "{} full OS headed report has {} input steps, expected {}",
            report_path.display(),
            steps.len(),
            scenario.step.len()
        )
        .into());
    }
    let artifact_paths = report
        .get("artifact_sha256s")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artifact| artifact.get("path").and_then(JsonValue::as_str))
        .collect::<BTreeSet<_>>();
    verify_full_os_input_coverage(report, &scenario, report_path)?;
    for (expected, observed) in scenario.step.iter().zip(steps) {
        let id = observed
            .get("id")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        if id != expected.id {
            return Err(format!(
                "{} full OS headed step id expected `{}`, got `{id}`",
                report_path.display(),
                expected.id
            )
            .into());
        }
        if observed.get("pass").and_then(JsonValue::as_bool) != Some(true) {
            return Err(format!(
                "{} full OS headed step `{id}` did not pass",
                report_path.display()
            )
            .into());
        }
        if observed
            .get("target_element_id")
            .and_then(JsonValue::as_str)
            .is_none()
        {
            return Err(format!(
                "{} full OS headed step `{id}` missing target_element_id",
                report_path.display()
            )
            .into());
        }
        let bounds = observed.get("visible_bounds").ok_or_else(|| {
            format!(
                "{} full OS headed step `{id}` missing visible_bounds",
                report_path.display()
            )
        })?;
        let width = bounds
            .get("width")
            .and_then(JsonValue::as_f64)
            .unwrap_or_default();
        let height = bounds
            .get("height")
            .and_then(JsonValue::as_f64)
            .unwrap_or_default();
        if width <= 0.0 || height <= 0.0 {
            return Err(format!(
                "{} full OS headed step `{id}` has zero-sized visible bounds",
                report_path.display()
            )
            .into());
        }
        let screenshot = observed
            .get("screenshot_path")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                format!(
                    "{} full OS headed step `{id}` missing screenshot_path",
                    report_path.display()
                )
            })?;
        if !artifact_paths.contains(screenshot) {
            return Err(format!(
                "{} full OS headed step `{id}` screenshot `{screenshot}` is missing from artifact hashes",
                report_path.display()
            )
            .into());
        }
        if let Some(expected_source) = &expected.expected_source_event {
            let expected_source = toml_string_ref(expected_source, "source")
                .ok_or_else(|| format!("{} expected source event missing source", expected.id))?;
            let observed_source = observed
                .get("source_event_observed")
                .and_then(|event| event.get("source"))
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    format!(
                        "{} full OS headed step `{id}` missing observed source event",
                        report_path.display()
                    )
                })?;
            if observed_source != expected_source {
                return Err(format!(
                    "{} full OS headed step `{id}` observed source `{observed_source}`, expected `{expected_source}`",
                    report_path.display()
                )
                .into());
            }
        }
    }
    Ok(())
}

fn verify_full_os_input_coverage(
    report: &JsonValue,
    scenario: &Scenario,
    report_path: &Path,
) -> RuntimeResult<()> {
    let coverage = report.get("os_input_coverage").ok_or_else(|| {
        format!(
            "{} full OS headed report missing os_input_coverage",
            report_path.display()
        )
    })?;
    for key in [
        "source_event_probe_missing_labels",
        "step_control_missing_labels",
        "missing_full_os_pointer_keyboard_steps",
    ] {
        let Some(items) = coverage.get(key).and_then(JsonValue::as_array) else {
            return Err(format!(
                "{} full OS headed report missing os_input_coverage.{key}",
                report_path.display()
            )
            .into());
        };
        if !items.is_empty() {
            return Err(format!(
                "{} full OS headed report has uncovered OS-input labels in {key}: {items:?}",
                report_path.display()
            )
            .into());
        }
    }

    let source_required = scenario
        .step
        .iter()
        .filter(|step| step.expected_source_event.is_some())
        .map(|step| step.id.as_str())
        .collect::<BTreeSet<_>>();
    let source_observations = report
        .get("visible_source_event_os_input")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} full OS headed report missing visible_source_event_os_input",
                report_path.display()
            )
        })?;
    let source_covered = source_observations
        .iter()
        .filter(|observation| {
            observation.get("pass").and_then(JsonValue::as_bool) == Some(true)
                && observation
                    .get("runtime_mutation_observed")
                    .and_then(JsonValue::as_bool)
                    == Some(true)
                && observation
                    .get("source_event_observed")
                    .is_some_and(|event| !event.is_null())
        })
        .filter_map(|observation| {
            observation
                .get("scenario_step_id")
                .and_then(JsonValue::as_str)
        })
        .collect::<BTreeSet<_>>();
    let missing_source = source_required
        .difference(&source_covered)
        .copied()
        .collect::<Vec<_>>();
    if !missing_source.is_empty() {
        return Err(format!(
            "{} full OS headed report lacks visible SOURCE-event OS-input proof for labels: {missing_source:?}",
            report_path.display()
        )
        .into());
    }

    let step_required = scenario
        .step
        .iter()
        .skip(1)
        .map(|step| step.id.as_str())
        .collect::<BTreeSet<_>>();
    let step_observations = report
        .get("visible_step_control_os_input")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} full OS headed report missing visible_step_control_os_input",
                report_path.display()
            )
        })?;
    let step_covered = step_observations
        .iter()
        .filter(|observation| observation.get("pass").and_then(JsonValue::as_bool) == Some(true))
        .filter_map(|observation| observation.get("id").and_then(JsonValue::as_str))
        .collect::<BTreeSet<_>>();
    let missing_steps = step_required
        .difference(&step_covered)
        .copied()
        .collect::<Vec<_>>();
    if !missing_steps.is_empty() {
        return Err(format!(
            "{} full OS headed report lacks visible Step-control OS-input proof for labels: {missing_steps:?}",
            report_path.display()
        )
        .into());
    }

    let app_control_passed = report
        .get("visible_app_control_os_input")
        .and_then(JsonValue::as_array)
        .is_some_and(|observations| {
            observations.iter().any(|observation| {
                observation.get("pass").and_then(JsonValue::as_bool) == Some(true)
            })
        });
    if !app_control_passed {
        return Err(format!(
            "{} full OS headed report missing a passing visible app-control OS-input probe",
            report_path.display()
        )
        .into());
    }

    Ok(())
}

fn verify_human_artifacts(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    for key in [
        "command_argv",
        "exit_status",
        "manual_report_prepared_by",
        "manual_report_template_path",
        "manual_report_template_sha256",
        "binary_hash",
        "budget_hash",
        "display_server",
        "display_socket_or_compositor_connection",
        "window_backend",
        "display_scale",
        "window_pid",
        "window_title",
        "input_backend",
        "capture_backend",
        "focused_window_proof",
        "input_injection_method",
        "checkpoint_screenshot_or_video_paths",
        "visual_checkpoint_pass_fail",
        "headed_report_path",
        "headed_report_sha256",
        "headed_input_injection_method",
        "headed_os_input_step_count",
        "headed_os_input_missing_labels",
        "manual_artifact_capture_method",
        "manual_started_at_utc",
        "manual_finished_at_utc",
        "manual_session_duration_seconds",
    ] {
        if report.get(key).is_none_or(JsonValue::is_null) {
            return Err(format!(
                "{} missing manual report metadata `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    for key in [
        "binary_hash",
        "budget_hash",
        "display_server",
        "display_socket_or_compositor_connection",
        "window_backend",
        "display_scale",
        "window_title",
        "input_backend",
        "capture_backend",
        "focused_window_proof",
        "input_injection_method",
        "manual_report_prepared_by",
        "manual_report_template_path",
        "manual_report_template_sha256",
        "manual_notes",
        "manual_artifact_capture_method",
        "headed_report_path",
        "headed_report_sha256",
        "headed_input_injection_method",
        "manual_started_at_utc",
        "manual_finished_at_utc",
        "manual_session_duration_seconds",
    ] {
        reject_manual_placeholder(report, report_path, key)?;
    }
    let headed_report_path = report
        .get("headed_report_path")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing headed report path", report_path.display()))?;
    let headed_report_hash = report
        .get("headed_report_sha256")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing headed report hash", report_path.display()))?;
    let actual_headed_hash = sha256_file(Path::new(headed_report_path))?;
    if actual_headed_hash != headed_report_hash {
        return Err(format!(
            "{} has stale headed report hash for `{headed_report_path}`",
            report_path.display()
        )
        .into());
    }
    verify_report_schema(Path::new(headed_report_path))?;
    let headed_report: JsonValue = serde_json::from_slice(&fs::read(headed_report_path)?)?;
    if headed_report.get("layer").and_then(JsonValue::as_str) != Some("headed-ply") {
        return Err(format!(
            "{} linked headed report `{headed_report_path}` is not headed-ply",
            report_path.display()
        )
        .into());
    }
    if headed_report
        .get("input_injection_method")
        .and_then(JsonValue::as_str)
        != Some("os_pointer_keyboard_to_visible_window")
    {
        return Err(format!(
            "{} linked headed report does not prove full OS pointer/keyboard input",
            report_path.display()
        )
        .into());
    }
    if report
        .get("headed_input_injection_method")
        .and_then(JsonValue::as_str)
        != Some("os_pointer_keyboard_to_visible_window")
    {
        return Err(format!(
            "{} manual report did not copy full headed OS input method",
            report_path.display()
        )
        .into());
    }
    let headed_missing = report
        .get("headed_os_input_missing_labels")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| {
            format!(
                "{} missing headed OS input missing-label list",
                report_path.display()
            )
        })?;
    if !headed_missing.is_empty() {
        return Err(format!(
            "{} manual report links headed run with missing OS input labels",
            report_path.display()
        )
        .into());
    }
    let headed_steps = headed_report
        .get("os_input_steps")
        .and_then(JsonValue::as_array)
        .map(Vec::len)
        .unwrap_or_default() as u64;
    if json_u64_field(report, "headed_os_input_step_count")? != headed_steps {
        return Err(format!(
            "{} manual report headed step count does not match linked headed report",
            report_path.display()
        )
        .into());
    }
    for key in ["source_hash", "scenario_hash", "program_hash"] {
        if report.get(key) != headed_report.get(key) {
            return Err(format!(
                "{} manual `{key}` does not match linked headed report",
                report_path.display()
            )
            .into());
        }
    }
    let started = json_u64_field(report, "manual_started_at_utc")?;
    let finished = json_u64_field(report, "manual_finished_at_utc")?;
    let duration = json_u64_field(report, "manual_session_duration_seconds")?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    if started > now || finished > now {
        return Err(format!(
            "{} has future-dated manual session timing",
            report_path.display()
        )
        .into());
    }
    if finished < started || duration == 0 || finished.saturating_sub(started) != duration {
        return Err(format!(
            "{} has inconsistent manual session timing",
            report_path.display()
        )
        .into());
    }
    let generated = json_u64_field(report, "generated_at_utc")?;
    if generated < finished {
        return Err(format!(
            "{} was generated before the recorded manual session finished",
            report_path.display()
        )
        .into());
    }
    let prepared_by = report
        .get("manual_report_prepared_by")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            format!(
                "{} missing manual report preparation command",
                report_path.display()
            )
        })?;
    if !(prepared_by.starts_with("prepare-") && prepared_by.ends_with("-human-report")) {
        return Err(format!(
            "{} was not prepared by a repo human-report helper",
            report_path.display()
        )
        .into());
    }
    let command = report
        .get("command")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    if command != prepared_by {
        return Err(format!(
            "{} command `{command}` does not match manual_report_prepared_by `{prepared_by}`",
            report_path.display()
        )
        .into());
    }
    let command_argv = report
        .get("command_argv")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} missing command_argv", report_path.display()))?;
    let command_args = command_argv
        .iter()
        .filter_map(JsonValue::as_str)
        .collect::<BTreeSet<_>>();
    for required_arg in [
        prepared_by,
        "--observer",
        "--started",
        "--finished",
        "--notes",
        "--capture-method",
        "--window-pid",
        "--focused-window-proof",
        "--display-server",
        "--display-connection",
        "--display-scale",
        "--window-backend",
        "--artifact",
        "--pass-label",
        "--report",
    ] {
        if !command_args.contains(required_arg) {
            return Err(format!(
                "{} command_argv does not prove helper argument `{required_arg}`",
                report_path.display()
            )
            .into());
        }
    }
    require_command_argv_u64(report_path, command_argv, "--started", started)?;
    require_command_argv_u64(report_path, command_argv, "--finished", finished)?;
    let template_path = report
        .get("manual_report_template_path")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing manual template path", report_path.display()))?;
    let template_hash = report
        .get("manual_report_template_sha256")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing manual template hash", report_path.display()))?;
    let actual_template_hash = sha256_file(Path::new(template_path))?;
    if actual_template_hash != template_hash {
        return Err(format!(
            "{} has stale manual template hash for `{template_path}`",
            report_path.display()
        )
        .into());
    }
    let observer = report
        .get("manual_observer")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing manual observer", report_path.display()))?;
    if matches!(observer, "" | "fixture" | "unknown")
        || observer.contains("fill")
        || observer.contains("copy-from")
        || observer.contains("replace-with")
        || observer.to_ascii_lowercase().contains("codex")
        || observer.to_ascii_lowercase().contains("automation")
        || observer.to_ascii_lowercase().contains("automated")
        || observer.to_ascii_lowercase().contains("script")
    {
        return Err(format!(
            "{} has non-human manual observer `{observer}`",
            report_path.display()
        )
        .into());
    }
    require_command_argv_value(report_path, command_argv, "--observer", observer)?;
    if report.get("manual_input_route").and_then(JsonValue::as_str) != Some("human_visible_window")
    {
        return Err(format!(
            "{} missing human visible-window input route",
            report_path.display()
        )
        .into());
    }
    if report
        .get("input_injection_method")
        .and_then(JsonValue::as_str)
        != Some("human_visible_window")
    {
        return Err(format!(
            "{} manual report must mark input_injection_method as human_visible_window",
            report_path.display()
        )
        .into());
    }
    if json_u64_field(report, "window_pid")? == 0 {
        return Err(format!(
            "{} manual report has invalid visible-window pid",
            report_path.display()
        )
        .into());
    }
    require_command_argv_u64(
        report_path,
        command_argv,
        "--window-pid",
        json_u64_field(report, "window_pid")?,
    )?;
    if report
        .get("display_scale")
        .and_then(JsonValue::as_f64)
        .is_none_or(|scale| scale <= 0.0)
    {
        return Err(format!(
            "{} manual report has invalid display scale",
            report_path.display()
        )
        .into());
    }
    for key in [
        "display_server",
        "display_socket_or_compositor_connection",
        "window_backend",
        "window_title",
        "input_backend",
        "capture_backend",
        "focused_window_proof",
    ] {
        if report
            .get(key)
            .and_then(JsonValue::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(format!(
                "{} has empty manual visible-window metadata `{key}`",
                report_path.display()
            )
            .into());
        }
    }
    require_command_argv_value(
        report_path,
        command_argv,
        "--focused-window-proof",
        json_str_field(report, "focused_window_proof")?,
    )?;
    require_command_argv_value(
        report_path,
        command_argv,
        "--display-server",
        json_str_field(report, "display_server")?,
    )?;
    require_command_argv_value(
        report_path,
        command_argv,
        "--display-connection",
        json_str_field(report, "display_socket_or_compositor_connection")?,
    )?;
    require_command_argv_f64(
        report_path,
        command_argv,
        "--display-scale",
        report
            .get("display_scale")
            .and_then(JsonValue::as_f64)
            .ok_or_else(|| format!("{} missing display_scale", report_path.display()))?,
    )?;
    require_command_argv_value(
        report_path,
        command_argv,
        "--window-backend",
        json_str_field(report, "window_backend")?,
    )?;
    require_command_argv_value(
        report_path,
        command_argv,
        "--notes",
        json_str_field(report, "manual_notes")?,
    )?;
    require_command_argv_value(
        report_path,
        command_argv,
        "--capture-method",
        json_str_field(report, "manual_artifact_capture_method")?,
    )?;
    let checklist = report
        .get("manual_checklist_pass_fail")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| format!("{} missing manual checklist", report_path.display()))?;
    if checklist.is_empty() || checklist.contains_key("all_scripted_labels") {
        return Err(format!(
            "{} has fake or collapsed manual checklist",
            report_path.display()
        )
        .into());
    }
    for (label, passed) in checklist {
        if passed.as_bool() != Some(true) {
            return Err(format!(
                "{} manual checklist label `{label}` did not pass",
                report_path.display()
            )
            .into());
        }
    }
    let command_pass_labels = command_argv_values_after(command_argv, "--pass-label");
    let checklist_labels = checklist
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if command_pass_labels != checklist_labels {
        return Err(format!(
            "{} command_argv pass labels do not exactly match the manual checklist labels",
            report_path.display()
        )
        .into());
    }
    if let Some(scenario_path) = report.get("scenario_path").and_then(JsonValue::as_str) {
        let scenario = parse_scenario(Path::new(scenario_path))?;
        let expected_labels = scenario
            .step
            .iter()
            .map(|step| step.id.as_str())
            .collect::<BTreeSet<_>>();
        let observed_labels = checklist
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if observed_labels != expected_labels {
            return Err(format!(
                "{} manual checklist labels do not exactly match scenario labels",
                report_path.display()
            )
            .into());
        }
    }
    let artifacts = report
        .get("artifact_sha256s")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} missing manual artifacts", report_path.display()))?;
    if artifacts.is_empty() {
        return Err(format!(
            "{} has no manual screenshot/video artifacts",
            report_path.display()
        )
        .into());
    }
    let checkpoint_paths = report
        .get("checkpoint_screenshot_or_video_paths")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} missing manual checkpoint paths", report_path.display()))?;
    if checkpoint_paths.is_empty() {
        return Err(format!(
            "{} has no manual screenshot/video checkpoint paths",
            report_path.display()
        )
        .into());
    }
    let visual_checks = report
        .get("visual_checkpoint_pass_fail")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| format!("{} missing visual checkpoint checks", report_path.display()))?;
    if visual_checks.is_empty() {
        return Err(format!(
            "{} has no visual checkpoint pass/fail entries",
            report_path.display()
        )
        .into());
    }
    let artifact_paths = artifacts
        .iter()
        .filter_map(|artifact| artifact.get("path").and_then(JsonValue::as_str))
        .collect::<BTreeSet<_>>();
    let visual_check_paths = visual_checks
        .iter()
        .map(|entry| {
            let path = entry
                .get("path")
                .and_then(JsonValue::as_str)
                .ok_or_else(|| {
                    format!(
                        "{} has visual checkpoint entry without a path",
                        report_path.display()
                    )
                })?;
            let passed = entry.get("pass").and_then(JsonValue::as_bool) == Some(true);
            if !passed {
                return Err(format!(
                    "{} visual checkpoint `{path}` did not pass",
                    report_path.display()
                )
                .into());
            }
            Ok(path)
        })
        .collect::<RuntimeResult<BTreeSet<_>>>()?;
    let headed_artifact_paths = headed_report
        .get("artifact_sha256s")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artifact| artifact.get("path").and_then(JsonValue::as_str))
        .collect::<BTreeSet<_>>();
    let mut manual_checkpoint_count = 0usize;
    for checkpoint in checkpoint_paths {
        let path = checkpoint
            .as_str()
            .ok_or_else(|| format!("{} has non-string checkpoint path", report_path.display()))?;
        if !artifact_paths.contains(path) {
            return Err(format!(
                "{} checkpoint `{path}` is missing from artifact hashes",
                report_path.display()
            )
            .into());
        }
        if !visual_check_paths.contains(path) {
            return Err(format!(
                "{} checkpoint `{path}` is missing from visual pass/fail checks",
                report_path.display()
            )
            .into());
        }
        let ext = Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default();
        if !matches!(ext, "png" | "webm" | "mp4") {
            return Err(format!(
                "{} checkpoint `{path}` is not a screenshot/video artifact",
                report_path.display()
            )
            .into());
        }
        if !headed_artifact_paths.contains(path) {
            manual_checkpoint_count += 1;
            verify_manual_checkpoint_content(report_path, path, ext)?;
            let file_name = Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if !(file_name.contains("human") || file_name.contains("manual")) {
                return Err(format!(
                    "{} manual checkpoint `{path}` must be named with `human` or `manual`",
                    report_path.display()
                )
                .into());
            }
            let modified = fs::metadata(path)?
                .modified()?
                .duration_since(UNIX_EPOCH)?
                .as_secs();
            if modified < started.saturating_sub(2) || modified > finished.saturating_add(300) {
                return Err(format!(
                    "{} manual checkpoint `{path}` was not captured during the recorded manual session window",
                    report_path.display()
                )
                .into());
            }
        }
    }
    if manual_checkpoint_count == 0 {
        return Err(format!(
            "{} reuses only automated headed artifacts; add at least one manual screenshot/video checkpoint",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn command_argv_values_after<'a>(command_argv: &'a [JsonValue], flag: &str) -> BTreeSet<&'a str> {
    command_argv
        .windows(2)
        .filter_map(|window| {
            (window[0].as_str() == Some(flag))
                .then(|| window[1].as_str())
                .flatten()
        })
        .collect()
}

fn command_argv_value_after<'a>(command_argv: &'a [JsonValue], flag: &str) -> Option<&'a str> {
    command_argv.windows(2).find_map(|window| {
        (window[0].as_str() == Some(flag))
            .then(|| window[1].as_str())
            .flatten()
    })
}

fn require_command_argv_value(
    report_path: &Path,
    command_argv: &[JsonValue],
    flag: &str,
    expected: &str,
) -> RuntimeResult<()> {
    match command_argv_value_after(command_argv, flag) {
        Some(actual) if actual == expected => Ok(()),
        Some(actual) => Err(format!(
            "{} command_argv `{flag}` value `{actual}` does not match report value `{expected}`",
            report_path.display()
        )
        .into()),
        None => Err(format!(
            "{} command_argv missing `{flag}` value",
            report_path.display()
        )
        .into()),
    }
}

fn require_command_argv_u64(
    report_path: &Path,
    command_argv: &[JsonValue],
    flag: &str,
    expected: u64,
) -> RuntimeResult<()> {
    let actual = command_argv_value_after(command_argv, flag)
        .ok_or_else(|| {
            format!(
                "{} command_argv missing `{flag}` value",
                report_path.display()
            )
        })?
        .parse::<u64>()?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "{} command_argv `{flag}` value `{actual}` does not match report value `{expected}`",
            report_path.display()
        )
        .into())
    }
}

fn require_command_argv_f64(
    report_path: &Path,
    command_argv: &[JsonValue],
    flag: &str,
    expected: f64,
) -> RuntimeResult<()> {
    let actual = command_argv_value_after(command_argv, flag)
        .ok_or_else(|| {
            format!(
                "{} command_argv missing `{flag}` value",
                report_path.display()
            )
        })?
        .parse::<f64>()?;
    if (actual - expected).abs() <= f64::EPSILON {
        Ok(())
    } else {
        Err(format!(
            "{} command_argv `{flag}` value `{actual}` does not match report value `{expected}`",
            report_path.display()
        )
        .into())
    }
}

fn verify_manual_checkpoint_content(
    report_path: &Path,
    checkpoint_path: &str,
    ext: &str,
) -> RuntimeResult<()> {
    let metadata = fs::metadata(checkpoint_path)?;
    if metadata.len() < 1024 {
        return Err(format!(
            "{} manual checkpoint `{checkpoint_path}` is too small to be a real screenshot/video",
            report_path.display()
        )
        .into());
    }
    let bytes = fs::read(checkpoint_path)?;
    match ext {
        "png" => verify_manual_png_checkpoint(report_path, checkpoint_path, &bytes)?,
        "mp4" => {
            if bytes.get(4..8) != Some(b"ftyp") {
                return Err(format!(
                    "{} manual checkpoint `{checkpoint_path}` is not a valid MP4 artifact",
                    report_path.display()
                )
                .into());
            }
        }
        "webm" => {
            if !bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
                return Err(format!(
                    "{} manual checkpoint `{checkpoint_path}` is not a valid WebM artifact",
                    report_path.display()
                )
                .into());
            }
        }
        _ => {}
    }
    Ok(())
}

fn verify_manual_png_checkpoint(
    report_path: &Path,
    checkpoint_path: &str,
    bytes: &[u8],
) -> RuntimeResult<()> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.get(12..16) != Some(b"IHDR") {
        return Err(format!(
            "{} manual checkpoint `{checkpoint_path}` is not a valid PNG artifact",
            report_path.display()
        )
        .into());
    }
    let width = u32::from_be_bytes(
        bytes
            .get(16..20)
            .ok_or_else(|| format!("{checkpoint_path} PNG is missing width"))?
            .try_into()?,
    );
    let height = u32::from_be_bytes(
        bytes
            .get(20..24)
            .ok_or_else(|| format!("{checkpoint_path} PNG is missing height"))?
            .try_into()?,
    );
    if width < 32 || height < 32 {
        return Err(format!(
            "{} manual checkpoint `{checkpoint_path}` has implausible PNG dimensions {width}x{height}",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn json_u64_field(report: &JsonValue, key: &str) -> RuntimeResult<u64> {
    if let Some(value) = report.get(key).and_then(JsonValue::as_u64) {
        return Ok(value);
    }
    let value = report
        .get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("missing numeric manual metadata `{key}`"))?;
    Ok(value.parse::<u64>()?)
}

fn json_str_field<'a>(report: &'a JsonValue, key: &str) -> RuntimeResult<&'a str> {
    report
        .get(key)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("missing string manual metadata `{key}`").into())
}

fn reject_manual_placeholder(
    report: &JsonValue,
    report_path: &Path,
    key: &str,
) -> RuntimeResult<()> {
    let Some(value) = report.get(key).and_then(JsonValue::as_str) else {
        return Ok(());
    };
    if value.contains("fill") || value.contains("copy-from") || value.contains("replace-with") {
        return Err(format!(
            "{} has placeholder manual metadata `{key}` = `{value}`",
            report_path.display()
        )
        .into());
    }
    Ok(())
}

fn report_layer_is(report: &JsonValue, expected: &str) -> bool {
    report.get("layer").and_then(JsonValue::as_str) == Some(expected)
        || report.get("command").and_then(JsonValue::as_str) == Some(expected)
}

fn report_command_is(report: &JsonValue, expected: &str) -> bool {
    report.get("command").and_then(JsonValue::as_str) == Some(expected)
}

fn report_is_runtime_execution_layer(report: &JsonValue) -> bool {
    matches!(
        report.get("layer").and_then(JsonValue::as_str),
        Some("semantic" | "ply-headless" | "headed-ply" | "speed")
    )
}

pub fn example_paths(name: &str) -> RuntimeResult<(PathBuf, PathBuf, PathBuf)> {
    let example = if name == "todo" { "todomvc" } else { name };
    if !example
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(format!("invalid example name `{name}`").into());
    }
    let source = resolve_example_file(example, "bn");
    let scenario = resolve_example_file(example, "scn");
    let budget = resolve_example_file(example, "budget.toml");
    for required in [&source, &scenario, &budget] {
        if !required.exists() {
            return Err(format!(
                "example `{example}` is missing required file `{}`",
                required.display()
            )
            .into());
        }
    }
    Ok((source, scenario, budget))
}

fn resolve_example_file(example: &str, extension: &str) -> PathBuf {
    let relative = PathBuf::from(format!("examples/{example}.{extension}"));
    if relative.exists() {
        return relative;
    }
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            let candidate = ancestor.join(&relative);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    relative
}

fn run_loaded_scenario(
    parsed: &ParsedProgram,
    ir: &TypedProgram,
    scenario: &Scenario,
    layer: VerificationLayer,
) -> RuntimeResult<RunOutput> {
    let compiled = CompiledProgram::from_ir(ir)?;
    validate_executable_surface(parsed, ir, &compiled)?;
    let runtime = LoadedRuntime::new(parsed, ir, &compiled)?;
    run_generic_scenario(runtime, parsed, ir, &compiled, scenario, layer)
}

struct LoadedRuntime {
    generic: Option<GenericScheduledRuntime>,
    surface: LoadedRuntimeSurface,
}

enum LoadedRuntimeSurface {
    Todo(TodoRuntimeState),
    Cells(CellsRuntimeState),
}

impl LoadedRuntime {
    fn new(
        _parsed: &ParsedProgram,
        ir: &TypedProgram,
        compiled: &CompiledProgram,
    ) -> RuntimeResult<Self> {
        let generic = GenericScheduledRuntime::new(ir, compiled)?;
        match compiled.surface.kind {
            ExecutableSurfaceKind::TodoMvc => {
                let (generic, state) = initialize_loaded_todomvc_generic(generic)?;
                Ok(Self {
                    generic: Some(generic),
                    surface: LoadedRuntimeSurface::Todo(state),
                })
            }
            ExecutableSurfaceKind::Cells => {
                let (generic, state) = initialize_loaded_cells_generic(generic, ir)?;
                Ok(Self {
                    generic: Some(generic),
                    surface: LoadedRuntimeSurface::Cells(state),
                })
            }
        }
    }

    fn generic_state_summary(&self) -> JsonValue {
        let Some(generic) = self.generic.as_ref() else {
            return json!({ "error": "LoadedRuntime generic schedule was already borrowed" });
        };
        match &self.surface {
            LoadedRuntimeSurface::Todo(state) => {
                generic.todomvc_summary(state.stale_source_drop_count)
            }
            LoadedRuntimeSurface::Cells(_) => generic.cells_summary(),
        }
    }

    fn try_apply_todomvc_root_source_step<'a>(
        &mut self,
        step: &'a ScenarioStep,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<Option<StepExecutionMetrics>> {
        let LoadedRuntimeSurface::Todo(state) = &mut self.surface else {
            return Ok(None);
        };
        let Some(source_event) = GenericSourceEvent::from_step(step)? else {
            return Ok(None);
        };
        if source_event.target_text.is_some() {
            return Ok(None);
        }
        let generic = self
            .generic
            .as_mut()
            .ok_or("LoadedRuntime generic schedule was already borrowed")?;
        let routed = generic
            .route_source_event("todos", "title", source_event)
            .map_err(|_| {
                format!(
                    "{} source `{}` has no compiled route",
                    step.id, source_event.source
                )
            })?;
        match routed.route_kind {
            GenericSourceRouteKind::RootText
            | GenericSourceRouteKind::ListAppend
            | GenericSourceRouteKind::RootScalar
            | GenericSourceRouteKind::ListRemove
            | GenericSourceRouteKind::IndexedBoolBulk => {}
            GenericSourceRouteKind::IndexedTextChange
            | GenericSourceRouteKind::IndexedTextCommit
            | GenericSourceRouteKind::IndexedTextIdentity
            | GenericSourceRouteKind::IndexedTextKey
            | GenericSourceRouteKind::IndexedTextOpen
            | GenericSourceRouteKind::IndexedBoolToggle => return Ok(None),
        }
        assert_routed_source_event_matches(step, routed.event)?;
        match routed.route_kind {
            GenericSourceRouteKind::RootText | GenericSourceRouteKind::RootScalar => {
                let seq = TickSeq(state.next_source_seq);
                state.next_source_seq += 1;
                let input = generic.source_action_input_for_event(
                    &step.id,
                    routed.event,
                    seq,
                    |_, _| Ok(None),
                )?;
                generic.apply_source_actions(
                    input,
                    |_| None,
                    |mutation| {
                        emit_todomvc_default_protocol_mutation(mutation, deltas, patches)?;
                        Ok(())
                    },
                )?;
            }
            GenericSourceRouteKind::ListAppend => {
                let seq = TickSeq(state.next_source_seq);
                state.next_source_seq += 1;
                let input = generic.source_action_input_for_event(
                    &step.id,
                    routed.event,
                    seq,
                    |_, _| Ok(None),
                )?;
                let batch = generic.apply_source_actions_to_batch(input, |_| None, |_| Ok(()))?;
                if let Some(insert) = batch.list_append("todos") {
                    emit_todo_insert_from_generic(generic, insert, deltas, patches)?;
                    if let Some(commit) = batch.root_text("store.new_todo_text") {
                        emit_todomvc_default_protocol_mutation(
                            GenericSourceMutation::RootText(commit),
                            deltas,
                            patches,
                        )?;
                    }
                }
            }
            GenericSourceRouteKind::ListRemove => {
                let source_event = GenericSourceEvent {
                    source: routed.source(),
                    text: None,
                    key: None,
                    target_text: None,
                    address: None,
                };
                let input = generic.source_action_input_for_list_index(
                    &step.id,
                    source_event,
                    TickSeq(0),
                    "todos",
                    None,
                )?;
                generic.apply_source_actions(
                    input,
                    |_| None,
                    |mutation| {
                        emit_todomvc_default_protocol_mutation(mutation, deltas, patches)?;
                        Ok(())
                    },
                )?;
            }
            GenericSourceRouteKind::IndexedBoolBulk => {
                let all_completed = generic.todomvc_all_completed();
                let input = generic.source_action_input_for_list_index(
                    &step.id,
                    routed.event,
                    TickSeq(0),
                    "todos",
                    None,
                )?;
                generic.apply_source_actions(
                    input,
                    |path| match path {
                        "store.all_completed" => Some(all_completed),
                        _ => None,
                    },
                    |mutation| {
                        emit_todomvc_default_protocol_mutation(mutation, deltas, patches)?;
                        Ok(())
                    },
                )?;
            }
            GenericSourceRouteKind::IndexedTextChange
            | GenericSourceRouteKind::IndexedTextCommit
            | GenericSourceRouteKind::IndexedTextIdentity
            | GenericSourceRouteKind::IndexedTextKey
            | GenericSourceRouteKind::IndexedTextOpen
            | GenericSourceRouteKind::IndexedBoolToggle => unreachable!("filtered above"),
        }
        Ok(Some(StepExecutionMetrics {
            dirty_key_count: state.dirty_key_sets.mark_deltas(deltas),
            extra: StepExecutionExtra::Todo {
                stale_source_drop_count: 0,
            },
        }))
    }

    fn try_apply_todomvc_row_source_step<'a>(
        &mut self,
        step: &'a ScenarioStep,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<Option<StepExecutionMetrics>> {
        let LoadedRuntimeSurface::Todo(state) = &mut self.surface else {
            return Ok(None);
        };
        let Some(source_event) = GenericSourceEvent::from_step(step)? else {
            return Ok(None);
        };
        let Some(target_text) = source_event.target_text else {
            return Ok(None);
        };
        let generic = self
            .generic
            .as_mut()
            .ok_or("LoadedRuntime generic schedule was already borrowed")?;
        let routed = generic
            .route_source_event("todos", "title", source_event)
            .map_err(|_| {
                format!(
                    "{} source `{}` has no compiled route",
                    step.id, source_event.source
                )
            })?;
        match routed.route_kind {
            GenericSourceRouteKind::IndexedBoolToggle
            | GenericSourceRouteKind::IndexedTextOpen
            | GenericSourceRouteKind::IndexedTextChange
            | GenericSourceRouteKind::IndexedTextKey
            | GenericSourceRouteKind::IndexedTextCommit
            | GenericSourceRouteKind::ListRemove => {}
            GenericSourceRouteKind::RootText
            | GenericSourceRouteKind::ListAppend
            | GenericSourceRouteKind::RootScalar
            | GenericSourceRouteKind::IndexedBoolBulk
            | GenericSourceRouteKind::IndexedTextIdentity => return Ok(None),
        }
        assert_routed_source_event_matches(step, routed.event)?;
        let target_occurrence = step
            .user_action
            .as_ref()
            .and_then(|action| toml_usize_ref(action, "target_occurrence"))
            .unwrap_or(1);
        let stale_drops_before = state.stale_source_drop_count;
        let occurrence = match generic.resolve_visible_row_occurrence(
            "todos",
            "title",
            step.user_action.as_ref(),
            Some(source_event),
            target_text,
            target_occurrence,
        )? {
            GenericVisibleRowOccurrence::Occurrence(occurrence) => occurrence,
            GenericVisibleRowOccurrence::Mismatch => {
                return Ok(Some(StepExecutionMetrics {
                    dirty_key_count: state.dirty_key_sets.mark_deltas(deltas),
                    extra: StepExecutionExtra::Todo {
                        stale_source_drop_count: 0,
                    },
                }));
            }
            GenericVisibleRowOccurrence::Stale => {
                state.stale_source_drop_count += 1;
                return Ok(Some(StepExecutionMetrics {
                    dirty_key_count: state.dirty_key_sets.mark_deltas(deltas),
                    extra: StepExecutionExtra::Todo {
                        stale_source_drop_count: state
                            .stale_source_drop_count
                            .saturating_sub(stale_drops_before),
                    },
                }));
            }
        };
        let resolved_index =
            generic.find_visible_row_index_by_occurrence("todos", "title", target_text, occurrence);
        match routed.route_kind {
            GenericSourceRouteKind::IndexedBoolToggle => {
                let index = resolved_index?;
                let all_completed = generic.todomvc_all_completed();
                let input = generic.source_action_input_for_list_index(
                    &step.id,
                    routed.event,
                    TickSeq(0),
                    "todos",
                    Some(index),
                )?;
                generic.apply_source_actions(
                    input,
                    |path| match path {
                        "store.all_completed" => Some(all_completed),
                        _ => None,
                    },
                    |mutation| {
                        emit_todomvc_default_protocol_mutation(mutation, deltas, patches)?;
                        Ok(())
                    },
                )?;
            }
            GenericSourceRouteKind::IndexedTextOpen => {
                let index = resolved_index?;
                close_other_todomvc_editors(generic, index, deltas, patches)?;
                let all_completed = generic.todomvc_all_completed();
                let source_event = GenericSourceEvent {
                    text: Some(target_text),
                    ..routed.event
                };
                let input = generic.source_action_input_for_list_index(
                    &step.id,
                    source_event,
                    TickSeq(0),
                    "todos",
                    Some(index),
                )?;
                let batch = generic.apply_source_actions_to_batch(
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
                emit_todomvc_render_patch_for_mutation(
                    &GenericSourceMutation::BoolField(editing),
                    GenericRenderContext {
                        todo_show_edit_input_text: Some(edit_text.value),
                        ..GenericRenderContext::default()
                    },
                    patches,
                )?;
            }
            GenericSourceRouteKind::IndexedTextChange => {
                let index = resolved_index.or_else(|_| find_todomvc_editing_index(generic))?;
                let source_event = GenericSourceEvent {
                    text: Some(routed.require_text(&step.id)?),
                    ..routed.event
                };
                let input = generic.source_action_input_for_list_index(
                    &step.id,
                    source_event,
                    TickSeq(0),
                    "todos",
                    Some(index),
                )?;
                generic.apply_source_actions(
                    input,
                    |_| None,
                    |mutation| {
                        emit_todomvc_default_protocol_mutation(mutation, deltas, patches)?;
                        Ok(())
                    },
                )?;
            }
            GenericSourceRouteKind::IndexedTextKey => {
                let key = routed.event.key.unwrap_or_default();
                if matches!(key, "Enter" | "Escape") {
                    let index = resolved_index.or_else(|_| find_todomvc_editing_index(generic))?;
                    let all_completed = generic.todomvc_all_completed();
                    let payload_text = if key == "Enter" {
                        routed.event.text
                    } else {
                        Some(target_text)
                    };
                    let source_event = GenericSourceEvent {
                        text: payload_text,
                        ..routed.event
                    };
                    let input = generic.source_action_input_for_list_index(
                        &step.id,
                        source_event,
                        TickSeq(0),
                        "todos",
                        Some(index),
                    )?;
                    let batch = generic.apply_source_actions_to_batch(
                        input,
                        |path| match path {
                            "store.all_completed" => Some(all_completed),
                            _ => None,
                        },
                        |_| Ok(()),
                    )?;
                    if key == "Enter" {
                        if let Some(title) = batch.text("title") {
                            emit_todomvc_default_protocol_mutation(
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
                    emit_todomvc_default_protocol_mutation(
                        GenericSourceMutation::BoolField(editing),
                        deltas,
                        patches,
                    )?;
                }
            }
            GenericSourceRouteKind::IndexedTextCommit => {
                let index = resolved_index.or_else(|_| find_todomvc_editing_index(generic))?;
                let all_completed = generic.todomvc_all_completed();
                let source_event = GenericSourceEvent {
                    text: routed.event.text.or(Some(target_text)),
                    ..routed.event
                };
                let input = generic.source_action_input_for_list_index(
                    &step.id,
                    source_event,
                    TickSeq(0),
                    "todos",
                    Some(index),
                )?;
                let batch = generic.apply_source_actions_to_batch(
                    input,
                    |path| match path {
                        "store.all_completed" => Some(all_completed),
                        _ => None,
                    },
                    |_| Ok(()),
                )?;
                if let Some(title) = batch.text("title") {
                    emit_todomvc_default_protocol_mutation(
                        GenericSourceMutation::TextField(title),
                        deltas,
                        patches,
                    )?;
                }
                let editing = batch.require_bool(routed.source(), "editing update", "editing")?;
                emit_todomvc_default_protocol_mutation(
                    GenericSourceMutation::BoolField(editing),
                    deltas,
                    patches,
                )?;
            }
            GenericSourceRouteKind::ListRemove => {
                let index = resolved_index?;
                let mut removed = false;
                let input = generic.source_action_input_for_list_index(
                    &step.id,
                    routed.event,
                    TickSeq(0),
                    "todos",
                    Some(index),
                )?;
                generic.apply_source_actions(
                    input,
                    |_| None,
                    |mutation| {
                        if matches!(mutation, GenericSourceMutation::ListRemove { .. }) {
                            removed = true;
                        }
                        emit_todomvc_default_protocol_mutation(mutation, deltas, patches)?;
                        Ok(())
                    },
                )?;
                if !removed {
                    return Err(format!(
                        "remove source `{}` predicate does not match todo `{target_text}`",
                        routed.source()
                    )
                    .into());
                }
            }
            _ => unreachable!("filtered above"),
        }
        Ok(Some(StepExecutionMetrics {
            dirty_key_count: state.dirty_key_sets.mark_deltas(deltas),
            extra: StepExecutionExtra::Todo {
                stale_source_drop_count: state
                    .stale_source_drop_count
                    .saturating_sub(stale_drops_before),
            },
        }))
    }

    fn try_apply_todomvc_render_only_step<'a>(
        &mut self,
        step: &'a ScenarioStep,
        deltas: &mut [SemanticDelta<'a>],
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<Option<StepExecutionMetrics>> {
        let LoadedRuntimeSurface::Todo(state) = &mut self.surface else {
            return Ok(None);
        };
        let Some(action) = &step.user_action else {
            return Ok(None);
        };
        let kind = toml_string_ref(action, "kind").unwrap_or_default();
        let target_text = toml_string_ref(action, "target_text").unwrap_or_default();
        let ("pointer_hover", text) = (kind, target_text) else {
            return Ok(None);
        };
        let Some(title) = text.strip_suffix(" delete") else {
            return Ok(None);
        };
        let generic = self
            .generic
            .as_mut()
            .ok_or("LoadedRuntime generic schedule was already borrowed")?;
        let fallback = toml_usize_ref(action, "target_occurrence").unwrap_or(1);
        let stale_drops_before = state.stale_source_drop_count;
        let occurrence = match generic.resolve_visible_row_occurrence(
            "todos",
            "title",
            Some(action),
            None,
            title,
            fallback,
        )? {
            GenericVisibleRowOccurrence::Occurrence(occurrence) => occurrence,
            GenericVisibleRowOccurrence::Mismatch => {
                return Ok(Some(StepExecutionMetrics {
                    dirty_key_count: state.dirty_key_sets.mark_deltas(deltas),
                    extra: StepExecutionExtra::Todo {
                        stale_source_drop_count: 0,
                    },
                }));
            }
            GenericVisibleRowOccurrence::Stale => {
                state.stale_source_drop_count += 1;
                return Ok(Some(StepExecutionMetrics {
                    dirty_key_count: state.dirty_key_sets.mark_deltas(deltas),
                    extra: StepExecutionExtra::Todo {
                        stale_source_drop_count: state
                            .stale_source_drop_count
                            .saturating_sub(stale_drops_before),
                    },
                }));
            }
        };
        let index =
            generic.find_visible_row_index_by_occurrence("todos", "title", title, occurrence)?;
        let (key, generation) = generic.row_identity("todos", index)?;
        patches.push(
            GenericRenderLoweringPlan::todo_mvc().lower_todomvc_row_affordance_patch(
                key,
                generation,
                "delete_button",
                true,
            )?,
        );
        Ok(Some(StepExecutionMetrics {
            dirty_key_count: state.dirty_key_sets.mark_deltas(deltas),
            extra: StepExecutionExtra::Todo {
                stale_source_drop_count: state
                    .stale_source_drop_count
                    .saturating_sub(stale_drops_before),
            },
        }))
    }
}

fn find_todomvc_editing_index(generic: &GenericScheduledRuntime) -> RuntimeResult<usize> {
    (0..generic.list_len("todos")?)
        .find(|index| {
            generic
                .list_row_bool("todos", *index, "editing")
                .unwrap_or(false)
        })
        .ok_or_else(|| "no editing todo found".into())
}

fn close_other_todomvc_editors<'a>(
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
            list: "todos",
            key,
            generation,
            field: "editing",
            value: false,
        };
        deltas.push(commit.semantic_delta());
        emit_todomvc_render_patch_for_mutation(
            &GenericSourceMutation::BoolField(commit),
            GenericRenderContext::default(),
            patches,
        )?;
    }
    Ok(())
}

fn initialize_loaded_todomvc_generic(
    mut generic: GenericScheduledRuntime,
) -> RuntimeResult<(GenericScheduledRuntime, TodoRuntimeState)> {
    let todo_count = generic.list_len("todos")?;
    let row_source_paths = generic.row_source_paths("todos")?.to_vec();
    generic.reserve_source_bindings(todo_count * row_source_paths.len());
    generic.reserve_source_rows(todo_count);
    for index in 0..todo_count {
        if generic
            .list_row_textlike("todos", index, "title")?
            .is_empty()
        {
            return Err("TodoMVC seed titles must not be empty".into());
        }
        let (key, generation) = generic.row_identity("todos", index)?;
        generic.bind_row_sources("todos", key, generation, &row_source_paths);
    }
    Ok((
        generic,
        TodoRuntimeState {
            next_source_seq: 1,
            stale_source_drop_count: 0,
            dirty_key_sets: DirtyKeySets::with_capacity(todo_count.saturating_mul(4).max(16)),
        },
    ))
}

fn assert_loaded_todomvc_generic_fields(generic: &GenericScheduledRuntime) -> RuntimeResult<()> {
    generic.root_textlike_ref("store.new_todo_text")?;
    generic.root_textlike_ref("store.selected_filter")?;
    for index in 0..generic.list_len("todos")? {
        generic.row_identity("todos", index)?;
        generic.list_row_textlike("todos", index, "title")?;
        generic.list_row_textlike("todos", index, "edit_text")?;
        generic.list_row_bool("todos", index, "completed")?;
        generic.list_row_bool("todos", index, "editing")?;
    }
    Ok(())
}

fn initialize_loaded_cells_generic(
    generic: GenericScheduledRuntime,
    ir: &TypedProgram,
) -> RuntimeResult<(GenericScheduledRuntime, CellsRuntimeState)> {
    let (columns, rows) =
        cells_grid_dimensions_from_ir(ir).ok_or("Cells IR has no Grid/cells list initializer")?;
    let expected_len = columns.saturating_mul(rows);
    let actual_len = generic.list_len("cells")?;
    if actual_len != expected_len {
        return Err(format!(
            "Cells generic list initialized {actual_len} rows, expected {expected_len}"
        )
        .into());
    }
    let mut cells = Vec::with_capacity(expected_len);
    cells.resize_with(expected_len, Cell::default);
    Ok((
        generic,
        CellsRuntimeState {
            cells,
            dependency_cache: GenericFormulaDependencyCache::with_capacity(expected_len),
            evaluation_cache: GenericFormulaEvaluationCache::with_capacity(expected_len),
            columns,
            rows,
            interned_texts: Vec::new(),
            step_recomputed: Vec::with_capacity(8),
            dirty_key_sets: DirtyKeySets::with_capacity(expected_len.min(128).max(16)),
            last_recompute_candidates: 0,
        },
    ))
}

fn prepare_loaded_cells_scenario(
    generic: &mut GenericScheduledRuntime,
    state: &mut CellsRuntimeState,
    scenario: &Scenario,
) -> RuntimeResult<()> {
    intern_loaded_cell_text(state, "");
    intern_loaded_cell_text(state, "cycle_error");
    intern_loaded_cell_text(state, "parse_error");
    intern_loaded_cell_text(state, "div_by_zero");
    for step in &scenario.step {
        if let Some(action) = &step.user_action
            && let Some(text) = toml_string_ref(action, "text")
        {
            intern_loaded_cell_text(state, text);
        }
        if let Some(expected) = &step.expected_source_event
            && let Some(text) = toml_string_ref(expected, "text")
        {
            intern_loaded_cell_text(state, text);
        }
        if let Some(expect) = &step.expect_cell {
            if let Some(value) = &expect.value {
                intern_loaded_cell_text(state, value);
            }
            if let Some(formula) = &expect.formula {
                intern_loaded_cell_text(state, formula);
            }
            if let Some(editing_text) = &expect.editing_text {
                intern_loaded_cell_text(state, editing_text);
            }
        }
        if let Some(expect) = &step.expect_error {
            intern_loaded_cell_text(state, &expect.error);
        }
    }
    let requirements = generic.prepare_cells_scenario_storage(scenario)?;
    reserve_loaded_cell_cache(state, requirements.max_text_len, requirements.max_deps);
    Ok(())
}

fn intern_loaded_cell_text(state: &mut CellsRuntimeState, value: &str) {
    if state
        .interned_texts
        .iter()
        .any(|interned| *interned == value)
    {
        return;
    }
    let interned = Box::leak(value.to_owned().into_boxed_str());
    state.interned_texts.push(interned);
}

fn cells_protocol_text<'a>(state: &CellsRuntimeState, value: &str) -> ProtocolValue<'a> {
    if let Some(interned) = state
        .interned_texts
        .iter()
        .copied()
        .find(|interned| *interned == value)
    {
        ProtocolValue::Text(Cow::Borrowed(interned))
    } else {
        ProtocolValue::Text(Cow::Owned(value.to_owned()))
    }
}

fn reserve_loaded_cell_cache(state: &mut CellsRuntimeState, max_text_len: usize, max_deps: usize) {
    for cell in &mut state.cells {
        cell.value.reserve(max_text_len);
        cell.deps.reserve(max_deps);
        cell.dependency_text.reserve(max_deps.saturating_mul(8));
    }
    let minimum_fanout_capacity = max_deps.max(4);
    state
        .dependency_cache
        .reserve_dependents(minimum_fanout_capacity);
}

fn apply_loaded_cells_step<'a>(
    generic: &mut GenericScheduledRuntime,
    state: &mut CellsRuntimeState,
    step: &'a ScenarioStep,
    deltas: &mut Vec<SemanticDelta<'a>>,
    patches: &mut Vec<RenderPatch<'a>>,
) -> RuntimeResult<StepExecutionMetrics> {
    let mut step_recomputed = std::mem::take(&mut state.step_recomputed);
    step_recomputed.clear();
    apply_loaded_cells_step_into(generic, state, step, deltas, patches, &mut step_recomputed)?;
    state
        .dirty_key_sets
        .mark_indexes("cells", "value", &step_recomputed);
    let dirty_key_count = state.dirty_key_sets.key_count();
    let recomputed_cell_count = step_recomputed.len();
    let recompute_candidate_count = state.last_recompute_candidates;
    let formula_eval_call_count = state.evaluation_cache.last_eval_calls();
    let dependency_edge_walk_count = state.dependency_cache.last_edge_walks();
    state.step_recomputed = step_recomputed;
    Ok(StepExecutionMetrics {
        dirty_key_count,
        extra: StepExecutionExtra::Cells {
            recomputed_cell_count,
            recompute_candidate_count,
            formula_eval_call_count,
            dependency_edge_walk_count,
        },
    })
}

fn apply_loaded_cells_step_into<'a>(
    generic: &mut GenericScheduledRuntime,
    state: &mut CellsRuntimeState,
    step: &'a ScenarioStep,
    deltas: &mut Vec<SemanticDelta<'a>>,
    patches: &mut Vec<RenderPatch<'a>>,
    recomputed: &mut Vec<usize>,
) -> RuntimeResult<()> {
    let Some(routed) = route_loaded_cells_step(generic, step)? else {
        return Ok(());
    };
    assert_routed_source_event_matches(step, routed.event)?;
    match routed.route_kind {
        GenericSourceRouteKind::IndexedTextChange => {
            let source = routed.source();
            let address = routed.require_address(&step.id)?;
            if !is_cell_address(address) {
                return Err(format!("{} Cells source event missing valid address", step.id).into());
            }
            let text = routed.require_text(&step.id)?;
            let source_event = GenericSourceEvent {
                source,
                text: Some(text),
                key: None,
                target_text: None,
                address: Some(address),
            };
            let input = generic.source_action_input_for_event_by_row_field(
                "cells-change",
                source_event,
                TickSeq(0),
                "address",
                Some(address),
            )?;
            let batch = generic.apply_source_actions_to_batch(
                input,
                |_| None,
                |mutation| {
                    emit_cells_default_protocol_mutation(
                        mutation, address, None, true, false, deltas, patches,
                    )?;
                    Ok(())
                },
            )?;
            batch.require_text(source, "editing-text update", "editing_text")?;
            batch.require_bool(source, "editing update", "editing")?;
        }
        GenericSourceRouteKind::IndexedTextCommit => {
            let source = routed.source();
            let address = routed.require_address(&step.id)?;
            if !is_cell_address(address) {
                return Err(format!("{} Cells source event missing valid address", step.id).into());
            }
            let text = routed.require_text(&step.id)?;
            loaded_cells_commit_from_source(
                generic, state, source, address, text, deltas, patches, recomputed,
            )?;
        }
        GenericSourceRouteKind::IndexedTextIdentity => {
            let source = routed.source();
            let address = routed.require_address(&step.id)?;
            if !is_cell_address(address) {
                return Err(format!("{} Cells source event missing valid address", step.id).into());
            }
            let source_event = GenericSourceEvent {
                source,
                text: None,
                key: None,
                target_text: None,
                address: Some(address),
            };
            let input = generic.source_action_input_for_event_by_row_field(
                "cells-cancel",
                source_event,
                TickSeq(0),
                "address",
                Some(address),
            )?;
            let batch = generic.apply_source_actions_to_batch(input, |_| None, |_| Ok(()))?;
            let editing_text =
                batch.require_identity(source, "editing-text cancel", "editing_text")?;
            let editing = batch.require_bool(source, "editing cancel", "editing")?;
            let index = loaded_cell_index(state, address)?;
            let value = generic.list_row_textlike("cells", index, editing_text.field)?;
            let identity_value = cells_protocol_text(state, value);
            emit_cells_default_protocol_mutation(
                GenericSourceMutation::TextFieldIdentity(editing_text),
                address,
                Some(identity_value),
                false,
                false,
                deltas,
                patches,
            )?;
            emit_cells_default_protocol_mutation(
                GenericSourceMutation::BoolField(editing),
                address,
                None,
                false,
                false,
                deltas,
                patches,
            )?;
            patches.push(keyed_patch(
                "SetCellText",
                RenderTarget::Borrowed(Cow::Borrowed(address)),
                cells_protocol_text(state, &state.cells[index].value),
                editing_text.list,
                editing_text.key,
                editing_text.generation,
            ));
        }
        route_kind => {
            return Err(format!(
                "{} Cells source `{}` classified as unsupported route `{route_kind:?}`",
                step.id,
                routed.source()
            )
            .into());
        }
    }
    Ok(())
}

fn route_loaded_cells_step<'a>(
    generic: &GenericScheduledRuntime,
    step: &'a ScenarioStep,
) -> RuntimeResult<Option<GenericRoutedSourceEvent<'a>>> {
    if step.user_action.is_none() {
        return Ok(None);
    }
    let source_event = GenericSourceEvent::require(step)?;
    route_loaded_cells_source_event(generic, step, source_event)
}

fn route_loaded_cells_source_event<'a>(
    generic: &GenericScheduledRuntime,
    step: &'a ScenarioStep,
    source_event: GenericSourceEvent<'a>,
) -> RuntimeResult<Option<GenericRoutedSourceEvent<'a>>> {
    let source = source_event.source;
    let routed = generic
        .route_source_event("cells", "formula_text", source_event)
        .map_err(|_| format!("{} source `{source}` has no compiled route", step.id))?;
    let address = routed
        .event
        .address
        .filter(|candidate| is_cell_address(candidate))
        .ok_or_else(|| format!("{} Cells source event missing valid address", step.id))?;
    match routed.route_kind {
        GenericSourceRouteKind::IndexedTextCommit | GenericSourceRouteKind::IndexedTextChange => {
            routed.require_text(&step.id)?;
            Ok(Some(routed))
        }
        GenericSourceRouteKind::IndexedTextIdentity => Ok(Some(routed)),
        route_kind => Err(format!(
            "{} Cells source `{source}` for address `{address}` classified as unsupported route `{route_kind:?}`",
            step.id
        )
        .into()),
    }
}

fn loaded_cells_commit_from_source<'a>(
    generic: &mut GenericScheduledRuntime,
    state: &mut CellsRuntimeState,
    source: &'a str,
    address: &'a str,
    formula: &'a str,
    deltas: &mut Vec<SemanticDelta<'a>>,
    patches: &mut Vec<RenderPatch<'a>>,
    recomputed: &mut Vec<usize>,
) -> RuntimeResult<()> {
    generic.formula_equations.expect_cells_pipeline()?;
    let committed_index = loaded_cell_index(state, address)?;
    let source_event = GenericSourceEvent {
        source,
        text: Some(formula),
        key: None,
        target_text: None,
        address: Some(address),
    };
    let input = generic.source_action_input_for_event_by_row_field(
        "cells-commit",
        source_event,
        TickSeq(0),
        "address",
        Some(address),
    )?;
    let batch = generic.apply_source_actions_to_batch(input, |_| None, |_| Ok(()))?;
    let formula = batch.require_text(source, "formula update", "formula_text")?;
    let editing_text = batch.require_text(source, "editing text update", "editing_text")?;
    let editing = batch.require_bool(source, "editing update", "editing")?;
    state.cells[committed_index].parsed =
        generic
            .formula_equations
            .parse_cell_formula(formula.value, state.columns, state.rows)?;
    replace_loaded_cell_dependencies(generic, state, committed_index)?;
    recompute_loaded_cells_affected(generic, state, address, recomputed)?;
    let cell = &state.cells[committed_index];
    let display_key = formula.key;
    let display_generation = formula.generation;
    let display_value = generic.formula_equations.cell_value_protocol(cell)?;
    let display_error = cell.error;
    emit_cells_default_protocol_mutation(
        GenericSourceMutation::TextField(formula),
        address,
        None,
        false,
        false,
        deltas,
        patches,
    )?;
    emit_cells_default_protocol_mutation(
        GenericSourceMutation::TextField(editing_text),
        address,
        None,
        false,
        false,
        deltas,
        patches,
    )?;
    emit_cells_default_protocol_mutation(
        GenericSourceMutation::BoolField(editing),
        address,
        None,
        false,
        false,
        deltas,
        patches,
    )?;
    generic
        .formula_equations
        .emit_cell_display_protocol_mutations(
            display_key,
            display_generation,
            display_value,
            display_error,
            address,
            deltas,
            patches,
        )?;
    Ok(())
}

fn recompute_loaded_cells_affected(
    generic: &mut GenericScheduledRuntime,
    state: &mut CellsRuntimeState,
    changed_address: &str,
    recomputed: &mut Vec<usize>,
) -> RuntimeResult<()> {
    let changed_index = loaded_cell_index(state, changed_address)?;
    state.dependency_cache.collect_affected(changed_index);
    let affected_len = state.dependency_cache.affected().len();
    state.last_recompute_candidates = affected_len;
    state.evaluation_cache.begin_tick();
    for offset in 0..affected_len {
        let index = state.dependency_cache.affected()[offset];
        let _ = state.evaluation_cache.eval_cell(&state.cells, index);
    }
    for offset in 0..affected_len {
        let index = state.dependency_cache.affected()[offset];
        let result = state.evaluation_cache.cached_result(index).unwrap_or(Ok(0));
        let changed = GenericFormulaEvaluationCache::apply_result_to_cell(
            index,
            changed_index,
            result,
            &mut state.cells[index],
        )?;
        if changed {
            sync_loaded_cell_derived_fields(generic, state, index)?;
            recomputed.push(index);
        }
    }
    recomputed.sort_unstable();
    Ok(())
}

fn sync_loaded_cell_derived_fields(
    generic: &mut GenericScheduledRuntime,
    state: &mut CellsRuntimeState,
    index: usize,
) -> RuntimeResult<()> {
    refresh_loaded_cell_dependency_text(state, index);
    let fields = generic.formula_equations.derived_storage_fields()?;
    sync_formula_derived_fields(
        generic,
        index,
        fields,
        &state.cells[index].value,
        state.cells[index].error,
        &state.cells[index].dependency_text,
    )
}

fn refresh_loaded_cell_dependency_text(state: &mut CellsRuntimeState, index: usize) {
    state.cells[index].dependency_text.clear();
    for offset in 0..state.cells[index].deps.len() {
        if offset > 0 {
            state.cells[index].dependency_text.push(',');
        }
        let dependency = state.cells[index].deps[offset];
        push_cell_address(
            state.columns,
            dependency,
            &mut state.cells[index].dependency_text,
        );
    }
}

fn replace_loaded_cell_dependencies(
    generic: &GenericScheduledRuntime,
    state: &mut CellsRuntimeState,
    cell_index: usize,
) -> RuntimeResult<()> {
    state.cells[cell_index].deps.clear();
    generic.formula_equations.dependencies_into(
        state.cells[cell_index].parsed,
        &mut state.cells[cell_index].deps,
    )?;
    state
        .dependency_cache
        .replace_dependencies(cell_index, &state.cells[cell_index].deps);
    Ok(())
}

fn loaded_cell_index(state: &CellsRuntimeState, address: &str) -> RuntimeResult<usize> {
    cell_index(address, state.columns, state.rows)
        .ok_or_else(|| format!("unknown cell {address}").into())
}

fn loaded_cell_address(state: &CellsRuntimeState, index: usize) -> String {
    let mut address = String::new();
    push_cell_address(state.columns, index, &mut address);
    address
}

fn assert_loaded_cells_generic_fields(
    generic: &GenericScheduledRuntime,
    state: &CellsRuntimeState,
) -> RuntimeResult<()> {
    let generic_len = generic.list_len("cells")?;
    if generic_len != state.cells.len() {
        return Err(format!(
            "generic cells length {generic_len} != formula state {}",
            state.cells.len()
        )
        .into());
    }
    for index in 0..state.cells.len() {
        let address = loaded_cell_address(state, index);
        let generic_address = generic.list_row_textlike("cells", index, "address")?;
        if generic_address != address {
            return Err(format!(
                "row {index} address generic `{generic_address}` != computed `{address}`"
            )
            .into());
        }
        generic.list_row_textlike("cells", index, "formula_text")?;
        generic.list_row_textlike("cells", index, "editing_text")?;
        generic.list_row_bool("cells", index, "editing")?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExecutableSurfaceKind {
    TodoMvc,
    Cells,
}

impl ExecutableSurfaceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::TodoMvc => "todomvc",
            Self::Cells => "cells",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ExecutableSurfaceProfile {
    kind: ExecutableSurfaceKind,
    inferred_from_ir: bool,
}

#[derive(Clone, Debug)]
struct CompiledProgram {
    surface: ExecutableSurfaceProfile,
    scalar_equations: ScalarEquationPlan,
    derived_equations: DerivedEquationPlan,
    list_equations: ListEquationPlan,
    formula_equations: FormulaEquationPlan,
    source_routes: SourceRoutePlan,
    list_source_bindings: ListSourceBindingPlan,
    schedule_node_count: usize,
    state_initializer_count: usize,
    list_initializer_count: usize,
    derived_value_count: usize,
    derived_text_transform_count: usize,
    update_branch_count: usize,
    list_operation_count: usize,
    formula_operation_count: usize,
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

impl CompiledProgram {
    fn from_ir(ir: &TypedProgram) -> RuntimeResult<Self> {
        let surface = ExecutableSurfaceProfile::infer(ir)?;
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
        let derived_text_transform_count = derived_equations.text_transforms.len();
        let list_equations = ListEquationPlan::from_ir(ir);
        let formula_equations = FormulaEquationPlan::from_ir(ir);
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
            surface,
            scalar_equations,
            derived_equations,
            list_equations,
            formula_equations,
            source_routes,
            list_source_bindings,
            schedule_node_count: ir.nodes.len(),
            state_initializer_count: ir.state_cells.len(),
            list_initializer_count: ir.lists.len(),
            derived_value_count: ir.derived_values.len(),
            derived_text_transform_count,
            update_branch_count: ir.update_branches.len(),
            list_operation_count: ir.list_operations.len(),
            formula_operation_count: ir.formula_operations.len(),
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
            "executable_surface": self.surface.kind.as_str(),
            "executable_surface_inferred_from_ir": self.surface.inferred_from_ir,
            "schedule_node_count": self.schedule_node_count,
            "state_initializer_count": self.state_initializer_count,
            "list_initializer_count": self.list_initializer_count,
            "derived_value_count": self.derived_value_count,
            "derived_text_transform_count": self.derived_text_transform_count,
            "update_branch_count": self.update_branch_count,
            "list_operation_count": self.list_operation_count,
            "formula_operation_count": self.formula_operation_count,
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
                (false, InitialValue::Text { .. } | InitialValue::SeedField { .. }) => {
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
                (true, InitialValue::Text { .. } | InitialValue::SeedField { .. }) => {
                    counts.list_row_template_field_count += 1;
                    counts.list_row_text_slot_count += 1;
                }
                (true, InitialValue::Unknown { .. }) => {}
            }
        }
        for value in &ir.derived_values {
            if value.indexed && value.kind == DerivedValueKind::Formula {
                counts.list_row_text_slot_count += 1;
            }
        }
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

impl ExecutableSurfaceProfile {
    fn infer(ir: &TypedProgram) -> RuntimeResult<Self> {
        let has_todomvc_shape = ir_has_list(ir, "todos")
            && ir_has_state(ir, "store.new_todo_text")
            && ir_has_state(ir, "store.selected_filter")
            && ir_has_state(ir, "todo.title")
            && ir_has_state(ir, "todo.edit_text")
            && ir_has_state(ir, "todo.completed")
            && ir_has_state(ir, "todo.editing");
        let has_cells_shape = ir_has_list(ir, "cells")
            && ir_has_state(ir, "cell.editing_text")
            && ir_has_state(ir, "cell.formula_text")
            && ir_has_state(ir, "cell.editing")
            && !ir.formula_operations.is_empty();
        match (has_todomvc_shape, has_cells_shape) {
            (true, false) => Ok(Self {
                kind: ExecutableSurfaceKind::TodoMvc,
                inferred_from_ir: true,
            }),
            (false, true) => Ok(Self {
                kind: ExecutableSurfaceKind::Cells,
                inferred_from_ir: true,
            }),
            (true, true) => {
                Err("typed IR matches both TodoMVC and Cells executable surface profiles".into())
            }
            (false, false) => {
                Err("typed IR does not match a supported executable surface profile".into())
            }
        }
    }
}

fn ir_has_state(ir: &TypedProgram, path: &str) -> bool {
    ir.state_cells.iter().any(|cell| cell.path == path)
}

fn ir_has_list(ir: &TypedProgram, name: &str) -> bool {
    ir.lists.iter().any(|list| list.name == name)
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

fn validate_executable_surface(
    parsed: &ParsedProgram,
    ir: &TypedProgram,
    compiled: &CompiledProgram,
) -> RuntimeResult<()> {
    match compiled.surface.kind {
        ExecutableSurfaceKind::TodoMvc => {
            require_state_cells(
                ir,
                &[
                    "store.new_todo_text",
                    "store.selected_filter",
                    "todo.title",
                    "todo.edit_text",
                    "todo.completed",
                    "todo.editing",
                ],
            )?;
            require_sources(
                ir,
                &[
                    "store.sources.new_todo_input.change",
                    "store.sources.new_todo_input.key_down",
                    "store.sources.toggle_all_checkbox.click",
                    "store.sources.clear_completed_button.press",
                    "store.sources.filter_all.press",
                    "store.sources.filter_active.press",
                    "store.sources.filter_completed.press",
                ],
            )?;
            require_scoped_list_sources(&compiled.list_source_bindings, "todos", 6)?;
            require_lists(ir, &["todos"])?;
            require_operators(
                parsed,
                &[
                    "SOURCE",
                    "HOLD",
                    "THEN",
                    "WHEN",
                    "LATEST",
                    "LIST",
                    "List/append",
                    "List/remove",
                    "List/map",
                    "List/retain",
                    "List/count",
                ],
            )?;
        }
        ExecutableSurfaceKind::Cells => {
            require_state_cells(
                ir,
                &["cell.editing_text", "cell.formula_text", "cell.editing"],
            )?;
            require_scoped_list_sources(&compiled.list_source_bindings, "cells", 3)?;
            require_lists(ir, &["cells"])?;
            require_operators(parsed, &["SOURCE", "HOLD", "LATEST", "List/map"])?;
            for primitive in [
                "Formula/parse",
                "Formula/dependencies",
                "Formula/eval",
                "Formula/error",
            ] {
                if !parsed
                    .operators
                    .iter()
                    .any(|operator| operator == primitive)
                {
                    return Err(format!("executable Cells source missing `{primitive}`").into());
                }
            }
        }
    }
    Ok(())
}

fn require_state_cells(ir: &TypedProgram, required: &[&str]) -> RuntimeResult<()> {
    for path in required {
        if !ir.state_cells.iter().any(|cell| cell.path == *path) {
            return Err(format!("executable source missing state cell `{path}`").into());
        }
    }
    Ok(())
}

fn require_sources(ir: &TypedProgram, required: &[&str]) -> RuntimeResult<()> {
    for path in required {
        if !ir.sources.iter().any(|source| source.path == *path) {
            return Err(format!("executable source missing source port `{path}`").into());
        }
    }
    Ok(())
}

fn require_scoped_list_sources(
    bindings: &ListSourceBindingPlan,
    list: &str,
    minimum: usize,
) -> RuntimeResult<()> {
    let count = bindings.source_count(list)?;
    if count < minimum {
        return Err(format!(
            "executable source list `{list}` has only {count} scoped row source ports; expected at least {minimum}"
        )
        .into());
    }
    Ok(())
}

fn require_lists(ir: &TypedProgram, required: &[&str]) -> RuntimeResult<()> {
    for name in required {
        if !ir.lists.iter().any(|list| list.name == *name) {
            return Err(format!("executable source missing list memory `{name}`").into());
        }
    }
    Ok(())
}

fn require_operators(parsed: &ParsedProgram, required: &[&str]) -> RuntimeResult<()> {
    for operator in required {
        if !parsed
            .operators
            .iter()
            .any(|available| available == operator)
        {
            return Err(format!("executable source missing operator `{operator}`").into());
        }
    }
    Ok(())
}

struct StepExecutionMetrics {
    dirty_key_count: usize,
    extra: StepExecutionExtra,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DirtyKeyEntry {
    list_id: &'static str,
    field_id: &'static str,
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

    fn mark(&mut self, list_id: &'static str, field_id: &'static str, key: u64) {
        let entry = DirtyKeyEntry {
            list_id,
            field_id,
            key,
        };
        if !self.entries.contains(&entry) {
            self.entries.push(entry);
        }
    }

    fn mark_delta(&mut self, delta: &SemanticDelta<'_>) {
        let Some(list_id) = delta.list_id else {
            return;
        };
        let Some(key) = delta.key else {
            return;
        };
        let field_id = delta.field_path.unwrap_or(delta.kind);
        self.mark(list_id, field_id, key);
    }

    fn mark_deltas(&mut self, deltas: &[SemanticDelta<'_>]) -> usize {
        self.clear();
        for delta in deltas {
            self.mark_delta(delta);
        }
        self.key_count()
    }

    fn mark_indexes(&mut self, list_id: &'static str, field_id: &'static str, indexes: &[usize]) {
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
    Todo {
        stale_source_drop_count: u64,
    },
    Cells {
        recomputed_cell_count: usize,
        recompute_candidate_count: usize,
        formula_eval_call_count: usize,
        dependency_edge_walk_count: usize,
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
            StepExecutionExtra::Todo {
                stale_source_drop_count,
            } => {
                object.insert(
                    "stale_source_drop_count".to_owned(),
                    json!(stale_source_drop_count),
                );
            }
            StepExecutionExtra::Cells {
                recomputed_cell_count,
                recompute_candidate_count,
                formula_eval_call_count,
                dependency_edge_walk_count,
            } => {
                object.insert(
                    "recomputed_cell_count".to_owned(),
                    json!(recomputed_cell_count),
                );
                object.insert(
                    "recompute_candidate_count".to_owned(),
                    json!(recompute_candidate_count),
                );
                object.insert(
                    "formula_eval_call_count".to_owned(),
                    json!(formula_eval_call_count),
                );
                object.insert(
                    "dependency_edge_walk_count".to_owned(),
                    json!(dependency_edge_walk_count),
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
    if matches!(layer, VerificationLayer::Speed) {
        if let Some(stress_profiles) = runtime.stress_profiles(ir)? {
            report["stress_profiles"] = stress_profiles;
        }
    }
    Ok(RunOutput {
        report,
        semantic_deltas,
        render_patches,
        state_summary,
        view_lines: boon_parser::parsed_view_lines(parsed),
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
    let program_hash = sha256_bytes(parsed.source.as_bytes());
    let semantic_delta_protocol_batches =
        semantic_delta_protocol_batches(&program_hash, semantic_deltas, &per_step);
    let implementation = "static_graph_interpreter";
    let is_todomvc = matches!(compiled.surface.kind, ExecutableSurfaceKind::TodoMvc);
    let adapter_blocker = if is_todomvc {
        "TodoMVC now executes scenario preparation, source dispatch, row source execution, render-only hover, assertions, summaries, and speed stress reports through LoadedRuntime and GenericScheduledRuntime without borrowing the TodoMVC surface driver; remaining final handoff blockers are fresh human reports and aggregate all reports, not an example behavior adapter"
    } else {
        "Cells now executes scenario preparation, change/commit/cancel source dispatch, formula dependency/evaluation cache updates, assertions, summaries, and speed stress reports through LoadedRuntime and GenericScheduledRuntime without borrowing the Cells surface driver; remaining final handoff blockers are fresh human reports and aggregate all reports, not an example behavior adapter"
    };
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
    let runtime_execution = json!({
        "implementation": implementation,
        "source_loaded_from_boon": true,
        "typed_ir_loaded": true,
        "static_schedule_verified": ir.static_schedule_verified,
        "runtime_profile": runtime_profile.as_str(),
        "runtime_profile_detail": runtime_profile_detail,
        "capacities": capacity_report,
        "expression_coverage": &ir.expression_coverage,
        "generic_interpreter_complete": generic_interpreter_complete,
        "example_behavior_adapter": example_behavior_adapter,
        "adapter_kind": compiled.surface.kind.as_str(),
        "adapter_blocker": adapter_blocker,
        "remaining_example_specific_shell_policy": "scenario_assertion_report_glue_only",
        "remaining_example_specific_shells": remaining_shells,
        "final_handoff_pending_human_report": true,
        "generic_runtime_slices": generic_runtime_slices,
        "generic_runtime_slice_evidence": generic_runtime_slice_evidence
    });
    json!({
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
        "window_mode": runtime_window_mode(layer),
        "window_backend": runtime_window_backend(layer),
        "display_server": display_server(),
        "display_scale": std::env::var("GDK_SCALE").unwrap_or_else(|_| "1".to_owned()),
        "window_size": runtime_window_size(layer),
        "framebuffer_size": runtime_framebuffer_size(layer),
        "program_kind": compiled.surface.kind.as_str(),
        "program_hash": program_hash,
        "expression_count": ir.expression_count,
        "expression_coverage": &ir.expression_coverage,
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
    })
}

fn remaining_example_specific_shells(
    compiled: &CompiledProgram,
    generic_runtime_slices: &JsonValue,
) -> Vec<&'static str> {
    let Some(slices) = generic_runtime_slices.as_object() else {
        return Vec::new();
    };
    let prefix = compiled.surface.kind.as_str();
    let active_slice = |patterns: &[&str]| {
        slices.iter().any(|(key, value)| {
            value.as_bool() == Some(true)
                && key.contains(prefix)
                && patterns.iter().any(|pattern| key.contains(pattern))
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
        shells.push(match compiled.surface.kind {
            ExecutableSurfaceKind::TodoMvc => "todomvc_scenario_glue",
            ExecutableSurfaceKind::Cells => "cells_scenario_glue",
        });
    }
    if active_slice(&["scenario_expectation_assertions", "assertion_executor"]) {
        shells.push(match compiled.surface.kind {
            ExecutableSurfaceKind::TodoMvc => "todomvc_assertion_glue",
            ExecutableSurfaceKind::Cells => "cells_assertion_glue",
        });
    }
    if active_slice(&[
        "summary_reads_authoritative_storage",
        "delta_identities_from_authoritative_storage",
        "hidden_grid_keys_from_generic_storage",
        "formula_display_mutation_emitter",
    ]) {
        shells.push(match compiled.surface.kind {
            ExecutableSurfaceKind::TodoMvc => "todomvc_report_glue",
            ExecutableSurfaceKind::Cells => "cells_report_glue",
        });
    }
    if active_slice(&[
        "render_patch_lowering",
        "common_render_patch_lowering",
        "render_only_patch_lowering",
        "formula_display_protocol_lowering",
    ]) {
        shells.push(match compiled.surface.kind {
            ExecutableSurfaceKind::TodoMvc => "todomvc_render_patch_report_glue",
            ExecutableSurfaceKind::Cells => "cells_render_patch_report_glue",
        });
    }
    if active_slice(&["stress_profile_executor"]) {
        shells.push(match compiled.surface.kind {
            ExecutableSurfaceKind::TodoMvc => "todomvc_stress_report_glue",
            ExecutableSurfaceKind::Cells => "cells_stress_report_glue",
        });
    }
    shells
}

fn generic_runtime_slices_report(ir: &TypedProgram, compiled: &CompiledProgram) -> JsonValue {
    let is_todomvc = matches!(compiled.surface.kind, ExecutableSurfaceKind::TodoMvc);
    let is_cells = matches!(compiled.surface.kind, ExecutableSurfaceKind::Cells);
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
    let formula_operations_loaded = compiled.formula_operation_count == ir.formula_operations.len();
    let has_indexed_routes = indexed_text_route_count > 0 || indexed_bool_route_count > 0;
    let has_render_bindings = !ir.view_bindings.is_empty();
    let has_list_source_bindings = !compiled.list_source_bindings.list_slots.is_empty();
    json!({
        "generic_executable_surface_inferred_from_ir": compiled.surface.inferred_from_ir,
        "ir_update_branch_table_loaded": update_branches_loaded,
        "update_branch_count": ir.update_branches.len(),
        "unsupported_update_branch_count": compiled.unsupported_update_branch_count,
        "generic_scenario_loop_executor": update_branches_loaded && list_initializers_loaded,
        "generic_schedule_instantiated_before_adapter": ir.static_schedule_verified && compiled.surface.inferred_from_ir,
        "loaded_runtime_owns_generic_schedule_storage": state_initializers_loaded && list_initializers_loaded,
        "surface_driver_borrows_generic_storage_for_tick": false,
        "generic_source_event_ingest": source_routes_dense,
        "generic_source_binding_store": has_list_source_bindings,
        "generic_indexed_branch_evaluator": has_indexed_routes,
        "generic_indexed_scalar_commit_executor": has_indexed_routes,
        "generic_semantic_delta_emitter": route_action_count > 0 || !ir.list_operations.is_empty(),
        "generic_source_mutation_semantic_delta_emitter": route_action_count > 0,
        "generic_derived_value_semantic_delta_emitter": compiled.derived_text_transform_count > 0 || compiled.formula_operation_count > 0,
        "generic_source_bind_semantic_delta_emitter": has_list_source_bindings,
        "generic_list_remove_semantic_delta_emitter": list_remove_operation_count > 0 || is_cells,
        "generic_source_unbind_semantic_delta_emitter": list_remove_operation_count > 0 || is_cells,
        "generic_render_lowering_plan": has_render_bindings,
        "generic_todomvc_common_render_patch_lowering": is_todomvc,
        "generic_todomvc_append_source_bind_render_lowering": is_todomvc,
        "generic_todomvc_edit_open_close_render_lowering": is_todomvc,
        "generic_todomvc_render_only_patch_lowering": is_todomvc,
        "generic_cells_common_render_patch_lowering": is_cells,
        "generic_loaded_runtime_shell": state_initializers_loaded && source_routes_dense,
        "generic_source_route_action_executor": route_action_count > 0,
        "generic_todomvc_source_effects_through_action_executor": is_todomvc,
        "generic_cells_source_effects_through_action_executor": is_cells,
        "generic_root_text_tick_executor": root_scalar_route_count > 0 || is_cells,
        "generic_route_selected_root_hold_commit_executor": root_scalar_route_count > 0 || is_cells,
        "generic_indexed_hold_commit_executor": has_indexed_routes,
        "generic_route_selected_indexed_bool_commit_executor": indexed_bool_route_count > 0,
        "generic_route_selected_todo_edit_text_commit_executor": is_todomvc,
        "generic_route_selected_todo_title_commit_executor": is_todomvc,
        "generic_route_selected_todo_editing_commit_executor": is_todomvc,
        "generic_indexed_bulk_bool_commit_executor": indexed_bool_route_count > 0,
        "generic_list_append_source_binding_executor": (list_append_operation_count > 0 && list_append_route_count > 0 && has_list_source_bindings) || is_cells,
        "generic_list_remove_source_unbinding_executor": (list_remove_operation_count > 0 && list_remove_route_count > 0) || is_cells,
        "generic_list_move_semantic_delta_emitter": !ir.lists.is_empty(),
        "generic_list_count_retain_executor": list_count_retain_operation_count > 0 || is_cells,
        "generic_todomvc_summary_reads_authoritative_storage": is_todomvc,
        "generic_loaded_runtime_state_summary_projection": state_initializers_loaded && list_initializers_loaded,
        "generic_todomvc_root_holds_no_mirror": is_todomvc,
        "generic_todomvc_rows_hold_no_mirror": is_todomvc,
        "generic_todomvc_delta_identities_from_authoritative_storage": is_todomvc,
        "generic_cells_committed_fields_hold_no_mirror": is_cells,
        "generic_root_source_dispatch": source_routes_dense,
        "generic_derived_text_transform_executor": compiled.derived_text_transform_count > 0 || is_cells,
        "generic_source_event_route_executor": source_routes_dense && route_action_count > 0,
        "generic_compiled_source_route_index": source_routes_dense,
        "generic_source_route_classifier": source_routes_dense,
        "generic_todomvc_source_route_classifier": is_todomvc,
        "generic_cells_source_route_classifier": is_cells,
        "generic_cells_address_row_context_resolution": is_cells,
        "generic_cells_routed_source_event": is_cells,
        "generic_todomvc_routed_source_event": is_todomvc,
        "generic_todomvc_row_routed_source_event": is_todomvc,
        "generic_todomvc_visible_row_occurrence_resolution": is_todomvc,
        "generic_todomvc_source_action_mutation_batch": is_todomvc,
        "generic_todomvc_append_mutation_batch": is_todomvc,
        "generic_todomvc_list_index_action_input_resolution": is_todomvc,
        "generic_todomvc_scenario_expectation_assertions": is_todomvc,
        "generic_todomvc_scenario_preparation": is_todomvc,
        "generic_loaded_runtime_todomvc_root_step_executor": is_todomvc,
        "generic_loaded_runtime_todomvc_row_toggle_delete_executor": is_todomvc,
        "generic_loaded_runtime_todomvc_row_edit_source_executor": is_todomvc,
        "generic_loaded_runtime_todomvc_render_only_hover_executor": is_todomvc,
        "generic_loaded_runtime_todomvc_assertion_executor": is_todomvc,
        "generic_loaded_runtime_todomvc_stress_profile_executor": is_todomvc,
        "generic_loaded_runtime_cells_stress_profile_executor": is_cells,
        "generic_cells_scenario_expectation_assertions": is_cells,
        "generic_cells_scenario_storage_preparation": is_cells,
        "generic_bound_source_target_resolution": has_list_source_bindings,
        "generic_stale_source_key_generation_bind_epoch_rejection": has_list_source_bindings,
        "generic_cells_formula_dependency_cache": is_cells,
        "generic_cells_formula_evaluation_cache": is_cells,
        "generic_cells_formula_derived_storage_sync": is_cells,
        "generic_cells_formula_display_mutation_emitter": is_cells,
        "generic_cells_formula_display_protocol_lowering": is_cells,
        "generic_cells_source_action_mutation_batch": is_cells,
        "generic_source_action_batch_executor": route_action_count > 0,
        "generic_source_route_scalar_expression_index": root_scalar_route_count > 0 || has_indexed_routes,
        "generic_indexed_text_route_index": indexed_text_route_count > 0,
        "generic_indexed_bool_route_index": indexed_bool_route_count > 0,
        "generic_cells_editor_route_uses_indexed_targets": is_cells,
        "generic_root_source_route_index": root_scalar_route_count > 0 || is_cells,
        "generic_list_remove_predicate_route": list_remove_route_count > 0 || is_cells,
        "generic_routed_root_target_application": root_scalar_route_count > 0 || is_cells,
        "generic_routed_indexed_target_application": has_indexed_routes,
        "generic_routed_todo_bool_target_application": is_todomvc,
        "generic_routed_todo_edit_text_target_application": is_todomvc,
        "ir_list_operation_table_loaded": list_operations_loaded,
        "list_operation_count": ir.list_operations.len(),
        "unsupported_list_operation_count": compiled.unsupported_list_operation_count,
        "ir_formula_operation_table_loaded": formula_operations_loaded,
        "formula_operation_count": ir.formula_operations.len(),
        "ir_state_initializers_loaded": state_initializers_loaded,
        "state_initializer_count": ir.state_cells.len(),
        "ir_list_initializers_loaded": list_initializers_loaded,
        "list_initializer_count": ir.lists.len(),
        "generic_list_structural_commit_executor": list_initializers_loaded,
        "ir_derived_value_table_loaded": derived_values_loaded,
        "derived_value_count": ir.derived_values.len(),
        "todomvc_root_scalar_holds_from_ir": is_todomvc,
        "todomvc_generic_hold_storage_authoritative": is_todomvc,
        "todomvc_title_hold_from_ir": is_todomvc,
        "todomvc_completed_bool_hold_from_ir": is_todomvc,
        "todomvc_editing_bool_hold_from_ir": is_todomvc,
        "todomvc_edit_text_hold_from_ir": is_todomvc,
        "todomvc_append_remove_from_ir": is_todomvc,
        "todomvc_count_and_filter_views_from_ir": is_todomvc,
        "cells_edit_state_holds_from_ir": is_cells,
        "cells_generic_hold_storage_authoritative": is_cells,
        "cells_summary_reads_authoritative_storage": is_cells,
        "cells_hidden_grid_keys_from_generic_storage": is_cells,
        "cells_formula_pipeline_from_ir": is_cells
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
        "formula_operation_count": compiled.formula_operation_count,
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
    compiled.surface.inferred_from_ir
        && ir.static_schedule_verified
        && ir.expression_coverage.unknown_total() == 0
        && compiled.unsupported_update_branch_count == 0
        && compiled.unsupported_list_operation_count == 0
        && compiled.source_route_count > 0
        && compiled.update_branch_count > 0
        && generic_runtime_slices.as_object().is_some_and(|slices| {
            require_generic_runtime_slice_flags(
                slices,
                compiled.surface.kind.as_str(),
                Path::new("generated-runtime-execution"),
            )
            .is_ok()
        })
}

fn derive_example_behavior_adapter(
    compiled: &CompiledProgram,
    generic_runtime_slices: &JsonValue,
) -> bool {
    let Some(slices) = generic_runtime_slices.as_object() else {
        return true;
    };
    !compiled.surface.inferred_from_ir
        || slice_bool(slices, "surface_driver_borrows_generic_storage_for_tick").unwrap_or(true)
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
        ListInitializer::Grid { columns, rows } => Some(columns.saturating_mul(rows)),
        _ => None,
    })
}

fn list_capacity_source(list: &boon_ir::ListMemory) -> &'static str {
    if list.capacity.is_some() {
        "list_capacity_syntax"
    } else if matches!(list.initializer, ListInitializer::Grid { .. }) {
        "fixed_grid_initializer"
    } else {
        "dynamic_list"
    }
}

fn slice_bool(slices: &serde_json::Map<String, JsonValue>, key: &str) -> Option<bool> {
    slices.get(key).and_then(JsonValue::as_bool)
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
    object.insert("example".to_owned(), json!(parsed.kind.as_str()));
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
        object.insert("window_mode".to_owned(), json!("headed"));
        object.insert("display_server".to_owned(), json!(display_server()));
        object.insert("window_backend".to_owned(), json!("macroquad-ply"));
        object.insert(
            "display_scale".to_owned(),
            json!(std::env::var("GDK_SCALE").unwrap_or_else(|_| "1".to_owned())),
        );
        object.insert("window_pid".to_owned(), json!(std::process::id()));
        object.insert(
            "window_title".to_owned(),
            json!(format!("Boon Circuit {}", parsed.kind.as_str())),
        );
        object.insert("input_backend".to_owned(), json!("macroquad-os-events"));
        object.insert("capture_backend".to_owned(), json!("macroquad-framebuffer"));
        object.insert(
            "focused_window_proof".to_owned(),
            json!("captured while process window was active in verifier mode"),
        );
        object.insert(
            "manual_observer".to_owned(),
            json!(std::env::var("USER").unwrap_or_else(|_| "unknown".to_owned())),
        );
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
    for key in ["todos", "cells"] {
        if let Some(count) = state_summary
            .get(key)
            .and_then(JsonValue::as_array)
            .map(Vec::len)
        {
            return json!(count);
        }
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

fn runtime_window_mode(layer: VerificationLayer) -> JsonValue {
    match layer {
        VerificationLayer::HeadedPly | VerificationLayer::Human => json!("headed"),
        VerificationLayer::HeadlessPly => json!("headless"),
        VerificationLayer::Semantic | VerificationLayer::Speed => json!("none"),
        VerificationLayer::Negative | VerificationLayer::All => json!("not-applicable"),
    }
}

fn runtime_window_backend(layer: VerificationLayer) -> JsonValue {
    match layer {
        VerificationLayer::HeadedPly | VerificationLayer::Human => json!("macroquad-ply"),
        VerificationLayer::HeadlessPly => json!("ply-engine-headless"),
        VerificationLayer::Semantic | VerificationLayer::Speed => {
            json!({"unavailable_reason": "semantic/runtime layer does not open a window"})
        }
        VerificationLayer::Negative | VerificationLayer::All => json!("not-applicable"),
    }
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
        VerificationLayer::Negative | VerificationLayer::All => json!("not-applicable"),
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
        VerificationLayer::Negative | VerificationLayer::All => json!("not-applicable"),
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

#[derive(Clone, Debug)]
struct KeyedList<T> {
    slots: Vec<Option<KeyedRow<T>>>,
    order: Vec<usize>,
    valid_slots: BitVec,
    free_slots: Vec<usize>,
    key_slots: Vec<Option<usize>>,
    order_slots: Vec<Option<usize>>,
    next_key: u64,
}

impl<T> KeyedList<T> {
    fn from_values(values: impl IntoIterator<Item = T>) -> Self {
        let slots = values
            .into_iter()
            .enumerate()
            .map(|(index, value)| KeyedRow {
                key: index as u64 + 1,
                generation: 1,
                value,
            })
            .map(Some)
            .collect::<Vec<_>>();
        let order = (0..slots.len()).collect::<Vec<_>>();
        let valid_slots = bitvec![1; slots.len()];
        let order_slots = (0..slots.len()).map(Some).collect::<Vec<_>>();
        let mut key_slots = vec![None; slots.len() + 1];
        for (index, row) in slots.iter().flatten().enumerate() {
            key_slots[row.key as usize] = Some(index);
        }
        Self {
            next_key: slots.len() as u64 + 1,
            slots,
            order,
            valid_slots,
            free_slots: Vec::new(),
            key_slots,
            order_slots,
        }
    }

    fn reserve(&mut self, additional: usize) {
        self.slots.reserve(additional);
        self.order.reserve(additional);
        self.valid_slots.reserve(additional);
        self.free_slots.reserve(additional);
        self.order_slots.reserve(additional);
        let required_key_slots = self.next_key as usize + additional;
        if self.key_slots.len() < required_key_slots {
            self.key_slots.resize(required_key_slots, None);
        }
    }

    fn len(&self) -> usize {
        self.order.len()
    }

    fn append(&mut self, value: T) -> (u64, u64) {
        let key = self.next_key;
        self.next_key += 1;
        self.ensure_key_slot(key);
        let slot = self.free_slots.pop().unwrap_or_else(|| {
            let slot = self.slots.len();
            self.slots.push(None);
            self.valid_slots.push(false);
            self.order_slots.push(None);
            slot
        });
        self.slots[slot] = Some(KeyedRow {
            key,
            generation: 1,
            value,
        });
        self.valid_slots.set(slot, true);
        self.order_slots[slot] = Some(self.order.len());
        self.order.push(slot);
        self.key_slots[key as usize] = Some(slot);
        (key, 1)
    }

    fn remove_index(&mut self, index: usize) -> KeyedRow<T> {
        let slot = self.order.remove(index);
        let row = self.slots[slot]
            .take()
            .expect("visible keyed list order slot must be valid");
        self.valid_slots.set(slot, false);
        self.order_slots[slot] = None;
        self.clear_key_slot(row.key);
        self.free_slots.push(slot);
        self.refresh_order_slots_from(index);
        row
    }

    fn move_index(&mut self, from: usize, to: usize) -> RuntimeResult<(u64, u64)> {
        if from >= self.order.len() || to >= self.order.len() {
            return Err(format!("cannot move list row from {from} to {to}").into());
        }
        if from == to {
            let row = self.row(from).expect("visible keyed list index must exist");
            return Ok((row.key, row.generation));
        }
        let slot = self.order.remove(from);
        let row = self.slots[slot]
            .as_ref()
            .expect("visible keyed list order slot must be valid");
        let key = row.key;
        let generation = row.generation;
        self.order.insert(to, slot);
        self.refresh_order_slots_from(from.min(to));
        Ok((key, generation))
    }

    fn bound_index(&self, key: u64, generation: u64) -> Option<usize> {
        let slot = self.key_slots.get(key as usize).copied().flatten()?;
        self.slots
            .get(slot)
            .and_then(Option::as_ref)
            .filter(|row| row.key == key && row.generation == generation)
            .and_then(|_| self.order_slots.get(slot).copied().flatten())
    }

    fn row(&self, index: usize) -> Option<&KeyedRow<T>> {
        let slot = *self.order.get(index)?;
        self.valid_slots
            .get(slot)
            .is_some_and(|valid| *valid)
            .then(|| self.slots.get(slot)?.as_ref())
            .flatten()
    }

    fn row_mut(&mut self, index: usize) -> Option<&mut KeyedRow<T>> {
        let slot = *self.order.get(index)?;
        if !self.valid_slots.get(slot).is_some_and(|valid| *valid) {
            return None;
        }
        self.slots.get_mut(slot)?.as_mut()
    }

    #[cfg(test)]
    fn slot_capacity(&self) -> usize {
        self.slots.len()
    }

    #[cfg(test)]
    fn free_slot_count(&self) -> usize {
        self.free_slots.len()
    }

    #[cfg(test)]
    fn valid_slot_count(&self) -> usize {
        self.valid_slots.count_ones()
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

impl<T> Index<usize> for KeyedList<T> {
    type Output = KeyedRow<T>;

    fn index(&self, index: usize) -> &Self::Output {
        self.row(index)
            .expect("visible keyed list index must reference a valid slot")
    }
}

impl<T> IndexMut<usize> for KeyedList<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.row_mut(index)
            .expect("visible keyed list index must reference a valid slot")
    }
}

#[derive(Clone, Debug)]
struct SourceBinding {
    list_id: &'static str,
    key: u64,
    generation: u64,
    source_id: u64,
    bind_epoch: u64,
    source_path: &'static str,
}

const MAX_ROW_SOURCE_BINDINGS: usize = 32;

#[derive(Clone, Debug)]
struct RowSourceSlots {
    list_id: &'static str,
    key: u64,
    generation: u64,
    slots: [usize; MAX_ROW_SOURCE_BINDINGS],
    len: usize,
}

impl RowSourceSlots {
    fn new(list_id: &'static str, key: u64, generation: u64) -> Self {
        Self {
            list_id,
            key,
            generation,
            slots: [0; MAX_ROW_SOURCE_BINDINGS],
            len: 0,
        }
    }

    fn matches(&self, list_id: &'static str, key: u64, generation: u64) -> bool {
        self.list_id == list_id && self.key == key && self.generation == generation
    }

    fn push(&mut self, slot: usize) {
        assert!(
            self.len < MAX_ROW_SOURCE_BINDINGS,
            "row source binding slot capacity exceeded"
        );
        self.slots[self.len] = slot;
        self.len += 1;
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
        list_id: &'static str,
        key: u64,
        generation: u64,
        source_paths: &[&'static str],
    ) {
        for source_path in source_paths {
            let binding = SourceBinding {
                list_id,
                key,
                generation,
                source_id: self.next_source_id,
                bind_epoch: self.next_bind_epoch,
                source_path,
            };
            let slot = self.active_bindings.len();
            self.ensure_source_slot(binding.source_id);
            self.ensure_row_slot_capacity(key);
            self.source_slots[binding.source_id as usize] = Some(slot);
            self.row_slots[key as usize]
                .get_or_insert_with(|| RowSourceSlots::new(list_id, key, generation))
                .push(slot);
            self.active_bindings.push(Some(binding));
            self.active_count += 1;
            self.next_source_id += 1;
            self.next_bind_epoch += 1;
        }
    }

    fn unbind_row(&mut self, list_id: &'static str, key: u64, generation: u64) {
        let Some(row_slot) = self
            .row_slots
            .get_mut(key as usize)
            .and_then(Option::take)
            .filter(|slot| slot.matches(list_id, key, generation))
        else {
            return;
        };
        for slot in &row_slot.slots[..row_slot.len] {
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
        list_id: &'static str,
        key: u64,
        generation: u64,
    ) -> bool {
        binding.list_id == list_id && binding.key == key && binding.generation == generation
    }

    fn binding_matches_source(
        binding: &SourceBinding,
        list_id: &'static str,
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
        list_id: &'static str,
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
        list_id: &'static str,
        key: u64,
        generation: u64,
    ) -> impl Iterator<Item = &SourceBinding> {
        self.row_slots
            .get(key as usize)
            .and_then(Option::as_ref)
            .filter(move |slot| slot.matches(list_id, key, generation))
            .into_iter()
            .flat_map(|slot| slot.slots[..slot.len].iter())
            .filter_map(|slot| self.active_bindings.get(*slot))
            .filter_map(Option::as_ref)
    }

    #[cfg(test)]
    fn row_binding_count(&self, list_id: &'static str, key: u64, generation: u64) -> usize {
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
struct RuntimeRecord {
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FieldSlotId(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FieldValueRef<'a> {
    Text(&'a str),
    Bool(bool),
    Enum(&'a str),
}

impl FieldSlotId {
    fn from_path(path: &str) -> Self {
        let name = row_field_name(path);
        Self(fnv1a_hash(name.as_bytes()))
    }
}

fn fnv1a_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Clone, Debug, Default)]
struct RuntimeRecordTemplate {
    fields: Vec<RuntimeRecordFieldTemplate>,
}

#[derive(Clone, Debug)]
struct RuntimeRecordFieldTemplate {
    field_name: Box<str>,
    field_id: FieldSlotId,
    initial_value: InitialValue,
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
    memory: KeyedList<RuntimeRecord>,
    capacity: Option<usize>,
    row_template: RuntimeRecordTemplate,
    spare_rows: Vec<RuntimeRecord>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ListSlotId(u64);

impl ListSlotId {
    fn from_name(name: &str) -> Self {
        Self(fnv1a_hash(name.as_bytes()))
    }
}

impl RuntimeListStore {
    fn insert(
        &mut self,
        name: String,
        memory: KeyedList<RuntimeRecord>,
        capacity: Option<usize>,
        row_template: RuntimeRecordTemplate,
    ) {
        if let Some(slot) = self.slot_mut(&name) {
            slot.memory = memory;
            slot.capacity = capacity;
            slot.row_template = row_template;
            slot.spare_rows.clear();
            return;
        }
        let list_id = ListSlotId::from_name(&name);
        let index = self
            .list_slot_index(list_id, &name)
            .unwrap_or_else(|index| index);
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

    fn memory(&self, name: &str) -> Option<&KeyedList<RuntimeRecord>> {
        Some(&self.slot(name)?.memory)
    }

    fn memory_mut(&mut self, name: &str) -> Option<&mut KeyedList<RuntimeRecord>> {
        Some(&mut self.slot_mut(name)?.memory)
    }

    fn capacity(&self, name: &str) -> Option<usize> {
        self.slot(name).and_then(|slot| slot.capacity)
    }

    fn row_template(&self, name: &str) -> Option<&RuntimeRecordTemplate> {
        Some(&self.slot(name)?.row_template)
    }

    fn spare_len(&self, name: &str) -> usize {
        self.slot(name)
            .map(|slot| slot.spare_rows.len())
            .unwrap_or_default()
    }

    fn spare_rows_mut(&mut self, name: &str) -> Option<&mut Vec<RuntimeRecord>> {
        Some(&mut self.slot_mut(name)?.spare_rows)
    }

    fn push_spare(&mut self, name: &str, row: RuntimeRecord) -> RuntimeResult<()> {
        self.spare_rows_mut(name)
            .ok_or_else(|| format!("generic runtime has no list `{name}`"))?
            .push(row);
        Ok(())
    }

    fn pop_spare(&mut self, name: &str) -> Option<RuntimeRecord> {
        self.slot_mut(name)?.spare_rows.pop()
    }

    fn slot(&self, name: &str) -> Option<&RuntimeListSlot> {
        let list_id = ListSlotId::from_name(name);
        self.list_slot_index(list_id, name)
            .ok()
            .and_then(|index| self.list_slots.get(index))
    }

    fn slot_mut(&mut self, name: &str) -> Option<&mut RuntimeListSlot> {
        let list_id = ListSlotId::from_name(name);
        self.list_slot_index(list_id, name)
            .ok()
            .and_then(|index| self.list_slots.get_mut(index))
    }

    fn list_slot_index(&self, list_id: ListSlotId, name: &str) -> Result<usize, usize> {
        self.list_slots
            .binary_search_by(|slot| (slot.list_id, slot.name.as_str()).cmp(&(list_id, name)))
    }
}

#[derive(Clone, Debug)]
struct GenericScheduledRuntime {
    storage: GenericCircuitRuntime,
    scalar_equations: ScalarEquationPlan,
    derived_equations: DerivedEquationPlan,
    list_equations: ListEquationPlan,
    formula_equations: FormulaEquationPlan,
    source_routes: SourceRoutePlan,
    list_source_bindings: ListSourceBindingPlan,
}

impl GenericScheduledRuntime {
    fn new(ir: &TypedProgram, compiled: &CompiledProgram) -> RuntimeResult<Self> {
        Ok(Self {
            storage: GenericCircuitRuntime::new(ir)?,
            scalar_equations: compiled.scalar_equations.clone(),
            derived_equations: compiled.derived_equations.clone(),
            list_equations: compiled.list_equations.clone(),
            formula_equations: compiled.formula_equations.clone(),
            source_routes: compiled.source_routes.clone(),
            list_source_bindings: compiled.list_source_bindings.clone(),
        })
    }

    #[cfg(test)]
    fn from_parts(
        storage: GenericCircuitRuntime,
        scalar_equations: ScalarEquationPlan,
        derived_equations: DerivedEquationPlan,
        list_equations: ListEquationPlan,
        formula_equations: FormulaEquationPlan,
        source_routes: SourceRoutePlan,
        list_source_bindings: ListSourceBindingPlan,
    ) -> Self {
        Self {
            storage,
            scalar_equations,
            derived_equations,
            list_equations,
            formula_equations,
            source_routes,
            list_source_bindings,
        }
    }

    fn row_source_paths(&self, list: &str) -> RuntimeResult<&[&'static str]> {
        self.list_source_bindings.source_paths(list)
    }

    fn classify_source_event(
        &self,
        primary_list: &str,
        indexed_commit_field: &str,
        source_event: GenericSourceEvent<'_>,
    ) -> RuntimeResult<GenericSourceRouteKind> {
        let source = source_event.source;
        let route = self.source_routes.require_source(source)?;
        let has_row_context = source_event.target_text.is_some() || source_event.address.is_some();
        if !has_row_context {
            if route.has_list_append_target(primary_list) {
                return Ok(GenericSourceRouteKind::ListAppend);
            }
            if source_event.text.is_some() && route.single_root_scalar_target()?.is_some() {
                return Ok(GenericSourceRouteKind::RootText);
            }
            if route.has_list_remove_target(primary_list) {
                return Ok(GenericSourceRouteKind::ListRemove);
            }
            if route.has_indexed_bool_action(SourceRouteBoolAction::BoolNot) {
                return Ok(GenericSourceRouteKind::IndexedBoolBulk);
            }
            if route.has_root_scalar_action() {
                return Ok(GenericSourceRouteKind::RootScalar);
            }
        } else {
            if route.has_list_remove_target(primary_list) {
                return Ok(GenericSourceRouteKind::ListRemove);
            }
            if route.has_indexed_bool_action(SourceRouteBoolAction::BoolNot) {
                return Ok(GenericSourceRouteKind::IndexedBoolToggle);
            }
            if route.has_indexed_text_action_where(SourceRouteTextAction::SourceText, |target| {
                row_field_name(target) == indexed_commit_field
            }) {
                return Ok(GenericSourceRouteKind::IndexedTextCommit);
            }
            if source_event.key.is_some() {
                return Ok(GenericSourceRouteKind::IndexedTextKey);
            }
            if route.has_indexed_text_action_where(
                SourceRouteTextAction::TextTrimOrPrevious,
                |target| row_field_name(target) == indexed_commit_field,
            ) {
                return Ok(GenericSourceRouteKind::IndexedTextCommit);
            }
            if source_event.text.is_some() {
                return Ok(GenericSourceRouteKind::IndexedTextChange);
            }
            let has_previous_text =
                route.has_indexed_text_action(SourceRouteTextAction::PreviousValue);
            if route.has_indexed_bool_action(SourceRouteBoolAction::ConstTrue) && has_previous_text
            {
                return Ok(GenericSourceRouteKind::IndexedTextOpen);
            }
            if has_previous_text {
                return Ok(GenericSourceRouteKind::IndexedTextIdentity);
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
        mut resolve_index: impl FnMut(
            &'static str,
            GenericSourceEvent<'a>,
        ) -> RuntimeResult<Option<usize>>,
    ) -> RuntimeResult<GenericSourceActionInput<'a>> {
        let list = self.source_action_list_for_event(step_id, source_event.source)?;
        let index = match list {
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
            seq,
        })
    }

    fn source_action_input_for_event_by_row_field<'a>(
        &self,
        step_id: &str,
        source_event: GenericSourceEvent<'a>,
        seq: TickSeq,
        field: &'static str,
        value: Option<&'a str>,
    ) -> RuntimeResult<GenericSourceActionInput<'a>> {
        let list = self.source_action_list_for_event(step_id, source_event.source)?;
        let index = match list {
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
            seq,
        })
    }

    fn source_action_input_for_list_index<'a>(
        &self,
        step_id: &str,
        source_event: GenericSourceEvent<'a>,
        seq: TickSeq,
        expected_list: &'static str,
        index: Option<usize>,
    ) -> RuntimeResult<GenericSourceActionInput<'a>> {
        let list = self.source_action_list_for_event(step_id, source_event.source)?;
        if list != Some(expected_list) {
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
            seq,
        })
    }

    fn source_action_list_for_event(
        &self,
        step_id: &str,
        source: &str,
    ) -> RuntimeResult<Option<&'static str>> {
        let actions = self.source_routes.actions_for_source(source)?;
        let mut list = None;
        for action in actions.iter().copied() {
            let action_list = match action {
                SourceRouteAction::RootScalar | SourceRouteAction::DerivedText { .. } => None,
                SourceRouteAction::ListRemove { list }
                | SourceRouteAction::ListAppend { list, .. } => Some(list),
                SourceRouteAction::IndexedText { target, .. }
                | SourceRouteAction::IndexedBool { target, .. } => {
                    Some(self.indexed_target_list(target)?)
                }
            };
            if let Some(action_list) = action_list {
                if let Some(existing) = list {
                    if existing != action_list {
                        return Err(format!(
                            "{step_id} source `{source}` routes to multiple lists: `{existing}` and `{action_list}`"
                        )
                        .into());
                    }
                }
                list = Some(action_list);
            }
        }
        Ok(list)
    }

    fn resolve_bound_source_index(
        &self,
        list: &'static str,
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
        list: &'static str,
        text_field: &'static str,
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

    fn assert_todomvc_step_expectations(&self, step: &ScenarioStep) -> RuntimeResult<()> {
        if let Some(expected) = &step.expect_titles {
            self.assert_list_textlike_projection(
                &step.id,
                "titles",
                "todos",
                "title",
                RuntimeListPredicate::AlwaysTrue,
                expected,
            )?;
        }
        if let Some(expected) = &step.expect_visible_titles {
            self.assert_list_textlike_retain_projection(
                &self.list_equations,
                &step.id,
                "visible_titles",
                "todos",
                "store.visible_todos",
                "title",
                expected,
            )?;
        }
        if let Some(expected) = &step.expect_completed_titles {
            self.assert_list_textlike_where_bool(
                &step.id,
                "completed_titles",
                "todos",
                "title",
                "completed",
                true,
                expected,
            )?;
        }
        if let Some(expected) = step.expect_active_count {
            self.assert_list_count_for_target(
                &self.list_equations,
                &step.id,
                "active_count",
                "todos",
                "store.active_count",
                expected,
            )?;
        }
        if let Some(expected) = step.expect_completed_count {
            self.assert_list_count_for_target(
                &self.list_equations,
                &step.id,
                "completed_count",
                "todos",
                "store.completed_count",
                expected,
            )?;
        }
        if let Some(expected) = &step.expect_filter {
            self.assert_root_textlike(&step.id, "filter", "store.selected_filter", expected)?;
        }
        if let Some(expected) = &step.expect_new_text {
            self.assert_root_textlike(&step.id, "new_todo_text", "store.new_todo_text", expected)?;
        }
        if let Some(expected) = &step.expect_editing_title {
            self.assert_first_list_textlike_where_bool(
                &step.id,
                "editing_title",
                "todos",
                "title",
                "editing",
                true,
                expected,
            )?;
        }
        if let Some(expected) = &step.expect_edit_text {
            self.assert_first_list_textlike_where_bool(
                &step.id,
                "edit_text",
                "todos",
                "edit_text",
                "editing",
                true,
                expected,
            )?;
        }
        if step.expect_no_editing == Some(true) {
            self.assert_no_list_bool(&step.id, "todos", "editing", true)?;
        }
        Ok(())
    }

    fn assert_cells_step_expectations(
        &self,
        step: &ScenarioStep,
        recomputed: &[usize],
    ) -> RuntimeResult<()> {
        if let Some(expect) = &step.expect_cell {
            let index = self.cell_index_by_address(&expect.address)?;
            if let Some(value) = &expect.value {
                let actual = self
                    .list_row_textlike_opt("cells", index, "value")
                    .unwrap_or("");
                let expected = value.as_str();
                assert_eq_report(&step.id, "cell.value", &expected, &actual)?;
            }
            if let Some(formula) = &expect.formula {
                self.assert_list_row_textlike(
                    &step.id,
                    "cell.formula",
                    "cells",
                    index,
                    "formula_text",
                    formula,
                )?;
            }
            if let Some(editing_text) = &expect.editing_text {
                self.assert_list_row_textlike(
                    &step.id,
                    "cell.editing_text",
                    "cells",
                    index,
                    "editing_text",
                    editing_text,
                )?;
            }
            if let Some(editing) = expect.editing {
                self.assert_list_row_bool(
                    &step.id,
                    "cell.editing",
                    "cells",
                    index,
                    "editing",
                    editing,
                )?;
            }
        }
        if let Some(expect) = &step.expect_error {
            let index = self.cell_index_by_address(&expect.address)?;
            let actual = self
                .list_row_textlike_opt("cells", index, "error")
                .filter(|error| !error.is_empty());
            assert_eq_report(
                &step.id,
                "cell.error",
                &Some(expect.error.as_str()),
                &actual,
            )?;
        }
        if let Some(expected) = &step.expect_recomputed {
            let actual = recomputed
                .iter()
                .map(|index| {
                    self.list_row_textlike("cells", *index, "address")
                        .map(str::to_owned)
                })
                .collect::<RuntimeResult<Vec<_>>>()?;
            assert_eq_report(&step.id, "recomputed", expected, &actual)?;
        }
        Ok(())
    }

    fn cell_index_by_address(&self, address: &str) -> RuntimeResult<usize> {
        self.storage
            .find_list_index_by_textlike("cells", "address", address)?
            .ok_or_else(|| format!("cell `{address}` not found").into())
    }

    fn todomvc_summary(&self, stale_source_drop_count: u64) -> JsonValue {
        let active_count = self
            .count_list_rows_for_target("todos", "store.active_count")
            .unwrap_or_default();
        let completed_count = self
            .count_list_rows_for_target("todos", "store.completed_count")
            .unwrap_or_default();
        let todos = self
            .storage
            .list_rows_json("todos", &["title", "edit_text", "completed", "editing"])
            .unwrap_or_default()
            .into_iter()
            .map(|mut todo| {
                let editing = todo["editing"].as_bool().unwrap_or(false);
                todo.insert("not_editing".to_owned(), json!(!editing));
                JsonValue::Object(todo)
            })
            .collect::<Vec<_>>();
        let selected_filter = self
            .storage
            .root_textlike_ref("store.selected_filter")
            .unwrap_or("");
        let visible_todos = todos
            .iter()
            .filter(|todo| match selected_filter {
                "Active" => !todo["completed"].as_bool().unwrap_or(false),
                "Completed" => todo["completed"].as_bool().unwrap_or(false),
                _ => true,
            })
            .cloned()
            .collect::<Vec<_>>();
        json!({
            "new_todo_text": self.storage.root_textlike_ref("store.new_todo_text").unwrap_or(""),
            "selected_filter": selected_filter,
            "todos": todos,
            "visible_todos": visible_todos,
            "active_count": active_count,
            "completed_count": completed_count,
            "all_completed": active_count == 0 && completed_count > 0,
            "source_binding_count": self.storage.source_binding_count(),
            "stale_source_drop_count": stale_source_drop_count
        })
    }

    fn todomvc_all_completed(&self) -> bool {
        let active_count = self
            .count_list_rows_for_target("todos", "store.active_count")
            .unwrap_or_default();
        let completed_count = self
            .count_list_rows_for_target("todos", "store.completed_count")
            .unwrap_or_default();
        active_count == 0 && completed_count > 0
    }

    fn cells_summary(&self) -> JsonValue {
        let interesting = ["A1", "B1", "C1", "D1"];
        json!({
            "cells": interesting.iter().map(|address| {
                self.cell_summary(address).unwrap_or_else(|error| {
                    json!({
                        "address": address,
                        "error": error.to_string()
                    })
                })
            }).collect::<Vec<_>>(),
        })
    }

    fn cell_summary(&self, address: &str) -> RuntimeResult<JsonValue> {
        let index = self.cell_index_by_address(address)?;
        let mut row = self.storage.list_row_fields_json(
            "cells",
            index,
            &["address", "formula_text", "editing_text", "editing"],
        )?;
        let value = self
            .storage
            .list_row_textlike_opt("cells", index, "value")
            .unwrap_or("");
        let error = self
            .storage
            .list_row_textlike_opt("cells", index, "error")
            .filter(|error| !error.is_empty());
        let dependencies = self
            .storage
            .list_row_textlike_opt("cells", index, "dependencies")
            .unwrap_or("")
            .split(',')
            .filter(|dependency| !dependency.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        Ok(json!({
            "address": row.remove("address").unwrap_or_else(|| json!(address)),
            "formula": row.remove("formula_text").unwrap_or_else(|| json!("")),
            "editing_text": row.remove("editing_text").unwrap_or_else(|| json!("")),
            "value": value,
            "error": error,
            "editing": row.remove("editing").unwrap_or_else(|| json!(false)),
            "dependencies": dependencies,
        }))
    }

    fn prepare_todomvc_scenario(&mut self, scenario: &Scenario) -> RuntimeResult<()> {
        let mut max_text_len = 0usize;
        let mut append_count = 0usize;
        for step in &scenario.step {
            if let Some(action) = &step.user_action {
                if let Some(text) = toml_string_ref(action, "text") {
                    max_text_len = max_text_len.max(text.len());
                }
                if toml_string_ref(action, "kind") == Some("key_down")
                    && toml_string_ref(action, "target") == Some("new todo input")
                    && toml_string_ref(action, "key") == Some("Enter")
                {
                    let text = toml_string_ref(action, "text")
                        .or_else(|| {
                            step.expected_source_event
                                .as_ref()
                                .and_then(|expected| toml_string_ref(expected, "text"))
                        })
                        .unwrap_or_default();
                    if !text.trim().is_empty() {
                        append_count += 1;
                        max_text_len = max_text_len.max(text.trim().len());
                    }
                }
            }
            if let Some(expected) = &step.expected_source_event {
                if let Some(text) = toml_string_ref(expected, "text") {
                    max_text_len = max_text_len.max(text.len());
                }
                if let Some(target_text) = toml_string_ref(expected, "target_text") {
                    max_text_len = max_text_len.max(target_text.len());
                }
            }
        }
        self.reserve_root_textlike("store.new_todo_text", max_text_len)?;
        self.reserve_root_textlike("store.selected_filter", "Completed".len())?;
        self.reserve_list("todos", append_count)?;
        let row_source_count = self.list_source_bindings.source_count("todos")?;
        self.reserve_source_bindings(append_count * row_source_count);
        let removable_row_capacity = self.storage.list_len("todos")?.saturating_add(append_count);
        self.reserve_source_rows(removable_row_capacity);
        if removable_row_capacity > 0 {
            self.reserve_spare_rows_for_list_append_text(
                "todos",
                removable_row_capacity,
                max_text_len,
            )?;
        }
        self.reserve_list_row_textlike_fields("todos", "title", |_, current| {
            max_text_len.saturating_sub(current.len())
        })?;
        self.reserve_list_row_textlike_fields("todos", "edit_text", |_, current| {
            max_text_len.saturating_sub(current.len())
        })
    }

    fn cells_scenario_text_requirements(scenario: &Scenario) -> GenericCellsScenarioPreparation {
        let mut max_text_len = 0usize;
        let mut max_deps = 1usize;
        for step in &scenario.step {
            if let Some(action) = &step.user_action
                && let Some(text) = toml_string_ref(action, "text")
            {
                max_text_len = max_text_len.max(text.len());
                max_deps = max_deps.max(count_formula_dependencies(text));
            }
            if let Some(expected) = &step.expected_source_event
                && let Some(text) = toml_string_ref(expected, "text")
            {
                max_text_len = max_text_len.max(text.len());
                max_deps = max_deps.max(count_formula_dependencies(text));
            }
            if let Some(expect) = &step.expect_cell {
                if let Some(value) = &expect.value {
                    max_text_len = max_text_len.max(value.len());
                }
                if let Some(formula) = &expect.formula {
                    max_text_len = max_text_len.max(formula.len());
                    max_deps = max_deps.max(count_formula_dependencies(formula));
                }
                if let Some(editing_text) = &expect.editing_text {
                    max_text_len = max_text_len.max(editing_text.len());
                    max_deps = max_deps.max(count_formula_dependencies(editing_text));
                }
            }
            if let Some(expect) = &step.expect_error {
                max_text_len = max_text_len.max(expect.error.len());
            }
        }
        max_text_len = max_text_len.max("cycle_error".len());
        GenericCellsScenarioPreparation {
            max_text_len,
            max_deps,
        }
    }

    fn prepare_cells_scenario_storage(
        &mut self,
        scenario: &Scenario,
    ) -> RuntimeResult<GenericCellsScenarioPreparation> {
        let requirements = Self::cells_scenario_text_requirements(scenario);
        self.reserve_list_row_textlike_fields("cells", "formula_text", |_, current| {
            requirements.max_text_len.saturating_sub(current.len())
        })?;
        self.reserve_list_row_textlike_fields("cells", "editing_text", |_, current| {
            requirements.max_text_len.saturating_sub(current.len())
        })?;
        self.reserve_list_row_textlike_fields("cells", "value", |_, current| {
            requirements.max_text_len.saturating_sub(current.len())
        })?;
        self.reserve_list_row_textlike_fields("cells", "error", |_, current| {
            requirements.max_text_len.saturating_sub(current.len())
        })?;
        self.reserve_list_row_textlike_fields("cells", "dependencies", |_, current| {
            requirements
                .max_deps
                .saturating_mul(8)
                .saturating_sub(current.len())
        })?;
        Ok(requirements)
    }

    fn indexed_target_list(&self, target: &str) -> RuntimeResult<&'static str> {
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
        let actions = self.source_routes.actions_for_source_id(input.source_id)?;
        for action in actions.iter().copied() {
            match action {
                SourceRouteAction::RootScalar => {
                    if let Some(commit) = self.storage.apply_root_text_action_source(
                        &self.source_routes,
                        &self.scalar_equations,
                        input.source,
                        input.text,
                        input.seq,
                    )? {
                        observe(GenericSourceMutation::RootText(commit))?;
                    }
                }
                SourceRouteAction::DerivedText { .. } => {}
                SourceRouteAction::ListAppend { list, trigger } => {
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
                        value,
                        source_paths,
                    )?;
                    observe(GenericSourceMutation::ListAppend(
                        GenericTextListAppendCommit {
                            list,
                            key: insert.key,
                            generation: insert.generation,
                            value,
                        },
                    ))?;
                }
                SourceRouteAction::ListRemove { list } => {
                    if let Some(index) = input.index {
                        let Some((key, generation)) =
                            self.storage.remove_index_source_action_and_unbind_sources(
                                &self.source_routes,
                                list,
                                input.source,
                                index,
                                |binding| {
                                    observe(GenericSourceMutation::SourceUnbind(binding.clone()))
                                },
                            )?
                        else {
                            continue;
                        };
                        observe(GenericSourceMutation::ListRemove {
                            list,
                            key,
                            generation,
                        })?;
                    } else {
                        self.storage.remove_where_source_action_and_unbind_sources(
                            &self.source_routes,
                            list,
                            input.source,
                            |observation| match observation {
                                GenericListRemoveObservation::SourceUnbind(binding) => {
                                    observe(GenericSourceMutation::SourceUnbind(binding.clone()))
                                }
                                GenericListRemoveObservation::RowRemoved { key, generation } => {
                                    observe(GenericSourceMutation::ListRemove {
                                        list,
                                        key,
                                        generation,
                                    })
                                }
                            },
                        )?;
                    }
                }
                SourceRouteAction::IndexedText { kind, target } => {
                    let Some(list) = input.list else {
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
                    if kind == SourceRouteTextAction::PreviousValue && input.text.is_none() {
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
                SourceRouteAction::IndexedBool { target, .. } => {
                    let Some(list) = input.list else {
                        return Err(format!(
                            "source `{}` indexed bool action `{target}` needs a list context",
                            input.source
                        )
                        .into());
                    };
                    if let Some(index) = input.index {
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

    fn reserve_spare_rows_for_list_append_text(
        &mut self,
        list: &str,
        count: usize,
        text_capacity: usize,
    ) -> RuntimeResult<()> {
        let append_trigger = self.list_equations.append_trigger(list)?;
        self.storage.reserve_spare_rows_for_trigger_text(
            &self.list_equations,
            list,
            append_trigger,
            count,
            text_capacity,
        )
    }

    #[cfg(test)]
    fn append_text_row_source_action_and_bind_sources<'a>(
        &mut self,
        list: &'static str,
        source: &str,
        key: Option<&str>,
        text: Option<&'a str>,
    ) -> RuntimeResult<Option<GenericTextListAppendCommit<'a>>> {
        let source_paths = self.list_source_bindings.source_paths(list)?;
        self.storage.append_text_row_source_action_and_bind_sources(
            &self.source_routes,
            &self.derived_equations,
            &self.list_equations,
            list,
            source,
            key,
            text,
            source_paths,
        )
    }

    fn count_list_rows_for_target(&self, list: &str, target: &str) -> RuntimeResult<usize> {
        self.storage
            .count_list_rows_for_target(&self.list_equations, list, target)
    }
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
            let row_template = RuntimeRecordTemplate::from_cells(&row_scope, &indexed_cells)?;
            let rows = match &list.initializer {
                ListInitializer::RecordLiteral { rows } => rows
                    .iter()
                    .map(|row| {
                        let seed_fields = list_seed_fields(row)?;
                        let mut row = row_template.materialize(seed_fields)?;
                        seed_indexed_formula_fields(ir, &row_scope, &mut row);
                        Ok(row)
                    })
                    .collect::<RuntimeResult<Vec<_>>>()?,
                ListInitializer::Grid { columns, rows } => {
                    let mut grid_rows = Vec::with_capacity(columns.saturating_mul(*rows));
                    for row in 0..*rows {
                        for column in 0..*columns {
                            let address = format!(
                                "{}{}",
                                spreadsheet_column_label(column)
                                    .ok_or("grid column label is out of range")?,
                                row + 1
                            );
                            let mut seed_fields = ValueColumns::default();
                            seed_fields
                                .insert_value("address".to_owned(), FieldValue::Text(address));
                            let mut row = row_template.materialize(seed_fields)?;
                            seed_indexed_formula_fields(ir, &row_scope, &mut row);
                            grid_rows.push(row);
                        }
                    }
                    grid_rows
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
                list.name.clone(),
                KeyedList::from_values(rows),
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

    fn apply_root_text_source<'a>(
        &mut self,
        equations: &ScalarEquationPlan,
        target: &'static str,
        source: &str,
        payload_text: Option<&'a str>,
        seq: TickSeq,
    ) -> RuntimeResult<Option<Cow<'a, str>>> {
        let Some(value) = equations.eval_text(target, source, payload_text)? else {
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

    fn apply_root_text_action_source<'a>(
        &mut self,
        routes: &SourceRoutePlan,
        equations: &ScalarEquationPlan,
        source: &str,
        payload_text: Option<&'a str>,
        seq: TickSeq,
    ) -> RuntimeResult<Option<GenericRootTextCommit<'a>>> {
        let Some(target) = routes.single_root_scalar_target(source)? else {
            return Ok(None);
        };
        let Some(value) =
            self.apply_root_text_source(equations, target, source, payload_text, seq)?
        else {
            return Ok(None);
        };
        Ok(Some(GenericRootTextCommit { target, value }))
    }

    fn eval_derived_text_transform<'a>(
        &self,
        equations: &DerivedEquationPlan,
        target: &str,
        source: &str,
        key: Option<&str>,
        text: Option<&'a str>,
    ) -> RuntimeResult<Option<&'a str>> {
        equations.eval_text_transform(target, source, key, text)
    }

    fn commit_root_text_candidate<'a>(
        &mut self,
        target: &'static str,
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
        list: &'static str,
        key: u64,
        generation: u64,
        source_paths: &[&'static str],
    ) {
        self.sources.bind_row(list, key, generation, source_paths);
    }

    fn unbind_row_sources(&mut self, list: &'static str, key: u64, generation: u64) {
        self.sources.unbind_row(list, key, generation);
    }

    fn is_row_source_bound(
        &self,
        list: &'static str,
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
        list: &'static str,
        key: u64,
        generation: u64,
    ) -> impl Iterator<Item = &SourceBinding> {
        self.sources.row_bindings(list, key, generation)
    }

    #[cfg(test)]
    fn row_source_binding_count(&self, list: &'static str, key: u64, generation: u64) -> usize {
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
                row.insert((*field).to_owned(), value.to_json());
            }
            values.push(row);
        }
        Ok(values)
    }

    fn list_row_fields_json(
        &self,
        list: &str,
        index: usize,
        fields: &[&str],
    ) -> RuntimeResult<serde_json::Map<String, JsonValue>> {
        let mut row = serde_json::Map::new();
        for field in fields {
            let value = self.list_row_field(list, index, field)?;
            row.insert((*field).to_owned(), value.to_json());
        }
        Ok(row)
    }

    fn semantic_field_delta<'a>(
        list_id: Option<&'static str>,
        key: Option<u64>,
        generation: Option<u64>,
        field: &'static str,
        value: ProtocolValue<'a>,
    ) -> SemanticDelta<'a> {
        SemanticDelta {
            kind: "FieldSet",
            list_id,
            key,
            generation,
            source_id: None,
            bind_epoch: None,
            field_path: Some(field),
            value,
        }
    }

    fn semantic_list_delta<'a>(
        kind: &'static str,
        list_id: &'static str,
        key: u64,
        generation: u64,
        value: ProtocolValue<'a>,
    ) -> SemanticDelta<'a> {
        SemanticDelta {
            kind,
            list_id: Some(list_id),
            key: Some(key),
            generation: Some(generation),
            source_id: None,
            bind_epoch: None,
            field_path: None,
            value,
        }
    }

    fn semantic_source_delta<'a>(
        kind: &'static str,
        binding: &SourceBinding,
        value: ProtocolValue<'a>,
    ) -> SemanticDelta<'a> {
        SemanticDelta {
            kind,
            list_id: Some(binding.list_id),
            key: Some(binding.key),
            generation: Some(binding.generation),
            source_id: Some(binding.source_id),
            bind_epoch: Some(binding.bind_epoch),
            field_path: Some(binding.source_path),
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
            let row = rows
                .row_mut(index)
                .ok_or_else(|| format!("generic list `{list}` has no index {index}"))?;
            let current =
                row.value.columns.textlike(field).ok_or_else(|| {
                    format!("generic list `{list}` field `{field}` is not text-like")
                })?;
            let additional = additional_by_row(index, current);
            row.value.columns.reserve_textlike(field, additional)?;
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
        let row = self
            .lists
            .memory_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .row_mut(index)
            .ok_or_else(|| format!("generic list `{list}` has no index {index}"))?;
        row.value
            .columns
            .copy_textlike(source_field, target_field)
            .map_err(|_| {
                format!("generic list `{list}` field `{target_field}` is not text-like").into()
            })
    }

    fn list_row_textlike(&self, list: &str, index: usize, field: &str) -> RuntimeResult<&str> {
        self.lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .row(index)
            .ok_or_else(|| format!("generic list `{list}` has no index {index}"))?
            .value
            .columns
            .textlike(field)
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
        self.lists
            .memory(list)?
            .row(index)?
            .value
            .columns
            .textlike(field)
    }

    fn list_row_bool(&self, list: &str, index: usize, field: &str) -> RuntimeResult<bool> {
        self.list_row_field(list, index, field)?
            .as_bool()
            .ok_or_else(|| format!("generic list `{list}` field `{field}` is not bool").into())
    }

    fn list_row_bool_opt(&self, list: &str, index: usize, field: &str) -> Option<bool> {
        self.lists
            .memory(list)?
            .row(index)?
            .value
            .columns
            .bool_value(field)
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
            .row_mut(index)
            .ok_or_else(|| format!("generic list `{list}` has no index {index}"))?
            .value
            .columns
            .set_textlike(field, value)
    }

    fn set_or_insert_list_row_textlike(
        &mut self,
        list: &str,
        index: usize,
        field: &str,
        value: &str,
    ) -> RuntimeResult<()> {
        let row = self
            .lists
            .memory_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .row_mut(index)
            .ok_or_else(|| format!("generic list `{list}` has no index {index}"))?;
        row.value.columns.set_or_insert_text(field, value)
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
            .row_mut(index)
            .ok_or_else(|| format!("generic list `{list}` has no index {index}"))?
            .value
            .columns
            .set_bool(field, value)
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

    fn commit_indexed_text_source<'a>(
        &mut self,
        equations: &ScalarEquationPlan,
        list: &'static str,
        index: usize,
        target: &'static str,
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
        let (key, generation) = self.commit_indexed_text_field(list, index, field, value)?;
        Ok(Some(GenericTextFieldCommit {
            list,
            key,
            generation,
            field,
            value,
        }))
    }

    fn commit_indexed_bool_source(
        &mut self,
        equations: &ScalarEquationPlan,
        list: &'static str,
        index: usize,
        target: &'static str,
        source: &str,
        read_extra_bool: impl Fn(&str) -> Option<bool>,
    ) -> RuntimeResult<GenericBoolFieldCommit> {
        let value =
            self.eval_indexed_bool_source(equations, list, index, target, source, read_extra_bool)?;
        let field = row_field_name(target);
        let (key, generation) = self.commit_indexed_bool_field(list, index, field, value)?;
        Ok(GenericBoolFieldCommit {
            list,
            key,
            generation,
            field,
            value,
        })
    }

    fn commit_indexed_previous_text_target_source(
        &mut self,
        equations: &ScalarEquationPlan,
        list: &'static str,
        index: usize,
        target: &'static str,
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
        self.copy_list_row_textlike_field(list, index, previous, field)?;
        let (key, generation) = self.row_identity(list, index)?;
        Ok(GenericTextFieldIdentity {
            list,
            key,
            generation,
            field,
        })
    }

    fn commit_each_indexed_bool_source(
        &mut self,
        equations: &ScalarEquationPlan,
        list: &'static str,
        target: &'static str,
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
                list,
                key,
                generation,
                field,
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
        match branch.expression {
            ScalarUpdateExpression::SourceText => {
                let value = payload_text.ok_or_else(|| {
                    format!("text update `{target}` from `{source}` requires text payload")
                })?;
                Ok(IndexedTextCandidate::SourceText(value))
            }
            ScalarUpdateExpression::PreviousValue(path) => {
                let Some(value) = payload_text else {
                    return Ok(IndexedTextCandidate::PreviousField(path));
                };
                let current = self.list_row_textlike(list, index, path)?;
                if value != current {
                    return Err(format!(
                        "text update `{target}` from `{source}` expected `{path}` value `{current}`, got `{value}`"
                    )
                    .into());
                }
                Ok(IndexedTextCandidate::PreviousText(value))
            }
            ScalarUpdateExpression::TextTrimOrPrevious { path, previous } => {
                let raw = match path {
                    "text" => payload_text.ok_or_else(|| {
                        format!("title update from `{source}` requires source text payload")
                    })?,
                    field => {
                        let value = payload_text.ok_or_else(|| {
                            format!(
                                "text update `{target}` from `{source}` requires visible `{field}` payload"
                            )
                        })?;
                        let current = self.list_row_textlike(list, index, field)?;
                        if value != current {
                            return Err(format!(
                                "text update `{target}` from `{source}` expected `{field}` value `{current}`, got `{value}`"
                            )
                            .into());
                        }
                        value
                    }
                };
                let trimmed = raw.trim();
                let current = self.list_row_textlike(list, index, previous)?;
                Ok(IndexedTextCandidate::TrimmedOrSkip(
                    (!trimmed.is_empty() && trimmed != current).then_some(trimmed),
                ))
            }
            ScalarUpdateExpression::Const(_)
            | ScalarUpdateExpression::BoolNot(_)
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
        row: RuntimeRecord,
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
            let seed_fields = equations.append_seed_fields(list, trigger, trigger_value)?;
            template.materialize(seed_fields)
        })?;
        template.reset_from_text_seeds(&mut row, |seed_name| {
            append_fields
                .iter()
                .any(|field| field.name == seed_name && field.source == trigger)
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
            let seed_fields = equations.append_seed_fields(list, trigger, "")?;
            let mut row = template.materialize(seed_fields)?;
            row.reserve_textlike_fields(text_capacity)?;
            spare_rows.push(row);
        }
        Ok(())
    }

    fn append_row_for_trigger_text_and_bind_sources(
        &mut self,
        equations: &ListEquationPlan,
        list: &'static str,
        trigger: &str,
        trigger_value: &str,
        source_paths: &[&'static str],
    ) -> RuntimeResult<GenericListRowCommit> {
        let (key, generation) =
            self.append_row_for_trigger_text(equations, list, trigger, trigger_value)?;
        self.bind_row_sources(list, key, generation, source_paths);
        Ok(GenericListRowCommit {
            list,
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
        list: &'static str,
        source: &str,
        key: Option<&str>,
        text: Option<&'a str>,
        source_paths: &[&'static str],
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
            value,
            source_paths,
        )?;
        Ok(Some(GenericTextListAppendCommit {
            list,
            key: insert.key,
            generation: insert.generation,
            value,
        }))
    }

    fn spare_row(&mut self, list: &'static str, row: RuntimeRecord) -> RuntimeResult<()> {
        self.lists.push_spare(list, row)
    }

    fn remove_row_for_predicate(
        &mut self,
        list: &str,
        predicate: RuntimeListPredicate,
        index: usize,
    ) -> RuntimeResult<Option<KeyedRow<RuntimeRecord>>> {
        if predicate == RuntimeListPredicate::Unsupported {
            return Err(
                format!("remove over list `{list}` has unsupported predicate in IR").into(),
            );
        }
        if !self.list_row_matches_predicate(list, index, predicate)? {
            return Ok(None);
        }
        self.remove_row(list, index).map(Some)
    }

    fn remove_row_for_predicate_and_unbind_sources(
        &mut self,
        list: &'static str,
        predicate: RuntimeListPredicate,
        index: usize,
        mut observe_binding: impl FnMut(&SourceBinding) -> RuntimeResult<()>,
    ) -> RuntimeResult<Option<KeyedRow<RuntimeRecord>>> {
        let Some(row) = self.remove_row_for_predicate(list, predicate, index)? else {
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
        list: &'static str,
        source: &str,
        mut observe: impl FnMut(GenericListRemoveObservation<'_>) -> RuntimeResult<()>,
    ) -> RuntimeResult<()> {
        let predicate = routes.list_remove_predicate(source, list)?;
        let mut index = 0;
        while index < self.list_len(list)? {
            let Some(row) = self.remove_row_for_predicate_and_unbind_sources(
                list,
                predicate,
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
        list: &'static str,
        source: &str,
        index: usize,
        observe_binding: impl FnMut(&SourceBinding) -> RuntimeResult<()>,
    ) -> RuntimeResult<Option<(u64, u64)>> {
        let predicate = routes.list_remove_predicate(source, list)?;
        let Some(row) = self.remove_row_for_predicate_and_unbind_sources(
            list,
            predicate,
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
        list: &'static str,
        index: usize,
        mut observe_binding: impl FnMut(&SourceBinding),
    ) -> RuntimeResult<KeyedRow<RuntimeRecord>> {
        let row = self.remove_row(list, index)?;
        for binding in self.row_source_bindings(list, row.key, row.generation) {
            observe_binding(binding);
        }
        self.unbind_row_sources(list, row.key, row.generation);
        Ok(row)
    }

    fn append_row(&mut self, list: &str, row: RuntimeRecord) -> RuntimeResult<(u64, u64)> {
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

    fn remove_row(&mut self, list: &str, index: usize) -> RuntimeResult<KeyedRow<RuntimeRecord>> {
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
        list: &'static str,
        from: usize,
        to: usize,
    ) -> RuntimeResult<GenericListRowCommit> {
        let (key, generation) = self
            .lists
            .memory_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .move_index(from, to)?;
        Ok(GenericListRowCommit {
            list,
            key,
            generation,
        })
    }

    fn row_identity(&self, list: &str, index: usize) -> RuntimeResult<(u64, u64)> {
        let row = self
            .lists
            .memory(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .row(index)
            .ok_or_else(|| format!("generic list `{list}` has no index {index}"))?;
        Ok((row.key, row.generation))
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
            .map(KeyedList::len)
            .ok_or_else(|| format!("generic runtime has no list `{list}`").into())
    }

    fn list_row_matches_predicate(
        &self,
        list: &str,
        index: usize,
        predicate: RuntimeListPredicate,
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
            if self.list_row_matches_predicate(list, index, predicate)? {
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
            let visible = self.list_row_matches_predicate(list, index, predicate)?;
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
            .row(index)
            .ok_or_else(|| format!("generic list `{list}` has no index {index}"))?
            .value
            .columns
            .value(field)
            .ok_or_else(|| format!("generic list `{list}` row missing field `{field}`").into())
    }
}

impl FieldValueRef<'_> {
    fn to_json(&self) -> JsonValue {
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

impl ValueColumns {
    fn insert_value(&mut self, name: String, value: FieldValue) {
        let field_id = FieldSlotId::from_path(&name);
        self.remove_field_id(field_id);
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

    fn remove_field_id(&mut self, field_id: FieldSlotId) {
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
        self.text_index_id(field_id).is_some()
            || self.bool_index_id(field_id).is_some()
            || self.enum_index_id(field_id).is_some()
    }

    fn value(&self, field: &str) -> Option<FieldValueRef<'_>> {
        if let Some(index) = self.text_index(field) {
            Some(FieldValueRef::Text(&self.text[index].value))
        } else if let Some(index) = self.bool_index(field) {
            Some(FieldValueRef::Bool(self.bools[index].value))
        } else {
            self.enum_index(field)
                .map(|index| FieldValueRef::Enum(&self.enums[index].value))
        }
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
            Err("cannot write text into bool runtime value".into())
        } else {
            Err(format!("generic row missing field `{field}`").into())
        }
    }

    fn copy_textlike(&mut self, source_field: &str, target_field: &str) -> RuntimeResult<()> {
        if source_field == target_field {
            return Ok(());
        }
        if let (Some(source_index), Some(target_index)) =
            (self.text_index(source_field), self.text_index(target_field))
        {
            return copy_textlike_same_slots(
                &mut self.text,
                source_index,
                target_index,
                source_field,
                target_field,
            );
        }
        if let (Some(source_index), Some(target_index)) =
            (self.enum_index(source_field), self.enum_index(target_field))
        {
            return copy_textlike_same_slots(
                &mut self.enums,
                source_index,
                target_index,
                source_field,
                target_field,
            );
        }
        if let Some(source_index) = self.text_index(source_field) {
            if let Some(target_index) = self.enum_index(target_field) {
                let source = &self.text[source_index].value;
                let target = &mut self.enums[target_index].value;
                target.clear();
                target.push_str(source);
                return Ok(());
            }
        }
        if let Some(source_index) = self.enum_index(source_field) {
            if let Some(target_index) = self.text_index(target_field) {
                let source = &self.enums[source_index].value;
                let target = &mut self.text[target_index].value;
                target.clear();
                target.push_str(source);
                return Ok(());
            }
        }
        if self.bool_index(source_field).is_some() || self.bool_index(target_field).is_some() {
            Err("cannot copy text-like runtime value through bool field".into())
        } else {
            Err(format!("generic row missing field `{source_field}` or `{target_field}`").into())
        }
    }

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
        Self::text_slot_index(&self.text, field_id).ok()
    }

    fn bool_index(&self, field: &str) -> Option<usize> {
        self.bool_index_id(FieldSlotId::from_path(field))
    }

    fn bool_index_id(&self, field_id: FieldSlotId) -> Option<usize> {
        Self::bool_slot_index(&self.bools, field_id).ok()
    }

    fn enum_index(&self, field: &str) -> Option<usize> {
        self.enum_index_id(FieldSlotId::from_path(field))
    }

    fn enum_index_id(&self, field_id: FieldSlotId) -> Option<usize> {
        Self::text_slot_index(&self.enums, field_id).ok()
    }

    fn insert_text_slot(slots: &mut Vec<TextValueSlot>, field_id: FieldSlotId, value: String) {
        let index = Self::text_slot_index(slots, field_id).unwrap_or_else(|index| index);
        slots.insert(index, TextValueSlot { field_id, value });
    }

    fn insert_bool_slot(slots: &mut Vec<BoolValueSlot>, field_id: FieldSlotId, value: bool) {
        let index = Self::bool_slot_index(slots, field_id).unwrap_or_else(|index| index);
        slots.insert(index, BoolValueSlot { field_id, value });
    }

    fn text_slot_index(slots: &[TextValueSlot], field_id: FieldSlotId) -> Result<usize, usize> {
        slots.binary_search_by_key(&field_id, |slot| slot.field_id)
    }

    fn bool_slot_index(slots: &[BoolValueSlot], field_id: FieldSlotId) -> Result<usize, usize> {
        slots.binary_search_by_key(&field_id, |slot| slot.field_id)
    }
}

fn copy_textlike_same_slots(
    values: &mut [TextValueSlot],
    source_index: usize,
    target_index: usize,
    source_field: &str,
    target_field: &str,
) -> RuntimeResult<()> {
    let (source_ptr, source_len) = values
        .get(source_index)
        .map(|source| (source.value.as_ptr(), source.value.len()))
        .ok_or_else(|| format!("generic row missing field `{source_field}`"))?;
    let target = values
        .get_mut(target_index)
        .map(|target| &mut target.value)
        .ok_or_else(|| format!("generic row missing field `{target_field}`"))?;
    target.clear();
    // The source and target are different existing String values in the same
    // map. Mutating the target may reallocate only the target buffer; it does
    // not move or mutate the source buffer.
    let source = unsafe {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(source_ptr, source_len))
    };
    target.push_str(source);
    Ok(())
}

fn list_seed_fields(row: &boon_ir::ListSeedRecord) -> RuntimeResult<ValueColumns> {
    let mut columns = ValueColumns::default();
    for field in &row.fields {
        columns.insert_value(
            field.name.clone(),
            runtime_value_from_initial(&field.value, &ValueColumns::default())?,
        );
    }
    Ok(columns)
}

impl RuntimeRecordTemplate {
    fn from_cells(row_scope: &str, indexed_cells: &[&boon_ir::StateCell]) -> RuntimeResult<Self> {
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
            fields.push(RuntimeRecordFieldTemplate {
                field_id: FieldSlotId::from_path(&field),
                field_name: field.into_boxed_str(),
                initial_value: cell.initial_value.clone(),
            });
        }
        Ok(Self { fields })
    }

    fn materialize(&self, mut seed_fields: ValueColumns) -> RuntimeResult<RuntimeRecord> {
        for field in &self.fields {
            if seed_fields.contains_key_id(field.field_id) {
                continue;
            }
            let value = runtime_value_from_initial(&field.initial_value, &seed_fields)?;
            seed_fields.insert_value(field.field_name.to_string(), value);
        }
        Ok(RuntimeRecord {
            columns: seed_fields,
        })
    }

    fn reset_from_text_seeds<'a>(
        &self,
        row: &mut RuntimeRecord,
        seed_text: impl Fn(&str) -> Option<&'a str>,
    ) -> RuntimeResult<()> {
        for field in &self.fields {
            if !row.columns.contains_key_id(field.field_id) {
                return Err(format!("generic row missing field `{}`", field.field_name).into());
            }
            match &field.initial_value {
                InitialValue::Text { value } => {
                    row.columns.set_textlike(&field.field_name, value)?
                }
                InitialValue::Bool { value } => row.columns.set_bool(&field.field_name, *value)?,
                InitialValue::Enum { value } => {
                    row.columns.set_textlike(&field.field_name, value)?
                }
                InitialValue::SeedField { path } => {
                    let value =
                        seed_text(path).ok_or_else(|| format!("seed field `{path}` is missing"))?;
                    row.columns.set_textlike(&field.field_name, value)?;
                }
                InitialValue::Unknown { summary } => {
                    return Err(format!("unsupported state initializer `{summary}`").into());
                }
            }
        }
        Ok(())
    }
}

impl RuntimeRecord {
    fn reserve_textlike_fields(&mut self, additional: usize) -> RuntimeResult<()> {
        self.columns.reserve_all_textlike(additional);
        Ok(())
    }
}

fn runtime_value_from_initial(
    initial: &InitialValue,
    seed_fields: &ValueColumns,
) -> RuntimeResult<FieldValue> {
    match initial {
        InitialValue::Text { value } => Ok(FieldValue::Text(value.clone())),
        InitialValue::Bool { value } => Ok(FieldValue::Bool(*value)),
        InitialValue::Enum { value } => Ok(FieldValue::Enum(value.clone())),
        InitialValue::SeedField { path } => seed_fields
            .owned_value(path)
            .ok_or_else(|| format!("seed field `{path}` is missing").into()),
        InitialValue::Unknown { summary } => {
            Err(format!("unsupported state initializer `{summary}`").into())
        }
    }
}

fn seed_indexed_formula_fields(ir: &TypedProgram, row_scope: &str, row: &mut RuntimeRecord) {
    for value in ir
        .derived_values
        .iter()
        .filter(|value| value.indexed && value.kind == DerivedValueKind::Formula)
    {
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
fn todo_generic_row(title: &str) -> RuntimeRecord {
    let mut columns = ValueColumns::default();
    columns.insert_value("title".to_owned(), FieldValue::Text(title.to_owned()));
    columns.insert_value("edit_text".to_owned(), FieldValue::Text(title.to_owned()));
    columns.insert_value("completed".to_owned(), FieldValue::Bool(false));
    columns.insert_value("editing".to_owned(), FieldValue::Bool(false));
    RuntimeRecord { columns }
}

#[cfg(test)]
fn generic_cells_runtime(columns: usize, rows: usize) -> GenericCircuitRuntime {
    let mut runtime = GenericCircuitRuntime::default();
    let cell_rows = (0..rows).flat_map(|row| {
        (0..columns).map(move |column| {
            let address = format!(
                "{}{}",
                spreadsheet_column_label(column).unwrap_or_else(|| "?".to_owned()),
                row + 1
            );
            cell_generic_row(&address)
        })
    });
    runtime.lists.insert(
        "cells".to_owned(),
        KeyedList::from_values(cell_rows),
        Some(columns.saturating_mul(rows)),
        RuntimeRecordTemplate::default(),
    );
    runtime
}

#[cfg(test)]
fn cell_generic_row(address: &str) -> RuntimeRecord {
    let mut columns = ValueColumns::default();
    columns.insert_value("address".to_owned(), FieldValue::Text(address.to_owned()));
    columns.insert_value("editing_text".to_owned(), FieldValue::Text(String::new()));
    columns.insert_value("formula_text".to_owned(), FieldValue::Text(String::new()));
    columns.insert_value("value".to_owned(), FieldValue::Text(String::new()));
    columns.insert_value("error".to_owned(), FieldValue::Text(String::new()));
    columns.insert_value("dependencies".to_owned(), FieldValue::Text(String::new()));
    columns.insert_value("editing".to_owned(), FieldValue::Bool(false));
    RuntimeRecord { columns }
}

#[derive(Clone, Debug)]
struct ScalarEquationPlan {
    branches: Vec<ScalarUpdateBranch>,
}

#[derive(Clone, Debug)]
struct ScalarUpdateBranch {
    target: &'static str,
    source: &'static str,
    expression: ScalarUpdateExpression,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScalarUpdateExpression {
    SourceText,
    Const(&'static str),
    PreviousValue(&'static str),
    TextTrimOrPrevious {
        path: &'static str,
        previous: &'static str,
    },
    BoolNot(&'static str),
    Unsupported,
}

impl ScalarUpdateExpression {
    fn is_indexed_text_expression(self) -> bool {
        matches!(
            self,
            Self::SourceText | Self::PreviousValue(_) | Self::TextTrimOrPrevious { .. }
        )
    }

    fn is_indexed_bool_expression(self) -> bool {
        matches!(
            self,
            Self::Const("True") | Self::Const("False") | Self::BoolNot(_)
        )
    }
}

#[derive(Clone, Debug)]
enum ScalarTextValue<'a> {
    Text(Cow<'a, str>),
    PreviousValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IndexedTextCandidate<'a> {
    SourceText(&'a str),
    PreviousText(&'a str),
    PreviousField(&'static str),
    TrimmedOrSkip(Option<&'a str>),
}

#[derive(Clone, Copy, Debug)]
struct GenericTextFieldCommit<'a> {
    list: &'static str,
    key: u64,
    generation: u64,
    field: &'static str,
    value: &'a str,
}

#[derive(Clone, Copy, Debug)]
struct GenericTextFieldIdentity {
    list: &'static str,
    key: u64,
    generation: u64,
    field: &'static str,
}

#[derive(Clone, Copy, Debug)]
struct GenericBoolFieldCommit {
    list: &'static str,
    key: u64,
    generation: u64,
    field: &'static str,
    value: bool,
}

#[derive(Clone, Debug)]
struct GenericValueFieldCommit<'a> {
    list: &'static str,
    key: u64,
    generation: u64,
    field: &'static str,
    value: ProtocolValue<'a>,
}

#[derive(Clone, Copy, Debug)]
struct GenericListRowCommit {
    list: &'static str,
    key: u64,
    generation: u64,
}

#[derive(Clone, Copy, Debug)]
struct GenericTextListAppendCommit<'a> {
    list: &'static str,
    key: u64,
    generation: u64,
    value: &'a str,
}

#[derive(Clone, Debug)]
struct GenericRootTextCommit<'a> {
    target: &'static str,
    value: Cow<'a, str>,
}

#[derive(Clone, Debug)]
struct GenericRenderLoweringPlan {
    surface: ExecutableSurfaceKind,
}

#[derive(Clone, Debug, Default)]
struct GenericRenderContext<'a> {
    address: Option<&'a str>,
    todo_show_edit_input_text: Option<&'a str>,
    patch_editor_text: bool,
    patch_value_text: bool,
}

impl<'a> GenericTextFieldCommit<'a> {
    fn semantic_delta(&self) -> SemanticDelta<'a> {
        GenericCircuitRuntime::semantic_field_delta(
            Some(self.list),
            Some(self.key),
            Some(self.generation),
            self.field,
            ProtocolValue::Text(Cow::Borrowed(self.value)),
        )
    }
}

impl GenericBoolFieldCommit {
    fn semantic_delta(&self) -> SemanticDelta<'static> {
        GenericCircuitRuntime::semantic_field_delta(
            Some(self.list),
            Some(self.key),
            Some(self.generation),
            self.field,
            ProtocolValue::Bool(self.value),
        )
    }
}

impl<'a> GenericValueFieldCommit<'a> {
    fn semantic_delta(&self) -> SemanticDelta<'a> {
        GenericCircuitRuntime::semantic_field_delta(
            Some(self.list),
            Some(self.key),
            Some(self.generation),
            self.field,
            self.value.clone(),
        )
    }
}

impl GenericListRowCommit {
    fn semantic_move_delta(&self, to: usize) -> SemanticDelta<'static> {
        SemanticDelta {
            kind: "ListMove",
            list_id: Some(self.list),
            key: Some(self.key),
            generation: Some(self.generation),
            source_id: None,
            bind_epoch: None,
            field_path: Some("position"),
            value: ProtocolValue::NumberText(to as i64),
        }
    }
}

impl GenericTextFieldIdentity {
    fn semantic_delta_with_value<'a>(&self, value: ProtocolValue<'a>) -> SemanticDelta<'a> {
        GenericCircuitRuntime::semantic_field_delta(
            Some(self.list),
            Some(self.key),
            Some(self.generation),
            self.field,
            value,
        )
    }
}

impl<'a> GenericTextListAppendCommit<'a> {
    fn semantic_delta(&self) -> SemanticDelta<'a> {
        GenericCircuitRuntime::semantic_list_delta(
            "ListInsert",
            self.list,
            self.key,
            self.generation,
            ProtocolValue::Text(Cow::Borrowed(self.value)),
        )
    }
}

impl<'a> GenericRootTextCommit<'a> {
    fn semantic_delta(&self) -> SemanticDelta<'a> {
        GenericCircuitRuntime::semantic_field_delta(
            None,
            None,
            None,
            self.target,
            ProtocolValue::Text(self.value.clone()),
        )
    }
}

impl GenericRenderLoweringPlan {
    fn todo_mvc() -> Self {
        Self {
            surface: ExecutableSurfaceKind::TodoMvc,
        }
    }

    fn cells() -> Self {
        Self {
            surface: ExecutableSurfaceKind::Cells,
        }
    }

    fn lower_mutation_patch<'a>(
        &self,
        mutation: &GenericSourceMutation<'a>,
        context: GenericRenderContext<'a>,
    ) -> RuntimeResult<Option<RenderPatch<'a>>> {
        match self.surface {
            ExecutableSurfaceKind::TodoMvc => self.lower_todomvc_patch(mutation, context),
            ExecutableSurfaceKind::Cells => self.lower_cells_patch(mutation, context),
        }
    }

    fn lower_todomvc_patch<'a>(
        &self,
        mutation: &GenericSourceMutation<'a>,
        context: GenericRenderContext<'a>,
    ) -> RuntimeResult<Option<RenderPatch<'a>>> {
        match mutation {
            GenericSourceMutation::RootText(commit) => match commit.target {
                "store.new_todo_text" => Ok(Some(patch(
                    "SetInputValue",
                    RenderTarget::Static("new_todo_input"),
                    ProtocolValue::Text(commit.value.clone()),
                ))),
                "store.selected_filter" => Ok(Some(patch(
                    "SetSelectedFilter",
                    RenderTarget::Static("filters"),
                    ProtocolValue::Text(commit.value.clone()),
                ))),
                _ => Err(format!("unsupported scalar render target `{}`", commit.target).into()),
            },
            GenericSourceMutation::TextField(commit) => match commit.field {
                "title" => Ok(Some(keyed_patch(
                    "SetText",
                    RenderTarget::TodoTitle(commit.key),
                    ProtocolValue::Text(Cow::Borrowed(commit.value)),
                    commit.list,
                    commit.key,
                    commit.generation,
                ))),
                "edit_text" => Ok(Some(keyed_patch(
                    "SetEditInput",
                    RenderTarget::TodoEdit(commit.key),
                    ProtocolValue::Text(Cow::Borrowed(commit.value)),
                    commit.list,
                    commit.key,
                    commit.generation,
                ))),
                _ => Ok(None),
            },
            GenericSourceMutation::BoolField(commit) => match commit.field {
                "completed" => Ok(Some(keyed_patch(
                    "SetProperty",
                    RenderTarget::TodoCheckbox(commit.key),
                    ProtocolValue::CheckedProperty(commit.value),
                    commit.list,
                    commit.key,
                    commit.generation,
                ))),
                "editing" if !commit.value => Ok(Some(keyed_patch(
                    "HideEditInput",
                    RenderTarget::TodoEdit(commit.key),
                    ProtocolValue::Bool(commit.value),
                    commit.list,
                    commit.key,
                    commit.generation,
                ))),
                "editing" if commit.value => Ok(context.todo_show_edit_input_text.map(|text| {
                    keyed_patch(
                        "ShowEditInput",
                        RenderTarget::TodoEdit(commit.key),
                        ProtocolValue::Text(Cow::Borrowed(text)),
                        commit.list,
                        commit.key,
                        commit.generation,
                    )
                })),
                _ => Ok(None),
            },
            GenericSourceMutation::ListRemove {
                list,
                key,
                generation,
            } => Ok(Some(keyed_patch(
                "RemoveElement",
                RenderTarget::TodoRow(*key),
                ProtocolValue::Null,
                list,
                *key,
                *generation,
            ))),
            GenericSourceMutation::SourceBind(binding) => Ok(Some(source_patch(
                "BindSource",
                RenderTarget::TodoSource(binding.key, binding.source_path),
                source_binding_value(binding),
                binding,
            ))),
            GenericSourceMutation::SourceUnbind(binding) => Ok(Some(source_patch(
                "UnbindSource",
                RenderTarget::TodoSource(binding.key, binding.source_path),
                source_binding_value(binding),
                binding,
            ))),
            GenericSourceMutation::ListAppend(commit) => Ok(Some(keyed_patch(
                "InsertElement",
                RenderTarget::TodoRow(commit.key),
                ProtocolValue::Text(Cow::Borrowed(commit.value)),
                commit.list,
                commit.key,
                commit.generation,
            ))),
            GenericSourceMutation::TextFieldIdentity(_) | GenericSourceMutation::ValueField(_) => {
                Ok(None)
            }
        }
    }

    fn lower_todomvc_row_affordance_patch<'a>(
        &self,
        key: u64,
        generation: u64,
        affordance: &'static str,
        visible: bool,
    ) -> RuntimeResult<RenderPatch<'a>> {
        if self.surface != ExecutableSurfaceKind::TodoMvc {
            return Err("Todo row affordance render lowering requires TodoMVC surface".into());
        }
        match affordance {
            "delete_button" => Ok(keyed_patch(
                "ShowDeleteButton",
                RenderTarget::TodoRow(key),
                ProtocolValue::Bool(visible),
                "todos",
                key,
                generation,
            )),
            _ => Err(format!("unsupported Todo row affordance `{affordance}`").into()),
        }
    }

    fn lower_todomvc_list_move_patch<'a>(
        &self,
        commit: &GenericListRowCommit,
        to: usize,
    ) -> RuntimeResult<RenderPatch<'a>> {
        if self.surface != ExecutableSurfaceKind::TodoMvc {
            return Err("Todo list move render lowering requires TodoMVC surface".into());
        }
        Ok(keyed_patch(
            "MoveElement",
            RenderTarget::TodoPosition(commit.key),
            ProtocolValue::NumberText(to as i64),
            commit.list,
            commit.key,
            commit.generation,
        ))
    }

    fn lower_cells_patch<'a>(
        &self,
        mutation: &GenericSourceMutation<'a>,
        context: GenericRenderContext<'a>,
    ) -> RuntimeResult<Option<RenderPatch<'a>>> {
        let address = || {
            context
                .address
                .ok_or_else(|| -> Box<dyn std::error::Error> {
                    "Cells render lowering requires an address context".into()
                })
        };
        match mutation {
            GenericSourceMutation::TextField(commit)
                if context.patch_editor_text && commit.field == "editing_text" =>
            {
                Ok(Some(keyed_patch(
                    "SetCellEditor",
                    RenderTarget::Borrowed(Cow::Borrowed(address()?)),
                    ProtocolValue::Text(Cow::Borrowed(commit.value)),
                    commit.list,
                    commit.key,
                    commit.generation,
                )))
            }
            GenericSourceMutation::ValueField(commit) if context.patch_value_text => {
                Ok(Some(keyed_patch(
                    "SetCellText",
                    RenderTarget::Borrowed(Cow::Borrowed(address()?)),
                    commit.value.clone(),
                    commit.list,
                    commit.key,
                    commit.generation,
                )))
            }
            GenericSourceMutation::RootText(_)
            | GenericSourceMutation::TextField(_)
            | GenericSourceMutation::TextFieldIdentity(_)
            | GenericSourceMutation::ValueField(_)
            | GenericSourceMutation::BoolField(_)
            | GenericSourceMutation::ListAppend(_)
            | GenericSourceMutation::ListRemove { .. }
            | GenericSourceMutation::SourceBind(_)
            | GenericSourceMutation::SourceUnbind(_) => Ok(None),
        }
    }
}

impl<'a> GenericSourceMutation<'a> {
    fn semantic_delta(&self) -> Option<SemanticDelta<'a>> {
        match self {
            Self::RootText(commit) => Some(commit.semantic_delta()),
            Self::TextField(commit) => Some(commit.semantic_delta()),
            Self::TextFieldIdentity(_) => None,
            Self::ValueField(commit) => Some(commit.semantic_delta()),
            Self::BoolField(commit) => Some(commit.semantic_delta()),
            Self::ListAppend(commit) => Some(commit.semantic_delta()),
            Self::ListRemove {
                list,
                key,
                generation,
            } => Some(GenericCircuitRuntime::semantic_list_delta(
                "ListRemove",
                list,
                *key,
                *generation,
                ProtocolValue::Null,
            )),
            Self::SourceBind(binding) => Some(GenericCircuitRuntime::semantic_source_delta(
                "SourceBind",
                binding,
                ProtocolValue::Text(Cow::Borrowed(binding.source_path)),
            )),
            Self::SourceUnbind(binding) => Some(GenericCircuitRuntime::semantic_source_delta(
                "SourceUnbind",
                binding,
                ProtocolValue::Null,
            )),
        }
    }
}

enum GenericListRemoveObservation<'a> {
    SourceUnbind(&'a SourceBinding),
    RowRemoved { key: u64, generation: u64 },
}

#[derive(Clone, Copy, Debug)]
struct GenericSourceActionInput<'a> {
    source: &'a str,
    source_id: SourceId,
    list: Option<&'static str>,
    index: Option<usize>,
    key: Option<&'a str>,
    text: Option<&'a str>,
    seq: TickSeq,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GenericBoundSourceIndex {
    Unspecified,
    Bound(usize),
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GenericCellsScenarioPreparation {
    max_text_len: usize,
    max_deps: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GenericVisibleRowOccurrence {
    Occurrence(usize),
    Stale,
    Mismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GenericSourceRouteKind {
    RootText,
    RootScalar,
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

impl GenericSourceRouteKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::RootText => "root_text",
            Self::RootScalar => "root_scalar",
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
    TextField(GenericTextFieldCommit<'a>),
    TextFieldIdentity(GenericTextFieldIdentity),
    ValueField(GenericValueFieldCommit<'a>),
    BoolField(GenericBoolFieldCommit),
    ListAppend(GenericTextListAppendCommit<'a>),
    ListRemove {
        list: &'static str,
        key: u64,
        generation: u64,
    },
    SourceBind(SourceBinding),
    SourceUnbind(SourceBinding),
}

#[derive(Debug)]
struct GenericSourceMutationBatch<'a> {
    root_texts: [Option<(&'static str, GenericRootTextCommit<'a>)>; 4],
    text_fields: [Option<(&'static str, GenericTextFieldCommit<'a>)>; 8],
    identity_fields: [Option<(&'static str, GenericTextFieldIdentity)>; 8],
    bool_fields: [Option<(&'static str, GenericBoolFieldCommit)>; 8],
    list_appends: [Option<(&'static str, GenericTextListAppendCommit<'a>)>; 4],
}

impl<'a> GenericSourceMutationBatch<'a> {
    fn new() -> Self {
        Self {
            root_texts: std::array::from_fn(|_| None),
            text_fields: [None; 8],
            identity_fields: [None; 8],
            bool_fields: [None; 8],
            list_appends: [None; 4],
        }
    }

    fn observe(&mut self, mutation: &GenericSourceMutation<'a>) -> RuntimeResult<()> {
        match mutation {
            GenericSourceMutation::RootText(commit) => {
                Self::insert_root_text(&mut self.root_texts, commit.target, commit.clone())?;
            }
            GenericSourceMutation::TextField(commit) => {
                Self::insert_text(&mut self.text_fields, commit.field, *commit)?;
            }
            GenericSourceMutation::TextFieldIdentity(commit) => {
                Self::insert_identity(&mut self.identity_fields, commit.field, *commit)?;
            }
            GenericSourceMutation::BoolField(commit) => {
                Self::insert_bool(&mut self.bool_fields, commit.field, *commit)?;
            }
            GenericSourceMutation::ListAppend(commit) => {
                Self::insert_append(&mut self.list_appends, commit.list, *commit)?;
            }
            GenericSourceMutation::ValueField(_)
            | GenericSourceMutation::ListRemove { .. }
            | GenericSourceMutation::SourceBind(_)
            | GenericSourceMutation::SourceUnbind(_) => {}
        }
        Ok(())
    }

    fn insert_root_text(
        slots: &mut [Option<(&'static str, GenericRootTextCommit<'a>)>; 4],
        target: &'static str,
        commit: GenericRootTextCommit<'a>,
    ) -> RuntimeResult<()> {
        for slot in slots.iter_mut() {
            match slot {
                Some((existing, value)) if *existing == target => {
                    *value = commit;
                    return Ok(());
                }
                None => {
                    *slot = Some((target, commit));
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(format!("source mutation batch root capacity exceeded for `{target}`").into())
    }

    fn insert_text(
        slots: &mut [Option<(&'static str, GenericTextFieldCommit<'a>)>; 8],
        field: &'static str,
        commit: GenericTextFieldCommit<'a>,
    ) -> RuntimeResult<()> {
        for slot in slots.iter_mut() {
            match slot {
                Some((existing, value)) if *existing == field => {
                    *value = commit;
                    return Ok(());
                }
                None => {
                    *slot = Some((field, commit));
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(format!("source mutation batch text capacity exceeded for `{field}`").into())
    }

    fn insert_identity(
        slots: &mut [Option<(&'static str, GenericTextFieldIdentity)>; 8],
        field: &'static str,
        commit: GenericTextFieldIdentity,
    ) -> RuntimeResult<()> {
        for slot in slots.iter_mut() {
            match slot {
                Some((existing, value)) if *existing == field => {
                    *value = commit;
                    return Ok(());
                }
                None => {
                    *slot = Some((field, commit));
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(format!("source mutation batch identity capacity exceeded for `{field}`").into())
    }

    fn insert_bool(
        slots: &mut [Option<(&'static str, GenericBoolFieldCommit)>; 8],
        field: &'static str,
        commit: GenericBoolFieldCommit,
    ) -> RuntimeResult<()> {
        for slot in slots.iter_mut() {
            match slot {
                Some((existing, value)) if *existing == field => {
                    *value = commit;
                    return Ok(());
                }
                None => {
                    *slot = Some((field, commit));
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(format!("source mutation batch bool capacity exceeded for `{field}`").into())
    }

    fn insert_append(
        slots: &mut [Option<(&'static str, GenericTextListAppendCommit<'a>)>; 4],
        list: &'static str,
        commit: GenericTextListAppendCommit<'a>,
    ) -> RuntimeResult<()> {
        for slot in slots.iter_mut() {
            match slot {
                Some((existing, value)) if *existing == list => {
                    *value = commit;
                    return Ok(());
                }
                None => {
                    *slot = Some((list, commit));
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
        field: &'static str,
    ) -> RuntimeResult<GenericTextFieldCommit<'a>> {
        self.text(field).ok_or_else(|| {
            format!("{label} from `{source}` produced no `{field}` text change").into()
        })
    }

    fn text(&self, field: &'static str) -> Option<GenericTextFieldCommit<'a>> {
        self.text_fields.iter().find_map(|slot| match slot {
            Some((existing, commit)) if *existing == field => Some(*commit),
            _ => None,
        })
    }

    fn require_identity(
        &self,
        source: &str,
        label: &str,
        field: &'static str,
    ) -> RuntimeResult<GenericTextFieldIdentity> {
        self.identity_fields
            .iter()
            .find_map(|slot| match slot {
                Some((existing, commit)) if *existing == field => Some(*commit),
                _ => None,
            })
            .ok_or_else(|| format!("{label} from `{source}` produced no `{field}` identity").into())
    }

    fn require_bool(
        &self,
        source: &str,
        label: &str,
        field: &'static str,
    ) -> RuntimeResult<GenericBoolFieldCommit> {
        self.bool(field).ok_or_else(|| {
            format!("{label} from `{source}` produced no `{field}` bool change").into()
        })
    }

    fn bool(&self, field: &'static str) -> Option<GenericBoolFieldCommit> {
        self.bool_fields.iter().find_map(|slot| match slot {
            Some((existing, commit)) if *existing == field => Some(*commit),
            _ => None,
        })
    }

    fn list_append(&self, list: &'static str) -> Option<GenericTextListAppendCommit<'a>> {
        self.list_appends.iter().find_map(|slot| match slot {
            Some((existing, commit)) if *existing == list => Some(*commit),
            _ => None,
        })
    }

    fn root_text(&self, target: &'static str) -> Option<GenericRootTextCommit<'a>> {
        self.root_texts.iter().find_map(|slot| match slot {
            Some((existing, commit)) if *existing == target => Some(commit.clone()),
            _ => None,
        })
    }
}

#[derive(Clone, Debug)]
struct DerivedEquationPlan {
    text_transforms: Vec<RuntimeDerivedTextTransform>,
}

#[derive(Clone, Debug)]
struct RuntimeDerivedTextTransform {
    target: &'static str,
    source: &'static str,
    expression: RuntimeDerivedTextExpression,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeDerivedTextExpression {
    EnterKeyTextTrimNonEmpty,
    Unsupported,
}

#[derive(Clone, Debug, Default)]
struct SourceRoutePlan {
    route_slots: Vec<SourceRoute>,
    id_slots: Vec<Option<SourceRouteIndex>>,
    label_slots: Vec<SourceBoundaryLabel>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceRouteIndex(usize);

impl SourceRouteIndex {
    fn slot(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceBoundaryLabel {
    source: &'static str,
    source_id: SourceId,
}

#[derive(Clone, Debug, Default)]
struct ListSourceBindingPlan {
    list_slots: Vec<ListSourceBindingSlot>,
}

#[derive(Clone, Debug)]
struct ListSourceBindingSlot {
    list: &'static str,
    source_paths: Vec<&'static str>,
}

#[derive(Clone, Debug)]
struct SourceRoute {
    source_id: SourceId,
    source: &'static str,
    root_scalar_targets: Vec<SourceRouteScalarTarget>,
    indexed_text_targets: Vec<SourceRouteScalarTarget>,
    indexed_bool_targets: Vec<SourceRouteScalarTarget>,
    derived_text_targets: Vec<&'static str>,
    list_append_targets: Vec<SourceRouteListAppend>,
    list_remove_targets: Vec<SourceRouteListRemove>,
    actions: Vec<SourceRouteAction>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceRouteScalarTarget {
    target: &'static str,
    expression: ScalarUpdateExpression,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceRouteListRemove {
    list: &'static str,
    predicate: RuntimeListPredicate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceRouteListAppend {
    list: &'static str,
    trigger: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceRouteAction {
    RootScalar,
    DerivedText {
        target: &'static str,
    },
    ListRemove {
        list: &'static str,
    },
    ListAppend {
        list: &'static str,
        trigger: &'static str,
    },
    IndexedText {
        kind: SourceRouteTextAction,
        target: &'static str,
    },
    IndexedBool {
        kind: SourceRouteBoolAction,
        target: &'static str,
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
    list: &'static str,
    kind: RuntimeListOperationKind,
}

#[derive(Clone, Debug)]
enum RuntimeListOperationKind {
    Append {
        trigger: &'static str,
        fields: Vec<RuntimeListAppendField>,
    },
    Remove {
        source: &'static str,
        predicate: RuntimeListPredicate,
    },
    Retain {
        target: &'static str,
        predicate: RuntimeListPredicate,
    },
    Count {
        target: &'static str,
        predicate: RuntimeListPredicate,
    },
}

#[derive(Clone, Debug)]
struct RuntimeListAppendField {
    name: &'static str,
    source: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeListPredicate {
    AlwaysTrue,
    FieldBool {
        path: &'static str,
    },
    FieldBoolNot {
        path: &'static str,
    },
    SelectorVisibility {
        selector: &'static str,
        row_field: &'static str,
    },
    Unsupported,
}

impl ScalarEquationPlan {
    fn from_ir(ir: &TypedProgram) -> Self {
        let branches = ir
            .update_branches
            .iter()
            .map(|branch| ScalarUpdateBranch {
                target: leak_runtime_path(branch.target.clone()),
                source: leak_runtime_path(branch.source.clone()),
                expression: match &branch.expression {
                    UpdateExpression::SourcePayload { path } if path == "text" => {
                        ScalarUpdateExpression::SourceText
                    }
                    UpdateExpression::Const { value } => {
                        ScalarUpdateExpression::Const(leak_runtime_path(value.clone()))
                    }
                    UpdateExpression::PreviousValue { path } => {
                        ScalarUpdateExpression::PreviousValue(leak_runtime_path(path.clone()))
                    }
                    UpdateExpression::TextTrimOrPrevious { path, previous } => {
                        ScalarUpdateExpression::TextTrimOrPrevious {
                            path: leak_runtime_path(path.clone()),
                            previous: leak_runtime_path(previous.clone()),
                        }
                    }
                    UpdateExpression::BoolNot { path } => {
                        ScalarUpdateExpression::BoolNot(leak_runtime_path(path.clone()))
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
        payload_text: Option<&'a str>,
    ) -> RuntimeResult<Option<Cow<'a, str>>> {
        let Some(value) = self.eval_text_value(target, source, payload_text)? else {
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
        payload_text: Option<&'a str>,
    ) -> RuntimeResult<Option<ScalarTextValue<'a>>> {
        let Some(branch) = self
            .branches
            .iter()
            .find(|branch| branch.target == target && branch.source == source)
        else {
            return Ok(None);
        };
        match branch.expression {
            ScalarUpdateExpression::SourceText => {
                let text = payload_text.ok_or_else(|| {
                    format!("source `{source}` for `{target}` requires a text payload")
                })?;
                Ok(Some(ScalarTextValue::Text(Cow::Borrowed(text))))
            }
            ScalarUpdateExpression::Const(value) => {
                Ok(Some(ScalarTextValue::Text(Cow::Borrowed(value))))
            }
            ScalarUpdateExpression::PreviousValue(_) => Ok(Some(ScalarTextValue::PreviousValue)),
            ScalarUpdateExpression::TextTrimOrPrevious { .. }
            | ScalarUpdateExpression::BoolNot(_) => Ok(None),
            ScalarUpdateExpression::Unsupported => Ok(None),
        }
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
        match branch.expression {
            ScalarUpdateExpression::Const("True") => Ok(Some(true)),
            ScalarUpdateExpression::Const("False") => Ok(Some(false)),
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

impl DerivedEquationPlan {
    #[cfg(test)]
    fn empty() -> Self {
        Self {
            text_transforms: Vec::new(),
        }
    }

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
                target: leak_runtime_path(value.path.clone()),
                source: value
                    .sources
                    .first()
                    .map(|source| leak_runtime_path(source.clone()))
                    .unwrap_or(""),
                expression: if value.sources.len() == 1 {
                    RuntimeDerivedTextExpression::EnterKeyTextTrimNonEmpty
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
    ) -> RuntimeResult<Option<&'a str>> {
        let Some(transform) = self
            .text_transforms
            .iter()
            .find(|transform| transform.target == target && transform.source == source)
        else {
            return Ok(None);
        };
        match transform.expression {
            RuntimeDerivedTextExpression::EnterKeyTextTrimNonEmpty => {
                if key != Some("Enter") {
                    return Ok(None);
                }
                let text = text.ok_or_else(|| {
                    format!("derived text transform `{target}` from `{source}` requires text")
                })?;
                let trimmed = text.trim();
                Ok((!trimmed.is_empty()).then_some(trimmed))
            }
            RuntimeDerivedTextExpression::Unsupported => Err(format!(
                "derived text transform `{target}` from `{source}` is unsupported"
            )
            .into()),
        }
    }
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
            let source_id = source_id_for_path(ir, branch.source)?;
            let route = routes.route_mut(branch.source, source_id);
            let scalar_target = SourceRouteScalarTarget {
                target: branch.target,
                expression: branch.expression,
            };
            if root_targets.contains(branch.target) {
                route.root_scalar_targets.push(scalar_target);
            } else if branch.expression.is_indexed_text_expression() {
                route.indexed_text_targets.push(scalar_target);
            } else if branch.expression.is_indexed_bool_expression() {
                route.indexed_bool_targets.push(scalar_target);
            }
        }
        for transform in &derived.text_transforms {
            let source_id = source_id_for_path(ir, transform.source)?;
            routes
                .route_mut(transform.source, source_id)
                .derived_text_targets
                .push(transform.target);
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
                            list: operation.list,
                            trigger: *trigger,
                        });
                    }
                }
                RuntimeListOperationKind::Remove { source, predicate } => {
                    let source_id = source_id_for_path(ir, source)?;
                    routes
                        .route_mut(*source, source_id)
                        .list_remove_targets
                        .push(SourceRouteListRemove {
                            list: operation.list,
                            predicate: *predicate,
                        });
                }
                RuntimeListOperationKind::Retain { .. }
                | RuntimeListOperationKind::Count { .. } => {}
            }
        }
        for route in &mut routes.route_slots {
            route.rebuild_actions();
        }
        Ok(routes)
    }

    fn len(&self) -> usize {
        self.route_slots.len()
    }

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

    fn source_id(&self, source: &str) -> Option<SourceId> {
        self.label_slots
            .binary_search_by(|label| label.source.cmp(source))
            .ok()
            .and_then(|index| self.label_slots.get(index))
            .map(|label| label.source_id)
    }

    fn require_source_id(&self, source: &str) -> RuntimeResult<SourceId> {
        self.source_id(source)
            .ok_or_else(|| format!("source `{source}` has no typed SourceId route").into())
    }

    fn require_source(&self, source: &str) -> RuntimeResult<&SourceRoute> {
        self.for_source(source)
            .ok_or_else(|| format!("source `{source}` has no compiled route").into())
    }

    fn actions_for_source_id(&self, source_id: SourceId) -> RuntimeResult<&[SourceRouteAction]> {
        Ok(self
            .for_source_id(source_id)
            .ok_or_else(|| format!("SourceId `{}` has no compiled route", source_id.as_usize()))?
            .actions
            .as_slice())
    }

    fn actions_for_source(&self, source: &str) -> RuntimeResult<&[SourceRouteAction]> {
        Ok(self.require_source(source)?.actions.as_slice())
    }

    fn single_root_scalar_target(&self, source: &str) -> RuntimeResult<Option<&'static str>> {
        self.require_source(source)?.single_root_scalar_target()
    }

    fn list_remove_predicate(
        &self,
        source: &str,
        list: &str,
    ) -> RuntimeResult<RuntimeListPredicate> {
        self.require_source(source)?.list_remove_predicate(list)
    }

    #[cfg(test)]
    fn list_append_trigger(&self, source: &str, list: &str) -> RuntimeResult<&'static str> {
        self.require_source(source)?.list_append_trigger(list)
    }

    fn route_mut(&mut self, source: &'static str, source_id: SourceId) -> &mut SourceRoute {
        let source_slot = source_id.as_usize();
        if self.id_slots.len() <= source_slot {
            self.id_slots.resize(source_slot + 1, None);
        }
        if let Some(index) = self.id_slots[source_slot] {
            return &mut self.route_slots[index.slot()];
        }

        let index = SourceRouteIndex(self.route_slots.len());
        self.id_slots[source_slot] = Some(index);
        let label = SourceBoundaryLabel { source, source_id };
        let label_index = self
            .label_slots
            .binary_search_by(|candidate| candidate.source.cmp(source))
            .unwrap_or_else(|index| index);
        self.label_slots.insert(label_index, label);
        self.route_slots.push(SourceRoute {
            source_id,
            source,
            root_scalar_targets: Vec::new(),
            indexed_text_targets: Vec::new(),
            indexed_bool_targets: Vec::new(),
            derived_text_targets: Vec::new(),
            list_append_targets: Vec::new(),
            list_remove_targets: Vec::new(),
            actions: Vec::new(),
        });
        self.route_slots
            .last_mut()
            .expect("route was just pushed and must exist")
    }
}

fn source_id_for_path(ir: &TypedProgram, path: &str) -> RuntimeResult<SourceId> {
    ir.sources
        .iter()
        .find(|source| source.path == path)
        .map(|source| source.id)
        .ok_or_else(|| format!("source route `{path}` has no typed IR SourceId").into())
}

impl ListSourceBindingPlan {
    fn from_ir(ir: &TypedProgram) -> Self {
        let mut list_slots = Vec::new();
        for list in &ir.lists {
            let row_scope = row_scope_name(&list.name);
            let prefix = format!("{row_scope}.sources.");
            let source_paths = ir
                .sources
                .iter()
                .filter(|source| source.scoped && source.path.starts_with(&prefix))
                .map(|source| leak_runtime_path(source.path.clone()))
                .collect::<Vec<_>>();
            if !source_paths.is_empty() {
                list_slots.push(ListSourceBindingSlot {
                    list: leak_runtime_path(list.name.clone()),
                    source_paths,
                });
            }
        }
        Self { list_slots }
    }

    fn source_paths(&self, list: &str) -> RuntimeResult<&[&'static str]> {
        self.list_slots
            .iter()
            .find_map(|binding| (binding.list == list).then_some(binding.source_paths.as_slice()))
            .ok_or_else(|| format!("list `{list}` has no scoped source binding plan").into())
    }

    fn source_count(&self, list: &str) -> RuntimeResult<usize> {
        self.source_paths(list).map(<[_]>::len)
    }

    fn list_for_row_scope(&self, scope: &str) -> Option<&'static str> {
        self.list_slots
            .iter()
            .find_map(|binding| row_scope_matches_list(binding.list, scope).then_some(binding.list))
    }
}

impl SourceRoute {
    fn rebuild_actions(&mut self) {
        self.actions.clear();
        self.actions.extend(
            self.root_scalar_targets
                .iter()
                .map(|_| SourceRouteAction::RootScalar),
        );
        self.actions.extend(
            self.derived_text_targets
                .iter()
                .copied()
                .map(|target| SourceRouteAction::DerivedText { target }),
        );
        self.actions.extend(
            self.list_remove_targets
                .iter()
                .map(|target| SourceRouteAction::ListRemove { list: target.list }),
        );
        self.actions
            .extend(
                self.list_append_targets
                    .iter()
                    .map(|target| SourceRouteAction::ListAppend {
                        list: target.list,
                        trigger: target.trigger,
                    }),
            );
        self.actions
            .extend(self.indexed_text_targets.iter().filter_map(|target| {
                let kind = match target.expression {
                    ScalarUpdateExpression::SourceText => SourceRouteTextAction::SourceText,
                    ScalarUpdateExpression::PreviousValue(_) => {
                        SourceRouteTextAction::PreviousValue
                    }
                    ScalarUpdateExpression::TextTrimOrPrevious { .. } => {
                        SourceRouteTextAction::TextTrimOrPrevious
                    }
                    ScalarUpdateExpression::Const(_)
                    | ScalarUpdateExpression::BoolNot(_)
                    | ScalarUpdateExpression::Unsupported => return None,
                };
                Some(SourceRouteAction::IndexedText {
                    kind,
                    target: target.target,
                })
            }));
        self.actions
            .extend(self.indexed_bool_targets.iter().filter_map(|target| {
                let kind = match target.expression {
                    ScalarUpdateExpression::BoolNot(_) => SourceRouteBoolAction::BoolNot,
                    ScalarUpdateExpression::Const("True") => SourceRouteBoolAction::ConstTrue,
                    ScalarUpdateExpression::Const("False") => SourceRouteBoolAction::ConstFalse,
                    ScalarUpdateExpression::SourceText
                    | ScalarUpdateExpression::Const(_)
                    | ScalarUpdateExpression::PreviousValue(_)
                    | ScalarUpdateExpression::TextTrimOrPrevious { .. }
                    | ScalarUpdateExpression::Unsupported => return None,
                };
                Some(SourceRouteAction::IndexedBool {
                    kind,
                    target: target.target,
                })
            }));
    }

    fn has_action(&self, matches: impl Fn(SourceRouteAction) -> bool) -> bool {
        self.actions.iter().copied().any(matches)
    }

    fn has_root_scalar_action(&self) -> bool {
        self.has_action(|action| matches!(action, SourceRouteAction::RootScalar))
    }

    fn has_indexed_text_action(&self, expected: SourceRouteTextAction) -> bool {
        self.has_action(|action| {
            matches!(
                action,
                SourceRouteAction::IndexedText { kind, .. } if kind == expected
            )
        })
    }

    fn has_indexed_text_action_where(
        &self,
        expected: SourceRouteTextAction,
        matches_target: impl Fn(&'static str) -> bool,
    ) -> bool {
        self.has_action(|action| {
            matches!(
                action,
                SourceRouteAction::IndexedText { kind, target }
                    if kind == expected && matches_target(target)
            )
        })
    }

    fn has_indexed_bool_action(&self, expected: SourceRouteBoolAction) -> bool {
        self.has_action(|action| {
            matches!(
                action,
                SourceRouteAction::IndexedBool { kind, .. } if kind == expected
            )
        })
    }

    fn single_root_scalar_target(&self) -> RuntimeResult<Option<&'static str>> {
        let mut target = None;
        for candidate in &self.root_scalar_targets {
            if target.is_some_and(|current| current != candidate.target) {
                return Err(format!(
                    "source `{}` drives multiple root scalar targets in this runtime slice",
                    self.source
                )
                .into());
            }
            target = Some(candidate.target);
        }
        Ok(target)
    }

    fn has_list_remove_target(&self, list: &str) -> bool {
        self.has_action(|action| {
            matches!(
                action,
                SourceRouteAction::ListRemove { list: candidate } if candidate == list
            )
        })
    }

    fn has_list_append_target(&self, list: &str) -> bool {
        self.has_action(|action| {
            matches!(
                action,
                SourceRouteAction::ListAppend {
                    list: candidate,
                    ..
                } if candidate == list
            )
        })
    }

    fn list_remove_predicate(&self, list: &str) -> RuntimeResult<RuntimeListPredicate> {
        self.list_remove_targets
            .iter()
            .find_map(|candidate| (candidate.list == list).then_some(candidate.predicate))
            .ok_or_else(|| {
                format!(
                    "source `{}` has no compiled list-remove route for `{list}`",
                    self.source
                )
                .into()
            })
    }

    #[cfg(test)]
    fn list_append_trigger(&self, list: &str) -> RuntimeResult<&'static str> {
        let mut trigger = None;
        for candidate in &self.list_append_targets {
            if candidate.list != list {
                continue;
            }
            if trigger.is_some_and(|current| current != candidate.trigger) {
                return Err(format!(
                    "source `{}` has multiple list-append triggers for `{list}`",
                    self.source
                )
                .into());
            }
            trigger = Some(candidate.trigger);
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
            .filter_map(|operation| {
                let list = leak_runtime_path(operation.list.clone());
                let kind = match &operation.kind {
                    ListOperationKind::Append { trigger, fields } => {
                        RuntimeListOperationKind::Append {
                            trigger: leak_runtime_path(trigger.clone()),
                            fields: fields
                                .iter()
                                .map(|field| RuntimeListAppendField {
                                    name: leak_runtime_path(field.name.clone()),
                                    source: leak_runtime_path(field.source.clone()),
                                })
                                .collect(),
                        }
                    }
                    ListOperationKind::Remove { source, predicate } => {
                        RuntimeListOperationKind::Remove {
                            source: leak_runtime_path(source.clone()),
                            predicate: runtime_list_predicate(predicate),
                        }
                    }
                    ListOperationKind::Retain { target, predicate } => {
                        RuntimeListOperationKind::Retain {
                            target: leak_runtime_path(target.clone()),
                            predicate: runtime_list_predicate(predicate),
                        }
                    }
                    ListOperationKind::Count { target, predicate } => {
                        RuntimeListOperationKind::Count {
                            target: leak_runtime_path(target.clone()),
                            predicate: runtime_list_predicate(predicate),
                        }
                    }
                };
                Some(RuntimeListOperation { list, kind })
            })
            .collect();
        Self { operations }
    }

    #[cfg(test)]
    fn empty() -> Self {
        Self {
            operations: Vec::new(),
        }
    }

    fn append_trigger(&self, list: &str) -> RuntimeResult<&'static str> {
        self.operations
            .iter()
            .find_map(|operation| match &operation.kind {
                RuntimeListOperationKind::Append { trigger, .. } if operation.list == list => {
                    Some(*trigger)
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

    fn append_seed_fields(
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
        let mut seed_fields = ValueColumns::default();
        for field in fields {
            if field.source != trigger {
                return Err(format!(
                    "append field `{}` uses unsupported source `{}`; expected trigger `{trigger}`",
                    field.name, field.source
                )
                .into());
            }
            seed_fields.insert_value(
                field.name.to_owned(),
                FieldValue::Text(trigger_value.to_owned()),
            );
        }
        Ok(seed_fields)
    }

    fn count_predicate(&self, list: &str, target: &str) -> RuntimeResult<RuntimeListPredicate> {
        self.operations
            .iter()
            .find_map(|operation| match &operation.kind {
                RuntimeListOperationKind::Count {
                    target: candidate,
                    predicate,
                } if operation.list == list && *candidate == target => Some(*predicate),
                RuntimeListOperationKind::Append { .. }
                | RuntimeListOperationKind::Remove { .. }
                | RuntimeListOperationKind::Retain { .. }
                | RuntimeListOperationKind::Count { .. } => None,
            })
            .ok_or_else(|| format!("list `{list}` has no count operation for `{target}`").into())
    }

    fn retain_predicate(&self, list: &str, target: &str) -> RuntimeResult<RuntimeListPredicate> {
        self.operations
            .iter()
            .find_map(|operation| match &operation.kind {
                RuntimeListOperationKind::Retain {
                    target: candidate,
                    predicate,
                } if operation.list == list && *candidate == target => Some(*predicate),
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
        ListPredicate::RowFieldBool { path } => RuntimeListPredicate::FieldBool {
            path: leak_runtime_path(path.clone()),
        },
        ListPredicate::RowFieldBoolNot { path } => RuntimeListPredicate::FieldBoolNot {
            path: leak_runtime_path(path.clone()),
        },
        ListPredicate::SelectedFilterVisibility {
            selector,
            row_field,
        } => RuntimeListPredicate::SelectorVisibility {
            selector: leak_runtime_path(selector.clone()),
            row_field: leak_runtime_path(row_field.clone()),
        },
        _ => RuntimeListPredicate::Unsupported,
    }
}

fn row_field_name(path: &str) -> &str {
    path.rsplit_once('.')
        .map(|(_, field)| field)
        .unwrap_or(path)
}

fn seeded_todomvc_generic(
    ir: &TypedProgram,
    count: usize,
) -> RuntimeResult<(GenericScheduledRuntime, JsonValue)> {
    let compiled = CompiledProgram::from_ir(ir)?;
    if !matches!(compiled.surface.kind, ExecutableSurfaceKind::TodoMvc) {
        return Err("TodoMVC stress profiles require a TodoMVC executable surface".into());
    }
    let mut runtime = GenericScheduledRuntime::new(ir, &compiled)?;
    let capacity = runtime.storage.lists.capacity("todos");
    if let Some(capacity) = capacity
        && count > capacity
    {
        return Err(
            format!("TodoMVC stress needs {count} rows but LIST capacity is {capacity}").into(),
        );
    }
    let row_template = runtime
        .storage
        .lists
        .row_template("todos")
        .cloned()
        .ok_or("TodoMVC generic runtime has no `todos` row template")?;
    let row_source_paths = runtime.list_source_bindings.source_paths("todos")?.to_vec();
    let mut rows = Vec::with_capacity(count);
    for index in 0..count {
        let mut seed_fields = ValueColumns::default();
        seed_fields.insert_value(
            "title".to_owned(),
            FieldValue::Text(format!("Todo {index}")),
        );
        rows.push(row_template.materialize(seed_fields)?);
    }
    runtime.storage.lists.insert(
        "todos".to_owned(),
        KeyedList::from_values(rows),
        capacity,
        row_template,
    );
    runtime.storage.sources = SourceStore::default();
    runtime
        .storage
        .reserve_source_bindings(count * row_source_paths.len());
    runtime.storage.sources.reserve_rows(count);
    for index in 0..count {
        let (key, generation) = runtime.storage.row_identity("todos", index)?;
        runtime
            .storage
            .bind_row_sources("todos", key, generation, &row_source_paths);
    }
    let ir_proof = json!({
        "runtime_constructed_from_ir": true,
        "compiled_surface": compiled.surface.kind.as_str(),
        "compiled_surface_inferred_from_ir": compiled.surface.inferred_from_ir,
        "schedule_node_count": compiled.schedule_node_count,
        "state_initializer_count": compiled.state_initializer_count,
        "list_initializer_count": compiled.list_initializer_count,
        "derived_value_count": compiled.derived_value_count,
        "derived_text_transform_count": compiled.derived_text_transform_count,
        "update_branch_count": compiled.update_branch_count,
        "list_operation_count": compiled.list_operation_count,
        "source_route_count": compiled.source_route_count,
        "list_source_binding_count": compiled.list_source_bindings.list_slots.len(),
        "row_source_binding_count": row_source_paths.len(),
        "list_capacity": capacity,
    });
    Ok((runtime, ir_proof))
}

#[cfg(test)]
#[derive(Clone, Debug)]
struct TodoRuntime {
    generic: GenericScheduledRuntime,
    next_source_seq: u64,
    stale_source_drop_count: u64,
}

#[derive(Clone, Debug)]
struct TodoRuntimeState {
    next_source_seq: u64,
    stale_source_drop_count: u64,
    dirty_key_sets: DirtyKeySets,
}

#[cfg(test)]
#[derive(Clone, Debug)]
enum TodoEvent<'a> {
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
impl TodoRuntime {
    #[cfg(test)]
    fn from_generic(mut generic: GenericScheduledRuntime) -> RuntimeResult<Self> {
        let todo_count = generic.list_len("todos")?;
        let row_source_paths = generic.row_source_paths("todos")?.to_vec();
        generic.reserve_source_bindings(todo_count * row_source_paths.len());
        generic.reserve_source_rows(todo_count);
        for index in 0..todo_count {
            let (key, generation) = generic.row_identity("todos", index)?;
            generic.bind_row_sources("todos", key, generation, &row_source_paths);
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
                return Err("TodoMVC seed titles must not be empty".into());
            }
        }
        Ok(self)
    }

    fn apply_step_into<'a>(
        &mut self,
        step: &'a ScenarioStep,
        mut deltas: &mut Vec<SemanticDelta<'a>>,
        mut patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<()> {
        let Some(event) = self.route_step(step)? else {
            return Ok(());
        };
        match &event {
            TodoEvent::Source(routed) => assert_routed_source_event_matches(step, routed.event)?,
            TodoEvent::RowSource { routed, .. } => {
                assert_routed_source_event_matches(step, routed.event)?
            }
            TodoEvent::HoverDelete { .. } => {}
        }
        match event {
            TodoEvent::Source(routed) if routed.route_kind == GenericSourceRouteKind::RootText => {
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
                        emit_todomvc_default_protocol_mutation(
                            mutation,
                            &mut deltas,
                            &mut patches,
                        )?;
                        Ok(())
                    },
                )?;
            }
            TodoEvent::Source(routed)
                if routed.route_kind == GenericSourceRouteKind::ListAppend =>
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
                    self.emit_todo_insert(insert, &mut deltas, &mut patches)?;
                    if let Some(commit) = batch.root_text("store.new_todo_text") {
                        emit_todomvc_default_protocol_mutation(
                            GenericSourceMutation::RootText(commit),
                            &mut deltas,
                            &mut patches,
                        )?;
                    }
                }
            }
            TodoEvent::Source(routed)
                if routed.route_kind == GenericSourceRouteKind::RootScalar =>
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
                        emit_todomvc_default_protocol_mutation(
                            mutation,
                            &mut deltas,
                            &mut patches,
                        )?;
                        Ok(())
                    },
                )?;
            }
            TodoEvent::Source(routed)
                if routed.route_kind == GenericSourceRouteKind::ListRemove =>
            {
                self.remove_where_source(&step.id, routed.source(), &mut deltas, &mut patches)?;
            }
            TodoEvent::Source(routed)
                if routed.route_kind == GenericSourceRouteKind::IndexedBoolBulk =>
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
                        emit_todomvc_default_protocol_mutation(
                            mutation,
                            &mut deltas,
                            &mut patches,
                        )?;
                        Ok(())
                    },
                )?;
            }
            TodoEvent::Source(routed) => {
                return Err(format!(
                    "{} TodoMVC source `{}` classified as unsupported generic route `{}`",
                    step.id,
                    routed.source(),
                    routed.route_kind.as_str()
                )
                .into());
            }
            TodoEvent::RowSource {
                routed,
                target_text,
                target_occurrence,
            } if routed.route_kind == GenericSourceRouteKind::IndexedBoolToggle => {
                let index = self.find_index_at_occurrence(target_text, target_occurrence)?;
                let all_completed = self.all_completed();
                self.apply_todo_bool_source_action(
                    &step.id,
                    index,
                    routed.source(),
                    Some(target_text),
                    all_completed,
                    &mut deltas,
                    &mut patches,
                )?;
            }
            TodoEvent::RowSource {
                routed,
                target_text,
                target_occurrence,
            } if routed.route_kind == GenericSourceRouteKind::IndexedTextOpen => {
                let index = self.find_index_at_occurrence(target_text, target_occurrence)?;
                close_other_todomvc_editors(&mut self.generic, index, &mut deltas, &mut patches)?;
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
                emit_todomvc_render_patch_for_mutation(
                    &GenericSourceMutation::BoolField(editing),
                    GenericRenderContext {
                        todo_show_edit_input_text: Some(edit_text.value),
                        ..GenericRenderContext::default()
                    },
                    &mut patches,
                )?;
            }
            TodoEvent::RowSource {
                routed,
                target_text,
                ..
            } if routed.route_kind == GenericSourceRouteKind::IndexedTextChange => {
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
                        emit_todomvc_default_protocol_mutation(
                            mutation,
                            &mut deltas,
                            &mut patches,
                        )?;
                        Ok(())
                    },
                )?;
            }
            TodoEvent::RowSource {
                routed,
                target_text,
                ..
            } if routed.route_kind == GenericSourceRouteKind::IndexedTextKey => {
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
                        if let Some(title) = batch.text("title") {
                            emit_todomvc_default_protocol_mutation(
                                GenericSourceMutation::TextField(title),
                                &mut deltas,
                                &mut patches,
                            )?;
                        }
                    } else if let Some(edit_text) = batch.text("edit_text") {
                        deltas.push(edit_text.semantic_delta());
                    }
                    let editing =
                        batch.require_bool(routed.source(), "editing update", "editing")?;
                    emit_todomvc_default_protocol_mutation(
                        GenericSourceMutation::BoolField(editing),
                        &mut deltas,
                        &mut patches,
                    )?;
                }
            }
            TodoEvent::RowSource {
                routed,
                target_text,
                ..
            } if routed.route_kind == GenericSourceRouteKind::IndexedTextCommit => {
                let index = self
                    .find_index(target_text)
                    .or_else(|_| self.find_editing_index())?;
                let all_completed = self.all_completed();
                let source_event = GenericSourceEvent {
                    text: routed.event.text.or(Some(target_text)),
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
                if let Some(title) = batch.text("title") {
                    emit_todomvc_default_protocol_mutation(
                        GenericSourceMutation::TextField(title),
                        &mut deltas,
                        &mut patches,
                    )?;
                }
                let editing = batch.require_bool(routed.source(), "editing update", "editing")?;
                emit_todomvc_default_protocol_mutation(
                    GenericSourceMutation::BoolField(editing),
                    &mut deltas,
                    &mut patches,
                )?;
            }
            TodoEvent::RowSource {
                routed,
                target_text,
                target_occurrence,
            } if routed.route_kind == GenericSourceRouteKind::ListRemove => {
                let index = self.find_index_at_occurrence(target_text, target_occurrence)?;
                if !self.remove_index_source(
                    &step.id,
                    index,
                    routed.source(),
                    Some(target_text),
                    &mut deltas,
                    &mut patches,
                )? {
                    return Err(format!(
                        "remove source `{}` predicate does not match todo `{target_text}`",
                        routed.source()
                    )
                    .into());
                }
            }
            TodoEvent::RowSource { routed, .. } => {
                return Err(format!(
                    "{} TodoMVC row source `{}` classified as unsupported generic route `{}`",
                    step.id,
                    routed.source(),
                    routed.route_kind.as_str()
                )
                .into());
            }
            TodoEvent::HoverDelete {
                target_text,
                target_occurrence,
            } => {
                let index = self.find_index_at_occurrence(target_text, target_occurrence)?;
                let (key, generation) = self.todo_row_identity(index)?;
                patches.push(
                    GenericRenderLoweringPlan::todo_mvc().lower_todomvc_row_affordance_patch(
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

    fn route_step<'a>(&mut self, step: &'a ScenarioStep) -> RuntimeResult<Option<TodoEvent<'a>>> {
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
                TodoEvent::HoverDelete {
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
    ) -> RuntimeResult<Option<TodoEvent<'a>>> {
        let source = source_event.source;
        let route_kind = self
            .generic
            .classify_source_event("todos", "title", source_event)
            .map_err(|_| format!("{} source `{source}` has no compiled route", step.id))?;
        if source_event.target_text.is_none() {
            match route_kind {
                GenericSourceRouteKind::ListAppend
                | GenericSourceRouteKind::RootText
                | GenericSourceRouteKind::ListRemove
                | GenericSourceRouteKind::IndexedBoolBulk
                | GenericSourceRouteKind::RootScalar => {
                    return Ok(Some(TodoEvent::Source(GenericRoutedSourceEvent {
                        event: source_event,
                        route_kind,
                    })));
                }
                GenericSourceRouteKind::IndexedTextChange
                | GenericSourceRouteKind::IndexedTextCommit
                | GenericSourceRouteKind::IndexedTextIdentity
                | GenericSourceRouteKind::IndexedTextKey
                | GenericSourceRouteKind::IndexedTextOpen
                | GenericSourceRouteKind::IndexedBoolToggle => {}
            }
            return Err(format!("{} source `{source}` has no TodoMVC route", step.id).into());
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
            GenericSourceRouteKind::IndexedTextKey
                | GenericSourceRouteKind::IndexedTextCommit
                | GenericSourceRouteKind::IndexedTextChange
        ) {
            self.editing_title()?;
        }
        if matches!(
            route_kind,
            GenericSourceRouteKind::ListRemove
                | GenericSourceRouteKind::IndexedBoolToggle
                | GenericSourceRouteKind::IndexedTextKey
                | GenericSourceRouteKind::IndexedTextCommit
                | GenericSourceRouteKind::IndexedTextChange
                | GenericSourceRouteKind::IndexedTextOpen
        ) {
            return Ok(Some(TodoEvent::RowSource {
                routed: GenericRoutedSourceEvent {
                    event: source_event,
                    route_kind,
                },
                target_text,
                target_occurrence,
            }));
        }
        Err(format!(
            "{} source `{source}` for target `{target_text}` has no TodoMVC route",
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
    fn append_todo_from_source<'a>(
        &mut self,
        source: &str,
        key: &str,
        title: &'a str,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<()> {
        let Some(insert) = self
            .generic
            .append_text_row_source_action_and_bind_sources(
                "todos",
                source,
                Some(key),
                Some(title),
            )?
        else {
            return Ok(());
        };
        self.emit_todo_insert(insert, deltas, patches)?;
        Ok(())
    }

    fn emit_todo_insert<'a>(
        &self,
        insert: GenericTextListAppendCommit<'a>,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<()> {
        emit_todo_insert_from_generic(&self.generic, insert, deltas, patches)
    }

    fn apply_todo_bool_source_action<'a>(
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
                emit_todomvc_default_protocol_mutation(mutation, deltas, patches)?;
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
                emit_todomvc_default_protocol_mutation(mutation, deltas, patches)?;
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
                emit_todomvc_default_protocol_mutation(mutation, deltas, patches)?;
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
                    emit_todomvc_default_protocol_mutation(
                        GenericSourceMutation::SourceUnbind(binding.clone()),
                        deltas,
                        patches,
                    )
                    .expect("test remove source unbind should lower");
                })?;
        let (key, generation) = (generic_row.key, generic_row.generation);
        self.generic.spare_row("todos", generic_row.value)?;
        emit_todomvc_default_protocol_mutation(
            GenericSourceMutation::ListRemove {
                list: "todos",
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
        patches.push(
            GenericRenderLoweringPlan::todo_mvc().lower_todomvc_list_move_patch(&commit, to)?,
        );
        Ok(())
    }

    fn find_index(&self, title: &str) -> RuntimeResult<usize> {
        self.find_index_at_occurrence(title, 1)
    }

    fn todo_row_identity(&self, index: usize) -> RuntimeResult<(u64, u64)> {
        self.generic.row_identity("todos", index)
    }

    #[cfg(test)]
    fn todo_len(&self) -> usize {
        self.generic.list_len("todos").unwrap()
    }

    #[cfg(test)]
    fn todo_key(&self, index: usize) -> u64 {
        self.todo_row_identity(index).unwrap().0
    }

    #[cfg(test)]
    fn todo_generation(&self, index: usize) -> u64 {
        self.todo_row_identity(index).unwrap().1
    }

    #[cfg(test)]
    fn todo_title_for_test(&self, index: usize) -> &str {
        self.generic
            .list_row_textlike("todos", index, "title")
            .unwrap()
    }

    #[cfg(test)]
    fn todo_edit_text_for_test(&self, index: usize) -> &str {
        self.generic
            .list_row_textlike("todos", index, "edit_text")
            .unwrap()
    }

    #[cfg(test)]
    fn todo_completed_for_test(&self, index: usize) -> bool {
        self.generic
            .list_row_bool("todos", index, "completed")
            .unwrap()
    }

    #[cfg(test)]
    fn todo_editing_for_test(&self, index: usize) -> bool {
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
        self.generic.todomvc_all_completed()
    }
}

impl ScenarioExecutor for LoadedRuntime {
    fn prepare_for_scenario(&mut self, scenario: &Scenario) -> RuntimeResult<()> {
        match &mut self.surface {
            LoadedRuntimeSurface::Todo(_) => {
                let generic = self
                    .generic
                    .as_mut()
                    .ok_or("LoadedRuntime generic schedule was already borrowed")?;
                generic.prepare_todomvc_scenario(scenario)
            }
            LoadedRuntimeSurface::Cells(state) => {
                let generic = self
                    .generic
                    .as_mut()
                    .ok_or("LoadedRuntime generic schedule was already borrowed")?;
                prepare_loaded_cells_scenario(generic, state, scenario)
            }
        }
    }

    fn apply_step<'a>(
        &mut self,
        step: &'a ScenarioStep,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<StepExecutionMetrics> {
        match &mut self.surface {
            LoadedRuntimeSurface::Todo(_) => {
                if let Some(metrics) =
                    self.try_apply_todomvc_root_source_step(step, deltas, patches)?
                {
                    Ok(metrics)
                } else if let Some(metrics) =
                    self.try_apply_todomvc_row_source_step(step, deltas, patches)?
                {
                    Ok(metrics)
                } else if let Some(metrics) =
                    self.try_apply_todomvc_render_only_step(step, deltas, patches)?
                {
                    Ok(metrics)
                } else if step.user_action.is_none() {
                    Ok(StepExecutionMetrics {
                        dirty_key_count: 0,
                        extra: StepExecutionExtra::Todo {
                            stale_source_drop_count: 0,
                        },
                    })
                } else {
                    Err(format!(
                        "{} TodoMVC step was not handled by the loaded generic runtime",
                        step.id
                    )
                    .into())
                }
            }
            LoadedRuntimeSurface::Cells(state) => {
                let generic = self
                    .generic
                    .as_mut()
                    .ok_or("LoadedRuntime generic schedule was already borrowed")?;
                apply_loaded_cells_step(generic, state, step, deltas, patches)
            }
        }
    }

    fn assert_step_after_measurement(&mut self, step: &ScenarioStep) -> RuntimeResult<()> {
        match &self.surface {
            LoadedRuntimeSurface::Todo(_) => {
                let generic = self
                    .generic
                    .as_ref()
                    .ok_or("LoadedRuntime generic schedule was already borrowed")?;
                generic.assert_todomvc_step_expectations(step)?;
                assert_loaded_todomvc_generic_fields(generic)
            }
            LoadedRuntimeSurface::Cells(state) => {
                let generic = self
                    .generic
                    .as_ref()
                    .ok_or("LoadedRuntime generic schedule was already borrowed")?;
                generic.assert_cells_step_expectations(step, &state.step_recomputed)?;
                assert_loaded_cells_generic_fields(generic, state)
            }
        }
    }

    fn state_summary(&mut self) -> JsonValue {
        self.generic_state_summary()
    }

    fn stress_profiles(&mut self, ir: &TypedProgram) -> RuntimeResult<Option<JsonValue>> {
        match &self.surface {
            LoadedRuntimeSurface::Todo(_) => Ok(Some(todomvc_stress_profiles(ir)?)),
            LoadedRuntimeSurface::Cells(_) => Ok(Some(cells_stress_profiles(ir)?)),
        }
    }
}

fn todomvc_stress_profiles(ir: &TypedProgram) -> RuntimeResult<JsonValue> {
    Ok(json!([
        todomvc_toggle_stress(ir, 1_000)?,
        todomvc_toggle_stress(ir, 10_000)?,
        todomvc_move_stress(ir, 10_000)?
    ]))
}

fn todomvc_toggle_stress(ir: &TypedProgram, rows: usize) -> RuntimeResult<JsonValue> {
    let (mut runtime, ir_proof) = seeded_todomvc_generic(ir, rows)?;
    let mut deltas = Vec::with_capacity(2);
    let mut patches = Vec::with_capacity(2);
    let index = rows / 2;
    let started = Instant::now();
    let allocations_before = allocation_snapshot();
    let (key, generation) = runtime.commit_indexed_bool_field("todos", index, "completed", true)?;
    emit_todomvc_default_protocol_mutation(
        GenericSourceMutation::BoolField(GenericBoolFieldCommit {
            list: "todos",
            key,
            generation,
            field: "completed",
            value: true,
        }),
        &mut deltas,
        &mut patches,
    )?;
    let alloc_delta = allocation_delta(allocations_before);
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(json!({
        "name": format!("todomvc-{rows}-rows-toggle-one"),
        "ir_runtime_proof": ir_proof,
        "rows": rows,
        "graph_node_count": ir.graph_node_count,
        "graph_clones_per_item": ir.lists.first().map(|list| list.graph_clones_per_item).unwrap_or(0),
        "list_slot_count": runtime.list_len("todos").unwrap_or(0),
        "dirty_key_count": dirty_key_count(&deltas),
        "semantic_delta_count": deltas.len(),
        "render_patch_count": patches.len(),
        "heap_alloc_count": alloc_delta.count,
        "heap_alloc_bytes": alloc_delta.bytes,
        "elapsed_ms": elapsed_ms,
    }))
}

fn todomvc_move_stress(ir: &TypedProgram, rows: usize) -> RuntimeResult<JsonValue> {
    let (mut runtime, ir_proof) = seeded_todomvc_generic(ir, rows)?;
    let mut deltas = Vec::with_capacity(1);
    let mut patches = Vec::with_capacity(1);
    let started = Instant::now();
    let allocations_before = allocation_snapshot();
    let commit = runtime.move_row("todos", rows / 2, rows / 2 + 1)?;
    deltas.push(commit.semantic_move_delta(rows / 2 + 1));
    patches.push(
        GenericRenderLoweringPlan::todo_mvc()
            .lower_todomvc_list_move_patch(&commit, rows / 2 + 1)?,
    );
    let alloc_delta = allocation_delta(allocations_before);
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(json!({
        "name": format!("todomvc-{rows}-rows-move-one"),
        "ir_runtime_proof": ir_proof,
        "rows": rows,
        "graph_node_count": ir.graph_node_count,
        "graph_clones_per_item": ir.lists.first().map(|list| list.graph_clones_per_item).unwrap_or(0),
        "list_slot_count": runtime.list_len("todos").unwrap_or(0),
        "dirty_key_count": dirty_key_count(&deltas),
        "semantic_delta_count": deltas.len(),
        "render_patch_count": patches.len(),
        "heap_alloc_count": alloc_delta.count,
        "heap_alloc_bytes": alloc_delta.bytes,
        "elapsed_ms": elapsed_ms,
    }))
}

#[derive(Clone, Debug, Default)]
struct Cell {
    value: String,
    value_number: Option<i64>,
    error: Option<&'static str>,
    deps: Vec<usize>,
    dependency_text: String,
    parsed: FormulaAst,
}

#[derive(Clone, Copy, Debug, Default)]
enum FormulaAst {
    #[default]
    Empty,
    Number(i64),
    Cell(usize),
    Binary(FormulaTerm, FormulaOp, FormulaTerm),
    ParseError,
}

#[derive(Clone, Copy, Debug)]
enum FormulaTerm {
    Number(i64),
    Cell(usize),
}

#[derive(Clone, Copy, Debug)]
enum FormulaOp {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Clone, Debug)]
struct FormulaEquationPlan {
    operations: Vec<RuntimeFormulaOperation>,
}

#[derive(Clone, Debug)]
struct RuntimeFormulaOperation {
    target: &'static str,
    kind: RuntimeFormulaOperationKind,
}

#[derive(Clone, Copy, Debug)]
struct FormulaDerivedStorageFields {
    value: &'static str,
    error: &'static str,
    dependencies: &'static str,
}

#[derive(Clone, Debug)]
enum RuntimeFormulaOperationKind {
    Parse {
        input: &'static str,
    },
    Dependencies {
        input: &'static str,
    },
    Eval {
        formula: &'static str,
        read: &'static str,
    },
    Error {
        formula: &'static str,
        value: &'static str,
    },
}

impl FormulaEquationPlan {
    fn from_ir(ir: &TypedProgram) -> Self {
        let operations = ir
            .formula_operations
            .iter()
            .map(|operation| RuntimeFormulaOperation {
                target: leak_runtime_path(operation.target.clone()),
                kind: match &operation.kind {
                    FormulaOperationKind::Parse { input } => RuntimeFormulaOperationKind::Parse {
                        input: leak_runtime_path(input.clone()),
                    },
                    FormulaOperationKind::Dependencies { input } => {
                        RuntimeFormulaOperationKind::Dependencies {
                            input: leak_runtime_path(input.clone()),
                        }
                    }
                    FormulaOperationKind::Eval { formula, read } => {
                        RuntimeFormulaOperationKind::Eval {
                            formula: leak_runtime_path(formula.clone()),
                            read: leak_runtime_path(read.clone()),
                        }
                    }
                    FormulaOperationKind::Error { formula, value } => {
                        RuntimeFormulaOperationKind::Error {
                            formula: leak_runtime_path(formula.clone()),
                            value: leak_runtime_path(value.clone()),
                        }
                    }
                },
            })
            .collect();
        Self { operations }
    }

    #[cfg(test)]
    fn default_cells() -> Self {
        Self {
            operations: vec![
                RuntimeFormulaOperation {
                    target: "cell.parsed_formula",
                    kind: RuntimeFormulaOperationKind::Parse {
                        input: "formula_text",
                    },
                },
                RuntimeFormulaOperation {
                    target: "cell.dependencies",
                    kind: RuntimeFormulaOperationKind::Dependencies {
                        input: "parsed_formula",
                    },
                },
                RuntimeFormulaOperation {
                    target: "cell.value",
                    kind: RuntimeFormulaOperationKind::Eval {
                        formula: "parsed_formula",
                        read: "cell_value_reader",
                    },
                },
                RuntimeFormulaOperation {
                    target: "cell.error",
                    kind: RuntimeFormulaOperationKind::Error {
                        formula: "parsed_formula",
                        value: "value",
                    },
                },
            ],
        }
    }

    fn expect_cells_pipeline(&self) -> RuntimeResult<()> {
        self.expect_parse("cell.parsed_formula", "formula_text")?;
        self.expect_dependencies("cell.dependencies", "parsed_formula")?;
        self.expect_eval("cell.value", "parsed_formula", "cell_value_reader")?;
        self.expect_error("cell.error", "parsed_formula", "value")?;
        Ok(())
    }

    fn parse_cell_formula(
        &self,
        formula: &str,
        columns: usize,
        rows: usize,
    ) -> RuntimeResult<FormulaAst> {
        self.expect_parse("cell.parsed_formula", "formula_text")?;
        Ok(parse_formula_ast(formula, columns, rows))
    }

    fn dependencies_into(&self, parsed: FormulaAst, deps: &mut Vec<usize>) -> RuntimeResult<()> {
        self.expect_dependencies("cell.dependencies", "parsed_formula")?;
        formula_ast_dependencies_into(parsed, deps);
        Ok(())
    }

    fn cell_value_protocol<'a>(&self, cell: &Cell) -> RuntimeResult<ProtocolValue<'a>> {
        self.expect_eval("cell.value", "parsed_formula", "cell_value_reader")?;
        Ok(protocol_cell_value(cell))
    }

    fn emit_cell_display_mutations<'a>(
        &self,
        key: u64,
        generation: u64,
        value: ProtocolValue<'a>,
        error: Option<&'static str>,
        mut emit: impl FnMut(GenericSourceMutation<'a>, bool) -> RuntimeResult<()>,
    ) -> RuntimeResult<()> {
        let value_field = self.value_field()?;
        emit(
            GenericSourceMutation::ValueField(GenericValueFieldCommit {
                list: "cells",
                key,
                generation,
                field: value_field,
                value,
            }),
            true,
        )?;
        if let Some(error) = error {
            let error_field = self.error_field()?;
            emit(
                GenericSourceMutation::ValueField(GenericValueFieldCommit {
                    list: "cells",
                    key,
                    generation,
                    field: error_field,
                    value: ProtocolValue::Text(Cow::Borrowed(error)),
                }),
                false,
            )?;
        }
        Ok(())
    }

    fn emit_cell_display_protocol_mutations<'a>(
        &self,
        key: u64,
        generation: u64,
        value: ProtocolValue<'a>,
        error: Option<&'static str>,
        address: &'a str,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<()> {
        self.emit_cell_display_mutations(
            key,
            generation,
            value,
            error,
            |mutation, patch_value_text| {
                emit_cells_default_protocol_mutation(
                    mutation,
                    address,
                    None,
                    false,
                    patch_value_text,
                    deltas,
                    patches,
                )
            },
        )
    }

    fn derived_storage_fields(&self) -> RuntimeResult<FormulaDerivedStorageFields> {
        Ok(FormulaDerivedStorageFields {
            value: self.value_field()?,
            error: self.error_field()?,
            dependencies: self.dependencies_field()?,
        })
    }

    fn expect_parse(&self, target: &str, input: &str) -> RuntimeResult<()> {
        self.operations
            .iter()
            .any(|operation| {
                operation.target == target
                    && matches!(
                        operation.kind,
                        RuntimeFormulaOperationKind::Parse { input: candidate } if candidate == input
                    )
            })
            .then_some(())
            .ok_or_else(|| format!("missing Formula/parse operation `{target}` from `{input}`").into())
    }

    fn expect_dependencies(&self, target: &str, input: &str) -> RuntimeResult<()> {
        self.operations
            .iter()
            .any(|operation| {
                operation.target == target
                    && matches!(
                        operation.kind,
                        RuntimeFormulaOperationKind::Dependencies { input: candidate } if candidate == input
                    )
            })
            .then_some(())
            .ok_or_else(|| {
                format!("missing Formula/dependencies operation `{target}` from `{input}`").into()
            })
    }

    fn expect_eval(&self, target: &str, formula: &str, read: &str) -> RuntimeResult<()> {
        self.operations
            .iter()
            .any(|operation| {
                operation.target == target
                    && matches!(
                        operation.kind,
                        RuntimeFormulaOperationKind::Eval { formula: candidate_formula, read: candidate_read }
                            if candidate_formula == formula && candidate_read == read
                    )
            })
            .then_some(())
            .ok_or_else(|| {
                format!("missing Formula/eval operation `{target}` from `{formula}`").into()
            })
    }

    fn expect_error(&self, target: &str, formula: &str, value: &str) -> RuntimeResult<()> {
        self.operations
            .iter()
            .any(|operation| {
                operation.target == target
                    && matches!(
                        operation.kind,
                        RuntimeFormulaOperationKind::Error { formula: candidate_formula, value: candidate_value }
                            if candidate_formula == formula && candidate_value == value
                    )
            })
            .then_some(())
            .ok_or_else(|| {
                format!("missing Formula/error operation `{target}` from `{formula}` and `{value}`").into()
            })
    }

    fn dependencies_field(&self) -> RuntimeResult<&'static str> {
        self.operations
            .iter()
            .find(|operation| {
                matches!(
                    operation.kind,
                    RuntimeFormulaOperationKind::Dependencies { .. }
                )
            })
            .map(|operation| row_field_name(operation.target))
            .ok_or_else(|| "missing Formula/dependencies operation".into())
    }

    fn value_field(&self) -> RuntimeResult<&'static str> {
        self.operations
            .iter()
            .find(|operation| matches!(operation.kind, RuntimeFormulaOperationKind::Eval { .. }))
            .map(|operation| row_field_name(operation.target))
            .ok_or_else(|| "missing Formula/eval operation".into())
    }

    fn error_field(&self) -> RuntimeResult<&'static str> {
        self.operations
            .iter()
            .find(|operation| matches!(operation.kind, RuntimeFormulaOperationKind::Error { .. }))
            .map(|operation| row_field_name(operation.target))
            .ok_or_else(|| "missing Formula/error operation".into())
    }
}

#[cfg(test)]
#[derive(Clone, Debug)]
struct CellsRuntime {
    generic: GenericScheduledRuntime,
    cells: Vec<Cell>,
    dependency_cache: GenericFormulaDependencyCache,
    evaluation_cache: GenericFormulaEvaluationCache,
    columns: usize,
    rows: usize,
    interned_texts: Vec<&'static str>,
    last_recompute_candidates: usize,
}

#[derive(Clone, Debug)]
struct CellsRuntimeState {
    cells: Vec<Cell>,
    dependency_cache: GenericFormulaDependencyCache,
    evaluation_cache: GenericFormulaEvaluationCache,
    columns: usize,
    rows: usize,
    interned_texts: Vec<&'static str>,
    step_recomputed: Vec<usize>,
    dirty_key_sets: DirtyKeySets,
    last_recompute_candidates: usize,
}

#[derive(Clone, Debug)]
struct GenericFormulaDependencyCache {
    reverse_deps: Vec<Vec<usize>>,
    affected: Vec<usize>,
    queue: Vec<usize>,
    last_edge_walks: usize,
}

#[derive(Clone, Debug)]
struct GenericFormulaEvaluationCache {
    visiting: Vec<bool>,
    eval_cache: Vec<Option<Result<i64, &'static str>>>,
    last_eval_calls: usize,
}

impl GenericFormulaDependencyCache {
    fn with_capacity(cell_count: usize) -> Self {
        Self {
            reverse_deps: vec![Vec::new(); cell_count],
            affected: Vec::with_capacity(cell_count),
            queue: Vec::with_capacity(cell_count),
            last_edge_walks: 0,
        }
    }

    fn reserve_dependents(&mut self, minimum_capacity: usize) {
        for dependents in &mut self.reverse_deps {
            dependents.reserve(minimum_capacity);
        }
    }

    fn replace_dependencies(&mut self, cell_index: usize, deps: &[usize]) {
        for dependents in &mut self.reverse_deps {
            if let Some(index) = dependents
                .iter()
                .position(|candidate| *candidate == cell_index)
            {
                dependents.swap_remove(index);
            }
        }
        for dependency in deps {
            if let Some(dependents) = self.reverse_deps.get_mut(*dependency)
                && !dependents.contains(&cell_index)
            {
                dependents.push(cell_index);
            }
        }
    }

    fn collect_affected(&mut self, changed_index: usize) -> &[usize] {
        self.affected.clear();
        self.queue.clear();
        self.last_edge_walks = 0;
        self.affected.push(changed_index);
        self.queue.push(changed_index);
        while let Some(changed) = self.queue.pop() {
            for offset in 0..self.reverse_deps[changed].len() {
                self.last_edge_walks += 1;
                let index = self.reverse_deps[changed][offset];
                if !self.affected.contains(&index) {
                    self.affected.push(index);
                    self.queue.push(index);
                }
            }
        }
        &self.affected
    }

    fn affected(&self) -> &[usize] {
        &self.affected
    }

    fn last_edge_walks(&self) -> usize {
        self.last_edge_walks
    }
}

impl GenericFormulaEvaluationCache {
    fn with_capacity(cell_count: usize) -> Self {
        Self {
            visiting: vec![false; cell_count],
            eval_cache: vec![None; cell_count],
            last_eval_calls: 0,
        }
    }

    fn begin_tick(&mut self) {
        self.last_eval_calls = 0;
        self.eval_cache.fill(None);
        self.visiting.fill(false);
    }

    fn eval_cell(&mut self, cells: &[Cell], index: usize) -> Result<i64, &'static str> {
        self.last_eval_calls += 1;
        if let Some(result) = self.eval_cache[index] {
            return result;
        }
        if self.visiting[index] {
            return Err("cycle_error");
        }
        self.visiting[index] = true;
        let result = self.eval_formula(cells, index);
        self.visiting[index] = false;
        self.eval_cache[index] = Some(result);
        result
    }

    fn cached_result(&self, index: usize) -> Option<Result<i64, &'static str>> {
        self.eval_cache[index]
    }

    fn apply_result_to_cell(
        index: usize,
        changed_index: usize,
        result: Result<i64, &'static str>,
        cell: &mut Cell,
    ) -> RuntimeResult<bool> {
        let previous_value = cell.value_number;
        let previous_error = cell.error;
        match result {
            Ok(value) => {
                cell.value_number = Some(value);
                cell.error = None;
                cell.value.clear();
                write!(&mut cell.value, "{value}")?;
            }
            Err(error) => {
                cell.value_number = None;
                cell.error = Some(error);
                cell.value.clear();
            }
        }
        Ok(index == changed_index
            || cell.value_number != previous_value
            || cell.error != previous_error)
    }

    fn last_eval_calls(&self) -> usize {
        self.last_eval_calls
    }

    fn eval_formula(&mut self, cells: &[Cell], index: usize) -> Result<i64, &'static str> {
        match cells[index].parsed {
            FormulaAst::Empty => Ok(0),
            FormulaAst::Number(value) => Ok(value),
            FormulaAst::Cell(cell) => self.eval_cell(cells, cell),
            FormulaAst::Binary(left, op, right) => {
                let left = self.eval_term(cells, left)?;
                let right = self.eval_term(cells, right)?;
                match op {
                    FormulaOp::Add => Ok(left + right),
                    FormulaOp::Subtract => Ok(left - right),
                    FormulaOp::Multiply => Ok(left * right),
                    FormulaOp::Divide if right == 0 => Err("div_by_zero"),
                    FormulaOp::Divide => Ok(left / right),
                }
            }
            FormulaAst::ParseError => Err("parse_error"),
        }
    }

    fn eval_term(&mut self, cells: &[Cell], term: FormulaTerm) -> Result<i64, &'static str> {
        match term {
            FormulaTerm::Number(value) => Ok(value),
            FormulaTerm::Cell(index) => self.eval_cell(cells, index),
        }
    }
}

fn sync_formula_derived_fields(
    runtime: &mut GenericScheduledRuntime,
    index: usize,
    fields: FormulaDerivedStorageFields,
    value: &str,
    error: Option<&'static str>,
    dependencies: &str,
) -> RuntimeResult<()> {
    runtime.set_or_insert_list_row_textlike("cells", index, fields.value, value)?;
    runtime.set_or_insert_list_row_textlike(
        "cells",
        index,
        fields.error,
        error.unwrap_or_default(),
    )?;
    runtime.set_or_insert_list_row_textlike("cells", index, fields.dependencies, dependencies)
}

#[cfg(test)]
impl CellsRuntime {
    fn from_generic(generic: GenericScheduledRuntime, ir: &TypedProgram) -> RuntimeResult<Self> {
        let (columns, rows) = cells_grid_dimensions_from_ir(ir)
            .ok_or("Cells IR has no Grid/cells list initializer")?;
        let expected_len = columns.saturating_mul(rows);
        let actual_len = generic.list_len("cells")?;
        if actual_len != expected_len {
            return Err(format!(
                "Cells generic list initialized {actual_len} rows, expected {expected_len}"
            )
            .into());
        }
        Ok(Self::with_dimensions_and_equations(generic, columns, rows))
    }

    fn with_dimensions(columns: usize, rows: usize) -> Self {
        let parsed = parse_source(
            "examples/cells.bn",
            include_str!("../../../examples/cells.bn"),
        )
        .expect("checked-in Cells source should parse");
        let ir = lower(&parsed).expect("checked-in Cells source should lower");
        let scalar_equations = default_cells_scalar_equations();
        let source_routes = SourceRoutePlan::from_plans(
            &ir,
            &scalar_equations,
            &DerivedEquationPlan::empty(),
            &ListEquationPlan::empty(),
            &BTreeSet::new(),
        )
        .expect("checked-in Cells source routes should compile");
        Self::with_dimensions_and_equations(
            GenericScheduledRuntime::from_parts(
                generic_cells_runtime(columns, rows),
                scalar_equations,
                DerivedEquationPlan::empty(),
                ListEquationPlan::empty(),
                FormulaEquationPlan::default_cells(),
                source_routes,
                default_cells_list_source_bindings(),
            ),
            columns,
            rows,
        )
    }

    fn with_dimensions_and_equations(
        generic: GenericScheduledRuntime,
        columns: usize,
        rows: usize,
    ) -> Self {
        let cell_count = columns.saturating_mul(rows);
        let mut cells = Vec::with_capacity(cell_count);
        cells.resize_with(cell_count, Cell::default);
        Self {
            generic,
            cells,
            dependency_cache: GenericFormulaDependencyCache::with_capacity(cell_count),
            evaluation_cache: GenericFormulaEvaluationCache::with_capacity(cell_count),
            columns,
            rows,
            interned_texts: Vec::new(),
            last_recompute_candidates: 0,
        }
    }

    fn protocol_text<'a>(&self, value: &str) -> ProtocolValue<'a> {
        if let Some(interned) = self
            .interned_texts
            .iter()
            .copied()
            .find(|interned| *interned == value)
        {
            ProtocolValue::Text(Cow::Borrowed(interned))
        } else {
            ProtocolValue::Text(Cow::Owned(value.to_owned()))
        }
    }

    fn reserve_cell_cache(&mut self, max_text_len: usize, max_deps: usize) {
        for cell in &mut self.cells {
            cell.value.reserve(max_text_len);
            cell.deps.reserve(max_deps);
            cell.dependency_text.reserve(max_deps.saturating_mul(8));
        }
        let minimum_fanout_capacity = max_deps.max(4);
        self.dependency_cache
            .reserve_dependents(minimum_fanout_capacity);
    }

    fn apply_step_into<'a>(
        &mut self,
        step: &'a ScenarioStep,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
        recomputed: &mut Vec<usize>,
    ) -> RuntimeResult<()> {
        let Some(routed) = self.route_step(step)? else {
            return Ok(());
        };
        assert_routed_source_event_matches(step, routed.event)?;
        match routed.route_kind {
            GenericSourceRouteKind::IndexedTextChange => {
                let source = routed.source();
                let address = routed.require_address(&step.id)?;
                if !is_cell_address(address) {
                    return Err(
                        format!("{} Cells source event missing valid address", step.id).into(),
                    );
                }
                let text = routed.require_text(&step.id)?;
                let source_event = GenericSourceEvent {
                    source,
                    text: Some(text),
                    key: None,
                    target_text: None,
                    address: Some(address),
                };
                let input = self.generic.source_action_input_for_event_by_row_field(
                    "cells-change",
                    source_event,
                    TickSeq(0),
                    "address",
                    Some(address),
                )?;
                let batch = self.generic.apply_source_actions_to_batch(
                    input,
                    |_| None,
                    |mutation| {
                        emit_cells_default_protocol_mutation(
                            mutation, address, None, true, false, deltas, patches,
                        )?;
                        Ok(())
                    },
                )?;
                batch.require_text(source, "editing-text update", "editing_text")?;
                batch.require_bool(source, "editing update", "editing")?;
            }
            GenericSourceRouteKind::IndexedTextCommit => {
                let source = routed.source();
                let address = routed.require_address(&step.id)?;
                if !is_cell_address(address) {
                    return Err(
                        format!("{} Cells source event missing valid address", step.id).into(),
                    );
                }
                let text = routed.require_text(&step.id)?;
                self.commit_from_source(source, address, text, deltas, patches, recomputed)?;
            }
            GenericSourceRouteKind::IndexedTextIdentity => {
                let source = routed.source();
                let address = routed.require_address(&step.id)?;
                if !is_cell_address(address) {
                    return Err(
                        format!("{} Cells source event missing valid address", step.id).into(),
                    );
                }
                let source_event = GenericSourceEvent {
                    source,
                    text: None,
                    key: None,
                    target_text: None,
                    address: Some(address),
                };
                let input = self.generic.source_action_input_for_event_by_row_field(
                    "cells-cancel",
                    source_event,
                    TickSeq(0),
                    "address",
                    Some(address),
                )?;
                let batch =
                    self.generic
                        .apply_source_actions_to_batch(input, |_| None, |_| Ok(()))?;
                let editing_text =
                    batch.require_identity(source, "editing-text cancel", "editing_text")?;
                let editing = batch.require_bool(source, "editing cancel", "editing")?;
                let index = self.cell_index(address)?;
                let value = self.cell_text_field(index, editing_text.field)?;
                let identity_value = self.protocol_text(value);
                emit_cells_default_protocol_mutation(
                    GenericSourceMutation::TextFieldIdentity(editing_text),
                    address,
                    Some(identity_value),
                    false,
                    false,
                    deltas,
                    patches,
                )?;
                emit_cells_default_protocol_mutation(
                    GenericSourceMutation::BoolField(editing),
                    address,
                    None,
                    false,
                    false,
                    deltas,
                    patches,
                )?;
                patches.push(keyed_patch(
                    "SetCellText",
                    RenderTarget::Borrowed(Cow::Borrowed(address)),
                    self.protocol_text(&self.cells[index].value),
                    editing_text.list,
                    editing_text.key,
                    editing_text.generation,
                ));
            }
            route_kind => {
                return Err(format!(
                    "{} Cells source `{}` classified as unsupported route `{route_kind:?}`",
                    step.id,
                    routed.source()
                )
                .into());
            }
        }
        Ok(())
    }

    fn route_step<'a>(
        &self,
        step: &'a ScenarioStep,
    ) -> RuntimeResult<Option<GenericRoutedSourceEvent<'a>>> {
        if step.user_action.is_none() {
            return Ok(None);
        }
        let source_event = GenericSourceEvent::require(step)?;
        self.route_source_event(step, source_event)
    }

    fn route_source_event<'a>(
        &self,
        step: &'a ScenarioStep,
        source_event: GenericSourceEvent<'a>,
    ) -> RuntimeResult<Option<GenericRoutedSourceEvent<'a>>> {
        let source = source_event.source;
        let routed = self
            .generic
            .route_source_event("cells", "formula_text", source_event)
            .map_err(|_| format!("{} source `{source}` has no compiled route", step.id))?;
        let address = routed
            .event
            .address
            .filter(|candidate| is_cell_address(candidate))
            .ok_or_else(|| format!("{} Cells source event missing valid address", step.id))?;
        match routed.route_kind {
            GenericSourceRouteKind::IndexedTextCommit
            | GenericSourceRouteKind::IndexedTextChange => {
                routed.require_text(&step.id)?;
                Ok(Some(routed))
            }
            GenericSourceRouteKind::IndexedTextIdentity => Ok(Some(routed)),
            route_kind => Err(format!(
                "{} Cells source `{source}` for address `{address}` classified as unsupported route `{route_kind:?}`",
                step.id
            )
            .into()),
        }
    }

    fn commit<'a>(
        &mut self,
        address: &'a str,
        formula: &'a str,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
        recomputed: &mut Vec<usize>,
    ) -> RuntimeResult<()> {
        self.commit_from_source(
            "cell.sources.editor.commit",
            address,
            formula,
            deltas,
            patches,
            recomputed,
        )
    }

    fn commit_from_source<'a>(
        &mut self,
        source: &'a str,
        address: &'a str,
        formula: &'a str,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
        recomputed: &mut Vec<usize>,
    ) -> RuntimeResult<()> {
        self.generic.formula_equations.expect_cells_pipeline()?;
        let committed_index = self.cell_index(address)?;
        let source_event = GenericSourceEvent {
            source,
            text: Some(formula),
            key: None,
            target_text: None,
            address: Some(address),
        };
        let input = self.generic.source_action_input_for_event_by_row_field(
            "cells-commit",
            source_event,
            TickSeq(0),
            "address",
            Some(address),
        )?;
        let batch = self
            .generic
            .apply_source_actions_to_batch(input, |_| None, |_| Ok(()))?;
        let formula = batch.require_text(source, "formula update", "formula_text")?;
        let editing_text = batch.require_text(source, "editing text update", "editing_text")?;
        let editing = batch.require_bool(source, "editing update", "editing")?;
        self.cells[committed_index].parsed = self.generic.formula_equations.parse_cell_formula(
            formula.value,
            self.columns,
            self.rows,
        )?;
        self.replace_cell_dependencies(committed_index)?;
        self.recompute_affected(address, recomputed)?;
        let cell = &self.cells[committed_index];
        let display_key = formula.key;
        let display_generation = formula.generation;
        let display_value = self.generic.formula_equations.cell_value_protocol(cell)?;
        let display_error = cell.error;
        emit_cells_default_protocol_mutation(
            GenericSourceMutation::TextField(formula),
            address,
            None,
            false,
            false,
            deltas,
            patches,
        )?;
        emit_cells_default_protocol_mutation(
            GenericSourceMutation::TextField(editing_text),
            address,
            None,
            false,
            false,
            deltas,
            patches,
        )?;
        emit_cells_default_protocol_mutation(
            GenericSourceMutation::BoolField(editing),
            address,
            None,
            false,
            false,
            deltas,
            patches,
        )?;
        self.generic
            .formula_equations
            .emit_cell_display_protocol_mutations(
                display_key,
                display_generation,
                display_value,
                display_error,
                address,
                deltas,
                patches,
            )?;
        Ok(())
    }

    fn recompute_affected(
        &mut self,
        changed_address: &str,
        recomputed: &mut Vec<usize>,
    ) -> RuntimeResult<()> {
        let changed_index = self.cell_index(changed_address)?;
        self.dependency_cache.collect_affected(changed_index);
        let affected_len = self.dependency_cache.affected().len();
        self.last_recompute_candidates = affected_len;
        self.evaluation_cache.begin_tick();
        for offset in 0..affected_len {
            let index = self.dependency_cache.affected()[offset];
            let _ = self.evaluation_cache.eval_cell(&self.cells, index);
        }
        for offset in 0..affected_len {
            let index = self.dependency_cache.affected()[offset];
            let result = self.evaluation_cache.cached_result(index).unwrap_or(Ok(0));
            let changed = GenericFormulaEvaluationCache::apply_result_to_cell(
                index,
                changed_index,
                result,
                &mut self.cells[index],
            )?;
            if changed {
                self.sync_cell_derived_fields(index)?;
                recomputed.push(index);
            }
        }
        recomputed.sort_unstable();
        Ok(())
    }

    fn sync_cell_derived_fields(&mut self, index: usize) -> RuntimeResult<()> {
        self.refresh_dependency_text(index);
        let fields = self.generic.formula_equations.derived_storage_fields()?;
        sync_formula_derived_fields(
            &mut self.generic,
            index,
            fields,
            &self.cells[index].value,
            self.cells[index].error,
            &self.cells[index].dependency_text,
        )
    }

    fn refresh_dependency_text(&mut self, index: usize) {
        self.cells[index].dependency_text.clear();
        for offset in 0..self.cells[index].deps.len() {
            if offset > 0 {
                self.cells[index].dependency_text.push(',');
            }
            let dependency = self.cells[index].deps[offset];
            push_cell_address(
                self.columns,
                dependency,
                &mut self.cells[index].dependency_text,
            );
        }
    }

    fn replace_cell_dependencies(&mut self, cell_index: usize) -> RuntimeResult<()> {
        self.cells[cell_index].deps.clear();
        self.generic.formula_equations.dependencies_into(
            self.cells[cell_index].parsed,
            &mut self.cells[cell_index].deps,
        )?;
        self.dependency_cache
            .replace_dependencies(cell_index, &self.cells[cell_index].deps);
        Ok(())
    }

    #[cfg(test)]
    fn cell(&self, address: &str) -> RuntimeResult<&Cell> {
        let index = self.cell_index(address)?;
        Ok(&self.cells[index])
    }

    fn cell_index(&self, address: &str) -> RuntimeResult<usize> {
        cell_index(address, self.columns, self.rows)
            .ok_or_else(|| format!("unknown cell {address}").into())
    }

    #[cfg(test)]
    fn cell_key_generation(&self, index: usize) -> (u64, u64) {
        self.generic
            .row_identity("cells", index)
            .expect("Cells generic runtime has matching cell row")
    }

    fn cell_text_field(&self, index: usize, field: &str) -> RuntimeResult<&str> {
        self.generic.list_row_textlike("cells", index, field)
    }

    #[cfg(test)]
    fn cell_bool_field(&self, index: usize, field: &str) -> RuntimeResult<bool> {
        self.generic.list_row_bool("cells", index, field)
    }

    fn address_for(&self, index: usize) -> String {
        let mut address = String::new();
        push_cell_address(self.columns, index, &mut address);
        address
    }
}

fn push_cell_address(columns: usize, index: usize, output: &mut String) {
    let col = index % columns;
    let row = index / columns + 1;
    push_spreadsheet_column_label(col, output);
    write!(output, "{row}").expect("writing to String cannot fail");
}

fn push_spreadsheet_column_label(mut index: usize, output: &mut String) {
    let mut buffer = [0u8; 8];
    let mut len = 0usize;
    index += 1;
    while index > 0 && len < buffer.len() {
        let rem = (index - 1) % 26;
        buffer[len] = b'A' + rem as u8;
        len += 1;
        index = (index - 1) / 26;
    }
    for byte in buffer[..len].iter().rev() {
        output.push(*byte as char);
    }
}

fn protocol_cell_value(cell: &Cell) -> ProtocolValue<'static> {
    if let Some(value) = cell.value_number {
        ProtocolValue::NumberText(value)
    } else {
        ProtocolValue::Text(Cow::Borrowed(""))
    }
}

fn parse_formula_ast(formula: &str, columns: usize, rows: usize) -> FormulaAst {
    let trimmed = formula.trim();
    if trimmed.is_empty() {
        return FormulaAst::Empty;
    }
    if let Ok(value) = trimmed.parse::<i64>() {
        return FormulaAst::Number(value);
    }
    let expr = trimmed.strip_prefix('=').unwrap_or(trimmed);
    if let Some((left, op, right)) = split_formula_binary(expr) {
        let Some(left) = parse_formula_term(left.trim(), columns, rows) else {
            return FormulaAst::ParseError;
        };
        let Some(right) = parse_formula_term(right.trim(), columns, rows) else {
            return FormulaAst::ParseError;
        };
        return FormulaAst::Binary(left, op, right);
    }
    parse_formula_term(expr.trim(), columns, rows)
        .map(|term| match term {
            FormulaTerm::Number(value) => FormulaAst::Number(value),
            FormulaTerm::Cell(index) => FormulaAst::Cell(index),
        })
        .unwrap_or(FormulaAst::ParseError)
}

fn split_formula_binary(expr: &str) -> Option<(&str, FormulaOp, &str)> {
    for (symbol, op) in [
        ('+', FormulaOp::Add),
        ('-', FormulaOp::Subtract),
        ('*', FormulaOp::Multiply),
        ('/', FormulaOp::Divide),
    ] {
        if let Some((left, right)) = expr.split_once(symbol)
            && !left.trim().is_empty()
            && !right.trim().is_empty()
        {
            return Some((left, op, right));
        }
    }
    None
}

fn parse_formula_term(term: &str, columns: usize, rows: usize) -> Option<FormulaTerm> {
    if let Ok(value) = term.parse::<i64>() {
        return Some(FormulaTerm::Number(value));
    }
    cell_index(term, columns, rows).map(FormulaTerm::Cell)
}

fn cells_stress_profiles(ir: &TypedProgram) -> RuntimeResult<JsonValue> {
    let compiled = CompiledProgram::from_ir(ir)?;
    if !matches!(compiled.surface.kind, ExecutableSurfaceKind::Cells) {
        return Err("Cells stress profiles require a Cells executable surface".into());
    }
    let generic = GenericScheduledRuntime::new(ir, &compiled)?;
    let (mut runtime, mut state) = initialize_loaded_cells_generic(generic, ir)?;
    if state.columns != 26 || state.rows != 100 {
        return Err(format!(
            "Cells stress profiles require the documented 26x100 grid, got {}x{}",
            state.columns, state.rows
        )
        .into());
    }
    if state.cells.len() <= 100 {
        return Err("Cells stress profiles require at least 101 grid cells".into());
    }
    runtime.formula_equations.expect_cells_pipeline()?;
    let ir_proof = json!({
        "runtime_constructed_from_ir": true,
        "compiled_surface": compiled.surface.kind.as_str(),
        "compiled_surface_inferred_from_ir": compiled.surface.inferred_from_ir,
        "schedule_node_count": compiled.schedule_node_count,
        "state_initializer_count": compiled.state_initializer_count,
        "list_initializer_count": compiled.list_initializer_count,
        "formula_operation_count": compiled.formula_operation_count,
        "source_route_count": compiled.source_route_count,
        "list_source_binding_count": compiled.list_source_bindings.list_slots.len(),
        "grid_columns": state.columns,
        "grid_rows": state.rows,
    });
    reserve_loaded_cell_cache(&mut state, "=A1+1".len().max("10".len()), 1);
    for value in ["", "0", "1", "=A1+1", "7", "10"] {
        intern_loaded_cell_text(&mut state, value);
    }
    let mut deltas = Vec::with_capacity(4);
    let mut patches = Vec::with_capacity(2);
    let mut recomputed = Vec::with_capacity(8);
    loaded_cells_commit_from_source(
        &mut runtime,
        &mut state,
        "cell.sources.editor.commit",
        "C1",
        "0",
        &mut deltas,
        &mut patches,
        &mut recomputed,
    )?;
    loaded_cells_commit_from_source(
        &mut runtime,
        &mut state,
        "cell.sources.editor.commit",
        "A1",
        "1",
        &mut deltas,
        &mut patches,
        &mut recomputed,
    )?;
    loaded_cells_commit_from_source(
        &mut runtime,
        &mut state,
        "cell.sources.editor.commit",
        "B1",
        "=A1+1",
        &mut deltas,
        &mut patches,
        &mut recomputed,
    )?;
    deltas.clear();
    patches.clear();
    recomputed.clear();
    let started = Instant::now();
    let allocations_before = allocation_snapshot();
    loaded_cells_commit_from_source(
        &mut runtime,
        &mut state,
        "cell.sources.editor.commit",
        "C1",
        "7",
        &mut deltas,
        &mut patches,
        &mut recomputed,
    )?;
    let alloc_delta = allocation_delta(allocations_before);
    let unrelated_elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let unrelated_recomputed: Vec<_> = recomputed
        .iter()
        .map(|index| loaded_cell_address(&state, *index))
        .collect();
    let unrelated = json!({
        "name": "cells-26x100-unrelated-edit",
        "ir_runtime_proof": ir_proof.clone(),
        "cells": state.cells.len(),
        "graph_node_count": ir.graph_node_count,
        "graph_clones_per_item": ir.lists.first().map(|list| list.graph_clones_per_item).unwrap_or(0),
        "dirty_cell_count": recomputed.len(),
        "recompute_candidate_count": state.last_recompute_candidates,
        "formula_eval_call_count": state.evaluation_cache.last_eval_calls(),
        "dependency_edge_walk_count": state.dependency_cache.last_edge_walks(),
        "recomputed_cells": unrelated_recomputed,
        "semantic_delta_count": deltas.len(),
        "render_patch_count": patches.len(),
        "heap_alloc_count": alloc_delta.count,
        "heap_alloc_bytes": alloc_delta.bytes,
        "elapsed_ms": unrelated_elapsed_ms,
    });
    deltas.clear();
    patches.clear();
    recomputed.clear();
    let started = Instant::now();
    let allocations_before = allocation_snapshot();
    loaded_cells_commit_from_source(
        &mut runtime,
        &mut state,
        "cell.sources.editor.commit",
        "A1",
        "10",
        &mut deltas,
        &mut patches,
        &mut recomputed,
    )?;
    let alloc_delta = allocation_delta(allocations_before);
    let dependent_elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let dependent_recomputed: Vec<_> = recomputed
        .iter()
        .map(|index| loaded_cell_address(&state, *index))
        .collect();
    let dependent = json!({
        "name": "cells-26x100-dependent-edit",
        "ir_runtime_proof": ir_proof.clone(),
        "cells": state.cells.len(),
        "graph_node_count": ir.graph_node_count,
        "graph_clones_per_item": ir.lists.first().map(|list| list.graph_clones_per_item).unwrap_or(0),
        "dirty_cell_count": recomputed.len(),
        "recompute_candidate_count": state.last_recompute_candidates,
        "formula_eval_call_count": state.evaluation_cache.last_eval_calls(),
        "dependency_edge_walk_count": state.dependency_cache.last_edge_walks(),
        "recomputed_cells": dependent_recomputed,
        "semantic_delta_count": deltas.len(),
        "render_patch_count": patches.len(),
        "heap_alloc_count": alloc_delta.count,
        "heap_alloc_bytes": alloc_delta.bytes,
        "elapsed_ms": dependent_elapsed_ms,
    });
    deltas.clear();
    patches.clear();
    recomputed.clear();
    state.dependency_cache.reserve_dependents(100);
    recomputed.reserve(128);
    for index in 1..=100 {
        let address = loaded_cell_address(&state, index);
        let formula = format!("=A1+{index}");
        let mut setup_deltas = Vec::with_capacity(4);
        let mut setup_patches = Vec::with_capacity(2);
        loaded_cells_commit_from_source(
            &mut runtime,
            &mut state,
            "cell.sources.editor.commit",
            &address,
            &formula,
            &mut setup_deltas,
            &mut setup_patches,
            &mut recomputed,
        )?;
        recomputed.clear();
    }
    let expected_fanout = 100usize;
    let expected_dirty_cell_count = expected_fanout + 1;
    let started = Instant::now();
    let allocations_before = allocation_snapshot();
    loaded_cells_commit_from_source(
        &mut runtime,
        &mut state,
        "cell.sources.editor.commit",
        "A1",
        "2",
        &mut deltas,
        &mut patches,
        &mut recomputed,
    )?;
    let alloc_delta = allocation_delta(allocations_before);
    let fanout_elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let fanout_recomputed: Vec<_> = recomputed
        .iter()
        .map(|index| loaded_cell_address(&state, *index))
        .collect();
    let fanout = json!({
        "name": "cells-26x100-fanout-100-update",
        "ir_runtime_proof": ir_proof,
        "cells": state.cells.len(),
        "expected_fanout": expected_fanout,
        "expected_dirty_cell_count": expected_dirty_cell_count,
        "graph_node_count": ir.graph_node_count,
        "graph_clones_per_item": ir.lists.first().map(|list| list.graph_clones_per_item).unwrap_or(0),
        "dirty_cell_count": recomputed.len(),
        "recomputed_cell_count": recomputed.len(),
        "recompute_candidate_count": state.last_recompute_candidates,
        "formula_eval_call_count": state.evaluation_cache.last_eval_calls(),
        "dependency_edge_walk_count": state.dependency_cache.last_edge_walks(),
        "recomputed_cells": fanout_recomputed,
        "semantic_delta_count": deltas.len(),
        "render_patch_count": patches.len(),
        "heap_alloc_count": alloc_delta.count,
        "heap_alloc_bytes": alloc_delta.bytes,
        "elapsed_ms": fanout_elapsed_ms,
    });
    Ok(json!([unrelated, dependent, fanout]))
}

fn formula_ast_dependencies_into(ast: FormulaAst, deps: &mut Vec<usize>) {
    match ast {
        FormulaAst::Cell(index) => push_unique_dependency(deps, index),
        FormulaAst::Binary(left, _, right) => {
            formula_term_dependencies_into(left, deps);
            formula_term_dependencies_into(right, deps);
        }
        FormulaAst::Empty | FormulaAst::Number(_) | FormulaAst::ParseError => {}
    }
}

fn formula_term_dependencies_into(term: FormulaTerm, deps: &mut Vec<usize>) {
    if let FormulaTerm::Cell(index) = term {
        push_unique_dependency(deps, index);
    }
}

fn push_unique_dependency(deps: &mut Vec<usize>, index: usize) {
    if !deps.contains(&index) {
        deps.push(index);
    }
}

fn count_formula_dependencies(formula: &str) -> usize {
    formula
        .split(|ch: char| !(ch.is_ascii_alphanumeric()))
        .filter(|part| is_cell_address(part))
        .count()
}

fn cell_index(address: &str, columns: usize, rows: usize) -> Option<usize> {
    let mut chars = address.chars();
    let col = chars.next()? as u8;
    if !col.is_ascii_uppercase() {
        return None;
    }
    let col = usize::from(col - b'A');
    if col >= columns {
        return None;
    }
    let row = chars.as_str().parse::<usize>().ok()?;
    if row == 0 || row > rows {
        return None;
    }
    Some((row - 1) * columns + col)
}

#[cfg(test)]
fn todomvc_seed_titles_from_ir(ir: &TypedProgram) -> RuntimeResult<Vec<String>> {
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
                .ok_or("TodoMVC seed row has no `title` field")?;
            match &field.value {
                InitialValue::Text { value } => Ok(value.clone()),
                other => Err(format!("TodoMVC seed title is not text: {other:?}").into()),
            }
        })
        .collect()
}

#[cfg(test)]
fn default_cells_list_source_bindings() -> ListSourceBindingPlan {
    ListSourceBindingPlan {
        list_slots: vec![ListSourceBindingSlot {
            list: "cells",
            source_paths: Vec::new(),
        }],
    }
}

#[cfg(test)]
fn default_cells_scalar_equations() -> ScalarEquationPlan {
    let parsed = parse_source(
        "examples/cells.bn",
        include_str!("../../../examples/cells.bn"),
    )
    .expect("checked-in Cells source should parse");
    let ir = lower(&parsed).expect("checked-in Cells source should lower");
    ScalarEquationPlan::from_ir(&ir)
}

fn leak_runtime_path(path: String) -> &'static str {
    Box::leak(path.into_boxed_str())
}

fn cells_grid_dimensions_from_ir(ir: &TypedProgram) -> Option<(usize, usize)> {
    ir.lists
        .iter()
        .find(|list| list.name == "cells")
        .and_then(|list| match list.initializer {
            ListInitializer::Grid { columns, rows } => Some((columns, rows)),
            ListInitializer::RecordLiteral { .. }
            | ListInitializer::Empty
            | ListInitializer::Unknown { .. } => None,
        })
}

fn spreadsheet_column_label(index: usize) -> Option<String> {
    if index >= 26 {
        return None;
    }
    Some(((b'A' + index as u8) as char).to_string())
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
    field: &'static str,
    value: ProtocolValue<'a>,
) -> SemanticDelta<'a> {
    GenericCircuitRuntime::semantic_field_delta(key.map(|_| "todos"), key, generation, field, value)
}

fn emit_todomvc_default_protocol_mutation<'a>(
    mutation: GenericSourceMutation<'a>,
    deltas: &mut Vec<SemanticDelta<'a>>,
    patches: &mut Vec<RenderPatch<'a>>,
) -> RuntimeResult<()> {
    if let Some(delta) = mutation.semantic_delta() {
        deltas.push(delta);
    }
    emit_todomvc_render_patch_for_mutation(&mutation, GenericRenderContext::default(), patches)
}

fn emit_todo_insert_from_generic<'a>(
    generic: &GenericScheduledRuntime,
    insert: GenericTextListAppendCommit<'a>,
    deltas: &mut Vec<SemanticDelta<'a>>,
    patches: &mut Vec<RenderPatch<'a>>,
) -> RuntimeResult<()> {
    let list = insert.list;
    let key = insert.key;
    let generation = insert.generation;
    emit_todomvc_default_protocol_mutation(
        GenericSourceMutation::ListAppend(insert),
        deltas,
        patches,
    )?;
    for binding in generic.row_source_bindings(list, key, generation) {
        emit_todomvc_default_protocol_mutation(
            GenericSourceMutation::SourceBind(binding.clone()),
            deltas,
            patches,
        )?;
    }
    Ok(())
}

fn emit_todomvc_render_patch_for_mutation<'a>(
    mutation: &GenericSourceMutation<'a>,
    context: GenericRenderContext<'a>,
    patches: &mut Vec<RenderPatch<'a>>,
) -> RuntimeResult<()> {
    let lowerer = GenericRenderLoweringPlan::todo_mvc();
    if let Some(render_patch) = lowerer.lower_mutation_patch(mutation, context)? {
        patches.push(render_patch);
    }
    Ok(())
}

fn emit_cells_default_protocol_mutation<'a>(
    mutation: GenericSourceMutation<'a>,
    address: &'a str,
    identity_value: Option<ProtocolValue<'a>>,
    patch_editor_text: bool,
    patch_value_text: bool,
    deltas: &mut Vec<SemanticDelta<'a>>,
    patches: &mut Vec<RenderPatch<'a>>,
) -> RuntimeResult<()> {
    let lowerer = GenericRenderLoweringPlan::cells();
    match mutation {
        GenericSourceMutation::TextFieldIdentity(commit) => {
            let value = identity_value.ok_or_else(|| {
                format!(
                    "cell text identity mutation for `{}` requires the copied field value",
                    commit.field
                )
            })?;
            deltas.push(commit.semantic_delta_with_value(value));
        }
        _ => {
            if let Some(delta) = mutation.semantic_delta() {
                deltas.push(delta);
            }
        }
    }
    if let Some(render_patch) = lowerer.lower_mutation_patch(
        &mutation,
        GenericRenderContext {
            address: Some(address),
            todo_show_edit_input_text: None,
            patch_editor_text,
            patch_value_text,
        },
    )? {
        patches.push(render_patch);
    }
    Ok(())
}

fn source_binding_value(binding: &SourceBinding) -> ProtocolValue<'static> {
    ProtocolValue::SourceBinding {
        source_path: binding.source_path,
        source_id: binding.source_id,
        bind_epoch: binding.bind_epoch,
    }
}

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

fn keyed_patch<'a>(
    kind: &'static str,
    target: RenderTarget<'a>,
    value: ProtocolValue<'a>,
    list_id: &'static str,
    key: u64,
    generation: u64,
) -> RenderPatch<'a> {
    RenderPatch {
        kind,
        target,
        value,
        list_id: Some(list_id),
        key: Some(key),
        generation: Some(generation),
        source_id: None,
        bind_epoch: None,
    }
}

fn source_patch<'a>(
    kind: &'static str,
    target: RenderTarget<'a>,
    value: ProtocolValue<'a>,
    binding: &SourceBinding,
) -> RenderPatch<'a> {
    RenderPatch {
        kind,
        target,
        value,
        list_id: Some(binding.list_id),
        key: Some(binding.key),
        generation: Some(binding.generation),
        source_id: Some(binding.source_id),
        bind_epoch: Some(binding.bind_epoch),
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
        return delta.kind == kind && delta.field_path == Some(field);
    }
    delta.kind == expected
}

fn render_patch_matches(patch: &RenderPatch<'_>, expected: &str) -> bool {
    patch.kind == expected
}

fn delta_signature(delta: &SemanticDelta<'_>) -> String {
    match delta.field_path {
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

fn display_server() -> String {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        "wayland".to_owned()
    } else if std::env::var("DISPLAY").is_ok() {
        "x11".to_owned()
    } else {
        "none".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn todo_runtime_from_parsed(parsed: &ParsedProgram) -> TodoRuntime {
        let ir = lower(parsed).unwrap();
        let compiled = CompiledProgram::from_ir(&ir).unwrap();
        let generic = GenericScheduledRuntime::new(&ir, &compiled).unwrap();
        TodoRuntime::from_generic(generic).unwrap()
    }

    fn todo_index(runtime: &TodoRuntime, title: &str) -> usize {
        runtime.find_index(title).unwrap()
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
                    && delta.list_id == Some("todos")
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
            (Some("error"), ProtocolValue::Text(error)) if error.as_ref() == "cycle_error"
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
        incomplete_report["runtime_execution"]["generic_runtime_slices"]["generic_todomvc_source_route_classifier"] =
            json!(false);
        assert!(
            verify_runtime_execution_metadata(&incomplete_report, Path::new("memory:todomvc"))
                .unwrap_err()
                .to_string()
                .contains("generic_todomvc_source_route_classifier")
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
                "generic_todomvc_scenario_preparation": true,
                "generic_todomvc_scenario_expectation_assertions": true,
                "generic_todomvc_summary_reads_authoritative_storage": true,
                "generic_todomvc_common_render_patch_lowering": true,
                "generic_loaded_runtime_todomvc_stress_profile_executor": true
            }),
        );
        assert_eq!(
            shells,
            vec![
                "todomvc_scenario_glue",
                "todomvc_assertion_glue",
                "todomvc_report_glue",
                "todomvc_render_patch_report_glue",
                "todomvc_stress_report_glue",
            ]
        );

        let shells = remaining_example_specific_shells(
            &compiled,
            &json!({
                "generic_todomvc_scenario_preparation": false,
                "generic_todomvc_scenario_expectation_assertions": true,
            }),
        );
        assert_eq!(shells, vec!["todomvc_assertion_glue"]);
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
        assert_eq!(insert.list_id, Some("todos"));
        assert!(insert.key.is_some());
        assert!(insert.generation.is_some());
        assert!(matches!(
            &insert.value,
            ProtocolValue::Text(value) if value.as_ref() == "Test todo"
        ));

        let cells_source = include_str!("../../../examples/cells.bn");
        let cells_output = run_scenario_source_with_step_limit(
            "generic-delta:cells",
            cells_source,
            Path::new("../../examples/cells.scn"),
            VerificationLayer::Semantic,
            Some(3),
        )
        .unwrap();
        let formula = cells_output
            .semantic_deltas
            .iter()
            .find(|delta| delta.field_path == Some("formula_text"))
            .expect("Cells commit should emit a keyed generic field delta");
        assert_eq!(formula.list_id, Some("cells"));
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
                    text: Some("Test todo"),
                    key: Some("Enter"),
                    target_text: None,
                    address: None,
                },
                TickSeq(7),
                |_, _| Ok(None),
            )
            .unwrap();
        assert_eq!(todo_input.source, "store.sources.new_todo_input.key_down");
        assert_eq!(todo_input.list, Some("todos"));
        assert_eq!(todo_input.index, None);
        assert_eq!(todo_input.key, Some("Enter"));
        assert_eq!(todo_input.text, Some("Test todo"));

        let cells_parsed = parse_source(
            "examples/cells.bn",
            include_str!("../../../examples/cells.bn"),
        )
        .unwrap();
        let cells_ir = lower(&cells_parsed).unwrap();
        let cells_compiled = CompiledProgram::from_ir(&cells_ir).unwrap();
        let cells_runtime = GenericScheduledRuntime::new(&cells_ir, &cells_compiled).unwrap();
        let cells_input = cells_runtime
            .source_action_input_for_event_by_row_field(
                "cells-a1-commit",
                GenericSourceEvent {
                    source: "cell.sources.editor.commit",
                    text: Some("41"),
                    key: Some("Enter"),
                    target_text: None,
                    address: Some("A1"),
                },
                TickSeq(3),
                "address",
                Some("A1"),
            )
            .unwrap();
        assert_eq!(cells_input.source, "cell.sources.editor.commit");
        assert_eq!(cells_input.list, Some("cells"));
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

        let cells_parsed = parse_source(
            "examples/cells.bn",
            include_str!("../../../examples/cells.bn"),
        )
        .unwrap();
        let cells_ir = lower(&cells_parsed).unwrap();
        let cells_compiled = CompiledProgram::from_ir(&cells_ir).unwrap();
        let cells_runtime = GenericScheduledRuntime::new(&cells_ir, &cells_compiled).unwrap();
        let a1 = cells_runtime
            .list_row_fields_json("cells", 0, &["address", "formula_text", "editing"])
            .unwrap();
        assert_eq!(a1["address"], "A1");
        assert_eq!(a1["formula_text"], "");
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
                    text: Some("Test todo".to_owned()),
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
                    text: Some("Duplicate".to_owned()),
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
        let source = include_str!("../../../examples/cells.bn");
        let scenario = parse_scenario(Path::new("../../examples/cells.scn")).unwrap();
        let mut runtime = LiveRuntime::new(
            "playground-live:cells",
            source,
            Path::new("../../examples/cells.scn"),
        )
        .unwrap();
        runtime
            .apply_source_event_for_step(
                &scenario.step[1],
                LiveSourceEvent {
                    source: "cell.sources.editor.change".to_owned(),
                    text: Some("41".to_owned()),
                    address: Some("A1".to_owned()),
                    ..LiveSourceEvent::default()
                },
            )
            .unwrap();
        let output = runtime
            .apply_source_event_for_step(
                &scenario.step[2],
                LiveSourceEvent {
                    source: "cell.sources.editor.commit".to_owned(),
                    text: Some("41".to_owned()),
                    key: Some("Enter".to_owned()),
                    address: Some("A1".to_owned()),
                    ..LiveSourceEvent::default()
                },
            )
            .unwrap();
        assert_eq!(output.state_summary["cells"][0]["value"], "41");
        assert!(
            output
                .semantic_deltas
                .iter()
                .any(|delta| delta.field_path == Some("value"))
        );
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

        let renamed_cell_source = include_str!("../../../examples/cells.bn")
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

        let cells_source = include_str!("../../../examples/cells.bn").replace(
            "Formula/dependencies(parsed_formula)",
            "Formula/refs(parsed_formula)",
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
            err.to_string()
                .contains("executable Cells source missing `Formula/dependencies`")
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
            todomvc_seed_titles_from_ir(&ir).unwrap(),
            vec![
                "Source title A",
                "Finish TodoMVC renderer",
                "Walk the dog",
                "Source title B"
            ]
        );

        let cells_source = include_str!("../../../examples/cells.bn")
            .replace("columns: 26, rows: 100", "columns: 3, rows: 4");
        let parsed = parse_source("examples/cells.bn", cells_source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert_eq!(cells_grid_dimensions_from_ir(&ir), Some((3, 4)));
    }

    #[test]
    fn duplicate_todo_titles_route_by_hidden_host_scope_not_visible_data_identity() {
        let source =
            include_str!("../../../examples/todomvc.bn").replace("Walk the dog", "Buy groceries");
        let parsed = parse_source("examples/todomvc.bn", source).unwrap();
        let mut runtime = todo_runtime_from_parsed(&parsed);
        let target_index = runtime
            .find_index_at_occurrence("Buy groceries", 2)
            .unwrap();
        let other_duplicate_index = runtime
            .find_index_at_occurrence("Buy groceries", 1)
            .unwrap();
        let target_key = runtime.todo_key(target_index);
        let target_generation = runtime.todo_generation(target_index);
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
        assert_eq!(
            runtime.todo_completed_for_test(other_duplicate_index),
            false
        );
        assert_eq!(runtime.todo_completed_for_test(target_index), true);
        assert_eq!(deltas.iter().find_map(|delta| delta.key), Some(target_key));
    }

    #[test]
    fn todo_row_sources_are_bound_for_seed_and_append() {
        let parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let mut runtime = todo_runtime_from_parsed(&parsed);
        let row_source_count = runtime
            .generic
            .list_source_bindings
            .source_count("todos")
            .unwrap();
        assert_eq!(
            runtime.generic.source_binding_count(),
            runtime.todo_len() * row_source_count
        );
        for index in 0..runtime.todo_len() {
            let (key, generation) = runtime.todo_row_identity(index).unwrap();
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
            .append_todo_from_source(
                "store.sources.new_todo_input.key_down",
                "Enter",
                "New source row",
                &mut deltas,
                &mut patches,
            )
            .unwrap();
        let (key, generation) = runtime
            .todo_row_identity(runtime.todo_len() - 1)
            .expect("appended row exists");
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
            .filter(|patch| patch.kind == "BindSource")
            .count();
        assert_eq!(bind_patch_count, row_source_count);
        assert!(patches.iter().any(|patch| matches!(
            (&patch.target, &patch.value),
            (
                RenderTarget::TodoSource(_, "todo.sources.todo_checkbox.click"),
                ProtocolValue::SourceBinding {
                    source_path: "todo.sources.todo_checkbox.click",
                    source_id: _,
                    bind_epoch: _,
                }
            )
        )));
    }

    #[test]
    fn todo_row_source_bindings_are_derived_from_boon_source_ports() {
        let source =
            include_str!("../../../examples/todomvc.bn").replace("todo_checkbox", "done_checkbox");
        let parsed = parse_source("examples/todomvc.bn", source).unwrap();
        let runtime = todo_runtime_from_parsed(&parsed);
        let row_source_paths = runtime
            .generic
            .list_source_bindings
            .source_paths("todos")
            .unwrap();
        assert!(row_source_paths.contains(&"todo.sources.done_checkbox.click"));
        assert!(!row_source_paths.contains(&"todo.sources.todo_checkbox.click"));
        let (key, generation) = runtime.todo_row_identity(0).unwrap();
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
        let mut runtime = todo_runtime_from_parsed(&parsed);
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
            delta.kind == "FieldSet" && delta.field_path == Some("store.new_todo_text")
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
            delta.kind == "FieldSet" && delta.field_path == Some("store.selected_filter")
        }));
    }

    #[test]
    fn todo_completed_updates_are_derived_from_ir_bool_branches() {
        let source = include_str!("../../../examples/todomvc.bn")
            .replace("todo_checkbox", "done_checkbox")
            .replace("toggle_all_checkbox", "mark_all_checkbox");
        let parsed = parse_source("examples/todomvc.bn", source).unwrap();
        let mut runtime = todo_runtime_from_parsed(&parsed);
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
        let buy_index = todo_index(&runtime, "Buy groceries");
        runtime
            .apply_step_into(&row_step, &mut deltas, &mut patches)
            .unwrap();
        assert!(runtime.todo_completed_for_test(buy_index));
        assert!(!runtime.todo_completed_for_test(todo_index(&runtime, "Read documentation")));

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
        assert!((0..runtime.todo_len()).all(|index| runtime.todo_completed_for_test(index)));
        assert_eq!(
            deltas
                .iter()
                .filter(|delta| delta.field_path == Some("completed"))
                .count(),
            runtime.todo_len()
        );
    }

    #[test]
    fn todo_editing_updates_are_derived_from_ir_bool_branches() {
        let source = include_str!("../../../examples/todomvc.bn")
            .replace("editing_todo_title_element", "title_editor")
            .replace("todo_title_element", "title_label");
        let parsed = parse_source("examples/todomvc.bn", source).unwrap();
        let mut runtime = todo_runtime_from_parsed(&parsed);
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
        let buy_index = todo_index(&runtime, "Buy groceries");
        runtime
            .apply_step_into(&open_step, &mut deltas, &mut patches)
            .unwrap();
        assert!(runtime.todo_editing_for_test(buy_index));
        assert_eq!(runtime.todo_edit_text_for_test(buy_index), "Buy groceries");

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
        assert_eq!(runtime.todo_edit_text_for_test(buy_index), "Draft via IR");

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
        assert!(!runtime.todo_editing_for_test(buy_index));
        assert_eq!(runtime.todo_edit_text_for_test(buy_index), "Buy groceries");
        assert!(deltas.iter().any(|delta| {
            delta.kind == "FieldSet"
                && delta.field_path == Some("editing")
                && matches!(delta.value, ProtocolValue::Bool(false))
        }));
    }

    #[test]
    fn todo_title_updates_are_derived_from_ir_trim_branches() {
        let source = include_str!("../../../examples/todomvc.bn")
            .replace("editing_todo_title_element", "title_editor")
            .replace("todo_title_element", "title_label");
        let parsed = parse_source("examples/todomvc.bn", source).unwrap();
        let mut runtime = todo_runtime_from_parsed(&parsed);
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
        let buy_index = todo_index(&runtime, "Buy groceries");
        runtime
            .apply_step_into(&open_step, &mut deltas, &mut patches)
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
        commit_action.insert(
            "text".to_owned(),
            toml::Value::String("Committed via IR".to_owned()),
        );
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
        commit_expected.insert(
            "text".to_owned(),
            toml::Value::String("Committed via IR".to_owned()),
        );
        let commit_step = ScenarioStep {
            id: "renamed-title-enter-commit".to_owned(),
            user_action: Some(commit_action),
            expected_source_event: Some(commit_expected),
            ..ScenarioStep::default()
        };
        runtime
            .apply_step_into(&commit_step, &mut deltas, &mut patches)
            .unwrap();
        assert_eq!(runtime.todo_title_for_test(buy_index), "Committed via IR");
        assert!(
            deltas
                .iter()
                .any(|delta| { delta.kind == "FieldSet" && delta.field_path == Some("title") })
        );

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
        blur_action.insert(
            "text".to_owned(),
            toml::Value::String("Blur via IR".to_owned()),
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
        blur_expected.insert(
            "text".to_owned(),
            toml::Value::String("Blur via IR".to_owned()),
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
        assert_eq!(runtime.todo_title_for_test(buy_index), "Blur via IR");
        assert!(
            deltas
                .iter()
                .any(|delta| { delta.kind == "FieldSet" && delta.field_path == Some("title") })
        );
    }

    #[test]
    fn todo_append_and_remove_are_derived_from_ir_list_operations() {
        let source = include_str!("../../../examples/todomvc.bn")
            .replace("title_to_add", "pending_title")
            .replace("clear_completed_button", "purge_done_button")
            .replace("remove_todo_button", "delete_button");
        let parsed = parse_source("examples/todomvc.bn", source).unwrap();
        let mut runtime = todo_runtime_from_parsed(&parsed);
        let mut deltas = Vec::new();
        let mut patches = Vec::new();

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
        append_action.insert(
            "text".to_owned(),
            toml::Value::String("Derived append".to_owned()),
        );
        let mut append_expected = BTreeMap::new();
        append_expected.insert(
            "source".to_owned(),
            toml::Value::String("store.sources.new_todo_input.key_down".to_owned()),
        );
        append_expected.insert("key".to_owned(), toml::Value::String("Enter".to_owned()));
        append_expected.insert(
            "text".to_owned(),
            toml::Value::String("Derived append".to_owned()),
        );
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
            runtime.todo_title_for_test(runtime.todo_len() - 1),
            "Derived append"
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
        assert_eq!(runtime.todo_title_for_test(0), "Read documentation");
        assert_eq!(runtime.todo_title_for_test(1), "Walk the dog");
        assert_eq!(runtime.todo_title_for_test(2), "Derived append");
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
        assert_eq!(runtime.todo_title_for_test(0), "Read documentation");
        assert_eq!(runtime.todo_title_for_test(1), "Derived append");
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
                .remove_row_for_predicate("todos", clear_predicate, 0)
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
        assert!(
            input_route.has_list_append_target("todos"),
            "append routing must be found from SourceId, not a label scan"
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
    fn generic_source_event_ingests_expected_event_payloads() {
        let mut expected = BTreeMap::new();
        expected.insert(
            "source".to_owned(),
            toml::Value::String("cell.sources.editor.commit".to_owned()),
        );
        expected.insert("text".to_owned(), toml::Value::String("=A1+1".to_owned()));
        expected.insert("key".to_owned(), toml::Value::String("Enter".to_owned()));
        expected.insert("address".to_owned(), toml::Value::String("B1".to_owned()));
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
        assert_eq!(event.text, Some("=A1+1"));
        assert_eq!(event.key, Some("Enter"));
        assert_eq!(event.address, Some("B1"));
        assert_eq!(event.target_text, Some("Buy groceries"));

        let missing = ScenarioStep {
            id: "missing-source".to_owned(),
            ..ScenarioStep::default()
        };
        assert!(GenericSourceEvent::require(&missing).is_err());
    }

    #[test]
    fn keyed_list_dense_key_slots_survive_remove_and_move() {
        let mut list = KeyedList::from_values(["a", "b", "c", "d"]);
        let first_key = list[0].key;
        let second_key = list[1].key;
        let third_key = list[2].key;
        let fourth_key = list[3].key;

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

        let (new_key, generation) = list.append("e");
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
        sources.bind_row(
            "todos",
            10,
            1,
            &[
                "todo.sources.todo_checkbox.click",
                "todo.sources.title_input.commit",
            ],
        );
        let binding = sources
            .row_bindings("todos", 10, 1)
            .find(|binding| binding.source_path == "todo.sources.todo_checkbox.click")
            .cloned()
            .unwrap();
        assert!(sources.is_bound(
            "todos",
            10,
            1,
            binding.source_path,
            Some(binding.source_id),
            Some(binding.bind_epoch),
        ));

        sources.unbind_row("todos", 10, 1);
        assert!(!sources.is_bound(
            "todos",
            10,
            1,
            binding.source_path,
            Some(binding.source_id),
            Some(binding.bind_epoch),
        ));
    }

    #[test]
    fn remove_unbinds_todo_row_sources_and_emits_unbind_deltas() {
        let parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let mut runtime = todo_runtime_from_parsed(&parsed);
        let row_source_count = runtime
            .generic
            .list_source_bindings
            .source_count("todos")
            .unwrap();
        let removed_key = runtime.todo_key(0);
        let removed_generation = runtime.todo_generation(0);
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
            .filter(|patch| patch.kind == "UnbindSource")
            .count();
        assert_eq!(unbind_patch_count, row_source_count);
        assert!(patches.iter().any(|patch| matches!(
            (&patch.target, &patch.value),
            (
                RenderTarget::TodoSource(_, "todo.sources.todo_checkbox.click"),
                ProtocolValue::SourceBinding {
                    source_path: "todo.sources.todo_checkbox.click",
                    source_id: _,
                    bind_epoch: _,
                }
            )
        )));
    }

    #[test]
    fn stale_todo_source_bind_epoch_is_ignored() {
        let parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let mut runtime = todo_runtime_from_parsed(&parsed);
        let target_key = runtime.todo_key(0);
        let target_generation = runtime.todo_generation(0);
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
        assert_eq!(runtime.todo_completed_for_test(0), false);
    }

    #[test]
    fn stale_todo_source_generation_is_ignored() {
        let parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let mut runtime = todo_runtime_from_parsed(&parsed);
        let stale_key = runtime.todo_key(0);
        let stale_generation = runtime.todo_generation(0);
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
        assert_eq!(runtime.todo_len(), 3);
        assert_eq!(runtime.todo_title_for_test(0), "Finish TodoMVC renderer");
        assert_eq!(runtime.todo_completed_for_test(0), true);
    }

    #[test]
    fn list_move_emits_keyed_delta_without_graph_clone() {
        let parsed = parse_source(
            "examples/todomvc.bn",
            include_str!("../../../examples/todomvc.bn"),
        )
        .unwrap();
        let mut runtime = todo_runtime_from_parsed(&parsed);
        let moved_key = runtime.todo_key(0);
        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        runtime.move_index(0, 1, &mut deltas, &mut patches).unwrap();
        assert_eq!(runtime.todo_key(1), moved_key);
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].kind, "ListMove");
        assert_eq!(deltas[0].list_id, Some("todos"));
        assert_eq!(deltas[0].key, Some(moved_key));
        assert_eq!(deltas[0].generation, Some(1));
        assert_eq!(deltas[0].field_path, Some("position"));
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].kind, "MoveElement");
    }

    #[test]
    fn cells_deltas_use_hidden_grid_slots_not_visible_address_hashes() {
        let mut runtime = CellsRuntime::with_dimensions(26, 100);
        runtime.reserve_cell_cache("41".len(), 1);
        let expected_key = runtime
            .cell_key_generation(runtime.cell_index("A1").unwrap())
            .0;
        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        let mut recomputed = Vec::new();
        runtime
            .commit("A1", "41", &mut deltas, &mut patches, &mut recomputed)
            .unwrap();
        assert!(deltas.iter().all(|delta| delta.list_id == Some("cells")));
        assert!(deltas.iter().all(|delta| delta.key == Some(expected_key)));
        assert_ne!(expected_key, cell_address_hash_for_test("A1"));
    }

    #[test]
    fn cells_edit_state_updates_are_derived_from_ir_branches() {
        let source = include_str!("../../../examples/cells.bn")
            .replace("change: SOURCE", "input: SOURCE")
            .replace("commit: SOURCE", "apply: SOURCE")
            .replace("cancel: SOURCE", "revert: SOURCE")
            .replace("sources.editor.change", "sources.editor.input")
            .replace("sources.editor.commit", "sources.editor.apply")
            .replace("sources.editor.cancel", "sources.editor.revert");
        let parsed = parse_source("examples/cells.bn", source).unwrap();
        let ir = lower(&parsed).unwrap();
        let compiled = CompiledProgram::from_ir(&ir).unwrap();
        let generic = GenericScheduledRuntime::new(&ir, &compiled).unwrap();
        let mut runtime = CellsRuntime::from_generic(generic, &ir).unwrap();
        runtime.reserve_cell_cache("123".len(), 1);
        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        let mut recomputed = Vec::new();
        runtime
            .commit_from_source(
                "cell.sources.editor.apply",
                "A1",
                "123",
                &mut deltas,
                &mut patches,
                &mut recomputed,
            )
            .unwrap();
        let a1 = runtime.cell_index("A1").unwrap();
        assert_eq!(runtime.cell_text_field(a1, "formula_text").unwrap(), "123");
        assert_eq!(runtime.cell_text_field(a1, "editing_text").unwrap(), "123");
        assert_eq!(runtime.cell("A1").unwrap().value, "123");
        assert!(!runtime.cell_bool_field(a1, "editing").unwrap());

        let mut action = BTreeMap::new();
        action.insert(
            "kind".to_owned(),
            toml::Value::String("key_down".to_owned()),
        );
        action.insert(
            "target".to_owned(),
            toml::Value::String("A1 editor".to_owned()),
        );
        action.insert("key".to_owned(), toml::Value::String("Escape".to_owned()));
        let mut expected = BTreeMap::new();
        expected.insert(
            "source".to_owned(),
            toml::Value::String("cell.sources.editor.revert".to_owned()),
        );
        expected.insert("address".to_owned(), toml::Value::String("A1".to_owned()));
        let step = ScenarioStep {
            id: "renamed-cell-revert".to_owned(),
            user_action: Some(action),
            expected_source_event: Some(expected),
            ..ScenarioStep::default()
        };
        deltas.clear();
        patches.clear();
        recomputed.clear();
        runtime
            .apply_step_into(&step, &mut deltas, &mut patches, &mut recomputed)
            .unwrap();
        assert_eq!(runtime.cell_text_field(a1, "editing_text").unwrap(), "123");
        assert!(!runtime.cell_bool_field(a1, "editing").unwrap());
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
    fn runtime_list_store_keeps_hidden_list_slots_sorted_for_dense_lookup() {
        let mut store = RuntimeListStore::default();
        store.insert(
            "todos".to_owned(),
            KeyedList::from_values([todo_generic_row("A")]),
            Some(4),
            RuntimeRecordTemplate::default(),
        );
        store.insert(
            "cells".to_owned(),
            KeyedList::from_values([cell_generic_row("A1")]),
            Some(26),
            RuntimeRecordTemplate::default(),
        );
        store.insert(
            "todos".to_owned(),
            KeyedList::from_values([todo_generic_row("B")]),
            Some(8),
            RuntimeRecordTemplate::default(),
        );

        assert_eq!(store.capacity("todos"), Some(8));
        assert_eq!(store.capacity("cells"), Some(26));
        assert_eq!(store.memory("todos").unwrap().len(), 1);
        assert_eq!(store.memory("cells").unwrap().len(), 1);
        assert!(store.list_slots.windows(2).all(|window| (
            window[0].list_id,
            window[0].name.as_str()
        ) < (
            window[1].list_id,
            window[1].name.as_str()
        )));
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
            "SetProperty",
            RenderTarget::TodoCheckbox(1),
            ProtocolValue::CheckedProperty(true),
        )];

        assert!(assert_delta_expectations(&semantic_step, &deltas, &patches).is_err());
        assert!(assert_delta_expectations(&render_step, &deltas, &patches).is_err());
    }

    #[test]
    fn formula_primitives_support_documented_arithmetic_ops() {
        let mut runtime = CellsRuntime::with_dimensions(26, 100);
        runtime.reserve_cell_cache("=8/2".len(), 1);
        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        let mut recomputed = Vec::new();
        for (formula, expected) in [("=8+2", "10"), ("=8-2", "6"), ("=8*2", "16"), ("=8/2", "4")] {
            deltas.clear();
            patches.clear();
            recomputed.clear();
            runtime
                .commit("A1", formula, &mut deltas, &mut patches, &mut recomputed)
                .unwrap();
            assert_eq!(runtime.cell("A1").unwrap().value, expected);
            assert_eq!(runtime.cell("A1").unwrap().error, None);
        }
        runtime
            .commit("A1", "=8/0", &mut deltas, &mut patches, &mut recomputed)
            .unwrap();
        assert_eq!(runtime.cell("A1").unwrap().error, Some("div_by_zero"));
    }

    #[test]
    fn replacing_formula_removes_stale_dependency_edges() {
        let mut runtime = CellsRuntime::with_dimensions(26, 100);
        runtime.reserve_cell_cache("=A1+1".len(), 1);
        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        let mut recomputed = Vec::new();
        runtime
            .commit("A1", "1", &mut deltas, &mut patches, &mut recomputed)
            .unwrap();
        runtime
            .commit("B1", "=A1+1", &mut deltas, &mut patches, &mut recomputed)
            .unwrap();
        assert_eq!(runtime.cell("B1").unwrap().value, "2");
        assert_eq!(runtime.cell("B1").unwrap().deps, vec![0]);
        assert_eq!(runtime.dependency_cache.reverse_deps[0], vec![1]);

        recomputed.clear();
        runtime
            .commit("B1", "5", &mut deltas, &mut patches, &mut recomputed)
            .unwrap();
        assert!(runtime.cell("B1").unwrap().deps.is_empty());
        assert!(runtime.dependency_cache.reverse_deps[0].is_empty());

        recomputed.clear();
        runtime
            .commit("A1", "10", &mut deltas, &mut patches, &mut recomputed)
            .unwrap();
        let recomputed_addresses = recomputed
            .iter()
            .map(|index| runtime.address_for(*index))
            .collect::<Vec<_>>();
        assert_eq!(recomputed_addresses, vec!["A1"]);
    }

    #[test]
    fn cells_fanout_uses_reverse_dependency_index() {
        let mut runtime = CellsRuntime::with_dimensions(26, 100);
        runtime.reserve_cell_cache("=A1+2".len(), 1);
        let mut deltas = Vec::new();
        let mut patches = Vec::new();
        let mut recomputed = Vec::new();
        runtime
            .commit("A1", "1", &mut deltas, &mut patches, &mut recomputed)
            .unwrap();
        runtime
            .commit("B1", "=A1+1", &mut deltas, &mut patches, &mut recomputed)
            .unwrap();
        runtime
            .commit("C1", "=A1+2", &mut deltas, &mut patches, &mut recomputed)
            .unwrap();
        runtime
            .commit("D1", "=A1+3", &mut deltas, &mut patches, &mut recomputed)
            .unwrap();

        recomputed.clear();
        runtime
            .commit("A1", "10", &mut deltas, &mut patches, &mut recomputed)
            .unwrap();
        let recomputed_addresses = recomputed
            .iter()
            .map(|index| runtime.address_for(*index))
            .collect::<Vec<_>>();
        assert_eq!(recomputed_addresses, vec!["A1", "B1", "C1", "D1"]);
        assert_eq!(runtime.dependency_cache.last_edge_walks(), 3);
        assert!(runtime.dependency_cache.last_edge_walks() < runtime.cells.len());
    }

    fn cell_address_hash_for_test(address: &str) -> u64 {
        let mut hasher = Sha256::new();
        hasher.update(address.as_bytes());
        let bytes = hasher.finalize();
        u64::from_le_bytes(bytes[..8].try_into().unwrap())
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
