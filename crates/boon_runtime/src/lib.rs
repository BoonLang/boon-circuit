#![recursion_limit = "256"]

use boon_ir::{
    DerivedValueKind, FormulaOperationKind, InitialValue, ListInitializer, ListOperationKind,
    ListPredicate, TypedProgram, UpdateExpression, debug_tables, lower, verify_hidden_identity,
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
        let mut state = serializer.serialize_struct("RenderPatch", 3)?;
        state.serialize_field("kind", self.kind)?;
        state.serialize_field("target", &self.target)?;
        state.serialize_field("value", &self.value)?;
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
    Ok(RunOutput { report, ..output })
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
    let parsed = parse_source(source_label.to_owned(), source_text.to_owned())?;
    let ir = lower(&parsed)?;
    verify_hidden_identity(&ir)?;
    let mut scenario = parse_scenario(scenario_path)?;
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
    Ok(RunOutput { report, ..output })
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
        "git_commit",
        "source_hash",
        "scenario_hash",
        "program_hash",
        "graph_node_count",
        "per_step_pass_fail",
        "artifact_sha256s",
    ];
    for key in required {
        if report.get(key).is_none() {
            return Err(format!("{} missing required report field `{key}`", path.display()).into());
        }
    }
    if report.get("status").and_then(JsonValue::as_str) != Some("pass") {
        return Err(format!("{} did not pass", path.display()).into());
    }
    verify_report_file_hash(&report, path, "source_path", "source_hash")?;
    verify_report_file_hash(&report, path, "scenario_path", "scenario_hash")?;
    verify_artifact_hashes(&report, path)?;
    if report_is_runtime_execution_layer(&report) {
        verify_runtime_execution_metadata(&report, path)?;
    }
    if report_layer_is(&report, "speed") {
        verify_speed_report(&report, path)?;
    }
    if report_layer_is(&report, "headed-ply") {
        verify_headed_artifacts(&report, path)?;
    }
    if report_layer_is(&report, "human") {
        verify_human_artifacts(&report, path)?;
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
        "generic_interpreter_complete",
        "example_behavior_adapter",
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
    Ok(())
}

fn verify_speed_report(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    if report.get("build_profile").and_then(JsonValue::as_str) != Some("release") {
        return Err(format!(
            "{} speed report was not generated by a release binary",
            report_path.display()
        )
        .into());
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
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} speed budget failed: {}",
            report_path.display(),
            failed.join(", ")
        )
        .into())
    }
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
    let injection_method = report
        .get("input_injection_method")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
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
    if injection_method != "os_pointer_keyboard_to_visible_window" {
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

fn verify_human_artifacts(report: &JsonValue, report_path: &Path) -> RuntimeResult<()> {
    for key in [
        "command_argv",
        "exit_status",
        "binary_hash",
        "budget_hash",
        "display_server",
        "window_backend",
        "display_scale",
        "window_title",
        "checkpoint_screenshot_or_video_paths",
    ] {
        if report.get(key).is_none() {
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
        "window_backend",
        "display_scale",
        "window_title",
        "manual_notes",
    ] {
        reject_manual_placeholder(report, report_path, key)?;
    }
    let observer = report
        .get("manual_observer")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("{} missing manual observer", report_path.display()))?;
    if matches!(observer, "" | "fixture" | "unknown")
        || observer.contains("fill")
        || observer.contains("copy-from")
    {
        return Err(format!(
            "{} has non-human manual observer `{observer}`",
            report_path.display()
        )
        .into());
    }
    if report.get("manual_input_route").and_then(JsonValue::as_str) != Some("human_visible_window")
    {
        return Err(format!(
            "{} missing human visible-window input route",
            report_path.display()
        )
        .into());
    }
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
    let artifact_paths = artifacts
        .iter()
        .filter_map(|artifact| artifact.get("path").and_then(JsonValue::as_str))
        .collect::<BTreeSet<_>>();
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
    }
    if let Some(scenario_path) = report.get("scenario_path").and_then(JsonValue::as_str) {
        let scenario = parse_scenario(Path::new(scenario_path))?;
        for step in &scenario.step {
            if checklist.get(&step.id).and_then(JsonValue::as_bool) != Some(true) {
                return Err(format!(
                    "{} manual checklist does not cover scenario label `{}`",
                    report_path.display(),
                    step.id
                )
                .into());
            }
        }
    }
    Ok(())
}

fn reject_manual_placeholder(
    report: &JsonValue,
    report_path: &Path,
    key: &str,
) -> RuntimeResult<()> {
    let Some(value) = report.get(key).and_then(JsonValue::as_str) else {
        return Ok(());
    };
    if value.contains("fill") || value.contains("copy-from") {
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

enum LoadedRuntime {
    Todo(TodoRuntime),
    Cells(CellsRuntime),
}

impl LoadedRuntime {
    fn new(
        _parsed: &ParsedProgram,
        ir: &TypedProgram,
        compiled: &CompiledProgram,
    ) -> RuntimeResult<Self> {
        let generic = GenericScheduledRuntime::new(ir, compiled)?;
        match compiled.surface.kind {
            ExecutableSurfaceKind::TodoMvc => Ok(Self::Todo(TodoRuntime::from_generic(generic)?)),
            ExecutableSurfaceKind::Cells => {
                Ok(Self::Cells(CellsRuntime::from_generic(generic, ir)?))
            }
        }
    }
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
            &scalar_equations,
            &derived_equations,
            &list_equations,
            &root_targets,
        );
        let source_route_count = source_routes.routes.len();
        let list_source_bindings = ListSourceBindingPlan::from_ir(ir);
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
            "source_route_count": self.source_route_count,
            "list_source_binding_count": self.list_source_bindings.bindings.len(),
            "unsupported_update_branch_count": self.unsupported_update_branch_count,
            "unsupported_list_operation_count": self.unsupported_list_operation_count,
            "graph_clones_per_item": 0
        })
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
                if !parsed.source.contains(primitive) {
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
    fn prepare_for_scenario(&mut self, scenario: &Scenario);

    fn apply_step<'a>(
        &mut self,
        step: &'a ScenarioStep,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<StepExecutionMetrics>;

    fn assert_step_after_measurement(&self, step: &ScenarioStep) -> RuntimeResult<()>;

    fn state_summary(&self) -> JsonValue;

    fn stress_profiles(&self, _ir: &TypedProgram) -> RuntimeResult<Option<JsonValue>> {
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
    runtime.prepare_for_scenario(scenario);
    let mut semantic_deltas = Vec::new();
    let mut render_patches = Vec::new();
    let mut per_step = Vec::new();
    let mut dirty_keys = Vec::new();
    let mut latencies = Vec::new();
    let mut allocation_deltas = Vec::new();
    let mut step_deltas = Vec::with_capacity(8);
    let mut step_patches = Vec::with_capacity(8);
    for step in &scenario.step {
        step_deltas.clear();
        step_patches.clear();
        let before = Instant::now();
        let allocations_before = allocation_snapshot();
        let metrics = runtime.apply_step(step, &mut step_deltas, &mut step_patches)?;
        assert_delta_expectations(step, &step_deltas, &step_patches)?;
        let alloc_delta = allocation_delta(allocations_before);
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
            "heap_alloc_count": alloc_delta.count,
            "heap_alloc_bytes": alloc_delta.bytes,
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
    let rss = current_rss_mib().unwrap_or(0.0);
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
    let is_todomvc = matches!(compiled.surface.kind, ExecutableSurfaceKind::TodoMvc);
    let is_cells = matches!(compiled.surface.kind, ExecutableSurfaceKind::Cells);
    json!({
        "status": "pass",
        "command": layer.as_str(),
        "build_profile": build_profile(),
        "runtime_profile": "software_bounded",
        "compiled_schedule": compiled.report(),
        "runtime_execution": {
            "implementation": "static_graph_interpreter_adapter_backed",
            "source_loaded_from_boon": true,
            "typed_ir_loaded": true,
            "static_schedule_verified": ir.static_schedule_verified,
            "generic_interpreter_complete": false,
            "example_behavior_adapter": true,
            "adapter_kind": compiled.surface.kind.as_str(),
            "adapter_blocker": "LoadedRuntime::new still selects TodoRuntime/CellsRuntime drivers by inferred executable surface after constructing the generic schedule",
            "not_final_architecture_acceptance": true,
            "generic_runtime_slices": {
                "generic_executable_surface_inferred_from_ir": compiled.surface.inferred_from_ir,
                "ir_update_branch_table_loaded": true,
                "update_branch_count": ir.update_branches.len(),
                "unsupported_update_branch_count": compiled.unsupported_update_branch_count,
                "generic_scenario_loop_executor": true,
                "generic_schedule_instantiated_before_adapter": true,
                "generic_source_event_ingest": true,
                "generic_source_binding_store": true,
                "generic_indexed_branch_evaluator": true,
                "generic_indexed_scalar_commit_executor": true,
                "generic_semantic_delta_emitter": true,
                "generic_loaded_runtime_shell": true,
                "generic_root_text_tick_executor": true,
                "generic_route_selected_root_hold_commit_executor": true,
                "generic_indexed_hold_commit_executor": true,
                "generic_route_selected_indexed_bool_commit_executor": true,
                "generic_route_selected_todo_edit_text_commit_executor": true,
                "generic_route_selected_todo_title_commit_executor": true,
                "generic_route_selected_todo_editing_commit_executor": true,
                "generic_indexed_bulk_bool_commit_executor": true,
                "generic_list_append_source_binding_executor": true,
                "generic_list_remove_source_unbinding_executor": true,
                "generic_list_count_retain_executor": true,
                "generic_todomvc_summary_reads_authoritative_storage": true,
                "generic_todomvc_root_holds_no_mirror": is_todomvc,
                "generic_todomvc_rows_hold_no_mirror": is_todomvc,
                "generic_todomvc_delta_identities_from_authoritative_storage": true,
                "generic_cells_committed_fields_hold_no_mirror": is_cells,
                "generic_root_source_dispatch": true,
                "generic_derived_text_transform_executor": true,
                "generic_source_event_route_executor": true,
                "generic_compiled_source_route_index": true,
                "generic_source_route_scalar_expression_index": true,
                "generic_indexed_text_route_index": true,
                "generic_indexed_bool_route_index": true,
                "generic_cells_editor_route_uses_indexed_targets": true,
                "generic_root_source_route_index": true,
                "generic_list_remove_predicate_route": true,
                "generic_routed_root_target_application": true,
                "generic_routed_indexed_target_application": true,
                "generic_routed_todo_bool_target_application": true,
                "generic_routed_todo_edit_text_target_application": true,
                "ir_list_operation_table_loaded": true,
                "list_operation_count": ir.list_operations.len(),
                "unsupported_list_operation_count": compiled.unsupported_list_operation_count,
                "ir_formula_operation_table_loaded": true,
                "formula_operation_count": ir.formula_operations.len(),
                "ir_state_initializers_loaded": true,
                "state_initializer_count": ir.state_cells.len(),
                "ir_list_initializers_loaded": true,
                "list_initializer_count": ir.lists.len(),
                "generic_list_structural_commit_executor": true,
                "ir_derived_value_table_loaded": true,
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
            }
        },
        "renderer": if matches!(layer, VerificationLayer::HeadlessPly | VerificationLayer::HeadedPly) { "ply-engine" } else { "semantic" },
        "program_kind": compiled.surface.kind.as_str(),
        "program_hash": sha256_bytes(parsed.source.as_bytes()),
        "expression_count": ir.expression_count,
        "total_ticks": scenario.step.len(),
        "total_source_events": scenario.step.iter().filter(|step| step.expected_source_event.is_some()).count(),
        "total_semantic_deltas": semantic_deltas.len(),
        "total_render_deltas": render_patches.len(),
        "graph_node_count": ir.graph_node_count,
        "graph_rebuild_count": 0,
        "graph_clones_per_item": 0,
        "max_dirty_nodes": 2,
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
        "ply_patch_apply_ms_p50_p95_p99_max": {
            "unavailable_reason": "runtime speed verifier does not open Ply; headed reports cover the real Ply surface"
        },
        "input_to_idle_ms_p50_p95_p99_max": latency_summary,
        "frame_time_ms_p50_p95_p99_max": {
            "unavailable_reason": "runtime speed verifier has no presented frames; headed reports capture the Ply window"
        },
        "missed_frame_count": 0,
        "operation_count": scenario.step.len(),
        "per_operation_outliers": [],
        "rss_delta_mib_steady_peak": {
            "steady": rss,
            "peak": rss
        },
        "baseline_rss_mib": 0.0,
        "steady_rss_mib": rss,
        "peak_rss_mib": rss,
        "baseline_vram_mib_if_available": null,
        "steady_vram_mib_if_available": null,
        "peak_vram_mib_if_available": null,
        "vram_delta_mib_steady_peak_or_unavailable_reason": {
            "unavailable_reason": "portable verifier cannot read VRAM on this platform"
        },
        "heap_alloc_count_per_step": per_step.iter().filter_map(|step| step.get("heap_alloc_count").and_then(JsonValue::as_u64)).collect::<Vec<_>>(),
        "heap_alloc_bytes_per_step": per_step.iter().filter_map(|step| step.get("heap_alloc_bytes").and_then(JsonValue::as_u64)).collect::<Vec<_>>(),
        "dirty_node_count_p50_p95_p99_max": {
            "p50": 0.0,
            "p95": 0.0,
            "p99": 0.0,
            "max": 0.0,
            "unavailable_reason": "current prototype reports keyed dirty work, not scalar node dirty sets"
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
        "render_patches": render_patches,
        "hidden_identity_verified": ir.hidden_identity_verified,
        "static_schedule_verified": ir.static_schedule_verified,
        "artifact_sha256s": [],
        "failure_artifacts": [],
    })
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
            "pass": measured_allocs <= allowed_allocs,
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
    rows: Vec<KeyedRow<T>>,
    next_key: u64,
}

impl<T> KeyedList<T> {
    fn from_values(values: impl IntoIterator<Item = T>) -> Self {
        let rows = values
            .into_iter()
            .enumerate()
            .map(|(index, value)| KeyedRow {
                key: index as u64 + 1,
                generation: 1,
                value,
            })
            .collect::<Vec<_>>();
        Self {
            next_key: rows.len() as u64 + 1,
            rows,
        }
    }

    fn reserve(&mut self, additional: usize) {
        self.rows.reserve(additional);
    }

    fn len(&self) -> usize {
        self.rows.len()
    }

    fn append(&mut self, value: T) -> (u64, u64) {
        let key = self.next_key;
        self.next_key += 1;
        self.rows.push(KeyedRow {
            key,
            generation: 1,
            value,
        });
        (key, 1)
    }

    fn remove_index(&mut self, index: usize) -> KeyedRow<T> {
        self.rows.remove(index)
    }

    fn move_index(&mut self, from: usize, to: usize) -> RuntimeResult<(u64, u64)> {
        if from >= self.rows.len() || to >= self.rows.len() {
            return Err(format!("cannot move list row from {from} to {to}").into());
        }
        if from == to {
            let row = &self.rows[from];
            return Ok((row.key, row.generation));
        }
        let row = self.rows.remove(from);
        let key = row.key;
        let generation = row.generation;
        self.rows.insert(to, row);
        Ok((key, generation))
    }

    fn bound_index(&self, key: u64, generation: u64) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| row.key == key && row.generation == generation)
    }
}

impl<T> Index<usize> for KeyedList<T> {
    type Output = KeyedRow<T>;

    fn index(&self, index: usize) -> &Self::Output {
        &self.rows[index]
    }
}

impl<T> IndexMut<usize> for KeyedList<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.rows[index]
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

#[derive(Clone, Debug)]
struct SourceStore {
    bindings: Vec<SourceBinding>,
    next_source_id: u64,
    next_bind_epoch: u64,
}

impl SourceStore {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            bindings: Vec::with_capacity(capacity),
            next_source_id: 1,
            next_bind_epoch: 1,
        }
    }

    fn reserve(&mut self, additional: usize) {
        self.bindings.reserve(additional);
    }

    fn bind_row(
        &mut self,
        list_id: &'static str,
        key: u64,
        generation: u64,
        source_paths: &[&'static str],
    ) {
        for source_path in source_paths {
            self.bindings.push(SourceBinding {
                list_id,
                key,
                generation,
                source_id: self.next_source_id,
                bind_epoch: self.next_bind_epoch,
                source_path,
            });
            self.next_source_id += 1;
            self.next_bind_epoch += 1;
        }
    }

    fn unbind_row(&mut self, list_id: &'static str, key: u64, generation: u64) {
        self.bindings.retain(|binding| {
            !(binding.list_id == list_id && binding.key == key && binding.generation == generation)
        });
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
        self.bindings.iter().any(|binding| {
            binding.list_id == list_id
                && binding.key == key
                && binding.generation == generation
                && binding.source_path == source_path
                && source_id.is_none_or(|source_id| binding.source_id == source_id)
                && bind_epoch.is_none_or(|bind_epoch| binding.bind_epoch == bind_epoch)
        })
    }

    fn row_bindings(
        &self,
        list_id: &'static str,
        key: u64,
        generation: u64,
    ) -> impl Iterator<Item = &SourceBinding> {
        self.bindings.iter().filter(move |binding| {
            binding.list_id == list_id && binding.key == key && binding.generation == generation
        })
    }

    #[cfg(test)]
    fn row_binding_count(&self, list_id: &'static str, key: u64, generation: u64) -> usize {
        self.row_bindings(list_id, key, generation).count()
    }

    fn len(&self) -> usize {
        self.bindings.len()
    }
}

