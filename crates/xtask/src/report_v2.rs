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
pub const MAX_SIDECAR_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CHECKS: usize = 64;
const MAX_PRODUCT_CHECKS: usize = 60;
const MAX_BLOCKERS: usize = 16;
const MAX_PRODUCT_METRICS: usize = 8;
const MAX_PROFILE_ARGUMENTS: usize = 24;
const MAX_PROFILE_CHECKPOINTS: usize = 32;
const MAX_ARTIFACTS: usize = MAX_PROFILE_CHECKPOINTS + MAX_PRODUCT_METRICS;
const MAX_ASYNC_LANES: usize = 16;
const MAX_BUDGET_METRICS: usize = 32;
const MAX_HANDOFF_GATES: usize = 32;
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

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GateName(BoundedString<64>);

impl GateName {
    pub fn new(value: impl Into<String>) -> ValidationResult<Self> {
        let value = BoundedString::new(value)?;
        validate_kebab_identifier(value.as_str(), "gate")?;
        Ok(Self(value))
    }

    pub fn slug(&self) -> &str {
        self.0.as_str()
    }

    fn verifier_command(&self) -> ValidationResult<GateCommand> {
        GateCommand::new(format!("verify-{}", self.slug()))
    }
}

impl Serialize for GateName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.slug())
    }
}

impl<'de> Deserialize<'de> for GateName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GateCommand(BoundedString<96>);

