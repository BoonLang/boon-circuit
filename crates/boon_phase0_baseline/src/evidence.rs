use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};

pub const DEFAULT_FIXTURE_EVIDENCE_MANIFEST: &str = "docs/architecture/phase0/fixtures.toml";
pub const DEFAULT_BASELINE_EVIDENCE_MANIFEST: &str = "docs/architecture/phase0/baselines.toml";
pub const REQUIRED_FIXTURE_IDS: &[&str] = &[
    "direct-out",
    "wrapped-out",
    "exact-arithmetic",
    "tags-presence-fault",
    "bits",
    "map-set",
    "typed-views",
    "proof-erasure",
    "nested-ownership",
    "effect-cancellation",
    "stale-routing",
    "visible-windows",
    "packed-scalar-row",
    "bounded-hardware-eligibility",
];
pub const REQUIRED_FIXTURE_DATASET_IDS: &[(&str, &[&str])] = &[
    ("direct-out", &["correctness.direct-out.v1"]),
    ("wrapped-out", &["correctness.wrapped-out.v1"]),
    (
        "exact-arithmetic",
        &[
            "correctness.exact-arithmetic-current.v1",
            "correctness.exact-integer-current.v1",
        ],
    ),
    (
        "tags-presence-fault",
        &["correctness.tags-presence-fault.v1"],
    ),
    ("bits", &["correctness.future-bits-rejection.v1"]),
    (
        "map-set",
        &[
            "correctness.future-map-rejection.v1",
            "correctness.future-set-rejection.v1",
        ],
    ),
    ("typed-views", &["correctness.typed-views.v1"]),
    (
        "proof-erasure",
        &[
            "correctness.future-where-rejection.v1",
            "correctness.proof-erasure-current.v1",
        ],
    ),
    ("nested-ownership", &["correctness.nested-ownership.v1"]),
    (
        "effect-cancellation",
        &["correctness.effect-cancellation.v1"],
    ),
    ("stale-routing", &["correctness.stale-routing.v1"]),
    ("visible-windows", &["performance.visible-window.v1"]),
    ("packed-scalar-row", &["performance.packed-scalar-row.v1"]),
    (
        "bounded-hardware-eligibility",
        &[
            "hardware.bounded-profile-analogue.v1",
            "hardware.future-bits-rejection.v1",
        ],
    ),
];
pub const REQUIRED_BASELINE_AREA_IDS: &[&str] = &[
    "allocations",
    "memory",
    "lookup-currentness-work",
    "native-wasm",
    "product-latency",
    "persistence",
];
const MAX_EVIDENCE_MANIFEST_BYTES: u64 = 512 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FixtureEvidenceManifestV2 {
    pub format_version: u16,
    pub harness: String,
    pub dataset_manifest: String,
    pub fixtures: Vec<FixtureEvidenceV2>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FixtureEvidenceV2 {
    pub id: String,
    pub classification: FixtureClassification,
    pub status: FixtureStatus,
    pub target_status: FixtureTargetStatus,
    pub evidence_status: FixtureEvidenceStatus,
    pub owner_phase: String,
    pub owner_plan: String,
    pub path: String,
    pub dataset_fixture_ids: Vec<String>,
    pub command: String,
    pub expected: String,
    pub replacement_seam: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureClassification {
    CurrentRegression,
    FutureLanguage,
    FutureList,
    FutureFormal,
    FutureRuntime,
    FutureProduct,
    FutureNative,
    FutureHardware,
}

impl FixtureClassification {
    fn is_future(self) -> bool {
        !matches!(self, Self::CurrentRegression)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureStatus {
    Existing,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureTargetStatus {
    CurrentSupported,
    CurrentAnalogue,
    NotYetImplemented,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureEvidenceStatus {
    CompileExecuteCurrent,
    CompileRejectFuture,
    CurrentExecutionPlusFutureRejection,
    HeadlessCurrentAnalogue,
    CurrentExecutionPlusMeasuredBaseline,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BaselineEvidenceManifestV2 {
    pub format_version: u16,
    pub baseline_source_head: String,
    pub baseline_scope: String,
    pub packed_report: String,
    pub packed_protocol: String,
    pub compiler: CompilerBaseline,
    pub historical_reports: Vec<HistoricalReport>,
    pub areas: Vec<BaselineArea>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CompilerBaseline {
    pub source_head: String,
    pub workspace_state: String,
    pub command: String,
    pub status: HistoricalStatus,
    pub total_passed: u64,
    pub crates: Vec<CompilerCrateBaseline>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CompilerCrateBaseline {
    pub name: String,
    pub passed: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HistoricalReport {
    pub id: String,
    pub path: String,
    pub status: HistoricalStatus,
    pub source_head: String,
    pub workspace_digest: String,
    pub dirty: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum HistoricalStatus {
    Pass,
    Stale,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BaselineArea {
    pub id: String,
    pub status: BaselineAreaStatus,
    pub scope: String,
    pub owner_phase: String,
    pub owner_plan: String,
    pub command: String,
    pub claim: String,
    pub evidence: Vec<BaselineEvidence>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum BaselineAreaStatus {
    Measured,
    Partial,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BaselineEvidence {
    pub id: String,
    pub status: BaselineEvidenceStatus,
    pub metric: String,
    #[serde(default)]
    pub value_u64: Option<u64>,
    #[serde(default)]
    pub value_text: Option<String>,
    pub unit: String,
    pub source: String,
    pub detail: String,
    #[serde(default)]
    pub required_change: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum BaselineEvidenceStatus {
    Measured,
    Unavailable,
    Stale,
    NotApplicable,
}

impl FixtureEvidenceManifestV2 {
    pub fn load(path: &Path) -> Result<Self, String> {
        let manifest: Self = load_toml(path)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate_workspace(&self, workspace: &Path) -> Result<(), String> {
        self.validate()?;
        require_workspace_file(workspace, &self.dataset_manifest, "dataset manifest")?;
        let datasets = crate::dataset::DatasetFixtureManifestV1::load(
            &workspace.join(&self.dataset_manifest),
        )?;
        datasets.verify_workspace(workspace)?;
        let dataset_ids = datasets
            .fixtures
            .iter()
            .map(|fixture| (fixture.id.as_str(), fixture.entrypoint.as_str()))
            .collect::<std::collections::BTreeMap<_, _>>();
        for fixture in &self.fixtures {
            require_workspace_file(workspace, &fixture.owner_plan, "fixture owner plan")?;
            require_workspace_file(workspace, &fixture.path, "fixture source")?;
            let mut covers_primary = false;
            for dataset_id in &fixture.dataset_fixture_ids {
                let entrypoint = dataset_ids.get(dataset_id.as_str()).ok_or_else(|| {
                    format!(
                        "fixture {} references unknown dataset fixture {}",
                        fixture.id, dataset_id
                    )
                })?;
                covers_primary |= **entrypoint == fixture.path;
            }
            if !covers_primary {
                return Err(format!(
                    "fixture {} primary source {} is not the entrypoint of a declared dataset fixture",
                    fixture.id, fixture.path
                ));
            }
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        if self.format_version != 2 {
            return Err(format!(
                "fixture evidence format {} is not 2",
                self.format_version
            ));
        }
        if self.harness != "boon_phase0_baseline::fixtures" {
            return Err(format!("unexpected fixture harness `{}`", self.harness));
        }
        safe_relative(&self.dataset_manifest)?;
        let ids = self
            .fixtures
            .iter()
            .map(|fixture| fixture.id.as_str())
            .collect::<Vec<_>>();
        if ids != REQUIRED_FIXTURE_IDS {
            return Err(format!(
                "fixture IDs/order differ: actual={ids:?}, expected={REQUIRED_FIXTURE_IDS:?}"
            ));
        }
        for fixture in &self.fixtures {
            safe_relative(&fixture.path)?;
            safe_relative(&fixture.owner_plan)?;
            if fixture.dataset_fixture_ids.is_empty() || fixture.dataset_fixture_ids.len() > 4 {
                return Err(format!(
                    "fixture {} must name 1..=4 dataset fixture IDs",
                    fixture.id
                ));
            }
            let mut previous = None;
            for dataset_id in &fixture.dataset_fixture_ids {
                require_bounded(dataset_id, 96, "fixture dataset ID")?;
                if previous.is_some_and(|previous: &str| previous >= dataset_id.as_str()) {
                    return Err(format!(
                        "fixture {} dataset fixture IDs are not unique and canonically ordered",
                        fixture.id
                    ));
                }
                previous = Some(dataset_id);
            }
            let expected_dataset_ids = REQUIRED_FIXTURE_DATASET_IDS
                .iter()
                .find_map(|(id, dataset_ids)| (*id == fixture.id).then_some(*dataset_ids))
                .ok_or_else(|| {
                    format!("fixture {} has no dataset identity contract", fixture.id)
                })?;
            if fixture
                .dataset_fixture_ids
                .iter()
                .map(String::as_str)
                .ne(expected_dataset_ids.iter().copied())
            {
                return Err(format!(
                    "fixture {} dataset IDs differ from the required executable bundles",
                    fixture.id
                ));
            }
            require_bounded(&fixture.owner_phase, 96, "fixture owner phase")?;
            require_bounded(&fixture.expected, 320, "fixture expected observation")?;
            require_bounded(&fixture.replacement_seam, 320, "fixture replacement seam")?;
            if !fixture
                .command
                .starts_with("cargo test -p boon_phase0_baseline ")
                || !fixture.command.ends_with(" --lib")
                || fixture.command.len() > 256
            {
                return Err(format!(
                    "fixture {} has a noncanonical executable command",
                    fixture.id
                ));
            }
            if fixture.classification.is_future()
                && fixture.target_status == FixtureTargetStatus::CurrentSupported
            {
                return Err(format!(
                    "future fixture {} cannot claim current target support",
                    fixture.id
                ));
            }
            if fixture.target_status == FixtureTargetStatus::NotYetImplemented
                && fixture.replacement_seam.is_empty()
            {
                return Err(format!(
                    "future fixture {} has no concrete replacement seam",
                    fixture.id
                ));
            }
        }
        Ok(())
    }
}

impl BaselineEvidenceManifestV2 {
    pub fn load(path: &Path) -> Result<Self, String> {
        let manifest: Self = load_toml(path)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate_workspace(&self, workspace: &Path) -> Result<(), String> {
        self.validate()?;
        for area in &self.areas {
            require_workspace_file(workspace, &area.owner_plan, "baseline owner plan")?;
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        if self.format_version != 2 {
            return Err(format!(
                "baseline evidence format {} is not 2",
                self.format_version
            ));
        }
        require_hex_digest(&self.baseline_source_head, "baseline source head", 40)?;
        require_bounded(&self.baseline_scope, 128, "baseline scope")?;
        safe_relative(&self.packed_report)?;
        if self.packed_protocol != "boon-phase0-current-runtime-v1" {
            return Err(format!(
                "unexpected packed protocol `{}`",
                self.packed_protocol
            ));
        }
        if self.compiler.status != HistoricalStatus::Pass
            || self.compiler.source_head != self.baseline_source_head
            || self.compiler.workspace_state != "clean"
            || self.compiler.total_passed != 405
        {
            return Err(
                "compiler baseline no longer identifies the frozen clean 405-test run".to_owned(),
            );
        }
        let compiler = self
            .compiler
            .crates
            .iter()
            .map(|entry| (entry.name.as_str(), entry.passed))
            .collect::<Vec<_>>();
        if compiler
            != [
                ("boon_parser", 24),
                ("boon_typecheck", 131),
                ("boon_ir", 103),
                ("boon_compiler", 147),
            ]
        {
            return Err(format!("unexpected compiler crate baseline {compiler:?}"));
        }
        let area_ids = self
            .areas
            .iter()
            .map(|area| area.id.as_str())
            .collect::<Vec<_>>();
        if area_ids != REQUIRED_BASELINE_AREA_IDS {
            return Err(format!(
                "baseline area IDs/order differ: actual={area_ids:?}, expected={REQUIRED_BASELINE_AREA_IDS:?}"
            ));
        }
        for report in &self.historical_reports {
            safe_relative(&report.path)?;
            require_hex_digest(&report.source_head, "historical source head", 40)?;
            require_hex_digest(&report.workspace_digest, "historical workspace digest", 64)?;
            if report.status != HistoricalStatus::Stale {
                return Err(format!(
                    "historical report {} is not explicitly stale",
                    report.id
                ));
            }
        }
        for area in &self.areas {
            safe_relative(&area.owner_plan)?;
            require_bounded(&area.scope, 128, "baseline area scope")?;
            require_bounded(&area.owner_phase, 96, "baseline area owner phase")?;
            require_bounded(&area.command, 1024, "baseline area command")?;
            require_bounded(&area.claim, 320, "baseline area claim")?;
            let uses_packed_report = area.evidence.iter().any(|evidence| {
                evidence
                    .source
                    .strip_prefix(&self.packed_report)
                    .is_some_and(|suffix| suffix.starts_with('#'))
            });
            if uses_packed_report
                && !area
                    .command
                    .starts_with("cargo xtask verify-packed-baseline --check-existing")
            {
                return Err(format!(
                    "baseline area {} does not use the registered strict packed verifier command",
                    area.id
                ));
            }
            if area.evidence.is_empty() || area.evidence.len() > 32 {
                return Err(format!(
                    "baseline area {} must have 1..=32 evidence rows",
                    area.id
                ));
            }
            let mut ids = BTreeSet::new();
            let mut has_gap = false;
            for evidence in &area.evidence {
                if !ids.insert(evidence.id.as_str()) {
                    return Err(format!(
                        "baseline area {} repeats evidence {}",
                        area.id, evidence.id
                    ));
                }
                require_bounded(&evidence.metric, 128, "baseline evidence metric")?;
                require_bounded(&evidence.unit, 64, "baseline evidence unit")?;
                require_bounded(&evidence.source, 320, "baseline evidence source")?;
                require_bounded(&evidence.detail, 384, "baseline evidence detail")?;
                if evidence.value_u64.is_some() && evidence.value_text.is_some() {
                    return Err(format!("baseline evidence {} has two values", evidence.id));
                }
                match evidence.status {
                    BaselineEvidenceStatus::Measured => {
                        let report_backed = evidence
                            .source
                            .strip_prefix(&self.packed_report)
                            .is_some_and(|suffix| suffix.starts_with('#'));
                        if report_backed {
                            if evidence.value_u64.is_some() || evidence.value_text.is_some() {
                                return Err(format!(
                                    "report-backed baseline evidence {} duplicates a generated value in the tracked manifest",
                                    evidence.id
                                ));
                            }
                        } else if evidence.value_u64.is_none() && evidence.value_text.is_none() {
                            return Err(format!(
                                "non-report measured baseline evidence {} has no exact value",
                                evidence.id
                            ));
                        }
                        if evidence.required_change.is_some() {
                            return Err(format!(
                                "measured baseline evidence {} has a required change",
                                evidence.id
                            ));
                        }
                    }
                    BaselineEvidenceStatus::Unavailable | BaselineEvidenceStatus::Stale => {
                        has_gap = true;
                        if evidence.status == BaselineEvidenceStatus::Unavailable
                            && evidence
                                .required_change
                                .as_deref()
                                .is_none_or(str::is_empty)
                        {
                            return Err(format!(
                                "unavailable baseline evidence {} has no required change",
                                evidence.id
                            ));
                        }
                    }
                    BaselineEvidenceStatus::NotApplicable => {}
                }
            }
            match area.status {
                BaselineAreaStatus::Measured if has_gap => {
                    return Err(format!(
                        "measured baseline area {} contains unavailable or stale evidence",
                        area.id
                    ));
                }
                BaselineAreaStatus::Partial if !has_gap => {
                    return Err(format!(
                        "partial baseline area {} has no explicit unavailable or stale edge",
                        area.id
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn load_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.len() == 0 || metadata.len() > MAX_EVIDENCE_MANIFEST_BYTES {
        return Err(format!(
            "{} is {} bytes; expected 1..={MAX_EVIDENCE_MANIFEST_BYTES}",
            path.display(),
            metadata.len()
        ));
    }
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    toml::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn require_workspace_file(workspace: &Path, relative: &str, label: &str) -> Result<(), String> {
    safe_relative(relative)?;
    let path = workspace.join(relative);
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("{label} {} is not a file", path.display()))
    }
}

fn safe_relative(value: &str) -> Result<(), String> {
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        Err(format!(
            "evidence path `{value}` is not a safe relative path"
        ))
    } else {
        Ok(())
    }
}

fn require_bounded(value: &str, maximum: usize, label: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > maximum || value.trim() != value {
        Err(format!(
            "{label} length {} or surrounding whitespace is invalid; expected 1..={maximum}",
            value.len()
        ))
    } else {
        Ok(())
    }
}

fn require_hex_digest(value: &str, label: &str, length: usize) -> Result<(), String> {
    if value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(format!(
            "{label} must be {length} lowercase hexadecimal characters"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .to_path_buf()
    }

    #[test]
    fn fixture_evidence_manifest_is_complete_and_honest() {
        let workspace = workspace();
        let manifest =
            FixtureEvidenceManifestV2::load(&workspace.join(DEFAULT_FIXTURE_EVIDENCE_MANIFEST))
                .unwrap();
        manifest.validate_workspace(&workspace).unwrap();
    }

    #[test]
    fn fixture_evidence_rejects_a_missing_companion_dataset() {
        let workspace = workspace();
        let mut manifest =
            FixtureEvidenceManifestV2::load(&workspace.join(DEFAULT_FIXTURE_EVIDENCE_MANIFEST))
                .unwrap();
        manifest
            .fixtures
            .iter_mut()
            .find(|fixture| fixture.id == "map-set")
            .unwrap()
            .dataset_fixture_ids
            .pop();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn baseline_evidence_manifest_has_no_blanket_unknown_area() {
        let workspace = workspace();
        let manifest =
            BaselineEvidenceManifestV2::load(&workspace.join(DEFAULT_BASELINE_EVIDENCE_MANIFEST))
                .unwrap();
        manifest.validate_workspace(&workspace).unwrap();
        assert_eq!(manifest.areas.len(), REQUIRED_BASELINE_AREA_IDS.len());
    }

    #[test]
    fn baseline_evidence_rejects_an_unregistered_packed_command() {
        let workspace = workspace();
        let mut manifest =
            BaselineEvidenceManifestV2::load(&workspace.join(DEFAULT_BASELINE_EVIDENCE_MANIFEST))
                .unwrap();
        manifest
            .areas
            .iter_mut()
            .find(|area| area.id == "allocations")
            .unwrap()
            .command = "cargo xtask packed-baseline --release --check-existing".to_owned();
        assert!(manifest.validate().is_err());
    }
}