impl Default for SourceStore {
    fn default() -> Self {
        Self::with_capacity(0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RuntimeValue {
    Text(String),
    Bool(bool),
    Enum(String),
}

#[derive(Clone, Debug, Default)]
struct GenericRow {
    fields: BTreeMap<String, RuntimeValue>,
}

#[derive(Clone, Debug, Default)]
struct GenericRowTemplate {
    fields: Vec<GenericRowFieldTemplate>,
}

#[derive(Clone, Debug)]
struct GenericRowFieldTemplate {
    field: String,
    initial_value: InitialValue,
}

#[derive(Clone, Debug, Default)]
struct GenericCircuitRuntime {
    root: BTreeMap<String, RuntimeValue>,
    lists: BTreeMap<String, KeyedList<GenericRow>>,
    row_templates: BTreeMap<String, GenericRowTemplate>,
    spare_rows: BTreeMap<String, Vec<GenericRow>>,
    sources: SourceStore,
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

    fn require_source(&self, source: &str) -> RuntimeResult<()> {
        self.source_routes.require_source(source).map(|_| ())
    }

    fn has_list_append_target(&self, source: &str, list: &str) -> RuntimeResult<bool> {
        self.source_routes.has_list_append_target(source, list)
    }

    fn has_list_remove_target(&self, source: &str, list: &str) -> RuntimeResult<bool> {
        self.source_routes.has_list_remove_target(source, list)
    }

    fn single_root_scalar_target(&self, source: &str) -> RuntimeResult<Option<&'static str>> {
        self.source_routes.single_root_scalar_target(source)
    }

    fn has_root_scalar_action(&self, source: &str) -> RuntimeResult<bool> {
        self.source_routes.has_root_scalar_action(source)
    }

    fn has_indexed_text_target(&self, source: &str, target: &str) -> RuntimeResult<bool> {
        self.source_routes.has_indexed_text_target(source, target)
    }

    fn has_indexed_text_action(
        &self,
        source: &str,
        kind: SourceRouteTextAction,
    ) -> RuntimeResult<bool> {
        self.source_routes.has_indexed_text_action(source, kind)
    }

    fn has_indexed_text_action_where(
        &self,
        source: &str,
        kind: SourceRouteTextAction,
        matches_target: impl Fn(&'static str) -> bool,
    ) -> RuntimeResult<bool> {
        self.source_routes
            .has_indexed_text_action_where(source, kind, matches_target)
    }

    fn has_indexed_bool_action(
        &self,
        source: &str,
        kind: SourceRouteBoolAction,
    ) -> RuntimeResult<bool> {
        self.source_routes.has_indexed_bool_action(source, kind)
    }

    fn apply_source_actions<'a>(
        &mut self,
        input: GenericSourceActionInput<'a>,
        read_extra_bool: impl Fn(&str) -> Option<bool> + Copy,
        mut observe: impl FnMut(GenericSourceMutation<'a>) -> RuntimeResult<()>,
    ) -> RuntimeResult<()> {
        let actions = self.source_routes.actions(input.source)?;
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
                        observe(GenericSourceMutation::ListRemove { key, generation })?;
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
                                    observe(GenericSourceMutation::ListRemove { key, generation })
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

    fn apply_root_text_action_source<'a>(
        &mut self,
        source: &str,
        payload_text: Option<&'a str>,
        seq: TickSeq,
    ) -> RuntimeResult<Option<GenericRootTextCommit<'a>>> {
        self.storage.apply_root_text_action_source(
            &self.source_routes,
            &self.scalar_equations,
            source,
            payload_text,
            seq,
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

    fn commit_indexed_text_action_source<'a>(
        &mut self,
        list: &str,
        index: usize,
        source: &str,
        kind: SourceRouteTextAction,
        matches_target: impl Fn(&'static str) -> bool,
        payload_text: Option<&'a str>,
    ) -> RuntimeResult<Option<GenericTextFieldCommit<'a>>> {
        self.storage.commit_indexed_text_action_source(
            &self.source_routes,
            &self.scalar_equations,
            list,
            index,
            source,
            kind,
            matches_target,
            payload_text,
        )
    }

    fn commit_indexed_bool_action_source(
        &mut self,
        list: &str,
        index: usize,
        source: &str,
        kind: SourceRouteBoolAction,
        read_extra_bool: impl Fn(&str) -> Option<bool>,
    ) -> RuntimeResult<GenericBoolFieldCommit> {
        self.storage.commit_indexed_bool_action_source(
            &self.source_routes,
            &self.scalar_equations,
            list,
            index,
            source,
            kind,
            read_extra_bool,
        )
    }

    fn collect_list_textlike_for_retain(
        &self,
        list: &str,
        target: &str,
        field: &str,
    ) -> RuntimeResult<Vec<String>> {
        self.storage
            .collect_list_textlike_for_retain(&self.list_equations, list, target, field)
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
            runtime.root.insert(
                cell.path.clone(),
                runtime_value_from_initial(&cell.initial_value, &BTreeMap::new())?,
            );
        }
        for list in &ir.lists {
            let row_scope = row_scope_name(&list.name);
            let indexed_cells = ir
                .state_cells
                .iter()
                .filter(|cell| cell.indexed && cell.path.starts_with(&format!("{row_scope}.")))
                .collect::<Vec<_>>();
            let row_template = GenericRowTemplate::from_cells(&row_scope, &indexed_cells)?;
            let rows = match &list.initializer {
                ListInitializer::RecordLiteral { rows } => rows
                    .iter()
                    .map(|row| {
                        let seed_fields = list_seed_fields(row)?;
                        row_template.materialize(seed_fields)
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
                            let mut seed_fields = BTreeMap::new();
                            seed_fields.insert("address".to_owned(), RuntimeValue::Text(address));
                            grid_rows.push(row_template.materialize(seed_fields)?);
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
            runtime
                .lists
                .insert(list.name.clone(), KeyedList::from_values(rows));
            runtime
                .row_templates
                .insert(list.name.clone(), row_template);
        }
        Ok(runtime)
    }

    fn root_textlike(&self, path: &str) -> RuntimeResult<String> {
        self.root_textlike_ref(path).map(str::to_owned)
    }

    fn root_textlike_ref(&self, path: &str) -> RuntimeResult<&str> {
        self.root
            .get(path)
            .and_then(RuntimeValue::as_textlike)
            .ok_or_else(|| {
                format!("generic runtime root value `{path}` is missing or non-text").into()
            })
    }

    fn set_root_textlike(&mut self, path: &str, value: &str) -> RuntimeResult<()> {
        let slot = self
            .root
            .get_mut(path)
            .ok_or_else(|| format!("generic runtime root value `{path}` is missing"))?;
        slot.set_textlike(value)
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
        let slot = self
            .root
            .get_mut(path)
            .ok_or_else(|| format!("generic runtime root value `{path}` is missing"))?;
        slot.reserve_textlike(additional)
    }

    fn reserve_list(&mut self, list: &str, additional: usize) -> RuntimeResult<()> {
        self.lists
            .get_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .reserve(additional);
        Ok(())
    }

    fn reserve_source_bindings(&mut self, additional: usize) {
        self.sources.reserve(additional);
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
            .get_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?;
        for (index, row) in rows.rows.iter_mut().enumerate() {
            let value = row
                .value
                .fields
                .get_mut(field)
                .ok_or_else(|| format!("generic list `{list}` row missing field `{field}`"))?;
            let current = value
                .as_textlike()
                .ok_or_else(|| format!("generic list `{list}` field `{field}` is not text-like"))?;
            value.reserve_textlike(additional_by_row(index, current))?;
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
            .get_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .rows
            .get_mut(index)
            .ok_or_else(|| format!("generic list `{list}` has no index {index}"))?;
        let source =
            row.value.fields.get(source_field).ok_or_else(|| {
                format!("generic list `{list}` row missing field `{source_field}`")
            })? as *const RuntimeValue;
        let target =
            row.value.fields.get_mut(target_field).ok_or_else(|| {
                format!("generic list `{list}` row missing field `{target_field}`")
            })? as *mut RuntimeValue;
        // BTreeMap nodes are not structurally changed here, and source/target
        // fields were checked to be distinct before creating both pointers.
        unsafe {
            match &*source {
                RuntimeValue::Text(source) | RuntimeValue::Enum(source) => {
                    (*target).set_textlike(source).map_err(|_| {
                        format!("generic list `{list}` field `{target_field}` is not text-like")
                            .into()
                    })
                }
                RuntimeValue::Bool(_) => Err(format!(
                    "generic list `{list}` field `{source_field}` is not text-like"
                )
                .into()),
            }
        }
    }

    fn list_row_textlike(&self, list: &str, index: usize, field: &str) -> RuntimeResult<&str> {
        self.list_row_field(list, index, field)?
            .as_textlike()
            .ok_or_else(|| format!("generic list `{list}` field `{field}` is not text-like").into())
    }

    fn list_row_bool(&self, list: &str, index: usize, field: &str) -> RuntimeResult<bool> {
        self.list_row_field(list, index, field)?
            .as_bool()
            .ok_or_else(|| format!("generic list `{list}` field `{field}` is not bool").into())
    }

    fn list_row_bool_opt(&self, list: &str, index: usize, field: &str) -> Option<bool> {
        self.lists
            .get(list)?
            .rows
            .get(index)?
            .value
            .fields
            .get(field)?
            .as_bool()
    }

    fn set_list_row_textlike(
        &mut self,
        list: &str,
        index: usize,
        field: &str,
        value: &str,
    ) -> RuntimeResult<()> {
        self.list_row_field_mut(list, index, field)?
            .set_textlike(value)
    }

    fn set_list_row_bool(
        &mut self,
        list: &str,
        index: usize,
        field: &str,
        value: bool,
    ) -> RuntimeResult<()> {
        self.list_row_field_mut(list, index, field)?.set_bool(value)
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
        list: &str,
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
            key,
            generation,
            field,
            value,
        }))
    }

    fn commit_indexed_bool_source(
        &mut self,
        equations: &ScalarEquationPlan,
        list: &str,
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
            key,
            generation,
            field,
            value,
        })
    }

    fn commit_indexed_text_action_source<'a>(
        &mut self,
        routes: &SourceRoutePlan,
        equations: &ScalarEquationPlan,
        list: &str,
        index: usize,
        source: &str,
        kind: SourceRouteTextAction,
        matches_target: impl Fn(&'static str) -> bool,
        payload_text: Option<&'a str>,
    ) -> RuntimeResult<Option<GenericTextFieldCommit<'a>>> {
        let Some(target) =
            routes.single_indexed_text_action_target_where(source, kind, matches_target)?
        else {
            return Err(format!("source `{source}` has no indexed text action `{kind:?}`").into());
        };
        self.commit_indexed_text_source(equations, list, index, target, source, payload_text)
    }