impl GateCommand {
    pub fn new(value: impl Into<String>) -> ValidationResult<Self> {
        let value = BoundedString::new(value)?;
        let Some(suffix) = value.as_str().strip_prefix("verify-") else {
            return Err("gate verifier command must start with verify-".to_owned());
        };
        validate_kebab_identifier(suffix, "gate verifier suffix")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl Serialize for GateCommand {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GateCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateRunner {
    Architecture,
    NativeProduct,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AggregateCommand {
    #[serde(rename = "verify-all")]
    VerifyAll,
}

impl AggregateCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VerifyAll => "verify-all",
        }
    }
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
    pub order: u16,
    pub gate: GateName,
    pub verifier: GateCommand,
    pub runner: GateRunner,
    pub output: RelativePath,
    pub byte_limit: u64,
    pub sidecar_byte_limit: u64,
    #[serde(default)]
    pub profile: Option<VerifierProfile>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerifierProfile {
    pub id: BoundedId,
    pub arguments: Vec<VerifierArgument>,
    pub measurements: Vec<TimingMetric>,
    #[serde(default)]
    pub proof_requirements: ProfileProofRequirements,
}

impl VerifierProfile {
    pub fn digest(&self) -> Sha256Digest {
        let bytes = serde_json::to_vec(self).expect("validated verifier profile serializes");
        sha256_bytes(&bytes)
    }

    pub fn argument(&self, flag: &str) -> Option<&str> {
        self.arguments
            .iter()
            .find(|argument| argument.flag.as_str() == flag)
            .map(|argument| argument.value.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerifierArgument {
    pub flag: BoundedString<64>,
    pub value: RelativePath,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileProofRequirements {
    pub scenario: Option<ScenarioRequirement>,
    pub budget: Option<BudgetRequirement>,
    pub state_root: Option<StateRootRequirement>,
    pub native_workflow: Option<NativeWorkflowRequirement>,
    #[serde(default)]
    pub async_lanes: Vec<AsyncLaneKind>,
    #[serde(default)]
    pub checkpoints: Vec<CheckpointRequirement>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckpointRequirement {
    pub id: BoundedId,
    #[serde(flatten)]
    pub evidence: CheckpointEvidenceRequirement,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CheckpointEvidenceRequirement {
    ScenarioStep {
        scenario_step: BoundedId,
    },
    RestartRestore {
        baseline_checkpoint: BoundedId,
    },
    ResponsiveLayout {
        baseline_checkpoint: BoundedId,
        logical_width: u32,
    },
    StaleCompileRejection,
    PersistenceOperation {
        operation: PersistenceEvidenceOperation,
    },
    NativeWorkflowStep {
        scenario_step: BoundedId,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWorkflowRequirement {
    pub delivery: NativeWorkflowDelivery,
    pub scenario_boundary: NativeWorkflowScenarioBoundary,
    pub capture_method: CaptureMethod,
    pub durability: NativeWorkflowDurability,
    pub steps: Vec<BoundedId>,
    pub proof_steps: Vec<BoundedId>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeWorkflowDelivery {
    KernelUinputIsolatedSeat,
}

impl NativeWorkflowDelivery {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::KernelUinputIsolatedSeat => "kernel-uinput-isolated-seat",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeWorkflowScenarioBoundary {
    KernelUinputAndSemanticAssertions,
}

impl NativeWorkflowScenarioBoundary {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::KernelUinputAndSemanticAssertions => "kernel-uinput-and-semantic-assertions",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeWorkflowDurability {
    StateChangingStepsAcked,
}

impl NativeWorkflowDurability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StateChangingStepsAcked => "state-changing-steps-acked",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PersistenceEvidenceOperation {
    Exported,
    CorruptionRejected,
    ClearedAndStartedOver,
    ImportPreviewed,
    ImportActivated,
    MigrationActivated,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioRequirement {
    pub path: RelativePath,
    pub semantic_assertions: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetRequirement {
    pub path: RelativePath,
    pub metrics: Vec<BoundedId>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StateRootPolicy {
    LaunchScopedClean,
}

impl StateRootPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LaunchScopedClean => "launch-scoped-clean",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StateRootRequirement {
    pub policy: StateRootPolicy,
    pub restart_required: bool,
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
        if self.gates.is_empty() || self.gates.len() > MAX_HANDOFF_GATES {
            return Err(format!(
                "manifest gates must contain 1..={MAX_HANDOFF_GATES} entries"
            ));
        }
        validate_relative_json_path(&self.aggregate_output)?;
        validate_byte_limit(self.aggregate_byte_limit)?;

        let mut gate_names = BTreeSet::new();
        let mut verifiers = BTreeSet::new();
        let mut outputs = BTreeSet::new();
        let mut architecture_runners = 0;
        for (index, entry) in self.gates.iter().enumerate() {
            if usize::from(entry.order) != index {
                return Err(format!(
                    "manifest gate {} has order {}; expected {index}",
                    entry.gate.slug(),
                    entry.order
                ));
            }
            if !gate_names.insert(entry.gate.slug()) {
                return Err(format!("manifest gate {} is duplicated", entry.gate.slug()));
            }
            if !verifiers.insert(entry.verifier.as_str()) {
                return Err(format!(
                    "manifest verifier {} is duplicated",
                    entry.verifier.as_str()
                ));
            }
            let expected_verifier = entry.gate.verifier_command()?;
            if entry.verifier != expected_verifier {
                return Err(format!(
                    "manifest gate {} must use {}",
                    entry.gate.slug(),
                    expected_verifier.as_str()
                ));
            }
            if entry.verifier.as_str() == self.aggregate.as_str()
                || entry.verifier.as_str() == "shaders"
            {
                return Err(format!(
                    "manifest verifier {} conflicts with a fixed xtask command",
                    entry.verifier.as_str()
                ));
            }
            validate_relative_json_path(&entry.output)?;
            validate_byte_limit(entry.byte_limit)?;
            validate_sidecar_byte_limit(entry.sidecar_byte_limit)?;
            if !outputs.insert(entry.output.as_str()) {
                return Err(format!(
                    "manifest output {} is duplicated",
                    entry.output.as_str()
                ));
            }
            match (entry.runner, &entry.profile) {
                (GateRunner::Architecture, None) => {
                    architecture_runners += 1;
                    if index != 0 {
                        return Err(
                            "the architecture runner must be the first manifest gate".to_owned()
                        );
                    }
                    if entry.sidecar_byte_limit != 0 {
                        return Err(
                            "the architecture runner must have a zero sidecar byte limit"
                                .to_owned(),
                        );
                    }
                }
                (GateRunner::NativeProduct, None) => {
                    return Err(format!(
                        "manifest gate {} requires a verifier profile",
                        entry.gate.slug()
                    ));
                }
                (GateRunner::Architecture, Some(_)) => {
                    return Err(
                        "the architecture runner must not launch a verifier profile".to_owned()
                    );
                }
                (GateRunner::NativeProduct, Some(profile)) => {
                    profile.validate(entry.sidecar_byte_limit)?
                }
            }
        }
        if architecture_runners != 1 {
            return Err("manifest must contain exactly one architecture runner".to_owned());
        }
        if outputs.contains(self.aggregate_output.as_str()) {
            return Err("aggregate output must differ from every gate output".to_owned());
        }
        Ok(())
    }

    pub fn gate(&self, gate: &GateName) -> &ManifestGate {
        self.gates
            .iter()
            .find(|entry| &entry.gate == gate)
            .expect("validated manifest contains the selected gate")
    }

    pub fn gate_for_verifier(&self, verifier: &str) -> Option<&ManifestGate> {
        self.gates
            .iter()
            .find(|entry| entry.verifier.as_str() == verifier)
    }
}

impl VerifierProfile {
    fn validate(&self, sidecar_byte_limit: u64) -> ValidationResult<()> {
        const ALLOWED_ARGUMENTS: [&str; 9] = [
            "--harness",
            "--example",
            "--visible-mode",
            "--visible-samples",
            "--alternate-target",
            "--selection-samples",
            "--scroll-samples",
            "--switch-samples",
            "--profile-benchmark-steps",
        ];
        const REQUIRED_TIMED_ARGUMENTS: [&str; 8] = [
            "--harness",
            "--example",
            "--visible-mode",
            "--visible-samples",
            "--alternate-target",
            "--selection-samples",
            "--scroll-samples",
            "--switch-samples",
        ];
        if self.arguments.is_empty() || self.arguments.len() > MAX_PROFILE_ARGUMENTS {
            return Err(format!(
                "verifier profile {} arguments must contain 1..={MAX_PROFILE_ARGUMENTS} entries",
                self.id
            ));
        }
        let mut flags = BTreeSet::new();
        for argument in &self.arguments {
            validate_profile_flag(&argument.flag)?;
            if !ALLOWED_ARGUMENTS.contains(&argument.flag.as_str()) {
                return Err(format!(
                    "verifier profile {} has unsupported argument {}",
                    self.id, argument.flag
                ));
            }
            if !flags.insert(argument.flag.as_str()) {
                return Err(format!(
                    "verifier profile {} duplicates argument {}",
                    self.id, argument.flag
                ));
            }
        }
        let harness = self
            .argument("--harness")
            .ok_or_else(|| format!("verifier profile {} requires --harness", self.id))?;
        match harness {
            "timed" => {
                if self.argument("--example").is_none_or(str::is_empty) {
                    return Err(format!(
                        "timed verifier profile {} requires --example",
                        self.id
                    ));
                }
                for required in REQUIRED_TIMED_ARGUMENTS {
                    if self.argument(required).is_none() {
                        return Err(format!(
                            "timed verifier profile {} requires {required}",
                            self.id
                        ));
                    }
                }
                if self.measurements.is_empty() {
                    return Err(format!(
                        "timed verifier profile {} requires product measurements",
                        self.id
                    ));
                }
                if sidecar_byte_limit == 0 {
                    return Err(format!(
                        "timed verifier profile {} requires a non-zero sidecar byte limit",
                        self.id
                    ));
                }
            }
            "negative" => {
                if self.arguments.len() != 1 {
                    return Err(format!(
                        "negative verifier profile {} may contain only --harness",
                        self.id
                    ));
                }
                if !self.measurements.is_empty() {
                    return Err(format!(
                        "negative verifier profile {} cannot require product measurements",
                        self.id
                    ));
                }
                if sidecar_byte_limit != 0 {
                    return Err(format!(
                        "negative verifier profile {} must have a zero sidecar byte limit",
                        self.id
                    ));
                }
                if !self.proof_requirements.is_empty() {
                    return Err(format!(
                        "negative verifier profile {} cannot require product proof",
                        self.id
                    ));
                }
            }
            value => {
                return Err(format!(
                    "verifier profile {} has unsupported --harness value {value}",
                    self.id
                ));
            }
        }

        if self.measurements.len() > MAX_PRODUCT_METRICS {
            return Err(format!(
                "verifier profile {} measurements exceed {MAX_PRODUCT_METRICS} entries",
                self.id
            ));
        }
        let mut measurements = BTreeSet::new();
        for metric in &self.measurements {
            if *metric == TimingMetric::AsyncProof {
                return Err("async-proof is implicit and must not be a product metric".to_owned());
            }
            if !measurements.insert(*metric) {
                return Err(format!(
                    "verifier profile {} duplicates measurement {}",
                    self.id,
                    metric_name(*metric)
                ));
            }
        }
        if harness == "timed" {
            let has_profile_benchmark = self.argument("--profile-benchmark-steps").is_some();
            if has_profile_benchmark != self.proof_requirements.budget.is_some() {
                return Err(format!(
                    "verifier profile {} must declare profile benchmark steps exactly when it declares budget proof",
                    self.id
                ));
            }
            let visible_mode = self.argument("--visible-mode").expect("required above");
            let alternate_target = self.argument("--alternate-target").expect("required above");
            if !matches!(visible_mode, "click" | "hover") {
                return Err(format!(
                    "verifier profile {} has unsupported visible mode {visible_mode}",
                    self.id
                ));
            }
            if !matches!(alternate_target, "none" | "any" | "same-source") {
                return Err(format!(
                    "verifier profile {} has unsupported alternate target {alternate_target}",
                    self.id
                ));
            }
            if (visible_mode == "click" && alternate_target != "none")
                || (visible_mode == "hover" && alternate_target == "none")
            {
                return Err(format!(
                    "verifier profile {} has an incompatible visible/alternate target policy",
                    self.id
                ));
            }
            let visible_samples = self.argument_usize("--visible-samples")?;
            let selection_samples = self.argument_usize("--selection-samples")?;
            let scroll_samples = self.argument_usize("--scroll-samples")?;
            let switch_samples = self.argument_usize("--switch-samples")?;
            if !(70..=256).contains(&visible_samples) {
                return Err(format!(
                    "verifier profile {} visible samples must be within 70..=256",
                    self.id
                ));
            }
            validate_optional_profile_samples(&self.id, "selection", selection_samples, 24, 128)?;
            validate_optional_profile_samples(&self.id, "scroll", scroll_samples, 140, 256)?;
            validate_optional_profile_samples(&self.id, "switch", switch_samples, 23, 64)?;
            if selection_samples > 0 && alternate_target == "none" {
                return Err(format!(
                    "verifier profile {} selection samples require an alternate target",
                    self.id
                ));
            }

            let mut expected = BTreeSet::from([
                TimingMetric::CallbackToHostEvent,
                TimingMetric::WarmVisibleInteraction,
            ]);
            if selection_samples > 0 {
                expected.insert(TimingMetric::RepeatedSelection);
            }
            if scroll_samples > 0 {
                expected.insert(TimingMetric::WarmScroll);
            }
            if switch_samples > 0 {
                expected.insert(TimingMetric::ExampleSwitchAcknowledgement);
                expected.insert(TimingMetric::ExampleSwitchFinalPreview);
            }
            if measurements != expected {
                return Err(format!(
                    "verifier profile {} measurements do not match its sampling arguments",
                    self.id
                ));
            }
        }
        self.proof_requirements.validate()?;
        Ok(())
    }

    fn argument_usize(&self, flag: &str) -> ValidationResult<usize> {
        let value = self
            .argument(flag)
            .expect("validated required profile argument");
        value.parse::<usize>().map_err(|error| {
            format!(
                "verifier profile {} has invalid {flag} value {value}: {error}",
                self.id
            )
        })
    }
}

fn validate_optional_profile_samples(
    profile: &BoundedId,
    label: &str,
    value: usize,
    minimum: usize,
    maximum: usize,
) -> ValidationResult<()> {
    if value != 0 && !(minimum..=maximum).contains(&value) {
        return Err(format!(
            "verifier profile {profile} {label} samples must be zero or within {minimum}..={maximum}"
        ));
    }
    Ok(())
}

impl ProfileProofRequirements {
    fn is_empty(&self) -> bool {
        self.scenario.is_none()
            && self.budget.is_none()
            && self.state_root.is_none()
            && self.native_workflow.is_none()
            && self.async_lanes.is_empty()
            && self.checkpoints.is_empty()
    }

    fn validate(&self) -> ValidationResult<()> {
        if let Some(requirement) = &self.scenario {
            validate_relative_extension(&requirement.path, "scn", "scenario")?;
        }
        if let Some(requirement) = &self.budget {
            validate_relative_extension(&requirement.path, "toml", "budget")?;
            if requirement.metrics.is_empty() || requirement.metrics.len() > MAX_BUDGET_METRICS {
                return Err(format!(
                    "budget proof metrics must contain 1..={MAX_BUDGET_METRICS} entries"
                ));
            }
            let mut metrics = BTreeSet::new();
            for metric in &requirement.metrics {
                if !metrics.insert(metric.as_str()) {
                    return Err(format!("budget proof duplicates metric {metric}"));
                }
            }
        }
        if let Some(requirement) = &self.native_workflow {
            if self.scenario.is_none() {
                return Err("native workflow requires a scenario proof".to_owned());
            }
            if requirement.delivery != NativeWorkflowDelivery::KernelUinputIsolatedSeat
                || requirement.scenario_boundary
                    != NativeWorkflowScenarioBoundary::KernelUinputAndSemanticAssertions
                || requirement.capture_method != CaptureMethod::AppOwnedRenderTargetReadback
                || requirement.durability != NativeWorkflowDurability::StateChangingStepsAcked
            {
                return Err(
                    "native workflow must require isolated kernel input, semantic assertions, production-target readback, and durable acknowledgements"
                        .to_owned(),
                );
            }
            if requirement.steps.is_empty() || requirement.steps.len() > MAX_PROFILE_CHECKPOINTS {
                return Err(format!(
                    "native workflow steps must contain 1..={MAX_PROFILE_CHECKPOINTS} entries"
                ));
            }
            let steps = requirement
                .steps
                .iter()
                .map(BoundedId::as_str)
                .collect::<BTreeSet<_>>();
            if steps.len() != requirement.steps.len()
                || requirement.proof_steps.is_empty()
                || requirement.proof_steps.len() > requirement.steps.len()
                || requirement
                    .proof_steps
                    .iter()
                    .any(|step| !steps.contains(step.as_str()))
                || requirement
                    .proof_steps
                    .iter()
                    .map(BoundedId::as_str)
                    .collect::<BTreeSet<_>>()
                    .len()
                    != requirement.proof_steps.len()
            {
                return Err(
                    "native workflow and proof steps must be unique, bounded, and proof steps must be a non-empty subset"
                        .to_owned(),
                );
            }
        }
        if self.async_lanes.len() > MAX_ASYNC_LANES {
            return Err(format!(
                "profile requires more than {MAX_ASYNC_LANES} async lanes"
            ));
        }
        let mut async_lanes = BTreeSet::new();
        for lane in &self.async_lanes {
            if !async_lanes.insert(*lane) {
                return Err(format!("profile duplicates async lane {}", lane.as_str()));
            }
        }
        if self.checkpoints.len() > MAX_PROFILE_CHECKPOINTS {
            return Err(format!(
                "profile checkpoints exceed {MAX_PROFILE_CHECKPOINTS} entries"
            ));
        }
        let mut checkpoints = BTreeSet::new();
        for checkpoint in &self.checkpoints {
            if !checkpoints.insert(checkpoint.id.as_str()) {
                return Err(format!("profile duplicates checkpoint {}", checkpoint.id));
            }
            match &checkpoint.evidence {
                CheckpointEvidenceRequirement::ScenarioStep { .. } if self.scenario.is_none() => {
                    return Err(format!(
                        "scenario checkpoint {} requires a scenario proof",
                        checkpoint.id
                    ));
                }
                CheckpointEvidenceRequirement::NativeWorkflowStep { scenario_step }
                    if self.native_workflow.as_ref().is_none_or(|workflow| {
                        !workflow
                            .proof_steps
                            .iter()
                            .any(|step| step == scenario_step)
                    }) =>
                {
                    return Err(format!(
                        "native workflow checkpoint {} must reference a declared proof step",
                        checkpoint.id
                    ));
                }
                CheckpointEvidenceRequirement::RestartRestore { .. }
                | CheckpointEvidenceRequirement::PersistenceOperation { .. }
                    if self.state_root.is_none() =>
                {
                    return Err(format!(
                        "durable checkpoint {} requires a state-root proof",
                        checkpoint.id
                    ));
                }
                CheckpointEvidenceRequirement::ResponsiveLayout { logical_width, .. }
                    if !(240..=1_920).contains(logical_width) =>
                {
                    return Err(format!(
                        "responsive checkpoint {} has unsupported logical width {}",
                        checkpoint.id, logical_width
                    ));
                }
                _ => {}
            }
        }
        for checkpoint in &self.checkpoints {
            let baseline = match &checkpoint.evidence {
                CheckpointEvidenceRequirement::RestartRestore {
                    baseline_checkpoint,
                }
                | CheckpointEvidenceRequirement::ResponsiveLayout {
                    baseline_checkpoint,
                    ..
                } => Some(baseline_checkpoint),
                _ => None,
            };
            if baseline.is_some_and(|baseline| !checkpoints.contains(baseline.as_str())) {
                return Err(format!(
                    "checkpoint {} references an undeclared baseline {}",
                    checkpoint.id,
                    baseline.expect("checked Some")
                ));
            }
        }
        Ok(())
    }
}

fn validate_kebab_identifier(value: &str, kind: &str) -> ValidationResult<()> {
    let bytes = value.as_bytes();
    let bounded_by_alphanumeric = bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    if !bounded_by_alphanumeric
        || bytes
            .iter()
            .any(|byte| !byte.is_ascii_lowercase() && !byte.is_ascii_digit() && *byte != b'-')
        || value.contains("--")
    {
        return Err(format!(
            "{kind} {value:?} must be a lowercase kebab-case identifier"
        ));
    }
    Ok(())
}

fn validate_profile_flag(flag: &BoundedString<64>) -> ValidationResult<()> {
    const RESERVED: [&str; 23] = [
        "--role",
        "--gate",
        "--evidence-output",
        "--artifact-dir",
        "--run-id",
        "--source-digest",
        "--profile",
        "--profile-digest",
        "--report",
        "--scenario-proof",
        "--require-semantic-scenario",
        "--budget-proof",
        "--required-budget-metrics",
        "--required-async-lanes",
        "--state-root-policy",
        "--restart-required",
        "--required-checkpoints",
        "--required-native-workflow-steps",
        "--required-native-workflow-proof-steps",
        "--native-workflow-delivery",
        "--native-workflow-scenario-boundary",
        "--native-workflow-capture-method",
        "--native-workflow-durability",
    ];
    let value = flag.as_str();
    if RESERVED.contains(&value) {
        return Err(format!(
            "verifier profile cannot override reserved argument {value}"
        ));
    }
    if !value.starts_with("--")
        || value.len() <= 2
        || !value[2..]
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(format!("invalid verifier profile argument flag {value}"));
    }
    Ok(())
}

fn validate_byte_limit(limit: u64) -> ValidationResult<()> {
    if limit == 0 || limit > MAX_REPORT_BYTES {
        return Err(format!(
            "report byte limit must be between 1 and {MAX_REPORT_BYTES}, found {limit}"
        ));
    }
    Ok(())
}

fn validate_sidecar_byte_limit(limit: u64) -> ValidationResult<()> {
    if limit > MAX_SIDECAR_BYTES {
        return Err(format!(
            "sidecar byte limit must be at most {MAX_SIDECAR_BYTES}, found {limit}"
        ));
    }
    Ok(())
}

fn validate_relative_extension(
    path: &RelativePath,
    extension: &str,
    kind: &str,
) -> ValidationResult<()> {
    validate_relative_path(path)?;
    if Path::new(path.as_str())
        .extension()
        .and_then(|value| value.to_str())
        != Some(extension)
    {
        return Err(format!("{kind} path {path} must end in .{extension}"));
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
    pub surface_id: ShortText,
    pub process_id: u32,
    pub session_id: ShortText,
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
        if self.process_id == 0 {
            return Err("frame evidence process_id must be non-zero".to_owned());
        }
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

    fn same_producer_surface(&self, other: &Self) -> bool {
        self.surface_id == other.surface_id
            && self.process_id == other.process_id
            && self.session_id == other.session_id
            && self.surface_epoch == other.surface_epoch
    }

    pub(crate) fn capture_token_digest(&self) -> Sha256Digest {
        let mut digest = Sha256::new();
        digest.update((self.surface_id.as_str().len() as u64).to_le_bytes());
        digest.update(self.surface_id.as_str().as_bytes());
        digest.update(self.process_id.to_le_bytes());
        digest.update((self.session_id.as_str().len() as u64).to_le_bytes());
        digest.update(self.session_id.as_str().as_bytes());
        for revision in [
            self.frame_id,
            self.input_id,
            self.content_id,
            self.layout_id,
            self.render_id,
            self.surface_epoch,
            self.present_id,
            self.proof_id,
        ] {
            digest.update(revision.to_le_bytes());
        }
        Sha256Digest::new(format!("{:x}", digest.finalize()))
            .expect("SHA-256 output is a valid digest")
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
    pub capture_method: CaptureMethod,
    pub capture_token_digest: Sha256Digest,
    pub nonblank_samples: u64,
    pub unique_rgba_values: u64,
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
        if self.nonblank_samples == 0 || self.unique_rgba_values <= 1 {
            return Err(format!(
                "artifact {} must contain nonblank, non-uniform app-owned pixels",
                self.path
            ));
        }
        if self.capture_token_digest != self.frame.capture_token_digest() {
            return Err(format!(
                "artifact {} capture token does not match its exact production frame",
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
    AppOwnedRenderTargetReadback,
}

impl CaptureMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AppOwnedRenderTargetReadback => "app-owned-render-target-readback",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchIsolationPhase {
    Primary,
    Restart,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchIsolationEvidence {
    pub phase: LaunchIsolationPhase,
    pub session_id: ShortText,
    pub seat_name: ShortText,
    pub pointer_device_owned: bool,
    pub keyboard_device_owned: bool,
    pub owned_device_count: u32,
    pub workspace_inactive: bool,
    pub mapped_surface_count: u32,
    pub tiling_enabled: bool,
    pub tiled_window_count: u32,
    pub floating_window_count: u32,
    pub maximized_window_count: u32,
    pub ownership_and_layout_preceded_input: bool,
}

impl LaunchIsolationEvidence {
    fn validate(&self) -> ValidationResult<()> {
        if !self.pointer_device_owned || !self.keyboard_device_owned || self.owned_device_count != 2
        {
            return Err(
                "native launch isolation requires exactly two owned input devices".to_owned(),
            );
        }
        if !self.workspace_inactive {
            return Err("native launch isolation workspace must remain inactive".to_owned());
        }
        if !self.tiling_enabled
            || self.tiled_window_count == 0
            || self.mapped_surface_count != self.tiled_window_count
            || self.floating_window_count != 0
            || self.maximized_window_count != 0
        {
            return Err(
                "native launch isolation requires every mapped role window to be tiled".to_owned(),
            );
        }
        if !self.ownership_and_layout_preceded_input {
            return Err(
                "native launch isolation must prove ownership and layout before input".to_owned(),
            );
        }
        Ok(())
    }
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
    pub launch_isolation: Vec<LaunchIsolationEvidence>,
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
        if self.launch_isolation.is_empty() || self.launch_isolation.len() > 2 {
            return Err(
                "passing native evidence requires one primary and at most one restart isolation record"
                    .to_owned(),
            );
        }
        let mut phases = BTreeSet::new();
        let mut sessions = BTreeSet::new();
        for isolation in &self.launch_isolation {
            isolation.validate()?;
            if !phases.insert(isolation.phase) {
                return Err("native launch isolation phases must be unique".to_owned());
            }
            if !sessions.insert(isolation.session_id.as_str()) {
                return Err("native launch isolation sessions must be unique".to_owned());
            }
        }
        if !phases.contains(&LaunchIsolationPhase::Primary) {
            return Err("native launch isolation is missing the primary phase".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TimingMetric {
    CallbackToHostEvent,
    WarmVisibleInteraction,
    RepeatedSelection,
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
    pub completed_after_frame: FrameEvidenceKey,
    pub proof_lag_frames: u32,
    pub artifact_id: BoundedId,
    pub snapshot_prepare_us: u64,
    pub worker_us: u64,
    pub summary: TimingSummary,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AsyncLaneKind {
    ChildProgramCompile,
    PersistenceTurn,
    ProgramArtifactStore,
    ProgramArtifactLoad,
    ProofReadback,
}

impl AsyncLaneKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChildProgramCompile => "child-program-compile",
            Self::PersistenceTurn => "persistence-turn",
            Self::ProgramArtifactStore => "program-artifact-store",
            Self::ProgramArtifactLoad => "program-artifact-load",
            Self::ProofReadback => "proof-readback",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AsyncLaneOutcome {
    Applied,
    StaleRejected,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AsyncLaneEvidence {
    pub lane: AsyncLaneKind,
    pub request_id: BoundedId,
    pub revision: u64,
    pub queue_depth: u32,
    pub queue_wait_us: u64,
    pub worker_us: u64,
    pub apply_us: u64,
    pub end_to_end_us: u64,
    pub outcome: AsyncLaneOutcome,
    pub frame: FrameEvidenceKey,
}

impl AsyncLaneEvidence {
    fn validate(&self) -> ValidationResult<()> {
        self.frame.validate()?;
        if self.revision == 0 || self.queue_depth == 0 {
            return Err(format!(
                "async lane {} requires a non-zero revision and bounded queue depth",
                self.lane.as_str()
            ));
        }
        let accounted = self
            .queue_wait_us
            .saturating_add(self.worker_us)
            .saturating_add(self.apply_us);
        if self.end_to_end_us < accounted {
            return Err(format!(
                "async lane {} end-to-end time does not account for queue, worker, and apply phases",
                self.lane.as_str()
            ));
        }
        Ok(())
    }
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScenarioBoundary {
    NativeTestPlayback,
    NativeTestPlaybackAndSemanticAssertions,
    KernelUinputWorkflowAndSemanticAssertions,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioProof {
    pub path: RelativePath,
    pub sha256: Sha256Digest,
    pub boundary: ScenarioBoundary,
    pub request_id: Option<u64>,
    pub declared_steps: u32,
    pub executable_steps: u32,
    pub completed_steps: u32,
    pub passed: bool,
    pub semantic_assertions_proven: bool,
}

impl ScenarioProof {
    fn validate(&self) -> ValidationResult<()> {
        validate_relative_extension(&self.path, "scn", "scenario proof")?;
        if self.executable_steps > self.declared_steps {
            return Err("scenario executable steps exceed declared steps".to_owned());
        }
        if self.completed_steps > self.executable_steps {
            return Err("scenario completed steps exceed executable steps".to_owned());
        }
        if self.passed
            && (self.request_id.is_none()
                || self.executable_steps == 0
                || self.completed_steps != self.executable_steps)
        {
            return Err(
                "passing scenario proof requires a request and every executable step".to_owned(),
            );
        }
        if self.semantic_assertions_proven
            != matches!(
                self.boundary,
                ScenarioBoundary::NativeTestPlaybackAndSemanticAssertions
                    | ScenarioBoundary::KernelUinputWorkflowAndSemanticAssertions
            )
        {
            return Err(
                "scenario semantic assertion claim does not match its proof boundary".to_owned(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BudgetUnit {
    Microseconds,
    Bytes,
    Count,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BudgetComparison {
    AtMost,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetObservation {
    pub metric: BoundedId,
    pub unit: BudgetUnit,
    pub comparison: BudgetComparison,
    pub observed: u64,
    pub limit: u64,
}

impl BudgetObservation {
    fn passes(&self) -> bool {
        match self.comparison {
            BudgetComparison::AtMost => self.observed <= self.limit,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetProof {
    pub path: RelativePath,
    pub sha256: Sha256Digest,
    pub observations: Vec<BudgetObservation>,
}

impl BudgetProof {
    fn validate(&self) -> ValidationResult<()> {
        validate_relative_extension(&self.path, "toml", "budget proof")?;
        if self.observations.len() > MAX_BUDGET_METRICS {
            return Err(format!(
                "budget proof observations exceed {MAX_BUDGET_METRICS} entries"
            ));
        }
        let mut metrics = BTreeSet::new();
        for observation in &self.observations {
            if !metrics.insert(observation.metric.as_str()) {
                return Err(format!(
                    "budget proof duplicates observation {}",
                    observation.metric
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StateRootProof {
    pub root: ShortText,
    pub policy: StateRootPolicy,
    pub clean_at_start: bool,
    pub durable_file_count: u32,
    pub restart_count: u32,
    pub restored_after_restart: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StateCheckpointProof {
    pub id: BoundedId,
    pub source_revision: u64,
    pub runtime_sequence: u64,
    pub durable_epoch: u64,
    pub durable_turn_sequence: u64,
    pub state_digest: Sha256Digest,
    pub frame: FrameEvidenceKey,
    #[serde(flatten)]
    pub evidence: StateCheckpointEvidence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "boundary", rename_all = "kebab-case")]
pub enum StateCheckpointEvidence {
    ScenarioSemanticFrame {
        scenario_step: BoundedId,
        assertion_count: u32,
    },
    RestartRestore {
        baseline_checkpoint: BoundedId,
        before_restart_digest: Sha256Digest,
        baseline_durable_epoch: u64,
        baseline_durable_turn_sequence: u64,
        baseline_frame: FrameEvidenceKey,
        process_replaced: bool,
        session_replaced: bool,
        first_observable_frame: bool,
        startup_restored: bool,
    },
    ResponsiveLayout {
        baseline_checkpoint: BoundedId,
        logical_width: u32,
        logical_height: u32,
        action_count: u32,
        action_digest: Sha256Digest,
    },
    StaleCompileRejection {
        session: BoundedId,
        stale_revision: u64,
        latest_revision: u64,
    },
    PersistenceOperation {
        operation: PersistenceEvidenceOperation,
        before_state_digest: Sha256Digest,
    },
    NativeWorkflowFrame {
        scenario_step: BoundedId,
        action_kind: NativeWorkflowActionKind,
        request_id: u64,
        action_digest: Sha256Digest,
        input_first_sequence: u64,
        input_last_sequence: u64,
        input_event_count: u32,
        input_event_digest: Sha256Digest,
        durable_turn_sequence: u64,
        durable_acked: bool,
        assertion_count: u32,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeWorkflowActionKind {
    AssertionOnly,
    Click,
    TypeText,
    DoubleClick,
    Key,
    Blur,
}

impl StateCheckpointProof {
    fn validate(&self) -> ValidationResult<()> {
        if self.source_revision == 0
            || self.runtime_sequence == 0
            || self.durable_epoch == 0
            || self.durable_turn_sequence == 0
        {
            return Err(format!(
                "state checkpoint {} requires non-zero source, runtime, and durable identities",
                self.id
            ));
        }
        self.frame.validate()?;
        if let StateCheckpointEvidence::RestartRestore { baseline_frame, .. } = &self.evidence {
            baseline_frame.validate()?;
        }
        match &self.evidence {
            StateCheckpointEvidence::ScenarioSemanticFrame {
                assertion_count, ..
            } if *assertion_count == 0 => {
                return Err(format!(
                    "scenario checkpoint {} proves no semantic assertions",
                    self.id
                ));
            }
            StateCheckpointEvidence::NativeWorkflowFrame {
                action_kind,
                request_id,
                input_first_sequence,
                input_last_sequence,
                input_event_count,
                durable_turn_sequence,
                durable_acked,
                assertion_count,
                ..
            } if *request_id == 0
                || *assertion_count == 0
                || *durable_turn_sequence == 0
                || !durable_acked
                || (*action_kind == NativeWorkflowActionKind::AssertionOnly
                    && (*input_first_sequence != 0
                        || *input_last_sequence != 0
                        || *input_event_count != 0))
                || (*action_kind != NativeWorkflowActionKind::AssertionOnly
                    && (*input_first_sequence == 0
                        || *input_last_sequence < *input_first_sequence
                        || *input_event_count == 0)) =>
            {
                return Err(format!(
                    "native workflow checkpoint {} lacks a valid action span, durable acknowledgement, or semantic assertions",
                    self.id
                ));
            }
            StateCheckpointEvidence::RestartRestore {
                before_restart_digest,
                baseline_durable_epoch,
                baseline_durable_turn_sequence,
                baseline_frame,
                process_replaced,
                session_replaced,
                first_observable_frame,
                startup_restored,
                ..
            } if !startup_restored
                || before_restart_digest != &self.state_digest
                || *baseline_durable_epoch == 0
                || *baseline_durable_turn_sequence == 0
                || self.durable_epoch < *baseline_durable_epoch
                || self.durable_turn_sequence < *baseline_durable_turn_sequence
                || !process_replaced
                || !session_replaced
                || !first_observable_frame
                || baseline_frame.process_id == self.frame.process_id
                || baseline_frame.session_id == self.frame.session_id =>
            {
                return Err(format!(
                    "restart checkpoint {} does not prove a new process restored the durable authority before its first frame",
                    self.id
                ));
            }
            StateCheckpointEvidence::ResponsiveLayout {
                logical_width,
                logical_height,
                action_count,
                ..
            } if !(240..=1_920).contains(logical_width)
                || !(320..=2_160).contains(logical_height)
                || *action_count == 0 =>
            {
                return Err(format!(
                    "responsive checkpoint {} has invalid layout evidence",
                    self.id
                ));
            }
            StateCheckpointEvidence::StaleCompileRejection {
                stale_revision,
                latest_revision,
                ..
            } if stale_revision >= latest_revision => {
                return Err(format!(
                    "stale checkpoint {} has non-increasing revisions",
                    self.id
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationProfileEvidence {
    pub profile_id: BoundedId,
    pub profile_digest: Sha256Digest,
    pub scenario: Option<ScenarioProof>,
    pub budget: Option<BudgetProof>,
    pub state_root: Option<StateRootProof>,
    pub native_workflow: Option<NativeWorkflowProof>,
    pub checkpoints: Vec<StateCheckpointProof>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWorkflowProof {
    pub input_delivery: InputDelivery,
    pub scenario_boundary: NativeWorkflowScenarioBoundary,
    pub test_request_id: u64,
    pub initial_state_digest: Sha256Digest,
    pub final_state_digest: Sha256Digest,
    pub ready_frame: FrameEvidenceKey,
    pub final_frame: FrameEvidenceKey,
    pub steps: Vec<NativeWorkflowStepProof>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWorkflowStepProof {
    pub request_id: u64,
    pub ordinal: u32,
    pub scenario_step: BoundedId,
    pub source_path: ShortText,
    pub action_kind: NativeWorkflowActionKind,
    pub action_digest: Sha256Digest,
    pub input_first_sequence: u64,
    pub input_last_sequence: u64,
    pub input_event_count: u32,
    pub input_event_digest: Sha256Digest,
    pub assertion_count: u32,
    pub source_revision: u64,
    pub runtime_sequence: u64,
    pub durable_epoch: u64,
    pub durable_turn_sequence: u64,
    pub durable_acked: bool,
    pub before_state_digest: Sha256Digest,
    pub state_digest: Sha256Digest,
    pub frame: FrameEvidenceKey,
}

impl NativeWorkflowProof {
    fn validate(&self) -> ValidationResult<()> {
        if self.input_delivery != InputDelivery::NativeOsAppWindowCallback
            || self.scenario_boundary
                != NativeWorkflowScenarioBoundary::KernelUinputAndSemanticAssertions
            || self.test_request_id == 0
            || self.steps.is_empty()
            || self.steps.len() > MAX_PROFILE_CHECKPOINTS
            || self.initial_state_digest == self.final_state_digest
        {
            return Err(
                "native workflow requires bounded real OS steps and a changed final authority"
                    .to_owned(),
            );
        }
        self.ready_frame.validate()?;
        self.final_frame.validate()?;
        if !self.ready_frame.same_producer_surface(&self.final_frame)
            || self.final_frame.frame_id < self.ready_frame.frame_id
        {
            return Err(
                "native workflow frames do not form one ordered producer surface".to_owned(),
            );
        }
        let mut ids = BTreeSet::new();
        let mut requests = BTreeSet::new();
        let mut previous_frame_id = self.ready_frame.frame_id;
        let mut previous_state_digest = self.initial_state_digest.clone();
        for (index, step) in self.steps.iter().enumerate() {
            if step.ordinal as usize != index + 1
                || step.request_id == 0
                || step.assertion_count == 0
                || step.source_revision == 0
                || step.runtime_sequence == 0
                || step.durable_epoch == 0
                || step.durable_turn_sequence == 0
                || !step.durable_acked
                || step.before_state_digest != previous_state_digest
                || !ids.insert(step.scenario_step.as_str())
                || !requests.insert(step.request_id)
            {
                return Err(
                    "native workflow steps are incomplete, duplicated, or unordered".to_owned(),
                );
            }
            let assertion_only = step.action_kind == NativeWorkflowActionKind::AssertionOnly;
            if assertion_only
                != (step.input_first_sequence == 0
                    && step.input_last_sequence == 0
                    && step.input_event_count == 0)
            {
                return Err(
                    "native workflow assertion-only and real-input spans are inconsistent"
                        .to_owned(),
                );
            }
            if !assertion_only
                && (step.input_first_sequence == 0
                    || step.input_last_sequence < step.input_first_sequence
                    || step.input_event_count == 0)
            {
                return Err("native workflow action has an invalid real-input span".to_owned());
            }
            step.frame.validate()?;
            if !self.ready_frame.same_producer_surface(&step.frame)
                || step.frame.frame_id <= previous_frame_id
            {
                return Err(
                    "native workflow steps must use distinct ordered producer frames".to_owned(),
                );
            }
            previous_frame_id = step.frame.frame_id;
            previous_state_digest = step.state_digest.clone();
        }
        if self.steps.last().is_none_or(|step| {
            step.state_digest != self.final_state_digest || step.frame != self.final_frame
        }) {
            return Err("native workflow completion does not match its final step".to_owned());
        }
        Ok(())
    }
}

impl VerificationProfileEvidence {
    fn validate_shape(&self) -> ValidationResult<()> {
        if let Some(scenario) = &self.scenario {
            scenario.validate()?;
        }
        if let Some(budget) = &self.budget {
            budget.validate()?;
        }
        if let Some(workflow) = &self.native_workflow {
            workflow.validate()?;
        }
        if self.checkpoints.len() > MAX_PROFILE_CHECKPOINTS {
            return Err(format!(
                "profile evidence checkpoints exceed {MAX_PROFILE_CHECKPOINTS} entries"
            ));
        }
        let mut checkpoints = BTreeSet::new();
        for checkpoint in &self.checkpoints {
            checkpoint.validate()?;
            if !checkpoints.insert(checkpoint.id.as_str()) {
                return Err(format!(
                    "profile evidence duplicates checkpoint {}",
                    checkpoint.id
                ));
            }
        }
        Ok(())
    }

    fn validate_for(
        &self,
        profile: &VerifierProfile,
        report_status: ReportStatus,
    ) -> ValidationResult<()> {
        self.validate_shape()?;
        if self.profile_id != profile.id || self.profile_digest != profile.digest() {
            return Err("verifier profile evidence identity mismatch".to_owned());
        }
        let require_complete = report_status == ReportStatus::Pass;
        let requirements = &profile.proof_requirements;

        if let Some(requirement) = &requirements.scenario {
            match &self.scenario {
                Some(proof) => {
                    if proof.path != requirement.path {
                        return Err("scenario proof path differs from verifier profile".to_owned());
                    }
                    if require_complete
                        && (!proof.passed
                            || proof.completed_steps != proof.executable_steps
                            || (requirement.semantic_assertions
                                && !proof.semantic_assertions_proven))
                    {
                        return Err(
                            "passing profile requires complete semantic scenario proof".to_owned()
                        );
                    }
                    if require_complete
                        && requirements.native_workflow.is_some()
                        && proof.boundary
                            != ScenarioBoundary::KernelUinputWorkflowAndSemanticAssertions
                    {
                        return Err(
                            "native workflow profile requires the kernel-uinput semantic scenario boundary"
                                .to_owned(),
                        );
                    }
                }
                None if require_complete => {
                    return Err("passing profile requires scenario proof".to_owned());
                }
                None => {}
            }
        } else if self.scenario.is_some() {
            return Err("profile evidence includes an undeclared scenario proof".to_owned());
        }

        if let Some(requirement) = &requirements.budget {
            match &self.budget {
                Some(proof) => {
                    if proof.path != requirement.path {
                        return Err("budget proof path differs from verifier profile".to_owned());
                    }
                    if require_complete {
                        for metric in &requirement.metrics {
                            let observation = proof
                                .observations
                                .iter()
                                .find(|observation| observation.metric == *metric)
                                .ok_or_else(|| format!("missing budget observation {metric}"))?;
                            if !observation.passes() {
                                return Err(format!(
                                    "budget observation {metric} exceeds its limit"
                                ));
                            }
                        }
                    }
                }
                None if require_complete => {
                    return Err("passing profile requires budget proof".to_owned());
                }
                None => {}
            }
        } else if self.budget.is_some() {
            return Err("profile evidence includes an undeclared budget proof".to_owned());
        }

        if let Some(requirement) = &requirements.state_root {
            match &self.state_root {
                Some(proof) if require_complete => {
                    if proof.policy != requirement.policy
                        || !proof.clean_at_start
                        || proof.durable_file_count == 0
                        || (requirement.restart_required
                            && (proof.restart_count == 0 || !proof.restored_after_restart))
                    {
                        return Err(
                            "passing profile requires clean launch-scoped durable state and restore proof"
                                .to_owned(),
                        );
                    }
                }
                Some(proof) if proof.policy != requirement.policy => {
                    return Err("state-root proof policy differs from verifier profile".to_owned());
                }
                Some(_) => {}
                None if require_complete => {
                    return Err("passing profile requires state-root proof".to_owned());
                }
                None => {}
            }
        } else if self.state_root.is_some() {
            return Err("profile evidence includes an undeclared state-root proof".to_owned());
        }

        if let Some(requirement) = &requirements.native_workflow {
            match &self.native_workflow {
                Some(proof) if require_complete => {
                    if proof.scenario_boundary != requirement.scenario_boundary
                        || proof.steps.len() != requirement.steps.len()
                        || proof
                            .steps
                            .iter()
                            .zip(&requirement.steps)
                            .any(|(observed, required)| observed.scenario_step != *required)
                    {
                        return Err(
                            "passing profile native workflow differs from the manifest order"
                                .to_owned(),
                        );
                    }
                }
                Some(_) => {}
                None if require_complete => {
                    return Err("passing profile requires native workflow proof".to_owned());
                }
                None => {}
            }
        } else if self.native_workflow.is_some() {
            return Err("profile evidence includes an undeclared native workflow".to_owned());
        }

        if require_complete {
            for required in &requirements.checkpoints {
                let proof = self
                    .checkpoints
                    .iter()
                    .find(|checkpoint| checkpoint.id == required.id)
                    .ok_or_else(|| format!("missing required state checkpoint {}", required.id))?;
                validate_checkpoint_requirement(required, proof)?;
            }
        }
        Ok(())
    }
}

fn validate_checkpoint_requirement(
    requirement: &CheckpointRequirement,
    proof: &StateCheckpointProof,
) -> ValidationResult<()> {
    let matches = match (&requirement.evidence, &proof.evidence) {
        (
            CheckpointEvidenceRequirement::ScenarioStep { scenario_step },
            StateCheckpointEvidence::ScenarioSemanticFrame {
                scenario_step: observed,
                ..
            },
        ) => scenario_step == observed,
        (
            CheckpointEvidenceRequirement::RestartRestore {
                baseline_checkpoint,
            },
            StateCheckpointEvidence::RestartRestore {
                baseline_checkpoint: observed,
                ..
            },
        ) => baseline_checkpoint == observed,
        (
            CheckpointEvidenceRequirement::ResponsiveLayout {
                baseline_checkpoint,
                logical_width,
            },
            StateCheckpointEvidence::ResponsiveLayout {
                baseline_checkpoint: observed,
                logical_width: observed_width,
                ..
            },
        ) => baseline_checkpoint == observed && logical_width == observed_width,
        (
            CheckpointEvidenceRequirement::StaleCompileRejection,
            StateCheckpointEvidence::StaleCompileRejection { .. },
        ) => true,
        (
            CheckpointEvidenceRequirement::PersistenceOperation { operation },
            StateCheckpointEvidence::PersistenceOperation {
                operation: observed,
                ..
            },
        ) => operation == observed,
        (
            CheckpointEvidenceRequirement::NativeWorkflowStep { scenario_step },
            StateCheckpointEvidence::NativeWorkflowFrame {
                scenario_step: observed,
                ..
            },
        ) => scenario_step == observed,
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(format!(
            "state checkpoint {} uses the wrong evidence boundary",
            requirement.id
        ))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GateEvidence {
    pub checks: Vec<CheckEvidence>,
    pub producer: Option<ProducerEvidence>,
    pub profile: Option<VerificationProfileEvidence>,
    pub native: Option<NativeEvidence>,
    pub product_ux_timings: Vec<ProductTimingEvidence>,
    pub async_proof_timing: Option<AsyncProofTimingEvidence>,
    #[serde(default)]
    pub async_lanes: Vec<AsyncLaneEvidence>,
    pub artifacts: Vec<ArtifactMetadata>,
}

impl GateEvidence {
    fn validate(
        &self,
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
        if self.async_lanes.len() > MAX_ASYNC_LANES {
            return Err(format!(
                "async lane evidence exceeds {MAX_ASYNC_LANES} entries"
            ));
        }
        let mut async_lanes = BTreeSet::new();
        for lane in &self.async_lanes {
            lane.validate()?;
            if !async_lanes.insert(lane.lane) {
                return Err(format!(
                    "duplicate async lane evidence {}",
                    lane.lane.as_str()
                ));
            }
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
        }
        if let Some(profile) = &self.profile {
            profile.validate_shape()?;
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

    fn validate_profile(
        &self,
        manifest_gate: &ManifestGate,
        report_status: ReportStatus,
    ) -> ValidationResult<()> {
        match manifest_gate.runner {
            GateRunner::Architecture if self.producer.is_some() => {
                return Err("architecture runner cannot carry producer metadata".to_owned());
            }
            GateRunner::NativeProduct
                if report_status == ReportStatus::Pass && self.producer.is_none() =>
            {
                return Err("passing process gate requires producer metadata".to_owned());
            }
            GateRunner::Architecture | GateRunner::NativeProduct => {}
        }
        let result = match (&manifest_gate.profile, &self.profile) {
            (None, None) => Ok(()),
            (None, Some(_)) => Err(format!(
                "{} report carries verifier profile evidence for a profile-less gate",
                manifest_gate.gate.slug()
            )),
            (Some(profile), Some(evidence)) => evidence.validate_for(profile, report_status),
            (Some(_), None) if report_status == ReportStatus::Pass => Err(format!(
                "passing {} report requires verifier profile evidence",
                manifest_gate.gate.slug()
            )),
            (Some(_), None) => Ok(()),
        };
        result?;
        if report_status == ReportStatus::Pass
            && let Some(profile) = &manifest_gate.profile
        {
            for required in &profile.proof_requirements.async_lanes {
                if !self.async_lanes.iter().any(|observed| {
                    observed.lane == *required && observed.outcome == AsyncLaneOutcome::Applied
                }) {
                    return Err(format!(
                        "passing {} report is missing applied request-level async lane {}",
                        manifest_gate.gate.slug(),
                        required.as_str()
                    ));
                }
            }
        }
        if report_status == ReportStatus::Pass
            && let Some(profile) = &self.profile
        {
            for checkpoint in &profile.checkpoints {
                if !self
                    .artifacts
                    .iter()
                    .any(|artifact| artifact.frame == checkpoint.frame)
                {
                    return Err(format!(
                        "state checkpoint {} has no matching app-owned WGPU readback artifact",
                        checkpoint.id
                    ));
                }
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
            proof.completed_after_frame.validate()?;
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
            if !proof
                .captured_frame
                .same_producer_surface(&proof.completed_after_frame)
                || proof.completed_after_frame.present_id < proof.captured_frame.present_id
            {
                return Err(
                    "async proof completion is not ordered on the captured production surface"
                        .to_owned(),
                );
            }
            let expected_completion = proof
                .captured_frame
                .frame_id
                .checked_add(u64::from(proof.proof_lag_frames))
                .ok_or_else(|| "async proof frame lag overflows".to_owned())?;
            if expected_completion != proof.completed_after_frame.frame_id {
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
            if self
                .native
                .as_ref()
                .is_some_and(|native| native.capture_method != artifact.capture_method)
            {
                return Err(
                    "native capture method does not match the linked proof artifact".to_owned(),
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
        self.evidence.validate(self.status, &self.measurement)?;
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
        let expected_measurement = measurement_contract(manifest_gate);
        if self.measurement != expected_measurement {
            return Err(format!(
                "{} report measurement contract differs from the manifest profile",
                manifest_gate.gate.slug()
            ));
        }
        self.evidence.validate_profile(manifest_gate, self.status)?;
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

    pub fn validate_artifacts(
        &self,
        workspace: &Path,
        sidecar_byte_limit: u64,
    ) -> ValidationResult<()> {
        let total_bytes = self
            .evidence
            .artifacts
            .iter()
            .try_fold(0_u64, |total, artifact| {
                total
                    .checked_add(artifact.byte_len)
                    .ok_or_else(|| "proof artifact byte total overflows".to_owned())
            })?;
        if total_bytes > sidecar_byte_limit {
            return Err(format!(
                "proof artifacts total {total_bytes} bytes; manifest sidecar limit is {sidecar_byte_limit}"
            ));
        }
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
        manifest_gate: &ManifestGate,
        run_id: &BoundedId,
        source: &SourceIdentity,
    ) -> ValidationResult<()> {
        if self.format != FORMAT_VERSION {
            return Err(format!(
                "producer evidence format must be {FORMAT_VERSION}, found {}",
                self.format
            ));
        }
        if self.gate != manifest_gate.gate {
            return Err(format!(
                "producer returned {} evidence for {}",
                self.gate.slug(),
                manifest_gate.gate.slug()
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
        self.evidence
            .validate_profile(manifest_gate, ReportStatus::Fail)?;
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
            return Err(format!(
                "aggregate must contain exactly {} gate results",
                handoff.gates.len()
            ));
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

pub fn measurement_contract(gate: &ManifestGate) -> MeasurementContract {
    let Some(profile) = &gate.profile else {
        return MeasurementContract::NotApplicable {
            reason: ShortText::new("structural or rejection gate has no product timing samples")
                .expect("static reason is bounded"),
        };
    };
    if profile.measurements.is_empty() {
        return MeasurementContract::NotApplicable {
            reason: ShortText::new("structural or rejection gate has no product timing samples")
                .expect("static reason is bounded"),
        };
    }

    let product_ux = profile
        .measurements
        .iter()
        .copied()
        .map(metric_definition_for)
        .collect();
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

fn metric_definition_for(metric: TimingMetric) -> MetricDefinition {
    match metric {
        TimingMetric::CallbackToHostEvent => metric_definition(
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
        TimingMetric::WarmVisibleInteraction => metric_definition(
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
        TimingMetric::RepeatedSelection => metric_definition(
            TimingMetric::RepeatedSelection,
            MetricBoundaryPoint::HostEventAccepted,
            MetricBoundaryPoint::FramePresented,
            16_700,
            Some(16_700),
            None,
            33_400,
            20,
            4,
        ),
        TimingMetric::WarmScroll => metric_definition(
            TimingMetric::WarmScroll,
            MetricBoundaryPoint::HostEventAccepted,
            MetricBoundaryPoint::FramePresented,
            16_700,
            Some(16_700),
            None,
            33_400,
            120,
            20,
        ),
        TimingMetric::ExampleSwitchAcknowledgement => metric_definition(
            TimingMetric::ExampleSwitchAcknowledgement,
            MetricBoundaryPoint::ExampleSwitchRequested,
            MetricBoundaryPoint::ExampleSwitchAcknowledged,
            16_700,
            Some(16_700),
            None,
            33_400,
            20,
            3,
        ),
        TimingMetric::ExampleSwitchFinalPreview => metric_definition(
            TimingMetric::ExampleSwitchFinalPreview,
            MetricBoundaryPoint::ExampleSwitchRequested,
            MetricBoundaryPoint::FramePresented,
            250_000,
            Some(250_000),
            None,
            500_000,
            20,
            3,
        ),
        TimingMetric::AsyncProof => {
            unreachable!("async proof has a dedicated measurement definition")
        }
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
        TimingMetric::RepeatedSelection => "repeated-selection",
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
        profile: None,
        native: None,
        product_ux_timings: Vec::new(),
        async_proof_timing: None,
        artifacts: Vec::new(),
        async_lanes: Vec::new(),
    }
}

pub fn gate_report(
    manifest_gate: &ManifestGate,
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
            report_id: make_report_id(&run_id, manifest_gate.gate.slug())?,
            run_id,
            gate: manifest_gate.gate.clone(),
            source: expected.source,
            tooling: expected.tooling,
            generated_unix_ms: unix_time_ms(),
        },
        status,
        measurement: measurement_contract(manifest_gate),
        evidence,
        blockers,
    };
    report.validate_shape()?;
    report
        .evidence
        .validate_profile(manifest_gate, report.status)?;
    Ok(report)
}

pub fn workspace_path(workspace: &Path, relative: &RelativePath) -> PathBuf {
    workspace.join(relative.as_str())
}
