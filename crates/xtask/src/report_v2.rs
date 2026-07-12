use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub type ToolResult<T> = Result<T, Box<dyn std::error::Error>>;
pub type ValidationResult<T> = Result<T, String>;

pub const FORMAT_VERSION: u16 = 2;
pub const MANIFEST_RELATIVE_PATH: &str = "docs/architecture/native_gpu_handoff_manifest.json";
pub const MAX_MANIFEST_BYTES: u64 = 32 * 1024;
pub const MAX_REPORT_BYTES: u64 = 512 * 1024;
pub const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CHECKS: usize = 64;
const MAX_PRODUCT_CHECKS: usize = 60;
const MAX_BLOCKERS: usize = 16;
const MAX_ARTIFACTS: usize = 8;
const MAX_PRODUCT_METRICS: usize = 8;
const REPORT_PROTOCOL: &str = "boon-gate-evidence-v2";
const TOOL_CONTRACT: &str = "boon-xtask-report-v2";

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BoundedString<const MAX: usize>(String);

impl<const MAX: usize> BoundedString<MAX> {
    pub fn new(value: impl Into<String>) -> ValidationResult<Self> {
        let value = value.into();
        if value.is_empty() {
            return Err("bounded string must not be empty".to_owned());
        }
        if value.len() > MAX {
            return Err(format!(
                "bounded string is {} bytes; maximum is {MAX}",
                value.len()
            ));
        }
        if value.bytes().any(|byte| byte == 0) {
            return Err("bounded string must not contain NUL".to_owned());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<const MAX: usize> fmt::Debug for BoundedString<MAX> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<const MAX: usize> fmt::Display for BoundedString<MAX> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<const MAX: usize> Serialize for BoundedString<MAX> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de, const MAX: usize> Deserialize<'de> for BoundedString<MAX> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

pub type BoundedId = BoundedString<96>;
pub type ShortText = BoundedString<256>;
pub type DetailText = BoundedString<1024>;
pub type RelativePath = BoundedString<240>;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    pub fn new(value: impl Into<String>) -> ValidationResult<Self> {
        let value = value.into();
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("SHA-256 digest must be 64 lowercase hexadecimal bytes".to_owned());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for Sha256Digest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GitCommit(String);

impl GitCommit {
    pub fn new(value: impl Into<String>) -> ValidationResult<Self> {
        let value = value.into();
        if value.len() != 40
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("git commit must be 40 lowercase hexadecimal bytes".to_owned());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for GitCommit {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for GitCommit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateName {
    Architecture,
    CounterDev,
    TodomvcPhysical,
    Cells,
    Novywave,
    Negative,
}

impl GateName {
    pub const ALL: [Self; 6] = [
        Self::Architecture,
        Self::CounterDev,
        Self::TodomvcPhysical,
        Self::Cells,
        Self::Novywave,
        Self::Negative,
    ];

    pub fn command(self) -> GateCommand {
        match self {
            Self::Architecture => GateCommand::Architecture,
            Self::CounterDev => GateCommand::CounterDev,
            Self::TodomvcPhysical => GateCommand::TodomvcPhysical,
            Self::Cells => GateCommand::Cells,
            Self::Novywave => GateCommand::Novywave,
            Self::Negative => GateCommand::Negative,
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Architecture => "architecture",
            Self::CounterDev => "counter-dev",
            Self::TodomvcPhysical => "todomvc-physical",
            Self::Cells => "cells",
            Self::Novywave => "novywave",
            Self::Negative => "negative",
        }
    }

    pub fn is_timed_product(self) -> bool {
        matches!(
            self,
            Self::CounterDev | Self::TodomvcPhysical | Self::Cells | Self::Novywave
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum GateCommand {
    #[serde(rename = "verify-architecture")]
    Architecture,
    #[serde(rename = "verify-counter-dev")]
    CounterDev,
    #[serde(rename = "verify-todomvc-physical")]
    TodomvcPhysical,
    #[serde(rename = "verify-cells")]
    Cells,
    #[serde(rename = "verify-novywave")]
    Novywave,
    #[serde(rename = "verify-negative")]
    Negative,
}

impl GateCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Architecture => "verify-architecture",
            Self::CounterDev => "verify-counter-dev",
            Self::TodomvcPhysical => "verify-todomvc-physical",
            Self::Cells => "verify-cells",
            Self::Novywave => "verify-novywave",
            Self::Negative => "verify-negative",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AggregateCommand {
    #[serde(rename = "verify-all")]
    VerifyAll,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffManifest {
    pub format: u16,
    pub id: BoundedString<64>,
    pub aggregate: AggregateCommand,
    pub aggregate_output: RelativePath,
    pub aggregate_byte_limit: u64,
    pub gates: Vec<ManifestGate>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestGate {
    pub gate: GateName,
    pub verifier: GateCommand,
    pub output: RelativePath,
    pub byte_limit: u64,
}

impl HandoffManifest {
    pub fn validate(&self) -> ValidationResult<()> {
        if self.format != FORMAT_VERSION {
            return Err(format!(
                "manifest format must be {FORMAT_VERSION}, found {}",
                self.format
            ));
        }
        if self.id.as_str() != "boon-native-handoff-v2" {
            return Err("manifest id must be boon-native-handoff-v2".to_owned());
        }
        if self.gates.len() != GateName::ALL.len() {
            return Err(format!(
                "manifest must contain exactly {} gates",
                GateName::ALL.len()
            ));
        }
        validate_relative_json_path(&self.aggregate_output)?;
        validate_byte_limit(self.aggregate_byte_limit)?;

        let mut outputs = BTreeSet::new();
        for (entry, expected_gate) in self.gates.iter().zip(GateName::ALL) {
            if entry.gate != expected_gate {
                return Err(format!(
                    "manifest gate order mismatch: expected {}, found {}",
                    expected_gate.slug(),
                    entry.gate.slug()
                ));
            }
            if entry.verifier != entry.gate.command() {
                return Err(format!(
                    "manifest gate {} must use {}",
                    entry.gate.slug(),
                    entry.gate.command().as_str()
                ));
            }
            validate_relative_json_path(&entry.output)?;
            validate_byte_limit(entry.byte_limit)?;
            if !outputs.insert(entry.output.as_str()) {
                return Err(format!(
                    "manifest output {} is duplicated",
                    entry.output.as_str()
                ));
            }
        }
        if outputs.contains(self.aggregate_output.as_str()) {
            return Err("aggregate output must differ from every gate output".to_owned());
        }
        Ok(())
    }

    pub fn gate(&self, gate: GateName) -> &ManifestGate {
        self.gates
            .iter()
            .find(|entry| entry.gate == gate)
            .expect("validated manifest contains every v2 gate")
    }
}

fn validate_byte_limit(limit: u64) -> ValidationResult<()> {
    if limit == 0 || limit > MAX_REPORT_BYTES {
        return Err(format!(
            "report byte limit must be between 1 and {MAX_REPORT_BYTES}, found {limit}"
        ));
    }
    Ok(())
}

fn validate_relative_json_path(path: &RelativePath) -> ValidationResult<()> {
    validate_relative_path(path)?;
    if Path::new(path.as_str())
        .extension()
        .and_then(|value| value.to_str())
        != Some("json")
    {
        return Err(format!("report path {path} must end in .json"));
    }
    Ok(())
}

fn validate_relative_path(path: &RelativePath) -> ValidationResult<()> {
    let path = Path::new(path.as_str());
    if path.is_absolute() {
        return Err(format!(
            "path {} must be workspace-relative",
            path.display()
        ));
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!(
            "path {} must not escape the workspace",
            path.display()
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportStatus {
    Pass,
    Fail,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckOutcome {
    Pass,
    Fail,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckEvidence {
    pub id: BoundedId,
    pub outcome: CheckOutcome,
    pub detail: DetailText,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    pub head: GitCommit,
    pub workspace_digest: Sha256Digest,
    pub dirty: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolIdentity {
    pub contract: BoundedString<64>,
    pub contract_digest: Sha256Digest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedIdentity {
    pub source: SourceIdentity,
    pub tooling: ToolIdentity,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GateIdentity {
    pub report_id: BoundedId,
    pub run_id: BoundedId,
    pub gate: GateName,
    pub source: SourceIdentity,
    pub tooling: ToolIdentity,
    pub generated_unix_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FrameEvidenceKey {
    pub frame_id: u64,
    pub input_id: u64,
    pub content_id: u64,
    pub layout_id: u64,
    pub render_id: u64,
    pub surface_epoch: u64,
    pub present_id: u64,
    pub proof_id: u64,
}

impl FrameEvidenceKey {
    fn validate(&self) -> ValidationResult<()> {
        let values = [
            ("frame_id", self.frame_id),
            ("input_id", self.input_id),
            ("content_id", self.content_id),
            ("layout_id", self.layout_id),
            ("render_id", self.render_id),
            ("surface_epoch", self.surface_epoch),
            ("present_id", self.present_id),
            ("proof_id", self.proof_id),
        ];
        for (name, value) in values {
            if value == 0 {
                return Err(format!("frame evidence {name} must be non-zero"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactKind {
    WgpuPngReadback,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactMetadata {
    pub artifact_id: BoundedId,
    pub kind: ArtifactKind,
    pub path: RelativePath,
    pub sha256: Sha256Digest,
    pub byte_len: u64,
    pub frame: FrameEvidenceKey,
}

impl ArtifactMetadata {
    fn validate(&self) -> ValidationResult<()> {
        validate_relative_path(&self.path)?;
        if Path::new(self.path.as_str())
            .extension()
            .and_then(|value| value.to_str())
            != Some("png")
        {
            return Err(format!(
                "WGPU readback artifact {} must be a PNG",
                self.path
            ));
        }
        if self.byte_len == 0 || self.byte_len > MAX_ARTIFACT_BYTES {
            return Err(format!(
                "artifact {} byte length must be between 1 and {MAX_ARTIFACT_BYTES}",
                self.path
            ));
        }
        self.frame.validate()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdapterBackend {
    Vulkan,
    Metal,
    Dx12,
    Gl,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdapterDeviceType {
    IntegratedGpu,
    DiscreteGpu,
    VirtualGpu,
    Cpu,
    Other,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PresentMode {
    Fifo,
    FifoRelaxed,
    Immediate,
    Mailbox,
    AutoVsync,
    AutoNoVsync,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowBackend {
    Wayland,
    X11,
    Windows,
    Macos,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputDelivery {
    NativeOsAppWindowCallback,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostBoundary {
    PublicHostEvent,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureMethod {
    AppOwnedWgpuReadback,
    AppOwnedRenderTargetReadback,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeEvidence {
    pub adapter_name: ShortText,
    pub adapter_backend: AdapterBackend,
    pub adapter_device_type: AdapterDeviceType,
    pub software_adapter: bool,
    pub present_mode: PresentMode,
    pub surface_format: BoundedString<64>,
    pub window_backend: WindowBackend,
    pub preview_pid: u32,
    pub dev_pid: u32,
    pub input_delivery: InputDelivery,
    pub scenario_boundary: HostBoundary,
    pub capture_method: CaptureMethod,
    pub private_runtime_dispatch_used: bool,
}

impl NativeEvidence {
    fn validate_for_passing_product(&self) -> ValidationResult<()> {
        if self.software_adapter || matches!(self.adapter_device_type, AdapterDeviceType::Cpu) {
            return Err("passing native evidence requires a hardware adapter".to_owned());
        }
        if self.preview_pid == 0 || self.dev_pid == 0 || self.preview_pid == self.dev_pid {
            return Err(
                "passing native evidence requires distinct preview and dev PIDs".to_owned(),
            );
        }
        if self.window_backend != WindowBackend::Wayland {
            return Err("passing native evidence requires the native Wayland path".to_owned());
        }
        if self.input_delivery != InputDelivery::NativeOsAppWindowCallback {
            return Err(
                "passing native evidence must enter through the app_window callback path"
                    .to_owned(),
            );
        }
        if self.private_runtime_dispatch_used {
            return Err("private runtime dispatch cannot be passing evidence".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TimingMetric {
    CallbackToHostEvent,
    WarmVisibleInteraction,
    CellsSelection,
    WarmScroll,
    ExampleSwitchAcknowledgement,
    ExampleSwitchFinalPreview,
    AsyncProof,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetricBoundaryPoint {
    WindowCallbackObserved,
    HostEventAccepted,
    FramePresented,
    ExampleSwitchRequested,
    ExampleSwitchAcknowledged,
    ProofRequestedAfterPresent,
    PngPersisted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetricBoundary {
    pub start: MetricBoundaryPoint,
    pub end: MetricBoundaryPoint,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClockOwner {
    NativeRoleMonotonic,
    ProofWorkerMonotonic,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutlierPolicy {
    RetainInSamplesAndCount,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PercentileMethod {
    NearestRank,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SamplePolicy {
    pub minimum_samples: u32,
    pub warmup_samples: u32,
    pub outlier_threshold_us: u64,
    pub outliers: OutlierPolicy,
    pub percentiles: PercentileMethod,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimingBudget {
    pub p95_us: Option<u64>,
    pub p99_us: Option<u64>,
    pub max_us: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetricDefinition {
    pub metric: TimingMetric,
    pub boundary: MetricBoundary,
    pub clock_owner: ClockOwner,
    pub samples: SamplePolicy,
    pub budget: TimingBudget,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, tag = "mode", rename_all = "kebab-case")]
pub enum MeasurementContract {
    NotApplicable {
        reason: ShortText,
    },
    Timed {
        product_ux: Vec<MetricDefinition>,
        async_proof: MetricDefinition,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimingSummary {
    pub sample_count: u32,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
    pub outlier_count: u32,
}

impl TimingSummary {
    fn validate(
        &self,
        definition: &MetricDefinition,
        enforce_budget: bool,
    ) -> ValidationResult<()> {
        if self.sample_count < definition.samples.minimum_samples {
            return Err(format!(
                "{} has {} samples; minimum is {}",
                metric_name(definition.metric),
                self.sample_count,
                definition.samples.minimum_samples
            ));
        }
        if !(self.p50_us <= self.p95_us && self.p95_us <= self.p99_us && self.p99_us <= self.max_us)
        {
            return Err(format!(
                "{} timing percentiles are not monotonic",
                metric_name(definition.metric)
            ));
        }
        if self.outlier_count > self.sample_count {
            return Err(format!(
                "{} outlier count exceeds sample count",
                metric_name(definition.metric)
            ));
        }
        if enforce_budget
            && let Some(limit) = definition.budget.p95_us
            && self.p95_us > limit
        {
            return Err(format!(
                "{} p95 {}us exceeds {limit}us",
                metric_name(definition.metric),
                self.p95_us
            ));
        }
        if enforce_budget
            && let Some(limit) = definition.budget.p99_us
            && self.p99_us > limit
        {
            return Err(format!(
                "{} p99 {}us exceeds {limit}us",
                metric_name(definition.metric),
                self.p99_us
            ));
        }
        if enforce_budget && self.max_us > definition.budget.max_us {
            return Err(format!(
                "{} max {}us exceeds {}us",
                metric_name(definition.metric),
                self.max_us,
                definition.budget.max_us
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProductTimingEvidence {
    pub metric: TimingMetric,
    pub representative_frame: FrameEvidenceKey,
    pub representative_sample_ordinal: u32,
    pub summary: TimingSummary,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AsyncProofTimingEvidence {
    pub linked_product_metric: TimingMetric,
    pub captured_frame: FrameEvidenceKey,
    pub completed_after_frame_id: u64,
    pub proof_lag_frames: u32,
    pub artifact_id: BoundedId,
    pub snapshot_prepare_us: u64,
    pub worker_us: u64,
    pub summary: TimingSummary,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerEvidence {
    pub program: ShortText,
    pub protocol: BoundedString<64>,
    pub exit_code: Option<i32>,
    pub elapsed_ms: u64,
}

impl ProducerEvidence {
    fn validate(&self, report_status: ReportStatus) -> ValidationResult<()> {
        if self.protocol.as_str() != REPORT_PROTOCOL {
            return Err(format!(
                "producer protocol must be {REPORT_PROTOCOL}, found {}",
                self.protocol
            ));
        }
        if report_status == ReportStatus::Pass && self.exit_code != Some(0) {
            return Err("passing report requires a successful producer process".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GateEvidence {
    pub checks: Vec<CheckEvidence>,
    pub producer: Option<ProducerEvidence>,
    pub native: Option<NativeEvidence>,
    pub product_ux_timings: Vec<ProductTimingEvidence>,
    pub async_proof_timing: Option<AsyncProofTimingEvidence>,
    pub artifacts: Vec<ArtifactMetadata>,
}

impl GateEvidence {
    fn validate(
        &self,
        gate: GateName,
        report_status: ReportStatus,
        measurement: &MeasurementContract,
    ) -> ValidationResult<()> {
        if self.checks.is_empty() || self.checks.len() > MAX_CHECKS {
            return Err(format!("gate checks must contain 1..={MAX_CHECKS} entries"));
        }
        if self.product_ux_timings.len() > MAX_PRODUCT_METRICS {
            return Err(format!(
                "product timing evidence exceeds {MAX_PRODUCT_METRICS} entries"
            ));
        }
        if self.artifacts.len() > MAX_ARTIFACTS {
            return Err(format!("artifact metadata exceeds {MAX_ARTIFACTS} entries"));
        }
        let mut check_ids = BTreeSet::new();
        for check in &self.checks {
            if !check_ids.insert(check.id.as_str()) {
                return Err(format!("duplicate check id {}", check.id));
            }
        }
        let mut artifact_ids = BTreeSet::new();
        for artifact in &self.artifacts {
            artifact.validate()?;
            if !artifact_ids.insert(artifact.artifact_id.as_str()) {
                return Err(format!("duplicate artifact id {}", artifact.artifact_id));
            }
        }
        if let Some(producer) = &self.producer {
            producer.validate(report_status)?;
        } else if report_status == ReportStatus::Pass && gate != GateName::Architecture {
            return Err("passing process gate requires producer metadata".to_owned());
        }

        match measurement {
            MeasurementContract::NotApplicable { .. } => {
                if !self.product_ux_timings.is_empty() || self.async_proof_timing.is_some() {
                    return Err(
                        "not-applicable measurement contract cannot carry timing evidence"
                            .to_owned(),
                    );
                }
            }
            MeasurementContract::Timed {
                product_ux,
                async_proof,
            } => {
                self.validate_timed_evidence(report_status, product_ux, async_proof)?;
            }
        }
        Ok(())
    }

    fn validate_timed_evidence(
        &self,
        report_status: ReportStatus,
        definitions: &[MetricDefinition],
        proof_definition: &MetricDefinition,
    ) -> ValidationResult<()> {
        let require_complete = report_status == ReportStatus::Pass;
        if require_complete {
            self.native
                .as_ref()
                .ok_or_else(|| "passing timed report requires native evidence".to_owned())?
                .validate_for_passing_product()?;
            if self.product_ux_timings.len() != definitions.len() {
                return Err(format!(
                    "passing timed report requires {} product metrics, found {}",
                    definitions.len(),
                    self.product_ux_timings.len()
                ));
            }
        }

        let mut observed_metrics = BTreeSet::new();
        for timing in &self.product_ux_timings {
            timing.representative_frame.validate()?;
            if !observed_metrics.insert(timing.metric) {
                return Err(format!(
                    "duplicate product metric {}",
                    metric_name(timing.metric)
                ));
            }
            let definition = definitions
                .iter()
                .find(|definition| definition.metric == timing.metric)
                .ok_or_else(|| {
                    format!("unexpected product metric {}", metric_name(timing.metric))
                })?;
            if timing.representative_frame.frame_id <= 1
                || timing.representative_sample_ordinal <= definition.samples.warmup_samples
            {
                return Err(format!(
                    "{} proof uses a stale first/warmup frame",
                    metric_name(timing.metric)
                ));
            }
            timing.summary.validate(definition, require_complete)?;
        }

        if require_complete {
            for definition in definitions {
                if !observed_metrics.contains(&definition.metric) {
                    return Err(format!(
                        "missing product metric {}",
                        metric_name(definition.metric)
                    ));
                }
            }
        }

        if let Some(proof) = &self.async_proof_timing {
            proof.captured_frame.validate()?;
            if proof_definition.metric != TimingMetric::AsyncProof {
                return Err("async proof definition has the wrong metric".to_owned());
            }
            proof.summary.validate(proof_definition, require_complete)?;
            let accounted_us = proof.snapshot_prepare_us.saturating_add(proof.worker_us);
            if proof.summary.sample_count != 1
                || proof.summary.p50_us != accounted_us
                || proof.summary.p95_us != accounted_us
                || proof.summary.p99_us != accounted_us
                || proof.summary.max_us != accounted_us
            {
                return Err(
                    "async proof summary must account for snapshot preparation plus worker time"
                        .to_owned(),
                );
            }
            let product = self
                .product_ux_timings
                .iter()
                .find(|timing| timing.metric == proof.linked_product_metric)
                .ok_or_else(|| "async proof does not name a product UX metric".to_owned())?;
            if product.representative_frame != proof.captured_frame {
                return Err(
                    "async proof frame identity does not match its product UX frame".to_owned(),
                );
            }
            let expected_completion = proof
                .captured_frame
                .frame_id
                .checked_add(u64::from(proof.proof_lag_frames))
                .ok_or_else(|| "async proof frame lag overflows".to_owned())?;
            if expected_completion != proof.completed_after_frame_id {
                return Err("async proof lag does not match its completion frame".to_owned());
            }
            let artifact = self
                .artifacts
                .iter()
                .find(|artifact| artifact.artifact_id == proof.artifact_id)
                .ok_or_else(|| {
                    "async proof names no path-backed WGPU readback artifact".to_owned()
                })?;
            if artifact.frame != proof.captured_frame {
                return Err(
                    "proof artifact frame identity does not match timing evidence".to_owned(),
                );
            }
        } else if require_complete {
            return Err("passing timed report requires separate async proof timing".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateReportKind {
    Gate,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GateReport {
    pub format: u16,
    pub kind: GateReportKind,
    pub identity: GateIdentity,
    pub status: ReportStatus,
    pub measurement: MeasurementContract,
    pub evidence: GateEvidence,
    pub blockers: Vec<DetailText>,
}

impl GateReport {
    pub fn validate_shape(&self) -> ValidationResult<()> {
        if self.format != FORMAT_VERSION {
            return Err(format!(
                "gate report format must be {FORMAT_VERSION}, found {}",
                self.format
            ));
        }
        if self.identity.generated_unix_ms == 0 {
            return Err("gate identity generation time must be non-zero".to_owned());
        }
        if self.identity.tooling.contract.as_str() != TOOL_CONTRACT {
            return Err(format!("tool contract must be {TOOL_CONTRACT}"));
        }
        if self.blockers.len() > MAX_BLOCKERS {
            return Err(format!("blockers exceed {MAX_BLOCKERS} entries"));
        }
        self.evidence
            .validate(self.identity.gate, self.status, &self.measurement)?;
        let failed_checks = self
            .evidence
            .checks
            .iter()
            .filter(|check| check.outcome == CheckOutcome::Fail)
            .count();
        match self.status {
            ReportStatus::Pass if failed_checks != 0 || !self.blockers.is_empty() => {
                Err("passing report cannot contain failed checks or blockers".to_owned())
            }
            ReportStatus::Fail if failed_checks == 0 || self.blockers.is_empty() => {
                Err("failing report requires a failed check and a blocker".to_owned())
            }
            _ => Ok(()),
        }
    }

    pub fn validate_current(
        &self,
        manifest_gate: &ManifestGate,
        expected: &ExpectedIdentity,
    ) -> ValidationResult<()> {
        self.validate_shape()?;
        if self.identity.gate != manifest_gate.gate {
            return Err(format!(
                "report gate is {}, expected {}",
                self.identity.gate.slug(),
                manifest_gate.gate.slug()
            ));
        }
        if self.identity.source != expected.source {
            return Err(format!(
                "{} report has a stale source identity",
                manifest_gate.gate.slug()
            ));
        }
        if self.identity.tooling != expected.tooling {
            return Err(format!(
                "{} report has a stale tooling identity",
                manifest_gate.gate.slug()
            ));
        }
        Ok(())
    }

    pub fn validate_artifacts(&self, workspace: &Path) -> ValidationResult<()> {
        for artifact in &self.evidence.artifacts {
            validate_artifact_file(workspace, artifact)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceProtocol {
    BoonGateEvidenceV2,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerEnvelope {
    pub format: u16,
    pub protocol: EvidenceProtocol,
    pub gate: GateName,
    pub run_id: BoundedId,
    pub source_digest: Sha256Digest,
    pub evidence: GateEvidence,
}

impl ProducerEnvelope {
    pub fn validate_for(
        &self,
        gate: GateName,
        run_id: &BoundedId,
        source: &SourceIdentity,
    ) -> ValidationResult<()> {
        if self.format != FORMAT_VERSION {
            return Err(format!(
                "producer evidence format must be {FORMAT_VERSION}, found {}",
                self.format
            ));
        }
        if self.gate != gate {
            return Err(format!(
                "producer returned {} evidence for {}",
                self.gate.slug(),
                gate.slug()
            ));
        }
        if &self.run_id != run_id {
            return Err("producer evidence run identity mismatch".to_owned());
        }
        if self.source_digest != source.workspace_digest {
            return Err("producer evidence source identity is stale".to_owned());
        }
        if self.evidence.producer.is_some() {
            return Err("producer cannot self-assert process metadata".to_owned());
        }
        if self.evidence.checks.len() > MAX_PRODUCT_CHECKS {
            return Err(format!(
                "producer checks exceed reserved limit {MAX_PRODUCT_CHECKS}"
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AggregateMode {
    Fresh,
    CheckExisting,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AggregateReportKind {
    Aggregate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChildValidation {
    Valid,
    Invalid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReportFileMetadata {
    pub path: RelativePath,
    pub sha256: Sha256Digest,
    pub byte_len: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AggregateGateResult {
    pub gate: GateName,
    pub verifier: GateCommand,
    pub report: Option<ReportFileMetadata>,
    pub validation: ChildValidation,
    pub outcome: Option<ReportStatus>,
    pub report_id: Option<BoundedId>,
    pub run_id: Option<BoundedId>,
    pub issue: Option<DetailText>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AggregateIdentity {
    pub report_id: BoundedId,
    pub run_id: BoundedId,
    pub source: SourceIdentity,
    pub tooling: ToolIdentity,
    pub generated_unix_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestIdentity {
    pub id: BoundedString<64>,
    pub digest: Sha256Digest,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AggregateReport {
    pub format: u16,
    pub kind: AggregateReportKind,
    pub identity: AggregateIdentity,
    pub mode: AggregateMode,
    pub manifest: ManifestIdentity,
    pub status: ReportStatus,
    pub gates: Vec<AggregateGateResult>,
    pub blockers: Vec<DetailText>,
}

impl AggregateReport {
    pub fn validate(
        &self,
        handoff: &HandoffManifest,
        manifest_digest: &Sha256Digest,
        expected: &ExpectedIdentity,
    ) -> ValidationResult<()> {
        if self.format != FORMAT_VERSION {
            return Err(format!(
                "aggregate format must be {FORMAT_VERSION}, found {}",
                self.format
            ));
        }
        if self.identity.generated_unix_ms == 0 {
            return Err("aggregate generation time must be non-zero".to_owned());
        }
        if self.identity.source != expected.source || self.identity.tooling != expected.tooling {
            return Err("aggregate identity is stale".to_owned());
        }
        if self.manifest.id != handoff.id || &self.manifest.digest != manifest_digest {
            return Err("aggregate manifest identity mismatch".to_owned());
        }
        if self.gates.len() != handoff.gates.len() {
            return Err("aggregate must contain exactly six gate results".to_owned());
        }
        if self.blockers.len() > MAX_BLOCKERS {
            return Err(format!("aggregate blockers exceed {MAX_BLOCKERS} entries"));
        }

        let mut all_pass = true;
        for (result, entry) in self.gates.iter().zip(&handoff.gates) {
            if result.gate != entry.gate || result.verifier != entry.verifier {
                return Err(format!(
                    "aggregate gate order/command mismatch at {}",
                    entry.gate.slug()
                ));
            }
            if let Some(metadata) = &result.report {
                validate_relative_json_path(&metadata.path)?;
                if metadata.path != entry.output {
                    return Err(format!(
                        "aggregate {} report path differs from manifest",
                        entry.gate.slug()
                    ));
                }
                if metadata.byte_len == 0 || metadata.byte_len > entry.byte_limit {
                    return Err(format!(
                        "aggregate {} report byte length is outside its manifest bound",
                        entry.gate.slug()
                    ));
                }
            }
            match result.validation {
                ChildValidation::Valid => {
                    if result.report.is_none()
                        || result.outcome.is_none()
                        || result.report_id.is_none()
                        || result.run_id.is_none()
                        || result.issue.is_some()
                    {
                        return Err(format!(
                            "valid {} aggregate entry is incomplete",
                            entry.gate.slug()
                        ));
                    }
                    if self.mode == AggregateMode::Fresh
                        && result.run_id.as_ref() != Some(&self.identity.run_id)
                    {
                        return Err(format!(
                            "fresh aggregate {} run identity mismatch",
                            entry.gate.slug()
                        ));
                    }
                    all_pass &= result.outcome == Some(ReportStatus::Pass);
                }
                ChildValidation::Invalid => {
                    if result.outcome.is_some()
                        || result.report_id.is_some()
                        || result.run_id.is_some()
                        || result.issue.is_none()
                    {
                        return Err(format!(
                            "invalid {} aggregate entry has contradictory fields",
                            entry.gate.slug()
                        ));
                    }
                    all_pass = false;
                }
            }
        }

        match self.status {
            ReportStatus::Pass if !all_pass || !self.blockers.is_empty() => {
                Err("passing aggregate has failed/invalid gates or blockers".to_owned())
            }
            ReportStatus::Fail if all_pass || self.blockers.is_empty() => {
                Err("failing aggregate requires a failed/invalid gate and blocker".to_owned())
            }
            _ => Ok(()),
        }
    }
}

pub fn measurement_contract(gate: GateName) -> MeasurementContract {
    if !gate.is_timed_product() {
        return MeasurementContract::NotApplicable {
            reason: ShortText::new("structural or rejection gate has no product timing samples")
                .expect("static reason is bounded"),
        };
    }

    let mut product_ux = vec![
        metric_definition(
            TimingMetric::CallbackToHostEvent,
            MetricBoundaryPoint::WindowCallbackObserved,
            MetricBoundaryPoint::HostEventAccepted,
            1_000,
            None,
            Some(1_000),
            2_000,
            60,
            10,
        ),
        metric_definition(
            TimingMetric::WarmVisibleInteraction,
            MetricBoundaryPoint::HostEventAccepted,
            MetricBoundaryPoint::FramePresented,
            16_700,
            Some(16_700),
            None,
            33_400,
            60,
            10,
        ),
    ];
    if gate == GateName::Cells {
        product_ux.push(metric_definition(
            TimingMetric::CellsSelection,
            MetricBoundaryPoint::HostEventAccepted,
            MetricBoundaryPoint::FramePresented,
            16_700,
            Some(16_700),
            None,
            33_400,
            20,
            4,
        ));
        product_ux.push(metric_definition(
            TimingMetric::WarmScroll,
            MetricBoundaryPoint::HostEventAccepted,
            MetricBoundaryPoint::FramePresented,
            16_700,
            Some(16_700),
            None,
            33_400,
            120,
            20,
        ));
    }
    if gate == GateName::CounterDev {
        product_ux.push(metric_definition(
            TimingMetric::ExampleSwitchAcknowledgement,
            MetricBoundaryPoint::ExampleSwitchRequested,
            MetricBoundaryPoint::ExampleSwitchAcknowledged,
            16_700,
            Some(16_700),
            None,
            33_400,
            20,
            3,
        ));
        product_ux.push(metric_definition(
            TimingMetric::ExampleSwitchFinalPreview,
            MetricBoundaryPoint::ExampleSwitchRequested,
            MetricBoundaryPoint::FramePresented,
            250_000,
            Some(250_000),
            None,
            500_000,
            20,
            3,
        ));
    }
    let async_proof = MetricDefinition {
        metric: TimingMetric::AsyncProof,
        boundary: MetricBoundary {
            start: MetricBoundaryPoint::ProofRequestedAfterPresent,
            end: MetricBoundaryPoint::PngPersisted,
        },
        clock_owner: ClockOwner::ProofWorkerMonotonic,
        samples: SamplePolicy {
            minimum_samples: 1,
            warmup_samples: 0,
            outlier_threshold_us: 500_000,
            outliers: OutlierPolicy::RetainInSamplesAndCount,
            percentiles: PercentileMethod::NearestRank,
        },
        budget: TimingBudget {
            p95_us: None,
            p99_us: None,
            max_us: 5_000_000,
        },
    };
    MeasurementContract::Timed {
        product_ux,
        async_proof,
    }
}

#[allow(clippy::too_many_arguments)]
fn metric_definition(
    metric: TimingMetric,
    start: MetricBoundaryPoint,
    end: MetricBoundaryPoint,
    outlier_threshold_us: u64,
    p95_us: Option<u64>,
    p99_us: Option<u64>,
    max_us: u64,
    minimum_samples: u32,
    warmup_samples: u32,
) -> MetricDefinition {
    MetricDefinition {
        metric,
        boundary: MetricBoundary { start, end },
        clock_owner: ClockOwner::NativeRoleMonotonic,
        samples: SamplePolicy {
            minimum_samples,
            warmup_samples,
            outlier_threshold_us,
            outliers: OutlierPolicy::RetainInSamplesAndCount,
            percentiles: PercentileMethod::NearestRank,
        },
        budget: TimingBudget {
            p95_us,
            p99_us,
            max_us,
        },
    }
}

fn metric_name(metric: TimingMetric) -> &'static str {
    match metric {
        TimingMetric::CallbackToHostEvent => "callback-to-host-event",
        TimingMetric::WarmVisibleInteraction => "warm-visible-interaction",
        TimingMetric::CellsSelection => "cells-selection",
        TimingMetric::WarmScroll => "warm-scroll",
        TimingMetric::ExampleSwitchAcknowledgement => "example-switch-acknowledgement",
        TimingMetric::ExampleSwitchFinalPreview => "example-switch-final-preview",
        TimingMetric::AsyncProof => "async-proof",
    }
}

pub fn load_manifest(workspace: &Path) -> ToolResult<(HandoffManifest, Sha256Digest)> {
    let path = workspace.join(MANIFEST_RELATIVE_PATH);
    let bytes = read_bounded(&path, MAX_MANIFEST_BYTES)?;
    let manifest: HandoffManifest = serde_json::from_slice(&bytes)?;
    manifest
        .validate()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok((manifest, sha256_bytes(&bytes)))
}

pub fn read_gate_report(path: &Path, byte_limit: u64) -> ToolResult<GateReport> {
    let bytes = read_bounded(path, byte_limit)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn read_producer_envelope(path: &Path) -> ToolResult<ProducerEnvelope> {
    let bytes = read_bounded(path, MAX_REPORT_BYTES)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn write_gate_report(path: &Path, report: &GateReport, byte_limit: u64) -> ToolResult<()> {
    report
        .validate_shape()
        .map_err(|error| format!("refusing to write invalid gate report: {error}"))?;
    write_atomic_json(path, report, byte_limit)
}

pub fn write_aggregate_report(
    path: &Path,
    report: &AggregateReport,
    byte_limit: u64,
) -> ToolResult<()> {
    write_atomic_json(path, report, byte_limit)
}

fn write_atomic_json<T: Serialize>(path: &Path, value: &T, byte_limit: u64) -> ToolResult<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > byte_limit {
        return Err(format!(
            "serialized report is {} bytes; limit is {byte_limit}",
            bytes.len()
        )
        .into());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let suffix = format!("tmp-{}-{}", std::process::id(), unix_time_ms());
    let temp = path.with_extension(suffix);
    fs::write(&temp, bytes)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&temp, path)?;
    Ok(())
}

fn read_bounded(path: &Path, byte_limit: u64) -> ToolResult<Vec<u8>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(format!("{} is not a regular file", path.display()).into());
    }
    if metadata.len() == 0 || metadata.len() > byte_limit {
        return Err(format!(
            "{} is {} bytes; expected 1..={byte_limit}",
            path.display(),
            metadata.len()
        )
        .into());
    }
    Ok(fs::read(path)?)
}

pub fn current_identity(workspace: &Path) -> ToolResult<ExpectedIdentity> {
    let source = current_source_identity(workspace)?;
    let executable = std::env::current_exe()?;
    let mut hasher = Sha256::new();
    hash_file_into(&executable, &mut hasher)?;
    hash_file_into(&workspace.join(MANIFEST_RELATIVE_PATH), &mut hasher)?;
    hasher.update(FORMAT_VERSION.to_le_bytes());
    let tooling = ToolIdentity {
        contract: BoundedString::new(TOOL_CONTRACT)?,
        contract_digest: digest_from_hasher(hasher),
    };
    Ok(ExpectedIdentity { source, tooling })
}

fn current_source_identity(workspace: &Path) -> ToolResult<SourceIdentity> {
    let head_text = git_stdout(workspace, &["rev-parse", "HEAD"])?;
    let head = GitCommit::new(String::from_utf8(head_text)?.trim().to_owned())?;
    let diff = git_stdout(workspace, &["diff", "--binary", "HEAD", "--", "."])?;
    let untracked = git_stdout(
        workspace,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".",
        ],
    )?;

    let mut hasher = Sha256::new();
    hasher.update(b"boon-source-identity-v2\0");
    hasher.update(head.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(&diff);
    for raw_path in untracked
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let relative = std::str::from_utf8(raw_path)?;
        hasher.update([0]);
        hasher.update(raw_path);
        hasher.update([0]);
        let path = workspace.join(relative);
        if path.is_file() {
            hash_file_into(&path, &mut hasher)?;
        }
    }
    Ok(SourceIdentity {
        head,
        workspace_digest: digest_from_hasher(hasher),
        dirty: !diff.is_empty() || !untracked.is_empty(),
    })
}

fn git_stdout(workspace: &Path, args: &[&str]) -> ToolResult<Vec<u8>> {
    let output = Command::new("git")
        .current_dir(workspace)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(output.stdout)
}

pub fn sha256_bytes(bytes: &[u8]) -> Sha256Digest {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    digest_from_hasher(hasher)
}

pub fn sha256_file(path: &Path) -> ToolResult<Sha256Digest> {
    let mut hasher = Sha256::new();
    hash_file_into(path, &mut hasher)?;
    Ok(digest_from_hasher(hasher))
}

fn hash_file_into(path: &Path, hasher: &mut Sha256) -> ToolResult<()> {
    let mut file = fs::File::open(path)?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(())
}

fn digest_from_hasher(hasher: Sha256) -> Sha256Digest {
    Sha256Digest(format!("{:x}", hasher.finalize()))
}

pub fn report_file_metadata(
    workspace: &Path,
    relative: &RelativePath,
    byte_limit: u64,
) -> ToolResult<ReportFileMetadata> {
    validate_relative_json_path(relative).map_err(|error| error.to_string())?;
    let path = workspace.join(relative.as_str());
    let metadata = fs::metadata(&path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > byte_limit {
        return Err(format!(
            "{} is not a bounded report file (1..={byte_limit} bytes)",
            path.display()
        )
        .into());
    }
    Ok(ReportFileMetadata {
        path: relative.clone(),
        sha256: sha256_file(&path)?,
        byte_len: metadata.len(),
    })
}

fn validate_artifact_file(workspace: &Path, artifact: &ArtifactMetadata) -> ValidationResult<()> {
    let root = workspace
        .canonicalize()
        .map_err(|error| format!("canonicalize workspace: {error}"))?;
    let path = workspace.join(artifact.path.as_str());
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("open proof artifact {}: {error}", path.display()))?;
    if !canonical.starts_with(&root) {
        return Err(format!(
            "proof artifact {} resolves outside the workspace",
            path.display()
        ));
    }
    let metadata = fs::metadata(&canonical)
        .map_err(|error| format!("stat proof artifact {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() != artifact.byte_len {
        return Err(format!(
            "proof artifact {} length does not match metadata",
            path.display()
        ));
    }
    let bytes = fs::read(&canonical)
        .map_err(|error| format!("read proof artifact {}: {error}", path.display()))?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(format!("proof artifact {} is not a PNG", path.display()));
    }
    if sha256_bytes(&bytes) != artifact.sha256 {
        return Err(format!("proof artifact {} digest mismatch", path.display()));
    }
    Ok(())
}

pub fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

pub fn make_run_id(prefix: &str) -> ValidationResult<BoundedId> {
    let id = format!("{prefix}-{}-{}", unix_time_ms(), std::process::id());
    BoundedId::new(id)
}

pub fn make_report_id(run_id: &BoundedId, subject: &str) -> ValidationResult<BoundedId> {
    let digest = sha256_bytes(
        format!(
            "{}\0{subject}\0{}\0{}",
            run_id.as_str(),
            unix_time_ms(),
            std::process::id()
        )
        .as_bytes(),
    );
    BoundedId::new(format!("{subject}-{}", &digest.as_str()[..24]))
}

pub fn detail(value: impl AsRef<str>) -> DetailText {
    let value = value.as_ref();
    let value = if value.is_empty() {
        "unspecified failure"
    } else {
        value
    };
    if value.len() <= 1024 {
        return DetailText::new(value.to_owned()).expect("non-empty bounded detail");
    }
    let mut end = 1024;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    DetailText::new(value[..end].to_owned()).expect("truncated bounded detail")
}

pub fn check(
    id: impl Into<String>,
    outcome: CheckOutcome,
    detail_value: impl AsRef<str>,
) -> CheckEvidence {
    CheckEvidence {
        id: BoundedId::new(id).expect("check IDs are static and bounded"),
        outcome,
        detail: detail(detail_value),
    }
}

pub fn protocol_name() -> BoundedString<64> {
    BoundedString::new(REPORT_PROTOCOL).expect("static protocol name is bounded")
}

pub fn empty_evidence(checks: Vec<CheckEvidence>) -> GateEvidence {
    GateEvidence {
        checks,
        producer: None,
        native: None,
        product_ux_timings: Vec::new(),
        async_proof_timing: None,
        artifacts: Vec::new(),
    }
}

pub fn gate_report(
    gate: GateName,
    run_id: BoundedId,
    expected: ExpectedIdentity,
    status: ReportStatus,
    evidence: GateEvidence,
    blockers: Vec<DetailText>,
) -> ValidationResult<GateReport> {
    let report = GateReport {
        format: FORMAT_VERSION,
        kind: GateReportKind::Gate,
        identity: GateIdentity {
            report_id: make_report_id(&run_id, gate.slug())?,
            run_id,
            gate,
            source: expected.source,
            tooling: expected.tooling,
            generated_unix_ms: unix_time_ms(),
        },
        status,
        measurement: measurement_contract(gate),
        evidence,
        blockers,
    };
    report.validate_shape()?;
    Ok(report)
}

pub fn workspace_path(workspace: &Path, relative: &RelativePath) -> PathBuf {
    workspace.join(relative.as_str())
}