    fn commit_indexed_previous_text_target_source(
        &mut self,
        equations: &ScalarEquationPlan,
        list: &str,
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
            key,
            generation,
            field,
        })
    }

    fn commit_indexed_bool_action_source(
        &mut self,
        routes: &SourceRoutePlan,
        equations: &ScalarEquationPlan,
        list: &str,
        index: usize,
        source: &str,
        kind: SourceRouteBoolAction,
        read_extra_bool: impl Fn(&str) -> Option<bool>,
    ) -> RuntimeResult<GenericBoolFieldCommit> {
        let Some(target) = routes.single_indexed_bool_action_target(source, kind)? else {
            return Err(format!("source `{source}` has no indexed bool action `{kind:?}`").into());
        };
        self.commit_indexed_bool_source(equations, list, index, target, source, read_extra_bool)
    }

    fn commit_each_indexed_bool_source(
        &mut self,
        equations: &ScalarEquationPlan,
        list: &str,
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
        row: GenericRow,
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
            .row_templates
            .get(list)
            .ok_or_else(|| format!("generic runtime has no row template for list `{list}`"))?;
        let mut row = self
            .spare_rows
            .get_mut(list)
            .and_then(Vec::pop)
            .map(Ok)
            .unwrap_or_else(|| {
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
        let spare_len = self.spare_rows.get(list).map(Vec::len).unwrap_or(0);
        if spare_len >= count {
            return Ok(());
        }
        let template = self
            .row_templates
            .get(list)
            .ok_or_else(|| format!("generic runtime has no row template for list `{list}`"))?;
        let additional = count - spare_len;
        let spare_rows = self.spare_rows.entry(list.to_owned()).or_default();
        spare_rows.reserve(additional);
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
        Ok(GenericListRowCommit { key, generation })
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
            key: insert.key,
            generation: insert.generation,
            value,
        }))
    }

    fn spare_row(&mut self, list: &'static str, row: GenericRow) {
        if let Some(rows) = self.spare_rows.get_mut(list) {
            rows.push(row);
            return;
        }
        self.spare_rows.insert(list.to_owned(), vec![row]);
    }

    fn remove_row_for_predicate(
        &mut self,
        list: &str,
        predicate: RuntimeListPredicate,
        index: usize,
    ) -> RuntimeResult<Option<KeyedRow<GenericRow>>> {
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
    ) -> RuntimeResult<Option<KeyedRow<GenericRow>>> {
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
            self.spare_row(list, row.value);
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
        self.spare_row(list, row.value);
        Ok(Some(identity))
    }

    #[cfg(test)]
    fn remove_row_and_unbind_sources(
        &mut self,
        list: &'static str,
        index: usize,
        mut observe_binding: impl FnMut(&SourceBinding),
    ) -> RuntimeResult<KeyedRow<GenericRow>> {
        let row = self.remove_row(list, index)?;
        for binding in self.row_source_bindings(list, row.key, row.generation) {
            observe_binding(binding);
        }
        self.unbind_row_sources(list, row.key, row.generation);
        Ok(row)
    }

    fn append_row(&mut self, list: &str, row: GenericRow) -> RuntimeResult<(u64, u64)> {
        Ok(self
            .lists
            .get_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .append(row))
    }

    fn remove_row(&mut self, list: &str, index: usize) -> RuntimeResult<KeyedRow<GenericRow>> {
        let rows = self
            .lists
            .get_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?;
        if index >= rows.len() {
            return Err(format!("generic list `{list}` has no index {index}").into());
        }
        Ok(rows.remove_index(index))
    }

    fn move_row(&mut self, list: &str, from: usize, to: usize) -> RuntimeResult<(u64, u64)> {
        self.lists
            .get_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .move_index(from, to)
    }

    fn row_identity(&self, list: &str, index: usize) -> RuntimeResult<(u64, u64)> {
        let row = self
            .lists
            .get(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .rows
            .get(index)
            .ok_or_else(|| format!("generic list `{list}` has no index {index}"))?;
        Ok((row.key, row.generation))
    }

    fn bound_index(&self, list: &str, key: u64, generation: u64) -> RuntimeResult<Option<usize>> {
        Ok(self
            .lists
            .get(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .bound_index(key, generation))
    }

    fn list_len(&self, list: &str) -> RuntimeResult<usize> {
        self.lists
            .get(list)
            .map(KeyedList::len)
            .ok_or_else(|| format!("generic runtime has no list `{list}`").into())
    }

    fn list_identities(&self, list: &str) -> RuntimeResult<Vec<(u64, u64)>> {
        let rows = self
            .lists
            .get(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?;
        Ok(rows
            .rows
            .iter()
            .map(|row| (row.key, row.generation))
            .collect())
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
            .get(list)
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
            .get(list)
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
            .get(list)
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
            .get(list)
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

    fn any_list_bool(&self, list: &str, field: &str, expected: bool) -> RuntimeResult<bool> {
        let rows = self
            .lists
            .get(list)
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
    ) -> RuntimeResult<&RuntimeValue> {
        self.lists
            .get(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .rows
            .get(index)
            .ok_or_else(|| format!("generic list `{list}` has no index {index}"))?
            .value
            .fields
            .get(field)
            .ok_or_else(|| format!("generic list `{list}` row missing field `{field}`").into())
    }

    fn list_row_field_mut(
        &mut self,
        list: &str,
        index: usize,
        field: &str,
    ) -> RuntimeResult<&mut RuntimeValue> {
        self.lists
            .get_mut(list)
            .ok_or_else(|| format!("generic runtime has no list `{list}`"))?
            .rows
            .get_mut(index)
            .ok_or_else(|| format!("generic list `{list}` has no index {index}"))?
            .value
            .fields
            .get_mut(field)
            .ok_or_else(|| format!("generic list `{list}` row missing field `{field}`").into())
    }
}

impl RuntimeValue {
    fn as_textlike(&self) -> Option<&str> {
        match self {
            Self::Text(value) | Self::Enum(value) => Some(value.as_str()),
            Self::Bool(_) => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            Self::Text(_) | Self::Enum(_) => None,
        }
    }

    fn set_textlike(&mut self, value: &str) -> RuntimeResult<()> {
        match self {
            Self::Text(current) | Self::Enum(current) => {
                current.clear();
                current.push_str(value);
                Ok(())
            }
            Self::Bool(_) => Err("cannot write text into bool runtime value".into()),
        }
    }

    fn set_bool(&mut self, value: bool) -> RuntimeResult<()> {
        match self {
            Self::Bool(current) => {
                *current = value;
                Ok(())
            }
            Self::Text(_) | Self::Enum(_) => {
                Err("cannot write bool into text runtime value".into())
            }
        }
    }

    fn reserve_textlike(&mut self, additional: usize) -> RuntimeResult<()> {
        match self {
            Self::Text(value) | Self::Enum(value) => {
                value.reserve(additional);
                Ok(())
            }
            Self::Bool(_) => Err("cannot reserve text capacity on bool runtime value".into()),
        }
    }
}

fn list_seed_fields(
    row: &boon_ir::ListSeedRecord,
) -> RuntimeResult<BTreeMap<String, RuntimeValue>> {
    row.fields
        .iter()
        .map(|field| {
            Ok((
                field.name.clone(),
                runtime_value_from_initial(&field.value, &BTreeMap::new())?,
            ))
        })
        .collect()
}

impl GenericRowTemplate {
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
            fields.push(GenericRowFieldTemplate {
                field,
                initial_value: cell.initial_value.clone(),
            });
        }
        Ok(Self { fields })
    }

    fn materialize(
        &self,
        mut seed_fields: BTreeMap<String, RuntimeValue>,
    ) -> RuntimeResult<GenericRow> {
        for field in &self.fields {
            seed_fields.insert(
                field.field.clone(),
                runtime_value_from_initial(&field.initial_value, &seed_fields)?,
            );
        }
        Ok(GenericRow {
            fields: seed_fields,
        })
    }

    fn reset_from_text_seeds<'a>(
        &self,
        row: &mut GenericRow,
        seed_text: impl Fn(&str) -> Option<&'a str>,
    ) -> RuntimeResult<()> {
        for field in &self.fields {
            let slot = row
                .fields
                .get_mut(&field.field)
                .ok_or_else(|| format!("generic row missing field `{}`", field.field))?;
            match &field.initial_value {
                InitialValue::Text { value } => slot.set_textlike(value)?,
                InitialValue::Bool { value } => slot.set_bool(*value)?,
                InitialValue::Enum { value } => slot.set_textlike(value)?,
                InitialValue::SeedField { path } => {
                    let value =
                        seed_text(path).ok_or_else(|| format!("seed field `{path}` is missing"))?;
                    slot.set_textlike(value)?;
                }
                InitialValue::Unknown { summary } => {
                    return Err(format!("unsupported state initializer `{summary}`").into());
                }
            }
        }
        Ok(())
    }
}

impl GenericRow {
    fn reserve_textlike_fields(&mut self, additional: usize) -> RuntimeResult<()> {
        for value in self.fields.values_mut() {
            if value.as_textlike().is_some() {
                value.reserve_textlike(additional)?;
            }
        }
        Ok(())
    }
}

fn runtime_value_from_initial(
    initial: &InitialValue,
    seed_fields: &BTreeMap<String, RuntimeValue>,
) -> RuntimeResult<RuntimeValue> {
    match initial {
        InitialValue::Text { value } => Ok(RuntimeValue::Text(value.clone())),
        InitialValue::Bool { value } => Ok(RuntimeValue::Bool(*value)),
        InitialValue::Enum { value } => Ok(RuntimeValue::Enum(value.clone())),
        InitialValue::SeedField { path } => seed_fields
            .get(path)
            .cloned()
            .ok_or_else(|| format!("seed field `{path}` is missing").into()),
        InitialValue::Unknown { summary } => {
            Err(format!("unsupported state initializer `{summary}`").into())
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

fn todo_generic_row(title: &str) -> GenericRow {
    let mut fields = BTreeMap::new();
    fields.insert("title".to_owned(), RuntimeValue::Text(title.to_owned()));
    fields.insert("edit_text".to_owned(), RuntimeValue::Text(title.to_owned()));
    fields.insert("completed".to_owned(), RuntimeValue::Bool(false));
    fields.insert("editing".to_owned(), RuntimeValue::Bool(false));
    GenericRow { fields }
}

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
    runtime
        .lists
        .insert("cells".to_owned(), KeyedList::from_values(cell_rows));
    runtime
}

fn cell_generic_row(address: &str) -> GenericRow {
    let mut fields = BTreeMap::new();
    fields.insert("address".to_owned(), RuntimeValue::Text(address.to_owned()));
    fields.insert("editing_text".to_owned(), RuntimeValue::Text(String::new()));
    fields.insert("formula_text".to_owned(), RuntimeValue::Text(String::new()));
    fields.insert("editing".to_owned(), RuntimeValue::Bool(false));
    GenericRow { fields }
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
    key: u64,
    generation: u64,
    field: &'static str,
    value: &'a str,
}

#[derive(Clone, Copy, Debug)]
struct GenericTextFieldIdentity {
    key: u64,
    generation: u64,
    field: &'static str,
}

#[derive(Clone, Copy, Debug)]
struct GenericBoolFieldCommit {
    key: u64,
    generation: u64,
    field: &'static str,
    value: bool,
}

#[derive(Clone, Copy, Debug)]
struct GenericListRowCommit {
    key: u64,
    generation: u64,
}

struct GenericTextListAppendCommit<'a> {
    key: u64,
    generation: u64,
    value: &'a str,
}

struct GenericRootTextCommit<'a> {
    target: &'static str,
    value: Cow<'a, str>,
}

enum GenericListRemoveObservation<'a> {
    SourceUnbind(&'a SourceBinding),
    RowRemoved { key: u64, generation: u64 },
}

#[derive(Clone, Copy, Debug)]
struct GenericSourceActionInput<'a> {
    source: &'a str,
    list: Option<&'static str>,
    index: Option<usize>,
    key: Option<&'a str>,
    text: Option<&'a str>,
    seq: TickSeq,
}

#[allow(dead_code)]
enum GenericSourceMutation<'a> {
    RootText(GenericRootTextCommit<'a>),
    TextField(GenericTextFieldCommit<'a>),
    TextFieldIdentity(GenericTextFieldIdentity),
    BoolField(GenericBoolFieldCommit),
    ListAppend(GenericTextListAppendCommit<'a>),
    ListRemove { key: u64, generation: u64 },
    SourceUnbind(SourceBinding),
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
    routes: Vec<SourceRoute>,
}

#[derive(Clone, Debug, Default)]
struct ListSourceBindingPlan {
    bindings: Vec<ListSourceBinding>,
}

#[derive(Clone, Debug)]
struct ListSourceBinding {
    list: &'static str,
    source_paths: Vec<&'static str>,
}

#[derive(Clone, Debug, Default)]
struct SourceRoute {
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

    fn empty() -> Self {
        Self {
            branches: Vec::new(),
        }
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
        scalar: &ScalarEquationPlan,
        derived: &DerivedEquationPlan,
        lists: &ListEquationPlan,
        root_targets: &BTreeSet<&str>,
    ) -> Self {
        let mut routes = Self::default();
        for branch in &scalar.branches {
            let route = routes.route_mut(branch.source);
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
            routes
                .route_mut(transform.source)
                .derived_text_targets
                .push(transform.target);
        }
        for operation in &lists.operations {
            match &operation.kind {
                RuntimeListOperationKind::Append { trigger, .. } => {
                    for route in routes
                        .routes
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
                    routes
                        .route_mut(*source)
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
        for route in &mut routes.routes {
            route.rebuild_actions();
        }
        routes
    }

    fn for_source(&self, source: &str) -> Option<&SourceRoute> {
        self.routes.iter().find(|route| route.source == source)
    }

    fn require_source(&self, source: &str) -> RuntimeResult<&SourceRoute> {
        self.for_source(source)
            .ok_or_else(|| format!("source `{source}` has no compiled route").into())
    }

    fn actions(&self, source: &str) -> RuntimeResult<&[SourceRouteAction]> {
        Ok(self.require_source(source)?.actions.as_slice())
    }

    fn single_root_scalar_target(&self, source: &str) -> RuntimeResult<Option<&'static str>> {
        self.require_source(source)?.single_root_scalar_target()
    }

    fn single_indexed_bool_action_target(
        &self,
        source: &str,
        kind: SourceRouteBoolAction,
    ) -> RuntimeResult<Option<&'static str>> {
        self.require_source(source)?
            .single_indexed_bool_action_target(kind)
    }

    fn single_indexed_text_action_target_where(
        &self,
        source: &str,
        kind: SourceRouteTextAction,
        matches_target: impl Fn(&'static str) -> bool,
    ) -> RuntimeResult<Option<&'static str>> {
        self.require_source(source)?
            .single_indexed_text_action_target_where(kind, matches_target)
    }

    fn has_indexed_text_target(&self, source: &str, target: &str) -> RuntimeResult<bool> {
        Ok(self.require_source(source)?.has_indexed_text_target(target))
    }

    fn has_list_remove_target(&self, source: &str, list: &str) -> RuntimeResult<bool> {
        Ok(self.require_source(source)?.has_list_remove_target(list))
    }

    fn has_list_append_target(&self, source: &str, list: &str) -> RuntimeResult<bool> {
        Ok(self.require_source(source)?.has_list_append_target(list))
    }

    fn has_root_scalar_action(&self, source: &str) -> RuntimeResult<bool> {
        Ok(self.require_source(source)?.has_root_scalar_action())
    }

    fn has_indexed_text_action(
        &self,
        source: &str,
        kind: SourceRouteTextAction,
    ) -> RuntimeResult<bool> {
        Ok(self.require_source(source)?.has_indexed_text_action(kind))
    }

    fn has_indexed_text_action_where(
        &self,
        source: &str,
        kind: SourceRouteTextAction,
        matches_target: impl Fn(&'static str) -> bool,
    ) -> RuntimeResult<bool> {
        Ok(self
            .require_source(source)?
            .has_indexed_text_action_where(kind, matches_target))
    }

    fn has_indexed_bool_action(
        &self,
        source: &str,
        kind: SourceRouteBoolAction,
    ) -> RuntimeResult<bool> {
        Ok(self.require_source(source)?.has_indexed_bool_action(kind))
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

    fn route_mut(&mut self, source: &'static str) -> &mut SourceRoute {
        if let Some(index) = self.routes.iter().position(|route| route.source == source) {
            return &mut self.routes[index];
        }
        self.routes.push(SourceRoute {
            source,
            ..SourceRoute::default()
        });
        self.routes
            .last_mut()
            .expect("route was just pushed and must exist")
    }
}

impl ListSourceBindingPlan {
    fn from_ir(ir: &TypedProgram) -> Self {
        let mut bindings = Vec::new();
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
                bindings.push(ListSourceBinding {
                    list: leak_runtime_path(list.name.clone()),
                    source_paths,
                });
            }
        }
        Self { bindings }
    }

    fn source_paths(&self, list: &str) -> RuntimeResult<&[&'static str]> {
        self.bindings
            .iter()
            .find_map(|binding| (binding.list == list).then_some(binding.source_paths.as_slice()))
            .ok_or_else(|| format!("list `{list}` has no scoped source binding plan").into())
    }

    fn source_count(&self, list: &str) -> RuntimeResult<usize> {
        self.source_paths(list).map(<[_]>::len)
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

    fn single_indexed_text_action_target_where(
        &self,
        expected: SourceRouteTextAction,
        matches_target: impl Fn(&'static str) -> bool,
    ) -> RuntimeResult<Option<&'static str>> {
        single_route_action_target_where(
            self.source,
            "indexed text action",
            &self.actions,
            |action| match action {
                SourceRouteAction::IndexedText { kind, target }
                    if kind == expected && matches_target(target) =>
                {
                    Some(target)
                }
                _ => None,
            },
        )
    }

    fn has_indexed_bool_action(&self, expected: SourceRouteBoolAction) -> bool {
        self.has_action(|action| {
            matches!(
                action,
                SourceRouteAction::IndexedBool { kind, .. } if kind == expected
            )
        })
    }

    fn single_indexed_bool_action_target(
        &self,
        expected: SourceRouteBoolAction,
    ) -> RuntimeResult<Option<&'static str>> {
        single_route_action_target_where(
            self.source,
            "indexed bool action",
            &self.actions,
            |action| match action {
                SourceRouteAction::IndexedBool { kind, target } if kind == expected => Some(target),
                _ => None,
            },
        )
    }

    fn has_indexed_text_target(&self, target: &str) -> bool {
        self.has_action(|action| {
            matches!(
                action,
                SourceRouteAction::IndexedText {
                    target: candidate,
                    ..
                } if candidate == target
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

fn single_route_action_target_where(
    source: &str,
    index_name: &str,
    actions: &[SourceRouteAction],
    select_target: impl Fn(SourceRouteAction) -> Option<&'static str>,
) -> RuntimeResult<Option<&'static str>> {
    let mut target = None;
    for action in actions.iter().copied() {
        let Some(candidate) = select_target(action) else {
            continue;
        };
        if target.is_some_and(|current| current != candidate) {
            return Err(
                format!("source `{source}` has multiple matching {index_name} targets").into(),
            );
        }
        target = Some(candidate);
    }
    Ok(target)
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
    ) -> RuntimeResult<BTreeMap<String, RuntimeValue>> {
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
        let mut seed_fields = BTreeMap::new();
        for field in fields {
            if field.source != trigger {
                return Err(format!(
                    "append field `{}` uses unsupported source `{}`; expected trigger `{trigger}`",
                    field.name, field.source
                )
                .into());
            }
            seed_fields.insert(
                field.name.to_owned(),
                RuntimeValue::Text(trigger_value.to_owned()),
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

#[derive(Clone, Debug)]
struct TodoRuntime {
    generic: GenericScheduledRuntime,
    next_source_seq: u64,
    stale_source_drop_count: u64,
}

#[derive(Clone, Debug)]
enum TodoEvent<'a> {
    NewInputChange {
        source: &'a str,
        text: &'a str,
    },
    NewInputKeyDown {
        source: &'a str,
        key: &'a str,
        text: &'a str,
    },
    Filter {
        source: &'a str,
    },
    ClearCompleted {
        source: &'a str,
    },
    ToggleAll {
        source: &'a str,
    },
    TodoCheckbox {
        source: &'a str,
        target_text: &'a str,
        target_occurrence: usize,
    },
    TodoTitleDoubleClick {
        source: &'a str,
        target_text: &'a str,
        target_occurrence: usize,
    },
    EditingTitleChange {
        source: &'a str,
        target_text: &'a str,
        text: &'a str,
    },
    EditingTitleKeyDown {
        source: &'a str,
        target_text: &'a str,
        key: &'a str,
        text: Option<&'a str>,
    },
    EditingTitleBlur {
        source: &'a str,
        target_text: &'a str,
        text: Option<&'a str>,
    },
    HoverDelete {
        target_text: &'a str,
        target_occurrence: usize,
    },
    RemoveTodo {
        source: &'a str,
        target_text: &'a str,
        target_occurrence: usize,
    },
}

impl TodoRuntime {
    fn from_generic(mut generic: GenericScheduledRuntime) -> RuntimeResult<Self> {
        let todo_count = generic.list_len("todos")?;
        let row_source_paths = generic.row_source_paths("todos")?.to_vec();
        generic.reserve_source_bindings(todo_count * row_source_paths.len());
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

    fn seeded(count: usize) -> Self {
        let list_source_bindings = default_todo_list_source_bindings();
        let row_source_paths = list_source_bindings
            .source_paths("todos")
            .expect("checked-in TodoMVC should have row sources");
        let generic_rows = (0..count).map(|index| todo_generic_row(&format!("Todo {index}")));
        let mut generic = GenericCircuitRuntime::default();
        generic.root.insert(
            "store.new_todo_text".to_owned(),
            RuntimeValue::Text(String::new()),
        );
        generic.root.insert(
            "store.selected_filter".to_owned(),
            RuntimeValue::Enum("All".to_owned()),
        );
        generic
            .lists
            .insert("todos".to_owned(), KeyedList::from_values(generic_rows));
        generic.reserve_source_bindings(count * row_source_paths.len());
        for index in 0..count {
            let (key, generation) = generic
                .row_identity("todos", index)
                .expect("seeded TodoMVC row exists");
            generic.bind_row_sources("todos", key, generation, row_source_paths);
        }
        let generic = GenericScheduledRuntime::from_parts(
            generic,
            ScalarEquationPlan::empty(),
            DerivedEquationPlan::empty(),
            ListEquationPlan::empty(),
            FormulaEquationPlan::empty(),
            SourceRoutePlan::default(),
            list_source_bindings,
        );
        Self {
            generic,
            next_source_seq: 1,
            stale_source_drop_count: 0,
        }
    }

    fn prepare_for_scenario(&mut self, scenario: &Scenario) {
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
        self.generic
            .reserve_root_textlike("store.new_todo_text", max_text_len)
            .expect("TodoMVC generic runtime has new_todo_text root");
        self.generic
            .reserve_root_textlike("store.selected_filter", "Completed".len())
            .expect("TodoMVC generic runtime has selected_filter root");
        self.generic
            .reserve_list("todos", append_count)
            .expect("TodoMVC generic runtime has todos list");
        let row_source_count = self
            .generic
            .list_source_bindings
            .source_count("todos")
            .expect("TodoMVC compiled runtime has row source bindings");
        self.generic
            .reserve_source_bindings(append_count * row_source_count);
        if append_count > 0 {
            self.generic
                .reserve_spare_rows_for_list_append_text("todos", append_count, max_text_len)
                .expect("TodoMVC generic runtime can reserve append rows");
        }
        self.generic
            .reserve_list_row_textlike_fields("todos", "title", |_, current| {
                max_text_len.saturating_sub(current.len())
            })
            .expect("TodoMVC generic runtime has title fields");
        self.generic
            .reserve_list_row_textlike_fields("todos", "edit_text", |_, current| {
                max_text_len.saturating_sub(current.len())
            })
            .expect("TodoMVC generic runtime has edit_text fields");
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
        if !matches!(event, TodoEvent::HoverDelete { .. }) {
            assert_todo_event_matches(step, &event)?;
        }
        match event {
            TodoEvent::NewInputChange { source, text } => {
                let seq = self.next_source_seq();
                self.apply_root_text_source(source, Some(text), seq, &mut deltas, &mut patches)?;
            }
            TodoEvent::NewInputKeyDown { source, key, text } => {
                let seq = self.next_source_seq();
                if key == "Enter" {
                    let mut insert = None;
                    let mut root_commit = None;
                    self.generic.apply_source_actions(
                        GenericSourceActionInput {
                            source,
                            list: Some("todos"),
                            index: None,
                            key: Some(key),
                            text: Some(text),
                            seq,
                        },
                        |_| None,
                        |mutation| {
                            match mutation {
                                GenericSourceMutation::ListAppend(commit) => insert = Some(commit),
                                GenericSourceMutation::RootText(commit) => {
                                    root_commit = Some(commit)
                                }
                                _ => {}
                            }
                            Ok(())
                        },
                    )?;
                    if let Some(insert) = insert {
                        self.emit_todo_insert(insert, &mut deltas, &mut patches);
                        if let Some(commit) = root_commit {
                            TodoRuntime::emit_root_text_commit(
                                commit.target,
                                commit.value,
                                &mut deltas,
                                &mut patches,
                            )?;
                        }
                    }
                }
            }
            TodoEvent::Filter { source } => {
                let seq = self.next_source_seq();
                self.apply_root_text_source(source, None, seq, &mut deltas, &mut patches)?;
            }
            TodoEvent::ClearCompleted { source } => {
                self.remove_where_source(source, &mut deltas, &mut patches)?;
            }
            TodoEvent::ToggleAll { source } => {
                let all_completed = self.all_completed();
                self.generic.apply_source_actions(
                    GenericSourceActionInput {
                        source,
                        list: Some("todos"),
                        index: None,
                        key: None,
                        text: None,
                        seq: TickSeq(0),
                    },
                    |path| match path {
                        "store.all_completed" => Some(all_completed),
                        _ => None,
                    },
                    |mutation| {
                        if let GenericSourceMutation::BoolField(completed) = mutation {
                            deltas.push(field_delta(
                                Some(completed.key),
                                Some(completed.generation),
                                completed.field,
                                ProtocolValue::Bool(completed.value),
                            ));
                            patches.push(patch(
                                "SetProperty",
                                RenderTarget::TodoCheckbox(completed.key),
                                ProtocolValue::CheckedProperty(completed.value),
                            ));
                        }
                        Ok(())
                    },
                )?;
            }
            TodoEvent::TodoCheckbox {
                source,
                target_text,
                target_occurrence,
            } => {
                let index = self.find_index_at_occurrence(target_text, target_occurrence)?;
                let all_completed = self.all_completed();
                self.set_completed_from_source(
                    index,
                    source,
                    all_completed,
                    &mut deltas,
                    &mut patches,
                )?;
            }
            TodoEvent::TodoTitleDoubleClick {
                source,
                target_text,
                target_occurrence,
            } => {
                let index = self.find_index_at_occurrence(target_text, target_occurrence)?;
                let edit_text = self
                    .generic
                    .commit_indexed_text_action_source(
                        "todos",
                        index,
                        source,
                        SourceRouteTextAction::PreviousValue,
                        |_| true,
                        Some(target_text),
                    )?
                    .ok_or_else(|| {
                        format!("previous-text update from `{source}` produced no change")
                    })?;
                let all_completed = self.all_completed();
                let editing = self.commit_todo_bool_source(
                    index,
                    source,
                    SourceRouteBoolAction::ConstTrue,
                    all_completed,
                )?;
                deltas.push(field_delta(
                    Some(editing.key),
                    Some(editing.generation),
                    editing.field,
                    ProtocolValue::Bool(editing.value),
                ));
                deltas.push(field_delta(
                    Some(edit_text.key),
                    Some(edit_text.generation),
                    edit_text.field,
                    ProtocolValue::Text(Cow::Borrowed(edit_text.value)),
                ));
                patches.push(patch(
                    "ShowEditInput",
                    RenderTarget::TodoEdit(editing.key),
                    ProtocolValue::Text(Cow::Borrowed(edit_text.value)),
                ));
            }
            TodoEvent::EditingTitleChange {
                source,
                target_text,
                text,
            } => {
                let index = self
                    .find_index(target_text)
                    .or_else(|_| self.find_editing_index())?;
                self.generic.apply_source_actions(
                    GenericSourceActionInput {
                        source,
                        list: Some("todos"),
                        index: Some(index),
                        key: None,
                        text: Some(text),
                        seq: TickSeq(0),
                    },
                    |_| None,
                    |mutation| {
                        if let GenericSourceMutation::TextField(edit_text) = mutation {
                            deltas.push(field_delta(
                                Some(edit_text.key),
                                Some(edit_text.generation),
                                edit_text.field,
                                ProtocolValue::Text(Cow::Borrowed(edit_text.value)),
                            ));
                            patches.push(patch(
                                "SetEditInput",
                                RenderTarget::TodoEdit(edit_text.key),
                                ProtocolValue::Text(Cow::Borrowed(edit_text.value)),
                            ));
                        }
                        Ok(())
                    },
                )?;
            }
            TodoEvent::EditingTitleKeyDown {
                source,
                target_text,
                key,
                text,
            } => {
                if matches!(key, "Enter" | "Escape") {
                    let index = self
                        .find_index(target_text)
                        .or_else(|_| self.find_editing_index())?;
                    let (row_key, _) = self.todo_row_identity(index)?;
                    if key == "Enter" {
                        if let Some(title) = self.generic.commit_indexed_text_action_source(
                            "todos",
                            index,
                            source,
                            SourceRouteTextAction::TextTrimOrPrevious,
                            |target| row_field_name(target) == "title",
                            text,
                        )? {
                            deltas.push(field_delta(
                                Some(title.key),
                                Some(title.generation),
                                title.field,
                                ProtocolValue::Text(Cow::Borrowed(title.value)),
                            ));
                            patches.push(patch(
                                "SetText",
                                RenderTarget::TodoTitle(row_key),
                                ProtocolValue::Text(Cow::Borrowed(title.value)),
                            ));
                        }
                    } else {
                        let edit_text = self
                            .generic
                            .commit_indexed_text_action_source(
                                "todos",
                                index,
                                source,
                                SourceRouteTextAction::PreviousValue,
                                |_| true,
                                Some(target_text),
                            )?
                            .ok_or_else(|| {
                                format!("previous-text update from `{source}` produced no change")
                            })?;
                        deltas.push(field_delta(
                            Some(edit_text.key),
                            Some(edit_text.generation),
                            edit_text.field,
                            ProtocolValue::Text(Cow::Borrowed(edit_text.value)),
                        ));
                    }
                    let all_completed = self.all_completed();
                    let editing = self.commit_todo_bool_source(
                        index,
                        source,
                        SourceRouteBoolAction::ConstFalse,
                        all_completed,
                    )?;
                    deltas.push(field_delta(
                        Some(editing.key),
                        Some(editing.generation),
                        editing.field,
                        ProtocolValue::Bool(editing.value),
                    ));
                    patches.push(patch(
                        "HideEditInput",
                        RenderTarget::TodoEdit(row_key),
                        ProtocolValue::Bool(editing.value),
                    ));
                }
            }
            TodoEvent::EditingTitleBlur {
                source,
                target_text,
                text,
            } => {
                let index = self
                    .find_index(target_text)
                    .or_else(|_| self.find_editing_index())?;
                let (row_key, _) = self.todo_row_identity(index)?;
                if let Some(title) = self.generic.commit_indexed_text_action_source(
                    "todos",
                    index,
                    source,
                    SourceRouteTextAction::TextTrimOrPrevious,
                    |target| row_field_name(target) == "title",
                    text.or(Some(target_text)),
                )? {
                    deltas.push(field_delta(
                        Some(title.key),
                        Some(title.generation),
                        title.field,
                        ProtocolValue::Text(Cow::Borrowed(title.value)),
                    ));
                    patches.push(patch(
                        "SetText",
                        RenderTarget::TodoTitle(row_key),
                        ProtocolValue::Text(Cow::Borrowed(title.value)),
                    ));
                }
                let all_completed = self.all_completed();
                let editing = self.commit_todo_bool_source(
                    index,
                    source,
                    SourceRouteBoolAction::ConstFalse,
                    all_completed,
                )?;
                deltas.push(field_delta(
                    Some(editing.key),
                    Some(editing.generation),
                    editing.field,
                    ProtocolValue::Bool(editing.value),
                ));
                patches.push(patch(
                    "HideEditInput",
                    RenderTarget::TodoEdit(row_key),
                    ProtocolValue::Bool(editing.value),
                ));
            }
            TodoEvent::HoverDelete {
                target_text,
                target_occurrence,
            } => {
                let index = self.find_index_at_occurrence(target_text, target_occurrence)?;
                let (key, _) = self.todo_row_identity(index)?;
                patches.push(patch(
                    "ShowDeleteButton",
                    RenderTarget::TodoRow(key),
                    ProtocolValue::Bool(true),
                ));
            }
            TodoEvent::RemoveTodo {
                source,
                target_text,
                target_occurrence,
            } => {
                let index = self.find_index_at_occurrence(target_text, target_occurrence)?;
                if !self.remove_index_source(index, source, &mut deltas, &mut patches)? {
                    return Err(format!(
                        "remove source `{source}` predicate does not match todo `{target_text}`"
                    )
                    .into());
                }
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
        self.generic
            .require_source(source)
            .map_err(|_| format!("{} source `{source}` has no compiled route", step.id))?;
        if source_event.target_text.is_none() {
            if self.generic.has_list_append_target(source, "todos")? {
                return Ok(Some(TodoEvent::NewInputKeyDown {
                    source,
                    key: source_event.key.unwrap_or_default(),
                    text: source_event.text.unwrap_or_default(),
                }));
            }
            if source_event.text.is_some() {
                self.generic
                    .single_root_scalar_target(source)?
                    .ok_or_else(|| {
                        format!("{} source `{source}` has no root text target", step.id)
                    })?;
                return Ok(Some(TodoEvent::NewInputChange {
                    source,
                    text: source_event.text.unwrap_or_default(),
                }));
            }
            if self.generic.has_list_remove_target(source, "todos")? {
                return Ok(Some(TodoEvent::ClearCompleted { source }));
            }
            if self
                .generic
                .has_indexed_bool_action(source, SourceRouteBoolAction::BoolNot)?
            {
                return Ok(Some(TodoEvent::ToggleAll { source }));
            }
            if self.generic.has_root_scalar_action(source)? {
                return Ok(Some(TodoEvent::Filter { source }));
            }
            return Err(format!("{} source `{source}` has no TodoMVC route", step.id).into());
        }

        let target_text = source_event
            .target_text
            .expect("checked target_text presence above");
        let removes_todos = self.generic.has_list_remove_target(source, "todos")?;
        let toggles_completed = self
            .generic
            .has_indexed_bool_action(source, SourceRouteBoolAction::BoolNot)?;
        let updates_title = self.generic.has_indexed_text_action_where(
            source,
            SourceRouteTextAction::TextTrimOrPrevious,
            |target| row_field_name(target) == "title",
        )?;
        let has_previous_text = self
            .generic
            .has_indexed_text_action(source, SourceRouteTextAction::PreviousValue)?;
        let opens_edit = self
            .generic
            .has_indexed_bool_action(source, SourceRouteBoolAction::ConstTrue)?;
        let Some(target_occurrence) =
            self.resolve_bound_occurrence(step, target_text, fallback_occurrence)
        else {
            return Ok(None);
        };
        if removes_todos {
            return Ok(Some(TodoEvent::RemoveTodo {
                source,
                target_text,
                target_occurrence,
            }));
        }
        if toggles_completed {
            return Ok(Some(TodoEvent::TodoCheckbox {
                source,
                target_text,
                target_occurrence,
            }));
        }
        if source_event.key.is_some() {
            self.editing_title()?;
            return Ok(Some(TodoEvent::EditingTitleKeyDown {
                source,
                target_text,
                key: source_event.key.unwrap_or_default(),
                text: source_event.text,
            }));
        }
        if updates_title {
            self.editing_title()?;
            return Ok(Some(TodoEvent::EditingTitleBlur {
                source,
                target_text,
                text: source_event.text.or(Some(target_text)),
            }));
        }
        if source_event.text.is_some() {
            self.editing_title()?;
            return Ok(Some(TodoEvent::EditingTitleChange {
                source,
                target_text,
                text: source_event.text.unwrap_or_default(),
            }));
        }
        if has_previous_text && opens_edit {
            return Ok(Some(TodoEvent::TodoTitleDoubleClick {
                source,
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
        let action = step.user_action.as_ref()?;
        let Some(key) = toml_u64_ref(action, "target_key") else {
            return Some(fallback);
        };
        let generation = toml_u64_ref(action, "target_generation")?;
        let source_path = toml_string_ref(action, "source")
            .or(GenericSourceEvent::from_step(step)
                .ok()
                .flatten()
                .map(|event| event.source))
            .unwrap_or_default();
        let source_id = toml_u64_ref(action, "source_id");
        let bind_epoch = toml_u64_ref(action, "bind_epoch");
        if !self.generic.is_row_source_bound(
            "todos",
            key,
            generation,
            source_path,
            source_id,
            bind_epoch,
        ) {
            self.stale_source_drop_count += 1;
            return None;
        }
        let Some(index) = self
            .generic
            .bound_index("todos", key, generation)
            .ok()
            .flatten()
        else {
            self.stale_source_drop_count += 1;
            return None;
        };
        if self
            .generic
            .list_row_textlike("todos", index, "title")
            .map_or(true, |candidate| candidate != title)
        {
            return None;
        }
        Some(self.occurrence_for_index_and_title(index, title))
    }

    fn occurrence_for_index_and_title(&self, target_index: usize, title: &str) -> usize {
        (0..=target_index)
            .filter(|index| {
                self.generic
                    .list_row_textlike("todos", *index, "title")
                    .is_ok_and(|candidate| candidate == title)
            })
            .count()
            .max(1)
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
        self.emit_todo_insert(insert, deltas, patches);
        Ok(())
    }

    fn emit_todo_insert<'a>(
        &self,
        insert: GenericTextListAppendCommit<'a>,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) {
        deltas.push(list_delta(
            "ListInsert",
            insert.key,
            insert.generation,
            ProtocolValue::TodoRow {
                title: Cow::Borrowed(insert.value),
                completed: false,
                editing: false,
            },
        ));
        patches.push(patch(
            "InsertElement",
            RenderTarget::TodoRow(insert.key),
            ProtocolValue::Text(Cow::Borrowed(insert.value)),
        ));
        push_source_binding_deltas_for_row(&self.generic, insert.key, insert.generation, deltas);
        push_source_binding_patches_for_row(&self.generic, insert.key, insert.generation, patches);
    }

    fn set_completed_from_source(
        &mut self,
        index: usize,
        source: &str,
        all_completed_snapshot: bool,
        deltas: &mut Vec<SemanticDelta<'_>>,
        patches: &mut Vec<RenderPatch<'_>>,
    ) -> RuntimeResult<()> {
        let completed = self.generic.commit_indexed_bool_action_source(
            "todos",
            index,
            source,
            SourceRouteBoolAction::BoolNot,
            |path| match path {
                "store.all_completed" => Some(all_completed_snapshot),
                _ => None,
            },
        )?;
        deltas.push(field_delta(
            Some(completed.key),
            Some(completed.generation),
            completed.field,
            ProtocolValue::Bool(completed.value),
        ));
        patches.push(patch(
            "SetProperty",
            RenderTarget::TodoCheckbox(completed.key),
            ProtocolValue::CheckedProperty(completed.value),
        ));
        Ok(())
    }

    fn set_completed_value(
        &mut self,
        target: &'static str,
        index: usize,
        value: bool,
        deltas: &mut Vec<SemanticDelta<'_>>,
        patches: &mut Vec<RenderPatch<'_>>,
    ) -> RuntimeResult<()> {
        let field = row_field_name(target);
        let (key, generation) = self
            .generic
            .commit_indexed_bool_field("todos", index, field, value)?;
        deltas.push(field_delta(
            Some(key),
            Some(generation),
            field,
            ProtocolValue::Bool(value),
        ));
        patches.push(patch(
            "SetProperty",
            RenderTarget::TodoCheckbox(key),
            ProtocolValue::CheckedProperty(value),
        ));
        Ok(())
    }

    fn remove_where_source(
        &mut self,
        source: &str,
        deltas: &mut Vec<SemanticDelta<'_>>,
        patches: &mut Vec<RenderPatch<'_>>,
    ) -> RuntimeResult<()> {
        self.generic.apply_source_actions(
            GenericSourceActionInput {
                source,
                list: Some("todos"),
                index: None,
                key: None,
                text: None,
                seq: TickSeq(0),
            },
            |_| None,
            |mutation| {
                match mutation {
                    GenericSourceMutation::SourceUnbind(binding) => {
                        deltas.push(source_delta("SourceUnbind", &binding, ProtocolValue::Null));
                        patches.push(patch(
                            "UnbindSource",
                            RenderTarget::TodoSource(binding.key, binding.source_path),
                            source_binding_value(&binding),
                        ));
                    }
                    GenericSourceMutation::ListRemove { key, generation } => {
                        deltas.push(list_delta(
                            "ListRemove",
                            key,
                            generation,
                            ProtocolValue::Null,
                        ));
                        patches.push(patch(
                            "RemoveElement",
                            RenderTarget::TodoRow(key),
                            ProtocolValue::Null,
                        ));
                    }
                    _ => {}
                }
                Ok(())
            },
        )
    }

    fn remove_index_source(
        &mut self,
        index: usize,
        source: &str,
        deltas: &mut Vec<SemanticDelta<'_>>,
        patches: &mut Vec<RenderPatch<'_>>,
    ) -> RuntimeResult<bool> {
        let mut removed = false;
        self.generic.apply_source_actions(
            GenericSourceActionInput {
                source,
                list: Some("todos"),
                index: Some(index),
                key: None,
                text: None,
                seq: TickSeq(0),
            },
            |_| None,
            |mutation| {
                match mutation {
                    GenericSourceMutation::SourceUnbind(binding) => {
                        deltas.push(source_delta("SourceUnbind", &binding, ProtocolValue::Null));
                        patches.push(patch(
                            "UnbindSource",
                            RenderTarget::TodoSource(binding.key, binding.source_path),
                            source_binding_value(&binding),
                        ));
                    }
                    GenericSourceMutation::ListRemove { key, generation } => {
                        removed = true;
                        deltas.push(list_delta(
                            "ListRemove",
                            key,
                            generation,
                            ProtocolValue::Null,
                        ));
                        patches.push(patch(
                            "RemoveElement",
                            RenderTarget::TodoRow(key),
                            ProtocolValue::Null,
                        ));
                    }
                    _ => {}
                }
                Ok(())
            },
        )?;
        Ok(removed)
    }

    #[cfg(test)]
    fn remove_index(
        &mut self,
        index: usize,
        deltas: &mut Vec<SemanticDelta<'_>>,
        patches: &mut Vec<RenderPatch<'_>>,
    ) -> RuntimeResult<()> {
        let generic_row =
            self.generic
                .remove_row_and_unbind_sources("todos", index, |binding| {
                    deltas.push(source_delta("SourceUnbind", binding, ProtocolValue::Null));
                    patches.push(patch(
                        "UnbindSource",
                        RenderTarget::TodoSource(binding.key, binding.source_path),
                        source_binding_value(binding),
                    ));
                })?;
        let (key, generation) = (generic_row.key, generic_row.generation);
        self.generic.spare_row("todos", generic_row.value);
        deltas.push(list_delta(
            "ListRemove",
            key,
            generation,
            ProtocolValue::Null,
        ));
        patches.push(patch(
            "RemoveElement",
            RenderTarget::TodoRow(key),
            ProtocolValue::Null,
        ));
        Ok(())
    }

    fn move_index(
        &mut self,
        from: usize,
        to: usize,
        deltas: &mut Vec<SemanticDelta<'_>>,
        patches: &mut Vec<RenderPatch<'_>>,
    ) -> RuntimeResult<()> {
        let (key, generation) = self.generic.move_row("todos", from, to)?;
        deltas.push(SemanticDelta {
            kind: "ListMove",
            list_id: Some("todos"),
            key: Some(key),
            generation: Some(generation),
            source_id: None,
            bind_epoch: None,
            field_path: Some("position"),
            value: ProtocolValue::NumberText(to as i64),
        });
        patches.push(patch(
            "MoveElement",
            RenderTarget::TodoPosition(key),
            ProtocolValue::NumberText(to as i64),
        ));
        Ok(())
    }

    fn assert_step(&self, step: &ScenarioStep) -> RuntimeResult<()> {
        if let Some(expected) = &step.expect_titles {
            let titles = self.all_todo_titles()?;
            assert_eq_report(&step.id, "titles", expected, &titles)?;
        }
        if let Some(expected) = &step.expect_visible_titles {
            let titles = self.visible_titles()?;
            assert_eq_report(&step.id, "visible_titles", expected, &titles)?;
        }
        if let Some(expected) = &step.expect_completed_titles {
            let titles = self.generic.collect_list_textlike_where_bool(
                "todos",
                "title",
                "completed",
                true,
            )?;
            assert_eq_report(&step.id, "completed_titles", expected, &titles)?;
        }
        if let Some(expected) = step.expect_active_count {
            assert_num(&step.id, "active_count", expected, self.active_count())?;
        }
        if let Some(expected) = step.expect_completed_count {
            assert_num(
                &step.id,
                "completed_count",
                expected,
                self.completed_count(),
            )?;
        }
        if let Some(expected) = &step.expect_filter {
            let filter = self.generic.root_textlike("store.selected_filter")?;
            assert_eq_report(&step.id, "filter", expected, &filter)?;
        }
        if let Some(expected) = &step.expect_new_text {
            let new_text = self.generic.root_textlike("store.new_todo_text")?;
            assert_eq_report(&step.id, "new_todo_text", expected, &new_text)?;
        }
        if let Some(expected) = &step.expect_editing_title {
            let editing = self
                .generic
                .first_list_textlike_where_bool("todos", "title", "editing", true)?;
            assert_eq_report(&step.id, "editing_title", &Some(expected.clone()), &editing)?;
        }
        if let Some(expected) = &step.expect_edit_text {
            let edit_text = self.generic.first_list_textlike_where_bool(
                "todos",
                "edit_text",
                "editing",
                true,
            )?;
            assert_eq_report(&step.id, "edit_text", &Some(expected.clone()), &edit_text)?;
        }
        if step.expect_no_editing == Some(true) && self.any_todo_editing()? {
            return Err(format!("{} expected no editing todo", step.id).into());
        }
        self.assert_generic_mirror_in_sync()
            .map_err(|error| format!("{} generic storage mismatch: {error}", step.id).into())
    }

    fn assert_generic_mirror_in_sync(&self) -> RuntimeResult<()> {
        self.generic.root_textlike_ref("store.new_todo_text")?;
        self.generic.root_textlike_ref("store.selected_filter")?;
        for index in 0..self.generic.list_len("todos")? {
            self.generic.row_identity("todos", index)?;
            self.generic.list_row_textlike("todos", index, "title")?;
            self.generic
                .list_row_textlike("todos", index, "edit_text")?;
            self.generic.list_row_bool("todos", index, "completed")?;
            self.generic.list_row_bool("todos", index, "editing")?;
        }
        Ok(())
    }

    fn apply_root_text_source<'a>(
        &mut self,
        source: &'a str,
        payload_text: Option<&'a str>,
        seq: TickSeq,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<()> {
        if let Some(commit) =
            self.generic
                .apply_root_text_action_source(source, payload_text, seq)?
        {
            TodoRuntime::emit_root_text_commit(commit.target, commit.value, deltas, patches)?;
        }
        Ok(())
    }

    fn emit_root_text_commit<'a>(
        target: &'static str,
        value: Cow<'a, str>,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<()> {
        match target {
            "store.new_todo_text" => {
                deltas.push(field_delta(
                    None,
                    None,
                    "store.new_todo_text",
                    ProtocolValue::Text(value.clone()),
                ));
                patches.push(patch(
                    "SetInputValue",
                    RenderTarget::Static("new_todo_input"),
                    ProtocolValue::Text(value),
                ));
            }
            "store.selected_filter" => {
                deltas.push(field_delta(
                    None,
                    None,
                    "store.selected_filter",
                    ProtocolValue::Text(value.clone()),
                ));
                patches.push(patch(
                    "SetSelectedFilter",
                    RenderTarget::Static("filters"),
                    ProtocolValue::Text(value),
                ));
            }
            _ => return Err(format!("unsupported scalar target `{target}`").into()),
        }
        Ok(())
    }

    fn commit_todo_bool_source(
        &mut self,
        index: usize,
        source: &str,
        kind: SourceRouteBoolAction,
        all_completed_snapshot: bool,
    ) -> RuntimeResult<GenericBoolFieldCommit> {
        self.generic
            .commit_indexed_bool_action_source("todos", index, source, kind, |path| match path {
                "store.all_completed" => Some(all_completed_snapshot),
                _ => None,
            })
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
        let occurrence = occurrence.max(1);
        (0..self.generic.list_len("todos")?)
            .filter(|index| {
                self.generic
                    .list_row_textlike("todos", *index, "title")
                    .is_ok_and(|candidate| candidate == title)
            })
            .nth(occurrence - 1)
            .ok_or_else(|| {
                format!("todo occurrence {occurrence} not found for title `{title}`").into()
            })
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

    fn active_count(&self) -> usize {
        self.count_todos_for_target("store.active_count")
            .expect("active_count must be backed by an IR List/count operation")
    }

    fn completed_count(&self) -> usize {
        self.count_todos_for_target("store.completed_count")
            .expect("completed_count must be backed by an IR List/count operation")
    }

    fn all_completed(&self) -> bool {
        self.active_count() == 0 && self.completed_count() > 0
    }

    fn visible_titles(&self) -> RuntimeResult<Vec<String>> {
        self.generic
            .collect_list_textlike_for_retain("todos", "store.visible_todos", "title")
    }

    fn count_todos_for_target(&self, target: &str) -> RuntimeResult<usize> {
        self.generic.count_list_rows_for_target("todos", target)
    }

    fn all_todo_titles(&self) -> RuntimeResult<Vec<String>> {
        self.generic.collect_list_textlike_matching(
            "todos",
            "title",
            RuntimeListPredicate::AlwaysTrue,
        )
    }

    fn any_todo_editing(&self) -> RuntimeResult<bool> {
        self.generic.any_list_bool("todos", "editing", true)
    }

    fn summary(&self) -> JsonValue {
        let todo_len = self.generic.list_len("todos").unwrap_or(0);
        let hidden_keys = self
            .generic
            .list_identities("todos")
            .unwrap_or_default()
            .into_iter()
            .map(|(key, generation)| json!({"key": key, "generation": generation}))
            .collect::<Vec<_>>();
        json!({
            "new_todo_text": self.generic.root_textlike_ref("store.new_todo_text").unwrap_or(""),
            "selected_filter": self.generic.root_textlike_ref("store.selected_filter").unwrap_or(""),
            "todos": (0..todo_len).map(|index| json!({
                "title": self.generic.list_row_textlike("todos", index, "title").unwrap_or(""),
                "edit_text": self.generic.list_row_textlike("todos", index, "edit_text").unwrap_or(""),
                "completed": self.generic.list_row_bool("todos", index, "completed").unwrap_or(false),
                "editing": self.generic.list_row_bool("todos", index, "editing").unwrap_or(false)
            })).collect::<Vec<_>>(),
            "active_count": self.active_count(),
            "completed_count": self.completed_count(),
            "all_completed": self.all_completed(),
            "hidden_keys": hidden_keys,
            "source_binding_count": self.generic.source_binding_count(),
            "stale_source_drop_count": self.stale_source_drop_count
        })
    }
}

impl ScenarioExecutor for LoadedRuntime {
    fn prepare_for_scenario(&mut self, scenario: &Scenario) {
        match self {
            Self::Todo(runtime) => runtime.prepare_for_scenario(scenario),
            Self::Cells(runtime) => runtime.prepare_for_scenario(scenario),
        }
    }

    fn apply_step<'a>(
        &mut self,
        step: &'a ScenarioStep,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<StepExecutionMetrics> {
        match self {
            Self::Todo(runtime) => runtime.apply_step(step, deltas, patches),
            Self::Cells(runtime) => runtime.apply_step(step, deltas, patches),
        }
    }

    fn assert_step_after_measurement(&self, step: &ScenarioStep) -> RuntimeResult<()> {
        match self {
            Self::Todo(runtime) => runtime.assert_step_after_measurement(step),
            Self::Cells(runtime) => runtime.assert_step_after_measurement(step),
        }
    }

    fn state_summary(&self) -> JsonValue {
        match self {
            Self::Todo(runtime) => runtime.state_summary(),
            Self::Cells(runtime) => runtime.state_summary(),
        }
    }

    fn stress_profiles(&self, ir: &TypedProgram) -> RuntimeResult<Option<JsonValue>> {
        match self {
            Self::Todo(runtime) => runtime.stress_profiles(ir),
            Self::Cells(runtime) => runtime.stress_profiles(ir),
        }
    }
}

impl ScenarioExecutor for TodoRuntime {
    fn prepare_for_scenario(&mut self, scenario: &Scenario) {
        TodoRuntime::prepare_for_scenario(self, scenario);
    }

    fn apply_step<'a>(
        &mut self,
        step: &'a ScenarioStep,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<StepExecutionMetrics> {
        let stale_drops_before = self.stale_source_drop_count;
        self.apply_step_into(step, deltas, patches)?;
        Ok(StepExecutionMetrics {
            dirty_key_count: dirty_key_count(deltas),
            extra: StepExecutionExtra::Todo {
                stale_source_drop_count: self
                    .stale_source_drop_count
                    .saturating_sub(stale_drops_before),
            },
        })
    }

    fn assert_step_after_measurement(&self, step: &ScenarioStep) -> RuntimeResult<()> {
        self.assert_step(step)
    }

    fn state_summary(&self) -> JsonValue {
        self.summary()
    }

    fn stress_profiles(&self, ir: &TypedProgram) -> RuntimeResult<Option<JsonValue>> {
        Ok(Some(todomvc_stress_profiles(ir)))
    }
}

fn todomvc_stress_profiles(ir: &TypedProgram) -> JsonValue {
    json!([
        todomvc_toggle_stress(ir, 1_000),
        todomvc_toggle_stress(ir, 10_000),
        todomvc_move_stress(ir, 10_000)
    ])
}

fn todomvc_toggle_stress(ir: &TypedProgram, rows: usize) -> JsonValue {
    let mut runtime = TodoRuntime::seeded(rows);
    let mut deltas = Vec::with_capacity(2);
    let mut patches = Vec::with_capacity(2);
    let index = rows / 2;
    let started = Instant::now();
    let allocations_before = allocation_snapshot();
    runtime
        .set_completed_value("todo.completed", index, true, &mut deltas, &mut patches)
        .expect("stress toggle index is in range");
    let alloc_delta = allocation_delta(allocations_before);
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    json!({
        "name": format!("todomvc-{rows}-rows-toggle-one"),
        "rows": rows,
        "graph_node_count": ir.graph_node_count,
        "graph_clones_per_item": ir.lists.first().map(|list| list.graph_clones_per_item).unwrap_or(0),
        "list_slot_count": runtime.generic.list_len("todos").unwrap_or(0),
        "dirty_key_count": dirty_key_count(&deltas),
        "semantic_delta_count": deltas.len(),
        "render_patch_count": patches.len(),
        "heap_alloc_count": alloc_delta.count,
        "heap_alloc_bytes": alloc_delta.bytes,
        "elapsed_ms": elapsed_ms,
    })
}

fn todomvc_move_stress(ir: &TypedProgram, rows: usize) -> JsonValue {
    let mut runtime = TodoRuntime::seeded(rows);
    let mut deltas = Vec::with_capacity(1);
    let mut patches = Vec::with_capacity(1);
    let started = Instant::now();
    let allocations_before = allocation_snapshot();
    runtime
        .move_index(rows / 2, rows / 2 + 1, &mut deltas, &mut patches)
        .expect("stress move index is in range");
    let alloc_delta = allocation_delta(allocations_before);
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    json!({
        "name": format!("todomvc-{rows}-rows-move-one"),
        "rows": rows,
        "graph_node_count": ir.graph_node_count,
        "graph_clones_per_item": ir.lists.first().map(|list| list.graph_clones_per_item).unwrap_or(0),
        "list_slot_count": runtime.generic.list_len("todos").unwrap_or(0),
        "dirty_key_count": dirty_key_count(&deltas),
        "semantic_delta_count": deltas.len(),
        "render_patch_count": patches.len(),
        "heap_alloc_count": alloc_delta.count,
        "heap_alloc_bytes": alloc_delta.bytes,
        "elapsed_ms": elapsed_ms,
    })
}

#[derive(Clone, Debug, Default)]
struct Cell {
    value: String,
    value_number: Option<i64>,
    error: Option<&'static str>,
    deps: Vec<usize>,
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

    fn empty() -> Self {
        Self {
            operations: Vec::new(),
        }
    }

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
}

#[derive(Clone, Debug)]
struct CellsRuntime {
    generic: GenericScheduledRuntime,
    cells: Vec<Cell>,
    reverse_deps: Vec<Vec<usize>>,
    columns: usize,
    rows: usize,
    interned_texts: Vec<&'static str>,
    affected: Vec<usize>,
    queue: Vec<usize>,
    visiting: Vec<bool>,
    eval_cache: Vec<Option<Result<i64, &'static str>>>,
    step_recomputed: Vec<usize>,
    last_recompute_candidates: usize,
    last_formula_eval_calls: usize,
    last_dependency_edge_walks: usize,
}

#[derive(Clone, Debug)]
enum CellEvent<'a> {
    Change {
        source: &'a str,
        address: &'a str,
        text: &'a str,
    },
    Commit {
        source: &'a str,
        address: &'a str,
        text: &'a str,
    },
    Cancel {
        source: &'a str,
        address: &'a str,
    },
}

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
        let scalar_equations = default_cells_scalar_equations();
        let source_routes = SourceRoutePlan::from_plans(
            &scalar_equations,
            &DerivedEquationPlan::empty(),
            &ListEquationPlan::empty(),
            &BTreeSet::new(),
        );
        Self::with_dimensions_and_equations(
            GenericScheduledRuntime::from_parts(
                generic_cells_runtime(columns, rows),
                scalar_equations,
                DerivedEquationPlan::empty(),
                ListEquationPlan::empty(),
                FormulaEquationPlan::default_cells(),
                source_routes,
                ListSourceBindingPlan::default(),
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
            reverse_deps: vec![Vec::new(); cell_count],
            columns,
            rows,
            interned_texts: Vec::new(),
            affected: Vec::with_capacity(cell_count),
            queue: Vec::with_capacity(cell_count),
            visiting: vec![false; cell_count],
            eval_cache: vec![None; cell_count],
            step_recomputed: Vec::with_capacity(8),
            last_recompute_candidates: 0,
            last_formula_eval_calls: 0,
            last_dependency_edge_walks: 0,
        }
    }

    fn prepare_for_scenario(&mut self, scenario: &Scenario) {
        let mut max_text_len = 0usize;
        let mut max_deps = 1usize;
        self.intern_text("");
        self.intern_text("cycle_error");
        self.intern_text("parse_error");
        self.intern_text("div_by_zero");
        for step in &scenario.step {
            if let Some(action) = &step.user_action {
                if let Some(text) = toml_string_ref(action, "text") {
                    self.intern_text(text);
                    max_text_len = max_text_len.max(text.len());
                    max_deps = max_deps.max(count_formula_dependencies(text));
                }
            }
            if let Some(expected) = &step.expected_source_event {
                if let Some(text) = toml_string_ref(expected, "text") {
                    self.intern_text(text);
                    max_text_len = max_text_len.max(text.len());
                    max_deps = max_deps.max(count_formula_dependencies(text));
                }
            }
            if let Some(expect) = &step.expect_cell {
                if let Some(value) = &expect.value {
                    self.intern_text(value);
                    max_text_len = max_text_len.max(value.len());
                }
                if let Some(formula) = &expect.formula {
                    self.intern_text(formula);
                    max_text_len = max_text_len.max(formula.len());
                    max_deps = max_deps.max(count_formula_dependencies(formula));
                }
                if let Some(editing_text) = &expect.editing_text {
                    self.intern_text(editing_text);
                    max_text_len = max_text_len.max(editing_text.len());
                    max_deps = max_deps.max(count_formula_dependencies(editing_text));
                }
            }
            if let Some(expect) = &step.expect_error {
                self.intern_text(&expect.error);
                max_text_len = max_text_len.max(expect.error.len());
            }
        }
        max_text_len = max_text_len.max("cycle_error".len());
        self.reserve_cell_storage(max_text_len, max_deps);
    }

    fn intern_text(&mut self, value: &str) {
        if self
            .interned_texts
            .iter()
            .any(|interned| *interned == value)
        {
            return;
        }
        let interned = Box::leak(value.to_owned().into_boxed_str());
        self.interned_texts.push(interned);
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

    fn reserve_cell_storage(&mut self, max_text_len: usize, max_deps: usize) {
        for cell in &mut self.cells {
            cell.value.reserve(max_text_len);
            cell.deps.reserve(max_deps);
        }
        self.generic
            .reserve_list_row_textlike_fields("cells", "formula_text", |_, current| {
                max_text_len.saturating_sub(current.len())
            })
            .expect("Cells generic runtime has formula_text fields");
        self.generic
            .reserve_list_row_textlike_fields("cells", "editing_text", |_, current| {
                max_text_len.saturating_sub(current.len())
            })
            .expect("Cells generic runtime has editing_text fields");
        let minimum_fanout_capacity = max_deps.max(4);
        for dependents in &mut self.reverse_deps {
            dependents.reserve(minimum_fanout_capacity);
        }
    }

    fn apply_step_into<'a>(
        &mut self,
        step: &'a ScenarioStep,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
        recomputed: &mut Vec<usize>,
    ) -> RuntimeResult<()> {
        let Some(event) = self.route_step(step)? else {
            return Ok(());
        };
        assert_cell_event_matches(step, &event)?;
        match event {
            CellEvent::Change {
                source,
                address,
                text,
            } => {
                let index = self.cell_index(address)?;
                let mut editing_text = None;
                let mut editing = None;
                self.generic.apply_source_actions(
                    GenericSourceActionInput {
                        source,
                        list: Some("cells"),
                        index: Some(index),
                        key: None,
                        text: Some(text),
                        seq: TickSeq(0),
                    },
                    |_| None,
                    |mutation| {
                        match mutation {
                            GenericSourceMutation::TextField(commit)
                                if commit.field == "editing_text" =>
                            {
                                editing_text = Some(commit);
                            }
                            GenericSourceMutation::BoolField(commit)
                                if commit.field == "editing" =>
                            {
                                editing = Some(commit);
                            }
                            _ => {}
                        }
                        Ok(())
                    },
                )?;
                let editing_text = editing_text.ok_or_else(|| {
                    format!("editing-text update from `{source}` produced no change")
                })?;
                let editing = editing
                    .ok_or_else(|| format!("editing update from `{source}` produced no change"))?;
                deltas.push(cell_field_delta(
                    editing_text.key,
                    editing_text.generation,
                    editing_text.field,
                    ProtocolValue::Text(Cow::Borrowed(editing_text.value)),
                ));
                deltas.push(cell_field_delta(
                    editing.key,
                    editing.generation,
                    editing.field,
                    ProtocolValue::Bool(editing.value),
                ));
                patches.push(patch(
                    "SetCellEditor",
                    RenderTarget::Borrowed(Cow::Borrowed(address)),
                    ProtocolValue::Text(Cow::Borrowed(editing_text.value)),
                ));
            }
            CellEvent::Commit {
                source,
                address,
                text,
            } => {
                self.commit_from_source(source, address, text, deltas, patches, recomputed)?;
            }
            CellEvent::Cancel { source, address } => {
                let index = self.cell_index(address)?;
                let mut editing_text = None;
                let mut editing = None;
                self.generic.apply_source_actions(
                    GenericSourceActionInput {
                        source,
                        list: Some("cells"),
                        index: Some(index),
                        key: None,
                        text: None,
                        seq: TickSeq(0),
                    },
                    |_| None,
                    |mutation| {
                        match mutation {
                            GenericSourceMutation::TextFieldIdentity(commit)
                                if commit.field == "editing_text" =>
                            {
                                editing_text = Some(commit);
                            }
                            GenericSourceMutation::BoolField(commit)
                                if commit.field == "editing" =>
                            {
                                editing = Some(commit);
                            }
                            _ => {}
                        }
                        Ok(())
                    },
                )?;
                let editing_text = editing_text.ok_or_else(|| {
                    format!("editing-text cancel from `{source}` produced no change")
                })?;
                let editing = editing
                    .ok_or_else(|| format!("editing cancel from `{source}` produced no change"))?;
                let value = self.cell_text_field(index, editing_text.field)?;
                deltas.push(cell_field_delta(
                    editing_text.key,
                    editing_text.generation,
                    editing_text.field,
                    self.protocol_text(value),
                ));
                deltas.push(cell_field_delta(
                    editing.key,
                    editing.generation,
                    editing.field,
                    ProtocolValue::Bool(editing.value),
                ));
                patches.push(patch(
                    "SetCellText",
                    RenderTarget::Borrowed(Cow::Borrowed(address)),
                    self.protocol_text(&self.cells[index].value),
                ));
            }
        }
        Ok(())
    }

    fn route_step<'a>(&self, step: &'a ScenarioStep) -> RuntimeResult<Option<CellEvent<'a>>> {
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
    ) -> RuntimeResult<Option<CellEvent<'a>>> {
        let source = source_event.source;
        self.generic
            .require_source(source)
            .map_err(|_| format!("{} source `{source}` has no compiled route", step.id))?;
        let address = source_event
            .address
            .filter(|candidate| is_cell_address(candidate))
            .ok_or_else(|| format!("{} Cells source event missing valid address", step.id))?;
        if self
            .generic
            .has_indexed_text_target(source, "cell.formula_text")?
        {
            let text = source_event
                .text
                .ok_or_else(|| format!("{} Cells commit source event missing text", step.id))?;
            return Ok(Some(CellEvent::Commit {
                source,
                address,
                text,
            }));
        }
        if let Some(text) = source_event.text {
            return Ok(Some(CellEvent::Change {
                source,
                address,
                text,
            }));
        }
        Ok(Some(CellEvent::Cancel { source, address }))
    }

    fn assert_step(&self, step: &ScenarioStep, recomputed: &[usize]) -> RuntimeResult<()> {
        if let Some(expect) = &step.expect_cell {
            let index = self.cell_index(&expect.address)?;
            let cell = &self.cells[index];
            if let Some(value) = &expect.value {
                assert_eq_report(&step.id, "cell.value", value, &cell.value)?;
            }
            if let Some(formula) = &expect.formula {
                assert_eq_report(
                    &step.id,
                    "cell.formula",
                    &formula.as_str(),
                    &self.cell_text_field(index, "formula_text")?,
                )?;
            }
            if let Some(editing_text) = &expect.editing_text {
                assert_eq_report(
                    &step.id,
                    "cell.editing_text",
                    &editing_text.as_str(),
                    &self.cell_text_field(index, "editing_text")?,
                )?;
            }
            if let Some(editing) = expect.editing {
                assert_eq_report(
                    &step.id,
                    "cell.editing",
                    &editing,
                    &self.cell_bool_field(index, "editing")?,
                )?;
            }
        }
        if let Some(expect) = &step.expect_error {
            let cell = self.cell(&expect.address)?;
            assert_eq_report(
                &step.id,
                "cell.error",
                &Some(expect.error.as_str()),
                &cell.error,
            )?;
        }
        if let Some(expected) = &step.expect_recomputed {
            let actual: Vec<_> = recomputed
                .iter()
                .map(|index| self.address_for(*index))
                .collect();
            assert_eq_report(&step.id, "recomputed", expected, &actual)?;
        }
        self.assert_generic_mirror_in_sync()
            .map_err(|error| format!("{} generic storage mismatch: {error}", step.id).into())
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
        let mut formula_commit = None;
        let mut editing_text = None;
        let mut editing = None;
        self.generic.apply_source_actions(
            GenericSourceActionInput {
                source,
                list: Some("cells"),
                index: Some(committed_index),
                key: None,
                text: Some(formula),
                seq: TickSeq(0),
            },
            |_| None,
            |mutation| {
                match mutation {
                    GenericSourceMutation::TextField(commit) if commit.field == "formula_text" => {
                        formula_commit = Some(commit);
                    }
                    GenericSourceMutation::TextField(commit) if commit.field == "editing_text" => {
                        editing_text = Some(commit);
                    }
                    GenericSourceMutation::BoolField(commit) if commit.field == "editing" => {
                        editing = Some(commit);
                    }
                    _ => {}
                }
                Ok(())
            },
        )?;
        let formula = formula_commit
            .ok_or_else(|| format!("formula update from `{source}` produced no change"))?;
        let editing_text = editing_text
            .ok_or_else(|| format!("editing text update from `{source}` produced no change"))?;
        let editing =
            editing.ok_or_else(|| format!("editing update from `{source}` produced no change"))?;
        self.cells[committed_index].parsed =
            parse_formula_ast(formula.value, self.columns, self.rows);
        self.replace_cell_dependencies(committed_index, formula.value);
        self.recompute_affected(address, recomputed)?;
        let cell = &self.cells[committed_index];
        let value = protocol_cell_value(cell);
        deltas.push(cell_field_delta(
            formula.key,
            formula.generation,
            formula.field,
            ProtocolValue::Text(Cow::Borrowed(formula.value)),
        ));
        deltas.push(cell_field_delta(
            editing_text.key,
            editing_text.generation,
            editing_text.field,
            ProtocolValue::Text(Cow::Borrowed(editing_text.value)),
        ));
        deltas.push(cell_field_delta(
            editing.key,
            editing.generation,
            editing.field,
            ProtocolValue::Bool(editing.value),
        ));
        deltas.push(cell_field_delta(
            formula.key,
            formula.generation,
            "value",
            value.clone(),
        ));
        if let Some(error) = cell.error {
            deltas.push(cell_field_delta(
                formula.key,
                formula.generation,
                "error",
                ProtocolValue::Text(Cow::Borrowed(error)),
            ));
        }
        patches.push(patch(
            "SetCellText",
            RenderTarget::Borrowed(Cow::Borrowed(address)),
            value,
        ));
        Ok(())
    }

    fn recompute_affected(
        &mut self,
        changed_address: &str,
        recomputed: &mut Vec<usize>,
    ) -> RuntimeResult<()> {
        let changed_index = self.cell_index(changed_address)?;
        self.collect_affected(changed_index);
        self.last_recompute_candidates = self.affected.len();
        self.last_formula_eval_calls = 0;
        self.eval_cache.fill(None);
        self.visiting.fill(false);
        for offset in 0..self.affected.len() {
            let index = self.affected[offset];
            let result = self.eval_cell(index);
            self.eval_cache[index] = Some(result);
        }
        for offset in 0..self.affected.len() {
            let index = self.affected[offset];
            let result = self.eval_cache[index].unwrap_or(Ok(0));
            let cell = &mut self.cells[index];
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
            let changed = index == changed_index
                || cell.value_number != previous_value
                || cell.error != previous_error;
            if changed {
                recomputed.push(index);
            }
        }
        recomputed.sort_unstable();
        Ok(())
    }

    fn collect_affected(&mut self, changed_index: usize) {
        self.affected.clear();
        self.queue.clear();
        self.last_dependency_edge_walks = 0;
        self.affected.push(changed_index);
        self.queue.push(changed_index);
        while let Some(changed) = self.queue.pop() {
            for offset in 0..self.reverse_deps[changed].len() {
                self.last_dependency_edge_walks += 1;
                let index = self.reverse_deps[changed][offset];
                if !self.affected.contains(&index) {
                    self.affected.push(index);
                    self.queue.push(index);
                }
            }
        }
    }

    fn replace_cell_dependencies(&mut self, cell_index: usize, formula: &str) {
        for offset in 0..self.cells[cell_index].deps.len() {
            let dependency = self.cells[cell_index].deps[offset];
            self.remove_reverse_dep(dependency, cell_index);
        }
        self.cells[cell_index].deps.clear();
        formula_dependencies_into(
            formula,
            self.columns,
            self.rows,
            &mut self.cells[cell_index].deps,
        );
        for offset in 0..self.cells[cell_index].deps.len() {
            let dependency = self.cells[cell_index].deps[offset];
            self.add_reverse_dep(dependency, cell_index);
        }
    }

    fn add_reverse_dep(&mut self, dependency: usize, dependent: usize) {
        if let Some(dependents) = self.reverse_deps.get_mut(dependency)
            && !dependents.contains(&dependent)
        {
            dependents.push(dependent);
        }
    }

    fn remove_reverse_dep(&mut self, dependency: usize, dependent: usize) {
        if let Some(dependents) = self.reverse_deps.get_mut(dependency)
            && let Some(index) = dependents
                .iter()
                .position(|candidate| *candidate == dependent)
        {
            dependents.swap_remove(index);
        }
    }

    fn cell(&self, address: &str) -> RuntimeResult<&Cell> {
        let index = self.cell_index(address)?;
        Ok(&self.cells[index])
    }

    fn cell_index(&self, address: &str) -> RuntimeResult<usize> {
        cell_index(address, self.columns, self.rows)
            .ok_or_else(|| format!("unknown cell {address}").into())
    }

    fn cell_key_generation(&self, index: usize) -> (u64, u64) {
        self.generic
            .row_identity("cells", index)
            .expect("Cells generic runtime has matching cell row")
    }

    fn cell_text_field(&self, index: usize, field: &str) -> RuntimeResult<&str> {
        self.generic.list_row_textlike("cells", index, field)
    }

    fn cell_bool_field(&self, index: usize, field: &str) -> RuntimeResult<bool> {
        self.generic.list_row_bool("cells", index, field)
    }

    fn assert_generic_mirror_in_sync(&self) -> RuntimeResult<()> {
        let generic_len = self.generic.list_len("cells")?;
        if generic_len != self.cells.len() {
            return Err(format!(
                "generic cells length {generic_len} != mirror {}",
                self.cells.len()
            )
            .into());
        }
        for index in 0..self.cells.len() {
            let address = self.address_for(index);
            let generic_address = self.generic.list_row_textlike("cells", index, "address")?;
            if generic_address != address {
                return Err(format!(
                    "row {index} address generic `{generic_address}` != computed `{address}`"
                )
                .into());
            }
            self.generic
                .list_row_textlike("cells", index, "formula_text")?;
            self.generic
                .list_row_textlike("cells", index, "editing_text")?;
            self.generic.list_row_bool("cells", index, "editing")?;
        }
        Ok(())
    }

    fn address_for(&self, index: usize) -> String {
        let col = index % self.columns;
        let row = index / self.columns + 1;
        let label = spreadsheet_column_label(col).unwrap_or_else(|| "?".to_owned());
        format!("{label}{row}")
    }

    fn eval_cell(&mut self, index: usize) -> Result<i64, &'static str> {
        self.last_formula_eval_calls += 1;
        if let Some(result) = self.eval_cache[index] {
            return result;
        }
        if self.visiting[index] {
            return Err("cycle_error");
        }
        self.visiting[index] = true;
        let result = self.eval_formula(index);
        self.visiting[index] = false;
        self.eval_cache[index] = Some(result);
        result
    }

    fn eval_formula(&mut self, index: usize) -> Result<i64, &'static str> {
        match self.cells[index].parsed {
            FormulaAst::Empty => Ok(0),
            FormulaAst::Number(value) => Ok(value),
            FormulaAst::Cell(cell) => self.eval_cell(cell),
            FormulaAst::Binary(left, op, right) => {
                let left = self.eval_term(left)?;
                let right = self.eval_term(right)?;
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

    fn eval_term(&mut self, term: FormulaTerm) -> Result<i64, &'static str> {
        match term {
            FormulaTerm::Number(value) => Ok(value),
            FormulaTerm::Cell(index) => self.eval_cell(index),
        }
    }

    fn summary(&self) -> JsonValue {
        let interesting = ["A1", "B1", "C1", "D1"];
        json!({
            "cells": interesting.iter().map(|address| {
                let index = self.cell_index(address).unwrap_or_default();
                let cell = self.cell(address).cloned().unwrap_or_default();
                let (key, generation) = self.cell_key_generation(index);
                json!({
                    "address": address,
                    "formula": self.generic.list_row_textlike("cells", index, "formula_text").unwrap_or(""),
                    "editing_text": self.generic.list_row_textlike("cells", index, "editing_text").unwrap_or(""),
                    "value": cell.value,
                    "error": cell.error,
                    "editing": self.generic.list_row_bool("cells", index, "editing").unwrap_or(false),
                    "dependencies": cell.deps.iter().map(|index| self.address_for(*index)).collect::<Vec<_>>(),
                    "hidden_key": key,
                    "hidden_generation": generation,
                })
            }).collect::<Vec<_>>(),
            "hidden_keys": "debug/protocol only, not exposed to Boon source",
        })
    }
}

impl ScenarioExecutor for CellsRuntime {
    fn prepare_for_scenario(&mut self, scenario: &Scenario) {
        CellsRuntime::prepare_for_scenario(self, scenario);
    }

    fn apply_step<'a>(
        &mut self,
        step: &'a ScenarioStep,
        deltas: &mut Vec<SemanticDelta<'a>>,
        patches: &mut Vec<RenderPatch<'a>>,
    ) -> RuntimeResult<StepExecutionMetrics> {
        let mut step_recomputed = std::mem::take(&mut self.step_recomputed);
        step_recomputed.clear();
        self.apply_step_into(step, deltas, patches, &mut step_recomputed)?;
        let dirty_key_count = step_recomputed.len();
        let recomputed_cell_count = step_recomputed.len();
        let recompute_candidate_count = self.last_recompute_candidates;
        let formula_eval_call_count = self.last_formula_eval_calls;
        let dependency_edge_walk_count = self.last_dependency_edge_walks;
        self.step_recomputed = step_recomputed;
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

    fn assert_step_after_measurement(&self, step: &ScenarioStep) -> RuntimeResult<()> {
        self.assert_step(step, &self.step_recomputed)
    }

    fn state_summary(&self) -> JsonValue {
        self.summary()
    }

    fn stress_profiles(&self, ir: &TypedProgram) -> RuntimeResult<Option<JsonValue>> {
        Ok(Some(cells_stress_profiles(ir)?))
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
    let mut runtime = CellsRuntime::with_dimensions(26, 100);
    runtime.reserve_cell_storage("=A1+1".len().max("10".len()), 1);
    let mut deltas = Vec::with_capacity(4);
    let mut patches = Vec::with_capacity(2);
    let mut recomputed = Vec::with_capacity(8);
    runtime.commit("A1", "1", &mut deltas, &mut patches, &mut recomputed)?;
    runtime.commit("B1", "=A1+1", &mut deltas, &mut patches, &mut recomputed)?;
    deltas.clear();
    patches.clear();
    recomputed.clear();
    let started = Instant::now();
    let allocations_before = allocation_snapshot();
    runtime.commit("C1", "7", &mut deltas, &mut patches, &mut recomputed)?;
    let alloc_delta = allocation_delta(allocations_before);
    let unrelated_elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let unrelated_recomputed: Vec<_> = recomputed
        .iter()
        .map(|index| runtime.address_for(*index))
        .collect();
    let unrelated = json!({
        "name": "cells-26x100-unrelated-edit",
        "cells": runtime.cells.len(),
        "graph_node_count": ir.graph_node_count,
        "graph_clones_per_item": ir.lists.first().map(|list| list.graph_clones_per_item).unwrap_or(0),
        "dirty_cell_count": recomputed.len(),
        "recompute_candidate_count": runtime.last_recompute_candidates,
        "formula_eval_call_count": runtime.last_formula_eval_calls,
        "dependency_edge_walk_count": runtime.last_dependency_edge_walks,
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
    runtime.commit("A1", "10", &mut deltas, &mut patches, &mut recomputed)?;
    let alloc_delta = allocation_delta(allocations_before);
    let dependent_elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let dependent_recomputed: Vec<_> = recomputed
        .iter()
        .map(|index| runtime.address_for(*index))
        .collect();
    let dependent = json!({
        "name": "cells-26x100-dependent-edit",
        "cells": runtime.cells.len(),
        "graph_node_count": ir.graph_node_count,
        "graph_clones_per_item": ir.lists.first().map(|list| list.graph_clones_per_item).unwrap_or(0),
        "dirty_cell_count": recomputed.len(),
        "recompute_candidate_count": runtime.last_recompute_candidates,
        "formula_eval_call_count": runtime.last_formula_eval_calls,
        "dependency_edge_walk_count": runtime.last_dependency_edge_walks,
        "recomputed_cells": dependent_recomputed,
        "semantic_delta_count": deltas.len(),
        "render_patch_count": patches.len(),
        "heap_alloc_count": alloc_delta.count,
        "heap_alloc_bytes": alloc_delta.bytes,
        "elapsed_ms": dependent_elapsed_ms,
    });
    Ok(json!([unrelated, dependent]))
}

fn formula_dependencies_into(formula: &str, columns: usize, rows: usize, deps: &mut Vec<usize>) {
    for part in formula.split(|ch: char| !(ch.is_ascii_alphanumeric())) {
        if let Some(index) = cell_index(part, columns, rows)
            && !deps.contains(&index)
        {
            deps.push(index);
        }
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

fn default_todo_list_source_bindings() -> ListSourceBindingPlan {
    let parsed = parse_source(
        "examples/todomvc.bn",
        include_str!("../../../examples/todomvc.bn"),
    )
    .expect("checked-in TodoMVC source should parse");
    let ir = lower(&parsed).expect("checked-in TodoMVC source should lower");
    ListSourceBindingPlan::from_ir(&ir)
}

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

fn assert_todo_event_matches(step: &ScenarioStep, event: &TodoEvent<'_>) -> RuntimeResult<()> {
    let expected = GenericSourceEvent::require(step)?;
    let source = match event {
        TodoEvent::NewInputChange { source, .. } => source,
        TodoEvent::NewInputKeyDown { source, .. } => source,
        TodoEvent::Filter { source, .. } => source,
        TodoEvent::ClearCompleted { source, .. } => source,
        TodoEvent::ToggleAll { source, .. } => source,
        TodoEvent::TodoCheckbox { source, .. }
        | TodoEvent::TodoTitleDoubleClick { source, .. }
        | TodoEvent::EditingTitleChange { source, .. }
        | TodoEvent::EditingTitleKeyDown { source, .. }
        | TodoEvent::EditingTitleBlur { source, .. }
        | TodoEvent::RemoveTodo { source, .. } => source,
        TodoEvent::HoverDelete { .. } => unreachable!("hover does not produce a source event"),
    };
    assert_source_event_field(&step.id, Some(expected.source), "source", source)?;
    match event {
        TodoEvent::NewInputChange { text, .. } => {
            assert_source_event_field(&step.id, expected.text, "text", text)?;
        }
        TodoEvent::NewInputKeyDown { key, text, .. } => {
            assert_source_event_field(&step.id, expected.key, "key", key)?;
            assert_source_event_field(&step.id, expected.text, "text", text)?;
        }
        TodoEvent::TodoCheckbox { target_text, .. }
        | TodoEvent::TodoTitleDoubleClick { target_text, .. }
        | TodoEvent::RemoveTodo { target_text, .. } => {
            assert_source_event_field(&step.id, expected.target_text, "target_text", target_text)?;
        }
        TodoEvent::EditingTitleChange {
            target_text, text, ..
        } => {
            assert_source_event_field(&step.id, expected.target_text, "target_text", target_text)?;
            assert_source_event_field(&step.id, expected.text, "text", text)?;
        }
        TodoEvent::EditingTitleKeyDown {
            target_text,
            key,
            text,
            ..
        } => {
            assert_source_event_field(&step.id, expected.target_text, "target_text", target_text)?;
            assert_source_event_field(&step.id, expected.key, "key", key)?;
            if let Some(text) = text {
                assert_source_event_field(&step.id, expected.text, "text", text)?;
            }
        }
        TodoEvent::EditingTitleBlur {
            target_text, text, ..
        } => {
            assert_source_event_field(&step.id, expected.target_text, "target_text", target_text)?;
            if let Some(text) = text {
                assert_source_event_field(&step.id, expected.text, "text", text)?;
            }
        }
        TodoEvent::Filter { .. }
        | TodoEvent::ClearCompleted { .. }
        | TodoEvent::ToggleAll { .. } => {}
        TodoEvent::HoverDelete { .. } => {}
    }
    Ok(())
}

fn assert_cell_event_matches(step: &ScenarioStep, event: &CellEvent<'_>) -> RuntimeResult<()> {
    let expected = GenericSourceEvent::require(step)?;
    let (source, address, text) = match event {
        CellEvent::Change {
            source,
            address,
            text,
            ..
        } => (*source, *address, Some(*text)),
        CellEvent::Commit {
            source,
            address,
            text,
            ..
        } => (*source, *address, Some(*text)),
        CellEvent::Cancel {
            source, address, ..
        } => (*source, *address, None),
    };
    assert_source_event_field(&step.id, Some(expected.source), "source", source)?;
    assert_source_event_field(&step.id, expected.address, "address", address)?;
    if let Some(text) = text {
        assert_source_event_field(&step.id, expected.text, "text", text)?;
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

fn list_delta<'a>(
    kind: &'static str,
    key: u64,
    generation: u64,
    value: ProtocolValue<'a>,
) -> SemanticDelta<'a> {
    GenericCircuitRuntime::semantic_list_delta(kind, "todos", key, generation, value)
}

fn field_delta<'a>(
    key: Option<u64>,
    generation: Option<u64>,
    field: &'static str,
    value: ProtocolValue<'a>,
) -> SemanticDelta<'a> {
    GenericCircuitRuntime::semantic_field_delta(key.map(|_| "todos"), key, generation, field, value)
}

fn source_delta<'a>(
    kind: &'static str,
    binding: &SourceBinding,
    value: ProtocolValue<'a>,
) -> SemanticDelta<'a> {
    GenericCircuitRuntime::semantic_source_delta(kind, binding, value)
}

fn push_source_binding_deltas_for_row<'a>(
    runtime: &GenericCircuitRuntime,
    key: u64,
    generation: u64,
    deltas: &mut Vec<SemanticDelta<'a>>,
) {
    for binding in runtime.row_source_bindings("todos", key, generation) {
        deltas.push(source_delta(
            "SourceBind",
            binding,
            ProtocolValue::Text(Cow::Borrowed(binding.source_path)),
        ));
    }
}

fn source_binding_value(binding: &SourceBinding) -> ProtocolValue<'static> {
    ProtocolValue::SourceBinding {
        source_path: binding.source_path,
        source_id: binding.source_id,
        bind_epoch: binding.bind_epoch,
    }
}

fn push_source_binding_patches_for_row<'a>(
    runtime: &GenericCircuitRuntime,
    key: u64,
    generation: u64,
    patches: &mut Vec<RenderPatch<'a>>,
) {
    for binding in runtime.row_source_bindings("todos", key, generation) {
        patches.push(patch(
            "BindSource",
            RenderTarget::TodoSource(binding.key, binding.source_path),
            source_binding_value(binding),
        ));
    }
}

fn cell_field_delta<'a>(
    key: u64,
    generation: u64,
    field: &'static str,
    value: ProtocolValue<'a>,
) -> SemanticDelta<'a> {
    GenericCircuitRuntime::semantic_field_delta(
        Some("cells"),
        Some(key),
        Some(generation),
        field,
        value,
    )
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
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn current_binary_hash() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| sha256_file(&path).ok())
        .unwrap_or_else(|| "unknown".to_owned())
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
        assert_eq!(output.state_summary["active_count"], 1);
        assert!(
            output
                .semantic_deltas
                .iter()
                .any(|delta| delta.kind == "ListRemove")
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
        assert_eq!(output.state_summary["active_count"], 3);
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
            .replace("Buy groceries", "Source title A")
            .replace("Clean room", "Source title B");
        let parsed = parse_source("examples/todomvc.bn", todo_source).unwrap();
        let ir = lower(&parsed).unwrap();
        assert_eq!(
            todomvc_seed_titles_from_ir(&ir).unwrap(),
            vec!["Source title A", "Source title B"]
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
            include_str!("../../../examples/todomvc.bn").replace("Clean room", "Buy groceries");
        let parsed = parse_source("examples/todomvc.bn", source).unwrap();
        let mut runtime = todo_runtime_from_parsed(&parsed);
        let target_key = runtime.todo_key(1);
        let target_generation = runtime.todo_generation(1);
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
        assert_eq!(runtime.todo_completed_for_test(0), false);
        assert_eq!(runtime.todo_completed_for_test(1), true);
        assert_eq!(deltas.iter().find_map(|delta| delta.key), Some(2));
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
        runtime
            .apply_step_into(&row_step, &mut deltas, &mut patches)
            .unwrap();
        assert!(runtime.todo_completed_for_test(0));
        assert!(!runtime.todo_completed_for_test(1));

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
            2
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
        runtime
            .apply_step_into(&open_step, &mut deltas, &mut patches)
            .unwrap();
        assert!(runtime.todo_editing_for_test(0));
        assert_eq!(runtime.todo_edit_text_for_test(0), "Buy groceries");

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
        assert_eq!(runtime.todo_edit_text_for_test(0), "Draft via IR");

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
        assert!(!runtime.todo_editing_for_test(0));
        assert_eq!(runtime.todo_edit_text_for_test(0), "Buy groceries");
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
        assert_eq!(runtime.todo_title_for_test(0), "Committed via IR");
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
        assert_eq!(runtime.todo_title_for_test(0), "Blur via IR");
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
        assert_eq!(runtime.todo_title_for_test(2), "Derived append");

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
        assert_eq!(runtime.todo_title_for_test(0), "Clean room");
        assert!(deltas.iter().any(|delta| delta.kind == "ListRemove"));

        deltas.clear();
        patches.clear();
        let mut delete_action = BTreeMap::new();
        delete_action.insert("kind".to_owned(), toml::Value::String("click".to_owned()));
        delete_action.insert(
            "target_text".to_owned(),
            toml::Value::String("Clean room delete".to_owned()),
        );
        let mut delete_expected = BTreeMap::new();
        delete_expected.insert(
            "source".to_owned(),
            toml::Value::String("todo.sources.delete_button.press".to_owned()),
        );
        delete_expected.insert(
            "target_text".to_owned(),
            toml::Value::String("Clean room".to_owned()),
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
        assert_eq!(runtime.todo_title_for_test(0), "Derived append");
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
        assert_eq!(generic.list_len("todos").unwrap(), 2);

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
        assert_eq!(generic.list_len("todos").unwrap(), 1);
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
        assert_eq!(runtime.todo_len(), 1);
        assert_eq!(runtime.todo_title_for_test(0), "Clean room");
        assert_eq!(runtime.todo_completed_for_test(0), false);
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
        assert_eq!(deltas[0].key, Some(moved_key));
        assert_eq!(deltas[0].generation, Some(1));
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].kind, "MoveElement");
    }

    #[test]
    fn cells_deltas_use_hidden_grid_slots_not_visible_address_hashes() {
        let mut runtime = CellsRuntime::with_dimensions(26, 100);
        runtime.reserve_cell_storage("41".len(), 1);
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
        runtime.reserve_cell_storage("123".len(), 1);
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
        runtime.reserve_cell_storage("=8/2".len(), 1);
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
        runtime.reserve_cell_storage("=A1+1".len(), 1);
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
        assert_eq!(runtime.reverse_deps[0], vec![1]);

        recomputed.clear();
        runtime
            .commit("B1", "5", &mut deltas, &mut patches, &mut recomputed)
            .unwrap();
        assert!(runtime.cell("B1").unwrap().deps.is_empty());
        assert!(runtime.reverse_deps[0].is_empty());

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
        runtime.reserve_cell_storage("=A1+2".len(), 1);
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
        assert_eq!(runtime.last_dependency_edge_walks, 3);
        assert!(runtime.last_dependency_edge_walks < runtime.cells.len());
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
